#!/usr/bin/env node
// L3c (#191): draft `## 13. Live results (L3)` for docs/planning/decisions/L1-beacon-memory.md
// from the L3 phase runs (the l3-beacon scenario, L3b #190). PRINT ONLY.
//
//   node tools/herdr/lib/l3-report.mjs --baseline <run dir> --probe <run dir> [--verify <run dir>]
//        [--prior <run dir> ...] [--note B5='<PASS|FINDING|NOT RUN>: <text>'] [--note B6=...]
//
// It prints the draft to stdout and writes nothing, anywhere: there is no --write mode. The L3d
// agent (#192) reviews the draft and pastes it into §13 itself.
//
// Inputs. Each run dir's run-manifest.json is read, and nothing else. The phase record is
// `scenarioData.l3` (lib/l3.mjs L3Record). A record whose `version` is not L3_RECORD_VERSION is
// refused, as is a record whose phase does not match the flag it was given under.
//
// Three vocabularies (oac-gates references/scripted-runs.md). A phase run's outcome (PASS / FAIL
// / NOT RUN) is not a step result. A run that ended NOT RUN or FAIL makes its steps NOT RUN,
// with the manifest's outcomeReason. A zero-hit scan in a PASS run is a result and is reported
// as one. Step results here are PASS / FINDING / NOT RUN (L1 §12 "Exit"); none is a gate
// verdict and the draft changes none.
//
// One L3 box spans the phases (L1 §12 "Timebox: 60 minutes"). Its start is the baseline
// record's `box.start`. Every step whose phase run ended after start + 60 minutes is
// `NOT RUN (L3 box expired)`.
//
// Operator decisions of 2026-09-30 on #168 (binding): B1 is recorded NOT RUN (Beacon was
// installed before B0; no uninstall/reinstall); B5 and B6 are NOT RUN by default (Beacon steps
// only the operator may do; an operator note replaces the default); pin drift is recorded as a
// finding and does not stop the leg (L3 is not a gate). Since #216 (2026-10-01) this holds for
// every scripted run: harness versions float, and a difference from PINS.md's last tested (or
// minimum) version is a VERSION WARNING finding, never a stop.
//
// Leak guard. Before anything is printed, the whole draft is checked: (1) no probe-marker- or
// fake-token-shaped value, nor a prefix followed by a partial value (the report never holds the
// probe values, only their sha256, so the shape is what it can test; with in-process markers,
// assertNoMarkerLeak runs too); (2) createRedactor().scan() on neutralizePlaceholders(draft)
// reports no residual (home path, username, host, token shape, secret assignment). Any hit
// aborts with exit 4 and prints no draft; the error names kinds and line numbers only.
//
// Refusal messages (stderr) carry only safe-shaped ids, never a manifest value, and pass the leak
// guard too. Manifest free text is collapsed to one line, so it cannot forge a step line.
// Timestamps count only when zone-qualified (`Z` or an offset).
//
// Exit codes: 0 draft printed; 2 usage error or refused input; 4 leak guard abort.
//
// Node built-ins only.

import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { L3_RECORD_VERSION, MARKER_SHAPE, TOKEN_SHAPE, sha256, compareSections, neutralizePlaceholders, assertNoMarkerLeak } from './l3.mjs';
import { createRedactor } from './redact.mjs';
import { describeDialogs } from './gate-report-common.mjs';

export class L3ReportError extends Error {}
export class L3LeakAbort extends Error {}

export const PHASES = Object.freeze(['baseline', 'probe', 'verify']);
export const STEPS = Object.freeze(['B0', 'B1', 'B2', 'B3', 'B4', 'B5', 'B6', 'B7']);
export const RESULTS = Object.freeze({ PASS: 'PASS', FINDING: 'FINDING', NOT_RUN: 'NOT RUN' });
// L1 §12 "Timebox: 60 minutes", declared before B0 and never extended.
export const L3_BOX_MS = 60 * 60 * 1000;
// L1 §2: the Beacon version the leg runs against. A Beacon pin move updates this constant.
export const BEACON_PIN = '1.3.29';
// The pin as a whole version: `1.3.290` or `11.3.29` do not match.
const BEACON_PIN_RE = new RegExp(`(?<![\\d.])${BEACON_PIN.replace(/\./g, '\\.')}(?![\\d.])`);
// Which phase run carries each step (L3b #190: baseline = B0; probe = B1 record plus B2-B4;
// verify = B7). B5 and B6 are operator steps outside the driver.
export const STEP_PHASE = Object.freeze({ B0: 'baseline', B1: 'probe', B2: 'probe', B3: 'probe', B4: 'probe', B5: null, B6: null, B7: 'verify' });
// B3's outbound capture: the tool-invocation actions L1 §12 B3 names.
export const TOOL_INVOKED_ACTIONS = Object.freeze(['mcp.tool_invoked', 'tool.invoked']);

const OPERATOR_DECISION = 'operator decisions of 2026-09-30 on #168';
const B1_NOT_RUN = `B1 recorded NOT RUN by the ${OPERATOR_DECISION} (item 2): Beacon was installed before any B0 baseline, and it is not uninstalled and re-installed. Evidence is the 2026-09-30 MSI observations (L1 §12 amendment) plus the weaker \`.beacon.bak\` evidence, stated as such`;
const B56_NOT_RUN = `default taken in the ${OPERATOR_DECISION}: B5 and B6 are Beacon steps only the operator may do; no operator note was supplied (--note B5=... / --note B6=...)`;

// --- helpers ---------------------------------------------------------------------------

// Manifest-sourced free text is collapsed to one line wherever it is rendered, so a newline in
// an outcomeReason, finding, note or id cannot start a forged step line in the draft.
export const oneLine = (s) => String(s ?? '').replace(/\s+/g, ' ').trim();
const tick = (s) => `\`${oneLine(s).replace(/`/g, "'")}\``;
// Values that may appear in a refusal message (stderr, which the draft's leak guard does not
// cover): an id of a safe shape, else a placeholder. Never the value itself.
export const safeId = (s) => (/^[A-Za-z0-9._:-]{1,80}$/.test(String(s ?? '')) ? String(s) : '<withheld>');
const safeVersion = (v) => (Number.isInteger(v) ? String(v) : '<non-integer>');
const safePhase = (p) => (PHASES.includes(p) ? p : '<unknown>');
const val = (v) => {
  if (v === null || v === undefined) return 'not recorded';
  if (typeof v === 'object') return Object.entries(v).map(([k, x]) => `${k} ${tick(x ?? 'null')}`).join(', ') || 'not recorded';
  return tick(v);
};
// PINS.md's versions as a record holds them: minimum and last tested since #216, the then
// "last observed" version in older records.
const pinsText = (p) => (p?.lastTested ? `PINS.md minimum ${val(p.minimum)}, last tested ${val(p.lastTested)}` : `PINS.md last observed ${val(p?.lastObserved)}`);
const list = (a) => (a && a.length ? a.map(tick).join(', ') : 'none');
// Zone-qualified ISO times only (`Z` or `+hh:mm`): Date.parse reads an unzoned time as local
// time, so such a time counts as not recorded.
const ZONED_ISO = /^\d{4}-\d\d-\d\dT\d\d:\d\d(?::\d\d(?:\.\d+)?)?(?:Z|[+-]\d\d:\d\d)$/;
const ms = (t) => (typeof t === 'string' && ZONED_ISO.test(t) ? Date.parse(t) : NaN);
const isoOrNull = (n) => (Number.isFinite(n) ? new Date(n).toISOString() : null);
// Config labels the draft may carry: the `~` and `$VAR` labels lib/l3.mjs harnessConfigTargets()
// produces. Anything else could carry a path, so it is withheld.
const SAFE_LABEL = /^(?:~|\$CLAUDE_CONFIG_DIR|\$CODEX_HOME)(?:\/[A-Za-z0-9._-]+)+$/;
const label = (l) => (SAFE_LABEL.test(String(l)) ? tick(l) : '`<label withheld: not a ~ or $VAR label>`');

