// SPDX-License-Identifier: Apache-2.0

//! Device keys and pairing (06 row 8; `spec/security.md` §5.3, §13 "Trust on first use"):
//! removing a key revokes everything it held in one step, a key becomes trusted only through
//! an operator-confirmed pairing, and a party in the middle of the pairing exchange is
//! caught by the code the operator compares.

use oac_core::authorization::{OperatorConfirmed, RemoveKeyError};
use oac_core::delivery::{DeliveryState, ErrorCode};
use oac_core::keys::{DeviceIdentity, DeviceKey, SecretSeed};
use oac_core::pairing::{
    ConfirmError, PAIRING_MAX_ATTEMPTS, PairedPeer, PairingEnd, PairingError, PairingInitiator,
    PairingStore, ResponderPairing,
};
use oac_core::presence_auth::{
    AuthenticatedPresenceRecord, PresenceAuthDiscard, accept_authenticated_record,
};
use oac_core::registry::PresenceRegistry;
use oac_core::transport::CarrierHandle;
use oac_security_suite::{Device, T0, announcement, granted_pair, identity, sid, token, ts};

fn unknown_key(d: &oac_security_suite::Delivery) {
    assert_eq!(
        d.outcome(),
        (DeliveryState::Rejected, Some(ErrorCode::UnknownKey)),
        "{d:?}"
    );
    assert!(d.handed.is_none());
}

/// 06 row 8 ([SEC-KEY-035]): removing a leaked key takes it out of the trusted key set and,
/// in the same step, every grant and binding that names it, and saves that. Its next message
/// is refused at step 1.
#[test]
fn row08_removing_a_key_revokes_it_in_one_step() {
    let (alice, mut bob) = granted_pair();
    assert_eq!(
        bob.receive(alice.sign("m1", &sid(1), &sid(2), "hi").octets())
            .state,
        DeliveryState::HandedToHarness
    );
    let removal = bob
        .engine
        .remove_key(
            &alice.key_id(),
            OperatorConfirmed::by_operator(),
            &bob.pairings,
        )
        .expect("removed");
    assert_eq!(removal.grants.len(), 1);
    assert_eq!(removal.bindings, vec![sid(1)]);
    assert!(bob.engine.trusted_keys().get(&alice.key_id()).is_none());
    assert!(bob.engine.grants().is_empty());
    assert_eq!(bob.engine.binding(&sid(1)), None);
    let saved = bob.pairings.load().expect("loads");
    assert!(saved.paired.is_empty() && saved.grants.is_empty());
    unknown_key(&bob.receive(alice.sign("m2", &sid(1), &sid(2), "still here?").octets()));
}

/// 06 row 8: an envelope the attacker captured, or signed with the leaked key, before the
/// removal is refused after it, inside the replay window.
#[test]
fn row08_captured_envelope_from_a_removed_key_is_rejected() {
    let (alice, mut bob) = granted_pair();
    let captured = alice.sign("m1", &sid(1), &sid(2), "signed before the removal");
    bob.engine
        .remove_key(
            &alice.key_id(),
            OperatorConfirmed::by_operator(),
            &bob.pairings,
        )
        .unwrap();
    unknown_key(&bob.receive(captured.octets()));
}

/// 06 row 8: the removal is in the pairing store, so a restart brings back neither the key
/// nor its grants.
#[test]
fn row08_removal_survives_a_restart() {
    let (alice, mut bob) = granted_pair();
    bob.engine
        .remove_key(
            &alice.key_id(),
            OperatorConfirmed::by_operator(),
            &bob.pairings,
        )
        .unwrap();
    bob.restart();
    assert!(bob.engine.trusted_keys().get(&alice.key_id()).is_none());
    assert!(bob.engine.grants().is_empty());
    unknown_key(&bob.receive(alice.sign("m1", &sid(1), &sid(2), "after restart").octets()));
}

/// 06 row 8: when the store fails to save the removal, it still stands in memory (fail
/// closed), and the operator is told.
#[test]
fn row08_a_failed_save_still_revokes() {
    let (alice, mut bob) = granted_pair();
    bob.pairings.fail_saves(true);
    let r = bob.engine.remove_key(
        &alice.key_id(),
        OperatorConfirmed::by_operator(),
        &bob.pairings,
    );
    assert!(matches!(r, Err(RemoveKeyError::NotSaved(_))), "{r:?}");
    unknown_key(&bob.receive(alice.sign("m1", &sid(1), &sid(2), "hi").octets()));
}

