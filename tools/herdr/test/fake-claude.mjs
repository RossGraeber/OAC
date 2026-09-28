#!/usr/bin/env node
// TEST DOUBLE of an interactive Claude Code session, used only by the herdr driver's
// self-test (tools/herdr/test/g1.mjs) to exercise tools/herdr/scenarios/g1-claude-wake.mjs
// end to end with no Claude Code installed. It is started by test/fake-herdr.mjs for
// `agent start --kind claude` when FAKE_HERDR_MODE contains `fake-claude`.
//
// It is not Claude Code and proves nothing about Claude Code: its screen text, its dialog,
// its in-progress indicator and its turn behavior are this file's inventions (the dialog
// reuses Box C's recorded text so the scenario's comparison path can be exercised). It
// speaks just enough legacy MCP over stdio to drive the real quarantined G1 channel server
// that the scenario stages: it starts the server named in the project's .mcp.json, runs
// the handshake, renders channel notifications, and calls the server's `reply` tool.
//
//   node fake-claude.mjs <agent dir> <pane buffer file>
//
// Files in <agent dir>: screen.txt (the visible screen), state (idle|working|blocked),
// keys.log (keys from `agent send-keys`), inbox.log (JSONL prompts from `agent prompt`).
// Environment:
//   FAKE_CLAUDE_VERSION         clientInfo.version on the wire (default 2.1.283)
//   FAKE_CLAUDE_DIALOG          dev-channels (default) | unknown | wrong-selection | none
//   FAKE_CLAUDE_SELF_ACCEPT_MS  dismiss the dialog by itself after N ms (stands in for an
//                               operator pressing Enter outside the driver)
//   FAKE_CLAUDE_STEP_MS         duration of each simulated tool call (default 1500)
//   FAKE_CLAUDE_DISCOVER        1 = send a server/discover probe before initialize

