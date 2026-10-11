// SPDX-License-Identifier: Apache-2.0

//! The Zenoh transport's own behaviour beyond the shared contract suite: the language
//! binding's errors (`oac_core::transport::TransportError`), the `not-taken` cases of
//! Table 6.1, and the neutral public surface (C7 §2).

use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, TcpListener, UdpSocket};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use zenoh::Wait;

use oac_core::health::HealthState;
use oac_core::ids::{KeyId, SessionId};
use oac_core::transport::{
    Deadline, Destination, Inbound, Payload, PayloadKind, PublishResult, Transport,
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
    assert!(caps.sealing);
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
            &dev,
            Payload::new(
                PayloadKind::Sealed,
                vec![0; MAX_PAYLOAD_OCTETS as usize + 1],
            ),
            later(),
        ),
        // A deadline already reached.
        t.publish(
            &dev,
            Payload::new(PayloadKind::Sealed, b"x".to_vec()),
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
        .subscribe(
            &Destination::Device(key('a')),
            Arc::new(move |i| g.lock().unwrap().push(i)),
        )
        .unwrap();
    t.shutdown();
    t.start(&key('a'), c.wrap()).unwrap();
    // The subscription made before the restart is gone with it ([IFC-TRN-035]).
    for _ in 0..5 {
        t.publish(
            &Destination::Device(key('a')),
            Payload::new(PayloadKind::Sealed, b"x".to_vec()),
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

/// The opt-in switch for tests that listen beyond loopback on purpose. Every other test in
/// the default tier binds loopback only, so a default run starts no listener the host
/// firewall filters (on Windows, each rebuilt test executable that listens on a LAN
/// address raises a firewall prompt). `node scripts/local-ci.mjs --tier lan` sets it for
/// its own child; `scripts/check-test-listeners.mjs` allows a non-loopback bind only in a
/// test that checks this switch first.
const LAN_OPT_IN: &str = "OAC_TEST_LAN";

/// True when [`LAN_OPT_IN`] is `1`. Otherwise writes a SKIPPED line straight to the
/// process's stderr, past the test harness's output capture, so a default run shows it.
fn lan_opt_in(test: &str) -> bool {
    if std::env::var(LAN_OPT_IN).as_deref() == Ok("1") {
        return true;
    }
    let _ = writeln!(
        std::io::stderr(),
        "SKIPPED {test}: listens beyond loopback on purpose; opt-in only \
         ({LAN_OPT_IN}=1, set by `node scripts/local-ci.mjs --tier lan`)"
    );
    false
}

/// PR #364 review, blocking finding 1: local mode never reaches beyond loopback. A plain
/// Zenoh peer listening on this host's LAN address, with multicast scouting on (interface
/// "auto"), gossip on and a subscriber on every key, receives none of the frames two local
/// transports exchange, and is never linked to either of them.
///
/// Opt-in ([`LAN_OPT_IN`]): the probe is a LAN listener and a multicast scout by design.
/// When opted in, a host with no LAN address fails the test rather than skipping it, so an
/// opt-in run never passes without having probed.
#[test]
fn local_mode_reaches_nothing_beyond_loopback() {
    if !lan_opt_in("local_mode_reaches_nothing_beyond_loopback") {
        return;
    }
    /// A non-loopback IPv4 address of this host, or why there is none. Connecting a UDP
    /// socket sends nothing; it only asks the OS which source address it would use. Kept
    /// inside the opt-in test, after its gate: it binds the unspecified address.
    fn lan_address() -> Result<Ipv4Addr, String> {
        let s = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(|e| e.to_string())?;
        s.connect(("192.0.2.1", 9))
            .map_err(|e| format!("no route off the host: {e}"))?;
        match s.local_addr().map_err(|e| e.to_string())?.ip() {
            IpAddr::V4(a) if !a.is_loopback() && !a.is_unspecified() => Ok(a),
            other => Err(format!("the only source address is {other}")),
        }
    }
    let lan = lan_address().unwrap_or_else(|why| {
        panic!("{LAN_OPT_IN}=1 but this host has no non-loopback interface to probe from ({why})")
    });
    let mut probe_conf = zenoh::Config::default();
    for (k, v) in [
        ("mode", r#""peer""#.to_owned()),
        ("listen/endpoints", format!(r#"["tcp/{lan}:0"]"#)),
        ("scouting/multicast/enabled", "true".to_owned()),
        ("scouting/gossip/enabled", "true".to_owned()),
    ] {
        probe_conf.insert_json5(k, &v).unwrap();
    }
    let probe = zenoh::open(probe_conf)
        .wait()
        .unwrap_or_else(|e| panic!("{LAN_OPT_IN}=1 but the probe cannot listen on {lan}: {e}"));
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
    let _s = b
        .subscribe(&Destination::Device(key('b')), sink(&got))
        .unwrap();
    let start = Instant::now();
    let mut probe_linked = 0;
    while got.lock().unwrap().is_empty() || start.elapsed() < Duration::from_secs(4) {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the control never arrived"
        );
        a.publish(
            &Destination::Device(key('b')),
            Payload::new(PayloadKind::Sealed, b"local only".to_vec()),
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
    let _s = j2
        .subscribe(&Destination::Device(key('c')), sink(&got))
        .unwrap();
    let start = Instant::now();
    while got.lock().unwrap().is_empty() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "j1 never reached j2 (j1 linked to {}, j2 to {})",
            j1.connected_peers(),
            j2.connected_peers()
        );
        j1.publish(
            &Destination::Device(key('c')),
            Payload::new(PayloadKind::Sealed, b"j".to_vec()),
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
    let _s = j
        .subscribe(&Destination::Device(key('b')), sink(&got))
        .unwrap();
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
            &Destination::Device(key('b')),
            Payload::new(PayloadKind::Sealed, b"again".to_vec()),
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
            &Destination::Device(key('b')),
            Arc::new(move |_| {
                st.fetch_add(1, Ordering::SeqCst);
                let _wait = r.lock().unwrap();
            }),
        )
        .unwrap();
    let fine: Got = Arc::default();
    let _ok = other
        .subscribe(&Destination::Device(key('c')), sink(&fine))
        .unwrap();
    let start = Instant::now();
    while stalled.load(Ordering::SeqCst) == 0 {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "b never received"
        );
        a.publish(
            &Destination::Device(key('b')),
            Payload::new(PayloadKind::Sealed, b"s".to_vec()),
            later(),
        );
        thread::sleep(Duration::from_millis(100));
    }
    // b's handler is now stuck. Publishing to it, and to the other peer, still returns at
    // once, and the other peer still receives.
    let t = Instant::now();
    for _ in 0..50 {
        a.publish(
            &Destination::Device(key('b')),
            Payload::new(PayloadKind::Sealed, vec![0; 4096]),
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
            &Destination::Device(key('c')),
            Payload::new(PayloadKind::Sealed, b"f".to_vec()),
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

/// IFC-TRN-105/050: all kinds' sealed frames go unchanged to each local device stream;
/// the transport never chooses a consumer from a clear-text destination or kind.
#[test]
fn sealed_frames_reach_every_device_stream_and_no_session_or_watch() {
    let c = conf();
    let (a, b, other) = (
        PeerTransport::new(),
        PeerTransport::new(),
        PeerTransport::new(),
    );
    a.start(&key('a'), c.clone().wrap()).unwrap();
    b.start(&key('b'), c.clone().wrap()).unwrap();
    other.start(&key('c'), c.wrap()).unwrap();
    let [own, bdev, cdev, session_got]: [Got; 4] = Default::default();
    let _subs = [
        a.subscribe(&Destination::Device(key('a')), sink(&own))
            .unwrap(),
        b.subscribe(&Destination::Device(key('b')), sink(&bdev))
            .unwrap(),
        other
            .subscribe(&Destination::Device(key('c')), sink(&cdev))
            .unwrap(),
        b.subscribe(&session(99), sink(&session_got)).unwrap(),
    ];
    let watches = Arc::new(AtomicUsize::new(0));
    let w = watches.clone();
    b.watch_presence(Arc::new(move |_| {
        w.fetch_add(1, Ordering::SeqCst);
    }))
    .unwrap();
    // Actual HPKE frames of every inner kind are exercised by the shared suite.
    // These arbitrary octets prove the transport never parses or alters the sealed bytes.
    let start = Instant::now();
    while [&own, &bdev, &cdev]
        .iter()
        .any(|g| g.lock().unwrap().is_empty())
    {
        assert!(start.elapsed() < Duration::from_secs(15));
        assert_eq!(
            a.publish(
                &Destination::Device(key('b')),
                Payload::new(PayloadKind::Sealed, vec![0, 255, 7]),
                later()
            ),
            PublishResult::Taken
        );
        thread::sleep(Duration::from_millis(100));
    }
    for g in [&own, &bdev, &cdev] {
        assert!(
            g.lock()
                .unwrap()
                .iter()
                .all(|(kind, bytes)| *kind == PayloadKind::Sealed && bytes == &[0, 255, 7])
        );
    }
    assert!(session_got.lock().unwrap().is_empty());
    assert_eq!(watches.load(Ordering::SeqCst), 0);
    for t in [a, b, other] {
        t.shutdown();
    }
}
