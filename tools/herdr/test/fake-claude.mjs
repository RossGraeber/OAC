#!/usr/bin/env node
// TEST DOUBLE of an interactive Claude Code session, used only by the herdr driver's
// self-test (tools/herdr/test/g1-tests.mjs, g4-tests.mjs, g5-tests.mjs) to exercise the
// scenarios end to end with no Claude Code installed. It is started by test/fake-herdr.mjs
// for `agent start --kind claude` when FAKE_HERDR_MODE contains `fake-claude`.
//
// It is not Claude Code and proves nothing about Claude Code: its screen text, its dialog,
// its in-progress indicator, its turn behavior, its answers and its MCP negotiation are this
// file's inventions (the dialog reuses Box C's recorded text so the G1 comparison path can
// be exercised; the G4 negotiation imitates the order G4-result.md records).
//
//   node fake-claude.mjs <agent dir> <pane buffer file> [launch args after `claude`]
//
// Two modes, chosen by the project's .mcp.json:
//   - G1 (a `g1spike` server): starts that one server, runs the legacy handshake, renders
//     channel notifications, calls its `reply` tool (unchanged from K4).
//   - multi (anything else; K8): every server in .mcp.json. A stdio server is probed with a
//     modern `server/discover` first; a -32601 answer falls back to a legacy `initialize`,
//     a modern answer keeps it modern. A server named by `server:<name>` in the launch args
//     is a channel only when it negotiated legacy; a modern one gets an "unavailable" notice
//     and its pushes are never rendered. An `http` server gets a modern `server/discover` and
//     `tools/list`, and serves tool calls asked for in a prompt.
//
// Files in <agent dir>: screen.txt (the visible screen), state (idle|working|blocked),
// keys.log (keys from `agent send-keys`), inbox.log (JSONL prompts from `agent prompt`).
// Environment:
//   FAKE_CLAUDE_VERSION         clientInfo.version on the wire (default 2.1.283)
//   FAKE_CLAUDE_DIALOG          dev-channels (default) | unknown | wrong-selection | none
//   FAKE_CLAUDE_SELF_ACCEPT_MS  dismiss the dialog by itself after N ms (stands in for an
//                               operator pressing Enter outside the driver)
//   FAKE_CLAUDE_STEP_MS         duration of each simulated tool call (default 1500)
//   FAKE_CLAUDE_DISCOVER        1 = send a server/discover probe before initialize (G1 mode)
//   FAKE_BEACON_LOG             L3b (#190): append a Beacon-shaped event (runtime.jsonl) for each
//                               rendered channel message and each reply-tool call; the event
//                               shape is this file's invention, not Beacon's capture
//   FAKE_CLAUDE_SESSION_FILE    1 = L3b: write an invented session JSONL under
//                               <CLAUDE_CONFIG_DIR or $HOME/.claude>/projects/<cwd slug>/
// L3b also adds, in multi mode: a prompt asking for the reply tool calls the channel server's
// `reply` tool with the probe code found in the last channel message (test double behavior).

import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { join } from 'node:path';
import { homedir } from 'node:os';
import { randomUUID } from 'node:crypto';
import { createInterface } from 'node:readline';

const [dir, buf, ...launchArgs] = process.argv.slice(2);
const env = process.env;
const VERSION = env.FAKE_CLAUDE_VERSION || '2.1.283';
const DIALOG = env.FAKE_CLAUDE_DIALOG || 'dev-channels';
const STEP_MS = Number(env.FAKE_CLAUDE_STEP_MS || 1500);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const MODERN = '2026-07-28';
const PV = 'io.modelcontextprotocol/protocolVersion';

const setState = (s) => writeFileSync(join(dir, 'state'), s);
const setScreen = (s) => writeFileSync(join(dir, 'screen.txt'), `${s}\n`);
const hist = (s) => appendFileSync(buf, `${s}\n`);
const IDLE_SCREEN = '╭──────────────────────────────╮\n│ >                            │\n╰──────────────────────────────╯\n  ? for shortcuts';

