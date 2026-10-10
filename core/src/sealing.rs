// SPDX-License-Identifier: Apache-2.0

//! Payload sealing (`spec/security.md` §14; `spec/interfaces.md` §6.10; #369): the
//! per-device X25519 agreement key, the agreement statement that binds it to the device key,
//! and the sealed frame every payload travels in on a sealing transport.
//!
//! Sealing adds confidentiality and nothing else (§14.1). A frame that opens proves nothing
//! about who sent it ([SEC-SEL-034]): the signed object inside is verified exactly as it is
//! without sealing, and an authenticated payload is still an untrusted instruction (§1.2).
//!
//! # Agreement keys (§14.2)
//!
//! An [`AgreementKeys`] holds this device's agreement private keys, newest first. Each private
//! key is 32 octets drawn from the operating system's CSPRNG ([SEC-SEL-002]), never derived
//! from the device key ([SEC-SEL-003]), and kept apart from it ([SEC-SEL-001]). It is stored
//! as the device key's seed is, behind a store seam ([`AgreementKeyStore`]; the reference
//! implementation's stores are the `oac` binary's, as for [`crate::keys::KeyStore`]), with the
//! highest `seq` this device has issued, as durably as the device key ([SEC-SEL-043]). A
//! private key never leaves this module except through [`AgreementPrivateKey::expose`], which a
//! store calls to write it in this process ([SEC-SEL-004]); no type here prints one.
//!
//! # Agreement statements (§14.3)
//!
//! [`AgreementStatement::issue`] signs a statement under `oac-agreement-v1` ([SEC-SEL-011]).
//! [`check_statement`] is a consumer's admission: exactly the four members ([SEC-SEL-010]), a
//! trusted signing key and a verifying signature ([SEC-SEL-012]), an acceptable agreement key
//! ([SEC-SEL-013]), `issued_at` earlier than the consumer's time plus `W` ([SEC-SEL-042]), and
//! a `seq` higher than the held one's ([SEC-SEL-014]). The consumer's held statements live in
//! [`crate::authorization::AuthorizationEngine`], beside the trusted key set they belong to,
//! and are kept and removed with it ([SEC-SEL-015], [SEC-SEL-041]).
//!
//! # Sealing and opening (§14.4, §14.5)
//!
//! HPKE (RFC 9180) base mode, single-shot: DHKEM(X25519, HKDF-SHA256), HKDF-SHA256,
//! ChaCha20Poly1305, `info` `oac-seal-v1`, empty `aad`, through the `hpke` crate 0.12.0. The
//! plaintext (Table 14.1) and the frame (Table 14.2) are built and checked here. Each frame
//! has a fresh ephemeral key from the operating system's CSPRNG ([SEC-SEL-021]); plaintext
//! buffers are zeroized when dropped. [`open`] fails closed: any failing check of §14.5 is
//! `None`, with nothing said about which.
//!
//! The direct `x25519-dalek` dependency enables `zeroize` for hpke's internal
//! `StaticSecret`, `EphemeralSecret` and `SharedSecret` drops.

use std::fmt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use hpke::aead::{AeadTag, ChaCha20Poly1305};
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem as KemTrait, OpModeR, OpModeS, Serializable};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::authorization::{AuthorizationEngine, Binding, SentRecord};
use crate::base64url;
use crate::canonical::SigningDomain;
use crate::ids::{KeyId, Timestamp};
use crate::json::{Json, JsonNumber, JsonObject};
use crate::keys::{DeviceIdentity, KeyStoreError, random_octets};
use crate::replay::REPLAY_WINDOW_MS;
use crate::signing::{VerifyError, verify_signed};
use crate::transport::PayloadKind;
use crate::trust::TrustedKeySet;

type Kem = X25519HkdfSha256;
type Kdf = HkdfSha256;
type Aead = ChaCha20Poly1305;

/// The HPKE `info` of every frame: the ASCII octets `oac-seal-v1` (§14.4).
pub const SEAL_INFO: &[u8] = b"oac-seal-v1";

/// The frame version octet (Table 14.2).
pub const FRAME_VERSION: u8 = 0x01;

/// The smallest frame: version, `enc`, kind, `L` and the AEAD tag, with an empty payload and
/// no padding (§14.4; [IFC-TRN-104]).
pub const FRAME_OVERHEAD: usize = 1 + ENC_LEN + PLAINTEXT_HEADER + TAG_LEN;

/// The padding granularity a sender pads each plaintext to ([SEC-SEL-025]).
pub const PADDING_BLOCK: usize = 256;

/// The highest `seq` an agreement statement may carry: 2^53 - 1 (§14.3).
pub const MAX_STATEMENT_SEQ: u64 = 9_007_199_254_740_991;

const ENC_LEN: usize = 32;
const TAG_LEN: usize = 16;
const PLAINTEXT_HEADER: usize = 5;
const WINDOW_NANOS: i128 = REPLAY_WINDOW_MS as i128 * 1_000_000;

/// The u-coordinates, below p = 2^255 - 19, of the points of small order on Curve25519 and
/// on its twist (§14.3, "acceptable"): 0 (order 2 on both), 1 (order 4 on the curve),
/// p - 1 (order 4 on the twist) and the two of order 8 on the curve. The curve's 8-torsion has
/// 8 points and the twist's 4-torsion 4 points; the identity has no u-coordinate, and these
/// five values cover the rest. `sealing::tests::small_order_list_is_exactly_the_torsion`
/// checks each one against `curve25519-dalek`.
const SMALL_ORDER: [[u8; 32]; 5] = [
    [0; 32],
    [
        1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ],
    [
        0xec, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0x7f,
    ],
    [
        0xe0, 0xeb, 0x7a, 0x7c, 0x3b, 0x41, 0xb8, 0xae, 0x16, 0x56, 0xe3, 0xfa, 0xf1, 0x9f, 0xc4,
        0x6a, 0xda, 0x09, 0x8d, 0xeb, 0x9c, 0x32, 0xb1, 0xfd, 0x86, 0x62, 0x05, 0x16, 0x5f, 0x49,
        0xb8, 0x00,
    ],
    [
        0x5f, 0x9c, 0x95, 0xbc, 0xa3, 0x50, 0x8c, 0x24, 0xb1, 0xd0, 0xb1, 0x55, 0x9c, 0x83, 0xef,
        0x5b, 0x04, 0x44, 0x5c, 0xc4, 0x58, 0x1c, 0x8e, 0x86, 0xd8, 0x22, 0x4e, 0xdd, 0xd0, 0x9f,
        0x11, 0x57,
    ],
];

/// Whether `octets` is an acceptable agreement key (§14.3): its most significant bit is zero,
/// its value as a little-endian integer is below 2^255 - 19, and its point has no small
/// order on the curve or on its twist ([SEC-SEL-013]).
pub fn is_acceptable_agreement_key(octets: &[u8; 32]) -> bool {
    if octets[31] & 0x80 != 0 {
        return false;
    }
    // With the top bit clear, u >= p exactly when u is one of p..=2^255-1: the top octet
    // 0x7f, octets 1..=30 all 0xff, and the low octet at least 0xed.
    if octets[31] == 0x7f && octets[1..31].iter().all(|&b| b == 0xff) && octets[0] >= 0xed {
        return false;
    }
    let mut small = subtle::Choice::from(0u8);
    for s in &SMALL_ORDER {
        small |= s.ct_eq(octets);
    }
    !bool::from(small)
}

/// An acceptable agreement public key (§14.3): the 32-octet u-coordinate.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AgreementPublicKey([u8; 32]);

impl AgreementPublicKey {
    /// The key whose u-coordinate is `octets`; `None` when it is not acceptable
    /// ([SEC-SEL-013]).
    pub fn from_octets(octets: [u8; 32]) -> Option<AgreementPublicKey> {
        is_acceptable_agreement_key(&octets).then_some(AgreementPublicKey(octets))
    }

    /// The key whose unpadded base64url encoding is `s`, under the rules of
    /// [`AgreementPublicKey::from_octets`].
    pub fn from_base64url(s: &str) -> Option<AgreementPublicKey> {
        AgreementPublicKey::from_octets(base64url::decode_exact::<32>(s)?)
    }

