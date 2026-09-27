#!/usr/bin/env node
// Enforce that docs/planning/gates/fixtures/MANIFEST.json (D6, issue #39 T1) stays in
// sync with the fixtures actually committed under docs/planning/gates/fixtures/.
//
//   node scripts/check-fixture-manifest.mjs
//
// Checks: MANIFEST.json parses as JSON; every file tracked by git under the fixtures
// tree (except MANIFEST.json itself) has exactly one manifest entry; every manifest
// entry's `path` exists on disk. Exits non-zero on any violation so CI fails loudly.

import { readFileSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join, dirname, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const fixturesDir = 'docs/planning/gates/fixtures';
const manifestPath = join(root, fixturesDir, 'MANIFEST.json');

const problems = [];

if (!existsSync(manifestPath)) {
  console.error(`FAIL  ${fixturesDir}/MANIFEST.json: missing`);
  process.exit(1);
}

let manifest;
try {
  manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
} catch (err) {
  console.error(`FAIL  ${fixturesDir}/MANIFEST.json: does not parse as JSON — ${err.message}`);
  process.exit(1);
}

if (!Array.isArray(manifest.fixtures)) {
  problems.push('MANIFEST.json: top-level `fixtures` array is missing');
}

const entries = manifest.fixtures ?? [];
const manifestPaths = new Map();
for (const entry of entries) {
  if (!entry.path) {
    problems.push('an entry has no `path` field');
    continue;
  }
  if (manifestPaths.has(entry.path)) {
    problems.push(`${entry.path}: duplicate manifest entry`);
  }
  manifestPaths.set(entry.path, (manifestPaths.get(entry.path) ?? 0) + 1);
  if (!existsSync(join(root, entry.path))) {
    problems.push(`${entry.path}: listed in MANIFEST.json but not found on disk`);
  }
}

let tracked;
try {
  tracked = execFileSync('git', ['ls-files', fixturesDir], { cwd: root, encoding: 'utf8' })
    .split(/\r?\n/)
    .filter(Boolean)
    .map((p) => relative(root, join(root, p)).replace(/\\/g, '/'));
} catch (err) {
  console.error(`Could not run \`git ls-files ${fixturesDir}\`: ${err.message}`);
  process.exit(1);
}

for (const file of tracked) {
  if (file.endsWith('MANIFEST.json')) continue;
  if (!manifestPaths.has(file)) {
    problems.push(`${file}: committed under ${fixturesDir}/ but has no MANIFEST.json entry`);
  }
}

if (problems.length > 0) {
  console.error(`\n${problems.length} fixture-manifest violation(s):`);
  for (const problem of problems) console.error(`  FAIL  ${problem}`);
  process.exit(1);
}

console.log(`All ${entries.length} MANIFEST.json entries match the ${tracked.length - 1} committed fixture file(s).`);
