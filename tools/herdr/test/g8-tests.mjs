// Unit checks for the G8 S1 capture tooling (#69/#403): gate-servers/g8-server.mjs (the stub
// MCP server), gate-servers/g8-capture.mjs (the passive capture client) and
// scenarios/g8-codex-tools.mjs (the analysis helpers). Run by run.mjs --self-test (unit half);
// the scenario has no lifecycle case: it needs an operator's Codex TUI.

import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { sanitizeTranscript } from '../lib/g2.mjs';
import { isHistoryCursor } from '../lib/elide.mjs';
import { TOOL, serveCall, negotiate, parseTable, ancestorChain } from '../gate-servers/g8-server.mjs';
import { resumeParams } from '../gate-servers/g8-capture.mjs';
import {
  g8Names, parseServerLog, serverSecrets, serverLogAsTranscript, mcpItems, orderAgainstServer, processTreeSnapshot, imageOf, otherThreadIds, redactOtherThreadIds, serverLogPrecondition,
} from '../scenarios/g8-codex-tools.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const read = (p) => readFileSync(p, 'utf8').replace(/\r\n/g, '\n');
const SCENARIO = read(join(REPO, 'tools', 'herdr', 'scenarios', 'g8-codex-tools.mjs'));
const CLIENT = read(join(REPO, 'tools', 'herdr', 'gate-servers', 'g8-capture.mjs'));
const code = (s) => s.replace(/\/\/.*$/gm, '');

// Drive the real stub server over stdio: frames in, frames out.
function driveServer(frames, logPath) {
  return new Promise((resolveRun, reject) => {
    const child = spawn(process.execPath, [join(REPO, 'tools', 'herdr', 'gate-servers', 'g8-server.mjs'), '--log', logPath], { stdio: ['pipe', 'pipe', 'ignore'] });
    let out = '';
    child.stdout.on('data', (d) => (out += d));
    child.on('error', reject);
    child.on('close', () => resolveRun(out.split('\n').filter(Boolean).map((l) => JSON.parse(l))));
    for (const f of frames) child.stdin.write(`${JSON.stringify(f)}\n`);
    setTimeout(() => child.stdin.end(), 400);
  });
}

