#!/usr/bin/env node
// Test double for the herdr v0.9.1 CLI, used only by tools/herdr/test/selftest.mjs so the
// driver's lifecycle, bounds, and manifest can be exercised with no herdr installed.
// It imitates the command shapes and JSON envelopes the driver relies on (herdr's
// cli-reference.mdx and src/api/schema/response.rs at tag v0.9.1); it is not herdr and
// proves nothing about herdr's own behavior.
//
// State lives under $FAKE_HERDR_STATE. $FAKE_HERDR_MODE is a comma list of behaviors:
//   version=<v>          print `herdr <v>` for --version (default 0.9.1)
//   server-version=<v>   version reported by `status server --json`
//   never-match          pane wait-output never matches (herdr times out itself)
//   hang-wait            pane wait-output ignores --timeout and never returns
//   hang-agent-read      agent read never returns
//   hang-send-keys       agent send-keys never returns
//   prompt-timeout       agent prompt --wait times out (herdr `timeout` error)
//   server-ignores-stop  server ignores `session stop`
//   leak-pane            server leaves its pane processes running on stop
//   pane-child-survives  each pane process starts a child that outlives it (a harness's
//                        own subprocess, not killed by session stop)
//   fail-agent-read      agent read fails with agent_not_found
//   fake-claude          `agent start --kind claude` runs test/fake-claude.mjs (a test double
//                        of an interactive Claude Code session, K4) in the pane's cwd; agent
//                        read/send-keys/prompt/wait then talk to it through files
//   fake-codex           `agent start --kind codex` runs the `codex` on the pane's PATH (the
//                        self-test's copy of test/fake-codex.mjs, K7) the same way

import { appendFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync, readdirSync } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';

const STATE = process.env.FAKE_HERDR_STATE;
if (!STATE) {
  console.error('fake-herdr: FAKE_HERDR_STATE is not set');
  process.exit(2);
}
mkdirSync(STATE, { recursive: true });
const MODES = new Set((process.env.FAKE_HERDR_MODE || '').split(',').filter(Boolean));
const modeVal = (k) => [...MODES].find((m) => m.startsWith(`${k}=`))?.slice(k.length + 1);

const raw = process.argv.slice(2);
appendFileSync(
  join(STATE, 'calls.log'),
  `${JSON.stringify({ argv: raw, configPath: process.env.HERDR_CONFIG_PATH ?? null, inheritedSocketPath: process.env.HERDR_SOCKET_PATH ?? null })}\n`,
);

let session = 'default';
const args = [];
for (let i = 0; i < raw.length; i++) {
  if (raw[i] === '--') {
    args.push(...raw.slice(i));
    break;
  }
  if (raw[i] === '--session') session = raw[++i];
  else args.push(raw[i]);
}

const sleepSync = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
const hang = () => setInterval(() => {}, 1 << 30);
const out = (result) => {
  process.stdout.write(`${JSON.stringify({ id: 'cli', result })}\n`);
  process.exit(0);
};
const fail = (code, message, exit = 1) => {
  process.stderr.write(`${JSON.stringify({ id: 'cli', error: { code, message } })}\n`);
  process.exit(exit);
};
const alive = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};
const opt = (name) => {
  const i = args.indexOf(name);
  return i === -1 ? undefined : args[i + 1];
};
const optAll = (name) => args.flatMap((a, i) => (a === name ? [args[i + 1]] : []));

const sdirOf = (name) => join(STATE, 'sessions', name);
const sdir = sdirOf(session);
const serverFile = (dir) => join(dir, 'server.json');
const running = (dir) => existsSync(serverFile(dir)) && alive(JSON.parse(readFileSync(serverFile(dir), 'utf8')).pid);
const paneFile = (id) => join(sdir, 'panes', `${id.replace(/:/g, '_')}.json`);
const bufFile = (id) => join(sdir, 'panes', `${id.replace(/:/g, '_')}.buf`);
const pane = (id) => {
  if (!existsSync(paneFile(id))) fail('pane_not_found', `no pane ${id}`);
  return JSON.parse(readFileSync(paneFile(id), 'utf8'));
};
const agentPane = (target) => {
  const f = join(sdir, 'agents', `${target}.json`);
  return existsSync(f) ? JSON.parse(readFileSync(f, 'utf8')).pane : target;
};
const append = (id, text) => appendFileSync(bufFile(id), text);
const agentRec = (target) => {
  const f = join(sdir, 'agents', `${target}.json`);
  return existsSync(f) ? JSON.parse(readFileSync(f, 'utf8')) : null;
};
const agentState = (a) => (a?.dir && existsSync(join(a.dir, 'state')) ? readFileSync(join(a.dir, 'state'), 'utf8').trim() : 'idle');

