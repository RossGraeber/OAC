// SPDX-License-Identifier: Apache-2.0

//! Replay and duplicates (06 row 4; `spec/security.md` §8): a captured envelope is refused
//! outside the replay window, handed off at most once inside it, and draws a bounded number
//! of receipts. The restart residual ([SEC-RPL-025]) is asserted as what it is.

use std::time::{Duration, Instant};

use oac_core::delivery::{DeliveryState, ErrorCode};
use oac_core::receiver::{ReceiptLimiter, ReceiptRefusal};
use oac_security_suite::{T0, granted_pair, sid, ts};

const NANOS_PER_SEC: i128 = 1_000_000_000;

/// 06 row 4 ([SEC-RPL-001], [SEC-RPL-002]): a validly signed envelope with no `ttl_ms`,
/// replayed more than 300 seconds after its `created_at`, is refused at security step 3
/// with `outside-replay-window`, before authorization and before the duplicate store.
#[test]
fn row04_copy_outside_the_replay_window_is_rejected() {
    let (alice, mut bob) = granted_pair();
    let env = alice.sign_at("m1", &sid(1), &sid(2), ts(T0), "captured");
    bob.clock.advance_nanos(301 * NANOS_PER_SEC);
    let d = bob.receive(env.octets());
    assert_eq!(
        d.outcome(),
        (DeliveryState::Expired, Some(ErrorCode::OutsideReplayWindow))
    );
    assert!(d.handed.is_none() && !d.looked_up);
    assert!(
        bob.duplicates.is_empty(),
        "no store entry for a refused copy"
    );
}

/// 06 row 4 ([SEC-RPL-002]): the window is two-sided; a copy dated more than 300 seconds
/// ahead of the receiver clock is refused too, so an attacker cannot pre-date a capture
/// to outlive the store.
#[test]
fn row04_copy_dated_ahead_of_the_window_is_rejected() {
    let (alice, mut bob) = granted_pair();
    let env = alice.sign_at("m1", &sid(1), &sid(2), ts("2026-10-07T12:05:01Z"), "early");
    let d = bob.receive(env.octets());
    assert_eq!(
        d.outcome(),
        (DeliveryState::Expired, Some(ErrorCode::OutsideReplayWindow))
    );
    assert!(d.handed.is_none());
}

/// 06 row 4 ([SEC-RPL-002], [SEC-RPL-003]): the window is open at its ends and compared at
/// full precision. A copy dated exactly 300 seconds ahead of the receiver clock is refused,
/// and one dated a nanosecond less is accepted, so the window is neither widened nor closed
/// at its edge. The ahead side is probed because on the past side the hand-off deadline
/// would refuse a copy at the edge anyway; `replay::tests::window_is_open_at_both_ends_at_full_precision`
/// in `oac-core` probes both ends of the window function itself.
#[test]
fn row04_the_window_is_open_at_its_edge() {
    let (alice, mut bob) = granted_pair();
    let at = |offset: i128| {
        oac_core::ids::Timestamp::from_unix_nanos(ts(T0).unix_nanos() + offset).expect("a time")
    };
    let edge = alice.sign_at(
        "m1",
        &sid(1),
        &sid(2),
        at(300 * NANOS_PER_SEC),
        "at the edge",
    );
    assert_eq!(
        bob.receive(edge.octets()).outcome(),
        (DeliveryState::Expired, Some(ErrorCode::OutsideReplayWindow))
    );
    let inside = alice.sign_at(
        "m2",
        &sid(1),
        &sid(2),
        at(300 * NANOS_PER_SEC - 1),
        "just inside",
    );
    assert_eq!(
        bob.receive(inside.octets()).state,
        DeliveryState::HandedToHarness
    );
}

/// `spec/security.md` §13 "Duplicate suppression that blocks a legitimate retransmission"
/// ([SEC-RPL-022], [SC-RCP-009]): a copy that was not handed off, because the session was
/// not accepting input or because the hand-off call failed, leaves no store entry, so its
/// retransmission is delivered rather than reported `duplicate`.
#[test]
fn s13_a_copy_not_handed_off_does_not_block_its_retransmission() {
    let (alice, mut bob) = granted_pair();
    let env = alice.sign("m1", &sid(1), &sid(2), "please arrive");
    let failed = bob.receive_with(env.octets(), oac_core::adapter::HandOffOutcome::Failed);
    assert_eq!(failed.state, DeliveryState::Failed);
    let again = bob.receive(env.octets());
    assert_eq!(again.state, DeliveryState::HandedToHarness, "{again:?}");

    let env = alice.sign("m2", &sid(1), &sid(2), "session away");
    bob.end_session(&sid(2));
    let away = bob.receive(env.octets());
    assert_eq!(away.state, DeliveryState::Unreachable);
    bob.register(&sid(2), "/work/b");
    let back = bob.receive(env.octets());
    assert_eq!(back.state, DeliveryState::HandedToHarness, "{back:?}");
}

