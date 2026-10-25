// g8-codex-pairing: the G8 slice S6 live leg (#69 box 1, live confirmation of #69 boxes 2 and 4,
// #433 box 3), driven through herdr. NOT A GATE RUN and NOT VERDICT-BEARING: it records what a
// real Codex and a real Claude Code do with the OAC shim, the example live daemon and the Codex
// adapter, on the machine it runs on. It never changes a gate verdict, STATUS.md's gate table or
// PINS.md.
//
// Three parties:
//   - the example live daemon (cli/examples/live_daemon.rs: the product's Daemon::start with an
//     in-memory key, the in-memory transport, a test namespace, the Claude and Codex adapters; the
//     Codex carrier is the operator's real control socket, opened read-only in the product's way),
//     in a herdr pane;
//   - a real Claude Code in print mode, as in the G9 leg A recipe (g9-claude-pairing), in a herdr
//     pane, with the OAC shim and the hook handler, started by the driver once Codex is paired;
//   - the operator's own Codex TUI, in the project directory: the driver launches no Codex, types
//     nothing into it, accepts no dialog and never answers a server request. The operator
//     registers OAC as an MCP server named oac-g8live with the Codex CLI (OAC_SCOPE=<ns> in its
//     environment; command: the absolute path of oac with `mcp-shim --harness codex`) before the run
//     and removes it afterwards, accepts every dialog
//     (this once, never "Always") and types the prompts the driver prints. The steps are in the
//     operator script (g8-live-operator-steps.md). The driver never registers or removes an MCP
//     server, never runs the daemon's start or stop verbs (it runs only `codex app-server daemon
//     version`), never starts or stops Codex, and never signs in. It does not call turn/steer or
//     any other method that writes into a thread: its Codex-facing clients are the passive
//     `watch` and `midwatch` modes of gate-servers/g8-capture.mjs (thread/resume only).
//
// Operator command:
//
//   node tools/herdr/run.mjs --scenario g8-codex-pairing --out <run dir> \
//     --param oac=<abs path to oac.exe> --param daemon=<abs path to the live_daemon example> \
//     --param projectDir=<abs path of the directory the operator's TUI runs in> \
//     [--param namespace=g8live]
//
// What it shows, each with its evidence in the run manifest (scenarioData.g8live.claims):
//   1. The Codex refusal, then the pairing, then `whoami` returning a session id (mcpToolCall
//      items of the registered server on a passive watch connection).
//   2. A Codex `send` reaching the Claude session (the `send` result and the Claude session's own
//      report of the channel message).
//   3. A Claude-to-Codex hand-off arriving in the Codex thread (a userMessage item) with the C6
//      frame: the provenance header outside the delimited, line-quoted body.
//   4. A Codex `reply` with and without `in_reply_to`, reported correlated and uncorrelated.
//   5. Open question A: a `thread/resume` made while the thread's first OAC tool call is in flight
//      (`midwatch`, started when that call's item/started shows) and whether it still receives
//      that call's item/completed; the adapter's own mid-turn resume is the second witness (the
//      pairing completing at all).
//   6. Open question B (11-risks row 89): the shim's process chain (pid, parent, image), the
//      nearest `codex` ancestor, the control-socket server's pid from the carrier opener, and
//      whether they are the same process.
//   7. How often Codex echoes `oac_message_id` in `in_reply_to` (risk row 17), per hand-off, with
//      the instruction each hand-off gave.
//
// Redaction is driver-side, as in g8-codex-tools: thread ids, session ids, device fingerprints and
// pairing values are registered as run literals before any capture is written, the shared
// sanitizeTranscript drops other threads' list entries and `readStates`, and other loaded threads'
// ids are replaced. Credentials: nothing here reads a file under the Codex home or asks an OS
// credential store for anything; the rollout files are never opened. The control socket path is
// read by the daemon, not by this driver.

import { existsSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { basename, dirname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomBytes } from 'node:crypto';

import { NotRunError, DriverError } from '../lib/herdr.mjs';
import { parseCodexVersions, pinsReadWarning, parseCodexCliVersion, parseCodexDaemonVersion, codexVersionWarning, CODEX_PIN_ROW, CODEX_DAEMON_VERSION_FIELDS } from '../lib/pins.mjs';
import { runBounded, spawnLongRunning, killTree, within, processTable } from '../lib/proc.mjs';
import { CODEX_DAEMON_SCRATCH_HOLDER } from '../lib/scratch.mjs';
import { committedFile } from '../lib/g1.mjs';
import { stageGateFiles } from '../lib/gate-common.mjs';
import { regexLiteral } from '../lib/pane-shell.mjs';
import { PINS_PATH, parseG2Transcript, g2Facts, identifyTuiThread, sanitizeTranscript } from '../lib/g2.mjs';
import { mcpItems, otherThreadIds, processTreeSnapshot } from './g8-codex-tools.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');

export const LIVE_GATE = 'G8 S6 live leg';
export const LIVE_NON_VERDICT = '#69/#433 G8 slice S6: a live leg of the Codex pairing and hand-off; compared against no other run, not a gate run, never changes a gate verdict';
export const LIVE_CLIENT_FILES = Object.freeze(['g8-capture.mjs']);
export const DEFAULT_SERVER_NAME = 'oac-g8live';
export const DEFAULT_MARKER = 'G8 live leg marker: reply with the single word ready.';
// The text of the three Claude-to-Codex hand-offs, by test number. Each says how the reply is to be made;
// the third gives no instruction, to see whether Codex names the message by itself (risk row 17).
export const HANDOFF_INSTRUCTION = Object.freeze({
  1: 'in_reply_to',
  2: 'to-only',
  3: 'unspecified',
});

const sleep = (ms) => new Promise((r) => setTimeout(r, Math.max(0, ms)));

// --- capture names ---------------------------------------------------------------------------------

export function liveNames(date, version) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) throw new Error(`bad date ${JSON.stringify(date)}`);
  const v = /^\d+\.\d+\.\d+$/.test(version ?? '') ? version : null;
  const tail = v ? `${date}-${v}` : `unverified-${date}`;
  return {
    transcript: `transcript-g8live-${tail}-herdr.jsonl`,
    events: `events-g8live-${tail}.jsonl`,
    claude: `claude-g8live-${tail}.txt`,
    proc: `transcript-g8live-proc-${tail}.jsonl`,
  };
}

// --- the prompts the operator types and the prompt Claude runs ---------------------------------------

// What the operator types into the Codex TUI for the pairing (claim 1). The refusal text allows one
// more call (spec/bindings/mcp.md 4.5.3 step 4).
export const pairingPrompt = (serverName) =>
  `Call the whoami tool of the ${serverName} MCP server. If it is refused as unauthorized, call whoami once more with no arguments. Then tell me the session_id it returned.`;

// What the operator types for the Codex-to-Claude message (claim 2).
export const sendPrompt = (serverName, claudeSessionId, nonce) =>
  `Call the send tool of the ${serverName} MCP server once, with to set to ${claudeSessionId} and content set to one text part with exactly this text: G8LIVE ${nonce} FROM-CODEX. Then say done and stop.`;

export const handoffText = (nonce, n) => {
  if (n === 1) return `G8LIVE ${nonce} T1 Please answer this message with the OAC reply tool: put the oac_message_id of this message in in_reply_to, and use the content ACK1.`;
  if (n === 2) return `G8LIVE ${nonce} T2 Please answer this message with the OAC reply tool: name my session (the oac_sender of this message) in to, give no in_reply_to, and use the content ACK2.`;
  return `G8LIVE ${nonce} T3 Please answer this message.`;
};

