// SPDX-License-Identifier: Apache-2.0

//! The in-memory network: the shared medium that [`MemoryTransport`] endpoints join, its
//! delivery thread, and its clock.
//!
//! Every copy of a payload is bound, when the network takes it, to a subscription (or a
//! presence watch) that exists at that moment. It is dropped when its deadline comes, when
//! its sending or receiving endpoint leaves, or when its subscription ends. That is the
//! whole of what the network holds ([IFC-TRN-033] to [IFC-TRN-036]).
//!
//! [`MemoryTransport`]: crate::MemoryTransport

use std::collections::{BTreeMap, HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle, ThreadId};
use std::time::{Duration, Instant};

use oac_core::ids::KeyId;
use oac_core::transport::{
    CarrierHandle, Deadline, Destination, Inbound, InboundHandler, Payload, PayloadKind,
    PresenceEvent, PresenceHandler, PublishResult, Reach, TransportCapabilities, TransportError,
};

use crate::faults::{FaultInjector, NoFaults, PublishInfo};

/// The default `max_payload_octets`: the floor of [IFC-TRN-023], which is also the default
/// envelope limit of [SC-ENV-004].
pub const DEFAULT_MAX_PAYLOAD_OCTETS: u64 = oac_core::transport::MIN_MAX_PAYLOAD_OCTETS;

pub(crate) type EndpointId = u64;
pub(crate) type SubId = u64;

#[derive(Clone, Copy, PartialEq, Eq)]
enum To {
    Sub(SubId),
    Watchers,
}

enum Item {
    /// One copy of a payload, for every recipient bound to it when it was taken. The list
    /// can be empty: `publish` queues the same items whether or not anything is subscribed
    /// ([IFC-TRN-043]), and an empty copy is dropped when it comes due.
    Copy {
        source: EndpointId,
        recipients: Vec<(EndpointId, To)>,
        payload: Payload,
        deadline: Deadline,
    },
    CarrierLoss {
        source: EndpointId,
        target: EndpointId,
    },
}

impl Item {
    fn expired(&self, now: Instant) -> bool {
        matches!(self, Item::Copy { deadline, .. } if deadline.has_passed_at(now))
    }

    /// Forget `endpoint` as a source or a recipient; false when nothing is left to keep.
    fn forget_endpoint(&mut self, endpoint: EndpointId) -> bool {
        match self {
            Item::Copy {
                source, recipients, ..
            } => {
                recipients.retain(|(target, _)| *target != endpoint);
                *source != endpoint && !recipients.is_empty()
            }
            Item::CarrierLoss { target, .. } => *target != endpoint,
        }
    }

    /// Forget one subscription as a recipient; false when nothing is left to keep.
    fn forget_subscription(&mut self, endpoint: EndpointId, sub: SubId) -> bool {
        match self {
            Item::Copy { recipients, .. } => {
                recipients.retain(|r| *r != (endpoint, To::Sub(sub)));
                !recipients.is_empty()
            }
            Item::CarrierLoss { .. } => true,
        }
    }
}

struct Endpoint {
    key: KeyId,
    subs: HashMap<SubId, (Destination, InboundHandler)>,
    watchers: Vec<PresenceHandler>,
    /// Endpoints that delivered at least one payload here: each is a link whose loss is
    /// reported here when that endpoint leaves ([IFC-TRN-061]).
    links_from: HashSet<EndpointId>,
}

struct State {
    endpoints: HashMap<EndpointId, Endpoint>,
    /// For a local-only network, the one device key it carries for.
    local_key: Option<KeyId>,
    next_id: u64,
    /// Copies in flight and pending carrier-loss events, by (due instant, sequence).
    queue: BTreeMap<(Instant, u64), Item>,
    manual_offset: Duration,
    /// The endpoint, and subscription, whose handler the delivery thread is running.
    delivering: Option<(EndpointId, Option<SubId>)>,
    delivery_thread: Option<ThreadId>,
    closed: bool,
    faults: Box<dyn FaultInjector>,
}

