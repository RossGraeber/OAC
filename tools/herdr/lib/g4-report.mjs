#!/usr/bin/env node
// Draft the criterion-by-criterion comparison of a scripted G4 run against the human-run
// 2026-09-26 re-run (Epic K, K8 #131): `docs/planning/gates/herdr-runs/G4-<YYYY-MM-DD>.md`.
//
//   node tools/herdr/lib/g4-report.mjs --run <run dir>
//        [--score 1=equivalent|not-equivalent --note 1='<why, citing pane lines>']
//        [--score 5=... --note 5=...] [--write] [--root <dir>] [--baseline <fixture>]
//
// <run dir> is the --out directory of `node tools/herdr/run.mjs --scenario g4-mcp-dual-era`.
// Without --write the draft is printed. With --write it lands in the repository: the report
// and the run manifest beside it (docs/planning/gates/herdr-runs/G4-<date>.md and
// .run-manifest.json), the three captures under docs/planning/gates/fixtures/g4-mcp-dual-era/,
// and draft MANIFEST.json entries (with `schema` and `driver` blocks) in
// <run dir>/manifest-entries.draft.json for the operator to review and merge. Existing files
// are never overwritten. --write refuses anything but a PASS run with verified harness
// versions, from a clean, committed tools/herdr/ (like K7's generator, stricter than G1's).
//
// NOT VERDICT-BEARING, and a RECONSTRUCTED SERVER: the record says both at its top. It never
// changes G4's verdict, STATUS.md or PINS.md. Its Verification section (#252) is generated
// from the run manifest for the recording agent to re-check.
//
// Scoring, against docs/planning/gates/fixtures/g4-mcp-dual-era/transcript-2026-09-26.jsonl
// and G4-result.md. The five criteria are read verbatim from the oac-gates reference AS
// COMMITTED at HEAD and bound by G4_CRITERIA_SHA256 (lib/g4.mjs); a reworded or reordered
// reference is refused before anything is scored.
//   - 2, 3, 4: mechanical, on the server's own wire transcript (every copy writes to it).
//   - 1 and 5: need a human reading of the Claude pane (did the legacy pushes arrive; was
//     g4modern refused as a channel and did its pushes not arrive). Their wire preconditions
//     are checked here; the score stays `not evaluable` until the operator gives --score/--note.
//   - herdr agent state never scores anything. Any run outcome other than PASS makes every
//     criterion `not evaluable`.

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  BASELINE_TRANSCRIPT, FIXTURE_DIR, HERDR_RUNS_DIR, MANIFEST_PATH, MODERN, OAC_EXT, OAC_EXT_PLACEHOLDER, G4_CRITERIA_SHA256, G4_SERVER_FILES,
  assertG4Criteria, g4Facts, modernRequests, parseG4Transcript, readG4Criteria, roles, codexSessions, placeholderIntegrity,
} from './g4.mjs';
import { parseSections, committedFile } from './g1.mjs';
import { isLegacyRevision } from './compare-transcripts.mjs';
import {
  SCORES, ReportError, check, cell, mechanicalRow, operatorRow, parseOperatorScores, parseReportArgs, writeTargets, verification, harnessVerification, reconstructionCallout, describeDialogs, noConsentCriterionLine, lineSpan, redactScriptSha256,
} from './gate-report-common.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');

export { SCORES, ReportError };
export const OPERATOR_ROWS = Object.freeze(['1', '5']);
export const parseG4OperatorScores = (pairs) => parseOperatorScores(pairs, OPERATOR_ROWS, 'is scored mechanically from the server\'s wire transcript');

const INJECTED = /notifications\/claude\/channel|G4 wake test|relayed from http|oac_message_id/i;

// The wire evidence of one transcript for each criterion. Used on the baseline and on the run
// alike, so "equivalent" means the run shows what the human run's fixture shows.
export function wireEvidence(f) {
  const r = roles(f);
  const legacy = r.legacyPid;
  const modernCopy = r.modernPid;
  const init = f.stdioInitialize.find((x) => x.pid === legacy);
  const legacyPushes = f.pushes.filter((p) => p.pid === legacy);
  const wakes = legacyPushes.filter((p) => p.meta.g4_stdio_era === 'legacy');
  const relays = legacyPushes.filter((p) => p.meta.relay_from);
  const modernPushes = f.pushes.filter((p) => p.pid === modernCopy);
  const claudeModern = f.toolCalls.filter((c) => c.pid === legacy && c.era === 'modern' && c.surface === 'http');
  const codexCalls = f.toolCalls.filter((c) => c.pid === legacy && c.codexVersion);
  const codexInits = f.httpInitialize.filter((x) => x.pid === legacy && x.codexVersion);
  const mreq = modernRequests(f);
  const disc = f.stdioDiscover.find((x) => x.pid === modernCopy && !x.error);
  const provOk = (c) => c.provenance && c.provenance.served_by_pid === legacy && c.provenance.surface === 'http-modern' && c.resultType === 'complete' && !c.error && /^g4 echo: /.test(c.resultText);
  const firstCodexLine = Math.min(...codexInits.map((x) => x.reqLine), ...codexCalls.map((c) => c.reqLine));
  const lastCodexLine = Math.max(...codexCalls.map((c) => c.resLine ?? c.reqLine), ...relays.map((p) => p.line));
  const anyCodex = codexInits.length + codexCalls.length > 0;
  const before = anyCodex ? claudeModern.filter((c) => c.reqLine < firstCodexLine) : [];
  const after = anyCodex ? claudeModern.filter((c) => c.reqLine > lastCodexLine) : [];
  return {
    roles: r,
    c1: { init, legacyNegotiated: !!init && isLegacyRevision(init.negotiated) && init.channelCap, wakes, relays },
    c2: { claudeModern, allProvenance: claudeModern.length >= 1 && claudeModern.every(provOk), codexEras: [...new Set(codexInits.map((x) => x.negotiated))] },
    c3: { modernRequests: mreq, missing: mreq.filter((x) => x.carriesPV !== MODERN) },
    c4: {
      legacyStarts: f.instances.filter((i) => i.pid === legacy).length,
      before,
      after,
      codexInits,
      codexCalls,
      codexOk: codexCalls.length >= 2 && codexCalls.every((c) => !c.error && c.provenance),
      relayOnLegacyStdio: relays.length >= 1,
      wakeAfterCodex: anyCodex && wakes.some((w) => w.line > lastCodexLine),
      errorsOnLegacy: f.errors.filter((e) => e.pid === legacy && e.line > (init?.resLine ?? 0)),
      span: init && wakes.length ? { from: init.reqLine, to: wakes.at(-1).line } : null,
    },
    c5: { discover: disc, modernOnly: !!disc && JSON.stringify(disc.supportedVersions) === JSON.stringify([MODERN]) && disc.channelCap, modernPushes },
  };
}

