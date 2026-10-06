// SPDX-License-Identifier: Apache-2.0

//! Replay defence and duplicate suppression (`spec/security.md` §8): step 3 and step 5 of
//! the security stage (§7.1, Table 7.1), and the hand-off deadline that bounds both
//! (`spec/session-channels.md` §8.1.3).
//!
//! Step 4, authorization, sits between them: [`crate::authorization`] (#54, F5). Step 5
//! takes only the [`AuthorizedMessage`] that step 4 produces, so a copy refused at step 4
//! can never add an entry. A receiver runs, for each envelope:
//!
//! 1. [`crate::envelope::receive_envelope`]: envelope-stage validation;
//! 2. [`crate::signing::authenticate`]: steps 1 and 2. Only after them is `created_at`
//!    trusted ([SEC-STG-003]);
//! 3. [`check_replay_window`]: step 3, `outside-replay-window` with state `expired`;
//! 4. [`crate::authorization::AuthorizationEngine::authorize_delivery`]: step 4,
//!    `unauthorized`. A copy refused there adds no entry, because it has no
//!    [`AuthorizedMessage`];
//! 5. [`DuplicateStore::admit`]: step 5, which tests for an entry and adds one as a single
//!    step ([SEC-RPL-021]), waiting while an earlier copy is being handed off
//!    ([SEC-RPL-026]);
//! 6. the delivery stage, whose step 4 re-checks [`HandOffDeadline`] immediately before the
//!    hand-off call ([SC-RCP-091]);
//! 7. [`Reservation::handed_off`] when the hand-off call succeeded or its outcome is
//!    indeterminate, [`Reservation::not_handed_off`] when the delivery stage refused the
//!    copy or the call failed ([SEC-RPL-022]).
//!
//! A copy that passes all of this is still untrusted content (`spec/security.md` §1.2):
//! suppression decides whether to hand a copy off, never whether to obey it.
//!
//! # Timing (§13.1)
//!
//! Step 3 reads `created_at` and the clock, and nothing else: no work in it depends on the
//! addressed session, so it gives an unauthorized sender no timing signal about `to`
//! ([SEC-STG-005]). Step 5 runs only after authorization, so whatever it reveals is
//! revealed to an authorized sender only.
//!
//! # The store
//!
//! One store per device (`docs/planning/decisions/C5-envelope-auth.md` §8), keyed by the
//! duplicate key (`security.key_id`, `security.nonce`), compared as exact strings
//! ([SEC-RPL-020]). It holds no content.
//!
//! - **Eviction.** An entry is kept until the envelope's hand-off deadline, read on the
//!   store's clock ([SEC-RPL-023]), and evicted at or after it ([SEC-RPL-024]): lazily, on
//!   each admission, and on [`DuplicateStore::evict_expired`]. No copy is handed off at or
//!   after the deadline ([SC-RCP-091]), so forgetting the entry then lets no duplicate
//!   through. An entry whose hand-off call is still running is never evicted; it settles
//!   first.
//! - **Bound.** Eviction bounds the store by the inbound rate times the window
//!   (C5 §8). On top of that it holds at most [`DuplicateStore::capacity`] entries
//!   (default [`DEFAULT_CAPACITY`]). An authorized copy that finds the store full of live
//!   entries is refused with `failed` and `internal-error`, an error unrelated to the
//!   envelope's validity whose next step is to retransmit (`spec/session-channels.md`
//!   Table 8.3), and adds nothing. Nothing is evicted early to make room, because that
//!   could let a duplicate through ([SEC-RPL-023]). Only an authorized sender can fill the
//!   store, since entries are added after step 4.
//! - **Restart.** The store is process memory and is not persisted
//!   (C5 §8, "In-memory, not on-disk"; [SEC-RPL-025] permits this). A new process starts
//!   with an empty store, so for up to the replay window after a restart a copy of an
//!   envelope handed off before it can be handed off again. Only the window bounds that
//!   residual; delivery is not exactly-once (`spec/session-channels.md` §8). A store kept
//!   across restart would not be a mailbox either, but none is kept.
//! - **Receipts.** At most one `duplicate` receipt per entry ([SEC-RPL-030]):
//!   [`Admission::Duplicate`] says whether this copy may draw it.

use crate::authorization::AuthorizedMessage;
use crate::clock::Clock;
use crate::delivery::{DeliveryState, ErrorCode};
use crate::envelope::{ChannelMessage, Envelope};
use crate::ids::Timestamp;
use crate::signing::SecurityRejection;
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

/// The replay window width `W`, in milliseconds: exactly 300000 ([SEC-RPL-001]). It is also
/// the replay-window skew allowance a sender adds to its retry deadline (§8.1).
pub const REPLAY_WINDOW_MS: u64 = 300_000;

const WINDOW_NANOS: i128 = REPLAY_WINDOW_MS as i128 * 1_000_000;

/// The most entries a [`DuplicateStore`] holds unless built with another capacity.
pub const DEFAULT_CAPACITY: usize = 65_536;

