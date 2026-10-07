// SPDX-License-Identifier: Apache-2.0

//! The presence registry (#55, F6): what one implementation, the observer, knows about
//! whether each session is reachable (`spec/session-channels.md` §7.2), and the two answers
//! built on it, discovery (§7.3.2) and the presence half of a send decision (§7.3.3).
//!
//! # What it holds
//!
//! - **Its own sessions.** A session the implementation binds needs no presence record: it
//!   is `online` while bound and not withheld under [SC-ID-154], and `unreachable` after
//!   that (§7.2.1). [`PresenceRegistry::register_own`], [`PresenceRegistry::set_withheld`]
//!   and [`PresenceRegistry::deregister_own`] track it.
//! - **Accepted records** for other sessions: for each session id, the latest record the
//!   registry accepted, the instant it accepted it, the link it arrived on and whether that
//!   link has since been lost ([SC-DLV-040] to [SC-DLV-047]).
//!
//! # Process model
//!
//! The registry is per device, not per session: the per-device process holds device
//! presence and the session bookkeeping (`docs/planning/decisions/C2-process-model.md` §5).
//! A session's exit deregisters that session alone, which then reads `unreachable`; every
//! other session, own or remote, keeps its state. A registry ends with its process, and a
//! new one starts empty, as [SC-ID-156] has every session end with it.
//!
//! # Time
//!
//! Lifetimes are measured on a monotonic clock, from the instant the record was accepted
//! ([SC-DLV-044], [SC-DLV-049]). Every operation that reads time takes the instant from its
//! caller as a [`std::time::Instant`], so tests and the conformance runner script it and no
//! test waits for real time. Nothing here reads a clock on its own, runs a timer or asks
//! anyone for records: records and carrier loss are pushed in by the transport's
//! `watch_presence` handler (`spec/interfaces.md` §6.6), and a state is computed when it is
//! read. There is no polling (§7.1).
//!
//! # Trust
//!
//! The registry takes a record as already authenticated ([SC-DLV-043]). A record from
//! another implementation reaches it only through [`crate::presence_auth`], which checks
//! `spec/security.md` §11 first and applies the cross-implementation cap of [SEC-PRS-007]
//! ([`RecordOrigin::OtherImplementation`]). A presence state is local and is never passed to
//! a transport or a peer ([IFC-TYP-061]): [`PresenceState`] has no wire form.

use crate::capabilities::{SessionCapabilities, SessionDescriptor};
use crate::delivery::ErrorCode;
use crate::ids::{KeyId, SessionId};
use crate::json::Json;
use crate::presence::{PresenceKind, PresenceRecord, PresenceState, PresenceViolation};
use crate::transport::CarrierHandle;
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

/// The longest an announcement from another implementation counts after it was accepted,
/// whatever its `lifetime_ms`: 300000 milliseconds ([SEC-PRS-007]; presence between
/// machines is capped at 5 minutes, operator decision on #45).
pub const CROSS_IMPLEMENTATION_PRESENCE_CAP_MS: u64 = 300_000;

/// Where an accepted record came from, which decides how long an announcement counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordOrigin {
    /// Issued inside this implementation: the announcement counts for its full
    /// `lifetime_ms` (§7.2.4).
    SameImplementation,
    /// Issued by another implementation and authenticated under `spec/security.md` §11:
    /// the announcement counts for at most [`CROSS_IMPLEMENTATION_PRESENCE_CAP_MS`]
    /// ([SEC-PRS-007]).
    OtherImplementation,
}

/// Why the registry discarded a record. A discarded record changes nothing (§7.2.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceDiscard {
    /// The record fails a requirement of §7.2.2 ([SC-DLV-040]).
    Invalid(PresenceViolation),
    /// Its `seq` is not greater than that of the latest record accepted for the session
    /// ([SC-DLV-042]): a late or second copy.
    NotNewer,
    /// A record for a session the registry does not hold, while it holds
    /// [`PresenceRegistry::capacity`] sessions, none of them is `unreachable`, and no
    /// issuer holds more than one session beyond this record's issuer's share.
    Full,
    /// A record for a new session from an issuer that already holds
    /// [`PresenceRegistry::per_issuer_quota`] sessions, none of them `unreachable`.
    IssuerQuota,
}

/// The most sessions, other than its own bound ones, a registry holds unless built with
/// another capacity.
pub const DEFAULT_PRESENCE_CAPACITY: usize = 4096;

/// The share of the capacity one issuer may hold unless the registry is built with another
/// quota: a quarter.
pub const DEFAULT_ISSUER_SHARE_DIVISOR: usize = 4;

/// The outcome of offering a record to the registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceAcceptance {
    /// The record is now the latest for its session. For an announcement,
    /// `effective_lifetime_ms` is how long it counts from now ([SEC-PRS-007]); `None` for a
    /// withdrawal.
    Accepted {
        /// The lifetime the registry applies, in milliseconds.
        effective_lifetime_ms: Option<u64>,
    },
    /// The record was discarded.
    Discarded(PresenceDiscard),
}

#[derive(Clone, Debug)]
struct Held {
    record: PresenceRecord,
    /// The instant the announcement stops counting; `None` for a withdrawal.
    stale_at: Option<Instant>,
    carrier: CarrierHandle,
    carrier_lost: bool,
    /// The key that signed the record ([`PresenceRegistry::accept_signed`]); `None` for a
    /// record from inside this implementation. Quotas count per issuer.
    issuer: Option<KeyId>,
}

impl Held {
    fn state(&self, now: Instant) -> PresenceState {
        match self.stale_at {
            // [SC-DLV-045], [SC-DLV-046]: stale once the lifetime has passed or the
            // carrier is lost.
            Some(at) if !self.carrier_lost && now < at => PresenceState::Online,
            _ => PresenceState::Unreachable,
        }
    }
}

#[derive(Clone, Debug)]
struct Own {
    descriptor: SessionDescriptor,
    withheld: bool,
}

