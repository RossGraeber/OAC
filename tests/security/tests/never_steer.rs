// SPDX-License-Identifier: Apache-2.0

//! Permission relay and steering (06 rows 11 and 12; `spec/security.md` §9.6): a delivery
//! permit authorizes delivery into the session's input and nothing else. Relaying a
//! permission answer is a decision of its own, off unless an operator turns it on for one
//! session, and no decision of any kind enables steering a running turn. The Claude
//! adapter's half of row 11 (it declares no permission relay) is in `provenance.rs`, with
//! the other tests that drive that adapter.

mod codex_scene;

use codex_scene::{Fake, Scene, TUI, array, array_len, at, js, queue_only_violations};
use oac_core::adapter::{HandOffOutcome, ProviderAdapter};
use oac_core::authorization::OperatorConfirmed;
use oac_core::authorization::{AuthorizationRequest, Basis, Grant, Kind, LocalSide, PeerSide};
use oac_core::delivery::{DeliveryState, ErrorCode};
use oac_core::ids::SessionId;
use oac_core::json::{self, Json};
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

// ---- 06 row 12, adapter half: the Codex adapter hands off queue-only ---------------------
//
// [SEC-AUZ-025] to [SEC-AUZ-027], [MCPB-CDX-002] to [MCPB-CDX-005]: every delivery is one
// `thread/queue/add` with the three members of the hand-off, whatever state the thread is
// in, and nothing is sent in its place after a refusal. The tests drive the real
// `CodexAdapter` (a dev-dependency since #68; `scripts/check-crate-deps.mjs` rule 5, as
// `provenance.rs` does for the Claude adapter's row 11 half) against the fake Codex
// app-server process (F9), and read what the adapter sent from the fake's call log, which
// records every request before the fake decides anything and flags `turn/start` and
// `turn/steer` as steering whatever the thread's state. The scene is the adapter's own
// integration scene (`adapters/codex/tests/adapter.rs`), reduced to what a delivery needs:
// a stand-in for the core that issues a pairing value and binds, a carrier to the fake, and
// a shim connection through which the "Codex" session makes the one refused call that
// reveals its thread.

/// 06 row 12, adapter half ([MCPB-CDX-002], [SEC-AUZ-025], [SEC-AUZ-026]): a delivery to an
/// idle thread is one `thread/queue/add` of the three allowed members, and nothing else
/// hands off. The queue starts the turn (the fake dispatches the head at once, as the S3
/// capture recorded), so the adapter needs no idle check and has no `turn/start` to race.
#[test]
fn row12_codex_delivery_to_an_idle_thread_is_one_queue_add() {
    let s = Scene::new();
    let mut sess = s.session();
    s.pair(&mut sess);
    let running = s.running_turn(&sess);
    s.fake.end_turn(&sess.thread, "completed", "ok");
    assert_eq!(at(&s.state(&sess), "activeTurnId"), &Json::Null);

    let h = s.hand_off(&sess, "idle delivery");
    assert_eq!(s.adapter.deliver(h), HandOffOutcome::Completed);

    s.assert_queue_only(1);
    // The head went off the queue into a turn of its own: the thread is running a turn the add
    // started; the message is in that turn's items and not in the earlier turn's.
    let state = s.state(&sess);
    assert_eq!(
        array_len(at(&state, "queue")),
        0,
        "taken off the queue at once"
    );
    let started = at(&state, "activeTurnId")
        .as_str()
        .expect("the add started a turn")
        .to_owned();
    s.fake.end_turn(&sess.thread, "completed", "ok");
    let turns = s.state(&sess);
    let items_of = |id: &str| {
        array(at(&turns, "turns"))
            .iter()
            .find(|t| at(t, "id").as_str() == Some(id))
            .map(|t| at(t, "items").to_compact())
            .unwrap_or_else(|| panic!("no turn {id}"))
    };
    assert!(items_of(&started).contains("idle delivery"));
    assert!(!items_of(&running).contains("idle delivery"));
    sess.shim.end();
}

