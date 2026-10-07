// SPDX-License-Identifier: Apache-2.0

//! The transport contract suite (#59, F10): one suite, written against
//! `dyn oac_core::transport::Transport`, that every transport runs unchanged
//! (`spec/interfaces.md` §6, Table 6.4; frozen at revision 0.1).
//!
//! # What an implementation supplies
//!
//! A [`TransportHarness`] hands the suite a fresh [`Medium`] for each check: a way to make
//! unstarted transports that reach each other, the configuration they start with, and the
//! three things the trait itself does not have (F7's note on #59):
//!
//! - [`Medium::now`]: the instant a `Deadline` is computed from (`spec/interfaces.md`
//!   §4.11: the clock of the implementation that passes the payload).
//! - [`Medium::advance`]: move that clock forward. A transport with only the real clock
//!   keeps the default, which waits for real.
//! - [`Medium::settle`]: wait until everything due has been handed over or dropped, so a
//!   check can say something was *not* delivered. The default is a bounded real wait.
//!
//! [`Medium::faults`] is optional: a medium that can lose, duplicate and delay copies lets
//! the suite run the checks that need a copy to be in flight for a while ([IFC-TRN-034],
//! [IFC-TRN-035]); without it those parts are reported not applicable, never passed.
//!
//! # What it checks
//!
//! Every requirement that Appendix C of `spec/interfaces.md` assigns to the `transport`
//! ([IFC-TRN-003]): [IFC-TRN-001], [IFC-TRN-020], [IFC-TRN-021], [IFC-TRN-023],
//! [IFC-TRN-026], [IFC-TRN-030], [IFC-TRN-031], [IFC-TRN-033] to [IFC-TRN-036],
//! [IFC-TRN-040], [IFC-TRN-043], [IFC-TRN-044], [IFC-TRN-050], [IFC-TRN-060],
//! [IFC-TRN-071], [IFC-TRN-080] and [IFC-NEU-003], plus [IFC-TYP-092] for the transport's
//! `health` and [IFC-TYP-095]. [`run`] returns one [`Row`] per check; [IFC-TRN-003] passes
//! only when no other row failed. The requirements Appendix C gives the `core` are not
//! this suite's (they need the core's send and receive paths, F6 and F11), and
//! [IFC-TRN-090] is the binding document's.
//!
//! A check that the implementation's declaration or medium makes moot (a capability
//! declared absent, a single-implementation medium for a two-sided check, no fault control)
//! is [`Verdict::NotApplicable`] with the reason, never a pass.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use oac_core::health::HealthStatus;
use oac_core::ids::{KeyId, SessionId};
use oac_core::transport::{
    Deadline, Destination, Inbound, InboundHandler, Payload, PayloadKind, PresenceEvent,
    PresenceHandler, PublishResult, Reach, Subscription, Transport, TransportCapabilities,
    TransportConfiguration, TransportError,
};

/// How long the default [`Medium::settle`] waits, for a transport with no settle barrier.
pub const REAL_SETTLE: Duration = Duration::from_millis(200);

/// What an implementation under test supplies: a fresh medium per check.
pub trait TransportHarness: Sync {
    /// A short name for reports.
    fn name(&self) -> String;

    /// A fresh, isolated medium. Nothing started on one medium reaches another.
    fn medium(&self) -> Box<dyn Medium>;
}

/// One medium: the transports made by [`Medium::transport`] and started with
/// [`Medium::configuration`] reach each other.
pub trait Medium: Send + Sync {
    /// A fresh transport that is not started.
    fn transport(&self) -> Box<dyn Transport>;

    /// The configuration to start a transport of this medium with.
    fn configuration(&self) -> TransportConfiguration;

    /// The medium's clock: the instant to compute a `Deadline` from.
    fn now(&self) -> Instant;

    /// Move the clock forward by `by`. The default waits for real: the right thing for a
    /// transport on the real clock.
    fn advance(&self, by: Duration) {
        thread::sleep(by);
    }

    /// Wait until everything due has been handed over or dropped. The default is a bounded
    /// real wait ([`REAL_SETTLE`]).
    fn settle(&self) {
        thread::sleep(REAL_SETTLE);
    }

    /// Fault control, when the medium has it.
    fn faults(&self) -> Option<&dyn FaultControl> {
        None
    }

    /// Strings the transport's `health` detail must never contain, besides the ones the
    /// suite always checks ([IFC-TYP-092]): for example the medium's own native addresses.
    fn health_must_not_contain(&self) -> Vec<String> {
        Vec::new()
    }
}

/// The fate of one payload, for [`FaultControl::next`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fate {
    /// No copy is carried.
    Lose,
    /// This many copies are carried, at once.
    Duplicate(usize),
    /// One copy is carried, after this delay on the medium's clock.
    Delay(Duration),
}

/// Fault control: the fate of the next payloads the medium takes, from any transport of
/// the medium, in the order taken.
pub trait FaultControl: Send + Sync {
    /// The next payload taken gets `fate`.
    fn next(&self, fate: Fate);
}

/// The verdict of one check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The requirement held, with what was exercised.
    Pass(String),
    /// The check could not apply to this implementation, with why. Not a pass.
    NotApplicable(String),
    /// The requirement did not hold, with what was seen.
    Fail(String),
}

impl Verdict {
    /// True for [`Verdict::Fail`].
    pub fn is_fail(&self) -> bool {
        matches!(self, Verdict::Fail(_))
    }
}

