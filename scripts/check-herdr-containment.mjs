#!/usr/bin/env node
// Keep herdr (Epic K, #123: dev/test-only terminal automation) contained. This is
// oac-boundaries checks 9 and 10 (.claude/skills/oac-boundaries/references/mechanical-checks.md).
//
//   node scripts/check-herdr-containment.mjs              # check this repository
//   node scripts/check-herdr-containment.mjs --root <dir> # check another git work tree
//   node scripts/check-herdr-containment.mjs --self-test  # plant violations, assert each fails
//
// Check 9  (containment): no `herdr` / `HERDR_` match (case-insensitive) in any git-tracked
//          entry under adapters/, core/, cli/, transports/, or spec/; and no workspace or
//          package manifest outside tools/herdr/ that references tools/herdr.
// Check 10 (driver hygiene): no harness-credential access or harness-config mutation in any
//          git-tracked entry under tools/herdr/ -- auth.json, .credentials.json, provider
//          and harness credential variables, keyring/keychain/OS credential-store access,
//          writes to harness config files, and harness config-mutating CLI calls
//          (`integration install`, `claude mcp add`, ...). (ADR-001 boundaries 3, 4, 13.)
//
// "Entry" means every git index entry, by its index mode: the tracked path itself is
// matched; a regular file's content is matched; a symlink's stored target text is matched
// (and the content of the file it resolves to, if any); a submodule (gitlink) is matched by
// its path and by its .gitmodules url. Nothing is skipped because it is not a regular file.
//
// A target with no git-tracked files (the path is not built yet) reports PENDING, never
// clean: pending is not a pass, and the check must be re-read once the path exists.
// Hits print file:line and the rule matched, never the line or link target itself (a
// credential line or home path must not be echoed into a CI log).
//
// Exit codes: 0 = no violations (result CLEAN or PENDING, printed on the last line);
// 1 = at least one violation (or a failed self-test case); 2 = usage or environment error.

import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync, statSync, lstatSync } from 'node:fs';
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

