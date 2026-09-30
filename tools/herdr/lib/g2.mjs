// Shared pieces of the scripted G2 re-run (Epic K, K7 #130): the plain Codex launch, the
// committed quarantined G2 client and how it is staged, the human-run 0.157.1 baseline, the
// facts read from a G2 client transcript, the fixture sanitizer, the pane-process argv
// proof, and pane-text classification used for SCHEDULING and SAFETY only.
//
// Nothing here scores a criterion on herdr agent state. Turn completion and delivery come
// from the app-server event stream the quarantined client itself records; pane text is
// read verbatim and kept; the classification below only decides whether the driver may
// continue (a dialog is up) and never stands in for evidence.
//
// Every Codex pane-text pattern below is UNVERIFIED against a live Codex TUI: no Codex
// dialog or in-progress text is on record in this repository. They are best guesses to be
// confirmed or corrected by the first operator run; an unrecognized dialog is never
// accepted by the driver.
//
// Credential hygiene: nothing here opens anything under the Codex home directory. The only
// process data read is a pid's argv (/proc/<pid>/cmdline, `ps -o command=`, or the Win32
// process CommandLine), never a process environment.

import { readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { basename, join } from 'node:path';

import { committedFile, sha256, selectedOption, normalizeDialogText, planDriverAccept } from './g1.mjs';
import { diffSequences } from './compare-transcripts.mjs';

// G2's launch: plain `codex`, no arguments and no config overrides (G2 criterion 1;
// G2-result.md: "the human operator ran plain `codex` (no `-c`, no `--remote`, no flags)").
// launch[0] is the herdr agent kind; herdr runs `codex` and passes nothing after it.
export const G2_LAUNCH = Object.freeze(['codex']);

export const FIXTURE_DIR = 'docs/planning/gates/fixtures/g2-codex-inject';
export const COMMITTED_CLIENT = `${FIXTURE_DIR}/client.mjs.throwaway-quarantined`;
export const BASELINE_TRANSCRIPT = `${FIXTURE_DIR}/transcript-2026-09-26-0.157.1.jsonl`;
export const HERDR_RUNS_DIR = 'docs/planning/gates/herdr-runs';
export const PINS_PATH = 'docs/planning/PINS.md';
export const MANIFEST_PATH = 'docs/planning/gates/fixtures/MANIFEST.json';

// sha256 of COMMITTED_CLIENT as committed (the G2 runs ran this file); a guard that the
// committed blob itself has not been swapped since K7 was written.
export const COMMITTED_CLIENT_SHA256 = '134d41a57e861a92f5381dce045e990cb77b7171aed8d5d81901ea6e6c4347d9';

// The four G2 pass criteria are read verbatim, at run time, from the gate's reference file
// as committed at HEAD, rather than restated here: criterion 4's wording names the provider
// and its credential store, which the containment lint (oac-boundaries check 10) keeps out
// of tools/herdr/. G2_CRITERIA_SHA256 below binds the text read to the text scored.
export const G2_REFERENCE = '.claude/skills/oac-gates/references/G2-codex-inject.md';
export function parseG2Criteria(referenceText) {
  const lines = String(referenceText).split(/\r?\n/);
  const start = lines.findIndex((l) => /^## Pass criteria/.test(l));
  if (start === -1) throw new Error(`${G2_REFERENCE} has no "## Pass criteria" section`);
  const items = [];
  for (const l of lines.slice(start + 1)) {
    if (/^## /.test(l)) break;
    const m = /^- \[[ x]\] (.*)$/.exec(l);
    if (m) items.push(m[1].trim());
    else if (items.length && /^\s+\S/.test(l)) items[items.length - 1] += ` ${l.trim()}`;
  }
  if (items.length !== 4) throw new Error(`${G2_REFERENCE} lists ${items.length} pass criteria; G2 has four`);
  return items;
}
// The scoring in lib/g2-report.mjs is written against exactly these four criteria, in this
// order. Their text is bound by this sha256 of JSON.stringify(<the four parsed criteria>),
// taken from the reference as committed when K7 was written (the hash carries none of the
// criterion text). A reworded, reordered, added or removed criterion changes it, and the
// report refuses to score until K7's scoring has been re-reviewed against the new text and
// this pin moved in the same change.
export const G2_CRITERIA_SHA256 = '012480fff690f067f608c16183949bf0eb9072e4d356f3024732418af105cd67';

export class CriteriaDriftError extends Error {}

// The criteria as COMMITTED at HEAD (never the working tree; K4's pattern), verified
// against G2_CRITERIA_SHA256. -> { criteria, reference: { path, headCommit, fileSha256,
// criteriaSha256, workingTreeMatchesHead } }. Throws CriteriaDriftError on a mismatch.
// `pin` exists only so the self-test can plant a drift in a throwaway repository.
export function readG2Criteria(repoRoot, { pin = G2_CRITERIA_SHA256 } = {}) {
  const c = committedFile(repoRoot, G2_REFERENCE);
  const criteria = parseG2Criteria(c.bytes.toString('utf8'));
  const criteriaSha256 = sha256(JSON.stringify(criteria));
  const reference = { path: G2_REFERENCE, headCommit: c.headCommit, fileSha256: c.committedSha256, criteriaSha256, workingTreeMatchesHead: c.workingTreeMatchesHead };
  if (criteriaSha256 !== pin) {
    throw new CriteriaDriftError(
      `the G2 pass criteria in ${G2_REFERENCE} at HEAD ${c.headCommit} hash to ${criteriaSha256}, not the ${pin} lib/g2-report.mjs scores against: ` +
        "the reference changed; re-review K7's scoring against the new criteria and move G2_CRITERIA_SHA256 (tools/herdr/lib/g2.mjs) in the same change. Nothing was scored or written",
    );
  }
  return { criteria, reference };
}

// Operator's own TUI message in the human 0.157.1 re-run (G2-result.md, thread/list preview
// "Greet me in a single word."); the default operator prompt here, typed as the operator
// typed it. It is the operator's input to their own TUI, never a delivered message.
export const DEFAULT_OPERATOR_PROMPT = 'Greet me in a single word.';

// Injection 1 of the human 0.157.1 re-run, with "through herdr" added; the expected reply
// is the human run's. The busy turn and the queued message are the committed client's own
// `busyqueue` texts, unchanged.
export const defaultInjectText = (version) =>
  `G2 re-run on ${version} through herdr (from a second daemon client, not typed in this TUI): reply with exactly the words OAC G2 RERUN RECEIVED.`;
export const EXPECTED_REPLIES = Object.freeze({ inject: 'OAC G2 RERUN RECEIVED', queued: 'OAC G2 QUEUED' });
export const BUSY_TEXT_MARK = 'G2 spike busy-turn test';
export const QUEUED_TEXT_MARK = 'G2 spike queued message';

// Operator prompts and parameters may never carry a delivered message: delivery comes only
// from the second app-server client.
export function assertNotInjected(label, text) {
  const s = String(text);
  if (/OAC G2|second daemon client|thread\/queue\/add|turn\/start|G2 spike (?:busy-turn|queued)/i.test(s)) {
    throw new Error(`${label} looks like a delivered message; delivery into the thread comes only from the second app-server client, never typed by herdr`);
  }
}

// --- staging the committed client ---------------------------------------------------------

// Stage the quarantined client into destDir as `client.mjs` (Node needs the .mjs name), from
// the blob COMMITTED at HEAD, and re-check the copy's sha256. The working-tree file's own
// state is reported alongside (the scenario refuses to run when it differs). The file is
// never edited and never imported; it runs as its own process. It appends its wire
// transcript to `transcript.jsonl` beside itself, i.e. in destDir.
export function stageClientCopy(repoRoot, relPath, destDir) {
  const c = committedFile(repoRoot, relPath);
  const copyPath = join(destDir, 'client.mjs');
  writeFileSync(copyPath, c.bytes);
  const { bytes, ...rest } = c;
  const copySha256 = sha256(readFileSync(copyPath));
  return { ...rest, copyPath, copySha256, match: copySha256 === c.committedSha256 };
}

// --- fixture names ----------------------------------------------------------------------

export function fixtureNames(date, version) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) throw new Error(`bad fixture date ${JSON.stringify(date)}`);
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error(`bad Codex version ${JSON.stringify(version)}`);
  return { transcript: `transcript-${date}-${version}-herdr.jsonl`, pane: `pane-${date}-${version}-herdr.txt` };
}

// Not fixture-shaped, and never publishable: a run whose Codex version is not (yet) verified
// on the CLI, the daemon and the wire.
export function unverifiedNames(date) {
  return { transcript: `unverified-transcript-${date}-herdr.jsonl`, pane: `unverified-pane-${date}-herdr.txt` };
}

// --- pane text: scheduling and safety only -----------------------------------------------

export const CODEX_DIALOG_KINDS = Object.freeze({
  'workspace-trust': {
    detect: /trust (?:the (?:files|contents) (?:in|of) )?this (?:folder|directory|project)|allow Codex to work in this folder/i,
    acceptOption: /^Yes\b/i,
    verified: null, // UNVERIFIED: no captured Codex dialog text on record
  },
});
const GENERIC_DIALOG = /Press enter to (?:confirm|continue)|Enter to confirm|Esc to cancel|\(y\/n\)|Allow command\?|Approve\b.*\?/i;

// -> { dialog: kind | 'unknown' | null, selected, busy }
export function classifyCodexScreen(text, { busyIndicator = 'esc to interrupt' } = {}) {
  const s = String(text ?? '');
  let dialog = null;
  for (const [kind, def] of Object.entries(CODEX_DIALOG_KINDS)) {
    if (def.detect.test(s)) {
      dialog = kind;
      break;
    }
  }
  if (!dialog && GENERIC_DIALOG.test(s)) dialog = 'unknown';
  const busy = busyIndicator ? s.toLowerCase().includes(busyIndicator.toLowerCase()) : false;
  return { dialog, selected: dialog ? selectedOption(s) : null, busy };
}

// No Codex kind lists its options (no Codex dialog text is on record), so the driver accepts
// a Codex dialog only when its accepting option is already preselected (planDriverAccept).
export function driverMayAcceptCodex(classification) {
  return planDriverAccept(classification, CODEX_DIALOG_KINDS);
}
export { normalizeDialogText };

// --- the pane's process argv (the "plain codex" proof) ------------------------------------

// One pid's argv, read from the OS: Linux /proc/<pid>/cmdline, macOS `ps -o command=`,
// Windows the Win32_Process CommandLine. Never the process environment. `argv` is an array
// where the OS gives one (Linux), else null with `commandLine` as the OS prints it.
export function processArgv(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return { pid, argv: null, commandLine: null, source: 'no pid' };
  if (process.platform === 'linux') {
    try {
      const argv = readFileSync(`/proc/${pid}/cmdline`, 'utf8').split('\0').filter((a, i, all) => a !== '' || i < all.length - 1);
      return { pid, argv, commandLine: null, source: `/proc/${pid}/cmdline` };
    } catch {
      return { pid, argv: null, commandLine: null, source: 'not readable (process gone?)' };
    }
  }
  if (process.platform === 'darwin') {
    const ps = spawnSync('ps', ['-o', 'command=', '-p', String(pid)], { encoding: 'utf8', timeout: 10000 });
    return { pid, argv: null, commandLine: ps.status === 0 ? ps.stdout.trim() : null, source: 'ps -o command=' };
  }
  if (process.platform === 'win32') {
    const ps = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', `(Get-CimInstance Win32_Process -Filter 'ProcessId=${pid}').CommandLine`], { encoding: 'utf8', timeout: 20000, windowsHide: true });
    return { pid, argv: null, commandLine: ps.status === 0 ? ps.stdout.trim() || null : null, source: 'Win32_Process.CommandLine' };
  }
  return { pid, argv: null, commandLine: null, source: `not available on ${process.platform}` };
}

// Split a command line the way the OS printed it (quotes stripped); good enough to find the
// `codex` token and what follows it. The verbatim string is kept beside it.
export function splitCommandLine(s) {
  const out = [];
  const re = /"([^"]*)"|'([^']*)'|(\S+)/g;
  for (const m of String(s ?? '').matchAll(re)) out.push(m[1] ?? m[2] ?? m[3]);
  return out;
}