// The one-line prompt of the Claude Code print-mode session. No quote, backtick, dollar sign or line
// break, so that it survives a pane shell's quoting unchanged. Claude's own report lines carry the
// nonce, so a line the shell echoes is not mistaken for one.
export function claudePrompt({ nonce, codexSessionId, maxFirstSleeps = 20, maxLaterSleeps = 30 }) {
  return [
    `You are the Claude side of an OAC live test, nonce ${nonce}. Do these steps in order.`,
    `Step 1: call the whoami tool of the oac MCP server and print one line: CLAUDE-SID-${nonce}: followed by the session_id it returned.`,
    `Step 2: run the Bash command sleep 10 again and again, at most ${maxFirstSleeps} times, stopping as soon as a channel message from another OAC session has reached you.`,
    `Whenever a channel message reaches you, at any step, print one line: RECEIVED-${nonce}: followed by its oac_sender, its oac_message_id and its text. Never reply to a message.`,
    `Step 3: call the send tool of the oac MCP server three times, to the session ${codexSessionId}, each time with one text part, in this order.`,
    `Text 1: ${handoffText(nonce, 1)}`,
    `Text 2: ${handoffText(nonce, 2)}`,
    `Text 3: ${handoffText(nonce, 3)}`,
    `After each send print one line: SENT-${nonce}-N: followed by the id the send result returned, where N is 1, 2 or 3.`,
    `Step 4: run sleep 10 again and again, at most ${maxLaterSleeps} times, stopping once three more channel messages have reached you.`,
    `Step 5: print the line CLAUDE-DONE-${nonce}.`,
  ].join(' ');
}

// --- the daemon's events file ---------------------------------------------------------------------------

export function jsonLines(text) {
  const out = [];
  for (const line of String(text ?? '').split(/\r?\n/)) {
    if (!line.trim().startsWith('{')) continue;
    try {
      out.push(JSON.parse(line));
    } catch {
      /* a partial line */
    }
  }
  return out;
}

// The daemon's events as they may be captured: pid, parent-free chains by image name, no resolved path.
export function sanitizeDaemonEvents(events) {
  const chain = (c) => (Array.isArray(c) ? c.map((x) => ({ pid: x?.pid ?? null, image: typeof x?.image === 'string' ? basename(x.image.replace(/\\/g, '/')) : null })) : []);
  return events.map((e) => {
    switch (e.event) {
      case 'accepted':
        return { event: e.event, unix_ms: e.unix_ms, label: e.label, peer: e.peer ? { pid: e.peer.pid } : null, chain: chain(e.chain), harness: e.harness ? { pid: e.harness.pid } : null, paired: e.paired };
      case 'ended':
        return { event: e.event, unix_ms: e.unix_ms, label: e.label, peer: e.peer ? { pid: e.peer.pid } : null };
      case 'carrier_server':
        return { event: e.event, unix_ms: e.unix_ms, pid: e.pid ?? null, chain: chain(e.chain), pairing_key: e.pairing_key ?? null };
      case 'ready':
        return { event: e.event, unix_ms: e.unix_ms, namespace: e.namespace, pid: e.pid };
      case 'log':
        return { event: e.event, unix_ms: e.unix_ms, line: e.line };
      default:
        return { event: e.event, unix_ms: e.unix_ms };
    }
  });
}

// --- the OAC tool calls on a watch connection ------------------------------------------------------------

// The mcpToolCall items of `serverName` on connections of `mode`, in the order their first frame arrived.
export function oacCalls(entries, serverName, mode = 'watch') {
  return mcpItems(entries.filter((e) => e.mode === mode))
    .map((it) => {
      const s = it.started;
      const c = it.completed;
      const f = c ?? s;
      return {
        itemId: it.itemId,
        server: f?.server ?? null,
        tool: f?.tool ?? null,
        turnId: f?.turnId ?? null,
        startedLine: s?.line ?? null,
        startedT: s?.t ?? null,
        completedLine: c?.line ?? null,
        completedT: c?.t ?? null,
        status: c?.status ?? null,
        arguments: (s ?? c)?.arguments ?? null,
        argumentsKind: (s ?? c)?.argumentsKind ?? null,
        resultText: c?.resultContentText ?? null,
        isError: c?.isError ?? null,
      };
    })
    .filter((c) => c.server === serverName);
}

export const PAIRING_VALUE = /oac-pair-[0-9a-f]{32}/g;
const SESSION_ID_IN_TEXT = /"session_id"\s*:\s*"([^"]+)"/;
const DEVICE_IN_TEXT = /"device_fingerprint"\s*:\s*"([^"]+)"/;

// Claim 1: the refusal, then the pairing, then whoami returning a session id.
export function pairingSequence(calls) {
  const who = calls.filter((c) => c.tool === 'whoami');
  const refusal = who.find((c) => c.completedLine !== null && /unauthorized/i.test(c.resultText ?? '') && new RegExp(PAIRING_VALUE.source).test(c.resultText ?? '')) ?? null;
  const after = refusal ? who.filter((c) => c.completedLine !== null && c.completedLine > refusal.completedLine) : [];
  const paired = after.find((c) => SESSION_ID_IN_TEXT.test(c.resultText ?? '')) ?? null;
  return {
    refusal: refusal && { itemId: refusal.itemId, status: refusal.status, completedLine: refusal.completedLine, carriesPairingValue: true },
    paired: paired && { itemId: paired.itemId, status: paired.status, completedLine: paired.completedLine },
    codexSessionId: paired ? SESSION_ID_IN_TEXT.exec(paired.resultText)[1] : null,
    deviceFingerprint: paired ? DEVICE_IN_TEXT.exec(paired.resultText)?.[1] ?? null : null,
    refusedThenPaired: !!(refusal && paired),
  };
}

// --- the hand-off frame (C6 5.0) --------------------------------------------------------------------

const KEYS = ['oac_sender', 'oac_device', 'oac_session', 'oac_message_id', 'oac_reply_to'];

// The frame a hand-off is written as, checked from its text alone. -> { found, ok, problems, delimiter,
// header, headerKeysInOrder, bodyLineCount, bodyAllQuoted, endMarker, provenanceOutsideBody }.
export function parseFrame(text) {
  const lines = String(text ?? '').replace(/\r\n/g, '\n').split('\n');
  const start = lines.findIndex((l) => /^--- oac-envelope \S+ ---$/.test(l));
  const base = { found: start >= 0, ok: false, problems: [], delimiter: null, header: {}, headerKeysInOrder: false, bodyLineCount: 0, bodyAllQuoted: false, endMarker: false, provenanceOutsideBody: false };
  if (start < 0) {
    base.problems.push('no envelope line');
    return base;
  }
  const delimiter = /^--- oac-envelope (\S+) ---$/.exec(lines[start])[1];
  base.delimiter = delimiter;
  const bodyAt = lines.findIndex((l, i) => i > start && new RegExp(`^--- oac-body ${regexLiteral(delimiter)}(?: |$)`).test(l));
  const endAt = lines.findIndex((l, i) => i > start && l === `--- oac-end ${delimiter} ---`);
  if (bodyAt < 0) base.problems.push('no body line with the same delimiter');
  if (endAt < 0) base.problems.push('no end line with the same delimiter');
  else base.endMarker = true;
  const headerLines = bodyAt >= 0 ? lines.slice(start + 1, bodyAt) : [];
  const seen = [];
  for (const l of headerLines) {
    const m = /^(oac_[a-z_]+): ?(.*)$/.exec(l);
    if (!m) {
      base.problems.push('a header line is not key: value');
      continue;
    }
    seen.push(m[1]);
    base.header[m[1]] = m[2];
  }
  base.headerKeysInOrder = seen.join() === KEYS.join();
  if (!base.headerKeysInOrder) base.problems.push(`header keys are ${seen.join(',') || 'none'}`);
  const body = bodyAt >= 0 && endAt > bodyAt ? lines.slice(bodyAt + 1, endAt) : [];
  base.bodyLineCount = body.length;
  base.bodyAllQuoted = body.length > 0 && body.every((l) => l === '|' || l.startsWith('| '));
  if (!base.bodyAllQuoted) base.problems.push('a body line is not line-quoted');
  // Provenance is outside the body when every header key sits before the body marker and no body line is a header line.
  base.provenanceOutsideBody = base.headerKeysInOrder && !body.some((l) => /^oac_[a-z_]+:/.test(l));
  base.ok = base.problems.length === 0 && base.provenanceOutsideBody;
  return base;
}