pub(crate) struct Shared {
    state: Mutex<State>,
    /// Wakes the delivery thread.
    wake: Condvar,
    /// Wakes callers waiting for the delivery thread to be idle or to leave a handler.
    idle: Condvar,
    /// Wakes the purge thread, which drops copies at their deadline even while the
    /// delivery thread is inside a handler ([IFC-TRN-034]).
    reap: Condvar,
    reach: Reach,
    max_payload_octets: u64,
    ordering: bool,
    manual_base: Option<Instant>,
}

/// Hand an event to each presence watcher. Each call is isolated, so one watcher that
/// panics does not stop the others.
fn call_each(watchers: &[PresenceHandler], event: impl Fn() -> PresenceEvent) {
    for w in watchers {
        let e = event();
        let _ = catch_unwind(AssertUnwindSafe(|| w(e)));
    }
}

fn carrier(source: EndpointId, target: EndpointId) -> CarrierHandle {
    let mut octets = [0u8; 16];
    octets[..8].copy_from_slice(&source.to_be_bytes());
    octets[8..].copy_from_slice(&target.to_be_bytes());
    CarrierHandle::from_opaque(octets.to_vec())
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn now(&self, st: &State) -> Instant {
        match self.manual_base {
            // `advance` keeps `base + manual_offset` representable.
            Some(base) => base.checked_add(st.manual_offset).unwrap_or(base),
            None => Instant::now(),
        }
    }

    /// Drop every copy whose deadline has come ([IFC-TRN-034]); wake `settle` if any went.
    fn purge_expired(&self, st: &mut State, now: Instant) {
        let before = st.queue.len();
        st.queue.retain(|_, item| !item.expired(now));
        if st.queue.len() != before {
            self.idle.notify_all();
        }
    }

    /// The earliest instant at which something in the queue comes due or expires.
    fn next_event(st: &State) -> Option<Instant> {
        let due = st.queue.keys().next().map(|k| k.0);
        let deadline = Self::next_deadline(st);
        match (due, deadline) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn next_deadline(st: &State) -> Option<Instant> {
        st.queue
            .values()
            .filter_map(|item| match item {
                Item::Copy { deadline, .. } => Some(deadline.instant()),
                Item::CarrierLoss { .. } => None,
            })
            .min()
    }

    /// Wait on `cv` until `at` on the network's clock, or until notified. A manual clock
    /// only moves through `advance`, which notifies.
    fn wait_until<'a>(
        &self,
        cv: &Condvar,
        st: MutexGuard<'a, State>,
        now: Instant,
        at: Option<Instant>,
    ) -> MutexGuard<'a, State> {
        match (self.manual_base, at) {
            (None, Some(at)) => {
                cv.wait_timeout(st, at.saturating_duration_since(now))
                    .unwrap_or_else(|e| e.into_inner())
                    .0
            }
            _ => cv.wait(st).unwrap_or_else(|e| e.into_inner()),
        }
    }

    /// The purge thread: drops each copy when its deadline comes, whether or not the
    /// delivery thread is busy in a handler ([IFC-TRN-034]). It runs no handler.
    fn reap(self: Arc<Shared>) {
        let mut st = self.lock();
        while !st.closed {
            let now = self.now(&st);
            self.purge_expired(&mut st, now);
            let next = Self::next_deadline(&st);
            st = self.wait_until(&self.reap, st, now, next);
        }
    }

    fn on_delivery_thread(&self, st: &State) -> bool {
        st.delivery_thread == Some(thread::current().id())
    }

    pub(crate) fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            // No retransmission: a copy is handed over once or dropped.
            reliability: false,
            // Required absent ([IFC-TRN-026]); nothing outlives an endpoint.
            persistence: false,
            offline_queueing: false,
            ordering: self.ordering,
            // Endpoints are joined explicitly; there is no discovery, and no relay.
            multicast_discovery: false,
            routing_federation: false,
            reach: self.reach,
            // A session subscription can be made on any endpoint (§6.4 forbids revealing
            // whether one exists), so a payload is not restricted to one device's holder.
            destination_restricted: false,
            max_payload_octets: self.max_payload_octets,
        }
    }

    pub(crate) fn join(&self, key: &KeyId) -> Result<EndpointId, TransportError> {
        let mut st = self.lock();
        if self.reach == Reach::LocalOnly && st.local_key.as_ref().is_some_and(|k| k != key) {
            return Err(TransportError::InvalidConfiguration(
                "a local-only network carries payloads for one device key".into(),
            ));
        }
        if st.endpoints.values().any(|e| &e.key == key) {
            return Err(TransportError::InvalidConfiguration(
                "that device key already has a running transport on this network".into(),
            ));
        }
        st.local_key.get_or_insert_with(|| key.clone());
        st.next_id += 1;
        let id = st.next_id;
        st.endpoints.insert(
            id,
            Endpoint {
                key: key.clone(),
                subs: HashMap::new(),
                watchers: Vec::new(),
                links_from: HashSet::new(),
            },
        );
        Ok(id)
    }

    /// The endpoint leaves: every copy it sent or would receive is dropped
    /// ([IFC-TRN-035]), each endpoint that received from it gets a carrier loss, and once
    /// this returns none of its handlers runs again ([IFC-TRN-071]).
    pub(crate) fn leave(&self, id: EndpointId) {
        let mut st = self.lock();
        if st.endpoints.remove(&id).is_none() {
            return;
        }
        st.queue.retain(|_, item| item.forget_endpoint(id));
        let now = self.now(&st);
        let peers: Vec<EndpointId> = st
            .endpoints
            .iter_mut()
            .filter_map(|(peer, e)| e.links_from.remove(&id).then_some(*peer))
            .collect();
        for target in peers {
            st.next_id += 1;
            let seq = st.next_id;
            st.queue
                .insert((now, seq), Item::CarrierLoss { source: id, target });
        }
        self.wake.notify_all();
        if !self.on_delivery_thread(&st) {
            while matches!(st.delivering, Some((e, _)) if e == id) {
                st = self.idle.wait(st).unwrap_or_else(|e| e.into_inner());
            }
        }
    }

    pub(crate) fn publish(
        &self,
        source: EndpointId,
        destination: &Destination,
        payload: Payload,
        deadline: Deadline,
        presence: bool,
    ) -> PublishResult {
        let mut st = self.lock();
        // Each refusal below means no copy can be delivered ([IFC-TRN-031]), and none
        // depends on whether the destination is subscribed ([IFC-TRN-044]).
        if !st.endpoints.contains_key(&source)
            || (payload.kind() == PayloadKind::Presence) != presence
            || !payload.kind().fits(destination)
            || payload.len() as u64 > self.max_payload_octets
        {
            return PublishResult::NotTaken;
        }
        let now = self.now(&st);
        if deadline.has_passed_at(now) {
            return PublishResult::NotTaken;
        }
        let delays = st.faults.copies(PublishInfo {
            destination,
            kind: payload.kind(),
            len: payload.len(),
        });
        // Bind each copy to what can receive it now. Nothing waits for a destination that
        // is not reachable while the payload is in flight ([IFC-TRN-036]). The same items
        // are queued whatever this finds, and no handler runs in this call, so the
        // publisher's call does not depend on subscriptions ([IFC-TRN-043]).
        let mut recipients: Vec<(EndpointId, To)> = Vec::new();
        for (id, e) in &st.endpoints {
            if presence {
                if matches!(destination, Destination::Device(k) if *k == e.key)
                    && !e.watchers.is_empty()
                {
                    recipients.push((*id, To::Watchers));
                }
            } else {
                recipients.extend(
                    e.subs
                        .iter()
                        .filter(|(_, (d, _))| d == destination)
                        .map(|(sub, _)| (*id, To::Sub(*sub))),
                );
            }
        }
        for delay in delays {
            // A delay past every representable instant is past the deadline too.
            let Some(due) = now.checked_add(delay) else {
                continue;
            };
            if deadline.has_passed_at(due) {
                continue; // it would be held to its deadline and dropped ([IFC-TRN-034])
            }
            st.next_id += 1;
            let seq = st.next_id;
            st.queue.insert(
                (due, seq),
                Item::Copy {
                    source,
                    recipients: recipients.clone(),
                    payload: payload.clone(),
                    deadline,
                },
            );
        }
        self.wake.notify_all();
        self.reap.notify_all();
        PublishResult::Taken
    }

    pub(crate) fn subscribe(
        &self,
        endpoint: EndpointId,
        destination: &Destination,
        handler: InboundHandler,
    ) -> Result<SubId, TransportError> {
        let mut st = self.lock();
        st.next_id += 1;
        let sub = st.next_id;
        let e = st
            .endpoints
            .get_mut(&endpoint)
            .ok_or(TransportError::NotStarted)?;
        if matches!(destination, Destination::Device(k) if *k != e.key) {
            return Err(TransportError::NotLocalDevice);
        }
        e.subs.insert(sub, (destination.clone(), handler));
        Ok(sub)
    }

    /// The subscription ends: its copies in flight are dropped, and once this returns its
    /// handler does not run again.
    pub(crate) fn unsubscribe(&self, endpoint: EndpointId, sub: SubId) {
        let mut st = self.lock();
        let Some(e) = st.endpoints.get_mut(&endpoint) else {
            return;
        };
        if e.subs.remove(&sub).is_none() {
            return;
        }
        st.queue
            .retain(|_, item| item.forget_subscription(endpoint, sub));
        if !self.on_delivery_thread(&st) {
            while st.delivering == Some((endpoint, Some(sub))) {
                st = self.idle.wait(st).unwrap_or_else(|e| e.into_inner());
            }
        }
    }

    pub(crate) fn watch(
        &self,
        endpoint: EndpointId,
        handler: PresenceHandler,
    ) -> Result<(), TransportError> {
        let mut st = self.lock();
        let e = st
            .endpoints
            .get_mut(&endpoint)
            .ok_or(TransportError::NotStarted)?;
        e.watchers.push(handler);
        Ok(())
    }

    /// Take the item due under `key` off the queue, for one recipient, and return the call
    /// that hands it over, with the handler it runs. A copy with further recipients goes
    /// back under the same key, so it stays first. `None` when there is nothing to call.
    #[allow(clippy::type_complexity)]
    fn resolve(
        st: &mut State,
        key: (Instant, u64),
    ) -> Option<(Box<dyn FnOnce() + Send>, (EndpointId, Option<SubId>))> {
        match st.queue.remove(&key)? {
            Item::Copy {
                source,
                mut recipients,
                payload,
                deadline,
            } => {
                if recipients.is_empty() {
                    return None;
                }
                let (target, to) = recipients.remove(0);
                if !recipients.is_empty() {
                    st.queue.insert(
                        key,
                        Item::Copy {
                            source,
                            recipients,
                            payload: payload.clone(),
                            deadline,
                        },
                    );
                }
                let e = st.endpoints.get_mut(&target)?;
                let carrier = carrier(source, target);
                match to {
                    To::Sub(sub) => {
                        let handler = e.subs.get(&sub)?.1.clone();
                        e.links_from.insert(source);
                        Some((
                            Box::new(move || handler(Inbound { payload, carrier })),
                            (target, Some(sub)),
                        ))
                    }
                    To::Watchers => {
                        let watchers = e.watchers.clone();
                        e.links_from.insert(source);
                        Some((
                            Box::new(move || {
                                call_each(&watchers, || PresenceEvent::Record {
                                    payload: payload.clone(),
                                    carrier: carrier.clone(),
                                })
                            }),
                            (target, None),
                        ))
                    }
                }
            }
            Item::CarrierLoss { source, target } => {
                let watchers = st.endpoints.get(&target)?.watchers.clone();
                let carrier = carrier(source, target);
                Some((
                    Box::new(move || {
                        call_each(&watchers, || PresenceEvent::CarrierLoss {
                            carrier: carrier.clone(),
                        })
                    }),
                    (target, None),
                ))
            }
        }
    }

    /// The delivery thread: hands each due copy to its handler, on the transport's own
    /// initiative ([IFC-TRN-040], [IFC-TRN-060]), and drops each copy at its deadline
    /// ([IFC-TRN-034]).
    fn run(self: Arc<Shared>) {
        let mut st = self.lock();
        st.delivery_thread = Some(thread::current().id());
        loop {
            if st.closed {
                break;
            }
            let now = self.now(&st);
            self.purge_expired(&mut st, now);
            let first = st.queue.keys().next().copied();
            if let Some(key) = first.filter(|k| k.0 <= now) {
                if let Some((call, running)) = Shared::resolve(&mut st, key) {
                    st.delivering = Some(running);
                    drop(st);
                    // A panicking handler must not stop delivery to everyone else.
                    let _ = catch_unwind(AssertUnwindSafe(call));
                    st = self.lock();
                    st.delivering = None;
                    self.idle.notify_all();
                }
                continue;
            }
            self.idle.notify_all();
            let next = Self::next_event(&st);
            st = self.wait_until(&self.wake, st, now, next);
        }
    }
}

