#!/usr/bin/env node
// Compare two G1 channel-server wire transcripts (Epic K, K4 #127).
//
//   node tools/herdr/lib/compare-transcripts.mjs <baseline.jsonl> <candidate.jsonl>
//        [--segment all|last] [--detail] [--json]
//
// Input: the JSONL the quarantined G1 channel server writes, one
// `{ "t": <ISO>, "direction": "spike"|"client->server"|"server->client", "payload": {...} }`
// per line (docs/planning/gates/fixtures/g1-claude-wake/channel-server.mjs.throwaway-quarantined,
// function log()). Every committed G1 transcript has this shape.
//
// What it does:
//   - normalizes what differs between any two runs by construction: wall-clock timestamps
//     (turned into offsets from the segment's first frame), JSON-RPC ids (renumbered in
//     order of first appearance), Claude Code tool-use ids, progress tokens, and process ids;
//   - reduces each frame to a method-level event key ("c->s request initialize",
//     "s->c result (initialize)", "s->c notification notifications/claude/channel", ...);
//   - diffs the two event sequences (longest common subsequence) and reports what is only
//     in the baseline (-) and only in the candidate (+);
//   - extracts per-transcript facts the G1 criteria are scored on (negotiated revision,
//     declared claude/channel capability, channel notifications with their meta keys, reply
//     tool calls), so a report can put both runs side by side.
//
// `--segment last` compares only the last channel-server instance in each file (from its
// last "server started" note onward). Box C holds two instances; only the second (lines
// 11-25) carries pass-criteria evidence (G1-result.md "Command transcript summary").
//
// Pure functions, Node built-ins only. It reads two files and prints; it never writes.

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export class TranscriptError extends Error {}

const DIRECTIONS = new Set(['spike', 'client->server', 'server->client']);
const SHORT = { 'client->server': 'c->s', 'server->client': 's->c' };
const START_NOTE = /channel server started/i;

// -> [{ line, t, direction, payload }] ; throws TranscriptError naming the bad line.
export function parseTranscript(text) {
  const entries = [];
  String(text)
    .split(/\r?\n/)
    .forEach((raw, i) => {
      if (!raw.trim()) return;
      let rec;
      try {
        rec = JSON.parse(raw);
      } catch (err) {
        throw new TranscriptError(`line ${i + 1}: not JSON (${err.message})`);
      }
      if (!rec || typeof rec !== 'object' || !DIRECTIONS.has(rec.direction) || typeof rec.payload !== 'object' || rec.payload === null) {
        throw new TranscriptError(`line ${i + 1}: not a channel-server transcript record ({t, direction, payload})`);
      }
      entries.push({ line: i + 1, t: typeof rec.t === 'string' ? rec.t : null, direction: rec.direction, payload: rec.payload });
    });
  return entries;
}

// Split at every "channel server started" note. A preamble before the first note (if
// any) is its own segment.
export function segmentInstances(entries) {
  const segs = [];
  let cur = [];
  for (const e of entries) {
    if (e.direction === 'spike' && START_NOTE.test(String(e.payload.note ?? '')) && cur.length) {
      segs.push(cur);
      cur = [];
    }
    cur.push(e);
  }
  if (cur.length) segs.push(cur);
  return segs;
}

export function selectSegment(entries, segment = 'all') {
  if (segment === 'all') return entries;
  if (segment === 'last') return segmentInstances(entries).at(-1) ?? [];
  throw new TranscriptError(`unknown segment "${segment}" (all, last)`);
}

// A `meta` key Claude Code keeps as a <channel> attribute: letters, digits, underscore
// (oac-claude-channels §2; G1 reference "meta keys must be identifier-safe").
export const isIdentifierSafe = (k) => /^[A-Za-z0-9_]+$/.test(k);

// Channel message ids are `g1-spike-<kind>-<seq>`; the kind survives normalization.
export function messageKind(id) {
  const m = /^g1-spike-(.+)-\d+$/.exec(String(id ?? ''));
  return m ? m[1] : null;
}

// Legacy MCP revisions are date strings no later than 2025-11-25 (G1 reference, criterion 1).
export function isLegacyRevision(v) {
  return typeof v === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(v) && v <= '2025-11-25';
}

function idKey(id) {
  return JSON.stringify(id);
}

// Event keys for a sequence of entries. Responses are labelled with the method of the
// request they answer (matched by id, opposite direction).
export function eventKeys(entries, { detail = false } = {}) {
  const pending = new Map();
  return entries.map((e) => {
    if (e.direction === 'spike') {
      const note = String(e.payload.note ?? '(no note)')
        .replace(/\s+—.*$/, '')
        .trim();
      return `spike: ${note}`;
    }
    const p = e.payload;
    const dir = SHORT[e.direction];
    if (typeof p.method === 'string' && p.id !== undefined) {
      pending.set(`${dir} ${idKey(p.id)}`, p.method);
      const tool = p.method === 'tools/call' && detail ? ` [${p.params?.name ?? '?'}]` : '';
      return `${dir} request ${p.method}${tool}`;
    }
    if (typeof p.method === 'string') {
      const kind = detail && p.method === 'notifications/claude/channel' ? ` [${messageKind(p.params?.meta?.oac_message_id) ?? '?'}]` : '';
      return `${dir} notification ${p.method}${kind}`;
    }
    if ('result' in p || 'error' in p) {
      const other = dir === 'c->s' ? 's->c' : 'c->s';
      const method = pending.get(`${other} ${idKey(p.id)}`) ?? '?';
      return p.error ? `${dir} error ${p.error.code ?? '?'} (${method})` : `${dir} result (${method})`;
    }
    return `${dir} unrecognized frame`;
  });
}

