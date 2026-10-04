// Known-answer tests of the runner's own building blocks, run by `run.mjs --self-test`.
// They check the runner against published vectors, independently of the fixtures.

import { parse, DUPLICATE, JNum, isPlainInt, ijsonViolation } from './json.mjs';
import { canonicalize } from './jcs.mjs';
import { verify, sign, publicKeyFromSeed, decodePoint, isSmallOrder, acceptablePublicKey } from './ed25519.mjs';
import { parseTimestamp, isSessionId, isToken } from './core.mjs';

export function selfTest() {
  const failures = [];
  const expect = (name, cond) => {
    if (!cond) failures.push(name);
  };
  const hex = (s) => Buffer.from(s, 'hex');

  // RFC 8032 §7.1, TEST 1 and TEST 2.
  const vectors = [
    ['9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60', 'd75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a', '',
      'e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b'],
    ['4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb', '3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c', '72',
      '92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00'],
  ];
  for (const [seed, pub, msg, sig] of vectors) {
    expect(`RFC 8032 public key ${pub.slice(0, 8)}`, publicKeyFromSeed(hex(seed)).equals(hex(pub)));
    expect(`RFC 8032 signature ${pub.slice(0, 8)}`, sign(hex(seed), hex(msg)).equals(hex(sig)));
    expect(`RFC 8032 verify ${pub.slice(0, 8)}`, verify(hex(pub), hex(msg), hex(sig)));
    const bad = hex(sig);
    bad[63] ^= 0x10;
    expect(`RFC 8032 tampered ${pub.slice(0, 8)}`, !verify(hex(pub), hex(msg), bad));
  }
  // Strictness: the identity point is small order; y = p is not a canonical encoding; a
  // zero x with the sign bit set is not canonical.
  const identity = Buffer.alloc(32);
  identity[0] = 1;
  expect('identity is small order', isSmallOrder(decodePoint(identity)));
  expect('identity is not an acceptable public key', !acceptablePublicKey(identity));
  const yp = Buffer.from('edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f', 'hex');
  expect('y = p rejected', decodePoint(yp) === null);
  const negZero = Buffer.from(identity);
  negZero[31] |= 0x80;
  expect('x = 0 with sign bit rejected', decodePoint(negZero) === null);
  // S = L must be rejected even when S - L would verify ([SEC-SIG-021]).
  const [seed1, pub1, , sig1] = vectors[0];
  const L = 2n ** 252n + 27742317777372353535851937790883648493n;
  const s = hex(sig1).subarray(32);
  let S = 0n;
  for (let k = 31; k >= 0; k--) S = (S << 8n) | BigInt(s[k]);
  const S2 = S + L;
  const s2 = Buffer.alloc(32);
  let x = S2;
  for (let k = 0; k < 32; k++) {
    s2[k] = Number(x & 0xffn);
    x >>= 8n;
  }
  expect('S + L rejected', !verify(hex(pub1), Buffer.alloc(0), Buffer.concat([hex(sig1).subarray(0, 32), s2])));
  expect('seed check', seed1.length === 64);

  // RFC 8785 §3.2.2.3 number serialization and §3.2.3 member ordering.
  const numbers = [['1E30', '1e+30'], ['4.50', '4.5'], ['0.002', '0.002'], ['1E-7', '1e-7'], ['-0', '0'],
    ['9007199254740993', '9007199254740992'], ['333333333.33333329', '333333333.3333333'], ['1e+21', '1e+21'], ['1e20', '100000000000000000000']];
  for (const [inp, out] of numbers) expect(`JCS number ${inp}`, canonicalize(parse(inp)) === out);
  const ordered = canonicalize(parse('{"\\u20ac":1,"\\r":2,"\\ufb33":3,"1":4,"\\ud83d\\ude00":5,"\\u0080":6,"\\u00f6":7}'));
  expect('JCS UTF-16 member order', ordered === '{"\\r":2,"1":4,"\u0080":6,"\u00f6":7,"\u20ac":1,"\ud83d\ude00":5,"\ufb33":3}');
  expect('JCS escapes', canonicalize(parse('"\\u000f\\u2028\\u007f\\"\\\\"')) === '"\\u000f\u2028\u007f\\"\\\\"');

  // The strict JSON reader.
  const dup = parse('{"a":1,"a":2}');
  expect('duplicate member detected', dup[DUPLICATE] === true && ijsonViolation(dup) !== null);
  expect('number spelling kept', parse('300000.0') instanceof JNum && parse('300000.0').raw === '300000.0');
  expect('fraction is not a plain integer', !isPlainInt(parse('300000.0'), 1, 86400000));
  expect('exponent is not a plain integer', !isPlainInt(parse('3e5'), 1, 86400000));
  expect('lone surrogate kept and flagged', ijsonViolation(parse('"\\ud800"')) !== null);
  expect('noncharacter flagged', ijsonViolation(parse('"\\uffff"')) !== null);
  for (const bad of ['{"a":1,}', '[1,]', '01', '"\t"', "{'a':1}", 'NaN', '1.', '.5', '{"a" 1}']) {
    let threw = false;
    try {
      parse(bad);
    } catch {
      threw = true;
    }
    expect(`rejects ${JSON.stringify(bad)}`, threw);
  }

  // Timestamps and identifiers.
  expect('timestamp parse', parseTimestamp('1970-01-01T00:00:01.5Z') === 1500000000n);
  expect('leap day', parseTimestamp('2024-02-29T00:00:00Z') !== null && parseTimestamp('2025-02-29T00:00:00Z') === null);
  expect('leap second rejected', parseTimestamp('2026-10-03T23:59:60Z') === null);
  expect('numeric offset rejected', parseTimestamp('2026-10-03T12:00:00+00:00') === null);
  expect('nine fraction digits', parseTimestamp('1970-01-01T00:00:00.000000001Z') === 1n);
  expect('session id', isSessionId('7gq3m8z2c5k9t1w4x6b0n2r8vd') && !isSessionId('8gq3m8z2c5k9t1w4x6b0n2r8vd') && !isSessionId('7gq3m8z2c5k9t1w4x6b0n2r8vu'));
  expect('token rejects trailing LF', !isToken('msg-1\n'));
  return failures;
}
