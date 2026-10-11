// SPDX-License-Identifier: Apache-2.0

//! One destination-independent frame key per configured partition.

use sha2::{Digest, Sha256};

/// The fixed first two chunks of every key expression this transport uses (C7 §3).
const ROOT: &str = "oac/1";

const PARTITION_TAG: &[u8] = b"oac transport partition v1\0";
const LINK_TAG: &[u8] = b"oac transport link v1\0";

/// Receiver-local opaque stream-handle octets; never transmitted.
pub(crate) type Digest128 = [u8; 16];

fn digest(tag: &[u8], input: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(tag);
    h.update(input);
    h.finalize().into()
}

fn first16(d: [u8; 32]) -> Digest128 {
    let mut o = [0u8; 16];
    o.copy_from_slice(&d[..16]);
    o
}

fn hex(octets: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(octets.len() * 2);
    for b in octets {
        s.push(H[usize::from(b >> 4)] as char);
        s.push(H[usize::from(b & 15)] as char);
    }
    s
}

/// The opaque octets of a carrier handle for a link, from the receiving transport's own random native identifier.
/// The identifier is hashed, so a handle never carries it.
pub(crate) fn link_octets(native_peer_id: &str) -> Digest128 {
    first16(digest(LINK_TAG, native_peer_id.as_bytes()))
}

/// The key expressions of one partition.
#[derive(Clone, Debug)]
pub(crate) struct Partition {
    prefix: String,
}

impl Partition {
    pub(crate) fn new(label: &str) -> Partition {
        let p = digest(PARTITION_TAG, label.as_bytes());
        Partition {
            prefix: format!("{ROOT}/{}", hex(&p[..8])),
        }
    }

    /// The bare prefix `oac/1/<partition>`.
    pub(crate) fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Every sample in this partition has this same key, for publish and subscribe.
    pub(crate) fn key(&self) -> String {
        format!("{}/frames", self.prefix())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_frame_keys_separate_partitions() {
        let a = Partition::new("a");
        assert_eq!(a.key(), format!("{}/frames", a.prefix()));
        assert_ne!(a.key(), Partition::new("b").key());
    }
}
