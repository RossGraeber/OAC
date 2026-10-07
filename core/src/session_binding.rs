// SPDX-License-Identifier: Apache-2.0

//! Binding a session id to a live session from native signals (`spec/session-channels.md`
//! §6.7; #331): the pairing outcome, the ordered cases of §6.7.3, the stale-binding rule of
//! §6.7.4 and the local records they require.
//!
//! [`decide`] is the whole decision for one native signal. It takes the attachments of one
//! adapter as the core sees them (an [`AttachmentState`] each), the [`NativeSignal`] the
//! adapter reported, and the [`Pairing`] the core made for it, and returns a
//! [`BindingDecision`]: one binding result, the change to make, and the records to write.
//! It changes nothing itself. [`apply`] makes the change on a list of states, which is what
//! the conformance runner checks (§6.10, stage `binding`); [`crate::pipeline::Pipelines`]
//! makes it on its own attachments instead, with a registration record and a transport
//! subscription.
//!
//! # Pairing
//!
//! The pairing key is what the core process observes from the operating system about the
//! peer of a local connection ([SC-ID-121]; `spec/interfaces.md` §5.2), never a value the
//! peer sent: a [`PairingKey`] has no constructor from a request, an event or an envelope,
//! only [`PairingKey::from_observation`], for the code that made the observation. A
//! cross-check value is never a pairing key ([SC-ID-122], [SC-ID-127]); nothing here pairs on
//! one.
//!
//! *UNVERIFIED (`spec/session-channels.md` §6.7.2, dated note of 2026-10-03):* which
//! operating-system facility yields such a key on each platform is open, owned by G9 (#70).
//! Until a caller supplies one, no connection has a key and every native signal is
//! unpairable ([SC-ID-125]): it fails closed with a finding ([SC-ID-129]). That costs
//! availability, never authority.
//!
//! Nothing here names a harness: `native_id` and the cross-check values are compared as
//! exact strings ([SC-ID-143]) and never interpreted (`spec/interfaces.md` Table C.1).

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::adapter::{Attachment, NativeSignal, StartKind};
use crate::ids::{SessionId, Token};

/// A pairing key: what the core process observed from the operating system about the peer
/// of one local connection ([SC-ID-121]). Two connections pair when their keys are equal.
///
/// It is opaque, compared as exact octets, and never leaves the process: it has no
/// serialization, and its `Debug` form shows its length only.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PairingKey(Box<[u8]>);

impl PairingKey {
    /// The key for an observation the core process made itself, through an operating-system
    /// facility, of a connection's peer (for example the identity of the harness process
    /// it descends from). Never call it with a value the peer, an adapter or an envelope
    /// supplied ([SC-ID-121]; `spec/interfaces.md` [IFC-ADP-031]).
    pub fn from_observation(observed: impl Into<Box<[u8]>>) -> PairingKey {
        PairingKey(observed.into())
    }
}

impl fmt::Debug for PairingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PairingKey({} octets)", self.0.len())
    }
}

/// What the core process observed about the peer of one local connection when it accepted
/// and authenticated it ([IFC-ADP-012]): the pairing key, and the scope a registration
/// record made for an attachment on it needs (`spec/session-channels.md` §6.7.1). Each is
/// `None` when it was not observed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PeerObservation {
    /// The pairing key ([SC-ID-121]).
    pub pairing_key: Option<PairingKey>,
    /// The harness label of the registration record: the label of the endpoint the
    /// connection was accepted on. Display only ([SC-ID-043]).
    pub harness_label: Option<Token>,
    /// The working directory of the peer's harness process, the registration record's
    /// scope (`docs/planning/decisions/C4-session-identity.md` §5). Never published
    /// ([SC-ID-045], [SEC-KEY-042]).
    pub working_directory: Option<String>,
}

impl PeerObservation {
    /// Nothing observed: no pairing key, so a native signal on the connection, or one that
    /// would pair with it, is unpairable ([SC-ID-125]).
    pub fn none() -> PeerObservation {
        PeerObservation::default()
    }
}

