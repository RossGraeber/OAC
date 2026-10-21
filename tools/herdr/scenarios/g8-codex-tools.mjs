// g8-codex-tools: the G8 slice S1 capture (#69 with #403), driven through herdr. NOT A GATE
// RUN and NOT VERDICT-BEARING: it records what a live Codex app-server says about MCP tool
// calls (`mcpToolCall` items) so the fake Codex app-server (tests/fakes/codex-app-server/)
// can replay a recorded shape instead of one modelled from source, and so facts C1-C10 of
// spec/bindings/mcp.md §4.5.1, read at rust-v0.161.0, can be checked at the version that runs.
// It never changes a gate verdict, STATUS.md's gate table or PINS.md.
//
// The operator's own Codex TUI is the system under test. The driver launches no Codex, types
// nothing into it and accepts no dialog: the operator registers the stub MCP server, starts
// plain `codex`, accepts the trust and MCP-approval dialogs and types the prompts. The steps
// are written out in the operator script (G8 S1). The driver never runs `codex mcp add`,
// `codex mcp remove`, `codex app-server daemon start` or `daemon stop`; it runs only
// `codex --version` (the manifest does) and `codex app-server daemon version`. The driver's
// own hash of the Codex config files must show no change over the run, so the registration is
// made before the run starts and removed after it ends.
//
// Operator command:
//
//   node tools/herdr/run.mjs --scenario g8-codex-tools --out <run dir> \
//     --param serverLog=<abs path of the stub server's log file> \
//     --param projectDir=<abs path of the directory the operator's TUI runs in>
//
// What it does:
//   0. Preflight: parameters, the stub server's log must not exist yet, versions against
//      PINS.md as committed (a difference is a VERSION WARNING finding, never a stop, #216).
//   1. `list` (gate-servers/g8-capture.mjs): proves the shared daemon answers; no daemon is
//      started.
//   2. Waits (bounded, polling `list`) for the operator's marker prompt to appear as a loaded
//      thread in the project directory: that is the operator's TUI thread.
//   3. `watch <threadId>`: the capture client subscribes with thread/resume carrying only
//      { threadId, excludeTurns: true } and records every frame the daemon pushes.
//   4. Waits (bounded) for the operator to have the model call the stub tool: until the stub's
//      log shows `expectedCalls` answered calls and the wire shows as many completed
//      mcpToolCall items, then a quiet period. Process-tree data is taken when the stub's own
//      `tree` event appears: pid, parent pid and image name only, never a command line.
//   5. Analysis (recorded in the run manifest): the mcpToolCall items, their order against the
//      stub's own call timestamps, whether the completed refusal item carries the issued
//      value, and whether the stub's process descends from a Codex app-server process.
//   Captures: `transcript-g8-<date>-<version>-herdr.jsonl` (the capture client's wire
//   transcript, sanitized as G2's), `transcript-g8-server-<date>-<version>.jsonl` (the stub's
//   own log re-shaped as a transcript) and `transcript-g8-proc-<date>-<version>.jsonl`
//   (the process-tree data). Thread ids, pairing values, session ids, SIDs, user names and the
//   project directory are redacted.
//
// Credentials: nothing here reads a file under the Codex home or asks an OS credential store
// for anything, and the rollout files are never opened. Node built-ins and the driver's libs.

import { existsSync, readFileSync, realpathSync } from 'node:fs';
import { basename, dirname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { NotRunError, DriverError } from '../lib/herdr.mjs';
import { parseCodexVersions, pinsReadWarning, parseCodexCliVersion, parseCodexDaemonVersion, codexVersionWarning, CODEX_PIN_ROW, CODEX_DAEMON_VERSION_FIELDS } from '../lib/pins.mjs';
import { runBounded, spawnLongRunning, killTree, within, processTable, commandTokens } from '../lib/proc.mjs';
import { CODEX_DAEMON_SCRATCH_HOLDER } from '../lib/scratch.mjs';
import { committedFile } from '../lib/g1.mjs';
import { stageGateFiles } from '../lib/gate-common.mjs';
import { harnessVersions } from '../lib/manifest.mjs';
import { PINS_PATH, parseG2Transcript, g2Facts, identifyTuiThread, sanitizeTranscript, loadedSince } from '../lib/g2.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');

