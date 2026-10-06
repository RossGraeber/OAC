// SPDX-License-Identifier: Apache-2.0

//! Authorization (`spec/security.md` §9; `spec/interfaces.md` §4.9): grants, the binding
//! table, reply rights, the five kinds of decision, and security step 4 of Table 7.1.
//!
//! # Default deny
//!
//! Nothing is permitted until a recorded fact permits it ([SEC-AUZ-001]). An
//! [`AuthorizationEngine`] starts with no grant, no reply right, no hand-off record and no
//! operator setting, so every decision it makes is `deny` until an operator adds a grant
//! or the engine's own sessions send or are handed messages. A [`Outcome::Permit`] cannot
//! be built without the [`Basis`] that permits it ([IFC-TYP-081]), and only the engine
//! builds decisions.
//!
//! # What a decision may read
//!
//! An [`AuthorizationRequest`] holds, for its kind, only the per-question inputs Table 4.9
//! lists ([IFC-TYP-080]). Everything else a decision reads is a fact the engine recorded
//! itself: the grants, the binding table, the own-session facts, and the sent and hand-off
//! records. No request has a slot for a display form, an alias, a `display_name`, a harness
//! label, a principal label on its own, a transport peer identifier, a harness-native id, a
//! cross-check value, a memory reference or anything from `content` ([SEC-AUZ-004],
//! [SEC-AUZ-024]). A `deliver` request is built only from a verified [`ChannelMessage`]
//! ([`AuthorizationRequest::deliver`]), so the key id it carries is the one that verified
//! the signature, never a claimed one.
//!
//! # Authenticated but untrusted
//!
//! A permit of kind `deliver` authorizes delivery of the message into the addressed
//! session's input, and nothing else. It never authorizes an action the message's content
//! asks for ([SEC-AUZ-020]): approving a permission request is its own kind,
//! `relay-permission`, off unless an operator enabled it for the session ([SEC-AUZ-021]),
//! and a decision of one kind is never read as a decision of another ([IFC-TYP-082];
//! [`AuthorizationDecision::permits`]). There is no kind that enables steering a running
//! turn, because no decision may enable it ([SEC-AUZ-022]).
//!
//! # Logging
//!
//! Every decision the engine makes is written to its [`DecisionLog`], with the principal it
//! concerns and the session ids involved, never with a message body: a [`DecisionRecord`]
//! is built from the request, which has no slot for content.

use crate::clock::Clock;
use crate::delivery::{DeliveryState, ErrorCode};
use crate::envelope::{ChannelMessage, Envelope, SecurityPrincipal};
use crate::ids::{KeyId, SessionId, Timestamp, Token};
use crate::keys::DeviceIdentity;
use crate::pairing::{PairedPeer, PairingRecord, PairingSnapshot, PairingStore, PairingStoreError};
use crate::registration::RegistrationRecord;
use crate::trust::{AddKeyError, TrustedKeySet};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex};

/// The reply period of `spec/security.md` §9.5: 86400000 milliseconds (24 hours).
pub const REPLY_PERIOD_MS: u64 = 86_400_000;

const NANOS_PER_MS: i128 = 1_000_000;

/// True while `now` is before `created_at` plus the reply period. The period ends at that
/// instant: a record created exactly 24 hours before `now` no longer counts
/// (`sec-auz/SEC-AUZ-015.n01`, `SEC-AUZ-016.n02`).
fn within_reply_period(created_at: &Timestamp, now: &Timestamp) -> bool {
    now.unix_nanos() < created_at.unix_nanos() + i128::from(REPLY_PERIOD_MS) * NANOS_PER_MS
}

/// A working-directory scope (`spec/security.md` §9.3): the working directory a session's
/// registration record holds. Two scopes are the same only when equal as recorded, so a
/// subdirectory is another scope ([SEC-AUZ-008]). Local: never sent to a peer
/// ([SEC-KEY-042]), and never written into a [`DecisionRecord`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkingDirectoryScope(String);

impl WorkingDirectoryScope {
    /// The scope recorded as `s`, unchanged.
    pub fn new(s: impl Into<String>) -> WorkingDirectoryScope {
        WorkingDirectoryScope(s.into())
    }

    /// The scope as recorded.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for WorkingDirectoryScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WorkingDirectoryScope(..)")
    }
}

/// The side of a grant on this implementation's own device (`spec/security.md` §9.2): one of
/// its sessions, one of its working-directory scopes, or the whole device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalSide {
    /// One session. As a target it names that id, and covers it until an operator removes
    /// the grant, even after the id's binding has ended (§9.2, dated note after
    /// [SEC-AUZ-006]); it never covers another id ([SEC-AUZ-006]).
    Session(SessionId),
    /// Every session registered with this scope at the time of the check ("folder-wide").
    Scope(WorkingDirectoryScope),
    /// Every session this implementation binds at the time of the check ("machine-wide").
    Device,
}

impl LocalSide {
    /// Whether this side includes the session `s`, given the own-session facts.
    fn includes(&self, s: &SessionId, own: &BTreeMap<SessionId, WorkingDirectoryScope>) -> bool {
        match self {
            LocalSide::Session(id) => id == s,
            LocalSide::Scope(scope) => own.get(s) == Some(scope),
            LocalSide::Device => own.contains_key(s),
        }
    }
}

/// The side of a grant on another device (`spec/security.md` §9.2): a key id (any session
/// bound to that device key), or a key id and a session id (that one session).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerSide {
    /// The device's key id.
    pub key_id: KeyId,
    /// One session of that device, or `None` for any session bound to its key.
    pub session_id: Option<SessionId>,
}

impl PeerSide {
    /// Every session bound to `key_id`.
    pub fn device(key_id: KeyId) -> PeerSide {
        PeerSide {
            key_id,
            session_id: None,
        }
    }

    /// The one session `session_id` of the device `key_id`.
    pub fn session(key_id: KeyId, session_id: SessionId) -> PeerSide {
        PeerSide {
            key_id,
            session_id: Some(session_id),
        }
    }

    /// Whether this side names the key `key` together with `session`, or `key` with no
    /// session.
    fn names(&self, key: &KeyId, session: &SessionId) -> bool {
        &self.key_id == key && self.session_id.as_ref().is_none_or(|s| s == session)
    }
}

/// A grant (`spec/security.md` §9.2): a **writer** may send to a **target**. One-way: it
/// never lets the target send to the writer. An operator records it on both
/// implementations involved, each in the form it evaluates. Between two sessions of one
/// implementation the inbound grant alone serves, with the implementation's own key id as
/// the writer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Grant {
    /// On the target's implementation: a peer (or this device's own key) may write to a
    /// local session, scope or the device.
    Inbound {
        /// Who may write.
        writer: PeerSide,
        /// What it may write to.
        target: LocalSide,
    },
    /// On the writer's implementation: a local session, scope or the device may write to a
    /// peer device or one of its sessions.
    Outbound {
        /// Who may write.
        writer: LocalSide,
        /// What it may write to.
        target: PeerSide,
    },
}

impl Grant {
    /// Whether the grant names the key id `key` ([SEC-KEY-035]).
    pub fn names_key(&self, key: &KeyId) -> bool {
        match self {
            Grant::Inbound { writer, .. } => &writer.key_id == key,
            Grant::Outbound { target, .. } => &target.key_id == key,
        }
    }
}

/// An entry of the binding table (`spec/security.md` §11.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Binding {
    /// The session id is bound to this key id.
    Key(KeyId),
    /// A conflict mark: these key ids each claimed the session id. While it stands, the
    /// session id is bound to no key and nothing claiming it is accepted ([SEC-PRS-012]).
    Conflict(BTreeSet<KeyId>),
}

impl Binding {
    /// Whether the entry names `key`.
    fn names(&self, key: &KeyId) -> bool {
        match self {
            Binding::Key(k) => k == key,
            Binding::Conflict(keys) => keys.contains(key),
        }
    }
}

/// What [`AuthorizationEngine::bind`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindOutcome {
    /// The session id had no entry, and is now bound to the key.
    Bound,
    /// The session id was already bound to the same key.
    AlreadyBound,
    /// The session id is bound to another key. Nothing changed; the caller records a
    /// finding ([SEC-PRS-004]).
    BoundToOther(KeyId),
    /// The session id is under a conflict mark. Nothing changed ([SEC-PRS-012]).
    UnderConflict,
}

/// A record of an envelope one of this implementation's own sessions passed to a
/// transport ([SEC-AUZ-013]): the source of a reply right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SentRecord {
    /// The envelope's `id`.
    pub id: Token,
    /// The sending session, one of this implementation's own.
    pub from: SessionId,
    /// The addressed session.
    pub to: SessionId,
    /// The key `to` was bound to when the envelope was sent.
    pub to_key_id: KeyId,
    /// The envelope's `created_at`.
    pub created_at: Timestamp,
}

impl SentRecord {
    /// The record for `env`, sent while `to` was bound to `to_key`.
    pub fn of(env: &Envelope, to_key: KeyId) -> SentRecord {
        SentRecord {
            id: env.id().clone(),
            from: env.from().clone(),
            to: env.to().clone(),
            to_key_id: to_key,
            created_at: env.created_at().clone(),
        }
    }
}

/// A record of an envelope this implementation handed off to one of its own sessions with
/// the outcome `handed-to-harness` or `unknown` ([SEC-AUZ-016]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandOffRecord {
    /// The envelope's `id`.
    pub id: Token,
    /// The envelope's `from`.
    pub from: SessionId,
    /// The receiving session, one of this implementation's own.
    pub to: SessionId,
    /// The envelope's `created_at`.
    pub created_at: Timestamp,
}

