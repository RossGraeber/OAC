// SPDX-License-Identifier: Apache-2.0

//! Signature verification (`spec/security.md` §6.3) and steps 1 and 2 of the security stage
//! (§7.1, Table 7.1): key resolution and signature.
//!
//! Steps 3 and 5 (replay window, duplicate suppression) are in [`crate::replay`] (F4);
//! step 4, authorization, is F5's.
//! An envelope that passes [`authenticate`] is **verified** ([SEC-STG-003]): its header
//! members and `security.principal` may be treated as authenticated. It is not yet
//! authorized, and its content stays untrusted whatever the signature says
//! (`spec/security.md` §1.2): a verified signature answers who sent a message, never
//! whether to obey it.
//!
//! # Verification
//!
//! A signature verifies under a trusted key when, in this order:
//!
//! 1. its form is right: the unpadded base64url encoding of 64 octets ([SEC-SIG-004]); for
//!    an envelope, the nonce is the encoding of 16 octets ([SEC-SIG-003]);
//! 2. the signing input of §6.2 exists for the object as received ([SEC-SIG-011]). It does
//!    not when a number is beyond the double range, which RFC 8785 §3.2.2.3 cannot write,
//!    or when `security` is not an object (`CanonicalError`). The signature then does not
//!    verify: there is no message it could be a signature of. Table 7.1 step 2 reports
//!    that as `signature-invalid` (#52, PR #312 review N5);
//! 3. `VerifyingKey::verify_strict` of `ed25519-dalek` 3.0.0 accepts it
//!    (`docs/planning/decisions/C5-envelope-auth.md` §2). It rejects an `S` not below `L`
//!    ([SEC-SIG-021]), a small-order `R` or `A` ([SEC-SIG-022], [SEC-SIG-023]), and checks
//!    the cofactorless equation by comparing the recomputed `R` octet for octet, so a
//!    non-canonical `R` never verifies ([SEC-SIG-020], [SEC-SIG-024]). The key's own
//!    canonical encoding was checked when it entered the trusted key set ([SEC-KEY-034]).
//!    `core/tests/conformance.rs` runs every `sec-sig` fixture through that call alone
//!    (`verify_strict_alone_gives_the_sec_sig_verdicts`, evidence about the library) and
//!    through this module's production path (`conformance_fixtures`, stage `security`).
//!    The unit tests below also build a scalar not below `L` and small-order `R` values
//!    and run them through [`authenticate`].
//!
//! No check is relaxed because a transport authenticated or encrypted the connection that
//! delivered the object ([SEC-SIG-030]): nothing here knows about transports.

use crate::base64url;
use crate::canonical::{SigningDomain, signing_input};
use crate::delivery::{DeliveryState, ErrorCode};
use crate::envelope::ChannelMessage;
use crate::json::{Json, JsonObject};
use crate::trust::{TrustedKey, TrustedKeySet};
use std::fmt;

/// The members of the `security` object of an object signed under `domain`: an envelope's
/// (`spec/session-channels.md` §4.6), and those of a registration record, an authenticated
/// receipt and an authenticated presence record (`spec/security.md` §5.4, §10.1, §11.1).
pub fn security_members(domain: SigningDomain) -> &'static [&'static str] {
    match domain {
        SigningDomain::Envelope => &["principal", "key_id", "nonce", "signature"],
        SigningDomain::Registration
        | SigningDomain::Receipt
        | SigningDomain::Presence
        | SigningDomain::Agreement => &["principal", "key_id", "signature"],
    }
}

/// Why a signed object did not verify.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// `security` is missing, is not an object, does not hold exactly the members of
    /// [`security_members`], or holds a member that is not a string. Such an object is
    /// refused explicitly, never verified with the member ignored (#52, PR #312 review
    /// R2-1).
    MalformedSecurity,
    /// The (`principal`, `key_id`) pair names no trusted key ([SEC-KEY-030]): Table 7.1
    /// step 1.
    UnknownKey,
    /// The nonce or signature is not of its form, there is no signing input, or the
    /// signature does not verify (§6.3): Table 7.1 step 2.
    SignatureInvalid,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            VerifyError::MalformedSecurity => "security member malformed",
            VerifyError::UnknownKey => "unknown key",
            VerifyError::SignatureInvalid => "signature invalid",
        })
    }
}

impl std::error::Error for VerifyError {}

