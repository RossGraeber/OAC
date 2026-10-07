// SPDX-License-Identifier: Apache-2.0

//! The receiving side of a delivery (#55, F6): security step 5 and the delivery stage of
//! `spec/session-channels.md` §8.3.2 for one copy, the states a receiver reports (§8.1,
//! `spec/interfaces.md` Table 5.3), and when it may send a receipt (`spec/security.md` §8.4,
//! §10.3).
//!
//! # One copy, start to finish
//!
//! A receiver runs envelope-stage validation ([`crate::envelope::receive_envelope`]), then
//! [`receive`], which runs the security stage in Table 7.1 order: steps 1 and 2
//! ([`crate::signing::authenticate`] against the engine's trusted key set), step 3
//! ([`crate::replay::check_replay_window`] on the receiver's clock) and step 4
//! ([`AuthorizationEngine::authorize_delivery`]). Step 4 is the only source of an
//! [`AuthorizedMessage`], and step 5 takes nothing else, so no copy reaches the
//! duplicate store without passing authorization ([SEC-RPL-022]). [`redeliver`], its tail
//! and the re-offer path for a re-queued copy, does the rest:
//!
//! 1. security step 5 through [`DuplicateStore::try_admit`], which never waits. A copy whose
//!    earlier twin is still being handed off comes back as [`Received::InFlight`] with the
//!    twin's key, and the caller re-queues it rather than parking a thread on it, offering it
//!    again when [`DuplicateStore::when_settled`] calls back (PR #317 review N2, item 2;
//!    [SEC-RPL-026]). This function never calls [`DuplicateStore::admit`], so it cannot
//!    re-enter it for a key whose reservation it holds (item 3);
//! 2. delivery-stage steps 1 to 3 against the addressed session ([`delivery_checks`]);
//! 3. step 4, [`HandOffDeadline::refusal_at`] on the receiver's clock, read immediately
//!    before the hand-off call ([SC-RCP-091], [SC-RCP-092]; item 1);
//! 4. step 5, the hand-off call, made at most once ([IFC-ADP-057]), and its outcome
//!    recorded by Table 5.3 ([IFC-ADP-055]);
//! 5. the reservation settled on every path: [`Reservation::handed_off`] for
//!    `handed-to-harness` and `unknown`, [`Reservation::not_handed_off`] for every refusal
//!    and every failed or refused call ([SEC-RPL-022]; item 3). A panic before the hand-off
//!    call (in the session lookup or the clock) settles it as not handed off, since nothing
//!    was, so a retransmission is not a `duplicate` ([SC-RCP-009]). A panic in the hand-off
//!    call drops the reservation, which counts as handed off: the outcome is indeterminate.
//!
//! The hand-off call's own duration is the adapter's: an adapter bounds its input call and
//! reports a call that did not return in time as `indeterminate` ([SC-RCP-006]).
//!
//! # What a receiver can claim
//!
//! The strongest state is `handed-to-harness`: the input call completed as the harness
//! surface defines completion, which on a surface that returns nothing is the end of the
//! write (§8.1.3). [`HandOffOutcome`] has no outcome stronger than `completed`, and no state
//! in [`DeliveryState`] means that a model saw, read or acted on a message, so no path here
//! can produce a receipt that asserts model visibility ([SC-RCP-004], [SC-RCP-005]).
//!
//! # No holding
//!
//! A copy is handed off now or refused now: a session not accepting input is reported
//! `destination-unavailable` at once, never held for later ([SC-DLV-007], [IFC-ADP-056]).

use crate::authorization::{AuthorizationEngine, AuthorizedMessage, HandOffRecord};
use crate::clock::Clock;
use crate::delivery::{DeliveryState, ErrorCode, Observer};
use crate::envelope::{ChannelMessage, Envelope, EnvelopeLimits, receive_envelope};
use crate::ids::{KeyId, SessionId, Timestamp};
use crate::receipt::{DeliveryReceipt, ReceiptViolation};
use crate::replay::check_replay_window;
use crate::replay::{Admission, DuplicateKey, DuplicateStore, HandOffDeadline, Reservation};
use crate::sender::ObservedReceipt;
use crate::signing::{SecurityRejection, authenticate};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// What the delivery stage needs to know about the addressed local session (§8.3.2 steps 2
/// and 3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryTarget {
    /// The first two conditions of "accepting input" (§7.1.3), judged before any call: the
    /// session is bound with a live attachment, and delivery to it is not withheld under
    /// [SC-ID-154].
    pub accepting: bool,
    /// The session's own `active_inbound` for the envelope's major version ([SC-ID-105]).
    pub active_inbound: bool,
    /// The part types, besides `text`, the session takes.
    pub content_types: Vec<String>,
    /// The largest serialized envelope the session takes, when it is smaller than the
    /// receiver-wide limit.
    pub max_envelope_octets: Option<u64>,
}

/// Steps 1 to 3 of the delivery stage (§8.3.2) for `env`, whose addressed session the
/// receiver knows as `target`. The first failing step decides ([SC-RCP-078]).
///
/// # Errors
///
/// `unreachable` with `unknown-destination` when no session with that id is bound here
/// ([SC-ID-155]); `unreachable` with `destination-unavailable` when it is not accepting
/// input ([SC-DLV-007]); `rejected` with `unsupported-capability` when it lacks a capability
/// the envelope needs ([SC-ID-105], [SC-RCP-077]).
pub fn delivery_checks(
    env: &Envelope,
    target: Option<&DeliveryTarget>,
) -> Result<(), (DeliveryState, ErrorCode)> {
    let t = target.ok_or((DeliveryState::Unreachable, ErrorCode::UnknownDestination))?;
    if !t.accepting {
        return Err((
            DeliveryState::Unreachable,
            ErrorCode::DestinationUnavailable,
        ));
    }
    let lacks = !t.active_inbound
        || !env
            .content()
            .iter()
            .all(|p| p.part_type() == "text" || t.content_types.iter().any(|c| c == p.part_type()))
        || t.max_envelope_octets
            .is_some_and(|m| env.octets().len() as u64 > m);
    if lacks {
        return Err((DeliveryState::Rejected, ErrorCode::UnsupportedCapability));
    }
    Ok(())
}