impl HandOffRecord {
    /// The record for `env`.
    pub fn of(env: &Envelope) -> HandOffRecord {
        HandOffRecord {
            id: env.id().clone(),
            from: env.from().clone(),
            to: env.to().clone(),
            created_at: env.created_at().clone(),
        }
    }
}

/// The kinds of question of `spec/interfaces.md` Table 4.9.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// Does an inbound grant or a live reply right cover this verified envelope?
    Deliver,
    /// May this requester discover this session?
    Discover,
    /// May this own session's presence record be released to this device?
    ReleasePresence,
    /// Is this announcement's signing key related to this consumer for this session?
    AcceptPresence,
    /// May a peer message answer this session's permission request?
    RelayPermission,
}

impl Kind {
    /// The kind's name in Table 4.9.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Deliver => "deliver",
            Kind::Discover => "discover",
            Kind::ReleasePresence => "release-presence",
            Kind::AcceptPresence => "accept-presence",
            Kind::RelayPermission => "relay-permission",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The per-question inputs of a `deliver` request. Built only from a verified message, so
/// the key id is the one that verified the signature ([SEC-STG-003]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliverQuestion {
    verifying_key: KeyId,
    from: SessionId,
    to: SessionId,
    reply_to: Option<Token>,
}

impl DeliverQuestion {
    /// The key id that verified the envelope.
    pub fn verifying_key(&self) -> &KeyId {
        &self.verifying_key
    }

    /// The envelope's `from`.
    pub fn from(&self) -> &SessionId {
        &self.from
    }

    /// The envelope's `to`.
    pub fn to(&self) -> &SessionId {
        &self.to
    }

    /// The envelope's `reply_to`, if any.
    pub fn reply_to(&self) -> Option<&Token> {
        self.reply_to.as_ref()
    }
}

/// Who asks to discover a session (Table 4.9, kind `discover`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Requester {
    /// One of this implementation's own sessions.
    Session(SessionId),
    /// A peer device, by key id: may this implementation release the session's presence
    /// record to it ([SEC-AUZ-011])?
    Device(KeyId),
}

/// One question of `spec/security.md` §9 (`spec/interfaces.md` §4.9). Each kind holds only
/// the per-question inputs Table 4.9 lists for it ([IFC-TYP-080]); the recorded facts it
/// may also read are the engine's own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthorizationRequest {
    /// Kind `deliver`: the verifying key id and the envelope's `from`, `to` and
    /// `reply_to`.
    Deliver(DeliverQuestion),
    /// Kind `discover`: the requester and the session id.
    Discover {
        /// An own session id, or a peer device's key id.
        requester: Requester,
        /// The session to discover.
        session: SessionId,
    },
    /// Kind `release-presence`: the session id and the device's key id.
    ReleasePresence {
        /// One of this implementation's own sessions.
        session: SessionId,
        /// The peer device.
        device: KeyId,
    },
    /// Kind `accept-presence`: the signing key id and the session id.
    AcceptPresence {
        /// The key that signed the announcement (after it verified).
        signing_key: KeyId,
        /// The session the announcement is for.
        session: SessionId,
    },
    /// Kind `relay-permission`: the session id.
    RelayPermission {
        /// The session whose permission request a peer message would answer.
        session: SessionId,
    },
}

impl AuthorizationRequest {
    /// The `deliver` request for `msg`: its verifying key id, `from`, `to` and `reply_to`,
    /// and nothing from `content`. `None` when `msg` has not passed security steps 1 and 2
    /// ([IFC-TYP-041], [SEC-STG-003]).
    pub fn deliver(msg: &ChannelMessage) -> Option<AuthorizationRequest> {
        let by = msg.verified_by()?;
        let env = msg.envelope();
        Some(AuthorizationRequest::Deliver(DeliverQuestion {
            verifying_key: by.key_id().clone(),
            from: env.from().clone(),
            to: env.to().clone(),
            reply_to: env.reply_to().cloned(),
        }))
    }

    /// The request's kind.
    pub fn kind(&self) -> Kind {
        match self {
            AuthorizationRequest::Deliver(_) => Kind::Deliver,
            AuthorizationRequest::Discover { .. } => Kind::Discover,
            AuthorizationRequest::ReleasePresence { .. } => Kind::ReleasePresence,
            AuthorizationRequest::AcceptPresence { .. } => Kind::AcceptPresence,
            AuthorizationRequest::RelayPermission { .. } => Kind::RelayPermission,
        }
    }
}

/// The recorded fact that permits a decision ([IFC-TYP-081]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Basis {
    /// An inbound grant.
    InboundGrant(Grant),
    /// An outbound grant.
    OutboundGrant(Grant),
    /// A live reply right, from this sent record ([SEC-AUZ-014], [SEC-AUZ-015]).
    ReplyRight(SentRecord),
    /// A sent record within the reply period ([SEC-AUZ-017]).
    Sent(SentRecord),
    /// A hand-off record within the reply period ([SEC-AUZ-016], [SEC-AUZ-017]).
    HandOff(HandOffRecord),
    /// The operator enabled permission relay for this session ([SEC-AUZ-021]).
    RelaySetting(SessionId),
}

impl Basis {
    /// A short label for a log line.
    pub fn label(&self) -> &'static str {
        match self {
            Basis::InboundGrant(_) => "inbound-grant",
            Basis::OutboundGrant(_) => "outbound-grant",
            Basis::ReplyRight(_) => "reply-right",
            Basis::Sent(_) => "sent-record",
            Basis::HandOff(_) => "hand-off-record",
            Basis::RelaySetting(_) => "relay-setting",
        }
    }
}

/// `permit`, with its basis, or `deny`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Permitted, because of this recorded fact.
    Permit(Basis),
    /// Denied: no recorded fact permits it. The default.
    Deny,
}

/// The answer to one [`AuthorizationRequest`] (`spec/interfaces.md` §4.9). Only the engine
/// builds one, so a `permit` always names its basis ([IFC-TYP-081]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizationDecision {
    kind: Kind,
    outcome: Outcome,
}

impl AuthorizationDecision {
    /// The kind of question this decision answers.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// `permit` with its basis, or `deny`.
    pub fn outcome(&self) -> &Outcome {
        &self.outcome
    }

    /// The basis of a `permit`; `None` for `deny`.
    pub fn basis(&self) -> Option<&Basis> {
        match &self.outcome {
            Outcome::Permit(b) => Some(b),
            Outcome::Deny => None,
        }
    }

    /// Whether this decision permits a question of kind `kind`. False for a decision of any
    /// other kind, whatever its outcome: a decision of one kind is never the outcome of a
    /// decision of another ([IFC-TYP-082]). A `deliver` permit does not permit relay
    /// ([SEC-AUZ-020]).
    pub fn permits(&self, kind: Kind) -> bool {
        self.kind == kind && matches!(self.outcome, Outcome::Permit(_))
    }
}

/// An operator's confirmation, required to create a grant ([SEC-AUZ-005]), to enable
/// permission relay ([SEC-AUZ-021]) and to remove a key ([SEC-KEY-035]).
///
/// The core cannot see an operator. This value is the caller's statement that one
/// confirmed the change: only an operator-facing path (the command line) builds it, never
/// an adapter acting on a harness's, a session's or a model's request. A model that asks
/// for access, in content or through a harness, never grants it to itself.
#[derive(Clone, Copy, Debug)]
pub struct OperatorConfirmed(());

impl OperatorConfirmed {
    /// States that an operator confirmed the change at hand.
    pub fn by_operator() -> OperatorConfirmed {
        OperatorConfirmed(())
    }
}

/// A local record the security stage requires: a claim on a session id bound to another
/// key ([SEC-PRS-004]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingFinding {
    /// The session id claimed.
    pub session_id: SessionId,
    /// The key it is bound to.
    pub bound_to: KeyId,
    /// The key that claimed it.
    pub claimed_by: KeyId,
}

/// One decision, as logged: the kind, the outcome and its basis, the principal it concerns
/// and the session ids involved. It has no member that could hold a message body or any
/// other content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionRecord {
    /// The engine's clock at the decision.
    pub at: Timestamp,
    /// The kind.
    pub kind: Kind,
    /// `true` for `permit`.
    pub permitted: bool,
    /// The basis label of a `permit` ([`Basis::label`]).
    pub basis: Option<&'static str>,
    /// The principal the decision concerns, from the trusted key set: the verifying key's
    /// for `deliver`, the signing key's for `accept-presence`, the device's for
    /// `release-presence` and a device requester. `None` when the key is not trusted or the
    /// kind names none.
    pub principal: Option<SecurityPrincipal>,
    /// The key id the decision concerns, when it names one.
    pub key_id: Option<KeyId>,
    /// The sending or requesting session, when there is one.
    pub from: Option<SessionId>,
    /// The addressed, discovered or released session.
    pub session: Option<SessionId>,
}

/// One line of the decision log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogEntry {
    /// A decision.
    Decision(DecisionRecord),
    /// A finding ([SEC-PRS-004]).
    Finding(BindingFinding),
    /// An operator change: a key paired or removed, a grant added or removed, relay
    /// enabled or disabled. The text names key ids and session ids only.
    Change(String),
}

/// Where an [`AuthorizationEngine`] writes its decisions, findings and operator changes.
pub trait DecisionLog: Send {
    /// Writes one entry.
    fn record(&mut self, entry: LogEntry);
}