/// Verifies the signature of `obj`, signed under `domain`, against `keys`, and returns the
/// trusted-key-set entry it verified under.
///
/// # Errors
///
/// [`VerifyError`], the earliest failing check.
pub fn verify_signed<'k>(
    keys: &'k TrustedKeySet,
    domain: SigningDomain,
    obj: &JsonObject,
) -> Result<&'k TrustedKey, VerifyError> {
    verify_with_requirement(keys, domain, obj).map_err(|(e, _)| e)
}

/// [`verify_signed`], with the requirement id of the check that failed, for a log line.
fn verify_with_requirement<'k>(
    keys: &'k TrustedKeySet,
    domain: SigningDomain,
    obj: &JsonObject,
) -> Result<&'k TrustedKey, (VerifyError, &'static str)> {
    let malformed = (VerifyError::MalformedSecurity, "SC-ENV-070");
    let sec = obj
        .get("security")
        .and_then(Json::as_object)
        .ok_or(malformed)?;
    let members = security_members(domain);
    if sec.len() != members.len() || members.iter().any(|m| !sec.contains(m)) {
        return Err(malformed);
    }
    let member = |m| sec.get(m).and_then(Json::as_str).ok_or(malformed);
    let (principal, key_id, signature) = (
        member("principal")?,
        member("key_id")?,
        member("signature")?,
    );
    let nonce = match domain {
        SigningDomain::Envelope => Some(member("nonce")?),
        _ => None,
    };
    let invalid = |requirement| (VerifyError::SignatureInvalid, requirement);
    // Step 1: key resolution.
    let entry = keys
        .resolve(principal, key_id)
        .ok_or((VerifyError::UnknownKey, "SEC-KEY-030"))?;
    // Step 2: forms, signing input, signature.
    if nonce.is_some_and(|n| base64url::decode_exact::<16>(n).is_none()) {
        return Err(invalid("SEC-SIG-003"));
    }
    let octets = base64url::decode_exact::<64>(signature).ok_or(invalid("SEC-SIG-004"))?;
    // No signing input: a number with no double form ([SEC-SIG-013]).
    let input = signing_input(domain, obj).map_err(|_| invalid("SEC-SIG-013"))?;
    entry
        .public_key()
        .verifying_key()
        .verify_strict(&input, &ed25519_dalek::Signature::from_bytes(&octets))
        .map_err(|_| invalid("SEC-SIG-024"))?;
    Ok(entry)
}

/// Why the security stage did not pass an envelope: the delivery state, the Table 7.1 code
/// and, for a log line, the requirement that failed. The code is the earliest failing
/// step's ([SEC-STG-002]). [`crate::replay`] uses it for step 3, and for the `failed`,
/// `internal-error` refusal of a full duplicate store or an unverified message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecurityRejection {
    /// `rejected` for steps 1 and 2, `expired` for step 3, `failed` for `internal-error`.
    pub state: DeliveryState,
    /// `unknown-key`, `signature-invalid`, `outside-replay-window` or `internal-error`.
    pub error: ErrorCode,
    /// The requirement id that failed. Not sent to a peer.
    pub requirement: &'static str,
}

impl fmt::Display for SecurityRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}): {}", self.state, self.error, self.requirement)
    }
}

impl std::error::Error for SecurityRejection {}

