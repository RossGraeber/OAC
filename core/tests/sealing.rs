// SPDX-License-Identifier: Apache-2.0

//! Payload sealing through the composed pipelines (#369; `spec/security.md` §14,
//! `spec/interfaces.md` §6.10), over an in-test sealing transport ([`SealBus`]) and in-test
//! adapters: unit tier, CI-default.
//!
//! The bus behaves as a transport that delivers every payload to every implementation it
//! reaches: each sealed frame goes to every started endpoint's device subscription, the
//! sender's own included, and each receiver keeps what opens. It takes no kind but `sealed`
//! ([IFC-TRN-113]) and records every payload it is offered. The frame format itself is
//! proven by the `oac-sealing-fixture/1` fixtures (`tests/conformance.rs`) and by
//! `oac_core::sealing`'s unit tests.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use oac_core::adapter::{
    AdapterCapabilities, AdapterEvent, AdapterEventHandler, Attachment, Connection, HandOff,
    HandOffOutcome, ProviderAdapter, RequestSink, SendRequest, SendRequestResult,
};
use oac_core::authorization::{
    AuthorizationEngine, Grant, LocalSide, MemoryDecisionLog, OperatorConfirmed, PeerSide,
};
use oac_core::clock::{Clock, ManualClock};
use oac_core::delivery::{DeliveryState, ErrorCode};
use oac_core::envelope::{ContentPart, TextPart};
use oac_core::health::{HealthState, HealthStatus};
use oac_core::ids::{KeyId, SessionId, Timestamp, Token};
use oac_core::json::{self, Json};
use oac_core::keys::{DeviceIdentity, DeviceKey};
use oac_core::pairing::{MemoryPairingStore, PairedPeer};
use oac_core::pipeline::{AdapterId, PipelineConfig, PipelineError, Pipelines};
use oac_core::sealing::{
    AgreementKeys, AgreementPublicKey, AgreementStatement, MemoryAgreementKeyStore, seal,
};
use oac_core::transport::{
    CarrierHandle, Deadline, Destination, Inbound, InboundHandler, Payload, PayloadKind,
    PresenceEvent, PresenceHandler, PublishResult, Reach, Subscription, Transport,
    TransportCapabilities, TransportConfiguration, TransportError,
};

const WAIT: std::time::Duration = std::time::Duration::from_secs(5);
const MAX_FRAME: u64 = 131_072;

// ---- an in-test sealing transport -------------------------------------------------------

#[derive(Default)]
struct SealState {
    /// Device subscriptions, one per started endpoint.
    devices: Vec<(KeyId, InboundHandler)>,
    watchers: Vec<(KeyId, PresenceHandler)>,
    /// Every payload taken: the sender, its kind, its destination and its octets.
    log: Vec<(KeyId, PayloadKind, Destination, Vec<u8>)>,
    /// The kinds of payloads offered and refused ([IFC-TRN-113]).
    refused: Vec<PayloadKind>,
    session_subscriptions: usize,
    send_presence_calls: usize,
    /// The link handle the next endpoint gets: a counter, never a key id ([IFC-TRN-108]).
    next_link: u8,
    /// Frames from this device are taken and logged, but held instead of delivered.
    hold: Option<KeyId>,
    max_frame: Option<u64>,
    fail_start: bool,
    fail_subscribe: bool,
    fail_watch: bool,
    shutdown_calls: usize,
    publish_calls: usize,
    held: Vec<Vec<u8>>,
}

#[derive(Clone, Default)]
struct SealBus(Arc<Mutex<SealState>>);

impl SealBus {
    fn endpoint(&self) -> Arc<SealEndpoint> {
        let link = {
            let mut st = self.0.lock().unwrap();
            st.next_link += 1;
            st.next_link
        };
        Arc::new(SealEndpoint {
            bus: self.clone(),
            key: Mutex::new(None),
            link,
        })
    }

    fn log(&self) -> Vec<(KeyId, PayloadKind, Destination, Vec<u8>)> {
        self.0.lock().unwrap().log.clone()
    }

    fn from(&self, k: &KeyId) -> Vec<Vec<u8>> {
        self.log()
            .into_iter()
            .filter(|(f, ..)| f == k)
            .map(|(.., o)| o)
            .collect()
    }

    /// Holds the frames `from` publishes from now on (`None`: delivers them again).
    fn hold(&self, from: Option<KeyId>) {
        self.0.lock().unwrap().hold = from;
    }

    /// The frames held so far, in order.
    fn take_held(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.0.lock().unwrap().held)
    }

    /// Hands `octets`, as a payload of `kind`, to every device subscription.
    fn inject(&self, kind: PayloadKind, octets: Vec<u8>) {
        let handlers: Vec<InboundHandler> = self
            .0
            .lock()
            .unwrap()
            .devices
            .iter()
            .map(|(_, h)| h.clone())
            .collect();
        for h in handlers {
            h(Inbound {
                payload: Payload::new(kind, octets.clone()),
                carrier: CarrierHandle::from_opaque(vec![0xee]),
            });
        }
    }
}

struct SealEndpoint {
    bus: SealBus,
    key: Mutex<Option<KeyId>>,
    link: u8,
}

impl Transport for SealEndpoint {
    fn start(
        &self,
        local_device: &KeyId,
        _configuration: TransportConfiguration,
    ) -> Result<TransportCapabilities, TransportError> {
        *self.key.lock().unwrap() = Some(local_device.clone());
        if self.bus.0.lock().unwrap().fail_start {
            return Err(TransportError::NotStarted);
        }
        Ok(TransportCapabilities {
            reliability: false,
            persistence: false,
            offline_queueing: false,
            ordering: true,
            multicast_discovery: false,
            routing_federation: false,
            reach: Reach::CrossImplementation,
            destination_restricted: false,
            max_payload_octets: self.bus.0.lock().unwrap().max_frame.unwrap_or(MAX_FRAME),
            sealing: true,
        })
    }

