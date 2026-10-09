// SPDX-License-Identifier: Apache-2.0

//! The private key-expression mapping (C7 §3): a `Destination` to a Zenoh key expression,
//! one way, inside this crate only.
//!
//! A destination's key expression is `oac/1/<partition>/<digest>`:
//!
//! - `<partition>` is 16 hex digits of SHA-256 over a domain tag and the configured
//!   partition label ([`crate::PeerConfiguration::partition`]). Peers in different
//!   partitions share no key expression.
//! - `<digest>` is 32 hex digits, the first 128 bits of SHA-256 over a domain tag that names
//!   the destination's kind (session or device) and the destination's id. A session id is
//!   128 bits of CSPRNG output ([SC-ID-003]), so its digest cannot be guessed without the id,
//!   and cannot be turned back into the id.
//!
//! The kind of payload is not in the key expression: it travels inside the frame
//! ([`crate::frame`]). Nothing here is ever written into an envelope, a receipt, a presence
//! record, a health detail or an error.

use oac_core::ids::KeyId;
use oac_core::transport::Destination;
use sha2::{Digest, Sha256};

/// The fixed first two chunks of every key expression this transport uses (C7 §3).
const ROOT: &str = "oac/1";

const PARTITION_TAG: &[u8] = b"oac transport partition v1\0";
const SESSION_TAG: &[u8] = b"oac transport session v1\0";
const DEVICE_TAG: &[u8] = b"oac transport device v1\0";
const LINK_TAG: &[u8] = b"oac transport link v1\0";

/// 128 bits naming a destination on the wire.
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

fn unhex(s: &str) -> Option<Digest128> {
    let b = s.as_bytes();
    if b.len() != 32 {
        return None;
    }
    let nib = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    let mut o = [0u8; 16];
    for (i, pair) in b.chunks(2).enumerate() {
        o[i] = (nib(pair[0])? << 4) | nib(pair[1])?;
    }
    Some(o)
}

/// The digest of a destination.
pub(crate) fn destination_digest(d: &Destination) -> Digest128 {
    match d {
        Destination::Session(s) => first16(digest(SESSION_TAG, s.as_str().as_bytes())),
        Destination::Device(k) => device_digest(k),
    }
}

/// The digest of a device's key id.
pub(crate) fn device_digest(k: &KeyId) -> Digest128 {
    first16(digest(DEVICE_TAG, k.as_str().as_bytes()))
}

/// The opaque octets of a carrier handle for a link, from the sending peer's own identifier.
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

    /// The key expression a payload for `digest` is put on.
    pub(crate) fn key(&self, digest: &Digest128) -> String {
        format!("{}/{}", self.prefix, hex(digest))
    }

    /// The one key expression every peer of the partition subscribes to: every destination
    /// of the partition, so that what a peer declares never depends on which destinations
    /// it has subscriptions for ([IFC-TRN-043]).
    pub(crate) fn all(&self) -> String {
        format!("{}/*", self.prefix)
    }

    /// The digest a key expression of this partition names, if it is one.
    pub(crate) fn digest_of(&self, key: &str) -> Option<Digest128> {
        let rest = key.strip_prefix(self.prefix.as_str())?.strip_prefix('/')?;
        unhex(rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oac_core::ids::SessionId;

    fn session(n: u8) -> Destination {
        Destination::Session(SessionId::from_random_octets([n; 16]))
    }

    fn key(c: char) -> KeyId {
        KeyId::parse(&c.to_string().repeat(64)).unwrap()
    }

    #[test]
    fn keys_are_fixed_width_hex_under_the_partition() {
        let p = Partition::new("default");
        let k = p.key(&destination_digest(&session(1)));
        let parts: Vec<_> = k.split('/').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(&parts[..2], ["oac", "1"]);
        assert_eq!(parts[2].len(), 16);
        assert_eq!(parts[3].len(), 32);
        assert!(k.bytes().all(|b| b == b'/' || b.is_ascii_alphanumeric()));
        assert_eq!(p.all(), format!("oac/1/{}/*", parts[2]));
    }

    #[test]
    fn the_mapping_is_one_way_and_hides_the_id() {
        let s = SessionId::from_random_octets([0xa5; 16]);
        let k =
            Partition::new("default").key(&destination_digest(&Destination::Session(s.clone())));
        assert!(!k.contains(s.as_str()));
        let dev = key('a');
        let kd =
            Partition::new("default").key(&destination_digest(&Destination::Device(dev.clone())));
        assert!(!kd.contains(&dev.as_str()[..16]));
    }

    #[test]
    fn digests_separate_kinds_destinations_and_partitions() {
        assert_ne!(
            destination_digest(&session(1)),
            destination_digest(&session(2))
        );
        assert_ne!(
            destination_digest(&Destination::Device(key('a'))),
            destination_digest(&Destination::Device(key('b')))
        );
        let a = Partition::new("a");
        let b = Partition::new("b");
        let d = destination_digest(&session(1));
        assert_ne!(a.key(&d), b.key(&d));
        assert_eq!(a.digest_of(&a.key(&d)), Some(d));
        assert_eq!(b.digest_of(&a.key(&d)), None);
        assert_eq!(a.digest_of("oac/1/x/y"), None);
    }

    #[test]
    fn link_octets_do_not_carry_the_identifier() {
        let id = "0123456789abcdef";
        let o = link_octets(id);
        assert_ne!(&o[..], id.as_bytes());
        assert_ne!(link_octets(id), link_octets("fedcba9876543210"));
    }
}
