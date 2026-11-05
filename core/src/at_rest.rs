// SPDX-License-Identifier: Apache-2.0

//! The device's agreement keys at rest (#437 S1b; `docs/planning/decisions/G-437-pairing.md`
//! §7): the bytes of the file the `oac` binary keeps next to the device key, sealed so that a
//! copy of the file alone is useless and a file the device key's holder did not write fails to
//! open.
//!
//! # The construction
//!
//! HPKE (RFC 9180) **PSK mode**, single-shot, through the `hpke` crate 0.12.0 that
//! [`crate::sealing`] already uses: DHKEM(X25519, HKDF-SHA256), HKDF-SHA256,
//! ChaCha20Poly1305. Two values are derived from the device key's seed, each under its own
//! domain-separation label, and neither is the seed:
//!
//! - the storage key pair: `DeriveKeyPair` (RFC 9180 §7.1.3) over `"oac-agreement-store-kem-v1"
//!   0x00 || seed`;
//! - the PSK: SHA-256 over `"oac-agreement-store-psk-v1" 0x00 || seed`, 32 octets.
//!
//! `info` and `psk_id` are the ASCII octets `oac-agreement-store-v1`. [`crate::sealing::SEAL_INFO`]
//! (`oac-seal-v1`) is never used here, so a payload frame is never a valid stored file and a
//! stored file is never a valid payload frame.
//!
//! **Why not base mode.** Base mode "is the only mode that does not provide any kind of sender
//! identity authentication" (`hpke` 0.12.0, `src/op_mode.rs`): with it, anyone who knows the
//! storage *public* key could write a file that opens, holding agreement private keys that
//! writer chose. In PSK mode the writer must hold the PSK, which only a holder of the seed can
//! compute. This adds no protection against rollback to an older valid file.
//!
//! # The file
//!
//! `header || enc || ciphertext || tag`. The header is `OACR`, the version octet `1`, the kind
//! octet `1` (agreement keys) and the 64 lower-case hexadecimal digits of the device key id
//! (70 octets). **The header is the associated data**: a file renamed to another key id, or of
//! another kind or version, fails to open. `enc` is the 32-octet encapsulated key. Every write
//! draws a fresh ephemeral key from the operating system's CSPRNG, so no two writes share an
//! `enc` or a content key.
//!
//! # Where the seed goes
//!
//! Nowhere. [`crate::keys::DeviceIdentity::at_rest`] derives the two values inside this
//! crate and returns an [`AtRest`], which holds them (zeroized on drop) and offers no accessor.
//! The handle **fixes its own device key id** at construction: no method takes a key id, so a
//! caller cannot ask it to seal or open for another one. The state is serialized here too, so
//! the `oac` binary handles ciphertext only and never builds a plaintext private key
//! ([SEC-SEL-004]).
//!
//! # The plaintext
//!
//! Binary, not JSON, so that no private key is ever spelled as a `String` that is not
//! zeroized: a flags octet (`1` when a highest `seq` is recorded, else `0`), the highest `seq`
//! (8 octets, big-endian), the number of keys (2 octets, at most [`MAX_KEYS`]), and for each
//! key its 32 private octets, its `seq` (8 octets) and its `issued_at` as one length octet and
//! that many ASCII octets. A reader accepts exactly that and nothing after it.

use std::fmt;

use hpke::aead::{AeadTag, ChaCha20Poly1305};
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{
    Deserializable, Kem as KemTrait, OpModeR, OpModeS, PskBundle, Serializable,
    single_shot_open_in_place_detached, single_shot_seal_in_place_detached,
};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::ids::{KeyId, Timestamp};
use crate::sealing::{AgreementKeyState, AgreementPrivateKey, OsRng, StoredAgreementKey};

type Kem = X25519HkdfSha256;
type Kdf = HkdfSha256;
type Aead = ChaCha20Poly1305;

/// The HPKE `info` and `psk_id` of the stored file: the ASCII octets `oac-agreement-store-v1`.
/// Not [`crate::sealing::SEAL_INFO`].
pub const AT_REST_INFO: &[u8] = b"oac-agreement-store-v1";

/// The derivation label of the storage key pair, followed by `0x00` and the seed.
const KEM_LABEL: &[u8] = b"oac-agreement-store-kem-v1";

/// The derivation label of the PSK, followed by `0x00` and the seed.
const PSK_LABEL: &[u8] = b"oac-agreement-store-psk-v1";

/// The start of every stored file.
const MAGIC: &[u8; 4] = b"OACR";

/// The file version this module writes and reads.
const VERSION: u8 = 1;

/// The kind octet of a file of agreement keys.
const KIND_AGREEMENT_KEYS: u8 = 1;

/// The header: magic, version, kind and the 64 digits of the key id. It is the associated data.
pub const HEADER_LEN: usize = 4 + 1 + 1 + 64;

const ENC_LEN: usize = 32;
const TAG_LEN: usize = 16;

/// The smallest file: header, `enc` and the tag of an empty plaintext.
const MIN_FILE: usize = HEADER_LEN + ENC_LEN + TAG_LEN;

/// The most keys a stored state holds. A device holds a few (the current one and those not yet
/// erased under [SEC-SEL-018]).
pub const MAX_KEYS: usize = 1024;

/// The longest `issued_at` text (`spec/session-channels.md` §4.4.6: nine fraction digits).
const MAX_TIMESTAMP_TEXT: usize = 40;

/// The most octets the plaintext of a full state takes.
const MAX_PLAINTEXT: usize = 1 + 8 + 2 + MAX_KEYS * (32 + 8 + 1 + MAX_TIMESTAMP_TEXT);

