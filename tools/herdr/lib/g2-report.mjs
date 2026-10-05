#!/usr/bin/env node
// Draft the criterion-by-criterion comparison of a scripted G2 run against the human-run
// `0.157.1` re-run (Epic K, K7 #130): `docs/planning/gates/herdr-runs/G2-<YYYY-MM-DD>.md`.
//
//   node tools/herdr/lib/g2-report.mjs --run <run dir>
//        [--score 3=equivalent|not-equivalent --note 3='<why, citing pane lines>']
//        [--write] [--root <dir>] [--baseline <fixture>]
//
// <run dir> is the --out directory of `node tools/herdr/run.mjs --scenario g2-codex-inject`
// (run-manifest.json plus the redacted captures). Without --write the draft is printed.
// With --write it lands in the repository: the report and the run manifest beside it
// (`G2-<date>.md`, `G2-<date>.run-manifest.json` under docs/planning/gates/herdr-runs/), the
// two captures under docs/planning/gates/fixtures/g2-codex-inject/, and draft MANIFEST.json
// entries (with the `schema` and K5 `driver` blocks) in <run dir>/manifest-entries.draft.json
// for the operator to review and merge. Existing files are never overwritten. --root writes
// under another directory instead of the repository (used by the self-test).
//
// NOT VERDICT-BEARING. The record never changes G2's verdict, STATUS.md or PINS.md. Its
// Verification section (#252; oac-gates references/scripted-runs.md "Verification") states
// herdr, the harness and every dialog accept as verified from the run manifest, with the
// field cited, or UNVERIFIED; the recording agent re-checks it and fills its slots.
//
// Scoring: each criterion is `equivalent`, `not equivalent`, or `not evaluable` against the
// human run's fixture (docs/planning/gates/fixtures/g2-codex-inject/
// transcript-2026-09-26-0.157.1.jsonl) and G2-result.md.
//   - Criteria 1, 2 and 4 are scored mechanically: the client's own wire transcript, the
//     driver's command log, the OS-read pane process argv, and the run manifest.
//   - Criterion 3 needs a human reading of the verbatim pane text (did the TUI show the
//     delivered messages and the answers). Its mechanical preconditions are checked here;
//     the score stays `not evaluable` until the operator supplies it with --score/--note.
//   - herdr agent state never scores anything.
//   - Any run outcome other than PASS makes every criterion `not evaluable`.

import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  BASELINE_TRANSCRIPT, COMMITTED_CLIENT, COMMITTED_CLIENT_SHA256, EXPECTED_REPLIES, FIXTURE_DIR, HERDR_RUNS_DIR, MANIFEST_PATH, BUSY_TEXT_MARK, QUEUED_TEXT_MARK,
  DEFAULT_OPERATOR_PROMPT, compareByMode, formatModeDiff, g2Facts, identifyTuiThread, parseG2Transcript, readG2Criteria, G2_CRITERIA_SHA256, CriteriaDriftError,
} from './g2.mjs';
import { parseSections, committedFile, sha256 } from './g1.mjs';
import { describeDialogs, harnessVerification, lineSpan, noConsentCriterionLine, verification } from './gate-report-common.mjs';
import { CODEX_DAEMON_VERSION_FIELDS } from './pins.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');

export const SCORES = Object.freeze({ EQ: 'equivalent', NEQ: 'not equivalent', NE: 'not evaluable' });
export class ReportError extends Error {}

// Where the run's scratch project directory appears in a redacted capture.
const RUN_PROJECT_DIRS = ['<SCRATCH>/g2-project'];
// The human run's own project directory, as its fixture redacts it.
const BASELINE_PROJECT_DIRS = ['<SPIKE_DIR>'];

// operatorScores: { 3: { score, note } }
export function parseOperatorScores(pairs) {
  const out = {};
  for (const { n, score, note } of pairs) {
    if (n !== 3) throw new ReportError(`criterion ${n} is scored mechanically from the wire transcript, the command log and the run manifest; only criterion 3 takes an operator score`);
    const s = score === 'not-equivalent' ? SCORES.NEQ : score;
    if (![SCORES.EQ, SCORES.NEQ].includes(s)) throw new ReportError(`criterion ${n}: score must be equivalent or not-equivalent`);
    if (!note || !String(note).trim()) throw new ReportError(`criterion ${n}: an operator score needs a --note saying what in the pane text supports it`);
    out[n] = { score: s, note: String(note).trim() };
  }
  return out;
}

const check = (name, ok, detail = null) => ({ name, ok: !!ok, detail });
const required = (row) => row.checks.filter((c) => !c.name.startsWith('(supporting'));
const failed = (row) => required(row).filter((c) => !c.ok).map((c) => c.name).join('; ');

// Keys whose presence in a frame the client sent would mean it carried a credential.
const CREDENTIAL_KEY = /(?:token|secret|password|passwd|api[_-]?key|authorization|credential|bearer|cookie)/i;
const CREDENTIAL_VALUE = /\bBearer\s+\S|\bsk-[A-Za-z0-9_-]{8,}/;
export function credentialShapedFields(value, path = '$', out = []) {
  if (Array.isArray(value)) value.forEach((v, i) => credentialShapedFields(v, `${path}[${i}]`, out));
  else if (value && typeof value === 'object') {
    for (const [k, v] of Object.entries(value)) {
      if (CREDENTIAL_KEY.test(k)) out.push(`${path}.${k}`);
      credentialShapedFields(v, `${path}.${k}`, out);
    }
  } else if (typeof value === 'string' && CREDENTIAL_VALUE.test(value)) out.push(path);
  return out;
}