    fn publish(
        &self,
        destination: &Destination,
        payload: Payload,
        _deadline: Deadline,
    ) -> PublishResult {
        self.bus.0.lock().unwrap().publish_calls += 1;
        let Some(me) = self.key.lock().unwrap().clone() else {
            return PublishResult::NotTaken;
        };
        let handlers: Vec<InboundHandler> = {
            let mut st = self.bus.0.lock().unwrap();
            if payload.kind() != PayloadKind::Sealed {
                st.refused.push(payload.kind());
                return PublishResult::NotTaken;
            }
            st.log.push((
                me.clone(),
                payload.kind(),
                destination.clone(),
                payload.octets().to_vec(),
            ));
            if st.hold.as_ref() == Some(&me) {
                st.held.push(payload.octets().to_vec());
                return PublishResult::Taken;
            }
            // Every endpoint alike ([IFC-TRN-106]), this one included.
            st.devices.iter().map(|(_, h)| h.clone()).collect()
        };
        for h in handlers {
            h(Inbound {
                payload: payload.clone(),
                carrier: CarrierHandle::from_opaque(vec![self.link]),
            });
        }
        PublishResult::Taken
    }

    fn subscribe(
        &self,
        destination: &Destination,
        handler: InboundHandler,
    ) -> Result<Subscription, TransportError> {
        if self.bus.0.lock().unwrap().fail_subscribe {
            return Err(TransportError::NotStarted);
        }
        let me = self
            .key
            .lock()
            .unwrap()
            .clone()
            .ok_or(TransportError::NotStarted)?;
        let mut st = self.bus.0.lock().unwrap();
        match destination {
            Destination::Device(k) if k == &me => st.devices.push((me, handler)),
            Destination::Device(_) => return Err(TransportError::NotLocalDevice),
            Destination::Session(_) => st.session_subscriptions += 1,
        }
        Ok(Subscription::new(|| {}))
    }

    fn send_presence(
        &self,
        _destination: &Destination,
        payload: Payload,
        _deadline: Deadline,
    ) -> PublishResult {
        let mut st = self.bus.0.lock().unwrap();
        st.send_presence_calls += 1;
        st.refused.push(payload.kind());
        PublishResult::NotTaken
    }

    fn watch_presence(&self, handler: PresenceHandler) -> Result<(), TransportError> {
        if self.bus.0.lock().unwrap().fail_watch {
            return Err(TransportError::NotStarted);
        }
        let me = self
            .key
            .lock()
            .unwrap()
            .clone()
            .ok_or(TransportError::NotStarted)?;
        self.bus.0.lock().unwrap().watchers.push((me, handler));
        Ok(())
    }

    fn health(&self) -> HealthStatus {
        HealthStatus::new(HealthState::Healthy)
    }

    fn shutdown(&self) {
        self.bus.0.lock().unwrap().shutdown_calls += 1;
        if let Some(me) = self.key.lock().unwrap().take() {
            let mut st = self.bus.0.lock().unwrap();
            st.devices.retain(|(k, _)| k != &me);
            st.watchers.retain(|(k, _)| k != &me);
        }
    }
}

// ---- an in-test adapter -------------------------------------------------------------------

#[derive(Default)]
struct TestAdapter {
    events: Mutex<Option<AdapterEventHandler>>,
    sink: Mutex<Option<Arc<dyn RequestSink>>>,
    bound: Mutex<HashMap<Attachment, Option<SessionId>>>,
    texts: Mutex<Vec<String>>,
    deliver_calls: AtomicUsize,
    max_envelope_octets: Mutex<Option<u64>>,
    content_types: Mutex<Vec<String>>,
}

impl TestAdapter {
    fn send(&self, r: SendRequest) -> SendRequestResult {
        let sink = self.sink.lock().unwrap().clone().expect("accept_requests");
        sink.send(r)
    }

    fn texts(&self) -> Vec<String> {
        self.texts.lock().unwrap().clone()
    }

    fn calls(&self) -> usize {
        self.deliver_calls.load(Ordering::SeqCst)
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
        AdapterCapabilities {
            active_inbound: true,
            content_types: self.content_types.lock().unwrap().clone(),
            max_envelope_octets: *self.max_envelope_octets.lock().unwrap(),
        }
    }

    fn deliver(&self, hand_off: HandOff) -> HandOffOutcome {
        self.deliver_calls.fetch_add(1, Ordering::SeqCst);
        for p in hand_off.message().envelope().content() {
            if let ContentPart::Text(t) = p {
                self.texts.lock().unwrap().push(t.text().to_owned());
            }
        }
        HandOffOutcome::Completed
    }

    fn accept_requests(&self, sink: Arc<dyn RequestSink>) {
        *self.sink.lock().unwrap() = Some(sink);
    }

    fn health(&self) -> HealthStatus {
        HealthStatus::new(HealthState::Healthy)
    }

    fn shutdown(&self) {}
}

// ---- fixtures -------------------------------------------------------------------------------

struct Node {
    pipes: Pipelines,
    adapter: Arc<TestAdapter>,
    adapter_id: AdapterId,
    store: MemoryPairingStore,
    agreement_store: MemoryAgreementKeyStore,
    clock: Arc<ManualClock>,
}

