// SPDX-License-Identifier: Apache-2.0

//! The send and receive pipelines (#313), through the public API only, over an in-test
//! transport ([`Bus`]) and in-test adapters ([`TestAdapter`]): unit tier, CI-default.
//!
//! The bus delivers inline, on the publisher's thread, which the pipelines must tolerate,
//! since they hold no lock across a transport call. It declares `cross-implementation` and
//! `destination_restricted`, so two devices on it exchange presence records
//! ([IFC-TRN-081]); a test of one device uses it alone. The end-to-end run over the
//! in-memory transport and the fake harnesses is `transports/memory/tests/pipelines.rs`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oac_core::adapter::{
    AdapterCapabilities, AdapterEvent, AdapterEventHandler, Attachment, Connection, Correlation,
    DiscoveryRequest, DiscoveryRequestResult, HandOff, HandOffOutcome, NativeSignal,
    ProviderAdapter, RequestSink, SendRequest, SendRequestResult, StartKind,
};
use oac_core::authorization::{
    AuthorizationEngine, Binding, Grant, LocalSide, MemoryDecisionLog, OperatorConfirmed, PeerSide,
    REPLY_PERIOD_MS,
};
use oac_core::clock::{Clock, ManualClock, SystemClock};
use oac_core::delivery::{DeliveryState, ErrorCode, Observer, Scope};
use oac_core::envelope::{ContentPart, TextPart};
use oac_core::health::{HealthState, HealthStatus};
use oac_core::ids::{KeyId, SessionId, Token};
use oac_core::keys::{DeviceIdentity, DeviceKey};
use oac_core::pairing::{MemoryPairingStore, PairedPeer};
use oac_core::pipeline::{AdapterId, PipelineConfig, PipelineError, Pipelines};
use oac_core::receipt::DeliveryReceipt;
use oac_core::session_binding::{
    BindingLog, BindingLogEntry, BindingResult, MemoryBindingLog, PairingKey, PeerObservation,
    RecordKind,
};
use oac_core::status::{SessionState, StatusCounts};
use oac_core::transport::{
    CarrierHandle, Deadline, Destination, Inbound, InboundHandler, Payload, PayloadKind,
    PresenceEvent, PresenceHandler, PublishResult, Reach, Subscription, Transport,
    TransportCapabilities, TransportConfiguration, TransportError,
};

const WAIT: Duration = Duration::from_secs(5);

// ---- an in-test transport ----------------------------------------------------------------

type PublishHook = Box<dyn FnOnce(&Payload) + Send>;

#[derive(Default)]
struct BusState {
    on_publish: Option<PublishHook>,
    next: u64,
    subs: Vec<(u64, KeyId, Destination, InboundHandler)>,
    watchers: Vec<(KeyId, PresenceHandler)>,
    /// Every payload taken, in order: sender, kind, destination.
    log: Vec<(KeyId, PayloadKind, Destination)>,
    subscribe_calls: usize,
    refuse_envelopes: bool,
    /// How many presence records to refuse next.
    refuse_presence: usize,
    /// The last envelope payload taken, for re-injecting a copy.
    last_envelope: Option<Payload>,
    /// Declare `destination_restricted` absent ([IFC-TRN-081] then keeps presence off).
    unrestricted: bool,
    /// Run once, on the sender's thread, when the next presence record is passed and
    /// before it is taken.
    on_presence: Option<Box<dyn FnOnce() + Send>>,
    /// Declared `publish_bound_ms` of every endpoint ([IFC-TRN-114]).
    publish_bound: Option<u64>,
    /// Take payloads without delivering them: the medium is paused after ownership, so a
    /// test holding a device's core lock is not met by an inline delivery.
    no_delivery: bool,
    /// Every presence record taken, with the device that sent it.
    presence_taken: Vec<(KeyId, Destination, Payload)>,
}

/// A loopback medium for several endpoints, one per device key.
#[derive(Clone, Default)]
struct Bus(Arc<Mutex<BusState>>);

impl Bus {
    fn endpoint(&self) -> Arc<Endpoint> {
        Arc::new(Endpoint {
            bus: self.clone(),
            key: Mutex::new(None),
        })
    }

    fn log(&self) -> Vec<(KeyId, PayloadKind, Destination)> {
        self.0.lock().unwrap().log.clone()
    }

    /// Reports a carrier appearance to the core of `owner`, as that device's transport does
    /// when a link becomes usable ([IFC-TRN-120], [IFC-TRN-121]). The event carries the
    /// handle of `link` and nothing else ([IFC-TRN-122]).
    fn appear(&self, owner: &KeyId, link: &str) {
        let handlers: Vec<PresenceHandler> = self
            .0
            .lock()
            .unwrap()
            .watchers
            .iter()
            .filter(|(o, _)| o == owner)
            .map(|(_, h)| h.clone())
            .collect();
        for h in handlers {
            h(PresenceEvent::CarrierAppeared {
                carrier: CarrierHandle::from_opaque(link.as_bytes().to_vec()),
            });
        }
    }

    /// The presence records `from` has passed to the transport, in order.
    fn presence_from(&self, from: &KeyId) -> Vec<Payload> {
        self.0
            .lock()
            .unwrap()
            .presence_taken
            .iter()
            .filter(|(f, ..)| f == from)
            .map(|(_, _, p)| p.clone())
            .collect()
    }

    /// The `(session, device)` of every presence record `from` has passed, in order.
    fn presence_pairs_from(&self, from: &KeyId) -> Vec<(SessionId, KeyId)> {
        self.0
            .lock()
            .unwrap()
            .presence_taken
            .iter()
            .filter(|(f, ..)| f == from)
            .map(|(_, d, p)| match d {
                Destination::Device(k) => (record_of(p).session_id().clone(), k.clone()),
                other => panic!("a presence record to {other:?}"),
            })
            .collect()
    }

    /// Hands `payload` again to every subscription for `destination`: a second copy.
    fn redeliver(&self, from: &KeyId, destination: &Destination, payload: Payload) {
        let handlers: Vec<InboundHandler> = self
            .0
            .lock()
            .unwrap()
            .subs
            .iter()
            .filter(|(_, _, d, _)| d == destination)
            .map(|(_, _, _, h)| h.clone())
            .collect();
        for h in handlers {
            h(Inbound {
                payload: payload.clone(),
                carrier: CarrierHandle::from_opaque(from.as_str().as_bytes().to_vec()),
            });
        }
    }
}

struct Endpoint {
    bus: Bus,
    key: Mutex<Option<KeyId>>,
}

impl Endpoint {
    fn me(&self) -> Option<KeyId> {
        self.key.lock().unwrap().clone()
    }
}

impl Transport for Endpoint {
    fn publish_bound_ms(&self) -> Option<std::num::NonZeroU64> {
        self.bus
            .0
            .lock()
            .unwrap()
            .publish_bound
            .and_then(std::num::NonZeroU64::new)
    }

    fn start(
        &self,
        local_device: &KeyId,
        _configuration: TransportConfiguration,
    ) -> Result<TransportCapabilities, TransportError> {
        *self.key.lock().unwrap() = Some(local_device.clone());
        let unrestricted = self.bus.0.lock().unwrap().unrestricted;
        Ok(TransportCapabilities {
            reliability: false,
            persistence: false,
            offline_queueing: false,
            ordering: true,
            multicast_discovery: false,
            routing_federation: false,
            reach: Reach::CrossImplementation,
            destination_restricted: !unrestricted,
            max_payload_octets: 65_536 * 4,
            // Payload sealing is tested over a sealing transport in tests/sealing.rs.
            sealing: false,
        })
    }

    fn publish(
        &self,
        destination: &Destination,
        payload: Payload,
        _deadline: Deadline,
    ) -> PublishResult {
        let Some(me) = self.me() else {
            return PublishResult::NotTaken;
        };
        if !payload.kind().fits(destination) || payload.kind() == PayloadKind::Presence {
            return PublishResult::NotTaken;
        }
        let hook = self.bus.0.lock().unwrap().on_publish.take();
        if let Some(hook) = hook {
            hook(&payload);
        }
        let handlers: Vec<InboundHandler> = {
            let mut st = self.bus.0.lock().unwrap();
            if payload.kind() == PayloadKind::Envelope {
                if st.refuse_envelopes {
                    return PublishResult::NotTaken;
                }
                st.last_envelope = Some(payload.clone());
            }
            st.log
                .push((me.clone(), payload.kind(), destination.clone()));
            let no_delivery = st.no_delivery;
            st.subs
                .iter()
                .filter(|(_, owner, d, _)| {
                    !no_delivery
                        && d == destination
                        && match destination {
                            Destination::Device(k) => owner == k,
                            Destination::Session(_) => true,
                        }
                })
                .map(|(_, _, _, h)| h.clone())
                .collect()
        };
        for h in handlers {
            h(Inbound {
                payload: payload.clone(),
                carrier: CarrierHandle::from_opaque(me.as_str().as_bytes().to_vec()),
            });
        }
        PublishResult::Taken
    }

    fn subscribe(
        &self,
        destination: &Destination,
        handler: InboundHandler,
    ) -> Result<Subscription, TransportError> {
        let me = self.me().ok_or(TransportError::NotStarted)?;
        if let Destination::Device(k) = destination
            && k != &me
        {
            return Err(TransportError::NotLocalDevice);
        }
        let mut st = self.bus.0.lock().unwrap();
        st.next += 1;
        st.subscribe_calls += 1;
        let id = st.next;
        st.subs.push((id, me, destination.clone(), handler));
        let bus = self.bus.clone();
        Ok(Subscription::new(move || {
            bus.0.lock().unwrap().subs.retain(|(i, ..)| *i != id);
        }))
    }

    fn send_presence(
        &self,
        destination: &Destination,
        payload: Payload,
        _deadline: Deadline,
    ) -> PublishResult {
        let (Some(me), Destination::Device(k)) = (self.me(), destination) else {
            return PublishResult::NotTaken;
        };
        if payload.kind() != PayloadKind::Presence {
            return PublishResult::NotTaken;
        }
        let hook = self.bus.0.lock().unwrap().on_presence.take();
        if let Some(hook) = hook {
            hook();
        }
        let watchers: Vec<PresenceHandler> = {
            let mut st = self.bus.0.lock().unwrap();
            if st.refuse_presence > 0 {
                st.refuse_presence -= 1;
                return PublishResult::NotTaken;
            }
            st.log
                .push((me.clone(), payload.kind(), destination.clone()));
            st.presence_taken
                .push((me.clone(), destination.clone(), payload.clone()));
            st.watchers
                .iter()
                .filter(|(owner, _)| owner == k)
                .map(|(_, h)| h.clone())
                .collect()
        };
        for w in watchers {
            w(PresenceEvent::Record {
                payload: payload.clone(),
                carrier: CarrierHandle::from_opaque(me.as_str().as_bytes().to_vec()),
            });
        }
        PublishResult::Taken
    }

    fn watch_presence(&self, handler: PresenceHandler) -> Result<(), TransportError> {
        let me = self.me().ok_or(TransportError::NotStarted)?;
        self.bus.0.lock().unwrap().watchers.push((me, handler));
        Ok(())
    }

    fn health(&self) -> HealthStatus {
        HealthStatus::new(HealthState::Healthy)
    }

    fn shutdown(&self) {
        if let Some(me) = self.key.lock().unwrap().take() {
            let mut st = self.bus.0.lock().unwrap();
            st.subs.retain(|(_, o, ..)| o != &me);
            st.watchers.retain(|(o, _)| o != &me);
        }
    }
}

// ---- an in-test adapter --------------------------------------------------------------------

type OnDeliver = Box<dyn Fn(&HandOff) -> HandOffOutcome + Send + Sync>;

