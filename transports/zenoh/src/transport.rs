// SPDX-License-Identifier: Apache-2.0

//! [`PeerTransport`]: `oac_core::transport::Transport` over a Zenoh peer session, on the
//! stable API's synchronous `Wait` path.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::{self, JoinHandle, ThreadId};
use std::time::Instant;

use oac_core::health::{HealthState, HealthStatus};
use oac_core::ids::KeyId;
use oac_core::transport::{
    CarrierHandle, Deadline, Destination, Inbound, InboundHandler, Payload, PayloadKind,
    PresenceHandler, PublishResult, Reach, Subscription, Transport, TransportCapabilities,
    TransportConfiguration, TransportError,
};
use zenoh::Wait;
use zenoh::bytes::Encoding;
use zenoh::handlers::{IntoHandler, RingChannel, RingChannelHandler};
use zenoh::pubsub::Subscriber;
use zenoh::qos::CongestionControl;
use zenoh::sample::Sample;

use crate::addressing::{self, Digest128, Partition};
use crate::config::{PeerConfiguration, Role};
use crate::gate::Gate;
use crate::presence::Watchers;

/// The largest payload this transport takes: 1 MiB. Zenoh fragments larger messages
/// itself; the bound is what the contract suite exercises ([IFC-TRN-023], [IFC-TRN-030]).
pub const MAX_PAYLOAD_OCTETS: u64 = 1 << 20;

/// Native RingChannel capacity: at most 16 MiB of admitted frame octets.
/// Overflow drops the oldest frame; a dispatching frame adds at most 1 MiB.
const QUEUE_FRAMES: usize = 16;

/// The transport. Cloning shares one transport.
#[derive(Clone, Default)]
pub struct PeerTransport {
    inner: Arc<Mutex<State>>,
    /// Every native string a start of this transport held, kept after shutdown, for leak
    /// tests (the `test-support` feature).
    #[cfg(feature = "test-support")]
    leak_checks: Arc<Mutex<Vec<String>>>,
    #[cfg(feature = "test-support")]
    carriage: Arc<Mutex<Vec<CarriageObservation>>>,
}

#[derive(Default)]
enum State {
    #[default]
    NotStarted,
    Running(Arc<Running>),
    ShutDown,
}

/// A started transport.
struct Running {
    session: zenoh::Session,
    subscriber: Mutex<Option<Subscriber<()>>>,
    partition: Partition,
    local: KeyId,
    link: Digest128,
    gate: Arc<Gate>,
    subs: Arc<Subs>,
    watchers: Arc<Watchers>,
    dispatcher: Mutex<Option<JoinHandle<()>>>,
    dispatcher_id: ThreadId,
}

/// Only local device subscriptions receive sealed frames. Session entries stay local.
#[derive(Default)]
struct Subs {
    next: AtomicU64,
    map: Mutex<HashMap<Destination, Vec<Entry>>>,
}

#[derive(Clone)]
struct Entry {
    id: u64,
    handler: InboundHandler,
    gate: Arc<Gate>,
}

impl Subs {
    fn lock(&self) -> MutexGuard<'_, HashMap<Destination, Vec<Entry>>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn add(&self, destination: Destination, handler: InboundHandler) -> Entry {
        let e = Entry {
            id: self.next.fetch_add(1, Ordering::Relaxed),
            handler,
            gate: Gate::new(),
        };
        self.lock().entry(destination).or_default().push(e.clone());
        e
    }

    fn remove(&self, destination: &Destination, id: u64) {
        let mut m = self.lock();
        if let Some(v) = m.get_mut(destination) {
            v.retain(|e| e.id != id);
            if v.is_empty() {
                m.remove(destination);
            }
        }
    }

    fn dispatch(
        &self,
        gate: &Arc<Gate>,
        destination: &Destination,
        octets: Vec<u8>,
        link: &Digest128,
    ) {
        let entries = self.lock().get(destination).cloned().unwrap_or_default();
        if entries.is_empty() {
            return;
        }
        let Some(_transport) = gate.enter() else {
            return;
        };
        let payload = Payload::new(PayloadKind::Sealed, octets);
        let carrier = CarrierHandle::from_opaque(link.to_vec());
        for e in entries {
            let Some(_sub) = e.gate.enter() else { continue };
            (e.handler)(Inbound {
                payload: payload.clone(),
                carrier: carrier.clone(),
            });
        }
    }
}