/// 06 row 4 ([SEC-RPL-020], [SEC-RPL-021], [SEC-RPL-030]): inside the window, the store keyed
/// by `(key_id, nonce)` hands the envelope off once; every later copy is `duplicate`, and
/// only the first of them may draw a `duplicate` receipt.
#[test]
fn row04_replay_inside_the_window_is_a_duplicate_handed_off_once() {
    let (alice, mut bob) = granted_pair();
    let env = alice.sign("m1", &sid(1), &sid(2), "once");
    let first = bob.receive(env.octets());
    assert_eq!(first.state, DeliveryState::HandedToHarness);
    let mut handed = 1;
    for (n, receipt_allowed) in [(2, true), (3, false), (4, false)] {
        bob.clock.advance_nanos(10 * NANOS_PER_SEC);
        let d = bob.receive(env.octets());
        assert_eq!(
            d.outcome(),
            (DeliveryState::Duplicate, Some(ErrorCode::Duplicate)),
            "copy {n}"
        );
        handed += usize::from(d.handed.is_some());
        assert_eq!(
            d.report.unwrap().duplicate_receipt_allowed(),
            Some(receipt_allowed),
            "copy {n}"
        );
    }
    assert_eq!(handed, 1, "handed off exactly once");
}

/// 06 row 4 (`spec/session-channels.md` §8.3.2, `ttl_ms` before the window): a copy past
/// both its expiry and the window is reported `expired` with `expired`, never with
/// `outside-replay-window`.
#[test]
fn row04_expiry_is_checked_before_the_replay_window() {
    let (alice, mut bob) = granted_pair();
    let env = alice.sign("m1", &sid(1), &sid(2), "short-lived");
    bob.clock.advance_nanos(301 * NANOS_PER_SEC);
    let d = bob.receive(env.octets());
    assert_eq!(
        d.outcome(),
        (DeliveryState::Expired, Some(ErrorCode::Expired))
    );
    assert!(d.handed.is_none());
}

/// 06 row 4 residual ([SEC-RPL-025]; 06 §7 "cold-restart gap"): the duplicate store lives in
/// memory, so after a restart a copy still inside the window is handed off again. This is
/// the named, accepted residual, asserted so that a change to it is seen. The window itself
/// still holds across the restart.
#[test]
fn row04_replay_after_restart_inside_the_window_is_the_named_residual() {
    let (alice, mut bob) = granted_pair();
    let env = alice.sign_at("m1", &sid(1), &sid(2), ts(T0), "again");
    assert_eq!(
        bob.receive(env.octets()).state,
        DeliveryState::HandedToHarness
    );
    assert_eq!(bob.receive(env.octets()).state, DeliveryState::Duplicate);
    bob.restart();
    assert_eq!(
        bob.receive(env.octets()).state,
        DeliveryState::HandedToHarness,
        "the residual: delivery is not exactly-once across a restart"
    );
    bob.restart();
    bob.clock.advance_nanos(301 * NANOS_PER_SEC);
    assert_eq!(
        bob.receive(env.octets()).outcome(),
        (DeliveryState::Expired, Some(ErrorCode::OutsideReplayWindow)),
        "outside the window the restart opens nothing"
    );
}

/// 06 row 4, receipt flooding ([SEC-RPL-030], [SEC-RPL-031]): replays inside the window draw
/// one `duplicate` receipt in all, and replays outside it draw no more receipts than the
/// sending device's allowance, however many arrive at once.
#[test]
fn row04_receipt_flooding_by_replay_is_bounded() {
    let (alice, mut bob) = granted_pair();
    let now = Instant::now();
    let mut limiter = ReceiptLimiter::new(3, Duration::from_secs(1), 16);
    let env = alice.sign_at("m1", &sid(1), &sid(2), ts(T0), "flood");
    let mut receipts = 0;
    for _ in 0..10 {
        let d = bob.receive(env.octets());
        receipts += usize::from(d.receipt_decision(&mut limiter, now).is_ok());
    }
    assert_eq!(receipts, 2, "one for the hand-off, one duplicate receipt");

    let mut limiter = ReceiptLimiter::new(3, Duration::from_secs(1), 16);
    bob.clock.advance_nanos(301 * NANOS_PER_SEC);
    let mut refusals = Vec::new();
    for _ in 0..10 {
        let d = bob.receive(env.octets());
        assert_eq!(d.error, Some(ErrorCode::OutsideReplayWindow));
        if let Err(r) = d.receipt_decision(&mut limiter, now) {
            refusals.push(r);
        }
    }
    assert_eq!(refusals.len(), 7, "three receipts, the device's burst");
    assert!(refusals.iter().all(|r| *r == ReceiptRefusal::RateLimited));
}
