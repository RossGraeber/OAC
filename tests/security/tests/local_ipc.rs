// SPDX-License-Identifier: Apache-2.0

//! Local attachments (06 rows 13, 19 and 24; `spec/security.md` [SEC-AUZ-030];
//! `spec/session-channels.md` §6.7): the daemon's local IPC endpoint, its OS peer check,
//! and how a native session signal is bound to an attachment.
//!
//! The binding decision is the core's (#331): every `native-signal` event goes through
//! `spec/session-channels.md` §6.7 in [`Pipelines`](oac_core::pipeline::Pipelines), which
//! the `row19_` and `row24_` tests drive through the stub adapter. The pairing key there is
//! a value the test supplies as the core process's observation
//! ([`Pipelines::connect_observed`](oac_core::pipeline::Pipelines::connect_observed)). The
//! daemon, its IPC and the OS facility that yields that key are G9 (#70), and whether one
//! exists on every platform is UNVERIFIED (§6.7.2); the `gated_` tests hold their place,
//! ignored, and fail if run.

use oac_core::adapter::{AdapterEvent, Attachment, NativeSignal, SendRequestResult, StartKind};
use oac_core::delivery::ErrorCode;
use oac_core::session_binding::{
    BindingResult, MemoryBindingLog, PairingKey, PeerObservation, RecordKind,
};
use oac_security_suite::pipeline::{PipelineDevice, send_request};
use oac_security_suite::{identity, token};
use oac_transport_memory::MemoryNetwork;

/// 06 row 13 ([SEC-AUZ-030]): the IPC endpoint admits only a peer whose OS-asserted user
/// equals the daemon's (named-pipe security descriptors and `GetNamedPipeClientProcessId`
/// on Windows; `SO_PEERCRED` / `getpeereid()` on Unix).
#[test]
#[ignore = "GATED on #70 (G9, daemon, MCP shims and authenticated local IPC)"]
fn gated_row13_ipc_admits_only_the_same_user() {
    std::panic!("GATED on #70: connect as another user and assert the daemon refuses the peer");
}

/// 06 row 19: a session's lifetime follows its IPC connection, so a registration held open
/// after the harness session ended binds nothing.
#[test]
#[ignore = "GATED on #70 (G9, daemon, MCP shims and authenticated local IPC)"]
fn gated_row19_session_lifetime_follows_the_ipc_connection() {
    std::panic!("GATED on #70: drop the shim's connection and assert the session ends with it");
}

/// 06 row 24, the daemon's part: a real shim, started with a spoofed session variable,
/// reaches the daemon over the G9 IPC and binds nothing, with the pairing key observed by
/// the daemon from the operating system (C4 §3, revision 2026-10-02).
#[test]
#[ignore = "GATED on #70 (G9, daemon, MCP shims and authenticated local IPC)"]
fn gated_row24_spoofed_session_variable_binds_nothing() {
    std::panic!(
        "GATED on #70: start a shim with a spoofed session variable and assert it binds nothing"
    );
}

// ---- the core's binding decision (#331) ----------------------------------------------------

/// What the core process observed about a peer: the harness process `key` it descends from.
fn observed(key: &str) -> PeerObservation {
    PeerObservation {
        pairing_key: Some(PairingKey::from_observation(key.as_bytes().to_vec())),
        harness_label: Some(token("harness")),
        working_directory: Some("/work/b".into()),
    }
}

/// A shim's attachment: observed with `key` (or nothing), reporting the session variable
/// `variable` as its cross-check value.
fn shim(bob: &PipelineDevice, key: Option<&str>, variable: &str) -> Attachment {
    bob.observed_attachment(
        key.map_or_else(PeerObservation::none, observed),
        Some(variable),
    )
}

/// A hook payload: native id `n`, arriving on `on`.
fn hook(on: &Attachment, n: &str, s: StartKind) -> AdapterEvent {
    AdapterEvent::NativeSignal(NativeSignal {
        native_id: n.into(),
        start_kind: Some(s),
        cross_check: None,
        connection: Some(on.clone()),
    })
}

fn device() -> (PipelineDevice, MemoryBindingLog) {
    let bob = PipelineDevice::new(&MemoryNetwork::new(), identity("bob"));
    let log = MemoryBindingLog::new(64);
    bob.pipes.set_binding_log(Box::new(log.clone()));
    (bob, log)
}

fn findings(log: &MemoryBindingLog) -> Vec<&'static str> {
    log.entries()
        .iter()
        .filter(|e| e.record.kind == RecordKind::Finding)
        .map(|e| e.record.requirement)
        .collect()
}

/// 06 row 24 ([SC-ID-121], [SC-ID-122], [SC-ID-125]): the session variable is never a
/// pairing key. A shim whose variable names the victim's conversation binds nothing, and
/// the victim's hook payload pairs with the victim's shim by the observed key alone. With
/// no observed key, which is every connection until G9 supplies one, even a matching
/// payload fails closed with a finding.
#[test]
fn row24_a_spoofed_session_variable_is_never_a_pairing_key() {
    let (bob, log) = device();
    let victim = shim(&bob, Some("claude-1"), "conv-x");
    let spoof = shim(&bob, Some("intruder-7"), "conv-x");
    bob.adapter.emit(hook(&victim, "conv-x", StartKind::Fresh));
    assert!(bob.pipes.binding(&victim).is_some());
    assert_eq!(bob.pipes.binding(&spoof), None);
    assert_eq!(bob.adapter.told(&spoof), None);

    let (bob, log2) = device();
    let unobserved = shim(&bob, None, "conv-x");
    bob.adapter
        .emit(hook(&unobserved, "conv-x", StartKind::Fresh));
    assert_eq!(bob.pipes.binding(&unobserved), None);
    assert_eq!(findings(&log2), ["SC-ID-129"]);
    assert!(log.entries().is_empty(), "the victim bound under case 3(a)");
}

