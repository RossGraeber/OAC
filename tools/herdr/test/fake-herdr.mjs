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
//   linger-working=<ms>  (#246) an agent reports `working` for <ms> after its harness double
//                        went idle (the TUI still drawing after the wire turn completed), so
//                        an `agent read --lines` straight after the wire says done is refused
//   wait-no-status       (#253) `agent wait` answers with no agent_status, a response the
//                        driver cannot read a state from
//
// Agent responses carry herdr's AgentInfo shape (#253): the state is `agent_status` (there is
// no `state` field) with `state_change_seq`, the count of state changes the harness double
// recorded in its state-seq file. `agent read --lines N` of recent/recent-unwrapped history is
// refused with agent_not_idle while the agent is working, blocked or unknown (herdr
// agent-automation.mdx at v0.9.1).

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
// The state herdr would report, and its state_change_seq (#253, #246).
const agentStatus = (a) => {
  // The harness doubles' atomic snapshot (state, seq, time; written by rename), so a state is
  // never paired with another state's seq; without one, the separate files.
  const snap = a?.dir && existsSync(join(a.dir, 'status')) ? readFileSync(join(a.dir, 'status'), 'utf8').trim().split(' ') : null;
  const st = snap?.length === 3 ? snap[0] : agentState(a);
  const f = a?.dir ? join(a.dir, 'state-seq') : null;
  const [seq, at] = snap?.length === 3 ? [Number(snap[1]), Number(snap[2])] : f && existsSync(f) ? readFileSync(f, 'utf8').trim().split(' ').map(Number) : [0, 0];
  const linger = Number(modeVal('linger-working') ?? 0);
  if (linger > 0 && ['idle', 'done'].includes(st) && Date.now() - at < linger) return { state: 'working', seq: Math.max(0, seq - 1) };
  return { state: st, seq };
};
// The transitions herdr's event stream would have carried after state_change_seq `after`
// (the harness doubles log each one to state-log), so a wait catches a short-lived state as
// herdr's event-driven waits do. A transition still hidden by linger-working is not seen yet.
const transitionsAfter = (a, after, cur = agentStatus(a)) => {
  const f = a?.dir ? join(a.dir, 'state-log') : null;
  if (!f || !existsSync(f)) return [];
  return readFileSync(f, 'utf8')
    .split('\n')
    .filter(Boolean)
    .map((l) => ({ seq: Number(l.split(' ')[0]), state: l.split(' ')[1] }))
    .filter((t) => t.seq > after && t.seq <= cur.seq);
};
const agentInfo = (name, a, s = agentStatus(a)) => {
  return { name, terminal_id: `term_${name}`, pane_id: a?.pane ?? null, agent_status: s.state, state_change_seq: s.seq };
};

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
      const gone = []; // K8: groups and pids killed here, awaited below
      const agentsDir = join(sdir, 'agents');
      for (const f of existsSync(agentsDir) ? readdirSync(agentsDir).filter((x) => x.endsWith('.json')) : []) {
        const a = JSON.parse(readFileSync(join(agentsDir, f), 'utf8'));
        if (!a.pid) continue;
        try {
          process.kill(-a.pid, 'SIGKILL'); // the fake Claude and the channel server it started
          gone.push(-a.pid);
        } catch {
          /* gone */
        }
      }
      for (const f of readdirSync(join(sdir, 'panes')).filter((x) => x.endsWith('.json'))) {
        try {
          const pid = JSON.parse(readFileSync(join(sdir, 'panes', f), 'utf8')).pid;
          process.kill(pid, 'SIGKILL');
          gone.push(pid);
        } catch {
          /* gone */
        }
      }
      // Like a server that waits for its panes to exit before reporting stopped (K8: a loaded
      // self-test machine could otherwise see a SIGKILLed process still alive for a few ms).
      const stillThere = () => gone.filter((p) => { try { process.kill(p, 0); return true; } catch { return false; } });
      for (let t = 0; t < 80 && stillThere().length; t++) sleepSync(25);
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
  // herdr v0.9.1: "--timeout <MS> Wait for interactive readiness (default: 30000; max: 300000)".
  if (opt('--timeout') && !(Number(opt('--timeout')) <= 300000)) fail('invalid_agent_timeout', `--timeout must be at most 300000 ms`);
  const p = pane(opt('--pane'));
  const sep = args.indexOf('--');
  const rest = sep === -1 ? [] : args.slice(sep + 1);
  mkdirSync(join(sdir, 'agents'), { recursive: true });
  if (MODES.has('fake-claude') && opt('--kind') === 'claude') {
    const adir = join(sdir, 'agents', `${c2}.d`);
    mkdirSync(adir, { recursive: true });
    const child = spawn(process.execPath, [join(dirname(process.argv[1]), 'fake-claude.mjs'), adir, bufFile(p.id), ...rest], { cwd: p.cwd, env: p.env, detached: true, stdio: 'ignore' });
    child.unref();
    writeFileSync(join(sdir, 'agents', `${c2}.json`), JSON.stringify({ pane: p.id, pid: child.pid, dir: adir, argv: ['claude', ...rest] }));
    const settled = () => existsSync(join(adir, 'state')) && ['blocked', 'idle'].includes(agentState({ dir: adir }));
    for (let t = 0; t < 100 && !settled(); t++) sleepSync(50);
    if (agentState({ dir: adir }) === 'blocked') fail('agent_not_ready', `agent ${c2} is blocked during startup`);
    out({ type: 'agent_started', agent: agentInfo(c2, { dir: adir, pane: p.id }), argv: ['claude', ...rest] });
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
    out({ type: 'agent_started', agent: agentInfo(c2, { dir: adir, pane: p.id }), argv: ['codex', ...rest] });
  }
  writeFileSync(join(sdir, 'agents', `${c2}.json`), JSON.stringify({ pane: p.id }));
  append(p.id, `[agent ${opt('--kind')} started]\n`);
  out({ type: 'agent_started', agent: agentInfo(c2, { pane: p.id }), argv: [opt('--kind'), ...rest] });
} else if (c0 === 'agent' && c1 === 'read') {
  needsServer();
  if (MODES.has('hang-agent-read')) hang();
  else if (MODES.has('fail-agent-read')) fail('agent_not_found', `no agent ${c2}`);
  else {
    const a = agentRec(c2);
    const screen = a?.dir ? join(a.dir, 'screen.txt') : null;
    const src = opt('--source') ?? 'visible';
    const st = agentStatus(a).state;
    if (a?.dir && opt('--lines') && ['recent', 'recent-unwrapped'].includes(src) && ['working', 'blocked', 'unknown'].includes(st)) {
      fail('agent_not_idle', `cannot read ${opt('--lines')} lines while ${c2} is ${st}: its alternate-screen history can only be captured by scrolling while idle. Wait and retry, or use --source visible`);
    }
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
  // The agent as it was when the prompt was submitted (#253): taken before the harness double
  // can pick the prompt up, so a fast fake turn cannot already be inside the baseline.
  const before = agentInfo(c2, pa);
  if (pa?.dir) appendFileSync(join(pa.dir, 'inbox.log'), `${JSON.stringify({ text: args[3] })}\n`);
  if (args.includes('--wait') && MODES.has('prompt-timeout')) {
    sleepSync(Number(opt('--timeout') ?? 1000));
    fail('timeout', 'timed out waiting for the agent');
  }
  if (args.includes('--wait')) {
    // herdr v0.9.1 `agent prompt --wait` (src/api/wait.rs prompt_agent): unless the agent was
    // already working, an observed working/blocked state with state_change_seq past the
    // queued state's within 5000 ms (else agent_prompt_stalled, or `timeout` when the caller's
    // timeout is the shorter), then the first requested settled state.
    const until = optAll('--until');
    const settled = until.length ? until : ['idle', 'done', 'blocked'];
    const timeout = opt('--timeout') ? Number(opt('--timeout')) : Infinity;
    const start = Date.now();
    let active = before.agent_status === 'working';
    for (;;) {
      const s = agentStatus(pa);
      const elapsed = Date.now() - start;
      if (!active) {
        if (['working', 'blocked'].includes(s.state) && s.seq > before.state_change_seq) active = true;
        else if (transitionsAfter(pa, before.state_change_seq, s).some((t) => ['working', 'blocked'].includes(t.state))) active = true;
        else if (elapsed >= Math.min(5000, timeout)) fail(timeout <= 5000 ? 'timeout' : 'agent_prompt_stalled', `no working or blocked state observed for ${c2} after the prompt`);
      }
      if (active && settled.includes(s.state)) out({ type: 'agent_prompted', agent: agentInfo(c2, pa, s) });
      if (elapsed >= timeout) fail('timeout', `timed out after ${timeout}ms waiting for ${settled.join('|')}`);
      sleepSync(20);
    }
  }
  out({ type: 'agent_prompted', agent: before });
} else if (c0 === 'agent' && c1 === 'wait') {
  needsServer();
  const a = agentRec(c2);
  const until = optAll('--until');
  const timeout = opt('--timeout') ? Number(opt('--timeout')) : Infinity;
  const start = Date.now();
  const wanted = until.length ? until : ['idle', 'done', 'blocked']; // herdr: "Without --until, matches idle, done, or blocked."
  const startSeq = agentStatus(a).seq;
  for (;;) {
    const snap = agentStatus(a);
    const st = snap.state;
    // herdr (src/api/wait.rs wait_for_agent): the current state first, then any state the event
    // stream carried after the request (accept_transient_status), reported with that state.
    const transient = wanted.includes(st) ? null : transitionsAfter(a, startSeq, snap).find((t) => wanted.includes(t.state));
    if (wanted.includes(st) || transient) {
      if (MODES.has('wait-no-status')) out({ type: 'agent_info', agent: { name: c2, terminal_id: `term_${c2}` } });
      out({ type: 'agent_info', agent: { ...agentInfo(c2, a, snap), ...(transient ? { agent_status: transient.state } : {}) } });
    }
    if (Date.now() - start >= timeout) fail('timeout', `timed out after ${timeout}ms waiting for ${until.join('|')}`);
    sleepSync(50);
  }
} else if (c0 === 'agent' && (c1 === 'get' || c1 === 'explain')) {
  needsServer();
  out({ type: 'agent_info', agent: agentInfo(c2, agentRec(c2)) });
} else {
  fail('unknown_command', `fake-herdr does not implement: ${args.join(' ')}`, 2);
}
