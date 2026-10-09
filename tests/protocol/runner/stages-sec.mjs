// Fixture stages of spec/security.md §3.3: `security`, `replay`, `key-id`, `key-removal`,
// `registration`, `receipt-auth`, `presence-auth`, `discovery-auth`, `exchange`,
// `provenance` and `body`.

import { createHash } from 'node:crypto';
import { isObj, isStr, toPlain, deepEqual } from './json.mjs';
import { canonicalize, signingInput } from './jcs.mjs';
import { verify } from './ed25519.mjs';
import {
  envelopeStage, readEnvelopeInput, parseTimestamp, isToken, isKeyId, isNonceForm, isSignatureForm,
  b64urlDecode, presenceRecordViolation, receiptViolation, REPLAY_WINDOW_NS, REPLY_PERIOD_NS, PRESENCE_CAP_MS,
} from './core.mjs';

const has = (o, k) => Object.prototype.hasOwnProperty.call(o, k);
const clone = (v) => JSON.parse(JSON.stringify(v));

// ---------------------------------------------------------------------------------------
// The state one implementation holds, built from a stage's `context`.

export function makeState(c) {
  return {
    ownKeyId: c.own_key_id,
    trustedKeys: clone(c.trusted_keys || []),
    bindings: clone(c.bindings || {}),
    sessions: clone(c.sessions || {}),
    grants: clone(c.grants || []),
    sent: clone(c.sent || []),
    handedOff: clone(c.handed_off || []),
    latestSeq: clone(c.latest_seq || {}),
    dupStore: new Map((c.duplicate_store || []).map((e) => [dupKey(e.key_id, e.nonce), { receiptSent: false }])),
    issued: new Set(), // "<session>|<device>" announcements issued ([SEC-PRS-010])
  };
}

const dupKey = (keyId, nonce) => JSON.stringify([keyId, nonce]); // [SEC-RPL-020]: exact strings

// [SEC-KEY-030] with the forms of §5 (Table 7.1, step 1).
function resolveKey(state, principal, keyId) {
  if (!isToken(principal) || !isKeyId(keyId)) return null; // [SEC-KEY-020], §5.2
  return state.trustedKeys.find((k) => k.principal === principal && k.key_id === keyId) || null; // [SEC-KEY-011]
}

// Verifies the signature of a signed object (§6.2, §6.3) under a trusted-key entry.
function verifySigned(entry, domain, obj) {
  const sig = obj.security.signature;
  if (!isSignatureForm(sig)) return false; // [SEC-SIG-004]
  const { bytes } = signingInput(domain, obj);
  return verify(b64urlDecode(entry.public_key), bytes, b64urlDecode(sig));
}

// Is the session one of the implementation's own sessions?
const isOwn = (state, sid) => has(state.sessions, sid);

// Does a grant side on the implementation's own device include own session `sid`? (§9.2)
// A session-id side names that one session id, bound or not ([SEC-AUZ-006]): a grant can
// outlive its session's binding, and the delivery stage then reports `unknown-destination`
// (spec/session-channels.md Table 8.3.3, [SC-ID-155]). A scope or device side covers only
// the sessions the implementation binds now.
function ownSideIncludes(state, side, sid) {
  if (has(side, 'session_id')) return side.session_id === sid; // [SEC-AUZ-006]
  if (!isOwn(state, sid)) return false;
  if (has(side, 'working_directory_scope')) return side.working_directory_scope === state.sessions[sid].working_directory_scope; // [SEC-AUZ-008]
  if (side.device === true) return true;
  return false;
}

// Does a grant side on another device name key K and session R? (`session_id` optional)
const remoteSideNames = (side, K, R) => side.key_id === K && (!has(side, 'session_id') || side.session_id === R);

// An inbound grant covers an envelope verified under K, from `from`, to `to` (§9.2).
const inboundCovers = (state, g, K, from, to) => g.direction === 'inbound' && remoteSideNames(g.writer, K, from) && ownSideIncludes(state, g.target, to);

// A live reply right covers the envelope ([SEC-AUZ-014], [SEC-AUZ-015]).
function replyRightCovers(state, envl, K, nowNs) {
  return state.sent.some((e) => has(envl, 'reply_to') && envl.reply_to === e.id && envl.from === e.to && envl.to === e.from
    && K === e.to_key_id && isOwn(state, e.from) && nowNs < parseTimestamp(e.created_at) + REPLY_PERIOD_NS);
}

// ---------------------------------------------------------------------------------------
// The security stage (§7.1) after envelope-stage validation.

