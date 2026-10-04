#!/usr/bin/env node
// Draft the criterion-by-criterion comparison of a scripted G1 run against Box C
// (Epic K, K4 #127): `docs/planning/gates/herdr-runs/G1-<YYYY-MM-DD>.md`.
//
//   node tools/herdr/lib/g1-report.mjs --run <run dir>
//        [--score 2=equivalent|not-equivalent --note 2='<why, citing pane lines>']
//        [--score 3=... --note 3='...'] [--write] [--root <dir>]
//
// <run dir> is the --out directory of `node tools/herdr/run.mjs --scenario g1-claude-wake`
// (run-manifest.json plus the redacted captures). Without --write the draft is printed.
// With --write it lands in the repository: the report and the run manifest beside it
// (`G1-<date>.md`, `G1-<date>.run-manifest.json` under docs/planning/gates/herdr-runs/), the
// two captures under docs/planning/gates/fixtures/g1-claude-wake/, and draft MANIFEST.json
// entries (with the K5 `driver` block) in <run dir>/manifest-entries.draft.json for the
// operator to review and merge. Existing files are never overwritten. --root writes under
// another directory instead of the repository (used by the self-test).
//
// NOT VERDICT-BEARING. The record never changes G1's verdict, STATUS.md or PINS.md.
//
// Scoring: each criterion is `equivalent`, `not equivalent`, or `not evaluable` against Box C.
//   - Criteria 1 and 4 are scored mechanically from the wire transcript.
//   - Criteria 2 and 3 need a human reading of the verbatim pane text (the `<channel>`
//     attribute answer; where the mid-turn notifications landed). The mechanical
//     preconditions are checked here; the score stays `not evaluable` until the operator
//     supplies it with --score/--note. Mid-turn timing comes from wire timestamps against
//     timestamped pane reads (lib/g1.mjs midTurnWindow), never from herdr agent state.
//   - Criterion 5: a DRIVER-SENT accept of the dev-channels dialog is never scored as
//     meeting it -- `not evaluable`, whatever else holds, and no --score can override that.
//     The #196 operator decision lets the driver accept Claude Code's dialogs in dev/test runs
//     by default, but keeps this: criterion 5 is about the consent step itself
//     (docs/planning/decisions/K-196-driver-accepts-dialogs.md). A human accept is
//     `equivalent` when the dialog text, read before any keystroke, matches Box C's.
//   - The record names who accepted each dialog as the run manifest records it (driver, with
//     its keys and herdr command seqs, or human), and its Verification section (#252) says
//     so; a human accept of the dev-channels dialog is the one human action it asks a
//     person to sign.
//   - Any run outcome other than PASS makes every criterion `not evaluable`.

import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { compareTranscripts, formatDiff, parseTranscript, selectSegment, transcriptFacts, isLegacyRevision } from './compare-transcripts.mjs';
import { BOX_C_TRANSCRIPT, BOX_C_WAKE_ATTRIBUTES, FIXTURE_DIR, G1_CRITERIA, HERDR_RUNS_DIR, dialogMatchesBoxC, midTurnWindow, parseSections } from './g1.mjs';
import { TO_FILL, describeDialog, describeDialogs, harnessVerification, verification } from './gate-report-common.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');

export const SCORES = Object.freeze({ EQ: 'equivalent', NEQ: 'not equivalent', NE: 'not evaluable' });
const INPUT_ROLES = new Set(['operator-input', 'dialog-accept']);

export class ReportError extends Error {}

// operatorScores: { 2: { score: 'equivalent'|'not equivalent', note }, 3: {...} }
export function parseOperatorScores(pairs) {
  const out = {};
  for (const { n, score, note } of pairs) {
    if (![2, 3].includes(n)) {
      throw new ReportError(
        n === 5
          ? 'criterion 5 cannot be scored by hand: a driver-sent accept is never scored as meeting it, and a human accept is scored from the recorded dialog text'
          : `criterion ${n} is scored mechanically from the wire transcript; only criteria 2 and 3 take an operator score`,
      );
    }
    const s = score === 'not-equivalent' ? SCORES.NEQ : score;
    if (![SCORES.EQ, SCORES.NEQ].includes(s)) throw new ReportError(`criterion ${n}: score must be equivalent or not-equivalent`);
    if (!note || !String(note).trim()) throw new ReportError(`criterion ${n}: an operator score needs a --note saying what in the pane text supports it`);
    out[n] = { score: s, note: String(note).trim() };
  }
  return out;
}

