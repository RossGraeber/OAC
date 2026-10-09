// HPKE [RFC 9180] in mode_base for the one suite spec/security.md §14 fixes:
// DHKEM(X25519, HKDF-SHA256) (0x0020), HKDF-SHA256 (0x0001), ChaCha20Poly1305 (0x0003),
// and the sealed frame of spec/security.md §14.4-§14.5 built on it.
//
// Node.js built-ins only. X25519, HMAC-SHA-256 and ChaCha20-Poly1305 come from node:crypto;
// the key schedule (LabeledExtract, LabeledExpand, ExtractAndExpand, KeySchedule) is written
// here from RFC 9180 §4, §4.1 and §5.1, and checked against its Appendix A.2.1 vector in
// self-test.mjs.

import { createHmac, createPrivateKey, createPublicKey, diffieHellman, createCipheriv, createDecipheriv } from 'node:crypto';

const KEM_ID = 0x0020;
const KDF_ID = 0x0001;
const AEAD_ID = 0x0003;
const N_SECRET = 32;
const N_ENC = 32;
const N_K = 32;
const N_N = 12;
const N_T = 16;

const i2osp = (n, w) => {
  const b = Buffer.alloc(w);
  for (let k = w - 1; k >= 0; k--) {
    b[k] = n & 0xff;
    n = Math.floor(n / 256);
  }
  return b;
};
const ascii = (s) => Buffer.from(s, 'ascii');
const SUITE_KEM = Buffer.concat([ascii('KEM'), i2osp(KEM_ID, 2)]);
const SUITE_HPKE = Buffer.concat([ascii('HPKE'), i2osp(KEM_ID, 2), i2osp(KDF_ID, 2), i2osp(AEAD_ID, 2)]);

// RFC 5869 HKDF-Extract and HKDF-Expand over SHA-256.
const extract = (salt, ikm) => createHmac('sha256', salt.length ? salt : Buffer.alloc(32)).update(ikm).digest();
function expand(prk, info, len) {
  const out = [];
  let t = Buffer.alloc(0);
  for (let i = 1; Buffer.concat(out).length < len; i++) {
    t = createHmac('sha256', prk).update(Buffer.concat([t, info, Buffer.from([i])])).digest();
    out.push(t);
  }
  return Buffer.concat(out).subarray(0, len);
}
// RFC 9180 §4.
const labeledExtract = (suite, salt, label, ikm) => extract(salt, Buffer.concat([ascii('HPKE-v1'), suite, ascii(label), ikm]));
const labeledExpand = (suite, prk, label, info, len) =>
  expand(prk, Buffer.concat([i2osp(len, 2), ascii('HPKE-v1'), suite, ascii(label), info]), len);

// X25519 [RFC 7748] through DER-wrapped raw keys. Returns null when the shared secret is
// all zero, which RFC 9180 §7.1.4 requires a DH to refuse (OpenSSL refuses it too).
const PKCS8 = Buffer.from('302e020100300506032b656e04220420', 'hex');
const SPKI = Buffer.from('302a300506032b656e032100', 'hex');
const privKey = (sk) => createPrivateKey({ key: Buffer.concat([PKCS8, sk]), format: 'der', type: 'pkcs8' });
const pubKey = (pk) => createPublicKey({ key: Buffer.concat([SPKI, pk]), format: 'der', type: 'spki' });
export function x25519(sk, pk) {
  let out;
  try {
    out = diffieHellman({ privateKey: privKey(sk), publicKey: pubKey(pk) });
  } catch {
    return null;
  }
  return out.every((b) => b === 0) ? null : out;
}
export const x25519Public = (sk) => Buffer.from(createPublicKey(privKey(sk)).export({ format: 'der', type: 'spki' }).subarray(SPKI.length));

// RFC 9180 §4.1, DHKEM.
function extractAndExpand(dh, kemContext) {
  const prk = labeledExtract(SUITE_KEM, Buffer.alloc(0), 'eae_prk', dh);
  return labeledExpand(SUITE_KEM, prk, 'shared_secret', kemContext, N_SECRET);
}
function encap(pkR, skE) {
  const dh = x25519(skE, pkR);
  if (!dh) return null;
  const enc = x25519Public(skE);
  return { shared: extractAndExpand(dh, Buffer.concat([enc, pkR])), enc };
}
function decap(enc, skR) {
  const dh = x25519(skR, enc);
  if (!dh) return null;
  return extractAndExpand(dh, Buffer.concat([enc, x25519Public(skR)]));
}