export function securityCheck(state, input, nowNs, supportedMajors, opts = {}) {
  const read = readEnvelopeInput(input);
  const envStage = envelopeStage(read, { supportedMajors, receiverTimeNs: nowNs, supportedTypes: opts.supportedTypes, sizeLimit: opts.sizeLimit });
  if (envStage.result !== 'valid') return { ...envStage, receipt_permitted: false }; // [SEC-STG-001], [SC-RCP-041]
  const raw = read.parsed;
  const envl = toPlain(raw);
  const out = { canonical: signingInput('oac-envelope-v1', raw).text, record: 'none' };
  const sec = envl.security;
  // Step 1: key resolution.
  const key = resolveKey(state, sec.principal, sec.key_id);
  if (!key) return { ...out, result: 'rejected', error: 'unknown-key', receipt_permitted: false };
  // Step 2: signature ([SEC-SIG-003], [SEC-SIG-004], §6.3).
  if (!isNonceForm(sec.nonce) || !verifySigned(key, 'oac-envelope-v1', raw)) {
    return { ...out, result: 'rejected', error: 'signature-invalid', receipt_permitted: false };
  }
  const K = key.key_id;
  // Step 3: replay window on arrival ([SEC-RPL-002], [SEC-RPL-003]).
  const c = parseTimestamp(envl.created_at);
  if (!(nowNs - REPLAY_WINDOW_NS < c && c < nowNs + REPLAY_WINDOW_NS)) return { ...out, result: 'expired', error: 'outside-replay-window' };
  // Step 4: authorization.
  const b = state.bindings[envl.from];
  if (b !== undefined && (isObj(b) || b !== K)) {
    // [SEC-AUZ-003]; a session id bound to a different key records a finding ([SEC-PRS-004]).
    return { ...out, result: 'rejected', error: 'unauthorized', record: isObj(b) ? 'none' : 'finding' };
  }
  const covered = state.grants.some((g) => inboundCovers(state, g, K, envl.from, envl.to)) || replyRightCovers(state, envl, K, nowNs);
  if (!covered) return { ...out, result: 'rejected', error: 'unauthorized' }; // [SEC-AUZ-001], [SEC-AUZ-002]
  // Step 5: duplicate suppression ([SEC-RPL-021]).
  const dk = dupKey(K, sec.nonce);
  if (state.dupStore.has(dk)) return { ...out, result: 'duplicate', error: 'duplicate', dupKey: dk };
  return { ...out, result: 'passed', K, envl, dupKey: dk };
}

// Applies a passed envelope's binding claim ([SEC-PRS-005]) and adds its store entry.
function admit(state, chk) {
  if (state.bindings[chk.envl.from] === undefined) state.bindings[chk.envl.from] = chk.K;
  state.dupStore.set(chk.dupKey, { receiptSent: false });
}

// security (§3.3).
export function securityStage(fx, env, opts = {}) {
  const c = toPlain(fx.context);
  const state = makeState(c);
  const chk = securityCheck(state, fx.input, parseTimestamp(c.receiver_time), c.supported_major_versions, opts);
  if (chk.result === 'passed') admit(state, chk);
  const out = { result: chk.result };
  if (chk.error) out.error = chk.error;
  if (has(chk, 'canonical')) out.canonical = chk.canonical;
  out.receipt_permitted = chk.receipt_permitted === false ? false : true;
  out.bindings_after = state.bindings;
  out.record = chk.record || 'none';
  return out;
}
// The `security` stage's context (spec/security.md §3.3) lists its members in place of those
// of spec/session-channels.md §3.3, and `max_envelope_octets` is not one of them: a security
// fixture is judged at the default receiver-wide limit, so the member is refused, not ignored.
export function security(fx, env) {
  if (Object.prototype.hasOwnProperty.call(fx.context, 'max_envelope_octets')) throw new Error('context.max_envelope_octets is not a member of the security stage (spec/security.md §3.3)');
  return securityStage(fx, env);
}

// ---------------------------------------------------------------------------------------
// replay (§8): arrivals at one receiver, starting from an empty duplicate store.

