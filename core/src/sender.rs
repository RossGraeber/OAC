// SPDX-License-Identifier: Apache-2.0

//! The sending side of a delivery (#55, F6): the send decision of
//! `spec/session-channels.md` §8.3.3 and the delivery-state machine a sending implementation
//! keeps for each envelope (§8.1, §8.4).
//!
//! # The send decision
//!
//! [`check_send`] runs the six refusal steps of §8.3.3 in order and reports the first that
//! applies ([SC-RCP-090]): attribution, presence (with discovery authorization folded in,
//! [SC-DLV-075], [SC-DLV-076]), the agreed version, `active_inbound`, part types and size.
//! A refusal creates no envelope, so it is a request error with a `request`-scoped code
//! ([SC-RCP-075]), never a delivery state (§7.2.1).
//!
//! # The envelope's state
//!
//! An [`EnvelopeTracker`] holds every state the sending implementation has for one envelope:
//! its own (`accepted-by-adapter` for each copy passed to a transport, or the failure that
//! kept a copy from one) and the receiver's, from receipts. It reports the combined state of
//! [SC-RCP-085] and the retry and retransmission rules of §8.4.2.
//!
//! What it can report is bounded by what was observed ([SC-RCP-003]):
//!
//! - a receiver-observed state enters only through an [`ObservedReceipt`], which only the
//!   receiver pipeline of this implementation ([`crate::receiver`]) and the authenticated
//!   receipt check ([`crate::receipt_auth`]) can make ([SC-RCP-040]);
//! - after a copy was passed to a transport, the sender's own `unreachable` or `failed`
//!   never decides the state ([SC-RCP-007]);
//! - once the hand-off deadline has passed with nothing better known, the state is
//!   `unknown`, not `accepted-by-adapter` ([SC-RCP-010]).
//!
//! No state means that a model read, processed or acted on a message ([SC-RCP-005]); the
//! strongest is `handed-to-harness`, the end of an input call (§8.1.3).

use crate::capabilities::{Agreed, Implemented, SessionCapabilities};
use crate::delivery::{DeliveryState, ErrorCode, Observer};
use crate::envelope::Envelope;
use crate::ids::{SessionId, Timestamp, Token, Version};
use crate::receipt::DeliveryReceipt;
use crate::replay::{HandOffDeadline, REPLAY_WINDOW_MS};

/// The send decision of §8.3.3 for a request to `to`.
///
/// - `requester`: the session the request's attachment is bound to; `None` when the
///   attachment is not bound to exactly one session, or is suspended under [SC-ID-154].
/// - `presence`: the presence step for (`requester`, `to`), which also applies discovery
///   authorization: [`crate::registry::PresenceRegistry::send_presence`], or any function
///   with its contract. It is not called for a request refused at step 1.
/// - `implemented`: the implemented list (§6.10).
/// - `part_types`: the `type` of each content part, in order.
/// - `envelope_octets`: the size of the envelope the sender would build under the agreed
///   revision, called only once steps 1 to 5 pass.
///
/// On success, the agreed version: the envelope's `version` is its `revision`
/// ([SC-ID-087]).
///
/// # Errors
///
/// The code of the earliest step that applies ([SC-RCP-079], [SC-RCP-090]): `unauthorized`,
/// `unknown-destination`, `destination-unavailable`, `unsupported-version`,
/// `unsupported-capability`, `unsupported-content-type` or `envelope-too-large`.
pub fn check_send<'d>(
    requester: Option<&SessionId>,
    to: &SessionId,
    presence: impl FnOnce(&SessionId, &SessionId) -> Result<&'d SessionCapabilities, ErrorCode>,
    implemented: &[Implemented],
    part_types: &[&str],
    envelope_octets: impl FnOnce(Version) -> u64,
) -> Result<Agreed, ErrorCode> {
    // Step 1: attribution ([SC-ID-161], [SC-ID-154]).
    let requester = requester.ok_or(ErrorCode::Unauthorized)?;
    // Step 2: presence ([SC-DLV-071] to [SC-DLV-076]); also [SC-ID-086], since a declaration
    // is held exactly for an `online` session ([SC-DLV-070]).
    let declaration = presence(requester, to)?;
    // Step 3: an agreed version ([SC-ID-084]).
    let agreed = declaration
        .agree(implemented)
        .ok_or(ErrorCode::UnsupportedVersion)?;
    // Step 4: `active_inbound` in the agreed entry ([SC-ID-100]).
    if !agreed.entry.active_inbound() {
        return Err(ErrorCode::UnsupportedCapability);
    }
    // Step 5: advertised part types ([SC-ID-101], [SC-ENV-066]).
    if !part_types.iter().all(|t| agreed.entry.accepts_part_type(t)) {
        return Err(ErrorCode::UnsupportedContentType);
    }
    // Step 6: the size limit ([SC-ENV-005], [SC-ID-065]).
    if envelope_octets(agreed.revision) > agreed.entry.max_envelope_octets() {
        return Err(ErrorCode::EnvelopeTooLarge);
    }
    Ok(agreed)
}

