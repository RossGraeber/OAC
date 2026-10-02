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

// Kill exactly one process, never its tree: teardown decides each pid itself (#136), so a
// process it did not verify (the operator's Codex daemon, anything the run did not start) is
// never reached through a parent. Windows: `taskkill /F` without /T; POSIX: SIGKILL the pid.
export function killPid(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return;
  if (IS_WIN) {
    spawnSync('taskkill', ['/pid', String(pid), '/F'], { stdio: 'ignore', timeout: 10000, windowsHide: true });
    return;
  }
  try {
    process.kill(pid, 'SIGKILL');
  } catch {
    /* already gone */
  }
}

// --- the process table (#136) ----------------------------------------------------------
//
// One snapshot of every live process: Map pid -> { pid, ppid, start, startKey, argv,
// commandLine }. Reads only pids, parent pids, creation times and argv / command lines,
// never process environments.
//   start     identity string: "this pid is still the same process" while it is unchanged
//             (Linux start time in clock ticks, macOS `ps` lstart, Windows CreationDate UTC)
//   startKey  a number that orders processes of one snapshot by creation (null if unknown)
//   argv      array where the OS gives one (Linux), else null; commandLine is then the
//             OS's own string (macOS `ps` command, Windows Win32_Process.CommandLine)
// Returns null when the table cannot be read; callers then treat every pid as unverified
// and kill nothing on its strength.

// Windows: one Get-CimInstance Win32_Process query (documented WMI class; ~1-2 s).
const WIN32_PROCESS_QUERY =
  "[Console]::OutputEncoding=[Text.Encoding]::UTF8; Get-CimInstance Win32_Process | ForEach-Object { [pscustomobject]@{ p=[int]$_.ProcessId; pp=[int]$_.ParentProcessId; c=if ($_.CreationDate) { $_.CreationDate.ToUniversalTime().ToString('o') } else { $null }; cl=$_.CommandLine } } | ConvertTo-Json -Compress";

export function parseWin32ProcessJson(text) {
  let rows;
  try {
    rows = JSON.parse(String(text).replace(/^\uFEFF/, ''));
  } catch {
    return null;
  }
  if (rows && !Array.isArray(rows)) rows = [rows];
  if (!Array.isArray(rows)) return null;
  const table = new Map();
  for (const r of rows) {
    const pid = Number(r?.p);
    if (!Number.isInteger(pid) || pid <= 0) continue;
    const start = typeof r.c === 'string' && r.c ? r.c : null;
    const key = start ? Date.parse(start) : NaN;
    table.set(pid, { pid, ppid: Number.isInteger(Number(r.pp)) ? Number(r.pp) : null, start, startKey: Number.isFinite(key) ? key : null, argv: null, commandLine: typeof r.cl === 'string' ? r.cl : null });
  }
  return table;
}

// macOS: `ps -axo pid=,ppid=,lstart=,command=`; lstart is "Wed Oct  2 10:00:00 2026".
export function parsePsTable(text) {
  const table = new Map();
  for (const line of String(text).split('\n')) {
    const m = /^\s*(\d+)\s+(\d+)\s+(\w{3}\s+\w{3}\s+\d+\s+\d\d:\d\d:\d\d\s+\d{4})\s?(.*)$/.exec(line);
    if (!m) continue;
    const start = m[3].replace(/\s+/g, ' ');
    const key = Date.parse(start);
    table.set(Number(m[1]), { pid: Number(m[1]), ppid: Number(m[2]), start, startKey: Number.isFinite(key) ? key : null, argv: null, commandLine: m[4] });
  }
  return table;
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

function linuxProcessTable() {
  const table = new Map();
  for (const entry of readdirSync('/proc')) {
    if (!/^\d+$/.test(entry)) continue;
    const st = procStat(entry);
    if (!st || /^[ZX]$/.test(st[0])) continue; // zombies are not alive
    let argv = null;
    try {
      argv = readFileSync(`/proc/${entry}/cmdline`, 'utf8').split('\0').filter(Boolean);
    } catch {
      /* gone, or not readable */
    }
    table.set(Number(entry), { pid: Number(entry), ppid: Number(st[1]), start: st[19] ?? null, startKey: st[19] == null ? null : Number(st[19]), argv, commandLine: null });
  }
  return table;
}

export function processTable(platform = process.platform) {
  if (platform === 'linux') return linuxProcessTable();
  if (platform === 'darwin') {
    const ps = spawnSync('ps', ['-axo', 'pid=,ppid=,lstart=,command='], { encoding: 'utf8', timeout: 10000, env: { ...process.env, LC_ALL: 'C' } });
    return ps.status === 0 ? parsePsTable(ps.stdout) : null;
  }
  if (platform === 'win32') {
    const ps = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', WIN32_PROCESS_QUERY], { encoding: 'utf8', timeout: 30000, windowsHide: true, maxBuffer: 64 * 1024 * 1024 });
    return ps.status === 0 ? parseWin32ProcessJson(ps.stdout) : null;
  }
  return null;
}

// Descendants of rootPid in a table (children, grandchildren, ...). A parent link counts
// only when the child was created no earlier than its parent: Windows keeps a dead parent's
// pid in ParentProcessId, and a reused pid would otherwise adopt unrelated processes. A
// process with no known creation time is never counted (fail-safe: unverifiable).
export function treeFrom(table, rootPid) {
  if (!table?.has(rootPid)) return [];
  const children = new Map();
  for (const p of table.values()) {
    if (p.ppid == null || p.pid === p.ppid) continue;
    if (!children.has(p.ppid)) children.set(p.ppid, []);
    children.get(p.ppid).push(p);
  }
  const out = [];
  const queue = [table.get(rootPid)];
  while (queue.length) {
    const parent = queue.shift();
    for (const c of children.get(parent.pid) ?? []) {
      if (c.pid === rootPid || out.includes(c.pid)) continue;
      if (c.startKey == null || parent.startKey == null || c.startKey < parent.startKey) continue;
      out.push(c.pid);
      queue.push(c);
    }
  }
  return out;
}

// Split a command line the way the OS printed it (double quotes stripped).
export function commandTokens(proc) {
  if (proc?.argv) return proc.argv;
  return [...String(proc?.commandLine ?? '').matchAll(/"([^"]*)"|(\S+)/g)].map((m) => m[1] ?? m[2]);
}

// Does a process carry `--session <name>` (or `--session=<name>`) in its argv?
export function carriesSession(proc, name) {
  const argv = commandTokens(proc);
  return argv.some((a, i) => (a === '--session' && argv[i + 1] === name) || a === `--session=${name}`);
}

// Processes teardown must never kill, whoever started them: the Codex app-server (the
// operator's long-lived shared daemon, #202/#203; a pane's `codex` could also have started
// one). Matched on the argv token `app-server`; left running and recorded instead.
export function protectedReason(proc) {
  return commandTokens(proc).includes('app-server') ? 'Codex app-server (the shared daemon is never stopped by the driver)' : null;
}

// PIDs of live processes whose argv contains `--session <name>`. null when the process
// table cannot be read: the caller records "not checked" rather than "none found".
export function processesForSession(name, table = processTable()) {
  if (!table) return null;
  return [...table.values()].filter((p) => p.pid !== process.pid && carriesSession(p, name)).map((p) => p.pid);
}

// Live descendants of pid. null when the process table cannot be read.
export function descendants(pid, table = processTable()) {
  return table ? treeFrom(table, pid) : null;
}
