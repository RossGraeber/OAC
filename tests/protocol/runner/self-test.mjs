// Known-answer tests of the runner's own building blocks, run by `run.mjs --self-test`.
// They check the runner against published vectors, independently of the fixtures.

import { parse, DUPLICATE, JNum, isPlainInt, ijsonViolation } from './json.mjs';
import { canonicalize } from './jcs.mjs';
import { createHash } from 'node:crypto';
import { verify, sign, publicKeyFromSeed, decodePoint, encodePoint, isSmallOrder, acceptablePublicKey, _internal } from './ed25519.mjs';

const leBig = (b) => {
  let r = 0n;
  for (let k = b.length - 1; k >= 0; k--) r = (r << 8n) | BigInt(b[k]);
  return r;
};
const leBytes = (x) => {
  const out = Buffer.alloc(32);
  for (let k = 0; k < 32; k++) {
    out[k] = Number(x & 0xffn);
    x >>= 8n;
  }
  return out;
};
const L = 2n ** 252n + 27742317777372353535851937790883648493n;
import { parseTimestamp, isSessionId, isToken } from './core.mjs';
import { checkOwners } from './index-check.mjs';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

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
  // Small-order points other than the identity: order 2 (y = p - 1), order 4 (y = 0) and
  // order 8 (the two encodings published in the Ed25519 small-order lists, e.g. libsodium).
  const torsion = [
    'ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f',
    '0000000000000000000000000000000000000000000000000000000000000000',
    '0000000000000000000000000000000000000000000000000000000000000080',
    '26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05',
    'c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a',
  ];
  for (const t of torsion) {
    const pt = decodePoint(hex(t));
    expect(`torsion ${t.slice(0, 8)} decodes`, pt !== null);
    expect(`torsion ${t.slice(0, 8)} is small order`, pt !== null && isSmallOrder(pt));
    expect(`torsion ${t.slice(0, 8)} is not the identity`, pt !== null && !hex(t).equals(identity));
    expect(`torsion ${t.slice(0, 8)} refused as a public key`, !acceptablePublicKey(hex(t)));
  }
  // Mixed order: A' = A + T8 is not small order, so it decodes as an acceptable key, but a
  // signature that only the cofactored equation accepts is rejected ([SEC-SIG-024]).
  {
    const { mul, add, B } = _internal;
    const T8 = decodePoint(hex(torsion[3]));
    const seed = hex(vectors[0][0]);
    const h = createHash('sha512').update(seed).digest();
    const a0 = Buffer.from(h.subarray(0, 32));
    a0[0] &= 248;
    a0[31] &= 127;
    a0[31] |= 64;
    const a = leBig(a0);
    const Amixed = encodePoint(add(mul(a, B), T8));
    expect('mixed-order key is not small order', acceptablePublicKey(Amixed));
    let tried = 0;
    for (let n = 0; n < 64; n++) {
      const msg = Buffer.from(`mixed-${n}`);
      const r = leBig(createHash('sha512').update(Buffer.concat([h.subarray(32), msg])).digest()) % L;
      const Rb = encodePoint(mul(r, B));
      const k = leBig(createHash('sha512').update(Buffer.concat([Rb, Amixed, msg])).digest()) % L;
      if (k % 8n === 0n) continue; // [k]T8 vanishes, so both equations would agree
      const S = (r + k * a) % L;
      // [8][S]B = [8]R + [8][k]A' holds; [S]B = R + [k]A' does not.
      expect('mixed-order key: cofactored-only signature rejected', !verify(Amixed, msg, Buffer.concat([Rb, leBytes(S)])));
      tried++;
      if (tried === 3) break;
    }
    expect('mixed-order key: cases tried', tried === 3);
  }

  // S = L must be rejected even when S - L would verify ([SEC-SIG-021]).
  const [seed1, pub1, , sig1] = vectors[0];
  const s =hex(sig1).subarray(32);
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

  // The owner index of spec/interfaces.md Appendix C (checkOwners): the committed text is clean,
  // and each planted edit to it fails.
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
  const readRepo = (rel) => fs.readFileSync(path.join(root, rel), 'utf8').replace(/\r\n/g, '\n');
  const iface = readRepo('spec/interfaces.md');
  const rowOf = (area, owner) => iface.split('\n').find((l) => l.startsWith(`| ${area} | ${owner} | `));
  const withInterfaces = (text) => (rel) => (rel === 'spec/interfaces.md' ? text : readRepo(rel));
  expect('owner index: committed text is clean', checkOwners(readRepo).length === 0);
  const rct = rowOf('SEC-RCT', 'core'); // | SEC-RCT | core | 001, 002, 003, 004, 005 |
  const env = rowOf('SC-ENV', 'adapter');
  const planted = {
    'removed id': iface.replace(rct, rct.replace(', 005 |', ' |')),
    'id with two owners across rows': iface.replace(env, `${env}\n| SC-ENV | core | 064 |`),
    'id twice within one row': iface.replace(rct, rct.replace('001, 002', '001, 001, 002')),
    'owner harness': iface.replace(rct, rct.replace('| core |', '| harness |')),
    'SHOULD id listed': iface.replace(rct, `${rct}\n| SEC-RPL | core | 031 |`),
    'nonexistent id': iface.replace(rct, rct.replace(', 005 |', ', 005, 999 |')),
    'capitalised owner': iface.replace(rct, rct.replace('| core |', '| Core |')),
    'backticked area': iface.replace(rct, rct.replace('| SEC-RCT |', '| `SEC-RCT` |')),
    'row under a C.1 subheading': iface.replace(rct, `${rct}\n\n### C.1 More rows\n\n| SEC-RCT | adapter | 001 |`),
    'row without a leading pipe': iface.replace(rct, `${rct}\nSEC-STG | adapter | 001 |`),
    'row without outer pipes': iface.replace(rct, `${rct}\nSEC-STG | adapter | 001`),
    'row with two leading spaces': iface.replace(rct, `${rct}\n  | SEC-STG | adapter | 001 |`),
  };
  // Code fences follow CommonMark §4.5 (#283) and HTML comments §4.6 (#291). These cases append
  // after Appendix C's last row, which is inside the appendix only while no top-level `## `
  // heading follows it; appended() asserts that. A malformed row after a line that is not a
  // fence opener must be checked; one inside a real fence, up to a line that is a valid closer,
  // must not.
  const BAD = '| SEC-STG | adapter | 001 |';
  const ownerHeading = iface.indexOf('\n## Appendix C. Owner index\n');
  const appendixCLast = ownerHeading >= 0 && !/^## /m.test(iface.slice(ownerHeading + 2));
  const appended = (block) => {
    if (!appendixCLast) throw new Error('self-test: appended() needs Appendix C to be the last ## section of spec/interfaces.md');
    return `${iface.replace(/\n+$/, '')}\n\n${block}\n`;
  };
  // Not fences: a backtick in a backtick fence's info string makes the line a paragraph, so the
  // row after it is rendered and must fail; the trailing ``` then opens a fence to the end.
  planted['row after ```a`b (not a fence)'] = appended(`\`\`\`a\`b\n\n${BAD}\n\n\`\`\``);
  planted['row after ```js `x` (not a fence)'] = appended(`\`\`\`js \`x\`\n\n${BAD}\n\n\`\`\``);
  // #291: a `## ` line inside a fence or an HTML comment is not a heading, so it does not end the
  // appendix, and the row after the block must still be checked.
  planted['row after a fence holding a ## line'] = appended(`\`\`\`\n## x\n\`\`\`\n\n${BAD}`);
  planted['row after a comment holding a ## line'] = appended(`<!--\n## x\n-->\n\n${BAD}`);
  // #291: a fence marker inside an HTML comment opens no fence, so the row after the comment must
  // still be checked.
  planted['row after a comment holding ```'] = appended(`<!--\n\`\`\`\n-->\n\n${BAD}`);
  planted['row after a comment holding ~~~ on its last line'] = appended(`<!-- x\n~~~ -->\n\n${BAD}`);
  planted['row after a one-line comment'] = appended(`<!-- \`\`\` -->\n${BAD}`);
  planted['row after a fence holding <!--'] = appended(`\`\`\`\n<!--\n\`\`\`\n\n${BAD}`);
  for (const [name, text] of Object.entries(planted)) {
    expect(`owner index: planted ${name} is applied`, text !== iface);
    expect(`owner index: planted ${name} fails`, checkOwners(withInterfaces(text)).length > 0);
  }
  const fenced = {
    'backtick fence with an info string': `\`\`\`text\n${BAD}\n\`\`\``,
    'tilde fence with a backtick in its info string': `~~~a\`b\n${BAD}\n~~~`,
    'longer closing fence': `\`\`\`\n${BAD}\n\`\`\`\`\``,
    'indented fence and closer': `   \`\`\`\n${BAD}\n  \`\`\``,
    '```js inside a fence is not a closer': `\`\`\`\n${BAD}\n\`\`\`js\n${BAD}\n\`\`\``,
    'shorter run inside a fence is not a closer': `\`\`\`\`\n${BAD}\n\`\`\`\n${BAD}\n\`\`\`\``,
    'other fence character is not a closer': `\`\`\`\n${BAD}\n~~~\n${BAD}\n\`\`\``,
    'row inside a comment': `<!--\n${BAD}\n-->`,
    'row inside a comment after ```': `<!--\n\`\`\`\n${BAD}\n-->`,
    'row on a comment\'s closing line': `<!--\n${BAD} -->`,
  };
  // Control: the row with no fence around it fails, so a clean result below is the fence's doing.
  expect('owner index: appended unfenced row fails', checkOwners(withInterfaces(appended(BAD))).length > 0);
  for (const [name, block] of Object.entries(fenced)) {
    expect(`owner index: ${name} stays clean`, checkOwners(withInterfaces(appended(block))).length === 0);
  }
  return failures;
}
