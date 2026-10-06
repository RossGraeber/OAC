// SPDX-License-Identifier: Apache-2.0

//! Device keys (`spec/security.md` §5.1, §5.2): the device key pair, its public key and key
//! id, the device identity that signs, and the storage seam for the private seed.
//!
//! # Where the private seed may go
//!
//! The 32-octet private seed lives in a [`SecretSeed`] while it is read or written, and in a
//! [`DeviceKey`] while the process signs. Neither type implements `Clone`, `Display` or a
//! `Debug` that shows the seed, and both are zeroized when dropped. The seed leaves this
//! crate in one place only: [`SecretSeed::expose`], which a [`KeyStore`] calls to write it
//! to the operating system's credential store or the encrypted-file fallback of the same
//! process ([SEC-KEY-004]; `docs/planning/decisions/C4-session-identity.md` §10-§11).
//! Nothing here writes a seed to a log, a receipt, an envelope, a peer, a harness, a
//! session or a model. A session gets no signing key of its own: it is bound to the device
//! key by a registration record ([SEC-KEY-005], [`crate::registration`]).
//!
//! Signing uses `ed25519-dalek` 3.0.0 (`docs/planning/decisions/C5-envelope-auth.md` §2),
//! through its ordinary `SigningKey` API only, never a `hazmat` one (C5 §2).

use crate::base64url;
use crate::canonical::{CanonicalError, SigningDomain, signing_input};
use crate::envelope::{Envelope, EnvelopeDraft, Nonce, SecurityPrincipal, Signature};
use crate::ids::{KeyId, Token};
use crate::json::JsonObject;
use crate::trust::TrustedKey;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use std::fmt;
use std::sync::Mutex;
use zeroize::Zeroizing;

/// Fills `out` from the operating system's cryptographically secure random number
/// generator.
///
/// # Panics
///
/// When the operating system has no random source; no key, nonce or id may then be made.
pub(crate) fn random_octets<const N: usize>() -> Zeroizing<[u8; N]> {
    let mut out = Zeroizing::new([0u8; N]);
    getrandom::fill(out.as_mut()).expect("the operating system's random number generator");
    out
}

/// An Ed25519 public key that may enter the trusted key set ([SEC-KEY-034]): a canonical
/// point encoding (RFC 8032 §5.1.3) whose point is not of small order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PublicKey {
    key: VerifyingKey,
}

impl PublicKey {
    /// The key whose 32-octet encoding is `octets`. `None` when the encoding is not
    /// canonical (a y-coordinate not less than p, or a zero x-coordinate with its sign bit
    /// set) or the point has small order ([SEC-KEY-034], [SEC-SIG-023]).
    ///
    /// `VerifyingKey::from_bytes` keeps a non-canonical encoding rather than rejecting it
    /// (`spec/security.md` §6.3, reference implementation note), so the y-coordinate is
    /// checked here. A zero x-coordinate occurs only at y = 1 and y = p - 1, the identity
    /// and the point of order 2, which `is_weak` refuses whatever the sign bit says.
    pub fn from_octets(octets: [u8; 32]) -> Option<PublicKey> {
        if !y_is_canonical(&octets) {
            return None;
        }
        let key = VerifyingKey::from_bytes(&octets).ok()?;
        if key.is_weak() {
            return None;
        }
        Some(PublicKey { key })
    }

    /// The key whose unpadded base64url encoding is `s` (`spec/security.md` §3.2), under
    /// the rules of [`PublicKey::from_octets`].
    pub fn from_base64url(s: &str) -> Option<PublicKey> {
        PublicKey::from_octets(base64url::decode_exact::<32>(s)?)
    }

    /// The 32-octet encoding (RFC 8032 §5.1.5).
    pub fn to_octets(&self) -> [u8; 32] {
        self.key.to_bytes()
    }

    /// The unpadded base64url encoding of [`PublicKey::to_octets`].
    pub fn to_base64url(&self) -> String {
        base64url::encode(&self.to_octets())
    }

    /// The key id: the lower-case hexadecimal SHA-256 digest of the 32-octet encoding
    /// ([SEC-KEY-010]).
    pub fn key_id(&self) -> KeyId {
        let digest = Sha256::digest(self.to_octets());
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        KeyId::parse(&hex).expect("64 lower-case hexadecimal digits")
    }