fn now() -> Timestamp {
    oac_core::clock::SystemClock.now()
}

/// A device on `bus`, with its own agreement keys, started.
fn node(bus: &SealBus, principal: &str) -> Node {
    let n = unstarted(bus, principal);
    let keys = AgreementKeys::load_or_generate(&n.agreement_store, n.pipes.device(), &now())
        .expect("agreement keys");
    n.pipes.set_agreement_keys(keys).expect("own statement");
    n.pipes
        .start(TransportConfiguration::new(()))
        .expect("start");
    n
}

fn unstarted(bus: &SealBus, principal: &str) -> Node {
    let identity = DeviceIdentity::new(DeviceKey::generate(), Token::parse(principal).unwrap());
    let clock = Arc::new(ManualClock::new(now()));
    let engine =
        AuthorizationEngine::new(&identity, clock.clone(), Box::new(MemoryDecisionLog::new()));
    let pipes = Pipelines::new(
        identity,
        engine,
        clock.clone(),
        Arc::new(Instant::now),
        bus.endpoint(),
        PipelineConfig::default(),
    );
    let adapter = Arc::new(TestAdapter::default());
    let adapter_id = pipes.add_adapter(adapter.clone());
    Node {
        pipes,
        adapter,
        adapter_id,
        store: MemoryPairingStore::new(),
        agreement_store: MemoryAgreementKeyStore::new(),
        clock,
    }
}

impl Node {
    fn key(&self) -> KeyId {
        self.pipes.device().key_id().clone()
    }

    /// A second handle on this device's agreement keys, as a test's own opener: it reads the
    /// same store.
    fn opener(&self) -> AgreementKeys {
        AgreementKeys::load_or_generate(&self.agreement_store, self.pipes.device(), &now()).unwrap()
    }

    fn agreement_key(&self) -> AgreementPublicKey {
        let st = self.pipes.agreement_statement().unwrap();
        AgreementPublicKey::from_base64url(
            st.as_json()
                .get("agreement_key")
                .and_then(Json::as_str)
                .unwrap(),
        )
        .unwrap()
    }
}

/// `x` pairs `y`'s key, with `y`'s agreement statement when `with_statement`.
fn pair(x: &Node, y: &Node, with_statement: bool) {
    let mut peer = PairedPeer::by_key_id_comparison(
        y.pipes.device().principal().clone(),
        *y.pipes.device().public_key(),
        y.pipes.device().key_id(),
        now(),
        OperatorConfirmed::by_operator(),
    )
    .unwrap();
    if with_statement {
        peer = peer.with_agreement_statement(y.pipes.agreement_statement().unwrap());
    }
    x.pipes.with_engine(|e| e.pair(peer, &x.store).unwrap());
}

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
            now(),
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

fn request(from: &Attachment, to: &SessionId, s: &str) -> SendRequest {
    SendRequest {
        attachment: from.clone(),
        to: to.clone(),
        content: vec![ContentPart::Text(TextPart::new(s).unwrap())],
        requested_target: None,
        conversation_id: None,
        correlation_id: None,
    }
}

/// x and y paired both ways with their statements, a third device z paired with both, and
/// one grant from `sx` on x to `sy` on y. `sy` is bound last, so its announcement reaches x.
struct Three {
    bus: SealBus,
    x: Node,
    y: Node,
    z: Node,
    xa: Attachment,
    sx: SessionId,
    sy: SessionId,
}

fn three(statements: bool) -> Three {
    let bus = SealBus::default();
    let (x, y, z) = (
        node(&bus, "device-x"),
        node(&bus, "device-y"),
        node(&bus, "device-z"),
    );
    for (a, b) in [(&x, &y), (&y, &x), (&x, &z), (&z, &x), (&y, &z), (&z, &y)] {
        pair(a, b, statements);
    }
    let (xa, sx) = session(&x, 1);
    let sy = SessionId::from_random_octets([2; 16]);
    grant(
        &x,
        Grant::Outbound {
            writer: LocalSide::Session(sx.clone()),
            target: PeerSide::session(y.key(), sy.clone()),
        },
    );
    grant(
        &y,
        Grant::Inbound {
            writer: PeerSide::session(x.key(), sx.clone()),
            target: LocalSide::Session(sy.clone()),
        },
    );
    session(&y, 2);
    Three {
        bus,
        x,
        y,
        z,
        xa,
        sx,
        sy,
    }
}

fn sent(r: SendRequestResult) -> (Token, oac_core::adapter::ReceiptStream) {
    match r {
        SendRequestResult::Sent { id, receipts, .. } => (id, receipts),
        other => panic!("not sent: {other:?}"),
    }
}

/// Every payload the bus took is a sealed frame with a `device` destination ([IFC-TRN-100],
/// [IFC-TRN-101]); none was offered unsealed or through `send_presence`.
fn all_sealed(bus: &SealBus) {
    let st = bus.0.lock().unwrap();
    assert!(st.refused.is_empty(), "offered unsealed: {:?}", st.refused);
    assert_eq!(st.send_presence_calls, 0);
    assert_eq!(st.session_subscriptions, 0, "no session subscription");
    for (_, k, d, o) in &st.log {
        assert_eq!(*k, PayloadKind::Sealed);
        assert!(matches!(d, Destination::Device(_)), "{d:?}");
        // [SEC-SEL-025]: padded to 256-octet blocks, so the size shows the block count only.
        assert_eq!((o.len() - 49) % 256, 0, "{}", o.len());
    }
}

// ---- tests ------------------------------------------------------------------------------------

