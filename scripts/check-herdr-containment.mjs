#!/usr/bin/env node
// Keep herdr (Epic K, #123: dev/test-only terminal automation) contained. This is
// oac-boundaries checks 9 and 10 (.claude/skills/oac-boundaries/references/mechanical-checks.md).
//
//   node scripts/check-herdr-containment.mjs              # check this repository
//   node scripts/check-herdr-containment.mjs --root <dir> # check another git work tree
//   node scripts/check-herdr-containment.mjs --self-test  # plant violations, assert each fails
//
// Check 9  (containment): no `herdr` / `HERDR_` match (case-insensitive) in any git-tracked
//          file under adapters/, core/, cli/, transports/, or spec/; and no workspace or
//          package manifest outside tools/herdr/ that references tools/herdr.
// Check 10 (driver hygiene): no harness-credential access or harness-config mutation in any
//          git-tracked file under tools/herdr/ -- auth.json, .credentials.json,
//          ANTHROPIC_API_KEY, OPENAI_API_KEY, keyring/keychain/OS credential-store access,
//          `integration install`. (ADR-001 boundaries 3, 4, 13.)
//
// A target with no git-tracked files (the path is not built yet) reports PENDING, never
// clean: pending is not a pass, and the check must be re-read once the path exists.
// Hits print file:line and the rule matched, never the line itself (a credential line
// must not be echoed into a CI log).
//
// Exit codes: 0 = no violations (result CLEAN or PENDING, printed on the last line);
// 1 = at least one violation (or a failed self-test case); 2 = usage or environment error.

import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync, statSync } from 'node:fs';
import { execFileSync, spawnSync } from 'node:child_process';
import { join, dirname, resolve, basename } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const scriptPath = fileURLToPath(import.meta.url);

const PRODUCT_PATHS = ['adapters', 'core', 'cli', 'transports', 'spec'];
const DRIVER_PATH = 'tools/herdr';

// Covers `herdr`, `Herdr`, and every `HERDR_*` environment variable.
const HERDR_TOKEN = /herdr/i;
const HERDR_PATH_REF = /tools[\\/]+herdr/i;

// Workspace and package manifests (and lockfiles, which record path dependencies) for the
// toolchains the C1 language decision left in play.
const MANIFEST_NAMES = new Set([
  'Cargo.toml',
  'Cargo.lock',
  'package.json',
  'package-lock.json',
  'npm-shrinkwrap.json',
  'pnpm-workspace.yaml',
  'pnpm-lock.yaml',
  'yarn.lock',
  'deno.json',
  'deno.jsonc',
  'go.mod',
  'go.work',
  'pyproject.toml',
]);

