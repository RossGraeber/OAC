// SPDX-License-Identifier: Apache-2.0

//! Issued-value pairing, the core's part (`spec/security.md` §13 "Misattributed send
//! request"; the threat table of `spec/bindings/mcp.md` §4.5.5; G8 slice S2, #69).
//!
//! One Codex process serves several conversations, so every attachment under it has the same
//! operating-system pairing key. The core narrows them with a value it issues on an
//! attachment and finds again in the harness's own report ([SC-ID-125]). These tests drive
//! the core through its public API with the stub adapter standing in for the Codex adapter:
//! they prove the core refuses what the binding's threat table says it must (a forged,
//! replayed, late or foreign reveal; endless pairing windows; an unconfirmed attachment).
//! What the adapter owns, that is, taking the thread id from the notification
//! ([MCPB-ATT-008]), subscribed threads only ([MCPB-ATT-018]) and the per-call confirmation
//! ([MCPB-ATT-013], [MCPB-ATT-014], [MCPB-ATT-021]), stays gated on G8 (#69) in
//! `tests/provenance.rs`.

use std::time::Duration;

use oac_core::adapter::{AdapterEvent, Attachment, NativeSignal, SendRequestResult, StartKind};
use oac_core::delivery::ErrorCode;
use oac_core::session_binding::{MemoryBindingLog, PairingKey, PeerObservation, RecordKind};
use oac_security_suite::pipeline::{PipelineDevice, send_request};
use oac_security_suite::{identity, sid, token};
use oac_transport_memory::{ManualClock, MemoryNetwork};

/// The Codex process `key`, as the core process observed it.
fn observed(key: &str) -> PeerObservation {
    PeerObservation {
        pairing_key: Some(PairingKey::from_observation(key.as_bytes().to_vec())),
        harness_label: Some(token("codex")),
        working_directory: Some("/work/codex".into()),
    }
}

struct Rig {
    bob: PipelineDevice,
    log: MemoryBindingLog,
    clock: ManualClock,
}

fn rig() -> Rig {
    let net = MemoryNetwork::builder().manual_clock().build();
    let clock = net.manual_clock().expect("a manual clock");
    let bob = PipelineDevice::new(&net, identity("bob"));
    let log = MemoryBindingLog::new(64);
    bob.pipes.set_binding_log(Box::new(log.clone()));
    Rig { bob, log, clock }
}

impl Rig {
    /// A thread's OAC server connection: an attachment under the Codex process `key`.
    fn thread(&self, key: &str) -> Attachment {
        self.bob.observed_attachment(observed(key), None)
    }

    /// The pairing value the refusal of a call from `a` carries (`None` for a plain one).
    fn value(&self, a: &Attachment) -> Option<String> {
        let r = self.bob.adapter.send(send_request(a, &sid(9), "x"));
        match &r {
            SendRequestResult::Refused {
                error: ErrorCode::Unauthorized,
                pairing_value,
            } => pairing_value.clone(),
            other => panic!("not an unauthorized refusal: {other:?}"),
        }
    }

    fn reveal(&self, on: &Attachment, thread: &str, value: &str) {
        self.bob
            .adapter
            .emit(AdapterEvent::NativeSignal(NativeSignal {
                native_id: thread.into(),
                start_kind: Some(StartKind::Fresh),
                cross_check: None,
                connection: Some(on.clone()),
                revealed: Some(value.into()),
            }));
    }

    fn requirements(&self) -> Vec<&'static str> {
        self.log
            .entries()
            .iter()
            .map(|e| e.record.requirement)
            .filter(|r| r.starts_with("MCPB-") || r.starts_with("IFC-"))
            .collect()
    }
}

/// Impersonation row ([MCPB-ATT-007], [SC-ID-125]): two conversations of one Codex process
/// share the operating-system key. Each reveals the value issued on its own connection, and
/// each binds that connection and no other, in either order; no connection ends up bound to
/// the other's conversation.
#[test]
fn s13_two_conversations_of_one_process_bind_their_own_connections() {
    let r = rig();
    let carrier = r.bob.carrier(observed("codex-1"));
    let t = r.thread("codex-1");
    let u = r.thread("codex-1");
    let (vt, vu) = (r.value(&t).expect("a value"), r.value(&u).expect("a value"));
    assert_ne!(vt, vu);
    r.reveal(&carrier, "thread-U", &vu);
    assert_eq!(
        r.bob.pipes.binding(&t),
        None,
        "U's reveal binds not T's connection"
    );
    let su = r.bob.pipes.binding(&u).expect("U's connection is bound");
    r.reveal(&carrier, "thread-T", &vt);
    let st = r.bob.pipes.binding(&t).expect("T's connection is bound");
    assert_ne!(st, su);
    assert_eq!(r.bob.pipes.binding(&u), Some(su));
}