/// The largest file this module reads or writes.
pub const MAX_FILE_LEN: usize = HEADER_LEN + ENC_LEN + MAX_PLAINTEXT + TAG_LEN;

/// Why a stored file was not made or did not open. Says nothing of key material, and nothing
/// of which check failed beyond this classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtRestError {
    /// The state is larger than a stored file may be, or the file is.
    TooLarge,
    /// The file is too short, or does not begin with the magic.
    Malformed,
    /// The file has another version or kind.
    Unsupported,
    /// The header names another device key id than the handle's.
    NotThisDevice,
    /// The file did not authenticate under this device's seed: it was edited, was not sealed
    /// by a holder of the seed, or was sealed for another purpose.
    Authentication,
    /// The file authenticated, but its plaintext is not a state this module wrote.
    Corrupt,
    /// The seal itself failed.
    Seal,
}

impl fmt::Display for AtRestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AtRestError::TooLarge => "larger than an agreement-key file may be",
            AtRestError::Malformed => "not an agreement-key file",
            AtRestError::Unsupported => "an agreement-key file of another version or kind",
            AtRestError::NotThisDevice => "an agreement-key file of another device key id",
            AtRestError::Authentication => {
                "it did not authenticate under this device's key: it was edited, or was not sealed by this device"
            }
            AtRestError::Corrupt => "authentic, but not a state this version wrote",
            AtRestError::Seal => "the state could not be sealed",
        })
    }
}

impl std::error::Error for AtRestError {}

/// What seals and opens one device's agreement-key file: the storage key pair and the PSK
/// derived from the device key's seed, and the device key id they belong to. Zeroized on drop;
/// no `Clone`; its `Debug` shows the key id only.
///
/// Made by [`crate::keys::DeviceIdentity::at_rest`] and by nothing else outside this crate.
pub struct AtRest {
    key_id: KeyId,
    private: <Kem as KemTrait>::PrivateKey,
    public: <Kem as KemTrait>::PublicKey,
    psk: Zeroizing<[u8; 32]>,
}

impl fmt::Debug for AtRest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AtRest(key_id: {})", self.key_id)
    }
}

impl AtRest {
    /// The handle for the device key whose seed is `seed` and whose key id is `key_id`.
    pub(crate) fn derive(seed: &[u8; 32], key_id: KeyId) -> AtRest {
        let mut ikm = Zeroizing::new(Vec::with_capacity(KEM_LABEL.len() + 1 + seed.len()));
        ikm.extend_from_slice(KEM_LABEL);
        ikm.push(0);
        ikm.extend_from_slice(seed);
        let (private, public) = <Kem as KemTrait>::derive_keypair(&ikm);

        let mut hasher = Sha256::new();
        hasher.update(PSK_LABEL);
        hasher.update([0u8]);
        hasher.update(seed);
        let mut digest = hasher.finalize();
        let mut psk = Zeroizing::new([0u8; 32]);
        psk.copy_from_slice(&digest);
        // `zeroize` writes volatile and cannot be optimized away, as a plain `fill(0)` of a
        // value that is not used again can be.
        zeroize::Zeroize::zeroize(digest.as_mut_slice());

        AtRest {
            key_id,
            private,
            public,
            psk,
        }
    }

    /// The device key id this handle belongs to.
    pub fn key_id(&self) -> &KeyId {
        &self.key_id
    }

    /// The name of this device's file: `agreement-<key id>.sealed`. The key id is the
    /// handle's own, so a caller cannot name another device's file through it.
    pub fn file_name(&self) -> String {
        format!("agreement-{}.sealed", self.key_id)
    }

    /// The bytes of the file that holds `state`, sealed for this device.
    ///
    /// # Errors
    ///
    /// [`AtRestError::TooLarge`] for a state of more than [`MAX_KEYS`] keys,
    /// [`AtRestError::Corrupt`] for a state a reader would not accept (a `seq` beyond the
    /// highest), [`AtRestError::Seal`].
    pub fn seal_agreement_state(&self, state: &AgreementKeyState) -> Result<Vec<u8>, AtRestError> {
        let plaintext = encode_state(state)?;
        // `hpke` 0.12 does not run RFC 9180's `VerifyPSKInputs` (its `setup.rs` only has the
        // check as a comment; `op_mode.rs` documents the requirement on the caller). So this
        // code always supplies a 32-octet PSK and a non-empty `psk_id`: the PSK is the SHA-256
        // output of `derive`, and the `psk_id` is [`AT_REST_INFO`].
        debug_assert!(self.psk.len() == 32 && !AT_REST_INFO.is_empty());
        self.seal_raw(
            &self.key_id,
            &OpModeS::Psk(PskBundle {
                psk: self.psk.as_slice(),
                psk_id: AT_REST_INFO,
            }),
            AT_REST_INFO,
            &plaintext,
        )
    }

    /// The state in `file`, which this device sealed.
    ///
    /// # Errors
    ///
    /// [`AtRestError`]. Fails closed: a file that was edited in any octet, written by anyone
    /// who does not hold the seed, sealed in another mode or under another `info`, or renamed
    /// to another key id, does not open.
    pub fn open_agreement_state(&self, file: &[u8]) -> Result<AgreementKeyState, AtRestError> {
        self.open_raw(
            file,
            &OpModeR::Psk(PskBundle {
                psk: self.psk.as_slice(),
                psk_id: AT_REST_INFO,
            }),
            AT_REST_INFO,
        )
    }

