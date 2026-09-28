// K4 (#127) tests, run by the driver self-test (`node tools/herdr/run.mjs --self-test`).
//
// Unit half: tools/herdr/lib/compare-transcripts.mjs against the REAL committed G1
// transcripts (Box C, Box B, the original run) and synthetic variants of Box C with known
// differences; the Claude Code pin-move check; lib/g1.mjs (dialog recognition against Box
// C's recorded dialog text, mid-turn windows, pane sections, server staging); and the
// scoring rules of lib/g1-report.mjs.
//
// Lifecycle half: tools/herdr/scenarios/g1-claude-wake.mjs end to end through run.mjs
// against test/fake-herdr.mjs and test/fake-claude.mjs, both TEST DOUBLES. These prove the
// scenario's and the driver's own behavior (staging, ordering, the accept-origin rule,
// version stops, captures, redaction, teardown). They prove nothing about herdr or Claude
// Code: a live G1 run through herdr is UNVERIFIED until an operator runs it.

import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { compareTranscripts, diffSequences, formatDiff, parseTranscript, selectSegment, transcriptFacts, normalizeEntries, TranscriptError, isLegacyRevision } from '../lib/compare-transcripts.mjs';
import { parseClaudeLastObserved, parseClaudeCliVersion, claudePinMoveTrigger } from '../lib/pins.mjs';
import {
  BOX_C_TRANSCRIPT, COMMITTED_SERVER, FIXTURE_DIR, G1_LAUNCH, classifyScreen, driverMayAccept, dialogMatchesBoxC, formatSection, parseSections,
  fixtureNames, stageServerCopy, verifyServerCopy, sha256, midTurnWindow,
} from '../lib/g1.mjs';
import { evaluateG1, parseOperatorScores, SCORES, ReportError } from '../lib/g1-report.mjs';
import { assertNotInjected, DEFAULT_PROMPTS } from '../scenarios/g1-claude-wake.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const FIX = (f) => join(REPO, FIXTURE_DIR, f);
const read = (p) => readFileSync(p, 'utf8');
const BOX_C = read(join(REPO, BOX_C_TRANSCRIPT));
const BOX_B = read(FIX('transcript-2026-09-28-2.1.283.jsonl'));
const ORIGINAL = read(FIX('transcript.jsonl'));
const toJsonl = (entries) => `${entries.map((e) => JSON.stringify({ t: e.t, direction: e.direction, payload: e.payload })).join('\n')}\n`;
const throws = (fn, cls, re) => {
  try {
    fn();
    return false;
  } catch (e) {
    return (!cls || e instanceof cls) && (!re || re.test(e.message));
  }
};

// Box C's last server instance, rewritten the way a different run would differ by
// construction: other wall-clock times, JSON-RPC ids, tool-use id, progress token and pid.
function boxCAsAnotherRun({ shiftMs = 86400000 + 12345 } = {}) {
  return selectSegment(parseTranscript(BOX_C), 'last').map((e) => {
    const p = JSON.parse(JSON.stringify(e.payload));
    if (typeof p.id === 'number') p.id += 40;
    if (typeof p.id === 'string') p.id = `${p.id}-other`;
    if (p.pid) p.pid = 99999;
    if (p.params?._meta?.['claudecode/toolUseId']) p.params._meta['claudecode/toolUseId'] = 'toolu_0000000000000000otherrun';
    if (p.params?._meta?.progressToken !== undefined) p.params._meta.progressToken = 77;
    return { t: new Date(Date.parse(e.t) + shiftMs).toISOString(), direction: e.direction, payload: p };
  });
}

// Box C's dialog, as quoted in G1-result.md criterion 5 (blockquote lines; `\>` is the
// selection marker).
function boxCDialogFromResult() {
  const lines = read(join(REPO, 'docs', 'planning', 'gates', 'G1-result.md')).split('\n');
  const start = lines.findIndex((l) => /^\s*> WARNING: Loading development channels/.test(l));
  const end = lines.findIndex((l, i) => i > start && /^\s*> Enter to confirm/.test(l));
  return lines.slice(start, end + 1).map((l) => l.replace(/^\s*> ?/, '').replace(/^\\>/, '❯')).join('\n');
}

