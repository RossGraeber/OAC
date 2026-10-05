// Tool-output elision for herdr captures (#130, #131).
//
// The fixtures the driver writes are committed to a public repository. A harness can read a
// file or call a tool on its own initiative while it answers a delivered message, and the
// app-server then carries what came back on the wire verbatim: a file the harness read, a
// tool's description, an MCP server's reply, a diff of a file it edited. That is third-party
// text. The capture keeps the frame and replaces each such body with a marker that still
// identifies it:
//
//   <ELIDED tool-output bytes=<UTF-8 byte length> sha256=<64 hex>>
//
// (`harness-text` in place of `tool-output` for DAEMON_TEXT_KEYS below.) Both are of the body after redaction (it is elided from the redacted record, so the hash
// can disclose nothing that redaction would not have let through in the clear).
//
// What is elided is decided by shape, never by length or content: the fields listed in
// ITEM_OUTPUT_FIELDS and NOTIFICATION_OUTPUT_FIELDS below. They come from one pass over every
// ThreadItem variant and every ServerNotification of the Codex app-server v2 schema
// (codex-rs/app-server-protocol/schema/json, rust-v0.160.0), keeping the fields that carry
// harness- or tool-produced text. The list covers those fields, not the whole protocol.
// Items are matched wherever they sit in a frame (item/started, item/completed,
// turn/completed's turn.items, thread/turns/list, thread/read and thread/resume results, a
// G5 `response` record, ...).
//   - A file read is a commandExecution whose commandActions name a read (Codex runs it as a
//     command; the G2 2026-10-05 run read a skill file this way): its body is
//     aggregatedOutput and the outputDelta frames.
//   - A file diff (fileChange, turn/diff/updated, item/fileChange/patchUpdated) reproduces
//     the text of the file it edits or deletes: its `diff` is elided, its path and kind stay.
//   - Hook output (hook/started, hook/completed run entries; hookPrompt fragments) and a
//     process's buffered output (process/exited) are elided like the output deltas.
//   - A skill a user names in a turn is a `{type:"skill", name, path}` input: no body.
//   - Requests the daemon sends its client (SERVER_REQUEST_OUTPUT_FIELDS): an MCP server's
//     elicitation message, schema and url, and the file contents of the legacy patch approval.
//     A daemon request, like a daemon response, is recognised by the capture record's
//     `direction: "daemon->client"` (#297 NB5), never by the absence of `"jsonrpc"`.
//   - Harness-authored instruction and prompt text, and upstream error detail, in anything the
//     daemon sends its client (DAEMON_TEXT_KEYS, marker kind `harness-text`): matched by key
//     name, because a response does not name the method it answers. The keys come from one
//     pass over every *Response definition of the v2 schema at rust-v0.160.0 (and the
//     notifications and requests): Settings.developer_instructions (thread/resume's
//     collaborationMode.settings, and the thread/settings/updated notification's
//     threadSettings.collaborationMode.settings), Config.developer_instructions / instructions /
//     compact_prompt (config/read), ConfigRequirements.additionalDeveloperInstructions,
//     PluginInterface / SkillInterface.defaultPrompt and AppScreenshot.userPrompt (plugin,
//     skill and app listings), TurnError.additionalDetails and the MisalignmentErrorDetails
//     `detailedExplanation` and `steer` (a turn's error, wherever the turn sits). The schema's
//     thread/start, thread/resume and thread/fork params name baseInstructions and
//     developerInstructions; their snake_case forms and user_instructions are older Codex
//     names. All are listed so that a response echoing one is elided too. Thread items
//     (THREAD_ITEM_TYPES) are not searched by key: their fields are the per-type lists above.
//     Responses in scope are those to the requests the herdr Codex clients send: the G2
//     client and gate-servers/g5-codex.mjs send only initialize, thread/loaded/list,
//     thread/list, thread/resume, thread/turns/list, turn/start and thread/queue/add, and the
//     only harness-authored text in their responses is the keys above. Every other response
//     is out of scope: it can carry harness- or third-party text on no list (model/list
//     upgrade copy, experimental-feature announcements, workspace messages, plugin skill
//     contents, MCP resource-read and tool-call results, app metadata and app/list/updated),
//     which the residual scan below catches only at LONG_TEXT_MIN characters or more.
// Out of scope, each for a reason:
//   - results of client requests the herdr clients never send (command/exec, fs/readFile,
//     process/spawn, skills/list). process/exited, which follows a process/spawn, is
//     elided anyway, like process/outputDelta;
//   - model- and user-authored text (agentMessage, reasoning, plan, userMessage, review
//     text, the command line, tool arguments): the gate criteria score it, and it is not
//     what a tool returned.
// Inside an elided body every string becomes a marker, and so does every object key the
// schemas do not name (free-text keys such as an MCP structuredContent's). Only the
// schemas' keys (BODY_KEYS) and enum `type` tags (BODY_TYPE_TAGS) stay. One record stays
// one line, so transcript line numbers hold.
//
// The residual scan does not rely on this list alone: unrecognisedLongText() flags any string
// of LONG_TEXT_MIN characters or more in an app-server item, notification, daemon->client
// request or daemon->client response that is neither elided nor on a keep-list of fields
// that a gate criterion scores or that OAC or its client wrote (or, for a few short
// harness-status fields, decided one by one: #297 NB3, below). Text under that length in a
// field no list names is caught only by the elision list (known limit, #297 NB4).
//
// #297 NB3, each keep-list field that can carry text the model did not write:
//   - turn.error.message / error.message (turn/started, turn/completed, the error
//     notification, and a turn inside a response): KEPT. Codex's own status line for a failed
//     turn; the scenarios report it as the reason a run failed. Its detail
//     (additionalDetails) and the misalignment explanation and steer text are ELIDED (above).
//   - mcpServer/startupStatus/updated `error`: ELIDED. An MCP server's startup error chain;
//     it can carry the server's own reply or stderr, no criterion reads it, and it is not
//     short (the committed D6 fixtures hold a 971-character one). Its name and status stay.
//   - configWarning `details`: ELIDED. It can quote the user's config file; summary and path
//     stay.
//   - agentMessage memoryCitation: entries[].note ELIDED (memory-file text); paths and
//     thread ids stay.
//
// Claude Code: the herdr captures of Claude Code are the MCP traffic between Claude Code and
// OAC's own spike server. Claude Code's own tool results (Read, Bash, ...) are not on that
// wire, and the tools/call results that are on it are the OAC server's own replies, which
// G1 and G4 score. Nothing there is elided; MCP `notifications/...` frames are outside the
// app-server scan.
//
// Pane text: a TUI can show a few lines of a tool's output. A pane line is elided only on a
// reliable signal: after trimming its indentation and leading TUI glyphs it is at least
// PANE_MIN_LENGTH characters long, it occurs verbatim in a tool-output body elided from a
// wire transcript of the same run, and it occurs in no text the transcript keeps. The line
// stays (its leading indentation and glyphs too), so pane line numbers hold. A pane whose
// tool outputs are not on any captured wire (Claude Code's own tools; Codex in G4, whose
// transcript is the OAC server's side) has no such signal and is left as it is.