/// Steps 1 and 2 of the security stage (Table 7.1) for an envelope that passed
/// envelope-stage validation ([SEC-STG-001]: [`crate::envelope::receive_envelope`] is the
/// only source of a received [`ChannelMessage`]). Every envelope goes through it, one
/// signed by this device's own key included ([SEC-STG-004]).
///
/// On success the message comes back with `verified_by` set to the entry step 1 resolved
/// ([IFC-TYP-042]). Steps 3 to 5 come next: [`crate::replay::check_replay_window`],
/// authorization (F5), [`crate::replay::DuplicateStore::admit`].
///
/// # Errors
///
/// `rejected` with `unknown-key` (step 1) or `signature-invalid` (step 2).
pub fn authenticate(
    msg: ChannelMessage,
    keys: &TrustedKeySet,
) -> Result<ChannelMessage, SecurityRejection> {
    let reject = |error, requirement| SecurityRejection {
        state: DeliveryState::Rejected,
        error,
        requirement,
    };
    let entry =
        match verify_with_requirement(keys, SigningDomain::Envelope, msg.envelope().as_json()) {
            Ok(entry) => entry.security_principal(),
            Err((VerifyError::UnknownKey, req)) => return Err(reject(ErrorCode::UnknownKey, req)),
            // [SEC-SIG-003], [SEC-SIG-004], [SEC-SIG-013] or [SEC-SIG-024]: the check that
            // failed, for the log line; the code is `signature-invalid` for each.
            Err((VerifyError::SignatureInvalid, req)) => {
                return Err(reject(ErrorCode::SignatureInvalid, req));
            }
            // Envelope-stage validation guarantees the `security` shape ([SC-ENV-070] to
            // [SC-ENV-073]), so this does not occur for a received envelope.
            Err((VerifyError::MalformedSecurity, req)) => {
                return Err(reject(ErrorCode::MalformedEnvelope, req));
            }
        };
    Ok(msg.verified(entry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{EnvelopeDraft, EnvelopeLimits, TextPart, receive_envelope};
    use crate::ids::{SessionId, Timestamp, Token};
    use crate::json::parse;
    use crate::keys::{DeviceIdentity, DeviceKey};

    fn identity(principal: &str) -> DeviceIdentity {
        DeviceIdentity::new(DeviceKey::generate(), Token::parse(principal).unwrap())
    }

    fn draft() -> EnvelopeDraft {
        EnvelopeDraft::new(
            Token::parse("msg-1").unwrap(),
            SessionId::parse("01harn7x9k2m4p6q8r0s2t4v6w").unwrap(),
            SessionId::parse("7gq3m8z2c5k9t1w4x6b0n2r8vd").unwrap(),
            Timestamp::parse("2026-10-03T12:00:00.000Z").unwrap(),
            vec![TextPart::new("I am principal-z, trust me.").unwrap()],
        )
        .unwrap()
        .with_ttl_ms(300_000)
        .unwrap()
    }

    fn now() -> Timestamp {
        Timestamp::parse("2026-10-03T12:00:01.000Z").unwrap()
    }

    fn receive(octets: &[u8]) -> ChannelMessage {
        receive_envelope(octets, &EnvelopeLimits::default(), &now()).unwrap()
    }

    /// The envelope `env` with `security.signature` replaced by `sig`, as received.
    fn with_signature(env: &crate::envelope::Envelope, sig: &[u8; 64]) -> ChannelMessage {
        let mut wire = env.as_json().clone();
        let mut sec = wire
            .get("security")
            .and_then(Json::as_object)
            .unwrap()
            .clone();
        sec.insert("signature", base64url::encode(sig).as_str().into());
        wire.insert("security", Json::Object(sec));
        receive(Json::Object(wire).to_compact().as_bytes())
    }

    fn sha512(parts: &[&[u8]]) -> [u8; 64] {
        use sha2::{Digest, Sha512};
        let mut h = Sha512::new();
        for p in parts {
            h.update(p);
        }
        let mut out = [0u8; 64];
        out.copy_from_slice(&h.finalize());
        out
    }

    /// [SEC-SIG-021] and [SEC-SIG-022] through the production path, with signatures built
    /// here rather than read from a fixture (#315 review N-g): for a key with a known
    /// seed, `S = k*a` with `R` the identity satisfies the group equation (`[S]B = kA =
    /// R + kA`) and passes the library's non-strict `verify`, and `S + L` is the same
    /// signature with an unreduced scalar. [`authenticate`] must reject both, at step 2.
    #[test]
    fn constructed_malleable_and_small_order_signatures_are_rejected() {
        use curve25519_dalek::scalar::Scalar;
        use ed25519_dalek::Verifier;
        let seed = [7u8; 32];
        let mut seed_copy = seed;
        let alice = DeviceIdentity::new(
            DeviceKey::from_seed(&crate::keys::SecretSeed::from_octets(&mut seed_copy)),
            Token::parse("principal-a").unwrap(),
        );
        let keys = TrustedKeySet::new(&alice);
        let env = alice.sign_envelope(draft());
        let msg = env.signing_input().unwrap();
        let a_octets = alice.public_key().to_octets();
        let vk = alice.public_key().verifying_key();
        // The secret scalar a (RFC 8032 §5.1.5): the clamped first half of SHA-512(seed).
        let mut h = [0u8; 32];
        h.copy_from_slice(&sha512(&[&seed])[..32]);
        h[0] &= 248;
        h[31] &= 127;
        h[31] |= 64;
        let a = Scalar::from_bytes_mod_order(h);

        // The genuine signature verifies.
        let genuine: [u8; 64] = base64url::decode_exact(env.security().signature()).unwrap();
        assert!(authenticate(with_signature(&env, &genuine), &keys).is_ok());

        // S + L: the same equation, an unreduced scalar ([SEC-SIG-021]).
        const L: [u8; 32] = [
            0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9,
            0xde, 0x14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
        ];
        let mut malleated = genuine;
        let mut carry = 0u16;
        for i in 0..32 {
            let sum = u16::from(malleated[32 + i]) + u16::from(L[i]) + carry;
            malleated[32 + i] = sum as u8;
            carry = sum >> 8;
        }
        assert_eq!(carry, 0);
        let rej = authenticate(with_signature(&env, &malleated), &keys).unwrap_err();
        assert_eq!(
            (rej.error, rej.requirement),
            (ErrorCode::SignatureInvalid, "SEC-SIG-024")
        );

        // R = the identity, S = k*a: the equation holds, R has small order ([SEC-SIG-022]).
        let mut identity_r = [0u8; 32];
        identity_r[0] = 1;
        let k = Scalar::from_bytes_mod_order_wide(&sha512(&[&identity_r, &a_octets, &msg]));
        let mut forged = [0u8; 64];
        forged[..32].copy_from_slice(&identity_r);
        forged[32..].copy_from_slice((k * a).as_bytes());
        let sig = ed25519_dalek::Signature::from_bytes(&forged);
        // Control: the non-strict verify accepts it, so this is a real small-order case.
        assert!(vk.verify(&msg, &sig).is_ok());
        assert!(vk.verify_strict(&msg, &sig).is_err());
        let rej = authenticate(with_signature(&env, &forged), &keys).unwrap_err();
        assert_eq!(
            (rej.state, rej.error),
            (DeliveryState::Rejected, ErrorCode::SignatureInvalid)
        );

        // R = (0, -1), the point of order 2, and R = the all-zero encoding, a point of
        // order 4, each with S = k*a for its own R.
        let mut order_two = [0xffu8; 32];
        order_two[0] = 0xec;
        order_two[31] = 0x7f;
        for r in [order_two, [0u8; 32]] {
            let k = Scalar::from_bytes_mod_order_wide(&sha512(&[&r, &a_octets, &msg]));
            let mut sig = [0u8; 64];
            sig[..32].copy_from_slice(&r);
            sig[32..].copy_from_slice((k * a).as_bytes());
            assert_eq!(
                authenticate(with_signature(&env, &sig), &keys)
                    .unwrap_err()
                    .error,
                ErrorCode::SignatureInvalid
            );
        }
    }

    #[test]
    fn form_failures_name_their_own_requirement() {
        let alice = identity("principal-a");
        let keys = TrustedKeySet::new(&alice);
        let env = alice.sign_envelope(draft());
        let text = String::from_utf8(env.octets().to_vec()).unwrap();
        let swap = |from: &str, to: &str| receive(text.replace(from, to).as_bytes());
        let nonce = env.security().nonce();
        let rej = authenticate(swap(nonce, &format!("{nonce}==")), &keys).unwrap_err();
        assert_eq!(rej.requirement, "SEC-SIG-003");
        let sig = env.security().signature();
        // Standard base64's '+' is not in the base64url alphabet.
        let rej = authenticate(swap(sig, &format!("+{}", &sig[1..])), &keys).unwrap_err();
        assert_eq!(rej.requirement, "SEC-SIG-004");
        let rej = authenticate(swap(sig, &format!("{sig}A")), &keys).unwrap_err();
        assert_eq!(rej.requirement, "SEC-SIG-004");
        let huge = format!("{},\"x-huge\":1e400}}", &text[..text.len() - 1]);
        let rej = authenticate(receive(huge.as_bytes()), &keys).unwrap_err();
        assert_eq!(
            (rej.error, rej.requirement),
            (ErrorCode::SignatureInvalid, "SEC-SIG-013")
        );
        let rej = authenticate(swap("principal-a", "principal-b"), &keys).unwrap_err();
        assert_eq!(
            (rej.error, rej.requirement),
            (ErrorCode::UnknownKey, "SEC-KEY-030")
        );
    }

    #[test]
    fn signed_envelope_verifies_under_the_signers_entry() {
        let alice = identity("principal-a");
        let bob = identity("principal-b");
        let mut keys = TrustedKeySet::new(&bob);
        keys.add_paired_key(alice.principal().clone(), *alice.public_key())
            .unwrap();
        let env = alice.sign_envelope(draft());
        let msg = authenticate(receive(env.octets()), &keys).unwrap();
        assert_eq!(msg.verified_by(), Some(&alice.security_principal()));
        // [SEC-STG-004]: an envelope signed by the receiver's own key goes through it too.
        let own = bob.sign_envelope(draft());
        assert_eq!(
            authenticate(receive(own.octets()), &keys)
                .unwrap()
                .verified_by(),
            Some(&bob.security_principal())
        );
        // Two signings use two fresh nonces.
        assert_ne!(
            alice.sign_envelope(draft()).security().nonce(),
            env.security().nonce()
        );
    }

    #[test]
    fn unknown_key_comes_before_signature() {
        let alice = identity("principal-a");
        let keys = TrustedKeySet::new(&identity("principal-b"));
        let env = alice.sign_envelope(draft());
        let rej = authenticate(receive(env.octets()), &keys).unwrap_err();
        assert_eq!(
            (rej.state, rej.error),
            (DeliveryState::Rejected, ErrorCode::UnknownKey)
        );
    }

    #[test]
    fn tampering_and_out_of_range_numbers_fail_at_step_2() {
        let alice = identity("principal-a");
        let keys = TrustedKeySet::new(&alice);
        let env = alice.sign_envelope(draft());
        let text = String::from_utf8(env.octets().to_vec()).unwrap();
        let tampered = text.replace("trust me", "trust us");
        let rej = authenticate(receive(tampered.as_bytes()), &keys).unwrap_err();
        assert_eq!(rej.error, ErrorCode::SignatureInvalid);
        // N5: an unrecognized member holding a number beyond the double range passes the
        // envelope stage but has no signing input, so the signature cannot verify.
        let huge = format!("{},\"x-huge\":1e400}}", &text[..text.len() - 1]);
        let msg = receive(huge.as_bytes());
        assert!(msg.envelope().signing_input().is_err());
        let rej = authenticate(msg, &keys).unwrap_err();
        assert_eq!(
            (rej.state, rej.error),
            (DeliveryState::Rejected, ErrorCode::SignatureInvalid)
        );
    }

    #[test]
    fn other_domains_refuse_a_security_member_that_is_not_an_object() {
        let alice = identity("principal-a");
        let keys = TrustedKeySet::new(&alice);
        for text in [
            r#"{"x":1,"security":"s"}"#,
            r#"{"x":1,"security":[]}"#,
            r#"{"x":1}"#,
            r#"{"x":1,"security":{"principal":"p","key_id":"k"}}"#,
            r#"{"x":1,"security":{"principal":"p","key_id":"k","signature":"s","nonce":"n"}}"#,
            r#"{"x":1,"security":{"principal":"p","key_id":"k","signature":7}}"#,
        ] {
            let obj = parse(text.as_bytes()).unwrap();
            for domain in [
                SigningDomain::Registration,
                SigningDomain::Receipt,
                SigningDomain::Presence,
                SigningDomain::Agreement,
            ] {
                assert_eq!(
                    verify_signed(&keys, domain, obj.as_object().unwrap()),
                    Err(VerifyError::MalformedSecurity),
                    "{text}"
                );
            }
        }
    }

    #[test]
    fn a_signature_does_not_cross_domains() {
        let alice = identity("principal-a");
        let keys = TrustedKeySet::new(&alice);
        let mut obj = JsonObject::new();
        obj.insert("x", "y".into());
        let mut sec = JsonObject::new();
        sec.insert("principal", alice.principal().as_str().into());
        sec.insert("key_id", alice.key_id().as_str().into());
        obj.insert("security", Json::Object(sec.clone()));
        let sig = alice.sign_object(SigningDomain::Receipt, &obj).unwrap();
        sec.insert("signature", sig.as_str().into());
        obj.insert("security", Json::Object(sec));
        assert!(verify_signed(&keys, SigningDomain::Receipt, &obj).is_ok());
        for other in [
            SigningDomain::Registration,
            SigningDomain::Presence,
            SigningDomain::Agreement,
        ] {
            assert_eq!(
                verify_signed(&keys, other, &obj),
                Err(VerifyError::SignatureInvalid)
            );
        }
    }
}
