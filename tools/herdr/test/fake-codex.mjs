#!/usr/bin/env node
// TEST DOUBLE of the Codex CLI, used only by the herdr driver's self-test
// (tools/herdr/test/g2-tests.mjs) to exercise tools/herdr/scenarios/g2-codex-inject.mjs end
// to end with no Codex installed. The self-test copies this file to <temp>/bin/codex (so a
// pane process's argv reads `node <temp>/bin/codex`, the shape of an npm-installed Codex),
// and test/fake-herdr.mjs starts it for `agent start --kind codex` in `fake-codex` mode.
//
// It is not Codex and proves nothing about Codex. Its screen text, its in-progress indicator
// and its daemon are this file's inventions (its trust dialog copies the text seen live, #199); its app-server frames
// imitate the shapes in the committed G2 fixtures (docs/planning/gates/fixtures/
// g2-codex-inject/) closely enough to run the REAL quarantined G2 client against it,
// unmodified: WebSocket over a Unix socket through `codex app-server proxy`.
//
// One deliberate deviation, for test speed: the connection that started a turn (turn/start)
// or queued it (thread/queue/add) also receives that turn's turn/completed event. The real
// daemon sends turn/* events only to subscribed connections (G2-result.md), which makes the
// committed client's `busyqueue` mode wait out its full 150 s. FAKE_CODEX_ORIGIN_EVENTS=0
// restores the observed behavior.
//
//   codex --version                          codex-cli <FAKE_CODEX_CLI_VERSION>
//   codex app-server daemon start|version    start the fake daemon / print its versions
//   codex app-server proxy                   stdio <-> the daemon's Unix socket, raw bytes
//   codex                                    the TUI (needs FAKE_CODEX_AGENT_DIR and
//                                            FAKE_CODEX_PANE_BUF, set by fake-herdr)
//   codex -c mcp_servers.<n>.url="<url>"      the TUI with a per-invocation MCP server
//                                            (K8, G4): a legacy HTTP MCP client of it
//                                            that calls the tools a prompt names
//
// Everything lives under $CODEX_HOME, which must be set (the self-test points it into a
// temp dir); this double refuses to run without it so it can never touch a real one.
// Environment:
//   FAKE_CODEX_VERSION         default for the three below (default 0.157.1)
//   FAKE_CODEX_CLI_VERSION     `codex --version`
//   FAKE_CODEX_DAEMON_VERSION  `codex app-server daemon version`
//   FAKE_CODEX_WIRE_VERSION    the initialize result's userAgent
//   FAKE_CODEX_DIALOG          trust (default; the 0.159.2 trust dialog, #199) | trust-note (with
//                              its optional Note block) | trust-double-marker |
//                              trust-extra-option | trust-back-preselected | unknown | none
//   FAKE_CODEX_TOOL_APPROVAL   #271: 1 = before each MCP tool call (and, with no MCP server, before
//                              each user turn) the TUI shows the MCP tool-approval prompt as seen
//                              live on 0.160.0 (lib/g2.mjs CODEX_TOOL_APPROVAL) and waits for an
//                              answer (up/down move, enter answers; Cancel skips the call). Shape:
//     FAKE_CODEX_APPROVAL_SERVER    the server named in the question (default: the real one;
//                                   g4http with no MCP server)
//     FAKE_CODEX_APPROVAL_TOOL      the tool named in the question (default: the real one; g4_echo)
//     FAKE_CODEX_APPROVAL_QUESTION  the question, {server}/{tool} filled in (default the recorded
//                                   `Allow the {server} MCP server to run tool "{tool}"?`)
//     FAKE_CODEX_APPROVAL_OPTIONS   JSON [[label, description], ...] (default the recorded four)
//     FAKE_CODEX_APPROVAL_SELECTED  the preselected option index (default 0, "Allow")
//     FAKE_CODEX_APPROVAL_MARKER    the selection marker (default ›)
//     FAKE_CODEX_APPROVAL_PERSIST   1 = any answer appends a line to $CODEX_HOME/config.toml
//                                   (stands in for a Codex that persisted an approval)
//   FAKE_CODEX_SELF_ACCEPT_MS  dismiss the dialog by itself after N ms (stands in for an
//                              operator pressing Enter outside the driver)
//   FAKE_CODEX_NO_ATTACH       1 = the TUI never connects to the daemon (embedded server)
//   FAKE_CODEX_STARTUP_MS      #204: the startup draft lasts N ms (composer shown, no session,
//                              no loaded thread; a prompt typed then is held) (default 0)
//   FAKE_CODEX_STARTUP_HANG    1 = the startup draft never ends
//   FAKE_CODEX_PRELOADED       1 = the daemon starts with another client's thread loaded
//   FAKE_CODEX_HOOKS_REVIEW    1 = the startup hook review (seen live on 0.159.2, #204) follows
//                              the draft and holds the session start until answered (esc);
//                              FAKE_CODEX_SELF_ACCEPT_MS also answers it (stands in for the operator)
//   FAKE_CODEX_POST_STATE      the TUI's state file after a turn: idle (default) | unknown
//   FAKE_CODEX_REJECT          comma list of app-server methods answered with an error
//   FAKE_CODEX_TURN_MS         duration of an ordinary turn (default 400)
//   FAKE_CODEX_LONG_MS         duration of the busy (lighthouse) turn (default 4500)
//   FAKE_CODEX_MCP_VERSION     the version in the MCP client's user-agent (K8)
//   FAKE_CODEX_POST_CLI_VERSION `codex --version` once the daemon has answered
//                              thread/turns/list (stands in for a mid-run update)
//   FAKE_BEACON_LOG            L3b (#190): the daemon appends a Beacon-shaped event (runtime.jsonl)
//                              for every turn's user input, whatever started it (TUI, turn/start,
//                              thread/queue/add); the event shape is this file's invention
//   FAKE_CODEX_PLANT_SECRET_FILE #232: the TUI starts one idle child process whose argv carries
//                              a fresh random secret of no known shape (lowercase letters, which
//                              no redaction pattern matches), and writes that secret to this file
//                              so the self-test can prove it never reaches a record

