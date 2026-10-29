#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// No product path names a harness's credential file, database, rollout file or configuration
// directory (G10 #71 slice 5, G7 #68 acceptance 4; ADR-001 Boundary: no stealing or reusing
// another harness's credentials, no rollout-file manipulation, no private surfaces). `oac
// doctor` and the Codex carrier are the places that could be tempted; the adapters are where
// the boundary matters most.
//
//   node scripts/check-doctor-boundary.mjs              # check this repository
//   node scripts/check-doctor-boundary.mjs --root <dir> # check another git work tree
//   node scripts/check-doctor-boundary.mjs --self-test  # plant violations, assert each fails
//
// Two rule sets over git-tracked .rs files:
//
// 1. TEXT rules, cli/src only. Forbidden, case-insensitively, anywhere in the text (comments
//    included, so that a doc comment cannot hide a read):
//      auth.json   .credentials.json   .sqlite   CODEX_HOME   CLAUDE_CONFIG_DIR
//      .claude   .codex   (the default locations of a harness's own files; `.claude` also
//      catches `.claude.json` and `.claude/`)
// 2. CODE rules, every product source path: adapters/*/src, cli/src, core/src,
//    transports/*/src. Comments are NOT scanned (a doc comment that says "never reads
//    auth.json" is the boundary, not a breach); strings, identifiers and attributes are.
//    Forbidden, case-insensitively (CODE_RULES): a rollout file, auth.json, .credentials.json,
//    .claude.json, *.sqlite, the Codex home (CODEX_HOME, a `.codex` path segment, a
//    `.join("sessions")`), CLAUDE_CONFIG_DIR and, outside cli/src (which holds OAC's own
//    device key store), an OS credential store (keyring, keychain, Credential Manager, Secret
//    Service). The comment stripper is a small Rust lexer (line, nested block, string, raw
//    string and char literal aware); a `//` inside a string is not a comment.
//
// ALLOW lists the legitimate mentions, one exact trimmed line each with its reason, for either
// rule set. An entry that no longer matches fails (stale). Its entries are a struct field
// access that is not a path and, in cli/src/codex.rs alone, the two constants that build the
// Codex control socket's path. Not caught: a path assembled at run time from pieces, or a
// name hidden by a macro or an escape; read the diff for those. Only src/ is scanned; a
// crate's tests/ directory is not product code.
//
// Exit codes: 0 = clean; 1 = a violation (or a failed self-test case); 2 = usage or
// environment error. Node built-ins and git only.

import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptPath = fileURLToPath(import.meta.url);
const repoRoot = resolve(dirname(scriptPath), '..');

export const FORBIDDEN = ['auth.json', '.credentials.json', '.sqlite', 'CODEX_HOME', 'CLAUDE_CONFIG_DIR', '.claude', '.codex'];
// { file, line (exact, trimmed), reason }
export const ALLOW = [
  // The Codex control socket's path (G8 S5, #433; lead approval 2026-10-21, approval 2): ONE module,
  // cli/src/codex.rs, names the Codex home variable and its default directory, on exactly these
  // two lines, to build `<home>/app-server-control/app-server-control.sock`. The module reads
  // metadata of, and connects to, that one path and nothing else under the home: no credential
  // file, database, rollout or configuration. No other file in cli/src may name either.
  { file: 'cli/src/codex.rs', line: 'const HOME_VARIABLE: &str = "CODEX_HOME";', reason: 'the environment variable that locates the control socket (approval 2)' },
  { file: 'cli/src/codex.rs', line: 'const HOME_DIRECTORY: &str = ".codex";', reason: 'the default home directory name, to locate the control socket only (approval 2)' },
  // The `claude` field of the doctor's probes (`claude --version`), not a path.
  { file: 'cli/src/doctor.rs', line: 'match (probes.claude)() {', reason: 'a struct field named claude, not the .claude directory' },
];

