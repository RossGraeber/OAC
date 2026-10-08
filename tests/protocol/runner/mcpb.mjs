// The `mcp-binding` stage of spec/bindings/mcp.md §12.2 (fixture format oac-mcpb-fixture/1).
//
// A fixture records one MCP exchange and says whether the server's messages and actions
// meet one requirement. There is one checker per requirement id; each returns true
// (conformant) or false (nonconformant), reading only `request`, `context` and the server's
// messages and actions that the requirement covers.

import { toPlain, deepEqual, isObj, isStr } from './json.mjs';
import { EXTENSION_ID, isSessionId } from './core.mjs';

const has = (o, k) => o !== null && typeof o === 'object' && Object.prototype.hasOwnProperty.call(o, k);
const OAC_TOOLS = ['send', 'reply', 'list_sessions', 'whoami'];
const LEGACY = '2025-11-25';
const MODERN = '2026-07-28';
const OAC_PREFIX = 'io.github.rossgraeber/';

// Keys the MCP specification defines in `_meta` (schema 2026-07-28, RequestMetaObject,
// NotificationMetaObject, ResultMetaObject), and those a server writes.
const MCP_META_KEYS = new Set([
  'progressToken',
  'io.modelcontextprotocol/protocolVersion',
  'io.modelcontextprotocol/clientInfo',
  'io.modelcontextprotocol/clientCapabilities',
  'io.modelcontextprotocol/logLevel',
  'io.modelcontextprotocol/subscriptionId',
  'io.modelcontextprotocol/serverInfo',
]);
const MCP_SERVER_META_KEYS = new Set(['io.modelcontextprotocol/subscriptionId', 'io.modelcontextprotocol/serverInfo']);
// Codes MCP defines in its reserved range -32020 to -32099 (schema 2026-07-28; -32042 at
// 2025-11-25 only).
const MCP_RESERVED_DEFINED = new Set([-32020, -32021, -32022, -32042]);

export function mcpBinding(fx, env) {
  const ctx = toPlain(fx.context);
  const ex = toPlain(fx.input.mcp_exchange);
  const check = CHECKS[fx.requirement];
  if (!check) throw new Error(`no mcp-binding checker for ${fx.requirement}`);
  return { result: check(ex, ctx, env) ? 'conformant' : 'nonconformant' };
}

// ---------------------------------------------------------------------------------------
// Helpers.

const req = (ex) => ex.request || null;
const method = (ex) => (req(ex) ? req(ex).method : null);
const toolName = (ex) => (method(ex) === 'tools/call' ? req(ex).params.name : null);
const isOacToolCall = (ex) => OAC_TOOLS.includes(toolName(ex));
const result = (ex) => (has(ex.server_message, 'result') ? ex.server_message.result : null);
const rpcError = (ex) => (has(ex.server_message, 'error') ? ex.server_message.error : null);
const caps = (ex) => (result(ex) && isObj(result(ex).capabilities) ? result(ex).capabilities : null);
const related = (ex) => ex.related_messages || [];
const subsequent = (ex) => ex.subsequent_messages || [];
const serverEntries = (list) => list.filter((e) => e.from === 'server');

// Every Table 8.3 code that appears in a text, as a whole hyphenated word, spelled exactly.
function codesIn(text, table83) {
  const found = [];
  for (const word of String(text).match(/[A-Za-z0-9-]+/g) || []) if (table83.has(word) && !found.includes(word)) found.push(word);
  return found;
}
const textBlocks = (res) => (res && Array.isArray(res.content) ? res.content.filter((b) => b.type === 'text').map((b) => b.text) : []);
const codesInResult = (res, table83) => [...new Set(textBlocks(res).flatMap((t) => codesIn(t, table83)))];
const isToolError = (res) => res !== null && res.isError === true;

// The presence the calling session may see of the addressed session ([SC-DLV-075]).
function seenPresence(ctx) {
  if (!ctx.addressed) return null;
  return ctx.addressed.discoverable === false ? 'unknown' : ctx.addressed.presence;
}

