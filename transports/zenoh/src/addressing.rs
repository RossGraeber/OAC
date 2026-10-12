// SPDX-License-Identifier: Apache-2.0

//! One destination-independent frame key per configured partition.

use sha2::{Digest, Sha256};

/// The fixed first two chunks of every key expression this transport uses (C7 §3).
const ROOT: &str = "oac/1";

const PARTITION_TAG: &[u8] = b"oac transport partition v1\0";
const LINK_TAG: &[u8] = b"oac transport link v1\0";
const TOKEN_TAG: &[u8] = b"oac transport liveness v1\0";

/// The length in hex digits of a liveliness token's opaque name.
const TOKEN_HEX: usize = 32;

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

/// The opaque name of this transport's liveliness token: a hash of its random native session
/// id, so it names one transport start, never a device or a session ([IFC-TRN-108]), and
/// differs from the receiver-local stream handle.
pub(crate) fn token_name(native_peer_id: &str) -> String {
    hex(&digest(TOKEN_TAG, native_peer_id.as_bytes())[..TOKEN_HEX / 2])
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

    /// The liveliness token key of the transport named `token` (see [`token_name`]).
    pub(crate) fn token_key(&self, token: &str) -> String {
        format!("{}/live/{token}", self.prefix())
    }

    /// Every liveliness token key of this partition.
    pub(crate) fn token_pattern(&self) -> String {
        format!("{}/live/*", self.prefix())
    }

    /// The opaque name in a token key of this partition, if the key is one of this
    /// transport's tokens: exactly [`TOKEN_HEX`] lowercase hex digits after `live/`. Any
    /// other key is somebody else's and is ignored.
    pub(crate) fn token_of<'a>(&self, key: &'a str) -> Option<&'a str> {
        let name = key.strip_prefix(self.prefix())?.strip_prefix("/live/")?;
        (name.len() == TOKEN_HEX && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
            .then_some(name)
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

    #[test]
    fn token_keys_are_opaque_per_partition_names() {
        let p = Partition::new("a");
        let name = token_name("0123456789abcdef0123456789abcdef");
        assert_eq!(name.len(), 32);
        assert_ne!(name, token_name("0123456789abcdef0123456789abcdee"));
        let key = p.token_key(&name);
        assert!(key.starts_with(&format!("{}/live/", p.prefix())));
        assert_eq!(p.token_of(&key), Some(name.as_str()));
        // Another partition's key, a wrong length, upper case and extra chunks are not ours.
        assert_eq!(Partition::new("b").token_of(&key), None);
        for bad in ["", "abc", &name.to_uppercase(), &format!("{name}/x")] {
            assert_eq!(p.token_of(&format!("{}/live/{bad}", p.prefix())), None);
        }
        assert_eq!(p.token_of(&p.key()), None);
    }
}
