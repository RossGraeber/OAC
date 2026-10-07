// SPDX-License-Identifier: Apache-2.0

//! The shared transport contract suite (#59, F10; `tests/protocol/contract/transport/`) run
//! against the in-memory transport, unchanged, under four media: one and two
//! implementations, each with the system clock and no faults, and with a manual clock and
//! scripted faults. This is the last acceptance item of #56 ("passes the transport
//! contract suite").
//!
//! The second half plants contract breaches in wrappers around the in-memory transport and
//! checks that the suite reports each one under its requirement id, so that a pass above
//! means something.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oac_contract_transport::{Fate, FaultControl, Medium, Report, TransportHarness, Verdict, run};
use oac_core::health::HealthStatus;
use oac_core::ids::KeyId;
use oac_core::transport::{
    Deadline, Destination, InboundHandler, Payload, PresenceHandler, PublishResult, Reach,
    Subscription, Transport, TransportCapabilities, TransportConfiguration, TransportError,
};
use oac_transport_memory::{
    MemoryConfiguration, MemoryNetwork, MemoryTransport, NoFaults, ScriptedFaults,
};

// ---- the harness --------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct MemoryHarness {
    reach: Reach,
    scripted: bool,
}

struct MemoryMedium {
    network: MemoryNetwork,
    faults: Option<Faults>,
}

/// The suite's fault control over the network's scripted faults.
struct Faults(ScriptedFaults);

impl FaultControl for Faults {
    fn next(&self, fate: Fate) {
        match fate {
            Fate::Lose => self.0.lose(),
            Fate::Duplicate(n) => self.0.duplicate(n),
            Fate::Delay(d) => self.0.delay(d),
        }
    }
}

impl TransportHarness for MemoryHarness {
    fn name(&self) -> String {
        format!(
            "memory ({}, {})",
            self.reach.as_str(),
            if self.scripted {
                "manual clock, scripted faults"
            } else {
                "system clock, no faults"
            }
        )
    }

    fn medium(&self) -> Box<dyn Medium> {
        Box::new(memory_medium(*self))
    }
}

fn memory_medium(h: MemoryHarness) -> MemoryMedium {
    let b = MemoryNetwork::builder().reach(h.reach);
    if h.scripted {
        let faults = ScriptedFaults::new();
        MemoryMedium {
            network: b.faults(faults.clone()).manual_clock().build(),
            faults: Some(Faults(faults)),
        }
    } else {
        MemoryMedium {
            network: b.faults(NoFaults).build(),
            faults: None,
        }
    }
}

impl Medium for MemoryMedium {
    fn transport(&self) -> Box<dyn Transport> {
        Box::new(MemoryTransport::new())
    }

    fn configuration(&self) -> TransportConfiguration {
        MemoryConfiguration::wrap(&self.network)
    }

    fn now(&self) -> Instant {
        self.network.now()
    }

    fn advance(&self, by: Duration) {
        match self.network.manual_clock() {
            Some(c) => c.advance(by),
            None => std::thread::sleep(by),
        }
    }

    fn settle(&self) {
        self.network.settle();
    }

    fn faults(&self) -> Option<&dyn FaultControl> {
        self.faults.as_ref().map(|f| f as &dyn FaultControl)
    }
}

const MEDIA: [MemoryHarness; 4] = [
    MemoryHarness {
        reach: Reach::LocalOnly,
        scripted: false,
    },
    MemoryHarness {
        reach: Reach::CrossImplementation,
        scripted: false,
    },
    MemoryHarness {
        reach: Reach::LocalOnly,
        scripted: true,
    },
    MemoryHarness {
        reach: Reach::CrossImplementation,
        scripted: true,
    },
];

fn not_applicable(r: &Report) -> Vec<&str> {
    r.rows
        .iter()
        .filter(|x| matches!(x.verdict, Verdict::NotApplicable(_)))
        .map(|x| x.id)
        .collect()
}

#[test]
fn the_memory_transport_passes_the_contract_suite() {
    for h in MEDIA {
        let report = run(&h);
        println!("{report}");
        report.assert_conformant();
        // What is not applicable, and why, is fixed by the medium: destination_restricted is
        // declared absent ([IFC-TRN-080]); subscriptions cannot be revealed to another
        // implementation on a one-implementation medium ([IFC-TRN-043]).
        let mut expected = vec!["IFC-TRN-080"];
        if h.reach == Reach::LocalOnly {
            expected.insert(0, "IFC-TRN-043");
        }
        assert_eq!(not_applicable(&report), expected, "{report}");
    }
}