/// `spec/security.md` §14: an exchange between x and y over a transport that delivers every
/// frame to every device. Each payload is sealed to its recipient device ([SEC-SEL-020],
/// [SEC-SEL-023]); y opens and hands off the envelope, x opens y's sealed receipt, and the
/// third device z opens nothing, hands off nothing and sends nothing ([SEC-SEL-030],
/// [SEC-SEL-031]), counting what it discards ([SEC-SEL-038]).
#[test]
fn only_the_recipient_opens_and_a_third_device_stays_silent() {
    let t = three(true);
    let (_, receipts) = sent(t.x.adapter.send(request(&t.xa, &t.sy, "ping")));
    assert_eq!(t.y.adapter.texts(), ["ping"]);
    let r = receipts.0.recv_timeout(WAIT).unwrap();
    assert_eq!(r.state(), DeliveryState::HandedToHarness);
    all_sealed(&t.bus);
    // The envelope frame went to y's device, and opens only under y's agreement key.
    let log = t.bus.log();
    let frames_from_x: Vec<&(KeyId, PayloadKind, Destination, Vec<u8>)> =
        log.iter().filter(|(f, ..)| f == &t.x.key()).collect();
    assert!(!frames_from_x.is_empty());
    let (yo, zo) = (t.y.opener(), t.z.opener());
    for (_, _, d, frame) in &frames_from_x {
        assert_eq!(d, &Destination::Device(t.y.key()));
        assert!(yo.open(frame).is_some());
        assert!(zo.open(frame).is_none(), "z opened x's frame");
        // Neither the content, nor x's key id, nor the session ids, appear in clear.
        let hay = String::from_utf8_lossy(frame);
        for needle in [
            "ping",
            t.x.key().as_str(),
            t.sx.as_str(),
            t.sy.as_str(),
            "principal",
        ] {
            assert!(!hay.contains(needle), "{needle} in a frame");
        }
    }
    // z took every frame, opened none, and sent nothing at all.
    assert_eq!(t.z.adapter.calls(), 0);
    assert!(t.bus.from(&t.z.key()).is_empty());
    assert!(
        t.z.pipes.discarded_frames() >= 2,
        "{}",
        t.z.pipes.discarded_frames()
    );
    // The sender's own frames come back to it and do not open there either.
    assert!(t.x.pipes.discarded_frames() >= 1);
}

/// [SEC-SEL-024], [SEC-SEL-023]: with the recipient trusted but no statement held for it,
/// nothing at all is passed to the transport, sealed or in the clear, and the envelope is
/// `not-passed` with `transport-failure`. Once the statement arrives over a later channel
/// (`Pipelines::admit_statement`), the same send goes through.
#[test]
fn no_statement_means_nothing_is_passed() {
    let t = three(false);
    // No device holds another's statement: not even an announcement crossed.
    assert_eq!(t.bus.log().len(), 0, "nothing crossed without statements");
    // y now holds x's statement, and announces sy to x, sealed to x; x still holds no
    // statement for y.
    t.y.pipes
        .admit_statement(&t.x.pipes.agreement_statement().unwrap(), &t.y.store)
        .unwrap();
    assert!(!t.bus.from(&t.y.key()).is_empty(), "y announced sy to x");
    let r = t.x.adapter.send(request(&t.xa, &t.sy, "ping"));
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
    assert!(t.bus.from(&t.x.key()).is_empty(), "x passed nothing");
    all_sealed(&t.bus);
    assert_eq!(t.y.adapter.calls(), 0);
    // The statement reaches x later; the retry is sealed and handed off.
    t.x.pipes
        .admit_statement(&t.y.pipes.agreement_statement().unwrap(), &t.x.store)
        .unwrap();
    sent(t.x.adapter.send(request(&t.xa, &t.sy, "again")));
    assert_eq!(t.y.adapter.texts(), ["again"]);
    all_sealed(&t.bus);
}

/// [SEC-SEL-001]: a sealing transport is not started without an agreement key, and the
/// transport is shut down again.
#[test]
fn a_sealing_transport_needs_an_agreement_key() {
    let bus = SealBus::default();
    let n = unstarted(&bus, "device-x");
    assert_eq!(
        n.pipes.start(TransportConfiguration::new(())),
        Err(PipelineError::NoAgreementKey)
    );
    assert!(bus.0.lock().unwrap().devices.is_empty());
}

/// [SEC-SEL-016]: a device holds its own statement as admitted for its own key, so a
/// message between two of its own sessions is sealed to it like any other, and handed off.
#[test]
fn own_sessions_are_sealed_to_the_own_statement() {
    let bus = SealBus::default();
    let n = node(&bus, "device-x");
    let own = n.key();
    assert!(n.pipes.with_engine(|e| e.held_statement(&own).is_some()));
    let (a1, s1) = session(&n, 1);
    let (_, s2) = session(&n, 2);
    grant(
        &n,
        Grant::Inbound {
            writer: PeerSide::session(own.clone(), s1.clone()),
            target: LocalSide::Session(s2.clone()),
        },
    );
    let (_, receipts) = sent(n.adapter.send(request(&a1, &s2, "inside")));
    assert_eq!(n.adapter.texts(), ["inside"]);
    assert_eq!(
        receipts.0.recv_timeout(WAIT).unwrap().state(),
        DeliveryState::HandedToHarness
    );
    all_sealed(&bus);
    let log = bus.log();
    assert_eq!(log.len(), 1, "one frame, the envelope");
    assert_eq!(log[0].2, Destination::Device(own));
    assert!(n.opener().open(&log[0].3).is_some());
}

