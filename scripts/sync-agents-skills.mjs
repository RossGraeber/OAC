#!/usr/bin/env node
// Keep the Codex-facing copy of the agent instructions in sync with its source (#235).
//
//   node scripts/sync-agents-skills.mjs              # regenerate the copy
//   node scripts/sync-agents-skills.mjs --check      # exit 1 if the copy has drifted
//   node scripts/sync-agents-skills.mjs --self-test  # plant drift, assert --check fails
//   node scripts/sync-agents-skills.mjs --root <dir> # operate on another tree
//
// Source -> copy:
//   .claude/skills/**  ->  .agents/skills/**   (Codex scans .agents/skills for SKILL.md)
//   CLAUDE.md          ->  AGENTS.md           (Codex reads AGENTS.md as instructions)
//
// The transform is the identity, byte for byte after normalising line endings to LF.
// Nothing is renamed or rewritten: product names, URLs, paths and skill names stay as
// written in the source. The skills are harness-neutral procedure, and `.claude/skills`
// is the canonical source path every skill names, so it stays true in the copy. Never
// hand-edit the copy; edit the source and re-run this script.
//
// --check fails on: a source file missing from the copy, a copy file whose content
// differs, and a stray copy file with no source. Exits non-zero on any drift.

import {
  readFileSync,
  writeFileSync,
  readdirSync,
  statSync,
  existsSync,
  mkdirSync,
  rmSync,
  mkdtempSync,
} from 'node:fs';
import { join, dirname, resolve, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const scriptPath = fileURLToPath(import.meta.url);

// [source, copy] pairs, repo-relative. A directory pair maps every file beneath it.
const PAIRS = [
  { kind: 'dir', src: '.claude/skills', dst: '.agents/skills' },
  { kind: 'file', src: 'CLAUDE.md', dst: 'AGENTS.md' },
];

const toLf = (text) => text.replace(/\r\n/g, '\n');
const posixRel = (from, to) => relative(from, to).split(sep).join('/');

function walk(dir) {
  if (!existsSync(dir)) return [];
  const out = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) out.push(...walk(full));
    else out.push(full);
  }
  return out.sort();
}

// Returns the expected copy as a Map of repo-relative copy path -> content, plus the
// list of copy paths that currently exist (for stray detection).
function plan(root) {
  const expected = new Map();
  const present = [];
  for (const pair of PAIRS) {
    const src = join(root, pair.src);
    const dst = join(root, pair.dst);
    if (pair.kind === 'file') {
      if (!existsSync(src)) throw new Error(`source missing: ${pair.src}`);
      expected.set(pair.dst, toLf(readFileSync(src, 'utf8')));
      if (existsSync(dst)) present.push(pair.dst);
    } else {
      if (!existsSync(src)) throw new Error(`source missing: ${pair.src}`);
      for (const file of walk(src)) {
        expected.set(`${pair.dst}/${posixRel(src, file)}`, toLf(readFileSync(file, 'utf8')));
      }
      for (const file of walk(dst)) present.push(`${pair.dst}/${posixRel(dst, file)}`);
    }
  }
  return { expected, present };
}

function check(root) {
  const { expected, present } = plan(root);
  const drift = [];
  for (const [path, content] of expected) {
    const full = join(root, path);
    if (!existsSync(full)) drift.push(`missing   ${path}`);
    else if (toLf(readFileSync(full, 'utf8')) !== content) drift.push(`differs   ${path}`);
  }
  for (const path of present) {
    if (!expected.has(path)) drift.push(`stray     ${path}`);
  }
  return drift;
}

function sync(root) {
  const { expected, present } = plan(root);
  let written = 0;
  let removed = 0;
  for (const path of present) {
    if (!expected.has(path)) {
      rmSync(join(root, path));
      removed++;
    }
  }
  for (const [path, content] of expected) {
    const full = join(root, path);
    if (existsSync(full) && toLf(readFileSync(full, 'utf8')) === content) continue;
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, content);
    written++;
  }
  return { written, removed, total: expected.size };
}