/// The pairing the core made for one native signal (§6.7.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pairing<A> {
    /// Paired, with certainty, with this attachment.
    Paired(A),
    /// Held as not yet pairable, and the bounded window ended first ([SC-ID-124]).
    WindowExpired,
    /// Unpairable: no observed key, or more than one candidate attachment ([SC-ID-125]).
    Unpairable,
}

/// The binding of one attachment: the native id N and the session id bound to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeBinding {
    /// N.
    pub native_id: String,
    /// The session id.
    pub session_id: SessionId,
}

/// One attachment as the binding decision sees it (§6.10, "attachment list").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentState<A> {
    /// The attachment.
    pub attachment: A,
    /// The attachment's cross-check value, reported to its own process. A hint for logs
    /// only until a signal is paired with it ([SC-ID-126]).
    pub cross_check: Option<String>,
    /// Its binding, or `None` when it is unbound.
    pub binding: Option<NativeBinding>,
}

/// A binding result of §6.7.3: a local outcome, never a delivery state (§6.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BindingResult {
    /// `unchanged` (case 1).
    Unchanged,
    /// `refused` (case 2).
    Refused,
    /// `failed-closed` (case 3(b), or an unpairable signal).
    FailedClosed,
    /// `bound` (cases 3(a), 3(c) and 4).
    Bound,
    /// `dropped` (a signal dropped at the end of the window).
    Dropped,
}

impl BindingResult {
    /// The result's name in §6.7.3.
    pub fn as_str(self) -> &'static str {
        match self {
            BindingResult::Unchanged => "unchanged",
            BindingResult::Refused => "refused",
            BindingResult::FailedClosed => "failed-closed",
            BindingResult::Bound => "bound",
            BindingResult::Dropped => "dropped",
        }
    }
}

/// The kind of a local record (§6.7.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RecordKind {
    /// A diagnostic: for troubleshooting only.
    Diagnostic,
    /// A finding: for an operator to review, since it might show a misconfiguration or an
    /// attack.
    Finding,
}

impl RecordKind {
    /// The kind's name in §6.10: `diagnostic` or `finding`.
    pub fn as_str(self) -> &'static str {
        match self {
            RecordKind::Diagnostic => "diagnostic",
            RecordKind::Finding => "finding",
        }
    }
}

/// One local record a decision requires, with the requirement that requires it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BindingRecord {
    /// The kind.
    pub kind: RecordKind,
    /// The requirement id, such as `SC-ID-132`.
    pub requirement: &'static str,
}

/// The change a decision makes to the attachments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingAction<A> {
    /// No change.
    None,
    /// Bind `attachment` to `native_id` under a new session id ([SC-ID-136], [SC-ID-139],
    /// [SC-ID-140]), deregistering its earlier registration record first when it has one
    /// ([SC-ID-150]). The new session carries no authorization state of the earlier one
    /// ([SC-ID-151]).
    Bind {
        /// The attachment.
        attachment: A,
        /// N.
        native_id: String,
        /// Whether the attachment's earlier registration record is deregistered first.
        deregister_first: bool,
    },
    /// Deregister `attachment`'s registration record: a stale binding ([SC-ID-152]). It
    /// ends unbound.
    Deregister {
        /// The attachment.
        attachment: A,
    },
}

/// The decision for one native signal: the binding result, the change, and the records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingDecision<A> {
    /// The binding result.
    pub result: BindingResult,
    /// The change to make.
    pub action: BindingAction<A>,
    /// The records to write, in the order the rules require them.
    pub records: Vec<BindingRecord>,
}

impl<A> BindingDecision<A> {
    /// The strongest record the decision requires, as §6.10 reports it: a finding over a
    /// diagnostic; `None` for none.
    pub fn record(&self) -> Option<RecordKind> {
        self.records.iter().map(|r| r.kind).max()
    }
}

fn finding(requirement: &'static str) -> BindingRecord {
    BindingRecord {
        kind: RecordKind::Finding,
        requirement,
    }
}

fn diagnostic(requirement: &'static str) -> BindingRecord {
    BindingRecord {
        kind: RecordKind::Diagnostic,
        requirement,
    }
}

