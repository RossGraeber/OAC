// Child-process helpers for the herdr driver: every child gets a hard, driver-side deadline.
//
// herdr v0.9.1 bounds only `agent wait`, `agent prompt --wait` and `pane wait-output` itself
// (`--timeout`); `agent read` and `agent send-keys` take no timeout at all
// (docs/planning/decisions/K1-herdr-evaluation.md §5 item 3). So nothing here trusts a child
// to exit: runBounded() kills the child (and, on POSIX, its whole process group) when the
// deadline passes, and resolves even if the kill cannot be confirmed.

import { spawn, spawnSync } from 'node:child_process';
import { readdirSync, readFileSync } from 'node:fs';

const IS_WIN = process.platform === 'win32';
const OUTPUT_CAP = 8 * 1024 * 1024;

// A zombie (exited, not yet reaped by its parent) is not alive: signal 0 still succeeds on
// it, so on Linux its /proc state is checked too.
export function isAlive(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
  } catch (err) {
    if (err.code !== 'EPERM') return false;
  }
  if (process.platform === 'linux') {
    try {
      const stat = readFileSync(`/proc/${pid}/stat`, 'utf8');
      return !/^[ZX]$/.test(stat.slice(stat.lastIndexOf(')') + 2, stat.lastIndexOf(')') + 3));
    } catch {
      return false;
    }
  }
  return true;
}

// Kill a process and, where the platform allows, everything in its process group / tree.
export function killTree(pid, signal = 'SIGKILL') {
  if (!Number.isInteger(pid) || pid <= 0) return;
  if (IS_WIN) {
    spawnSync('taskkill', ['/pid', String(pid), '/T', '/F'], { stdio: 'ignore', timeout: 10000 });
    return;
  }
  try {
    process.kill(-pid, signal); // the group, when the child leads one (spawned detached)
  } catch {
    /* not a group leader, or already gone */
  }
  try {
    process.kill(pid, signal);
  } catch {
    /* already gone */
  }
}

// Run `file args` with a hard deadline. Never rejects for a non-zero exit or a timeout;
// the caller reads the result. `signal` (an AbortSignal) kills the child early.
//
// Result: { exitCode, signal, stdout, stderr, timedOut, killedByDriver, killUnconfirmed,
//           spawnError, startedAt, endedAt, durationMs }
export function runBounded(file, args, { deadlineMs, env, cwd, input, abortSignal } = {}) {
  if (!(deadlineMs > 0)) throw new Error('runBounded: an explicit deadlineMs > 0 is required');
  const started = Date.now();
  return new Promise((resolvePromise) => {
    let stdout = '';
    let stderr = '';
    let settled = false;
    let timedOut = false;
    let killedByDriver = false;
    let hardStop = null;
    let timer = null;
    let child;

    const finish = (extra) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      clearTimeout(hardStop);
      abortSignal?.removeEventListener('abort', onAbort);
      const ended = Date.now();
      resolvePromise({
        exitCode: null,
        signal: null,
        stdout,
        stderr,
        timedOut,
        killedByDriver,
        killUnconfirmed: false,
        spawnError: null,
        startedAt: new Date(started).toISOString(),
        endedAt: new Date(ended).toISOString(),
        durationMs: ended - started,
        ...extra,
      });
    };

    const kill = () => {
      killedByDriver = true;
      killTree(child?.pid);
      // A grandchild holding the pipes open can keep 'close' from ever firing.
      hardStop = setTimeout(() => finish({ killUnconfirmed: true }), 3000);
    };
    const onAbort = () => kill();

    try {
      child = spawn(file, args, {
        env,
        cwd,
        stdio: ['pipe', 'pipe', 'pipe'],
        detached: !IS_WIN, // own process group, so a deadline kill reaches the whole group
        windowsHide: true,
      });
    } catch (err) {
      finish({ spawnError: err.code || String(err) });
      return;
    }

    timer = setTimeout(() => {
      timedOut = true;
      kill();
    }, deadlineMs);
    if (abortSignal) {
      if (abortSignal.aborted) queueMicrotask(onAbort);
      else abortSignal.addEventListener('abort', onAbort, { once: true });
    }

    child.stdout.on('data', (d) => {
      if (stdout.length < OUTPUT_CAP) stdout += d;
    });
    child.stderr.on('data', (d) => {
      if (stderr.length < OUTPUT_CAP) stderr += d;
    });
    child.on('error', (err) => finish({ spawnError: err.code || String(err) }));
    child.on('close', (code, sig) => finish({ exitCode: code, signal: sig }));
    child.stdin.on('error', () => {});
    child.stdin.end(input ?? '');
  });
}

