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
// windowsVerbatimArguments: pass args to the child unquoted (Windows only; a cmd.exe /s /c
// command line the caller has quoted itself, as harness `--version` probes do, #140).
export function runBounded(file, args, { deadlineMs, env, cwd, input, abortSignal, windowsVerbatimArguments = false } = {}) {
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
        windowsVerbatimArguments,
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
// commandLine[, platform] }. Reads only pids, parent pids, creation times and argv / command lines,
// never process environments.
//   start     identity string: "this pid is still the same process" while it is unchanged
//             (Linux start time in clock ticks, macOS `ps` lstart, Windows CreationDate UTC)
//   startKey  a number that orders processes of one snapshot by creation (null if unknown)
//   argv      array where the OS gives one (Linux), else null; commandLine is then the
//             OS's own string (macOS `ps` command, Windows Win32_Process.CommandLine)
//   platform  the OS the row was read on: on a commandLine row, whose rules split it ('win32'
//             or 'darwin'; #244); 'linux' on a /proc row (#249: argv null = unreadable)
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
    table.set(pid, { pid, ppid: Number.isInteger(Number(r.pp)) ? Number(r.pp) : null, start, startKey: Number.isFinite(key) ? key : null, argv: null, commandLine: typeof r.cl === 'string' ? r.cl : null, platform: 'win32' });
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
    table.set(Number(m[1]), { pid: Number(m[1]), ppid: Number(m[2]), start, startKey: Number.isFinite(key) ? key : null, argv: null, commandLine: m[4], platform: 'darwin' });
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
    table.set(Number(entry), { pid: Number(entry), ppid: Number(st[1]), start: st[19] ?? null, startKey: st[19] == null ? null : Number(st[19]), argv, commandLine: null, platform: 'linux' });
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

// Split a command line the way the OS printed it into the argv the process received. On
// Windows (Win32_Process.CommandLine) that is the Microsoft C runtime's rule set
// (splitWindowsCommandLine, #243). Elsewhere (macOS `ps`, which prints the arguments joined
// by spaces with no quoting) a quoted run is kept together and quotes are stripped; good
// enough to find the `codex` token. null for a non-string or one carrying a NUL.
export function splitCommandLine(s, { platform = process.platform } = {}) {
  if (platform === 'win32') return splitWindowsCommandLine(s);
  if (typeof s !== 'string' || s.includes('\0')) return null;
  const out = [];
  const re = /"([^"]*)"|'([^']*)'|(\S+)/g;
  for (const m of s.matchAll(re)) out.push(m[1] ?? m[2] ?? m[3]);
  return out;
}

// #243: the argv a Microsoft C runtime program (node.exe, codex.exe) builds from its command
// line, per Microsoft's "Parsing C command-line arguments"
// (https://learn.microsoft.com/en-us/cpp/c-language/parsing-c-command-line-arguments,
// ms.date 2021-12-09, retrieved 2026-10-02):
// - arguments are delimited by spaces or tabs;
// - argv[0], the program name, is special: double-quoted parts keep spaces and tabs, the
//   quotes are dropped, and none of the rules below apply (no backslash escaping);
// - a double-quoted string is one argument and may be embedded in an argument; inside a
//   quoted string a pair of double quotes is one literal double quote (and the string goes
//   on); a command line that ends inside a quoted string ends the last argument there;
// - backslashes are literal unless they immediately precede a double quote: 2n backslashes
//   then a quote give n backslashes and the quote is a delimiter; 2n+1 backslashes then a
//   quote give n backslashes and a literal quote.
// Fail closed: a non-string, or one carrying a NUL (no real command line does), gives null, so
// no launch proof can be computed from it; an empty one gives [].
export function splitWindowsCommandLine(s) {
  if (typeof s !== 'string' || s.includes('\0')) return null;
  if (s === '') return [];
  const out = [];
  const n = s.length;
  const blank = (c) => c === ' ' || c === '\t';
  let i = 0;
  // argv[0]: quotes toggle, are dropped; whitespace outside quotes ends it.
  let arg0 = '';
  let inQuote = false;
  for (; i < n; i += 1) {
    const c = s[i];
    if (c === '"') inQuote = !inQuote;
    else if (!inQuote && blank(c)) break;
    else arg0 += c;
  }
  out.push(arg0);
  for (;;) {
    while (i < n && blank(s[i])) i += 1;
    if (i >= n) break;
    let arg = '';
    inQuote = false;
    for (; i < n; i += 1) {
      const c = s[i];
      if (c === '\\') {
        let k = i;
        while (k < n && s[k] === '\\') k += 1;
        const count = k - i;
        if (k < n && s[k] === '"') {
          arg += '\\'.repeat(count >> 1);
          if (count % 2 === 1) {
            arg += '"';
            i = k; // the escaped quote is consumed
          } else {
            i = k - 1; // the quote is handled as a delimiter next round
          }
        } else {
          arg += '\\'.repeat(count);
          i = k - 1;
        }
        continue;
      }
      if (c === '"') {
        if (inQuote && s[i + 1] === '"') {
          arg += '"'; // "" inside a quoted string: one literal quote, still quoted
          i += 1;
        } else {
          inQuote = !inQuote;
        }
        continue;
      }
      if (!inQuote && blank(c)) break;
      arg += c;
    }
    out.push(arg);
  }
  return out;
}

