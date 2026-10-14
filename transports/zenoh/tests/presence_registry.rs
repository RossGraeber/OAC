// SPDX-License-Identifier: Apache-2.0

//! #63 (G2) at the neutral surface: the core's presence registry fed by a real Zenoh
//! transport. The carrier handle that frames arrive with is the one a carrier loss names, and
//! a loss moves announcements toward `unreachable` only ([IFC-TRN-062], [SC-DLV-046]).
//!
//! The records here are accepted with the carrier handle of frames that really crossed the
//! transport; the sealed route of the records themselves is the core's and is exercised by
//! the shared contract suite and `core/tests/sealing.rs`.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use oac_core::authorization::{
    AuthorizationEngine, Grant, LocalSide, MemoryDecisionLog, OperatorConfirmed, PeerSide,
};
use oac_core::capabilities::{CapabilitiesEntry, SessionCapabilities, SessionDescriptor};
use oac_core::clock::{Clock, SystemClock};
use oac_core::ids::{EXTENSION_ID_V0, SessionId, Token, Version};
use oac_core::keys::{DeviceIdentity, DeviceKey};
use oac_core::pairing::{MemoryPairingStore, PairedPeer};
use oac_core::presence::{PresenceRecord, PresenceState};
use oac_core::presence_auth::{AuthenticatedPresenceRecord, accept_authenticated_record};
use oac_core::registry::PresenceRegistry;
use oac_core::transport::{
    CarrierHandle, Deadline, Destination, Payload, PayloadKind, PresenceEvent, Transport,
};
use oac_transport_zenoh::{PeerConfiguration, PeerTransport};

#[path = "support/ports.rs"]
mod ports;

struct Consumer {
    registry: PresenceRegistry,
    engine: AuthorizationEngine,
}

fn wait_for(what: &str, cond: impl Fn() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "timed out waiting for {what}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn identity(name: &str) -> DeviceIdentity {
    DeviceIdentity::new(DeviceKey::generate(), Token::parse(name).unwrap())
}

/// A consumer that has paired with `issuers` and granted each an outbound grant, which
/// relates them to it ([SEC-AUZ-017]).
fn consumer(me: &DeviceIdentity, issuers: &[&DeviceIdentity]) -> Consumer {
    let store = MemoryPairingStore::new();
    let mut engine = AuthorizationEngine::new(
        me,
        Arc::new(SystemClock),
        Box::new(MemoryDecisionLog::new()),
    );
    for issuer in issuers {
        let peer = PairedPeer::by_key_id_comparison(
            issuer.principal().clone(),
            *issuer.public_key(),
            issuer.key_id(),
            SystemClock.now(),
            OperatorConfirmed::by_operator(),
        )
        .unwrap();
        engine.pair(peer, &store).unwrap();
        engine
            .add_grant(
                Grant::Outbound {
                    writer: LocalSide::Device,
                    target: PeerSide::device(issuer.key_id().clone()),
                },
                OperatorConfirmed::by_operator(),
                &store,
            )
            .unwrap();
    }
    Consumer {
        registry: PresenceRegistry::new(),
        engine,
    }
}

fn announcement(
    issuer: &DeviceIdentity,
    to: &DeviceIdentity,
    session: &SessionId,
    seq: u64,
) -> AuthenticatedPresenceRecord {
    let caps = SessionCapabilities::declare([(
        EXTENSION_ID_V0,
        CapabilitiesEntry::new(Version { major: 0, minor: 1 }, true),
    )])
    .unwrap();
    let descriptor = SessionDescriptor::new(session.clone(), caps, None, None).unwrap();
    let record = PresenceRecord::announcement(seq, SystemClock.now(), 60_000, descriptor).unwrap();
    AuthenticatedPresenceRecord::issue(issuer, &record, to.key_id())
}

/// The carrier handle of a frame `from` sent to `to`'s device stream.
fn carrier_of(from: &PeerTransport, to: &PeerTransport, device: &DeviceIdentity) -> CarrierHandle {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let _s = to
        .subscribe(
            &Destination::Device(device.key_id().clone()),
            Arc::new(move |i| {
                let _ = tx.lock().unwrap().send(i.carrier);
            }),
        )
        .unwrap();
    let start = Instant::now();
    loop {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "no frame arrived"
        );
        from.publish(
            &Destination::Device(device.key_id().clone()),
            Payload::new(PayloadKind::Sealed, vec![1, 2, 3]),
            Deadline::at(Instant::now() + Duration::from_secs(30)),
        );
        if let Ok(c) = rx.recv_timeout(Duration::from_millis(100)) {
            return c;
        }
    }
}