    /// The 32 octets.
    pub fn to_octets(&self) -> [u8; 32] {
        self.0
    }

    /// The unpadded base64url encoding.
    pub fn to_base64url(&self) -> String {
        base64url::encode(&self.0)
    }
}

impl fmt::Debug for AgreementPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AgreementPublicKey({})", self.to_base64url())
    }
}

/// An agreement private key's 32 octets, while a store reads or writes it. Zeroized on drop;
/// no `Clone`, and its `Debug` shows nothing of the key ([SEC-SEL-004]).
pub struct AgreementPrivateKey(Zeroizing<[u8; 32]>);

impl AgreementPrivateKey {
    /// The key whose octets are in `octets`, as read back from a store; `octets` is
    /// zeroized, so no plain copy outlives the call.
    pub fn from_octets(octets: &mut [u8; 32]) -> AgreementPrivateKey {
        let key = AgreementPrivateKey(Zeroizing::new(*octets));
        zeroize::Zeroize::zeroize(octets);
        key
    }

    /// The octets, for an [`AgreementKeyStore`] to write in this process. No other caller
    /// has a reason to read them ([SEC-SEL-004]).
    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }

    /// A second zeroizing copy.
    pub fn duplicate(&self) -> AgreementPrivateKey {
        let mut copy = Zeroizing::new([0u8; 32]);
        copy.copy_from_slice(self.0.as_slice());
        AgreementPrivateKey(copy)
    }

    fn ct_eq(&self, other: &AgreementPrivateKey) -> bool {
        self.0.as_slice().ct_eq(other.0.as_slice()).into()
    }
}

impl fmt::Debug for AgreementPrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AgreementPrivateKey(..)")
    }
}

/// The public key of the private key `octets` (RFC 7748 §5, which clamps it).
fn public_of(octets: &[u8; 32]) -> Option<(<Kem as KemTrait>::PrivateKey, [u8; 32])> {
    let sk = <Kem as KemTrait>::PrivateKey::from_bytes(octets).ok()?;
    let pk = <Kem as KemTrait>::sk_to_pk(&sk).to_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(pk.as_slice());
    Some((sk, out))
}

/// The operating system's CSPRNG, for the `hpke` crate's ephemeral keys ([SEC-SEL-021]).
struct OsRng;

impl hpke::rand_core::RngCore for OsRng {
    fn next_u32(&mut self) -> u32 {
        hpke::rand_core::impls::next_u32_via_fill(self)
    }

    fn next_u64(&mut self) -> u64 {
        hpke::rand_core::impls::next_u64_via_fill(self)
    }

    /// # Panics
    ///
    /// When the operating system has no random source, as [`crate::keys`] does: no frame
    /// may then be sealed.
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        getrandom::fill(dest).expect("the operating system's random number generator");
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), hpke::rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl hpke::rand_core::CryptoRng for OsRng {}

// ---- the frame (§14.4, §14.5) ------------------------------------------------------------

/// The kind octet of Table 14.1; `None` for `sealed`, which is not sealed again.
fn kind_octet(kind: PayloadKind) -> Option<u8> {
    match kind {
        PayloadKind::Envelope => Some(0x01),
        PayloadKind::Presence => Some(0x02),
        PayloadKind::Receipt => Some(0x03),
        PayloadKind::Sealed => None,
    }
}

fn kind_of(octet: u8) -> Option<PayloadKind> {
    match octet {
        0x01 => Some(PayloadKind::Envelope),
        0x02 => Some(PayloadKind::Presence),
        0x03 => Some(PayloadKind::Receipt),
        _ => None,
    }
}

/// Why a payload was not sealed. Nothing was passed to a transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealError {
    /// The payload is of kind `sealed`; only the three kinds of Table 14.1 are sealed.
    NotSealable,
    /// The frame would be longer than the transport carries, even unpadded.
    TooLarge,
    /// The recipient's key gives an all-zero shared secret, or the AEAD failed. An
    /// acceptable key never does.
    Hpke,
}

impl fmt::Display for SealError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SealError::NotSealable => "a sealed payload is not sealed again",
            SealError::TooLarge => "the frame would exceed the transport's payload limit",
            SealError::Hpke => "sealing failed",
        })
    }
}

impl std::error::Error for SealError {}

/// The padding a sender adds to a payload of `payload_len` octets ([SEC-SEL-025]): enough to
/// make the plaintext a multiple of [`PADDING_BLOCK`] octets, or none when that frame would be
/// longer than `max_frame_octets`. `None` when even the unpadded frame would be.
pub fn padding_for(payload_len: usize, max_frame_octets: u64) -> Option<usize> {
    let plain = payload_len.checked_add(PLAINTEXT_HEADER)?;
    let frame = |p: usize| p.checked_add(1 + ENC_LEN + TAG_LEN);
    let fits = |p: usize| frame(p).and_then(|f| u64::try_from(f).ok()) <= Some(max_frame_octets);
    let padded = plain.div_ceil(PADDING_BLOCK).checked_mul(PADDING_BLOCK)?;
    if fits(padded) {
        Some(padded - plain)
    } else if fits(plain) {
        Some(0)
    } else {
        None
    }
}

/// Seals `payload`, of `kind`, to `recipient` ([SEC-SEL-020]), padded under
/// [`padding_for`] for a transport whose `max_payload_octets` is `max_frame_octets`.
///
/// `payload` is the payload's octets exactly as signed and serialized, so the signature is
/// inside the frame ([SEC-SEL-022]). The caller takes `recipient` only from an agreement
/// statement it admitted for the recipient's key id ([SEC-SEL-023]).
///
/// # Errors
///
/// [`SealError`]; nothing is to be passed to the transport then.
pub fn seal(
    recipient: &AgreementPublicKey,
    kind: PayloadKind,
    payload: &[u8],
    max_frame_octets: u64,
) -> Result<Vec<u8>, SealError> {
    let padding = padding_for(payload.len(), max_frame_octets).ok_or(SealError::TooLarge)?;
    seal_padded(recipient, kind, payload, padding)
}

/// [`seal`] with an explicit number of padding octets.
///
/// # Errors
///
/// [`SealError`].
pub fn seal_padded(
    recipient: &AgreementPublicKey,
    kind: PayloadKind,
    payload: &[u8],
    padding: usize,
) -> Result<Vec<u8>, SealError> {
    let k = kind_octet(kind).ok_or(SealError::NotSealable)?;
    let len = u32::try_from(payload.len()).map_err(|_| SealError::TooLarge)?;
    let total = payload
        .len()
        .checked_add(PLAINTEXT_HEADER)
        .and_then(|n| n.checked_add(padding))
        .ok_or(SealError::TooLarge)?;
    // Table 14.1, in a buffer that is zeroized when dropped; it becomes the ciphertext.
    let mut buf = Zeroizing::new(Vec::with_capacity(total));
    buf.push(k);
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(payload);
    buf.resize(total, 0);
    let pk = <Kem as KemTrait>::PublicKey::from_bytes(&recipient.0).map_err(|_| SealError::Hpke)?;
    let (enc, tag) = hpke::single_shot_seal_in_place_detached::<Aead, Kdf, Kem, _>(
        &OpModeS::Base,
        &pk,
        SEAL_INFO,
        buf.as_mut_slice(),
        &[],
        &mut OsRng,
    )
    .map_err(|_| SealError::Hpke)?;
    // Table 14.2.
    let mut frame = Vec::with_capacity(1 + ENC_LEN + total + TAG_LEN);
    frame.push(FRAME_VERSION);
    frame.extend_from_slice(enc.to_bytes().as_slice());
    frame.extend_from_slice(buf.as_slice());
    frame.extend_from_slice(tag.to_bytes().as_slice());
    Ok(frame)
}

/// A payload that a frame opened to: its kind and its octets. Opening proves nothing about
/// who sent it ([SEC-SEL-034]). The octets are zeroized when dropped.
pub struct Opened {
    /// The kind its kind octet names ([SEC-SEL-032]).
    pub kind: PayloadKind,
    /// The payload's octets, exactly as the sender signed and serialized them.
    pub payload: Zeroizing<Vec<u8>>,
}