/// The presence registry of one implementation. See the module documentation.
///
/// # Bound
///
/// Besides the sessions the implementation binds now, it holds at most
/// [`PresenceRegistry::capacity`] sessions (default [`DEFAULT_PRESENCE_CAPACITY`]): held
/// records and ended own sessions together. A trusted device that announces fresh session
/// ids cannot grow it past that. When a record for a new session arrives and the registry
/// is full, it first forgets every session that is `unreachable` at that instant, which
/// [SC-DLV-048] permits. An own session that ends while the registry is full makes room by
/// forgetting an ended one. Forgotten session ids are kept, also at most `capacity` of
/// them, until [`PresenceRegistry::take_forgotten`] hands them to the caller, which may then
/// remove their binding-table entries ([SEC-PRS-009]).
///
/// # Shares
///
/// Sessions are counted per issuer: the key that signed the record
/// ([`PresenceRegistry::accept_signed`]), or this implementation for its own records. So
/// that one related device cannot lock other peers out by holding every place:
///
/// - **Quota.** An issuer holds at most [`PresenceRegistry::per_issuer_quota`] sessions
///   (default a quarter of the capacity). A record for a new session past it first forgets
///   that issuer's own `unreachable` sessions, and is otherwise discarded as
///   [`PresenceDiscard::IssuerQuota`].
/// - **Fair share.** When the registry is full of `online` sessions, a record for a new
///   session from an issuer evicts one session of the issuer holding the most, as long as
///   that issuer would still hold more than this one; only when no issuer holds more is the
///   record discarded as [`PresenceDiscard::Full`]. A peer whose session was forgotten,
///   for example after a carrier loss, therefore gets it back on its next announcement
///   however many sessions another device keeps `online`.
///
/// The residual is recorded in `docs/planning/v0.1/11-risks.md`: several colluding related
/// devices can still shrink every issuer's share toward an equal split of the capacity.
#[derive(Clone, Debug)]
pub struct PresenceRegistry {
    own: BTreeMap<SessionId, Own>,
    /// Own sessions whose binding has ended: `unreachable` until forgotten (§7.2.1).
    ended: BTreeMap<SessionId, ()>,
    held: HashMap<SessionId, Held>,
    capacity: usize,
    per_issuer_quota: usize,
    forgotten: Vec<SessionId>,
}

impl Default for PresenceRegistry {
    fn default() -> PresenceRegistry {
        PresenceRegistry::with_capacity(DEFAULT_PRESENCE_CAPACITY)
    }
}

impl PresenceRegistry {
    /// An empty registry: every session is `unknown`.
    pub fn new() -> PresenceRegistry {
        PresenceRegistry::default()
    }

    /// An empty registry that holds at most `capacity` sessions besides its own bound ones
    /// (at least one), each issuer at most a quarter of them (at least one).
    pub fn with_capacity(capacity: usize) -> PresenceRegistry {
        PresenceRegistry::with_limits(capacity, capacity / DEFAULT_ISSUER_SHARE_DIVISOR)
    }

    /// An empty registry of `capacity` sessions, each issuer at most `per_issuer_quota` of
    /// them; each limit is at least one, and the quota at most the capacity.
    pub fn with_limits(capacity: usize, per_issuer_quota: usize) -> PresenceRegistry {
        let capacity = capacity.max(1);
        PresenceRegistry {
            own: BTreeMap::new(),
            ended: BTreeMap::new(),
            held: HashMap::new(),
            capacity,
            per_issuer_quota: per_issuer_quota.clamp(1, capacity),
            forgotten: Vec::new(),
        }
    }

    /// The most sessions one issuer holds.
    pub fn per_issuer_quota(&self) -> usize {
        self.per_issuer_quota
    }

    fn held_by(&self, issuer: &Option<KeyId>) -> usize {
        self.held.values().filter(|h| &h.issuer == issuer).count()
    }

    /// Forgets `session`, held, and notes it.
    fn evict(&mut self, session: &SessionId) {
        if self.held.remove(session).is_some() {
            self.note_forgotten(session.clone());
        }
    }

    /// Makes room for a new session from `issuer` at `now`, by the rules of the type
    /// documentation; `Err` with the reason when there is none.
    fn make_room(&mut self, issuer: &Option<KeyId>, now: Instant) -> Result<(), PresenceDiscard> {
        if self.held_by(issuer) >= self.per_issuer_quota {
            let own_stale: Vec<SessionId> = self
                .held
                .iter()
                .filter(|(_, h)| &h.issuer == issuer && h.state(now) != PresenceState::Online)
                .map(|(s, _)| s.clone())
                .collect();
            for s in &own_stale {
                self.evict(s);
            }
            if self.held_by(issuer) >= self.per_issuer_quota {
                return Err(PresenceDiscard::IssuerQuota);
            }
        }
        if self.len() < self.capacity {
            return Ok(());
        }
        self.sweep(now);
        if self.len() < self.capacity {
            return Ok(());
        }
        // Fair share: one session of the heaviest issuer, when it holds more than this
        // issuer would after the insert.
        let mine = self.held_by(issuer);
        let mut counts: BTreeMap<Option<&KeyId>, usize> = BTreeMap::new();
        for h in self.held.values() {
            *counts.entry(h.issuer.as_ref()).or_default() += 1;
        }
        let heaviest = counts
            .into_iter()
            .max_by_key(|(_, n)| *n)
            .filter(|(_, n)| *n > mine + 1)
            .map(|(k, _)| k.cloned());
        let Some(heavy) = heaviest else {
            return Err(PresenceDiscard::Full);
        };
        let victim = self
            .held
            .iter()
            .filter(|(_, h)| h.issuer == heavy)
            .map(|(s, _)| s.clone())
            .min();
        match victim {
            Some(v) => {
                self.evict(&v);
                Ok(())
            }
            None => Err(PresenceDiscard::Full),
        }
    }

    /// The most sessions held besides the own bound ones.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The number of sessions held besides the own bound ones: held records and ended own
    /// sessions.
    pub fn len(&self) -> usize {
        self.held.len() + self.ended.len()
    }