/// The decision for `signal`, paired as `pairing`, over `attachments`: the attachments of
/// the adapter that reported the signal, as they stand before it (§6.7.2 to §6.7.4).
///
/// The cases are applied in order and the first that matches decides (§6.7.3). Native ids
/// and cross-check values are compared as exact strings ([SC-ID-143]). A `Paired`
/// attachment that is not in `attachments` cannot be paired with certainty and fails closed
/// ([SC-ID-125]).
pub fn decide<A: Clone + Eq>(
    attachments: &[AttachmentState<A>],
    signal: &NativeSignal,
    pairing: &Pairing<A>,
) -> BindingDecision<A> {
    let none = |result, records| BindingDecision {
        result,
        action: BindingAction::None,
        records,
    };
    let att = match pairing {
        // [SC-ID-124], [SC-ID-128].
        Pairing::WindowExpired => {
            return none(BindingResult::Dropped, vec![diagnostic("SC-ID-128")]);
        }
        // [SC-ID-125], [SC-ID-129].
        Pairing::Unpairable => {
            return none(BindingResult::FailedClosed, vec![finding("SC-ID-129")]);
        }
        Pairing::Paired(a) => match attachments.iter().find(|s| &s.attachment == a) {
            Some(s) => s,
            None => return none(BindingResult::FailedClosed, vec![finding("SC-ID-129")]),
        },
    };
    let n = signal.native_id.as_str();
    // Case 1, same N ([SC-ID-130]), whatever the start kind and the cross-check values.
    if att.binding.as_ref().is_some_and(|b| b.native_id == n) {
        return none(BindingResult::Unchanged, Vec::new());
    }
    // The attachment is bound to a different N: a refusal or a fail-closed below is a stale
    // binding (§6.7.4), and a bind deregisters the earlier record first ([SC-ID-150]).
    let bound_to_other = att.binding.is_some();
    let stale = |mut d: BindingDecision<A>| {
        if bound_to_other {
            // [SC-ID-152], [SC-ID-153].
            d.action = BindingAction::Deregister {
                attachment: att.attachment.clone(),
            };
            d.records.push(finding("SC-ID-153"));
        }
        d
    };
    let bind = |records| BindingDecision {
        result: BindingResult::Bound,
        action: BindingAction::Bind {
            attachment: att.attachment.clone(),
            native_id: n.to_owned(),
            deregister_first: bound_to_other,
        },
        records,
    };
    // Case 2, duplicate N: bound to another attachment that is still live. Only N is
    // compared, never a cross-check value ([SC-ID-134]); the other binding is untouched
    // ([SC-ID-133]).
    let duplicate = attachments.iter().any(|s| {
        s.attachment != att.attachment && s.binding.as_ref().is_some_and(|b| b.native_id == n)
    });
    if duplicate {
        // [SC-ID-131], [SC-ID-132].
        return stale(none(BindingResult::Refused, vec![finding("SC-ID-132")]));
    }
    // A cross-check value differs when it is present and not equal to N.
    let differs = |e: &Option<String>| e.as_deref().is_some_and(|e| e != n);
    let any_differs = differs(&att.cross_check) || differs(&signal.cross_check);
    match signal.start_kind {
        // Case 4, transition: no cross-check value is compared for the decision
        // ([SC-ID-140], [SC-ID-127]); a differing one is a diagnostic ([SC-ID-142]).
        Some(StartKind::Transition) => bind(if any_differs {
            vec![diagnostic("SC-ID-142")]
        } else {
            Vec::new()
        }),
        // Case 3, fresh start; a missing or unmapped start kind is `fresh` ([SC-ID-135]).
        Some(StartKind::Fresh) | Some(StartKind::Unmapped) | None => {
            if any_differs {
                // Case 3(b) ([SC-ID-137], [SC-ID-138]).
                stale(none(
                    BindingResult::FailedClosed,
                    vec![finding("SC-ID-138")],
                ))
            } else if att.cross_check.is_some() {
                // Case 3(a) ([SC-ID-136]).
                bind(Vec::new())
            } else {
                // Case 3(c) ([SC-ID-139], [SC-ID-141]).
                bind(vec![diagnostic("SC-ID-141")])
            }
        }
    }
}