export function evaluateG2({ manifest, transcriptText, paneText, baselineText, criteria, operatorScores = {} }) {
  // The checks below are written for exactly these four criteria in this order; any other
  // text is refused rather than scored (lib/g2.mjs G2_CRITERIA_SHA256).
  if (!Array.isArray(criteria) || sha256(JSON.stringify(criteria)) !== G2_CRITERIA_SHA256) {
    throw new CriteriaDriftError(`the criteria given hash to ${Array.isArray(criteria) ? sha256(JSON.stringify(criteria)) : 'nothing'}, not the ${G2_CRITERIA_SHA256} this scoring is written against; the reference changed, re-review K7's scoring`);
  }
  const g2 = manifest?.scenarioData?.g2 ?? null;
  const baseEntries = parseG2Transcript(baselineText);
  const base = g2Facts(baseEntries);
  const runEntries = transcriptText ? parseG2Transcript(transcriptText) : null;
  const run = runEntries ? g2Facts(runEntries) : null;
  const sections = paneText ? parseSections(paneText) : [];
  const commands = manifest?.commands ?? [];
  const rows = criteria.map((text, i) => ({ n: i + 1, text, baseline: null, run: null, checks: [], score: SCORES.NE, reason: null, operator: null }));
  const [c1, c2, c3, c4] = rows;

  // --- the human run, from its fixture and G2-result.md -----------------------------------
  const bt = identifyTuiThread(base, { operatorPrompt: DEFAULT_OPERATOR_PROMPT, projectDirs: BASELINE_PROJECT_DIRS });
  const bLoaded = base.loadedLists.at(-1);
  c1.baseline = `plain \`codex\` launched by the operator (G2-result.md); thread/loaded/list (line ${bLoaded?.line}) returned ${bLoaded?.data.length} loaded thread(s), the TUI's ${bt.threadId ?? '?'} identified by thread/list preview and cwd (line ${bt.candidates[0]?.listLine ?? '?'}); process argv not recorded`;
  const bTurns = base.turnsLists.at(-1)?.turns ?? [];
  c2.baseline = `${base.turnStarts.map((t) => `turn/start line ${t.reqLine} -> ${t.turnId}${t.error ? ' ERROR' : ''}`).join('; ')}; ${base.queueAdds.map((q) => `thread/queue/add line ${q.reqLine} -> queuedSubmission ${q.queuedId}`).join('; ')}; daemon turn record (line ${base.turnsLists.at(-1)?.line}): ${bTurns.map((t) => `${t.status} "${String(t.agentMessages[0] ?? '').split('\n')[0].slice(0, 30)}"`).join(', ')}`;
  c3.baseline = 'the operator\'s own TUI paste (G2-result.md): the injected message and "OAC G2 RERUN RECEIVED", the lighthouse list, then the queued message and "OAC G2 QUEUED", in that order';
  c4.baseline = 'the client has no credential handling and never opens the Codex home; the committed transcript carries no token, bearer value or API key; post-redaction scan found 0 residual matches (G2-result.md)';

  const outcome = manifest?.outcome ?? 'missing';
  if (outcome !== 'PASS' || !g2 || !run) {
    const why = outcome !== 'PASS' ? `run outcome ${outcome}${manifest?.outcomeReason ? `: ${manifest.outcomeReason}` : ''}` : !g2 ? 'no G2 scenario record in the run manifest' : 'no wire transcript captured';
    for (const r of rows) {
      r.run = 'not run to completion';
      r.reason = why;
    }
    return { rows, base, run, sections };
  }

  const tid = g2.thread?.id ?? null;
  const watchAfter = (list) => list.filter((x) => x.mode === 'watch');

  // --- criterion 1 ------------------------------------------------------------------------
  const found = identifyTuiThread(run, { operatorPrompt: g2.operatorInput?.text ?? DEFAULT_OPERATOR_PROMPT, projectDirs: RUN_PROJECT_DIRS, sinceLine: run.loadedLists[0]?.line ?? 0 });
  const proof = g2.paneArgv?.[0]?.proof ?? null;
  const preLoaded = run.loadedLists[0]?.data ?? null;
  const prompts = commands.filter((c) => c.role === 'operator-input' && c.argv.includes('prompt'));
  c1.run = `daemon start exit ${g2.daemon?.start?.exitCode}; launch ${JSON.stringify(manifest.launch?.argv)} (herdr-reported ${JSON.stringify(g2.agentStart?.herdrReportedArgv ?? null)}); pane process argv ${proof?.found ? JSON.stringify(g2.paneArgv[0].argv.find((a) => a.pid === proof.pid)?.argv ?? null) : 'not shown'}; TUI thread ${found.threadId ?? 'not identified'}${found.candidates[0] ? ` (thread/list line ${found.candidates[0].listLine})` : ''}`;
  c1.checks.push(
    check('`codex app-server daemon start` succeeded and the daemon reports running', g2.daemon?.start?.exitCode === 0 && g2.versions?.daemon?.status === 'running', `status ${g2.versions?.daemon?.status}`),
    check('launch parameter is plain `codex` (herdr agent kind codex, nothing after it)', g2.launch?.verbatim === true && JSON.stringify(manifest.launch?.argv) === '["codex"]' && (g2.agentStart?.herdrReportedArgv === null || JSON.stringify(g2.agentStart?.herdrReportedArgv) === '["codex"]')),
    check('pane process argv, read from the OS, shows `codex` with no argument after it', proof?.found === true && proof.plain === true, proof?.found ? `pid ${proof.pid}, args after codex ${JSON.stringify(proof.argsAfterCodex)}` : 'no codex process argv on record'),
    check('the TUI thread is loaded in the daemon and identified by thread/list preview and cwd (not originator/source)', !!found.threadId && found.threadId === tid, found.why),
    check('that thread was not loaded before the launch (first thread/loaded/list)', Array.isArray(preLoaded) && !!tid && !preLoaded.includes(tid), preLoaded ? `line ${run.loadedLists[0].line}: ${preLoaded.length} loaded` : null),
    check('exactly one operator prompt went to the TUI (the operator\'s own message)', prompts.length === 1, `${prompts.length}`),
  );
  if (!(proof?.found === true)) {
    c1.score = SCORES.NE;
    c1.reason = 'the plain launch is not shown from the pane process argv (OS argv unavailable on this platform, or no codex process found)';
  } else {
    c1.score = required(c1).every((c) => c.ok) ? SCORES.EQ : SCORES.NEQ;
    c1.reason = c1.score === SCORES.EQ ? 'plain launch shown from the process argv; the TUI\'s thread loaded in the daemon, identified as the human run identified it' : `not met: ${failed(c1)}`;
  }

  // --- criterion 2 ------------------------------------------------------------------------
  const inj = run.turnStarts.filter((t) => t.mode === 'turn');
  const busy = run.turnStarts.filter((t) => t.mode === 'busyqueue');
  const qa = run.queueAdds.filter((q) => q.mode === 'busyqueue');
  const done = (id) => watchAfter(run.events.turnCompleted).find((e) => e.turnId === id);
  const startedEv = (id) => watchAfter(run.events.turnStarted).find((e) => e.turnId === id);
  const injDone = inj[0] ? done(inj[0].turnId) : null;
  const busyStarted = busy[0] ? startedEv(busy[0].turnId) : null;
  const busyDone = busy[0] ? done(busy[0].turnId) : null;
  const queuedItem = qa[0] ? watchAfter(run.events.userItems).find((u) => u.clientId && u.clientId === qa[0].clientUserMessageId) : null;
  const queuedStarted = queuedItem ? startedEv(queuedItem.turnId) : null;
  const queuedDone = queuedItem ? done(queuedItem.turnId) : null;
  const record = run.turnsLists.filter((t) => t.mode === 'turns').at(-1);
  const recIds = (record?.turns ?? []).map((t) => t.id);
  const wantOrder = [queuedItem?.turnId, busy[0]?.turnId, inj[0]?.turnId]; // thread/turns/list is newest first
  const typed = commands.filter((c) => ['operator-input', 'dialog-accept'].includes(c.role) && c.argv.some((a) => [g2.inject?.text, BUSY_TEXT_MARK, QUEUED_TEXT_MARK, 'OAC G2'].some((m) => m && String(a).includes(m))));
  c2.run = `turn/start ${inj.map((t) => `line ${t.reqLine} -> ${t.turnId ?? 'ERROR'}`).join(', ') || 'none'} ; busy turn/start ${busy.map((t) => `line ${t.reqLine} -> ${t.turnId ?? 'ERROR'}`).join(', ') || 'none'} ; thread/queue/add ${qa.map((q) => `line ${q.reqLine} -> ${q.queuedId ?? 'ERROR'}`).join(', ') || 'none'} ; daemon turn record line ${record?.line ?? '?'}: ${(record?.turns ?? []).slice(0, 3).map((t) => t.status).join(', ')}`;
  c2.checks.push(
    check('turn/start from the second client into the idle TUI thread accepted', inj.length === 1 && !inj[0].error && !!inj[0].turnId && inj[0].threadId === tid),
    check('that turn completed on the subscribed event stream (turn/completed)', injDone?.status === 'completed', injDone ? `line ${injDone.line}` : null),
    check('busy turn/start accepted and its turn visibly started on the event stream', busy.length === 1 && !busy[0].error && !!busyStarted, busyStarted ? `turn/started line ${busyStarted.line}` : null),
    check('thread/queue/add accepted (queuedSubmission id) while the busy turn was in progress', qa.length === 1 && !qa[0].error && !!qa[0].queuedId && !!busyStarted && !!busyDone && Date.parse(qa[0].reqT) >= Date.parse(busyStarted.t) && Date.parse(qa[0].reqT) < Date.parse(busyDone.t), qa[0] ? `request line ${qa[0].reqLine} at ${qa[0].reqT}; busy turn ${busyStarted?.t ?? '?'} to ${busyDone?.t ?? '?'}` : null),
    check('the queued message ran as its own turn after the busy turn completed (in order), and completed', !!queuedStarted && !!busyDone && queuedStarted.line > busyDone.line && queuedDone?.status === 'completed', queuedStarted ? `busy turn/completed line ${busyDone?.line}, queued turn/started line ${queuedStarted.line}` : 'queued turn not seen on the event stream'),
    check('the daemon\'s own turn record lists the three delivered turns, completed, newest first', !!record && wantOrder.every(Boolean) && JSON.stringify(recIds.slice(0, 3)) === JSON.stringify(wantOrder) && record.turns.slice(0, 3).every((t) => t.status === 'completed'), record ? `line ${record.line}` : 'no thread/turns/list'),
    check('never typed by herdr: no herdr input command carries a delivered text', typed.length === 0, typed.length ? `commands #${typed.map((c) => c.seq).join(', #')}` : null),
    check('each delivery sent once: one turn/start (`turn`), one turn/start and one thread/queue/add (`busyqueue`)', inj.length === 1 && busy.length === 1 && qa.length === 1 && (g2.injectionsSent ?? []).length === 2),
    check('(supporting only) the answers match the human run\'s', injDone?.agentMessages.includes(EXPECTED_REPLIES.inject) && queuedDone?.agentMessages.includes(EXPECTED_REPLIES.queued), `${JSON.stringify(injDone?.agentMessages ?? [])}, ${JSON.stringify(queuedDone?.agentMessages ?? [])}`),
  );
  c2.score = required(c2).every((c) => c.ok) ? SCORES.EQ : SCORES.NEQ;
  c2.reason = c2.score === SCORES.EQ ? 'both delivery paths, turn/start into the idle thread and thread/queue/add into the busy one, as the human run; completion taken from the event stream' : `not met: ${failed(c2)}`;

  // --- criterion 3: pane text, operator reading ---------------------------------------------
  const injSec = sections.find((s) => s.seq === g2.inject?.afterReadSeq);
  const bqSec = sections.find((s) => s.seq === g2.busyQueue?.afterReadSeq);
  // An answer line: holds the expected words but is not the delivered message quoting them.
  const answerAt = (text, words) => String(text ?? '').split('\n').findIndex((l) => l.includes(words) && !/reply with exactly/i.test(l));
  const at = (text, s) => String(text ?? '').indexOf(s);
  c3.run = `pane reads after the delivered turn (#${g2.inject?.afterReadSeq ?? '?'}) and after the busy and queued turns (#${g2.busyQueue?.afterReadSeq ?? '?'})`;
  c3.checks.push(
    check('pane text after the delivered turn captured verbatim', !!injSec),
    check('pane text after the busy and queued turns captured verbatim', !!bqSec),
    check('(supporting only) the pane text holds the delivered message and, as a separate line, its answer', injSec && at(injSec.text, 'second daemon client') !== -1 && answerAt(injSec.text, EXPECTED_REPLIES.inject) !== -1, 'a text match only; an operator must read the pane text itself'),
    check('(supporting only) the pane text holds the busy message, then the queued message, then its answer', bqSec && at(bqSec.text, BUSY_TEXT_MARK) !== -1 && at(bqSec.text, QUEUED_TEXT_MARK) > at(bqSec.text, BUSY_TEXT_MARK) && answerAt(bqSec.text, EXPECTED_REPLIES.queued) !== -1),
  );
  const op = operatorScores[3];
  if (!required(c3).every((c) => c.ok)) {
    c3.reason = `mechanical preconditions not met (${failed(c3)})${op ? '; the operator score was not applied' : ''}`;
  } else if (!op) c3.reason = 'operator review of the verbatim pane text pending (--score 3=... --note 3=...)';
  else {
    c3.score = op.score;
    c3.operator = op;
    c3.reason = `operator: ${op.note}`;
  }

  // --- criterion 4 --------------------------------------------------------------------------
  const sentFrames = runEntries.filter((e) => e.direction === 'client->daemon');
  const credFields = sentFrames.flatMap((e) => credentialShapedFields(e.payload).map((p) => `line ${e.line} ${p}`));
  const capsClean = (manifest.captures ?? []).length > 0 && manifest.captures.every((c) => c.written && c.redaction?.hazardProtocolFrames?.length === 0 && c.redaction?.residualLeaks?.length === 0 && c.redaction?.residualGenericHits?.length === 0);
  const authMode = runEntries.find((e) => e.payload?.method === 'account/updated')?.payload?.params?.authMode ?? null;
  c4.run = `client = committed blob (sha256 ${g2.client?.copySha256 ?? '?'}); ${sentFrames.length} client frames, ${credFields.length} credential-shaped field(s); captures clean: ${capsClean}; daemon account/updated authMode ${authMode ?? 'not seen'}`;
  c4.checks.push(
    check('the client that ran is the committed G2 client, unmodified (sha256 of the copy = the blob at HEAD = the one the G2 runs used)', g2.client?.match === true && g2.client?.workingTreeMatchesHead === true && g2.client?.committedSha256 === COMMITTED_CLIENT_SHA256 && g2.client?.copySha256 === COMMITTED_CLIENT_SHA256),
    check('no frame the client sent carries a credential-shaped field or value', credFields.length === 0, credFields.slice(0, 5).join('; ') || null),
    check('both captures written with no hazard frame and no residual hit after redaction', capsClean),
    check('harness config (the Codex config and hooks files) byte-identical before and after', manifest.harnessConfig?.unchanged === true),
    check('(supporting only) the daemon reported its own sign-in mode on the wire', !!authMode, 'the model turns ran on the daemon\'s own sign-in; nothing on the wire shows its source directly (as G2-result.md)'),
  );
  c4.score = required(c4).every((c) => c.ok) ? SCORES.EQ : SCORES.NEQ;
  c4.reason =
    c4.score === SCORES.EQ
      ? 'same basis as the human run: a credential-free client, a clean transcript. Driver-side, no credential access is enforced by scripts/check-herdr-containment.mjs (oac-boundaries check 10) and traced by the self-test; which herdr and harness executables ran is in the Verification section'
      : `not met: ${failed(c4)}`;

  return { rows, base, run, sections };
}

