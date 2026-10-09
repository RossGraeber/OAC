// SPDX-License-Identifier: Apache-2.0

//! Unpadded base64url ([RFC4648] §5), the encoding of public keys, nonces and signatures in
//! `spec/security.md` (§3.2, [SEC-SIG-003], [SEC-SIG-004]).
//!
//! Decoding is strict: no padding, no character outside the base64url alphabet, and the
//! unused low-order bits of the last character must be zero, so every octet string has
//! exactly one accepted encoding (`spec/security.md` §6.1).

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// The unpadded base64url encoding of `octets`.
pub(crate) fn encode(octets: &[u8]) -> String {
    let mut out = String::with_capacity(octets.len().div_ceil(3) * 4);
    for chunk in octets.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |a, (k, &b)| a | (u32::from(b) << (16 - 8 * k)));
        for k in 0..=chunk.len() {
            out.push(ALPHABET[((n >> (18 - 6 * k)) & 63) as usize] as char);
        }
    }
    out
}

fn value(c: u8) -> Option<u32> {
    Some(u32::from(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'-' => 62,
        b'_' => 63,
        _ => return None,
    }))
}

/// The `N` octets that `s` encodes, if `s` is their one canonical unpadded base64url
/// encoding.
pub(crate) fn decode_exact<const N: usize>(s: &str) -> Option<[u8; N]> {
    let s = s.as_bytes();
    if s.len() != (N * 4).div_ceil(3) {
        return None;
    }
    let mut out = [0u8; N];
    let mut written = 0;
    for chunk in s.chunks(4) {
        let mut n = 0u32;
        for (k, &c) in chunk.iter().enumerate() {
            n |= value(c)? << (18 - 6 * k);
        }
        let octets = chunk.len() - 1;
        // The bits of the last character below the last whole octet must be zero.
        let unused = 24 - 8 * octets;
        if unused < 24 && n & ((1 << unused) - 1) != 0 {
            return None;
        }
        for k in 0..octets {
            out[written] = (n >> (16 - 8 * k)) as u8;
            written += 1;
        }
    }
    (written == N).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_refuses_other_forms() {
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(encode(b"fo"), "Zm8");
        assert_eq!(decode_exact::<2>("Zm8"), Some(*b"fo"));
        assert_eq!(decode_exact::<6>("Zm9vYmFy"), Some(*b"foobar"));
        // Non-zero unused bits: "Zm9" also decodes to "fo" under a lax decoder.
        assert_eq!(decode_exact::<2>("Zm9"), None);
        // Padding, the standard alphabet, and a wrong length are refused.
        assert_eq!(decode_exact::<2>("Zm8="), None);
        assert_eq!(decode_exact::<3>("+/+/"), None);
        assert_eq!(decode_exact::<3>("-_-_"), Some([0xfb, 0xff, 0xbf]));
        assert_eq!(decode_exact::<16>(&encode(&[0xff; 15])), None);
        let sixteen = [0xa5; 16];
        assert_eq!(decode_exact::<16>(&encode(&sixteen)), Some(sixteen));
        let sixty_four = [0x5a; 64];
        assert_eq!(decode_exact::<64>(&encode(&sixty_four)), Some(sixty_four));
    }
}
