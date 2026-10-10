// SPDX-License-Identifier: Apache-2.0

//! Permission relay and steering (06 rows 11 and 12; `spec/security.md` §9.6): a delivery
//! permit authorizes delivery into the session's input and nothing else. Relaying a
//! permission answer is a decision of its own, off unless an operator turns it on for one
//! session, and no decision of any kind enables steering a running turn. The Claude
//! adapter's half of row 11 (it declares no permission relay) is in `provenance.rs`, with
//! the other tests that drive that adapter.

use oac_core::adapter::HandOffOutcome;
use oac_core::authorization::OperatorConfirmed;
use oac_core::authorization::{AuthorizationRequest, Basis, Grant, Kind, LocalSide, PeerSide};
use oac_core::delivery::DeliveryState;
use oac_core::ids::SessionId;
use oac_security_suite::{Device, granted_pair, paired_pair, sid};

fn relay(bob: &mut Device, s: SessionId) -> bool {
    bob.engine
        .decide(&AuthorizationRequest::RelayPermission { session: s })
        .permits(Kind::RelayPermission)
}

/// 06 row 11 ([SEC-AUZ-021]; Appendix A C10): permission relay is off by default, and a
/// grant, even one over the whole device, does not turn it on.
#[test]
fn row11_relay_is_off_by_default_even_with_a_device_wide_grant() {
    let (alice, mut bob) = paired_pair();
    bob.grant(Grant::Inbound {
        writer: PeerSide::device(alice.key_id()),
        target: LocalSide::Device,
    });
    assert_eq!(
        bob.receive(alice.sign("m1", &sid(1), &sid(2), "approve it").octets())
            .state,
        DeliveryState::HandedToHarness
    );
    assert!(!relay(&mut bob, sid(2)));
}

/// 06 row 11 ([SEC-AUZ-005], [SEC-AUZ-021]): relay is an operator's decision for one
/// session. Enabled for one, it covers no other; disabled, it is off again; and an enable
/// the store failed to save is undone.
#[test]
fn row11_relay_is_enabled_for_one_session_only_by_the_operator() {
    let (_alice, mut bob) = granted_pair();
    bob.register(&sid(3), "/work/b");
    bob.engine
        .set_relay(
            sid(2),
            true,
            OperatorConfirmed::by_operator(),
            &bob.pairings,
        )
        .unwrap();
    let d = bob
        .engine
        .decide(&AuthorizationRequest::RelayPermission { session: sid(2) });
    assert_eq!(d.basis(), Some(&Basis::RelaySetting(sid(2))));
    assert!(!relay(&mut bob, sid(3)), "another session");
    bob.engine
        .set_relay(
            sid(2),
            false,
            OperatorConfirmed::by_operator(),
            &bob.pairings,
        )
        .unwrap();
    assert!(!relay(&mut bob, sid(2)));
    bob.pairings.fail_saves(true);
    assert!(
        bob.engine
            .set_relay(
                sid(3),
                true,
                OperatorConfirmed::by_operator(),
                &bob.pairings
            )
            .is_err()
    );
    assert!(!relay(&mut bob, sid(3)), "an unsaved enable is undone");
}

/// 06 row 11 ([IFC-TYP-082], [SEC-AUZ-020]): a `deliver` permit is never read as a relay
/// permit, even for a session with relay enabled.
#[test]
fn row11_a_deliver_permit_never_permits_relay() {
    let (alice, mut bob) = granted_pair();
    bob.engine
        .set_relay(
            sid(2),
            true,
            OperatorConfirmed::by_operator(),
            &bob.pairings,
        )
        .unwrap();
    let d = bob.receive(alice.sign("m1", &sid(1), &sid(2), "yes, approve").octets());
    let msg = d.handed.expect("handed off");
    let deliver = bob
        .engine
        .decide(&AuthorizationRequest::deliver(&msg).expect("verified"));
    assert!(deliver.permits(Kind::Deliver));
    assert!(!deliver.permits(Kind::RelayPermission));
}

