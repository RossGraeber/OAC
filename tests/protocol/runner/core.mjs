// Shared rules of spec/session-channels.md: identifier forms, timestamps, envelope-stage
// validation (§4, §5, §8.3.2), capability negotiation (§6.4, §6.5), presence records
// (§7.2.2) and receipts (§8.1.4). Every function cites the requirement it applies.

import { JNum, parse, decodeUtf8Strict, isObj, isStr, isBool, isPlainInt, containsNull, ijsonViolation, hasLoneSurrogate } from './json.mjs';

export const EXTENSION_ID = 'io.github.rossgraeber/oac-session-channels';
export const MAX_SAFE = '9007199254740991';
export const DEFAULT_MAX_OCTETS = 65536; // [SC-ENV-004]
export const REPLAY_WINDOW_NS = 300000n * 1000000n; // spec/security.md [SEC-RPL-001]
export const REPLY_PERIOD_NS = 86400000n * 1000000n; // spec/security.md §9.5
export const PRESENCE_CAP_MS = 300000; // spec/security.md [SEC-PRS-007]

// §4.3 identifier token, tested against the whole value ([SC-ENV-011]).
export const isToken = (v) => isStr(v) && /^[A-Za-z0-9._:-]{1,128}$/.test(v) && !/\n/.test(v);
// §6.1 session id ([SC-ID-001], [SC-ID-002]).
export const isSessionId = (v) => isStr(v) && /^[0-7][0-9a-hjkmnp-tv-z]{25}$/.test(v) && !/\n/.test(v);
// §4.4.1 version ([SC-ENV-020]).
export const isVersion = (v) => isStr(v) && /^(0|[1-9][0-9]{0,3})\.(0|[1-9][0-9]{0,3})$/.test(v) && !/\n/.test(v);
export const majorOf = (version) => Number(version.split('.')[0]);
// §4.5.2 part types.
export const isCoreType = (t) => isStr(t) && /^[a-z][a-z0-9-]{0,31}$/.test(t) && !/\n/.test(t);
const LABEL = '[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?';
const EXT_TYPE = new RegExp(`^${LABEL}(?:\\.${LABEL})+/[a-z][a-z0-9-]{0,63}$`);
export const isExtensionType = (t) => isStr(t) && EXT_TYPE.test(t) && !/\n/.test(t);
// spec/security.md §5.2 key id, and §6.1 nonce and signature forms.
export const isKeyId = (v) => isStr(v) && /^[0-9a-f]{64}$/.test(v) && !/\n/.test(v);
export const isNonceForm = (v) => isStr(v) && /^[A-Za-z0-9_-]{21}[AQgw]$/.test(v) && !/\n/.test(v);
export const isSignatureForm = (v) => isStr(v) && /^[A-Za-z0-9_-]{85}[AQgw]$/.test(v) && !/\n/.test(v);

export function b64urlDecode(s) {
  return Buffer.from(s.replace(/-/g, '+').replace(/_/g, '/'), 'base64');
}

// §4.4.6 timestamp: RFC 3339 date-time, upper-case T and Z, 1-9 fraction digits, seconds
// 00-59, a real calendar date. Returns nanoseconds since the epoch as a BigInt, or null.
export function parseTimestamp(v) {
  if (!isStr(v)) return null;
  const m = /^([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})(?:\.([0-9]{1,9}))?Z$/.exec(v);
  if (!m || /\n/.test(v)) return null;
  const [y, mo, d, h, mi, s] = m.slice(1, 7).map(Number);
  if (mo < 1 || mo > 12 || h > 23 || mi > 59 || s > 59) return null;
  const leap = (y % 4 === 0 && y % 100 !== 0) || y % 400 === 0;
  const dim = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][mo - 1];
  if (d < 1 || d > dim) return null;
  const days = daysFromCivil(y, mo, d);
  const frac = BigInt((m[7] || '').padEnd(9, '0') || '0');
  return ((BigInt(days) * 86400n + BigInt(h * 3600 + mi * 60 + s)) * 1000000000n) + frac;
}

function daysFromCivil(y, m, d) {
  y -= m <= 2 ? 1 : 0;
  const era = Math.floor(y / 400);
  const yoe = y - era * 400;
  const doy = Math.floor((153 * (m + (m > 2 ? -3 : 9)) + 2) / 5) + d - 1;
  const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
  return era * 146097 + doe - 719468;
}

export const msToNs = (ms) => BigInt(ms) * 1000000n;

// ---------------------------------------------------------------------------------------
// Envelope-stage validation, in the step order of §8.3.2 ([SC-RCP-071]).
//
// `raw` is the input as a parsed tree (json.mjs) or a parse failure; `octets` the serialized
// size. Returns { result: 'valid' } or { result: 'rejected'|'expired', error }.

