// SPDX-License-Identifier: Apache-2.0

//! A device running the core's composed send and receive pipelines
//! ([`oac_core::pipeline::Pipelines`], #313) over the in-memory transport, with a stub
//! adapter in place of a provider adapter.
//!
//! The stub is not an Epic G adapter and models no harness: every connection it takes is an
//! attachment at once, it records each hand-off and answers `completed`, and it exposes the
//! request sink so a test can pass a harness's send request in. It is the smallest
//! `ProviderAdapter` that lets the pipelines run end to end; nothing it does is under test.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use oac_core::adapter::{
    AdapterCapabilities, AdapterEvent, AdapterEventHandler, Attachment, Connection, HandOff,
    HandOffOutcome, ProviderAdapter, RequestSink, SendRequest, SendRequestResult,
};
use oac_core::authorization::{AuthorizationEngine, MemoryDecisionLog, OperatorConfirmed};
use oac_core::clock::{Clock, SystemClock};
use oac_core::envelope::{ContentPart, TextPart};
use oac_core::health::{HealthState, HealthStatus};
use oac_core::ids::SessionId;
use oac_core::keys::DeviceIdentity;
use oac_core::pairing::{MemoryPairingStore, PairedPeer};
use oac_core::pipeline::{AdapterId, PipelineConfig, Pipelines};
use oac_core::session_binding::PeerObservation;
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};

use crate::token;

/// The stub adapter (see the module documentation).
#[derive(Default)]
pub struct StubAdapter {
    events: Mutex<Option<AdapterEventHandler>>,
    sink: Mutex<Option<Arc<dyn RequestSink>>>,
    bound: Mutex<HashMap<Attachment, Option<SessionId>>>,
    delivered: Mutex<Vec<HandOff>>,
    deliver_calls: AtomicUsize,
    /// The cross-check value reported with the next `attachment-opened`.
    cross_check_next: Mutex<Option<String>>,
}

impl StubAdapter {
    /// How many times the core called `deliver`.
    pub fn deliver_calls(&self) -> usize {
        self.deliver_calls.load(Ordering::SeqCst)
    }

    /// The hand-offs the core made, in order.
    pub fn delivered(&self) -> Vec<HandOff> {
        self.delivered.lock().unwrap().clone()
    }

    /// Reports `event` to the core, as the adapter would on observing it (for example a
    /// `native-signal`).
    pub fn emit(&self, event: AdapterEvent) {
        let h = self.events.lock().unwrap().clone();
        if let Some(h) = h {
            h(event);
        }
    }

    /// The session the core's latest `set_binding` named for `attachment`.
    pub fn told(&self, attachment: &Attachment) -> Option<SessionId> {
        self.bound
            .lock()
            .unwrap()
            .get(attachment)
            .cloned()
            .flatten()
    }

    /// Passes `request` into the core's request sink, as the harness's send would.
    pub fn send(&self, request: SendRequest) -> SendRequestResult {
        let sink = self.sink.lock().unwrap().clone().expect("accept_requests");
        sink.send(request)
    }
}

impl ProviderAdapter for StubAdapter {
    fn take_connection(&self, connection: Connection) {
        let (a, _, _) = connection.into_parts();
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
        AdapterCapabilities {
            active_inbound: true,
            content_types: Vec::new(),
            max_envelope_octets: None,
        }
    }

    fn deliver(&self, hand_off: HandOff) -> HandOffOutcome {
        self.deliver_calls.fetch_add(1, Ordering::SeqCst);
        self.delivered.lock().unwrap().push(hand_off);
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

/// A device running the pipelines over `network`, with one stub adapter.
pub struct PipelineDevice {
    /// The pipelines.
    pub pipes: Pipelines,
    /// The stub adapter.
    pub adapter: Arc<StubAdapter>,
    adapter_id: AdapterId,
    /// The pairing store the engine saves to.
    pub pairings: MemoryPairingStore,
}

impl PipelineDevice {
    /// A device for `identity` on `network`, started, with its stub adapter added.
    pub fn new(network: &MemoryNetwork, identity: DeviceIdentity) -> PipelineDevice {
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let engine =
            AuthorizationEngine::new(&identity, clock.clone(), Box::new(MemoryDecisionLog::new()));
        let net = network.clone();
        let pipes = Pipelines::new(
            identity,
            engine,
            clock,
            Arc::new(move || net.now()),
            Arc::new(MemoryTransport::new()),
            PipelineConfig::default(),
        );
        pipes
            .start(MemoryConfiguration::wrap(network))
            .expect("started");
        let adapter = Arc::new(StubAdapter::default());
        let adapter_id = pipes.add_adapter(adapter.clone());
        PipelineDevice {
            pipes,
            adapter,
            adapter_id,
            pairings: MemoryPairingStore::new(),
        }
    }

    /// A harness session: a connection given to the stub adapter, bound to `session`
    /// registered under working directory `wd`.
    pub fn session(&self, session: &SessionId, wd: &str) -> Attachment {
        let conn = Connection::accept(std::io::empty(), std::io::sink());
        let a = conn.handle().clone();
        self.pipes
            .connect(self.adapter_id, conn)
            .expect("connected");
        let record = self
            .pipes
            .device()
            .register(
                session.clone(),
                token("harness"),
                "native-1",
                wd,
                SystemClock.now(),
            )
            .expect("a record");
        self.pipes.bind(&a, &record, None).expect("bound");
        a
    }

    /// A connection given to the stub adapter, which reports it as an attachment with
    /// `cross_check` as its cross-check value, and `observation` as what the core process
    /// observed about its peer. It is bound only by a native signal
    /// (`spec/session-channels.md` §6.7).
    pub fn observed_attachment(
        &self,
        observation: PeerObservation,
        cross_check: Option<&str>,
    ) -> Attachment {
        *self.adapter.cross_check_next.lock().unwrap() = cross_check.map(str::to_owned);
        let conn = Connection::accept(std::io::empty(), std::io::sink());
        let a = conn.handle().clone();
        self.pipes
            .connect_observed(self.adapter_id, conn, observation)
            .expect("connected");
        a
    }

    /// The operator pairs `peer` by comparing its key id.
    pub fn pair(&self, peer: &DeviceIdentity) {
        let p = PairedPeer::by_key_id_comparison(
            peer.principal().clone(),
            *peer.public_key(),
            peer.key_id(),
            SystemClock.now(),
            OperatorConfirmed::by_operator(),
        )
        .expect("the key id matches");
        self.pipes
            .with_engine(|e| e.pair(p, &self.pairings).expect("paired"));
    }

    /// The operator adds `grant`.
    pub fn grant(&self, grant: oac_core::authorization::Grant) {
        self.pipes.with_engine(|e| {
            e.add_grant(grant, OperatorConfirmed::by_operator(), &self.pairings)
                .expect("saved")
        });
    }
}

/// A send request of `text` from `from` to `to`.
pub fn send_request(from: &Attachment, to: &SessionId, text: &str) -> SendRequest {
    SendRequest {
        attachment: from.clone(),
        to: to.clone(),
        content: vec![ContentPart::Text(TextPart::new(text).expect("text"))],
        requested_target: None,
        conversation_id: None,
        correlation_id: None,
    }
}