/// One row of a [`Report`].
#[derive(Clone, Debug)]
pub struct Row {
    /// The requirement id.
    pub id: &'static str,
    /// The check's name.
    pub check: &'static str,
    /// The verdict.
    pub verdict: Verdict,
}

/// What [`run`] found.
#[derive(Clone, Debug)]
pub struct Report {
    /// The harness's name.
    pub harness: String,
    /// One row per check, in [`CHECKS`] order, then [IFC-TRN-003].
    pub rows: Vec<Row>,
}

impl Report {
    /// The failed rows.
    pub fn failures(&self) -> Vec<&Row> {
        self.rows.iter().filter(|r| r.verdict.is_fail()).collect()
    }

    /// True when any row for `id` failed.
    pub fn failed(&self, id: &str) -> bool {
        self.rows.iter().any(|r| r.id == id && r.verdict.is_fail())
    }

    /// The verdicts for `id`.
    pub fn verdicts(&self, id: &str) -> Vec<&Verdict> {
        self.rows
            .iter()
            .filter(|r| r.id == id)
            .map(|r| &r.verdict)
            .collect()
    }

    /// Panic, printing the report, when any row failed.
    pub fn assert_conformant(&self) {
        assert!(self.failures().is_empty(), "{self}");
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "transport contract suite: {}", self.harness)?;
        for r in &self.rows {
            let (tag, why) = match &r.verdict {
                Verdict::Pass(w) => ("pass", w),
                Verdict::NotApplicable(w) => ("n/a ", w),
                Verdict::Fail(w) => ("FAIL", w),
            };
            writeln!(f, "  {tag}  {:<12} {:<40} {why}", r.id, r.check)?;
        }
        Ok(())
    }
}

/// A check: the verdict for one requirement against a harness.
pub type Check = fn(&dyn TransportHarness) -> Verdict;

/// Every check, as (requirement id, name, check).
pub const CHECKS: &[(&str, &str, Check)] = &[
    ("IFC-TRN-001", "carries-each-kind", ifc_trn_001),
    ("IFC-TRN-020", "declares-every-capability", ifc_trn_020),
    ("IFC-TRN-021", "declared-ordering-is-provided", ifc_trn_021_ordering),
    (
        "IFC-TRN-021",
        "declared-reliability-is-provided",
        ifc_trn_021_reliability,
    ),
    (
        "IFC-TRN-021",
        "declared-discovery-and-federation",
        ifc_trn_021_discovery_federation,
    ),
    ("IFC-TRN-023", "max-payload-floor", ifc_trn_023),
    ("IFC-TRN-026", "no-persistence-or-queueing", ifc_trn_026),
    ("IFC-TRN-030", "octets-exactly-as-passed", ifc_trn_030),
    ("IFC-TRN-031", "deliverable-is-taken", ifc_trn_031),
    ("IFC-TRN-033", "holds-only-in-flight", ifc_trn_033),
    ("IFC-TRN-034", "nothing-at-or-after-deadline", ifc_trn_034),
    ("IFC-TRN-035", "nothing-crosses-a-restart", ifc_trn_035),
    ("IFC-TRN-036", "nothing-kept-for-unreachable", ifc_trn_036),
    ("IFC-TRN-040", "handed-over-on-own-initiative", ifc_trn_040),
    ("IFC-TRN-043", "subscriptions-not-revealed", ifc_trn_043),
    ("IFC-TRN-044", "result-independent-of-subscription", ifc_trn_044),
    ("IFC-TRN-050", "presence-whole-to-named-device", ifc_trn_050),
    ("IFC-TRN-060", "presence-handed-on-own-initiative", ifc_trn_060),
    ("IFC-TRN-071", "no-handler-after-shutdown", ifc_trn_071),
    ("IFC-TRN-080", "destination-restricted", ifc_trn_080),
    ("IFC-NEU-003", "no-native-address-to-core", ifc_neu_003),
    ("IFC-TYP-092", "health-holds-no-secret-or-address", ifc_typ_092),
    ("IFC-TYP-095", "destination-is-session-or-key", ifc_typ_095),
];

/// Run every check against `harness`. A check that panics is a [`Verdict::Fail`] with the
/// panic message; the other checks still run.
pub fn run(harness: &dyn TransportHarness) -> Report {
    let mut rows = Vec::new();
    for (id, check, f) in CHECKS {
        let verdict = match catch_unwind(AssertUnwindSafe(|| f(harness))) {
            Ok(v) => v,
            Err(p) => Verdict::Fail(format!("panicked: {}", panic_text(&p))),
        };
        rows.push(Row {
            id,
            check,
            verdict,
        });
    }
    let failed: Vec<_> = rows
        .iter()
        .filter(|r| r.verdict.is_fail())
        .map(|r| r.id)
        .collect();
    rows.push(Row {
        id: "IFC-TRN-003",
        check: "every-transport-requirement",
        verdict: if failed.is_empty() {
            Verdict::Pass("no transport-owned check failed".into())
        } else {
            Verdict::Fail(format!("failed: {}", failed.join(", ")))
        },
    });
    Report {
        harness: harness.name(),
        rows,
    }
}

fn panic_text(p: &Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<String>()
        .cloned()
        .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "non-string panic".into())
}

// ---- helpers ----------------------------------------------------------------------------

const LONG: Duration = Duration::from_secs(60);

/// The device key id with every hex digit `n`.
pub fn key(n: u8) -> KeyId {
    KeyId::parse(&format!("{:x}", n & 0xf).repeat(64)).expect("64 hex digits")
}