/// [SEC-SEL-030], [SEC-SEL-031], [SEC-SEL-038]: frames that do not open, and frames that
/// open only under another device's key, are discarded with no hand-off, no receipt and
/// nothing sent; each is counted.
#[test]
fn frames_that_do_not_open_are_discarded_silently() {
    let t = three(true);
    let before = t.y.pipes.discarded_frames();
    let sent_before = t.bus.log().len();
    // Garbage, a short frame, an unknown version, and a frame sealed to z.
    t.bus.inject(PayloadKind::Sealed, vec![0u8; 10]);
    t.bus.inject(PayloadKind::Sealed, vec![1u8; 300]);
    let mut v2 = seal(
        &t.y.agreement_key(),
        PayloadKind::Envelope,
        b"{}",
        MAX_FRAME,
    )
    .unwrap();
    v2[0] = 2;
    t.bus.inject(PayloadKind::Sealed, v2);
    let to_z = seal(
        &t.z.agreement_key(),
        PayloadKind::Envelope,
        b"{}",
        MAX_FRAME,
    )
    .unwrap();
    t.bus.inject(PayloadKind::Sealed, to_z);
    assert_eq!(t.y.pipes.discarded_frames(), before + 4);
    assert_eq!(t.bus.log().len(), sent_before, "nothing answered");
    assert_eq!(t.y.adapter.calls(), 0);
}

/// Hands `octets` to every `watch_presence` handler as a presence record in the clear.
fn plain_presence(bus: &SealBus, octets: &[u8]) {
    let watchers: Vec<PresenceHandler> = bus
        .0
        .lock()
        .unwrap()
        .watchers
        .iter()
        .map(|(_, h)| h.clone())
        .collect();
    for w in watchers {
        w(PresenceEvent::Record {
            payload: Payload::new(PayloadKind::Presence, octets.to_vec()),
            carrier: CarrierHandle::from_opaque(vec![9]),
        });
    }
}

/// [IFC-TRN-103]: payloads of any kind other than `sealed`, from a sealing transport's
/// device subscription or from `watch_presence`, are discarded unread. A fresh, valid,
/// authorized envelope and a fresh announcement, each in the clear, are not taken; the same
/// payloads sealed are.
#[test]
fn unsealed_payloads_from_a_sealing_transport_are_discarded() {
    let t = three(true);
    // A new session s5 on y that sx may write to, announced to x while y's frames are held.
    let s5 = SessionId::from_random_octets([5; 16]);
    grant(
        &t.x,
        Grant::Outbound {
            writer: LocalSide::Session(t.sx.clone()),
            target: PeerSide::session(t.y.key(), s5.clone()),
        },
    );
    grant(
        &t.y,
        Grant::Inbound {
            writer: PeerSide::session(t.x.key(), t.sx.clone()),
            target: LocalSide::Session(s5.clone()),
        },
    );
    t.bus.hold(Some(t.y.key()));
    session(&t.y, 5);
    t.bus.hold(None);
    let held = t.bus.take_held();
    let xo = t.x.opener();
    let announcement = held
        .iter()
        .filter_map(|f| xo.open(f))
        .find(|o| o.kind == PayloadKind::Presence)
        .expect("y's announcement of s5 to x");
    // In the clear, through `watch_presence` and the device subscription: not taken, so x
    // does not know s5.
    plain_presence(&t.bus, &announcement.payload);
    t.bus
        .inject(PayloadKind::Presence, announcement.payload.to_vec());
    assert!(matches!(
        t.x.adapter.send(request(&t.xa, &s5, "too early")),
        SendRequestResult::Refused {
            error: ErrorCode::UnknownDestination
        }
    ));
    // Sealed: taken.
    for f in held {
        t.bus.inject(PayloadKind::Sealed, f);
    }
    // x's envelope to s5, held, then offered to y in the clear, as each other kind.
    t.bus.hold(Some(t.x.key()));
    sent(t.x.adapter.send(request(&t.xa, &s5, "sealed only")));
    t.bus.hold(None);
    let frames = t.bus.take_held();
    let yo = t.y.opener();
    let envelope = frames
        .iter()
        .filter_map(|f| yo.open(f))
        .find(|o| o.kind == PayloadKind::Envelope)
        .expect("x's envelope to s5");
    for kind in [
        PayloadKind::Envelope,
        PayloadKind::Receipt,
        PayloadKind::Presence,
    ] {
        t.bus.inject(kind, envelope.payload.to_vec());
    }
    plain_presence(&t.bus, &envelope.payload);
    assert_eq!(t.y.adapter.calls(), 0, "nothing unsealed was taken");
    for f in frames {
        t.bus.inject(PayloadKind::Sealed, f);
    }
    assert_eq!(t.y.adapter.texts(), ["sealed only"]);
}