const check = (name, ok, detail = null) => ({ name, ok: !!ok, detail });

// Evaluate the five criteria. Inputs are texts/objects so tests can feed synthetic runs.
export function evaluateG1({ manifest, transcriptText, paneText, baselineText, operatorScores = {} }) {
  const g1 = manifest?.scenarioData?.g1 ?? null;
  const base = transcriptFacts(selectSegment(parseTranscript(baselineText), 'last'));
  const runFacts = transcriptText ? transcriptFacts(selectSegment(parseTranscript(transcriptText), 'last')) : null;
  const sections = paneText ? parseSections(paneText) : [];
  const busyIndicator = g1?.params?.busyIndicator ?? 'esc to interrupt';
  const commands = manifest?.commands ?? [];
  const rows = G1_CRITERIA.map((text, i) => ({ n: i + 1, text, boxC: null, run: null, checks: [], score: SCORES.NE, reason: null, operator: null }));
  const [c1, c2, c3, c4, c5] = rows;

  // Box C summaries (from its own transcript, last instance; C2/C3/C5 from G1-result.md).
  c1.boxC = `initialize protocolVersion ${base.initialize?.protocolVersion}, negotiated ${base.negotiatedProtocolVersion}, claude/channel declared: ${base.serverDeclaresChannel}; server/discover probe first: ${base.discoverProbe}`;
  c2.boxC = `wake ${base.channelNotifications.find((n) => n.kind === 'wake-test')?.id ?? '?'}; operator-queried attributes ${Object.entries(BOX_C_WAKE_ATTRIBUTES).map(([k, v]) => `${k}=${v}`).join(', ')}; non-identifier-safe key dropped (G1-result.md)`;
  c3.boxC = 'two mid-turn notifications ~1.85 s apart during the first of four Start-Sleep calls; delivered in order at two separate tool-call boundaries (G1-result.md)';
  c4.boxC = base.replyCalls.map((c) => `reply "${c.message}" in_reply_to ${c.inReplyTo}, result ok: ${c.resultOk}`).join('; ') || 'none';
  c5.boxC = 'dialog text captured verbatim before the operator (human) accepted it (G1-result.md)';

  const outcome = manifest?.outcome ?? 'missing';
  if (outcome !== 'PASS' || !g1 || !runFacts) {
    const why = outcome !== 'PASS' ? `run outcome ${outcome}${manifest?.outcomeReason ? `: ${manifest.outcomeReason}` : ''}` : !g1 ? 'no G1 scenario record in the run manifest' : 'no wire transcript captured';
    for (const r of rows) {
      r.run = 'not run to completion';
      r.reason = why;
    }
    return { rows, base, runFacts, sections };
  }

  // --- criterion 1: wire only -----------------------------------------------------------
  c1.run = `initialize protocolVersion ${runFacts.initialize?.protocolVersion}, negotiated ${runFacts.negotiatedProtocolVersion}, claude/channel declared: ${runFacts.serverDeclaresChannel}; server/discover probe first: ${runFacts.discoverProbe}`;
  c1.checks.push(
    check('initialize from the real client on the wire', !!runFacts.initialize, runFacts.initialize ? `clientInfo ${runFacts.initialize.clientInfo.name} ${runFacts.initialize.clientInfo.version}` : null),
    check('server declares capabilities.experimental["claude/channel"]', runFacts.serverDeclaresChannel),
    check('negotiated revision is legacy (2025-11-25 or earlier)', isLegacyRevision(runFacts.negotiatedProtocolVersion), runFacts.negotiatedProtocolVersion),
    check('notifications/initialized received', runFacts.initializedNotification),
  );
  if (!runFacts.initialize) c1.reason = 'no initialize on the wire';
  else {
    c1.score = c1.checks.every((c) => c.ok) ? SCORES.EQ : SCORES.NEQ;
    c1.reason = runFacts.discoverProbe === base.discoverProbe ? 'same handshake shape as Box C' : `server/discover probe ${runFacts.discoverProbe ? 'present' : 'absent'} here, ${base.discoverProbe ? 'present' : 'absent'} in Box C (a version-level behavior note, not a criterion failure, per G1-result.md)`;
  }

  // --- criterion 2: wire preconditions + operator reading -----------------------------
  const wake = runFacts.channelNotifications.find((n) => n.kind === 'wake-test');
  const wakeT = wake ? Date.parse(wake.t) : NaN;
  const promptsBeforeWake = commands.filter((c) => c.role === 'operator-input' && c.argv.includes('prompt') && Date.parse(c.startedAt) < wakeT);
  const preWake = sections.find((s) => s.seq === g1.wake?.preReadSeq);
  const aqSection = sections.find((s) => s.seq === g1.attributeQuery?.answerReadSeq);
  c2.run = wake ? `wake ${wake.id} at ${wake.t}; meta keys ${JSON.stringify(wake.metaKeys)}` : 'no wake notification on the wire';
  c2.checks.push(
    check('wake notification sent by the channel server (content string, meta map)', wake && wake.contentIsString && wake.metaKeys.length > 0),
    check('wake carries the identifier-safe keys oac_message_id, oac_sender and one non-identifier-safe key', wake && ['oac_message_id', 'oac_sender'].every((k) => wake.identifierSafeKeys.includes(k)) && wake.droppedKeys.length === 1, wake ? `unsafe: ${JSON.stringify(wake.droppedKeys)}` : null),
    check('no operator prompt was sent before the wake', wake && promptsBeforeWake.length === 0, `${promptsBeforeWake.length} prompt(s)`),
    check('pane read just before the wake shows no work in progress and no dialog', preWake && g1.wake?.preReadBusy === false && !g1.wake?.preReadDialog, preWake ? `read #${preWake.seq}` : 'read not in the pane capture'),
    check('attribute query asked and its answer captured verbatim', !!aqSection, aqSection ? `read #${aqSection.seq}` : null),
  );
  c2.checks.push(check('(supporting only) the screen changed after the wake with no operator input', !!g1.wake?.turnStartSeen, g1.wake?.turnStartSeen ? `read #${g1.wake.turnStartSeen.seq}` : 'not seen (a fast turn can start and end between reads)'));
  const expectedValues = [wake?.id, 'g1spike', 'g1-spike-operator'].filter(Boolean);
  c2.checks.push(check('(supporting only) the answer read contains the expected attribute values', aqSection && expectedValues.every((v) => aqSection.text.includes(v)), 'pane text also holds earlier scrollback; an operator must read the answer itself'));
  scoreWithOperator(c2, 2, operatorScores, c2.checks.filter((c) => !c.name.startsWith('(supporting')).every((c) => c.ok));

  // --- criterion 3: mid-turn timing from wire vs pane, then operator reading ----------------
  const mids = runFacts.channelNotifications.filter((n) => n.kind === 'midturn-test');
  const inputs = commands.filter((c) => INPUT_ROLES.has(c.role)).map((c) => ({ seq: c.seq, role: c.role, startedAt: c.startedAt }));
  const windows = mids.map((n) => ({ id: n.id, t: n.t, window: midTurnWindow({ notificationT: n.t, sections, promptSubmittedAt: g1.busyTurn?.prompt?.endedAt, inputCommands: inputs, busyIndicator }) }));
  const seqOf = (id) => Number(/-(\d+)$/.exec(id ?? '')?.[1]);
  c3.run = mids.length ? mids.map((n) => `${n.id} at ${n.t}`).join('; ') + (mids.length === 2 ? `; gap ${Date.parse(mids[1].t) - Date.parse(mids[0].t)} ms` : '') : 'no mid-turn notifications on the wire';
  c3.checks.push(
    check('exactly two mid-turn notifications on the wire', mids.length === 2, `${mids.length}`),
    check('sent in id order', mids.length === 2 && seqOf(mids[0].id) < seqOf(mids[1].id) && Date.parse(mids[0].t) < Date.parse(mids[1].t)),
    ...windows.map((w) =>
      check(
        `${w.id} sent mid-turn (wire t against timestamped pane reads, not herdr state)`,
        w.window.established,
        w.window.established ? `reads #${w.window.lastBeforeSeq} and #${w.window.firstAfterSeq} bracket it; unobserved window ${w.window.uncoveredWindowMs} ms` : w.window.reasons.join('; '),
      ),
    ),
    check('pane text after the busy turn captured verbatim', sections.some((s) => s.seq === g1.busyTurn?.afterReadSeq)),
  );
  scoreWithOperator(c3, 3, operatorScores, c3.checks.every((c) => c.ok));

  // --- criterion 4: wire only -----------------------------------------------------------
  const sentIds = new Set(runFacts.channelNotifications.map((n) => n.id));
  const replies = runFacts.replyCalls;
  c4.run = replies.map((c) => `reply "${c.message}" in_reply_to ${c.inReplyTo}, result ok: ${c.resultOk}`).join('; ') || 'no reply tool call';
  const good = replies.find((c) => c.resultOk && sentIds.has(c.inReplyTo));
  c4.checks.push(check('`reply` tools/call from Claude Code, answered without error', replies.some((c) => c.resultOk)), check('in_reply_to echoes a channel message id the server sent', !!good));
  if (!replies.length) c4.reason = 'no reply tool call on the wire';
  else {
    c4.score = good ? SCORES.EQ : SCORES.NEQ;
    c4.reason = good ? 'ordinary MCP tool reply, as Box C' : 'reply present but errored or did not echo a sent message id';
  }

  // --- criterion 5: dialog text before any keystroke; accept origin ---------------------------
  const dialog = (g1.dialogs ?? []).find((d) => d.kind === 'dev-channels');
  const agentStartSeq = g1.agentStart?.seq ?? 0;
  if (!dialog) {
    c5.run = 'the dev-channels dialog was not recognized on screen';
    c5.checks.push(check('dev-channels dialog seen', false));
    c5.score = SCORES.NEQ;
    c5.reason = 'no dev-channels dialog on record; check the pane capture for an unrecognized dialog before concluding it was skipped';
  } else {
    const sec = sections.find((s) => s.seq === dialog.readSeq);
    const inputBefore = commands.filter((c) => c.seq > agentStartSeq && c.seq < dialog.readSeq && INPUT_ROLES.has(c.role));
    const match = sec ? dialogMatchesBoxC(sec.text) : { matches: false, missing: ['(dialog read not in the pane capture)'] };
    const acceptCmd = commands.find((c) => c.seq === dialog.acceptSeq);
    // Defense in depth: a driver accept policy, or ANY dialog-accept command in the run,
    // counts as driver-sent even if the dialog record itself were wrong.
    const anyDriverAccept = commands.some((c) => c.role === 'dialog-accept');
    const driverSent = dialog.acceptOrigin === 'driver' || acceptCmd?.role === 'dialog-accept' || g1.acceptPolicy !== 'human' || anyDriverAccept;
    c5.run = `dialog read at herdr command #${dialog.readSeq}; accepted by ${driverSent ? `the DRIVER or under a driver accept policy (policy ${g1.acceptPolicy}; dialog-accept commands: ${commands.filter((c) => c.role === 'dialog-accept').map((c) => `#${c.seq}`).join(', ') || 'none'})` : dialog.acceptOrigin}`;
    c5.checks.push(
      check('dialog text read from the pane and kept verbatim', !!sec, sec ? `read #${sec.seq}` : null),
      check('no keystroke reached the pane between launch and that read', inputBefore.length === 0, inputBefore.length ? `commands #${inputBefore.map((c) => c.seq).join(', #')}` : null),
      check('dialog text matches Box C', match.matches, match.missing.length ? `missing: ${match.missing.join(' | ')}` : null),
      check('accepted by a human (the driver sent no keystroke to it)', !driverSent && dialog.acceptOrigin === 'human' && dialog.inputBetweenReadAndAccept === 0),
    );
    if (driverSent) {
      c5.score = SCORES.NE;
      c5.reason = 'driver-sent accept: a driver-sent accept is never scored as meeting criterion 5 (scripted-runs.md "Operator-consent dialogs"; kept by the #196 decision)';
    } else if (!sec || inputBefore.length) {
      c5.score = SCORES.NE;
      c5.reason = 'dialog text is not on record from before any keystroke';
    } else if (dialog.acceptOrigin !== 'human') {
      c5.score = SCORES.NE;
      c5.reason = `accept origin not established (${dialog.acceptOrigin})`;
    } else {
      c5.score = match.matches ? SCORES.EQ : SCORES.NEQ;
      c5.reason = match.matches ? 'dialog text read before any keystroke matches Box C; accepted by a human' : 'dialog text differs from Box C (see the check detail)';
    }
  }
  return { rows, base, runFacts, sections, windows };
}