const CODEX_TOKEN = /^codex(?:\.js|\.exe|\.cmd|\.ps1)?$/i;

// Given argv records (herdr's foreground processes first, then their descendants), find the
// first process running `codex` and what came after the `codex` token. `plain` is true only
// when that process has no argument after it.
export function codexLaunchProof(records) {
  for (const r of records) {
    const tokens = r.argv ?? (r.commandLine ? splitCommandLine(r.commandLine) : null);
    if (!tokens) continue;
    const i = tokens.findIndex((t) => CODEX_TOKEN.test(basename(String(t).replace(/\\/g, '/'))));
    if (i === -1) continue;
    const after = tokens.slice(i + 1);
    return { found: true, pid: r.pid, codexToken: basename(String(tokens[i]).replace(/\\/g, '/')), argsAfterCodex: after, plain: after.length === 0 };
  }
  return { found: false, pid: null, codexToken: null, argsAfterCodex: null, plain: false };
}

// --- the client's wire transcript -----------------------------------------------------------
//
// Lines are the committed client's own records: { t, mode, direction, payload }, direction
// one of handshake, client->daemon, daemon->client, proxy-stderr, ws-close, unparsed. Several
// client processes append to one file; each process run is one connection, begun by its
// `handshake` line. Two processes of the same mode never overlap in this scenario, so a
// connection is (mode, the handshake that opened it).

