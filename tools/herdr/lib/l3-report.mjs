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
// finding and does not stop the leg (L3 is not a gate).
//
// Leak guard. Before anything is printed, the whole draft is checked: (1) no probe-marker- or
// fake-token-shaped value, nor a prefix followed by a partial value (the report never holds the
// probe values, only their sha256, so the shape is what it can test; with in-process markers,
// assertNoMarkerLeak runs too); (2) createRedactor().scan() on neutralizePlaceholders(draft)
// reports no residual (home path, username, host, token shape, secret assignment). Any hit
// aborts with exit 4 and prints no draft; the error names kinds and line numbers only.
//
// Exit codes: 0 draft printed; 2 usage error or refused input; 4 leak guard abort.
//
// Node built-ins only.

import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { L3_RECORD_VERSION, MARKER_SHAPE, TOKEN_SHAPE, sha256, compareSections, neutralizePlaceholders, assertNoMarkerLeak } from './l3.mjs';
import { createRedactor } from './redact.mjs';

export class L3ReportError extends Error {}
export class L3LeakAbort extends Error {}

export const PHASES = Object.freeze(['baseline', 'probe', 'verify']);
export const STEPS = Object.freeze(['B0', 'B1', 'B2', 'B3', 'B4', 'B5', 'B6', 'B7']);
export const RESULTS = Object.freeze({ PASS: 'PASS', FINDING: 'FINDING', NOT_RUN: 'NOT RUN' });
// L1 §12 "Timebox: 60 minutes", declared before B0 and never extended.
export const L3_BOX_MS = 60 * 60 * 1000;
// L1 §2: the Beacon version the leg runs against. A Beacon pin move updates this constant.
export const BEACON_PIN = '1.3.29';
// Which phase run carries each step (L3b #190: baseline = B0; probe = B1 record plus B2-B4;
// verify = B7). B5 and B6 are operator steps outside the driver.
export const STEP_PHASE = Object.freeze({ B0: 'baseline', B1: 'probe', B2: 'probe', B3: 'probe', B4: 'probe', B5: null, B6: null, B7: 'verify' });
// B3's outbound capture: the tool-invocation actions L1 §12 B3 names.
export const TOOL_INVOKED_ACTIONS = Object.freeze(['mcp.tool_invoked', 'tool.invoked']);

const OPERATOR_DECISION = 'operator decisions of 2026-09-30 on #168';
const B1_NOT_RUN = `B1 recorded NOT RUN by the ${OPERATOR_DECISION} (item 2): Beacon was installed before any B0 baseline, and it is not uninstalled and re-installed. Evidence is the 2026-09-30 MSI observations (L1 §12 amendment) plus the weaker \`.beacon.bak\` evidence, stated as such`;
const B56_NOT_RUN = `default taken in the ${OPERATOR_DECISION}: B5 and B6 are Beacon steps only the operator may do; no operator note was supplied (--note B5=... / --note B6=...)`;

// --- helpers ---------------------------------------------------------------------------

const tick = (s) => `\`${String(s).replace(/`/g, "'")}\``;
const val = (v) => {
  if (v === null || v === undefined) return 'not recorded';
  if (typeof v === 'object') return Object.entries(v).map(([k, x]) => `${k} ${tick(x ?? 'null')}`).join(', ') || 'not recorded';
  return tick(v);
};
const list = (a) => (a && a.length ? a.map(tick).join(', ') : 'none');
const ms = (t) => (typeof t === 'string' ? Date.parse(t) : NaN);
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
  const id = manifest?.runId ?? '?';
  if (rec === undefined || rec === null) {
    if (manifest?.outcome === 'PASS') throw new L3ReportError(`${phase} run ${id} ended PASS but carries no scenarioData.l3 record; refusing`);
    return null;
  }
  if (rec.version !== L3_RECORD_VERSION) throw new L3ReportError(`${phase} run ${id}: unknown L3 record version ${JSON.stringify(rec.version ?? null)} (this report knows ${L3_RECORD_VERSION}); refusing`);
  if (rec.phase !== phase) throw new L3ReportError(`run ${id} was given as --${phase} but its L3 record is phase ${JSON.stringify(rec.phase ?? null)}; refusing`);
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
    source = 'baseline run manifest `timebox.start` (the baseline carries no L3 record)';
  }
  if (rec?.box && rec.box.budgetMs !== L3_BOX_MS) notes.push(`the baseline record declares box.budgetMs ${rec.box.budgetMs}; L1 §12 fixes the box at ${L3_BOX_MS} ms, which this report applies`);
  if (rec?.box && rec.box.sourceRunId !== null && rec.box.sourceRunId !== undefined) notes.push('the baseline record continues another run\'s box (box.sourceRunId is set); the baseline phase should open the box');
  const s = ms(start);
  return { start, source, budgetMs: L3_BOX_MS, end: isoOrNull(s + L3_BOX_MS), endMs: s + L3_BOX_MS, known: Number.isFinite(s), notes };
}