/// A minimal adapter: every connection it takes is an attachment at once, and each
/// hand-off is recorded and answered with the outcome set (or by `on_deliver`).
struct TestAdapter {
    events: Mutex<Option<AdapterEventHandler>>,
    sink: Mutex<Option<Arc<dyn RequestSink>>>,
    bound: Mutex<HashMap<Attachment, Option<SessionId>>>,
    delivered: Mutex<Vec<HandOff>>,
    outcome: Mutex<HandOffOutcome>,
    on_deliver: Mutex<Option<OnDeliver>>,
    caps: Mutex<AdapterCapabilities>,
    deliver_calls: AtomicUsize,
    shutdown_bound: AtomicUsize,
    /// Run once at the next `capabilities` call.
    on_capabilities: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// Run once at the next `set_binding` that names a session: after the core registered
    /// it, while the decision that bound it still holds the binding turn.
    on_set_binding: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// Run once at the next `set_binding` that names no session: while the core is
    /// deregistering a binding (it tells the adapter first), still holding the binding turn.
    on_unset_binding: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// The next connection taken carries native signals only: it is not an attachment.
    carrier_next: Mutex<bool>,
    /// The cross-check value reported with the next `attachment-opened`.
    cross_check_next: Mutex<Option<String>>,
    /// What `health` answers.
    health: Mutex<HealthStatus>,
    /// Run at every `health` call, before it answers.
    on_health: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl TestAdapter {
    fn new() -> Arc<TestAdapter> {
        Arc::new(TestAdapter {
            events: Mutex::default(),
            sink: Mutex::default(),
            bound: Mutex::default(),
            delivered: Mutex::default(),
            outcome: Mutex::new(HandOffOutcome::Completed),
            on_deliver: Mutex::default(),
            caps: Mutex::new(AdapterCapabilities {
                active_inbound: true,
                content_types: Vec::new(),
                max_envelope_octets: None,
            }),
            deliver_calls: AtomicUsize::new(0),
            shutdown_bound: AtomicUsize::new(1000),
            on_capabilities: Mutex::default(),
            on_set_binding: Mutex::default(),
            on_unset_binding: Mutex::default(),
            carrier_next: Mutex::default(),
            cross_check_next: Mutex::default(),
            health: Mutex::new(HealthStatus::new(HealthState::Healthy)),
            on_health: Mutex::default(),
        })
    }

    /// Reports `e` to the core, as if the adapter observed it.
    fn emit(&self, e: AdapterEvent) {
        let h = self.events.lock().unwrap().clone();
        if let Some(h) = h {
            h(e);
        }
    }

    fn sink(&self) -> Arc<dyn RequestSink> {
        self.sink.lock().unwrap().clone().expect("accept_requests")
    }

    fn texts(&self) -> Vec<String> {
        self.delivered
            .lock()
            .unwrap()
            .iter()
            .flat_map(|h| {
                h.message()
                    .envelope()
                    .content()
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::Text(t) => Some(t.text().to_owned()),
                        ContentPart::Other(_) => None,
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

impl ProviderAdapter for TestAdapter {
    fn shutdown_bound_ms(&self) -> u64 {
        self.shutdown_bound.load(Ordering::SeqCst) as u64
    }

    fn take_connection(&self, connection: Connection) {
        let (a, _, _) = connection.into_parts();
        if std::mem::take(&mut *self.carrier_next.lock().unwrap()) {
            return;
        }
        self.bound.lock().unwrap().insert(a.clone(), None);
        let cross_check = self.cross_check_next.lock().unwrap().take();
        let h = self.events.lock().unwrap().clone();
        if let Some(h) = h {
            h(AdapterEvent::AttachmentOpened {
                attachment: a,
                cross_check,
            });
        }
    }

    fn watch_attachments(&self, handler: AdapterEventHandler) {
        *self.events.lock().unwrap() = Some(handler);
    }

    fn set_binding(&self, attachment: &Attachment, session: Option<SessionId>) {
        let names = session.is_some();
        self.bound
            .lock()
            .unwrap()
            .insert(attachment.clone(), session);
        let hook = if names {
            self.on_set_binding.lock().unwrap().take()
        } else {
            self.on_unset_binding.lock().unwrap().take()
        };
        if let Some(hook) = hook {
            hook();
        }
    }

    fn capabilities(&self, _attachment: &Attachment) -> AdapterCapabilities {
        let hook = self.on_capabilities.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        self.caps.lock().unwrap().clone()
    }

    fn deliver(&self, hand_off: HandOff) -> HandOffOutcome {
        self.deliver_calls.fetch_add(1, Ordering::SeqCst);
        // [IFC-ADP-030]: only to a bound attachment.
        let bound = self
            .bound
            .lock()
            .unwrap()
            .get(hand_off.attachment())
            .is_some_and(Option::is_some);
        assert!(bound, "a hand-off to an unbound attachment");
        let custom = self.on_deliver.lock().unwrap().take();
        let outcome = match &custom {
            Some(f) => f(&hand_off),
            None => *self.outcome.lock().unwrap(),
        };
        *self.on_deliver.lock().unwrap() = custom;
        self.delivered.lock().unwrap().push(hand_off);
        outcome
    }

    fn accept_requests(&self, sink: Arc<dyn RequestSink>) {
        *self.sink.lock().unwrap() = Some(sink);
    }

    fn health(&self) -> HealthStatus {
        let hook = self.on_health.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook();
        }
        self.health.lock().unwrap().clone()
    }

    fn shutdown(&self) {
        let open: Vec<Attachment> = self.bound.lock().unwrap().keys().cloned().collect();
        let h = self.events.lock().unwrap().clone();
        if let Some(h) = h {
            for a in open {
                h(AdapterEvent::AttachmentClosed { attachment: a });
            }
        }
    }
}

// ---- fixtures -------------------------------------------------------------------------------

struct Node {
    pipes: Pipelines,
    adapter: Arc<TestAdapter>,
    adapter_id: AdapterId,
    store: MemoryPairingStore,
}

fn node(bus: &Bus, principal: &str, config: PipelineConfig) -> Node {
    node_on(bus, principal, config, Arc::new(SystemClock))
}

fn node_on(bus: &Bus, principal: &str, config: PipelineConfig, clock: Arc<dyn Clock>) -> Node {
    node_mono(bus, principal, config, clock, Arc::new(Instant::now))
}

fn node_mono(
    bus: &Bus,
    principal: &str,
    config: PipelineConfig,
    clock: Arc<dyn Clock>,
    monotonic: oac_core::pipeline::MonotonicClock,
) -> Node {
    let identity = DeviceIdentity::new(DeviceKey::generate(), Token::parse(principal).unwrap());
    let engine =
        AuthorizationEngine::new(&identity, clock.clone(), Box::new(MemoryDecisionLog::new()));
    let pipes = Pipelines::new(identity, engine, clock, monotonic, bus.endpoint(), config);
    pipes.start(TransportConfiguration::new(())).expect("start");
    let adapter = TestAdapter::new();
    let adapter_id = pipes.add_adapter(adapter.clone());
    Node {
        pipes,
        adapter,
        adapter_id,
        store: MemoryPairingStore::new(),
    }
}

/// A session on `n`: a connection given to its adapter, bound to a fresh session id.
fn session(n: &Node, seed: u8) -> (Attachment, SessionId) {
    let conn = Connection::accept(Default::default(), Default::default());
    let a = conn.handle().clone();
    n.pipes.connect(n.adapter_id, conn).unwrap();
    let sid = SessionId::from_random_octets([seed; 16]);
    let record = n
        .pipes
        .device()
        .register(
            sid.clone(),
            Token::parse("test-harness").unwrap(),
            &format!("native-{seed}"),
            "/work",
            SystemClock.now(),
        )
        .unwrap();
    n.pipes.bind(&a, &record, None).unwrap();
    (a, sid)
}

fn grant(n: &Node, g: Grant) {
    n.pipes.with_engine(|e| {
        e.add_grant(g, OperatorConfirmed::by_operator(), &n.store)
            .unwrap();
    });
}

/// A grant on one device from `from` to `to`.
fn local_grant(n: &Node, from: &SessionId, to: &SessionId) {
    let key = n.pipes.device().key_id().clone();
    grant(
        n,
        Grant::Inbound {
            writer: PeerSide::session(key, from.clone()),
            target: LocalSide::Session(to.clone()),
        },
    );
}

fn text(s: &str) -> Vec<ContentPart> {
    vec![ContentPart::Text(TextPart::new(s).unwrap())]
}

fn request(from: &Attachment, to: &SessionId, s: &str) -> SendRequest {
    SendRequest {
        attachment: from.clone(),
        to: to.clone(),
        content: text(s),
        requested_target: None,
        conversation_id: None,
        correlation_id: None,
    }
}

#[test]
fn local_core_send_waits_for_ownership_after_commit_and_keeps_the_original_id() {
    use oac_core::local_call::{CallAdmission, LocalResult};
    for taken in [false, true] {
        let bus = Bus::default();
        let n = node(&bus, "local-call", PipelineConfig::default());
        let (from, from_id) = session(&n, 41);
        let (_, to) = session(&n, 42);
        local_grant(&n, &from_id, &to);
        let (entered, entered_rx) = std::sync::mpsc::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        {
            let mut st = bus.0.lock().unwrap();
            st.refuse_envelopes = !taken;
            st.on_publish = Some(Box::new(move |payload| {
                let env = oac_core::json::parse(payload.octets()).unwrap();
                let id = env
                    .as_object()
                    .unwrap()
                    .get("id")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned();
                entered.send(id).unwrap();
                release_rx.recv().unwrap();
            }));
        }
        let scope = CallAdmission::default();
        let scope2 = scope.clone();
        let sink = n.adapter.sink.lock().unwrap().clone().unwrap();
        let request = request(&from, &to, "ownership race");
        let (done, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut call = scope2.call(Some(request.attachment.clone()), 10);
            let result = sink.send_local(&mut call, request);
            done.send(result).unwrap();
        });
        let original_id = entered_rx.recv_timeout(WAIT).unwrap();
        scope.stop();
        from.begin_close();
        assert!(done_rx.recv_timeout(Duration::from_millis(40)).is_err());
        assert_eq!(
            n.pipes
                .shutdown_guarantee(n.adapter_id)
                .unwrap()
                .composed_ms(),
            None
        );
        release.send(()).unwrap();
        let LocalResult::Completed(result) = done_rx.recv_timeout(WAIT).unwrap() else {
            panic!("committed request lost its result");
        };
        assert_eq!(result.id().unwrap().as_str(), original_id);
        assert_eq!(result.outcome(), if taken { "sent" } else { "not-passed" });
        if !taken {
            assert_eq!(result.error(), Some(ErrorCode::TransportFailure));
        }
        worker.join().unwrap();
        assert!(done_rx.try_recv().is_err());
        assert_eq!(
            n.pipes.shutdown_guarantee(n.adapter_id).unwrap().publish_ms,
            None
        );
        assert!(
            n.pipes
                .shutdown_guarantee(n.adapter_id)
                .unwrap()
                .composed_ms()
                .is_some()
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ownership {
    Taken,
    NotTaken,
    AnnouncementRefused,
}

/// IFC-ADP-117, IFC-ADP-118: once the transport has settled the publication, the ownership
/// result is returned within the local budget however long the core lock stays held. The
/// transport is paused in `publish` (or `send_presence`) and declares a 500 ms bound, the core
/// lock is held through `with_engine`, closure may have begun, and only then is the
/// publication released. The result must arrive while the lock is still held.
fn check_post_publication_core_contention(case: Ownership, closing: bool) {
    use oac_core::local_call::{CallAdmission, LocalResult};
    let t = two_devices(false);
    let (entered, entered_rx) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    {
        let mut st = t.bus.0.lock().unwrap();
        st.publish_bound = Some(500);
        st.no_delivery = true;
        match case {
            Ownership::AnnouncementRefused => {
                st.refuse_presence = 1;
                st.on_presence = Some(Box::new(move || {
                    entered.send(()).unwrap();
                    release_rx.recv().unwrap();
                }));
            }
            Ownership::Taken | Ownership::NotTaken => {
                st.refuse_envelopes = case == Ownership::NotTaken;
                st.on_publish = Some(Box::new(move |_| {
                    entered.send(()).unwrap();
                    release_rx.recv().unwrap();
                }));
            }
        }
    }
    assert_eq!(
        t.x.pipes
            .shutdown_guarantee(t.x.adapter_id)
            .unwrap()
            .publish_ms,
        Some(500)
    );
    let scope = CallAdmission::default();
    let scope2 = scope.clone();
    let sink = t.x.adapter.sink();
    let request = request(&t.xa, &t.sy, "contended");
    let (done, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut call = scope2.call(Some(request.attachment.clone()), 1000);
        done.send(sink.send_local(&mut call, request)).unwrap();
    });
    entered_rx.recv_timeout(WAIT).unwrap();
    let pipes = t.x.pipes.clone();
    let (locked, locked_rx) = std::sync::mpsc::channel();
    let (unlock, unlock_rx) = std::sync::mpsc::channel();
    let blocker = std::thread::spawn(move || {
        pipes.with_engine(|_| {
            locked.send(()).unwrap();
            unlock_rx.recv().unwrap();
        })
    });
    locked_rx.recv_timeout(WAIT).unwrap();
    if closing {
        scope.stop();
        t.xa.begin_close();
    }
    release.send(()).unwrap();
    let released = Instant::now();
    let LocalResult::Completed(result) = done_rx
        .recv_timeout(Duration::from_millis(500))
        .unwrap_or_else(|_| panic!("{case:?}: no result while the core lock is held"))
    else {
        panic!("{case:?}: committed request lost its result");
    };
    assert!(released.elapsed() < Duration::from_millis(500));
    match case {
        Ownership::Taken => assert_eq!(result.outcome(), "sent"),
        Ownership::NotTaken | Ownership::AnnouncementRefused => {
            assert_eq!(result.outcome(), "not-passed");
            assert_eq!(result.error(), Some(ErrorCode::TransportFailure));
        }
    }
    let id = result.id().expect("the original id is returned").clone();
    worker.join().unwrap();
    assert!(done_rx.try_recv().is_err(), "exactly one result");
    unlock.send(()).unwrap();
    blocker.join().unwrap();
    // A snapshot taken once the lock is free reflects the deferred bookkeeping: a send not
    // passed is no longer tracked, a taken one is.
    let sender = SessionId::from_random_octets([1; 16]);
    let state = t.x.pipes.delivery_state(&id, &sender);
    match case {
        Ownership::Taken => assert!(state.is_some(), "{state:?}"),
        Ownership::NotTaken | Ownership::AnnouncementRefused => {
            assert_eq!(state, None, "{case:?}: a send not passed is not tracked");
        }
    }
    if case == Ownership::AnnouncementRefused && !closing {
        // The deferred update was applied before the core served the next send: the refused
        // announcement is issued again, ahead of the envelope.
        sent(t.x.adapter.sink().send(request_from(&t, "again")));
        assert_eq!(t.count_from_x(PayloadKind::Presence), 1);
    }
    // Shutdown also joins the core-owned bookkeeping worker.
    t.x.pipes.shutdown();
}

fn request_from(t: &TwoDevices, s: &str) -> SendRequest {
    request(&t.xa, &t.sy, s)
}

#[test]
fn a_committed_send_returns_taken_while_the_core_lock_is_held_after_publication() {
    check_post_publication_core_contention(Ownership::Taken, false);
}

#[test]
fn a_committed_send_returns_not_taken_while_the_core_lock_is_held_after_publication() {
    check_post_publication_core_contention(Ownership::NotTaken, false);
}

#[test]
fn a_refused_announcement_returns_while_the_core_lock_is_held_after_publication() {
    check_post_publication_core_contention(Ownership::AnnouncementRefused, false);
}

#[test]
fn shutdown_gets_a_taken_result_while_the_core_lock_is_held_after_publication() {
    check_post_publication_core_contention(Ownership::Taken, true);
}

#[test]
fn shutdown_gets_a_not_taken_result_while_the_core_lock_is_held_after_publication() {
    check_post_publication_core_contention(Ownership::NotTaken, true);
}

#[test]
fn shutdown_gets_a_refused_announcement_result_while_the_core_lock_is_held_after_publication() {
    check_post_publication_core_contention(Ownership::AnnouncementRefused, true);
}

#[test]
fn local_core_snapshots_commit_or_cancel_without_identity_material_on_failure() {
    use oac_core::adapter::{IdentityRequest, IdentityRequestResult};
    use oac_core::local_call::{CallAdmission, LocalResult};
    let n = node(&Bus::default(), "snapshot", PipelineConfig::default());
    let (attachment, session_id) = session(&n, 43);
    let sink = n.adapter.sink.lock().unwrap().clone().unwrap();
    let scope = CallAdmission::default();
    let mut identity = scope.call(Some(attachment.clone()), 1000);
    let result = sink.identity_local(
        &mut identity,
        IdentityRequest {
            attachment: attachment.clone(),
        },
    );
    let LocalResult::Completed(IdentityRequestResult::LocalIdentity(value)) = result else {
        panic!("identity snapshot");
    };
    assert_eq!(value.session_id, session_id);
    assert_eq!(&value.device_fingerprint, n.pipes.device().key_id());
    scope.stop();
    assert_eq!(identity.cancellation(), None);
    let mut stopped = scope.call(Some(attachment.clone()), 1000);
    assert_eq!(
        sink.identity_local(
            &mut stopped,
            IdentityRequest {
                attachment: attachment.clone()
            }
        ),
        LocalResult::ShuttingDown
    );
    let mut stopped = scope.call(Some(attachment.clone()), 1000);
    assert_eq!(
        sink.discover_local(
            &mut stopped,
            DiscoveryRequest {
                attachment: attachment.clone()
            }
        ),
        LocalResult::ShuttingDown
    );
    let mut stopped = scope.call(Some(attachment.clone()), 1000);
    assert!(matches!(
        sink.send_local(&mut stopped, request(&attachment, &session_id, "cancel")),
        LocalResult::ShuttingDown
    ));
}

#[test]
fn final_local_closure_returns_before_blocked_withdrawal_and_never_calls_the_stopped_adapter() {
    check_final_local_closure(false);
}

#[test]
fn expired_final_local_closure_reports_timeout_and_still_removes_the_binding() {
    check_final_local_closure(true);
}

fn check_final_local_closure(expire_final: bool) {
    use oac_core::adapter::LocalEventHandler;
    use oac_core::local_call::{CallAdmission, LocalResult};
    struct LocalAdapter {
        inner: Arc<TestAdapter>,
        scope: Mutex<CallAdmission>,
        closed: AtomicUsize,
        expire_final: bool,
    }
    impl ProviderAdapter for LocalAdapter {
        fn local_calls(&self) -> bool {
            true
        }
        fn connection_limit(&self) -> usize {
            4
        }
        fn configure_local_calls(&self, scope: CallAdmission, _: u64, _: u64) {
            *self.scope.lock().unwrap() = scope;
        }
        fn watch_local_attachments(&self, handler: LocalEventHandler) {
            let scope = self.scope.lock().unwrap().clone();
            let expire_final = self.expire_final;
            self.inner.watch_attachments(Arc::new(move |event| {
                let (attachment, final_report) = match &event {
                    AdapterEvent::AttachmentOpened { attachment, .. } => {
                        (attachment.clone(), false)
                    }
                    AdapterEvent::AttachmentClosed { attachment } => (attachment.clone(), true),
                    _ => panic!("unexpected test event"),
                };
                let expire = final_report && expire_final;
                let mut call = scope.call(Some(attachment), if expire { 1 } else { 100 });
                if final_report {
                    call = call.final_report();
                }
                if expire {
                    std::thread::sleep(Duration::from_millis(2));
                }
                assert_eq!(
                    handler(&mut call, event),
                    if expire {
                        LocalResult::TimedOut
                    } else {
                        LocalResult::Completed(())
                    }
                );
            }));
        }
        fn take_connection(&self, c: Connection) {
            self.inner.take_connection(c);
        }
        fn watch_attachments(&self, h: AdapterEventHandler) {
            self.inner.watch_attachments(h);
        }
        fn set_binding(&self, a: &Attachment, s: Option<SessionId>) {
            assert_eq!(
                self.closed.load(Ordering::SeqCst),
                0,
                "callback after shutdown"
            );
            self.inner.set_binding(a, s);
        }
        fn capabilities(&self, a: &Attachment) -> AdapterCapabilities {
            assert_eq!(
                self.closed.load(Ordering::SeqCst),
                0,
                "callback after shutdown"
            );
            self.inner.capabilities(a)
        }
        fn deliver(&self, h: HandOff) -> HandOffOutcome {
            assert_eq!(
                self.closed.load(Ordering::SeqCst),
                0,
                "callback after shutdown"
            );
            self.inner.deliver(h)
        }
        fn accept_requests(&self, sink: Arc<dyn RequestSink>) {
            self.inner.accept_requests(sink);
        }
        fn health(&self) -> HealthStatus {
            self.inner.health()
        }
        fn shutdown(&self) {
            self.scope.lock().unwrap().stop();
            for h in self.inner.bound.lock().unwrap().keys() {
                h.close();
            }
            self.inner.shutdown();
            self.closed.fetch_add(1, Ordering::SeqCst);
        }
    }
    let bus = Bus::default();
    let mut n = node(&bus, "final-report", PipelineConfig::default());
    let local = Arc::new(LocalAdapter {
        inner: TestAdapter::new(),
        scope: Mutex::default(),
        closed: AtomicUsize::new(0),
        expire_final,
    });
    n.adapter = local.inner.clone();
    n.adapter_id = n.pipes.add_adapter(local.clone());
    let peer = node(&bus, "peer", PipelineConfig::default());
    pair(&n, &peer);
    pair(&peer, &n);
    let sid = SessionId::from_random_octets([44; 16]);
    let target = SessionId::from_random_octets([45; 16]);
    grant(
        &n,
        Grant::Outbound {
            writer: LocalSide::Session(sid.clone()),
            target: PeerSide::session(peer.pipes.device().key_id().clone(), target.clone()),
        },
    );
    grant(
        &peer,
        Grant::Inbound {
            writer: PeerSide::session(n.pipes.device().key_id().clone(), sid.clone()),
            target: LocalSide::Session(target),
        },
    );
    session(&peer, 45);
    let (attachment, _) = session(&n, 44);
    let (entered, entered_rx) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let (finished, finished_rx) = std::sync::mpsc::channel();
    bus.0.lock().unwrap().on_presence = Some(Box::new(move || {
        entered.send(()).unwrap();
        release_rx.recv().unwrap();
        finished.send(()).unwrap();
    }));
    let pipes = n.pipes.clone();
    let (locked, locked_rx) = std::sync::mpsc::channel();
    let (unlock, unlock_rx) = std::sync::mpsc::channel();
    let blocker = std::thread::spawn(move || {
        pipes.with_engine(|_| {
            locked.send(()).unwrap();
            unlock_rx.recv().unwrap();
        })
    });
    locked_rx.recv_timeout(WAIT).unwrap();
    let start = Instant::now();
    local.shutdown();
    assert!(start.elapsed() < Duration::from_millis(1000));
    assert_eq!(
        n.pipes.binding(&attachment),
        None,
        "closure recorded at return despite the held core lock"
    );
    unlock.send(()).unwrap();
    blocker.join().unwrap();
    assert_eq!(n.pipes.binding(&attachment), None);
    assert!(attachment.is_closed());
    entered_rx.recv_timeout(WAIT).unwrap();
    let mut call = CallAdmission::default().call(Some(attachment.clone()), 100);
    assert!(matches!(
        n.adapter
            .sink()
            .identity_local(&mut call, oac_core::adapter::IdentityRequest { attachment }),
        LocalResult::Closed
    ));
    release.send(()).unwrap();
    finished_rx.recv_timeout(WAIT).unwrap();
    assert_eq!(local.closed.load(Ordering::SeqCst), 1);
}

#[test]
fn all_local_request_methods_time_out_on_a_blocked_core_lock_without_late_work() {
    use oac_core::adapter::IdentityRequest;
    use oac_core::local_call::{CallAdmission, LocalResult};
    let bus = Bus::default();
    let n = node(&bus, "blocked-core", PipelineConfig::default());
    let (attachment, sid) = session(&n, 46);
    let sink = n.adapter.sink();
    let before = bus.log();
    let pipes = n.pipes.clone();
    let (entered, entered_rx) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let blocker = std::thread::spawn(move || {
        pipes.with_engine(|_| {
            entered.send(()).unwrap();
            release_rx.recv().unwrap();
        })
    });
    entered_rx.recv_timeout(WAIT).unwrap();
    let scope = CallAdmission::default();
    let mut call = scope.call(Some(attachment.clone()), 10);
    assert!(matches!(
        sink.send_local(&mut call, request(&attachment, &sid, "never created")),
        LocalResult::TimedOut
    ));
    let mut call = scope.call(Some(attachment.clone()), 10);
    assert_eq!(
        sink.discover_local(
            &mut call,
            DiscoveryRequest {
                attachment: attachment.clone()
            }
        ),
        LocalResult::TimedOut
    );
    let mut call = scope.call(Some(attachment.clone()), 10);
    assert_eq!(
        sink.identity_local(
            &mut call,
            IdentityRequest {
                attachment: attachment.clone()
            }
        ),
        LocalResult::TimedOut
    );
    release.send(()).unwrap();
    blocker.join().unwrap();
    assert_eq!(
        bus.log(),
        before,
        "no cancelled invocation can later publish"
    );
    n.pipes.shutdown();
    let mut supplied_live_scope = CallAdmission::default().call(None, 1000);
    assert_eq!(
        sink.identity_local(&mut supplied_live_scope, IdentityRequest { attachment }),
        LocalResult::ShuttingDown,
        "core owns admission even if a caller supplies a fresh scope"
    );
}

/// Table 8.1 and Table 8.3 for a request result: `refused` has a `request`-scoped code and
/// no state; `not-passed` is `failed` with a sender code; `sent` is `accepted-by-adapter`.
fn check_result_tables(r: &SendRequestResult) {
    match r {
        SendRequestResult::Refused { error, .. } => {
            assert!(error.scope().contains(&Scope::Request), "{error}");
            assert_eq!(r.state(), None);
        }
        SendRequestResult::NotPassed { error, .. } => {
            assert_eq!(r.state(), Some(DeliveryState::Failed));
            assert!(
                error.fits_receipt(DeliveryState::Failed, Observer::Sender),
                "{error}"
            );
        }
        SendRequestResult::Sent { .. } => {
            assert_eq!(r.state(), Some(DeliveryState::AcceptedByAdapter));
            assert!(DeliveryState::AcceptedByAdapter.allowed_for(Observer::Sender));
        }
    }
}

/// Table 8.1 and Table 8.3 for a receipt: a receiver state, with a code exactly when the
/// state carries one, and a code in the receiver scope for that state.
fn check_receipt_tables(r: &DeliveryReceipt) {
    assert_eq!(r.observer(), Observer::Receiver);
    assert!(r.state().allowed_for(Observer::Receiver), "{}", r.state());
    match r.error() {
        None => assert!(!r.state().carries_error(), "{}", r.state()),
        Some(e) => {
            let code = ErrorCode::parse(e.as_str()).expect("a Table 8.3 code");
            assert!(code.fits_receipt(r.state(), Observer::Receiver), "{code}");
        }
    }
}

fn sent(r: SendRequestResult) -> (Token, Option<Correlation>, oac_core::adapter::ReceiptStream) {
    check_result_tables(&r);
    match r {
        SendRequestResult::Sent {
            id,
            correlation,
            receipts,
        } => (id, correlation, receipts),
        other => panic!("not sent: {other:?}"),
    }
}

fn refused(r: SendRequestResult) -> ErrorCode {
    check_result_tables(&r);
    match r {
        SendRequestResult::Refused { error, .. } => error,
        other => panic!("not refused: {other:?}"),
    }
}

// ---- one device -----------------------------------------------------------------------------

/// The acceptance path on one device: a request from one attachment becomes an envelope,
/// passes through the transport to the other session's subscription, is handed off through
/// the adapter, and its receiver-observed state reaches the sender's receipt stream.
#[test]
fn a_request_is_sent_handed_off_and_its_receipt_returns() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (a, sa) = session(&n, 1);
    let (_b, sb) = session(&n, 2);
    local_grant(&n, &sa, &sb);
    let (id, correlation, receipts) = sent(n.adapter.sink().send(request(&a, &sb, "hello")));
    assert_eq!(correlation, None);
    assert_eq!(n.adapter.texts(), ["hello"]);
    let h = &n.adapter.delivered.lock().unwrap()[0];
    let env = h.message().envelope();
    assert_eq!((env.id(), env.from(), env.to()), (&id, &sa, &sb));
    assert_eq!(
        h.message().verified_by().unwrap().key_id(),
        n.pipes.device().key_id()
    );
    let r = receipts.0.recv_timeout(WAIT).unwrap();
    check_receipt_tables(&r);
    assert_eq!(r.state(), DeliveryState::HandedToHarness);
    assert_eq!(
        n.pipes.delivery_state(&id, &sa),
        Some(DeliveryState::HandedToHarness)
    );
    // One device: nothing but the envelope crossed the transport; no presence record and no
    // receipt on the wire ([SC-RCP-040]).
    let kinds: Vec<PayloadKind> = bus.log().iter().map(|(_, k, _)| *k).collect();
    assert_eq!(kinds, [PayloadKind::Envelope]);
}

/// Every hand-off outcome becomes the Table 5.3 state and code, on a receipt that fits
/// Tables 8.1 and 8.3, and the sender's combined state follows ([SC-RCP-085]).
#[test]
fn every_hand_off_outcome_maps_to_table_8_1_and_8_3() {
    for outcome in HandOffOutcome::ALL {
        let bus = Bus::default();
        let n = node(&bus, "device-a", PipelineConfig::default());
        let (a, sa) = session(&n, 1);
        let (_b, sb) = session(&n, 2);
        local_grant(&n, &sa, &sb);
        *n.adapter.outcome.lock().unwrap() = outcome;
        let (id, _, receipts) = sent(n.adapter.sink().send(request(&a, &sb, "x")));
        let r = receipts.0.recv_timeout(WAIT).unwrap();
        check_receipt_tables(&r);
        let (state, code) = outcome.recorded();
        assert_eq!(r.state(), state, "{outcome:?}");
        assert_eq!(
            r.error().map(|e| e.as_str().to_owned()),
            code.map(|c| c.as_str().to_owned()),
            "{outcome:?}"
        );
        assert_eq!(n.pipes.delivery_state(&id, &sa), Some(state), "{outcome:?}");
        assert_eq!(n.adapter.deliver_calls.load(Ordering::SeqCst), 1);
    }
}

/// The send path's refusals, each with its request-scoped code, and no envelope on the
/// transport for any of them ([SC-RCP-075]).
#[test]
fn refusals_create_no_envelope() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (a, sa) = session(&n, 1);
    let (_b, sb) = session(&n, 2);
    // No grant: the requester may not discover the target, which is `unknown` to it
    // ([SC-DLV-075]).
    assert_eq!(
        refused(n.adapter.sink().send(request(&a, &sb, "x"))),
        ErrorCode::UnknownDestination
    );
    local_grant(&n, &sa, &sb);
    // An attachment that is not bound, and one never given to this adapter: unauthorized
    // ([SC-ID-161], [IFC-ADP-031]).
    let stray = Connection::accept(Default::default(), Default::default())
        .handle()
        .clone();
    assert_eq!(
        refused(n.adapter.sink().send(request(&stray, &sb, "x"))),
        ErrorCode::Unauthorized
    );
    let conn = Connection::accept(Default::default(), Default::default());
    let unbound = conn.handle().clone();
    n.pipes.connect(n.adapter_id, conn).unwrap();
    assert_eq!(
        refused(n.adapter.sink().send(request(&unbound, &sb, "x"))),
        ErrorCode::Unauthorized
    );
    // A request with a malformed conversation id.
    let mut bad = request(&a, &sb, "x");
    bad.conversation_id = Some("not a token".into());
    assert_eq!(
        refused(n.adapter.sink().send(bad)),
        ErrorCode::InvalidRequest
    );
    // A part type the session does not take ([SC-ID-101]).
    let mut other = request(&a, &sb, "x");
    other.content = vec![
        ContentPart::from_json(
            &oac_core::json::parse(br#"{"type":"com.example/note","text":"n"}"#).unwrap(),
        )
        .unwrap(),
    ];
    assert_eq!(
        refused(n.adapter.sink().send(other)),
        ErrorCode::UnsupportedContentType
    );
    assert!(bus.log().is_empty(), "{:?}", bus.log());
    assert_eq!(n.adapter.deliver_calls.load(Ordering::SeqCst), 0);
}

/// A receiver session that does not take active delivery is refused at the sender
/// ([SC-ID-100]); one that advertises a part type takes it ([SC-ID-101]).
#[test]
fn the_declaration_comes_from_the_adapter() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (a, sa) = session(&n, 1);
    *n.adapter.caps.lock().unwrap() = AdapterCapabilities {
        active_inbound: false,
        content_types: Vec::new(),
        max_envelope_octets: None,
    };
    let (_b, sb) = session(&n, 2);
    *n.adapter.caps.lock().unwrap() = AdapterCapabilities {
        active_inbound: true,
        content_types: vec!["com.example/note".into()],
        max_envelope_octets: Some(131_072),
    };
    let (_c, sc) = session(&n, 3);
    local_grant(&n, &sa, &sb);
    local_grant(&n, &sa, &sc);
    assert_eq!(
        refused(n.adapter.sink().send(request(&a, &sb, "x"))),
        ErrorCode::UnsupportedCapability
    );
    let mut note = request(&a, &sc, "x");
    note.content = vec![
        ContentPart::from_json(
            &oac_core::json::parse(br#"{"type":"com.example/note","text":"n"}"#).unwrap(),
        )
        .unwrap(),
    ];
    let (_, _, receipts) = sent(n.adapter.sink().send(note));
    assert_eq!(
        receipts.0.recv_timeout(WAIT).unwrap().state(),
        DeliveryState::HandedToHarness
    );
}

/// A transport that does not take the envelope: `not-passed`, `failed` with
/// `transport-failure`, and no hand-off.
#[test]
fn a_refused_publish_is_not_passed() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (a, sa) = session(&n, 1);
    let (_b, sb) = session(&n, 2);
    local_grant(&n, &sa, &sb);
    bus.0.lock().unwrap().refuse_envelopes = true;
    let r = n.adapter.sink().send(request(&a, &sb, "x"));
    check_result_tables(&r);
    match r {
        SendRequestResult::NotPassed { error, .. } => {
            assert_eq!(error, ErrorCode::TransportFailure)
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(n.adapter.deliver_calls.load(Ordering::SeqCst), 0);
}

/// A reply names the message it answers; `reply_to`, `conversation_id` and
/// `correlation_id` come from the hand-off record, never from the request ([SC-RCP-050] to
/// [SC-RCP-055]). A target that was never handed off to the replier goes uncorrelated.
#[test]
fn a_reply_copies_its_headers_from_the_hand_off_record() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (a, sa) = session(&n, 1);
    let (b, sb) = session(&n, 2);
    local_grant(&n, &sa, &sb);
    local_grant(&n, &sb, &sa);
    let mut first = request(&a, &sb, "question");
    first.conversation_id = Some("conv-1".into());
    first.correlation_id = Some("corr-1".into());
    let (id, _, _) = sent(n.adapter.sink().send(first));
    let mut reply = request(&b, &sa, "answer");
    reply.requested_target = Some(id.as_str().to_owned());
    reply.conversation_id = Some("forged".into());
    let (_, correlation, _) = sent(n.adapter.sink().send(reply));
    assert_eq!(correlation, Some(Correlation::Correlated));
    let delivered = n.adapter.delivered.lock().unwrap();
    let env = delivered[1].message().envelope();
    assert_eq!(env.reply_to(), Some(&id));
    assert_eq!(env.conversation_id().map(Token::as_str), Some("conv-1"));
    assert_eq!(env.correlation_id().map(Token::as_str), Some("corr-1"));
    drop(delivered);
    let mut stray = request(&b, &sa, "answer");
    stray.requested_target = Some("msg-never-sent".into());
    let (_, correlation, _) = sent(n.adapter.sink().send(stray));
    assert_eq!(correlation, Some(Correlation::Uncorrelated));
    let delivered = n.adapter.delivered.lock().unwrap();
    let env = delivered[2].message().envelope();
    assert_eq!(
        (env.reply_to(), env.conversation_id(), env.correlation_id()),
        (None, None, None)
    );
}

/// A copy that arrives while its twin is being handed off is re-queued, not parked, and
/// offered again once the twin settles ([SEC-RPL-026]): the twin's call failed, so the
/// re-queued copy is handed off. One subscription per session, however many copies arrive:
/// nothing is asked for ([IFC-TRN-041]).
#[test]
fn an_in_flight_copy_is_requeued_until_its_twin_settles() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (a, sa) = session(&n, 1);
    let (_b, sb) = session(&n, 2);
    local_grant(&n, &sa, &sb);
    let (bus2, key, to) = (
        bus.clone(),
        n.pipes.device().key_id().clone(),
        Destination::Session(sb.clone()),
    );
    let first = Arc::new(AtomicUsize::new(0));
    let f = first.clone();
    *n.adapter.on_deliver.lock().unwrap() = Some(Box::new(move |_h: &HandOff| {
        if f.fetch_add(1, Ordering::SeqCst) == 0 {
            // A second copy of the same envelope arrives during this hand-off call.
            let p = bus2.0.lock().unwrap().last_envelope.clone().unwrap();
            bus2.redeliver(&key, &to, p);
            HandOffOutcome::Failed
        } else {
            HandOffOutcome::Completed
        }
    }));
    let (id, _, receipts) = sent(n.adapter.sink().send(request(&a, &sb, "twice")));
    assert_eq!(n.adapter.deliver_calls.load(Ordering::SeqCst), 2);
    let states: Vec<DeliveryState> = (0..2)
        .map(|_| receipts.0.recv_timeout(WAIT).unwrap())
        .inspect(check_receipt_tables)
        .map(|r| r.state())
        .collect();
    assert_eq!(
        states,
        [DeliveryState::Failed, DeliveryState::HandedToHarness]
    );
    assert_eq!(
        n.pipes.delivery_state(&id, &sa),
        Some(DeliveryState::HandedToHarness)
    );
    // A third copy is a duplicate, and draws the one duplicate receipt.
    let p = bus.0.lock().unwrap().last_envelope.clone().unwrap();
    bus.redeliver(
        n.pipes.device().key_id(),
        &Destination::Session(sb.clone()),
        p,
    );
    let r = receipts.0.recv_timeout(WAIT).unwrap();
    check_receipt_tables(&r);
    assert_eq!(r.state(), DeliveryState::Duplicate);
    assert_eq!(n.adapter.deliver_calls.load(Ordering::SeqCst), 2);
    // Subscriptions: the device's receipts and one per bound session, made once.
    assert_eq!(bus.0.lock().unwrap().subscribe_calls, 3);
}

/// Bounded memory: past `max_trackers`, the oldest envelope is forgotten and its receipt
/// stream ends; a receipt stream holds at most its capacity.
#[test]
fn trackers_and_receipt_streams_are_bounded() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_trackers: 2,
        receipt_stream_capacity: 1,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let (a, sa) = session(&n, 1);
    let (_b, sb) = session(&n, 2);
    local_grant(&n, &sa, &sb);
    let (first, _, rx1) = sent(n.adapter.sink().send(request(&a, &sb, "1")));
    let (_, _, _rx2) = sent(n.adapter.sink().send(request(&a, &sb, "2")));
    let (_, _, _rx3) = sent(n.adapter.sink().send(request(&a, &sb, "3")));
    assert_eq!(n.pipes.delivery_state(&first, &sa), None);
    assert_eq!(
        rx1.0.recv_timeout(WAIT).unwrap().state(),
        DeliveryState::HandedToHarness
    );
    assert_eq!(
        rx1.0.recv_timeout(WAIT),
        Err(RecvTimeoutError::Disconnected)
    );
}

/// Unbinding ends the session's subscription ([IFC-TRN-042]) and its binding at the
/// adapter ([IFC-ADP-030]); a request from it is then unattributed.
#[test]
fn unbinding_ends_the_subscription() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (a, sa) = session(&n, 1);
    let (b, sb) = session(&n, 2);
    local_grant(&n, &sa, &sb);
    n.pipes.unbind(&b);
    assert_eq!(n.pipes.binding(&b), None);
    assert_eq!(n.adapter.bound.lock().unwrap().get(&b), Some(&None));
    let session_subs = bus
        .0
        .lock()
        .unwrap()
        .subs
        .iter()
        .filter(|(_, _, d, _)| matches!(d, Destination::Session(_)))
        .count();
    assert_eq!(session_subs, 1);
    // `sb` is no longer an own session, so `sa` may no longer discover it ([SC-DLV-075]).
    assert_eq!(
        refused(n.adapter.sink().send(request(&a, &sb, "x"))),
        ErrorCode::UnknownDestination
    );
    assert_eq!(
        refused(n.adapter.sink().send(request(&b, &sa, "x"))),
        ErrorCode::Unauthorized
    );
    n.pipes.shutdown();
    assert_eq!(n.pipes.binding(&a), None);
}

/// Discovery through the request sink: the requester sees the sessions it may write to.
#[test]
fn discovery_goes_through_the_core() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (a, sa) = session(&n, 1);
    let (_b, sb) = session(&n, 2);
    let listed = |n: &Node| match n.adapter.sink().discover(DiscoveryRequest {
        attachment: a.clone(),
    }) {
        DiscoveryRequestResult::DiscoveryResult(d) => {
            d.iter().map(|d| d.session_id().clone()).collect::<Vec<_>>()
        }
        other => panic!("{other:?}"),
    };
    assert!(listed(&n).is_empty());
    local_grant(&n, &sa, &sb);
    assert_eq!(listed(&n), [sb]);
}

