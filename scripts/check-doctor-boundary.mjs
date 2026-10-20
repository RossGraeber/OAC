#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// cli/ never names a harness's credential file, database or configuration directory
// (G10, #71, slice 5; ADR-001 Boundary: no stealing or reusing another harness's
// credentials, no private surfaces). `oac doctor` is the verb that could be tempted.
//
//   node scripts/check-doctor-boundary.mjs              # check this repository
//   node scripts/check-doctor-boundary.mjs --root <dir> # check another git work tree
//   node scripts/check-doctor-boundary.mjs --self-test  # plant violations, assert each fails
//
// Scope: every git-tracked .rs file under cli/src/. Forbidden, case-insensitively, anywhere
// in the text (comments included, so that a doc comment cannot hide a read):
//   auth.json   .credentials.json   .sqlite   CODEX_HOME   CLAUDE_CONFIG_DIR
//   .claude   .codex   (the default locations of a harness's own files; `.claude` also
//   catches `.claude.json` and `.claude/`)
// ALLOW lists the legitimate mentions, one exact trimmed line each with its reason. An entry
// that no longer matches fails (stale). Its one entry is a struct field access that is not a path.
//
// Exit codes: 0 = clean; 1 = a violation (or a failed self-test case); 2 = usage or
// environment error. Node built-ins and git only.

import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptPath = fileURLToPath(import.meta.url);
const repoRoot = resolve(dirname(scriptPath), '..');

export const FORBIDDEN = ['auth.json', '.credentials.json', '.sqlite', 'CODEX_HOME', 'CLAUDE_CONFIG_DIR', '.claude', '.codex'];
// { file, line (exact, trimmed), reason }
export const ALLOW = [
  // The `claude` field of the doctor's probes (`claude --version`), not a path.
  { file: 'cli/src/doctor.rs', line: 'match (probes.claude)() {', reason: 'a struct field named claude, not the .claude directory' },
];

export function scan(root, allow = ALLOW) {
  const files = execFileSync('git', ['-C', root, 'ls-files', '--', 'cli/src'], { encoding: 'utf8' })
    .split('\n').filter((f) => f.endsWith('.rs'));
  const hits = [];
  const used = new Set();
  for (const file of files) {
    const lines = readFileSync(join(root, file), 'utf8').split('\n');
    lines.forEach((text, i) => {
      const lower = text.toLowerCase();
      for (const word of FORBIDDEN) {
        if (!lower.includes(word.toLowerCase())) continue;
        const idx = allow.findIndex((a) => a.file === file && a.line === text.trim());
        if (idx >= 0) { used.add(idx); continue; }
        hits.push(`${file}:${i + 1}: names ${word}`);
      }
    });
  }
  allow.forEach((a, i) => { if (!used.has(i)) hits.push(`stale allow entry: ${a.file}: ${a.line}`); });
  return hits;
}

function selfTest() {
  const dir = mkdtempSync(join(tmpdir(), 'oac-doctor-boundary-'));
  const git = (...args) => execFileSync('git', ['-C', dir, ...args], { stdio: 'pipe' });
  let failed = 0;
  const expect = (name, ok) => { if (!ok) { failed += 1; console.error(`self-test FAILED: ${name}`); } };
  try {
    git('init', '-q');
    mkdirSync(join(dir, 'cli/src'), { recursive: true });
    const put = (text) => { writeFileSync(join(dir, 'cli/src/x.rs'), text); git('add', '-A'); };
    put('fn clean() {}\n');
    expect('a clean tree passes', scan(dir, []).length === 0);
    for (const word of FORBIDDEN) {
      put(`// reads ${word}\n`);
      expect(`${word} in a comment fails`, scan(dir, []).length === 1);
      put(`let p = home.join("${word.toUpperCase()}");\n`);
      expect(`${word} in code (any case) fails`, scan(dir, []).length === 1);
    }
    for (const word of ['codex_home', 'claude_config_dir', 'Codex_Home']) {
      put(`std::env::var_os("${word}");\n`);
      expect(`${word} (other case) fails`, scan(dir, []).length === 1);
    }
    put('let a = "auth.json";\n');
    expect('an allowed exact line passes', scan(dir, [{ file: 'cli/src/x.rs', line: 'let a = "auth.json";', reason: 't' }]).length === 0);
    put('fn clean() {}\n');
    expect('a stale allow entry fails', scan(dir, [{ file: 'cli/src/x.rs', line: 'gone', reason: 't' }]).length === 1);
    mkdirSync(join(dir, 'core'), { recursive: true });
    writeFileSync(join(dir, 'core/y.rs'), '// auth.json\n'); git('add', '-A');
    expect('files outside cli/src are not scanned', scan(dir, []).length === 0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
  if (failed) process.exit(1);
  console.log('check-doctor-boundary self-test: ok');
}

const args = process.argv.slice(2);
if (args[0] === '--self-test') selfTest();
else {
  let root = repoRoot;
  if (args[0] === '--root' && args[1]) root = resolve(args[1]);
  else if (args.length) { console.error('usage: check-doctor-boundary.mjs [--root <dir>] [--self-test]'); process.exit(2); }
  const hits = scan(root);
  if (hits.length) { console.error(hits.join('\n')); process.exit(1); }
  console.log('check-doctor-boundary: cli/src names no harness credential file, database or configuration directory');
}
