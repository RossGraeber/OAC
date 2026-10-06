// #130: tool-output elision in the capture path (lib/elide.mjs, lib/redact.mjs, run.mjs).
//
// Every frame here is SYNTHETIC: shaped like the Codex 0.160.0 app-server frames of the G2
// 2026-10-05 run (a file read run as a commandExecution with a read action, its output
// deltas, an mcpToolCall with a text result, the turn record), with bodies this file makes
// up. No text a harness read or a tool returned in any real run appears in this file.

import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

import { createRedactor, reportIsClean, UNELIDED_LABEL, UNRECOGNISED_LABEL } from '../lib/redact.mjs';
import { ELIDED_RE, ITEM_OUTPUT_FIELDS, NOTIFICATION_OUTPUT_FIELDS, PANE_MIN_LENGTH, elideToolOutputs, unrecognisedLongText, isHistoryCursor, elidePaneLines, keptStrings, redactCaptures } from '../lib/elide.mjs';
import { g2Facts, parseG2Transcript } from '../lib/g2.mjs';
import { g5CodexFacts, parseJsonl } from '../lib/g5.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const FIXTURES = join(REPO, 'docs', 'planning', 'gates', 'fixtures');
const REDACT_CLI = join(REPO, 'tools', 'herdr', 'lib', 'redact.mjs');
const sha256 = (s) => createHash('sha256').update(s, 'utf8').digest('hex');
const marker = (s, kind = 'tool-output') => `<ELIDED ${kind} bytes=${Buffer.byteLength(s, 'utf8')} sha256=${sha256(s)}>`;

// --- synthetic bodies --------------------------------------------------------------------
const line = (tag, n) => `synthetic ${tag} body line ${n} for the elision self-test (#130)`;
export const SYNTH = Object.freeze({
  command: 'Get-Content -Raw synthetic/SKILL-FIXTURE.md',
  name: 'SKILL-FIXTURE.md',
  path: 'synthetic/SKILL-FIXTURE.md',
  output: [1, 2, 3, 4].map((n) => line('file-read', n)).join('\n') + '\n',
  server: 'synthetic_server',
  tool: 'synthetic_tool',
  arguments: { code: 'await synthetic.getState();' },
  mcpResult: [1, 2, 3].map((n) => line('mcp-result', n)).join('\n'),
});
const SHARED = line('quoted-by-the-model', 1); // in a tool body AND quoted in an agent message
const FN_OUT = line('function-output', 1);
const DYN_OUT = line('dynamic-tool', 1);
const WEB_OUT = line('web-result', 1);
const PROGRESS = line('mcp-progress', 1);
const STRUCT = line('structured-content', 1);
const ERR = line('mcp-error', 1);
// #130 review B1/N1/N2: file diffs, hook output, process output, MCP event streams, free-text keys.
const DIFF = [1, 2, 3].map((n) => line('file-diff', n)).join('\n');
const TURN_DIFF = [1, 2].map((n) => line('turn-diff', n)).join('\n');
const PATCH_DIFF = line('patch-updated-diff', 1);
const HOOK_OUT = line('hook-entry', 1);
const HOOK_PROMPT = line('hook-prompt-fragment', 1);
const PROC_OUT = line('process-stdout', 1);
const PROC_ERR = line('process-stderr', 1);
const MCP_EVENT = line('mcp-event-stream', 1);
const FREE_KEY = 'a free-text key a tool chose (#130 review N2)';
const FREE_TYPE = 'a prose type value a tool chose';
const ALL_BODIES = [...SYNTH.output.split('\n').filter(Boolean), ...SYNTH.mcpResult.split('\n'), SHARED, FN_OUT, DYN_OUT, WEB_OUT, PROGRESS, STRUCT, ERR, ...DIFF.split('\n'), ...TURN_DIFF.split('\n'), PATCH_DIFF, HOOK_OUT, HOOK_PROMPT, PROC_OUT, PROC_ERR, MCP_EVENT, FREE_KEY, FREE_TYPE];

const DELIVERED = 'G2 synthetic delivery (from a second daemon client, not typed in this TUI): reply with exactly OAC G2 SYNTHETIC';
const ANSWER = `OAC G2 SYNTHETIC. Quoting: ${SHARED}`;
const LONG_ANSWER = Array.from({ length: 120 }, (_, i) => `${i + 1}. a long model-authored answer line`).join('\n'); // > 4000 chars
const TH = '01a00000-0000-7000-8000-000000000001';
const TURN = '01a00000-0000-7000-8000-000000000002';
const ids = { threadId: TH, turnId: TURN };
const cmdItem = (done) => ({ type: 'commandExecution', id: 'call_cmd1', command: SYNTH.command, cwd: null, processId: null, source: 'agent', status: done ? 'completed' : 'inProgress', commandActions: [{ type: 'read', command: SYNTH.command, name: SYNTH.name, path: SYNTH.path }], aggregatedOutput: done ? SYNTH.output : null, exitCode: done ? 0 : null, durationMs: done ? 12 : null });
const mcpItem = (done) => ({ type: 'mcpToolCall', id: 'call_mcp1', server: SYNTH.server, tool: SYNTH.tool, status: done ? 'completed' : 'inProgress', arguments: SYNTH.arguments, result: done ? { content: [{ type: 'text', text: SYNTH.mcpResult }, { type: 'text', text: SHARED }], structuredContent: { note: STRUCT, count: 3, [FREE_KEY]: true, kind: { type: FREE_TYPE } }, _meta: null } : null, error: null, durationMs: done ? 30 : null });
const mcpFailed = { type: 'mcpToolCall', id: 'call_mcp2', server: SYNTH.server, tool: SYNTH.tool, status: 'failed', arguments: {}, result: null, error: { message: ERR }, durationMs: 3 };
const fnItem = { type: 'functionCallOutput', id: 'call_fn1', name: 'synthetic_fn', namespace: null, output: [{ type: 'input_text', text: FN_OUT }] };
const dynItem = { type: 'dynamicToolCall', id: 'call_dyn1', tool: 'synthetic_dyn', namespace: null, arguments: {}, status: 'completed', success: true, contentItems: [{ type: 'inputText', text: DYN_OUT }], durationMs: 5 };
const webItem = { type: 'webSearch', id: 'ws_1', query: 'synthetic query', action: { type: 'search', query: 'synthetic query' }, results: [{ title: WEB_OUT, url: 'https://example.invalid/x' }] };
const userItem = { type: 'userMessage', id: 'u1', clientId: null, content: [{ type: 'text', text: DELIVERED, text_elements: [] }] };
const agentItem = { type: 'agentMessage', id: 'msg_1', text: ANSWER, phase: 'final_answer' };
const longItem = { type: 'agentMessage', id: 'msg_2', text: LONG_ANSWER, phase: 'final_answer' };
const fileItem = { type: 'fileChange', id: 'call_patch1', status: 'completed', changes: [{ path: 'synthetic/edited.md', kind: { type: 'update', move_path: null }, diff: DIFF }] };
const hookPromptItem = { type: 'hookPrompt', id: 'hook_1', fragments: [{ hookRunId: 'run_1', text: HOOK_PROMPT }] };
const turnItems = [userItem, cmdItem(true), mcpItem(true), mcpFailed, fnItem, dynItem, webItem, fileItem, hookPromptItem, agentItem, longItem];