export function evaluateG4({ manifest, transcriptText, paneClaudeText, baselineText, criteria, operatorScores = {} }) {
  assertG4Criteria(criteria);
  const g4 = manifest?.scenarioData?.g4 ?? null;
  const base = g4Facts(parseG4Transcript(baselineText));
  const be = wireEvidence(base);
  const run = transcriptText ? g4Facts(parseG4Transcript(transcriptText)) : null;
  const sections = paneClaudeText ? parseSections(paneClaudeText) : [];
  const commands = manifest?.commands ?? [];
  const rows = criteria.map((text, i) => ({ n: String(i + 1), text, baseline: null, run: null, checks: [], score: SCORES.NE, reason: null, operator: null }));
  const [c1, c2, c3, c4, c5] = rows;

  c1.baseline = `g4spike (pid ${be.roles.legacyPid}) stdio initialize negotiated ${be.c1.init?.negotiated} with claude/channel (lines ${be.c1.init?.reqLine}-${be.c1.init?.resLine}); legacy pushes ${be.c1.wakes.map((w) => `${w.meta.oac_message_id} line ${w.line}`).join(', ')} and relay ${be.c1.relays.map((w) => `${w.meta.oac_message_id} line ${w.line}`).join(', ')}; arrival confirmed in the operator's Claude UI paste (G4-result.md UI observations 2, 5, 7): x`;
  c2.baseline = `Claude HTTP-modern tools/call lines ${be.c2.claudeModern.map((c) => `${c.reqLine}-${c.resLine}`).join(', ')}, resultType complete, OAC _meta provenance (served_by_pid ${be.roles.legacyPid}); Codex stayed on ${be.c2.codexEras.join('/')} (G4-result.md caveat): x`;
  c3.baseline = `${be.c3.modernRequests.length} modern-path requests (lines ${be.c3.modernRequests.map((x) => x.line).join(', ')}), every one carrying _meta protocolVersion ${MODERN}: x`;
  c4.baseline = `one pid (${be.roles.legacyPid}) held the legacy stdio channel from line ${be.c4.span?.from} to line ${be.c4.span?.to} while serving Claude modern calls before (lines ${be.c4.before.map((c) => c.reqLine).join(', ')}) and after (lines ${be.c4.after.map((c) => c.reqLine).join(', ')}) Codex's legacy traffic (initialize lines ${be.c4.codexInits.map((x) => x.reqLine).join(', ')}; calls ${be.c4.codexCalls.map((c) => c.reqLine).join(', ')}): x`;
  c5.baseline = `g4modern (pid ${be.roles.modernPid}) answered server/discover with supportedVersions ${JSON.stringify(be.c5.discover?.supportedVersions)} and claude/channel (line ${be.c5.discover?.resLine}); its pushes (lines ${be.c5.modernPushes.map((p) => p.line).join(', ')}) never arrived; UI: "Channel messages from \\"g4modern\\" are unavailable" (G4-result.md UI observation 1): x`;

  const outcome = manifest?.outcome ?? 'missing';
  if (outcome !== 'PASS' || !g4 || !run) {
    const why = outcome !== 'PASS' ? `run outcome ${outcome}${manifest?.outcomeReason ? `: ${manifest.outcomeReason}` : ''}` : !g4 ? 'no G4 scenario record in the run manifest' : 'no wire transcript captured';
    for (const r of rows) {
      r.run = 'not run to completion';
      r.reason = why;
    }
    return { rows, base, run, sections, be, re: null };
  }
  const re = wireEvidence(run);
  const typed = commands.filter((c) => ['operator-input', 'dialog-accept'].includes(c.role) && c.argv.some((a) => INJECTED.test(String(a))));
  const sec = (seq) => sections.find((s) => s.seq === seq);
  const wakeReads = (g4.wakes ?? []).map((w) => sec(w.afterReadSeq));
  const relayRead = sec(g4.codex?.claudeAfterRelayReadSeq);

  // --- 1 -------------------------------------------------------------------------------------
  c1.run = `g4spike pid ${re.roles.legacyPid}: initialize ${re.c1.init?.negotiated ?? '?'} (lines ${re.c1.init?.reqLine}-${re.c1.init?.resLine}); legacy wakes ${re.c1.wakes.map((w) => `${w.meta.oac_message_id} line ${w.line}`).join(', ') || 'none'}; relay ${re.c1.relays.map((w) => `${w.meta.oac_message_id} line ${w.line}`).join(', ') || 'none'}`;
  c1.checks.push(
    check('the legacy copy negotiated a legacy revision (2025-11-25 or earlier) with capabilities.experimental["claude/channel"]', re.c1.legacyNegotiated, re.c1.init ? `lines ${re.c1.init.reqLine}-${re.c1.init.resLine}` : 'no stdio initialize from the legacy copy'),
    check('the legacy copy pushed both wakes and the Codex-triggered relay on its stdio', re.c1.wakes.length >= 2 && re.c1.relays.length >= 1),
    check('the Claude pane was read verbatim after each wake and after the relay', wakeReads.length >= 2 && wakeReads.every(Boolean) && !!relayRead),
    check('never typed by herdr: no herdr input command carries a channel notification', typed.length === 0, typed.length ? `commands #${typed.map((c) => c.seq).join(', #')}` : null),
    check('(supporting only) the Claude pane reads mention each legacy push', [...re.c1.wakes, ...re.c1.relays].every((p) => [...wakeReads, relayRead].some((s) => s && (s.text.includes(p.meta.oac_message_id) || s.text.includes(String(p.content).slice(0, 30))))), 'a text match only; the operator reads the pane'),
  );
  operatorRow(c1, operatorScores['1'], 'operator review of the verbatim Claude pane pending: did every legacy push arrive as a channel message (--score 1=... --note 1=...)');

  // --- 2 -------------------------------------------------------------------------------------
  c2.run = `Claude modern tools/call ${re.c2.claudeModern.map((c) => `lines ${c.reqLine}-${c.resLine}${c.provenance ? ' with provenance' : ''}`).join(', ') || 'none'}; Codex negotiated ${re.c2.codexEras.join('/') || 'nothing'}`;
  c2.checks.push(
    check('Claude\'s modern tools/call on the legacy copy\'s HTTP surface answered resultType complete, "g4 echo: ...", with OAC _meta provenance (served_by_pid, surface http-modern), twice', re.c2.claudeModern.length >= 2 && re.c2.allProvenance),
    check(`the provenance key is the OAC extension identifier (captured as ${OAC_EXT_PLACEHOLDER}; the scenario's sanitizer replaced ${g4.sanitizer?.extensionIdReplaced ?? 0} occurrence(s) of the public identifier)`, (g4.sanitizer?.extensionIdReplaced ?? 0) > 0 && re.c2.claudeModern.every((c) => !!c.provenance)),
    check('the placeholder survived redaction intact in the transcript (as many as the sanitizer put in, no identifier fragment left)', placeholderIntegrity(transcriptText, g4.sanitizer?.extensionIdReplaced ?? 0).ok, JSON.stringify(placeholderIntegrity(transcriptText, g4.sanitizer?.extensionIdReplaced ?? 0))),
    check('(supporting only) Codex stayed on the legacy era, as in the human run (the caveat G4-result.md records)', re.c2.codexEras.length > 0 && re.c2.codexEras.every((v) => isLegacyRevision(v)), `Codex: ${re.c2.codexEras.join(', ') || 'no Codex initialize'}; with a feature-flag override this differs from the baseline by design`),
  );
  mechanicalRow(c2, 'the current-revision path served tools/call with OAC _meta provenance, as in the human run (Claude as the modern client, as there)');

  // --- 3 -------------------------------------------------------------------------------------
  c3.run = `${re.c3.modernRequests.length} modern-path requests; ${re.c3.missing.length} without the protocolVersion _meta${re.c3.missing.length ? ` (lines ${re.c3.missing.map((x) => x.line).join(', ')})` : ''}`;
  c3.checks.push(
    check('modern-path requests are on the wire (stdio probes, HTTP discover, tools/list, tools/call)', re.c3.modernRequests.length >= 5 && ['server/discover', 'tools/list', 'tools/call'].every((m) => re.c3.modernRequests.some((x) => x.method === m))),
    check(`every one carries _meta["io.modelcontextprotocol/protocolVersion"] = "${MODERN}"`, re.c3.missing.length === 0, re.c3.missing.map((x) => `line ${x.line} ${x.method}`).join('; ') || null),
  );
  mechanicalRow(c3, 'every modern-path request carries the protocol-version _meta, as in the human run');

  // --- 4 -------------------------------------------------------------------------------------
  c4.run = `legacy pid ${re.roles.legacyPid} started ${re.c4.legacyStarts} time(s); Claude modern calls before Codex ${re.c4.before.map((c) => c.reqLine).join(', ') || 'none'}, after ${re.c4.after.map((c) => c.reqLine).join(', ') || 'none'}; Codex initialize ${re.c4.codexInits.map((x) => x.reqLine).join(', ')}, calls ${re.c4.codexCalls.map((c) => c.reqLine).join(', ')}`;
  c4.checks.push(
    check('one legacy server process for the whole run (never restarted) holding the stdio channel', re.c4.legacyStarts === 1 && !!re.c4.span),
    check('Claude\'s modern tools/call succeeded both BEFORE and AFTER the Codex legacy traffic, on the same process', re.c4.before.length >= 1 && re.c4.after.length >= 1 && [...re.c4.before, ...re.c4.after].every((c) => c.provenance && !c.error)),
    check('Codex\'s legacy HTTP session initialized and its g4_echo and g4_relay_to_claude calls succeeded on that process', re.c4.codexInits.length >= 1 && re.c4.codexOk),
    check('the relay was pushed on the same process\'s legacy stdio, and a wake followed the Codex traffic', re.c4.relayOnLegacyStdio && re.c4.wakeAfterCodex),
    check('no error answer on the legacy process after its initialize', re.c4.errorsOnLegacy.length === 0, re.c4.errorsOnLegacy.map((e) => `line ${e.line} ${e.method}`).join('; ') || null),
    check('exactly one Codex HTTP MCP session reached the server, so the Codex traffic is attributable to the per-invocation registration (a second session means another registration, e.g. a leftover global entry, also connected)', codexSessions(run, re.roles.legacyPid).length === 1, `${codexSessions(run, re.roles.legacyPid).length} Codex session(s)`),
    check('the Codex session ran while the Claude session was live (Claude agent started first, never stopped)', commands.some((c) => c.argv.includes('g4claude') && c.argv.includes('start')) && commands.findIndex((c) => c.argv.includes('g4claude') && c.argv.includes('start')) < commands.findIndex((c) => c.argv.includes('g4codex') && c.argv.includes('start'))),
  );
  mechanicalRow(c4, 'concurrent, not sequential: the modern path still correct after legacy/Codex activity interleaved with it, on one process, as in the human run');

  // --- 5 -------------------------------------------------------------------------------------
  const startupRead = sec(g4.afterStartupReadSeq);
  c5.run = `g4modern pid ${re.roles.modernPid}: server/discover supportedVersions ${JSON.stringify(re.c5.discover?.supportedVersions ?? null)} (line ${re.c5.discover?.resLine ?? '?'}); pushes lines ${re.c5.modernPushes.map((p) => p.line).join(', ') || 'none'}`;
  c5.checks.push(
    check('the negative-case copy answered server/discover modern-only (2026-07-28) while declaring claude/channel', re.c5.modernOnly),
    check('it pushed a channel notification at each wake (so non-arrival is observable)', re.c5.modernPushes.length >= 2),
    check('the Claude pane was read verbatim after startup and after each wake', !!startupRead && wakeReads.every(Boolean)),
    check('(supporting only) the startup pane read shows an "unavailable" notice naming g4modern', !!startupRead && /g4modern/.test(startupRead.text) && /unavailable/i.test(startupRead.text), 'a text match only; the operator reads the pane'),
  );
  operatorRow(c5, operatorScores['5'], 'operator review of the verbatim Claude pane pending: was g4modern refused as a channel, and did its pushes not arrive (--score 5=... --note 5=...)');

  return { rows, base, run, sections, be, re };
}