/// Builds a [`MemoryNetwork`].
pub struct MemoryNetworkBuilder {
    reach: Reach,
    max_payload_octets: u64,
    faults: Box<dyn FaultInjector>,
    manual_clock: bool,
}

impl MemoryNetworkBuilder {
    /// `local-only` (the default): the network carries payloads for one device key only,
    /// so within one implementation ([IFC-TRN-002]). `cross-implementation`: endpoints of
    /// several device keys may join, each standing for its own implementation, as the
    /// two-sided tests of the transport suite need.
    pub fn reach(mut self, reach: Reach) -> Self {
        self.reach = reach;
        self
    }

    /// The largest payload carried; at least the floor of [IFC-TRN-023]. A smaller value
    /// is raised to the floor.
    pub fn max_payload_octets(mut self, octets: u64) -> Self {
        self.max_payload_octets = octets.max(DEFAULT_MAX_PAYLOAD_OCTETS);
        self
    }

    /// The fault injector ([`NoFaults`] by default).
    pub fn faults(mut self, faults: impl FaultInjector + 'static) -> Self {
        self.faults = Box::new(faults);
        self
    }

    /// Use a [`ManualClock`] that only [`ManualClock::advance`] moves, in place of the
    /// system's monotonic clock. Deadlines are then read on that clock.
    pub fn manual_clock(mut self) -> Self {
        self.manual_clock = true;
        self
    }

