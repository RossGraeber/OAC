// SPDX-License-Identifier: Apache-2.0

//! Unit tests of the in-memory transport against the transport-owned requirements of
//! `spec/interfaces.md` §6 (#56, F7). The shared transport contract suite is F10 (#59); it
//! runs against `oac_core::transport::Transport`, which this crate implements.
//!
//! Every test runs in process, with no network and no clock but the network's own: a
//! `ManualClock` where deadlines or delays matter, and `MemoryNetwork::settle` to wait for
//! the delivery thread. The one exception is the real-clock purge test, which waits for
//! the system clock in a bounded loop; elsewhere waits are bounded channel receives.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use oac_core::health::HealthState;
use oac_core::ids::{KeyId, SessionId};
use oac_core::transport::{
    CarrierHandle, Deadline, Destination, Inbound, InboundHandler, Payload, PayloadKind,
    PresenceEvent, PresenceHandler, PublishResult, Reach, Transport, TransportConfiguration,
    TransportError,
};
use oac_transport_memory::{
    FixedDelay, MemoryConfiguration, MemoryNetwork, MemoryTransport, ScriptedFaults,
};

const SEC: Duration = Duration::from_secs(1);

fn key(digit: char) -> KeyId {
    KeyId::parse(&digit.to_string().repeat(64)).unwrap()
}

fn session(n: u8) -> Destination {
    Destination::Session(SessionId::from_random_octets([n; 16]))
}

fn envelope(text: &str) -> Payload {
    Payload::new(PayloadKind::Envelope, text.as_bytes().to_vec())
}

fn started(network: &MemoryNetwork, digit: char) -> MemoryTransport {
    let t = MemoryTransport::new();
    t.start(&key(digit), MemoryConfiguration::wrap(network))
        .unwrap();
    t
}

fn deadline(network: &MemoryNetwork, after: Duration) -> Deadline {
    Deadline::at(network.now() + after)
}

/// Records what a handler is given, and on which thread.
#[derive(Clone)]
struct Seen<T> {
    items: Arc<Mutex<Vec<T>>>,
    threads: Arc<Mutex<Vec<thread::ThreadId>>>,
}

impl<T> Default for Seen<T> {
    fn default() -> Self {
        Seen {
            items: Arc::default(),
            threads: Arc::default(),
        }
    }
}

impl<T: Clone + Send + 'static> Seen<T> {
    fn push(&self, item: T) {
        self.items.lock().unwrap().push(item);
        self.threads.lock().unwrap().push(thread::current().id());
    }
    fn get(&self) -> Vec<T> {
        self.items.lock().unwrap().clone()
    }
}

impl Seen<Inbound> {
    fn handler(&self) -> InboundHandler {
        let s = self.clone();
        Arc::new(move |i| s.push(i))
    }
    fn octets(&self) -> Vec<Vec<u8>> {
        self.get()
            .into_iter()
            .map(|i| i.payload.octets().to_vec())
            .collect()
    }
}

impl Seen<PresenceEvent> {
    fn handler(&self) -> PresenceHandler {
        let s = self.clone();
        Arc::new(move |e| s.push(e))
    }
}

fn cross() -> MemoryNetwork {
    MemoryNetwork::builder()
        .reach(Reach::CrossImplementation)
        .build()
}

// --- start and the declaration (§6.3) -------------------------------------------------

#[test]
fn declares_every_capability_honestly() {
    // [IFC-TRN-020], [IFC-TRN-021], [IFC-TRN-023], [IFC-TRN-026]
    let network = MemoryNetwork::new();
    let caps = MemoryTransport::new()
        .start(&key('1'), MemoryConfiguration::wrap(&network))
        .unwrap();
    assert!(caps.contract_violations().is_empty());
    assert!(!caps.reliability);
    assert!(!caps.persistence);
    assert!(!caps.offline_queueing);
    assert!(caps.ordering, "no faults: order is kept");
    assert!(!caps.multicast_discovery);
    assert!(!caps.routing_federation);
    assert_eq!(caps.reach, Reach::LocalOnly);
    assert!(!caps.destination_restricted);
    assert_eq!(caps.max_payload_octets, 65536);

    let scripted = MemoryNetwork::builder()
        .reach(Reach::CrossImplementation)
        .faults(ScriptedFaults::new())
        .max_payload_octets(1)
        .build();
    let caps = scripted.capabilities();
    assert!(
        !caps.ordering,
        "an injector that may reorder declares no ordering"
    );
    assert_eq!(caps.reach, Reach::CrossImplementation);
    assert_eq!(caps.max_payload_octets, 65536, "never below the floor");
    assert!(
        MemoryNetwork::builder()
            .faults(FixedDelay(SEC))
            .build()
            .capabilities()
            .ordering
    );
}