/// 06 row 8 ([SEC-KEY-031]): a device's own key cannot be removed, so revocation cannot be
/// turned against the device's own sessions.
#[test]
fn row08_own_key_cannot_be_removed() {
    let (_alice, mut bob) = granted_pair();
    let own = bob.key_id();
    assert_eq!(
        bob.engine
            .remove_key(&own, OperatorConfirmed::by_operator(), &bob.pairings),
        Err(RemoveKeyError::OwnKey)
    );
}

/// 06 row 8 ([SEC-KEY-004]): the private seed is never disclosed in debug output, the
/// first place a seed leaks into logs.
#[test]
fn row08_device_key_never_appears_in_debug_output() {
    let mut octets = [0x5a; 32];
    let seed = SecretSeed::from_octets(&mut octets);
    let key = DeviceKey::from_seed(&seed);
    let id = DeviceIdentity::new(DeviceKey::from_seed(&seed), token("alice"));
    let dev = Device::with_identity(
        DeviceIdentity::new(DeviceKey::from_seed(&seed), token("alice")),
        oac_security_suite::clock(),
    );
    let shown = format!("{seed:?} {key:?} {id:?} {:?}", dev.engine);
    for leak in ["5a5a5a5a", "5A5A5A5A", "WlpaWlpa", "90, 90, 90", "[90"] {
        assert!(!shown.contains(leak), "the seed shows as {leak}: {shown}");
    }
}

/// `spec/security.md` §13 "Trust on first use" and pairing ([SEC-KEY-032]; finding F5-1): a
/// party on the exchange channel that runs one exchange with each device, substituting its
/// own key, leaves the two devices showing different codes, so the code the operator copies
/// from one device into the other is refused, and the device that would have paired shows
/// the substituted key's id. The control, with no one in the middle, matches and pairs.
///
/// What this does not show: with independent random nonces the two codes differ whether or
/// not the code also binds the keys, so the key binding itself is proven by the core's
/// deterministic unit test `pairing::tests::substituted_key_or_nonce_changes_the_code_or_fails`
/// (cited in the threat map), which fixes the nonces through seams that stay crate-private.
///
/// The codes are six digits: two different exchanges show the same code with probability
/// 10^-6 (the documented per-exchange residual), so this test fails spuriously with that
/// probability.
#[test]
fn s13_pairing_mitm_substitution_is_caught_by_the_code() {
    let now = ts(T0);
    let (alice, bob, mallory) = (identity("alice"), identity("bob"), identity("mallory"));

    // Control: Alice and Bob directly.
    let (init, offer) = PairingInitiator::start(&alice, now.clone());
    let mut resp = ResponderPairing::start(now.clone());
    let answer = resp.answer(&bob, offer, &now).unwrap();
    let (mut a_session, reveal) = init.receive(answer, &now).unwrap();
    let b_session = resp.reveal(reveal, &now).unwrap();
    assert_eq!(a_session.code(), b_session.code());
    assert_eq!(a_session.peer_key_id(), *bob.key_id());
    let peer = a_session.confirm_entered(b_session.code(), &now).unwrap();
    assert_eq!(peer.key_id(), *bob.key_id());

    // Mallory in the middle: she answers Alice's offer as a responder, and offers to Bob as
    // an initiator, each with her own key.
    let (init, offer_a) = PairingInitiator::start(&alice, now.clone());
    let mut m_resp = ResponderPairing::start(now.clone());
    let to_alice = m_resp.answer(&mallory, offer_a, &now).unwrap();
    let (mut a_session, _reveal_a) = init.receive(to_alice, &now).unwrap();
    let (m_init, offer_m) = PairingInitiator::start(&mallory, now.clone());
    let mut b_resp = ResponderPairing::start(now.clone());
    let to_mallory = b_resp.answer(&bob, offer_m, &now).unwrap();
    let (_m_session, reveal_m) = m_init.receive(to_mallory, &now).unwrap();
    let b_session = b_resp.reveal(reveal_m, &now).unwrap();
    assert_eq!(
        a_session.peer_key_id(),
        *mallory.key_id(),
        "the substituted key"
    );
    assert_ne!(
        a_session.code(),
        b_session.code(),
        "the operator sees two codes"
    );
    assert!(matches!(
        a_session.confirm_entered(b_session.code(), &now),
        Err(ConfirmError::Mismatch { .. })
    ));
}

