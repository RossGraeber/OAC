#!/usr/bin/env node
// Enforce that docs/planning/gates/fixtures/MANIFEST.json (D6, issue #39 T1) stays in
// sync with the fixtures actually committed under docs/planning/gates/fixtures/.
//
//   node scripts/check-fixture-manifest.mjs              # check this repository
//   node scripts/check-fixture-manifest.mjs --root <dir> # check another git work tree
//   node scripts/check-fixture-manifest.mjs --self-test  # plant violations, assert each fails
//
// Checks: MANIFEST.json parses as JSON; every entry's `path` is inside the fixtures
// tree and exists on disk; every file tracked by git under the fixtures tree (except
// MANIFEST.json itself) has exactly one manifest entry; every entry carries the
// required fields (schema is additionally required for a Codex-touching entry).
//
// herdr-driven fixtures (Epic K, K5 #128; rules in docs/planning/gates/README.md
// "Scripted runs (herdr)"): a fixture whose file name carries the `-herdr` suffix
// (`<kind>-<YYYY-MM-DD>-<version>-herdr.<ext>`) must carry a `driver` block with
// `herdr_version`, `driver_commit` (full 40-hex commit) and `run_manifest` (a committed
// `docs/planning/gates/herdr-runs/*.run-manifest.json`). That run manifest must parse,
// record outcome PASS, and name the same driver commit and `herdr --version` output as
// the block. A `driver` block on a fixture without the suffix is a violation (the suffix
// and the block label a herdr run together), and an `unverified-*-herdr.*` capture is
// never committed.
//
// Exits non-zero on any violation so CI fails loudly.

import { readFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { execFileSync, spawnSync } from 'node:child_process';
import { join, dirname, resolve, posix } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const scriptPath = fileURLToPath(import.meta.url);
const fixturesDir = 'docs/planning/gates/fixtures';
const manifestRelPath = `${fixturesDir}/MANIFEST.json`;
const herdrRunsDir = 'docs/planning/gates/herdr-runs';

// Every entry must carry these top-level fields (value may legitimately be `null`,
// e.g. capture_utc_range for a fixture with no wall-clock timestamps — the key must
// still be present so a reviewer sees the field was considered, not omitted).
const REQUIRED_KEYS = [
  'path',
  'provider',
  'surface',
  'observed_version',
  'pins_row',
  'pins_as_of',
  'version_matches_pin',
  'capture_date',
  'capture_utc_range',
  'superseded_by',
  'redaction',
  'coverage',
];
const REQUIRED_REDACTION_KEYS = ['script', 'script_sha256', 'residual_scan_result'];
const REQUIRED_DRIVER_KEYS = ['herdr_version', 'driver_commit', 'run_manifest'];

// `-herdr` immediately before the extension (or at the end of an extensionless name).
const HERDR_SUFFIX = /-herdr(?:\.[^/]*)?$/;
const UNVERIFIED_PREFIX = /^unverified-/;

function isHerdrFixture(path) {
  return HERDR_SUFFIX.test(posix.basename(path));
}

function isCodexTouching(entry) {
  return typeof entry.provider === 'string' && entry.provider.includes('codex');
}

// A Codex-touching entry's `schema` block (T7, issue #39) must be one of three shapes:
//  1. { applicable: false, reason } -- Codex appears only as an outbound MCP client, not
//     the app-server protocol this schema describes (e.g. g4-mcp-dual-era entries).
//  2. The 0.154.0-superseded-baseline shape: { commit, hash_method, upstream: null,
//     local_generation: null, sha256: null, note, validated_against } -- schema was never
//     regenerated at that fixture's own (pre-floating-pin) observed version, so it must
//     NOT carry a 0.157.1 hash.
//  3. The full shape: { commit, hash_method, upstream: {path, file_count, tree_sha256,
//     verified: {date, method, result}}, local_generation: {cli, default: {command,
//     file_count, tree_sha256}, experimental: {command, file_count, tree_sha256, note}},
//     validated_against }.
function schemaShapeProblem(schema) {
  if (!schema || typeof schema !== 'object') return 'schema is missing or not an object';
  if (schema.applicable === false) {
    return typeof schema.reason === 'string' ? null : 'applicable:false schema block is missing `reason`';
  }
  if (typeof schema.commit !== 'string') return 'schema.commit is missing';
  if (typeof schema.hash_method !== 'string') return 'schema.hash_method is missing';
  if (schema.upstream === null && schema.local_generation === null) {
    // shape 2: superseded-baseline, schema not regenerated at this version.
    if (schema.sha256 !== null) return 'superseded-baseline schema.sha256 must be null (schema not regenerated at this version)';
    if (typeof schema.note !== 'string') return 'superseded-baseline schema is missing `note`';
    return null;
  }
  // shape 3: full comparison record.
  const u = schema.upstream;
  if (!u || typeof u.path !== 'string' || typeof u.file_count !== 'number' || typeof u.tree_sha256 !== 'string') {
    return 'schema.upstream is missing path/file_count/tree_sha256';
  }
  if (!u.verified || typeof u.verified.date !== 'string' || typeof u.verified.method !== 'string' || typeof u.verified.result !== 'string') {
    return 'schema.upstream.verified is missing date/method/result';
  }
  const lg = schema.local_generation;
  if (!lg || typeof lg.cli !== 'string') return 'schema.local_generation is missing `cli`';
  for (const tier of ['default', 'experimental']) {
    const t = lg[tier];
    if (!t || typeof t.command !== 'string' || typeof t.file_count !== 'number' || typeof t.tree_sha256 !== 'string') {
      return `schema.local_generation.${tier} is missing command/file_count/tree_sha256`;
    }
  }
  if (typeof schema.validated_against !== 'string') return 'schema.validated_against is missing';
  return null;
}

// The `driver` block of a `-herdr` fixture entry (K5). Returns a list of problems.
// The run manifest is the driver's own `run-manifest.json` (tools/herdr/run.mjs), committed
// beside the herdr-runs record; the block must agree with it.
function driverBlockProblems(driver, root) {
  if (driver === undefined) return ['is a `-herdr` fixture but has no `driver` block (herdr_version, driver_commit, run_manifest)'];
  if (!driver || typeof driver !== 'object' || Array.isArray(driver)) return ['`driver` must be an object with herdr_version, driver_commit, run_manifest'];
  const out = [];
  for (const key of REQUIRED_DRIVER_KEYS) {
    if (typeof driver[key] !== 'string' || !driver[key].trim()) out.push(`driver.${key} is missing or empty`);
  }
  if (out.length) return out;
  if (!/^herdr \S+$/.test(driver.herdr_version)) out.push(`driver.herdr_version must be the verbatim \`herdr --version\` output ("herdr <version>"), got ${JSON.stringify(driver.herdr_version)}`);
  if (!/^[0-9a-f]{40}$/.test(driver.driver_commit)) out.push('driver.driver_commit must be a full 40-hex commit');
  const rm = driver.run_manifest;
  if (!rm.startsWith(`${herdrRunsDir}/`) || !rm.endsWith('.run-manifest.json') || rm.split('/').includes('..')) {
    out.push(`driver.run_manifest must be a ${herdrRunsDir}/<name>.run-manifest.json path`);
  }
  // Cross-check against the run manifest only once the block itself is well formed.
  if (out.length) return out;
  const abs = join(root, rm);
  if (!existsSync(abs)) {
    out.push(`driver.run_manifest ${rm} not found on disk`);
    return out;
  }
  let run;
  try {
    run = JSON.parse(readFileSync(abs, 'utf8'));
  } catch (err) {
    out.push(`driver.run_manifest ${rm} does not parse as JSON — ${err.message}`);
    return out;
  }
  if (run?.outcome !== 'PASS') out.push(`driver.run_manifest records outcome ${JSON.stringify(run?.outcome ?? null)}; only a PASS run's captures are committed as fixtures`);
  if (run?.driver?.commit !== driver.driver_commit) out.push('driver.driver_commit does not match the run manifest\'s driver.commit');
  if (run?.herdr?.observedVersionOutput !== driver.herdr_version) out.push('driver.herdr_version does not match the run manifest\'s herdr.observedVersionOutput');
  return out;
}

function checkManifest(root) {
  const manifestPath = join(root, manifestRelPath);
  const problems = [];

  if (!existsSync(manifestPath)) return { fatal: `FAIL  ${manifestRelPath}: missing` };

  let manifest;
  try {
    manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  } catch (err) {
    return { fatal: `FAIL  ${manifestRelPath}: does not parse as JSON — ${err.message}` };
  }

  if (!Array.isArray(manifest.fixtures)) {
    problems.push('MANIFEST.json: top-level `fixtures` array is missing');
  }

  const entries = manifest.fixtures ?? [];
  const manifestPaths = new Map();
  let herdrEntries = 0;

  for (const entry of entries) {
    if (!entry.path) {
      problems.push('an entry has no `path` field');
      continue;
    }
    const label = entry.path;

    if (manifestPaths.has(entry.path)) {
      problems.push(`${label}: duplicate manifest entry`);
    }
    manifestPaths.set(entry.path, (manifestPaths.get(entry.path) ?? 0) + 1);

    if (!entry.path.startsWith(`${fixturesDir}/`)) {
      problems.push(`${label}: path is outside ${fixturesDir}/`);
    }
    if (entry.path === manifestRelPath) {
      problems.push(`${label}: an entry must not describe MANIFEST.json itself`);
    }
    if (!existsSync(join(root, entry.path))) {
      problems.push(`${label}: listed in MANIFEST.json but not found on disk`);
    }

    for (const key of REQUIRED_KEYS) {
      if (!(key in entry)) problems.push(`${label}: missing required field \`${key}\``);
    }
    if (entry.redaction && typeof entry.redaction === 'object') {
      for (const key of REQUIRED_REDACTION_KEYS) {
        if (!(key in entry.redaction)) problems.push(`${label}: redaction is missing \`${key}\``);
      }
    } else if ('redaction' in entry) {
      problems.push(`${label}: \`redaction\` must be an object with ${REQUIRED_REDACTION_KEYS.join(', ')}`);
    }

    if (isCodexTouching(entry)) {
      if (!('schema' in entry)) {
        problems.push(`${label}: provider "${entry.provider}" is Codex-touching but has no \`schema\` block`);
      } else {
        const problem = schemaShapeProblem(entry.schema);
        if (problem) problems.push(`${label}: schema -- ${problem}`);
      }
    }

    if (isHerdrFixture(entry.path)) {
      herdrEntries += 1;
      if (UNVERIFIED_PREFIX.test(posix.basename(entry.path))) {
        problems.push(`${label}: an \`unverified-*\` herdr capture is never a fixture (its harness version was not verified against PINS.md)`);
      }
      for (const p of driverBlockProblems(entry.driver, root)) problems.push(`${label}: ${p}`);
    } else if ('driver' in entry) {
      problems.push(`${label}: has a \`driver\` block but its file name lacks the \`-herdr\` suffix; a herdr-driven fixture carries both`);
    }
  }

  let tracked;
  try {
    tracked = execFileSync('git', ['ls-files', fixturesDir], { cwd: root, encoding: 'utf8' })
      .split(/\r?\n/)
      .filter(Boolean);
  } catch (err) {
    return { fatal: `Could not run \`git ls-files ${fixturesDir}\`: ${err.message}` };
  }

  const trackedFixtureFiles = tracked.filter((f) => f !== manifestRelPath);

  for (const file of trackedFixtureFiles) {
    if (!manifestPaths.has(file)) {
      problems.push(`${file}: committed under ${fixturesDir}/ but has no MANIFEST.json entry`);
    }
  }

  return { problems, entries: entries.length, tracked: trackedFixtureFiles.length, herdrEntries };
}

function report(result) {
  if (result.fatal) {
    console.error(result.fatal);
    return 1;
  }
  if (result.problems.length > 0) {
    console.error(`\n${result.problems.length} fixture-manifest violation(s):`);
    for (const problem of result.problems) console.error(`  FAIL  ${problem}`);
    return 1;
  }
  console.log(
    `All ${result.entries} MANIFEST.json entries match the ${result.tracked} other committed fixture file(s)` +
      ` (${result.herdrEntries} \`-herdr\` entr${result.herdrEntries === 1 ? 'y' : 'ies'}, each with a checked \`driver\` block).`,
  );
  return 0;
}

// --- self-test -------------------------------------------------------------

// Each case builds a throwaway git work tree holding a MANIFEST.json, the fixture files it
// names, and (where a case needs one) a herdr run manifest; runs this script against it
// with --root; and asserts the exit code and, for a violation case, the one expected
// message. Every case also carries a human-run control entry with no `driver` block, so
// every case proves that entry raises no false positive.
const COMMIT = 'a'.repeat(40);
const HUMAN_FIXTURE = `${fixturesDir}/g1-claude-wake/transcript-2026-09-28-2.1.283-boxC.jsonl`;
const HERDR_FIXTURE = `${fixturesDir}/g1-claude-wake/transcript-2026-10-01-2.1.283-herdr.jsonl`;
const RUN_MANIFEST = `${herdrRunsDir}/G1-2026-10-01.run-manifest.json`;

function baseEntry(path) {
  return {
    path,
    provider: 'claude',
    surface: 'claude-channels',
    observed_version: { claude_code: '2.1.283' },
    pins_row: 'Claude Code (Channels)',
    pins_as_of: 'self-test',
    version_matches_pin: true,
    capture_date: '2026-10-01',
    capture_utc_range: null,
    superseded_by: null,
    redaction: { script: 'tools/herdr/lib/redact.mjs', script_sha256: 'x', residual_scan_result: 'self-test' },
    coverage: {},
  };
}
const DRIVER = { herdr_version: 'herdr 0.9.1', driver_commit: COMMIT, run_manifest: RUN_MANIFEST };
const RUN = { outcome: 'PASS', driver: { commit: COMMIT }, herdr: { observedVersionOutput: 'herdr 0.9.1' } };

function tree({ entries, run = RUN, extraFiles = [] }) {
  const files = { [manifestRelPath]: JSON.stringify({ fixtures: [baseEntry(HUMAN_FIXTURE), ...entries] }, null, 2) };
  for (const e of entries) files[e.path] = '{}\n';
  for (const f of extraFiles) files[f] = '{}\n';
  files[HUMAN_FIXTURE] = '{}\n';
  if (run) files[RUN_MANIFEST] = JSON.stringify(run);
  return files;
}
const herdrEntry = (driver, path = HERDR_FIXTURE) => ({ ...baseEntry(path), ...(driver === undefined ? {} : { driver }) });
const without = (k) => Object.fromEntries(Object.entries(DRIVER).filter(([key]) => key !== k));

const SELF_TEST_CASES = [
  { name: 'control: human-run entry only, no driver block', expect: 'pass', files: tree({ entries: [], run: null }) },
  { name: 'control: -herdr entry with a complete driver block matching its run manifest', expect: 'pass', files: tree({ entries: [herdrEntry(DRIVER)] }) },
  { name: '-herdr entry with no driver block', expect: 'has no `driver` block', files: tree({ entries: [herdrEntry(undefined)] }) },
  { name: 'driver is not an object', expect: '`driver` must be an object', files: tree({ entries: [herdrEntry('herdr 0.9.1')] }) },
  { name: 'driver.herdr_version missing', expect: 'driver.herdr_version is missing', files: tree({ entries: [herdrEntry(without('herdr_version'))] }) },
  { name: 'driver.driver_commit missing', expect: 'driver.driver_commit is missing', files: tree({ entries: [herdrEntry(without('driver_commit'))] }) },
  { name: 'driver.run_manifest missing', expect: 'driver.run_manifest is missing', files: tree({ entries: [herdrEntry(without('run_manifest'))] }) },
  { name: 'driver.herdr_version not `herdr --version` output', expect: 'driver.herdr_version must be', files: tree({ entries: [herdrEntry({ ...DRIVER, herdr_version: '0.9.1' })] }) },
  { name: 'driver.driver_commit abbreviated', expect: 'full 40-hex commit', files: tree({ entries: [herdrEntry({ ...DRIVER, driver_commit: 'aaaaaaa' })] }) },
  { name: 'driver.run_manifest outside herdr-runs/', expect: 'driver.run_manifest must be a', files: tree({ entries: [herdrEntry({ ...DRIVER, run_manifest: '/tmp/run-manifest.json' })] }) },
  { name: 'driver.run_manifest not committed', expect: 'not found on disk', files: tree({ entries: [herdrEntry(DRIVER)], run: null }) },
  { name: 'run manifest outcome NOT RUN', expect: 'only a PASS run', files: tree({ entries: [herdrEntry(DRIVER)], run: { ...RUN, outcome: 'NOT RUN' } }) },
  { name: 'run manifest driver commit differs', expect: 'driver_commit does not match', files: tree({ entries: [herdrEntry(DRIVER)], run: { ...RUN, driver: { commit: 'b'.repeat(40) } } }) },
  { name: 'run manifest herdr version differs', expect: 'herdr_version does not match', files: tree({ entries: [herdrEntry(DRIVER)], run: { ...RUN, herdr: { observedVersionOutput: 'herdr 0.9.2' } } }) },
  { name: 'driver block on a fixture without the -herdr suffix', expect: 'lacks the `-herdr` suffix', files: tree({ entries: [herdrEntry(DRIVER, `${fixturesDir}/g1-claude-wake/transcript-2026-10-01-2.1.283.jsonl`)] }) },
  { name: 'unverified-* herdr capture committed as a fixture', expect: 'is never a fixture', files: tree({ entries: [herdrEntry(DRIVER, `${fixturesDir}/g1-claude-wake/unverified-transcript-2026-10-01-herdr.jsonl`)] }) },
  { name: 'committed -herdr file with no manifest entry', expect: 'has no MANIFEST.json entry', files: tree({ entries: [], extraFiles: [HERDR_FIXTURE] }) },
];

function runSelfTest() {
  let failed = 0;
  for (const tc of SELF_TEST_CASES) {
    const dir = mkdtempSync(join(tmpdir(), 'oac-fixture-manifest-selftest-'));
    try {
      for (const [rel, content] of Object.entries(tc.files)) {
        mkdirSync(dirname(join(dir, rel)), { recursive: true });
        writeFileSync(join(dir, rel), content);
      }
      execFileSync('git', ['init', '-q'], { cwd: dir, stdio: 'ignore' });
      execFileSync('git', ['add', '-A'], { cwd: dir, stdio: 'ignore' });
      const run = spawnSync(process.execPath, [scriptPath, '--root', dir], { encoding: 'utf8' });
      const output = `${run.stdout}${run.stderr}`;
      const failLines = output.split('\n').filter((l) => l.trim().startsWith('FAIL  ')).length;
      const ok = tc.expect === 'pass' ? run.status === 0 : run.status === 1 && failLines === 1 && output.includes(tc.expect);
      if (!ok) failed += 1;
      console.log(`  ${ok ? 'ok  ' : 'FAIL'}  ${tc.name}  (exit ${run.status}${tc.expect === 'pass' ? '' : `, ${failLines} violation(s)`})`);
      if (!ok) console.log(output);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }
  const total = SELF_TEST_CASES.length;
  const violations = SELF_TEST_CASES.filter((tc) => tc.expect !== 'pass').length;
  console.log(
    `\nself-test: ${total - failed}/${total} cases as expected (${violations} planted violations must exit 1 with ` +
      `exactly the one expected violation; ${total - violations} controls must exit 0).`,
  );
  return failed ? 1 : 0;
}

// --- main ------------------------------------------------------------------

const argv = process.argv.slice(2);
if (argv.includes('--self-test')) {
  process.exit(runSelfTest());
}
let root = join(dirname(scriptPath), '..');
const rootIdx = argv.indexOf('--root');
if (rootIdx !== -1) {
  if (!argv[rootIdx + 1]) {
    console.error('usage: node scripts/check-fixture-manifest.mjs [--root <dir>] [--self-test]');
    process.exit(2);
  }
  root = resolve(argv[rootIdx + 1]);
}
process.exit(report(checkManifest(root)));