// #244: the tokens of a process-table row. Linux rows carry the argv itself. A Windows or macOS
// row carries the OS's command line string, split by splitCommandLine under the rules of the
// platform the row was read on (row.platform, set by the parsers above; the host's otherwise),
// so teardown and the launch proof (lib/g2.mjs paneArgv) split a command line by one rule set.
// null when the command line cannot be split (a non-string or a NUL), and (#249) when a row
// the OS process table produced (tagged with its platform: win32, darwin or linux) carries no
// argv and no command line, i.e. it could not be read: Win32_Process.CommandLine is null for
// a process the query may not read (another user's, a protected or system process), and a
// Linux /proc/<pid>/cmdline read can fail. An unread command line cannot be shown not to be
// the app-server. The caller treats a null as unverifiable, never as "no match, safe to kill"
// (commandLineProblem names which). [] only for an untagged row with neither (a hand-built
// table: nothing to match). A readable but empty Linux cmdline (a kernel thread) is argv [].
export function commandTokens(proc) {
  if (Array.isArray(proc?.argv)) return proc.argv;
  if (proc?.commandLine == null) return proc?.platform ? null : [];
  return splitCommandLine(proc.commandLine, { platform: proc.platform ?? process.platform });
}

// Does a process carry `--session <name>` (or `--session=<name>`) in its argv? A command line
// that cannot be split matches nothing.
export function carriesSession(proc, name) {
  const argv = commandTokens(proc);
  if (!argv) return false;
  return argv.some((a, i) => (a === '--session' && argv[i + 1] === name) || a === `--session=${name}`);
}

// Processes teardown must never kill, whoever started them: the Codex app-server (the
// operator's long-lived shared daemon, #202/#203; a pane's `codex` could also have started
// one). Matched on the argv token `app-server`; left running and recorded instead. Fail-safe
// (#244): a command line that cannot be split, or (#249) one that could not be read,
// cannot be shown NOT to be the daemon, so it is protected as well (teardown reports such a
// pid as unverified before it gets here).
export const UNSPLITTABLE_REASON = 'command line could not be split, so it cannot be shown not to be the Codex app-server';
export const UNREADABLE_REASON = 'command line could not be read, so it cannot be shown not to be the Codex app-server';
// Why a row's command line gives no tokens (UNREADABLE_REASON or UNSPLITTABLE_REASON), or null
// when it gives tokens (possibly none).
export function commandLineProblem(proc) {
  if (commandTokens(proc) !== null) return null;
  return proc?.commandLine == null ? UNREADABLE_REASON : UNSPLITTABLE_REASON;
}
export function protectedReason(proc) {
  const argv = commandTokens(proc);
  if (!argv) return commandLineProblem(proc);
  return argv.includes('app-server') ? 'Codex app-server (the shared daemon is never stopped by the driver)' : null;
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