impl fmt::Debug for Opened {
    /// The kind and length only: the octets are content.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Opened")
            .field("kind", &self.kind)
            .field("len", &self.payload.len())
            .finish()
    }
}

/// Opens `frame` under one private key: `Some` exactly when every condition of §14.5 holds
/// (length, version, HPKE open, kind octet, `L`, zero padding). Fails closed, and says
/// nothing about which condition failed.
fn open_under(sk: &<Kem as KemTrait>::PrivateKey, frame: &[u8]) -> Option<Opened> {
    // 1 and 2.
    if frame.len() < FRAME_OVERHEAD || frame[0] != FRAME_VERSION {
        return None;
    }
    let enc = <Kem as KemTrait>::EncappedKey::from_bytes(&frame[1..1 + ENC_LEN]).ok()?;
    let body = &frame[1 + ENC_LEN..];
    let (ct, tag) = body.split_at(body.len() - TAG_LEN);
    let tag = AeadTag::<Aead>::from_bytes(tag).ok()?;
    // 3: decrypted in a buffer that is zeroized when dropped.
    let mut pt = Zeroizing::new(ct.to_vec());
    hpke::single_shot_open_in_place_detached::<Aead, Kdf, Kem>(
        &OpModeR::Base,
        sk,
        &enc,
        SEAL_INFO,
        pt.as_mut_slice(),
        &[],
        &tag,
    )
    .ok()?;
    // 4.
    let kind = kind_of(pt[0])?;
    // 5.
    let mut l = [0u8; 4];
    l.copy_from_slice(&pt[1..PLAINTEXT_HEADER]);
    let l = usize::try_from(u32::from_be_bytes(l)).ok()?;
    if l > pt.len() - PLAINTEXT_HEADER {
        return None;
    }
    // 6.
    let end = PLAINTEXT_HEADER + l;
    if pt[end..].iter().fold(0u8, |a, &b| a | b) != 0 {
        return None;
    }
    let mut payload = Zeroizing::new(Vec::with_capacity(l));
    payload.extend_from_slice(&pt[PLAINTEXT_HEADER..end]);
    Some(Opened { kind, payload })
}

/// Opens `frame` with the agreement private key `key` alone (§14.5); [`AgreementKeys::open`]
/// is the receiver's path, which tries every key it holds.
pub fn open(key: &AgreementPrivateKey, frame: &[u8]) -> Option<Opened> {
    let (sk, _) = public_of(key.expose())?;
    open_under(&sk, frame)
}

// ---- the recipient of an opened payload (§14.4, [SEC-SEL-035]) ----------------------------

/// Whom an opened payload names as its recipient (§14.4), as far as it names one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recipient {
    /// This device.
    Own,
    /// Another device: the payload is dropped silently ([SEC-SEL-035]).
    Another,
    /// None: an envelope whose `to` is unbound or under conflict, or octets that name
    /// nothing. The payload goes on to its own checks, which report on it as without
    /// sealing.
    Unnamed,
}

/// The sent record ([SEC-AUZ-013]) of the envelope an authenticated receipt's octets
/// describe: the one whose `id`, `from`, `to` and nonce the receipt names ([SEC-RCT-003]
/// item 3). `None` when the octets name none, or this device sent no such envelope.
pub fn sent_record_for_receipt<'e>(
    octets: &[u8],
    engine: &'e AuthorizationEngine,
) -> Option<&'e SentRecord> {
    let parsed = crate::json::parse(octets).ok()?;
    let o = parsed.as_object()?;
    let s = |m: &str| o.get(m).and_then(Json::as_str);
    let inner = o.get("receipt").and_then(Json::as_object)?;
    let r = |m: &str| inner.get(m).and_then(Json::as_str);
    let (id, from, to, nonce) = (
        r("envelope_id")?,
        r("envelope_from")?,
        s("envelope_to")?,
        s("envelope_nonce")?,
    );
    let from = crate::ids::SessionId::parse(from)?;
    engine.sent_records_from(&from).find(|rec| {
        rec.id.as_str() == id && rec.to.as_str() == to && rec.nonce.as_deref() == Some(nonce)
    })
}

/// The recipient an opened payload of `kind` names (§14.4), read against `engine`, the
/// receiver's binding table and sent records:
///
/// - an envelope: the key its binding table binds `to` to; an unbound `to`, or one under
///   conflict, names none;
/// - an authenticated presence record: its `audience`;
/// - an authenticated receipt: this device exactly when it holds a record of sending the
///   envelope the receipt describes ([SEC-RCT-003] item 3), another otherwise.
///
/// The check of [SEC-SEL-035] runs on this before any other check of the payload.
pub fn recipient_of(kind: PayloadKind, octets: &[u8], engine: &AuthorizationEngine) -> Recipient {
    let own = engine.own_key_id();
    match kind {
        PayloadKind::Receipt => match sent_record_for_receipt(octets, engine) {
            Some(_) => Recipient::Own,
            None => Recipient::Another,
        },
        PayloadKind::Envelope | PayloadKind::Presence => {
            let Some(parsed) = crate::json::parse(octets).ok() else {
                return Recipient::Unnamed;
            };
            let Some(o) = parsed.as_object() else {
                return Recipient::Unnamed;
            };
            let named = if kind == PayloadKind::Envelope {
                let to = o
                    .get("to")
                    .and_then(Json::as_str)
                    .and_then(crate::ids::SessionId::parse);
                match to.and_then(|t| engine.binding(&t).cloned()) {
                    Some(Binding::Key(k)) => Some(k.as_str().to_owned()),
                    _ => None,
                }
            } else {
                o.get("audience").and_then(Json::as_str).map(str::to_owned)
            };
            match named {
                Some(k) if k == own.as_str() => Recipient::Own,
                Some(_) => Recipient::Another,
                None => Recipient::Unnamed,
            }
        }
        PayloadKind::Sealed => Recipient::Unnamed,
    }
}

// ---- agreement statements (§14.3) --------------------------------------------------------

const STATEMENT_MEMBERS: [&str; 4] = ["agreement_key", "seq", "issued_at", "security"];

/// An agreement statement (§14.3). It keeps the object as received or built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgreementStatement {
    wire: JsonObject,
}

impl AgreementStatement {
    /// The statement that `key` is `identity`'s agreement key, with `seq` and `issued_at`,
    /// signed with the device key under `oac-agreement-v1` ([SEC-SEL-011]). `None` when `seq`
    /// is above [`MAX_STATEMENT_SEQ`].
    pub fn issue(
        identity: &DeviceIdentity,
        key: &AgreementPublicKey,
        seq: u64,
        issued_at: &Timestamp,
    ) -> Option<AgreementStatement> {
        if seq > MAX_STATEMENT_SEQ {
            return None;
        }
        let mut o = JsonObject::new();
        o.insert("agreement_key", key.to_base64url().as_str().into());
        o.insert("seq", Json::Number(JsonNumber::from_u64(seq)));
        o.insert("issued_at", issued_at.as_str().into());
        let mut sec = JsonObject::new();
        sec.insert("principal", identity.principal().as_str().into());
        sec.insert("key_id", identity.key_id().as_str().into());
        o.insert("security", Json::Object(sec.clone()));
        let signature = identity
            .sign_object(SigningDomain::Agreement, &o)
            .expect("a statement holds strings and an integer only, so it canonicalizes");
        sec.insert("signature", signature.as_str().into());
        o.insert("security", Json::Object(sec));
        Some(AgreementStatement { wire: o })
    }

    /// Wraps a received value. Any object is kept; [`check_statement`] checks it.
    pub fn from_json(v: &Json) -> Option<AgreementStatement> {
        Some(AgreementStatement {
            wire: v.as_object()?.clone(),
        })
    }

    /// The object as held.
    pub fn as_json(&self) -> &JsonObject {
        &self.wire
    }
}