#[test]
fn every_transport_owned_requirement_has_a_check() {
    // Appendix C of spec/interfaces.md, owner `transport`, area IFC-TRN; and IFC-NEU-003.
    let owned = [
        "IFC-TRN-001",
        "IFC-TRN-003",
        "IFC-TRN-020",
        "IFC-TRN-021",
        "IFC-TRN-023",
        "IFC-TRN-026",
        "IFC-TRN-030",
        "IFC-TRN-031",
        "IFC-TRN-033",
        "IFC-TRN-034",
        "IFC-TRN-035",
        "IFC-TRN-036",
        "IFC-TRN-040",
        "IFC-TRN-043",
        "IFC-TRN-044",
        "IFC-TRN-050",
        "IFC-TRN-060",
        "IFC-TRN-071",
        "IFC-TRN-080",
        "IFC-NEU-003",
    ];
    let report = run(&MEDIA[3]);
    let ids: HashSet<&str> = report.rows.iter().map(|r| r.id).collect();
    for id in owned {
        assert!(ids.contains(id), "no check for {id}");
    }
}

// ---- planted breaches: the suite must catch each -----------------------------------------

/// What a broken wrapper does wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Breach {
    /// Flips the first octet of every inbound payload ([IFC-TRN-030]).
    AltersOctets,
    /// Returns `not-taken` for an envelope while still carrying it ([IFC-TRN-031]).
    LiesNotTaken,
    /// Returns `not-taken` when the destination has no subscription on the medium
    /// ([IFC-TRN-043], [IFC-TRN-044]).
    RevealsSubscriptions,
    /// Keeps every envelope it publishes and hands it to every later subscription for the
    /// same destination ([IFC-TRN-033], [IFC-TRN-036]).
    HoldsCopies,
    /// `shutdown` does nothing ([IFC-TRN-071]).
    IgnoresShutdown,
    /// Declares persistence ([IFC-TRN-026]).
    ClaimsPersistence,
    /// Declares ordering on a medium that reorders ([IFC-TRN-021]).
    ClaimsOrdering,
    /// Drops presence records ([IFC-TRN-050], [IFC-TRN-060]).
    DropsPresence,
    /// Puts a working directory in its health detail ([IFC-TYP-092]).
    LeaksDirectory,
    /// Delivers an envelope late: re-sends it with a deadline an hour away ([IFC-TRN-034]).
    ExtendsDeadlines,
}

#[derive(Default)]
struct Shared {
    subscribed: Mutex<HashSet<String>>,
    kept: Mutex<Vec<(Destination, Payload)>>,
}

struct Broken {
    inner: MemoryTransport,
    breach: Breach,
    shared: Arc<Shared>,
}

fn dest_key(d: &Destination) -> String {
    format!("{d:?}")
}

impl Transport for Broken {
    fn start(
        &self,
        local_device: &KeyId,
        configuration: TransportConfiguration,
    ) -> Result<TransportCapabilities, TransportError> {
        let mut caps = self.inner.start(local_device, configuration)?;
        match self.breach {
            Breach::ClaimsPersistence => caps.persistence = true,
            Breach::ClaimsOrdering => caps.ordering = true,
            _ => {}
        }
        Ok(caps)
    }

    fn publish(&self, d: &Destination, p: Payload, deadline: Deadline) -> PublishResult {
        match self.breach {
            Breach::RevealsSubscriptions
                if !self
                    .shared
                    .subscribed
                    .lock()
                    .unwrap()
                    .contains(&dest_key(d)) =>
            {
                return PublishResult::NotTaken;
            }
            Breach::HoldsCopies => self
                .shared
                .kept
                .lock()
                .unwrap()
                .push((d.clone(), p.clone())),
            Breach::ExtendsDeadlines => {
                let r = self.inner.publish(d, p.clone(), deadline);
                let far = Deadline::at(deadline.instant() + Duration::from_secs(3600));
                self.inner.publish(d, p, far);
                return r;
            }
            _ => {}
        }
        let r = self.inner.publish(d, p, deadline);
        if self.breach == Breach::LiesNotTaken {
            return PublishResult::NotTaken;
        }
        r
    }