function scoreWithOperator(row, n, operatorScores, preconditionsOk) {
  const op = operatorScores[n];
  if (!preconditionsOk) {
    row.score = SCORES.NE;
    row.reason = `mechanical preconditions not met (${row.checks.filter((c) => !c.ok && !c.name.startsWith('(supporting')).map((c) => c.name).join('; ')})`;
    if (op) row.reason += '; the operator score was not applied';
    return;
  }
  if (!op) {
    row.score = SCORES.NE;
    row.reason = 'operator review of the verbatim pane text pending (--score/--note)';
    return;
  }
  row.score = op.score;
  row.operator = op;
  row.reason = `operator: ${op.note}`;
}

const cell = (s) => String(s ?? '').replace(/\|/g, '\\|').replace(/\n/g, ' ');

export function renderReport({ manifest, evaluation, diffText, date, fixtures, runManifestName, baselinePath = BOX_C_TRANSCRIPT }) {
  const g1 = manifest?.scenarioData?.g1 ?? {};
  const v = g1.versions ?? {};
  const out = [];
  out.push(`# G1 scripted re-run through herdr, ${date}: comparison with Box C`);
  out.push('');
  out.push('> **Not verdict-bearing.** Proof of concept (Epic K, K4 #127). This record compares a');
  out.push('> herdr-driven run of G1 against the human-run Box C. G1\'s verdict');
  out.push('> (`docs/planning/gates/G1-result.md`) and `docs/planning/STATUS.md` are unchanged by it.');
  out.push('> A driver-sent accept of the dev-channels dialog is never scored as meeting criterion 5.');
  out.push('');
  out.push(`- **Driver:** herdr (\`${manifest?.herdr?.observedVersionOutput ?? '?'}\`, PINS.md \`herdr (test tooling)\` ${manifest?.herdr?.pinnedTag ?? '?'}) via \`tools/herdr/run.mjs\`, scenario \`${manifest?.scenario?.file ?? '?'}\`, driver commit \`${manifest?.driver?.commit ?? '?'}\`${manifest?.driver?.toolsHerdrDirty ? ' (tools/herdr had uncommitted changes)' : ''}`);
  out.push(`- **Run outcome:** ${manifest?.outcome ?? '?'}${manifest?.outcomeReason ? ` — ${manifest.outcomeReason}` : ''}`);
  out.push(`- **Claude Code version:** \`claude --version\` = \`${v.cliOutput ?? '?'}\` (post-run \`${g1.postRunVersion ?? 'not recorded'}\`); wire \`clientInfo.version\` = \`${v.wireClientInfo ?? '?'}\`; transport user-agent N/A (stdio); PINS.md \`${v.pinsRow ?? 'Claude Code (Channels)'}\` ${pinsVersionsText(v)}; version warnings: ${v.warnings?.length ?? 0} (listed under Findings; versions float and are never gated, #216)`);
  out.push(`- **Launch (verbatim):** \`${(manifest?.launch?.argv ?? []).join(' ')}\`; herdr-reported argv \`${JSON.stringify(manifest?.launch?.herdrReportedArgv ?? null)}\``);
  out.push(`- **Timebox:** ${manifest?.timebox?.budgetMs ?? '?'} ms, ${manifest?.timebox?.start ?? '?'} to ${manifest?.timebox?.end ?? '?'}; expired: ${manifest?.timebox?.expired ?? '?'}`);
  out.push(`- **Accept policy:** ${g1.acceptPolicy ?? '?'}; dialogs on record: ${describeDialogs(g1.dialogs)}`);
  out.push(`- **Channel server:** \`${g1.server?.committed ?? '?'}\` run from a scratch copy of the blob at HEAD \`${g1.server?.headCommit ?? '?'}\`; sha256 committed \`${g1.server?.committedSha256 ?? '?'}\`, copy \`${g1.server?.copySha256 ?? '?'}\`, match: ${g1.server?.match ?? '?'}; working tree matched HEAD: ${g1.server?.workingTreeMatchesHead ?? '?'}`);
  out.push(fixtures ? `- **Fixtures:** \`${fixtures.transcript}\`, \`${fixtures.pane}\`` : `- **Fixtures:** none (${writeRefusal(manifest) ?? fixtureWithheld(manifest) ?? 'not published'})`);
  out.push(`- **Run manifest:** \`${runManifestName}\` (beside this file); harness config unchanged: ${manifest?.harnessConfig?.unchanged ?? '?'}; teardown clean: ${manifest?.teardown?.clean ?? '?'}`);
  out.push(`- **Baseline:** \`${baselinePath}\` (last server instance, the verdict-bearing lines)`);
  out.push('');
  out.push('## Criteria');
  out.push('');
  out.push('| # | Criterion (verbatim, `oac-gates` G1 reference) | Box C | This run | Score |');
  out.push('|---|---|---|---|---|');
  for (const r of evaluation.rows) out.push(`| ${r.n} | ${cell(r.text)} | ${cell(r.boxC)} | ${cell(r.run)} | **${r.score}** |`);
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
  out.push('From `tools/herdr/lib/compare-transcripts.mjs` (Box C = baseline, this run = candidate):');
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
  out.push('- Operator prompts are scenario parameters; Box C\'s own prompt texts are not on record, so wording differs from Box C by construction.');
  out.push('- Pane-text patterns other than the dev-channels dialog (in-progress indicator, other dialogs) were written before any live run; confirm them against this run\'s pane capture.');
  out.push('');
  // #196, #252: criterion 5 is a consent step, so its accept is the one human action a G1
  // record needs signed: the person who pressed the key names themself. A driver accept is
  // stated as the driver's, which leaves criterion 5 not evaluable and the record not an
  // equivalence record.
  const dev = (g1.dialogs ?? []).find((d) => d.kind === 'dev-channels');
  const devDriver = !dev || dev.acceptOrigin !== 'human' || g1.acceptPolicy !== 'human' || (manifest?.commands ?? []).some((c) => c.role === 'dialog-accept');
  const humanActions = !dev
    ? 'none: no dev-channels dialog on record, so criterion 5 is not evaluable and this record is neither an equivalence record nor verdict-bearing.'
    : devDriver
      ? `none for criterion 5: ${describeDialog(dev)}; accept policy ${g1.acceptPolicy ?? '?'}; dialog-accept commands in the run: ${(manifest?.commands ?? []).filter((c) => c.role === 'dialog-accept').map((c) => `#${c.seq}`).join(', ') || 'none'}. Criterion 5 is not evaluable, so this record is neither an equivalence record nor verdict-bearing.`
      : `the dev-channels dialog accept, G1 criterion 5's consent step (read #${dev.readSeq}; the driver sent no keystroke, so the record cannot show who pressed the key): accepted at the keyboard by ${TO_FILL}: the person who accepted it>.`;
  out.push(...verification({
    manifest,
    harness: harnessVerification(manifest, { verified: versionsVerified(g1), versions: `Claude Code: CLI \`${v.cliOutput ?? '?'}\` (\`scenarioData.g1.versions.cliOutput\`, parsed \`${v.cli ?? '?'}\` in \`scenarioData.g1.versions.cli\`), wire \`clientInfo.version\` \`${v.wireClientInfo ?? '?'}\` (\`scenarioData.g1.versions.wireClientInfo\`), post-run \`${g1.postRunVersion ?? 'not recorded'}\` (\`scenarioData.g1.postRunVersion\`)` }),
    dialogs: g1.dialogs,
    dialogsField: 'scenarioData.g1.dialogs',
    humanActions,
  }));
  return out.join('\n');
}