fn state(c: &Arc<Mutex<Consumer>>, session: &SessionId) -> PresenceState {
    c.lock().unwrap().registry.state(session, Instant::now())
}

/// Online while the issuer lives, `unreachable` when its transport leaves, `unknown` for a
/// session never announced. The record is not guessed back: only the issuer's next
/// announcement makes a lost session `online` again.
#[test]
fn a_departing_issuer_makes_its_session_unreachable_and_a_stranger_stays_unknown() {
    // A rendezvous port claimed for the life of the test (`tests/support/ports.rs`).
    let conf = ports::claimed(|port| {
        PeerConfiguration::rendezvous(port).with_partition(format!("registry-{port}"))
    });
    let (issuer, other, me) = (identity("issuer"), identity("other"), identity("consumer"));
    let (relay, a, c, b) = (
        PeerTransport::new(),
        PeerTransport::new(),
        PeerTransport::new(),
        PeerTransport::new(),
    );
    let relay_id = identity("relay");
    relay.start(relay_id.key_id(), conf.clone().wrap()).unwrap();
    a.start(issuer.key_id(), conf.clone().wrap()).unwrap();
    c.start(other.key_id(), conf.clone().wrap()).unwrap();
    b.start(me.key_id(), conf.clone().wrap()).unwrap();

    let state_of = Arc::new(Mutex::new(consumer(&me, &[&issuer, &other])));
    let s = state_of.clone();
    b.watch_presence(Arc::new(move |event| {
        if let PresenceEvent::CarrierLoss { carrier } = event {
            s.lock().unwrap().registry.carrier_loss(&carrier);
        }
    }))
    .unwrap();
    wait_for("the consumer to see all three", || {
        b.observed_live_links() == 3
    });

    // Both issuers' frames reach the consumer on one receiver-local stream handle.
    let from_issuer = carrier_of(&a, &b, &me);
    let from_other = carrier_of(&c, &b, &me);
    assert_eq!(
        from_issuer, from_other,
        "the stream handle groups the frame stream, not senders"
    );

    let (s_issuer, s_other, s_stranger) = (
        SessionId::from_random_octets([1; 16]),
        SessionId::from_random_octets([2; 16]),
        SessionId::from_random_octets([3; 16]),
    );
    for (who, session, carrier) in [
        (&issuer, &s_issuer, from_issuer.clone()),
        (&other, &s_other, from_other),
    ] {
        let mut guard = state_of.lock().unwrap();
        let Consumer { registry, engine } = &mut *guard;
        let record = announcement(who, &me, session, 0);
        let out = accept_authenticated_record(&record, engine, registry, carrier, Instant::now());
        assert!(out.accepted(), "{out:?}");
    }
    assert_eq!(state(&state_of, &s_issuer), PresenceState::Online);
    assert_eq!(state(&state_of, &s_other), PresenceState::Online);
    assert_eq!(state(&state_of, &s_stranger), PresenceState::Unknown);

    // The issuer's transport leaves: its session is unreachable at once. The other issuer is
    // alive, but its announcement arrived on the same stream handle, so it is stale too (the
    // residual of 11-risks row 83: a loss cannot be tied to one issuer).
    a.shutdown();
    wait_for("the issuer's session to go unreachable", || {
        state(&state_of, &s_issuer) == PresenceState::Unreachable
    });
    assert_eq!(state(&state_of, &s_other), PresenceState::Unreachable);
    assert_eq!(state(&state_of, &s_stranger), PresenceState::Unknown);

    // Only the live issuer's next announcement makes its session online again.
    {
        let mut guard = state_of.lock().unwrap();
        let Consumer { registry, engine } = &mut *guard;
        let record = announcement(&other, &me, &s_other, 1);
        let out =
            accept_authenticated_record(&record, engine, registry, from_issuer, Instant::now());
        assert!(out.accepted(), "{out:?}");
    }
    assert_eq!(state(&state_of, &s_other), PresenceState::Online);
    assert_eq!(state(&state_of, &s_issuer), PresenceState::Unreachable);
    for t in [b, c, relay] {
        t.shutdown();
    }
}