    fn subscribe(
        &self,
        d: &Destination,
        handler: InboundHandler,
    ) -> Result<Subscription, TransportError> {
        self.shared.subscribed.lock().unwrap().insert(dest_key(d));
        let handler: InboundHandler = if self.breach == Breach::AltersOctets {
            Arc::new(move |mut i| {
                let mut o = i.payload.octets().to_vec();
                if let Some(x) = o.first_mut() {
                    *x ^= 0xff;
                }
                i.payload = Payload::new(i.payload.kind(), o);
                handler(i)
            })
        } else {
            handler
        };
        if self.breach == Breach::HoldsCopies {
            let kept: Vec<_> = self.shared.kept.lock().unwrap().clone();
            for (kd, p) in kept {
                if &kd == d {
                    handler(oac_core::transport::Inbound {
                        payload: p,
                        carrier: oac_core::transport::CarrierHandle::from_opaque(vec![0]),
                    });
                }
            }
        }
        self.inner.subscribe(d, handler)
    }

    fn send_presence(&self, d: &Destination, p: Payload, deadline: Deadline) -> PublishResult {
        if self.breach == Breach::DropsPresence {
            return PublishResult::Taken;
        }
        self.inner.send_presence(d, p, deadline)
    }

    fn watch_presence(&self, handler: PresenceHandler) -> Result<(), TransportError> {
        self.inner.watch_presence(handler)
    }

    fn health(&self) -> HealthStatus {
        if self.breach == Breach::LeaksDirectory {
            let d = std::env::current_dir().unwrap().display().to_string();
            return HealthStatus::with_detail(oac_core::health::HealthState::Healthy, d);
        }
        self.inner.health()
    }

    fn shutdown(&self) {
        if self.breach != Breach::IgnoresShutdown {
            self.inner.shutdown();
        }
    }
}

struct BrokenHarness {
    base: MemoryHarness,
    breach: Breach,
}

struct BrokenMedium {
    inner: MemoryMedium,
    breach: Breach,
    shared: Arc<Shared>,
}

impl TransportHarness for BrokenHarness {
    fn name(&self) -> String {
        format!("{:?} over {}", self.breach, self.base.name())
    }

    fn medium(&self) -> Box<dyn Medium> {
        Box::new(BrokenMedium {
            inner: memory_medium(self.base),
            breach: self.breach,
            shared: Arc::default(),
        })
    }
}

impl Medium for BrokenMedium {
    fn transport(&self) -> Box<dyn Transport> {
        Box::new(Broken {
            inner: MemoryTransport::new(),
            breach: self.breach,
            shared: self.shared.clone(),
        })
    }
    fn configuration(&self) -> TransportConfiguration {
        self.inner.configuration()
    }
    fn now(&self) -> Instant {
        self.inner.now()
    }
    fn advance(&self, by: Duration) {
        self.inner.advance(by)
    }
    fn settle(&self) {
        self.inner.settle()
    }
    fn faults(&self) -> Option<&dyn FaultControl> {
        self.inner.faults()
    }
}

#[test]
fn the_suite_catches_each_planted_breach() {
    let scripted_cross = MEDIA[3];
    let cases: [(Breach, &[&str]); 10] = [
        (Breach::AltersOctets, &["IFC-TRN-030"]),
        (Breach::LiesNotTaken, &["IFC-TRN-031"]),
        (
            Breach::RevealsSubscriptions,
            &["IFC-TRN-043", "IFC-TRN-044"],
        ),
        (Breach::HoldsCopies, &["IFC-TRN-033", "IFC-TRN-036"]),
        (Breach::IgnoresShutdown, &["IFC-TRN-071"]),
        (Breach::ClaimsPersistence, &["IFC-TRN-026"]),
        (Breach::ClaimsOrdering, &["IFC-TRN-021"]),
        (Breach::DropsPresence, &["IFC-TRN-050", "IFC-TRN-060"]),
        (Breach::LeaksDirectory, &["IFC-TYP-092"]),
        (Breach::ExtendsDeadlines, &["IFC-TRN-034"]),
    ];
    for (breach, ids) in cases {
        let report = run(&BrokenHarness {
            base: scripted_cross,
            breach,
        });
        for id in ids {
            assert!(
                report.failed(id),
                "{breach:?} not caught as {id}:\n{report}"
            );
        }
        assert!(report.failed("IFC-TRN-003"), "{report}");
    }
}

