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

use crate::adapter::{Correlation, SendRequest};
use crate::capabilities::{Agreed, Implemented, SessionCapabilities};
use crate::delivery::{DeliveryState, ErrorCode, Observer};
use crate::envelope::{Envelope, EnvelopeDraft};
use crate::ids::{SessionId, Timestamp, Token, Version};
use crate::keys::DeviceIdentity;
use crate::receipt::DeliveryReceipt;
use crate::replay::{HandOffDeadline, REPLAY_WINDOW_MS};
use crate::reply::ReplyHeaders;

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

/// What the send stage built for a request that passed the send decision (#313): the
/// signed envelope, the agreed version, and, for a reply, whether it is correlated.
#[derive(Clone, Debug)]
pub struct PreparedSend {
    /// The envelope, built under the agreed revision and signed with the device key.
    pub envelope: Envelope,
    /// The agreed version ([SC-ID-087]).
    pub agreed: Agreed,
    /// For a reply (a request with a `requested_target`), whether `reply_to` was set
    /// ([SC-RCP-055]); `None` for a new message.
    pub correlation: Option<Correlation>,
}

/// The send stage for `request` (§8.3.3, §8.2.2, §4): the send decision of [`check_send`],
/// with its size step measured on the envelope itself, built and signed here, not on an
/// estimate (#313).
///
/// - `requester`: as for [`check_send`], the session the request's attachment is bound to.
/// - `presence`, `implemented`: as for [`check_send`].
/// - `reply`: for a request with a `requested_target`, the reply headers the hand-off
///   records give for it ([`crate::authorization::AuthorizationEngine::reply_headers`]);
///   ignored otherwise. The headers come from the implementation's own records, never from
///   the request ([SC-RCP-050] to [SC-RCP-054]).
/// - `identity`, `id`, `created_at`: the signer, a fresh `id` ([SC-ENV-023]) and the
///   creation time.
///
/// A new message carries the request's `conversation_id` and `correlation_id`, which must
/// be identifier tokens. A reply carries the hand-off record's, and never the request's.
///
/// # Errors
///
/// `unauthorized` for an unattributed request (step 1); `invalid-request` for empty content
/// or a `conversation_id` or `correlation_id` that is not an identifier token; then the
/// code of the earliest step of [`check_send`] that applies. Every code has `request` in its
/// Table 8.3 scope, and no envelope exists for a refused request ([SC-RCP-075]).
#[allow(clippy::too_many_arguments)]
pub fn prepare_send<'d>(
    requester: Option<&SessionId>,
    request: &SendRequest,
    presence: impl FnOnce(&SessionId, &SessionId) -> Result<&'d SessionCapabilities, ErrorCode>,
    implemented: &[Implemented],
    reply: ReplyHeaders,
    identity: &DeviceIdentity,
    id: Token,
    created_at: Timestamp,
) -> Result<PreparedSend, ErrorCode> {
    // Step 1 first: an unattributed request learns nothing more ([SC-RCP-090]).
    let from = requester.ok_or(ErrorCode::Unauthorized)?;
    if request.content.is_empty() {
        return Err(ErrorCode::InvalidRequest);
    }
    let (headers, correlation) = if request.requested_target.is_some() {
        let c = if reply.correlated() {
            Correlation::Correlated
        } else {
            Correlation::Uncorrelated
        };
        (reply, Some(c))
    } else {
        let token = |v: &Option<String>| match v {
            None => Ok(None),
            Some(s) => Token::parse(s).map(Some).ok_or(ErrorCode::InvalidRequest),
        };
        (
            ReplyHeaders {
                reply_to: None,
                conversation_id: token(&request.conversation_id)?,
                correlation_id: token(&request.correlation_id)?,
            },
            None,
        )
    };
    let part_types: Vec<&str> = request.content.iter().map(|p| p.part_type()).collect();
    let mut built: Option<Envelope> = None;
    let agreed = check_send(
        Some(from),
        &request.to,
        presence,
        implemented,
        &part_types,
        |version| {
            let Some(draft) = EnvelopeDraft::with_parts(
                id,
                from.clone(),
                request.to.clone(),
                created_at,
                request.content.clone(),
            ) else {
                return u64::MAX;
            };
            let env = identity.sign_envelope(headers.apply(draft.with_version(version)));
            let n = env.octets().len() as u64;
            built = Some(env);
            n
        },
    )?;
    let envelope = built.ok_or(ErrorCode::InternalError)?;
    Ok(PreparedSend {
        envelope,
        agreed,
        correlation,
    })
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

/// One state held for an envelope: a state and its observer (`spec/session-channels.md`
/// §8.5, stage `combine`, `held`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldState {
    /// The state as processed ([SC-RCP-030]: `failed` for an unrecognized code).
    pub state: DeliveryState,
    /// Who observed it.
    pub observer: Observer,
}

