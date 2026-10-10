// SPDX-License-Identifier: Apache-2.0

//! Devices running the core's composed pipelines over a sealing transport (#369;
//! `spec/interfaces.md` §6.10), with the [`crate::pipeline::StubAdapter`].
//!
//! [`SealBus`] stands in for a transport that delivers every payload to every implementation
//! it reaches, the case payload sealing exists for: each frame goes to every started
//! device's subscription, the sender's own included. It declares `sealing`, takes no kind but
//! `sealed` ([IFC-TRN-113]), and records what it is offered, so a test can read the wire as
//! every other device on it would. It is a test double, not the reference transport: the
//! reference transport's sealing framing is G1's follow-up (#62).

use std::sync::{Arc, Mutex};

use oac_core::adapter::{Attachment, Connection};
use oac_core::authorization::{AuthorizationEngine, MemoryDecisionLog, OperatorConfirmed};
use oac_core::clock::{Clock, SystemClock};
use oac_core::health::{HealthState, HealthStatus};
use oac_core::ids::{KeyId, SessionId};
use oac_core::json::Json;
use oac_core::keys::DeviceIdentity;
use oac_core::pairing::{MemoryPairingStore, PairedPeer};
use oac_core::pipeline::{AdapterId, PipelineConfig, Pipelines};
use oac_core::sealing::{AgreementKeys, AgreementPublicKey, MemoryAgreementKeyStore};
use oac_core::transport::{
    CarrierHandle, Deadline, Destination, Inbound, InboundHandler, Payload, PayloadKind,
    PresenceEvent, PresenceHandler, PublishResult, Reach, Subscription, Transport,
    TransportCapabilities, TransportConfiguration, TransportError,
};

use crate::pipeline::StubAdapter;
use crate::token;

/// The bus's `max_payload_octets`.
pub const MAX_FRAME: u64 = 131_072;

/// One payload the bus took.
#[derive(Clone, Debug)]
pub struct Taken {
    /// The publishing device.
    pub from: KeyId,
    /// Its kind.
    pub kind: PayloadKind,
    /// Its destination.
    pub destination: Destination,
    /// Its octets, as every device on the bus received them.
    pub octets: Vec<u8>,
}

#[derive(Default)]
struct BusState {
    devices: Vec<(KeyId, InboundHandler)>,
    watchers: Vec<(KeyId, PresenceHandler)>,
    taken: Vec<Taken>,
    refused: Vec<PayloadKind>,
    send_presence_calls: usize,
    session_subscriptions: usize,
    next_link: u8,
}

/// A transport that delivers every sealed frame to every device on it (see the module
/// documentation).
#[derive(Clone, Default)]
pub struct SealBus(Arc<Mutex<BusState>>);

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

    /// Every payload taken, in order.
    pub fn taken(&self) -> Vec<Taken> {
        self.0.lock().unwrap().taken.clone()
    }

    /// The octets of every payload `from` published.
    pub fn from(&self, from: &KeyId) -> Vec<Vec<u8>> {
        self.taken()
            .into_iter()
            .filter(|t| &t.from == from)
            .map(|t| t.octets)
            .collect()
    }

    /// The kinds of the payloads offered unsealed, which the bus refused, and how many
    /// times `send_presence` and a session `subscribe` were called.
    pub fn unsealed_offers(&self) -> (Vec<PayloadKind>, usize, usize) {
        let st = self.0.lock().unwrap();
        (
            st.refused.clone(),
            st.send_presence_calls,
            st.session_subscriptions,
        )
    }

    /// Hands `octets`, as a payload of `kind`, to every device subscription, as a peer that
    /// publishes them would.
    pub fn inject(&self, kind: PayloadKind, octets: &[u8]) {
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
                payload: Payload::new(kind, octets.to_vec()),
                carrier: CarrierHandle::from_opaque(vec![0xee]),
            });
        }
    }

    /// Hands `octets` to every `watch_presence` handler as a presence record in the clear.
    pub fn inject_presence(&self, octets: &[u8]) {
        let watchers: Vec<PresenceHandler> = self
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

impl SealEndpoint {
    fn me(&self) -> Option<KeyId> {
        self.key.lock().unwrap().clone()
    }
}

impl Transport for SealEndpoint {
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
            destination_restricted: false,
            max_payload_octets: MAX_FRAME,
            sealing: true,
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
        let handlers: Vec<InboundHandler> = {
            let mut st = self.bus.0.lock().unwrap();
            if payload.kind() != PayloadKind::Sealed {
                st.refused.push(payload.kind());
                return PublishResult::NotTaken;
            }
            st.taken.push(Taken {
                from: me,
                kind: payload.kind(),
                destination: destination.clone(),
                octets: payload.octets().to_vec(),
            });
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
        let me = self.me().ok_or(TransportError::NotStarted)?;
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
            st.devices.retain(|(k, _)| k != &me);
            st.watchers.retain(|(k, _)| k != &me);
        }
    }
}

