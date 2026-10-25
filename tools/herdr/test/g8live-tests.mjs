// Unit checks for the G8 slice S6 live leg (#69/#433): scenarios/g8-codex-pairing.mjs (the analysis
// helpers, the prompts and the scenario's guards) and the `midwatch` mode of gate-servers/
// g8-capture.mjs. Run by run.mjs --self-test (unit half); the scenario has no lifecycle case: it
// needs an operator's Codex TUI and a logged-in Claude Code. The inputs below are test inputs for the
// pure analysers, not harness fixtures.

import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import scenario, {
  liveNames, claudePrompt, handoffText, pairingPrompt, sendPrompt, parseFrame, oacCalls, pairingSequence, handoffFrames, echoTrials, testNumberOf,
  midTurnVerdict, carrierAncestry, claudeEvidence, claudeEvidenceText, evaluateClaims, sanitizeDaemonEvents, jsonLines, PAIRING_VALUE,
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

export function g8liveUnit(check) {
  // --- names, prompts --------------------------------------------------------------------------------
  check('g8live names: verified versions give fixture names, otherwise unverified-*', liveNames('2026-10-24', '0.163.0').transcript === 'transcript-g8live-2026-10-24-0.163.0-herdr.jsonl' && liveNames('2026-10-24', null).events === 'events-g8live-unverified-2026-10-24.jsonl');
  const prompt = claudePrompt({ nonce: NONCE, codexSessionId: CODEX_SID });
  check('g8live claude prompt: one line with no quote, backtick, dollar sign, pipe or redirection, so a pane shell cannot change it', !/["'`$|<>\n\r]/.test(prompt) && prompt.includes(CODEX_SID) && prompt.includes(`CLAUDE-DONE-${NONCE}`) && prompt.includes(`Never reply to a message`));
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

  // --- the Claude session's report ----------------------------------------------------------------------------------------------------
  const echoOfPrompt = `PS C:\\> claude -p "${claudePrompt({ nonce: NONCE, codexSessionId: CODEX_SID })}"`;
  const stream = [
    JSON.stringify({ type: 'assistant', message: { content: [{ type: 'text', text: `CLAUDE-SID-${NONCE}: ${CLAUDE_SID}` }] } }),
    JSON.stringify({ type: 'user', message: { content: [{ type: 'text', text: '<channel source="oac" oac_sender="S" oac_message_id="M">G8LIVE x</channel>' }] } }),
    JSON.stringify({ type: 'assistant', message: { content: [{ type: 'text', text: `RECEIVED-${NONCE}: oac_sender=S oac_message_id=M G8LIVE ${NONCE} FROM-CODEX\nSENT-${NONCE}-1: id1\nSENT-${NONCE}-2: id2\nCLAUDE-DONE-${NONCE}` }] } }),
  ].join('\n');
  const evPromptOnly = claudeEvidence(echoOfPrompt, NONCE);
  const ev = claudeEvidence(`${echoOfPrompt}\n${stream}`, NONCE);
  check('g8live claude report: the shell\'s echo of the prompt is not a report (no id, no received line, not done)', evPromptOnly.sessionId === null && evPromptOnly.received.length === 0 && evPromptOnly.sent.length === 0 && evPromptOnly.done === false);
  check('g8live claude report: the session id, the received line, the sent ids, the channel tag attributes and the done line are read from stream-json lines', ev.sessionId === CLAUDE_SID && ev.received.length === 1 && ev.received[0].line.includes('FROM-CODEX') && ev.sent.map((s) => s.n).join() === '1,2' && ev.done === true && ev.channelTags.some((t) => t.join() === 'source,oac_sender,oac_message_id'));
  const ct = claudeEvidenceText(`${echoOfPrompt}\n${stream}`, NONCE);
  check('g8live claude capture: only the report lines are kept, not the stream', /^CLAUDE-SID-/.test(ct) && !/"type":"assistant"/.test(ct) && ct.split('\n').length <= 8);

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

  // --- the capture client's third mode ---------------------------------------------------------------------------------------------------------------
  check('g8live client: midwatch is a mode of the passive capture client and shares watch\'s code (thread/resume with only threadId and excludeTurns); it sends no method that changes a thread', /'midwatch'/.test(code(CLIENT)) && /mode !== 'list' && !threadId/.test(CLIENT) && !/'(?:turn\/start|turn\/steer|thread\/queue\/add|turn\/interrupt|thread\/start|thread\/fork)'/.test(code(CLIENT)));

  // --- the scenario's guards -----------------------------------------------------------------------------------------------------------------------------
  const sc = code(SCENARIO);
  check('g8live scenario: it never calls turn/steer or any method that writes into a thread, and answers no server request', !/['"](?:turn\/steer|turn\/start|turn\/interrupt|thread\/queue\/add|thread\/start|thread\/fork)['"]/.test(sc) && !/elicitation|serverRequest|resolveServerRequest/.test(sc));
  check('g8live scenario: it never runs codex mcp, nor daemon start, stop or restart (only daemon version), and never signs in', !/'mcp'|'remove'|'login'|'logout'/.test(sc) && !/'daemon',\s*'(?:start|stop|restart)'/.test(sc) && /codexCli\(\['app-server', 'daemon', 'version'\]/.test(sc));
  check('g8live scenario: it reads no credential, sqlite, rollout or Codex-home file and no OS credential store', !/auth\.json|\.sqlite|rollout|CODEX_HOME|app-server-control|\.codex\b/.test(sc) && !new RegExp(['key', 'ring'].join('')).test(sc));
  check('g8live scenario: it never types into the Codex TUI: its panes are the daemon\'s and Claude\'s', !/agentPrompt|sendKeys|agentStart|startAgent/.test(sc) && [...sc.matchAll(/paneRun\(([^,]+),/g)].every((m) => /^(?:daemonPane|wsB\.paneId)$/.test(m[1])) && /paneRun\(daemonPane/.test(sc) && /paneRun\(wsB\.paneId/.test(sc));
  check('g8live scenario: thread ids, session ids, fingerprints, pairing values and the project directory are registered for redaction before any capture is written', ['<THREAD_ID>', '<SESSION_ID>', '<DEVICE_FINGERPRINT>', '<PAIRING_VALUE>', '<PROJECT_DIR>'].every((p) => sc.includes(`'${p}'`)) && sc.indexOf("'<PAIRING_VALUE>'") < sc.indexOf('ctx.capture(g.captureNames.transcript') && /sanitizeTranscript/.test(sc) && /otherThreadIds/.test(sc));
  check('g8live scenario: the Claude session is print mode with stream-json, the development-channels flag for the oac server, a strict MCP config and a SessionStart hook (the G9 leg A recipe)', /'-p'/.test(sc) && /'stream-json'/.test(sc) && /'--dangerously-load-development-channels'/.test(sc) && /'--strict-mcp-config'/.test(sc) && /SessionStart/.test(sc));
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
