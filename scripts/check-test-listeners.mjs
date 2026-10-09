#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// Loopback-only listeners (refs #7): nothing the default local-ci tier runs may listen
// beyond loopback, except a test that is opt-in by an environment switch and checks it
// before it binds anything.
//
//   node scripts/check-test-listeners.mjs              # check this repository
//   node scripts/check-test-listeners.mjs --root <dir> # check another git work tree
//   node scripts/check-test-listeners.mjs --self-test  # plant violations, assert each fails
//
// Why: the Windows firewall does not filter loopback (127.0.0.1, ::1), but a program that
// listens on any other address (the unspecified address, a LAN interface address, a
// multicast group) raises an "allow access" prompt, and every cargo rebuild makes new test
// executables, so each rebuild prompts again. It is also the rule of oac-testing section 2
// and 09-test-strategy section 3: the default tier uses no network beyond loopback.
//
// Scope: every git-tracked .rs, .mjs, .js, .cjs, .ts, .py and .sh file under core/, cli/,
// adapters/, transports/, tests/, tools/ and scripts/ (product code is in scope too: the
// tests start it). Not scanned: docs/ (G3's recorded spike scripts), .github/local-ci.sh
// (its g3-macos section is the opt-in macOS tier, multicast by design). Comment lines (`//`
// in Rust and JS, `#` in Python and shell) are skipped. The scan is the git index, as in
// scripts/check-containment.mjs.
//
// Rules, one line at a time:
//   unspecified   the unspecified address in code: 0.0.0.0, [::], the UNSPECIFIED constant
//                 of Ipv4Addr/Ipv6Addr, INADDR_ANY, in6addr_any.
//   rust-bind     TcpListener::bind / UdpSocket::bind / TcpSocket::bind whose argument does
//                 not start with a literal loopback address ("127.0.0.1:...", "[::1]:...",
//                 (Ipv4Addr::LOCALHOST, ...), (Ipv6Addr::LOCALHOST, ...)). `localhost`
//                 counts as not loopback: what it binds depends on name resolution.
//   endpoint      a Zenoh-style locator ("tcp/...", "udp/...", "quic/...", "tls/...") whose
//                 host is not 127.0.0.1 or [::1].
//   zenoh-default a raw default Zenoh configuration (zenoh::Config::default): Zenoh's
//                 defaults listen on the unspecified address and scout by multicast.
//   multicast     multicast on: scouting/multicast/enabled set true, a 224.x group address,
//                 join_multicast_v4/v6, dgram addMembership.
//   js-listen     a JS .listen( call with no literal loopback host ('127.0.0.1' or '::1')
//                 on the line: Node's default host is the unspecified address.
//   js-dgram      a JS dgram createSocket: UDP sockets are always checked by hand.
//
// Allowed, each entry stale-checked (an entry that no longer matches a hit fails):
//   OPT_IN   a test function that listens beyond loopback on purpose. Every hit must lie
//            inside the function, after its gate: a line `if !<gate>(...)` followed by
//            `return;`. The file must define the switch the gate reads. The local-ci tier
//            named in the entry sets the switch for its own child only.
//   REVIEWED a line that matches a rule but binds nothing beyond loopback (a host already
//            validated as loopback, a Unix socket path, a test that asserts the address is
//            refused), with the reason.
//
// Not proven (a text lint cannot see these): an address built at run time or read from
// configuration, a bind through a wrapper defined elsewhere, a listener in a dependency.
// The runtime evidence is a netstat sample during a run (PR body for this check).
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

const SCOPE = ['core/', 'cli/', 'adapters/', 'transports/', 'tests/', 'tools/', 'scripts/'];
const EXTENSIONS = /\.(?:rs|mjs|js|cjs|ts|py|sh)$/;

// Built from pieces so that this file's own text matches none of its rules.
const P = (...parts) => parts.join('');

