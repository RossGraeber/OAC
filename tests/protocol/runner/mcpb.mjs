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

  // §4.3 eras.
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

  // §4.4 attribution.
  'MCPB-ATT-002': (ex, ctx, env) => {
    if (!isOacToolCall(ex) || ctx.bound !== false) return true;
    const res = result(ex);
    return isToolError(res) && codesInResult(res, env.table83).includes('unauthorized');
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
  'MCPB-META-007': (ex) => {
    const msgs = [ex.server_message, ...related(ex), ...serverEntries(subsequent(ex)).map((e) => e.message).filter(Boolean)];
    return msgs.every((m) => !(m && m.method === 'notifications/claude/channel' && isObj(m.params) && isObj(m.params.meta))
      || Object.keys(m.params.meta).every((k) => !k.includes('/')));
  },

  // §8.1 channel path.
  'MCPB-CLD-001': (ex) => {
    const c = caps(ex);
    if (!c || !isObj(c.experimental) || !has(c.experimental, 'claude/channel')) return true;
    return method(ex) === 'initialize' && isStr(result(ex).protocolVersion) && result(ex).protocolVersion <= LEGACY;
  },
};

export const MCPB_CHECKED = Object.keys(CHECKS);