/// A receiver-observed delivery state for one envelope that the sending implementation may
/// hold ([SC-RCP-040]): from this implementation's own receiver, or from an authenticated
/// receipt that [`crate::receipt_auth::accept_receipt`] accepted. Nothing else can make
/// one, so no unauthenticated receipt reaches an [`EnvelopeTracker`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedReceipt(DeliveryReceipt);

impl ObservedReceipt {
    /// Crate-private: only the two sources above build one.
    pub(crate) fn new(receipt: DeliveryReceipt) -> ObservedReceipt {
        ObservedReceipt(receipt)
    }

    /// The receipt.
    pub fn receipt(&self) -> &DeliveryReceipt {
        &self.0
    }
}

/// One state held for an envelope: a state, its observer and, when the state carries one,
/// its code (`spec/session-channels.md` §8.5, stage `combine`, `held`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldState {
    /// The state as processed ([SC-RCP-030]: `failed` for an unrecognized code).
    pub state: DeliveryState,
    /// Who observed it.
    pub observer: Observer,
}

/// The error states of §8.4.1. `duplicate` carries a code but is not one.
pub fn is_error_state(s: DeliveryState) -> bool {
    matches!(
        s,
        DeliveryState::Rejected
            | DeliveryState::Expired
            | DeliveryState::Unreachable
            | DeliveryState::Failed
    )
}

/// The combined state of an envelope ([SC-RCP-085]): the first rule that applies to the
/// states `held`, in arrival order, given how many copies were passed to a transport and
/// whether the hand-off deadline has passed on the sending implementation's clock.
pub fn combined_state(
    copies_passed: u64,
    held: &[HeldState],
    handoff_deadline_passed: bool,
) -> DeliveryState {
    let any = |f: &dyn Fn(&HeldState) -> bool| held.iter().any(f);
    // Rule 1, then rule 2: a late `duplicate` never overwrites `handed-to-harness`.
    if any(&|h| h.state == DeliveryState::HandedToHarness) {
        return DeliveryState::HandedToHarness;
    }
    if any(&|h| h.state == DeliveryState::Duplicate) {
        return DeliveryState::Duplicate;
    }
    // Rule 3: a receiver-observed `unknown`.
    if any(&|h| h.state == DeliveryState::Unknown && h.observer == Observer::Receiver) {
        return DeliveryState::Unknown;
    }
    // Rule 4: one copy passed, the error state received first. Only a receiver reports
    // one for a passed copy: the sender's own error state is not reported once a copy has
    // been passed ([SC-RCP-007]).
    if copies_passed == 1
        && let Some(h) = held
            .iter()
            .find(|h| h.observer == Observer::Receiver && is_error_state(h.state))
    {
        return h.state;
    }
    // Rule 5: no copy passed, the sender's own error state ([SC-RCP-007] keeps it out
    // once a copy has been passed).
    if copies_passed == 0
        && let Some(h) = held
            .iter()
            .find(|h| h.observer == Observer::Sender && is_error_state(h.state))
    {
        return h.state;
    }
    // Rule 6 ([SC-RCP-010]).
    if handoff_deadline_passed {
        DeliveryState::Unknown
    } else {
        DeliveryState::AcceptedByAdapter
    }
}

