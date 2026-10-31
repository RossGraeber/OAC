// SPDX-License-Identifier: Apache-2.0

//! #63 (G2): carrier loss from native Zenoh liveliness, over loopback, default tier.
//!
//! One test per acceptance item, plus the measurements the binding document records. Raw
//! native sessions stand in for a foreign Zenoh participant and for controls ("what does
//! Zenoh do without `history(true)`"); the transport under test is always `PeerTransport`.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use oac_core::ids::KeyId;
use oac_core::transport::{
    CarrierHandle, Deadline, Destination, Payload, PayloadKind, PresenceEvent, Transport,
};
use zenoh::Wait;
use zenoh::sample::SampleKind;

use crate::addressing::{Partition, token_name};
use crate::config::Role;
use crate::ports::{Claimed, PortClaim, claimed_rebuildable};
use crate::{LINK_LEASE_MILLIS, PeerConfiguration, PeerTransport};

/// A configuration on a rendezvous port claimed for as long as it lives (`crate::ports`).
/// Several tests of this crate run in parallel, each opening sockets, and a rendezvous port
/// that something else already answers on is refused by design, so a port is claimed and
/// never probed and released. The first start of a test goes through `Claimed::start_first`,
/// which moves to a fresh port, bounded and logged, if a program outside the claim protocol
/// took the port in between (#432).
fn conf(tag: &str) -> Claimed<PeerConfiguration> {
    let tag = tag.to_owned();
    claimed_rebuildable(move |port| {
        PeerConfiguration::rendezvous(port).with_partition(format!("live-{tag}-{port}"))
    })
}
fn key(c: char) -> KeyId {
    KeyId::parse(&c.to_string().repeat(64)).unwrap()
}
fn wait_for(what: &str, limit: Duration, cond: impl Fn() -> bool) -> Duration {
    let start = Instant::now();
    while !cond() {
        assert!(start.elapsed() < limit, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(5));
    }
    start.elapsed()
}
fn later() -> Deadline {
    Deadline::at(Instant::now() + Duration::from_secs(30))
}

/// The presence events a transport handed to its `watch_presence` handler.
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<(Instant, PresenceEvent)>>>);
impl Events {
    fn watch(&self, t: &PeerTransport) {
        let e = self.0.clone();
        t.watch_presence(Arc::new(move |event| {
            e.lock().unwrap().push((Instant::now(), event));
        }))
        .unwrap();
    }
    fn losses(&self) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, e)| matches!(e, PresenceEvent::CarrierLoss { .. }))
            .count()
    }
    fn all(&self) -> usize {
        self.0.lock().unwrap().len()
    }
    fn nth_loss(&self, n: usize) -> Option<Instant> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, e)| matches!(e, PresenceEvent::CarrierLoss { .. }))
            .nth(n)
            .map(|(at, _)| *at)
    }
    fn first_loss(&self) -> Option<(Instant, CarrierHandle)> {
        self.0.lock().unwrap().iter().find_map(|(at, e)| match e {
            PresenceEvent::CarrierLoss { carrier } => Some((*at, carrier.clone())),
            PresenceEvent::Record { .. } | PresenceEvent::CarrierAppeared { .. } => None,
        })
    }
}

/// The carrier handle of the frames `receiver` gets: publish until one arrives.
fn frame_carrier(
    sender: &PeerTransport,
    receiver: &PeerTransport,
    device: &KeyId,
) -> CarrierHandle {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let _sub = receiver
        .subscribe(
            &Destination::Device(device.clone()),
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
        sender.publish(
            &Destination::Device(device.clone()),
            Payload::new(PayloadKind::Sealed, vec![9, 9, 9]),
            later(),
        );
        if let Ok(c) = rx.recv_timeout(Duration::from_millis(100)) {
            return c;
        }
    }
}

/// A raw native session on the transport's own configuration: a foreign Zenoh participant,
/// a control, never the transport under test.
fn raw_session(c: &PeerConfiguration) -> zenoh::Session {
    zenoh::open(c.native(Role::Joiner, 0).unwrap())
        .wait()
        .unwrap()
}

// ---- acceptance 1: a newly started peer learns of peers already online ----------------

