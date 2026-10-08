// SPDX-License-Identifier: Apache-2.0

//! [`PeerTransport`]: `oac_core::transport::Transport` over a Zenoh peer session, on the
//! stable API's synchronous `Wait` path.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use oac_core::health::{HealthState, HealthStatus};
use oac_core::ids::KeyId;
use oac_core::transport::{
    CarrierHandle, Deadline, Destination, Inbound, InboundHandler, Payload, PayloadKind,
    PresenceHandler, PublishResult, Reach, Subscription, Transport, TransportCapabilities,
    TransportConfiguration, TransportError,
};
use zenoh::Wait;
use zenoh::pubsub::Subscriber;
use zenoh::qos::CongestionControl;
use zenoh::sample::Sample;

use crate::addressing::{self, Digest128, Partition};
use crate::config::{PeerConfiguration, Role};
use crate::frame;
use crate::gate::Gate;
use crate::presence::Watchers;

/// The largest payload this transport takes: 1 MiB. Zenoh fragments larger messages
/// itself; the bound is what the contract suite exercises ([IFC-TRN-023], [IFC-TRN-030]).
pub const MAX_PAYLOAD_OCTETS: u64 = 1 << 20;

/// The transport. Cloning shares one transport.
#[derive(Clone, Default)]
pub struct PeerTransport {
    inner: Arc<Mutex<State>>,
    /// Every native string a start of this transport held, kept after shutdown.
    leak_checks: Arc<Mutex<Vec<String>>>,
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
}

/// The core's subscriptions, by destination digest.
#[derive(Default)]
struct Subs {
    next: AtomicU64,
    map: Mutex<HashMap<Digest128, Vec<Entry>>>,
}

#[derive(Clone)]
struct Entry {
    id: u64,
    accepts: PayloadKind,
    handler: InboundHandler,
    gate: Arc<Gate>,
}