#[test]
fn start_refuses_what_it_cannot_do() {
    let network = MemoryNetwork::new();
    let t = MemoryTransport::new();
    assert_eq!(
        t.start(&key('1'), TransportConfiguration::new(17u32)),
        Err(TransportError::InvalidConfiguration(
            "not an in-memory transport configuration".into()
        ))
    );
    t.start(&key('1'), MemoryConfiguration::wrap(&network))
        .unwrap();
    assert_eq!(
        t.start(&key('1'), MemoryConfiguration::wrap(&network)),
        Err(TransportError::AlreadyStarted)
    );
    // One running transport per device key.
    assert!(matches!(
        MemoryTransport::new().start(&key('1'), MemoryConfiguration::wrap(&network)),
        Err(TransportError::InvalidConfiguration(_))
    ));
    // A local-only network carries for one device key ([IFC-TRN-002]).
    assert!(matches!(
        MemoryTransport::new().start(&key('2'), MemoryConfiguration::wrap(&network)),
        Err(TransportError::InvalidConfiguration(_))
    ));
    // A restart, for the same key, is allowed.
    t.shutdown();
    t.start(&key('1'), MemoryConfiguration::wrap(&network))
        .unwrap();
}

#[test]
fn operations_before_start_take_nothing() {
    let network = MemoryNetwork::new();
    let t = MemoryTransport::new();
    let d = deadline(&network, SEC);
    assert_eq!(
        t.publish(&session(1), envelope("x"), d),
        PublishResult::NotTaken
    );
    assert_eq!(
        t.send_presence(
            &Destination::Device(key('1')),
            Payload::new(PayloadKind::Presence, vec![1]),
            d
        ),
        PublishResult::NotTaken
    );
    assert!(matches!(
        t.subscribe(&session(1), Seen::<Inbound>::default().handler()),
        Err(TransportError::NotStarted)
    ));
    assert_eq!(
        t.watch_presence(Seen::<PresenceEvent>::default().handler()),
        Err(TransportError::NotStarted)
    );
}

#[test]
fn health_follows_the_lifecycle() {
    // [IFC-TYP-092]: the detail carries no endpoint number or other native value.
    let network = MemoryNetwork::new();
    let t = MemoryTransport::new();
    assert_eq!(t.health().state, HealthState::Unavailable);
    t.start(&key('1'), MemoryConfiguration::wrap(&network))
        .unwrap();
    assert_eq!(t.health().state, HealthState::Healthy);
    assert_eq!(t.health().detail, None);
    t.shutdown();
    let h = t.health();
    assert_eq!(h.state, HealthState::Unavailable);
    assert_eq!(h.detail.as_deref(), Some("shut down"));
}

// --- carrying payloads (§6.1, §6.4, §6.5) ---------------------------------------------

#[test]
fn carries_each_kind_exactly_on_its_own_initiative() {
    // [IFC-TRN-001], [IFC-TRN-030], [IFC-TRN-040], [IFC-TRN-050], [IFC-TRN-060]
    let network = cross();
    let a = started(&network, 'a');
    let b = started(&network, 'b');
    let d = deadline(&network, SEC);

    let envelopes = Seen::<Inbound>::default();
    let _s = b.subscribe(&session(1), envelopes.handler()).unwrap();
    let receipts = Seen::<Inbound>::default();
    let _r = b
        .subscribe(&Destination::Device(key('b')), receipts.handler())
        .unwrap();
    let presence = Seen::<PresenceEvent>::default();
    b.watch_presence(presence.handler()).unwrap();

    // Octets that are not text, and a full-size payload, arrive unchanged.
    let odd: Vec<u8> = (0..=255u8).rev().collect();
    let big = vec![0x5a; 65536];
    let envelope_odd = Payload::new(PayloadKind::Envelope, odd.clone());
    let envelope_big = Payload::new(PayloadKind::Envelope, big.clone());
    let receipt = Payload::new(PayloadKind::Receipt, b"receipt".to_vec());
    let record = Payload::new(PayloadKind::Presence, b"record".to_vec());
    assert_eq!(
        a.publish(&session(1), envelope_odd.clone(), d),
        PublishResult::Taken
    );
    assert_eq!(
        a.publish(&session(1), envelope_big.clone(), d),
        PublishResult::Taken
    );
    assert_eq!(
        a.publish(&Destination::Device(key('b')), receipt.clone(), d),
        PublishResult::Taken
    );
    assert_eq!(
        a.send_presence(&Destination::Device(key('b')), record.clone(), d),
        PublishResult::Taken
    );
    network.settle();

    assert_eq!(envelopes.octets(), vec![odd, big]);
    assert_eq!(envelopes.get()[0].payload, envelope_odd);
    assert_eq!(receipts.get().len(), 1);
    assert_eq!(receipts.get()[0].payload, receipt);
    let got = presence.get();
    assert_eq!(got.len(), 1);
    let PresenceEvent::Record { payload, carrier } = &got[0] else {
        panic!("expected a record");
    };
    assert_eq!(payload, &record);
    // One link from a to b: every payload over it names the same carrier.
    assert_eq!(carrier, &envelopes.get()[0].carrier);
    assert_eq!(carrier, &receipts.get()[0].carrier);
    // Handed over by the transport's thread, not inside the publisher's call.
    let me = thread::current().id();
    assert!(envelopes.threads.lock().unwrap().iter().all(|t| *t != me));
    assert!(presence.threads.lock().unwrap().iter().all(|t| *t != me));
}