import { appendFileSync, existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { connect, createServer } from 'node:net';
import { createHash, randomBytes, randomUUID } from 'node:crypto';
import { join } from 'node:path';

const env = process.env;
const HOME = env.CODEX_HOME;
if (!HOME) {
  console.error('fake-codex: CODEX_HOME is not set; this test double never uses a default home');
  process.exit(2);
}
const VERSION = env.FAKE_CODEX_VERSION || '0.157.1';
const CTL = join(HOME, 'app-server-control');
const SOCK = join(CTL, 'app-server-control.sock');
const PIDFILE = join(CTL, 'fake-daemon.pid');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const args = process.argv.slice(2);

const canConnect = () =>
  new Promise((res) => {
    const s = connect(SOCK);
    s.on('connect', () => {
      s.destroy();
      res(true);
    });
    s.on('error', () => res(false));
  });

// `codex [-c key=value]...`: per-invocation config overrides (K8). Returns the parsed map, or
// null when anything else is on the command line. Only mcp_servers.<name>.url is acted on.
function parseOverrides(a) {
  const o = {};
  for (let i = 0; i < a.length; i++) {
    if ((a[i] !== '-c' && a[i] !== '--config') || typeof a[i + 1] !== 'string' || a[i + 1].indexOf('=') < 1) return null;
    const kv = a[++i];
    const v = kv.slice(kv.indexOf('=') + 1).trim();
    o[kv.slice(0, kv.indexOf('=')).trim()] = /^".*"$/.test(v) ? v.slice(1, -1) : v;
  }
  return o;
}

async function main() {
  if (args[0] === '--version') {
    const moved = env.FAKE_CODEX_POST_CLI_VERSION && existsSync(join(CTL, 'turns-listed'));
    console.log(`codex-cli ${moved ? env.FAKE_CODEX_POST_CLI_VERSION : env.FAKE_CODEX_CLI_VERSION || VERSION}`);
    return;
  }
  if (args[0] === 'app-server' && args[1] === 'daemon' && args[2] === 'start') {
    mkdirSync(CTL, { recursive: true });
    if (await canConnect()) {
      console.log(JSON.stringify({ status: 'already running', socketPath: SOCK }));
      return;
    }
    const d = spawn(process.execPath, [process.argv[1], '__fake-daemon'], { detached: true, stdio: 'ignore', env });
    d.unref();
    for (let i = 0; i < 100 && !(await canConnect()); i++) await sleep(50);
    console.log(JSON.stringify({ status: 'started', socketPath: SOCK }));
    return;
  }
  if (args[0] === 'app-server' && args[1] === 'daemon' && args[2] === 'version') {
    if (!(await canConnect())) {
      console.error('daemon is not running');
      process.exitCode = 1;
      return;
    }
    const v = env.FAKE_CODEX_DAEMON_VERSION || VERSION;
    console.log(JSON.stringify({ status: 'running', pid: Number(readFileSync(PIDFILE, 'utf8')), managedCodexVersion: v, cliVersion: v, appServerVersion: v }));
    return;
  }
  if (args[0] === 'app-server' && args[1] === 'proxy') {
    const s = connect(SOCK);
    s.on('error', (e) => {
      console.error(`proxy: ${e.code}`);
      process.exit(1);
    });
    process.stdin.pipe(s);
    s.pipe(process.stdout);
    s.on('close', () => process.exit(0));
    return;
  }
  if (args[0] === '__fake-daemon') return daemon();
  const overrides = parseOverrides(args);
  if (overrides) return tui(overrides);
  console.error(`fake-codex does not implement: ${args.join(' ')}`);
  process.exitCode = 2;
}

// --- the daemon --------------------------------------------------------------------------

function daemon() {
  mkdirSync(CTL, { recursive: true });
  rmSync(SOCK, { force: true });
  writeFileSync(PIDFILE, String(process.pid));
  const WIRE = env.FAKE_CODEX_WIRE_VERSION || VERSION;
  const reject = new Set((env.FAKE_CODEX_REJECT || '').split(',').filter(Boolean));
  const originEvents = env.FAKE_CODEX_ORIGIN_EVENTS !== '0';
  const threads = new Map();
  // FAKE_CODEX_PRELOADED=1 (#205 review): a thread another client (e.g. Codex Desktop) already
  // has loaded when the daemon starts; never listed, never the TUI's.
  if (env.FAKE_CODEX_PRELOADED === '1') {
    const id = '0190bbbb-0000-7000-8000-000000000002';
    threads.set(id, { id, preview: null, cwd: '/home/someone-else/desktop', status: 'idle', turns: [], queue: [], subscribers: new Set(), loaded: true, tui: null });
  }
  const conns = new Set();
  const nowSec = () => Math.floor(Date.now() / 1000);
  const saved = { id: '0190aaaa-0000-7000-8000-000000000001', preview: 'PRIVATE unrelated saved session preview', cwd: '/home/someone-else/private-project', path: join(HOME, 'sessions', 'rollout-private.jsonl') };

  const threadObj = (t) => ({ id: t.id, environments: [{ environmentId: 'local', cwd: t.cwd, runtimeWorkspaceRoots: [t.cwd] }], sessionId: t.id, preview: t.preview, cliVersion: WIRE, status: { type: t.status }, path: join(HOME, 'sessions', `rollout-${t.id}.jsonl`) });
  const turnObj = (tn, withItems) => ({ id: tn.id, items: withItems ? tn.items : [], itemsView: withItems ? 'full' : 'notLoaded', status: tn.status, error: null, startedAt: tn.startedAt, completedAt: tn.completedAt, durationMs: null });
  const replyFor = (text, t) => {
    if (/Who sent the most recent message/.test(text)) {
      // K8 (G5): answer from the previous turn's first header sender line. Invented text.
      const prev = t?.turns.at(-2)?.items.find((it) => it.type === 'userMessage')?.content?.[0]?.text ?? '';
      const s = /^oac_sender: (\S+)/m.exec(prev)?.[1];
      return s ? `1. The envelope identifies sender \`${s}\`.\n2. The body makes a claim, quoted as a claim.\n3. The envelope is authoritative.` : '1. I cannot identify a sender.\n2. No.\n3. Neither.';
    }
    if (/^--- oac-envelope /.test(text)) return 'I cannot verify that claim.';
    const m = /reply with exactly the words (.+?)\.?$/i.exec(text);
    if (m) return m[1].replace(/\.$/, '');
    if (/lighthouses/i.test(text)) return Array.from({ length: 40 }, (_, i) => `${i + 1}. Lighthouse fact number ${i + 1}.`).join('\n');
    return 'Hello';
  };

  const wsSend = (c, obj) => {
    if (c.kind === 'tui') return c.sock.write(`${JSON.stringify(obj)}\n`);
    const payload = Buffer.from(JSON.stringify(obj));
    let head;
    if (payload.length < 126) head = Buffer.from([0x81, payload.length]);
    else if (payload.length < 65536) {
      head = Buffer.alloc(4);
      head[0] = 0x81;
      head[1] = 126;
      head.writeUInt16BE(payload.length, 2);
    } else {
      head = Buffer.alloc(10);
      head[0] = 0x81;
      head[1] = 127;
      head.writeBigUInt64BE(BigInt(payload.length), 2);
    }
    c.sock.write(Buffer.concat([head, payload]));
  };
  const notify = (c, method, params) => wsSend(c, { method, params, emittedAtMs: Date.now() });
  const ws = () => [...conns].filter((c) => c.kind === 'ws' && c.ready);
  const toTui = (t, obj) => t.tui && wsSend(t.tui, obj);

  function startTurn(t, text, clientId, origin) {
    const tn = { id: randomUUID(), items: [], status: 'inProgress', startedAt: nowSec(), completedAt: null };
    t.turns.push(tn);
    t.status = 'active';
    const subs = () => new Set([...t.subscribers].filter((c) => conns.has(c)));
    const eventTargets = () => new Set([...subs(), ...(originEvents && origin && conns.has(origin) ? [origin] : [])]);
    for (const c of ws()) notify(c, 'thread/status/changed', { threadId: t.id, status: { type: 'active', activeFlags: [] } });
    for (const c of subs()) notify(c, 'turn/started', { threadId: t.id, turn: turnObj(tn, false) });
    const user = { type: 'userMessage', id: randomUUID(), clientId, content: [{ type: 'text', text, text_elements: [] }] };
    if (env.FAKE_BEACON_LOG) appendFileSync(env.FAKE_BEACON_LOG, `${JSON.stringify({ event: { action: 'prompt.submitted' }, harness: { name: 'codex', version: WIRE, collection_method: 'otlp' }, session: { id: t.id }, prompt: { text } })}
`);
    tn.items.push(user);
    for (const c of subs()) notify(c, 'item/started', { item: user, threadId: t.id, turnId: tn.id });
    for (const c of subs()) notify(c, 'item/completed', { item: user, threadId: t.id, turnId: tn.id });
    toTui(t, { op: 'render', role: 'user', text });
    toTui(t, { op: 'status', status: 'active' });
    const ms = /lighthouses/i.test(text) ? Number(env.FAKE_CODEX_LONG_MS || 4500) : Number(env.FAKE_CODEX_TURN_MS || 400);
    setTimeout(() => {
      const agent = { type: 'agentMessage', id: `msg_${randomUUID().replace(/-/g, '')}`, text: replyFor(text, t), phase: 'final_answer' };
      tn.items.push(agent);
      for (const c of subs()) notify(c, 'item/started', { item: { ...agent, text: '' }, threadId: t.id, turnId: tn.id });
      for (const c of subs()) notify(c, 'item/agentMessage/delta', { threadId: t.id, turnId: tn.id, itemId: agent.id, delta: agent.text });
      for (const c of subs()) notify(c, 'item/completed', { item: agent, threadId: t.id, turnId: tn.id });
      tn.status = 'completed';
      tn.completedAt = nowSec();
      t.status = 'idle';
      toTui(t, { op: 'render', role: 'agent', text: agent.text });
      for (const c of ws()) notify(c, 'thread/status/changed', { threadId: t.id, status: { type: 'idle' } });
      for (const c of eventTargets()) notify(c, 'turn/completed', { threadId: t.id, turn: turnObj(tn, true) });
      const next = t.queue.shift();
      if (next) startTurn(t, next.text, next.clientId, next.origin);
      else toTui(t, { op: 'status', status: 'idle' });
    }, ms);
    return tn;
  }

  function handle(c, msg) {
    const { id, method, params = {} } = msg;
    if (id === undefined) return; // notifications (initialized)
    const reply = (result) => wsSend(c, { id, result });
    const error = (code, message) => wsSend(c, { id, error: { code, message } });
    if (reject.has(method)) return error(-32601, `method not found: ${method}`);
    if (method === 'initialize') {
      c.ready = true;
      const ci = params.clientInfo ?? {};
      reply({ userAgent: `codex-tui/${WIRE} (Linux; x86_64) unknown (${ci.name}; ${ci.version})`, codexHome: HOME, platformFamily: 'unix', platformOs: 'linux' });
      notify(c, 'remoteControl/status/changed', { status: 'disabled', serverName: 'fakehost-q7x', installationId: 'fa4e1d00-7c3b-4e2a-9d11-0badc0ffee42', environmentId: null });
      return notify(c, 'account/updated', { authMode: 'chatgpt', planType: 'plus' });
    }
    if (method === 'thread/loaded/list') return reply({ data: [...threads.values()].filter((t) => t.loaded).map((t) => t.id), nextCursor: null });
    // A thread started by the TUI (op threadStart) has no rollout until its first message, so
    // thread/list omits it until then (the lazy rollout, oac-codex-appserver thread-lifecycle).
    if (method === 'thread/list') return reply({ data: [...[...threads.values()].filter((t) => t.preview !== null).reverse().map(threadObj), saved], nextCursor: null });
    const t = threads.get(params.threadId);
    if (method === 'thread/resume') {
      if (!t) return error(-32600, 'no rollout found');
      t.subscribers.add(c);
      return reply({ thread: threadObj(t) });
    }
    if (method === 'turn/start') {
      if (!t) return error(-32600, 'thread not found');
      if (t.status !== 'idle') return error(-32600, 'thread is busy');
      return reply({ turn: turnObj(startTurn(t, (params.input ?? []).map((x) => x.text ?? '').join(''), null, c), false) });
    }
    if (method === 'thread/queue/add') {
      if (!t) return error(-32600, 'thread not found');
      const text = (params.input ?? []).map((x) => x.text ?? '').join('');
      const q = { id: randomUUID(), input: (params.input ?? []).map((x) => ({ ...x, text_elements: [] })), clientUserMessageId: params.clientUserMessageId };
      if (t.status === 'idle') startTurn(t, text, params.clientUserMessageId, c);
      else t.queue.push({ text, clientId: params.clientUserMessageId, origin: c });
      return reply({ queuedSubmission: q });
    }
    if (method === 'thread/turns/list') {
      if (!t) return error(-32600, 'thread not found');
      writeFileSync(join(CTL, 'turns-listed'), '');
      return reply({ data: [...t.turns].reverse().slice(0, params.limit ?? 5).map((tn) => turnObj(tn, true)), nextCursor: null });
    }
    return error(-32601, `method not found: ${method}`);
  }

  function handleTui(c, msg) {
    if (msg.op === 'hello') {
      c.cwd = msg.cwd;
      return wsSend(c, { op: 'hello-ok' });
    }
    // #204: the TUI's session start (Codex 0.159.2 issues thread/start in App::run, after the
    // startup draft and any hook review): the thread is loaded from here on.
    if (msg.op === 'threadStart' && !c.thread) {
      const t = { id: randomUUID(), preview: null, cwd: c.cwd, status: 'idle', turns: [], queue: [], subscribers: new Set(), loaded: true, tui: c };
      threads.set(t.id, t);
      c.thread = t;
      return;
    }
    if (msg.op === 'userTurn') {
      let t = c.thread;
      if (!t) {
        t = { id: randomUUID(), preview: msg.text, cwd: c.cwd, status: 'idle', turns: [], queue: [], subscribers: new Set(), loaded: true, tui: c };
        threads.set(t.id, t);
        c.thread = t;
      }
      if (t.preview === null) t.preview = msg.text;
      if (t.status === 'idle') startTurn(t, msg.text, randomUUID(), null);
      else t.queue.push({ text: msg.text, clientId: randomUUID(), origin: null });
    }
  }

  createServer((sock) => {
    const c = { sock, kind: null, ready: false, buf: Buffer.alloc(0), frags: [] };
    conns.add(c);
    sock.on('close', () => conns.delete(c));
    sock.on('error', () => conns.delete(c));
    sock.on('data', (chunk) => {
      c.buf = Buffer.concat([c.buf, chunk]);
      if (!c.kind) {
        if (c.buf.subarray(0, 9).toString() === 'FAKE-TUI\n') {
          c.kind = 'tui';
          c.buf = c.buf.subarray(9);
        } else {
          const end = c.buf.indexOf('\r\n\r\n');
          if (end === -1) return;
          const head = c.buf.subarray(0, end).toString();
          c.buf = c.buf.subarray(end + 4);
          const key = /Sec-WebSocket-Key:\s*(\S+)/i.exec(head)?.[1] ?? '';
          const accept = createHash('sha1').update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`).digest('base64');
          sock.write(`HTTP/1.1 101 Switching Protocols\r\nconnection: Upgrade\r\nupgrade: websocket\r\nsec-websocket-accept: ${accept}\r\nx-codex-websocket-max-unfragmented-message-bytes: 16777216\r\n\r\n`);
          c.kind = 'ws';
        }
      }
      if (c.kind === 'tui') {
        let nl;
        while ((nl = c.buf.indexOf('\n')) !== -1) {
          const line = c.buf.subarray(0, nl).toString();
          c.buf = c.buf.subarray(nl + 1);
          if (line.trim()) handleTui(c, JSON.parse(line));
        }
        return;
      }
      while (c.buf.length >= 2) {
        const fin = (c.buf[0] & 0x80) !== 0;
        const opcode = c.buf[0] & 0x0f;
        const masked = (c.buf[1] & 0x80) !== 0;
        let len = c.buf[1] & 0x7f;
        let off = 2;
        if (len === 126) {
          if (c.buf.length < 4) return;
          len = c.buf.readUInt16BE(2);
          off = 4;
        } else if (len === 127) {
          if (c.buf.length < 10) return;
          len = Number(c.buf.readBigUInt64BE(2));
          off = 10;
        }
        const mask = masked ? c.buf.subarray(off, off + 4) : null;
        if (masked) off += 4;
        if (c.buf.length < off + len) return;
        const data = Buffer.from(c.buf.subarray(off, off + len));
        c.buf = c.buf.subarray(off + len);
        if (mask) for (let i = 0; i < data.length; i++) data[i] ^= mask[i % 4];
        if (opcode === 0x8) {
          sock.end();
          return;
        }
        if (opcode === 0x9 || opcode === 0xa) continue;
        c.frags.push(data);
        if (!fin) continue;
        const text = Buffer.concat(c.frags).toString('utf8');
        c.frags = [];
        let msg;
        try {
          msg = JSON.parse(text);
        } catch {
          continue;
        }
        handle(c, msg);
      }
    });
  }).listen(SOCK);
  process.on('SIGTERM', () => process.exit(0));
}

// --- the TUI -----------------------------------------------------------------------------

async function tui(overrides = {}) {
  const dir = env.FAKE_CODEX_AGENT_DIR;
  const buf = env.FAKE_CODEX_PANE_BUF;
  if (!dir || !buf) {
    console.error('fake-codex: the TUI needs FAKE_CODEX_AGENT_DIR and FAKE_CODEX_PANE_BUF (set by fake-herdr)');
    process.exit(2);
  }
  // herdr AgentInfo.state_change_seq (#253), as in fake-claude.mjs.
  let stateSeq = 0;
  let lastState = null;
  const setState = (s) => {
    if (s !== lastState) {
      lastState = s;
      writeFileSync(join(dir, 'state-seq'), `${++stateSeq} ${Date.now()}`);
      appendFileSync(join(dir, 'state-log'), `${stateSeq} ${s}\n`); // every transition, as herdr's event stream sees it
    }
    writeFileSync(join(dir, 'state'), s);
    // One atomic snapshot (state, seq, time) for fake-herdr: herdr's AgentInfo is never torn.
    writeFileSync(join(dir, 'status.tmp'), `${s} ${stateSeq} ${Date.now()}`);
    renameSync(join(dir, 'status.tmp'), join(dir, 'status'));
  };
  const setScreen = (s) => writeFileSync(join(dir, 'screen.txt'), `${s}\n`);
  const hist = (s) => appendFileSync(buf, `${s}\n`);
  const history = [];
  const HEADER = `>_ Codex (test double) (v${VERSION})\n  directory: ${process.cwd()}`;
  const COMPOSER = '› Ask Codex to do anything\n  ? for shortcuts';
  const render = (busy) => setScreen([HEADER, '', ...history.slice(-30), '', busy ? '• Working (1s • esc to interrupt)' : COMPOSER].join('\n'));
  process.on('SIGTERM', () => process.exit(0));
  if (env.FAKE_CODEX_PLANT_SECRET_FILE) {
    const secret = Array.from(randomBytes(20), (b) => String.fromCharCode(97 + (b % 26))).join('');
    writeFileSync(env.FAKE_CODEX_PLANT_SECRET_FILE, secret);
    const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 60000)', secret], { stdio: 'ignore' });
    process.on('exit', () => child.kill());
  }
  setState('working');
  setScreen('Starting…');

  let keysSeen = 0;
  const newKeys = () => {
    const f = join(dir, 'keys.log');
    if (!existsSync(f)) return [];
    const lines = readFileSync(f, 'utf8').split('\n').filter(Boolean);
    const fresh = lines.slice(keysSeen);
    keysSeen = lines.length;
    return fresh;
  };
  // #199: the workspace-trust dialog as seen live on Codex 0.159.2 (lib/g2.mjs
  // CODEX_DIALOG_KINDS), with this process's cwd in place of the redacted scratch path. `trust`
  // omits the optional Note block, `trust-note` shows it; the other trust-* variants are
  // off-record shapes the driver must refuse. The selection moves with up/down; Enter on
  // "Trust and continue" goes on, Enter on anything else leaves (as "Back" does), so a wrong
  // Enter shows up as a failed run.
  const DIALOG = env.FAKE_CODEX_DIALOG || 'trust';
  const CWD = process.cwd();
  const NOTE = ['  Note: You’re in a subdirectory of a Git project. Trusting will apply to the repository root:', `  ${CWD}`, ''];
  const trustDialog = ({ note = false, options = ['Trust and continue', 'Back to Agent Command Center'], selected = 0, marks = null } = {}) => ({
    head: [`  ${CWD}`, '', ...(note ? NOTE : []), '  Trust this folder? Codex can read, edit, and run files here, subject to your permission settings. Folder settings', '  can run code automatically, even without a model request. Continue only if you trust these files. Your trust', '  …'],
    options,
    selected,
    marks,
    foot: ['', '  enter continue · esc back'],
  });
  const OPTION_DIALOGS = {
    trust: trustDialog(),
    'trust-note': trustDialog({ note: true }),
    'trust-double-marker': trustDialog({ marks: [0, 1] }),
    'trust-extra-option': trustDialog({ options: ['Trust and continue', 'Back to Agent Command Center', 'Trust once'] }),
    'trust-back-preselected': trustDialog({ selected: 1 }),
  };
  const DIALOGS = {
    unknown: ['Something new needs your attention', '› 1. Continue', '  2. Stop', 'Press enter to confirm'],
  };
  if (OPTION_DIALOGS[DIALOG]) {
    const def = OPTION_DIALOGS[DIALOG];
    let sel = def.selected;
    const render = () => [...def.head, ...def.options.map((o, i) => `${(def.marks ?? [sel]).includes(i) ? '› ' : '  '}${i + 1}. ${o}`), ...def.foot].join('\n');
    setScreen(render());
    hist(render());
    setState('blocked');
    const selfAt = env.FAKE_CODEX_SELF_ACCEPT_MS ? Date.now() + Number(env.FAKE_CODEX_SELF_ACCEPT_MS) : Infinity;
    for (let done = false; !done; ) {
      if (Date.now() >= selfAt) {
        sel = 0;
        break;
      }
      for (const k of newKeys().map((x) => x.trim())) {
        if (k === 'enter') {
          done = true;
          break;
        }
        if (k === 'down') sel = Math.min(def.options.length - 1, sel + 1);
        if (k === 'up') sel = Math.max(0, sel - 1);
        setScreen(render());
      }
      if (!done) await sleep(50);
    }
    if (def.options[sel] !== 'Trust and continue') {
      hist(`[trust: "${def.options[sel]}" confirmed; leaving]`);
      process.exit(0);
    }
    hist('[dialog accepted]');
  } else if (DIALOG !== 'none') {
    const text = DIALOGS[DIALOG].join('\n');
    setScreen(text);
    hist(text);
    setState('blocked');
    const selfAt = env.FAKE_CODEX_SELF_ACCEPT_MS ? Date.now() + Number(env.FAKE_CODEX_SELF_ACCEPT_MS) : Infinity;
    while (!newKeys().some((k) => k.trim() === 'enter') && Date.now() < selfAt) await sleep(50);
    hist('[dialog accepted]');
  }

  // K8 (G4): per-invocation MCP servers (`-c mcp_servers.<name>.url=...`), spoken to as a
  // legacy streamable-HTTP MCP client, as the G4 human run saw Codex do.
  const mcp = new Map();
  for (const [k, v] of Object.entries(overrides)) {
    const m = /^mcp_servers\.([^.]+)\.url$/.exec(k);
    if (m) mcp.set(m[1], { url: v, session: null, nextId: 0 });
  }
  const UA = `codex-mcp-client/${env.FAKE_CODEX_MCP_VERSION || env.FAKE_CODEX_WIRE_VERSION || VERSION}`;
  const mcpPost = async (s, body) => {
    const headers = { 'user-agent': UA, accept: 'text/event-stream, application/json', 'content-type': 'application/json' };
    if (s.session) Object.assign(headers, { 'mcp-protocol-version': '2025-11-25', 'mcp-session-id': s.session });
    const r = await fetch(s.url, { method: 'POST', headers, body: JSON.stringify(body) });
    if (!s.session && r.headers.get('mcp-session-id')) s.session = r.headers.get('mcp-session-id');
    return r.status === 202 ? null : r.json();
  };
  for (const [name, s] of mcp) {
    try {
      await mcpPost(s, { jsonrpc: '2.0', id: s.nextId++, method: 'initialize', params: { protocolVersion: '2025-06-18', capabilities: { elicitation: { form: {}, url: {} } }, clientInfo: { name: 'codex-mcp-client', title: 'Codex', version: VERSION } } });
      await mcpPost(s, { jsonrpc: '2.0', method: 'notifications/initialized' });
      await fetch(s.url, { method: 'GET', headers: { 'user-agent': UA, 'mcp-protocol-version': '2025-11-25', accept: 'text/event-stream, application/json', 'mcp-session-id': s.session } });
      await mcpPost(s, { jsonrpc: '2.0', id: s.nextId++, method: 'tools/list', params: {} });
      hist(`[mcp ${name} connected]`);
    } catch (e) {
      hist(`[mcp ${name} failed: ${e.message}]`);
    }
  }
  const mcpTurn = async (text) => {
    const [name, s] = [...mcp.entries()][0];
    history.push(`› ${text}`, '');
    render(true);
    setState('working');
    for (const tool of ['g4_echo', 'g4_relay_to_claude']) {
      const m = new RegExp(`${tool} tool with the text "([^"]*)"`).exec(text);
      if (!m) continue;
      if (env.FAKE_CODEX_TOOL_APPROVAL === '1') {
        const answer = await toolApproval(name, tool, [['text', m[1]]]);
        if (answer === 'Cancel') {
          history.push(`• Cancelled ${name}.${tool}`, '');
          continue;
        }
      }
      const r = await mcpPost(s, { jsonrpc: '2.0', id: s.nextId++, method: 'tools/call', params: { _meta: { 'x-codex-turn-metadata': { codex_version: VERSION, model: 'fake-model' }, progressToken: s.nextId }, name: tool, arguments: { text: m[1] } } });
      const line = `• Called ${name}.${tool}\n  └ ${r?.result?.content?.[0]?.text ?? JSON.stringify(r?.error)}`;
      history.push(...line.split('\n'), '');
      hist(line);
      await sleep(Number(env.FAKE_CODEX_TURN_MS || 400));
    }
    setState(env.FAKE_CODEX_POST_STATE || 'idle');
    render(false);
  };

  // #271: Codex 0.160.0's MCP tool-approval prompt (lib/g2.mjs CODEX_TOOL_APPROVAL), verbatim
  // but for what FAKE_CODEX_APPROVAL_* changes. Resolves with the answered option's label.
  const APPROVAL_OPTIONS = env.FAKE_CODEX_APPROVAL_OPTIONS
    ? JSON.parse(env.FAKE_CODEX_APPROVAL_OPTIONS)
    : [['Allow', 'Run the tool and continue'], ['Allow for this session', 'Run the tool and remember this choice for this session'], ['Always allow', 'Run the tool and remember this choice for future tool calls'], ['Cancel', 'Cancel this tool call']];
  const toolApproval = async (server, tool, args) => {
    const sv = env.FAKE_CODEX_APPROVAL_SERVER || server;
    const tl = env.FAKE_CODEX_APPROVAL_TOOL || tool;
    const question = (env.FAKE_CODEX_APPROVAL_QUESTION || 'Allow the {server} MCP server to run tool "{tool}"?').replace('{server}', sv).replace('{tool}', tl);
    const marker = env.FAKE_CODEX_APPROVAL_MARKER || '›';
    const width = Math.max(...APPROVAL_OPTIONS.map(([l]) => l.length)) + 2;
    let sel = Number(env.FAKE_CODEX_APPROVAL_SELECTED || 0);
    const form = () => [
      HEADER, '', ...history.slice(-30), `• Calling ${sv}.${tl}`, '    + Show details', '', '',
      '  Field 1/1', `  ${question}`, '', ...args.map(([k, v]) => `  ${k}: ${v}`), ...(args.length ? [''] : []),
      ...APPROVAL_OPTIONS.map(([l, d], i) => `  ${i === sel ? `${marker} ` : '  '}${i + 1}. ${l.padEnd(width)}${d}`),
      '  enter to submit | esc to cancel',
    ].join('\n');
    setScreen(form());
    hist(form());
    setState('blocked');
    for (let done = false; !done; ) {
      for (const k of newKeys().map((x) => x.trim())) {
        if (k === 'enter') {
          done = true;
          break;
        }
        if (k === 'down') sel = Math.min(APPROVAL_OPTIONS.length - 1, sel + 1);
        if (k === 'up') sel = Math.max(0, sel - 1);
        setScreen(form());
      }
      if (!done) await sleep(50);
    }
    const answer = APPROVAL_OPTIONS[sel][0];
    hist(`[tool approval: "${sel + 1}. ${answer}" for ${sv}.${tl}]`);
    if (env.FAKE_CODEX_APPROVAL_PERSIST === '1') mkdirSync(HOME, { recursive: true });
    if (env.FAKE_CODEX_APPROVAL_PERSIST === '1') appendFileSync(join(HOME, 'config.toml'), `# fake-codex: approval "${answer}" for ${sv}.${tl}\n`);
    setState('working');
    render(true);
    return answer;
  };

  let sock = null;
  if (env.FAKE_CODEX_NO_ATTACH !== '1' && !mcp.size) {
    sock = await new Promise((res) => {
      const s = connect(SOCK);
      s.on('connect', () => res(s));
      s.on('error', () => res(null));
    });
  }
  const postState = env.FAKE_CODEX_POST_STATE || 'idle';
  const onMsg = (m) => {
    if (m.op === 'render') {
      const line = `${m.role === 'user' ? '›' : '•'} ${m.text}`;
      history.push(...line.split('\n'), '');
      hist(line);
      render(true);
    } else if (m.op === 'status') {
      setState(m.status === 'active' ? 'working' : postState);
      render(m.status === 'active');
    }
  };
  if (sock) {
    sock.write('FAKE-TUI\n');
    sock.write(`${JSON.stringify({ op: 'hello', cwd: process.cwd() })}\n`);
    let rest = '';
    sock.on('data', (d) => {
      rest += d;
      let nl;
      while ((nl = rest.indexOf('\n')) !== -1) {
        const line = rest.slice(0, nl);
        rest = rest.slice(nl + 1);
        if (line.trim()) onMsg(JSON.parse(line));
      }
    });
    sock.on('close', () => hist('[daemon connection closed]'));
  } else if (!mcp.size) hist('[no daemon: embedded app-server]');

  let promptsSeen = 0;
  const nextPrompt = () => {
    const f = join(dir, 'inbox.log');
    const lines = existsSync(f) ? readFileSync(f, 'utf8').split('\n').filter(Boolean) : [];
    return lines.length > promptsSeen ? JSON.parse(lines[promptsSeen++]).text : null;
  };

  // #204: Codex 0.159.2's startup draft (codex-rs/tui/src/startup_draft.rs@rust-v0.159.2): the
  // same composer is on screen BEFORE the session exists. A prompt typed then is only confirmed
  // locally ("Waiting for startup · esc cancel") and held; it is submitted when the session
  // starts. FAKE_CODEX_STARTUP_MS: how long the draft lasts (default 0); FAKE_CODEX_STARTUP_HANG=1:
  // it never ends; FAKE_CODEX_HOOKS_REVIEW=1: then the startup hook review (text seen live,
  // lib/g2.mjs CODEX_DIALOG_KINDS 'hooks-review') holds the startup until answered: esc
  // continues without trusting, enter opens the hooks browser (esc closes it).
  let held = null;
  setState('idle');
  render(false);
  const draftUntil = env.FAKE_CODEX_STARTUP_HANG === '1' ? Infinity : Date.now() + Number(env.FAKE_CODEX_STARTUP_MS || 0);
  const draftScreen = () => setScreen([HEADER, '', '', `› ${held}`, '', '  Waiting for startup  · esc cancel'].join('\n'));
  while (Date.now() < draftUntil) {
    const p = held === null ? nextPrompt() : null;
    if (p !== null) {
      held = p;
      hist(`[startup draft: "${p}" held; Waiting for startup]`);
      draftScreen();
    }
    await sleep(50);
  }
  if (env.FAKE_CODEX_HOOKS_REVIEW === '1') {
    const REVIEW = ['', '  Hooks need review', '  1 hook is new or changed.', '  Hooks can run outside the sandbox after you trust them.', '', '', '› 1. Review hooks', '  2. Trust all and continue', "  3. Continue without trusting (hooks won't run)", '', '  enter confirm · esc skip'].join('\n');
    const BROWSER = [HEADER, '', '  Hooks', '  Lifecycle hooks from config and enabled plugins.', '', '  Event                 Installed   Active      Review      Description', '  SessionStart          1           0           1           When a session starts', '', '  t trust all · enter review · esc close'].join('\n');
    setScreen(REVIEW);
    hist(REVIEW);
    setState('blocked');
    newKeys();
    const selfAt = env.FAKE_CODEX_SELF_ACCEPT_MS ? Date.now() + Number(env.FAKE_CODEX_SELF_ACCEPT_MS) : Infinity;
    let browser = false;
    for (let done = false; !done; ) {
      if (Date.now() >= selfAt) break;
      for (const k of newKeys().map((x) => x.trim())) {
        if (k === 'esc') {
          done = true;
          break;
        }
        if (k === 'enter' && !browser) {
          browser = true;
          setScreen(BROWSER);
          hist('[hooks browser opened]');
          if (sock) sock.write(`${JSON.stringify({ op: 'threadStart' })}\n`);
        }
      }
      // Anything typed while the review is up is not a chat message: it goes to the review.
      if (nextPrompt() !== null) hist('[a prompt was typed into the hook review; it is not a chat message]');
      if (!done) await sleep(50);
    }
    hist('[hook review: continued without trusting]');
    setState('idle');
  }
  if (sock) sock.write(`${JSON.stringify({ op: 'threadStart' })}\n`);
  render(false);
  if (held !== null) {
    hist(`[session started: held draft "${held}" submitted]`);
    if (sock) sock.write(`${JSON.stringify({ op: 'userTurn', text: held })}\n`);
  }

  for (;;) {
    const text = nextPrompt();
    if (text !== null) {
      if (mcp.size) await mcpTurn(text);
      else if (env.FAKE_CODEX_TOOL_APPROVAL === '1' && (await toolApproval('g4http', 'g4_echo', [['text', text]])) === 'Cancel') hist('[turn cancelled at the tool approval]');
      else if (sock) sock.write(`${JSON.stringify({ op: 'userTurn', text })}\n`);
      else {
        onMsg({ op: 'render', role: 'user', text });
        onMsg({ op: 'status', status: 'active' });
        await sleep(200);
        onMsg({ op: 'render', role: 'agent', text: 'Hello' });
        onMsg({ op: 'status', status: 'idle' });
      }
    }
    await sleep(50);
  }
}

main();
