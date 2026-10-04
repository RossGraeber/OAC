// Strict JSON reader for the conformance runner.
//
// The fixtures need three things JSON.parse cannot give:
// - the exact spelling of every number (spec/session-channels.md [SC-ENV-050] and the other
//   "written as a JSON number with no sign, fraction or exponent" rules);
// - duplicate member names, which I-JSON forbids (RFC 7493 §2.3, [SC-ENV-002]);
// - lone surrogates kept as they are, so that a validator can see them ([SC-ENV-002]).
//
// parse() follows the RFC 8259 grammar exactly and throws JsonSyntaxError on anything else.
// Numbers become JNum objects; an object whose source repeated a member name carries the
// DUPLICATE symbol (the last value wins, as in JSON.parse). toPlain() turns a parsed tree
// into ordinary JavaScript values (each number as the IEEE 754 double nearest to it).

export class JsonSyntaxError extends Error {}

export class JNum {
  constructor(raw) {
    this.raw = raw;
    this.value = Number(raw);
  }
}

export const DUPLICATE = Symbol('duplicate-member');

const WS = new Set([0x20, 0x09, 0x0a, 0x0d]);

export function parse(text) {
  let i = 0;
  const n = text.length;
  const fail = (msg) => {
    throw new JsonSyntaxError(`${msg} at offset ${i}`);
  };
  const ws = () => {
    while (i < n && WS.has(text.charCodeAt(i))) i++;
  };
  const value = () => {
    ws();
    if (i >= n) fail('unexpected end');
    const c = text[i];
    if (c === '{') return object();
    if (c === '[') return array();
    if (c === '"') return string();
    if (c === 't') return literal('true', true);
    if (c === 'f') return literal('false', false);
    if (c === 'n') return literal('null', null);
    if (c === '-' || (c >= '0' && c <= '9')) return number();
    return fail(`unexpected character ${JSON.stringify(c)}`);
  };
  const literal = (word, v) => {
    if (text.startsWith(word, i)) {
      i += word.length;
      return v;
    }
    return fail('bad literal');
  };
  const number = () => {
    const m = /^-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?/.exec(text.slice(i));
    if (!m) fail('bad number');
    i += m[0].length;
    return new JNum(m[0]);
  };
  const string = () => {
    i++; // opening quote
    let out = '';
    for (;;) {
      if (i >= n) fail('unterminated string');
      const code = text.charCodeAt(i);
      if (code === 0x22) {
        i++;
        return out;
      }
      if (code < 0x20) fail('control character in string');
      if (code === 0x5c) {
        const e = text[i + 1];
        i += 2;
        if (e === '"') out += '"';
        else if (e === '\\') out += '\\';
        else if (e === '/') out += '/';
        else if (e === 'b') out += '\b';
        else if (e === 'f') out += '\f';
        else if (e === 'n') out += '\n';
        else if (e === 'r') out += '\r';
        else if (e === 't') out += '\t';
        else if (e === 'u') {
          const hex = text.slice(i, i + 4);
          if (!/^[0-9A-Fa-f]{4}$/.test(hex)) fail('bad \\u escape');
          out += String.fromCharCode(parseInt(hex, 16));
          i += 4;
        } else fail('bad escape');
        continue;
      }
      out += text[i];
      i++;
    }
  };
  const array = () => {
    i++;
    const arr = [];
    ws();
    if (text[i] === ']') {
      i++;
      return arr;
    }
    for (;;) {
      arr.push(value());
      ws();
      if (text[i] === ',') {
        i++;
        continue;
      }
      if (text[i] === ']') {
        i++;
        return arr;
      }
      fail('expected , or ]');
    }
  };
  const object = () => {
    i++;
    const obj = {};
    const seen = new Set();
    ws();
    if (text[i] === '}') {
      i++;
      return obj;
    }
    for (;;) {
      ws();
      if (text[i] !== '"') fail('expected member name');
      const key = string();
      ws();
      if (text[i] !== ':') fail('expected :');
      i++;
      const v = value();
      if (seen.has(key)) obj[DUPLICATE] = true;
      seen.add(key);
      Object.defineProperty(obj, key, { value: v, enumerable: true, writable: true, configurable: true });
      ws();
      if (text[i] === ',') {
        i++;
        continue;
      }
      if (text[i] === '}') {
        i++;
        return obj;
      }
      fail('expected , or }');
    }
  };
  const v = value();
  ws();
  if (i !== n) fail('trailing characters');
  return v;
}