    /// Build the network and start its delivery thread.
    pub fn build(self) -> MemoryNetwork {
        let shared = Arc::new(Shared {
            reach: self.reach,
            max_payload_octets: self.max_payload_octets,
            ordering: self.faults.preserves_order(),
            manual_base: self.manual_clock.then(Instant::now),
            wake: Condvar::new(),
            idle: Condvar::new(),
            reap: Condvar::new(),
            state: Mutex::new(State {
                endpoints: HashMap::new(),
                local_key: None,
                next_id: 0,
                queue: BTreeMap::new(),
                manual_offset: Duration::ZERO,
                delivering: None,
                delivery_thread: None,
                closed: false,
                faults: self.faults,
            }),
        });
        let runner = shared.clone();
        let delivery = thread::Builder::new()
            .name("oac-memory-transport".into())
            .spawn(move || runner.run())
            .expect("spawn the in-memory transport's delivery thread");
        let reaper = shared.clone();
        let purge = thread::Builder::new()
            .name("oac-memory-transport-purge".into())
            .spawn(move || reaper.reap())
            .expect("spawn the in-memory transport's purge thread");
        MemoryNetwork {
            inner: Arc::new(Owner {
                shared,
                delivery: Mutex::new(Some(delivery)),
                purge: Mutex::new(Some(purge)),
            }),
        }
    }
}