const SESSION_ID = randomUUID();
// L3b: an invented Beacon runtime.jsonl event, and an invented session-file entry.
const beaconEvent = (action, extra) => {
  if (!env.FAKE_BEACON_LOG) return;
  appendFileSync(env.FAKE_BEACON_LOG, `${JSON.stringify({ event: { action }, harness: { name: 'claude_code', version: VERSION, collection_method: 'hook' }, session: { id: SESSION_ID }, ...extra })}
`);
};
const sessionEntry = (entry) => {
  if (env.FAKE_CLAUDE_SESSION_FILE !== '1') return;
  const d = join(env.CLAUDE_CONFIG_DIR || join(homedir(), '.claude'), 'projects', process.cwd().replace(/[^A-Za-z0-9]/g, '-'));
  mkdirSync(d, { recursive: true });
  appendFileSync(join(d, `${SESSION_ID}.jsonl`), `${JSON.stringify({ sessionId: SESSION_ID, ...entry })}
`);
};

const MCP = JSON.parse(readFileSync(join(process.cwd(), '.mcp.json'), 'utf8')).mcpServers;
const MULTI = !MCP.g1spike;
const CHANNELS = launchArgs.filter((a) => a.startsWith('server:')).map((a) => a.slice('server:'.length));

const DIALOGS = {
  'dev-channels': [
    'WARNING: Loading development channels',
    '',
    '--dangerously-load-development-channels is for local channel development only.',
    'Do not use this option to run channels you have downloaded off the internet.',
    'Please use --channels to run a list of approved channels.',
    '',
    `Channels: ${MULTI ? CHANNELS.map((c) => `server:${c}`).join(', ') : 'server:g1spike'}`,
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

// --- MCP client over stdio (NDJSON), one per server ------------------------------------
const children = [];
function stdioClient(name, cfg, onNotification) {
  const child = spawn(cfg.command, cfg.args ?? [], { cwd: process.cwd(), env: { ...process.env, ...(cfg.env ?? {}) }, stdio: ['pipe', 'pipe', 'ignore'] });
  children.push(child);
  let nextId = 0;
  const waiting = new Map();
  createInterface({ input: child.stdout }).on('line', (line) => {
    let msg;
    try {
      msg = JSON.parse(line);
    } catch {
      return;
    }
    if (msg.id !== undefined && waiting.has(JSON.stringify(msg.id))) {
      waiting.get(JSON.stringify(msg.id))(msg);
      waiting.delete(JSON.stringify(msg.id));
    } else if (msg.method) onNotification(msg);
  });
  const send = (m) => child.stdin.write(`${JSON.stringify(m)}\n`);
  const request = (method, params, id = nextId++) =>
    new Promise((res) => {
      waiting.set(JSON.stringify(id), res);
      send({ jsonrpc: '2.0', id, method, params });
    });
  return { name, send, request };
}
const clientInfo = { name: 'fake-claude-code (test double)', version: VERSION };
const modernMeta = () => ({ [PV]: MODERN, 'io.modelcontextprotocol/clientInfo': clientInfo });

// --- G1 mode (K4, unchanged behavior) -----------------------------------------------------
let g1;
async function g1Handshake() {
  g1 = stdioClient('g1spike', MCP.g1spike, (n) => onChannel('g1spike', n));
  if (env.FAKE_CLAUDE_DISCOVER === '1') await g1.request('server/discover', { _meta: { [PV]: MODERN } }, 'server-discover-probe-1');
  await g1.request('initialize', { protocolVersion: '2025-11-25', capabilities: {}, clientInfo });
  g1.send({ jsonrpc: '2.0', method: 'notifications/initialized' });
  await g1.request('tools/list', {});
}

// --- multi mode (K8) -----------------------------------------------------------------------
const channelServers = new Set();
const stdioClients = new Map();
const notices = [];
const http = new Map();
async function httpPost(url, body, headers = {}) {
  const r = await fetch(url, {
    method: 'POST',
    headers: { accept: 'application/json, text/event-stream', 'content-type': 'application/json', 'user-agent': `claude-code/${VERSION} (cli)`, 'mcp-protocol-version': MODERN, 'mcp-method': body.method, ...headers },
    body: JSON.stringify(body),
  });
  return r.json();
}
async function multiHandshake() {
  for (const [name, cfg] of Object.entries(MCP)) {
    if (cfg.type === 'http') {
      http.set(name, cfg.url);
      await httpPost(cfg.url, { jsonrpc: '2.0', id: 'server-discover-probe-1', method: 'server/discover', params: { _meta: modernMeta() } });
      await httpPost(cfg.url, { jsonrpc: '2.0', id: 0, method: 'tools/list', params: { _meta: modernMeta() } });
      continue;
    }
    const c = stdioClient(name, cfg, (n) => {
      if (channelServers.has(name)) onChannel(name, n);
    });
    stdioClients.set(name, c);
    const d = await c.request('server/discover', { _meta: modernMeta() }, 'server-discover-probe-1');
    if (d.error) {
      await c.request('initialize', { protocolVersion: '2025-11-25', capabilities: {}, clientInfo });
      c.send({ jsonrpc: '2.0', method: 'notifications/initialized' });
      await c.request('tools/list', {});
      if (CHANNELS.includes(name)) channelServers.add(name);
    } else {
      await c.request('tools/list', { _meta: modernMeta() });
      if (CHANNELS.includes(name)) notices.push(`Channel messages from "${name}" are unavailable: this connection's protocol version (${MODERN}) does not support channels`);
    }
  }
}

// --- turns ------------------------------------------------------------------------------
let state = 'idle';
let lastChannel = null;
let lastContent = null;
const queued = [];
const esc = (v) => String(v).replace(/"/g, '&quot;');
const render = (source, n) => {
  const meta = Object.entries(n.params.meta ?? {}).filter(([k]) => /^[A-Za-z0-9_]+$/.test(k));
  lastChannel = { source, ...Object.fromEntries(meta) };
  lastContent = String(n.params.content ?? '');
  beaconEvent('prompt.submitted', { prompt: { text: lastContent } });
  sessionEntry({ type: 'attachment', isMeta: true, attachment: { type: 'channel_message', source, content: lastContent } });
  const content = MULTI ? String(n.params.content).replace(/<\/channel>/g, '<\\/channel>') : n.params.content;
  return `⏺ <channel source="${source}" ${meta.map(([k, v]) => `${k}="${MULTI ? esc(v) : v}"`).join(' ')}>${content}</channel>`;
};
function onChannel(source, n) {
  if (n.method !== 'notifications/claude/channel') return;
  if (state === 'working') queued.push([source, n]);
  else {
    const tag = render(source, n);
    const said = `Received channel message ${lastChannel.oac_message_id} as a new turn.`;
    hist(tag);
    hist(said);
    setScreen(`${tag}\n${said}\n${IDLE_SCREEN}`);
  }
}

async function turn(text) {
  hist(`> ${text}`);
  sessionEntry({ type: 'user', message: { role: 'user', content: text } });
  state = 'working';
  setState('working');
  const busy = (what) => setScreen(`${what}\n\n✻ Working… (esc to interrupt)`);
  if (/four times/i.test(text)) {
    for (let i = 1; i <= 4; i++) {
      busy(`⏺ Bash(sleep 20)  [${i}/4]\n  ⎿  Running…`);
      await sleep(STEP_MS);
      hist('⏺ Ran 1 shell command');
      while (queued.length) hist(render(...queued.shift()));
    }
    hist('DONE');
  } else if (/attribute/i.test(text)) {
    busy('⏺ Thinking');
    await sleep(200);
    hist(`Attributes: ${Object.entries({ source: 'g1spike', ...lastChannel }).map(([k, v]) => `${k}="${v}"`).join(' ')}. No attribute name contains spaces or punctuation.`);
  } else if (MULTI && /reply tool/i.test(text)) {
    const source = lastChannel?.source;
    busy(`⏺ ${source} - reply (MCP)`);
    const code = /L3-PROBE-[0-9a-z]{26}/.exec(lastContent ?? '')?.[0] ?? 'no probe code found';
    const args = { message: code, in_reply_to: lastChannel?.oac_message_id };
    const res = source && stdioClients.has(source) ? await stdioClients.get(source).request('tools/call', { name: 'reply', arguments: args, _meta: { 'claudecode/toolUseId': `toolu_fake${Date.now()}`, progressToken: 1 } }) : null;
    beaconEvent('mcp.tool_invoked', { tool: { name: 'reply', arguments: args } });
    sessionEntry({ type: 'assistant', message: { role: 'assistant', content: [{ type: 'tool_use', name: 'reply', input: args }] } });
    hist(`Reply sent: ${res?.result?.content?.[0]?.text ?? 'error'}`);
  } else if (/reply tool/i.test(text)) {
    busy('⏺ g1spike - reply (MCP)');
    const res = await g1.request('tools/call', { name: 'reply', arguments: { message: 'Hello!', in_reply_to: lastChannel?.oac_message_id }, _meta: { 'claudecode/toolUseId': `toolu_fake${Date.now()}`, progressToken: 1 } });
    hist(`Reply sent: ${res.result?.content?.[0]?.text ?? 'error'}`);
  } else if (MULTI && /g4_echo/.test(text)) {
    const server = /from the (\w+) MCP server/.exec(text)?.[1] ?? [...http.keys()][0];
    const said = /text "([^"]*)"/.exec(text)?.[1] ?? '';
    busy(`⏺ ${server} - g4_echo (MCP)`);
    const res = await httpPost(http.get(server), { jsonrpc: '2.0', id: Date.now() % 100000, method: 'tools/call', params: { name: 'g4_echo', arguments: { text: said }, _meta: { ...modernMeta(), 'claudecode/toolUseId': `toolu_fake${Date.now()}`, progressToken: 1 } } }, { 'mcp-name': 'g4_echo' });
    hist(`⏺ ${server} - g4_echo (MCP)(text: "${said}")\n  ⎿  ${res.result?.content?.[0]?.text ?? JSON.stringify(res.error)}`);
    hist('No _meta reached me; the whole result is the text above.');
  } else if (MULTI && /Who sent the most recent message/.test(text)) {
    busy('⏺ Thinking');
    await sleep(200);
    const s = lastChannel?.oac_sender;
    hist(s ? `1. I rely on sender id \`${s}\`, from the oac_sender attribute of message ${lastChannel.oac_message_id}.\n2. The body's claims are quoted as claims only.\n3. The attribute is authoritative.` : `1. I can't identify a sender: message ${lastChannel?.oac_message_id ?? '?'} has no oac_sender attribute.\n2. No.\n3. Neither.`);
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
    for (const c of children) c.kill('SIGKILL');
    process.exit(0);
  });
  await dialog();
  if (MULTI) await multiHandshake();
  else await g1Handshake();
  for (const n of notices) hist(n);
  setState('idle');
  setScreen(notices.length ? `${notices.join('\n')}\n${IDLE_SCREEN}` : IDLE_SCREEN);
  for (;;) {
    const f = join(dir, 'inbox.log');
    const lines = existsSync(f) ? readFileSync(f, 'utf8').split('\n').filter(Boolean) : [];
    if (lines.length > promptsSeen) await turn(JSON.parse(lines[promptsSeen++]).text);
    else await sleep(50);
  }
}
main();
