// SPDX-License-Identifier: Apache-2.0

//! Native sample observations and overflow checks; default tier, loopback only.
use std::net::TcpListener;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use oac_core::ids::{KeyId, SessionId};
use oac_core::sealing::{AgreementPublicKey, seal};
use oac_core::transport::{Deadline, Destination, Payload, PayloadKind, PublishResult, Transport};
use zenoh::Wait;
use zenoh::bytes::Encoding;
use zenoh::qos::CongestionControl;

use crate::addressing::Partition;
use crate::config::Role;
use crate::{MAX_PAYLOAD_OCTETS, PeerConfiguration, PeerTransport};

fn conf(tag: &str) -> PeerConfiguration {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    PeerConfiguration::rendezvous(port).with_partition(format!("unit-{tag}-{port}"))
}
fn key(c: char) -> KeyId {
    KeyId::parse(&c.to_string().repeat(64)).unwrap()
}
fn wait_for(cond: impl Fn() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(start.elapsed() < Duration::from_secs(15));
        thread::sleep(Duration::from_millis(20));
    }
}

/// IFC-TRN-043/106/107: native matching and sample carriage are identical before,
/// during and after local interest changes, using a real core-generated HPKE frame.
#[test]
fn native_interest_and_sealed_carriage_do_not_depend_on_subscriptions() {
    let c = conf("043");
    let a = PeerTransport::new();
    a.start(&key('a'), c.clone().wrap()).unwrap();
    let observer = zenoh::open(c.native(Role::Joiner, 0).unwrap())
        .wait()
        .unwrap();
    let p = Partition::new(c.partition());
    let publisher = observer.declare_publisher(p.key()).wait().unwrap();
    wait_for(|| publisher.matching_status().wait().unwrap().matching());
    let capture = observer.declare_subscriber(p.key()).wait().unwrap();
    let device_got = Arc::new(Mutex::new(Vec::new()));
    let g = device_got.clone();
    let _device = a
        .subscribe(
            &Destination::Device(key('a')),
            Arc::new(move |i| {
                if i.payload.octets() != b"native-ready" {
                    g.lock().unwrap().push(i.payload);
                }
            }),
        )
        .unwrap();
    // The observer's declaration must propagate back to the sender. Native matching
    // at the observer proves the opposite direction only; use actual sample readiness.
    let start = Instant::now();
    loop {
        assert!(start.elapsed() < Duration::from_secs(15));
        a.publish(
            &Destination::Device(key('b')),
            Payload::new(PayloadKind::Sealed, b"native-ready".to_vec()),
            Deadline::at(Instant::now() + Duration::from_secs(5)),
        );
        if capture
            .recv_timeout(Duration::from_millis(100))
            .unwrap()
            .is_some()
        {
            break;
        }
    }
    let s1 = Destination::Session(SessionId::from_random_octets([1; 16]));
    let s2 = Destination::Session(SessionId::from_random_octets([2; 16]));
    // Public agreement key from sec-test-keys.json, also used by core's sealing tests.
    let recipient =
        AgreementPublicKey::from_base64url("wcSSzJLhvIMKmwB60NLYtLZcibdrU5_XcugIsJugBG8").unwrap();
    let bytes = seal(
        &recipient,
        PayloadKind::Presence,
        b"synthetic presence",
        MAX_PAYLOAD_OCTETS,
    )
    .unwrap();
    let mut local = None;
    for stage in 0..3 {
        if stage == 1 {
            local = Some(
                a.subscribe(&s1, Arc::new(|_| panic!("sealed frame reached session")))
                    .unwrap(),
            );
        }
        if stage == 2 {
            local.take().unwrap().end();
        }
        assert!(publisher.matching_status().wait().unwrap().matching());
        for dest in [Destination::Device(key('b')), Destination::Device(key('c'))] {
            assert_eq!(
                a.publish(
                    &dest,
                    Payload::new(PayloadKind::Sealed, bytes.clone()),
                    Deadline::at(Instant::now() + Duration::from_secs(5))
                ),
                PublishResult::Taken
            );
            let deadline = Instant::now() + Duration::from_secs(5);
            let sample = loop {
                let sample = capture.recv_deadline(deadline).unwrap().unwrap();
                if sample.payload().to_bytes().as_ref() != b"native-ready" {
                    break sample;
                }
            };
            assert_eq!(sample.key_expr().as_str(), p.key());
            assert_eq!(*sample.encoding(), Encoding::ZENOH_BYTES);
            assert_eq!(sample.payload().to_bytes().as_ref(), &bytes);
            assert!(sample.attachment().is_none());
            assert!(sample.timestamp().is_none());
        }
        for dest in [&s1, &s2] {
            assert_eq!(
                a.publish(
                    dest,
                    Payload::new(PayloadKind::Sealed, bytes.clone()),
                    Deadline::at(Instant::now() + Duration::from_secs(5))
                ),
                PublishResult::NotTaken
            );
        }
    }
    wait_for(|| device_got.lock().unwrap().len() == 6);
    assert!(
        device_got
            .lock()
            .unwrap()
            .iter()
            .all(|p| p.kind() == PayloadKind::Sealed && p.octets() == bytes)
    );
    a.shutdown();
    observer.close().wait().unwrap();
}