/// 06 row 24 ([SC-ID-131] to [SC-ID-133]): a newcomer whose hook id is already bound to a
/// live shim is refused with a finding and never displaces the existing binding, which
/// keeps its session and its send right.
#[test]
fn row24_a_newcomer_never_displaces_a_bound_hook_id() {
    let (bob, log) = device();
    let victim = shim(&bob, Some("claude-1"), "conv-x");
    bob.adapter.emit(hook(&victim, "conv-x", StartKind::Fresh));
    let sv = bob.pipes.binding(&victim).expect("bound");
    let spoof = shim(&bob, Some("intruder-7"), "conv-x");
    bob.adapter.emit(hook(&spoof, "conv-x", StartKind::Fresh));
    assert_eq!(bob.pipes.binding(&spoof), None);
    assert_eq!(bob.pipes.binding(&victim), Some(sv.clone()));
    assert_eq!(bob.adapter.told(&victim), Some(sv));
    assert_eq!(findings(&log), ["SC-ID-132"]);
    assert_eq!(log.entries()[0].result, BindingResult::Refused);
    let r = bob
        .adapter
        .send(send_request(&spoof, &oac_security_suite::sid(9), "hi"));
    assert!(matches!(
        r,
        SendRequestResult::Refused {
            error: ErrorCode::Unauthorized
        }
    ));
}

/// 06 row 24 ([SC-ID-125], [SC-ID-129], [SC-ID-154]): a payload that cannot be paired with
/// certainty, here because two shims descend from the same observed process, binds nothing
/// and records a finding; the bound shim it is attributed to stops sending until a later
/// payload pairs with it.
#[test]
fn row24_an_unpairable_payload_fails_closed() {
    let (bob, log) = device();
    let first = shim(&bob, Some("claude-1"), "conv-x");
    bob.adapter.emit(hook(&first, "conv-x", StartKind::Fresh));
    let s1 = bob.pipes.binding(&first).expect("bound");
    let second = shim(&bob, Some("claude-1"), "conv-y");
    bob.adapter.emit(hook(&second, "conv-y", StartKind::Fresh));
    assert_eq!(bob.pipes.binding(&second), None);
    assert_eq!(findings(&log), ["SC-ID-129", "SC-ID-154"]);
    assert_eq!(bob.adapter.told(&first), None, "withheld");
    let r = bob.adapter.send(send_request(&first, &s1, "hi"));
    assert!(matches!(
        r,
        SendRequestResult::Refused {
            error: ErrorCode::Unauthorized
        }
    ));
}

/// 06 row 24 ([SC-ID-152], [SC-ID-153]): a refused payload for a shim bound to a different
/// hook id also deregisters that shim's record, so it never delivers into a conversation it
/// has left.
#[test]
fn row24_a_refused_payload_deregisters_a_stale_shim() {
    let (bob, log) = device();
    let victim = shim(&bob, Some("claude-1"), "conv-x");
    bob.adapter.emit(hook(&victim, "conv-x", StartKind::Fresh));
    let other = shim(&bob, Some("claude-2"), "conv-m");
    bob.adapter.emit(hook(&other, "conv-m", StartKind::Fresh));
    let stale = bob.pipes.binding(&other).expect("bound");
    bob.adapter
        .emit(hook(&other, "conv-x", StartKind::Transition));
    assert_eq!(bob.pipes.binding(&other), None);
    assert_eq!(bob.adapter.told(&other), None);
    bob.pipes
        .with_engine(|e| assert_eq!(e.binding(&stale), None, "deregistered"));
    assert_eq!(findings(&log), ["SC-ID-132", "SC-ID-153"]);
    assert!(bob.pipes.binding(&victim).is_some(), "untouched");
}

/// 06 row 19 ([SC-ID-140], [SC-ID-150], [SC-ID-151]): a resume reported as a transition
/// binds a new registration under a new session id; the superseded registration is
/// deregistered, so the old id reaches nothing, and the new id inherits none of its grants.
#[test]
fn row19_a_resume_takes_a_new_registration_and_the_old_one_ends() {
    let (bob, _log) = device();
    let a = shim(&bob, Some("claude-1"), "conv-x");
    bob.adapter.emit(hook(&a, "conv-x", StartKind::Fresh));
    let old = bob.pipes.binding(&a).expect("bound");
    let peer = bob.session(&oac_security_suite::sid(2), "/work/b");
    bob.grant(oac_core::authorization::Grant::Inbound {
        writer: oac_core::authorization::PeerSide::session(
            bob.pipes.device().key_id().clone(),
            oac_security_suite::sid(2),
        ),
        target: oac_core::authorization::LocalSide::Session(old.clone()),
    });
    bob.adapter.emit(hook(&a, "conv-y", StartKind::Transition));
    let new = bob.pipes.binding(&a).expect("re-bound");
    assert_ne!(new, old);
    bob.pipes
        .with_engine(|e| assert_eq!(e.binding(&old), None, "superseded record gone"));
    for to in [&old, &new] {
        let r = bob.adapter.send(send_request(&peer, to, "hi"));
        assert!(
            matches!(r, SendRequestResult::Refused { .. }),
            "{to}: {r:?}"
        );
    }
    assert_eq!(bob.adapter.deliver_calls(), 0);
}
