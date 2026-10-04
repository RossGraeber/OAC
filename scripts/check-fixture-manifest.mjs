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
// (`<kind>-<YYYY-MM-DD>-<version>-herdr.<ext>`) must carry a boolean `version_matches_pin`
// and a `driver` block with `herdr_version`, `driver_commit` (full 40-hex commit) and
// `run_manifest` (a git-tracked `docs/planning/gates/herdr-runs/*.run-manifest.json`,
// with its `.md` record tracked beside it). That run manifest must parse, record
// outcome PASS, a clean `tools/herdr/` (`driver.toolsHerdrDirty: false`), the same driver
// commit and `herdr --version` output as the block, and list this fixture's file name
// among its captures as written. A `driver` block on a fixture without the suffix is a
// violation (the suffix and the block label a herdr run together), and an
// `unverified-*-herdr.*` capture is never committed.
//
// Executables and capture bytes (#140): a run manifest of schemaVersion 2 or later records
// the herdr executable the driver spawned and the sha256 of each written capture. Such a
// run's `-herdr` fixture is refused when the run used the node-run test-double herdr
// (`herdr.executable.testDouble` not false), when the herdr executable is not a hashed
// native binary (`format` elf/pe/mach-o, 64-hex `sha256`), or when the fixture's committed
// bytes (the git index blob; the work tree only if the file is not tracked) do not hash to
// the capture's recorded `sha256`, or (#252) when the driver compared the herdr hash with
// PINS.md's expected one (`herdr.executableCheck`) and it did not match. A schemaVersion 1
// run manifest predates #140: its fixtures are printed as a WARN line (nothing binds them
// mechanically). Any other schemaVersion is a violation. What is still NOT checked: that the
// harness that ran was real (harnessExecutables is recorded, not judged), and that a human
// accepted a consent dialog (a human action the record names; scripted-runs.md).
//
// Harness versions float and are never gated (operator decision on #216, 2026-10-01): a
// `-herdr` entry with `version_matches_pin: false` (its harness version is not PINS.md's
// last tested one) is printed as a WARN line and does not fail the check.
//
// herdr-run records: a tracked herdr-runs/*.md record that claims to be an equivalence
// record (`> **Equivalence record** for G<n> at herdr <tag>`), and a tracked
// G<n>-result.md whose `- **Driver:**` line names herdr, must carry a complete
// `## Verification` section (#252, verification completeness): herdr VERIFIED with its
// executable sha256, Harness VERIFIED, Dialogs, Human actions, Verified by + date, and no
// `<TO FILL` slot left. A record made before #252 may instead carry the complete
// `## Operator attestation` of that time (four lines; kept valid as history), but not when
// its run manifest was written by the #252 driver (`herdr.executableCheck` present). For an
// equivalence record whose run manifest (the `.run-manifest.json` beside it) is
// schemaVersion 2 or later, the stated herdr hash must equal the manifest's
// `herdr.executable.sha256` (#140); a verified record's manifest must also record
// `herdr.executableCheck.result` "match" against that hash (#252). The rest of each line is
// checked by the recording agent from the citations, not by this script.
//
// Exits non-zero on any violation so CI fails loudly.

import { readFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { join, dirname, resolve, posix } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

import { parseHerdrExpectedExecutables } from '../tools/herdr/lib/pins.mjs';

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
// Run-manifest schema versions (tools/herdr/lib/manifest.mjs MANIFEST_SCHEMA_VERSION): 1
// predates #140; 2 records the executables and capture hashes.
const LEGACY_RUN_SCHEMA = 1;
const IDENTITY_RUN_SCHEMA = 2;
// 3 (#252) records herdr.executableCheck; a schemaVersion 3 manifest without it is refused.
const CHECK_RUN_SCHEMA = 3;
const NATIVE_FORMATS = ['elf', 'pe', 'mach-o'];
const HEX64 = /^[0-9a-f]{64}$/;
const sha256 = (buf) => createHash('sha256').update(buf).digest('hex');

// `-herdr` immediately before the extension (or at the end of an extensionless name).
const HERDR_SUFFIX = /-herdr(?:\.[^/]*)?$/;
const UNVERIFIED_PREFIX = /^unverified-/;

// The verification an equivalence record or a verdict-bearing scripted run must carry
// (.claude/skills/oac-gates/references/scripted-runs.md "Verification"; operator decision
// on #252, 2026-10-03: verification from the evidence, not attestation). A record made
// before #252 may instead carry the complete `## Operator attestation` of that time; it
// stays valid history, but not for a run whose manifest the #252 driver wrote.
const EQUIVALENCE_CALLOUT = /^> \*\*Equivalence record\*\* for G\d+ at herdr /m;
const HERDR_DRIVER_LINE = /^- \*\*Driver:\*\* herdr\b/m;
const VERIFICATION_LINES = [
  ['herdr line (VERIFIED, with the executable sha256)', /^- \*\*herdr:\*\* VERIFIED\b.*\b[0-9a-f]{64}\b/m],
  ['Harness line (VERIFIED)', /^- \*\*Harness:\*\* VERIFIED\b/m],
  ['Dialogs line', /^- \*\*Dialogs:\*\* \S/m],
  ['Human actions line', /^- \*\*Human actions:\*\* \S/m],
  ['Verified by line (who, YYYY-MM-DD)', /^- \*\*Verified by:\*\* \S.*\b\d{4}-\d{2}-\d{2}\b/m],
];
const UNFILLED = '<TO FILL';
const ATTESTATION_LINES = [
  ['herdr line (real herdr, sha256 of its executable)', /^- \[x\] \*\*herdr:\*\* .*\b[0-9a-f]{64}\b/m],
  ['Harness line (real, logged-in harness)', /^- \[x\] \*\*Harness:\*\* \S/m],
  ['Consent dialog line', /^- \[x\] \*\*Consent dialog:\*\* \S/m],
  ['Attested by line (who, YYYY-MM-DD)', /^- \*\*Attested by:\*\* \S.*\b\d{4}-\d{2}-\d{2}\b/m],
];

// The body of a `## <heading>` section (up to the next `## `), or null when absent.
function sectionOf(text, heading) {
  const m = new RegExp(`^## ${heading}[ \\t]*$`, 'm').exec(text);
  if (!m) return null;
  const rest = text.slice(m.index + m[0].length);
  const next = /^## /m.exec(rest);
  return next ? rest.slice(0, next.index) : rest;
}

// -> { form: 'verification' | 'attestation' | null, problems }.
function verificationProblems(text) {
  const v = sectionOf(text, 'Verification');
  if (v !== null) {
    const problems = VERIFICATION_LINES.filter(([, re]) => !re.test(v)).map(([name]) => `verification is missing its ${name}`);
    if (v.includes(UNFILLED)) problems.push('verification still has an unfilled `<TO FILL: ...>` slot');
    return { form: 'verification', problems };
  }
  const a = sectionOf(text, 'Operator attestation');
  if (a !== null) return { form: 'attestation', problems: ATTESTATION_LINES.filter(([, re]) => !re.test(a)).map(([name]) => `operator attestation is missing its ${name}`) };
  return { form: null, problems: ['has no `## Verification` section (#252; a record made before #252 may carry a complete `## Operator attestation` instead)'] };
}

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
// The committed bytes of a file: the git index blob (what CI and a clone see, whatever the
// work tree's line endings), else the work-tree file when it is not tracked.
function committedBytes(root, path) {
  try {
    return execFileSync('git', ['cat-file', 'blob', `:${path}`], { cwd: root, stdio: ['ignore', 'pipe', 'ignore'], maxBuffer: 256 * 1024 * 1024 });
  } catch {
    try {
      return readFileSync(join(root, path));
    } catch {
      return null;
    }
  }
}

// #140: the herdr executable and the capture hash a schemaVersion >= 2 run manifest records.
function identityProblems(run, entry, root, name, warn) {
  const schema = run?.schemaVersion;
  if (schema === LEGACY_RUN_SCHEMA) {
    warn(`run manifest ${entry.driver.run_manifest} is schemaVersion 1 (before #140): which herdr and harness executables ran, and that this fixture is that run's capture byte for byte, are not recorded by the driver (UNVERIFIED; that record's operator attestation is pre-#252 history)`);
    return [];
  }
  if (!Number.isInteger(schema) || schema < IDENTITY_RUN_SCHEMA) return [`the run manifest's schemaVersion ${JSON.stringify(schema ?? null)} is not one this check knows (1, or 2 and later)`];
  const out = [];
  const x = run?.herdr?.executable;
  if (!x || x.testDouble !== false) {
    out.push(`the run manifest records ${x ? `herdr.executable.testDouble ${JSON.stringify(x.testDouble ?? null)}` : 'no herdr.executable'}: a test-double (or unrecorded) herdr run is never a fixture source`);
  } else if (!NATIVE_FORMATS.includes(x.format) || !HEX64.test(x.sha256 ?? '')) {
    out.push(`the run manifest's herdr.executable is not a hashed native binary (format ${JSON.stringify(x.format ?? null)}); a test-double run is never a fixture source`);
  } else if ((schema >= CHECK_RUN_SCHEMA || run?.herdr?.executableCheck) && run?.herdr?.executableCheck?.result !== 'match') {
    // #252: a driver that compares the hash with PINS.md's expected one (schemaVersion 3, or
    // any manifest carrying the check) must have recorded a match.
    out.push(`the run manifest's herdr.executableCheck.result is ${JSON.stringify(run?.herdr?.executableCheck?.result ?? null)}, not "match": a run whose herdr identity is UNVERIFIED is never a fixture source`);
  }
  const cap = (run?.captures ?? []).find((c) => c?.file === name && c?.written === true);
  if (cap) {
    if (!HEX64.test(cap.sha256 ?? '')) out.push(`the run manifest records no sha256 for capture ${name}`);
    else {
      const bytes = committedBytes(root, entry.path);
      if (!bytes) out.push(`cannot read the committed bytes of ${name} to check them against the run manifest's capture sha256`);
      else if (sha256(bytes) !== cap.sha256) out.push(`the committed bytes of ${name} differ from the run manifest's capture sha256; this file is not that run's capture`);
    }
  }
  return out;
}

function driverBlockProblems(entry, root, tracked, warn = () => {}) {
  const driver = entry.driver;
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
  if (!tracked.has(rm)) {
    out.push(`driver.run_manifest ${rm} is not committed (not tracked by git)`);
    return out;
  }
  const record = rm.replace(/\.run-manifest\.json$/, '.md');
  if (!tracked.has(record)) out.push(`the herdr-runs record ${record} is not committed beside its run manifest`);
  const abs = join(root, rm);
  let run;
  try {
    run = JSON.parse(readFileSync(abs, 'utf8'));
  } catch (err) {
    out.push(`driver.run_manifest ${rm} does not parse as JSON — ${err.message}`);
    return out;
  }
  if (run?.outcome !== 'PASS') out.push(`driver.run_manifest records outcome ${JSON.stringify(run?.outcome ?? null)}; only a PASS run's captures are committed as fixtures`);
  if (run?.driver?.toolsHerdrDirty !== false) out.push(`driver.run_manifest records driver.toolsHerdrDirty ${JSON.stringify(run?.driver?.toolsHerdrDirty ?? null)}; only a run from a clean, committed tools/herdr/ is committed as fixtures`);
  if (run?.driver?.commit !== driver.driver_commit) out.push('driver.driver_commit does not match the run manifest\'s driver.commit');
  if (run?.herdr?.observedVersionOutput !== driver.herdr_version) out.push('driver.herdr_version does not match the run manifest\'s herdr.observedVersionOutput');
  const name = posix.basename(entry.path);
  if (!(run?.captures ?? []).some((c) => c?.file === name && c?.written === true)) out.push(`the run manifest does not list ${name} among its written captures`);
  out.push(...identityProblems(run, entry, root, name, warn));
  return out;
}

// #252: everything a claiming record or gate result says about herdr is tied to a committed,
// parseable run manifest: an equivalence record's own (`<record>.run-manifest.json` beside
// it), a gate result's through the herdr-runs record its Driver line names. Then:
//  - Verification form: the manifest must be a #252 driver's (schemaVersion 3, or carrying
//    herdr.executableCheck), record a `match` against a first-party expected value, and that
//    value must be PINS.md's committed row for the platform (in the index, and at the run's
//    driver.commit, which must resolve in this repository or the file is refused; fetch full
//    history in a shallow clone). The stated herdr sha256 must be the manifest's.
//  - Attestation form (pre-#252 history): accepted only on a pre-#252 manifest (schemaVersion
//    <= 2 and no executableCheck), from a run the driver dated (timebox.start) no later than the #252 cut-off, and
//    only with the `> **Pre-#252 attestation (history).**` callout saying it is not a current
//    basis. Its herdr hash must still equal a schemaVersion 2 manifest's.
const PRE252_CALLOUT = /^> \*\*Pre-#252 attestation \(history\)\.\*\*/m;
const CUTOFF_252 = '2026-10-03';
const PINS_PATH = 'docs/planning/PINS.md';

function driverRecordOf(text) {
  const start = /^- \*\*Driver:\*\*/m.exec(text);
  if (!start) return null;
  const rest = text.slice(start.index + start[0].length);
  const end = /^(?:- \*\*|#)/m.exec(rest);
  const block = end ? rest.slice(0, end.index) : rest;
  const rec = /herdr-runs\/([A-Za-z0-9._-]+\.md)/.exec(block)?.[1];
  return rec ? `${herdrRunsDir}/${rec}` : null;
}

// PINS.md's expected herdr rows: rev null = the committed (index) file; otherwise at that
// commit. undefined when the commit is not in this repository (e.g. a shallow clone).
function pinsRowsAt(root, rev) {
  let bytes;
  if (rev === null) bytes = committedBytes(root, PINS_PATH);
  else {
    try {
      bytes = execFileSync('git', ['cat-file', 'blob', `${rev}:${PINS_PATH}`], { cwd: root, stdio: ['ignore', 'pipe', 'ignore'] });
    } catch {
      return undefined;
    }
  }
  if (!bytes) return null;
  try {
    return parseHerdrExpectedExecutables(bytes.toString('utf8'));
  } catch {
    return null;
  }
}

function evidenceProblems(text, file, root, tracked, form, isRecord) {
  let record = file;
  if (!isRecord) {
    record = driverRecordOf(text);
    if (!record) return ['its Driver line names no docs/planning/gates/herdr-runs/<record>.md, so nothing ties it to a run manifest'];
    if (!tracked.has(record)) return [`the record its Driver line names, ${record}, is not committed`];
  }
  const rm = record.replace(/\.md$/, '.run-manifest.json');
  if (!tracked.has(rm)) return [`its run manifest ${rm} is not committed, so nothing backs what it states`];
  let run;
  try {
    run = JSON.parse(readFileSync(join(root, rm), 'utf8'));
  } catch (err) {
    return [`its run manifest ${rm} does not parse as JSON — ${err.message}`];
  }
  const check = run?.herdr?.executableCheck;
  const schema = run?.schemaVersion;
  const by252 = (Number.isInteger(schema) && schema >= CHECK_RUN_SCHEMA) || !!check;
  const recorded = run?.herdr?.executable?.sha256;
  const out = [];
  if (form === 'attestation') {
    if (by252) return [`its run manifest ${rm} was written by the #252 driver, so it carries a \`## Verification\` section, not an operator attestation`];
    // The run's date is the one the driver recorded (timebox.start), never the file name.
    const start = run?.timebox?.start;
    const date = typeof start === 'string' && /^\d{4}-\d{2}-\d{2}T/.test(start) ? start.slice(0, 10) : null;
    if (!date) out.push(`its run manifest ${rm} records no timebox.start, so nothing shows the run predates #252; an operator attestation is accepted only for a run the driver dated on or before ${CUTOFF_252}`);
    else if (date > CUTOFF_252) out.push(`its run started ${date} (run manifest timebox.start), after #252 (${CUTOFF_252}), so it carries a \`## Verification\` section, not an operator attestation`);
    if (!PRE252_CALLOUT.test(text)) out.push('it carries a pre-#252 operator attestation without the `> **Pre-#252 attestation (history).**` callout that says it is not a current basis (#252)');
  } else {
    if (!by252) return [`its run manifest ${rm} predates #252 (no herdr.executableCheck), so herdr cannot be VERIFIED`];
    // The driver commit must be resolvable here, so PINS.md as the driver read it can be
    // checked; a shallow clone must fetch full history (the CI job uses fetch-depth: 0).
    const dc = run?.driver?.commit;
    if (!/^[0-9a-f]{40}$/.test(dc ?? '')) return [`its run manifest ${rm} records no full driver.commit, so PINS.md at the driver commit cannot be checked`];
    if (pinsRowsAt(root, dc) === undefined) return [`its run manifest's driver.commit ${dc} is not in this repository, so PINS.md at the driver commit cannot be checked; fetch full history (git fetch --unshallow) and re-run`];
    if (check?.result !== 'match' || !HEX64.test(recorded ?? '') || check?.expectedSha256 !== recorded) {
      out.push(`its run manifest ${rm} records no herdr.executableCheck match (result ${JSON.stringify(check?.result ?? null)}), so herdr cannot be VERIFIED`);
    } else if (check.firstParty !== true) {
      out.push(`its run manifest ${rm} records a match against a value that is not first-party (herdr.executableCheck.firstParty ${JSON.stringify(check.firstParty ?? null)}), so herdr cannot be VERIFIED`);
    } else {
      for (const [label, rev] of [['committed', null], [`at driver commit ${dc}`, dc]]) {
        const rows = pinsRowsAt(root, rev);
        const row = (rows ?? []).find((r) => r.platform === check.platform);
        if (!row || row.sha256 !== check.expectedSha256 || row.firstParty !== true) {
          out.push(`its run manifest's expected herdr sha256 for ${check.platform} is not PINS.md's first-party row (${label})`);
        }
      }
    }
  }
  if (!Number.isInteger(schema) || schema < IDENTITY_RUN_SCHEMA) return out;
  const lineRe = form === 'verification' ? /^- \*\*herdr:\*\* VERIFIED\b.*$/m : /^- \[x\] \*\*herdr:\*\* .*$/m;
  const stated = /\b([0-9a-f]{64})\b/.exec(lineRe.exec(text)?.[0] ?? '')?.[1];
  if (!stated) return out;
  if (!HEX64.test(recorded ?? '')) return [...out, `its run manifest ${rm} records no herdr executable sha256 to back the stated one`];
  if (stated !== recorded) out.push(`the herdr sha256 it states is not the one its run manifest ${rm} recorded (herdr.executable.sha256)`);
  return out;
}

function checkManifest(root) {
  const manifestPath = join(root, manifestRelPath);
  const problems = [];
  const warnings = [];

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

  let tracked;
  let trackedGates;
  try {
    const ls = (path) => execFileSync('git', ['ls-files', '--', path], { cwd: root, encoding: 'utf8' }).split(/\r?\n/).filter(Boolean);
    tracked = ls(fixturesDir);
    trackedGates = ls('docs/planning/gates');
  } catch (err) {
    return { fatal: `Could not run \`git ls-files\`: ${err.message}` };
  }
  const trackedGateSet = new Set(trackedGates);

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
        problems.push(`${label}: an \`unverified-*\` herdr capture is never a fixture (its harness sources did not report one and the same version)`);
      }
      if (typeof entry.version_matches_pin !== 'boolean') {
        problems.push(`${label}: a \`-herdr\` fixture's version_matches_pin must be true or false`);
      } else if (entry.version_matches_pin === false) {
        // #216: versions float; a version other than PINS.md's last tested one is a warning only.
        warnings.push(`${label}: VERSION WARNING: version_matches_pin is false (the harness version is not PINS.md's last tested one); versions float and are never gated (#216)${entry.version_matches_pin_note ? ` -- ${entry.version_matches_pin_note}` : ''}`);
      }
      for (const p of driverBlockProblems(entry, root, trackedGateSet, (w) => warnings.push(`${label}: ${w}`))) problems.push(`${label}: ${p}`);
    } else if ('driver' in entry) {
      problems.push(`${label}: has a \`driver\` block but its file name lacks the \`-herdr\` suffix; a herdr-driven fixture carries both`);
    }
  }

  const trackedFixtureFiles = tracked.filter((f) => f !== manifestRelPath);

  for (const file of trackedFixtureFiles) {
    if (!manifestPaths.has(file)) {
      problems.push(`${file}: committed under ${fixturesDir}/ but has no MANIFEST.json entry`);
    }
  }

  // herdr-run records and gate results that claim what only a verified run may claim.
  let verified = 0;
  for (const file of trackedGates) {
    const isRecord = file.startsWith(`${herdrRunsDir}/`) && file.endsWith('.md');
    const isResult = /^docs\/planning\/gates\/G\d+-result\.md$/.test(file);
    if (!isRecord && !isResult) continue;
    let text;
    try {
      text = readFileSync(join(root, file), 'utf8');
    } catch {
      continue; // tracked but deleted in the work tree; not this check's concern
    }
    const claims = isRecord ? EQUIVALENCE_CALLOUT.test(text) : HERDR_DRIVER_LINE.test(text);
    if (!claims) continue;
    verified += 1;
    const why = isRecord ? 'claims to be an equivalence record' : 'names herdr as its Driver';
    const v = verificationProblems(text);
    for (const p of v.problems) problems.push(`${file}: ${why} but ${p}`);
    if (v.form) for (const p of evidenceProblems(text, file, root, trackedGateSet, v.form, isRecord)) problems.push(`${file}: ${why} but ${p}`);
  }

  return { problems, warnings, entries: entries.length, tracked: trackedFixtureFiles.length, herdrEntries, verified };
}

