#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// Module containment lints (#61, F12): oac-boundaries checks 12 and 13
// (.claude/skills/oac-boundaries/references/mechanical-checks.md).
//
//   node scripts/check-containment.mjs              # check this repository
//   node scripts/check-containment.mjs --root <dir> # check another git work tree
//   node scripts/check-containment.mjs --self-test  # plant violations, assert each fails
//
// Check 12 (Zenoh containment; DESIGN acceptance criterion 9; 07 section 4(a); C7 section 2;
//          [ADR-001 Boundary] "MUST NOT leak Zenoh-specific concepts into the neutral
//          protocol"): no Zenoh name, `zid`, key expression or liveliness term in any
//          git-tracked entry outside transports/zenoh/, over the ZENOH_SCOPE below. It
//          closes the scope gap 09 section 8 and C7 section 2 record for check 1:
//            - `zenoh` matches anywhere, so a leak embedded in a longer identifier
//              (`x_zenoh_helper`, `ZenohSession`) fails;
//            - `zid` matches as a snake_case, kebab-case or camelCase identifier segment
//              (`session_zid`, `sessionZid`, `SESSION_ZID`, `zidMap`, `ZidMap`), not
//              inside a word (`zidane`);
//            - adapters/ and cli/ are in scope, as are transports/memory/, spec/, the fakes,
//              the contract suites, the conformance fixtures and the root manifests.
//          Comments and prose count: a path or line that names Zenoh outside its module is a
//          hit. The one allowance is the module's own name (`oac-transport-zenoh`,
//          `oac_transport_zenoh`, `transports/zenoh`), which names the OAC crate, not a Zenoh
//          concept: cli/ constructs it (07 section 3) and the workspace lists it. A Zenoh
//          type reached through it (`oac_transport_zenoh::ZenohConfig`) still fails.
//          Not proven (a text lint cannot see these): a Zenoh type behind an alias whose
//          name avoids every pattern; a name built at compile time (`concat!`, `stringify!`
//          pieces, a macro pasting `ze` and `noh`) or at run time; `\u{..}` or other escapes
//          spelling it; and homoglyphs (a Cyrillic `е` in `zеnoh`).
//          Not scanned: tests/security/ and tests/integration/, which compose a real
//          transport in later stages (H1, H2); docs/, scripts/, tools/ and .github/, which
//          are not the product. Their dependency edges are scripts/check-crate-deps.mjs's.
// Check 13 (test-double containment; PR #318 review item 9): no product path (core/, cli/,
//          adapters/, transports/) refers to tests/fakes/ or tests/protocol/contract/ by path
//          except as a Cargo dev-dependency. A `#[path]`, `include!`, `include_str!`, JS
//          import or path string reaching a fake from product code fails; so does a
//          Cargo.toml naming one under [dependencies] or [build-dependencies]. A crate's own
//          test targets (a `tests/` or `benches/` directory inside the crate) may name a fake
//          by path to spawn it, the intended use (tests/fakes/codex-app-server/README.md),
//          but may not compile or import it in (`#[path]`, `include*!`, `import`,
//          `require(`). Leading
//          comment lines (`//` in Rust and JS, `#` in TOML) and Markdown are not code and
//          are skipped. scripts/check-crate-deps.mjs rule 5 already keeps the fake crates
//          out of every product build over the cargo graph; this check covers the routes
//          cargo does not see.
// Both checks: the scan is the git index (what CI sees), not the work tree. A symlink or a
// submodule anywhere in a check's scope fails closed (nothing in this repository uses
// either; follow-and-match would be the change to make if one is ever needed). An empty
// scope is an error, not a pass.
//
// Exit codes: 0 = clean; 1 = at least one violation (or a failed self-test case);
// 2 = usage or environment error. Node built-ins and git only.

import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptPath = fileURLToPath(import.meta.url);
const repoRoot = resolve(dirname(scriptPath), '..');

// ---- check 12: Zenoh containment ------------------------------------------------------