// Normalized copies of the entries: timestamps -> offset from the first frame, JSON-RPC
// ids -> <ID:n>, tool-use ids -> <TOOL_USE_ID>, progress tokens -> <PROGRESS_TOKEN>,
// pids -> <PID>. The input is not modified.
export function normalizeEntries(entries) {
  const ids = new Map();
  const t0 = entries.find((e) => e.t)?.t;
  const base = t0 ? Date.parse(t0) : null;
  const walk = (v, key) => {
    if (Array.isArray(v)) return v.map((x) => walk(x, null));
    if (v && typeof v === 'object') {
      const out = {};
      for (const [k, x] of Object.entries(v)) out[k] = walk(x, k);
      return out;
    }
    if (key === 'claudecode/toolUseId' && typeof v === 'string') return '<TOOL_USE_ID>';
    if (key === 'progressToken') return '<PROGRESS_TOKEN>';
    if (key === 'pid' && typeof v === 'number') return '<PID>';
    return v;
  };
  return entries.map((e) => {
    const payload = walk(e.payload, null);
    if (e.direction !== 'spike' && payload.id !== undefined) {
      const k = idKey(payload.id);
      if (!ids.has(k)) ids.set(k, `<ID:${ids.size + 1}>`);
      payload.id = ids.get(k);
    }
    const offsetMs = base !== null && e.t ? Date.parse(e.t) - base : null;
    return { line: e.line, offsetMs, direction: e.direction, payload };
  });
}

// Longest-common-subsequence diff of two key arrays.
// -> [{ op: '='|'-'|'+', key, a: index|null, b: index|null }]
export function diffSequences(a, b) {
  const n = a.length;
  const m = b.length;
  const L = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) L[i][j] = a[i] === b[j] ? L[i + 1][j + 1] + 1 : Math.max(L[i + 1][j], L[i][j + 1]);
  const ops = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) ops.push({ op: '=', key: a[i], a: i++, b: j++ });
    else if (L[i + 1][j] >= L[i][j + 1]) ops.push({ op: '-', key: a[i], a: i++, b: null });
    else ops.push({ op: '+', key: b[j], a: null, b: j++ });
  }
  while (i < n) ops.push({ op: '-', key: a[i], a: i++, b: null });
  while (j < m) ops.push({ op: '+', key: b[j], a: null, b: j++ });
  return ops;
}

// Facts the G1 criteria are scored on, from one transcript (or one segment of it).
export function transcriptFacts(entries) {
  const facts = {
    frames: entries.length,
    serverInstances: entries.filter((e) => e.direction === 'spike' && START_NOTE.test(String(e.payload.note ?? ''))).length,
    discoverProbe: false,
    initialize: null,
    negotiatedProtocolVersion: null,
    serverDeclaresChannel: false,
    initializedNotification: false,
    toolsListed: [],
    channelNotifications: [],
    replyCalls: [],
    firstT: entries.find((e) => e.t)?.t ?? null,
    lastT: [...entries].reverse().find((e) => e.t)?.t ?? null,
  };
  const requests = new Map();
  for (const e of entries) {
    const p = e.payload;
    if (e.direction === 'client->server') {
      if (p.method === 'server/discover') facts.discoverProbe = true;
      if (p.method === 'initialize') {
        facts.initialize = { line: e.line, protocolVersion: p.params?.protocolVersion ?? null, clientInfo: { name: p.params?.clientInfo?.name ?? null, version: p.params?.clientInfo?.version ?? null } };
      }
      if (p.method === 'notifications/initialized') facts.initializedNotification = true;
      if (p.method === 'tools/call' && p.params?.name === 'reply') {
        const call = { line: e.line, t: e.t, id: p.id, message: p.params?.arguments?.message ?? null, inReplyTo: p.params?.arguments?.in_reply_to ?? null, resultLine: null, resultOk: null };
        facts.replyCalls.push(call);
      }
      if (typeof p.method === 'string' && p.id !== undefined) requests.set(idKey(p.id), p.method);
    } else if (e.direction === 'server->client') {
      const method = p.id !== undefined ? requests.get(idKey(p.id)) : undefined;
      if (method === 'initialize' && p.result) {
        facts.negotiatedProtocolVersion = p.result.protocolVersion ?? null;
        facts.serverDeclaresChannel = Object.prototype.hasOwnProperty.call(p.result.capabilities?.experimental ?? {}, 'claude/channel');
      }
      if (method === 'tools/list' && p.result) facts.toolsListed = (p.result.tools ?? []).map((t) => t.name);
      if (method === 'tools/call') {
        const call = [...facts.replyCalls].reverse().find((c) => idKey(c.id) === idKey(p.id) && c.resultLine === null);
        if (call) {
          call.resultLine = e.line;
          call.resultOk = !p.error && p.result?.isError !== true;
        }
      }
      if (p.method === 'notifications/claude/channel') {
        const meta = p.params?.meta ?? {};
        const keys = Object.keys(meta);
        facts.channelNotifications.push({
          line: e.line,
          t: e.t,
          id: meta.oac_message_id ?? null,
          kind: messageKind(meta.oac_message_id),
          contentIsString: typeof p.params?.content === 'string',
          metaKeys: keys,
          identifierSafeKeys: keys.filter(isIdentifierSafe),
          droppedKeys: keys.filter((k) => !isIdentifierSafe(k)),
        });
      }
    }
  }
  for (const c of facts.replyCalls) delete c.id;
  return facts;
}