/// `HandOffOutcome` of `spec/interfaces.md` §4.10, defined once, with the adapter contract
/// ([`crate::adapter`]); re-exported here, where the receiver records it (Table 5.3).
pub use crate::adapter::HandOffOutcome;

/// The receiver's side of an outcome: what it means for the duplicate store.
impl HandOffOutcome {
    /// Whether the harness may hold the content: the entry stays in the duplicate store
    /// ([SEC-RPL-022]).
    pub fn may_be_handed_off(self) -> bool {
        matches!(
            self,
            HandOffOutcome::Completed | HandOffOutcome::Indeterminate
        )
    }
}

/// What a receiver reports for one copy: a receiver-observed state and, when the state
/// carries one, its code.
///
/// It is sealed: only this crate's receiver pipeline ([`receive`], [`redeliver`]) makes one,
/// and its fields are private, so no code outside the crate can make a report for a copy
/// no receiver saw, nor turn one into an [`ObservedReceipt`] ([SC-RCP-003], [SC-RCP-040]):
///
/// ```compile_fail
/// use oac_core::delivery::DeliveryState;
/// use oac_core::receiver::ReceiverReport;
/// // E0451: the fields are private.
/// let forged = ReceiverReport {
///     state: DeliveryState::HandedToHarness,
///     error: None,
///     duplicate_receipt_allowed: None,
/// };
/// ```
///
/// ```compile_fail
/// use oac_core::delivery::DeliveryState;
/// use oac_core::receiver::ReceiverReport;
/// // E0624: the constructor is crate-private.
/// let forged = ReceiverReport::of(DeliveryState::HandedToHarness, None);
/// ```
///
/// ```compile_fail
/// # fn f(r: &oac_core::receiver::ReceiverReport, e: &oac_core::envelope::Envelope,
/// #      t: oac_core::ids::Timestamp) {
/// // E0624: only this crate turns a report into an observed receipt.
/// let observed = r.observed(e, t);
/// # }
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiverReport {
    state: DeliveryState,
    error: Option<ErrorCode>,
    duplicate_receipt_allowed: Option<bool>,
}

impl ReceiverReport {
    /// A report of `state` with `error`. A pair that Table 8.3 does not allow for a
    /// receiver ([SC-RCP-002], [SC-RCP-025] to [SC-RCP-028]) cannot be made: it becomes
    /// `failed` with `internal-error`, an error unrelated to the envelope, so every report
    /// has a receipt. No caller in this crate passes such a pair.
    pub(crate) fn of(state: DeliveryState, error: Option<ErrorCode>) -> ReceiverReport {
        let valid = state.allowed_for(Observer::Receiver)
            && match error {
                None => !state.carries_error(),
                Some(c) => c.fits_receipt(state, Observer::Receiver),
            };
        debug_assert!(valid, "receiver report {state} {error:?}");
        let (state, error) = if valid {
            (state, error)
        } else {
            (DeliveryState::Failed, Some(ErrorCode::InternalError))
        };
        ReceiverReport {
            state,
            error,
            duplicate_receipt_allowed: None,
        }
    }

    /// The report for a copy the security stage refused.
    pub(crate) fn from_rejection(r: &SecurityRejection) -> ReceiverReport {
        ReceiverReport::of(r.state, Some(r.error))
    }

    /// The state.
    pub fn state(&self) -> DeliveryState {
        self.state
    }

    /// The code, exactly when the state carries one.
    pub fn error(&self) -> Option<ErrorCode> {
        self.error
    }

    /// For a `duplicate`: whether this copy may draw its store entry's one `duplicate`
    /// receipt ([SEC-RPL-030]). `None` for any other state.
    pub fn duplicate_receipt_allowed(&self) -> Option<bool> {
        self.duplicate_receipt_allowed
    }

    /// The receipt for this report about `env`, observed at `observed_at` (§8.1.4). It
    /// holds identifiers, the state, the code and the time only, nothing from `content`
    /// ([SC-RCP-031]).
    ///
    /// # Errors
    ///
    /// None in practice: a report is only ever made of a pair a receipt can carry. The
    /// result is passed on rather than unwrapped, so no report can panic here.
    pub fn receipt(
        &self,
        env: &Envelope,
        observed_at: Timestamp,
    ) -> Result<DeliveryReceipt, ReceiptViolation> {
        DeliveryReceipt::new(
            env.id().clone(),
            env.from().clone().into(),
            self.state,
            Observer::Receiver,
            self.error,
            observed_at,
        )
    }

    /// The same receipt, for the sending implementation when it is this implementation:
    /// a state this receiver observed itself ([SC-RCP-040]). Crate-private, so only the
    /// receiver pipeline's own reports reach an [`crate::sender::EnvelopeTracker`].
    ///
    /// # Errors
    ///
    /// As [`ReceiverReport::receipt`].
    #[allow(dead_code)] // The same-implementation sender path (#313) calls it.
    pub(crate) fn observed(
        &self,
        env: &Envelope,
        observed_at: Timestamp,
    ) -> Result<ObservedReceipt, ReceiptViolation> {
        self.receipt(env, observed_at).map(ObservedReceipt::new)
    }
}

/// The outcome of security step 5 and the delivery stage for one copy.
#[derive(Debug, PartialEq, Eq)]
pub enum Received {
    /// An earlier copy with the same duplicate key is being handed off now. Nothing was
    /// decided and nothing was added: re-queue the copy and offer it again once that
    /// hand-off has settled ([SEC-RPL-026]). Deciding `duplicate` now could report one for a
    /// copy whose twin then fails ([SC-RCP-009]).
    ///
    /// The key is the earlier copy's: [`DuplicateStore::when_settled`] with it calls back
    /// once that copy settles, so the caller re-offers the copy then, with no timer, through
    /// [`redeliver`], the only re-offer path: it records a hand-off in the engine as
    /// [`receive`] does ([SEC-AUZ-016]).
    InFlight(DuplicateKey),
    /// The copy's outcome.
    Reported(ReceiverReport),
}