export function envelopeStage({ parsed, parseError, octets }, ctx) {
  const supportedTypes = ctx.supportedTypes || ['text'];
  const sizeLimit = ctx.sizeLimit || DEFAULT_MAX_OCTETS;
  // Step 1: receiver-wide size limit ([SC-ENV-004], [SC-RCP-076]).
  if (octets > sizeLimit) return rej('envelope-too-large');
  // Step 2: encoding ([SC-ENV-001], [SC-ENV-002]) and `version` ([SC-ENV-020]).
  if (parseError) return rej('malformed-envelope');
  const env = parsed;
  if (!isObj(env)) return rej('malformed-envelope');
  if (ijsonViolation(env)) return rej('malformed-envelope');
  if (!Object.prototype.hasOwnProperty.call(env, 'version') || !isVersion(env.version)) return rej('malformed-envelope');
  // Step 3: major version ([SC-VER-001]).
  if (!ctx.supportedMajors.includes(majorOf(env.version))) return rej('unsupported-version');
  // Step 4: every other requirement of §4, §5.4 and §6.1.
  if (structureViolation(env)) return rej('malformed-envelope');
  // Step 5: part types the receiver supports for no session ([SC-ENV-065], [SC-RCP-076]).
  for (const part of env.content) if (!(part.type === 'text' || supportedTypes.includes(part.type))) return rej('unsupported-content-type');
  // Step 6: expiry ([SC-ENV-100]).
  if (ctx.receiverTimeNs !== undefined && expiryNs(env) !== null && expiryNs(env) <= ctx.receiverTimeNs) {
    return { result: 'expired', error: 'expired' };
  }
  return { result: 'valid' };
}

const rej = (error) => ({ result: 'rejected', error });

export function expiryNs(env) {
  if (!Object.prototype.hasOwnProperty.call(env, 'ttl_ms')) return null;
  const ttl = env.ttl_ms instanceof JNum ? env.ttl_ms.value : env.ttl_ms;
  return parseTimestamp(env.created_at) + msToNs(ttl);
}

// Returns a description of the first §4/§6.1 rule the (I-JSON, versioned) envelope breaks.
export function structureViolation(env) {
  if (containsNull(env)) return 'SC-ENV-003 null member';
  for (const m of ['id', 'from', 'to', 'created_at', 'content', 'security']) {
    if (!Object.prototype.hasOwnProperty.call(env, m)) return `missing ${m}`;
  }
  for (const m of ['id', 'from', 'to', 'conversation_id', 'reply_to', 'correlation_id']) {
    if (Object.prototype.hasOwnProperty.call(env, m) && !isToken(env[m])) return `SC-ENV-010 ${m}`;
  }
  if (!isSessionId(env.from) || !isSessionId(env.to)) return 'SC-ID-001';
  if (parseTimestamp(env.created_at) === null) return 'SC-ENV-041';
  if (Object.prototype.hasOwnProperty.call(env, 'ttl_ms') && !isPlainInt(env.ttl_ms, 1, 86400000)) return 'SC-ENV-050';
  if (!Array.isArray(env.content) || env.content.length < 1) return 'SC-ENV-060';
  for (const part of env.content) {
    if (!isObj(part) || !isStr(part.type)) return 'SC-ENV-061';
    if (part.type === 'text' && !(isStr(part.text) && part.text.length >= 1)) return 'SC-ENV-062';
  }
  const sec = env.security;
  if (!isObj(sec)) return 'SC-ENV-070';
  const SEC = ['principal', 'key_id', 'nonce', 'signature'];
  for (const m of SEC) if (!Object.prototype.hasOwnProperty.call(sec, m)) return 'SC-ENV-071';
  for (const m of SEC) if (!isStr(sec[m])) return 'SC-ENV-072';
  if (Object.keys(sec).some((k) => !SEC.includes(k))) return 'SC-ENV-073';
  return null;
}

// Reads an input of the §3.3 kinds into { parsed, parseError, octets }.
export function readEnvelopeInput(rawInput) {
  if (Object.prototype.hasOwnProperty.call(rawInput, 'envelope')) {
    const text = serializeRaw(rawInput.envelope);
    return { parsed: rawInput.envelope, octets: Buffer.byteLength(text, 'utf8') };
  }
  if (Object.prototype.hasOwnProperty.call(rawInput, 'envelope_text')) {
    const text = rawInput.envelope_text;
    if (hasLoneSurrogate(text)) return { parseError: 'lone surrogate in text', octets: text.length };
    try {
      return { parsed: parse(text), octets: Buffer.byteLength(text, 'utf8') };
    } catch (e) {
      return { parseError: e.message, octets: Buffer.byteLength(text, 'utf8') };
    }
  }
  if (Object.prototype.hasOwnProperty.call(rawInput, 'envelope_base64')) {
    const bytes = Buffer.from(rawInput.envelope_base64, 'base64');
    const text = decodeUtf8Strict(bytes);
    if (text === null) return { parseError: 'not UTF-8', octets: bytes.length };
    try {
      return { parsed: parse(text), octets: bytes.length };
    } catch (e) {
      return { parseError: e.message, octets: bytes.length };
    }
  }
  throw new Error('input holds none of envelope, envelope_text, envelope_base64');
}