/// An agreement statement a consumer admitted and holds for one device key (§14.3): what a
/// sender seals to ([SEC-SEL-023]), and what a store keeps ([SEC-SEL-041]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldStatement {
    key_id: KeyId,
    agreement_key: AgreementPublicKey,
    seq: u64,
    issued_at: Timestamp,
}

impl HeldStatement {
    /// A held statement read back from a store. Only a store implementation calls this, for a
    /// statement admitted earlier. `None` when `seq` is above [`MAX_STATEMENT_SEQ`]; the key
    /// was checked when its [`AgreementPublicKey`] was built.
    pub fn from_store(
        key_id: KeyId,
        agreement_key: AgreementPublicKey,
        seq: u64,
        issued_at: Timestamp,
    ) -> Option<HeldStatement> {
        (seq <= MAX_STATEMENT_SEQ).then_some(HeldStatement {
            key_id,
            agreement_key,
            seq,
            issued_at,
        })
    }

    /// The device key it belongs to.
    pub fn key_id(&self) -> &KeyId {
        &self.key_id
    }

    /// The agreement key.
    pub fn agreement_key(&self) -> &AgreementPublicKey {
        &self.agreement_key
    }

    /// `seq`.
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// `issued_at`.
    pub fn issued_at(&self) -> &Timestamp {
        &self.issued_at
    }
}

/// Why a consumer refused an agreement statement. Nothing changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatementRefusal {
    /// Not exactly the members of §14.3 with their types ([SEC-SEL-010]).
    Malformed,
    /// No trusted key, or the signature does not verify under `oac-agreement-v1`
    /// ([SEC-SEL-012]).
    Signature,
    /// The agreement key is not acceptable ([SEC-SEL-013]).
    UnacceptableKey,
    /// `issued_at` is not earlier than the consumer's time plus `W` ([SEC-SEL-042]).
    TooFarAhead,
    /// A statement with the same or a higher `seq` is held for that key ([SEC-SEL-014]).
    NotNewer,
    /// The statement was admissible but the store did not save it, so it was not admitted
    /// (fail closed; [SEC-SEL-041]).
    NotSaved(crate::pairing::PairingStoreError),
}

impl fmt::Display for StatementRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StatementRefusal::Malformed => f.write_str("agreement statement malformed"),
            StatementRefusal::Signature => {
                f.write_str("agreement statement not signed by a trusted key")
            }
            StatementRefusal::UnacceptableKey => f.write_str("agreement key not acceptable"),
            StatementRefusal::TooFarAhead => {
                f.write_str("agreement statement issued too far in the future")
            }
            StatementRefusal::NotNewer => {
                f.write_str("an agreement statement with the same or a higher seq is held")
            }
            StatementRefusal::NotSaved(e) => write!(f, "agreement statement not saved: {e}"),
        }
    }
}

impl std::error::Error for StatementRefusal {}

/// A consumer's admission of `statement` at its time `now` (§14.3), against its trusted key
/// set and `held`, the statement it holds for the signing key, if any. On success, the
/// statement to hold in place of `held`.
///
/// # Errors
///
/// [`StatementRefusal`], the first failing check.
pub fn check_statement(
    statement: &AgreementStatement,
    trusted: &TrustedKeySet,
    held: impl FnOnce(&KeyId) -> Option<HeldStatement>,
    now: &Timestamp,
) -> Result<HeldStatement, StatementRefusal> {
    let o = &statement.wire;
    // [SEC-SEL-010]: exactly the four members.
    if o.len() != STATEMENT_MEMBERS.len() || STATEMENT_MEMBERS.iter().any(|m| !o.contains(m)) {
        return Err(StatementRefusal::Malformed);
    }
    // [SEC-SEL-012]: `verify_signed` also refuses a `security` other than exactly
    // `principal`, `key_id` and `signature` ([SEC-SEL-010]).
    let key_id = match verify_signed(trusted, SigningDomain::Agreement, o) {
        Ok(entry) => entry.key_id().clone(),
        Err(VerifyError::MalformedSecurity) => return Err(StatementRefusal::Malformed),
        Err(_) => return Err(StatementRefusal::Signature),
    };
    // [SEC-SEL-013].
    let agreement_key = match o.get("agreement_key").and_then(Json::as_str) {
        Some(s) => {
            AgreementPublicKey::from_base64url(s).ok_or(StatementRefusal::UnacceptableKey)?
        }
        None => return Err(StatementRefusal::Malformed),
    };
    // [SEC-SEL-010]: the types of `issued_at` and `seq`.
    let issued_at = o
        .get("issued_at")
        .and_then(Json::as_str)
        .and_then(Timestamp::parse)
        .ok_or(StatementRefusal::Malformed)?;
    let seq = o
        .get("seq")
        .and_then(Json::as_number)
        .and_then(|n| n.plain_integer_in(0, MAX_STATEMENT_SEQ))
        .ok_or(StatementRefusal::Malformed)?;
    // [SEC-SEL-042]: refused unless earlier than now + W.
    if issued_at.unix_nanos() >= now.unix_nanos() + WINDOW_NANOS {
        return Err(StatementRefusal::TooFarAhead);
    }
    // [SEC-SEL-014]: by `seq`, never by `issued_at`.
    if held(&key_id).is_some_and(|h| h.seq >= seq) {
        return Err(StatementRefusal::NotNewer);
    }
    Ok(HeldStatement {
        key_id,
        agreement_key,
        seq,
        issued_at,
    })
}

// ---- this device's agreement keys (§14.2, §14.3, §14.9) ----------------------------------

/// One stored agreement key: the private key and the `seq` and `issued_at` of its statement.
pub struct StoredAgreementKey {
    /// The private key.
    pub private_key: AgreementPrivateKey,
    /// Its statement's `seq`.
    pub seq: u64,
    /// Its statement's `issued_at`.
    pub issued_at: Timestamp,
}

impl StoredAgreementKey {
    fn duplicate(&self) -> StoredAgreementKey {
        StoredAgreementKey {
            private_key: self.private_key.duplicate(),
            seq: self.seq,
            issued_at: self.issued_at.clone(),
        }
    }
}

impl fmt::Debug for StoredAgreementKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredAgreementKey")
            .field("seq", &self.seq)
            .field("issued_at", &self.issued_at)
            .finish_non_exhaustive()
    }
}

/// What an [`AgreementKeyStore`] keeps: the agreement keys this device still opens frames
/// with, and the highest `seq` it has issued ([SEC-SEL-043]), which outlives a key erased
/// under [SEC-SEL-018].
#[derive(Debug, Default)]
pub struct AgreementKeyState {
    /// The keys.
    pub keys: Vec<StoredAgreementKey>,
    /// The highest `seq` issued; `None` before the first statement.
    pub highest_seq: Option<u64>,
}

impl AgreementKeyState {
    /// A second copy, with zeroizing copies of the keys.
    pub fn duplicate(&self) -> AgreementKeyState {
        AgreementKeyState {
            keys: self
                .keys
                .iter()
                .map(StoredAgreementKey::duplicate)
                .collect(),
            highest_seq: self.highest_seq,
        }
    }
}

/// Where an implementation keeps its agreement keys and its highest issued `seq`, as durably
/// as its device key ([SEC-SEL-043]; §14.2 reference implementation note: beside the device
/// key's seed). A store holds this device's own keys only.
pub trait AgreementKeyStore {
    /// The stored state, or `None` when nothing was saved yet.
    ///
    /// # Errors
    ///
    /// [`KeyStoreError`] when the store cannot be read.
    fn load(&self) -> Result<Option<AgreementKeyState>, KeyStoreError>;

    /// Replaces the stored state with `state`, as one write.
    ///
    /// # Errors
    ///
    /// [`KeyStoreError`] when the store cannot be written.
    fn save(&self, state: &AgreementKeyState) -> Result<(), KeyStoreError>;
}

/// An [`AgreementKeyStore`] in this process's memory: nothing outlives the process. The test
/// double; [`MemoryAgreementKeyStore::fail_saves`] makes its writes fail.
#[derive(Debug, Default)]
pub struct MemoryAgreementKeyStore {
    state: Mutex<Option<AgreementKeyState>>,
    fail: AtomicBool,
}

