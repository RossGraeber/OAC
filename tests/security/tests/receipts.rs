// SPDX-License-Identifier: Apache-2.0

//! Receipt forgery (`spec/security.md` §10, §13 "Receipt forgery"): a sender holds a
//! receiver-observed state only from an authenticated receipt that passes every check of
//! [SEC-RCT-003], and a receiver sends none for a copy it could not verify.

use std::time::{Duration, Instant};

use oac_core::authorization::SentRecord;
use oac_core::delivery::{DeliveryState, Observer};
use oac_core::envelope::Envelope;
use oac_core::json;
use oac_core::receipt::DeliveryReceipt;
use oac_core::receipt_auth::{AuthenticatedReceipt, ReceiptDiscard, accept_receipt};
use oac_core::receiver::{ReceiptLimiter, ReceiptRefusal};
use oac_security_suite::{Device, granted_pair, rewrite, sid};

/// Alice sent `m1` to Bob's session, recorded it, and binds Bob's session to Bob's key.
fn sent() -> (Device, Device, Envelope) {
    let (mut alice, mut bob) = granted_pair();
    let env = alice.sign("m1", &sid(1), &sid(2), "hello");
    assert_eq!(
        bob.receive(env.octets()).state,
        DeliveryState::HandedToHarness
    );
    alice.engine.record_sent(SentRecord::of(&env, bob.key_id()));
    alice.engine.bind(&sid(2), &bob.key_id());
    (alice, bob, env)
}

fn receipt(
    env: &Envelope,
    state: DeliveryState,
    observer: Observer,
    at: &Device,
) -> DeliveryReceipt {
    DeliveryReceipt::new(
        env.id().clone(),
        env.from().clone().into(),
        state,
        observer,
        None,
        at.clock_now(),
    )
    .expect("a receipt")
}

fn handed(env: &Envelope, at: &Device) -> DeliveryReceipt {
    receipt(env, DeliveryState::HandedToHarness, Observer::Receiver, at)
}

/// The control: Bob's own receipt is accepted.
fn assert_control(alice: &Device, bob: &Device, env: &Envelope) {
    let ar = AuthenticatedReceipt::issue(&bob.identity, &handed(env, bob), env);
    assert!(accept_receipt(&ar, &alice.engine).is_ok(), "control");
}

/// `spec/security.md` §13 ([SEC-RCT-003] checks 1-2): a receipt signed by a key the sender
/// never paired is discarded.
#[test]
fn s13_receipt_from_an_unpaired_key_is_discarded() {
    let (alice, bob, env) = sent();
    let mallory = Device::new("mallory", alice.clock.clone());
    let ar = AuthenticatedReceipt::issue(&mallory.identity, &handed(&env, &bob), &env);
    assert_eq!(
        accept_receipt(&ar, &alice.engine).map(|_| ()),
        Err(ReceiptDiscard::Signature)
    );
    assert_control(&alice, &bob, &env);
}

/// [SEC-RCT-003] check 4: a receipt signed by another trusted device, not the one the
/// addressed session is bound to, is discarded.
#[test]
fn s13_receipt_from_another_trusted_device_is_discarded() {
    let (mut alice, bob, env) = sent();
    let carol = Device::new("carol", alice.clock.clone());
    alice.pair(&carol.identity);
    let ar = AuthenticatedReceipt::issue(&carol.identity, &handed(&env, &bob), &env);
    assert_eq!(
        accept_receipt(&ar, &alice.engine).map(|_| ()),
        Err(ReceiptDiscard::WrongReceiver)
    );
    assert_control(&alice, &bob, &env);
}

/// [SEC-RCT-001]: the receipt is signed whole, so a node on the path that turns a `failed`
/// into `handed-to-harness`, or anything else, gets it discarded.
#[test]
fn s13_altered_receipt_is_discarded() {
    let (alice, bob, env) = sent();
    let unknown = receipt(&env, DeliveryState::Unknown, Observer::Receiver, &bob);
    let ar = AuthenticatedReceipt::issue(&bob.identity, &unknown, &env);
    let wire = json::Json::Object(ar.as_json().clone()).to_compact();
    let altered = rewrite(wire.as_bytes(), "\"unknown\"", "\"handed-to-harness\"");
    let forged = AuthenticatedReceipt::from_json(&json::parse(&altered).unwrap()).unwrap();
    assert_eq!(
        accept_receipt(&forged, &alice.engine).map(|_| ()),
        Err(ReceiptDiscard::Signature)
    );
}

/// [SEC-RCT-003] check 3: a receipt about an envelope the sender never sent is discarded,
/// even when signed by the right receiver.
#[test]
fn s13_receipt_for_an_envelope_never_sent_is_discarded() {
    let (alice, bob, _env) = sent();
    let other = alice.sign("m9", &sid(1), &sid(2), "never sent");
    let ar = AuthenticatedReceipt::issue(&bob.identity, &handed(&other, &bob), &other);
    assert_eq!(
        accept_receipt(&ar, &alice.engine).map(|_| ()),
        Err(ReceiptDiscard::NotSent)
    );
}

/// [SEC-RCT-002], [SEC-RCT-003] check 3: the receipt names the envelope's nonce, so a
/// receipt about another envelope with the same `id`, `from` and `to` does not settle the
/// one that was sent.
#[test]
fn s13_receipt_with_the_wrong_nonce_is_discarded() {
    let (alice, bob, env) = sent();
    let twin = alice.sign("m1", &sid(1), &sid(2), "hello");
    assert_ne!(twin.security().nonce(), env.security().nonce());
    let ar = AuthenticatedReceipt::issue(&bob.identity, &handed(&twin, &bob), &twin);
    assert_eq!(
        accept_receipt(&ar, &alice.engine).map(|_| ()),
        Err(ReceiptDiscard::NotSent)
    );
    assert_control(&alice, &bob, &env);
}

/// [SEC-RCT-003] check 5: a receiver cannot speak for the sender's side; a receipt whose
/// observer is `sender` is discarded.
#[test]
fn s13_receipt_claiming_the_sender_observer_is_discarded() {
    let (alice, bob, env) = sent();
    let r = receipt(
        &env,
        DeliveryState::AcceptedByAdapter,
        Observer::Sender,
        &bob,
    );
    let ar = AuthenticatedReceipt::issue(&bob.identity, &r, &env);
    assert_eq!(
        accept_receipt(&ar, &alice.engine).map(|_| ()),
        Err(ReceiptDiscard::NotReceiver)
    );
}

/// [SC-RCP-041], [SEC-RCT-005]: a receiver sends no receipt for a copy that failed
/// verification, so a forged envelope cannot be used to make it sign receipts for an
/// address the attacker chooses.
#[test]
fn s13_no_receipt_for_an_unverified_copy() {
    let (alice, mut bob) = granted_pair();
    let mallory = Device::new("mallory", bob.clock.clone());
    let mut limiter = ReceiptLimiter::new(8, Duration::from_secs(1), 8);
    let now = Instant::now();
    let forged = bob.receive(mallory.sign("m1", &sid(1), &sid(2), "hi").octets());
    assert_eq!(
        forged.receipt_decision(&mut limiter, now),
        Err(ReceiptRefusal::NotVerified)
    );
    let env = alice.sign("m2", &sid(1), &sid(2), "hi");
    let tampered = bob.receive(&rewrite(env.octets(), "\"hi\"", "\"ho\""));
    assert_eq!(
        tampered.receipt_decision(&mut limiter, now),
        Err(ReceiptRefusal::NotVerified)
    );
    let good = bob.receive(env.octets());
    assert_eq!(good.receipt_decision(&mut limiter, now), Ok(()));
}
