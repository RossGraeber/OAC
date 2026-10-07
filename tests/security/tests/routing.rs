// SPDX-License-Identifier: Apache-2.0

//! Unauthorized routing and discovery (06 rows 2, 7, 14, 18; `spec/security.md` §13
//! "Existence oracle"): a trusted peer reaches and sees only what a grant or a live reply
//! right covers, scopes are exact, and a refusal says nothing about whether the session
//! exists (acceptance item 4 of #60).

use std::cell::RefCell;
use std::time::{Duration, Instant};

use oac_core::authorization::{
    AuthorizationEngine, AuthorizationRequest, Grant, Kind, LocalSide, PeerSide, Requester,
    SentRecord, WorkingDirectoryScope,
};
use oac_core::delivery::{DeliveryState, ErrorCode};
use oac_core::ids::SessionId;
use oac_core::presence_auth::{AuthenticatedPresenceRecord, accept_authenticated_record};
use oac_core::receiver::ReceiptLimiter;
use oac_core::registry::PresenceRegistry;
use oac_core::transport::CarrierHandle;
use oac_security_suite::{Device, announcement, descriptor, granted_pair, paired_pair, sid, token};

fn unauthorized(d: &oac_security_suite::Delivery) {
    assert_eq!(
        d.outcome(),
        (DeliveryState::Rejected, Some(ErrorCode::Unauthorized)),
        "{d:?}"
    );
    assert!(d.handed.is_none(), "never handed off");
    assert!(!d.looked_up, "the addressed session is never looked up");
}

fn may_discover(engine: &RefCell<&mut AuthorizationEngine>, r: &SessionId, s: &SessionId) -> bool {
    engine
        .borrow_mut()
        .decide(&AuthorizationRequest::Discover {
            requester: Requester::Session(r.clone()),
            session: s.clone(),
        })
        .permits(Kind::Discover)
}

/// 06 row 2 ([SEC-AUZ-001], [SEC-AUZ-002]; acceptance item 4): pairing makes a key trusted
/// and grants nothing. A paired peer with no grant is refused at step 4 with
/// `unauthorized`, although its signature verifies.
#[test]
fn row02_trusted_peer_without_a_grant_is_rejected_unauthorized() {
    let (alice, mut bob) = paired_pair();
    let d = bob.receive(alice.sign("m1", &sid(1), &sid(2), "let me in").octets());
    unauthorized(&d);
    assert_eq!(
        d.verified_by,
        Some(alice.key_id()),
        "verified, yet not authorized"
    );
}

/// 06 row 2 (`spec/security.md` §9.2): a grant is one-way. Alice may write to Bob's
/// session; Bob's session may not write back to Alice's on that grant.
#[test]
fn row02_a_grant_is_one_way() {
    let (mut alice, bob) = granted_pair();
    let d = alice.receive(bob.sign("m1", &sid(2), &sid(1), "writing back").octets());
    unauthorized(&d);
}

/// 06 row 2 ([SEC-AUZ-006]): a grant that names one session covers that session only.
#[test]
fn row02_a_grant_for_one_session_does_not_cover_another() {
    let (alice, mut bob) = granted_pair();
    bob.register(&sid(3), "/work/b");
    unauthorized(&bob.receive(alice.sign("m1", &sid(1), &sid(3), "hi").octets()));
    assert_eq!(
        bob.receive(alice.sign("m2", &sid(1), &sid(2), "hi").octets())
            .state,
        DeliveryState::HandedToHarness
    );
}

/// 06 row 2 ([SEC-AUZ-010], [SEC-AUZ-011]; acceptance item 4): a peer device may discover a
/// session, and be sent its presence record, only when a grant relates them. A paired but
/// ungranted device is denied both; the granted one is permitted.
#[test]
fn row02_unauthorized_peer_cannot_discover_a_session() {
    let (alice, mut bob) = granted_pair();
    let carol = Device::new("carol", bob.clock.clone());
    bob.pair(&carol.identity);
    let ask = |engine: &mut AuthorizationEngine, device: &Device| {
        let k = device.key_id();
        (
            engine
                .decide(&AuthorizationRequest::Discover {
                    requester: Requester::Device(k.clone()),
                    session: sid(2),
                })
                .permits(Kind::Discover),
            engine
                .decide(&AuthorizationRequest::ReleasePresence {
                    session: sid(2),
                    device: k,
                })
                .permits(Kind::ReleasePresence),
        )
    };
    assert_eq!(ask(&mut bob.engine, &carol), (false, false));
    assert_eq!(ask(&mut bob.engine, &alice), (true, true));
}

