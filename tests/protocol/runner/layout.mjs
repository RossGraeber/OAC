// SPDX-License-Identifier: Apache-2.0
//
// What the runner skips under tests/protocol/contract/ (#59, PR #323 review N1). The
// contract suites live there as Rust crates, one directory per crate. Only such a
// directory (one holding a Cargo.toml) is skipped, and only for its Rust sources and
// manifests: any other entry, and any data file inside a crate (a `.json` fixture put there
// by mistake, or any file other than `.rs`, `Cargo.toml` and `README.md`), is reported as
// stray, so a fixture is never silently left unrun. The crates hold no data files today;
// one they come to need is listed in DATA_FILES below.
//
// Nothing is skipped by name alone (#324): a `target/` directory inside a crate is stray
// (the workspace builds into the repository's own target/, so a crate holds none), and a
// `.rs` file counts as a Rust source only where cargo looks for one (`src/`, `tests/`,
// `benches/`, `examples/`, or `build.rs` at the crate root) and only when its text is not
// JSON, so a fixture renamed to `.rs` is stray too.

import fs from 'node:fs';
import path from 'node:path';

const MANIFEST_FILE = /^(Cargo\.toml|README\.md)$/;
// Where cargo looks for a crate's Rust sources.
const RUST_SOURCE_PATH = /^(?:(?:src|tests|benches|examples)\/.+|build)\.rs$/;
// Data files a contract crate may hold, by path under tests/protocol/contract/. None yet.
export const DATA_FILES = new Set([]);

// A `.rs` file's text that is a JSON document, not Rust.
function isJson(text) {
  const t = text.trim();
  if (!/^[[{]/.test(t)) return false;
  try {
    JSON.parse(t);
    return true;
  } catch {
    return false;
  }
}

// entries: [{ name, isDir, hasCargoToml, files, dirs, texts }] for the entries of
// tests/protocol/contract/; `files` lists each file under a directory entry, by path
// relative to it, `dirs` each `target/` directory found there (not descended into), and
// `texts` maps a `.rs` file's path to its text. Returns the stray messages.
export function contractStrays(entries) {
  const stray = [];
  for (const e of entries) {
    if (!(e.isDir && e.hasCargoToml)) {
      stray.push(
        `tests/protocol/contract/${e.name}${e.isDir ? '/' : ''}: not a contract-suite crate (a directory with a Cargo.toml)`,
      );
      continue;
    }
    for (const d of e.dirs ?? []) {
      stray.push(`tests/protocol/contract/${e.name}/${d}/: a target/ directory inside a contract-suite crate (nothing is skipped by name)`);
    }
    for (const f of e.files ?? []) {
      const rel = `${e.name}/${f}`;
      if (DATA_FILES.has(rel)) continue;
      if (MANIFEST_FILE.test(path.posix.basename(f))) continue;
      if (f.endsWith('.rs')) {
        if (!RUST_SOURCE_PATH.test(f)) {
          stray.push(`tests/protocol/contract/${rel}: a .rs file where cargo looks for no Rust source (only src/, tests/, benches/, examples/ and build.rs are skipped)`);
        } else if (isJson(e.texts?.[f] ?? '')) {
          stray.push(`tests/protocol/contract/${rel}: a JSON document named .rs inside a contract-suite crate (a fixture is never skipped)`);
        }
        continue;
      }
      stray.push(`tests/protocol/contract/${rel}: a data file inside a contract-suite crate (only Rust sources and manifests are skipped)`);
    }
  }
  return stray;
}

// The files under `dir`, by relative path, and the `target/` directories, not descended into.
function walk(dir, rel = '', out = { files: [], dirs: [] }) {
  for (const e of fs.readdirSync(path.join(dir, rel), { withFileTypes: true })) {
    const r = rel ? `${rel}/${e.name}` : e.name;
    if (e.isDirectory()) {
      if (e.name === 'target') out.dirs.push(r);
      else walk(dir, r, out);
    } else {
      out.files.push(r);
    }
  }
  return out;
}

// The entries of `dir` (tests/protocol/contract/), read from disk.
export function readContractEntries(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).map((e) => {
    const isDir = e.isDirectory();
    const crate = path.join(dir, e.name);
    const hasCargoToml = isDir && fs.existsSync(path.join(crate, 'Cargo.toml'));
    if (!hasCargoToml) return { name: e.name, isDir, hasCargoToml, files: [], dirs: [], texts: {} };
    const { files, dirs } = walk(crate);
    const texts = {};
    for (const f of files) if (f.endsWith('.rs')) texts[f] = fs.readFileSync(path.join(crate, f), 'utf8');
    return { name: e.name, isDir, hasCargoToml, files, dirs, texts };
  });
}