import { createHash } from 'node:crypto';

export const ELIDED_RE = /^<ELIDED (?:tool-output(?:-line|-key)?|harness-text) bytes=\d+ sha256=[0-9a-f]{64}>$/;
export const PANE_MIN_LENGTH = 16;
export const LONG_TEXT_MIN = 120;

// Thread item type -> its output-bearing fields (ThreadItem, app-server v2 schema). A field is
// a dotted path from the item; `*` steps into every element of an array.
export const ITEM_OUTPUT_FIELDS = Object.freeze({
  commandExecution: ['aggregatedOutput'],
  fileChange: ['changes.*.diff'],
  functionCallOutput: ['output'],
  mcpToolCall: ['result', 'error'],
  dynamicToolCall: ['contentItems'],
  webSearch: ['results'],
  imageGeneration: ['result'],
  hookPrompt: ['fragments.*.text'],
  agentMessage: ['memoryCitation.entries.*.note'], // #297 NB3: memory-file text, not the answer
});

// Notification method -> its output-bearing params fields (ServerNotification, v2 schema).
export const NOTIFICATION_OUTPUT_FIELDS = Object.freeze({
  'item/commandExecution/outputDelta': ['delta'],
  'item/fileChange/outputDelta': ['delta'],
  'item/fileChange/patchUpdated': ['changes.*.diff'],
  'turn/diff/updated': ['diff'],
  'item/mcpToolCall/progress': ['message'],
  'command/exec/outputDelta': ['deltaBase64'],
  'process/outputDelta': ['deltaBase64'],
  'process/exited': ['stdout', 'stderr'],
  'hook/started': ['run.entries.*.text'],
  'hook/completed': ['run.entries.*.text'],
  'mcpServer/event/stream/notification': ['notification'],
  'mcpServer/startupStatus/updated': ['error'], // #297 NB3
  configWarning: ['details'], // #297 NB3
});