const [c0, c1, c2] = args;
const needsServer = () => {
  if (!running(sdir)) fail('server_not_running', `session ${session} is not running`);
};

if (c0 === '--version') {
  console.log(`herdr ${modeVal('version') ?? '0.9.1'}`);
  process.exit(0);
}

if (c0 === 'server' && c1 === undefined) {
  mkdirSync(join(sdir, 'panes'), { recursive: true });
  writeFileSync(serverFile(sdir), JSON.stringify({ pid: process.pid }));
  setInterval(() => {
    if (!existsSync(join(sdir, 'stop')) || MODES.has('server-ignores-stop')) return;
    if (!MODES.has('leak-pane')) {
      const agentsDir = join(sdir, 'agents');
      for (const f of existsSync(agentsDir) ? readdirSync(agentsDir).filter((x) => x.endsWith('.json')) : []) {
        const a = JSON.parse(readFileSync(join(agentsDir, f), 'utf8'));
        if (!a.pid) continue;
        try {
          process.kill(-a.pid, 'SIGKILL'); // the fake Claude and the channel server it started
        } catch {
          /* gone */
        }
      }
      for (const f of readdirSync(join(sdir, 'panes')).filter((x) => x.endsWith('.json'))) {
        try {
          process.kill(JSON.parse(readFileSync(join(sdir, 'panes', f), 'utf8')).pid, 'SIGKILL');
        } catch {
          /* gone */
        }
      }
    }
    rmSync(serverFile(sdir), { force: true });
    rmSync(join(sdir, 'stop'), { force: true });
    process.exit(0);
  }, 50);
} else if (c0 === 'status' && c1 === 'server') {
  const r = running(sdir);
  console.log(JSON.stringify(r ? { status: 'running', running: true, version: modeVal('server-version') ?? modeVal('version') ?? '0.9.1' } : { status: 'not running', running: false }));
  process.exit(0);
} else if (c0 === 'server' && c1 === 'agent-manifests') {
  needsServer();
  out({ type: 'agent_manifests', manifests: [{ agent: 'claude', source: 'bundled', version: '2026.09.11.1' }, { agent: 'codex', source: 'bundled', version: '2026.09.14.1' }] });
} else if (c0 === 'session' && c1 === 'stop') {
  const dir = sdirOf(c2);
  if (!running(dir)) fail('session_not_running', `session ${c2} is not running`);
  writeFileSync(join(dir, 'stop'), '');
  for (let t = 0; t < 60 && existsSync(serverFile(dir)); t++) sleepSync(50);
  if (existsSync(serverFile(dir))) fail('stop_timeout', `session ${c2} did not stop`);
  out({ type: 'session_stopped', name: c2 });
} else if (c0 === 'session' && c1 === 'delete') {
  const dir = sdirOf(c2);
  if (running(dir)) fail('session_running', `session ${c2} is running; stop it before deleting`);
  rmSync(dir, { recursive: true, force: true });
  out({ type: 'session_deleted', name: c2 });
} else if (c0 === 'workspace' && c1 === 'create') {
  needsServer();
  const n = readdirSync(join(sdir, 'panes')).filter((x) => x.endsWith('.json')).length + 1;
  const ids = { ws: `w${n}`, tab: `w${n}:t1`, pane: `w${n}:p1` };
  const env = { ...process.env };
  for (const k of ['CODEX_THREAD_ID', 'OMPCODE', 'WT_SESSION']) delete env[k];
  for (const kv of optAll('--env')) env[kv.slice(0, kv.indexOf('='))] = kv.slice(kv.indexOf('=') + 1);
  Object.assign(env, {
    HERDR_ENV: '1',
    HERDR_SOCKET_PATH: join(sdir, 'herdr.sock'),
    HERDR_BIN_PATH: process.argv[1],
    HERDR_WORKSPACE_ID: ids.ws,
    HERDR_TAB_ID: ids.tab,
    HERDR_PANE_ID: ids.pane,
    TERM: 'xterm-256color',
    COLORTERM: 'truecolor',
  });
  const shellCode = MODES.has('pane-child-survives')
    ? "require('child_process').spawn(process.execPath, ['-e', 'setInterval(() => {}, 1 << 30)'], { detached: true, stdio: 'ignore' }).unref(); setInterval(() => {}, 1 << 30)"
    : 'setInterval(() => {}, 1 << 30)';
  const shell = spawn(process.execPath, ['-e', shellCode], { detached: true, stdio: 'ignore', env });
  shell.unref();
  writeFileSync(paneFile(ids.pane), JSON.stringify({ id: ids.pane, cwd: opt('--cwd') ?? STATE, pid: shell.pid, env }));
  writeFileSync(bufFile(ids.pane), '$ \n');
  out({ type: 'workspace_created', workspace: { workspace_id: ids.ws, label: opt('--label') ?? null }, tab: { tab_id: ids.tab }, root_pane: { pane_id: ids.pane }, tabs: [] });
} else if (c0 === 'pane' && c1 === 'process-info') {
  needsServer();
  const p = pane(opt('--pane'));
  const agentsDir = join(sdir, 'agents');
  const fg = (existsSync(agentsDir) ? readdirSync(agentsDir) : [])
    .filter((x) => x.endsWith('.json'))
    .map((x) => JSON.parse(readFileSync(join(agentsDir, x), 'utf8')))
    .filter((a) => a.pane === p.id && a.pid)
    .map((a) => ({ pid: a.pid }));
  out({ type: 'pane_process_info', process_info: { pane_id: p.id, shell_pid: p.pid, foreground_processes: fg } });
} else if (c0 === 'pane' && c1 === 'run') {
  needsServer();
  const p = pane(c2);
  const cmd = args[3];
  append(p.id, `$ ${cmd}\n`);
  const r = spawnSync('sh', ['-c', cmd], { cwd: p.cwd, env: p.env, encoding: 'utf8', timeout: 30000 });
  append(p.id, `${r.stdout ?? ''}${r.stderr ?? ''}`);
  out({ type: 'ok' });
} else if (c0 === 'pane' && c1 === 'wait-output') {
  needsServer();
  if (MODES.has('hang-wait')) hang();
  else {
    const p = pane(c2);
    const re = opt('--regex') ? new RegExp(opt('--regex')) : null;
    const lit = opt('--match');
    const timeout = opt('--timeout') ? Number(opt('--timeout')) : Infinity;
    const start = Date.now();
    const tick = () => {
      const lines = readFileSync(bufFile(p.id), 'utf8').split('\n');
      if (!MODES.has('never-match') && lines.some((l) => (re ? re.test(l) : l.includes(lit)))) out({ type: 'pane_output_matched', pane_id: p.id });
      if (Date.now() - start >= timeout) fail('timeout', `timed out after ${timeout}ms waiting for output`);
      setTimeout(tick, 50);
    };
    tick();
  }
} else if (c0 === 'pane' && c1 === 'read') {
  needsServer();
  process.stdout.write(readFileSync(bufFile(pane(c2).id), 'utf8'));
  process.exit(0);
} else if (c0 === 'agent' && c1 === 'start') {
  needsServer();
  const p = pane(opt('--pane'));
  const sep = args.indexOf('--');
  const rest = sep === -1 ? [] : args.slice(sep + 1);
  mkdirSync(join(sdir, 'agents'), { recursive: true });
  if (MODES.has('fake-claude') && opt('--kind') === 'claude') {
    const adir = join(sdir, 'agents', `${c2}.d`);
    mkdirSync(adir, { recursive: true });
    const child = spawn(process.execPath, [join(dirname(process.argv[1]), 'fake-claude.mjs'), adir, bufFile(p.id)], { cwd: p.cwd, env: p.env, detached: true, stdio: 'ignore' });
    child.unref();
    writeFileSync(join(sdir, 'agents', `${c2}.json`), JSON.stringify({ pane: p.id, pid: child.pid, dir: adir, argv: ['claude', ...rest] }));
    const settled = () => existsSync(join(adir, 'state')) && ['blocked', 'idle'].includes(agentState({ dir: adir }));
    for (let t = 0; t < 100 && !settled(); t++) sleepSync(50);
    if (agentState({ dir: adir }) === 'blocked') fail('agent_not_ready', `agent ${c2} is blocked during startup`);
    out({ type: 'agent_started', agent: { name: c2, state: agentState({ dir: adir }) }, argv: ['claude', ...rest] });
  }
  if (MODES.has('fake-codex') && opt('--kind') === 'codex') {
    // The pane runs whatever `codex` its PATH resolves (the self-test's copy of
    // test/fake-codex.mjs), with exactly the args after `--`, as herdr runs its canonical
    // executable for the kind.
    const adir = join(sdir, 'agents', `${c2}.d`);
    mkdirSync(adir, { recursive: true });
    const child = spawn('codex', rest, { cwd: p.cwd, env: { ...p.env, FAKE_CODEX_AGENT_DIR: adir, FAKE_CODEX_PANE_BUF: bufFile(p.id) }, detached: true, stdio: 'ignore' });
    child.unref();
    writeFileSync(join(sdir, 'agents', `${c2}.json`), JSON.stringify({ pane: p.id, pid: child.pid, dir: adir, argv: ['codex', ...rest] }));
    const settled = () => existsSync(join(adir, 'state')) && ['blocked', 'idle'].includes(agentState({ dir: adir }));
    for (let t = 0; t < 100 && !settled(); t++) sleepSync(50);
    if (agentState({ dir: adir }) === 'blocked') fail('agent_not_ready', `agent ${c2} is blocked during startup`);
    out({ type: 'agent_started', agent: { name: c2, state: agentState({ dir: adir }) }, argv: ['codex', ...rest] });
  }
  writeFileSync(join(sdir, 'agents', `${c2}.json`), JSON.stringify({ pane: p.id }));
  append(p.id, `[agent ${opt('--kind')} started]\n`);
  out({ type: 'agent_started', agent: { name: c2, state: 'idle' }, argv: [opt('--kind'), ...rest] });
} else if (c0 === 'agent' && c1 === 'read') {
  needsServer();
  if (MODES.has('hang-agent-read')) hang();
  else if (MODES.has('fail-agent-read')) fail('agent_not_found', `no agent ${c2}`);
  else {
    const a = agentRec(c2);
    const screen = a?.dir ? join(a.dir, 'screen.txt') : null;
    const src = opt('--source') ?? 'visible';
    process.stdout.write(readFileSync(src === 'visible' && screen && existsSync(screen) ? screen : bufFile(agentPane(c2)), 'utf8'));
    process.exit(0);
  }
} else if (c0 === 'agent' && c1 === 'send-keys') {
  needsServer();
  if (MODES.has('hang-send-keys')) hang();
  else {
    const a = agentRec(c2);
    if (a?.dir) appendFileSync(join(a.dir, 'keys.log'), `${args.slice(3).join(' ')}\n`);
    else append(agentPane(c2), `[keys ${args.slice(3).join(' ')}]\n`);
    out({ type: 'ok' });
  }
} else if (c0 === 'agent' && c1 === 'prompt') {
  needsServer();
  appendFileSync(join(STATE, 'prompts.log'), `${JSON.stringify({ target: c2, text: args[3] })}\n`);
  const pa = agentRec(c2);
  if (pa?.dir) appendFileSync(join(pa.dir, 'inbox.log'), `${JSON.stringify({ text: args[3] })}\n`);
  if (args.includes('--wait') && MODES.has('prompt-timeout')) {
    sleepSync(Number(opt('--timeout') ?? 1000));
    fail('timeout', 'timed out waiting for the agent');
  }
  out({ type: 'agent_prompted', agent: { name: c2, state: 'working' } });
} else if (c0 === 'agent' && c1 === 'wait') {
  needsServer();
  const a = agentRec(c2);
  const until = optAll('--until');
  const timeout = opt('--timeout') ? Number(opt('--timeout')) : Infinity;
  const start = Date.now();
  for (;;) {
    const st = agentState(a);
    if (!until.length || until.includes(st)) out({ type: 'agent_info', agent: { name: c2, state: st } });
    if (Date.now() - start >= timeout) fail('timeout', `timed out after ${timeout}ms waiting for ${until.join('|')}`);
    sleepSync(50);
  }
} else if (c0 === 'agent' && (c1 === 'get' || c1 === 'explain')) {
  needsServer();
  out({ type: 'agent_info', agent: { name: c2, state: 'idle' } });
} else {
  fail('unknown_command', `fake-herdr does not implement: ${args.join(' ')}`, 2);
}