const cell = (s) => String(s ?? '').replace(/\|/g, '\\|').replace(/\n/g, ' ');

export function renderReport({ manifest, evaluation, diffText, date, fixtures, runManifestName, baselinePath = BASELINE_TRANSCRIPT, reference = null }) {
  const g2 = manifest?.scenarioData?.g2 ?? {};
  const v = g2.versions ?? {};
  const d = v.daemon ?? {};
  const out = [];
  out.push(`# G2 scripted re-run through herdr, ${date}: comparison with the human-run 0.157.1 re-run`);
  out.push('');
  out.push('> **Not verdict-bearing.** Epic K, K7 #130. This record compares a herdr-driven run of');
  out.push('> G2 against the human-run `0.157.1` re-run. G2\'s verdict');
  out.push('> (`docs/planning/gates/G2-result.md`) and `docs/planning/STATUS.md` are unchanged by it.');
  out.push('> Its Verification section is generated from the run manifest; until the recording agent has');
  out.push('> re-checked it and filled its slots, this is neither an equivalence record nor verdict-bearing.');
  out.push('');
  out.push(`- **Driver:** herdr (\`${manifest?.herdr?.observedVersionOutput ?? '?'}\`, PINS.md \`herdr (test tooling)\` ${manifest?.herdr?.pinnedTag ?? '?'}) via \`tools/herdr/run.mjs\`, scenario \`${manifest?.scenario?.file ?? '?'}\`, driver commit \`${manifest?.driver?.commit ?? '?'}\`${manifest?.driver?.toolsHerdrDirty !== false ? ` (tools/herdr dirty: ${manifest?.driver?.toolsHerdrDirty})` : ''}`);
  out.push(`- **Run outcome:** ${manifest?.outcome ?? '?'}${manifest?.outcomeReason ? ` — ${manifest.outcomeReason}` : ''}`);
  out.push(`- **Codex version:** \`codex --version\` = \`${v.cliOutput ?? '?'}\` (post-run \`${g2.postRun?.cliOutput ?? 'not recorded'}\`); daemon ${CODEX_DAEMON_VERSION_FIELDS.map((k) => `${k} \`${d[k] ?? '?'}\``).join(', ')}; wire \`initialize\` userAgent \`${v.wireUserAgent ?? '?'}\`; PINS.md \`${v.pinsRow ?? 'Codex CLI / app-server'}\` ${pinsVersionsText(v)}; version warnings: ${v.warnings?.length ?? 0} (listed under Findings; versions float and are never gated, #216); version unchanged through the run: ${g2.postRun?.matches ?? '?'}`);
  out.push(`- **Launch:** \`${(manifest?.launch?.argv ?? []).join(' ')}\` (plain; herdr-reported argv \`${JSON.stringify(manifest?.launch?.herdrReportedArgv ?? null)}\`); pane process argv ${g2.paneArgv?.[0]?.proof?.found ? `pid ${g2.paneArgv[0].proof.pid}, arguments after \`${g2.paneArgv[0].proof.codexToken}\`: ${JSON.stringify(g2.paneArgv[0].proof.argsAfterCodex)}` : 'not shown'}`);
  out.push(`- **Daemon:** \`codex app-server daemon start\` exit ${g2.daemon?.start?.exitCode ?? '?'}; left running after the run (the operator's, as in the human run)`);
  out.push(`- **Timebox:** ${manifest?.timebox?.budgetMs ?? '?'} ms, ${manifest?.timebox?.start ?? '?'} to ${manifest?.timebox?.end ?? '?'}; expired: ${manifest?.timebox?.expired ?? '?'}`);
  out.push(`- **Accept policy:** ${g2.acceptPolicy ?? '?'}; dialogs on record: ${describeDialogs(g2.dialogs)} (no G2 criterion names a consent step)`);
  out.push(`- **herdr agent states seen** (scheduling only, never evidence): ${(g2.herdrStates ?? []).map((s) => `${s.state} (#${s.seq}, ${s.context})`).join(', ') || 'none'}`);
  out.push(`- **Client:** \`${g2.client?.committed ?? COMMITTED_CLIENT}\` run unmodified from a scratch copy of the blob at HEAD \`${g2.client?.headCommit ?? '?'}\`; sha256 committed \`${g2.client?.committedSha256 ?? '?'}\`, copy \`${g2.client?.copySha256 ?? '?'}\`, match: ${g2.client?.match ?? '?'}; working tree matched HEAD: ${g2.client?.workingTreeMatchesHead ?? '?'}; runs: ${(g2.clientRuns ?? []).map((r) => `${r.mode} (exit ${r.exitCode})`).join(', ') || 'none'}; divergence: ${(g2.divergence ?? []).join('; ') || 'none'}`);
  out.push(fixtures ? `- **Fixtures:** \`${fixtures.transcript}\`, \`${fixtures.pane}\`` : `- **Fixtures:** none (${writeRefusal(manifest) ?? fixtureWithheld(manifest) ?? 'not published'})`);
  out.push(`- **Fixture sanitizer:** ${g2.sanitizer ? `${g2.sanitizer.threadListEntriesRemoved} unrelated thread/list entr(ies) removed; serverName ${g2.sanitizer.serverNames}, installationId ${g2.sanitizer.installationIds}, plan ${g2.sanitizer.planFields}, credit ${g2.sanitizer.creditFields} field(s) replaced` : 'not run'}`);
  out.push(`- **Run manifest:** \`${runManifestName}\` (beside this file); harness config unchanged: ${manifest?.harnessConfig?.unchanged ?? '?'}; teardown clean: ${manifest?.teardown?.clean ?? '?'}`);
  out.push(`- **Baseline:** \`${baselinePath}\` and \`docs/planning/gates/G2-result.md\` (the human-run 0.157.1 re-run)`);
  out.push(`- **Criteria source:** ${reference ? `\`${reference.path}\` as committed at \`${reference.headCommit}\` (file sha256 \`${reference.fileSha256}\`); the four parsed criteria hash to \`${reference.criteriaSha256}\`, the pin the scoring is written against${reference.workingTreeMatchesHead ? '' : ' (the working-tree copy differs from HEAD and was not used)'}` : 'not recorded'}`);
  out.push('');
  out.push('## Criteria');
  out.push('');
  out.push('| # | Criterion (verbatim, `oac-gates` G2 reference) | Human run (0.157.1) | This run | Score |');
  out.push('|---|---|---|---|---|');
  for (const r of evaluation.rows) out.push(`| ${r.n} | ${cell(r.text)} | ${cell(r.baseline)} | ${cell(r.run)} | **${r.score}** |`);
  out.push('');
  for (const r of evaluation.rows) {
    out.push(`### Criterion ${r.n}: ${r.score}`);
    out.push('');
    out.push(`- Reason: ${r.reason ?? '—'}`);
    for (const c of r.checks) out.push(`- [${c.ok ? 'x' : ' '}] ${c.name}${c.detail ? ` — ${c.detail}` : ''}`);
    out.push('');
  }
  out.push('## Method-sequence diff');
  out.push('');
  out.push('Per client connection, the human run (baseline) against this run\'s last connection of the same mode (`tools/herdr/lib/g2.mjs` compareByMode):');
  out.push('');
  out.push('```text');
  out.push(diffText ?? '(no transcript captured)');
  out.push('```');
  out.push('');
  out.push('## Findings and UNVERIFIED');
  out.push('');
  for (const f of manifest?.findings ?? []) out.push(`- Finding: ${f}`);
  if (fixtureWithheld(manifest)) out.push(`- Finding: ${fixtureWithheld(manifest)}`);
  out.push('- Harness versions float and are never gated (#216): a version other than PINS.md\'s last tested one, or other than the baseline run\'s, is a finding here and does not by itself disqualify this record, including as an equivalence record.');
  out.push('- Earlier `NOT RUN` or `FAIL` runs of this scenario at the same pins: none listed by this generator; add each by hand (run id, outcome, reason from its run manifest).');
  out.push('- The delivered texts are the human run\'s, with "through herdr" added to injection 1; the busy and queued texts are the committed client\'s own. The operator prompt is a scenario parameter.');
  out.push('- Codex pane-text patterns (dialogs, the in-progress indicator) were written before any live run; confirm them against this run\'s pane capture.');
  out.push('- herdr agent states are recorded above for K1 §5 item 5 (what Codex settles in after a response); they scored nothing.');
  out.push('');
  out.push(...verification({
    manifest,
    harness: harnessVerification(manifest, {
      verified: versionsVerified(g2),
      versions: `Codex: CLI \`${v.cliOutput ?? '?'}\` (\`scenarioData.g2.versions.cliOutput\`, parsed \`${v.cli ?? '?'}\` in \`scenarioData.g2.versions.cli\`), daemon ${CODEX_DAEMON_VERSION_FIELDS.map((k) => `${k} \`${d[k] ?? '?'}\``).join(', ')} (\`scenarioData.g2.versions.daemon\`), wire \`initialize\` userAgent \`${v.wireUserAgent ?? '?'}\` (\`scenarioData.g2.versions.wireUserAgent\`, parsed \`${v.wire ?? '?'}\` in \`scenarioData.g2.versions.wire\`), post-run \`${g2.postRun?.cliOutput ?? 'not recorded'}\` (\`scenarioData.g2.postRun.cliOutput\`)`,
    }),
    dialogs: g2.dialogs,
    dialogsField: 'scenarioData.g2.dialogs',
    humanActions: noConsentCriterionLine('G2'),
  }));
  return out.join('\n');
}