// ---- two devices ----------------------------------------------------------------------------

fn pair(x: &Node, y: &Node) {
    let peer = PairedPeer::by_key_id_comparison(
        y.pipes.device().principal().clone(),
        *y.pipes.device().public_key(),
        y.pipes.device().key_id(),
        SystemClock.now(),
        OperatorConfirmed::by_operator(),
    )
    .unwrap();
    x.pipes.with_engine(|e| e.pair(peer, &x.store).unwrap());
}

/// Across devices: the receiver's announcement reaches the sender when it binds; the
/// sender's announcement goes out before its first envelope ([SEC-PRS-010]); the receiver
/// hands the envelope off and returns an authenticated receipt the sender accepts
/// ([SEC-RCT-001], [SEC-RCT-003]); and the receiver replies under the reply right
/// ([SEC-AUZ-014]) with one grant only (`spec/security.md` §9.5, "One grant suffices").
#[test]
fn two_devices_exchange_a_message_and_a_correlated_reply() {
    let bus = Bus::default();
    let x = node(&bus, "device-x", PipelineConfig::default());
    let y = node(&bus, "device-y", PipelineConfig::default());
    pair(&x, &y);
    pair(&y, &x);
    let (xk, yk) = (
        x.pipes.device().key_id().clone(),
        y.pipes.device().key_id().clone(),
    );
    let (xa, sx) = session(&x, 1);
    // One grant, "sx may write to sy", recorded on both devices.
    let sy = SessionId::from_random_octets([2; 16]);
    grant(
        &x,
        Grant::Outbound {
            writer: LocalSide::Session(sx.clone()),
            target: PeerSide::session(yk.clone(), sy.clone()),
        },
    );
    grant(
        &y,
        Grant::Inbound {
            writer: PeerSide::session(xk.clone(), sx.clone()),
            target: LocalSide::Session(sy.clone()),
        },
    );
    let (ya, sy2) = session(&y, 2);
    assert_eq!(sy2, sy);
    // y released sy's announcement to x when it bound it ([SC-DLV-051]).
    assert!(bus.log().iter().any(|(from, k, d)| from == &yk
        && *k == PayloadKind::Presence
        && d == &Destination::Device(xk.clone())));
    let (id, _, receipts) = sent(x.adapter.sink().send(request(&xa, &sy, "ping")));
    let log = bus.log();
    let presence = log
        .iter()
        .position(|(f, k, _)| f == &xk && *k == PayloadKind::Presence)
        .expect("x's announcement");
    let envelope = log
        .iter()
        .position(|(f, k, _)| f == &xk && *k == PayloadKind::Envelope)
        .expect("x's envelope");
    assert!(presence < envelope, "{log:?}");
    assert_eq!(y.adapter.texts(), ["ping"]);
    let r = receipts.0.recv_timeout(WAIT).unwrap();
    check_receipt_tables(&r);
    assert_eq!(r.state(), DeliveryState::HandedToHarness);
    assert!(log.iter().any(|(f, k, d)| f == &yk
        && *k == PayloadKind::Receipt
        && d == &Destination::Device(xk.clone())));
    // y replies to x, correlated, under x's reply right.
    let mut reply = request(&ya, &sx, "pong");
    reply.requested_target = Some(id.as_str().to_owned());
    let (_, correlation, back) = sent(y.adapter.sink().send(reply));
    assert_eq!(correlation, Some(Correlation::Correlated));
    assert_eq!(x.adapter.texts(), ["pong"]);
    assert_eq!(
        back.0.recv_timeout(WAIT).unwrap().state(),
        DeliveryState::HandedToHarness
    );
    // [IFC-TRN-102]: a transport that does not declare `sealing` is never passed a sealed
    // payload, and every payload crosses as before (#369).
    assert!(
        bus.log().iter().all(|(_, k, _)| *k != PayloadKind::Sealed),
        "{:?}",
        bus.log()
    );
    // An uncorrelated message from y to x needs a grant y does not have: x refuses it
    // ([SEC-AUZ-014]), and y's tracker reads the receiver's `rejected`.
    let (_, _, refused_stream) = sent(y.adapter.sink().send(request(&ya, &sx, "unsolicited")));
    let r = refused_stream.0.recv_timeout(WAIT).unwrap();
    check_receipt_tables(&r);
    assert_eq!(
        (r.state(), r.error().map(|e| e.as_str().to_owned())),
        (DeliveryState::Rejected, Some("unauthorized".to_owned()))
    );
    assert_eq!(x.adapter.texts(), ["pong"]);
}

/// Two devices with one grant from `sx` on x to `sy` on y, and `sy` bound on y.
struct TwoDevices {
    bus: Bus,
    x: Node,
    y: Node,
    xk: KeyId,
    xa: Attachment,
    sy: SessionId,
}

fn two_devices(unrestricted: bool) -> TwoDevices {
    let bus = Bus::default();
    bus.0.lock().unwrap().unrestricted = unrestricted;
    let x = node(&bus, "device-x", PipelineConfig::default());
    let y = node(&bus, "device-y", PipelineConfig::default());
    pair(&x, &y);
    pair(&y, &x);
    let (xk, yk) = (
        x.pipes.device().key_id().clone(),
        y.pipes.device().key_id().clone(),
    );
    let (xa, sx) = session(&x, 1);
    let sy = SessionId::from_random_octets([2; 16]);
    grant(
        &x,
        Grant::Outbound {
            writer: LocalSide::Session(sx.clone()),
            target: PeerSide::session(yk, sy.clone()),
        },
    );
    grant(
        &y,
        Grant::Inbound {
            writer: PeerSide::session(xk.clone(), sx),
            target: LocalSide::Session(sy.clone()),
        },
    );
    session(&y, 2);
    TwoDevices {
        bus,
        x,
        y,
        xk,
        xa,
        sy,
    }
}

impl TwoDevices {
    fn count_from_x(&self, k: PayloadKind) -> usize {
        self.bus
            .log()
            .iter()
            .filter(|(f, kind, _)| f == &self.xk && *kind == k)
            .count()
    }
}

/// [SEC-PRS-010] (PR #326 review B1): when the transport does not take this send's own
/// announcement, the announcement was not issued, so the envelope is not passed either:
/// `not-passed` with `transport-failure`. The next send issues it again before its envelope;
/// once one is taken, later envelopes within the refresh interval need none.
#[test]
fn an_announcement_not_taken_is_issued_again() {
    let t = two_devices(false);
    t.bus.0.lock().unwrap().refuse_presence = 1;
    let r = t.x.adapter.sink().send(request(&t.xa, &t.sy, "one"));
    check_result_tables(&r);
    assert!(
        matches!(
            r,
            SendRequestResult::NotPassed {
                error: ErrorCode::TransportFailure,
                ..
            }
        ),
        "{r:?}"
    );
    assert_eq!(t.count_from_x(PayloadKind::Presence), 0);
    assert_eq!(t.count_from_x(PayloadKind::Envelope), 0);
    sent(t.x.adapter.sink().send(request(&t.xa, &t.sy, "two")));
    sent(t.x.adapter.sink().send(request(&t.xa, &t.sy, "three")));
    let kinds: Vec<PayloadKind> = t
        .bus
        .log()
        .iter()
        .filter(|(f, ..)| f == &t.xk)
        .map(|(_, k, _)| *k)
        .collect();
    assert_eq!(
        kinds,
        [
            PayloadKind::Presence,
            PayloadKind::Envelope,
            PayloadKind::Envelope
        ]
    );
    assert_eq!(t.y.adapter.texts(), ["two", "three"]);
}

/// The 9f70686 race (PR #326 review N3): a send that finds another send's announcement
/// issued but not yet taken issues its own, so its envelope never overtakes the only
/// announcement. Here the second send runs while the first one's announcement is being
/// passed to the transport.
#[test]
fn a_send_does_not_rely_on_an_announcement_still_in_flight() {
    let t = two_devices(false);
    let (x2, xa, sy) = (t.x.adapter.clone(), t.xa.clone(), t.sy.clone());
    t.bus.0.lock().unwrap().on_presence = Some(Box::new(move || {
        sent(x2.sink().send(request(&xa, &sy, "second")));
    }));
    sent(t.x.adapter.sink().send(request(&t.xa, &t.sy, "first")));
    let kinds: Vec<PayloadKind> = t
        .bus
        .log()
        .iter()
        .filter(|(f, ..)| f == &t.xk)
        .map(|(_, k, _)| *k)
        .collect();
    assert_eq!(
        kinds,
        [
            PayloadKind::Presence,
            PayloadKind::Envelope,
            PayloadKind::Presence,
            PayloadKind::Envelope
        ]
    );
    assert_eq!(t.y.adapter.texts(), ["second", "first"]);
}

/// [IFC-TRN-081] (PR #326 review B3): over a transport that does not declare
/// `destination_restricted`, no presence record crosses to another implementation, so the
/// sender holds no declaration for the peer's session and the send is refused before any
/// envelope.
#[test]
fn no_presence_crosses_an_unrestricted_transport() {
    let t = two_devices(true);
    assert_eq!(
        refused(t.x.adapter.sink().send(request(&t.xa, &t.sy, "x"))),
        ErrorCode::UnknownDestination
    );
    let kinds: Vec<PayloadKind> = t.bus.log().iter().map(|(_, k, _)| *k).collect();
    assert!(kinds.is_empty(), "{kinds:?}");
}

/// [SC-ID-160], [IFC-ADP-031] (PR #326 review B3): a request is attributed only on the
/// adapter that reported the attachment. A second adapter that labels a request with the
/// first adapter's bound attachment is refused `unauthorized`.
#[test]
fn another_adapters_attachment_is_not_attributed() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (a, sa) = session(&n, 1);
    let (_b, sb) = session(&n, 2);
    local_grant(&n, &sa, &sb);
    let other = TestAdapter::new();
    n.pipes.add_adapter(other.clone());
    assert_eq!(
        refused(other.sink().send(request(&a, &sb, "spoofed"))),
        ErrorCode::Unauthorized
    );
    assert!(bus.log().is_empty());
    // The owner's own request is attributed.
    sent(n.adapter.sink().send(request(&a, &sb, "own")));
}

/// [IFC-ADP-013] (PR #326 review B3): an adapter cannot make a handle an attachment unless
/// the core gave it that connection: one the core never gave it, or gave another adapter,
/// cannot be bound.
#[test]
fn an_attachment_needs_a_connection_given_to_that_adapter() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let other = TestAdapter::new();
    n.pipes.add_adapter(other.clone());
    let record = |seed: u8| {
        n.pipes
            .device()
            .register(
                SessionId::from_random_octets([seed; 16]),
                Token::parse("test-harness").unwrap(),
                "native",
                "/work",
                SystemClock.now(),
            )
            .unwrap()
    };
    // A handle the core minted but gave to no adapter.
    let forged = Connection::accept(Default::default(), Default::default())
        .handle()
        .clone();
    other.emit(AdapterEvent::AttachmentOpened {
        attachment: forged.clone(),
        cross_check: None,
    });
    assert_eq!(
        n.pipes.bind(&forged, &record(5), None),
        Err(PipelineError::UnknownAttachment)
    );
    // A handle given to the first adapter, reported open by the other one.
    let conn = Connection::accept(Default::default(), Default::default());
    let theirs = conn.handle().clone();
    n.pipes.connect(n.adapter_id, conn).unwrap();
    n.pipes.unbind(&theirs);
    other.emit(AdapterEvent::AttachmentClosed {
        attachment: theirs.clone(),
    });
    // Still the first adapter's: the other adapter's close is ignored, and its own open
    // event for the handle would be too.
    other.emit(AdapterEvent::AttachmentOpened {
        attachment: theirs.clone(),
        cross_check: None,
    });
    n.pipes.bind(&theirs, &record(6), None).unwrap();
    assert_eq!(
        refused(other.sink().send(request(
            &theirs,
            &SessionId::from_random_octets([6; 16]),
            "x"
        ))),
        ErrorCode::Unauthorized
    );
}

/// The receipt gate (PR #326 review B3; [SEC-RPL-030]): copies of an envelope already
/// handed off draw one `duplicate` receipt between them, never more.
#[test]
fn the_receipt_gate_allows_one_duplicate_receipt() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (a, sa) = session(&n, 1);
    let (_b, sb) = session(&n, 2);
    local_grant(&n, &sa, &sb);
    let (_, _, receipts) = sent(n.adapter.sink().send(request(&a, &sb, "once")));
    assert_eq!(
        receipts.0.recv_timeout(WAIT).unwrap().state(),
        DeliveryState::HandedToHarness
    );
    let p = bus.0.lock().unwrap().last_envelope.clone().unwrap();
    let to = Destination::Session(sb.clone());
    for _ in 0..3 {
        bus.redeliver(n.pipes.device().key_id(), &to, p.clone());
    }
    assert_eq!(
        receipts.0.recv_timeout(WAIT).unwrap().state(),
        DeliveryState::Duplicate
    );
    assert!(receipts.0.try_recv().is_err());
    assert_eq!(n.adapter.deliver_calls.load(Ordering::SeqCst), 1);
}

/// The binding re-check right before the hand-off call (PR #326 review N3; [SC-DLV-007]):
/// a session unbound after the delivery-stage checks gets no call, and the copy is
/// `unreachable` with `destination-unavailable`.
#[test]
fn a_binding_that_ends_before_the_call_gets_no_call() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (a, sa) = session(&n, 1);
    let (b, sb) = session(&n, 2);
    local_grant(&n, &sa, &sb);
    let (pipes, b2) = (n.pipes.clone(), b.clone());
    // The delivery stage reads the adapter's capabilities after it has looked the session
    // up; the session's binding ends right then.
    *n.adapter.on_capabilities.lock().unwrap() = Some(Box::new(move || pipes.unbind(&b2)));
    let (_, _, receipts) = sent(n.adapter.sink().send(request(&a, &sb, "late")));
    let r = receipts.0.recv_timeout(WAIT).unwrap();
    check_receipt_tables(&r);
    assert_eq!(
        (r.state(), r.error().map(|e| e.as_str().to_owned())),
        (
            DeliveryState::Unreachable,
            Some("destination-unavailable".to_owned())
        )
    );
    assert_eq!(n.adapter.deliver_calls.load(Ordering::SeqCst), 0);
}

/// PR #326 review N1: the hand-off record exists while the hand-off call runs, so a reply
/// the harness makes before the call returns is correlated; a call that ends `failed`
/// leaves no record, so a later reply naming it goes uncorrelated.
#[test]
fn a_reply_during_the_hand_off_call_is_correlated() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (a, sa) = session(&n, 1);
    let (b, sb) = session(&n, 2);
    local_grant(&n, &sa, &sb);
    // A grant back, so a reply that goes uncorrelated is still sent and can be read.
    local_grant(&n, &sb, &sa);
    let (adapter, b2, sa2) = (n.adapter.clone(), b.clone(), sa.clone());
    let replied = Arc::new(Mutex::new(None));
    let r2 = replied.clone();
    *n.adapter.on_deliver.lock().unwrap() = Some(Box::new(move |h: &HandOff| {
        if h.message().envelope().to() == &sa2 {
            return HandOffOutcome::Completed;
        }
        let mut reply = request(&b2, &sa2, "quick answer");
        reply.requested_target = Some(h.message().envelope().id().as_str().to_owned());
        if let SendRequestResult::Sent { correlation, .. } = adapter.sink().send(reply) {
            *r2.lock().unwrap() = correlation;
        }
        HandOffOutcome::Failed
    }));
    let (id, _, _) = sent(n.adapter.sink().send(request(&a, &sb, "question")));
    assert_eq!(*replied.lock().unwrap(), Some(Correlation::Correlated));
    // The call failed: the record is gone, and a reply naming it now is uncorrelated.
    *n.adapter.on_deliver.lock().unwrap() = None;
    let mut late = request(&b, &sa, "late answer");
    late.requested_target = Some(id.as_str().to_owned());
    let (_, correlation, _) = sent(n.adapter.sink().send(late));
    assert_eq!(correlation, Some(Correlation::Uncorrelated));
}

/// PR #326 review N2: a panicking hand-off call is contained, counts as `indeterminate`
/// (`unknown`), and the copy that was re-queued behind it is still offered on that thread.
#[test]
fn a_panicking_hand_off_is_unknown_and_the_requeue_still_drains() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (a, sa) = session(&n, 1);
    let (_b, sb) = session(&n, 2);
    local_grant(&n, &sa, &sb);
    let (bus2, key, to) = (
        bus.clone(),
        n.pipes.device().key_id().clone(),
        Destination::Session(sb.clone()),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    *n.adapter.on_deliver.lock().unwrap() = Some(Box::new(move |_h: &HandOff| {
        if c.fetch_add(1, Ordering::SeqCst) == 0 {
            let p = bus2.0.lock().unwrap().last_envelope.clone().unwrap();
            bus2.redeliver(&key, &to, p);
            panic!("the adapter's input call failed hard");
        }
        HandOffOutcome::Completed
    }));
    let (_, _, receipts) = sent(n.adapter.sink().send(request(&a, &sb, "boom")));
    let states: Vec<DeliveryState> = (0..2)
        .map(|_| receipts.0.recv_timeout(WAIT).unwrap())
        .inspect(check_receipt_tables)
        .map(|r| r.state())
        .collect();
    // The panicked call may have handed the content off, so the re-queued twin is a
    // duplicate of it, offered at once rather than left waiting for the next envelope.
    assert_eq!(states, [DeliveryState::Unknown, DeliveryState::Duplicate]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// PR #326 re-review: `AuthorizationEngine::prune` is wired. When a session ends, every
/// sent and hand-off record past its reply period goes, including those of sessions still
/// bound, which would otherwise wait for their own partition's next record.
#[test]
fn a_session_end_prunes_expired_records() {
    let bus = Bus::default();
    let clock = Arc::new(ManualClock::new(SystemClock.now()));
    let n = node_on(&bus, "device-a", PipelineConfig::default(), clock.clone());
    let (a, sa) = session(&n, 1);
    let (_b, sb) = session(&n, 2);
    let (c, _sc) = session(&n, 3);
    local_grant(&n, &sa, &sb);
    sent(n.adapter.sink().send(request(&a, &sb, "old")));
    let counts = |n: &Node| {
        n.pipes
            .with_engine(|e| (e.sent_records().count(), e.handoff_records().count()))
    };
    assert_eq!(counts(&n), (1, 1));
    clock.advance_nanos(i128::from(REPLY_PERIOD_MS) * 1_000_000);
    assert_eq!(counts(&n), (1, 1));
    n.pipes.unbind(&c);
    assert_eq!(counts(&n), (0, 0));
}

// ---- binding from native signals (`spec/session-channels.md` §6.7, #331) -----------------

/// What the core process observed about a peer: the pairing key `key`, and a scope.
fn observed(key: &str) -> PeerObservation {
    PeerObservation {
        pairing_key: Some(PairingKey::from_observation(key.as_bytes().to_vec())),
        harness_label: Some(Token::parse("test-harness").unwrap()),
        working_directory: Some("/work".into()),
    }
}

/// A binding log `n` writes to from now on.
fn binding_log(n: &Node) -> MemoryBindingLog {
    let log = MemoryBindingLog::new(64);
    n.pipes.set_binding_log(Box::new(log.clone()));
    log
}

fn requirements(log: &MemoryBindingLog) -> Vec<&'static str> {
    log.entries().iter().map(|e| e.record.requirement).collect()
}

/// An attachment whose peer was observed with `key`, reporting cross-check value `e`.
fn observed_attachment(n: &Node, key: &str, e: Option<&str>) -> Attachment {
    *n.adapter.cross_check_next.lock().unwrap() = e.map(str::to_owned);
    let conn = Connection::accept(Default::default(), Default::default());
    let a = conn.handle().clone();
    n.pipes
        .connect_observed(n.adapter_id, conn, observed(key))
        .unwrap();
    a
}

/// A connection observed with `key` that carries native signals and is no attachment.
fn carrier(n: &Node, key: &str) -> Attachment {
    *n.adapter.carrier_next.lock().unwrap() = true;
    let conn = Connection::accept(Default::default(), Default::default());
    let c = conn.handle().clone();
    n.pipes
        .connect_observed(n.adapter_id, conn, observed(key))
        .unwrap();
    c
}

fn signal(on: Option<&Attachment>, native: &str, s: StartKind, e: Option<&str>) -> AdapterEvent {
    AdapterEvent::NativeSignal(NativeSignal {
        native_id: native.into(),
        start_kind: Some(s),
        cross_check: e.map(str::to_owned),
        connection: on.cloned(),
        revealed: None,
    })
}

/// The adapter's view: the session the core's latest `set_binding` named.
fn told(n: &Node, a: &Attachment) -> Option<SessionId> {
    n.adapter.bound.lock().unwrap().get(a).cloned().flatten()
}

/// [SC-ID-125], [SC-ID-129], [SC-ID-122]: with no observed pairing key, which is every
/// connection until G9 establishes one, a native signal binds nothing, even when its
/// cross-check value matches, and records a finding.
#[test]
fn a_native_signal_without_an_observed_key_fails_closed() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let log = binding_log(&n);
    let conn = Connection::accept(Default::default(), Default::default());
    let a = conn.handle().clone();
    n.pipes.connect(n.adapter_id, conn).unwrap();
    n.adapter.emit(signal(
        Some(&a),
        "native-x",
        StartKind::Fresh,
        Some("native-x"),
    ));
    n.adapter
        .emit(signal(None, "native-x", StartKind::Fresh, None));
    assert_eq!(n.pipes.binding(&a), None);
    assert_eq!(told(&n, &a), None);
    assert_eq!(requirements(&log), ["SC-ID-129", "SC-ID-129"]);
    assert!(log.entries().iter().all(|e| {
        e.result == BindingResult::FailedClosed && e.record.kind == RecordKind::Finding
    }));
    assert_eq!(n.pipes.next_native_signal_expiry(), None, "nothing held");
}

/// Case 3(a) ([SC-ID-136]): a fresh signal paired with an attachment whose cross-check value
/// is N binds N under a fresh session id, registered to this device's key with the observed
/// scope ([SC-ID-009]). Case 1 ([SC-ID-130], [SC-ID-007]): a repeat keeps it.
#[test]
fn a_paired_fresh_signal_binds_under_a_fresh_session_id() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let log = binding_log(&n);
    let a = observed_attachment(&n, "harness-1", Some("native-x"));
    n.adapter
        .emit(signal(Some(&a), "native-x", StartKind::Fresh, None));
    let sid = n.pipes.binding(&a).expect("bound");
    assert_eq!(told(&n, &a), Some(sid.clone()));
    let own = n.pipes.device().key_id().clone();
    n.pipes.with_engine(|e| {
        assert_eq!(e.binding(&sid), Some(&Binding::Key(own)));
        assert!(e.scope_of(&sid).is_some());
    });
    assert!(log.entries().is_empty(), "case 3(a) records nothing");
    n.adapter
        .emit(signal(Some(&a), "native-x", StartKind::Transition, None));
    assert_eq!(n.pipes.binding(&a), Some(sid));
    assert!(log.entries().is_empty(), "case 1 records nothing");
}

