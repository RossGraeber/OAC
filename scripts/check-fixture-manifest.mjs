#!/usr/bin/env node
// Enforce that docs/planning/gates/fixtures/MANIFEST.json (D6, issue #39 T1) stays in
// sync with the fixtures actually committed under docs/planning/gates/fixtures/.
//
//   node scripts/check-fixture-manifest.mjs
//
// Checks: MANIFEST.json parses as JSON; every entry's `path` is inside the fixtures
// tree and exists on disk; every file tracked by git under the fixtures tree (except
// MANIFEST.json itself) has exactly one manifest entry; every entry carries the
// required fields (schema is additionally required for a Codex-touching entry).
// Exits non-zero on any violation so CI fails loudly.

import { readFileSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const fixturesDir = 'docs/planning/gates/fixtures';
const manifestRelPath = `${fixturesDir}/MANIFEST.json`;
const manifestPath = join(root, manifestRelPath);

// Every entry must carry these top-level fields (value may legitimately be `null`,
// e.g. capture_utc_range for a fixture with no wall-clock timestamps — the key must
// still be present so a reviewer sees the field was considered, not omitted).
const REQUIRED_KEYS = [
  'path',
  'provider',
  'surface',
  'observed_version',
  'pins_row',
  'pins_as_of',
  'version_matches_pin',
  'capture_date',
  'capture_utc_range',
  'superseded_by',
  'redaction',
  'coverage',
];
const REQUIRED_REDACTION_KEYS = ['script', 'script_sha256', 'residual_scan_result'];

function isCodexTouching(entry) {
  return typeof entry.provider === 'string' && entry.provider.includes('codex');
}

// A Codex-touching entry's `schema` block (T7, issue #39) must be one of three shapes:
//  1. { applicable: false, reason } -- Codex appears only as an outbound MCP client, not
//     the app-server protocol this schema describes (e.g. g4-mcp-dual-era entries).
//  2. The 0.154.0-superseded-baseline shape: { commit, hash_method, upstream: null,
//     local_generation: null, sha256: null, note, validated_against } -- schema was never
//     regenerated at that fixture's own (pre-floating-pin) observed version, so it must
//     NOT carry a 0.157.1 hash.
//  3. The full shape: { commit, hash_method, upstream: {path, file_count, tree_sha256,
//     verified: {date, method, result}}, local_generation: {cli, default: {command,
//     file_count, tree_sha256}, experimental: {command, file_count, tree_sha256, note}},
//     validated_against }.
function schemaShapeProblem(schema) {
  if (!schema || typeof schema !== 'object') return 'schema is missing or not an object';
  if (schema.applicable === false) {
    return typeof schema.reason === 'string' ? null : 'applicable:false schema block is missing `reason`';
  }
  if (typeof schema.commit !== 'string') return 'schema.commit is missing';
  if (typeof schema.hash_method !== 'string') return 'schema.hash_method is missing';
  if (schema.upstream === null && schema.local_generation === null) {
    // shape 2: superseded-baseline, schema not regenerated at this version.
    if (schema.sha256 !== null) return 'superseded-baseline schema.sha256 must be null (schema not regenerated at this version)';
    if (typeof schema.note !== 'string') return 'superseded-baseline schema is missing `note`';
    return null;
  }
  // shape 3: full comparison record.
  const u = schema.upstream;
  if (!u || typeof u.path !== 'string' || typeof u.file_count !== 'number' || typeof u.tree_sha256 !== 'string') {
    return 'schema.upstream is missing path/file_count/tree_sha256';
  }
  if (!u.verified || typeof u.verified.date !== 'string' || typeof u.verified.method !== 'string' || typeof u.verified.result !== 'string') {
    return 'schema.upstream.verified is missing date/method/result';
  }
  const lg = schema.local_generation;
  if (!lg || typeof lg.cli !== 'string') return 'schema.local_generation is missing `cli`';
  for (const tier of ['default', 'experimental']) {
    const t = lg[tier];
    if (!t || typeof t.command !== 'string' || typeof t.file_count !== 'number' || typeof t.tree_sha256 !== 'string') {
      return `schema.local_generation.${tier} is missing command/file_count/tree_sha256`;
    }
  }
  if (typeof schema.validated_against !== 'string') return 'schema.validated_against is missing';
  return null;
}

const problems = [];

if (!existsSync(manifestPath)) {
  console.error(`FAIL  ${manifestRelPath}: missing`);
  process.exit(1);
}

let manifest;
try {
  manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
} catch (err) {
  console.error(`FAIL  ${manifestRelPath}: does not parse as JSON — ${err.message}`);
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
  const label = entry.path;

  if (manifestPaths.has(entry.path)) {
    problems.push(`${label}: duplicate manifest entry`);
  }
  manifestPaths.set(entry.path, (manifestPaths.get(entry.path) ?? 0) + 1);

  if (!entry.path.startsWith(`${fixturesDir}/`)) {
    problems.push(`${label}: path is outside ${fixturesDir}/`);
  }
  if (entry.path === manifestRelPath) {
    problems.push(`${label}: an entry must not describe MANIFEST.json itself`);
  }
  if (!existsSync(join(root, entry.path))) {
    problems.push(`${label}: listed in MANIFEST.json but not found on disk`);
  }

  for (const key of REQUIRED_KEYS) {
    if (!(key in entry)) problems.push(`${label}: missing required field \`${key}\``);
  }
  if (entry.redaction && typeof entry.redaction === 'object') {
    for (const key of REQUIRED_REDACTION_KEYS) {
      if (!(key in entry.redaction)) problems.push(`${label}: redaction is missing \`${key}\``);
    }
  } else if ('redaction' in entry) {
    problems.push(`${label}: \`redaction\` must be an object with ${REQUIRED_REDACTION_KEYS.join(', ')}`);
  }

  if (isCodexTouching(entry)) {
    if (!('schema' in entry)) {
      problems.push(`${label}: provider "${entry.provider}" is Codex-touching but has no \`schema\` block`);
    } else {
      const problem = schemaShapeProblem(entry.schema);
      if (problem) problems.push(`${label}: schema -- ${problem}`);
    }
  }
}

let tracked;
try {
  tracked = execFileSync('git', ['ls-files', fixturesDir], { cwd: root, encoding: 'utf8' })
    .split(/\r?\n/)
    .filter(Boolean);
} catch (err) {
  console.error(`Could not run \`git ls-files ${fixturesDir}\`: ${err.message}`);
  process.exit(1);
}

const trackedFixtureFiles = tracked.filter((f) => f !== manifestRelPath);

for (const file of trackedFixtureFiles) {
  if (!manifestPaths.has(file)) {
    problems.push(`${file}: committed under ${fixturesDir}/ but has no MANIFEST.json entry`);
  }
}

if (problems.length > 0) {
  console.error(`\n${problems.length} fixture-manifest violation(s):`);
  for (const problem of problems) console.error(`  FAIL  ${problem}`);
  process.exit(1);
}

console.log(`All ${entries.length} MANIFEST.json entries match the ${trackedFixtureFiles.length} other committed fixture file(s).`);
