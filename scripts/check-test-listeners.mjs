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
// (its g3-macos section is the opt-in macOS tier, multicast by design). Comments are masked,
// preserving source offsets. The scan is the git index, as in
// scripts/check-containment.mjs.
//
// Rules (calls and multicast settings span lines):
//   unspecified   the unspecified address in code: 0.0.0.0, [::], the UNSPECIFIED constant
//                 of Ipv4Addr/Ipv6Addr, INADDR_ANY, in6addr_any.
//   rust-bind     bind/bind_to/socket constructor calls by name, including bare functions,
//                 paths, methods and turbofish, whose
//                 own address argument is not a literal loopback address ("127.0.0.1:0", "[::1]:0",
//                 (Ipv4Addr::LOCALHOST, ...), (Ipv6Addr::LOCALHOST, ...)). `localhost`
//                 counts as not loopback: what it binds depends on name resolution.
//   endpoint      unresolved Zenoh listen/connect/endpoints settings, or a Zenoh-style
//                 locator ("tcp/...", "udp/...", "quic/...", "tls/...") whose
//                 host is not 127.0.0.1 or [::1].
//   zenoh-default a raw default Zenoh configuration (zenoh::Config::default): Zenoh's
//                 defaults listen on the unspecified address and scout by multicast.
//   multicast     multicast on: scouting/multicast/enabled set true, a 224.x group address,
//                 join_multicast_v4/v6, dgram addMembership.
//   js-listen     calls named listen/bind/bind_to, including optional calls and chains.
//                 listen requires a literal numeric port and a literal loopback host;
//                 unresolved ports (which may be options), options and spreads need review.
//   js-dgram      a JS dgram createSocket: UDP sockets are always checked by hand.
//
// Allowed, each entry stale-checked (an entry that no longer matches a hit fails):
//   OPT_IN   a test function that listens beyond loopback on purpose. Every hit must lie
//            inside the function, after its complete opening `if !<gate>("<test>") {
//            return; }` block. Extra condition terms/statements fail. The file must define
//            the switch the gate reads. The local-ci tier
//            named in the entry sets the switch for its own child only.
//   REVIEWED exactly one complete trimmed line that matches a rule but binds nothing beyond
//            loopback (a validated host, a Unix socket path, a test that asserts an address is
//            refused), with the reason. Repeated identical session-binding lines also pin
//            adjacent lines; computed endpoints pin the complete setting call. Any extra
//            listener on the reviewed line fails.
//
// Residual: this is a lexical lint, not a Rust/JS type/control-flow analyzer. Direct binds
// with computed/variable addresses and computed endpoint settings fail closed; exact
// reviewed session/key bind sites
// need human re-review if their receiver's type changes. A wrapper/renamed bind function,
// macro-generated call, JS computed property, template interpolation, computed setting
// key, dependency listener,
// or changed gate-helper semantics can escape it. OPT_IN proves the opening guard's syntax
// and listener placement, not the helper's behavior. Static text cannot see runtime
// addresses; that is why unresolved direct calls/settings fail closed. Those changes
// require source review.
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
  { id: 'rust-bind', langs: 'rs' },
  { id: 'endpoint', langs: 'all', re: new RegExp(P('["\'\\[](?:tcp|udp|quic|tls)', '/(?!127\\.0\\.0\\.1:|\\[::1\\]:)')) },
  { id: 'zenoh-default', langs: 'rs', re: new RegExp(P('\\bzenoh::(?:config::)?Config::defaul', 't\\s*\\(')) },
  { id: 'multicast', langs: 'all' },
  { id: 'js-listen', langs: 'js' },
  { id: 'js-dgram', langs: 'js', re: new RegExp(P('\\bcreate', 'Socket\\s*\\(')) },
];

const langOf = (path) => (path.endsWith('.rs') ? 'rs' : /\.(?:mjs|js|cjs|ts)$/.test(path) ? 'js' : /\.py$/.test(path) ? 'py' : 'sh');