    pub(crate) fn verifying_key(&self) -> &VerifyingKey {
        &self.key
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({})", self.to_base64url())
    }
}

/// Whether the little-endian y-coordinate in `octets` (the sign bit cleared) is below
/// p = 2^255 - 19.
fn y_is_canonical(octets: &[u8; 32]) -> bool {
    let top = octets[31] & 0x7f;
    // y >= p exactly when y is one of p..=2^255-1: the top byte 0x7f, bytes 1..=30 all
    // 0xff, and the low byte at least 0xed.
    !(top == 0x7f && octets[1..31].iter().all(|&b| b == 0xff) && octets[0] >= 0xed)
}

/// A device key's 32-octet private seed, while a [`KeyStore`] reads or writes it.
/// Zeroized on drop; its `Debug` shows nothing of the seed.
pub struct SecretSeed(Zeroizing<[u8; 32]>);

impl SecretSeed {
    /// The seed whose octets are `octets`, as read back from a store.
    pub fn from_octets(octets: [u8; 32]) -> SecretSeed {
        SecretSeed(Zeroizing::new(octets))
    }

    /// The seed that `octets` holds, if it holds exactly 32 octets.
    pub fn from_slice(octets: &[u8]) -> Option<SecretSeed> {
        let mut seed = Zeroizing::new([0u8; 32]);
        if octets.len() != seed.len() {
            return None;
        }
        seed.copy_from_slice(octets);
        Some(SecretSeed(seed))
    }

    /// The seed's octets, for a [`KeyStore`] to write to its storage in this process. No
    /// other caller has a reason to read them ([SEC-KEY-004]).
    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for SecretSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretSeed(..)")
    }
}

/// Why a [`KeyStore`] could not read or write the device key. The text never holds key
/// material.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyStoreError {
    /// The store cannot be used on this host (for example, no credential service is
    /// running). A caller may fall back to another store.
    Unavailable(String),
    /// The store holds an entry that is not a device key seed, or cannot be decrypted.
    Corrupt(String),
    /// The store failed while reading or writing.
    Failed(String),
}

impl fmt::Display for KeyStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyStoreError::Unavailable(e) => write!(f, "key store unavailable: {e}"),
            KeyStoreError::Corrupt(e) => write!(f, "key store entry unusable: {e}"),
            KeyStoreError::Failed(e) => write!(f, "key store failed: {e}"),
        }
    }
}

impl std::error::Error for KeyStoreError {}

/// Where an implementation keeps its device key's private seed (`spec/security.md` §5.1:
/// "How an implementation stores the private seed is its own choice").
///
/// The reference implementation's stores are the operating system's credential store, with
/// an encrypted-file fallback (`docs/planning/decisions/C4-session-identity.md` §10-§11),
/// built by the `oac` binary. [`MemoryKeyStore`] is the in-process double the CI-default
/// tests use. A store holds OAC's own device key only, never a harness's credentials.
pub trait KeyStore {
    /// The stored seed, or `None` when the store holds no device key yet.
    ///
    /// # Errors
    ///
    /// [`KeyStoreError`] when the store cannot be read.
    fn load(&self) -> Result<Option<SecretSeed>, KeyStoreError>;

    /// Writes `seed` as the device key, replacing nothing: a store that already holds a
    /// device key refuses.
    ///
    /// # Errors
    ///
    /// [`KeyStoreError`] when the store cannot be written or already holds a key.
    fn save(&self, seed: &SecretSeed) -> Result<(), KeyStoreError>;
}

/// A [`KeyStore`] in this process's memory: nothing outlives the process. For tests and
/// for an ephemeral device that is not meant to be paired.
#[derive(Debug, Default)]
pub struct MemoryKeyStore {
    seed: Mutex<Option<SecretSeed>>,
}

impl MemoryKeyStore {
    /// An empty store.
    pub fn new() -> MemoryKeyStore {
        MemoryKeyStore::default()
    }
}

impl KeyStore for MemoryKeyStore {
    fn load(&self) -> Result<Option<SecretSeed>, KeyStoreError> {
        let seed = self
            .seed
            .lock()
            .map_err(|_| KeyStoreError::Failed("lock poisoned".into()))?;
        Ok(seed.as_ref().map(|s| SecretSeed::from_octets(*s.expose())))
    }