/// Pairing ([SEC-KEY-032]; PR #316 review N1): a responder answers one offer per
/// operator-started pairing, so a party that restarts the exchange to try for another code
/// ends the pairing visibly instead.
#[test]
fn s13_second_offer_ends_the_pairing() {
    let now = ts(T0);
    let (alice, bob, mallory) = (identity("alice"), identity("bob"), identity("mallory"));
    let mut resp = ResponderPairing::start(now.clone());
    let (_i, offer) = PairingInitiator::start(&mallory, now.clone());
    resp.answer(&bob, offer, &now).unwrap();
    let (_j, again) = PairingInitiator::start(&alice, now.clone());
    assert_eq!(
        resp.answer(&bob, again, &now).map(|_| ()),
        Err(PairingError::Ended(PairingEnd::SecondOffer))
    );
    assert_eq!(resp.ended(&now), Some(PairingEnd::SecondOffer));
}

/// Pairing (C5 §10(b)): five wrong code entries abort the session, after which even the
/// right code is refused; and a code confirmed after 120 seconds is refused.
#[test]
fn s13_five_wrong_codes_abort_and_the_window_expires() {
    let now = ts(T0);
    let (alice, bob) = (identity("alice"), identity("bob"));
    let exchange = || {
        let (init, offer) = PairingInitiator::start(&alice, now.clone());
        let mut resp = ResponderPairing::start(now.clone());
        let answer = resp.answer(&bob, offer, &now).unwrap();
        let (a, reveal) = init.receive(answer, &now).unwrap();
        let b = resp.reveal(reveal, &now).unwrap();
        (a, b)
    };
    let (mut a, b) = exchange();
    let wrong = if b.code() == "000000" {
        "000001"
    } else {
        "000000"
    };
    for left in (0..PAIRING_MAX_ATTEMPTS).rev() {
        let r = a.confirm_entered(wrong, &now);
        assert_eq!(
            r.map(|_| ()),
            Err(ConfirmError::Mismatch {
                attempts_left: left
            })
        );
    }
    assert_eq!(
        a.confirm_entered(b.code(), &now).map(|_| ()),
        Err(ConfirmError::Aborted)
    );
    let (mut a, b) = exchange();
    let late = ts("2026-10-07T12:02:00Z");
    assert_eq!(
        a.confirm_entered(b.code(), &late).map(|_| ()),
        Err(ConfirmError::Expired)
    );
}

/// Pairing, the file-exchange form ([SEC-KEY-032]): the operator confirms the key id read
/// from the peer device, so a key substituted on the way does not match it.
#[test]
fn s13_key_id_comparison_refuses_a_substituted_key() {
    let (alice, mallory) = (identity("alice"), identity("mallory"));
    let substituted = PairedPeer::by_key_id_comparison(
        token("alice"),
        *mallory.public_key(),
        alice.key_id(),
        ts(T0),
        OperatorConfirmed::by_operator(),
    );
    assert!(substituted.is_none());
}

/// `spec/security.md` §13 "Trust on first use" ([SEC-KEY-033]): no signature makes a key
/// trusted. An unpaired device's envelope and presence record are both refused, and the
/// trusted key set is unchanged after them.
#[test]
fn s13_a_signature_alone_never_makes_a_key_trusted() {
    let (_alice, mut bob) = granted_pair();
    let mallory = Device::new("mallory", bob.clock.clone());
    let before = bob.engine.trusted_keys().len();
    unknown_key(&bob.receive(mallory.sign("m1", &sid(8), &sid(2), "trust me").octets()));
    let record = AuthenticatedPresenceRecord::issue(
        &mallory.identity,
        &announcement(&sid(8), 1, bob.clock_now(), 60_000),
        &bob.key_id(),
    );
    let mut registry = PresenceRegistry::new();
    let out = accept_authenticated_record(
        &record,
        &mut bob.engine,
        &mut registry,
        CarrierHandle::from_opaque(b"l".to_vec()),
        std::time::Instant::now(),
    );
    assert_eq!(out.result, Err(PresenceAuthDiscard::Signature));
    assert_eq!(bob.engine.trusted_keys().len(), before);
    assert!(bob.engine.trusted_keys().get(&mallory.key_id()).is_none());
}
