// HerdrSession: every herdr CLI call the driver makes goes through exec(), which
//   - gives it a role (operator-input, dialog-accept, wait, read, lifecycle, preflight),
//   - gives it a hard driver-side deadline, and for waits also herdr's own --timeout,
//   - records it (argv, role, bounds, exit, timing) for the run manifest,
//   - turns any timeout into NotRunError and halts all further input: after a timeout the
//     driver never sends another keystroke or prompt, so nothing is ever re-submitted.
//
// herdr runs as an external process only (child_process); it is never linked or imported.
// Command syntax is herdr's own CLI reference at the pinned tag (v0.9.1,
// docs/next/website/src/content/docs/cli-reference.mdx), cited in
// docs/planning/decisions/K1-herdr-evaluation.md §5.

import { openSync, closeSync } from 'node:fs';
import { runBounded, spawnLongRunning, killTree, isAlive, within, sleep, processesForSession, processStartTime, descendants } from './proc.mjs';

export const ROLES = Object.freeze(['operator-input', 'dialog-accept', 'wait', 'read', 'lifecycle', 'preflight']);

// `herdr agent start --timeout` maximum at v0.9.1 (`herdr agent start --help`).
export const AGENT_START_MAX_TIMEOUT_MS = 300000;
const INPUT_ROLES = new Set(['operator-input', 'dialog-accept']);
// The read-guard state a screen read (agent read, pane read) leaves on its pane (#139).
const SCREEN_READ = 'screen-read';

// A timeout, a refused precondition (wrong herdr version), an expired timebox, or an
// operator abort: the run did not happen as specified. Never a pass, never a failure.
export class NotRunError extends Error {
  constructor(reason) {
    super(reason);
    this.name = 'NotRunError';
    this.reason = reason;
  }
}

export class DriverError extends Error {
  constructor(message) {
    super(message);
    this.name = 'DriverError';
  }
}

// herdr commands that wait on the server and so must carry --timeout (herdr waits
// indefinitely without it: cli-reference.mdx "Output waits").
// herdr's global options that come before the command; the ones listed take a value.
const VALUE_OPTIONS = new Set(['--session', '--machine', '--remote', '--remote-keybindings']);
export function stripGlobalOptions(args) {
  let i = 0;
  while (i < args.length && String(args[i]).startsWith('-') && args[i] !== '--') i += VALUE_OPTIONS.has(args[i]) ? 2 : 1;
  return args.slice(i);
}

export function isHerdrWait(rawArgs) {
  const args = stripGlobalOptions(rawArgs);
  const [a, b] = args;
  return (a === 'agent' && b === 'wait') || (a === 'pane' && b === 'wait-output') || (a === 'agent' && b === 'prompt' && args.includes('--wait'));
}

// herdr commands that put input into a pane or start something in it. Classified from the
// argv itself, never from the role a caller declares, so a mislabelled call cannot slip
// input past the post-timeout halt.
export function isInputCommand(rawArgs) {
  const args = stripGlobalOptions(rawArgs);
  const [a, b] = args;
  if (a === 'agent') return ['prompt', 'send-keys', 'start', 'attach'].includes(b);
  if (a === 'pane') return ['run', 'send-text', 'send-keys'].includes(b);
  if (a === 'terminal') return b === 'attach' || (b === 'session' && args[2] === 'control');
  return false;
}

// Session names: herdr accepts ASCII letters, digits, '.', '_', '-', at most 64 bytes
// (herdr src/session.rs validate_name at v0.9.1).
export function makeSessionName(scenario, now = new Date(), rand = Math.random().toString(16).slice(2, 8)) {
  const stamp = now.toISOString().replace(/[-:]/g, '').replace(/\.\d+Z$/, 'Z');
  const s = String(scenario).toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 20) || 'run';
  return `oac-k-${s}-${stamp}-${rand}`.slice(0, 64);
}

function parseErrorCode(stderr) {
  const lines = String(stderr).trim().split('\n').reverse();
  for (const line of lines) {
    try {
      const j = JSON.parse(line);
      if (j?.error?.code) return String(j.error.code);
    } catch {
      /* not JSON */
    }
  }
  return null;
}