// Compact serialization of a parsed tree, keeping each number's spelling.
export function serializeRaw(v) {
  if (v instanceof JNum) return v.raw;
  if (v === null || typeof v !== 'object') return JSON.stringify(v);
  if (Array.isArray(v)) return '[' + v.map(serializeRaw).join(',') + ']';
  return '{' + Object.keys(v).map((k) => JSON.stringify(k) + ':' + serializeRaw(v[k])).join(',') + '}';
}

// ---------------------------------------------------------------------------------------
// Capability declarations and negotiation (§6.4, §6.5).

// The valid entries of a declaration for the implemented list: [{ extension, major, entry }].
export function validEntries(declaration, implemented) {
  if (!isObj(declaration)) return []; // [SC-ID-070]
  const out = [];
  for (const impl of implemented) {
    if (!Object.prototype.hasOwnProperty.call(declaration, impl.extension)) continue; // [SC-ID-082]
    const e = declaration[impl.extension];
    if (entryValid(e, impl.major)) out.push({ extension: impl.extension, major: impl.major, revision: impl.revision, entry: e });
  }
  return out;
}

// [SC-ID-061], [SC-ID-062], [SC-ID-064], [SC-ID-066]; a failing entry is absent ([SC-ID-068]).
export function entryValid(e, major) {
  if (!isObj(e)) return false;
  if (!isVersion(e.revision) || majorOf(e.revision) !== major) return false;
  if (!isBool(e.active_inbound)) return false;
  if (Object.prototype.hasOwnProperty.call(e, 'content_types')) {
    if (!Array.isArray(e.content_types)) return false;
    if (!e.content_types.every((t) => isCoreType(t) || isExtensionType(t))) return false;
  }
  if (Object.prototype.hasOwnProperty.call(e, 'max_envelope_octets') && !isPlainInt(e.max_envelope_octets, 65536, MAX_SAFE)) return false;
  return true;
}

// [SC-ID-083]: the highest major with a valid entry, or null.
export function agree(declaration, implemented) {
  const entries = validEntries(declaration, implemented);
  if (entries.length === 0) return null;
  return entries.reduce((a, b) => (b.major > a.major ? b : a));
}

// Sender refusal steps 3-6 of §8.3.3, given a declaration the sender holds. Returns
// { result: 'sent', version } or { result: 'refused', error }.
export function senderChecks(declaration, implemented, content, envelopeOctets) {
  const agreed = agree(declaration, implemented);
  if (!agreed) return { result: 'refused', error: 'unsupported-version' }; // [SC-ID-084]
  if (agreed.entry.active_inbound !== true) return { result: 'refused', error: 'unsupported-capability' }; // [SC-ID-100]
  const listed = Array.isArray(agreed.entry.content_types) ? agreed.entry.content_types : [];
  for (const part of content) {
    if (!(part.type === 'text' || listed.includes(part.type))) return { result: 'refused', error: 'unsupported-content-type' }; // [SC-ID-101]
  }
  const limit = agreed.entry.max_envelope_octets ? Number(agreed.entry.max_envelope_octets.raw) : DEFAULT_MAX_OCTETS;
  if (envelopeOctets > limit) return { result: 'refused', error: 'envelope-too-large' }; // [SC-ENV-005]
  return { result: 'sent', version: agreed.revision }; // [SC-ID-087]
}

// Size of the envelope a sender would build for a request (header members at their
// largest plausible lengths); only a request near the limit can depend on the estimate.
export function estimateEnvelopeOctets(from, to, version, content) {
  const env = {
    version,
    id: 'x'.repeat(128),
    from,
    to,
    created_at: '2026-10-03T12:00:00.000000000Z',
    ttl_ms: 86400000,
    content,
    security: { principal: 'x'.repeat(128), key_id: 'x'.repeat(64), nonce: 'x'.repeat(22), signature: 'x'.repeat(86) },
  };
  return Buffer.byteLength(serializeRaw(env), 'utf8');
}

// ---------------------------------------------------------------------------------------
// Presence records (§7.2.2): the first of [SC-DLV-021] to [SC-DLV-031] the record breaks.

