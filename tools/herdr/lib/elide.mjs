// Tool-output elision for herdr captures (#130, #131).
//
// The fixtures the driver writes are committed to a public repository. A harness can read a
// file or call a tool on its own initiative while it answers a delivered message, and the
// app-server then carries what came back on the wire verbatim: a file the harness read, a
// tool's description, an MCP server's reply. That is third-party text. The capture keeps the
// frame and replaces each such body with a marker that still identifies it:
//
//   <ELIDED tool-output bytes=<UTF-8 byte length> sha256=<64 hex>>
//
// Both are of the body after redaction (it is elided from the redacted record, so the hash
// can disclose nothing that redaction would not have let through in the clear).
//
// What is elided is decided by shape, never by length or content. The catalogue below is the
// Codex app-server v2 protocol (codex-rs/app-server-protocol/schema/json, rust-v0.160.0):
//   - the output-bearing fields of a thread item, wherever the item sits in a frame
//     (item/started, item/completed, turn/completed's turn.items, thread/turns/list,
//     thread/read and thread/resume results, a G5 `response` record, ...);
//   - the output deltas and progress messages of item notifications.
// A file read is a commandExecution whose commandActions name a read (Codex runs it as a
// command; the G2 2026-10-05 run read a skill file this way), so its body is
// aggregatedOutput and the outputDelta frames. A skill a user names in a turn is a
// `{type:"skill", name, path}` input, which carries no body.
// Kept: every key, id, method, status, exit code, duration, the command line and tool
// arguments (the model's own input), `type` tags inside an elided body, and every message
// text (userMessage, agentMessage, reasoning, turn/start and thread/queue/add input) -- the
// gate criteria score those. One record stays one line, so transcript line numbers hold.
//
// Claude Code: the herdr captures of Claude Code are the MCP traffic between Claude Code and
// OAC's own spike server. Claude Code's own tool results (Read, Bash, ...) are not on that
// wire, and the tools/call results that are on it are the OAC server's own replies, which
// G1 and G4 score. Nothing there is elided.
//
// Pane text: a TUI can show a few lines of a tool's output. A pane line is elided only on a
// reliable signal: after trimming its indentation and leading TUI glyphs it is at least
// PANE_MIN_LENGTH characters long, it occurs verbatim in a tool-output body elided from a
// wire transcript of the same run, and it occurs in no text the transcript keeps. The line
// stays (its leading indentation and glyphs too), so pane line numbers hold. A pane whose
// tool outputs are not on any captured wire (Claude Code's own tools; Codex in G4, whose
// transcript is the OAC server's side) has no such signal and is left as it is.

import { createHash } from 'node:crypto';

export const ELIDED_RE = /^<ELIDED tool-output(?:-line)? bytes=\d+ sha256=[0-9a-f]{64}>$/;
export const PANE_MIN_LENGTH = 16;

// Thread item type -> its output-bearing fields (ThreadItem, app-server v2 schema).
export const ITEM_OUTPUT_FIELDS = Object.freeze({
  commandExecution: ['aggregatedOutput'],
  functionCallOutput: ['output'],
  mcpToolCall: ['result', 'error'],
  dynamicToolCall: ['contentItems'],
  webSearch: ['results'],
  imageGeneration: ['result'],
});

// Notification method -> its output-bearing params fields (ServerNotification, v2 schema).
export const NOTIFICATION_OUTPUT_FIELDS = Object.freeze({
  'item/commandExecution/outputDelta': ['delta'],
  'item/fileChange/outputDelta': ['delta'],
  'item/mcpToolCall/progress': ['message'],
  'command/exec/outputDelta': ['deltaBase64'],
  'process/outputDelta': ['deltaBase64'],
});

const sha256 = (s) => createHash('sha256').update(s, 'utf8').digest('hex');
const bytes = (s) => Buffer.byteLength(s, 'utf8');

export function elisionMarker(body, kind = 'tool-output') {
  return `<ELIDED ${kind} bytes=${bytes(body)} sha256=${sha256(body)}>`;
}

const isItem = (o) => typeof o.type === 'string' && typeof o.id === 'string' && Object.hasOwn(ITEM_OUTPUT_FIELDS, o.type);
const isOutputNotification = (o) => typeof o.method === 'string' && Object.hasOwn(NOTIFICATION_OUTPUT_FIELDS, o.method) && o.params && typeof o.params === 'object';

// Every string leaf of a body becomes a marker; `type` tags stay, so does the shape.
function elideBody(v, path, out) {
  if (typeof v === 'string') {
    if (v === '' || ELIDED_RE.test(v)) return v;
    out.push({ path, bytes: bytes(v), sha256: sha256(v), body: v });
    return elisionMarker(v);
  }
  if (Array.isArray(v)) return v.map((x, i) => elideBody(x, `${path}[${i}]`, out));
  if (v && typeof v === 'object') {
    const o = {};
    for (const [k, x] of Object.entries(v)) o[k] = k === 'type' && typeof x === 'string' ? x : elideBody(x, `${path}.${k}`, out);
    return o;
  }
  return v;
}