/// History replay: a transport started after others learns they are alive, with no event
/// since its own start. Controls with raw native subscribers show what history adds.
#[test]
fn a_late_started_peer_learns_of_peers_already_online() {
    let mut c = conf("late");
    let (a, b, late) = (
        PeerTransport::new(),
        PeerTransport::new(),
        PeerTransport::new(),
    );
    c.start_first(|conf| a.start(&key('a'), conf.clone().wrap()))
        .unwrap();
    b.start(&key('b'), c.clone().wrap()).unwrap();
    wait_for("a and b to see each other", Duration::from_secs(15), || {
        a.observed_live_links() == 1 && b.observed_live_links() == 1
    });
    // `late` starts after both tokens were declared; nothing is declared afterwards.
    late.start(&key('c'), c.clone().wrap()).unwrap();
    let took = wait_for(
        "the late peer to learn of both",
        Duration::from_secs(15),
        || late.observed_live_links() == 2,
    );
    println!("MEASURED late peer learned 2 online peers from history in {took:?}");
    // The native control: without `history(true)` Zenoh tells a subscriber nothing about a
    // token declared before it subscribed; history does.
    let raw = raw_session(&c);
    let pattern = Partition::new(c.partition()).token_pattern();
    let (plain_tx, plain_rx) = mpsc::channel();
    let _plain = raw
        .liveliness()
        .declare_subscriber(&pattern)
        .callback(move |s| {
            let _ = plain_tx.send((s.kind(), s.key_expr().to_string()));
        })
        .wait()
        .unwrap();
    let (hist_tx, hist_rx) = mpsc::channel();
    let _hist = raw
        .liveliness()
        .declare_subscriber(&pattern)
        .history(true)
        .callback(move |s| {
            let _ = hist_tx.send((s.kind(), s.key_expr().to_string()));
        })
        .wait()
        .unwrap();
    let mut replayed = Vec::new();
    while replayed.len() < 3 {
        let (kind, k) = hist_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("history replays every live token");
        assert_eq!(kind, SampleKind::Put);
        replayed.push(k);
    }
    // a, b and late: three distinct opaque token keys, none of them a device or session id.
    replayed.sort();
    replayed.dedup();
    assert_eq!(replayed.len(), 3);
    // Zenoh replayed nothing to the subscriber without history, though all three tokens were
    // declared before it subscribed (the history subscriber above, declared after it, got
    // all three): history is what teaches a late starter.
    let plain: Vec<_> = plain_rx.try_iter().collect();
    println!(
        "MEASURED subscriber without history saw {} pre-existing tokens",
        plain.len()
    );
    assert!(
        plain.is_empty(),
        "a subscriber without history saw {plain:?}"
    );
    // A native `liveliness().get` is a `liveliness_query`, which the ACL does not allow:
    // history returns the same set, so the rule stays minimal. Asked from a fresh session with
    // no liveliness subscription, so that nothing can answer from local knowledge.
    let probe = raw_session(&c);
    let replies = probe.liveliness().get(&pattern).wait().unwrap();
    let mut got = 0;
    while let Ok(Some(reply)) = replies.recv_timeout(Duration::from_secs(2)) {
        if reply.result().is_ok() {
            got += 1;
        }
    }
    assert_eq!(got, 0, "the ACL refuses a liveliness query");
    drop((_plain, _hist));
    let _ = probe.close().wait();
    let _ = raw.close().wait();
    for t in [a, b, late] {
        t.shutdown();
    }
}

/// What a late start is worth at the neutral surface: the early peer exits after the late
/// one started, and the late one reports a carrier loss for the handle its frames carry.
#[test]
fn a_late_peer_reports_the_loss_of_a_peer_that_was_online_before_it_started() {
    let mut c = conf("late-loss");
    let (early, late) = (PeerTransport::new(), PeerTransport::new());
    c.start_first(|conf| early.start(&key('a'), conf.clone().wrap()))
        .unwrap();
    late.start(&key('b'), c.clone().wrap()).unwrap();
    let events = Events::default();
    events.watch(&late);
    wait_for("history", Duration::from_secs(15), || {
        late.observed_live_links() == 1
    });
    let carrier = frame_carrier(&early, &late, &key('b'));
    assert_eq!(
        events.losses(),
        0,
        "no loss is reported while the peer lives"
    );
    early.shutdown();
    wait_for("the loss", Duration::from_secs(15), || events.losses() == 1);
    assert_eq!(events.first_loss().unwrap().1, carrier);
    late.shutdown();
}

