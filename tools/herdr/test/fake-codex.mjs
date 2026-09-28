#!/usr/bin/env node
// TEST DOUBLE of the Codex CLI, used only by the herdr driver's self-test
// (tools/herdr/test/g2-tests.mjs) to exercise tools/herdr/scenarios/g2-codex-inject.mjs end
// to end with no Codex installed. The self-test copies this file to <temp>/bin/codex (so a
// pane process's argv reads `node <temp>/bin/codex`, the shape of an npm-installed Codex),
// and test/fake-herdr.mjs starts it for `agent start --kind codex` in `fake-codex` mode.
//
// It is not Codex and proves nothing about Codex. Its screen text, its dialog, its
// in-progress indicator and its daemon are this file's inventions; its app-server frames
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
//
// Everything lives under $CODEX_HOME, which must be set (the self-test points it into a
// temp dir); this double refuses to run without it so it can never touch a real one.
// Environment:
//   FAKE_CODEX_VERSION         default for the three below (default 0.157.1)
//   FAKE_CODEX_CLI_VERSION     `codex --version`
//   FAKE_CODEX_DAEMON_VERSION  `codex app-server daemon version`
//   FAKE_CODEX_WIRE_VERSION    the initialize result's userAgent
//   FAKE_CODEX_DIALOG          trust (default) | unknown | none
//   FAKE_CODEX_SELF_ACCEPT_MS  dismiss the dialog by itself after N ms (stands in for an
//                              operator pressing Enter outside the driver)
//   FAKE_CODEX_NO_ATTACH       1 = the TUI never connects to the daemon (embedded server)
//   FAKE_CODEX_POST_STATE      the TUI's state file after a turn: idle (default) | unknown
//   FAKE_CODEX_REJECT          comma list of app-server methods answered with an error
//   FAKE_CODEX_TURN_MS         duration of an ordinary turn (default 400)
//   FAKE_CODEX_LONG_MS         duration of the busy (lighthouse) turn (default 4500)
//   FAKE_CODEX_POST_CLI_VERSION `codex --version` once the daemon has answered
//                              thread/turns/list (stands in for a mid-run update)

import { appendFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { connect, createServer } from 'node:net';
import { createHash, randomUUID } from 'node:crypto';
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
  if (args.length === 0) return tui();
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
  const conns = new Set();
  const nowSec = () => Math.floor(Date.now() / 1000);
  const saved = { id: '0190aaaa-0000-7000-8000-000000000001', preview: 'PRIVATE unrelated saved session preview', cwd: '/home/someone-else/private-project', path: join(HOME, 'sessions', 'rollout-private.jsonl') };

  const threadObj = (t) => ({ id: t.id, environments: [{ environmentId: 'local', cwd: t.cwd, runtimeWorkspaceRoots: [t.cwd] }], sessionId: t.id, preview: t.preview, cliVersion: WIRE, status: { type: t.status }, path: join(HOME, 'sessions', `rollout-${t.id}.jsonl`) });
  const turnObj = (tn, withItems) => ({ id: tn.id, items: withItems ? tn.items : [], itemsView: withItems ? 'full' : 'notLoaded', status: tn.status, error: null, startedAt: tn.startedAt, completedAt: tn.completedAt, durationMs: null });
  const replyFor = (text) => {
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
    tn.items.push(user);
    for (const c of subs()) notify(c, 'item/started', { item: user, threadId: t.id, turnId: tn.id });
    for (const c of subs()) notify(c, 'item/completed', { item: user, threadId: t.id, turnId: tn.id });
    toTui(t, { op: 'render', role: 'user', text });
    toTui(t, { op: 'status', status: 'active' });
    const ms = /lighthouses/i.test(text) ? Number(env.FAKE_CODEX_LONG_MS || 4500) : Number(env.FAKE_CODEX_TURN_MS || 400);
    setTimeout(() => {
      const agent = { type: 'agentMessage', id: `msg_${randomUUID().replace(/-/g, '')}`, text: replyFor(text), phase: 'final_answer' };
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
    if (method === 'thread/list') return reply({ data: [...[...threads.values()].reverse().map(threadObj), saved], nextCursor: null });
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
    if (msg.op === 'userTurn') {
      let t = c.thread;
      if (!t) {
        t = { id: randomUUID(), preview: msg.text, cwd: c.cwd, status: 'idle', turns: [], queue: [], subscribers: new Set(), loaded: true, tui: c };
        threads.set(t.id, t);
        c.thread = t;
      }
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

async function tui() {
  const dir = env.FAKE_CODEX_AGENT_DIR;
  const buf = env.FAKE_CODEX_PANE_BUF;
  if (!dir || !buf) {
    console.error('fake-codex: the TUI needs FAKE_CODEX_AGENT_DIR and FAKE_CODEX_PANE_BUF (set by fake-herdr)');
    process.exit(2);
  }
  const setState = (s) => writeFileSync(join(dir, 'state'), s);
  const setScreen = (s) => writeFileSync(join(dir, 'screen.txt'), `${s}\n`);
  const hist = (s) => appendFileSync(buf, `${s}\n`);
  const history = [];
  const HEADER = `>_ Codex (test double) (v${VERSION})\n  directory: ${process.cwd()}`;
  const COMPOSER = '› Ask Codex to do anything\n  ? for shortcuts';
  const render = (busy) => setScreen([HEADER, '', ...history.slice(-30), '', busy ? '• Working (1s • esc to interrupt)' : COMPOSER].join('\n'));
  process.on('SIGTERM', () => process.exit(0));
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
  const DIALOG = env.FAKE_CODEX_DIALOG || 'trust';
  const DIALOGS = {
    trust: ['You are running Codex in a folder it has not seen before.', '', 'Do you trust the files in this folder?', '', '› 1. Yes, continue', '  2. No, quit', '', 'Press enter to continue'],
    unknown: ['Something new needs your attention', '› 1. Continue', '  2. Stop', 'Press enter to confirm'],
  };
  if (DIALOG !== 'none') {
    const text = DIALOGS[DIALOG].join('\n');
    setScreen(text);
    hist(text);
    setState('blocked');
    const selfAt = env.FAKE_CODEX_SELF_ACCEPT_MS ? Date.now() + Number(env.FAKE_CODEX_SELF_ACCEPT_MS) : Infinity;
    while (!newKeys().some((k) => k.trim() === 'enter') && Date.now() < selfAt) await sleep(50);
    hist('[dialog accepted]');
  }

  let sock = null;
  if (env.FAKE_CODEX_NO_ATTACH !== '1') {
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
  } else hist('[no daemon: embedded app-server]');
  setState('idle');
  render(false);

  let promptsSeen = 0;
  for (;;) {
    const f = join(dir, 'inbox.log');
    const lines = existsSync(f) ? readFileSync(f, 'utf8').split('\n').filter(Boolean) : [];
    if (lines.length > promptsSeen) {
      const text = JSON.parse(lines[promptsSeen++]).text;
      if (sock) sock.write(`${JSON.stringify({ op: 'userTurn', text })}\n`);
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