    /// Whether no session is held besides the own bound ones.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The session ids forgotten to make room since the last call, oldest first.
    pub fn take_forgotten(&mut self) -> Vec<SessionId> {
        std::mem::take(&mut self.forgotten)
    }

    fn note_forgotten(&mut self, s: SessionId) {
        if self.forgotten.len() >= self.capacity {
            self.forgotten.remove(0);
        }
        self.forgotten.push(s);
    }

    /// Forgets every session that is `unreachable` at `now` ([SC-DLV-048]).
    fn sweep(&mut self, now: Instant) {
        let stale: Vec<SessionId> = self
            .held
            .iter()
            .filter(|(s, h)| !self.own.contains_key(*s) && h.state(now) != PresenceState::Online)
            .map(|(s, _)| s.clone())
            .chain(self.ended.keys().cloned())
            .collect();
        for s in stale {
            self.held.remove(&s);
            self.ended.remove(&s);
            self.note_forgotten(s);
        }
    }

    // ---------------------------------------------------------------------------------
    // Own sessions.

    /// Records that this implementation binds the session `descriptor` names, with the
    /// declaration it currently announces ([SC-DLV-065], [SC-DLV-070]). It is `online` from
    /// now on. Registering it again replaces the descriptor, as a changed declaration does
    /// ([SC-DLV-052]).
    pub fn register_own(&mut self, descriptor: SessionDescriptor) {
        let id = descriptor.session_id().clone();
        self.ended.remove(&id);
        let withheld = self.own.get(&id).is_some_and(|o| o.withheld);
        self.own.insert(
            id,
            Own {
                descriptor,
                withheld,
            },
        );
    }

    /// Starts or stops withholding delivery from an own session under [SC-ID-154]. While
    /// withheld it is `unreachable` (§7.2.1). False when the session is not one of the
    /// registry's own.
    pub fn set_withheld(&mut self, session: &SessionId, withheld: bool) -> bool {
        match self.own.get_mut(session) {
            Some(o) => {
                o.withheld = withheld;
                true
            }
            None => false,
        }
    }

    /// Records that an own session's binding ended: the session reads `unreachable` from
    /// now on, until forgotten (§7.2.1, [SC-DLV-048]). Every other session keeps its state:
    /// presence survives one session's exit (C2 §5). False when it was not an own session.
    pub fn deregister_own(&mut self, session: &SessionId) -> bool {
        if self.own.remove(session).is_some() {
            if self.len() >= self.capacity
                && let Some(evicted) = self.ended.keys().next().cloned()
            {
                self.ended.remove(&evicted);
                self.held.remove(&evicted);
                self.note_forgotten(evicted);
            }
            self.ended.insert(session.clone(), ());
            true
        } else {
            false
        }
    }

    /// Whether `session` is one this implementation binds now.
    pub fn is_own(&self, session: &SessionId) -> bool {
        self.own.contains_key(session)
    }

    /// The descriptor an own session currently announces.
    pub fn own_descriptor(&self, session: &SessionId) -> Option<&SessionDescriptor> {
        self.own.get(session).map(|o| &o.descriptor)
    }

    // ---------------------------------------------------------------------------------
    // Records.

    /// Offers a record, as a JSON value, that arrived on `carrier` at `now`. It is read
    /// with [`PresenceRecord::from_json`] first: a record that fails §7.2.2 is discarded
    /// ([SC-DLV-040]).
    pub fn accept_json(
        &mut self,
        record: &Json,
        carrier: CarrierHandle,
        origin: RecordOrigin,
        now: Instant,
    ) -> PresenceAcceptance {
        match PresenceRecord::from_json(record) {
            Ok(r) => self.accept(r, carrier, origin, now),
            Err(v) => PresenceAcceptance::Discarded(PresenceDiscard::Invalid(v)),
        }
    }

    /// Offers an authenticated, well-formed record that arrived on `carrier` at `now`.
    ///
    /// It is accepted when its `seq` is greater than that of the latest record accepted
    /// for its session id ([SC-DLV-042]); a second copy or a late record is discarded and
    /// changes nothing. An accepted announcement counts from `now`, by the registry's
    /// caller's monotonic clock, not from `issued_at` ([SC-DLV-044]), for its `lifetime_ms`,
    /// capped for [`RecordOrigin::OtherImplementation`] ([SEC-PRS-007]).
    pub fn accept(
        &mut self,
        record: PresenceRecord,
        carrier: CarrierHandle,
        origin: RecordOrigin,
        now: Instant,
    ) -> PresenceAcceptance {
        self.insert(record, carrier, origin, None, now)
    }

    /// [`PresenceRegistry::accept`] for a record from another implementation, signed by
    /// `issuer` (`spec/security.md` §11): it counts toward that key's share (see the type
    /// documentation), and its announcement is capped as [`RecordOrigin::OtherImplementation`]
    /// ([SEC-PRS-007]).
    pub fn accept_signed(
        &mut self,
        record: PresenceRecord,
        carrier: CarrierHandle,
        issuer: &KeyId,
        now: Instant,
    ) -> PresenceAcceptance {
        self.insert(
            record,
            carrier,
            RecordOrigin::OtherImplementation,
            Some(issuer.clone()),
            now,
        )
    }

