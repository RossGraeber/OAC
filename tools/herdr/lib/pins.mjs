// The herdr pin, read from docs/planning/PINS.md -- never restated in the driver.
//
// PINS.md's pin table carries a row whose first cell is `herdr (test tooling)`; its
// "Pinned version" cell starts with the release tag in backticks (`v0.9.1` at K1). herdr
// prints its version as `herdr <CARGO_PKG_VERSION>` on the stable channel and
// `herdr <version>-<channel>[.<build>]` otherwise (herdr src/main.rs and
// src/build_info.rs at tag v0.9.1), so the only accepted `herdr --version` output is
// `herdr ` + the tag without its leading `v`. A preview build of the same base version
// is refused.

import { readFileSync } from 'node:fs';

export const PIN_ROW = 'herdr (test tooling)';

export function parseHerdrPin(pinsText) {
  const table = pinsText
    .split(/\r?\n/)
    .filter((line) => /^\s*\|/.test(line))
    .map((line) =>
      line
        .trim()
        .replace(/^\||\|$/g, '')
        .split('|')
        .map((c) => c.trim()),
    );
  const header = table.find((cells) => cells[0] === 'Surface' && cells.includes('Pinned version'));
  if (!header) throw new Error('PINS.md has no pin table header with a "Pinned version" column');
  const col = header.indexOf('Pinned version');
  const rows = table.filter((cells) => cells[0] === PIN_ROW);
  if (rows.length === 0) throw new Error(`PINS.md has no "${PIN_ROW}" pin-table row`);
  if (rows.length > 1) throw new Error(`PINS.md has ${rows.length} "${PIN_ROW}" rows; expected exactly one`);
  // The tag is the first backticked token of the row's "Pinned version" cell.
  const tag = (/`([^`]+)`/.exec(rows[0][col] ?? '') ?? [])[1];
  if (!tag || !/^v?\d+\.\d+\.\d+$/.test(tag)) {
    throw new Error(`PINS.md "${PIN_ROW}" row has no parseable release tag in its version cell`);
  }
  const version = tag.replace(/^v/, '');
  return { tag, version, expectedVersionOutput: `herdr ${version}` };
}

export function readHerdrPin(pinsPath) {
  return parseHerdrPin(readFileSync(pinsPath, 'utf8'));
}

// Exact comparison of `herdr --version` stdout (surrounding whitespace ignored).
export function versionMatches(pin, stdout) {
  return String(stdout ?? '').trim() === pin.expectedVersionOutput;
}