export class HerdrSession {
  // herdrCmd: [file, ...prefixArgs] -- normally ['herdr'].
  // timebox: { remainingMs(): number } ; commands: array the manifest records into.
  constructor({ herdrCmd, sessionName, env, cwd, timebox, commands, abortSignal, graceMs = 5000, defaultDeadlineMs = 15000 }) {
    this.herdrCmd = herdrCmd;
    this.name = sessionName;
    this.env = env;
    this.cwd = cwd;
    this.timebox = timebox;
    this.commands = commands;
    this.abortSignal = abortSignal;
    this.graceMs = graceMs;
    this.defaultDeadlineMs = defaultDeadlineMs;
    this.inputHalted = null; // reason string once a timeout has occurred
    // The dialog-accept read guard (#139): the last successful role per PANE. An agent name
    // and the pane id it was started in are one guard key (agentStart records the mapping), so
    // a command sent to the pane directly resets the guard for the agent too, and a screen read
    // of either covers both. Any other target is its own key. Only `agent read` and `pane read`
    // arm the guard; input with no recorded target resets every guard.
    this.lastRoleByTarget = new Map();
    this.paneOfAgent = new Map(); // agent name -> pane id
    this.panePids = new Map(); // pid -> start time (null where the platform gives none)
    this.server = null;
    // Any abort (timebox expiry, operator signal, end of run) halts input for good, whether
    // or not a command was in flight -- including for calls made with teardown: true.
    const haltOnAbort = () => {
      this.inputHalted ??= this.abortReason();
    };
    if (abortSignal?.aborted) haltOnAbort();
    else abortSignal?.addEventListener('abort', haltOnAbort, { once: true });
  }

  // --- the one choke point -------------------------------------------------------------