    fn insert(
        &mut self,
        record: PresenceRecord,
        carrier: CarrierHandle,
        origin: RecordOrigin,
        issuer: Option<KeyId>,
        now: Instant,
    ) -> PresenceAcceptance {
        if self
            .held
            .get(record.session_id())
            .is_some_and(|h| record.seq() <= h.record.seq())
        {
            return PresenceAcceptance::Discarded(PresenceDiscard::NotNewer);
        }
        if !self.held.contains_key(record.session_id())
            && let Err(why) = self.make_room(&issuer, now)
        {
            return PresenceAcceptance::Discarded(why);
        }
        let effective_lifetime_ms = match record.kind() {
            PresenceKind::Announcement { lifetime_ms, .. } => Some(match origin {
                RecordOrigin::SameImplementation => *lifetime_ms,
                RecordOrigin::OtherImplementation => {
                    (*lifetime_ms).min(CROSS_IMPLEMENTATION_PRESENCE_CAP_MS)
                }
            }),
            PresenceKind::Withdrawal => None,
        };
        let stale_at = effective_lifetime_ms.map(|ms| now + Duration::from_millis(ms));
        self.held.insert(
            record.session_id().clone(),
            Held {
                record,
                stale_at,
                carrier,
                carrier_lost: false,
                issuer,
            },
        );
        PresenceAcceptance::Accepted {
            effective_lifetime_ms,
        }
    }

    /// The transport reported that the link `carrier` names has ended: every accepted
    /// announcement that arrived on it is stale from now on ([SC-DLV-046]). Carrier loss
    /// only ever moves a session toward `unreachable`; it never makes one `online`, so a
    /// spoofed loss can at worst deny service until the issuer's next announcement
    /// (§7.2.4). Returns the number of sessions it made stale.
    pub fn carrier_loss(&mut self, carrier: &CarrierHandle) -> usize {
        let mut n = 0;
        for h in self.held.values_mut() {
            if &h.carrier == carrier && h.record.present() && !h.carrier_lost {
                h.carrier_lost = true;
                n += 1;
            }
        }
        n
    }

    /// The `seq` of the latest record accepted for `session`, while it is remembered.
    pub fn latest_seq(&self, session: &SessionId) -> Option<u64> {
        self.held.get(session).map(|h| h.record.seq())
    }

    /// Forgets a session whose state is `unreachable` at `now`, with its latest `seq`
    /// ([SC-DLV-048]): it reads `unknown` afterwards. False, and nothing changes, for a
    /// session in any other state. After a forget, [SC-DLV-042] no longer protects the
    /// session against an older record; `spec/security.md` [SEC-PRS-006] and [SEC-PRS-007]
    /// bound that replay for records from other implementations.
    pub fn forget(&mut self, session: &SessionId, now: Instant) -> bool {
        if self.state(session, now) != PresenceState::Unreachable {
            return false;
        }
        let a = self.ended.remove(session).is_some();
        let b = self.held.remove(session).is_some();
        a || b
    }

    // ---------------------------------------------------------------------------------
    // States.

    /// The presence state of `session` at `now` (§7.2.1, [SC-DLV-047]). Exactly one of
    /// the three states ([SC-DLV-020]).
    pub fn state(&self, session: &SessionId, now: Instant) -> PresenceState {
        if let Some(o) = self.own.get(session) {
            return if o.withheld {
                PresenceState::Unreachable
            } else {
                PresenceState::Online
            };
        }
        if self.ended.contains_key(session) {
            return PresenceState::Unreachable;
        }
        match self.held.get(session) {
            Some(h) => h.state(now),
            None => PresenceState::Unknown,
        }
    }

    /// The descriptor of `session` while it is `online` at `now`: the one an own session
    /// currently announces, or that of the latest accepted announcement ([SC-DLV-065],
    /// [SC-DLV-070]). `None` in any other state.
    pub fn online_descriptor(
        &self,
        session: &SessionId,
        now: Instant,
    ) -> Option<&SessionDescriptor> {
        if self.state(session, now) != PresenceState::Online {
            return None;
        }
        if let Some(o) = self.own.get(session) {
            return Some(&o.descriptor);
        }
        match self.held.get(session).map(|h| h.record.kind()) {
            Some(PresenceKind::Announcement { descriptor, .. }) => Some(descriptor),
            _ => None,
        }
    }

    /// Every session id the registry knows: own, ended and held.
    pub fn sessions(&self) -> impl Iterator<Item = &SessionId> {
        self.own.keys().chain(self.ended.keys()).chain(
            self.held
                .keys()
                .filter(|s| !self.own.contains_key(*s) && !self.ended.contains_key(*s)),
        )
    }

    // ---------------------------------------------------------------------------------
    // Answers.

    /// The answer to a discovery request (§7.3.2) that arrived on an attachment bound to
    /// `requester`, at `now`. `may_discover(requester, session)` is the authorization of
    /// `spec/security.md` §9.4.
    ///
    /// # Errors
    ///
    /// `unauthorized` when the attachment is not bound to exactly one session (`requester`
    /// is `None`; [SC-DLV-060]).
    pub fn discover(
        &self,
        requester: Option<&SessionId>,
        may_discover: impl Fn(&SessionId, &SessionId) -> bool,
        now: Instant,
    ) -> Result<Vec<SessionDescriptor>, ErrorCode> {
        discovery_result(
            requester,
            self.sessions()
                .map(|s| (s, self.state(s, now), self.online_descriptor(s, now))),
            may_discover,
        )
    }

    /// The presence step of a send decision (§8.3.3 step 2, §7.3.3) for a request from
    /// `requester` to `to`, at `now`: the capability declaration the sender may use.
    /// `may_discover` is asked first; a session the requester may not discover is
    /// `unknown` to it, and the registry is not consulted at all ([SC-DLV-075],
    /// [SC-DLV-076]).
    ///
    /// # Errors
    ///
    /// `unknown-destination` for a session that is `unknown` as the requester may see it
    /// ([SC-DLV-072]); `destination-unavailable` for one that is `unreachable`
    /// ([SC-DLV-073]).
    pub fn send_presence(
        &self,
        requester: &SessionId,
        to: &SessionId,
        may_discover: impl FnOnce(&SessionId, &SessionId) -> bool,
        now: Instant,
    ) -> Result<&SessionCapabilities, ErrorCode> {
        if !may_discover(requester, to) {
            return Err(ErrorCode::UnknownDestination);
        }
        match self.state(to, now) {
            PresenceState::Unknown => Err(ErrorCode::UnknownDestination),
            PresenceState::Unreachable => Err(ErrorCode::DestinationUnavailable),
            PresenceState::Online => self
                .online_descriptor(to, now)
                .map(SessionDescriptor::capabilities)
                .ok_or(ErrorCode::InternalError),
        }
    }
}