// Why a phase's steps are NOT RUN, or null when they can be evaluated.
function phaseBlock(phase, run, box, baseline) {
  if (!run) return `no ${phase} run was supplied (--${phase})`;
  const m = run.manifest;
  if (m.outcome !== 'PASS') return `the ${phase} run ${m.runId ?? '?'} ended ${m.outcome ?? 'without an outcome'}${m.outcomeReason ? `: ${m.outcomeReason}` : ''}`;
  if (!box.known) return 'the L3 box start is not recorded, so the box cannot be checked';
  const end = ms(m.timebox?.end);
  if (!Number.isFinite(end)) return `the ${phase} run's end is not recorded, so the L3 box cannot be checked`;
  if (end > box.endMs) return `L3 box expired: the ${phase} run ended ${m.timebox.end}, after the box ended ${box.end}`;
  if (phase !== 'baseline') {
    const b = run.record?.box;
    if (!b || b.start !== box.start || b.sourceRunId !== (baseline?.manifest?.runId ?? null)) {
      return `the ${phase} record does not continue the baseline's L3 box (box.start ${b?.start ?? 'missing'}, box.sourceRunId ${b?.sourceRunId ?? 'missing'})`;
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
  else if (!String(beacon).includes(BEACON_PIN)) findings.push(`Beacon version differs from the L1 §2 pin ${BEACON_PIN}`);
  for (const [k, name] of [['claude', 'Claude Code'], ['codex', 'Codex']]) if (v.pins?.[k]?.differs) findings.push(`${name} differs from its PINS.md last-observed value (pin drift, recorded as a finding; L3 is not a gate)`);
  for (const f of r.config?.findings ?? []) findings.push(f);
  const set = (prefix) => (hashes.some((h) => String(h.label).startsWith(prefix)) ? 'set' : 'not set');
  const evidence = [
    `Versions: Beacon ${val(beacon)}; \`claude --version\` ${val(v.claudeCli)}; Codex CLI ${val(v.codex?.cli)}, daemon ${val(v.codex?.daemon)}, wire ${val(v.codex?.wire)}`,
    `Environment (whether set only): CLAUDE_CONFIG_DIR ${set('$CLAUDE_CONFIG_DIR/')}, CODEX_HOME ${set('$CODEX_HOME/')}`,
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

function markerLines(scan, id) {
  const b = scan.byMarker?.[id];
  if (!b) return { lines: [`  - ${id}: not in the scan summary`], token: false, zero: true, b: null };
  return {
    b,
    token: b.tokenVerbatimLines > 0,
    zero: b.lines === 0,
    lines: [
      `  - ${id}: ${b.lines} hit line(s)${b.lines === 0 ? ' (zero hits: a result, not NOT RUN)' : ''}; marker in ${b.markerLines}; fake token unredacted in ${b.tokenVerbatimLines}`,
      `    event.action ${list(b.actions)}; collection method ${list(b.collectionMethods)}; JSON path ${list(b.paths)}`,
      '    per-action hit counts: not in the L3 record v1 scan summary (it lists the actions seen, not a count per action)',
    ],
  };
}

const HARNESS_OF = { 'claude-channel': /claude/i, 'codex-turn-start': /codex/i, 'codex-queue-add': /codex/i };

function sessionFindings(scan, ids) {
  const groups = (scan.bySession ?? []).filter((g) => g.markerIds.some((m) => ids.includes(m)));
  const f = [];
  for (const g of groups) {
    if (g.sessionId === null || g.harness === null) f.push(`${g.lines} hit line(s) carry no harness or session id: outside any identifiable probe session`);
    const wrong = g.markerIds.filter((m) => ids.includes(m) && HARNESS_OF[m] && g.harness !== null && !HARNESS_OF[m].test(g.harness));
    if (wrong.length) f.push(`a ${wrong.join(', ')} hit is in a ${tick(g.harness)} session: outside the probe session for that path`);
  }
  for (const h of ['claude', 'codex']) {
    const n = new Set(groups.filter((g) => g.harness && new RegExp(h, 'i').test(g.harness)).map((g) => g.sessionId)).size;
    if (n > 1) f.push(`hits in ${n} ${h} sessions; the probe used one, so at least ${n - 1} is outside the probe session`);
  }
  return { groups, findings: f };
}

function sessionLines(groups) {
  if (!groups.length) return ['  - no hit, so no session'];
  return groups.map((g) => `  - harness ${val(g.harness)}, session ${val(g.sessionId)}: ${g.lines} line(s), markers ${list(g.markerIds)}, actions ${list(g.actions)}, fake token unredacted: ${g.tokenVerbatim ? 'yes' : 'no'}`);
}

const POLL_NOT_RUN = (h) => `Poll path (\`beacon endpoint ${h} sync --print\`): NOT RUN (the L3 record v1 has no field for its counts)`;

function b2b3b4(probe) {
  const scan = probe.record.scan;
  if (!scan) return ['B2', 'B3', 'B4'].map((id) => step(id, RESULTS.NOT_RUN, 'the probe run PASSed but its L3 record carries no runtime-log scan'));
  const common = scanFileLines(scan);
  const commonF = scanFindings(scan);

  const c = markerLines(scan, 'claude-channel');
  const cs = sessionFindings(scan, ['claude-channel']);
  const f2 = [...commonF, ...cs.findings];
  if (c.token) f2.push('the claude-channel fake token appears unredacted in Beacon\'s log');
  const s2 = step('B2', f2.length ? RESULTS.FINDING : RESULTS.PASS, c.b ? `claude-channel: ${c.b.lines} hit line(s) in Beacon's log${c.zero ? ' (zero hits is a result)' : ''}` : 'claude-channel: no scan entry', [
    'Local log (OTLP/hook path), per delivery path:',
    ...common,
    ...c.lines,
    POLL_NOT_RUN('claude'),
    'Claude session file (entry type and flags): not in the L3 record v1',
    'Hits by session:',
    ...sessionLines(cs.groups),
  ], f2);

  const tool = (c.b?.actions ?? []).filter((a) => TOOL_INVOKED_ACTIONS.includes(a));
  const f3 = [...commonF, ...cs.findings];
  if (c.token) f3.push('the claude-channel fake token appears unredacted in Beacon\'s log (B2 and B3 share the marker)');
  const s3 = step('B3', f3.length ? RESULTS.FINDING : RESULTS.PASS, tool.length ? `tool-invocation capture seen for the claude-channel marker (${tool.join(', ')})` : 'no tool-invocation hit for the claude-channel marker (zero is a result)', [
    `Tool-invocation actions among the claude-channel hits: ${list(tool)}`,
    'B3 shares B2\'s marker; the L3 record v1 does not split its paths or token lines per action.',
  ], f3);

  const x = markerLines(scan, 'codex-turn-start');
  const q = markerLines(scan, 'codex-queue-add');
  const xs = sessionFindings(scan, ['codex-turn-start', 'codex-queue-add']);
  const f4 = [...commonF, ...xs.findings];
  if (x.token) f4.push('the codex-turn-start fake token appears unredacted in Beacon\'s log');
  if (q.token) f4.push('the codex-queue-add fake token appears unredacted in Beacon\'s log');
  const s4 = step('B4', f4.length ? RESULTS.FINDING : RESULTS.PASS, `turn/start: ${x.b?.lines ?? '?'} hit line(s); thread/queue/add: ${q.b?.lines ?? '?'} hit line(s)${x.zero && q.zero ? ' (zero hits is a result)' : ''}`, [
    'Local log (OTLP path), per method:',
    ...x.lines,
    ...q.lines,
    POLL_NOT_RUN('codex'),
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

function versionsSig(rec) {
  const v = rec?.versions;
  return v ? JSON.stringify([v.beacon ?? null, v.claudeCli ?? null, v.codex ?? null]) : null;
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
    steps.push(n ? step(id, n.result, `operator note (not driver evidence): ${n.text}`) : step(id, RESULTS.NOT_RUN, B56_NOT_RUN));
  }
  if (ok('verify') && ok('baseline')) steps.push(b7(runs.verify, runs.baseline));
  else steps.push(step('B7', RESULTS.NOT_RUN, blocks.verify ?? `the baseline is not evaluable (${blocks.baseline})`));

  // Header facts.
  const recs = PHASES.map((p) => runs[p]?.record).filter(Boolean);
  const vrec = runs.probe?.record ?? runs.baseline?.record ?? runs.verify?.record ?? null;
  const findings = [...box.notes];
  if (new Set(recs.map(versionsSig).filter(Boolean)).size > 1) findings.push('the recorded Beacon / Claude Code / Codex versions differ between phases');
  for (const [k, name] of [['claude', 'Claude Code'], ['codex', 'Codex']]) {
    const pin = vrec?.versions?.pins?.[k];
    if (pin?.differs) findings.push(`pin drift: ${name} installed version differs from PINS.md last-observed ${tick(pin.lastObserved ?? '?')} (recorded as a finding by the ${OPERATOR_DECISION}; PINS.md is not moved for L3)`);
  }
  for (const p of PHASES) {
    const r = runs[p];
    if (!r) continue;
    for (const f of r.manifest.findings ?? []) findings.push(`${p} run: ${f}`);
    for (const f of r.record?.findings ?? []) findings.push(`${p} record: ${f}`);
  }
  const drivers = PHASES.filter((p) => runs[p]).map((p) => ({ phase: p, m: runs[p].manifest }));
  const irreproducible = drivers.filter(({ m }) => m.driver?.toolsHerdrDirty !== false || !m.driver?.commit).map(({ phase }) => phase);
  if (irreproducible.length) findings.push(`the ${irreproducible.join(', ')} run(s) cannot be reproduced: tools/herdr/ was not clean and committed (driver.toolsHerdrDirty not false, or no driver commit; scripted-runs.md "Driver identity")`);
  const driverAccepts = drivers.filter(({ m }) => (m.commands ?? []).some((c) => c.role === 'dialog-accept') || /^driver$/i.test(String(m.scenario?.params?.accept ?? ''))).map(({ phase }) => phase);
  if (driverAccepts.length) findings.push(`driver-sent dialog accepts in the ${driverAccepts.join(', ')} run(s): not accept=human; never verdict-bearing (scripted-runs.md "Operator-consent dialogs")`);

  // Earlier runs of the same scenario at the same pins that ended NOT RUN or FAIL.
  const bm = runs.baseline?.manifest ?? {};
  const samePins = (m) => JSON.stringify(m.harnessVersions ?? null) === JSON.stringify(bm.harnessVersions ?? null) && (m.herdr?.observedVersionOutput ?? null) === (bm.herdr?.observedVersionOutput ?? null);
  const priorRows = [];
  const priorSkipped = [];
  for (const { manifest: m } of priors) {
    if ((m.scenario?.file ?? null) !== (bm.scenario?.file ?? null)) priorSkipped.push(`${m.runId ?? '?'} (a different scenario)`);
    else if (!samePins(m)) priorSkipped.push(`${m.runId ?? '?'} (different harness or herdr versions)`);
    else if (m.outcome === 'NOT RUN' || m.outcome === 'FAIL') priorRows.push(`earlier run ${tick(m.runId ?? '?')} ended ${m.outcome}${m.outcomeReason ? `: ${m.outcomeReason}` : ''} (No automatic re-submission: a retry is a new run the operator started)`);
    else priorSkipped.push(`${m.runId ?? '?'} (outcome ${m.outcome ?? 'missing'})`);
  }
  findings.push(...priorRows);

  return { runs, box, blocks, steps, findings, priorSkipped, vrec, drivers, irreproducible, driverAccepts };
}

// --- rendering -------------------------------------------------------------------------

function driverLine(m) {
  const d = m.driver ?? {};
  return `herdr (${tick(m.herdr?.observedVersionOutput ?? '?')}, PINS.md \`herdr (test tooling)\` ${m.herdr?.pinnedTag ?? '?'}) via \`tools/herdr/run.mjs\`, scenario ${tick(m.scenario?.file ?? '?')}, driver commit ${tick(d.commit ?? '?')}; \`driver.toolsHerdrDirty\`: ${JSON.stringify(d.toolsHerdrDirty ?? null)}`;
}

export function renderL3Draft(ev) {
  const { runs, box, steps, findings, vrec, drivers, irreproducible, driverAccepts } = ev;
  const v = vrec?.versions ?? {};
  const out = [];
  out.push('## 13. Live results (L3)');
  out.push('');
  out.push('> **Draft generated by `tools/herdr/lib/l3-report.mjs` (L3c, #191); reviewed and pasted by L3d (#192).** The runs');
  out.push('> were herdr-driven (`node tools/herdr/run.mjs`, the L3 scenario, one run per phase). Consent-dialog accepts were');
  out.push(`> \`accept=human\`${driverAccepts.length ? ` EXCEPT in the ${driverAccepts.join(', ')} run(s), which hold driver-sent accepts (see Findings)` : ': no phase run holds a `dialog-accept` command'}. This is not a gate result and`);
  out.push('> changes no verdict (Beacon\'s PINS.md row: `Gates affected: none`). Step results are PASS / FINDING / NOT RUN; a phase');
  out.push('> run\'s own outcome is not a step result. The operator attestation below is unticked as generated.');
  if (irreproducible.length) {
    out.push('>');
    out.push(`> **These runs cannot be reproduced:** \`tools/herdr/\` was not clean and committed for the ${irreproducible.join(', ')} run(s).`);
  }
  out.push('');
  out.push(`- **Date:** ${box.start ? box.start.slice(0, 10) : 'not recorded'} (L3 box start)`);
  out.push(`- **Beacon:** ${val(v.beacon)} (L1 §2 pin ${BEACON_PIN})`);
  out.push(`- **Claude Code:** \`claude --version\` ${val(v.claudeCli)}; PINS.md last observed ${val(v.pins?.claude?.lastObserved)}, differs: ${v.pins?.claude?.differs ?? 'not recorded'}`);
  out.push(`- **Codex:** CLI ${val(v.codex?.cli)}; daemon ${val(v.codex?.daemon)}; wire ${val(v.codex?.wire)}; PINS.md last observed ${val(v.pins?.codex?.lastObserved)}, differs: ${v.pins?.codex?.differs ?? 'not recorded'}`);
  for (const { phase, m } of drivers) out.push(`- **Driver (${phase}):** ${driverLine(m)}`);
  out.push(`- **L3 box:** start ${box.start ?? 'not recorded'} (${box.source}); budget ${box.budgetMs} ms (60 minutes, never extended); ends ${box.end ?? 'unknown'}`);
  for (const p of PHASES) {
    const r = runs[p];
    out.push(r
      ? `- **Phase ${p}:** run ${tick(r.manifest.runId ?? '?')}, ${r.manifest.timebox?.start ?? '?'} to ${r.manifest.timebox?.end ?? '?'}; run outcome ${r.manifest.outcome ?? '?'}${r.manifest.outcomeReason ? ` (${r.manifest.outcomeReason})` : ''}`
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
  out.push('### Not recorded by the L3 record v1');
  out.push('');
  out.push('- `sync --print` poll-path counts (B2, B4); per-action hit counts; the probe session ids (hits outside them are inferred from harness and session count); the Claude session file\'s entry types and flags (B2); redacted excerpts (the report holds no log line and no probe value).');
  out.push('');
  out.push('### Operator attestation');
  out.push('');
  out.push('Generated unticked. Only the operator who ran this machine ticks these lines, each only if true (`.claude/skills/oac-gates/references/scripted-runs.md` "Operator attestation", adapted to name Beacon).');
  out.push('');
  const herdrV = drivers[0]?.m?.herdr?.observedVersionOutput;
  out.push(`- [ ] **herdr:** the real herdr binary ran, not a test double. \`herdr --version\`: ${tick(herdrV ?? '?')}; sha256 of the executable: \`<64 hex>\``);
  out.push(`- [ ] **Harness:** the real, logged-in Claude Code CLI (\`claude --version\`: ${val(v.claudeCli)}) and Codex CLI (\`codex --version\`: ${val(v.codex?.cli)}) ran, not test doubles.`);
  out.push(`- [ ] **Beacon:** the real, operator-installed Beacon endpoint (\`beacon version\`: ${val(v.beacon)}) ran in Local mode, not a test double.`);
  out.push('- [ ] **Consent dialog:** Claude Code\'s development-channels dialog was accepted by me, a human at the keyboard, during this run.');
  out.push('- **Attested by:** <operator>, <YYYY-MM-DD>');
  return out.join('\n');
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
      error(`l3-report: ${err.message}\nusage: node tools/herdr/lib/l3-report.mjs --baseline <run dir> --probe <run dir> [--verify <run dir>] [--prior <run dir> ...] [--note B5='<PASS|FINDING|NOT RUN>: <text>'] [--note B6=...]`);
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
