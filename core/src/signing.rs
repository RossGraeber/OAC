// SPDX-License-Identifier: Apache-2.0

//! Signature verification (`spec/security.md` §6.3) and steps 1 and 2 of the security stage
//! (§7.1, Table 7.1): key resolution and signature.
//!
//! Steps 3 to 5 (replay window, authorization, duplicate suppression) are tasks F4 and F5.
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
//!    `core/tests/conformance.rs` runs every `sec-sig` fixture through that call alone.
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
        SigningDomain::Registration | SigningDomain::Receipt | SigningDomain::Presence => {
            &["principal", "key_id", "signature"]
        }
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
    let sec = obj
        .get("security")
        .and_then(Json::as_object)
        .ok_or(VerifyError::MalformedSecurity)?;
    let members = security_members(domain);
    if sec.len() != members.len() || members.iter().any(|m| !sec.contains(m)) {
        return Err(VerifyError::MalformedSecurity);
    }
    let member = |m| {
        sec.get(m)
            .and_then(Json::as_str)
            .ok_or(VerifyError::MalformedSecurity)
    };
    let (principal, key_id, signature) = (
        member("principal")?,
        member("key_id")?,
        member("signature")?,
    );
    let nonce = match domain {
        SigningDomain::Envelope => Some(member("nonce")?),
        _ => None,
    };
    // Step 1: key resolution.
    let entry = keys
        .resolve(principal, key_id)
        .ok_or(VerifyError::UnknownKey)?;
    // Step 2: forms, signing input, signature.
    if nonce.is_some_and(|n| base64url::decode_exact::<16>(n).is_none()) {
        return Err(VerifyError::SignatureInvalid); // [SEC-SIG-003]
    }
    let octets = base64url::decode_exact::<64>(signature).ok_or(VerifyError::SignatureInvalid)?; // [SEC-SIG-004]
    let input = signing_input(domain, obj).map_err(|_| VerifyError::SignatureInvalid)?;
    entry
        .public_key()
        .verifying_key()
        .verify_strict(&input, &ed25519_dalek::Signature::from_bytes(&octets))
        .map_err(|_| VerifyError::SignatureInvalid)?;
    Ok(entry)
}

/// Why the security stage did not pass an envelope: the delivery state, the Table 7.1 code
/// and, for a log line, the requirement that failed. The code is the earliest failing
/// step's ([SEC-STG-002]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecurityRejection {
    /// `rejected`, for steps 1 and 2.
    pub state: DeliveryState,
    /// `unknown-key` or `signature-invalid`.
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
/// ([IFC-TYP-042]). Steps 3 to 5 come next (F4, F5).
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
    let entry = match verify_signed(keys, SigningDomain::Envelope, msg.envelope().as_json()) {
        Ok(entry) => entry.security_principal(),
        Err(VerifyError::UnknownKey) => return Err(reject(ErrorCode::UnknownKey, "SEC-KEY-030")),
        Err(VerifyError::SignatureInvalid) => {
            return Err(reject(ErrorCode::SignatureInvalid, "SEC-SIG-024"));
        }
        // Envelope-stage validation guarantees the `security` shape ([SC-ENV-070] to
        // [SC-ENV-073]), so this does not occur for a received envelope.
        Err(VerifyError::MalformedSecurity) => {
            return Err(reject(ErrorCode::MalformedEnvelope, "SC-ENV-070"));
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
        for other in [SigningDomain::Registration, SigningDomain::Presence] {
            assert_eq!(
                verify_signed(&keys, other, &obj),
                Err(VerifyError::SignatureInvalid)
            );
        }
    }
}