// A per-exchange count, baseline against the run: surface, era and method.
export function exchangeTable(baseFacts, runFacts) {
  const key = (r) => `${r.surface} ${r.era ?? '-'} ${r.method}`;
  const count = (f) => {
    const m = new Map();
    for (const r of f?.requests ?? []) m.set(key(r), (m.get(key(r)) ?? 0) + 1);
    for (const p of f?.pushes ?? []) m.set(`stdio ${p.era} push notifications/claude/channel`, (m.get(`stdio ${p.era} push notifications/claude/channel`) ?? 0) + 1);
    return m;
  };
  const b = count(baseFacts);
  const r = count(runFacts);
  return [...new Set([...b.keys(), ...r.keys()])].sort().map((k) => ({ key: k, baseline: b.get(k) ?? 0, run: r.get(k) ?? 0 }));
}

export function renderReport({ manifest, evaluation, date, fixtures, runManifestName, baselinePath = BASELINE_TRANSCRIPT, reference = null }) {
  const g4 = manifest?.scenarioData?.g4 ?? {};
  const v = g4.versions ?? {};
  const out = [];
  out.push(`# G4 scripted re-run through herdr, ${date}: comparison with the human-run 2026-09-26 re-run`);
  out.push('');
  out.push('> **Not verdict-bearing.** Epic K, K8 #131. This record compares a herdr-driven run of G4 against the');
  out.push('> human-run 2026-09-26 re-run. G4\'s verdict (`docs/planning/gates/G4-result.md`, PASS) and');
  out.push('> `docs/planning/STATUS.md` are unchanged by it. Its Verification section is generated from the run manifest;');
  out.push('> until the recording agent has re-checked it and filled its slots, this is neither an equivalence record nor verdict-bearing.');
  out.push('>');
  out.push(...reconstructionCallout('G4', G4_SERVER_FILES, 'docs/planning/gates/G4-result.md'));
  out.push('');
  out.push(`- **Driver:** herdr (\`${manifest?.herdr?.observedVersionOutput ?? '?'}\`, PINS.md \`herdr (test tooling)\` ${manifest?.herdr?.pinnedTag ?? '?'}) via \`tools/herdr/run.mjs\`, scenario \`${manifest?.scenario?.file ?? '?'}\`, driver commit \`${manifest?.driver?.commit ?? '?'}\`${manifest?.driver?.toolsHerdrDirty !== false ? ` (tools/herdr dirty: ${manifest?.driver?.toolsHerdrDirty})` : ''}`);
  out.push(`- **Run outcome:** ${manifest?.outcome ?? '?'}${manifest?.outcomeReason ? ` — ${manifest.outcomeReason}` : ''}`);
  out.push(`- **Versions:** \`claude --version\` = \`${v.cliOutput?.claude ?? '?'}\`, wire clientInfo \`${v.wire?.claude ?? '?'}\`; \`codex --version\` = \`${v.cliOutput?.codex ?? '?'}\`, wire MCP user-agent version \`${v.wire?.codex ?? '?'}\`; ${pinsVersionsText(v.pins)} (committed at \`${v.pins?.headCommit ?? '?'}\`); version warnings: ${v.warnings?.length ?? 0} (listed under Findings; versions float and are never gated, #216); unchanged through the run: ${g4.postRun?.matches ?? '?'}`);
  out.push(`- **Claude launch:** \`${(manifest?.launch?.argv ?? []).join(' ')}\` (verbatim G4 launch: ${g4.launch?.verbatim ?? '?'}); pane env ${JSON.stringify(g4.claudeEnv ?? {})}`);
  out.push(`- **Codex launch (per invocation):** \`${(g4.codexLaunch?.argv ?? []).join(' ')}\`; pane process argv ${g4.codexLaunch?.paneArgv?.proof?.found ? `pid ${g4.codexLaunch.paneArgv.proof.pid}, args ${JSON.stringify(g4.codexLaunch.paneArgv.proof.argsAfterCodex)}, matches the launch: ${g4.codexLaunch.paneArgv.matchesLaunch}` : 'not shown'}. **Method difference from the human run, by K8's acceptance:** the human run registered Codex's MCP connection in the operator's GLOBAL Codex config (G4-result.md, criterion 2); this run passes it per invocation, edits no Codex config and copies no Codex home. It cannot see the operator's own Codex config: a registration there pointing at this run's port would connect too, indistinguishably on the wire except as an extra session. This run saw ${codexSessions(evaluation.run ?? { httpInitialize: [] }, evaluation.re?.roles?.legacyPid).length} Codex HTTP session(s); criterion 4 requires exactly one before any Codex traffic is attributed to the per-invocation registration.`);
  out.push(`- **Server:** ${(g4.server ?? []).map((s) => `\`${s.path}\` working-tree sha256 \`${s.workingTreeSha256}\` (matches HEAD: ${s.workingTreeMatchesHead}), staged copy match: ${s.match}`).join('; ') || 'not staged'}; ports ${g4.ports?.httpPort}/${g4.ports?.modernHttpPort} free before the run: ${JSON.stringify(g4.ports?.freeBefore ?? null)}`);
  out.push(`- **Timebox:** ${manifest?.timebox?.budgetMs ?? '?'} ms, ${manifest?.timebox?.start ?? '?'} to ${manifest?.timebox?.end ?? '?'}; expired: ${manifest?.timebox?.expired ?? '?'}`);
  out.push(`- **Accept policy:** ${g4.acceptPolicy ?? '?'}; dialogs on record: ${describeDialogs(g4.dialogs)} (no G4 criterion names a consent step; the dev-channels confirmation scores nothing here)`);
  out.push(`- **herdr agent states seen** (scheduling only, never evidence): ${(g4.herdrStates ?? []).map((s) => `${s.agent} ${s.state} (#${s.seq})`).join(', ') || 'none'}`);
  out.push(fixtures ? `- **Fixtures:** ${Object.values(fixtures).map((f) => `\`${f}\``).join(', ')}` : `- **Fixtures:** none (${writeRefusal(manifest) ?? fixtureWithheld(manifest) ?? 'not published'})`);
  out.push(`- **Fixture sanitizer:** the public extension identifier \`${OAC_EXT}\` replaced by \`${OAC_EXT_PLACEHOLDER}\` before run.mjs's redaction, so identity redaction cannot rewrite it: transcript ${g4.sanitizer?.extensionIdReplaced ?? 0}, Claude pane ${g4.sanitizer?.paneClaude ?? '?'}, Codex pane ${g4.sanitizer?.paneCodex ?? '?'} time(s). --write re-counts the placeholders in each redacted capture and refuses on a mismatch or a surviving identifier fragment (a pane line wrapped mid-identifier escapes the substitution)`);
  out.push(`- **Run manifest:** \`${runManifestName}\` (beside this file); harness config unchanged: ${manifest?.harnessConfig?.unchanged ?? '?'}; teardown clean: ${manifest?.teardown?.clean ?? '?'}`);
  out.push(`- **Baseline:** \`${baselinePath}\` and \`docs/planning/gates/G4-result.md\` (the human-run 2026-09-26 re-run, verdict PASS, produced by the uncommitted original spike server)`);
  out.push(`- **Criteria source:** ${reference ? `\`${reference.path}\` as committed at \`${reference.headCommit}\` (file sha256 \`${reference.fileSha256}\`); the five parsed criteria hash to \`${reference.criteriaSha256}\`, the pin the scoring is written against (\`G4_CRITERIA_SHA256\` = \`${G4_CRITERIA_SHA256}\`)${reference.workingTreeMatchesHead ? '' : ' (the working-tree copy differs from HEAD and was not used)'}` : 'not recorded'}`);
  out.push('');
  out.push('## Criteria');
  out.push('');
  out.push('| # | Criterion (verbatim, `oac-gates` G4 reference) | Human run (recorded result) | This run | Score |');
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
  out.push('## Exchange counts');
  out.push('');
  out.push('Requests and pushes by surface, era and method: the human run\'s fixture against this run\'s transcript. The human run also had Codex\'s second (stdio) registration and two Codex HTTP sessions; this run passes one registration, per invocation (criterion 4 shows how many Codex sessions actually connected).');
  out.push('');
  out.push('| Exchange | Human run | This run |');
  out.push('|---|---|---|');
  for (const x of exchangeTable(evaluation.base, evaluation.run)) out.push(`| ${cell(x.key)} | ${x.baseline} | ${x.run} |`);
  out.push('');
  out.push('## Findings and UNVERIFIED');
  out.push('');
  for (const f of manifest?.findings ?? []) out.push(`- Finding: ${f}`);
  if (fixtureWithheld(manifest)) out.push(`- Finding: ${fixtureWithheld(manifest)}`);
  out.push('- Harness versions float and are never gated (#216): a version other than PINS.md\'s last tested one, or other than the baseline run\'s, is a finding here and does not by itself disqualify this record, including as an equivalence record.');
  out.push('- The server is a reconstruction (callout above); a difference in any criterion may come from it, not from Claude Code or Codex.');
  out.push('- Earlier `NOT RUN` or `FAIL` runs of this scenario at the same pins: none listed by this generator; add each by hand (run id, outcome, reason from its run manifest).');
  out.push('- Whether Codex honors a per-invocation `-c mcp_servers.<name>.url=...` override for an HTTP server was UNVERIFIED when this scenario was written. This run\'s wire answers it only if exactly one Codex session connected (criterion 4) and the recording agent confirmed with `codex mcp list` (read-only) that no other Codex entry points at this run\'s port.');
  out.push('- Pane-text patterns (dialogs, the in-progress indicator) were written before any live run; confirm them against this run\'s pane captures.');
  out.push('');
  out.push(...verification({
    manifest,
    harness: harnessVerification(manifest, { verified: versionsVerified(g4), versions: `Claude Code: CLI \`${v.cliOutput?.claude ?? '?'}\` (\`scenarioData.g4.versions.cliOutput.claude\`), wire clientInfo \`${v.wire?.claude ?? '?'}\` (\`scenarioData.g4.versions.wire.claude\`); Codex: CLI \`${v.cliOutput?.codex ?? '?'}\` (\`scenarioData.g4.versions.cliOutput.codex\`), wire MCP user-agent \`${v.wire?.codex ?? '?'}\` (\`scenarioData.g4.versions.wire.codex\`); post-run match \`${g4.postRun?.matches ?? 'not recorded'}\` (\`scenarioData.g4.postRun.matches\`)` }),
    dialogs: g4.dialogs,
    dialogsField: 'scenarioData.g4.dialogs',
    humanActions: noConsentCriterionLine('G4'),
  }));
  return out.join('\n');
}

