// SPDX-License-Identifier: Apache-2.0

//! The Zenoh transport's own behaviour beyond the shared contract suite: the language
//! binding's errors (`oac_core::transport::TransportError`), the `not-taken` cases of
//! Table 6.1, and the neutral public surface (C7 §2).

use std::net::{IpAddr, Ipv4Addr, TcpListener, UdpSocket};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use zenoh::Wait;

use oac_core::health::HealthState;
use oac_core::ids::{KeyId, SessionId};
use oac_core::transport::{
    Deadline, Destination, Inbound, Payload, PayloadKind, PresenceEvent, PublishResult, Transport,
    TransportConfiguration, TransportError,
};
use oac_transport_zenoh::{
    DEFAULT_PARTITION, DEFAULT_RENDEZVOUS_PORT, MAX_PAYLOAD_OCTETS, PeerConfiguration,
    PeerTransport,
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
fn the_local_default_is_the_loopback_rendezvous_in_the_default_partition() {
    let c = PeerConfiguration::default();
    assert_eq!(c, PeerConfiguration::local());
    assert_eq!(c, PeerConfiguration::rendezvous(DEFAULT_RENDEZVOUS_PORT));
    assert_eq!(c.partition(), DEFAULT_PARTITION);
    assert_eq!(c.rendezvous_port(), DEFAULT_RENDEZVOUS_PORT);
}

/// A non-loopback IPv4 address of this host, or why there is none. Connecting a UDP socket
/// sends nothing; it only asks the OS which source address it would use.
fn lan_address() -> Result<Ipv4Addr, String> {
    let s = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(|e| e.to_string())?;
    s.connect(("192.0.2.1", 9))
        .map_err(|e| format!("no route off the host: {e}"))?;
    match s.local_addr().map_err(|e| e.to_string())?.ip() {
        IpAddr::V4(a) if !a.is_loopback() && !a.is_unspecified() => Ok(a),
        other => Err(format!("the only source address is {other}")),
    }
}

/// PR #364 review, blocking finding 1: local mode never reaches beyond loopback. A plain
/// Zenoh peer listening on this host's LAN address, with multicast scouting on (interface
/// "auto"), gossip on and a subscriber on every key, receives none of the frames two local
/// transports exchange, and is never linked to either of them.
#[test]
fn local_mode_reaches_nothing_beyond_loopback() {
    let lan = match lan_address() {
        Ok(a) => a,
        Err(why) => {
            eprintln!("SKIPPED: this host has no non-loopback interface to probe from ({why})");
            return;
        }
    };
    let mut probe_conf = zenoh::Config::default();
    for (k, v) in [
        ("mode", r#""peer""#.to_owned()),
        ("listen/endpoints", format!(r#"["tcp/{lan}:0"]"#)),
        ("scouting/multicast/enabled", "true".to_owned()),
        ("scouting/gossip/enabled", "true".to_owned()),
    ] {
        probe_conf.insert_json5(k, &v).unwrap();
    }
    let probe = match zenoh::open(probe_conf).wait() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("SKIPPED: the probe cannot listen on {lan}: {e}");
            return;
        }
    };
    let heard = Arc::new(AtomicUsize::new(0));
    let h = heard.clone();
    let _all = probe
        .declare_subscriber("**")
        .callback(move |_| {
            h.fetch_add(1, Ordering::SeqCst);
        })
        .wait()
        .unwrap();
    let c = conf();
    let (a, b) = (PeerTransport::new(), PeerTransport::new());
    a.start(&key('a'), c.clone().wrap()).unwrap();
    b.start(&key('b'), c.wrap()).unwrap();
    let got: Got = Arc::default();
    let _s = b.subscribe(&session(9), sink(&got)).unwrap();
    let start = Instant::now();
    let mut probe_linked = 0;
    while got.lock().unwrap().is_empty() || start.elapsed() < Duration::from_secs(4) {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the control never arrived"
        );
        a.publish(
            &session(9),
            Payload::new(PayloadKind::Envelope, b"local only".to_vec()),
            later(),
        );
        let info = probe.info();
        probe_linked =
            probe_linked.max(info.peers_zid().wait().count() + info.routers_zid().wait().count());
        thread::sleep(Duration::from_millis(200));
    }
    assert_eq!(
        probe_linked, 0,
        "a local transport was linked to the LAN probe"
    );
    assert_eq!(
        heard.load(Ordering::SeqCst),
        0,
        "the LAN probe received frames"
    );
    // The later transport is linked to the rendezvous holder and to nothing else.
    assert_eq!(b.connected_peers(), 1);
    a.shutdown();
    b.shutdown();
    let _ = probe.close().wait();
}

/// A rendezvous port held by a program that is not an OAC transport (here a plain TCP
/// listener) makes `start` fail with an error that names the port and says so.
#[test]
fn a_port_held_by_something_else_is_named_in_the_start_error() {
    let squatter = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = squatter.local_addr().unwrap().port();
    let t = PeerTransport::new();
    let e = t
        .start(
            &key('a'),
            PeerConfiguration::rendezvous(port)
                .with_partition(format!("squat-{port}"))
                .wrap(),
        )
        .unwrap_err();
    let TransportError::InvalidConfiguration(why) = e else {
        panic!("unexpected error {e:?}");
    };
    assert!(why.contains(&port.to_string()), "{why}");
    assert!(why.contains("not an OAC transport"), "{why}");
    assert!(!why.contains("127.0.0.1"), "{why}");
    drop(squatter);
}

/// With gossip off, a later peer links only to the peer holding the rendezvous port; two
/// later peers still reach each other, routed through it.
#[test]
fn later_peers_reach_each_other_through_the_rendezvous_peer() {
    let c = conf();
    let (holder, j1, j2) = (
        PeerTransport::new(),
        PeerTransport::new(),
        PeerTransport::new(),
    );
    holder.start(&key('a'), c.clone().wrap()).unwrap();
    j1.start(&key('b'), c.clone().wrap()).unwrap();
    j2.start(&key('c'), c.wrap()).unwrap();
    let got: Got = Arc::default();
    let _s = j2.subscribe(&session(12), sink(&got)).unwrap();
    let start = Instant::now();
    while got.lock().unwrap().is_empty() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "j1 never reached j2 (j1 linked to {}, j2 to {})",
            j1.connected_peers(),
            j2.connected_peers()
        );
        j1.publish(
            &session(12),
            Payload::new(PayloadKind::Envelope, b"j".to_vec()),
            later(),
        );
        thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        j1.connected_peers(),
        1,
        "a later peer linked to more than the rendezvous"
    );
    for t in [holder, j1, j2] {
        t.shutdown();
    }
}

