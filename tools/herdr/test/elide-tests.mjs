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

import { createRedactor, reportIsClean, UNELIDED_LABEL } from '../lib/redact.mjs';
import { ELIDED_RE, ITEM_OUTPUT_FIELDS, PANE_MIN_LENGTH, elideToolOutputs, elidePaneLines, keptStrings, redactCaptures } from '../lib/elide.mjs';
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
const ALL_BODIES = [...SYNTH.output.split('\n').filter(Boolean), ...SYNTH.mcpResult.split('\n'), SHARED, FN_OUT, DYN_OUT, WEB_OUT, PROGRESS, STRUCT, ERR];

const DELIVERED = 'G2 synthetic delivery (from a second daemon client, not typed in this TUI): reply with exactly OAC G2 SYNTHETIC';
const ANSWER = `OAC G2 SYNTHETIC. Quoting: ${SHARED}`;
const LONG_ANSWER = Array.from({ length: 120 }, (_, i) => `${i + 1}. a long model-authored answer line`).join('\n'); // > 4000 chars
const TH = '01a00000-0000-7000-8000-000000000001';
const TURN = '01a00000-0000-7000-8000-000000000002';
const ids = { threadId: TH, turnId: TURN };
const cmdItem = (done) => ({ type: 'commandExecution', id: 'call_cmd1', command: SYNTH.command, cwd: null, processId: null, source: 'agent', status: done ? 'completed' : 'inProgress', commandActions: [{ type: 'read', command: SYNTH.command, name: SYNTH.name, path: SYNTH.path }], aggregatedOutput: done ? SYNTH.output : null, exitCode: done ? 0 : null, durationMs: done ? 12 : null });
const mcpItem = (done) => ({ type: 'mcpToolCall', id: 'call_mcp1', server: SYNTH.server, tool: SYNTH.tool, status: done ? 'completed' : 'inProgress', arguments: SYNTH.arguments, result: done ? { content: [{ type: 'text', text: SYNTH.mcpResult }, { type: 'text', text: SHARED }], structuredContent: { note: STRUCT, count: 3 }, _meta: null } : null, error: null, durationMs: done ? 30 : null });
const mcpFailed = { type: 'mcpToolCall', id: 'call_mcp2', server: SYNTH.server, tool: SYNTH.tool, status: 'failed', arguments: {}, result: null, error: { message: ERR }, durationMs: 3 };
const fnItem = { type: 'functionCallOutput', id: 'call_fn1', name: 'synthetic_fn', namespace: null, output: [{ type: 'input_text', text: FN_OUT }] };
const dynItem = { type: 'dynamicToolCall', id: 'call_dyn1', tool: 'synthetic_dyn', namespace: null, arguments: {}, status: 'completed', success: true, contentItems: [{ type: 'inputText', text: DYN_OUT }], durationMs: 5 };
const webItem = { type: 'webSearch', id: 'ws_1', query: 'synthetic query', action: { type: 'search', query: 'synthetic query' }, results: [{ title: WEB_OUT, url: 'https://example.invalid/x' }] };
const userItem = { type: 'userMessage', id: 'u1', clientId: null, content: [{ type: 'text', text: DELIVERED, text_elements: [] }] };
const agentItem = { type: 'agentMessage', id: 'msg_1', text: ANSWER, phase: 'final_answer' };
const longItem = { type: 'agentMessage', id: 'msg_2', text: LONG_ANSWER, phase: 'final_answer' };
const turnItems = [userItem, cmdItem(true), mcpItem(true), mcpFailed, fnItem, dynItem, webItem, agentItem, longItem];

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
const TOOL_LINES = (text) => text.split('\n').map((l, i) => [l, i + 1]).filter(([l]) => /outputDelta|mcpToolCall|commandExecution|functionCallOutput|dynamicToolCall|webSearch/.test(l)).map(([, n]) => n);

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
  check('elide: MCP result text, structuredContent leaves, error message and progress message become markers; `type` tags stay', [SYNTH.mcpResult, STRUCT, ERR, PROGRESS].every((b) => text.includes(JSON.stringify(marker(b)))) && text.includes(`{"type":"text","text":${JSON.stringify(marker(SYNTH.mcpResult))}}`) && text.includes('"count":3'));
  check('elide: functionCallOutput, dynamicToolCall and webSearch bodies become markers', [FN_OUT, DYN_OUT, WEB_OUT].every((b) => text.includes(JSON.stringify(marker(b)))) && text.includes('"type":"input_text"') && text.includes('"type":"inputText"'));
  const parsed = outLines.filter(Boolean).map((l) => JSON.parse(l));
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
    for (const l of readFileSync(f, 'utf8').split('\n')) {
      if (!l.trim().startsWith('{')) continue;
      let v;
      try {
        v = JSON.parse(l);
      } catch {
        continue;
      }
      records += 1;
      const { elided } = elideToolOutputs(v);
      if (elided.length) changed.push(f);
      bodiesByDir.set(dirname(f), [...(bodiesByDir.get(dirname(f)) ?? []), ...elided.map((e) => e.body)]);
    }
  }
  check('committed fixtures: elision leaves every committed transcript record unchanged (G1, G2, G4, G5, D6)', jsonl.length >= 20 && records > 1000 && changed.length === 0, `${jsonl.length} files, ${records} records, changed: ${[...new Set(changed)].join(', ')}`);
  const scanner = createRedactor();
  const flagged = jsonl.filter((f) => scanner.scan(readFileSync(f, 'utf8')).residualGenericHits.some((h) => h.label === UNELIDED_LABEL));
  check('committed fixtures: the residual scan flags no committed transcript as un-elided', flagged.length === 0, flagged.join(', '));
  const panes = files.filter((f) => /pane-.*-herdr\.txt$/.test(f));
  const paneChanged = panes.filter((f) => {
    const t = readFileSync(f, 'utf8');
    return elidePaneLines(t, { bodies: bodiesByDir.get(dirname(f)) ?? [], kept: [] }).text !== t;
  });
  check('committed fixtures: every committed herdr pane capture (G1, G4, G5) is unchanged by pane elision', panes.length >= 5 && paneChanged.length === 0, paneChanged.join(', '));
  check('catalogue: the item types are the app-server v2 ThreadItem variants that carry tool output', Object.keys(ITEM_OUTPUT_FIELDS).sort().join() === 'commandExecution,dynamicToolCall,functionCallOutput,imageGeneration,mcpToolCall,webSearch');
}

export function elideUnit(check) {
  wire(check);
  scorers(check);
  pane(check);
  committed(check);
}
