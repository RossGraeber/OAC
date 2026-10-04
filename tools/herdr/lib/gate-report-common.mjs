// Shared pieces of the K8 (#131) report generators, lib/g4-report.mjs and lib/g5-report.mjs:
// score vocabulary, check rows, CLI argument parsing, the never-overwrite writer, the
// verification block (#252), and the reconstruction callout every G4/G5 comparison carries.

import { copyFileSync, existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';

export const SCORES = Object.freeze({ EQ: 'equivalent', NEQ: 'not equivalent', NE: 'not evaluable' });
export class ReportError extends Error {}

export const check = (name, ok, detail = null) => ({ name, ok: !!ok, detail });
export const required = (row) => row.checks.filter((c) => !c.name.startsWith('(supporting'));
export const failed = (row) => required(row).filter((c) => !c.ok).map((c) => c.name).join('; ');
export const allRequired = (row) => required(row).every((c) => c.ok);
export const cell = (s) => String(s ?? '').replace(/\|/g, '\\|').replace(/\n/g, ' ');

// Operator scores, only for the rows that take one; each needs a note.
export function parseOperatorScores(pairs, allowed, why) {
  const out = {};
  for (const { n, score, note } of pairs) {
    if (!allowed.includes(n)) throw new ReportError(`criterion ${n} ${why}; operator scores are taken only for ${allowed.join(', ')}`);
    const s = score === 'not-equivalent' ? SCORES.NEQ : score;
    if (![SCORES.EQ, SCORES.NEQ].includes(s)) throw new ReportError(`criterion ${n}: score must be equivalent or not-equivalent`);
    if (!note || !String(note).trim()) throw new ReportError(`criterion ${n}: an operator score needs a --note saying what in the pane text or wire supports it`);
    out[n] = { score: s, note: String(note).trim() };
  }
  return out;
}

// Apply an operator score to a row whose mechanical preconditions are all met.
export function operatorRow(row, op, pendingWhy) {
  if (!allRequired(row)) {
    row.score = SCORES.NE;
    row.reason = `mechanical preconditions not met (${failed(row)})${op ? '; the operator score was not applied' : ''}`;
  } else if (!op) {
    row.score = SCORES.NE;
    row.reason = pendingWhy;
  } else {
    row.score = op.score;
    row.operator = op;
    row.reason = `operator: ${op.note}`;
  }
}

export function mechanicalRow(row, okWhy) {
  row.score = allRequired(row) ? SCORES.EQ : SCORES.NEQ;
  row.reason = row.score === SCORES.EQ ? okWhy : `not met: ${failed(row)}`;
}

// --score/--note <key>=<value>, --case <key>=<value> (per-case results), --run, --write,
// --root, --baseline.
export function parseReportArgs(argv, { keyRe = /^\d+$/, caseRe = null } = {}) {
  const o = { scores: [], notes: {}, cases: [], write: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--run') o.run = argv[++i];
    else if (a === '--baseline') o.baseline = argv[++i];
    else if (a === '--write') o.write = true;
    else if (a === '--root') o.root = argv[++i];
    else if (a === '--score' || a === '--note') {
      const kv = argv[++i] ?? '';
      const eq = kv.indexOf('=');
      const n = kv.slice(0, eq);
      if (eq < 1 || !keyRe.test(n)) throw new ReportError(`${a} takes <criterion>=<value>`);
      if (a === '--score') o.scores.push({ n, score: kv.slice(eq + 1) });
      else o.notes[n] = kv.slice(eq + 1);
    } else if (a === '--case' && caseRe) {
      const kv = argv[++i] ?? '';
      const eq = kv.indexOf('=');
      if (eq < 1 || !caseRe.test(kv.slice(0, eq))) throw new ReportError(`--case takes <case>.<criterion>=x|f, e.g. X2.c2=f`);
      o.cases.push({ key: kv.slice(0, eq), value: kv.slice(eq + 1) });
    } else throw new ReportError(`unknown argument ${a}`);
  }
  if (!o.run) throw new ReportError('--run <run dir> is required');
  return o;
}

// Write the report and copy the run's files; never overwrite anything.
export function writeTargets(targets, reportText) {
  const exists = targets.filter(([t]) => existsSync(t)).map(([t]) => t);
  if (exists.length) throw new ReportError(`refusing to overwrite: ${exists.join(', ')}`);
  for (const [t] of targets) mkdirSync(dirname(t), { recursive: true });
  writeFileSync(targets[0][0], `${reportText}\n`);
  for (const [to, from] of targets.slice(1)) copyFileSync(from, to);
}

// #196: who accepted each dialog, as the record says, never inferred beyond it. A driver
// accept names its keys and herdr command seqs; a human accept says what it rests on (the
// driver sent nothing and the screen changed), because nothing observes a human keystroke.
export function describeDialog(d) {
  const who = d.agent ? `${d.agent} ` : '';
  let how;
  if (d.acceptOrigin === 'driver') {
    const keys = (d.acceptKeys ?? []).map((k) => `${k.key} #${k.seq}`).join(', ') || `enter #${d.acceptSeq ?? '?'}`;
    how = `accepted by the DRIVER (herdr dialog-accept: ${keys})`;
  } else if (d.acceptOrigin === 'human') {
    how = 'accepted outside the driver (recorded as human: the driver sent no keystroke and the screen changed)';
  } else how = `not accepted (${d.acceptOrigin ?? 'no accept recorded'})`;
  // #267: a recorded variant (the multi-select MCP form) names the servers it listed.
  const variant = d.variant ? `; ${d.variant} form listing ${JSON.stringify(d.listedServers ?? [])}, expected ${JSON.stringify(d.expectedServers ?? null)}` : '';
  return `${who}${d.kind} (read #${d.readSeq ?? '?'}${variant}; ${how})`;
}

export const describeDialogs = (dialogs) => (dialogs ?? []).map(describeDialog).join('; ') || 'none';

// --- Verification (#252) -------------------------------------------------------------------
//
// Operator decision on #252 (2026-10-03): a record's findings rest on verification from the
// evidence, with citations, or are UNVERIFIED; the operator's attestation is not the basis.
// The generator derives each line from the run manifest and cites the field; the recording
// agent re-checks every citation and fills the `<TO FILL: ...>` slots, which
// scripts/check-fixture-manifest.mjs refuses to see left in a record that claims eligibility.
// A person signs off only on the human actions: steps the agent could not do (a consent step a
// criterion names, an interactive sign-in, credentials).
export const TO_FILL = '<TO FILL';

// A consent-free gate's human-actions line: no criterion of it names a consent step.
export const noConsentCriterionLine = (gate) => `none required by a criterion: no criterion of ${gate} names a consent step, and every dialog accept above is the driver's or recorded as human.`;

// The herdr line: the version against the pin, the executable hash against PINS.md's expected
// value for the platform (herdr.executableCheck, #252), native and not the test double, and
// unchanged at teardown. -> { verified, text }.
export function herdrVerification(manifest) {
  const h = manifest?.herdr ?? {};
  const x = h.executable;
  const c = h.executableCheck;
  const why = [];
  if (!h.observedVersionOutput || h.observedVersionOutput !== h.expectedVersionOutput) why.push(`\`herdr --version\` ${JSON.stringify(h.observedVersionOutput ?? null)} is not the pin's ${JSON.stringify(h.expectedVersionOutput ?? null)}`);
  if (!x) why.push('the run manifest records no herdr.executable (schemaVersion 1, before #140)');
  else {
    if (x.testDouble !== false) why.push(`herdr.executable.testDouble is ${JSON.stringify(x.testDouble ?? null)} (the node-run test double)`);
    else if (!['elf', 'pe', 'mach-o'].includes(x.format) || !/^[0-9a-f]{64}$/.test(x.sha256 ?? '')) why.push(`herdr.executable is not a hashed native binary (format ${JSON.stringify(x.format ?? null)})`);
    if (x.unchangedAfterRun === false) why.push('the executable changed during the run (herdr.executable.unchangedAfterRun false)');
  }
  if (!c) why.push('the run manifest records no herdr.executableCheck (a driver before #252): the hash was compared with nothing');
  else if (c.result !== 'match') why.push(`herdr.executableCheck.result is \`${c.result}\` (${c.detail ?? 'no detail'})`);
  else if (c.firstParty !== true) why.push(`the executable matches the locally observed value PINS.md records for \`${c.platform}\` (first-party source UNVERIFIED; \`herdr.executableCheck.firstParty\` ${JSON.stringify(c.firstParty ?? null)})`);
  const hash = /^[0-9a-f]{64}$/.test(x?.sha256 ?? '') ? `\`${x.sha256}\`` : 'none recorded';
  if (why.length) return { verified: false, text: `UNVERIFIED — ${why.join('; ')}. Executable sha256: ${hash}.` };
  return {
    verified: true,
    text: `VERIFIED — \`herdr --version\` \`${h.observedVersionOutput}\` equals the PINS.md pin (\`herdr.observedVersionOutput\`, \`herdr.expectedVersionOutput\`); executable \`${x.basename ?? '?'}\` (${x.format}, not the test double) sha256 ${hash} equals PINS.md's first-party expected sha256 for \`${c.platform}\` (\`herdr.executable.sha256\`, \`herdr.executableCheck\`; basis: ${c.basis ?? 'not recorded'}); unchanged at teardown: ${x.unchangedAfterRun ?? 'not recorded'} (\`herdr.executable.unchangedAfterRun\`).`,
  };
}

// The herdr line for a record built from several runs (L3: one run per phase): VERIFIED only
// when every run's herdr is, each phase cited. -> { verified, text }.
export function herdrVerificationAll(runs) {
  const each = runs.map(({ label, manifest }) => ({ label, ...herdrVerification(manifest) }));
  if (!each.length) return { verified: false, text: 'UNVERIFIED — no run manifest.' };
  const verified = each.every((e) => e.verified);
  return { verified, text: `${verified ? 'VERIFIED' : 'UNVERIFIED'} — ${each.map((e) => `${e.label}: ${e.text}`).join(' ')}` };
}

// The harness line. `verified` is the report's own versionsVerified(): every source it
// checks (CLI, wire, daemon; post-run where the gate records one) reported one version.
// `versions` names those sources and their fields; the executables are cited by hash.
export function harnessVerification(manifest, { verified, versions }) {
  const ex = Object.entries(manifest?.harnessExecutables ?? {}).filter(([, e]) => e && typeof e === 'object');
  const exe = ex.map(([k, e]) => `\`${k}\` → \`${e.basename ?? '?'}\` sha256 ${/^[0-9a-f]{64}$/.test(e.sha256 ?? '') ? `\`${e.sha256}\`` : 'not recorded'} (\`harnessExecutables.${k}\`)`).join(', ') || 'not recorded';
  return verified
    ? `VERIFIED — ${versions}: one and the same version per harness from every source listed (the report's versionsVerified). Executables that answered \`--version\`: ${exe}. Not shown by the record: that the pane ran these files (its shell resolves PATH itself; scripted-runs.md "What the record cannot show").`
    : `UNVERIFIED — ${versions}: the sources did not report one and the same version per harness, or were not recorded (see Findings). Executables that answered \`--version\`: ${exe}.`;
}

// The verification block. `humanActions`: the steps a person did that the agent could not,
// each naming who (a `<TO FILL: ...>` slot where only the person can say).
export function verification({ manifest, harness, dialogs, dialogsField, humanActions, heading = '## Verification', extra = [], herdr = herdrVerification(manifest) }) {
  return [
    heading,
    '',
    'Each line is checked from the evidence it cites (run-manifest.json fields beside this record), or marked UNVERIFIED with the reason (`.claude/skills/oac-gates/references/scripted-runs.md` "Verification"). Generated from the run manifest; the recording agent re-checks every citation and fills each `<TO FILL: ...>` slot. A herdr or Harness line that is UNVERIFIED means this record is neither an equivalence record nor verdict-bearing.',
    '',
    `- **herdr:** ${herdr.text}`,
    `- **Harness:** ${harness}`,
    ...extra,
    `- **Dialogs:** ${describeDialogs(dialogs)} (\`${dialogsField}\`; each driver key is a \`dialog-accept\` command in \`commands\`)`,
    `- **Human actions:** ${humanActions} Sign-ins or credentials a person supplied for this run: ${TO_FILL}: none, or each action and who did it>.`,
    `- **Verified by:** ${TO_FILL}: recording agent>, ${TO_FILL}: YYYY-MM-DD>`,
    '',
  ];
}

export const reconstructionCallout = (gate, files, resultDoc) => [
  `> **Reconstructed gate server${files.length > 1 ? 's' : ''}.** This run did NOT use the programs that produced the human-run ${gate} fixtures: those spike`,
  `> programs were never committed (\`${resultDoc}\`). It ran ${files.map((f) => `\`tools/herdr/gate-servers/${f}\``).join(', ')},`,
  `> rebuilt for K8 (#131) from \`${resultDoc}\`'s architecture and the committed fixtures' wire shapes. They cannot be`,
  '> verified byte-identical, or behavior-identical, to the originals. Every "equivalent" below means "equivalent against a',
  '> reconstruction", and a difference may come from the reconstruction rather than from the harness. Committing these',
  '> servers is a documented exception to `oac-gates`\' "nothing durable is built on spike code" rule.',
];
