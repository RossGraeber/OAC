// L3a (#189): shared helpers for the Beacon live leg (L3, #168). The L3 scenario (L3b,
// scenarios/l3-beacon.mjs) and its report (L3c, lib/l3-report.mjs) both build on this file.
//
// What is here, all pure and harness-free:
//   - probe-marker generation (one marker and one fake token per delivery path);
//   - the scratch-only augmentation of gate-servers/g5-cases.json (L3C, L3X, X4 replaced);
//   - a schema-tolerant scanner for Beacon's runtime.jsonl (and rotated siblings);
//   - redacted excerpts around a hit;
//   - the harness-config targets, whole-file and per-section hashes, and their comparison;
//   - a leak assertion that L3b's lifecycle test reuses.
// Background: docs/planning/decisions/L1-beacon-memory.md §11 items 1 and 4, §12 (B0-B7 and
// its 2026-09-30 amendment).
//
// What is NOT here: nothing launches herdr, a harness or Beacon; nothing calls, spawns or
// configures Beacon (L1 §1, §12). The only file system access is the read-and-hash in
// hashConfig(). This module never writes a file, and never reads a credential file
// (oac-boundaries mechanical check 10). Harness config content never leaves hashConfig():
// only hashes, key names and table-header names do.
//
// Probe values. A marker or fake token must never be written to stdout, stderr, the run
// manifest or a capture. Beacon's hooks also record the orchestrating Claude session's
// commands and tool results, so a value that reaches that session counts as a hit. Only ids
// and sha256 hashes are recorded. The objects makeProbeMarkers() returns serialize (JSON and
// util.inspect) to ids and hashes only, as a backstop; the values are read from the `marker`
// and `token` properties in-process and handed only to the scratch case table.
//
// Node built-ins only.

import { createHash, randomBytes } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

import { crockford128 } from '../gate-servers/g5-codex.mjs';
import { SECURITY_KEYS } from '../gate-servers/g5-channel.mjs';

export const L3_RECORD_VERSION = 1;

/**
 * The `scenarioData.l3` record L3b writes into its run manifest and L3c reads. Version 1.
 * No field ever holds a marker value, a fake-token value, or harness config content.
 *
 * @typedef {object} L3Record
 * @property {1} version                   L3_RECORD_VERSION.
 * @property {'baseline'|'probe'|'verify'} phase
 *           baseline = B0; probe = B1 record plus B2-B4; verify = B7 hash check.
 * @property {object} box                  The one 60-minute L3 box, spanning all phases.
 * @property {string} box.start            ISO time the box was declared (before B0).
 * @property {number} box.budgetMs         3600000 (L1 §12 "Timebox: 60 minutes").
 * @property {string|null} box.sourceRunId The run id whose box this phase continues
 *                                         (null for the baseline phase, which opens it).
 * @property {object} versions
 * @property {string|null} versions.beacon       `beacon version` output, or null (beaconCli=off
 *                                               and the operator did not supply it).
 * @property {string|null} versions.claudeCli    `claude --version`.
 * @property {object} versions.codex             { cli, daemon, wire }: the CLI, the daemon's
 *                                               version report, and the wire userAgent value.
 * @property {object} versions.pins              PINS.md minimum and last tested versions and
 *                                               whether the CLI differs from the last tested one:
 *                                               { claude: { minimum, lastTested, differs },
 *                                               codex: { minimum, lastTested, differs } }. Records
 *                                               made before #216 carry `lastObserved` instead. A
 *                                               difference is a VERSION WARNING finding and never
 *                                               stops the leg (versions float, #216).
 * @property {object} config                     Harness config, hashes only.
 * @property {Array<ConfigHash>} config.hashes   hashConfig() output for this phase.
 * @property {object|null} config.compare        compareSections(baseline, this phase), or null
 *                                               for the baseline phase.
 * @property {Array<string>} config.findings     harnessConfigTargets() findings, e.g. that
 *                                               CLAUDE_CONFIG_DIR is set (name only).
 * @property {Array<MarkerRecord>} markers       markerRecords(): ids and sha256 only.
 * @property {object|null} scan                  scanRuntimeLog() summary for the probe phase:
 *                                               { files, byMarker, bySession } (no excerpts;
 *                                               L3c builds redacted excerpts itself).
 * @property {Array<string>} findings            Free-text findings, redacted, value-free.
 *
 * Added by L3b (scenarios/l3-beacon.mjs, #190) within version 1 (additive; nothing above changed):
 * @property {'driver'|'human'} acceptPolicy     The dialog accept policy (#196: `driver` by
 *                                               default; each dialog's own acceptOrigin says
 *                                               who accepted it).
 * @property {object} steps                      { B0..B7: { status: 'recorded'|'NOT RUN', ... } } per phase.
 * @property {object|null} scan.counts           Probe phase: hitCounts() per delivery path:
 *                                               { lines, markerLines, tokenLines, byAction,
 *                                               byPath, byCollectionMethod, byHarness }.
 * @property {Array<object>} scan.excerpts       One redactedExcerpt() per hit (first 100):
 *                                               { file, line, markerId, eventAction, text }, text
 *                                               in the manifest-safe {L3-MARKER <id>} form.
 * @property {object} steps.B2.log / B3.log / B4.log  Log snapshots: { cumulative, delta }, each
 *                                               { byMarker, counts, bySession, warnings }.
 * @property {object} beacon.sync                { B2, B3, B4 }: `sync --print` poll-path results,
 *                                               { harness, exitCode, lines, truncated, byMarker,
 *                                               counts, bySession }, or { status: 'NOT RUN' }.
 * @property {object|null} probeSessions         { claude: string[]|null, codex: string[] }:
 *                                               harness-side ids of the probe sessions.
 * @property {object|null} sessionFile           { read, dirsFound, files, sessionIds, lines,
 *                                               entries: [{ parsed, type, isMeta,
 *                                               attachmentType, attachmentKeys, markerPresent,
 *                                               tokenPresent }] } (types and flags only).
 */

