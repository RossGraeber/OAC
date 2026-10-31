// SPDX-License-Identifier: Apache-2.0

//! The transport contract suite (#59, F10): one suite, written against
//! `dyn oac_core::transport::Transport`, that every transport runs unchanged
//! (`spec/interfaces.md` §6, Table 6.4; revision 0.3 sealing extension).
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
//! the suite run the checks that need a copy to be in flight for a while ([IFC-TRN-035]);
//! without it those parts are reported not applicable, never passed. A medium that can also
//! hold a copy at the sending end ([`FaultControl::next_sender_hold`]) lets the suite observe
//! a late hand-over of a held copy ([IFC-TRN-034], revision 0.7).
//!
//! A sealing declaration selects suite-side sealing/opening of the same logical checks.
//! Every start is cross-checked against the independently reviewed binding value; run-wide
//! raw observations cover all worlds and callbacks before opening/filtering.
//! Only sealed frames cross the tested transport; presence uses `publish` and a device
//! subscription. Raw sealing checks bypass that helper. [`Medium::sealing_observations`]
//! is mandatory for that path, and missing observations fail, rather than skip, checks.
//!
//! # What it checks
//!
//! Every requirement that Appendix C of `spec/interfaces.md` assigns to the `transport`
//! ([IFC-TRN-003]): [IFC-TRN-001], [IFC-TRN-020], [IFC-TRN-021], [IFC-TRN-023],
//! [IFC-TRN-026], [IFC-TRN-030], [IFC-TRN-031], [IFC-TRN-033] to [IFC-TRN-036],
//! [IFC-TRN-040], [IFC-TRN-043], [IFC-TRN-044], [IFC-TRN-050], [IFC-TRN-060],
//! [IFC-TRN-071], [IFC-TRN-080], [IFC-TRN-118], [IFC-TRN-119] and [IFC-NEU-003], plus [IFC-TYP-092] for the transport's
//! `health` and [IFC-TYP-095], plus [IFC-TRN-104], [IFC-TRN-105], [IFC-TRN-107],
//! [IFC-TRN-108], [IFC-TRN-109] and [IFC-TRN-113] when sealing is declared, and
//! [IFC-TRN-110] against the binding. [`run`]
//! returns one [`Row`] per check; [IFC-TRN-003] passes only when no other row failed. The requirements Appendix C gives the `core` are not
//! this suite's (they need the core's send and receive paths, F6 and F11), and
//! [IFC-TRN-090] and [IFC-TRN-117] (a binding document states its clock-skew limit) are the
//! binding document's, reviewed by reading it, so this suite has no row for them.
//! [IFC-TRN-038] is a SHOULD, with no fixture (Appendix A).
//!
//! [IFC-TRN-034] is checked at the sending end only (revision 0.7): a payload passed at or
//! after its `Deadline` is refused and no copy of it is handed over, and a copy held at the
//! sender is not handed over once the `Deadline` has come. The suite does not ask a transport
//! to suppress the late arrival of a copy that it handed to the carrier in time. The
//! receiving core refuses such a copy, on its own clock (`spec/interfaces.md` §6.4,
//! "Late arrival"; a receipt has no receiving-side rule, §7.4).
//!
//! An existing check that the implementation's declaration or medium makes moot (a capability
//! declared absent, a single-implementation medium for a two-sided check, no fault control)
//! is [`Verdict::NotApplicable`] with the reason, never a pass. The new sealing rows
//! explicitly report absence when sealing is false; they claim no sealing evidence.

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
    /// Sealing value from the independently reviewed binding (absent means false).
    /// Must not be computed from the implementation's `start` declaration.
    fn binding_sealing(&self) -> bool;

    /// A short name for reports.
    fn name(&self) -> String;

    /// A fresh, isolated medium. Nothing started on one medium reaches another.
    fn medium(&self) -> Box<dyn Medium>;
}

/// One medium: the transports made by [`Medium::transport`] and started with
/// [`Medium::configuration`] reach each other.
pub trait Medium: Send + Sync {
    /// Suite-internal audit hook for destinations before logical sealing hides them.
    /// Implementations need not override this; the run-wide audit owns the record.
    #[doc(hidden)]
    fn audit_destination(&self, _destination: &Destination) {}

    /// Raw carriage captured at the implementation boundary, including headers and link,
    /// peer, carrier and liveness identifiers. Required for a sealing declaration.
    /// This is observation, never permission to omit a check. See the suite README.
    fn sealing_observations(&self) -> Option<Vec<SealingObservation>> {
        None
    }

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