// Server request method -> its output-bearing params fields (ServerRequest, v2 schema; #130
// review NB2). An MCP server's elicitation text, schema and url (#297 NB6: url mode), and the
// file contents of the legacy patch approval (`%`: every entry of the path -> change map; the
// paths stay). Approval reasons, commands, questions and dynamic-tool arguments are the
// model's or the harness's own text and are on the scan's keep-list instead.
export const SERVER_REQUEST_OUTPUT_FIELDS = Object.freeze({
  'mcpServer/elicitation/request': ['message', 'requestedSchema', '_meta', 'url'],
  applyPatchApproval: ['fileChanges.%.content', 'fileChanges.%.unified_diff'],
});

// Keys whose value is elided wherever it sits in a daemon->client frame (a response's result
// or error, a notification's or request's params), outside thread items (header). Sources
// at rust-v0.160.0, codex-rs/app-server-protocol/schema/json/
// codex_app_server_protocol.v2.schemas.json: Settings, Config, ConfigRequirements,
// PluginInterface, SkillInterface, AppScreenshot, TurnError, MisalignmentErrorDetails;
// ThreadStartParams / ThreadResumeParams / ThreadForkParams for the instruction names.
export const DAEMON_TEXT_KEYS = Object.freeze(new Set([
  'developer_instructions', 'developerInstructions', 'base_instructions', 'baseInstructions', 'user_instructions', 'userInstructions',
  'instructions', 'additionalDeveloperInstructions', 'compact_prompt', 'compactPrompt', 'defaultPrompt', 'userPrompt',
  'additionalDetails', 'detailedExplanation', 'steer',
]));

// Inside an elided body: object keys the schemas name (MCP CallToolResult and its content
// blocks; the app-server's function and dynamic-tool content items; an error object; an MCP
// event's method/params envelope) stay, every other key becomes a marker. A `type` value stays
// only if it is one of these enum tags.
export const BODY_KEYS = Object.freeze(new Set([
  'content', 'structuredContent', '_meta', 'isError', 'type', 'text', 'data', 'mimeType', 'uri', 'name', 'title',
  'resource', 'annotations', 'audience', 'priority', 'lastModified', 'blob', 'size', 'message',
  'image_url', 'file_id', 'audio_url', 'encrypted_content', 'detail', 'imageUrl', 'audioUrl', 'method', 'params',
]));
export const BODY_TYPE_TAGS = Object.freeze(new Set([
  'text', 'image', 'audio', 'resource', 'resource_link', 'input_text', 'input_image', 'input_audio', 'encrypted_content', 'inputText', 'inputImage', 'inputAudio',
]));

const sha256 = (s) => createHash('sha256').update(s, 'utf8').digest('hex');
const bytes = (s) => Buffer.byteLength(s, 'utf8');

export function elisionMarker(body, kind = 'tool-output') {
  return `<ELIDED ${kind} bytes=${bytes(body)} sha256=${sha256(body)}>`;
}

const isItem = (o) => typeof o.type === 'string' && typeof o.id === 'string';
const isAppServerNotification = (o) => typeof o.method === 'string' && !o.method.startsWith('notifications/') && o.id === undefined && !!o.params && typeof o.params === 'object';
const isObj = (o) => !!o && typeof o === 'object' && !Array.isArray(o);
// What the Codex daemon sent its client (#297 NB5): a capture record `{direction:
// "daemon->client", payload}` (G2, G5, D6), or a G5 spike `{spike, response}` record, which
// copies a daemon response. Keyed on the record, never on the frame's own fields: a daemon
// request is not told from OAC's own requests (turn/start, MCP tools/call) by whether it
// carries `"jsonrpc"`, which a future Codex may add. -> { key, kind } or null; kind is
// 'request' (method and id), 'response' (result or error, no method), 'notification'
// (method, no id) or 'other'.
function daemonFrame(r) {
  let key = null;
  if (r.direction === 'daemon->client' && isObj(r.payload)) key = 'payload';
  else if (typeof r.spike === 'string' && isObj(r.response) && r.response.method === undefined) key = 'response';
  if (!key) return null;
  const p = r[key];
  const kind = typeof p.method === 'string' ? (p.id !== undefined ? 'request' : 'notification') : p.id !== undefined && (Object.hasOwn(p, 'result') || Object.hasOwn(p, 'error')) ? 'response' : 'other';
  return { key, kind };
}

