// Fixture stages of spec/security.md §14.10 (payload sealing), fixture format
// `oac-sealing-fixture/1`: `agreement` (§14.3), `seal` (§14.4) and `open` (§14.5).

import { isObj, isStr, toPlain } from './json.mjs';
import { signingInput } from './jcs.mjs';
import { verify } from './ed25519.mjs';
import { isToken, isKeyId, isSignatureForm, b64urlDecode, parseTimestamp } from './core.mjs';
import { seal as sealFrame, open as openFrame, acceptableAgreementKey, KINDS, OVERHEAD } from './hpke.mjs';

const has = (o, k) => Object.prototype.hasOwnProperty.call(o, k);
const b64url = (b) => Buffer.from(b).toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
// Unpadded base64url of exactly 32 octets, with the unused low-order bits zero.
const isKey32Form = (v) => isStr(v) && /^[A-Za-z0-9_-]{42}[AEIMQUYcgkosw048]$/.test(v);

export const AGREEMENT_DOMAIN = 'oac-agreement-v1';
const STATEMENT_MEMBERS = ['agreement_key', 'issued_at', 'security'];

// [SEC-SEL-012]: the trusted-key entry a statement names, or null.
function resolve(trusted, sec) {
  if (!isToken(sec.principal) || !isKeyId(sec.key_id)) return null;
  return trusted.find((k) => k.principal === sec.principal && k.key_id === sec.key_id) || null;
}

// Whether a consumer admits `st`, given its trusted keys and the statements it holds (§14.3).
export function admits(trusted, held, st) {
  if (!isObj(st) || Object.keys(st).length !== STATEMENT_MEMBERS.length || !STATEMENT_MEMBERS.every((m) => has(st, m))) return false; // [SEC-SEL-010]
  const sec = st.security;
  if (!isObj(sec) || Object.keys(sec).sort().join() !== 'key_id,principal,signature') return false; // [SEC-SEL-010]
  const entry = resolve(trusted, sec);
  if (!entry || !isSignatureForm(sec.signature)) return false; // [SEC-SEL-012]
  if (!verify(b64urlDecode(entry.public_key), signingInput(AGREEMENT_DOMAIN, st).bytes, b64urlDecode(sec.signature))) return false; // [SEC-SEL-012]
  if (!isKey32Form(st.agreement_key) || !acceptableAgreementKey(b64urlDecode(st.agreement_key))) return false; // [SEC-SEL-013]
  const t = parseTimestamp(st.issued_at);
  if (t === null) return false; // [SEC-SEL-010]
  const prior = held[sec.key_id];
  if (prior && t <= parseTimestamp(prior.issued_at)) return false; // [SEC-SEL-014]
  return true;
}

export function agreement(fx) {
  const c = toPlain(fx.context);
  return { result: admits(c.trusted_keys || [], c.admitted || {}, toPlain(fx.input).statement) ? 'admitted' : 'refused' };
}

export function seal(fx) {
  const c = toPlain(fx.context);
  const i = toPlain(fx.input);
  const trusted = (c.trusted_keys || []).some((k) => k.key_id === i.recipient_key_id);
  const held = (c.admitted || {})[i.recipient_key_id];
  // [SEC-SEL-023], [SEC-SEL-024]: only an admitted statement of a trusted key; otherwise the
  // payload is not passed at all, and an envelope is reported `transport-failure`.
  if (!trusted || !held) return { result: 'refused', error: 'transport-failure' };
  if (!has(KINDS, i.kind)) throw new Error(`unknown kind ${i.kind}`);
  const frame = sealFrame(b64urlDecode(held.agreement_key), i.kind, b64urlDecode(i.payload), i.padding, b64urlDecode(i.ephemeral_private_key)); // [SEC-SEL-020]
  if (!frame) throw new Error('the ephemeral key gives an all-zero shared secret');
  return { result: 'sealed', frame: b64url(frame) };
}

export function open(fx) {
  const c = toPlain(fx.context);
  const frame = b64urlDecode(toPlain(fx.input).frame);
  const r = openFrame(frame, b64urlDecode(c.own_agreement_private_key)); // [SEC-SEL-030]
  if (!r.kind) return { result: 'discarded', record: 'none' }; // [SEC-SEL-031]
  return { result: 'opened', kind: r.kind, payload: b64url(r.payload) }; // [SEC-SEL-032]
}

export { OVERHEAD };