/// Raw sample boundary observations, available only to this binding's tests.
#[cfg(feature = "test-support")]
#[derive(Clone, Debug)]
pub struct CarriageObservation {
    pub frame: Vec<u8>,
    pub accompanying: Vec<Vec<u8>>,
    pub key: String,
    pub encoding: String,
    pub identifiers: Vec<Vec<u8>>,
}

/// Blocking read on the established native stream, not inbox polling. Core handlers
/// never run on Zenoh's receive thread. The native ring owns queuing and overflow.
fn dispatch_loop(
    rx: RingChannelHandler<Vec<u8>>,
    subs: Arc<Subs>,
    gate: Arc<Gate>,
    local: KeyId,
    link: Digest128,
) {
    while let Ok(octets) = rx.recv() {
        subs.dispatch(&gate, &Destination::Device(local.clone()), octets, &link);
    }
}

impl PeerTransport {
    /// A transport that is not started.
    pub fn new() -> PeerTransport {
        PeerTransport::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn running(&self) -> Option<Arc<Running>> {
        match &*self.lock() {
            State::Running(r) => Some(r.clone()),
            _ => None,
        }
    }

    /// The number of other peers this transport has a link to now; 0 when not started.
    /// A diagnostic, for tests that wait for links: nothing in the contract uses it.
    pub fn connected_peers(&self) -> usize {
        self.running()
            .map(|r| {
                let info = r.session.info();
                info.peers_zid().wait().count() + info.routers_zid().wait().count()
            })
            .unwrap_or(0)
    }

    /// For leak tests only, behind the `test-support` feature: the transport-native strings
    /// every start of this transport held (its own peer id, its configured listener and rendezvous
    /// endpoints, its key expression prefix), kept after shutdown, so that a test can check
    /// that none of them reaches the core.
    #[cfg(feature = "test-support")]
    pub fn identifiers_for_leak_checks(&self) -> Vec<String> {
        self.leak_checks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    #[cfg(feature = "test-support")]
    fn capture(
        capture: &Mutex<Vec<CarriageObservation>>,
        frame: &[u8],
        key: &str,
        encoding: &Encoding,
        link: &Digest128,
        accompanying: Vec<Vec<u8>>,
    ) {
        capture
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(CarriageObservation {
                frame: frame.to_vec(),
                accompanying,
                key: key.to_owned(),
                encoding: encoding.to_string(),
                identifiers: vec![link.to_vec()],
            });
    }

    /// Raw samples and carrier bytes for this binding's contract harness only.
    #[cfg(feature = "test-support")]
    pub fn carriage_for_tests(&self) -> Vec<CarriageObservation> {
        let mut observations = self
            .carriage
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let mut identifiers = self.identifiers_for_leak_checks();
        if let Some(r) = self.running() {
            identifiers.extend(r.session.info().peers_zid().wait().map(|id| id.to_string()));
            identifiers.extend(
                r.session
                    .info()
                    .routers_zid()
                    .wait()
                    .map(|id| id.to_string()),
            );
        }
        for observation in &mut observations {
            observation
                .identifiers
                .extend(identifiers.iter().map(|s| s.as_bytes().to_vec()));
        }
        observations
    }

    /// Open the native session; Zenoh allocates the joiner's ephemeral listener.
    fn open(conf: &PeerConfiguration) -> Result<zenoh::Session, TransportError> {
        let mut last = String::new();
        let roles = conf.roles();
        // Something already accepts connections on the port when the joiner side is tried
        // first (`PeerConfiguration::roles`).
        let port_answers = roles[0] == Role::Joiner;
        for role in roles {
            let native = conf
                .native(role, conf.listen_port(role))
                .map_err(TransportError::InvalidConfiguration)?;
            match zenoh::open(native).wait() {
                Ok(s) => return Ok(s),
                Err(_) => {
                    last = match role {
                        Role::First => "the peer session did not open".into(),
                        Role::Joiner => "the peer session could not reach the rendezvous".into(),
                    }
                }
            }
        }
        if port_answers {
            // The port accepts connections, yet neither joining it nor holding it worked:
            // what holds it is not an OAC transport. Name the port (no address: the error
            // may reach a health detail).
            last = format!(
                "the loopback rendezvous port {} is held by a program that is not an OAC transport; \
                 stop that program or configure another rendezvous port",
                conf.rendezvous_port()
            );
        }
        Err(TransportError::InvalidConfiguration(last))
    }

    fn put(
        &self,
        destination: &Destination,
        payload: &Payload,
        deadline: Deadline,
    ) -> PublishResult {
        let Some(r) = self.running() else {
            return PublishResult::NotTaken;
        };
        let kind = payload.kind();
        if kind != PayloadKind::Sealed || !kind.fits(destination) {
            return PublishResult::NotTaken;
        }
        if u64::try_from(payload.len()).unwrap_or(u64::MAX) > MAX_PAYLOAD_OCTETS {
            return PublishResult::NotTaken;
        }
        let key = r.partition.key();
        let frame = payload.octets().to_vec();
        let now = Instant::now();
        if deadline.has_passed_at(now) {
            return PublishResult::NotTaken;
        }
        #[cfg(feature = "test-support")]
        Self::capture(
            &self.carriage,
            &frame,
            &key,
            &Encoding::ZENOH_BYTES,
            &r.link,
            vec![],
        );
        match r
            .session
            .put(key, frame)
            .encoding(Encoding::ZENOH_BYTES)
            .congestion_control(CongestionControl::Block)
            .wait()
        {
            Ok(()) => PublishResult::Taken,
            Err(_) => PublishResult::NotTaken,
        }
    }
}

impl Transport for PeerTransport {
    fn start(
        &self,
        local_device: &KeyId,
        configuration: TransportConfiguration,
    ) -> Result<TransportCapabilities, TransportError> {
        let conf = configuration
            .into_inner::<PeerConfiguration>()
            .map_err(|_| TransportError::InvalidConfiguration("not a peer configuration".into()))?;
        let mut state = self.lock();
        if matches!(*state, State::Running(_)) {
            return Err(TransportError::AlreadyStarted);
        }
        let session = Self::open(&conf)?;
        let native_id = session.zid().to_string();
        let partition = Partition::new(conf.partition());
        let gate = Gate::new();
        let subs: Arc<Subs> = Arc::default();
        let watchers: Arc<Watchers> = Arc::default();
        let link = addressing::link_octets(&native_id);
        let (tx, rx) = RingChannel::new(QUEUE_FRAMES).into_handler();
        let (s, g, local) = (subs.clone(), gate.clone(), local_device.clone());
        let dispatcher = match thread::Builder::new()
            .name("oac-peer-dispatch".into())
            .spawn(move || dispatch_loop(rx, s, g, local, link))
        {
            Ok(h) => h,
            Err(_) => {
                let _ = session.close().wait();
                return Err(TransportError::InvalidConfiguration(
                    "the dispatch thread did not start".into(),
                ));
            }
        };
        let dispatcher_id = dispatcher.thread().id();
        #[cfg(feature = "test-support")]
        let capture = self.carriage.clone();
        let subscriber = match session
            .declare_subscriber(partition.key())
            .callback(move |sample: Sample| {
                #[cfg(feature = "test-support")]
                Self::capture(
                    &capture,
                    &sample.payload().to_bytes(),
                    sample.key_expr().as_str(),
                    sample.encoding(),
                    &link,
                    sample
                        .attachment()
                        .map(|a| a.to_bytes().into_owned())
                        .into_iter()
                        .chain(sample.timestamp().map(|t| t.to_string().into_bytes()))
                        .collect(),
                );
                // Zenoh's handlers bound count, not bytes. Admission precedes the copy;
                // Vec owns only frame octets, rather than retaining native backing buffers.
                if sample.payload().len() <= MAX_PAYLOAD_OCTETS as usize
                    && *sample.encoding() == Encoding::ZENOH_BYTES
                {
                    tx.call(sample.payload().to_bytes().into_owned());
                }
            })
            .wait()
        {
            Ok(sub) => sub,
            Err(_) => {
                let _ = session.close().wait();
                let _ = dispatcher.join();
                return Err(TransportError::InvalidConfiguration(
                    "the peer session refused the subscription".into(),
                ));
            }
        };
        #[cfg(feature = "test-support")]
        {
            let mut l = self.leak_checks.lock().unwrap_or_else(|e| e.into_inner());
            l.extend([
                native_id.clone(),
                partition.prefix().to_owned(),
                partition.key(),
            ]);
            l.push("tcp/127.0.0.1:0".into());
            l.push(format!("tcp/127.0.0.1:{}", conf.rendezvous_port()));
        }
        *state = State::Running(Arc::new(Running {
            session,
            subscriber: Mutex::new(Some(subscriber)),
            partition,
            local: local_device.clone(),
            link,
            gate,
            subs,
            watchers,
            dispatcher: Mutex::new(Some(dispatcher)),
            dispatcher_id,
        }));
        Ok(TransportCapabilities {
            reliability: false,
            persistence: false,
            offline_queueing: false,
            ordering: false,
            multicast_discovery: false,
            routing_federation: false,
            reach: Reach::CrossImplementation,
            destination_restricted: false,
            max_payload_octets: MAX_PAYLOAD_OCTETS,
            sealing: true,
        })
    }

    fn publish(
        &self,
        destination: &Destination,
        payload: Payload,
        deadline: Deadline,
    ) -> PublishResult {
        self.put(destination, &payload, deadline)
    }

    fn subscribe(
        &self,
        destination: &Destination,
        handler: InboundHandler,
    ) -> Result<Subscription, TransportError> {
        let r = self.running().ok_or(TransportError::NotStarted)?;
        if matches!(destination, Destination::Device(k) if *k != r.local) {
            return Err(TransportError::NotLocalDevice);
        }
        let destination = destination.clone();
        let entry = r.subs.add(destination.clone(), handler);
        let subs: Weak<Subs> = Arc::downgrade(&r.subs);
        let (gate, id) = (entry.gate, entry.id);
        Ok(Subscription::new(move || {
            gate.close();
            if let Some(s) = subs.upgrade() {
                s.remove(&destination, id);
            }
        }))
    }

    fn send_presence(
        &self,
        destination: &Destination,
        payload: Payload,
        deadline: Deadline,
    ) -> PublishResult {
        // Plain presence is refused; sealed records use the same device stream.
        self.put(destination, &payload, deadline)
    }

    fn watch_presence(&self, handler: PresenceHandler) -> Result<(), TransportError> {
        let r = self.running().ok_or(TransportError::NotStarted)?;
        r.watchers.add(handler);
        Ok(())
    }

    fn health(&self) -> HealthStatus {
        match &*self.lock() {
            State::NotStarted => HealthStatus::with_detail(HealthState::Unavailable, "not started"),
            State::ShutDown => HealthStatus::with_detail(HealthState::Unavailable, "shut down"),
            State::Running(r) if r.session.is_closed() => {
                HealthStatus::with_detail(HealthState::Unavailable, "peer session closed")
            }
            State::Running(_) => HealthStatus::with_detail(HealthState::Healthy, "running"),
        }
    }

    fn shutdown(&self) {
        let r = {
            let mut s = self.lock();
            match std::mem::take(&mut *s) {
                State::Running(r) => {
                    *s = State::ShutDown;
                    r
                }
                State::NotStarted => return,
                State::ShutDown => {
                    *s = State::ShutDown;
                    return;
                }
            }
        };
        // No handler runs once this returns ([IFC-TRN-071]): close the gate first, then let
        // the native session go.
        r.gate.close();
        #[cfg(feature = "test-support")]
        {
            let mut ids = self.leak_checks.lock().unwrap_or_else(|e| e.into_inner());
            ids.extend(r.session.info().peers_zid().wait().map(|id| id.to_string()));
            ids.extend(
                r.session
                    .info()
                    .routers_zid()
                    .wait()
                    .map(|id| id.to_string()),
            );
        }
        let sub = r
            .subscriber
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(sub) = sub {
            let _ = sub.undeclare().wait();
        }
        let _ = r.session.close().wait();
        // Undeclaring drops the native ring callback, ending the stream; wait for it,
        // unless this is it (a handler that called shutdown).
        let worker = r
            .dispatcher
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(h) = worker
            && thread::current().id() != r.dispatcher_id
        {
            let _ = h.join();
        }
    }
}