const RULES = [
  { id: 'unspecified', langs: 'all', re: new RegExp(P('\\b0\\.0\\.0', '\\.0\\b|\\[::\\]|\\bUNSPECIFIE', 'D\\b|\\bINADDR_AN', 'Y\\b|\\bin6addr_an', 'y\\b')) },
  { id: 'rust-bind', langs: 'rs', re: new RegExp(P('\\b(?:TcpListener|UdpSocket|TcpSocket)::bin', 'd\\s*\\(\\s*(?!"127\\.0\\.0\\.1:|"\\[::1\\]:|\\(\\s*(?:std::net::)?Ipv[46]Addr::LOCALHOST\\b)')) },
  { id: 'endpoint', langs: 'all', re: new RegExp(P('["\'\\[](?:tcp|udp|quic|tls)', '/(?!127\\.0\\.0\\.1:|\\[::1\\]:)')) },
  { id: 'zenoh-default', langs: 'rs', re: new RegExp(P('\\bzenoh::(?:config::)?Config::defaul', 't\\s*\\(')) },
  { id: 'multicast', langs: 'all', re: new RegExp(P('scouting/multicast/enabled[^\\n]*\\btru', 'e\\b|\\b22[4-9]\\.\\d+\\.\\d+\\.\\d+\\b|\\bjoin_multicast_v[46]\\b|\\baddMember', 'ship\\s*\\(')) },
  { id: 'js-listen', langs: 'js', re: new RegExp(P('\\.liste', 'n\\s*\\((?![^\\n]*([\'"])(?:127\\.0\\.0\\.1|::1)\\1)')) },
  { id: 'js-dgram', langs: 'js', re: new RegExp(P('\\bcreate', 'Socket\\s*\\(')) },
];