/// 06 row 2 ([SC-DLV-075], [SC-DLV-076]; acceptance item 4): an own session that may not
/// discover a peer's session cannot address it. The send's presence step answers
/// `unknown-destination` for it exactly as for a session that does not exist, although the
/// registry holds it `online`; the session the grant covers gets its declaration.
#[test]
fn row02_unauthorized_peer_cannot_address_a_session_through_presence() {
    let (mut alice, bob) = granted_pair();
    alice.register(&sid(4), "/work/a");
    let mut registry = PresenceRegistry::new();
    let now = Instant::now();
    let record = AuthenticatedPresenceRecord::issue(
        &bob.identity,
        &announcement(&sid(2), 1, alice.clock_now(), 60_000),
        &alice.key_id(),
    );
    let out = accept_authenticated_record(
        &record,
        &mut alice.engine,
        &mut registry,
        CarrierHandle::from_opaque(b"link".to_vec()),
        now,
    );
    assert!(out.accepted(), "{out:?}");
    let engine = RefCell::new(&mut alice.engine);
    let ask = |from: &SessionId, to: &SessionId| {
        registry
            .send_presence(from, to, |r, s| may_discover(&engine, r, s), now)
            .map(|_| ())
    };
    assert_eq!(ask(&sid(4), &sid(2)), Err(ErrorCode::UnknownDestination));
    assert_eq!(ask(&sid(4), &sid(9)), Err(ErrorCode::UnknownDestination));
    assert_eq!(ask(&sid(1), &sid(2)), Ok(()));
}

/// 06 rows 2 and 18 ([SEC-AUZ-013] to [SEC-AUZ-015]): a reply right covers a reply from the
/// session that was written to, under the key it was sent to, naming that one message in
/// `reply_to`. A reply without it, naming another message, from another session or under
/// another key, is refused.
#[test]
fn row02_reply_right_covers_only_the_reply_to_the_one_message() {
    let (mut alice, mut bob) = granted_pair();
    let carol = Device::new("carol", alice.clock.clone());
    alice.pair(&carol.identity);
    bob.register(&sid(3), "/work/b");
    let sent = alice.sign("m1", &sid(1), &sid(2), "question");
    assert_eq!(
        bob.receive(sent.octets()).state,
        DeliveryState::HandedToHarness
    );
    // The send pipeline records this when it passes the envelope to a transport (#313). It is
    // recorded by hand here: the reply needs two devices, and the in-memory transport keeps
    // presence off between implementations ([IFC-TRN-081]), so a two-device send cannot run
    // through the pipelines over it.
    alice
        .engine
        .record_sent(SentRecord::of(&sent, bob.key_id()));
    let reply = |by: &Device, id: &str, from: &SessionId, reply_to: Option<&str>| {
        let mut d = oac_security_suite::draft(id, from, &sid(1), by.clock_now(), "answer");
        if let Some(r) = reply_to {
            d = d.with_reply_to(token(r));
        }
        by.identity.sign_envelope(d)
    };
    unauthorized(&alice.receive(reply(&bob, "r1", &sid(2), None).octets()));
    unauthorized(&alice.receive(reply(&bob, "r2", &sid(2), Some("m0")).octets()));
    unauthorized(&alice.receive(reply(&bob, "r3", &sid(3), Some("m1")).octets()));
    unauthorized(&alice.receive(reply(&carol, "r4", &sid(2), Some("m1")).octets()));
    let d = alice.receive(reply(&bob, "r5", &sid(2), Some("m1")).octets());
    assert_eq!(d.state, DeliveryState::HandedToHarness, "{d:?}");
}

/// 06 row 7 ([SEC-AUZ-008]): a grant to a working-directory scope covers the sessions
/// registered with exactly that scope, not a session of another project.
#[test]
fn row07_scope_grant_does_not_cover_another_working_directory() {
    let (alice, mut bob) = paired_pair();
    bob.register(&sid(3), "/work/c");
    bob.grant(Grant::Inbound {
        writer: PeerSide::device(alice.key_id()),
        target: LocalSide::Scope(WorkingDirectoryScope::new("/work/b")),
    });
    assert_eq!(
        bob.receive(alice.sign("m1", &sid(1), &sid(2), "hi").octets())
            .state,
        DeliveryState::HandedToHarness
    );
    unauthorized(&bob.receive(alice.sign("m2", &sid(1), &sid(3), "hi").octets()));
}