#[test]
fn delivers_between_sessions_of_one_implementation() {
    // [IFC-TRN-002]: a local-only network, one implementation.
    let network = MemoryNetwork::new();
    let t = started(&network, '1');
    let seen = Seen::<Inbound>::default();
    let _s = t.subscribe(&session(2), seen.handler()).unwrap();
    assert_eq!(
        t.publish(&session(2), envelope("own"), deadline(&network, SEC)),
        PublishResult::Taken
    );
    network.settle();
    assert_eq!(seen.octets(), vec![b"own".to_vec()]);
}

#[test]
fn takes_nothing_that_cannot_be_delivered() {
    // [IFC-TRN-031]: each not-taken is a payload of which no copy can be delivered.
    let network = MemoryNetwork::builder().manual_clock().build();
    let t = started(&network, '1');
    let seen = Seen::<Inbound>::default();
    let _s = t.subscribe(&session(1), seen.handler()).unwrap();
    let dev = Destination::Device(key('1'));
    let d = deadline(&network, SEC);
    let receipt = Payload::new(PayloadKind::Receipt, vec![1]);
    let record = Payload::new(PayloadKind::Presence, vec![1]);
    // Kinds that do not fit Table 6.1, or the operation.
    assert_eq!(t.publish(&dev, envelope("x"), d), PublishResult::NotTaken);
    assert_eq!(
        t.publish(&session(1), receipt.clone(), d),
        PublishResult::NotTaken
    );
    assert_eq!(t.publish(&dev, record.clone(), d), PublishResult::NotTaken);
    assert_eq!(t.send_presence(&dev, receipt, d), PublishResult::NotTaken);
    assert_eq!(
        t.send_presence(&session(1), record, d),
        PublishResult::NotTaken
    );
    // Over max_payload_octets.
    let over = Payload::new(PayloadKind::Envelope, vec![0; 65537]);
    assert_eq!(t.publish(&session(1), over, d), PublishResult::NotTaken);
    // A deadline already reached.
    let now = Deadline::at(network.now());
    assert_eq!(
        t.publish(&session(1), envelope("late"), now),
        PublishResult::NotTaken
    );
    network.settle();
    assert!(seen.get().is_empty());
    assert_eq!(network.in_flight(), 0);
    // After shutdown.
    t.shutdown();
    assert_eq!(
        t.publish(&session(1), envelope("x"), d),
        PublishResult::NotTaken
    );
}

#[test]
fn device_subscription_must_name_the_local_device() {
    let network = cross();
    let a = started(&network, 'a');
    assert!(matches!(
        a.subscribe(
            &Destination::Device(key('b')),
            Seen::<Inbound>::default().handler()
        ),
        Err(TransportError::NotLocalDevice)
    ));
}

#[test]
fn keeps_order_without_faults() {
    // `ordering` is declared present for this network; check it holds.
    let network = MemoryNetwork::new();
    let t = started(&network, '1');
    let seen = Seen::<Inbound>::default();
    let _s = t.subscribe(&session(1), seen.handler()).unwrap();
    let d = deadline(&network, 60 * SEC);
    let sent: Vec<Vec<u8>> = (0..200u32).map(|i| i.to_be_bytes().to_vec()).collect();
    for s in &sent {
        let p = Payload::new(PayloadKind::Envelope, s.clone());
        assert_eq!(t.publish(&session(1), p, d), PublishResult::Taken);
    }
    network.settle();
    assert_eq!(seen.octets(), sent);
}

// --- not revealing subscriptions (§6.4) -----------------------------------------------