function report(result) {
  if (result.fatal) {
    console.error(result.fatal);
    return 1;
  }
  for (const w of result.warnings ?? []) console.log(`  WARN  ${w}`);
  if (result.problems.length > 0) {
    console.error(`\n${result.problems.length} fixture-manifest violation(s):`);
    for (const problem of result.problems) console.error(`  FAIL  ${problem}`);
    return 1;
  }
  console.log(
    `All ${result.entries} MANIFEST.json entries match the ${result.tracked} other committed fixture file(s)` +
      ` (${result.herdrEntries} \`-herdr\` entr${result.herdrEntries === 1 ? 'y' : 'ies'}, each with a checked \`driver\` block;` +
      ` ${result.verified} herdr-run record(s) or gate result(s) needing a verification section, each complete).`,
  );
  return 0;
}

// --- self-test -------------------------------------------------------------

// Each case builds a throwaway git work tree holding a MANIFEST.json, the fixture files it
// names, and (where a case needs them) a herdr run manifest, its herdr-runs record, and a
// gate result; runs this script against it with --root; and asserts the exit code and,
// for a violation case, the one expected message. `untracked` files are written after
// `git add`, so they exist on disk but are not tracked. Every case also carries a
// human-run control entry with no `driver` block, so every case proves that entry raises
// no false positive.
const COMMIT = 'a'.repeat(40);
const HUMAN_FIXTURE = `${fixturesDir}/g1-claude-wake/transcript-2026-09-28-2.1.283-boxC.jsonl`;
const HERDR_FIXTURE = `${fixturesDir}/g1-claude-wake/transcript-2026-10-01-2.1.283-herdr.jsonl`;
const UNVERIFIED_FIXTURE = `${fixturesDir}/g1-claude-wake/unverified-transcript-2026-10-01-herdr.jsonl`;
const RUN_MANIFEST = `${herdrRunsDir}/G1-2026-10-01.run-manifest.json`;
const RECORD = `${herdrRunsDir}/G1-2026-10-01.md`;
const RESULT = 'docs/planning/gates/G1-result.md';

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
// Every planted fixture file holds FIXTURE_BYTES, so the RUN capture hashes match it.
const FIXTURE_BYTES = '{}\n';
const HERDR_SHA = 'c'.repeat(64);
const HERDR_EXE = { requested: 'herdr', resolved: true, basename: 'herdr', realBasename: 'herdr', sha256: HERDR_SHA, bytes: 1, format: 'elf', runUnderNode: false, testDouble: false };
const RUN = {
  schemaVersion: 2,
  outcome: 'PASS',
  driver: { commit: COMMIT, toolsHerdrDirty: false },
  timebox: { start: '2026-10-01T00:00:00.000Z' },
  herdr: { observedVersionOutput: 'herdr 0.9.1', executable: HERDR_EXE },
  captures: [posix.basename(HERDR_FIXTURE), posix.basename(UNVERIFIED_FIXTURE)].map((file) => ({ file, written: true, sha256: sha256(FIXTURE_BYTES) })),
};
const PRE252 = '> **Pre-#252 attestation (history).** Self-test: not a current basis (#252).\n';
const ATTESTATION_BODY = [
  '## Operator attestation',
  '',
  `- [x] **herdr:** the real herdr binary ran, not a test double. sha256 of the executable: \`${'c'.repeat(64)}\``,
  '- [x] **Harness:** the real, logged-in Claude Code CLI ran, not a test double.',
  '- [x] **Consent dialog:** accepted by me, a human at the keyboard, during this run.',
  '- **Attested by:** self-test operator, 2026-10-01',
  '',
].join('\n');
const ATTESTATION = `${PRE252}\n${ATTESTATION_BODY}`;
const EQUIV = '> **Equivalence record** for G1 at herdr `v0.9.1`\n\n# G1 scripted re-run\n';
const withoutLine = (label) => ATTESTATION.split('\n').filter((l) => !l.includes(label)).join('\n');
// #252: the verification a record made by the #252 driver carries, and that driver's run
// manifest (schemaVersion 3; herdr.executableCheck records the comparison with PINS.md's
// expected hash), and the committed PINS.md row it must agree with.
const VERIFICATION = [
  '## Verification',
  '',
  `- **herdr:** VERIFIED — \`herdr --version\` \`herdr 0.9.1\` equals the PINS.md pin; executable sha256 \`${HERDR_SHA}\` equals PINS.md's expected sha256 for \`linux-x64\` (\`herdr.executableCheck\`).`,
  '- **Harness:** VERIFIED — Claude Code: CLI `2.1.283`, wire `2.1.283` (`scenarioData.g1.versions`).',
  '- **Dialogs:** dev-channels (read #4; accepted outside the driver (recorded as human))',
  '- **Human actions:** the dev-channels dialog accept, G1 criterion 5: accepted at the keyboard by self-test operator. Sign-ins: none.',
  '- **Verified by:** self-test agent, 2026-10-03',
  '',
].join('\n');
const CHECK_MATCH = { result: 'match', platform: 'linux-x64', expectedSha256: HERDR_SHA, firstParty: true, basis: 'self-test', detail: 'equal' };
// A #252 run's driver commit is the self-test repository's real first commit (runSelfTest
// replaces this placeholder with its sha), so PINS.md at the driver commit is checked for real.
const DRIVER_COMMIT = 'd0'.repeat(20);
const DRIVER_252 = { ...DRIVER, driver_commit: DRIVER_COMMIT };
const RUN_252 = { ...RUN, schemaVersion: 3, driver: { commit: DRIVER_COMMIT, toolsHerdrDirty: false }, herdr: { ...RUN.herdr, executableCheck: CHECK_MATCH } };
const run252With = (check, exe = {}) => ({ ...RUN_252, herdr: { ...RUN_252.herdr, executable: { ...HERDR_EXE, ...exe }, executableCheck: check } });
const verificationWith = (from, to) => VERIFICATION.replace(from, to);
const pinsText = ({ sha = HERDR_SHA, firstParty = 'yes' } = {}) => [
  '| Platform | Release asset | Asset digest (sha256) | Expected executable sha256 | First-party | Basis |',
  '|---|---|---|---|---|---|',
  `| \`linux-x64\` | \`herdr-linux-x86_64\` | \`${sha}\` | \`${sha}\` | ${firstParty} | self-test |`,
  '',
].join('\n');
const RESULT_DRIVER = (record = RECORD) => `### G1 claude-wake\n\n- **Driver:** herdr (\`herdr 0.9.1\`) via \`tools/herdr/run.mjs\`, record \`${record}\`\n- **Gate id:** G1\n\n`;

