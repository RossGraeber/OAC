#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// The fake Codex app-server endpoint (#58, F9): a test double that replays the recorded
// Stage 1 Codex fixtures over stdio (JSONL) and a loopback WebSocket listener. Dev/test
// only: not a workspace member, not shipped, and no product path may depend on it
// (README "Containment"). No Codex install, no credential, no network beyond loopback.
//
//   node tests/fakes/codex-app-server/server.mjs --stdio
//   node tests/fakes/codex-app-server/server.mjs --listen ws://127.0.0.1:0
//   node tests/fakes/codex-app-server/server.mjs --stdio --listen ws://127.0.0.1:0
//
// With --listen, the first stderr line is one JSON object:
//   {"oacFakeCodex":"listening","url":"ws://127.0.0.1:<port>"}
// Every connection (stdio or WebSocket) shares one state, like clients of one daemon, so a
// test can drive the adapter over stdio and steer the fake (oacFake/* methods) over a
// WebSocket connection. With --stdio the process exits when stdin closes; otherwise on
// SIGINT/SIGTERM.

import { createInterface } from 'node:readline';
import { FakeCodexAppServer } from './lib/model.mjs';
import { MAX_UNFRAGMENTED, listen } from './lib/ws.mjs';

function usage(msg) {
  if (msg) process.stderr.write(`${msg}\n`);
  process.stderr.write('usage: server.mjs [--stdio] [--listen ws://127.0.0.1:<port>]  (at least one)\n');
  process.exit(2);
}

async function main(argv) {
  let stdio = false;
  let listenUrl = null;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--stdio') stdio = true;
    else if (argv[i] === '--listen') listenUrl = argv[++i] ?? usage('--listen needs a URL');
    else usage(`unknown argument ${argv[i]}`);
  }
  if (!stdio && !listenUrl) usage();

  const fake = new FakeCodexAppServer();
  let ws = null;
  if (listenUrl) {
    ws = await listen(listenUrl, (ch) => {
      const conn = fake.connect((obj) => ch.send(JSON.stringify(obj)), { transport: 'websocket' });
      ch.on('message', (text) => conn.receiveText(text));
      ch.on('tooLarge', (bytes) => conn.tooLarge(bytes));
      ch.on('close', () => conn.close());
    });
    process.stderr.write(`${JSON.stringify({ oacFakeCodex: 'listening', url: ws.url })}\n`);
  }
  const stop = async () => {
    if (ws) await ws.close();
    process.exit(0);
  };
  process.on('SIGINT', stop);
  process.on('SIGTERM', stop);

  if (stdio) {
    const conn = fake.connect((obj) => process.stdout.write(`${JSON.stringify(obj)}\n`), { transport: 'stdio' });
    const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });
    rl.on('line', (line) => {
      if (!line.trim()) return;
      if (Buffer.byteLength(line) > MAX_UNFRAGMENTED) {
        conn.tooLarge(Buffer.byteLength(line));
        return;
      }
      conn.receiveText(line);
    });
    rl.on('close', () => {
      conn.close();
      stop();
    });
  }
}

main(process.argv.slice(2)).catch((e) => {
  process.stderr.write(`${e?.stack ?? e}\n`);
  process.exit(1);
});
