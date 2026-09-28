#!/usr/bin/env node
// G4 dual-era MCP server: a RECONSTRUCTION, committed as permanent test tooling (Epic K, K8 #131).
//
// NOT THE ORIGINAL SPIKE SERVER. The server that produced the G4 fixtures
// (`g4-server.mjs` in the operator's scratchpad) was throwaway spike code and was never
// committed (docs/planning/gates/G4-result.md "Fixtures captured": "`g4-server.mjs` (the spike
// server) is **not committed**"). This file is rebuilt from G4-result.md's architecture
// description and from the wire shapes in the committed fixture
// docs/planning/gates/fixtures/g4-mcp-dual-era/transcript-2026-09-26.jsonl. It cannot be
// verified byte-identical, or behavior-identical, to the server that actually ran. Committing
// it is a deliberate, documented exception to `oac-gates`' "nothing durable is built on spike
// code" rule (issue #131): the herdr scenario needs a server to replay the gate, and the
// original does not exist in this repository. Nothing in adapters/, core/, cli/ or transports/
// may use it; it is test tooling for tools/herdr/scenarios/g4-mcp-dual-era.mjs only.
//
// Architecture, as G4-result.md states it: one process, two surfaces.
//   - stdio: a legacy MCP channel server (answers `initialize` with protocolVersion
//     `2025-11-25` and capabilities.experimental["claude/channel"]; a pre-initialize
//     `server/discover` probe gets -32601). With G4_STDIO_MODERN=1 it instead answers as a
//     modern-only (`2026-07-28`) server that still declares the channel capability: the
//     negative case.
//   - HTTP at 127.0.0.1:<G4_HTTP_PORT>/mcp, speaking both eras: a request whose body carries
//     `_meta["io.modelcontextprotocol/protocolVersion"]` is modern (stateless); `initialize`
//     and requests carrying an `Mcp-Session-Id` are legacy. GET and DELETE are 405, any other
//     path 404.
//   Tools on every surface: `g4_echo(text)` and `g4_relay_to_claude(text)`; every tools/call
//   result carries OAC `_meta` provenance under the public extension identifier. The relay
//   pushes `notifications/claude/channel` on THIS process's stdio (meta.relay_from).
//   `touch wake.trigger` (beside this file) makes every running copy push one wake
//   notification on its stdio. Every copy appends its protocol JSON to transcript.jsonl beside
//   this file, one line per frame: { t, pid, surface, era, direction, payload, ... }.
//
// Environment: G4_STDIO_MODERN=1 (negative-case copy), G4_HTTP_PORT (default 17448).
// Node built-ins only. It reads and writes nothing but the two files beside it.

import { appendFileSync, existsSync, realpathSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomUUID } from 'node:crypto';
import { createInterface } from 'node:readline';

const HERE = dirname(fileURLToPath(import.meta.url));
const TRANSCRIPT = join(HERE, 'transcript.jsonl');
const TRIGGER = join(HERE, 'wake.trigger');
const STDIO_MODERN = process.env.G4_STDIO_MODERN === '1';
const PORT = Number(process.env.G4_HTTP_PORT || 17448);
const PID = process.pid;

export const MODERN = '2026-07-28';
export const LEGACY = '2025-11-25';
export const PV_KEY = 'io.modelcontextprotocol/protocolVersion';
export const OAC_EXT = 'io.github.rossgraeber/oac-session-channels';
const SERVER_INFO = { name: 'g4-spike', version: '0.0.1-throwaway' };
const TOOLS = [
  {
    name: 'g4_echo',
    description: 'G4 spike: echo text back. The result carries OAC provenance in _meta.',
    inputSchema: { type: 'object', properties: { text: { type: 'string' } }, required: ['text'] },
  },
  {
    name: 'g4_relay_to_claude',
    description: 'G4 spike: relay text to the live Claude Code session as a channel message, from this same server process.',
    inputSchema: { type: 'object', properties: { text: { type: 'string' } }, required: ['text'] },
  },
];
const CACHE = { ttlMs: 60000, cacheScope: 'public' };

const log = (surface, era, direction, payload, extra = {}) => {
  try {
    appendFileSync(TRANSCRIPT, `${JSON.stringify({ t: new Date().toISOString(), pid: PID, surface, era, direction, ...extra, payload })}\n`);
  } catch {
    /* the transcript is evidence, never a reason to crash the server */
  }
};

// --- stdio surface ---------------------------------------------------------------------------

let counter = 0;
const nextMessageId = () => `g4-${++counter}`;
let stdioInitialized = false;
let armed = false;

const stdioSend = (era, msg) => {
  process.stdout.write(`${JSON.stringify(msg)}\n`);
  log('stdio', era, 'server->client', msg);
};

