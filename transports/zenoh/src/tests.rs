// SPDX-License-Identifier: Apache-2.0

//! Tests that need the crate's inside: the native view another peer has of this transport
//! ([IFC-TRN-043], [IFC-TRN-013]). Loopback only, over a fixed rendezvous port with
//! scouting off, so that nothing outside the test joins.

use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use oac_core::ids::{KeyId, SessionId};
use oac_core::transport::{
    Deadline, Destination, Inbound, Payload, PayloadKind, PresenceEvent, PublishResult, Transport,
};
use zenoh::Wait;

use crate::addressing::{Partition, destination_digest};
use crate::config::Role;
use crate::{PeerConfiguration, PeerTransport};

fn conf(tag: &str) -> PeerConfiguration {
    let port = TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .unwrap()
        .port();
    PeerConfiguration::rendezvous(port).with_partition(format!("unit-{tag}-{port}"))
}

fn key(c: char) -> KeyId {
    KeyId::parse(&c.to_string().repeat(64)).unwrap()
}

fn session(n: u8) -> Destination {
    Destination::Session(SessionId::from_random_octets([n; 16]))
}

fn wait_for(cond: impl Fn() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(15) {
        if cond() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    cond()
}

/// [IFC-TRN-043], differential: a third party on the partition, holding the session ids,
/// sees the same native interest for a subscribed and an unsubscribed destination, before
/// and after the subscription is made and ended.
#[test]
fn native_interest_does_not_depend_on_subscriptions() {
    let c = conf("043");
    let a = PeerTransport::new();
    a.start(&key('a'), c.clone().wrap()).unwrap();
    let observer = zenoh::open(c.native(Role::Joiner).unwrap()).wait().unwrap();
    assert!(wait_for(|| a.connected_peers() >= 1));
    let p = Partition::new(c.partition());
    let (subd, unsubd) = (session(1), session(2));
    let pub_subd = observer
        .declare_publisher(p.key(&destination_digest(&subd)))
        .wait()
        .unwrap();
    let pub_unsubd = observer
        .declare_publisher(p.key(&destination_digest(&unsubd)))
        .wait()
        .unwrap();
    let matching = || {
        (
            pub_subd.matching_status().wait().unwrap().matching(),
            pub_unsubd.matching_status().wait().unwrap().matching(),
        )
    };
    // The transport's one declaration reaches the observer.
    assert!(wait_for(|| matching() == (true, true)));
    let before = matching();
    let s = a.subscribe(&subd, Arc::new(|_| {})).unwrap();
    thread::sleep(Duration::from_millis(500));
    let during = matching();
    s.end();
    thread::sleep(Duration::from_millis(500));
    let after = matching();
    assert_eq!(
        (before, during, after),
        ((true, true), (true, true), (true, true))
    );
    a.shutdown();
    let _ = observer.close().wait();
}

/// [IFC-TRN-013]: two transports that found each other hand the core nothing, neither a
/// presence event nor an inbound payload, until a payload is sent: transport discovery is
/// not a session or a presence record.
#[test]
fn discovery_alone_hands_the_core_nothing() {
    let c = conf("013");
    let (a, b) = (PeerTransport::new(), PeerTransport::new());
    a.start(&key('a'), c.clone().wrap()).unwrap();
    let events: Arc<Mutex<Vec<PresenceEvent>>> = Arc::default();
    let inbound: Arc<Mutex<Vec<Inbound>>> = Arc::default();
    let (e, i) = (events.clone(), inbound.clone());
    a.watch_presence(Arc::new(move |x| e.lock().unwrap().push(x)))
        .unwrap();
    let _s = a
        .subscribe(
            &Destination::Device(key('a')),
            Arc::new(move |x| i.lock().unwrap().push(x)),
        )
        .unwrap();
    b.start(&key('b'), c.wrap()).unwrap();
    assert!(wait_for(
        || a.connected_peers() >= 1 && b.connected_peers() >= 1
    ));
    thread::sleep(Duration::from_secs(1));
    assert!(events.lock().unwrap().is_empty());
    assert!(inbound.lock().unwrap().is_empty());
    // The control: a record sent on purpose arrives.
    let r = b.send_presence(
        &Destination::Device(key('a')),
        Payload::new(PayloadKind::Presence, b"record".to_vec()),
        Deadline::at(Instant::now() + Duration::from_secs(10)),
    );
    assert_eq!(r, PublishResult::Taken);
    assert!(wait_for(|| events.lock().unwrap().len() == 1));
    b.shutdown();
    a.shutdown();
}

/// A frame that reaches a peer after its expiry is dropped ([IFC-TRN-034]): a native put of
/// an expired frame on a subscribed destination is never handed over, while a fresh one is.
#[test]
fn an_expired_frame_is_dropped_on_arrival() {
    let c = conf("034");
    let a = PeerTransport::new();
    a.start(&key('a'), c.clone().wrap()).unwrap();
    let got: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
    let g = got.clone();
    let _s = a
        .subscribe(
            &session(3),
            Arc::new(move |x| g.lock().unwrap().push(x.payload.octets().to_vec())),
        )
        .unwrap();
    let raw = zenoh::open(c.native(Role::Joiner).unwrap()).wait().unwrap();
    assert!(wait_for(|| a.connected_peers() >= 1));
    let k = Partition::new(c.partition()).key(&destination_digest(&session(3)));
    let now = crate::transport::unix_millis_now();
    let link = [7u8; 16];
    // The fresh frame doubles as the readiness probe: keep putting it until it arrives.
    assert!(wait_for(|| {
        raw.put(
            &k,
            crate::frame::encode(PayloadKind::Envelope, now + 60_000, &link, b"fresh"),
        )
        .wait()
        .unwrap();
        !got.lock().unwrap().is_empty()
    }));
    raw.put(
        &k,
        crate::frame::encode(PayloadKind::Envelope, now - 1, &link, b"expired"),
    )
    .wait()
    .unwrap();
    raw.put(&k, b"not a frame".to_vec()).wait().unwrap();
    thread::sleep(Duration::from_millis(500));
    let got = got.lock().unwrap();
    assert!(got.iter().all(|o| o == b"fresh"), "{got:?}");
    drop(got);
    let _ = raw.close().wait();
    a.shutdown();
}