impl MemoryAgreementKeyStore {
    /// An empty store.
    pub fn new() -> MemoryAgreementKeyStore {
        MemoryAgreementKeyStore::default()
    }

    /// Makes every later save fail (`true`) or succeed (`false`).
    pub fn fail_saves(&self, fail: bool) {
        self.fail.store(fail, Ordering::SeqCst);
    }
}

impl AgreementKeyStore for MemoryAgreementKeyStore {
    fn load(&self) -> Result<Option<AgreementKeyState>, KeyStoreError> {
        let state = self
            .state
            .lock()
            .map_err(|_| KeyStoreError::Failed("lock poisoned".into()))?;
        Ok(state.as_ref().map(AgreementKeyState::duplicate))
    }

    fn save(&self, state: &AgreementKeyState) -> Result<(), KeyStoreError> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(KeyStoreError::Failed("save refused (test double)".into()));
        }
        let mut slot = self
            .state
            .lock()
            .map_err(|_| KeyStoreError::Failed("lock poisoned".into()))?;
        *slot = Some(state.duplicate());
        Ok(())
    }
}

/// Why this device's agreement keys could not be loaded, made or replaced. Never holds key
/// material.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgreementKeyError {
    /// The store failed. Nothing new was issued.
    Store(KeyStoreError),
    /// The store holds a key that is not a usable agreement private key, or a key whose
    /// `seq` is above the highest it records.
    Corrupt,
    /// The highest `seq` is [`MAX_STATEMENT_SEQ`]: no later statement can be issued, and the
    /// key can be replaced only by pairing again with a new device key.
    SeqExhausted,
    /// The saved state did not read back.
    ReadBack,
}

impl fmt::Display for AgreementKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgreementKeyError::Store(e) => e.fmt(f),
            AgreementKeyError::Corrupt => f.write_str("the stored agreement keys are unusable"),
            AgreementKeyError::SeqExhausted => {
                f.write_str("no agreement statement can be issued after the highest seq")
            }
            AgreementKeyError::ReadBack => {
                f.write_str("the saved agreement keys did not read back")
            }
        }
    }
}

impl std::error::Error for AgreementKeyError {}

/// One of this device's agreement keys, with its statement.
struct OwnKey {
    private_key: AgreementPrivateKey,
    hpke_key: <Kem as KemTrait>::PrivateKey,
    seq: u64,
    issued_at: Timestamp,
    statement: AgreementStatement,
}

/// This device's agreement keys (§14.2): every key it still opens frames with, newest first by
/// the `issued_at` of their statements ([SEC-SEL-037]), and the highest `seq` it has issued.
/// The first is the current key, whose statement it gives to its peers. Its `Debug` shows
/// counts only.
pub struct AgreementKeys {
    keys: Vec<OwnKey>,
    highest_seq: u64,
}

impl fmt::Debug for AgreementKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgreementKeys")
            .field("keys", &self.keys.len())
            .field("highest_seq", &self.highest_seq)
            .finish()
    }
}

/// A fresh agreement private key from the operating system's CSPRNG ([SEC-SEL-002]), drawn
/// independently of the device key ([SEC-SEL-003]). An acceptable public key is all but
/// certain; a draw that is not is drawn again.
fn fresh_private_key() -> AgreementPrivateKey {
    loop {
        let octets = random_octets::<32>();
        if public_of(&octets).is_some_and(|(_, pk)| is_acceptable_agreement_key(&pk)) {
            return AgreementPrivateKey(octets);
        }
    }
}

impl AgreementKeys {
    fn own_key(
        identity: &DeviceIdentity,
        stored: &StoredAgreementKey,
    ) -> Result<OwnKey, AgreementKeyError> {
        let (hpke_key, pk) =
            public_of(stored.private_key.expose()).ok_or(AgreementKeyError::Corrupt)?;
        let public = AgreementPublicKey::from_octets(pk).ok_or(AgreementKeyError::Corrupt)?;
        let statement = AgreementStatement::issue(identity, &public, stored.seq, &stored.issued_at)
            .ok_or(AgreementKeyError::Corrupt)?;
        Ok(OwnKey {
            private_key: stored.private_key.duplicate(),
            hpke_key,
            seq: stored.seq,
            issued_at: stored.issued_at.clone(),
            statement,
        })
    }

    fn from_state(
        identity: &DeviceIdentity,
        state: &AgreementKeyState,
    ) -> Result<AgreementKeys, AgreementKeyError> {
        let highest_seq = state.highest_seq.ok_or(AgreementKeyError::Corrupt)?;
        let mut keys = Vec::with_capacity(state.keys.len());
        for k in &state.keys {
            if k.seq > highest_seq {
                return Err(AgreementKeyError::Corrupt);
            }
            keys.push(AgreementKeys::own_key(identity, k)?);
        }
        if keys.is_empty() {
            return Err(AgreementKeyError::Corrupt);
        }
        // [SEC-SEL-037]: newest first by `issued_at`; `seq` breaks a tie.
        keys.sort_by(|a, b| {
            b.issued_at
                .unix_nanos()
                .cmp(&a.issued_at.unix_nanos())
                .then(b.seq.cmp(&a.seq))
        });
        Ok(AgreementKeys { keys, highest_seq })
    }

    fn state(&self) -> AgreementKeyState {
        AgreementKeyState {
            keys: self
                .keys
                .iter()
                .map(|k| StoredAgreementKey {
                    private_key: k.private_key.duplicate(),
                    seq: k.seq,
                    issued_at: k.issued_at.clone(),
                })
                .collect(),
            highest_seq: Some(self.highest_seq),
        }
    }

    /// Saves `state` to `store` and reads it back, so a store that dropped the write fails
    /// now rather than at the next start with a lower `seq` ([SEC-SEL-043]).
    fn save_checked(
        store: &dyn AgreementKeyStore,
        state: &AgreementKeyState,
    ) -> Result<(), AgreementKeyError> {
        store.save(state).map_err(AgreementKeyError::Store)?;
        let back = store
            .load()
            .map_err(AgreementKeyError::Store)?
            .ok_or(AgreementKeyError::ReadBack)?;
        let same = back.highest_seq == state.highest_seq
            && back.keys.len() == state.keys.len()
            && back.keys.iter().zip(&state.keys).all(|(a, b)| {
                a.seq == b.seq && a.issued_at == b.issued_at && a.private_key.ct_eq(&b.private_key)
            });
        if same {
            Ok(())
        } else {
            Err(AgreementKeyError::ReadBack)
        }
    }

    /// The stored agreement keys, or, when `store` holds none, a new one whose statement has
    /// `seq` 0 and `issued_at` `now`, saved to `store` before it is used ([SEC-SEL-043]).
    /// Statements are signed with `identity`'s device key ([SEC-SEL-011]).
    ///
    /// # Errors
    ///
    /// [`AgreementKeyError`].
    pub fn load_or_generate(
        store: &dyn AgreementKeyStore,
        identity: &DeviceIdentity,
        now: &Timestamp,
    ) -> Result<AgreementKeys, AgreementKeyError> {
        match store.load().map_err(AgreementKeyError::Store)? {
            Some(state) if !state.keys.is_empty() => AgreementKeys::from_state(identity, &state),
            Some(state) => {
                // Every key was erased; the count stays.
                let seq = AgreementKeys::next_seq(state.highest_seq)?;
                AgreementKeys::generate(store, identity, Vec::new(), seq, now)
            }
            None => AgreementKeys::generate(store, identity, Vec::new(), 0, now),
        }
    }

    fn next_seq(highest: Option<u64>) -> Result<u64, AgreementKeyError> {
        match highest {
            None => Ok(0),
            Some(h) if h >= MAX_STATEMENT_SEQ => Err(AgreementKeyError::SeqExhausted),
            Some(h) => Ok(h + 1),
        }
    }