function arrive(state, envelopeRaw, at, delivery, supportedMajors) {
  const nowNs = parseTimestamp(at);
  const chk = securityCheck(state, { envelope: envelopeRaw }, nowNs, supportedMajors);
  if (chk.result === 'duplicate') {
    const entry = state.dupStore.get(chk.dupKey);
    const allowed = !entry.receiptSent; // [SEC-RPL-030]
    entry.receiptSent = true;
    return { result: 'duplicate', error: 'duplicate', duplicate_receipt_allowed: allowed };
  }
  if (chk.result !== 'passed') return { result: chk.result, error: chk.error };
  admit(state, chk);
  const outcome = delivery || 'handed-to-harness';
  const envl = chk.envl;
  if (outcome === 'handed-to-harness' || outcome === 'unknown') {
    state.handedOff.push({ id: envl.id, from: envl.from, to: envl.to, created_at: envl.created_at });
    return { result: outcome };
  }
  state.dupStore.delete(chk.dupKey); // [SEC-RPL-022]
  if (outcome === 'destination-unavailable') return { result: 'unreachable', error: 'destination-unavailable' };
  if (outcome === 'handoff-failed') return { result: 'failed', error: 'handoff-failed' };
  throw new Error(`unknown delivery outcome ${outcome}`);
}

export function replay(fx) {
  const c = toPlain(fx.context);
  if (c.replay_window_ms !== 300000) throw new Error('replay_window_ms is not 300000 ([SEC-RPL-001])');
  const state = makeState(c);
  const results = fx.input.arrivals.map((a) => {
    const ap = toPlain(a);
    if (ap.grants_add) state.grants.push(...ap.grants_add);
    // A copy that arrives during the previous hand-off waits for its outcome ([SEC-RPL-026]);
    // arrivals are processed in order, so that outcome is already applied.
    return arrive(state, fx.input.envelopes[ap.envelope], ap.at, ap.delivery, c.supported_major_versions);
  });
  return { results };
}

// ---------------------------------------------------------------------------------------
// key-id (§5.2) and key-removal (§5.3).

export function keyId(fx) {
  return { key_id: createHash('sha256').update(b64urlDecode(fx.input.public_key)).digest('hex') }; // [SEC-KEY-010]
}

export function keyRemoval(fx) {
  const c = toPlain(fx.context);
  const K = fx.input.remove_key_id;
  const bindings = {};
  for (const [sid, v] of Object.entries(c.bindings)) {
    if (v === K || (isObj(v) && v.conflict.includes(K))) continue; // [SEC-KEY-035]
    bindings[sid] = v;
  }
  const names = (side) => side && side.key_id === K;
  const grants = c.grants.filter((g) => !names(g.writer) && !names(g.target));
  return { bindings_after: bindings, grants_after: grants };
}

// ---------------------------------------------------------------------------------------
// registration (§5.4).

export function registration(fx) {
  const state = makeState(toPlain(fx.context));
  const rec = fx.input.record;
  const MEMBERS = ['session_id', 'harness_label', 'native_id', 'working_directory', 'registered_at', 'security'];
  const invalid = { result: 'invalid', binding_usable: false };
  if (!isObj(rec) || Object.keys(rec).length !== MEMBERS.length || !MEMBERS.every((m) => has(rec, m))) return invalid; // [SEC-KEY-040]
  const sec = rec.security;
  if (!isObj(sec) || Object.keys(sec).sort().join() !== 'key_id,principal,signature') return invalid;
  const key = resolveKey(state, sec.principal, sec.key_id);
  if (!key || !verifySigned(key, 'oac-registration-v1', rec)) return invalid; // [SEC-KEY-041], [SEC-KEY-043]
  return { result: 'verified', canonical: signingInput('oac-registration-v1', rec).text };
}

// ---------------------------------------------------------------------------------------
// receipt-auth (§10.2).

export function receiptAuth(fx, env) {
  const state = makeState(toPlain(fx.context));
  const ar = fx.input.authenticated_receipt;
  const out = {};
  if (isObj(ar)) {
    try {
      out.canonical = signingInput('oac-receipt-v1', ar).text;
    } catch {
      // not canonicalizable
    }
  }
  const discard = { ...out, result: 'discarded' };
  if (!isObj(ar) || !isObj(ar.security) || Object.keys(ar.security).sort().join() !== 'key_id,principal,signature') return discard;
  if (receiptViolation(ar.receipt, env.table83)) return discard; // [SC-RCP-032]
  const key = resolveKey(state, ar.security.principal, ar.security.key_id); // check 1
  if (!key || !verifySigned(key, 'oac-receipt-v1', ar)) return discard; // check 2
  const r = toPlain(ar.receipt);
  const sent = state.sent.find((s) => s.id === r.envelope_id && s.from === r.envelope_from && s.to === ar.envelope_to && s.nonce === ar.envelope_nonce);
  if (!sent) return discard; // check 3
  if (state.bindings[ar.envelope_to] !== key.key_id) return discard; // check 4 (a conflict mark binds no key)
  if (r.observer !== 'receiver') return discard; // check 5
  return { ...out, result: 'authenticated' };
}