// ---- carrier appearance (IFC-TRN-120 to IFC-TRN-122; #419) -----------------------------

/// A `watch_presence` handler that forwards each event to a channel, so that a test blocks
/// on the event itself instead of sampling for it.
fn forwarded(t: &PeerTransport) -> mpsc::Receiver<PresenceEvent> {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    t.watch_presence(Arc::new(move |event| {
        let _ = tx.lock().unwrap().send(event);
    }))
    .unwrap();
    rx
}

/// The next event, which must arrive within the test's bound.
fn next_event(rx: &mpsc::Receiver<PresenceEvent>, what: &str) -> PresenceEvent {
    rx.recv_timeout(Duration::from_secs(15))
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

/// [IFC-TRN-120], [IFC-TRN-121], [IFC-TRN-122], [IFC-TRN-090]: a peer already online when
/// this transport started is reported as an appearance. Native history replayed its token
/// before any handler existed (the test waits for that), and the handler registered
/// afterwards still hears of it, from the transport, without being asked. The event is the
/// carrier handle and nothing else, and it is the handle the peer's frames carry.
#[test]
fn a_late_started_peer_reports_a_carrier_appearance_for_a_link_already_usable_at_start() {
    let mut c = conf("appear-late");
    let (early, late) = (PeerTransport::new(), PeerTransport::new());
    c.start_first(|conf| early.start(&key('a'), conf.clone().wrap()))
        .unwrap();
    late.start(&key('b'), c.clone().wrap()).unwrap();
    // History has delivered the early peer's token; no handler exists yet.
    wait_for("history", Duration::from_secs(15), || {
        late.observed_live_links() == 1
    });
    let events = forwarded(&late);
    let appeared = next_event(&events, "the appearance of the early peer");
    // Only a handle: matching the variant exhaustively leaves no other member.
    let PresenceEvent::CarrierAppeared { carrier } = appeared else {
        panic!("expected carrier-appeared, got {appeared:?}");
    };
    assert_eq!(carrier, frame_carrier(&early, &late, &key('b')));
    assert_eq!(format!("{carrier:?}"), "CarrierHandle(..)");
    early.shutdown();
    // The link that appeared is the link whose loss follows, under the same handle.
    match next_event(&events, "the loss of the early peer") {
        PresenceEvent::CarrierLoss { carrier: lost } => assert_eq!(lost, carrier),
        other => panic!("expected carrier-loss, got {other:?}"),
    }
    late.shutdown();
}

/// [IFC-TRN-121], [IFC-TRN-122]: a peer that starts while this transport is running is
/// reported as an appearance, once per new link; a link that went and came back is a new
/// appearance.
#[test]
fn a_peer_that_starts_during_normal_operation_reports_a_carrier_appearance() {
    let mut c = conf("appear-live");
    let (watching, first, second) = (
        PeerTransport::new(),
        PeerTransport::new(),
        PeerTransport::new(),
    );
    c.start_first(|conf| watching.start(&key('a'), conf.clone().wrap()))
        .unwrap();
    let events = forwarded(&watching);
    first.start(&key('b'), c.clone().wrap()).unwrap();
    let PresenceEvent::CarrierAppeared { carrier } =
        next_event(&events, "the appearance of the first peer")
    else {
        panic!("expected carrier-appeared");
    };
    assert_eq!(carrier, frame_carrier(&first, &watching, &key('a')));
    second.start(&key('c'), c.clone().wrap()).unwrap();
    match next_event(&events, "the appearance of the second peer") {
        PresenceEvent::CarrierAppeared { carrier: again } => assert_eq!(again, carrier),
        other => panic!("expected carrier-appeared, got {other:?}"),
    }
    // A peer that left and starts again is a link that has become usable again.
    first.shutdown();
    assert!(matches!(
        next_event(&events, "the loss of the first peer"),
        PresenceEvent::CarrierLoss { .. }
    ));
    let again = PeerTransport::new();
    again.start(&key('d'), c.clone().wrap()).unwrap();
    assert!(matches!(
        next_event(&events, "the appearance of the restarted peer"),
        PresenceEvent::CarrierAppeared { .. }
    ));
    for t in [&watching, &second, &again] {
        t.shutdown();
    }
    // Every event was one of those: no record, and nothing after shutdown.
    assert!(events.try_recv().is_err());
}

/// A token that is not another transport's of this partition is no carrier, so it is no
/// appearance either (the same inputs that `an_ambiguous_token_is_unknown_and_reports_no_loss`
/// shows report no loss).
#[test]
fn a_token_that_is_not_another_transports_reports_no_carrier_appearance() {
    let mut c = conf("appear-foreign");
    let t = PeerTransport::new();
    c.start_first(|conf| t.start(&key('a'), conf.clone().wrap()))
        .unwrap();
    let partition = Partition::new(c.partition());
    let events = forwarded(&t);
    let raw = raw_session(&c);
    let malformed = raw
        .liveliness()
        .declare_token(format!("{}/live/not-a-token", partition.prefix()))
        .wait();
    // Control, which also orders the checks: a well-formed token is the first event, so the
    // malformed token above, declared first, had its chance to be reported before it.
    let good = raw
        .liveliness()
        .declare_token(partition.token_key(&token_name("well formed")))
        .wait()
        .unwrap();
    assert!(matches!(
        next_event(&events, "the well formed token"),
        PresenceEvent::CarrierAppeared { .. }
    ));
    assert_eq!(t.observed_live_links(), 1);
    drop((malformed, good));
    let _ = raw.close().wait();
    t.shutdown();
}

// ---- acceptance 2: exit produces unreachable within a stated bound ---------------------

/// Graceful exit: the exiting transport undeclares its token before it closes. Measured
/// from the call to `shutdown` to the watcher's event, for the relay and for a joiner, and
/// seen by another joiner through the relay.
#[test]
fn a_graceful_exit_is_reported_at_once_to_every_remaining_peer() {
    let mut c = conf("graceful");
    let (relay, j1, j2) = (
        PeerTransport::new(),
        PeerTransport::new(),
        PeerTransport::new(),
    );
    c.start_first(|conf| relay.start(&key('a'), conf.clone().wrap()))
        .unwrap();
    j1.start(&key('b'), c.clone().wrap()).unwrap();
    j2.start(&key('c'), c.clone().wrap()).unwrap();
    let (er, e1, e2) = (Events::default(), Events::default(), Events::default());
    er.watch(&relay);
    e1.watch(&j1);
    e2.watch(&j2);
    wait_for("full visibility", Duration::from_secs(15), || {
        relay.observed_live_links() == 2
            && j1.observed_live_links() == 2
            && j2.observed_live_links() == 2
    });
    // A joiner leaves: the relay and the other joiner each report one loss.
    let left = Instant::now();
    j1.shutdown();
    // What j1 heard before it exited (the appearances of its peers); nothing may follow.
    let at_exit = e1.all();
    wait_for("relay loss", Duration::from_secs(15), || er.losses() == 1);
    wait_for("j2 loss", Duration::from_secs(15), || e2.losses() == 1);
    let (at_relay, at_j2) = (er.nth_loss(0).unwrap(), e2.nth_loss(0).unwrap());
    println!(
        "MEASURED graceful joiner exit, from the call to shutdown: relay {:?}, other joiner {:?}",
        at_relay.saturating_duration_since(left),
        at_j2.saturating_duration_since(left)
    );
    assert_eq!(j1.observed_live_links(), 0);
    assert_eq!(
        e1.all(),
        at_exit,
        "the exiting transport reports nothing after shutdown"
    );
    // The relay leaves: the remaining joiner reports it.
    let left = Instant::now();
    relay.shutdown();
    wait_for("relay-exit loss", Duration::from_secs(15), || {
        e2.losses() == 2
    });
    println!(
        "MEASURED graceful relay exit, from the call to shutdown: joiner {:?}",
        e2.nth_loss(1).unwrap().saturating_duration_since(left)
    );
    j2.shutdown();
}

/// Abrupt exit: a separate process dies without any shutdown. The operating system closes
/// its sockets and Zenoh reports the closed link at once, with no lease wait.
#[test]
fn an_abrupt_process_exit_is_reported_when_zenoh_sees_the_link_close() {
    let mut c = conf("abrupt");
    let relay = PeerTransport::new();
    c.start_first(|conf| relay.start(&key('a'), conf.clone().wrap()))
        .unwrap();
    let events = Events::default();
    events.watch(&relay);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "liveness_tests::child_peer_for_the_abrupt_exit_test",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_PORT, c.rendezvous_port().to_string())
        .env(CHILD_PARTITION, c.partition())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    struct Reap(std::process::Child);
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut out = child.stdout.take().unwrap();
    let reap = Mutex::new(Reap(child));
    let (ready_tx, ready_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut seen = String::new();
        let mut buf = [0u8; 256];
        while let Ok(n) = out.read(&mut buf) {
            if n == 0 {
                break;
            }
            seen.push_str(&String::from_utf8_lossy(&buf[..n]));
            if seen.contains("CHILD-READY") {
                let _ = ready_tx.send(());
                break;
            }
        }
        // Keep draining so the child never blocks on its stdout.
        while out.read(&mut buf).map(|n| n > 0).unwrap_or(false) {}
    });
    ready_rx
        .recv_timeout(Duration::from_secs(60))
        .expect("the child peer started");
    wait_for("the child's token", Duration::from_secs(15), || {
        relay.observed_live_links() == 1
    });
    let killed = Instant::now();
    reap.lock().unwrap().0.kill().unwrap();
    wait_for(
        "the loss after process death",
        Duration::from_millis(LINK_LEASE_MILLIS) + Duration::from_secs(10),
        || events.losses() == 1,
    );
    let seen = events
        .first_loss()
        .unwrap()
        .0
        .saturating_duration_since(killed);
    println!("MEASURED abrupt process exit, from the kill call to the event: {seen:?}");
    assert!(
        seen < Duration::from_millis(LINK_LEASE_MILLIS),
        "an abrupt exit is reported by the closed link, well inside the lease: {seen:?}"
    );
    relay.shutdown();
}

