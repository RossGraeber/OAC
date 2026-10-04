// Ed25519 (RFC 8032 §5.1) with the strict verification rules of spec/security.md §6.3.
//
// Written from RFC 8032 with BigInt arithmetic so that the verdicts do not depend on a
// library's defaults (spec/security.md §6.3 records that one platform verifier accepts a
// small-order R). Only SHA-512 and SHA-256 come from node:crypto. Not constant-time: this
// is a conformance checker, never a signer of real messages.

import { createHash } from 'node:crypto';

const P = 2n ** 255n - 19n;
export const L = 2n ** 252n + 27742317777372353535851937790883648493n;
const mod = (a, m = P) => {
  const r = a % m;
  return r >= 0n ? r : r + m;
};
const pow = (b, e, m = P) => {
  let r = 1n;
  b = mod(b, m);
  while (e > 0n) {
    if (e & 1n) r = (r * b) % m;
    b = (b * b) % m;
    e >>= 1n;
  }
  return r;
};
const inv = (a) => pow(a, P - 2n);
const D = mod(-121665n * inv(121666n));
const SQRT_M1 = pow(2n, (P - 1n) / 4n);

// Extended coordinates (X, Y, Z, T), x = X/Z, y = Y/Z, xy = T/Z.
const IDENTITY = [0n, 1n, 1n, 0n];

function add([X1, Y1, Z1, T1], [X2, Y2, Z2, T2]) {
  const A = mod((Y1 - X1) * (Y2 - X2));
  const B = mod((Y1 + X1) * (Y2 + X2));
  const C = mod(2n * T1 * T2 * D);
  const Dd = mod(2n * Z1 * Z2);
  const E = B - A;
  const F = Dd - C;
  const G = Dd + C;
  const H = B + A;
  return [mod(E * F), mod(G * H), mod(F * G), mod(E * H)];
}

function mul(s, Pt) {
  let Q = IDENTITY;
  let R = Pt;
  while (s > 0n) {
    if (s & 1n) Q = add(Q, R);
    R = add(R, R);
    s >>= 1n;
  }
  return Q;
}

const neg = ([X, Y, Z, T]) => [mod(-X), Y, Z, mod(-T)];

function equal([X1, Y1, Z1], [X2, Y2, Z2]) {
  return mod(X1 * Z2 - X2 * Z1) === 0n && mod(Y1 * Z2 - Y2 * Z1) === 0n;
}

const isIdentity = (Pt) => equal(Pt, IDENTITY);

const leToBig = (bytes) => {
  let r = 0n;
  for (let k = bytes.length - 1; k >= 0; k--) r = (r << 8n) | BigInt(bytes[k]);
  return r;
};
const bigToLe = (x, len) => {
  const out = Buffer.alloc(len);
  for (let k = 0; k < len; k++) {
    out[k] = Number(x & 0xffn);
    x >>= 8n;
  }
  return out;
};

// RFC 8032 §5.1.3 decoding, rejecting every non-canonical encoding: y >= p, and x = 0 with
// the sign bit set. Returns null when the octets encode no point.
export function decodePoint(bytes) {
  if (bytes.length !== 32) return null;
  const sign = bytes[31] >> 7;
  const yb = Buffer.from(bytes);
  yb[31] &= 0x7f;
  const y = leToBig(yb);
  if (y >= P) return null;
  const u = mod(y * y - 1n);
  const v = mod(D * y * y + 1n);
  let x = mod(u * pow(v, 3n) * pow(u * pow(v, 7n), (P - 5n) / 8n));
  const vx2 = mod(v * x * x);
  if (vx2 === u) {
    // x is a root
  } else if (vx2 === mod(-u)) {
    x = mod(x * SQRT_M1);
  } else {
    return null;
  }
  if (x === 0n && sign === 1) return null;
  if (Number(x & 1n) !== sign) x = P - x;
  return [x, y, 1n, mod(x * y)];
}

export function encodePoint([X, Y, Z]) {
  const zi = inv(Z);
  const x = mod(X * zi);
  const y = mod(Y * zi);
  const out = bigToLe(y, 32);
  if (x & 1n) out[31] |= 0x80;
  return out;
}

// Small order: the point's order divides 8.
export const isSmallOrder = (Pt) => isIdentity(mul(8n, Pt));

const BY = mod(4n * inv(5n));
const B = decodePoint(bigToLe(BY, 32));

const sha512 = (...parts) => createHash('sha512').update(Buffer.concat(parts)).digest();

// A public key is acceptable when it is a canonical encoding of a point that does not have
// small order ([SEC-SIG-023], [SEC-KEY-034]).
export function acceptablePublicKey(pub) {
  const A = decodePoint(pub);
  return A !== null && !isSmallOrder(A);
}

// Strict verification of spec/security.md [SEC-SIG-020] to [SEC-SIG-024].
export function verify(pub, msg, sig) {
  if (pub.length !== 32 || sig.length !== 64) return false;
  const A = decodePoint(pub);
  if (A === null || isSmallOrder(A)) return false; // [SEC-SIG-023]
  const Rb = sig.subarray(0, 32);
  const R = decodePoint(Rb);
  if (R === null || isSmallOrder(R)) return false; // [SEC-SIG-022]
  const S = leToBig(sig.subarray(32));
  if (S >= L) return false; // [SEC-SIG-021]
  const k = mod(leToBig(sha512(Rb, pub, msg)), L); // [SEC-SIG-020]
  // [SEC-SIG-024]: cofactorless [S]B = R + [k]A
  return equal(mul(S, B), add(R, mul(k, A)));
}

// RFC 8032 §5.1.5 and §5.1.6, for regenerating fixture signatures from the test seeds.
export function publicKeyFromSeed(seed) {
  const h = sha512(seed);
  return encodePoint(mul(clamp(h), B));
}

export function sign(seed, msg) {
  const h = sha512(seed);
  const a = clamp(h);
  const pub = encodePoint(mul(a, B));
  const r = mod(leToBig(sha512(h.subarray(32), msg)), L);
  const Rb = encodePoint(mul(r, B));
  const k = mod(leToBig(sha512(Rb, pub, msg)), L);
  const S = mod(r + k * a, L);
  return Buffer.concat([Rb, bigToLe(S, 32)]);
}

function clamp(h) {
  const a = Buffer.from(h.subarray(0, 32));
  a[0] &= 248;
  a[31] &= 127;
  a[31] |= 64;
  return leToBig(a);
}

// Exposed for the runner's self-test only.
export const _internal = { mul, add, neg, B, IDENTITY, P };