// Verified when the CLI, all three daemon fields and the wire userAgent report one and the
// same version, and none of them moved during the run, so a fixture names one version.
// Whether that version is PINS.md's last tested one is NOT part of this: versions float
// and are never gated (#216); a difference is a VERSION WARNING finding and shows as
// version_matches_pin: false in the fixture manifest entry.
export function versionsVerified(g2) {
  const v = g2?.versions;
  if (!v || v.verified !== true || !v.cli) return false;
  return v.wire === v.cli && CODEX_DAEMON_VERSION_FIELDS.every((k) => v.daemon?.[k] === v.cli) && g2.postRun?.matches === true;
}

// Whether the verified version equals PINS.md's last tested version (informational only).
export function versionMatchesLastTested(g2) {
  return versionsVerified(g2) && !!g2.versions.pinsLastTested && g2.versions.cli === g2.versions.pinsLastTested;
}

// PINS.md's versions as the run recorded them. A run recorded before #216 carries only the
// then "last observed" version.
function pinsVersionsText(v) {
  if (v.pinsLastTested) return `minimum \`${v.pinsMinimum ?? '?'}\`, last tested \`${v.pinsLastTested}\` (commit \`${v.pinsCommit ?? '?'}\`)`;
  return `last observed \`${v.pinsLastObserved ?? '?'}\` (commit \`${v.pinsCommit ?? '?'}\`; recorded before #216)`;
}