const CHILD_PORT: &str = "OAC_LIVENESS_CHILD_PORT";
const CHILD_PARTITION: &str = "OAC_LIVENESS_CHILD_PARTITION";

/// Not a test of its own: the peer process that `an_abrupt_process_exit_...` kills. Without
/// the environment variables it returns at once.
#[test]
fn child_peer_for_the_abrupt_exit_test() {
    let (Ok(port), Ok(partition)) = (std::env::var(CHILD_PORT), std::env::var(CHILD_PARTITION))
    else {
        return;
    };
    let c = PeerConfiguration::rendezvous(port.parse().unwrap()).with_partition(partition);
    let t = PeerTransport::new();
    t.start(&key('c'), c.wrap()).unwrap();
    wait_for("the relay", Duration::from_secs(30), || {
        t.observed_live_links() == 1
    });
    println!("CHILD-READY");
    let _ = std::io::stdout().flush();
    thread::sleep(Duration::from_secs(120));
}

/// A loopback relay for the silent-loss test: it forwards bytes between two sockets and, once
/// `blackhole` is set, reads and discards them without closing either side, as a network
/// that stops delivering does. Neither end sees a close or an error.
struct Link {
    blackhole: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}
impl Link {
    fn start(listen: u16, target: u16) -> Link {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, listen)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (blackhole, stop) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let (b, s) = (blackhole.clone(), stop.clone());
        let accept = thread::spawn(move || {
            while !s.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((from, _)) => {
                        let Ok(to) = TcpStream::connect((Ipv4Addr::LOCALHOST, target)) else {
                            continue;
                        };
                        for (r, w) in [
                            (from.try_clone().unwrap(), to.try_clone().unwrap()),
                            (to, from),
                        ] {
                            let (b, s) = (b.clone(), s.clone());
                            thread::spawn(move || pump(r, w, &b, &s));
                        }
                    }
                    Err(_) => thread::sleep(Duration::from_millis(10)),
                }
            }
        });
        Link {
            blackhole,
            stop,
            accept: Some(accept),
        }
    }
}
fn pump(mut from: TcpStream, mut to: TcpStream, blackhole: &AtomicBool, stop: &AtomicBool) {
    from.set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    let mut buf = [0u8; 16 * 1024];
    while !stop.load(Ordering::SeqCst) {
        match from.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if !blackhole.load(Ordering::SeqCst) && to.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
            Err(_) => {}
        }
    }
}
impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.accept.take() {
            let _ = h.join();
        }
    }
}

