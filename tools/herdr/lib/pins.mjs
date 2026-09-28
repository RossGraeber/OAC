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

// --- Claude Code (Channels): floating, last-observed version (K4) ----------------------
//
// PINS.md's `Claude Code (Channels)` row does not hold a fixed pin: its "Pinned version"
// cell reads "**floating** — last observed `v2.1.283`; ..." (PINS.md "Floating-version
// policy", operator decision 2026-09-27). Any newly observed version is a pin-move trigger
// under that policy, so a scripted G1 run on a different version stops instead of running.

export const CLAUDE_PIN_ROW = 'Claude Code (Channels)';

function pinTableCell(pinsText, rowName, column) {
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
  const header = table.find((cells) => cells[0] === 'Surface' && cells.includes(column));
  if (!header) throw new Error(`PINS.md has no pin table header with a "${column}" column`);
  const rows = table.filter((cells) => cells[0] === rowName);
  if (rows.length === 0) throw new Error(`PINS.md has no "${rowName}" pin-table row`);
  if (rows.length > 1) throw new Error(`PINS.md has ${rows.length} "${rowName}" rows; expected exactly one`);
  return rows[0][header.indexOf(column)] ?? '';
}

// -> { row, lastObserved: '2.1.283', cell } ; throws when the row or version is missing.
export function parseClaudeLastObserved(pinsText) {
  const cell = pinTableCell(pinsText, CLAUDE_PIN_ROW, 'Pinned version');
  const m = /last[\s-]+observed\s+`v?(\d+\.\d+\.\d+)`/i.exec(cell);
  if (!m) throw new Error(`PINS.md "${CLAUDE_PIN_ROW}" row has no "last observed \`vX.Y.Z\`" version in its version cell`);
  return { row: CLAUDE_PIN_ROW, lastObserved: m[1], cell };
}

// `claude --version` prints e.g. `2.1.283 (Claude Code)` (G1-result.md "Version triple").
// Returns the bare X.Y.Z, or null when the output does not start with one.
export function parseClaudeCliVersion(stdout) {
  const m = /^\s*v?(\d+\.\d+\.\d+)(?=$|[\s(])/.exec(String(stdout ?? ''));
  return m ? m[1] : null;
}

// The pin-move-trigger check for a scripted Claude-side run. Returns null when the
// observed version equals PINS.md's last-observed version, else a message saying why the
// run must stop. `source` names where the observed version came from.
export function claudePinMoveTrigger({ observed, lastObserved, source }) {
  if (observed && observed === lastObserved) return null;
  const seen = observed ? `v${observed}` : 'no parseable version';
  return (
    `PIN-MOVE TRIGGER: ${source} reports ${seen}, but docs/planning/PINS.md "${CLAUDE_PIN_ROW}" last observed ` +
    `v${lastObserved}. Under PINS.md's floating-version policy a newly observed Claude Code version is a ` +
    'pin-move trigger: run the pin-move checklist (and re-verify the PLANNING-PROMPT.md §3.1 facts) before ' +
    're-running G1. This scripted run stops here and does not edit PINS.md.'
  );
}