const ARG_SEP = String.raw`['"\x60]?[\s,[]+['"\x60]?`;
const HARNESS_CONFIG = String.raw`(?:\.claude\W+settings(?:\.local)?\.json|\.codex\W+config\.toml|hooks\.json)`;

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
  { id: 'CLAUDE_CODE_OAUTH_TOKEN', label: 'harness credential variable CLAUDE_CODE_OAUTH_TOKEN', re: /CLAUDE_CODE_OAUTH_TOKEN/i },
  { id: 'ANTHROPIC_AUTH_TOKEN', label: 'provider credential variable ANTHROPIC_AUTH_TOKEN', re: /ANTHROPIC_AUTH_TOKEN/i },
  { id: 'CODEX_API_KEY', label: 'harness credential variable CODEX_API_KEY', re: /CODEX_API_KEY/i },
  // `[\s'"`,[]+` lets each command pattern match a shell string and an argv array alike,
  // e.g. `herdr integration install` and ['integration', 'install'].
  { id: 'integration-install', label: 'harness-config mutation `integration install`', re: /integration[\s'"`,[]+install/i },
  {
    id: 'harness-cli-mutation',
    label: 'harness config-mutating CLI call',
    re: new RegExp(
      String.raw`\b(?:claude|codex)${ARG_SEP}(?:` +
        String.raw`mcp${ARG_SEP}(?:add[\w-]*|remove|reset[\w-]*|login|logout)` +
        String.raw`|plugins?${ARG_SEP}(?:install|uninstall|enable|disable|update|marketplace)` +
        String.raw`|features${ARG_SEP}(?:enable|disable)` +
        String.raw`|login|logout|setup-token)\b`,
      'i',
    ),
  },
  // Harness config files: ~/.claude/settings(.local).json, ~/.codex/config.toml, hooks.json.
  // Reading or hashing them is allowed (K3 records before/after hashes); a write is not.
  // Line-level heuristic: a write API, shell redirect, `tee`, or `sed -i` on the same line
  // as the file name. Keep config paths out of variables passed to write calls.
  {
    id: 'harness-config-write',
    label: 'write to a harness config file',
    re: new RegExp(
      String.raw`(?:\b(?:writeFile|appendFile|copyFile|cpSync|rename|symlink|link|unlink|rmSync|truncate|createWriteStream|tee|sed\s+-i)\w*|(?<![=-])>>?\s*)` +
        String.raw`.*?${HARNESS_CONFIG}`,
      'i',
    ),
  },
];

// --- scanning --------------------------------------------------------------

// Every git index entry under `pathspec`, with its mode: 100644/100755 regular file,
// 120000 symlink (the blob is the link target text), 160000 submodule gitlink.
function trackedEntries(root, pathspec) {
  const args = ['ls-files', '-s', '-z'];
  if (pathspec) args.push('--', pathspec);
  const out = execFileSync('git', args, { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  return out
    .split('\0')
    .filter(Boolean)
    .map((rec) => {
      const tab = rec.indexOf('\t');
      const [mode, sha] = rec.slice(0, tab).split(' ');
      return { mode, sha, path: rec.slice(tab + 1) };
    });
}

function readRegularFile(abs) {
  try {
    if (!statSync(abs).isFile()) return null;
    return readFileSync(abs, 'utf8');
  } catch {
    return null; // tracked but deleted in the work tree, or a dangling link
  }
}

// submodule path -> { url, line } from a tracked .gitmodules, if any.
function readGitmodules(root) {
  const map = new Map();
  const text = readRegularFile(join(root, '.gitmodules'));
  if (text === null) return map;
  let path = null;
  let url = null;
  const flush = () => {
    if (path !== null && url !== null) map.set(path, url);
    path = null;
    url = null;
  };
  text.split(/\r?\n/).forEach((line, i) => {
    if (/^\s*\[/.test(line)) flush();
    const m = /^\s*(path|url)\s*=\s*(.*?)\s*$/.exec(line);
    if (m && m[1] === 'path') path = m[2];
    if (m && m[1] === 'url') url = { text: m[2], line: i + 1 };
  });
  flush();
  return map;
}

function matchText(text, where, rules, hits) {
  text.split(/\r?\n/).forEach((line, i) => {
    for (const rule of rules) {
      const m = rule.re.exec(line);
      if (m) hits.push({ where: where(i + 1), rule: rule.label, match: m[0] });
    }
  });
}

function scanEntry(root, entry, rules, hits, { matchPath, gitmodules }) {
  const rel = entry.path;
  if (matchPath) matchText(rel, () => `${rel} (tracked path)`, rules, hits);

  if (entry.mode === '160000') {
    const url = gitmodules.get(rel);
    if (url) matchText(url.text, () => `.gitmodules:${url.line} (url of submodule ${rel})`, rules, hits);
    return;
  }

  if (entry.mode === '120000') {
    // The link target as git stores it, independent of core.symlinks on this checkout.
    const target = execFileSync('git', ['cat-file', 'blob', entry.sha], { cwd: root, encoding: 'utf8' });
    matchText(target, () => `${rel} (symlink target)`, rules, hits);
    const abs = join(root, rel);
    let isLink = false;
    try {
      isLink = lstatSync(abs).isSymbolicLink();
    } catch {
      // not in the work tree
    }
    if (isLink) {
      const content = readRegularFile(abs);
      if (content !== null) matchText(content, (n) => `${rel}:${n} (via symlink)`, rules, hits);
    }
    return;
  }

  const content = readRegularFile(join(root, rel));
  if (content !== null) matchText(content, (n) => `${rel}:${n}`, rules, hits);
}

function runChecks(root) {
  const results = []; // { check, target, status: 'ok'|'pending'|'fail', detail, hits }
  const gitmodules = readGitmodules(root);

  // Check 9a: herdr in product paths.
  const productRule = [{ label: 'herdr reference in a product path', re: HERDR_TOKEN }];
  for (const dir of PRODUCT_PATHS) {
    const files = trackedEntries(root, `${dir}/`);
    if (files.length === 0) {
      results.push({ check: '9', target: `${dir}/`, status: 'pending', detail: 'no git-tracked files (path not built yet)' });
      continue;
    }
    const hits = [];
    for (const e of files) scanEntry(root, e, productRule, hits, { matchPath: true, gitmodules });
    results.push({
      check: '9',
      target: `${dir}/`,
      status: hits.length ? 'fail' : 'ok',
      detail: `${files.length} tracked entr${files.length === 1 ? 'y' : 'ies'}, ${hits.length} hit(s)`,
      hits,
    });
  }

  // Check 9b: manifests outside tools/herdr/ referencing tools/herdr.
  const manifests = trackedEntries(root).filter(
    (e) => MANIFEST_NAMES.has(basename(e.path)) && !e.path.startsWith(`${DRIVER_PATH}/`),
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
    for (const e of manifests) scanEntry(root, e, rule, hits, { matchPath: false, gitmodules });
    results.push({
      check: '9',
      target: `manifests outside ${DRIVER_PATH}/`,
      status: hits.length ? 'fail' : 'ok',
      detail: `${manifests.length} manifest(s), ${hits.length} hit(s)`,
      hits,
    });
  }

  // Check 10: credential access / harness-config mutation in the driver.
  const driverFiles = trackedEntries(root, `${DRIVER_PATH}/`);
  if (driverFiles.length === 0) {
    results.push({ check: '10', target: `${DRIVER_PATH}/`, status: 'pending', detail: 'no git-tracked files (K3 creates the driver)' });
  } else {
    const hits = [];
    for (const e of driverFiles) scanEntry(root, e, DRIVER_RULES, hits, { matchPath: true, gitmodules });
    results.push({
      check: '10',
      target: `${DRIVER_PATH}/`,
      status: hits.length ? 'fail' : 'ok',
      detail: `${driverFiles.length} tracked entr${driverFiles.length === 1 ? 'y' : 'ies'}, ${hits.length} hit(s)`,
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
// `links` plants tracked symlinks and `gitlinks` tracked submodules straight into the git
// index, so a case does not depend on the OS being able to create symlinks.
const CLEAN_BASE = {
  'adapters/claude/src/lib.rs': '// adapter\n',
  'core/src/lib.rs': '// core\n',
  'cli/src/main.rs': 'fn main() {}\n',
  'transports/zenoh/src/lib.rs': '// transport\n',
  'spec/session-channels.md': '# Session Channels\n',
  'tools/herdr/run.mjs': "// driver\nimport { spawnSync } from 'node:child_process';\n",
  // Allowed in the driver: hashing (reading) harness config, writing herdr's own config,
  // launching a harness. These guard check 10 against false positives in every case.
  'tools/herdr/lib/allowed.mjs': [
    "const before = sha256(readFileSync(join(home, '.claude', 'settings.json')));",
    "const hashes = paths.map((p) => sha256(readFileSync(join(home, '.codex', 'config.toml'))));",
    "writeFileSync(join(scratch, 'herdr', 'config.toml'), herdrConfig);",
    "spawnSync('claude', ['--channels', 'plugin:oac@local']);",
    "spawnSync('codex', ['app-server']);",
    '',
  ].join('\n'),
  // A manifest inside tools/herdr/ may name its own path.
  'tools/herdr/scenarios.json': '{ "root": "tools/herdr" }\n',
  'Cargo.toml': '[workspace]\nmembers = ["core", "cli"]\n',
  // herdr is allowed outside product paths (docs, scripts, the driver itself).
  'docs/planning/herdr-notes.md': 'herdr is test tooling; HERDR_SESSION is set by the driver.\n',
};

function violation(name, path, content, extra = {}) {
  return { name, expect: 'fail', files: { ...CLEAN_BASE, ...(path ? { [path]: content } : {}) }, ...extra };
}

const SELF_TEST_CASES = [
  // Check 9: one per product path, plus the HERDR_ spelling.
  violation('9 herdr in adapters/', 'adapters/claude/src/lib.rs', 'use herdr_client;\n'),
  violation('9 HERDR_ in core/', 'core/src/lib.rs', 'const S: &str = "HERDR_SESSION";\n'),
  violation('9 herdr in cli/', 'cli/src/main.rs', '// spawn herdr for tests\nfn main() {}\n'),
  violation('9 herdr in transports/', 'transports/zenoh/src/lib.rs', 'mod Herdr;\n'),
  violation('9 herdr in spec/', 'spec/session-channels.md', 'Harnesses MAY be driven by herdr.\n'),
  // Check 9: entries that are not a regular file's content.
  violation('9 herdr in a tracked file name', 'adapters/claude/src/herdr_bridge.rs', '// bridge\n'),
  violation('9 symlink into tools/herdr', null, null, {
    links: { 'adapters/claude/vendor': '../../tools/herdr/lib' },
  }),
  violation('9 symlink to a file whose content names herdr', 'scripts/helper.rs', 'use herdr;\n', {
    links: { 'adapters/claude/src/extra.rs': '../../../scripts/helper.rs' },
    needsOsSymlinks: true,
  }),
  violation('9 submodule named herdr', null, null, { gitlinks: ['adapters/herdr'] }),
  violation(
    '9 submodule whose url names herdr',
    '.gitmodules',
    '[submodule "bridge"]\n\tpath = adapters/vendor/bridge\n\turl = https://example.invalid/herdr-bridge.git\n',
    { gitlinks: ['adapters/vendor/bridge'] },
  ),
  // Check 9: manifests outside tools/herdr/ referencing tools/herdr.
  violation('9 Cargo.toml references tools/herdr', 'Cargo.toml', '[workspace]\nmembers = ["core", "tools/herdr"]\n'),
  violation('9 package.json references tools/herdr', 'package.json', '{ "workspaces": ["tools/herdr"] }\n'),
  violation('9 nested manifest references tools\\herdr', 'tools/other/pyproject.toml', 'path = "..\\\\tools\\\\herdr"\n'),
  // Check 10: one per driver rule.
  violation('10 auth.json', 'tools/herdr/run.mjs', "readFileSync(join(home, '.codex', 'auth.json'));\n"),
  violation('10 .credentials.json', 'tools/herdr/run.mjs', "const f = '~/.claude/.credentials.json';\n"),
  violation('10 ANTHROPIC_API_KEY', 'tools/herdr/lib/env.mjs', 'const k = process.env.ANTHROPIC_API_KEY;\n'),
  violation('10 OPENAI_API_KEY', 'tools/herdr/lib/env.mjs', "env['OPENAI_API_KEY'] = token;\n"),
  violation('10 CLAUDE_CODE_OAUTH_TOKEN', 'tools/herdr/lib/env.mjs', 'env.CLAUDE_CODE_OAUTH_TOKEN = t;\n'),
  violation('10 ANTHROPIC_AUTH_TOKEN', 'tools/herdr/lib/env.mjs', 'const t = process.env.ANTHROPIC_AUTH_TOKEN;\n'),
  violation('10 CODEX_API_KEY', 'tools/herdr/lib/env.mjs', 'export CODEX_API_KEY="$KEY"\n'),
  violation('10 keyring', 'tools/herdr/lib/auth.mjs', "import keyring from 'keyring';\n"),
  violation('10 keychain', 'tools/herdr/lib/auth.mjs', '// read the macOS Keychain entry\n'),
  violation('10 OS credential store', 'tools/herdr/lib/auth.mjs', "spawnSync('secret-tool', ['lookup']);\n"),
  violation('10 integration install (shell)', 'tools/herdr/setup.sh', 'herdr integration install claude\n'),
  violation('10 integration install (argv)', 'tools/herdr/run.mjs', "spawnSync('herdr', ['integration', 'install']);\n"),
  violation('10 claude mcp add (argv)', 'tools/herdr/run.mjs', "execFileSync('claude', ['mcp', 'add', 'oac', '--', 'node', 'x.mjs']);\n"),
  violation('10 codex mcp remove (shell)', 'tools/herdr/setup.sh', 'codex mcp remove oac\n'),
  violation('10 claude plugin install (shell)', 'tools/herdr/setup.sh', 'claude plugin install oac@local\n'),
  violation('10 write ~/.claude/settings.json', 'tools/herdr/run.mjs', "writeFileSync(join(home, '.claude', 'settings.json'), JSON.stringify(s));\n"),
  violation('10 append ~/.codex/config.toml', 'tools/herdr/setup.sh', "echo '[mcp_servers.oac]' >> ~/.codex/config.toml\n"),
  violation('10 write hooks.json', 'tools/herdr/run.mjs', "fs.writeFileSync(dir + '/hooks.json', '{}');\n"),
  violation('10 credential file by tracked name', 'tools/herdr/fixtures/auth.json', '{}\n'),
  violation('10 symlink to a credential file', null, null, {
    links: { 'tools/herdr/fixtures/creds': '/home/operator/.codex/auth.json' },
  }),
  // Controls: these must NOT fail.
  { name: 'control: empty tree reports PENDING', expect: 'pending', files: { 'README.md': '# empty\n' } },
  { name: 'control: clean full tree reports CLEAN', expect: 'clean', files: CLEAN_BASE },
  {
    name: 'control: untracked herdr file is ignored',
    expect: 'clean',
    files: CLEAN_BASE,
    untracked: { 'adapters/claude/scratch.rs': 'herdr\n' },
  },
  {
    name: 'control: clean symlink and submodule in a product path',
    expect: 'clean',
    files: CLEAN_BASE,
    links: { 'adapters/claude/src/alias.rs': 'lib.rs' },
    gitlinks: ['adapters/vendor/dep'],
  },
];

function writeTree(dir, files) {
  for (const [rel, content] of Object.entries(files)) {
    const abs = join(dir, rel);
    mkdirSync(dirname(abs), { recursive: true });
    writeFileSync(abs, content);
  }
}

// Returns false when the OS checkout did not produce a real symlink (e.g. Windows with
// core.symlinks=false); the index entry is a symlink either way.
function plantIndexEntries(dir, tc) {
  const git = (args, input) => execFileSync('git', args, { cwd: dir, encoding: 'utf8', input }).trim();
  let osSymlinks = true;
  for (const [rel, target] of Object.entries(tc.links ?? {})) {
    mkdirSync(dirname(join(dir, rel)), { recursive: true });
    const sha = git(['hash-object', '-w', '--stdin'], target);
    git(['update-index', '--add', '--cacheinfo', `120000,${sha},${rel}`]);
    git(['checkout', '--', rel]);
    try {
      if (!lstatSync(join(dir, rel)).isSymbolicLink()) osSymlinks = false;
    } catch {
      osSymlinks = false;
    }
  }
  for (const rel of tc.gitlinks ?? []) {
    mkdirSync(join(dir, rel), { recursive: true });
    git(['update-index', '--add', '--cacheinfo', `160000,${'1'.repeat(40)},${rel}`]);
  }
  return osSymlinks;
}

function runSelfTest() {
  let failed = 0;
  let skipped = 0;
  for (const tc of SELF_TEST_CASES) {
    const dir = mkdtempSync(join(tmpdir(), 'oac-herdr-selftest-'));
    try {
      execFileSync('git', ['init', '-q'], { cwd: dir });
      writeTree(dir, tc.files);
      execFileSync('git', ['add', '-A'], { cwd: dir });
      const osSymlinks = plantIndexEntries(dir, tc);
      if (tc.untracked) writeTree(dir, tc.untracked);
      if (tc.needsOsSymlinks && !osSymlinks) {
        skipped += 1;
        console.log(`  skip  ${tc.name}  (this checkout cannot create symlinks)`);
        continue;
      }
      const run = spawnSync(process.execPath, [scriptPath, '--root', dir], { encoding: 'utf8' });
      const resultLine = (run.stdout.match(/^Result: (\w+)/m) ?? [])[1];
      const hitLines = (run.stdout.match(/^ {13}FAIL {2}/gm) ?? []).length;
      const ok =
        tc.expect === 'fail'
          ? run.status === 1 && resultLine === 'FAIL' && hitLines === 1
          : run.status === 0 && resultLine === tc.expect.toUpperCase();
      if (!ok) failed += 1;
      console.log(`  ${ok ? 'ok  ' : 'FAIL'}  ${tc.name}  (exit ${run.status}, Result: ${resultLine ?? 'none'})`);
      if (!ok) console.log(run.stdout + run.stderr);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }
  const total = SELF_TEST_CASES.length;
  const violations = SELF_TEST_CASES.filter((tc) => tc.expect === 'fail').length;
  console.log(
    `\nself-test: ${total - failed - skipped}/${total} cases as expected` +
      (skipped ? `, ${skipped} skipped` : '') +
      ` (${violations} planted violations must exit 1 with exactly one hit; ` +
      `${total - violations} controls must exit 0).`,
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
