#!/usr/bin/env node
// Sync docs/planning/backlog/*.json into GitHub labels, milestones, and issues.
// Idempotent: matches existing items by title, creates what is missing, and
// skips what already exists. Run with --dry-run to see the plan without writing.
//
//   node scripts/sync-backlog.mjs --dry-run
//   node scripts/sync-backlog.mjs
//
// Requires: gh CLI, authenticated (`gh auth login`).

import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { writeFileSync, mkdtempSync } from 'node:fs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const backlogDir = join(root, 'docs', 'planning', 'backlog');
const dryRun = process.argv.includes('--dry-run');

const GH = [
  process.env.GH_PATH,
  'C:\\Program Files\\GitHub CLI\\gh.exe',
  'gh',
].find((p) => p && (p === 'gh' || existsSync(p)));

function gh(args, { json = false } = {}) {
  const out = execFileSync(GH, args, { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 });
  return json ? JSON.parse(out) : out.trim();
}

function load(name) {
  return JSON.parse(readFileSync(join(backlogDir, name), 'utf8'));
}

const meta = load('00-meta.json');
const epics = load('01-epics.json');
const tasks = readdirSync(backlogDir)
  .filter((f) => /^0[2-9].*-tasks-.*\.json$/.test(f))
  .sort()
  .flatMap((f) => load(f));

const repo = gh(['repo', 'view', '--json', 'nameWithOwner'], { json: true }).nameWithOwner;
console.log(`repo: ${repo}`);
console.log(`plan: ${meta.labels.length} labels, ${meta.milestones.length} milestones, ${epics.length} epics, ${tasks.length} tasks`);
if (dryRun) console.log('(dry run — nothing will be written)\n');

// ---------- labels ----------
const existingLabels = new Set(
  gh(['label', 'list', '--limit', '200', '--json', 'name'], { json: true }).map((l) => l.name),
);
for (const l of meta.labels) {
  if (existingLabels.has(l.name)) { console.log(`label   = ${l.name}`); continue; }
  console.log(`label   + ${l.name}`);
  if (!dryRun) gh(['label', 'create', l.name, '--color', l.color, '--description', l.description]);
}

// ---------- milestones ----------
const existingMilestones = new Map(
  gh(['api', `repos/${repo}/milestones?state=all&per_page=100`], { json: true }).map((m) => [m.title, m.number]),
);
for (const m of meta.milestones) {
  if (existingMilestones.has(m.title)) { console.log(`milestn = ${m.title}`); continue; }
  console.log(`milestn + ${m.title}`);
  if (!dryRun) {
    const created = gh(['api', `repos/${repo}/milestones`, '-f', `title=${m.title}`, '-f', `description=${m.description}`], { json: true });
    existingMilestones.set(m.title, created.number);
  }
}

// ---------- issues ----------
const existingIssues = new Map(
  gh(['issue', 'list', '--state', 'all', '--limit', '500', '--json', 'number,title'], { json: true })
    .map((i) => [i.title, i.number]),
);

const tmp = mkdtempSync(join(tmpdir(), 'oac-backlog-'));
let bodyFileSeq = 0;
function bodyFile(text) {
  const p = join(tmp, `body-${bodyFileSeq++}.md`);
  writeFileSync(p, text, 'utf8');
  return p;
}

function createIssue({ title, body, labels, milestone }) {
  if (existingIssues.has(title)) {
    console.log(`issue   = #${existingIssues.get(title)} ${title}`);
    return existingIssues.get(title);
  }
  console.log(`issue   + ${title}`);
  if (dryRun) return null;
  const args = ['issue', 'create', '--title', title, '--body-file', bodyFile(body)];
  for (const l of labels || []) args.push('--label', l);
  if (milestone) args.push('--milestone', milestone);
  const url = gh(args);
  const number = Number(url.trim().split('/').pop());
  existingIssues.set(title, number);
  return number;
}

// Pass 1: epics, so tasks can reference their numbers.
const epicNumbers = new Map();
for (const e of epics) {
  const n = createIssue({ title: e.title, body: e.body, labels: e.labels, milestone: e.milestone });
  epicNumbers.set(e.key, n);
}

// Pass 2: tasks, in dependency order so every "depends on" resolves to a real issue number.
const taskNumbers = new Map();
const byKey = new Map(tasks.map((t) => [t.key, t]));
const ordered = [];
const seen = new Map();
(function topo() {
  const visit = (k, stack) => {
    if (seen.get(k) === 'done' || !byKey.has(k)) return;
    if (seen.get(k) === 'open') { console.warn(`cycle at ${[...stack, k].join(' -> ')}`); return; }
    seen.set(k, 'open');
    for (const dep of byKey.get(k).depends || []) visit(dep, [...stack, k]);
    seen.set(k, 'done');
    ordered.push(byKey.get(k));
  };
  for (const t of tasks) visit(t.key, []);
})();

for (const t of ordered) {
  const epicNo = epicNumbers.get(t.epic);
  const deps = (t.depends || [])
    .map((k) => (taskNumbers.has(k) ? `#${taskNumbers.get(k)}` : (byKey.has(k) ? `\`${k}\`` : `\`${k}\``)))
    .join(', ');
  const header = [
    `**Epic:** ${epicNo ? `#${epicNo}` : t.epic}`,
    `**Backlog key:** \`${t.key}\``,
    deps ? `**Depends on:** ${deps}` : null,
  ].filter(Boolean).join('  \n');
  const n = createIssue({
    title: t.title,
    body: `${header}\n\n---\n\n${t.body}`,
    labels: t.labels,
    milestone: t.milestone,
  });
  taskNumbers.set(t.key, n);
}

// Pass 3: append task checklists to each epic.
for (const e of epics) {
  const n = epicNumbers.get(e.key);
  const children = tasks.filter((t) => t.epic === e.key);
  if (!n || !children.length) continue;
  const list = children.map((t) => `- [ ] #${taskNumbers.get(t.key)} — ${t.title}`).join('\n');
  const body = `${e.body}\n\n## Tasks\n\n${list}\n`;
  console.log(`epic    ~ #${n} checklist (${children.length} tasks)`);
  if (!dryRun) gh(['issue', 'edit', String(n), '--body-file', bodyFile(body)]);
}

console.log('\ndone.');