/// 06 row 7 ([SEC-AUZ-008]): scopes are compared as recorded, so a subdirectory is another
/// scope.
#[test]
fn row07_scope_grant_does_not_cover_a_subdirectory() {
    let (alice, mut bob) = paired_pair();
    bob.register(&sid(3), "/work/b/sub");
    bob.grant(Grant::Inbound {
        writer: PeerSide::device(alice.key_id()),
        target: LocalSide::Scope(WorkingDirectoryScope::new("/work/b")),
    });
    unauthorized(&bob.receive(alice.sign("m1", &sid(1), &sid(3), "hi").octets()));
}

/// 06 row 7 ([SEC-AUZ-011], [SEC-PRS-013]): a session's presence record is released to a
/// device only when a grant covering that session names it, so a project's sessions are
/// not announced to a device granted for another project.
#[test]
fn row07_presence_is_released_only_to_granted_devices() {
    let (alice, mut bob) = paired_pair();
    let carol = Device::new("carol", bob.clock.clone());
    bob.pair(&carol.identity);
    bob.register(&sid(3), "/work/c");
    bob.grant(Grant::Inbound {
        writer: PeerSide::device(alice.key_id()),
        target: LocalSide::Scope(WorkingDirectoryScope::new("/work/b")),
    });
    let mut release = |s: SessionId, d: &Device| {
        bob.engine
            .decide(&AuthorizationRequest::ReleasePresence {
                session: s,
                device: d.key_id(),
            })
            .permits(Kind::ReleasePresence)
    };
    assert!(release(sid(2), &alice));
    assert!(!release(sid(3), &alice), "another project's session");
    assert!(!release(sid(2), &carol), "an ungranted device");
}

/// 06 row 14 ([SC-DLV-060] to [SC-DLV-063], [SEC-AUZ-010]): discovery lists only the
/// sessions the requesting session may write to. A grant over one project's scope lists
/// that project's sessions; another project's online session is never listed, and an
/// attachment bound to no session gets `unauthorized`.
#[test]
fn row14_discovery_lists_only_sessions_the_requester_may_reach() {
    let mut bob = Device::new("bob", oac_security_suite::clock());
    let mut registry = PresenceRegistry::new();
    for (n, wd) in [(2, "/work/b"), (3, "/work/c"), (4, "/work/b")] {
        bob.register(&sid(n), wd);
        registry.register_own(descriptor(&sid(n)));
    }
    let own = bob.key_id();
    bob.grant(Grant::Inbound {
        writer: PeerSide::session(own, sid(2)),
        target: LocalSide::Scope(WorkingDirectoryScope::new("/work/b")),
    });
    let now = Instant::now();
    let engine = RefCell::new(&mut bob.engine);
    let listed: Vec<SessionId> = registry
        .discover(Some(&sid(2)), |r, s| may_discover(&engine, r, s), now)
        .expect("a bound requester")
        .iter()
        .map(|d| d.session_id().clone())
        .collect();
    assert!(listed.contains(&sid(4)));
    assert!(
        !listed.contains(&sid(3)),
        "another project's session: {listed:?}"
    );
    let from_c: Vec<_> = registry
        .discover(Some(&sid(3)), |r, s| may_discover(&engine, r, s), now)
        .unwrap();
    assert!(from_c.is_empty(), "no grant, nothing listed");
    assert_eq!(
        registry.discover(None, |_, _| true, now).map(|v| v.len()),
        Err(ErrorCode::Unauthorized)
    );
}

/// `spec/security.md` §13 "Existence oracle" ([SC-RCP-073], [SEC-RCT-004]): a refused
/// sender learns nothing about whether the addressed session exists. For a session that is
/// registered and one that is not, the outcome, the code, whether the session was looked
/// up, and the receipt decision are the same: the [SEC-RCT-004] differential test.
#[test]
fn s13_unauthorized_refusal_is_the_same_whether_or_not_the_session_exists() {
    let (alice, mut bob) = paired_pair();
    let now = Instant::now();
    let mut seen = Vec::new();
    for (id, to) in [("m1", sid(2)), ("m2", sid(9))] {
        let mut limiter = ReceiptLimiter::new(8, Duration::from_secs(1), 8);
        let d = bob.receive(alice.sign(id, &sid(1), &to, "probe").octets());
        seen.push((
            d.outcome(),
            d.verified_by.clone(),
            d.looked_up,
            d.handed.is_some(),
            d.receipt_decision(&mut limiter, now),
        ));
    }
    assert_eq!(seen[0], seen[1], "existing and missing sessions look alike");
    assert_eq!(
        seen[0].0,
        (DeliveryState::Rejected, Some(ErrorCode::Unauthorized))
    );
}

