// Teardown process accounting (#136). Run from test/selftest.mjs.
//
// teardownUnit runs on every platform, Windows included: HerdrSession.teardown() against a
// stub herdr (a one-line node script that answers `pane process-info` and succeeds on
// everything else) and FAKE process operations (procOps) -- a scripted process table, a
// liveness set, and a kill that only records. No real process is signalled. Tables are
// built in both platform shapes: Win32_Process rows through parseWin32ProcessJson (the
// Windows path), and Linux-shaped rows (start-time ticks, argv arrays).
//
// The lifecycle half (POSIX only, in selftest.mjs) covers the real thing against
// test/fake-herdr.mjs: a scenario that never calls paneProcessInfo, with a forced server kill.

import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { HerdrSession } from '../lib/herdr.mjs';
import { parseWin32ProcessJson, parsePsTable, treeFrom, carriesSession, protectedReason, processesForSession, descendants, commandTokens, commandLineProblem, splitWindowsCommandLine, UNSPLITTABLE_REASON, UNREADABLE_REASON } from '../lib/proc.mjs';

const SESSION = 'oac-k-unit-20261002T000000Z-abc123';
// The stub herdr: process-info for pane w1:p1 reports shell 101 and foreground 102.
const STUB = [
  process.execPath,
  '-e',
  `const a = process.argv.join(' ');
   if (a.includes('process-info') && a.includes('w-broken')) { process.stderr.write(JSON.stringify({ error: { code: 'pane_not_found' } }) + '\\n'); process.exit(1); }
   process.stdout.write(a.includes('process-info') ? JSON.stringify({ result: { process_info: { pane_id: 'w1:p1', shell_pid: 101, foreground_processes: [{ pid: 102 }] } } }) : '{}');`,
  '--',
];