export function toPlain(v) {
  if (v instanceof JNum) return v.value;
  if (Array.isArray(v)) return v.map(toPlain);
  if (v !== null && typeof v === 'object') {
    const o = {};
    for (const k of Object.keys(v)) {
      Object.defineProperty(o, k, { value: toPlain(v[k]), enumerable: true, writable: true, configurable: true });
    }
    return o;
  }
  return v;
}

// Decode octets as UTF-8, rejecting every ill-formed sequence (overlong forms, encoded
// surrogates, truncation). Returns null when the octets are not UTF-8.
export function decodeUtf8Strict(bytes) {
  try {
    return new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
  } catch {
    return null;
  }
}

// True when the string holds a lone surrogate or a Unicode noncharacter (RFC 7493 §2.1).
export function hasForbiddenCodePoint(s) {
  for (let k = 0; k < s.length; k++) {
    const c = s.charCodeAt(k);
    if (c >= 0xd800 && c <= 0xdbff) {
      const d = s.charCodeAt(k + 1);
      if (!(d >= 0xdc00 && d <= 0xdfff)) return true;
      const cp = ((c - 0xd800) << 10) + (d - 0xdc00) + 0x10000;
      if ((cp & 0xfffe) === 0xfffe) return true;
      k++;
      continue;
    }
    if (c >= 0xdc00 && c <= 0xdfff) return true;
    if (c >= 0xfdd0 && c <= 0xfdef) return true;
    if (c === 0xfffe || c === 0xffff) return true;
  }
  return false;
}

// True when the string holds a UTF-16 code unit that is half of no surrogate pair.
export function hasLoneSurrogate(s) {
  for (let k = 0; k < s.length; k++) {
    const c = s.charCodeAt(k);
    if (c >= 0xd800 && c <= 0xdbff) {
      const d = s.charCodeAt(k + 1);
      if (!(d >= 0xdc00 && d <= 0xdfff)) return true;
      k++;
    } else if (c >= 0xdc00 && c <= 0xdfff) return true;
  }
  return false;
}

// Walk a parsed tree and report the first I-JSON violation: a duplicate member name, or a
// member name or string value holding a lone surrogate or a noncharacter.
export function ijsonViolation(v) {
  if (typeof v === 'string') return hasForbiddenCodePoint(v) ? 'forbidden code point' : null;
  if (Array.isArray(v)) {
    for (const x of v) {
      const r = ijsonViolation(x);
      if (r) return r;
    }
    return null;
  }
  if (v !== null && typeof v === 'object' && !(v instanceof JNum)) {
    if (v[DUPLICATE]) return 'duplicate member name';
    for (const k of Object.keys(v)) {
      if (hasForbiddenCodePoint(k)) return 'forbidden code point in member name';
      const r = ijsonViolation(v[k]);
      if (r) return r;
    }
  }
  return null;
}

export const isObj = (v) => v !== null && typeof v === 'object' && !Array.isArray(v) && !(v instanceof JNum);
export const isStr = (v) => typeof v === 'string';
export const isBool = (v) => typeof v === 'boolean';

// An integer written as a JSON number with no sign, fraction or exponent, within [min, max].
export function isPlainInt(v, min, max) {
  if (!(v instanceof JNum)) return false;
  if (!/^(0|[1-9][0-9]*)$/.test(v.raw)) return false;
  const b = BigInt(v.raw);
  return b >= BigInt(min) && b <= BigInt(max);
}

// Any JSON null at any depth (member value or array element).
export function containsNull(v) {
  if (v === null) return true;
  if (Array.isArray(v)) return v.some(containsNull);
  if (isObj(v)) return Object.keys(v).some((k) => containsNull(v[k]));
  return false;
}

// Deep equality of plain JSON values (member order ignored).
export function deepEqual(a, b) {
  if (a instanceof JNum) a = a.value;
  if (b instanceof JNum) b = b.value;
  if (a === b) return true;
  if (typeof a !== typeof b || a === null || b === null) return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a)) return a.length === b.length && a.every((x, k) => deepEqual(x, b[k]));
  if (typeof a === 'object') {
    const ka = Object.keys(a);
    const kb = Object.keys(b);
    if (ka.length !== kb.length) return false;
    return ka.every((k) => Object.prototype.hasOwnProperty.call(b, k) && deepEqual(a[k], b[k]));
  }
  return false;
}
