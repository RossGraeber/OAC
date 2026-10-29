// Unit checks for the G8 slice S6 live leg (#69/#433): scenarios/g8-codex-pairing.mjs (the analysis
// helpers, the prompts and the scenario's guards) and the `midwatch` mode of gate-servers/
// g8-capture.mjs. Run by run.mjs --self-test (unit half); the scenario has no lifecycle case: it
// needs an operator's Codex TUI and a logged-in Claude Code. The inputs below are test inputs for the
// pure analysers, not harness fixtures.

import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { NotRunError } from '../lib/herdr.mjs';
import { classifyScreen, driverMayAccept, settingsWarningRules } from '../lib/g1.mjs';
import { classifyForLive, driverMayAcceptLive, liveToolApproval, makeRegistration, OAC_TOOL_INPUTS } from '../lib/codex-driver.mjs';

import scenario, {
  liveNames, claudePrompt, handoffText, pairingPrompt, sendPrompt, parseFrame, oacCalls, pairingSequence, handoffFrames, echoTrials, testNumberOf,
  claudeLaunchPlan, unmetRequiredClaims, CLAUDE_SERVER_KEY, CLAUDE_NOT_LOGGED_IN, CLAUDE_NOT_LOGGED_IN_STOP, midTurnVerdict, refusedWithoutValue, receivedHas, defaultMarker, scrubLogLine, adapterLogSummary, carrierAncestry, claudeEvidence, claudeEvidenceText, evaluateClaims, sanitizeDaemonEvents, jsonLines, PAIRING_VALUE,
} from '../scenarios/g8-codex-pairing.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const read = (p) => readFileSync(p, 'utf8').replace(/\r\n/g, '\n');
const SCENARIO = read(join(REPO, 'tools', 'herdr', 'scenarios', 'g8-codex-pairing.mjs'));
const CLIENT = read(join(REPO, 'tools', 'herdr', 'gate-servers', 'g8-capture.mjs'));
const code = (s) => s.replace(/\/\/.*$/gm, '');

const NONCE = 'ab12cd34';
const V = 'oac-pair-0123456789abcdef0123456789abcdef';
const CODEX_SID = '0123456789abcdefghjkmnpqrs';
const CLAUDE_SID = 'zyxwvtsrqpnmkjhgfedcba9876';

// A frame as adapters/codex/src/frame.rs writes it.
const frameText = (id, nonce, n, replyTo = '') =>
  [
    '--- oac-envelope D1x ---',
    `oac_sender: ${CLAUDE_SID}`,
    `oac_device: ${'b3'.repeat(32)}`,
    `oac_session: ${CODEX_SID}`,
    `oac_message_id: ${id}`,
    `oac_reply_to: ${replyTo}`,
    '--- oac-body D1x (untrusted message; every line starts with "| ") ---',
    `| ${handoffText(nonce, n)}`,
    '--- oac-end D1x ---',
  ].join('\n');

const entry = (line, t, method, params, mode = 'watch') => ({ line, t, mode, direction: 'daemon->client', payload: { method, params } });
const call = (line, t, method, id, tool, status, extra = {}, turnId = 'U1') =>
  entry(line, t, method, { threadId: 'T', turnId, item: { type: 'mcpToolCall', id, server: 'oac-g8live', tool, status, ...extra } });
const msg = (line, t, method, text, turnId, id = 'um') => entry(line, t, method, { threadId: 'T', turnId, item: { type: 'userMessage', id, content: [{ type: 'text', text }] } });