// G2-shaped records: {t, mode, direction, payload}.
let tick = 0;
const rec = (mode, direction, payload) => JSON.stringify({ t: `2026-10-05T02:06:${String(10 + tick++).padStart(2, '0')}.000Z`, mode, direction, payload });
const note = (method, params) => rec('watch', 'daemon->client', { method, params, emittedAtMs: 1 });
export function syntheticG2Transcript() {
  tick = 0;
  const lines = [
    rec('turn', 'handshake', 'HTTP/1.1 101 Switching Protocols\r\n'),
    rec('turn', 'client->daemon', { jsonrpc: '2.0', id: 1, method: 'turn/start', params: { threadId: TH, input: [{ type: 'text', text: DELIVERED }] } }),
    rec('turn', 'daemon->client', { id: 1, result: { turn: { id: TURN, items: [], itemsView: 'notLoaded', status: 'inProgress', error: null, startedAt: null, completedAt: null, durationMs: null } } }),
    note('turn/started', { threadId: TH, turn: { id: TURN, items: [], status: 'inProgress' } }),
    note('item/started', { item: userItem, ...ids }),
    note('item/completed', { item: userItem, ...ids }),
    note('item/started', { item: cmdItem(false), ...ids }),
    ...SYNTH.output.match(/[^\n]*\n/g).map((delta) => note('item/commandExecution/outputDelta', { ...ids, itemId: 'call_cmd1', delta })),
    note('item/completed', { item: cmdItem(true), ...ids }),
    note('item/started', { item: mcpItem(false), ...ids }),
    note('item/mcpToolCall/progress', { ...ids, itemId: 'call_mcp1', message: PROGRESS }),
    note('item/completed', { item: mcpItem(true), ...ids }),
    note('item/completed', { item: mcpFailed, ...ids }),
    note('item/completed', { item: fnItem, ...ids }),
    note('item/completed', { item: dynItem, ...ids }),
    note('item/completed', { item: webItem, ...ids }),
    note('item/completed', { item: fileItem, ...ids }),
    note('item/fileChange/patchUpdated', { ...ids, itemId: 'call_patch1', changes: [{ path: 'synthetic/edited.md', kind: { type: 'delete' }, diff: PATCH_DIFF }] }),
    note('turn/diff/updated', { ...ids, diff: TURN_DIFF }),
    note('item/completed', { item: hookPromptItem, ...ids }),
    note('hook/completed', { threadId: TH, turnId: TURN, run: { id: 'run_1', status: 'completed', eventName: 'stop', entries: [{ kind: 'stdout', text: HOOK_OUT }] } }),
    note('process/exited', { processHandle: 'proc_1', exitCode: 0, stdout: PROC_OUT, stderr: PROC_ERR, stdoutCapReached: false, stderrCapReached: false }),
    note('mcpServer/event/stream/notification', { subscriptionId: 'sub_1', notification: { method: 'synthetic/event', params: { detail: MCP_EVENT } } }),
    note('item/agentMessage/delta', { ...ids, itemId: 'msg_1', delta: ANSWER }),
    note('item/completed', { item: agentItem, ...ids }),
    note('item/completed', { item: longItem, ...ids }),
    note('turn/completed', { threadId: TH, turn: { id: TURN, items: turnItems, status: 'completed' } }),
    rec('turns', 'client->daemon', { jsonrpc: '2.0', id: 4, method: 'thread/turns/list', params: { threadId: TH } }),
    rec('turns', 'daemon->client', { id: 4, result: { data: [{ id: TURN, items: turnItems, status: 'completed', startedAt: 1, completedAt: 2 }], nextCursor: null } }),
    // An MCP tools/call result from an OAC spike server (G4 shape): OAC's own reply, kept.
    JSON.stringify({ t: '2026-10-05T02:07:00.000Z', pid: 1, surface: 'http', era: 'modern', direction: 'server->client', payload: { jsonrpc: '2.0', id: 9, result: { content: [{ type: 'text', text: 'OAC g4 synthetic echo: kept, scored by G4' }] } } }),
  ];
  return `${lines.join('\n')}\n`;
}
const TOOL_LINES = (text) => text.split('\n').map((l, i) => [l, i + 1]).filter(([l]) => /outputDelta|mcpToolCall|commandExecution|functionCallOutput|dynamicToolCall|webSearch|fileChange|turn\/diff|hookPrompt|hook\/completed|process\/exited|mcpServer\/event/.test(l)).map(([, n]) => n);