export function g1Unit(check) {
  // --- compare-transcripts on real committed transcripts ------------------------------
  const self = compareTranscripts(BOX_C, BOX_C);
  check('compare: Box C against itself is an identical 25-frame sequence', self.summary.identicalSequence && self.summary.same === 25 && self.ops.length === 25);
  const c = compareTranscripts(BOX_C, BOX_C, { segment: 'last' });
  const f = c.baseline.facts;
  check('compare: Box C last segment is lines 11-25 (the verdict-bearing instance)', JSON.stringify(c.baseline.lineRange) === '[11,25]' && f.serverInstances === 1 && transcriptFacts(parseTranscript(BOX_C)).serverInstances === 2);
  check(
    'compare: Box C facts -- discover probe, legacy 2025-11-25, claude/channel declared, 3 notifications, dropped key, reply',
    f.discoverProbe && f.negotiatedProtocolVersion === '2025-11-25' && f.serverDeclaresChannel && f.initialize.clientInfo.version === '2.1.283' &&
      f.channelNotifications.map((n) => n.id).join(',') === 'g1-spike-wake-test-1,g1-spike-midturn-test-2,g1-spike-midturn-test-3' &&
      f.channelNotifications.every((n) => n.droppedKeys.length === 1 && n.droppedKeys[0] === 'not identifier safe!' && n.identifierSafeKeys.join(',') === 'oac_message_id,oac_sender') &&
      f.replyCalls.length === 1 && f.replyCalls[0].inReplyTo === 'g1-spike-midturn-test-3' && f.replyCalls[0].message === 'Hello!' && f.replyCalls[0].resultOk === true && f.replyCalls[0].line === 24 && f.replyCalls[0].resultLine === 25,
    JSON.stringify(f),
  );
  check('compare: response frames are labelled with the method they answer', c.baseline.keys.includes('s->c error -32601 (server/discover)') && c.baseline.keys.includes('s->c result (tools/call)') && c.baseline.keys.includes('s->c result (initialize)'));

  const orig = compareTranscripts(ORIGINAL, BOX_C, { segment: 'last' });
  const plus = orig.ops.filter((o) => o.op === '+').map((o) => o.key);
  check(
    'compare: original v2.1.282 run vs Box C (last instances) -- exactly the discover pair and one more channel notification are new',
    orig.ops.every((o) => o.op !== '-') && plus.join('|') === 'c->s request server/discover|s->c error -32601 (server/discover)|s->c notification notifications/claude/channel' && !orig.baseline.facts.discoverProbe,
    plus.join('|'),
  );
  const boxB = compareTranscripts(BOX_C, BOX_B, { segment: 'last', detail: true });
  const minus = boxB.ops.filter((o) => o.op === '-');
  check('compare: Box B vs Box C with --detail -- Box B lacks exactly one mid-turn notification (line 23 of Box C)', boxB.ops.every((o) => o.op !== '+') && minus.length === 1 && minus[0].key === 's->c notification notifications/claude/channel [midturn-test]' && minus[0].baselineLine === 23, JSON.stringify(minus));

  // --- synthetic variants with known differences ----------------------------------------
  const other = boxCAsAnotherRun();
  const same = compareTranscripts(BOX_C, toJsonl(other), { segment: 'last', detail: true });
  check('compare: timestamps, JSON-RPC ids, tool-use id, progress token and pid are normalized away', same.summary.identicalSequence && same.summary.same === 15, formatDiff(same));
  const na = normalizeEntries(selectSegment(parseTranscript(BOX_C), 'last')).map(({ line, ...r }) => r);
  const nb = normalizeEntries(parseTranscript(toJsonl(other))).map(({ line, ...r }) => r);
  check('compare: normalized frames of the shifted copy are byte-identical to Box C\'s', JSON.stringify(na) === JSON.stringify(nb));
  check('compare: normalization maps ids by first appearance and masks tool-use ids', na.find((e) => e.payload.method === 'initialize').payload.id === '<ID:2>' && JSON.stringify(na).includes('<TOOL_USE_ID>') && !JSON.stringify(na).includes('toolu_'));

  const noProbe = other.filter((e) => !(e.payload.method === 'server/discover' || String(e.payload.id).startsWith('server-discover')));
  const np = compareTranscripts(BOX_C, toJsonl(noProbe), { segment: 'last' });
  check('compare: a run without the discover probe diffs as exactly the two probe frames removed', np.summary.onlyInBaseline === 2 && np.summary.onlyInCandidate === 0 && np.ops.filter((o) => o.op === '-').map((o) => o.key).join('|') === 'c->s request server/discover|s->c error -32601 (server/discover)');

  const wakeIdx = other.findIndex((e) => e.payload.params?.meta?.oac_message_id === 'g1-spike-wake-test-1');
  const extra = [...other.slice(0, wakeIdx + 1), { ...other[wakeIdx], payload: { ...other[wakeIdx].payload, params: { ...other[wakeIdx].payload.params, meta: { ...other[wakeIdx].payload.params.meta, oac_message_id: 'g1-spike-wake-test-9' } } } }, ...other.slice(wakeIdx + 1)];
  const ex = compareTranscripts(BOX_C, toJsonl(extra), { segment: 'last', detail: true });
  check('compare: an extra wake notification shows as one added frame, with --detail naming its kind', ex.summary.onlyInCandidate === 1 && ex.ops.find((o) => o.op === '+').key === 's->c notification notifications/claude/channel [wake-test]' && ex.ops.find((o) => o.op === '+').candidateLine === wakeIdx + 2);
  const exPlain = compareTranscripts(BOX_C, toJsonl(extra), { segment: 'last' });
  check('compare: without --detail the same extra frame is still one method-level addition', exPlain.summary.onlyInCandidate === 1 && exPlain.summary.onlyInBaseline === 0);

  const m2 = other.findIndex((e) => e.payload.params?.meta?.oac_message_id === 'g1-spike-midturn-test-2');
  const swapped = [...other];
  [swapped[m2], swapped[m2 + 1]] = [swapped[m2 + 1], swapped[m2]];
  const sw = compareTranscripts(BOX_C, toJsonl(swapped), { segment: 'last', detail: true });
  check('compare: swapped mid-turn ids keep the method sequence but the facts show the order', sw.summary.identicalSequence && sw.candidate.facts.channelNotifications.map((n) => n.id).slice(1).join(',') === 'g1-spike-midturn-test-3,g1-spike-midturn-test-2');
  const noReply = other.filter((e) => e.payload.method !== 'tools/call' && !(e.direction === 'server->client' && e.payload.id === 42));
  const nr = compareTranscripts(BOX_C, toJsonl(noReply), { segment: 'last' });
  check('compare: a missing reply shows the call and its result removed; facts have no reply', nr.ops.filter((o) => o.op === '-').map((o) => o.key).join('|') === 'c->s request tools/call|s->c result (tools/call)' && nr.candidate.facts.replyCalls.length === 0);

  check('compare: LCS diff on plain arrays', JSON.stringify(diffSequences(['a', 'b', 'c'], ['a', 'c', 'd']).map((o) => o.op + o.key)) === JSON.stringify(['=a', '-b', '=c', '+d']));
  check('compare: a malformed line is rejected with its line number', throws(() => parseTranscript(`${BOX_C.split('\n')[0]}\n{oops\n`), TranscriptError, /line 2/) && throws(() => parseTranscript('{"t":"x","direction":"sideways","payload":{}}'), TranscriptError, /line 1/));
  check('compare: unknown segment rejected', throws(() => compareTranscripts(BOX_C, BOX_C, { segment: 'first' }), TranscriptError));
  check('compare: legacy revision test', isLegacyRevision('2025-11-25') && isLegacyRevision('2025-06-18') && !isLegacyRevision('2026-07-28') && !isLegacyRevision(undefined));
  const cli = spawnSync(process.execPath, [join(REPO, 'tools', 'herdr', 'lib', 'compare-transcripts.mjs'), join(REPO, BOX_C_TRANSCRIPT), FIX('transcript-2026-09-28-2.1.283.jsonl'), '--segment', 'last', '--detail'], { encoding: 'utf8', timeout: 15000 });
  check('compare: CLI prints the diff and exits 1 on a difference', cli.status === 1 && cli.stdout.includes('- s->c notification notifications/claude/channel [midturn-test]') && /summary: 14 same, 1 only in/.test(cli.stdout), cli.stdout + cli.stderr);

  // --- Claude Code pin-move check ---------------------------------------------------------
  const real = parseClaudeLastObserved(read(join(REPO, 'docs', 'planning', 'PINS.md')));
  check('pin: PINS.md "Claude Code (Channels)" last-observed version parses', /^\d+\.\d+\.\d+$/.test(real.lastObserved), real.lastObserved);
  const table = (cellText) => `| Surface | Stability label | Pinned version | Gates affected |\n|---|---|---|---|\n| Claude Code (Channels) | research preview | ${cellText} | G1 |\n`;
  check('pin: synthetic floating row -> last observed', parseClaudeLastObserved(table('**floating** — last observed `v2.1.300`; see policy')).lastObserved === '2.1.300');
  check('pin: a row without a last-observed version throws', throws(() => parseClaudeLastObserved(table('`v2.1.274`'))) && throws(() => parseClaudeLastObserved('| Surface | Pinned version |\n|---|---|\n| zenoh | `1.10.1` |')));
  check('pin: claude --version output parses', parseClaudeCliVersion('2.1.283 (Claude Code)\n') === '2.1.283' && parseClaudeCliVersion('v2.1.284') === '2.1.284' && parseClaudeCliVersion('N/A (not runnable: ENOENT)') === null && parseClaudeCliVersion('2.1.283-beta (x)') === null);
  const trig = claudePinMoveTrigger({ observed: '2.1.284', lastObserved: '2.1.283', source: '`claude --version`' });
  check('pin: equal versions -> no trigger; a different version -> a pin-move trigger naming both, no PINS.md edit', claudePinMoveTrigger({ observed: '2.1.283', lastObserved: '2.1.283', source: 'x' }) === null && /^PIN-MOVE TRIGGER/.test(trig) && trig.includes('v2.1.284') && trig.includes('v2.1.283') && /does not edit PINS\.md/.test(trig));
  check('pin: an unparseable version is a trigger too', /no parseable version/.test(claudePinMoveTrigger({ observed: null, lastObserved: '2.1.283', source: 'x' })));

  // --- lib/g1.mjs -----------------------------------------------------------------------
  const dialog = boxCDialogFromResult();
  const cls = classifyScreen(dialog);
  check('g1: Box C\'s recorded dialog text (G1-result.md) is recognized as the dev-channels dialog, option 1 selected', cls.dialog === 'dev-channels' && cls.selected?.number === 1 && cls.selected.text === 'I am using this for local development' && !cls.busy, JSON.stringify({ cls, dialog }));
  check('g1: the driver may accept it (its own preselected option is the accepting one)', driverMayAccept(cls).ok);
  check('g1: Box C\'s recorded dialog text matches Box C', dialogMatchesBoxC(dialog).matches);
  const boxed = `╭────────╮\n│ ${dialog.split('\n').join(' │\n│ ')} │\n╰────────╯`;
  check('g1: a boxed, re-wrapped rendering of the same text still matches', dialogMatchesBoxC(boxed).matches && classifyScreen(boxed).selected?.number === 1, dialogMatchesBoxC(boxed).missing.join('|'));
  const changed = dialog.replace('Please use --channels to run a list of approved channels.', 'Please use --channels instead.');
  check('g1: a changed dialog text is reported with the missing Box C line', !dialogMatchesBoxC(changed).matches && dialogMatchesBoxC(changed).missing.join() === 'Please use --channels to run a list of approved channels.');
  const wrong = dialog.replace('❯ 1.', '  1.').replace('2. Exit', '\n❯ 2. Exit');
  check('g1: the driver refuses when a non-accepting option is selected', classifyScreen(wrong).dialog === 'dev-channels' && !driverMayAccept(classifyScreen(wrong)).ok);
  const unknown = classifyScreen('Something new\n❯ 1. Continue\nEnter to confirm · Esc to cancel');
  check('g1: an unrecognized dialog is "unknown" and never driver-accepted', unknown.dialog === 'unknown' && !driverMayAccept(unknown).ok && /never accepts/.test(driverMayAccept(unknown).why));
  check('g1: the in-progress indicator is a parameter', classifyScreen('✻ Working… (esc to interrupt)').busy && !classifyScreen('> ').busy && classifyScreen('Busy!', { busyIndicator: 'busy!' }).busy);
  check('g1: plain idle screen has no dialog', classifyScreen('╭───╮\n│ > │\n╰───╯\n  ? for shortcuts').dialog === null);

  const secs = formatSection({ seq: 3, label: 'a', source: 'visible', startedAt: 's', endedAt: 'e' }, 'x\ny\n') + formatSection({ seq: 5, label: 'b', source: 'recent-unwrapped', startedAt: 's2', endedAt: 'e2' }, '');
  const parsed = parseSections(secs);
  check('g1: pane sections round-trip with their command seq and timestamps', parsed.length === 2 && parsed[0].text === 'x\ny' && parsed[0].seq === 3 && parsed[1].text === '' && parsed[1].source === 'recent-unwrapped');
  check('g1: a section without its end marker is rejected', throws(() => parseSections(secs.split('\n').slice(0, 3).join('\n'))));
  check('g1: fixture names follow K4', JSON.stringify(fixtureNames('2026-09-28', '2.1.283')) === JSON.stringify({ transcript: 'transcript-2026-09-28-2.1.283-herdr.jsonl', pane: 'pane-2026-09-28-2.1.283-herdr.txt' }) && throws(() => fixtureNames('2026-9-28', '2.1.283')) && throws(() => fixtureNames('2026-09-28', 'latest')));

  const committed = join(REPO, COMMITTED_SERVER);
  const before = sha256(readFileSync(committed));
  const tmp = mkdtempSync(join(tmpdir(), 'oac-g1-unit-'));
  try {
    const st = stageServerCopy(committed, tmp);
    check('g1: the server copy is byte-identical (sha256) to the committed file', st.match && st.copySha256 === before && st.copyPath === join(tmp, 'channel-server.mjs'));
    writeFileSync(st.copyPath, `${readFileSync(st.copyPath, 'utf8')}// edited\n`);
    check('g1: an edited copy fails the sha256 check', verifyServerCopy(committed, st.copyPath).match === false);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
  check('g1: the committed quarantined server is unchanged', sha256(readFileSync(committed)) === before);
  const scen = read(join(REPO, 'tools', 'herdr', 'scenarios', 'g1-claude-wake.mjs'));
  check('g1: the scenario never imports the quarantined server', !/import\s*\(?[^;]*(?:throwaway-quarantined|channel-server)/.test(scen) && !/require\([^)]*channel-server/.test(scen));
  check('g1: the default launch is G1\'s, verbatim', JSON.stringify(G1_LAUNCH) === '["claude","--dangerously-load-development-channels","server:g1spike"]');
  check('g1: default prompts carry no notification; an injected one is refused', Object.values(DEFAULT_PROMPTS).every((p) => !throws(() => assertNotInjected('p', p))) && throws(() => assertNotInjected('p', 'send notifications/claude/channel now')) && throws(() => assertNotInjected('p', 'pretend g1-spike-wake-test-1 arrived')));

  // --- mid-turn windows ------------------------------------------------------------------
  const at = (ms) => new Date(Date.parse('2026-09-28T10:00:00.000Z') + ms).toISOString();
  const sec = (seq, s, e, text, source = 'visible') => ({ seq, startedAt: at(s), endedAt: at(e), text, source });
  const BUSY = '⏺ Bash(sleep 20)\n✻ Working… (esc to interrupt)';
  const IDLE = '> ';
  const base = { notificationT: at(5000), promptSubmittedAt: at(1000) };
  const w1 = midTurnWindow({ ...base, sections: [sec(1, 1500, 1600, BUSY), sec(2, 4800, 4900, BUSY), sec(3, 5100, 5200, BUSY)] });
  check('midturn: busy reads on both sides of the wire t -> established, with the unobserved window', w1.established && w1.lastBeforeSeq === 2 && w1.firstAfterSeq === 3 && w1.uncoveredWindowMs === 200 && w1.gapBeforeMs === 100 && w1.gapAfterMs === 100, JSON.stringify(w1));
  const w2 = midTurnWindow({ ...base, sections: [sec(1, 1500, 1600, BUSY), sec(2, 4800, 4900, BUSY), sec(3, 5100, 5200, IDLE)] });
  check('midturn: idle read after the notification -> not established', !w2.established && /first pane read after/.test(w2.reasons.join()));
  const w3 = midTurnWindow({ ...base, sections: [sec(1, 1500, 1600, BUSY), sec(2, 3000, 3100, IDLE), sec(3, 4800, 4900, BUSY), sec(4, 5100, 5200, BUSY)] });
  check('midturn: an idle read between turn start and the notification -> not established (the turn may have ended)', !w3.established && /showed it not in progress/.test(w3.reasons.join()));
  const w4 = midTurnWindow({ ...base, sections: [sec(2, 4800, 4900, BUSY), sec(3, 5100, 5200, BUSY)], inputCommands: [{ seq: 9, role: 'operator-input', startedAt: at(3000) }, { seq: 10, role: 'dialog-accept', startedAt: at(3500) }] });
  check('midturn: operator input inside the window -> not established; a dialog accept is allowed', !w4.established && /#9/.test(w4.reasons.join()) && !/#10/.test(w4.reasons.join()));
  const w5 = midTurnWindow({ ...base, sections: [sec(2, 4800, 5100, BUSY), sec(3, 5100, 5200, BUSY, 'recent-unwrapped')] });
  check('midturn: a read overlapping t does not count, recent-unwrapped reads are ignored -> not established', !w5.established);
  const w6 = midTurnWindow({ ...base, notificationT: at(500), sections: [sec(2, 4800, 4900, BUSY), sec(3, 5100, 5200, BUSY)] });
  check('midturn: a notification before the busy prompt -> not established', !w6.established && /before the busy prompt/.test(w6.reasons.join()));
  const w7 = midTurnWindow({ ...base, sections: [sec(2, 4800, 4900, 'Do you want to proceed?\n❯ 1. Yes\n  2. No'), sec(3, 5100, 5200, BUSY)] });
  check('midturn: a tool-permission dialog counts as the turn in progress', w7.established);

  // --- report scoring rules --------------------------------------------------------------
  check('report: criterion 5 cannot take an operator score', throws(() => parseOperatorScores([{ n: 5, score: 'equivalent', note: 'x' }]), ReportError, /never scored/));
  check('report: criteria 1 and 4 cannot take an operator score', throws(() => parseOperatorScores([{ n: 1, score: 'equivalent', note: 'x' }]), ReportError, /mechanically/));
  check('report: an operator score needs a note and a valid value', throws(() => parseOperatorScores([{ n: 2, score: 'equivalent', note: '' }]), ReportError, /note/) && throws(() => parseOperatorScores([{ n: 3, score: 'probably', note: 'x' }]), ReportError));
  const notRun = evaluateG1({ manifest: { outcome: 'NOT RUN', outcomeReason: 'PIN-MOVE TRIGGER: ...', scenarioData: {} }, transcriptText: null, paneText: null, baselineText: BOX_C });
  check('report: a NOT RUN leaves every criterion not evaluable', notRun.rows.every((r) => r.score === SCORES.NE && /NOT RUN/.test(r.reason)));
}

// --- lifecycle cases (driver end to end against the fakes) --------------------------------

const today = () => new Date().toISOString().slice(0, 10);
const FAST = ['--param', 'settleMs=300', '--param', 'pollMs=200', '--param', 'midturnDelayMs=500', '--param', 'midturnGapMs=700', '--param', 'wireTimeoutMs=8000', '--param', 'turnTimeoutMs=30000', '--param', 'startupTimeoutMs=20000', '--param', 'handshakeTimeoutMs=20000'];

// Each case: { name, opts: { scenario, mode, args, fakeClaude: {env} }, assert(r) }.
export function g1Cases(check) {
  const cases = [];
  const run = (name, opts, assert) => cases.push({ name, opts: { scenario: 'g1-claude-wake', mode: 'fake-claude', ...opts }, assert });
  const committedSha = sha256(readFileSync(join(REPO, COMMITTED_SERVER)));
  const names = () => fixtureNames(today(), '2.1.283');
  const inputAfter = (m, seq) => m.commands.filter((x) => x.seq > seq && ['operator-input', 'dialog-accept'].includes(x.role));
  const evalRun = (r, operatorScores = {}) =>
    evaluateG1({ manifest: r.manifest, transcriptText: r.capture(names().transcript), paneText: r.capture(names().pane), baselineText: BOX_C, operatorScores });

  run('g1 driver accept', { args: ['--param', 'accept=driver', ...FAST], fakeClaude: { FAKE_CLAUDE_STEP_MS: '1000' } }, (r) => {
    const m = r.manifest;
    const g1 = m.scenarioData.g1;
    check('g1 driver accept: PASS (exit 0)', r.status === 0 && m.outcome === 'PASS', `${r.status} ${m.outcome} ${m.outcomeReason}`);
    check('g1 driver accept: launch verbatim, passed as the launch parameter', JSON.stringify(m.launch.argv) === JSON.stringify(G1_LAUNCH) && g1.launch.verbatim === true && r.calls.some((c) => c.argv.join(' ').includes('agent start g1claude --kind claude --pane w1:p1 --timeout 20000 -- --dangerously-load-development-channels server:g1spike')));
    check('g1 driver accept: server copy sha256 matches the committed file', g1.server.match && g1.server.committedSha256 === committedSha && g1.server.copySha256 === committedSha && g1.server.copy === '<SCRATCH>/g1-server/channel-server.mjs');
    const d = g1.dialogs[0];
    const acc = m.commands.find((x) => x.role === 'dialog-accept');
    check('g1 driver accept: dialog read verbatim, then accepted by the driver with no input in between', d?.kind === 'dev-channels' && d.acceptOrigin === 'driver' && acc && acc.seq === d.acceptSeq && m.commands.find((x) => x.seq === acc.seq - 1)?.argv.includes('read') && d.inputBetweenReadAndAccept === 0 && d.matchesBoxC.matches, JSON.stringify(d));
    check('g1 driver accept: no keystroke reached the agent before the dialog read', inputAfter(m, g1.agentStart.seq).every((x) => x.seq > d.readSeq));
    check('g1 driver accept: captures carry the K4 fixture names and were written redacted', m.captures.map((x) => x.file).sort().join() === [names().pane, names().transcript].sort().join() && m.captures.every((x) => x.written && x.redaction.residualLeaks.length === 0 && x.redaction.residualGenericHits.length === 0));
    const tf = transcriptFacts(parseTranscript(r.capture(names().transcript)));
    check('g1 driver accept: wire covers idle wake, two mid-turn notifications and a reply', tf.channelNotifications.map((n) => n.kind).join(',') === 'wake-test,midturn-test,midturn-test' && tf.replyCalls.length === 1 && tf.replyCalls[0].resultOk);
    check('g1 driver accept: every notification came from a channel-server trigger, none from herdr', g1.triggers.length === tf.channelNotifications.length && r.prompts.every((p) => !/notifications\/claude\/channel|g1-spike-(wake|midturn)/.test(p.text)) && r.prompts.length === 3);
    check('g1 driver accept: both mid-turn notifications established mid-turn from wire t vs pane reads', g1.busyTurn.notifications.every((n) => n.midTurn.established), JSON.stringify(g1.busyTurn.notifications.map((n) => n.midTurn)));
    const pane = parseSections(r.capture(names().pane));
    check('g1 driver accept: the pane capture holds the dialog read verbatim, keyed to its herdr command', pane.some((s) => s.seq === d.readSeq && dialogMatchesBoxC(s.text).matches));
    check('g1 driver accept: versions recorded (CLI, wire, PINS.md last observed)', g1.versions.cli === '2.1.283' && g1.versions.wireClientInfo === '2.1.283' && /^\d+\.\d+\.\d+$/.test(g1.versions.pinsLastObserved));
    const ev = evalRun(r);
    const s = ev.rows.map((x) => x.score);
    check('g1 driver accept: report scores C1/C4 equivalent, C2/C3 pending the operator, C5 NOT scored (driver-sent accept)', s[0] === SCORES.EQ && s[3] === SCORES.EQ && s[1] === SCORES.NE && s[2] === SCORES.NE && s[4] === SCORES.NE && /driver-sent accept/.test(ev.rows[4].reason) && /operator review/.test(ev.rows[1].reason), JSON.stringify(ev.rows.map((x) => [x.score, x.reason])));
    const ev2 = evalRun(r, parseOperatorScores([{ n: 2, score: 'equivalent', note: 'pane read shows the three attributes' }, { n: 3, score: 'not-equivalent', note: 'synthetic' }]));
    check('g1 driver accept: operator scores apply to C2/C3 only when their mechanical preconditions hold; C5 still not scored', ev2.rows[1].score === SCORES.EQ && ev2.rows[2].score === SCORES.NEQ && ev2.rows[4].score === SCORES.NE);
    const diff = compareTranscripts(BOX_C, r.capture(names().transcript), { segment: 'last' });
    check('g1 driver accept: method-sequence diff vs Box C is only the discover probe the fake does not send', diff.summary.onlyInCandidate === 0 && diff.ops.filter((o) => o.op === '-').map((o) => o.key).join('|') === 'c->s request server/discover|s->c error -32601 (server/discover)', formatDiff(diff));
    check('g1 driver accept: committed quarantined server unchanged after the run', sha256(readFileSync(join(REPO, COMMITTED_SERVER))) === committedSha);

    // The report CLI: draft to stdout, then --write into a temporary root (never the repo).
    const REPORT = join(REPO, 'tools', 'herdr', 'lib', 'g1-report.mjs');
    const draft = spawnSync(process.execPath, [REPORT, '--run', r.outDir], { encoding: 'utf8', timeout: 20000 });
    check('g1 report CLI: draft printed, marked not verdict-bearing, with the diff', draft.status === 0 && /Not verdict-bearing/.test(draft.stdout) && /## Method-sequence diff/.test(draft.stdout) && /\*\*not evaluable\*\*/.test(draft.stdout), draft.stderr);
    const bad = spawnSync(process.execPath, [REPORT, '--run', r.outDir, '--score', '5=equivalent', '--note', '5=I watched it'], { encoding: 'utf8', timeout: 20000 });
    check('g1 report CLI: refuses an operator score for criterion 5', bad.status === 2 && /criterion 5 cannot be scored by hand/.test(bad.stderr));
    const root = mkdtempSync(join(tmpdir(), 'oac-g1-report-'));
    try {
      const w = spawnSync(process.execPath, [REPORT, '--run', r.outDir, '--write', '--root', root], { encoding: 'utf8', timeout: 20000 });
      const date = g1.date;
      const expected = [`docs/planning/gates/herdr-runs/G1-${date}.md`, `docs/planning/gates/herdr-runs/G1-${date}.run-manifest.json`, `${FIXTURE_DIR}/${names().transcript}`, `${FIXTURE_DIR}/${names().pane}`];
      check('g1 report CLI --write: report, run manifest beside it, and both fixtures written under the root', w.status === 0 && expected.every((p) => existsSync(join(root, p))), w.stdout + w.stderr);
      const entries = JSON.parse(readFileSync(join(r.outDir, 'manifest-entries.draft.json'), 'utf8'));
      const REQUIRED = ['path', 'provider', 'surface', 'observed_version', 'pins_row', 'pins_as_of', 'version_matches_pin', 'capture_date', 'capture_utc_range', 'superseded_by', 'redaction', 'coverage'];
      check('g1 report CLI --write: draft MANIFEST entries carry every required field and the K5 driver block', entries.length === 2 && entries.every((e) => REQUIRED.every((k) => k in e) && e.driver.herdr_version === 'herdr 0.9.1' && /^[0-9a-f]{40}$/.test(e.driver.driver_commit) && e.driver.run_manifest === `docs/planning/gates/herdr-runs/G1-${date}.run-manifest.json`));
      const again = spawnSync(process.execPath, [REPORT, '--run', r.outDir, '--write', '--root', root], { encoding: 'utf8', timeout: 20000 });
      check('g1 report CLI --write: never overwrites', again.status === 2 && /refusing to overwrite/.test(again.stderr));
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  run('g1 human accept', { args: ['--param', 'accept=human', ...FAST], fakeClaude: { FAKE_CLAUDE_STEP_MS: '1000', FAKE_CLAUDE_SELF_ACCEPT_MS: '1500' } }, (r) => {
    const m = r.manifest;
    const d = m.scenarioData.g1.dialogs[0];
    check('g1 human accept: PASS', r.status === 0 && m.outcome === 'PASS', `${m.outcome} ${m.outcomeReason}`);
    check('g1 human accept: the driver sent no keystroke at all (no send-keys, no dialog-accept)', !m.commands.some((x) => x.role === 'dialog-accept' || x.argv.includes('send-keys')) && d.acceptOrigin === 'human' && d.inputBetweenReadAndAccept === 0);
    const ev = evalRun(r);
    check('g1 human accept: C5 scored from the recorded dialog text (fake reuses Box C text -> equivalent)', ev.rows[4].score === SCORES.EQ && ev.rows[4].checks.every((x) => x.ok), JSON.stringify(ev.rows[4]));
  });

  run('g1 human accept timeout', { args: ['--param', 'accept=human', '--param', 'humanAcceptTimeoutMs=1500', ...FAST], fakeClaude: {} }, (r) => {
    const m = r.manifest;
    const d = m.scenarioData.g1.dialogs[0];
    check('g1 human accept timeout: NOT RUN, nothing sent to the dialog', r.status === 3 && /not accepted by the operator/.test(m.outcomeReason) && inputAfter(m, d.readSeq).length === 0, `${r.status} ${m.outcomeReason}`);
    check('g1 human accept timeout: the dialog text is still captured', parseSections(r.capture(names().pane)).some((s) => s.seq === d.readSeq && dialogMatchesBoxC(s.text).matches));
  });

  run('g1 CLI version is a pin-move trigger', { args: ['--param', 'accept=driver', ...FAST], fakeClaude: { FAKE_CLAUDE_CLI_VERSION: '2.1.999' } }, (r) => {
    const m = r.manifest;
    check('g1 pin move (CLI): NOT RUN (exit 3) with a pin-move trigger naming both versions', r.status === 3 && /^PIN-MOVE TRIGGER/.test(m.outcomeReason) && m.outcomeReason.includes('v2.1.999') && m.findings.some((f) => /^PIN-MOVE TRIGGER/.test(f)), m.outcomeReason);
    check('g1 pin move (CLI): stopped before any workspace, launch or input', !r.calls.some((c) => c.argv.includes('workspace') || c.argv.includes('agent')) && !m.commands.some((x) => ['operator-input', 'dialog-accept'].includes(x.role)) && m.captures.length === 0);
  });

  run('g1 wire version is a pin-move trigger', { args: ['--param', 'accept=driver', ...FAST], fakeClaude: { FAKE_CLAUDE_VERSION: '2.1.999' } }, (r) => {
    const m = r.manifest;
    check('g1 pin move (wire): NOT RUN with a pin-move trigger from clientInfo.version', r.status === 3 && /^PIN-MOVE TRIGGER: the wire initialize clientInfo\.version reports v2\.1\.999/.test(m.outcomeReason), m.outcomeReason);
    check('g1 pin move (wire): no operator prompt was sent, no notification triggered', r.prompts.length === 0 && m.scenarioData.g1.triggers.length === 0);
  });

  run('g1 launch not verbatim', { args: ['--launch', '["claude","--channels","server:g1spike"]', ...FAST], fakeClaude: {} }, (r) => {
    check('g1 launch not verbatim: FAIL before anything is launched', r.status === 1 && /not G1's verbatim launch/.test(r.manifest.outcomeReason) && !r.calls.some((c) => c.argv.includes('agent')));
  });

  run('g1 unknown dialog', { args: ['--param', 'accept=driver', ...FAST], fakeClaude: { FAKE_CLAUDE_DIALOG: 'unknown' } }, (r) => {
    const m = r.manifest;
    check('g1 unknown dialog: NOT RUN, the driver never accepts it', r.status === 3 && /never accepts a dialog it cannot name/.test(m.outcomeReason) && !m.commands.some((x) => x.role === 'dialog-accept'), m.outcomeReason);
  });

  run('g1 wrong preselection', { args: ['--param', 'accept=driver', ...FAST], fakeClaude: { FAKE_CLAUDE_DIALOG: 'wrong-selection' } }, (r) => {
    const m = r.manifest;
    check('g1 wrong preselection: NOT RUN, the driver does not press Enter on "Exit"', r.status === 3 && /is not the dev-channels accepting option/.test(m.outcomeReason) && !m.commands.some((x) => x.role === 'dialog-accept'), m.outcomeReason);
  });
  return cases;
}

// A fake `claude` CLI for `claude --version` (the driver records harness versions with it).
export function installFakeClaudeCli(base) {
  const bin = join(base, 'bin');
  mkdirSync(bin, { recursive: true });
  const p = join(bin, 'claude');
  writeFileSync(p, '#!/bin/sh\necho "${FAKE_CLAUDE_CLI_VERSION:-2.1.283} (Claude Code)"\n');
  chmodSync(p, 0o755);
  return bin;
}