// The code §8.3.3 assigns to a send or reply that the context shows is refused before an
// envelope exists, or null when the context does not decide one.
function requiredRefusal(ex, ctx) {
  if (!['send', 'reply'].includes(toolName(ex))) return null;
  if (ctx.bound === false) return 'unauthorized'; // step 1, [MCPB-ATT-002]
  const seen = seenPresence(ctx);
  if (seen === 'unknown') return 'unknown-destination'; // step 2
  if (seen === 'unreachable') return 'destination-unavailable';
  return null;
}

// Does the exchange concern a session hidden from the connection ([MCPB-TOOL-021])?
const hiddenCase = (ctx) => ctx.bound === false || (ctx.addressed && ctx.addressed.discoverable === false);

// _meta objects the server wrote: in its results, notifications and later messages.
function serverMetas(ex) {
  const msgs = [ex.server_message, ...related(ex), ...serverEntries(subsequent(ex)).map((e) => e.message).filter(Boolean)];
  const metas = [];
  for (const m of msgs) {
    if (!m) continue;
    if (isObj(m.result) && isObj(m.result._meta)) metas.push(m.result._meta);
    if (isObj(m.params) && isObj(m.params._meta)) metas.push(m.params._meta);
  }
  return metas;
}

// MCP `_meta` prefix rules (§6.1): labels separated by dots, then `/`.
function prefixOf(key) {
  const slash = key.lastIndexOf('/');
  return slash < 0 ? null : key.slice(0, slash);
}
const reservedPrefix = (prefix) => {
  const labels = prefix.split('.');
  return labels.length >= 2 && (labels[1] === 'modelcontextprotocol' || labels[1] === 'mcp');
};

// The JSON-RPC error codes in the server's messages.
function serverErrorCodes(ex) {
  const msgs = [ex.server_message, ...related(ex), ...serverEntries(subsequent(ex)).map((e) => e.message).filter(Boolean)];
  return msgs.filter((m) => m && isObj(m.error)).map((m) => m.error.code);
}

// Does a JSON-RPC error carry an OAC taxonomy code (in its message or data)?
function rpcErrorCarriesOacCode(err, table83) {
  if (!err) return false;
  return codesIn(err.message || '', table83).length > 0 || codesIn(JSON.stringify(err.data ?? ''), table83).length > 0;
}

// Does the tool's input fail its inputSchema? Only the argument a fixture can rely on is
// tested: §5.1 names `to` for `send` and `in_reply_to` for `reply` (§4.4, §8.2).
function failsInputSchema(ex) {
  const args = req(ex).params.arguments;
  if (!isObj(args)) return true;
  if (toolName(ex) === 'send') return !isStr(args.to) || !Array.isArray(args.content);
  if (toolName(ex) === 'reply') return !isStr(args.in_reply_to) || !Array.isArray(args.content);
  return false;
}

const containsString = (v, s) => JSON.stringify(v).includes(s);

// Every string value inside a JSON value.
function stringsIn(v, out = []) {
  if (typeof v === 'string') out.push(v);
  else if (Array.isArray(v)) v.forEach((x) => stringsIn(x, out));
  else if (isObj(v)) Object.values(v).forEach((x) => stringsIn(x, out));
  return out;
}
// The words of a text: runs of letters, digits and hyphens.
const wordsIn = (s) => String(s).match(/[A-Za-z0-9-]+/g) || [];
// A device fingerprint is the key id: 64 lower-case hex digits (spec/security.md §5.2).
const isFingerprint = (s) => /^[0-9a-f]{64}$/.test(s);
// Delivery states (spec/session-channels.md Table 8.1); those only a receiver observes.
const STATE_NAMES = ['accepted-by-adapter', 'handed-to-harness', 'unknown', 'rejected', 'expired', 'duplicate', 'unreachable', 'failed'];
const RECEIVER_ONLY_STATES = ['handed-to-harness', 'rejected', 'expired', 'duplicate'];