/**
 * Elide tool-output bodies anywhere in one JSON value (a JSONL record).
 * @returns {{ value: any, elided: Array<{path: string, bytes: number, sha256: string, body: string}> }}
 *   `body` is the elided text; callers keep it out of anything they write.
 */
export function elideToolOutputs(value) {
  const elided = [];
  const visit = (v, path) => {
    if (Array.isArray(v)) return v.map((x, i) => visit(x, `${path}[${i}]`));
    if (!v || typeof v !== 'object') return v;
    const fields = isItem(v) ? ITEM_OUTPUT_FIELDS[v.type] : [];
    const o = {};
    for (const [k, x] of Object.entries(v)) {
      if (fields.includes(k) && x !== null && x !== undefined) o[k] = elideBody(x, `${path}.${k}`, elided);
      else if (k === 'params' && isOutputNotification(v) && x && typeof x === 'object') {
        const p = {};
        for (const [pk, px] of Object.entries(x)) p[pk] = NOTIFICATION_OUTPUT_FIELDS[v.method].includes(pk) ? elideBody(px, `${path}.params.${pk}`, elided) : visit(px, `${path}.params.${pk}`);
        o[k] = p;
      } else o[k] = visit(x, `${path}.${k}`);
    }
    return o;
  };
  return { value: visit(value, '$'), elided };
}

/** Paths of output-bearing fields still carrying a body that is not a marker (residual scan). */
export function unelidedToolOutputs(value) {
  return elideToolOutputs(value).elided.map((e) => e.path);
}

/** Every string leaf of every JSON line in a JSONL text (the text a transcript keeps). */
export function keptStrings(jsonlText) {
  const out = [];
  const visit = (v) => {
    if (typeof v === 'string') {
      if (!ELIDED_RE.test(v)) out.push(v);
    } else if (Array.isArray(v)) v.forEach(visit);
    else if (v && typeof v === 'object') Object.values(v).forEach(visit);
  };
  for (const line of String(jsonlText).split('\n')) {
    if (!line.trim()) continue;
    try {
      visit(JSON.parse(line));
    } catch {
      out.push(line);
    }
  }
  return out;
}

// Indentation, then TUI glyphs and the spaces after them (`└ `, `│ `, `⎿  `, `• `, `› `).
const PANE_PREFIX_RE = /^[\s└├│┃╰⎿•›·+-]*/u;

/**
 * Elide pane lines that are tool output, by the wire signal described in the header.
 * @param {string} text redacted pane text
 * @param {{ bodies: string[], kept: string[], minLength?: number }} wire
 *   bodies: tool-output bodies elided from this run's wire transcripts (redacted);
 *   kept: string leaves those transcripts keep.
 */
export function elidePaneLines(text, { bodies, kept, minLength = PANE_MIN_LENGTH }) {
  const elided = [];
  if (!bodies.length) return { text, elided };
  const lines = String(text).split('\n');
  const out = lines.map((line, i) => {
    const cr = line.endsWith('\r') ? '\r' : '';
    const l = cr ? line.slice(0, -1) : line;
    const prefix = PANE_PREFIX_RE.exec(l)[0];
    const rest = l.slice(prefix.length).replace(/\s+$/, '');
    if (rest.length < minLength || ELIDED_RE.test(rest)) return line;
    const probe = rest.replace(/…$/, '');
    if (!bodies.some((b) => b.includes(probe))) return line;
    if (kept.some((k) => k.includes(probe))) return line;
    elided.push({ line: i + 1, bytes: bytes(rest), sha256: sha256(rest) });
    return `${prefix}${elisionMarker(rest, 'tool-output-line')}${cr}`;
  });
  return { text: out.join('\n'), elided };
}

/**
 * Redact and elide one run's captures (run.mjs, record phase). Wire transcripts (`jsonl`) go
 * first: their elided bodies, and the text they keep, are the signal for the panes (`text`).
 * @param {ReturnType<import('./redact.mjs').createRedactor>} redactor
 * @param {Array<{ name: string, text: string, format: 'jsonl' | 'text' }>} captures
 * @returns {Map<object, { text: string, report: object }>} keyed by capture; a report never
 *   carries an elided body, only line, path, bytes and sha256.
 */
export function redactCaptures(redactor, captures) {
  const out = new Map();
  const wire = { bodies: [], kept: [] };
  for (const c of captures.filter((x) => x.format === 'jsonl')) {
    const { text, report, toolOutputBodies } = redactor.redactJsonl(c.text);
    out.set(c, { text, report });
    wire.bodies.push(...toolOutputBodies);
    wire.kept.push(...keptStrings(text));
  }
  for (const c of captures.filter((x) => x.format !== 'jsonl')) {
    const r = redactor.redactText(c.text);
    const pane = elidePaneLines(r.text, wire);
    // Re-scanned after elision; the written text is what the report describes.
    out.set(c, { text: pane.text, report: { ...r.report, elidedToolOutputLines: pane.elided, ...redactor.scan(pane.text) } });
  }
  return out;
}