/// Flood the actual receive callback while its device handler stalls. Size admission
/// precedes native queuing; overflow retains only the newest 16 admitted frames.
#[test]
fn native_ring_drops_oldest_and_refuses_oversized_samples_while_handler_stalls() {
    let c = conf("ring");
    let a = PeerTransport::new();
    a.start(&key('a'), c.clone().wrap()).unwrap();
    let raw = zenoh::open(c.native(Role::Joiner, 0).unwrap())
        .wait()
        .unwrap();
    let publisher = raw
        .declare_publisher(Partition::new(c.partition()).key())
        .encoding(Encoding::ZENOH_BYTES)
        .congestion_control(CongestionControl::Block)
        .wait()
        .unwrap();
    wait_for(|| publisher.matching_status().wait().unwrap().matching());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let got = Arc::new(Mutex::new(Vec::new()));
    let g = got.clone();
    let _sub = a
        .subscribe(
            &Destination::Device(key('a')),
            Arc::new(move |i| {
                let bytes = i.payload.octets();
                if bytes == b"hold" {
                    entered_tx.send(()).unwrap();
                    release_rx.lock().unwrap().recv().unwrap();
                } else {
                    g.lock().unwrap().push(bytes.to_vec());
                }
            }),
        )
        .unwrap();
    // Drop before the subscription on every unwind, releasing its blocked callback.
    struct Release(mpsc::Sender<()>);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let release = Release(release_tx);
    publisher.put(b"hold".to_vec()).wait().unwrap();
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    for n in 0..40u8 {
        publisher
            .put(vec![n; MAX_PAYLOAD_OCTETS as usize])
            .wait()
            .unwrap();
    }
    publisher
        .put(vec![99; MAX_PAYLOAD_OCTETS as usize + 1])
        .wait()
        .unwrap();
    publisher
        .put(b"wrong encoding".to_vec())
        .encoding(Encoding::TEXT_PLAIN)
        .wait()
        .unwrap();
    wait_for(|| {
        a.carriage_for_tests()
            .iter()
            .any(|o| o.frame == b"wrong encoding")
    });
    thread::sleep(Duration::from_millis(100));
    release.0.send(()).unwrap();
    wait_for(|| got.lock().unwrap().len() == 16);
    assert_eq!(
        got.lock().unwrap().iter().map(|b| b[0]).collect::<Vec<_>>(),
        (24..40).collect::<Vec<u8>>()
    );
    assert!(
        got.lock()
            .unwrap()
            .iter()
            .all(|b| b.len() == MAX_PAYLOAD_OCTETS as usize)
    );
    a.shutdown();
    raw.close().wait().unwrap();
}

#[test]
fn discovery_alone_hands_the_core_nothing() {
    let c = conf("013");
    let (a, b) = (PeerTransport::new(), PeerTransport::new());
    a.start(&key('a'), c.clone().wrap()).unwrap();
    a.watch_presence(Arc::new(|_| panic!("discovery became presence")))
        .unwrap();
    let got = Arc::new(Mutex::new(Vec::new()));
    let g = got.clone();
    let _sub = a
        .subscribe(
            &Destination::Device(key('a')),
            Arc::new(move |i| g.lock().unwrap().push(i.payload)),
        )
        .unwrap();
    b.start(&key('b'), c.wrap()).unwrap();
    thread::sleep(Duration::from_millis(500));
    assert!(got.lock().unwrap().is_empty());
    b.publish(
        &Destination::Device(key('a')),
        Payload::new(PayloadKind::Sealed, b"record".to_vec()),
        Deadline::at(Instant::now() + Duration::from_secs(5)),
    );
    wait_for(|| !got.lock().unwrap().is_empty());
    assert_eq!(got.lock().unwrap()[0].octets(), b"record");
    b.shutdown();
    a.shutdown();
}