/// 06 row 2 through the composed pipelines ([SEC-AUZ-001], [SC-DLV-075], [SC-DLV-076];
/// acceptance item 4): `oac_core::pipeline::Pipelines` over the in-memory transport, with a
/// stub adapter in place of a provider adapter.
///
/// - Send side: a session with no grant cannot address another session on its device. The
///   send request is refused `unknown-destination` exactly as for a session that does not
///   exist, nothing is published and the other session's adapter is never called. Once the
///   operator grants it, the same request is sent and handed off.
/// - Receive side: on a `cross-implementation` network, a paired but ungranted device's
///   endpoint publishes a validly signed envelope straight to the session; the receive
///   pipeline refuses it and the adapter is never called, while a granted device's envelope
///   is handed off.
#[test]
fn row02_unauthorized_send_through_the_composed_pipeline() {
    use oac_core::adapter::SendRequestResult;
    use oac_core::clock::{Clock, SystemClock};
    use oac_core::transport::{Deadline, Destination, Payload, PayloadKind, Reach, Transport};
    use oac_security_suite::identity;
    use oac_security_suite::pipeline::{PipelineDevice, send_request};
    use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};

    // Send side, one device.
    let network = MemoryNetwork::new();
    let bob = PipelineDevice::new(&network, identity("bob"));
    let a1 = bob.session(&sid(1), "/work/b");
    let _a2 = bob.session(&sid(2), "/work/b");
    for to in [sid(2), sid(9)] {
        let r = bob.adapter.send(send_request(&a1, &to, "let me in"));
        assert!(
            matches!(
                r,
                SendRequestResult::Refused {
                    error: ErrorCode::UnknownDestination
                }
            ),
            "{to}: {r:?}"
        );
    }
    network.settle();
    assert_eq!(
        bob.adapter.deliver_calls(),
        0,
        "nothing reached the other session"
    );
    bob.grant(Grant::Inbound {
        writer: PeerSide::session(bob.pipes.device().key_id().clone(), sid(1)),
        target: LocalSide::Session(sid(2)),
    });
    let r = bob.adapter.send(send_request(&a1, &sid(2), "now granted"));
    assert!(matches!(r, SendRequestResult::Sent { .. }), "{r:?}");
    network.settle();
    assert_eq!(bob.adapter.deliver_calls(), 1, "the control is handed off");

    // Receive side, three devices.
    let network = MemoryNetwork::builder()
        .reach(Reach::CrossImplementation)
        .build();
    let bob = PipelineDevice::new(&network, identity("bob"));
    let _a2 = bob.session(&sid(2), "/work/b");
    let (alice, carol) = (identity("alice"), identity("carol"));
    bob.pair(&alice);
    bob.pair(&carol);
    bob.grant(Grant::Inbound {
        writer: PeerSide::device(alice.key_id().clone()),
        target: LocalSide::Session(sid(2)),
    });
    let publish = |who: &oac_core::keys::DeviceIdentity, id: &str, from: SessionId| {
        let t = MemoryTransport::new();
        t.start(who.key_id(), MemoryConfiguration::wrap(&network))
            .expect("started");
        let env = who.sign_envelope(oac_security_suite::draft(
            id,
            &from,
            &sid(2),
            SystemClock.now(),
            "hello",
        ));
        t.publish(
            &Destination::Session(sid(2)),
            Payload::new(PayloadKind::Envelope, env.octets().to_vec()),
            Deadline::at(network.now() + Duration::from_secs(5)),
        );
        network.settle();
        t.shutdown();
    };
    publish(&carol, "m1", sid(5));
    assert_eq!(
        bob.adapter.deliver_calls(),
        0,
        "the ungranted device reached nothing"
    );
    publish(&alice, "m2", sid(1));
    assert_eq!(
        bob.adapter.deliver_calls(),
        1,
        "the granted device is handed off"
    );
    let handed = bob.adapter.delivered();
    assert_eq!(
        handed[0].message().verified_by().unwrap().key_id(),
        alice.key_id()
    );
}