/**
 * @typedef {object} MarkerRecord
 * @property {string} id           Delivery path: claude-channel | codex-turn-start | codex-queue-add.
 * @property {string} markerSha256 sha256 (hex) of the marker value.
 * @property {string} tokenSha256  sha256 (hex) of the fake token value.
 */

// --- probe markers ---------------------------------------------------------------------

// One per delivery path (L1 §12): B2 channel notification; B4 `deliver` (turn/start); B4 `x4`
// (thread/queue/add).
export const DELIVERY_PATHS = Object.freeze(['claude-channel', 'codex-turn-start', 'codex-queue-add']);

// Search is always for the FULL value; these prefixes are never searched for. L1 §12 itself
// contains them, so any agent that reads it creates prefix-only lines.
const MARKER_PREFIX = 'L3-PROBE-';
const TOKEN_PREFIX = 'sk-l3fake-';
export const MARKER_SHAPE = /^L3-PROBE-[0-9a-z]{26}$/;
export const TOKEN_SHAPE = /^sk-l3fake-[0-9a-z]{26}$/;

export const sha256 = (s) => createHash('sha256').update(s).digest('hex');

const INSPECT = Symbol.for('nodejs.util.inspect.custom');

/**
 * One marker and one fake token per delivery path, from the OS CSPRNG (26 lowercase
 * Crockford Base32 characters from 16 random bytes, as the G5 delimiter). `marker` and
 * `token` are NON-enumerable properties, so entries walks, spread and Object.assign see
 * { id, markerSha256, tokenSha256 } only; JSON and util.inspect are overridden to match.
 * @param {{ rand?: (n: number) => Buffer }} [opts]
 * @returns {Array<{ id: string, marker: string, token: string, markerSha256: string, tokenSha256: string }>}
 */
export function makeProbeMarkers({ rand = randomBytes } = {}) {
  const seen = new Set();
  const draw = (prefix) => {
    const v = prefix + crockford128(rand(16));
    if (seen.has(v)) throw new Error('makeProbeMarkers: the random source repeated a value; refusing duplicate probe values');
    seen.add(v);
    return v;
  };
  return DELIVERY_PATHS.map((id) => {
    const marker = draw(MARKER_PREFIX);
    const token = draw(TOKEN_PREFIX);
    // Only id and hashes are enumerable: the values are unreachable by Object.entries /
    // Object.keys walks (redactValue, the run-manifest writer), spread and Object.assign.
    const m = { id, markerSha256: sha256(marker), tokenSha256: sha256(token) };
    Object.defineProperty(m, 'marker', { value: marker, enumerable: false });
    Object.defineProperty(m, 'token', { value: token, enumerable: false });
    const safe = () => ({ id, markerSha256: m.markerSha256, tokenSha256: m.tokenSha256 });
    Object.defineProperty(m, 'toJSON', { value: safe, enumerable: false });
    Object.defineProperty(m, INSPECT, { value: () => `L3ProbeMarker ${JSON.stringify(safe())}`, enumerable: false });
    return Object.freeze(m);
  });
}

/** Ids and hashes only: what a record may carry. */
export const markerRecords = (markers) => markers.map((m) => ({ id: m.id, markerSha256: m.markerSha256, tokenSha256: m.tokenSha256 }));