/// A message that has not passed steps 1 and 2: its `created_at` and nonce are not yet
/// trusted ([SEC-STG-003]). Reaching steps 3 or 5 with one is a defect in the caller, so it
/// is refused with `internal-error` rather than checked.
fn unverified() -> SecurityRejection {
    SecurityRejection {
        state: DeliveryState::Failed,
        error: ErrorCode::InternalError,
        requirement: "SEC-STG-003",
    }
}

/// Whether `created_at` is inside the replay window at receiver time `now`: `now - W < c <
/// now + W`, both ends open (§8.1), compared in nanoseconds, the full precision of both
/// values ([SEC-RPL-003]).
pub fn inside_replay_window(created_at: &Timestamp, now: &Timestamp) -> bool {
    let (c, t) = (created_at.unix_nanos(), now.unix_nanos());
    t - WINDOW_NANOS < c && c < t + WINDOW_NANOS
}

/// Step 3 of the security stage (Table 7.1): the replay window on arrival ([SEC-RPL-002]).
/// `now` is the receiver's clock at arrival. It reads nothing but `created_at`
/// ([SEC-STG-005]).
///
/// # Errors
///
/// `expired` with `outside-replay-window` when `created_at` is outside the window; `failed`
/// with `internal-error` when `msg` has not passed steps 1 and 2.
pub fn check_replay_window(msg: &ChannelMessage, now: &Timestamp) -> Result<(), SecurityRejection> {
    if msg.verified_by().is_none() {
        return Err(unverified());
    }
    if inside_replay_window(msg.envelope().created_at(), now) {
        Ok(())
    } else {
        Err(SecurityRejection {
            state: DeliveryState::Expired,
            error: ErrorCode::OutsideReplayWindow,
            requirement: "SEC-RPL-002",
        })
    }
}

/// Which instant an envelope's hand-off deadline is (`spec/session-channels.md` §8.1.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingBound {
    /// The expiry instant, `created_at` plus `ttl_ms`: it is not later than the end of the
    /// replay window. Refused with the code `expired`.
    Expiry,
    /// The end of the replay window, `created_at` plus `W`: the envelope has no `ttl_ms`, or
    /// its expiry instant is later. Refused with the code `outside-replay-window`.
    ReplayWindow,
}

/// An envelope's hand-off deadline: the earlier of its expiry instant and the end of the
/// replay window for its `created_at` (`spec/session-channels.md` §8.1.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandOffDeadline {
    unix_nanos: i128,
    bound: BindingBound,
}

impl HandOffDeadline {
    /// The deadline of `env`.
    pub fn of(env: &Envelope) -> HandOffDeadline {
        let window_end = env.created_at().unix_nanos() + WINDOW_NANOS;
        match env.expiry_unix_nanos() {
            Some(e) if e <= window_end => HandOffDeadline {
                unix_nanos: e,
                bound: BindingBound::Expiry,
            },
            _ => HandOffDeadline {
                unix_nanos: window_end,
                bound: BindingBound::ReplayWindow,
            },
        }
    }

    /// The instant, in nanoseconds since the epoch.
    pub fn unix_nanos(&self) -> i128 {
        self.unix_nanos
    }

    /// Which of the two instants it is.
    pub fn bound(&self) -> BindingBound {
        self.bound
    }

    /// Whether `now` is at or after the deadline.
    pub fn passed_at(&self, now: &Timestamp) -> bool {
        now.unix_nanos() >= self.unix_nanos
    }

    /// The re-check immediately before the hand-off call (delivery stage, step 4;
    /// [SC-RCP-091]): `None` while the envelope may be handed off at `now`; otherwise the
    /// state and code to report ([SC-RCP-092]), `expired` with `expired` or with
    /// `outside-replay-window` by the binding bound.
    pub fn refusal_at(&self, now: &Timestamp) -> Option<(DeliveryState, ErrorCode)> {
        self.passed_at(now).then_some((
            DeliveryState::Expired,
            match self.bound {
                BindingBound::Expiry => ErrorCode::Expired,
                BindingBound::ReplayWindow => ErrorCode::OutsideReplayWindow,
            },
        ))
    }
}

/// The duplicate key of an envelope: (`security.key_id`, `security.nonce`) (§8.3). Not the
/// envelope's `id`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DuplicateKey {
    key_id: String,
    nonce: String,
}

impl DuplicateKey {
    /// The key for a key id and a nonce, as written. They are compared as exact strings
    /// ([SEC-RPL-020]).
    pub fn new(key_id: &str, nonce: &str) -> DuplicateKey {
        DuplicateKey {
            key_id: key_id.to_owned(),
            nonce: nonce.to_owned(),
        }
    }

    /// The key of `msg`, once steps 1 and 2 have verified it: the key id it verified under
    /// and its signed nonce. `None` before then ([SEC-STG-003]).
    pub fn of(msg: &ChannelMessage) -> Option<DuplicateKey> {
        let by = msg.verified_by()?;
        Some(DuplicateKey::new(
            by.key_id().as_str(),
            msg.envelope().security().nonce(),
        ))
    }

    /// The key id.
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// The nonce.
    pub fn nonce(&self) -> &str {
        &self.nonce
    }
}

