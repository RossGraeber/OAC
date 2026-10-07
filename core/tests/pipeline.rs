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
    DiscoveryRequest, DiscoveryRequestResult, HandOff, HandOffOutcome, ProviderAdapter,
    RequestSink, SendRequest, SendRequestResult,
};
use oac_core::authorization::{
    AuthorizationEngine, Grant, LocalSide, MemoryDecisionLog, OperatorConfirmed, PeerSide,
};
use oac_core::clock::{Clock, SystemClock};
use oac_core::delivery::{DeliveryState, ErrorCode, Observer, Scope};
use oac_core::envelope::{ContentPart, TextPart};
use oac_core::health::{HealthState, HealthStatus};
use oac_core::ids::{KeyId, SessionId, Token};
use oac_core::keys::{DeviceIdentity, DeviceKey};
use oac_core::pairing::{MemoryPairingStore, PairedPeer};
use oac_core::pipeline::{AdapterId, PipelineConfig, Pipelines};
use oac_core::receipt::DeliveryReceipt;
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
    /// The last envelope payload taken, for re-injecting a copy.
    last_envelope: Option<Payload>,
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
        Ok(TransportCapabilities {
            reliability: false,
            persistence: false,
            offline_queueing: false,
            ordering: true,
            multicast_discovery: false,
            routing_federation: false,
            reach: Reach::CrossImplementation,
            destination_restricted: true,
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
        let watchers: Vec<PresenceHandler> = {
            let mut st = self.bus.0.lock().unwrap();
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
        })
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
        self.bound.lock().unwrap().insert(a.clone(), None);
        let h = self.events.lock().unwrap().clone();
        if let Some(h) = h {
            h(AdapterEvent::AttachmentOpened {
                attachment: a,
                cross_check: None,
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
    let identity = DeviceIdentity::new(DeviceKey::generate(), Token::parse(principal).unwrap());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
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
