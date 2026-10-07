// SPDX-License-Identifier: Apache-2.0
//
// What the runner skips under tests/protocol/contract/ (#59, PR #323 review N1). The
// contract suites live there as Rust crates, one directory per crate. Only such a
// directory (one holding a Cargo.toml) is skipped; any other entry is reported as stray, so
// a fixture put there by mistake is never silently left unrun.

import fs from 'node:fs';
import path from 'node:path';

// entries: [{ name, isDir, hasCargoToml }] for the entries of tests/protocol/contract/.
// Returns the stray messages.
export function contractStrays(entries) {
  const stray = [];
  for (const e of entries) {
    if (e.isDir && e.hasCargoToml) continue;
    stray.push(
      `tests/protocol/contract/${e.name}${e.isDir ? '/' : ''}: not a contract-suite crate (a directory with a Cargo.toml)`,
    );
  }
  return stray;
}

// The entries of `dir` (tests/protocol/contract/), read from disk.
export function readContractEntries(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).map((e) => ({
    name: e.name,
    isDir: e.isDirectory(),
    hasCargoToml: e.isDirectory() && fs.existsSync(path.join(dir, e.name, 'Cargo.toml')),
  }));
}