/// Case 3(c) ([SC-ID-139], [SC-ID-141]): no cross-check value binds, with a diagnostic.
#[test]
fn a_fresh_signal_without_a_cross_check_value_binds_with_a_diagnostic() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let log = binding_log(&n);
    let a = observed_attachment(&n, "harness-1", None);
    n.adapter
        .emit(signal(Some(&a), "native-x", StartKind::Fresh, None));
    assert!(n.pipes.binding(&a).is_some());
    assert_eq!(requirements(&log), ["SC-ID-141"]);
    assert_eq!(log.entries()[0].record.kind, RecordKind::Diagnostic);
}

/// Case 4 ([SC-ID-140], [SC-ID-142], [SC-ID-150], [SC-ID-151]): a transition to another N
/// deregisters the earlier binding and binds under a new session id. The earlier id is no
/// longer reachable, and a grant naming it does not follow to the new one.
#[test]
fn a_transition_rebinds_under_a_new_session_id() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let log = binding_log(&n);
    let a = observed_attachment(&n, "harness-1", Some("native-x"));
    n.adapter
        .emit(signal(Some(&a), "native-x", StartKind::Fresh, None));
    let old = n.pipes.binding(&a).expect("bound");
    let (b, sb) = session(&n, 9);
    local_grant(&n, &sb, &old);
    n.adapter
        .emit(signal(Some(&a), "native-y", StartKind::Transition, None));
    let new = n.pipes.binding(&a).expect("re-bound");
    assert_ne!(new, old);
    assert_eq!(told(&n, &a), Some(new.clone()));
    assert_eq!(
        requirements(&log),
        ["SC-ID-142"],
        "the attachment's value differs"
    );
    n.pipes
        .with_engine(|e| assert_eq!(e.binding(&old), None, "deregistered"));
    for to in [&old, &new] {
        let r = n.adapter.sink().send(request(&b, to, "hi"));
        assert!(
            matches!(r, SendRequestResult::Refused { .. }),
            "{to}: {r:?}"
        );
    }
}

/// Case 2 ([SC-ID-131] to [SC-ID-133]): N bound to a live attachment refuses a newcomer
/// with a finding, and the existing binding is untouched.
#[test]
fn a_duplicate_native_id_is_refused_and_displaces_nothing() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let a = observed_attachment(&n, "harness-1", Some("native-x"));
    n.adapter
        .emit(signal(Some(&a), "native-x", StartKind::Fresh, None));
    let sa = n.pipes.binding(&a).expect("bound");
    let log = binding_log(&n);
    let b = observed_attachment(&n, "harness-2", Some("native-x"));
    n.adapter
        .emit(signal(Some(&b), "native-x", StartKind::Fresh, None));
    assert_eq!(n.pipes.binding(&b), None);
    assert_eq!(n.pipes.binding(&a), Some(sa.clone()));
    assert_eq!(told(&n, &a), Some(sa));
    assert_eq!(requirements(&log), ["SC-ID-132"]);
    assert_eq!(log.entries()[0].result, BindingResult::Refused);
}

/// Case 3(b) on a bound attachment ([SC-ID-137], [SC-ID-138], [SC-ID-152], [SC-ID-153]): a
/// fresh signal for another N whose own cross-check value differs fails closed, and the
/// attachment's stale binding is deregistered.
#[test]
fn a_cross_check_mismatch_fails_closed_and_ends_a_stale_binding() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let a = observed_attachment(&n, "harness-1", Some("native-x"));
    n.adapter
        .emit(signal(Some(&a), "native-x", StartKind::Fresh, None));
    let old = n.pipes.binding(&a).expect("bound");
    let log = binding_log(&n);
    n.adapter.emit(signal(
        Some(&a),
        "native-y",
        StartKind::Fresh,
        Some("native-z"),
    ));
    assert_eq!(n.pipes.binding(&a), None);
    assert_eq!(told(&n, &a), None);
    n.pipes.with_engine(|e| assert_eq!(e.binding(&old), None));
    assert_eq!(requirements(&log), ["SC-ID-138", "SC-ID-153"]);
}

/// Signals are decided one at a time (§6.7.3): eight threads reporting the same N on eight
/// attachments at once bind exactly one, and the other seven are refused under case 2.
#[test]
fn concurrent_signals_for_one_native_id_bind_once() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let log = binding_log(&n);
    let atts: Vec<Attachment> = (0..8)
        .map(|k| observed_attachment(&n, &format!("harness-{k}"), None))
        .collect();
    std::thread::scope(|s| {
        for a in &atts {
            let adapter = n.adapter.clone();
            s.spawn(move || adapter.emit(signal(Some(a), "native-x", StartKind::Fresh, None)));
        }
    });
    let bound = atts.iter().filter(|a| n.pipes.binding(a).is_some()).count();
    assert_eq!(bound, 1);
    let mut ids = requirements(&log);
    ids.sort_unstable();
    let mut want = vec!["SC-ID-132"; 7];
    want.push("SC-ID-141");
    assert_eq!(ids, want);
}

/// [SC-ID-123]: a signal that arrives before its attachment is held, and the attachment
/// that opens within the window pairs with it.
#[test]
fn a_signal_before_its_attachment_is_held_and_pairs() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let log = binding_log(&n);
    let c = carrier(&n, "harness-1");
    n.adapter
        .emit(signal(Some(&c), "native-x", StartKind::Fresh, None));
    assert!(n.pipes.next_native_signal_expiry().is_some(), "held");
    let a = observed_attachment(&n, "harness-1", Some("native-x"));
    assert!(n.pipes.binding(&a).is_some());
    assert_eq!(n.pipes.next_native_signal_expiry(), None);
    assert!(log.entries().is_empty());
}

/// [SC-ID-124], [SC-ID-128]: a held signal whose window ended is dropped with a
/// diagnostic, and an attachment that opens later binds nothing.
#[test]
fn a_held_signal_is_dropped_when_its_window_ends() {
    let bus = Bus::default();
    let config = PipelineConfig {
        native_signal_window: Duration::ZERO,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let log = binding_log(&n);
    let c = carrier(&n, "harness-1");
    n.adapter
        .emit(signal(Some(&c), "native-x", StartKind::Fresh, None));
    assert!(n.pipes.next_native_signal_expiry().is_some());
    n.pipes.expire_native_signals();
    assert_eq!(n.pipes.next_native_signal_expiry(), None);
    assert_eq!(requirements(&log), ["SC-ID-128"]);
    assert_eq!(log.entries()[0].result, BindingResult::Dropped);
    let a = observed_attachment(&n, "harness-1", Some("native-x"));
    assert_eq!(n.pipes.binding(&a), None);
}

/// Bounded memory ([SC-ID-123]): past `max_pending_signals`, a signal is dropped with a
/// diagnostic. With one place, another key's only held signal is kept and the newcomer
/// goes (#335); a second signal under the held one's own key replaces it.
#[test]
fn held_signals_are_bounded() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 1,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let log = binding_log(&n);
    let c1 = carrier(&n, "harness-1");
    let c2 = carrier(&n, "harness-2");
    n.adapter
        .emit(signal(Some(&c1), "native-x", StartKind::Fresh, None));
    n.adapter
        .emit(signal(Some(&c2), "native-y", StartKind::Fresh, None));
    assert_eq!(requirements(&log), ["SC-ID-128"]);
    n.adapter
        .emit(signal(Some(&c1), "native-x2", StartKind::Fresh, None));
    assert_eq!(requirements(&log), ["SC-ID-128", "SC-ID-128"]);
    let a2 = observed_attachment(&n, "harness-2", None);
    assert_eq!(n.pipes.binding(&a2), None, "the newcomer was dropped");
    let a1 = observed_attachment(&n, "harness-1", Some("native-x2"));
    assert!(
        n.pipes.binding(&a1).is_some(),
        "the held key kept its place"
    );
    assert_eq!(n.pipes.next_native_signal_expiry(), None);
}

/// #335: one key flooding the cap cannot evict another key's held signal; once it holds
/// the most, each of its new signals evicts its own oldest.
#[test]
fn a_flooding_key_cannot_evict_another_keys_held_signal() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 4,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let log = binding_log(&n);
    let victim = carrier(&n, "harness-v");
    let flood = carrier(&n, "harness-f");
    n.adapter
        .emit(signal(Some(&victim), "native-v", StartKind::Fresh, None));
    for i in 0..20 {
        n.adapter.emit(signal(
            Some(&flood),
            &format!("native-f{i}"),
            StartKind::Fresh,
            None,
        ));
    }
    // 21 signals, 4 places: 17 dropped, every one the flooder's.
    assert_eq!(requirements(&log), ["SC-ID-128"; 17]);
    let v = observed_attachment(&n, "harness-v", None);
    assert!(
        n.pipes.binding(&v).is_some(),
        "the victim's signal was held"
    );
}

/// #335: many keys, one signal each, cannot evict another key's held signal either: a
/// newcomer with nothing pending is refused while every holder has one.
#[test]
fn many_flooding_keys_cannot_evict_another_keys_held_signal() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 3,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let log = binding_log(&n);
    let victim = carrier(&n, "harness-v");
    n.adapter
        .emit(signal(Some(&victim), "native-v", StartKind::Fresh, None));
    for i in 0..10 {
        let c = carrier(&n, &format!("harness-f{i}"));
        n.adapter.emit(signal(
            Some(&c),
            &format!("native-f{i}"),
            StartKind::Fresh,
            None,
        ));
    }
    assert_eq!(requirements(&log), ["SC-ID-128"; 8]);
    let v = observed_attachment(&n, "harness-v", None);
    assert!(
        n.pipes.binding(&v).is_some(),
        "the victim's signal was held"
    );
    // The two flooders admitted first kept theirs; the refused ones hold nothing.
    let f0 = observed_attachment(&n, "harness-f0", None);
    let f9 = observed_attachment(&n, "harness-f9", None);
    assert!(n.pipes.binding(&f0).is_some());
    assert_eq!(n.pipes.binding(&f9), None);
}

/// #335: a key flooding while a decision holds the binding turn, so that every signal is
/// still queued, cannot evict another key's queued signal; an unkeyed connection
/// flooding counts against that connection's own share.
#[test]
fn a_flood_cannot_evict_another_keys_queued_signal() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 3,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let x = observed_attachment(&n, "harness-x", None);
    let victim = carrier(&n, "harness-v");
    let flood = carrier(&n, "harness-f");
    let bare = Connection::accept(Default::default(), Default::default());
    let unkeyed = bare.handle().clone();
    n.pipes.connect(n.adapter_id, bare).unwrap();
    let log = binding_log(&n);
    let adapter = n.adapter.clone();
    during_next_decision(&n, move || {
        adapter.emit(signal(Some(&victim), "native-v", StartKind::Fresh, None));
        for i in 0..10 {
            adapter.emit(signal(
                Some(&flood),
                &format!("native-f{i}"),
                StartKind::Fresh,
                None,
            ));
        }
        for i in 0..10 {
            adapter.emit(signal(
                Some(&unkeyed),
                &format!("native-u{i}"),
                StartKind::Fresh,
                None,
            ));
        }
    });
    n.adapter
        .emit(signal(Some(&x), "native-x", StartKind::Fresh, None));
    assert!(n.pipes.binding(&x).is_some());
    let ids = requirements(&log);
    // 21 queued in 3 places: 18 dropped, then the victim's and one flood signal held and
    // the unkeyed connection's last one decided unpairable.
    assert_eq!(
        ids.iter().filter(|r| **r == "SC-ID-128").count(),
        18,
        "{ids:?}"
    );
    assert_eq!(
        ids.iter().filter(|r| **r == "SC-ID-129").count(),
        1,
        "{ids:?}"
    );
    let v = observed_attachment(&n, "harness-v", None);
    assert!(
        n.pipes.binding(&v).is_some(),
        "the victim's signal was kept"
    );
}

/// #335, fair share: a holder with at least two more than the newcomer gives up its
/// oldest, held or queued, so a newcomer is not refused while one holder has the cap.
#[test]
fn the_heaviest_holder_makes_room_for_a_newcomer() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 3,
        ..PipelineConfig::default()
    };
    // Held: three signals of one key fill the cap; a new key's signal takes a place.
    let n = node(&bus, "device-a", config.clone());
    let log = binding_log(&n);
    let heavy = carrier(&n, "harness-h");
    let fresh = carrier(&n, "harness-n");
    for i in 0..3 {
        n.adapter.emit(signal(
            Some(&heavy),
            &format!("native-h{i}"),
            StartKind::Fresh,
            None,
        ));
    }
    assert!(requirements(&log).is_empty());
    n.adapter
        .emit(signal(Some(&fresh), "native-n", StartKind::Fresh, None));
    assert_eq!(requirements(&log), ["SC-ID-128"]);
    let a = observed_attachment(&n, "harness-n", None);
    assert!(n.pipes.binding(&a).is_some(), "the newcomer was held");
    // Queued: the same while a decision holds the turn.
    let n = node(&bus, "device-b", config);
    let x = observed_attachment(&n, "harness-x", Some("native-x"));
    let heavy = carrier(&n, "harness-h");
    let fresh = carrier(&n, "harness-n");
    let log = binding_log(&n);
    let adapter = n.adapter.clone();
    during_next_decision(&n, move || {
        for i in 0..3 {
            adapter.emit(signal(
                Some(&heavy),
                &format!("native-h{i}"),
                StartKind::Fresh,
                None,
            ));
        }
        adapter.emit(signal(Some(&fresh), "native-n", StartKind::Fresh, None));
    });
    n.adapter
        .emit(signal(Some(&x), "native-x", StartKind::Fresh, None));
    assert_eq!(requirements(&log), ["SC-ID-128"]);
    let a = observed_attachment(&n, "harness-n", None);
    assert!(
        n.pipes.binding(&a).is_some(),
        "the newcomer was queued, then held"
    );
}

/// #335: while a pass decides the held signals, those not yet decided still count against
/// the cap and their holders' shares, so signals reported meanwhile cannot exceed it.
#[test]
fn held_signals_count_while_a_pass_decides_them() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 2,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let l = carrier(&n, "harness-l");
    let m = carrier(&n, "harness-m");
    let f = carrier(&n, "harness-f");
    n.adapter
        .emit(signal(Some(&l), "native-l", StartKind::Fresh, None));
    n.adapter
        .emit(signal(Some(&m), "native-m", StartKind::Fresh, None));
    let log = binding_log(&n);
    let adapter = n.adapter.clone();
    during_next_decision(&n, move || {
        // `m`'s held signal and the first of these fill the cap; the second replaces the
        // first, its holder's own.
        adapter.emit(signal(Some(&f), "native-f1", StartKind::Fresh, None));
        adapter.emit(signal(Some(&f), "native-f2", StartKind::Fresh, None));
    });
    let al = observed_attachment(&n, "harness-l", Some("native-l"));
    assert!(n.pipes.binding(&al).is_some());
    assert_eq!(requirements(&log), ["SC-ID-128"]);
    let am = observed_attachment(&n, "harness-m", Some("native-m"));
    assert!(n.pipes.binding(&am).is_some(), "`m`'s held signal was kept");
}

/// PR #337 ([SC-ID-154], "until a later signal is paired"): a later transition dropped at
/// the cap while an earlier signal's decision for the same attachment runs keeps the
/// attachment withheld; that earlier decision does not release it. A signal reported after
/// the dropped one does.
#[test]
fn an_earlier_decision_does_not_release_a_later_drop() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 1,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let a = observed_attachment(&n, "harness-a", Some("native-a"));
    let g = carrier(&n, "harness-g");
    let log = binding_log(&n);
    let (adapter, aa) = (n.adapter.clone(), a.clone());
    // After the first signal registered `a`, while its decision still runs: a filler takes
    // the one place, so `a`'s later transition is refused.
    *n.adapter.on_set_binding.lock().unwrap() = Some(Box::new(move || {
        adapter.emit(signal(Some(&g), "native-g", StartKind::Fresh, None));
        adapter.emit(signal(Some(&aa), "native-a2", StartKind::Transition, None));
    }));
    n.adapter
        .emit(signal(Some(&a), "native-a", StartKind::Fresh, None));
    assert_eq!(requirements(&log), ["SC-ID-128", "SC-ID-154"]);
    let sa = n.pipes.binding(&a).expect("still registered");
    assert_eq!(
        told(&n, &a),
        None,
        "still withheld after the earlier decision"
    );
    let (_, sc) = session(&n, 51);
    local_grant(&n, &sa, &sc);
    let r = n.adapter.sink().send(request(&a, &sc, "from a"));
    assert_eq!(r.error(), Some(ErrorCode::Unauthorized));
    // The filler's attachment opens and frees the place; then a transition reported after
    // the dropped one pairs: `a` moves and is released.
    observed_attachment(&n, "harness-g", Some("native-g"));
    n.adapter
        .emit(signal(Some(&a), "native-a3", StartKind::Transition, None));
    let now = n.pipes.binding(&a).expect("re-bound");
    assert_ne!(now, sa);
    assert_eq!(told(&n, &a), Some(now));
}

/// PR #337 ([SC-ID-154]): a later transition dropped while an earlier transition's
/// decision is re-binding the attachment (deregistered, not yet registered again) makes
/// that new binding start withheld; a signal reported after the dropped one releases it.
#[test]
fn a_drop_between_bindings_withholds_the_new_binding() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 1,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let a = observed_attachment(&n, "harness-a", Some("native-a"));
    n.adapter
        .emit(signal(Some(&a), "native-a", StartKind::Fresh, None));
    let sa = n.pipes.binding(&a).expect("bound");
    let g = carrier(&n, "harness-g");
    let log = binding_log(&n);
    let (adapter, aa) = (n.adapter.clone(), a.clone());
    // During the re-binding's declaration step, after the earlier binding ended.
    during_next_decision(&n, move || {
        adapter.emit(signal(Some(&g), "native-g", StartKind::Fresh, None));
        adapter.emit(signal(Some(&aa), "native-a2", StartKind::Transition, None));
    });
    n.adapter
        .emit(signal(Some(&a), "native-a1", StartKind::Transition, None));
    let s1 = n
        .pipes
        .binding(&a)
        .expect("re-bound to the earlier transition");
    assert_ne!(s1, sa);
    let ids = requirements(&log);
    assert!(ids.contains(&"SC-ID-128"), "{ids:?}");
    assert!(ids.contains(&"SC-ID-154"), "{ids:?}");
    assert_eq!(told(&n, &a), None, "the new binding starts withheld");
    observed_attachment(&n, "harness-g", Some("native-g"));
    n.adapter
        .emit(signal(Some(&a), "native-a3", StartKind::Transition, None));
    let now = n.pipes.binding(&a).expect("re-bound");
    assert_ne!(now, s1);
    assert_eq!(told(&n, &a), Some(now));
}

/// PR #337 ([SC-ID-154]): a binding decided elsewhere (`Pipelines::bind`) is no signal, so
/// it does not answer a drop attributed to the attachment: it starts withheld until a
/// signal reported after the drop pairs.
#[test]
fn an_external_bind_does_not_answer_a_drop() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 1,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let a = observed_attachment(&n, "harness-a", Some("native-x"));
    let g = carrier(&n, "harness-g");
    n.adapter
        .emit(signal(Some(&g), "native-g", StartKind::Fresh, None));
    let log = binding_log(&n);
    // The place is taken: `a`'s signal is refused, attributed to `a` (not yet bound).
    n.adapter
        .emit(signal(Some(&a), "native-x", StartKind::Fresh, None));
    assert_eq!(requirements(&log), ["SC-ID-128"]);
    let sa = SessionId::from_random_octets([52; 16]);
    let record = n
        .pipes
        .device()
        .register(
            sa.clone(),
            Token::parse("test-harness").unwrap(),
            "native-x",
            "/work",
            SystemClock.now(),
        )
        .unwrap();
    n.pipes.bind(&a, &record, None).unwrap();
    assert_eq!(n.pipes.binding(&a), Some(sa.clone()));
    assert_eq!(requirements(&log), ["SC-ID-128", "SC-ID-154"]);
    assert_eq!(told(&n, &a), None, "starts withheld");
    observed_attachment(&n, "harness-g", Some("native-g"));
    n.adapter
        .emit(signal(Some(&a), "native-x", StartKind::Fresh, None));
    assert_eq!(
        told(&n, &a),
        n.pipes.binding(&a),
        "a later signal released it"
    );
    assert!(told(&n, &a).is_some());
}

/// #338 (the reviewer's reproduction): held `[s1(K), h2(L), s2(K)]`; L's attachment opens,
/// and K's opens while h2's decision runs, after s1 was found not yet pairable in that
/// pass. s2 then pairs and binds first. s1's window ends when s2 pairs ([SC-ID-123]): it
/// is dropped before it pairs ([SC-ID-124], [SC-ID-128]), so it never binds the
/// attachment back to the older native id, and a later signal for the newer one finds the
/// binding unchanged (case 1).
#[test]
fn an_older_held_signal_does_not_rebind_over_a_newer_one() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let ck = carrier(&n, "harness-k");
    let cl = carrier(&n, "harness-l");
    n.adapter
        .emit(signal(Some(&ck), "native-1", StartKind::Fresh, None));
    n.adapter
        .emit(signal(Some(&cl), "native-h2", StartKind::Fresh, None));
    n.adapter
        .emit(signal(Some(&ck), "native-2", StartKind::Fresh, None));
    let log = binding_log(&n);
    let opened: Arc<Mutex<Option<Attachment>>> = Arc::default();
    let (pipes, id, slot) = (n.pipes.clone(), n.adapter_id, opened.clone());
    // During h2's decision: K's attachment opens.
    during_next_decision(&n, move || {
        let conn = Connection::accept(Default::default(), Default::default());
        *slot.lock().unwrap() = Some(conn.handle().clone());
        pipes
            .connect_observed(id, conn, observed("harness-k"))
            .unwrap();
    });
    let l = observed_attachment(&n, "harness-l", Some("native-h2"));
    assert!(n.pipes.binding(&l).is_some());
    let a = opened.lock().unwrap().clone().expect("opened during h2");
    let ids = requirements(&log);
    assert_eq!(
        count(&ids, "SC-ID-128"),
        1,
        "the older signal is dropped: {ids:?}"
    );
    assert_eq!(
        n.pipes.next_native_signal_expiry(),
        None,
        "nothing left held"
    );
    let s = n.pipes.binding(&a).expect("bound to the newer signal");
    assert_eq!(told(&n, &a), Some(s.clone()));
    // A later signal for the newer native id: case 1, nothing changes.
    n.adapter
        .emit(signal(Some(&a), "native-2", StartKind::Fresh, None));
    assert_eq!(
        n.pipes.binding(&a),
        Some(s.clone()),
        "bound to native-2 all along"
    );
    assert_eq!(told(&n, &a), Some(s));
}

/// #338 ([SC-ID-154]): an older signal whose window ended when a newer signal of its key
/// paired withholds nothing, as the newer pairing answered it, and is no longer pending:
/// held `[s1(K), h2(L), s2(K)]` as above; while s2's binding is told to the adapter, two
/// fillers and K's third signal fit in the cap of 3 without evicting anything, because s1
/// has already gone.
#[test]
fn an_older_signal_dropped_after_a_newer_pairing_withholds_nothing() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 3,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let ck = carrier(&n, "harness-k");
    let cl = carrier(&n, "harness-l");
    let cx = carrier(&n, "harness-x");
    let cy = carrier(&n, "harness-y");
    n.adapter
        .emit(signal(Some(&ck), "native-1", StartKind::Fresh, None));
    n.adapter
        .emit(signal(Some(&cl), "native-h2", StartKind::Fresh, None));
    n.adapter
        .emit(signal(Some(&ck), "native-2", StartKind::Fresh, None));
    let log = binding_log(&n);
    let opened: Arc<Mutex<Option<Attachment>>> = Arc::default();
    let (pipes, id, slot) = (n.pipes.clone(), n.adapter_id, opened.clone());
    during_next_decision(&n, move || {
        let conn = Connection::accept(Default::default(), Default::default());
        *slot.lock().unwrap() = Some(conn.handle().clone());
        pipes
            .connect_observed(id, conn, observed("harness-k"))
            .unwrap();
    });
    // The first binding told is `l`'s (h2); it arms the hook for the next, `a`'s (s2).
    let (adapter, outer) = (n.adapter.clone(), n.adapter.clone());
    *n.adapter.on_set_binding.lock().unwrap() = Some(Box::new(move || {
        *outer.on_set_binding.lock().unwrap() = Some(Box::new(move || {
            adapter.emit(signal(Some(&cx), "native-x", StartKind::Fresh, None));
            adapter.emit(signal(Some(&cy), "native-y", StartKind::Fresh, None));
            adapter.emit(signal(Some(&ck), "native-3", StartKind::Fresh, None));
        }));
    }));
    observed_attachment(&n, "harness-l", Some("native-h2"));
    let a = opened.lock().unwrap().clone().expect("opened during h2");
    let ids = requirements(&log);
    assert_eq!(count(&ids, "SC-ID-128"), 1, "s1 is dropped: {ids:?}");
    assert_eq!(
        count(&ids, "SC-ID-154"),
        0,
        "and withholds nothing: {ids:?}"
    );
    assert!(n.pipes.binding(&a).is_some());
}