impl Subs {
    fn lock(&self) -> MutexGuard<'_, HashMap<Digest128, Vec<Entry>>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn add(&self, digest: Digest128, accepts: PayloadKind, handler: InboundHandler) -> Entry {
        let e = Entry {
            id: self.next.fetch_add(1, Ordering::Relaxed),
            accepts,
            handler,
            gate: Gate::new(),
        };
        self.lock().entry(digest).or_default().push(e.clone());
        e
    }

    fn remove(&self, digest: &Digest128, id: u64) {
        let mut m = self.lock();
        if let Some(v) = m.get_mut(digest) {
            v.retain(|e| e.id != id);
            if v.is_empty() {
                m.remove(digest);
            }
        }
    }

    fn dispatch(
        &self,
        gate: &Arc<Gate>,
        digest: &Digest128,
        kind: PayloadKind,
        octets: &[u8],
        link: &Digest128,
    ) {
        let entries: Vec<Entry> = match self.lock().get(digest) {
            Some(v) => v.iter().filter(|e| e.accepts == kind).cloned().collect(),
            None => return,
        };
        if entries.is_empty() {
            return;
        }
        let Some(_transport) = gate.enter() else {
            return;
        };
        let payload = Payload::new(kind, octets.to_vec());
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

pub(crate) fn unix_millis_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// The receive path: every frame put on the partition reaches every peer of it; this keeps
/// the ones for this peer's subscriptions and drops the rest.
fn on_sample(
    sample: &Sample,
    partition: &Partition,
    local_digest: &Digest128,
    gate: &Arc<Gate>,
    subs: &Subs,
    watchers: &Watchers,
) {
    let Some(digest) = partition.digest_of(sample.key_expr().as_str()) else {
        return;
    };
    let bytes = sample.payload().to_bytes();
    let Some(f) = frame::decode(&bytes) else {
        return;
    };
    if unix_millis_now() >= f.expiry_unix_millis {
        return;
    }
    match f.kind {
        PayloadKind::Presence => {
            if &digest == local_digest {
                watchers.record(gate, f.octets, CarrierHandle::from_opaque(f.link.to_vec()));
            }
        }
        PayloadKind::Envelope | PayloadKind::Receipt => {
            subs.dispatch(gate, &digest, f.kind, f.octets, &f.link)
        }
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
    /// A diagnostic, for tests that wait for discovery: nothing in the contract uses it.
    pub fn connected_peers(&self) -> usize {
        self.running()
            .map(|r| r.session.info().peers_zid().wait().count())
            .unwrap_or(0)
    }

    /// For leak tests only: the transport-native strings every start of this transport held
    /// (its own peer id, its fixed endpoint if any, its key expression prefix), kept after
    /// shutdown, so that a test can check that none of them reaches the core.
    pub fn identifiers_for_leak_checks(&self) -> Vec<String> {
        self.leak_checks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn open(conf: &PeerConfiguration) -> Result<zenoh::Session, TransportError> {
        let mut last = String::new();
        for role in conf.roles() {
            let native = conf
                .native(*role)
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
        Err(TransportError::InvalidConfiguration(last))
    }

    fn put(
        &self,
        destination: &Destination,
        payload: &Payload,
        deadline: Deadline,
        kinds: &[PayloadKind],
    ) -> PublishResult {
        let Some(r) = self.running() else {
            return PublishResult::NotTaken;
        };
        let kind = payload.kind();
        if !kinds.contains(&kind) || !kind.fits(destination) {
            return PublishResult::NotTaken;
        }
        if u64::try_from(payload.len()).unwrap_or(u64::MAX) > MAX_PAYLOAD_OCTETS {
            return PublishResult::NotTaken;
        }
        let now = Instant::now();
        if deadline.has_passed_at(now) {
            return PublishResult::NotTaken;
        }
        let left =
            u64::try_from(deadline.instant().duration_since(now).as_millis()).unwrap_or(u64::MAX);
        if left == 0 {
            return PublishResult::NotTaken;
        }
        let expiry = unix_millis_now().saturating_add(left);
        let digest = addressing::destination_digest(destination);
        let f = frame::encode(kind, expiry, &r.link, payload.octets());
        match r
            .session
            .put(r.partition.key(&digest), f)
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
        let local_digest = addressing::device_digest(local_device);
        let gate = Gate::new();
        let subs: Arc<Subs> = Arc::default();
        let watchers: Arc<Watchers> = Arc::default();
        let (p, g, s, w) = (
            partition.clone(),
            gate.clone(),
            subs.clone(),
            watchers.clone(),
        );
        let subscriber = match session
            .declare_subscriber(partition.all())
            .callback(move |sample| on_sample(&sample, &p, &local_digest, &g, &s, &w))
            .wait()
        {
            Ok(sub) => sub,
            Err(_) => {
                let _ = session.close().wait();
                return Err(TransportError::InvalidConfiguration(
                    "the peer session refused the subscription".into(),
                ));
            }
        };
        {
            let mut l = self.leak_checks.lock().unwrap_or_else(|e| e.into_inner());
            l.extend([native_id.clone(), partition.all()]);
            l.extend(conf.fixed_endpoint());
        }
        *state = State::Running(Arc::new(Running {
            session,
            subscriber: Mutex::new(Some(subscriber)),
            partition,
            local: local_device.clone(),
            link: addressing::link_octets(&native_id),
            gate,
            subs,
            watchers,
        }));
        Ok(TransportCapabilities {
            reliability: false,
            persistence: false,
            offline_queueing: false,
            ordering: false,
            multicast_discovery: conf.discovers_by_multicast(),
            routing_federation: false,
            reach: Reach::CrossImplementation,
            destination_restricted: false,
            max_payload_octets: MAX_PAYLOAD_OCTETS,
        })
    }

    fn publish(
        &self,
        destination: &Destination,
        payload: Payload,
        deadline: Deadline,
    ) -> PublishResult {
        self.put(
            destination,
            &payload,
            deadline,
            &[PayloadKind::Envelope, PayloadKind::Receipt],
        )
    }

    fn subscribe(
        &self,
        destination: &Destination,
        handler: InboundHandler,
    ) -> Result<Subscription, TransportError> {
        let r = self.running().ok_or(TransportError::NotStarted)?;
        let accepts = match destination {
            Destination::Session(_) => PayloadKind::Envelope,
            Destination::Device(k) if *k == r.local => PayloadKind::Receipt,
            Destination::Device(_) => return Err(TransportError::NotLocalDevice),
        };
        let digest = addressing::destination_digest(destination);
        let entry = r.subs.add(digest, accepts, handler);
        let subs: Weak<Subs> = Arc::downgrade(&r.subs);
        let (gate, id) = (entry.gate, entry.id);
        Ok(Subscription::new(move || {
            gate.close();
            if let Some(s) = subs.upgrade() {
                s.remove(&digest, id);
            }
        }))
    }

    fn send_presence(
        &self,
        destination: &Destination,
        payload: Payload,
        deadline: Deadline,
    ) -> PublishResult {
        // Carried like a receipt, as one whole frame, to the named device (presence.rs).
        self.put(destination, &payload, deadline, &[PayloadKind::Presence])
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
        let sub = r
            .subscriber
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(sub) = sub {
            let _ = sub.undeclare().wait();
        }
        let _ = r.session.close().wait();
    }
}