/// Silent loss: the network stops delivering and no close is seen. Zenoh's own link lease
/// ends the link and deletes the tokens that came over it. This is the stated bound.
#[test]
fn a_silent_link_loss_is_reported_within_the_zenoh_link_lease() {
    let c = conf("silent");
    let partition = Partition::new(c.partition());
    // The relay is a raw native router on a port of its own; the transport under test
    // connects to the rendezvous port, which the loopback relay forwards to it.
    let real_claim = PortClaim::claim();
    let real = real_claim.port();
    let relay = zenoh::open(c.native(Role::First, real).unwrap())
        .wait()
        .unwrap();
    let _token = relay
        .liveliness()
        .declare_token(partition.token_key(&token_name("raw relay")))
        .wait()
        .unwrap();
    let link = Link::start(c.rendezvous_port(), real);
    let t = PeerTransport::new();
    t.start(&key('b'), c.clone().wrap()).unwrap();
    let events = Events::default();
    events.watch(&t);
    wait_for("the raw relay's token", Duration::from_secs(15), || {
        t.observed_live_links() == 1
    });
    thread::sleep(Duration::from_millis(500));
    assert_eq!(events.losses(), 0);
    let silent = Instant::now();
    link.blackhole.store(true, Ordering::SeqCst);
    let limit = Duration::from_millis(LINK_LEASE_MILLIS) + Duration::from_secs(6);
    wait_for("the lease to run out", limit, || events.losses() == 1);
    let seen = events.first_loss().unwrap().0 - silent;
    println!(
        "MEASURED silent link loss reported {seen:?} after the network went quiet (lease {LINK_LEASE_MILLIS} ms)"
    );
    assert!(
        seen <= Duration::from_millis(LINK_LEASE_MILLIS) + Duration::from_secs(5),
        "reported later than the lease plus scheduling: {seen:?}"
    );
    assert_eq!(t.observed_live_links(), 0);
    t.shutdown();
    drop(link);
    let _ = relay.close().wait();
}