// RFC 9180 §5.1, mode_base (0x00), no PSK.
export function keySchedule(shared, info) {
  const pskIdHash = labeledExtract(SUITE_HPKE, Buffer.alloc(0), 'psk_id_hash', Buffer.alloc(0));
  const infoHash = labeledExtract(SUITE_HPKE, Buffer.alloc(0), 'info_hash', info);
  const ctx = Buffer.concat([Buffer.from([0x00]), pskIdHash, infoHash]);
  const secret = labeledExtract(SUITE_HPKE, shared, 'secret', Buffer.alloc(0));
  return {
    key: labeledExpand(SUITE_HPKE, secret, 'key', ctx, N_K),
    baseNonce: labeledExpand(SUITE_HPKE, secret, 'base_nonce', ctx, N_N),
  };
}

function aeadSeal(key, nonce, aad, pt) {
  const c = createCipheriv('chacha20-poly1305', key, nonce, { authTagLength: N_T });
  c.setAAD(aad, { plaintextLength: pt.length });
  return Buffer.concat([c.update(pt), c.final(), c.getAuthTag()]);
}
function aeadOpen(key, nonce, aad, ct) {
  if (ct.length < N_T) return null;
  try {
    const d = createDecipheriv('chacha20-poly1305', key, nonce, { authTagLength: N_T });
    d.setAAD(aad, { plaintextLength: ct.length - N_T });
    d.setAuthTag(ct.subarray(ct.length - N_T));
    return Buffer.concat([d.update(ct.subarray(0, ct.length - N_T)), d.final()]);
  } catch {
    return null;
  }
}

// Single-shot SealBase / OpenBase (RFC 9180 §6.1), sequence number 0. `skE` is the ephemeral
// private key: a fixture gives it so that the frame is reproducible; a sender draws it fresh.
export function sealBase(pkR, info, aad, pt, skE) {
  const e = encap(pkR, skE);
  if (!e) return null;
  const { key, baseNonce } = keySchedule(e.shared, info);
  return { enc: e.enc, ct: aeadSeal(key, baseNonce, aad, pt) };
}
export function openBase(enc, skR, info, aad, ct) {
  const shared = decap(enc, skR);
  if (!shared) return null;
  const { key, baseNonce } = keySchedule(shared, info);
  return aeadOpen(key, baseNonce, aad, ct);
}

// ---------------------------------------------------------------------------------------
// The sealed frame (spec/security.md §14.4, §14.5).

export const FRAME_VERSION = 0x01;
export const INFO = ascii('oac-seal-v1');
export const KINDS = { envelope: 0x01, presence: 0x02, receipt: 0x03 };
const KIND_NAMES = Object.fromEntries(Object.entries(KINDS).map(([k, v]) => [v, k]));
export const HEADER = 5; // kind octet and 4-octet length
export const OVERHEAD = 1 + N_ENC + HEADER + N_T; // 54

// Table 14.1: kind, length, payload, zero padding.
export function plaintext(kind, payload, padding) {
  return Buffer.concat([Buffer.from([KINDS[kind]]), i2osp(payload.length, 4), payload, Buffer.alloc(padding)]);
}

export function seal(pkR, kind, payload, padding, skE) {
  const s = sealBase(pkR, INFO, Buffer.alloc(0), plaintext(kind, payload, padding), skE);
  if (!s) return null;
  return Buffer.concat([Buffer.from([FRAME_VERSION]), s.enc, s.ct]);
}

// Opens a frame, or says why it cannot be opened ([SEC-SEL-030]).
export function open(frame, skR) {
  if (frame.length < OVERHEAD) return { why: 'short' };
  if (frame[0] !== FRAME_VERSION) return { why: 'version' };
  const pt = openBase(frame.subarray(1, 1 + N_ENC), skR, INFO, Buffer.alloc(0), frame.subarray(1 + N_ENC));
  if (!pt) return { why: 'open' };
  if (pt.length < HEADER) return { why: 'short-plaintext' };
  const kind = KIND_NAMES[pt[0]];
  if (!kind) return { why: 'kind' };
  const len = pt.readUInt32BE(1);
  if (len > pt.length - HEADER) return { why: 'length' };
  const pad = pt.subarray(HEADER + len);
  if (!pad.every((b) => b === 0)) return { why: 'padding' };
  return { kind, payload: pt.subarray(HEADER, HEADER + len) };
}

// An acceptable agreement key ([SEC-SEL-013]): 32 octets, most significant bit clear, a value
// less than 2^255 - 19, and not of small order on the curve or its twist. A clamped scalar is
// a multiple of 8, and both cofactors divide 8, so a small-order u gives an all-zero product.
const P = 2n ** 255n - 19n;
const PROBE = Buffer.alloc(32, 0x5a);
export function acceptableAgreementKey(u) {
  if (u.length !== 32) return false;
  if (u[31] & 0x80) return false;
  let v = 0n;
  for (let k = 31; k >= 0; k--) v = (v << 8n) | BigInt(u[k]);
  if (v >= P) return false;
  return x25519(PROBE, u) !== null;
}