/// The held `[older.., h2(L), s2(K)]` set-up of #338, `older` being K's earlier signals:
/// once the caller opens L's attachment, K's opens while h2's decision runs, so s2 pairs
/// with it before them. Returns the binding log (set before L's attachment opens) and the
/// slot K's attachment is put in.
fn k_opens_during_h2(
    n: &Node,
    older: &[&str],
) -> (MemoryBindingLog, Arc<Mutex<Option<Attachment>>>) {
    let ck = carrier(n, "harness-k");
    let cl = carrier(n, "harness-l");
    for native in older {
        n.adapter
            .emit(signal(Some(&ck), native, StartKind::Fresh, None));
    }
    n.adapter
        .emit(signal(Some(&cl), "native-h2", StartKind::Fresh, None));
    n.adapter
        .emit(signal(Some(&ck), "native-2", StartKind::Fresh, None));
    let log = binding_log(n);
    let opened: Arc<Mutex<Option<Attachment>>> = Arc::default();
    let (pipes, id, slot) = (n.pipes.clone(), n.adapter_id, opened.clone());
    during_next_decision(n, move || {
        let conn = Connection::accept(Default::default(), Default::default());
        *slot.lock().unwrap() = Some(conn.handle().clone());
        pipes
            .connect_observed(id, conn, observed("harness-k"))
            .unwrap();
    });
    (log, opened)
}

/// PR #339 review B1: the ordering belongs to the key (the harness process), not to one
/// attachment. s2 pairs with `a`; `a`'s connection ends while its binding is told to the
/// adapter; the shim reconnects as `b` under the same key. s1, older, must not bind `b`
/// to the older conversation: its window ended when s2 paired.
#[test]
fn an_older_held_signal_does_not_bind_a_reconnected_attachment() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (log, opened) = k_opens_during_h2(&n, &["native-1"]);
    // The first binding told is `l`'s; it arms the hook for `a`'s: `a` disconnects.
    let (outer, pipes, slot) = (n.adapter.clone(), n.pipes.clone(), opened.clone());
    *n.adapter.on_set_binding.lock().unwrap() = Some(Box::new(move || {
        *outer.on_set_binding.lock().unwrap() = Some(Box::new(move || {
            let a = slot.lock().unwrap().clone().expect("opened during h2");
            pipes.disconnect(&a);
        }));
    }));
    observed_attachment(&n, "harness-l", Some("native-h2"));
    let a = opened.lock().unwrap().clone().expect("opened during h2");
    assert_eq!(n.pipes.binding(&a), None, "disconnected");
    let b = observed_attachment(&n, "harness-k", None);
    assert_eq!(n.pipes.binding(&b), None, "s1 is gone: nothing binds `b`");
    let ids = requirements(&log);
    assert_eq!(count(&ids, "SC-ID-128"), 1, "{ids:?}");
    assert_eq!(
        n.pipes.next_native_signal_expiry(),
        None,
        "nothing left held"
    );
}

/// PR #339 review N2: every older held signal of the key goes when a newer one pairs, not
/// only the latest of them; and a later signal for the newer native id finds the binding
/// unchanged, so nothing moved it backwards.
#[test]
fn every_older_held_signal_of_the_key_goes_when_a_newer_one_pairs() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (log, opened) = k_opens_during_h2(&n, &["native-0", "native-1"]);
    observed_attachment(&n, "harness-l", Some("native-h2"));
    let a = opened.lock().unwrap().clone().expect("opened during h2");
    let ids = requirements(&log);
    assert_eq!(count(&ids, "SC-ID-128"), 2, "s0 and s1: {ids:?}");
    assert_eq!(
        n.pipes.next_native_signal_expiry(),
        None,
        "nothing left held"
    );
    let s = n.pipes.binding(&a).expect("bound to native-2");
    n.adapter
        .emit(signal(Some(&a), "native-2", StartKind::Fresh, None));
    assert_eq!(n.pipes.binding(&a), Some(s.clone()), "case 1: unchanged");
    assert_eq!(told(&n, &a), Some(s));
}

/// #338: a pairing ends only the windows of its own key under its own adapter. An older
/// held signal of another key, and one of the same key under another adapter, stay held
/// and still bind when their candidates open.
#[test]
fn a_pairing_ends_only_its_own_keys_windows() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let other = TestAdapter::new();
    let other_id = n.pipes.add_adapter(other.clone());
    *other.carrier_next.lock().unwrap() = true;
    let conn = Connection::accept(Default::default(), Default::default());
    let q = conn.handle().clone();
    n.pipes
        .connect_observed(other_id, conn, observed("harness-k"))
        .unwrap();
    other.emit(signal(Some(&q), "native-q", StartKind::Fresh, None));
    let cl = carrier(&n, "harness-l");
    n.adapter
        .emit(signal(Some(&cl), "native-l", StartKind::Fresh, None));
    // A newer signal of key K pairs at once with this adapter's K attachment.
    let a = observed_attachment(&n, "harness-k", Some("native-k"));
    let log = binding_log(&n);
    n.adapter
        .emit(signal(Some(&a), "native-k", StartKind::Fresh, None));
    assert!(n.pipes.binding(&a).is_some());
    assert_eq!(count(&requirements(&log), "SC-ID-128"), 0);
    let l = observed_attachment(&n, "harness-l", Some("native-l"));
    assert!(
        n.pipes.binding(&l).is_some(),
        "another key's signal stayed held"
    );
    let conn = Connection::accept(Default::default(), Default::default());
    let aq = conn.handle().clone();
    n.pipes
        .connect_observed(other_id, conn, observed("harness-k"))
        .unwrap();
    assert!(
        n.pipes.binding(&aq).is_some(),
        "the same key under another adapter stayed held"
    );
}

/// A binding log that runs a hook, on the thread writing it, at the first entry for
/// `requirement`: in the drop path that is after the signal was taken under the core lock
/// and before its withholding is applied. Entries are kept as `MemoryBindingLog` keeps them.
struct HookedLog {
    inner: MemoryBindingLog,
    requirement: &'static str,
    hook: Option<Box<dyn FnOnce() + Send>>,
}

impl BindingLog for HookedLog {
    fn record(&mut self, entry: BindingLogEntry) {
        let fire = entry.record.requirement == self.requirement;
        self.inner.record(entry);
        if fire && let Some(hook) = self.hook.take() {
            hook();
        }
    }
}

/// PR #339 review B1 ([SC-ID-154]): an eviction drop still in flight (its signal taken under
/// the lock on one thread, its withholding applied after the lock is released) can land
/// after a newer signal of the same key paired with the attachment on the turn holder's
/// thread. It is answered already and must withhold nothing.
///
/// Deterministic: cap 2, K's s1 held. While x's decision holds the turn, K's attachment
/// `a` opens and a filler queues; another thread then reports K's s2, which takes s1 by
/// K's own share and stops in the drop path (at s1's diagnostic) until s2 has paired and
/// bound `a` on the turn holder's thread.
#[test]
fn a_late_eviction_drop_does_not_withhold_a_newer_binding() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 2,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let ck = carrier(&n, "harness-k");
    let cf = carrier(&n, "harness-f");
    let x = observed_attachment(&n, "harness-x", Some("native-x"));
    n.adapter
        .emit(signal(Some(&ck), "native-1", StartKind::Fresh, None));
    let (evicted_tx, evicted_rx) = std::sync::mpsc::channel::<()>();
    let (bound_tx, bound_rx) = std::sync::mpsc::channel::<()>();
    let entries = MemoryBindingLog::new(64);
    n.pipes.set_binding_log(Box::new(HookedLog {
        inner: entries.clone(),
        requirement: "SC-ID-128",
        hook: Some(Box::new(move || {
            evicted_tx.send(()).unwrap();
            bound_rx.recv_timeout(WAIT).expect("s2 bound `a`");
        })),
    }));
    let opened: Arc<Mutex<Option<Attachment>>> = Arc::default();
    let other: Arc<Mutex<Option<std::thread::JoinHandle<()>>>> = Arc::default();
    let (pipes, id, adapter) = (n.pipes.clone(), n.adapter_id, n.adapter.clone());
    let (slot, joiner, outer) = (opened.clone(), other.clone(), n.adapter.clone());
    during_next_decision(&n, move || {
        // x's binding is told first; it arms the hook for the next, `a`'s (s2's).
        *outer.on_set_binding.lock().unwrap() = Some(Box::new({
            let outer = outer.clone();
            move || {
                *outer.on_set_binding.lock().unwrap() = Some(Box::new(move || {
                    bound_tx.send(()).unwrap();
                }));
            }
        }));
        *adapter.cross_check_next.lock().unwrap() = Some("native-2".into());
        let conn = Connection::accept(Default::default(), Default::default());
        *slot.lock().unwrap() = Some(conn.handle().clone());
        pipes
            .connect_observed(id, conn, observed("harness-k"))
            .unwrap();
        adapter.emit(signal(Some(&cf), "native-f", StartKind::Fresh, None));
        let reporter = adapter.clone();
        *joiner.lock().unwrap() = Some(std::thread::spawn(move || {
            reporter.emit(signal(Some(&ck), "native-2", StartKind::Fresh, None));
        }));
        evicted_rx
            .recv_timeout(WAIT)
            .expect("s1 taken for eviction");
    });
    n.adapter
        .emit(signal(Some(&x), "native-x", StartKind::Fresh, None));
    other
        .lock()
        .unwrap()
        .take()
        .expect("spawned")
        .join()
        .unwrap();
    let a = opened.lock().unwrap().clone().expect("opened");
    let s = n.pipes.binding(&a).expect("s2 bound `a`");
    let ids: Vec<&str> = entries
        .entries()
        .iter()
        .map(|e| e.record.requirement)
        .collect();
    assert_eq!(count(&ids, "SC-ID-128"), 1, "s1 was dropped: {ids:?}");
    assert_eq!(count(&ids, "SC-ID-154"), 0, "and withheld nothing: {ids:?}");
    assert_eq!(told(&n, &a), Some(s), "`a` still delivers");
}

/// PR #339 review B1, with an attachment already bound: what answers a late drop is the
/// attachment's *latest* pairing, not its first. `a` is bound by s0. While x's decision
/// holds the turn, a filler and `a`'s s1 queue (cap 2); another thread reports `a`'s s2,
/// which takes s1 by `a`'s key's own share and stops in the drop path. s2 then pairs on
/// the turn holder's thread (case 1, same native id), and only then is s1's drop applied:
/// s1 is newer than s0 but older than s2, so it withholds nothing.
#[test]
fn a_late_eviction_drop_is_answered_by_the_latest_pairing() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 2,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let a = observed_attachment(&n, "harness-k", Some("native-k"));
    n.adapter
        .emit(signal(Some(&a), "native-k", StartKind::Fresh, None));
    let s = n.pipes.binding(&a).expect("bound by s0");
    let cf = carrier(&n, "harness-f");
    let x = observed_attachment(&n, "harness-x", Some("native-x"));
    let (evicted_tx, evicted_rx) = std::sync::mpsc::channel::<()>();
    let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
    let entries = MemoryBindingLog::new(64);
    n.pipes.set_binding_log(Box::new(HookedLog {
        inner: entries.clone(),
        requirement: "SC-ID-128",
        hook: Some(Box::new(move || {
            evicted_tx.send(()).unwrap();
            go_rx.recv_timeout(WAIT).expect("s2 decided");
        })),
    }));
    let other: Arc<Mutex<Option<std::thread::JoinHandle<()>>>> = Arc::default();
    let (adapter, joiner, aa) = (n.adapter.clone(), other.clone(), a.clone());
    during_next_decision(&n, move || {
        adapter.emit(signal(Some(&cf), "native-f", StartKind::Fresh, None));
        adapter.emit(signal(Some(&aa), "native-k", StartKind::Fresh, None));
        let reporter = adapter.clone();
        *joiner.lock().unwrap() = Some(std::thread::spawn(move || {
            reporter.emit(signal(Some(&aa), "native-k", StartKind::Fresh, None));
        }));
        evicted_rx
            .recv_timeout(WAIT)
            .expect("s1 taken for eviction");
    });
    n.adapter
        .emit(signal(Some(&x), "native-x", StartKind::Fresh, None));
    // s2 has paired with `a` on this thread; now let s1's drop land.
    go_tx.send(()).unwrap();
    other
        .lock()
        .unwrap()
        .take()
        .expect("spawned")
        .join()
        .unwrap();
    let ids: Vec<&str> = entries
        .entries()
        .iter()
        .map(|e| e.record.requirement)
        .collect();
    assert_eq!(count(&ids, "SC-ID-128"), 1, "s1 was dropped: {ids:?}");
    assert_eq!(count(&ids, "SC-ID-154"), 0, "and withheld nothing: {ids:?}");
    assert_eq!(n.pipes.binding(&a), Some(s.clone()), "case 1: unchanged");
    assert_eq!(told(&n, &a), Some(s), "`a` still delivers");
}

/// PR #337 re-review N6 ([SC-ID-154]): a later transition dropped while `unbind` tells the
/// adapter (an earlier transition's re-binding, after its deregistration began and before
/// it registers again) still makes the new binding start withheld: `unbind` keeps the
/// drop's place in arrival order.
#[test]
fn a_drop_while_unbinding_withholds_the_new_binding() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 1,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let a = observed_attachment(&n, "harness-a", Some("native-a"));
    n.adapter
        .emit(signal(Some(&a), "native-a", StartKind::Fresh, None));
    let sa = n.pipes.binding(&a).expect("bound");
    let g = carrier(&n, "harness-g");
    let log = binding_log(&n);
    let (adapter, aa) = (n.adapter.clone(), a.clone());
    *n.adapter.on_unset_binding.lock().unwrap() = Some(Box::new(move || {
        adapter.emit(signal(Some(&g), "native-g", StartKind::Fresh, None));
        adapter.emit(signal(Some(&aa), "native-a2", StartKind::Transition, None));
    }));
    n.adapter
        .emit(signal(Some(&a), "native-a1", StartKind::Transition, None));
    let s1 = n
        .pipes
        .binding(&a)
        .expect("re-bound to the earlier transition");
    assert_ne!(s1, sa);
    let ids = requirements(&log);
    assert!(ids.contains(&"SC-ID-128"), "{ids:?}");
    assert_eq!(told(&n, &a), None, "the new binding starts withheld");
    observed_attachment(&n, "harness-g", Some("native-g"));
    n.adapter
        .emit(signal(Some(&a), "native-a3", StartKind::Transition, None));
    let now = n.pipes.binding(&a).expect("re-bound");
    assert_ne!(now, s1);
    assert_eq!(told(&n, &a), Some(now));
}

fn count(ids: &[&str], id: &str) -> usize {
    ids.iter().filter(|r| **r == id).count()
}

/// PR #337 review B1 ([SC-ID-154]): a queued transition evicted from the heaviest holder
/// by the fair share still withholds the bound attachment its key attributes it to; the
/// holder's later transitions then move the attachment to a new session.
#[test]
fn a_queued_transition_evicted_by_the_fair_share_still_withholds() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 3,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let a = observed_attachment(&n, "harness-a", Some("native-a"));
    n.adapter
        .emit(signal(Some(&a), "native-a", StartKind::Fresh, None));
    let sa = n.pipes.binding(&a).expect("bound");
    let x = observed_attachment(&n, "harness-x", Some("native-x"));
    let f = carrier(&n, "harness-f");
    let log = binding_log(&n);
    let (adapter, aa) = (n.adapter.clone(), a.clone());
    during_next_decision(&n, move || {
        for i in 1..=3 {
            adapter.emit(signal(
                Some(&aa),
                &format!("native-a{i}"),
                StartKind::Transition,
                None,
            ));
        }
        // Full: `a`'s key holds 3, the newcomer's 0, so `a`'s oldest transition goes.
        adapter.emit(signal(Some(&f), "native-f", StartKind::Fresh, None));
    });
    n.adapter
        .emit(signal(Some(&x), "native-x", StartKind::Fresh, None));
    let ids = requirements(&log);
    assert_eq!(count(&ids, "SC-ID-128"), 1, "{ids:?}");
    let dropped = ids.iter().position(|r| *r == "SC-ID-128").unwrap();
    assert_eq!(ids.get(dropped + 1), Some(&"SC-ID-154"), "{ids:?}");
    let now = n
        .pipes
        .binding(&a)
        .expect("re-bound by the later transitions");
    assert_ne!(now, sa);
    assert_eq!(
        told(&n, &a),
        Some(now),
        "released when a later signal paired"
    );
}

/// PR #337 review B1 with PR #333 B3 ([SC-ID-154]): a queued transition that its own
/// holder replaces at the cap, after its carrier disconnected, still withholds by the key
/// observed when it was reported; it is not unpairable (no SC-ID-129).
#[test]
fn a_queued_transition_replaced_by_its_own_holder_after_disconnect_still_withholds() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 1,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let a = observed_attachment(&n, "harness-k", Some("native-k"));
    n.adapter
        .emit(signal(Some(&a), "native-k", StartKind::Fresh, None));
    let sk = n.pipes.binding(&a).expect("bound");
    let x = observed_attachment(&n, "harness-x", Some("native-x"));
    let c1 = carrier(&n, "harness-k");
    let c2 = carrier(&n, "harness-k");
    let log = binding_log(&n);
    let (adapter, pipes) = (n.adapter.clone(), n.pipes.clone());
    during_next_decision(&n, move || {
        adapter.emit(signal(Some(&c1), "native-k1", StartKind::Transition, None));
        pipes.disconnect(&c1);
        // Full: the key holds 1, the newcomer is the same key, so its own oldest goes.
        adapter.emit(signal(Some(&c2), "native-k2", StartKind::Transition, None));
        pipes.disconnect(&c2);
    });
    n.adapter
        .emit(signal(Some(&x), "native-x", StartKind::Fresh, None));
    let ids = requirements(&log);
    assert_eq!(count(&ids, "SC-ID-128"), 1, "{ids:?}");
    assert_eq!(count(&ids, "SC-ID-129"), 0, "{ids:?}");
    let dropped = ids.iter().position(|r| *r == "SC-ID-128").unwrap();
    assert_eq!(ids.get(dropped + 1), Some(&"SC-ID-154"), "{ids:?}");
    let now = n
        .pipes
        .binding(&a)
        .expect("re-bound by the second transition");
    assert_ne!(now, sk);
    assert_eq!(told(&n, &a), Some(now));
}

/// PR #337 review N4 ([SC-ID-154]): a held signal evicted at the cap after its key's
/// attachment was bound, before a pass re-paired it, withholds that attachment.
#[test]
fn an_evicted_held_signal_withholds_its_bound_attachment() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 3,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let c = carrier(&n, "harness-k");
    for i in 1..=3 {
        n.adapter.emit(signal(
            Some(&c),
            &format!("native-{i}"),
            StartKind::Fresh,
            None,
        ));
    }
    let g = carrier(&n, "harness-g");
    let f = carrier(&n, "harness-f");
    let log = binding_log(&n);
    let adapter = n.adapter.clone();
    // Runs when the first held signal has bound the new attachment, while the other two
    // are still held: a filler takes the free place, then a newcomer evicts the key's
    // oldest held signal, which pairs with the now-bound attachment.
    *n.adapter.on_set_binding.lock().unwrap() = Some(Box::new(move || {
        adapter.emit(signal(Some(&g), "native-g", StartKind::Fresh, None));
        adapter.emit(signal(Some(&f), "native-f", StartKind::Fresh, None));
    }));
    // Opening the key's attachment pairs the first held signal, which binds it.
    observed_attachment(&n, "harness-k", Some("native-1"));
    let ids = requirements(&log);
    assert_eq!(count(&ids, "SC-ID-128"), 1, "{ids:?}");
    let dropped = ids.iter().position(|r| *r == "SC-ID-128").unwrap();
    assert_eq!(ids.get(dropped + 1), Some(&"SC-ID-154"), "{ids:?}");
    // The remaining held signal (another N under the same key) is decided after, against
    // the binding the first one made; what it decides is §6.7.3's, not this test's.
}

/// PR #337 review N5: a key counts against its share under the adapter that reported it,
/// as it pairs; the same key under another adapter is another holder.
#[test]
fn a_key_is_shared_per_adapter() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 3,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config.clone());
    let other = TestAdapter::new();
    let other_id = n.pipes.add_adapter(other.clone());
    let k = carrier(&n, "harness-k");
    let g = carrier(&n, "harness-g");
    n.adapter
        .emit(signal(Some(&k), "native-k", StartKind::Fresh, None));
    n.adapter
        .emit(signal(Some(&g), "native-g1", StartKind::Fresh, None));
    n.adapter
        .emit(signal(Some(&g), "native-g2", StartKind::Fresh, None));
    let log = binding_log(&n);
    // The same key, through the other adapter: it holds nothing there, so the heaviest
    // holder (`g`, 2) pays, not this adapter's single `k` signal.
    *other.carrier_next.lock().unwrap() = true;
    let conn = Connection::accept(Default::default(), Default::default());
    let q = conn.handle().clone();
    n.pipes
        .connect_observed(other_id, conn, observed("harness-k"))
        .unwrap();
    other.emit(signal(Some(&q), "native-q", StartKind::Fresh, None));
    assert_eq!(requirements(&log), ["SC-ID-128"]);
    let ak = observed_attachment(&n, "harness-k", Some("native-k"));
    assert!(
        n.pipes.binding(&ak).is_some(),
        "this adapter's `k` signal was kept"
    );

    // The other way round: the other adapter holds the key's one held signal, and this
    // adapter's newcomer under the same key holds nothing here, so `g` pays again.
    let n = node(&bus, "device-b", config);
    let other = TestAdapter::new();
    let other_id = n.pipes.add_adapter(other.clone());
    *other.carrier_next.lock().unwrap() = true;
    let conn = Connection::accept(Default::default(), Default::default());
    let q = conn.handle().clone();
    n.pipes
        .connect_observed(other_id, conn, observed("harness-k"))
        .unwrap();
    other.emit(signal(Some(&q), "native-q", StartKind::Fresh, None));
    let g = carrier(&n, "harness-g");
    n.adapter
        .emit(signal(Some(&g), "native-g1", StartKind::Fresh, None));
    n.adapter
        .emit(signal(Some(&g), "native-g2", StartKind::Fresh, None));
    let log = binding_log(&n);
    let k = carrier(&n, "harness-k");
    n.adapter
        .emit(signal(Some(&k), "native-k", StartKind::Fresh, None));
    assert_eq!(requirements(&log), ["SC-ID-128"]);
    let conn = Connection::accept(Default::default(), Default::default());
    let aq = conn.handle().clone();
    n.pipes
        .connect_observed(other_id, conn, observed("harness-k"))
        .unwrap();
    assert!(
        n.pipes.binding(&aq).is_some(),
        "the other adapter's held signal was kept"
    );
}

/// [SC-ID-125], [SC-ID-129], [SC-ID-154]: with two candidate attachments the signal is
/// unpairable and fails closed; the observed key attributes it to the harness process
/// behind the bound one, so delivery to it and its send requests are withheld until a
/// later signal pairs with it.
#[test]
fn an_unpairable_signal_withholds_the_attachments_it_is_attributed_to() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let a = observed_attachment(&n, "harness-1", None);
    let sa = SessionId::from_random_octets([40; 16]);
    let record = n
        .pipes
        .device()
        .register(
            sa.clone(),
            Token::parse("test-harness").unwrap(),
            "native-x",
            "/work",
            SystemClock.now(),
        )
        .unwrap();
    n.pipes.bind(&a, &record, None).unwrap();
    let (c, sc) = session(&n, 41);
    local_grant(&n, &sc, &sa);
    local_grant(&n, &sa, &sc);
    let b = observed_attachment(&n, "harness-1", None);
    let log = binding_log(&n);
    n.adapter
        .emit(signal(Some(&b), "native-y", StartKind::Transition, None));
    assert_eq!(requirements(&log), ["SC-ID-129", "SC-ID-154"]);
    assert_eq!(n.pipes.binding(&a), Some(sa.clone()), "still registered");
    assert_eq!(told(&n, &a), None, "the adapter hands nothing off to it");
    let r = n.adapter.sink().send(request(&a, &sc, "from a"));
    assert_eq!(r.error(), Some(ErrorCode::Unauthorized));
    let calls = n.adapter.deliver_calls.load(Ordering::SeqCst);
    let (_, _, rx) = sent(n.adapter.sink().send(request(&c, &sa, "to a")));
    let receipt = rx.0.recv_timeout(WAIT).unwrap();
    assert_ne!(receipt.state(), DeliveryState::HandedToHarness);
    assert_eq!(n.adapter.deliver_calls.load(Ordering::SeqCst), calls);
    // The other candidate ends; the next signal pairs with `a` (case 1) and releases it.
    n.adapter
        .emit(AdapterEvent::AttachmentClosed { attachment: b });
    n.adapter
        .emit(signal(Some(&a), "native-x", StartKind::Transition, None));
    assert_eq!(told(&n, &a), Some(sa.clone()));
    sent(n.adapter.sink().send(request(&a, &sc, "from a again")));
}