  async exec(role, args, opts = {}) {
    const { herdrTimeoutMs = null, deadlineMs = null, target = null, allowErrorCodes = [], session = true, teardown = false, json = false, screenRead = false } = opts;
    if (!ROLES.includes(role)) throw new DriverError(`unknown herdr command role "${role}"`);
    const input = isInputCommand(args);
    if (input && !INPUT_ROLES.has(role)) {
      throw new DriverError(`"herdr ${args.slice(0, 2).join(' ')}" sends input and must carry an input role, not "${role}"`);
    }
    if ((input || INPUT_ROLES.has(role)) && this.inputHalted) {
      throw new DriverError(`refusing ${role} "herdr ${args.slice(0, 2).join(' ')}": input is halted after a timeout (${this.inputHalted}); nothing is re-submitted`);
    }
    if (!teardown && this.abortSignal?.aborted) throw new NotRunError(this.abortReason());

    const argv = [...args];
    let bound;
    if (isHerdrWait(argv)) {
      if (!(herdrTimeoutMs > 0)) throw new DriverError(`herdr wait "herdr ${argv.slice(0, 2).join(' ')}" issued without an explicit timeout`);
      if (argv.includes('--timeout')) throw new DriverError('pass the wait timeout as herdrTimeoutMs, not in args');
      bound = { herdrTimeoutMs, driverDeadlineMs: herdrTimeoutMs + this.graceMs, by: 'herdr --timeout, backstopped by driver deadline' };
    } else if (herdrTimeoutMs > 0) {
      // Commands with their own non-wait --timeout (agent start).
      bound = { herdrTimeoutMs, driverDeadlineMs: herdrTimeoutMs + this.graceMs, by: 'herdr --timeout, backstopped by driver deadline' };
    } else {
      bound = { herdrTimeoutMs: null, driverDeadlineMs: deadlineMs ?? this.defaultDeadlineMs, by: 'driver deadline only (herdr has no --timeout for this command)' };
    }

    if (!teardown) {
      const remaining = this.timebox.remainingMs();
      if (remaining <= 0) {
        this.inputHalted ??= 'timebox expired';
        throw new NotRunError('timebox expired before the next herdr command');
      }
      // The timebox caps every bound: a wait may not outlive the box.
      if (bound.herdrTimeoutMs && bound.herdrTimeoutMs > remaining) {
        bound.herdrTimeoutMs = Math.max(1, remaining);
        bound.clippedToTimebox = true;
      }
      bound.driverDeadlineMs = Math.min(bound.driverDeadlineMs, (bound.herdrTimeoutMs ?? remaining) + this.graceMs, remaining + this.graceMs);
    }
    if (bound.herdrTimeoutMs) {
      // `--timeout` must precede any `-- <agent-args>` separator.
      const sep = argv.indexOf('--');
      argv.splice(sep === -1 ? argv.length : sep, 0, '--timeout', String(bound.herdrTimeoutMs));
    }

    const full = session ? ['--session', this.name, ...argv] : argv;
    const [file, ...prefix] = this.herdrCmd;
    const res = await runBounded(file, [...prefix, ...full], {
      deadlineMs: bound.driverDeadlineMs,
      env: this.env,
      cwd: this.cwd,
      abortSignal: teardown ? undefined : this.abortSignal,
    });

    const errorCode = res.exitCode === 0 ? null : parseErrorCode(res.stderr);
    const aborted = !teardown && this.abortSignal?.aborted && res.killedByDriver && !res.timedOut;
    const timeoutBy = res.timedOut ? 'driver' : errorCode === 'timeout' || errorCode === 'agent_prompt_stalled' ? 'herdr' : null;
    const entry = {
      seq: this.commands.length + 1,
      role,
      argv: ['herdr', ...full],
      target,
      startedAt: res.startedAt,
      endedAt: res.endedAt,
      durationMs: res.durationMs,
      bound,
      exitCode: res.exitCode,
      signal: res.signal,
      errorCode,
      timedOut: timeoutBy !== null,
      timeoutBy,
      killedByDriver: res.killedByDriver,
      killUnconfirmed: res.killUnconfirmed,
      spawnError: res.spawnError,
      stdoutBytes: Buffer.byteLength(res.stdout),
      stderrBytes: Buffer.byteLength(res.stderr),
    };
    this.commands.push(entry);
    // A target's last role is updated only by a command that succeeded: a failed read must
    // not unlock dialogAccept. Keyed per pane (#139). Input with no target could have reached
    // any pane, so it resets every guard.
    const guardKey = target ? this.guardKey(target) : null;
    if (guardKey) this.lastRoleByTarget.delete(guardKey);
    else if (input || INPUT_ROLES.has(role)) this.lastRoleByTarget.clear();

    if (res.spawnError) throw new DriverError(`could not start herdr (${res.spawnError})`);
    if (aborted) {
      this.inputHalted ??= this.abortReason();
      throw new NotRunError(this.abortReason());
    }
    if (timeoutBy) {
      this.inputHalted ??= `${role} command #${entry.seq} timed out (${timeoutBy})`;
      if (teardown) return { ...res, errorCode, entry, json: null };
      throw new NotRunError(`herdr ${argv.slice(0, 2).join(' ')} (${role}) timed out (${timeoutBy === 'driver' ? `driver deadline ${bound.driverDeadlineMs} ms` : `herdr ${errorCode}`}); run ends NOT RUN, nothing re-submitted`);
    }
    if (res.exitCode !== 0 && !allowErrorCodes.includes(errorCode) && !teardown) {
      throw new DriverError(`herdr ${argv.slice(0, 2).join(' ')} failed (exit ${res.exitCode}${errorCode ? `, ${errorCode}` : ''})`);
    }
    let parsed = null;
    if (json && res.exitCode === 0) {
      try {
        parsed = JSON.parse(res.stdout);
      } catch {
        if (!teardown) throw new DriverError(`herdr ${argv.slice(0, 2).join(' ')} did not print JSON`);
      }
    }
    // Only a read of the screen (agent read, pane read: screenRead) arms the guard; any other
    // successful command, a read-role one included (process-info, agent get/explain), resets it.
    if (guardKey && res.exitCode === 0) this.lastRoleByTarget.set(guardKey, role === 'read' && screenRead ? SCREEN_READ : role);
    return { ...res, errorCode, entry, json: parsed };
  }