export function presenceRecordViolation(rec) {
  if (!isObj(rec) || ijsonViolation(rec) || containsNull(rec)) return 'SC-DLV-021';
  for (const m of ['session_id', 'seq', 'present', 'issued_at']) if (!Object.prototype.hasOwnProperty.call(rec, m)) return 'SC-DLV-022';
  if (!isSessionId(rec.session_id)) return 'SC-DLV-023';
  if (!isPlainInt(rec.seq, 0, MAX_SAFE)) return 'SC-DLV-024';
  if (!isBool(rec.present)) return 'SC-DLV-025';
  if (parseTimestamp(rec.issued_at) === null) return 'SC-DLV-026';
  const has = (m) => Object.prototype.hasOwnProperty.call(rec, m);
  if (rec.present) {
    if (!has('lifetime_ms') || !has('descriptor')) return 'SC-DLV-027';
    if (!isPlainInt(rec.lifetime_ms, 1000, 3600000)) return 'SC-DLV-028';
    const d = rec.descriptor;
    if (!isObj(d) || !isSessionId(d.session_id) || !Object.prototype.hasOwnProperty.call(d, 'capabilities')) return 'SC-DLV-029';
    if (d.session_id !== rec.session_id) return 'SC-DLV-030';
  } else if (has('lifetime_ms') || has('descriptor')) {
    return 'SC-DLV-031';
  }
  return null;
}

// ---------------------------------------------------------------------------------------
// Delivery states and receipts (§8.1).

export const STATES = ['accepted-by-adapter', 'handed-to-harness', 'unknown', 'rejected', 'expired', 'duplicate', 'unreachable', 'failed'];
export const OBSERVERS = {
  // Table 8.1, "Observer" column: `either` is both.
  sender: ['accepted-by-adapter', 'unknown', 'unreachable', 'failed'],
  receiver: ['handed-to-harness', 'unknown', 'rejected', 'expired', 'duplicate', 'unreachable', 'failed'],
};
export const ERROR_CARRYING = ['rejected', 'expired', 'duplicate', 'unreachable', 'failed'];
export const ERROR_STATES = ['rejected', 'expired', 'unreachable', 'failed']; // §8.4.1

// Table 8.3, read from the spec text: Map code -> { stage, state, scope[] }.
export function readTable83(specText) {
  const start = specText.indexOf('Table 8.3.\n');
  if (start < 0) throw new Error('Table 8.3 not found in spec/session-channels.md');
  const table = new Map();
  for (const line of specText.slice(start).split('\n').slice(1)) {
    if (line.trim() === '') {
      if (table.size) break;
      continue;
    }
    const m = /^\| `([a-z][a-z0-9-]*)` \| ([a-z]+) \| (?:`([a-z-]+)`|none) \| ([a-z, ]+) \|/.exec(line);
    if (!m) continue;
    table.set(m[1], { stage: m[2], state: m[3] || null, scope: m[4].split(',').map((s) => s.trim()) });
  }
  return table;
}

// [SC-RCP-020] to [SC-RCP-028]: the first rule a receipt breaks, or null.
export function receiptViolation(r, table83) {
  if (!isObj(r) || ijsonViolation(r) || containsNull(r)) return 'SC-RCP-020';
  for (const m of ['envelope_id', 'envelope_from', 'state', 'observer', 'observed_at']) if (!Object.prototype.hasOwnProperty.call(r, m)) return 'SC-RCP-021';
  if (!isToken(r.envelope_id) || !isToken(r.envelope_from)) return 'SC-RCP-022';
  if (!isStr(r.state) || !STATES.includes(r.state)) return 'SC-RCP-001';
  if (r.observer !== 'sender' && r.observer !== 'receiver') return 'SC-RCP-023';
  if (!OBSERVERS[r.observer].includes(r.state)) return 'SC-RCP-002';
  if (parseTimestamp(r.observed_at) === null) return 'SC-RCP-024';
  const hasError = Object.prototype.hasOwnProperty.call(r, 'error');
  if (ERROR_CARRYING.includes(r.state) && !hasError) return 'SC-RCP-025';
  if (!ERROR_CARRYING.includes(r.state) && hasError) return 'SC-RCP-026';
  if (hasError) {
    if (!isStr(r.error) || !/^[a-z][a-z0-9-]{0,63}$/.test(r.error) || /\n/.test(r.error)) return 'SC-RCP-027';
    const row = table83.get(r.error);
    if (row && !(row.state === r.state && row.scope.includes(r.observer))) return 'SC-RCP-028';
  }
  return null;
}

// [SC-RCP-030]: the state a peer processes a valid receipt as.
export function effectiveState(r, table83) {
  if (Object.prototype.hasOwnProperty.call(r, 'error') && !table83.has(r.error) && r.state !== 'duplicate') return 'failed';
  return r.state;
}