function elideString(v, path, out, kind = 'tool-output') {
  if (v === '' || ELIDED_RE.test(v)) return v;
  out.push({ path, bytes: bytes(v), sha256: sha256(v), body: v });
  return elisionMarker(v, kind);
}

// Every string leaf of a body becomes a marker, and so does every key the schemas do not name;
// schema keys and enum `type` tags stay, so does the shape.
function elideBody(v, path, out, kind = 'tool-output') {
  if (typeof v === 'string') return elideString(v, path, out, kind);
  if (Array.isArray(v)) return v.map((x, i) => elideBody(x, `${path}[${i}]`, out, kind));
  if (v && typeof v === 'object') {
    const o = {};
    for (const [k, x] of Object.entries(v)) {
      const kept = BODY_KEYS.has(k) || ELIDED_RE.test(k);
      const key = kept ? k : elideString(k, `${path}.<key>`, out, 'tool-output-key');
      const sub = `${path}.${kept ? k : '<key>'}`;
      o[key] = k === 'type' && typeof x === 'string' && BODY_TYPE_TAGS.has(x) ? x : elideBody(x, sub, out, kind);
    }
    return o;
  }
  return v;
}

// The ThreadItem variants of the v2 schema at rust-v0.160.0 (ThreadItem.oneOf[].type, 19).
// Key-name elision skips an object only when it is one of these (#298 review NB-B): other
// objects with a string id and type, such as a ConfigLayerSource (`enterpriseManaged`:
// id, name, type), are searched like the rest of the frame.
export const THREAD_ITEM_TYPES = Object.freeze(new Set([
  'userMessage', 'hookPrompt', 'agentMessage', 'functionCallOutput', 'plan', 'reasoning', 'commandExecution', 'fileChange', 'mcpToolCall', 'dynamicToolCall',
  'collabAgentToolCall', 'subAgentActivity', 'webSearch', 'imageView', 'sleep', 'imageGeneration', 'enteredReviewMode', 'exitedReviewMode', 'contextCompaction',
]));
const isThreadItem = (o) => isItem(o) && THREAD_ITEM_TYPES.has(o.type);

// DAEMON_TEXT_KEYS anywhere under a daemon frame, thread items excepted (their fields are
// the per-type lists; an item's tool arguments are the model's input and stay).
function elideDaemonKeys(v, path, out) {
  if (Array.isArray(v)) return v.map((x, i) => elideDaemonKeys(x, `${path}[${i}]`, out));
  if (!isObj(v) || isThreadItem(v)) return v;
  const o = {};
  for (const [k, x] of Object.entries(v)) {
    o[k] = DAEMON_TEXT_KEYS.has(k) && x !== null && x !== undefined ? elideBody(x, `${path}.${k}`, out, 'harness-text') : elideDaemonKeys(x, `${path}.${k}`, out);
  }
  return o;
}

// Elide the field at a dotted spec path inside `obj`; returns a copy.
function elideAt(obj, parts, path, out) {
  if (!obj || typeof obj !== 'object') return obj;
  const [head, ...rest] = parts;
  if (head === '*') return Array.isArray(obj) ? obj.map((x, i) => elideAt(x, rest, `${path}[${i}]`, out)) : obj;
  // `%`: every value of a map object (keys, e.g. file paths, stay).
  if (head === '%') return Array.isArray(obj) ? obj : Object.fromEntries(Object.entries(obj).map(([k, x]) => [k, elideAt(x, rest, `${path}.<entry>`, out)]));
  if (Array.isArray(obj) || !Object.hasOwn(obj, head) || obj[head] === null || obj[head] === undefined) return obj;
  return { ...obj, [head]: rest.length ? elideAt(obj[head], rest, `${path}.${head}`, out) : elideBody(obj[head], `${path}.${head}`, out) };
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
    let o = v;
    if (isItem(v) && Object.hasOwn(ITEM_OUTPUT_FIELDS, v.type)) for (const f of ITEM_OUTPUT_FIELDS[v.type]) o = elideAt(o, f.split('.'), path, elided);
    const fields = (table, p) => (isObj(p.params) && Object.hasOwn(table, p.method) ? table[p.method] : []);
    if (isAppServerNotification(v)) {
      let p = o.params;
      for (const f of fields(NOTIFICATION_OUTPUT_FIELDS, v)) p = elideAt(p, f.split('.'), `${path}.params`, elided);
      o = { ...o, params: p };
    }
    const d = daemonFrame(v);
    if (d) {
      const at = `${path}.${d.key}`;
      let p = elideDaemonKeys(o[d.key], at, elided);
      if (d.kind === 'request') {
        let prm = p.params;
        for (const f of fields(SERVER_REQUEST_OUTPUT_FIELDS, p)) prm = elideAt(prm, f.split('.'), `${at}.params`, elided);
        p = { ...p, params: prm };
      }
      o = { ...o, [d.key]: p };
    }
    const r = {};
    for (const [k, x] of Object.entries(o)) r[k] = visit(x, `${path}.${k}`);
    return r;
  };
  return { value: visit(value, '$'), elided };
}