    fn generate(
        store: &dyn AgreementKeyStore,
        identity: &DeviceIdentity,
        mut keys: Vec<StoredAgreementKey>,
        seq: u64,
        now: &Timestamp,
    ) -> Result<AgreementKeys, AgreementKeyError> {
        keys.insert(
            0,
            StoredAgreementKey {
                private_key: fresh_private_key(),
                seq,
                issued_at: now.clone(),
            },
        );
        let state = AgreementKeyState {
            keys,
            highest_seq: Some(seq),
        };
        // [SEC-SEL-043]: the new `seq` is durable before its statement exists.
        AgreementKeys::save_checked(store, &state)?;
        AgreementKeys::from_state(identity, &state)
    }

    /// Replaces the current agreement key with a new one ([SEC-SEL-017]), whose statement has
    /// the next `seq` ([SEC-SEL-040]) and `issued_at` `now`. The new key and count are saved
    /// before the statement is returned ([SEC-SEL-043]). The replaced keys stay, so frames
    /// sealed to them by peers that have not yet admitted the new statement still open
    /// (§14.9); [`AgreementKeys::erase_replaced`] erases them.
    ///
    /// # Errors
    ///
    /// [`AgreementKeyError`]; nothing changes then.
    pub fn replace(
        &mut self,
        store: &dyn AgreementKeyStore,
        identity: &DeviceIdentity,
        now: &Timestamp,
    ) -> Result<&AgreementStatement, AgreementKeyError> {
        let seq = AgreementKeys::next_seq(Some(self.highest_seq))?;
        let state = self.state();
        *self = AgreementKeys::generate(store, identity, state.keys, seq, now)?;
        Ok(self.statement())
    }

    /// Erases every key but the current one ([SEC-SEL-018]), from `store` first and then
    /// from memory. The highest `seq` stays. Returns how many were erased.
    ///
    /// # Errors
    ///
    /// [`AgreementKeyError`]; nothing changes then.
    pub fn erase_replaced(
        &mut self,
        store: &dyn AgreementKeyStore,
    ) -> Result<usize, AgreementKeyError> {
        let current = self
            .keys
            .iter()
            .position(|k| k.seq == self.highest_seq)
            .unwrap_or(0);
        let mut state = self.state();
        let kept = state.keys.swap_remove(current);
        let erased = state.keys.len();
        state.keys = vec![kept];
        AgreementKeys::save_checked(store, &state)?;
        let kept = self.keys.swap_remove(current);
        self.keys = vec![kept];
        Ok(erased)
    }

    /// The statement of the current key: the one with the highest `seq`.
    pub fn statement(&self) -> &AgreementStatement {
        let current = self
            .keys
            .iter()
            .find(|k| k.seq == self.highest_seq)
            .unwrap_or(&self.keys[0]);
        &current.statement
    }

    /// Every statement held, newest first by `issued_at`.
    pub fn statements(&self) -> impl Iterator<Item = &AgreementStatement> {
        self.keys.iter().map(|k| &k.statement)
    }

    /// The highest `seq` issued ([SEC-SEL-040], [SEC-SEL-043]).
    pub fn highest_seq(&self) -> u64 {
        self.highest_seq
    }

    /// How many agreement keys are held.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Always false: at least one key is held.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Opens `frame` under the first key it opens under, trying the keys in descending
    /// order of their statements' `issued_at` ([SEC-SEL-037]). `None` when it opens under
    /// none ([SEC-SEL-030]).
    pub fn open(&self, frame: &[u8]) -> Option<Opened> {
        self.keys
            .iter()
            .find_map(|k| open_under(&k.hpke_key, frame))
    }

    /// Builds the keys from `stored`, for tests that need fixed private keys.
    #[cfg(test)]
    pub(crate) fn for_tests(
        identity: &DeviceIdentity,
        stored: &[StoredAgreementKey],
    ) -> AgreementKeys {
        let state = AgreementKeyState {
            keys: stored.iter().map(StoredAgreementKey::duplicate).collect(),
            highest_seq: stored.iter().map(|k| k.seq).max(),
        };
        AgreementKeys::from_state(identity, &state).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::Token;
    use crate::keys::DeviceKey;
    use curve25519_dalek::montgomery::MontgomeryPoint;
    use curve25519_dalek::scalar::Scalar;

    fn identity(p: &str) -> DeviceIdentity {
        DeviceIdentity::new(DeviceKey::generate(), Token::parse(p).unwrap())
    }

    fn ts(s: &str) -> Timestamp {
        Timestamp::parse(s).unwrap()
    }

    fn hex32(s: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, o) in out.iter_mut().enumerate() {
            *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
        }
        out
    }

    fn private(hex: &str) -> AgreementPrivateKey {
        AgreementPrivateKey::from_octets(&mut hex32(hex))
    }

    /// The test agreement keys of `tests/protocol/sec-test-keys.json` (test keys only).
    const BOB_PRIVATE: &str = "b62c69c556aa1ede2897d259922d81d71c4e530aa0fcdb729d8b5ab936c89f84";
    const BOB_PUBLIC: &str = "wcSSzJLhvIMKmwB60NLYtLZcibdrU5_XcugIsJugBG8";
    const CAROL_PRIVATE: &str = "89ed0b28241c49ab7ed3f327aa5e4ef178539ce6de59891b793c71fa3f1bc9a0";

    fn trusted(of: &[&DeviceIdentity]) -> TrustedKeySet {
        let mut set = TrustedKeySet::new(&identity("own"));
        for i in of {
            set.add_paired_key(i.principal().clone(), *i.public_key())
                .unwrap();
        }
        set
    }

    #[test]
    fn test_key_public_matches_the_key_file() {
        let (_, pk) = public_of(&hex32(BOB_PRIVATE)).unwrap();
        assert_eq!(base64url::encode(&pk), BOB_PUBLIC);
        assert!(AgreementPublicKey::from_base64url(BOB_PUBLIC).is_some());
    }

