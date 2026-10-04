// Fixture stages of spec/session-channels.md: `envelope` (§3.3), `negotiation`, `binding`
// and `send` (§6.10), `presence` and `discovery` (§7.5), and `receipt`, `reply`,
// `correlation`, `combine`, `routing` and `receive` (§8.5).
//
// Each stage function takes the fixture as a parsed tree (json.mjs: numbers keep their
// spelling) and returns the outcome under the member names of the stage's `expected`.

import { isObj, toPlain, deepEqual } from './json.mjs';
import {
  envelopeStage, readEnvelopeInput, parseTimestamp, agree, senderChecks, estimateEnvelopeOctets,
  presenceRecordViolation, receiptViolation, effectiveState, expiryNs, msToNs,
  ERROR_STATES,
} from './core.mjs';
import { securityStage } from './stages-sec.mjs';

const has = (o, k) => Object.prototype.hasOwnProperty.call(o, k);

// ---------------------------------------------------------------------------------------
// envelope (§3.3): envelope-stage validation; the receiver supports `text` only (§8.5).
export function envelope(fx) {
  const c = fx.context;
  const out = envelopeStage(readEnvelopeInput(fx.input), {
    supportedMajors: toPlain(c.supported_major_versions),
    receiverTimeNs: parseTimestamp(c.receiver_time),
  });
  if (out.result === 'valid') out.trusted_security = toPlain(envelopeOf(fx.input).security); // [SC-ENV-080]
  return out;
}

function envelopeOf(input) {
  const r = readEnvelopeInput(input);
  return r.parsed;
}

// ---------------------------------------------------------------------------------------
// negotiation (§6.4, §6.5).
export function negotiation(fx) {
  const agreed = agree(fx.input.declaration, toPlain(fx.context.implemented));
  if (!agreed) return { result: 'no-common-version' };
  return { result: 'agreed', extension: agreed.extension, major: agreed.major };
}

// ---------------------------------------------------------------------------------------
// binding (§6.7.3): the ordered cases, applied to one native signal.
export function binding(fx) {
  const attachments = toPlain(fx.context.attachments).map((a) => ({ ...a }));
  const sig = toPlain(fx.input.signal);
  if (sig.pairing === 'window-expired') return { result: 'dropped', attachments, record: 'diagnostic' }; // [SC-ID-124], [SC-ID-128]
  if (sig.pairing === 'unpairable') return { result: 'failed-closed', attachments, record: 'finding' }; // [SC-ID-125], [SC-ID-129]
  if (sig.pairing !== 'paired') throw new Error(`unknown pairing ${sig.pairing}`);
  const att = attachments.find((a) => a.attachment === sig.attachment);
  if (!att) throw new Error(`signal names no attachment ${sig.attachment}`);
  const N = sig.native_id;
  const S = sig.start_kind === 'transition' ? 'transition' : 'fresh'; // [SC-ID-135]
  const boundToOther = att.binding && att.binding.native_id !== N;
  const records = [];
  const bind = () => {
    if (att.binding) delete att.binding; // [SC-ID-150]
    att.binding = { native_id: N, session_id: 'new' };
  };
  const stale = () => {
    if (boundToOther) {
      delete att.binding; // [SC-ID-152]
      records.push('finding'); // [SC-ID-153]
    }
  };
  let result;
  // Case 1, same N ([SC-ID-130]); exact comparison ([SC-ID-143]).
  if (att.binding && att.binding.native_id === N) {
    result = 'unchanged';
  } else if (attachments.some((a) => a !== att && a.binding && a.binding.native_id === N)) {
    // Case 2, duplicate N ([SC-ID-131] to [SC-ID-134]).
    result = 'refused';
    records.push('finding');
    stale();
  } else if (S === 'fresh') {
    const differs = (has(att, 'cross_check') && att.cross_check !== N) || (has(sig, 'cross_check') && sig.cross_check !== N);
    if (differs) {
      result = 'failed-closed'; // case 3(b), [SC-ID-137], [SC-ID-138]
      records.push('finding');
      stale();
    } else if (has(att, 'cross_check')) {
      result = 'bound'; // case 3(a), [SC-ID-136]
      bind();
    } else {
      result = 'bound'; // case 3(c), [SC-ID-139], [SC-ID-141]
      records.push('diagnostic');
      bind();
    }
  } else {
    result = 'bound'; // case 4, [SC-ID-140]
    const differs = (has(att, 'cross_check') && att.cross_check !== N) || (has(sig, 'cross_check') && sig.cross_check !== N);
    if (differs) records.push('diagnostic'); // [SC-ID-142]
    bind();
  }
  const record = records.includes('finding') ? 'finding' : records.includes('diagnostic') ? 'diagnostic' : 'none';
  return { result, attachments, record };
}

