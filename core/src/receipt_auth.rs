// SPDX-License-Identifier: Apache-2.0

//! Receipts between implementations (#55, F6): the authenticated receipt of
//! `spec/security.md` §10.1, how a receiver issues one, and how a sending implementation
//! accepts one (§10.2) before it may hold a receiver-observed state ([SC-RCP-040]).
//!
//! A receiver decides first whether to send a receipt at all
//! ([`crate::receiver::may_send_receipt`]: never for an unverified envelope, at most one
//! `duplicate` receipt per store entry, rate-limited per sending device). Sending one is
//! optional ([SC-RCP-042]).

use crate::authorization::{AuthorizationEngine, Binding};
use crate::canonical::SigningDomain;
use crate::delivery::Observer;
use crate::envelope::Envelope;
use crate::ids::{KeyId, SessionId};
use crate::json::{Json, JsonObject};
use crate::keys::DeviceIdentity;
use crate::receipt::DeliveryReceipt;
use crate::sender::ObservedReceipt;
use crate::signing::verify_signed;
use crate::transport::{Destination, Payload, PayloadKind};

/// An authenticated receipt (§10.1). It keeps the object as received or built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedReceipt {
    wire: JsonObject,
}

impl AuthenticatedReceipt {
    /// The receipt `receipt` about `env`, signed with `identity` under `oac-receipt-v1`,
    /// with `envelope_to` and `envelope_nonce` taken from `env` ([SEC-RCT-001],
    /// [SEC-RCT-002]).
    pub fn issue(
        identity: &DeviceIdentity,
        receipt: &DeliveryReceipt,
        env: &Envelope,
    ) -> AuthenticatedReceipt {
        let mut o = JsonObject::new();
        o.insert("receipt", Json::Object(receipt.as_json().clone()));
        o.insert("envelope_to", env.to().as_str().into());
        o.insert("envelope_nonce", env.security().nonce().into());
        let mut sec = JsonObject::new();
        sec.insert("principal", identity.principal().as_str().into());
        sec.insert("key_id", identity.key_id().as_str().into());
        o.insert("security", Json::Object(sec.clone()));
        let signature = identity
            .sign_object(SigningDomain::Receipt, &o)
            .expect("a receipt holds strings only, so it canonicalizes");
        sec.insert("signature", signature.as_str().into());
        o.insert("security", Json::Object(sec));
        AuthenticatedReceipt { wire: o }
    }

    /// Wraps a received value. Any object is kept; [`accept_receipt`] checks it.
    pub fn from_json(v: &Json) -> Option<AuthenticatedReceipt> {
        Some(AuthenticatedReceipt {
            wire: v.as_object()?.clone(),
        })
    }

    /// The object as held.
    pub fn as_json(&self) -> &JsonObject {
        &self.wire
    }

    /// The `receipt` payload for the device whose key verified the envelope
    /// (`spec/interfaces.md` Table 6.1).
    pub fn to_payload(&self, sender_device: KeyId) -> (Destination, Payload) {
        (
            Destination::Device(sender_device),
            Payload::new(
                PayloadKind::Receipt,
                Json::Object(self.wire.clone()).to_compact().into_bytes(),
            ),
        )
    }
}

/// Why a sending implementation discarded an authenticated receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptDiscard {
    /// Not an object of `receipt`, `envelope_to`, `envelope_nonce` and `security`, or a
    /// `security` other than `principal`, `key_id` and `signature`.
    Malformed,
    /// The inner receipt fails §8.1.4 ([SC-RCP-032]).
    InvalidReceipt,
    /// No trusted key, or the signature does not verify (checks 1 and 2).
    Signature,
    /// No record of having sent that envelope (check 3).
    NotSent,
    /// `envelope_to` is not bound to the signing key, or is under conflict (check 4).
    WrongReceiver,
    /// The receipt's observer is not `receiver` (check 5).
    NotReceiver,
}