    /// Wait until a subscription or presence watch just made is in effect, so that a
    /// payload published next can reach it. The suite calls this after every `subscribe`
    /// and `watch_presence`, before it publishes (PR #323 review N2). A transport whose
    /// interest declarations propagate asynchronously, as a network transport's do, needs
    /// it; the default waits as [`Medium::settle`] does.
    fn subscribed(&self) {
        self.settle();
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

/// One raw sealed-frame carriage observation. Opaque link identifiers are allowed;
/// payload-derived headers and destination/deadline side channels are not.
#[derive(Clone, Debug)]
pub struct SealingObservation {
    /// Frame octets, exactly as carried.
    pub frame: Vec<u8>,
    /// All accompanying application values outside the frame (including kind labels).
    pub accompanying: Vec<Vec<u8>>,
    /// Raw identifiers of links, peers, carriers and liveness signals, even if opaque in Rust.
    pub identifiers: Vec<Vec<u8>>,
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

    /// Delay the next hand-off at the sender, where Deadline still applies.
    /// False means this medium can delay only network/receiving delivery.
    fn next_sender_hold(&self, _by: Duration) -> bool {
        false
    }
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
    sealing: bool,
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
        if self.sealing {
            for id in sealed::REQUIRED {
                assert!(
                    self.verdicts(id)
                        .iter()
                        .any(|v| matches!(v, Verdict::Pass(_))),
                    "missing mandatory sealing check {id}: {self}"
                );
            }
        }
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
    (
        "IFC-TRN-113",
        "rejects-every-plain-kind",
        sealed::reject_plain,
    ),
    ("IFC-TRN-104", "frame-size-cap", sealed::frame_cap),
    (
        "IFC-TRN-107",
        "frame-has-no-accompanying-values",
        sealed::carriage,
    ),
    (
        "IFC-TRN-108",
        "identifiers-do-not-name-devices-or-sessions",
        sealed::identifiers,
    ),
    ("IFC-TRN-109", "sealing-payload-floor", sealed::floor),
    (
        "IFC-TRN-034",
        "sealing-sender-expired-input",
        sealed::sender_expiry,
    ),
    (
        "IFC-TRN-105",
        "sealed-to-local-device-subscription",
        sealed::inbound,
    ),
    ("IFC-TRN-001", "carries-each-kind", ifc_trn_001),
    ("IFC-TRN-020", "declares-every-capability", ifc_trn_020),
    (
        "IFC-TRN-021",
        "declared-ordering-is-provided",
        ifc_trn_021_ordering,
    ),
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
    ("IFC-TRN-118", "declared-size-is-not-refused", ifc_trn_118),
    (
        "IFC-TRN-119",
        "taken-is-delivered-when-reliability-declared",
        ifc_trn_119,
    ),
    ("IFC-TRN-031", "deliverable-is-taken", ifc_trn_031),
    ("IFC-TRN-033", "holds-only-in-flight", ifc_trn_033),
    (
        "IFC-TRN-034",
        "no-accept-or-hand-over-at-or-after-deadline",
        ifc_trn_034,
    ),
    (
        "IFC-TRN-034",
        "no-hand-over-of-a-held-copy-after-deadline",
        ifc_trn_034_held,
    ),
    ("IFC-TRN-035", "nothing-crosses-a-restart", ifc_trn_035),
    ("IFC-TRN-036", "nothing-kept-for-unreachable", ifc_trn_036),
    ("IFC-TRN-040", "handed-over-on-own-initiative", ifc_trn_040),
    ("IFC-TRN-043", "subscriptions-not-revealed", ifc_trn_043),
    (
        "IFC-TRN-044",
        "result-independent-of-subscription",
        ifc_trn_044,
    ),
    ("IFC-TRN-050", "presence-whole-to-named-device", ifc_trn_050),
    (
        "IFC-TRN-060",
        "presence-handed-on-own-initiative",
        ifc_trn_060,
    ),
    ("IFC-TRN-071", "no-handler-after-shutdown", ifc_trn_071),
    ("IFC-TRN-080", "destination-restricted", ifc_trn_080),
    ("IFC-NEU-003", "no-native-address-to-core", ifc_neu_003),
    (
        "IFC-TYP-092",
        "health-holds-no-secret-or-address",
        ifc_typ_092,
    ),
    ("IFC-TYP-095", "destination-is-session-or-key", ifc_typ_095),
];

/// Run every check against `harness`. A check that panics is a [`Verdict::Fail`] with the
/// panic message; the other checks still run.
pub fn run(harness: &dyn TransportHarness) -> Report {
    let audit = audit::Harness::new(harness);
    let harness: &dyn TransportHarness = &audit;
    let sealing = harness.binding_sealing();
    let mut rows = Vec::new();
    for (id, check, f) in CHECKS {
        let verdict = match catch_unwind(AssertUnwindSafe(|| f(harness))) {
            Ok(v) => v,
            Err(p) => Verdict::Fail(format!("panicked: {}", panic_text(&p))),
        };
        rows.push(Row { id, check, verdict });
    }
    let sealing = sealing || audit.sealing_observed();
    rows.push(Row {
        id: "IFC-TRN-110",
        check: "declaration-agrees-with-binding",
        verdict: audit.declaration(),
    });
    if sealing {
        rows.push(Row {
            id: "IFC-TRN-071",
            check: "raw-no-handler-after-shutdown",
            verdict: audit.shutdown(),
        });
        let obs = audit.observations();
        for (id, check, verdict) in [
            (
                "IFC-TRN-107",
                "all-worlds-no-accompanying-values",
                match &obs {
                    Ok(v) => sealed::check_carriage(v),
                    Err(v) => v.clone(),
                },
            ),
            (
                "IFC-TRN-108",
                "all-worlds-no-identifier-leaks",
                match &obs {
                    Ok(v) => sealed::check_identifiers(v, &audit.identifier_needles()),
                    Err(v) => v.clone(),
                },
            ),
        ] {
            rows.push(Row { id, check, verdict });
        }
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
        sealing,
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

/// The most the size check allocates for one payload: a transport declaring a larger
/// `max_payload_octets` gets [`Verdict::NotApplicable`] from [IFC-TRN-118], never a pass.
const DECLARED_SIZE_LIMIT: u64 = 64 << 20;

/// The sizes [IFC-TRN-118] passes for a declared maximum of `max` octets: the maximum, one
/// less, a quarter, a half and three quarters of it, and 65536 when that lies below it. The
/// rule says "at most" the maximum, so a refusal anywhere in the range breaks it; these are a
/// sample, and a refusal confined to a narrower band than they leave between them is not seen.
fn size_probes(max: usize) -> Vec<usize> {
    let mut v = vec![max / 4, max / 2, max / 4 * 3, max.saturating_sub(1), max];
    if max > 65536 {
        v.push(65536);
    }
    v.retain(|n| *n >= 1);
    v.sort_unstable();
    v.dedup();
    v
}

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

fn start(
    m: Arc<dyn Medium>,
    n: u8,
    frames: sealed::FrameBook,
    epoch: Instant,
) -> Result<Ep, TransportError> {
    let raw = m.transport();
    let key = key(n);
    let caps = raw.start(&key, m.configuration())?;
    let t = sealed::logical(raw, &key, caps, frames, m, epoch);
    Ok(Ep { t, key, caps })
}

/// A medium with endpoint `a` and, on a cross-implementation medium, a second endpoint `b`
/// for another device key.
struct World {
    m: Arc<dyn Medium>,
    a: Ep,
    b: Option<Ep>,
    frames: sealed::FrameBook,
    epoch: Instant,
}

impl World {
    fn new(h: &dyn TransportHarness) -> World {
        let m: Arc<dyn Medium> = h.medium().into();
        let frames = sealed::FrameBook::default();
        let epoch = m.now();
        let a = start(m.clone(), 1, frames.clone(), epoch).expect("start of the first transport");
        let b = if a.caps.reach == Reach::CrossImplementation {
            Some(
                start(m.clone(), 2, frames.clone(), epoch)
                    .expect("start of a second device on a cross-implementation medium"),
            )
        } else {
            None
        };
        World {
            m,
            a,
            b,
            frames,
            epoch,
        }
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
                PresenceEvent::CarrierLoss { .. } | PresenceEvent::CarrierAppeared { .. } => None,
            })
            .collect()
    }
}

impl World {
    /// Subscribe `ep` to `d`, then wait for the subscription to be in effect.
    fn sub(&self, ep: &Ep, d: &Destination, seen: &Seen<Inbound>) -> Subscription {
        let s =
            ep.t.subscribe(d, seen.handler())
                .unwrap_or_else(|e| panic!("subscribe {d:?}: {e}"));
        self.m.subscribed();
        s
    }

    /// Watch presence on `ep`, then wait for the watch to be in effect.
    fn watch(&self, ep: &Ep, handler: PresenceHandler) {
        ep.t.watch_presence(handler).expect("watch_presence");
        self.m.subscribed();
    }
}

macro_rules! fail_if {
    ($cond:expr, $($msg:tt)+) => {
        if $cond {
            return Verdict::Fail(format!($($msg)+));
        }
    };
}

// ---- §6.1, §6.5: carrying the three kinds --------------------------------------------

mod audit;
mod sealed;

/// [IFC-TRN-001]: an envelope to a session, a receipt to a device and a presence record to
/// a device are each carried.
pub fn ifc_trn_001(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let r = w.recv();
    let (env, rct, prs) = (Seen::default(), Seen::default(), Seen::default());
    let _s1 = w.sub(r, &session(1), &env);
    let _s2 = w.sub(r, &Destination::Device(r.key.clone()), &rct);
    w.watch(r, prs.watcher());
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
    Verdict::Pass(format!(
        "envelope, receipt and presence carried ({})",
        w.sides()
    ))
}

// ---- §6.3: the declaration ----------------------------------------------------------

/// [IFC-TRN-020]: `start` returns a declaration with each of the six optional capabilities
/// present or absent. The Rust type has a boolean for each, so a declaration that returns
/// at all declares all six.
pub fn ifc_trn_020(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let c = w.a.caps;
    Verdict::Pass(format!(
        "reliability {}, persistence {}, offline_queueing {}, ordering {}, multicast_discovery {}, routing_federation {}, sealing {}",
        c.reliability,
        c.persistence,
        c.offline_queueing,
        c.ordering,
        c.multicast_discovery,
        c.routing_federation,
        c.sealing
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
    let _s = w.sub(r, &session(2), &seen);
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
///
/// The rule is about the octets of a payload the transport "delivers": it does not require
/// that every payload passed is delivered. That is `reliability` (Table 6.3), and
/// [IFC-TRN-021] has the suite check only that it is declared truthfully. So the two payloads
/// below are held to the delivery rule only when the transport declares `reliability`
/// present. A transport that declares it absent may lose either one on a loaded host (a
/// native best-effort send queue can drop a large frame), and then any payload it does
/// deliver must still be one passed, octet for octet, and something must arrive. The
/// duplicate copies below are scripted by the harness's fault control, not by the medium,
/// so they stay required either way.
pub fn ifc_trn_030(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let r = w.recv();
    let seen = Seen::default();
    let _s = w.sub(r, &session(3), &seen);
    let every: Vec<u8> = (0..=255).collect();
    let limit = if w.a.caps.sealing {
        w.a.caps
            .max_payload_octets
            .saturating_sub(sealed::LOGICAL_OVERHEAD)
    } else {
        w.a.caps.max_payload_octets
    };
    let max = usize::try_from(limit.min(1 << 20)).unwrap_or(1 << 20);
    let big: Vec<u8> = (0..max).map(|i| (i % 251) as u8).collect();
    let dl = w.deadline(LONG);
    // What the medium may lose when `reliability` is absent.
    let carried = vec![every.clone(), big.clone()];
    // What the harness's fault control makes the medium copy: never lost.
    let mut copies: Vec<Vec<u8>> = Vec::new();
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
        copies.push(dup.clone());
        copies.push(dup.clone());
    }
    let reliable = w.a.caps.reliability;
    let total = carried.len() + copies.len();
    w.eventually(|| seen.len() >= total);
    let mut got = seen.octets();
    let lengths = |v: &[Vec<u8>]| v.iter().map(Vec::len).collect::<Vec<_>>();
    let mut lost = Vec::new();
    if reliable {
        let mut sent: Vec<Vec<u8>> = carried.iter().chain(&copies).cloned().collect();
        got.sort();
        sent.sort();
        fail_if!(
            got != sent,
            "received {} payloads of lengths {:?}, sent {} of lengths {:?}",
            got.len(),
            lengths(&got),
            sent.len(),
            lengths(&sent)
        );
    } else {
        // Every delivered payload is one passed (a multiset match), so an altered, invented
        // or surplus payload fails; only a payload that never arrived is forgiven.
        let mut pool: Vec<Vec<u8>> = carried.iter().chain(&copies).cloned().collect();
        let mut unmatched = Vec::new();
        for g in &got {
            match pool.iter().position(|s| s == g) {
                Some(i) => {
                    pool.swap_remove(i);
                }
                None => unmatched.push(g.clone()),
            }
        }
        fail_if!(
            !unmatched.is_empty(),
            "delivered {} payloads of lengths {:?} that match no payload passed (sent lengths {:?})",
            unmatched.len(),
            lengths(&unmatched),
            lengths(&carried)
        );
        fail_if!(got.is_empty(), "no payload was delivered");
        fail_if!(
            got.iter().filter(|g| **g == dup).count() != copies.len(),
            "{} copies of the scripted duplicate delivered, expected {}",
            got.iter().filter(|g| **g == dup).count(),
            copies.len()
        );
        lost = lengths(&pool);
    }
    Verdict::Pass(format!(
        "every octet value, {max} octets{}{}",
        if w.faults().is_some() {
            ", and both copies of a duplicate"
        } else {
            ""
        },
        if lost.is_empty() {
            String::new()
        } else {
            format!(
                "; reliability declared absent, so payloads of lengths {lost:?} not delivered are not a failure"
            )
        }
    ))
}

/// [IFC-TRN-118]: a payload of at most the declared `max_payload_octets` octets is not
/// refused for its size. The check passes payloads of the sizes of `size_probes` (the
/// maximum itself, one less, and interior sizes), of each kind
/// the transport accepts (an envelope to a session, a receipt to a device and, through
/// `send_presence`, a presence record to a device), and requires `taken`. It does not ask for
/// delivery: without `reliability` that is not promised (Table 6.3, [IFC-TRN-030]), and with
/// it, [`ifc_trn_119`] asks. A small payload of the same kind is passed first, as a control:
/// if even that is refused, the refusal at the maximum cannot be put down to size and the
/// row is not applicable (the rows of [IFC-TRN-001] and [IFC-TRN-031] fail instead).
///
/// A sealing transport carries only sealed frames ([IFC-TRN-113]) and a sealed payload is
/// measured as its frame (§6.10), so the check passes frames of the declared maximum, one for
/// each inner kind (`sealed::declared_size`).
pub fn ifc_trn_118(h: &dyn TransportHarness) -> Verdict {
    if let Some(v) = sealed::declared_size(h) {
        return v;
    }
    let w = World::new(h);
    let max = w.a.caps.max_payload_octets;
    if max > DECLARED_SIZE_LIMIT {
        return Verdict::NotApplicable(format!(
            "declared maximum {max} exceeds the {DECLARED_SIZE_LIMIT} octets this check allocates"
        ));
    }
    let n = usize::try_from(max).expect("bounded by DECLARED_SIZE_LIMIT");
    let r = w.recv();
    let dev = Destination::Device(r.key.clone());
    let (env, rct, prs) = (Seen::default(), Seen::default(), Seen::default());
    let _s1 = w.sub(r, &session(6), &env);
    let _s2 = w.sub(r, &dev, &rct);
    w.watch(r, prs.watcher());
    let dl = w.deadline(LONG);
    let send = |kind: PayloadKind, octets: &[u8]| match kind {
        PayloadKind::Envelope => w.a.t.publish(&session(6), payload(kind, octets), dl),
        PayloadKind::Receipt => w.a.t.publish(&dev, payload(kind, octets), dl),
        _ => w.a.t.send_presence(&dev, payload(kind, octets), dl),
    };
    let kinds = [
        PayloadKind::Envelope,
        PayloadKind::Receipt,
        PayloadKind::Presence,
    ];
    for kind in kinds {
        let c = send(kind, b"size-control-118");
        if c != PublishResult::Taken {
            return Verdict::NotApplicable(format!(
                "a 16-octet {kind:?} was {c:?}, so a refusal at the declared maximum cannot be put down to size (see IFC-TRN-001, IFC-TRN-031)"
            ));
        }
    }
    // Each size is passed after the transport has settled from the last, so that a refusal
    // from a full queue is not counted against size.
    let sizes = size_probes(n);
    for len in &sizes {
        let octets = vec![0x5a; *len];
        for kind in kinds {
            w.m.settle();
            let got = send(kind, &octets);
            fail_if!(
                got != PublishResult::Taken,
                "a {kind:?} of {len} octets, within the declared maximum of {max}, was {got:?} while a 16-octet one was taken"
            );
        }
    }
    w.m.settle();
    Verdict::Pass(format!(
        "an envelope, a receipt and a presence record of each of {sizes:?} octets, up to the declared maximum {max}, each taken ({})",
        w.sides()
    ))
}

/// [IFC-TRN-119]: a transport that declares `reliability` delivers each payload for which it
/// returns `taken`, unless the payload's `Deadline` passes first, the destination is not
/// reachable, or the transport is shut down or restarted first. Not applicable to a transport
/// that declares `reliability` absent: [IFC-TRN-119] binds only one that declares it, and the
/// rest owe [IFC-TRN-118] and [IFC-TRN-030] alone. No transport of this repository declares
/// `reliability`, so the row is exercised only by a planted transport that claims it.
///
/// For each kind (an envelope to a session, a receipt to a device, a presence record to a
/// device) the check passes three distinct payloads: a short one, one of 4096 octets and one
/// of the declared maximum (up to 1 MiB, less the suite's routing control on a sealing
/// transport), all with a deadline a minute away, to a subscribed destination, on a running
/// transport. None of the three stop conditions applies then, so every payload for which
/// `taken` came back must arrive, octet for octet. A payload that was `not-taken` is not
/// held to delivery here (that is [IFC-TRN-118]'s and [IFC-TRN-031]'s business). The stop
/// conditions themselves are not exercised: they relax the rule, and the suite cannot script
/// an unreachable destination, a restart across a retransmission, or a link that loses
/// copies under a retransmitting transport (cf. [IFC-TRN-021]).
pub fn ifc_trn_119(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    if !w.a.caps.reliability {
        return Verdict::NotApplicable(
            "reliability declared absent: IFC-TRN-119 binds only a transport that declares it; IFC-TRN-118 and IFC-TRN-030 apply".into(),
        );
    }
    let r = w.recv();
    let dev = Destination::Device(r.key.clone());
    let (env, rct, prs) = (Seen::default(), Seen::default(), Seen::default());
    let _s1 = w.sub(r, &session(8), &env);
    let _s2 = w.sub(r, &dev, &rct);
    w.watch(r, prs.watcher());
    let limit = if w.a.caps.sealing {
        w.a.caps
            .max_payload_octets
            .saturating_sub(sealed::LOGICAL_OVERHEAD)
    } else {
        w.a.caps.max_payload_octets
    };
    let big = usize::try_from(limit.min(1 << 20)).unwrap_or(1 << 20);
    let dl = w.deadline(LONG);
    let kinds = [
        PayloadKind::Envelope,
        PayloadKind::Receipt,
        PayloadKind::Presence,
    ];
    // (kind, octets) of every payload that was taken.
    let mut taken: Vec<(PayloadKind, Vec<u8>)> = Vec::new();
    let mut refused = 0usize;
    for (k, kind) in kinds.into_iter().enumerate() {
        for (i, len) in [16usize, 4096, big].into_iter().enumerate() {
            let tag = (k * 3 + i) as u8;
            let octets: Vec<u8> = (0..len).map(|j| (j % 251) as u8 ^ tag).collect();
            let result = match kind {
                PayloadKind::Envelope => w.a.t.publish(&session(8), payload(kind, &octets), dl),
                PayloadKind::Receipt => w.a.t.publish(&dev, payload(kind, &octets), dl),
                _ => w.a.t.send_presence(&dev, payload(kind, &octets), dl),
            };
            if result == PublishResult::Taken {
                taken.push((kind, octets));
            } else {
                refused += 1;
            }
        }
    }
    if taken.is_empty() {
        return Verdict::NotApplicable(
            "reliability declared, but no payload was taken, so none is held to delivery (see IFC-TRN-031)".into(),
        );
    }
    let arrived = |kind: PayloadKind| -> Vec<Vec<u8>> {
        match kind {
            PayloadKind::Envelope => env.octets(),
            PayloadKind::Receipt => rct.octets(),
            _ => prs.records().iter().map(|p| p.octets().to_vec()).collect(),
        }
    };
    w.eventually(|| kinds.iter().map(|k| arrived(*k).len()).sum::<usize>() >= taken.len());
    let mut missing = Vec::new();
    for kind in kinds {
        let mut pool = arrived(kind);
        for (_, octets) in taken.iter().filter(|(k, _)| *k == kind) {
            match pool.iter().position(|g| g == octets) {
                Some(i) => {
                    pool.swap_remove(i);
                }
                None => missing.push((kind, octets.len())),
            }
        }
    }
    fail_if!(
        !missing.is_empty(),
        "reliability declared, but {} of {} taken payloads never arrived, although the deadline was a minute away, the destination subscribed and the transport running: (kind, octets) {missing:?}",
        missing.len(),
        taken.len()
    );
    Verdict::Pass(format!(
        "reliability declared: all {} taken payloads (envelope, receipt, presence; up to {big} octets) arrived unchanged{} ({}); the stop conditions are not exercised",
        taken.len(),
        if refused > 0 {
            format!(", {refused} not-taken ones not held to delivery")
        } else {
            String::new()
        },
        w.sides()
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
    let _s1 = w.sub(r, &session(4), &env);
    let _s2 = w.sub(r, &dev, &rct);
    w.watch(r, prs.watcher());
    let dl = w.deadline(LONG);
    let mut results = Vec::new();
    for i in 0..5u8 {
        results.push(
            w.a.t
                .publish(&session(4), payload(PayloadKind::Envelope, &[i]), dl),
        );
        results.push(w.a.t.publish(&dev, payload(PayloadKind::Receipt, &[i]), dl));
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
    Verdict::Pass(format!(
        "15 deliverable payloads taken and delivered ({})",
        w.sides()
    ))
}

/// [IFC-TRN-033]: a copy is held only while in flight. A delivered payload is not handed
/// to a subscription made afterwards.
pub fn ifc_trn_033(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let r = w.recv();
    let first = Seen::default();
    let s1 = w.sub(r, &session(5), &first);
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
    let _s2 = w.sub(r, &session(5), &later);
    let elsewhere = Seen::default();
    let _s3 = w.sub(&w.a, &session(5), &elsewhere);
    w.m.advance(Duration::from_secs(1));
    w.quiet();
    fail_if!(
        later.len() + elsewhere.len() > 0,
        "a delivered payload reached {} later subscription(s)",
        later.len() + elsewhere.len()
    );
    Verdict::Pass("a delivered payload reached no later subscription".into())
}

/// [IFC-TRN-034] (revision 0.7), the sending end: a transport does not accept a payload at
/// or after its `Deadline` and hands no copy to the carrier then. Observed neutrally, for
/// the first hand-over of each kind: a payload passed at its deadline and one passed after
/// it must each get `not-taken` ([IFC-TRN-034] has the transport refuse it, and
/// [IFC-TRN-031] permits that), and no copy of either may reach the receiving end. An
/// envelope, a receipt and a presence record are each tried. Controls with a distant
/// deadline are carried in the same world, so a receiving end that never works cannot make
/// the check pass.
///
/// This check does not ask the transport to suppress a late arrival. A copy handed to the
/// carrier before its deadline may arrive after it, and the receiving core refuses it
/// (§6.4 "Late arrival"; `spec/security.md` [SEC-SEL-036]; a receipt has no receiving-side
/// rule, §7.4).
///
/// Under a sealing declaration the suite's simulated receiving core drops expired payloads
/// itself, which masks the "none arrived" half of this check: only the `not-taken` half can
/// fail there, and the arrival half is observed raw by `sealing-sender-expired-input`. The retry, the retransmission
/// and the replay after a reconnection are observed by [`ifc_trn_034_held`].
pub fn ifc_trn_034(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    let r = w.recv();
    let device = Destination::Device(r.key.clone());
    let envelopes = Seen::default();
    let _s = w.sub(r, &session(6), &envelopes);
    let on_device = Seen::default();
    let _d = w.sub(r, &device, &on_device);
    let records = Seen::default();
    w.watch(r, records.watcher());

    // Control: in time, all three kinds are carried.
    let control = [
        (
            "envelope",
            w.a.t.publish(
                &session(6),
                payload(PayloadKind::Envelope, b"control-envelope"),
                w.deadline(LONG),
            ),
        ),
        (
            "receipt",
            w.a.t.publish(
                &device,
                payload(PayloadKind::Receipt, b"control-receipt"),
                w.deadline(LONG),
            ),
        ),
        (
            "presence record",
            w.a.t.send_presence(
                &device,
                payload(PayloadKind::Presence, b"control-presence"),
                w.deadline(LONG),
            ),
        ),
    ];
    for (what, res) in &control {
        fail_if!(
            *res != PublishResult::Taken,
            "control failed: an in-time {what} got {res:?}"
        );
    }
    fail_if!(
        !w.eventually(|| {
            envelopes.len() >= 1 && on_device.len() >= 1 && !records.records().is_empty()
        }),
        "control failed: an in-time payload never arrived"
    );

    // At the deadline, then after it. The clock moves between the two.
    let try_at = |label: &str, at: Deadline| -> Option<String> {
        let results = [
            (
                "envelope",
                w.a.t.publish(
                    &session(6),
                    payload(PayloadKind::Envelope, b"refused-envelope"),
                    at,
                ),
            ),
            (
                "receipt",
                w.a.t.publish(
                    &device,
                    payload(PayloadKind::Receipt, b"refused-receipt"),
                    at,
                ),
            ),
            (
                "presence record",
                w.a.t.send_presence(
                    &device,
                    payload(PayloadKind::Presence, b"refused-presence"),
                    at,
                ),
            ),
        ];
        results
            .into_iter()
            .find(|(_, res)| *res != PublishResult::NotTaken)
            .map(|(what, res)| {
                format!("the {what} passed {label} its deadline was accepted ({res:?})")
            })
    };
    if let Some(why) = try_at("at", Deadline::at(w.m.now())) {
        return Verdict::Fail(why);
    }
    let soon = w.deadline(Duration::from_millis(50));
    w.m.advance(Duration::from_millis(150));
    if let Some(why) = try_at("after", soon) {
        return Verdict::Fail(why);
    }
    w.m.advance(Duration::from_secs(1));
    w.quiet();
    let arrived = envelopes
        .octets()
        .into_iter()
        .chain(on_device.octets())
        .chain(records.records().iter().map(|p| p.octets().to_vec()))
        .filter(|o| o.starts_with(b"refused-"))
        .count();
    fail_if!(
        arrived > 0,
        "{arrived} payload(s) passed at or after the deadline reached the receiving end: a copy was handed over then"
    );
    // A copy handed to the carrier in time and delayed by the network past its deadline is a
    // timing fact (revision 0.7): it may arrive or be dropped, and neither fails the check.
    let late = if let Some(f) = w.faults() {
        f.next(Fate::Delay(Duration::from_secs(2)));
        w.a.t.publish(
            &session(6),
            payload(PayloadKind::Envelope, b"network-delayed"),
            w.deadline(Duration::from_secs(1)),
        );
        w.m.advance(Duration::from_secs(3));
        w.quiet();
        "; a copy delayed by the network past its deadline was exercised and, arrived or not, is not a breach"
    } else {
        "; no fault control: a network-delayed copy was not exercised"
    };
    Verdict::Pass(format!(
        "an envelope, a receipt and a presence record passed at and after their deadline were each refused with not-taken and none arrived; in-time controls were carried{late}"
    ))
}

/// [IFC-TRN-034] (revision 0.7), a copy the transport holds and would hand over later: the
/// retransmission, the retry after a link failure and the replay after a reconnection each
/// hand a held copy to the carrier after a wait. [`FaultControl::next_sender_hold`] makes
/// the sending end hold the next copy for a time on the medium's clock. A copy taken in time
/// and held beyond its deadline must not reach the receiving end, since the only way it can
/// is by a hand-over at or after the deadline. A copy held for less than its deadline is the
/// control: it does arrive, so the hold does not make the check vacuous.
///
/// Not applicable, never a pass, when the medium cannot hold a copy at the sender. The
/// replay after a reconnection in particular needs a medium that can cut and restore a link
/// (`TODO(fixture)`, F10 with G1 and G3). Also not applicable when a sealing transport is
/// declared: the suite's sealing helper then opens frames in the simulated receiving core,
/// which drops a late payload itself ([SEC-SEL-036]) and so hides what the sender emitted.
/// The raw `sealing-sender-expired-input` row observes the sender in that case.
pub fn ifc_trn_034_held(h: &dyn TransportHarness) -> Verdict {
    let w = World::new(h);
    if w.a.caps.sealing {
        return Verdict::NotApplicable(
            "sealing declared: the simulated receiving core drops late payloads and hides the sender; the sealing-sender-expired-input row observes it raw".into(),
        );
    }
    let Some(f) = w.faults() else {
        return Verdict::NotApplicable(
            "no fault control: retransmission, retry and reconnection replay not exercised (TODO(fixture): F10 with G1, G3)".into(),
        );
    };
    let r = w.recv();
    let seen = Seen::default();
    let _s = w.sub(r, &session(6), &seen);
    if !f.next_sender_hold(Duration::from_millis(500)) {
        return Verdict::NotApplicable(
            "the medium cannot hold a copy at the sending end (TODO(fixture): F10 with G1, G3)"
                .into(),
        );
    }
    let res = w.a.t.publish(
        &session(6),
        payload(PayloadKind::Envelope, b"held-in-time"),
        w.deadline(Duration::from_secs(5)),
    );
    fail_if!(
        res != PublishResult::Taken,
        "control failed: an in-time payload got {res:?}"
    );
    w.m.advance(Duration::from_millis(600));
    fail_if!(
        !w.eventually(|| seen.len() >= 1),
        "control failed: a copy held for less than its deadline never arrived"
    );
    f.next_sender_hold(Duration::from_secs(2));
    let res = w.a.t.publish(
        &session(6),
        payload(PayloadKind::Envelope, b"held-too-long"),
        w.deadline(Duration::from_secs(1)),
    );
    fail_if!(
        res != PublishResult::Taken,
        "control failed: an in-time payload got {res:?}"
    );
    w.m.advance(Duration::from_millis(1500));
    w.quiet();
    w.m.advance(Duration::from_secs(2));
    w.quiet();
    fail_if!(
        seen.octets().iter().any(|o| o == b"held-too-long"),
        "a copy held beyond its deadline was handed over after it"
    );
    Verdict::Pass(
        "a copy held for less than its deadline arrived; one held beyond it was not handed over (the late arrival of a copy handed over in time is not asserted)"
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
        let _s = w.sub(r, &session(7), &before);
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
        let again = sealed::logical(
            again,
            &r.key,
            r.caps,
            w.frames.clone(),
            w.m.clone(),
            w.epoch,
        );
        let after = Seen::default();
        let _s2 = again
            .subscribe(&session(7), after.handler())
            .expect("subscribe after restart");
        w.m.subscribed();
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
            let _s = w.sub(b, &session(8), &seen);
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
            let _again = sealed::logical(
                again,
                &w.a.key,
                w.a.caps,
                w.frames.clone(),
                w.m.clone(),
                w.epoch,
            );
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
    let _s = w.sub(r, &session(9), &later);
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
    let _s = w.sub(r, &session(10), &seen);
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
    let unsub =
        w.a.t
            .publish(&session(11), payload(PayloadKind::Envelope, b"x"), dl);
    let seen = Seen::default();
    let _s = w.sub(b, &session(11), &seen);
    let subd =
        w.a.t
            .publish(&session(11), payload(PayloadKind::Envelope, b"x"), dl);
    let after_health = w.a.t.health();
    fail_if!(
        unsub != subd,
        "publish returned {unsub:?} before the other side subscribed and {subd:?} after"
    );
    fail_if!(
        before_health != after_health,
        "the publisher's health changed when the other side subscribed: {before_health:?} then {after_health:?}"
    );
    let third = start(w.m.clone(), 3, w.frames.clone(), w.epoch);
    let refused = match &third {
        Ok(c) => {
            c.t.subscribe(&session(11), Seen::<Inbound>::default().handler())
                .err()
        }
        Err(_) => {
            w.a.t
                .subscribe(&session(11), Seen::<Inbound>::default().handler())
                .err()
        }
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
    let rct = || {
        w.a.t
            .publish(&dev, payload(PayloadKind::Receipt, b"044"), dl)
    };
    let (e0, r0) = (env(12), rct());
    let _s1 = w.sub(r, &session(12), &Seen::<Inbound>::default());
    let _s2 = w.sub(r, &dev, &Seen::<Inbound>::default());
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
    w.watch(r, at_r.watcher());
    if w.b.is_some() {
        w.watch(&w.a, at_a.watcher());
    }
    let record: Vec<u8> = (0..4096u32).map(|i| (i % 253) as u8).collect();
    let res = w.a.t.send_presence(
        &Destination::Device(r.key.clone()),
        payload(PayloadKind::Presence, &record),
        w.deadline(LONG),
    );
    fail_if!(
        res != PublishResult::Taken,
        "send_presence returned {res:?}"
    );
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
    w.watch(r, seen.watcher());
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
    let _s =
        r.t.subscribe(
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
    w.m.subscribed();
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
    let Ok(c) = start(w.m.clone(), 3, w.frames.clone(), w.epoch) else {
        return Verdict::NotApplicable("a third implementation could not start".into());
    };
    let (at_b, at_c) = (Seen::default(), Seen::default());
    w.watch(b, at_b.watcher());
    w.watch(&c, at_c.watcher());
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
    let _s = w.sub(r, &session(14), &seen);
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
        if let Ok(x) = std::env::var(var)
            && !x.is_empty()
        {
            v.push(x);
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
        if w.a.caps.sealing {
            w.m.audit_destination(&d);
        }
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