export const G8_GATE = 'G8 S1 capture';
export const G8_NON_VERDICT = '#69/#403 G8 slice S1: a fixture capture of mcpToolCall items; compared against no other run, not a gate run, never changes a gate verdict';
export const G8_SERVER_FILES = Object.freeze(['g8-server.mjs']);
export const G8_CLIENT_FILES = Object.freeze(['g8-capture.mjs']);
export const DEFAULT_MARKER = 'G8 capture marker: reply with the single word ready.';

const sleep = (ms) => new Promise((r) => setTimeout(r, Math.max(0, ms)));

// --- capture names ---------------------------------------------------------------------------------

export function g8Names(date, version) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) throw new Error(`bad date ${JSON.stringify(date)}`);
  const v = /^\d+\.\d+\.\d+$/.test(version ?? '') ? version : null;
  const tail = v ? `${date}-${v}` : `unverified-${date}`;
  return {
    transcript: `transcript-g8-${tail}-herdr.jsonl`,
    server: `transcript-g8-server-${tail}.jsonl`,
    proc: `transcript-g8-proc-${tail}.jsonl`,
  };
}

// --- the stub server's log -------------------------------------------------------------------------

// -> [{ line, t, pid, ppid, seq, event, ... }]; lines that are not JSON are counted, not thrown.
export function parseServerLog(text) {
  const events = [];
  let unparsed = 0;
  String(text ?? '').split('\n').forEach((raw, i) => {
    if (!raw.trim()) return;
    try {
      events.push({ line: i + 1, ...JSON.parse(raw) });
    } catch {
      unparsed += 1;
    }
  });
  return { events, unparsed };
}

export const answeredCalls = (events) => events.filter((e) => e.event === 'call-out');

// Values the stub issued, and the ids Codex put in a tools/call `_meta`, for redaction.
export function serverSecrets(events) {
  const pairing = new Set();
  const threadIds = new Set();
  const sessionIds = new Set();
  for (const e of events) {
    if (e.event === 'call-out' && typeof e.issuedValue === 'string') pairing.add(e.issuedValue);
    const meta = e.frame?.method === 'tools/call' ? e.frame.params?._meta : null;
    if (meta && typeof meta === 'object') {
      if (typeof meta.threadId === 'string') threadIds.add(meta.threadId);
      if (typeof meta.sessionId === 'string') sessionIds.add(meta.sessionId);
    }
  }
  return { pairing: [...pairing], threadIds: [...threadIds], sessionIds: [...sessionIds] };
}

// The stub's log as a transcript in the shape of the capture client's: rpc frames keep their
// direction (codex->server, server->codex); the other events are direction `event`.
export function serverLogAsTranscript(events) {
  return events
    .map((e) => {
      const { line, t, frame, event, ...rest } = e;
      if (event === 'rpc-in') return { t, mode: 'server', direction: 'codex->server', payload: frame };
      if (event === 'rpc-out') return { t, mode: 'server', direction: 'server->codex', payload: frame };
      return { t, mode: 'server', direction: 'event', payload: { event, ...rest } };
    })
    .map((o) => JSON.stringify(o))
    .join('\n');
}

// --- mcpToolCall items on the wire --------------------------------------------------------------------

const argumentsKind = (a) => (a === undefined ? 'absent' : a === null ? 'null' : typeof a === 'object' && !Array.isArray(a) ? (Object.keys(a).length ? 'object' : 'empty-object') : typeof a);