// Preserve offsets while masking comments and strings. Call delimiters are read from
// code, arguments from text: neither a comment nor a callback string proves a host.
function sourceView(source, lang) {
  const text = source.split('');
  const code = source.split('');
  const errors = [];
  const regexStart = (at) => /(?:^|[=(:,\[!&|?{;]|=>|\b(?:return|throw|case|yield))\s*$/.test(source.slice(0, at));
  const regexEnd = (at) => {
    let inClass = false;
    for (let j = at + 1; j < source.length && source[j] !== '\n'; j++) {
      if (source[j] === '\\') { j++; continue; }
      if (source[j] === '[') inClass = true;
      else if (source[j] === ']') inClass = false;
      else if (source[j] === '/' && !inClass) return j + 1;
    }
    errors.push(at);
    return source.length;
  };
  const quotedEnd = (at) => {
    const quote = source[at];
    for (let j = at + 1; j < source.length;) {
      if (source[j] === '\\') { j += 2; continue; }
      if (source[j] === quote) return j + 1;
      // Templates may nest other templates inside an interpolation. Skip the entire
      // expression here; calls hidden in interpolation remain a documented residual.
      if (quote === '`' && source.startsWith('${', j)) {
        j += 2;
        let depth = 1;
        while (j < source.length && depth) {
          if (/['"`]/.test(source[j])) j = quotedEnd(j);
          else if (source.startsWith('//', j)) { const end = source.indexOf('\n', j); j = end === -1 ? source.length : end; }
          else if (source.startsWith('/*', j)) { const end = source.indexOf('*/', j + 2); j = end === -1 ? source.length : end + 2; }
          else if (source[j] === '/' && regexStart(j)) j = regexEnd(j);
          else { if (source[j] === '{') depth++; if (source[j] === '}') depth--; j++; }
        }
      } else j++;
    }
    errors.push(at);
    return source.length;
  };
  const blank = (a, b, comment = false) => {
    for (let i = a; i < b; i++) {
      if (source[i] === '\n' || source[i] === '\r') continue;
      code[i] = ' ';
      if (comment) text[i] = ' ';
    }
  };
  for (let i = 0; i < source.length;) {
    const start = i;
    if ((lang === 'rs' || lang === 'js') && source.startsWith('//', i)) {
      i = source.indexOf('\n', i);
      if (i === -1) i = source.length;
      blank(start, i, true);
    } else if ((lang === 'rs' || lang === 'js') && source.startsWith('/*', i)) {
      i += 2;
      let depth = 1;
      while (i < source.length && depth) {
        if (lang === 'rs' && source.startsWith('/*', i)) { depth++; i += 2; }
        else if (source.startsWith('*/', i)) { depth--; i += 2; }
        else i++;
      }
      blank(start, i, true);
    } else if ((lang === 'py' || lang === 'sh') && source[i] === '#') {
      i = source.indexOf('\n', i);
      if (i === -1) i = source.length;
      blank(start, i, true);
    } else {
      // A regex at the start of a JS expression may contain quote characters. Mask it
      // before the string scanner sees them; division remains code. Ambiguous regex
      // forms that look like unterminated strings fail closed below.
      if (lang === 'js' && source[i] === '/' && regexStart(i)) {
        i = regexEnd(i);
        blank(start, i);
        continue;
      }
      const raw = lang === 'rs' && /^r(#+)?"/.exec(source.slice(i));
      if (raw) {
        const end = source.indexOf(`"${raw[1] ?? ''}`, i + raw[0].length);
        if (end === -1) errors.push(start);
        i = end === -1 ? source.length : end + 1 + (raw[1]?.length ?? 0);
        blank(start, i);
      } else if (source[i] === '"' || (lang !== 'rs' && /['`]/.test(source[i])) ||
                 (lang === 'rs' && /^'(?:\\.|[^'\\\n])'/.test(source.slice(i)))) {
        i = quotedEnd(i);
        blank(start, i);
      } else i++;
    }
  }
  return { text: text.join(''), code: code.join(''), errors };
}

function callAt(view, open) {
  const stack = [')'];
  const args = [];
  let start = open + 1;
  for (let i = start; i < view.code.length; i++) {
    const c = view.code[i];
    if ('([{'.includes(c)) stack.push({ '(': ')', '[': ']', '{': '}' }[c]);
    else if (')]}'.includes(c)) {
      if (stack.pop() !== c) return null;
      if (!stack.length) {
        args.push(view.text.slice(start, i).trim());
        return { args, end: i + 1 };
      }
    } else if (c === ',' && stack.length === 1) {
      args.push(view.text.slice(start, i).trim());
      start = i + 1;
    }
  }
  return null;
}

function sourceHits(source, lang) {
  const view = sourceView(source, lang);
  const hits = [];
  const add = (rule, offset, end = offset + 1) => hits.push({ rule, offset, end, line: source.slice(0, offset).split('\n').length - 1 });
  for (const offset of view.errors) add('unresolved source quoting (fails closed)', offset);
  for (const r of RULES) {
    if (['rust-bind', 'js-listen', 'js-dgram', 'multicast'].includes(r.id)) continue;
    if (r.langs !== 'all' && r.langs !== lang) continue;
    for (const m of view.text.matchAll(new RegExp(r.re.source, 'g'))) add(r.id, m.index, m.index + m[0].length);
  }
  // Identify the name first, then read its call suffix. Receiver spelling is irrelevant.
  // Unknown overloads, spreads and generic suffixes never provide evidence of safety.
  const names = new RegExp(P('\\b(?:bin', 'd|bind_to|listen|createSocket|UdpSocket|TcpListener|join_multicast\\w*|addMembership)\\b'), 'g');
  for (const m of view.code.matchAll(names)) {
    if (/\b(?:fn|function)\s*$/.test(view.code.slice(0, m.index))) continue;
    let at = m.index + m[0].length;
    const space = () => { while (/\s/.test(view.code[at] ?? '') && at < view.code.length) at++; };
    space();
    if (view.code.slice(at, at + 2) === '::') {
      at += 2; space();
      if (view.code[at] !== '<') continue; // associated methods are checked by their own name
    }
    if (view.code[at] === '<') {
      let depth = 0;
      const groups = [];
      do {
        const c = view.code[at];
        if ('([{'.includes(c)) groups.push(c);
        else if (')]}'.includes(c)) groups.pop();
        else if (!groups.length) {
          if (c === '<') depth++;
          if (c === '>' && view.code[at - 1] !== '-') depth--;
        }
        at++;
      } while (depth && at < view.code.length);
      space();
    }
    if (view.code.slice(at, at + 2) === '?.') { at += 2; space(); }
    if (view.code[at] !== '(') continue;
    const call = callAt(view, at);
    const name = m[0];
    const rule = /^(?:join_multicast|addMembership)/.test(name) ? 'multicast'
      : name === 'createSocket' ? 'js-dgram' : lang === 'js' ? 'js-listen' : 'rust-bind';
    const loopback = call && (lang === 'js' && name === 'listen'
      // Only a numeric port establishes the positional overload. A variable may be options.
      ? /^\d+$/.test(call.args[0]) && !call.args.some((arg) => arg.startsWith('...')) &&
        /^(['"])(?:127\.0\.0\.1|::1|\[::1\])\1$/.test(call.args[1] ?? '')
      : !/^(?:createSocket|join_multicast|addMembership)/.test(name) &&
        (/^"(?:127\.0\.0\.1|\[::1\]):\d+"$/.test(call.args[0]) ||
         /^\(\s*(?:std::net::)?Ipv[46]Addr::LOCALHOST\s*,[^]+\)$/.test(call.args[0])));
    hits.push({ rule, offset: m.index, end: call?.end ?? view.code.length,
      line: source.slice(0, m.index).split('\n').length - 1, safe: !!loopback, listener: true, callText: view.text.slice(m.index, call?.end ?? view.code.length).trim() });
  }
  const multicast = new RegExp(P('\\b22[4-9]\\.\\d+\\.\\d+\\.\\d+\\b'), 'g');
  for (const m of view.text.matchAll(multicast)) add('multicast', m.index, m.index + m[0].length);
  // A setting key must be the first argument of its containing call/tuple, not a read
  // inside an assertion. Only literal endpoint values or disabled scouting are safe.
  const setting = /["'](?:(?:listen|connect)\/endpoints|listen|connect|endpoints|scouting(?:\/[^"']+)?)["']/g;
  for (const m of view.text.matchAll(setting)) {
    const open = view.code.lastIndexOf('(', m.index);
    const call = open === -1 ? null : callAt(view, open);
    if (!call || call.args[0] !== m[0]) continue;
    const prefix = view.code.slice(0, open);
    if (!m[0].includes('/') && !/\b(?:set|insert_json5|insert|configure)\s*$/.test(prefix)) continue;
    if (/\b(?:json|get_json|assert_eq|assert_ne)\s*!?\s*$/.test(prefix)) continue;
    const endpoints = !m[0].includes('scouting');
    const value = call.args[1] ?? '';
    const literal = /^(?:r(#+)?"([\s\S]*)"\1|"([^"\\]*)")(?:(?:\.into|\.to_owned|\.to_string)\(\))?$/.exec(value);
    const content = literal?.[2] ?? literal?.[3];
    const safe = endpoints ? content !== undefined && (() => {
      try {
        const values = JSON.parse(content);
        return Array.isArray(values) && values.every((v) => typeof v === 'string' && /^(?:tcp|udp)\/(?:127\.0\.0\.1|\[::1\]):\d+$/.test(v));
      } catch { return false; }
    })() : /^(?:"false"|false)(?:\.(?:into|to_owned|to_string)\(\))?$/.test(value);
    if (!safe) hits.push({ rule: endpoints ? 'endpoint' : 'multicast', offset: m.index,
      end: call.end, line: source.slice(0, m.index).split('\n').length - 1, listener: true,
      callText: view.text.slice(open, call.end).trim() });
  }
  return { view, hits: hits.filter((h) => h.listener || !hits.some((c) => c.listener && c.rule === h.rule && c.offset <= h.offset && h.offset < c.end)) };
}

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

// Exact trimmed lines that match a rule but listen on nothing beyond loopback.
export const REVIEWED = [
  { path: "core/src/session_binding.rs", rule: "rust-bind", line: P("Some(StartKind::Transition", ") => bind(if any_differs {"), why: "Local BindingDecision closure; only constructs a session-binding decision, no socket" },
  { path: "core/src/session_binding.rs", rule: "rust-bind", line: P("bind(Vec", "::new())"), why: "Local BindingDecision closure; no socket" },
  { path: "core/src/session_binding.rs", rule: "rust-bind", line: P("bind(vec![diagnos", "tic(\"SC-ID-141\")])"), why: "Local BindingDecision closure; no socket" },
  { path: "core/tests/sealing.rs", rule: "rust-bind", line: P("n.pipes.bind(&a, &re", "cord, None).unwrap();"), why: "Pipelines attachment binding; no socket" },
  { path: "core/tests/sealing.rs", rule: "rust-bind", line: P("t.z.pipes.with_engine(|", "e| e.bind(&t.sy, &yk));"), why: "AuthorizationEngine session/key binding; no socket" },
  { path: "tests/security/src/sealing.rs", rule: "rust-bind", line: P("self.pipes.bind(&a, &reco", "rd, None).expect(\"bound\");"), why: "Pipelines attachment binding; no socket" },
  { path: "tests/security/tests/sealing.rs", rule: "rust-bind", line: P("t.z.pipes.with_engine(|", "e| e.bind(&t.sy, &yk));"), why: "AuthorizationEngine session/key binding; no socket" },
  { path: "tests/fakes/codex-app-server/server.mjs", rule: "js-listen", line: P("ws = await listen(l", "istenUrl, (ch) => {"), why: "The ws listen wrapper validates the URL through parseLoopbackUrl before opening the listener" },
  { path: "tools/herdr/gate-servers/g4-server.mjs", rule: "js-listen", line: P("http.listen(PORT, '127.0.0.1', () => log('http', null, 'spike', { note: ", "`listening http://127.0.0.1:${PORT}/mcp`, stdio_modern: STDIO_MODERN }));"), why: "PORT is Number(G4_HTTP_PORT || 17448); positional numeric port and literal loopback host" },
  { path: "tools/herdr/lib/gate-common.mjs", rule: "js-listen", line: P("s.listen(port, '1", "27.0.0.1', () => {"), why: "loopbackPortFree receives a numeric port from its callers; literal loopback host" },
  { path: "transports/memory/tests/pipelines.rs", rule: "rust-bind", line: P("let claude_sid = bind(&pipes, ", "&claude_att, 1, \"claude-code\");"), why: "Local helper calls Pipelines attachment binding; no socket" },
  { path: "transports/memory/tests/pipelines.rs", rule: "rust-bind", line: P("let codex_sid = bind(&pipe", "s, &codex_att, 2, \"codex\");"), why: "Local helper calls Pipelines attachment binding; no socket" },
  { path: "transports/memory/tests/pipelines.rs", rule: "rust-bind", line: P("assert_eq!(bind(&x, &claude_att,", " 1, \"claude-code\"), claude_sid);"), why: "Local helper calls Pipelines attachment binding; no socket" },
  { path: "transports/memory/tests/pipelines.rs", rule: "rust-bind", line: P("assert_eq!(bind(&y, &codex_a", "tt, 2, \"codex\"), codex_sid);"), why: "Local helper calls Pipelines attachment binding; no socket" },
  { path: "tests/protocol/runner/stages-sc.mjs", rule: "js-listen", line: P("bin", "d();"), next: P("} el", "se {"), prev: "result = 'bound'; // case 3(a), [SC-ID-136]", why: "Conformance runner local closure updates attachment binding; no socket" },
  { path: "tests/protocol/runner/stages-sc.mjs", rule: "js-listen", line: P("bin", "d();"), next: P("", "}"), prev: "records.push('diagnostic');", why: "Conformance runner local closure updates attachment binding; no socket" },
  { path: "tests/protocol/runner/stages-sc.mjs", rule: "js-listen", line: P("bin", "d();"), next: P("", "}"), prev: "if (differs) records.push('diagnostic'); // [SC-ID-142]", why: "Conformance runner local closure updates attachment binding; no socket" },
  { path: "transports/zenoh/src/config.rs", rule: "endpoint", line: P("\"listen/e", "ndpoints\","), call: P("(\n            \"listen/endpoints\",\n            forma", "t!(r#\"[\"tcp/127.0.0.1:{listen_port}\"]\"#),\n        )"), why: "native() fixes tcp/127.0.0.1; only the u16 port is formatted, asserted by its configuration tests" },
  { path: "transports/zenoh/src/config.rs", rule: "endpoint", line: P("\"connect/e", "ndpoints\","), call: P("(\n                \"connect/endpoints\",\n                fo", "rmat!(r#\"[\"tcp/127.0.0.1:{}\"]\"#, self.port),\n            )"), why: "native() fixes tcp/127.0.0.1; only the u16 port is formatted, asserted by its configuration tests" },
  { path: 'transports/zenoh/src/config.rs', rule: 'zenoh-default', line: P('let mut c = zenoh::Config::defaul', 't();'), why: 'native(): then sets listen/connect to tcp/127.0.0.1 and multicast scouting and gossip off; its unit tests assert all three' },
  { path: 'transports/zenoh/tests/contract.rs', rule: 'multicast', line: P('v.push("224.0.0', '.224".into());'), why: 'health_must_not_contain: a string a health detail must not hold; nothing joins the group' },
  { path: 'tests/fakes/codex-app-server/lib/ws.mjs', rule: 'js-listen', line: P('server.liste', 'n(port, host, () => {'), why: 'host comes from parseLoopbackUrl, which refuses anything but 127.0.0.1 and ::1' },
  { path: 'tools/herdr/test/fake-codex.mjs', rule: 'js-listen', line: P('}).liste', 'n(SOCK);'), why: 'SOCK is a Unix socket path (the POSIX lifecycle half), not a network address' },
  { path: 'tests/fakes/codex-app-server/self-test.mjs', rule: 'unspecified', line: P("await test('transports: a non-loopback or name-resolved --listen address is refused (0.0.0", ".0, localhost)', async () => {"), why: 'the test name of the refusal test below' },
  { path: 'tests/fakes/codex-app-server/self-test.mjs', rule: 'unspecified', line: P("for (const url of ['ws://0.0.0", ".0:0', 'ws://localhost:0']) {"), why: 'asserts the fake refuses these addresses; it exits before listening' },
  { path: "core/src/authorization.rs", rule: 'rust-bind', line: "p.bob_engine.bind(&sid(A1), p.alice.key_id()),", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/src/authorization.rs", rule: 'rust-bind', line: "assert_eq!(p.bob_engine.bind(&sid(A1), &ak), BindOutcome::Bound);", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/src/authorization.rs", rule: 'rust-bind', line: "assert_eq!(p.bob_engine.bind(&sid(A1), &ak), BindOutcome::AlreadyBound);", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/src/authorization.rs", rule: 'rust-bind', line: "e.bind(&sid(A1), &kid_n(7));", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/src/authorization.rs", rule: 'rust-bind', line: "e.bind(&sid(A1), &good);", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/src/authorization.rs", rule: 'rust-bind', line: "e.bind(&sid(B2), &flood);", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/src/authorization.rs", rule: 'rust-bind', line: "e.bind(&sid(A1), &peer);", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/src/authorization.rs", rule: 'rust-bind', line: "assert_eq!(p.bob_engine.bind(&s, &ak), BindOutcome::Bound);", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/src/authorization.rs", rule: 'rust-bind', line: "assert_eq!(p.bob_engine.bind(&s2, &ak), BindOutcome::Bound);", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/src/authorization.rs", rule: 'rust-bind', line: "assert_eq!(p.bob_engine.bind(&sid_n(1), &ak), BindOutcome::AlreadyBound);", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/src/presence_auth.rs", rule: 'rust-bind', line: "engine.bind(&sid, &key);", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/src/receipt_auth.rs", rule: 'rust-bind', line: "e.bind(env.to(), bob.key_id());", next: "assert_eq!(accept_receipt(&ar, &e), Err(ReceiptDiscard::NotSent));", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/src/receipt_auth.rs", rule: 'rust-bind', line: "e.bind(env.to(), bob.key_id());", next: "let got = accept_receipt(&ar, &e).unwrap();", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/tests/conformance/authorization.rs", rule: 'rust-bind', line: "Json::String(k) => assert_eq!(e.bind(&s, &kid(k)), BindOutcome::Bound),", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/tests/conformance/authorization.rs", rule: 'rust-bind', line: "assert_eq!(e.bind(&s, &keys.next().unwrap()), BindOutcome::Bound);", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/tests/pipeline.rs", rule: 'rust-bind', line: "n.pipes.bind(&a, &record, None).unwrap();", next: "(a, sid)", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/tests/pipeline.rs", rule: 'rust-bind', line: "n.pipes.bind(&forged, &record(5), None),", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/tests/pipeline.rs", rule: 'rust-bind', line: "n.pipes.bind(&theirs, &record(6), None).unwrap();", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/tests/pipeline.rs", rule: 'rust-bind', line: "n.pipes.bind(&a, &record, None).unwrap();", next: "assert_eq!(n.pipes.binding(&a), Some(sa.clone()));", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "core/tests/pipeline.rs", rule: 'rust-bind', line: "n.pipes.bind(&a, &record, None).unwrap();", next: "let (c, sc) = session(&n, 41);", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "tests/security/src/pipeline.rs", rule: 'rust-bind', line: "self.pipes.bind(&a, &record, None).expect(\"bound\");", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "tests/security/tests/receipts.rs", rule: 'rust-bind', line: "alice.engine.bind(&sid(2), &bob.key_id());", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
  { path: "transports/memory/tests/pipelines.rs", rule: 'rust-bind', line: "pipes.bind(attachment, &record, None).ok()", why: 'Session/key or attachment binding in the authorization engine or Pipelines; no socket is opened' },
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

// Rust fn range, counting braces outside comments/strings (including nested helpers).
function fnRange(lines, name) {
  const head = new RegExp(`^(\\s*)(?:pub(?:\\([^)]*\\))?\\s+)?(?:async\\s+)?fn\\s+${name}\\b`);
  for (let i = 0; i < lines.length; i++) {
    const m = head.exec(lines[i]);
    if (!m) continue;
    const code = sourceView(lines.join('\n'), 'rs').code.split('\n');
    let depth = 0;
    let opened = false;
    for (let j = i; j < lines.length; j++) for (const c of code[j]) {
      if (c === '{') { depth++; opened = true; }
      if (c === '}' && --depth === 0 && opened) return [i, j];
    }
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
  const reviewedUses = new Map();
  const files = new Map();
  for (const e of entries) {
    if (!e.mode.startsWith('100')) {
      hits.push({ path: e.path, line: 0, rule: 'symlink or submodule in scope (fails closed)', text: '' });
      continue;
    }
    const lang = langOf(e.path);
    const lines = (blobs.get(e.sha) ?? Buffer.alloc(0)).toString('utf8').split(/\r?\n/);
    files.set(e.path, lines);
    const { view, hits: found } = sourceHits(lines.join('\n'), lang);
    const opt = optIn.filter((o) => o.path === e.path).map((o) => ({ o, range: lang === 'rs' ? fnRange(lines, o.fn) : null }));
    found.sort((a, b) => a.line - b.line || RULES.findIndex((r) => r.id === a.rule) - RULES.findIndex((r) => r.id === b.rule) || a.offset - b.offset);
    const seen = new Set();
    for (const h of found) {
      const i = h.line;
      const l = lines[i];
      // Non-call rules report once per line; every listener call is checked separately.
      const key = `${i}:${h.rule}`;
      if (!h.listener && seen.has(key)) continue;
      seen.add(key);
      const rev = reviewed.findIndex((x) => x.path === e.path && x.rule === h.rule && l.trim() === x.line &&
        (x.next === undefined || lines[i + 1]?.trim() === x.next) &&
        (x.prev === undefined || lines[i - 1]?.trim() === x.prev) &&
        (x.call === undefined || h.callText === x.call) &&
        (!h.listener || found.filter((other) => other.listener && other.line === i).length === 1));
      if (rev !== -1) {
        used.add(`r${rev}`);
        reviewedUses.set(rev, [...(reviewedUses.get(rev) ?? []), i + 1]);
        allowed.push({ path: e.path, line: i + 1, rule: h.rule, why: reviewed[rev].why });
        continue;
      }
      if (h.safe) continue;
      const inFn = opt.find(({ range }) => range && i > range[0] && i < range[1]);
      if (inFn) {
        used.add(`o${optIn.indexOf(inFn.o)}`);
        inFn.hits = [...(inFn.hits ?? []), { line: i, rule: h.rule, text: l, offset: h.offset }];
        continue;
      }
      hits.push({ path: e.path, line: i + 1, rule: h.rule, text: l });
    }
    // An opt-in test: its gate comes first, and returns; the switch is defined in the file.
    for (const x of opt) {
      if (!x.range || !x.hits) continue;
      const [s] = x.range;
      const start = lines.slice(0, s).join('\n').length + (s ? 1 : 0);
      const body = view.code.indexOf('{', start) + 1;
      const gate = new RegExp(`^\\s*if\\s+!\\s*${x.o.gate}\\s*\\(\\s*"${x.o.fn}"\\s*\\)\\s*\\{\\s*return\\s*;\\s*\\}`);
      const opening = gate.exec(view.text.slice(body));
      const gateEnd = opening ? body + opening[0].length : -1;
      const why = !opening || x.hits.some((h) => h.offset < gateEnd) ? `opt-in test ${x.o.fn} does not open with \`if !${x.o.gate}(..) { return; }\`` : !view.text.split('\n').some((l) => l.trim() === x.o.defines) ? `opt-in test ${x.o.fn}: the file does not define its switch (${x.o.defines})` : null;
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
    else if (reviewedUses.get(n).length !== 1) hits.push({ path: x.path, line: 0, rule: `REVIEWED entry must match exactly one line (${x.rule})`, text: x.line });
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
    'set("listen/endpoints", r#"["tcp/127.0.0.1:0"]"#.into());\n',
    'set("scouting/multicast/enable', 'd", "false".into());\n',
    '// a comment naming 0.0.0', '.0 and tcp/0.0.0', '.0:7447 is not code\n',
    'let p = tmp.join("ws/vend/x");\n',
  ),
  'tests/fakes/x/server.mjs': P("server.liste", "n(0, '127.0.0.1', () => {});\nsrv.liste", 'n(0, "::1");\nconst f = g.call(this);\n'),
  'tools/herdr/x.mjs': P('// http.liste', 'n(8080) in a comment\n'),
  'docs/notes.md': P('listen on 0.0.0', '.0\n'),
  'docs/spike/peer.py': P("s.bind(('0.0.0", ".0', 7447))\n"),
};
const CASES = [
  ['unspecified: a Rust bind on the unspecified IPv4 address', { 'tests/a/tests/x.rs': P('let l = TcpListener::bind("0.0.0', '.0:0").unwrap();\n') }, ['unspecified', 'rust-bind']],
  ['unspecified: the Ipv6Addr constant outside an opt-in test', { 'core/src/net.rs': P('let a = (Ipv6Addr::UNSPECIFIE', 'D, 0);\n') }, ['unspecified']],
  ['unspecified: the IPv6 unspecified address in a JS URL', { 'tests/fakes/y.mjs': P("const u = 'ws://[:", ":]:0';\n") }, ['unspecified']],
  ['unspecified: a Python bind on the unspecified address', { 'scripts/p.py': P("s.bind(('0.0.0", ".0', 0))\n") }, ['unspecified', 'rust-bind']],
  ['rust-bind: localhost depends on name resolution', { 'cli/tests/x.rs': P('let l = TcpListener::bin', 'd("localhost:0").unwrap();\n') }, ['rust-bind']],
  ['rust-bind: an address held in a variable', { 'adapters/a/src/x.rs': P('let l = tokio::net::TcpListener::bin', 'd(addr).await?;\n') }, ['rust-bind']],
  ['rust-bind: a LAN address', { 'transports/x/tests/y.rs': P('let s = UdpSocket::bin', 'd("192.168.1.5:0").unwrap();\n') }, ['rust-bind']],
  ['endpoint: a Zenoh locator on a LAN address', { 'transports/x/src/c.rs': P('set("listen/endpoints", format!(r#"["tc', 'p/{lan}:0"]"#));\n') }, ['endpoint']],
  ['endpoint: a QUIC locator on localhost', { 'transports/x/src/c.rs': P('let e = "qui', 'c/localhost:7447";\n') }, ['endpoint']],
  ['zenoh-default: a raw default configuration', { 'transports/x/tests/r.rs': P('let s = zenoh::open(zenoh::Config::defaul', 't()).wait();\n') }, ['zenoh-default']],
  ['multicast: scouting on', { 'transports/x/tests/m.rs': P('("scouting/multicast/enable', 'd", "tru', 'e".to_owned()),\n') }, ['multicast']],
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
  for (const body of [
    P('server.liste', "n(port); // expected host '127.0.0.1'\n"),
    P('server.liste', "n(port, host, () => console.log('127.0.0.1'));\n"),
  ]) run('review 1: unrelated loopback string', { ...BASE, 'tools/herdr/s.mjs': body }, ['js-listen']);
  for (const body of [
    P('server.liste', "n(\n port,\n host, () => console.log('127.0.0.1')\n);\n"),
    P('server.liste', "n(port, '127.0.0.1' + suffix);\n"),
    P('server.liste', "n(port, { note: '127.0.0.1' });\n"),
    P('server.liste', "n(port, host /* '127.0.0.1' */);\n"),
  ]) run('host argument: unresolved expressions fail', { ...BASE, 'tools/herdr/s.mjs': body }, ['js-listen']);
  run('control: multiline literal host with comment', { ...BASE, 'tools/herdr/s.mjs': P('server.liste', "n(\n 0, /* host */ '127.0.0.1',\n () => {}\n);\n") }, []);
  run('quoted regex cannot hide a listener', { ...BASE, 'tools/herdr/s.mjs': P('const re = /[\'"]/; server.liste', 'n(port);\n') }, ['js-listen']);
  run('division cannot hide a listener', { ...BASE, 'tools/herdr/s.mjs': P('let x = a / server.liste', 'n(port) / b;\n') }, ['js-listen']);
  for (const body of [
    P('let s = TcpSocket::new_v4()?; s.bin', 'd(SocketAddr::from(([0,0,0,0], p)))?;\n'),
    P('use std::net::TcpListener as Listener; Listener::bin', 'd(SocketAddr::from(([0,0,0,0], p)));\n'),
    P('use std::net::UdpSocket as U; U::bin', 'd(addr);\n'),
    P('let addr = SocketAddr::from(([0,0,0,0], p)); s.bin', 'd(addr);\n'),
  ]) run('review 2: unresolved Rust bind fails closed', { ...BASE, 'cli/tests/x.rs': body }, ['rust-bind']);
  run('Rust bind: multiline variable', { ...BASE, 'cli/tests/x.rs': P('s.bin', 'd(\n addr\n)?;\n') }, ['rust-bind']);
  run('control: Rust aliases and instance binds with literal loopback', { ...BASE, 'cli/tests/x.rs': P('U::bin', 'd("127.0.0.1:0");\ns.bin', 'd(\n (Ipv6Addr::LOCALHOST, p)\n);\n') }, []);
  run('review 3: multiline multicast', { ...BASE, 'transports/x/tests/m.rs': P('set("scouting/multicast/enable', 'd",\n "tru', 'e".into());\n') }, ['multicast']);
  run('review 4: disabled gate', { ...BASE, 'transports/x/tests/lan.rs': GATED(GATE.replace(') {', ') && false {') + UNSPEC) }, ['unspecified; opt-in test lan_probe does not open with `if !lan_opt_in(..) { return; }`']);
  run('control: multiline gate', { ...BASE, 'transports/x/tests/lan.rs': GATED(GATE.replace('lan_opt_in("lan_probe")', 'lan_opt_in(\n "lan_probe"\n)') + UNSPEC) }, []);
  run('multicast: multiline tuple', { ...BASE, 'transports/x/tests/m.rs': P('("scouting/multicast/enable', 'd",\n "tru', 'e".to_owned()),\n') }, ['multicast']);
  run('control: multiline multicast off with comments', { ...BASE, 'transports/x/tests/m.rs': P('set("scouting/multicast/enable', 'd",\n /* disabled */ "false".into());\n') }, []);
  for (const body of [
    P('server.liste', "n({port:0, host:'::'}, '127.0.0.1');\n"),
    P("const opts = {port:0, host:'::'}; server.liste", "n(opts, '127.0.0.1');\n"),
    P('server.liste', "n(...[0,'::'], '127.0.0.1');\n"),
    P('server.liste', 'n?.(0);\n'),
  ]) run('round 2: Node overload/spread/optional call', { ...BASE, 'tools/herdr/s.mjs': body }, ['js-listen']);
  run('round 2: Rust turbofish', { ...BASE, 'cli/tests/x.rs': P('std::net::TcpListener::bin', 'd::<_>(addr());\n') }, ['rust-bind']);
  for (const value of [
    'format!(r#"["{}/{}:{listen_port}"]"#, "tcp", "192.0.2.1")',
    'concat!("tcp", "/", "192.0.2.1:0")',
    'endpoints',
  ]) run('round 2: computed Zenoh endpoint', { ...BASE, 'transports/x/src/c.rs': P('set("listen/end', 'points", ', value, ');\n') }, ['endpoint']);
  for (const body of [
    P('std::net::TcpListener::bin', 'd::<_>("127.0.0.1:0");\n'),
    P('server?.liste', 'n?.(0, "::1");\n'),
    P('net.createServer()\n ?.liste', 'n(0, "127.0.0.1");\n'),
  ]) run('round 2 control: literal loopback call variants', { ...BASE, [body.startsWith('std') ? 'cli/tests/x.rs' : 'tools/herdr/s.mjs']: body }, []);
  for (const [body, rule] of [
    [P('bin', 'd(addr);'), 'rust-bind'],
    [P('s.bind_', 'to::<_>(addr);'), 'rust-bind'],
    [P('UdpSock', 'et(addr);'), 'rust-bind'],
    [P('TcpList', 'ener(addr);'), 'rust-bind'],
    [P('s.join_multi', 'cast_v6::<_>(&g, &i);'), 'multicast'],
  ]) run('name detection: Rust unresolved call', { ...BASE, 'cli/tests/x.rs': body }, [rule]);
  for (const [body, rule] of [
    [P('liste', 'n?.(0);'), 'js-listen'],
    [P('net.createServer()\n .liste', 'n?.(0);'), 'js-listen'],
    [P('dgram.create', 'Socket?.("udp4");'), 'js-dgram'],
    [P('server.liste', 'n({port:0, host:"127.0.0.1"});'), 'js-listen'],
    [P('server.liste', 'n(0, "127.0.0.1", ...args);'), 'js-listen'],
  ]) run('name detection: Node unresolved call', { ...BASE, 'tools/herdr/s.mjs': body }, [rule]);
  run('control: literal IPv6 endpoint setting', { ...BASE, 'transports/x/src/c.rs': P('set("connect/end', 'points", r#"["udp/[::1]:0"]"#.into());') }, []);
  const endpointRev = [{ path: 'transports/x/src/c.rs', rule: 'endpoint',
    line: P('"listen/end', 'points",'), call: P('(\n "listen/end', 'points",\n format!(r#"["tcp/127.0.0.1:{p}"]"#),\n)'), why: 'planted fixed loopback with computed port' }];
  const endpointSite = P('set(\n "listen/end', 'points",\n format!(r#"["tcp/127.0.0.1:{p}"]"#),\n);');
  run('reviewed endpoint: fixed loopback computed port', { ...BASE, 'transports/x/src/c.rs': endpointSite }, [], { optIn: TEST_OPT_IN, reviewed: endpointRev });
  run('reviewed endpoint: changed multiline value fails', { ...BASE, 'transports/x/src/c.rs': endpointSite.replace('format!(r#"["tcp/127.0.0.1:{p}"]"#)', 'format!(r#"["{}/{}:{p}"]"#, "tcp", "192.0.2.1")') }, ['endpoint', 'stale REVIEWED entry (endpoint: line not found)'], { optIn: TEST_OPT_IN, reviewed: endpointRev });
  for (const key of ['connect', 'endpoints', 'scouting']) run('setting name: unresolved value', { ...BASE, 'transports/x/src/c.rs': P('set("', key, '", value);') }, [key === 'scouting' ? 'multicast' : 'endpoint']);
  run('turbofish: const-expression delimiters', { ...BASE, 'cli/tests/x.rs': P('s.bin', 'd::<{ 1 > 0 }>(addr);') }, ['rust-bind']);
  run('setting name: an event callback is not configuration', { ...BASE, 'tools/herdr/s.mjs': "socket.once('connect', () => {});" }, []);
  run('bind name: unresolved JS function binding requires review', { ...BASE, 'tools/herdr/s.mjs': P('g.bin', 'd(this);') }, ['js-listen']);
  const rev = [{ path: 'tools/herdr/s.mjs', rule: 'js-listen', line: P('http.liste', 'n(PORT, HOST);'), why: 'planted' }];
  const wsRev = [{ path: 'tests/fakes/codex-app-server/lib/ws.mjs', rule: 'js-listen', line: P('server.liste', 'n(port, host, () => {'), why: 'planted parseLoopbackUrl host' }];
  run('review 4: exact ws extra-listener example', { ...BASE, 'tests/fakes/codex-app-server/lib/ws.mjs': P('server.liste', 'n(port, host, () => {}); other.liste', 'n(port2);\n') }, ['js-listen', 'js-listen', 'stale REVIEWED entry (js-listen: line not found)'], { optIn: TEST_OPT_IN, reviewed: wsRev });
  run('review 4: additional listener on reviewed line', { ...BASE, 'tools/herdr/s.mjs': P('http.liste', 'n(PORT, HOST); other.liste', 'n(port2);\n') }, ['js-listen', 'js-listen', 'stale REVIEWED entry (js-listen: line not found)'], { optIn: TEST_OPT_IN, reviewed: rev });
  run('reviewed: a reviewed line passes', { ...BASE, 'tools/herdr/s.mjs': P('  http.liste', 'n(PORT, HOST);\n') }, [], { optIn: TEST_OPT_IN, reviewed: rev });
  run('reviewed: duplicated exact line fails', { ...BASE, 'tools/herdr/s.mjs': P('http.liste', 'n(PORT, HOST);\nhttp.liste', 'n(PORT, HOST);\n') }, ['REVIEWED entry must match exactly one line (js-listen)'], { optIn: TEST_OPT_IN, reviewed: rev });
  run('reviewed: extra literal listener also invalidates the line', { ...BASE, 'tools/herdr/s.mjs': P('http.liste', "n(PORT, HOST); other.liste", "n(0, '127.0.0.1');\n") }, ['js-listen', 'stale REVIEWED entry (js-listen: line not found)'], { optIn: TEST_OPT_IN, reviewed: rev });
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
