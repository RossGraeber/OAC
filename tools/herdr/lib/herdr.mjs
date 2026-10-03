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

import { openSync, closeSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { runBounded, spawnLongRunning, killTree, killPid, isAlive, within, sleep, processTable, treeFrom, carriesSession, protectedReason, commandLineProblem } from './proc.mjs';

const IS_WIN = process.platform === 'win32';
// The OS operations teardown needs; the self-test substitutes fakes (#136).
export const PROC_OPS = Object.freeze({
  table: () => processTable(),
  isAlive,
  killPid,
  // The herdr server: POSIX kills its process group (the driver spawned it detached, as the
  // group leader). Windows kills the server pid only, never its tree (taskkill /T would
  // reach every descendant unverified); its descendants are tracked and verified first.
  killServer: (pid, signal) => (IS_WIN ? killPid(pid) : killTree(pid, signal)),
  selfPid: process.pid,
});

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

// An agent's state as herdr v0.9.1 reports it (#253). Every agent response (`agent wait`,
// `agent prompt`, `agent get`, `agent start`) carries an AgentInfo under `result.agent`, and the
// state is its `agent_status` field (src/api/schema/agents.rs, `pub agent_status: AgentStatus`,
// at tag v0.9.1; one of idle, working, blocked, done, unknown, cli-reference.mdx "Agents"). There
// is no `state` field: reading one gives undefined, which is how every wait of the 2026-10-02
// runs recorded `null`. Anything else (a missing field, a value herdr does not document) is
// null, and a caller treats null as "state not established", never as settled.
export const AGENT_STATUSES = Object.freeze(['idle', 'working', 'blocked', 'done', 'unknown']);
export function agentStatusOf(json) {
  const s = json?.result?.agent?.agent_status;
  return AGENT_STATUSES.includes(s) ? s : null;
}
// AgentInfo.state_change_seq: herdr's counter of the agent's state changes. herdr's own
// `agent prompt --wait` takes the prompt response's value as its baseline and only counts a
// state as the prompt's effect when `state_change_seq > baseline` (src/api/wait.rs
// prompt_agent / agent_wait_matches at v0.9.1); the driver uses it the same way (#253).
export function stateChangeSeqOf(json) {
  const n = json?.result?.agent?.state_change_seq;
  return Number.isSafeInteger(n) && n >= 0 ? n : null;
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
  // procOps: the OS process operations teardown uses (#136); the self-test passes fakes.
  // fallbackCwd: where teardown commands run when cwd (the run's scratch directory) no longer
  // exists (#239): herdr cannot even be spawned in a missing directory.
  constructor({ herdrCmd, sessionName, env, cwd, timebox, commands, abortSignal, graceMs = 5000, defaultDeadlineMs = 15000, procOps = PROC_OPS, fallbackCwd = tmpdir() }) {
    this.proc = procOps;
    this.fallbackCwd = fallbackCwd;
    this.teardownCwdFallback = false;
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
    // Every process the run's panes (or the herdr server) started that the driver has seen:
    // pid -> { start, via } (start: the process-table identity, null if unknown). Teardown
    // verifies each against a fresh table before it kills anything (#136).
    this.panePids = new Map();
    this.panes = new Set(); // every pane id workspaceCreate returned (#136)
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
    // #239: teardown must stop what the run started even if the scratch directory (herdr's
    // working directory) was removed under the run; every spawn there fails ENOENT.
    let cwd = this.cwd;
    const cwdFallback = teardown && typeof cwd === 'string' && !existsSync(cwd);
    if (cwdFallback) {
      cwd = this.fallbackCwd;
      this.teardownCwdFallback = true;
    }
    const res = await runBounded(file, [...prefix, ...full], {
      deadlineMs: bound.driverDeadlineMs,
      env: this.env,
      cwd,
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
      ...(cwdFallback ? { cwdFallback: 'working directory gone at teardown; ran in os.tmpdir()' } : {}),
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

    if (res.spawnError) {
      // #239: a teardown step that cannot start herdr is recorded, never thrown: the steps
      // after it (the server kill, the pane-process accounting) must still run.
      if (teardown) return { ...res, errorCode, entry, json: null };
      throw new DriverError(`could not start herdr (${res.spawnError})`);
    }
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

  // Record a pid the run's panes started, with its process-table identity (null when the
  // table could not be read or no longer lists it: teardown then never kills it on that pid).
  trackPid(pid, table, via) {
    if (!Number.isInteger(pid) || pid <= 0 || pid === this.proc.selfPid || this.panePids.has(pid)) return;
    this.panePids.set(pid, { start: table?.get(pid)?.start ?? null, via });
  }

  // Record every live descendant of the processes seen so far (a harness's own child
  // processes: MCP servers, tool subprocesses), and of the herdr server, so teardown can check
  // them too. A tracked pid whose identity no longer matches is someone else's now: its tree
  // is not followed.
  trackPaneTrees(table = this.proc.table()) {
    if (!table) return;
    const roots = [...this.panePids].filter(([pid, rec]) => rec.start !== null && table.get(pid)?.start === rec.start).map(([pid]) => pid);
    for (const pid of roots) for (const d of treeFrom(table, pid)) this.trackPid(d, table, 'descendant of a pane process');
    const serverPid = this.server?.child.pid;
    if (serverPid && !this.server.exitInfo) for (const d of treeFrom(table, serverPid)) this.trackPid(d, table, 'descendant of the herdr server');
  }

  // Ask herdr which processes a pane runs and track them (no tree walk).
  async queryPane(paneId, { teardown = false } = {}) {
    const r = await this.exec('read', ['pane', 'process-info', '--pane', paneId], { target: paneId, json: true, teardown, deadlineMs: teardown ? 10000 : null });
    if (r.exitCode !== 0 || !r.json) return { ok: false, why: r.spawnError ? `herdr not started (${r.spawnError})` : r.entry?.timedOut ? 'timed out' : `exit ${r.exitCode}${r.errorCode ? ` ${r.errorCode}` : ''}` };
    const info = r.json?.result?.process_info ?? {};
    const pids = [info.shell_pid, ...(info.foreground_processes ?? []).map((p) => p?.pid)].filter((p) => Number.isInteger(p));
    return { ok: true, info, pids };
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
  //
  // Process accounting (#136): before the stop, every pane the run created is queried
  // (`pane process-info`), whether or not the scenario ever asked, and the pane processes'
  // and the herdr server's descendants are recorded from one process-table snapshot. After
  // the stop, each recorded pid still alive is checked against a fresh snapshot and killed
  // (that pid only, never its tree) only when ALL hold:
  //   - its creation time still matches the recorded one (not a reused pid),
  //   - it was created no earlier than this driver process (nothing older is the run's),
  //   - its command line was read and splits (#244/#249: commandLineProblem; an unsplittable
  //     one, or one that could not be read, is unverified),
  //   - it is not a protected process (protectedReason: the Codex app-server daemon).
  // Anything that cannot be verified (no table, no recorded identity) is never killed; if it
  // is still alive it is reported in leftoverProcesses and the teardown is not clean.
  // #239: no step throws. A herdr call that cannot even be started is recorded and teardown
  // goes on (the server kill and the process checks still run), and if the scratch directory
  // was removed under the run, teardown's herdr calls run in os.tmpdir() (t.cwdFallback).
  async teardown() {
    const t = {
      sessionStop: null,
      serverExited: null,
      panes: { created: this.panes.size, queried: [], notQueried: [] },
      forcedKills: [],
      leftoverProcesses: [],
      unverifiedPids: [],
      skippedReusedPids: [],
      skippedPreexistingPids: [],
      protectedProcesses: [],
      processScan: null,
      sessionDelete: null,
      clean: false,
    };
    const reported = [];
    for (const paneId of this.panes) {
      let q;
      try {
        q = await this.queryPane(paneId, { teardown: true });
      } catch (err) {
        q = { ok: false, why: err.message };
      }
      if (q.ok) {
        t.panes.queried.push(paneId);
        reported.push(...q.pids);
      } else t.panes.notQueried.push({ paneId, why: q.why });
    }
    const before = this.proc.table();
    for (const pid of reported) this.trackPid(pid, before, 'pane process (herdr pane process-info at teardown)');
    this.trackPaneTrees(before);

    const stop = await this.exec('lifecycle', ['session', 'stop', this.name, '--json'], { session: false, teardown: true, deadlineMs: 25000 });
    t.sessionStop = stop.exitCode === 0 ? 'ok' : stop.spawnError ? `herdr not started (${stop.spawnError})` : `exit ${stop.exitCode}${stop.errorCode ? ` ${stop.errorCode}` : ''}${stop.entry.timedOut ? ' (timed out)' : ''}`;

    const serverPid = this.server?.child.pid;
    if (this.server) {
      let exited = this.server.exitInfo ? true : (await within(this.server.exited, 5000)) !== 'timeout';
      if (!exited) {
        this.proc.killServer(serverPid, 'SIGTERM');
        exited = (await within(this.server.exited, 3000)) !== 'timeout';
        if (!exited) {
          this.proc.killServer(serverPid, 'SIGKILL');
          exited = (await within(this.server.exited, 3000)) !== 'timeout';
        }
        t.forcedKills.push({ what: 'herdr server', pid: serverPid });
      }
      t.serverExited = exited;
    }

    const after = this.proc.table();
    const floor = after?.get(this.proc.selfPid)?.startKey ?? null;
    const stray = new Set();
    const seen = new Set();
    // Unverifiable: not killed, and (if still alive at the end) a leftover; teardown not clean.
    const unverified = (pid, why) => t.unverifiedPids.push({ pid, why });
    const consider = (pid, recorded, what) => {
      if (seen.has(pid) || !this.proc.isAlive(pid)) return;
      seen.add(pid);
      const now = after?.get(pid);
      if (!after || recorded === null) return unverified(pid, !after ? 'process table not readable' : 'no creation time was recorded for it');
      // Alive per signal 0 but missing from the table (a partial table): unverifiable. If it
      // really exited in between, the final liveness re-check drops it from the leftovers.
      if (!now) return unverified(pid, 'alive but not in the process table');
      if (now.start !== recorded) {
        t.skippedReusedPids.push(pid);
        return;
      }
      if (floor === null || now.startKey === null) return unverified(pid, 'creation time not comparable with the driver\'s');
      if (now.startKey < floor) {
        t.skippedPreexistingPids.push({ pid, why: 'created before this driver process' });
        return;
      }
      // #244: a command line that cannot be split, or (#249) one that could not be read
      // (Win32_Process.CommandLine null, /proc/<pid>/cmdline unreadable), cannot be shown not
      // to be the app-server.
      const clProblem = commandLineProblem(now);
      if (clProblem) return unverified(pid, clProblem);
      const prot = protectedReason(now);
      if (prot) {
        t.protectedProcesses.push({ pid, why: prot });
        return;
      }
      stray.add(pid);
      t.forcedKills.push({ what, pid });
    };
    for (const [pid, rec] of this.panePids) consider(pid, rec.start, `pane process left running after session stop (${rec.via})`);
    if (after) {
      // A process still carrying this run's unique --session <name> (a herdr process of the run).
      for (const p of after.values()) if (p.pid !== this.proc.selfPid && carriesSession(p, this.name)) consider(p.pid, p.start, 'process still carrying --session <name>');
    }
    t.processScan = after ? 'process table (pid, parent pid, creation time, argv) checked for --session <name>' : `process table not readable on ${process.platform}; recorded pids checked individually, none killed`;
    for (const pid of stray) this.proc.killPid(pid);
    if (stray.size) await sleep(500);
    const unverifiedAlive = t.unverifiedPids.map((u) => u.pid).filter((pid) => this.proc.isAlive(pid));
    for (const pid of new Set([serverPid, ...stray, ...unverifiedAlive])) if (pid && this.proc.isAlive(pid)) t.leftoverProcesses.push(pid);

    const del = await this.exec('lifecycle', ['session', 'delete', this.name, '--json'], { session: false, teardown: true, deadlineMs: 15000 });
    t.sessionDelete = del.exitCode === 0 ? 'ok' : del.spawnError ? `herdr not started (${del.spawnError})` : `exit ${del.exitCode}${del.errorCode ? ` ${del.errorCode}` : ''}`;
    // #239: the scratch directory was gone by teardown (removed under the run), so teardown's
    // herdr calls ran in os.tmpdir() instead. Recorded; it does not by itself make teardown
    // unclean -- what teardown stopped and verified decides that.
    if (this.teardownCwdFallback) t.cwdFallback = 'the scratch directory (herdr\'s working directory) was gone at teardown; teardown\'s herdr commands ran in os.tmpdir()';
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
    // Teardown queries every pane the run created, whether or not the scenario did (#136).
    this.panes.add(out.paneId);
    return out;
  }

  async paneProcessInfo(paneId) {
    return (await this.paneProcessSnapshot(paneId)).info;
  }

  // herdr's answer for a pane and the ONE process-table snapshot the driver took for it
  // (#244, PR #242 review note D): a scenario that reads the pane's argv uses this `table`
  // rather than taking a second one (each is ~1-2 s, one WMI query, on Windows).
  async paneProcessSnapshot(paneId) {
    const q = await this.queryPane(paneId);
    const table = this.proc.table();
    for (const pid of q.pids ?? []) this.trackPid(pid, table, 'pane process (herdr pane process-info)');
    this.trackPaneTrees(table);
    return { info: q.info ?? {}, table };
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
  async agentRead(target, opts = {}) {
    return (await this.agentReadResult(target, opts)).text;
  }

  // The same read, returning herdr's error code too. allowErrorCodes lets a caller handle
  // `agent_not_idle` (#246): herdr refuses an `agent read --lines N` that needs a full-screen
  // agent's alternate-screen history while the agent is working, blocked or unknown
  // (agent-automation.mdx at v0.9.1: "wait for idle and retry, or use --source visible").
  async agentReadResult(target, { source = 'visible', lines, deadlineMs = 10000, allowErrorCodes = [] } = {}) {
    const args = ['agent', 'read', target, '--source', source];
    if (lines) args.push('--lines', String(lines));
    const r = await this.exec('read', args, { target, deadlineMs, screenRead: true, allowErrorCodes });
    return { text: r.exitCode === 0 ? r.stdout : null, errorCode: r.errorCode, entry: r.entry };
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
  // `state` is the reported agent_status, or null when the response carries none herdr
  // documents (#253): the caller must treat null as not established, never as settled.
  async agentWait(target, { until = [], timeoutMs }) {
    const args = ['agent', 'wait', target];
    for (const s of until) args.push('--until', s);
    const r = await this.exec('wait', args, { target, herdrTimeoutMs: timeoutMs, json: true, allowErrorCodes: [] });
    return { ...r, state: agentStatusOf(r.json), stateChangeSeq: stateChangeSeqOf(r.json) };
  }

  // `stateChangeSeq` is the agent's state_change_seq as herdr reported it in the prompt's own
  // response: the baseline a later wait compares against to tell this prompt's turn from the
  // state before it (#253; herdr's `--wait` does the same, src/api/wait.rs at v0.9.1).
  async agentPrompt(target, text, { wait = null, deadlineMs } = {}) {
    const args = ['agent', 'prompt', target, text];
    if (wait) {
      args.push('--wait');
      for (const s of wait.until ?? []) args.push('--until', s);
    }
    const r = await this.exec('operator-input', args, { target, herdrTimeoutMs: wait ? wait.timeoutMs : null, deadlineMs, json: true });
    return { ...r, state: agentStatusOf(r.json), stateChangeSeq: stateChangeSeqOf(r.json) };
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