const markerById = (markers, id) => {
  const m = markers.find((x) => x.id === id);
  if (!m) throw new Error(`no probe marker for delivery path ${id}`);
  return m;
};

/** The benign probe body for one delivery path. */
export const probeBody = (m) => `L3 probe ${m.marker}. Test credential ${m.token}. No action needed.`;

// --- scratch case table ----------------------------------------------------------------

export const L3_CLAUDE_CASE = 'L3C';
export const L3_CODEX_CASE = 'L3X';
// The X4 setup turn keeps its job (a long turn, so thread/queue/add lands mid-turn) and
// carries no probe value, so a hit on the queue-add marker can only come from the queued
// input, never from the setup turn/start.
export const L3_X4_SETUP_TEXT = 'L3 setup (not a probe): write a numbered list of 30 distinct one-sentence facts about lighthouses. Do not use any tools.';

/**
 * A new case table for the scratch copy of gate-servers/ (L1 §12 "Probe content"). Adds a
 * Claude case L3C (all five SECURITY_KEYS in meta, so g5-channel.mjs presend() does not
 * refuse it) and a framed Codex case L3X (X1's call/header/body shape), and replaces X4's
 * body with the codex-queue-add probe (g5-codex.mjs x4 only ever sends X4). The input is
 * not mutated; the committed gate-servers/ files are never edited.
 */
export function augmentCaseTable(table, markers) {
  const t = structuredClone(table);
  if (!Array.isArray(t.claude) || !Array.isArray(t.codex)) throw new Error('augmentCaseTable: not a g5-cases.json table');
  for (const id of [L3_CLAUDE_CASE, L3_CODEX_CASE]) {
    if ([...t.claude, ...t.codex].some((c) => c.id === id)) throw new Error(`augmentCaseTable: case ${id} already present`);
  }
  const c1 = t.claude.find((c) => c.id === 'C1');
  const x1 = t.codex.find((c) => c.id === 'X1');
  const x4 = t.codex.find((c) => c.id === 'X4');
  if (!c1 || !x1 || !x4) throw new Error('augmentCaseTable: C1, X1 and X4 are required');

  const claudeMeta = {};
  for (const k of SECURITY_KEYS) claudeMeta[k] = k === 'oac_message_id' ? 'l3-c' : k === 'oac_reply_to' ? '' : String(c1.meta?.[k] ?? '');
  t.claude.push({ id: L3_CLAUDE_CASE, content: probeBody(markerById(markers, 'claude-channel')), meta: claudeMeta, l3: 'claude-channel' });

  t.codex.push({
    id: L3_CODEX_CASE,
    call: x1.call,
    header: { ...x1.header, oac_message_id: 'l3-x', oac_reply_to: '' },
    body: probeBody(markerById(markers, 'codex-turn-start')),
    l3: 'codex-turn-start',
  });

  x4.body = probeBody(markerById(markers, 'codex-queue-add'));
  x4.setupText = L3_X4_SETUP_TEXT;
  x4.header = { ...x4.header, oac_message_id: 'l3-x4' };
  x4.l3 = 'codex-queue-add';

  t.note = `SCRATCH COPY for the L3 Beacon live leg (#168); never commit. ${t.note ?? ''}`.trim();
  return t;
}

// --- runtime.jsonl scanner -------------------------------------------------------------

// Beacon field names used here, cited at the pinned tag (L1 §11 convention; each citation is
// https://github.com/Asymptote-Labs/agent-beacon/blob/v1.3.29/<path>, read 2026-09-30):
//   - one Event per line, encoding/json Encoder: pkg/asymptoteobserve/jsonl_sink.go@v1.3.29 L57-59;
//   - `event` (Event.Event): pkg/asymptoteobserve/event.go@v1.3.29 L503;
//     `event.action`: same file L57 (EventInfo.Action);
//   - `harness` (Event.Harness): L507; `harness.name` L77, `harness.version` L78,
//     `harness.collection_method` L84 (HarnessInfo);
//   - `session` (Event.Session): L511; `session.id` L88 (SessionInfo);
//   - `prompt.text`: L524 (Event.Prompt), L227 (PromptInfo.Text).
//   The collector's exporter uses its own Event type with the same `event`, `harness`,
//   `session` and `prompt` tags and aliases EventInfo / HarnessInfo / SessionInfo /
//   PromptInfo to the ones above
//   (collector-builder/exporter/beaconjsonexporter/internal/beaconevent/event.go@v1.3.29
//   L15, L18, L19, L40, L80, L84, L88, L100).
//   UNVERIFIED: the rotated-archive file names of runtime.jsonl (the JSONL sink at
//   jsonl_sink.go@v1.3.29 L21 is "non-rotating"; whatever rotates the log was not read).
//   The scanner therefore takes a list of files and assumes nothing about their names.
// Path-valued fields (harness.executable_path, harness.config_path,
// session.working_directory; event.go@v1.3.29 L79-80, L89) hold home paths, so only their
// presence is recorded, never their value.
const HARNESS_ID_FIELDS = Object.freeze(['name', 'version', 'collection_method']);
const SESSION_ID_FIELDS = Object.freeze(['id']);

