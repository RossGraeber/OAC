// Shared pieces of the K8 (#131) report generators, lib/g4-report.mjs and lib/g5-report.mjs:
// score vocabulary, check rows, CLI argument parsing, the never-overwrite writer, the unticked
// operator attestation, and the reconstruction callout every G4/G5 comparison carries.

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

// The attestation block, generated unticked (oac-gates references/scripted-runs.md).
export function attestation({ herdrVersion, harnesses, consent }) {
  return [
    '## Operator attestation',
    '',
    'Generated unticked. Only the operator who ran this machine ticks these lines, each only if true (`.claude/skills/oac-gates/references/scripted-runs.md` "Operator attestation"). An unticked line means this record is neither an equivalence record nor verdict-bearing.',
    '',
    `- [ ] **herdr:** the real herdr binary ran, not a test double. \`herdr --version\`: \`${herdrVersion ?? '?'}\`; sha256 of the executable: \`<64 hex>\``,
    `- [ ] **Harness:** the real, logged-in ${harnesses} ran, not test doubles.`,
    `- [ ] **Consent dialog:** ${consent}`,
    '- **Attested by:** <operator>, <YYYY-MM-DD>',
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
