// SPDX-License-Identifier: Apache-2.0
//
// A minimal RFC 6455 WebSocket server and client over node:http / node:net, for the fake
// Codex app-server (#58, F9). Text frames only: each JSON-RPC message is one text message,
// as in the recorded D6/G2/G5 traffic. Node built-ins only; loopback only.
//
// The server's 101 response carries the one extra header every recorded handshake shows
// (`x-codex-websocket-max-unfragmented-message-bytes: 16777216`, D6 `transcript-conn*`
// line 2). The real listener's capability-token / signed-bearer auth is not modelled
// (README "Not modelled").

import { createHash, randomBytes } from 'node:crypto';
import { createServer } from 'node:http';
import { connect as netConnect } from 'node:net';

const GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';
export const MAX_UNFRAGMENTED = 16777216;
const LOOPBACK = new Set(['127.0.0.1', '::1', 'localhost']);

export const acceptKey = (key) => createHash('sha1').update(key + GUID).digest('base64');

function encodeFrame(opcode, payload, mask) {
  const len = payload.length;
  let header;
  if (len < 126) {
    header = Buffer.alloc(2);
    header[1] = len;
  } else if (len < 65536) {
    header = Buffer.alloc(4);
    header[1] = 126;
    header.writeUInt16BE(len, 2);
  } else {
    header = Buffer.alloc(10);
    header[1] = 127;
    header.writeBigUInt64BE(BigInt(len), 2);
  }
  header[0] = 0x80 | opcode;
  if (!mask) return Buffer.concat([header, payload]);
  header[1] |= 0x80;
  const key = randomBytes(4);
  const body = Buffer.from(payload);
  for (let i = 0; i < body.length; i++) body[i] ^= key[i & 3];
  return Buffer.concat([header, key, body]);
}

// Wrap a raw duplex socket (after the handshake) as a message channel.
function channel(socket, { masked, head }) {
  const listeners = { message: [], close: [] };
  let buf = head && head.length ? Buffer.from(head) : Buffer.alloc(0);
  let fragments = null;
  let closed = false;
  const emit = (ev, arg) => listeners[ev].forEach((f) => f(arg));
  const finish = () => {
    if (closed) return;
    closed = true;
    emit('close');
  };
  const send = (opcode, data) => {
    if (closed || socket.destroyed) return;
    socket.write(encodeFrame(opcode, data, masked));
  };
  const parse = () => {
    for (;;) {
      if (buf.length < 2) return;
      const fin = (buf[0] & 0x80) !== 0;
      const opcode = buf[0] & 0x0f;
      const isMasked = (buf[1] & 0x80) !== 0;
      let len = buf[1] & 0x7f;
      let off = 2;
      if (len === 126) {
        if (buf.length < 4) return;
        len = buf.readUInt16BE(2);
        off = 4;
      } else if (len === 127) {
        if (buf.length < 10) return;
        len = Number(buf.readBigUInt64BE(2));
        off = 10;
      }
      const maskKey = isMasked ? buf.subarray(off, off + 4) : null;
      if (isMasked) off += 4;
      if (buf.length < off + len) return;
      const payload = Buffer.from(buf.subarray(off, off + len));
      buf = buf.subarray(off + len);
      if (maskKey) for (let i = 0; i < payload.length; i++) payload[i] ^= maskKey[i & 3];
      if (opcode === 0x8) {
        send(0x8, payload.subarray(0, 2));
        socket.end();
        finish();
        return;
      }
      if (opcode === 0x9) {
        send(0xa, payload);
        continue;
      }
      if (opcode === 0xa) continue;
      if (opcode === 0x1 || opcode === 0x0) {
        if (opcode === 0x1) fragments = [];
        if (!fragments) continue;
        fragments.push(payload);
        if (fin) {
          const text = Buffer.concat(fragments).toString('utf8');
          fragments = null;
          emit('message', text);
        }
        continue;
      }
      // Binary or unknown opcode: close with 1003 (unsupported data).
      const code = Buffer.alloc(2);
      code.writeUInt16BE(1003);
      send(0x8, code);
      socket.end();
      finish();
      return;
    }
  };
  socket.on('data', (d) => {
    buf = Buffer.concat([buf, d]);
    parse();
  });
  socket.on('close', finish);
  socket.on('error', finish);
  if (buf.length) setImmediate(parse);
  return {
    on(ev, f) {
      listeners[ev].push(f);
    },
    send(text) {
      send(0x1, Buffer.from(text, 'utf8'));
    },
    close() {
      if (closed) return;
      const code = Buffer.alloc(2);
      code.writeUInt16BE(1000);
      send(0x8, code);
      socket.end();
      finish();
    },
    get closed() {
      return closed;
    },
  };
}