function wire(check) {
  const raw = syntheticG2Transcript();
  const r = createRedactor({ home: '/home/alice', username: 'alice', hostname: 'buildbox-7' });
  const { text, report, toolOutputBodies } = r.redactJsonl(raw);
  const rawLines = raw.split('\n');
  const outLines = text.split('\n');
  check('elide: one record per line, same line count (line numbers hold)', rawLines.length === outLines.length && outLines.filter(Boolean).every((l) => JSON.parse(l)), `${rawLines.length} ${outLines.length}`);
  const leaked = ALL_BODIES.filter((b) => text.includes(b) && b !== SHARED);
  check('elide: no tool-output body survives in the written transcript', leaked.length === 0, leaked.join(' | '));
  const count = (hay, needle) => hay.split(needle).length - 1;
  check('elide: a body the model quoted in its own answer survives only inside that answer', outLines.some((l) => l.includes(SHARED)) && outLines.every((l) => count(l, SHARED) === count(l, ANSWER)));
  check('elide: the aggregated file-read output becomes its marker (bytes and sha256 of the body)', text.includes(JSON.stringify(marker(SYNTH.output))) && text.includes(`"aggregatedOutput":${JSON.stringify(marker(SYNTH.output))}`));
  check('elide: each output delta becomes its own marker', SYNTH.output.match(/[^\n]*\n/g).every((d) => text.includes(`"delta":${JSON.stringify(marker(d))}`)));
  check('elide: MCP result text, structuredContent leaves, error message and progress message become markers; `type` tags stay', [SYNTH.mcpResult, STRUCT, ERR, PROGRESS].every((b) => text.includes(JSON.stringify(marker(b)))) && text.includes(`{"type":"text","text":${JSON.stringify(marker(SYNTH.mcpResult))}}`) && text.includes(`${JSON.stringify(marker('count', 'tool-output-key'))}:3`));
  check('elide: functionCallOutput, dynamicToolCall and webSearch bodies become markers', [FN_OUT, DYN_OUT, WEB_OUT].every((b) => text.includes(JSON.stringify(marker(b)))) && text.includes('"type":"input_text"') && text.includes('"type":"inputText"'));
  const parsed = outLines.filter(Boolean).map((l) => JSON.parse(l));
  const byMethod = (m) => parsed.filter((p) => p.payload?.method === m).map((p) => p.payload.params);
  const fc = parsed.find((p) => p.payload?.params?.item?.id === 'call_patch1').payload.params.item;
  check('review B1: a fileChange item\'s diff becomes a marker; its path and kind stay', fc.changes[0].diff === marker(DIFF) && fc.changes[0].path === 'synthetic/edited.md' && JSON.stringify(fc.changes[0].kind) === '{"type":"update","move_path":null}');
  const pu = byMethod('item/fileChange/patchUpdated')[0];
  check('review B1: item/fileChange/patchUpdated changes[].diff becomes a marker; path, kind and ids stay', pu.changes[0].diff === marker(PATCH_DIFF) && pu.changes[0].path === 'synthetic/edited.md' && pu.changes[0].kind.type === 'delete' && pu.itemId === 'call_patch1');
  check('review B1: turn/diff/updated diff becomes a marker', byMethod('turn/diff/updated')[0].diff === marker(TURN_DIFF));
  const hc = byMethod('hook/completed')[0];
  check('review N1: hook/completed run.entries[].text becomes a marker; kind, run id and status stay', hc.run.entries[0].text === marker(HOOK_OUT) && hc.run.entries[0].kind === 'stdout' && hc.run.id === 'run_1' && hc.run.status === 'completed');
  check('review N1: a hookPrompt item\'s fragments[].text becomes a marker; hookRunId stays', parsed.find((p) => p.payload?.params?.item?.id === 'hook_1').payload.params.item.fragments[0].text === marker(HOOK_PROMPT) && text.includes('"hookRunId":"run_1"'));
  const pe = byMethod('process/exited')[0];
  check('review N1: process/exited stdout and stderr become markers; exit code and handle stay', pe.stdout === marker(PROC_OUT) && pe.stderr === marker(PROC_ERR) && pe.exitCode === 0 && pe.processHandle === 'proc_1');
  check('review N1: mcpServer/event/stream/notification\'s notification is elided whole; subscription id stays', byMethod('mcpServer/event/stream/notification')[0].subscriptionId === 'sub_1' && !text.includes(MCP_EVENT) && !text.includes('synthetic/event'));
  check('review N2: a free-text key inside an elided body becomes a key marker; a number under it stays', !text.includes(FREE_KEY) && !text.includes('"note":') && text.includes(JSON.stringify(marker(FREE_KEY, 'tool-output-key'))) && text.includes(`${JSON.stringify(marker('count', 'tool-output-key'))}:3`));
  check('review N2: a `type` value that is not a schema enum tag becomes a marker; enum tags (text, input_text, inputText) stay', !text.includes(FREE_TYPE) && text.includes(`"type":${JSON.stringify(marker(FREE_TYPE))}`) && text.includes('{"type":"text","text":'));
  const done = parsed.find((p) => p.payload?.params?.item?.id === 'call_cmd1' && p.payload.method === 'item/completed').payload.params.item;
  check('elide: ids, method, status, exit code, duration, command line and read action are kept', done.id === 'call_cmd1' && done.status === 'completed' && done.exitCode === 0 && done.durationMs === 12 && done.command === SYNTH.command && JSON.stringify(done.commandActions) === JSON.stringify(cmdItem(true).commandActions));
  const mcp = parsed.find((p) => p.payload?.params?.item?.id === 'call_mcp1' && p.payload.method === 'item/completed').payload.params.item;
  check('elide: the MCP call keeps server, tool and arguments (the model\'s input)', mcp.server === SYNTH.server && mcp.tool === SYNTH.tool && JSON.stringify(mcp.arguments) === JSON.stringify(SYNTH.arguments));
  check('elide: the delivered text (turn/start input, userMessage) and the agent messages are byte-identical', outLines[1] === rawLines[1] && text.includes(JSON.stringify(DELIVERED)) && text.includes(JSON.stringify(ANSWER)) && text.includes(JSON.stringify(LONG_ANSWER)));
  check('elide: a record with no tool output is kept byte for byte', rawLines.every((l, i) => TOOL_LINES(raw).includes(i + 1) || /turn\/completed|thread\/turns|"id":4,"result"/.test(l) || l === outLines[i]));
  check('elide: an OAC server\'s own tools/call result (G4 shape) is kept', outLines.at(-2) === rawLines.at(-2) && outLines.at(-2).includes('OAC g4 synthetic echo'));
  check('elide: the turn record and thread/turns/list keep every item and type, bodies elided', parsed.filter((p) => p.payload?.params?.turn?.items?.length || p.payload?.result?.data).length === 2 && parsed.filter((p) => p.payload?.params?.turn?.items?.length || p.payload?.result?.data).every((p) => {
    const items = p.payload.params?.turn?.items ?? p.payload.result.data[0].items;
    return items.map((x) => x.type).join() === turnItems.map((x) => x.type).join() && items.find((x) => x.type === 'commandExecution').aggregatedOutput === marker(SYNTH.output);
  }));
  const ent = report.elidedToolOutputs;
  check('elide: the report lists each elision with its line, path, bytes and sha256, and never a body', ent.length > 10 && ent.every((e) => Number.isInteger(e.line) && /^\$\./.test(e.path) && Number.isInteger(e.bytes) && /^[0-9a-f]{64}$/.test(e.sha256) && !('body' in e)) && !ALL_BODIES.some((b) => JSON.stringify(report).includes(b)));
  check('elide: the report line numbers are the transcript\'s', ent.every((e) => outLines[e.line - 1].includes(`sha256=${e.sha256}`)));
  check('elide: the elided bodies are handed back (for the panes) outside the report', toolOutputBodies.includes(SYNTH.output) && toolOutputBodies.includes(SYNTH.mcpResult) && !('toolOutputBodies' in report));
  check('elide: the written transcript scans clean', reportIsClean(report), JSON.stringify(report.residualGenericHits));
  const again = r.redactJsonl(text);
  check('elide: a second pass is a no-op (markers are never re-elided)', again.text === text && again.report.elidedToolOutputs.length === 0);
  check('elide: the marker shape', ELIDED_RE.test(marker(SYNTH.output)) && !ELIDED_RE.test(`${marker(SYNTH.output)}x`));

  // Residual scan: structural, never by length.
  const rawScan = r.scan(raw).residualGenericHits.filter((h) => h.label === UNELIDED_LABEL).map((h) => h.line);
  const expectFlag = rawLines.map((l, i) => [l, i + 1]).filter(([l]) => l && unelided(l)).map(([, n]) => n);
  check('residual scan: flags every line still carrying a tool-output body, and only those', rawScan.length >= 10 && JSON.stringify(rawScan) === JSON.stringify(expectFlag), `${rawScan} vs ${expectFlag}`);
  check('residual scan: a long agent message (>4000 chars) is not tool output and is not flagged', LONG_ANSWER.length > 4000 && !rawScan.includes(rawLines.findIndex((l) => l.includes('"id":"msg_2"') && l.includes('item/completed')) + 1));
  check('residual scan: the elided transcript has no un-elided hit', !r.scan(text).residualGenericHits.some((h) => h.label === UNELIDED_LABEL));

  // Mutation baseline: the pre-#130 JSONL path (each record redacted by value, nothing
  // elided) leaves every body in place -- so the checks above are not vacuous -- and the
  // residual scan now refuses that output.
  const before = rawLines.map((l) => (l ? JSON.stringify(r.redactValue(JSON.parse(l)).value) : l)).join('\n');
  check('elide (mutation): the pre-#130 JSONL redaction leaves every tool-output body in the transcript', ALL_BODIES.every((b) => before.includes(b)));
  check('elide (mutation): the residual scan fails the pre-#130 output', !reportIsClean({ ...r.scan(before), hazardProtocolFrames: [] }));

  // The CLI (`redact.mjs --check`, used to re-check a committed fixture) flags it too.
  const dir = mkdtempSync(join(tmpdir(), 'oac-elide-'));
  try {
    const f = join(dir, 'raw.jsonl');
    writeFileSync(f, raw);
    const c = spawnSync(process.execPath, [REDACT_CLI, '--check', f], { encoding: 'utf8', timeout: 20000 });
    check('redact.mjs --check: exits 1 on an un-elided tool output, naming the label and never the body', c.status === 1 && c.stdout.includes(UNELIDED_LABEL) && !ALL_BODIES.some((b) => c.stdout.includes(b)), c.stdout.slice(0, 200));
    const o = join(dir, 'out.jsonl');
    const w = spawnSync(process.execPath, [REDACT_CLI, f, o, '--username', 'alice', '--hostname', 'buildbox-7', '--home', '/home/alice'], { encoding: 'utf8', timeout: 20000 });
    check('redact.mjs <in> <out>: elides, and the result checks clean', w.status === 0 && readFileSync(o, 'utf8') === text && spawnSync(process.execPath, [REDACT_CLI, '--check', o], { encoding: 'utf8', timeout: 20000 }).status === 0, w.stdout.slice(0, 200));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}
const unelided = (l) => elideToolOutputs(JSON.parse(l)).elided.length > 0;

// The criteria scorers read only kept fields: the facts they derive are identical before and
// after elision.
function scorers(check) {
  const raw = syntheticG2Transcript();
  const { text } = createRedactor({ home: '/home/alice', username: 'alice', hostname: 'buildbox-7' }).redactJsonl(raw);
  const a = g2Facts(parseG2Transcript(raw));
  const b = g2Facts(parseG2Transcript(text));
  check('scorers: g2Facts (criteria 1-2: thread lists, turn/start, queue/add, turn records, answers) is identical before and after elision', JSON.stringify(a) === JSON.stringify(b) && b.turnStarts[0]?.text === DELIVERED && b.turnsLists[0]?.turns[0]?.agentMessages.join() === [ANSWER, LONG_ANSWER].join() && b.events.agentMessages.length === 2);
  // G5's Codex transcript: spike records interleaved with wire records.
  const g5 = [
    JSON.stringify({ t: 't0', spike: 'delivery', case: 'X1', D: null, text: DELIVERED }),
    JSON.stringify({ t: 't1', direction: 'client->daemon', payload: { jsonrpc: '2.0', id: 1, method: 'turn/start', params: { threadId: TH, input: [{ type: 'text', text: DELIVERED }] } } }),
    JSON.stringify({ t: 't2', direction: 'daemon->client', payload: { id: 1, result: { turn: { id: TURN, status: 'inProgress' } } } }),
    JSON.stringify({ t: 't3', direction: 'client->daemon', payload: { jsonrpc: '2.0', id: 2, method: 'thread/turns/list', params: { threadId: TH } } }),
    JSON.stringify({ t: 't4', direction: 'daemon->client', payload: { id: 2, result: { data: [{ id: TURN, items: turnItems, status: 'completed' }] } } }),
    JSON.stringify({ t: 't5', spike: 'turns', threadId: TH, response: { id: 2, result: { data: [{ id: TURN, items: turnItems, status: 'completed' }] } } }),
  ].join('\n') + '\n';
  const g5out = createRedactor({ home: '/home/alice', username: 'alice', hostname: 'buildbox-7' }).redactJsonl(g5);
  const f = (t) => JSON.stringify(g5CodexFacts(parseJsonl(t), { question: 'q' }));
  check('scorers: g5CodexFacts (criteria 2 and 3-codex: deliveries, recorded text, answers) is identical before and after elision', f(g5) === f(g5out.text) && JSON.parse(f(g5out.text)).cases[0].recordedByteIdentical === true);
  check('scorers: a G5 `response` record (not JSON-RPC at the top) is elided too', g5out.report.elidedToolOutputs.some((e) => e.line === 6 && e.path.startsWith('$.response.')) && !ALL_BODIES.filter((x) => x !== SHARED).some((x) => g5out.text.includes(x)));
}

function pane(check) {
  const raw = syntheticG2Transcript();
  const r = createRedactor({ home: '/home/alice', username: 'alice', hostname: 'buildbox-7' });
  const j = r.redactJsonl(raw);
  const w = { bodies: j.toolOutputBodies, kept: keptStrings(j.text) };
  const out1 = SYNTH.output.split('\n');
  const paneText = [
    '› ' + DELIVERED,
    '',
    `• Ran ${SYNTH.command}`,
    `  └ ${out1[0]}`,
    `    ${out1[1].slice(0, 30)}…`,
    '    ok',
    `• ${ANSWER}`,
    `  ${SHARED}`,
    '  Worked for 5s • 9:06 PM',
    '',
  ].join('\n');
  const res = elidePaneLines(paneText, w);
  const lines = res.text.split('\n');
  check('pane: line count holds', lines.length === paneText.split('\n').length);
  check('pane: a line of a tool\'s output shown by the TUI is elided, its glyph prefix kept', lines[3] === `  └ ${marker(out1[0], 'tool-output-line')}`);
  check('pane: a TUI-truncated output line (trailing …) is elided', lines[4].startsWith('    <ELIDED tool-output-line '));
  check(`pane: a line shorter than ${PANE_MIN_LENGTH} characters is left`, lines[5] === '    ok');
  check('pane: the delivered message, the command line and the answer are kept', lines[0] === '› ' + DELIVERED && lines[2] === `• Ran ${SYNTH.command}` && lines[6] === `• ${ANSWER}`);
  check('pane: a line the transcript also keeps (the model quoted it) is kept', lines[7] === `  ${SHARED}`);
  check('pane: the report lists line numbers, bytes and sha256 only', JSON.stringify(res.elided.map((e) => e.line)) === '[4,5]' && res.elided.every((e) => !('body' in e)));
  // Mutations: without the wire signal nothing is elided; without the kept-text check the
  // quoted line would be.
  check('pane (mutation): with no wire bodies (no signal) the pane is left unchanged', elidePaneLines(paneText, { bodies: [], kept: [] }).text === paneText);
  check('pane (mutation): without the kept-text check the model-quoted line would be elided', elidePaneLines(paneText, { bodies: w.bodies, kept: [] }).text.split('\n')[7].includes('<ELIDED'));

  // run.mjs's capture loop: panes are elided from the wire whatever the capture order.
  const caps = [{ name: 'pane.txt', text: paneText, format: 'text' }, { name: 't.jsonl', text: raw, format: 'jsonl' }];
  const m = redactCaptures(r, caps);
  const p = m.get(caps[0]);
  check('redactCaptures: the pane is elided from the wire transcript even when it is captured first', p.text === res.text && p.report.elidedToolOutputLines.length === 2 && reportIsClean(p.report));
  check('redactCaptures: neither report carries a body', ![p.report, m.get(caps[1]).report].some((rep) => ALL_BODIES.some((b) => JSON.stringify(rep).includes(b))));
}

// The committed fixtures do not change: no committed transcript holds an app-server tool
// output, and no committed pane has a wire signal, so elision leaves every byte as it is.
function committed(check) {
  const files = [];
  const walk = (d) => {
    for (const e of readdirSync(d, { withFileTypes: true })) {
      if (e.isDirectory()) walk(join(d, e.name));
      else files.push(join(d, e.name));
    }
  };
  walk(FIXTURES);
  const jsonl = files.filter((f) => f.endsWith('.jsonl'));
  let records = 0;
  const changed = [];
  const bodiesByDir = new Map();
  for (const f of jsonl) {
    let lineNo = 0;
    for (const l of readFileSync(f, 'utf8').split('\n')) {
      lineNo += 1;
      if (!l.trim().startsWith('{')) continue;
      let v;
      try {
        v = JSON.parse(l);
      } catch {
        continue;
      }
      records += 1;
      const { elided } = elideToolOutputs(v);
      if (elided.length) changed.push({ f, n: lineNo, paths: elided.map((e) => e.path) });
      bodiesByDir.set(dirname(f), [...(bodiesByDir.get(dirname(f)) ?? []), ...elided.map((e) => e.body)]);
    }
  }
  // #297 NB3 elides mcpServer/startupStatus/updated `error`. Two committed D6 records (not
  // herdr captures, never re-captured) hold one: Codex's start-failure chain for OAC's own g4
  // spike server. They are the only committed records the elision list now touches, and only
  // that field: the server name and `failed` status, which carry the record's meaning, stay.
  const D6_STARTUP = ['d6-codex-protocol/transcript-conn1-2026-09-28.jsonl:42', 'd6-codex-protocol/transcript-conn2-2026-09-28.jsonl:16'];
  const rel = (f) => f.slice(FIXTURES.length + 1).replace(/\\/g, '/');
  check('committed fixtures: elision leaves every committed transcript record unchanged (G1, G2, G4, G5, D6) except the two D6 MCP startup errors (#297 NB3)', jsonl.length >= 20 && records > 1000 && JSON.stringify(changed.map((c) => `${rel(c.f)}:${c.n}`)) === JSON.stringify(D6_STARTUP) && changed.every((c) => c.paths.join() === '$.payload.params.error'), `${jsonl.length} files, ${records} records, changed: ${JSON.stringify(changed.map((c) => [rel(c.f), c.n, c.paths]))}`);
  check('committed fixtures: no G1, G2, G4 or G5 record (the gate scorers\' inputs) is touched', changed.every((c) => rel(c.f).startsWith('d6-codex-protocol/')));
  const scanner = createRedactor();
  const hitLines = (label) => jsonl.flatMap((f) => scanner.scan(readFileSync(f, 'utf8')).residualGenericHits.filter((h) => h.label === label).map((h) => `${rel(f)}:${h.line}`));
  const flagged = hitLines(UNELIDED_LABEL);
  check('committed fixtures: the residual scan flags no committed transcript as un-elided, but the two D6 MCP startup errors', JSON.stringify(flagged) === JSON.stringify(D6_STARTUP), flagged.join(', '));
  const panes = files.filter((f) => /pane-.*-herdr\.txt$/.test(f));
  const paneChanged = panes.filter((f) => {
    const t = readFileSync(f, 'utf8');
    return elidePaneLines(t, { bodies: bodiesByDir.get(dirname(f)) ?? [], kept: [] }).text !== t;
  });
  check('committed fixtures: every committed herdr pane capture (G1, G4, G5) is unchanged by pane elision', panes.length >= 5 && paneChanged.length === 0, paneChanged.join(', '));
  check('catalogue: the item types are the app-server v2 ThreadItem variants that carry tool, file or hook text, and agentMessage memory notes (#297 NB3)', Object.keys(ITEM_OUTPUT_FIELDS).sort().join() === 'agentMessage,commandExecution,dynamicToolCall,fileChange,functionCallOutput,hookPrompt,imageGeneration,mcpToolCall,webSearch' && ITEM_OUTPUT_FIELDS.agentMessage.join() === 'memoryCitation.entries.*.note');
  check('catalogue: the notifications include the file-diff, hook, process and MCP event carriers, the MCP startup error and the config-warning details', ['item/fileChange/patchUpdated', 'turn/diff/updated', 'hook/started', 'hook/completed', 'process/exited', 'mcpServer/event/stream/notification', 'mcpServer/startupStatus/updated', 'configWarning'].every((m) => Object.hasOwn(NOTIFICATION_OUTPUT_FIELDS, m)));
  const unrec = hitLines(UNRECOGNISED_LABEL);
  check('committed fixtures: the independent long-text scan, responses included, flags nothing in any committed transcript but the two D6 MCP startup errors (no over-flagging)', JSON.stringify(unrec) === JSON.stringify(D6_STARTUP), unrec.join(', '));
  // The response keep-list is not vacuous: the committed fixtures hold long cursors and an
  // OAC delivery echoed by thread/queue/add, which the scan would flag without it.
  let responseLong = 0;
  for (const f of jsonl) {
    for (const l of readFileSync(f, 'utf8').split('\n')) {
      if (!l.includes('"direction":"daemon->client"') || !l.includes('"result"')) continue;
      const p = JSON.parse(l).payload;
      const r = p?.method === undefined ? p.result : null;
      if (r && (['nextCursor', 'backwardsCursor', 'turnsBackwardsCursor', 'itemsBackwardsCursor'].some((k) => String(r[k] ?? '').length >= 120) || (r.queuedSubmission?.input ?? []).some((x) => String(x?.text ?? '').length >= 120))) responseLong += 1;
    }
  }
  check('committed fixtures: daemon responses holding 120+-character cursors or a queued delivery exist, so the response keep-list is exercised', responseLong >= 3, String(responseLong));
  // #299 NB-F: every JSON cursor Codex wrote into a committed fixture is a HistoryCursor (so
  // the allow-list keeps it), in at least two scope kinds.
  const cursors = [];
  for (const f of jsonl) {
    for (const l of readFileSync(f, 'utf8').split('\n')) {
      if (!l.includes('Cursor')) continue;
      const visit = (v) => {
        if (!v || typeof v !== 'object') return;
        for (const [k, x] of Object.entries(v)) {
          if (/Cursor$/.test(k) && typeof x === 'string' && x.startsWith('{')) cursors.push(x);
          else visit(x);
        }
      };
      visit(JSON.parse(l));
    }
  }
  const notKept = cursors.filter((s) => !isHistoryCursor(s));
  const kinds = new Set(cursors.map((s) => JSON.parse(s).scope?.kind));
  check('#299 NB-F: every JSON cursor in the committed fixtures is a HistoryCursor (kept), turns and item scopes both present', cursors.length >= 600 && notKept.length === 0 && kinds.has('turns') && kinds.has('itemsByCreatedAtOrdinal'), `${cursors.length} cursors, ${notKept.length} not kept, kinds ${[...kinds]}`);
}

// #130 review N3: the residual scan does not depend on the elision list alone.
function independent(check) {
  const long = (tag) => Array.from({ length: 4 }, (_, n) => line(tag, n + 1)).join('\n'); // > LONG_TEXT_MIN
  const r = createRedactor({ home: '/home/alice', username: 'alice', hostname: 'buildbox-7' });
  const frames = [
    // A thread item type no list knows (a future Codex), carrying a long tool body.
    note('item/completed', { item: { type: 'futureToolCall', id: 'call_future1', status: 'completed', output: long('future-item') }, ...ids }),
    // A notification no list knows, carrying a long body.
    note('item/futureTool/outputChunk', { ...ids, itemId: 'call_future1', chunk: long('future-notification') }),
    // A known item type with a long string in a field the elision list does not name.
    note('item/completed', { item: { ...cmdItem(true), aggregatedOutput: null, extraOutput: long('unlisted-field') }, ...ids }),
  ];
  for (const [i, f] of frames.entries()) {
    const { text, report } = r.redactJsonl(`${f}\n`);
    check(`independent scan ${i + 1}: a long body the elision list misses is not elided, but the scan flags it, so run.mjs withholds the capture`, report.elidedToolOutputs.length === 0 && report.residualGenericHits.some((h) => h.label === UNRECOGNISED_LABEL) && !report.residualGenericHits.some((h) => h.label === UNELIDED_LABEL) && !reportIsClean(report) && text.length > 0);
  }
  check('independent scan: names a JSON path, never the text', unrecognisedLongText(JSON.parse(frames[0])).join() === '$.payload.params.item.output');
  const raw = syntheticG2Transcript();
  const flagged = raw.split('\n').map((l, i) => (l && unrecognisedLongText(JSON.parse(l)).length ? i + 1 : null)).filter(Boolean);
  check('independent scan: it flags the raw synthetic transcript\'s long tool bodies by itself, without the elision list', flagged.length >= 4 && flagged.every((n) => TOOL_LINES(raw).includes(n)), JSON.stringify(flagged));
  const kept = [
    note('item/completed', { item: longItem, ...ids }),
    note('item/agentMessage/delta', { ...ids, itemId: 'msg_2', delta: LONG_ANSWER }),
    note('item/completed', { item: { ...userItem, content: [{ type: 'text', text: long('delivered-message'), text_elements: [] }] }, ...ids }),
    rec('turn', 'client->daemon', { jsonrpc: '2.0', id: 1, method: 'turn/start', params: { threadId: TH, input: [{ type: 'text', text: long('turn-start-input') }] } }),
    JSON.stringify({ t: 't', direction: 'server->client', payload: { jsonrpc: '2.0', method: 'notifications/claude/channel', params: { content: long('oac-channel-message'), meta: {} } } }),
    note('item/completed', { item: { type: 'reasoning', id: 'rs_1', summary: [long('reasoning-summary')], content: [] }, ...ids }),
  ];
  const keptHits = kept.filter((f) => unrecognisedLongText(JSON.parse(f)).length);
  check('independent scan: never flags the scored text (agent answers and deltas, delivered user messages, turn/start input, reasoning) or OAC\'s MCP channel notifications', keptHits.length === 0, JSON.stringify(keptHits.map((f) => unrecognisedLongText(JSON.parse(f)))));
}

// #130 re-review NB2: requests the Codex daemon sends its client (id + method, no "jsonrpc").
function serverRequests(check) {
  const r = createRedactor({ home: '/home/alice', username: 'alice', hostname: 'buildbox-7' });
  const ELICIT = [1, 2, 3].map((n) => line('elicitation-message', n)).join('\n');
  const SCHEMA_DESC = line('elicitation-schema-description', 1);
  const ADDED = [1, 2, 3].map((n) => line('patch-approval-added-file', n)).join('\n');
  const UDIFF = [1, 2, 3].map((n) => line('patch-approval-unified-diff', n)).join('\n');
  const REASON = [1, 2, 3].map((n) => line('approval-reason-by-the-model', n)).join(' ');
  const req = (id, method, params) => rec('watch', 'daemon->client', { id, method, params });
  const frames = [
    req(70, 'mcpServer/elicitation/request', { threadId: TH, turnId: TURN, serverName: SYNTH.server, mode: 'form', message: ELICIT, requestedSchema: { type: 'object', properties: { answer: { type: 'string', description: SCHEMA_DESC } } }, _meta: null }),
    req(71, 'applyPatchApproval', { callId: 'call_p', conversationId: TH, fileChanges: { 'synthetic/new.md': { type: 'add', content: ADDED }, 'synthetic/old.md': { type: 'update', unified_diff: UDIFF, move_path: null } }, reason: null, grantRoot: null }),
    req(72, 'item/commandExecution/requestApproval', { threadId: TH, turnId: TURN, itemId: 'call_cmd1', command: SYNTH.command, reason: REASON, startedAtMs: 1 }),
  ];
  const raw = `${frames.join('\n')}\n`;
  const { text, report } = r.redactJsonl(raw);
  const out = text.split('\n').filter(Boolean).map((l) => JSON.parse(l).payload);
  check('NB2: an MCP elicitation request\'s message and schema text become markers; id, method, server name and mode stay', out[0].id === 70 && out[0].method === 'mcpServer/elicitation/request' && out[0].params.message === marker(ELICIT) && out[0].params.serverName === SYNTH.server && out[0].params.mode === 'form' && !text.includes(SCHEMA_DESC));
  check('NB2: a patch approval\'s file contents and diffs become markers; the paths and change types stay', out[1].params.fileChanges['synthetic/new.md'].content === marker(ADDED) && out[1].params.fileChanges['synthetic/old.md'].unified_diff === marker(UDIFF) && out[1].params.fileChanges['synthetic/new.md'].type === 'add');
  check('NB2: an approval request\'s command and reason (model text) stay', out[2].params.reason === REASON && out[2].params.command === SYNTH.command);
  check('NB2: the elided requests scan clean', reportIsClean(report), JSON.stringify(report.residualGenericHits));
  // The independent scan covers daemon requests, known and unknown.
  const unknown = req(73, 'item/futureTool/requestSomething', { threadId: TH, payload: long('future-request') });
  const u = r.redactJsonl(`${unknown}\n`);
  check('NB2: a long string in an unknown daemon request is flagged by the independent scan (capture withheld)', u.report.residualGenericHits.some((h) => h.label === UNRECOGNISED_LABEL) && !reportIsClean(u.report));
  check('NB2: the scan sees an un-elided elicitation message in a raw request', unrecognisedLongText(JSON.parse(frames[0])).includes('$.payload.params.message'));
  // OAC's own requests (jsonrpc 2.0: turn/start, MCP tools/call) are not daemon requests.
  const own = [
    rec('turn', 'client->daemon', { jsonrpc: '2.0', id: 5, method: 'turn/start', params: { threadId: TH, input: [{ type: 'text', text: long('oac-delivered') }] } }),
    JSON.stringify({ t: 't', direction: 'client->server', payload: { jsonrpc: '2.0', id: 6, method: 'tools/call', params: { name: 'g4_relay', arguments: { text: long('oac-tool-argument') } } } }),
  ];
  check('NB2: OAC\'s own jsonrpc 2.0 requests (turn/start, MCP tools/call) are neither elided nor flagged', own.every((f) => elideToolOutputs(JSON.parse(f)).elided.length === 0 && unrecognisedLongText(JSON.parse(f)).length === 0));
}
const long = (tag) => Array.from({ length: 4 }, (_, n) => line(tag, n + 1)).join('\n');

// #130 (G2 run 20261005T041011Z-bb584c, transcript line 48): harness-authored instruction
// text in a daemon->client RESPONSE. Every body here is synthetic.
function responses(check) {
  const r = createRedactor({ home: '/home/alice', username: 'alice', hostname: 'buildbox-7' });
  const DEV = long('collaboration-mode-developer-instructions');
  const CFG_DEV = long('config-developer-instructions');
  const CFG_INSTR = long('config-instructions');
  const CFG_COMPACT = long('config-compact-prompt');
  const REQ_DEV = long('requirements-additional-developer-instructions');
  const DETAILS = long('turn-error-additional-details');
  const EXPLAIN = long('misalignment-detailed-explanation');
  const STEER = line('misalignment-steer-message', 1);
  const PREVIEW = long('operator-prompt-preview');
  const CURSOR = JSON.stringify({ requestedThreadId: TH, rolloutOrdinal: 1, includeAnchor: true, scope: { kind: 'turns' } }); // 125 characters: scanned
  const thread = { id: TH, preview: PREVIEW, cwd: '/home/alice/synthetic-project', environments: [{ environmentId: 'local', cwd: '/home/alice/synthetic-project' }], status: { type: 'idle' }, turns: [] };
  const resume = rec('watch', 'daemon->client', { id: 1, result: { thread, model: 'synthetic-model', cwd: '/home/alice/synthetic-project', collaborationMode: { mode: 'default', settings: { model: 'synthetic-model', reasoning_effort: 'high', developer_instructions: DEV } }, turnsBackwardsCursor: CURSOR, itemsBackwardsCursor: CURSOR } });
  const config = rec('watch', 'daemon->client', { id: 2, result: { config: { model: 'synthetic-model', developer_instructions: CFG_DEV, instructions: CFG_INSTR, compact_prompt: CFG_COMPACT }, origins: {}, layers: [] } });
  const reqs = rec('watch', 'daemon->client', { id: 3, result: { requirements: { additionalDeveloperInstructions: REQ_DEV, logDir: null } } });
  const turnErr = { message: 'synthetic turn failed', additionalDetails: DETAILS, misalignment: { errorType: 'synthetic', detailedExplanation: EXPLAIN, steer: { message: STEER } } };
  const turnStart = rec('turn', 'daemon->client', { id: 4, result: { turn: { id: TURN, items: [agentItem], status: 'failed', error: turnErr } } });
  const raw = `${[resume, config, reqs, turnStart].join('\n')}\n`;
  const { text, report } = r.redactJsonl(raw);
  const out = text.split('\n').filter(Boolean).map((l) => JSON.parse(l).payload.result);
  const harness = (s) => marker(s, 'harness-text');
  check('#130 responses: thread/resume collaborationMode developer_instructions becomes a harness-text marker; mode, model and reasoning effort stay', out[0].collaborationMode.settings.developer_instructions === harness(DEV) && out[0].collaborationMode.mode === 'default' && out[0].collaborationMode.settings.reasoning_effort === 'high' && out[0].model === 'synthetic-model');
  check('#130 responses: the scored and client-written fields of a response stay (thread preview, cwds, opaque cursors)', out[0].thread.preview === PREVIEW && out[0].thread.environments[0].cwd === '<USER_HOME>/synthetic-project' && out[0].turnsBackwardsCursor === CURSOR);
  check('#130 responses: config/read developer_instructions, instructions and compact_prompt, and configRequirements additionalDeveloperInstructions, become markers', out[1].config.developer_instructions === harness(CFG_DEV) && out[1].config.instructions === harness(CFG_INSTR) && out[1].config.compact_prompt === harness(CFG_COMPACT) && out[2].requirements.additionalDeveloperInstructions === harness(REQ_DEV) && out[2].requirements.logDir === null);
  const te = out[3].turn.error;
  check('#297 NB3: a turn error\'s additionalDetails, misalignment explanation and steer text become markers; its message and errorType stay; the turn\'s items are untouched', te.additionalDetails === harness(DETAILS) && te.misalignment.detailedExplanation === harness(EXPLAIN) && te.misalignment.steer.message === harness(STEER) && te.message === 'synthetic turn failed' && te.misalignment.errorType === 'synthetic' && JSON.stringify(out[3].turn.items) === JSON.stringify([agentItem]));
  check('#130 responses: the elided responses scan clean', reportIsClean(report), JSON.stringify(report.residualGenericHits));
  check('#130 responses: the report lists each harness-text elision by line and path, never a body', report.elidedToolOutputs.some((e) => e.line === 1 && e.path === '$.payload.result.collaborationMode.settings.developer_instructions') && ![DEV, CFG_DEV, DETAILS].some((b) => JSON.stringify(report).includes(b)));
  check('#130 responses: the marker shape covers harness-text', ELIDED_RE.test(harness(DEV)));
  // The independent scan covers responses (keyed on direction), without the elision list.
  check('#130 responses: the raw thread/resume response is flagged by both residual labels', ((h) => h.includes(UNELIDED_LABEL) && h.includes(UNRECOGNISED_LABEL))(r.scan(resume).residualGenericHits.map((x) => x.label)));
  check('#130 responses: the independent scan names the response path', unrecognisedLongText(JSON.parse(resume)).join() === '$.payload.result.collaborationMode.settings.developer_instructions');
  const unknownField = rec('watch', 'daemon->client', { id: 5, result: { thread, futureHarnessText: long('future-response-field') } });
  const u = r.redactJsonl(`${unknownField}\n`);
  check('#130 responses: a long string in a response field no list names is flagged (capture withheld)', u.report.elidedToolOutputs.length === 0 && u.report.residualGenericHits.some((h) => h.label === UNRECOGNISED_LABEL) && !reportIsClean(u.report));
  const proseCursor = rec('watch', 'daemon->client', { id: 6, result: { data: [], nextCursor: long('prose-in-a-cursor') } });
  check('#130 responses: prose in a cursor is flagged', unrecognisedLongText(JSON.parse(proseCursor)).join() === '$.payload.result.nextCursor');
  // #298 review NB-A, #299 NB-F: a cursor is kept only in the shape Codex writes, the compact
  // serde_json form of HistoryCursor at rust-v0.160.0, never because it lacks whitespace.
  const noSpace = Array.from({ length: 24 }, (_, i) => `synthetic_word_${i}`).join('_'); // > 120, no whitespace
  const cursorCase = (v) => unrecognisedLongText(JSON.parse(rec('watch', 'daemon->client', { id: 10, result: { data: [], nextCursor: v } }))).join();
  const FLAG = '$.payload.result.nextCursor';
  check('#298 NB-A: an underscore-joined or percent-encoded prose cursor (no whitespace) is flagged', cursorCase(noSpace) === FLAG && cursorCase(encodeURIComponent(long('percent-encoded-prose'))) === FLAG && !/\s/.test(noSpace));
  // Codex's shape, every scope kind and both anchors, each at least LONG_TEXT_MIN long (so
  // scanned): kept. isHistoryCursor agrees.
  const hc = (o = {}) => ({ requestedThreadId: TH, rolloutOrdinal: 1, includeAnchor: true, scope: { kind: 'turns' }, ...o });
  const codexShapes = [hc(), hc({ includeAnchor: false, rolloutOrdinal: 1234567 }), hc({ scope: { kind: 'itemsByCreatedAtOrdinal' } }), hc({ rolloutOrdinal: Number.MAX_SAFE_INTEGER, includeAnchor: false, scope: { kind: 'itemsByUpdatedAtOrdinal' } })].map((o) => JSON.stringify(o));
  check('#299 NB-F: the HistoryCursor shape (requestedThreadId, rolloutOrdinal, includeAnchor, scope.kind) is kept for every CursorScope kind', codexShapes.every((s) => s.length >= 120 && cursorCase(s) === '' && isHistoryCursor(s)) && cursorCase(CURSOR) === '', codexShapes.map((s) => `${s.length}:${cursorCase(s)}`).join());
  // One case per rule of isHistoryCursor; each is scanned (>= 120 characters) and would be
  // kept if that rule were dropped.
  const prose = noSpace.slice(0, 100);
  const dupKey = JSON.stringify(hc()).replace('{', `{"requestedThreadId":"${prose}",`); // JSON.parse keeps the last
  const flagCases = {
    'an extra key': JSON.stringify({ ...hc(), note: prose }),
    'a long key (#299 NB-E)': JSON.stringify({ requestedThreadId: TH, [noSpace.slice(0, 200)]: 1 }),
    'keys of a different object': JSON.stringify(Object.fromEntries(Array.from({ length: 40 }, (_, i) => [`k${i}`, 'v'.repeat(12)]))),
    'a requestedThreadId that is not a UUID': JSON.stringify(hc({ requestedThreadId: prose })),
    'a rolloutOrdinal that is not an integer': JSON.stringify(hc({ rolloutOrdinal: prose })),
    'a negative rolloutOrdinal (u64)': JSON.stringify(hc({ rolloutOrdinal: -1234567 })),
    'an includeAnchor that is not a boolean': JSON.stringify(hc({ includeAnchor: prose })),
    'a scope.kind that is not a CursorScope variant': JSON.stringify(hc({ scope: { kind: prose } })),
    'an extra key in scope': JSON.stringify(hc({ scope: { kind: 'turns', note: prose } })),
    'padding (not compact)': JSON.stringify(hc(), null, 1),
    'a duplicated key hiding text': dupKey,
  };
  const missed = Object.entries(flagCases).filter(([, s]) => !(s.length >= 120 && cursorCase(s) === FLAG && !isHistoryCursor(s))).map(([k, s]) => `${k} (${s.length})`);
  check('#299 NB-F: a JSON cursor off the HistoryCursor shape is flagged: an extra or long key, a non-UUID thread id, a non-u64 ordinal, a non-boolean anchor, an unknown scope kind or scope key, padding, a duplicated key', missed.length === 0 && JSON.parse(dupKey).requestedThreadId === TH, missed.join(', '));
  const unlistedToken = rec('watch', 'daemon->client', { id: 11, result: { thread, futureOpaqueField: noSpace } });
  check('#298 NB-A: a long string with no whitespace in a response field no list names is flagged', unrecognisedLongText(JSON.parse(unlistedToken)).join() === '$.payload.result.futureOpaqueField' && !reportIsClean(r.redactJsonl(`${unlistedToken}\n`).report), JSON.stringify(unrecognisedLongText(JSON.parse(unlistedToken))));
  // #298 review NB-B: key-name elision skips only real ThreadItem types. A ConfigLayerSource
  // (string id and type, like an item) is searched; a thread item's tool arguments (the
  // model's input, scored) are not touched even when a key matches.
  const SHORT_DEV = 'synthetic short layer developer text'; // < 120: only the elision list catches it
  const layer = rec('watch', 'daemon->client', { id: 12, result: { config: {}, origins: {}, layers: [{ name: { type: 'enterpriseManaged', id: 'synthetic-layer', name: 'synthetic', developer_instructions: SHORT_DEV }, version: '1' }] } });
  const lo = r.redactJsonl(`${layer}\n`);
  check('#298 NB-B: developer_instructions under a ConfigLayerSource-shaped object (string id and type) is elided', JSON.parse(lo.text).payload.result.layers[0].name.developer_instructions === harness(SHORT_DEV) && JSON.parse(lo.text).payload.result.layers[0].name.id === 'synthetic-layer' && reportIsClean(lo.report));
  const argItem = { type: 'mcpToolCall', id: 'call_args', server: SYNTH.server, tool: SYNTH.tool, status: 'inProgress', arguments: { instructions: 'synthetic model-written tool argument', steer: 'left' }, result: null, error: null };
  const itemFrames = [note('item/started', { item: argItem, ...ids }), rec('turns', 'daemon->client', { id: 13, result: { data: [{ id: TURN, items: [argItem], status: 'inProgress' }] } })];
  check('#298 NB-B: a thread item\'s tool arguments are not elided by key name (notification and response)', itemFrames.every((f) => r.redactJsonl(`${f}\n`).text.trim() === f && elideToolOutputs(JSON.parse(f)).elided.length === 0));
  const jsonErr = rec('watch', 'daemon->client', { id: 7, error: { code: -32600, message: long('jsonrpc-error-message'), data: { detail: long('jsonrpc-error-data') } } });
  check('#130 responses: a JSON-RPC error keeps its message (the scenarios report it); its data is flagged', unrecognisedLongText(JSON.parse(jsonErr)).join() === '$.payload.error.data.detail');
  // A G5 spike record copying a daemon response is covered too.
  const g5 = JSON.stringify({ t: 't', spike: 'turns', threadId: TH, response: { id: 8, result: { collaborationMode: { settings: { developer_instructions: DEV } } } } });
  const g5out = r.redactJsonl(`${g5}\n`);
  check('#130 responses: a G5 spike `response` record\'s instruction text is elided and scans clean', !g5out.text.includes(DEV) && reportIsClean(g5out.report) && unrecognisedLongText(JSON.parse(g5)).length === 1);
  // Not the daemon's: OAC's own requests, and an OAC MCP server's own `instructions`.
  const own = [
    rec('turn', 'client->daemon', { jsonrpc: '2.0', id: 9, method: 'thread/start', params: { cwd: '/x', developerInstructions: long('oac-client-developer-instructions') } }),
    JSON.stringify({ t: 't', direction: 'server->client', payload: { jsonrpc: '2.0', id: 0, result: { protocolVersion: '2025-06-18', serverInfo: { name: 'g1spike' }, instructions: long('oac-mcp-server-instructions') } } }),
  ];
  check('#130 responses: OAC\'s own thread/start params and an OAC MCP server\'s initialize instructions are neither elided nor flagged (not daemon->client)', own.every((f) => elideToolOutputs(JSON.parse(f)).elided.length === 0 && unrecognisedLongText(JSON.parse(f)).length === 0));
  // The G2 scorers read nothing elided here.
  const g2t = `${[rec('watch', 'client->daemon', { jsonrpc: '2.0', id: 1, method: 'thread/resume', params: { threadId: TH } }), resume].join('\n')}\n`;
  check('#130 responses: g2Facts (the thread/resume record G2 reads) is identical before and after', JSON.stringify(g2Facts(parseG2Transcript(g2t))) === JSON.stringify(g2Facts(parseG2Transcript(r.redactJsonl(g2t).text))) && g2Facts(parseG2Transcript(g2t)).resumes[0]?.status === 'idle');
}

// #297: NB5 (daemon requests keyed on direction), NB6 (elicitation url), NB3 (notifications).
function nb297(check) {
  const r = createRedactor({ home: '/home/alice', username: 'alice', hostname: 'buildbox-7' });
  const ELICIT = long('elicitation-message-jsonrpc');
  const withJsonrpc = rec('watch', 'daemon->client', { jsonrpc: '2.0', id: 80, method: 'mcpServer/elicitation/request', params: { threadId: TH, serverName: SYNTH.server, mode: 'form', message: ELICIT, requestedSchema: { type: 'object', properties: {} }, _meta: null } });
  const a = r.redactJsonl(`${withJsonrpc}\n`);
  check('#297 NB5: a daemon request carrying "jsonrpc":"2.0" is still elided (keyed on direction)', !a.text.includes(ELICIT) && JSON.parse(a.text).payload.params.message === marker(ELICIT) && reportIsClean(a.report));
  check('#297 NB5: and still scanned', unrecognisedLongText(JSON.parse(withJsonrpc)).join() === '$.payload.params.message');
  const clientNoJsonrpc = rec('turn', 'client->daemon', { id: 81, method: 'mcpServer/elicitation/request', params: { message: long('client-side-text') } });
  check('#297 NB5: a client->daemon request without "jsonrpc" is not taken for a daemon request', elideToolOutputs(JSON.parse(clientNoJsonrpc)).elided.length === 0 && unrecognisedLongText(JSON.parse(clientNoJsonrpc)).length === 0);
  const URL = `https://example.invalid/oauth/authorize?${'q=synthetic&'.repeat(14)}end`;
  const urlMode = rec('watch', 'daemon->client', { id: 82, method: 'mcpServer/elicitation/request', params: { threadId: TH, serverName: SYNTH.server, mode: 'url', elicitationId: 'el_1', message: 'Synthetic: open the link', url: URL, _meta: null } });
  const b = r.redactJsonl(`${urlMode}\n`);
  const bp = JSON.parse(b.text).payload.params;
  check('#297 NB6: a url-mode elicitation url (120+ characters) becomes a marker and the capture is publishable; mode and elicitation id stay', URL.length >= 120 && bp.url === marker(URL) && bp.mode === 'url' && bp.elicitationId === 'el_1' && reportIsClean(b.report), JSON.stringify(b.report.residualGenericHits));
  const STARTUP = long('mcp-startup-error-chain');
  const CFG_DETAILS = line('config-warning-details', 1);
  const NOTE = line('memory-citation-note', 1);
  const ERR_DETAILS = line('error-additional-details', 1);
  const TURN_DETAILS = line('turn-completed-error-details', 1);
  const frames = [
    note('mcpServer/startupStatus/updated', { threadId: TH, name: 'synthetic_server', status: 'failed', error: STARTUP }),
    note('configWarning', { summary: 'Synthetic config warning', details: CFG_DETAILS, path: '/home/alice/.codex/config.toml' }),
    note('item/completed', { item: { ...agentItem, memoryCitation: { entries: [{ path: 'memories/synthetic.md', lineStart: 1, lineEnd: 2, note: NOTE }], threadIds: [TH] } }, ...ids }),
    note('error', { threadId: TH, turnId: TURN, error: { message: 'synthetic stream error', additionalDetails: ERR_DETAILS } }),
    note('turn/completed', { threadId: TH, turn: { id: TURN, items: [], status: 'failed', error: { message: 'synthetic turn error', additionalDetails: TURN_DETAILS } } }),
  ];
  const c = r.redactJsonl(`${frames.join('\n')}\n`);
  const o = c.text.split('\n').filter(Boolean).map((l) => JSON.parse(l).payload.params);
  check('#297 NB3: the MCP startup error becomes a marker; server name and status stay', o[0].error === marker(STARTUP) && o[0].name === 'synthetic_server' && o[0].status === 'failed');
  check('#297 NB3: configWarning details become a marker; summary and path stay', o[1].details === marker(CFG_DETAILS) && o[1].summary === 'Synthetic config warning' && o[1].path === '<USER_HOME>/.codex/config.toml');
  check('#297 NB3: a memory citation note becomes a marker; its path, lines, thread ids and the answer stay', o[2].item.memoryCitation.entries[0].note === marker(NOTE) && o[2].item.memoryCitation.entries[0].path === 'memories/synthetic.md' && o[2].item.memoryCitation.threadIds[0] === TH && o[2].item.text === ANSWER);
  check('#297 NB3: the error notification\'s and a turn\'s additionalDetails become markers; their messages stay', o[3].error.additionalDetails === marker(ERR_DETAILS, 'harness-text') && o[3].error.message === 'synthetic stream error' && o[4].turn.error.additionalDetails === marker(TURN_DETAILS, 'harness-text') && o[4].turn.error.message === 'synthetic turn error');
  check('#297 NB3: the elided notifications scan clean', reportIsClean(c.report), JSON.stringify(c.report.residualGenericHits));
  check('#297 NB3: a raw MCP startup error is flagged by the independent scan (no longer on a keep-list)', unrecognisedLongText(JSON.parse(frames[0])).join() === '$.payload.params.error');
}

export function elideUnit(check) {
  responses(check);
  nb297(check);
  serverRequests(check);
  wire(check);
  scorers(check);
  pane(check);
  independent(check);
  committed(check);
}
