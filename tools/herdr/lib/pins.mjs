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

// --- Harness CLIs (Claude Code, Codex): versions float; warn, never gate (#216) ----------
//
// Operator decision on #216 (2026-10-01): "Minimum version is the first version encountered
// while working. Document last version tested against. Allow version to float. Do not gate
// on version, warn on version." PINS.md's `Claude Code (Channels)` and `Codex CLI /
// app-server` rows therefore hold no pin. Their "Pinned version" cell names a **minimum**
// version (the first one the project worked with, cited in PINS.md) and a **last tested**
// version (updated after each live run). A scripted run compares what the harness reports
// with both and records any difference as a VERSION WARNING finding. The warning never
// stops a run, never makes it NOT RUN, never blocks CI and never by itself invalidates a
// gate verdict. The herdr row above is test tooling, not a harness, and keeps its exact
// pin: #216 covers the harness CLIs only.

// Numeric X.Y.Z comparison: <0, 0 or >0. Both arguments must be X.Y.Z strings.
export function compareVersions(a, b) {
  const pa = String(a).split('.').map(Number);
  const pb = String(b).split('.').map(Number);
  for (let i = 0; i < 3; i++) if (pa[i] !== pb[i]) return pa[i] - pb[i];
  return 0;
}

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

const isVersion = (v) => typeof v === 'string' && /^\d+\.\d+\.\d+$/.test(v);

// The warning for one observed harness version, or null when it equals the last tested
// version. `display` formats a version for the message. Null-tolerant (#216 review): a
// missing or unreadable last tested or minimum version is itself a warning, never a throw.
function versionWarning({ harness, row, observed, lastTested, minimum, source, gate, display }) {
  const show = (v) => (isVersion(v) ? display(v) : 'unreadable');
  const issues = [];
  if (!isVersion(observed)) issues.push('no parseable version');
  if (!isVersion(lastTested)) issues.push('PINS.md\'s last tested version could not be read');
  if (!isVersion(minimum)) issues.push('PINS.md\'s minimum version could not be read');
  if (isVersion(observed)) {
    if (isVersion(minimum) && compareVersions(observed, minimum) < 0) issues.push(`below the minimum ${display(minimum)}`);
    if (isVersion(lastTested) && observed !== lastTested) issues.push(`not the last tested ${display(lastTested)}`);
  }
  if (!issues.length) return null;
  const seen = isVersion(observed) ? display(observed) : 'no parseable version';
  return (
    `VERSION WARNING (${gate}): ${source} reports ${seen}; docs/planning/PINS.md "${row}" records minimum ${show(minimum)}, ` +
    `last tested ${show(lastTested)} (${issues.join('; ')}). ${harness} versions float and are never gated (operator decision on #216, ` +
    '2026-10-01): the run continues, and this is a finding only. It does not by itself invalidate any gate verdict. The run does not edit ' +
    'PINS.md; record the version as last tested there after a live run.'
  );
}

// --- Claude Code (Channels) ---------------------------------------------------------------
//
// The cell reads "**floating** — minimum `v2.1.282`; last tested `v2.1.285` (...)".

export const CLAUDE_PIN_ROW = 'Claude Code (Channels)';

// Reads a harness row without ever throwing (#216 review): a missing table, a missing or
// duplicated row, or a cell without a minimum or last tested version gives null for what
// could not be read, and `problem` says why. A run reports `problem` through
// pinsReadWarning() and continues; the version checks then warn that PINS.md could not be read.
function readHarnessRow(pinsText, row, minRe, lastRe, shape) {
  let cell = null;
  try {
    cell = pinTableCell(String(pinsText ?? ''), row, 'Pinned version');
  } catch (err) {
    return { row, minimum: null, lastTested: null, cell: null, lastIndex: -1, problem: err.message };
  }
  const min = minRe.exec(cell);
  const last = lastRe.exec(cell);
  const missing = [last ? null : `"last tested \`${shape}\`"`, min ? null : `"minimum \`${shape}\`"`].filter(Boolean);
  return {
    row,
    minimum: min ? min[1] : null,
    lastTested: last ? last[1] : null,
    cell,
    lastIndex: last ? last.index : -1,
    problem: missing.length ? `PINS.md "${row}" row has no ${missing.join(' and no ')} version in its version cell` : null,
  };
}

// The finding for a harness row that could not be read, or null. Never a stop (#216).
export function pinsReadWarning(versions, gate) {
  if (!versions?.problem) return null;
  return (
    `VERSION WARNING (${gate}): could not read docs/planning/PINS.md "${versions.row}": ${versions.problem}. Harness versions are never gated ` +
    '(operator decision on #216, 2026-10-01): the run continues, and every version check of this harness warns that PINS.md could not be read.'
  );
}