/// A [`DecisionLog`] in memory. Clones share one list, so a test (or a status command) can
/// keep a clone and read what the engine wrote.
#[derive(Clone, Debug, Default)]
pub struct MemoryDecisionLog {
    entries: Arc<Mutex<Vec<LogEntry>>>,
}

impl MemoryDecisionLog {
    /// An empty log.
    pub fn new() -> MemoryDecisionLog {
        MemoryDecisionLog::default()
    }

    /// Every entry written so far.
    pub fn entries(&self) -> Vec<LogEntry> {
        self.entries.lock().map(|e| e.clone()).unwrap_or_default()
    }
}

impl DecisionLog for MemoryDecisionLog {
    fn record(&mut self, entry: LogEntry) {
        if let Ok(mut e) = self.entries.lock() {
            e.push(entry);
        }
    }
}

/// Why security step 4 refused an envelope: `rejected` with `unauthorized` (Table 7.1), the
/// requirement that failed, for a log line, and the finding [SEC-PRS-004] requires, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizationRefusal {
    /// `rejected`.
    pub state: DeliveryState,
    /// `unauthorized`, whatever the reason, so a sender learns nothing else ([SC-RCP-073]).
    pub error: ErrorCode,
    /// The requirement that failed. Not sent to a peer.
    pub requirement: &'static str,
    /// The finding, when `from` is bound to another key.
    pub finding: Option<BindingFinding>,
}

impl fmt::Display for AuthorizationRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}): {}", self.state, self.error, self.requirement)
    }
}

impl std::error::Error for AuthorizationRefusal {}

/// Why [`AuthorizationEngine::remove_key`] refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoveKeyError {
    /// The key is this device's own; the set always holds it ([SEC-KEY-031]).
    OwnKey,
    /// The key is not in the trusted key set.
    NotTrusted,
    /// The key, its grants and its bindings were removed in memory, but the store did not
    /// save the change. The engine stays without them (fail closed), but the store still
    /// holds them, so a restart before a successful save would restore the revoked device.
    /// The caller must report this to the operator loudly and retry the save with
    /// [`AuthorizationEngine::save`] until it succeeds (the `oac` pairing verb, #71).
    NotSaved(PairingStoreError),
}

impl fmt::Display for RemoveKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemoveKeyError::OwnKey => f.write_str("this device's own key cannot be removed"),
            RemoveKeyError::NotTrusted => f.write_str("the key is not trusted"),
            RemoveKeyError::NotSaved(e) => write!(f, "removed, but not saved: {e}"),
        }
    }
}

impl std::error::Error for RemoveKeyError {}

/// Why [`AuthorizationEngine::pair`] refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PairError {
    /// The key is already trusted.
    AlreadyTrusted(AddKeyError),
    /// The store did not save the pairing; the key was not added.
    NotSaved(PairingStoreError),
}

impl fmt::Display for PairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PairError::AlreadyTrusted(e) => e.fmt(f),
            PairError::NotSaved(e) => write!(f, "pairing not saved: {e}"),
        }
    }
}

impl std::error::Error for PairError {}

/// Why [`AuthorizationEngine::restore`] refused a snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestoreError {
    /// The store could not be read.
    Store(PairingStoreError),
    /// The snapshot pairs a key twice, or pairs this device's own key.
    DuplicateKey(KeyId),
}

impl fmt::Display for RestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RestoreError::Store(e) => e.fmt(f),
            RestoreError::DuplicateKey(k) => write!(f, "key {k} paired twice"),
        }
    }
}

impl std::error::Error for RestoreError {}

/// What a successful [`AuthorizationEngine::remove_key`] removed, in the same step
/// ([SEC-KEY-035]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyRemoval {
    /// The grants that named the key.
    pub grants: Vec<Grant>,
    /// The session ids whose binding or conflict mark named the key.
    pub bindings: Vec<SessionId>,
}

/// A message that passed security step 4: the only input step 5,
/// [`crate::replay::DuplicateStore::admit`], takes. Only
/// [`AuthorizationEngine::authorize_delivery`] builds one, so an envelope refused at step 4
/// can never reserve a duplicate-store entry ([SEC-RPL-022]: "the entry is added only once
/// the copy has passed authorization").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizedMessage {
    message: ChannelMessage,
    decision: AuthorizationDecision,
}

impl AuthorizedMessage {
    /// The verified message.
    pub fn message(&self) -> &ChannelMessage {
        &self.message
    }

    /// The `deliver` decision that permitted it, with its basis.
    pub fn decision(&self) -> &AuthorizationDecision {
        &self.decision
    }

    /// The message, for the delivery stage.
    pub fn into_message(self) -> ChannelMessage {
        self.message
    }

    /// A message taken as authorized without a decision, for unit tests of later steps.
    #[cfg(test)]
    pub(crate) fn for_tests(message: ChannelMessage) -> AuthorizedMessage {
        let key = message.verified_by().map_or_else(
            || KeyId::parse(&"0".repeat(64)).unwrap(),
            |p| p.key_id().clone(),
        );
        AuthorizedMessage {
            message,
            decision: AuthorizationDecision {
                kind: Kind::Deliver,
                outcome: Outcome::Permit(Basis::InboundGrant(Grant::Inbound {
                    writer: PeerSide::device(key),
                    target: LocalSide::Device,
                })),
            },
        }
    }
}

/// The authorization engine: the trusted key set, the grants, the binding table, the
/// own-session facts, the sent and hand-off records and the operator's relay settings,
/// and the decisions of Table 4.9 over them.
///
/// The trusted key set lives here, beside the grants and the binding table, so that
/// removing a key removes every grant and binding that names it in the same step
/// ([SEC-KEY-035]).
pub struct AuthorizationEngine {
    own_key: KeyId,
    trusted: TrustedKeySet,
    paired: BTreeMap<KeyId, PairingRecord>,
    own_sessions: BTreeMap<SessionId, WorkingDirectoryScope>,
    grants: Vec<Grant>,
    bindings: BTreeMap<SessionId, Binding>,
    sent: Vec<SentRecord>,
    handed_off: Vec<HandOffRecord>,
    relay: BTreeSet<SessionId>,
    clock: Arc<dyn Clock>,
    log: Box<dyn DecisionLog>,
}

impl fmt::Debug for AuthorizationEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthorizationEngine")
            .field("own_key", &self.own_key)
            .field("trusted", &self.trusted.len())
            .field("own_sessions", &self.own_sessions.len())
            .field("grants", &self.grants.len())
            .field("bindings", &self.bindings.len())
            .finish_non_exhaustive()
    }
}

impl AuthorizationEngine {
    /// An engine for the device `own` with no configuration: only `own`'s key is trusted
    /// ([SEC-KEY-031]), and there is no grant, no session, no record and no relay setting,
    /// so every decision is `deny` ([SEC-AUZ-001]).
    ///
    /// Two harnesses on this device need no pairing: their sessions are bound to this one
    /// device key, which is always trusted (`docs/planning/decisions/C5-envelope-auth.md`
    /// §10(a)). They still need a grant to reach each other ([SEC-AUZ-007]).
    ///
    /// Every decision reads the time from `clock`, the receiver clock the replay checks read
    /// too ([`crate::replay`]).
    pub fn new(
        own: &DeviceIdentity,
        clock: Arc<dyn Clock>,
        log: Box<dyn DecisionLog>,
    ) -> AuthorizationEngine {
        AuthorizationEngine {
            own_key: own.key_id().clone(),
            trusted: TrustedKeySet::new(own),
            paired: BTreeMap::new(),
            own_sessions: BTreeMap::new(),
            grants: Vec::new(),
            bindings: BTreeMap::new(),
            sent: Vec::new(),
            handed_off: Vec::new(),
            relay: BTreeSet::new(),
            clock,
            log,
        }
    }

    /// An engine for `own` with the paired keys, grants and relay settings `store` holds.
    /// An empty store gives [`AuthorizationEngine::new`]'s default-deny engine.
    ///
    /// # Errors
    ///
    /// [`RestoreError`] when the store cannot be read or pairs a key twice.
    pub fn restore(
        own: &DeviceIdentity,
        store: &dyn PairingStore,
        clock: Arc<dyn Clock>,
        log: Box<dyn DecisionLog>,
    ) -> Result<AuthorizationEngine, RestoreError> {
        let snapshot = store.load().map_err(RestoreError::Store)?;
        let mut engine = AuthorizationEngine::new(own, clock, log);
        for record in snapshot.paired {
            let key_id = record.key_id();
            engine
                .trusted
                .add_paired_key(record.principal().clone(), *record.public_key())
                .map_err(|_| RestoreError::DuplicateKey(key_id.clone()))?;
            engine.paired.insert(key_id, record);
        }
        for grant in snapshot.grants {
            if !engine.grants.contains(&grant) {
                engine.grants.push(grant);
            }
        }
        engine.relay = snapshot.relay_enabled.into_iter().collect();
        Ok(engine)
    }

    /// The paired keys, grants and relay settings, the state a [`PairingStore`] keeps.
    /// Binding tables and sent and hand-off records live in memory only
    /// (`spec/security.md` §11.3, §9.5).
    pub fn snapshot(&self) -> PairingSnapshot {
        PairingSnapshot {
            paired: self.paired.values().cloned().collect(),
            grants: self.grants.clone(),
            relay_enabled: self.relay.iter().cloned().collect(),
        }
    }

    /// Saves [`AuthorizationEngine::snapshot`] to `store`.
    ///
    /// # Errors
    ///
    /// [`PairingStoreError`] from the store.
    pub fn save(&self, store: &dyn PairingStore) -> Result<(), PairingStoreError> {
        store.save(&self.snapshot())
    }