#[test]
fn publish_result_and_errors_do_not_depend_on_subscriptions() {
    // [IFC-TRN-043], [IFC-TRN-044]: a differential over a subscribed and an unsubscribed
    // destination, as seen by another implementation.
    let network = cross();
    let a = started(&network, 'a');
    let b = started(&network, 'b');
    let seen = Seen::<Inbound>::default();
    let _s = b.subscribe(&session(1), seen.handler()).unwrap();
    let d = deadline(&network, SEC);
    let subscribed = a.publish(&session(1), envelope("x"), d);
    let unsubscribed = a.publish(&session(2), envelope("x"), d);
    assert_eq!(subscribed, unsubscribed);
    assert_eq!(subscribed, PublishResult::Taken);
    // Subscribing to a destination someone else subscribed to is no error either.
    assert!(
        a.subscribe(&session(1), Seen::<Inbound>::default().handler())
            .is_ok()
    );
    assert!(
        a.subscribe(&session(3), Seen::<Inbound>::default().handler())
            .is_ok()
    );
    // And health says nothing about them.
    assert_eq!(a.health(), b.health());
}

// --- what a transport may hold (§6.4) -------------------------------------------------

#[test]
fn never_holds_or_delivers_at_or_after_the_deadline() {
    // [IFC-TRN-033], [IFC-TRN-034], with a scripted clock.
    let faults = ScriptedFaults::new();
    let network = MemoryNetwork::builder()
        .manual_clock()
        .faults(faults.clone())
        .build();
    let clock = network.manual_clock().unwrap();
    let t = started(&network, '1');
    let seen = Seen::<Inbound>::default();
    let _s = t.subscribe(&session(1), seen.handler()).unwrap();

    // Delayed to exactly its deadline: never queued.
    faults.delay(SEC);
    assert_eq!(
        t.publish(&session(1), envelope("at"), deadline(&network, SEC)),
        PublishResult::Taken
    );
    assert_eq!(network.in_flight(), 0);

    // Delayed past now, deadline before the delay ends: held while in flight, then
    // dropped when the deadline comes, not delivered.
    faults.delay(3 * SEC);
    let d = deadline(&network, 5 * SEC);
    faults.delay(10 * SEC); // reaches the deadline below: never queued
    assert_eq!(
        t.publish(&session(1), envelope("ok"), d),
        PublishResult::Taken
    );
    assert_eq!(
        t.publish(&session(1), envelope("late"), d),
        PublishResult::Taken
    );
    assert_eq!(network.in_flight(), 1);
    clock.advance(2 * SEC);
    network.settle();
    assert!(seen.get().is_empty());
    assert_eq!(network.in_flight(), 1, "in flight: may be held");
    clock.advance(SEC);
    network.settle();
    assert_eq!(seen.octets(), vec![b"ok".to_vec()]);
    assert_eq!(network.in_flight(), 0);

    // Queued, then its deadline comes before it is due: dropped at the deadline.
    faults.delay(4 * SEC);
    let d = deadline(&network, 5 * SEC);
    assert_eq!(
        t.publish(&session(1), envelope("x"), d),
        PublishResult::Taken
    );
    // A copy due before the deadline but not yet handed over when the clock jumps past
    // it is dropped, not delivered.
    clock.advance(6 * SEC);
    network.settle();
    assert_eq!(seen.octets(), vec![b"ok".to_vec()]);
    assert_eq!(network.in_flight(), 0);
}

#[test]
fn keeps_nothing_for_a_destination_not_reachable() {
    // [IFC-TRN-036]: no subscription when the payload is in flight, nothing later.
    let faults = ScriptedFaults::new();
    let network = MemoryNetwork::builder()
        .manual_clock()
        .faults(faults.clone())
        .build();
    let clock = network.manual_clock().unwrap();
    let t = started(&network, '1');
    faults.delay(SEC);
    let d = deadline(&network, 10 * SEC);
    assert_eq!(
        t.publish(&session(1), envelope("early"), d),
        PublishResult::Taken
    );
    // Subscribed while the copy is in flight: the copy was bound to nobody when taken.
    let seen = Seen::<Inbound>::default();
    let _s = t.subscribe(&session(1), seen.handler()).unwrap();
    clock.advance(2 * SEC);
    network.settle();
    assert!(seen.get().is_empty());
    assert_eq!(network.in_flight(), 0);

    // Same for a presence record with no watcher.
    assert_eq!(
        t.send_presence(
            &Destination::Device(key('1')),
            Payload::new(PayloadKind::Presence, vec![1]),
            d
        ),
        PublishResult::Taken
    );
    network.settle();
    assert_eq!(network.in_flight(), 0);
}