// ---------------------------------------------------------------------------------------
// send (§6.5, §6.6, §6.8, §8.3.3). A session with a declaration is `online`, any other
// `unknown` (§7.5).
export function send(fx) {
  const c = fx.context;
  const req = fx.input.request;
  const implemented = toPlain(c.implemented);
  const att = toPlain(c.attachments).find((a) => a.attachment === req.attachment);
  // Step 1: attribution ([SC-ID-160] to [SC-ID-162]).
  if (!att || !att.binding) return { result: 'refused', error: 'unauthorized' };
  const from = att.binding.session_id;
  // Step 2: presence ([SC-DLV-071], [SC-DLV-072]).
  if (!has(c.declarations, req.to)) return { result: 'refused', error: 'unknown-destination' };
  const declaration = c.declarations[req.to];
  const agreed = agree(declaration, implemented);
  const octets = estimateEnvelopeOctets(from, req.to, agreed ? agreed.revision : '0.0', req.content);
  const out = senderChecks(declaration, implemented, toPlain(req.content), octets);
  if (out.result === 'sent') return { result: 'sent', from, version: out.version };
  return out;
}

// ---------------------------------------------------------------------------------------
// presence (§7.2, §7.3.3).
export function presence(fx) {
  const c = fx.context;
  const implemented = toPlain(c.implemented);
  const own = c.own_sessions || {};
  const latest = new Map(); // session id -> { record, acceptedAt, issuer, carrierLost }
  const discarded = [];
  fx.input.events.forEach((ev, idx) => {
    if (has(ev, 'carrier_lost')) {
      for (const st of latest.values()) if (st.issuer === ev.carrier_lost && st.record.present === true) st.carrierLost = true; // [SC-DLV-046]
      return;
    }
    const rec = ev.record;
    if (presenceRecordViolation(rec)) return discarded.push(idx); // [SC-DLV-040]
    const prev = latest.get(rec.session_id);
    if (prev && BigInt(rec.seq.raw) <= BigInt(prev.record.seq.raw)) return discarded.push(idx); // [SC-DLV-042]
    latest.set(rec.session_id, { record: rec, acceptedAt: ev.at_ms.value, issuer: ev.issuer, carrierLost: false });
  });
  const q = fx.input.query_at_ms.value;
  const stateOf = (sid) => {
    if (has(own, sid)) return 'online';
    const st = latest.get(sid);
    if (!st) return 'unknown';
    if (st.record.present !== true || st.carrierLost) return 'unreachable';
    return q < st.acceptedAt + st.record.lifetime_ms.value ? 'online' : 'unreachable'; // [SC-DLV-044], [SC-DLV-045]
  };
  const out = { discarded };
  const expectedStates = fx.expected && fx.expected.states ? Object.keys(fx.expected.states) : [];
  out.states = Object.fromEntries(expectedStates.map((sid) => [sid, stateOf(sid)]));
  if (has(fx.input, 'send')) {
    const s = fx.input.send;
    const discoverable = c.discoverable ? toPlain(c.discoverable) : null;
    let seen = stateOf(s.to);
    if (discoverable && !discoverable.some((p) => p.requester === s.from && p.session === s.to)) seen = 'unknown'; // [SC-DLV-075]
    if (seen === 'unknown') out.send = { result: 'refused', error: 'unknown-destination' };
    else if (seen === 'unreachable') out.send = { result: 'refused', error: 'destination-unavailable' };
    else {
      // [SC-DLV-070]: the declaration of the latest accepted announcement, or the own one.
      const declaration = has(own, s.to) ? own[s.to].capabilities : latest.get(s.to).record.descriptor.capabilities;
      const agreed = agree(declaration, implemented);
      const octets = estimateEnvelopeOctets(s.from, s.to, agreed ? agreed.revision : '0.0', s.content);
      const r = senderChecks(declaration, implemented, toPlain(s.content), octets);
      out.send = r.result === 'sent' ? { result: 'sent', version: r.version } : r;
    }
  }
  return out;
}