const ZENOH_HOME = 'transports/zenoh/';
const ZENOH_SCOPE = [
  'core/',
  'cli/',
  'adapters/',
  'transports/',
  'spec/',
  'tests/fakes/',
  'tests/protocol/',
  'Cargo.toml',
  'Cargo.lock',
];
// The module's own names: removed before matching, so they alone never hit.
const ZENOH_MODULE_NAME = /oac[-_]transport[-_]zenoh|transports[\\/]+zenoh(?![\w-])/gi;
const ZENOH_RULES = [
  { label: '12 zenoh name', re: /zenoh/i },
  // A leading or standalone segment: snake_case, kebab-case, whole word, SCREAMING_CASE,
  // and the first segment of camelCase / PascalCase (`zidMap`, `ZidMap`). Case-sensitive, so
  // the next character may be an uppercase letter (a new segment) but not a lowercase one
  // (`zidane` is a word, not a segment).
  { label: '12 zid identifier', re: /(?<![A-Za-z])(?:zid|Zid)(?![a-z])/ },
  // All-caps: SCREAMING_CASE (`PEER_ZID`) and an acronym before a PascalCase segment
  // (`ZIDMap`), but not an all-caps word that merely starts with the letters (`ZIDANE`,
  // #332): an uppercase letter after `ZID` must itself start a lowercase segment.
  { label: '12 zid identifier', re: /(?<![A-Za-z])ZID(?![a-z]|[A-Z](?![a-z]))/ },
  // camelCase / PascalCase segment: `sessionZid`, `peerZidOf`.
  { label: '12 zid identifier', re: /(?<=[a-z0-9])Zid(?![a-z])/ },
  { label: '12 key expression', re: /key[\s_-]*expr/i },
  { label: '12 liveliness', re: /liveliness/i },
];

// ---- check 13: test-double containment ------------------------------------------------