    /// This device's key id.
    pub fn own_key_id(&self) -> &KeyId {
        &self.own_key
    }

    /// The trusted key set, for security steps 1 and 2 ([`crate::signing::authenticate`]).
    pub fn trusted_keys(&self) -> &TrustedKeySet {
        &self.trusted
    }

    /// The grants, in the order they were added.
    pub fn grants(&self) -> &[Grant] {
        &self.grants
    }

    /// The binding-table entry for `session`.
    pub fn binding(&self, session: &SessionId) -> Option<&Binding> {
        self.bindings.get(session)
    }

    /// The binding table, in session-id order.
    pub fn bindings(&self) -> impl Iterator<Item = (&SessionId, &Binding)> {
        self.bindings.iter()
    }

    /// The working-directory scope of an own session.
    pub fn scope_of(&self, session: &SessionId) -> Option<&WorkingDirectoryScope> {
        self.own_sessions.get(session)
    }

    fn change(&mut self, text: String) {
        self.log.record(LogEntry::Change(text));
    }

    // ---- Pairing and keys -------------------------------------------------------------

    /// Adds the key of a pairing an operator confirmed ([SEC-KEY-032]; [`crate::pairing`])
    /// to the trusted key set, and saves it to `store`. Pairing makes the key trusted and
    /// grants nothing: a grant decides what the key may reach ([SEC-AUZ-001]).
    ///
    /// # Errors
    ///
    /// [`PairError`]: the key is already trusted, or the store did not save it, in which
    /// case the key is not added.
    pub fn pair(&mut self, peer: PairedPeer, store: &dyn PairingStore) -> Result<(), PairError> {
        let record = peer.into_record();
        let key_id = record.key_id();
        self.trusted
            .add_paired_key(record.principal().clone(), *record.public_key())
            .map_err(PairError::AlreadyTrusted)?;
        self.paired.insert(key_id.clone(), record);
        if let Err(e) = self.save(store) {
            self.trusted.remove(&key_id);
            self.paired.remove(&key_id);
            return Err(PairError::NotSaved(e));
        }
        self.change(format!("paired key {key_id}"));
        Ok(())
    }

    /// Removes `key_id` from the trusted key set and, in the same step, every grant and
    /// every binding-table entry that names it, conflict marks included ([SEC-KEY-035]),
    /// then saves the change to `store`. A conflict mark that names the key is removed
    /// whole, so the session id becomes unbound and the remaining device's next claim binds
    /// it (`sec-key/SEC-KEY-035.p01`).
    ///
    /// # Errors
    ///
    /// [`RemoveKeyError`]. On [`RemoveKeyError::NotSaved`] the removal stands in memory.
    pub fn remove_key(
        &mut self,
        key_id: &KeyId,
        _: OperatorConfirmed,
        store: &dyn PairingStore,
    ) -> Result<KeyRemoval, RemoveKeyError> {
        if key_id == &self.own_key {
            return Err(RemoveKeyError::OwnKey);
        }
        self.trusted
            .remove(key_id)
            .ok_or(RemoveKeyError::NotTrusted)?;
        self.paired.remove(key_id);
        let (removed, kept): (Vec<Grant>, Vec<Grant>) = std::mem::take(&mut self.grants)
            .into_iter()
            .partition(|g| g.names_key(key_id));
        self.grants = kept;
        let bindings: Vec<SessionId> = self
            .bindings
            .iter()
            .filter(|(_, b)| b.names(key_id))
            .map(|(s, _)| s.clone())
            .collect();
        for s in &bindings {
            self.bindings.remove(s);
        }
        self.change(format!(
            "removed key {key_id}: {} grant(s), {} binding(s)",
            removed.len(),
            bindings.len()
        ));
        self.save(store).map_err(RemoveKeyError::NotSaved)?;
        Ok(KeyRemoval {
            grants: removed,
            bindings,
        })
    }

    // ---- Grants and settings ----------------------------------------------------------

    /// Adds a grant an operator confirmed ([SEC-AUZ-005]) and saves it to `store`. Returns
    /// `false`, and changes nothing, when the engine already holds it.
    ///
    /// # Errors
    ///
    /// [`PairingStoreError`] when the store did not save it; the grant is then not added.
    pub fn add_grant(
        &mut self,
        grant: Grant,
        _: OperatorConfirmed,
        store: &dyn PairingStore,
    ) -> Result<bool, PairingStoreError> {
        if self.grants.contains(&grant) {
            return Ok(false);
        }
        self.grants.push(grant);
        if let Err(e) = self.save(store) {
            self.grants.pop();
            return Err(e);
        }
        self.change(format!("added grant {:?}", self.grants.last()));
        Ok(true)
    }

    /// Removes a grant and saves the change to `store`. Returns `false` when the engine
    /// does not hold it.
    ///
    /// # Errors
    ///
    /// [`PairingStoreError`] when the store did not save the change; the grant stays
    /// removed in memory (fail closed).
    pub fn remove_grant(
        &mut self,
        grant: &Grant,
        store: &dyn PairingStore,
    ) -> Result<bool, PairingStoreError> {
        let before = self.grants.len();
        self.grants.retain(|g| g != grant);
        if self.grants.len() == before {
            return Ok(false);
        }
        self.change(format!("removed grant {grant:?}"));
        self.save(store)?;
        Ok(true)
    }

    /// Enables or disables permission relay for `session` ([SEC-AUZ-021]). Off unless an
    /// operator enables it, as a decision of its own, for that one session.
    ///
    /// # Errors
    ///
    /// [`PairingStoreError`] when the store did not save the change. Enabling is then
    /// undone; disabling stands (fail closed).
    pub fn set_relay(
        &mut self,
        session: SessionId,
        enabled: bool,
        _: OperatorConfirmed,
        store: &dyn PairingStore,
    ) -> Result<(), PairingStoreError> {
        if enabled {
            let added = self.relay.insert(session.clone());
            if let Err(e) = self.save(store) {
                if added {
                    self.relay.remove(&session);
                }
                return Err(e);
            }
        } else {
            self.relay.remove(&session);
            self.save(store)?;
        }
        self.change(format!("relay for {session}: {enabled}"));
        Ok(())
    }

    // ---- Sessions and the binding table -----------------------------------------------

    /// Records an own session from its registration record: its working-directory scope
    /// becomes an own-session fact, and the binding table binds it to this device's key.
    /// Returns `false`, recording nothing, unless the record verifies under this device's
    /// own key ([SEC-KEY-043]).
    pub fn register_session(&mut self, record: &RegistrationRecord, own: &DeviceIdentity) -> bool {
        if own.key_id() != &self.own_key || !record.binding_usable(own) {
            return false;
        }
        let s = record.session_id().clone();
        self.own_sessions.insert(
            s.clone(),
            WorkingDirectoryScope::new(record.working_directory()),
        );
        // An own session is never under conflict ([SEC-PRS-015]): its registration record
        // is authoritative over any earlier claim on the id.
        self.bindings.insert(s, Binding::Key(self.own_key.clone()));
        true
    }

    /// Ends an own session's binding. Reply rights from the session end with it
    /// ([SEC-AUZ-015]), and so does its right to discover a sender it was handed a message
    /// from ([SEC-AUZ-016]). A grant that names the session id stays until an operator
    /// removes it.
    pub fn end_session(&mut self, session: &SessionId) -> bool {
        if self.own_sessions.remove(session).is_none() {
            return false;
        }
        self.bindings.remove(session);
        self.sent.retain(|r| &r.from != session);
        self.handed_off.retain(|r| &r.to != session);
        true
    }

    /// Binds another implementation's session id `session` to `key`, from an accepted
    /// announcement ([SEC-PRS-005]). Only an unbound session id is bound; an envelope binds
    /// through [`AuthorizationEngine::authorize_delivery`].
    pub fn bind(&mut self, session: &SessionId, key: &KeyId) -> BindOutcome {
        match self.bindings.get(session) {
            None => {
                self.bindings
                    .insert(session.clone(), Binding::Key(key.clone()));
                BindOutcome::Bound
            }
            Some(Binding::Key(k)) if k == key => BindOutcome::AlreadyBound,
            Some(Binding::Key(k)) => BindOutcome::BoundToOther(k.clone()),
            Some(Binding::Conflict(_)) => BindOutcome::UnderConflict,
        }
    }

    /// Marks another implementation's session id as under conflict between its current key
    /// and `claimant` ([SEC-PRS-014]), or adds `claimant` to an existing mark. The caller
    /// has checked that the claimant passes the relation test of [SEC-AUZ-017]
    /// ([`Kind::AcceptPresence`]). Refused, returning `false`, for an own session
    /// ([SEC-PRS-015]) and for a session id with no entry.
    pub fn mark_conflict(&mut self, session: &SessionId, claimant: &KeyId) -> bool {
        if self.own_sessions.contains_key(session) {
            return false;
        }
        let Some(entry) = self.bindings.get_mut(session) else {
            return false;
        };
        let keys = match entry {
            Binding::Key(k) => BTreeSet::from([k.clone(), claimant.clone()]),
            Binding::Conflict(keys) => {
                let mut keys = std::mem::take(keys);
                keys.insert(claimant.clone());
                keys
            }
        };
        *entry = Binding::Conflict(keys);
        true
    }