/// Runs `f` on the adapter's thread at the next `capabilities` call, which comes while a
/// binding decision holds the binding turn (`Pipelines::bind`'s declaration step).
fn during_next_decision(n: &Node, f: impl FnOnce() + Send + 'static) {
    *n.adapter.on_capabilities.lock().unwrap() = Some(Box::new(f));
}

/// PR #333 review B1 ([SC-ID-123]): an attachment that opens while another decision holds
/// the binding turn still pairs with the signal held for it; the holder passes again.
#[test]
fn a_held_signal_pairs_with_an_attachment_opened_during_another_decision() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let log = binding_log(&n);
    let c = carrier(&n, "harness-k");
    n.adapter
        .emit(signal(Some(&c), "native-k", StartKind::Fresh, None));
    let x = observed_attachment(&n, "harness-x", None);
    let opened: Arc<Mutex<Option<Attachment>>> = Arc::default();
    let (pipes, id, slot) = (n.pipes.clone(), n.adapter_id, opened.clone());
    during_next_decision(&n, move || {
        let conn = Connection::accept(Default::default(), Default::default());
        *slot.lock().unwrap() = Some(conn.handle().clone());
        pipes
            .connect_observed(id, conn, observed("harness-k"))
            .unwrap();
    });
    n.adapter
        .emit(signal(Some(&x), "native-x", StartKind::Fresh, None));
    let k = opened
        .lock()
        .unwrap()
        .clone()
        .expect("opened during the decision");
    assert!(n.pipes.binding(&x).is_some());
    assert!(n.pipes.binding(&k).is_some(), "the held signal paired");
    assert_eq!(n.pipes.next_native_signal_expiry(), None);
    assert!(!requirements(&log).contains(&"SC-ID-128"), "{log:?}");
}

/// PR #333 review N3: a signal reported while another decision holds the binding turn is
/// decided once that pass ends (the post-pass re-check), not left on the queue.
#[test]
fn a_signal_reported_during_another_decision_is_decided_after_it() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let x = observed_attachment(&n, "harness-x", None);
    let z = observed_attachment(&n, "harness-z", None);
    let (adapter, zz) = (n.adapter.clone(), z.clone());
    during_next_decision(&n, move || {
        adapter.emit(signal(Some(&zz), "native-z", StartKind::Fresh, None));
    });
    n.adapter
        .emit(signal(Some(&x), "native-x", StartKind::Fresh, None));
    assert!(n.pipes.binding(&x).is_some());
    assert!(n.pipes.binding(&z).is_some(), "decided by the re-check");
}

/// PR #333 review B2 ([SC-ID-154]): a signal dropped at `max_pending_signals` that its
/// observed key attributes to a bound attachment withholds delivery to it.
#[test]
fn a_signal_dropped_at_the_bound_still_withholds_its_attachment() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_pending_signals: 1,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let a = observed_attachment(&n, "harness-a", Some("native-a"));
    n.adapter
        .emit(signal(Some(&a), "native-a", StartKind::Fresh, None));
    let sa = n.pipes.binding(&a).expect("bound");
    let log = binding_log(&n);
    let y = observed_attachment(&n, "harness-y", None);
    let c = carrier(&n, "harness-9");
    let (adapter, aa) = (n.adapter.clone(), a.clone());
    during_next_decision(&n, move || {
        // The queue takes the first; the second, `a`'s transition, finds it full.
        adapter.emit(signal(Some(&c), "native-9", StartKind::Fresh, None));
        adapter.emit(signal(Some(&aa), "native-a2", StartKind::Transition, None));
    });
    n.adapter
        .emit(signal(Some(&y), "native-y", StartKind::Fresh, None));
    let ids = requirements(&log);
    assert!(ids.contains(&"SC-ID-128"), "{ids:?}");
    assert!(ids.contains(&"SC-ID-154"), "{ids:?}");
    assert_eq!(told(&n, &a), None, "withheld");
    assert_eq!(
        n.pipes.binding(&a),
        Some(sa),
        "still registered, not re-bound"
    );
}

/// PR #333 review N1: `disconnect` frees the place of any connection, a signal carrier
/// included, so `max_connections` is not used up by connections that ended.
#[test]
fn disconnected_connections_free_their_place() {
    let bus = Bus::default();
    let config = PipelineConfig {
        max_connections: 2,
        ..PipelineConfig::default()
    };
    let n = node(&bus, "device-a", config);
    let c1 = carrier(&n, "harness-1");
    let _c2 = carrier(&n, "harness-2");
    let third = Connection::accept(Default::default(), Default::default());
    assert_eq!(
        n.pipes.connect(n.adapter_id, third),
        Err(PipelineError::TooManyConnections)
    );
    n.pipes.disconnect(&c1);
    n.pipes.disconnect(&c1);
    let again = Connection::accept(Default::default(), Default::default());
    assert_eq!(n.pipes.connect(n.adapter_id, again), Ok(()));
    // A signal on the ended carrier has no observed key any more: it fails closed.
    let log = binding_log(&n);
    n.adapter
        .emit(signal(Some(&c1), "native-1", StartKind::Fresh, None));
    assert_eq!(requirements(&log), ["SC-ID-129"]);
}

/// PR #333 review N1 ([SC-ID-155]): an attachment's connection ending ends its binding,
/// as `attachment-closed` does; the adapter's later `attachment-closed` changes nothing.
#[test]
fn a_disconnected_attachment_is_deregistered() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let a = observed_attachment(&n, "harness-1", Some("native-x"));
    n.adapter
        .emit(signal(Some(&a), "native-x", StartKind::Fresh, None));
    let sa = n.pipes.binding(&a).expect("bound");
    n.pipes.disconnect(&a);
    assert_eq!(n.pipes.binding(&a), None);
    assert_eq!(told(&n, &a), None);
    n.pipes.with_engine(|e| assert_eq!(e.binding(&sa), None));
    n.adapter.emit(AdapterEvent::AttachmentClosed {
        attachment: a.clone(),
    });
    // A new attachment may now take the same native id (no stale duplicate).
    let b = observed_attachment(&n, "harness-2", Some("native-x"));
    n.adapter
        .emit(signal(Some(&b), "native-x", StartKind::Fresh, None));
    assert!(n.pipes.binding(&b).is_some());
}

/// PR #333 re-review B3 ([SC-ID-121], [SC-ID-125]): a hook that sends a fresh signal and
/// disconnects before the signal's turn still pairs by the key observed when it was
/// reported, and binds.
#[test]
fn a_fresh_signal_pairs_after_its_carrier_disconnects() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let a = observed_attachment(&n, "harness-k", Some("native-n"));
    let c = carrier(&n, "harness-k");
    let x = observed_attachment(&n, "harness-x", None);
    let log = binding_log(&n);
    let (adapter, pipes) = (n.adapter.clone(), n.pipes.clone());
    during_next_decision(&n, move || {
        adapter.emit(signal(Some(&c), "native-n", StartKind::Fresh, None));
        pipes.disconnect(&c);
    });
    n.adapter
        .emit(signal(Some(&x), "native-x", StartKind::Fresh, None));
    assert!(n.pipes.binding(&a).is_some(), "{:?}", requirements(&log));
    assert!(!requirements(&log).contains(&"SC-ID-129"));
}

/// PR #333 re-review B3 ([SC-ID-140], [SC-ID-150]): a transition from N to N' whose hook
/// disconnects before its turn still moves the attachment to a new session, so nothing
/// keeps delivering to the session it left.
#[test]
fn a_transition_moves_the_attachment_after_its_carrier_disconnects() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let a = observed_attachment(&n, "harness-k", Some("native-n"));
    let c = carrier(&n, "harness-k");
    n.adapter
        .emit(signal(Some(&c), "native-n", StartKind::Fresh, None));
    let old = n.pipes.binding(&a).expect("bound");
    let x = observed_attachment(&n, "harness-x", None);
    let c2 = carrier(&n, "harness-k");
    let (adapter, pipes) = (n.adapter.clone(), n.pipes.clone());
    during_next_decision(&n, move || {
        adapter.emit(signal(Some(&c2), "native-n2", StartKind::Transition, None));
        pipes.disconnect(&c2);
    });
    n.adapter
        .emit(signal(Some(&x), "native-x", StartKind::Fresh, None));
    let new = n.pipes.binding(&a).expect("moved");
    assert_ne!(new, old);
    assert_eq!(told(&n, &a), Some(new));
    n.pipes
        .with_engine(|e| assert_eq!(e.binding(&old), None, "the old session ended"));
}

/// PR #333 re-review N9 ([SC-ID-155]): an attachment whose connection ends while a bind
/// for it is under way registers no session. `disconnect` marks it closed under the lock
/// first, and the bind's locked re-check refuses a closed attachment, so whichever of the
/// two takes the lock first, no session outlives the attachment. This test runs the
/// disconnect inside the bind, between its first check and its locked re-check.
#[test]
fn a_disconnect_during_a_bind_leaves_no_session() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let log = binding_log(&n);
    let x = observed_attachment(&n, "harness-x", None);
    let before = n.pipes.with_engine(|e| e.bindings().count());
    let (pipes, xx) = (n.pipes.clone(), x.clone());
    during_next_decision(&n, move || pipes.disconnect(&xx));
    n.adapter
        .emit(signal(Some(&x), "native-x", StartKind::Fresh, None));
    assert_eq!(n.pipes.binding(&x), None);
    assert_eq!(told(&n, &x), None);
    assert_eq!(n.pipes.with_engine(|e| e.bindings().count()), before);
    let failed = log
        .entries()
        .iter()
        .any(|e| e.record.requirement == "SC-ID-009" && e.result == BindingResult::FailedClosed);
    assert!(failed, "{:?}", requirements(&log));
}

// Interfaces 0.4 local identity: attachment ownership and binding captured under one lock.
#[test]
fn identity_returns_only_the_current_attachment_binding_and_public_digest() {
    use oac_core::adapter::{IdentityRequest, IdentityRequestResult, LocalIdentity};
    let n = node(
        &Bus::default(),
        "identity-device",
        PipelineConfig::default(),
    );
    let (a, sa) = session(&n, 41);
    let (b, sb) = session(&n, 42);
    let get = |a: &Attachment| {
        n.adapter.sink().identity(IdentityRequest {
            attachment: a.clone(),
        })
    };
    let expected = LocalIdentity {
        session_id: sa.clone(),
        device_fingerprint: n.pipes.device().key_id().clone(),
    };
    assert_eq!(
        get(&a),
        IdentityRequestResult::LocalIdentity(expected.clone())
    );
    let LocalIdentity {
        session_id,
        device_fingerprint,
    } = expected; // exhaustive: no key members
    assert_eq!(session_id, sa);
    assert_eq!(device_fingerprint.as_str().len(), 64);
    assert!(
        device_fingerprint
            .as_str()
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    assert!(
        matches!(get(&b), IdentityRequestResult::LocalIdentity(LocalIdentity { session_id, .. }) if session_id == sb)
    );
    n.pipes.unbind(&a);
    assert_eq!(
        get(&a),
        IdentityRequestResult::Refused {
            error: ErrorCode::Unauthorized,
            pairing_value: None,
        }
    );
    n.adapter.emit(AdapterEvent::AttachmentClosed {
        attachment: b.clone(),
    });
    assert_eq!(
        get(&b),
        IdentityRequestResult::Refused {
            error: ErrorCode::Unauthorized,
            pairing_value: None,
        }
    );
    let unknown = Connection::accept(Default::default(), Default::default());
    assert_eq!(
        get(unknown.handle()),
        IdentityRequestResult::Refused {
            error: ErrorCode::Unauthorized,
            pairing_value: None,
        }
    );
}

#[test]
fn identity_cannot_select_another_adapters_attachment() {
    use oac_core::adapter::{IdentityRequest, IdentityRequestResult};
    let n = node(
        &Bus::default(),
        "identity-device",
        PipelineConfig::default(),
    );
    let (a, _) = session(&n, 43);
    let other = TestAdapter::new();
    n.pipes.add_adapter(other.clone());
    assert_eq!(
        other.sink().identity(IdentityRequest { attachment: a }),
        IdentityRequestResult::Refused {
            error: ErrorCode::Unauthorized,
            pairing_value: None,
        }
    );
}

#[test]
fn identity_racing_unbind_has_no_stale_fallback() {
    use oac_core::adapter::{IdentityRequest, IdentityRequestResult, LocalIdentity};
    let n = node(
        &Bus::default(),
        "identity-device",
        PipelineConfig::default(),
    );
    let (a, sa) = session(&n, 44);
    let sink = n.adapter.sink();
    let reader = sink.clone();
    let attachment = a.clone();
    let fingerprint = n.pipes.device().key_id().clone();
    let t = std::thread::spawn(move || {
        for _ in 0..100 {
            match reader.identity(IdentityRequest {
                attachment: attachment.clone(),
            }) {
                IdentityRequestResult::LocalIdentity(LocalIdentity {
                    session_id,
                    device_fingerprint,
                }) => {
                    assert_eq!(session_id, sa);
                    assert_eq!(device_fingerprint, fingerprint);
                }
                IdentityRequestResult::Refused { error, .. } => {
                    assert_eq!(error, ErrorCode::Unauthorized)
                }
            }
        }
    });
    n.pipes.unbind(&a);
    t.join().unwrap();
    assert_eq!(
        sink.identity(IdentityRequest { attachment: a }),
        IdentityRequestResult::Refused {
            error: ErrorCode::Unauthorized,
            pairing_value: None,
        }
    );
}

#[test]
fn a_connection_cannot_exceed_the_adapter_shutdown_budget() {
    let n = node(&Bus::default(), "budget-device", PipelineConfig::default());
    n.adapter.shutdown_bound.store(99, Ordering::SeqCst);
    let c = Connection::accept(Default::default(), Default::default());
    let h = c.handle().clone();
    assert!(matches!(
        n.pipes.connect(n.adapter_id, c),
        Err(PipelineError::ClosureBound)
    ));
    assert!(h.is_closed());
    assert!(
        n.adapter.bound.lock().unwrap().is_empty(),
        "not supplied to the adapter"
    );
}

#[test]
fn identity_refuses_a_withheld_or_closed_attachment_and_uses_a_new_binding() {
    use oac_core::adapter::{IdentityRequest, IdentityRequestResult, LocalIdentity};
    let n = node(
        &Bus::default(),
        "identity-device",
        PipelineConfig::default(),
    );
    let a = observed_attachment(&n, "harness-identity", None);
    let s = SessionId::from_random_octets([50; 16]);
    let record = n
        .pipes
        .device()
        .register(
            s,
            Token::parse("test-harness").unwrap(),
            "native-x",
            "/work",
            SystemClock.now(),
        )
        .unwrap();
    n.pipes.bind(&a, &record, None).unwrap();
    let b = observed_attachment(&n, "harness-identity", None);
    n.adapter
        .emit(signal(Some(&b), "native-y", StartKind::Transition, None));
    let get = || {
        n.adapter.sink().identity(IdentityRequest {
            attachment: a.clone(),
        })
    };
    assert_eq!(
        get(),
        IdentityRequestResult::Refused {
            error: ErrorCode::Unauthorized,
            pairing_value: None,
        }
    );
    n.adapter
        .emit(AdapterEvent::AttachmentClosed { attachment: b });
    n.adapter
        .emit(signal(Some(&a), "native-new", StartKind::Transition, None));
    let current = n.pipes.binding(&a).unwrap();
    assert!(
        matches!(get(), IdentityRequestResult::LocalIdentity(LocalIdentity { session_id, .. }) if session_id == current)
    );
    a.close(); // before the eventual attachment-closed callback reaches core
    assert_eq!(
        get(),
        IdentityRequestResult::Refused {
            error: ErrorCode::Unauthorized,
            pairing_value: None,
        }
    );
}

#[test]
fn identity_refuses_when_the_core_no_longer_serves_the_attachment() {
    use oac_core::adapter::{IdentityRequest, IdentityRequestResult};
    let n = node(
        &Bus::default(),
        "identity-device",
        PipelineConfig::default(),
    );
    let (a, _) = session(&n, 51);
    let sink = n.adapter.sink();
    drop(n);
    assert_eq!(
        sink.identity(IdentityRequest { attachment: a }),
        IdentityRequestResult::Refused {
            error: ErrorCode::Unauthorized,
            pairing_value: None,
        }
    );
}

/// An adapter that reports its events through the cooperative local handler, as the Claude
/// adapter does in production: the core takes each event on a worker of its own, after the
/// report returned.
struct LocalReports {
    inner: Arc<TestAdapter>,
    scope: Mutex<oac_core::local_call::CallAdmission>,
}

impl ProviderAdapter for LocalReports {
    fn local_calls(&self) -> bool {
        true
    }
    fn connection_limit(&self) -> usize {
        4
    }
    fn configure_local_calls(&self, scope: oac_core::local_call::CallAdmission, _: u64, _: u64) {
        *self.scope.lock().unwrap() = scope;
    }
    fn watch_local_attachments(&self, handler: oac_core::adapter::LocalEventHandler) {
        let scope = self.scope.lock().unwrap().clone();
        self.inner.watch_attachments(Arc::new(move |event| {
            let attachment = match &event {
                AdapterEvent::AttachmentOpened { attachment, .. } => Some(attachment.clone()),
                _ => None,
            };
            let mut call = scope.call(attachment, 1000);
            let closing = matches!(event, AdapterEvent::AttachmentClosed { .. });
            let result = handler(&mut call, event);
            // A report at shutdown is turned away; every other report is taken.
            assert!(
                closing || matches!(result, oac_core::local_call::LocalResult::Completed(())),
                "{result:?}"
            );
        }));
    }
    fn take_connection(&self, c: Connection) {
        self.inner.take_connection(c);
    }
    fn watch_attachments(&self, h: AdapterEventHandler) {
        self.inner.watch_attachments(h);
    }
    fn set_binding(&self, a: &Attachment, s: Option<SessionId>) {
        self.inner.set_binding(a, s);
    }
    fn capabilities(&self, a: &Attachment) -> AdapterCapabilities {
        self.inner.capabilities(a)
    }
    fn deliver(&self, h: HandOff) -> HandOffOutcome {
        self.inner.deliver(h)
    }
    fn accept_requests(&self, sink: Arc<dyn RequestSink>) {
        self.inner.accept_requests(sink);
    }
    fn health(&self) -> HealthStatus {
        self.inner.health()
    }
    fn shutdown(&self) {
        self.scope.lock().unwrap().stop();
        self.inner.shutdown();
    }
}

/// The B3 rule, for an adapter that reports through the local handler: a connection that only
/// carries a native signal (a hook that sends and exits) may end the moment the report returns,
/// and the signal still pairs by the key observed when it was reported. The key and the
/// signal's place in arrival order are taken at report time, not on the worker that decides it
/// later, so ending the connection at once ([`Pipelines::disconnect`]) does not lose the key
/// (SC-ID-129, nothing bound). Arrival order has its own test below.
#[test]
fn a_local_native_signal_keeps_its_pairing_key_when_its_connection_ends_at_once() {
    for round in 0..100 {
        let bus = Bus::default();
        let mut n = node(&bus, "b3", PipelineConfig::default());
        let log = binding_log(&n);
        let reports = Arc::new(LocalReports {
            inner: TestAdapter::new(),
            scope: Mutex::default(),
        });
        n.adapter = reports.inner.clone();
        n.adapter_id = n.pipes.add_adapter(reports.clone());
        let a = observed_attachment(&n, "harness-1", None);
        let c = carrier(&n, "harness-1");
        n.adapter
            .emit(signal(Some(&c), "native-x", StartKind::Fresh, None));
        // The daemon saw the hook handler exit and ends its connection at once.
        n.pipes.disconnect(&c);
        let end = Instant::now() + WAIT;
        while n.pipes.binding(&a).is_none() || log.entries().is_empty() {
            assert!(
                Instant::now() < end,
                "round {round}: not bound; log {:?}",
                log.entries()
            );
            std::thread::yield_now();
        }
        assert_eq!(
            requirements(&log),
            ["SC-ID-141"],
            "round {round}: bound without a finding (only the missing cross-check diagnostic)"
        );
        n.pipes.shutdown();
    }
}

/// [SC-ID-123]: signals are decided in the order they were reported, also for an adapter that
/// reports through the local handler, where the core takes each event on a worker of its own
/// and workers run in any order. An older signal queued behind a newer one would pair after
/// it and bind the attachment back to the older N (SC-ID-151, C4 §6). One attachment, two
/// carriers of its key: a `Fresh` signal for `old` and then a `Transition` for `new`, reported
/// back to back from one thread, must leave the attachment on `new`. Told apart from outside:
/// a third signal, for `new` from another attachment and so queued after both, is refused
/// under case 2 (SC-ID-132) when `new` is the bound N, and binds when `old` is.
#[test]
fn local_native_signals_are_decided_in_the_order_they_were_reported() {
    for round in 0..200 {
        let bus = Bus::default();
        let mut n = node(&bus, "order", PipelineConfig::default());
        let log = binding_log(&n);
        let reports = Arc::new(LocalReports {
            inner: TestAdapter::new(),
            scope: Mutex::default(),
        });
        n.adapter = reports.inner.clone();
        n.adapter_id = n.pipes.add_adapter(reports.clone());
        let a = observed_attachment(&n, "harness-1", None);
        let first = carrier(&n, "harness-1");
        let second = carrier(&n, "harness-1");
        let other = observed_attachment(&n, "harness-2", None);
        n.adapter
            .emit(signal(Some(&first), "native-old", StartKind::Fresh, None));
        n.adapter.emit(signal(
            Some(&second),
            "native-new",
            StartKind::Transition,
            None,
        ));
        n.adapter
            .emit(signal(Some(&other), "native-new", StartKind::Fresh, None));
        let end = Instant::now() + WAIT;
        while n.pipes.binding(&other).is_none() && !requirements(&log).contains(&"SC-ID-132") {
            assert!(
                Instant::now() < end,
                "round {round}: no decision for the last signal; log {:?}",
                log.entries()
            );
            std::thread::yield_now();
        }
        assert_eq!(
            n.pipes.binding(&other),
            None,
            "round {round}: `native-new` was free, so the older signal was decided after it; log {:?}",
            log.entries()
        );
        assert!(
            requirements(&log).contains(&"SC-ID-132"),
            "round {round}: {:?}",
            log.entries()
        );
        assert!(n.pipes.binding(&a).is_some(), "round {round}");
        n.pipes.shutdown();
    }
}

// ---- the status snapshot (G10, #71) ---------------------------------------------------------

/// The snapshot reports the transport's and each adapter's health as each answered, this
/// device's own sessions with their agreed capabilities, and what the core holds.
#[test]
fn status_reports_health_sessions_and_counts() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let empty = n.pipes.status();
    assert_eq!(empty.transport, HealthStatus::new(HealthState::Healthy));
    assert_eq!(empty.adapters.len(), 1);
    assert_eq!(empty.adapters[0].adapter, n.adapter_id);
    assert_eq!(
        empty.adapters[0].health,
        HealthStatus::new(HealthState::Healthy)
    );
    assert!(empty.sessions.is_empty());
    assert_eq!(empty.counts, StatusCounts::default());

    *n.adapter.health.lock().unwrap() = HealthStatus::with_detail(HealthState::Degraded, "slow");
    let degraded = n.pipes.status();
    assert_eq!(degraded.adapters[0].health.state, HealthState::Degraded);
    assert_eq!(degraded.adapters[0].health.detail.as_deref(), Some("slow"));

    let (_a, sid) = session(&n, 7);
    let (_b, other) = session(&n, 8);
    let conn = Connection::accept(Default::default(), Default::default());
    n.pipes.connect(n.adapter_id, conn).unwrap();
    local_grant(&n, &sid, &other);
    let s = n.pipes.status();
    assert_eq!(s.sessions.len(), 2);
    let mine = s.sessions.iter().find(|x| x.session_id == sid).unwrap();
    assert_eq!(mine.harness_label, Token::parse("test-harness"));
    assert_eq!(mine.state, SessionState::Online);
    let caps = mine.capabilities.as_ref().expect("agreed capabilities");
    assert!(caps.active_inbound);
    assert_eq!(caps.extension, oac_core::ids::EXTENSION_ID_V0);
    assert_eq!(caps.major, 0);
    assert_eq!(s.counts.open_connections, 3);
    assert_eq!(s.counts.unbound_attachments, 1);
    assert_eq!(s.adapters[0].unbound_attachments, 1);
    assert_eq!(
        s.adapters[0].unbound_attachments,
        s.counts.unbound_attachments
    );
    assert_eq!(s.counts.pending_signals, 0);
    assert_eq!(s.counts.held_signals, 0);
    assert_eq!(
        s.counts.trusted_devices, 0,
        "this device's own key is not counted"
    );
    assert_eq!(s.counts.grants, 1);
    assert_eq!(s.counts.discarded_frames, 0);
}

/// A session ending leaves the list, and a held native signal is counted.
#[test]
fn status_counts_held_signals_and_forgets_ended_sessions() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (a, _sid) = session(&n, 3);
    assert_eq!(n.pipes.status().sessions.len(), 1);
    n.pipes.unbind(&a);
    assert!(n.pipes.status().sessions.is_empty());
    // A signal from a carrier, before its attachment opens, is held for the window.
    let c = carrier(&n, "harness-1");
    n.adapter
        .emit(signal(Some(&c), "native-x", StartKind::Fresh, None));
    let held = n.pipes.status();
    assert_eq!(held.counts.held_signals, 1);
    // Its attachment opens: the signal pairs and binds, and nothing is held.
    let _ = observed_attachment(&n, "harness-1", Some("native-x"));
    let paired = n.pipes.status();
    assert_eq!(paired.counts.held_signals, 0);
    assert_eq!(paired.sessions.len(), 1);
}