#[derive(Clone, Debug)]
struct Issued {
    next_seq: u64,
    /// While announced: when the latest announcement was issued, on the monotonic clock.
    announced_at: Option<Instant>,
}

/// The issuing side of presence (§7.2.5) for this implementation's own sessions: it numbers
/// each session's records and says when an announcement is due again.
///
/// It issues; it does not send. The caller passes each record to the transport, wrapped per
/// audience by [`crate::presence_auth::AuthenticatedPresenceRecord::issue`] for another
/// implementation. A refresh is a keepalive (§7.1.1): [`PresenceIssuer::next_refresh_at`]
/// gives the one instant a timer must fire at, and nothing asks anyone about waiting
/// envelopes.
#[derive(Clone, Debug)]
pub struct PresenceIssuer {
    lifetime_ms: u64,
    sessions: BTreeMap<SessionId, Issued>,
}

impl PresenceIssuer {
    /// An issuer whose announcements carry `lifetime_ms`, from 1000 to 3600000
    /// ([SC-DLV-028]). For announcements to other implementations it should be at most
    /// [`CROSS_IMPLEMENTATION_PRESENCE_CAP_MS`] ([SEC-PRS-008]). `None` out of range.
    pub fn new(lifetime_ms: u64) -> Option<PresenceIssuer> {
        let (lo, hi) = crate::presence::LIFETIME_MS_RANGE;
        (lo..=hi).contains(&lifetime_ms).then(|| PresenceIssuer {
            lifetime_ms,
            sessions: BTreeMap::new(),
        })
    }

    fn next(&mut self, session: &SessionId) -> &mut Issued {
        self.sessions.entry(session.clone()).or_insert(Issued {
            next_seq: 0,
            announced_at: None,
        })
    }

    /// The announcement for an own session, carrying the declaration the implementation
    /// makes available for it ([SC-DLV-053]): when it starts to expose the session
    /// ([SC-DLV-051]), when its declaration changes ([SC-DLV-052]), when delivery resumes
    /// after [SC-ID-154], and as a refresh ([SC-DLV-054]). Its `seq` is greater than every
    /// earlier one for the session ([SC-DLV-050]). `None` once `seq` is exhausted.
    pub fn announce(
        &mut self,
        descriptor: SessionDescriptor,
        issued_at: crate::ids::Timestamp,
        now: Instant,
    ) -> Option<PresenceRecord> {
        let lifetime_ms = self.lifetime_ms;
        let s = self.next(descriptor.session_id());
        let record =
            PresenceRecord::announcement(s.next_seq, issued_at, lifetime_ms, descriptor).ok()?;
        s.next_seq += 1;
        s.announced_at = Some(now);
        Some(record)
    }

    /// The withdrawal for an own session: when its binding is deregistered ([SC-DLV-055]),
    /// with `ended` true, after which the issuer forgets it; when delivery to it starts to be
    /// withheld ([SC-DLV-056]); and before the implementation stops ([SC-DLV-057]). `None`
    /// once `seq` is exhausted.
    pub fn withdraw(
        &mut self,
        session: &SessionId,
        issued_at: crate::ids::Timestamp,
        ended: bool,
    ) -> Option<PresenceRecord> {
        let s = self.next(session);
        let record = PresenceRecord::withdrawal(session.clone(), s.next_seq, issued_at).ok()?;
        s.next_seq += 1;
        s.announced_at = None;
        if ended {
            self.sessions.remove(session);
        }
        Some(record)
    }

    /// The withdrawals to issue before the implementation stops ([SC-DLV-057]): one for
    /// each session it has announced.
    pub fn withdraw_all(&mut self, issued_at: &crate::ids::Timestamp) -> Vec<PresenceRecord> {
        let announced: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.announced_at.is_some())
            .map(|(k, _)| k.clone())
            .collect();
        announced
            .iter()
            .filter_map(|s| self.withdraw(s, issued_at.clone(), true))
            .collect()
    }

    fn refresh_at(&self, at: Instant) -> Instant {
        // Two fifths of the lifetime: comfortably before half of it ([SC-DLV-054]).
        at + Duration::from_millis(self.lifetime_ms * 2 / 5)
    }

    /// The earliest instant at which an announced session is due for a refresh, for one
    /// timer to fire at; `None` when nothing is announced.
    pub fn next_refresh_at(&self) -> Option<Instant> {
        self.sessions
            .values()
            .filter_map(|s| s.announced_at.map(|a| self.refresh_at(a)))
            .min()
    }

    /// The announced sessions due for a refresh at `now`.
    pub fn refresh_due(&self, now: Instant) -> Vec<SessionId> {
        self.sessions
            .iter()
            .filter(|(_, s)| s.announced_at.is_some_and(|a| now >= self.refresh_at(a)))
            .map(|(k, _)| k.clone())
            .collect()
    }
}