// Verified when each harness's CLI and wire report one and the same version and neither
// moved during the run, so a fixture names one version per harness. Whether those are
// PINS.md's last tested versions is NOT part of this: versions float and are never gated
// (#216); a difference is a VERSION WARNING finding and shows as version_matches_pin: false.
export function versionsVerified(g4) {
  const v = g4?.versions;
  if (!v || v.verified !== true || !v.cli?.claude || !v.cli?.codex) return false;
  return v.wire?.claude === v.cli.claude && v.wire?.codex === v.cli.codex && g4.postRun?.matches === true;
}

// Whether the verified versions equal PINS.md's last tested versions (informational only).
export function versionMatchesLastTested(g4) {
  const p = g4?.versions?.pins;
  return versionsVerified(g4) && !!p?.claudeLastTested && g4.versions.cli.claude === p.claudeLastTested && g4.versions.cli.codex === p.codexLastTested;
}

function pinsVersionsText(p) {
  if (p?.claudeLastTested) return `PINS.md Claude minimum \`${p.claudeMinimum ?? '?'}\`, last tested \`${p.claudeLastTested}\`; Codex minimum \`${p.codexMinimum ?? '?'}\`, last tested \`${p.codexLastTested ?? '?'}\``;
  return `PINS.md last observed Claude \`${p?.claudeLastObserved ?? '?'}\`, Codex \`${p?.codexLastObserved ?? '?'}\` (recorded before #216)`;
}