/// Security step 5 and the delivery stage for `msg`, a copy that passed security steps 1
/// to 4: the [`AuthorizedMessage`] step 4 built (see the module documentation for the
/// order).
///
/// - `target` looks up the addressed session once the copy is admitted.
/// - `clock` is the receiver's clock, read for the hand-off-deadline re-check right before
///   the call.
/// - `hand_off` is the adapter's hand-off call. It is called at most once, and only for a
///   copy that passed every earlier check.
///
/// Crate-private: it has no engine, so it cannot record a hand-off ([SEC-AUZ-016]). Code
/// outside the crate offers a copy through [`receive`] and re-offers it through
/// [`redeliver`], which both do.
///
/// # Errors
///
/// `failed` with `internal-error` when the duplicate store is full of live entries or `msg`
/// is not verified ([`DuplicateStore::try_admit`]).
pub(crate) fn deliver(
    store: &DuplicateStore,
    msg: &AuthorizedMessage,
    target: impl FnOnce(&SessionId) -> Option<DeliveryTarget>,
    clock: &dyn Clock,
    hand_off: impl FnOnce(&ChannelMessage) -> HandOffOutcome,
) -> Result<Received, SecurityRejection> {
    let reservation = match store.try_admit(msg)? {
        None => {
            // `try_admit` succeeded, so the message is verified and has a key.
            return Ok(DuplicateKey::of(msg.message()).map_or_else(
                || {
                    Received::Reported(ReceiverReport::of(
                        DeliveryState::Failed,
                        Some(ErrorCode::InternalError),
                    ))
                },
                Received::InFlight,
            ));
        }
        Some(Admission::Duplicate { receipt_allowed }) => {
            return Ok(Received::Reported(ReceiverReport {
                duplicate_receipt_allowed: Some(receipt_allowed),
                ..ReceiverReport::of(DeliveryState::Duplicate, Some(ErrorCode::Duplicate))
            }));
        }
        Some(Admission::Admitted(r)) => r,
    };
    Ok(Received::Reported(delivery_stage(
        reservation,
        msg.message(),
        target,
        clock,
        hand_off,
    )))
}

/// What [`receive`] did with one copy.
#[derive(Debug, PartialEq, Eq)]
pub struct ReceiveOutcome {
    /// The copy's outcome, or [`Received::InFlight`] to re-queue it.
    pub received: Received,
    /// With [`Received::InFlight`]: the copy, past steps 1 to 4, for the caller to offer to
    /// [`redeliver`] once [`DuplicateStore::when_settled`] calls back. `None` otherwise.
    pub requeue: Option<AuthorizedMessage>,
    /// The key id the copy verified under at steps 1 and 2; `None` when it failed one of
    /// them, and then no receipt may be sent for it ([`may_send_receipt`], [SEC-RCT-005]).
    pub verified_by: Option<KeyId>,
    /// Whether step 4 refused the copy with a finding: its `from` is bound to another key
    /// ([SEC-PRS-004]). The engine has logged it.
    pub finding: bool,
}

/// The security stage, in Table 7.1 order, and then security step 5 and the delivery stage
/// ([`redeliver`]), for `msg`, a copy that passed envelope-stage validation.
///
/// - `engine`: the authorization engine. Its trusted key set serves steps 1 and 2, and
///   [`AuthorizationEngine::authorize_delivery`] is step 4. A copy handed off with
///   `handed-to-harness` or `unknown` is recorded in it ([SEC-AUZ-016]).
/// - `store`: the duplicate store (step 5).
/// - `clock`: the receiver clock step 3 and the hand-off-deadline re-check read; it must be
///   the clock the engine and the store were built with, so every check of one copy reads
///   one clock.
/// - `target`: looks up the addressed session once the copy is admitted.
/// - `hand_off`: the adapter's hand-off call, made at most once, and only for a copy that
///   passed every earlier check.
pub fn receive(
    msg: ChannelMessage,
    engine: &mut AuthorizationEngine,
    store: &DuplicateStore,
    clock: &dyn Clock,
    target: impl FnOnce(&SessionId) -> Option<DeliveryTarget>,
    hand_off: impl FnOnce(&ChannelMessage) -> HandOffOutcome,
) -> ReceiveOutcome {
    let reported = |r: ReceiverReport, verified_by, finding| ReceiveOutcome {
        received: Received::Reported(r),
        requeue: None,
        verified_by,
        finding,
    };
    // Steps 1 and 2.
    let msg = match authenticate(msg, engine.trusted_keys()) {
        Ok(m) => m,
        Err(r) => return reported(ReceiverReport::from_rejection(&r), None, false),
    };
    let verified_by = msg.verified_by().map(|p| p.key_id().clone());
    // Step 3.
    if let Err(r) = check_replay_window(&msg, &clock.now()) {
        return reported(ReceiverReport::from_rejection(&r), verified_by, false);
    }
    // Step 4.
    let authorized = match engine.authorize_delivery(msg) {
        Ok(a) => a,
        Err(r) => {
            let finding = r.finding.is_some();
            return reported(
                ReceiverReport::of(r.state, Some(r.error)),
                verified_by,
                finding,
            );
        }
    };
    redeliver(authorized, engine, store, clock, target, hand_off)
}