// The client capabilities a request shows: `initialize` params, or modern per-request
// `_meta`. Null when the exchange does not show them (a request inside a legacy session).
function clientCaps(ex) {
  const r = req(ex);
  if (!r || !isObj(r.params)) return null;
  if (r.method === 'initialize') return isObj(r.params.capabilities) ? r.params.capabilities : {};
  const meta = isObj(r.params._meta) ? r.params._meta : null;
  if (meta && has(meta, 'io.modelcontextprotocol/protocolVersion')) {
    return isObj(meta['io.modelcontextprotocol/clientCapabilities']) ? meta['io.modelcontextprotocol/clientCapabilities'] : {};
  }
  return null;
}
// Pairing values (spec/bindings/mcp.md §4.5.2, [MCPB-ATT-005]).
const PAIRING_VALUE = /^oac-pair-[0-9a-f]{32}$/;
const isPairingWord = (w) => /^oac-pair-/i.test(w);
// The result of a call refused with `unauthorized` on an unbound Codex connection whose
// server pairs, or null.
function pairingRefusal(ex, ctx) {
  if (ctx.codex_pairing !== true || ctx.bound !== false || !isOacToolCall(ex)) return null;
  const res = result(ex);
  return isToolError(res) ? res : null;
}
const successResult = (ex) => {
  const res = result(ex);
  return res !== null && res.isError !== true ? res : null;
};

// ---------------------------------------------------------------------------------------
// One checker per requirement.

