// JSON Canonicalization Scheme (RFC 8785) over a parsed tree from json.mjs.
//
// spec/security.md §6.2: members sorted by their names as arrays of UTF-16 code units,
// strings and numbers serialized as ECMAScript does, and every number canonicalized as the
// IEEE 754 double nearest to its decimal value ([SEC-SIG-013]). JNum.value is exactly that
// double (ECMAScript Number parsing rounds to nearest, ties to even).

import { JNum } from './json.mjs';

export function canonicalize(v) {
  if (v === null) return 'null';
  if (v === true) return 'true';
  if (v === false) return 'false';
  if (v instanceof JNum) return serializeNumber(v.value);
  if (typeof v === 'number') return serializeNumber(v);
  if (typeof v === 'string') return JSON.stringify(v);
  if (Array.isArray(v)) return '[' + v.map(canonicalize).join(',') + ']';
  const keys = Object.keys(v).sort(); // default sort compares UTF-16 code units
  return '{' + keys.map((k) => JSON.stringify(k) + ':' + canonicalize(v[k])).join(',') + '}';
}

function serializeNumber(x) {
  if (!Number.isFinite(x)) throw new Error('JCS: number is not finite');
  if (Object.is(x, -0)) return '0';
  return String(x);
}

// The signing input of spec/security.md §6.2: domain string, one zero octet, then the JCS
// text of the object with `security.signature` removed, in UTF-8.
export function signingInput(domain, obj) {
  const copy = { ...obj };
  if (copy.security && typeof copy.security === 'object') {
    const sec = { ...copy.security };
    delete sec.signature;
    copy.security = sec;
  }
  const text = canonicalize(copy);
  return { text, bytes: Buffer.concat([Buffer.from(domain, 'ascii'), Buffer.from([0]), Buffer.from(text, 'utf8')]) };
}