// ---------------------------------------------------------------------------------------
// presence-auth (§11).

// [SEC-AUZ-017]: is key K related to session R for this consumer?
function related(state, K, R, nowNs) {
  const names = (side) => remoteSideNames(side, K, R);
  if (state.grants.some((g) => (g.direction === 'inbound' && names(g.writer)) || (g.direction === 'outbound' && names(g.target)))) return true;
  if (state.sent.some((e) => e.to === R && e.to_key_id === K && nowNs < parseTimestamp(e.created_at) + REPLY_PERIOD_NS)) return true;
  if (state.handedOff.some((h) => h.from === R && nowNs < parseTimestamp(h.created_at) + REPLY_PERIOD_NS)) return true;
  return false;
}

export function acceptPresence(state, ar, nowNs) {
  const out = { record: 'none' };
  if (isObj(ar)) {
    try {
      out.canonical = signingInput('oac-presence-v1', ar).text;
    } catch {
      // not canonicalizable
    }
  }
  const discard = () => ({ ...out, result: 'discarded' });
  if (!isObj(ar) || !has(ar, 'record') || !has(ar, 'audience') || !isObj(ar.security)) return discard();
  if (Object.keys(ar.security).sort().join() !== 'key_id,principal,signature') return discard();
  // Signature ([SEC-PRS-002]).
  const key = resolveKey(state, ar.security.principal, ar.security.key_id);
  if (!key || !verifySigned(key, 'oac-presence-v1', ar)) return discard();
  const K = key.key_id;
  // Audience ([SEC-PRS-013]).
  if (ar.audience !== state.ownKeyId) return discard();
  const rec = ar.record;
  if (!isObj(rec)) return discard();
  // Freshness ([SEC-PRS-006]).
  const issued = parseTimestamp(rec.issued_at);
  if (issued === null || !(nowNs - REPLAY_WINDOW_NS < issued && issued < nowNs + REPLAY_WINDOW_NS)) return discard();
  // Conflict ([SEC-PRS-003], [SEC-PRS-012], [SEC-PRS-014], [SEC-PRS-015]).
  const R = rec.session_id;
  const b = isStr(R) ? state.bindings[R] : undefined;
  if (isObj(b)) return discard(); // [SEC-PRS-012]
  if (b !== undefined && b !== K) {
    if (!isOwn(state, R) && related(state, K, R, nowNs)) {
      state.bindings[R] = { conflict: [b, K].sort() }; // [SEC-PRS-014]
    }
    return { ...discard(), record: 'finding' }; // [SEC-PRS-004]
  }
  // Relation ([SEC-AUZ-017]).
  if (rec.present === true && !related(state, K, R, nowNs)) return discard();
  // spec/session-channels.md §7.2.3.
  if (presenceRecordViolation(rec)) return discard(); // [SC-DLV-040]
  if (has(state.latestSeq, R) && BigInt(rec.seq.raw) <= BigInt(state.latestSeq[R])) return discard(); // [SC-DLV-042]
  if (rec.present !== true && b === undefined) return discard(); // [SEC-PRS-005]
  state.latestSeq[R] = Number(rec.seq.raw);
  if (b === undefined) state.bindings[R] = K; // [SEC-PRS-005]
  const accepted = { ...out, result: 'accepted' };
  if (rec.present === true) accepted.effective_lifetime_ms = Math.min(rec.lifetime_ms.value, PRESENCE_CAP_MS); // [SEC-PRS-007]
  return accepted;
}

export function presenceAuth(fx) {
  const c = toPlain(fx.context);
  const state = makeState(c);
  const out = acceptPresence(state, fx.input.authenticated_record, parseTimestamp(c.consumer_time));
  out.bindings_after = state.bindings;
  return out;
}

// ---------------------------------------------------------------------------------------
// discovery-auth (§9.4, §9.5).