// PINS.md's versions as the run recorded them. A run recorded before #216 carries only the
// then "last observed" version.
function pinsVersionsText(v) {
  if (v.pinsLastTested) return `minimum \`${v.pinsMinimum ?? '?'}\`, last tested \`${v.pinsLastTested}\``;
  return `last observed \`${v.pinsLastObserved ?? '?'}\` (recorded before #216)`;
}

// A run's Claude Code version is verified when the CLI and the wire clientInfo report one
// and the same version and the scenario marked it verified, so a fixture names one
// version. Whether that version is PINS.md's last tested one is NOT part of this: versions
// float and are never gated (#216); a difference is a VERSION WARNING finding and shows as
// version_matches_pin: false in the fixture manifest entry.
export function versionsVerified(g1) {
  const v = g1?.versions;
  return !!v && v.verified === true && !!v.cli && v.cli === v.wireClientInfo;
}

// Whether the verified version equals PINS.md's last tested version (informational only).
export function versionMatchesLastTested(g1) {
  const v = g1?.versions;
  return versionsVerified(g1) && !!v.pinsLastTested && v.cli === v.pinsLastTested;
}

// Why --write writes the record and run manifest but NO fixture, or null. Operator decision
// on #216 (2026-10-01): when the CLI and the wire disagree, or the version moved mid-run,
// the record is still written, with a VERSION WARNING; its captures stay `unverified-*`
// and no fixture or MANIFEST.json entry is produced, because a fixture names one version.
export function fixtureWithheld(manifest) {
  const g1 = manifest?.scenarioData?.g1;
  if (!g1 || versionsVerified(g1)) return null;
  return `VERSION WARNING: the CLI (${g1.versions?.cli ?? 'none'}) and the wire (${g1.versions?.wireClientInfo ?? 'none'}) did not report one and the same Claude Code version, so no fixture can name it; the record and run manifest are written, the captures stay unverified-* and no fixture is added (#216)`;
}