// Win32_Process rows as the driver's CIM query prints them ({p, pp, c, cl}). Times are ISO
// UTC; the driver process (50) started at :00, so anything earlier predates the run.
const T = (s) => `2026-10-02T10:00:${String(s).padStart(2, '0')}.1234567Z`;
function winRows({ reused = false } = {}) {
  return [
    { p: 4, pp: 0, c: T(0).replace('10:00', '09:00'), cl: null },
    { p: 50, pp: 4, c: T(0), cl: 'C:\\node.exe tools\\herdr\\run.mjs' },
    { p: 101, pp: 100, c: T(2), cl: 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe' }, // pane shell; parent 100 (herdr server) already gone
    { p: 102, pp: 101, c: reused ? T(40) : T(3), cl: 'C:\\Users\\op\\AppData\\Roaming\\npm\\codex.cmd' }, // fg harness (reused pid after the stop when `reused`)
    { p: 103, pp: 102, c: T(4), cl: 'node.exe mcp-server.js' }, // harness child (an MCP server)
    { p: 104, pp: 102, c: T(5), cl: 'C:\\codex.exe app-server daemon' }, // a daemon a pane's codex started: protected
    { p: 105, pp: 101, c: T(1), cl: 'C:\\stale-parent-link.exe' }, // created BEFORE its "parent": stale ParentProcessId, not the pane's
    { p: 106, pp: 4, c: T(0).replace('10:00', '08:00'), cl: 'C:\\codex.exe app-server daemon' }, // the operator's daemon, unrelated
    { p: 107, pp: 4, c: T(6), cl: `C:\\herdr.exe --session ${SESSION} pane read w1:p1` }, // a herdr process of this run, not a pane descendant
    { p: 108, pp: 4, c: T(0).replace('10:00', '09:59'), cl: `C:\\herdr.exe --session ${SESSION} server` }, // carries the name but predates the driver
    { p: 109, pp: 4, c: T(7), cl: 'C:\\unrelated.exe' },
  ];
}

// Linux-shaped rows: start = clock ticks since boot (a string), argv arrays.
function linuxTable({ reused = false } = {}) {
  const row = (pid, ppid, ticks, argv) => [pid, { pid, ppid, start: String(ticks), startKey: ticks, argv, commandLine: null }];
  return new Map([
    row(1, 0, 1, ['/sbin/init']),
    row(50, 1, 1000, ['node', 'tools/herdr/run.mjs']),
    row(101, 1, 1002, ['/bin/sh']), // reparented to init after its server exited
    row(102, 101, reused ? 9000 : 1003, ['codex']),
    row(103, 102, 1004, ['node', 'mcp-server.js']),
    row(104, 102, 1005, ['codex', 'app-server', 'daemon']),
    row(105, 101, 1001, ['stale']),
    row(106, 1, 10, ['codex', 'app-server', 'daemon']),
    row(107, 1, 1006, ['herdr', '--session', SESSION, 'pane', 'read', 'w1:p1']),
    row(108, 1, 999, ['herdr', `--session=${SESSION}`, 'server']),
    row(109, 1, 1007, ['unrelated']),
  ]);
}

// Fake procOps: `tables` are returned in order (the last repeats), each filtered to the pids
// still alive; killPid removes a pid from the alive set unless it is listed as unkillable.
function fakeOps(tables, { unkillable = [], alive: initial = null } = {}) {
  const alive = new Set(initial ?? [...tables[0].keys()]);
  const kills = [];
  const serverKills = [];
  let n = 0;
  return {
    kills,
    serverKills,
    alive,
    ops: {
      selfPid: 50,
      table: () => {
        const t = tables[Math.min(n++, tables.length - 1)];
        return t && new Map([...t].filter(([pid]) => alive.has(pid)));
      },
      isAlive: (pid) => alive.has(pid),
      killPid: (pid) => {
        kills.push(pid);
        if (!unkillable.includes(pid)) alive.delete(pid);
      },
      killServer: (pid, sig) => serverKills.push([pid, sig]),
    },
  };
}

function session(ops) {
  return new HerdrSession({ herdrCmd: STUB, sessionName: SESSION, env: process.env, cwd: tmpdir(), timebox: { remainingMs: () => 60000 }, commands: [], procOps: ops });
}

async function teardownCase(tables, { unkillable = [], panes = ['w1:p1'], alive = null } = {}) {
  const f = fakeOps(tables, { unkillable, alive });
  const s = session(f.ops);
  for (const p of panes) s.panes.add(p); // as workspaceCreate would; the scenario never asked
  const t = await s.teardown();
  return { t, f, s };
}

export async function teardownUnit(check) {
  // --- table parsing and the tree walk -----------------------------------------------------
  const win = parseWin32ProcessJson(`\uFEFF${JSON.stringify(winRows())}`);
  check('#136 win32: Win32_Process rows parse (BOM tolerated) with ISO creation identity and an ordering key', win?.size === 11 && win.get(103).start === T(4) && win.get(103).startKey < win.get(104).startKey && win.get(101).ppid === 100);
  const one = parseWin32ProcessJson(JSON.stringify({ p: 7, pp: 4, c: null, cl: null }));
  check('#136 win32: a single row (ConvertTo-Json prints an object) parses; no creation time -> null key', one?.size === 1 && one.get(7).start === null && one.get(7).startKey === null);
  check('#136 win32: unparseable output -> null (table not readable)', parseWin32ProcessJson('not json') === null);
  const mac = parsePsTable('  50     1 Wed Oct  2 10:00:00 2026     node run.mjs\n 101    50 Wed Oct  2 10:00:02 2026 /bin/zsh -l\n');
  check('#136 darwin: ps pid/ppid/lstart/command rows parse', mac.size === 2 && mac.get(101).ppid === 50 && mac.get(101).start === 'Wed Oct 2 10:00:02 2026' && mac.get(101).startKey > mac.get(50).startKey && mac.get(101).commandLine === '/bin/zsh -l');
  check('#136 tree (win32): descendants follow parent links only to processes created no earlier than their parent', JSON.stringify(treeFrom(win, 101).sort()) === JSON.stringify([102, 103, 104]), JSON.stringify(treeFrom(win, 101)));
  check('#136 tree (linux): same walk on Linux-shaped rows', JSON.stringify(treeFrom(linuxTable(), 101).sort()) === JSON.stringify([102, 103, 104]));
  check('#136 tree: an unknown root has no descendants; a null table means "not readable"', treeFrom(win, 999).length === 0 && descendants(101, null) === null && processesForSession(SESSION, null) === null);
  check('#136 session scan: --session <name> found in a Windows command line and a Linux argv', carriesSession(win.get(107), SESSION) && carriesSession(linuxTable().get(108), SESSION) && !carriesSession(win.get(109), SESSION) && JSON.stringify(processesForSession(SESSION, win).sort()) === JSON.stringify([107, 108]));
  check('#136 protected: the Codex app-server (daemon) is protected on both shapes; a plain codex is not', !!protectedReason(win.get(104)) && !!protectedReason(linuxTable().get(106)) && !protectedReason(win.get(102)));

  // #244: commandTokens() splits a Windows command line by the Microsoft C runtime rules
  // (lib/proc.mjs splitWindowsCommandLine, #243), the one rule set the launch proof uses too.
  // Each row carries the platform it was read on, so the result does not depend on the host.
  const winRow = (pid, cl) => parseWin32ProcessJson(JSON.stringify([{ p: pid, pp: 4, c: T(9), cl }])).get(pid);
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  // Escaped quotes: the old naive regex paired the escaped quotes as delimiters, swallowed
  // ` --session ` into one quoted token and missed the session; under the C runtime rules
  // `\"` is a literal quote inside one argument.
  const escSession = winRow(201, `"C:\\Program Files\\h\\herdr.exe" --label "a \\"b\\" c" --session "${SESSION}" server`);
  const escInside = winRow(202, `C:\\x\\codex.exe -c "features.note=\\"app-server\\" only" --session "x\\"${SESSION}"`);
  const escDaemon = winRow(203, 'C:\\x\\codex.exe "app-server" daemon --label "say \\"hi\\""');
  check('#244 commandTokens: Windows rows split by the C runtime rules (escaped quotes kept inside one argument), equal to splitWindowsCommandLine', win.get(107).platform === 'win32' && mac.get(101).platform === 'darwin' && same(commandTokens(escSession), ['C:\\Program Files\\h\\herdr.exe', '--label', 'a "b" c', '--session', SESSION, 'server']) && same(commandTokens(escInside), splitWindowsCommandLine(escInside.commandLine)) && same(commandTokens(escInside).slice(1, 3), ['-c', 'features.note="app-server" only']), JSON.stringify([commandTokens(escSession), commandTokens(escInside)]));
  check('#244 carriesSession (escaped quotes): a quoted --session after an escaped-quote argument matches; a name that is only part of a quoted argument does not', carriesSession(escSession, SESSION) && !carriesSession(escInside, SESSION));
  check('#244 protectedReason (escaped quotes): a quoted app-server token is protected; app-server inside an escaped-quote value is not a token, so not protected', protectedReason(escDaemon) !== null && protectedReason(escDaemon) !== UNSPLITTABLE_REASON && protectedReason(escInside) === null);
  const bad = winRow(204, `C:\\x\\herdr.exe --session ${SESSION} app-server`);
  bad.commandLine = `C:\\x\\herdr.exe --session ${SESSION}\0 app-server`; // a NUL: no real command line carries one
  check('#244 unsplittable command line (null tokens): carriesSession is "no match"; protectedReason is fail-safe (protected, never killed)', commandTokens(bad) === null && carriesSession(bad, SESSION) === false && protectedReason(bad) === UNSPLITTABLE_REASON);
  check('#244 commandTokens: a non-Windows row with no command line has no tokens (nothing to match); a Linux argv is used as is', same(commandTokens({ pid: 9, argv: null, commandLine: null }), []) && same(commandTokens({ pid: 9, argv: null, commandLine: null, platform: 'darwin' }), []) && same(commandTokens(linuxTable().get(107)), ['herdr', '--session', SESSION, 'pane', 'read', 'w1:p1']));
  // #249 (PR #248 review 6): Win32_Process.CommandLine is null where the query may not read it.
  check('#249 unreadable Windows command line (null): no tokens (null), no session match, protected as unreadable (never killed)', win.get(4).commandLine === null && commandTokens(win.get(4)) === null && carriesSession(win.get(4), SESSION) === false && protectedReason(win.get(4)) === UNREADABLE_REASON && commandLineProblem(win.get(4)) === UNREADABLE_REASON && commandLineProblem(bad) === UNSPLITTABLE_REASON && commandLineProblem(win.get(102)) === null);

  // --- teardown against fake process operations, both platform shapes -----------------------
  for (const [shape, before, after] of [
    ['win32', parseWin32ProcessJson(JSON.stringify(winRows())), parseWin32ProcessJson(JSON.stringify(winRows()))],
    ['linux', linuxTable(), linuxTable()],
  ]) {
    const { t, f, s } = await teardownCase([before, after]);
    const killed = [...f.kills].sort((a, b) => a - b);
    check(`#136 ${shape}: a pane the scenario never queried is queried at teardown`, JSON.stringify(t.panes) === JSON.stringify({ created: 1, queried: ['w1:p1'], notQueried: [] }) && s.commands.some((c) => c.argv.includes('process-info')), JSON.stringify(t.panes));
    check(`#136 ${shape}: the pane's shell, harness and harness child, and this run's herdr process are killed; nothing else`, JSON.stringify(killed) === JSON.stringify([101, 102, 103, 107]), JSON.stringify(killed));
    check(`#136 ${shape}: each kill is one pid, recorded with how it was found`, t.forcedKills.length === 4 && t.forcedKills.some((k) => k.pid === 103 && /descendant of a pane process/.test(k.what)) && t.forcedKills.some((k) => k.pid === 107 && /--session/.test(k.what)), JSON.stringify(t.forcedKills));
    check(`#136 ${shape}: the Codex app-server a pane started is left running and recorded, never killed`, !f.kills.includes(104) && t.protectedProcesses.some((p) => p.pid === 104) && f.alive.has(104));
    check(`#136 ${shape}: the operator's own daemon, a stale parent link and unrelated processes are never touched`, [105, 106, 109].every((p) => !f.kills.includes(p) && f.alive.has(p)) && !t.protectedProcesses.some((p) => p.pid === 106));
    check(`#136 ${shape}: a process carrying the session name but older than the driver is skipped, not killed`, !f.kills.includes(108) && t.skippedPreexistingPids.some((p) => p.pid === 108));
    check(`#136 ${shape}: leftoverProcesses empty once the kills took; teardown not clean (forced kills)`, t.leftoverProcesses.length === 0 && t.clean === false && f.serverKills.length === 0, JSON.stringify(t));
  }

  {
    const { t, f } = await teardownCase([linuxTable(), linuxTable()], { unkillable: [103] });
    check('#136: a kill that does not take leaves the pid in leftoverProcesses (reflects reality)', JSON.stringify(t.leftoverProcesses) === JSON.stringify([103]) && f.alive.has(103) && t.clean === false, JSON.stringify(t.leftoverProcesses));
  }
  {
    const { t, f } = await teardownCase([parseWin32ProcessJson(JSON.stringify(winRows())), parseWin32ProcessJson(JSON.stringify(winRows({ reused: true })))]);
    check('#136 win32: a tracked pid whose creation time changed (reused) is skipped, not killed', !f.kills.includes(102) && t.skippedReusedPids.includes(102) && f.kills.includes(101), JSON.stringify(t));
  }
  {
    // #244: a tracked pane descendant whose command line cannot be split is unverified: not
    // killed, a leftover, teardown not clean (it cannot be shown not to be the app-server).
    const rows = () => winRows().map((r) => (r.p === 103 ? { ...r, cl: 'node.exe mcp-server.js\0' } : r));
    const { t, f } = await teardownCase([parseWin32ProcessJson(JSON.stringify(rows())), parseWin32ProcessJson(JSON.stringify(rows()))]);
    check('#244 win32: an unsplittable command line is never killed; it is unverified, a leftover, and teardown is not clean', !f.kills.includes(103) && t.unverifiedPids.some((u) => u.pid === 103 && u.why === UNSPLITTABLE_REASON) && t.leftoverProcesses.includes(103) && !t.protectedProcesses.some((p) => p.pid === 103) && f.kills.includes(102) && t.clean === false, JSON.stringify(t));
  }
  {
    // #249 (PR #248 review 6): a tracked pane descendant whose command line the query could not
    // read (Win32_Process.CommandLine null) is unverified the same way: never killed.
    const rows = () => winRows().map((r) => (r.p === 103 ? { ...r, cl: null } : r));
    const { t, f } = await teardownCase([parseWin32ProcessJson(JSON.stringify(rows())), parseWin32ProcessJson(JSON.stringify(rows()))]);
    check('#249 win32: an unreadable (null) command line is never killed; it is unverified, a leftover, and teardown is not clean', !f.kills.includes(103) && t.unverifiedPids.some((u) => u.pid === 103 && u.why === UNREADABLE_REASON) && t.leftoverProcesses.includes(103) && !t.protectedProcesses.some((p) => p.pid === 103) && f.kills.includes(102) && t.clean === false, JSON.stringify(t));
  }
  {
    const { t, f } = await teardownCase([null, null], { alive: [101, 102] });
    check('#136 fail-safe: no readable process table -> nothing killed; herdr-reported live pids are unverified leftovers; not clean', f.kills.length === 0 && t.unverifiedPids.map((u) => u.pid).sort().join() === '101,102' && t.leftoverProcesses.slice().sort().join() === '101,102' && t.clean === false && /not readable/.test(t.processScan), JSON.stringify(t));
  }
  for (const [shape, mk] of [['win32', () => parseWin32ProcessJson(JSON.stringify(winRows()))], ['linux', linuxTable]]) {
    // #136 review, case A: a partial after-table omits a live tracked pid (101).
    const partial = mk();
    partial.delete(101);
    const a = await teardownCase([mk(), partial]);
    check(`#136 review ${shape}: a live tracked pid missing from the after-table is not killed, but is unverified, a leftover, and teardown is not clean`, !a.f.kills.includes(101) && a.t.unverifiedPids.some((u) => u.pid === 101 && /not in the process table/.test(u.why)) && a.t.leftoverProcesses.includes(101) && a.t.clean === false, JSON.stringify(a.t));
    // Case B: the after-table lacks the driver's own row (50): no floor, nothing comparable.
    const noDriver = mk();
    noDriver.delete(50);
    const b = await teardownCase([mk(), noDriver]);
    const live = [101, 102, 103, 107];
    check(`#136 review ${shape}: without the driver's row nothing is killed; live tracked pids and session carriers are unverified leftovers; not clean`, b.f.kills.length === 0 && live.every((p) => b.t.unverifiedPids.some((u) => u.pid === p && /not comparable/.test(u.why)) && b.t.leftoverProcesses.includes(p)) && b.t.skippedPreexistingPids.length === 0 && b.t.clean === false, JSON.stringify(b.t));
    check(`#136 review ${shape}: each unverified pid is listed once`, new Set(b.t.unverifiedPids.map((u) => u.pid)).size === b.t.unverifiedPids.length, JSON.stringify(b.t.unverifiedPids));
  }
  {
    const { t, f } = await teardownCase([linuxTable(), linuxTable()], { panes: ['w-broken'] });
    check('#136: a pane herdr cannot report is recorded as not queried; nothing guessed, only the session-name scan acts', t.panes.notQueried.length === 1 && t.panes.notQueried[0].paneId === 'w-broken' && JSON.stringify(f.kills) === JSON.stringify([107]), JSON.stringify({ panes: t.panes, kills: f.kills }));
  }
  {
    // Everything gone by itself (herdr's stop took the panes down): clean, nothing killed.
    const before = linuxTable();
    const after = new Map([...before].filter(([pid]) => ![101, 102, 103, 107].includes(pid)));
    const f = fakeOps([before, after]);
    for (const pid of [101, 102, 103, 107]) f.alive.delete(pid);
    const ops = { ...f.ops, table: (() => { let n = 0; return () => (n++ === 0 ? before : after); })() };
    const s = session(ops);
    s.panes.add('w1:p1');
    const t = await s.teardown();
    check('#136: when the stop took every pane process down, nothing is killed and teardown is clean', f.kills.length === 0 && t.forcedKills.length === 0 && t.leftoverProcesses.length === 0 && t.clean === true, JSON.stringify(t));
  }

  // #239: the scratch directory (herdr's working directory) removed under the run. Before the
  // fix every teardown herdr call failed to spawn (ENOENT), the first one threw out of
  // teardown, and the server and every pane process were left running.
  {
    const gone = mkdtempSync(join(tmpdir(), 'oac-herdr-unit239-'));
    rmSync(gone, { recursive: true, force: true });
    const f = fakeOps([linuxTable(), linuxTable()]);
    const s = new HerdrSession({ herdrCmd: STUB, sessionName: SESSION, env: process.env, cwd: gone, timebox: { remainingMs: () => 60000 }, commands: [], procOps: f.ops });
    s.panes.add('w1:p1');
    // #249 (PR #250 review): a teardown that throws here (the #239 regression) is a named
    // failing check, not an exception out of the whole unit run.
    let t = null;
    let threw = null;
    try {
      t = await s.teardown();
    } catch (err) {
      threw = err;
    }
    check('#239 scratch gone: teardown returns (no throw)', !threw, threw?.stack);
    const killed = [...f.kills].sort((a, b) => a - b);
    check('#239 scratch gone: teardown\'s herdr calls run in os.tmpdir(), recorded on the teardown and on each command', !!t && /gone at teardown/.test(t.cwdFallback ?? '') && s.commands.length === 3 && s.commands.every((c) => !c.spawnError && /os\.tmpdir\(\)/.test(c.cwdFallback ?? '')), JSON.stringify({ t, commands: s.commands }));
    check('#239 scratch gone: the pane is still queried, the session stopped and deleted, and the same pane processes killed', !!t && t.panes.queried.length === 1 && t.sessionStop === 'ok' && t.sessionDelete === 'ok' && JSON.stringify(killed) === JSON.stringify([101, 102, 103, 107]), JSON.stringify({ t, killed }));
  }
  {
    // herdr cannot be started at all during teardown: nothing throws; the server is still
    // force-killed, and the unstarted calls are recorded.
    const f = fakeOps([linuxTable(), linuxTable()]);
    let exit;
    const exited = new Promise((res) => (exit = res));
    const ops = { ...f.ops, killServer: (pid, sig) => (f.serverKills.push([pid, sig]), exit({ code: null, signal: sig })) };
    const missing = join(tmpdir(), 'oac-herdr-unit239-no-such-herdr', 'herdr');
    const s = new HerdrSession({ herdrCmd: [missing], sessionName: SESSION, env: process.env, cwd: tmpdir(), timebox: { remainingMs: () => 60000 }, commands: [], procOps: ops });
    s.panes.add('w1:p1');
    s.server = { child: { pid: 4242 }, exited, exitInfo: null };
    let t = null;
    let threw = null;
    try {
      t = await s.teardown();
    } catch (err) {
      threw = err;
    }
    check('#239 herdr not startable at teardown: teardown returns (no throw) and records each unstarted call', !threw && /herdr not started/.test(t?.sessionStop ?? '') && /herdr not started/.test(t?.sessionDelete ?? '') && /herdr not started/.test(t?.panes.notQueried[0]?.why ?? '') && s.commands.every((c) => c.spawnError), threw ? threw.stack : JSON.stringify(t));
    check('#239 herdr not startable at teardown: the herdr server is still force-killed; teardown not clean', !!t && f.serverKills.length === 1 && f.serverKills[0][0] === 4242 && t.forcedKills.some((k) => k.what === 'herdr server') && t.serverExited === true && t.clean === false, JSON.stringify({ t, serverKills: f.serverKills }));
  }
}