// Why --write writes the record and run manifest but NO fixture, or null. Operator decision
// on #216 (2026-10-01): when a harness's CLI and wire disagree, or a version moved mid-run, the record is
// still written, with a VERSION WARNING; its captures stay `unverified-*` and no fixture
// or MANIFEST.json entry is produced, because a fixture names one version.
export function fixtureWithheld(manifest) {
  const g4 = manifest?.scenarioData?.g4;
  if (!g4 || versionsVerified(g4)) return null;
  return `VERSION WARNING: a harness's CLI and wire (CLI ${JSON.stringify(g4.versions?.cli ?? null)}, wire ${JSON.stringify(g4.versions?.wire ?? null)}) did not report one and the same version before and after the run, so no fixture can name it; the record and run manifest are written, the captures stay unverified-* and no fixture is added (#216)`;
}

// Why --write must refuse this run, or null. Mixed versions never refuse it (fixtureWithheld).
// captureTexts: { transcript, paneClaude, paneCodex } as redacted, when the caller has them.
export function writeRefusal(manifest, captureTexts = null) {
  const g4 = manifest?.scenarioData?.g4;
  if (manifest?.outcome !== 'PASS') return `run outcome is ${manifest?.outcome ?? 'missing'}${manifest?.outcomeReason ? ` (${manifest.outcomeReason})` : ''}; only a PASS run is written`;
  if (!g4) return 'no G4 scenario record in the run manifest';
  const publish = versionsVerified(g4);
  if (publish) {
    if (!g4.fixtures || JSON.stringify(g4.fixtures) !== JSON.stringify(g4.captureNames)) return 'captures do not carry the verified K8 fixture names';
    for (const f of Object.values(g4.fixtures)) if (!manifest.captures?.some((c) => c.file === f && c.written)) return `capture ${f} was not written (withheld or missing)`;
  }
  if (!(g4.server ?? []).length || !g4.server.every((s) => s.match && s.workingTreeMatchesHead)) return 'the staged gate server does not match its committed source';
  const expected = { transcript: g4.sanitizer?.extensionIdReplaced, paneClaude: g4.sanitizer?.paneClaude, paneCodex: g4.sanitizer?.paneCodex };
  for (const k of publish ? Object.keys(expected) : []) {
    if (!Number.isInteger(expected[k])) return `the extension-identifier sanitizer did not record a count for the ${k} capture`;
    if (captureTexts) {
      const pi = placeholderIntegrity(captureTexts[k], expected[k]);
      if (!pi.ok) return `the ${k} capture's extension-identifier placeholders did not survive redaction intact (${pi.found} found, ${pi.expected} put in${pi.fragment ? ', and an identifier fragment remains' : ''})`;
    }
  }
  if (manifest.driver?.toolsHerdrDirty !== false || !manifest.driver?.commit) return `the run's tools/herdr/ was not clean and committed (toolsHerdrDirty ${JSON.stringify(manifest.driver?.toolsHerdrDirty ?? null)}); its captures are never committed as fixtures (scripted-runs.md "Driver identity")`;
  return null;
}