/// When the transport holding the rendezvous port shuts down, a later transport is cut off
/// until another transport takes the port; then it reconnects and receives again.
#[test]
fn a_later_peer_reconnects_when_the_rendezvous_holder_is_replaced() {
    let c = conf();
    let (holder, j) = (PeerTransport::new(), PeerTransport::new());
    holder.start(&key('a'), c.clone().wrap()).unwrap();
    j.start(&key('b'), c.clone().wrap()).unwrap();
    let got: Got = Arc::default();
    let _s = j.subscribe(&session(13), sink(&got)).unwrap();
    holder.shutdown();
    let next = PeerTransport::new();
    next.start(&key('c'), c.wrap()).unwrap();
    let start = Instant::now();
    while got.lock().unwrap().is_empty() {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the later peer never reconnected"
        );
        next.publish(
            &session(13),
            Payload::new(PayloadKind::Envelope, b"again".to_vec()),
            later(),
        );
        thread::sleep(Duration::from_millis(200));
    }
    j.shutdown();
    next.shutdown();
}

/// A handler that stalls holds up only its own transport's later handlers: the link keeps
/// draining, the publisher is not blocked, and another transport keeps receiving
/// (binding document, "Timing").
#[test]
fn a_stalled_handler_blocks_no_publisher_and_no_other_peer() {
    let c = conf();
    let (a, b, other) = (
        PeerTransport::new(),
        PeerTransport::new(),
        PeerTransport::new(),
    );
    a.start(&key('a'), c.clone().wrap()).unwrap();
    b.start(&key('b'), c.clone().wrap()).unwrap();
    other.start(&key('c'), c.wrap()).unwrap();
    let release = Arc::new(Mutex::new(()));
    let held = release.lock().unwrap();
    let stalled = Arc::new(AtomicUsize::new(0));
    let (r, st) = (release.clone(), stalled.clone());
    let _stuck = b
        .subscribe(
            &session(10),
            Arc::new(move |_| {
                st.fetch_add(1, Ordering::SeqCst);
                let _wait = r.lock().unwrap();
            }),
        )
        .unwrap();
    let fine: Got = Arc::default();
    let _ok = other.subscribe(&session(11), sink(&fine)).unwrap();
    let start = Instant::now();
    while stalled.load(Ordering::SeqCst) == 0 {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "b never received"
        );
        a.publish(
            &session(10),
            Payload::new(PayloadKind::Envelope, b"s".to_vec()),
            later(),
        );
        thread::sleep(Duration::from_millis(100));
    }
    // b's handler is now stuck. Publishing to it, and to the other peer, still returns at
    // once, and the other peer still receives.
    let t = Instant::now();
    for _ in 0..50 {
        a.publish(
            &session(10),
            Payload::new(PayloadKind::Envelope, vec![0; 4096]),
            later(),
        );
    }
    assert!(
        t.elapsed() < Duration::from_secs(2),
        "publish blocked: {:?}",
        t.elapsed()
    );
    let t = Instant::now();
    while fine.lock().unwrap().is_empty() {
        assert!(
            t.elapsed() < Duration::from_secs(15),
            "the other peer stopped receiving"
        );
        a.publish(
            &session(11),
            Payload::new(PayloadKind::Envelope, b"f".to_vec()),
            later(),
        );
        thread::sleep(Duration::from_millis(100));
    }
    drop(held);
    for t in [a, b, other] {
        t.shutdown();
    }
}