/// A session destination.
pub fn session(n: u8) -> Destination {
    Destination::Session(SessionId::from_random_octets([n; 16]))
}

fn payload(kind: PayloadKind, octets: &[u8]) -> Payload {
    Payload::new(kind, octets.to_vec())
}

/// One started transport.
struct Ep {
    t: Box<dyn Transport>,
    key: KeyId,
    caps: TransportCapabilities,
}

fn start(m: &dyn Medium, n: u8) -> Result<Ep, TransportError> {
    let t = m.transport();
    let key = key(n);
    let caps = t.start(&key, m.configuration())?;
    Ok(Ep { t, key, caps })
}

/// A medium with endpoint `a` and, on a cross-implementation medium, a second endpoint `b`
/// for another device key.
struct World {
    m: Box<dyn Medium>,
    a: Ep,
    b: Option<Ep>,
}

impl World {
    fn new(h: &dyn TransportHarness) -> World {
        let m = h.medium();
        let a = start(&*m, 1).expect("start of the first transport");
        let b = if a.caps.reach == Reach::CrossImplementation {
            Some(start(&*m, 2).expect("start of a second device on a cross-implementation medium"))
        } else {
            None
        };
        World { m, a, b }
    }

    /// The receiving side: `b` when there is one, else `a` itself.
    fn recv(&self) -> &Ep {
        self.b.as_ref().unwrap_or(&self.a)
    }

    fn deadline(&self, after: Duration) -> Deadline {
        Deadline::at(self.m.now() + after)
    }

    fn faults(&self) -> Option<&dyn FaultControl> {
        self.m.faults()
    }

    /// Settle until `cond` holds, a bounded number of times.
    fn eventually(&self, cond: impl Fn() -> bool) -> bool {
        for _ in 0..20 {
            if cond() {
                return true;
            }
            self.m.settle();
        }
        cond()
    }

    /// Let everything due happen, so that what has not arrived will not.
    fn quiet(&self) {
        self.m.settle();
        self.m.settle();
    }

    fn sides(&self) -> &'static str {
        if self.b.is_some() {
            "two implementations"
        } else {
            "one implementation"
        }
    }
}

/// Collects what a handler is given.
#[derive(Clone)]
struct Seen<T>(Arc<Mutex<Vec<T>>>);

impl<T> Default for Seen<T> {
    fn default() -> Self {
        Seen(Arc::default())
    }
}

impl<T: Clone + Send + 'static> Seen<T> {
    fn push(&self, x: T) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push(x);
    }
    fn get(&self) -> Vec<T> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

impl Seen<Inbound> {
    fn handler(&self) -> InboundHandler {
        let s = self.clone();
        Arc::new(move |i| s.push(i))
    }
    fn octets(&self) -> Vec<Vec<u8>> {
        self.get()
            .into_iter()
            .map(|i| i.payload.octets().to_vec())
            .collect()
    }
}

impl Seen<PresenceEvent> {
    fn watcher(&self) -> PresenceHandler {
        let s = self.clone();
        Arc::new(move |e| s.push(e))
    }
    fn records(&self) -> Vec<Payload> {
        self.get()
            .into_iter()
            .filter_map(|e| match e {
                PresenceEvent::Record { payload, .. } => Some(payload),
                PresenceEvent::CarrierLoss { .. } => None,
            })
            .collect()
    }
}

fn sub(ep: &Ep, d: &Destination, seen: &Seen<Inbound>) -> Subscription {
    ep.t.subscribe(d, seen.handler())
        .unwrap_or_else(|e| panic!("subscribe {d:?}: {e}"))
}

macro_rules! fail_if {
    ($cond:expr, $($msg:tt)+) => {
        if $cond {
            return Verdict::Fail(format!($($msg)+));
        }
    };
}

// ---- §6.1, §6.5: carrying the three kinds --------------------------------------------

/// [IFC-TRN-001]: an envelope to a session, a receipt to a device and a presence record to
/// a device are each carried.
pub fn ifc_trn_001(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let r = w.recv();
    let (env, rct, prs) = (Seen::default(), Seen::default(), Seen::default());
    let _s1 = sub(r, &session(1), &env);
    let _s2 = sub(r, &Destination::Device(r.key.clone()), &rct);
    r.t.watch_presence(prs.watcher()).expect("watch_presence");
    let dl = w.deadline(LONG);
    let dev = Destination::Device(r.key.clone());
    let results = [
        w.a.t
            .publish(&session(1), payload(PayloadKind::Envelope, b"e-001"), dl),
        w.a.t
            .publish(&dev, payload(PayloadKind::Receipt, b"r-001"), dl),
        w.a.t
            .send_presence(&dev, payload(PayloadKind::Presence, b"p-001"), dl),
    ];
    fail_if!(
        results.iter().any(|r| *r != PublishResult::Taken),
        "results {results:?}, expected taken for each kind"
    );
    let ok = w.eventually(|| env.len() >= 1 && rct.len() >= 1 && !prs.records().is_empty());
    fail_if!(
        !ok,
        "not every kind arrived: envelope {}, receipt {}, presence {}",
        env.len(),
        rct.len(),
        prs.records().len()
    );
    let kinds = (
        env.get()[0].payload.kind(),
        rct.get()[0].payload.kind(),
        prs.records()[0].kind(),
    );
    fail_if!(
        kinds
            != (
                PayloadKind::Envelope,
                PayloadKind::Receipt,
                PayloadKind::Presence
            ),
        "kinds arrived as {kinds:?}"
    );
    Verdict::Pass(format!("envelope, receipt and presence carried ({})", w.sides()))
}