/// The outcome of step 5 for a copy.
#[derive(Debug)]
pub enum Admission {
    /// No entry held the copy's key; one is now held for it, in flight. The copy goes on to
    /// the delivery stage, and the reservation records how that ended.
    Admitted(Reservation),
    /// An earlier copy was handed off or may have been: report `duplicate` and do not hand
    /// this one off ([SC-RCP-008]). `receipt_allowed` is true for the first such copy of an
    /// entry only ([SEC-RPL-030]).
    Duplicate {
        /// Whether this copy may draw the entry's one `duplicate` receipt.
        receipt_allowed: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// The copy that added the entry has not been handed off yet, or its hand-off call has
    /// not returned.
    InFlight,
    /// Handed off, or the outcome is indeterminate: the harness may hold the content.
    HandedOff,
}

#[derive(Debug)]
struct Entry {
    deadline: i128,
    phase: Phase,
    duplicate_receipt_sent: bool,
}

#[derive(Debug, Default)]
struct Inner {
    entries: HashMap<DuplicateKey, Entry>,
    by_deadline: BTreeSet<(i128, DuplicateKey)>,
}

impl Inner {
    /// Removes every settled entry whose deadline is at or before `now` ([SEC-RPL-024]).
    ///
    /// An in-flight entry is skipped even past its deadline: a copy waiting on it in
    /// [`DuplicateStore::admit`] must see its outcome ([SEC-RPL-026]). No fixture covers
    /// this guard; the unit test `an_in_flight_entry_is_not_evicted` does. It is defence in
    /// depth, since the hand-off re-check ([SC-RCP-091]) also stops a copy that waited past
    /// the deadline.
    fn evict(&mut self, now: i128) {
        let due: Vec<(i128, DuplicateKey)> = self
            .by_deadline
            .iter()
            .take_while(|(d, _)| *d <= now)
            .filter(|(_, k)| {
                self.entries
                    .get(k)
                    .is_some_and(|e| e.phase == Phase::HandedOff)
            })
            .cloned()
            .collect();
        for item in due {
            self.entries.remove(&item.1);
            self.by_deadline.remove(&item);
        }
    }

    fn remove(&mut self, key: &DuplicateKey) {
        if let Some(e) = self.entries.remove(key) {
            self.by_deadline.remove(&(e.deadline, key.clone()));
        }
    }

    fn insert(&mut self, key: DuplicateKey, deadline: i128, phase: Phase) {
        self.by_deadline.insert((deadline, key.clone()));
        self.entries.insert(
            key,
            Entry {
                deadline,
                phase,
                duplicate_receipt_sent: false,
            },
        );
    }
}

struct Shared {
    inner: Mutex<Inner>,
    settled: Condvar,
    clock: Arc<dyn Clock>,
    capacity: usize,
}

/// The duplicate store of `spec/security.md` §8.3. A handle: clones share one store.
#[derive(Clone)]
pub struct DuplicateStore {
    shared: Arc<Shared>,
}

impl fmt::Debug for DuplicateStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DuplicateStore")
            .field("len", &self.len())
            .field("capacity", &self.shared.capacity)
            .finish()
    }
}

fn full() -> SecurityRejection {
    SecurityRejection {
        state: DeliveryState::Failed,
        error: ErrorCode::InternalError,
        requirement: "SEC-RPL-023",
    }
}

impl DuplicateStore {
    /// An empty store of [`DEFAULT_CAPACITY`] entries, which reads `clock` for eviction.
    pub fn new(clock: Arc<dyn Clock>) -> DuplicateStore {
        DuplicateStore::with_capacity(clock, DEFAULT_CAPACITY)
    }