// -> { row, minimum: '2.1.282' | null, lastTested: '2.1.285' | null, cell, problem: string | null }.
// Never throws.
export function parseClaudeVersions(pinsText) {
  const { lastIndex, ...v } = readHarnessRow(pinsText, CLAUDE_PIN_ROW, /minimum\s+`v?(\d+\.\d+\.\d+)`/i, /last[\s-]+tested\s+`v?(\d+\.\d+\.\d+)`/i, 'vX.Y.Z');
  return v;
}

// `claude --version` prints e.g. `2.1.283 (Claude Code)` (G1-result.md "Version triple").
// Returns the bare X.Y.Z, or null when the output does not start with one.
export function parseClaudeCliVersion(stdout) {
  const m = /^\s*v?(\d+\.\d+\.\d+)(?=$|[\s(])/.exec(String(stdout ?? ''));
  return m ? m[1] : null;
}

// null when `observed` equals PINS.md's last tested version, else a VERSION WARNING string
// for ctx.finding. Never a reason to stop the run (#216).
export function claudeVersionWarning({ observed, lastTested, minimum, source, gate = 'G1' }) {
  return versionWarning({ harness: 'Claude Code', row: CLAUDE_PIN_ROW, observed, lastTested, minimum, source, gate, display: (v) => `v${v}` });
}

// --- Codex CLI / app-server ---------------------------------------------------------------
//
// The cell names the npm package and the release commit: "**floating** — minimum
// `@<scope>/codex@0.154.0`; last tested `@<scope>/codex@0.159.3` (commit `<40 hex>`; ...)".
// Codex reports its version from three places: the CLI (`codex --version`, e.g. `codex-cli
// 0.157.1`), the daemon (`codex app-server daemon version`: cliVersion / appServerVersion /
// managedCodexVersion) and the wire (the `initialize` result's `userAgent`, e.g.
// `codex-tui/0.157.1 (...)`).

export const CODEX_PIN_ROW = 'Codex CLI / app-server';

// -> { row, minimum: '0.154.0' | null, lastTested: '0.159.3' | null, commit: '<40 hex>' | null,
// cell, problem: string | null }. Never throws.
export function parseCodexVersions(pinsText) {
  const { lastIndex, ...v } = readHarnessRow(pinsText, CODEX_PIN_ROW, /minimum\s+`(?:@[\w.-]+\/)?codex@v?(\d+\.\d+\.\d+)`/i, /last[\s-]+tested\s+`(?:@[\w.-]+\/)?codex@v?(\d+\.\d+\.\d+)`/i, '@<scope>/codex@X.Y.Z');
  // The commit that follows the last tested version (the minimum may carry its own).
  const c = lastIndex >= 0 ? /commit\s+`([0-9a-f]{40})`/i.exec(v.cell.slice(lastIndex)) : null;
  return { ...v, commit: c ? c[1] : null };
}

// `codex --version` prints `codex-cli 0.157.1` (G2-result.md). Bare X.Y.Z, or null.
export function parseCodexCliVersion(stdout) {
  const m = /^\s*codex-cli\s+v?(\d+\.\d+\.\d+)(?=$|\s)/.exec(String(stdout ?? ''));
  return m ? m[1] : null;
}

// The `userAgent` of an `initialize` result: `<originator>/<X.Y.Z> (...)`. Bare X.Y.Z, or null.
export function parseCodexUserAgentVersion(userAgent) {
  const m = /^[^\s/]+\/v?(\d+\.\d+\.\d+)(?=$|[\s(])/.exec(String(userAgent ?? ''));
  return m ? m[1] : null;
}

// `codex app-server daemon version` prints one JSON object (G2-result.md quotes
// `{"status":"running",...,"managedCodexVersion":"0.157.1","cliVersion":"0.157.1",
// "appServerVersion":"0.157.1"}`). Returns the three version fields and the status only
// (nothing else from the output is kept), or null when no JSON object is found.
export const CODEX_DAEMON_VERSION_FIELDS = Object.freeze(['cliVersion', 'appServerVersion', 'managedCodexVersion']);
export function parseCodexDaemonVersion(stdout) {
  const s = String(stdout ?? '');
  const start = s.indexOf('{');
  const end = s.lastIndexOf('}');
  if (start === -1 || end <= start) return null;
  let j;
  try {
    j = JSON.parse(s.slice(start, end + 1));
  } catch {
    return null;
  }
  const out = { status: typeof j.status === 'string' ? j.status : null };
  for (const k of CODEX_DAEMON_VERSION_FIELDS) out[k] = typeof j[k] === 'string' && /^\d+\.\d+\.\d+$/.test(j[k]) ? j[k] : null;
  return out;
}

// null when `observed` equals PINS.md's last tested version, else a VERSION WARNING string
// for ctx.finding. Never a reason to stop the run (#216).
export function codexVersionWarning({ observed, lastTested, minimum, source, gate = 'G2' }) {
  return versionWarning({ harness: 'Codex', row: CODEX_PIN_ROW, observed, lastTested, minimum, source, gate, display: (v) => v });
}