function tree({ entries, run = RUN, runText = null, record = '# G1 scripted re-run\n', recordPath = RECORD, result = null, pins = pinsText(), extraFiles = [], untracked = {}, driverCommitPins = null }) {
  const files = { [manifestRelPath]: JSON.stringify({ fixtures: [baseEntry(HUMAN_FIXTURE), ...entries] }, null, 2) };
  for (const e of entries) files[e.path] = FIXTURE_BYTES;
  for (const f of extraFiles) files[f] = FIXTURE_BYTES;
  files[HUMAN_FIXTURE] = FIXTURE_BYTES;
  const runPath = recordPath.replace(/\.md$/, '.run-manifest.json');
  if (runText !== null) files[runPath] = runText;
  else if (run) files[runPath] = JSON.stringify(run);
  if (record) files[recordPath] = record;
  if (result) files[RESULT] = result;
  if (pins) files[PINS_PATH] = pins;
  return { files, untracked, ...(driverCommitPins ? { driverCommitPins } : {}) };
}
const herdrEntry = (driver, path = HERDR_FIXTURE, extra = {}) => ({ ...baseEntry(path), ...(driver === undefined ? {} : { driver }), ...extra });
const without = (k) => Object.fromEntries(Object.entries(DRIVER).filter(([key]) => key !== k));
const runWith = (patch) => ({ ...RUN, ...patch });
const herdrWith = (patch) => runWith({ herdr: { ...RUN.herdr, executable: { ...HERDR_EXE, ...patch } } });
const capturesWith = (patch) => runWith({ captures: RUN.captures.map((c) => ({ ...c, ...patch })) });