    #[test]
    fn small_order_list_is_exactly_the_torsion() {
        // Each listed u, times 8 (any multiple of the cofactor), is the identity: small order
        // on the curve or the twist. The Montgomery ladder works on both.
        let eight = Scalar::from(8u8);
        for u in &SMALL_ORDER {
            assert_eq!((MontgomeryPoint(*u) * eight).to_bytes(), [0u8; 32], "{u:?}");
            assert!(!is_acceptable_agreement_key(u));
        }
        // And a key that is not on the list does not vanish: the basepoint, and a generated
        // key.
        let base = curve25519_dalek::constants::X25519_BASEPOINT;
        assert_ne!((base * eight).to_bytes(), [0u8; 32]);
        assert!(is_acceptable_agreement_key(&base.to_bytes()));
        let (_, pk) = public_of(fresh_private_key().expose()).unwrap();
        assert!(is_acceptable_agreement_key(&pk));
        // The list holds five distinct values.
        for (i, a) in SMALL_ORDER.iter().enumerate() {
            for b in &SMALL_ORDER[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn acceptability_refuses_the_high_bit_and_values_not_below_p() {
        let mut k = hex32(&"11".repeat(32));
        k[31] = 0x11;
        assert!(is_acceptable_agreement_key(&k));
        // [SEC-SEL-013]: the most significant bit set.
        let mut high = k;
        high[31] |= 0x80;
        assert!(!is_acceptable_agreement_key(&high));
        // Every u from p to 2^255 - 1 is refused, whatever its point.
        for low in 0xedu8..=0xff {
            let mut u = [0xffu8; 32];
            u[0] = low;
            u[31] = 0x7f;
            assert!(!is_acceptable_agreement_key(&u), "{low:#x}");
        }
        // p - 2 is canonical and of large order.
        let mut p_minus_two = [0xffu8; 32];
        p_minus_two[0] = 0xeb;
        p_minus_two[31] = 0x7f;
        assert!(is_acceptable_agreement_key(&p_minus_two));
        // p + 1, the non-canonical spelling of 1, is refused for its value.
        let mut p_plus_one = [0xffu8; 32];
        p_plus_one[0] = 0xee;
        p_plus_one[31] = 0x7f;
        assert!(!is_acceptable_agreement_key(&p_plus_one));
    }

    #[test]
    fn seal_then_open_round_trips_for_each_kind() {
        let bob = private(BOB_PRIVATE);
        let to = AgreementPublicKey::from_base64url(BOB_PUBLIC).unwrap();
        for kind in [
            PayloadKind::Envelope,
            PayloadKind::Presence,
            PayloadKind::Receipt,
        ] {
            let frame = seal(&to, kind, b"{\"signed\":true}", 65_590).unwrap();
            // [SEC-SEL-025]: the plaintext is padded to a multiple of 256 octets.
            assert_eq!((frame.len() - 1 - ENC_LEN - TAG_LEN) % PADDING_BLOCK, 0);
            assert_eq!(frame[0], FRAME_VERSION);
            let opened = open(&bob, &frame).unwrap();
            assert_eq!(opened.kind, kind);
            assert_eq!(opened.payload.as_slice(), b"{\"signed\":true}");
            // [SEC-SEL-031]: Debug shows no content.
            assert!(!format!("{opened:?}").contains("signed"));
        }
        assert_eq!(
            seal(&to, PayloadKind::Sealed, b"x", 65_590),
            Err(SealError::NotSealable)
        );
    }

    #[test]
    fn each_frame_has_a_fresh_ephemeral_key() {
        // [SEC-SEL-021]: two frames of the same payload share no `enc` and no ciphertext.
        let to = AgreementPublicKey::from_base64url(BOB_PUBLIC).unwrap();
        let a = seal(&to, PayloadKind::Envelope, b"same", 65_590).unwrap();
        let b = seal(&to, PayloadKind::Envelope, b"same", 65_590).unwrap();
        assert_ne!(a[1..33], b[1..33]);
        assert_ne!(a[33..], b[33..]);
    }

    #[test]
    fn smallest_frame_is_54_octets_and_padding_respects_the_limit() {
        let to = AgreementPublicKey::from_base64url(BOB_PUBLIC).unwrap();
        let f = seal_padded(&to, PayloadKind::Envelope, b"", 0).unwrap();
        assert_eq!(f.len(), FRAME_OVERHEAD);
        assert_eq!(FRAME_OVERHEAD, 54);
        assert!(open(&private(BOB_PRIVATE), &f).is_some());
        // Padded to 256 when that fits; unpadded when only that fits; refused otherwise.
        assert_eq!(padding_for(10, 65_590), Some(256 - 15));
        assert_eq!(padding_for(10, 15 + 49), Some(0));
        assert_eq!(padding_for(10, 15 + 48), None);
        // A default-limit envelope fits a minimal sealing transport ([IFC-TRN-109]).
        assert!(padding_for(65_536, 65_590).is_some());
    }

    #[test]
    fn frames_that_fail_a_check_do_not_open() {
        let bob = private(BOB_PRIVATE);
        let to = AgreementPublicKey::from_base64url(BOB_PUBLIC).unwrap();
        let good = seal_padded(&to, PayloadKind::Envelope, b"payload", 3).unwrap();
        assert!(open(&bob, &good).is_some());
        // [SEC-SEL-030]: another device's key.
        assert!(open(&private(CAROL_PRIVATE), &good).is_none());
        // Version octet.
        let mut v = good.clone();
        v[0] = 0x02;
        assert!(open(&bob, &v).is_none());
        // Too short.
        assert!(open(&bob, &good[..FRAME_OVERHEAD - 1]).is_none());
        // Tampered ciphertext, tag and enc.
        for i in [1, 40, good.len() - 1] {
            let mut t = good.clone();
            t[i] ^= 1;
            assert!(open(&bob, &t).is_none(), "{i}");
        }
        // A small-order `enc` makes the shared secret zero.
        let mut z = good.clone();
        z[1..33].copy_from_slice(&[0u8; 32]);
        assert!(open(&bob, &z).is_none());
    }

    /// A frame built directly from a plaintext, for the checks after the HPKE open.
    fn frame_of(plaintext: &[u8]) -> Vec<u8> {
        let to = <Kem as KemTrait>::PublicKey::from_bytes(
            AgreementPublicKey::from_base64url(BOB_PUBLIC)
                .unwrap()
                .0
                .as_slice(),
        )
        .unwrap();
        let mut buf = plaintext.to_vec();
        let (enc, tag) = hpke::single_shot_seal_in_place_detached::<Aead, Kdf, Kem, _>(
            &OpModeS::Base,
            &to,
            SEAL_INFO,
            &mut buf,
            &[],
            &mut OsRng,
        )
        .unwrap();
        let mut f = vec![FRAME_VERSION];
        f.extend_from_slice(enc.to_bytes().as_slice());
        f.extend_from_slice(&buf);
        f.extend_from_slice(tag.to_bytes().as_slice());
        f
    }

    #[test]
    fn plaintext_checks_fail_closed() {
        let bob = private(BOB_PRIVATE);
        assert!(open(&bob, &frame_of(&[1, 0, 0, 0, 2, b'h', b'i', 0, 0])).is_some());
        // Unknown kind octets.
        for k in [0u8, 4, 0xff] {
            assert!(open(&bob, &frame_of(&[k, 0, 0, 0, 0])).is_none(), "{k}");
        }
        // `L` beyond the plaintext.
        assert!(open(&bob, &frame_of(&[1, 0, 0, 0, 3, b'h', b'i'])).is_none());
        assert!(open(&bob, &frame_of(&[1, 0xff, 0xff, 0xff, 0xff])).is_none());
        // Non-zero padding.
        assert!(open(&bob, &frame_of(&[1, 0, 0, 0, 1, b'h', 0, 1])).is_none());
    }

    #[test]
    fn statements_admit_and_refuse_by_section_14_3() {
        let bob = identity("principal-b");
        let mallory = identity("principal-m");
        let keys = trusted(&[&bob]);
        let now = ts("2026-10-09T13:00:00Z");
        let k1 = AgreementPublicKey::from_base64url(BOB_PUBLIC).unwrap();
        let st = AgreementStatement::issue(&bob, &k1, 1, &ts("2026-10-09T12:00:00Z")).unwrap();
        let held = check_statement(&st, &keys, |_| None, &now).unwrap();
        assert_eq!(held.key_id(), bob.key_id());
        assert_eq!(held.seq(), 1);
        // [SEC-SEL-012]: an untrusted signer.
        let st_m =
            AgreementStatement::issue(&mallory, &k1, 1, &ts("2026-10-09T12:00:00Z")).unwrap();
        assert_eq!(
            check_statement(&st_m, &keys, |_| None, &now),
            Err(StatementRefusal::Signature)
        );
        // [SEC-SEL-014]: not newer than the held one; a higher seq with an earlier time is.
        assert_eq!(
            check_statement(&st, &keys, |_| Some(held.clone()), &now),
            Err(StatementRefusal::NotNewer)
        );
        let st2 = AgreementStatement::issue(&bob, &k1, 2, &ts("2026-10-09T11:00:00Z")).unwrap();
        assert!(check_statement(&st2, &keys, |_| Some(held.clone()), &now).is_ok());
        // [SEC-SEL-042]: exactly now + W is refused; a nanosecond earlier is admitted.
        let edge = Timestamp::from_unix_nanos(now.unix_nanos() + WINDOW_NANOS).unwrap();
        let st_edge = AgreementStatement::issue(&bob, &k1, 3, &edge).unwrap();
        assert_eq!(
            check_statement(&st_edge, &keys, |_| None, &now),
            Err(StatementRefusal::TooFarAhead)
        );
        let inside =
            Timestamp::from_unix_nanos(now.unix_nanos() + WINDOW_NANOS - 1_000_000).unwrap();
        let st_in = AgreementStatement::issue(&bob, &k1, 3, &inside).unwrap();
        assert!(check_statement(&st_in, &keys, |_| None, &now).is_ok());
        // [SEC-SEL-010]: an extra member, and a seq out of range, re-signed so the signature
        // is not what refuses them.
        let mut extra = st.wire.clone();
        extra.insert("note", "x".into());
        assert_eq!(
            check_statement(&resigned(&bob, extra), &keys, |_| None, &now),
            Err(StatementRefusal::Malformed)
        );
        for seq in ["9007199254740992", "1.0", "1e0"] {
            let mut o = st.wire.clone();
            o.insert("seq", crate::json::parse(seq.as_bytes()).unwrap());
            assert_eq!(
                check_statement(&resigned(&bob, o), &keys, |_| None, &now),
                Err(StatementRefusal::Malformed),
                "{seq}"
            );
        }
        // The highest seq is admitted, and no statement can be issued above it.
        let top = AgreementStatement::issue(&bob, &k1, MAX_STATEMENT_SEQ, &now).unwrap();
        assert!(check_statement(&top, &keys, |_| None, &now).is_ok());
        assert!(AgreementStatement::issue(&bob, &k1, MAX_STATEMENT_SEQ + 1, &now).is_none());
        // [SEC-SEL-013]: a weak key, signed correctly.
        let mut weak = st.wire.clone();
        weak.insert(
            "agreement_key",
            base64url::encode(&[0u8; 32]).as_str().into(),
        );
        assert_eq!(
            check_statement(&resigned(&bob, weak), &keys, |_| None, &now),
            Err(StatementRefusal::UnacceptableKey)
        );
        // [SEC-SEL-012]: another domain's signature over the same object.
        let mut other = st.wire.clone();
        let mut sec = other.get("security").unwrap().as_object().unwrap().clone();
        sec.remove("signature");
        other.insert("security", Json::Object(sec.clone()));
        let sig = bob.sign_object(SigningDomain::Presence, &other).unwrap();
        sec.insert("signature", sig.as_str().into());
        other.insert("security", Json::Object(sec));
        assert_eq!(
            check_statement(&AgreementStatement { wire: other }, &keys, |_| None, &now),
            Err(StatementRefusal::Signature)
        );
    }

    /// `o`, with its signature replaced by `who`'s under `oac-agreement-v1`.
    fn resigned(who: &DeviceIdentity, mut o: JsonObject) -> AgreementStatement {
        let mut sec = o.get("security").unwrap().as_object().unwrap().clone();
        sec.remove("signature");
        o.insert("security", Json::Object(sec.clone()));
        let sig = who.sign_object(SigningDomain::Agreement, &o).unwrap();
        sec.insert("signature", sig.as_str().into());
        o.insert("security", Json::Object(sec));
        AgreementStatement { wire: o }
    }

    #[test]
    fn agreement_key_is_separate_from_and_not_derived_from_the_device_key() {
        // [SEC-SEL-001], [SEC-SEL-003]: the agreement key is not the device key's Montgomery
        // form, and its private key is not the device key's scalar.
        let me = identity("p");
        let store = MemoryAgreementKeyStore::new();
        let keys =
            AgreementKeys::load_or_generate(&store, &me, &ts("2026-10-09T12:00:00Z")).unwrap();
        let st = keys.statement();
        let ak = st.as_json().get("agreement_key").unwrap().as_str().unwrap();
        let ed = ed25519_dalek::VerifyingKey::from_bytes(&me.public_key().to_octets()).unwrap();
        assert_ne!(ak, base64url::encode(&ed.to_montgomery().to_bytes()));
        assert_ne!(ak, me.public_key().to_base64url());
        // [SEC-SEL-002]: two devices, two different keys.
        let other = AgreementKeys::load_or_generate(
            &MemoryAgreementKeyStore::new(),
            &me,
            &ts("2026-10-09T12:00:00Z"),
        )
        .unwrap();
        assert_ne!(other.statement(), keys.statement());
    }

    #[test]
    fn issuer_seq_rises_and_survives_a_restart() {
        // [SEC-SEL-040], [SEC-SEL-043].
        let me = identity("p");
        let store = MemoryAgreementKeyStore::new();
        let t0 = ts("2026-10-09T12:00:00Z");
        let mut keys = AgreementKeys::load_or_generate(&store, &me, &t0).unwrap();
        assert_eq!(keys.highest_seq(), 0);
        let first = keys.statement().clone();
        // A restart reads the same key and statement back.
        let again = AgreementKeys::load_or_generate(&store, &me, &t0).unwrap();
        assert_eq!(again.statement(), &first);
        // A replacement takes the next seq, and keeps the old key for opening.
        let t1 = ts("2026-10-09T12:10:00Z");
        keys.replace(&store, &me, &t1).unwrap();
        assert_eq!(keys.highest_seq(), 1);
        assert_eq!(keys.len(), 2);
        let restarted = AgreementKeys::load_or_generate(&store, &me, &t1).unwrap();
        assert_eq!(restarted.highest_seq(), 1);
        assert_eq!(restarted.statement(), keys.statement());
        // [SEC-SEL-018]: erasing the replaced key keeps the count.
        assert_eq!(keys.erase_replaced(&store).unwrap(), 1);
        assert_eq!(keys.len(), 1);
        let restarted = AgreementKeys::load_or_generate(&store, &me, &t1).unwrap();
        assert_eq!(restarted.highest_seq(), 1);
        assert_eq!(restarted.len(), 1);
        // A failing store issues nothing: the seq does not move.
        store.fail_saves(true);
        assert!(matches!(
            keys.replace(&store, &me, &t1),
            Err(AgreementKeyError::Store(_))
        ));
        assert_eq!(keys.highest_seq(), 1);
        // A store whose every key was erased still counts on from its seq.
        let empty = MemoryAgreementKeyStore::new();
        empty
            .save(&AgreementKeyState {
                keys: Vec::new(),
                highest_seq: Some(7),
            })
            .unwrap();
        assert_eq!(
            AgreementKeys::load_or_generate(&empty, &me, &t1)
                .unwrap()
                .highest_seq(),
            8
        );
        // At the highest seq, no replacement is possible.
        let full = MemoryAgreementKeyStore::new();
        full.save(&AgreementKeyState {
            keys: Vec::new(),
            highest_seq: Some(MAX_STATEMENT_SEQ),
        })
        .unwrap();
        assert_eq!(
            AgreementKeys::load_or_generate(&full, &me, &t1).unwrap_err(),
            AgreementKeyError::SeqExhausted
        );
    }

    #[test]
    fn keys_are_tried_newest_first() {
        // [SEC-SEL-037]: ordered by `issued_at`, not by `seq` or insertion order.
        let me = identity("p");
        let older = StoredAgreementKey {
            private_key: private(BOB_PRIVATE),
            seq: 5,
            issued_at: ts("2026-10-09T12:00:00Z"),
        };
        let newer = StoredAgreementKey {
            private_key: private(CAROL_PRIVATE),
            seq: 4,
            issued_at: ts("2026-10-09T13:00:00Z"),
        };
        let keys = AgreementKeys::for_tests(&me, &[older, newer]);
        let first = keys.statements().next().unwrap();
        assert_eq!(
            first.as_json().get("issued_at").unwrap().as_str(),
            Some("2026-10-09T13:00:00Z")
        );
        // The current key is the highest seq's.
        assert_eq!(
            keys.statement()
                .as_json()
                .get("agreement_key")
                .unwrap()
                .as_str(),
            Some(BOB_PUBLIC)
        );
    }

    #[test]
    fn no_type_prints_a_private_key() {
        // [SEC-SEL-004].
        let k = private(BOB_PRIVATE);
        let shown = format!(
            "{k:?} {:?} {:?}",
            StoredAgreementKey {
                private_key: k.duplicate(),
                seq: 0,
                issued_at: ts("2026-10-09T12:00:00Z"),
            },
            AgreementKeys::for_tests(
                &identity("p"),
                &[StoredAgreementKey {
                    private_key: k.duplicate(),
                    seq: 0,
                    issued_at: ts("2026-10-09T12:00:00Z"),
                }]
            )
        );
        assert!(!shown.contains(BOB_PRIVATE), "{shown}");
        assert!(!shown.contains(&base64url::encode(k.expose())), "{shown}");
        assert!(!shown.contains("182, 44"), "{shown}"); // the first two octets, as a slice
        let mut src = hex32(BOB_PRIVATE);
        let _ = AgreementPrivateKey::from_octets(&mut src);
        assert_eq!(src, [0u8; 32], "the caller's copy is zeroized");
    }
}