/// Forged-reveal row ([MCPB-ATT-006] with the carrier rule): a thread that learned a value
/// and makes a report that carries it can pair only through the adapter's own carrier. A
/// reveal that arrives on an attachment, on a connection the daemon never designated, or on
/// no connection binds nothing, and the value stays good for the real reveal.
#[test]
fn s13_a_reveal_from_a_connection_that_is_not_the_carrier_binds_nothing() {
    let r = rig();
    let carrier = r.bob.carrier(observed("codex-1"));
    let t = r.thread("codex-1");
    let attacker = r.thread("codex-1");
    let v = r.value(&t).expect("a value");
    r.reveal(&attacker, "thread-T", &v);
    r.reveal(&t, "thread-T", &v);
    r.bob.adapter.emit(AdapterEvent::NativeSignal(NativeSignal {
        native_id: "thread-T".into(),
        start_kind: Some(StartKind::Fresh),
        cross_check: None,
        connection: None,
        revealed: Some(v.clone()),
    }));
    assert_eq!(r.bob.pipes.binding(&t), None);
    assert_eq!(r.bob.pipes.binding(&attacker), None);
    assert_eq!(r.requirements(), ["MCPB-ATT-006"; 3]);
    r.reveal(&carrier, "thread-T", &v);
    assert!(r.bob.pipes.binding(&t).is_some());
}

/// Impersonation row, other process ([MCPB-ATT-022]): a connection whose process is not the
/// carrier's harness process gets no value, and no reveal on a carrier of another process
/// binds it. Where there is no operating-system key, no value is issued either.
#[test]
fn s13_a_connection_from_another_process_gets_no_value_and_no_pairing() {
    let r = rig();
    let carrier1 = r.bob.carrier(observed("codex-1"));
    let carrier2 = r.bob.carrier(observed("codex-2"));
    let own = r.thread("codex-1");
    let foreign = r.thread("intruder-7");
    let unobserved = r.bob.observed_attachment(PeerObservation::none(), None);
    assert_eq!(r.value(&foreign), None);
    assert_eq!(r.value(&unobserved), None);
    let v = r.value(&own).expect("a value");
    // The right process, the wrong carrier.
    r.reveal(&carrier2, "thread-T", &v);
    assert_eq!(r.bob.pipes.binding(&own), None);
    // A value the core never issued, however well formed.
    r.reveal(
        &carrier1,
        "thread-T",
        "oac-pair-00000000000000000000000000000000",
    );
    assert_eq!(r.bob.pipes.binding(&foreign), None);
    assert_eq!(r.requirements(), ["MCPB-ATT-022", "MCPB-ATT-007"]);
}

/// Replay row ([MCPB-ATT-010], [MCPB-ATT-009]): a reveal is used once and within its
/// window. A replay of a paired value changes nothing; a reveal after the window pairs
/// nothing; a value of an ended window is never issued again ([MCPB-ATT-026]).
#[test]
fn s13_a_replayed_or_late_reveal_binds_nothing() {
    let r = rig();
    let carrier = r.bob.carrier(observed("codex-1"));
    let t = r.thread("codex-1");
    let late = r.thread("codex-1");
    let v = r.value(&t).expect("a value");
    r.reveal(&carrier, "thread-T", &v);
    let bound = r.bob.pipes.binding(&t).expect("bound");
    r.reveal(&carrier, "thread-T", &v);
    assert_eq!(r.bob.pipes.binding(&t), Some(bound));
    assert_eq!(r.requirements(), ["MCPB-ATT-010"]);

    let v1 = r.value(&late).expect("a value");
    r.clock.advance(Duration::from_secs(61));
    r.reveal(&carrier, "thread-L", &v1);
    assert_eq!(r.bob.pipes.binding(&late), None);
    assert_eq!(r.requirements(), ["MCPB-ATT-010", "MCPB-ATT-009"]);
    assert_ne!(r.value(&late).expect("a new window"), v1);
}