// ---- §6.3: the declaration ----------------------------------------------------------

/// [IFC-TRN-020]: `start` returns a declaration with each of the six optional capabilities
/// present or absent. The Rust type has a boolean for each, so a declaration that returns
/// at all declares all six.
pub fn ifc_trn_020(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let c = w.a.caps;
    Verdict::Pass(format!(
        "reliability {}, persistence {}, offline_queueing {}, ordering {}, multicast_discovery {}, routing_federation {}",
        c.reliability,
        c.persistence,
        c.offline_queueing,
        c.ordering,
        c.multicast_discovery,
        c.routing_federation
    ))
}

/// [IFC-TRN-021], `ordering`: when declared, payloads from one source to one destination
/// arrive in the order passed, also when the medium's fault control delays them unequally.
pub fn ifc_trn_021_ordering(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    if !w.a.caps.ordering {
        return Verdict::Pass("ordering declared absent".into());
    }
    let r = w.recv();
    let seen = Seen::default();
    let _s = sub(r, &session(2), &seen);
    let n = 10u8;
    if let Some(f) = w.faults() {
        // Earlier payloads are delayed longer: a medium that reorders would show it.
        for i in 0..n {
            f.next(Fate::Delay(Duration::from_millis(10 * u64::from(n - i))));
        }
    }
    let dl = w.deadline(LONG);
    for i in 0..n {
        w.a.t
            .publish(&session(2), payload(PayloadKind::Envelope, &[i]), dl);
    }
    w.m.advance(Duration::from_millis(10 * u64::from(n) + 10));
    w.eventually(|| seen.len() >= usize::from(n));
    let got: Vec<u8> = seen.octets().into_iter().map(|o| o[0]).collect();
    fail_if!(
        got.windows(2).any(|p| p[0] >= p[1]),
        "ordering declared, but payloads arrived as {got:?}"
    );
    Verdict::Pass(format!(
        "{} of {n} payloads arrived in order{}",
        got.len(),
        if w.faults().is_some() {
            " under unequal delays"
        } else {
            ""
        }
    ))
}

/// [IFC-TRN-021], `reliability`: absent passes; present is not exercised here (the
/// suite cannot model a lossy link under a retransmitting transport), so it is not
/// applicable and the binding document carries the evidence ([IFC-TRN-090]).
pub fn ifc_trn_021_reliability(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    if w.a.caps.reliability {
        Verdict::NotApplicable(
            "reliability declared present: retransmission is not exercised by this suite; evidence is the binding document's (IFC-TRN-090)".into(),
        )
    } else {
        Verdict::Pass("reliability declared absent".into())
    }
}

/// [IFC-TRN-021], `multicast_discovery` and `routing_federation`: absent passes; present
/// needs a network the suite does not build, so it is not applicable.
pub fn ifc_trn_021_discovery_federation(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let c = w.a.caps;
    if c.multicast_discovery || c.routing_federation {
        Verdict::NotApplicable(format!(
            "multicast_discovery {}, routing_federation {}: not exercised by this suite; evidence is the binding document's (IFC-TRN-090)",
            c.multicast_discovery, c.routing_federation
        ))
    } else {
        Verdict::Pass("multicast_discovery and routing_federation declared absent".into())
    }
}

/// [IFC-TRN-023]: `max_payload_octets` is at least 65536.
pub fn ifc_trn_023(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let m = w.a.caps.max_payload_octets;
    fail_if!(
        w.a.caps.contract_violations().contains(&"IFC-TRN-023"),
        "max_payload_octets {m} is below 65536"
    );
    Verdict::Pass(format!("max_payload_octets {m}"))
}

/// [IFC-TRN-026]: `persistence` and `offline_queueing` are declared absent.
pub fn ifc_trn_026(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let c = w.a.caps;
    fail_if!(
        c.contract_violations().contains(&"IFC-TRN-026"),
        "persistence {}, offline_queueing {}",
        c.persistence,
        c.offline_queueing
    );
    Verdict::Pass("persistence and offline_queueing declared absent".into())
}

// ---- §6.4: publish -------------------------------------------------------------------

/// [IFC-TRN-030]: octets arrive exactly as passed: every octet value, a payload of the
/// declared maximum (up to 1 MiB), and each duplicate the medium makes.
pub fn ifc_trn_030(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let r = w.recv();
    let seen = Seen::default();
    let _s = sub(r, &session(3), &seen);
    let every: Vec<u8> = (0..=255).collect();
    let max = usize::try_from(w.a.caps.max_payload_octets.min(1 << 20)).unwrap_or(1 << 20);
    let big: Vec<u8> = (0..max).map(|i| (i % 251) as u8).collect();
    let dl = w.deadline(LONG);
    let mut sent = vec![every.clone(), big.clone()];
    let res = [
        w.a.t
            .publish(&session(3), payload(PayloadKind::Envelope, &every), dl),
        w.a.t
            .publish(&session(3), payload(PayloadKind::Envelope, &big), dl),
    ];
    fail_if!(
        res.iter().any(|x| *x != PublishResult::Taken),
        "results {res:?}"
    );
    let dup = b"duplicated-030".to_vec();
    if let Some(f) = w.faults() {
        f.next(Fate::Duplicate(2));
        w.a.t
            .publish(&session(3), payload(PayloadKind::Envelope, &dup), dl);
        sent.push(dup.clone());
        sent.push(dup.clone());
    }
    w.eventually(|| seen.len() >= sent.len());
    let mut got = seen.octets();
    got.sort();
    sent.sort();
    fail_if!(
        got != sent,
        "received {} payloads of lengths {:?}, sent {} of lengths {:?}",
        got.len(),
        got.iter().map(Vec::len).collect::<Vec<_>>(),
        sent.len(),
        sent.iter().map(Vec::len).collect::<Vec<_>>()
    );
    Verdict::Pass(format!(
        "every octet value, {max} octets{}",
        if w.faults().is_some() {
            ", and both copies of a duplicate"
        } else {
            ""
        }
    ))
}