#[test]
fn nothing_crosses_a_restart_of_either_side() {
    // [IFC-TRN-035]
    let faults = ScriptedFaults::new();
    let network = MemoryNetwork::builder()
        .reach(Reach::CrossImplementation)
        .manual_clock()
        .faults(faults.clone())
        .build();
    let clock = network.manual_clock().unwrap();
    let a = started(&network, 'a');
    let b = started(&network, 'b');
    let d = deadline(&network, 60 * SEC);

    // The receiver restarts while a copy is in flight.
    let before = Seen::<Inbound>::default();
    let s = b.subscribe(&session(1), before.handler()).unwrap();
    faults.delay(SEC);
    assert_eq!(
        a.publish(&session(1), envelope("1"), d),
        PublishResult::Taken
    );
    assert_eq!(network.in_flight(), 1);
    drop(s);
    b.shutdown();
    assert_eq!(network.in_flight(), 0);
    b.start(&key('b'), MemoryConfiguration::wrap(&network))
        .unwrap();
    let after = Seen::<Inbound>::default();
    let _s = b.subscribe(&session(1), after.handler()).unwrap();
    clock.advance(2 * SEC);
    network.settle();
    assert!(before.get().is_empty());
    assert!(after.get().is_empty());

    // The sender restarts while a copy is in flight.
    faults.delay(SEC);
    assert_eq!(
        a.publish(&session(1), envelope("2"), d),
        PublishResult::Taken
    );
    a.shutdown();
    a.start(&key('a'), MemoryConfiguration::wrap(&network))
        .unwrap();
    clock.advance(2 * SEC);
    network.settle();
    assert!(after.get().is_empty());
    assert_eq!(network.in_flight(), 0);

    // And after both restarts, delivery works again.
    assert_eq!(
        a.publish(&session(1), envelope("3"), d),
        PublishResult::Taken
    );
    network.settle();
    assert_eq!(after.octets(), vec![b"3".to_vec()]);
}

// --- fault injection ([IFC-TRN-011]) --------------------------------------------------

#[test]
fn injects_loss_duplication_delay_and_reordering() {
    let faults = ScriptedFaults::new();
    let network = MemoryNetwork::builder()
        .manual_clock()
        .faults(faults.clone())
        .build();
    let clock = network.manual_clock().unwrap();
    let t = started(&network, '1');
    let seen = Seen::<Inbound>::default();
    let _s = t.subscribe(&session(1), seen.handler()).unwrap();
    let d = deadline(&network, 60 * SEC);

    faults.lose();
    faults.duplicate(2);
    faults.delay(2 * SEC);
    faults.deliver();
    for name in ["lost", "twice", "delayed", "overtakes"] {
        // Taken in every case: loss is the transport's, not a refusal.
        assert_eq!(
            t.publish(&session(1), envelope(name), d),
            PublishResult::Taken
        );
    }
    network.settle();
    assert_eq!(
        seen.octets(),
        vec![b"twice".to_vec(), b"twice".to_vec(), b"overtakes".to_vec()]
    );
    clock.advance(2 * SEC);
    network.settle();
    assert_eq!(seen.octets().last().unwrap(), b"delayed");
    assert_eq!(seen.get().len(), 4);
    assert_eq!(faults.pending(), 0);
}

// --- handlers, subscriptions and shutdown (§6.4, §6.6, §6.8) ---------------------------

#[test]
fn a_copy_reaches_every_subscription_still_open_when_due() {
    let faults = ScriptedFaults::new();
    let network = MemoryNetwork::builder()
        .reach(Reach::CrossImplementation)
        .manual_clock()
        .faults(faults.clone())
        .build();
    let clock = network.manual_clock().unwrap();
    let a = started(&network, 'a');
    let b = started(&network, 'b');
    let on_a = Seen::<Inbound>::default();
    let _sa = a.subscribe(&session(1), on_a.handler()).unwrap();
    let on_b = Seen::<Inbound>::default();
    let sb = b.subscribe(&session(1), on_b.handler()).unwrap();
    let on_b2 = Seen::<Inbound>::default();
    let _sb2 = b.subscribe(&session(1), on_b2.handler()).unwrap();
    faults.delay(SEC);
    a.publish(&session(1), envelope("x"), deadline(&network, 60 * SEC));
    sb.end();
    clock.advance(SEC);
    network.settle();
    assert_eq!(on_a.octets(), vec![b"x".to_vec()]);
    assert!(on_b.get().is_empty());
    assert_eq!(on_b2.octets(), vec![b"x".to_vec()]);
    // Links differ: a to a, a to b.
    assert_ne!(on_a.get()[0].carrier, on_b2.get()[0].carrier);
    assert_eq!(network.in_flight(), 0);
}