/// A discovery result (§7.3.2) from candidate sessions, each with its presence state and,
/// when `online`, the descriptor [SC-DLV-065] names.
///
/// It lists every candidate that is `online` and that `requester` may discover
/// ([SC-DLV-061] to [SC-DLV-063]), once each, in session-id order ([SC-DLV-064]).
///
/// # Errors
///
/// `unauthorized` when `requester` is `None`, an attachment not bound to exactly one
/// session ([SC-DLV-060]).
pub fn discovery_result<'a>(
    requester: Option<&SessionId>,
    candidates: impl IntoIterator<Item = (&'a SessionId, PresenceState, Option<&'a SessionDescriptor>)>,
    may_discover: impl Fn(&SessionId, &SessionId) -> bool,
) -> Result<Vec<SessionDescriptor>, ErrorCode> {
    let requester = requester.ok_or(ErrorCode::Unauthorized)?;
    let mut out: BTreeMap<&SessionId, &SessionDescriptor> = BTreeMap::new();
    for (session, state, descriptor) in candidates {
        if state != PresenceState::Online || !may_discover(requester, session) {
            continue;
        }
        if let Some(d) = descriptor {
            out.insert(session, d);
        }
    }
    Ok(out.into_values().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::CapabilitiesEntry;
    use crate::ids::{EXTENSION_ID_V0, Timestamp, Version};

    fn sid(s: &str) -> SessionId {
        SessionId::parse(s).unwrap()
    }

    fn descriptor(s: &SessionId) -> SessionDescriptor {
        let caps = SessionCapabilities::declare([(
            EXTENSION_ID_V0,
            CapabilitiesEntry::new(Version { major: 0, minor: 1 }, true),
        )])
        .unwrap();
        SessionDescriptor::new(s.clone(), caps, None, None).unwrap()
    }

    fn at() -> Timestamp {
        Timestamp::parse("2026-10-03T12:00:00.000Z").unwrap()
    }

    fn announce(s: &SessionId, seq: u64, lifetime_ms: u64) -> PresenceRecord {
        PresenceRecord::announcement(seq, at(), lifetime_ms, descriptor(s)).unwrap()
    }

    fn withdraw(s: &SessionId, seq: u64) -> PresenceRecord {
        PresenceRecord::withdrawal(s.clone(), seq, at()).unwrap()
    }

    fn carrier(c: &str) -> CarrierHandle {
        CarrierHandle::from_opaque(c.as_bytes().to_vec())
    }

    const B: &str = "7gq3m8z2c5k9t1w4x6b0n2r8vd";
    const C: &str = "5mt9x2k7q4w8c1n6b3r0v5z2pd";
    const OWN: &str = "01harn7x9k2m4p6q8r0s2t4v6w";

    /// [SC-DLV-044], [SC-DLV-045], [SC-DLV-047]: unknown, then online for the lifetime from
    /// acceptance, then unreachable at exactly acceptance plus lifetime.
    #[test]
    fn unknown_online_unreachable_by_expiry() {
        let t0 = Instant::now();
        let b = sid(B);
        let mut r = PresenceRegistry::new();
        assert_eq!(r.state(&b, t0), PresenceState::Unknown);
        let t1 = t0 + Duration::from_secs(10);
        assert_eq!(
            r.accept(
                announce(&b, 1, 60_000),
                carrier("x"),
                RecordOrigin::SameImplementation,
                t1
            ),
            PresenceAcceptance::Accepted {
                effective_lifetime_ms: Some(60_000)
            }
        );
        assert_eq!(r.state(&b, t1), PresenceState::Online);
        let just_before = t1 + Duration::from_millis(59_999);
        assert_eq!(r.state(&b, just_before), PresenceState::Online);
        assert_eq!(
            r.state(&b, t1 + Duration::from_millis(60_000)),
            PresenceState::Unreachable
        );
        // A refresh before expiry keeps it online, counted from the refresh.
        let t2 = t1 + Duration::from_millis(30_000);
        r.accept(
            announce(&b, 2, 60_000),
            carrier("x"),
            RecordOrigin::SameImplementation,
            t2,
        );
        assert_eq!(
            r.state(&b, t1 + Duration::from_millis(80_000)),
            PresenceState::Online
        );
        assert_eq!(
            r.state(&b, t2 + Duration::from_millis(60_000)),
            PresenceState::Unreachable
        );
        // [SC-DLV-048]: forgetting an unreachable session makes it unknown.
        let late = t2 + Duration::from_secs(100);
        assert!(r.forget(&b, late));
        assert_eq!(r.state(&b, late), PresenceState::Unknown);
        assert_eq!(r.latest_seq(&b), None);
    }

    /// [SC-DLV-042]: a duplicate or late record changes nothing; withdrawal, then a
    /// re-announcement with a greater seq.
    #[test]
    fn seq_orders_records() {
        let t = Instant::now();
        let b = sid(B);
        let mut r = PresenceRegistry::new();
        r.accept(
            withdraw(&b, 5),
            carrier("x"),
            RecordOrigin::SameImplementation,
            t,
        );
        assert_eq!(r.state(&b, t), PresenceState::Unreachable);
        assert_eq!(
            r.accept(
                announce(&b, 4, 60_000),
                carrier("x"),
                RecordOrigin::SameImplementation,
                t
            ),
            PresenceAcceptance::Discarded(PresenceDiscard::NotNewer)
        );
        assert_eq!(
            r.accept(
                withdraw(&b, 5),
                carrier("x"),
                RecordOrigin::SameImplementation,
                t
            ),
            PresenceAcceptance::Discarded(PresenceDiscard::NotNewer)
        );
        assert_eq!(r.state(&b, t), PresenceState::Unreachable);
        r.accept(
            announce(&b, 6, 60_000),
            carrier("x"),
            RecordOrigin::SameImplementation,
            t,
        );
        assert_eq!(r.state(&b, t), PresenceState::Online);
        // An online session cannot be forgotten.
        assert!(!r.forget(&b, t));
    }

    /// [SC-DLV-046]: carrier loss makes every announcement from that link stale, and only
    /// those; a later announcement brings the session back.
    #[test]
    fn carrier_loss_is_per_link() {
        let t = Instant::now();
        let (b, c) = (sid(B), sid(C));
        let mut r = PresenceRegistry::new();
        r.accept(
            announce(&b, 1, 60_000),
            carrier("x"),
            RecordOrigin::SameImplementation,
            t,
        );
        r.accept(
            announce(&c, 1, 60_000),
            carrier("y"),
            RecordOrigin::SameImplementation,
            t,
        );
        assert_eq!(r.carrier_loss(&carrier("x")), 1);
        assert_eq!(r.state(&b, t), PresenceState::Unreachable);
        assert_eq!(r.state(&c, t), PresenceState::Online);
        r.accept(
            announce(&b, 2, 60_000),
            carrier("z"),
            RecordOrigin::SameImplementation,
            t,
        );
        assert_eq!(r.state(&b, t), PresenceState::Online);
    }

    /// [SEC-PRS-007]: an announcement from another implementation counts for at most 300
    /// seconds, whatever its lifetime; one from the same implementation keeps its own.
    #[test]
    fn cross_implementation_cap() {
        let t = Instant::now();
        let (b, c) = (sid(B), sid(C));
        let mut r = PresenceRegistry::new();
        assert_eq!(
            r.accept(
                announce(&b, 1, 3_600_000),
                carrier("x"),
                RecordOrigin::OtherImplementation,
                t
            ),
            PresenceAcceptance::Accepted {
                effective_lifetime_ms: Some(300_000)
            }
        );
        r.accept(
            announce(&c, 1, 3_600_000),
            carrier("x"),
            RecordOrigin::SameImplementation,
            t,
        );
        let later = t + Duration::from_millis(300_000);
        assert_eq!(
            r.state(&b, later - Duration::from_millis(1)),
            PresenceState::Online
        );
        assert_eq!(r.state(&b, later), PresenceState::Unreachable);
        assert_eq!(r.state(&c, later), PresenceState::Online);
    }

    /// Own sessions: online while bound, unreachable while withheld or after their binding
    /// ends. One session's exit leaves every other session's presence as it was (C2 §5).
    #[test]
    fn presence_survives_one_session_exit() {
        let t = Instant::now();
        let (own, b, c) = (sid(OWN), sid(B), sid(C));
        let mut r = PresenceRegistry::new();
        r.register_own(descriptor(&own));
        r.register_own(descriptor(&c));
        r.accept(
            announce(&b, 1, 60_000),
            carrier("x"),
            RecordOrigin::SameImplementation,
            t,
        );
        assert_eq!(r.state(&own, t), PresenceState::Online);
        assert!(r.set_withheld(&own, true));
        assert_eq!(r.state(&own, t), PresenceState::Unreachable);
        assert!(r.set_withheld(&own, false));
        assert!(r.deregister_own(&own));
        assert_eq!(r.state(&own, t), PresenceState::Unreachable);
        assert_eq!(r.state(&c, t), PresenceState::Online);
        assert_eq!(r.state(&b, t), PresenceState::Online);
        assert!(r.forget(&own, t));
        assert_eq!(r.state(&own, t), PresenceState::Unknown);
    }

    /// The bound: fresh session ids from a peer fill the registry to its capacity and no
    /// further while all are `online`; once some are `unreachable` they are forgotten to make
    /// room and handed to the caller; ended own sessions count and make room too.
    #[test]
    fn bounded_by_capacity() {
        let t = Instant::now();
        let mut r = PresenceRegistry::with_limits(3, 3);
        let ids: Vec<SessionId> = (1..=5u8)
            .map(|n| SessionId::from_random_octets([n; 16]))
            .collect();
        for s in &ids[..3] {
            assert!(matches!(
                r.accept(
                    announce(s, 1, 60_000),
                    carrier("x"),
                    RecordOrigin::OtherImplementation,
                    t
                ),
                PresenceAcceptance::Accepted { .. }
            ));
        }
        assert_eq!(
            r.accept(
                announce(&ids[3], 1, 60_000),
                carrier("x"),
                RecordOrigin::OtherImplementation,
                t
            ),
            // One issuer: its quota (here the whole capacity) stops it.
            PresenceAcceptance::Discarded(PresenceDiscard::IssuerQuota)
        );
        assert_eq!(r.len(), 3);
        // A newer record for a held session still fits.
        assert!(matches!(
            r.accept(
                withdraw(&ids[0], 2),
                carrier("x"),
                RecordOrigin::OtherImplementation,
                t
            ),
            PresenceAcceptance::Accepted { .. }
        ));
        // ids[0] is now unreachable: it is forgotten to make room.
        assert!(matches!(
            r.accept(
                announce(&ids[3], 1, 60_000),
                carrier("x"),
                RecordOrigin::OtherImplementation,
                t
            ),
            PresenceAcceptance::Accepted { .. }
        ));
        assert_eq!(r.take_forgotten(), vec![ids[0].clone()]);
        assert_eq!(r.state(&ids[0], t), PresenceState::Unknown);
        // After every lifetime has run out, all of them make room.
        let later = t + Duration::from_secs(61);
        assert!(matches!(
            r.accept(
                announce(&ids[4], 1, 60_000),
                carrier("x"),
                RecordOrigin::OtherImplementation,
                later
            ),
            PresenceAcceptance::Accepted { .. }
        ));
        assert_eq!(r.len(), 1);
        assert_eq!(r.take_forgotten().len(), 3);
        // Ended own sessions: bounded the same way.
        let mut o = PresenceRegistry::with_limits(2, 2);
        for s in &ids {
            o.register_own(descriptor(s));
            assert!(o.deregister_own(s));
            assert!(o.len() <= 2);
        }
        // Three were forgotten; the list keeps the newest `capacity` of them.
        assert_eq!(o.take_forgotten(), vec![ids[1].clone(), ids[2].clone()]);
    }

    /// PR #321 re-review N10: one related device cannot lock other peers out. Its quota
    /// stops it at its share; when the registry is full of its `online` sessions, a peer's
    /// new session, or a peer's session forgotten after a carrier loss and announced again,
    /// evicts one of its sessions.
    #[test]
    fn one_issuer_cannot_lock_others_out() {
        let t = Instant::now();
        let k = |c: char| KeyId::parse(&c.to_string().repeat(64)).unwrap();
        let (hog, peer) = (k('a'), k('b'));
        let ids: Vec<SessionId> = (1..=12u8)
            .map(|n| SessionId::from_random_octets([n; 16]))
            .collect();
        // Quota: the hog holds at most 3 of 4.
        let mut q = PresenceRegistry::with_limits(4, 3);
        for s in &ids[..3] {
            assert!(matches!(
                q.accept_signed(announce(s, 1, 60_000), carrier("h"), &hog, t),
                PresenceAcceptance::Accepted { .. }
            ));
        }
        assert_eq!(
            q.accept_signed(announce(&ids[3], 1, 60_000), carrier("h"), &hog, t),
            PresenceAcceptance::Discarded(PresenceDiscard::IssuerQuota)
        );
        assert!(matches!(
            q.accept_signed(announce(&ids[4], 1, 60_000), carrier("p"), &peer, t),
            PresenceAcceptance::Accepted { .. }
        ));
        // Fair share: with no quota in the way, the hog fills the registry with `online`
        // sessions; the peer's session, lost and forgotten, still comes back.
        let mut r = PresenceRegistry::with_limits(4, 4);
        r.accept_signed(announce(&ids[0], 1, 60_000), carrier("p"), &peer, t);
        for s in &ids[1..4] {
            r.accept_signed(announce(s, 1, 60_000), carrier("h"), &hog, t);
        }
        assert_eq!(r.len(), 4);
        assert_eq!(r.carrier_loss(&carrier("p")), 1);
        // The hog's next fresh session sweeps the peer's stale one away.
        assert!(matches!(
            r.accept_signed(announce(&ids[5], 1, 60_000), carrier("h"), &hog, t),
            PresenceAcceptance::Accepted { .. }
        ));
        assert_eq!(r.state(&ids[0], t), PresenceState::Unknown);
        // The peer announces it again: one of the hog's four makes room.
        assert!(matches!(
            r.accept_signed(announce(&ids[0], 2, 60_000), carrier("p2"), &peer, t),
            PresenceAcceptance::Accepted { .. }
        ));
        assert_eq!(r.state(&ids[0], t), PresenceState::Online);
        assert_eq!(r.len(), 4);
        // And a second peer session too (hog 3, peer 1 -> hog 2, peer 2) ...
        assert!(matches!(
            r.accept_signed(announce(&ids[6], 1, 60_000), carrier("p2"), &peer, t),
            PresenceAcceptance::Accepted { .. }
        ));
        // ... but not past an equal split: hog 2, peer 2, so a third is refused.
        assert_eq!(
            r.accept_signed(announce(&ids[7], 1, 60_000), carrier("p2"), &peer, t),
            PresenceAcceptance::Discarded(PresenceDiscard::Full)
        );
    }

    /// [SC-DLV-050] to [SC-DLV-057]: seq grows across announce, withdraw and re-announce;
    /// refreshes fall due before half the lifetime; every announced session is withdrawn on
    /// stop; a consumer takes the issued records in order.
    #[test]
    fn issuer_numbers_and_refreshes() {
        let t = Instant::now();
        let (a, b) = (sid(OWN), sid(B));
        let mut i = PresenceIssuer::new(60_000).unwrap();
        assert!(PresenceIssuer::new(999).is_none());
        assert_eq!(i.next_refresh_at(), None);
        let r0 = i.announce(descriptor(&a), at(), t).unwrap();
        let r1 = i.withdraw(&a, at(), false).unwrap();
        let r2 = i.announce(descriptor(&a), at(), t).unwrap();
        assert_eq!((r0.seq(), r1.seq(), r2.seq()), (0, 1, 2));
        i.announce(descriptor(&b), at(), t + Duration::from_secs(1))
            .unwrap();
        assert_eq!(i.next_refresh_at(), Some(t + Duration::from_millis(24_000)));
        assert!(i.refresh_due(t + Duration::from_millis(23_999)).is_empty());
        assert_eq!(
            i.refresh_due(t + Duration::from_millis(24_000)),
            vec![a.clone()]
        );
        let mut consumer = PresenceRegistry::new();
        for r in [r0, r1, r2] {
            assert!(matches!(
                consumer.accept(r, carrier("i"), RecordOrigin::SameImplementation, t),
                PresenceAcceptance::Accepted { .. }
            ));
        }
        let stop = i.withdraw_all(&at());
        assert_eq!(stop.len(), 2);
        assert!(stop.iter().all(|r| !r.present()));
        assert_eq!(i.next_refresh_at(), None);
    }

    /// [SC-DLV-060] to [SC-DLV-065], and the send-side presence step of §8.3.3.
    #[test]
    fn discovery_and_send_presence() {
        let t = Instant::now();
        let (own, b, c) = (sid(OWN), sid(B), sid(C));
        let mut r = PresenceRegistry::new();
        r.register_own(descriptor(&own));
        r.accept(
            announce(&b, 1, 60_000),
            carrier("x"),
            RecordOrigin::SameImplementation,
            t,
        );
        r.accept(
            announce(&c, 1, 60_000),
            carrier("x"),
            RecordOrigin::SameImplementation,
            t,
        );
        r.accept(
            withdraw(&c, 2),
            carrier("x"),
            RecordOrigin::SameImplementation,
            t,
        );
        assert_eq!(
            r.discover(None, |_, _| true, t),
            Err(ErrorCode::Unauthorized)
        );
        let all = r.discover(Some(&own), |_, _| true, t).unwrap();
        let ids: Vec<&str> = all.iter().map(|d| d.session_id().as_str()).collect();
        assert_eq!(ids, vec![OWN, B]);
        let none = r
            .discover(Some(&own), |_, s| s != &b && s != &own, t)
            .unwrap();
        assert!(none.is_empty());
        assert!(r.send_presence(&own, &b, |_, _| true, t).is_ok());
        assert_eq!(
            r.send_presence(&own, &c, |_, _| true, t),
            Err(ErrorCode::DestinationUnavailable)
        );
        // [SC-DLV-075]: hidden from the requester means unknown, whatever is held.
        assert_eq!(
            r.send_presence(&own, &c, |_, _| false, t),
            Err(ErrorCode::UnknownDestination)
        );
        assert_eq!(
            r.send_presence(&own, &sid("6wd4k1q7z3m9c2t6x0b5n8r1vf"), |_, _| true, t),
            Err(ErrorCode::UnknownDestination)
        );
    }
}