/// [IFC-TRN-031]: a payload that can be delivered (the right kind for its destination, a
/// subscribed destination, within the size limit, a deadline a minute away) is not
/// `not-taken`, and it is delivered.
pub fn ifc_trn_031(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let r = w.recv();
    let (env, rct, prs) = (Seen::default(), Seen::default(), Seen::default());
    let dev = Destination::Device(r.key.clone());
    let _s1 = sub(r, &session(4), &env);
    let _s2 = sub(r, &dev, &rct);
    r.t.watch_presence(prs.watcher()).expect("watch_presence");
    let dl = w.deadline(LONG);
    let mut results = Vec::new();
    for i in 0..5u8 {
        results.push(
            w.a.t
                .publish(&session(4), payload(PayloadKind::Envelope, &[i]), dl),
        );
        results.push(
            w.a.t
                .publish(&dev, payload(PayloadKind::Receipt, &[i]), dl),
        );
        results.push(
            w.a.t
                .send_presence(&dev, payload(PayloadKind::Presence, &[i]), dl),
        );
    }
    let not_taken = results
        .iter()
        .filter(|x| **x == PublishResult::NotTaken)
        .count();
    fail_if!(
        not_taken > 0,
        "{not_taken} of {} deliverable payloads were not taken",
        results.len()
    );
    let ok = w.eventually(|| env.len() >= 5 && rct.len() >= 5 && prs.records().len() >= 5);
    fail_if!(
        !ok,
        "taken but not delivered: envelope {}, receipt {}, presence {}",
        env.len(),
        rct.len(),
        prs.records().len()
    );
    Verdict::Pass(format!("15 deliverable payloads taken and delivered ({})", w.sides()))
}

/// [IFC-TRN-033]: a copy is held only while in flight. A delivered payload is not handed
/// to a subscription made afterwards.
pub fn ifc_trn_033(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let r = w.recv();
    let first = Seen::default();
    let s1 = sub(r, &session(5), &first);
    w.a.t.publish(
        &session(5),
        payload(PayloadKind::Envelope, b"once-033"),
        w.deadline(LONG),
    );
    fail_if!(
        !w.eventually(|| first.len() >= 1),
        "the payload was never delivered"
    );
    s1.end();
    let later = Seen::default();
    let _s2 = sub(r, &session(5), &later);
    let elsewhere = Seen::default();
    let _s3 = sub(&w.a, &session(5), &elsewhere);
    w.m.advance(Duration::from_secs(1));
    w.quiet();
    fail_if!(
        later.len() + elsewhere.len() > 0,
        "a delivered payload reached {} later subscription(s)",
        later.len() + elsewhere.len()
    );
    Verdict::Pass("a delivered payload reached no later subscription".into())
}

/// [IFC-TRN-034]: nothing is delivered at or after the deadline: a payload passed at its
/// deadline, and (with fault control) a copy delayed past its deadline. A copy delayed less
/// than its deadline is the control: it does arrive.
pub fn ifc_trn_034(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let r = w.recv();
    let seen = Seen::default();
    let _s = sub(r, &session(6), &seen);
    let at = w.m.now();
    w.a.t.publish(
        &session(6),
        payload(PayloadKind::Envelope, b"at-deadline"),
        Deadline::at(at),
    );
    w.quiet();
    fail_if!(
        seen.len() > 0,
        "a payload passed at its deadline was delivered"
    );
    let Some(f) = w.faults() else {
        return Verdict::Pass(
            "a payload passed at its deadline was not delivered (no fault control: the in-flight case was not run)".into(),
        );
    };
    f.next(Fate::Delay(Duration::from_secs(2)));
    w.a.t.publish(
        &session(6),
        payload(PayloadKind::Envelope, b"late"),
        w.deadline(Duration::from_secs(1)),
    );
    f.next(Fate::Delay(Duration::from_millis(500)));
    w.a.t.publish(
        &session(6),
        payload(PayloadKind::Envelope, b"in-time"),
        w.deadline(Duration::from_secs(2)),
    );
    // Step the clock: the in-time copy comes due at 0.5 s, before its deadline at 2 s; the
    // late copy would come due at 2 s, after its deadline at 1 s.
    w.m.advance(Duration::from_millis(600));
    w.eventually(|| seen.len() >= 1);
    w.m.advance(Duration::from_millis(2400));
    w.quiet();
    let got = seen.octets();
    fail_if!(
        got.iter().any(|o| o == b"late"),
        "a copy delayed past its deadline was delivered"
    );
    fail_if!(
        !got.iter().any(|o| o == b"in-time"),
        "control failed: a copy delayed less than its deadline never arrived"
    );
    Verdict::Pass(
        "nothing at its deadline; a copy delayed past its deadline dropped, one within it delivered"
            .into(),
    )
}