// ---- acceptance 3: ambiguous states are unknown, never a guess ---------------------------

/// Tokens that are not this transport's, and tokens of this transport's form that were
/// never seen alive, say nothing about a carrier: no event, no change in the live links.
#[test]
fn an_ambiguous_token_is_unknown_and_reports_no_loss() {
    let mut c = conf("ambiguous");
    let t = PeerTransport::new();
    c.start_first(|conf| t.start(&key('a'), conf.clone().wrap()))
        .unwrap();
    // After the start: a retry on a fresh port renames the partition.
    let partition = Partition::new(c.partition());
    let events = Events::default();
    events.watch(&t);
    let raw = raw_session(&c);
    // A malformed name and a name from another partition: not ours.
    for foreign in [
        format!("{}/live/not-a-token", partition.prefix()),
        format!(
            "{}/live/{}",
            Partition::new("other").prefix(),
            token_name("x")
        ),
    ] {
        let tok = raw.liveliness().declare_token(&foreign).wait();
        // The native ACL may refuse the foreign partition outright; either way, nothing.
        thread::sleep(Duration::from_millis(300));
        drop(tok);
    }
    thread::sleep(Duration::from_millis(500));
    assert_eq!(t.observed_live_links(), 0);
    assert_eq!(events.all(), 0);
    // Control: a well-formed token seen alive and then lost is a loss.
    let tok = raw
        .liveliness()
        .declare_token(partition.token_key(&token_name("foreign but well formed")))
        .wait()
        .unwrap();
    wait_for("the token", Duration::from_secs(10), || {
        t.observed_live_links() == 1
    });
    drop(tok);
    wait_for("its loss", Duration::from_secs(10), || events.losses() == 1);
    assert_eq!(t.observed_live_links(), 0);
    let _ = raw.close().wait();
    t.shutdown();
}

