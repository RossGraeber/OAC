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
// Both modes refuse (exit 1, nothing written or deleted) on any symlink or junction in
// the source or the copy, on CLAUDE.md or AGENTS.md not being a regular file, and on a
// source file that is not valid UTF-8 or holds a NUL. Sync unlinks a copy file before
// rewriting it, so a hard link in the copy never carries a write outside the tree.

import {
  readFileSync,
  writeFileSync,
  readdirSync,
  lstatSync,
  symlinkSync,
  linkSync,
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

class SyncError extends Error {}

// lstat that never follows a link: null when the path does not exist.
function lstatOrNull(path) {
  try {
    return lstatSync(path);
  } catch (err) {
    if (err.code === 'ENOENT') return null;
    throw err;
  }
}

// Refuse any symlink (or junction) on the way to, or at, a source or copy path: following
// one would read, write or delete outside the tree, or loop forever.
function assertNoLink(root, rel) {
  let cur = root;
  for (const part of rel.split('/')) {
    cur = join(cur, part);
    const st = lstatOrNull(cur);
    if (st === null) return;
    if (st.isSymbolicLink()) throw new SyncError(`symlink not allowed: ${posixRel(root, cur)}`);
  }
}

function walk(root, dir) {
  const out = [];
  const st = lstatOrNull(dir);
  if (st === null) return out;
  if (st.isSymbolicLink()) throw new SyncError(`symlink not allowed: ${posixRel(root, dir)}`);
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const est = lstatSync(full);
    if (est.isSymbolicLink()) throw new SyncError(`symlink not allowed: ${posixRel(root, full)}`);
    if (est.isDirectory()) out.push(...walk(root, full));
    else if (est.isFile()) out.push(full);
    else throw new SyncError(`not a regular file: ${posixRel(root, full)}`);
  }
  return out.sort();
}

const utf8 = new TextDecoder('utf-8', { fatal: true });

// Read a text file strictly: invalid UTF-8 or a NUL byte (binary) is refused, never mangled.
function readText(root, path) {
  const bytes = readFileSync(path);
  let text;
  try {
    text = utf8.decode(bytes);
  } catch {
    throw new SyncError(`not valid UTF-8: ${posixRel(root, path)}`);
  }
  if (text.includes('\0')) throw new SyncError(`binary (NUL byte): ${posixRel(root, path)}`);
  return toLf(text);
}

// Returns the expected copy as a Map of repo-relative copy path -> content, plus the
// list of copy paths that currently exist (for stray detection).
function plan(root) {
  const expected = new Map();
  const present = [];
  for (const pair of PAIRS) {
    assertNoLink(root, pair.src);
    assertNoLink(root, pair.dst);
    const src = join(root, pair.src);
    const dst = join(root, pair.dst);
    if (lstatOrNull(src) === null) throw new SyncError(`source missing: ${pair.src}`);
    if (pair.kind === 'file') {
      if (!lstatSync(src).isFile()) throw new SyncError(`not a regular file: ${pair.src}`);
      const dstStat = lstatOrNull(dst);
      if (dstStat !== null && !dstStat.isFile()) {
        throw new SyncError(`not a regular file: ${pair.dst}`);
      }
      expected.set(pair.dst, readText(root, src));
      if (dstStat !== null) present.push(pair.dst);
    } else {
      for (const file of walk(root, src)) {
        expected.set(`${pair.dst}/${posixRel(src, file)}`, readText(root, file));
      }
      for (const file of walk(root, dst)) present.push(`${pair.dst}/${posixRel(dst, file)}`);
    }
  }
  return { expected, present };
}

