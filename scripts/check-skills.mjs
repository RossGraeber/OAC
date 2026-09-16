#!/usr/bin/env node
// Enforce the progressive-disclosure budget from docs/planning/SKILLS-MODEL.md.
//
//   node scripts/check-skills.mjs
//
// Caps: CLAUDE.md 40 lines, skill description 2 lines, SKILL.md body 200 lines,
// references/*.md uncapped. Also checks that every skill in the tree exists, that
// each frontmatter `name` matches its directory, and that no skill is undeclared.
// Exits non-zero on any violation so CI fails loudly.

import { readFileSync, readdirSync, existsSync, statSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const skillsDir = join(root, '.claude', 'skills');

const CAPS = {
  claudeMdLines: 40,
  descriptionLines: 2,
  descriptionChars: 280,
  bodyLines: 200,
};

// The fourteen skills of the tree, per SKILLS-MODEL.md "The skill tree".
const EXPECTED = [
  'oac',
  'oac-boundaries',
  'oac-evidence',
  'oac-planning-package',
  'oac-gates',
  'oac-spec-authoring',
  'oac-implementation',
  'oac-security-work',
  'oac-testing',
  'oac-release',
  'oac-claude-channels',
  'oac-codex-appserver',
  'oac-mcp',
  'oac-zenoh',
  'oac-authoring-skills',
];

const problems = [];
const notes = [];

function fail(where, message) {
  problems.push(`${where}: ${message}`);
}

function countLines(text) {
  const trimmed = text.replace(/\s+$/, '');
  return trimmed === '' ? 0 : trimmed.split(/\r?\n/).length;
}

// Minimal frontmatter reader: the leading --- block, `key: value` pairs, with
// support for folded values continued on indented lines.
function parseFrontmatter(text) {
  const lines = text.split(/\r?\n/);
  if (lines[0]?.trim() !== '---') return null;
  const end = lines.indexOf('---', 1);
  if (end === -1) return null;
  const fields = {};
  let key = null;
  for (const line of lines.slice(1, end)) {
    const match = /^([A-Za-z0-9_-]+):\s?(.*)$/.exec(line);
    if (match) {
      key = match[1];
      fields[key] = match[2];
    } else if (key && line.trim() !== '') {
      fields[key] += `\n${line.trim()}`;
    }
  }
  return { fields, body: lines.slice(end + 1).join('\n') };
}

// --- CLAUDE.md -------------------------------------------------------------

const claudeMd = join(root, 'CLAUDE.md');
if (!existsSync(claudeMd)) {
  fail('CLAUDE.md', 'missing — Tier 0 requires it');
} else {
  const lines = countLines(readFileSync(claudeMd, 'utf8'));
  if (lines > CAPS.claudeMdLines) {
    fail('CLAUDE.md', `${lines} lines, cap is ${CAPS.claudeMdLines}`);
  } else {
    notes.push(`CLAUDE.md ${lines}/${CAPS.claudeMdLines} lines`);
  }
}

// --- skills ----------------------------------------------------------------

if (!existsSync(skillsDir)) {
  fail('.claude/skills', 'missing — no skills to check');
} else {
  const found = readdirSync(skillsDir).filter((entry) =>
    statSync(join(skillsDir, entry)).isDirectory(),
  );

  for (const name of EXPECTED) {
    if (!found.includes(name)) fail(name, 'declared in the skill tree but not present');
  }
  for (const name of found) {
    if (!EXPECTED.includes(name)) {
      fail(name, 'present but not declared in the skill tree (update EXPECTED or remove it)');
    }
  }

  for (const name of found) {
    const where = `${name}/SKILL.md`;
    const path = join(skillsDir, name, 'SKILL.md');
    if (!existsSync(path)) {
      fail(where, 'missing');
      continue;
    }
    const parsed = parseFrontmatter(readFileSync(path, 'utf8'));
    if (!parsed) {
      fail(where, 'no YAML frontmatter block');
      continue;
    }
    const { fields, body } = parsed;

    if (!fields.name) fail(where, 'frontmatter has no `name`');
    else if (fields.name.trim() !== name) {
      fail(where, `frontmatter name "${fields.name.trim()}" does not match directory "${name}"`);
    }

    if (!fields.description) {
      fail(where, 'frontmatter has no `description`');
    } else {
      const descLines = countLines(fields.description);
      const descChars = fields.description.replace(/\s+/g, ' ').trim().length;
      if (descLines > CAPS.descriptionLines) {
        fail(where, `description is ${descLines} lines, cap is ${CAPS.descriptionLines}`);
      }
      if (descChars > CAPS.descriptionChars) {
        fail(where, `description is ${descChars} chars, cap is ${CAPS.descriptionChars}`);
      }
    }

    const bodyLines = countLines(body);
    if (bodyLines > CAPS.bodyLines) {
      fail(where, `body is ${bodyLines} lines, cap is ${CAPS.bodyLines}`);
    } else {
      notes.push(`${where} ${bodyLines}/${CAPS.bodyLines} lines`);
    }
  }
}

// --- report ----------------------------------------------------------------

for (const note of notes.sort()) console.log(`  ok  ${note}`);

if (problems.length > 0) {
  console.error(`\n${problems.length} skill budget violation(s):`);
  for (const problem of problems) console.error(`  FAIL  ${problem}`);
  process.exit(1);
}

console.log(`\nAll ${EXPECTED.length} skills within budget.`);