#[test]
fn no_handler_runs_after_shutdown_or_unsubscribe_returns() {
    // [IFC-TRN-071]; and the end of a subscription drops its copies.
    let faults = ScriptedFaults::new();
    let network = MemoryNetwork::builder()
        .manual_clock()
        .faults(faults.clone())
        .build();
    let clock = network.manual_clock().unwrap();
    let t = started(&network, '1');
    let seen = Seen::<Inbound>::default();
    let s = t.subscribe(&session(1), seen.handler()).unwrap();
    let watched = Seen::<PresenceEvent>::default();
    t.watch_presence(watched.handler()).unwrap();
    let d = deadline(&network, 60 * SEC);

    faults.delay(SEC);
    t.publish(&session(1), envelope("x"), d);
    s.end();
    assert_eq!(network.in_flight(), 0);

    let s2 = Seen::<Inbound>::default();
    let _keep = t.subscribe(&session(2), s2.handler()).unwrap();
    faults.delay(SEC);
    t.publish(&session(2), envelope("y"), d);
    faults.delay(SEC);
    t.send_presence(
        &Destination::Device(key('1')),
        Payload::new(PayloadKind::Presence, vec![1]),
        d,
    );
    t.shutdown();
    clock.advance(2 * SEC);
    network.settle();
    assert!(seen.get().is_empty());
    assert!(s2.get().is_empty());
    assert!(watched.get().is_empty());
}

/// A handler that blocks until released. `entered` fires when a call starts, one message
/// on `release` lets it finish, and `done` counts finished calls.
struct Blocking {
    handler: InboundHandler,
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
    done: Arc<AtomicUsize>,
}

fn blocking() -> Blocking {
    let (entered_tx, entered) = mpsc::channel::<()>();
    let (release, release_rx) = mpsc::channel::<()>();
    let entered_tx = Mutex::new(entered_tx);
    let release_rx = Mutex::new(release_rx);
    let done = Arc::new(AtomicUsize::new(0));
    let d = done.clone();
    Blocking {
        handler: Arc::new(move |_| {
            entered_tx.lock().unwrap().send(()).unwrap();
            release_rx.lock().unwrap().recv().unwrap();
            d.fetch_add(1, Ordering::SeqCst);
        }),
        entered,
        release,
        done,
    }
}

/// Long enough for an unwaiting call to return; a bounded wait, never a sleep.
const NOT_RETURNED: Duration = Duration::from_millis(300);
const BOUND: Duration = Duration::from_secs(10);

#[test]
fn shutdown_waits_for_a_running_handler() {
    // [IFC-TRN-071]: shutdown, called while a handler runs, returns only after it ends.
    let network = MemoryNetwork::new();
    let t = Arc::new(started(&network, '1'));
    let b = blocking();
    let _s = t.subscribe(&session(1), b.handler.clone()).unwrap();
    t.publish(&session(1), envelope("x"), deadline(&network, 60 * SEC));
    b.entered.recv_timeout(BOUND).unwrap();

    let (returned_tx, returned) = mpsc::channel();
    let (t2, done) = (t.clone(), b.done.clone());
    let stopper = thread::spawn(move || {
        t2.shutdown();
        returned_tx.send(done.load(Ordering::SeqCst)).unwrap();
    });
    assert!(
        returned.recv_timeout(NOT_RETURNED).is_err(),
        "shutdown returned while the handler was still running"
    );
    b.release.send(()).unwrap();
    assert_eq!(
        returned.recv_timeout(BOUND).unwrap(),
        1,
        "handler's work done"
    );
    stopper.join().unwrap();
}

#[test]
fn ending_a_subscription_waits_for_its_running_handler() {
    // Once `end` returns, the subscription's handler is not running and is not called.
    let network = MemoryNetwork::new();
    let t = started(&network, '1');
    let b = blocking();
    let sub = t.subscribe(&session(1), b.handler.clone()).unwrap();
    t.publish(&session(1), envelope("x"), deadline(&network, 60 * SEC));
    b.entered.recv_timeout(BOUND).unwrap();

    let (returned_tx, returned) = mpsc::channel();
    let done = b.done.clone();
    let ender = thread::spawn(move || {
        sub.end();
        returned_tx.send(done.load(Ordering::SeqCst)).unwrap();
    });
    assert!(
        returned.recv_timeout(NOT_RETURNED).is_err(),
        "end returned while the handler was still running"
    );
    b.release.send(()).unwrap();
    assert_eq!(
        returned.recv_timeout(BOUND).unwrap(),
        1,
        "handler's work done"
    );
    ender.join().unwrap();
}