/// [IFC-TYP-020], [IFC-TYP-092]: the snapshot of a device with a bound session names
/// neither the harness-native id nor the working directory it was bound with.
#[test]
fn status_names_no_native_id_and_no_working_directory() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let a = observed_attachment(&n, "harness-1", Some("native-secret-id"));
    n.adapter
        .emit(signal(Some(&a), "native-secret-id", StartKind::Fresh, None));
    let bound = n.pipes.binding(&a).expect("bound");
    let status = n.pipes.status();
    assert_eq!(status.sessions.len(), 1);
    assert_eq!(status.sessions[0].session_id, bound);
    let text = format!("{status:?}");
    for forbidden in ["native-secret-id", "/work"] {
        assert!(!text.contains(forbidden), "{forbidden:?} in {text}");
    }
}

/// The core asks the transport and the adapters with no lock held: a `health` that calls
/// back into the core is answered. (A held lock would block it for ever; the call runs on a
/// thread and is waited for with a bound.)
#[test]
fn status_asks_the_adapters_with_no_core_lock_held() {
    let bus = Bus::default();
    let n = node(&bus, "device-a", PipelineConfig::default());
    let (att, sid) = session(&n, 5);
    let inside = n.pipes.clone();
    let sid_inside = sid.clone();
    let asked = Arc::new(AtomicUsize::new(0));
    let counted = asked.clone();
    *n.adapter.on_health.lock().unwrap() = Some(Arc::new(move || {
        // Both take the core's lock.
        assert_eq!(inside.binding(&att), Some(sid_inside.clone()));
        inside.with_engine(|e| e.grants().len());
        counted.fetch_add(1, Ordering::SeqCst);
    }));
    let pipes = n.pipes.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(pipes.status());
    });
    let status = rx
        .recv_timeout(WAIT)
        .expect("status returned: no lock was held across the adapter's health");
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(status.sessions[0].session_id, sid);
}

// ---- issued-value pairing (G8 slice S2; `spec/bindings/mcp.md` section 4.5) ------------------

/// A monotonic clock a test moves by hand.
#[derive(Clone)]
struct Stepped(Arc<Mutex<Instant>>);

impl Stepped {
    fn new() -> Stepped {
        Stepped(Arc::new(Mutex::new(Instant::now())))
    }

    fn clock(&self) -> oac_core::pipeline::MonotonicClock {
        let t = self.0.clone();
        Arc::new(move || *t.lock().unwrap())
    }

    fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

/// A node whose monotonic clock `Stepped` moves, with windows of 10 seconds.
fn pairing_node(bus: &Bus) -> (Node, Stepped) {
    let t = Stepped::new();
    let config = PipelineConfig {
        pairing_window: Duration::from_secs(10),
        ..PipelineConfig::default()
    };
    let n = node_mono(bus, "device-a", config, Arc::new(SystemClock), t.clock());
    (n, t)
}

/// A connection the daemon designates as the adapter's carrier, observed with `key`.
fn designated_carrier(n: &Node, key: &str) -> Attachment {
    *n.adapter.carrier_next.lock().unwrap() = true;
    let conn = Connection::accept(Default::default(), Default::default());
    let c = conn.handle().clone();
    n.pipes
        .connect_carrier(n.adapter_id, conn, observed(key))
        .unwrap();
    c
}

fn reveal(on: &Attachment, thread: &str, value: &str) -> AdapterEvent {
    AdapterEvent::NativeSignal(NativeSignal {
        native_id: thread.into(),
        start_kind: Some(StartKind::Fresh),
        cross_check: None,
        connection: Some(on.clone()),
        revealed: Some(value.into()),
    })
}

fn stranger() -> SessionId {
    SessionId::from_random_octets([0x77; 16])
}

/// The pairing value of the refusal of a send from `a`; the refusal is `unauthorized`.
fn pairing_refusal(n: &Node, a: &Attachment) -> Option<String> {
    let r = n.adapter.sink().send(request(a, &stranger(), "x"));
    assert_eq!(r.error(), Some(ErrorCode::Unauthorized), "{r:?}");
    assert_eq!(r.outcome(), "refused");
    r.pairing_value().map(str::to_owned)
}

fn issued_log(log: &MemoryBindingLog) -> Vec<&'static str> {
    requirements(log)
        .into_iter()
        .filter(|r| r.starts_with("MCPB-") || r.starts_with("IFC-"))
        .collect()
}

fn identity_of(n: &Node, a: &Attachment) -> oac_core::adapter::IdentityRequestResult {
    n.adapter
        .sink()
        .identity(oac_core::adapter::IdentityRequest {
            attachment: a.clone(),
        })
}

fn discovery_of(n: &Node, a: &Attachment) -> DiscoveryRequestResult {
    n.adapter.sink().discover(DiscoveryRequest {
        attachment: a.clone(),
    })
}

/// [MCPB-ATT-005], [IFC-ADP-090], [MCPB-ATT-025]: an unserved attachment of a carrier's
/// process gets one value, of the specified form, in every refusal of the window (send,
/// discovery and identity alike), whatever the call asked for; another attachment gets its
/// own.
#[test]
fn a_pairable_attachment_is_refused_with_one_value_per_window() {
    let bus = Bus::default();
    let (n, _t) = pairing_node(&bus);
    designated_carrier(&n, "codex-1");
    let a = observed_attachment(&n, "codex-1", None);
    let b = observed_attachment(&n, "codex-1", None);
    let v = pairing_refusal(&n, &a).expect("a value");
    let digits = v.strip_prefix("oac-pair-").expect("prefix");
    assert_eq!(digits.len(), 32, "{v}");
    assert!(
        digits
            .bytes()
            .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')),
        "{v}"
    );
    // The same value however the call differs (MCPB-TOOL-017).
    let other = n
        .adapter
        .sink()
        .send(request(&a, &SessionId::from_random_octets([1; 16]), "y"));
    assert_eq!(other.pairing_value(), Some(v.as_str()));
    assert_eq!(discovery_of(&n, &a).pairing_value(), Some(v.as_str()));
    assert_eq!(identity_of(&n, &a).pairing_value(), Some(v.as_str()));
    let vb = pairing_refusal(&n, &b).expect("a value");
    assert_ne!(vb, v);
}

/// [MCPB-ATT-022]: no value where the connection cannot be paired: no carrier, no observed
/// key, a key that is no carrier's, or a carrier that has gone. The refusal is the plain one.
#[test]
fn no_value_is_issued_where_pairing_is_impossible() {
    let bus = Bus::default();
    let (n, _t) = pairing_node(&bus);
    let a = observed_attachment(&n, "codex-1", None);
    assert_eq!(pairing_refusal(&n, &a), None, "no carrier registered");
    // A connection observed with the same key but never designated is no carrier.
    let _undesignated = carrier(&n, "codex-1");
    assert_eq!(
        pairing_refusal(&n, &a),
        None,
        "an undesignated connection is no carrier"
    );
    let c = designated_carrier(&n, "codex-1");
    assert!(pairing_refusal(&n, &a).is_some());
    let other_key = observed_attachment(&n, "codex-2", None);
    assert_eq!(
        pairing_refusal(&n, &other_key),
        None,
        "another process than the carrier's"
    );
    let conn = Connection::accept(Default::default(), Default::default());
    let no_key = conn.handle().clone();
    n.pipes.connect(n.adapter_id, conn).unwrap();
    assert_eq!(
        pairing_refusal(&n, &no_key),
        None,
        "no operating-system key"
    );
    // The same facts for discovery and identity.
    assert_eq!(discovery_of(&n, &other_key).pairing_value(), None);
    assert_eq!(identity_of(&n, &other_key).pairing_value(), None);
    assert_eq!(identity_of(&n, &no_key).pairing_value(), None);
    n.pipes.disconnect(&c);
    assert_eq!(pairing_refusal(&n, &a), None, "the carrier is gone");
}

/// A carrier whose observation has no key matches nothing, even an attachment with no key.
#[test]
fn a_carrier_without_a_key_pairs_nothing() {
    let bus = Bus::default();
    let (n, _t) = pairing_node(&bus);
    *n.adapter.carrier_next.lock().unwrap() = true;
    let conn = Connection::accept(Default::default(), Default::default());
    n.pipes
        .connect_carrier(n.adapter_id, conn, PeerObservation::none())
        .unwrap();
    let conn = Connection::accept(Default::default(), Default::default());
    let a = conn.handle().clone();
    n.pipes.connect(n.adapter_id, conn).unwrap();
    assert_eq!(pairing_refusal(&n, &a), None);
}

/// [MCPB-ATT-019], [MCPB-ATT-026], [MCPB-ATT-023]: a window ends within the bound and its
/// value is never repeated; at most three windows open until a reveal pairs, then none.
#[test]
fn windows_end_values_are_not_reused_and_three_is_the_most() {
    let bus = Bus::default();
    let (n, t) = pairing_node(&bus);
    designated_carrier(&n, "codex-1");
    let a = observed_attachment(&n, "codex-1", None);
    let v1 = pairing_refusal(&n, &a).unwrap();
    t.advance(Duration::from_secs(9));
    assert_eq!(pairing_refusal(&n, &a).as_deref(), Some(v1.as_str()));
    t.advance(Duration::from_secs(1));
    let v2 = pairing_refusal(&n, &a).unwrap();
    assert_ne!(v1, v2);
    t.advance(Duration::from_secs(10));
    let v3 = pairing_refusal(&n, &a).unwrap();
    assert!(v3 != v1 && v3 != v2);
    t.advance(Duration::from_secs(10));
    assert_eq!(pairing_refusal(&n, &a), None, "a fourth window");
    t.advance(Duration::from_secs(1000));
    assert_eq!(
        pairing_refusal(&n, &a),
        None,
        "for as long as the connection lasts"
    );
    // A new connection starts again.
    let b = observed_attachment(&n, "codex-1", None);
    assert!(pairing_refusal(&n, &b).is_some());
}

/// The configured window is cut to 60 seconds ([MCPB-ATT-019]).
#[test]
fn a_configured_window_longer_than_sixty_seconds_is_cut() {
    let bus = Bus::default();
    let t = Stepped::new();
    let config = PipelineConfig {
        pairing_window: Duration::from_secs(3600),
        ..PipelineConfig::default()
    };
    let n = node_mono(&bus, "device-a", config, Arc::new(SystemClock), t.clock());
    let c = designated_carrier(&n, "codex-1");
    let log = binding_log(&n);
    let a = observed_attachment(&n, "codex-1", None);
    let v = pairing_refusal(&n, &a).unwrap();
    t.advance(Duration::from_secs(61));
    n.adapter.emit(reveal(&c, "T1", &v));
    assert_eq!(n.pipes.binding(&a), None);
    assert_eq!(issued_log(&log), ["MCPB-ATT-009"]);
}

/// [SC-ID-125], [MCPB-ATT-007]: the attachments of one Codex process share its key, so the
/// key alone leaves several candidates. A reveal narrows them to the attachment its value was
/// issued on, and each thread binds the attachment of its own value, whatever the order.
#[test]
fn a_reveal_pairs_one_of_several_attachments_that_share_a_key() {
    let bus = Bus::default();
    let (n, _t) = pairing_node(&bus);
    let c = designated_carrier(&n, "codex-1");
    let log = binding_log(&n);
    let a1 = observed_attachment(&n, "codex-1", None);
    let a2 = observed_attachment(&n, "codex-1", None);
    let a3 = observed_attachment(&n, "codex-1", None);
    let (v1, v2, v3) = (
        pairing_refusal(&n, &a1).unwrap(),
        pairing_refusal(&n, &a2).unwrap(),
        pairing_refusal(&n, &a3).unwrap(),
    );
    // Without `revealed` the same signal is ambiguous: three candidates, nothing bound.
    n.adapter
        .emit(signal(Some(&c), "T0", StartKind::Fresh, None));
    assert!([&a1, &a2, &a3].iter().all(|a| n.pipes.binding(a).is_none()));
    n.adapter.emit(reveal(&c, "T2", &v2));
    assert_eq!(n.pipes.binding(&a1), None);
    assert!(n.pipes.binding(&a2).is_some());
    assert_eq!(n.pipes.binding(&a3), None);
    n.adapter.emit(reveal(&c, "T3", &v3));
    n.adapter.emit(reveal(&c, "T1", &v1));
    let sids: Vec<_> = [&a1, &a2, &a3]
        .iter()
        .map(|a| n.pipes.binding(a).expect("bound"))
        .collect();
    assert!(sids[0] != sids[1] && sids[1] != sids[2] && sids[0] != sids[2]);
    for (a, sid) in [&a1, &a2, &a3].iter().zip(&sids) {
        assert_eq!(told(&n, a).as_ref(), Some(sid));
    }
    // A bound attachment is served again: its request is no longer refused as unbound.
    let r = n.adapter.sink().send(request(&a2, &sids[0], "hello"));
    assert_ne!(r.error(), Some(ErrorCode::Unauthorized), "{r:?}");
    // The ambiguous signal left its own records; the reveals left none.
    assert!(issued_log(&log).is_empty(), "{:?}", requirements(&log));
}

/// [MCPB-ATT-006] with the carrier rule: a reveal counts only from a registered carrier.
/// From an attachment, from a connection observed with the same key but never designated, and
/// from a signal on no connection, it pairs nothing, withholds nothing, and leaves the value
/// for the real carrier.
#[test]
fn a_reveal_from_a_non_carrier_is_refused() {
    let bus = Bus::default();
    let (n, _t) = pairing_node(&bus);
    let c = designated_carrier(&n, "codex-1");
    let log = binding_log(&n);
    let a = observed_attachment(&n, "codex-1", None);
    let other = observed_attachment(&n, "codex-1", None);
    let v = pairing_refusal(&n, &a).unwrap();
    let undesignated = carrier(&n, "codex-1");
    n.adapter.emit(reveal(&a, "T1", &v));
    n.adapter.emit(reveal(&other, "T1", &v));
    n.adapter.emit(reveal(&undesignated, "T1", &v));
    n.adapter.emit(AdapterEvent::NativeSignal(NativeSignal {
        native_id: "T1".into(),
        start_kind: Some(StartKind::Fresh),
        cross_check: None,
        connection: None,
        revealed: Some(v.clone()),
    }));
    assert_eq!(n.pipes.binding(&a), None);
    assert_eq!(n.pipes.binding(&other), None);
    assert_eq!(issued_log(&log), ["MCPB-ATT-006"; 4]);
    assert!(
        log.entries()
            .iter()
            .all(|e| e.record.kind == RecordKind::Diagnostic)
    );
    // Nothing was used up: the carrier's reveal still pairs.
    n.adapter.emit(reveal(&c, "T1", &v));
    assert!(n.pipes.binding(&a).is_some());
}

/// [MCPB-ATT-022] at the reveal: a carrier of another process cannot pair an attachment of
/// this one, and an unknown value pairs nothing.
#[test]
fn a_reveal_pairs_only_the_attachments_of_its_carriers_process() {
    let bus = Bus::default();
    let (n, _t) = pairing_node(&bus);
    let c1 = designated_carrier(&n, "codex-1");
    let c2 = designated_carrier(&n, "codex-2");
    let log = binding_log(&n);
    let a = observed_attachment(&n, "codex-1", None);
    let v = pairing_refusal(&n, &a).unwrap();
    n.adapter.emit(reveal(&c2, "T1", &v));
    assert_eq!(n.pipes.binding(&a), None);
    assert_eq!(issued_log(&log), ["MCPB-ATT-022"]);
    n.adapter.emit(reveal(
        &c1,
        "T1",
        "oac-pair-00000000000000000000000000000000",
    ));
    assert_eq!(issued_log(&log), ["MCPB-ATT-022", "MCPB-ATT-007"]);
    n.adapter.emit(reveal(&c1, "T1", &v));
    assert!(n.pipes.binding(&a).is_some(), "the value was not used up");
}

/// [MCPB-ATT-009]: a reveal after the window ends binds nothing, however soon after.
#[test]
fn a_reveal_after_its_window_binds_nothing() {
    let bus = Bus::default();
    let (n, t) = pairing_node(&bus);
    let c = designated_carrier(&n, "codex-1");
    let log = binding_log(&n);
    let a = observed_attachment(&n, "codex-1", None);
    let v = pairing_refusal(&n, &a).unwrap();
    t.advance(Duration::from_secs(10));
    n.adapter.emit(reveal(&c, "T1", &v));
    assert_eq!(n.pipes.binding(&a), None);
    assert_eq!(issued_log(&log), ["MCPB-ATT-009"]);
    // Nor does the next window's value count for the old one.
    let v2 = pairing_refusal(&n, &a).unwrap();
    n.adapter.emit(reveal(&c, "T1", &v));
    assert_eq!(n.pipes.binding(&a), None);
    n.adapter.emit(reveal(&c, "T1", &v2));
    assert!(n.pipes.binding(&a).is_some());
}

/// [MCPB-ATT-010]: a paired value pairs once; a replay of it changes nothing. [MCPB-ATT-011],
/// [MCPB-ATT-012]: the same value revealed for another thread ends the binding, with a finding.
#[test]
fn a_replayed_value_changes_nothing_and_a_second_thread_ends_the_binding() {
    let bus = Bus::default();
    let (n, _t) = pairing_node(&bus);
    let c = designated_carrier(&n, "codex-1");
    let log = binding_log(&n);
    let a = observed_attachment(&n, "codex-1", None);
    let v = pairing_refusal(&n, &a).unwrap();
    n.adapter.emit(reveal(&c, "T1", &v));
    let sid = n.pipes.binding(&a).expect("bound");
    n.adapter.emit(reveal(&c, "T1", &v));
    assert_eq!(n.pipes.binding(&a), Some(sid.clone()), "no second binding");
    assert_eq!(told(&n, &a), Some(sid));
    assert_eq!(issued_log(&log), ["MCPB-ATT-010"]);
    n.adapter.emit(reveal(&c, "T2", &v));
    assert_eq!(n.pipes.binding(&a), None, "the connection ends unbound");
    assert_eq!(told(&n, &a), None);
    assert_eq!(issued_log(&log), ["MCPB-ATT-010", "MCPB-ATT-012"]);
    let finding = log.entries().last().cloned().unwrap();
    assert_eq!(finding.record.kind, RecordKind::Finding);
    assert_eq!(finding.attachment, Some(a.clone()));
    // Unbound, it stays unbound: the value cannot bind it again.
    n.adapter.emit(reveal(&c, "T2", &v));
    assert_eq!(n.pipes.binding(&a), None);
}

/// A value is scoped to the attachment it was issued on: revealing a's value binds a only.
#[test]
fn a_value_is_scoped_to_the_attachment_it_was_issued_on() {
    let bus = Bus::default();
    let (n, _t) = pairing_node(&bus);
    let c = designated_carrier(&n, "codex-1");
    let a = observed_attachment(&n, "codex-1", None);
    let b = observed_attachment(&n, "codex-1", None);
    let va = pairing_refusal(&n, &a).unwrap();
    let _vb = pairing_refusal(&n, &b).unwrap();
    n.adapter.emit(reveal(&c, "T1", &va));
    assert!(n.pipes.binding(&a).is_some());
    assert_eq!(n.pipes.binding(&b), None, "b's value was not revealed");
}

/// The windows of a connection that ended go with it: its value pairs nothing, and a new
/// connection of the same key gets a value of its own.
#[test]
fn an_ended_connection_takes_its_values_with_it() {
    let bus = Bus::default();
    let (n, _t) = pairing_node(&bus);
    let c = designated_carrier(&n, "codex-1");
    let log = binding_log(&n);
    let a = observed_attachment(&n, "codex-1", None);
    let v = pairing_refusal(&n, &a).unwrap();
    n.pipes.disconnect(&a);
    n.adapter.emit(reveal(&c, "T1", &v));
    assert_eq!(issued_log(&log), ["MCPB-ATT-007"]);
    let b = observed_attachment(&n, "codex-1", None);
    let vb = pairing_refusal(&n, &b).unwrap();
    assert_ne!(vb, v);
}

/// [IFC-ADP-092], [IFC-ADP-093], [IFC-ADP-091]: on `attachment-unconfirmed` the core stops
/// accepting requests from the attachment and handing off to it, and records a finding. The
/// stop is in force when the event call returns, so a request the adapter passes next is
/// refused. A later native signal paired with the attachment lifts it.
#[test]
fn an_unconfirmed_attachment_is_stopped_until_a_later_signal_pairs() {
    let bus = Bus::default();
    let (n, _t) = pairing_node(&bus);
    let log = binding_log(&n);
    let a = observed_attachment(&n, "harness-1", Some("native-x"));
    n.adapter
        .emit(signal(Some(&a), "native-x", StartKind::Fresh, None));
    let sid = n.pipes.binding(&a).expect("bound");
    let (b, sb) = session(&n, 9);
    local_grant(&n, &sb, &sid);
    local_grant(&n, &sid, &sb);
    assert_eq!(
        n.adapter.sink().send(request(&a, &sb, "before")).outcome(),
        "sent"
    );
    n.adapter.emit(AdapterEvent::AttachmentUnconfirmed {
        attachment: a.clone(),
    });
    // Nothing accepted from it.
    let r = n.adapter.sink().send(request(&a, &sb, "after"));
    assert_eq!(r.error(), Some(ErrorCode::Unauthorized), "{r:?}");
    assert!(matches!(
        discovery_of(&n, &a),
        DiscoveryRequestResult::Refused {
            error: ErrorCode::Unauthorized,
            ..
        }
    ));
    assert_eq!(
        told(&n, &a),
        None,
        "the adapter is told it names no session"
    );
    // Nothing handed off to it (the adapter asserts a hand-off goes to a bound attachment).
    let delivered = n.adapter.deliver_calls.load(Ordering::SeqCst);
    let _ = n.adapter.sink().send(request(&b, &sid, "to a"));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        n.adapter.deliver_calls.load(Ordering::SeqCst),
        delivered,
        "no hand-off to the stopped attachment"
    );
    // One finding for the event ([IFC-ADP-093]).
    assert_eq!(issued_log(&log), ["IFC-ADP-093"]);
    let finding = log.entries().last().cloned().unwrap();
    assert_eq!(finding.record.kind, RecordKind::Finding);
    assert_eq!(finding.attachment, Some(a.clone()));
    // A later paired signal lifts the stop.
    n.adapter
        .emit(signal(Some(&a), "native-x", StartKind::Fresh, None));
    assert_eq!(
        told(&n, &a),
        Some(sid.clone()),
        "a later paired signal lifts the stop"
    );
    assert_ne!(
        n.adapter.sink().send(request(&a, &sb, "again")).error(),
        Some(ErrorCode::Unauthorized)
    );
    assert_eq!(n.pipes.binding(&a), Some(sid));
}

/// [IFC-ADP-092] for an issued-value connection: after the event the attachment is refused
/// with a fresh pairing value, and only a reveal of that value (a later signal) serves it
/// again; a replay of the old reveal does not.
#[test]
fn an_unconfirmed_issued_value_attachment_is_served_again_by_a_new_reveal() {
    let bus = Bus::default();
    let (n, _t) = pairing_node(&bus);
    let c = designated_carrier(&n, "codex-1");
    let log = binding_log(&n);
    let a = observed_attachment(&n, "codex-1", None);
    let other = observed_attachment(&n, "codex-1", None);
    let v1 = pairing_refusal(&n, &a).unwrap();
    n.adapter.emit(reveal(&c, "T1", &v1));
    let sid = n.pipes.binding(&a).expect("bound");
    n.adapter.emit(AdapterEvent::AttachmentUnconfirmed {
        attachment: a.clone(),
    });
    assert_eq!(told(&n, &a), None);
    // The refusal that re-pairs it: a fresh window and value (the earlier one is used).
    let v2 = pairing_refusal(&n, &a).expect("a value to pair again");
    assert_ne!(v1, v2);
    // A replay of the old reveal does not lift the stop.
    n.adapter.emit(reveal(&c, "T1", &v1));
    assert_eq!(told(&n, &a), None);
    n.adapter.emit(reveal(&c, "T1", &v2));
    assert_eq!(
        n.pipes.binding(&a),
        Some(sid.clone()),
        "the same thread keeps its session"
    );
    assert_eq!(told(&n, &a), Some(sid));
    assert_eq!(n.pipes.binding(&other), None);
    assert!(issued_log(&log).contains(&"IFC-ADP-093"));
}