function pushChannel(era, content, meta) {
  stdioSend(era, { jsonrpc: '2.0', method: 'notifications/claude/channel', params: { content, meta: { oac_message_id: nextMessageId(), oac_sender: 'g4-spike', ...meta } } });
}

function arm(era) {
  if (armed) return;
  armed = true;
  log('stdio', era, 'spike', { note: 'armed: touch wake.trigger to push a channel notification' });
}

function toolResult(name, args, surface, era) {
  const text = String(args?.text ?? '');
  const meta = { _meta: { [OAC_EXT]: { spike: 'G4', served_by_pid: PID, surface, message_id: randomUUID() } } };
  const complete = era === 'modern' ? { resultType: 'complete' } : {};
  if (name === 'g4_echo') return { ...complete, content: [{ type: 'text', text: `g4 echo: ${text}` }], ...meta };
  if (name === 'g4_relay_to_claude') {
    const from = surface.replace('-', '_');
    pushChannel(STDIO_MODERN ? 'modern' : 'legacy', `relayed from ${surface}: ${text}`, { relay_from: from });
    return { ...complete, content: [{ type: 'text', text: 'relayed to Claude channel (written to stdio transport; no ack exists)' }], ...meta };
  }
  return null;
}

function handleStdio(msg) {
  const pv = msg?.params?._meta?.[PV_KEY];
  const inEra = pv ? 'modern' : STDIO_MODERN ? 'modern' : 'legacy';
  log('stdio', inEra, 'client->server', msg);
  if (msg.id === undefined) {
    if (msg.method === 'notifications/initialized' && !STDIO_MODERN) {
      stdioInitialized = true;
      arm('legacy');
    }
    return;
  }
  const reply = (era, result) => stdioSend(era, { jsonrpc: '2.0', id: msg.id, result });
  const error = (era, code, message) => stdioSend(era, { jsonrpc: '2.0', id: msg.id, error: { code, message } });
  if (STDIO_MODERN) {
    if (msg.method === 'server/discover') {
      reply('modern', { resultType: 'complete', supportedVersions: [MODERN], capabilities: { tools: {}, experimental: { 'claude/channel': {} }, extensions: { [OAC_EXT]: {} } }, _meta: { 'io.modelcontextprotocol/serverInfo': SERVER_INFO }, ...CACHE });
      return arm('modern');
    }
    if (msg.method === 'initialize') return error('modern', -32602, `unsupported protocol version; this server supports only ${MODERN}`);
    if (msg.method === 'tools/list') return reply('modern', { resultType: 'complete', tools: TOOLS, ...CACHE });
    if (msg.method === 'tools/call') {
      const r = toolResult(msg.params?.name, msg.params?.arguments, 'stdio-modern', 'modern');
      return r ? reply('modern', r) : error('modern', -32602, `unknown tool ${msg.params?.name}`);
    }
    return error('modern', -32601, 'Method not found');
  }
  // Legacy stdio surface.
  if (msg.method === 'server/discover' || (pv && !stdioInitialized && msg.method !== 'initialize')) return error(null, -32601, 'Method not found (legacy stdio surface)');
  if (msg.method === 'initialize') {
    return reply('legacy', {
      protocolVersion: LEGACY,
      capabilities: { experimental: { 'claude/channel': {} }, tools: {} },
      serverInfo: SERVER_INFO,
      instructions: 'G4 spike channel. Messages arrive as <channel source="g4spike" ...>. No reply expected.',
    });
  }
  if (msg.method === 'tools/list') return reply('legacy', { tools: TOOLS });
  if (msg.method === 'tools/call') {
    const r = toolResult(msg.params?.name, msg.params?.arguments, 'stdio-legacy', 'legacy');
    return r ? reply('legacy', r) : error('legacy', -32602, `unknown tool ${msg.params?.name}`);
  }
  return error('legacy', -32601, 'Method not found');
}

// --- HTTP surface ----------------------------------------------------------------------------

const sessions = new Set();