    fn save(&self, seed: &SecretSeed) -> Result<(), KeyStoreError> {
        let mut slot = self
            .seed
            .lock()
            .map_err(|_| KeyStoreError::Failed("lock poisoned".into()))?;
        if slot.is_some() {
            return Err(KeyStoreError::Failed(
                "a device key is already stored".into(),
            ));
        }
        *slot = Some(SecretSeed::from_octets(*seed.expose()));
        Ok(())
    }
}

/// The device key ([SEC-KEY-002]): one Ed25519 key pair. Its `Debug` shows the key id only;
/// the seed is zeroized when the key is dropped (`ed25519-dalek`'s `SigningKey` implements
/// `ZeroizeOnDrop`).
pub struct DeviceKey {
    signing: SigningKey,
    public: PublicKey,
}

impl DeviceKey {
    /// A new device key from a 32-octet seed drawn from the operating system's
    /// cryptographically secure random number generator ([SEC-KEY-003]).
    pub fn generate() -> DeviceKey {
        DeviceKey::from_seed(&SecretSeed(random_octets::<32>()))
    }

    /// The device key whose private seed is `seed`.
    pub fn from_seed(seed: &SecretSeed) -> DeviceKey {
        let signing = SigningKey::from_bytes(seed.expose());
        // A = aB with a clamped scalar a and B of prime order, so A has prime order and its
        // encoding is canonical: it always passes [SEC-KEY-034].
        let public = PublicKey::from_octets(signing.verifying_key().to_bytes())
            .expect("a derived public key is canonical and not of small order");
        DeviceKey { signing, public }
    }

    /// The stored device key, or, when `store` holds none, a new one that is saved to
    /// `store` first. A device keeps one key ([SEC-KEY-002]): an existing key is never
    /// replaced.
    ///
    /// # Errors
    ///
    /// [`KeyStoreError`] from the store.
    pub fn load_or_generate(store: &dyn KeyStore) -> Result<DeviceKey, KeyStoreError> {
        if let Some(seed) = store.load()? {
            return Ok(DeviceKey::from_seed(&seed));
        }
        let seed = SecretSeed(random_octets::<32>());
        store.save(&seed)?;
        // Read back what the store holds, so a store that dropped the write fails now
        // rather than at the next start with a different key.
        match store.load()? {
            Some(back) if back.expose() == seed.expose() => Ok(DeviceKey::from_seed(&seed)),
            _ => Err(KeyStoreError::Failed(
                "the saved device key did not read back".into(),
            )),
        }
    }

    /// The public key.
    pub fn public_key(&self) -> &PublicKey {
        &self.public
    }

    /// The key id ([SEC-KEY-010]).
    pub fn key_id(&self) -> KeyId {
        self.public.key_id()
    }

    /// The Ed25519 signature (RFC 8032 §5.1.6) of `message`.
    fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.signing.sign(message).to_bytes()
    }
}

impl fmt::Debug for DeviceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DeviceKey(key_id: {})", self.key_id())
    }
}

/// This device: its key, and the principal label under which that key is paired. Its own
/// trusted-key-set entry ([SEC-KEY-031]) is the one entry it signs as.
#[derive(Debug)]
pub struct DeviceIdentity {
    key: DeviceKey,
    entry: TrustedKey,
}

impl DeviceIdentity {
    /// The identity of a device whose key is `key`, paired under `principal`, an identifier
    /// token ([SEC-KEY-020]).
    pub fn new(key: DeviceKey, principal: Token) -> DeviceIdentity {
        let entry = TrustedKey::new(principal, *key.public_key());
        DeviceIdentity { key, entry }
    }

    /// The device key's public half.
    pub fn public_key(&self) -> &PublicKey {
        self.key.public_key()
    }

    /// The device key's key id.
    pub fn key_id(&self) -> &KeyId {
        self.entry.key_id()
    }

    /// The principal label.
    pub fn principal(&self) -> &Token {
        self.entry.principal()
    }

    /// This device's own trusted-key-set entry ([SEC-KEY-031]).
    pub fn trusted_entry(&self) -> &TrustedKey {
        &self.entry
    }