function splitLines(buf) {
  const out = [];
  let start = 0;
  for (let i = 0; i <= buf.length; i++) {
    if (i === buf.length || buf[i] === 0x0a) {
      if (i > start || i < buf.length) out.push({ offset: start, bytes: buf.subarray(start, i) });
      start = i + 1;
    }
  }
  return out;
}

// Replace every marker / token value in `s` by its placeholder.
export function substituteProbes(s, markers) {
  let out = String(s);
  for (const m of markers) {
    out = out.split(m.marker).join(`<L3-MARKER:${m.id}>`);
    out = out.split(m.token).join(`<L3-FAKE-TOKEN:${m.id}>`);
  }
  return out;
}

// Dotted JSON paths whose value (or key) holds `needle`.
function pathsHolding(v, needle, path = '', out = []) {
  if (typeof v === 'string') {
    if (v.includes(needle)) out.push(path || '$');
  } else if (Array.isArray(v)) {
    v.forEach((x, i) => pathsHolding(x, needle, `${path}[${i}]`, out));
  } else if (v && typeof v === 'object') {
    for (const [k, x] of Object.entries(v)) {
      const p = path ? `${path}.${k}` : k;
      if (k.includes(needle)) out.push(`${p}<key>`);
      pathsHolding(x, needle, p, out);
    }
  }
  return out;
}

function pick(obj, fields) {
  if (!obj || typeof obj !== 'object' || Array.isArray(obj)) return { present: false, ids: {}, otherFields: [] };
  const ids = {};
  for (const f of fields) if (typeof obj[f] === 'string' || typeof obj[f] === 'number') ids[f] = String(obj[f]);
  return { present: true, ids, otherFields: Object.keys(obj).filter((k) => !fields.includes(k)).sort() };
}

/**
 * Scan Beacon runtime.jsonl content for the FULL marker and token values.
 *
 * @param {string|Buffer|Array<{ label: string, text: string|Buffer, fromByte?: number }>} input
 *        One log's content, or a list of files (runtime.jsonl plus any rotated siblings).
 * @param {Array} markers makeProbeMarkers() output.
 * @param {{ fromByte?: number }} [opts] Default start offset for inputs that carry none. A
 *        line is scanned only if it STARTS at or after its file's fromByte.
 * @returns {{ files, hits, byMarker, bySession, warnings }} Value-free: ids, paths (probe values in a
 *        path are substituted), `event.action`, harness/session identifiers, byte offsets.
 */