function selfTest() {
  const results = [];
  const fresh = () => {
    const root = mkdtempSync(join(tmpdir(), 'oac-agents-sync-'));
    const put = (path, text) => {
      mkdirSync(dirname(join(root, path)), { recursive: true });
      writeFileSync(join(root, path), text);
    };
    put('CLAUDE.md', '# Example\n\nSee code.example.com and Example Code v2.1.\n');
    put('.claude/skills/a/SKILL.md', '---\nname: a\n---\nUse `.claude/skills/a`.\n');
    put('.claude/skills/a/references/r.md', 'Example Channels: --channels plugin:x\n');
    put('.claude/skills/b/SKILL.md', '---\nname: b\n---\nbody\r\nwith CRLF\r\n');
    sync(root);
    return { root, put };
  };
  const cases = [
    ['clean copy passes', () => {}, false],
    ['identity: copy is byte-equal to source', null, false],
    ['edited copy file fails', ({ put }) => put('.agents/skills/a/SKILL.md', 'hand edit\n'), true],
    ['rewritten product name fails', ({ root, put }) => {
      const p = '.agents/skills/a/references/r.md';
      put(p, readFileSync(join(root, p), 'utf8').replace('Example', 'Other'));
    }, true],
    ['missing copy file fails', ({ root }) => rmSync(join(root, '.agents/skills/a/references/r.md')), true],
    ['stray copy file fails', ({ put }) => put('.agents/skills/zz/SKILL.md', 'stray\n'), true],
    ['source edited without re-sync fails', ({ put }) => put('.claude/skills/b/SKILL.md', 'new\n'), true],
    ['AGENTS.md drift fails', ({ put }) => put('AGENTS.md', '# Different\n'), true],
    ['CRLF-only difference passes', ({ root, put }) => {
      put('AGENTS.md', readFileSync(join(root, 'AGENTS.md'), 'utf8').replace(/\n/g, '\r\n'));
    }, false],
  ];
  for (const [name, mutate, shouldFail] of cases) {
    const ctx = fresh();
    try {
      if (mutate === null) {
        const same = readFileSync(join(ctx.root, '.agents/skills/a/references/r.md'), 'utf8') ===
          readFileSync(join(ctx.root, '.claude/skills/a/references/r.md'), 'utf8') &&
          readFileSync(join(ctx.root, 'AGENTS.md'), 'utf8') ===
          readFileSync(join(ctx.root, 'CLAUDE.md'), 'utf8');
        results.push([name, same]);
        continue;
      }
      mutate(ctx);
      const failed = check(ctx.root).length > 0;
      let ok = failed === shouldFail;
      if (ok && shouldFail) {
        sync(ctx.root);
        ok = check(ctx.root).length === 0; // re-sync must repair every planted drift
      }
      results.push([name, ok]);
    } finally {
      rmSync(ctx.root, { recursive: true, force: true });
    }
  }
  let bad = 0;
  for (const [name, ok] of results) {
    console.log(`  ${ok ? 'ok  ' : 'FAIL'}  ${name}`);
    if (!ok) bad++;
  }
  console.log(`\nself-test: ${results.length - bad}/${results.length} passed`);
  process.exit(bad === 0 ? 0 : 1);
}

const args = process.argv.slice(2);
const rootIdx = args.indexOf('--root');
const root = rootIdx >= 0 ? resolve(args[rootIdx + 1]) : join(dirname(scriptPath), '..');

if (args.includes('--self-test')) {
  selfTest();
} else if (args.includes('--check')) {
  const drift = check(root);
  if (drift.length > 0) {
    console.error(`${drift.length} file(s) out of sync with their source:`);
    for (const line of drift) console.error(`  ${line}`);
    console.error('\nFix: node scripts/sync-agents-skills.mjs (never hand-edit .agents/skills or AGENTS.md)');
    process.exit(1);
  }
  console.log(`.agents/skills and AGENTS.md match their source (${plan(root).expected.size} files).`);
} else {
  const { written, removed, total } = sync(root);
  console.log(`synced ${total} file(s): ${written} written, ${removed} stray removed.`);
}