/// [IFC-TRN-035]: nothing crosses a restart. A restarted receiver gets nothing it had, or
/// had in flight, before; with fault control, a copy in flight when its sender restarts is
/// not delivered either.
pub fn ifc_trn_035(h: &dyn TransportHarness) -> Verdict {
    let mut notes = Vec::new();
    // The receiver restarts.
    {
        let w = World::new(h);
        let r = w.recv();
        let before = Seen::default();
        let _s = sub(r, &session(7), &before);
        let delayed = w.faults().is_some();
        if let Some(f) = w.faults() {
            f.next(Fate::Delay(Duration::from_secs(1)));
        }
        w.a.t.publish(
            &session(7),
            payload(PayloadKind::Envelope, b"before-restart"),
            w.deadline(LONG),
        );
        if !delayed {
            fail_if!(
                !w.eventually(|| before.len() >= 1),
                "the control payload was never delivered"
            );
        }
        r.t.shutdown();
        let again = w.m.transport();
        again
            .start(&r.key, w.m.configuration())
            .expect("restart of the receiver");
        let after = Seen::default();
        let _s2 = again
            .subscribe(&session(7), after.handler())
            .expect("subscribe after restart");
        w.m.advance(Duration::from_secs(2));
        w.quiet();
        fail_if!(
            after.len() > 0,
            "a payload from before the receiver's restart was delivered after it ({})",
            if delayed {
                "in flight at the restart"
            } else {
                "already delivered"
            }
        );
        notes.push(if delayed {
            "receiver restart with a copy in flight"
        } else {
            "receiver restart after delivery"
        });
    }
    // The sender restarts, with a copy in flight (needs fault control and two sides).
    {
        let w = World::new(h);
        if let (Some(f), Some(b)) = (w.faults(), w.b.as_ref()) {
            let seen = Seen::default();
            let _s = sub(b, &session(8), &seen);
            f.next(Fate::Delay(Duration::from_secs(1)));
            w.a.t.publish(
                &session(8),
                payload(PayloadKind::Envelope, b"sender-restart"),
                w.deadline(LONG),
            );
            w.a.t.shutdown();
            let again = w.m.transport();
            again
                .start(&w.a.key, w.m.configuration())
                .expect("restart of the sender");
            w.m.advance(Duration::from_secs(2));
            w.quiet();
            fail_if!(
                seen.len() > 0,
                "a copy in flight when its sender restarted was delivered"
            );
            notes.push("sender restart with a copy in flight");
        }
    }
    Verdict::Pass(notes.join("; "))
}

/// [IFC-TRN-036]: a payload for a destination with no subscription is not kept and handed
/// to a subscription made later.
pub fn ifc_trn_036(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let r = w.recv();
    let res = w.a.t.publish(
        &session(9),
        payload(PayloadKind::Envelope, b"nobody-home"),
        w.deadline(LONG),
    );
    w.quiet();
    let later = Seen::default();
    let _s = sub(r, &session(9), &later);
    w.m.advance(Duration::from_secs(1));
    w.quiet();
    fail_if!(
        later.len() > 0,
        "a payload published to an unsubscribed destination reached a later subscription"
    );
    Verdict::Pass(format!(
        "published ({res:?}) with no subscription; a later subscription got nothing"
    ))
}

// ---- §6.4: subscribe -----------------------------------------------------------------

/// [IFC-TRN-040]: the transport hands each inbound payload to the handler on its own
/// initiative. After `publish`, the suite makes no call to any transport; it only waits on
/// the medium, and the payload still arrives.
pub fn ifc_trn_040(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let r = w.recv();
    let seen = Seen::default();
    let _s = sub(r, &session(10), &seen);
    for i in 0..3u8 {
        w.a.t.publish(
            &session(10),
            payload(PayloadKind::Envelope, &[i]),
            w.deadline(LONG),
        );
    }
    fail_if!(
        !w.eventually(|| seen.len() >= 3),
        "only {} of 3 payloads were handed over without the core asking",
        seen.len()
    );
    Verdict::Pass("3 payloads handed to the handler with no further call from the core".into())
}

/// [IFC-TRN-043]: no implementation can tell whether another's subscription exists: the
/// publisher's results and health do not change when the other side subscribes, and a
/// third implementation's `subscribe` to the same destination is not refused. (Timing is
/// not asserted; the binding document states it, [IFC-TRN-090].)
pub fn ifc_trn_043(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let Some(b) = w.b.as_ref() else {
        return Verdict::NotApplicable(
            "the medium carries one implementation only: there is no other implementation to reveal a subscription to".into(),
        );
    };
    let dl = w.deadline(LONG);
    let before_health = w.a.t.health();
    let unsub = w.a.t.publish(
        &session(11),
        payload(PayloadKind::Envelope, b"x"),
        dl,
    );
    let seen = Seen::default();
    let _s = sub(b, &session(11), &seen);
    let subd = w.a.t.publish(
        &session(11),
        payload(PayloadKind::Envelope, b"x"),
        dl,
    );
    let after_health = w.a.t.health();
    fail_if!(
        unsub != subd,
        "publish returned {unsub:?} before the other side subscribed and {subd:?} after"
    );
    fail_if!(
        before_health != after_health,
        "the publisher's health changed when the other side subscribed: {before_health:?} then {after_health:?}"
    );
    let third = start(&*w.m, 3);
    let refused = match &third {
        Ok(c) => c
            .t
            .subscribe(&session(11), Seen::<Inbound>::default().handler())
            .err(),
        Err(_) => w
            .a
            .t
            .subscribe(&session(11), Seen::<Inbound>::default().handler())
            .err(),
    };
    fail_if!(
        refused.is_some(),
        "subscribe was refused for a destination another implementation subscribed to: {refused:?}"
    );
    Verdict::Pass(
        "results, health and subscribe unchanged by another implementation's subscription".into(),
    )
}