#[test]
fn a_panicking_watcher_does_not_stop_the_other_watchers() {
    let network = MemoryNetwork::new();
    let t = started(&network, '1');
    t.watch_presence(Arc::new(|_| panic!("watcher fails")))
        .unwrap();
    let seen = Seen::<PresenceEvent>::default();
    t.watch_presence(seen.handler()).unwrap();
    let record = Payload::new(PayloadKind::Presence, b"r".to_vec());
    assert_eq!(
        t.send_presence(
            &Destination::Device(key('1')),
            record.clone(),
            deadline(&network, 60 * SEC)
        ),
        PublishResult::Taken
    );
    network.settle();
    let got = seen.get();
    assert_eq!(got.len(), 1);
    assert!(matches!(&got[0], PresenceEvent::Record { payload, .. } if *payload == record));
}

#[test]
fn a_huge_scripted_delay_drops_the_copy_without_panicking() {
    let faults = ScriptedFaults::new();
    let network = MemoryNetwork::builder()
        .faults(faults.clone())
        .manual_clock()
        .build();
    let t = started(&network, '1');
    let seen = Seen::<Inbound>::default();
    let _s = t.subscribe(&session(1), seen.handler()).unwrap();
    let d = deadline(&network, 60 * SEC);
    faults.delay(Duration::MAX);
    faults.push(vec![Duration::MAX, Duration::ZERO]);
    assert_eq!(
        t.publish(&session(1), envelope("never"), d),
        PublishResult::Taken
    );
    assert_eq!(network.in_flight(), 0);
    assert_eq!(
        t.publish(&session(1), envelope("once"), d),
        PublishResult::Taken
    );
    network.settle();
    assert_eq!(seen.octets(), vec![b"once".to_vec()]);
}

#[test]
fn a_huge_clock_advance_saturates_without_panicking() {
    let network = MemoryNetwork::builder().manual_clock().build();
    let clock = network.manual_clock().unwrap();
    let t = started(&network, '1');
    let seen = Seen::<Inbound>::default();
    let _s = t.subscribe(&session(1), seen.handler()).unwrap();
    let earlier = deadline(&network, 60 * SEC);
    clock.advance(Duration::MAX);
    clock.advance(Duration::MAX);
    let now = network.now();
    assert_eq!(clock.now(), now);
    assert!(earlier.has_passed_at(now));
    // The network still answers, and still refuses what cannot be delivered.
    assert_eq!(
        t.publish(&session(1), envelope("late"), earlier),
        PublishResult::NotTaken
    );
    assert_eq!(
        t.publish(&session(1), envelope("late"), Deadline::at(now)),
        PublishResult::NotTaken
    );
    network.settle();
    assert!(seen.get().is_empty());
    assert_eq!(t.health().state, HealthState::Healthy);
}

#[test]
fn a_copy_is_dropped_at_its_deadline_while_a_handler_runs() {
    // [IFC-TRN-034] "hold": the delivery thread is busy, the copy still goes at its
    // deadline, on the manual clock (synchronously in `advance`).
    let network = MemoryNetwork::builder().manual_clock().build();
    let clock = network.manual_clock().unwrap();
    let t = started(&network, '1');
    let b = blocking();
    let _busy = t.subscribe(&session(1), b.handler.clone()).unwrap();
    let seen = Seen::<Inbound>::default();
    let _s = t.subscribe(&session(2), seen.handler()).unwrap();
    t.publish(&session(1), envelope("busy"), deadline(&network, 60 * SEC));
    b.entered.recv_timeout(BOUND).unwrap();
    t.publish(&session(2), envelope("held"), deadline(&network, 5 * SEC));
    assert_eq!(
        network.in_flight(),
        1,
        "due, but the delivery thread is busy"
    );
    clock.advance(5 * SEC);
    assert_eq!(
        network.in_flight(),
        0,
        "dropped at its deadline, handler still running"
    );
    b.release.send(()).unwrap();
    network.settle();
    assert!(seen.get().is_empty());
}