    /// The signer of everything this device signs: its own entry ([SEC-SIG-001],
    /// [SEC-SIG-002]).
    pub fn security_principal(&self) -> SecurityPrincipal {
        self.entry.security_principal()
    }

    /// Signs `draft` with the device key over the signing input of `spec/security.md` §6.2
    /// under `oac-envelope-v1` ([SEC-SIG-010]). `security.key_id` and
    /// `security.principal` are this device's ([SEC-SIG-001], [SEC-SIG-002]), and
    /// `security.nonce` is 16 fresh octets from the operating system's cryptographically
    /// secure random number generator ([SEC-SIG-003]). The envelope is serialized once and
    /// never changed afterwards ([SEC-SIG-012]).
    pub fn sign_envelope(&self, draft: EnvelopeDraft) -> Envelope {
        self.sign_envelope_with_nonce(draft, Nonce::from_octets(*random_octets::<16>()))
    }

    /// [`DeviceIdentity::sign_envelope`] with a given nonce. Crate-private: outside the
    /// conformance check that reproduces the fixtures' signatures, a nonce is always fresh.
    pub(crate) fn sign_envelope_with_nonce(&self, draft: EnvelopeDraft, nonce: Nonce) -> Envelope {
        let signer = self.security_principal();
        let input = draft
            .signing_input(&signer, &nonce)
            .expect("a draft's numbers are all integers, so it always canonicalizes");
        let signature = Signature::from_octets(self.key.sign(&input));
        draft.seal(&signer, nonce, signature)
    }