    /// Removes a binding-table entry for a forgotten session of another implementation
    /// ([SEC-PRS-009]). A conflict mark is never removed this way, nor an own session's
    /// binding.
    pub fn forget_binding(&mut self, session: &SessionId) -> bool {
        if self.own_sessions.contains_key(session)
            || matches!(
                self.bindings.get(session),
                Some(Binding::Conflict(_)) | None
            )
        {
            return false;
        }
        self.bindings.remove(session).is_some()
    }

    // ---- Records ----------------------------------------------------------------------

    /// Records a reply right for an envelope an own session passed to a transport
    /// ([SEC-AUZ-013]); build the record with [`SentRecord::of`].
    pub fn record_sent(&mut self, record: SentRecord) {
        self.sent.push(record);
    }

    /// Records a hand-off to an own session, when its outcome was `handed-to-harness` or
    /// `unknown` ([SEC-AUZ-016]); any other outcome records nothing. Build the record with
    /// [`HandOffRecord::of`].
    pub fn record_handoff(&mut self, record: HandOffRecord, outcome: DeliveryState) {
        if matches!(
            outcome,
            DeliveryState::HandedToHarness | DeliveryState::Unknown
        ) {
            self.handed_off.push(record);
        }
    }

    /// Forgets sent and hand-off records whose reply period has ended on the engine's
    /// clock. They no longer count after that instant anyway.
    pub fn prune(&mut self) {
        let now = self.clock.now();
        self.prune_at(&now);
    }

    fn prune_at(&mut self, now: &Timestamp) {
        self.sent
            .retain(|r| within_reply_period(&r.created_at, now));
        self.handed_off
            .retain(|r| within_reply_period(&r.created_at, now));
    }

    // ---- Decisions --------------------------------------------------------------------

    /// Answers `request` at the engine clock's time and logs the decision. Default deny: the
    /// decision is `permit` only with a basis among the recorded facts its kind may read.
    pub fn decide(&mut self, request: &AuthorizationRequest) -> AuthorizationDecision {
        let now = self.clock.now();
        self.decide_at(request, &now)
    }

    fn decide_at(
        &mut self,
        request: &AuthorizationRequest,
        now: &Timestamp,
    ) -> AuthorizationDecision {
        let decision = AuthorizationDecision {
            kind: request.kind(),
            outcome: self.evaluate(request, now),
        };
        let record = self.record_of(request, &decision, now);
        self.log.record(LogEntry::Decision(record));
        decision
    }

    fn record_of(
        &self,
        request: &AuthorizationRequest,
        decision: &AuthorizationDecision,
        now: &Timestamp,
    ) -> DecisionRecord {
        let (key, from, session) = match request {
            AuthorizationRequest::Deliver(q) => {
                (Some(&q.verifying_key), Some(&q.from), Some(&q.to))
            }
            AuthorizationRequest::Discover { requester, session } => match requester {
                Requester::Session(s) => (None, Some(s), Some(session)),
                Requester::Device(k) => (Some(k), None, Some(session)),
            },
            AuthorizationRequest::ReleasePresence { session, device } => {
                (Some(device), None, Some(session))
            }
            AuthorizationRequest::AcceptPresence {
                signing_key,
                session,
            } => (Some(signing_key), None, Some(session)),
            AuthorizationRequest::RelayPermission { session } => (None, None, Some(session)),
        };
        DecisionRecord {
            at: now.clone(),
            kind: decision.kind,
            permitted: matches!(decision.outcome, Outcome::Permit(_)),
            basis: decision.basis().map(Basis::label),
            principal: key
                .and_then(|k| self.trusted.get(k))
                .map(|e| e.security_principal()),
            key_id: key.cloned(),
            from: from.cloned(),
            session: session.cloned(),
        }
    }

    fn evaluate(&self, request: &AuthorizationRequest, now: &Timestamp) -> Outcome {
        let found = match request {
            AuthorizationRequest::Deliver(q) => self.deliver(q, now),
            AuthorizationRequest::Discover { requester, session } => match requester {
                Requester::Session(l) => self.discover(l, session, now),
                Requester::Device(k) => self.release(session, k),
            },
            AuthorizationRequest::ReleasePresence { session, device } => {
                self.release(session, device)
            }
            AuthorizationRequest::AcceptPresence {
                signing_key,
                session,
            } => self.related(signing_key, session, now),
            AuthorizationRequest::RelayPermission { session } => {
                (self.own_sessions.contains_key(session) && self.relay.contains(session))
                    .then(|| Basis::RelaySetting(session.clone()))
            }
        };
        found.map_or(Outcome::Deny, Outcome::Permit)
    }

    /// Kind `deliver` ([SEC-AUZ-002], [SEC-AUZ-003], [SEC-AUZ-014], [SEC-AUZ-015]).
    fn deliver(&self, q: &DeliverQuestion, now: &Timestamp) -> Option<Basis> {
        match self.bindings.get(&q.from) {
            Some(Binding::Key(k)) if k != &q.verifying_key => return None,
            Some(Binding::Conflict(_)) => return None,
            _ => {}
        }
        let own = &self.own_sessions;
        let grant = self.grants.iter().find(|g| match g {
            Grant::Inbound { writer, target } => {
                writer.names(&q.verifying_key, &q.from) && target.includes(&q.to, own)
            }
            Grant::Outbound { .. } => false,
        });
        if let Some(g) = grant {
            return Some(Basis::InboundGrant(g.clone()));
        }
        let reply_to = q.reply_to.as_ref()?;
        self.sent
            .iter()
            .find(|e| {
                e.to_key_id == q.verifying_key
                    && e.to == q.from
                    && e.from == q.to
                    && &e.id == reply_to
                    && within_reply_period(&e.created_at, now)
                    && own.contains_key(&e.from)
            })
            .map(|e| Basis::ReplyRight(e.clone()))
    }

    /// Kind `discover` for an own session `l` ([SEC-AUZ-010], [SEC-AUZ-012],
    /// [SEC-AUZ-016]): a session sees the sessions it may write to, and a sender it was
    /// handed a message from, for the reply period.
    fn discover(&self, l: &SessionId, s: &SessionId, now: &Timestamp) -> Option<Basis> {
        let own = &self.own_sessions;
        if !own.contains_key(l) {
            return None;
        }
        let by_grant = if own.contains_key(s) {
            self.grants.iter().find(|g| match g {
                Grant::Inbound { writer, target } => {
                    writer.names(&self.own_key, l) && target.includes(s, own)
                }
                Grant::Outbound { .. } => false,
            })
        } else if let Some(Binding::Key(k)) = self.bindings.get(s) {
            self.grants.iter().find(|g| match g {
                Grant::Outbound { writer, target } => writer.includes(l, own) && target.names(k, s),
                Grant::Inbound { .. } => false,
            })
        } else {
            None
        };
        if let Some(g) = by_grant {
            return Some(match g {
                Grant::Inbound { .. } => Basis::InboundGrant(g.clone()),
                Grant::Outbound { .. } => Basis::OutboundGrant(g.clone()),
            });
        }
        self.handed_off
            .iter()
            .find(|h| &h.to == l && &h.from == s && within_reply_period(&h.created_at, now))
            .map(|h| Basis::HandOff(h.clone()))
    }

    /// Kinds `release-presence`, and `discover` with a device requester ([SEC-AUZ-011]):
    /// an inbound grant lets `k`, or a session of `k`, write to `s`, or an outbound grant
    /// lets `s` write to `k` or a session of `k`.
    fn release(&self, s: &SessionId, k: &KeyId) -> Option<Basis> {
        let own = &self.own_sessions;
        if !own.contains_key(s) {
            return None;
        }
        self.grants.iter().find_map(|g| match g {
            Grant::Inbound { writer, target } if &writer.key_id == k && target.includes(s, own) => {
                Some(Basis::InboundGrant(g.clone()))
            }
            Grant::Outbound { writer, target }
                if &target.key_id == k && writer.includes(s, own) =>
            {
                Some(Basis::OutboundGrant(g.clone()))
            }
            _ => None,
        })
    }

    /// Kind `accept-presence`, the relation test of [SEC-AUZ-017]: a grant names `k` (with
    /// `r`, or with no session) as an inbound grant's writer or an outbound grant's target,
    /// or an own session sent an envelope to `r` under `k`, or was handed one from `r`,
    /// within the reply period.
    fn related(&self, k: &KeyId, r: &SessionId, now: &Timestamp) -> Option<Basis> {
        let by_grant = self.grants.iter().find_map(|g| match g {
            Grant::Inbound { writer, .. } if writer.names(k, r) => {
                Some(Basis::InboundGrant(g.clone()))
            }
            Grant::Outbound { target, .. } if target.names(k, r) => {
                Some(Basis::OutboundGrant(g.clone()))
            }
            _ => None,
        });
        by_grant
            .or_else(|| {
                self.sent
                    .iter()
                    .find(|e| {
                        &e.to == r && &e.to_key_id == k && within_reply_period(&e.created_at, now)
                    })
                    .map(|e| Basis::Sent(e.clone()))
            })
            .or_else(|| {
                self.handed_off
                    .iter()
                    .find(|h| &h.from == r && within_reply_period(&h.created_at, now))
                    .map(|h| Basis::HandOff(h.clone()))
            })
    }