// ---- reconnect, isolation and shutdown ---------------------------------------------------

/// After the relay is replaced, Zenoh reconnects the joiner and history resynchronises the
/// live set: the loss is reported once, and the new holder is seen alive.
#[test]
fn a_replaced_relay_is_reported_lost_and_the_new_one_is_seen_alive() {
    let mut c = conf("replaced");
    let (holder, joiner) = (PeerTransport::new(), PeerTransport::new());
    c.start_first(|conf| holder.start(&key('a'), conf.clone().wrap()))
        .unwrap();
    joiner.start(&key('b'), c.clone().wrap()).unwrap();
    let events = Events::default();
    events.watch(&joiner);
    wait_for("the holder's token", Duration::from_secs(15), || {
        joiner.observed_live_links() == 1
    });
    holder.shutdown();
    wait_for("the holder's loss", Duration::from_secs(15), || {
        events.losses() == 1
    });
    assert_eq!(joiner.observed_live_links(), 0);
    let next = PeerTransport::new();
    next.start(&key('c'), c.clone().wrap()).unwrap();
    let took = wait_for("the reconnect", Duration::from_secs(30), || {
        joiner.observed_live_links() == 1 && next.observed_live_links() == 1
    });
    println!("MEASURED relay replaced; joiner saw the new holder alive after {took:?}");
    assert_eq!(events.losses(), 1);
    joiner.shutdown();
    next.shutdown();
}

/// A watcher that stalls holds up neither the frame stream nor another peer's watcher.
#[test]
fn a_stalled_presence_watcher_blocks_neither_frames_nor_other_peers() {
    let mut c = conf("stalled");
    let (relay, a, b, c3) = (
        PeerTransport::new(),
        PeerTransport::new(),
        PeerTransport::new(),
        PeerTransport::new(),
    );
    c.start_first(|conf| relay.start(&key('d'), conf.clone().wrap()))
        .unwrap();
    a.start(&key('a'), c.clone().wrap()).unwrap();
    b.start(&key('b'), c.clone().wrap()).unwrap();
    c3.start(&key('c'), c.clone().wrap()).unwrap();
    let release = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicBool::new(false));
    let (r, e) = (release.clone(), entered.clone());
    b.watch_presence(Arc::new(move |event| {
        // Only a loss stalls this watcher; appearances at start pass through.
        if !matches!(event, PresenceEvent::CarrierLoss { .. }) {
            return;
        }
        e.store(true, Ordering::SeqCst);
        let t = Instant::now();
        while !r.load(Ordering::SeqCst) && t.elapsed() < Duration::from_secs(20) {
            thread::sleep(Duration::from_millis(5));
        }
    }))
    .unwrap();
    let other = Events::default();
    other.watch(&c3);
    wait_for("visibility", Duration::from_secs(15), || {
        b.observed_live_links() == 3 && c3.observed_live_links() == 3
    });
    a.shutdown();
    wait_for("b's watcher to stall", Duration::from_secs(15), || {
        entered.load(Ordering::SeqCst)
    });
    // b's frame stream still works while its watcher is stuck, and so does c's watcher.
    frame_carrier(&c3, &b, &key('b'));
    wait_for("c's own loss", Duration::from_secs(15), || {
        other.losses() == 1
    });
    release.store(true, Ordering::SeqCst);
    for t in [b, c3, relay] {
        t.shutdown();
    }
}