// Why --write writes the record and run manifest but NO fixture, or null. Operator decision
// on #216 (2026-10-01): when the Codex CLI, daemon and wire disagree, or a version moved mid-run, the record is
// still written, with a VERSION WARNING; its captures stay `unverified-*` and no fixture
// or MANIFEST.json entry is produced, because a fixture names one version.
export function fixtureWithheld(manifest) {
  const g2 = manifest?.scenarioData?.g2;
  if (!g2 || versionsVerified(g2)) return null;
  return `VERSION WARNING: the CLI (${g2.versions?.cli ?? 'none'}), the daemon (${JSON.stringify(g2.versions?.daemon ?? null)}) and the wire (${g2.versions?.wire ?? 'none'}) did not report one and the same Codex version before and after the run, so no fixture can name it; the record and run manifest are written, the captures stay unverified-* and no fixture is added (#216)`;
}

// Why --write must refuse this run, or null. Mixed versions never refuse it (fixtureWithheld).
export function writeRefusal(manifest) {
  const g2 = manifest?.scenarioData?.g2;
  if (manifest?.outcome !== 'PASS') return `run outcome is ${manifest?.outcome ?? 'missing'}${manifest?.outcomeReason ? ` (${manifest.outcomeReason})` : ''}; only a PASS run is written`;
  if (!g2) return 'no G2 scenario record in the run manifest';
  if (versionsVerified(g2)) {
    if (!g2.fixtures || JSON.stringify(g2.fixtures) !== JSON.stringify(g2.captureNames)) return 'captures do not carry the verified K7 fixture names';
    for (const f of [g2.fixtures.transcript, g2.fixtures.pane]) {
      if (!manifest.captures?.some((c) => c.file === f && c.written)) return `capture ${f} was not written (withheld or missing)`;
    }
  }
  if (g2.client?.match !== true || g2.client?.workingTreeMatchesHead !== true) return 'the client copy is not verified against the committed blob';
  if (manifest.driver?.toolsHerdrDirty !== false || !manifest.driver?.commit) return `the run's tools/herdr/ was not clean and committed (toolsHerdrDirty ${JSON.stringify(manifest.driver?.toolsHerdrDirty ?? null)}); its captures are never committed as fixtures (scripted-runs.md "Driver identity")`;
  return null;
}