// Every item/started and item/completed frame for an `mcpToolCall` item on a watch connection,
// grouped by item id, in the order the first frame for each arrived.
// -> [{ itemId, started: {line,t,...}|null, completed: {...}|null, ... }]
export function mcpItems(entries) {
  const byId = new Map();
  for (const e of entries) {
    const p = e.payload;
    if (e.direction !== 'daemon->client' || !p || typeof p !== 'object') continue;
    if (p.method !== 'item/started' && p.method !== 'item/completed') continue;
    const item = p.params?.item;
    if (item?.type !== 'mcpToolCall') continue;
    const key = String(item.id);
    if (!byId.has(key)) byId.set(key, { itemId: key, started: null, completed: null });
    const rec = byId.get(key);
    const frame = {
      line: e.line,
      t: e.t,
      threadId: p.params.threadId ?? null,
      turnId: p.params.turnId ?? null,
      server: item.server ?? null,
      tool: item.tool ?? null,
      status: item.status ?? null,
      argumentsKind: argumentsKind(item.arguments),
      arguments: item.arguments === undefined ? null : item.arguments,
      itemMembers: Object.keys(item).sort(),
      resultMembers: item.result && typeof item.result === 'object' ? Object.keys(item.result).sort() : null,
      resultContentText: Array.isArray(item.result?.content) ? item.result.content.map((c) => (typeof c?.text === 'string' ? c.text : '')).join('') : null,
      isError: typeof item.result?.isError === 'boolean' ? item.result.isError : null,
      hasError: item.error !== undefined && item.error !== null,
      paramsMembers: Object.keys(p.params).sort(),
    };
    rec[p.method === 'item/started' ? 'started' : 'completed'] = frame;
  }
  return [...byId.values()];
}

// The ms between two ISO timestamps (b - a), null when either is missing.
export const deltaMs = (a, b) => (a && b ? Date.parse(b) - Date.parse(a) : null);

// Pair the wire's items, in the order their first frame arrived, with the stub's calls by
// callSeq, and order each pair's frames against the stub's timestamps. The wire's `t` is the
// capture client's receipt time of the frame; the stub's is its own, same host clock, ms
// resolution, so equal stamps are reported as such, never as an order.
export function orderAgainstServer(items, events) {
  const callIn = new Map(events.filter((e) => e.event === 'call-in').map((e) => [e.callSeq, e]));
  const callOut = new Map(events.filter((e) => e.event === 'call-out').map((e) => [e.callSeq, e]));
  return items.map((it, i) => {
    const callSeq = i + 1;
    const ci = callIn.get(callSeq) ?? null;
    const co = callOut.get(callSeq) ?? null;
    const rel = (d) => (d === null ? 'unknown' : d < 0 ? 'before' : d === 0 ? 'same-ms' : 'after');
    return {
      itemId: it.itemId,
      callSeq,
      paired: !!(ci && co),
      startedLine: it.started?.line ?? null,
      completedLine: it.completed?.line ?? null,
      // delta = wire frame time minus the stub's time; negative: the frame arrived first
      startedVsCallIn: { ms: deltaMs(ci?.t, it.started?.t), relation: rel(deltaMs(ci?.t, it.started?.t)) },
      completedVsCallOut: { ms: deltaMs(co?.t, it.completed?.t), relation: rel(deltaMs(co?.t, it.completed?.t)) },
      startedBeforeCallIn: deltaMs(ci?.t, it.started?.t) === null ? null : deltaMs(ci?.t, it.started?.t) <= 0,
      completedAfterCallOut: deltaMs(co?.t, it.completed?.t) === null ? null : deltaMs(co?.t, it.completed?.t) >= 0,
      stubRefused: co?.refused ?? null,
      stubArgumentKeys: ci?.argumentKeys ?? null,
      stubHasArguments: ci?.hasArguments ?? null,
      wireArgumentsKind: it.started?.argumentsKind ?? it.completed?.argumentsKind ?? null,
      wireStatus: it.completed?.status ?? null,
      completedCarriesIssuedValue: co?.refused && it.completed?.resultContentText != null && typeof co.issuedValue === 'string' ? it.completed.resultContentText.includes(co.issuedValue) : null,
    };
  });
}

// --- process tree (pid, parent, image only) -----------------------------------------------------------

// Image = the executable's file name; never a directory, argument or user name.
export function imageOf(proc) {
  const tokens = commandTokens(proc);
  const first = Array.isArray(tokens) ? tokens[0] : null;
  return typeof first === 'string' && first ? basename(first.replace(/\\/g, '/')) : null;
}

const isAppServer = (proc) => {
  const t = commandTokens(proc);
  return Array.isArray(t) && t.includes('app-server');
};

