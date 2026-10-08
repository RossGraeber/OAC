// SPDX-License-Identifier: Apache-2.0

//! The Zenoh transport's own behaviour beyond the shared contract suite: the language
//! binding's errors (`oac_core::transport::TransportError`), the `not-taken` cases of
//! Table 6.1, and the neutral public surface (C7 §2).

use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use oac_core::health::HealthState;
use oac_core::ids::{KeyId, SessionId};
use oac_core::transport::{
    Deadline, Destination, Inbound, Payload, PayloadKind, PublishResult, Transport,
    TransportConfiguration, TransportError,
};
use oac_transport_zenoh::{
    DEFAULT_PARTITION, MAX_PAYLOAD_OCTETS, PeerConfiguration, PeerTransport,
};

fn conf() -> PeerConfiguration {
    let port = TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .unwrap()
        .port();
    PeerConfiguration::rendezvous(port).with_partition(format!("peer-transport-{port}"))
}

fn key(c: char) -> KeyId {
    KeyId::parse(&c.to_string().repeat(64)).unwrap()
}

fn session(n: u8) -> Destination {
    Destination::Session(SessionId::from_random_octets([n; 16]))
}

fn later() -> Deadline {
    Deadline::at(Instant::now() + Duration::from_secs(30))
}

#[test]
fn before_start_nothing_is_carried() {
    let t = PeerTransport::new();
    assert_eq!(t.health().state, HealthState::Unavailable);
    assert_eq!(
        t.publish(
            &session(1),
            Payload::new(PayloadKind::Envelope, b"x".to_vec()),
            later()
        ),
        PublishResult::NotTaken
    );
    assert_eq!(
        t.subscribe(&session(1), Arc::new(|_| {})).err(),
        Some(TransportError::NotStarted)
    );
    assert_eq!(
        t.watch_presence(Arc::new(|_| {})).err(),
        Some(TransportError::NotStarted)
    );
    t.shutdown();
    assert_eq!(t.connected_peers(), 0);
}

#[test]
fn a_foreign_configuration_is_refused() {
    let t = PeerTransport::new();
    let e = t
        .start(&key('a'), TransportConfiguration::new(17u32))
        .unwrap_err();
    assert!(matches!(e, TransportError::InvalidConfiguration(_)));
}

#[test]
fn start_twice_device_subscriptions_and_kinds() {
    let t = PeerTransport::new();
    let caps = t.start(&key('a'), conf().wrap()).unwrap();
    assert!(caps.contract_violations().is_empty());
    assert!(!caps.multicast_discovery);
    assert_eq!(caps.max_payload_octets, MAX_PAYLOAD_OCTETS);
    assert_eq!(t.health().state, HealthState::Healthy);
    assert_eq!(
        t.start(&key('a'), conf().wrap()).err(),
        Some(TransportError::AlreadyStarted)
    );
    assert_eq!(
        t.subscribe(&Destination::Device(key('b')), Arc::new(|_| {}))
            .err(),
        Some(TransportError::NotLocalDevice)
    );
    let dev = Destination::Device(key('b'));
    let not_taken = [
        // Wrong kind for the destination, or for the operation (Table 6.1).
        t.publish(
            &dev,
            Payload::new(PayloadKind::Envelope, b"x".to_vec()),
            later(),
        ),
        t.publish(
            &session(1),
            Payload::new(PayloadKind::Receipt, b"x".to_vec()),
            later(),
        ),
        t.publish(
            &dev,
            Payload::new(PayloadKind::Presence, b"x".to_vec()),
            later(),
        ),
        t.send_presence(
            &dev,
            Payload::new(PayloadKind::Receipt, b"x".to_vec()),
            later(),
        ),
        t.send_presence(
            &session(1),
            Payload::new(PayloadKind::Presence, b"x".to_vec()),
            later(),
        ),
        // Above the declared maximum.
        t.publish(
            &session(1),
            Payload::new(
                PayloadKind::Envelope,
                vec![0; MAX_PAYLOAD_OCTETS as usize + 1],
            ),
            later(),
        ),
        // A deadline already reached.
        t.publish(
            &session(1),
            Payload::new(PayloadKind::Envelope, b"x".to_vec()),
            Deadline::at(Instant::now()),
        ),
    ];
    assert!(
        not_taken.iter().all(|r| *r == PublishResult::NotTaken),
        "{not_taken:?}"
    );
    t.shutdown();
    assert_eq!(t.health().state, HealthState::Unavailable);
    assert_eq!(
        t.publish(
            &session(1),
            Payload::new(PayloadKind::Envelope, b"x".to_vec()),
            later()
        ),
        PublishResult::NotTaken
    );
}

#[test]
fn a_transport_starts_again_after_shutdown_with_nothing_kept() {
    let c = conf();
    let t = PeerTransport::new();
    t.start(&key('a'), c.clone().wrap()).unwrap();
    let got: Arc<Mutex<Vec<Inbound>>> = Arc::default();
    let g = got.clone();
    let _s = t
        .subscribe(&session(4), Arc::new(move |i| g.lock().unwrap().push(i)))
        .unwrap();
    t.shutdown();
    t.start(&key('a'), c.wrap()).unwrap();
    // The subscription made before the restart is gone with it ([IFC-TRN-035]).
    for _ in 0..5 {
        t.publish(
            &session(4),
            Payload::new(PayloadKind::Envelope, b"x".to_vec()),
            later(),
        );
        thread::sleep(Duration::from_millis(50));
    }
    assert!(got.lock().unwrap().is_empty());
    t.shutdown();
}

#[test]
fn the_local_default_is_multicast_in_the_default_partition() {
    let c = PeerConfiguration::default();
    assert!(c.discovers_by_multicast());
    assert_eq!(c.partition(), DEFAULT_PARTITION);
    assert!(!PeerConfiguration::rendezvous(1).discovers_by_multicast());
}