// The `schema` block for a draft entry. A Codex-touching MANIFEST entry needs one
// (scripts/check-fixture-manifest.mjs). When MANIFEST.json (as committed at HEAD) already
// holds a regenerated-schema record for this exact Codex version, its hashes are reused and
// the entry says it has NOT itself been validated against them yet; otherwise the
// not-regenerated shape, which carries no hash.
export function schemaBlockFor({ version, codexCommit, manifestJson }) {
  const entries = manifestJson?.fixtures ?? [];
  const src = entries.find((e) => typeof e.provider === 'string' && e.provider.includes('codex') && e.schema?.local_generation?.cli === `codex-cli ${version}` && e.schema?.upstream);
  if (src) {
    return {
      ...JSON.parse(JSON.stringify(src.schema)),
      validated_against: `NOT yet validated: draft entry from tools/herdr/lib/g2-report.mjs. The schema record is copied from ${src.path}'s entry for the same Codex version (${version}); validate this capture's frames against it (D6 procedure) before merging.`,
    };
  }
  return {
    commit: codexCommit ?? 'unknown',
    hash_method: entries.find((e) => typeof e.schema?.hash_method === 'string')?.schema.hash_method ?? 'not recorded',
    upstream: null,
    local_generation: null,
    sha256: null,
    note: `Schema not regenerated at Codex ${version} (no MANIFEST.json schema record for this version); regenerate and compare it (D6 procedure) before relying on this capture's message shapes.`,
    validated_against: 'not validated against a regenerated schema at this version',
  };
}