/// The checks of [SEC-RCT-003] on an authenticated receipt, against `engine`: its trusted
/// key set (checks 1 and 2), its sent records, which carry each envelope's nonce (check 3),
/// and its binding table (check 4). On success, the receipt as a
/// receiver-observed state the sending implementation may hold ([SC-RCP-040]).
///
/// # Errors
///
/// The first check that fails.
pub fn accept_receipt(
    ar: &AuthenticatedReceipt,
    engine: &AuthorizationEngine,
) -> Result<ObservedReceipt, ReceiptDiscard> {
    let o = &ar.wire;
    let s = |m| o.get(m).and_then(Json::as_str);
    let (Some(inner), Some(to), Some(nonce)) =
        (o.get("receipt"), s("envelope_to"), s("envelope_nonce"))
    else {
        return Err(ReceiptDiscard::Malformed);
    };
    let receipt = DeliveryReceipt::from_json(inner).map_err(|_| ReceiptDiscard::InvalidReceipt)?;
    // Checks 1 and 2.
    let key = match verify_signed(engine.trusted_keys(), SigningDomain::Receipt, o) {
        Ok(e) => e.key_id().clone(),
        Err(crate::signing::VerifyError::MalformedSecurity) => {
            return Err(ReceiptDiscard::Malformed);
        }
        Err(_) => return Err(ReceiptDiscard::Signature),
    };
    // Check 3.
    let sent = SessionId::parse(receipt.envelope_from().as_str()).is_some_and(|from| {
        engine.sent_records_from(&from).any(|r| {
            r.id == *receipt.envelope_id()
                && r.from.as_str() == receipt.envelope_from().as_str()
                && r.to.as_str() == to
                && r.nonce.as_deref() == Some(nonce)
        })
    });
    if !sent {
        return Err(ReceiptDiscard::NotSent);
    }
    // Check 4: a conflict mark binds no key.
    let bound = SessionId::parse(to).and_then(|sid| engine.binding(&sid).cloned());
    if bound != Some(Binding::Key(key)) {
        return Err(ReceiptDiscard::WrongReceiver);
    }
    // Check 5.
    if receipt.observer() != Observer::Receiver {
        return Err(ReceiptDiscard::NotReceiver);
    }
    Ok(ObservedReceipt::new(receipt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authorization::{MemoryDecisionLog, OperatorConfirmed, SentRecord};
    use crate::clock::ManualClock;
    use crate::delivery::DeliveryState;
    use crate::envelope::{EnvelopeDraft, TextPart};
    use crate::ids::{Timestamp, Token};
    use crate::keys::DeviceKey;
    use crate::pairing::{MemoryPairingStore, PairedPeer};
    use std::sync::Arc;

    #[test]
    fn round_trip_and_checks() {
        let ts = |s| Timestamp::parse(s).unwrap();
        let (alice, bob) = (
            DeviceIdentity::new(DeviceKey::generate(), Token::parse("a").unwrap()),
            DeviceIdentity::new(DeviceKey::generate(), Token::parse("b").unwrap()),
        );
        let env = alice.sign_envelope(
            EnvelopeDraft::new(
                Token::parse("m1").unwrap(),
                SessionId::parse("01harn7x9k2m4p6q8r0s2t4v6w").unwrap(),
                SessionId::parse("7gq3m8z2c5k9t1w4x6b0n2r8vd").unwrap(),
                ts("2026-10-03T12:00:00Z"),
                vec![TextPart::new("hi").unwrap()],
            )
            .unwrap(),
        );
        let receipt = DeliveryReceipt::new(
            env.id().clone(),
            env.from().clone().into(),
            DeliveryState::HandedToHarness,
            Observer::Receiver,
            None,
            ts("2026-10-03T12:00:01Z"),
        )
        .unwrap();
        let ar = AuthenticatedReceipt::issue(&bob, &receipt, &env);
        let clock = Arc::new(ManualClock::new(ts("2026-10-03T12:00:01Z")));
        let store = MemoryPairingStore::new();
        let mut e = AuthorizationEngine::new(&alice, clock, Box::new(MemoryDecisionLog::new()));
        let peer = PairedPeer::by_key_id_comparison(
            bob.principal().clone(),
            *bob.public_key(),
            bob.key_id(),
            ts("2026-10-03T00:00:00Z"),
            OperatorConfirmed::by_operator(),
        )
        .unwrap();
        e.pair(peer, &store).unwrap();
        // Not sent yet: check 3.
        assert_eq!(accept_receipt(&ar, &e), Err(ReceiptDiscard::NotSent));
        // A sent record with no nonce matches no receipt (check 3), even with `to` bound to
        // the signer.
        e.record_sent(SentRecord {
            nonce: None,
            ..SentRecord::of(&env, bob.key_id().clone())
        });
        e.bind(env.to(), bob.key_id());
        assert_eq!(accept_receipt(&ar, &e), Err(ReceiptDiscard::NotSent));
        let mut e = AuthorizationEngine::new(
            &alice,
            Arc::new(ManualClock::new(ts("2026-10-03T12:00:01Z"))),
            Box::new(MemoryDecisionLog::new()),
        );
        let peer = PairedPeer::by_key_id_comparison(
            bob.principal().clone(),
            *bob.public_key(),
            bob.key_id(),
            ts("2026-10-03T00:00:00Z"),
            OperatorConfirmed::by_operator(),
        )
        .unwrap();
        e.pair(peer, &store).unwrap();
        e.record_sent(SentRecord::of(&env, bob.key_id().clone()));
        // Sent, but `to` not bound to Bob: check 4.
        assert_eq!(accept_receipt(&ar, &e), Err(ReceiptDiscard::WrongReceiver));
        e.bind(env.to(), bob.key_id());
        let got = accept_receipt(&ar, &e).unwrap();
        assert_eq!(got.receipt(), &receipt);
        let (dest, p) = ar.to_payload(alice.key_id().clone());
        assert_eq!(dest, Destination::Device(alice.key_id().clone()));
        assert_eq!(p.kind(), PayloadKind::Receipt);
    }
}