// ---------------------------------------------------------------------------------------
// discovery (§7.3.2).
export function discovery(fx) {
  const c = toPlain(fx.context);
  const att = c.attachments.find((a) => a.attachment === fx.input.request.attachment);
  if (!att || !att.binding) return { result: 'refused', error: 'unauthorized' }; // [SC-DLV-060]
  const requester = att.binding.session_id;
  const sessions = Object.keys(c.sessions)
    .filter((sid) => c.sessions[sid].presence === 'online') // [SC-DLV-063]
    .filter((sid) => c.discoverable.some((p) => p.requester === requester && p.session === sid)) // [SC-DLV-061]
    .sort();
  return { result: 'listed', sessions };
}

// ---------------------------------------------------------------------------------------
// receipt (§8.1.4).
export function receipt(fx, env) {
  const r = fx.input.receipt;
  if (receiptViolation(r, env.table83)) return { result: 'discarded' }; // [SC-RCP-032]
  return { result: 'valid', effective_state: effectiveState(r, env.table83) };
}

// ---------------------------------------------------------------------------------------
// reply (§8.2.2).
export function reply(fx) {
  const handed = toPlain(fx.context.handed_off);
  const req = toPlain(fx.input.reply_request);
  if (!has(req, 'requested_target')) return { reply_headers: {}, correlation: 'uncorrelated' }; // [SC-RCP-052]
  // [SC-RCP-050], [SC-RCP-051]: a hand-off record to the replying session from the addressee.
  const rec = handed.find((h) => h.id === req.requested_target && h.to === req.from && h.from === req.to);
  if (!rec) return { reply_headers: {}, correlation: 'uncorrelated' };
  const headers = { reply_to: rec.id };
  if (has(rec, 'conversation_id')) headers.conversation_id = rec.conversation_id; // [SC-RCP-053]
  if (has(rec, 'correlation_id')) headers.correlation_id = rec.correlation_id; // [SC-RCP-054]
  return { reply_headers: headers, correlation: 'correlated' };
}

// ---------------------------------------------------------------------------------------
// correlation (§8.2.3).
export function correlation(fx) {
  const out = envelope(fx);
  delete out.trusted_security;
  const envl = envelopeOf(fx.input);
  if (out.result !== 'valid') return out;
  const sent = toPlain(fx.context.sent);
  // [SC-RCP-060]: a sent record with the reply's reply_to, from its `to` to its `from`.
  const rec = has(envl, 'reply_to') ? sent.find((s) => s.id === envl.reply_to && s.from === envl.to && s.to === envl.from) : null;
  if (!rec) return { ...out, correlation: 'unmatched' }; // [SC-RCP-061], [SC-RCP-062]
  return { ...out, correlation: 'matched', answers: { id: rec.id, from: rec.from } };
}