const PANE_SCHEMA = { applicable: false, reason: 'pane text: verbatim herdr agent reads of the Codex TUI screen, not app-server protocol traffic' };

export function draftManifestEntries({ manifest, fixtures, runManifestPath, transcriptText, pinsCommit, redactSha256, manifestJson }) {
  const g2 = manifest.scenarioData.g2;
  const v = g2.versions;
  const entries = parseG2Transcript(transcriptText);
  const f = g2Facts(entries);
  const cap = (name) => manifest.captures.find((c) => c.file === name)?.redaction ?? null;
  const residual = (name) => {
    const r = cap(name);
    return r ? `tools/herdr/lib/redact.mjs (run.mjs), after the lib/g2.mjs sanitizer (${g2.sanitizer ? `${g2.sanitizer.threadListEntriesRemoved} unrelated thread/list entries removed; host, installation id, plan and credit fields replaced` : 'not run'}): droppedHazardLines=${r.droppedHazardLines}, hazardProtocolFrames=${r.hazardProtocolFrames.length}, residualLeaks=${r.residualLeaks.length}, residualGenericHits=${r.residualGenericHits.length}` : 'not recorded';
  };
  const lines = (list) => list.map((x) => x.reqLine ? lineSpan(x.reqLine, x.line) : `${x.line}`).join(', ') || null;
  const common = {
    provider: 'codex',
    surface: 'codex-app-server',
    observed_version: {
      codex_cli: `${v.cli} (codex --version \`${v.cliOutput}\`)`,
      codex_daemon: `${CODEX_DAEMON_VERSION_FIELDS.map((k) => v.daemon?.[k]).join('/')} (cliVersion/appServerVersion/managedCodexVersion)`,
      commit: versionMatchesLastTested(g2) ? v.pinsCommit : null,
      node: null,
      client_visible_user_agent: `${v.wireUserAgent} (initialize result)`,
    },
    pins_row: 'Codex CLI / app-server',
    pins_as_of: `PINS.md as committed at HEAD ${v.pinsSource?.headCommit ?? '?'} when the run started (working tree matched HEAD: ${v.pinsSource?.workingTreeMatchesHead}); last commit touching PINS.md at report time: ${pinsCommit ?? 'unknown'}`,
    version_matches_pin: versionMatchesLastTested(g2),
    ...(versionMatchesLastTested(g2) ? {} : { version_matches_pin_note: `Codex ${v.cli} is not PINS.md's last tested ${v.pinsLastTested ?? v.pinsLastObserved ?? '?'} (minimum ${v.pinsMinimum ?? '?'}); recorded as a VERSION WARNING finding, not a gate (#216)` }),
    capture_date: g2.date,
    capture_utc_range: f.firstT && f.lastT ? `${f.firstT}-${f.lastT}` : null,
    superseded_by: null,
    driver: { herdr_version: manifest.herdr.observedVersionOutput, driver_commit: manifest.driver.commit, run_manifest: runManifestPath },
    notes: ['K7 scripted run through herdr; NOT verdict-bearing (G2 verdict unchanged). Compared against the human-run 0.157.1 re-run in the herdr-runs record beside the run manifest.'],
  };
  const redaction = (name) => ({ script: 'tools/herdr/lib/redact.mjs', script_sha256: redactSha256, residual_scan_result: residual(name) });
  const byMode = (list, mode) => list.filter((x) => x.mode === mode);
  return [
    {
      path: `${FIXTURE_DIR}/${fixtures.transcript}`,
      ...common,
      redaction: redaction(fixtures.transcript),
      coverage: {
        'initialize (upgrade + handshake), per connection': f.connections.map((c) => `${c.mode} ${c.firstLine}`).join(', '),
        'thread/loaded/list': lines(f.loadedLists),
        'thread/list': lines(f.threadLists),
        'thread/resume (event subscription)': lines(f.resumes),
        'turn/start (injection 1, idle thread)': lines(byMode(f.turnStarts, 'turn')),
        'turn/start (injection 2, busy-turn setup)': lines(byMode(f.turnStarts, 'busyqueue')),
        'thread/queue/add (injection 3, queued while busy)': lines(f.queueAdds),
        'thread/turns/list': lines(f.turnsLists),
        'item/completed': f.events.agentMessages.map((x) => x.line).join(', ') || null,
        'turn/completed': f.events.turnCompleted.map((x) => x.line).join(', ') || null,
        'thread/start': null,
      },
      schema: schemaBlockFor({ version: v.cli, codexCommit: versionMatchesLastTested(g2) ? v.pinsCommit : null, manifestJson }),
    },
    {
      path: `${FIXTURE_DIR}/${fixtures.pane}`,
      ...common,
      redaction: redaction(fixtures.pane),
      coverage: { 'pane reads': 'verbatim herdr agent reads of the Codex TUI, one section per kept read, each with its herdr command seq and timestamps' },
      schema: PANE_SCHEMA,
    },
  ];
}