export function scanRuntimeLog(input, markers, { fromByte = 0 } = {}) {
  const inputs = Array.isArray(input) ? input : [{ label: 'runtime.jsonl', text: input }];
  const files = [];
  const hits = [];
  const warnings = [];
  for (const f of inputs) {
    const buf = Buffer.isBuffer(f.text) ? f.text : Buffer.from(String(f.text ?? ''), 'utf8');
    const start = f.fromByte ?? fromByte;
    // A start past the end means the log was rotated or truncated since the offset was
    // taken: every line would be skipped and "no hits" would read as a clean negative.
    const info = { label: f.label, bytes: buf.length, fromByte: start, startBeyondEnd: start > buf.length, linesScanned: 0, linesSkipped: 0, unparsedLines: 0, hitLines: 0 };
    if (info.startBeyondEnd) warnings.push(`${f.label}: fromByte ${start} is past the end of the file (${buf.length} bytes); it was rotated or truncated, so no line was scanned; scan its rotated sibling`);
    splitLines(buf).forEach(({ offset, bytes }, idx) => {
      if (offset < start) {
        info.linesSkipped += 1;
        return;
      }
      info.linesScanned += 1;
      const line = bytes.toString('utf8');
      const present = markers.filter((m) => line.includes(m.marker) || line.includes(m.token));
      if (!present.length) return;
      info.hitLines += 1;
      let rec = null;
      try {
        rec = JSON.parse(line);
      } catch {
        info.unparsedLines += 1;
      }
      const parsed = rec !== null && typeof rec === 'object';
      const harness = parsed ? pick(rec.harness, HARNESS_ID_FIELDS) : { present: false, ids: {}, otherFields: [] };
      const session = parsed ? pick(rec.session, SESSION_ID_FIELDS) : { present: false, ids: {}, otherFields: [] };
      const action = parsed && rec.event && typeof rec.event.action === 'string' ? rec.event.action : null;
      for (const m of present) {
        const clean = (p) => substituteProbes(p, markers);
        hits.push({
          file: f.label,
          line: idx + 1,
          byteOffset: offset,
          markerId: m.id,
          markerPresent: line.includes(m.marker),
          tokenVerbatim: line.includes(m.token),
          parsed,
          markerPaths: parsed ? pathsHolding(rec, m.marker).map(clean) : [],
          tokenPaths: parsed ? pathsHolding(rec, m.token).map(clean) : [],
          eventAction: action,
          harness: harness.ids,
          harnessOtherFields: harness.otherFields,
          session: session.ids,
          sessionOtherFields: session.otherFields,
        });
      }
    });
    files.push(info);
  }

  const byMarker = {};
  for (const m of markers) {
    const hs = hits.filter((h) => h.markerId === m.id);
    byMarker[m.id] = {
      lines: hs.length,
      markerLines: hs.filter((h) => h.markerPresent).length,
      tokenVerbatimLines: hs.filter((h) => h.tokenVerbatim).length,
      actions: [...new Set(hs.map((h) => h.eventAction ?? '<none>'))].sort(),
      paths: [...new Set(hs.flatMap((h) => [...h.markerPaths, ...h.tokenPaths]))].sort(),
      collectionMethods: [...new Set(hs.map((h) => h.harness.collection_method ?? '<none>'))].sort(),
    };
  }

  const groups = new Map();
  for (const h of hits) {
    const key = JSON.stringify([h.harness.name ?? null, h.session.id ?? null]);
    if (!groups.has(key)) groups.set(key, { harness: h.harness.name ?? null, sessionId: h.session.id ?? null, lines: new Set(), markerIds: new Set(), tokenVerbatim: false, actions: new Set() });
    const g = groups.get(key);
    g.lines.add(`${h.file}:${h.line}`);
    g.markerIds.add(h.markerId);
    g.tokenVerbatim ||= h.tokenVerbatim;
    g.actions.add(h.eventAction ?? '<none>');
  }
  const bySession = [...groups.values()].map((g) => ({ harness: g.harness, sessionId: g.sessionId, lines: g.lines.size, markerIds: [...g.markerIds].sort(), tokenVerbatim: g.tokenVerbatim, actions: [...g.actions].sort() }));

  return { files, hits, byMarker, bySession, warnings };
}

// --- redacted excerpts -----------------------------------------------------------------

export const EXCERPT_CHARS = 160;

// The id placeholders an excerpt carries. To the redactor's scan, `<L3-FAKE-TOKEN:<id>>` reads
// as a secret assignment (NAME-TOKEN:value), so a leak scan of an excerpt (here, in tests and
// in L3c's leak guard) runs on neutralizePlaceholders(text).
// Not global, so .test() is stateless; callers that replace or iterate build a /g copy.
export const PLACEHOLDER_RE = /<L3-(?:MARKER|FAKE-TOKEN):[a-z-]+>/;
export const neutralizePlaceholders = (text) => String(text).replace(new RegExp(PLACEHOLDER_RE.source, 'g'), 'ZQL3PQZ');

/**
 * At most EXCERPT_CHARS characters around a hit. Markers become <L3-MARKER:<id>>, fake tokens
 * <L3-FAKE-TOKEN:<id>> (substituted over the whole line BEFORE the window is cut, so no
 * partial value survives at an edge, and no placeholder is cut), and the whole line goes through createRedactor()'s
 * redactText (home paths, usernames, hosts, other token shapes) before the window is cut
 * around the hit's placeholder. If anything still scans
 * as a residual, or a probe value survives, the excerpt is withheld whole.
 * @param {string} line   The raw log line.
 * @param {{ markerId?: string }|null} hit  Centre on this marker's placeholder (else the first).
 * @param {Array} markers
 * @param {ReturnType<import('./redact.mjs').createRedactor>} redactor
 */