/// S4 (#69) review question: `attachment-unconfirmed` for an attachment the core has NOT
/// bound is harmless. [IFC-ADP-093] makes the finding unconditional, so it is recorded, but it
/// names no session, withdraws no binding (the adapter is told nothing, since there is no
/// session to withdraw), withholds nothing of any other attachment, and does not stop the
/// attachment from being paired by a signal that arrives after the event: the value issued
/// to it before the event still binds it on its reveal. Nothing was handed off to it, and
/// nothing was bound.
#[test]
fn an_unconfirmed_event_for_an_unbound_attachment_is_a_finding_without_a_binding_to_end() {
    let bus = Bus::default();
    let (n, _t) = pairing_node(&bus);
    let c = designated_carrier(&n, "codex-1");
    let log = binding_log(&n);
    let a = observed_attachment(&n, "codex-1", None);
    let other = observed_attachment(&n, "codex-1", None);
    let v = pairing_refusal(&n, &a).expect("a value to pair with");
    assert_eq!(n.pipes.binding(&a), None);
    let told_before = n.adapter.bound.lock().unwrap().get(&a).cloned();
    n.adapter.emit(AdapterEvent::AttachmentUnconfirmed {
        attachment: a.clone(),
    });
    // The finding [IFC-ADP-093] requires, with no session: there was no binding to fail.
    assert_eq!(issued_log(&log), ["IFC-ADP-093"]);
    let finding = log.entries().last().cloned().unwrap();
    assert_eq!(finding.record.kind, RecordKind::Finding);
    assert_eq!(
        (finding.attachment, finding.session),
        (Some(a.clone()), None)
    );
    // No binding was made or ended: the adapter was not told anything about `a`.
    assert_eq!(n.pipes.binding(&a), None);
    assert_eq!(told(&n, &a), None);
    assert_eq!(
        n.adapter.bound.lock().unwrap().get(&a).cloned(),
        told_before,
        "the event changed what the adapter was told of an attachment that was never bound"
    );
    // The stop of [IFC-ADP-092] is in force between the event and the reveal: a request from
    // the attachment is refused.
    let r = n.adapter.sink().send(request(&a, &stranger(), "between"));
    assert_eq!(r.error(), Some(ErrorCode::Unauthorized), "{r:?}");
    assert_eq!(r.outcome(), "refused");
    // Another attachment is not touched: it is still refused with a value, not stopped.
    assert_ne!(pairing_refusal(&n, &other), None);
    // A signal after the event pairs it, with the value issued before the event.
    n.adapter.emit(reveal(&c, "T1", &v));
    let sid = n.pipes.binding(&a).expect("bound by the later reveal");
    assert_eq!(told(&n, &a), Some(sid));
    assert_eq!(n.pipes.binding(&other), None);
}

/// An event for an attachment the adapter does not own, or one unknown, stops nothing.
#[test]
fn an_unconfirmed_event_for_an_unknown_attachment_changes_nothing() {
    let bus = Bus::default();
    let (n, _t) = pairing_node(&bus);
    let log = binding_log(&n);
    let (a, sid) = session(&n, 1);
    let ghost = Connection::accept(Default::default(), Default::default())
        .handle()
        .clone();
    n.adapter
        .emit(AdapterEvent::AttachmentUnconfirmed { attachment: ghost });
    assert_eq!(n.pipes.binding(&a), Some(sid));
    assert!(log.entries().is_empty());
}

/// A node whose pending-signal cap is one, with `a` bound through a real reveal on the
/// carrier `c`, and the one slot taken by a held signal of another key.
fn full_queue_after_a_reveal() -> (Node, Attachment, Attachment, String, MemoryBindingLog) {
    let bus = Bus::default();
    let t = Stepped::new();
    let config = PipelineConfig {
        pairing_window: Duration::from_secs(10),
        max_pending_signals: 1,
        ..PipelineConfig::default()
    };
    let n = node_mono(&bus, "device-a", config, Arc::new(SystemClock), t.clock());
    let c = designated_carrier(&n, "codex-1");
    let log = binding_log(&n);
    let a = observed_attachment(&n, "codex-1", None);
    let v = pairing_refusal(&n, &a).unwrap();
    n.adapter.emit(reveal(&c, "T1", &v));
    assert!(n.pipes.binding(&a).is_some());
    let other = carrier(&n, "other-key");
    n.adapter
        .emit(signal(Some(&other), "X", StartKind::Fresh, None));
    assert!(
        n.pipes.next_native_signal_expiry().is_some(),
        "the slot is held"
    );
    (n, c, a, v, log)
}

/// A reveal nobody may make, turned away at the cap, is dropped with its diagnostic and
/// withholds nothing: one forged reveal cannot stall the connections of the carrier's key.
#[test]
fn a_rejected_reveal_at_the_cap_withholds_nothing() {
    let (n, c, a, _v, log) = full_queue_after_a_reveal();
    let sid = n.pipes.binding(&a).unwrap();
    n.adapter.emit(reveal(
        &c,
        "T9",
        "oac-pair-00000000000000000000000000000000",
    ));
    assert_eq!(n.pipes.binding(&a), Some(sid.clone()));
    assert_eq!(told(&n, &a), Some(sid));
    assert!(
        issued_log(&log).contains(&"MCPB-ATT-007"),
        "{:?}",
        requirements(&log)
    );
    assert!(!requirements(&log).contains(&"SC-ID-154"));
}

/// A conflict turned away at the cap still ends the binding and records the finding.
#[test]
fn a_conflicting_reveal_at_the_cap_still_ends_the_binding() {
    let (n, c, a, v, log) = full_queue_after_a_reveal();
    n.adapter.emit(reveal(&c, "T2", &v));
    assert_eq!(n.pipes.binding(&a), None);
    assert_eq!(told(&n, &a), None);
    assert!(
        issued_log(&log).contains(&"MCPB-ATT-012"),
        "{:?}",
        requirements(&log)
    );
    let finding = log
        .entries()
        .into_iter()
        .find(|e| e.record.requirement == "MCPB-ATT-012")
        .unwrap();
    assert_eq!(finding.record.kind, RecordKind::Finding);
}

/// [MCPB-ATT-023]: the count of windows restarts only when a reveal pairs. A reveal of a
/// live value that the pending-signal cap turns away has used its value up, but it paired
/// nothing, so the connection stays at its third window and gets no fourth.
#[test]
fn a_reveal_turned_away_at_the_cap_does_not_reset_the_window_count() {
    let bus = Bus::default();
    let t = Stepped::new();
    let config = PipelineConfig {
        pairing_window: Duration::from_secs(10),
        max_pending_signals: 1,
        ..PipelineConfig::default()
    };
    let n = node_mono(&bus, "device-a", config, Arc::new(SystemClock), t.clock());
    let c = designated_carrier(&n, "codex-1");
    let a = observed_attachment(&n, "codex-1", None);
    pairing_refusal(&n, &a).unwrap();
    t.advance(Duration::from_secs(10));
    pairing_refusal(&n, &a).unwrap();
    t.advance(Duration::from_secs(10));
    let third = pairing_refusal(&n, &a).unwrap();
    // The one slot is taken by a held signal of another key.
    let other = carrier(&n, "other-key");
    n.adapter
        .emit(signal(Some(&other), "X", StartKind::Fresh, None));
    assert!(
        n.pipes.next_native_signal_expiry().is_some(),
        "the slot is held"
    );
    // The live value is revealed and turned away at the cap: nothing is bound.
    n.adapter.emit(reveal(&c, "T1", &third));
    assert_eq!(n.pipes.binding(&a), None);
    t.advance(Duration::from_secs(1));
    assert_eq!(
        pairing_refusal(&n, &a),
        None,
        "three windows have opened and none paired"
    );
}

// ---- carrier appearances (#420; IFC-TRN-123 to IFC-TRN-125) ------------------------------
//
// The core's monotonic clock is a `Mono` that moves only when a test moves it, so no test
// sleeps, and the owner's timer (the daemon's, in production) is the test calling
// `begin_due_appearance_set` at the instant `next_appearance_set_at` names.

/// A monotonic clock that moves only when told to.
#[derive(Clone)]
struct Mono(Arc<Mutex<(Instant, Duration)>>);

impl Mono {
    fn new() -> Mono {
        Mono(Arc::new(Mutex::new((Instant::now(), Duration::ZERO))))
    }

    /// The time, without reading the clock as the core does.
    fn now(&self) -> Instant {
        self.0.lock().unwrap().0
    }

    fn advance(&self, by: Duration) {
        self.0.lock().unwrap().0 += by;
    }

    /// From now on each read by the core moves the clock on by `step`: the time the core
    /// itself takes between two reads (a lock wait, the signing of a record).
    fn step_per_core_read(&self, step: Duration) {
        self.0.lock().unwrap().1 = step;
    }

    fn clock(&self) -> oac_core::pipeline::MonotonicClock {
        let m = self.clone();
        Arc::new(move || {
            let mut g = m.0.lock().unwrap();
            let step = g.1;
            g.0 += step;
            g.0
        })
    }
}

const HOLD_OFF: Duration = oac_core::pipeline::CARRIER_APPEARANCE_HOLD_OFF;
const BOUND: Duration = oac_core::pipeline::CARRIER_APPEARANCE_BOUND;

/// The record inside a presence payload.
fn record_of(p: &Payload) -> oac_core::presence::PresenceRecord {
    let v = oac_core::json::parse(p.octets()).unwrap();
    let inner = v.as_object().unwrap().get("record").unwrap();
    oac_core::presence::PresenceRecord::from_json(inner).unwrap()
}

/// Device `y` has two sessions: `sy`, which an inbound grant lets a session of device `x`
/// write to ([SEC-AUZ-011]), and `sz`, which no grant covers. Only `y`'s clock is manual.
struct Appearing {
    bus: Bus,
    mono: Mono,
    y: Node,
    xk: KeyId,
    yk: KeyId,
    sy: SessionId,
    sz: SessionId,
}

fn appearing() -> Appearing {
    let bus = Bus::default();
    let mono = Mono::new();
    let x = node(&bus, "device-x", PipelineConfig::default());
    let y = node_mono(
        &bus,
        "device-y",
        PipelineConfig::default(),
        Arc::new(SystemClock),
        mono.clock(),
    );
    pair(&x, &y);
    pair(&y, &x);
    let (xk, yk) = (
        x.pipes.device().key_id().clone(),
        y.pipes.device().key_id().clone(),
    );
    let sx = SessionId::from_random_octets([1; 16]);
    let sy = SessionId::from_random_octets([2; 16]);
    grant(
        &y,
        Grant::Inbound {
            writer: PeerSide::session(xk.clone(), sx),
            target: LocalSide::Session(sy.clone()),
        },
    );
    session(&y, 2);
    let (_, sz) = session(&y, 3);
    Appearing {
        bus,
        mono,
        y,
        xk,
        yk,
        sy,
        sz,
    }
}

impl Appearing {
    fn sent(&self) -> usize {
        self.bus.presence_from(&self.yk).len()
    }

    /// The `(session, seq)` of every presence record `y` has passed to the transport.
    fn records(&self) -> Vec<(SessionId, u64)> {
        self.bus
            .presence_from(&self.yk)
            .iter()
            .map(|p| {
                let r = record_of(p);
                (r.session_id().clone(), r.seq())
            })
            .collect()
    }
}

/// [IFC-TRN-123]: an appearance that no set has covered, and no hold-off holds back, begins a
/// set at once. The set holds one new announcement for each session `y` announces, to each
/// device [SEC-AUZ-011] allows: here `sy` to `x`, and nothing for `sz`. Its `seq` is higher
/// than any earlier for that session ([SC-DLV-050]).
#[test]
fn an_appearance_begins_a_set_to_each_admitted_device() {
    let a = appearing();
    let before = a.records();
    assert_eq!(before.len(), 1, "the announcement made when sy bound");
    a.bus.appear(&a.yk, "link-1");
    let after = a.records();
    assert_eq!(after.len(), before.len() + 1, "{after:?}");
    let (session, seq) = after.last().unwrap();
    assert_eq!(session, &a.sy);
    assert!(*seq > before[0].1, "{after:?}");
    assert!(after.iter().all(|(s, _)| s != &a.sz));
    // Only to the admitted device, through the transport that reported the appearance.
    let destinations: Vec<Destination> = a
        .bus
        .log()
        .into_iter()
        .filter(|(f, k, _)| f == &a.yk && *k == PayloadKind::Presence)
        .map(|(_, _, d)| d)
        .collect();
    assert!(
        destinations
            .iter()
            .all(|d| d == &Destination::Device(a.xk.clone())),
        "{destinations:?}"
    );
    assert_eq!(a.y.pipes.next_appearance_set_at(), None);
}

/// [IFC-TRN-123], [IFC-TRN-125]: an event handed at the instant a set begins is not covered
/// by it. It gets a later set, which begins when the hold-off ends and so within the bound.
#[test]
fn an_appearance_at_the_start_of_a_set_gets_a_later_set_within_the_bound() {
    let a = appearing();
    let base = a.sent();
    let t0 = a.mono.now();
    a.bus.appear(&a.yk, "link-1");
    a.bus.appear(&a.yk, "link-2");
    assert_eq!(a.sent(), base + 1);
    let due = a.y.pipes.next_appearance_set_at().expect("a set is due");
    assert_eq!(due, t0 + HOLD_OFF);
    assert!(due <= t0 + BOUND);
    a.mono.advance(HOLD_OFF - Duration::from_millis(1));
    a.y.pipes.begin_due_appearance_set();
    assert_eq!(a.sent(), base + 1, "the hold-off has not ended");
    a.mono.advance(Duration::from_millis(1));
    a.y.pipes.begin_due_appearance_set();
    assert_eq!(a.sent(), base + 2);
    assert_eq!(a.y.pipes.next_appearance_set_at(), None);
    // The second set is new, not a copy of the first ([SC-DLV-050]).
    let seqs: Vec<u64> = a.records().iter().map(|(_, s)| *s).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
}

/// [IFC-TRN-123], [IFC-TRN-125]: an event handed while a set is being passed to the transport
/// gets a later set, within the bound after the event.
#[test]
fn an_appearance_during_a_set_gets_a_later_set_within_the_bound() {
    let a = appearing();
    let base = a.sent();
    let t0 = a.mono.now();
    let (bus, yk) = (a.bus.clone(), a.yk.clone());
    // Runs on the core's thread as the first announcement of the set is passed.
    a.bus.0.lock().unwrap().on_presence = Some(Box::new(move || bus.appear(&yk, "link-2")));
    a.bus.appear(&a.yk, "link-1");
    assert_eq!(a.sent(), base + 1, "the event during the set begins no set");
    let due = a.y.pipes.next_appearance_set_at().expect("a set is due");
    assert_eq!(due, t0 + HOLD_OFF);
    assert!(due <= t0 + BOUND);
    a.mono.advance(HOLD_OFF);
    a.y.pipes.begin_due_appearance_set();
    assert_eq!(a.sent(), base + 2);
    assert_eq!(a.y.pipes.next_appearance_set_at(), None);
}

/// [IFC-TRN-125]: a series of appearances from one transport, on different carrier handles,
/// begins no set less than the hold-off after the previous one, and every event gets a set
/// within the bound. After a quiet period an appearance begins a set at once.
#[test]
fn no_second_set_begins_within_the_hold_off() {
    let a = appearing();
    let mut began: Vec<Instant> = Vec::new();
    let mut seen = a.sent();
    let mut note = |a: &Appearing| {
        let now = a.sent();
        for _ in seen..now {
            began.push(a.mono.now());
        }
        seen = now;
        began.len()
    };
    let step = |ms: u64| a.mono.advance(Duration::from_millis(ms));
    let mut events: Vec<Instant> = Vec::new();

    events.push(a.mono.now());
    a.bus.appear(&a.yk, "link-1");
    note(&a);
    step(300);
    events.push(a.mono.now());
    a.bus.appear(&a.yk, "link-2");
    step(300);
    events.push(a.mono.now());
    a.bus.appear(&a.yk, "link-3");
    a.y.pipes.begin_due_appearance_set();
    assert_eq!(note(&a), 1, "no second set in the hold-off");
    step(400);
    a.y.pipes.begin_due_appearance_set();
    assert_eq!(
        note(&a),
        2,
        "the held-back set begins when the hold-off ends"
    );
    // At the instant that set began.
    events.push(a.mono.now());
    a.bus.appear(&a.yk, "link-4");
    assert_eq!(note(&a), 2);
    step(999);
    a.y.pipes.begin_due_appearance_set();
    assert_eq!(note(&a), 2);
    step(1);
    a.y.pipes.begin_due_appearance_set();
    assert_eq!(note(&a), 3);
    // Nothing is waiting, so nothing begins.
    step(5000);
    a.y.pipes.begin_due_appearance_set();
    assert_eq!(note(&a), 3);
    events.push(a.mono.now());
    a.bus.appear(&a.yk, "link-5");
    assert_eq!(note(&a), 4, "no hold-off after a quiet period");

    // [IFC-TRN-125]: the sets began at least the hold-off apart.
    assert!(
        began.windows(2).all(|w| w[1] - w[0] >= HOLD_OFF),
        "{began:?}"
    );
    // [IFC-TRN-123]: each event is covered by a set that began after it was handed (or at
    // the instant a later set began), and no later than the bound after it.
    for e in events {
        assert!(
            began.iter().any(|b| *b >= e && *b - e <= BOUND),
            "{e:?} not covered by {began:?}"
        );
    }
}

/// [IFC-TRN-124]: an appearance alone creates no presence record, session or authorization.
/// Device `x` holds an outbound grant to a session `sy` of `y`, which `y` has never
/// announced. After `x`'s transport reports appearances, `x` still knows no such session
/// (it cannot discover or write to it), its own sessions, grants and trusted devices are
/// unchanged, and `y` has been sent nothing but `x`'s own announcement.
#[test]
fn an_appearance_alone_creates_no_presence_record_session_or_authorization() {
    let bus = Bus::default();
    let x = node(&bus, "device-x", PipelineConfig::default());
    let y = node(&bus, "device-y", PipelineConfig::default());
    pair(&x, &y);
    pair(&y, &x);
    let (xk, yk) = (
        x.pipes.device().key_id().clone(),
        y.pipes.device().key_id().clone(),
    );
    let (xa, sx) = session(&x, 1);
    let sy = SessionId::from_random_octets([2; 16]);
    grant(
        &x,
        Grant::Outbound {
            writer: LocalSide::Session(sx.clone()),
            target: PeerSide::session(yk.clone(), sy.clone()),
        },
    );
    let discovered = || match x.adapter.sink().discover(DiscoveryRequest {
        attachment: xa.clone(),
    }) {
        DiscoveryRequestResult::DiscoveryResult(d) => d.len(),
        other => panic!("{other:?}"),
    };
    let before = x.pipes.status();
    assert_eq!(discovered(), 0);

    // Appearances on several handles: still no record of `sy`.
    bus.appear(&xk, "link-1");
    bus.appear(&xk, "link-to-y");
    bus.appear(&xk, "link-3");

    assert_eq!(discovered(), 0, "an appearance is not a presence record");
    assert_eq!(
        refused(x.adapter.sink().send(request(&xa, &sy, "hello"))),
        ErrorCode::UnknownDestination
    );
    let after = x.pipes.status();
    assert_eq!(after.sessions, before.sessions, "no session was created");
    assert_eq!(after.counts, before.counts, "no grant or trusted device");
    // `y` has nothing to announce, so the appearances on `x` made `y` send nothing.
    assert!(bus.presence_from(&yk).is_empty());
    assert!(
        bus.presence_from(&xk)
            .iter()
            .all(|p| record_of(p).session_id() == &sx)
    );

    // A device with no session and no admitted device: the set is empty.
    let lone = Bus::default();
    let w = node(&lone, "device-w", PipelineConfig::default());
    let wk = w.pipes.device().key_id().clone();
    let w_before = w.pipes.status();
    lone.appear(&wk, "link-1");
    assert!(lone.presence_from(&wk).is_empty());
    assert_eq!(w.pipes.status(), w_before);
}

/// [IFC-TRN-125]: a set begins when the core passes its first announcement to the transport,
/// not when it decides to begin it. Here the core's clock moves on while it prepares the
/// first record, so the first pass is later than the decision, and the hold-off runs from the
/// pass. [IFC-TRN-123]: an event handed after the decision is still not covered by that set.
#[test]
fn the_hold_off_runs_from_the_first_pass_not_from_the_decision() {
    let a = appearing();
    let base = a.sent();
    let decision = a.mono.now();
    // Every read of the core's clock now takes 400 ms; the core reads it for the decision,
    // for the announcement and just before it passes the record.
    a.mono.step_per_core_read(Duration::from_millis(400));
    let passed_at = Arc::new(Mutex::new(None));
    let (mono, seen) = (a.mono.clone(), passed_at.clone());
    // Runs on the core's thread as the first announcement is passed to the transport.
    a.bus.0.lock().unwrap().on_presence = Some(Box::new(move || {
        *seen.lock().unwrap() = Some(mono.now());
    }));
    a.bus.appear(&a.yk, "link-1");
    a.mono.step_per_core_read(Duration::ZERO);
    assert_eq!(a.sent(), base + 1);
    // The time the transport saw the record, which is no earlier than the core's pass.
    let in_transport = passed_at.lock().unwrap().expect("a record was passed");
    // An event handed after the decision is not covered by the set, and gets a later one.
    a.bus.appear(&a.yk, "link-2");
    assert_eq!(a.sent(), base + 1);
    let due = a.y.pipes.next_appearance_set_at().expect("a set is due");
    // The hold-off runs from the pass, which is at least two clock reads (800 ms) after the
    // decision and no later than the transport saw the record.
    assert!(due >= decision + Duration::from_millis(800) + HOLD_OFF);
    assert!(due <= in_transport + HOLD_OFF);
    assert!(due > decision + HOLD_OFF);
    // The clock is already past the decision instant plus the hold-off, so a hold-off that
    // ran from the decision would begin the second set now; it does not.
    assert!(a.mono.now() >= decision + HOLD_OFF);
    a.y.pipes.begin_due_appearance_set();
    assert_eq!(a.sent(), base + 1);
    a.mono.advance(due - a.mono.now());
    a.y.pipes.begin_due_appearance_set();
    assert_eq!(a.sent(), base + 2);
}

/// [IFC-TRN-123]: a set with nothing to pass has nothing to observe. It never begins, so it
/// starts no hold-off: when a session that can be announced appears, the next appearance
/// begins a set at once.
#[test]
fn an_empty_set_does_not_begin_and_starts_no_hold_off() {
    let bus = Bus::default();
    let mono = Mono::new();
    let x = node(&bus, "device-x", PipelineConfig::default());
    let y = node_mono(
        &bus,
        "device-y",
        PipelineConfig::default(),
        Arc::new(SystemClock),
        mono.clock(),
    );
    pair(&x, &y);
    pair(&y, &x);
    let (xk, yk) = (
        x.pipes.device().key_id().clone(),
        y.pipes.device().key_id().clone(),
    );
    // No session on y: the set is empty, and the event is discharged.
    bus.appear(&yk, "link-1");
    assert!(bus.presence_from(&yk).is_empty());
    assert_eq!(y.pipes.next_appearance_set_at(), None);
    // A session an inbound grant admits `x` to, bound in the same instant: no hold-off.
    let sx = SessionId::from_random_octets([1; 16]);
    let sy = SessionId::from_random_octets([2; 16]);
    grant(
        &y,
        Grant::Inbound {
            writer: PeerSide::session(xk, sx),
            target: LocalSide::Session(sy.clone()),
        },
    );
    session(&y, 2);
    let base = bus.presence_from(&yk).len();
    bus.appear(&yk, "link-2");
    assert_eq!(bus.presence_from(&yk).len(), base + 1);
}

/// [IFC-TRN-123], [SEC-AUZ-011]: a set holds one new announcement for each session being
/// announced, to each device that rule admits: here two sessions to each of two devices,
/// which is four. A session no grant covers (to any device) and a trusted device no grant
/// covers get none.
#[test]
fn a_set_holds_one_announcement_per_session_per_admitted_device() {
    let bus = Bus::default();
    let mono = Mono::new();
    let y = node_mono(
        &bus,
        "device-y",
        PipelineConfig::default(),
        Arc::new(SystemClock),
        mono.clock(),
    );
    let devices: Vec<Node> = ["device-a", "device-b", "device-c"]
        .iter()
        .map(|n| node(&bus, n, PipelineConfig::default()))
        .collect();
    let keys: Vec<KeyId> = devices
        .iter()
        .map(|d| d.pipes.device().key_id().clone())
        .collect();
    for d in &devices {
        pair(&y, d);
        pair(d, &y);
    }
    let writer = SessionId::from_random_octets([9; 16]);
    let (s1, s2, s3) = (
        SessionId::from_random_octets([1; 16]),
        SessionId::from_random_octets([2; 16]),
        SessionId::from_random_octets([3; 16]),
    );
    // Devices a and b may write to s1 and s2; device c, and every device for s3, may not.
    for target in [&s1, &s2] {
        for k in &keys[..2] {
            grant(
                &y,
                Grant::Inbound {
                    writer: PeerSide::session(k.clone(), writer.clone()),
                    target: LocalSide::Session(target.clone()),
                },
            );
        }
    }
    for seed in [1, 2, 3] {
        session(&y, seed);
    }
    let yk = y.pipes.device().key_id().clone();
    let base = bus.presence_pairs_from(&yk).len();
    assert_eq!(base, 4, "the announcements made as the sessions bound");

    bus.appear(&yk, "link-1");
    let set: Vec<(SessionId, KeyId)> = bus.presence_pairs_from(&yk)[base..].to_vec();
    assert_eq!(set.len(), 4, "{set:?}");
    for session in [&s1, &s2] {
        for k in &keys[..2] {
            assert_eq!(
                set.iter().filter(|(s, d)| s == session && d == k).count(),
                1,
                "{set:?}"
            );
        }
    }
    assert!(
        set.iter().all(|(s, d)| s != &s3 && d != &keys[2]),
        "{set:?}"
    );
}