impl HeldState {
    /// A state as carried, with its code as carried, processed by [SC-RCP-030]: a code that
    /// is not in Table 8.3 makes any state but `duplicate` count as `failed`, as
    /// [`DeliveryReceipt::effective_state`] does for a receipt.
    pub fn processed(state: DeliveryState, observer: Observer, error: Option<&str>) -> HeldState {
        let state = match error {
            Some(e) if ErrorCode::parse(e).is_none() && state != DeliveryState::Duplicate => {
                DeliveryState::Failed
            }
            _ => state,
        };
        HeldState { state, observer }
    }
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

/// The states held for one envelope, kept as the rules of [SC-RCP-085] and §8.4.2 read
/// them: whether any `handed-to-harness`, any `duplicate` and any receiver-observed
/// `unknown` is held, and the first receiver-observed and the first sender-observed error
/// state. Its size is fixed: a receipt replayed any number of times, which nothing in §10
/// stops, changes at most one flag once and adds nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Held {
    handed_to_harness: bool,
    duplicate: bool,
    receiver_unknown: bool,
    first_receiver_error: Option<DeliveryState>,
    first_sender_error: Option<DeliveryState>,
}

impl Held {
    /// Nothing held.
    pub fn new() -> Held {
        Held::default()
    }

    /// The states `held`, in arrival order.
    pub fn of<'a>(held: impl IntoIterator<Item = &'a HeldState>) -> Held {
        let mut h = Held::new();
        for s in held {
            h.add(s);
        }
        h
    }

    /// Adds one state, after those already held.
    pub fn add(&mut self, h: &HeldState) {
        match (h.state, h.observer) {
            (DeliveryState::HandedToHarness, _) => self.handed_to_harness = true,
            (DeliveryState::Duplicate, _) => self.duplicate = true,
            (DeliveryState::Unknown, Observer::Receiver) => self.receiver_unknown = true,
            (s, Observer::Receiver) if is_error_state(s) => {
                self.first_receiver_error.get_or_insert(s);
            }
            (s, Observer::Sender) if is_error_state(s) => {
                self.first_sender_error.get_or_insert(s);
            }
            // `accepted-by-adapter`, and a sender-observed `unknown`, which no rule reads.
            _ => {}
        }
    }
}