/// Makes `decision`'s change on `attachments`, taking each new session id from
/// `new_session` ([SC-ID-003]; [`fresh_session_id`] outside a test).
pub fn apply<A: Clone + Eq>(
    attachments: &mut [AttachmentState<A>],
    decision: &BindingDecision<A>,
    new_session: impl FnOnce() -> SessionId,
) {
    match &decision.action {
        BindingAction::None => {}
        BindingAction::Bind {
            attachment,
            native_id,
            ..
        } => {
            if let Some(s) = attachments.iter_mut().find(|s| &s.attachment == attachment) {
                // [SC-ID-150]: the earlier binding goes first; the new one replaces it.
                s.binding = Some(NativeBinding {
                    native_id: native_id.clone(),
                    session_id: new_session(),
                });
            }
        }
        BindingAction::Deregister { attachment } => {
            if let Some(s) = attachments.iter_mut().find(|s| &s.attachment == attachment) {
                s.binding = None;
            }
        }
    }
}

/// A fresh session id: 128 bits of the operating system's cryptographically secure random
/// number generator, derived from no input ([SC-ID-003], [SC-ID-004]).
pub fn fresh_session_id() -> SessionId {
    let mut octets = [0u8; 16];
    getrandom::fill(&mut octets).expect("the operating system's random number generator");
    SessionId::from_random_octets(octets)
}

/// One line of the binding log: a record a decision required (§6.7), with what it concerns.
/// It names attachments and session ids only: no native id, cross-check value or content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingLogEntry {
    /// The record.
    pub record: BindingRecord,
    /// The binding result of the decision that required it.
    pub result: BindingResult,
    /// The attachment the signal was paired with, or the one a withholding concerns.
    pub attachment: Option<Attachment>,
    /// The session id concerned: the one deregistered, withheld or kept.
    pub session: Option<SessionId>,
}

/// Where [`crate::pipeline::Pipelines`] writes the findings and diagnostics of §6.7.
pub trait BindingLog: Send {
    /// Writes one entry.
    fn record(&mut self, entry: BindingLogEntry);
}

/// A [`BindingLog`] in memory that keeps the latest `capacity` entries. Clones share one
/// list, so a test (or a status command) can keep a clone and read what was written.
#[derive(Clone, Debug)]
pub struct MemoryBindingLog {
    entries: Arc<Mutex<VecDeque<BindingLogEntry>>>,
    capacity: usize,
}

impl MemoryBindingLog {
    /// An empty log that keeps at most `capacity` entries (at least one), forgetting the
    /// oldest past it.
    pub fn new(capacity: usize) -> MemoryBindingLog {
        MemoryBindingLog {
            entries: Arc::default(),
            capacity: capacity.max(1),
        }
    }

    /// The entries kept, oldest first.
    pub fn entries(&self) -> Vec<BindingLogEntry> {
        self.entries
            .lock()
            .map(|e| e.iter().cloned().collect())
            .unwrap_or_default()
    }
}

