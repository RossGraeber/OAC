// SPDX-License-Identifier: Apache-2.0

//! The frame this transport puts on a key expression: a fixed header, then the payload's
//! octets exactly as the core passed them ([IFC-TRN-030]).
//!
//! | Offset | Octets | Field |
//! |---|---|---|
//! | 0 | 1 | magic `0x4f` |
//! | 1 | 1 | frame version, `1` |
//! | 2 | 1 | payload kind: `1` envelope, `2` presence, `3` receipt |
//! | 3 | 8 | expiry, milliseconds since the Unix epoch, big-endian |
//! | 11 | 16 | the sending link's opaque handle octets |
//! | 27 | rest | the payload's octets |
//!
//! The expiry is the payload's deadline carried as wall-clock time, because a `Deadline` is
//! an instant on the sender's own monotonic clock and means nothing to another process. The
//! receiver drops a frame at or after its expiry ([IFC-TRN-034]). Peers on one host read
//! one wall clock; across hosts the check is as good as their clock agreement (binding
//! document, "Deadlines").

use oac_core::transport::PayloadKind;

use crate::addressing::Digest128;

const MAGIC: u8 = 0x4f;
const VERSION: u8 = 1;
/// The header length.
pub(crate) const HEADER: usize = 27;

/// A decoded frame.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Frame<'a> {
    pub(crate) kind: PayloadKind,
    pub(crate) expiry_unix_millis: u64,
    pub(crate) link: Digest128,
    pub(crate) octets: &'a [u8],
}

fn kind_octet(k: PayloadKind) -> u8 {
    match k {
        PayloadKind::Envelope => 1,
        PayloadKind::Presence => 2,
        PayloadKind::Receipt => 3,
        // Never encoded: this transport does not declare `sealing`, and `put` refuses a
        // sealed payload before it builds a frame. 0 is no kind: `decode` refuses it. The
        // sealed framing is G1's follow-up (#62).
        PayloadKind::Sealed => 0,
    }
}

/// Encode a frame.
pub(crate) fn encode(
    kind: PayloadKind,
    expiry_unix_millis: u64,
    link: &Digest128,
    octets: &[u8],
) -> Vec<u8> {
    let mut v = Vec::with_capacity(HEADER + octets.len());
    v.push(MAGIC);
    v.push(VERSION);
    v.push(kind_octet(kind));
    v.extend_from_slice(&expiry_unix_millis.to_be_bytes());
    v.extend_from_slice(link);
    v.extend_from_slice(octets);
    v
}

/// Decode a frame; `None` for anything that is not a version-1 frame.
pub(crate) fn decode(b: &[u8]) -> Option<Frame<'_>> {
    if b.len() < HEADER || b[0] != MAGIC || b[1] != VERSION {
        return None;
    }
    let kind = match b[2] {
        1 => PayloadKind::Envelope,
        2 => PayloadKind::Presence,
        3 => PayloadKind::Receipt,
        _ => return None,
    };
    let mut e = [0u8; 8];
    e.copy_from_slice(&b[3..11]);
    let mut link = [0u8; 16];
    link.copy_from_slice(&b[11..27]);
    Some(Frame {
        kind,
        expiry_unix_millis: u64::from_be_bytes(e),
        link,
        octets: &b[HEADER..],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_kind_and_every_octet() {
        let octets: Vec<u8> = (0..=255).collect();
        for k in [
            PayloadKind::Envelope,
            PayloadKind::Presence,
            PayloadKind::Receipt,
        ] {
            let f = encode(k, 1_700_000_000_123, &[9; 16], &octets);
            assert_eq!(
                decode(&f),
                Some(Frame {
                    kind: k,
                    expiry_unix_millis: 1_700_000_000_123,
                    link: [9; 16],
                    octets: &octets,
                })
            );
        }
        assert_eq!(
            decode(&encode(PayloadKind::Envelope, 0, &[0; 16], b""))
                .unwrap()
                .octets,
            b""
        );
    }

    #[test]
    fn refuses_what_is_not_a_frame() {
        let f = encode(PayloadKind::Receipt, 5, &[1; 16], b"x");
        assert!(decode(&f[..HEADER - 1]).is_none());
        let mut bad = f.clone();
        bad[0] = 0;
        assert!(decode(&bad).is_none());
        let mut bad = f.clone();
        bad[1] = 2;
        assert!(decode(&bad).is_none());
        let mut bad = f;
        bad[2] = 9;
        assert!(decode(&bad).is_none());
    }
}