/// Security step 5 and the delivery stage for `msg`, a copy past steps 1 to 4: the tail of
/// [`receive`], and the way to re-offer a copy that came back as [`Received::InFlight`]
/// (from [`ReceiveOutcome::requeue`], once [`DuplicateStore::when_settled`] calls back). A
/// copy handed off with `handed-to-harness` or `unknown` is recorded in `engine`
/// ([SEC-AUZ-016]), whichever of the two paths handed it off. The arguments are as for
/// [`receive`].
pub fn redeliver(
    msg: AuthorizedMessage,
    engine: &mut AuthorizationEngine,
    store: &DuplicateStore,
    clock: &dyn Clock,
    target: impl FnOnce(&SessionId) -> Option<DeliveryTarget>,
    hand_off: impl FnOnce(&ChannelMessage) -> HandOffOutcome,
) -> ReceiveOutcome {
    let verified_by = msg.message().verified_by().map(|p| p.key_id().clone());
    let received = match deliver(store, &msg, target, clock, hand_off) {
        Ok(r) => r,
        Err(r) => Received::Reported(ReceiverReport::from_rejection(&r)),
    };
    if let Received::Reported(r) = &received
        && matches!(
            r.state,
            DeliveryState::HandedToHarness | DeliveryState::Unknown
        )
    {
        engine.record_handoff(HandOffRecord::of(msg.message().envelope()), r.state);
    }
    let requeue = matches!(received, Received::InFlight(_)).then_some(msg);
    ReceiveOutcome {
        received,
        requeue,
        verified_by,
        finding: false,
    }
}

/// Envelope-stage validation at the receiver clock's time, then [`receive`]: one copy, as
/// its octets arrived, through every check a receiver makes. An envelope-stage refusal is
/// reported with its §8.3.2 code and no `verified_by`, so no receipt may be sent for it
/// ([SC-RCP-041]).
pub fn receive_octets(
    octets: &[u8],
    limits: &EnvelopeLimits,
    engine: &mut AuthorizationEngine,
    store: &DuplicateStore,
    clock: &dyn Clock,
    target: impl FnOnce(&SessionId) -> Option<DeliveryTarget>,
    hand_off: impl FnOnce(&ChannelMessage) -> HandOffOutcome,
) -> ReceiveOutcome {
    match receive_envelope(octets, limits, &clock.now()) {
        Ok(msg) => receive(msg, engine, store, clock, target, hand_off),
        Err(r) => ReceiveOutcome {
            received: Received::Reported(ReceiverReport::of(r.state, Some(r.error))),
            requeue: None,
            verified_by: None,
            finding: false,
        },
    }
}

/// A reservation whose copy has not reached its hand-off call. Dropped, by a refusal or by
/// a panic in the session lookup or the clock, it settles as not handed off, since nothing
/// was: a retransmission is then not a `duplicate` ([SC-RCP-009], [SEC-RPL-022]). Only once
/// the hand-off call is about to start does the bare [`Reservation`] take over, whose own
/// drop counts as indeterminate.
struct BeforeHandOff(Option<Reservation>);

impl BeforeHandOff {
    fn refuse(mut self, (state, code): (DeliveryState, ErrorCode)) -> ReceiverReport {
        if let Some(r) = self.0.take() {
            r.not_handed_off();
        }
        ReceiverReport::of(state, Some(code))
    }

    fn into_reservation(mut self) -> Option<Reservation> {
        self.0.take()
    }
}

impl Drop for BeforeHandOff {
    fn drop(&mut self) {
        if let Some(r) = self.0.take() {
            r.not_handed_off();
        }
    }
}

fn delivery_stage(
    reservation: Reservation,
    msg: &ChannelMessage,
    target: impl FnOnce(&SessionId) -> Option<DeliveryTarget>,
    clock: &dyn Clock,
    hand_off: impl FnOnce(&ChannelMessage) -> HandOffOutcome,
) -> ReceiverReport {
    let env = msg.envelope();
    let pending = BeforeHandOff(Some(reservation));
    // Steps 1 to 3.
    if let Err(refusal) = delivery_checks(env, target(env.to()).as_ref()) {
        return pending.refuse(refusal);
    }
    // Step 4, immediately before the call.
    if let Some(refusal) = HandOffDeadline::of(env).refusal_at(&clock.now()) {
        return pending.refuse(refusal);
    }
    let Some(reservation) = pending.into_reservation() else {
        return ReceiverReport::of(DeliveryState::Failed, Some(ErrorCode::InternalError));
    };
    // Step 5.
    let outcome = hand_off(msg);
    if outcome.may_be_handed_off() {
        reservation.handed_off();
    } else {
        reservation.not_handed_off();
    }
    let (state, error) = outcome.recorded();
    ReceiverReport::of(state, error)
}

/// Why a receiver sends no receipt for a copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptRefusal {
    /// The copy failed envelope-stage validation or security step 1 or 2: its `from` is
    /// only a claim ([SC-RCP-041], [SEC-RCT-005]).
    NotVerified,
    /// A `duplicate` whose store entry already drew its one `duplicate` receipt
    /// ([SEC-RPL-030]).
    DuplicateReceiptSpent,
    /// The sending device has used up its receipt allowance ([SEC-RPL-031]).
    RateLimited,
}

/// How many receipts one sending device may draw at once, by default.
pub const DEFAULT_RECEIPT_BURST: u32 = 32;

/// How often one receipt of allowance returns to a sending device, by default.
pub const DEFAULT_RECEIPT_INTERVAL: Duration = Duration::from_millis(100);

/// How many sending devices the limiter tracks at once, by default.
pub const DEFAULT_RECEIPT_DEVICES: usize = 4096;

#[derive(Clone, Copy, Debug)]
struct Bucket {
    tokens: u32,
    last: Instant,
}

/// The receipt rate limit of [SEC-RPL-031]: a token bucket per sending device, keyed by
/// the key id the copy verified under. It keys by nothing else, so the decision does not
/// depend on the envelope's `to` ([SEC-RCT-004]).
///
/// Its memory is bounded: when it tracks [`ReceiptLimiter::max_devices`] devices, a full
/// bucket (a device idle long enough to have its whole allowance back) is dropped to make
/// room, and while none is full a new device gets no receipt. Receipts are optional
/// ([SC-RCP-042]), so refusing one is always conformant.
#[derive(Clone, Debug)]
pub struct ReceiptLimiter {
    burst: u32,
    interval: Duration,
    max_devices: usize,
    buckets: HashMap<KeyId, Bucket>,
}

impl Default for ReceiptLimiter {
    fn default() -> ReceiptLimiter {
        ReceiptLimiter::new(
            DEFAULT_RECEIPT_BURST,
            DEFAULT_RECEIPT_INTERVAL,
            DEFAULT_RECEIPT_DEVICES,
        )
    }
}

