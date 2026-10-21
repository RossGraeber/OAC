#!/usr/bin/env node
// G8 stub MCP server (G8 slice S1, #69 with #403): test tooling for
// tools/herdr/scenarios/g8-codex-tools.mjs. It is not OAC product code and nothing in
// adapters/, core/, cli/ or transports/ may use it.
//
// What it is for: Codex starts it as a stdio MCP server (the operator registers it with
// `codex mcp add <name> -- node <this file> --log <abs path>`), a model calls its one tool, and
// the scenario compares the server's own timestamps with the app-server's `mcpToolCall`
// item frames. It imitates the shape of spec/bindings/mcp.md §4.5 (the issued-value pairing)
// and nothing more: it binds nothing, sends no envelope and calls no model API.
//
// One tool, `g8_pair`, with two optional string members, `pairing` and `note`. Nothing is
// required, so a call with no `arguments`, with `{}` and with a value are all valid.
//   - The FIRST tools/call on a connection is refused with an error result (`isError: true`)
//     whose text carries a random pairing value V (128 bits, hex), whatever its arguments.
//   - Every LATER call succeeds. It says whether its `pairing` argument was V.
//
// Everything it sees is appended, one JSON object per line, to the file named by `--log`
// (required: it never writes beside itself, so the repository stays clean):
//   { t, pid, ppid, seq, event, ... }
//   start       the process id, its parent id, and (a moment later, event `tree`) the chain of
//               its ancestors as { pid, ppid, image } rows. Image is the executable's file
//               name only; no command line, path or user name is read or recorded.
//   rpc-in/out  each JSON-RPC frame the client sent and the server answered (id, method,
//               params or result)
//   call-in     a tools/call arrived: callSeq, hasArguments, argumentKeys, `_meta` member names
//   call-out    the result went back: callSeq, refused, the full result
// Timestamps are ISO-8601 UTC with milliseconds, from this host's clock, the same clock the
// scenario's capture client stamps its transcript with.
//
// Protocol: the legacy newline-delimited JSON-RPC stdio transport (what Codex speaks to a
// stdio server). It answers `initialize` with the client's protocolVersion when it is one
// of the revisions listed below, else the newest listed; `tools/list`; `tools/call`; `ping`.
//
// Node built-ins only.

import { spawn } from 'node:child_process';
import { appendFileSync, readFileSync, readdirSync, realpathSync } from 'node:fs';
import { basename } from 'node:path';
import { createInterface } from 'node:readline';
import { randomBytes } from 'node:crypto';
import { fileURLToPath } from 'node:url';

export const PROTOCOL_VERSIONS = Object.freeze(['2025-11-25', '2025-06-18', '2025-03-26', '2024-11-05']);
export const TOOL_NAME = 'g8_pair';
export const TOOL = Object.freeze({
  name: TOOL_NAME,
  description: 'G8 capture stub. The first call is refused with a pairing value; call again to be served. Takes optional `pairing` (the value from the refusal) and optional `note`.',
  inputSchema: { type: 'object', properties: { pairing: { type: 'string' }, note: { type: 'string' } } },
});
export const SERVER_INFO = Object.freeze({ name: 'g8-capture-stub', version: '0.0.1' });

// --- pure pieces (tested by tools/herdr/test/g8-tests.mjs) ---------------------------------------

export const refusalText = (value) => `unauthorized: this connection is not paired. Pairing value: ${value}. Call this tool again, passing that value as the "pairing" argument.`;

// -> { result, refused } for call number `callSeq` (1-based); `state.value` is the issued value.
export function serveCall(state, callSeq, params) {
  const args = params?.arguments;
  if (callSeq === 1) {
    return { refused: true, result: { content: [{ type: 'text', text: refusalText(state.value) }], isError: true } };
  }
  const given = args && typeof args === 'object' ? args.pairing : undefined;
  const paired = typeof given === 'string' && given === state.value;
  return { refused: false, paired, result: { content: [{ type: 'text', text: paired ? 'paired: the pairing value matched.' : 'served: no matching pairing value was given.' }] } };
}

export function negotiate(clientVersion) {
  return PROTOCOL_VERSIONS.includes(clientVersion) ? clientVersion : PROTOCOL_VERSIONS[0];
}

// pid -> { pid, ppid, image } rows from the OS, image = executable file name only.
export function parseTable(text) {
  const rows = new Map();
  for (const line of String(text).split(/\r?\n/)) {
    const m = /^\s*(\d+)\s*[, ]\s*(\d+)\s*[, ]\s*(.+?)\s*$/.exec(line);
    if (!m) continue;
    const image = basename(m[3].replace(/\\/g, '/'));
    rows.set(Number(m[1]), { pid: Number(m[1]), ppid: Number(m[2]), image });
  }
  return rows;
}

// Ancestors of pid, nearest first, ending at the root or at a loop / unknown parent.
export function ancestorChain(table, pid) {
  const out = [];
  const seen = new Set();
  let cur = table.get(pid);
  while (cur && !seen.has(cur.pid)) {
    seen.add(cur.pid);
    out.push({ pid: cur.pid, ppid: cur.ppid, image: cur.image });
    cur = table.get(cur.ppid);
  }
  return out;
}

