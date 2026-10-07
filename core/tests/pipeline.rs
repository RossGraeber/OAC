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
    BindingResult, MemoryBindingLog, PairingKey, PeerObservation, RecordKind,
};
use oac_core::transport::{
    CarrierHandle, Deadline, Destination, Inbound, InboundHandler, Payload, PayloadKind,
    PresenceEvent, PresenceHandler, PublishResult, Reach, Subscription, Transport,
    TransportCapabilities, TransportConfiguration, TransportError,
};

const WAIT: Duration = Duration::from_secs(5);

// ---- an in-test transport ----------------------------------------------------------------

#[derive(Default)]
struct BusState {
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
            st.subs
                .iter()
                .filter(|(_, owner, d, _)| {
                    d == destination
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
    /// Run once at the next `capabilities` call.
    on_capabilities: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// The next connection taken carries native signals only: it is not an attachment.
    carrier_next: Mutex<bool>,
    /// The cross-check value reported with the next `attachment-opened`.
    cross_check_next: Mutex<Option<String>>,
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
            on_capabilities: Mutex::default(),
            carrier_next: Mutex::default(),
            cross_check_next: Mutex::default(),
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
        self.bound
            .lock()
            .unwrap()
            .insert(attachment.clone(), session);
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
        HealthStatus::new(HealthState::Healthy)
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
    let identity = DeviceIdentity::new(DeviceKey::generate(), Token::parse(principal).unwrap());
    let engine =
        AuthorizationEngine::new(&identity, clock.clone(), Box::new(MemoryDecisionLog::new()));
    let pipes = Pipelines::new(
        identity,
        engine,
        clock,
        Arc::new(Instant::now),
        bus.endpoint(),
        config,
    );
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
    let conn = Connection::accept(std::io::empty(), std::io::sink());
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

/// Table 8.1 and Table 8.3 for a request result: `refused` has a `request`-scoped code and
/// no state; `not-passed` is `failed` with a sender code; `sent` is `accepted-by-adapter`.
fn check_result_tables(r: &SendRequestResult) {
    match r {
        SendRequestResult::Refused { error } => {
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
        SendRequestResult::Refused { error } => error,
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
    let stray = Connection::accept(std::io::empty(), std::io::sink())
        .handle()
        .clone();
    assert_eq!(
        refused(n.adapter.sink().send(request(&stray, &sb, "x"))),
        ErrorCode::Unauthorized
    );
    let conn = Connection::accept(std::io::empty(), std::io::sink());
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
    let forged = Connection::accept(std::io::empty(), std::io::sink())
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
    let conn = Connection::accept(std::io::empty(), std::io::sink());
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
    let conn = Connection::accept(std::io::empty(), std::io::sink());
    let a = conn.handle().clone();
    n.pipes
        .connect_observed(n.adapter_id, conn, observed(key))
        .unwrap();
    a
}

/// A connection observed with `key` that carries native signals and is no attachment.
fn carrier(n: &Node, key: &str) -> Attachment {
    *n.adapter.carrier_next.lock().unwrap() = true;
    let conn = Connection::accept(std::io::empty(), std::io::sink());
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
    let conn = Connection::accept(std::io::empty(), std::io::sink());
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
    let bare = Connection::accept(std::io::empty(), std::io::sink());
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
        let conn = Connection::accept(std::io::empty(), std::io::sink());
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
    let third = Connection::accept(std::io::empty(), std::io::sink());
    assert_eq!(
        n.pipes.connect(n.adapter_id, third),
        Err(PipelineError::TooManyConnections)
    );
    n.pipes.disconnect(&c1);
    n.pipes.disconnect(&c1);
    let again = Connection::accept(std::io::empty(), std::io::sink());
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