/// 06 row 12 ([SEC-AUZ-022]): the decision kinds are exactly the five of Table 4.9, and
/// none enables steering a running turn. This is a compile-time guard, not a runtime proof:
/// every arm is `false`, so the assertion cannot fail at run time; the match is exhaustive,
/// so adding a kind fails to compile here until this test says what it is. Row 12's runtime
/// proof in this suite is `row12_hand_off_is_made_at_most_once_and_no_outcome_steers`.
#[test]
fn row12_no_decision_kind_enables_steering() {
    let all = [
        Kind::Deliver,
        Kind::Discover,
        Kind::ReleasePresence,
        Kind::AcceptPresence,
        Kind::RelayPermission,
    ];
    for k in all {
        let steering = match k {
            Kind::Deliver
            | Kind::Discover
            | Kind::ReleasePresence
            | Kind::AcceptPresence
            | Kind::RelayPermission => false,
        };
        assert!(!steering, "{k}");
        assert!(!k.as_str().contains("steer") && !k.as_str().contains("turn"));
    }
}

/// 06 row 12 ([IFC-ADP-057], Table 5.3): the core makes the hand-off call at most once per
/// copy, and no hand-off outcome means anything stronger than "handed to the harness": none
/// starts, interrupts or steers a turn. A copy whose hand-off did not happen may be
/// retransmitted, and is again offered once.
#[test]
fn row12_hand_off_is_made_at_most_once_and_no_outcome_steers() {
    let names: Vec<&str> = HandOffOutcome::ALL.iter().map(|o| o.as_str()).collect();
    assert_eq!(
        names,
        ["completed", "not-now", "failed", "indeterminate", "refused"]
    );
    for (n, outcome) in HandOffOutcome::ALL.into_iter().enumerate() {
        let (alice, mut bob) = granted_pair();
        let env = alice.sign(&format!("m{n}"), &sid(1), &sid(2), "steer the turn now");
        let first = bob.receive_with(env.octets(), outcome);
        assert!(first.handed.is_some(), "{outcome:?}: one call");
        assert_eq!(first.outcome(), outcome.recorded());
        let again = bob.receive_with(env.octets(), HandOffOutcome::Completed);
        if outcome.may_be_handed_off() {
            assert!(again.handed.is_none(), "{outcome:?}: never twice");
            assert_eq!(again.state, DeliveryState::Duplicate);
        } else {
            assert!(
                again.handed.is_some(),
                "{outcome:?}: a retransmission is offered"
            );
        }
    }
}

/// 06 row 12, adapter half ([SEC-AUZ-025] to [SEC-AUZ-027]): the Codex adapter hands off
/// queue-only whenever a turn may be running, calls no steering method, and never falls
/// back to one; the fake app-server's call log flags every steering call. Gated: G7. This
/// suite may take an adapter as a dev-dependency since #65 (`scripts/check-crate-deps.mjs`
/// rule 5), as `provenance.rs` does for the Claude adapter's row 11 half.
#[test]
#[ignore = "GATED on #68 (G7, Codex adapter inbound injection): queue-only hand-off is the adapter's"]
fn gated_row12_codex_hand_off_is_queue_only() {
    std::panic!(
        "GATED on #68: drive mid-turn deliveries through the Codex adapter and the fake app-server"
    );
}

/// `spec/security.md` §13 "Bypass of a harness's own consent step" ([SEC-AUZ-023]): no
/// implementation automates past, suppresses or pre-answers a harness's consent prompt.
/// The spec names H2's review of the launch path as the proving test; nothing in the core
/// or the fakes launches a harness. Gated: H2.
#[test]
#[ignore = "GATED on #74 (H2, security verification against the real harnesses)"]
fn gated_s13_no_harness_consent_step_is_automated() {
    std::panic!(
        "GATED on #74: review the OAC-enabled launch path and assert no consent step is answered for the operator"
    );
}