// ---- asynchronous subscriptions (PR #323 review N2) ---------------------------------------

/// A subscription or presence watch waiting for [`Medium::subscribed`].
type Pending = Box<dyn FnOnce() + Send>;

/// A transport whose `subscribe` and `watch_presence` take effect only when the medium's
/// `subscribed` hook runs, as a network transport's interest declarations take effect
/// some time after the call. A suite that published before the hook would lose payloads.
struct Lazy {
    inner: Arc<MemoryTransport>,
    pending: Arc<Mutex<Vec<Pending>>>,
}

impl Transport for Lazy {
    fn start(
        &self,
        local_device: &KeyId,
        configuration: TransportConfiguration,
    ) -> Result<TransportCapabilities, TransportError> {
        self.inner.start(local_device, configuration)
    }
    fn publish(&self, d: &Destination, p: Payload, deadline: Deadline) -> PublishResult {
        self.inner.publish(d, p, deadline)
    }
    fn subscribe(
        &self,
        d: &Destination,
        handler: InboundHandler,
    ) -> Result<Subscription, TransportError> {
        let slot: Arc<Mutex<Option<Subscription>>> = Arc::default();
        let (inner, dest, s) = (self.inner.clone(), d.clone(), slot.clone());
        self.pending.lock().unwrap().push(Box::new(move || {
            if let Ok(sub) = inner.subscribe(&dest, handler) {
                *s.lock().unwrap() = Some(sub);
            }
        }));
        Ok(Subscription::new(move || {
            if let Some(sub) = slot.lock().unwrap().take() {
                sub.end();
            }
        }))
    }
    fn send_presence(&self, d: &Destination, p: Payload, deadline: Deadline) -> PublishResult {
        self.inner.send_presence(d, p, deadline)
    }
    fn watch_presence(&self, handler: PresenceHandler) -> Result<(), TransportError> {
        let inner = self.inner.clone();
        self.pending.lock().unwrap().push(Box::new(move || {
            let _ = inner.watch_presence(handler);
        }));
        Ok(())
    }
    fn health(&self) -> HealthStatus {
        self.inner.health()
    }
    fn shutdown(&self) {
        self.inner.shutdown();
    }
}

struct LazyHarness(MemoryHarness);

struct LazyMedium {
    inner: MemoryMedium,
    pending: Arc<Mutex<Vec<Pending>>>,
}

impl TransportHarness for LazyHarness {
    fn name(&self) -> String {
        format!("asynchronous subscriptions over {}", self.0.name())
    }
    fn medium(&self) -> Box<dyn Medium> {
        Box::new(LazyMedium {
            inner: memory_medium(self.0),
            pending: Arc::default(),
        })
    }
}

impl Medium for LazyMedium {
    fn transport(&self) -> Box<dyn Transport> {
        Box::new(Lazy {
            inner: Arc::new(MemoryTransport::new()),
            pending: self.pending.clone(),
        })
    }
    fn configuration(&self) -> TransportConfiguration {
        self.inner.configuration()
    }
    fn now(&self) -> Instant {
        self.inner.now()
    }
    fn advance(&self, by: Duration) {
        self.inner.advance(by)
    }
    fn settle(&self) {
        self.inner.settle()
    }
    fn subscribed(&self) {
        let pending: Vec<Pending> = std::mem::take(&mut *self.pending.lock().unwrap());
        for p in pending {
            p();
        }
    }
    fn faults(&self) -> Option<&dyn FaultControl> {
        self.inner.faults()
    }
}

#[test]
fn the_suite_waits_for_each_subscription_to_take_effect() {
    // Every check that publishes after subscribing calls the hook first, so a transport
    // whose subscriptions take effect later still passes, unchanged.
    for h in [MEDIA[1], MEDIA[3]] {
        let report = run(&LazyHarness(h));
        report.assert_conformant();
    }
}