/// 06 row 12, adapter half ([MCPB-CDX-002], [MCPB-CDX-004], [SEC-AUZ-025]): a delivery to a
/// thread with a running turn is one `thread/queue/add`; it waits, the running turn is not
/// touched, and its input arrives in a new turn after `turn/completed`, never under the
/// running turn's id.
#[test]
fn row12_codex_delivery_to_a_busy_thread_waits_in_the_queue_and_never_joins_the_turn() {
    let s = Scene::new();
    let mut sess = s.session();
    s.pair(&mut sess);
    let running = s.running_turn(&sess);

    let h = s.hand_off(&sess, "busy delivery");
    assert_eq!(s.adapter.deliver(h), HandOffOutcome::Completed);

    s.assert_queue_only(1);
    let state = s.state(&sess);
    assert_eq!(at(&state, "activeTurnId").as_str(), Some(running.as_str()));
    assert_eq!(array_len(at(&state, "queue")), 1, "held for the idle");
    assert!(
        !at(&state, "turns").to_compact().contains("busy delivery"),
        "nothing joined a turn"
    );
    // The running turn ends; only then does the queue start a turn, a new one.
    let done = s.fake.end_turn(&sess.thread, "completed", "ok");
    let next = at(&done, "dispatched").as_str().expect("the queue's turn");
    assert_ne!(
        next, running,
        "the input is not under the running turn's id"
    );
    let next = next.to_owned();
    s.fake.end_turn(&sess.thread, "completed", "ok");
    let turns = s.state(&sess);
    let items_of = |id: &str| {
        array(at(&turns, "turns"))
            .iter()
            .find(|t| at(t, "id").as_str() == Some(id))
            .map(|t| at(t, "items").to_compact())
            .unwrap_or_else(|| panic!("no turn {id}"))
    };
    assert!(!items_of(&running).contains("busy delivery"));
    assert!(items_of(&next).contains("busy delivery"));
    s.assert_queue_only(1);
    sess.shim.end();
}

/// 06 row 12, adapter half ([MCPB-CDX-003], [SEC-AUZ-026]): a turn the harness's own user
/// starts after the thread went idle is running when the delivery comes, and the delivery is
/// still one `thread/queue/add` that waits behind it. This shows the outcome of that
/// situation, not the race: the adapter makes no idle check, so what it had observed of the
/// thread's state when it sent the add (idle, or the new turn started) is not something a
/// test can set, and cannot change what it sends. That it sends no `turn/start` whatever it
/// observed is [MCPB-CDX-003] and what the other tests check.
#[test]
fn row12_codex_delivery_when_a_turn_starts_just_after_an_idle_status_still_only_queues() {
    let s = Scene::new();
    let mut sess = s.session();
    s.pair(&mut sess);
    s.fake.end_turn(&sess.thread, "completed", "ok");
    s.fake
        .tui_turn(&sess.thread, "the harness user types at once");
    let running = s.running_turn(&sess);

    let h = s.hand_off(&sess, "raced delivery");
    assert_eq!(s.adapter.deliver(h), HandOffOutcome::Completed);

    s.assert_queue_only(1);
    let state = s.state(&sess);
    assert_eq!(at(&state, "activeTurnId").as_str(), Some(running.as_str()));
    assert_eq!(array_len(at(&state, "queue")), 1);
    assert!(!at(&state, "turns").to_compact().contains("raced delivery"));
    sess.shim.end();
}

/// 06 row 12, adapter half ([MCPB-CDX-002]): a delivery to an idle thread whose last turn
/// ended interrupted is still one `thread/queue/add`. The harness holds it (nothing is
/// dispatched after an interrupt), and the adapter does not start a turn to get it through;
/// it runs once a turn completes uninterrupted.
#[test]
fn row12_codex_delivery_to_an_idle_thread_after_an_interrupted_turn_only_queues() {
    let s = Scene::new();
    let mut sess = s.session();
    s.pair(&mut sess);
    s.fake.end_turn(&sess.thread, "interrupted", "half an ans");
    let state = s.state(&sess);
    assert_eq!(at(&state, "lastTurnInterrupted").as_bool(), Some(true));
    assert_eq!(at(&state, "activeTurnId"), &Json::Null);

    let h = s.hand_off(&sess, "after the interrupt");
    assert_eq!(s.adapter.deliver(h), HandOffOutcome::Completed);

    s.assert_queue_only(1);
    let state = s.state(&sess);
    assert_eq!(array_len(at(&state, "queue")), 1, "held, not dispatched");
    assert_eq!(
        at(&state, "activeTurnId"),
        &Json::Null,
        "no turn was started"
    );
    // A turn the harness's user starts and that completes uninterrupted releases it.
    s.fake.tui_turn(&sess.thread, "the harness user goes on");
    let done = s.fake.end_turn(&sess.thread, "completed", "ok");
    assert!(at(&done, "dispatched").as_str().is_some(), "{done:?}");
    s.assert_queue_only(1);
    sess.shim.end();
}