impl BindingLog for MemoryBindingLog {
    fn record(&mut self, entry: BindingLogEntry) {
        if let Ok(mut e) = self.entries.lock() {
            if e.len() >= self.capacity {
                e.pop_front();
            }
            e.push_back(entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid(n: u8) -> SessionId {
        SessionId::from_random_octets([n; 16])
    }

    fn att(
        name: &'static str,
        e: Option<&str>,
        bound: Option<(&str, u8)>,
    ) -> AttachmentState<&'static str> {
        AttachmentState {
            attachment: name,
            cross_check: e.map(str::to_owned),
            binding: bound.map(|(n, s)| NativeBinding {
                native_id: n.to_owned(),
                session_id: sid(s),
            }),
        }
    }

    fn signal(n: &str, s: Option<StartKind>, e: Option<&str>) -> NativeSignal {
        NativeSignal {
            native_id: n.to_owned(),
            start_kind: s,
            cross_check: e.map(str::to_owned),
            connection: None,
        }
    }

    const FRESH: Option<StartKind> = Some(StartKind::Fresh);
    const TRANSITION: Option<StartKind> = Some(StartKind::Transition);

    #[test]
    fn a_paired_attachment_outside_the_list_fails_closed() {
        // [SC-ID-125]: it cannot be paired with certainty.
        let d = decide(
            &[att("A", None, None)],
            &signal("n", FRESH, None),
            &Pairing::Paired("B"),
        );
        assert_eq!(d.result, BindingResult::FailedClosed);
        assert_eq!(d.action, BindingAction::None);
        assert_eq!(d.record(), Some(RecordKind::Finding));
    }

    #[test]
    fn comparisons_are_exact() {
        // [SC-ID-143]: case and surrounding space make a different value.
        let list = [att("A", Some("Native-X"), None)];
        let d = decide(
            &list,
            &signal("native-x", FRESH, None),
            &Pairing::Paired("A"),
        );
        assert_eq!(d.result, BindingResult::FailedClosed);
        let list = [att("A", None, Some(("native-x", 1)))];
        let d = decide(
            &list,
            &signal("native-x ", FRESH, None),
            &Pairing::Paired("A"),
        );
        // Not case 1: a different N, so case 3(c) re-binds after deregistering.
        assert_eq!(d.result, BindingResult::Bound);
        assert!(matches!(
            d.action,
            BindingAction::Bind {
                deregister_first: true,
                ..
            }
        ));
    }

    #[test]
    fn a_rebind_deregisters_first_and_takes_a_new_session_id() {
        // [SC-ID-150], [SC-ID-007]: a transition to another N is a new binding.
        let mut list = vec![att("A", Some("x"), Some(("x", 1)))];
        let d = decide(&list, &signal("y", TRANSITION, None), &Pairing::Paired("A"));
        assert_eq!(
            d.action,
            BindingAction::Bind {
                attachment: "A",
                native_id: "y".into(),
                deregister_first: true
            }
        );
        assert_eq!(d.record(), Some(RecordKind::Diagnostic), "[SC-ID-142]");
        apply(&mut list, &d, || sid(2));
        assert_eq!(list[0].binding.as_ref().unwrap().session_id, sid(2));
    }

    #[test]
    fn a_stale_refusal_records_both_findings() {
        // Case 2 on an attachment bound elsewhere: [SC-ID-132] and [SC-ID-153].
        let list = [
            att("A", None, Some(("x", 1))),
            att("B", None, Some(("y", 2))),
        ];
        let d = decide(&list, &signal("x", TRANSITION, None), &Pairing::Paired("B"));
        assert_eq!(d.result, BindingResult::Refused);
        assert_eq!(d.action, BindingAction::Deregister { attachment: "B" });
        let ids: Vec<_> = d.records.iter().map(|r| r.requirement).collect();
        assert_eq!(ids, ["SC-ID-132", "SC-ID-153"]);
    }

    #[test]
    fn fresh_session_ids_are_random() {
        // [SC-ID-003], [SC-ID-008]: two draws differ.
        assert_ne!(fresh_session_id(), fresh_session_id());
    }

    #[test]
    fn the_memory_log_is_bounded() {
        let mut log = MemoryBindingLog::new(2);
        for r in ["SC-ID-128", "SC-ID-129", "SC-ID-132"] {
            log.record(BindingLogEntry {
                record: finding(r),
                result: BindingResult::Refused,
                attachment: None,
                session: None,
            });
        }
        let kept: Vec<_> = log.entries().iter().map(|e| e.record.requirement).collect();
        assert_eq!(kept, ["SC-ID-129", "SC-ID-132"]);
    }

    #[test]
    fn a_pairing_key_shows_no_content() {
        let k = PairingKey::from_observation(b"pid-1234".to_vec());
        assert_eq!(format!("{k:?}"), "PairingKey(8 octets)");
        assert_eq!(k, PairingKey::from_observation(b"pid-1234".to_vec()));
    }
}