impl ReceiptLimiter {
    /// A limiter that lets each device draw `burst` receipts at once and gives one back
    /// every `interval`, for at most `max_devices` devices. A zero `burst`, `interval` or
    /// `max_devices` is raised to one (nanosecond, for the interval).
    pub fn new(burst: u32, interval: Duration, max_devices: usize) -> ReceiptLimiter {
        ReceiptLimiter {
            burst: burst.max(1),
            interval: interval.max(Duration::from_nanos(1)),
            max_devices: max_devices.max(1),
            buckets: HashMap::new(),
        }
    }

    /// The most devices tracked at once.
    pub fn max_devices(&self) -> usize {
        self.max_devices
    }

    fn refill(&self, b: &mut Bucket, now: Instant) {
        let elapsed = now.saturating_duration_since(b.last);
        let back = elapsed.as_nanos() / self.interval.as_nanos();
        if back == 0 {
            return;
        }
        let back = u32::try_from(back).unwrap_or(u32::MAX);
        b.tokens = b.tokens.saturating_add(back).min(self.burst);
        b.last = if b.tokens == self.burst {
            now
        } else {
            b.last + self.interval * back
        };
    }

    /// Takes one receipt of `device`'s allowance at `now`; false when there is none.
    pub fn allow(&mut self, device: &KeyId, now: Instant) -> bool {
        if !self.buckets.contains_key(device) && self.buckets.len() >= self.max_devices {
            let full: Vec<KeyId> = self
                .buckets
                .iter()
                .filter(|(_, b)| {
                    let mut b = **b;
                    self.refill(&mut b, now);
                    b.tokens == self.burst
                })
                .map(|(k, _)| k.clone())
                .collect();
            for k in full {
                self.buckets.remove(&k);
            }
            if self.buckets.len() >= self.max_devices {
                return false;
            }
        }
        let burst = self.burst;
        let mut b = *self.buckets.entry(device.clone()).or_insert(Bucket {
            tokens: burst,
            last: now,
        });
        self.refill(&mut b, now);
        let ok = b.tokens > 0;
        if ok {
            b.tokens -= 1;
        }
        self.buckets.insert(device.clone(), b);
        ok
    }
}