/// [SEC-SEL-035]: y opens x's envelope and announcement and seals each again to z. z binds
/// the envelope's `to` to y, is not the announcement's audience, and never sent the envelope
/// a forwarded receipt describes: it discards each silently, before any other check, with
/// no receipt, no finding and no hand-off.
#[test]
fn forwarded_payloads_are_discarded_by_a_third_device() {
    let t = three(true);
    let (_, receipts) = sent(t.x.adapter.send(request(&t.xa, &t.sy, "for y only")));
    receipts.0.recv_timeout(WAIT).unwrap();
    // z binds sy to y: as if it had accepted y's announcement of sy.
    let yk = t.y.key();
    t.z.pipes.with_engine(|e| e.bind(&t.sy, &yk));
    // z grants x's device everything on z: only the recipient check stops an answer.
    grant(
        &t.z,
        Grant::Inbound {
            writer: PeerSide::device(t.x.key()),
            target: LocalSide::Device,
        },
    );
    let yo = t.y.opener();
    let xo = t.x.opener();
    let mut forwarded = Vec::new();
    for frame in t.bus.from(&t.x.key()) {
        if let Some(o) = yo.open(&frame) {
            forwarded.push(o);
        }
    }
    for frame in t.bus.from(&t.y.key()) {
        if let Some(o) = xo.open(&frame)
            && o.kind == PayloadKind::Receipt
        {
            forwarded.push(o);
        }
    }
    let kinds: Vec<PayloadKind> = forwarded.iter().map(|o| o.kind).collect();
    assert!(kinds.contains(&PayloadKind::Envelope), "{kinds:?}");
    assert!(kinds.contains(&PayloadKind::Presence), "{kinds:?}");
    assert!(kinds.contains(&PayloadKind::Receipt), "{kinds:?}");
    let zk = t.z.agreement_key();
    let z_before = t.bus.from(&t.z.key()).len();
    let discarded = t.z.pipes.discarded_frames();
    for o in &forwarded {
        let frame = seal(&zk, o.kind, &o.payload, MAX_FRAME).unwrap();
        t.bus.inject(PayloadKind::Sealed, frame);
    }
    // Opened (not counted as unopenable), but dropped as for another device.
    assert_eq!(t.z.pipes.discarded_frames(), discarded);
    assert_eq!(t.z.adapter.calls(), 0);
    assert_eq!(t.bus.from(&t.z.key()).len(), z_before, "z sent nothing");
}

const PAST_THE_WINDOW: i128 = 301 * 1_000_000_000;

/// [SEC-SEL-036]: a payload opened at or after its deadline on the receiver's clock is
/// dropped with no receipt and no finding. An envelope that would, unsealed, draw an
/// `outside-replay-window` receipt draws none; a receipt that would, unsealed, update its
/// envelope's tracker does not.
#[test]
fn late_opened_payloads_are_dropped_without_a_receipt() {
    let t = three(true);
    // An envelope (with x's first announcement), held, then opened by y past its deadline.
    t.bus.hold(Some(t.x.key()));
    let (_, receipts) = sent(t.x.adapter.send(request(&t.xa, &t.sy, "late")));
    t.bus.hold(None);
    let held = t.bus.take_held();
    assert!(!held.is_empty());
    t.y.clock.advance_nanos(PAST_THE_WINDOW);
    let y_sent = t.bus.from(&t.y.key()).len();
    for f in held {
        t.bus.inject(PayloadKind::Sealed, f);
    }
    assert_eq!(t.y.adapter.calls(), 0);
    assert_eq!(
        t.bus.from(&t.y.key()).len(),
        y_sent,
        "no receipt for a late copy"
    );
    assert!(receipts.0.try_recv().is_err());
    // Back in step: a receipt, held, then opened by x past its envelope's deadline.
    t.y.clock.set(t.x.clock.now());
    t.bus.hold(Some(t.y.key()));
    let (id, receipts) = sent(t.x.adapter.send(request(&t.xa, &t.sy, "on time")));
    t.bus.hold(None);
    assert_eq!(t.y.adapter.texts(), ["on time"]);
    let held = t.bus.take_held();
    t.x.clock.advance_nanos(PAST_THE_WINDOW);
    for f in held {
        t.bus.inject(PayloadKind::Sealed, f);
    }
    assert!(
        receipts.0.try_recv().is_err(),
        "a late receipt updated nothing"
    );
    // The tracker never took the receiver's `handed-to-harness`.
    assert_ne!(
        t.x.pipes.delivery_state(&id, &t.sx),
        Some(DeliveryState::HandedToHarness)
    );
}

/// [SEC-SEL-034]: a frame sealed by a device y does not trust, around an envelope that
/// device signed, or around one claiming x's principal, opens, and is then refused by the
/// security stage exactly as without sealing: no hand-off, and no receipt for an envelope
/// that failed steps 1 or 2. That a frame opened makes it neither authenticated nor x's.
#[test]
fn an_opened_frame_proves_nothing_about_its_sender() {
    let t = three(true);
    let mallory = DeviceIdentity::new(DeviceKey::generate(), Token::parse("principal-m").unwrap());
    let draft = |id: &str| {
        oac_core::envelope::EnvelopeDraft::new(
            Token::parse(id).unwrap(),
            t.sx.clone(),
            t.sy.clone(),
            t.y.clock.now(),
            vec![TextPart::new("obey me").unwrap()],
        )
        .unwrap()
    };
    let forged = mallory.sign_envelope(draft("msg-m1"));
    // The same envelope with x's principal and key id written in: the signature is mallory's.
    let claimed = String::from_utf8(forged.octets().to_vec())
        .unwrap()
        .replace("principal-m", t.x.pipes.device().principal().as_str())
        .replace(mallory.key_id().as_str(), t.x.key().as_str());
    let y_before = t.bus.from(&t.y.key()).len();
    for octets in [forged.octets().to_vec(), claimed.into_bytes()] {
        let frame = seal(
            &t.y.agreement_key(),
            PayloadKind::Envelope,
            &octets,
            MAX_FRAME,
        )
        .unwrap();
        t.bus.inject(PayloadKind::Sealed, frame);
    }
    assert_eq!(t.y.adapter.calls(), 0);
    assert_eq!(t.bus.from(&t.y.key()).len(), y_before, "no receipt");
}