const SELF_TEST_CASES = [
  { name: 'control: human-run entry only, no driver block', expect: 'pass', ...tree({ entries: [], run: null, record: null }) },
  { name: 'control: -herdr entry with a complete driver block matching its run manifest', expect: 'pass', ...tree({ entries: [herdrEntry(DRIVER)] }) },
  { name: '-herdr entry with no driver block', expect: 'has no `driver` block', ...tree({ entries: [herdrEntry(undefined)] }) },
  { name: 'driver is not an object', expect: '`driver` must be an object', ...tree({ entries: [herdrEntry('herdr 0.9.1')] }) },
  { name: 'driver.herdr_version missing', expect: 'driver.herdr_version is missing', ...tree({ entries: [herdrEntry(without('herdr_version'))] }) },
  { name: 'driver.driver_commit missing', expect: 'driver.driver_commit is missing', ...tree({ entries: [herdrEntry(without('driver_commit'))] }) },
  { name: 'driver.run_manifest missing', expect: 'driver.run_manifest is missing', ...tree({ entries: [herdrEntry(without('run_manifest'))] }) },
  { name: 'driver.herdr_version not `herdr --version` output', expect: 'driver.herdr_version must be', ...tree({ entries: [herdrEntry({ ...DRIVER, herdr_version: '0.9.1' })] }) },
  { name: 'driver.driver_commit abbreviated', expect: 'full 40-hex commit', ...tree({ entries: [herdrEntry({ ...DRIVER, driver_commit: 'aaaaaaa' })] }) },
  { name: 'driver.run_manifest outside herdr-runs/', expect: 'driver.run_manifest must be a', ...tree({ entries: [herdrEntry({ ...DRIVER, run_manifest: '/tmp/run-manifest.json' })] }) },
  { name: 'run manifest absent', expect: 'is not committed (not tracked by git)', ...tree({ entries: [herdrEntry(DRIVER)], run: null }) },
  { name: 'run manifest on disk but untracked', expect: 'is not committed (not tracked by git)', ...tree({ entries: [herdrEntry(DRIVER)], run: null, untracked: { [RUN_MANIFEST]: JSON.stringify(RUN) } }) },
  { name: 'herdr-runs record missing beside the run manifest', expect: 'is not committed beside its run manifest', ...tree({ entries: [herdrEntry(DRIVER)], record: null }) },
  { name: 'run manifest outcome NOT RUN', expect: 'only a PASS run', ...tree({ entries: [herdrEntry(DRIVER)], run: runWith({ outcome: 'NOT RUN' }) }) },
  { name: 'run manifest from a dirty tools/herdr/', expect: 'toolsHerdrDirty true', ...tree({ entries: [herdrEntry(DRIVER)], run: runWith({ driver: { commit: COMMIT, toolsHerdrDirty: true } }) }) },
  { name: 'run manifest with toolsHerdrDirty unknown (null)', expect: 'toolsHerdrDirty null', ...tree({ entries: [herdrEntry(DRIVER)], run: runWith({ driver: { commit: COMMIT, toolsHerdrDirty: null } }) }) },
  { name: 'run manifest driver commit differs', expect: 'driver_commit does not match', ...tree({ entries: [herdrEntry(DRIVER)], run: runWith({ driver: { commit: 'b'.repeat(40), toolsHerdrDirty: false } }) }) },
  { name: 'run manifest herdr version differs', expect: 'herdr_version does not match', ...tree({ entries: [herdrEntry(DRIVER)], run: runWith({ herdr: { ...RUN.herdr, observedVersionOutput: 'herdr 0.9.2' } }) }) },
  { name: 'fixture not among the run\'s captures', expect: 'among its written captures', ...tree({ entries: [herdrEntry(DRIVER)], run: runWith({ captures: [] }) }) },
  { name: 'fixture among the run\'s captures but withheld (written: false)', expect: 'among its written captures', ...tree({ entries: [herdrEntry(DRIVER)], run: runWith({ captures: [{ file: posix.basename(HERDR_FIXTURE), written: false }] }) }) },
  { name: 'control (#216): -herdr entry with version_matches_pin false passes with a VERSION WARNING', expect: 'pass', expectOutput: 'WARN  ', ...tree({ entries: [herdrEntry(DRIVER, HERDR_FIXTURE, { version_matches_pin: false, version_matches_pin_note: 'Claude Code 2.1.999 is not the last tested 2.1.285' })] }) },
  { name: '-herdr entry with version_matches_pin not a boolean', expect: 'must be true or false', ...tree({ entries: [herdrEntry(DRIVER, HERDR_FIXTURE, { version_matches_pin: 'yes' })] }) },
  { name: 'driver block on a fixture without the -herdr suffix', expect: 'lacks the `-herdr` suffix', ...tree({ entries: [herdrEntry(DRIVER, `${fixturesDir}/g1-claude-wake/transcript-2026-10-01-2.1.283.jsonl`)] }) },
  { name: 'unverified-* herdr capture committed as a fixture', expect: 'is never a fixture', ...tree({ entries: [herdrEntry(DRIVER, UNVERIFIED_FIXTURE)] }) },
  { name: 'committed -herdr file with no manifest entry', expect: 'has no MANIFEST.json entry', ...tree({ entries: [], extraFiles: [HERDR_FIXTURE] }) },
  { name: 'control (history): pre-#252 equivalence record with a complete operator attestation and its history callout', expect: 'pass', ...tree({ entries: [herdrEntry(DRIVER)], record: `${EQUIV}\n${ATTESTATION}` }) },
  { name: '#252: pre-#252 attestation without the history callout', expect: 'without the `> **Pre-#252 attestation (history).**` callout', ...tree({ entries: [herdrEntry(DRIVER)], record: `${EQUIV}\n${ATTESTATION_BODY}` }) },
  { name: '#252: an operator attestation on a run the driver dated after #252 (timebox.start)', expect: 'after #252 (2026-10-03)', ...tree({ entries: [], run: runWith({ timebox: { start: '2026-10-05T09:00:00.000Z' } }), record: `${EQUIV}\n${ATTESTATION}` }) },
  { name: '#252: an operator attestation on a backdated record name whose run started after #252', expect: 'after #252 (2026-10-03)', ...tree({ entries: [], recordPath: `${herdrRunsDir}/G1-2026-09-01.md`, run: runWith({ timebox: { start: '2026-10-05T09:00:00.000Z' } }), record: `${EQUIV}\n${ATTESTATION}` }) },
  { name: '#252: an operator attestation on an undated record name whose run manifest records no timebox.start', expect: 'records no timebox.start', ...tree({ entries: [], recordPath: `${herdrRunsDir}/G1-undated.md`, run: runWith({ timebox: undefined }), record: `${EQUIV}\n${ATTESTATION}` }) },
  { name: 'control (#252): an operator attestation on an undated record name whose run started before #252', expect: 'pass', ...tree({ entries: [], recordPath: `${herdrRunsDir}/G1-undated.md`, record: `${EQUIV}\n${ATTESTATION}` }) },
  { name: '#252: an operator attestation on a run the #252 driver recorded', expect: 'carries a `## Verification` section, not an operator attestation', ...tree({ entries: [herdrEntry(DRIVER_252)], run: RUN_252, record: `${EQUIV}\n${ATTESTATION}` }) },
  { name: 'equivalence record with neither a verification nor an attestation', expect: 'has no `## Verification` section', ...tree({ entries: [herdrEntry(DRIVER)], record: EQUIV }) },
  { name: 'control (#252): equivalence record with a complete verification, its schemaVersion 3 run manifest recording a first-party match that PINS.md agrees with', expect: 'pass', ...tree({ entries: [herdrEntry(DRIVER_252)], run: RUN_252, record: `${EQUIV}\n${VERIFICATION}` }) },
  { name: '#252: verification whose expected hash is PINS.md\'s committed row but not the row at the driver commit (a real two-commit repository)', expect: 'is not PINS.md\'s first-party row (at driver commit', ...tree({ entries: [], run: RUN_252, driverCommitPins: pinsText({ sha: 'e'.repeat(64) }), record: `${EQUIV}\n${VERIFICATION}` }) },
  { name: '#252: verification whose run manifest\'s driver.commit is not in the repository', expect: 'is not in this repository', ...tree({ entries: [], run: { ...RUN_252, driver: { commit: 'b'.repeat(40), toolsHerdrDirty: false } }, record: `${EQUIV}\n${VERIFICATION}` }) },
  { name: '#252: verification with no committed run manifest', expect: 'is not committed, so nothing backs what it states', ...tree({ entries: [], run: null, record: `${EQUIV}\n${VERIFICATION}` }) },
  { name: '#252: verification with a run manifest that does not parse', expect: 'does not parse as JSON', ...tree({ entries: [], runText: '{ not json', record: `${EQUIV}\n${VERIFICATION}` }) },
  { name: '#252: verification with an unfilled slot', expect: 'unfilled `<TO FILL', ...tree({ entries: [herdrEntry(DRIVER_252)], run: RUN_252, record: `${EQUIV}\n${verificationWith('Sign-ins: none.', 'Sign-ins: <TO FILL: none, or each action>.')}` }) },
  { name: '#252: verification whose herdr line is UNVERIFIED', expect: 'missing its herdr line', ...tree({ entries: [herdrEntry(DRIVER_252)], run: RUN_252, record: `${EQUIV}\n${verificationWith('**herdr:** VERIFIED', '**herdr:** UNVERIFIED')}` }) },
  { name: '#252: verification whose Harness line is UNVERIFIED', expect: 'missing its Harness line', ...tree({ entries: [herdrEntry(DRIVER_252)], run: RUN_252, record: `${EQUIV}\n${verificationWith('**Harness:** VERIFIED', '**Harness:** UNVERIFIED')}` }) },
  { name: '#252: verification without the Human actions line', expect: 'missing its Human actions line', ...tree({ entries: [herdrEntry(DRIVER_252)], run: RUN_252, record: `${EQUIV}\n${VERIFICATION.split('\n').filter((l) => !l.includes('**Human actions:**')).join('\n')}` }) },
  { name: '#252: verification without a Verified by date', expect: 'missing its Verified by line', ...tree({ entries: [herdrEntry(DRIVER_252)], run: RUN_252, record: `${EQUIV}\n${verificationWith('self-test agent, 2026-10-03', 'self-test agent')}` }) },
  { name: '#252: verification on a pre-#252 run manifest (schemaVersion 2, no executable check)', expect: 'predates #252', ...tree({ entries: [herdrEntry(DRIVER)], record: `${EQUIV}\n${VERIFICATION}` }) },
  { name: '#252: verification on a schemaVersion 3 run manifest with its executable check deleted', expect: 'records no herdr.executableCheck match', ...tree({ entries: [], run: { ...RUN_252, herdr: { ...RUN.herdr } }, record: `${EQUIV}\n${VERIFICATION}` }) },
  { name: '#252: verification whose run manifest records no expected value for the platform', expect: 'records no herdr.executableCheck match', ...tree({ entries: [], run: run252With({ ...CHECK_MATCH, result: 'no-expected-value', expectedSha256: null, firstParty: null }), record: `${EQUIV}\n${VERIFICATION}` }) },
  { name: '#252: verification on a match against a locally observed (not first-party) value', expect: 'is not first-party', ...tree({ entries: [], run: run252With({ ...CHECK_MATCH, firstParty: false }), record: `${EQUIV}\n${VERIFICATION}` }) },
  { name: '#252: verification whose manifest\'s expected hash is not PINS.md\'s committed row', expect: 'is not PINS.md\'s first-party row (committed)', ...tree({ entries: [], driverCommitPins: pinsText({ sha: 'e'.repeat(64) }), run: run252With({ ...CHECK_MATCH, expectedSha256: 'e'.repeat(64) }, { sha256: 'e'.repeat(64) }), record: `${EQUIV}\n${verificationWith(HERDR_SHA, 'e'.repeat(64))}` }) },
  { name: '#252: verification where PINS.md\'s committed row is not first-party', expect: 'is not PINS.md\'s first-party row (committed)', ...tree({ entries: [], driverCommitPins: pinsText(), run: RUN_252, pins: pinsText({ firstParty: 'no' }), record: `${EQUIV}\n${VERIFICATION}` }) },
  { name: '#252: verification stating a herdr sha256 other than the run manifest\'s', expect: 'is not the one its run manifest', ...tree({ entries: [], run: RUN_252, record: `${EQUIV}\n${verificationWith(HERDR_SHA, 'e'.repeat(64))}` }) },
  { name: '#252: a fixture from a run whose herdr matched no expected value', expect: 'is never a fixture source', ...tree({ entries: [herdrEntry(DRIVER_252)], run: run252With({ ...CHECK_MATCH, result: 'no-expected-value', expectedSha256: null }) }) },
  { name: '#252: a fixture from a schemaVersion 3 run manifest with no executable check', expect: 'is never a fixture source', ...tree({ entries: [herdrEntry(DRIVER)], run: { ...RUN, schemaVersion: 3 } }) },
  { name: 'control (#252): gate result naming herdr as Driver and its record, verified', expect: 'pass', ...tree({ entries: [], run: RUN_252, result: `${RESULT_DRIVER()}${VERIFICATION}` }) },
  { name: '#252: gate result naming herdr as Driver, verified, but naming no record', expect: 'names no docs/planning/gates/herdr-runs/<record>.md', ...tree({ entries: [], run: RUN_252, result: `### G1 claude-wake\n\n- **Driver:** herdr (\`herdr 0.9.1\`)\n\n${VERIFICATION}` }) },
  { name: '#252: gate result naming a record that is not committed', expect: 'is not committed', ...tree({ entries: [], run: null, record: null, result: `${RESULT_DRIVER()}${VERIFICATION}` }) },
  { name: 'equivalence record: attestation without the herdr sha256', expect: 'missing its herdr line', ...tree({ entries: [herdrEntry(DRIVER)], record: `${EQUIV}\n${ATTESTATION.replace('c'.repeat(64), '<sha256>')}` }) },
  { name: 'equivalence record: attestation without the Harness line', expect: 'missing its Harness line', ...tree({ entries: [herdrEntry(DRIVER)], record: `${EQUIV}\n${withoutLine('**Harness:**')}` }) },
  { name: 'equivalence record: attestation without the Consent dialog line', expect: 'missing its Consent dialog line', ...tree({ entries: [herdrEntry(DRIVER)], record: `${EQUIV}\n${withoutLine('**Consent dialog:**')}` }) },
  { name: 'equivalence record: attestation unticked', expect: 'missing its Harness line', ...tree({ entries: [herdrEntry(DRIVER)], record: `${EQUIV}\n${ATTESTATION.replace('- [x] **Harness:**', '- [ ] **Harness:**')}` }) },
  { name: 'equivalence record: attestation without Attested by', expect: 'missing its Attested by line', ...tree({ entries: [herdrEntry(DRIVER)], record: `${EQUIV}\n${withoutLine('**Attested by:**')}` }) },
  { name: 'control (#140): schemaVersion 1 run manifest (before #140) passes with a WARN', expect: 'pass', expectOutput: 'is schemaVersion 1 (before #140)', ...tree({ entries: [herdrEntry(DRIVER)], run: runWith({ schemaVersion: 1, herdr: { observedVersionOutput: 'herdr 0.9.1' }, captures: RUN.captures.map(({ file, written }) => ({ file, written })) }) }) },
  { name: '#140: run manifest with an unknown schemaVersion', expect: 'is not one this check knows', ...tree({ entries: [herdrEntry(DRIVER)], run: runWith({ schemaVersion: undefined }) }) },
  { name: '#140: run from the test-double herdr (run under node)', expect: 'herdr.executable.testDouble true', ...tree({ entries: [herdrEntry(DRIVER)], run: herdrWith({ basename: 'fake-herdr.mjs', realBasename: 'fake-herdr.mjs', format: 'script', runUnderNode: true, testDouble: true }) }) },
  { name: '#140: run manifest without herdr.executable', expect: 'no herdr.executable', ...tree({ entries: [herdrEntry(DRIVER)], run: runWith({ herdr: { observedVersionOutput: 'herdr 0.9.1' } }) }) },
  { name: '#140: herdr executable is a script, not a native binary', expect: 'is not a hashed native binary (format "script")', ...tree({ entries: [herdrEntry(DRIVER)], run: herdrWith({ format: 'script' }) }) },
  { name: '#140: herdr executable recorded unhashed', expect: 'is not a hashed native binary', ...tree({ entries: [herdrEntry(DRIVER)], run: herdrWith({ sha256: null }) }) },
  { name: '#140: fixture bytes differ from the recorded capture hash', expect: 'differ from the run manifest\'s capture sha256', ...tree({ entries: [herdrEntry(DRIVER)], run: capturesWith({ sha256: 'd'.repeat(64) }) }) },
  { name: '#140: capture recorded with no sha256', expect: 'records no sha256 for capture', ...tree({ entries: [herdrEntry(DRIVER)], run: capturesWith({ sha256: null }) }) },
  { name: '#140: equivalence record whose attested herdr hash is not the run manifest\'s', expect: 'is not the one its run manifest', ...tree({ entries: [herdrEntry(DRIVER)], run: herdrWith({ sha256: 'e'.repeat(64) }), record: `${EQUIV}\n${ATTESTATION}` }) },
  { name: 'control: gate result with Driver: human operator', expect: 'pass', ...tree({ entries: [], run: null, record: null, result: '### G1 claude-wake\n\n- **Driver:** human operator\n' }) },
  { name: 'gate result naming herdr as Driver with no verification', expect: 'names herdr as its Driver but has no', ...tree({ entries: [], run: null, record: null, result: '### G1 claude-wake\n\n- **Driver:** herdr (`herdr 0.9.1`, PINS.md `herdr (test tooling)` v0.9.1)\n' }) },
  { name: 'control (history): gate result naming herdr as Driver and its pre-#252 record, attested', expect: 'pass', ...tree({ entries: [], result: `${RESULT_DRIVER()}${ATTESTATION}` }) },
  { name: '#252: gate result naming herdr as Driver, attested, but naming no record (previously passed with no manifest)', expect: 'names no docs/planning/gates/herdr-runs/<record>.md', ...tree({ entries: [], run: null, record: null, result: `### G1 claude-wake\n\n- **Driver:** herdr (\`herdr 0.9.1\`)\n\n${ATTESTATION}` }) },
];