const CODEX_MCP_SCHEMA = { applicable: false, reason: 'Codex appears here only as an outbound MCP client (codex-mcp-client) calling tools on the reconstructed G4 MCP server, not as the Codex app-server JSON-RPC protocol; codex-rs/app-server-protocol/schema/json does not describe this exchange.' };
const PANE_SCHEMA = { applicable: false, reason: 'pane text: verbatim herdr agent reads of the Codex TUI screen, not app-server protocol traffic' };

export function draftManifestEntries({ manifest, fixtures, runManifestPath, transcriptText, pinsCommit, redactSha256 }) {
  const g4 = manifest.scenarioData.g4;
  const v = g4.versions;
  const f = g4Facts(parseG4Transcript(transcriptText));
  const re = wireEvidence(f);
  const red = (name) => {
    const r = manifest.captures.find((c) => c.file === name)?.redaction;
    return { script: 'tools/herdr/lib/redact.mjs', script_sha256: redactSha256, residual_scan_result: r ? `tools/herdr/lib/redact.mjs (run.mjs), after lib/g4.mjs sanitizeG4Transcript (extension identifier -> ${OAC_EXT_PLACEHOLDER}, ${g4.sanitizer?.extensionIdReplaced ?? 0}x): droppedHazardLines=${r.droppedHazardLines}, hazardProtocolFrames=${r.hazardProtocolFrames.length}, residualLeaks=${r.residualLeaks.length}, residualGenericHits=${r.residualGenericHits.length}` : 'not recorded' };
  };
  const lines = (xs) => xs.map((x) => (x.resLine ? lineSpan(x.reqLine, x.resLine) : `${x.line ?? x.reqLine}`)).join(', ') || null;
  const common = {
    pins_as_of: `PINS.md as committed at HEAD ${v.pins?.headCommit ?? '?'} when the run started; last commit touching PINS.md at report time: ${pinsCommit ?? 'unknown'}`,
    version_matches_pin: versionMatchesLastTested(g4),
    ...(versionMatchesLastTested(g4) ? {} : { version_matches_pin_note: `Claude Code ${v.cli.claude} / Codex ${v.cli.codex} are not both PINS.md's last tested versions (${v.pins?.claudeLastTested ?? v.pins?.claudeLastObserved ?? '?'} / ${v.pins?.codexLastTested ?? v.pins?.codexLastObserved ?? '?'}); recorded as VERSION WARNING findings, not a gate (#216)` }),
    capture_date: g4.date,
    superseded_by: null,
    driver: { herdr_version: manifest.herdr.observedVersionOutput, driver_commit: manifest.driver.commit, run_manifest: runManifestPath },
    notes: ['K8 scripted run through herdr; NOT verdict-bearing (G4 verdict unchanged). The server is tools/herdr/gate-servers/g4-server.mjs, a RECONSTRUCTION of the never-committed G4 spike server, not the server that produced the human-run fixtures.'],
  };
  return [
    {
      path: `${FIXTURE_DIR}/${fixtures.transcript}`,
      provider: 'claude+codex',
      surface: 'claude-channels (server) + codex MCP client (reconstructed G4 server)',
      observed_version: { claude_code: `${v.cli.claude} (claude --version; wire clientInfo.version ${v.wire.claude})`, codex_cli: `${v.cli.codex} (codex --version; MCP user-agent codex-mcp-client/${v.wire.codex})`, mcp_current_era: MODERN, mcp_legacy_era: re.c1.init?.negotiated ?? null, node: null },
      pins_row: 'Claude Code (Channels); Codex CLI / app-server',
      ...common,
      capture_utc_range: f.firstT && f.lastT ? `${f.firstT}-${f.lastT}` : null,
      redaction: red(fixtures.transcript),
      coverage: {
        'server/discover (stdio, modern-only negative-case copy)': lines(f.stdioDiscover.filter((x) => !x.error)),
        'server/discover (stdio probe, rejected -32601)': lines(f.stdioDiscover.filter((x) => x.error)),
        'server/discover (HTTP, modern)': lines(f.httpDiscover),
        'initialize (legacy stdio)': lines(f.stdioInitialize),
        'tools/list': lines([...f.stdioToolsList, ...f.httpToolsList]),
        'initialize (Codex HTTP legacy)': lines(f.httpInitialize),
        'tools/call (Claude HTTP-modern)': lines(re.c2.claudeModern),
        'tools/call (Codex HTTP legacy)': lines(re.c4.codexCalls),
        'notifications/claude/channel': f.pushes.map((p) => p.line).join(', ') || null,
      },
      schema: CODEX_MCP_SCHEMA,
    },
    {
      path: `${FIXTURE_DIR}/${fixtures.paneClaude}`,
      provider: 'claude',
      surface: 'claude-code TUI pane text (herdr agent read)',
      observed_version: { claude_code: v.cli.claude, node: null },
      pins_row: 'Claude Code (Channels)',
      ...common,
      capture_utc_range: null,
      redaction: red(fixtures.paneClaude),
      coverage: { 'pane reads': 'verbatim herdr agent reads of the Claude Code TUI, one section per kept read, each with its herdr command seq and timestamps' },
    },
    {
      path: `${FIXTURE_DIR}/${fixtures.paneCodex}`,
      provider: 'codex',
      surface: 'codex TUI pane text (herdr agent read)',
      observed_version: { codex_cli: v.cli.codex, node: null },
      pins_row: 'Codex CLI / app-server',
      ...common,
      capture_utc_range: null,
      redaction: red(fixtures.paneCodex),
      coverage: { 'pane reads': 'verbatim herdr agent reads of the Codex TUI, one section per kept read, each with its herdr command seq and timestamps' },
      schema: PANE_SCHEMA,
    },
  ];
}