    /// An empty store of at most `capacity` entries.
    pub fn with_capacity(clock: Arc<dyn Clock>, capacity: usize) -> DuplicateStore {
        DuplicateStore {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner::default()),
                settled: Condvar::new(),
                clock,
                capacity,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.shared
            .inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The most entries the store holds.
    pub fn capacity(&self) -> usize {
        self.shared.capacity
    }

    /// The number of entries held, in flight or handed off.
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    /// Whether the store holds no entry.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the store holds an entry for `key`.
    pub fn contains(&self, key: &DuplicateKey) -> bool {
        self.lock().entries.contains_key(key)
    }

    /// Evicts every settled entry whose hand-off deadline has passed on the store's clock
    /// ([SEC-RPL-024]). Admission does this too; a receiver may also call it on a timer.
    pub fn evict_expired(&self) {
        let now = self.shared.clock.now().unix_nanos();
        self.lock().evict(now);
    }

    /// **Test-only. Production code must not call it.** It seeds an entry with no
    /// verification: it records that a copy with `key`, whose hand-off deadline is
    /// `deadline`, was handed off. It exists for the conformance runner
    /// (`core/tests/conformance.rs`), which preloads a fixture's `duplicate_store`
    /// (`spec/security.md` §3.3). As an integration test, the runner can reach only the
    /// public API, so this is hidden from the documentation rather than `pub(crate)`. An
    /// existing entry is kept as it is.
    ///
    /// # Errors
    ///
    /// `failed` with `internal-error` when the store is full.
    #[doc(hidden)]
    pub fn insert_handed_off(
        &self,
        key: DuplicateKey,
        deadline: &Timestamp,
    ) -> Result<(), SecurityRejection> {
        let now = self.shared.clock.now().unix_nanos();
        let mut inner = self.lock();
        inner.evict(now);
        if inner.entries.contains_key(&key) {
            return Ok(());
        }
        if inner.entries.len() >= self.shared.capacity {
            return Err(full());
        }
        inner.insert(key, deadline.unix_nanos(), Phase::HandedOff);
        Ok(())
    }

    /// Step 5 of the security stage for `msg`, which has passed steps 1 to 4: tests for an
    /// entry and, when there is none, adds one, as a single step that no other copy can
    /// interleave with ([SEC-RPL-021]).
    ///
    /// When the entry found belongs to a copy whose hand-off call has not returned, this
    /// blocks until that copy's [`Reservation`] is settled, and then decides again
    /// ([SEC-RPL-026]): the copy is a duplicate only if the entry remains. The wait is
    /// bounded by that hand-off call. A receiver that holds the message across the wait
    /// re-checks the hand-off deadline before its own hand-off ([SC-RCP-091]).
    ///
    /// The caller's obligations (PR #317 review, N2):
    ///
    /// - **No re-entry before settling.** A thread that holds an unsettled [`Reservation`]
    ///   for a key and calls `admit` again for the same key deadlocks: it waits for a
    ///   settlement only it can make. Settle the reservation first, or use
    ///   [`DuplicateStore::try_admit`], which never waits.
    /// - **Re-check the deadline before every hand-off.** Call [`HandOffDeadline::refusal_at`]
    ///   right before each hand-off call. A copy admitted after waiting may already be past
    ///   its deadline.
    /// - **Bound the wait.** Every replayed copy of an envelope whose hand-off is running
    ///   parks one thread here, and replays of a captured envelope pass step 4. Either put a
    ///   time limit on the hand-off call, so its reservation settles, or use `try_admit` and
    ///   re-queue the copy.
    ///
    /// # Errors
    ///
    /// `failed` with `internal-error` when the store is full of live entries or `msg` is
    /// not verified.
    pub fn admit(&self, msg: &AuthorizedMessage) -> Result<Admission, SecurityRejection> {
        let (key, deadline) = Self::key_and_deadline(msg)?;
        let mut inner = self.lock();
        loop {
            if let Some(decided) = self.decide(&mut inner, &key, deadline) {
                return decided;
            }
            inner = self
                .shared
                .settled
                .wait(inner)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// [`DuplicateStore::admit`] without the wait: `Ok(None)` when an earlier copy with the
    /// same key is being handed off, and nothing is added. The caller retries once that
    /// hand-off has settled; deciding `duplicate` now would break [SEC-RPL-026].
    ///
    /// # Errors
    ///
    /// As [`DuplicateStore::admit`].
    pub fn try_admit(
        &self,
        msg: &AuthorizedMessage,
    ) -> Result<Option<Admission>, SecurityRejection> {
        let (key, deadline) = Self::key_and_deadline(msg)?;
        let mut inner = self.lock();
        self.decide(&mut inner, &key, deadline).transpose()
    }

    fn key_and_deadline(
        msg: &AuthorizedMessage,
    ) -> Result<(DuplicateKey, i128), SecurityRejection> {
        let msg = msg.message();
        let key = DuplicateKey::of(msg).ok_or_else(unverified)?;
        Ok((key, HandOffDeadline::of(msg.envelope()).unix_nanos()))
    }

    /// `None` while the entry for `key` is in flight.
    fn decide(
        &self,
        inner: &mut Inner,
        key: &DuplicateKey,
        deadline: i128,
    ) -> Option<Result<Admission, SecurityRejection>> {
        inner.evict(self.shared.clock.now().unix_nanos());
        let full_now = inner.entries.len() >= self.shared.capacity;
        match inner.entries.get_mut(key) {
            Some(Entry {
                phase: Phase::InFlight,
                ..
            }) => None,
            Some(e) => {
                let receipt_allowed = !e.duplicate_receipt_sent;
                e.duplicate_receipt_sent = true;
                Some(Ok(Admission::Duplicate { receipt_allowed }))
            }
            None if full_now => Some(Err(full())),
            None => {
                inner.insert(key.clone(), deadline, Phase::InFlight);
                Some(Ok(Admission::Admitted(Reservation {
                    store: self.clone(),
                    key: key.clone(),
                    settled: false,
                })))
            }
        }
    }

    fn settle(&self, key: &DuplicateKey, handed_off: bool) {
        let mut inner = self.lock();
        if handed_off {
            if let Some(e) = inner.entries.get_mut(key) {
                e.phase = Phase::HandedOff;
            }
        } else {
            inner.remove(key);
        }
        drop(inner);
        self.shared.settled.notify_all();
    }
}

/// The entry an admitted copy added. Settle it with how the copy's delivery ended.
///
/// Dropped unsettled, for example by a panic during the hand-off call, it counts as
/// [`Reservation::handed_off`]: the outcome is indeterminate, so the harness may hold the
/// content ([SC-RCP-006]), and keeping the entry cannot hand the content off twice.
#[must_use = "settle the reservation with the outcome of the copy's delivery"]
#[derive(Debug)]
pub struct Reservation {
    store: DuplicateStore,
    key: DuplicateKey,
    settled: bool,
}

impl Reservation {
    /// The duplicate key the entry holds.
    pub fn key(&self) -> &DuplicateKey {
        &self.key
    }

    /// The hand-off call succeeded (`handed-to-harness`) or its outcome is indeterminate
    /// (`unknown`): the entry stays until the deadline ([SEC-RPL-022], [SEC-RPL-023]).
    pub fn handed_off(mut self) {
        self.settled = true;
        self.store.settle(&self.key, true);
    }

    /// The copy was not handed off: the delivery stage refused it, or the hand-off call
    /// failed (`handoff-failed`). The entry is removed, so a later retransmission is not a
    /// duplicate ([SEC-RPL-022]), and a copy waiting on this one decides again
    /// ([SEC-RPL-026]).
    pub fn not_handed_off(mut self) {
        self.settled = true;
        self.store.settle(&self.key, false);
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.settled {
            self.store.settle(&self.key, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use crate::envelope::{EnvelopeDraft, EnvelopeLimits, TextPart, receive_envelope};
    use crate::ids::{SessionId, Token};
    use crate::keys::{DeviceIdentity, DeviceKey};
    use crate::signing::authenticate;
    use crate::trust::TrustedKeySet;
    use std::sync::mpsc;
    use std::thread;

    const SECOND: i128 = 1_000_000_000;

    fn ts(s: &str) -> Timestamp {
        Timestamp::parse(s).unwrap()
    }

    struct Fx {
        alice: DeviceIdentity,
        keys: TrustedKeySet,
        clock: Arc<ManualClock>,
    }

    fn fx() -> Fx {
        let alice =
            DeviceIdentity::new(DeviceKey::generate(), Token::parse("principal-a").unwrap());
        let keys = TrustedKeySet::new(&alice);
        Fx {
            alice,
            keys,
            clock: Arc::new(ManualClock::new(ts("2026-10-03T12:00:01.000Z"))),
        }
    }

    fn draft(created_at: &str, ttl_ms: Option<u64>, to: &str) -> EnvelopeDraft {
        let d = EnvelopeDraft::new(
            Token::parse("msg-1").unwrap(),
            SessionId::parse("01harn7x9k2m4p6q8r0s2t4v6w").unwrap(),
            SessionId::parse(to).unwrap(),
            ts(created_at),
            vec![TextPart::new("hello").unwrap()],
        )
        .unwrap();
        match ttl_ms {
            Some(t) => d.with_ttl_ms(t).unwrap(),
            None => d,
        }
    }

    impl Fx {
        /// A fresh envelope (a fresh nonce), received at `created_at` and verified.
        fn signed(&self, created_at: &str, ttl_ms: Option<u64>) -> Envelope {
            self.alice
                .sign_envelope(draft(created_at, ttl_ms, "7gq3m8z2c5k9t1w4x6b0n2r8vd"))
        }

        /// A copy of `env` as it arrives now, through envelope stage and steps 1 and 2.
        fn arrive(&self, env: &Envelope) -> ChannelMessage {
            let msg = receive_envelope(env.octets(), &EnvelopeLimits::default(), &self.clock.now())
                .unwrap();
            authenticate(msg, &self.keys).unwrap()
        }

        fn authorized(&self, env: &Envelope) -> AuthorizedMessage {
            AuthorizedMessage::for_tests(self.arrive(env))
        }

        fn store(&self, capacity: usize) -> DuplicateStore {
            DuplicateStore::with_capacity(self.clock.clone(), capacity)
        }
    }

    fn admitted(a: Result<Admission, SecurityRejection>) -> Reservation {
        match a {
            Ok(Admission::Admitted(r)) => r,
            other => panic!("expected admitted, got {other:?}"),
        }
    }

    fn duplicate(a: Result<Admission, SecurityRejection>) -> bool {
        match a {
            Ok(Admission::Duplicate { receipt_allowed }) => receipt_allowed,
            other => panic!("expected duplicate, got {other:?}"),
        }
    }

    #[test]
    fn window_is_open_at_both_ends_at_full_precision() {
        let now = ts("2026-10-03T12:05:00.000000001Z");
        let inside = |c: &str| inside_replay_window(&ts(c), &now);
        // [SEC-RPL-002]: exactly W in the past or the future is outside.
        assert!(!inside("2026-10-03T12:00:00.000000001Z"));
        assert!(inside("2026-10-03T12:00:00.000000002Z"));
        assert!(!inside("2026-10-03T12:10:00.000000001Z"));
        assert!(inside("2026-10-03T12:10:00.000000000Z"));
        // [SEC-RPL-003]: one nanosecond past the edge is outside; truncating `now` to
        // milliseconds would accept it.
        assert!(!inside("2026-10-03T12:00:00Z"));
        assert_eq!(REPLAY_WINDOW_MS, 300_000); // [SEC-RPL-001]
    }

    #[test]
    fn step_3_refuses_with_outside_replay_window_and_only_after_verification() {
        let f = fx();
        let env = f.signed("2026-10-03T12:00:00.000Z", None);
        let msg = f.arrive(&env);
        assert!(check_replay_window(&msg, &f.clock.now()).is_ok());
        let rej = check_replay_window(&msg, &ts("2026-10-03T12:05:00.000Z")).unwrap_err();
        assert_eq!(
            (rej.state, rej.error, rej.requirement),
            (
                DeliveryState::Expired,
                ErrorCode::OutsideReplayWindow,
                "SEC-RPL-002"
            )
        );
        // [SEC-STG-003]: an unverified message never reaches the comparison.
        let raw =
            receive_envelope(env.octets(), &EnvelopeLimits::default(), &f.clock.now()).unwrap();
        assert_eq!(
            check_replay_window(&raw, &f.clock.now()).unwrap_err(),
            unverified()
        );
        assert!(DuplicateKey::of(&raw).is_none());
        assert_eq!(
            f.store(4)
                .admit(&AuthorizedMessage::for_tests(raw))
                .unwrap_err(),
            unverified()
        );
    }

    /// [SEC-STG-005]: two envelopes that differ only in `to` get the same step-3 verdict at
    /// every instant; the step reads no member that names the session.
    #[test]
    fn step_3_does_not_depend_on_the_addressed_session() {
        let f = fx();
        let a = f.arrive(&f.alice.sign_envelope(draft(
            "2026-10-03T12:00:00.000Z",
            None,
            "7gq3m8z2c5k9t1w4x6b0n2r8vd",
        )));
        let b = f.arrive(&f.alice.sign_envelope(draft(
            "2026-10-03T12:00:00.000Z",
            None,
            "5mt9x2k7q4w8c1n6b3r0v5z2pd",
        )));
        for now in [
            "2026-10-03T11:55:00.000Z",
            "2026-10-03T11:55:00.001Z",
            "2026-10-03T12:04:59.999Z",
            "2026-10-03T12:05:00.000Z",
        ] {
            assert_eq!(
                check_replay_window(&a, &ts(now)),
                check_replay_window(&b, &ts(now))
            );
        }
    }

    #[test]
    fn hand_off_deadline_and_binding_bound() {
        let f = fx();
        let c = "2026-10-03T12:00:00.000Z";
        let end = ts("2026-10-03T12:05:00.000Z").unix_nanos();
        let none = HandOffDeadline::of(&f.signed(c, None));
        assert_eq!(
            (none.unix_nanos(), none.bound()),
            (end, BindingBound::ReplayWindow)
        );
        let short = HandOffDeadline::of(&f.signed(c, Some(60_000)));
        assert_eq!(
            (short.unix_nanos(), short.bound()),
            (end - 240 * SECOND, BindingBound::Expiry)
        );
        // Equal instants: the expiry is not later than the end of the window.
        let equal = HandOffDeadline::of(&f.signed(c, Some(300_000)));
        assert_eq!(
            (equal.unix_nanos(), equal.bound()),
            (end, BindingBound::Expiry)
        );
        let long = HandOffDeadline::of(&f.signed(c, Some(86_400_000)));
        assert_eq!(
            (long.unix_nanos(), long.bound()),
            (end, BindingBound::ReplayWindow)
        );
        // [SC-RCP-091], [SC-RCP-092]: refused at the deadline, not one instant before.
        assert_eq!(long.refusal_at(&ts("2026-10-03T12:04:59.999999999Z")), None);
        assert_eq!(
            long.refusal_at(&ts("2026-10-03T12:05:00Z")),
            Some((DeliveryState::Expired, ErrorCode::OutsideReplayWindow))
        );
        assert_eq!(
            short.refusal_at(&ts("2026-10-03T12:01:00Z")),
            Some((DeliveryState::Expired, ErrorCode::Expired))
        );
    }

    #[test]
    fn first_copy_admitted_later_copies_duplicate_one_receipt() {
        let f = fx();
        let store = f.store(16);
        let env = f.signed("2026-10-03T12:00:00.000Z", Some(600_000));
        admitted(store.admit(&f.authorized(&env))).handed_off();
        f.clock.advance_nanos(29 * SECOND);
        assert!(duplicate(store.admit(&f.authorized(&env)))); // [SEC-RPL-030]
        assert!(!duplicate(store.admit(&f.authorized(&env))));
        assert!(!duplicate(store.admit(&f.authorized(&env))));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn duplicate_key_is_key_id_and_nonce_compared_exactly() {
        let f = fx();
        let store = f.store(16);
        let env = f.signed("2026-10-03T12:00:00.000Z", None);
        let key = DuplicateKey::of(&f.arrive(&env)).unwrap();
        assert_eq!(key.key_id(), f.alice.key_id().as_str());
        assert_eq!(key.nonce(), env.security().nonce());
        let deadline = ts("2026-10-03T12:05:00Z");
        store.insert_handed_off(key.clone(), &deadline).unwrap();
        assert!(store.contains(&key));
        // Another key id with the same nonce, and a nonce differing in case, are other keys.
        assert!(!store.contains(&DuplicateKey::new("0".repeat(64).as_str(), key.nonce())));
        assert!(!store.contains(&DuplicateKey::new(
            key.key_id(),
            &key.nonce().to_ascii_uppercase()
        )));
        // A second envelope (a new nonce) is not a duplicate of the first.
        let other = f.signed("2026-10-03T12:00:00.000Z", None);
        admitted(store.admit(&f.authorized(&other))).handed_off();
        assert!(duplicate(store.admit(&f.authorized(&env))));
    }

    #[test]
    fn unknown_keeps_the_entry_and_failure_releases_it() {
        let f = fx();
        let store = f.store(16);
        // `unknown`: the harness may hold the content ([SEC-RPL-022]).
        let a = f.signed("2026-10-03T12:00:00.000Z", None);
        admitted(store.admit(&f.authorized(&a))).handed_off();
        assert!(duplicate(store.admit(&f.authorized(&a))));
        // Refused by the delivery stage, or `handoff-failed`: the entry goes.
        let b = f.signed("2026-10-03T12:00:00.000Z", None);
        admitted(store.admit(&f.authorized(&b))).not_handed_off();
        assert!(!store.contains(&DuplicateKey::of(&f.arrive(&b)).unwrap()));
        admitted(store.admit(&f.authorized(&b))).handed_off();
        // Dropped unsettled: indeterminate, kept.
        let c = f.signed("2026-10-03T12:00:00.000Z", None);
        drop(admitted(store.admit(&f.authorized(&c))));
        assert!(duplicate(store.admit(&f.authorized(&c))));
    }

    #[test]
    fn in_flight_copy_is_not_decided_until_the_first_settles() {
        let f = fx();
        let store = f.store(16);
        let env = f.signed("2026-10-03T12:00:00.000Z", None);
        let first = admitted(store.admit(&f.authorized(&env)));
        assert!(store.try_admit(&f.authorized(&env)).unwrap().is_none());
        first.not_handed_off();
        // [SEC-RPL-026], fixture SEC-RPL-026.p01: the earlier hand-off failed, so this copy
        // proceeds as if it had found no entry.
        let second = admitted(store.try_admit(&f.authorized(&env)).map(Option::unwrap));
        assert!(store.try_admit(&f.authorized(&env)).unwrap().is_none());
        second.handed_off();
        assert!(duplicate(
            store.try_admit(&f.authorized(&env)).map(Option::unwrap)
        ));
    }

    /// [SEC-RPL-021] under true concurrency (Appendix A: "atomicity under true concurrency:
    /// TODO(fixture), F4"): many threads admit copies of one envelope at once. Exactly one
    /// is admitted per hand-off; the others wait for its outcome ([SEC-RPL-026]); when the
    /// first hand-off fails, exactly one waiter takes over; once one succeeds, every other
    /// copy is a duplicate, and exactly one of them may draw a receipt.
    ///
    /// One round catches a test-and-add split with no gap about half the time (PR #317
    /// review, N4), so the test runs 20 rounds, each with a fresh store and envelope.
    #[test]
    fn concurrent_copies_are_admitted_once() {
        let f = fx();
        for _ in 0..20 {
            concurrent_round(&f);
        }
    }

    fn concurrent_round(f: &Fx) {
        let store = f.store(16);
        let env = f.signed("2026-10-03T12:00:00.000Z", None);
        let copies: Vec<AuthorizedMessage> = (0..16).map(|_| f.authorized(&env)).collect();
        let (tx, rx) = mpsc::channel();
        let barrier = Arc::new(std::sync::Barrier::new(copies.len()));
        let handles: Vec<_> = copies
            .into_iter()
            .map(|msg| {
                let (store, tx, barrier) = (store.clone(), tx.clone(), barrier.clone());
                thread::spawn(move || {
                    barrier.wait();
                    match store.admit(&msg).unwrap() {
                        Admission::Admitted(r) => {
                            tx.send(r).unwrap();
                            None
                        }
                        Admission::Duplicate { receipt_allowed } => Some(receipt_allowed),
                    }
                })
            })
            .collect();
        drop(tx);
        // The first admitted copy's hand-off fails; the second's succeeds.
        let first = rx.recv().unwrap();
        assert!(store.contains(first.key()));
        first.not_handed_off();
        let second = rx.recv().unwrap();
        second.handed_off();
        assert!(rx.recv().is_err(), "a third copy was admitted");
        let outcomes: Vec<Option<bool>> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let duplicates: Vec<bool> = outcomes.iter().filter_map(|o| *o).collect();
        assert_eq!(duplicates.len(), 14);
        assert_eq!(duplicates.iter().filter(|r| **r).count(), 1);
        assert_eq!(store.len(), 1);
    }

    /// [SEC-RPL-023], [SEC-RPL-024]: kept until the hand-off deadline, evicted at it.
    #[test]
    fn entries_are_kept_until_the_deadline_and_evicted_at_it() {
        let f = fx();
        let store = f.store(16);
        let no_ttl = f.signed("2026-10-03T12:00:00.000Z", None);
        let short = f.signed("2026-10-03T12:00:00.000Z", Some(60_000));
        admitted(store.admit(&f.authorized(&no_ttl))).handed_off();
        admitted(store.admit(&f.authorized(&short))).handed_off();
        f.clock.set(ts("2026-10-03T12:00:59.999999999Z"));
        store.evict_expired();
        assert_eq!(store.len(), 2);
        f.clock.set(ts("2026-10-03T12:01:00Z")); // the expiry instant of `short`
        store.evict_expired();
        assert_eq!(store.len(), 1);
        f.clock.set(ts("2026-10-03T12:04:59.999999999Z"));
        assert!(duplicate(store.admit(&f.authorized(&no_ttl))));
        f.clock.set(ts("2026-10-03T12:05:00Z"));
        store.evict_expired();
        assert!(store.is_empty());
        // A copy at the deadline is refused at step 3 either way.
        let late = authenticate(
            receive_envelope(no_ttl.octets(), &EnvelopeLimits::default(), &f.clock.now()).unwrap(),
            &f.keys,
        )
        .unwrap();
        assert_eq!(
            check_replay_window(&late, &f.clock.now())
                .unwrap_err()
                .error,
            ErrorCode::OutsideReplayWindow
        );
    }

    #[test]
    fn an_in_flight_entry_is_not_evicted() {
        let f = fx();
        let store = f.store(16);
        let env = f.signed("2026-10-03T12:00:00.000Z", None);
        let r = admitted(store.admit(&f.authorized(&env)));
        f.clock.set(ts("2026-10-03T12:10:00Z"));
        store.evict_expired();
        assert_eq!(store.len(), 1);
        r.handed_off();
        store.evict_expired();
        assert!(store.is_empty());
    }

    /// The bound: a full store refuses new copies with `failed` / `internal-error`, evicts
    /// nothing early, and takes copies again once entries reach their deadline.
    #[test]
    fn a_full_store_refuses_without_evicting_live_entries() {
        let f = fx();
        let store = f.store(2);
        assert_eq!(store.capacity(), 2);
        let a = f.signed("2026-10-03T12:00:00.000Z", Some(60_000));
        let b = f.signed("2026-10-03T12:00:00.500Z", None);
        let c = f.signed("2026-10-03T12:00:01.000Z", None);
        admitted(store.admit(&f.authorized(&a))).handed_off();
        let in_flight = admitted(store.admit(&f.authorized(&b)));
        let rej = store.admit(&f.authorized(&c)).unwrap_err();
        assert_eq!(
            (rej.state, rej.error),
            (DeliveryState::Failed, ErrorCode::InternalError)
        );
        assert_eq!(store.len(), 2);
        // Copies of held envelopes are still recognized while full.
        assert!(duplicate(store.admit(&f.authorized(&a))));
        assert!(store.try_admit(&f.authorized(&b)).unwrap().is_none());
        // A refused admission added nothing, and the next one succeeds once `a` reaches its
        // deadline.
        f.clock.set(ts("2026-10-03T12:01:00Z"));
        admitted(store.admit(&f.authorized(&c))).handed_off();
        in_flight.handed_off();
        assert_eq!(store.len(), 2);
        assert_eq!(
            DuplicateStore::new(f.clock.clone()).capacity(),
            DEFAULT_CAPACITY
        );
    }

    /// Restart ([SEC-RPL-025], C5 §8): the store is not persisted. A new process starts with
    /// an empty store and hands off again a copy still inside the window: the accepted
    /// residual. Once the window has passed, step 3 refuses the copy, so the residual ends
    /// at the hand-off deadline.
    #[test]
    fn restart_starts_empty_and_only_the_window_bounds_the_residual() {
        let f = fx();
        let env = f.signed("2026-10-03T12:00:00.000Z", None);
        {
            let before = f.store(16);
            admitted(before.admit(&f.authorized(&env))).handed_off();
            assert!(duplicate(before.admit(&f.authorized(&env))));
        } // the process ends; nothing of the store survives it
        f.clock.set(ts("2026-10-03T12:02:00Z"));
        let after = f.store(16);
        assert!(after.is_empty());
        let copy = f.arrive(&env);
        assert!(check_replay_window(&copy, &f.clock.now()).is_ok());
        admitted(after.admit(&AuthorizedMessage::for_tests(copy))).handed_off(); // handed off a second time
        assert!(duplicate(after.admit(&f.authorized(&env))));
        let later = f.store(16);
        f.clock.set(ts("2026-10-03T12:05:00Z"));
        let copy = authenticate(
            receive_envelope(env.octets(), &EnvelopeLimits::default(), &f.clock.now()).unwrap(),
            &f.keys,
        )
        .unwrap();
        assert_eq!(
            check_replay_window(&copy, &f.clock.now())
                .unwrap_err()
                .error,
            ErrorCode::OutsideReplayWindow
        );
        assert!(later.is_empty());
    }
}