/** Paths of output-bearing fields still carrying a body that is not a marker (residual scan). */
export function unelidedToolOutputs(value) {
  return elideToolOutputs(value).elided.map((e) => e.path);
}

// --- the independent residual check (#130 review, N3) -------------------------------------
// Fields known to hold model, user or harness-status text, never tool output. A path is
// relative to the item, or to a notification's params; `*` is one array step, a trailing
// `**` any suffix. Any other string of LONG_TEXT_MIN characters or more in an app-server item
// or notification, that is not a marker, is flagged whether or not the elision list names
// it. A field the list misses (a new Codex item type or notification included) so withholds
// the capture instead of reaching a fixture.
export const KEEP_ITEM_TEXT = Object.freeze({
  userMessage: ['content.*.text', 'content.*.path', 'content.*.url', 'content.*.name', 'content.*.text_elements.**'],
  agentMessage: ['text', 'memoryCitation.entries.*.path', 'memoryCitation.threadIds.*', 'questions.**'],
  reasoning: ['summary.**', 'content.**'],
  plan: ['text'],
  commandExecution: ['command', 'cwd', 'scriptPath', 'commandActions.*.command', 'commandActions.*.path', 'commandActions.*.name', 'commandActions.*.query'],
  fileChange: ['changes.*.path', 'changes.*.kind.**'],
  mcpToolCall: ['server', 'tool', 'arguments.**', 'mcpAppResourceUri', 'appContext.**'],
  dynamicToolCall: ['tool', 'namespace', 'arguments.**'],
  collabAgentToolCall: ['prompt'],
  subAgentActivity: ['agentPath'],
  webSearch: ['query', 'action.**'],
  imageView: ['path'],
  imageGeneration: ['revisedPrompt', 'savedPath'],
  enteredReviewMode: ['review'],
  exitedReviewMode: ['review'],
  functionCallOutput: ['name', 'namespace'],
});
export const KEEP_NOTIFICATION_TEXT = Object.freeze({
  'item/agentMessage/delta': ['delta'],
  'item/plan/delta': ['delta'],
  'item/reasoning/summaryTextDelta': ['delta'],
  'item/reasoning/textDelta': ['delta'],
  'turn/plan/updated': ['explanation', 'plan.**'],
  'turn/started': ['turn.error.message', 'turn.error.misalignment.errorType'],
  'turn/completed': ['turn.error.message', 'turn.error.misalignment.errorType'],
  'thread/started': ['thread.preview', 'thread.name', 'thread.cwd', 'thread.path'],
  'thread/name/updated': ['threadName'],
  error: ['error.message', 'error.misalignment.errorType'],
  warning: ['message'],
  guardianWarning: ['message'],
  configWarning: ['summary', 'path'],
  deprecationNotice: ['summary', 'details'],
});
// Daemon responses (#130, run 20261005T041011Z-bb584c): a response does not name its method,
// so its keep-list is one list of paths from the payload (`result.…`, `error.…`). Only what a
// gate criterion scores, or what OAC or its client wrote:
//   - result.userAgent: initialize; G2 reads the harness version from it;
//   - result.data.*.preview / .cwd / .environments.*.cwd (thread/list) and the same fields
//     of result.thread (thread/start, thread/resume, thread/fork, thread/read): G2 finds the
//     TUI's thread by them; the preview is the operator's prompt, the cwd the client's choice;
//   - result.cwd: the cwd the client asked for (thread/start, thread/resume);
//   - result.queuedSubmission.input.*.text: thread/queue/add echoing OAC's own delivery;
//   - result._fixtureNote: written by OAC's fixture sanitizer (lib/g2.mjs);
//   - error.message and a turn's error.message: Codex's status line for a failed request or
//     turn, which the scenarios report (#297 NB3).
// Pagination cursors (OPAQUE_RESPONSE_TOKENS) are kept only in the shape Codex writes them
// at rust-v0.160.0 (#298 review NB-A, #299 NB-F): serde_json's compact serialization of
// HistoryCursor (codex-rs/thread-store/src/local/thread_history/read.rs:38-44), exactly the
// keys requestedThreadId (a ThreadId: a hyphenated UUID, codex-rs/protocol/src/thread_id.rs),
// rolloutOrdinal (u64), includeAnchor (bool) and scope, whose only key is `kind`, one of the
// CursorScope variants (read.rs:30-36). See isHistoryCursor. The committed fixtures hold
// 677 of them, 125-144 characters long. Codex has a second cursor shape, ThreadTurnsCursor
// {turnId, includeAnchor} (codex-rs/app-server/src/request_processors/thread_processor.rs:
// 5632-5636, thread/turns/list on a non-paginated thread). It is out of scope: at most 71
// characters with a UUID turn id, under LONG_TEXT_MIN, so the scan never reaches it (a longer
// one would be flagged, withholding the capture). Known limit (#299 review N1): rolloutOrdinal
// is kept only up to Number.MAX_SAFE_INTEGER (2^53 - 1), though Codex can write any u64 from an
// i64 (read.rs:271-278); a larger one is flagged. Anything else of LONG_TEXT_MIN characters or more in a cursor field
// is flagged: prose, whitespace or not (percent-encoded, underscore-joined), in a key or a
// value, or hidden by padding or a duplicated key.
export const KEEP_RESPONSE_TEXT = Object.freeze([
  'result.userAgent',
  'result.data.*.preview', 'result.data.*.cwd', 'result.data.*.environments.*.cwd',
  'result.thread.preview', 'result.thread.cwd', 'result.thread.environments.*.cwd',
  'result.cwd',
  'result.queuedSubmission.input.*.text',
  'result._fixtureNote',
  'error.message', 'result.turn.error.message', 'result.data.*.error.message', 'result.thread.turns.*.error.message', 'result.data.*.turns.*.error.message',
]);
export const OPAQUE_RESPONSE_TOKENS = Object.freeze(['result.nextCursor', 'result.backwardsCursor', 'result.turnsBackwardsCursor', 'result.itemsBackwardsCursor']);
// Server requests (#130 review NB2): approval reasons and commands, the model's questions and
// dynamic-tool arguments. Any other long string in a daemon request is flagged, including
// every field of a request method this list does not know.
export const KEEP_SERVER_REQUEST_TEXT = Object.freeze({
  'item/commandExecution/requestApproval': ['command', 'cwd', 'reason', 'commandActions.**', 'proposedExecpolicyAmendment.**', 'proposedNetworkPolicyAmendments.**', 'networkApprovalContext.**'],
  'item/fileChange/requestApproval': ['reason', 'grantRoot'],
  'item/permissions/requestApproval': ['reason'],
  'item/tool/requestUserInput': ['questions.**'],
  'item/tool/call': ['tool', 'namespace', 'arguments.**'],
  execCommandApproval: ['command.**', 'cwd', 'reason', 'parsedCmd.**'],
  applyPatchApproval: ['reason', 'grantRoot'],
});