  // The read-guard key for a target: the pane an agent name was started in, else the target
  // itself (a pane id, or an agent this session did not start).
  guardKey(target) {
    return this.paneOfAgent.get(target) ?? target;
  }

  abortReason() {
    const r = this.abortSignal?.reason;
    return typeof r === 'string' ? r : 'operator abort (signal received)';
  }

  trackPid(pid) {
    if (!Number.isInteger(pid) || this.panePids.has(pid)) return;
    this.panePids.set(pid, processStartTime(pid));
  }

  // Record every live descendant of the pane processes seen so far (a harness's own child
  // processes: MCP servers, tool subprocesses), so teardown can check them too.
  trackPaneTrees() {
    for (const pid of [...this.panePids.keys()]) for (const d of descendants(pid) ?? []) this.trackPid(d);
  }

  // --- preflight and lifecycle -------------------------------------------------------

  async version() {
    const r = await this.exec('preflight', ['--version'], { session: false, deadlineMs: 10000 });
    return r.stdout.trim();
  }

  async startServer({ logPath, readyTimeoutMs = 15000, expectVersion }) {
    const [file, ...prefix] = this.herdrCmd;
    const args = ['--session', this.name, 'server'];
    const fd = openSync(logPath, 'a');
    const startedAt = new Date().toISOString();
    try {
      this.server = spawnLongRunning(file, [...prefix, ...args], { env: this.env, cwd: this.cwd, logFd: fd });
    } finally {
      closeSync(fd);
    }
    this.server.exitInfo = null;
    this.server.exited.then((info) => {
      this.server.exitInfo = info;
    });
    this.commands.push({
      seq: this.commands.length + 1,
      role: 'lifecycle',
      argv: ['herdr', ...args],
      target: null,
      startedAt,
      endedAt: null,
      durationMs: null,
      bound: { herdrTimeoutMs: null, driverDeadlineMs: null, by: 'long-running server; stopped by teardown (session stop, then process-group kill)' },
      pid: this.server.child.pid ?? null,
    });

    const deadline = Date.now() + Math.min(readyTimeoutMs, Math.max(0, this.timebox.remainingMs()));
    for (;;) {
      if (this.server.exitInfo) throw new DriverError(`herdr server exited during startup (code ${this.server.exitInfo.code}); see server.log in the scratch directory`);
      const r = await this.exec('lifecycle', ['status', 'server', '--json'], { json: true, deadlineMs: 5000, allowErrorCodes: [null] });
      if (r.json?.running === true) {
        if (expectVersion && r.json.version !== expectVersion) {
          throw new NotRunError(`herdr server reports version ${JSON.stringify(r.json.version)}, pin requires ${expectVersion}`);
        }
        return r.json;
      }
      if (Date.now() >= deadline) {
        this.inputHalted ??= 'server did not become ready';
        throw new NotRunError(`herdr server did not report running within ${readyTimeoutMs} ms`);
      }
      await sleep(250);
    }
  }