const CHECKS = {
  // §3: the identifier, exactly ([MCPB-EXT-001]); its settings object ([MCPB-EXT-003]).
  'MCPB-EXT-001': (ex) => {
    const c = caps(ex);
    if (!c || !['initialize', 'server/discover'].includes(method(ex))) return true;
    if (!isObj(c.extensions) || !has(c.extensions, EXTENSION_ID)) return false;
    return !Object.keys(c.extensions).some((k) => k !== EXTENSION_ID && k.toLowerCase() === EXTENSION_ID.toLowerCase());
  },
  'MCPB-EXT-003': (ex) => {
    const c = caps(ex);
    if (!c || !isObj(c.extensions) || !has(c.extensions, EXTENSION_ID)) return true;
    return isObj(c.extensions[EXTENSION_ID]);
  },
  // This binding revision defines no settings member (§3.6), so every member a client
  // places in its OAC settings object is unrecognized and is ignored: the request is served.
  'MCPB-EXT-004': (ex) => {
    const c = clientCaps(ex);
    if (!c || !isObj(c.extensions) || !isObj(c.extensions[EXTENSION_ID])) return true;
    if (Object.keys(c.extensions[EXTENSION_ID]).length === 0) return true;
    return result(ex) !== null;
  },

  // §4.3 eras.
  'MCPB-ERA-001': (ex, ctx) => method(ex) !== 'initialize' || ctx.era !== 'legacy' || result(ex) !== null,
  'MCPB-ERA-002': (ex) => {
    if (method(ex) !== 'initialize' || req(ex).params.protocolVersion !== LEGACY) return true;
    return result(ex) !== null && result(ex).protocolVersion === LEGACY;
  },
  // Served: only methods that a well-formed request cannot otherwise fail are decided.
  'MCPB-ERA-004': (ex, ctx) => {
    if (ctx.server_role === 'channel-path' || !['tools/list', 'server/discover'].includes(method(ex))) return true;
    const meta = req(ex).params && isObj(req(ex).params._meta) ? req(ex).params._meta : {};
    if (meta['io.modelcontextprotocol/protocolVersion'] !== MODERN) return true;
    return result(ex) !== null;
  },
  'MCPB-ERA-003': (ex, ctx) => {
    if (method(ex) !== 'initialize') return true;
    const asked = req(ex).params.protocolVersion;
    if ((ctx.supported_legacy_revisions || [LEGACY]).includes(asked)) return true;
    return result(ex) !== null && result(ex).protocolVersion === LEGACY;
  },
  'MCPB-ERA-005': (ex, ctx) => {
    if (method(ex) !== 'server/discover' || ctx.server_role === 'channel-path') return true;
    const v = result(ex) && result(ex).supportedVersions;
    return Array.isArray(v) && v.includes(MODERN) && v.includes(LEGACY);
  },
  'MCPB-ERA-008': (ex) => {
    if (method(ex) !== 'initialize' || !result(ex)) return true;
    const c = caps(ex);
    return !!c && isObj(c.extensions) && has(c.extensions, EXTENSION_ID);
  },
  'MCPB-ERA-009': (ex, ctx) => {
    if (method(ex) !== 'server/discover' || !result(ex) || ctx.server_role === 'channel-path') return true;
    const c = caps(ex);
    return !!c && isObj(c.extensions) && has(c.extensions, EXTENSION_ID);
  },
  'MCPB-ERA-011': (ex, ctx) => {
    const r = req(ex);
    if (!r) return true;
    const meta = r.params && isObj(r.params._meta) ? r.params._meta : {};
    const inScope = !has(meta, 'io.modelcontextprotocol/protocolVersion') && ctx.era !== 'legacy' && ctx.legacy_initialized !== true;
    if (!inScope) return true;
    return rpcError(ex) !== null && rpcError(ex).code === -32602;
  },

  // §4.4 attribution. Whether the connection is bound comes from `context.bound`. A call
  // served on an unbound connection was attributed to some caller; unless the client
  // asserted its own session in `_meta`, the only basis left is the connection or process.
  // The arguments (`to`, `in_reply_to`, `content`) name the addressee and the message,
  // never the caller, so they are not searched.
  'MCPB-ATT-001': (ex, ctx) => {
    if (!isOacToolCall(ex) || ctx.bound !== false || successResult(ex) === null) return true;
    const meta = isObj(req(ex).params._meta) ? req(ex).params._meta : {};
    return stringsIn(meta).flatMap(wordsIn).some((w) => isSessionId(w));
  },
  'MCPB-ATT-002':(ex, ctx, env) => {
    if (!isOacToolCall(ex) || ctx.bound !== false) return true;
    const res = result(ex);
    return isToolError(res) && codesInResult(res, env.table83).includes('unauthorized');
  },

  // §4.5 Codex issued-value pairing (binding revision 0.2). A pairing refusal is the
  // `unauthorized` refusal of a call on an unbound Codex connection that carries a value of
  // the `oac-pair-` form; the checks below apply only where `context.codex_pairing` is true.
  'MCPB-ATT-004': (ex, ctx) => {
    const res = pairingRefusal(ex, ctx);
    if (!res) return true;
    const anywhere = stringsIn(res).flatMap(wordsIn).filter(isPairingWord);
    const inText = new Set(textBlocks(res).flatMap(wordsIn).filter(isPairingWord));
    return anywhere.every((w) => inText.has(w));
  },
  'MCPB-ATT-005': (ex, ctx) => {
    const res = pairingRefusal(ex, ctx);
    if (!res) return true;
    return stringsIn(res).flatMap(wordsIn).filter(isPairingWord).every((w) => PAIRING_VALUE.test(w));
  },
  // A value issued before this exchange appears in none of the server's messages here.
  'MCPB-ATT-006': (ex, ctx) => {
    if (ctx.codex_pairing !== true) return true;
    const issued = ctx.issued_pairing_values || [];
    const msgs = [ex.server_message, ...related(ex), ...serverEntries(subsequent(ex)).map((e) => e.message).filter(Boolean)];
    return !msgs.some((m) => issued.some((v) => containsString(m, v)));
  },

  // §5 tools.
  'MCPB-TOOL-001': (ex) => {
    if (method(ex) !== 'tools/list') return true;
    const tools = result(ex) && result(ex).tools;
    if (!Array.isArray(tools)) return false;
    const names = tools.map((t) => t.name);
    return OAC_TOOLS.every((n) => names.includes(n));
  },
  'MCPB-TOOL-005': (ex, ctx, env) => {
    if (!isOacToolCall(ex) || ctx.bound === false || !failsInputSchema(ex)) return true;
    const res = result(ex);
    return isToolError(res) && codesInResult(res, env.table83).includes('invalid-request');
  },
  // The values §5.3 lists, in a `text` block of a successful result. The message id of a
  // `send` or `reply` is not decided: this revision fixes no layout that identifies it.
  'MCPB-TOOL-006': (ex) => {
    const res = successResult(ex);
    if (!isOacToolCall(ex) || !res) return true;
    const words = textBlocks(res).flatMap(wordsIn);
    if (toolName(ex) === 'whoami') return words.some(isSessionId) && words.some(isFingerprint);
    if (toolName(ex) === 'list_sessions') return CHECKS['MCPB-TOOL-015'](ex);
    return words.includes('accepted-by-adapter');
  },
  // A result reports a delivery state only when it succeeds (§5.3): a tool execution error
  // reports a refusal or a failure by its code ([MCPB-TOOL-010]), and its text is not
  // searched for state names.
  'MCPB-TOOL-008': (ex) => {
    const res = successResult(ex);
    if (!['send', 'reply'].includes(toolName(ex)) || !res) return true;
    return !stringsIn(res).flatMap(wordsIn).some((w) => RECEIVER_ONLY_STATES.includes(w));
  },
  'MCPB-TOOL-010': (ex, ctx, env) => {
    if (!isOacToolCall(ex)) return true;
    const err = rpcError(ex);
    if (err) return !(rpcErrorCarriesOacCode(err, env.table83) || requiredRefusal(ex, ctx) !== null);
    const res = result(ex);
    if (codesInResult(res, env.table83).length > 0 && res.isError !== true) return false;
    return true;
  },
  'MCPB-TOOL-011': (ex, ctx, env) => {
    const res = result(ex);
    if (!isOacToolCall(ex) || !isToolError(res)) return true;
    return codesInResult(res, env.table83).length > 0;
  },
  'MCPB-TOOL-012': (ex, ctx, env) => {
    const err = rpcError(ex);
    if (!err || !isOacToolCall(ex)) return true;
    return !(rpcErrorCarriesOacCode(err, env.table83) || requiredRefusal(ex, ctx) !== null || ctx.bound === false);
  },
  'MCPB-TOOL-013': (ex) => !serverErrorCodes(ex).some((c) => c <= -32020 && c >= -32099 && !MCP_RESERVED_DEFINED.has(c)),
  'MCPB-TOOL-015': (ex) => {
    const res = result(ex);
    if (toolName(ex) !== 'list_sessions' || !res || res.isError === true) return true;
    return textBlocks(res).some((t) => {
      let v;
      try {
        v = JSON.parse(t);
      } catch {
        return false;
      }
      return isObj(v) && Array.isArray(v.sessions)
        && v.sessions.every((d) => isObj(d) && isSessionId(d.session_id) && has(d, 'capabilities') && isObj(d.capabilities));
    });
  },
  'MCPB-TOOL-016': (ex, ctx, env) => {
    const code = requiredRefusal(ex, ctx);
    if (code === null || code === 'unauthorized') return true;
    const res = result(ex);
    if (!isToolError(res)) return false;
    const codes = codesInResult(res, env.table83);
    return codes.length === 1 && codes[0] === code;
  },
  'MCPB-TOOL-017': (ex, ctx) => {
    if (!['send', 'reply'].includes(toolName(ex)) || !ctx.addressed || ctx.addressed.discoverable !== false || ctx.bound === false) return true;
    return result(ex) !== null && deepEqual(result(ex), ctx.unknown_session_result);
  },
  'MCPB-TOOL-018': (ex, ctx, env) => {
    if (method(ex) !== 'tools/call' || OAC_TOOLS.includes(toolName(ex))) return true;
    const res = result(ex);
    if (res && codesInResult(res, env.table83).length > 0) return false;
    return rpcError(ex) !== null;
  },
  'MCPB-TOOL-019': (ex, ctx) => {
    if (!['send', 'reply'].includes(toolName(ex)) || !ctx.addressed || ctx.addressed.discoverable !== false || ctx.bound === false) return true;
    return deepEqual(related(ex), ctx.unknown_session_messages || []);
  },
  'MCPB-TOOL-020': (ex, ctx, env) => {
    if (!['send', 'reply'].includes(toolName(ex))) return true;
    const res = result(ex);
    if (!isToolError(res) || !codesInResult(res, env.table83).includes('unknown-destination')) return true;
    const to = req(ex).params.arguments.to;
    return !containsString(res, to) && !related(ex).some((m) => containsString(m, to));
  },
  'MCPB-TOOL-021': (ex, ctx) => {
    if (!hiddenCase(ctx)) return true;
    if (has(ctx, 'unknown_session_result') && !ctx.server_message_kind && result(ex) !== null && !deepEqual(result(ex), ctx.unknown_session_result)) return false;
    return deepEqual(subsequent(ex), ctx.unknown_session_subsequent_messages || []);
  },

  // §6 `_meta`.
  'MCPB-META-001': (ex) => serverMetas(ex).every((m) => Object.keys(m).every((k) => !k.startsWith(OAC_PREFIX) || k === EXTENSION_ID)),
  'MCPB-META-002': (ex) => serverMetas(ex).every((m) => !has(m, EXTENSION_ID) || isObj(m[EXTENSION_ID])),
  'MCPB-META-003': (ex) => serverMetas(ex).every((m) => Object.keys(m).every((k) => k.startsWith(OAC_PREFIX) || MCP_META_KEYS.has(k))),
  'MCPB-META-004': (ex) => serverMetas(ex).every((m) => Object.keys(m).every((k) => {
    const p = prefixOf(k);
    return p === null || !reservedPrefix(p) || MCP_SERVER_META_KEYS.has(k);
  })),
  // A session id, a fingerprint or a delivery state in a tool result's `_meta` is a value
  // the model needs ([MCPB-TOOL-006]); it must also be in a `text` block.
  'MCPB-META-006': (ex) => {
    const res = result(ex);
    if (!isOacToolCall(ex) || !res || !isObj(res._meta)) return true;
    const words = new Set(textBlocks(res).flatMap(wordsIn));
    return stringsIn(res._meta).flatMap(wordsIn)
      .filter((w) => isSessionId(w) || isFingerprint(w) || STATE_NAMES.includes(w))
      .every((w) => words.has(w));
  },
  'MCPB-META-007': (ex) => {
    const msgs = [ex.server_message, ...related(ex), ...serverEntries(subsequent(ex)).map((e) => e.message).filter(Boolean)];
    return msgs.every((m) => !(m && m.method === 'notifications/claude/channel' && isObj(m.params) && isObj(m.params.meta))
      || Object.keys(m.params.meta).every((k) => !k.includes('/')));
  },

  // §7 fallback: a client whose capabilities do not declare the identifier.
  'MCPB-FBK-001': (ex, ctx) => {
    const c = clientCaps(ex);
    if (!c || (isObj(c.extensions) && has(c.extensions, EXTENSION_ID))) return true;
    if (method(ex) === 'tools/list') return result(ex) !== null && Array.isArray(result(ex).tools);
    if (isOacToolCall(ex) && ctx.bound === true) return rpcError(ex) === null;
    return true;
  },

  // §8.1 channel path.
  'MCPB-CLD-001': (ex) => {
    const c = caps(ex);
    if (!c || !isObj(c.experimental) || !has(c.experimental, 'claude/channel')) return true;
    return method(ex) === 'initialize' && isStr(result(ex).protocolVersion) && result(ex).protocolVersion <= LEGACY;
  },
  'MCPB-CLD-002': (ex, ctx) => {
    if (ctx.era === 'legacy') return true;
    const msgs = [ex.server_message, ...related(ex), ...serverEntries(subsequent(ex)).map((e) => e.message).filter(Boolean)];
    return !msgs.some((m) => m && m.method === 'notifications/claude/channel');
  },
  // MCP 2026-07-28 names UnsupportedProtocolVersionError (-32022) as a recognized modern
  // error; G4 showed -32601 makes Claude Code fall back (§8.1). Other codes: undecided.
  'MCPB-CLD-003': (ex, ctx) => {
    if (ctx.server_role !== 'channel-path' || method(ex) !== 'server/discover') return true;
    const err = rpcError(ex);
    if (!err) return false;
    if (err.code === -32022) return false;
    if (err.code === -32601) return true;
    throw new Error(`MCPB-CLD-003: whether error code ${err.code} is a recognized modern error is not decided by spec/bindings/mcp.md`);
  },
};

export const MCPB_CHECKED = Object.keys(CHECKS);