// From one OS process table: the stub's ancestors (nearest first), every process whose image
// is a codex executable, and which of the stub's ancestors are such a process. A parent link
// counts only when the child began no earlier than its parent (a reused pid is no parent).
// `table` is lib/proc.mjs's pid -> row Map; `appServerProxy` is true on a codex process whose
// argv carries `app-server` (the daemon, or a `proxy`), recorded as a flag, not as argv.
export function processTreeSnapshot(table, serverPid) {
  if (!table || !table.has(serverPid)) return { available: false, reason: !table ? 'the process table could not be read' : `pid ${serverPid} is not in the process table`, serverPid };
  const row = (p) => ({ pid: p.pid, ppid: p.ppid, image: imageOf(p) });
  const chain = [];
  const seen = new Set();
  let cur = table.get(serverPid);
  while (cur && !seen.has(cur.pid)) {
    seen.add(cur.pid);
    chain.push(row(cur));
    const parent = cur.ppid == null ? null : table.get(cur.ppid);
    if (!parent) break;
    if (cur.startKey != null && parent.startKey != null && cur.startKey < parent.startKey) break;
    cur = parent;
  }
  const codex = [...table.values()].filter((p) => /^codex(?:\.exe|\.js)?$/i.test(imageOf(p) ?? '') || isAppServer(p)).map((p) => ({ ...row(p), appServer: isAppServer(p) }));
  const codexPids = new Set(codex.map((c) => c.pid));
  const ancestorsThatAreCodex = chain.slice(1).filter((c) => codexPids.has(c.pid)).map((c) => ({ ...c, appServer: codex.find((x) => x.pid === c.pid)?.appServer ?? false }));
  return {
    available: true,
    serverPid,
    serverChain: chain,
    codexProcesses: codex,
    ancestorsThatAreCodex,
    serverDescendsFromAppServer: ancestorsThatAreCodex.some((c) => c.appServer),
    serverDescendsFromCodexImage: ancestorsThatAreCodex.length > 0,
  };
}

// --- the scenario ---------------------------------------------------------------------------------------