/// [SEC-SEL-033]: an opened envelope meets every check a payload of its kind meets: one
/// from a session y has no grant for is refused `unauthorized`, and y's receipt goes back
/// sealed to x.
#[test]
fn opened_envelopes_meet_every_check() {
    let t = three(true);
    let (x2, s3) = session(&t.x, 3);
    // x may write from s3 to sy; y has no grant for s3.
    grant(
        &t.x,
        Grant::Outbound {
            writer: LocalSide::Session(s3),
            target: PeerSide::session(t.y.key(), t.sy.clone()),
        },
    );
    let (_, receipts) = sent(t.x.adapter.send(request(&x2, &t.sy, "unauthorized")));
    let r = receipts.0.recv_timeout(WAIT).unwrap();
    assert_eq!(
        (r.state(), r.error().map(|e| e.as_str().to_owned())),
        (DeliveryState::Rejected, Some("unauthorized".to_owned()))
    );
    assert_eq!(t.y.adapter.calls(), 0);
    all_sealed(&t.bus);
}

/// [SEC-SEL-017], §14.9, [SEC-SEL-037]: y replaces its agreement key. Until x admits the
/// new statement, x seals to the old key, and y, holding both, still opens; once x admits
/// it, frames go to the new key. A replayed older statement is then refused
/// ([SEC-SEL-014]); after y erases the old key ([SEC-SEL-018]), frames to it no longer open.
#[test]
fn a_replaced_key_still_opens_until_erased() {
    let t = three(true);
    let old = t.y.agreement_key();
    let old_statement = t.y.pipes.agreement_statement().unwrap();
    let mut keys = t.y.opener();
    keys.replace(
        &t.y.agreement_store,
        t.y.pipes.device(),
        &Timestamp::from_unix_nanos(now().unix_nanos() + 1_000_000).unwrap(),
    )
    .unwrap();
    t.y.pipes.set_agreement_keys(keys).unwrap();
    let new = t.y.agreement_key();
    assert_ne!(old, new);
    // x still holds the old statement: sealed to the old key, and y opens it.
    sent(t.x.adapter.send(request(&t.xa, &t.sy, "to the old key")));
    assert_eq!(t.y.adapter.texts(), ["to the old key"]);
    // x admits the new statement over a later channel; the old one is refused again.
    t.x.pipes
        .admit_statement(&t.y.pipes.agreement_statement().unwrap(), &t.x.store)
        .unwrap();
    assert!(
        t.x.pipes
            .admit_statement(&old_statement, &t.x.store)
            .is_err()
    );
    sent(t.x.adapter.send(request(&t.xa, &t.sy, "to the new key")));
    assert_eq!(t.y.adapter.texts(), ["to the old key", "to the new key"]);
    let last = t.bus.from(&t.x.key()).pop().unwrap();
    let only_new = {
        let mut k = t.y.opener();
        k.erase_replaced(&t.y.agreement_store).unwrap();
        k
    };
    assert!(only_new.open(&last).is_some(), "sealed to the new key");
    // Erased: a frame to the old key no longer opens on y.
    t.y.pipes.set_agreement_keys(only_new).unwrap();
    let before = t.y.pipes.discarded_frames();
    t.bus.inject(
        PayloadKind::Sealed,
        seal(&old, PayloadKind::Envelope, b"{}", MAX_FRAME).unwrap(),
    );
    assert_eq!(t.y.pipes.discarded_frames(), before + 1);
}

/// [SEC-SEL-015], [SEC-SEL-041]: removing a key removes its statement in the same step,
/// so nothing more is sealed to that device; statements are saved with the trusted keys and
/// come back after a restart, where a replayed older statement is still refused.
#[test]
fn statements_live_and_die_with_their_keys() {
    let t = three(true);
    let yk = t.y.key();
    // A restart of x's engine from its pairing store holds y's statement again.
    let restored = AuthorizationEngine::restore(
        t.x.pipes.device(),
        &t.x.store,
        t.x.clock.clone(),
        Box::new(MemoryDecisionLog::new()),
    )
    .unwrap();
    let held = restored.held_statement(&yk).unwrap().clone();
    assert_eq!(
        Some(&held),
        t.x.pipes
            .with_engine(|e| e.held_statement(&yk).cloned())
            .as_ref()
    );
    let mut restored = restored;
    assert!(
        restored
            .admit_statement(&t.y.pipes.agreement_statement().unwrap(), &t.x.store)
            .is_err(),
        "the same seq is refused after the restart"
    );
    // Removing y's key removes its statement, and x passes nothing more to y.
    let removal = t.x.pipes.with_engine(|e| {
        e.remove_key(&yk, OperatorConfirmed::by_operator(), &t.x.store)
            .unwrap()
    });
    assert_eq!(removal.agreement_statement, Some(held));
    assert!(t.x.pipes.with_engine(|e| e.held_statement(&yk).is_none()));
    let snapshot = t.x.pipes.with_engine(|e| e.snapshot());
    assert!(
        snapshot
            .agreement_statements
            .iter()
            .all(|h| h.key_id() != &yk)
    );
}