/// 06 row 12, adapter half ([SEC-AUZ-027], [MCPB-CDX-003], [MCPB-CDX-004]): when the add is
/// turned away (here the recorded archived-thread refusal) the adapter reports the hand-off
/// failed and sends nothing in its place: no `turn/start`, no `turn/steer`, no second add of
/// the same message. A later, separate delivery is again one add.
#[test]
fn row12_codex_a_turned_away_add_is_not_followed_by_any_other_hand_off() {
    let s = Scene::new();
    let mut sess = s.session();
    s.pair(&mut sess);
    s.fake.end_turn(&sess.thread, "completed", "ok");
    s.fake.archive(&sess.thread, true);

    let out = s
        .adapter
        .deliver(s.hand_off(&sess, "to an archived thread"));
    assert_eq!(out, HandOffOutcome::Failed);
    assert_eq!(out.recorded().1, Some(ErrorCode::HandoffFailed));
    // Exactly the refused add: the one hand-off call, answered with the recorded refusal.
    let report = s.report();
    s.assert_queue_only(1);
    let refusal: Vec<String> = array(at(&report, "calls"))
        .iter()
        .filter(|c| at(c, "method").as_str() == Some("thread/queue/add"))
        .map(|c| {
            at(c, "error.message")
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert_eq!(refusal.len(), 1);
    assert!(refusal[0].contains("is archived"), "{refusal:?}");

    s.fake.archive(&sess.thread, false);
    let again = s.adapter.deliver(s.hand_off(&sess, "after unarchive"));
    assert_eq!(again, HandOffOutcome::Completed);
    s.assert_queue_only(2);
    sess.shim.end();
}

/// 06 row 12, adapter half ([MCPB-CDX-005]): the negative cases. The check the tests above
/// apply to the adapter's calls reports a `turn/start`, a `turn/steer`, and a
/// `thread/queue/add` that carries a thread setting such as `approvalPolicy`; so a regression
/// in the adapter into any of them would fail those tests, not pass. Three such calls are
/// made by a second client of the same fake (the "TUI user"), which the fake logs and flags
/// as it does the adapter's.
#[test]
fn row12_codex_a_steering_call_or_a_setting_member_is_reported_nonconformant() {
    let fake = Fake::spawn();
    let thread = fake.create_thread();
    // turn/start on an idle thread, and again on a running one (steers it).
    fake.tui_turn(&thread, "first");
    fake.tui_turn(&thread, "joins the running turn");
    // turn/steer is answered NOT_MODELLED; it is logged and flagged regardless.
    let _ = fake.call(
        "turn/steer",
        &format!(
            "{{\"threadId\":{},\"expectedTurnId\":\"t\",\"input\":[{{\"type\":\"text\",\"text\":\"steer\"}}]}}",
            js(&thread)
        ),
    );
    // An add with a thread setting beside the three members of the hand-off.
    let _ = fake.call(
        "thread/queue/add",
        &format!(
            "{{\"threadId\":{},\"input\":[{{\"type\":\"text\",\"text\":\"q\"}}],\"clientUserMessageId\":\"c1\",\"approvalPolicy\":\"never\"}}",
            js(&thread)
        ),
    );
    let report = fake.report(TUI);
    let violations = queue_only_violations(&report, 1);
    for needle in ["turn/start", "turn/steer", "approvalPolicy"] {
        assert!(
            violations.iter().any(|v| v.contains(needle)),
            "{needle} not reported: {violations:?}"
        );
    }
    assert_eq!(
        array_len(at(&report, "steering")),
        3,
        "the fake flagged each"
    );
    assert_eq!(array_len(at(&report, "overrideMembers")), 1);
    // And the same check passes a hand-off that is exactly one conformant add.
    let ok = json::parse(
        b"{\"calls\":[{\"method\":\"thread/queue/add\",\"params\":{\"threadId\":\"t\",\"input\":[],\"clientUserMessageId\":\"c\"}}],\"steering\":[],\"overrideMembers\":[],\"malformed\":[],\"control\":[]}",
    )
    .expect("JSON");
    assert_eq!(queue_only_violations(&ok, 1), Vec::<String>::new());
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
