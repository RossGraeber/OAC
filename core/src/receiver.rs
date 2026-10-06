// SPDX-License-Identifier: Apache-2.0

//! The receiving side of a delivery (#55, F6): security step 5 and the delivery stage of
//! `spec/session-channels.md` §8.3.2 for one copy, the states a receiver reports (§8.1,
//! `spec/interfaces.md` Table 5.3), and when it may send a receipt (`spec/security.md` §8.4,
//! §10.3).
//!
//! # One copy, start to finish
//!
//! A receiver runs, for each copy: envelope-stage validation, security steps 1 to 4
//! ([`crate::signing::authenticate`], [`crate::replay::check_replay_window`],
//! authorization), then [`deliver`], which does the rest:
//!
//! 1. security step 5 through [`DuplicateStore::try_admit`], which never waits. A copy whose
//!    earlier twin is still being handed off comes back as [`Received::InFlight`], and the
//!    caller re-queues it rather than parking a thread on it (PR #317 review N2, item 2;
//!    [SEC-RPL-026]). This function never calls [`DuplicateStore::admit`], so it cannot
//!    re-enter it for a key whose reservation it holds (item 3);
//! 2. delivery-stage steps 1 to 3 against the addressed session ([`delivery_checks`]);
//! 3. step 4, [`HandOffDeadline::refusal_at`] on the receiver's clock, read immediately
//!    before the hand-off call ([SC-RCP-091], [SC-RCP-092]; item 1);
//! 4. step 5, the hand-off call, made at most once ([IFC-ADP-057]), and its outcome
//!    recorded by Table 5.3 ([IFC-ADP-055]);
//! 5. the reservation settled on every path: [`Reservation::handed_off`] for
//!    `handed-to-harness` and `unknown`, [`Reservation::not_handed_off`] for every refusal
//!    and every failed or refused call ([SEC-RPL-022]; item 3). A panic in the hand-off call
//!    drops the reservation, which counts as handed off: the outcome is indeterminate.
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

use crate::clock::Clock;
use crate::delivery::{DeliveryState, ErrorCode, Observer};
use crate::envelope::{ChannelMessage, Envelope};
use crate::ids::{KeyId, SessionId, Timestamp};
use crate::receipt::DeliveryReceipt;
use crate::replay::{Admission, DuplicateStore, HandOffDeadline, Reservation};
use crate::sender::ObservedReceipt;
use crate::signing::SecurityRejection;
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

/// `HandOffOutcome` of `spec/interfaces.md` §4.10: what an adapter observed at one hand-off
/// call (§5.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandOffOutcome {
    /// The input call completed successfully, as the surface defines completion
    /// ([IFC-ADP-051]). Not "seen by the model".
    Completed,
    /// The surface turned the call away as unable to take input now, without taking it
    /// ([IFC-ADP-052]).
    NotNow,
    /// The input call failed for any other reason.
    Failed,
    /// The call returned neither success nor failure ([IFC-ADP-053]): timed out, or its
    /// connection closed.
    Indeterminate,
    /// No call was made: the surface would drop or alter a provenance field
    /// ([IFC-ADP-054]).
    Refused,
}

impl HandOffOutcome {
    /// The state and code the core records for the outcome (Table 5.3; [IFC-ADP-055]).
    pub fn recorded(self) -> (DeliveryState, Option<ErrorCode>) {
        match self {
            HandOffOutcome::Completed => (DeliveryState::HandedToHarness, None),
            HandOffOutcome::NotNow => (
                DeliveryState::Unreachable,
                Some(ErrorCode::DestinationUnavailable),
            ),
            HandOffOutcome::Failed => (DeliveryState::Failed, Some(ErrorCode::HandoffFailed)),
            HandOffOutcome::Indeterminate => (DeliveryState::Unknown, None),
            HandOffOutcome::Refused => (DeliveryState::Failed, Some(ErrorCode::InternalError)),
        }
    }

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiverReport {
    /// The state.
    pub state: DeliveryState,
    /// The code, exactly when the state carries one.
    pub error: Option<ErrorCode>,
    /// For a `duplicate`: whether this copy may draw its store entry's one `duplicate`
    /// receipt ([SEC-RPL-030]). `None` for any other state.
    pub duplicate_receipt_allowed: Option<bool>,
}

impl ReceiverReport {
    fn of(state: DeliveryState, error: Option<ErrorCode>) -> ReceiverReport {
        ReceiverReport {
            state,
            error,
            duplicate_receipt_allowed: None,
        }
    }

    /// The report for a copy the security stage refused.
    pub fn from_rejection(r: &SecurityRejection) -> ReceiverReport {
        ReceiverReport::of(r.state, Some(r.error))
    }

    /// The receipt for this report about `env`, observed at `observed_at` (§8.1.4). It
    /// holds identifiers, the state, the code and the time only, nothing from `content`
    /// ([SC-RCP-031]).
    pub fn receipt(&self, env: &Envelope, observed_at: Timestamp) -> DeliveryReceipt {
        DeliveryReceipt::new(
            env.id().clone(),
            env.from().clone().into(),
            self.state,
            Observer::Receiver,
            self.error,
            observed_at,
        )
        .expect("a receiver report pairs each state with a code Table 8.3 lists for it")
    }