type Got = Arc<Mutex<Vec<(PayloadKind, Vec<u8>)>>>;

fn sink(g: &Got) -> Arc<dyn Fn(Inbound) + Send + Sync> {
    let g = g.clone();
    Arc::new(move |i| {
        g.lock()
            .unwrap()
            .push((i.payload.kind(), i.payload.octets().to_vec()))
    })
}

/// Every frame reaches every peer of the partition (one native subscriber per transport,
/// for [IFC-TRN-043]), so the local filter is what keeps content from the wrong consumer:
/// an envelope reaches only that session's subscriptions, a receipt only the named
/// device's subscription, a presence record only the named device's watchers, and nothing
/// reaches another device's transport.
#[test]
fn frames_reach_only_their_own_local_consumer() {
    let c = conf();
    let (a, b, other) = (
        PeerTransport::new(),
        PeerTransport::new(),
        PeerTransport::new(),
    );
    a.start(&key('a'), c.clone().wrap()).unwrap();
    b.start(&key('b'), c.clone().wrap()).unwrap();
    other.start(&key('c'), c.wrap()).unwrap();
    let [s1, s2, s3, bdev, cdev, bprs, cprs]: [Got; 7] = Default::default();
    let _subs = [
        b.subscribe(&session(1), sink(&s1)).unwrap(),
        b.subscribe(&session(2), sink(&s2)).unwrap(),
        other.subscribe(&session(3), sink(&s3)).unwrap(),
        b.subscribe(&Destination::Device(key('b')), sink(&bdev))
            .unwrap(),
        other
            .subscribe(&Destination::Device(key('c')), sink(&cdev))
            .unwrap(),
    ];
    for (t, g) in [(&b, &bprs), (&other, &cprs)] {
        let g = g.clone();
        t.watch_presence(Arc::new(move |e| {
            if let PresenceEvent::Record { payload, .. } = e {
                g.lock()
                    .unwrap()
                    .push((payload.kind(), payload.octets().to_vec()));
            }
        }))
        .unwrap();
    }
    let dev_b = Destination::Device(key('b'));
    let start = Instant::now();
    while s1.lock().unwrap().is_empty()
        || bdev.lock().unwrap().is_empty()
        || bprs.lock().unwrap().is_empty()
    {
        assert!(start.elapsed() < Duration::from_secs(15), "nothing arrived");
        a.publish(
            &session(1),
            Payload::new(PayloadKind::Envelope, b"e".to_vec()),
            later(),
        );
        a.publish(
            &dev_b,
            Payload::new(PayloadKind::Receipt, b"r".to_vec()),
            later(),
        );
        a.send_presence(
            &dev_b,
            Payload::new(PayloadKind::Presence, b"p".to_vec()),
            later(),
        );
        thread::sleep(Duration::from_millis(100));
    }
    thread::sleep(Duration::from_millis(500));
    let only = |g: &Got, k: PayloadKind, o: &[u8]| {
        g.lock().unwrap().iter().all(|(gk, go)| *gk == k && go == o)
    };
    assert!(only(&s1, PayloadKind::Envelope, b"e"));
    assert!(only(&bdev, PayloadKind::Receipt, b"r"));
    assert!(only(&bprs, PayloadKind::Presence, b"p"));
    for (name, g) in [
        ("session 2", &s2),
        ("session 3", &s3),
        ("device c", &cdev),
        ("device c presence", &cprs),
    ] {
        let got = g.lock().unwrap().clone();
        assert!(got.is_empty(), "{name} received {got:?}");
    }
    for t in [a, b, other] {
        t.shutdown();
    }
}