/// A device on a [`SealBus`], with its own agreement keys.
pub struct SealedDevice {
    /// The pipelines.
    pub pipes: Pipelines,
    /// The stub adapter.
    pub adapter: Arc<StubAdapter>,
    adapter_id: AdapterId,
    /// The pairing store the engine saves to.
    pub pairings: MemoryPairingStore,
    /// The store of this device's agreement keys.
    pub agreement_store: MemoryAgreementKeyStore,
}

impl SealedDevice {
    /// A device for `identity` on `bus`: agreement keys made and given to the pipelines,
    /// started, with its stub adapter added.
    pub fn new(bus: &SealBus, identity: DeviceIdentity) -> SealedDevice {
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let engine =
            AuthorizationEngine::new(&identity, clock.clone(), Box::new(MemoryDecisionLog::new()));
        let pipes = Pipelines::new(
            identity,
            engine,
            clock,
            Arc::new(std::time::Instant::now),
            bus.endpoint(),
            PipelineConfig::default(),
        );
        let agreement_store = MemoryAgreementKeyStore::new();
        let keys =
            AgreementKeys::load_or_generate(&agreement_store, pipes.device(), &SystemClock.now())
                .expect("agreement keys");
        pipes.set_agreement_keys(keys).expect("own statement");
        pipes
            .start(TransportConfiguration::new(()))
            .expect("started");
        let adapter = Arc::new(StubAdapter::default());
        let adapter_id = pipes.add_adapter(adapter.clone());
        SealedDevice {
            pipes,
            adapter,
            adapter_id,
            pairings: MemoryPairingStore::new(),
            agreement_store,
        }
    }

    /// This device's key id.
    pub fn key(&self) -> KeyId {
        self.pipes.device().key_id().clone()
    }

    /// A harness session bound to `session`.
    pub fn session(&self, session: &SessionId) -> Attachment {
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
                "/work",
                SystemClock.now(),
            )
            .expect("a record");
        self.pipes.bind(&a, &record, None).expect("bound");
        a
    }

    /// The operator pairs `peer` by comparing its key id; with its agreement statement,
    /// read beside its key file, when `with_statement`.
    pub fn pair(&self, peer: &SealedDevice, with_statement: bool) {
        let mut p = PairedPeer::by_key_id_comparison(
            peer.pipes.device().principal().clone(),
            *peer.pipes.device().public_key(),
            peer.pipes.device().key_id(),
            SystemClock.now(),
            OperatorConfirmed::by_operator(),
        )
        .expect("the key id matches");
        if with_statement {
            p = p.with_agreement_statement(peer.pipes.agreement_statement().expect("keys"));
        }
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

    /// A second handle on this device's agreement keys, read from its store: the test's
    /// own opener, standing for this device's view of a frame.
    pub fn opener(&self) -> AgreementKeys {
        AgreementKeys::load_or_generate(
            &self.agreement_store,
            self.pipes.device(),
            &SystemClock.now(),
        )
        .expect("agreement keys")
    }

    /// This device's current agreement public key.
    pub fn agreement_key(&self) -> AgreementPublicKey {
        let st = self.pipes.agreement_statement().expect("keys");
        AgreementPublicKey::from_base64url(
            st.as_json()
                .get("agreement_key")
                .and_then(Json::as_str)
                .expect("agreement_key"),
        )
        .expect("acceptable")
    }
}