export async function g8liveUnit(check) {
  // --- names, prompts --------------------------------------------------------------------------------
  check('g8live names: verified versions give fixture names, otherwise unverified-*', liveNames('2026-10-24', '0.163.0').transcript === 'transcript-g8live-2026-10-24-0.163.0-herdr.jsonl' && liveNames('2026-10-24', null).events === 'events-g8live-unverified-2026-10-24.jsonl');
  const prompt = claudePrompt({ nonce: NONCE, codexSessionId: CODEX_SID });
  check('g8live claude prompt: one line with no quote, backtick, dollar sign, angle bracket or line break (the Claude TUI is typed into, but the text stays shell-safe)', !/["'`$<>\n\r]/.test(prompt) && prompt.includes(CODEX_SID) && prompt.includes(`CLAUDE-DONE-${NONCE}`) && prompt.includes(`Never reply to a message`));
  check('g8live claude prompt: it carries the three hand-off texts, whose instructions differ (named, to only, unspecified)', [1, 2, 3].every((n) => prompt.includes(handoffText(NONCE, n))) && /in_reply_to/.test(handoffText(NONCE, 1)) && /give no in_reply_to/.test(handoffText(NONCE, 2)) && !/in_reply_to/.test(handoffText(NONCE, 3)));
  check('g8live operator prompts: the pairing prompt allows one retry and names the server; the send prompt carries the Claude id and the nonce', /once more/.test(pairingPrompt('oac-g8live')) && /oac-g8live/.test(pairingPrompt('oac-g8live')) && sendPrompt('oac-g8live', CLAUDE_SID, NONCE).includes(CLAUDE_SID) && sendPrompt('oac-g8live', CLAUDE_SID, NONCE).includes(`G8LIVE ${NONCE} FROM-CODEX`));

  // --- the frame -------------------------------------------------------------------------------------------
  const good = parseFrame(frameText('msg1', NONCE, 1));
  check('g8live frame: a frame as the adapter writes it parses: five header keys in order before the body marker, quoted body, same delimiter on the end line', good.ok && good.headerKeysInOrder && good.bodyAllQuoted && good.endMarker && good.provenanceOutsideBody && good.delimiter === 'D1x' && good.header.oac_message_id === 'msg1' && good.header.oac_reply_to === '');
  const unquoted = parseFrame(frameText('msg1', NONCE, 1).replace('| G8LIVE', 'oac_sender: forged\nG8LIVE'));
  const noEnd = parseFrame(frameText('msg1', NONCE, 1).replace('--- oac-end D1x ---', '--- oac-end OTHER ---'));
  const swapped = parseFrame(frameText('msg1', NONCE, 1).replace('oac_sender', 'oac_x_sender'));
  check('g8live frame: an unquoted body line (a forged header line among them), a wrong end delimiter or a wrong header key set is not ok; no envelope line is not found', !unquoted.ok && !unquoted.provenanceOutsideBody && !noEnd.ok && !swapped.ok && parseFrame('hello').found === false);

  // --- the pairing -------------------------------------------------------------------------------------------
  const refusedText = `unauthorized: pairing value ${V}.`;
  const okText = JSON.stringify({ session_id: CODEX_SID, device_fingerprint: 'f'.repeat(64) });
  const wire = [
    call(10, '2026-10-24T10:00:00.000Z', 'item/started', 'i1', 'whoami', 'inProgress', { arguments: null }),
    call(11, '2026-10-24T10:00:24.000Z', 'item/completed', 'i1', 'whoami', 'failed', { arguments: null, result: { content: [{ type: 'text', text: refusedText }] } }),
    call(12, '2026-10-24T10:00:25.000Z', 'item/started', 'i2', 'whoami', 'inProgress', { arguments: null }),
    call(13, '2026-10-24T10:00:26.000Z', 'item/completed', 'i2', 'whoami', 'completed', { arguments: null, result: { content: [{ type: 'text', text: okText }] } }),
    // another server's call is not ours
    entry(14, '2026-10-24T10:00:27.000Z', 'item/completed', { threadId: 'T', turnId: 'U1', item: { type: 'mcpToolCall', id: 'x', server: 'other', tool: 'whoami', status: 'completed', result: { content: [{ type: 'text', text: okText }] } } }),
  ];
  const calls = oacCalls(wire, 'oac-g8live');
  const seq = pairingSequence(calls);
  check('g8live pairing: only the registered server\'s calls are read; the refusal that carries a pairing value, then a whoami with a session id, is the sequence; the id and fingerprint are returned', calls.length === 2 && seq.refusedThenPaired && seq.codexSessionId === CODEX_SID && seq.deviceFingerprint === 'f'.repeat(64) && seq.refusal.itemId === 'i1' && seq.paired.itemId === 'i2');
  const onlyPaired = pairingSequence(oacCalls(wire.slice(2, 4), 'oac-g8live'));
  const plainRefusal = call(11, '2026-10-24T10:00:24.000Z', 'item/completed', 'i1', 'whoami', 'failed', { arguments: null, result: { content: [{ type: 'text', text: 'unauthorized' }] } });
  const noValue = pairingSequence(oacCalls([wire[0], plainRefusal, wire[2], wire[3]], 'oac-g8live'));
  check('g8live pairing: a success with no refusal before it, or a refusal with no pairing value, is not "refused then paired"', !onlyPaired.refusedThenPaired && !noValue.refusedThenPaired);
  check('g8live pairing: the pairing-value pattern is the spec\'s (oac-pair- and 32 lower-case hex)', new RegExp(PAIRING_VALUE.source).test(V) && !new RegExp(PAIRING_VALUE.source).test('oac-pair-XYZ'));

  // --- hand-offs, replies, echo ---------------------------------------------------------------------------------
  const wire2 = [
    msg(20, '2026-10-24T10:05:00.000Z', 'item/started', frameText('m1', NONCE, 1), 'U2'),
    msg(21, '2026-10-24T10:05:00.010Z', 'item/completed', frameText('m1', NONCE, 1), 'U2'),
    call(22, '2026-10-24T10:05:02.000Z', 'item/started', 'r1', 'reply', 'inProgress', { arguments: { in_reply_to: 'm1', content: [{ type: 'text', text: 'ACK1' }] } }, 'U2'),
    call(23, '2026-10-24T10:05:04.000Z', 'item/completed', 'r1', 'reply', 'completed', { arguments: { in_reply_to: 'm1', content: [] }, result: { content: [{ type: 'text', text: '{"id":"r-1","state":"accepted-by-adapter","correlation":"correlated"}' }] } }, 'U2'),
    msg(30, '2026-10-24T10:06:00.000Z', 'item/completed', frameText('m2', NONCE, 2), 'U3', 'um2'),
    call(31, '2026-10-24T10:06:02.000Z', 'item/completed', 'r2', 'reply', 'completed', { arguments: { to: CLAUDE_SID, content: [] }, result: { content: [{ type: 'text', text: '{"id":"r-2","state":"accepted-by-adapter","correlation":"uncorrelated"}' }] } }, 'U3'),
    msg(40, '2026-10-24T10:07:00.000Z', 'item/completed', frameText('m3', NONCE, 3), 'U4', 'um3'),
    call(41, '2026-10-24T10:07:02.000Z', 'item/completed', 'r3', 'reply', 'completed', { arguments: { in_reply_to: 'something-else', to: CLAUDE_SID, content: [] }, result: { content: [{ type: 'text', text: '{"id":"r-3","state":"accepted-by-adapter","correlation":"uncorrelated"}' }] } }, 'U4'),
  ];
  const frames = handoffFrames(wire2);
  check('g8live frames: started and completed frames of one hand-off are one; each is parsed; the test number comes from the quoted body', frames.length === 3 && frames.every((f) => f.frame.ok) && frames.map((f) => testNumberOf(f.text, NONCE)).join() === '1,2,3' && frames.map((f) => f.messageId).join() === 'm1,m2,m3');
  const trials = echoTrials(frames, oacCalls(wire2, 'oac-g8live'), NONCE);
  check('g8live echo: the reply made in the hand-off\'s turn is matched; in_reply_to is "echoed-the-id", "absent" or "other-value"; the reported correlation is read from the result', trials.map((t) => t.inReplyTo).join() === 'echoed-the-id,absent,other-value' && trials.map((t) => t.reportedCorrelation).join() === 'correlated,uncorrelated,uncorrelated' && trials.map((t) => t.instruction).join() === 'in_reply_to,to-only,unspecified' && trials[1].toPresent === true);
  const none = echoTrials(frames, [], NONCE);
  check('g8live echo: a hand-off Codex did not reply to is a trial with replied false', none.length === 3 && none.every((t) => !t.replied && t.inReplyTo === 'absent'));

  // --- open question A ---------------------------------------------------------------------------------------------------
  const item = { itemId: 'i1', startedT: '2026-10-24T10:00:00.000Z', completedT: '2026-10-24T10:00:24.000Z' };
  const b = (extra) => [
    { line: 1, t: '2026-10-24T10:00:03.000Z', mode: 'midwatch', direction: 'client->daemon', payload: { id: 1, method: 'thread/resume', params: { threadId: 'T', excludeTurns: true } } },
    { line: 2, t: '2026-10-24T10:00:03.100Z', mode: 'midwatch', direction: 'daemon->client', payload: { id: 1, result: { thread: {} } } },
    ...extra,
  ];
  const completedOnB = entry(3, '2026-10-24T10:00:24.010Z', 'item/completed', { threadId: 'T', item: { type: 'mcpToolCall', id: 'i1' } }, 'midwatch');
  const rcv = midTurnVerdict({ item, bEntries: b([completedOnB]) });
  const missed = midTurnVerdict({ item, bEntries: b([]) });
  const late = midTurnVerdict({ item, bEntries: b([]).map((e) => ({ ...e, t: '2026-10-24T10:01:00.000Z' })) });
  const early = midTurnVerdict({ item, bEntries: b([]).map((e) => ({ ...e, t: '2026-10-24T09:59:00.000Z' })) });
  check('g8live mid-turn: a resume answered between the call\'s start and completion that gets the item/completed is "received"; one that does not is "not-received"', rcv.verdict === 'received' && rcv.itemCompletedOnB === 1 && Math.round(rcv.resumeMsAfterStart) === 3100 && missed.verdict === 'not-received');
  check('g8live mid-turn: a resume answered after the call completed or before it started, no call, or no answer, is "not-exercised" (never a negative answer)', late.verdict === 'not-exercised' && early.verdict === 'not-exercised' && midTurnVerdict({ item: null, bEntries: [] }).verdict === 'not-exercised' && midTurnVerdict({ item, bEntries: [] }).verdict === 'not-exercised');

  // --- open question B ---------------------------------------------------------------------------------------------------------
  const chain = [{ pid: 500, ppid: 400, image: 'oac.exe' }, { pid: 400, ppid: 300, image: 'codex.exe' }, { pid: 300, ppid: 1, image: 'codex.exe' }];
  const same = carrierAncestry({ shimChain: chain, daemonChain: [{ pid: 500, image: 'oac.exe' }, { pid: 400, image: 'codex.exe' }], carrierPid: 400, daemonHarnessPid: 400, daemonPaired: true });
  const differ = carrierAncestry({ shimChain: chain, carrierPid: 300, daemonHarnessPid: 400, daemonPaired: false });
  const unknown = carrierAncestry({ shimChain: chain, carrierPid: null });
  check('g8live ancestry: the nearest codex ancestor (not the shim itself) is compared with the opener\'s server pid; the daemon\'s own harness pid is a second witness', same.nearestCodexAncestorPid === 400 && same.sameProcess === true && same.daemonHarnessIsCarrier === true && differ.sameProcess === false && differ.daemonHarnessIsCarrier === false && unknown.sameProcess === null && Object.keys(same.shimChain[0]).sort().join() === 'image,pid,ppid');

  // --- the Claude session's report (the interactive TUI's text) --------------------------------------------------------------------------------
  const promptEcho = `> ${claudePrompt({ nonce: NONCE, codexSessionId: CODEX_SID })}`;
  const tui = [
    `● CLAUDE-SID-${NONCE}: ${CLAUDE_SID}`,
    '  ← oac: <channel source="oac" oac_sender="S" oac_message_id="M">G8LIVE x</channel>',
    `● RECEIVED-${NONCE}: G8LIVE ${NONCE} FROM-CODEX | S | M`,
    `● SENT-${NONCE}-1: id1`,
    `● SENT-${NONCE}-2: id2`,
    `● CLAUDE-DONE-${NONCE}`,
  ].join('\n');
  const evPromptOnly = claudeEvidence(promptEcho, NONCE);
  const ev = claudeEvidence(`${promptEcho}\n${tui}`, NONCE);
  const wrappedEcho = claudeEvidence(`${promptEcho.replace('Step 5: print the line ', 'Step 5: print the line\n')}\n● CLAUDE-SID-${NONCE}: ${CLAUDE_SID}`, NONCE);
  check('g8live claude report: the TUI\'s echo of the typed prompt is not a report (no id, no received line, not done), even where it wraps CLAUDE-DONE onto its own line before the answer', evPromptOnly.sessionId === null && evPromptOnly.received.length === 0 && evPromptOnly.sent.length === 0 && evPromptOnly.done === false && wrappedEcho.done === false);
  check('g8live claude report: the session id, the received line, the sent ids, the channel tag attributes and the done line (after the id line, behind a bullet) are read from the TUI text', ev.sessionId === CLAUDE_SID && ev.received.length === 1 && ev.received[0].line.includes('FROM-CODEX') && ev.sent.map((s) => s.n).join() === '1,2' && ev.done === true && ev.channelTags.some((t) => t.join() === 'source,oac_sender,oac_message_id'));
  const ct = claudeEvidenceText(`${promptEcho}\n${tui}`, NONCE);
  check('g8live claude capture: only the report lines are kept', /^CLAUDE-SID-/.test(ct) && !/You are the Claude side/.test(ct) && ct.split('\n').length <= 8);

  // The TUI wraps a long line at about 117 columns and herdr does not rejoin it: a wrap at a space and a wrap inside a token.
  const pad = 'x'.repeat(60);
  const wrapSpace = [`> prompt echo`, `● RECEIVED-${NONCE}: G8LIVE ${NONCE}`, `  FROM-CODEX | S | M`, `● CLAUDE-SID-${NONCE}: ${CLAUDE_SID.slice(0, 10)}`, `  ${CLAUDE_SID.slice(10)}`, `● SENT-${NONCE}-1: id`, '  1234', `● CLAUDE-DONE-${NONCE}`].join('\n');
  const wrapToken = [`● CLAUDE-SID-${NONCE}: ${CLAUDE_SID}`, `● RECEIVED-${NONCE}: G8LIVE ${NONCE} FROM-`, `  CODEX | S | M`, `  ⎿ done`, `● CLAUDE-DONE-${NONCE}`].join('\n');
  const w1 = claudeEvidence(wrapSpace, NONCE);
  const w2 = claudeEvidence(wrapToken, NONCE);
  const longLine = `● RECEIVED-${NONCE}: ${pad} G8LIVE ${NONCE} FROM-CODEX | S | M`;
  const cut = longLine.slice(0, 117);
  const w3 = claudeEvidence([`● CLAUDE-SID-${NONCE}: ${CLAUDE_SID}`, cut, `  ${longLine.slice(117)}`].join('\n'), NONCE);
  check('g8live claude report: a line the TUI wrapped at about 117 columns (at a space, inside a token, or cut at exactly 117) is rejoined: the received text, the session id and the sent id come out whole, and an indented non-continuation line (a tool result) is not joined', w1.received.length === 1 && w1.received[0].line.includes(`G8LIVE ${NONCE} FROM-CODEX`) && w1.sessionId === CLAUDE_SID && w1.sent[0].messageId === 'id1234' && receivedHas(w2, `G8LIVE ${NONCE} FROM-CODEX`) && w2.done === true && receivedHas(w3, `G8LIVE ${NONCE} FROM-CODEX`));
  check('g8live claude prompt: the report line is text first, then the sender and id (RECEIVED-n: text | sender | id), so a wrap can only cut the tail', /RECEIVED-\w+: then its text, then a vertical bar, then its oac_sender, then a vertical bar, then its oac_message_id/.test(claudePrompt({ nonce: NONCE, codexSessionId: CODEX_SID })) && /PowerShell\(Start-Sleep:\*\)/.test(SCENARIO) && /Bash\(sleep:\*\)/.test(SCENARIO));

  // --- a refusal with no pairing value; the mid-turn verdict's two new guards ---------------------------------------------------------------------
  const plainCall = oacCalls([call(11, '2026-10-24T10:00:24.000Z', 'item/completed', 'p1', 'whoami', 'failed', { arguments: null, result: { content: [{ type: 'text', text: 'unauthorized' }] } })], 'oac-g8live');
  check('g8live no-value: a completed oac call refused as unauthorized with no pairing value is found; one that carries a value, and a success, are not', refusedWithoutValue(plainCall)?.itemId === 'p1' && refusedWithoutValue(oacCalls(wire.slice(0, 2), 'oac-g8live')) === null && refusedWithoutValue(oacCalls(wire.slice(2, 4), 'oac-g8live')) === null);
  const open2 = { itemId: 'i1', startedT: '2026-10-24T10:00:00.000Z', completedT: null };
  const bOpen = [
    { line: 1, t: '2026-10-24T10:00:03.000Z', mode: 'midwatch', direction: 'client->daemon', payload: { id: 1, method: 'thread/resume', params: {} } },
    { line: 2, t: '2026-10-24T10:00:03.100Z', mode: 'midwatch', direction: 'daemon->client', payload: { id: 1, result: {} } },
  ];
  const ended = midTurnVerdict({ item, bEntries: b([]), bEndedAt: '2026-10-24T10:00:10.000Z' });
  const alive = midTurnVerdict({ item, bEntries: b([]), bEndedAt: '2026-10-24T10:00:30.000Z' });
  check('g8live mid-turn: a call that never completed, or a midwatch process that ended before the call completed, is "not-exercised"; one still alive at completion with no item/completed is "not-received"', midTurnVerdict({ item: open2, bEntries: bOpen }).verdict === 'not-exercised' && ended.verdict === 'not-exercised' && /ended before/.test(ended.why) && alive.verdict === 'not-received');
  const empty = evaluateClaims({ pairing: null, codexSend: null, claude: null, frames: null, trials: null, mid: null, carrier: null, nonce: NONCE });
  const nv = evaluateClaims({ pairing: { refusedThenPaired: false, refusal: null }, codexSend: null, claude: null, frames: [], trials: [], mid: null, carrier: same, nonce: NONCE, noValueRefusal: { status: 'failed' } });
  const nvDiffer = evaluateClaims({ pairing: { refusedThenPaired: false, refusal: null }, codexSend: null, claude: null, frames: [], trials: [], mid: null, carrier: differ, nonce: NONCE, noValueRefusal: { status: 'failed' } });
  const nvNone = evaluateClaims({ pairing: { refusedThenPaired: false, refusal: null }, codexSend: null, claude: null, frames: [], trials: [], mid: null, carrier: null, nonce: NONCE, noValueRefusal: { status: 'failed' } });
  check('g8live claims: with no data at all every claim is evaluated and none is shown; on the no-value path claim 6 takes its answer from the process-tree result (same process: shown with the no-value note; different: answered-negative; no tree: not-shown), never always no', empty.length === 7 && empty.every((c) => c.status === 'not-shown' && c.why) && nv[5].status === 'shown' && nv[5].evidence.some((e) => /no pairing value/.test(e)) && nvDiffer[5].status === 'answered-negative' && nvNone[5].status === 'not-shown' && nv[0].status === 'not-shown');

  // --- the daemon's events -----------------------------------------------------------------------------------------------------------------------
  const evs = jsonLines([
    '{"event":"ready","unix_ms":1,"namespace":"g8live","endpoints":["\\\\.\\\\pipe\\\\x"],"pid":9}',
    '{"event":"carrier_server","unix_ms":2,"pid":400,"chain":[{"pid":400,"started":5,"image":"C:\\\\Users\\\\someone\\\\codex.exe","resolved":"C:\\\\Users\\\\someone\\\\codex.exe"}],"pairing_key":true}',
    '{"event":"accepted","unix_ms":3,"label":"codex","peer":{"pid":500,"started":6},"chain":[{"pid":500,"started":6,"image":"oac.exe","resolved":null}],"harness":{"pid":400,"started":5},"paired":true}',
    'not json',
  ].join('\n'));
  const san = JSON.stringify(sanitizeDaemonEvents(evs));
  check('g8live events: carrier_server and accepted keep pid and image name only; no path, resolved name or start time reaches the capture', evs.length === 3 && !/Users|someone|resolved|started|endpoints/.test(san) && /"pid":400/.test(san) && /"image":"codex.exe"/.test(san) && /"harness":\{"pid":400\}/.test(san));

  // --- the claims --------------------------------------------------------------------------------------------------------------------------------
  const claudeEv = { received: [{ line: `oac_sender=S G8LIVE ${NONCE} FROM-CODEX` }], channelTags: [['source']] };
  const sendCall = { status: 'completed', resultText: '{"id":"s1","state":"accepted-by-adapter"}' };
  const all = evaluateClaims({ pairing: seq, codexSend: sendCall, claude: claudeEv, frames, trials, mid: rcv, carrier: same, nonce: NONCE });
  check('g8live claims: seven claims in order, each shown when its evidence is all there', all.length === 7 && all.map((c) => c.n).join() === '1,2,3,4,5,6,7' && all.every((c) => c.status === 'shown') && all.every((c) => c.claim && c.evidence.length > 0));
  const weak = evaluateClaims({ pairing: { refusedThenPaired: false, refusal: null }, codexSend: null, claude: { received: [], channelTags: [] }, frames: [], trials: [], mid: missed, carrier: differ, nonce: NONCE });
  check('g8live claims: missing evidence is "not-shown" with a reason; an open question answered no is "answered-negative" (an answer), not "not-shown"; none is shown on a hunch', weak[0].status === 'not-shown' && weak[1].status === 'not-shown' && weak[2].status === 'not-shown' && weak[3].status === 'not-shown' && weak[4].status === 'answered-negative' && weak[5].status === 'answered-negative' && weak[6].status === 'not-shown' && weak.every((c) => c.status === 'shown' || c.why));
  const badFrame = evaluateClaims({ pairing: seq, codexSend: sendCall, claude: claudeEv, frames: [{ ...frames[0], frame: { ...frames[0].frame, ok: false, problems: ['x'] } }], trials, mid: rcv, carrier: same, nonce: NONCE });
  check('g8live claims: one hand-off frame that is not ok keeps claim 3 from being shown', badFrame[2].status === 'not-shown');

  // --- the marker, the registered name and the adapter's log lines (PR #459) ---------------------------------------------------------------
  check('g8live marker: the default marker carries a run tag, so two runs never share one and a stale thread cannot match; an override replaces it', defaultMarker('ab12') !== defaultMarker('cd34') && /marker ab12:/.test(defaultMarker('ab12')) && /const runTag = randomBytes\(2\)/.test(SCENARIO) && /params\.operatorPrompt \|\| defaultMarker\(runTag\)/.test(SCENARIO) && /operatorPrompt: ''/.test(SCENARIO));
  check('g8live registered name: the daemon pane is started with OAC_CODEX_REGISTERED_NAME set to the server name the operator registers, and the name is recorded', /OAC_SCOPE: ns, OAC_CODEX_REGISTERED_NAME: serverName/.test(SCENARIO) && /registeredName: serverName/.test(SCENARIO));
  const logEv = (line) => ({ event: 'log', unix_ms: 1, line });
  const adapterLines = [
    logEv('the Codex carrier is connected (registered name oac-g8live)'),
    logEv('a Codex pairing window opened: 2 threads subscribed, 2 served, 0 failed'),
    logEv('a Codex call is unpairable, no pairing window: no thread served'),
    logEv('an mcpToolCall from server oac-g8live, not the registered name oac: it pairs nothing (the registration name must equal the registered name)'),
  ];
  const sum = adapterLogSummary(adapterLines, 'oac-g8live');
  const ok = adapterLogSummary(adapterLines.slice(0, 3), 'oac-g8live');
  check('g8live adapter log: the carrier line\'s registered name, windows opened, unpairable calls and other-server lines are counted; an other-server line that names the operator\'s server is flagged', sum.carrierConnectedRegisteredName === 'oac-g8live' && sum.windowsOpened === 1 && sum.unpairable === 1 && sum.otherServers.length === 1 && sum.registeredByOperatorNamedAsOther === true && ok.registeredByOperatorNamedAsOther === false && /claim 1: the daemon logged an mcpToolCall/.test(SCENARIO));
  const dirty = sanitizeDaemonEvents([logEv(`a Codex reveal ${V} for thread 0198f2c4-1111-7222-8333-444455556666 was dropped`)]);
  check('g8live adapter log: the captured log lines carry no pairing value and no thread id (a UUID)', !dirty[0].line.includes(V) && !/0198f2c4/.test(dirty[0].line) && dirty[0].line.includes('<PAIRING_VALUE>') && dirty[0].line.includes('<THREAD_ID>') && scrubLogLine('plain line') === 'plain line');

  // --- the Codex side is the driver's (lead decision 2026-10-29): the dialog table, the registration -----------------------------------------
  const LIVE = 'oac-g8live';
  const PROJECT = 'C:\\Users\\someone\\g8live-project';
  const livePlan = (t, project = PROJECT) => driverMayAcceptLive(() => liveToolApproval(LIVE), project)(classifyForLive(t));
  const TRUST_LIVE = [
    `  ${PROJECT}`,
    '',
    '  Trust this folder? Codex can read, edit, and run files here, subject to your permission settings. Folder settings',
    '  can run code automatically, even without a model request. Continue only if you trust these files. Your trust',
    '  …',
    '› 1. Trust and continue',
    '  2. Back to Agent Command Center',
    '',
    '  enter continue · esc back',
  ].join('\n');
  const UPDATE_LIVE = ['', '  Update available · 0.164.0 → 0.164.1', `  Release notes: https://github.com/open${'a'}i/codex/releases/latest`, '', "› 1. Update now (runs `powershell -ExecutionPolicy Bypass -c 'irm x | iex'`)", '  2. Skip', '  3. Skip until next version', '', '  enter continue · esc skip'].join('\n');
  const approval = (server, tool, args = [], sel = 1) => [
    '• Calling', '', '  Field 1/1', `  Allow the ${server} MCP server to run tool "${tool}"?`, '', ...args.map(([k, v]) => `  ${k}: ${v}`), ...(args.length ? [''] : []),
    ...['Allow                   Run the tool and continue', 'Allow for this session  Run the tool and remember this choice for this session', 'Always allow            Run the tool and remember this choice for future tool calls', 'Cancel                  Cancel this tool call'].map((o, i) => `  ${i + 1 === sel ? '›' : ' '} ${i + 1}. ${o}`),
    '  enter to submit | esc to cancel',
  ].join('\n');
  const t1 = livePlan(TRUST_LIVE);
  check('g8live codex dialogs: the folder-trust screen of the project directory is accepted with Enter on the preselected "Trust and continue"; one naming another directory, or none, is refused', t1.ok && t1.keys.join() === 'enter' && t1.moves.length === 0 && !livePlan(TRUST_LIVE, 'C:\\Users\\someone\\other-project').ok && /does not name the project directory/.test(livePlan(TRUST_LIVE, 'C:\\Users\\someone\\other-project').why) && !livePlan(TRUST_LIVE, '').ok);
  const u1 = livePlan(UPDATE_LIVE);
  check('g8live codex dialogs: the update prompt is answered "2. Skip" (one down, verified, then Enter) and never "Update now" or "Skip until next version"', u1.ok && u1.answer === '2. Skip' && u1.keys.join() === 'down,enter' && !livePlan(UPDATE_LIVE.replace('› 1. Update', '  1. Update').replace('  3. Skip until', '› 3. Skip until')).ok);
  const a1 = livePlan(approval(LIVE, 'whoami'));
  const a2 = livePlan(approval(LIVE, 'send', [['to', CODEX_SID], ['content', '[{"type":"text","text":"x"}]']]));
  check('g8live codex dialogs: the tool-approval prompt of the registered server\'s OAC tools is answered "1. Allow" (this call only), with Enter on the preselected option and no selection move', a1.ok && a1.answer === '1. Allow' && a1.keys.join() === 'enter' && a1.moves.length === 0 && a2.ok && a2.toolApproval.tool === 'send' && ['whoami', 'send', 'reply', 'list_sessions'].every((t) => livePlan(approval(LIVE, t)).ok));
  const refused = (t) => { const p = livePlan(t); return !p.ok && p.keys.length === 0; };
  check('g8live codex dialogs: "Allow for this session" or "Always allow" preselected, another server, another tool, an undeclared argument and a reworded question are refused (never moved to the allow-once option)', refused(approval(LIVE, 'whoami', [], 2)) && refused(approval(LIVE, 'whoami', [], 3)) && refused(approval('some-other-server', 'whoami')) && refused(approval(LIVE, 'delete_everything')) && refused(approval(LIVE, 'whoami', [['rm', '-rf']])) && refused(approval(LIVE, 'whoami').replace('to run tool', 'to execute tool')));
  const HOOKS = ['', '  Hooks need review', '  1 hook is new or changed.', '  Hooks can run outside the sandbox after you trust them.', '', '', '› 1. Review hooks', '  2. Trust all and continue', "  3. Continue without trusting (hooks won't run)", '', '  enter confirm · esc skip'].join('\n');
  const EXEC = ['', '  Would you like to run the following command?', '', '    $ rm -rf build', '', '› 1. Yes, proceed (y)', '  2. No, and tell Codex what to do differently (esc)', '', '  Press enter to confirm or esc to cancel'].join('\n');
  const SIGNIN = ['', '  Welcome to Codex', '', '› 1. Sign in with ChatGPT', '  2. Provide your own API key', '', '  Press enter to continue'].join('\n');
  const SANDBOX = ['', '  Allow command? sandbox escalation requested', '', '› 1. Allow once', '  2. Deny', '', '  Esc to cancel'].join('\n');
  const ELICIT = ['', '  Field 1/2', '  Please enter your API key for the oac-g8live server', '', '  key: ', '', '  enter to submit | esc to cancel'].join('\n');
  check('g8live codex dialogs: the startup hook review, a sandbox or exec approval, a sign-in, a permission prompt and a free-form elicitation are all refused with no key (the run would end NOT RUN); a "Hook failed" line is not a dialog', [HOOKS, EXEC, SIGNIN, SANDBOX, ELICIT].every(refused) && classifyForLive('Hook failed - hook exited with code 1\n› Ask Codex to do anything').dialog === null);
  let hookCalls = 0;
  classifyForLive('Hook failed - hook exited with code 1', { onHookFailed: () => { hookCalls += 1; } });
  classifyForLive('nothing', { onHookFailed: () => { hookCalls += 10; } });
  check('g8live codex dialogs: a screen with a Hook failed line calls the finding hook; one without does not', hookCalls === 1);
  const rs = readFileSync(join(REPO, 'adapters', 'mcp-tools', 'src', 'lib.rs'), 'utf8');
  check('g8live codex dialogs: the tool inputs the driver may allow are the OAC tools\' declared inputs (adapters/mcp-tools/src/lib.rs names every one)', Object.keys(OAC_TOOL_INPUTS).join() === 'send,reply,list_sessions,whoami' && Object.entries(OAC_TOOL_INPUTS).every(([t, a]) => rs.includes(`"${t}"`) || rs.includes(t.toUpperCase()) ) && Object.values(OAC_TOOL_INPUTS).flat().every((n) => rs.includes(`"${n}"`)));

  // The registration: remove a leftover (a finding), add, verify; remove in teardown on every path.
  const fakeCli = (state, { addFails = false, removeThrows = false } = {}) => async (argv) => {
    const [, sub, name] = argv;
    const reply = (exitCode, stdout = '', stderr = '') => ({ exitCode, stdout, stderr, timedOut: false, spawnError: null });
    if (sub === 'get') return state.has(name) ? reply(0, `${name}\n  enabled: true\n  env: OAC_SCOPE=*****`) : reply(1, '', `No MCP server named '${name}' found.`);
    if (sub === 'remove') {
      if (removeThrows) throw new Error('spawn blew up');
      return state.delete(name) ? reply(0, `Removed global MCP server '${name}'.`) : reply(1, '', `No MCP server named '${name}' found.`);
    }
    if (sub === 'add') {
      if (addFails) return reply(1, '', 'add failed');
      state.add(name);
      return reply(0, `Added global MCP server '${name}'.`);
    }
    return reply(2);
  };
  const mk = (state, opts = {}, extra = {}) => {
    const findings = [];
    const changes = [];
    const reg = makeRegistration({ run: fakeCli(state, opts), serverName: LIVE, ns: 'g8live', oac: 'C:\\x\\oac.exe', finding: (t) => findings.push(t), changesConfig: async (why, fn) => { changes.push(why); return fn(); }, ...extra });
    return { reg, findings, changes };
  };
  const s1 = new Set();
  const r1 = mk(s1);
  await r1.reg.register();
  const added = r1.reg.commands.find((c) => c.label === 'add');
  check('g8live registration: get, remove (tolerating not found), add and get run in that order; the add names the server, OAC_SCOPE and the shim argv; every command and its output is recorded; a fresh machine has no leftover finding', r1.reg.commands.map((c) => c.label).join() === 'get (before),remove (before),add,get (after)' && added.argv.join(' ') === `codex mcp add ${LIVE} --env OAC_SCOPE=g8live -- C:\\x\\oac.exe mcp-shim --harness codex` && r1.reg.commands.every((c) => 'stdout' in c && 'stderr' in c && 'exitCode' in c) && r1.reg.commands[1].exitCode === 1 && s1.has(LIVE) && r1.findings.length === 0);
  check('g8live registration: only the commands that change the Codex configuration go through the config checkpoint (remove, add, teardown remove), never get', r1.changes.length === 2 && r1.changes.every((c) => /remove|add/.test(c)));
  const s2 = new Set([LIVE]);
  const r2 = mk(s2);
  await r2.reg.register();
  check('g8live registration: a server of the same name that already exists is removed first and recorded as a finding (a leftover of an earlier run)', r2.findings.some((f) => /already registered/.test(f)) && r2.reg.commands[1].exitCode === 0 && s2.has(LIVE));
  await r1.reg.unregister();
  const again = await r1.reg.unregister();
  check('g8live registration: teardown removes the server, checks it is gone, and a second call does nothing', !s1.has(LIVE) && r1.reg.removed === true && again === null && r1.reg.commands.filter((c) => c.label === 'remove (teardown)').length === 1);

  // Every path: an error, a NOT RUN stop, a failed add, a runner that throws, a driver that never registered.
  const onPath = async (body, opts = {}) => {
    const state = new Set();
    const m = mk(state, opts);
    try {
      await m.reg.register();
      await body();
    } catch {
      /* the scenario's own error */
    } finally {
      await m.reg.unregister();
    }
    return { state, m };
  };
  const p1 = await onPath(async () => { throw new Error('boom'); });
  const p2 = await onPath(async () => { throw new NotRunError('stopped on a dialog'); });
  const p3 = await onPath(async () => {}, { addFails: true });
  check('g8live registration: after an error, a NOT RUN stop, or a failed add (a partial registration) the server is not left registered', !p1.state.has(LIVE) && !p2.state.has(LIVE) && !p3.state.has(LIVE) && p1.m.reg.removed && p2.m.reg.removed && p3.m.reg.commands.some((c) => c.label === 'remove (teardown)'));
  const p4 = await onPath(async () => {}, { removeThrows: true });
  const never = mk(new Set());
  const nothing = await never.reg.unregister();
  check('g8live registration: a teardown whose removal fails does not throw and says so, with the command to run by hand; one that never registered runs nothing', p4.m.findings.some((f) => /remove it by hand: codex mcp remove oac-g8live/.test(f)) && nothing === null && never.reg.commands.length === 0);
  const scNoComments = code(SCENARIO);
  check('g8live registration: the scenario removes it in its finally block before recording, with no abort signal on the codex command, through the config checkpoint', scNoComments.indexOf('registration.unregister()') > scNoComments.indexOf('} finally {') && scNoComments.indexOf('registration.unregister()') < scNoComments.indexOf("ctx.record('g8live', g)") && !/runBounded\(file, a, \{[^}]*abortSignal/.test(scNoComments.slice(scNoComments.indexOf('const codexMcp'), scNoComments.indexOf('const registration'))) && /driverChangesHarnessConfig/.test(scNoComments) && /checkpointFailures/.test(read(join(REPO, 'tools', 'herdr', 'run.mjs'))) && /driverChangesHarnessConfig/.test(read(join(REPO, 'tools', 'herdr', 'run.mjs'))));
  check('g8live codex mode: driver is the default; operator keeps the old mode (the driver prints the prompts)', scenario.defaults.params.codex === 'driver' && /codexMode === 'driver'/.test(scNoComments) && /else say\(`The live daemon is ready/.test(scNoComments) && /if \(!codex\) \{\s*say\(operatorText\)/.test(scNoComments));

  // --- the outcome follows the claims (run 7, 2026-10-29: PASS with claims 2, 3, 4 and 7 not shown) ----------------------------------------------
  const st = (n, status) => ({ n, status, why: status === 'shown' ? undefined : 'x' });
  const run7 = [st(1, 'shown'), st(2, 'not-shown'), st(3, 'not-shown'), st(4, 'not-shown'), st(5, 'shown'), st(6, 'shown'), st(7, 'not-shown')];
  const allShown = [1, 2, 3, 4, 5, 6, 7].map((n) => st(n, 'shown'));
  const answeredNo = allShown.map((c) => (c.n === 5 || c.n === 6 ? st(c.n, 'answered-negative') : c));
  const openUnanswered = allShown.map((c) => (c.n === 5 ? st(5, 'not-shown') : c));
  check('g8live outcome: claims 1, 2, 3, 4 and 7 not shown make a completed run unmet (run 7\'s shape: 2, 3, 4, 7); all shown, or the open questions 5 and 6 answered no or unanswered, do not', unmetRequiredClaims(run7).map((c) => c.n).join() === '2,3,4,7' && unmetRequiredClaims(allShown).length === 0 && unmetRequiredClaims(answeredNo).length === 0 && unmetRequiredClaims(openUnanswered).length === 0 && unmetRequiredClaims(null).map((c) => c.n).join() === '1,2,3,4,7' && unmetRequiredClaims([st(2, 'not-shown')]).length === 5);
  check('g8live outcome: the scenario throws a DriverError (outcome FAIL, as g9-claude-pairing and g10-cli-verbs do on "expectations not met") after its finally block, so a stop or error keeps its own outcome and a completed run with unmet claims is not a PASS', /\} finally \{[\s\S]*unmetRequiredClaims\(g\.claims\)[\s\S]*throw new DriverError\(`G8 S6 live leg expectations not met/.test(code(SCENARIO)) && /throw new Error\('G9 leg A expectations not met/.test(read(join(REPO, 'tools', 'herdr', 'scenarios', 'g9-claude-pairing.mjs'))) && /expectations not met/.test(read(join(REPO, 'tools', 'herdr', 'scenarios', 'g10-cli-verbs.mjs'))) && /manifest\.outcome = 'FAIL'/.test(read(join(REPO, 'tools', 'herdr', 'run.mjs'))));
  check('g8live live daemon: the example holds the one local grant (this device\'s own key may write to every session of this device) and reports it, because sessions of one device need an explicit grant (SEC-AUZ-007, SEC-AUZ-010) and without it a send is unknown-destination', /Grant::Inbound/.test(read(join(REPO, 'cli', 'examples', 'live_daemon.rs'))) && /PeerSide::device\(own_key_id\)/.test(read(join(REPO, 'cli', 'examples', 'live_daemon.rs'))) && /LocalSide::Device/.test(read(join(REPO, 'cli', 'examples', 'live_daemon.rs'))) && /"local_grant"/.test(read(join(REPO, 'cli', 'examples', 'live_daemon.rs'))));

  // --- Claude's launch: one server name in .mcp.json, the channels flag and the allowed tools (run 5, 2026-10-29) ---------------------------------
  const lp = claudeLaunchPlan({ oac: 'C:\\x\\oac.exe', ns: 'g8live', explicitEnv: false, settingsPath: 'C:\\x\\settings.json' });
  const flagAt = lp.launchArgv.indexOf('--dangerously-load-development-channels');
  const allowed = lp.launchArgv[lp.launchArgv.indexOf('--allowedTools') + 1].split(',');
  const mcpTools = allowed.filter((t) => t.startsWith('mcp__'));
  check('g8live claude launch: the .mcp.json key, the server:<name> of the channels flag, the mcp__<name>__ prefix of every allowed OAC tool and the name the MCP approval is checked against are one and the same', Object.keys(lp.mcpJson.mcpServers).join() === lp.serverKey && lp.launchArgv[flagAt + 1] === `server:${lp.serverKey}` && mcpTools.length === 3 && mcpTools.every((t) => t.startsWith(`mcp__${lp.serverKey}__`)) && lp.serverKey === CLAUDE_SERVER_KEY && /driverMayAcceptExpecting\(\(\) => \[CLAUDE_SERVER_KEY\]\)/.test(SCENARIO));
  check('g8live claude launch: no --mcp-config or --strict-mcp-config (a server given only that way is "no MCP server configured with that name" to the channels flag); the shim command and the hook are the oac path; the scenario writes the .mcp.json into the session\'s working directory', !lp.launchArgv.some((a) => /mcp-config/.test(a)) && lp.mcpJson.mcpServers.oac.command === 'C:\\x\\oac.exe' && lp.mcpJson.mcpServers.oac.args.join(' ') === 'mcp-shim --harness claude' && /"C:\/x\/oac\.exe" hook/.test(lp.settingsJson.hooks.SessionStart[0].hooks[0].command) && lp.launchArgv.at(-1) === 'C:\\x\\settings.json' && /writeFileSync\(join\(claudeCwd, '\.mcp\.json'\)/.test(SCENARIO) && /cwd: claudeCwd/.test(SCENARIO));
  check('g8live claude launch: --no-chrome keeps the Claude in Chrome integration and its startup dialog off (run 6, 2026-10-29)', lp.launchArgv.includes('--no-chrome') && !lp.launchArgv.includes('--chrome'));
  const MS = ['', '  1 new MCP server found in this project', '  Select any you wish to enable.', '', '  MCP servers may execute code or access system resources. All tool calls require approval. Learn more in the MCP', '  documentation.', '', '  ❯ [✔] oac', '     Enable selected', ' Space to select · Esc to reject all'].join('\n');
  check('g8live claude launch: the project-server approval that .mcp.json brings is answered only when it lists exactly the server (oac); one that lists another server is refused', driverMayAccept(classifyScreen(MS), { expectedMcpServers: [CLAUDE_SERVER_KEY] }).ok && !driverMayAccept(classifyScreen(MS.replace('[✔] oac', '[✔] other')), { expectedMcpServers: [CLAUDE_SERVER_KEY] }).ok);
  // Login: Claude's own report, the driver never signs in.
  const LOGIN1 = ['● Login expired · Please run /login', '', 'Not logged in · Run /login'].join('\n');
  check('g8live claude auth state: "Login expired", "Not logged in" and "Please run /login" are recognised; an ordinary screen and the OAC prompt text are not', [LOGIN1, 'Not logged in · Run /login', '● Login expired'].every((t) => CLAUDE_NOT_LOGGED_IN.test(t)) && !CLAUDE_NOT_LOGGED_IN.test('Claude Code v2.1.285 / Opus 5.5 (1M context) · Claude Max') && !CLAUDE_NOT_LOGGED_IN.test(claudePrompt({ nonce: NONCE, codexSessionId: CODEX_SID })) && CLAUDE_NOT_LOGGED_IN_STOP === 'Claude Code is not logged in (the operator must log in; the driver never signs in)');
  check('g8live claude auth state: every Claude read stops the run NOT RUN at once with that text (no wait for the session id), after a finding', /CLAUDE_NOT_LOGGED_IN\.test\(String\(t \?\? ''\)\)/.test(SCENARIO) && /stop\(CLAUDE_NOT_LOGGED_IN_STOP\)/.test(SCENARIO) && /never signs in/.test(SCENARIO));

  // --- Claude Code's Settings Warning (G8 S6 run 4, 2026-10-29) -------------------------------------------------------------------------------
  const SETTINGS_WARNING = [
    '',
    '  Settings Warning',
    '',
    '  C:\\Users\\someone\\.claude\\settings.json',
    '  └ permissions.allow: Invalid permission rule "codex exec" was skipped: Tool names must start with uppercase. Use',
    '    "Codex exec"',
    '',
    '  The values listed above were skipped; the rest of the file is in effect.',
    '',
    '  ❯ 1. Continue',
    '    2. Fix with Claude',
    '    3. Exit and fix manually',
    '',
    '  Enter to confirm · Esc to cancel',
  ].join('\n');
  const swPlan = (t) => driverMayAccept(classifyScreen(t));
  const sw = swPlan(SETTINGS_WARNING);
  check('g8live claude settings-warning: the recorded screen is a known kind and is accepted with Enter on the preselected "1. Continue", no selection move', classifyScreen(SETTINGS_WARNING).dialog === 'settings-warning' && sw.ok && sw.keys.join() === 'enter' && sw.moves.length === 0);
  const swRefused = (t) => { const p = swPlan(t); return !p.ok && p.keys.length === 0; };
  check('g8live claude settings-warning: "Fix with Claude" or "Exit" preselected, an extra option, a missing option, another warning or a changed sentence is refused with no key', swRefused(SETTINGS_WARNING.replace('❯ 1. Continue', '  1. Continue').replace('    2. Fix with Claude', '  ❯ 2. Fix with Claude')) && swRefused(SETTINGS_WARNING.replace('❯ 1. Continue', '  1. Continue').replace('    3. Exit and fix manually', '  ❯ 3. Exit and fix manually')) && swRefused(SETTINGS_WARNING.replace('    3. Exit and fix manually', '    3. Exit and fix manually\n    4. Open the file')) && swRefused(SETTINGS_WARNING.replace('    2. Fix with Claude\n', '')) && swRefused(SETTINGS_WARNING.replace('Invalid permission rule', 'Unknown setting')) && swRefused(SETTINGS_WARNING.replace('the rest of the file is in effect', 'the whole file was dropped')) && swRefused(SETTINGS_WARNING.replace('Continue\n', 'Continue anyway\n')));
  const rules = settingsWarningRules(SETTINGS_WARNING);
  check('g8live claude settings-warning: the skipped rule is read, its wrapped line joined, for the finding; a screen without the list gives none', rules.length === 1 && rules[0] === 'permissions.allow: Invalid permission rule "codex exec" was skipped: Tool names must start with uppercase. Use "Codex exec"' && settingsWarningRules('nothing').length === 0 && /operator settings: \$\{rule\} \(Claude Code skipped it; the run continues\)/.test(SCENARIO));

  // --- the capture client's third mode ---------------------------------------------------------------------------------------------------------------
  check('g8live client: midwatch is a mode of the passive capture client and shares watch\'s code (thread/resume with only threadId and excludeTurns); it sends no method that changes a thread', /'midwatch'/.test(code(CLIENT)) && /mode !== 'list' && !threadId/.test(CLIENT) && !/'(?:turn\/start|turn\/steer|thread\/queue\/add|turn\/interrupt|thread\/start|thread\/fork)'/.test(code(CLIENT)));

  // --- the scenario's guards -----------------------------------------------------------------------------------------------------------------------------
  const sc = code(SCENARIO);
  check('g8live scenario: it never calls turn/steer or any method that writes into a thread, and answers no server request', !/['"](?:turn\/steer|turn\/start|turn\/interrupt|thread\/queue\/add|thread\/start|thread\/fork)['"]/.test(sc) && !/elicitation|serverRequest|resolveServerRequest/.test(sc));
  check('g8live scenario: it never runs codex mcp, nor daemon start, stop or restart (only daemon version), and never signs in', !/'mcp'|'remove'|'login'|'logout'/.test(sc) && !/'daemon',\s*'(?:start|stop|restart)'/.test(sc) && /codexCli\(\['app-server', 'daemon', 'version'\]/.test(sc));
  check('g8live scenario: it reads no credential, sqlite, rollout or Codex-home file and no OS credential store', !/auth\.json|\.sqlite|rollout|CODEX_HOME|app-server-control|\.codex\b/.test(sc) && !new RegExp(['key', 'ring'].join('')).test(sc));
  check('g8live scenario: its panes are the daemon\'s, Claude\'s and (driver mode) Codex\'s; the only agents are CLAUDE_AGENT and CODEX_AGENT; the Codex TUI is typed into only through codex.prompt (codexInput), by the driver; the only paneRun target is the daemon pane', !/sendKeys|startAgent/.test(sc) && [...sc.matchAll(/paneRun\(([^,]+),/g)].every((m) => /^daemonPane$/.test(m[1])) && [...sc.matchAll(/agentStart\(([^,]+),/g)].every((m) => /^(?:CLAUDE_AGENT|CODEX_AGENT)$/.test(m[1])) && !/agentPrompt\(/.test(sc) && (sc.match(/workspaceCreate\(/g) ?? []).length === 3 && /launchArgv: \['codex'\]/.test(sc));
  check('g8live scenario: thread ids, session ids, fingerprints, pairing values and the project directory are registered for redaction before any capture is written', ['<THREAD_ID>', '<SESSION_ID>', '<DEVICE_FINGERPRINT>', '<PAIRING_VALUE>', '<PROJECT_DIR>'].every((p) => sc.includes(`'${p}'`)) && sc.indexOf("'<PAIRING_VALUE>'") < sc.indexOf('ctx.capture(g.captureNames.transcript') && /sanitizeTranscript/.test(sc) && /otherThreadIds/.test(sc));
  check('g8live scenario: the Claude session is interactive (no -p, stream-json, verbose or max-turns) and keeps the development-channels flag, the settings hook and allowedTools, with the server in a project .mcp.json (not --mcp-config); it goes through makeAgent, records dialogs before the driver answers them and exits with /exit', !/'-p'|'stream-json'|'--verbose'|'--max-turns'/.test(sc) && /'--dangerously-load-development-channels'/.test(sc) && !/--mcp-config|--strict-mcp-config/.test(sc) && /'\.mcp\.json'/.test(sc) && /'--settings'/.test(sc) && /'--allowedTools'/.test(sc) && /SessionStart/.test(sc) && /makeAgent\(/.test(sc) && /DIALOG_KINDS/.test(sc) && /classifyScreen/.test(sc) && /claude\.prompt\('\/exit'\)/.test(sc) && /claude\.settle\('startup'/.test(sc) && sc.indexOf("claude.settle('startup'") < sc.indexOf('claude.prompt(claudePrompt'));
  check('g8live scenario: the analysis and the claims run in the finally block, on every path', /try \{\s*analyze\(\);/.test(sc) && sc.indexOf('analyze();') > sc.indexOf('} finally {') && /claude\.sections/.test(sc));
  check('g8live scenario: a refusal with no pairing value ends the wait at once (no pairTimeoutMs wait) and records claim 6 as answered no', /refusedWithoutValue\(cs\)/.test(sc) && /noValueRefusal/.test(sc) && /stop\('the Codex whoami was refused with no pairing value/.test(sc));
  check('g8live claude prompt: the first wait allows at least 60 sleeps', /Step 2: run the Bash command sleep 10 again and again, at most (\d+) times/.exec(claudePrompt({ nonce: NONCE, codexSessionId: CODEX_SID }))?.[1] >= 60);
  check('g8live scenario: it is named, not verdict-bearing and records its claims', scenario.name === 'g8-codex-pairing' && /LIVE_NON_VERDICT/.test(sc) && /ctx\.record\('g8live', g\)/.test(sc) && scenario.harnesses.join() === 'codex,claude');
  const refusals = [{}, { oac: 'relative', daemon: '/x', projectDir: '/y' }, { oac: '/o', daemon: '/d', projectDir: '/p', namespace: 'Bad NS' }];
  return Promise.all(refusals.map(async (params) => {
    try {
      await scenario.run({ herdr: {}, params: { ...scenario.defaults.params, ...params } });
      return false;
    } catch (e) {
      return /--param .* is required|namespace must be/.test(String(e.message));
    }
  })).then((r) => check('g8live scenario: missing, relative or malformed parameters are refused before anything starts', r.every(Boolean)));
}
