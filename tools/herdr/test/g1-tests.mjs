// K4 (#127) tests, run by the driver self-test (`node tools/herdr/run.mjs --self-test`).
//
// Unit half: tools/herdr/lib/compare-transcripts.mjs against the REAL committed G1
// transcripts (Box C, Box B, the original run) and synthetic variants of Box C with known
// differences; the Claude Code version warning (#216); lib/g1.mjs (dialog recognition against Box
// C's recorded dialog text, mid-turn windows, pane sections, server staging); and the
// scoring rules of lib/g1-report.mjs.
//
// Lifecycle half: tools/herdr/scenarios/g1-claude-wake.mjs end to end through run.mjs
// against test/fake-herdr.mjs and test/fake-claude.mjs, both TEST DOUBLES. These prove the
// scenario's and the driver's own behavior (staging, ordering, the accept-origin rule,
// version warnings that never stop a run, captures, redaction, teardown). They prove nothing about herdr or Claude
// Code: a live G1 run through herdr is evidenced only by its own local run record.

import { spawnSync } from 'node:child_process';
import { chmodSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { compareTranscripts, diffSequences, formatDiff, parseTranscript, selectSegment, transcriptFacts, normalizeEntries, TranscriptError, isLegacyRevision } from '../lib/compare-transcripts.mjs';
import { parseClaudeVersions, parseClaudeCliVersion, claudeVersionWarning, compareVersions, pinsReadWarning } from '../lib/pins.mjs';
import {
  BOX_C_TRANSCRIPT, COMMITTED_SERVER, FIXTURE_DIR, G1_LAUNCH, classifyScreen, driverMayAccept, dialogMatchesBoxC, formatSection, parseSections,
  fixtureNames, unverifiedNames, stageServerCopy, verifyServerCopy, committedFile, sha256, midTurnWindow, COMMITTED_SERVER_SHA256, selectedOption, sameDialog, acceptHint, selectionCheck,
} from '../lib/g1.mjs';
import { evaluateG1, parseOperatorScores, SCORES, ReportError, writeRefusal, fixtureWithheld, versionsVerified, versionMatchesLastTested } from '../lib/g1-report.mjs';
import { assertNotInjected, DEFAULT_PROMPTS, operatorProjectDir } from '../scenarios/g1-claude-wake.mjs';
import { AGENT_START_MAX_TIMEOUT_MS, DriverError } from '../lib/herdr.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const FIX = (f) => join(REPO, FIXTURE_DIR, f);
const read = (p) => readFileSync(p, 'utf8');
const BOX_C = read(join(REPO, BOX_C_TRANSCRIPT));
const BOX_B = read(FIX('transcript-2026-09-28-2.1.283.jsonl'));
const ORIGINAL = read(FIX('transcript.jsonl'));
const PINNED = parseClaudeVersions(read(join(REPO, 'docs', 'planning', 'PINS.md')));
const TESTED = PINNED.lastTested;
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
  check('compare: timestamps, JSON-RPC ids, tool-use id, progress token and pid are normalized away (sequence AND payloads identical)', same.summary.identical && same.summary.same === 15 && same.summary.payloadDifferences === 0, formatDiff(same));
  check('compare: Box C against itself is identical in payloads too', self.summary.identical && self.summary.payloadDifferences === 0);
  // Review finding: same method sequence, different content, must not read "identical".
  const modern = other.map((e) => {
    if (e.direction !== 'server->client' || !e.payload.result?.capabilities) return e;
    const p = JSON.parse(JSON.stringify(e.payload));
    p.result.protocolVersion = '2026-07-28';
    delete p.result.capabilities.experimental;
    return { ...e, payload: p };
  });
  const mo = compareTranscripts(BOX_C, toJsonl(modern), { segment: 'last' });
  const mop = mo.ops.find((o) => o.payloadDiff?.length);
  check('compare: a 2026-07-28 / no claude/channel candidate has the same sequence but is NOT identical; the differing paths are named', mo.summary.identicalSequence && !mo.summary.identical && mo.summary.payloadDifferences === 1 && mop.key === 's->c result (initialize)' && mop.payloadDiff.includes('$.result.protocolVersion') && mop.payloadDiff.includes('$.result.capabilities.experimental') && /~ s->c result \(initialize\)/.test(formatDiff(mo)) && !/identical method sequence and payloads/.test(formatDiff(mo)), formatDiff(mo));
  const tmpC = mkdtempSync(join(tmpdir(), 'oac-g1-cmp-'));
  try {
    writeFileSync(join(tmpC, 'modern.jsonl'), toJsonl(modern));
    writeFileSync(join(tmpC, 'same.jsonl'), toJsonl(other));
    const cm = spawnSync(process.execPath, [join(REPO, 'tools', 'herdr', 'lib', 'compare-transcripts.mjs'), join(REPO, BOX_C_TRANSCRIPT), join(tmpC, 'modern.jsonl'), '--segment', 'last'], { encoding: 'utf8', timeout: 15000 });
    const cs = spawnSync(process.execPath, [join(REPO, 'tools', 'herdr', 'lib', 'compare-transcripts.mjs'), join(REPO, BOX_C_TRANSCRIPT), join(tmpC, 'same.jsonl'), '--segment', 'last'], { encoding: 'utf8', timeout: 15000 });
    check('compare: CLI exits 1 on a payload-only difference and 0 only when sequence and payloads match', cm.status === 1 && /differ in payload/.test(cm.stdout) && cs.status === 0 && /identical method sequence and payloads/.test(cs.stdout), cm.stdout + cs.stdout);
  } finally {
    rmSync(tmpC, { recursive: true, force: true });
  }
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
  check('compare: CLI prints the diff and exits 1 on a difference', cli.status === 1 && cli.stdout.includes('- s->c notification notifications/claude/channel [midturn-test]') && /summary: 14 same method, \d+ of those with a different payload, 1 only in/.test(cli.stdout), cli.stdout + cli.stderr);

  // --- Claude Code version warning (#216: warn, never gate) --------------------------------
  check('version: PINS.md "Claude Code (Channels)" minimum and last tested versions parse; the minimum is the first worked-with v2.1.282', /^\d+\.\d+\.\d+$/.test(PINNED.lastTested) && PINNED.minimum === '2.1.282' && compareVersions(PINNED.lastTested, PINNED.minimum) >= 0, JSON.stringify(PINNED));
  const table = (cellText) => `| Surface | Stability label | Pinned version | Gates affected |\n|---|---|---|---|\n| Claude Code (Channels) | research preview | ${cellText} | G1 |\n`;
  const syn = parseClaudeVersions(table('**floating** — minimum `v2.1.200`; last tested `v2.1.300` (L9, 2026-12-01); see policy'));
  check('version: synthetic row -> minimum and last tested', syn.minimum === '2.1.200' && syn.lastTested === '2.1.300');
  // #216 review: the reader never throws; what it cannot read is null, with the reason.
  const bad = [table('`v2.1.274`'), table('**floating** — last observed `v2.1.283`'), table('**floating** — last tested `v2.1.283`'), '| Surface | Pinned version |\n|---|---|\n| zenoh | `1.10.1` |', 'no table at all', '', undefined];
  const badParsed = bad.map((t) => { try { return parseClaudeVersions(t); } catch (e) { return { threw: e.message }; } });
  check('version (#216 review): a malformed or missing row never throws; unreadable versions are null and the problem is named', badParsed.every((p) => !p.threw && typeof p.problem === 'string' && p.problem.length > 0) && badParsed[2].lastTested === '2.1.283' && badParsed[2].minimum === null && /no "minimum/.test(badParsed[2].problem) && badParsed[0].lastTested === null && badParsed[4].cell === null, JSON.stringify(badParsed));
  check('version (#216 review): pinsReadWarning is a VERSION WARNING naming the row and the problem (never a stop), null for a good row', /^VERSION WARNING \(G1\): could not read docs\/planning\/PINS\.md "Claude Code \(Channels\)"/.test(pinsReadWarning(badParsed[0], 'G1')) && /run continues/.test(pinsReadWarning(badParsed[0], 'G1')) && pinsReadWarning(PINNED, 'G1') === null && PINNED.problem === null);
  const nullW = claudeVersionWarning({ observed: '2.1.285', lastTested: null, minimum: undefined, source: 'x' });
  check('version (#216 review): a null last tested or minimum version is a "could not be read" warning, never "vundefined"/"vnull"', /last tested version could not be read/.test(nullW) && /minimum version could not be read/.test(nullW) && /records minimum unreadable, last tested unreadable/.test(nullW) && !/vundefined|vnull|undefined|null/.test(nullW), nullW);
  check('version: compareVersions orders numerically', compareVersions('2.1.10', '2.1.9') > 0 && compareVersions('2.1.282', '2.1.282') === 0 && compareVersions('0.154.0', '0.159.3') < 0);
  check('pin: claude --version output parses', parseClaudeCliVersion('2.1.283 (Claude Code)\n') === '2.1.283' && parseClaudeCliVersion('v2.1.284') === '2.1.284' && parseClaudeCliVersion('N/A (not runnable: ENOENT)') === null && parseClaudeCliVersion('2.1.283-beta (x)') === null);
  const w = claudeVersionWarning({ observed: '2.1.286', lastTested: '2.1.285', minimum: '2.1.282', source: '`claude --version`' });
  check('version: equal to last tested -> no warning; a newer version -> a VERSION WARNING naming both, never a stop, no PINS.md edit', claudeVersionWarning({ observed: '2.1.285', lastTested: '2.1.285', minimum: '2.1.282', source: 'x' }) === null && /^VERSION WARNING \(G1\)/.test(w) && w.includes('v2.1.286') && w.includes('v2.1.285') && /not the last tested/.test(w) && !/below the minimum/.test(w) && /run continues/.test(w) && /does not by itself invalidate/.test(w) && /does not edit PINS\.md/.test(w) && !/PIN-MOVE|NOT RUN/.test(w), w);
  const below = claudeVersionWarning({ observed: '2.1.281', lastTested: '2.1.285', minimum: '2.1.282', source: 'x', gate: 'G5' });
  check('version: below the minimum is a warning too (says so), never a stop', /^VERSION WARNING \(G5\)/.test(below) && /below the minimum v2\.1\.282/.test(below) && /run continues/.test(below), below);
  check('version: an unparseable version is a warning too', /no parseable version/.test(claudeVersionWarning({ observed: null, lastTested: '2.1.285', minimum: '2.1.282', source: 'x' })));

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
  // #156: Claude Code v2.1.283's folder-trust dialog as captured live (Windows, herdr seq 12).
  const TRUST = [
    ' Accessing workspace:', '', ' <SCRATCH>\\g1-project', '',
    ' Quick safety check: Is this a project you created or one you trust? (Like your own code, a well-known open source',
    " project, or work from your team). If not, take a moment to review what's in this folder first.", '',
    " Claude Code'll be able to read, edit, and execute files here.", '', ' Security guide', '',
    ' ❯ No, exit', '   Yes, I trust this folder', '', ' Enter to confirm · Esc to cancel',
  ].join('\n');
  const trust = classifyScreen(TRUST);
  check('g1: the live folder-trust dialog is recognized, its unnumbered selection read ("No, exit")', trust.dialog === 'workspace-trust' && trust.selected?.number === null && trust.selected.text === 'No, exit', JSON.stringify(trust));
  // #196: the driver accepts it by moving the selection down to "Yes, I trust this folder"
  // (verified by a read) before Enter; never Enter on the preselected "No, exit".
  const trustPlan = driverMayAccept(trust);
  check('g1 #196: with "No, exit" preselected the driver plans down (expect "Yes, I trust this folder"), then Enter', trustPlan.ok && JSON.stringify(trustPlan.keys) === '["down","enter"]' && trustPlan.moves[0].expect === 'Yes, I trust this folder', JSON.stringify(trustPlan));
  const trustYes = classifyScreen(TRUST.replace(' ❯ No, exit', '   No, exit').replace('   Yes, I trust', ' ❯ Yes, I trust'));
  check('g1: with "Yes" preselected, the driver may accept it with Enter alone', trustYes.selected?.text === 'Yes, I trust this folder' && JSON.stringify(driverMayAccept(trustYes).keys) === '["enter"]');
  const trustOdd = classifyScreen(TRUST.replace('   Yes, I trust this folder', '   Yes, I trust this folder\n   Yes, and remember it'));
  check('g1 #196: a trust dialog with an option not on record is refused (no guessed keystroke)', trustOdd.dialog === 'workspace-trust' && !driverMayAccept(trustOdd).ok && /not the ones on record|exactly one selected/.test(driverMayAccept(trustOdd).why), driverMayAccept(trustOdd).why);
  const trustTwoMarks = classifyScreen(TRUST.replace('   Yes, I trust', ' ❯ Yes, I trust'));
  check('g1 #196: two selection markers are refused', !driverMayAccept(trustTwoMarks).ok, driverMayAccept(trustTwoMarks).why);
  check('g1: a numbered selection wins over an unnumbered line; ">" and "*" never mark an unnumbered option', classifyScreen(`${dialog}\n › stray`).selected?.number === 1 && selectedOption('Enter to confirm\n> Yes\n* Yes') === null);
  // #160: the same trust dialog after an attach resized the pane (live run, seq 12 -> seq 33):
  // re-wrapped, and the shell lines above it scrolled off. Still the same dialog.
  const SHELL = 'PS <PROJECT>> C:\\nvm4w\\nodejs\\node.exe <REPO>\\tools\\herdr\\lib\\env-probe.mjs <USER_HOME>\\\nENVPROBE-OK-1\n\n';
  const TRUST_REWRAPPED = [
    ' Accessing workspace:', '', ' <PROJECT>', '',
    ' Quick safety check: Is this a project you created or one you trust? (Like your own code, a',
    ' well-known open source project, or work from your team). If not, take a moment to review',
    " what's in this folder first.", '',
    " Claude Code'll be able to read, edit, and execute files here.", '', ' Security guide', '',
    ' ❯ No, exit', '   Yes, I trust this folder', '', ' Enter to confirm · Esc to cancel',
  ].join('\n');
  const asRead = (text) => ({ text, screen: classifyScreen(text) });
  check('g1 #160: a re-wrapped, scrolled redraw of the same dialog is the same dialog (not an answer to it)', sameDialog(SHELL + TRUST.replace('<SCRATCH>\\g1-project', '<PROJECT>'), asRead(TRUST_REWRAPPED), 'workspace-trust'));
  check('g1 #160: a moved selection is still the same dialog', sameDialog(TRUST, asRead(TRUST.replace(' ❯ No, exit', '   No, exit').replace('   Yes, I trust', ' ❯ Yes, I trust')), 'workspace-trust'));
  check('g1 #160: the dialog gone, or a different dialog, is not the same dialog', !sameDialog(TRUST, asRead('╭───╮\n│ > │\n╰───╯\n  ? for shortcuts'), 'workspace-trust') && !sameDialog(TRUST, asRead(dialog), 'workspace-trust') && !sameDialog(TRUST, asRead(TRUST.replace('Security guide', 'Security guide v2')), 'workspace-trust'));
  // #161: the project MCP-server dialog as captured live (seq 47, first read).
  const MCP = [
    '  New MCP server found in this project: g1spike', '',
    '  MCP servers may execute code or access system resources. All tool calls require approval.',
    '  Learn more in the MCP documentation.', '',
    '    Use this MCP server', '    Use this and all future MCP servers in this project',
    '  ❯ Continue without using this MCP server', '', '  Enter to confirm · Esc to cancel',
  ].join('\n');
  const mcpCls = classifyScreen(MCP);
  const mcpPlan = driverMayAccept(mcpCls);
  check('g1 #196: the live MCP-server dialog is recognized; "Continue without…" preselected, so the driver plans up, up (to "Use this MCP server"), then Enter', mcpCls.dialog === 'mcp-server-approval' && mcpCls.selected?.text === 'Continue without using this MCP server' && mcpPlan.ok && JSON.stringify(mcpPlan.keys) === '["up","up","enter"]' && JSON.stringify(mcpPlan.moves.map((m) => m.expect)) === JSON.stringify(['Use this and all future MCP servers in this project', 'Use this MCP server']), JSON.stringify({ mcpCls, mcpPlan }));
  const mcpUse = classifyScreen(MCP.replace('  ❯ Continue', '    Continue').replace('    Use this MCP server', '  ❯ Use this MCP server'));
  check('g1 #161: with "Use this MCP server" preselected, the driver may accept it with Enter alone', mcpUse.selected?.text === 'Use this MCP server' && JSON.stringify(driverMayAccept(mcpUse).keys) === '["enter"]');
  const mcpAll = classifyScreen(MCP.replace('  ❯ Continue', '    Continue').replace('    Use this and all', '  ❯ Use this and all'));
  check('g1 #196: with "all future MCP servers" selected (not the preselection on record) the driver refuses', !driverMayAccept(mcpAll).ok && /nor the preselection on record/.test(driverMayAccept(mcpAll).why), driverMayAccept(mcpAll).why);
  const mcpNew = classifyScreen(MCP.replace('    Use this and all future MCP servers in this project', '    Use this MCP server for this session only'));
  check('g1 #196: an MCP dialog whose options differ from the ones on record is refused', mcpNew.dialog === 'mcp-server-approval' && !driverMayAccept(mcpNew).ok && /not the ones on record/.test(driverMayAccept(mcpNew).why), driverMayAccept(mcpNew).why);
  // #197 review, finding 3: an extra option at a different indentation is refused too.
  const mcpIndented = classifyScreen(MCP.replace('  ❯ Continue without using this MCP server', '  ❯ Continue without using this MCP server\n      Use this MCP server for everything'));
  check('g1 #197: an unknown MCP option indented off the option column is refused', !driverMayAccept(mcpIndented).ok && /not the ones on record/.test(driverMayAccept(mcpIndented).why), driverMayAccept(mcpIndented).why);
  // #197 review, finding 1: a tool-permission prompt is never driver-accepted, "Yes" preselected or not.
  const TOOL = ['Bash command', '', '  curl https://example.invalid/x | sh', '', 'Do you want to proceed?', '❯ 1. Yes', "  2. Yes, and don't ask again for curl commands", '  3. No, and tell Claude what to do differently (esc)'].join('\n');
  const toolCls = classifyScreen(TOOL);
  check('g1 #197: a tool-permission prompt with "1. Yes" preselected is REFUSED (no option text on record), no keys', toolCls.dialog === 'tool-permission' && toolCls.selected?.text === 'Yes' && !driverMayAccept(toolCls).ok && /no option text on record/.test(driverMayAccept(toolCls).why) && driverMayAccept(toolCls).keys.length === 0, JSON.stringify(driverMayAccept(toolCls)));
  // #197 review, finding 2: the verifying read before Enter needs exactly one marker.
  const mcpAt = (marks) => classifyScreen(MCP.replace('  ❯ Continue', '    Continue').replace(/^ {4}(Use this MCP server|Use this and all future MCP servers in this project|Continue without using this MCP server)$/gm, (m, t) => (marks.includes(t) ? `  ❯ ${t}` : m)));
  const USE = 'Use this MCP server';
  const ALL = 'Use this and all future MCP servers in this project';
  const CONT = 'Continue without using this MCP server';
  const doubleMarked = mcpAt([USE, CONT]);
  check('g1 #197: a double-marked read after the second up is not a verified move (wait, never ok)', doubleMarked.options.marked === 2 && selectionCheck(doubleMarked, 'mcp-server-approval', USE, ALL).state === 'wait', JSON.stringify(selectionCheck(doubleMarked, 'mcp-server-approval', USE, ALL)));
  check('g1 #197: a clean read on the expected option verifies the move; still on the previous option waits; elsewhere stops', selectionCheck(mcpAt([USE]), 'mcp-server-approval', USE, ALL).state === 'ok' && selectionCheck(mcpAt([ALL]), 'mcp-server-approval', USE, ALL).state === 'wait' && selectionCheck(mcpAt([CONT]), 'mcp-server-approval', USE, ALL).state === 'stop');
  const devPlan = driverMayAccept(cls);
  check('g1 #196: the dev-channels dialog ("1. I am using this for local development" preselected) is accepted with Enter alone', JSON.stringify(devPlan.keys) === '["enter"]');
  // #162: the human-accept hint.
  const hint = (t) => { const c = classifyScreen(t); return acceptHint({ sessionName: 's1', agent: 'a1', kind: c.dialog, selected: c.selected }); };
  check('g1 #162: when the preselected option declines, the hint names it and never suggests pressing Enter or send-keys enter', /Preselected: "No, exit", which is NOT/.test(hint(TRUST)) && /Do not just press Enter/.test(hint(TRUST)) && !/send-keys/.test(hint(TRUST)) && !/send-keys/.test(hint(MCP)), hint(TRUST));
  check('g1 #162: when the preselected option accepts, the hint offers Enter and send-keys enter; an unknown dialog gets the careful hint', /the accepting option/.test(hint(dialog)) && /herdr --session s1 agent send-keys a1 enter/.test(hint(dialog)) && /herdr session attach s1/.test(hint(dialog)) && /not recognized/.test(hint('Something new\n❯ 1. Continue\nEnter to confirm · Esc to cancel')) && !/send-keys/.test(hint('Something new\n❯ 1. Continue\nEnter to confirm · Esc to cancel')));
  // #155: herdr's agent start --timeout maximum.
  check('herdr: the agent-start readiness timeout cap is herdr v0.9.1\'s documented maximum', AGENT_START_MAX_TIMEOUT_MS === 300000);

  const secs = formatSection({ seq: 3, label: 'a', source: 'visible', startedAt: 's', endedAt: 'e' }, 'x\ny\n') + formatSection({ seq: 5, label: 'b', source: 'recent-unwrapped', startedAt: 's2', endedAt: 'e2' }, '');
  const parsed = parseSections(secs);
  check('g1: pane sections round-trip with their command seq and timestamps', parsed.length === 2 && parsed[0].text === 'x\ny' && parsed[0].seq === 3 && parsed[1].text === '' && parsed[1].source === 'recent-unwrapped');
  check('g1: a section without its end marker is rejected', throws(() => parseSections(secs.split('\n').slice(0, 3).join('\n'))));
  const un = unverifiedNames('2026-09-28');
  check('g1: unverified capture names are never fixture-shaped', un.transcript.startsWith('unverified-') && un.pane.startsWith('unverified-') && !/^transcript-\d{4}-\d{2}-\d{2}-\d+\.\d+\.\d+-herdr\.jsonl$/.test(un.transcript) && !/^pane-\d{4}-\d{2}-\d{2}-\d+\.\d+\.\d+-herdr\.txt$/.test(un.pane));
  check('g1: fixture names follow K4', JSON.stringify(fixtureNames('2026-09-28', '2.1.283')) === JSON.stringify({ transcript: 'transcript-2026-09-28-2.1.283-herdr.jsonl', pane: 'pane-2026-09-28-2.1.283-herdr.txt' }) && throws(() => fixtureNames('2026-9-28', '2.1.283')) && throws(() => fixtureNames('2026-09-28', 'latest')));

  const committed = join(REPO, COMMITTED_SERVER);
  const before = sha256(readFileSync(committed));
  const tmp = mkdtempSync(join(tmpdir(), 'oac-g1-unit-'));
  try {
    const st = stageServerCopy(REPO, COMMITTED_SERVER, tmp);
    check('g1: the server copy is staged from the blob at HEAD, sha256 = the one Box C ran', st.match && st.committedSha256 === COMMITTED_SERVER_SHA256 && st.copySha256 === COMMITTED_SERVER_SHA256 && st.copyPath === join(tmp, 'channel-server.mjs') && st.mode === '100644' && /^[0-9a-f]{40}$/.test(st.headCommit));
    check('g1: this checkout\'s quarantined server matches HEAD', st.workingTreeMatchesHead === true && st.workingTreeIsSymlink === false);
    writeFileSync(st.copyPath, `${readFileSync(st.copyPath, 'utf8')}// edited\n`);
    check('g1: an edited copy fails the sha256 check', verifyServerCopy(st.copyPath, st.committedSha256).match === false);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
  // Review finding: a locally edited or symlink-replaced working-tree file must not pass as
  // the committed file. A throwaway git repo stands in for the checkout.
  const gr = mkdtempSync(join(tmpdir(), 'oac-g1-git-'));
  try {
    const git = (...a) => spawnSync('git', ['-c', 'user.email=t@example.invalid', '-c', 'user.name=t', '-c', 'commit.gpgsign=false', ...a], { cwd: gr, encoding: 'utf8', timeout: 15000 });
    git('init', '-q');
    writeFileSync(join(gr, 'srv.mjs'), 'committed\n');
    writeFileSync(join(gr, 'other.mjs'), 'something else\n');
    git('add', '.');
    git('commit', '-q', '-m', 'x');
    const clean = committedFile(gr, 'srv.mjs');
    writeFileSync(join(gr, 'srv.mjs'), 'edited locally\n');
    const edited = committedFile(gr, 'srv.mjs');
    const out = mkdtempSync(join(tmpdir(), 'oac-g1-stage-'));
    const staged = stageServerCopy(gr, 'srv.mjs', out);
    const copied = readFileSync(join(out, 'channel-server.mjs'), 'utf8');
    rmSync(out, { recursive: true, force: true });
    unlinkSync(join(gr, 'srv.mjs'));
    symlinkSync(join(gr, 'other.mjs'), join(gr, 'srv.mjs'));
    const linked = committedFile(gr, 'srv.mjs');
    writeFileSync(join(gr, 'other.mjs'), 'committed\n'); // symlink target now has identical bytes
    const linkedSame = committedFile(gr, 'srv.mjs');
    check('g1: committedFile reads HEAD\'s blob, not the disk file', clean.workingTreeMatchesHead && clean.committedSha256 === sha256('committed\n') && edited.committedSha256 === sha256('committed\n'));
    check('g1: a locally edited working-tree file is caught (workingTreeMatchesHead false)', edited.workingTreeMatchesHead === false && edited.workingTreeSha256 === sha256('edited locally\n'));
    check('g1: staging an edited checkout still copies the COMMITTED bytes, and reports the edit', copied === 'committed\n' && staged.match && staged.workingTreeMatchesHead === false);
    check('g1: a symlink-replaced file is caught, even when its target has the committed bytes', linked.workingTreeMatchesHead === false && linkedSame.workingTreeMatchesHead === false && linkedSame.workingTreeIsSymlink === true);
    check('g1: a path not in HEAD throws', throws(() => committedFile(gr, 'nope.mjs'), null, /cannot read/));
    // #152: a CRLF checkout of an LF blob (core.autocrlf=true, Git for Windows' default) is
    // clean to git, so it matches HEAD; a real edit on top of CRLF still does not.
    unlinkSync(join(gr, 'srv.mjs'));
    git('config', 'core.autocrlf', 'true');
    writeFileSync(join(gr, 'srv.mjs'), 'committed\r\n');
    const crlf = committedFile(gr, 'srv.mjs');
    writeFileSync(join(gr, 'srv.mjs'), 'edited locally\r\n');
    const crlfEdited = committedFile(gr, 'srv.mjs');
    check('g1: a CRLF checkout of the committed LF file matches HEAD (git-normalized), raw sha256 recorded as on disk', crlf.workingTreeMatchesHead === true && crlf.workingTreeSha256 === sha256('committed\r\n') && crlf.committedSha256 === sha256('committed\n'), JSON.stringify({ ...crlf, bytes: undefined }));
    check('g1: an edited CRLF checkout still does not match HEAD', crlfEdited.workingTreeMatchesHead === false);
  } finally {
    rmSync(gr, { recursive: true, force: true });
  }
  check('g1: the committed quarantined server is unchanged', sha256(readFileSync(committed)) === before);
  const scen = read(join(REPO, 'tools', 'herdr', 'scenarios', 'g1-claude-wake.mjs'));
  check('g1: the scenario never imports the quarantined server', !/import\s*\(?[^;]*(?:throwaway-quarantined|channel-server)/.test(scen) && !/require\([^)]*channel-server/.test(scen));
  check('g1: the default launch is G1\'s, verbatim', JSON.stringify(G1_LAUNCH) === '["claude","--dangerously-load-development-channels","server:g1spike"]');
  // #156: --param projectDir (an operator-trusted directory reused across runs).
  const pd = mkdtempSync(join(tmpdir(), 'oac-g1-projectdir-'));
  try {
    const ok = operatorProjectDir(pd) === resolve(pd);
    writeFileSync(join(pd, '.mcp.json'), JSON.stringify({ mcpServers: { g1spike: { command: 'node', args: ['x'] } } }));
    const oursOk = operatorProjectDir(pd) === resolve(pd);
    writeFileSync(join(pd, '.mcp.json'), JSON.stringify({ mcpServers: { g1spike: {}, mine: {} } }));
    const foreign = throws(() => operatorProjectDir(pd), DriverError, /refusing to overwrite/);
    writeFileSync(join(pd, '.mcp.json'), '{oops');
    const garbled = throws(() => operatorProjectDir(pd), DriverError, /refusing to overwrite/);
    check('g1 projectDir: an existing directory is accepted, also when its .mcp.json is this scenario\'s own', ok && oursOk);
    check('g1 projectDir: a .mcp.json registering anything else, or unreadable, is never overwritten', foreign && garbled);
  } finally {
    rmSync(pd, { recursive: true, force: true });
  }
  check('g1 projectDir: relative, missing, and in-repository paths are refused', throws(() => operatorProjectDir('rel/dir'), DriverError, /absolute/) && throws(() => operatorProjectDir(join(tmpdir(), 'oac-g1-nope-does-not-exist')), DriverError, /does not exist/) && throws(() => operatorProjectDir(join(REPO, 'tools')), DriverError, /inside this repository/) && throws(() => operatorProjectDir(REPO), DriverError, /inside this repository/));
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
  const notRun = evaluateG1({ manifest: { outcome: 'NOT RUN', outcomeReason: 'timed out', scenarioData: {} }, transcriptText: null, paneText: null, baselineText: BOX_C });
  check('report: a NOT RUN leaves every criterion not evaluable', notRun.rows.every((r) => r.score === SCORES.NE && /NOT RUN/.test(r.reason)));
  const v = (o) => ({ versions: { verified: true, cli: '2.1.285', wireClientInfo: '2.1.285', pinsMinimum: '2.1.282', pinsLastTested: '2.1.285', pinsSource: { workingTreeMatchesHead: true }, ...o } });
  check('report: versions verified when CLI and wire report one and the same version; PINS.md is not part of it (#216)', versionsVerified(v({})) && !versionsVerified(v({ wireClientInfo: '2.1.999' })) && !versionsVerified(v({ cli: '2.1.999' })) && !versionsVerified(v({ verified: false })) && versionsVerified(v({ pinsSource: { workingTreeMatchesHead: false } })) && versionsVerified(v({ cli: '2.1.999', wireClientInfo: '2.1.999' })));
  check('report (#216): matching the last tested version is informational: a drifted version is verified but does not match', versionMatchesLastTested(v({})) && !versionMatchesLastTested(v({ cli: '2.1.999', wireClientInfo: '2.1.999' })) && !versionMatchesLastTested(v({ cli: '2.1.200', wireClientInfo: '2.1.200' })));
  check('report: --write refuses a NOT RUN outcome', /only a PASS run is written/.test(writeRefusal({ outcome: 'NOT RUN', outcomeReason: 'timed out', scenarioData: { g1: v({}) } })));
  const mixed = { outcome: 'PASS', scenarioData: { g1: { ...v({ wireClientInfo: '2.1.999', verified: false }), server: { match: true, workingTreeMatchesHead: true } } } };
  check('report (#216 operator decision): CLI != wire never refuses --write; the fixture is withheld with a VERSION WARNING', writeRefusal(mixed) === null && /^VERSION WARNING: .*no fixture is added/.test(fixtureWithheld(mixed)) && fixtureWithheld({ outcome: 'PASS', scenarioData: { g1: v({}) } }) === null);
}

// --- lifecycle cases (driver end to end against the fakes) --------------------------------

const today = () => new Date().toISOString().slice(0, 10);
const FAST = ['--param', 'settleMs=300', '--param', 'pollMs=200', '--param', 'midturnDelayMs=500', '--param', 'midturnGapMs=700', '--param', 'wireTimeoutMs=8000', '--param', 'turnTimeoutMs=30000', '--param', 'startupTimeoutMs=20000', '--param', 'handshakeTimeoutMs=20000'];

// Each case: { name, opts: { scenario, mode, args, fakeClaude: {env} }, assert(r) }.
export function g1Cases(check) {
  const cases = [];
  const run = (name, opts, assert) => cases.push({ name, opts: { scenario: 'g1-claude-wake', mode: 'fake-claude', ...opts }, assert });
  const committedSha = sha256(readFileSync(join(REPO, COMMITTED_SERVER)));
  const names = () => fixtureNames(today(), TESTED);
  const inputAfter = (m, seq) => m.commands.filter((x) => x.seq > seq && ['operator-input', 'dialog-accept'].includes(x.role));
  const evalRun = (r, operatorScores = {}) =>
    evaluateG1({ manifest: r.manifest, transcriptText: r.capture(names().transcript), paneText: r.capture(names().pane), baselineText: BOX_C, operatorScores });

  run('g1 driver accept', { args: ['--param', 'accept=driver', ...FAST], fakeClaude: { FAKE_CLAUDE_STEP_MS: '1000' } }, (r) => {
    const m = r.manifest;
    const g1 = m.scenarioData.g1;
    check('g1 driver accept: PASS (exit 0)', r.status === 0 && m.outcome === 'PASS', `${r.status} ${m.outcome} ${m.outcomeReason}`);
    check('g1 driver accept: launch verbatim, passed as the launch parameter', JSON.stringify(m.launch.argv) === JSON.stringify(G1_LAUNCH) && g1.launch.verbatim === true && r.calls.some((c) => c.argv.join(' ').includes('agent start g1claude --kind claude --pane w1:p1 --timeout 20000 -- --dangerously-load-development-channels server:g1spike')));
    check('g1 driver accept: server copy sha256 matches the committed blob at HEAD, working tree clean', g1.server.match && g1.server.committedSha256 === COMMITTED_SERVER_SHA256 && g1.server.copySha256 === committedSha && g1.server.workingTreeMatchesHead === true && g1.server.copy === '<SCRATCH>/g1-server/channel-server.mjs');
    check('g1 driver accept: versions verified on CLI and wire; PINS.md read from HEAD; at the last tested version, no VERSION WARNING', g1.versions.verified === true && g1.versions.matchesLastTested === true && g1.versions.warnings.length === 0 && !m.findings.some((f) => /VERSION WARNING/.test(f)) && g1.versions.pinsSource.workingTreeMatchesHead === true && JSON.stringify(g1.fixtures) === JSON.stringify(g1.captureNames));
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
    check('g1 driver accept: versions recorded (CLI, wire, PINS.md minimum and last tested)', g1.versions.cli === TESTED && g1.versions.wireClientInfo === TESTED && g1.versions.pinsLastTested === TESTED && g1.versions.pinsMinimum === PINNED.minimum);
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
      check('g1 report CLI --write: draft MANIFEST entries carry every required field and the K5 driver block', entries.length === 2 && entries.every((e) => REQUIRED.every((k) => k in e) && e.version_matches_pin === true && /working tree matched HEAD: true/.test(e.pins_as_of) && e.driver.herdr_version === 'herdr 0.9.1' && /^[0-9a-f]{40}$/.test(e.driver.driver_commit) && e.driver.run_manifest === `docs/planning/gates/herdr-runs/G1-${date}.run-manifest.json`));
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
    // Defense in depth: even with the dialog record saying "human", any dialog-accept
    // command in the run, or a driver accept policy, blocks the criterion-5 score.
    const tampered = JSON.parse(JSON.stringify(m));
    tampered.commands.push({ seq: 9999, role: 'dialog-accept', argv: ['herdr', 'agent', 'send-keys', 'g1claude', 'enter'], startedAt: m.timebox.end });
    const policy = JSON.parse(JSON.stringify(m));
    policy.scenarioData.g1.acceptPolicy = 'driver';
    const evT = evaluateG1({ manifest: tampered, transcriptText: r.capture(names().transcript), paneText: r.capture(names().pane), baselineText: BOX_C });
    const evP = evaluateG1({ manifest: policy, transcriptText: r.capture(names().transcript), paneText: r.capture(names().pane), baselineText: BOX_C });
    check('g1 human accept: any dialog-accept command, or a driver policy, keeps C5 not evaluable despite a "human" dialog record', evT.rows[4].score === SCORES.NE && /driver-sent/.test(evT.rows[4].reason) && evP.rows[4].score === SCORES.NE);
  });

  run('g1 human accept timeout', { args: ['--param', 'accept=human', '--param', 'humanAcceptTimeoutMs=1500', ...FAST], fakeClaude: {} }, (r) => {
    const m = r.manifest;
    const d = m.scenarioData.g1.dialogs[0];
    const g1 = m.scenarioData.g1;
    check('g1 human accept timeout: NOT RUN, nothing sent to the dialog', r.status === 3 && /not accepted by the operator/.test(m.outcomeReason) && inputAfter(m, d.readSeq).length === 0, `${r.status} ${m.outcomeReason}`);
    check('g1 human accept timeout: the dialog text is still captured, under an unverified (non-fixture) name', g1.fixtures === null && g1.captureNames.pane.startsWith('unverified-') && parseSections(r.capture(g1.captureNames.pane)).some((s) => s.seq === d.readSeq && dialogMatchesBoxC(s.text).matches) && m.captures.every((c) => c.file.startsWith('unverified-')));
  });

  // #216: versions float; a version other than PINS.md's last tested one (or below the
  // minimum) is a VERSION WARNING finding and the run proceeds to the end.
  run('g1 drifted version warns and the run proceeds', { args: ['--param', 'accept=driver', ...FAST], fakeClaude: { FAKE_CLAUDE_STEP_MS: '1000', FAKE_CLAUDE_CLI_VERSION: '2.1.999', FAKE_CLAUDE_VERSION: '2.1.999' } }, (r) => {
    const m = r.manifest;
    const g1 = m.scenarioData.g1;
    check('g1 #216 drift: PASS (exit 0), never NOT RUN on a version', r.status === 0 && m.outcome === 'PASS', `${r.status} ${m.outcome} ${m.outcomeReason}`);
    check('g1 #216 drift: a VERSION WARNING finding for the CLI and for the wire, naming both versions', g1.versions.warnings.length === 2 && m.findings.filter((f) => /^VERSION WARNING \(G1\)/.test(f)).length === 2 && m.findings.some((f) => /`claude --version` reports v2\.1\.999/.test(f) && f.includes(`v${TESTED}`)) && m.findings.some((f) => /clientInfo\.version reports v2\.1\.999/.test(f)), JSON.stringify(m.findings));
    check('g1 #216 drift: the whole run happened (three prompts, wake and mid-turn triggers) and the captures name the observed version', r.prompts.length === 3 && g1.triggers.length >= 3 && g1.versions.verified === true && g1.versions.matchesLastTested === false && JSON.stringify(g1.fixtures) === JSON.stringify(fixtureNames(today(), '2.1.999')), JSON.stringify(g1.captureNames));
    const REPORT = join(REPO, 'tools', 'herdr', 'lib', 'g1-report.mjs');
    const root = mkdtempSync(join(tmpdir(), 'oac-g1-report-'));
    try {
      const w = spawnSync(process.execPath, [REPORT, '--run', r.outDir, '--write', '--root', root], { encoding: 'utf8', timeout: 20000 });
      const entries = w.status === 0 ? JSON.parse(readFileSync(join(r.outDir, 'manifest-entries.draft.json'), 'utf8')) : [];
      check('g1 #216 drift: report --write is not refused on version; the draft entries say version_matches_pin false with a note', w.status === 0 && entries.length === 2 && entries.every((e) => e.version_matches_pin === false && /VERSION WARNING finding, not a gate/.test(e.version_matches_pin_note)), w.stderr);
      const p = spawnSync(process.execPath, [REPORT, '--run', r.outDir], { encoding: 'utf8', timeout: 20000 });
      check('g1 #216 drift: the draft lists the warnings as findings and states the policy', p.status === 0 && /Finding: VERSION WARNING \(G1\)/.test(p.stdout) && /version warnings: 2/.test(p.stdout) && /never gated, #216/.test(p.stdout), p.stdout.slice(0, 3000));
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  run('g1 below-minimum version warns and the run proceeds', { args: ['--param', 'accept=driver', ...FAST], fakeClaude: { FAKE_CLAUDE_STEP_MS: '1000', FAKE_CLAUDE_CLI_VERSION: '2.1.200', FAKE_CLAUDE_VERSION: '2.1.200' } }, (r) => {
    const m = r.manifest;
    check('g1 #216 below minimum: PASS, with VERSION WARNINGs that say "below the minimum"', r.status === 0 && m.outcome === 'PASS' && m.findings.filter((f) => /^VERSION WARNING/.test(f) && /below the minimum v2\.1\.282/.test(f)).length === 2, `${m.outcome} ${m.outcomeReason} ${JSON.stringify(m.findings)}`);
  });

  run('g1 CLI and wire disagree: warns, proceeds, captures stay unverified', { args: ['--param', 'accept=driver', ...FAST], fakeClaude: { FAKE_CLAUDE_STEP_MS: '1000', FAKE_CLAUDE_VERSION: '2.1.999' } }, (r) => {
    const m = r.manifest;
    const g1 = m.scenarioData.g1;
    check('g1 #216 CLI != wire: PASS, a wire VERSION WARNING plus a finding that the captures cannot name one version', r.status === 0 && m.outcome === 'PASS' && m.findings.some((f) => /^VERSION WARNING \(G1\): the wire initialize clientInfo\.version reports v2\.1\.999/.test(f)) && m.findings.some((f) => /captures stay unverified-\*/.test(f)), `${m.outcome} ${m.outcomeReason} ${JSON.stringify(m.findings)}`);
    check('g1 #216 CLI != wire: the run went on to the end (three prompts) but no fixture-named capture', r.prompts.length === 3 && g1.fixtures === null && g1.versions.verified === false && m.captures.length > 0 && m.captures.every((c) => c.file.startsWith('unverified-')), JSON.stringify(m.captures.map((c) => c.file)));
    const REPORT = join(REPO, 'tools', 'herdr', 'lib', 'g1-report.mjs');
    const root = mkdtempSync(join(tmpdir(), 'oac-g1-report-'));
    try {
      const w = spawnSync(process.execPath, [REPORT, '--run', r.outDir, '--write', '--root', root], { encoding: 'utf8', timeout: 20000 });
      const date = g1.date;
      const fixDir = join(root, FIXTURE_DIR);
      check('g1 #216 CLI != wire: --write writes the record and run manifest with a VERSION WARNING, but no fixture and no MANIFEST draft (operator decision)', w.status === 0 && existsSync(join(root, `docs/planning/gates/herdr-runs/G1-${date}.md`)) && existsSync(join(root, `docs/planning/gates/herdr-runs/G1-${date}.run-manifest.json`)) && !existsSync(fixDir) && !existsSync(join(r.outDir, 'manifest-entries.draft.json')) && /No fixture written: VERSION WARNING/.test(w.stdout) && /Finding: VERSION WARNING: the CLI .* no fixture is added/.test(readFileSync(join(root, `docs/planning/gates/herdr-runs/G1-${date}.md`), 'utf8')), w.stdout + w.stderr);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  // #216 review: a malformed PINS.md harness row never stops a run. The driver runs from a
  // temporary clone whose committed PINS.md has the Claude Code row's versions removed.
  let pinsClone = null;
  run('g1 malformed PINS.md row warns and the run proceeds', { args: ['--param', 'accept=driver', ...FAST], fakeClaude: { FAKE_CLAUDE_STEP_MS: '1000' }, prepare: () => (pinsClone = cloneWithPins((t) => t.replace(/^(\| Claude Code \(Channels\) \| [^|]*\| )[^|]*/m, '$1**floating** — see "Version policy" '))) }, (r) => {
    try {
      const m = r.manifest;
      const g1 = m.scenarioData.g1;
      check('g1 #216 malformed PINS.md: PASS (exit 0), not FAIL or NOT RUN', r.status === 0 && m.outcome === 'PASS', `${r.status} ${m.outcome} ${m.outcomeReason}`);
      check('g1 #216 malformed PINS.md: a "could not read" VERSION WARNING, and the version checks warn that PINS.md could not be read', m.findings.some((f) => /^VERSION WARNING \(G1\): could not read docs\/planning\/PINS\.md "Claude Code \(Channels\)"/.test(f)) && m.findings.some((f) => /last tested version could not be read/.test(f)) && g1.versions.pinsLastTested === null && g1.versions.pinsMinimum === null && !m.findings.some((f) => /vundefined|vnull/.test(f)), JSON.stringify(m.findings));
      check('g1 #216 malformed PINS.md: the whole run happened and the captures still name the observed version', r.prompts.length === 3 && g1.versions.verified === true && g1.versions.matchesLastTested === false && JSON.stringify(g1.fixtures) === JSON.stringify(fixtureNames(today(), TESTED)));
    } finally {
      if (pinsClone) rmSync(pinsClone, { recursive: true, force: true });
    }
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

  // #196: the three dialogs Claude Code showed live, in the live order, each with its live
  // preselection: "No, exit" (trust) and "Continue without using this MCP server" (MCP) are
  // refusing options, so the driver must move the selection, verified by a read, before Enter.
  run('g1 driver accepts trust, MCP and dev-channels', { args: ['--param', 'accept=driver', ...FAST], fakeClaude: { FAKE_CLAUDE_STEP_MS: '1000', FAKE_CLAUDE_DIALOG: 'workspace-trust,mcp-server-approval,dev-channels' } }, (r) => {
    const m = r.manifest;
    const g1 = m.scenarioData.g1;
    check('g1 #196 three dialogs: PASS (the fake exits on any refusing option, so a wrong Enter fails the run)', r.status === 0 && m.outcome === 'PASS', `${r.status} ${m.outcome} ${m.outcomeReason}`);
    const byKind = Object.fromEntries(g1.dialogs.map((d) => [d.kind, d]));
    check('g1 #196 three dialogs: all three recorded in order, each accepted by the DRIVER (origin recorded as driver)', g1.dialogs.map((d) => d.kind).join() === 'workspace-trust,mcp-server-approval,dev-channels' && g1.dialogs.every((d) => d.acceptOrigin === 'driver'), JSON.stringify(g1.dialogs.map((d) => [d.kind, d.acceptOrigin])));
    const keysOf = (d) => (d?.acceptKeys ?? []).map((k) => k.key).join(',');
    check('g1 #196 three dialogs: keys trust down,enter; MCP up,up,enter; dev-channels enter', keysOf(byKind['workspace-trust']) === 'down,enter' && keysOf(byKind['mcp-server-approval']) === 'up,up,enter' && keysOf(byKind['dev-channels']) === 'enter', JSON.stringify(g1.dialogs.map(keysOf)));
    const cmd = (seq) => m.commands.find((x) => x.seq === seq);
    const allReadBefore = g1.dialogs.every((d) => d.acceptKeys.every((k) => cmd(k.seq)?.role === 'dialog-accept' && cmd(k.seq - 1)?.role === 'read'));
    check('g1 #196 three dialogs: every dialog key is a dialog-accept command straight after a read of the pane', allReadBefore);
    const verified = g1.dialogs.every((d) => d.acceptKeys.filter((k) => k.key !== 'enter').every((k) => k.verifiedSeq > k.seq));
    check('g1 #196 three dialogs: every selection move was verified by a later read before Enter', verified && g1.dialogs.every((d) => d.inputBetweenReadAndAccept === 0));
    check('g1 #196 three dialogs: dialog text read verbatim before the first key reached it', g1.dialogs.every((d) => d.readSeq < d.acceptKeys[0].seq));
    const ev = evalRun(r);
    check('g1 #196 three dialogs: C5 stays not evaluable (a driver-accepted dev-channels dialog is never scored as meeting criterion 5)', ev.rows[4].score === SCORES.NE && /driver/i.test(ev.rows[4].reason), JSON.stringify(ev.rows[4]));
  });

  // #197 review: under the DEFAULT policy (accept=driver since the #196 G1 decision) a
  // tool-permission prompt with "Yes" preselected is refused: NOT RUN, no key sent.
  run('g1 default policy refuses a tool-permission prompt', { args: [...FAST], fakeClaude: { FAKE_CLAUDE_DIALOG: 'tool-permission' } }, (r) => {
    const m = r.manifest;
    check('g1 #197 tool permission: default policy is driver, run NOT RUN, no dialog-accept command, dialog text on record', r.status === 3 && m.scenarioData.g1.acceptPolicy === 'driver' && /no option text on record/.test(m.outcomeReason) && !m.commands.some((x) => x.role === 'dialog-accept') && m.scenarioData.g1.dialogs[0]?.kind === 'tool-permission', `${r.status} ${m.outcomeReason}`);
  });

  run('g1 driver refuses MCP options not on record', { args: ['--param', 'accept=driver', ...FAST], fakeClaude: { FAKE_CLAUDE_DIALOG: 'mcp-unknown-options' } }, (r) => {
    const m = r.manifest;
    check('g1 #196 unknown MCP options: NOT RUN, no keystroke sent to the dialog', r.status === 3 && /not the ones on record/.test(m.outcomeReason) && !m.commands.some((x) => x.role === 'dialog-accept'), m.outcomeReason);
  });

  run('g1 driver: selection does not move', { args: ['--param', 'accept=driver', ...FAST], fakeClaude: { FAKE_CLAUDE_DIALOG: 'workspace-trust', FAKE_CLAUDE_IGNORE_KEYS: '1' } }, (r) => {
    const m = r.manifest;
    const acc = m.commands.filter((x) => x.role === 'dialog-accept');
    check('g1 #196 stuck selection: NOT RUN after one "down", never Enter on "No, exit", nothing re-sent', r.status === 3 && /did not move/.test(m.outcomeReason) && acc.length === 1 && acc[0].argv.at(-1) === 'down', `${m.outcomeReason} ${JSON.stringify(acc.map((a) => a.argv.at(-1)))}`);
  });
  return cases;
}

// A temporary clone of this repository with the working tree's tools/herdr/ copied over it
// and `mutate(PINS.md text)` committed, so a scenario reads a malformed PINS.md row from HEAD
// (#216 review). With { commit: false } the mutation is left as an uncommitted working-tree
// edit instead (#139). Test setup only: everything happens in a fresh temp directory.
export function cloneWithPins(mutate, { commit = true } = {}) {
  const dir = mkdtempSync(join(tmpdir(), 'oac-pins-'));
  const git = (args) => {
    const r = spawnSync('git', ['-c', 'user.name=oac-selftest', '-c', 'user.email=selftest@invalid', '-c', 'commit.gpgsign=false', ...args], { cwd: dir, encoding: 'utf8', timeout: 60000 });
    if (r.status !== 0) throw new Error(`git ${args.join(' ')} failed in the test clone: ${r.stderr}`);
  };
  git(['clone', '-q', '--no-hardlinks', REPO, '.']);
  cpSync(join(REPO, 'tools', 'herdr'), join(dir, 'tools', 'herdr'), { recursive: true });
  const pins = join(dir, 'docs', 'planning', 'PINS.md');
  writeFileSync(pins, mutate(readFileSync(join(REPO, 'docs', 'planning', 'PINS.md'), 'utf8')));
  if (!commit) return dir;
  git(['add', '--', 'docs/planning/PINS.md']);
  git(['commit', '-q', '-m', 'test: malformed PINS.md row', '--', 'docs/planning/PINS.md']);
  return dir;
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