export function redactedExcerpt(line, hit, markers, redactor) {
  const WITHHELD = '<WITHHELD: excerpt still carried a residual after redaction>';
  // Probe values first become inert sentinels: the final placeholders look like a
  // `NAME-TOKEN:value` assignment to the redactor, which would rewrite them. The whole line is
  // then redacted BEFORE the window is cut, so an identity or home path at a window edge
  // cannot survive as a fragment the redactor no longer recognizes.
  const sentinel = (kind, i) => `ZQL3${kind}${i}QZ`;
  let s = String(line).replace(/[\r\n]+/g, ' ');
  markers.forEach((m, i) => {
    s = s.split(m.marker).join(sentinel('M', i)).split(m.token).join(sentinel('T', i));
  });
  let red = redactor.redactText(s).text;
  if (!red) return WITHHELD;
  markers.forEach((m, i) => {
    red = red.split(sentinel('M', i)).join(`<L3-MARKER:${m.id}>`).split(sentinel('T', i)).join(`<L3-FAKE-TOKEN:${m.id}>`);
  });
  const PH = new RegExp(PLACEHOLDER_RE.source, 'g');
  const spans = [...red.matchAll(PH)].map((x) => [x.index, x.index + x[0].length]);
  const want = hit?.markerId;
  let at = want ? red.indexOf(`<L3-MARKER:${want}>`) : -1;
  if (at === -1 && want) at = red.indexOf(`<L3-FAKE-TOKEN:${want}>`);
  if (at === -1) at = spans.length ? spans[0][0] : 0;
  let from = Math.max(0, Math.min(at - Math.floor(EXCERPT_CHARS / 2), red.length - EXCERPT_CHARS));
  let to = Math.min(red.length, from + EXCERPT_CHARS);
  // Never cut through a placeholder.
  for (const [a, b] of spans) {
    if (from > a && from < b) from = a;
    if (to > a && to < b) to = b;
  }
  const text = red.slice(from, to);
  // Scan with the placeholders neutralized (they are ours, not residuals).
  const scan = redactor.scan(neutralizePlaceholders(text));
  if (scan.residualLeaks.length || scan.residualGenericHits.length || findMarkerLeaks(text, markers).length) return WITHHELD;
  return text;
}

// --- leak assertion --------------------------------------------------------------------

/** Ids and kinds of every probe value present in `text`; never the values. */
export function findMarkerLeaks(text, markers) {
  const s = String(text ?? '');
  const out = [];
  for (const m of markers) {
    if (s.includes(m.marker)) out.push({ id: m.id, what: 'marker' });
    if (s.includes(m.token)) out.push({ id: m.id, what: 'token' });
  }
  return out;
}

export class MarkerLeakError extends Error {}

/**
 * Throws MarkerLeakError when `text` (stdout, stderr, a manifest, a capture) holds any
 * marker or fake-token value. The message names ids and kinds only. L3b's lifecycle test
 * reuses this on every output of a run.
 */
export function assertNoMarkerLeak(text, markers, where = 'output') {
  const leaks = findMarkerLeaks(text, markers);
  if (leaks.length) throw new MarkerLeakError(`probe value leaked into ${where}: ${leaks.map((l) => `${l.id} ${l.what}`).join(', ')}`);
  return true;
}

// --- harness config: targets, hashes, sections -----------------------------------------

/**
 * The four files L1 §12 B0 names, at Beacon's FIXED $HOME paths: Beacon ignores
 * CLAUDE_CONFIG_DIR and CODEX_HOME (L1 §11 item 4;
 * cli/beacon/internal/endpoint/harness/harness.go@v1.3.29 L364, L404). When
 * either variable is set, the env-dir equivalents are added too, with a finding (the
 * variable's NAME only). Read-and-hash only: these paths go to hashConfig() and nowhere
 * else; never hand one to a write (oac-boundaries mechanical check 10).
 * @returns {{ targets: Array<{ label: string, kind: string, path: string }>, findings: string[] }}
 */
export function harnessConfigTargets(env = process.env, { home = homedir() } = {}) {
  const targets = [
    { label: '~/.claude/settings.json', kind: 'claude-settings', path: join(home, '.claude', 'settings.json') },
    { label: '~/.claude.json', kind: 'claude-state', path: join(home, '.claude.json') },
    { label: '~/.codex/config.toml', kind: 'codex-config', path: join(home, '.codex', 'config.toml') },
    { label: '~/.codex/hooks.json', kind: 'codex-hooks', path: join(home, '.codex', 'hooks.json') },
  ];
  const findings = [];
  if (env.CLAUDE_CONFIG_DIR) {
    targets.push({ label: '$CLAUDE_CONFIG_DIR/settings.json', kind: 'claude-settings', path: join(env.CLAUDE_CONFIG_DIR, 'settings.json') });
    // UNVERIFIED: that Claude Code keeps .claude.json inside CLAUDE_CONFIG_DIR when it is set.
    targets.push({ label: '$CLAUDE_CONFIG_DIR/.claude.json', kind: 'claude-state', path: join(env.CLAUDE_CONFIG_DIR, '.claude.json') });
    findings.push('CLAUDE_CONFIG_DIR is set: Claude Code may read a settings file Beacon does not write (Beacon writes ~/.claude/settings.json whatever it says; L1 §11 item 4). Its files are hashed as well.');
  }
  if (env.CODEX_HOME) {
    targets.push({ label: '$CODEX_HOME/config.toml', kind: 'codex-config', path: join(env.CODEX_HOME, 'config.toml') });
    targets.push({ label: '$CODEX_HOME/hooks.json', kind: 'codex-hooks', path: join(env.CODEX_HOME, 'hooks.json') });
    findings.push('CODEX_HOME is set: Codex may read a config.toml Beacon does not write (Beacon writes ~/.codex/config.toml whatever it says; L1 §11 item 4). Its files are hashed as well.');
  }
  return { targets, findings };
}