function runSelfTest() {
  let failed = 0;
  for (const tc of SELF_TEST_CASES) {
    const dir = mkdtempSync(join(tmpdir(), 'oac-fixture-manifest-selftest-'));
    try {
      const put = (files) => {
        for (const [rel, content] of Object.entries(files)) {
          mkdirSync(dirname(join(dir, rel)), { recursive: true });
          writeFileSync(join(dir, rel), content);
        }
      };
      // A real first commit (#252), the "driver commit": it holds PINS.md as the driver read it
      // (tc.driverCommitPins, else the case's own PINS.md). Its sha replaces DRIVER_COMMIT in
      // every planted file; the case's files are then staged on top, so the index is "committed".
      const git = (...a) => execFileSync('git', ['-c', 'user.name=oac-selftest', '-c', 'user.email=selftest@invalid', '-c', 'commit.gpgsign=false', '-c', 'core.autocrlf=false', ...a], { cwd: dir, stdio: ['ignore', 'pipe', 'ignore'], encoding: 'utf8' });
      git('init', '-q');
      const c1Pins = tc.driverCommitPins ?? tc.files[PINS_PATH];
      if (c1Pins) put({ [PINS_PATH]: c1Pins });
      git('add', '-A');
      git('commit', '-q', '--allow-empty', '-m', 'driver commit');
      const c1 = git('rev-parse', 'HEAD').trim();
      put(Object.fromEntries(Object.entries(tc.files).map(([k, v]) => [k, v.replaceAll(DRIVER_COMMIT, c1)])));
      if (!tc.files[PINS_PATH] && c1Pins) rmSync(join(dir, PINS_PATH));
      git('add', '-A');
      put(tc.untracked ?? {});
      const run = spawnSync(process.execPath, [scriptPath, '--root', dir], { encoding: 'utf8' });
      const output = `${run.stdout}${run.stderr}`;
      const failLines = output.split('\n').filter((l) => l.trim().startsWith('FAIL  ')).length;
      const ok = tc.expect === 'pass' ? run.status === 0 && (!tc.expectOutput || output.includes(tc.expectOutput)) : run.status === 1 && failLines === 1 && output.includes(tc.expect);
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