// Start a long-running child (the herdr server). Output goes to the given file descriptor.
export function spawnLongRunning(file, args, { env, cwd, logFd }) {
  const child = spawn(file, args, {
    env,
    cwd,
    stdio: ['ignore', logFd ?? 'ignore', logFd ?? 'ignore'],
    detached: !IS_WIN,
    windowsHide: true,
  });
  const exited = new Promise((res) => {
    child.on('exit', (code, signal) => res({ code, signal }));
    child.on('error', (err) => res({ code: null, signal: null, error: err.code || String(err) }));
  });
  return { child, exited };
}

// Resolve within ms, or return 'timeout'.
export function within(promise, ms) {
  let t;
  return Promise.race([
    promise.finally(() => clearTimeout(t)),
    new Promise((res) => {
      t = setTimeout(() => res('timeout'), ms);
    }),
  ]);
}

export const sleep = (ms) => new Promise((res) => setTimeout(res, ms));

// PIDs of live processes whose argv contains `--session <name>` (or `--session=<name>`).
// Reads only process argv (Linux /proc/<pid>/cmdline, macOS `ps`), never process
// environments. Returns null where the platform gives no cheap way to list argv (Windows):
// the caller records "not checked" rather than "none found".
export function processesForSession(name) {
  const matches = (argv) =>
    argv.some((a, i) => (a === '--session' && argv[i + 1] === name) || a === `--session=${name}`);
  if (process.platform === 'linux') {
    const found = [];
    for (const entry of readdirSync('/proc')) {
      if (!/^\d+$/.test(entry)) continue;
      const pid = Number(entry);
      if (pid === process.pid) continue;
      let argv;
      try {
        argv = readFileSync(`/proc/${entry}/cmdline`, 'utf8').split('\0').filter(Boolean);
      } catch {
        continue;
      }
      if (matches(argv)) found.push(pid);
    }
    return found;
  }
  if (process.platform === 'darwin') {
    const ps = spawnSync('ps', ['-axo', 'pid=,command='], { encoding: 'utf8', timeout: 10000 });
    if (ps.status !== 0) return null;
    const found = [];
    for (const line of ps.stdout.split('\n')) {
      const m = /^\s*(\d+)\s+(.*)$/.exec(line);
      if (!m || Number(m[1]) === process.pid) continue;
      if (matches(m[2].split(/\s+/))) found.push(Number(m[1]));
    }
    return found;
  }
  return null;
}

// Linux: /proc/<pid>/stat fields after the parenthesised command name.
function procStat(pid) {
  try {
    const stat = readFileSync(`/proc/${pid}/stat`, 'utf8');
    return stat.slice(stat.lastIndexOf(')') + 2).split(' ');
  } catch {
    return null;
  }
}

// An identity for "this pid is still the same process": Linux start time in clock ticks
// (stat field 22), macOS `ps -o lstart`. null where unavailable (then only the pid is known).
export function processStartTime(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return null;
  if (process.platform === 'linux') return procStat(pid)?.[19] ?? null;
  if (process.platform === 'darwin') {
    const ps = spawnSync('ps', ['-o', 'lstart=', '-p', String(pid)], { encoding: 'utf8', timeout: 5000 });
    return ps.status === 0 && ps.stdout.trim() ? ps.stdout.trim() : null;
  }
  return null;
}

// Live descendants of pid (children, grandchildren, ...). Reads only pids and parent pids.
// null where the platform gives no cheap way to list them (Windows).
export function descendants(pid) {
  const children = new Map();
  const add = (child, parent) => {
    if (!children.has(parent)) children.set(parent, []);
    children.get(parent).push(child);
  };
  if (process.platform === 'linux') {
    for (const entry of readdirSync('/proc')) {
      if (!/^\d+$/.test(entry)) continue;
      const st = procStat(entry);
      if (st && st[0] !== 'Z') add(Number(entry), Number(st[1]));
    }
  } else if (process.platform === 'darwin') {
    const ps = spawnSync('ps', ['-axo', 'pid=,ppid='], { encoding: 'utf8', timeout: 10000 });
    if (ps.status !== 0) return null;
    for (const line of ps.stdout.split('\n')) {
      const m = /^\s*(\d+)\s+(\d+)/.exec(line);
      if (m) add(Number(m[1]), Number(m[2]));
    }
  } else return null;
  const out = [];
  const queue = [pid];
  while (queue.length) {
    for (const c of children.get(queue.shift()) ?? []) {
      if (!out.includes(c)) {
        out.push(c);
        queue.push(c);
      }
    }
  }
  return out;
}