    /// The file for `plaintext`, with `header_key_id` in the header and the given mode and
    /// `info`. [`AtRest::seal_agreement_state`] is this with this device's own values; the
    /// tests make the files that must be refused through it.
    fn seal_raw(
        &self,
        header_key_id: &KeyId,
        mode: &OpModeS<Kem>,
        info: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, AtRestError> {
        let header = header(header_key_id);
        let mut buf = Zeroizing::new(plaintext.to_vec());
        let (enc, tag) = single_shot_seal_in_place_detached::<Aead, Kdf, Kem, _>(
            mode,
            &self.public,
            info,
            buf.as_mut_slice(),
            &header,
            &mut OsRng,
        )
        .map_err(|_| AtRestError::Seal)?;
        let mut file = Vec::with_capacity(HEADER_LEN + ENC_LEN + buf.len() + TAG_LEN);
        file.extend_from_slice(&header);
        file.extend_from_slice(enc.to_bytes().as_slice());
        file.extend_from_slice(buf.as_slice());
        file.extend_from_slice(tag.to_bytes().as_slice());
        Ok(file)
    }

    fn open_raw(
        &self,
        file: &[u8],
        mode: &OpModeR<Kem>,
        info: &[u8],
    ) -> Result<AgreementKeyState, AtRestError> {
        if file.len() > MAX_FILE_LEN {
            return Err(AtRestError::TooLarge);
        }
        if file.len() < MIN_FILE || &file[..MAGIC.len()] != MAGIC {
            return Err(AtRestError::Malformed);
        }
        if file[4] != VERSION || file[5] != KIND_AGREEMENT_KEYS {
            return Err(AtRestError::Unsupported);
        }
        let (head, rest) = file.split_at(HEADER_LEN);
        if &head[6..] != self.key_id.as_str().as_bytes() {
            return Err(AtRestError::NotThisDevice);
        }
        let enc = <Kem as KemTrait>::EncappedKey::from_bytes(&rest[..ENC_LEN])
            .map_err(|_| AtRestError::Malformed)?;
        let (ciphertext, tag) = rest[ENC_LEN..].split_at(rest.len() - ENC_LEN - TAG_LEN);
        let tag = AeadTag::<Aead>::from_bytes(tag).map_err(|_| AtRestError::Malformed)?;
        let mut plaintext = Zeroizing::new(ciphertext.to_vec());
        single_shot_open_in_place_detached::<Aead, Kdf, Kem>(
            mode,
            &self.private,
            &enc,
            info,
            plaintext.as_mut_slice(),
            head,
            &tag,
        )
        .map_err(|_| AtRestError::Authentication)?;
        decode_state(&plaintext)
    }
}

/// The 70 octets that begin a file and are its associated data.
fn header(key_id: &KeyId) -> [u8; HEADER_LEN] {
    let mut head = [0u8; HEADER_LEN];
    head[..4].copy_from_slice(MAGIC);
    head[4] = VERSION;
    head[5] = KIND_AGREEMENT_KEYS;
    head[6..].copy_from_slice(key_id.as_str().as_bytes());
    head
}

/// The octets [`encode_state`] writes for `state`, which it reserves exactly: a buffer that
/// held private keys and then grew would leave its freed predecessor, with the keys in it,
/// unzeroized. The checks of the writer are here, so the writer cannot fail half-way.
fn encoded_len(state: &AgreementKeyState) -> Result<usize, AtRestError> {
    if state.keys.len() > MAX_KEYS {
        return Err(AtRestError::TooLarge);
    }
    let mut total = 1 + 8 + 2;
    for key in &state.keys {
        // A reader rejects a key whose `seq` is above the highest recorded one.
        if state.highest_seq.is_none_or(|h| key.seq > h) {
            return Err(AtRestError::Corrupt);
        }
        let len = key.issued_at.as_str().len();
        if len == 0 || len > MAX_TIMESTAMP_TEXT {
            return Err(AtRestError::Corrupt);
        }
        total += 32 + 8 + 1 + len;
    }
    Ok(total)
}

fn encode_state(state: &AgreementKeyState) -> Result<Zeroizing<Vec<u8>>, AtRestError> {
    let total = encoded_len(state)?;
    let count = u16::try_from(state.keys.len()).map_err(|_| AtRestError::TooLarge)?;
    let mut out = Zeroizing::new(Vec::with_capacity(total));
    out.push(u8::from(state.highest_seq.is_some()));
    out.extend_from_slice(&state.highest_seq.unwrap_or(0).to_be_bytes());
    out.extend_from_slice(&count.to_be_bytes());
    for key in &state.keys {
        let text = key.issued_at.as_str().as_bytes();
        // `encoded_len` checked the length against [`MAX_TIMESTAMP_TEXT`], below 256.
        let len = u8::try_from(text.len()).map_err(|_| AtRestError::Corrupt)?;
        out.extend_from_slice(key.private_key.expose());
        out.extend_from_slice(&key.seq.to_be_bytes());
        out.push(len);
        out.extend_from_slice(text);
    }
    debug_assert_eq!(out.len(), total);
    Ok(out)
}

/// A cursor over the plaintext that refuses to read past its end.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], AtRestError> {
        if self.0.len() < n {
            return Err(AtRestError::Corrupt);
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }

    fn u64(&mut self) -> Result<u64, AtRestError> {
        let mut b = [0u8; 8];
        b.copy_from_slice(self.take(8)?);
        Ok(u64::from_be_bytes(b))
    }
}