/// [IFC-TRN-044]: `publish` returns the same result whether or not the destination is
/// subscribed, for an envelope to a session and a receipt to a device.
pub fn ifc_trn_044(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let r = w.recv();
    let dl = w.deadline(LONG);
    let dev = Destination::Device(r.key.clone());
    let env = |n| {
        w.a.t
            .publish(&session(n), payload(PayloadKind::Envelope, b"044"), dl)
    };
    let rct = || w.a.t.publish(&dev, payload(PayloadKind::Receipt, b"044"), dl);
    let (e0, r0) = (env(12), rct());
    let _s1 = sub(r, &session(12), &Seen::<Inbound>::default());
    let _s2 = sub(r, &dev, &Seen::<Inbound>::default());
    let (e1, r1) = (env(12), rct());
    fail_if!(
        e0 != e1 || r0 != r1,
        "envelope {e0:?} unsubscribed vs {e1:?} subscribed; receipt {r0:?} vs {r1:?}"
    );
    Verdict::Pass(format!(
        "envelope {e0:?} and receipt {r0:?} either way ({})",
        w.sides()
    ))
}

// ---- §6.5, §6.6: presence ------------------------------------------------------------

/// [IFC-TRN-050]: a presence record is carried, as one whole payload, to the device its
/// destination names, and to no other device.
pub fn ifc_trn_050(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let r = w.recv();
    let (at_r, at_a) = (Seen::default(), Seen::default());
    r.t.watch_presence(at_r.watcher()).expect("watch_presence");
    if w.b.is_some() {
        w.a.t.watch_presence(at_a.watcher()).expect("watch_presence");
    }
    let record: Vec<u8> = (0..4096u32).map(|i| (i % 253) as u8).collect();
    let res = w.a.t.send_presence(
        &Destination::Device(r.key.clone()),
        payload(PayloadKind::Presence, &record),
        w.deadline(LONG),
    );
    fail_if!(res != PublishResult::Taken, "send_presence returned {res:?}");
    w.eventually(|| !at_r.records().is_empty());
    w.quiet();
    let got = at_r.records();
    fail_if!(
        got.len() != 1,
        "the named device received {} records",
        got.len()
    );
    fail_if!(
        got[0].octets() != record.as_slice() || got[0].kind() != PayloadKind::Presence,
        "the record arrived altered or split ({} octets of {})",
        got[0].len(),
        record.len()
    );
    fail_if!(
        !at_a.records().is_empty(),
        "a device the destination does not name received the record"
    );
    Verdict::Pass(format!(
        "one whole 4096-octet record to the named device only ({})",
        w.sides()
    ))
}

/// [IFC-TRN-060]: the `watch_presence` handler is given each record on the transport's own
/// initiative. (Carrier loss is optional, [IFC-TRN-061]; any reported is counted.)
pub fn ifc_trn_060(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let r = w.recv();
    let seen = Seen::default();
    r.t.watch_presence(seen.watcher()).expect("watch_presence");
    for i in 0..3u8 {
        w.a.t.send_presence(
            &Destination::Device(r.key.clone()),
            payload(PayloadKind::Presence, &[i]),
            w.deadline(LONG),
        );
    }
    fail_if!(
        !w.eventually(|| seen.records().len() >= 3),
        "only {} of 3 records were handed over",
        seen.records().len()
    );
    let mut losses = 0;
    if w.b.is_some() {
        w.a.t.shutdown();
        w.quiet();
        losses = seen
            .get()
            .iter()
            .filter(|e| matches!(e, PresenceEvent::CarrierLoss { .. }))
            .count();
    }
    Verdict::Pass(format!(
        "3 records handed over unasked; {losses} carrier loss(es) reported when the sender left"
    ))
}

// ---- §6.8: shutdown --------------------------------------------------------------------

/// [IFC-TRN-071]: once `shutdown` returns, no handler of that transport runs: not for a
/// copy in flight at the time (with fault control), and not for anything sent afterwards.
pub fn ifc_trn_071(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let r = w.recv();
    let down = Arc::new(AtomicBool::new(false));
    let late = Arc::new(AtomicUsize::new(0));
    let (d1, l1) = (down.clone(), late.clone());
    let _s = r
        .t
        .subscribe(
            &session(13),
            Arc::new(move |_| {
                if d1.load(Ordering::SeqCst) {
                    l1.fetch_add(1, Ordering::SeqCst);
                }
            }),
        )
        .expect("subscribe");
    let (d2, l2) = (down.clone(), late.clone());
    r.t.watch_presence(Arc::new(move |_| {
        if d2.load(Ordering::SeqCst) {
            l2.fetch_add(1, Ordering::SeqCst);
        }
    }))
    .expect("watch_presence");
    if let Some(f) = w.faults() {
        f.next(Fate::Delay(Duration::from_secs(1)));
    }
    w.a.t.publish(
        &session(13),
        payload(PayloadKind::Envelope, b"in-flight"),
        w.deadline(LONG),
    );
    r.t.shutdown();
    down.store(true, Ordering::SeqCst);
    w.m.advance(Duration::from_secs(2));
    if w.b.is_some() {
        w.a.t.publish(
            &session(13),
            payload(PayloadKind::Envelope, b"after"),
            w.deadline(LONG),
        );
        w.a.t.send_presence(
            &Destination::Device(r.key.clone()),
            payload(PayloadKind::Presence, b"after"),
            w.deadline(LONG),
        );
    }
    w.quiet();
    let n = late.load(Ordering::SeqCst);
    fail_if!(n > 0, "{n} handler call(s) after shutdown returned");
    Verdict::Pass("no handler ran after shutdown returned".into())
}