/// [IFC-TRN-071] end to end: nothing follows `shutdown`. One handler is held in flight
/// while a second loss is pending in the native signal ring, and `shutdown` starts while
/// the handler runs. This test does not prove the gate: `shutdown` undeclares the
/// liveliness subscriber and closes the session before it joins the loss thread, and
/// Zenoh's ring receiver holds only a weak reference to its ring, so Zenoh itself
/// discards the pending signal. The gate is proved deterministically at the loop by
/// `transport::tests::a_pending_loss_signal_reaches_no_watcher_once_the_gate_is_closed`
/// (ring sender kept alive) and by `presence::tests::a_loss_reaches_every_watcher_until_the_gate_closes`.
#[test]
fn no_loss_reaches_a_watcher_once_shutdown_has_begun() {
    let mut c = conf("after-shutdown");
    let (relay, a1, a2, b) = (
        PeerTransport::new(),
        PeerTransport::new(),
        PeerTransport::new(),
        PeerTransport::new(),
    );
    c.start_first(|conf| relay.start(&key('d'), conf.clone().wrap()))
        .unwrap();
    a1.start(&key('a'), c.clone().wrap()).unwrap();
    a2.start(&key('c'), c.clone().wrap()).unwrap();
    b.start(&key('b'), c.clone().wrap()).unwrap();
    let (calls, release) = (
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicBool::new(false)),
    );
    let (n, r) = (calls.clone(), release.clone());
    b.watch_presence(Arc::new(move |event| {
        // Only a loss holds the handler in flight; the appearances of the peers already
        // online when `b` started pass through.
        if !matches!(event, PresenceEvent::CarrierLoss { .. }) {
            return;
        }
        n.fetch_add(1, Ordering::SeqCst);
        let t = Instant::now();
        while !r.load(Ordering::SeqCst) && t.elapsed() < Duration::from_secs(20) {
            thread::sleep(Duration::from_millis(5));
        }
    }))
    .unwrap();
    wait_for("visibility", Duration::from_secs(15), || {
        b.observed_live_links() == 3
    });
    // The first loss puts the handler in flight (it waits for `release`)...
    a1.shutdown();
    wait_for("the handler in flight", Duration::from_secs(15), || {
        calls.load(Ordering::SeqCst) == 1
    });
    // ...the second is seen by the native subscriber and waits in the ring behind it.
    a2.shutdown();
    wait_for(
        "the second loss to be seen",
        Duration::from_secs(15),
        || b.observed_live_links() == 1,
    );
    let closing = {
        let b = b.clone();
        thread::spawn(move || b.shutdown())
    };
    // `shutdown` closes the gate, then waits for the handler in flight; `b`'s session is
    // still alive until the handler returns.
    thread::sleep(Duration::from_millis(300));
    release.store(true, Ordering::SeqCst);
    closing.join().unwrap();
    thread::sleep(Duration::from_millis(500));
    assert_eq!(calls.load(Ordering::SeqCst), 1, "a loss followed shutdown");
    relay.shutdown();
}

// ---- acceptance 4: nothing Zenoh-specific is visible ------------------------------------

/// The token key, its opaque name and the native ids are held by the transport only: none
/// contains a device key id or a session id, and the carrier handle prints nothing.
#[test]
fn token_names_and_carrier_losses_name_no_device_or_session() {
    let mut c = conf("names");
    let (a, b) = (PeerTransport::new(), PeerTransport::new());
    let (ka, kb) = (key('a'), key('b'));
    c.start_first(|conf| a.start(&ka, conf.clone().wrap()))
        .unwrap();
    b.start(&kb, c.clone().wrap()).unwrap();
    let events = Events::default();
    events.watch(&b);
    wait_for("visibility", Duration::from_secs(15), || {
        b.observed_live_links() == 1
    });
    a.shutdown();
    wait_for("the loss", Duration::from_secs(15), || events.losses() == 1);
    let (_, carrier) = events.first_loss().unwrap();
    assert_eq!(format!("{carrier:?}"), "CarrierHandle(..)");
    for t in [&a, &b] {
        for id in t.identifiers_for_leak_checks() {
            for needle in [ka.as_str(), kb.as_str()] {
                assert!(!id.contains(needle), "{id} contains a device key id");
            }
        }
    }
    // Both tokens' native keys were among the identifiers the contract audit scans.
    let tokens: Vec<_> = a
        .identifiers_for_leak_checks()
        .into_iter()
        .filter(|s| s.contains("/live/"))
        .collect();
    assert_eq!(tokens.len(), 1);
    b.shutdown();
}