// The hand-offs that arrived in the thread: userMessage items whose text holds an envelope line,
// once each (item/started and item/completed of one message are one).
export function handoffFrames(entries, mode = 'watch') {
  const f = g2Facts(entries.filter((e) => e.mode === mode));
  const seen = new Set();
  const out = [];
  for (const u of f.events.userItems) {
    if (!/^--- oac-envelope \S+ ---$/m.test(u.text)) continue;
    const frame = parseFrame(u.text);
    const key = `${u.turnId}\0${frame.header.oac_message_id ?? u.text.length}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({ line: u.line, t: u.t, turnId: u.turnId, threadId: u.threadId, messageId: frame.header.oac_message_id ?? null, sender: frame.header.oac_sender ?? null, frame, text: u.text });
  }
  return out;
}

// Which test (1, 2 or 3) a hand-off was, from the quoted body's own marker.
export const testNumberOf = (text, nonce) => {
  const m = new RegExp(`G8LIVE ${regexLiteral(nonce)} T([123])\\b`).exec(String(text ?? ''));
  return m ? Number(m[1]) : null;
};

// Claim 4 and 7: for each hand-off, the Codex `reply` calls made in its turn, whether they named the
// message, and how the reply was reported.
export function echoTrials(frames, replyCalls, nonce) {
  return frames.map((fr) => {
    const test = testNumberOf(fr.text, nonce);
    const mine = replyCalls.filter((c) => c.tool === 'reply' && c.turnId === fr.turnId);
    const first = mine[0] ?? null;
    const args = first && first.arguments && typeof first.arguments === 'object' ? first.arguments : {};
    const named = typeof args.in_reply_to === 'string' && args.in_reply_to !== '' ? args.in_reply_to : null;
    let correlation = null;
    if (first?.resultText) {
      try {
        correlation = JSON.parse(first.resultText).correlation ?? null;
      } catch {
        correlation = /"correlation"\s*:\s*"(\w+)"/.exec(first.resultText)?.[1] ?? null;
      }
    }
    return {
      test,
      instruction: HANDOFF_INSTRUCTION[test] ?? null,
      messageId: fr.messageId,
      frameOk: fr.frame.ok,
      replyCalls: mine.length,
      replied: !!first,
      inReplyTo: named === null ? 'absent' : named === fr.messageId ? 'echoed-the-id' : 'other-value',
      toPresent: typeof args.to === 'string' && args.to !== '',
      replyStatus: first?.status ?? null,
      reportedCorrelation: correlation,
    };
  });
}

// --- open question A ---------------------------------------------------------------------------------------

// A: the watch connection that was subscribed before the call; B: the `midwatch` connection started once
// the call's item/started had shown. `item` is A's call; `bEntries` are B's transcript entries.
// -> { verdict: 'received' | 'not-received' | 'not-exercised', why, ... }
export function midTurnVerdict({ item, bEntries }) {
  if (!item || !item.startedT) return { verdict: 'not-exercised', why: 'no OAC tool call was seen to start on the watch connection' };
  const resumed = (() => {
    // the answer to B's thread/resume: the first daemon->client frame with a result after its request
    const req = bEntries.find((e) => e.mode === 'midwatch' && e.direction === 'client->daemon' && e.payload?.method === 'thread/resume');
    if (!req) return null;
    return bEntries.find((e) => e.mode === 'midwatch' && e.direction === 'daemon->client' && e.payload?.id === req.payload.id && !e.payload.method) ?? null;
  })();
  if (!resumed) return { verdict: 'not-exercised', why: 'the midwatch connection never got an answer to its thread/resume' };
  if (resumed.payload?.error) return { verdict: 'not-exercised', why: 'the midwatch thread/resume was answered with an error' };
  const tR = Date.parse(resumed.t);
  const tS = Date.parse(item.startedT);
  const tC = item.completedT ? Date.parse(item.completedT) : Infinity;
  const got = bEntries.filter((e) => e.mode === 'midwatch' && e.direction === 'daemon->client' && e.payload?.method === 'item/completed' && e.payload.params?.item?.id === item.itemId);
  const gotStarted = bEntries.filter((e) => e.mode === 'midwatch' && e.direction === 'daemon->client' && e.payload?.method === 'item/started' && e.payload.params?.item?.id === item.itemId);
  const mid = tR > tS && tR < tC;
  const base = { resumeAnsweredT: resumed.t, callStartedT: item.startedT, callCompletedT: item.completedT, resumeMsAfterStart: tR - tS, resumeMsBeforeCompleted: Number.isFinite(tC) ? tC - tR : null, itemCompletedOnB: got.length, itemStartedOnB: gotStarted.length, itemId: '<ITEM_ID>' };
  if (!mid) return { verdict: 'not-exercised', why: tR <= tS ? 'the resume was answered before the call started' : 'the call had completed before the resume was answered', ...base };
  if (got.length > 0) return { verdict: 'received', why: 'a thread/resume answered while the call was in flight received its item/completed', ...base };
  return { verdict: 'not-received', why: 'a thread/resume answered while the call was in flight did not receive its item/completed', ...base };
}

// --- open question B ---------------------------------------------------------------------------------------

const isCodexImage = (image) => /^codex(?:\.exe|\.js)?$/i.test(String(image ?? ''));

// The shim's nearest `codex` ancestor against the control-socket server of the carrier opener.
// `shimChain` is [{pid, ppid, image}] from the shim upward (processTreeSnapshot.serverChain);
// `daemonChain` the daemon's own [{pid, image}] for the same shim; `carrierPid` the opener's.
export function carrierAncestry({ shimChain, daemonChain = [], carrierPid, daemonHarnessPid = null, daemonPaired = null }) {
  const nearest = (chain) => chain.slice(1).find((c) => isCodexImage(c.image)) ?? null;
  const mine = nearest(shimChain ?? []);
  const theirs = nearest(daemonChain ?? []);
  return {
    shimPid: shimChain?.[0]?.pid ?? null,
    shimChain: (shimChain ?? []).map((c) => ({ pid: c.pid, ppid: c.ppid ?? null, image: c.image })),
    nearestCodexAncestorPid: mine?.pid ?? null,
    nearestCodexAncestorByDaemonChain: theirs?.pid ?? null,
    carrierServerPid: carrierPid ?? null,
    sameProcess: mine && carrierPid != null ? mine.pid === carrierPid : null,
    daemonHarnessPid,
    daemonPaired,
    daemonHarnessIsCarrier: daemonHarnessPid != null && carrierPid != null ? daemonHarnessPid === carrierPid : null,
  };
}

// --- the Claude session's own report --------------------------------------------------------------------------

// Strings of a Claude Code stream-json run, found in the pane text: every string value of every JSON
// line, plus the raw text (a line that is not JSON, or that the pane cut).
export function claudeStrings(paneText) {
  const out = [];
  const walk = (v) => {
    if (typeof v === 'string') out.push(v);
    else if (Array.isArray(v)) v.forEach(walk);
    else if (v && typeof v === 'object') Object.values(v).forEach(walk);
  };
  for (const o of jsonLines(paneText)) walk(o);
  out.push(String(paneText ?? ''));
  return out;
}

// -> { sessionId, received: [{ sender, messageId, text }], sent: [{ n, messageId }], done, channelTags: [[attr names]] }
export function claudeEvidence(paneText, nonce) {
  const strings = claudeStrings(paneText);
  const n = regexLiteral(nonce);
  const first = (re) => {
    for (const s of strings) {
      const m = re.exec(s);
      if (m) return m;
    }
    return null;
  };
  const all = (re) => {
    const out = [];
    for (const s of strings) for (const m of s.matchAll(re)) out.push(m);
    return out;
  };
  const sid = first(new RegExp(`CLAUDE-SID-${n}: ([0-9a-z]{16,40})`));
  const received = [];
  for (const m of all(new RegExp(`RECEIVED-${n}: ([^\\n\\\\]*)`, 'g'))) {
    const rest = m[1].trim();
    if (!rest || /^followed by/i.test(rest)) continue;
    if (!received.some((r) => r.line === rest)) received.push({ line: rest });
  }
  const sent = [];
  for (const m of all(new RegExp(`SENT-${n}-([123]): ([^\\n\\\\ ]+)`, 'g'))) {
    if (!sent.some((x) => x.n === Number(m[1]))) sent.push({ n: Number(m[1]), messageId: m[2] });
  }
  const tags = [];
  for (const m of all(/<channel\b([^>]*)>/g)) {
    const attrs = [...m[1].matchAll(/([A-Za-z_][\w-]*)=\\?"/g)].map((a) => a[1]);
    if (attrs.length && !tags.some((t) => t.join() === attrs.join())) tags.push(attrs);
  }
  return { sessionId: sid ? sid[1] : null, received, sent, done: !!first(new RegExp(`^\\s*CLAUDE-DONE-${n}\\.?\\s*$`, 'm')), channelTags: tags };
}

// --- the claims ------------------------------------------------------------------------------------------------

export const CLAIM_TEXT = Object.freeze({
  1: 'The Codex refusal, then the pairing, then whoami returning a session id',
  2: 'A Codex send reaching the Claude session',
  3: 'A Claude-to-Codex hand-off arriving in the Codex thread through thread/queue/add, with the provenance header outside the delimited body (C6)',
  4: 'A Codex reply with and without in_reply_to, reported correlated and uncorrelated respectively',
  5: 'Open question A: a thread/resume made mid-turn still receives the in-flight call\'s item/completed',
  6: 'Open question B (risk row 89): the shim\'s nearest codex ancestor is the control-socket server process',
  7: 'How often Codex echoes oac_message_id in in_reply_to (risk row 17)',
});

const shown = (n, evidence) => ({ n, claim: CLAIM_TEXT[n], status: 'shown', evidence });
const notShown = (n, why, evidence = []) => ({ n, claim: CLAIM_TEXT[n], status: 'not-shown', why, evidence });
const open = (n, why, evidence = []) => ({ n, claim: CLAIM_TEXT[n], status: 'answered-negative', why, evidence });

// All seven, from the run's facts. Pure. A claim is `shown` only with the evidence listed; an open question
// is `shown` when answered either way ("answered-negative" is an answer: the thing asked was found not to hold).
export function evaluateClaims({ pairing, codexSend, claude, frames, trials, mid, carrier, nonce }) {
  const out = [];
  // 1
  if (pairing?.refusedThenPaired && pairing.codexSessionId) out.push(shown(1, [`refusal item (status ${pairing.refusal.status}) carried a pairing value; later whoami item (status ${pairing.paired.status}) returned a session_id`]));
  else out.push(notShown(1, !pairing?.refusal ? 'no refused whoami call with a pairing value was seen' : 'no whoami call returned a session id after the refusal', []));
  // 2
  const sendOk = !!codexSend && codexSend.status === 'completed' && /accepted-by-adapter/.test(codexSend.resultText ?? '');
  const recvOk = (claude?.received ?? []).some((r) => new RegExp(`G8LIVE ${regexLiteral(nonce)} FROM-CODEX`).test(r.line));
  if (sendOk && recvOk) out.push(shown(2, ['Codex send result: accepted-by-adapter', 'the Claude session reported the message (RECEIVED line with the test text)', `channel tag attributes seen: ${JSON.stringify(claude.channelTags)}`]));
  else out.push(notShown(2, !sendOk ? 'the Codex send call did not complete with accepted-by-adapter' : 'the Claude session did not report the message', [`send result ok: ${sendOk}`, `claude reported it: ${recvOk}`]));
  // 3
  const arrived = (frames ?? []).filter((f) => testNumberOf(f.text, nonce) !== null);
  if (arrived.length > 0 && arrived.every((f) => f.frame.ok)) out.push(shown(3, [`${arrived.length} hand-off(s) arrived as userMessage items; each: header keys in order before the body marker, body lines all line-quoted, same delimiter on the end line`]));
  else out.push(notShown(3, arrived.length === 0 ? 'no hand-off frame arrived in the thread' : `frame problems: ${arrived.map((f) => f.frame.problems.join('; ') || 'ok').join(' | ')}`, []));
  // 4
  const by = (instr) => (trials ?? []).find((t) => t.instruction === instr && t.replied) ?? null;
  const withId = by('in_reply_to');
  const withoutId = by('to-only');
  const c1 = withId && withId.inReplyTo === 'echoed-the-id' && withId.reportedCorrelation === 'correlated';
  const c2 = withoutId && withoutId.inReplyTo === 'absent' && withoutId.reportedCorrelation === 'uncorrelated';
  if (c1 && c2) out.push(shown(4, ['a reply naming the message was reported correlated', 'a reply without in_reply_to was reported uncorrelated']));
  else out.push(notShown(4, `named-reply ${c1 ? 'ok' : withId ? `got inReplyTo=${withId.inReplyTo}, correlation=${withId.reportedCorrelation}` : 'not made'}; to-only reply ${c2 ? 'ok' : withoutId ? `got inReplyTo=${withoutId.inReplyTo}, correlation=${withoutId.reportedCorrelation}` : 'not made'}`, []));
  // 5
  if (mid?.verdict === 'received') out.push(shown(5, [mid.why, `resume answered ${mid.resumeMsAfterStart} ms after the call started and ${mid.resumeMsBeforeCompleted} ms before it completed`, 'the adapter\'s own mid-turn resume is a second witness when claim 1 holds']));
  else if (mid?.verdict === 'not-received') out.push(open(5, mid.why, [`resume answered ${mid.resumeMsAfterStart} ms after the call started and ${mid.resumeMsBeforeCompleted} ms before it completed`]));
  else out.push(notShown(5, mid?.why ?? 'no midwatch result', []));
  // 6
  if (carrier && carrier.sameProcess !== null && carrier.carrierServerPid !== null) {
    const witness = `daemon: nearest harness ancestor pid ${carrier.daemonHarnessPid}, paired ${carrier.daemonPaired}`;
    if (carrier.sameProcess) out.push(shown(6, [`nearest codex ancestor pid ${carrier.nearestCodexAncestorPid} equals the carrier server pid ${carrier.carrierServerPid}`, witness]));
    else out.push(open(6, `the nearest codex ancestor (pid ${carrier.nearestCodexAncestorPid}) is not the carrier server (pid ${carrier.carrierServerPid})`, [witness]));
  } else out.push(notShown(6, carrier?.carrierServerPid == null ? 'the opener reported no server pid' : 'no codex ancestor was found for the shim', []));
  // 7
  const replied = (trials ?? []).filter((t) => t.replied);
  if (replied.length > 0) out.push(shown(7, [`${replied.length} of ${(trials ?? []).length} hand-off(s) were replied to`, ...replied.map((t) => `test ${t.test} (${t.instruction}): in_reply_to ${t.inReplyTo}, reported ${t.reportedCorrelation}`)]));
  else out.push(notShown(7, 'Codex made no reply to any hand-off', []));
  return out;
}

// --- the scenario ------------------------------------------------------------------------------------------------

export default {
  name: 'g8-codex-pairing',
  description: 'G8 S6 (#69, #433): a real Codex TUI and a real Claude Code (print mode) against the example live daemon: pairing, hand-offs both ways, correlation, the mid-turn resume question and the process ancestry. Not a gate run; not verdict-bearing.',
  harnesses: ['codex', 'claude'],
  defaults: {
    launch: ['codex'], // recorded only: the operator starts Codex; the driver launches nothing
    timeboxMs: 90 * 60 * 1000,
    params: {
      oac: '',
      daemon: '',
      projectDir: '',
      namespace: 'g8live',
      serverName: DEFAULT_SERVER_NAME,
      operatorPrompt: DEFAULT_MARKER,
      explicitEnv: '0',
      daemonWaitMs: '60000',
      attachTimeoutMs: '900000',
      pairTimeoutMs: '900000',
      claudeStartMs: '240000',
      codexSendTimeoutMs: '900000',
      handoffTimeoutMs: '600000',
      replyTimeoutMs: '1200000',
      replyIdleMs: '180000',
      claudeDoneMs: '600000',
      quietMs: '8000',
      wireTimeoutMs: '30000',
      cliTimeoutMs: '60000',
      clientTimeoutMs: '60000',
      listPollMs: '3000',
      claudeLines: '3000',
    },
  },

  async run(ctx) {
    const { herdr, params } = ctx;
    const num = (k) => {
      const v = Number(params[k]);
      if (!(v >= 0)) throw new DriverError(`--param ${k} must be a non-negative number`);
      return v;
    };
    for (const k of ['oac', 'daemon', 'projectDir']) {
      if (!params[k] || !isAbsolute(params[k])) throw new DriverError(`--param ${k}=<absolute path> is required`);
    }
    const ns = params.namespace;
    if (!/^[a-z0-9]{1,24}$/.test(ns)) throw new DriverError('namespace must be 1 to 24 lower-case letters and digits');
    const serverName = params.serverName;
    if (!/^[A-Za-z0-9_-]{1,40}$/.test(serverName)) throw new DriverError('serverName must be letters, digits, - and _');
    const operatorPrompt = params.operatorPrompt || DEFAULT_MARKER;
    const abortSignal = herdr.abortSignal;
    const nonce = randomBytes(4).toString('hex');
    const slash = (p) => p.replace(/\\/g, '/');

    const g = {
      nonVerdictBearing: LIVE_NON_VERDICT,
      params: { ...params, oac: '<oac>', daemon: '<daemon>', projectDir: '<projectDir>' },
      nonce,
      date: new Date().toISOString().slice(0, 10),
      versions: null,
      client: null,
      daemon: { versionBefore: null, versionAfter: null },
      clientRuns: [],
      thread: null,
      watch: null,
      midwatch: null,
      stages: [],
      pairing: null,
      codexSend: null,
      claude: null,
      frames: null,
      trials: null,
      midTurn: null,
      carrier: null,
      processTree: null,
      methodsSeen: null,
      claims: null,
      findings: [],
      captureNames: null,
      stoppedAt: null,
    };
    const stage = (name, extra = {}) => g.stages.push({ at: new Date().toISOString(), name, ...extra });

    let clientDir = null;
    let watchProc = null;
    let midProc = null;
    let ownThreadId = null;
    let daemonPane = null;
    let daemonStopped = false;
    let eventsPath = null;
    let claudeText = '';
    let codexSessionId = null;
    let claudeSessionId = null;
    const secrets = { threadIds: new Set(), sessionIds: new Set(), fingerprints: new Set(), pairing: new Set() };

    const stop = (reason) => {
      herdr.inputHalted ??= reason;
      g.stoppedAt = reason;
      throw new NotRunError(reason);
    };
    const aborted = () => {
      if (abortSignal?.aborted) throw new NotRunError(herdr.abortReason());
    };
    const deadlineFor = (ms) => Date.now() + Math.min(ms, Math.max(0, ctx.remainingMs()));
    const say = (text) => console.error(`\n[g8-codex-pairing] ${text}\n`);

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
    const daemonEvents = () => (eventsPath && existsSync(eventsPath) ? jsonLines(readFileSync(eventsPath, 'utf8')) : []);
    const calls = (mode = 'watch') => oacCalls(entries(), serverName, mode);

    const runClient = async (mode, args) => {
      aborted();
      const linesBefore = entries().at(-1)?.line ?? 0;
      const r = await runBounded(process.execPath, [join(clientDir, 'g8-capture.mjs'), mode, ...args], { deadlineMs: Math.min(num('clientTimeoutMs'), Math.max(1, ctx.remainingMs())), env: process.env, cwd: clientDir, abortSignal });
      aborted();
      g.clientRuns.push({ mode, startedAt: r.startedAt, endedAt: r.endedAt, exitCode: r.exitCode, timedOut: r.timedOut, linesBefore });
      if (r.timedOut) stop(`the capture client's \`${mode}\` run timed out after ${num('clientTimeoutMs')} ms`);
      if (r.spawnError || r.exitCode !== 0) throw new DriverError(`the capture client's \`${mode}\` run failed (${r.spawnError ?? `exit ${r.exitCode}`}); is the Codex app-server daemon running? Nothing was started`);
      return { linesBefore };
    };

    // Poll `predicate` until it returns a truthy value; `onTimeout` decides (default: stop the run, NOT RUN).
    const waitFor = async (label, predicate, timeoutMs, { pollMs = 500, onTimeout = null } = {}) => {
      const deadline = deadlineFor(timeoutMs);
      for (;;) {
        aborted();
        const v = await predicate();
        if (v) return v;
        if (ctx.remainingMs() <= 0 || Date.now() >= deadline) {
          if (onTimeout) return onTimeout();
          stop(`timed out after ${timeoutMs} ms waiting for: ${label}`);
        }
        await sleep(pollMs);
      }
    };

    const killClients = async () => {
      for (const p of [watchProc, midProc]) {
        if (!p) continue;
        killTree(p.child.pid, 'SIGTERM');
        await within(p.exited, 3000);
      }
    };

    try {
      // --- 0. preflight ------------------------------------------------------------------------------
      const cliRaw = ctx.harnessVersion('codex');
      const pinsFile = committedFile(REPO, PINS_PATH);
      const pin = parseCodexVersions(pinsFile.bytes.toString('utf8'));
      const cli = parseCodexCliVersion(cliRaw);
      g.versions = { pinsRow: CODEX_PIN_ROW, pinsMinimum: pin.minimum, pinsLastTested: pin.lastTested, cliOutput: cliRaw, cli, daemon: null, wire: null, verified: false, matchesLastTested: null, warnings: [], claudeOutput: ctx.harnessVersion('claude') };
      const warn = (w) => {
        if (!w) return;
        g.versions.warnings.push(w);
        ctx.finding(w);
      };
      warn(pinsReadWarning(pin, LIVE_GATE));
      if (!cli && /^N\/A/.test(cliRaw ?? 'N/A')) stop(`codex --version could not be run (${cliRaw ?? 'not recorded'}); nothing started`);
      warn(codexVersionWarning({ observed: cli, lastTested: pin.lastTested, minimum: pin.minimum, source: '`codex --version`', gate: LIVE_GATE }));

      clientDir = ctx.dir('g8live-client');
      g.client = stageGateFiles(REPO, LIVE_CLIENT_FILES, clientDir);
      for (const f of g.client) if (!f.match) throw new DriverError(`the staged copy of ${f.path} does not match its source (sha256)`);

      // The shared daemon runs (the operator or the orchestrator started it); the driver never does.
      ctx.noteScratchHolder?.(CODEX_DAEMON_SCRATCH_HOLDER);
      const dv = await codexCli(['app-server', 'daemon', 'version'], 'daemon version');
      const daemonV = parseCodexDaemonVersion(dv.r.stdout);
      g.daemon.versionBefore = { ...dv.rec, parsed: daemonV };
      g.versions.daemon = daemonV;
      if (!daemonV || daemonV.status !== 'running') stop('the shared Codex app-server daemon is not running (`codex app-server daemon version` reports no running status); the driver never starts it: the lead or the orchestrator starts it before the run. Nothing was started');
      for (const k of CODEX_DAEMON_VERSION_FIELDS) warn(codexVersionWarning({ observed: daemonV?.[k] ?? null, lastTested: pin.lastTested, minimum: pin.minimum, source: `\`codex app-server daemon version\` ${k}`, gate: LIVE_GATE }));
      const pre = await runClient('list', []);
      const preConn = facts().connections.find((c) => c.mode === 'list' && c.firstLine > pre.linesBefore);
      g.versions.wire = preConn?.userAgentVersion ?? null;
      warn(codexVersionWarning({ observed: g.versions.wire, lastTested: pin.lastTested, minimum: pin.minimum, source: 'the wire initialize userAgent', gate: LIVE_GATE }));
      const same = !!cli && g.versions.wire === cli && CODEX_DAEMON_VERSION_FIELDS.every((k) => daemonV?.[k] === cli);
      g.versions.verified = same;
      g.versions.matchesLastTested = same ? cli === pin.lastTested : null;
      if (!same) ctx.finding(`the Codex CLI (${cliRaw}), daemon (${JSON.stringify(daemonV)}) and wire (${g.versions.wire ?? 'none'}) do not report one and the same version; the run continues, but its captures stay unverified-*`);
      g.captureNames = liveNames(g.date, same ? cli : null);
      stage('preflight');

      // --- 1. the live daemon, in a pane -----------------------------------------------------------------
      const work = ctx.dir('g8live');
      eventsPath = join(work, 'events.jsonl');
      const wsA = await herdr.workspaceCreate({ cwd: work, label: 'oac-g8live-daemon', env: { OAC_SCOPE: ns } });
      daemonPane = wsA.paneId;
      ctx.record('daemonPane', wsA);
      await herdr.paneRun(daemonPane, ctx.quoteForPane([params.daemon, '--namespace', ns, '--events', eventsPath]));
      try {
        await herdr.paneWaitOutput(daemonPane, { regex: regexLiteral('"event":"ready"'), timeoutMs: num('daemonWaitMs') });
      } catch (err) {
        ctx.capture('pane-daemon-not-ready.txt', await herdr.paneRead(daemonPane, { source: 'recent-unwrapped', lines: 200 }));
        throw err;
      }
      const carrierEv = await waitFor('the daemon reporting the Codex carrier it opened (a carrier_server event)', () => daemonEvents().find((e) => e.event === 'carrier_server') ?? (daemonEvents().some((e) => e.event === 'log' && /no Codex control socket|carrier was not opened/.test(e.line ?? '')) ? 'failed' : null), num('daemonWaitMs'));
      if (carrierEv === 'failed') stop('the live daemon could not open the Codex control socket as a carrier (see its log lines); is the Codex app-server daemon running for this user? Nothing was started');
      stage('daemon-ready', { carrierServerPid: carrierEv.pid ?? null });

      // --- 2. the operator's Codex TUI: the marker ------------------------------------------------------------
      const projectDirs = [...new Set([params.projectDir, existsSync(params.projectDir) ? realpathSync(params.projectDir) : params.projectDir])];
      say(`The live daemon is ready (namespace ${ns}). Start plain \`codex\` in ${basename(params.projectDir)} (see the operator steps), then type exactly this and press Enter:\n  ${operatorPrompt}`);
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
      secrets.threadIds.add(threadId);
      g.thread = { id: '<THREAD_ID>' };
      stage('thread-identified');

      // --- 3. subscribe (watch): before any OAC tool call ------------------------------------------------------
      const watchFrom = entries().at(-1)?.line ?? 0;
      watchProc = spawnLongRunning(process.execPath, [join(clientDir, 'g8-capture.mjs'), 'watch', threadId], { env: process.env, cwd: clientDir });
      abortSignal?.addEventListener('abort', () => killTree(watchProc?.child.pid, 'SIGTERM'), { once: true });
      g.watch = { startedAt: new Date().toISOString(), linesBefore: watchFrom, resumeLine: null, resumeStatus: null };
      const exited = (p) => (p && (p.child.exitCode !== null || p.child.signalCode !== null) ? `exited (${p.child.exitCode ?? p.child.signalCode})` : null);
      const resume = await waitFor('the watch connection\'s thread/resume result', () => facts().resumes.find((x) => x.mode === 'watch' && x.line > watchFrom && x.threadId === threadId) ?? (exited(watchProc) ? 'exited' : null), num('wireTimeoutMs'));
      if (resume === 'exited') throw new DriverError(`the capture client's watch process ${exited(watchProc)} before thread/resume answered`);
      if (resume.error) throw new DriverError(`thread/resume answered with error ${JSON.stringify(resume.error)}`);
      g.watch.resumeLine = resume.line;
      g.watch.resumeStatus = resume.status;
      stage('watching');

      // --- 4. the pairing (claim 1), the mid-turn resume (claim 5), the process tree (claim 6) ----------------------
      say(`Subscribed to the TUI thread. In the Codex TUI, type this and press Enter, then follow the operator steps for each dialog (copy its text first, then Allow this once):\n  ${pairingPrompt(serverName)}`);
      let treeTaken = false;
      const takeTree = () => {
        if (treeTaken) return;
        const acc = daemonEvents().find((e) => e.event === 'accepted' && e.label === 'codex' && e.peer?.pid);
        if (!acc) return;
        treeTaken = true;
        const snap = processTreeSnapshot(processTable(), acc.peer.pid);
        const carrier = daemonEvents().find((e) => e.event === 'carrier_server');
        g.processTree = { shimPid: acc.peer.pid, snapshot: snap, note: 'pid, parent pid and image name only' };
        g.carrier = carrierAncestry({
          shimChain: snap.available ? snap.serverChain : [],
          daemonChain: sanitizeDaemonEvents([acc])[0].chain,
          carrierPid: carrier?.pid ?? null,
          daemonHarnessPid: acc.harness?.pid ?? null,
          daemonPaired: acc.paired ?? null,
        });
        g.carrier.codexProcesses = snap.available ? snap.codexProcesses : null;
        g.carrier.carrierServerIsListedCodex = snap.available && carrier?.pid != null ? snap.codexProcesses.some((c) => c.pid === carrier.pid) : null;
        g.carrier.carrierServerPairingKey = carrier?.pairing_key ?? null;
      };
      let midStartedFor = null;
      const startMidwatch = (call) => {
        if (midProc || midStartedFor) return;
        midStartedFor = call.itemId;
        g.midwatch = { startedAt: new Date().toISOString(), linesBefore: entries().at(-1)?.line ?? 0, afterItem: '<ITEM_ID>', callStartedLine: call.startedLine, callStartedT: call.startedT };
        midProc = spawnLongRunning(process.execPath, [join(clientDir, 'g8-capture.mjs'), 'midwatch', threadId], { env: process.env, cwd: clientDir });
        abortSignal?.addEventListener('abort', () => killTree(midProc?.child.pid, 'SIGTERM'), { once: true });
      };
      const pairedSeq = await waitFor('the refused whoami, then a whoami that returns a session id', () => {
        takeTree();
        const cs = calls();
        const firstOac = cs.find((c) => c.startedLine !== null);
        if (firstOac) startMidwatch(firstOac);
        const seq = pairingSequence(cs);
        return seq.refusedThenPaired ? seq : exited(watchProc) ? 'exited' : null;
      }, num('pairTimeoutMs'), { pollMs: 300 });
      if (pairedSeq === 'exited') throw new DriverError(`the capture client's watch process ${exited(watchProc)} while waiting for the pairing`);
      g.pairing = pairedSeq;
      codexSessionId = pairedSeq.codexSessionId;
      secrets.sessionIds.add(codexSessionId);
      if (pairedSeq.deviceFingerprint) secrets.fingerprints.add(pairedSeq.deviceFingerprint);
      g.pairing = { ...pairedSeq, codexSessionId: '<CODEX_SESSION_ID>', deviceFingerprint: pairedSeq.deviceFingerprint ? '<DEVICE_FINGERPRINT>' : null };
      takeTree();
      stage('paired');

      // --- 5. Claude Code, print mode (the G9 leg A recipe) -----------------------------------------------------
      const mcpPath = join(work, 'mcp.json');
      const settingsPath = join(work, 'settings.json');
      const server = { command: params.oac, args: ['mcp-shim', '--harness', 'claude'] };
      if (params.explicitEnv === '1') server.env = { OAC_SCOPE: ns };
      writeFileSync(mcpPath, JSON.stringify({ mcpServers: { oac: server } }));
      writeFileSync(settingsPath, JSON.stringify({ hooks: { SessionStart: [{ hooks: [{ type: 'command', command: `"${slash(params.oac)}" hook` }] }] } }));
      const wsB = await herdr.workspaceCreate({ cwd: ctx.dir('claude-cwd'), label: 'oac-g8live-claude', env: { OAC_SCOPE: ns } });
      ctx.record('claudePane', wsB);
      await ctx.probeEnv(wsB.paneId, { timeoutMs: num('daemonWaitMs') });
      const doneMark = `G8LIVE-SHELL-DONE-${nonce}`;
      const claude = ctx.quoteForPane([
        'claude',
        '-p',
        claudePrompt({ nonce, codexSessionId }),
        '--output-format',
        'stream-json',
        '--verbose',
        '--dangerously-load-development-channels',
        'server:oac',
        '--allowedTools',
        'mcp__oac__whoami,mcp__oac__send,mcp__oac__list_sessions,Bash(sleep:*)',
        '--mcp-config',
        mcpPath,
        '--strict-mcp-config',
        '--settings',
        settingsPath,
        '--max-turns',
        '90',
      ]);
      await herdr.paneRun(wsB.paneId, `${claude}; echo ${doneMark}`);
      stage('claude-started');
      const readClaude = async () => {
        claudeText = await herdr.paneRead(wsB.paneId, { source: 'recent-unwrapped', lines: num('claudeLines') });
        return claudeEvidence(claudeText, nonce);
      };
      const shellDone = () => new RegExp(`^\\s*${regexLiteral(doneMark)}\\s*$`, 'm').test(claudeText);
      const ev0 = await waitFor('the Claude session printing its own session id (CLAUDE-SID line)', async () => {
        const ev = await readClaude();
        if (ev.sessionId) return ev;
        return shellDone() ? 'ended' : null;
      }, num('claudeStartMs'), { pollMs: 2000 });
      if (ev0 === 'ended') {
        ctx.finding('the Claude Code print-mode process ended before it printed a session id (does `--dangerously-load-development-channels` work with `-p`? is the oac shim reachable?); see the Claude capture');
        stop('the Claude session ended without printing a session id');
      }
      claudeSessionId = ev0.sessionId;
      secrets.sessionIds.add(claudeSessionId);
      stage('claude-bound');

      // --- 6. Codex send -> Claude (claim 2) ------------------------------------------------------------------
      say(`Claude is bound. In the Codex TUI, type this and press Enter (one send; copy any dialog's text first, then Allow this once):\n  ${sendPrompt(serverName, claudeSessionId, nonce)}`);
      let claudeEv = ev0;
      const codexSendCall = () => calls().find((c) => c.tool === 'send' && c.completedLine !== null) ?? null;
      await waitFor('the Codex send call completing and the Claude session reporting the message', async () => {
        claudeEv = await readClaude();
        const s = codexSendCall();
        const got = claudeEv.received.some((r) => r.line.includes(`G8LIVE ${nonce} FROM-CODEX`));
        return s && got ? true : shellDone() && s ? true : null;
      }, num('codexSendTimeoutMs'), { pollMs: 2000, onTimeout: () => { ctx.finding('the Codex send and the Claude report of it were not both seen in time; later steps depend on Claude having received it before it sends the hand-offs'); return false; } });
      const sc = codexSendCall();
      g.codexSend = sc && { status: sc.status, completedLine: sc.completedLine, resultState: /accepted-by-adapter/.test(sc.resultText ?? '') ? 'accepted-by-adapter' : 'other' };
      stage('codex-send-done');

      // --- 7. Claude -> Codex hand-offs (claim 3), Codex replies (claims 4 and 7) --------------------------------------
      say('Claude now sends three hand-offs. They arrive in the Codex thread by themselves; do not type anything. Each Codex reply raises a dialog: copy its full text and options here first, THEN choose Allow (this once; never Always).');
      const frames = () => handoffFrames(entries()).filter((f) => testNumberOf(f.text, nonce) !== null);
      await waitFor('the three hand-offs arriving in the Codex thread', async () => {
        if (frames().length >= 3) return true;
        await readClaude();
        return shellDone() ? true : null;
      }, num('handoffTimeoutMs'), { pollMs: 1500, onTimeout: () => { ctx.finding(`only ${frames().length} of 3 hand-offs arrived in the Codex thread`); return false; } });
      let lastCount = -1;
      let lastChange = Date.now();
      await waitFor('the Codex replies', async () => {
        await readClaude();
        if (shellDone() && frames().length < 3) return true;
        const done = calls().filter((c) => c.tool === 'reply' && c.completedLine !== null).length;
        const n = `${done}/${frames().length}/${entries().length}`;
        if (n !== String(lastCount)) {
          lastCount = n;
          lastChange = Date.now();
        }
        if (frames().length >= 3 && done >= 3 && Date.now() - lastChange >= num('quietMs')) return true;
        if (frames().length > 0 && Date.now() - lastChange >= num('replyIdleMs')) return true;
        return null;
      }, num('replyTimeoutMs'), { pollMs: 1500, onTimeout: () => { ctx.finding('the wait for the Codex replies ran out; what arrived is kept'); return false; } });
      stage('handoffs-done');

      // --- 8. Claude finishes, the daemon stops ------------------------------------------------------------------------
      await waitFor('the Claude print-mode process ending', async () => {
        claudeEv = await readClaude();
        return shellDone() ? true : null;
      }, num('claudeDoneMs'), { pollMs: 3000, onTimeout: () => { ctx.finding('the Claude print-mode process had not ended when the wait ran out; its pane report so far is kept'); return false; } });
      claudeEv = await readClaude();
      await herdr.paneProcessInfo(wsB.paneId);
      await herdr.paneRun(daemonPane, 'stop');
      try {
        await herdr.paneWaitOutput(daemonPane, { regex: regexLiteral('"event":"stopped"'), timeoutMs: num('daemonWaitMs') });
        daemonStopped = true;
      } catch {
        ctx.finding('the live daemon did not report `stopped` within the wait after a `stop` line');
      }
      stage('daemon-stopped');

      // --- 9. analysis ----------------------------------------------------------------------------------------------------
      const all = entries();
      const fr = handoffFrames(all).filter((f) => testNumberOf(f.text, nonce) !== null);
      const allCalls = calls();
      const trials = echoTrials(fr, allCalls, nonce);
      for (const f of fr) {
        if (f.frame.header.oac_sender) secrets.sessionIds.add(f.frame.header.oac_sender);
        if (f.frame.header.oac_session) secrets.sessionIds.add(f.frame.header.oac_session);
        if (f.frame.header.oac_device) secrets.fingerprints.add(f.frame.header.oac_device);
      }
      g.frames = fr.map((f) => ({ line: f.line, t: f.t, test: testNumberOf(f.text, nonce), messageId: f.messageId, frameOk: f.frame.ok, problems: f.frame.problems, headerKeysInOrder: f.frame.headerKeysInOrder, bodyLineCount: f.frame.bodyLineCount, bodyAllQuoted: f.frame.bodyAllQuoted, endMarker: f.frame.endMarker, provenanceOutsideBody: f.frame.provenanceOutsideBody }));
      g.trials = trials;
      const midItem = allCalls.find((c) => c.itemId === midStartedFor) ?? null;
      g.midTurn = midTurnVerdict({ item: midItem, bEntries: all.filter((e) => e.mode === 'midwatch') });
      g.claude = { sessionId: '<CLAUDE_SESSION_ID>', received: claudeEv.received.length, sentIds: claudeEv.sent.length, done: claudeEv.done, channelTags: claudeEv.channelTags, shellEnded: shellDone() };
      const methods = {};
      for (const e of all) if (e.mode === 'watch' && e.direction === 'daemon->client' && e.payload?.method) methods[e.payload.method] = (methods[e.payload.method] ?? 0) + 1;
      g.methodsSeen = methods;
      const devEvents = daemonEvents();
      g.daemonSummary = {
        accepted: devEvents.filter((e) => e.event === 'accepted').map((e) => ({ label: e.label, paired: e.paired, peer: e.peer?.pid ?? null, harness: e.harness?.pid ?? null })),
        stopped: devEvents.some((e) => e.event === 'stopped'),
        bindingLines: devEvents.filter((e) => e.event === 'log' && /binding/.test(e.line ?? '')).length,
        deliverDecisions: devEvents.filter((e) => e.event === 'log' && /decision/.test(e.line ?? '') && /deliver/i.test(e.line ?? '')).length,
      };
      const claims = evaluateClaims({ pairing: pairedSeq, codexSend: sc, claude: claudeEv, frames: fr, trials, mid: g.midTurn, carrier: g.carrier, nonce });
      g.claims = claims;
      for (const c of claims) if (c.status === 'not-shown') ctx.finding(`claim ${c.n} not shown: ${c.why}`);
      if (claims.some((c) => c.status !== 'shown')) g.someClaimsNotShown = true;

      // --- 10. post-run versions -----------------------------------------------------------------------------------------
      const dv2 = await codexCli(['app-server', 'daemon', 'version'], 'daemon version (post-run)');
      g.daemon.versionAfter = { ...dv2.rec, parsed: parseCodexDaemonVersion(dv2.r.stdout) };
      const postOk = CODEX_DAEMON_VERSION_FIELDS.every((k) => g.daemon.versionAfter.parsed?.[k] === cli);
      if (!postOk) {
        ctx.finding('the Codex daemon version changed during the run; the captures lose their fixture names');
        g.versions.verified = false;
        g.captureNames = liveNames(g.date, null);
      }
    } finally {
      await killClients();
      if (daemonPane && !daemonStopped) {
        try {
          await herdr.paneRun(daemonPane, 'stop');
        } catch {
          /* the driver's teardown ends the pane */
        }
      }
      const rawTranscript = clientDir && existsSync(transcriptPath()) ? readFileSync(transcriptPath(), 'utf8') : '';
      const rawEvents = eventsPath && existsSync(eventsPath) ? readFileSync(eventsPath, 'utf8') : '';
      // Pairing values wherever they appear; session ids named in the daemon's binding lines.
      for (const text of [rawTranscript, rawEvents, claudeText]) for (const m of String(text).matchAll(PAIRING_VALUE)) secrets.pairing.add(m[0]);
      for (const e of jsonLines(rawEvents)) if (e.event === 'log' && /session=/.test(e.line ?? '')) for (const m of String(e.line).matchAll(/\b[0-9a-hjkmnp-tv-z]{26}\b/g)) secrets.sessionIds.add(m[0]);
      if (codexSessionId) secrets.sessionIds.add(codexSessionId);
      if (claudeSessionId) secrets.sessionIds.add(claudeSessionId);
      secrets.threadIds.forEach((id) => ctx.redactLiteral(id, '<THREAD_ID>'));
      secrets.sessionIds.forEach((id) => ctx.redactLiteral(id, '<SESSION_ID>'));
      secrets.fingerprints.forEach((id) => ctx.redactLiteral(id, '<DEVICE_FINGERPRINT>'));
      secrets.pairing.forEach((v) => ctx.redactLiteral(v, '<PAIRING_VALUE>'));
      ctx.redactLiteral(params.projectDir, '<PROJECT_DIR>');
      let transcriptText = null;
      if (rawTranscript) {
        const san = sanitizeTranscript(rawTranscript, { ownThreadId });
        for (const id of san.installationIds) ctx.redactLiteral(id, '<INSTALLATION_ID>');
        g.sanitizer = san.report;
        transcriptText = san.text;
        // Other loaded threads' ids appear in thread/loaded/list: not ours to publish.
        otherThreadIds(transcriptText, [...secrets.threadIds]).forEach((id, i) => ctx.redactLiteral(id, `<OTHER_THREAD_ID_${i + 1}>`));
      }
      ctx.record('g8live', g);
      if (g.captureNames) {
        if (transcriptText !== null) ctx.capture(g.captureNames.transcript, transcriptText, { format: 'jsonl' });
        if (rawEvents) ctx.capture(g.captureNames.events, sanitizeDaemonEvents(jsonLines(rawEvents)).map((e) => JSON.stringify(e)).join('\n'), { format: 'jsonl' });
        if (claudeText) ctx.capture(g.captureNames.claude, claudeEvidenceText(claudeText, nonce));
        if (g.processTree) ctx.capture(g.captureNames.proc, JSON.stringify({ t: new Date().toISOString(), mode: 'proc', direction: 'event', payload: g.processTree }), { format: 'jsonl' });
      }
    }
  },
};

// The Claude pane as captured: only the report lines the session was asked to print and the channel tags it
// saw, not the whole stream (which holds the session's init data and tool outputs).
export function claudeEvidenceText(paneText, nonce) {
  const ev = claudeEvidence(paneText, nonce);
  const lines = [];
  if (ev.sessionId) lines.push(`CLAUDE-SID-${nonce}: ${ev.sessionId}`);
  for (const s of ev.sent) lines.push(`SENT-${nonce}-${s.n}: ${s.messageId}`);
  for (const r of ev.received) lines.push(`RECEIVED-${nonce}: ${r.line}`);
  for (const t of ev.channelTags) lines.push(`channel tag attributes: ${t.join(' ')}`);
  if (ev.done) lines.push(`CLAUDE-DONE-${nonce}`);
  return lines.join('\n');
}