function check(root) {
  const { expected, present } = plan(root);
  const drift = [];
  for (const [path, content] of expected) {
    const st = lstatOrNull(join(root, path));
    if (st === null) drift.push(`missing   ${path}`);
    else if (!st.isFile()) drift.push(`not-file  ${path}`);
    else if (toLf(readFileSync(join(root, path), 'utf8')) !== content) drift.push(`differs   ${path}`);
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
    const st = lstatOrNull(full);
    if (st !== null && st.isFile() && toLf(readFileSync(full, 'utf8')) === content) continue;
    // Unlink first, never write in place: an existing copy file may be a hard link to a
    // file outside the tree, and writing through it would change that file. plan() has
    // already refused symlinks, so this removes only entries inside the copy.
    rmSync(full, { recursive: true, force: true });
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
    // Dir links use a junction on Windows (no privilege needed); a file link the OS
    // refuses (EPERM/EACCES) makes the case a skip, not a pass.
    const link = (target, path, dir) => {
      mkdirSync(dirname(join(root, path)), { recursive: true });
      symlinkSync(join(root, target), join(root, path), dir ? 'junction' : 'file');
    };
    put('CLAUDE.md', '# Example\n\nSee code.example.com and Example Code v2.1.\n');
    put('.claude/skills/a/SKILL.md', '---\nname: a\n---\nUse `.claude/skills/a`.\n');
    put('.claude/skills/a/references/r.md', 'Example Channels: --channels plugin:x\n');
    put('.claude/skills/b/SKILL.md', '---\nname: b\n---\nbody\r\nwith CRLF\r\n');
    put('outside/keep.md', 'outside the tree\n');
    sync(root);
    return { root, put, link };
  };
  // expect: 'pass' (check clean), 'drift' (check reports drift and a re-sync repairs it),
  // 'refuse' (check and sync both throw SyncError; nothing outside the tree changes).
  const cases = [
    ['clean copy passes', () => {}, 'pass'],
    ['identity: copy is byte-equal to source', null, 'pass'],
    ['edited copy file fails', ({ put }) => put('.agents/skills/a/SKILL.md', 'hand edit\n'), 'drift'],
    ['rewritten product name fails', ({ root, put }) => {
      const p = '.agents/skills/a/references/r.md';
      put(p, readFileSync(join(root, p), 'utf8').replace('Example', 'Other'));
    }, 'drift'],
    ['missing copy file fails', ({ root }) => rmSync(join(root, '.agents/skills/a/references/r.md')), 'drift'],
    ['stray copy file fails', ({ put }) => put('.agents/skills/zz/SKILL.md', 'stray\n'), 'drift'],
    ['source edited without re-sync fails', ({ put }) => put('.claude/skills/b/SKILL.md', 'new\n'), 'drift'],
    ['AGENTS.md drift fails', ({ put }) => put('AGENTS.md', '# Different\n'), 'drift'],
    ['CRLF-only difference passes', ({ root, put }) => {
      put('AGENTS.md', readFileSync(join(root, 'AGENTS.md'), 'utf8').replace(/\n/g, '\r\n'));
    }, 'pass'],
    ['copy dir symlink pointing outside is refused', ({ link }) => link('outside', '.agents/skills/zz', true), 'refuse'],
    ['copy root symlink is refused', ({ root, link }) => {
      rmSync(join(root, '.agents'), { recursive: true });
      link('outside', '.agents', true);
    }, 'refuse'],
    ['source dir symlink is refused', ({ link }) => link('outside', '.claude/skills/c', true), 'refuse'],
    ['source symlink loop is refused', ({ link }) => link('.claude/skills/a', '.claude/skills/a/loop', true), 'refuse'],
    ['AGENTS.md file symlink is refused', ({ root, link }) => {
      rmSync(join(root, 'AGENTS.md'));
      link('outside/keep.md', 'AGENTS.md', false);
    }, 'refuse'],
    ['hard-linked copy file is replaced, not written through', ({ root }) => {
      rmSync(join(root, '.agents/skills/a/SKILL.md'));
      linkSync(join(root, 'outside/keep.md'), join(root, '.agents/skills/a/SKILL.md'));
    }, 'drift'],
    ['hard-linked AGENTS.md is replaced, not written through', ({ root }) => {
      rmSync(join(root, 'AGENTS.md'));
      linkSync(join(root, 'outside/keep.md'), join(root, 'AGENTS.md'));
    }, 'drift'],
    ['directory at a copy file path fails and re-sync repairs it', ({ root, put }) => {
      rmSync(join(root, '.agents/skills/a/SKILL.md'));
      put('.agents/skills/a/SKILL.md/x.md', 'x\n');
    }, 'drift'],
    ['CLAUDE.md as a directory is refused', ({ root, put }) => {
      rmSync(join(root, 'CLAUDE.md'));
      put('CLAUDE.md/x.md', 'x\n');
    }, 'refuse'],
    ['AGENTS.md as a directory is refused', ({ root, put }) => {
      rmSync(join(root, 'AGENTS.md'));
      put('AGENTS.md/x.md', 'x\n');
    }, 'refuse'],
    ['non-UTF-8 source is refused', ({ root }) => {
      writeFileSync(join(root, '.claude/skills/a/bad.md'), Buffer.from([0x61, 0xff, 0xfe, 0x0a]));
    }, 'refuse'],
    ['binary (NUL byte) source is refused', ({ root }) => {
      writeFileSync(join(root, '.claude/skills/a/bin.dat'), Buffer.from([0x61, 0x00, 0x62]));
    }, 'refuse'],
  ];
  const throwsSyncError = (fn) => {
    try {
      fn();
      return false;
    } catch (err) {
      return err instanceof SyncError;
    }
  };
  for (const [name, mutate, expect] of cases) {
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
      try {
        mutate(ctx);
      } catch (err) {
        if (err.code === 'EPERM' || err.code === 'EACCES') {
          results.push([name, 'skip']);
          continue;
        }
        throw err;
      }
      let ok;
      if (expect === 'refuse') {
        ok = throwsSyncError(() => check(ctx.root)) && throwsSyncError(() => sync(ctx.root));
      } else {
        const failed = check(ctx.root).length > 0;
        ok = failed === (expect === 'drift');
        if (ok && failed) {
          sync(ctx.root);
          ok = check(ctx.root).length === 0; // re-sync must repair every planted drift
        }
      }
      // Every case: nothing outside the tree may change (links, hard links included).
      ok = ok && readdirSync(join(ctx.root, 'outside')).length === 1 &&
        readFileSync(join(ctx.root, 'outside/keep.md'), 'utf8') === 'outside the tree\n';
      results.push([name, ok]);
    } finally {
      rmSync(ctx.root, { recursive: true, force: true });
    }
  }
  let bad = 0;
  let skipped = 0;
  for (const [name, ok] of results) {
    console.log(`  ${ok === 'skip' ? 'skip' : ok ? 'ok  ' : 'FAIL'}  ${name}`);
    if (ok === 'skip') skipped++;
    else if (!ok) bad++;
  }
  const passed = results.length - bad - skipped;
  console.log(`\nself-test: ${passed}/${results.length} passed, ${skipped} skipped`);
  process.exit(bad === 0 ? 0 : 1);
}