    /// Security step 4 of Table 7.1, authorization, for an envelope that passed steps 1 to
    /// 3. It fails when `from` is bound to another key or is under conflict
    /// ([SEC-AUZ-003]), or when neither an inbound grant nor a live reply right covers the
    /// envelope ([SEC-AUZ-001], [SEC-AUZ-002]). Each failure is `rejected` with
    /// `unauthorized`, whether `to` is unknown, unavailable or simply not granted
    /// (`spec/security.md` §7.1).
    ///
    /// When the envelope passes and `from` has no binding yet, `from` is bound to the
    /// verifying key ([SEC-PRS-005]). When it fails, nothing is bound. A claim on a session
    /// id bound to another key returns a finding, also written to the log ([SEC-PRS-004]).
    /// An envelope never sets a conflict mark.
    ///
    /// # Errors
    ///
    /// [`AuthorizationRefusal`].
    pub fn authorize_delivery(
        &mut self,
        msg: ChannelMessage,
    ) -> Result<AuthorizedMessage, AuthorizationRefusal> {
        let now = self.clock.now();
        self.authorize_delivery_at(msg, &now)
    }

    fn authorize_delivery_at(
        &mut self,
        msg: ChannelMessage,
        now: &Timestamp,
    ) -> Result<AuthorizedMessage, AuthorizationRefusal> {
        let refuse = |requirement, finding| AuthorizationRefusal {
            state: DeliveryState::Rejected,
            error: ErrorCode::Unauthorized,
            requirement,
            finding,
        };
        let Some(request) = AuthorizationRequest::deliver(&msg) else {
            return Err(refuse("SEC-STG-003", None));
        };
        let AuthorizationRequest::Deliver(q) = &request else {
            unreachable!("AuthorizationRequest::deliver builds a Deliver request")
        };
        let (from, key) = (q.from.clone(), q.verifying_key.clone());
        let decision = self.decide_at(&request, now);
        match self.bindings.get(&from) {
            Some(Binding::Key(bound)) if bound != &key => {
                let finding = BindingFinding {
                    session_id: from,
                    bound_to: bound.clone(),
                    claimed_by: key,
                };
                self.log.record(LogEntry::Finding(finding.clone()));
                return Err(refuse("SEC-AUZ-003", Some(finding)));
            }
            Some(Binding::Conflict(_)) => return Err(refuse("SEC-PRS-012", None)),
            _ => {}
        }
        if !decision.permits(Kind::Deliver) {
            return Err(refuse("SEC-AUZ-001", None));
        }
        self.bindings.entry(from).or_insert(Binding::Key(key));
        Ok(AuthorizedMessage {
            message: msg,
            decision,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{EnvelopeDraft, EnvelopeLimits, TextPart, receive_envelope};
    use crate::keys::DeviceKey;
    use crate::pairing::MemoryPairingStore;
    use crate::signing::authenticate;

    fn sid(s: &str) -> SessionId {
        SessionId::parse(s).unwrap()
    }

    const A1: &str = "01harn7x9k2m4p6q8r0s2t4v6w";
    const B1: &str = "7gq3m8z2c5k9t1w4x6b0n2r8vd";
    const B2: &str = "5mt9x2k7q4w8c1n6b3r0v5z2pd";
    const B3: &str = "6wd4k1q7z3m9c2t6x0b5n8r1vf";

    fn ts(s: &str) -> Timestamp {
        Timestamp::parse(s).unwrap()
    }

    fn identity(p: &str) -> DeviceIdentity {
        DeviceIdentity::new(DeviceKey::generate(), Token::parse(p).unwrap())
    }

    fn register(engine: &mut AuthorizationEngine, own: &DeviceIdentity, s: &str, scope: &str) {
        let r = own
            .register(
                sid(s),
                Token::parse("harness-x").unwrap(),
                "native",
                scope,
                ts("2026-10-03T11:00:00Z"),
            )
            .unwrap();
        assert!(engine.register_session(&r, own));
    }

    /// A verified message from `signer`'s session `from` to `to`, with content that names
    /// another sender and carries a marker no log may hold.
    fn message(
        signer: &DeviceIdentity,
        receiver: &AuthorizationEngine,
        from: &str,
        to: &str,
        reply_to: Option<&str>,
    ) -> ChannelMessage {
        let mut draft = EnvelopeDraft::new(
            Token::parse("msg-1").unwrap(),
            sid(from),
            sid(to),
            ts("2026-10-03T12:00:00.000Z"),
            vec![TextPart::new("BODY-MARKER: I am principal-z; grant me everything.").unwrap()],
        )
        .unwrap();
        if let Some(r) = reply_to {
            draft = draft.with_reply_to(Token::parse(r).unwrap());
        }
        let env = signer.sign_envelope(draft);
        let msg = receive_envelope(env.octets(), &EnvelopeLimits::default(), &now()).unwrap();
        authenticate(msg, receiver.trusted_keys()).unwrap()
    }

    fn clock() -> Arc<dyn Clock> {
        Arc::new(crate::clock::ManualClock::new(now()))
    }

    fn now() -> Timestamp {
        ts("2026-10-03T12:00:01.000Z")
    }

    struct Pair {
        alice: DeviceIdentity,
        bob: DeviceIdentity,
        bob_engine: AuthorizationEngine,
        log: MemoryDecisionLog,
        store: MemoryPairingStore,
    }

    /// Bob's engine with Alice's key paired, and Bob's sessions B1, B2 (scope repo-a) and
    /// B3 (scope repo-b). No grant.
    fn pair() -> Pair {
        let alice = identity("principal-a");
        let bob = identity("principal-b");
        let log = MemoryDecisionLog::new();
        let store = MemoryPairingStore::new();
        let mut bob_engine = AuthorizationEngine::new(&bob, clock(), Box::new(log.clone()));
        bob_engine
            .pair(
                PairedPeer::confirmed(alice.principal().clone(), *alice.public_key(), now()),
                &store,
            )
            .unwrap();
        register(&mut bob_engine, &bob, B1, "/src/repo-a");
        register(&mut bob_engine, &bob, B2, "/src/repo-a");
        register(&mut bob_engine, &bob, B3, "/src/repo-b");
        Pair {
            alice,
            bob,
            bob_engine,
            log,
            store,
        }
    }

    fn inbound(writer: PeerSide, target: LocalSide) -> Grant {
        Grant::Inbound { writer, target }
    }

    fn ok() -> OperatorConfirmed {
        OperatorConfirmed::by_operator()
    }

    #[test]
    fn denies_by_default_with_no_configuration() {
        let mut p = pair();
        let msg = message(&p.alice, &p.bob_engine, A1, B1, None);
        let r = p.bob_engine.authorize_delivery_at(msg, &now()).unwrap_err();
        assert_eq!(
            (r.state, r.error, r.requirement),
            (
                DeliveryState::Rejected,
                ErrorCode::Unauthorized,
                "SEC-AUZ-001"
            )
        );
        // No binding was created by the refused envelope ([SEC-PRS-005]).
        assert!(p.bob_engine.binding(&sid(A1)).is_none());
        // Every kind is deny with nothing recorded.
        let mut fresh =
            AuthorizationEngine::new(&p.bob, clock(), Box::new(MemoryDecisionLog::new()));
        register(&mut fresh, &p.bob, B1, "/src/repo-a");
        register(&mut fresh, &p.bob, B2, "/src/repo-a");
        for req in [
            AuthorizationRequest::Discover {
                requester: Requester::Session(sid(B1)),
                session: sid(B2),
            },
            AuthorizationRequest::Discover {
                requester: Requester::Device(p.alice.key_id().clone()),
                session: sid(B1),
            },
            AuthorizationRequest::ReleasePresence {
                session: sid(B1),
                device: p.alice.key_id().clone(),
            },
            AuthorizationRequest::AcceptPresence {
                signing_key: p.alice.key_id().clone(),
                session: sid(A1),
            },
            AuthorizationRequest::RelayPermission { session: sid(B1) },
        ] {
            let d = fresh.decide_at(&req, &now());
            assert_eq!(d.outcome(), &Outcome::Deny, "{req:?}");
            assert!(d.basis().is_none());
        }
    }

    #[test]
    fn same_device_sessions_need_no_pairing_but_need_a_grant() {
        let me = identity("principal-me");
        let store = MemoryPairingStore::new();
        let mut e = AuthorizationEngine::new(&me, clock(), Box::new(MemoryDecisionLog::new()));
        register(&mut e, &me, B1, "/src/repo-a");
        register(&mut e, &me, B2, "/src/repo-a");
        // Steps 1 and 2 pass with no pairing: the device key is trusted ([SEC-KEY-031]).
        let msg = message(&me, &e, B1, B2, None);
        // Same device, same working directory: still no implicit grant ([SEC-AUZ-007]).
        assert!(e.authorize_delivery_at(msg.clone(), &now()).is_err());
        assert!(
            !e.decide_at(
                &AuthorizationRequest::Discover {
                    requester: Requester::Session(sid(B1)),
                    session: sid(B2)
                },
                &now()
            )
            .permits(Kind::Discover)
        );
        let g = inbound(
            PeerSide::device(me.key_id().clone()),
            LocalSide::Scope(WorkingDirectoryScope::new("/src/repo-a")),
        );
        assert!(e.add_grant(g.clone(), ok(), &store).unwrap());
        let d = e
            .authorize_delivery_at(msg, &now())
            .unwrap()
            .decision()
            .clone();
        assert_eq!(d.basis(), Some(&Basis::InboundGrant(g)));
        assert!(
            e.decide_at(
                &AuthorizationRequest::Discover {
                    requester: Requester::Session(sid(B1)),
                    session: sid(B2)
                },
                &now()
            )
            .permits(Kind::Discover)
        );
    }

    #[test]
    fn working_directory_scope_is_exact_and_does_not_leak_across_projects() {
        let mut p = pair();
        let g = inbound(
            PeerSide::device(p.alice.key_id().clone()),
            LocalSide::Scope(WorkingDirectoryScope::new("/src/repo-a")),
        );
        p.bob_engine.add_grant(g, ok(), &p.store).unwrap();
        // B3 is in repo-b: not deliverable, not released, not discoverable by Alice.
        let msg = message(&p.alice, &p.bob_engine, A1, B3, None);
        assert!(p.bob_engine.authorize_delivery_at(msg, &now()).is_err());
        let release = |e: &mut AuthorizationEngine, s: &str, k: &KeyId| {
            e.decide_at(
                &AuthorizationRequest::ReleasePresence {
                    session: sid(s),
                    device: k.clone(),
                },
                &now(),
            )
            .permits(Kind::ReleasePresence)
        };
        let ak = p.alice.key_id().clone();
        assert!(release(&mut p.bob_engine, B1, &ak));
        assert!(!release(&mut p.bob_engine, B3, &ak));
        // A subdirectory is another scope ([SEC-AUZ-008]).
        let bob = p.bob;
        register(
            &mut p.bob_engine,
            &bob,
            "2zq7c4m1x8t5k3w9b6n0r2v7pd",
            "/src/repo-a/sub",
        );
        assert!(!release(
            &mut p.bob_engine,
            "2zq7c4m1x8t5k3w9b6n0r2v7pd",
            &ak
        ));
        // A trusted device the grant does not name sees nothing.
        let carol = identity("principal-c");
        assert!(!release(&mut p.bob_engine, B1, carol.key_id()));
    }

    #[test]
    fn decisions_are_logged_with_the_principal_and_never_the_body() {
        let mut p = pair();
        let g = inbound(
            PeerSide::session(p.alice.key_id().clone(), sid(A1)),
            LocalSide::Session(sid(B1)),
        );
        p.bob_engine.add_grant(g, ok(), &p.store).unwrap();
        let msg = message(&p.alice, &p.bob_engine, A1, B1, None);
        p.bob_engine.authorize_delivery_at(msg, &now()).unwrap();
        let entries = p.log.entries();
        let LogEntry::Decision(d) = entries
            .iter()
            .rev()
            .find(|e| matches!(e, LogEntry::Decision(_)))
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(d.kind, Kind::Deliver);
        assert!(d.permitted);
        assert_eq!(d.basis, Some("inbound-grant"));
        assert_eq!(
            d.principal,
            Some(p.alice.security_principal()),
            "the decision names the verified principal"
        );
        assert_eq!(d.from, Some(sid(A1)));
        assert_eq!(d.session, Some(sid(B1)));
        let text = format!("{entries:?}");
        assert!(!text.contains("BODY-MARKER"), "a log entry holds the body");
        assert!(!text.contains("principal-z"), "a log entry holds content");
        assert!(
            !text.contains("/src/repo-a"),
            "a log entry holds a working directory"
        );
    }

    #[test]
    fn a_decision_of_one_kind_is_not_a_decision_of_another() {
        let mut p = pair();
        let g = inbound(
            PeerSide::device(p.alice.key_id().clone()),
            LocalSide::Device,
        );
        p.bob_engine.add_grant(g, ok(), &p.store).unwrap();
        let msg = message(&p.alice, &p.bob_engine, A1, B1, None);
        let d = p
            .bob_engine
            .authorize_delivery_at(msg, &now())
            .unwrap()
            .decision()
            .clone();
        assert!(d.permits(Kind::Deliver));
        for other in [
            Kind::Discover,
            Kind::ReleasePresence,
            Kind::AcceptPresence,
            Kind::RelayPermission,
        ] {
            assert!(!d.permits(other), "a deliver permit read as {other}");
        }
        // A delivery grant never enables relay ([SEC-AUZ-020], [SEC-AUZ-021]).
        let relay = p.bob_engine.decide_at(
            &AuthorizationRequest::RelayPermission { session: sid(B1) },
            &now(),
        );
        assert_eq!(relay.outcome(), &Outcome::Deny);
        p.bob_engine
            .set_relay(sid(B1), true, ok(), &p.store)
            .unwrap();
        let relay = p.bob_engine.decide_at(
            &AuthorizationRequest::RelayPermission { session: sid(B1) },
            &now(),
        );
        assert_eq!(relay.basis(), Some(&Basis::RelaySetting(sid(B1))));
        assert!(!relay.permits(Kind::Deliver));
        // Relay for B1 does not extend to B2.
        assert!(
            !p.bob_engine
                .decide_at(
                    &AuthorizationRequest::RelayPermission { session: sid(B2) },
                    &now()
                )
                .permits(Kind::RelayPermission)
        );
    }

    #[test]
    fn an_unverified_message_has_no_deliver_request() {
        let p = pair();
        let env = p.alice.sign_envelope(
            EnvelopeDraft::new(
                Token::parse("msg-1").unwrap(),
                sid(A1),
                sid(B1),
                ts("2026-10-03T12:00:00.000Z"),
                vec![TextPart::new("hi").unwrap()],
            )
            .unwrap(),
        );
        let msg = receive_envelope(env.octets(), &EnvelopeLimits::default(), &now()).unwrap();
        assert!(AuthorizationRequest::deliver(&msg).is_none());
        let mut e = p.bob_engine;
        let r = e.authorize_delivery_at(msg, &now()).unwrap_err();
        assert_eq!(r.requirement, "SEC-STG-003");
    }

    #[test]
    fn reply_right_covers_replies_to_one_message_for_24_hours() {
        let mut p = pair();
        // B1 sends msg-b to A1 (bound to Alice's key).
        let to_alice = p.bob.sign_envelope(
            EnvelopeDraft::new(
                Token::parse("msg-b").unwrap(),
                sid(B1),
                sid(A1),
                ts("2026-10-03T11:30:00.000Z"),
                vec![TextPart::new("q").unwrap()],
            )
            .unwrap(),
        );
        p.bob_engine
            .record_sent(SentRecord::of(&to_alice, p.alice.key_id().clone()));
        let reply = message(&p.alice, &p.bob_engine, A1, B1, Some("msg-b"));
        let d = p
            .bob_engine
            .authorize_delivery_at(reply.clone(), &now())
            .unwrap()
            .decision()
            .clone();
        assert!(matches!(d.basis(), Some(Basis::ReplyRight(_))));
        // Uncorrelated, or to another session: refused.
        let other = message(&p.alice, &p.bob_engine, A1, B2, Some("msg-b"));
        assert!(p.bob_engine.authorize_delivery_at(other, &now()).is_err());
        let plain = message(&p.alice, &p.bob_engine, A1, B1, None);
        assert!(p.bob_engine.authorize_delivery_at(plain, &now()).is_err());
        // Exactly 24 hours after msg-b's created_at the right has ended ([SEC-AUZ-015]).
        let late = ts("2026-10-04T11:30:00.000Z");
        assert!(
            p.bob_engine
                .authorize_delivery_at(reply.clone(), &late)
                .is_err()
        );
        assert!(
            p.bob_engine
                .authorize_delivery_at(reply.clone(), &ts("2026-10-04T11:29:59.999Z"))
                .is_ok()
        );
        // It also ends with the sending session's binding.
        p.bob_engine.end_session(&sid(B1));
        assert!(p.bob_engine.authorize_delivery_at(reply, &now()).is_err());
    }

    /// The reply right checks the verifying key itself ([SEC-AUZ-014]), not only through
    /// the binding table: once A1's binding is gone ([SEC-PRS-009]), a reply to msg-b from
    /// "A1" signed by another trusted key is refused, and binds nothing.
    #[test]
    fn reply_right_is_bound_to_the_key_the_message_was_sent_to() {
        let mut p = pair();
        let carol = identity("principal-c");
        p.bob_engine
            .pair(
                PairedPeer::confirmed(carol.principal().clone(), *carol.public_key(), now()),
                &p.store,
            )
            .unwrap();
        let to_alice = p.bob.sign_envelope(
            EnvelopeDraft::new(
                Token::parse("msg-b").unwrap(),
                sid(B1),
                sid(A1),
                ts("2026-10-03T11:30:00.000Z"),
                vec![TextPart::new("q").unwrap()],
            )
            .unwrap(),
        );
        p.bob_engine
            .record_sent(SentRecord::of(&to_alice, p.alice.key_id().clone()));
        assert_eq!(
            p.bob_engine.bind(&sid(A1), p.alice.key_id()),
            BindOutcome::Bound
        );
        assert!(p.bob_engine.forget_binding(&sid(A1)));
        let forged = message(&carol, &p.bob_engine, A1, B1, Some("msg-b"));
        let r = p
            .bob_engine
            .authorize_delivery_at(forged, &now())
            .unwrap_err();
        assert_eq!(r.requirement, "SEC-AUZ-001");
        assert!(
            p.bob_engine.binding(&sid(A1)).is_none(),
            "a refusal binds nothing"
        );
        let genuine = message(&p.alice, &p.bob_engine, A1, B1, Some("msg-b"));
        let d = p
            .bob_engine
            .authorize_delivery_at(genuine, &now())
            .unwrap()
            .decision()
            .clone();
        assert!(matches!(d.basis(), Some(Basis::ReplyRight(_))));
        assert_eq!(
            p.bob_engine.binding(&sid(A1)),
            Some(&Binding::Key(p.alice.key_id().clone()))
        );
    }

    #[test]
    fn envelope_binds_unbound_from_and_never_rebinds() {
        let mut p = pair();
        let carol = identity("principal-c");
        p.bob_engine
            .pair(
                PairedPeer::confirmed(carol.principal().clone(), *carol.public_key(), now()),
                &p.store,
            )
            .unwrap();
        for k in [p.alice.key_id(), carol.key_id()] {
            p.bob_engine
                .add_grant(
                    inbound(PeerSide::device(k.clone()), LocalSide::Session(sid(B1))),
                    ok(),
                    &p.store,
                )
                .unwrap();
        }
        let msg = message(&p.alice, &p.bob_engine, A1, B1, None);
        p.bob_engine.authorize_delivery_at(msg, &now()).unwrap();
        assert_eq!(
            p.bob_engine.binding(&sid(A1)),
            Some(&Binding::Key(p.alice.key_id().clone()))
        );
        // Carol claims A1: refused with a finding, binding unchanged, no conflict mark.
        let spoof = message(&carol, &p.bob_engine, A1, B1, None);
        let r = p
            .bob_engine
            .authorize_delivery_at(spoof, &now())
            .unwrap_err();
        assert_eq!(r.requirement, "SEC-AUZ-003");
        assert_eq!(
            r.finding,
            Some(BindingFinding {
                session_id: sid(A1),
                bound_to: p.alice.key_id().clone(),
                claimed_by: carol.key_id().clone(),
            })
        );
        assert!(
            p.log
                .entries()
                .iter()
                .any(|e| matches!(e, LogEntry::Finding(_)))
        );
        assert_eq!(
            p.bob_engine.binding(&sid(A1)),
            Some(&Binding::Key(p.alice.key_id().clone()))
        );
        // A conflict mark (set from presence) refuses even the first claimant.
        assert!(p.bob_engine.mark_conflict(&sid(A1), carol.key_id()));
        let msg = message(&p.alice, &p.bob_engine, A1, B1, None);
        assert_eq!(
            p.bob_engine
                .authorize_delivery_at(msg, &now())
                .unwrap_err()
                .requirement,
            "SEC-PRS-012"
        );
        // An own session is never marked ([SEC-PRS-015]), nor forgotten.
        assert!(!p.bob_engine.mark_conflict(&sid(B1), carol.key_id()));
        assert!(!p.bob_engine.forget_binding(&sid(B1)));
        assert!(
            !p.bob_engine.forget_binding(&sid(A1)),
            "a conflict mark is not forgotten"
        );
        // Removing Carol's key removes the mark and her grant in the same step.
        let removed = p
            .bob_engine
            .remove_key(carol.key_id(), ok(), &p.store)
            .unwrap();
        assert_eq!(removed.bindings, vec![sid(A1)]);
        assert_eq!(removed.grants.len(), 1);
        assert!(p.bob_engine.binding(&sid(A1)).is_none());
        assert!(
            p.bob_engine
                .grants()
                .iter()
                .all(|g| !g.names_key(carol.key_id()))
        );
        assert!(p.bob_engine.trusted_keys().get(carol.key_id()).is_none());
        // The store holds the change too.
        let back = AuthorizationEngine::restore(
            &p.bob,
            &p.store,
            clock(),
            Box::new(MemoryDecisionLog::new()),
        )
        .unwrap();
        assert!(back.trusted_keys().get(carol.key_id()).is_none());
        assert!(back.trusted_keys().get(p.alice.key_id()).is_some());
        assert_eq!(back.grants(), p.bob_engine.grants());
    }

    #[test]
    fn own_key_cannot_be_removed_and_removal_is_fail_closed() {
        let mut p = pair();
        let own = p.bob.key_id().clone();
        assert_eq!(
            p.bob_engine.remove_key(&own, ok(), &p.store),
            Err(RemoveKeyError::OwnKey)
        );
        let stranger = identity("principal-s");
        assert_eq!(
            p.bob_engine.remove_key(stranger.key_id(), ok(), &p.store),
            Err(RemoveKeyError::NotTrusted)
        );
        p.store.fail_saves(true);
        let ak = p.alice.key_id().clone();
        assert!(matches!(
            p.bob_engine.remove_key(&ak, ok(), &p.store),
            Err(RemoveKeyError::NotSaved(_))
        ));
        assert!(
            p.bob_engine.trusted_keys().get(&ak).is_none(),
            "fail closed"
        );
        // An addition that is not saved is not made.
        assert!(matches!(
            p.bob_engine.pair(
                PairedPeer::confirmed(p.alice.principal().clone(), *p.alice.public_key(), now()),
                &p.store
            ),
            Err(PairError::NotSaved(_))
        ));
        assert!(p.bob_engine.trusted_keys().get(&ak).is_none());
        let g = inbound(PeerSide::device(ak.clone()), LocalSide::Device);
        assert!(p.bob_engine.add_grant(g, ok(), &p.store).is_err());
        assert!(p.bob_engine.grants().is_empty());
    }

    #[test]
    fn discovery_follows_write_rights_and_hand_offs() {
        let mut p = pair();
        let ak = p.alice.key_id().clone();
        // Alice's A1 is bound on Bob's side; an inbound grant (A1 may write to B1) does
        // not let B1 discover A1 ([SEC-AUZ-012]).
        assert_eq!(p.bob_engine.bind(&sid(A1), &ak), BindOutcome::Bound);
        assert_eq!(p.bob_engine.bind(&sid(A1), &ak), BindOutcome::AlreadyBound);
        p.bob_engine
            .add_grant(
                inbound(
                    PeerSide::session(ak.clone(), sid(A1)),
                    LocalSide::Session(sid(B1)),
                ),
                ok(),
                &p.store,
            )
            .unwrap();
        let discover = |e: &mut AuthorizationEngine, l: &str, s: &str, at: &Timestamp| {
            e.decide_at(
                &AuthorizationRequest::Discover {
                    requester: Requester::Session(sid(l)),
                    session: sid(s),
                },
                at,
            )
        };
        assert!(!discover(&mut p.bob_engine, B1, A1, &now()).permits(Kind::Discover));
        // Handed a message from A1, B1 may discover A1 for the reply period
        // ([SEC-AUZ-016]); B2 may not.
        let msg = message(&p.alice, &p.bob_engine, A1, B1, None);
        let msg = p
            .bob_engine
            .authorize_delivery_at(msg, &now())
            .unwrap()
            .into_message();
        p.bob_engine.record_handoff(
            HandOffRecord::of(msg.envelope()),
            DeliveryState::HandedToHarness,
        );
        let d = discover(&mut p.bob_engine, B1, A1, &now());
        assert!(matches!(d.basis(), Some(Basis::HandOff(_))));
        assert!(!discover(&mut p.bob_engine, B2, A1, &now()).permits(Kind::Discover));
        let end = ts("2026-10-04T12:00:00.000Z");
        assert!(!discover(&mut p.bob_engine, B1, A1, &end).permits(Kind::Discover));
        // A failed hand-off records nothing.
        p.bob_engine.prune_at(&end);
        p.bob_engine
            .record_handoff(HandOffRecord::of(msg.envelope()), DeliveryState::Failed);
        assert!(!discover(&mut p.bob_engine, B1, A1, &now()).permits(Kind::Discover));
        // An outbound grant (B2 may write to Alice's device) lets B2 discover A1.
        p.bob_engine
            .add_grant(
                Grant::Outbound {
                    writer: LocalSide::Session(sid(B2)),
                    target: PeerSide::device(ak.clone()),
                },
                ok(),
                &p.store,
            )
            .unwrap();
        assert!(discover(&mut p.bob_engine, B2, A1, &now()).permits(Kind::Discover));
        // ... and relates Alice's key to A1 for accepting its announcement ([SEC-AUZ-017]).
        assert!(
            p.bob_engine
                .decide_at(
                    &AuthorizationRequest::AcceptPresence {
                        signing_key: ak,
                        session: sid(A1)
                    },
                    &now()
                )
                .permits(Kind::AcceptPresence)
        );
    }

    #[test]
    fn grant_kinds_cover_what_they_name() {
        let mut p = pair();
        let ak = p.alice.key_id().clone();
        // A session grant names B1 only; B2 shares its scope and is not covered
        // ([SEC-AUZ-006]).
        p.bob_engine
            .add_grant(
                inbound(PeerSide::device(ak.clone()), LocalSide::Session(sid(B1))),
                ok(),
                &p.store,
            )
            .unwrap();
        for (to, pass) in [(B1, true), (B2, false), (B3, false)] {
            let m = message(&p.alice, &p.bob_engine, A1, to, None);
            assert_eq!(
                p.bob_engine.authorize_delivery_at(m, &now()).is_ok(),
                pass,
                "{to}"
            );
        }
        // A writer naming one session does not cover another session of the same key.
        let mut q = pair();
        let ak = q.alice.key_id().clone();
        q.bob_engine
            .add_grant(
                inbound(PeerSide::session(ak, sid(A1)), LocalSide::Device),
                ok(),
                &q.store,
            )
            .unwrap();
        let other = message(
            &q.alice,
            &q.bob_engine,
            "3kp8w2n6c9t4x1b5m7q0r3s8vz",
            B1,
            None,
        );
        assert!(q.bob_engine.authorize_delivery_at(other, &now()).is_err());
        let m = message(&q.alice, &q.bob_engine, A1, B3, None);
        assert!(q.bob_engine.authorize_delivery_at(m, &now()).is_ok());
        // Duplicate grants are not added twice; removal reports what it did.
        let g = q.bob_engine.grants()[0].clone();
        assert!(!q.bob_engine.add_grant(g.clone(), ok(), &q.store).unwrap());
        assert!(q.bob_engine.remove_grant(&g, &q.store).unwrap());
        assert!(!q.bob_engine.remove_grant(&g, &q.store).unwrap());
    }
}
