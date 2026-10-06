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

use oac_core::capabilities::{CapabilitiesEntry, SessionCapabilities, SessionDescriptor};
use oac_core::clock::{Clock, SystemClock};
use oac_core::ids::{EXTENSION_ID_V0, KeyId, SessionId, Timestamp, Token, Version};
use oac_core::json;
use oac_core::keys::{DeviceIdentity, DeviceKey};
use oac_core::presence::{PresenceRecord, PresenceState};
use oac_core::presence_auth::{
    AuthenticatedPresenceRecord, BindingEntry, ConsumerBindings, accept_authenticated_record,
};
use oac_core::registry::PresenceRegistry;
use oac_core::transport::{Deadline, PresenceEvent, PublishResult, Reach, Transport};
use oac_core::trust::TrustedKeySet;
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};

/// A binding table that relates every key, for this test only; authorization is F5's.
#[derive(Default)]
struct Related(std::collections::HashMap<SessionId, KeyId>);

impl ConsumerBindings for Related {
    fn binding(&self, s: &SessionId) -> BindingEntry {
        self.0
            .get(s)
            .map_or(BindingEntry::Unbound, |k| BindingEntry::Bound(k.clone()))
    }
    fn is_own_session(&self, _: &SessionId) -> bool {
        false
    }
    fn related(&self, _: &KeyId, _: &SessionId, _: &Timestamp) -> bool {
        true
    }
    fn bind(&mut self, s: &SessionId, k: &KeyId) {
        self.0.insert(s.clone(), k.clone());
    }
    fn mark_conflict(&mut self, s: &SessionId, _: &KeyId, _: &KeyId) {
        self.0.remove(s);
    }
}

struct Consumer {
    registry: PresenceRegistry,
    bindings: Related,
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

    let mut keys = TrustedKeySet::new(&consumer);
    keys.add_paired_key(issuer.principal().clone(), *issuer.public_key())
        .unwrap();
    let own = consumer.key_id().clone();
    let state = Arc::new(Mutex::new(Consumer {
        registry: PresenceRegistry::new(),
        bindings: Related::default(),
    }));
    let s = state.clone();
    b.watch_presence(Arc::new(move |event| {
        let mut st = s.lock().unwrap();
        let Consumer { registry, bindings } = &mut *st;
        match event {
            PresenceEvent::Record { payload, carrier } => {
                let ar = json::parse(payload.octets())
                    .ok()
                    .and_then(|v| AuthenticatedPresenceRecord::from_json(&v))
                    .expect("an authenticated presence record");
                let out = accept_authenticated_record(
                    &ar,
                    &keys,
                    &own,
                    bindings,
                    registry,
                    carrier,
                    &SystemClock.now(),
                    Instant::now(),
                );
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