// --- loading and refusal ---------------------------------------------------------------

/** Read one phase run dir's run-manifest.json. Nothing else in the dir is read. */
export function loadRun(dir) {
  const path = join(resolve(dir), 'run-manifest.json');
  let text;
  try {
    text = readFileSync(path, 'utf8');
  } catch (err) {
    throw new L3ReportError(`cannot read run-manifest.json in a given run dir (${err.code ?? 'read failed'})`);
  }
  try {
    return JSON.parse(text);
  } catch {
    throw new L3ReportError('a run-manifest.json is not JSON');
  }
}

/**
 * The phase record of one run, checked. Refuses an unknown record version and a record of the
 * wrong phase. A PASS run without a record is refused too (the scenario must have written one);
 * a NOT RUN or FAIL run may lack one.
 * @returns {object|null}
 */
export function phaseRecord(manifest, phase) {
  const rec = manifest?.scenarioData?.l3;
  const id = safeId(manifest?.runId);
  if (rec === undefined || rec === null) {
    if (manifest?.outcome === 'PASS') throw new L3ReportError(`${phase} run ${id} ended PASS but carries no scenarioData.l3 record; refusing`);
    return null;
  }
  if (rec.version !== L3_RECORD_VERSION) throw new L3ReportError(`${phase} run ${id}: unknown L3 record version ${safeVersion(rec.version)} (this report knows ${L3_RECORD_VERSION}); refusing`);
  if (rec.phase !== phase) throw new L3ReportError(`run ${id} was given as --${phase} but its L3 record is phase ${safePhase(rec.phase)}; refusing`);
  return rec;
}

// --- evaluation ------------------------------------------------------------------------

function boxOf(baseline) {
  const rec = baseline?.record;
  const notes = [];
  let start = rec?.box?.start ?? null;
  let source = 'baseline record `box.start`';
  if (!start) {
    start = baseline?.manifest?.timebox?.start ?? null;
    source = `baseline run manifest \`timebox.start\` (${rec ? 'the baseline record has no `box.start`' : 'the baseline carries no L3 record'})`;
  }
  if (rec?.box && rec.box.budgetMs !== L3_BOX_MS) notes.push(`the baseline record declares box.budgetMs ${tick(rec.box.budgetMs)}; L1 §12 fixes the box at ${L3_BOX_MS} ms, which this report applies`);
  if (rec?.box && rec.box.sourceRunId !== null && rec.box.sourceRunId !== undefined) notes.push('the baseline record continues another run\'s box (box.sourceRunId is set); the baseline phase should open the box');
  const s = ms(start);
  return { start: start === null ? null : oneLine(start), source, budgetMs: L3_BOX_MS, startMs: s, end: isoOrNull(s + L3_BOX_MS), endMs: s + L3_BOX_MS, known: Number.isFinite(s), notes };
}

// Why a phase's steps are NOT RUN, or null when they can be evaluated.
function phaseBlock(phase, run, box, baseline) {
  if (!run) return `no ${phase} run was supplied (--${phase})`;
  const m = run.manifest;
  if (m.outcome !== 'PASS') return `the ${phase} run ${tick(m.runId ?? '?')} ended ${oneLine(m.outcome ?? 'without an outcome')}${m.outcomeReason ? `: ${oneLine(m.outcomeReason)}` : ''}`;
  if (!box.known) return 'the L3 box start is not recorded as a zone-qualified ISO time, so the box cannot be checked';
  const start = ms(m.timebox?.start);
  const end = ms(m.timebox?.end);
  if (!Number.isFinite(start) || !Number.isFinite(end)) return `the ${phase} run's start or end is not recorded as a zone-qualified ISO time, so the L3 box cannot be checked`;
  // The baseline run opens the box: run.mjs records its timebox.start before setup, so the
  // scenario declares box.start slightly later. The baseline is refused only when the box was
  // declared after the baseline run had ended. Probe and verify must start inside the box.
  if (phase === 'baseline') {
    if (box.startMs > end) return `the L3 box was declared (${box.start}) after the baseline run ended ${m.timebox.end}`;
  } else if (start < box.startMs) return `the ${phase} run started ${m.timebox.start}, before the L3 box was declared (${box.start})`;
  if (end > box.endMs) return `L3 box expired: the ${phase} run ended ${m.timebox.end}, after the box ended ${box.end}`;
  if (phase !== 'baseline') {
    const b = run.record?.box;
    if (!b || b.start !== box.start || b.sourceRunId !== (baseline?.manifest?.runId ?? null)) {
      return `the ${phase} record does not continue the baseline's L3 box (box.start ${tick(b?.start ?? 'missing')}, box.sourceRunId ${tick(b?.sourceRunId ?? 'missing')})`;
    }
  }
  return null;
}

const step = (id, result, summary, evidence = [], findings = []) => ({ id, result, summary, evidence, findings });

function b0(run) {
  const r = run.record;
  const v = r.versions ?? {};
  const hashes = r.config?.hashes ?? [];
  const findings = [];
  const beacon = v.beacon ?? null;
  if (!beacon) findings.push('`beacon version` output not recorded (beaconCli off and not supplied)');
  else if (!BEACON_PIN_RE.test(String(beacon))) findings.push(`Beacon version differs from the L1 §2 pin ${BEACON_PIN}`);
  for (const [k, name] of [['claude', 'Claude Code'], ['codex', 'Codex']]) if (v.pins?.[k]?.differs) findings.push(`${name} differs from its PINS.md last tested version (VERSION WARNING, recorded as a finding; versions float and are never gated, #216)`);
  for (const f of r.config?.findings ?? []) findings.push(oneLine(f));
  // config.envSet (L3b #195) is the scenario's own record; older records fall back to the labels.
  const env = r.config?.envSet;
  const set = (name, prefix) => (env && typeof env[name] === 'boolean' ? (env[name] ? 'set' : 'not set') : hashes.some((h) => String(h.label).startsWith(prefix)) ? 'set' : 'not set');
  const evidence = [
    `Versions: Beacon ${val(beacon)}; \`claude --version\` ${val(v.claudeCli)}; Codex CLI ${val(v.codex?.cli)} (daemon and wire are read in the probe phase)`,
    `Environment (whether set only${env ? '' : ', inferred from the hash labels'}): CLAUDE_CONFIG_DIR ${set('CLAUDE_CONFIG_DIR', '$CLAUDE_CONFIG_DIR/')}, CODEX_HOME ${set('CODEX_HOME', '$CODEX_HOME/')}`,
    'Config hashes (sha256, labels only):',
    ...hashes.map((h) => `  - ${label(h.label)}: ${!h.present ? 'absent' : h.sha256 ? tick(h.sha256) : `unreadable (${h.error ?? 'no hash'})`}`),
  ];
  const tc = hashes.find((h) => h.kind === 'codex-config' && h.trailingComment)?.trailingComment;
  if (tc) evidence.push(`config.toml trailing-comment headers: ${tc.headers.length}${tc.headers.length ? ` (lines ${tc.headers.map((x) => x.line).join(', ')})` : ''}; one directly after [otel]/[otel.*]: ${tc.anyAfterOtel ? 'yes' : 'no'}`);
  if (hashes.some((h) => !SAFE_LABEL.test(String(h.label)))) findings.push('a config hash label is not a ~ or $VAR label and was withheld');
  return step('B0', findings.length ? RESULTS.FINDING : RESULTS.PASS, `preflight and baselines recorded (${hashes.length} config file(s) hashed)`, evidence, findings);
}