// Why --write must refuse this run, or null. Only a PASS run with an untouched committed
// server is written; when its versions are verified, the K4 fixture names and both captures
// written clean are required too. Mixed versions never refuse the write (fixtureWithheld).
export function writeRefusal(manifest) {
  const g1 = manifest?.scenarioData?.g1;
  if (manifest?.outcome !== 'PASS') return `run outcome is ${manifest?.outcome ?? 'missing'}${manifest?.outcomeReason ? ` (${manifest.outcomeReason})` : ''}; only a PASS run is written`;
  if (!g1) return 'no G1 scenario record in the run manifest';
  if (!versionsVerified(g1)) return g1.server?.match !== true || g1.server?.workingTreeMatchesHead !== true ? 'the channel server copy is not verified against the committed blob' : null;
  if (!g1.fixtures || JSON.stringify(g1.fixtures) !== JSON.stringify(g1.captureNames)) return 'captures do not carry the verified K4 fixture names';
  for (const f of [g1.fixtures.transcript, g1.fixtures.pane]) {
    if (!manifest.captures?.some((c) => c.file === f && c.written)) return `capture ${f} was not written (withheld or missing)`;
  }
  if (g1.server?.match !== true || g1.server?.workingTreeMatchesHead !== true) return 'the channel server copy is not verified against the committed blob';
  return null;
}