export function parseG2Transcript(text, { completeLinesOnly = false } = {}) {
  let s = String(text ?? '');
  if (completeLinesOnly) s = s.slice(0, s.lastIndexOf('\n') + 1);
  const out = [];
  s.split('\n').forEach((raw, i) => {
    if (!raw.trim()) return;
    let e;
    try {
      e = JSON.parse(raw);
    } catch {
      throw new Error(`G2 transcript line ${i + 1} is not JSON`);
    }
    out.push({ line: i + 1, t: e.t, mode: e.mode, direction: e.direction, payload: e.payload });
  });
  return out;
}

const textOf = (input) => (Array.isArray(input) ? input.map((x) => (typeof x?.text === 'string' ? x.text : '')).join('') : '');
const statusType = (s) => (typeof s === 'string' ? s : s?.type ?? null);
const cwdsOf = (th) => [...new Set([th?.cwd, ...(Array.isArray(th?.environments) ? th.environments.map((e) => e?.cwd) : [])].filter((x) => typeof x === 'string'))];

// Tag every entry with its connection and, for daemon responses, the method answered.
export function annotateConnections(entries) {
  const conns = [];
  const current = new Map(); // mode -> connection
  for (const e of entries) {
    let c = current.get(e.mode);
    if (e.direction === 'handshake' || !c) {
      c = { index: conns.length, mode: e.mode, firstLine: e.line, lastLine: e.line, upgraded: null, handshake: null, requests: new Map(), userAgent: null, userAgentVersion: null, errors: [], unparsed: 0 };
      conns.push(c);
      current.set(e.mode, c);
    }
    c.lastLine = e.line;
    e.conn = c.index;
    if (e.direction === 'handshake') {
      c.handshake = String(e.payload ?? '').split('\r\n')[0];
      c.upgraded = /^HTTP\/1\.1 101/.test(c.handshake);
    } else if (e.direction === 'unparsed') c.unparsed += 1;
    const p = e.payload;
    if (e.direction === 'client->daemon' && p && p.id !== undefined && p.method) c.requests.set(p.id, { method: p.method, params: p.params, line: e.line, t: e.t });
    if (e.direction === 'daemon->client' && p && p.id !== undefined && !p.method && c.requests.has(p.id)) {
      const req = c.requests.get(p.id);
      e.answers = req.method;
      e.request = req;
      if (p.error) c.errors.push({ line: e.line, method: req.method, error: p.error });
      if (req.method === 'initialize' && p.result) {
        c.userAgent = p.result.userAgent ?? null;
        const m = /^[^\s/]+\/v?(\d+\.\d+\.\d+)(?=$|[\s(])/.exec(String(c.userAgent ?? ''));
        c.userAgentVersion = m ? m[1] : null;
      }
    }
  }
  return conns.map(({ requests, ...rest }) => ({ ...rest, requestMethods: [...requests.values()].map((r) => r.method) }));
}

// Everything the scenario and the report read from a G2 transcript.
export function g2Facts(entries) {
  const es = entries.map((e) => ({ ...e }));
  const connections = annotateConnections(es);
  const f = {
    connections,
    loadedLists: [],
    threadLists: [],
    resumes: [],
    turnStarts: [],
    queueAdds: [],
    turnsLists: [],
    events: { turnStarted: [], turnCompleted: [], userItems: [], agentMessages: [], statusChanged: [] },
    firstT: es[0]?.t ?? null,
    lastT: es.at(-1)?.t ?? null,
  };
  for (const e of es) {
    const p = e.payload;
    if (e.direction !== 'daemon->client' || !p || typeof p !== 'object') continue;
    const base = { line: e.line, t: e.t, mode: e.mode, conn: e.conn };
    if (e.answers) {
      const req = e.request;
      const err = p.error ?? null;
      if (e.answers === 'thread/loaded/list') f.loadedLists.push({ ...base, data: Array.isArray(p.result?.data) ? p.result.data : [], error: err });
      else if (e.answers === 'thread/list') f.threadLists.push({ ...base, entries: (p.result?.data ?? []).map((th) => ({ id: th?.id, preview: th?.preview ?? null, cwds: cwdsOf(th) })), fixtureNote: p.result?._fixtureNote ?? null, error: err });
      else if (e.answers === 'thread/resume') f.resumes.push({ ...base, reqLine: req.line, threadId: req.params?.threadId, status: statusType(p.result?.thread?.status), error: err });
      else if (e.answers === 'turn/start') f.turnStarts.push({ ...base, reqLine: req.line, reqT: req.t, threadId: req.params?.threadId, text: textOf(req.params?.input), turnId: p.result?.turn?.id ?? null, status: p.result?.turn?.status ?? null, error: err });
      else if (e.answers === 'thread/queue/add') f.queueAdds.push({ ...base, reqLine: req.line, reqT: req.t, threadId: req.params?.threadId, clientUserMessageId: req.params?.clientUserMessageId ?? null, text: textOf(req.params?.input), queuedId: p.result?.queuedSubmission?.id ?? null, error: err });
      else if (e.answers === 'thread/turns/list') {
        f.turnsLists.push({
          ...base,
          threadId: req.params?.threadId,
          error: err,
          turns: (p.result?.data ?? []).map((tn) => ({
            id: tn?.id,
            status: tn?.status ?? null,
            startedAt: tn?.startedAt ?? null,
            completedAt: tn?.completedAt ?? null,
            userTexts: (tn?.items ?? []).filter((it) => it?.type === 'userMessage').map((it) => textOf(it.content)),
            userClientIds: (tn?.items ?? []).filter((it) => it?.type === 'userMessage').map((it) => it.clientId ?? null),
            agentMessages: (tn?.items ?? []).filter((it) => it?.type === 'agentMessage').map((it) => it.text ?? ''),
          })),
        });
      }
      continue;
    }
    const prm = p.params ?? {};
    if (p.method === 'turn/started') f.events.turnStarted.push({ ...base, threadId: prm.threadId, turnId: prm.turn?.id ?? null });
    else if (p.method === 'turn/completed') f.events.turnCompleted.push({ ...base, threadId: prm.threadId, turnId: prm.turn?.id ?? null, status: prm.turn?.status ?? null, agentMessages: (prm.turn?.items ?? []).filter((it) => it?.type === 'agentMessage').map((it) => it.text ?? '') });
    else if ((p.method === 'item/started' || p.method === 'item/completed') && prm.item?.type === 'userMessage') f.events.userItems.push({ ...base, method: p.method, threadId: prm.threadId, turnId: prm.turnId ?? null, clientId: prm.item.clientId ?? null, text: textOf(prm.item.content) });
    else if (p.method === 'item/completed' && prm.item?.type === 'agentMessage') f.events.agentMessages.push({ ...base, threadId: prm.threadId, turnId: prm.turnId ?? null, text: prm.item.text ?? '' });
    else if (p.method === 'thread/status/changed') f.events.statusChanged.push({ ...base, threadId: prm.threadId, type: statusType(prm.status) });
  }
  return f;
}

// The TUI's thread: listed in thread/list with the operator's prompt as its preview and the
// scratch project directory as its cwd, and loaded in the daemon (thread/loaded/list).
// originator/source are deliberately NOT used (G2-result.md: they track the
// first-connecting client, not the thread's creator). -> { threadId, candidates, why }
export function identifyTuiThread(facts, { operatorPrompt, projectDirs, sinceLine = 0 }) {
  const loaded = new Set(facts.loadedLists.filter((l) => l.line > sinceLine).flatMap((l) => l.data));
  const norm = (p) => String(p).replace(/\\/g, '/').replace(/\/+$/, '');
  const dirs = new Set(projectDirs.map(norm));
  const want = String(operatorPrompt).trim();
  const cands = new Map();
  for (const tl of facts.threadLists.filter((l) => l.line > sinceLine)) {
    for (const th of tl.entries) {
      const previewOk = typeof th.preview === 'string' && th.preview.trim().length > 0 && (th.preview.trim() === want || want.startsWith(th.preview.trim().replace(/…$|\.\.\.$/, '')));
      const cwdOk = th.cwds.some((c) => dirs.has(norm(c)));
      if (previewOk && cwdOk && loaded.has(th.id)) cands.set(th.id, { id: th.id, listLine: tl.line });
    }
  }
  const list = [...cands.values()];
  if (list.length === 1) return { threadId: list[0].id, candidates: list, why: null };
  return { threadId: null, candidates: list, why: list.length ? 'more than one loaded thread matches the operator prompt and project directory' : 'no loaded thread matches the operator prompt and project directory yet' };
}

// --- fixture sanitizer ---------------------------------------------------------------------
//
// What the human runs' redaction did to the committed G2 fixtures (G2-result.md "Redaction"),
// done structurally here before the generic redaction in run.mjs:
//   - thread/list results keep only the run's own TUI thread (other saved sessions carry
//     private prompt previews and rollout paths) and are marked with _fixtureNote;
//   - remoteControl/status/changed serverName -> <HOST>, installationId -> <INSTALLATION_ID>;
//   - account plan and credit fields -> <REDACTED>.
// Returns the sanitized text, counts, and the installation ids seen (for the caller to also
// register as run literals; they are never recorded themselves).
export function sanitizeTranscript(text, { ownThreadId = null } = {}) {
  const lines = String(text ?? '').split('\n');
  const report = { threadListEntriesRemoved: 0, threadListResultsTouched: 0, serverNames: 0, installationIds: 0, planFields: 0, creditFields: 0, unparsedLines: 0 };
  const installationIds = new Set();
  const pending = new Map(); // `${mode}\0${id}` -> method (per open connection of a mode)
  const out = lines.map((raw) => {
    if (!raw.trim()) return raw;
    let e;
    try {
      e = JSON.parse(raw);
    } catch {
      report.unparsedLines += 1;
      return raw; // left for run.mjs's fail-closed redaction and residual scan
    }
    const p = e.payload;
    if (e.direction === 'handshake') for (const k of [...pending.keys()]) if (k.startsWith(`${e.mode}\0`)) pending.delete(k);
    if (e.direction === 'client->daemon' && p?.id !== undefined && p.method) pending.set(`${e.mode}\0${p.id}`, p.method);
    if (e.direction === 'daemon->client' && p && typeof p === 'object') {
      const answered = p.id !== undefined && !p.method ? pending.get(`${e.mode}\0${p.id}`) : null;
      if (answered === 'thread/list' && Array.isArray(p.result?.data)) {
        const keep = p.result.data.filter((th) => ownThreadId && th?.id === ownThreadId);
        const removed = p.result.data.length - keep.length;
        if (removed) {
          p.result.data = keep;
          p.result._fixtureNote = 'unrelated sessions removed for privacy';
          report.threadListEntriesRemoved += removed;
          report.threadListResultsTouched += 1;
        }
      }
      const prm = p.params;
      if (p.method === 'remoteControl/status/changed' && prm) {
        if (typeof prm.serverName === 'string' && prm.serverName !== '<HOST>') {
          prm.serverName = '<HOST>';
          report.serverNames += 1;
        }
        if (typeof prm.installationId === 'string' && prm.installationId !== '<INSTALLATION_ID>') {
          installationIds.add(prm.installationId);
          prm.installationId = '<INSTALLATION_ID>';
          report.installationIds += 1;
        }
      }
      if (p.method === 'account/updated' && prm && prm.planType != null && prm.planType !== '<REDACTED>') {
        prm.planType = '<REDACTED>';
        report.planFields += 1;
      }
      if (p.method === 'account/rateLimits/updated' && prm?.rateLimits) {
        for (const k of ['planType', 'credits']) {
          if (prm.rateLimits[k] != null && prm.rateLimits[k] !== '<REDACTED>') {
            prm.rateLimits[k] = '<REDACTED>';
            report[k === 'credits' ? 'creditFields' : 'planFields'] += 1;
          }
        }
      }
    }
    return JSON.stringify(e);
  });
  return { text: out.join('\n'), report, installationIds: [...installationIds] };
}

// --- method-sequence comparison, one client connection against another ------------------

// Notifications the daemon pushes to every connection regardless of what the client did.
export const AMBIENT_NOTIFICATIONS = new Set(['remoteControl/status/changed', 'account/updated', 'account/rateLimits/updated', 'thread/tokenUsage/updated']);

export function connectionKeys(entries, connIndex, { ambient = false } = {}) {
  return entries
    .filter((e) => e.conn === connIndex)
    .filter((e) => ambient || !AMBIENT_NOTIFICATIONS.has(e.payload?.method))
    .map((e) => {
      const p = e.payload;
      if (e.direction === 'handshake') return `handshake ${String(p ?? '').split('\r\n')[0]}`;
      if (e.direction === 'client->daemon') return `c->d ${p?.id !== undefined ? 'request' : 'notification'} ${p?.method}`;
      if (e.direction === 'daemon->client') {
        if (p?.method) return `d->c ${p.id !== undefined ? 'server-request' : 'notification'} ${p.method}`;
        return `d->c ${p?.error ? `error ${p.error.code}` : 'result'} (${e.answers ?? '?'})`;
      }
      return e.direction;
    });
}

// For each baseline connection mode (list, turn, busyqueue, turns), diff the baseline's
// connection of that mode against the run's last connection of that mode, ambient
// notifications left out.
export function compareByMode(baselineEntries, runEntries) {
  const b = baselineEntries.map((e) => ({ ...e }));
  const r = runEntries.map((e) => ({ ...e }));
  const bc = annotateConnections(b);
  const rc = annotateConnections(r);
  const out = [];
  for (const mode of [...new Set(bc.map((c) => c.mode))]) {
    const bConn = bc.filter((c) => c.mode === mode).at(-1);
    const rConn = rc.filter((c) => c.mode === mode).at(-1);
    const bk = connectionKeys(b, bConn.index);
    const rk = rConn ? connectionKeys(r, rConn.index) : [];
    const ops = diffSequences(bk, rk);
    out.push({ mode, baselineLines: [bConn.firstLine, bConn.lastLine], runLines: rConn ? [rConn.firstLine, rConn.lastLine] : null, ops, same: ops.every((o) => o.op === '=') });
  }
  const extra = [...new Set(rc.map((c) => c.mode))].filter((m) => !bc.some((c) => c.mode === m));
  return { modes: out, runOnlyModes: extra };
}

export function formatModeDiff(cmp) {
  const lines = [];
  for (const m of cmp.modes) {
    lines.push(`== ${m.mode}: baseline lines ${m.baselineLines.join('-')}, this run ${m.runLines ? `lines ${m.runLines.join('-')}` : 'no such connection'}${m.same ? ' (same method sequence)' : ''}`);
    for (const o of m.ops) lines.push(`${o.op === '=' ? ' ' : o.op} ${o.key}`);
  }
  if (cmp.runOnlyModes.length) lines.push(`== connections only in this run: ${cmp.runOnlyModes.join(', ')}`);
  lines.push('(ambient notifications -- remoteControl/status/changed, account/*, thread/tokenUsage/updated -- are left out)');
  return lines.join('\n');
}