struct Owner {
    shared: Arc<Shared>,
    delivery: Mutex<Option<JoinHandle<()>>>,
    purge: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for Owner {
    fn drop(&mut self) {
        let on_delivery_thread = {
            let mut st = self.shared.lock();
            st.closed = true;
            self.shared.on_delivery_thread(&st)
        };
        self.shared.wake.notify_all();
        self.shared.reap.notify_all();
        let take =
            |m: &Mutex<Option<JoinHandle<()>>>| m.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(t) = take(&self.purge) {
            let _ = t.join(); // runs no handler, so never this thread
        }
        if let (Some(t), false) = (take(&self.delivery), on_delivery_thread) {
            let _ = t.join();
        }
    }
}

/// The in-memory medium that [`MemoryTransport`] endpoints join. Cloning shares it. Its
/// delivery thread stops when the last clone, and the last transport started on it, are
/// dropped.
///
/// The test-only views ([`MemoryNetwork::in_flight`], [`MemoryNetwork::settle`]) belong to
/// whoever built the network, not to any endpoint: a transport's core never sees them.
///
/// [`MemoryTransport`]: crate::MemoryTransport
#[derive(Clone)]
pub struct MemoryNetwork {
    inner: Arc<Owner>,
}

impl MemoryNetwork {
    /// A builder: `local-only`, the system clock, no faults, the minimum payload size.
    pub fn builder() -> MemoryNetworkBuilder {
        MemoryNetworkBuilder {
            reach: Reach::LocalOnly,
            max_payload_octets: DEFAULT_MAX_PAYLOAD_OCTETS,
            faults: Box::new(NoFaults),
            manual_clock: false,
        }
    }