import { appendFileSync, existsSync, readFileSync, writeFileSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

const [dir, buf] = process.argv.slice(2);
const env = process.env;
const VERSION = env.FAKE_CLAUDE_VERSION || '2.1.283';
const DIALOG = env.FAKE_CLAUDE_DIALOG || 'dev-channels';
const STEP_MS = Number(env.FAKE_CLAUDE_STEP_MS || 1500);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const setState = (s) => writeFileSync(join(dir, 'state'), s);
const setScreen = (s) => writeFileSync(join(dir, 'screen.txt'), `${s}\n`);
const hist = (s) => appendFileSync(buf, `${s}\n`);
const IDLE_SCREEN = '╭──────────────────────────────╮\n│ >                            │\n╰──────────────────────────────╯\n  ? for shortcuts';

const DIALOGS = {
  'dev-channels': [
    'WARNING: Loading development channels',
    '',
    '--dangerously-load-development-channels is for local channel development only.',
    'Do not use this option to run channels you have downloaded off the internet.',
    'Please use --channels to run a list of approved channels.',
    '',
    'Channels: server:g1spike',
    '',
    '❯ 1. I am using this for local development',
    '  2. Exit',
    '',
    'Enter to confirm · Esc to cancel',
  ],
  'wrong-selection': [
    'WARNING: Loading development channels',
    'Channels: server:g1spike',
    '  1. I am using this for local development',
    '❯ 2. Exit',
    'Enter to confirm · Esc to cancel',
  ],
  unknown: ['Something new needs your attention', '❯ 1. Continue', '  2. Stop', 'Enter to confirm · Esc to cancel'],
};

let keysSeen = 0;
const newKeys = () => {
  const f = join(dir, 'keys.log');
  if (!existsSync(f)) return [];
  const lines = readFileSync(f, 'utf8').split('\n').filter(Boolean);
  const fresh = lines.slice(keysSeen);
  keysSeen = lines.length;
  return fresh;
};

async function dialog() {
  if (DIALOG === 'none') return;
  const text = DIALOGS[DIALOG].join('\n');
  setScreen(text);
  hist(text);
  setState('blocked');
  const selfAt = env.FAKE_CLAUDE_SELF_ACCEPT_MS ? Date.now() + Number(env.FAKE_CLAUDE_SELF_ACCEPT_MS) : Infinity;
  for (;;) {
    if (newKeys().some((k) => k.trim() === 'enter') || Date.now() >= selfAt) break;
    await sleep(50);
  }
  if (DIALOG === 'wrong-selection') process.exit(0); // "Exit" was selected
  setState('working');
  setScreen('Starting…');
  hist('[dialog accepted]');
}

// --- MCP client over stdio (NDJSON) ----------------------------------------------------
let server;
let nextId = 0;
const waiting = new Map();
const onNotification = [];
function startServer() {
  const cfg = JSON.parse(readFileSync(join(process.cwd(), '.mcp.json'), 'utf8')).mcpServers.g1spike;
  server = spawn(cfg.command, cfg.args, { cwd: process.cwd(), stdio: ['pipe', 'pipe', 'ignore'] });
  createInterface({ input: server.stdout }).on('line', (line) => {
    let msg;
    try {
      msg = JSON.parse(line);
    } catch {
      return;
    }
    if (msg.id !== undefined && waiting.has(JSON.stringify(msg.id))) {
      waiting.get(JSON.stringify(msg.id))(msg);
      waiting.delete(JSON.stringify(msg.id));
    } else if (msg.method) for (const f of onNotification) f(msg);
  });
}
const send = (m) => server.stdin.write(`${JSON.stringify(m)}\n`);
const request = (method, params, id = nextId++) =>
  new Promise((res) => {
    waiting.set(JSON.stringify(id), res);
    send({ jsonrpc: '2.0', id, method, params });
  });

async function handshake() {
  const clientInfo = { name: 'fake-claude-code (test double)', version: VERSION };
  if (env.FAKE_CLAUDE_DISCOVER === '1') await request('server/discover', { _meta: { 'io.modelcontextprotocol/protocolVersion': '2026-07-28' } }, 'server-discover-probe-1');
  await request('initialize', { protocolVersion: '2025-11-25', capabilities: {}, clientInfo });
  send({ jsonrpc: '2.0', method: 'notifications/initialized' });
  await request('tools/list', {});
}

// --- turns ------------------------------------------------------------------------------
let state = 'idle';
let lastChannel = null;
const queued = [];
const render = (n) => {
  const meta = Object.entries(n.params.meta ?? {}).filter(([k]) => /^[A-Za-z0-9_]+$/.test(k));
  lastChannel = Object.fromEntries(meta);
  return `⏺ <channel source="g1spike" ${meta.map(([k, v]) => `${k}="${v}"`).join(' ')}>${n.params.content}</channel>`;
};
onNotification.push((n) => {
  if (n.method !== 'notifications/claude/channel') return;
  if (state === 'working') queued.push(n);
  else {
    const tag = render(n);
    const said = `Received channel message ${lastChannel.oac_message_id} as a new turn.`;
    hist(tag);
    hist(said);
    setScreen(`${tag}\n${said}\n${IDLE_SCREEN}`);
  }
});

async function turn(text) {
  hist(`> ${text}`);
  state = 'working';
  setState('working');
  const busy = (what) => setScreen(`${what}\n\n✻ Working… (esc to interrupt)`);
  if (/four times/i.test(text)) {
    for (let i = 1; i <= 4; i++) {
      busy(`⏺ Bash(sleep 20)  [${i}/4]\n  ⎿  Running…`);
      await sleep(STEP_MS);
      hist('⏺ Ran 1 shell command');
      while (queued.length) hist(render(queued.shift()));
    }
    hist('DONE');
  } else if (/attribute/i.test(text)) {
    busy('⏺ Thinking');
    await sleep(200);
    hist(`Attributes: ${Object.entries({ source: 'g1spike', ...lastChannel }).map(([k, v]) => `${k}="${v}"`).join(' ')}. No attribute name contains spaces or punctuation.`);
  } else if (/reply tool/i.test(text)) {
    busy('⏺ g1spike - reply (MCP)');
    const res = await request('tools/call', { name: 'reply', arguments: { message: 'Hello!', in_reply_to: lastChannel?.oac_message_id }, _meta: { 'claudecode/toolUseId': `toolu_fake${Date.now()}`, progressToken: nextId } });
    hist(`Reply sent: ${res.result?.content?.[0]?.text ?? 'error'}`);
  } else {
    await sleep(100);
    hist('ok');
  }
  state = 'idle';
  setState('idle');
  setScreen(IDLE_SCREEN);
}

let promptsSeen = 0;
async function main() {
  setState('working');
  setScreen('Starting…');
  process.on('SIGTERM', () => {
    server?.kill('SIGKILL');
    process.exit(0);
  });
  await dialog();
  startServer();
  await handshake();
  setState('idle');
  setScreen(IDLE_SCREEN);
  for (;;) {
    const f = join(dir, 'inbox.log');
    const lines = existsSync(f) ? readFileSync(f, 'utf8').split('\n').filter(Boolean) : [];
    if (lines.length > promptsSeen) await turn(JSON.parse(lines[promptsSeen++]).text);
    else await sleep(50);
  }
}
main();