  // Stop the session and make sure nothing it started is still running. Always runs, even
  // after an abort or a timeout; each step is bounded.
  async teardown() {
    const t = { sessionStop: null, serverExited: null, forcedKills: [], leftoverProcesses: [], skippedReusedPids: [], processScan: null, sessionDelete: null, clean: false };
    this.trackPaneTrees();
    const stop = await this.exec('lifecycle', ['session', 'stop', this.name, '--json'], { session: false, teardown: true, deadlineMs: 25000 });
    t.sessionStop = stop.exitCode === 0 ? 'ok' : `exit ${stop.exitCode}${stop.errorCode ? ` ${stop.errorCode}` : ''}${stop.entry.timedOut ? ' (timed out)' : ''}`;

    const serverPid = this.server?.child.pid;
    if (this.server) {
      let exited = this.server.exitInfo ? true : (await within(this.server.exited, 5000)) !== 'timeout';
      if (!exited) {
        killTree(serverPid, 'SIGTERM');
        exited = (await within(this.server.exited, 3000)) !== 'timeout';
        if (!exited) {
          killTree(serverPid, 'SIGKILL');
          exited = (await within(this.server.exited, 3000)) !== 'timeout';
        }
        t.forcedKills.push({ what: 'herdr server', pid: serverPid });
      }
      t.serverExited = exited;
    }

    const stray = new Set();
    for (const [pid, started] of this.panePids) {
      if (!isAlive(pid)) continue;
      // PID reuse guard: a recorded pid now carrying a different start time is someone else's.
      const now = processStartTime(pid);
      if (started !== null && now !== null && now !== started) t.skippedReusedPids.push(pid);
      else stray.add(pid);
    }
    const scanned = processesForSession(this.name);
    t.processScan = scanned === null ? `not available on ${process.platform}; pane and server pids checked individually` : 'argv scan for --session <name>';
    for (const pid of scanned ?? []) stray.add(pid);
    for (const pid of stray) {
      killTree(pid, 'SIGKILL');
      t.forcedKills.push({ what: this.panePids.has(pid) ? 'pane process (or its descendant) left running after session stop' : 'process still carrying --session <name>', pid });
    }
    if (stray.size) await sleep(500);
    for (const pid of [serverPid, ...stray]) if (pid && isAlive(pid)) t.leftoverProcesses.push(pid);

    const del = await this.exec('lifecycle', ['session', 'delete', this.name, '--json'], { session: false, teardown: true, deadlineMs: 15000 });
    t.sessionDelete = del.exitCode === 0 ? 'ok' : `exit ${del.exitCode}${del.errorCode ? ` ${del.errorCode}` : ''}`;
    t.clean = t.sessionStop === 'ok' && t.serverExited !== false && t.forcedKills.length === 0 && t.leftoverProcesses.length === 0 && t.sessionDelete === 'ok';
    return t;
  }

  // --- topology ----------------------------------------------------------------------

  async agentManifests() {
    const r = await this.exec('read', ['server', 'agent-manifests', '--json'], { json: true });
    return r.json;
  }

  async workspaceCreate({ cwd, label, env = {} }) {
    const args = ['workspace', 'create', '--no-focus'];
    if (cwd) args.push('--cwd', cwd);
    if (label) args.push('--label', label);
    for (const [k, v] of Object.entries(env)) args.push('--env', `${k}=${v}`);
    const r = await this.exec('lifecycle', args, { json: true });
    const res = r.json?.result ?? {};
    const out = { workspaceId: res.workspace?.workspace_id, tabId: res.tab?.tab_id, paneId: res.root_pane?.pane_id };
    if (!out.paneId) throw new DriverError('workspace create returned no root_pane.pane_id');
    return out;
  }

  async paneProcessInfo(paneId) {
    const r = await this.exec('read', ['pane', 'process-info', '--pane', paneId], { target: paneId, json: true });
    const info = r.json?.result?.process_info ?? {};
    this.trackPid(info.shell_pid);
    for (const p of info.foreground_processes ?? []) this.trackPid(p.pid);
    this.trackPaneTrees();
    return info;
  }

  // --- panes -------------------------------------------------------------------------

  paneRun(paneId, command) {
    return this.exec('operator-input', ['pane', 'run', paneId, command], { target: paneId });
  }

  paneWaitOutput(paneId, { match, regex, source, lines, timeoutMs }) {
    const args = ['pane', 'wait-output', paneId];
    if (regex) args.push('--regex', regex);
    else args.push('--match', match);
    if (source) args.push('--source', source);
    if (lines) args.push('--lines', String(lines));
    return this.exec('wait', args, { target: paneId, herdrTimeoutMs: timeoutMs });
  }

  async paneRead(paneId, { source = 'recent-unwrapped', lines, deadlineMs } = {}) {
    const args = ['pane', 'read', paneId, '--source', source];
    if (lines) args.push('--lines', String(lines));
    const r = await this.exec('read', args, { target: paneId, deadlineMs, screenRead: true });
    return r.stdout;
  }