const globMatch = (pattern, path) => {
  const p = pattern.split('.');
  const s = path.split('.');
  for (let i = 0; i < p.length; i++) {
    if (p[i] === '**') return true;
    if (i >= s.length) return false;
    if (p[i] === '*' ? !/^\d+$/.test(s[i]) : p[i] !== s[i]) return false;
  }
  return p.length === s.length;
};

// Long string leaves under `v`, as dotted paths relative to it; does not descend into an
// object for which `stop` holds (a nested item, checked on its own).
function longStrings(v, rel, out, stop) {
  if (typeof v === 'string') {
    if (v.length >= LONG_TEXT_MIN && !ELIDED_RE.test(v)) out.push([rel, v]);
  } else if (Array.isArray(v)) v.forEach((x, i) => longStrings(x, rel ? `${rel}.${i}` : String(i), out, stop));
  else if (v && typeof v === 'object' && !stop(v)) for (const [k, x] of Object.entries(v)) longStrings(x, rel ? `${rel}.${k}` : k, out, stop);
}

// HistoryCursor at rust-v0.160.0 (read.rs:38-44), in serde's field order; CursorScope's
// variants, camelCase (read.rs:30-36). (#299 NB-F, replacing #298 NB-A's length and
// key/leaf-length caps: a cursor of this shape is at most 159 characters, with no room for
// text, so the caps are subsumed.)
export const HISTORY_CURSOR_KEYS = Object.freeze(['requestedThreadId', 'rolloutOrdinal', 'includeAnchor', 'scope']);
export const CURSOR_SCOPE_KINDS = Object.freeze(new Set(['turns', 'itemsByCreatedAtOrdinal', 'itemsByUpdatedAtOrdinal']));
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const sameKeys = (o, keys) => isObj(o) && Object.keys(o).join() === keys.join();
/** True when `s` is a HistoryCursor exactly as Codex serializes one (header). */
export function isHistoryCursor(s) {
  let v;
  try {
    v = JSON.parse(s);
  } catch {
    return false;
  }
  return sameKeys(v, HISTORY_CURSOR_KEYS)
    && typeof v.requestedThreadId === 'string' && UUID_RE.test(v.requestedThreadId)
    && Number.isSafeInteger(v.rolloutOrdinal) && v.rolloutOrdinal >= 0
    && typeof v.includeAnchor === 'boolean'
    && sameKeys(v.scope, ['kind']) && CURSOR_SCOPE_KINDS.has(v.scope.kind)
    // serde_json's compact form: no padding, no duplicated key, no alternative number spelling.
    && JSON.stringify(v) === s;
}
const keptResponseText = (rel, s) => KEEP_RESPONSE_TEXT.some((g) => globMatch(g, rel)) || (OPAQUE_RESPONSE_TOKENS.includes(rel) && isHistoryCursor(s));