function main(argv) {
  const o = parseReportArgs(argv);
  const operatorScores = parseG4OperatorScores(o.scores.map((s) => ({ ...s, note: o.notes[s.n] })));
  const runDir = resolve(o.run);
  const manifest = JSON.parse(readFileSync(join(runDir, 'run-manifest.json'), 'utf8'));
  const g4 = manifest.scenarioData?.g4;
  const names = g4?.captureNames ?? null;
  const fixtures = g4?.fixtures ?? null;
  const written = (name) => name && manifest.captures?.some((c) => c.file === name && c.written) && existsSync(join(runDir, name));
  const readCap = (name) => (written(name) ? readFileSync(join(runDir, name), 'utf8') : null);
  const transcriptText = names ? readCap(names.transcript) : null;
  const paneClaudeText = names ? readCap(names.paneClaude) : null;
  const baselinePath = o.baseline ?? BASELINE_TRANSCRIPT;
  const baselineText = readFileSync(resolve(REPO, baselinePath), 'utf8');
  const { criteria, reference } = readG4Criteria(REPO); // throws before anything is printed or written if the criteria drifted
  const evaluation = evaluateG4({ manifest, transcriptText, paneClaudeText, baselineText, criteria, operatorScores });
  const date = g4?.date ?? manifest.timebox?.start?.slice(0, 10) ?? 'unknown-date';
  const runManifestName = `G4-${date}.run-manifest.json`;
  const refusal = writeRefusal(manifest, names ? { transcript: transcriptText, paneClaude: paneClaudeText, paneCodex: readCap(names.paneCodex) } : null);
  const publish = !refusal && !fixtureWithheld(manifest);
  const report = renderReport({ manifest, evaluation, date, fixtures: publish ? Object.fromEntries(Object.entries(fixtures).map(([k, f]) => [k, `${FIXTURE_DIR}/${f}`])) : null, runManifestName, baselinePath, reference });
  if (!o.write) {
    console.log(report);
    return 0;
  }
  if (refusal) throw new ReportError(`--write refused: ${refusal}. Nothing was written; print the draft without --write to inspect the run`);
  const root = o.root ? resolve(o.root) : REPO;
  const runsDir = join(root, HERDR_RUNS_DIR);
  const targets = [[join(runsDir, `G4-${date}.md`), null], [join(runsDir, runManifestName), join(runDir, 'run-manifest.json')], ...(publish ? Object.values(fixtures).map((f) => [join(root, FIXTURE_DIR, f), join(runDir, f)]) : [])];
  writeTargets(targets, report);
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
    redactSha256: redactScriptSha256(),
  });
  JSON.parse(committedFile(REPO, MANIFEST_PATH).bytes.toString('utf8')); // MANIFEST.json must parse at HEAD before a draft is offered for merging
  writeFileSync(join(runDir, 'manifest-entries.draft.json'), `${JSON.stringify(entries, null, 2)}\n`);
  console.log(`wrote ${targets.map(([t]) => t).join('\n      ')}`);
  console.log('Next: review the draft; score criteria 1 and 5 from the Claude pane text; the recording agent re-checks the Verification section and fills its slots; merge <run dir>/manifest-entries.draft.json into docs/planning/gates/fixtures/MANIFEST.json; run node scripts/check-fixture-manifest.mjs; add only a pointer to G4-result.md.');
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (err) {
    console.error(`g4-report: ${err.message}`);
    process.exitCode = 2;
  }
}