    /// The signature, as unpadded base64url, of `obj` under `domain` (§6.2): the object
    /// must already hold its `security` member, without `signature`.
    pub(crate) fn sign_object(
        &self,
        domain: SigningDomain,
        obj: &JsonObject,
    ) -> Result<Signature, CanonicalError> {
        let input = signing_input(domain, obj)?;
        Ok(Signature::from_octets(self.key.sign(&input)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex32(s: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, o) in out.iter_mut().enumerate() {
            *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
        }
        out
    }

    #[test]
    fn test_key_alice_matches_the_fixture_key_file() {
        // tests/protocol/sec-test-keys.json, key "alice" (test keys only).
        let key = DeviceKey::from_seed(&SecretSeed::from_octets(hex32(
            "404616bd59c9b8b3739dc249fff73869964eeea749e75fc6f2e49ba7567b8019",
        )));
        assert_eq!(
            key.public_key().to_base64url(),
            "b88HyoN6BEjyTX4WNo6n8JJcjhXQUuVjex4U1p-vqzc"
        );
        assert_eq!(
            key.key_id().as_str(),
            "b3db3806621d3de16e61d08fcb01841f6c60c54a68e7d482076677db5c44d174"
        );
    }

    #[test]
    fn signing_reproduces_the_fixture_signature() {
        // spec/security.md §3.2: Ed25519 signing is deterministic, so signing the envelope
        // of sec-sig/SEC-SIG-010.p01 with the test key "alice" and the fixture's nonce gives
        // the fixture's signature, octet for octet, and the fixture's envelope.
        use crate::envelope::TextPart;
        use crate::ids::{SessionId, Timestamp};
        use crate::json::{Json, parse};
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/protocol/sec-sig/SEC-SIG-010.p01-signature-verifies.json");
        let fx = parse(&std::fs::read(path).unwrap()).unwrap();
        let env = fx
            .as_object()
            .and_then(|o| o.get("input"))
            .and_then(Json::as_object)
            .and_then(|o| o.get("envelope"))
            .and_then(Json::as_object)
            .unwrap();
        let s = |m: &str| env.get(m).and_then(Json::as_str).unwrap();
        let sec = env.get("security").and_then(Json::as_object).unwrap();
        let alice = DeviceIdentity::new(
            DeviceKey::from_seed(&SecretSeed::from_octets(hex32(
                "404616bd59c9b8b3739dc249fff73869964eeea749e75fc6f2e49ba7567b8019",
            ))),
            Token::parse("principal-a").unwrap(),
        );
        let draft = EnvelopeDraft::new(
            Token::parse(s("id")).unwrap(),
            SessionId::parse(s("from")).unwrap(),
            SessionId::parse(s("to")).unwrap(),
            Timestamp::parse(s("created_at")).unwrap(),
            vec![TextPart::new("Please review the scheduler module.").unwrap()],
        )
        .unwrap()
        .with_ttl_ms(300_000)
        .unwrap()
        .with_conversation_id(Token::parse(s("conversation_id")).unwrap());
        let nonce = sec.get("nonce").and_then(Json::as_str).unwrap();
        let signed = alice.sign_envelope_with_nonce(
            draft,
            Nonce::from_octets(base64url::decode_exact::<16>(nonce).unwrap()),
        );
        assert_eq!(
            signed.security().signature(),
            sec.get("signature").and_then(Json::as_str).unwrap()
        );
        // Same members and values; member order is the draft's own and does not matter.
        let canon = |o: &JsonObject| crate::canonical::canonical(&Json::Object(o.clone()));
        assert_eq!(canon(signed.as_json()), canon(env));
    }

    #[test]
    fn admission_refuses_non_canonical_and_small_order_keys() {
        // The identity, (0, 1): small order.
        let mut identity = [0u8; 32];
        identity[0] = 1;
        assert!(PublicKey::from_octets(identity).is_none());
        // The identity with its sign bit set: x = 0 with the sign bit set, non-canonical.
        let mut signed_identity = identity;
        signed_identity[31] |= 0x80;
        assert!(PublicKey::from_octets(signed_identity).is_none());
        // (0, -1), the point of order 2.
        let mut order_two = [0xffu8; 32];
        order_two[0] = 0xec;
        order_two[31] = 0x7f;
        assert!(PublicKey::from_octets(order_two).is_none());
        // y = p + 1, a non-canonical encoding of the identity.
        let mut y_p_plus_one = [0xffu8; 32];
        y_p_plus_one[0] = 0xee;
        y_p_plus_one[31] = 0x7f;
        assert!(!y_is_canonical(&y_p_plus_one));
        assert!(PublicKey::from_octets(y_p_plus_one).is_none());
        // Every y from p to 2^255 - 1 is refused, whatever the point.
        for low in 0xedu8..=0xff {
            let mut y = [0xffu8; 32];
            y[0] = low;
            y[31] = 0x7f;
            assert!(PublicKey::from_octets(y).is_none(), "{low:#x}");
        }
        // p - 1 itself is canonical (the order-2 point is refused for its order).
        let mut p_minus_one = [0xffu8; 32];
        p_minus_one[0] = 0xec;
        p_minus_one[31] = 0x7f;
        assert!(y_is_canonical(&p_minus_one));
        // A generated key is always admitted.
        assert!(PublicKey::from_octets(DeviceKey::generate().public_key().to_octets()).is_some());
    }

    #[test]
    fn debug_never_shows_the_seed() {
        let seed = [0x42u8; 32];
        let key = DeviceKey::from_seed(&SecretSeed::from_octets(seed));
        let key_shown = format!("{key:?}");
        let shown = format!(
            "{:?} {key_shown} {:?}",
            SecretSeed::from_octets(seed),
            DeviceIdentity::new(key, Token::parse("p").unwrap())
        );
        assert!(!shown.contains("42, 42"), "{shown}");
        assert!(!shown.to_lowercase().contains(&"42".repeat(8)), "{shown}");
        assert!(!shown.contains(&base64url::encode(&seed)), "{shown}");
    }

    #[test]
    fn load_or_generate_keeps_one_key() {
        let store = MemoryKeyStore::new();
        let first = DeviceKey::load_or_generate(&store).unwrap();
        let again = DeviceKey::load_or_generate(&store).unwrap();
        assert_eq!(first.key_id(), again.key_id());
        assert!(matches!(
            store.save(&SecretSeed::from_octets([1; 32])),
            Err(KeyStoreError::Failed(_))
        ));
        assert_eq!(
            DeviceKey::load_or_generate(&store).unwrap().key_id(),
            first.key_id()
        );
        // Two generated keys differ.
        assert_ne!(
            DeviceKey::generate().key_id(),
            DeviceKey::generate().key_id()
        );
    }

    #[test]
    fn seed_from_slice_needs_32_octets() {
        assert!(SecretSeed::from_slice(&[0; 31]).is_none());
        assert!(SecretSeed::from_slice(&[0; 33]).is_none());
        assert_eq!(SecretSeed::from_slice(&[7; 32]).unwrap().expose(), &[7; 32]);
    }
}