function readTable() {
  return new Promise((resolve) => {
    let cmd;
    let args;
    if (process.platform === 'win32') {
      cmd = 'powershell.exe';
      args = ['-NoProfile', '-NonInteractive', '-Command', 'Get-CimInstance Win32_Process | ForEach-Object { "$($_.ProcessId),$($_.ParentProcessId),$($_.Name)" }'];
    } else if (process.platform === 'linux') {
      const rows = [];
      try {
        for (const pidText of readdirSync('/proc').filter((e) => /^[0-9]+$/.test(e))) {
          const stat = readFileSync(`/proc/${pidText}/stat`, 'utf8');
          const open = stat.indexOf('(');
          const close = stat.lastIndexOf(')');
          rows.push(`${pidText},${stat.slice(close + 2).split(' ')[1]},${stat.slice(open + 1, close)}`);
        }
      } catch {
        /* partial table is still a table */
      }
      resolve(parseTable(rows.join('\n')));
      return;
    } else {
      cmd = 'ps';
      args = ['-axo', 'pid=,ppid=,comm='];
    }
    let out = '';
    let child;
    try {
      child = spawn(cmd, args, { stdio: ['ignore', 'pipe', 'ignore'], windowsHide: true });
    } catch {
      resolve(null);
      return;
    }
    const timer = setTimeout(() => {
      child.kill();
      resolve(null);
    }, 20000);
    child.stdout.on('data', (d) => (out += d));
    child.on('error', () => {
      clearTimeout(timer);
      resolve(null);
    });
    child.on('close', () => {
      clearTimeout(timer);
      resolve(parseTable(out));
    });
  });
}

// --- the server --------------------------------------------------------------------------------

function main(argv) {
  const at = argv.indexOf('--log');
  const logPath = at >= 0 ? argv[at + 1] : null;
  if (!logPath) {
    process.stderr.write('g8-server: --log <absolute path> is required (it never writes beside itself)\n');
    process.exit(2);
  }
  const state = { value: randomBytes(16).toString('hex'), callSeq: 0, seq: 0 };
  const log = (event, extra = {}) => {
    try {
      appendFileSync(logPath, `${JSON.stringify({ t: new Date().toISOString(), pid: process.pid, ppid: process.ppid, seq: ++state.seq, event, ...extra })}\n`);
    } catch {
      /* the log is evidence, never a reason to fail a call */
    }
  };
  const send = (msg) => {
    process.stdout.write(`${JSON.stringify(msg)}\n`);
    log('rpc-out', { frame: msg });
  };
  log('start', { argvLength: argv.length, platform: process.platform, nodeVersion: process.version });
  readTable().then((table) => {
    log('tree', { available: !!table, ancestors: table ? ancestorChain(table, process.pid) : null });
  });

  const rl = createInterface({ input: process.stdin });
  rl.on('line', (line) => {
    if (!line.trim()) return;
    let msg;
    try {
      msg = JSON.parse(line);
    } catch {
      log('rpc-in-unparsed', { bytes: Buffer.byteLength(line) });
      return;
    }
    log('rpc-in', { frame: msg });
    if (msg.id === undefined) return; // a notification (notifications/initialized, ...)
    switch (msg.method) {
      case 'initialize':
        return send({ jsonrpc: '2.0', id: msg.id, result: { protocolVersion: negotiate(msg.params?.protocolVersion), capabilities: { tools: {} }, serverInfo: SERVER_INFO } });
      case 'ping':
        return send({ jsonrpc: '2.0', id: msg.id, result: {} });
      case 'tools/list':
        return send({ jsonrpc: '2.0', id: msg.id, result: { tools: [TOOL] } });
      case 'tools/call': {
        const callSeq = ++state.callSeq;
        const p = msg.params ?? {};
        const a = p.arguments;
        log('call-in', { callSeq, name: p.name, hasArguments: 'arguments' in p, argumentKeys: a && typeof a === 'object' ? Object.keys(a) : null, metaKeys: p._meta && typeof p._meta === 'object' ? Object.keys(p._meta) : null });
        if (p.name !== TOOL_NAME) return send({ jsonrpc: '2.0', id: msg.id, error: { code: -32602, message: `unknown tool ${JSON.stringify(p.name)}` } });
        const served = serveCall(state, callSeq, p);
        log('call-out', { callSeq, refused: served.refused, paired: served.paired ?? null, issuedValue: served.refused ? state.value : undefined, result: served.result });
        return send({ jsonrpc: '2.0', id: msg.id, result: served.result });
      }
      default:
        return send({ jsonrpc: '2.0', id: msg.id, error: { code: -32601, message: `method not found: ${msg.method}` } });
    }
  });
  rl.on('close', () => {
    log('stdin-closed');
    process.exit(0);
  });
}

const isMain = () => {
  try {
    return !!process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
};
if (isMain()) main(process.argv.slice(2));