/// Forged-reveal row, residual ([MCPB-ATT-011], [MCPB-ATT-012]): one value revealed for two
/// threads can only mean a second server under the registered name produced it. The
/// connection ends unbound and a finding is recorded; the second thread gets nothing.
#[test]
fn s13_a_value_revealed_for_two_threads_ends_the_binding_with_a_finding() {
    let r = rig();
    let carrier = r.bob.carrier(observed("codex-1"));
    let t = r.thread("codex-1");
    let v = r.value(&t).expect("a value");
    r.reveal(&carrier, "thread-T", &v);
    assert!(r.bob.pipes.binding(&t).is_some());
    r.reveal(&carrier, "thread-U", &v);
    assert_eq!(r.bob.pipes.binding(&t), None);
    assert_eq!(r.bob.adapter.told(&t), None);
    let finding = r.log.entries().last().cloned().expect("a record");
    assert_eq!(finding.record.requirement, "MCPB-ATT-012");
    assert_eq!(finding.record.kind, RecordKind::Finding);
    assert_eq!(finding.attachment, Some(t));
}

/// Endless-retries row ([MCPB-ATT-023], [MCPB-ATT-019]): at most three windows on one
/// connection since the last reveal paired, each at most 60 seconds, and then plain
/// refusals for as long as the connection lasts. A new connection starts again.
#[test]
fn s13_pairing_windows_are_bounded_per_connection() {
    let r = rig();
    let _carrier = r.bob.carrier(observed("codex-1"));
    let t = r.thread("codex-1");
    let mut seen = Vec::new();
    for _ in 0..3 {
        let v = r.value(&t).expect("a value");
        assert!(
            !seen.contains(&v),
            "a value of an ended window is not reused"
        );
        assert_eq!(
            r.value(&t).as_deref(),
            Some(v.as_str()),
            "one value per window"
        );
        seen.push(v);
        r.clock.advance(Duration::from_secs(60));
    }
    assert_eq!(r.value(&t), None, "a fourth window");
    r.clock.advance(Duration::from_secs(3600));
    assert_eq!(r.value(&t), None);
    assert!(r.value(&r.thread("codex-1")).is_some());
}

/// Race and replay rows ([IFC-ADP-091] to [IFC-ADP-093]): when the adapter cannot confirm a
/// request it reports `attachment-unconfirmed` before passing the request. The core stops
/// accepting requests from the attachment and handing off to it, records a finding, and
/// serves it again only after a later reveal is paired with it. The refusal in between
/// carries a fresh pairing value, not the one already used.
#[test]
fn s13_an_unconfirmed_connection_is_refused_until_a_new_reveal_pairs() {
    let r = rig();
    let carrier = r.bob.carrier(observed("codex-1"));
    let t = r.thread("codex-1");
    let v1 = r.value(&t).expect("a value");
    r.reveal(&carrier, "thread-T", &v1);
    let session = r.bob.pipes.binding(&t).expect("bound");
    let peer = r.bob.session(&sid(2), "/work/peer");
    r.bob.grant(oac_core::authorization::Grant::Inbound {
        writer: oac_core::authorization::PeerSide::session(
            r.bob.pipes.device().key_id().clone(),
            sid(2),
        ),
        target: oac_core::authorization::LocalSide::Session(session.clone()),
    });
    r.bob.adapter.emit(AdapterEvent::AttachmentUnconfirmed {
        attachment: t.clone(),
    });
    assert_eq!(r.requirements(), ["IFC-ADP-093"]);
    assert_eq!(r.bob.adapter.told(&t), None);
    // Nothing is accepted from it ...
    let v2 = r.value(&t).expect("a value to pair again");
    assert_ne!(v1, v2);
    // ... and nothing is handed off to it.
    let before = r.bob.adapter.deliver_calls();
    let _ = r
        .bob
        .adapter
        .send(send_request(&peer, &session, "to the stopped one"));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(r.bob.adapter.deliver_calls(), before);
    // A replay of the old reveal lifts nothing; the new one does.
    r.reveal(&carrier, "thread-T", &v1);
    assert_eq!(r.bob.adapter.told(&t), None);
    r.reveal(&carrier, "thread-T", &v2);
    assert_eq!(r.bob.adapter.told(&t), Some(session));
}