// Which changed sections fall outside L1 §11 item 4's table of Beacon writes.
function beyondTable(kind, sections, status) {
  const all = [...sections.changed, ...sections.added, ...sections.removed];
  if (kind === 'claude-settings') return all.filter((k) => !/^(?:key:env|key:hooks|env\..+|hooks\..+)$/.test(k));
  if (kind === 'codex-config') return all.filter((k) => !/^\[otel(?:\.[^\]]*)?\](?:#\d+)?$/.test(k));
  if (kind === 'claude-state') return status === 'unchanged' || status === 'absent' ? [] : ['(any change: only `beacon mcp connect` writes this file, and it is out of scope)'];
  return [];
}

function configDiff(before, after, attribution) {
  const cmp = compareSections(before, after);
  const kinds = Object.fromEntries([...before, ...after].map((h) => [h.label, h.kind]));
  const lines = [];
  const beyond = [];
  for (const f of cmp.files) {
    const s = f.sections;
    const parts = [`${label(f.label)}: ${f.status}${f.formattingOnly ? ' (formatting only: every section hashes equal)' : ''}`];
    if (s.changed.length || s.added.length || s.removed.length) parts.push(`sections changed ${list(s.changed)}, added ${list(s.added)}, removed ${list(s.removed)}`);
    const out = beyondTable(kinds[f.label], s, f.status);
    if (out.length) {
      parts.push(`outside L1 §11 item 4's table: ${out.map((k) => (k.startsWith('(') ? k : tick(k))).join(', ')}`);
      beyond.push(f.label);
    }
    if (f.status !== 'unchanged' && f.status !== 'absent' && attribution) parts.push(`attributed to: ${attribution}`);
    lines.push(`  - ${parts.join('; ')}`);
  }
  return { cmp, lines, beyond };
}

function b1(probe, baseline) {
  const evidence = [];
  if (probe && baseline?.record) {
    const d = configDiff(baseline.record.config?.hashes ?? [], probe.record.config?.hashes ?? [], 'operator Beacon step (the MSI install and its repair pass, outside the driver)');
    evidence.push('Supporting evidence only, not a B1 result: baseline vs. probe-phase start, per file:', ...d.lines);
    const tc = (rec) => rec.config?.hashes?.find((h) => h.kind === 'codex-config' && h.trailingComment)?.trailingComment;
    const tb = tc(baseline.record);
    const tp = tc(probe.record);
    evidence.push(`config.toml trailing-comment check: baseline ${tb ? `${tb.headers.length} header(s), after [otel]: ${tb.anyAfterOtel ? 'yes' : 'no'}` : 'not recorded'}; probe start ${tp ? `${tp.headers.length} header(s), after [otel]: ${tp.anyAfterOtel ? 'yes' : 'no'}` : 'not recorded'}`);
  } else evidence.push('No baseline-to-probe config comparison is available (a phase is not evaluable).');
  return step('B1', RESULTS.NOT_RUN, B1_NOT_RUN, evidence);
}

function scanFileLines(scan) {
  const out = [];
  for (const f of scan.files ?? []) out.push(`  - log ${tick(f.label)}: ${f.linesScanned} line(s) scanned from byte ${f.fromByte}, ${f.hitLines} with a hit, ${f.unparsedLines} unparsed${f.startBeyondEnd ? '; START PAST THE END (rotated or truncated): no line scanned' : ''}`);
  return out;
}

function scanFindings(scan) {
  const f = [];
  if (!(scan.files ?? []).length) f.push('no runtime log file was scanned');
  for (const x of scan.files ?? []) {
    if (x.startBeyondEnd) f.push(`log ${tick(x.label)} was rotated or truncated after the offset was taken: no line scanned, so zero hits there is not a clean negative`);
    if (x.unparsedLines) f.push(`log ${tick(x.label)}: ${x.unparsedLines} hit line(s) did not parse as JSON; their fields are unknown`);
  }
  return f;
}

// A { key: count } map, rendered value-free (keys ticked, counts as integers).
const counts = (o) => (o && Object.keys(o).length ? Object.entries(o).map(([k, n]) => `${tick(k)} ${Number.isFinite(Number(n)) ? Number(n) : '?'}`).join(', ') : 'none');

function markerLines(scan, id) {
  const b = scan.byMarker?.[id];
  // scanRuntimeLog() makes an entry for every marker it was given, so a missing entry means the
  // path was not scanned: NOT RUN, never a zero-hit result.
  if (!b) return { lines: [`  - ${id}: not in the scan summary (this delivery path was not scanned)`], token: false, zero: false, missing: true, b: null };
  // L3b (#195) records per-action / per-path counts in scan.counts; older records do not.
  const c = scan.counts?.[id];
  return {
    b,
    token: b.tokenVerbatimLines > 0,
    zero: b.lines === 0,
    lines: [
      `  - ${id}: ${b.lines} hit line(s)${b.lines === 0 ? ' (zero hits: a result, not NOT RUN)' : ''}; marker in ${b.markerLines}; fake token unredacted in ${b.tokenVerbatimLines}`,
      `    event.action ${list(b.actions)}; collection method ${list(b.collectionMethods)}; JSON path ${list(b.paths)}`,
      c
        ? `    hit lines per action: ${counts(c.byAction)}; per JSON path: ${counts(c.byPath)}; per collection method: ${counts(c.byCollectionMethod)}`
        : '    per-action hit counts: not in this record (records from before L3b #195 list the actions seen, not a count per action)',
    ],
  };
}

// The poll path (`beacon endpoint <h> sync --print`) for one step, from record.beacon.sync.
function pollLines(rec, stepId, h, ids) {
  const s = rec.beacon?.sync?.[stepId];
  const cmd = `\`beacon endpoint ${h} sync --print\``;
  if (!s) return { lines: [`Poll path (${cmd}) at ${stepId}: NOT RUN (this record carries no poll-path result; records from before L3b #195 have no field for it)`], findings: [] };
  if (s.status !== 'recorded') return { lines: [`Poll path (${cmd}) at ${stepId}: NOT RUN (${oneLine(s.reason ?? s.status ?? 'no reason recorded')})`], findings: [] };
  const lines = [`Poll path (${cmd}) at ${stepId}: ${Number(s.lines ?? 0)} event line(s) read${s.streamed ? ', scanned as streamed (no size cap)' : ''}`];
  const findings = [];
  if (s.truncated) findings.push(`the ${stepId} poll-path output was truncated; its counts are a lower bound`);
  for (const id of ids) {
    const c = s.counts?.[id];
    const b = s.byMarker?.[id];
    if (!c && !b) {
      lines.push(`  - ${id}: not in the poll-path summary`);
      continue;
    }
    const tokenLines = c?.tokenLines ?? b.tokenVerbatimLines;
    lines.push(`  - ${id}: ${c?.lines ?? b.lines} line(s)${(c?.lines ?? b.lines) === 0 ? ' (zero hits: a result)' : ''}; marker in ${c?.markerLines ?? b.markerLines}; fake token unredacted in ${tokenLines}${c ? `; per action ${counts(c.byAction)}` : ''}`);
    if (tokenLines > 0) findings.push(`the ${id} fake token appears unredacted in the poll-path output (${cmd}, ${stepId})`);
  }
  return { lines, findings };
}

function sessionFileLines(rec) {
  const sf = rec.sessionFile;
  if (!sf) return ['Claude session file (entry types and flags): not in this record (records from before L3b #195 have no field for it)'];
  if (!sf.read) return ['Claude session file (entry types and flags): not read (readSessionFile was not true)'];
  const e = sf.entries ?? [];
  return [
    `Claude session file (entry types and flags only, no content; ${Number(sf.files ?? 0)} file(s) in ${Number(sf.dirsFound ?? 0)} probe-project dir(s)): ${e.length} entry(ies) hold the claude-channel probe value`,
    ...e.map((x) => `  - ${x.parsed ? `type ${tick(x.type ?? 'null')}, isMeta ${tick(String(x.isMeta))}, attachment type ${tick(x.attachmentType ?? 'none')}, attachment keys ${list(x.attachmentKeys ?? [])}` : 'an unparsed line'}; marker ${x.markerPresent ? 'yes' : 'no'}, fake token ${x.tokenPresent ? 'yes' : 'no'}`),
  ];
}

// B2 and B3 are split by the scenario's log snapshots (steps.B2.log, steps.B3.log).
function snapshotLine(rec, stepId, which) {
  const snap = rec.steps?.[stepId]?.log;
  const c = snap?.[which]?.counts?.['claude-channel'];
  if (!c) return null;
  return `${stepId} log snapshot (${which === 'cumulative' ? 'from the probe start, taken before B3' : 'lines added since the B2 snapshot'}): claude-channel ${Number(c.lines)} line(s), per action ${counts(c.byAction)}`;
}

const HARNESS_OF = { 'claude-channel': /claude/i, 'codex-turn-start': /codex/i, 'codex-queue-add': /codex/i };

// probeSessions (L3b #195): the harness-side ids of the probe's own sessions. When a harness's
// list is recorded, "outside the probe session" is exact against it (under the UNVERIFIED
// equality of Beacon's session.id and the harness id); otherwise it is inferred from counts.
function sessionFindings(scan, ids, probeSessions = null) {
  const groups = (scan.bySession ?? []).filter((g) => g.markerIds.some((m) => ids.includes(m)));
  const f = [];
  // An empty list means unknown (e.g. no session file was found), never "no probe session".
  const known = (h) => (Array.isArray(probeSessions?.[h]) && probeSessions[h].length ? probeSessions[h] : null);
  for (const g of groups) {
    if (g.sessionId === null || g.harness === null) f.push(`${g.lines} hit line(s) carry no harness or session id: outside any identifiable probe session`);
    const wrong = g.markerIds.filter((m) => ids.includes(m) && HARNESS_OF[m] && g.harness !== null && !HARNESS_OF[m].test(g.harness));
    if (wrong.length) f.push(`a ${wrong.join(', ')} hit is in a ${tick(g.harness)} session: outside the probe session for that path`);
    const h = /claude/i.test(g.harness ?? '') ? 'claude' : /codex/i.test(g.harness ?? '') ? 'codex' : null;
    const k = h ? known(h) : null;
    if (k && g.sessionId !== null && !k.includes(g.sessionId)) f.push(`${g.lines} hit line(s) in ${h} session ${tick(g.sessionId)}, which is not a recorded probe session (${list(k)}): outside the probe session (Beacon session.id = harness id is UNVERIFIED)`);
  }
  for (const h of ['claude', 'codex']) {
    if (known(h)) continue;
    const n = new Set(groups.filter((g) => g.harness && new RegExp(h, 'i').test(g.harness)).map((g) => g.sessionId)).size;
    if (n > 1) f.push(`hits in ${n} ${h} sessions; the probe used one, so at least ${n - 1} is outside the probe session`);
  }
  return { groups, findings: f };
}

function sessionLines(groups) {
  if (!groups.length) return ['  - no hit, so no session'];
  return groups.map((g) => `  - harness ${val(g.harness)}, session ${val(g.sessionId)}: ${g.lines} line(s), markers ${list(g.markerIds)}, actions ${list(g.actions)}, fake token unredacted: ${g.tokenVerbatim ? 'yes' : 'no'}`);
}

function b2b3b4(probe) {
  const rec = probe.record;
  const scan = rec.scan;
  if (!scan) return ['B2', 'B3', 'B4'].map((id) => step(id, RESULTS.NOT_RUN, 'the probe run PASSed but its L3 record carries no runtime-log scan'));
  const common = scanFileLines(scan);
  const commonF = scanFindings(scan);
  const ps = rec.probeSessions ?? null;

  const c = markerLines(scan, 'claude-channel');
  const cs = sessionFindings(scan, ['claude-channel'], ps);
  const p2 = pollLines(rec, 'B2', 'claude', ['claude-channel']);
  const f2 = [...commonF, ...cs.findings, ...p2.findings];
  if (c.token) f2.push('the claude-channel fake token appears unredacted in Beacon\'s log');
  const notScanned = (ids) => `delivery path ${ids.join(', ')} not in the scan summary: not scanned`;
  const snap2 = snapshotLine(rec, 'B2', 'cumulative');
  const s2 = step('B2', c.missing ? RESULTS.NOT_RUN : f2.length ? RESULTS.FINDING : RESULTS.PASS, c.missing ? notScanned(['claude-channel']) : `claude-channel: ${c.b.lines} hit line(s) in Beacon's log${c.zero ? ' (zero hits is a result)' : ''}`, [
    'Local log (OTLP/hook path), per delivery path (whole probe):',
    ...common,
    ...c.lines,
    ...(snap2 ? [snap2] : []),
    ...p2.lines,
    ...sessionFileLines(rec),
    'Hits by session:',
    ...sessionLines(cs.groups),
  ], f2);

  // B3: the reply-tool call is recorded from the channel server's wire (steps.B3), and B3's
  // own log lines are the delta after the B2 snapshot (steps.B3.log.delta).
  const b3 = rec.steps?.B3 ?? null;
  const delta = b3?.log?.delta?.counts?.['claude-channel'] ?? null;
  // Tool-invocation actions come from the WHOLE-RUN scan: the B3 delta snapshot can close on a
  // late B2 line before the tool-invocation line lands (#195 re-review).
  const wholeRun = scan.counts?.['claude-channel']?.byAction ? Object.keys(scan.counts['claude-channel'].byAction) : (c.b?.actions ?? []);
  const tool = wholeRun.filter((a) => TOOL_INVOKED_ACTIONS.includes(a));
  const toolInDelta = delta ? Object.keys(delta.byAction ?? {}).filter((a) => TOOL_INVOKED_ACTIONS.includes(a)) : null;
  const p3 = pollLines(rec, 'B3', 'claude', ['claude-channel']);
  const f3 = [...commonF, ...cs.findings, ...p3.findings];
  if (c.token) f3.push('the claude-channel fake token appears unredacted in Beacon\'s log (B2 and B3 share the marker)');
  const calls = typeof b3?.replyToolCalls === 'number' ? b3.replyToolCalls : null;
  // L1 §11 item 1 point 2 is CONFIRMED from source (outbound MCP tool arguments reach
  // runtime.jsonl); no tool-invocation capture contradicts it only if the reply was made.
  let summary3;
  if (c.missing) summary3 = notScanned(['claude-channel']);
  else if (calls === 0) {
    f3.push('the reply tool was not invoked (0 reply-tool calls on the channel-server wire), so L1 §11 item 1 point 2 was not exercised live');
    summary3 = 'the reply tool was not invoked; no outbound capture to check';
  } else if (tool.length) summary3 = `tool-invocation capture seen for the claude-channel marker (${tool.join(', ')})`;
  else if (calls !== null) {
    const NO_TOOL = `the reply tool was invoked (${calls} call(s)) but no tool-invocation capture of the marker was logged: contradicts L1 §11 item 1 point 2`;
    f3.push(NO_TOOL);
    summary3 = NO_TOOL;
  } else {
    const NO_TOOL = 'no tool-invocation capture of the marker: contradicts L1 §11 item 1 point 2 unless the reply tool was not invoked; this record (from before L3b #195) does not say whether it was';
    f3.push(NO_TOOL);
    summary3 = `${NO_TOOL} (zero hits is a result, but not a PASS)`;
  }
  const snap3 = snapshotLine(rec, 'B3', 'delta');
  const s3 = step('B3', c.missing ? RESULTS.NOT_RUN : f3.length ? RESULTS.FINDING : RESULTS.PASS, summary3, [
    calls === null ? 'Reply tool on the channel-server wire: not in this record' : `Reply tool on the channel-server wire: ${calls} call(s); its arguments carried the marker: ${b3.replyArgsCarryMarker ? 'yes' : 'no'}; the fake token: ${b3.replyArgsCarryToken ? 'yes' : 'no'} (checked in-process; no value recorded)`,
    `Tool-invocation actions among the claude-channel hits (whole probe run): ${list(tool)}${toolInDelta ? `; within the B3 snapshot: ${list(toolInDelta)}` : ''}`,
    snap3 ?? 'B3 shares B2\'s marker, and this record (from before L3b #195) does not split B2 from B3.',
    ...p3.lines,
  ], f3);

  const x = markerLines(scan, 'codex-turn-start');
  const q = markerLines(scan, 'codex-queue-add');
  const xs = sessionFindings(scan, ['codex-turn-start', 'codex-queue-add'], ps);
  const p4 = pollLines(rec, 'B4', 'codex', ['codex-turn-start', 'codex-queue-add']);
  const f4 = [...commonF, ...xs.findings, ...p4.findings];
  if (x.token) f4.push('the codex-turn-start fake token appears unredacted in Beacon\'s log');
  if (q.token) f4.push('the codex-queue-add fake token appears unredacted in Beacon\'s log');
  const missing4 = [['codex-turn-start', x], ['codex-queue-add', q]].filter(([, m]) => m.missing).map(([id]) => id);
  const s4 = step('B4', missing4.length ? RESULTS.NOT_RUN : f4.length ? RESULTS.FINDING : RESULTS.PASS, missing4.length ? notScanned(missing4) : `turn/start: ${x.b.lines} hit line(s); thread/queue/add: ${q.b.lines} hit line(s)${x.zero && q.zero ? ' (zero hits is a result)' : ''}`, [
    'Local log (OTLP path), per method:',
    ...x.lines,
    ...q.lines,
    ...p4.lines,
    'Hits by session:',
    ...sessionLines(xs.groups),
  ], f4);
  return [s2, s3, s4];
}

function b7(verify, baseline) {
  const d = configDiff(baseline.record.config?.hashes ?? [], verify.record.config?.hashes ?? [], null);
  const notRestored = d.cmp.files.filter((f) => f.status !== 'unchanged' && f.status !== 'absent');
  const findings = notRestored.map((f) => `${label(f.label)} could not be restored to its B0 hash (${f.status})`);
  const rc = verify.record.config?.compare;
  if (rc && rc.unchanged !== d.cmp.unchanged) findings.push('the verify record\'s own comparison disagrees with the hashes it carries; the hashes were used');
  return step('B7', findings.length ? RESULTS.FINDING : RESULTS.PASS, notRestored.length ? `${notRestored.length} file(s) differ from B0` : 'every config file matches its B0 hash', ['Baseline vs. verify, per file:', ...d.lines], findings);
}

/** Parse --note B5=<PASS|FINDING|NOT RUN>: <text>. */
export function parseNote(kv) {
  const m = /^(B5|B6)=(PASS|FINDING|NOT RUN):\s*(\S[\s\S]*)$/.exec(String(kv));
  if (!m) throw new L3ReportError('--note takes B5=<PASS|FINDING|NOT RUN>: <text> or B6=... (operator notes exist for B5 and B6 only)');
  return { id: m[1], result: m[2], text: m[3].trim() };
}

// Versions that should agree across phases, compared only where recorded: the baseline has no
// Codex daemon/wire version (read in the probe), and the verify phase runs no Beacon command.
const VERSION_FIELDS = Object.freeze([['Beacon', (v) => v.beacon], ['`claude --version`', (v) => v.claudeCli], ['Codex CLI', (v) => v.codex?.cli]]);
function versionDrift(recs) {
  const out = [];
  for (const [name, get] of VERSION_FIELDS) {
    const seen = new Set(recs.map((r) => (r?.versions ? get(r.versions) : null)).filter((x) => x !== null && x !== undefined).map(String));
    if (seen.size > 1) out.push(name);
  }
  return out;
}

/**
 * @param {{ baseline: {manifest}, probe: {manifest}, verify?: {manifest}|null, priors?: Array<{manifest}>, notes?: object }} runs
 */
export function evaluateL3({ baseline, probe, verify = null, priors = [], notes = {} }) {
  const runs = { baseline, probe, verify };
  for (const p of PHASES) if (runs[p]) runs[p] = { manifest: runs[p].manifest, record: phaseRecord(runs[p].manifest, p) };
  const box = boxOf(runs.baseline);
  const blocks = Object.fromEntries(PHASES.map((p) => [p, phaseBlock(p, runs[p], box, runs.baseline)]));
  const ok = (p) => blocks[p] === null;

  const steps = [];
  steps.push(ok('baseline') ? b0(runs.baseline) : step('B0', RESULTS.NOT_RUN, blocks.baseline));
  steps.push(b1(ok('probe') && ok('baseline') ? runs.probe : null, runs.baseline));
  if (ok('probe') && ok('baseline')) steps.push(...b2b3b4(runs.probe));
  else for (const id of ['B2', 'B3', 'B4']) steps.push(step(id, RESULTS.NOT_RUN, blocks.probe ?? `the baseline is not evaluable (${blocks.baseline})`));
  for (const id of ['B5', 'B6']) {
    const n = notes[id];
    steps.push(n ? step(id, n.result, `operator note (not driver evidence): ${oneLine(n.text)}`) : step(id, RESULTS.NOT_RUN, B56_NOT_RUN));
  }
  if (ok('verify') && ok('baseline')) steps.push(b7(runs.verify, runs.baseline));
  else {
    // Which harness config files were not restored is safety-relevant, so a verify run that
    // ended PASS keeps its hash comparison as supporting evidence even when B7 is NOT RUN
    // (e.g. after the box expired), as B1 does.
    const evidence = [];
    if (runs.verify?.manifest?.outcome === 'PASS' && runs.verify.record && runs.baseline?.record) {
      const d = configDiff(runs.baseline.record.config?.hashes ?? [], runs.verify.record.config?.hashes ?? [], null);
      evidence.push('Supporting evidence only, not a B7 result: baseline vs. verify, per file:', ...d.lines);
    }
    steps.push(step('B7', RESULTS.NOT_RUN, blocks.verify ?? `the baseline is not evaluable (${blocks.baseline})`, evidence));
  }

  // Header facts.
  const recs = PHASES.map((p) => runs[p]?.record).filter(Boolean);
  const vrec = runs.probe?.record ?? runs.baseline?.record ?? runs.verify?.record ?? null;
  const findings = [...box.notes];
  const drift = versionDrift(recs);
  if (drift.length) findings.push(`the recorded ${drift.join(', ')} version(s) differ between phases`);
  for (const [k, name] of [['claude', 'Claude Code'], ['codex', 'Codex']]) {
    const pin = vrec?.versions?.pins?.[k];
    if (pin?.differs) findings.push(`VERSION WARNING: ${name} installed version differs from PINS.md ${pin.lastTested ? `last tested ${tick(pin.lastTested)}` : `last observed ${tick(pin.lastObserved ?? '?')}`} (a finding, never a stop: ${OPERATOR_DECISION} and #216; the run does not edit PINS.md)`);
  }
  for (const p of PHASES) {
    const r = runs[p];
    if (!r) continue;
    // The scenario writes each of its findings to both the manifest and the record: each is
    // listed once.
    const seen = new Set();
    for (const [src, list0] of [['run', r.manifest.findings ?? []], ['record', r.record?.findings ?? []]]) {
      for (const f of list0) {
        const t = oneLine(f);
        if (seen.has(t)) continue;
        seen.add(t);
        findings.push(`${p} ${src}: ${t}`);
      }
    }
  }
  const drivers = PHASES.filter((p) => runs[p]).map((p) => ({ phase: p, m: runs[p].manifest }));
  const irreproducible = drivers.filter(({ m }) => m.driver?.toolsHerdrDirty !== false || !m.driver?.commit).map(({ phase }) => phase);
  if (irreproducible.length) findings.push(`the ${irreproducible.join(', ')} run(s) cannot be reproduced: tools/herdr/ was not clean and committed (driver.toolsHerdrDirty not false, or no driver commit; scripted-runs.md "Driver identity")`);
  // Who accepted the dialogs, per phase run (#196). `human`: policy exactly `human` (params and,
  // when recorded, the record's own acceptPolicy) and no dialog-accept command. `driver`: policy
  // exactly `driver` in both places; every dialog-accept command is the driver's. Anything else
  // (absent, another value, a human policy holding a dialog-accept command) is `unclear` and
  // reported as such: it is never rendered as a human accept.
  const acceptOf = ({ phase, m }) => {
    const param = m.scenario?.params?.accept;
    const rec = runs[phase]?.record?.acceptPolicy;
    const cmds = (m.commands ?? []).some((c) => c.role === 'dialog-accept');
    if (param === 'human' && (rec === undefined || rec === 'human') && !cmds) return 'human';
    if (param === 'driver' && (rec === undefined || rec === 'driver')) return 'driver';
    return 'unclear';
  };
  const acceptBy = Object.fromEntries(drivers.map((d) => [d.phase, acceptOf(d)]));
  const driverAccepts = drivers.filter((d) => acceptBy[d.phase] === 'driver').map(({ phase }) => phase);
  const unclearAccepts = drivers.filter((d) => acceptBy[d.phase] === 'unclear').map(({ phase }) => phase);
  const dialogsOf = (p) => (Array.isArray(runs[p]?.record?.dialogs) ? runs[p].record.dialogs : []);
  for (const p of driverAccepts) {
    if (dialogsOf(p).length) findings.push(`the ${p} run ran under \`accept=driver\` (#196): Claude Code's and Codex's dialogs were accepted by the DRIVER, not by a human: ${describeDialogs(dialogsOf(p))}`);
  }
  if (unclearAccepts.length) findings.push(`accept origin not established in the ${unclearAccepts.join(', ')} run(s): the accept policy is neither exactly \`human\` nor exactly \`driver\`, or a \`human\` run holds a \`dialog-accept\` command; no dialog accept in it is reported as a human's`);

  // Earlier runs of the same scenario at the same pins that ended NOT RUN or FAIL.
  const bm = runs.baseline?.manifest ?? {};
  const samePins = (m) => JSON.stringify(m.harnessVersions ?? null) === JSON.stringify(bm.harnessVersions ?? null) && (m.herdr?.observedVersionOutput ?? null) === (bm.herdr?.observedVersionOutput ?? null);
  const priorRows = [];
  const priorSkipped = [];
  for (const { manifest: m } of priors) {
    if ((m.scenario?.file ?? null) !== (bm.scenario?.file ?? null)) priorSkipped.push(`${tick(m.runId ?? '?')} (a different scenario)`);
    else if (!samePins(m)) priorSkipped.push(`${tick(m.runId ?? '?')} (different harness or herdr versions)`);
    else if (m.outcome === 'NOT RUN' || m.outcome === 'FAIL') priorRows.push(`earlier run ${tick(m.runId ?? '?')} ended ${m.outcome}${m.outcomeReason ? `: ${oneLine(m.outcomeReason)}` : ''} (No automatic re-submission: a retry is a new run the operator started)`);
    else priorSkipped.push(`${tick(m.runId ?? '?')} (outcome ${tick(m.outcome ?? 'missing')})`);
  }
  findings.push(...priorRows);

  return { runs, box, blocks, steps, findings, priorSkipped, vrec, drivers, irreproducible, driverAccepts, unclearAccepts, acceptBy, dialogsOf };
}

// --- rendering -------------------------------------------------------------------------

function driverLine(m) {
  const d = m.driver ?? {};
  return `herdr (${tick(m.herdr?.observedVersionOutput ?? '?')}, PINS.md \`herdr (test tooling)\` ${oneLine(m.herdr?.pinnedTag ?? '?')}) via \`tools/herdr/run.mjs\`, scenario ${tick(m.scenario?.file ?? '?')}, driver commit ${tick(d.commit ?? '?')}; \`driver.toolsHerdrDirty\`: ${JSON.stringify(d.toolsHerdrDirty ?? null)}`;
}

export function renderL3Draft(ev) {
  const { runs, box, steps, findings, vrec, drivers, irreproducible, acceptBy, dialogsOf } = ev;
  const v = vrec?.versions ?? {};
  const out = [];
  out.push('## 13. Live results (L3)');
  out.push('');
  out.push('> **Draft generated by `tools/herdr/lib/l3-report.mjs` (L3c, #191); reviewed and pasted by L3d (#192).** The runs');
  out.push('> were herdr-driven (`node tools/herdr/run.mjs`, the L3 scenario, one run per phase). Dialog accepts, as each run');
  const originText = { driver: '`accept=driver` (accepted by the driver, #196)', human: '`accept=human` (the driver sent no dialog key)', unclear: 'accept origin NOT established (see Findings)' };
  out.push(`> records them: ${drivers.map(({ phase }) => `${phase} ${originText[acceptBy[phase]]}`).join('; ') || 'no runs'}. This is not a gate result and`);
  out.push('> changes no verdict (Beacon\'s PINS.md row: `Gates affected: none`). Step results are PASS / FINDING / NOT RUN; a phase');
  out.push('> run\'s own outcome is not a step result. The operator attestation below is unticked as generated.');
  if (irreproducible.length) {
    out.push('>');
    out.push(`> **These runs cannot be reproduced:** \`tools/herdr/\` was not clean and committed for the ${irreproducible.join(', ')} run(s).`);
  }
  out.push('');
  out.push(`- **Date:** ${box.start ? box.start.slice(0, 10) : 'not recorded'} (L3 box start)`);
  out.push(`- **Beacon:** ${val(v.beacon)} (L1 §2 pin ${BEACON_PIN})`);
  out.push(`- **Claude Code:** \`claude --version\` ${val(v.claudeCli)}; ${pinsText(v.pins?.claude)}, differs: ${v.pins?.claude?.differs ?? 'not recorded'}`);
  out.push(`- **Codex:** CLI ${val(v.codex?.cli)}; daemon ${val(v.codex?.daemon)}; wire ${val(v.codex?.wire)}; ${pinsText(v.pins?.codex)}, differs: ${v.pins?.codex?.differs ?? 'not recorded'}`);
  for (const { phase, m } of drivers) out.push(`- **Driver (${phase}):** ${driverLine(m)}`);
  out.push(`- **L3 box:** start ${box.start ?? 'not recorded'} (${box.source}); budget ${box.budgetMs} ms (60 minutes, never extended); ends ${box.end ?? 'unknown'}`);
  for (const p of PHASES) {
    const r = runs[p];
    out.push(r
      ? `- **Phase ${p}:** run ${tick(r.manifest.runId ?? '?')}, ${tick(r.manifest.timebox?.start ?? '?')} to ${tick(r.manifest.timebox?.end ?? '?')}; run outcome ${tick(r.manifest.outcome ?? '?')}${r.manifest.outcomeReason ? ` (${oneLine(r.manifest.outcomeReason)})` : ''}`
      : `- **Phase ${p}:** no run supplied`);
  }
  out.push('');
  out.push('### Results, one line per step');
  out.push('');
  for (const s of steps) out.push(`- **${s.id}:** ${s.result} — ${s.summary}`);
  out.push('');
  out.push('### Evidence');
  out.push('');
  for (const s of steps) {
    if (!s.evidence.length && !s.findings.length) continue;
    out.push(`#### ${s.id} (${s.result})`);
    out.push('');
    for (const e of s.evidence) out.push(e.startsWith('  ') ? e : `- ${e}`);
    for (const f of s.findings) out.push(`- Finding: ${f}`);
    out.push('');
  }
  out.push('### Findings');
  out.push('');
  if (!findings.length) out.push('- none recorded');
  for (const f of findings) out.push(`- ${f}`);
  if (ev.priorSkipped.length) out.push(`- (Not listed: ${ev.priorSkipped.join('; ')})`);
  out.push('');
  // Redacted excerpts, as the scenario recorded them (redactedExcerpt(), manifest-safe
  // placeholders). The report holds no log line and no probe value; the leak guard still runs.
  const pr = runs.probe?.record;
  const ex = Array.isArray(pr?.scan?.excerpts) ? pr.scan.excerpts : null;
  if (ex && ex.length) {
    out.push('### Redacted excerpts (from the probe record)');
    out.push('');
    for (const e of ex.slice(0, 40)) out.push(`- ${tick(e.markerId ?? '?')}, ${tick(e.eventAction ?? 'no event.action')}, ${tick(e.file ?? '?')} line ${Number.isInteger(e.line) ? e.line : '?'}: ${tick(e.text ?? '')}`);
    if (ex.length > 40 || pr.scan.excerptsTruncated) out.push(`- (${ex.length > 40 ? `${ex.length - 40} more in the probe record` : 'more hits than excerpts'}; see the probe run manifest)`);
    out.push('');
  }
  // What this probe record does not carry (records from before L3b #195 lack these fields).
  const missing = [];
  if (pr) {
    if (!pr.beacon?.sync) missing.push('`sync --print` poll-path counts (B2, B3, B4)');
    if (!pr.scan?.counts) missing.push('per-action hit counts');
    if (!pr.probeSessions) missing.push('the probe session ids (hits outside them are inferred from harness and session count)');
    if (!pr.sessionFile) missing.push('the Claude session file\'s entry types and flags (B2)');
    if (!ex) missing.push('redacted excerpts');
    if (typeof pr.steps?.B3?.replyToolCalls !== 'number') missing.push('whether the reply tool was invoked (B3)');
  }
  out.push('### Not recorded by the probe record');
  out.push('');
  out.push(!pr ? '- no probe record' : missing.length ? `- ${missing.join('; ')}.` : '- nothing: the probe record carries every field this draft uses.');
  out.push('');
  out.push('### Operator attestation');
  out.push('');
  out.push('Generated unticked. Only the operator who ran this machine ticks these lines, each only if true (`.claude/skills/oac-gates/references/scripted-runs.md` "Operator attestation", adapted to name Beacon).');
  out.push('');
  const herdrV = drivers[0]?.m?.herdr?.observedVersionOutput;
  out.push(`- [ ] **herdr:** the real herdr binary ran, not a test double. \`herdr --version\`: ${tick(herdrV ?? '?')}; sha256 of the executable: \`<64 hex>\``);
  out.push(`- [ ] **Harness:** the real, logged-in Claude Code CLI (\`claude --version\`: ${val(v.claudeCli)}) and Codex CLI (\`codex --version\`: ${val(v.codex?.cli)}) ran, not test doubles.`);
  out.push(`- [ ] **Beacon:** the real, operator-installed Beacon endpoint (\`beacon version\`: ${val(v.beacon)}) ran in Local mode, not a test double.`);
  // #196: the consent line states the probe run's accept origin as recorded; only a human accept
  // is offered for the operator to attest as their own.
  const probeAccept = runs.probe ? acceptBy.probe : null;
  const probeDialogs = dialogsOf('probe');
  out.push(probeAccept === 'human'
    ? '- [ ] **Consent dialog:** Claude Code\'s development-channels dialog was accepted by me, a human at the keyboard, during this run.'
    : probeAccept === 'driver'
      ? `- [ ] **Consent dialog:** accepted by the DRIVER (\`accept=driver\`, #196), not by me: ${describeDialogs(probeDialogs)}.`
      : `- [ ] **Consent dialog:** accept origin not established for the probe run (${runs.probe ? 'see Findings' : 'no probe run'}); not attested as a human accept.`);
  out.push('- **Attested by:** <operator>, <YYYY-MM-DD>');
  // Backstop: every element is exactly one line (its leading indent kept), so no value that
  // slipped past oneLine() at its source can start a line of its own.
  return out.map((l) => {
    const indent = /^ */.exec(l)[0];
    return indent + oneLine(l.slice(indent.length));
  }).join('\n');
}

// --- leak guard ------------------------------------------------------------------------

const unanchored = (re) => re.source.replace(/^\^/, '').replace(/\$$/, '');
// A full probe value's shape, and a prefix followed by 8 or more value characters (a partial
// value). Case-insensitive, so an upper-cased copy is caught too.
const FULL_SHAPES = [
  { kind: 'marker', re: new RegExp(unanchored(MARKER_SHAPE), 'gi') },
  { kind: 'token', re: new RegExp(unanchored(TOKEN_SHAPE), 'gi') },
];
const PARTIAL = [
  { kind: 'marker', re: new RegExp(`${unanchored(MARKER_SHAPE).replace(/\[0-9a-z\]\{26\}$/, '')}[0-9a-z]{8,}`, 'gi') },
  { kind: 'token', re: new RegExp(`${unanchored(TOKEN_SHAPE).replace(/\[0-9a-z\]\{26\}$/, '')}[0-9a-z]{8,}`, 'gi') },
];

/**
 * Throws L3LeakAbort if `text` carries a probe-shaped value, or a residual the redactor scan
 * reports. The message names kinds, ids and line numbers only, never a value.
 * @param {string} text
 * @param {{ markerRecords?: Array<{id, markerSha256, tokenSha256}>, markers?: Array|null, redactor?: object }} [opts]
 */
export function leakGuard(text, { markerRecords = [], markers = null, redactor = createRedactor() } = {}) {
  const problems = [];
  const lineOf = (i) => String(text).slice(0, i).split('\n').length;
  for (const { kind, re } of [...FULL_SHAPES, ...PARTIAL]) {
    for (const m of String(text).matchAll(re)) {
      const h = sha256(m[0]);
      const rec = markerRecords.find((r) => r.markerSha256 === h || r.tokenSha256 === h);
      problems.push(`${kind}-shaped value on line ${lineOf(m.index)}${rec ? ` (the recorded ${rec.id} ${kind})` : ''}`);
    }
  }
  if (markers) {
    try {
      assertNoMarkerLeak(text, markers, 'the draft');
    } catch (err) {
      problems.push(err.message);
    }
  }
  const scan = redactor.scan(neutralizePlaceholders(text));
  for (const h of [...scan.residualLeaks, ...scan.residualGenericHits]) problems.push(`${h.label} on line ${h.line}`);
  if (problems.length) throw new L3LeakAbort(`leak guard: the draft is not printed; ${[...new Set(problems)].join('; ')}`);
  return true;
}

/** Evaluate, render and leak-check. Returns the draft text; throws before returning anything else. */
export function draftL3(runs, { markers = null, redactor } = {}) {
  const ev = evaluateL3(runs);
  const text = renderL3Draft(ev);
  const markerRecords = PHASES.flatMap((p) => ev.runs[p]?.record?.markers ?? []);
  leakGuard(text, { markerRecords, markers, redactor });
  return text;
}

// --- CLI -------------------------------------------------------------------------------

export function parseArgs(argv) {
  const o = { priors: [], notes: {} };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const need = () => {
      const v = argv[++i];
      if (v === undefined) throw new L3ReportError(`${a} needs a value`);
      return v;
    };
    if (a === '--baseline' || a === '--probe' || a === '--verify') {
      const k = a.slice(2);
      if (o[k]) throw new L3ReportError(`${a} given twice`);
      o[k] = need();
    } else if (a === '--prior') o.priors.push(need());
    else if (a === '--note') {
      const n = parseNote(need());
      if (o.notes[n.id]) throw new L3ReportError(`--note ${n.id} given twice`);
      o.notes[n.id] = n;
    } else if (a === '--write') throw new L3ReportError('there is no --write mode: the draft is printed only, and L3d pastes it into §13');
    else throw new L3ReportError(`unknown argument ${a}`);
  }
  if (!o.baseline || !o.probe) throw new L3ReportError('--baseline <run dir> and --probe <run dir> are required');
  return o;
}