/// Whether the sending implementation may retry the message on its own initiative
/// (§8.4.2): with no copy passed, only for an error state ([SC-RCP-086] does not apply);
/// otherwise only after the retry deadline ([SC-RCP-086], [SC-RCP-087]), never for
/// `handed-to-harness` or `duplicate` ([SC-RCP-080], [SC-RCP-081]), and not when a receiver
/// reported `unknown` ([SC-RCP-082]).
pub fn retry_allowed(
    copies_passed: u64,
    held: &[HeldState],
    state: DeliveryState,
    retry_deadline_passed: bool,
) -> bool {
    if copies_passed == 0 {
        return is_error_state(state);
    }
    retry_deadline_passed
        && !matches!(
            state,
            DeliveryState::HandedToHarness | DeliveryState::Duplicate
        )
        && !held
            .iter()
            .any(|h| h.state == DeliveryState::Unknown && h.observer == Observer::Receiver)
}

/// The delivery-state machine a sending implementation keeps for one envelope (§8.4.1).
#[derive(Clone, Debug)]
pub struct EnvelopeTracker {
    id: Token,
    from: SessionId,
    deadline: HandOffDeadline,
    copies_passed: u64,
    held: Vec<HeldState>,
}

impl EnvelopeTracker {
    /// A tracker for `env`, an envelope this implementation built and signed, before any
    /// copy is passed to a transport.
    pub fn new(env: &Envelope) -> EnvelopeTracker {
        EnvelopeTracker {
            id: env.id().clone(),
            from: env.from().clone(),
            deadline: HandOffDeadline::of(env),
            copies_passed: 0,
            held: Vec::new(),
        }
    }

    /// A copy was passed to a transport (`PublishResult` `taken`): `accepted-by-adapter`.
    pub fn passed(&mut self) {
        self.copies_passed += 1;
        self.held.push(HeldState {
            state: DeliveryState::AcceptedByAdapter,
            observer: Observer::Sender,
        });
    }

    /// A copy could not be passed to a transport (`PublishResult` `not-taken`, or an
    /// internal error before it): `failed` with `transport-failure` or `internal-error`.
    /// The copy is not passed (§8.4.1). Returns false, recording nothing, for any other
    /// code: no other code fits a sender-observed `failed` (Table 8.3).
    pub fn not_passed(&mut self, code: ErrorCode) -> bool {
        if !code.fits_receipt(DeliveryState::Failed, Observer::Sender) {
            return false;
        }
        self.held.push(HeldState {
            state: DeliveryState::Failed,
            observer: Observer::Sender,
        });
        true
    }

    /// A receiver-observed state for this envelope. False, recording nothing, when the
    /// receipt names another envelope (`envelope_id` and `envelope_from` together, §8.1.4).
    pub fn receipt(&mut self, r: &ObservedReceipt) -> bool {
        let r = r.receipt();
        if r.envelope_id() != &self.id || r.envelope_from().as_str() != self.from.as_str() {
            return false;
        }
        self.held.push(HeldState {
            state: r.effective_state(),
            observer: r.observer(),
        });
        true
    }

    /// The number of copies passed to a transport.
    pub fn copies_passed(&self) -> u64 {
        self.copies_passed
    }

    /// The states held, in arrival order.
    pub fn held(&self) -> &[HeldState] {
        &self.held
    }

    /// Whether the hand-off deadline has passed at `now`, on this implementation's clock.
    pub fn handoff_deadline_passed(&self, now: &Timestamp) -> bool {
        self.deadline.passed_at(now)
    }

    /// Whether the retry deadline, the hand-off deadline plus the replay-window skew
    /// allowance, has passed at `now` (§8.4.2).
    pub fn retry_deadline_passed(&self, now: &Timestamp) -> bool {
        now.unix_nanos() >= self.deadline.unix_nanos() + i128::from(REPLAY_WINDOW_MS) * 1_000_000
    }

    /// The combined state at `now` ([SC-RCP-085]).
    pub fn state(&self, now: &Timestamp) -> DeliveryState {
        combined_state(
            self.copies_passed,
            &self.held,
            self.handoff_deadline_passed(now),
        )
    }