  // --- agents ------------------------------------------------------------------------

  // launchArgv[0] is the herdr agent kind (claude, codex, ...): herdr fixes the executable
  // by kind and passes everything after `--` through unchanged (K1 §5 item 2).
  // herdr v0.9.1 `agent start --help`: "--timeout <MS> Wait for interactive readiness
  // (default: 30000; max: 300000)"; a larger value is refused with invalid_agent_timeout
  // (#155). The readiness wait is clamped here; a scenario's own startup budget is separate.
  async agentStart(name, { launchArgv, paneId, timeoutMs, allowErrorCodes = [] }) {
    if (!Array.isArray(launchArgv) || launchArgv.length === 0) throw new DriverError('agentStart needs a launch argv');
    const [kind, ...rest] = launchArgv;
    const args = ['agent', 'start', name, '--kind', kind, '--pane', paneId];
    if (rest.length) args.push('--', ...rest);
    const herdrTimeoutMs = timeoutMs == null ? timeoutMs : Math.min(timeoutMs, AGENT_START_MAX_TIMEOUT_MS);
    // From here on the agent name and its pane share one read guard (#139).
    if (paneId) this.paneOfAgent.set(name, paneId);
    const r = await this.exec('operator-input', args, { target: name, herdrTimeoutMs, json: true, allowErrorCodes });
    return { argv: r.json?.result?.argv ?? null, errorCode: r.errorCode, agent: r.json?.result?.agent ?? null, herdrTimeoutMs };
  }

  // No --timeout exists for `agent read` at v0.9.1: bounded by the driver deadline only.
  async agentRead(target, { source = 'visible', lines, deadlineMs = 10000 } = {}) {
    const args = ['agent', 'read', target, '--source', source];
    if (lines) args.push('--lines', String(lines));
    const r = await this.exec('read', args, { target, deadlineMs, screenRead: true });
    return r.stdout;
  }

  // No --timeout exists for `agent send-keys` at v0.9.1: bounded by the driver deadline only.
  agentSendKeys(target, keys, { deadlineMs = 10000, role = 'operator-input' } = {}) {
    return this.exec(role, ['agent', 'send-keys', target, ...keys], { target, deadlineMs });
  }

  // Accepting a dialog is only allowed straight after reading that target, so the dialog
  // text is on record before any keystroke reaches it (K1 §5 item 4). "Straight after" is
  // per pane (#139): a command sent to the agent's pane by pane id after the read also
  // resets the guard.
  dialogAccept(target, keys = ['enter'], opts = {}) {
    if (this.lastRoleByTarget.get(this.guardKey(target)) !== SCREEN_READ) {
      throw new DriverError(`dialog-accept on "${target}" refused: the last command on it was not a read`);
    }
    return this.agentSendKeys(target, keys, { ...opts, role: 'dialog-accept' });
  }

  // Waits request states explicitly; herdr state is a scheduling signal only, never evidence.
  agentWait(target, { until = [], timeoutMs }) {
    const args = ['agent', 'wait', target];
    for (const s of until) args.push('--until', s);
    return this.exec('wait', args, { target, herdrTimeoutMs: timeoutMs, json: true, allowErrorCodes: [] });
  }

  async agentPrompt(target, text, { wait = null, deadlineMs } = {}) {
    const args = ['agent', 'prompt', target, text];
    if (wait) {
      args.push('--wait');
      for (const s of wait.until ?? []) args.push('--until', s);
    }
    return this.exec('operator-input', args, { target, herdrTimeoutMs: wait ? wait.timeoutMs : null, deadlineMs, json: true });
  }

  async agentGet(target) {
    const r = await this.exec('read', ['agent', 'get', target], { target, json: true });
    return r.json?.result?.agent ?? null;
  }

  async agentExplain(target) {
    const r = await this.exec('read', ['agent', 'explain', target, '--json'], { target, json: true, allowErrorCodes: [] });
    return r.json;
  }
}