function main(argv) {
  const o = { scores: [], notes: {}, write: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--run') o.run = argv[++i];
    else if (a === '--baseline') o.baseline = argv[++i];
    else if (a === '--write') o.write = true;
    else if (a === '--root') o.root = argv[++i];
    else if (a === '--score' || a === '--note') {
      const kv = argv[++i] ?? '';
      const eq = kv.indexOf('=');
      const n = Number(kv.slice(0, eq));
      if (eq < 1 || !Number.isInteger(n)) throw new ReportError(`${a} takes <criterion>=<value>`);
      if (a === '--score') o.scores.push({ n, score: kv.slice(eq + 1) });
      else o.notes[n] = kv.slice(eq + 1);
    } else throw new ReportError(`unknown argument ${a}`);
  }
  if (!o.run) throw new ReportError('--run <run dir> is required');
  const operatorScores = parseOperatorScores(o.scores.map((s) => ({ ...s, note: o.notes[s.n] })));
  const runDir = resolve(o.run);
  const manifest = JSON.parse(readFileSync(join(runDir, 'run-manifest.json'), 'utf8'));
  const g2 = manifest.scenarioData?.g2;
  const names = g2?.captureNames ?? g2?.fixtures ?? null;
  const fixtures = g2?.fixtures ?? null;
  const written = (name) => name && manifest.captures?.some((c) => c.file === name && c.written) && existsSync(join(runDir, name));
  const transcriptText = names && written(names.transcript) ? readFileSync(join(runDir, names.transcript), 'utf8') : null;
  const paneText = names && written(names.pane) ? readFileSync(join(runDir, names.pane), 'utf8') : null;
  const baselinePath = o.baseline ?? BASELINE_TRANSCRIPT;
  const baselineText = readFileSync(resolve(REPO, baselinePath), 'utf8');
  const { criteria, reference } = readG2Criteria(REPO); // throws, before anything is printed or written, if the criteria drifted
  const evaluation = evaluateG2({ manifest, transcriptText, paneText, baselineText, criteria, operatorScores });
  const diffText = transcriptText ? formatModeDiff(compareByMode(parseG2Transcript(baselineText), parseG2Transcript(transcriptText))) : null;
  const date = g2?.date ?? manifest.timebox?.start?.slice(0, 10) ?? 'unknown-date';
  const runManifestName = `G2-${date}.run-manifest.json`;
  const refusal = writeRefusal(manifest);
  const publish = !refusal && !fixtureWithheld(manifest);
  const report = renderReport({ manifest, evaluation, diffText, date, fixtures: publish ? { transcript: `${FIXTURE_DIR}/${fixtures.transcript}`, pane: `${FIXTURE_DIR}/${fixtures.pane}` } : null, runManifestName, baselinePath, reference });
  if (!o.write) {
    console.log(report);
    return 0;
  }
  if (refusal) throw new ReportError(`--write refused: ${refusal}. Nothing was written; print the draft without --write to inspect the run`);
  const root = o.root ? resolve(o.root) : REPO;
  const runsDir = join(root, HERDR_RUNS_DIR);
  const targets = [
    [join(runsDir, `G2-${date}.md`), null],
    [join(runsDir, runManifestName), join(runDir, 'run-manifest.json')],
    ...(publish ? [[join(root, FIXTURE_DIR, fixtures.transcript), join(runDir, fixtures.transcript)], [join(root, FIXTURE_DIR, fixtures.pane), join(runDir, fixtures.pane)]] : []),
  ];
  const exists = targets.filter(([t]) => existsSync(t)).map(([t]) => t);
  if (exists.length) throw new ReportError(`refusing to overwrite: ${exists.join(', ')}`);
  for (const [t] of targets) mkdirSync(dirname(t), { recursive: true });
  writeFileSync(targets[0][0], `${report}\n`);
  for (const [to, from] of targets.slice(1)) copyFileSync(from, to);
  if (!publish) {
    console.log(`wrote ${targets.map(([t]) => t).join('\n      ')}`);
    console.log(`No fixture written: ${fixtureWithheld(manifest)}`);
    return 0;
  }
  const git = spawnSync('git', ['log', '-1', '--format=%H', '--', 'docs/planning/PINS.md'], { cwd: REPO, encoding: 'utf8', timeout: 10000 });
  const entries = draftManifestEntries({
    manifest,
    fixtures,
    runManifestPath: `${HERDR_RUNS_DIR}/${runManifestName}`,
    transcriptText,
    pinsCommit: git.status === 0 ? git.stdout.trim() : null,
    redactSha256: createHash('sha256').update(readFileSync(join(HERE, 'redact.mjs'))).digest('hex'),
    manifestJson: JSON.parse(committedFile(REPO, MANIFEST_PATH).bytes.toString('utf8')),
  });
  writeFileSync(join(runDir, 'manifest-entries.draft.json'), `${JSON.stringify(entries, null, 2)}\n`);
  console.log(`wrote ${targets.map(([t]) => t).join('\n      ')}`);
  console.log('Next: review the draft and score criterion 3 from the pane text; the recording agent re-checks the Verification section and fills its slots; merge <run dir>/manifest-entries.draft.json into docs/planning/gates/fixtures/MANIFEST.json after validating the transcript against the schema; run node scripts/check-fixture-manifest.mjs; add only a pointer to G2-result.md.');
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (err) {
    console.error(`g2-report: ${err.message}`);
    process.exitCode = 2;
  }
}