    /// Whether a retry on the implementation's own initiative is permitted at `now`
    /// (§8.4.2). A retry is a new envelope, to which the send decision applies again in
    /// full ([SEC-AUZ-018]).
    pub fn retry_allowed(&self, now: &Timestamp) -> bool {
        retry_allowed(
            self.copies_passed,
            &self.held,
            self.state(now),
            self.retry_deadline_passed(now),
        )
    }

    /// Whether retransmitting the unchanged envelope is permitted at `now`: before the
    /// hand-off deadline, for `accepted-by-adapter`, `unknown`, `unreachable` or `failed`
    /// ([SC-RCP-083]); not for `rejected` or `expired` ([SC-RCP-084]) and never for
    /// `handed-to-harness` or `duplicate`.
    pub fn retransmit_allowed(&self, now: &Timestamp) -> bool {
        !self.handoff_deadline_passed(now)
            && matches!(
                self.state(now),
                DeliveryState::AcceptedByAdapter
                    | DeliveryState::Unknown
                    | DeliveryState::Unreachable
                    | DeliveryState::Failed
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::CapabilitiesEntry;
    use crate::envelope::{EnvelopeDraft, TextPart};
    use crate::ids::EXTENSION_ID_V0;
    use crate::keys::{DeviceIdentity, DeviceKey};

    fn sid(s: &str) -> SessionId {
        SessionId::parse(s).unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        Timestamp::parse(s).unwrap()
    }

    fn envelope(ttl_ms: u64) -> Envelope {
        let id = DeviceIdentity::new(DeviceKey::generate(), Token::parse("p").unwrap());
        let draft = EnvelopeDraft::new(
            Token::parse("m1").unwrap(),
            sid("01harn7x9k2m4p6q8r0s2t4v6w"),
            sid("7gq3m8z2c5k9t1w4x6b0n2r8vd"),
            ts("2026-10-03T12:00:00Z"),
            vec![TextPart::new("hi").unwrap()],
        )
        .unwrap()
        .with_ttl_ms(ttl_ms)
        .unwrap();
        id.sign_envelope(draft)
    }

    fn receipt(env: &Envelope, state: DeliveryState, error: Option<ErrorCode>) -> ObservedReceipt {
        ObservedReceipt::new(
            DeliveryReceipt::new(
                env.id().clone(),
                env.from().clone().into(),
                state,
                Observer::Receiver,
                error,
                ts("2026-10-03T12:00:01Z"),
            )
            .unwrap(),
        )
    }

    /// The acceptance criterion: a sender distinguishes `accepted-by-adapter`,
    /// `handed-to-harness` and `unknown`, and moves to `unknown` at the hand-off deadline
    /// when nothing better is known ([SC-RCP-010]).
    #[test]
    fn accepted_handed_off_unknown() {
        let env = envelope(60_000);
        let mut t = EnvelopeTracker::new(&env);
        let before = ts("2026-10-03T12:00:30Z");
        let after = ts("2026-10-03T12:01:00Z");
        t.passed();
        assert_eq!(t.state(&before), DeliveryState::AcceptedByAdapter);
        assert_eq!(t.state(&after), DeliveryState::Unknown);
        assert!(t.retransmit_allowed(&before));
        assert!(!t.retransmit_allowed(&after));
        let mut u = t.clone();
        assert!(u.receipt(&receipt(&env, DeliveryState::Unknown, None)));
        assert_eq!(u.state(&before), DeliveryState::Unknown);
        // [SC-RCP-082]: a receiver's `unknown` forbids a retry, even after the deadline.
        assert!(!u.retry_allowed(&ts("2026-10-03T13:00:00Z")));
        assert!(t.receipt(&receipt(&env, DeliveryState::HandedToHarness, None)));
        assert!(t.receipt(&receipt(
            &env,
            DeliveryState::Duplicate,
            Some(ErrorCode::Duplicate)
        )));
        assert_eq!(t.state(&after), DeliveryState::HandedToHarness);
        assert!(!t.retry_allowed(&ts("2026-10-03T13:00:00Z")));
    }

    /// [SC-RCP-007]: once a copy was passed, the sender's own failure on a second copy does
    /// not decide the state; with none passed, it does.
    #[test]
    fn own_failure_only_without_a_passed_copy() {
        let env = envelope(60_000);
        let now = ts("2026-10-03T12:00:10Z");
        let mut t = EnvelopeTracker::new(&env);
        assert!(t.not_passed(ErrorCode::TransportFailure));
        assert_eq!(t.state(&now), DeliveryState::Failed);
        assert!(t.retry_allowed(&now));
        t.passed();
        assert!(t.not_passed(ErrorCode::TransportFailure));
        assert_eq!(t.state(&now), DeliveryState::AcceptedByAdapter);
        assert!(!t.not_passed(ErrorCode::HandoffFailed));
    }

    /// A receipt for another envelope is not recorded.
    #[test]
    fn receipt_for_another_envelope_ignored() {
        let (a, b) = (envelope(60_000), envelope(60_000));
        let mut t = EnvelopeTracker::new(&a);
        // Same id and from: the test envelopes share both, so build one that differs.
        let other = ObservedReceipt::new(
            DeliveryReceipt::new(
                Token::parse("m2").unwrap(),
                b.from().clone().into(),
                DeliveryState::HandedToHarness,
                Observer::Receiver,
                None,
                ts("2026-10-03T12:00:01Z"),
            )
            .unwrap(),
        );
        assert!(!t.receipt(&other));
        assert!(t.held().is_empty());
    }

    /// §8.3.3 order: presence before version, version before `active_inbound`, part types
    /// before size.
    #[test]
    fn send_check_order() {
        let implemented = [Implemented {
            extension: EXTENSION_ID_V0.to_owned(),
            major: 0,
            revision: Version { major: 0, minor: 1 },
        }];
        let (me, to) = (
            sid("01harn7x9k2m4p6q8r0s2t4v6w"),
            sid("7gq3m8z2c5k9t1w4x6b0n2r8vd"),
        );
        let inactive = SessionCapabilities::declare([(
            EXTENSION_ID_V0,
            CapabilitiesEntry::new(Version { major: 0, minor: 1 }, false),
        )])
        .unwrap();
        let active = SessionCapabilities::declare([(
            EXTENSION_ID_V0,
            CapabilitiesEntry::new(Version { major: 0, minor: 1 }, true),
        )])
        .unwrap();
        assert_eq!(
            check_send(
                None,
                &to,
                |_, _| Ok(&active),
                &implemented,
                &["text"],
                |_| 0
            ),
            Err(ErrorCode::Unauthorized)
        );
        assert_eq!(
            check_send(
                Some(&me),
                &to,
                |_, _| Err(ErrorCode::DestinationUnavailable),
                &implemented,
                &["x/y"],
                |_| u64::MAX
            ),
            Err(ErrorCode::DestinationUnavailable)
        );
        assert_eq!(
            check_send(
                Some(&me),
                &to,
                |_, _| Ok(&inactive),
                &implemented,
                &["text"],
                |_| 0
            ),
            Err(ErrorCode::UnsupportedCapability)
        );
        assert_eq!(
            check_send(
                Some(&me),
                &to,
                |_, _| Ok(&active),
                &implemented,
                &["com.example/x"],
                |_| { u64::MAX }
            ),
            Err(ErrorCode::UnsupportedContentType)
        );
        assert_eq!(
            check_send(
                Some(&me),
                &to,
                |_, _| Ok(&active),
                &implemented,
                &["text"],
                |_| 65_537
            ),
            Err(ErrorCode::EnvelopeTooLarge)
        );
        let agreed = check_send(
            Some(&me),
            &to,
            |_, _| Ok(&active),
            &implemented,
            &["text"],
            |_| 100,
        )
        .unwrap();
        assert_eq!(agreed.revision, Version { major: 0, minor: 1 });
        assert_eq!(
            check_send(Some(&me), &to, |_, _| Ok(&active), &[], &["text"], |_| 100),
            Err(ErrorCode::UnsupportedVersion)
        );
    }
}
