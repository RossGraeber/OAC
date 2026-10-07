// SPDX-License-Identifier: Apache-2.0
//
// What the runner skips under tests/protocol/contract/ (#59, PR #323 review N1). The
// contract suites live there as Rust crates, one directory per crate. Only such a
// directory (one holding a Cargo.toml) is skipped, and only for its Rust sources and
// manifests: any other entry, and any data file inside a crate (a `.json` fixture put there
// by mistake, or any file other than `.rs`, `Cargo.toml` and `README.md`), is reported as
// stray, so a fixture is never silently left unrun. The crates hold no data files today;
// one they come to need is listed in DATA_FILES below.

import fs from 'node:fs';
import path from 'node:path';

const SOURCE_FILE = /(\.rs|^Cargo\.toml|^README\.md)$/;
// Data files a contract crate may hold, by path under tests/protocol/contract/. None yet.
export const DATA_FILES = new Set([]);

// entries: [{ name, isDir, hasCargoToml, files }] for the entries of
// tests/protocol/contract/; `files` lists each file under a directory entry, by path
// relative to it. Returns the stray messages.
export function contractStrays(entries) {
  const stray = [];
  for (const e of entries) {
    if (!(e.isDir && e.hasCargoToml)) {
      stray.push(
        `tests/protocol/contract/${e.name}${e.isDir ? '/' : ''}: not a contract-suite crate (a directory with a Cargo.toml)`,
      );
      continue;
    }
    for (const f of e.files ?? []) {
      const rel = `${e.name}/${f}`;
      if (SOURCE_FILE.test(path.posix.basename(f)) || DATA_FILES.has(rel)) continue;
      stray.push(`tests/protocol/contract/${rel}: a data file inside a contract-suite crate (only Rust sources and manifests are skipped)`);
    }
  }
  return stray;
}

function filesUnder(dir, rel = '') {
  const out = [];
  for (const e of fs.readdirSync(path.join(dir, rel), { withFileTypes: true })) {
    const r = rel ? `${rel}/${e.name}` : e.name;
    if (e.isDirectory()) {
      if (e.name !== 'target') out.push(...filesUnder(dir, r));
    } else {
      out.push(r);
    }
  }
  return out;
}

// The entries of `dir` (tests/protocol/contract/), read from disk.
export function readContractEntries(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).map((e) => {
    const isDir = e.isDirectory();
    const hasCargoToml = isDir && fs.existsSync(path.join(dir, e.name, 'Cargo.toml'));
    return { name: e.name, isDir, hasCargoToml, files: hasCargoToml ? filesUnder(path.join(dir, e.name)) : [] };
  });
}