fn decode_state(plaintext: &[u8]) -> Result<AgreementKeyState, AtRestError> {
    let mut r = Reader(plaintext);
    let flag = r.take(1)?[0];
    let highest = r.u64()?;
    let highest_seq = match (flag, highest) {
        (0, 0) => None,
        (1, h) => Some(h),
        _ => return Err(AtRestError::Corrupt),
    };
    let mut count = [0u8; 2];
    count.copy_from_slice(r.take(2)?);
    let count = usize::from(u16::from_be_bytes(count));
    if count > MAX_KEYS {
        return Err(AtRestError::Corrupt);
    }
    let mut keys = Vec::with_capacity(count);
    for _ in 0..count {
        let mut octets = Zeroizing::new([0u8; 32]);
        octets.copy_from_slice(r.take(32)?);
        let private_key = AgreementPrivateKey::from_octets(&mut octets);
        let seq = r.u64()?;
        let len = usize::from(r.take(1)?[0]);
        if len == 0 || len > MAX_TIMESTAMP_TEXT {
            return Err(AtRestError::Corrupt);
        }
        let text = std::str::from_utf8(r.take(len)?).map_err(|_| AtRestError::Corrupt)?;
        // Parsing keeps the spelling, and a spelling that is not written back as it was read
        // is refused, so a state round-trips exactly.
        let issued_at = Timestamp::parse(text)
            .filter(|t| t.as_str() == text)
            .ok_or(AtRestError::Corrupt)?;
        keys.push(StoredAgreementKey {
            private_key,
            seq,
            issued_at,
        });
    }
    if !r.0.is_empty() {
        return Err(AtRestError::Corrupt);
    }
    Ok(AgreementKeyState { keys, highest_seq })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::Token;
    use crate::keys::{DeviceIdentity, DeviceKey, SecretSeed};
    use crate::sealing::SEAL_INFO;

    fn identity(n: u8) -> DeviceIdentity {
        let key = DeviceKey::from_seed(&SecretSeed::from_octets(&mut [n; 32]));
        DeviceIdentity::new(key, Token::parse("device").unwrap())
    }

    fn handle(n: u8) -> AtRest {
        identity(n).at_rest()
    }

    fn key(n: u8, seq: u64, at: &str) -> StoredAgreementKey {
        StoredAgreementKey {
            private_key: AgreementPrivateKey::from_octets(&mut [n; 32]),
            seq,
            issued_at: Timestamp::parse(at).unwrap(),
        }
    }

    fn state() -> AgreementKeyState {
        AgreementKeyState {
            keys: vec![
                key(0x61, 3, "2026-11-03T10:00:00.123456789Z"),
                key(0x62, 1, "2026-11-01T09:30:00Z"),
            ],
            highest_seq: Some(3),
        }
    }

    fn same(a: &AgreementKeyState, b: &AgreementKeyState) -> bool {
        a.highest_seq == b.highest_seq
            && a.keys.len() == b.keys.len()
            && a.keys.iter().zip(&b.keys).all(|(x, y)| {
                x.seq == y.seq
                    && x.issued_at == y.issued_at
                    && x.private_key.expose() == y.private_key.expose()
            })
    }

    fn policy_psk(a: &AtRest) -> OpModeS<'_, Kem> {
        OpModeS::Psk(PskBundle {
            psk: a.psk.as_slice(),
            psk_id: AT_REST_INFO,
        })
    }

    #[test]
    fn a_state_round_trips_exactly() {
        let a = handle(1);
        let file = a.seal_agreement_state(&state()).unwrap();
        assert!(same(&a.open_agreement_state(&file).unwrap(), &state()));
        let empty = AgreementKeyState::default();
        let file = a.seal_agreement_state(&empty).unwrap();
        assert!(same(&a.open_agreement_state(&file).unwrap(), &empty));
        // An erased-keys state keeps its count.
        let counted = AgreementKeyState {
            keys: Vec::new(),
            highest_seq: Some(7),
        };
        let file = a.seal_agreement_state(&counted).unwrap();
        assert!(same(&a.open_agreement_state(&file).unwrap(), &counted));
    }

    #[test]
    fn the_file_name_and_header_carry_the_handles_own_key_id() {
        let id = identity(1);
        let a = id.at_rest();
        assert_eq!(a.key_id(), id.key_id());
        assert_eq!(
            a.file_name(),
            format!("agreement-{}.sealed", id.key_id().as_str())
        );
        let file = a.seal_agreement_state(&state()).unwrap();
        assert_eq!(&file[..4], b"OACR");
        assert_eq!(&file[4..6], &[1, 1]);
        assert_eq!(&file[6..HEADER_LEN], id.key_id().as_str().as_bytes());
        assert_eq!(format!("{a:?}"), format!("AtRest(key_id: {})", id.key_id()));
    }

    #[test]
    fn the_agreement_file_holds_no_plaintext_key() {
        let a = handle(1);
        let st = state();
        let file = a.seal_agreement_state(&st).unwrap();
        for k in &st.keys {
            let octets = k.private_key.expose();
            assert!(
                !file.windows(32).any(|w| w == octets),
                "the private key's octets are in the file"
            );
            let b64 = crate::base64url::encode(octets);
            let hex: String = octets.iter().map(|b| format!("{b:02x}")).collect();
            for needle in [b64, hex] {
                assert!(
                    !file.windows(needle.len()).any(|w| w == needle.as_bytes()),
                    "the private key is in the file as text"
                );
            }
        }
        // Nor the plaintext's other members: the timestamps are inside the ciphertext.
        assert!(
            !file.windows(10).any(|w| w == b"2026-11-03"),
            "a timestamp is in the file in the clear"
        );
        // And not the seed or anything derived from it, in the clear.
        assert!(!file.windows(32).any(|w| w == a.psk.as_slice()));
        assert!(
            !file
                .windows(32)
                .any(|w| w == a.public.to_bytes().as_slice())
        );
    }

    #[test]
    fn a_file_sealed_in_base_mode_is_refused() {
        // Anyone who knows the storage public key can seal in base mode; the file must not open.
        let a = handle(1);
        let plaintext = encode_state(&state()).unwrap();
        let file = a
            .seal_raw(&a.key_id, &OpModeS::Base, AT_REST_INFO, &plaintext)
            .unwrap();
        assert_eq!(
            a.open_agreement_state(&file).unwrap_err(),
            AtRestError::Authentication
        );
        // The same file does open in base mode, so the refusal is the mode, not the file.
        assert!(a.open_raw(&file, &OpModeR::Base, AT_REST_INFO).is_ok());
    }

    #[test]
    fn a_file_sealed_under_the_wrong_psk_is_refused() {
        let a = handle(1);
        let plaintext = encode_state(&state()).unwrap();
        let wrong = [0x5au8; 32];
        let file = a
            .seal_raw(
                &a.key_id,
                &OpModeS::Psk(PskBundle {
                    psk: &wrong,
                    psk_id: AT_REST_INFO,
                }),
                AT_REST_INFO,
                &plaintext,
            )
            .unwrap();
        assert_eq!(
            a.open_agreement_state(&file).unwrap_err(),
            AtRestError::Authentication
        );
        // A different psk_id is a different PSK input too.
        let file = a
            .seal_raw(
                &a.key_id,
                &OpModeS::Psk(PskBundle {
                    psk: a.psk.as_slice(),
                    psk_id: b"another-psk-id",
                }),
                AT_REST_INFO,
                &plaintext,
            )
            .unwrap();
        assert_eq!(
            a.open_agreement_state(&file).unwrap_err(),
            AtRestError::Authentication
        );
    }

    #[test]
    fn a_file_sealed_under_the_wrong_info_is_refused() {
        let a = handle(1);
        let plaintext = encode_state(&state()).unwrap();
        for info in [SEAL_INFO, b"oac-agreement-store-v2", b""] {
            let file = a
                .seal_raw(&a.key_id, &policy_psk(&a), info, &plaintext)
                .unwrap();
            assert_eq!(
                a.open_agreement_state(&file).unwrap_err(),
                AtRestError::Authentication,
                "info {info:?}"
            );
        }
    }

    #[test]
    fn a_file_renamed_to_another_key_id_is_refused() {
        let (a, b) = (handle(1), handle(2));
        let file = a.seal_agreement_state(&state()).unwrap();
        // Another device's handle refuses the file by its header.
        assert_eq!(
            b.open_agreement_state(&file).unwrap_err(),
            AtRestError::NotThisDevice
        );
        // And with the header rewritten to b's id it still fails, now by authentication:
        // the header is the associated data and the seed is not b's.
        let mut renamed = file.clone();
        renamed[6..HEADER_LEN].copy_from_slice(b.key_id().as_str().as_bytes());
        assert_eq!(
            b.open_agreement_state(&renamed).unwrap_err(),
            AtRestError::Authentication
        );
        // The header alone is bound: one seed under two key ids, a file sealed for the first
        // with the second's id written into its header does not open for the second.
        let seed = [9u8; 32];
        let first = AtRest::derive(&seed, identity(1).key_id().clone());
        let second = AtRest::derive(&seed, identity(2).key_id().clone());
        let sealed = first.seal_agreement_state(&state()).unwrap();
        let mut moved = sealed.clone();
        moved[6..HEADER_LEN].copy_from_slice(second.key_id().as_str().as_bytes());
        assert_eq!(
            second.open_agreement_state(&moved).unwrap_err(),
            AtRestError::Authentication
        );
        assert!(first.open_agreement_state(&sealed).is_ok());
    }

    #[test]
    fn a_file_the_seed_did_not_seal_fails_to_open() {
        // Review blocker B2. A writer with the same key id but another seed.
        let a = handle(1);
        let other = AtRest::derive(&[0x77; 32], a.key_id().clone());
        let file = other.seal_agreement_state(&state()).unwrap();
        assert_eq!(
            a.open_agreement_state(&file).unwrap_err(),
            AtRestError::Authentication
        );
        // A writer that knows the storage public key and picks its own PSK.
        let attacker_psk = [0x42u8; 32];
        let plaintext = encode_state(&state()).unwrap();
        let forged = a
            .seal_raw(
                &a.key_id,
                &OpModeS::Psk(PskBundle {
                    psk: &attacker_psk,
                    psk_id: AT_REST_INFO,
                }),
                AT_REST_INFO,
                &plaintext,
            )
            .unwrap();
        assert_eq!(
            a.open_agreement_state(&forged).unwrap_err(),
            AtRestError::Authentication
        );
    }

    #[test]
    fn an_edited_agreement_file_fails_authentication() {
        let a = handle(1);
        let file = a.seal_agreement_state(&state()).unwrap();
        assert!(a.open_agreement_state(&file).is_ok());
        // One bit of every octet of the file: the header (the associated data), `enc`, the ciphertext, the tag.
        for i in 0..file.len() {
            let mut edited = file.clone();
            edited[i] ^= 1 << (i % 8);
            assert!(
                a.open_agreement_state(&edited).is_err(),
                "octet {i} was edited and the file opened"
            );
        }
        // Truncated and extended.
        for n in (0..=MIN_FILE + 1).chain([file.len() / 2, file.len() - 1]) {
            assert!(a.open_agreement_state(&file[..n]).is_err(), "length {n}");
        }
        let mut longer = file.clone();
        longer.push(0);
        assert!(a.open_agreement_state(&longer).is_err());
        // The edits are refused as authentication failures wherever the structure is intact.
        // The top bit of the last octet of `enc`, which the loop above (bit `i % 8`) never
        // reaches: X25519 ignores it, so the edit is refused by the key schedule's other
        // inputs, not by a parse error.
        let mut top_bit = file.clone();
        top_bit[HEADER_LEN + ENC_LEN - 1] ^= 0x80;
        assert_eq!(
            a.open_agreement_state(&top_bit).unwrap_err(),
            AtRestError::Authentication
        );
        let mut ciphertext = file.clone();
        ciphertext[HEADER_LEN + ENC_LEN] ^= 1;
        assert_eq!(
            a.open_agreement_state(&ciphertext).unwrap_err(),
            AtRestError::Authentication
        );
        let mut version = file.clone();
        version[4] = 2;
        assert_eq!(
            a.open_agreement_state(&version).unwrap_err(),
            AtRestError::Unsupported
        );
    }

    #[test]
    fn each_write_draws_a_fresh_enc() {
        let a = handle(1);
        let st = state();
        let files: Vec<Vec<u8>> = (0..8)
            .map(|_| a.seal_agreement_state(&st).unwrap())
            .collect();
        for (i, x) in files.iter().enumerate() {
            for y in &files[i + 1..] {
                let (ex, ey) = (
                    &x[HEADER_LEN..HEADER_LEN + ENC_LEN],
                    &y[HEADER_LEN..HEADER_LEN + ENC_LEN],
                );
                assert_ne!(ex, ey, "two writes share an enc");
                assert_ne!(x[HEADER_LEN + ENC_LEN..], y[HEADER_LEN + ENC_LEN..]);
            }
        }
    }

    #[test]
    fn the_info_is_not_the_payload_info() {
        assert_ne!(AT_REST_INFO, SEAL_INFO);
        assert_eq!(AT_REST_INFO, b"oac-agreement-store-v1");
        // The module never names the payload info; its source is checked here.
        let production: String = include_str!("at_rest.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap()
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!production.contains("SEAL_INFO"));
        assert!(!production.contains("oac-seal-v1"));
    }

    #[test]
    fn the_handle_has_no_key_id_parameter() {
        // A source check, like the one that keeps a seed out of the binary's signatures: no
        // item of this module that is visible outside the module takes a key id, or a string
        // that could carry one, so a caller cannot ask for another device's file. Any form of
        // visibility counts (`pub`, `pub(crate)`, `pub const`...). The one exception is named:
        // `derive`, which only `DeviceIdentity::at_rest` calls with the identity's own id.
        let source = include_str!("at_rest.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        let mut checked = 0;
        let mut lines = production.lines();
        while let Some(line) = lines.next() {
            let Some(after_pub) = line.trim_start().strip_prefix("pub") else {
                continue;
            };
            let mut rest = after_pub;
            if rest.starts_with('(') {
                rest = rest.split_once(')').map_or("", |(_, r)| r);
            }
            let mut rest = rest.trim_start();
            while let Some(r) = rest
                .strip_prefix("const ")
                .or_else(|| rest.strip_prefix("unsafe "))
                .or_else(|| rest.strip_prefix("async "))
            {
                rest = r.trim_start();
            }
            let Some(declaration) = rest.strip_prefix("fn ") else {
                continue;
            };
            // The signature: from `fn` to the line that opens the body.
            let mut signature = declaration.to_owned();
            while !signature.contains('{') {
                signature.push_str(lines.next().expect("a body"));
            }
            if signature.starts_with("derive(") {
                // The deliberate exception; it is `pub(crate)`, not reachable from outside.
                assert!(line.contains("pub(crate)"), "{line}");
                continue;
            }
            // The parameter list only: from the first `(` to the `->` or the body.
            let parameters = signature
                .split_once('(')
                .map(|(_, rest)| rest.split("->").next().unwrap())
                .unwrap();
            for forbidden in ["KeyId", "key_id", "&str", "String"] {
                assert!(
                    !parameters.contains(forbidden),
                    "a visible fn takes {forbidden}: {signature}"
                );
            }
            checked += 1;
        }
        // Not vacuous: the four public methods were looked at.
        assert!(checked >= 4, "{checked} visible fns checked");
    }

    #[test]
    fn encoding_reserves_the_exact_length_so_no_freed_buffer_holds_a_key() {
        // A `Zeroizing<Vec>` that grows frees its old buffer unzeroized, with the private keys
        // in it. The capacity reserved up front must be the length written.
        for count in [0usize, 1, 2, 7] {
            for text in ["2026-11-03T10:00:00Z", "2026-11-03T10:00:00.123456789Z"] {
                let st = AgreementKeyState {
                    keys: (0..count)
                        .map(|i| key(0x70 + i as u8, i as u64, text))
                        .collect(),
                    highest_seq: Some(count as u64),
                };
                let reserved = encoded_len(&st).unwrap();
                let out = encode_state(&st).unwrap();
                assert_eq!(out.len(), reserved);
                assert_eq!(out.capacity(), reserved, "the buffer grew: {count} keys");
            }
        }
    }

    #[test]
    fn a_key_above_the_highest_seq_is_not_written() {
        let a = handle(1);
        let above = AgreementKeyState {
            keys: vec![key(1, 5, "2026-11-03T10:00:00Z")],
            highest_seq: Some(4),
        };
        assert_eq!(
            a.seal_agreement_state(&above).unwrap_err(),
            AtRestError::Corrupt
        );
        let uncounted = AgreementKeyState {
            keys: vec![key(1, 0, "2026-11-03T10:00:00Z")],
            highest_seq: None,
        };
        assert_eq!(
            a.seal_agreement_state(&uncounted).unwrap_err(),
            AtRestError::Corrupt
        );
    }

    #[test]
    fn the_seed_is_not_what_the_values_are_derived_by_themselves() {
        // Two seeds give two handles; the same seed twice gives the same one (the file
        // written by one run opens in the next).
        let seed = [3u8; 32];
        let id = identity(3).key_id().clone();
        let (x, y) = (AtRest::derive(&seed, id.clone()), AtRest::derive(&seed, id));
        assert_eq!(x.psk.as_slice(), y.psk.as_slice());
        assert_eq!(x.public.to_bytes(), y.public.to_bytes());
        assert_ne!(x.psk.as_slice(), &seed);
        assert_ne!(x.public.to_bytes().as_slice(), &seed);
        // The two derivations are separated by label: the PSK is not the KEM input.
        assert_ne!(x.psk.as_slice(), x.public.to_bytes().as_slice());
        let z = AtRest::derive(&[4u8; 32], identity(4).key_id().clone());
        assert_ne!(x.psk.as_slice(), z.psk.as_slice());
    }

    #[test]
    fn a_state_a_reader_would_not_accept_is_not_written() {
        let a = handle(1);
        let too_many = AgreementKeyState {
            keys: (0..=MAX_KEYS)
                .map(|i| key(1, i as u64, "2026-11-03T10:00:00Z"))
                .collect(),
            highest_seq: Some(MAX_KEYS as u64),
        };
        assert_eq!(
            a.seal_agreement_state(&too_many).unwrap_err(),
            AtRestError::TooLarge
        );
        let file = vec![0u8; MAX_FILE_LEN + 1];
        assert_eq!(
            a.open_agreement_state(&file).unwrap_err(),
            AtRestError::TooLarge
        );
    }

    #[test]
    fn a_plaintext_that_authenticates_but_is_not_a_state_is_corrupt() {
        let a = handle(1);
        let seal = |plaintext: &[u8]| {
            a.seal_raw(&a.key_id, &policy_psk(&a), AT_REST_INFO, plaintext)
                .unwrap()
        };
        let good = encode_state(&state()).unwrap();
        // Trailing octets, a short body, a flag that is neither 0 nor 1, a count past the keys.
        let mut trailing = good.to_vec();
        trailing.push(0);
        let mut bad_flag = good.to_vec();
        bad_flag[0] = 2;
        let mut flag_zero_with_seq = good.to_vec();
        flag_zero_with_seq[0] = 0;
        let mut overcount = good.to_vec();
        overcount[9] = 0xff;
        overcount[10] = 0xff;
        for plaintext in [
            trailing,
            bad_flag,
            flag_zero_with_seq,
            overcount,
            good[..good.len() - 1].to_vec(),
            Vec::new(),
        ] {
            assert_eq!(
                a.open_agreement_state(&seal(&plaintext)).unwrap_err(),
                AtRestError::Corrupt,
                "{plaintext:?}"
            );
        }
    }

    // ---- RFC 9180 Appendix A -----------------------------------------------------------------

    fn unhex(s: &str) -> Vec<u8> {
        assert_eq!(s.len() % 2, 0);
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    /// An RNG that returns the same octets from `fill_bytes` every time: `ikmE` of the vector,
    /// from which `hpke` draws its ephemeral key exactly as RFC 9180 §4.1 does
    /// (`skE = DeriveKeyPair(ikmE)`).
    struct Fixed(Vec<u8>);

    impl hpke::rand_core::RngCore for Fixed {
        fn next_u32(&mut self) -> u32 {
            unreachable!("hpke draws its ephemeral key with fill_bytes")
        }
        fn next_u64(&mut self) -> u64 {
            unreachable!("hpke draws its ephemeral key with fill_bytes")
        }
        fn fill_bytes(&mut self, dest: &mut [u8]) {
            assert_eq!(dest.len(), self.0.len());
            dest.copy_from_slice(&self.0);
        }
        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), hpke::rand_core::Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }

    impl hpke::rand_core::CryptoRng for Fixed {}

    /// RFC 9180 Appendix A.2.2, "PSK Setup Information" of DHKEM(X25519, HKDF-SHA256),
    /// HKDF-SHA256, ChaCha20Poly1305 (mode 1, kem_id 32, kdf_id 1, aead_id 3), with the
    /// encryption of sequence number 0 (A.2.2.1) and the exported value for an empty context
    /// (A.2.2.2). Source: https://www.rfc-editor.org/rfc/rfc9180 (text at
    /// https://www.rfc-editor.org/rfc/rfc9180.txt), Appendix A.2.2, retrieved 2026-11-04. The
    /// hexadecimal strings are copied from it by a script that joins the RFC's wrapped lines;
    /// none is typed by hand.
    #[test]
    fn rfc9180_appendix_a_psk_vector_passes_on_hpke_0_12() {
        let info = unhex("4f6465206f6e2061204772656369616e2055726e");
        let ikm_e = unhex("35706a0b09fb26fb45c39c2f5079c709c7cf98e43afa973f14d88ece7e29c2e3");
        let pk_em = unhex("2261299c3f40a9afc133b969a97f05e95be2c514e54f3de26cbe5644ac735b04");
        let sk_em = unhex("0c35fdf49df7aa01cd330049332c40411ebba36e0c718ebc3edf5845795f6321");
        let ikm_r = unhex("26b923eade72941c8a85b09986cdfa3f1296852261adedc52d58d2930269812b");
        let pk_rm = unhex("13640af826b722fc04feaa4de2f28fbd5ecc03623b317834e7ff4120dbe73062");
        let sk_rm = unhex("77d114e0212be51cb1d76fa99dd41cfd4d0166b08caa09074430a6c59ef17879");
        let psk = unhex("0247fd33b913760fa1fa51e1892d9f307fbe65eb171e8132c2af18555a738b82");
        let psk_id = unhex("456e6e796e20447572696e206172616e204d6f726961");
        let enc = unhex("2261299c3f40a9afc133b969a97f05e95be2c514e54f3de26cbe5644ac735b04");
        let pt = unhex("4265617574792069732074727574682c20747275746820626561757479");
        let aad = unhex("436f756e742d30");
        let ct = unhex(
            "4a177f9c0d6f15cfdf533fb65bf84aecdc6ab16b8b85b4cf65a370e07fc1d78d28fb073214525276f4a89608ff",
        );
        let exported = unhex("813c1bfc516c99076ae0f466671f0ba5ff244a41699f7b2417e4c59d46d39f40");

        // DeriveKeyPair of both ikm values gives the RFC's keys.
        let (sk_e, pk_e) = <Kem as KemTrait>::derive_keypair(&ikm_e);
        assert_eq!(sk_e.to_bytes().as_slice(), sk_em.as_slice());
        assert_eq!(pk_e.to_bytes().as_slice(), pk_em.as_slice());
        let (sk_r, pk_r) = <Kem as KemTrait>::derive_keypair(&ikm_r);
        assert_eq!(sk_r.to_bytes().as_slice(), sk_rm.as_slice());
        assert_eq!(pk_r.to_bytes().as_slice(), pk_rm.as_slice());

        // The sender's setup in PSK mode with the RFC's ephemeral key: `enc` is pkEm, and the
        // first encryption and the exporter value are the RFC's.
        let mode = OpModeS::Psk(PskBundle {
            psk: &psk,
            psk_id: &psk_id,
        });
        let (got_enc, mut sender) =
            hpke::setup_sender::<Aead, Kdf, Kem, _>(&mode, &pk_r, &info, &mut Fixed(ikm_e.clone()))
                .unwrap();
        assert_eq!(got_enc.to_bytes().as_slice(), enc.as_slice());
        let mut out = [0u8; 32];
        sender.export(&[], &mut out).unwrap();
        assert_eq!(out.as_slice(), exported.as_slice());
        assert_eq!(sender.seal(&pt, &aad).unwrap(), ct);

        // The entry points the product uses (`seal_raw` and `open_raw` call exactly these,
        // with this module's `Aead`, `Kdf` and `Kem`): `single_shot_seal_in_place_detached`
        // and `single_shot_open_in_place_detached`. They are called here directly, because
        // `seal_raw` and `open_raw` fix the keys, the `info` and the associated data of a
        // device's file and cannot take the vector's. Sealing in place with the fixed ephemeral
        // key gives the vector's `enc` and `ct || tag`...
        let mut buf = pt.clone();
        let (enc_in_place, tag) = single_shot_seal_in_place_detached::<Aead, Kdf, Kem, _>(
            &mode,
            &pk_r,
            &info,
            buf.as_mut_slice(),
            &aad,
            &mut Fixed(ikm_e.clone()),
        )
        .unwrap();
        assert_eq!(enc_in_place.to_bytes().as_slice(), enc.as_slice());
        let mut ct_and_tag = buf.clone();
        ct_and_tag.extend_from_slice(tag.to_bytes().as_slice());
        assert_eq!(ct_and_tag, ct);
        // ...and opening in place the vector's `ct`, split into the ciphertext and its
        // 16-octet tag as `open_raw` splits a file, gives the vector's plaintext.
        let (body, tag_octets) = ct.split_at(ct.len() - TAG_LEN);
        let tag = AeadTag::<Aead>::from_bytes(tag_octets).unwrap();
        let mut plain = body.to_vec();
        single_shot_open_in_place_detached::<Aead, Kdf, Kem>(
            &OpModeR::Psk(PskBundle {
                psk: &psk,
                psk_id: &psk_id,
            }),
            &sk_r,
            &<Kem as KemTrait>::EncappedKey::from_bytes(&enc).unwrap(),
            &info,
            plain.as_mut_slice(),
            &aad,
            &tag,
        )
        .unwrap();
        assert_eq!(plain, pt);

        // The receiver's setup opens the RFC's ciphertext (sequence number 0), in PSK mode.
        let receiver_mode = OpModeR::Psk(PskBundle {
            psk: &psk,
            psk_id: &psk_id,
        });
        let enc_key = <Kem as KemTrait>::EncappedKey::from_bytes(&enc).unwrap();
        let opened = hpke::single_shot_open::<Aead, Kdf, Kem>(
            &receiver_mode,
            &sk_r,
            &enc_key,
            &info,
            &ct,
            &aad,
        )
        .unwrap();
        assert_eq!(opened, pt);

        // And what the vector is for: the same ciphertext does not open in base mode, under
        // another PSK, another psk_id, another info or another aad.
        let refused = |mode: &OpModeR<Kem>, info: &[u8], aad: &[u8]| {
            hpke::single_shot_open::<Aead, Kdf, Kem>(mode, &sk_r, &enc_key, info, &ct, aad).is_err()
        };
        assert!(refused(&OpModeR::Base, &info, &aad));
        let wrong_psk = [0u8; 32];
        assert!(refused(
            &OpModeR::Psk(PskBundle {
                psk: &wrong_psk,
                psk_id: &psk_id
            }),
            &info,
            &aad
        ));
        assert!(refused(
            &OpModeR::Psk(PskBundle {
                psk: &psk,
                psk_id: b"another"
            }),
            &info,
            &aad
        ));
        assert!(refused(&receiver_mode, b"another", &aad));
        assert!(refused(&receiver_mode, &info, b"another"));
    }
}