function handleHttp(req, res) {
  const headers = { ...req.headers };
  delete headers.host;
  delete headers.connection;
  delete headers['content-length'];
  const url = req.url ?? '/';
  const send = (status, body, extraHeaders = {}) => {
    res.writeHead(status, { 'content-type': 'application/json', ...extraHeaders });
    res.end(body === null ? '' : JSON.stringify(body));
  };
  if (url !== '/mcp') {
    log('http', null, 'client->server', { method: req.method, url, headers }, { note: `${req.method} ${url} -> 404` });
    return send(404, null);
  }
  if (req.method !== 'POST') {
    log('http', headers['mcp-session-id'] ? 'legacy?' : null, 'client->server', { method: req.method, url, headers }, { note: `${req.method} -> 405` });
    return send(405, null);
  }
  let raw = '';
  req.on('data', (d) => (raw += d));
  req.on('end', () => {
    let body;
    try {
      body = JSON.parse(raw);
    } catch {
      log('http', null, 'client->server', { method: 'POST', url, headers, unparsed: raw.slice(0, 2000) }, { note: 'unparseable body -> 400' });
      return send(400, { jsonrpc: '2.0', id: null, error: { code: -32700, message: 'Parse error' } });
    }
    const modern = !!body?.params?._meta?.[PV_KEY];
    const era = modern ? 'modern' : 'legacy';
    log('http', era, 'client->server', { method: 'POST', url, headers, body });
    const reply = (result, extra = {}, statusHeaders = null) => {
      const payload = { jsonrpc: '2.0', id: body.id, result };
      log('http', era, 'server->client', payload, statusHeaders ? { status: 200, headers: statusHeaders } : extra);
      send(200, payload, statusHeaders ?? {});
    };
    const error = (code, message, status = 200) => {
      const payload = { jsonrpc: '2.0', id: body.id ?? null, error: { code, message } };
      log('http', era, 'server->client', payload, { status });
      send(status, payload);
    };
    if (modern) {
      if (body.method === 'server/discover') {
        return reply({ resultType: 'complete', supportedVersions: [MODERN, LEGACY], capabilities: { tools: {}, extensions: { [OAC_EXT]: {} } }, _meta: { 'io.modelcontextprotocol/serverInfo': SERVER_INFO }, instructions: 'G4 spike server. Tools: g4_echo, g4_relay_to_claude.', ...CACHE });
      }
      if (body.method === 'tools/list') return reply({ resultType: 'complete', tools: TOOLS, ...CACHE });
      if (body.method === 'tools/call') {
        const r = toolResult(body.params?.name, body.params?.arguments, 'http-modern', 'modern');
        return r ? reply(r) : error(-32602, `unknown tool ${body.params?.name}`);
      }
      return error(-32601, 'Method not found');
    }
    if (body.method === 'initialize') {
      const sid = randomUUID();
      sessions.add(sid);
      return reply({ protocolVersion: LEGACY, capabilities: { tools: {}, extensions: { [OAC_EXT]: {} } }, serverInfo: SERVER_INFO }, {}, { 'Mcp-Session-Id': sid });
    }
    const sid = headers['mcp-session-id'];
    if (!sid) return error(-32600, 'legacy request without Mcp-Session-Id (and no modern protocolVersion _meta)', 400);
    if (!sessions.has(sid)) return error(-32600, 'unknown Mcp-Session-Id', 404);
    if (body.id === undefined) {
      log('http', 'legacy', 'server->client', { note: `notification ${body.method} -> 202` });
      res.writeHead(202);
      return res.end();
    }
    if (body.method === 'tools/list') return reply({ tools: TOOLS });
    if (body.method === 'tools/call') {
      const r = toolResult(body.params?.name, body.params?.arguments, 'http-legacy', 'legacy');
      return r ? reply(r) : error(-32602, `unknown tool ${body.params?.name}`);
    }
    return error(-32601, 'Method not found');
  });
}

// --- main ------------------------------------------------------------------------------------

function main() {
  log('stdio', null, 'spike', { note: 'g4 server started', stdio_modern: STDIO_MODERN });
  const http = createServer(handleHttp);
  http.on('error', (err) => log('http', null, 'listen-error', String(err)));
  http.listen(PORT, '127.0.0.1', () => log('http', null, 'spike', { note: `listening http://127.0.0.1:${PORT}/mcp`, stdio_modern: STDIO_MODERN }));

  createInterface({ input: process.stdin }).on('line', (line) => {
    if (!line.trim()) return;
    let msg;
    try {
      msg = JSON.parse(line);
    } catch {
      log('stdio', null, 'client->server', { unparsed: line.slice(0, 2000) });
      return;
    }
    handleStdio(msg);
  });
  process.stdin.on('end', () => {
    log('stdio', null, 'spike', { note: 'stdin closed; exiting' });
    http.close();
    process.exit(0);
  });

  // wake.trigger: every change of its mtime pushes one wake notification, once armed.
  let seen = existsSync(TRIGGER) ? statSync(TRIGGER).mtimeMs : null;
  setInterval(() => {
    if (!existsSync(TRIGGER)) return;
    const m = statSync(TRIGGER).mtimeMs;
    if (m === seen) return;
    seen = m;
    if (!armed) return;
    const era = STDIO_MODERN ? 'modern' : 'legacy';
    const content = STDIO_MODERN
      ? `G4 wake test from the MODERN (${MODERN}, should NOT be a channel) stdio surface, pid ${PID}.`
      : `G4 wake test from the LEGACY (${LEGACY} channel) stdio surface, pid ${PID}.`;
    pushChannel(era, content, { g4_stdio_era: era });
  }, 100);
}

const isMain = () => {
  try {
    return !!process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
};
if (isMain()) main();