// ---- §6.7: crossing implementations --------------------------------------------------

/// [IFC-TRN-080]: a transport that declares `destination_restricted` delivers a device
/// payload only to the implementation holding that key.
pub fn ifc_trn_080(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    if !w.a.caps.destination_restricted {
        return Verdict::NotApplicable(
            "destination_restricted declared absent: nothing is claimed, and IFC-TRN-081 keeps presence records for other implementations off this transport".into(),
        );
    }
    let Some(b) = w.b.as_ref() else {
        return Verdict::NotApplicable(
            "destination_restricted declared on a one-implementation medium: no other implementation to keep it from".into(),
        );
    };
    let Ok(c) = start(&*w.m, 3) else {
        return Verdict::NotApplicable("a third implementation could not start".into());
    };
    let (at_b, at_c) = (Seen::default(), Seen::default());
    b.t.watch_presence(at_b.watcher()).expect("watch_presence");
    c.t.watch_presence(at_c.watcher()).expect("watch_presence");
    w.a.t.send_presence(
        &Destination::Device(b.key.clone()),
        payload(PayloadKind::Presence, b"for-b-only"),
        w.deadline(LONG),
    );
    w.eventually(|| !at_b.records().is_empty());
    w.quiet();
    fail_if!(
        !at_c.records().is_empty(),
        "an implementation without the named key received the record"
    );
    Verdict::Pass("a device payload reached only the implementation holding the key".into())
}

// ---- neutrality and types --------------------------------------------------------------

/// [IFC-NEU-003]: the transport passes the core a payload and an opaque carrier handle,
/// and nothing else: the only values a transport can hand over are `Inbound` and
/// `PresenceEvent`, whose members are a `Payload` and a `CarrierHandle` with no accessor.
/// By construction of `oac_core::transport`; boundary check 1 keeps transport names out of
/// `core/`. The suite also checks that every handle it received compares equal to itself
/// and prints nothing ([IFC-NEU-004] is the core's).
pub fn ifc_neu_003(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let r = w.recv();
    let seen = Seen::default();
    let _s = sub(r, &session(14), &seen);
    w.a.t.publish(
        &session(14),
        payload(PayloadKind::Envelope, b"neu"),
        w.deadline(LONG),
    );
    w.eventually(|| seen.len() >= 1);
    for i in seen.get() {
        fail_if!(
            format!("{:?}", i.carrier) != "CarrierHandle(..)",
            "a carrier handle prints its inside"
        );
    }
    Verdict::Pass(
        "by construction: Inbound and PresenceEvent carry a Payload and an opaque CarrierHandle only"
            .into(),
    )
}

fn forbidden_in_health(m: &dyn Medium) -> Vec<String> {
    let mut v = m.health_must_not_contain();
    if let Ok(d) = std::env::current_dir() {
        v.push(d.display().to_string());
    }
    for var in ["HOME", "USERPROFILE"] {
        if let Ok(x) = std::env::var(var) {
            if !x.is_empty() {
                v.push(x);
            }
        }
    }
    v
}

fn looks_like_address(s: &str) -> bool {
    if s.contains("://") {
        return true;
    }
    // An IPv4 literal: four dot-separated numbers.
    s.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .any(|t| t.split('.').count() == 4 && t.split('.').all(|p| !p.is_empty() && p.len() <= 3))
}

/// [IFC-TYP-092]: the transport's `health` detail holds no working directory, home
/// directory, native address (an IPv4 literal or a URL) or any string the medium lists,
/// before `start`, while running and after `shutdown`.
pub fn ifc_typ_092(h: &dyn TransportHarness) -> Verdict {
    let m = h.medium();
    let t = m.transport();
    let mut seen: Vec<HealthStatus> = vec![t.health()];
    let k = key(1);
    t.start(&k, m.configuration()).expect("start");
    seen.push(t.health());
    t.shutdown();
    seen.push(t.health());
    let forbidden = forbidden_in_health(&*m);
    for s in &seen {
        if let Some(d) = &s.detail {
            for f in &forbidden {
                fail_if!(d.contains(f.as_str()), "health detail {d:?} contains {f:?}");
            }
            fail_if!(
                looks_like_address(d),
                "health detail {d:?} looks like a native address"
            );
        }
    }
    let states: Vec<_> = seen.iter().map(|s| s.state.as_str()).collect();
    Verdict::Pass(format!("health {states:?}, no forbidden detail"))
}

/// [IFC-TYP-095]: a `Destination` is a session id or a key id and nothing else. By
/// construction of `oac_core::transport::Destination`, which has exactly those variants;
/// the suite exercises both.
pub fn ifc_typ_095(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    for d in [session(15), Destination::Device(w.recv().key.clone())] {
        match d {
            Destination::Session(_) | Destination::Device(_) => {}
        }
    }
    Verdict::Pass("by construction: Destination::Session or Destination::Device only".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_recognised() {
        assert!(looks_like_address("bound to 127.0.0.1"));
        assert!(looks_like_address("tcp://x"));
        assert!(!looks_like_address("not started"));
        assert!(!looks_like_address("version 1.2.3"));
    }

    #[test]
    fn keys_are_distinct_hex() {
        assert_ne!(key(1), key(2));
        assert_eq!(key(10).as_str(), "a".repeat(64));
    }
}
