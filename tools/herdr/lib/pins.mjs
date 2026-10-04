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
import { join } from 'node:path';

import { committedFile } from './committed-file.mjs';

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

// --- The expected herdr executable (#252) -----------------------------------------------
//
// Operator decision on #252 (2026-10-03): which herdr ran is verified by the driver, not
// attested. PINS.md's "herdr (test tooling)" section carries a table whose header starts
// `| Platform | ... | Expected executable sha256 | ... |`, one row per platform
// (`process.platform`-`process.arch`, in backticks: `win32-x64`, `linux-x64`, ...). The
// expected sha256 is the first backticked 64-hex token of that cell; the Basis cell says
// where it comes from. The driver compares the executable it resolved and hashed with the
// row for its platform (checkHerdrExecutable). No table, or no row for this platform, is
// not an error: the comparison is then `no-expected-value`, and the herdr identity UNVERIFIED.
export const EXPECTED_HASH_COLUMN = 'Expected executable sha256';
export const FIRST_PARTY_COLUMN = 'First-party';

export function parseHerdrExpectedExecutables(pinsText) {
  const lines = pinsText.split(/\r?\n/);
  const cells = (line) => line.trim().replace(/^\||\|$/g, '').split('|').map((c) => c.trim());
  const start = lines.findIndex((l) => /^\s*\|/.test(l) && cells(l)[0] === 'Platform' && cells(l).includes(EXPECTED_HASH_COLUMN));
  if (start === -1) return [];
  const header = cells(lines[start]);
  const col = (name) => header.indexOf(name);
  const out = [];
  for (const line of lines.slice(start + 2)) {
    if (!/^\s*\|/.test(line)) break;
    const row = cells(line);
    const platform = (/`([^`]+)`/.exec(row[0] ?? '') ?? [])[1];
    const sha256 = (/`([0-9a-f]{64})`/.exec(row[col(EXPECTED_HASH_COLUMN)] ?? '') ?? [])[1];
    if (!platform || !sha256) throw new Error(`PINS.md "${PIN_ROW}" expected-executable row has no backticked platform or 64-hex sha256: ${line.trim()}`);
    if (out.some((e) => e.platform === platform)) throw new Error(`PINS.md "${PIN_ROW}" expected-executable table lists ${platform} twice`);
    // First-party: `yes` only when the expected value comes from a first-party source (the
    // release's own digest or provenance attestation), not from a locally observed binary.
    const fp = col(FIRST_PARTY_COLUMN) === -1 ? '' : String(row[col(FIRST_PARTY_COLUMN)] ?? '').toLowerCase();
    out.push({ platform, sha256, firstParty: /^\**yes\b/.test(fp), basis: col('Basis') === -1 ? null : row[col('Basis')] ?? null });
  }
  return out;
}

export const herdrPlatform = (platform = process.platform, arch = process.arch) => `${platform}-${arch}`;

// Compare the herdr executable the driver resolved and hashed (manifest.mjs herdrIdentity)
// with PINS.md's expected sha256 for this platform. -> { result, platform, expectedSha256,
// firstParty, basis, detail }. result: `match`; `mismatch` and `unhashed` (run.mjs: NOT RUN,
// nothing else spawned); `no-expected-value` (a finding; the run goes on, its herdr
// UNVERIFIED); `test-double` (the node-run self-test herdr: never compared, never verified).
// A `match` against a row whose firstParty is false shows only that the binary is the one
// observed locally; the record states herdr UNVERIFIED (first-party source UNVERIFIED).
export function checkHerdrExecutable(executable, expected, platform = herdrPlatform()) {
  const row = (expected ?? []).find((e) => e.platform === platform) ?? null;
  const base = { platform, expectedSha256: row?.sha256 ?? null, firstParty: row ? row.firstParty === true : null, basis: row?.basis ?? null };
  if (executable?.testDouble !== false) return { ...base, result: 'test-double', detail: 'the node-run test-double herdr is never compared with an expected value' };
  if (!/^[0-9a-f]{64}$/.test(executable.sha256 ?? '')) return { ...base, result: 'unhashed', detail: 'the herdr executable could not be hashed, so it cannot be compared with an expected value' };
  if (!row) return { ...base, result: 'no-expected-value', detail: `PINS.md "${PIN_ROW}" has no expected executable sha256 for ${platform}` };
  if (executable.sha256 !== row.sha256) return { ...base, result: 'mismatch', detail: `herdr executable sha256 ${executable.sha256} is not PINS.md's expected ${row.sha256} for ${platform}` };
  return { ...base, result: 'match', detail: `herdr executable sha256 equals PINS.md's expected value for ${platform}${row.firstParty ? '' : ' (a locally observed value; first-party source UNVERIFIED)'}` };
}

