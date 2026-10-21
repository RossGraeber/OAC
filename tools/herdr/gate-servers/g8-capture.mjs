#!/usr/bin/env node
// G8 capture client (G8 slice S1, #69 with #403): test tooling for
// tools/herdr/scenarios/g8-codex-tools.mjs. It is not OAC product code.
//
// A WebSocket client of `codex app-server proxy` (the shared daemon's control socket), in the
// pattern of gate-servers/g5-codex.mjs: an HTTP/1.1 Upgrade and RFC 6455 framing over the
// stdio of the proxy process. Two modes, one connection per process run; every frame is
// appended to transcript.jsonl beside this file as { t, mode, direction, payload }, the
// format lib/g2.mjs parseG2Transcript reads:
//
//   node g8-capture.mjs list                  initialize, thread/loaded/list, thread/list
//   node g8-capture.mjs watch <threadId>      initialize, then thread/resume with ONLY
//                                             { threadId, excludeTurns: true }, then stays
//                                             connected, recording the item/started and
//                                             item/completed frames the daemon pushes, until
//                                             it is killed or the connection closes
//
// `watch` is a passive subscriber. It never calls turn/start, thread/queue/add, turn/steer or
// any method that changes a thread; the thread/resume request carries no model, cwd, approval,
// sandbox or config member, so it changes no setting. It never answers a server-to-client
// request (the TUI user owns approvals).
//
// Credentials: none. It never reads anything under the Codex home directory; the daemon serves
// its own sign-in. Node built-ins only.

import { spawn } from 'node:child_process';
import { appendFileSync, realpathSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomBytes } from 'node:crypto';

const HERE = dirname(fileURLToPath(import.meta.url));
const TRANSCRIPT = join(HERE, 'transcript.jsonl');

// The exact thread/resume request: nothing but these two members.
export const resumeParams = (threadId) => ({ threadId, excludeTurns: true });