// Draft MANIFEST.json entries for the two captures, with the K5 `driver` block.
export function draftManifestEntries({ manifest, fixtures, runManifestPath, transcriptText, pinsCommit, redactSha256 }) {
  const g1 = manifest.scenarioData.g1;
  const facts = transcriptFacts(parseTranscript(transcriptText));
  const cap = (name) => manifest.captures.find((c) => c.file === name)?.redaction ?? null;
  const residual = (name) => {
    const r = cap(name);
    return r ? `tools/herdr/lib/redact.mjs (run.mjs): droppedHazardLines=${r.droppedHazardLines}, hazardProtocolFrames=${r.hazardProtocolFrames.length}, residualLeaks=${r.residualLeaks.length}, residualGenericHits=${r.residualGenericHits.length}` : 'not recorded';
  };
  const common = {
    provider: 'claude',
    surface: 'claude-channels',
    observed_version: { claude_code: `${g1.versions.cli} (claude --version \`${g1.versions.cliOutput}\`; wire clientInfo.version ${g1.versions.wireClientInfo})`, mcp_protocol: `${facts.negotiatedProtocolVersion} (negotiated)`, node: null },
    pins_row: 'Claude Code (Channels)',
    pins_as_of: `PINS.md as committed at HEAD ${g1.versions.pinsSource?.headCommit ?? '?'} when the run started (working tree matched HEAD: ${g1.versions.pinsSource?.workingTreeMatchesHead}); last commit touching PINS.md at report time: ${pinsCommit ?? 'unknown'}`,
    version_matches_pin: versionMatchesLastTested(g1),
    ...(versionMatchesLastTested(g1) ? {} : { version_matches_pin_note: `Claude Code ${g1.versions.cli} is not PINS.md's last tested ${g1.versions.pinsLastTested ?? g1.versions.pinsLastObserved ?? '?'} (minimum ${g1.versions.pinsMinimum ?? '?'}); recorded as a VERSION WARNING finding, not a gate (#216)` }),
    capture_date: g1.date,
    capture_utc_range: facts.firstT && facts.lastT ? `${facts.firstT}-${facts.lastT}` : null,
    superseded_by: null,
    driver: { herdr_version: manifest.herdr.observedVersionOutput, driver_commit: manifest.driver.commit, run_manifest: runManifestPath },
    notes: ['K4 scripted run through herdr; NOT verdict-bearing (G1 verdict unchanged). Compared against Box C in the herdr-runs record beside the run manifest.'],
  };
  const redaction = (name) => ({ script: 'tools/herdr/lib/redact.mjs', script_sha256: redactSha256, residual_scan_result: residual(name) });
  return [
    { path: `${FIXTURE_DIR}/${fixtures.transcript}`, ...common, redaction: redaction(fixtures.transcript), coverage: { 'channel notifications': facts.channelNotifications.map((n) => `${n.id} line ${n.line}`).join('; '), 'reply tool calls': facts.replyCalls.map((c) => `lines ${c.line}-${c.resultLine}`).join('; ') } },
    { path: `${FIXTURE_DIR}/${fixtures.pane}`, ...common, redaction: redaction(fixtures.pane), coverage: { 'pane reads': 'verbatim herdr agent reads, one section per kept read, each with its herdr command seq and timestamps' } },
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
  const g1 = manifest.scenarioData?.g1;
  // Printing reads whatever was captured (fixture-named or `unverified-*`); --write below
  // refuses anything but a verified PASS run.
  const names = g1?.captureNames ?? g1?.fixtures ?? null;
  const fixtures = g1?.fixtures ?? null;
  const written = (name) => name && manifest.captures?.some((c) => c.file === name && c.written) && existsSync(join(runDir, name));
  const transcriptText = names && written(names.transcript) ? readFileSync(join(runDir, names.transcript), 'utf8') : null;
  const paneText = names && written(names.pane) ? readFileSync(join(runDir, names.pane), 'utf8') : null;
  const baselinePath = o.baseline ?? BOX_C_TRANSCRIPT;
  const baselineText = readFileSync(resolve(REPO, baselinePath), 'utf8');
  const evaluation = evaluateG1({ manifest, transcriptText, paneText, baselineText, operatorScores });
  const diffText = transcriptText ? formatDiff(compareTranscripts(baselineText, transcriptText, { segment: 'last', detail: true }), { baselineLabel: 'Box C', candidateLabel: 'herdr run' }) : null;
  const date = g1?.date ?? manifest.timebox?.start?.slice(0, 10) ?? 'unknown-date';
  const runManifestName = `G1-${date}.run-manifest.json`;
  const refusal = writeRefusal(manifest);
  const publish = !refusal && !fixtureWithheld(manifest);
  const report = renderReport({ manifest, evaluation, diffText, date, fixtures: publish ? { transcript: `${FIXTURE_DIR}/${fixtures.transcript}`, pane: `${FIXTURE_DIR}/${fixtures.pane}` } : null, runManifestName, baselinePath });
  if (!o.write) {
    console.log(report);
    return 0;
  }
  if (refusal) throw new ReportError(`--write refused: ${refusal}. Nothing was written; print the draft without --write to inspect the run`);
  const root = o.root ? resolve(o.root) : REPO;
  const runsDir = join(root, HERDR_RUNS_DIR);
  const targets = [
    [join(runsDir, `G1-${date}.md`), null],
    [join(runsDir, runManifestName), join(runDir, 'run-manifest.json')],
    ...(publish && transcriptText ? [[join(root, FIXTURE_DIR, fixtures.transcript), join(runDir, fixtures.transcript)]] : []),
    ...(publish && paneText ? [[join(root, FIXTURE_DIR, fixtures.pane), join(runDir, fixtures.pane)]] : []),
  ];
  const exists = targets.filter(([t]) => existsSync(t)).map(([t]) => t);
  if (exists.length) throw new ReportError(`refusing to overwrite: ${exists.join(', ')}`);
  for (const [t] of targets) mkdirSync(dirname(t), { recursive: true });
  writeFileSync(targets[0][0], `${report}\n`);
  for (const [to, from] of targets.slice(1)) copyFileSync(from, to);
  if (publish && transcriptText) {
    const git = spawnSync('git', ['log', '-1', '--format=%H', '--', 'docs/planning/PINS.md'], { cwd: REPO, encoding: 'utf8', timeout: 10000 });
    const entries = draftManifestEntries({
      manifest,
      fixtures,
      runManifestPath: `${HERDR_RUNS_DIR}/${runManifestName}`,
      transcriptText,
      pinsCommit: git.status === 0 ? git.stdout.trim() : null,
      redactSha256: createHash('sha256').update(readFileSync(join(HERE, 'redact.mjs'))).digest('hex'),
    });
    writeFileSync(join(runDir, 'manifest-entries.draft.json'), `${JSON.stringify(entries, null, 2)}\n`);
  }
  console.log(`wrote ${targets.map(([t]) => t).join('\n      ')}`);
  if (!publish) console.log(`No fixture written: ${fixtureWithheld(manifest)}`);
  console.log('Next: review the draft, merge <run dir>/manifest-entries.draft.json into docs/planning/gates/fixtures/MANIFEST.json, run node scripts/check-fixture-manifest.mjs, and add only a pointer to G1-result.md.');
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (err) {
    console.error(`g1-report: ${err.message}`);
    process.exitCode = 2;
  }
}