/// Whether a receiver may send a receipt for a copy (§8.1.5; `spec/security.md` §8.4, §10.3).
///
/// - `verified_by`: the key id the copy verified under at security steps 1 and 2, `None`
///   when it failed envelope-stage validation or one of those steps.
/// - `report`: what the receiver reports for the copy.
///
/// Sending a receipt at all is optional ([SC-RCP-042]); this says only when one must not be
/// sent. Nothing here reads the envelope's `to` or the state of the session it names, so
/// for a copy refused before security step 4 the decision cannot reveal whether that
/// session exists ([SEC-RCT-004]).
///
/// # Errors
///
/// The reason no receipt may be sent. Only a permitted receipt takes allowance from the
/// limiter.
pub fn may_send_receipt(
    verified_by: Option<&KeyId>,
    report: &ReceiverReport,
    limiter: &mut ReceiptLimiter,
    now: Instant,
) -> Result<(), ReceiptRefusal> {
    let device = verified_by.ok_or(ReceiptRefusal::NotVerified)?;
    if report.duplicate_receipt_allowed == Some(false) {
        return Err(ReceiptRefusal::DuplicateReceiptSpent);
    }
    if !limiter.allow(device, now) {
        return Err(ReceiptRefusal::RateLimited);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use crate::envelope::{EnvelopeDraft, EnvelopeLimits, TextPart, receive_envelope};
    use crate::ids::Token;
    use crate::keys::{DeviceIdentity, DeviceKey};
    use crate::signing::authenticate;
    use crate::trust::TrustedKeySet;
    use std::sync::Arc;

    fn ts(s: &str) -> Timestamp {
        Timestamp::parse(s).unwrap()
    }

    struct Fx {
        keys: TrustedKeySet,
        clock: Arc<ManualClock>,
        store: DuplicateStore,
        msg: AuthorizedMessage,
        /// The same copy before steps 1 to 4, for [`receive`].
        raw: ChannelMessage,
        id: DeviceIdentity,
    }

    fn fx(ttl_ms: u64) -> Fx {
        let id = DeviceIdentity::new(DeviceKey::generate(), Token::parse("p").unwrap());
        let created = ts("2026-10-03T12:00:00Z");
        let draft = EnvelopeDraft::new(
            Token::parse("m1").unwrap(),
            SessionId::parse("01harn7x9k2m4p6q8r0s2t4v6w").unwrap(),
            SessionId::parse("7gq3m8z2c5k9t1w4x6b0n2r8vd").unwrap(),
            created.clone(),
            vec![TextPart::new("hi").unwrap()],
        )
        .unwrap()
        .with_ttl_ms(ttl_ms)
        .unwrap();
        let env = id.sign_envelope(draft);
        let keys = TrustedKeySet::new(&id);
        let clock = Arc::new(ManualClock::new(ts("2026-10-03T12:00:00.500Z")));
        let msg = receive_envelope(env.octets(), &EnvelopeLimits::default(), &clock.now()).unwrap();
        let raw = msg.clone();
        let msg = AuthorizedMessage::for_tests(authenticate(msg, &keys).unwrap());
        let store = DuplicateStore::new(clock.clone());
        Fx {
            keys,
            clock,
            store,
            msg,
            raw,
            id,
        }
    }

    fn target() -> Option<DeliveryTarget> {
        Some(DeliveryTarget {
            accepting: true,
            active_inbound: true,
            content_types: vec![],
            max_envelope_octets: None,
        })
    }

    fn reported(r: Result<Received, SecurityRejection>) -> ReceiverReport {
        match r.unwrap() {
            Received::Reported(r) => r,
            Received::InFlight(_) => panic!("in flight"),
        }
    }

    /// Table 5.3: each outcome's state and code, and whether a later copy is a duplicate
    /// ([SEC-RPL-022]).
    #[test]
    fn outcomes_and_settlement() {
        use DeliveryState as S;
        for (outcome, state, code, dup_after) in [
            (HandOffOutcome::Completed, S::HandedToHarness, None, true),
            (HandOffOutcome::Indeterminate, S::Unknown, None, true),
            (
                HandOffOutcome::NotNow,
                S::Unreachable,
                Some(ErrorCode::DestinationUnavailable),
                false,
            ),
            (
                HandOffOutcome::Failed,
                S::Failed,
                Some(ErrorCode::HandoffFailed),
                false,
            ),
            (
                HandOffOutcome::Refused,
                S::Failed,
                Some(ErrorCode::InternalError),
                false,
            ),
        ] {
            assert_eq!(outcome.recorded(), (state, code), "{outcome:?}");
            let f = fx(60_000);
            let r = reported(deliver(
                &f.store,
                &f.msg,
                |_| target(),
                &*f.clock,
                |_| outcome,
            ));
            assert_eq!((r.state(), r.error()), (state, code), "{outcome:?}");
            // Every report has a receipt, with the same state and code.
            let receipt = r.receipt(f.msg.message().envelope(), ts("2026-10-03T12:00:01Z"));
            let receipt = receipt.unwrap();
            assert_eq!(receipt.state(), state);
            let again = reported(deliver(
                &f.store,
                &f.msg,
                |_| target(),
                &*f.clock,
                |_| HandOffOutcome::Completed,
            ));
            assert_eq!(
                again.state == DeliveryState::Duplicate,
                dup_after,
                "{outcome:?}"
            );
            let _ = &f.keys;
        }
    }

    /// Delivery-stage refusals release the entry, and the hand-off call is never made.
    #[test]
    fn refusals_make_no_call() {
        let f = fx(60_000);
        let mut t = target().unwrap();
        t.accepting = false;
        let r = reported(deliver(
            &f.store,
            &f.msg,
            |_| Some(t.clone()),
            &*f.clock,
            |_| panic!("no call"),
        ));
        assert_eq!(
            (r.state, r.error),
            (
                DeliveryState::Unreachable,
                Some(ErrorCode::DestinationUnavailable)
            )
        );
        let r = reported(deliver(
            &f.store,
            &f.msg,
            |_| None,
            &*f.clock,
            |_| panic!("no call"),
        ));
        assert_eq!(r.error, Some(ErrorCode::UnknownDestination));
        t.accepting = true;
        t.active_inbound = false;
        let r = reported(deliver(
            &f.store,
            &f.msg,
            |_| Some(t.clone()),
            &*f.clock,
            |_| panic!("no call"),
        ));
        assert_eq!(r.error, Some(ErrorCode::UnsupportedCapability));
        assert!(f.store.is_empty());
    }

    /// [SC-RCP-091]: the deadline is read at hand-off, after admission, on the receiver's
    /// clock.
    #[test]
    fn deadline_rechecked_before_the_call() {
        let f = fx(1_000);
        f.clock.set(ts("2026-10-03T12:00:01Z"));
        let r = reported(deliver(
            &f.store,
            &f.msg,
            |_| target(),
            &*f.clock,
            |_| panic!("no call"),
        ));
        assert_eq!(
            (r.state, r.error),
            (DeliveryState::Expired, Some(ErrorCode::Expired))
        );
        assert!(f.store.is_empty());
    }

    /// [`receive`] runs steps 1 to 4 before the store: with no grant, step 4 refuses the copy
    /// with `unauthorized` and it adds no entry; with one, it is handed off and admitted.
    #[test]
    fn receive_runs_step_4_before_the_store() {
        use crate::authorization::{
            AuthorizationRequest, Grant, Kind, LocalSide, MemoryDecisionLog, OperatorConfirmed,
            PeerSide,
        };
        use crate::pairing::MemoryPairingStore;
        let f = fx(60_000);
        let mut e =
            AuthorizationEngine::new(&f.id, f.clock.clone(), Box::new(MemoryDecisionLog::new()));
        let from = f.raw.envelope().from().clone();
        let related = |e: &mut AuthorizationEngine| {
            e.decide(&AuthorizationRequest::AcceptPresence {
                signing_key: f.id.key_id().clone(),
                session: from.clone(),
            })
            .permits(Kind::AcceptPresence)
        };
        let out = receive(
            f.raw.clone(),
            &mut e,
            &f.store,
            &*f.clock,
            |_| target(),
            |_| panic!("no call"),
        );
        assert_eq!(
            out.received,
            Received::Reported(ReceiverReport::of(
                DeliveryState::Rejected,
                Some(ErrorCode::Unauthorized)
            ))
        );
        assert_eq!(out.verified_by.as_ref(), Some(f.id.key_id()));
        assert!(f.store.is_empty());
        // Nothing relates the sender's device yet ([SEC-AUZ-017]).
        assert!(!related(&mut e));
        let store = MemoryPairingStore::new();
        e.add_grant(
            Grant::Inbound {
                writer: PeerSide::session(f.id.key_id().clone(), from.clone()),
                target: LocalSide::Session(f.raw.envelope().to().clone()),
            },
            OperatorConfirmed::by_operator(),
            &store,
        )
        .unwrap();
        let out = receive(
            f.raw.clone(),
            &mut e,
            &f.store,
            &*f.clock,
            |_| target(),
            |_| HandOffOutcome::Completed,
        );
        assert_eq!(
            out.received,
            Received::Reported(ReceiverReport::of(DeliveryState::HandedToHarness, None))
        );
        assert_eq!(f.store.len(), 1);
        let _ = &f.keys;
    }

    /// PR #317 review N2, item 2: a copy whose twin is being handed off comes back in
    /// flight, at once, adding nothing; it decides once the twin has settled.
    #[test]
    fn in_flight_copy_is_requeued_not_parked() {
        let f = fx(60_000);
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let r = reported(deliver(
            &f.store,
            &f.msg,
            |_| target(),
            &*f.clock,
            |_| {
                // The twin arrives while this hand-off call is running.
                let twin = deliver(
                    &f.store,
                    &f.msg,
                    |_| target(),
                    &*f.clock,
                    |_| panic!("no call"),
                );
                let Received::InFlight(key) = twin.unwrap() else {
                    panic!("not in flight")
                };
                // The re-queued twin asks to hear when the earlier copy settles.
                let tx = tx.clone();
                f.store.when_settled(&key, move || tx.send(()).unwrap());
                assert!(rx.try_recv().is_err(), "called back before settling");
                HandOffOutcome::Failed
            },
        ));
        assert_eq!(r.state, DeliveryState::Failed);
        // The settle callback fired once the earlier copy's reservation settled.
        rx.try_recv().expect("settle notification");
        // The twin, re-queued, now takes over.
        let r = reported(deliver(
            &f.store,
            &f.msg,
            |_| target(),
            &*f.clock,
            |_| HandOffOutcome::Completed,
        ));
        assert_eq!(r.state, DeliveryState::HandedToHarness);
    }

    /// An engine for `f`'s device that binds `to` as an own session and holds an inbound
    /// grant from `from` to it.
    fn engine_for(f: &Fx) -> AuthorizationEngine {
        use crate::authorization::{
            Grant, LocalSide, MemoryDecisionLog, OperatorConfirmed, PeerSide,
        };
        use crate::pairing::MemoryPairingStore;
        let env = f.raw.envelope();
        let mut e =
            AuthorizationEngine::new(&f.id, f.clock.clone(), Box::new(MemoryDecisionLog::new()));
        let rec =
            f.id.register(
                env.to().clone(),
                Token::parse("harness-x").unwrap(),
                "native",
                "/w",
                ts("2026-10-03T11:00:00Z"),
            )
            .unwrap();
        assert!(e.register_session(&rec, &f.id));
        e.add_grant(
            Grant::Inbound {
                writer: PeerSide::session(f.id.key_id().clone(), env.from().clone()),
                target: LocalSide::Session(env.to().clone()),
            },
            OperatorConfirmed::by_operator(),
            &MemoryPairingStore::new(),
        )
        .unwrap();
        e
    }

    /// [SEC-AUZ-016]: `receive` records a hand-off in the engine for `handed-to-harness`
    /// and for `unknown`, and for nothing else; the receiving session may then discover the
    /// sender for the reply period.
    #[test]
    fn receive_records_the_hand_off() {
        use crate::authorization::{AuthorizationRequest, Basis, Kind, Requester};
        for (outcome, recorded) in [
            (HandOffOutcome::Completed, true),
            (HandOffOutcome::Indeterminate, true),
            (HandOffOutcome::NotNow, false),
            (HandOffOutcome::Failed, false),
        ] {
            let f = fx(60_000);
            let mut e = engine_for(&f);
            let env = f.raw.envelope().clone();
            let out = receive(
                f.raw.clone(),
                &mut e,
                &f.store,
                &*f.clock,
                |_| target(),
                |_| outcome,
            );
            assert!(matches!(out.received, Received::Reported(_)), "{outcome:?}");
            let d = e.decide(&AuthorizationRequest::Discover {
                requester: Requester::Session(env.to().clone()),
                session: env.from().clone(),
            });
            assert_eq!(d.permits(Kind::Discover), recorded, "{outcome:?}");
            if recorded {
                assert!(matches!(d.basis(), Some(Basis::HandOff(_))), "{outcome:?}");
            }
        }
    }

    /// Table 7.1 order in `receive`: a copy outside the replay window and not authorized is
    /// refused at step 3, `expired` with `outside-replay-window`, not at step 4.
    #[test]
    fn receive_runs_step_3_before_step_4() {
        let f = fx(60_000);
        let mut e = AuthorizationEngine::new(
            &f.id,
            f.clock.clone(),
            Box::new(crate::authorization::MemoryDecisionLog::new()),
        );
        f.clock.set(ts("2026-10-03T12:05:01Z"));
        let out = receive(
            f.raw.clone(),
            &mut e,
            &f.store,
            &*f.clock,
            |_| target(),
            |_| panic!("no call"),
        );
        assert_eq!(
            out.received,
            Received::Reported(ReceiverReport::of(
                DeliveryState::Expired,
                Some(ErrorCode::OutsideReplayWindow)
            ))
        );
        assert!(out.verified_by.is_some());
        assert!(f.store.is_empty());
        // And with authorization in place but the window passed, step 3 still decides.
        let mut e = engine_for(&f);
        let out = receive(
            f.raw.clone(),
            &mut e,
            &f.store,
            &*f.clock,
            |_| target(),
            |_| panic!("no call"),
        );
        assert!(matches!(
            out.received,
            Received::Reported(r) if r.error() == Some(ErrorCode::OutsideReplayWindow)
        ));
    }

    /// [SC-RCP-091]: the deadline is read after the session lookup, immediately before the
    /// call: time that passes during the lookup counts.
    #[test]
    fn deadline_read_after_the_lookup() {
        let f = fx(1_000);
        let r = reported(deliver(
            &f.store,
            &f.msg,
            |_| {
                f.clock.set(ts("2026-10-03T12:00:01Z"));
                target()
            },
            &*f.clock,
            |_| panic!("no call"),
        ));
        assert_eq!(
            (r.state, r.error),
            (DeliveryState::Expired, Some(ErrorCode::Expired))
        );
        assert!(f.store.is_empty());
    }

    /// A panic before the hand-off call (here, in the session lookup) settles the copy as
    /// not handed off: a retransmission is handed off, not reported `duplicate`
    /// ([SC-RCP-009]).
    #[test]
    fn panic_before_the_call_is_not_a_hand_off() {
        let f = fx(60_000);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            deliver(
                &f.store,
                &f.msg,
                |_| panic!("lookup failed"),
                &*f.clock,
                |_| HandOffOutcome::Completed,
            )
        }));
        assert!(panicked.is_err());
        assert!(f.store.is_empty());
        let r = reported(deliver(
            &f.store,
            &f.msg,
            |_| target(),
            &*f.clock,
            |_| HandOffOutcome::Completed,
        ));
        assert_eq!(r.state, DeliveryState::HandedToHarness);
        // A panic in the hand-off call itself is indeterminate: the entry stays.
        let g = fx(60_000);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            deliver(
                &g.store,
                &g.msg,
                |_| target(),
                &*g.clock,
                |_| panic!("call failed"),
            )
        }));
        assert!(panicked.is_err());
        let r = reported(deliver(
            &g.store,
            &g.msg,
            |_| target(),
            &*g.clock,
            |_| HandOffOutcome::Completed,
        ));
        assert_eq!(r.state, DeliveryState::Duplicate);
    }

    /// PR #321 re-review B2: a re-queued copy re-offered through [`redeliver`] after its twin
    /// failed is handed off and recorded ([SEC-AUZ-016]): the receiving session may then
    /// discover the sender.
    #[test]
    fn redelivered_copy_is_recorded() {
        use crate::authorization::{AuthorizationRequest, Basis, Kind, Requester};
        let f = fx(60_000);
        let mut e = engine_for(&f);
        let env = f.raw.envelope().clone();
        let mut twin = None;
        let first = reported(deliver(
            &f.store,
            &f.msg,
            |_| target(),
            &*f.clock,
            |_| {
                twin = Some(receive(
                    f.raw.clone(),
                    &mut e,
                    &f.store,
                    &*f.clock,
                    |_| target(),
                    |_| panic!("no call"),
                ));
                HandOffOutcome::Failed
            },
        ));
        assert_eq!(first.state, DeliveryState::Failed);
        let twin = twin.unwrap();
        assert!(matches!(twin.received, Received::InFlight(_)));
        let out = redeliver(
            twin.requeue.expect("the copy, for re-queueing"),
            &mut e,
            &f.store,
            &*f.clock,
            |_| target(),
            |_| HandOffOutcome::Completed,
        );
        assert_eq!(
            out.received,
            Received::Reported(ReceiverReport::of(DeliveryState::HandedToHarness, None))
        );
        let d = e.decide(&AuthorizationRequest::Discover {
            requester: Requester::Session(env.to().clone()),
            session: env.from().clone(),
        });
        assert!(d.permits(Kind::Discover));
        assert!(matches!(d.basis(), Some(Basis::HandOff(_))));
    }

    /// `when_settled` for a key with nothing in flight calls back at once.
    #[test]
    fn when_settled_without_flight_calls_at_once() {
        let f = fx(60_000);
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        f.store
            .when_settled(&DuplicateKey::new("k", "n"), move || tx.send(()).unwrap());
        rx.try_recv().expect("called at once");
    }

    /// `receive` hands an in-flight copy back for re-queueing.
    #[test]
    fn receive_returns_the_in_flight_copy() {
        let f = fx(60_000);
        let mut e = engine_for(&f);
        let mut second = None;
        let first = deliver(
            &f.store,
            &f.msg,
            |_| target(),
            &*f.clock,
            |_| {
                let out = receive(
                    f.raw.clone(),
                    &mut e,
                    &f.store,
                    &*f.clock,
                    |_| target(),
                    |_| panic!("no call"),
                );
                second = Some(out);
                HandOffOutcome::Completed
            },
        );
        assert!(matches!(first.unwrap(), Received::Reported(_)));
        let second = second.unwrap();
        assert!(matches!(second.received, Received::InFlight(_)));
        let again = second.requeue.expect("the copy, for re-queueing");
        // Offered again after the first settled: a duplicate of a handed-off copy.
        let r = reported(deliver(
            &f.store,
            &again,
            |_| target(),
            &*f.clock,
            |_| panic!("no call"),
        ));
        assert_eq!(r.state, DeliveryState::Duplicate);
    }

    /// The acceptance criterion: no outcome yields a state beyond `handed-to-harness`, and
    /// no delivery state is "seen by the model" ([SC-RCP-005]).
    #[test]
    fn no_state_asserts_model_visibility() {
        for s in DeliveryState::ALL {
            assert!(!s.as_str().contains("model") && !s.as_str().contains("seen"));
        }
        let names: Vec<&str> = DeliveryState::ALL.iter().map(|s| s.as_str()).collect();
        assert!(!names.contains(&"delivered") && !names.contains(&"read"));
    }

    /// [SEC-RPL-030], [SEC-RPL-031], [SEC-RCT-005].
    #[test]
    fn receipt_gate() {
        let k = KeyId::parse(&"a".repeat(64)).unwrap();
        let other = KeyId::parse(&"b".repeat(64)).unwrap();
        let t0 = Instant::now();
        let mut l = ReceiptLimiter::new(2, Duration::from_secs(1), 1);
        let ok = ReceiverReport::of(DeliveryState::HandedToHarness, None);
        assert_eq!(
            may_send_receipt(None, &ok, &mut l, t0),
            Err(ReceiptRefusal::NotVerified)
        );
        let spent = ReceiverReport {
            state: DeliveryState::Duplicate,
            error: Some(ErrorCode::Duplicate),
            duplicate_receipt_allowed: Some(false),
        };
        assert_eq!(
            may_send_receipt(Some(&k), &spent, &mut l, t0),
            Err(ReceiptRefusal::DuplicateReceiptSpent)
        );
        assert!(may_send_receipt(Some(&k), &ok, &mut l, t0).is_ok());
        assert!(may_send_receipt(Some(&k), &ok, &mut l, t0).is_ok());
        assert_eq!(
            may_send_receipt(Some(&k), &ok, &mut l, t0),
            Err(ReceiptRefusal::RateLimited)
        );
        // A second device does not fit while the first is not idle.
        assert!(!l.allow(&other, t0));
        let t1 = t0 + Duration::from_secs(1);
        assert!(l.allow(&k, t1));
        assert!(!l.allow(&k, t1));
        // Once the first is idle with its whole allowance back, it makes room.
        let t3 = t1 + Duration::from_secs(5);
        assert!(l.allow(&other, t3));
    }
}