export function compareTranscripts(baselineText, candidateText, { segment = 'all', detail = false } = {}) {
  const side = (text) => {
    const all = parseTranscript(text);
    const entries = selectSegment(all, segment);
    return {
      totalLines: all.length,
      lineRange: entries.length ? [entries[0].line, entries.at(-1).line] : null,
      keys: eventKeys(entries, { detail }),
      entries,
      normalized: normalizeEntries(entries),
      facts: transcriptFacts(entries),
    };
  };
  const a = side(baselineText);
  const b = side(candidateText);
  const ops = diffSequences(a.keys, b.keys).map((o) => ({
    ...o,
    baselineLine: o.a === null ? null : a.entries[o.a].line,
    candidateLine: o.b === null ? null : b.entries[o.b].line,
  }));
  const summary = {
    segment,
    detail,
    same: ops.filter((o) => o.op === '=').length,
    onlyInBaseline: ops.filter((o) => o.op === '-').length,
    onlyInCandidate: ops.filter((o) => o.op === '+').length,
  };
  summary.identicalSequence = summary.onlyInBaseline === 0 && summary.onlyInCandidate === 0;
  const strip = ({ entries, ...rest }) => rest;
  return { baseline: strip(a), candidate: strip(b), ops, summary };
}

// Plain-text rendering of a comparison's method-sequence diff.
export function formatDiff(result, { baselineLabel = 'baseline', candidateLabel = 'candidate' } = {}) {
  const { summary } = result;
  const range = (s) => (s.lineRange ? `lines ${s.lineRange[0]}-${s.lineRange[1]} of ${s.totalLines}` : 'no frames');
  const out = [
    `--- ${baselineLabel} (${range(result.baseline)})`,
    `+++ ${candidateLabel} (${range(result.candidate)})`,
    `segment: ${summary.segment}; keys: method-level${summary.detail ? ' + message kind / tool name' : ''}; timestamps, JSON-RPC ids, tool-use ids, progress tokens and pids normalized`,
  ];
  for (const o of result.ops) {
    const where = o.op === '=' ? `A:${o.baselineLine} B:${o.candidateLine}` : o.op === '-' ? `A:${o.baselineLine}` : `B:${o.candidateLine}`;
    out.push(`${o.op === '=' ? ' ' : o.op} ${o.key}`.padEnd(64) + `  (${where})`);
  }
  out.push(`summary: ${summary.same} same, ${summary.onlyInBaseline} only in ${baselineLabel}, ${summary.onlyInCandidate} only in ${candidateLabel}${summary.identicalSequence ? ' -- identical method sequence' : ''}`);
  return out.join('\n');
}

function main(argv) {
  const files = [];
  const o = { segment: 'all', detail: false, json: false };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--segment') o.segment = argv[++i];
    else if (argv[i] === '--detail') o.detail = true;
    else if (argv[i] === '--json') o.json = true;
    else if (argv[i].startsWith('--')) {
      console.error(`unknown option ${argv[i]}`);
      return 2;
    } else files.push(argv[i]);
  }
  if (files.length !== 2) {
    console.error('usage: node tools/herdr/lib/compare-transcripts.mjs <baseline.jsonl> <candidate.jsonl> [--segment all|last] [--detail] [--json]');
    return 2;
  }
  let res;
  try {
    res = compareTranscripts(readFileSync(files[0], 'utf8'), readFileSync(files[1], 'utf8'), o);
  } catch (err) {
    console.error(`compare-transcripts: ${err.message}`);
    return 2;
  }
  if (o.json) console.log(JSON.stringify({ summary: res.summary, ops: res.ops, baselineFacts: res.baseline.facts, candidateFacts: res.candidate.facts }, null, 2));
  else console.log(formatDiff(res, { baselineLabel: files[0], candidateLabel: files[1] }));
  return res.summary.identicalSequence ? 0 : 1;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
