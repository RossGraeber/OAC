// SPDX-License-Identifier: Apache-2.0

//! The presence registry fed by a transport's `watch_presence` (#55, F6): an authenticated
//! announcement pushed from one implementation to another reaches the consumer's registry
//! through the watch handler alone, and the issuer leaving the network reaches it as carrier
//! loss (`spec/session-channels.md` §7.2.4, [SC-DLV-046]; `spec/interfaces.md` §6.6).
//!
//! Nothing on the consumer side asks for records: the registry has no fetch operation, and
//! the only path in is the handler the transport calls on its own initiative, so presence
//! here involves no polling (§7.1, [SC-DLV-002]).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oac_core::authorization::{
    AuthorizationEngine, Grant, LocalSide, MemoryDecisionLog, OperatorConfirmed, PeerSide,
};
use oac_core::capabilities::{CapabilitiesEntry, SessionCapabilities, SessionDescriptor};
use oac_core::clock::{Clock, SystemClock};
use oac_core::ids::{EXTENSION_ID_V0, SessionId, Token, Version};
use oac_core::json;
use oac_core::keys::{DeviceIdentity, DeviceKey};
use oac_core::pairing::{MemoryPairingStore, PairedPeer};
use oac_core::presence::{PresenceRecord, PresenceState};
use oac_core::presence_auth::{AuthenticatedPresenceRecord, accept_authenticated_record};
use oac_core::registry::PresenceRegistry;
use oac_core::transport::{Deadline, PresenceEvent, PublishResult, Reach, Transport};
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};

struct Consumer {
    registry: PresenceRegistry,
    engine: AuthorizationEngine,
}

#[test]
fn watch_presence_feeds_the_registry_and_carrier_loss_ends_it() {
    let network = MemoryNetwork::builder()
        .reach(Reach::CrossImplementation)
        .build();
    let issuer = DeviceIdentity::new(DeviceKey::generate(), Token::parse("issuer").unwrap());
    let consumer = DeviceIdentity::new(DeviceKey::generate(), Token::parse("consumer").unwrap());
    let a = MemoryTransport::new();
    a.start(issuer.key_id(), MemoryConfiguration::wrap(&network))
        .unwrap();
    let b = MemoryTransport::new();
    b.start(consumer.key_id(), MemoryConfiguration::wrap(&network))
        .unwrap();

    // The consumer's authorization engine: the issuer's key paired, and an outbound grant
    // naming it, which relates it to the consumer ([SEC-AUZ-017]).
    let store = MemoryPairingStore::new();
    let mut engine = AuthorizationEngine::new(
        &consumer,
        Arc::new(SystemClock),
        Box::new(MemoryDecisionLog::new()),
    );
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
    let state = Arc::new(Mutex::new(Consumer {
        registry: PresenceRegistry::new(),
        engine,
    }));
    let s = state.clone();
    b.watch_presence(Arc::new(move |event| {
        let mut st = s.lock().unwrap();
        let Consumer { registry, engine } = &mut *st;
        match event {
            PresenceEvent::Record { payload, carrier } => {
                let ar = json::parse(payload.octets())
                    .ok()
                    .and_then(|v| AuthenticatedPresenceRecord::from_json(&v))
                    .expect("an authenticated presence record");
                let out =
                    accept_authenticated_record(&ar, engine, registry, carrier, Instant::now());
                assert!(out.accepted(), "{out:?}");
            }
            PresenceEvent::CarrierLoss { carrier } => {
                registry.carrier_loss(&carrier);
            }
        }
    }))
    .unwrap();

    let sid = SessionId::from_random_octets([7; 16]);
    let caps = SessionCapabilities::declare([(
        EXTENSION_ID_V0,
        CapabilitiesEntry::new(Version { major: 0, minor: 1 }, true),
    )])
    .unwrap();
    let descriptor = SessionDescriptor::new(sid.clone(), caps, None, None).unwrap();
    let record = PresenceRecord::announcement(0, SystemClock.now(), 60_000, descriptor).unwrap();
    let (dest, payload) = AuthenticatedPresenceRecord::issue(&issuer, &record, consumer.key_id())
        .to_payload()
        .unwrap();
    let deadline = Deadline::at(network.now() + Duration::from_secs(5));
    assert_eq!(
        a.send_presence(&dest, payload, deadline),
        PublishResult::Taken
    );
    network.settle();
    assert_eq!(
        state.lock().unwrap().registry.state(&sid, Instant::now()),
        PresenceState::Online
    );

    // The issuer leaves: carrier loss, and the session is unreachable at once.
    a.shutdown();
    network.settle();
    assert_eq!(
        state.lock().unwrap().registry.state(&sid, Instant::now()),
        PresenceState::Unreachable
    );
    b.shutdown();
}