/**
 * @typedef {object} ConfigHash
 * @property {string} label
 * @property {string} kind
 * @property {boolean} present
 * @property {string|null} sha256        Whole file.
 * @property {object|null} sections      { [section]: sha256 | null } (see sectionHashes).
 * @property {boolean} [parseError]
 * @property {object} [trailingComment]  config.toml only: trailingCommentHeaders().
 * @property {string} [error]            Read error code, never content.
 */

/** Whole-file sha256 plus per-section hashes. Reads each target; returns no content. */
export function hashConfig(targets) {
  return targets.map(({ label, kind, path }) => {
    if (!existsSync(path)) return { label, kind, present: false, sha256: null, sections: null };
    let buf;
    try {
      buf = readFileSync(path);
    } catch (err) {
      return { label, kind, present: true, sha256: null, sections: null, error: err.code || 'read failed' };
    }
    const text = buf.toString('utf8');
    const out = { label, kind, present: true, sha256: createHash('sha256').update(buf).digest('hex'), ...sectionHashes(kind, text) };
    if (kind === 'codex-config') out.trailingComment = trailingCommentHeaders(text);
    return out;
  });
}

// Canonical JSON: keys sorted at every depth, no whitespace. Formatting-only re-encoding
// (key order, indentation, Go's json.MarshalIndent; L1 §11 item 4 side effect 2) hashes equal.
function canonical(v) {
  if (Array.isArray(v)) return `[${v.map(canonical).join(',')}]`;
  if (v && typeof v === 'object') return `{${Object.keys(v).sort().map((k) => `${JSON.stringify(k)}:${canonical(v[k])}`).join(',')}}`;
  return JSON.stringify(v);
}
const hashValue = (v) => sha256(canonical(v));