const DRIVER_RULES = [
  { id: 'auth-json', label: 'harness credential file auth.json', re: /auth\.json/i },
  { id: 'credentials-json', label: 'harness credential file .credentials.json', re: /\.credentials\.json/i },
  { id: 'ANTHROPIC_API_KEY', label: 'provider API key variable ANTHROPIC_API_KEY', re: /ANTHROPIC_API_KEY/i },
  { id: 'OPENAI_API_KEY', label: 'provider API key variable OPENAI_API_KEY', re: /OPENAI_API_KEY/i },
  { id: 'keyring', label: 'keyring access', re: /keyring/i },
  { id: 'keychain', label: 'keychain access', re: /keychain/i },
  {
    id: 'credential-store',
    label: 'OS credential-store access',
    re: /secret-tool|libsecret|find-(?:generic|internet)-password|credential[\s_-]*manager|cmdkey/i,
  },
  // Matches both a shell string and an argv array ('integration', 'install').
  { id: 'integration-install', label: 'harness-config mutation `integration install`', re: /integration[\s'"`,]+install/i },
];

// --- scanning --------------------------------------------------------------

function trackedFiles(root, pathspec) {
  const args = ['ls-files', '-z'];
  if (pathspec) args.push('--', pathspec);
  const out = execFileSync('git', args, { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  return out.split('\0').filter(Boolean);
}

function readTracked(root, rel) {
  const abs = join(root, rel);
  try {
    if (!statSync(abs).isFile()) return null; // submodule gitlink or similar
    return readFileSync(abs, 'utf8');
  } catch {
    return null; // tracked but deleted in the work tree
  }
}

function scanFile(root, rel, rules, hits) {
  const text = readTracked(root, rel);
  if (text === null) return;
  const lines = text.split(/\r?\n/);
  lines.forEach((line, i) => {
    for (const rule of rules) {
      const m = rule.re.exec(line);
      if (m) hits.push({ where: `${rel}:${i + 1}`, rule: rule.label, match: m[0] });
    }
  });
}

function runChecks(root) {
  const results = []; // { check, target, status: 'ok'|'pending'|'fail', detail, hits }

  // Check 9a: herdr in product paths.
  const productRule = [{ label: 'herdr reference in a product path', re: HERDR_TOKEN }];
  for (const dir of PRODUCT_PATHS) {
    const files = trackedFiles(root, `${dir}/`);
    if (files.length === 0) {
      results.push({ check: '9', target: `${dir}/`, status: 'pending', detail: 'no git-tracked files (path not built yet)' });
      continue;
    }
    const hits = [];
    for (const f of files) scanFile(root, f, productRule, hits);
    results.push({
      check: '9',
      target: `${dir}/`,
      status: hits.length ? 'fail' : 'ok',
      detail: `${files.length} tracked file(s), ${hits.length} hit(s)`,
      hits,
    });
  }

  // Check 9b: manifests outside tools/herdr/ referencing tools/herdr.
  const manifests = trackedFiles(root).filter(
    (f) => MANIFEST_NAMES.has(basename(f)) && !f.startsWith(`${DRIVER_PATH}/`),
  );
  if (manifests.length === 0) {
    results.push({
      check: '9',
      target: `manifests outside ${DRIVER_PATH}/`,
      status: 'pending',
      detail: 'no workspace or package manifests tracked yet',
    });
  } else {
    const hits = [];
    const rule = [{ label: `manifest references ${DRIVER_PATH}`, re: HERDR_PATH_REF }];
    for (const f of manifests) scanFile(root, f, rule, hits);
    results.push({
      check: '9',
      target: `manifests outside ${DRIVER_PATH}/`,
      status: hits.length ? 'fail' : 'ok',
      detail: `${manifests.length} manifest(s), ${hits.length} hit(s)`,
      hits,
    });
  }

  // Check 10: credential access / harness-config mutation in the driver.
  const driverFiles = trackedFiles(root, `${DRIVER_PATH}/`);
  if (driverFiles.length === 0) {
    results.push({ check: '10', target: `${DRIVER_PATH}/`, status: 'pending', detail: 'no git-tracked files (K3 creates the driver)' });
  } else {
    const hits = [];
    for (const f of driverFiles) scanFile(root, f, DRIVER_RULES, hits);
    results.push({
      check: '10',
      target: `${DRIVER_PATH}/`,
      status: hits.length ? 'fail' : 'ok',
      detail: `${driverFiles.length} tracked file(s), ${hits.length} hit(s)`,
      hits,
    });
  }

  return results;
}

function report(root, results) {
  const tag = { ok: 'ok     ', pending: 'PENDING', fail: 'FAIL   ' };
  console.log(`herdr containment -- oac-boundaries checks 9 and 10 (root: ${root})`);
  for (const r of results) {
    console.log(`  ${tag[r.status]}  check ${r.check.padEnd(2)}  ${r.target}: ${r.detail}`);
    for (const h of r.hits ?? []) console.log(`             FAIL  ${h.where}: ${h.rule} (matched \`${h.match}\`)`);
  }
  const fails = results.filter((r) => r.status === 'fail');
  const pending = results.filter((r) => r.status === 'pending');
  const hitCount = fails.reduce((n, r) => n + r.hits.length, 0);
  if (fails.length) {
    console.log(`\nResult: FAIL -- ${hitCount} violation(s) in ${fails.length} target(s). Stop and cite the boundary (oac-boundaries).`);
    return 1;
  }
  if (pending.length) {
    console.log(
      `\nResult: PENDING -- 0 violations; ${pending.length} of ${results.length} target(s) not built yet. ` +
        'Pending is not a pass: re-run once those paths exist.',
    );
    return 0;
  }
  console.log(`\nResult: CLEAN -- 0 violations across all ${results.length} targets.`);
  return 0;
}

// --- self-test -------------------------------------------------------------

// Each case builds a throwaway git work tree, runs this script against it with --root, and
// asserts the exit code and the result line. Violation cases plant exactly one hit.
const CLEAN_BASE = {
  'adapters/claude/src/lib.rs': '// adapter\n',
  'core/src/lib.rs': '// core\n',
  'cli/src/main.rs': 'fn main() {}\n',
  'transports/zenoh/src/lib.rs': '// transport\n',
  'spec/session-channels.md': '# Session Channels\n',
  'tools/herdr/run.mjs': "// driver\nimport { spawnSync } from 'node:child_process';\n",
  // A manifest inside tools/herdr/ may name its own path.
  'tools/herdr/scenarios.json': '{ "root": "tools/herdr" }\n',
  'Cargo.toml': '[workspace]\nmembers = ["core", "cli"]\n',
  // herdr is allowed outside product paths (docs, scripts, the driver itself).
  'docs/planning/herdr-notes.md': 'herdr is test tooling; HERDR_SESSION is set by the driver.\n',
};

function violation(name, path, content) {
  return { name, expect: 'fail', files: { ...CLEAN_BASE, [path]: content } };
}

const SELF_TEST_CASES = [
  // Check 9: one per product path, plus the HERDR_ spelling.
  violation('9 herdr in adapters/', 'adapters/claude/src/lib.rs', 'use herdr_client;\n'),
  violation('9 HERDR_ in core/', 'core/src/lib.rs', 'const S: &str = "HERDR_SESSION";\n'),
  violation('9 herdr in cli/', 'cli/src/main.rs', '// spawn herdr for tests\nfn main() {}\n'),
  violation('9 herdr in transports/', 'transports/zenoh/src/lib.rs', 'mod Herdr;\n'),
  violation('9 herdr in spec/', 'spec/session-channels.md', 'Harnesses MAY be driven by herdr.\n'),
  // Check 9: manifests outside tools/herdr/ referencing tools/herdr.
  violation('9 Cargo.toml references tools/herdr', 'Cargo.toml', '[workspace]\nmembers = ["core", "tools/herdr"]\n'),
  violation('9 package.json references tools/herdr', 'package.json', '{ "workspaces": ["tools/herdr"] }\n'),
  violation('9 nested manifest references tools\\herdr', 'tools/other/pyproject.toml', 'path = "..\\\\tools\\\\herdr"\n'),
  // Check 10: one per driver rule.
  violation('10 auth.json', 'tools/herdr/run.mjs', "readFileSync(join(home, '.codex', 'auth.json'));\n"),
  violation('10 .credentials.json', 'tools/herdr/run.mjs', "const f = '~/.claude/.credentials.json';\n"),
  violation('10 ANTHROPIC_API_KEY', 'tools/herdr/lib/env.mjs', 'const k = process.env.ANTHROPIC_API_KEY;\n'),
  violation('10 OPENAI_API_KEY', 'tools/herdr/lib/env.mjs', "env['OPENAI_API_KEY'] = token;\n"),
  violation('10 keyring', 'tools/herdr/lib/auth.mjs', "import keyring from 'keyring';\n"),
  violation('10 keychain', 'tools/herdr/lib/auth.mjs', '// read the macOS Keychain entry\n'),
  violation('10 OS credential store', 'tools/herdr/lib/auth.mjs', "spawnSync('secret-tool', ['lookup']);\n"),
  violation('10 integration install (shell)', 'tools/herdr/setup.sh', 'herdr integration install claude\n'),
  violation('10 integration install (argv)', 'tools/herdr/run.mjs', "spawnSync('herdr', ['integration', 'install']);\n"),
  // Controls: these must NOT fail.
  { name: 'control: empty tree reports PENDING', expect: 'pending', files: { 'README.md': '# empty\n' } },
  { name: 'control: clean full tree reports CLEAN', expect: 'clean', files: CLEAN_BASE },
  {
    name: 'control: untracked herdr file is ignored',
    expect: 'clean',
    files: CLEAN_BASE,
    untracked: { 'adapters/claude/scratch.rs': 'herdr\n' },
  },
];

function writeTree(dir, files) {
  for (const [rel, content] of Object.entries(files)) {
    const abs = join(dir, rel);
    mkdirSync(dirname(abs), { recursive: true });
    writeFileSync(abs, content);
  }
}

function runSelfTest() {
  let failed = 0;
  for (const tc of SELF_TEST_CASES) {
    const dir = mkdtempSync(join(tmpdir(), 'oac-herdr-selftest-'));
    try {
      execFileSync('git', ['init', '-q'], { cwd: dir });
      writeTree(dir, tc.files);
      execFileSync('git', ['add', '-A'], { cwd: dir });
      if (tc.untracked) writeTree(dir, tc.untracked);
      const run = spawnSync(process.execPath, [scriptPath, '--root', dir], { encoding: 'utf8' });
      const resultLine = (run.stdout.match(/^Result: (\w+)/m) ?? [])[1];
      const ok =
        tc.expect === 'fail'
          ? run.status === 1 && resultLine === 'FAIL' && (run.stdout.match(/FAIL {2}\S+:\d+: /g) ?? []).length === 1
          : run.status === 0 && resultLine === tc.expect.toUpperCase();
      if (!ok) failed += 1;
      console.log(`  ${ok ? 'ok  ' : 'FAIL'}  ${tc.name}  (exit ${run.status}, Result: ${resultLine ?? 'none'})`);
      if (!ok) console.log(run.stdout + run.stderr);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }
  const violations = SELF_TEST_CASES.filter((tc) => tc.expect === 'fail').length;
  console.log(
    `\nself-test: ${SELF_TEST_CASES.length - failed}/${SELF_TEST_CASES.length} cases as expected ` +
      `(${violations} planted violations must exit 1; ${SELF_TEST_CASES.length - violations} controls must exit 0).`,
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
    console.error('usage: node scripts/check-herdr-containment.mjs [--root <dir>] [--self-test]');
    process.exit(2);
  }
  root = resolve(argv[rootIdx + 1]);
}

let results;
try {
  results = runChecks(root);
} catch (err) {
  console.error(`Could not list git-tracked files under ${root}: ${err.message}`);
  process.exit(2);
}
process.exit(report(root, results));