export async function g8Unit(check) {
  // --- the stub server ------------------------------------------------------------------------------
  const state = { value: 'abc123' };
  const first = serveCall(state, 1, { name: 'g8_pair' });
  const withValue = serveCall(state, 2, { name: 'g8_pair', arguments: { pairing: 'abc123' } });
  const empty = serveCall(state, 3, { name: 'g8_pair', arguments: {} });
  const absent = serveCall(state, 4, { name: 'g8_pair' });
  const wrong = serveCall(state, 5, { name: 'g8_pair', arguments: { pairing: 'nope' } });
  check('g8 stub: the first call is refused with an error result carrying the issued value, whatever its arguments', first.refused === true && first.result.isError === true && first.result.content[0].text.includes('abc123') && serveCall(state, 1, { arguments: { pairing: 'abc123' } }).refused === true);
  check('g8 stub: a later call with the value succeeds and says it was paired; a call with {}, with no arguments and with a wrong value succeeds unpaired', withValue.refused === false && withValue.paired === true && [empty, absent, wrong].every((r) => r.refused === false && r.paired === false && !r.result.isError));
  check('g8 stub: the one tool has no required member, so {} and no arguments are valid calls', TOOL.name === 'g8_pair' && TOOL.inputSchema.required === undefined && negotiate('2025-06-18') === '2025-06-18' && negotiate('x') === '2025-11-25');

  const dir = mkdtempSync(join(tmpdir(), 'oac-g8-unit-'));
  try {
    const log = join(dir, 'server.jsonl');
    const out = await driveServer(
      [
        { jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-06-18' } },
        { jsonrpc: '2.0', method: 'notifications/initialized' },
        { jsonrpc: '2.0', id: 2, method: 'tools/list' },
        { jsonrpc: '2.0', id: 3, method: 'tools/call', params: { name: 'g8_pair', _meta: { threadId: 'T-1', sessionId: 'S-1' } } },
        { jsonrpc: '2.0', id: 4, method: 'tools/call', params: { name: 'g8_pair', arguments: {} } },
      ],
      log,
    );
    const { events, unparsed } = parseServerLog(readFileSync(log, 'utf8'));
    const refused = out.find((o) => o.id === 3)?.result;
    const issued = events.find((e) => e.event === 'call-out' && e.callSeq === 1)?.issuedValue;
    check('g8 stub (process): initialize, tools/list and two calls answer as designed; the refusal text carries the value the log records', out.find((o) => o.id === 1)?.result?.protocolVersion === '2025-06-18' && out.find((o) => o.id === 2)?.result?.tools?.[0]?.name === 'g8_pair' && refused?.isError === true && typeof issued === 'string' && issued.length === 32 && refused.content[0].text.includes(issued) && out.find((o) => o.id === 4)?.result?.isError === undefined);
    check('g8 stub (process): the log holds call-in with the argument shape and _meta member names, timestamps on every line, and starts with a start event', unparsed === 0 && events[0].event === 'start' && events.find((e) => e.event === 'call-in' && e.callSeq === 1)?.hasArguments === false && events.find((e) => e.event === 'call-in' && e.callSeq === 2)?.hasArguments === true && events.find((e) => e.event === 'call-in' && e.callSeq === 1).metaKeys.join() === 'threadId,sessionId' && events.every((e) => /^\d{4}-\d\d-\d\dT[\d:.]+Z$/.test(e.t)));
    const sec = serverSecrets(events);
    check('g8 stub: serverSecrets finds the pairing value and the ids Codex put in _meta', sec.pairing[0] === issued && sec.threadIds[0] === 'T-1' && sec.sessionIds[0] === 'S-1');
    const tr = serverLogAsTranscript(events).split('\n').map((l) => JSON.parse(l));
    check('g8 stub: the log re-shaped as a transcript keeps frame directions and marks other events', tr.some((o) => o.direction === 'codex->server') && tr.some((o) => o.direction === 'server->codex') && tr.some((o) => o.payload.event === 'call-out'));
    check('g8 stub: it refuses to run without --log (it never writes beside itself)', await new Promise((r) => {
      const c = spawn(process.execPath, [join(REPO, 'tools', 'herdr', 'gate-servers', 'g8-server.mjs')], { stdio: 'ignore' });
      c.on('close', (exit) => r(exit === 2));
    }));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
  check('g8 stub: no log file appears beside the server', !existsSync(join(REPO, 'tools', 'herdr', 'gate-servers', 'server.jsonl')));

  // --- process table pieces ---------------------------------------------------------------------------
  const tbl = parseTable('10,1,codex.exe\n20,10,C:\\Program Files\\x\\node.exe\n30,20,node.exe\n');
  check('g8 stub: the table keeps the image file name only and the chain walks to the root', tbl.get(20).image === 'node.exe' && ancestorChain(tbl, 30).map((r) => r.pid).join() === '30,20,10');

  // --- the capture client -----------------------------------------------------------------------------------
  check('g8 client: thread/resume carries only threadId and excludeTurns', JSON.stringify(resumeParams('T')) === '{"threadId":"T","excludeTurns":true}' && /ws\.request\('thread\/resume', resumeParams\(threadId\)\)/.test(CLIENT));
  check('g8 client: it sends no method that changes a thread', !/'(?:turn\/start|turn\/steer|thread\/queue\/add|turn\/interrupt|thread\/start|thread\/fork)'/.test(code(CLIENT)));
  check('g8 client: it never reads a file, the Codex home or a credential', !/auth\.json|readFileSync|\.sqlite|rollout|homedir|CODEX_HOME/.test(code(CLIENT)));

  // --- the scenario's analysis --------------------------------------------------------------------------------
  const frame = (line, t, method, item) => ({ line, t, mode: 'watch', direction: 'daemon->client', payload: { method, params: { threadId: 'T', turnId: 'U', item } } });
  const item = (id, status, args, result) => ({ type: 'mcpToolCall', id, server: 'oac-g8', tool: 'g8_pair', status, ...(args === undefined ? {} : { arguments: args }), ...(result ? { result } : {}) });
  const wire = [
    frame(10, '2026-10-21T10:00:00.100Z', 'item/started', item('i1', 'inProgress', null)),
    frame(11, '2026-10-21T10:00:00.400Z', 'item/completed', item('i1', 'failed', null, { content: [{ type: 'text', text: 'Pairing value: V1.' }], isError: true })),
    frame(12, '2026-10-21T10:00:01.300Z', 'item/started', item('i2', 'inProgress', {})),
    frame(13, '2026-10-21T10:00:01.400Z', 'item/completed', item('i2', 'completed', {}, { content: [{ type: 'text', text: 'served' }] })),
    { line: 14, t: '2026-10-21T10:00:02.000Z', mode: 'watch', direction: 'daemon->client', payload: { method: 'item/started', params: { item: { type: 'agentMessage', id: 'a' } } } },
  ];
  const items = mcpItems(wire);
  check('g8 items: only mcpToolCall frames, grouped by item id in arrival order, with the arguments kind and result members', items.length === 2 && items[0].started.argumentsKind === 'null' && items[1].started.argumentsKind === 'empty-object' && items[0].completed.status === 'failed' && items[0].completed.isError === true && items[0].completed.resultMembers.join() === 'content,isError' && items[0].started.itemMembers.includes('arguments'));
  const events = parseServerLog([
    { t: '2026-10-21T10:00:00.200Z', event: 'call-in', callSeq: 1, hasArguments: false, argumentKeys: null },
    { t: '2026-10-21T10:00:00.300Z', event: 'call-out', callSeq: 1, refused: true, issuedValue: 'V1' },
    { t: '2026-10-21T10:00:01.300Z', event: 'call-in', callSeq: 2, hasArguments: true, argumentKeys: [] },
    { t: '2026-10-21T10:00:01.350Z', event: 'call-out', callSeq: 2, refused: false },
  ].map((o) => JSON.stringify(o)).join('\n')).events;
  const ord = orderAgainstServer(items, events);
  check('g8 ordering: started before the stub saw the call, completed after it answered; the refusal item carries the issued value; equal stamps are not an order', ord[0].startedBeforeCallIn === true && ord[0].completedAfterCallOut === true && ord[0].completedCarriesIssuedValue === true && ord[0].startedVsCallIn.relation === 'before' && ord[1].startedVsCallIn.relation === 'same-ms' && ord[1].startedBeforeCallIn === true && ord[1].wireArgumentsKind === 'empty-object' && ord[1].stubHasArguments === true);

  const start = (n) => `2026-10-21T10:00:0${n}.000Z`;
  const row = (pid, ppid, cl, n) => [pid, { pid, ppid, start: start(n), startKey: Date.parse(start(n)), argv: null, commandLine: cl, platform: 'win32' }];
  const table = new Map([
    row(1, 0, 'C:\\Windows\\explorer.exe', 0),
    row(100, 1, '"C:\\x\\codex.exe" app-server --listen unix://', 1),
    row(200, 100, '"C:\\x\\codex.exe" --some-secret-flag value', 2),
    row(300, 200, 'node.exe C:\\Users\\someone\\g8-server.mjs --log C:\\Users\\someone\\log', 3),
    row(400, 1, 'codex.exe', 2),
  ]);
  const snap = processTreeSnapshot(table, 300);
  check('g8 process tree: the stub chain and the codex processes, pid/ppid/image only; descent from an app-server process is found', snap.available && snap.serverChain.map((c) => c.pid).join() === '300,200,100,1' && snap.serverDescendsFromAppServer === true && snap.ancestorsThatAreCodex.map((c) => c.pid).join() === '200,100' && snap.codexProcesses.length === 3 && imageOf(table.get(300)) === 'node.exe');
  check('g8 process tree: no command line, argument or path reaches the snapshot', !/secret|Users|someone|--log|listen/.test(JSON.stringify(snap)) && Object.keys(snap.serverChain[0]).sort().join() === 'image,pid,ppid');
  const noApp = new Map([row(1, 0, 'explorer.exe', 0), row(300, 1, 'node.exe g8-server.mjs', 1)]);
  check('g8 process tree: a stub whose chain has no codex process is reported as not descending; a missing pid or table is unavailable', processTreeSnapshot(noApp, 300).serverDescendsFromAppServer === false && processTreeSnapshot(noApp, 999).available === false && processTreeSnapshot(null, 1).available === false);
  const reused = new Map([row(1, 0, 'explorer.exe', 5), row(300, 1, 'node.exe', 1)]);
  check('g8 process tree: a parent link to a process that began after the child is not followed (reused pid)', processTreeSnapshot(reused, 300).serverChain.map((c) => c.pid).join() === '300');

  // --- other threads' ids ------------------------------------------------------------------------------------
  const line = (payload, direction = 'daemon->client', mode = 'list') => JSON.stringify({ t: 'x', mode, direction, payload });
  const text = [
    line({ id: 1, result: { data: ['own-1', 'loaded-2'], nextCursor: null } }),
    line({ method: 'thread/status/changed', params: { threadId: 'notification-6' } }),
  ].join('\n');
  const others = otherThreadIds(text, ['own-1']);
  check('g8 other ids: loaded-list ids other than our own, in order of first appearance; notification ids are not guessed', JSON.stringify(others) === '["loaded-2"]');
  const red = redactOtherThreadIds(text, ['loaded-2', 'x-9']);
  check('g8 other ids: each is replaced by a numbered placeholder and nothing else changes', !/loaded-2/.test(red) && red.includes('<OTHER_THREAD_ID_1>') && red.includes('own-1') && red.includes('notification-6'));

  // --- the shared sanitizer drops other threads' readStates entries (Codex 0.163.0 thread/list) ---------------
  const rsText = [
    line({ jsonrpc: '2.0', id: 2, method: 'thread/list', params: {} }, 'client->daemon'),
    line({ id: 2, result: { data: [{ id: 'own-1' }, { id: 'other-2' }], readStates: { 'other-3': { firstUnread: null, revision: 'initial:other-3' }, 'own-1': { firstUnread: null, revision: 'initial:own-1' }, 'other-4': { firstUnread: null, revision: 'initial:other-4' } }, nextCursor: null } }),
    line({ jsonrpc: '2.0', id: 3, method: 'thread/list', params: {} }, 'client->daemon', 'other'),
    line({ id: 3, result: { data: [], readStates: { 'other-5': { firstUnread: null, revision: 'initial:other-5' } } } }, 'daemon->client', 'other'),
  ].join('\n');
  const san = sanitizeTranscript(rsText, { ownThreadId: 'own-1' });
  const got = san.text.split('\n').map((l) => JSON.parse(l)).filter((o) => o.direction === 'daemon->client');
  check('g8 sanitizer: a thread/list readStates map keeps only the run\'s own thread; every other key and its revision value is gone, and the result is marked', JSON.stringify(Object.keys(got[0].payload.result.readStates)) === '["own-1"]' && got[0].payload.result.readStates['own-1'].revision === 'initial:own-1' && !/other-[2-5]/.test(san.text) && got[0].payload.result._fixtureNote === 'unrelated sessions removed for privacy' && san.report.readStatesEntriesRemoved === 3);
  const none = JSON.parse(sanitizeTranscript(rsText, { ownThreadId: null }).text.split(/\r?\n/)[1]);
  check('g8 sanitizer: with no own thread every readStates entry goes', Object.keys(none.payload.result.readStates).length === 0);
  check('g8 sanitizer: the shared function is the one the g2, g5, l3 and s3 scenarios import', ['g2-codex-inject', 'g5-provenance', 'l3-beacon', 's3-codex-capture', 'g8-codex-tools', 'g8-codex-pairing'].every((n) => /sanitizeTranscript/.test(read(join(REPO, 'tools', 'herdr', 'scenarios', `${n}.mjs`)))));

  // --- the HistoryCursor check keeps the redaction placeholder, exactly -----------------------------------------
  const cur = (id) => JSON.stringify({ requestedThreadId: id, rolloutOrdinal: 1, includeAnchor: true, scope: { kind: 'turns' } });
  check('g8 cursor: isHistoryCursor keeps the exact <THREAD_ID> and a UUID, and rejects <THREAD_ID>x, <OTHER_THREAD_ID_1> and a non-id', isHistoryCursor(cur('<THREAD_ID>')) && isHistoryCursor(cur('01a161bd-83f6-75c1-9598-16d1f0b29724')) && !isHistoryCursor(cur('<THREAD_ID>x')) && !isHistoryCursor(cur('<OTHER_THREAD_ID_1>')) && !isHistoryCursor(cur('x')));

  // --- a reused stub log: exactly one start, no tool call ----------------------------------------------------------
  const logLines = (...evs) => evs.map((e) => JSON.stringify({ t: '2026-10-22T00:00:00.000Z', pid: 1, ppid: 0, seq: 1, ...e })).join('\n');
  const okLog = logLines({ event: 'start' }, { event: 'rpc-in', frame: { jsonrpc: '2.0', id: 0, method: 'initialize' } }, { event: 'tree' });
  const p1 = serverLogPrecondition(okLog);
  check('g8 precondition: a log with one start and no tool call is usable, and the counts are returned', p1.ok && p1.starts === 1 && p1.calls === 0 && p1.reason === null);
  const p0 = serverLogPrecondition(logLines({ event: 'rpc-in', frame: { method: 'x' } }));
  const p2 = serverLogPrecondition(logLines({ event: 'start' }, { event: 'start' }));
  check('g8 precondition: no start, or two starts, is refused with the count in the reason', !p0.ok && p0.starts === 0 && /0 .start. events/.test(p0.reason) && !p2.ok && p2.starts === 2 && /2 .start. events/.test(p2.reason));
  const pc = serverLogPrecondition(logLines({ event: 'start' }, { event: 'call-in', callSeq: 1 }));
  const pf = serverLogPrecondition(logLines({ event: 'start' }, { event: 'rpc-in', frame: { jsonrpc: '2.0', id: 2, method: 'tools/call' } }));
  check('g8 precondition: a log that already holds a tool call (call-in, or a tools/call frame) is refused', !pc.ok && pc.calls === 1 && !pf.ok && pf.calls === 1);
  check('g8 precondition: the scenario applies it before anything else runs and records the counts', /logAtStart && !logAtStart\.ok\) throw new DriverError\(logAtStart\.reason\)/.test(code(SCENARIO)) && /serverLogAtStart:/.test(code(SCENARIO)));

  // --- names and the scenario's guards ---------------------------------------------------------------------------
  check('g8 names: verified versions give fixture names, otherwise unverified-*', g8Names('2026-10-21', '0.162.1').transcript === 'transcript-g8-2026-10-21-0.162.1-herdr.jsonl' && g8Names('2026-10-21', null).server === 'transcript-g8-server-unverified-2026-10-21.jsonl');
  const sc = code(SCENARIO);
  check('g8 scenario: it never runs codex mcp, nor daemon start, stop or restart; of the daemon verbs it runs only daemon version', !/'mcp'|'remove'/.test(sc) && !/'daemon',\s*'(?:start|stop|restart)'/.test(sc) && /codexCli\(\['app-server', 'daemon', 'version'\]/.test(sc));
  check('g8 scenario: it reads no credential, sqlite or rollout file and sends no keystroke to a pane', !/auth\.json|\.sqlite|rollout|CODEX_HOME/.test(sc) && !new RegExp(['key', 'ring'].join('')).test(sc) && !/agentPrompt|sendKeys|paneRun|agentRead|workspaceCreate/.test(sc));
  check('g8 scenario: thread ids, pairing values, session ids and the project directory are registered for redaction before the captures are written', /redactLiteral\(id, '<THREAD_ID>'\)/.test(sc) && /redactLiteral\(v, '<PAIRING_VALUE>'\)/.test(sc) && /'<SESSION_ID>'/.test(sc) && /'<PROJECT_DIR>'/.test(sc) && sc.indexOf("'<PAIRING_VALUE>'") < sc.indexOf('ctx.capture(g8.captureNames.transcript'));
}