/// [IFC-TRN-104]: over a sealing transport, a session's declared `max_envelope_octets` is
/// at most the transport's `max_payload_octets` less 54, whatever the adapter states; and an
/// envelope of that size goes out in one frame.
#[test]
fn the_declared_envelope_limit_leaves_room_for_the_frame() {
    let bus = SealBus::default();
    let (x, y) = (node(&bus, "device-x"), node(&bus, "device-y"));
    pair(&x, &y, true);
    pair(&y, &x, true);
    *x.adapter.max_envelope_octets.lock().unwrap() = Some(10_000_000);
    let s1 = SessionId::from_random_octets([1; 16]);
    grant(
        &x,
        Grant::Inbound {
            writer: PeerSide::device(y.key()),
            target: LocalSide::Session(s1.clone()),
        },
    );
    // The declaration goes out in x's announcement of s1 to y, as y receives it.
    session(&x, 1);
    let frames = bus.from(&x.key());
    let yo = y.opener();
    let presence = frames
        .iter()
        .filter_map(|f| yo.open(f))
        .find(|o| o.kind == PayloadKind::Presence)
        .expect("x announced s1 to y");
    let rec = json::parse(&presence.payload).unwrap();
    let text = rec.to_compact();
    let want = format!("\"max_envelope_octets\":{}", MAX_FRAME - 54);
    assert!(text.contains(&want), "{text}");
    assert!(text.contains(s1.as_str()));
}

/// A statement offered at pairing that the paired key did not sign is not taken with it
/// ([SEC-SEL-012]); the pairing stands, with no statement held.
#[test]
fn pairing_takes_only_the_paired_keys_statement() {
    let bus = SealBus::default();
    let (x, y, z) = (
        node(&bus, "device-x"),
        node(&bus, "device-y"),
        node(&bus, "device-z"),
    );
    // z is already trusted, but has no held statement yet: its valid signature
    // must not authorize storing its agreement key under y.
    pair(&x, &z, false);
    let peer = PairedPeer::by_key_id_comparison(
        y.pipes.device().principal().clone(),
        *y.pipes.device().public_key(),
        y.pipes.device().key_id(),
        now(),
        OperatorConfirmed::by_operator(),
    )
    .unwrap()
    .with_agreement_statement(z.pipes.agreement_statement().unwrap());
    x.pipes.with_engine(|e| e.pair(peer, &x.store).unwrap());
    let yk = y.key();
    assert!(x.pipes.with_engine(|e| e.trusted_keys().get(&yk).is_some()));
    assert!(x.pipes.with_engine(|e| e.held_statement(&yk).is_none()));
    // A statement is plain data: its JSON round-trips.
    let st = y.pipes.agreement_statement().unwrap();
    let back = AgreementStatement::from_json(
        &json::parse(Json::Object(st.as_json().clone()).to_compact().as_bytes()).unwrap(),
    )
    .unwrap();
    assert_eq!(back, st);
}

/// A presence declaration can exceed the frame limit even when envelopes are capped.
/// A failed seal must never offer the original payload to the transport.
#[test]
fn seal_failure_passes_nothing_to_the_transport() {
    let bus = SealBus::default();
    bus.0.lock().unwrap().max_frame = Some(65_590);
    let (x, y) = (node(&bus, "device-x"), node(&bus, "device-y"));
    pair(&x, &y, true);
    *x.adapter.content_types.lock().unwrap() =
        (0..5000).map(|i| format!("org.example/type-{i}")).collect();
    grant(
        &x,
        Grant::Inbound {
            writer: PeerSide::device(y.key()),
            target: LocalSide::Session(SessionId::from_random_octets([1; 16])),
        },
    );
    session(&x, 1);
    assert!(bus.log().is_empty(), "no frame was published");
    let st = bus.0.lock().unwrap();
    assert!(st.refused.is_empty(), "no plain payload was offered");
    assert_eq!(st.send_presence_calls, 0);
    assert_eq!(st.publish_calls, 0);
}

/// Without a transport declaration, even authorized announcements are not passed.
#[test]
fn unknown_capabilities_pass_nothing() {
    let t = three(true);
    sent(
        t.x.adapter
            .send(request(&t.xa, &t.sy, "establish announcement")),
    );
    {
        let mut st = t.bus.0.lock().unwrap();
        st.fail_watch = true;
        st.log.clear();
        st.publish_calls = 0;
    }
    assert!(t.x.pipes.start(TransportConfiguration::new(())).is_err());
    *t.x.adapter.max_envelope_octets.lock().unwrap() = Some(70_000);
    let handler = t.x.adapter.events.lock().unwrap().clone().unwrap();
    handler(AdapterEvent::CapabilitiesChanged {
        attachment: t.xa.clone(),
    });
    let st = t.bus.0.lock().unwrap();
    assert!(st.log.is_empty());
    assert!(st.refused.is_empty());
    assert_eq!(st.send_presence_calls, 0);
    assert_eq!(st.publish_calls, 0);
}

/// Each failure after transport startup shuts it down. Existing authorized sends
/// report not-passed/transport-failure without offering a plain payload.
#[test]
fn start_failures_shutdown_and_refuse_sends() {
    for failure in 0..3 {
        let t = three(true);
        {
            let mut st = t.bus.0.lock().unwrap();
            st.fail_start = failure == 0;
            st.fail_subscribe = failure == 1;
            st.fail_watch = failure == 2;
            st.log.clear();
            st.publish_calls = 0;
        }
        assert!(matches!(
            t.x.pipes.start(TransportConfiguration::new(())),
            Err(PipelineError::Transport(_))
        ));
        assert_eq!(t.bus.0.lock().unwrap().shutdown_calls, 1);
        assert!(matches!(
            t.x.adapter.send(request(&t.xa, &t.sy, "refuse")),
            SendRequestResult::NotPassed {
                error: ErrorCode::TransportFailure,
                ..
            }
        ));
        let st = t.bus.0.lock().unwrap();
        assert!(st.log.is_empty());
        assert!(st.refused.is_empty());
        assert_eq!(st.send_presence_calls, 0);
        assert_eq!(st.publish_calls, 0);
    }
}