    /// A `local-only` network with no faults on the system clock.
    pub fn new() -> MemoryNetwork {
        MemoryNetwork::builder().build()
    }

    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.inner.shared
    }

    /// The capability declaration a transport started on this network returns.
    pub fn capabilities(&self) -> TransportCapabilities {
        self.inner.shared.capabilities()
    }

    /// The network's clock: the instant to compute a `Deadline` from.
    pub fn now(&self) -> Instant {
        let st = self.inner.shared.lock();
        self.inner.shared.now(&st)
    }

    /// The manual clock, if the network was built with one.
    pub fn manual_clock(&self) -> Option<ManualClock> {
        self.inner.shared.manual_base.map(|_| ManualClock {
            shared: self.inner.shared.clone(),
        })
    }

    /// For tests: the number of copies in flight, and of carrier losses not yet reported.
    pub fn in_flight(&self) -> usize {
        self.inner.shared.lock().queue.len()
    }

    /// For tests: wait until nothing is due at the current instant of the network's clock
    /// (every due copy handed over or dropped, every expired copy dropped) and no handler
    /// is running. Called from inside a handler, it returns at once.
    pub fn settle(&self) {
        let shared = &self.inner.shared;
        let mut st = shared.lock();
        if shared.on_delivery_thread(&st) {
            return;
        }
        loop {
            let now = shared.now(&st);
            let busy = st.delivering.is_some()
                || st
                    .queue
                    .iter()
                    .any(|((due, _), item)| *due <= now || item.expired(now));
            if !busy {
                return;
            }
            shared.wake.notify_all();
            st = shared.idle.wait(st).unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl Default for MemoryNetwork {
    fn default() -> Self {
        MemoryNetwork::new()
    }
}

impl std::fmt::Debug for MemoryNetwork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryNetwork")
            .field("capabilities", &self.capabilities())
            .finish()
    }
}

/// A clock that only [`ManualClock::advance`] moves, for tests of deadlines
/// ([IFC-TRN-034]). Obtained from [`MemoryNetwork::manual_clock`].
#[derive(Clone)]
pub struct ManualClock {
    shared: Arc<Shared>,
}

impl ManualClock {
    /// The current instant.
    pub fn now(&self) -> Instant {
        let st = self.shared.lock();
        self.shared.now(&st)
    }

    /// Move the clock forward by `by`, saturating at the latest instant the platform can
    /// represent. Copies whose deadline comes are dropped before this returns
    /// ([IFC-TRN-034]), even while a handler runs. Copies that come due are handed over by
    /// the delivery thread; [`MemoryNetwork::settle`] waits for that.
    pub fn advance(&self, by: Duration) {
        let shared = &self.shared;
        let mut st = shared.lock();
        let base = shared.manual_base.expect("a manual clock has a base");
        let mut offset = st.manual_offset.saturating_add(by);
        // Halve the step until `base + offset` is representable: at most ~100 rounds.
        while base.checked_add(offset).is_none() {
            offset = st.manual_offset + (offset - st.manual_offset) / 2;
        }
        st.manual_offset = offset;
        let now = shared.now(&st);
        shared.purge_expired(&mut st, now);
        shared.wake.notify_all();
        shared.reap.notify_all();
    }
}

impl std::fmt::Debug for ManualClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ManualClock(..)")
    }
}