export function mayDiscover(state, requester, session, nowNs) {
  if (has(requester, 'session_id')) {
    const L = requester.session_id;
    // [SEC-AUZ-016]: a session handed an envelope may discover its sender for the reply period.
    if (state.handedOff.some((h) => h.to === L && h.from === session && nowNs < parseTimestamp(h.created_at) + REPLY_PERIOD_NS)) return true;
    if (isOwn(state, session)) {
      // [SEC-AUZ-010]: an inbound grant with the own key id as writer covers L -> session.
      return state.grants.some((g) => inboundCovers(state, g, state.ownKeyId, L, session));
    }
    // [SEC-AUZ-012]: session bound to K, and an outbound grant lets L write to K or session.
    const K = state.bindings[session];
    if (!isStr(K)) return false;
    return state.grants.some((g) => g.direction === 'outbound' && ownSideIncludes(state, g.writer, L) && remoteSideNames(g.target, K, session));
  }
  // [SEC-AUZ-011]: release of an own session's presence record to device K.
  const K = requester.device;
  if (!isOwn(state, session)) return false;
  return state.grants.some((g) => (g.direction === 'inbound' && g.writer.key_id === K && ownSideIncludes(state, g.target, session))
    || (g.direction === 'outbound' && ownSideIncludes(state, g.writer, session) && g.target.key_id === K));
}

export function discoveryAuth(fx) {
  const c = toPlain(fx.context);
  const state = makeState(c);
  return { discoverable: mayDiscover(state, toPlain(fx.input.requester), fx.input.session, parseTimestamp(c.now)) };
}

// ---------------------------------------------------------------------------------------
// exchange (§9): two implementations, one sequence of operations.

export function exchange(fx) {
  const impls = {};
  for (const [label, ic] of Object.entries(toPlain(fx.context.implementations))) {
    const state = makeState(ic);
    for (const sid of Object.keys(state.sessions)) state.bindings[sid] = state.ownKeyId; // own sessions bound
    impls[label] = state;
  }
  const results = fx.input.steps.map((stepRaw) => {
    const step = toPlain(stepRaw);
    const state = impls[step.actor];
    if (!state) throw new Error(`unknown actor ${step.actor}`);
    const nowNs = parseTimestamp(step.at);
    switch (step.op) {
      case 'release': {
        const ok = mayDiscover(state, { device: step.to_device }, step.session, nowNs);
        if (ok) state.issued.add(`${step.session}|${step.to_device}`);
        return { released: ok };
      }
      case 'accept-presence':
        return { result: acceptPresence(state, stepRaw.authenticated_record, nowNs).result };
      case 'discover':
        return { discoverable: mayDiscover(state, { session_id: step.requester }, step.session, nowNs) };
      case 'remove-grant': {
        const idx = state.grants.findIndex((g) => deepEqual(g, step.grant));
        if (idx >= 0) state.grants.splice(idx, 1);
        return { removed: idx >= 0 };
      }
      case 'send': {
        const e = step.envelope;
        if (!mayDiscover(state, { session_id: e.from }, e.to, nowNs)) return { result: 'refused', error: 'unknown-destination' }; // [SC-DLV-075]
        const K = state.bindings[e.to];
        if (!state.issued.has(`${e.from}|${K}`)) throw new Error('send before the requester\'s announcement was issued: the exchange stage defines no outcome');
        state.sent.push({ id: e.id, from: e.from, to: e.to, to_key_id: K, created_at: e.created_at, nonce: e.security.nonce }); // [SEC-AUZ-013]
        return { result: 'sent' };
      }
      case 'receive': {
        const r = arrive(state, stepRaw.envelope, step.at, step.delivery, [0]);
        delete r.duplicate_receipt_allowed;
        return r;
      }
      default:
        throw new Error(`unknown op ${step.op}`);
    }
  });
  return { results };
}

// ---------------------------------------------------------------------------------------
// provenance (§12.2) and body (§12.3).

export function provenance(fx) {
  const f = toPlain(fx.input.fields);
  const ok = ['sender', 'device', 'session', 'message_id'].every((k) => isToken(f[k])) && (f.reply_to === '' || isToken(f.reply_to)); // [SEC-PRV-003]
  if (ok) return { result: 'rendered' };
  return { result: 'refused', state: 'failed', error: 'internal-error' }; // [SEC-PRV-014]
}

export function body(fx) {
  let s = fx.input.body;
  // [SEC-PRV-008]
  s = s.replace(/\r\n/g, '\n').replace(/[\r\u000b\u000c\u0085\u2028\u2029]/g, '\n');
  // [SEC-PRV-009]
  s = s.replace(/[\u0000-\u0008\u000e-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]/g, (ch) => `\\u{${ch.charCodeAt(0).toString(16).toUpperCase().padStart(4, '0')}}`);
  // [SEC-PRV-010]
  return { quoted_lines: s.split('\n').map((line) => (line === '' ? '|' : `| ${line}`)) };
}

export { canonicalize };