const PRODUCT_CODE = ['core/', 'cli/', 'adapters/', 'transports/'];
const TEST_DOUBLE_REF = /tests[\\/]+(?:fakes|protocol[\\/]+contract)(?![\w-])/i;
// A Cargo table header whose entries are dev-dependencies: [dev-dependencies],
// [dev-dependencies.name], [target.'cfg(...)'.dev-dependencies] and its dotted form.
const DEV_DEPS_TABLE = /^\[(?:target\.(?:'[^']*'|"[^"]*"|[^.\]]+)\.)?dev-dependencies(?:\.[^\]]+)?\]$/;
// A product crate's own test target: <crate>/tests/... or <crate>/benches/...
const CRATE_TEST_FILE = /^(?:core|cli|adapters\/[^/]+|transports\/[^/]+)\/(?:tests|benches)\//;
// Forms that compile or import another file's code in, rather than naming a path to spawn.
const COMPILE_IN = /#\s*!?\s*\[\s*path\b|\binclude(?:_str|_bytes)?\s*!|\bimport\b|\brequire\s*\(/;

function leadingComment(path) {
  if (/\.(?:rs|m?js|ts)$/.test(path)) return /^\s*\/\//;
  if (/\.toml$/.test(path)) return /^\s*#/;
  return null;
}

// ---- git index ------------------------------------------------------------------------

function git(root, args, input) {
  return execFileSync('git', args, {
    cwd: root,
    input,
    maxBuffer: 1024 * 1024 * 1024,
    stdio: ['pipe', 'pipe', 'pipe'],
  });
}

function indexEntries(root) {
  const out = git(root, ['ls-files', '-s', '-z']);
  const entries = [];
  let pos = 0;
  while (pos < out.length) {
    const end = out.indexOf(0, pos);
    const rec = out.subarray(pos, end === -1 ? out.length : end);
    pos = end === -1 ? out.length : end + 1;
    if (rec.length === 0) continue;
    const tab = rec.indexOf(0x09);
    const [mode, sha] = rec.subarray(0, tab).toString('latin1').split(' ');
    entries.push({ mode, sha, path: rec.subarray(tab + 1).toString('utf8') });
  }
  return entries;
}

function readBlobs(root, shas) {
  const unique = [...new Set(shas)];
  const blobs = new Map();
  if (unique.length === 0) return blobs;
  const out = git(root, ['cat-file', '--batch'], `${unique.join('\n')}\n`);
  let pos = 0;
  for (let n = 0; n < unique.length; n++) {
    const nl = out.indexOf(0x0a, pos);
    const [sha, type, size] = out.toString('latin1', pos, nl).split(' ');
    pos = nl + 1;
    if (type === 'missing' || size === undefined) continue;
    blobs.set(sha, out.subarray(pos, pos + Number(size)));
    pos += Number(size) + 1;
  }
  return blobs;
}

const inScope = (path, scope) => scope.some((s) => (s.endsWith('/') ? path.startsWith(s) : path === s));

// ---- the checks -----------------------------------------------------------------------

function excerpt(line) {
  const t = line.trim();
  return t.length > 120 ? `${t.slice(0, 117)}...` : t;
}

function zenohHits(text) {
  const rest = text.replace(ZENOH_MODULE_NAME, '');
  return ZENOH_RULES.filter((r) => r.re.test(rest)).map((r) => r.label);
}

function check(root) {
  const entries = indexEntries(root);
  const zenoh = entries.filter((e) => inScope(e.path, ZENOH_SCOPE) && !e.path.startsWith(ZENOH_HOME));
  const product = entries.filter((e) => inScope(e.path, PRODUCT_CODE));
  if (zenoh.length === 0 || product.length === 0) {
    return { error: `empty scope (check 12: ${zenoh.length} entries, check 13: ${product.length} entries)` };
  }
  const blobs = readBlobs(
    root,
    [...zenoh, ...product].filter((e) => e.mode.startsWith('100')).map((e) => e.sha),
  );
  const hits = [];
  const add = (path, line, label, text) => hits.push({ path, line, label, text });

  for (const e of zenoh) {
    if (!e.mode.startsWith('100')) {
      add(e.path, 0, '12 symlink or submodule in scope (fails closed)', '');
      continue;
    }
    for (const label of new Set(zenohHits(e.path))) add(e.path, 0, `${label} (path)`, e.path);
    const lines = (blobs.get(e.sha) ?? Buffer.alloc(0)).toString('utf8').split(/\r?\n/);
    lines.forEach((l, i) => {
      for (const label of new Set(zenohHits(l))) add(e.path, i + 1, label, l);
    });
  }

  for (const e of product) {
    if (!e.mode.startsWith('100')) {
      add(e.path, 0, '13 symlink or submodule in a product path (fails closed)', '');
      continue;
    }
    if (e.path.endsWith('.md')) continue;
    const comment = leadingComment(e.path);
    const isManifest = /(?:^|\/)Cargo\.toml$/.test(e.path);
    const isTest = CRATE_TEST_FILE.test(e.path);
    let devTable = false;
    const lines = (blobs.get(e.sha) ?? Buffer.alloc(0)).toString('utf8').split(/\r?\n/);
    lines.forEach((l, i) => {
      if (isManifest && /^\s*\[/.test(l)) devTable = DEV_DEPS_TABLE.test(l.replace(/#.*$/, '').trim());
      if (comment && comment.test(l)) return;
      if (!TEST_DOUBLE_REF.test(l)) return;
      if (isManifest && devTable) return;
      if (isManifest) return add(e.path, i + 1, '13 test double outside [dev-dependencies]', l);
      // A crate's test target may name a fake to spawn it; it may not compile one in. The
      // line before counts too, for a macro call wrapped over two lines.
      if (isTest && !COMPILE_IN.test(`${lines[i - 1] ?? ''}\n${l}`)) return;
      add(e.path, i + 1, isTest ? '13 test double compiled into a crate test' : '13 test double referenced from product code', l);
    });
  }
  return { hits, zenoh: zenoh.length, product: product.length };
}

function report(result) {
  if (result.error) {
    console.error(`check-containment: ${result.error}`);
    return 2;
  }
  for (const h of result.hits) {
    console.log(`${h.path}:${h.line} [${h.label}]${h.text ? ` ${excerpt(h.text)}` : ''}`);
  }
  const z = result.hits.filter((h) => h.label.startsWith('12')).length;
  const f = result.hits.filter((h) => h.label.startsWith('13')).length;
  console.log(`check 12 (Zenoh containment): ${z === 0 ? 'clean' : `${z} hit(s)`} over ${result.zenoh} tracked entries outside ${ZENOH_HOME}`);
  console.log(`check 13 (test-double containment): ${f === 0 ? 'clean' : `${f} hit(s)`} over ${result.product} tracked entries in product paths`);
  if (result.hits.length === 0) {
    console.log('Result: CLEAN');
    return 0;
  }
  console.log('Result: FAIL -- stop and cite the boundary (oac-boundaries)');
  return 1;
}

// ---- self-test ------------------------------------------------------------------------

// A minimal tree that passes: every scope non-empty, with the allowed shapes present.
const BASE = {
  'Cargo.toml': '[workspace]\nmembers = ["core", "cli", "transports/zenoh", "tests/fakes/claude"]\n',
  'Cargo.lock': '[[package]]\nname = "oac-transport-zenoh"\n',
  'core/src/lib.rs': '//! neutral core\npub struct Envelope;\n',
  'cli/Cargo.toml': '[dependencies]\noac-transport-zenoh = { path = "../transports/zenoh" }\n',
  'cli/src/main.rs': 'use oac_transport_zenoh as transport;\nfn main() {}\n',
  'transports/zenoh/src/lib.rs': 'use zenoh::key_expr::KeyExpr;\n// liveliness tokens, zid\n',
  'transports/memory/Cargo.toml':
    '[dependencies]\noac-core = { path = "../../core" }\n\n[dev-dependencies]\n' +
    'oac-fake-claude = { path = "../../tests/fakes/claude" }\n' +
    "[target.'cfg(unix)'.dev-dependencies]\noac-contract-transport = { path = \"../../tests/protocol/contract/transport\" }\n",
  'transports/memory/tests/contract.rs': '//! runs tests/protocol/contract/transport/ against this transport\nfn t() {}\n',
  'transports/memory/README.md': 'See tests/fakes/claude for the fake.\n',
  'transports/memory/tests/spawn.rs':
    'let fake = Command::new("node")\n    .arg("../../tests/fakes/codex-app-server/server.mjs");\n',
  'adapters/claude/src/lib.rs': '// a zidane-free file; validation, keyboard, expression, livelihood\nfn f() {}\n',
  // An all-caps word is not a zid segment (#332).
  'core/src/words.rs': '// ZIDANE, Zidane and zidane are names, not zids\nconst ZIDANE: u8 = 10;\n',
  'spec/session-channels.md': 'Neutral text. A key, an expression, lively.\n',
  'tests/fakes/claude/src/lib.rs': 'pub struct Fake;\n',
  'tests/security/src/lib.rs': '// may name zenoh: out of check 12 scope\n',
  'tests/integration/README.md': 'Zenoh end to end (opt-in).\n',
  'docs/notes.md': 'zenoh zid liveliness key expression tests/fakes\n',
};

const CASES = [
  ['12 zenoh in adapters', { 'adapters/codex/src/lib.rs': 'use zenoh::Session;\n' }, '12 zenoh name'],
  ['12 embedded zenoh identifier in core', { 'core/src/x.rs': 'fn x_zenoh_helper() {}\n' }, '12 zenoh name'],
  ['12 PascalCase Zenoh type in cli', { 'cli/src/run.rs': 'let c: ZenohConfig;\n' }, '12 zenoh name'],
  ['12 Zenoh type through the module name', { 'cli/src/t.rs': 'use oac_transport_zenoh::ZenohSession;\n' }, '12 zenoh name'],
  ['12 zenoh in a comment in transports/memory', { 'transports/memory/src/lib.rs': '// like zenoh\n' }, '12 zenoh name'],
  ['12 snake_case zid', { 'core/src/id.rs': 'let session_zid = 1;\n' }, '12 zid identifier'],
  ['12 camelCase zid', { 'tests/fakes/claude/src/a.rs': 'let sessionZid = 1;\n' }, '12 zid identifier'],
  ['12 leading camelCase zid', { 'cli/src/m.rs': 'let zidMap = 1;\n' }, '12 zid identifier'],
  ['12 leading PascalCase Zid', { 'core/src/m.rs': 'struct ZidMap;\n' }, '12 zid identifier'],
  ['12 kebab-case zid', { 'spec/y.md': 'The peer-zid value.\n' }, '12 zid identifier'],
  ['12 SCREAMING zid', { 'adapters/claude/src/k.rs': 'const PEER_ZID: u8 = 1;\n' }, '12 zid identifier'],
  ['12 leading SCREAMING zid', { 'adapters/claude/src/k.rs': 'const ZID_LEN: u8 = 16;\n' }, '12 zid identifier'],
  ['12 all-caps ZID acronym before a segment', { 'core/src/m.rs': 'struct ZIDMap;\n' }, '12 zid identifier'],
  ['12 bare all-caps ZID', { 'spec/z.md': 'The ZID of the peer.\n' }, '12 zid identifier'],
  ['12 key_expr', { 'adapters/claude/src/k.rs': 'let key_expr = "a/b";\n' }, '12 key expression'],
  ['12 KeyExpr', { 'core/src/k.rs': 'struct KeyExpr;\n' }, '12 key expression'],
  ['12 key expression in spec prose', { 'spec/x.md': 'Each key expression names a session.\n' }, '12 key expression'],
  ['12 liveliness', { 'cli/src/p.rs': 'let liveliness = 1;\n' }, '12 liveliness'],
  ['12 zenoh in a fixture', { 'tests/protocol/sc-env/X.json': '{"note":"zenoh"}\n' }, '12 zenoh name'],
  ['12 zenoh in a path name', { 'adapters/codex/src/zenoh_bridge.rs': 'fn f() {}\n' }, '12 zenoh name (path)'],
  ['12 zenoh crate in the root lock', { 'Cargo.lock': '[[package]]\nname = "zenoh"\n' }, '12 zenoh name'],
  ['12 zenoh under transports/zenoh-ish sibling', { 'transports/zenohx/src/lib.rs': 'fn f() {}\n' }, '12 zenoh name (path)'],
  ['13 #[path] into a fake', { 'core/src/lib.rs': '#[path = "../../tests/fakes/claude/src/lib.rs"]\nmod fake;\n' }, '13 test double referenced from product code'],
  ['13 include_str! of a contract suite file', { 'adapters/codex/src/x.rs': 'const S: &str = include_str!("../../../tests/protocol/contract/adapter/src/lib.rs");\n' }, '13 test double referenced from product code'],
  ['13 JS import of the fake Codex', { 'cli/tools/x.mjs': "import '../../tests/fakes/codex-app-server/server.mjs';\n" }, '13 test double referenced from product code'],
  ['13 fake as a normal dependency', { 'adapters/claude/Cargo.toml': '[dependencies]\noac-fake-claude = { path = "../../tests/fakes/claude" }\n' }, '13 test double outside [dev-dependencies]'],
  ['13 fake as a build dependency after dev', { 'adapters/claude/Cargo.toml': '[dev-dependencies]\nx = "1"\n[build-dependencies]\noac-fake-claude = { path = "../../tests/fakes/claude" }\n' }, '13 test double outside [dev-dependencies]'],
  ['13 #[path] into a fake from a crate test', { 'transports/memory/tests/x.rs': '#[path = "../../../tests/fakes/claude/src/lib.rs"]\nmod fake;\n' }, '13 test double compiled into a crate test'],
  ['13 include_str! wrapped over two lines in a crate test', { 'core/tests/x.rs': 'const S: &str = include_str!(\n    "../../tests/fakes/claude/src/lib.rs");\n' }, '13 test double compiled into a crate test'],
  ['13 Windows-style path', { 'cli/src/x.rs': 'let p = "..\\\\..\\\\tests\\\\fakes\\\\claude";\n' }, '13 test double referenced from product code'],
];

function plantTree(files) {
  const dir = mkdtempSync(join(tmpdir(), 'oac-containment-'));
  for (const [p, body] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, p)), { recursive: true });
    writeFileSync(join(dir, p), body);
  }
  git(dir, ['init', '-q']);
  git(dir, ['add', '-A']);
  return dir;
}

function runSelfTest() {
  let failed = 0;
  const run = (name, files, expect) => {
    const dir = plantTree(files);
    try {
      const r = check(dir);
      const labels = r.error ? [`error: ${r.error}`] : r.hits.map((h) => h.label);
      const ok = expect === null ? labels.length === 0 : labels.length === 1 && labels[0] === expect;
      console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ` -> ${JSON.stringify(labels)}`}`);
      if (!ok) failed++;
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  };
  run('control: the base tree is clean', BASE, null);
  for (const [name, plant, expect] of CASES) run(name, { ...BASE, ...plant }, expect);
  // An empty scope is an error, never a pass.
  {
    const dir = plantTree({ 'docs/x.md': 'x\n' });
    try {
      const r = check(dir);
      const ok = Boolean(r.error);
      console.log(`${ok ? 'ok  ' : 'FAIL'} empty scope is an error`);
      if (!ok) failed++;
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }
  const total = CASES.length + 2;
  console.log(`self-test: ${total - failed}/${total} passed`);
  return failed === 0 ? 0 : 1;
}

// ---- main -----------------------------------------------------------------------------

const args = process.argv.slice(2);
if (args[0] === '--self-test' && args.length === 1) {
  process.exit(runSelfTest());
}
let root = repoRoot;
if (args[0] === '--root' && args.length === 2) root = resolve(args[1]);
else if (args.length !== 0) {
  console.error('usage: node scripts/check-containment.mjs [--root <dir> | --self-test]');
  process.exit(2);
}
try {
  process.exit(report(check(root)));
} catch (err) {
  console.error(`check-containment: ${err.message}`);
  process.exit(2);
}