#[test]
fn a_copy_is_dropped_at_its_deadline_while_a_handler_runs_real_clock() {
    // The same on the system clock: the purge thread drops it, not the delivery thread.
    let network = MemoryNetwork::new();
    let t = started(&network, '1');
    let b = blocking();
    let _busy = t.subscribe(&session(1), b.handler.clone()).unwrap();
    let seen = Seen::<Inbound>::default();
    let _s = t.subscribe(&session(2), seen.handler()).unwrap();
    t.publish(&session(1), envelope("busy"), deadline(&network, 60 * SEC));
    b.entered.recv_timeout(BOUND).unwrap();
    t.publish(
        &session(2),
        envelope("held"),
        deadline(&network, Duration::from_millis(50)),
    );
    // A bounded wait for the purge thread, with the handler still blocked throughout.
    let give_up = std::time::Instant::now() + BOUND;
    while network.in_flight() != 0 {
        assert!(
            std::time::Instant::now() < give_up,
            "copy held past its deadline"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(b.done.load(Ordering::SeqCst), 0, "handler still running");
    b.release.send(()).unwrap();
    network.settle();
    assert!(seen.get().is_empty());
}

#[test]
fn shutdown_from_inside_a_handler_returns() {
    let network = MemoryNetwork::new();
    let t = Arc::new(started(&network, '1'));
    let weak = Arc::downgrade(&t);
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let _s = t
        .subscribe(
            &session(1),
            Arc::new(move |_| {
                c.fetch_add(1, Ordering::SeqCst);
                if let Some(t) = weak.upgrade() {
                    t.shutdown();
                }
            }),
        )
        .unwrap();
    let d = deadline(&network, 60 * SEC);
    t.publish(&session(1), envelope("1"), d);
    network.settle();
    assert_eq!(
        t.publish(&session(1), envelope("2"), d),
        PublishResult::NotTaken
    );
    network.settle();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_panicking_handler_does_not_stop_delivery() {
    let network = MemoryNetwork::new();
    let t = started(&network, '1');
    let _p = t
        .subscribe(&session(1), Arc::new(|_| panic!("handler fails")))
        .unwrap();
    let seen = Seen::<Inbound>::default();
    let _s = t.subscribe(&session(2), seen.handler()).unwrap();
    let d = deadline(&network, 60 * SEC);
    t.publish(&session(1), envelope("boom"), d);
    t.publish(&session(2), envelope("fine"), d);
    network.settle();
    assert_eq!(seen.octets(), vec![b"fine".to_vec()]);
}

#[test]
fn reports_carrier_loss_for_a_link_that_ends() {
    // [IFC-TRN-061] (MAY, taken), [IFC-TRN-060]
    let network = cross();
    let a = started(&network, 'a');
    let b = started(&network, 'b');
    let c = started(&network, 'c');
    let seen_b = Seen::<PresenceEvent>::default();
    b.watch_presence(seen_b.handler()).unwrap();
    let seen_c = Seen::<PresenceEvent>::default();
    c.watch_presence(seen_c.handler()).unwrap();
    let d = deadline(&network, 60 * SEC);
    a.send_presence(
        &Destination::Device(key('b')),
        Payload::new(PayloadKind::Presence, b"r".to_vec()),
        d,
    );
    network.settle();
    a.shutdown();
    network.settle();

    let got = seen_b.get();
    assert_eq!(got.len(), 2);
    let PresenceEvent::Record { carrier: over, .. } = &got[0] else {
        panic!("expected a record first");
    };
    assert_eq!(
        got[1],
        PresenceEvent::CarrierLoss {
            carrier: over.clone()
        }
    );
    // c never received anything from a: no link, no loss.
    assert!(seen_c.get().is_empty());

    // A restarted a is a new link.
    a.start(&key('a'), MemoryConfiguration::wrap(&network))
        .unwrap();
    a.send_presence(
        &Destination::Device(key('b')),
        Payload::new(PayloadKind::Presence, b"r".to_vec()),
        d,
    );
    network.settle();
    let PresenceEvent::Record { carrier: again, .. } = &seen_b.get()[2] else {
        panic!("expected a record");
    };
    assert_ne!(again, over);
    let _: &CarrierHandle = again;
}

#[test]
fn the_network_stops_when_its_last_owner_goes() {
    // Dropping every transport and the network ends the delivery thread without a hang.
    let network = MemoryNetwork::new();
    let t = started(&network, '1');
    let _s = t
        .subscribe(&session(1), Seen::<Inbound>::default().handler())
        .unwrap();
    drop(network);
    t.publish(
        &session(1),
        envelope("x"),
        Deadline::at(std::time::Instant::now() + SEC),
    );
    drop(t);
}

#[test]
fn usable_as_a_trait_object() {
    // The F10 suite drives a `dyn Transport`.
    let network = MemoryNetwork::new();
    let t: Box<dyn Transport> = Box::new(MemoryTransport::new());
    t.start(&key('1'), MemoryConfiguration::wrap(&network))
        .unwrap();
    assert_eq!(t.health().state, HealthState::Healthy);
}