/**
 * Long strings in app-server items, notifications, and daemon->client requests and responses
 * that are neither elided nor on a keep-list (residual scan). Returns JSON paths, never the
 * text.
 */
export function unrecognisedLongText(value) {
  const hits = [];
  const visit = (v, path) => {
    if (Array.isArray(v)) return v.forEach((x, i) => visit(x, `${path}[${i}]`));
    if (!v || typeof v !== 'object') return;
    if (isItem(v)) {
      const keep = KEEP_ITEM_TEXT[v.type] ?? [];
      const found = [];
      for (const [k, x] of Object.entries(v)) longStrings(x, k, found, (o) => isItem(o));
      for (const [rel] of found) if (!keep.some((g) => globMatch(g, rel))) hits.push(`${path}.${rel}`);
    } else if (isAppServerNotification(v)) {
      const keep = KEEP_NOTIFICATION_TEXT[v.method] ?? [];
      const found = [];
      longStrings(v.params, '', found, (o) => o !== v.params && isItem(o));
      for (const [rel] of found) if (!keep.some((g) => globMatch(g, rel))) hits.push(`${path}.params.${rel}`);
    }
    const d = daemonFrame(v);
    if (d && d.kind !== 'notification') {
      const p = v[d.key];
      const found = [];
      const notItem = (o) => isItem(o);
      if (d.kind === 'request') {
        const keep = KEEP_SERVER_REQUEST_TEXT[p.method] ?? [];
        longStrings(p.params, '', found, notItem);
        for (const [rel] of found) if (!keep.some((g) => globMatch(g, rel))) hits.push(`${path}.${d.key}.params.${rel}`);
      } else {
        // A response (#130, run 20261005T041011Z-bb584c), or a daemon frame of no known shape:
        // every field but the frame's own id and method.
        const { id, method, ...rest } = p;
        longStrings(rest, '', found, notItem);
        for (const [rel, s] of found) if (!(d.kind === 'response' && keptResponseText(rel, s))) hits.push(`${path}.${d.key}.${rel}`);
      }
    }
    for (const [k, x] of Object.entries(v)) visit(x, `${path}.${k}`);
  };
  visit(value, '$');
  return hits;
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