const langOf = (path) => (path.endsWith('.rs') ? 'rs' : /\.(?:mjs|js|cjs|ts)$/.test(path) ? 'js' : /\.py$/.test(path) ? 'py' : 'sh');
const commentLine = (lang) => (lang === 'rs' || lang === 'js' ? /^\s*\/\// : /^\s*#/);

// A test that listens beyond loopback on purpose, run only by its opt-in tier.
export const OPT_IN = [
  {
    path: 'transports/zenoh/tests/peer_transport.rs',
    fn: 'local_mode_reaches_nothing_beyond_loopback',
    gate: 'lan_opt_in',
    defines: P('const LAN_OPT_IN: &str = "OAC_TEST', '_LAN";'),
    tier: 'lan',
    why: 'PR #364 finding 1: a LAN probe (LAN listener, multicast scouting) must hear nothing from local mode',
  },
];

// Lines that match a rule but listen on nothing beyond loopback. `line` is a substring of
// the trimmed line.
export const REVIEWED = [
  { path: 'transports/zenoh/src/config.rs', rule: 'zenoh-default', line: P('let mut c = zenoh::Config::defaul', 't();'), why: 'native(): then sets listen/connect to tcp/127.0.0.1 and multicast scouting and gossip off; its unit tests assert all three' },
  { path: 'transports/zenoh/tests/contract.rs', rule: 'multicast', line: P('v.push("224.0.0', '.224".into());'), why: 'health_must_not_contain: a string a health detail must not hold; nothing joins the group' },
  { path: 'tests/fakes/codex-app-server/lib/ws.mjs', rule: 'js-listen', line: P('server.liste', 'n(port, host, () => {'), why: 'host comes from parseLoopbackUrl, which refuses anything but 127.0.0.1 and ::1' },
  { path: 'tools/herdr/test/fake-codex.mjs', rule: 'js-listen', line: P('}).liste', 'n(SOCK);'), why: 'SOCK is a Unix socket path (the POSIX lifecycle half), not a network address' },
  { path: 'tests/fakes/codex-app-server/self-test.mjs', rule: 'unspecified', line: P("await test('transports: a non-loopback or name-resolved --listen address is refused (0.0.0", ".0, localhost)'"), why: 'the test name of the refusal test below' },
  { path: 'tests/fakes/codex-app-server/self-test.mjs', rule: 'unspecified', line: P("for (const url of ['ws://0.0.0", ".0:0', 'ws://localhost:0']) {"), why: 'asserts the fake refuses these addresses; it exits before listening' },
];

// ---- git index ------------------------------------------------------------------------

function git(root, args, input) {
  return execFileSync('git', args, { cwd: root, input, maxBuffer: 1024 * 1024 * 1024, stdio: ['pipe', 'pipe', 'pipe'] });
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

// ---- the check ------------------------------------------------------------------------

// The line range [start, end] of Rust fn `name`: from its `fn` line to the first closing
// brace at the same indent.
function fnRange(lines, name) {
  const head = new RegExp(`^(\\s*)(?:pub(?:\\([^)]*\\))?\\s+)?(?:async\\s+)?fn\\s+${name}\\b`);
  for (let i = 0; i < lines.length; i++) {
    const m = head.exec(lines[i]);
    if (!m) continue;
    const close = new RegExp(`^${m[1]}\\}\\s*$`);
    for (let j = i + 1; j < lines.length; j++) if (close.test(lines[j])) return [i, j];
    return null;
  }
  return null;
}

export function check(root, { optIn = OPT_IN, reviewed = REVIEWED } = {}) {
  const entries = indexEntries(root).filter((e) => SCOPE.some((s) => e.path.startsWith(s)) && EXTENSIONS.test(e.path));
  if (entries.length === 0) return { error: 'empty scope: no tracked source file under core/, cli/, adapters/, transports/, tests/, tools/ or scripts/' };
  const blobs = readBlobs(root, entries.filter((e) => e.mode.startsWith('100')).map((e) => e.sha));
  const hits = [];
  const allowed = [];
  const used = new Set();
  const files = new Map();
  for (const e of entries) {
    if (!e.mode.startsWith('100')) {
      hits.push({ path: e.path, line: 0, rule: 'symlink or submodule in scope (fails closed)', text: '' });
      continue;
    }
    const lang = langOf(e.path);
    const lines = (blobs.get(e.sha) ?? Buffer.alloc(0)).toString('utf8').split(/\r?\n/);
    files.set(e.path, lines);
    const comment = commentLine(lang);
    const opt = optIn.filter((o) => o.path === e.path).map((o) => ({ o, range: lang === 'rs' ? fnRange(lines, o.fn) : null }));
    lines.forEach((l, i) => {
      if (comment.test(l)) return;
      for (const r of RULES) {
        if (r.langs !== 'all' && r.langs !== lang) continue;
        if (!r.re.test(l)) continue;
        const rev = reviewed.findIndex((x) => x.path === e.path && x.rule === r.id && l.trim().includes(x.line));
        if (rev !== -1) {
          used.add(`r${rev}`);
          allowed.push({ path: e.path, line: i + 1, rule: r.id, why: reviewed[rev].why });
          continue;
        }
        const inFn = opt.find(({ range }) => range && i > range[0] && i < range[1]);
        if (inFn) {
          used.add(`o${optIn.indexOf(inFn.o)}`);
          inFn.hits = [...(inFn.hits ?? []), { line: i, rule: r.id, text: l }];
          continue;
        }
        hits.push({ path: e.path, line: i + 1, rule: r.id, text: l });
      }
    });
    // An opt-in test: its gate comes first, and returns; the switch is defined in the file.
    for (const x of opt) {
      if (!x.range || !x.hits) continue;
      const [s, end] = x.range;
      const gate = new RegExp(`^\\s*if\\s+!\\s*${x.o.gate}\\s*\\(`);
      let g = -1;
      for (let i = s + 1; i < end; i++) {
        if (commentLine('rs').test(lines[i]) || !lines[i].trim()) continue;
        if (gate.test(lines[i]) && /^\s*return\s*;/.test(lines[i + 1] ?? '')) g = i;
        break; // the gate must be the first statement
      }
      const why = g === -1 ? `opt-in test ${x.o.fn} does not open with \`if !${x.o.gate}(..) { return; }\`` : !lines.some((l) => l.includes(x.o.defines)) ? `opt-in test ${x.o.fn}: the file does not define its switch (${x.o.defines})` : null;
      for (const h of x.hits) {
        if (why) hits.push({ path: x.o.path, line: h.line + 1, rule: `${h.rule}; ${why}`, text: h.text });
        else allowed.push({ path: x.o.path, line: h.line + 1, rule: h.rule, why: `opt-in (${x.o.fn}, tier ${x.o.tier}): ${x.o.why}` });
      }
    }
  }
  // Stale allowances fail: each must still match something.
  optIn.forEach((o, n) => {
    if (used.has(`o${n}`)) return;
    const lines = files.get(o.path);
    const what = !lines ? 'file not found in scope' : !fnRange(lines, o.fn) ? `fn ${o.fn} not found` : 'no listener inside it';
    hits.push({ path: o.path, line: 0, rule: `stale OPT_IN entry (${what})`, text: o.fn });
  });
  reviewed.forEach((x, n) => {
    if (!used.has(`r${n}`)) hits.push({ path: x.path, line: 0, rule: `stale REVIEWED entry (${x.rule}: line not found)`, text: x.line });
  });
  return { hits, allowed, files: entries.length };
}

function excerpt(line) {
  const t = line.trim();
  return t.length > 120 ? `${t.slice(0, 117)}...` : t;
}

function report(result) {
  if (result.error) {
    console.error(`check-test-listeners: ${result.error}`);
    return 2;
  }
  for (const a of result.allowed) console.log(`allowed ${a.path}:${a.line} [${a.rule}] ${a.why}`);
  for (const h of result.hits) console.log(`${h.path}:${h.line} [${h.rule}]${h.text ? ` ${excerpt(h.text)}` : ''}`);
  console.log(`loopback-only listeners: ${result.hits.length === 0 ? 'clean' : `${result.hits.length} hit(s)`} over ${result.files} tracked source files; ${result.allowed.length} allowed`);
  if (result.hits.length === 0) {
    console.log('Result: CLEAN');
    return 0;
  }
  console.log('Result: FAIL -- bind 127.0.0.1 or ::1, or make the test opt-in (OPT_IN in this script, a local-ci tier that sets its switch)');
  return 1;
}

// ---- self-test ------------------------------------------------------------------------

const GATED = (body, extra = '') => P(
  'const LAN_OPT_IN: &str = "OAC_TEST', '_LAN";\n',
  'fn lan_opt_in(t: &str) -> bool { std::env::var(LAN_OPT_IN).is_ok() }\n',
  '#[test]\nfn lan_probe() {\n', body, '}\n', extra,
);
const UNSPEC = P('    let a = (Ipv4Addr::UNSPECIFIE', 'D, 0);\n');
const GATE = '    if !lan_opt_in("lan_probe") {\n        return;\n    }\n';
const TEST_OPT_IN = [{ path: 'transports/x/tests/lan.rs', fn: 'lan_probe', gate: 'lan_opt_in', defines: P('const LAN_OPT_IN: &str = "OAC_TEST', '_LAN";'), tier: 'lan', why: 'planted' }];
const BASE = {
  'core/src/lib.rs': 'pub fn f() {}\n',
  'transports/x/tests/lan.rs': GATED(GATE + UNSPEC),
  'transports/x/tests/ok.rs': P(
    'let a = TcpListener::bind("127.0.0.1:0").unwrap();\n',
    'let b = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();\n',
    'let c = TcpListener::bind("[::1]:0").unwrap();\n',
    'set("listen/endpoints", format!(r#"["tcp/127.0.0.1:{p}"]"#));\n',
    'set("scouting/multicast/enabled", "false".into());\n',
    '// a comment naming 0.0.0', '.0 and tcp/0.0.0', '.0:7447 is not code\n',
    'let p = tmp.join("ws/vend/x");\n',
  ),
  'tests/fakes/x/server.mjs': P("server.liste", "n(0, '127.0.0.1', () => {});\nsrv.liste", 'n(port, "::1");\nconst f = g.bind(this);\n'),
  'tools/herdr/x.mjs': P('// http.liste', 'n(8080) in a comment\n'),
  'docs/notes.md': P('listen on 0.0.0', '.0\n'),
  'docs/spike/peer.py': P("s.bind(('0.0.0", ".0', 7447))\n"),
};
const CASES = [
  ['unspecified: a Rust bind on the unspecified IPv4 address', { 'tests/a/tests/x.rs': P('let l = TcpListener::bind("0.0.0', '.0:0").unwrap();\n') }, ['unspecified', 'rust-bind']],
  ['unspecified: the Ipv6Addr constant outside an opt-in test', { 'core/src/net.rs': P('let a = (Ipv6Addr::UNSPECIFIE', 'D, 0);\n') }, ['unspecified']],
  ['unspecified: the IPv6 unspecified address in a JS URL', { 'tests/fakes/y.mjs': P("const u = 'ws://[:", ":]:0';\n") }, ['unspecified']],
  ['unspecified: a Python bind on the unspecified address', { 'scripts/p.py': P("s.bind(('0.0.0", ".0', 0))\n") }, ['unspecified']],
  ['rust-bind: localhost depends on name resolution', { 'cli/tests/x.rs': P('let l = TcpListener::bin', 'd("localhost:0").unwrap();\n') }, ['rust-bind']],
  ['rust-bind: an address held in a variable', { 'adapters/a/src/x.rs': P('let l = tokio::net::TcpListener::bin', 'd(addr).await?;\n') }, ['rust-bind']],
  ['rust-bind: a LAN address', { 'transports/x/tests/y.rs': P('let s = UdpSocket::bin', 'd("192.168.1.5:0").unwrap();\n') }, ['rust-bind']],
  ['endpoint: a Zenoh locator on a LAN address', { 'transports/x/src/c.rs': P('set("listen/endpoints", format!(r#"["tc', 'p/{lan}:0"]"#));\n') }, ['endpoint']],
  ['endpoint: a QUIC locator on localhost', { 'transports/x/src/c.rs': P('let e = "qui', 'c/localhost:7447";\n') }, ['endpoint']],
  ['zenoh-default: a raw default configuration', { 'transports/x/tests/r.rs': P('let s = zenoh::open(zenoh::Config::defaul', 't()).wait();\n') }, ['zenoh-default']],
  ['multicast: scouting on', { 'transports/x/tests/m.rs': P('("scouting/multicast/enabled", "tru', 'e".to_owned()),\n') }, ['multicast']],
  ['multicast: a group address', { 'tests/fakes/m.mjs': P("sock.send(b, 7446, '224.0.0", ".224');\n") }, ['multicast']],
  ['multicast: a Rust socket joins a group', { 'tests/security/src/m.rs': P('s.join_multicast_v', '4(&g, &i).unwrap();\n') }, ['multicast']],
  ['js-listen: no host (Node listens on the unspecified address)', { 'tests/fakes/z.mjs': P('server.liste', 'n(0, () => {});\n') }, ['js-listen']],
  ['js-listen: host in a variable, not reviewed', { 'tools/herdr/s.mjs': P('http.liste', 'n(PORT, HOST);\n') }, ['js-listen']],
  ['js-listen: a LAN host', { 'tools/herdr/s.mjs': P("http.liste", "n(PORT, '10.0.0.2');\n") }, ['js-listen']],
  ['js-dgram: a UDP socket', { 'scripts/u.mjs': P("const s = dgram.create", "Socket('udp4');\n") }, ['js-dgram']],
  ['opt-in: a listener outside the opt-in fn in the same file', { 'transports/x/tests/lan.rs': GATED(GATE + UNSPEC, P('fn helper() {\n', UNSPEC, '}\n')) }, ['unspecified']],
  ['opt-in: the gate comes after the listener', { 'transports/x/tests/lan.rs': GATED(UNSPEC + GATE) }, ['unspecified; opt-in test lan_probe does not open with `if !lan_opt_in(..) { return; }`']],
  ['opt-in: no gate', { 'transports/x/tests/lan.rs': GATED(UNSPEC) }, ['unspecified; opt-in test lan_probe does not open with `if !lan_opt_in(..) { return; }`']],
  ['opt-in: the gate does not return', { 'transports/x/tests/lan.rs': GATED('    if !lan_opt_in("lan_probe") {\n        eprintln!("x");\n    }\n' + UNSPEC) }, ['unspecified; opt-in test lan_probe does not open with `if !lan_opt_in(..) { return; }`']],
  ['opt-in: the switch is not defined', { 'transports/x/tests/lan.rs': GATED(GATE + UNSPEC).replace(P('"OAC_TEST', '_LAN"'), '"OAC_OTHER"') }, [P('unspecified; opt-in test lan_probe: the file does not define its switch (const LAN_OPT_IN: &str = "OAC_TEST', '_LAN";)')]],
  ['opt-in: stale entry (the test lost its listener)', { 'transports/x/tests/lan.rs': GATED(GATE) }, ['stale OPT_IN entry (no listener inside it)']],
  ['opt-in: stale entry (the test was renamed)', { 'transports/x/tests/lan.rs': GATED(GATE + UNSPEC).replace('fn lan_probe', 'fn lan_probe_2') }, ['unspecified', 'stale OPT_IN entry (fn lan_probe not found)']],
];

function plantTree(files) {
  const dir = mkdtempSync(join(tmpdir(), 'oac-listeners-'));
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
  let total = 0;
  const run = (name, files, expect, opts = { optIn: TEST_OPT_IN, reviewed: [] }) => {
    total++;
    const dir = plantTree(files);
    try {
      const r = check(dir, opts);
      const rules = r.error ? [`error: ${r.error}`] : r.hits.map((h) => h.rule);
      const ok = JSON.stringify(rules) === JSON.stringify(expect);
      console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ` -> ${JSON.stringify(rules)}`}`);
      if (!ok) failed++;
      return r;
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  };
  const base = run('control: loopback binds, a gated opt-in test, comments and docs/ are clean', BASE, []);
  if (base.allowed) {
    total++;
    const ok = JSON.stringify(base.allowed.map((a) => a.rule)) === '["unspecified"]' && base.allowed.every((a) => a.why.includes('tier lan'));
    console.log(`${ok ? 'ok  ' : 'FAIL'} control: the opt-in listener is reported as allowed, with its tier`);
    if (!ok) failed++;
  }
  for (const [name, plant, expect] of CASES) run(name, { ...BASE, ...plant }, expect);
  const rev = [{ path: 'tools/herdr/s.mjs', rule: 'js-listen', line: P('http.liste', 'n(PORT, HOST);'), why: 'planted' }];
  run('reviewed: a reviewed line passes', { ...BASE, 'tools/herdr/s.mjs': P('  http.liste', 'n(PORT, HOST);\n') }, [], { optIn: TEST_OPT_IN, reviewed: rev });
  run('reviewed: the same line in another file still fails', { ...BASE, 'tools/herdr/s.mjs': P('http.liste', 'n(PORT, HOST);\n'), 'tools/herdr/t.mjs': P('http.liste', 'n(PORT, HOST);\n') }, ['js-listen'], { optIn: TEST_OPT_IN, reviewed: rev });
  run('reviewed: stale entry', BASE, ['stale REVIEWED entry (js-listen: line not found)'], { optIn: TEST_OPT_IN, reviewed: rev });
  run('empty scope is an error', { 'docs/x.md': 'x\n' }, ['error: empty scope: no tracked source file under core/, cli/, adapters/, transports/, tests/, tools/ or scripts/']);
  console.log(`self-test: ${total - failed}/${total} passed`);
  return failed === 0 ? 0 : 1;
}

// ---- main -----------------------------------------------------------------------------

if (process.argv[1] && resolve(process.argv[1]) === scriptPath) {
  const args = process.argv.slice(2);
  if (args[0] === '--self-test' && args.length === 1) process.exit(runSelfTest());
  let root = repoRoot;
  if (args[0] === '--root' && args.length === 2) root = resolve(args[1]);
  else if (args.length !== 0) {
    console.error('usage: node scripts/check-test-listeners.mjs [--root <dir> | --self-test]');
    process.exit(2);
  }
  try {
    process.exit(report(check(root)));
  } catch (err) {
    console.error(`check-test-listeners: ${err.message}`);
    process.exit(2);
  }
}