// The product source paths the CODE rules cover. `ownKeyStore`: OAC's own device key store
// lives here, so the OS credential store rule does not apply.
export const CODE_SCOPES = [
  { name: 'adapters/*/src', re: /^adapters\/[^/]+\/src\// },
  { name: 'cli/src', re: /^cli\/src\//, ownKeyStore: true },
  { name: 'core/src', re: /^core\/src\// },
  { name: 'transports/*/src', re: /^transports\/[^/]+\/src\// },
];
// A `.codex` path segment, not a field access (`inner.codex_version`, `x().codex`).
const DOT = String.raw`(?:^|[^A-Za-z0-9_)\]])`;
const END = String.raw`(?![A-Za-z0-9_])`;
export const CODE_RULES = [
  { label: 'a rollout file', re: /rollout/i },
  { label: 'auth.json', re: /auth\.json/i },
  { label: '.credentials.json', re: /\.credentials\.json/i },
  { label: '.claude.json', re: /\.claude\.json/i },
  { label: 'a .sqlite database', re: /\.sqlite/i },
  { label: 'CODEX_HOME', re: /codex_home/i },
  { label: 'CLAUDE_CONFIG_DIR', re: /claude_config_dir/i },
  { label: 'a .codex path', re: new RegExp(DOT + String.raw`\.codex` + END, 'i') },
  { label: 'a sessions directory join', re: /\.join\(\s*"(?:archived_)?sessions"/i },
  { label: 'an OS credential store', re: /keyring|keychain|credential[ _-]?manager|secret[ _-]?service|libsecret|wincred|find-generic-password/i, notIn: 'ownKeyStore' },
];

// `text` with every Rust comment replaced by spaces (newlines kept, so line numbers hold).
// Line comments (`//`, `///`, `//!`), nested block comments, "..." strings with escapes, raw
// strings (r".." r#".."# br#".."#) and char literals ('"') are tracked, so a comment marker
// inside a literal does not count and a quote inside a char literal does not open a string.
export function stripComments(text) {
  let out = '';
  let i = 0;
  const n = text.length;
  const blank = (c) => (c === '\n' ? '\n' : ' ');
  while (i < n) {
    const c = text[i];
    const d = text[i + 1];
    const raw = /^b?r(#*)"/.exec(text.slice(i, i + 40));
    if (c === '/' && d === '/') {
      while (i < n && text[i] !== '\n') { out += ' '; i += 1; }
    } else if (c === '/' && d === '*') {
      let depth = 0;
      do {
        if (text[i] === '/' && text[i + 1] === '*') { depth += 1; out += '  '; i += 2; }
        else if (text[i] === '*' && text[i + 1] === '/') { depth -= 1; out += '  '; i += 2; }
        else { out += blank(text[i]); i += 1; }
      } while (i < n && depth > 0);
    } else if (raw && !/[A-Za-z0-9_]/.test(text[i - 1] ?? ' ')) {
      const close = '"' + raw[1];
      const end = text.indexOf(close, i + raw[0].length);
      const stop = end < 0 ? n : end + close.length;
      out += text.slice(i, stop);
      i = stop;
    } else if (c === '"') {
      let j = i + 1;
      while (j < n && text[j] !== '"') j += text[j] === '\\' ? 2 : 1;
      const stop = Math.min(j + 1, n);
      out += text.slice(i, stop);
      i = stop;
    } else if (c === "'") {
      const m = /^'(?:\\(?:u\{[0-9A-Fa-f_]+\}|x[0-9A-Fa-f]{2}|.)|[^\\'\n])'/.exec(text.slice(i, i + 14));
      if (m) { out += m[0]; i += m[0].length; } else { out += c; i += 1; }
    } else {
      out += c;
      i += 1;
    }
  }
  return out;
}

export function scan(root, allow = ALLOW, { requireScopes = false } = {}) {
  const tracked = execFileSync('git', ['-C', root, 'ls-files', '--', 'adapters', 'cli', 'core', 'transports'], { encoding: 'utf8' })
    .split('\n').filter((f) => f.endsWith('.rs'));
  const hits = [];
  const used = new Set();
  const seen = new Set();
  for (const file of tracked) {
    const scope = CODE_SCOPES.find((s) => s.re.test(file));
    if (!scope) continue;
    seen.add(scope.name);
    const source = readFileSync(join(root, file), 'utf8');
    const lines = source.split('\n');
    const code = stripComments(source).split('\n');
    lines.forEach((text, i) => {
      const names = [];
      if (scope.ownKeyStore) {
        const lower = text.toLowerCase();
        for (const word of FORBIDDEN) if (lower.includes(word.toLowerCase())) names.push(word);
      }
      for (const rule of CODE_RULES) {
        if (rule.notIn && scope[rule.notIn]) continue;
        if (rule.re.test(code[i] ?? '')) names.push(rule.label);
      }
      if (!names.length) return;
      const idx = allow.findIndex((a) => a.file === file && a.line === text.trim());
      if (idx >= 0) { used.add(idx); return; }
      hits.push(`${file}:${i + 1}: names ${[...new Set(names)].join(', ')}`);
    });
  }
  if (requireScopes) {
    for (const scope of CODE_SCOPES) if (!seen.has(scope.name)) hits.push(`empty scope: no tracked .rs file under ${scope.name}`);
  }
  allow.forEach((a, i) => { if (!used.has(i)) hits.push(`stale allow entry: ${a.file}: ${a.line}`); });
  return hits;
}

function selfTest() {
  const dir = mkdtempSync(join(tmpdir(), 'oac-doctor-boundary-'));
  const git = (...args) => execFileSync('git', ['-C', dir, ...args], { stdio: 'pipe' });
  let failed = 0;
  const expect = (name, ok) => { if (!ok) { failed += 1; console.error(`self-test FAILED: ${name}`); } };
  try {
    git('init', '-q');
    // `put(text, file)` replaces the tree with one file.
    const put = (text, file = 'cli/src/x.rs') => {
      for (const d of ['cli', 'core', 'adapters', 'transports', 'tests']) rmSync(join(dir, d), { recursive: true, force: true });
      mkdirSync(join(dir, dirname(file)), { recursive: true });
      writeFileSync(join(dir, file), text);
      git('add', '-A');
    };
    const hitsOf = (allow = []) => scan(dir, allow);

    // ---- cli/src, TEXT rules (comments included) ----
    put('fn clean() {}\n');
    expect('a clean tree passes', hitsOf().length === 0);
    for (const word of FORBIDDEN) {
      put(`// reads ${word}\n`);
      expect(`${word} in a comment fails`, hitsOf().length === 1);
      put(`let p = home.join("${word.toUpperCase()}");\n`);
      expect(`${word} in code (any case) fails`, hitsOf().length === 1);
    }
    for (const word of ['codex_home', 'claude_config_dir', 'Codex_Home']) {
      put(`std::env::var_os("${word}");\n`);
      expect(`${word} (other case) fails`, hitsOf().length === 1);
    }
    put('let a = "auth.json";\n');
    expect('an allowed exact line passes', hitsOf([{ file: 'cli/src/x.rs', line: 'let a = "auth.json";', reason: 't' }]).length === 0);
    put('fn clean() {}\n');
    expect('a stale allow entry fails', hitsOf([{ file: 'cli/src/x.rs', line: 'gone', reason: 't' }]).length === 1);
    put('const HOME_VARIABLE: &str = "CODEX_HOME";\n');
    expect('the codex.rs allowance does not cover another file', hitsOf([{ file: 'cli/src/codex.rs', line: 'const HOME_VARIABLE: &str = "CODEX_HOME";', reason: 't' }]).length === 2);
    put('const HOME_VARIABLE: &str = "CODEX_HOME";\n', 'cli/src/codex.rs');
    expect('the real two-line allowance shape covers codex.rs', hitsOf([{ file: 'cli/src/codex.rs', line: 'const HOME_VARIABLE: &str = "CODEX_HOME";', reason: 't' }]).length === 0);
    put('fn clean() {}\n');
    put('// auth.json\n', 'tests/y.rs');
    expect('files outside the product source paths are not scanned', hitsOf().length === 0);
    put('// auth.json\n', 'adapters/codex/tests/y.rs');
    expect('an adapter tests/ file is not scanned', hitsOf().length === 0);
    put('std::fs::read(keyring_path);\n');
    expect('cli/src may name its own credential store (the device key store)', hitsOf().length === 0);

    // ---- CODE rules, every product source path ----
    const SCOPE_FILES = ['adapters/codex/src/x.rs', 'adapters/claude/src/x.rs', 'core/src/x.rs', 'transports/zenoh/src/x.rs', 'transports/memory/src/x.rs', 'cli/src/x.rs'];
    // one planted violation per pattern; `scopes` limits where it applies
    const PLANTED = [
      ['a rollout file', 'let p = dir.join("rollout-2026-10-29T10-00-00-abc.jsonl");'],
      ['a rollout identifier', 'let rollout_path = find();'],
      ['auth.json', 'let p = home.join("auth.json");'],
      ['auth.json (any case)', 'let p = home.join("Auth.JSON");'],
      ['.credentials.json', 'let p = dir.join(".credentials.json");'],
      ['.claude.json', 'let p = home.join(".claude.json");'],
      ['a .sqlite database', 'let p = home.join("state_5.sqlite");'],
      ['CODEX_HOME', 'let h = std::env::var_os("CODEX_HOME");'],
      ['CODEX_HOME (any case)', 'let h = std::env::var_os("codex_home");'],
      ['CLAUDE_CONFIG_DIR', 'let h = std::env::var_os("CLAUDE_CONFIG_DIR");'],
      ['a .codex path', 'let h = user.join(".codex");'],
      ['a .codex path (inside a longer string)', 'let h = format!("{}/.codex/sessions", user);'],
      ['a sessions directory join', 'let s = home.join("sessions");'],
      ['an archived sessions directory join', 'let s = home.join( "archived_sessions" );'],
    ];
    // One tree per scope, one file per planted pattern: each file must be reported once.
    const putAll = (files) => {
      for (const d of ['cli', 'core', 'adapters', 'transports', 'tests']) rmSync(join(dir, d), { recursive: true, force: true });
      for (const [file, text] of Object.entries(files)) {
        mkdirSync(join(dir, dirname(file)), { recursive: true });
        writeFileSync(join(dir, file), text);
      }
      git('add', '-A');
    };
    const STORES = ['keyring::Entry::new(s, a)', 'security_framework::keychain', 'CredentialManager', 'Secret Service', 'libsecret', 'find-generic-password'];
    for (const file of SCOPE_FILES) {
      const base = dirname(file);
      const own = file.startsWith('cli/');
      const files = {};
      const names = [];
      PLANTED.forEach(([name, line], i) => { files[`${base}/p${i}.rs`] = `${line}\n`; names.push(name); });
      // the OS credential store rule applies outside cli/src only
      STORES.forEach((word, i) => { files[`${base}/s${i}.rs`] = `let x = "${word}";\n`; names.push(word); });
      putAll(files);
      const hits = hitsOf();
      Object.keys(files).forEach((f, i) => {
        const expected = own && /\/s\d+\.rs$/.test(f) ? 0 : 1;
        const got = hits.filter((h) => h.startsWith(`${f}:`)).length;
        expect(`${names[i]} in ${base} reported ${expected} time(s), got ${got}`, got === expected);
      });
    }
    // comments, however they are written, are not scanned outside cli/src
    const NEVER = [
      '// never reads auth.json, a rollout file, the keyring or CODEX_HOME',
      '/// Never touches `.codex/sessions`, `state_5.sqlite` or `.credentials.json`.',
      '//! The adapter holds no Codex credential (`auth.json`, the keyring).',
      'let x = 1; // never auth.json or a rollout file',
      '/* block: auth.json and rollout and CODEX_HOME */ let x = 1;',
      '/* nested /* auth.json */ still a comment: rollout */ let x = 1;',
    ];
    for (const file of SCOPE_FILES.filter((f) => !f.startsWith('cli/'))) {
      const base = dirname(file);
      const files = {};
      NEVER.forEach((line, i) => { files[`${base}/n${i}.rs`] = `${line}\nfn clean() {}\n`; });
      files[`${base}/m.rs`] = '/* line one\nauth.json\nline three */\nfn clean() {}\n';
      putAll(files);
      expect(`comments in ${base} pass: ${hitsOf().join(' | ')}`, hitsOf().length === 0);
      put('/* open\nauth.json\n*/ let p = home.join("auth.json");\n', file);
      expect(`a violation after a block comment in ${file} reports its own line`, hitsOf().join().includes(':3:') && hitsOf().length === 1);
    }
    // a comment marker inside a literal does not hide the rest of the line
    const literals = {};
    for (const line of [
      'let u = "http://x"; let p = home.join("auth.json");',
      'let c = \'"\'; let p = home.join("auth.json");',
      'let p = r#"// not a comment"#; let q = home.join("auth.json");',
      'let p = "a \\" // b"; let q = home.join("auth.json");',
      'let p = br"//"; let q = home.join("rollout-1");',
      '#[doc = "x"] let p = home.join("auth.json"); // trailing',
      // a lifetime is not a char literal
      "fn f<'a>(x: &'a str) { let p = home.join(\"auth.json\"); }",
    ]) {
      literals[`adapters/codex/src/l${Object.keys(literals).length}.rs`] = `${line}\n`;
    }
    putAll(literals);
    {
      const hits = hitsOf();
      Object.keys(literals).forEach((f) => expect(`a violation after a literal containing //, or a lifetime, fails (${f})`, hits.filter((h) => h.startsWith(`${f}:`)).length === 1));
    }
    // names that merely contain a word are not paths
    const innocent = {};
    for (const line of [
      'let v = self.inner.codex_version();',
      'let v = f().codex;',
      'let v = x[0].codex;',
      'let v = codex_version;',
      'let sessions = list.join(",");',
      'fn on_session_start() {}',
      'let u = "https://example.com/x";',
    ]) {
      innocent[`adapters/codex/src/i${Object.keys(innocent).length}.rs`] = `${line}\n`;
    }
    putAll(innocent);
    expect(`names that merely contain a word pass: ${hitsOf().join(' | ')}`, hitsOf().length === 0);
    // the allowances still work for the code rules, and a stale one fails
    put('const HOME_DIRECTORY: &str = ".codex";\n', 'cli/src/codex.rs');
    expect('the codex.rs line is allowed in cli/src', hitsOf([{ file: 'cli/src/codex.rs', line: 'const HOME_DIRECTORY: &str = ".codex";', reason: 't' }]).length === 0);
    put('const HOME_DIRECTORY: &str = ".codex";\n', 'adapters/codex/src/codex.rs');
    expect('the same line in an adapter fails', hitsOf([{ file: 'cli/src/codex.rs', line: 'const HOME_DIRECTORY: &str = ".codex";', reason: 't' }]).length === 2);
    put('let a = "rollout";\n', 'core/src/x.rs');
    expect('an allowed exact line passes in core/src', hitsOf([{ file: 'core/src/x.rs', line: 'let a = "rollout";', reason: 't' }]).length === 0);
    // an empty scope is an error when asked for (the real run does)
    put('fn clean() {}\n', 'core/src/x.rs');
    expect('requireScopes reports the empty scopes', scan(dir, [], { requireScopes: true }).filter((h) => h.startsWith('empty scope')).length === 3);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
  if (failed) process.exit(1);
  console.log('check-doctor-boundary self-test: ok');
}

const args = process.argv.slice(2);
if (args[0] === '--self-test') selfTest();
else {
  let root = repoRoot;
  if (args[0] === '--root' && args[1]) root = resolve(args[1]);
  else if (args.length) { console.error('usage: check-doctor-boundary.mjs [--root <dir>] [--self-test]'); process.exit(2); }
  const hits = scan(root, ALLOW, { requireScopes: true });
  if (hits.length) { console.error(hits.join('\n')); process.exit(1); }
  console.log('check-doctor-boundary: no product source path (adapters/*/src, cli/src, core/src, transports/*/src) names a harness credential file, database, rollout file or configuration directory');
}