// Parse `ws://host:port` and refuse anything but a loopback host.
export function parseLoopbackUrl(url) {
  const u = new URL(url);
  if (u.protocol !== 'ws:') throw new Error(`only ws:// is supported: ${url}`);
  const host = u.hostname.replace(/^\[|\]$/g, '');
  if (!LOOPBACK.has(host)) throw new Error(`refusing a non-loopback listen address: ${url}`);
  return { host, port: Number(u.port || 80) };
}

// Start a WebSocket server; `onConnection(ch)` gets each upgraded connection.
// Resolves to { url, close() }.
export function listen(url, onConnection) {
  const { host, port } = parseLoopbackUrl(url);
  const server = createServer((req, res) => {
    res.writeHead(426, { 'content-type': 'text/plain' });
    res.end('WebSocket upgrade required\n');
  });
  const sockets = new Set();
  server.on('upgrade', (req, socket, head) => {
    const key = req.headers['sec-websocket-key'];
    if (!key || String(req.headers.upgrade).toLowerCase() !== 'websocket') {
      socket.end('HTTP/1.1 400 Bad Request\r\n\r\n');
      return;
    }
    sockets.add(socket);
    socket.on('close', () => sockets.delete(socket));
    socket.write(
      'HTTP/1.1 101 Switching Protocols\r\n' +
        'connection: Upgrade\r\n' +
        'upgrade: websocket\r\n' +
        `sec-websocket-accept: ${acceptKey(key)}\r\n` +
        `x-codex-websocket-max-unfragmented-message-bytes: ${MAX_UNFRAGMENTED}\r\n\r\n`,
    );
    onConnection(channel(socket, { masked: false, head }));
  });
  return new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(port, host, () => {
      const a = server.address();
      const h = a.family === 'IPv6' ? `[${a.address}]` : a.address;
      resolve({
        url: `ws://${h}:${a.port}`,
        close: () =>
          new Promise((r) => {
            for (const s of sockets) s.destroy();
            server.close(() => r());
          }),
      });
    });
  });
}

// Client side, for tests: resolves to a channel once the 101 arrives.
export function connectWs(url) {
  const { host, port } = parseLoopbackUrl(url);
  return new Promise((resolve, reject) => {
    const socket = netConnect({ host, port });
    const key = randomBytes(16).toString('base64');
    let head = Buffer.alloc(0);
    const onData = (d) => {
      head = Buffer.concat([head, d]);
      const end = head.indexOf('\r\n\r\n');
      if (end < 0) return;
      socket.off('data', onData);
      const status = head.subarray(0, end).toString('latin1');
      const rest = head.subarray(end + 4);
      if (!/^HTTP\/1\.1 101 /.test(status) || !status.toLowerCase().includes(`sec-websocket-accept: ${acceptKey(key).toLowerCase()}`)) {
        socket.destroy();
        reject(new Error(`WebSocket handshake failed: ${status.split('\r\n')[0]}`));
        return;
      }
      const ch = channel(socket, { masked: true, head: rest });
      ch.handshake = status;
      resolve(ch);
    };
    socket.on('data', onData);
    socket.once('error', reject);
    socket.once('connect', () => {
      socket.write(
        `GET / HTTP/1.1\r\nHost: ${host}:${port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n` +
          `Sec-WebSocket-Key: ${key}\r\nSec-WebSocket-Version: 13\r\n\r\n`,
      );
    });
  });
}