// What run.mjs does with a check (#252), kept here so it is unit-tested: -> { notRun, finding }.
// `notRun` is the NOT RUN reason (mismatch, unhashed), thrown before herdr is spawned;
// `finding` is recorded and the run goes on (no expected value, or a non-first-party match).
export function herdrCheckDecision(check) {
  if (check?.result === 'mismatch' || check?.result === 'unhashed') return { notRun: `${check.detail}. Refusing to run (#252).`, finding: null };
  if (check?.result === 'no-expected-value') return { notRun: null, finding: `herdr identity UNVERIFIED (#252): ${check.detail}; the executable was hashed (herdr.executable.sha256) but compared with nothing` };
  if (check?.result === 'match' && check.firstParty !== true) return { notRun: null, finding: `herdr identity UNVERIFIED as first-party (#252): ${check.detail}` };
  return { notRun: null, finding: null };
}

export const PINS_REL_PATH = 'docs/planning/PINS.md';

// The herdr pin as COMMITTED at HEAD (#139): the driver never accepts a herdr version on the
// strength of an uncommitted PINS.md edit. Throws (run.mjs ends the run NOT RUN) when git
// cannot read PINS.md at HEAD, when HEAD's herdr row does not parse, or when the working tree
// has an uncommitted change to the herdr pin itself: its herdr row missing, unparseable, or
// carrying a different tag from HEAD's. Any other uncommitted PINS.md change (a harness row,
// prose) does not stop the run (harness versions are never gated, #216): the pin is read from
// HEAD and `finding` says so. The working tree is compared with HEAD in git's normalized
// form, as `git status` does (#152). The expected herdr executable table (#252) is read the
// same way: an uncommitted change to it is refused like a tag change.
// -> { pin, expectedExecutables, source: { path, headCommit, committedSha256, workingTreeMatchesHead }, finding: string | null }.
export function readCommittedHerdrPin(repoRoot) {
  const f = committedFile(repoRoot, PINS_REL_PATH);
  const source = { path: PINS_REL_PATH, headCommit: f.headCommit, committedSha256: f.committedSha256, workingTreeMatchesHead: f.workingTreeMatchesHead };
  const refuse = (why) => {
    const err = new Error(`${PINS_REL_PATH} has an uncommitted change to the "${PIN_ROW}" row (${why}; HEAD ${f.headCommit}); the herdr pin is read only from the committed file, so commit or revert that edit first`);
    err.source = source;
    return err;
  };
  let pin;
  let expectedExecutables;
  try {
    pin = parseHerdrPin(f.bytes.toString('utf8'));
    expectedExecutables = parseHerdrExpectedExecutables(f.bytes.toString('utf8'));
  } catch (err) {
    err.source = source;
    throw err;
  }
  if (f.workingTreeMatchesHead) return { pin, expectedExecutables, source, finding: null };
  let onDisk;
  try {
    onDisk = readFileSync(join(repoRoot, PINS_REL_PATH), 'utf8');
  } catch {
    throw refuse('the working-tree file is missing');
  }
  let disk;
  try {
    disk = parseHerdrPin(onDisk);
  } catch (err) {
    throw refuse(`the working-tree row does not parse: ${err.message}`);
  }
  if (disk.tag !== pin.tag) throw refuse(`working tree ${disk.tag}, HEAD ${pin.tag}`);
  // #252: the expected executable sha256 table is part of the herdr pin, read only from HEAD.
  let diskExpected;
  try {
    diskExpected = parseHerdrExpectedExecutables(onDisk);
  } catch (err) {
    throw refuse(`the working-tree expected-executable table does not parse: ${err.message}`);
  }
  if (JSON.stringify(diskExpected) !== JSON.stringify(expectedExecutables)) throw refuse('the working-tree expected executable sha256 table differs from HEAD\'s');
  return {
    pin,
    expectedExecutables,
    source,
    finding: `${PINS_REL_PATH} has uncommitted changes outside the herdr pin (its "${PIN_ROW}" tag ${pin.tag} matches HEAD ${f.headCommit}); the herdr pin was read from HEAD and the run continued (#139)`,
  };
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