const USAGE = 'usage: node scripts/sync-agents-skills.mjs [--check | --self-test] [--root <dir>]';
const args = process.argv.slice(2);
const rootIdx = args.indexOf('--root');
if (rootIdx >= 0 && (!args[rootIdx + 1] || args[rootIdx + 1].startsWith('--'))) {
  console.error(`--root needs a directory argument\n${USAGE}`);
  process.exit(2);
}
const root = rootIdx >= 0 ? resolve(args[rootIdx + 1]) : join(dirname(scriptPath), '..');

// A refusal (symlink, non-UTF-8, binary, missing source) is a clean failure, not a crash.
function run(fn) {
  try {
    fn();
  } catch (err) {
    if (!(err instanceof SyncError)) throw err;
    console.error(`refused: ${err.message}`);
    process.exit(1);
  }
}

if (args.includes('--self-test')) {
  selfTest();
} else if (args.includes('--check')) {
  run(() => {
    const drift = check(root);
    if (drift.length > 0) {
      console.error(`${drift.length} file(s) out of sync with their source:`);
      for (const line of drift) console.error(`  ${line}`);
      console.error('\nFix: node scripts/sync-agents-skills.mjs (never hand-edit .agents/skills or AGENTS.md)');
      process.exit(1);
    }
    console.log(`.agents/skills and AGENTS.md match their source (${plan(root).expected.size} files).`);
  });
} else {
  run(() => {
    const { written, removed, total } = sync(root);
    console.log(`synced ${total} file(s): ${written} written, ${removed} stray removed.`);
  });
}