    /// The same receipt, for the sending implementation when it is this implementation:
    /// a state this receiver observed itself ([SC-RCP-040]).
    pub fn observed(&self, env: &Envelope, observed_at: Timestamp) -> ObservedReceipt {
        ObservedReceipt::new(self.receipt(env, observed_at))
    }
}

/// The outcome of [`deliver`] for one copy.
#[derive(Debug, PartialEq, Eq)]
pub enum Received {
    /// An earlier copy with the same duplicate key is being handed off now. Nothing was
    /// decided and nothing was added: re-queue the copy and offer it again once that
    /// hand-off has settled ([SEC-RPL-026]). Deciding `duplicate` now could report one for a
    /// copy whose twin then fails ([SC-RCP-009]).
    InFlight,
    /// The copy's outcome.
    Reported(ReceiverReport),
}

/// Security step 5 and the delivery stage for `msg`, a copy that passed security steps 1
/// to 4 (see the module documentation for the order).
///
/// - `target` looks up the addressed session once the copy is admitted.
/// - `clock` is the receiver's clock, read for the hand-off-deadline re-check right before
///   the call.
/// - `hand_off` is the adapter's hand-off call. It is called at most once, and only for a
///   copy that passed every earlier check.
///
/// # Errors
///
/// `failed` with `internal-error` when the duplicate store is full of live entries or `msg`
/// is not verified ([`DuplicateStore::try_admit`]).
pub fn deliver(
    store: &DuplicateStore,
    msg: &ChannelMessage,
    target: impl FnOnce(&SessionId) -> Option<DeliveryTarget>,
    clock: &dyn Clock,
    hand_off: impl FnOnce(&ChannelMessage) -> HandOffOutcome,
) -> Result<Received, SecurityRejection> {
    let reservation = match store.try_admit(msg)? {
        None => return Ok(Received::InFlight),
        Some(Admission::Duplicate { receipt_allowed }) => {
            return Ok(Received::Reported(ReceiverReport {
                state: DeliveryState::Duplicate,
                error: Some(ErrorCode::Duplicate),
                duplicate_receipt_allowed: Some(receipt_allowed),
            }));
        }
        Some(Admission::Admitted(r)) => r,
    };
    Ok(Received::Reported(delivery_stage(
        reservation,
        msg,
        target,
        clock,
        hand_off,
    )))
}

fn refuse(reservation: Reservation, (state, code): (DeliveryState, ErrorCode)) -> ReceiverReport {
    reservation.not_handed_off();
    ReceiverReport::of(state, Some(code))
}

fn delivery_stage(
    reservation: Reservation,
    msg: &ChannelMessage,
    target: impl FnOnce(&SessionId) -> Option<DeliveryTarget>,
    clock: &dyn Clock,
    hand_off: impl FnOnce(&ChannelMessage) -> HandOffOutcome,
) -> ReceiverReport {
    let env = msg.envelope();
    // Steps 1 to 3.
    if let Err(refusal) = delivery_checks(env, target(env.to()).as_ref()) {
        return refuse(reservation, refusal);
    }
    // Step 4, immediately before the call.
    if let Some(refusal) = HandOffDeadline::of(env).refusal_at(&clock.now()) {
        return refuse(reservation, refusal);
    }
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
        msg: ChannelMessage,
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
        let msg = authenticate(msg, &keys).unwrap();
        let store = DuplicateStore::new(clock.clone());
        Fx {
            keys,
            clock,
            store,
            msg,
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
            Received::InFlight => panic!("in flight"),
        }
    }

    /// Table 5.3: each outcome's state, and whether a later copy is a duplicate
    /// ([SEC-RPL-022]).
    #[test]
    fn outcomes_and_settlement() {
        for (outcome, state, dup_after) in [
            (
                HandOffOutcome::Completed,
                DeliveryState::HandedToHarness,
                true,
            ),
            (HandOffOutcome::Indeterminate, DeliveryState::Unknown, true),
            (HandOffOutcome::NotNow, DeliveryState::Unreachable, false),
            (HandOffOutcome::Failed, DeliveryState::Failed, false),
            (HandOffOutcome::Refused, DeliveryState::Failed, false),
        ] {
            let f = fx(60_000);
            let r = reported(deliver(
                &f.store,
                &f.msg,
                |_| target(),
                &*f.clock,
                |_| outcome,
            ));
            assert_eq!(r.state, state, "{outcome:?}");
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

    /// PR #317 review N2, item 2: a copy whose twin is being handed off comes back in
    /// flight, at once, adding nothing; it decides once the twin has settled.
    #[test]
    fn in_flight_copy_is_requeued_not_parked() {
        let f = fx(60_000);
        let r = reported(deliver(
            &f.store,
            &f.msg,
            |_| target(),
            &*f.clock,
            |m| {
                // The twin arrives while this hand-off call is running.
                let twin = deliver(&f.store, m, |_| target(), &*f.clock, |_| panic!("no call"));
                assert_eq!(twin.unwrap(), Received::InFlight);
                HandOffOutcome::Failed
            },
        ));
        assert_eq!(r.state, DeliveryState::Failed);
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