function connect(log) {
  const proc = spawn('codex', ['app-server', 'proxy'], { stdio: ['pipe', 'pipe', 'pipe'], shell: process.platform === 'win32', windowsHide: true });
  proc.stderr.on('data', (d) => log('proxy-stderr', d.toString()));
  const sendFrame = (opcode, data) => {
    const mask = randomBytes(4);
    let header;
    if (data.length < 126) header = Buffer.from([0x80 | opcode, 0x80 | data.length]);
    else if (data.length < 65536) {
      header = Buffer.alloc(4);
      header[0] = 0x80 | opcode;
      header[1] = 0x80 | 126;
      header.writeUInt16BE(data.length, 2);
    } else {
      header = Buffer.alloc(10);
      header[0] = 0x80 | opcode;
      header[1] = 0x80 | 127;
      header.writeBigUInt64BE(BigInt(data.length), 2);
    }
    const masked = Buffer.alloc(data.length);
    for (let i = 0; i < data.length; i++) masked[i] = data[i] ^ mask[i % 4];
    proc.stdin.write(Buffer.concat([header, mask, masked]));
  };
  let nextId = 0;
  const pending = new Map();
  const request = (method, params) => {
    const id = nextId++;
    const msg = { jsonrpc: '2.0', id, method, params };
    log('client->daemon', msg);
    sendFrame(0x1, Buffer.from(JSON.stringify(msg)));
    return new Promise((res) => pending.set(id, res));
  };
  const notify = (method, params) => {
    const msg = { jsonrpc: '2.0', method, params };
    log('client->daemon', msg);
    sendFrame(0x1, Buffer.from(JSON.stringify(msg)));
  };
  let onUp;
  const up = new Promise((r) => (onUp = r));
  let onClosed;
  const closed = new Promise((r) => (onClosed = r));
  let isUp = false;
  proc.on('exit', () => {
    // The proxy ending before the upgrade means no daemon answered on the control socket.
    if (!isUp) {
      console.error('[proxy exited before the WebSocket upgrade]');
      process.exit(3);
    }
    onClosed();
  });
  proc.stdin.write(`GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: ${randomBytes(16).toString('base64')}\r\nSec-WebSocket-Version: 13\r\n\r\n`);
  let raw = Buffer.alloc(0);
  let upgraded = false;
  let frags = [];
  proc.stdout.on('data', (chunk) => {
    raw = Buffer.concat([raw, chunk]);
    if (!upgraded) {
      const end = raw.indexOf('\r\n\r\n');
      if (end === -1) return;
      const head = raw.subarray(0, end).toString('utf8');
      log('handshake', head);
      raw = raw.subarray(end + 4);
      if (!/^HTTP\/1\.1 101/.test(head)) {
        console.error('[upgrade failed]');
        process.exit(3);
      }
      upgraded = true;
      isUp = true;
      onUp();
    }
    while (raw.length >= 2) {
      const fin = (raw[0] & 0x80) !== 0;
      const opcode = raw[0] & 0x0f;
      let len = raw[1] & 0x7f;
      let off = 2;
      if (len === 126) {
        if (raw.length < 4) return;
        len = raw.readUInt16BE(2);
        off = 4;
      } else if (len === 127) {
        if (raw.length < 10) return;
        len = Number(raw.readBigUInt64BE(2));
        off = 10;
      }
      if (raw.length < off + len) return;
      const data = raw.subarray(off, off + len);
      raw = raw.subarray(off + len);
      if (opcode === 0x9) {
        sendFrame(0xa, data);
        continue;
      }
      if (opcode === 0x8) {
        log('ws-close', data.toString('utf8'));
        continue;
      }
      if (opcode === 0xa) continue;
      frags.push(data);
      if (!fin) continue;
      const text = Buffer.concat(frags).toString('utf8');
      frags = [];
      let msg;
      try {
        msg = JSON.parse(text);
      } catch {
        log('unparsed', text);
        continue;
      }
      log('daemon->client', msg);
      if (msg.id !== undefined && pending.has(msg.id) && (msg.result !== undefined || msg.error !== undefined)) {
        pending.get(msg.id)(msg);
        pending.delete(msg.id);
      } else if (msg.id !== undefined && msg.method) {
        console.log(`[server-request ignored: ${msg.method}]`);
      }
    }
  });
  const close = (code = 0) => {
    try {
      proc.stdin.end();
    } catch {
      /* gone */
    }
    setTimeout(() => process.exit(code), 300);
  };
  return { up, closed, request, notify, close };
}

async function main() {
  const [, , mode, threadId] = process.argv;
  if (!['list', 'watch'].includes(mode) || (mode === 'watch' && !threadId)) {
    console.error('usage: g8-capture.mjs list | watch <threadId>');
    process.exit(2);
  }
  const log = (direction, payload) => appendFileSync(TRANSCRIPT, `${JSON.stringify({ t: new Date().toISOString(), mode, direction, payload })}\n`);
  const ws = connect(log);
  await ws.up;
  const init = await ws.request('initialize', { clientInfo: { name: 'oac_g8_capture', title: 'OAC G8 capture (test tooling)', version: '0.0.1' }, capabilities: { experimentalApi: true } });
  if (init.error) {
    console.log('[error] initialize');
    return ws.close(1);
  }
  ws.notify('initialized', {});
  if (mode === 'list') {
    await ws.request('thread/loaded/list', {});
    await ws.request('thread/list', { cursor: null, limit: 20, sortKey: 'created_at' });
    console.log('[done] list');
    return ws.close(0);
  }
  const r = await ws.request('thread/resume', resumeParams(threadId));
  if (r.error) {
    console.log('[error] thread/resume');
    return ws.close(1);
  }
  console.log('[done] resumed; watching');
  await ws.closed;
  console.log('[done] connection closed');
  process.exit(0);
}

const isMain = () => {
  try {
    return !!process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
};
if (isMain()) main();