export default {
  name: 'g8-codex-tools',
  description: 'G8 S1 (#69, #403): capture mcpToolCall items and their order against a stub MCP server, from the operator\'s own Codex TUI. Not a gate run; not verdict-bearing.',
  harnesses: ['codex'],
  defaults: {
    launch: ['codex'], // recorded only: the operator starts Codex; the driver launches nothing
    timeboxMs: 60 * 60 * 1000,
    params: {
      serverLog: '', // required: absolute path the stub server was registered to write
      projectDir: '', // required: absolute path of the directory the operator's TUI runs in
      operatorPrompt: DEFAULT_MARKER,
      serverName: 'oac-g8',
      expectedCalls: '3',
      quietMs: '8000', // after the last expected frame, nothing more is expected
      attachTimeoutMs: '900000',
      toolTimeoutMs: '1200000',
      wireTimeoutMs: '30000',
      cliTimeoutMs: '60000',
      clientTimeoutMs: '60000',
      listPollMs: '3000',
    },
  },

  async run(ctx) {
    const { params } = ctx;
    const num = (k) => {
      const v = Number(params[k]);
      if (!(v >= 0)) throw new DriverError(`--param ${k} must be a non-negative number`);
      return v;
    };
    for (const k of ['serverLog', 'projectDir']) {
      if (!params[k] || !isAbsolute(params[k])) throw new DriverError(`--param ${k}=<absolute path> is required`);
    }
    const expectedCalls = num('expectedCalls');
    const operatorPrompt = params.operatorPrompt || DEFAULT_MARKER;
    const abortSignal = ctx.herdr.abortSignal;
    const serverLog = params.serverLog;
    if (existsSync(serverLog)) throw new DriverError('the stub server log file already exists; use a fresh path so one run holds one registration\'s calls');

    const g8 = {
      nonVerdictBearing: G8_NON_VERDICT,
      params: { ...params, serverLog: '<serverLog>', projectDir: '<projectDir>' },
      date: new Date().toISOString().slice(0, 10),
      versions: null,
      server: null,
      client: null,
      daemon: { versionBefore: null, versionAfter: null },
      clientRuns: [],
      thread: null,
      watch: null,
      calls: null,
      processTree: null,
      items: null,
      ordering: null,
      findings: [],
      captureNames: null,
      stoppedAt: null,
    };
    let clientDir = null;
    let watchProc = null;
    let secrets = { pairing: [], threadIds: [], sessionIds: [] };
    let ownThreadId = null;
    let serverEvents = [];

    const stop = (reason) => {
      ctx.herdr.inputHalted ??= reason;
      g8.stoppedAt = reason;
      throw new NotRunError(reason);
    };
    const aborted = () => {
      if (abortSignal?.aborted) throw new NotRunError(ctx.herdr.abortReason());
    };
    const deadlineFor = (ms) => Date.now() + Math.min(ms, Math.max(0, ctx.remainingMs()));
    const say = (text) => console.error(`\n[g8-codex-tools] ${text}\n`);

    const codexCli = async (args, label) => {
      aborted();
      const [file, argv] = process.platform === 'win32' ? ['cmd.exe', ['/d', '/s', '/c', 'codex', ...args]] : ['codex', args];
      const r = await runBounded(file, argv, { deadlineMs: Math.min(num('cliTimeoutMs'), Math.max(1, ctx.remainingMs())), env: process.env, abortSignal });
      aborted();
      if (r.timedOut) stop(`\`codex ${args.join(' ')}\` timed out after ${num('cliTimeoutMs')} ms; nothing re-run`);
      return { r, rec: { label, argv: ['codex', ...args], exitCode: r.exitCode, timedOut: r.timedOut, spawnError: r.spawnError } };
    };

    const transcriptPath = () => join(clientDir, 'transcript.jsonl');
    const entries = () => (clientDir && existsSync(transcriptPath()) ? parseG2Transcript(readFileSync(transcriptPath(), 'utf8'), { completeLinesOnly: true }) : []);
    const facts = () => g2Facts(entries());
    const readServer = () => (existsSync(serverLog) ? parseServerLog(readFileSync(serverLog, 'utf8')).events : []);

    const runClient = async (mode, args) => {
      aborted();
      const linesBefore = entries().at(-1)?.line ?? 0;
      const r = await runBounded(process.execPath, [join(clientDir, 'g8-capture.mjs'), mode, ...args], { deadlineMs: Math.min(num('clientTimeoutMs'), Math.max(1, ctx.remainingMs())), env: process.env, cwd: clientDir, abortSignal });
      aborted();
      const rec = { mode, startedAt: r.startedAt, endedAt: r.endedAt, exitCode: r.exitCode, timedOut: r.timedOut, linesBefore };
      g8.clientRuns.push(rec);
      if (r.timedOut) stop(`the capture client's \`${mode}\` run timed out after ${num('clientTimeoutMs')} ms`);
      if (r.spawnError || r.exitCode !== 0) throw new DriverError(`the capture client's \`${mode}\` run failed (${r.spawnError ?? `exit ${r.exitCode}`}); is the Codex app-server daemon running? Nothing was started`);
      return rec;
    };

    try {
      // --- 0. preflight ------------------------------------------------------------------------------
      const cliRaw = ctx.harnessVersion('codex');
      const pinsFile = committedFile(REPO, PINS_PATH);
      const pin = parseCodexVersions(pinsFile.bytes.toString('utf8'));
      const cli = parseCodexCliVersion(cliRaw);
      g8.versions = { pinsRow: CODEX_PIN_ROW, pinsMinimum: pin.minimum, pinsLastTested: pin.lastTested, cliOutput: cliRaw, cli, daemon: null, wire: null, verified: false, matchesLastTested: null, warnings: [] };
      const warn = (w) => {
        if (!w) return;
        g8.versions.warnings.push(w);
        ctx.finding(w);
      };
      warn(pinsReadWarning(pin, G8_GATE));
      if (!cli && /^N\/A/.test(cliRaw ?? 'N/A')) stop(`codex --version could not be run (${cliRaw ?? 'not recorded'}); nothing started`);
      warn(codexVersionWarning({ observed: cli, lastTested: pin.lastTested, minimum: pin.minimum, source: '`codex --version`', gate: G8_GATE }));

      clientDir = ctx.dir('g8-client');
      g8.server = { staged: stageGateFiles(REPO, G8_SERVER_FILES, ctx.dir('g8-server')), note: 'the operator registers the committed path, not this copy; the staged copy is for the record (sha256)' };
      g8.client = stageGateFiles(REPO, G8_CLIENT_FILES, clientDir);
      for (const f of [...g8.server.staged, ...g8.client]) if (!f.match) throw new DriverError(`the staged copy of ${f.path} does not match its source (sha256)`);

      // --- 1. the daemon is running; versions ---------------------------------------------------------
      ctx.noteScratchHolder?.(CODEX_DAEMON_SCRATCH_HOLDER);
      const dv = await codexCli(['app-server', 'daemon', 'version'], 'daemon version');
      const daemonV = parseCodexDaemonVersion(dv.r.stdout);
      g8.daemon.versionBefore = { ...dv.rec, parsed: daemonV };
      g8.versions.daemon = daemonV;
      if (!daemonV || daemonV.status !== 'running') stop('the shared Codex app-server daemon is not running (`codex app-server daemon version` reports no running status); the driver never starts it: the operator runs `codex app-server daemon start` before starting the TUI. Nothing was started');
      for (const k of CODEX_DAEMON_VERSION_FIELDS) warn(codexVersionWarning({ observed: daemonV?.[k] ?? null, lastTested: pin.lastTested, minimum: pin.minimum, source: `\`codex app-server daemon version\` ${k}`, gate: G8_GATE }));
      const pre = await runClient('list', []);
      const preConn = facts().connections.find((c) => c.mode === 'list' && c.firstLine > pre.linesBefore);
      g8.versions.wire = preConn?.userAgentVersion ?? null;
      warn(codexVersionWarning({ observed: g8.versions.wire, lastTested: pin.lastTested, minimum: pin.minimum, source: 'the wire initialize userAgent', gate: G8_GATE }));
      const same = !!cli && g8.versions.wire === cli && CODEX_DAEMON_VERSION_FIELDS.every((k) => daemonV?.[k] === cli);
      g8.versions.verified = same;
      g8.versions.matchesLastTested = same ? cli === pin.lastTested : null;
      if (!same) ctx.finding(`the Codex CLI (${cliRaw}), daemon (${JSON.stringify(daemonV)}) and wire (${g8.versions.wire ?? 'none'}) do not report one and the same version; the run continues, but its captures stay unverified-*`);
      g8.captureNames = g8Names(g8.date, same ? cli : null);

      // --- 2. the operator's marker prompt -> the TUI's thread ------------------------------------------
      const projectDirs = [...new Set([params.projectDir, existsSync(params.projectDir) ? realpathSync(params.projectDir) : params.projectDir])];
      const preLoaded = loadedSince(facts().loadedLists, pre.linesBefore);
      say(`Ready. In the Codex TUI (started in ${basename(params.projectDir)}), type exactly this and press Enter:\n  ${operatorPrompt}`);
      const attachDeadline = deadlineFor(num('attachTimeoutMs'));
      let found = null;
      for (;;) {
        const r = await runClient('list', []);
        found = identifyTuiThread(facts(), { operatorPrompt, projectDirs, sinceLine: r.linesBefore });
        if (found.threadId) break;
        if (found.candidates.length > 1) throw new DriverError(`${found.why}: ${found.candidates.map((c) => c.id).join(', ')}`);
        if (Date.now() >= attachDeadline) {
          ctx.finding(`no thread loaded in the daemon matched the marker prompt and the project directory within ${num('attachTimeoutMs')} ms: the TUI may not have attached to the daemon (G2 criterion 1)`);
          stop(`no loaded thread matched the operator's TUI thread within ${num('attachTimeoutMs')} ms (${found.why})`);
        }
        await sleep(num('listPollMs'));
      }
      const threadId = found.threadId;
      ownThreadId = threadId;
      secrets.threadIds.push(threadId);
      g8.thread = { id: '<THREAD_ID>', loadedBeforeMarker: (preLoaded ?? []).includes(threadId) };

      // --- 3. subscribe ------------------------------------------------------------------------------------
      const watchFrom = entries().at(-1)?.line ?? 0;
      watchProc = spawnLongRunning(process.execPath, [join(clientDir, 'g8-capture.mjs'), 'watch', threadId], { env: process.env, cwd: clientDir });
      const killWatch = () => killTree(watchProc?.child.pid, 'SIGTERM');
      abortSignal?.addEventListener('abort', killWatch, { once: true });
      g8.watch = { startedAt: new Date().toISOString(), linesBefore: watchFrom, resumeLine: null, resumeStatus: null, resumeRequestMembers: null, stoppedAt: null };
      const watchExited = () => (watchProc.child.exitCode !== null || watchProc.child.signalCode !== null ? `the capture client's watch process exited (${watchProc.child.exitCode ?? watchProc.child.signalCode})` : null);
      const resumeDeadline = deadlineFor(num('wireTimeoutMs'));
      let resume = null;
      while (!resume) {
        aborted();
        resume = facts().resumes.find((x) => x.mode === 'watch' && x.line > watchFrom && x.threadId === threadId) ?? null;
        if (resume) break;
        if (watchExited()) throw new DriverError(`${watchExited()} before thread/resume answered`);
        if (Date.now() >= resumeDeadline) stop(`timed out after ${num('wireTimeoutMs')} ms waiting for the thread/resume result`);
        await sleep(200);
      }
      if (resume.error) throw new DriverError(`thread/resume answered with error ${JSON.stringify(resume.error)}`);
      g8.watch.resumeLine = resume.line;
      g8.watch.resumeStatus = resume.status;
      const req = entries().find((e) => e.line === resume.reqLine);
      g8.watch.resumeRequestMembers = Object.keys(req?.payload?.params ?? {}).sort();

      // --- 4. the operator has the model call the tool -----------------------------------------------------------
      say(`Subscribed to the TUI thread. Now follow the tool steps of the operator script: ask the model to call ${params.serverName}'s g8_pair tool, and accept each MCP approval dialog with "Allow" (never "Always"). Waiting for ${expectedCalls} calls.`);
      const toolDeadline = deadlineFor(num('toolTimeoutMs'));
      let lastActivity = Date.now();
      let seen = '';
      let treeTaken = false;
      for (;;) {
        aborted();
        serverEvents = readServer();
        const answered = answeredCalls(serverEvents).length;
        const completedItems = mcpItems(entries()).filter((i) => i.completed).length;
        const state = `${serverEvents.length}/${answered}/${completedItems}/${entries().length}`;
        if (state !== seen) {
          seen = state;
          lastActivity = Date.now();
        }
        if (!treeTaken) {
          const tree = serverEvents.find((e) => e.event === 'tree');
          if (tree) {
            treeTaken = true;
            g8.processTree = { stubOwn: tree.ancestors ?? null, stubOwnAvailable: tree.available, atServerEventLine: tree.line, snapshot: processTreeSnapshot(processTable(), tree.pid), note: 'pid, parent pid and image name only' };
          }
        }
        if (answered >= expectedCalls && completedItems >= expectedCalls && Date.now() - lastActivity >= num('quietMs')) break;
        const bail = watchExited();
        if (bail) throw new DriverError(`${bail} while waiting for the tool calls`);
        if (ctx.remainingMs() <= 0 || Date.now() >= toolDeadline) {
          ctx.finding(`only ${answered} stub call(s) answered and ${completedItems} completed mcpToolCall item(s) seen when the wait ended (wanted ${expectedCalls}); the run is NOT RUN, and what was captured is kept`);
          stop(`timed out waiting for ${expectedCalls} tool calls (${answered} answered, ${completedItems} completed items)`);
        }
        await sleep(500);
      }

      // --- 5. analysis -----------------------------------------------------------------------------------------
      serverEvents = readServer();
      secrets = serverSecrets(serverEvents);
      secrets.threadIds.push(threadId);
      const items = mcpItems(entries().filter((e) => e.mode === 'watch'));
      g8.calls = answeredCalls(serverEvents).map((e) => ({ callSeq: e.callSeq, refused: e.refused, paired: e.paired, t: e.t }));
      g8.items = items.map((it) => ({ itemId: it.itemId, started: it.started && { ...it.started, resultContentText: undefined }, completed: it.completed && { ...it.completed, resultContentText: undefined } }));
      g8.ordering = orderAgainstServer(items, serverEvents);
      if (items.length !== answeredCalls(serverEvents).length) ctx.finding(`the wire shows ${items.length} mcpToolCall item(s) and the stub answered ${answeredCalls(serverEvents).length} call(s); the pairing of items to calls by order is not reliable`);
      const toolMetaOnServer = serverEvents.filter((e) => e.event === 'call-in').map((e) => e.metaKeys);
      g8.stubSawMetaKeys = toolMetaOnServer;
      const wrongThread = items.flatMap((i) => [i.started, i.completed]).filter((f) => f && f.threadId !== threadId);
      if (wrongThread.length) ctx.finding(`${wrongThread.length} mcpToolCall frame(s) carried a threadId other than the TUI thread`);

      // --- 6. post-run versions -----------------------------------------------------------------------------------
      const post = await harnessVersions(['codex']);
      const dv2 = await codexCli(['app-server', 'daemon', 'version'], 'daemon version (post-run)');
      g8.daemon.versionAfter = { ...dv2.rec, parsed: parseCodexDaemonVersion(dv2.r.stdout) };
      const postCli = parseCodexCliVersion(post.codex);
      const wireVersions = [...new Set(facts().connections.map((c) => c.userAgentVersion))];
      g8.postRun = { cliOutput: post.codex, cli: postCli, daemon: g8.daemon.versionAfter.parsed, wireVersionsSeen: wireVersions, matches: !!cli && postCli === cli && CODEX_DAEMON_VERSION_FIELDS.every((k) => g8.daemon.versionAfter.parsed?.[k] === cli) && wireVersions.length === 1 && wireVersions[0] === cli };
      if (!g8.postRun.matches) {
        ctx.finding(`the Codex version changed during the run or differed between connections (CLI ${cliRaw} before, ${post.codex} after; wire ${JSON.stringify(wireVersions)}); the captures lose their fixture names`);
        g8.versions.verified = false;
        g8.captureNames = g8Names(g8.date, null);
      }
    } finally {
      if (watchProc) {
        killTree(watchProc.child.pid, 'SIGTERM');
        await within(watchProc.exited, 3000);
        if (g8.watch) g8.watch.stoppedAt = new Date().toISOString();
      }
      serverEvents = readServer();
      const s = serverSecrets(serverEvents);
      secrets = { pairing: [...new Set([...secrets.pairing, ...s.pairing])], threadIds: [...new Set([...secrets.threadIds, ...s.threadIds])], sessionIds: [...new Set([...secrets.sessionIds, ...s.sessionIds])] };
      secrets.threadIds.forEach((id) => ctx.redactLiteral(id, '<THREAD_ID>'));
      secrets.sessionIds.forEach((id) => ctx.redactLiteral(id, '<SESSION_ID>'));
      secrets.pairing.forEach((v) => ctx.redactLiteral(v, '<PAIRING_VALUE>'));
      if (params.projectDir) ctx.redactLiteral(params.projectDir, '<PROJECT_DIR>');
      let transcriptText = null;
      if (clientDir && existsSync(transcriptPath())) {
        const san = sanitizeTranscript(readFileSync(transcriptPath(), 'utf8'), { ownThreadId });
        for (const id of san.installationIds) ctx.redactLiteral(id, '<INSTALLATION_ID>');
        g8.sanitizer = san.report;
        transcriptText = san.text;
        // Other loaded threads' ids appear in thread/loaded/list: not ours to publish.
        const others = new Set();
        for (const l of g2Facts(parseG2Transcript(transcriptText)).loadedLists) for (const id of l.data) if (!secrets.threadIds.includes(id)) others.add(id);
        [...others].forEach((id) => ctx.redactLiteral(id, '<OTHER_THREAD_ID>'));
      }
      ctx.record('g8', g8);
      if (g8.captureNames) {
        if (transcriptText !== null) ctx.capture(g8.captureNames.transcript, transcriptText, { format: 'jsonl' });
        if (serverEvents.length) ctx.capture(g8.captureNames.server, serverLogAsTranscript(serverEvents), { format: 'jsonl' });
        if (g8.processTree) ctx.capture(g8.captureNames.proc, JSON.stringify({ t: new Date().toISOString(), mode: 'proc', direction: 'event', payload: g8.processTree }), { format: 'jsonl' });
      }
    }
  },
};