export function main(argv, { log = console.log, error = console.error } = {}) {
  try {
    const o = parseArgs(argv);
    const text = draftL3({
      baseline: { manifest: loadRun(o.baseline) },
      probe: { manifest: loadRun(o.probe) },
      verify: o.verify ? { manifest: loadRun(o.verify) } : null,
      priors: o.priors.map((d) => ({ manifest: loadRun(d) })),
      notes: o.notes,
    });
    log(text);
    return 0;
  } catch (err) {
    if (err instanceof L3LeakAbort) {
      error(`l3-report: ${err.message}`);
      return 4;
    }
    if (err instanceof L3ReportError) {
      // Refusal messages carry only safe ids by construction (safeId, safeVersion, safePhase);
      // they also pass the leak guard, and a message that fails it is not printed.
      let msg = oneLine(err.message);
      try {
        leakGuard(msg);
      } catch {
        msg = 'input refused; the refusal message was withheld because it failed the leak guard';
      }
      error(`l3-report: ${msg}\nusage: node tools/herdr/lib/l3-report.mjs --baseline <run dir> --probe <run dir> [--verify <run dir>] [--prior <run dir> ...] [--note B5='<PASS|FINDING|NOT RUN>: <text>'] [--note B6=...]`);
      return 2;
    }
    // Anything else: name the error type only, so no unchecked text reaches the terminal.
    error(`l3-report: internal error (${err?.name ?? 'Error'}); no draft printed`);
    return 2;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