// ---------------------------------------------------------------------------------------
// combine (§8.4.1, §8.4.2).
export function combine(fx) {
  const c = toPlain(fx.context);
  const held = toPlain(fx.input.held);
  let state;
  if (held.some((h) => h.state === 'handed-to-harness')) state = 'handed-to-harness';
  else if (held.some((h) => h.state === 'duplicate')) state = 'duplicate';
  else if (held.some((h) => h.state === 'unknown' && h.observer === 'receiver')) state = 'unknown';
  else if (c.copies_passed === 1 && held.some((h) => ERROR_STATES.includes(h.state))) state = held.find((h) => ERROR_STATES.includes(h.state)).state;
  else if (c.copies_passed === 0 && held.some((h) => h.observer === 'sender' && ERROR_STATES.includes(h.state))) {
    state = held.find((h) => h.observer === 'sender' && ERROR_STATES.includes(h.state)).state;
  } else state = c.deadline_passed ? 'unknown' : 'accepted-by-adapter';
  let retry;
  if (c.copies_passed === 0) retry = ERROR_STATES.includes(state);
  else {
    retry = c.deadline_passed && state !== 'handed-to-harness' && state !== 'duplicate'
      && !held.some((h) => h.state === 'unknown' && h.observer === 'receiver');
  }
  return { state, retry_allowed: retry };
}

// ---------------------------------------------------------------------------------------
// The delivery stage of §8.3.2, after the envelope and security stages passed. `sessions`
// maps each known session id to { accepting, content_types, active_inbound? }.
export function deliveryStage(envl, sessions, handoffNs, replayWindowNs) {
  if (!has(sessions, envl.to)) return { result: 'unreachable', error: 'unknown-destination' }; // step 1
  const s = sessions[envl.to];
  if (s.accepting !== true) return { result: 'unreachable', error: 'destination-unavailable' }; // step 2
  if (has(s, 'active_inbound') && s.active_inbound === false) return { result: 'rejected', error: 'unsupported-capability' }; // step 3, [SC-ID-105]
  for (const part of envl.content) {
    if (!(part.type === 'text' || s.content_types.includes(part.type))) return { result: 'rejected', error: 'unsupported-capability' }; // [SC-RCP-077]
  }
  // Step 4: the hand-off deadline ([SC-RCP-091], [SC-RCP-092]).
  const windowEnd = parseTimestamp(envl.created_at) + replayWindowNs;
  const expiry = expiryNs(envl);
  const deadline = expiry !== null && expiry < windowEnd ? expiry : windowEnd;
  if (handoffNs >= deadline) {
    const boundIsExpiry = expiry !== null && expiry <= windowEnd;
    return { result: 'expired', error: boundIsExpiry ? 'expired' : 'outside-replay-window' };
  }
  return { result: 'valid' }; // step 5: the hand-off call succeeds
}

// routing (§8.5): envelope stage, authorization from `authorized`, then the delivery stage.
export function routing(fx) {
  const c = toPlain(fx.context);
  const input = readEnvelopeInput(fx.input);
  const out = envelopeStage(input, {
    supportedMajors: c.supported_major_versions,
    receiverTimeNs: parseTimestamp(c.receiver_time),
    supportedTypes: c.receiver_content_types,
  });
  if (out.result !== 'valid') return out;
  const envl = toPlain(input.parsed);
  if (!c.authorized.some((p) => p.from === envl.from && p.to === envl.to)) return { result: 'rejected', error: 'unauthorized' };
  const handoff = parseTimestamp(c.handoff_time || c.receiver_time);
  return deliveryStage(envl, c.sessions, handoff, msToNs(c.replay_window_ms));
}

// receive (§8.5): envelope stage, the security stage of spec/security.md §7, then the
// delivery stage, at one receiver.
export function receive(fx, env) {
  const c = toPlain(fx.context);
  const sec = securityStage(fx, env, { supportedTypes: c.receiver_content_types });
  if (sec.result !== 'passed') return { result: sec.result, error: sec.error };
  const envl = toPlain(readEnvelopeInput(fx.input).parsed);
  const handoff = parseTimestamp(c.handoff_time || c.receiver_time);
  return deliveryStage(envl, c.delivery, handoff, msToNs(300000));
}

export { deepEqual };