/// The combined state of an envelope ([SC-RCP-085]): the first rule that applies to the
/// states `held`, given how many copies were passed to a transport and whether the
/// hand-off deadline has passed on the sending implementation's clock.
pub fn combined_state(
    copies_passed: u64,
    held: &Held,
    handoff_deadline_passed: bool,
) -> DeliveryState {
    // Rule 1, then rule 2: a late `duplicate` never overwrites `handed-to-harness`.
    if held.handed_to_harness {
        return DeliveryState::HandedToHarness;
    }
    if held.duplicate {
        return DeliveryState::Duplicate;
    }
    // Rule 3: a receiver-observed `unknown`.
    if held.receiver_unknown {
        return DeliveryState::Unknown;
    }
    // Rule 4: one copy passed, the error state received first. Only a receiver reports
    // one for a passed copy: the sender's own error state is not reported once a copy has
    // been passed ([SC-RCP-007]).
    if copies_passed == 1
        && let Some(s) = held.first_receiver_error
    {
        return s;
    }
    // Rule 5: no copy passed, the sender's own error state.
    if copies_passed == 0
        && let Some(s) = held.first_sender_error
    {
        return s;
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
    held: &Held,
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
        && !held.receiver_unknown
}

/// The delivery-state machine a sending implementation keeps for one envelope (§8.4.1).
/// Its memory is fixed: see [`Held`].
#[derive(Clone, Debug)]
pub struct EnvelopeTracker {
    id: Token,
    from: SessionId,
    deadline: HandOffDeadline,
    copies_passed: u64,
    held: Held,
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
            held: Held::new(),
        }
    }

    /// A copy was passed to a transport (`PublishResult` `taken`): `accepted-by-adapter`.
    pub fn passed(&mut self) {
        self.copies_passed = self.copies_passed.saturating_add(1);
    }

    /// A copy could not be passed to a transport (`PublishResult` `not-taken`, or an
    /// internal error before it): `failed` with `transport-failure` or `internal-error`.
    /// The copy is not passed (§8.4.1). Returns false, recording nothing, for any other
    /// code: no other code fits a sender-observed `failed` (Table 8.3).
    pub fn not_passed(&mut self, code: ErrorCode) -> bool {
        if !code.fits_receipt(DeliveryState::Failed, Observer::Sender) {
            return false;
        }
        self.held.add(&HeldState {
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
        self.held.add(&HeldState {
            state: r.effective_state(),
            observer: r.observer(),
        });
        true
    }

    /// The number of copies passed to a transport.
    pub fn copies_passed(&self) -> u64 {
        self.copies_passed
    }

    /// The states held, as the rules read them.
    pub fn held(&self) -> &Held {
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
        assert_eq!(t.held(), &Held::new());
    }

    fn h(state: DeliveryState, observer: Observer) -> HeldState {
        HeldState { state, observer }
    }

    /// The observer distinctions of [SC-RCP-085] rules 3 and 5 and of [SC-RCP-082]: only a
    /// receiver's `unknown` decides rule 3 and forbids a retry; only the sender's own error
    /// state decides rule 5.
    #[test]
    fn observers_matter() {
        use DeliveryState as S;
        use Observer::{Receiver as R, Sender as Snd};
        // Rule 3: a sender-observed `unknown` is not a receiver's.
        let sender_unknown = Held::of(&[h(S::AcceptedByAdapter, Snd), h(S::Unknown, Snd)]);
        assert_eq!(
            combined_state(1, &sender_unknown, false),
            S::AcceptedByAdapter
        );
        let receiver_unknown = Held::of(&[h(S::Unknown, R)]);
        assert_eq!(combined_state(1, &receiver_unknown, false), S::Unknown);
        // Retry: a sender's `unknown` (rule 6) allows one after the deadline; a receiver's
        // does not.
        assert!(retry_allowed(1, &sender_unknown, S::Unknown, true));
        assert!(!retry_allowed(1, &receiver_unknown, S::Unknown, true));
        // Rule 5: with no copy passed, a receiver's error state is not the sender's own.
        let receiver_error = Held::of(&[h(S::Rejected, R)]);
        assert_eq!(
            combined_state(0, &receiver_error, false),
            S::AcceptedByAdapter
        );
        let sender_error = Held::of(&[h(S::Failed, Snd)]);
        assert_eq!(combined_state(0, &sender_error, false), S::Failed);
        // Rule 4: one copy passed, the first receiver error; the sender's is not read.
        let both = Held::of(&[h(S::Failed, Snd), h(S::Expired, R), h(S::Rejected, R)]);
        assert_eq!(combined_state(1, &both, false), S::Expired);
        assert_eq!(combined_state(2, &both, false), S::AcceptedByAdapter);
    }

    /// [SC-RCP-030] in core: an unrecognized code is `failed`, except for `duplicate`.
    #[test]
    fn unrecognized_code_is_failed() {
        let r = Observer::Receiver;
        assert_eq!(
            HeldState::processed(DeliveryState::Rejected, r, Some("from-a-later-minor")).state,
            DeliveryState::Failed
        );
        assert_eq!(
            HeldState::processed(DeliveryState::Duplicate, r, Some("from-a-later-minor")).state,
            DeliveryState::Duplicate
        );
        assert_eq!(
            HeldState::processed(DeliveryState::Rejected, r, Some("unauthorized")).state,
            DeliveryState::Rejected
        );
    }

    /// A receipt replayed many times leaves the tracker as one copy of it does: its memory
    /// is fixed ([`Held`]).
    #[test]
    fn replayed_receipts_add_nothing() {
        let env = envelope(60_000);
        let mut t = EnvelopeTracker::new(&env);
        t.passed();
        let r = receipt(&env, DeliveryState::Failed, Some(ErrorCode::HandoffFailed));
        assert!(t.receipt(&r));
        let once = *t.held();
        for _ in 0..10_000 {
            assert!(t.receipt(&r));
        }
        assert_eq!(*t.held(), once);
        assert_eq!(t.state(&ts("2026-10-03T12:00:10Z")), DeliveryState::Failed);
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