// Beacon's own header test at the tag: trimmed line starts with `[` and ends with `]`
// (cli/beacon/internal/endpoint/harness/harness.go@v1.3.29 L471-485, per L1 §11 item 4).
const beaconSeesHeader = (line) => {
  const t = line.trim();
  return t.startsWith('[') && t.endsWith(']');
};
// A TOML table / array-of-tables header, with or without a trailing comment.
const TOML_HEADER = /^\s*(\[\[?[^\]]*\]\]?)\s*(#.*)?$/;
// Beacon's otel test on a recognized (trimmed) header: `header == "[otel]" ||
// strings.HasPrefix(header, "[otel.")`
// (cli/beacon/internal/endpoint/harness/harness.go@v1.3.29 L501-503).
const isOtel = (h) => h === '[otel]' || h.startsWith('[otel.');

// A config.toml header as a section key. Bare dotted headers (`[otel]`, `[mcp_servers.oac]`,
// `[[x.y]]`) are kept verbatim. Any other header, e.g. Codex's `[projects.'<absolute path>']`,
// would carry a home path and username into the key, which run-manifest redaction then
// rewrites, so a baseline read back from a manifest would show spurious added/removed
// sections. Those are keyed by their leading bare segments plus a hash of the rest:
// `[projects.<sha256:0123456789abcdef>]`.
export function sectionKey(header) {
  const compact = header.replace(/\s+/g, '');
  if (/^\[\[?[A-Za-z0-9_.-]+\]\]?$/.test(compact)) return compact;
  const h = header.trim();
  const open = h.startsWith('[[') ? '[[' : '[';
  const close = open === '[[' && h.endsWith(']]') ? ']]' : ']';
  const inner = h.slice(open.length, h.length - close.length);
  const lead = /^\s*(?:[A-Za-z0-9_-]+\s*\.\s*)*/.exec(inner)[0];
  const rest = inner.slice(lead.length);
  return `${open}${lead.replace(/\s+/g, '')}<sha256:${sha256(rest.trim()).slice(0, 16)}>${close}`;
}

/**
 * Per-section hashes, no values.
 *   codex-config (config.toml): one hash per table header, keyed by sectionKey(): bare
 *     dotted header text verbatim (`[otel]`, `[mcp_servers.oac]`), any other header as its
 *     leading bare segments plus a hash of the rest; repeated keys get `#2`, ...; lines
 *     before the first header are `<root>`. Trailing comments are not part of the key.
 *   claude-settings (settings.json): `key:<k>` per top-level key, plus `env.<name>` and
 *     `hooks.<event>` per key inside `env` and `hooks`. Canonical JSON, so formatting-only
 *     re-encoding does not change a section.
 *   claude-state (~/.claude.json): KEY NAMES ONLY, `key:<k>` -> null. The file holds account
 *     data, so no per-key value hash is kept; the whole-file hash says whether it changed.
 *   codex-hooks (hooks.json): `key:<k>` per top-level key.
 * @returns {{ sections: object, parseError?: true }}
 */
export function sectionHashes(kind, text) {
  const src = String(text).replace(/^﻿/, '');
  if (kind === 'codex-config') {
    const sections = {};
    const order = [];
    let name = '<root>';
    let lines = [];
    const flush = () => {
      if (name === '<root>' && lines.every((l) => l.trim() === '')) return;
      let key = name;
      for (let n = 2; Object.hasOwn(sections, key); n++) key = `${name}#${n}`;
      sections[key] = sha256(lines.map((l) => l.replace(/\s+$/, '')).join('\n'));
      order.push(key);
    };
    for (const line of src.split(/\r?\n/)) {
      const m = TOML_HEADER.exec(line);
      if (m) {
        flush();
        name = sectionKey(m[1]);
        lines = [];
      } else lines.push(line);
    }
    flush();
    return { sections };
  }
  let obj;
  try {
    obj = JSON.parse(src);
  } catch {
    return { sections: { '<unparseable>': sha256(src) }, parseError: true };
  }
  if (!obj || typeof obj !== 'object' || Array.isArray(obj)) return { sections: { '<non-object>': hashValue(obj) } };
  const sections = {};
  for (const k of Object.keys(obj).sort()) sections[`key:${k}`] = kind === 'claude-state' ? null : hashValue(obj[k]);
  if (kind === 'claude-settings') {
    for (const inner of ['env', 'hooks']) {
      const v = obj[inner];
      if (v && typeof v === 'object' && !Array.isArray(v)) for (const k of Object.keys(v).sort()) sections[`${inner}.${k}`] = hashValue(v[k]);
    }
  }
  return { sections };
}

/**
 * config.toml headers with a trailing comment (`^\s*\[[^\]]*\]\s*#`), and whether each
 * follows an `[otel]` / `[otel.*]` section, i.e. the nearest header above it that Beacon's
 * merge recognizes is an otel table. Such a header and its keys are dropped by Beacon's
 * `[otel]` merge (L1 §11 item 4 edge case). Line numbers are 1-based.
 * @returns {{ headers: Array<{ line: number, afterOtel: boolean }>, anyAfterOtel: boolean }}
 */
export function trailingCommentHeaders(text) {
  const headers = [];
  let lastRecognized = null;
  String(text).split(/\r?\n/).forEach((line, i) => {
    if (/^\s*\[[^\]]*\]\s*#/.test(line)) headers.push({ line: i + 1, afterOtel: lastRecognized !== null && isOtel(lastRecognized) });
    if (beaconSeesHeader(line)) lastRecognized = line.trim();
  });
  return { headers, anyAfterOtel: headers.some((h) => h.afterOtel) };
}

/**
 * Which files and sections changed, were added or were removed between two hashConfig()
 * results (matched by label).
 */
export function compareSections(before, after) {
  const labels = [...new Set([...before.map((x) => x.label), ...after.map((x) => x.label)])];
  const files = labels.map((label) => {
    const b = before.find((x) => x.label === label);
    const a = after.find((x) => x.label === label);
    const bp = !!b?.present;
    const ap = !!a?.present;
    let status;
    if (!bp && !ap) status = 'absent';
    else if (!bp) status = 'added';
    else if (!ap) status = 'removed';
    else status = b.sha256 === a.sha256 ? 'unchanged' : 'changed';
    const bs = b?.sections ?? {};
    const as = a?.sections ?? {};
    const sections = {
      changed: Object.keys(bs).filter((k) => Object.hasOwn(as, k) && bs[k] !== as[k]).sort(),
      added: Object.keys(as).filter((k) => !Object.hasOwn(bs, k)).sort(),
      removed: Object.keys(bs).filter((k) => !Object.hasOwn(as, k)).sort(),
    };
    const formattingOnly = status === 'changed' && !sections.changed.length && !sections.added.length && !sections.removed.length && !a?.parseError && !b?.parseError && a?.kind !== 'claude-state';
    return { label, status, formattingOnly, sections };
  });
  return { unchanged: files.every((f) => f.status === 'unchanged' || f.status === 'absent'), files };
}
