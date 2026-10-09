// SPDX-License-Identifier: Apache-2.0

//! The send and receive pipelines (#313): the composition of the envelope, signing, replay,
//! authorization, presence and receipt stages (F2 to F7) in the order of
//! `spec/session-channels.md` §8.3, between provider adapters (`spec/interfaces.md` §5) and
//! one transport (§6).
//!
//! # The only path between adapters and transports
//!
//! [`Pipelines`] holds the transport and every adapter. An adapter's harness requests reach
//! the core through the request sink it is given ([`crate::adapter::RequestSink`]), and a
//! hand-off reaches the adapter through `deliver`; a transport's payloads reach the core
//! through the handlers given to `subscribe` and `watch_presence`. No adapter is ever given
//! the transport, and no transport is ever given an adapter, so they reach each other only
//! through the stages below ([IFC-ADP-001]).
//!
//! # Send path
//!
//! For a `SendRequest` from an attachment, [`crate::adapter::RequestSink::send`]:
//!
//! 1. attribution: the session the attachment is bound to, and only when the request comes
//!    from the adapter that reported the attachment ([SC-ID-160], [IFC-ADP-031]);
//! 2. the send decision and the envelope ([`crate::sender::prepare_send`]): presence with
//!    discovery authorization folded in, version agreement, `active_inbound`, part types,
//!    then the size of the envelope itself, built under the agreed revision with reply
//!    headers read from the engine's hand-off records ([SC-RCP-050] to [SC-RCP-054]) and
//!    signed with the device key;
//! 3. the sender's own announcement to the recipient's device, issued before the envelope
//!    is passed to the transport, when the release is authorized ([SEC-PRS-010],
//!    [SEC-AUZ-011]) and the transport may carry it ([IFC-TRN-081]). An announcement
//!    counts as issued once the transport took it; when it does not take this send's
//!    announcement, the envelope is not passed either (`not-passed`, `transport-failure`),
//!    and a send that finds another thread's announcement not yet taken issues its own;
//! 4. the sent record ([SEC-AUZ-013]), so the reply right and the receipt check
//!    ([SEC-RCT-003]) hold before any reply or receipt can arrive;
//! 5. `publish`;
//! 6. the envelope's [`EnvelopeTracker`], which every receipt for it then updates.
//!
//! The result is one of the three of `spec/interfaces.md` §4.10: `refused` with a
//! `request`-scoped code, `not-passed` (`failed` with `transport-failure` or
//! `internal-error`), or `sent` (`accepted-by-adapter`) with the stream of receipts.
//!
//! # Receive path
//!
//! For each payload a session subscription hands over:
//!
//! 1. envelope-stage validation ([`crate::envelope::receive_envelope`]);
//! 2. security steps 1 to 4, under the engine ([`crate::receiver::receive`]'s order);
//! 3. security step 5 and the delivery stage, with the hand-off call made through the
//!    attachment's adapter (`ProviderAdapter::deliver`) and no lock held across it;
//! 4. the hand-off record ([SEC-AUZ-016]), kept from just before the call so that a reply
//!    made during it correlates, and removed again when the call does not hand off;
//! 5. the receipt gate ([`crate::receiver::may_send_receipt`]);
//! 6. the receipt: for an envelope this device signed, the receiver-observed state goes
//!    straight to its tracker ([SC-RCP-040]); otherwise an authenticated receipt is issued
//!    and published to the device whose key verified the envelope ([SEC-RCT-001]).
//!
//! A copy whose twin is being handed off comes back in flight. It is re-queued, not parked:
//! [`DuplicateStore::when_settled`] puts it back on a bounded queue when the twin settles,
//! and the thread that settled it offers it again through the receiver's re-offer path once
//! its own delivery is done ([SEC-RPL-026]). Nothing waits on a timer and nothing asks a
//! transport or an adapter for anything ([IFC-TRN-041]; no polling).
//!
//! # Bounds
//!
//! Every collection here is bounded ([`PipelineConfig`]): trackers, receipt streams, the
//! announcement table, connections and the re-queue. The engine's records, the duplicate
//! store, the presence registry and the receipt limiter bound themselves. No envelope is
//! held for later delivery: a copy is handed off now or refused now ([SC-DLV-007]).
//!
//! # Binding from native signals
//!
//! Each `native-signal` event an adapter reports goes through
//! `spec/session-channels.md` §6.7 here, so no adapter decides a binding (#331):
//!
//! 1. pairing ([`crate::session_binding`], §6.7.2): the key is the one the core process
//!    observed for the connection the signal arrived on ([`Pipelines::connect_observed`]),
//!    and the candidates are the open attachments of the same adapter observed with an
//!    equal key. No key, or more than one candidate, is unpairable; no candidate yet holds
//!    the signal for [`PipelineConfig::native_signal_window`], and an attachment opened
//!    within it pairs;
//! 2. the ordered cases of §6.7.3 and the stale-binding rule of §6.7.4
//!    ([`crate::session_binding::decide`]), against this adapter's attachments;
//! 3. the change: a bind registers N under a fresh session id with a record this device
//!    signs, through the same steps as [`Pipelines::bind`], after deregistering an earlier
//!    binding ([SC-ID-150]); a stale binding is deregistered ([SC-ID-152]);
//! 4. the records, to the [`crate::session_binding::BindingLog`]
//!    ([`Pipelines::set_binding_log`]).
//!
//! An unpairable or dropped signal that the observed key attributes to bound attachments
//! withholds delivery to them and their send requests until a signal reported after it
//! pairs with one ([SC-ID-154]): a decision already running for an earlier signal neither
//! releases the attachment nor binds it delivering, and nor does [`Pipelines::bind`].
//!
//! Signals are decided one at a time by whichever thread holds the binding turn; a signal
//! reported, or an attachment opened, while the turn is held makes the holder pass again.
//! Queued signals are decided in arrival order, but a held signal is decided when a
//! candidate opens, so a newer signal of a key can pair while an older one of that key is
//! still held (its candidate opened in the middle of a pass). Each signal carries its
//! place in arrival order, and a held signal's window ends when a newer signal of its key
//! (under its adapter) pairs ([SC-ID-123]): it is dropped before it pairs, with its
//! diagnostic ([SC-ID-124], [SC-ID-128]), binding nothing and withholding nothing. So an
//! older signal can never bind the key's attachment, nor a new one after a reconnect,
//! back to a conversation the harness has left (#338).
//!
//! The held and queued signals together are bounded by
//! [`PipelineConfig::max_pending_signals`], shared fairly between the observed keys (and
//! the connections of signals without one), so that one key cannot evict another's
//! signals by flooding (#335). A held signal's window is checked on every adapter event
//! and when the owner's timer calls [`Pipelines::expire_native_signals`]; nothing polls.
//!
//! The daemon calls [`Pipelines::disconnect`] when it observes a local connection end, so
//! that connections which only carry native signals, and never become attachments, free
//! their place ([`PipelineConfig::max_connections`]); for an attachment it is also the end
//! of its binding ([SC-ID-155]).
//!
//! *UNVERIFIED (§6.7.2, dated note of 2026-10-03):* no operating-system facility for the
//! pairing key is established yet (G9, #70). A connection given through
//! [`Pipelines::connect`] has none, so until one is, every native signal fails closed with
//! a finding ([SC-ID-125], [SC-ID-129]).
//!
//! # What is not here
//!
//! Accepting and authenticating local connections, observing their peers, and observing
//! them end, is the daemon's (G9); it passes each to [`Pipelines::connect_observed`] and
//! reports its end to [`Pipelines::disconnect`].

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

use crate::adapter::{
    AdapterEvent, Attachment, Connection, ConnectionHandle, DiscoveryRequest,
    DiscoveryRequestResult, HandOff, HandOffOutcome, NativeSignal, ProviderAdapter, ReceiptStream,
    RequestSink, SendRequest, SendRequestResult,
};
use crate::authorization::{
    AuthorizationEngine, AuthorizationRequest, AuthorizedMessage, Binding, HandOffRecord, Kind,
    Requester, SentRecord,
};
use crate::capabilities::{CapabilitiesEntry, Implemented, SessionCapabilities, SessionDescriptor};
use crate::clock::Clock;
use crate::delivery::{DeliveryState, ErrorCode};
use crate::envelope::{ChannelMessage, Envelope, EnvelopeLimits, receive_envelope};
use crate::ids::{EXTENSION_ID_V0, IMPLEMENTED_VERSION, KeyId, SessionId, Token};
use crate::json;
use crate::keys::DeviceIdentity;
use crate::presence_auth::{AuthenticatedPresenceRecord, accept_authenticated_record};
use crate::receipt::DeliveryReceipt;
use crate::receipt_auth::{AuthenticatedReceipt, accept_receipt};
use crate::receiver::{
    DeliveryTarget, ReceiptLimiter, ReceiveOutcome, Received, ReceiverReport, deliver_unrecorded,
    may_send_receipt, security_steps,
};
use crate::registration::RegistrationRecord;
use crate::registry::{CROSS_IMPLEMENTATION_PRESENCE_CAP_MS, PresenceIssuer, PresenceRegistry};
use crate::replay::{DuplicateStore, HandOffDeadline};
use crate::sender::{EnvelopeTracker, ObservedReceipt, prepare_send};
use crate::session_binding::{
    AttachmentState, BindingAction, BindingDecision, BindingLog, BindingLogEntry, BindingRecord,
    BindingResult, MemoryBindingLog, NativeBinding, Pairing, PairingKey, PeerObservation,
    RecordKind, decide, fresh_session_id,
};
use crate::transport::{
    Deadline, Destination, Inbound, Payload, PayloadKind, PresenceEvent, PublishResult, Reach,
    Subscription, Transport, TransportCapabilities, TransportConfiguration, TransportError,
};

/// The monotonic clock the pipelines read for transport deadlines, presence lifetimes and
/// the receipt rate limit: the clock of the transport's `Deadline` (`spec/interfaces.md`
/// §4.11).
pub type MonotonicClock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// The bounds and settings of [`Pipelines`].
#[derive(Clone, Debug)]
pub struct PipelineConfig {
    /// The implemented list (§6.10).
    pub implemented: Vec<Implemented>,
    /// The receiver-wide envelope-stage limits ([SC-RCP-076]).
    pub limits: EnvelopeLimits,
    /// The `lifetime_ms` of the announcements issued, at most
    /// [`CROSS_IMPLEMENTATION_PRESENCE_CAP_MS`] for other implementations ([SEC-PRS-008]).
    pub presence_lifetime_ms: u64,
    /// The most envelopes tracked at once; the oldest is forgotten past it, which ends its
    /// receipt stream.
    pub max_trackers: usize,
    /// The receipts each receipt stream holds unread; a receipt past it is not queued (the
    /// tracker still records it).
    pub receipt_stream_capacity: usize,
    /// The most (session, device) pairs whose announcement is remembered.
    pub max_announcements: usize,
    /// The most local connections taken at once.
    pub max_connections: usize,
    /// The most in-flight copies waiting to be offered again ([SEC-RPL-026]). A copy past it
    /// is reported `failed` with `internal-error` and may be retransmitted.
    pub max_requeued: usize,
    /// How long a receipt or an announcement may stay in flight in the transport.
    pub control_ttl: Duration,
    /// How long a native signal that is not yet pairable is held for an attachment to pair
    /// with ([SC-ID-123]); `spec/session-channels.md` §6.7.2 leaves the length to the
    /// implementation.
    pub native_signal_window: Duration,
    /// The most native signals held or waiting for their decision at once, at least 1.
    ///
    /// Each signal counts against its *holder*: the pairing key observed for its
    /// connection (under the adapter that reported it), else that connection, else the one
    /// holder of signals with neither. At the cap, a new signal from a holder with `n`
    /// pending signals makes room (#335):
    ///
    /// - **Fair share.** When a holder has at least `n + 2`, the most of any, its oldest
    ///   signal goes; among the holders with the most, the one whose latest signal arrived
    ///   last gives it up. No holder can steer the eviction onto another by its key or
    ///   connection identity; the timing of its own signals can decide a tie, which still
    ///   leaves the one that pays with `n + 1`.
    /// - **Own share.** Otherwise, when `n > 0`, the holder's own oldest signal goes.
    /// - **Refused.** Otherwise (`n == 0`) the new signal goes.
    ///
    /// So a holder is never brought below the number the newcomer then holds, and a
    /// flooding holder only ever evicts its own signals once it is the heaviest. The
    /// signal that goes is dropped with a diagnostic ([SC-ID-123], [SC-ID-128]),
    /// withholding any bound attachment its observed key attributes it to ([SC-ID-154]).
    pub max_pending_signals: usize,
}

impl Default for PipelineConfig {
    fn default() -> PipelineConfig {
        PipelineConfig {
            implemented: vec![Implemented {
                extension: EXTENSION_ID_V0.to_owned(),
                major: IMPLEMENTED_VERSION.major,
                revision: IMPLEMENTED_VERSION,
            }],
            limits: EnvelopeLimits::default(),
            presence_lifetime_ms: CROSS_IMPLEMENTATION_PRESENCE_CAP_MS,
            max_trackers: 4096,
            receipt_stream_capacity: 16,
            max_announcements: 4096,
            max_connections: 1024,
            max_requeued: 1024,
            control_ttl: Duration::from_secs(60),
            native_signal_window: Duration::from_secs(10),
            max_pending_signals: 64,
        }
    }
}

/// Which adapter of a [`Pipelines`]: the value [`Pipelines::add_adapter`] returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AdapterId(usize);

/// Why a [`Pipelines`] operation was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PipelineError {
    /// The transport refused `start` or `subscribe`.
    Transport(TransportError),
    /// The transport's declaration breaks `spec/interfaces.md` §6.3 on its face; the ids.
    Declaration(Vec<&'static str>),
    /// No adapter with that id.
    UnknownAdapter,
    /// [`PipelineConfig::max_connections`] connections are taken.
    TooManyConnections,
    /// The attachment is not open, or was reported by no adapter it was given to.
    UnknownAttachment,
    /// The attachment is already bound, or the session id already has an attachment.
    AlreadyBound,
    /// The registration record does not verify under this device's key ([SEC-KEY-043]).
    InvalidRegistration,
    /// The adapter's capabilities give no valid declaration ([SC-ID-063], [SC-ID-066]).
    InvalidCapabilities,
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PipelineError::Transport(e) => write!(f, "transport: {e}"),
            PipelineError::Declaration(ids) => {
                write!(f, "transport declaration breaks {}", ids.join(", "))
            }
            PipelineError::UnknownAdapter => f.write_str("unknown adapter"),
            PipelineError::TooManyConnections => f.write_str("too many connections"),
            PipelineError::UnknownAttachment => f.write_str("unknown attachment"),
            PipelineError::AlreadyBound => f.write_str("already bound"),
            PipelineError::InvalidRegistration => f.write_str("invalid registration record"),
            PipelineError::InvalidCapabilities => f.write_str("invalid adapter capabilities"),
        }
    }
}

impl std::error::Error for PipelineError {}

struct AttachmentEntry {
    adapter: AdapterId,
    open: bool,
    session: Option<SessionId>,
    /// The N of the binding, with `session` (§6.7).
    native_id: Option<String>,
    /// The cross-check value `attachment-opened` reported. Used only by the binding
    /// decision ([SC-ID-126]).
    cross_check: Option<String>,
    /// Delivery and send requests withheld under [SC-ID-154] until a later signal pairs
    /// with it (see `withheld_by`).
    withheld: bool,
    /// The arrival `seq` of the latest dropped or unpairable native signal attributed to
    /// it that no later signal has paired with yet. Only a signal that arrived after it
    /// ends the withholding ([SC-ID-154]: "a later signal"), so a decision already running
    /// for an earlier signal neither releases it nor binds it delivering, and a drop that
    /// lands while the attachment is between bindings is still answered (PR #337).
    withheld_by: Option<u64>,
    /// The arrival `seq` of the latest native signal that paired with it and was decided.
    /// A drop of an older signal, applied late (taken for eviction on another thread
    /// before that pairing, withheld after it), is answered already and withholds nothing
    /// ([SC-ID-154]; PR #339 review). Kept per attachment, as `withhold` acts on the
    /// attachment: a new attachment after a reconnect has nothing to withhold.
    paired_seq: Option<u64>,
}

/// A native signal waiting for its decision, with the pairing key observed for its
/// connection when it was reported (`None` when there was none).
struct QueuedSignal {
    adapter: AdapterId,
    signal: NativeSignal,
    key: Option<PairingKey>,
    /// Its place in arrival order, among every native signal reported.
    seq: u64,
}

impl QueuedSignal {
    fn holder(&self) -> SignalHolder {
        SignalHolder::of(
            self.adapter,
            self.key.as_ref(),
            self.signal.connection.as_ref(),
        )
    }
}

/// A native signal that is not yet pairable, held for the window ([SC-ID-123]).
struct HeldSignal {
    adapter: AdapterId,
    signal: NativeSignal,
    key: PairingKey,
    held_at: Instant,
    /// Its [`QueuedSignal::seq`].
    seq: u64,
}

/// Whose share of [`PipelineConfig::max_pending_signals`] a pending native signal counts
/// against (#335).
#[derive(Clone, PartialEq, Eq, Hash)]
enum SignalHolder {
    /// The pairing key observed for its connection when it was reported, under the
    /// adapter that reported it: the scope a key pairs in ([`Inner::pair_by_key`]), so two
    /// adapters observing the same process do not share one share (PR #337 review, N5).
    Key(AdapterId, PairingKey),
    /// The connection it arrived on, which had no observed key. A handle is never issued
    /// twice ([IFC-ADP-013]), so it needs no adapter.
    Connection(ConnectionHandle),
    /// Neither: such signals share one holder.
    Unattributed,
}

impl SignalHolder {
    fn of(
        adapter: AdapterId,
        key: Option<&PairingKey>,
        connection: Option<&ConnectionHandle>,
    ) -> SignalHolder {
        match (key, connection) {
            (Some(k), _) => SignalHolder::Key(adapter, k.clone()),
            (None, Some(c)) => SignalHolder::Connection(c.clone()),
            (None, None) => SignalHolder::Unattributed,
        }
    }
}

/// How a native signal pairs, at one moment (§6.7.2).
enum PairAttempt {
    Paired(Attachment),
    /// No candidate yet, under this key.
    NotYet(PairingKey),
    /// Unpairable; the bound attachments it can be attributed to, for [SC-ID-154].
    Unpairable(Vec<Attachment>),
}

struct TrackerEntry {
    tracker: EnvelopeTracker,
    receipts: SyncSender<DeliveryReceipt>,
}

type TrackerKey = (Token, SessionId);

/// An announcement issued to one device: when, and whether the transport took it. Until it
/// is taken, every send to that device issues its own ([SEC-PRS-010]), so no envelope can
/// overtake the only announcement on another thread.
#[derive(Clone, Copy, Debug)]
struct Announced {
    at: Instant,
    taken: bool,
}

/// Everything the pipelines change, under one lock. The lock is never held across a call
/// into an adapter or a transport.
struct Core {
    engine: AuthorizationEngine,
    registry: PresenceRegistry,
    issuer: PresenceIssuer,
    limiter: ReceiptLimiter,
    adapters: Vec<Arc<dyn ProviderAdapter>>,
    connections: HashMap<Attachment, AdapterId>,
    attachments: HashMap<Attachment, AttachmentEntry>,
    sessions: HashMap<SessionId, Attachment>,
    subscriptions: HashMap<SessionId, Subscription>,
    trackers: HashMap<TrackerKey, TrackerEntry>,
    tracker_order: VecDeque<TrackerKey>,
    /// The last announcement of each own session issued to each device.
    announced: HashMap<(SessionId, KeyId), Announced>,
    transport_caps: Option<TransportCapabilities>,
    device_subscription: Option<Subscription>,
    /// What the core process observed about each connection's peer ([IFC-ADP-012]); only
    /// connections given with an observation have an entry.
    observed: HashMap<Attachment, PeerObservation>,
    /// Native signals waiting for their decision, in arrival order.
    signals: VecDeque<QueuedSignal>,
    /// Native signals held as not yet pairable, in arrival order.
    held: VecDeque<HeldSignal>,
    /// The `seq` the next native signal reported takes.
    next_signal_seq: u64,
}

struct Inner {
    identity: DeviceIdentity,
    clock: Arc<dyn Clock>,
    monotonic: MonotonicClock,
    transport: Arc<dyn Transport>,
    store: DuplicateStore,
    config: PipelineConfig,
    core: Mutex<Core>,
    /// In-flight copies whose twin has settled, to offer again; shared with the settle
    /// callbacks the duplicate store runs.
    requeued: Arc<Mutex<VecDeque<AuthorizedMessage>>>,
    /// Copies waiting on a twin or on the re-queue, for [`PipelineConfig::max_requeued`].
    waiting: AtomicUsize,
    /// Held by the one thread deciding bindings, so that native signals and binds are
    /// decided one at a time against the state the previous one left (§6.7.3). A thread
    /// that finds it taken leaves its signal on the queue for the holder.
    binding_turn: Mutex<()>,
    /// Set when an attachment opens, so a held signal may now have a candidate. A holder of
    /// the binding turn clears it at the start of a pass and passes again while it is set.
    repair_needed: AtomicBool,
    /// Where the findings and diagnostics of §6.7 go.
    binding_log: Mutex<Box<dyn BindingLog>>,
}

/// The send and receive pipelines of one device (see the module documentation). Cloning
/// shares them.
#[derive(Clone)]
pub struct Pipelines {
    inner: Arc<Inner>,
}

impl fmt::Debug for Pipelines {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pipelines")
            .field("device", self.inner.identity.key_id())
            .finish_non_exhaustive()
    }
}

impl Pipelines {
    /// Pipelines for the device `identity`, over `transport`, deciding with `engine`.
    ///
    /// `clock` must be the clock `engine` was built with, so that every check of one copy
    /// reads one clock ([`crate::receiver::receive`]); the duplicate store is built on it.
    /// `monotonic` is the clock the transport's deadlines are read on.
    pub fn new(
        identity: DeviceIdentity,
        engine: AuthorizationEngine,
        clock: Arc<dyn Clock>,
        monotonic: MonotonicClock,
        transport: Arc<dyn Transport>,
        config: PipelineConfig,
    ) -> Pipelines {
        let issuer = PresenceIssuer::new(config.presence_lifetime_ms).unwrap_or_else(|| {
            PresenceIssuer::new(CROSS_IMPLEMENTATION_PRESENCE_CAP_MS)
                .expect("the cap is a valid lifetime")
        });
        let core = Core {
            engine,
            registry: PresenceRegistry::new(),
            issuer,
            limiter: ReceiptLimiter::default(),
            adapters: Vec::new(),
            connections: HashMap::new(),
            attachments: HashMap::new(),
            sessions: HashMap::new(),
            subscriptions: HashMap::new(),
            trackers: HashMap::new(),
            tracker_order: VecDeque::new(),
            announced: HashMap::new(),
            transport_caps: None,
            device_subscription: None,
            observed: HashMap::new(),
            signals: VecDeque::new(),
            held: VecDeque::new(),
            next_signal_seq: 0,
        };
        Pipelines {
            inner: Arc::new(Inner {
                identity,
                store: DuplicateStore::new(clock.clone()),
                clock,
                monotonic,
                transport,
                config,
                core: Mutex::new(core),
                requeued: Arc::default(),
                waiting: AtomicUsize::new(0),
                binding_turn: Mutex::new(()),
                repair_needed: AtomicBool::new(false),
                binding_log: Mutex::new(Box::new(MemoryBindingLog::new(1024))),
            }),
        }
    }

    /// Starts the transport for this device and subscribes to what it carries for it:
    /// receipts addressed to this device, and presence records ([IFC-TRN-040],
    /// [IFC-TRN-060]).
    ///
    /// # Errors
    ///
    /// [`PipelineError::Transport`] from the transport, or [`PipelineError::Declaration`]
    /// for a declaration that breaks §6.3 on its face.
    pub fn start(
        &self,
        configuration: TransportConfiguration,
    ) -> Result<TransportCapabilities, PipelineError> {
        let inner = &self.inner;
        let own = inner.identity.key_id().clone();
        let caps = inner
            .transport
            .start(&own, configuration)
            .map_err(PipelineError::Transport)?;
        let violations = caps.contract_violations();
        if !violations.is_empty() {
            inner.transport.shutdown();
            return Err(PipelineError::Declaration(violations));
        }
        let weak = Arc::downgrade(inner);
        let sub = inner
            .transport
            .subscribe(
                &Destination::Device(own),
                Arc::new(move |i: Inbound| {
                    if let Some(inner) = weak.upgrade() {
                        inner.on_device_payload(i);
                    }
                }),
            )
            .map_err(PipelineError::Transport)?;
        let weak = Arc::downgrade(inner);
        inner
            .transport
            .watch_presence(Arc::new(move |e: PresenceEvent| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_presence(e);
                }
            }))
            .map_err(PipelineError::Transport)?;
        let mut core = inner.lock();
        core.transport_caps = Some(caps);
        core.device_subscription = Some(sub);
        Ok(caps)
    }

    /// Adds a provider adapter: the core watches its attachments and is its request sink
    /// (`spec/interfaces.md` Table 5.2).
    pub fn add_adapter(&self, adapter: Arc<dyn ProviderAdapter>) -> AdapterId {
        let id = {
            let mut core = self.inner.lock();
            core.adapters.push(adapter.clone());
            AdapterId(core.adapters.len() - 1)
        };
        let weak = Arc::downgrade(&self.inner);
        adapter.watch_attachments(Arc::new(move |e: AdapterEvent| {
            if let Some(inner) = weak.upgrade() {
                inner.on_adapter_event(id, e);
            }
        }));
        adapter.accept_requests(Arc::new(Sink {
            inner: Arc::downgrade(&self.inner),
            adapter: id,
        }));
        id
    }

    /// Gives `connection`, a local connection the daemon accepted and authenticated
    /// ([IFC-ADP-012]), to `adapter`, with nothing observed about its peer. Only a handle
    /// given to an adapter here can become one of its attachments ([IFC-ADP-013]).
    ///
    /// A connection given this way has no pairing key, so no native signal pairs through
    /// it ([SC-ID-125]); see [`Pipelines::connect_observed`].
    ///
    /// # Errors
    ///
    /// [`PipelineError::UnknownAdapter`], or [`PipelineError::TooManyConnections`].
    pub fn connect(&self, adapter: AdapterId, connection: Connection) -> Result<(), PipelineError> {
        self.connect_observed(adapter, connection, PeerObservation::none())
    }

    /// [`Pipelines::connect`], with what the core process observed about the connection's
    /// peer when it accepted and authenticated it: the pairing key of [SC-ID-121] and the
    /// scope of a registration record made for it ([`crate::session_binding`]). The
    /// observation is the core process's own, never a value the peer sent.
    ///
    /// # Errors
    ///
    /// [`PipelineError::UnknownAdapter`], or [`PipelineError::TooManyConnections`].
    pub fn connect_observed(
        &self,
        adapter: AdapterId,
        connection: Connection,
        observation: PeerObservation,
    ) -> Result<(), PipelineError> {
        let a = {
            let mut core = self.inner.lock();
            let a = core
                .adapters
                .get(adapter.0)
                .cloned()
                .ok_or(PipelineError::UnknownAdapter)?;
            if core.connections.len() >= self.inner.config.max_connections {
                return Err(PipelineError::TooManyConnections);
            }
            let handle = connection.handle().clone();
            if observation != PeerObservation::none() {
                core.observed.insert(handle.clone(), observation);
            }
            core.connections.insert(handle, adapter);
            a
        };
        a.take_connection(connection);
        Ok(())
    }

    /// Binds the open attachment `attachment` to the session `record` registers, an own
    /// session of this device ([SEC-KEY-043]): the engine and the presence registry record
    /// it, the transport subscription for it starts, the adapter is told
    /// (`set_binding`, [IFC-ADP-030]), and its announcement is released to each device that
    /// may see it ([SC-DLV-051], [SEC-AUZ-011]). The declaration is the adapter's
    /// capabilities for the attachment, with the revision and extension identifier the core
    /// adds ([IFC-ADP-042]). The record's `native_id` is the binding's N for the decisions
    /// of later native signals (§6.7.3).
    ///
    /// The core binds from native signals itself (see the module documentation); this is
    /// for a binding decided elsewhere. It is decided in turn with native signals. It is no
    /// signal, so when a dropped or unpairable signal attributed to the attachment is still
    /// unanswered, the binding starts withheld until a later signal pairs ([SC-ID-154]).
    ///
    /// # Errors
    ///
    /// See [`PipelineError`]. Nothing changes on an error.
    pub fn bind(
        &self,
        attachment: &Attachment,
        record: &RegistrationRecord,
        display_name: Option<&str>,
    ) -> Result<(), PipelineError> {
        let result = {
            let _turn = self
                .inner
                .binding_turn
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            self.inner
                .bind_record(attachment, record, display_name, None)
        };
        self.inner.drain_binding();
        result
    }

    /// Where the findings and diagnostics of §6.7 are written from now on. By default they
    /// go to a [`MemoryBindingLog`] of 1024 entries that nothing reads.
    pub fn set_binding_log(&self, log: Box<dyn BindingLog>) {
        *self
            .inner
            .binding_log
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = log;
    }

    /// Drops each held native signal whose window has ended at the monotonic clock's now,
    /// with its diagnostic ([SC-ID-124], [SC-ID-128]). The owner arms one timer at
    /// [`Pipelines::next_native_signal_expiry`]; every adapter event also drops what has
    /// expired, so a late attachment never pairs with an expired signal.
    pub fn expire_native_signals(&self) {
        self.inner.drain_binding();
    }

    /// The local connection `connection` ended: the daemon (G9) calls this when its IPC
    /// layer observes the peer close, by end of stream or a broken pipe, never on a timeout.
    ///
    /// The core created the handle for that OS connection ([IFC-ADP-012]), so the end of
    /// the OS connection ends the core's own entries for it, for an attachment and for a
    /// connection that only carries native signals alike, and frees its place in
    /// [`PipelineConfig::max_connections`]. For an attachment it is also evidence that the
    /// attachment ended: its binding is deregistered ([SC-ID-155]), as on the adapter's
    /// `attachment-closed` ([IFC-ADP-022]), and a later `attachment-closed` for it changes
    /// nothing. An unknown handle changes nothing.
    pub fn disconnect(&self, connection: &ConnectionHandle) {
        self.inner.end_connection(connection);
    }

    /// The earliest instant a held native signal's window ends; `None` when none is held.
    pub fn next_native_signal_expiry(&self) -> Option<Instant> {
        let window = self.inner.config.native_signal_window;
        self.inner
            .lock()
            .held
            .iter()
            .map(|h| h.held_at + window)
            .min()
    }
}

impl Inner {
    /// The body of [`Pipelines::bind`], with the binding turn held; also how a native
    /// signal binds, `seq` being its arrival order (`None` for [`Pipelines::bind`]).
    ///
    /// [SC-ID-154]: when a dropped or unpairable signal attributed to the attachment is
    /// still unanswered, the new binding starts withheld unless `seq` is a later signal,
    /// which answers it. A binding decided elsewhere is no signal, so it answers nothing.
    fn bind_record(
        self: &Arc<Self>,
        attachment: &Attachment,
        record: &RegistrationRecord,
        display_name: Option<&str>,
        seq: Option<u64>,
    ) -> Result<(), PipelineError> {
        let inner = self;
        let sid = record.session_id().clone();
        let adapter = {
            let core = inner.lock();
            let entry = core
                .attachments
                .get(attachment)
                .filter(|e| e.open)
                .ok_or(PipelineError::UnknownAttachment)?;
            if entry.session.is_some() || core.sessions.contains_key(&sid) {
                return Err(PipelineError::AlreadyBound);
            }
            core.adapters[entry.adapter.0].clone()
        };
        let descriptor = descriptor_for(&*adapter, attachment, record, display_name)?;
        let weak = Arc::downgrade(inner);
        let sub = inner
            .transport
            .subscribe(
                &Destination::Session(sid.clone()),
                Arc::new(move |i: Inbound| {
                    if let Some(inner) = weak.upgrade() {
                        inner.on_envelope(i);
                    }
                }),
            )
            .map_err(PipelineError::Transport)?;
        let withheld = {
            let mut core = inner.lock();
            let still_open = core
                .attachments
                .get(attachment)
                .is_some_and(|e| e.open && e.session.is_none());
            if !still_open || core.sessions.contains_key(&sid) {
                drop(core);
                sub.end();
                return Err(PipelineError::AlreadyBound);
            }
            if !core.engine.register_session(record, &inner.identity) {
                drop(core);
                sub.end();
                return Err(PipelineError::InvalidRegistration);
            }
            core.registry.register_own(descriptor);
            core.sessions.insert(sid.clone(), attachment.clone());
            core.subscriptions.insert(sid.clone(), sub);
            match core.attachments.get_mut(attachment) {
                Some(e) => {
                    e.session = Some(sid.clone());
                    e.native_id = Some(record.native_id().to_owned());
                    if answers(e.withheld_by, seq) {
                        e.withheld_by = None;
                    }
                    e.withheld = e.withheld_by.is_some();
                    e.withheld
                }
                None => false,
            }
        };
        if withheld {
            // Bound, but withheld until a later signal pairs with it ([SC-ID-154]).
            inner.log(BindingLogEntry {
                record: BindingRecord {
                    kind: RecordKind::Finding,
                    requirement: "SC-ID-154",
                },
                result: BindingResult::FailedClosed,
                attachment: Some(attachment.clone()),
                session: Some(sid.clone()),
            });
        } else {
            adapter.set_binding(attachment, Some(sid.clone()));
        }
        inner.release_presence(&sid);
        Ok(())
    }
}

impl Pipelines {
    /// Ends the binding of `attachment`, if it has one: the adapter is told first, so it
    /// hands nothing more off to it ([IFC-ADP-030]); the transport subscription ends
    /// ([IFC-TRN-042]); the engine forgets the session's reply rights ([SEC-AUZ-015]) and
    /// its record partitions, and prunes every record past its reply period; the
    /// registry reads it `unreachable` ([SC-DLV-055]); and each device it was announced to
    /// is sent its withdrawal.
    pub fn unbind(&self, attachment: &Attachment) {
        self.inner.unbind(attachment);
    }

    /// Re-issues each announcement due for a refresh at the monotonic clock's now
    /// ([SC-DLV-054]). A refresh is a keepalive (§7.1.1): the owner arms one timer at
    /// [`Pipelines::next_presence_refresh_at`]; nothing here runs a timer or asks anyone for
    /// anything.
    pub fn refresh_presence(&self) {
        let inner = &self.inner;
        let now = (inner.monotonic)();
        let due: Vec<(SessionId, KeyId)> = {
            let core = inner.lock();
            core.announced
                .iter()
                .filter(|(_, a)| now >= a.at + inner.refresh_interval())
                .map(|(k, _)| k.clone())
                .collect()
        };
        for (session, device) in due {
            inner.announce_to(&session, &device, true);
        }
    }

    /// The earliest instant an announcement is due for a refresh; `None` when none was
    /// issued.
    pub fn next_presence_refresh_at(&self) -> Option<Instant> {
        let inner = &self.inner;
        let core = inner.lock();
        core.announced
            .values()
            .map(|a| a.at + inner.refresh_interval())
            .min()
    }

    /// This device's identity, which signs every envelope, record and receipt sent here,
    /// and registers sessions ([`DeviceIdentity::register`]).
    pub fn device(&self) -> &DeviceIdentity {
        &self.inner.identity
    }

    /// Runs `f` on the authorization engine, for an operator's grants, pairings and relay
    /// settings ([`AuthorizationEngine::add_grant`] and the rest). Do not call into the
    /// pipelines from `f`.
    pub fn with_engine<R>(&self, f: impl FnOnce(&mut AuthorizationEngine) -> R) -> R {
        f(&mut self.inner.lock().engine)
    }

    /// The session an attachment is bound to.
    pub fn binding(&self, attachment: &Attachment) -> Option<SessionId> {
        self.inner
            .lock()
            .attachments
            .get(attachment)
            .and_then(|e| e.session.clone())
    }

    /// The combined state of an envelope this device sent and still tracks
    /// ([SC-RCP-085]), at the clock's now.
    pub fn delivery_state(&self, id: &Token, from: &SessionId) -> Option<DeliveryState> {
        let now = self.inner.clock.now();
        self.inner
            .lock()
            .trackers
            .get(&(id.clone(), from.clone()))
            .map(|t| t.tracker.state(&now))
    }

    /// Stops: every adapter shuts down (reporting its attachments closed, which unbinds
    /// them), each announced session's withdrawal is sent ([SC-DLV-057]), and the
    /// transport shuts down.
    pub fn shutdown(&self) {
        let inner = &self.inner;
        let adapters: Vec<Arc<dyn ProviderAdapter>> = inner.lock().adapters.clone();
        for a in &adapters {
            a.shutdown();
        }
        let attachments: Vec<Attachment> = inner.lock().attachments.keys().cloned().collect();
        for a in attachments {
            inner.unbind(&a);
        }
        let device = inner.lock().device_subscription.take();
        if let Some(s) = device {
            s.end();
        }
        inner.transport.shutdown();
    }
}

/// The declaration for an attachment: the adapter's capabilities, with the revision and
/// extension identifier the core adds ([IFC-ADP-042]).
fn descriptor_for(
    adapter: &dyn ProviderAdapter,
    attachment: &Attachment,
    record: &RegistrationRecord,
    display_name: Option<&str>,
) -> Result<SessionDescriptor, PipelineError> {
    let caps = adapter.capabilities(attachment);
    let mut entry = CapabilitiesEntry::new(IMPLEMENTED_VERSION, caps.active_inbound);
    if !caps.content_types.is_empty() {
        entry = entry
            .with_content_types(caps.content_types)
            .ok_or(PipelineError::InvalidCapabilities)?;
    }
    if let Some(m) = caps.max_envelope_octets {
        entry = entry
            .with_max_envelope_octets(m)
            .ok_or(PipelineError::InvalidCapabilities)?;
    }
    let declaration = SessionCapabilities::declare([(EXTENSION_ID_V0, entry)])
        .ok_or(PipelineError::InvalidCapabilities)?;
    SessionDescriptor::new(
        record.session_id().clone(),
        declaration,
        display_name,
        Some(record.harness_label()),
    )
    .ok_or(PipelineError::InvalidCapabilities)
}

/// A fresh envelope `id`: 16 octets of the operating system's random number generator,
/// never derived from any input ([SC-ENV-023], [SC-ENV-024]).
fn fresh_id() -> Token {
    let mut octets = [0u8; 16];
    getrandom::fill(&mut octets).expect("the operating system's random number generator");
    Token::parse(&format!("msg-{}", crate::base64url::encode(&octets)))
        .expect("base64url is identifier-token safe")
}

/// The signal a withholding now waits to be answered after, when the signal `seq` withholds
/// an attachment already withheld by `withheld_by`: the later of the two, whichever order
/// the drops were applied in (an expired held signal can be dropped after a newer one).
fn withheld_until_after(withheld_by: Option<u64>, seq: u64) -> u64 {
    withheld_by.map_or(seq, |w| w.max(seq))
}

/// Whether a signal of arrival order `seq` (`None`: no signal) ends a withholding set by
/// the signal `withheld_by` ([SC-ID-154]: only "a later signal" does); with nothing
/// withheld, anything does.
fn answers(withheld_by: Option<u64>, seq: Option<u64>) -> bool {
    match (withheld_by, seq) {
        (None, _) => true,
        (Some(w), Some(s)) => s > w,
        (Some(_), None) => false,
    }
}

/// The rule of [`PipelineConfig::max_pending_signals`] over `pending`, each pending signal's
/// holder and `seq` (#335): the `seq` of the signal that makes room for a new one from
/// `holder`, or `None` when the new one goes instead.
fn fair_share_victim<H: Eq + std::hash::Hash>(
    pending: impl Iterator<Item = (H, u64)>,
    holder: &H,
) -> Option<u64> {
    // Per holder: how many it has pending, and the seq of its latest and its oldest.
    let mut counts: HashMap<H, (usize, u64, u64)> = HashMap::new();
    for (h, seq) in pending {
        let c = counts.entry(h).or_insert((0, seq, seq));
        c.0 += 1;
        c.1 = c.1.max(seq);
        c.2 = c.2.min(seq);
    }
    let mine = counts.get(holder).map_or(0, |c| c.0);
    let most = counts.values().map(|c| c.0).max().unwrap_or(0);
    if most >= mine + 2 {
        // Fair share: the oldest of the heaviest holder whose latest signal arrived last,
        // so neither a key's octets nor the map's order picks it.
        counts
            .values()
            .filter(|c| c.0 == most)
            .max_by_key(|c| c.1)
            .map(|c| c.2)
    } else {
        // Own share; `None` when this holder has nothing pending.
        counts.get(holder).map(|c| c.2)
    }
}

/// The request sink an adapter is given: every request it passes enters the send path
/// here, labelled with the adapter it came from.
struct Sink {
    inner: Weak<Inner>,
    adapter: AdapterId,
}

impl RequestSink for Sink {
    fn send(&self, request: SendRequest) -> SendRequestResult {
        match self.inner.upgrade() {
            Some(inner) => inner.send(self.adapter, request),
            None => SendRequestResult::Refused {
                error: ErrorCode::InternalError,
            },
        }
    }

    fn discover(&self, request: DiscoveryRequest) -> DiscoveryRequestResult {
        match self.inner.upgrade() {
            Some(inner) => inner.discover(self.adapter, request),
            None => DiscoveryRequestResult::Refused {
                error: ErrorCode::InternalError,
            },
        }
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, Core> {
        self.core.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn refresh_interval(&self) -> Duration {
        // Two fifths of the lifetime, as the issuer refreshes ([SC-DLV-054]).
        Duration::from_millis(self.config.presence_lifetime_ms * 2 / 5)
    }

    /// The transport deadline for the instant `unix_nanos` on the wall clock.
    fn deadline_at(&self, unix_nanos: i128) -> Deadline {
        let ahead = unix_nanos - self.clock.now().unix_nanos();
        let ahead = u64::try_from(ahead.max(0)).unwrap_or(u64::MAX);
        let now = (self.monotonic)();
        Deadline::at(
            now.checked_add(Duration::from_nanos(ahead))
                .unwrap_or(now + Duration::from_secs(86_400)),
        )
    }

    fn control_deadline(&self) -> Deadline {
        Deadline::at((self.monotonic)() + self.config.control_ttl)
    }

    /// The session bound to `attachment`, when the request came from the adapter that
    /// reported it and it is open (attribution, [SC-ID-160], [IFC-ADP-031]), and its send
    /// requests are not withheld ([SC-ID-154]).
    fn requester(core: &Core, adapter: AdapterId, attachment: &Attachment) -> Option<SessionId> {
        core.attachments
            .get(attachment)
            .filter(|e| e.adapter == adapter && e.open && !e.withheld)
            .and_then(|e| e.session.clone())
    }

    // ---- send path --------------------------------------------------------------------

    fn send(&self, adapter: AdapterId, request: SendRequest) -> SendRequestResult {
        let mono = (self.monotonic)();
        let now = self.clock.now();
        let mut guard = self.lock();
        let core = &mut *guard;
        let requester = Inner::requester(core, adapter, &request.attachment);
        let reply = match (&requester, &request.requested_target) {
            (Some(from), Some(t)) => core.engine.reply_headers(from, &request.to, Some(t)),
            _ => Default::default(),
        };
        let (engine, registry) = (&mut core.engine, &core.registry);
        let prepared = prepare_send(
            requester.as_ref(),
            &request,
            |r, t| {
                registry.send_presence(
                    r,
                    t,
                    |r, t| {
                        engine
                            .decide(&AuthorizationRequest::Discover {
                                requester: Requester::Session(r.clone()),
                                session: t.clone(),
                            })
                            .permits(Kind::Discover)
                    },
                    mono,
                )
            },
            &self.config.implemented,
            reply,
            &self.identity,
            fresh_id(),
            now,
        );
        let prepared = match prepared {
            Ok(p) => p,
            Err(error) => return SendRequestResult::Refused { error },
        };
        let env = prepared.envelope;
        let id = env.id().clone();
        let to_key = match core.engine.binding(env.to()) {
            Some(Binding::Key(k)) => k.clone(),
            // An `online` session is bound ([SEC-PRS-005]); anything else is a fault here.
            _ => {
                return SendRequestResult::NotPassed {
                    id,
                    error: ErrorCode::InternalError,
                };
            }
        };
        // [SEC-PRS-010]: the sender's announcement goes to the recipient's device first.
        let announcement = if &to_key == self.identity.key_id() {
            None
        } else {
            self.announcement(core, env.from(), &to_key, mono, false)
        };
        // [SEC-AUZ-013]: the reply right, and what a receipt must name (§10.2 check 3).
        core.engine
            .record_sent(SentRecord::of(&env, to_key.clone()));
        let (tx, rx) = sync_channel(self.config.receipt_stream_capacity.max(1));
        let key: TrackerKey = (id.clone(), env.from().clone());
        self.track(
            core,
            key.clone(),
            TrackerEntry {
                tracker: EnvelopeTracker::new(&env),
                receipts: tx,
            },
        );
        drop(guard);
        if let Some((destination, payload)) = announcement {
            let result =
                self.transport
                    .send_presence(&destination, payload, self.control_deadline());
            self.announcement_taken(env.from(), &to_key, mono, result);
            if result == PublishResult::NotTaken {
                // [SEC-PRS-010]: the announcement was not issued, so the envelope is not
                // passed either: `failed` with `transport-failure`, no copy passed (§8.4.1).
                // The sent record stays; no copy of its envelope exists to answer.
                self.lock().trackers.remove(&key);
                return SendRequestResult::NotPassed {
                    id,
                    error: ErrorCode::TransportFailure,
                };
            }
        }
        let deadline = self.deadline_at(HandOffDeadline::of(&env).unix_nanos());
        let result = self.transport.publish(
            &Destination::Session(env.to().clone()),
            Payload::new(PayloadKind::Envelope, env.octets().to_vec()),
            deadline,
        );
        let mut core = self.lock();
        match result {
            PublishResult::Taken => {
                if let Some(t) = core.trackers.get_mut(&key) {
                    t.tracker.passed();
                }
                SendRequestResult::Sent {
                    id,
                    correlation: prepared.correlation,
                    receipts: ReceiptStream(rx),
                }
            }
            PublishResult::NotTaken => {
                // `failed` with `transport-failure`: no copy was passed (§8.4.1); nothing
                // more will come for it, so it is not tracked.
                core.trackers.remove(&key);
                SendRequestResult::NotPassed {
                    id,
                    error: ErrorCode::TransportFailure,
                }
            }
        }
    }

    /// Keeps a tracker, within [`PipelineConfig::max_trackers`]: past it, trackers whose
    /// retry deadline has passed go first, then the oldest.
    fn track(&self, core: &mut Core, key: TrackerKey, entry: TrackerEntry) {
        let cap = self.config.max_trackers.max(1);
        if core.trackers.len() >= cap {
            let now = self.clock.now();
            core.trackers
                .retain(|_, t| !t.tracker.retry_deadline_passed(&now));
        }
        while core.trackers.len() >= cap {
            match core.tracker_order.pop_front() {
                Some(old) => {
                    core.trackers.remove(&old);
                }
                None => break,
            }
        }
        if core.tracker_order.len() >= 2 * cap {
            let live = &core.trackers;
            core.tracker_order.retain(|k| live.contains_key(k));
        }
        core.tracker_order.push_back(key.clone());
        core.trackers.insert(key, entry);
    }

    /// The announcement of the own session `session` for `device`, issued now when the
    /// release is authorized ([SEC-AUZ-011]), the transport may carry presence to another
    /// implementation ([IFC-TRN-081]), and none was issued to that device within the
    /// refresh interval (or `refresh` is set).
    fn announcement(
        &self,
        core: &mut Core,
        session: &SessionId,
        device: &KeyId,
        mono: Instant,
        refresh: bool,
    ) -> Option<(Destination, Payload)> {
        let caps = core.transport_caps?;
        if caps.reach != Reach::CrossImplementation || !caps.destination_restricted {
            return None;
        }
        let pair = (session.clone(), device.clone());
        if !refresh
            && core
                .announced
                .get(&pair)
                .is_some_and(|a| a.taken && mono < a.at + self.refresh_interval())
        {
            return None;
        }
        let permitted = core
            .engine
            .decide(&AuthorizationRequest::ReleasePresence {
                session: session.clone(),
                device: device.clone(),
            })
            .permits(Kind::ReleasePresence);
        if !permitted {
            return None;
        }
        let descriptor = core.registry.own_descriptor(session)?.clone();
        let record = core.issuer.announce(descriptor, self.clock.now(), mono)?;
        let payload =
            AuthenticatedPresenceRecord::issue(&self.identity, &record, device).to_payload()?;
        if !core.announced.contains_key(&pair)
            && core.announced.len() >= self.config.max_announcements.max(1)
            && let Some(oldest) = core
                .announced
                .iter()
                .min_by_key(|(_, a)| a.at)
                .map(|(k, _)| k.clone())
        {
            core.announced.remove(&oldest);
        }
        core.announced.insert(
            pair,
            Announced {
                at: mono,
                taken: false,
            },
        );
        Some(payload)
    }

    /// Records whether the transport took the announcement issued at `at`: taken, it
    /// serves later sends until its refresh; not taken, the next send issues another.
    fn announcement_taken(
        &self,
        session: &SessionId,
        device: &KeyId,
        at: Instant,
        result: PublishResult,
    ) {
        let mut core = self.lock();
        let pair = (session.clone(), device.clone());
        match (core.announced.get_mut(&pair), result) {
            (Some(a), PublishResult::Taken) if a.at == at => a.taken = true,
            (Some(a), PublishResult::NotTaken) if a.at == at => {
                core.announced.remove(&pair);
            }
            _ => {}
        }
    }

    fn announce_to(&self, session: &SessionId, device: &KeyId, refresh: bool) {
        let mono = (self.monotonic)();
        let payload = {
            let mut core = self.lock();
            self.announcement(&mut core, session, device, mono, refresh)
        };
        if let Some((destination, payload)) = payload {
            let result =
                self.transport
                    .send_presence(&destination, payload, self.control_deadline());
            self.announcement_taken(session, device, mono, result);
        }
    }

    /// Releases `session`'s announcement to each trusted device that may see it
    /// ([SC-DLV-051], [SEC-AUZ-011]).
    fn release_presence(&self, session: &SessionId) {
        let own = self.identity.key_id().clone();
        let devices: Vec<KeyId> = self
            .lock()
            .engine
            .trusted_keys()
            .iter()
            .map(|k| k.key_id().clone())
            .filter(|k| k != &own)
            .collect();
        for d in devices {
            self.announce_to(session, &d, false);
        }
    }

    fn discover(&self, adapter: AdapterId, request: DiscoveryRequest) -> DiscoveryRequestResult {
        let mono = (self.monotonic)();
        let mut guard = self.lock();
        let core = &mut *guard;
        let requester = Inner::requester(core, adapter, &request.attachment);
        // `discover` asks `may_discover` once per candidate; the engine logs each decision.
        let engine = std::cell::RefCell::new(&mut core.engine);
        let result = core.registry.discover(
            requester.as_ref(),
            |r, s| {
                engine
                    .borrow_mut()
                    .decide(&AuthorizationRequest::Discover {
                        requester: Requester::Session(r.clone()),
                        session: s.clone(),
                    })
                    .permits(Kind::Discover)
            },
            mono,
        );
        match result {
            Ok(list) => DiscoveryRequestResult::DiscoveryResult(list),
            Err(error) => DiscoveryRequestResult::Refused { error },
        }
    }

    // ---- receive path -----------------------------------------------------------------

    fn on_envelope(&self, inbound: Inbound) {
        if inbound.payload.kind() != PayloadKind::Envelope {
            return;
        }
        let limits = self.receiver_limits();
        let now = self.clock.now();
        // Envelope stage: a refused copy is reported with no `verified_by`, so no receipt
        // follows ([SC-RCP-041]).
        let Ok(msg) = receive_envelope(inbound.payload.octets(), &limits, &now) else {
            return;
        };
        let env = msg.envelope().clone();
        let steps = security_steps(msg, &mut self.lock().engine, &*self.clock);
        match steps {
            Ok(authorized) => self.deliver_now(authorized),
            Err(refused) => self.receipt(&env, *refused),
        }
        self.drain_requeued();
    }

    /// The receiver-wide envelope-stage limits: the configured ones, with the part types
    /// that some bound session takes besides `text`. A part type no session takes is refused
    /// at the envelope stage, one that only other sessions take at the delivery stage
    /// ([SC-RCP-076], [SC-RCP-077]).
    fn receiver_limits(&self) -> EnvelopeLimits {
        let mut types: Vec<String> = self.config.limits.part_types().to_vec();
        {
            let core = self.lock();
            for s in core.sessions.keys() {
                let agreed = core
                    .registry
                    .own_descriptor(s)
                    .and_then(|d| d.capabilities().agree(&self.config.implemented));
                if let Some(listed) = agreed.as_ref().and_then(|a| a.entry.content_types()) {
                    types.extend(listed.iter().filter(|t| *t != "text").cloned());
                }
            }
        }
        types.sort();
        types.dedup();
        self.config
            .limits
            .clone()
            .with_part_types(types)
            .unwrap_or_else(|| self.config.limits.clone())
    }

    /// Security step 5, the delivery stage and the hand-off, with no lock held across the
    /// hand-off call; then the hand-off record, and the receipt or the re-queue.
    fn deliver_now(&self, authorized: AuthorizedMessage) {
        let env = authorized.message().envelope().clone();
        // The hand-off record is kept by `hand_off` itself, before the call, so a reply the
        // harness makes during the call correlates; it is removed again unless the outcome
        // is `handed-to-harness` or `unknown` ([SEC-AUZ-016], [SC-RCP-050]).
        let (out, _record) = deliver_unrecorded(
            authorized,
            &self.store,
            &*self.clock,
            |to| self.target(to),
            |m| self.hand_off(m),
        );
        match out {
            ReceiveOutcome {
                received: Received::InFlight(key),
                requeue: Some(msg),
                ..
            } => self.requeue(&key, msg, &env),
            out => self.receipt(&env, out),
        }
    }

    /// The delivery stage's view of the addressed session (§8.3.2 steps 1 to 3): bound here
    /// with an open attachment and not withheld ([SC-ID-154]), with its adapter's
    /// capabilities.
    ///
    /// The `withheld` test here decides how the refusal is reported. It is not the only
    /// guard: [`Inner::hand_off`] re-checks `withheld` under the lock just before the call,
    /// which also covers a withholding that lands between the two, so dropping it here
    /// would change a report, never let a hand-off through (PR #333 review, N3).
    fn target(&self, to: &SessionId) -> Option<DeliveryTarget> {
        let (adapter, attachment, accepting) = {
            let core = self.lock();
            let attachment = core.sessions.get(to)?.clone();
            let entry = core.attachments.get(&attachment)?;
            let online = core.registry.state(to, (self.monotonic)())
                == crate::presence::PresenceState::Online;
            (
                core.adapters[entry.adapter.0].clone(),
                attachment,
                entry.open && !entry.withheld && online,
            )
        };
        let caps = adapter.capabilities(&attachment);
        Some(DeliveryTarget {
            accepting,
            active_inbound: caps.active_inbound,
            content_types: caps.content_types,
            max_envelope_octets: caps.max_envelope_octets,
        })
    }

    /// The hand-off call: at most one `deliver` on the adapter of the attachment bound to
    /// the envelope's `to` ([IFC-ADP-057]). A binding that ended since the delivery-stage
    /// checks makes no call and is `not-now` ([SC-DLV-007]).
    ///
    /// The hand-off record goes into the engine just before the call, so a reply the
    /// harness makes while the call is still running is correlated and the sender may be
    /// discovered ([SEC-AUZ-016]). It is removed when the call ends `not-now`, `failed` or
    /// `refused`, so a refused hand-off leaves no reply right or discovery right behind. A
    /// call that panics is contained and counts as `indeterminate` ([SC-RCP-006]): the
    /// content may have reached the harness, the record stays, and the thread goes on to
    /// offer any re-queued copy.
    fn hand_off(&self, msg: &ChannelMessage) -> HandOffOutcome {
        let target = {
            let core = self.lock();
            // The re-check is this lookup: `unbind`, and so an attachment's close, removes the
            // session's entry in the same step as its attachment's binding.
            core.sessions.get(msg.envelope().to()).and_then(|a| {
                let e = core.attachments.get(a).filter(|e| !e.withheld)?;
                Some((core.adapters[e.adapter.0].clone(), a.clone()))
            })
        };
        let Some((adapter, attachment)) = target else {
            return HandOffOutcome::NotNow;
        };
        // [IFC-TYP-091]: only a verified message is handed off; this one is, so the `None`
        // arm records a fault, never a hand-off.
        let Some(h) = HandOff::new(attachment, msg.clone()) else {
            return HandOffOutcome::Refused;
        };
        let record = HandOffRecord::of(msg.envelope());
        self.lock()
            .engine
            .record_handoff(record.clone(), DeliveryState::HandedToHarness);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| adapter.deliver(h)))
            .unwrap_or(HandOffOutcome::Indeterminate);
        if !outcome.may_be_handed_off() {
            self.lock().engine.forget_handoff(&record);
        }
        outcome
    }

    /// Re-queues an in-flight copy until its twin settles ([SEC-RPL-026]), within
    /// [`PipelineConfig::max_requeued`].
    fn requeue(&self, key: &crate::replay::DuplicateKey, msg: AuthorizedMessage, env: &Envelope) {
        let cap = self.config.max_requeued.max(1);
        if self.waiting.fetch_add(1, Ordering::SeqCst) >= cap {
            self.waiting.fetch_sub(1, Ordering::SeqCst);
            let verified_by = msg.message().verified_by().map(|p| p.key_id().clone());
            self.receipt(
                env,
                ReceiveOutcome {
                    received: Received::Reported(ReceiverReport::of(
                        DeliveryState::Failed,
                        Some(ErrorCode::InternalError),
                    )),
                    requeue: None,
                    verified_by,
                    finding: false,
                },
            );
            return;
        }
        let queue = Arc::clone(&self.requeued);
        self.store.when_settled(key, move || {
            queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push_back(msg);
        });
    }

    /// Offers again each re-queued copy whose twin has settled.
    fn drain_requeued(&self) {
        loop {
            let next = self
                .requeued
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front();
            let Some(msg) = next else { break };
            self.waiting.fetch_sub(1, Ordering::SeqCst);
            self.deliver_now(msg);
        }
    }

    /// The receipt gate, then the receipt (§8.1.5; `spec/security.md` §10).
    fn receipt(&self, env: &Envelope, out: ReceiveOutcome) {
        let Received::Reported(report) = out.received else {
            return;
        };
        let Some(device) = out.verified_by else {
            return; // [SC-RCP-041], [SEC-RCT-005]
        };
        let mono = (self.monotonic)();
        let observed_at = self.clock.now();
        let mut core = self.lock();
        if may_send_receipt(Some(&device), &report, &mut core.limiter, mono).is_err() {
            return;
        }
        if &device == self.identity.key_id() {
            // The sending implementation is this one: its receiver-observed state needs no
            // receipt on the wire ([SC-RCP-040]).
            if let Ok(observed) = report.observed(env, observed_at) {
                Inner::receipt_observed(&mut core, &observed);
            }
            return;
        }
        drop(core);
        let Ok(receipt) = report.receipt(env, observed_at) else {
            return;
        };
        let (destination, payload) =
            AuthenticatedReceipt::issue(&self.identity, &receipt, env).to_payload(device);
        let _ = self
            .transport
            .publish(&destination, payload, self.control_deadline());
    }

    /// A receiver-observed state for an envelope this device sent: into its tracker, and
    /// onto its receipt stream while there is room ([IFC-ADP-062]).
    fn receipt_observed(core: &mut Core, observed: &ObservedReceipt) {
        let r = observed.receipt();
        let Some(from) = SessionId::parse(r.envelope_from().as_str()) else {
            return;
        };
        let key = (r.envelope_id().clone(), from);
        if let Some(t) = core.trackers.get_mut(&key)
            && t.tracker.receipt(observed)
        {
            let _ = t.receipts.try_send(r.clone());
        }
    }

    fn on_device_payload(&self, inbound: Inbound) {
        if inbound.payload.kind() != PayloadKind::Receipt {
            return;
        }
        let Some(ar) = json::parse(inbound.payload.octets())
            .ok()
            .as_ref()
            .and_then(AuthenticatedReceipt::from_json)
        else {
            return;
        };
        let mut core = self.lock();
        if let Ok(observed) = accept_receipt(&ar, &core.engine) {
            Inner::receipt_observed(&mut core, &observed);
        }
    }

    fn on_presence(&self, event: PresenceEvent) {
        match event {
            PresenceEvent::Record { payload, carrier } => {
                if payload.kind() != PayloadKind::Presence {
                    return;
                }
                let Some(rec) = json::parse(payload.octets())
                    .ok()
                    .as_ref()
                    .and_then(AuthenticatedPresenceRecord::from_json)
                else {
                    return;
                };
                let mono = (self.monotonic)();
                let mut guard = self.lock();
                let core = &mut *guard;
                accept_authenticated_record(
                    &rec,
                    &mut core.engine,
                    &mut core.registry,
                    carrier,
                    mono,
                );
            }
            PresenceEvent::CarrierLoss { carrier } => {
                self.lock().registry.carrier_loss(&carrier);
            }
        }
    }

    // ---- attachments ------------------------------------------------------------------

    fn on_adapter_event(self: &Arc<Self>, adapter: AdapterId, event: AdapterEvent) {
        match event {
            AdapterEvent::AttachmentOpened {
                attachment,
                cross_check,
            } => {
                {
                    let mut core = self.lock();
                    // [IFC-ADP-013]: only a handle given to this adapter becomes its
                    // attachment.
                    if core.connections.get(&attachment) == Some(&adapter) {
                        core.attachments
                            .entry(attachment)
                            .or_insert(AttachmentEntry {
                                adapter,
                                open: true,
                                session: None,
                                native_id: None,
                                cross_check,
                                withheld: false,
                                withheld_by: None,
                                paired_seq: None,
                            });
                    }
                }
                // A held signal may pair with it now (§6.7.2). Set before the turn is tried,
                // so a holder that is mid-pass re-runs one (PR #333 review, B1).
                self.repair_needed.store(true, Ordering::SeqCst);
                self.drain_binding();
            }
            AdapterEvent::AttachmentClosed { attachment } => {
                let owned = self
                    .lock()
                    .attachments
                    .get(&attachment)
                    .is_some_and(|e| e.adapter == adapter);
                if owned {
                    self.end_connection(&attachment);
                }
            }
            AdapterEvent::CapabilitiesChanged { attachment } => {
                self.capabilities_changed(adapter, &attachment);
            }
            AdapterEvent::NativeSignal(signal) => {
                let dropped = {
                    let mut core = self.lock();
                    // The key is snapshot now, while the connection is known (B3).
                    let key = Inner::observed_key(&core, adapter, &signal);
                    let seq = core.next_signal_seq;
                    // One total arrival order; 2^64 signals never arrive, but say so rather
                    // than wrap silently in a release build.
                    core.next_signal_seq = seq
                        .checked_add(1)
                        .expect("fewer than 2^64 native signals in one process");
                    let queued = QueuedSignal {
                        adapter,
                        signal,
                        key,
                        seq,
                    };
                    let full = core.signals.len() + core.held.len()
                        >= self.config.max_pending_signals.max(1);
                    if !full {
                        core.signals.push_back(queued);
                        None
                    } else if let Some(victim) = Inner::pending_victim(&core, &queued.holder()) {
                        // Past the bound, a signal of the heaviest holder, or of this one,
                        // goes ([SC-ID-123]; #335).
                        let dropped = Inner::take_pending(&mut core, victim);
                        core.signals.push_back(queued);
                        dropped
                    } else {
                        // This holder has none to give up: this one goes.
                        let attempt = Inner::pair(&core, adapter, queued.key.as_ref());
                        Some((queued.signal, attempt, seq))
                    }
                };
                if let Some((s, attempt, seq)) = dropped {
                    self.drop_signal(&s, attempt, seq);
                }
                self.drain_binding();
            }
        }
    }

    /// The `seq` of the pending signal that makes room for a new one from `holder` at
    /// [`PipelineConfig::max_pending_signals`], by the rule documented there; `None` when
    /// the new one goes instead.
    fn pending_victim(core: &Core, holder: &SignalHolder) -> Option<u64> {
        let pending = core
            .held
            .iter()
            .map(|h| (SignalHolder::Key(h.adapter, h.key.clone()), h.seq))
            .chain(core.signals.iter().map(|q| (q.holder(), q.seq)));
        fair_share_victim(pending, holder)
    }

    /// Removes the pending signal `seq`, held or queued, with how it pairs now (by the key
    /// observed when it was reported) and its `seq`, to drop.
    fn take_pending(core: &mut Core, seq: u64) -> Option<(NativeSignal, PairAttempt, u64)> {
        if let Some(i) = core.held.iter().position(|h| h.seq == seq) {
            let h = core.held.remove(i)?;
            let attempt = Inner::pair_by_key(core, h.adapter, &h.key);
            return Some((h.signal, attempt, seq));
        }
        let i = core.signals.iter().position(|q| q.seq == seq)?;
        let q = core.signals.remove(i)?;
        let attempt = Inner::pair(core, q.adapter, q.key.as_ref());
        Some((q.signal, attempt, seq))
    }

    /// Drops `signal`, of arrival order `seq`, unbound, with its diagnostic ([SC-ID-124],
    /// [SC-ID-128]). The attachments its observed key attributes it to are withheld
    /// ([SC-ID-154]; PR #333 review, B2) until a signal that arrived after it pairs.
    fn drop_signal(&self, signal: &NativeSignal, attempt: PairAttempt, seq: u64) {
        let d = decide::<Attachment>(&[], signal, &Pairing::WindowExpired);
        self.log_decision(&d, None, None);
        let attributed = match attempt {
            PairAttempt::Paired(a) => vec![a],
            PairAttempt::Unpairable(bound) => bound,
            PairAttempt::NotYet(_) => Vec::new(),
        };
        for a in attributed {
            self.withhold(&a, seq);
        }
    }

    /// Forgets the connection `connection`: an attachment's binding ends first
    /// ([SC-ID-155]), then the core's own entries for it go, whatever kind of connection
    /// it was.
    ///
    /// The attachment is marked closed under the lock before anything else, so a bind
    /// already past its own checks on the binding turn finds it closed at its locked
    /// re-check and registers nothing; without that, a bind could complete between the
    /// unbind below and the removal, leaving a session with no attachment ([SC-ID-155];
    /// PR #333 re-review, N9).
    fn end_connection(&self, connection: &ConnectionHandle) {
        if let Some(e) = self.lock().attachments.get_mut(connection) {
            e.open = false;
        }
        self.unbind(connection);
        let mut core = self.lock();
        core.attachments.remove(connection);
        core.connections.remove(connection);
        core.observed.remove(connection);
    }

    // ---- binding from native signals (§6.7) -------------------------------------------

    /// Decides every queued native signal, one at a time, unless another thread is already
    /// doing so; that thread then decides this thread's signals too.
    fn drain_binding(self: &Arc<Self>) {
        loop {
            {
                let _turn = match self.binding_turn.try_lock() {
                    Ok(g) => g,
                    Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
                    Err(std::sync::TryLockError::WouldBlock) => return,
                };
                self.binding_pass();
            }
            // A signal queued, or an attachment opened, while the turn was held found the
            // turn taken and left its work here: do it now (PR #333 review, B1).
            if self.lock().signals.is_empty() && !self.repair_needed.load(Ordering::SeqCst) {
                return;
            }
        }
    }

    /// One pass, with the binding turn held: held signals whose window ended are dropped
    /// ([SC-ID-124]), held signals that now have a candidate are decided, then as many
    /// signals as were queued when the pass began are taken from the front of the queue,
    /// paired and decided, in arrival order. Those are the signals queued when the pass
    /// began, unless one of them was dropped at the cap during the pass: the signal queued
    /// in its place may then be decided in this pass (#335; PR #337 review, N3). Other work
    /// that arrives during the pass is left for [`Inner::drain_binding`]'s next pass.
    fn binding_pass(self: &Arc<Self>) {
        self.repair_needed.store(false, Ordering::SeqCst);
        let now = (self.monotonic)();
        let window = self.config.native_signal_window;
        let (expired, queued) = {
            let mut core = self.lock();
            let (gone, kept): (VecDeque<HeldSignal>, VecDeque<HeldSignal>) =
                core.held.drain(..).partition(|h| now >= h.held_at + window);
            core.held = kept;
            let expired: Vec<(HeldSignal, PairAttempt)> = gone
                .into_iter()
                .map(|h| {
                    let attempt = Inner::pair_by_key(&core, h.adapter, &h.key);
                    (h, attempt)
                })
                .collect();
            (expired, core.signals.len())
        };
        for (h, attempt) in expired {
            // [SC-ID-124], [SC-ID-128]: dropped, binding nothing; [SC-ID-154].
            self.drop_signal(&h.signal, attempt, h.seq);
        }
        // A held signal stays in `held` until it is decided, so it keeps counting against
        // its holder's share and keeps its place in arrival order (#335). One evicted
        // meanwhile is no longer there.
        let held: Vec<u64> = self.lock().held.iter().map(|h| h.seq).collect();
        for seq in held {
            let decided = {
                let mut core = self.lock();
                let Some(i) = core.held.iter().position(|h| h.seq == seq) else {
                    continue;
                };
                let attempt = Inner::pair_by_key(&core, core.held[i].adapter, &core.held[i].key);
                match attempt {
                    PairAttempt::NotYet(_) => None,
                    attempt => core.held.remove(i).map(|h| (h, attempt)),
                }
            };
            if let Some((h, attempt)) = decided {
                self.resolve(h.adapter, &h.signal, attempt, h.seq);
            }
        }
        for _ in 0..queued {
            let Some(QueuedSignal {
                adapter,
                signal,
                key,
                seq,
            }) = self.lock().signals.pop_front()
            else {
                return;
            };
            let attempt = Inner::pair(&self.lock(), adapter, key.as_ref());
            match attempt {
                PairAttempt::NotYet(key) => self.lock().held.push_back(HeldSignal {
                    adapter,
                    signal,
                    key,
                    held_at: now,
                    seq,
                }),
                attempt => self.resolve(adapter, &signal, attempt, seq),
            }
        }
    }

    /// Pairs `signal` by the key the core process observed for the connection it arrived
    /// on ([SC-ID-121]). No connection, a connection not given to `adapter`, or one with no
    /// observed key, is unpairable ([SC-ID-125]); nothing the signal carries is a key
    /// ([SC-ID-122]).
    ///
    /// The key is taken once, when the signal is reported ([`Inner::observed_key`]), and
    /// kept with it, so a connection that ends before the signal's turn (a hook that sends
    /// and exits) does not change how it pairs (PR #333 re-review, B3).
    fn pair(core: &Core, adapter: AdapterId, key: Option<&PairingKey>) -> PairAttempt {
        match key {
            Some(key) => Inner::pair_by_key(core, adapter, key),
            None => PairAttempt::Unpairable(Vec::new()),
        }
    }

    /// The key observed for the connection `signal` arrived on, when that connection was
    /// given to `adapter` and observed with one.
    fn observed_key(core: &Core, adapter: AdapterId, signal: &NativeSignal) -> Option<PairingKey> {
        signal
            .connection
            .as_ref()
            .filter(|c| core.connections.get(*c) == Some(&adapter))
            .and_then(|c| core.observed.get(c))
            .and_then(|o| o.pairing_key.clone())
    }

    /// The candidates for `key`: the open attachments of `adapter` whose observed key is
    /// equal. None is not yet pairable; more than one is unpairable ([SC-ID-125]).
    fn pair_by_key(core: &Core, adapter: AdapterId, key: &PairingKey) -> PairAttempt {
        let candidates: Vec<&Attachment> = core
            .attachments
            .iter()
            .filter(|(a, e)| {
                e.adapter == adapter
                    && e.open
                    && core.observed.get(*a).and_then(|o| o.pairing_key.as_ref()) == Some(key)
            })
            .map(|(a, _)| a)
            .collect();
        match candidates.as_slice() {
            [] => PairAttempt::NotYet(key.clone()),
            [one] => PairAttempt::Paired((*one).clone()),
            many => PairAttempt::Unpairable(
                many.iter()
                    .filter(|a| {
                        core.attachments
                            .get(**a)
                            .is_some_and(|e| e.session.is_some())
                    })
                    .map(|a| (*a).clone())
                    .collect(),
            ),
        }
    }

    /// The attachments of `adapter` as the binding decision sees them.
    fn attachment_states(core: &Core, adapter: AdapterId) -> Vec<AttachmentState<Attachment>> {
        core.attachments
            .iter()
            .filter(|(_, e)| e.adapter == adapter && e.open)
            .map(|(a, e)| AttachmentState {
                attachment: a.clone(),
                cross_check: e.cross_check.clone(),
                binding: e.session.clone().zip(e.native_id.clone()).map(
                    |(session_id, native_id)| NativeBinding {
                        native_id,
                        session_id,
                    },
                ),
            })
            .collect()
    }

    /// Decides one signal, of arrival order `seq`, that was paired or found unpairable, and
    /// makes the change.
    fn resolve(
        self: &Arc<Self>,
        adapter: AdapterId,
        signal: &NativeSignal,
        attempt: PairAttempt,
        seq: u64,
    ) {
        let (pairing, attributed) = match attempt {
            PairAttempt::Paired(a) => (Pairing::Paired(a), Vec::new()),
            PairAttempt::Unpairable(bound) => (Pairing::Unpairable, bound),
            PairAttempt::NotYet(_) => return,
        };
        // #338: a held signal is decided when a candidate opens, so a newer signal of the
        // same key can pair while an older one is still held, not yet pairable (its
        // candidate opened in the middle of a pass, or after it). Decided later, the older
        // one would bind the key's attachment back to the older conversation, even a new
        // attachment after a reconnect. So when a signal pairs, the window of every held
        // signal of its adapter and key that arrived before it ends now ([SC-ID-123]
        // allows "at most a bounded window"): each is dropped before it pairs, binding
        // nothing ([SC-ID-124], [SC-ID-128]). This pairing is the later signal that answers
        // them, so they withhold nothing ([SC-ID-154]). No state outlives an attachment:
        // a held signal's seq is lower than every queued one's, so a key's older signals
        // that are still pending are all held here. One can also be in flight: taken for
        // eviction on another thread, its drop not yet applied. That drop reaches
        // `withhold` after this pairing, which `paired_seq` (set here, under the same lock)
        // makes it ignore (PR #339 review).
        if let Pairing::Paired(a) = &pairing {
            let superseded = {
                let mut core = self.lock();
                if let Some(e) = core.attachments.get_mut(a) {
                    e.paired_seq = Some(e.paired_seq.map_or(seq, |p| p.max(seq)));
                }
                match core.observed.get(a).and_then(|o| o.pairing_key.clone()) {
                    Some(key) => {
                        let (gone, kept): (VecDeque<HeldSignal>, VecDeque<HeldSignal>) = core
                            .held
                            .drain(..)
                            .partition(|h| h.adapter == adapter && h.key == key && h.seq < seq);
                        core.held = kept;
                        gone
                    }
                    None => VecDeque::new(),
                }
            };
            for h in superseded {
                self.drop_signal(&h.signal, PairAttempt::Unpairable(Vec::new()), h.seq);
            }
        }
        let (decision, session) = {
            let core = self.lock();
            let states = Inner::attachment_states(&core, adapter);
            let session = match &pairing {
                Pairing::Paired(a) => core.attachments.get(a).and_then(|e| e.session.clone()),
                _ => None,
            };
            (decide(&states, signal, &pairing), session)
        };
        let paired = match &pairing {
            Pairing::Paired(a) => Some(a.clone()),
            _ => None,
        };
        self.log_decision(&decision, paired.clone(), session);
        // [SC-ID-154]: an unpairable signal that the observed key attributes to the harness
        // process behind bound attachments withholds delivery to them and their send
        // requests, until a later signal pairs with one.
        for a in attributed {
            self.withhold(&a, seq);
        }
        match decision.action {
            BindingAction::None => {}
            // [SC-ID-152]: a stale binding ends; the attachment stays unbound.
            BindingAction::Deregister { attachment } => self.unbind(&attachment),
            BindingAction::Bind {
                attachment,
                native_id,
                deregister_first,
            } => {
                if deregister_first {
                    // [SC-ID-150]; the new session id carries none of the earlier one's
                    // authorization state ([SC-ID-151]).
                    self.unbind(&attachment);
                }
                self.bind_from_signal(&attachment, &native_id, seq);
            }
        }
        if let Some(a) = paired {
            self.release_withheld(&a, seq);
        }
    }

    /// Binds `attachment` to N under a new session id, with a registration record this
    /// device signs ([SC-ID-009]) over the scope the core process observed for it. Without
    /// that scope no record can be made, so nothing is bound and a diagnostic says why.
    fn bind_from_signal(self: &Arc<Self>, attachment: &Attachment, native_id: &str, seq: u64) {
        let (scope, sid) = {
            let core = self.lock();
            let scope = core
                .observed
                .get(attachment)
                .and_then(|o| Some((o.harness_label.clone()?, o.working_directory.clone()?)));
            // [SC-ID-008]: 128 random bits never repeat in practice; a collision with a
            // live session would still be refused, so draw again.
            let mut sid = fresh_session_id();
            while core.sessions.contains_key(&sid) {
                sid = fresh_session_id();
            }
            (scope, sid)
        };
        let record = scope.and_then(|(label, wd)| {
            self.identity
                .register(sid, label, native_id, &wd, self.clock.now())
        });
        let bound =
            record.is_some_and(|r| self.bind_record(attachment, &r, None, Some(seq)).is_ok());
        if !bound {
            self.log(BindingLogEntry {
                record: BindingRecord {
                    kind: RecordKind::Diagnostic,
                    requirement: "SC-ID-009",
                },
                // Nothing was bound, and an earlier binding may have just ended
                // ([SC-ID-150]): the attachment is left unbound.
                result: BindingResult::FailedClosed,
                attachment: Some(attachment.clone()),
                session: None,
            });
        }
    }

    /// [SC-ID-154], for the dropped or unpairable signal `seq`: no hand-off to `attachment`
    /// and no send request from it until a signal that arrived after `seq` pairs with it;
    /// the adapter is told its binding names no session (`spec/interfaces.md` §5.4). An
    /// attachment between bindings (a decision for an earlier signal is re-binding it) has
    /// nothing to stop yet; `seq` is kept, so that re-binding starts withheld.
    ///
    /// A signal older than one that already paired with `attachment` is answered already
    /// and withholds nothing. When a signal pairs, the older held signals of its key are
    /// dropped there without withholding (#338); but a drop taken for eviction on another
    /// thread before that pairing can reach here after it, and `paired_seq` makes it a
    /// no-op (PR #339 review).
    ///
    /// The adapter is told after the lock is released, as every adapter call is: under a
    /// concurrent drop or release its view can briefly lag the core's. The core's own
    /// `withheld` check, taken under the lock, is what refuses hand-offs and send requests
    /// (PR #337 re-review, N7).
    fn withhold(&self, attachment: &Attachment, seq: u64) {
        let (adapter, session) = {
            let mut guard = self.lock();
            let core = &mut *guard;
            let Some(e) = core.attachments.get_mut(attachment) else {
                return;
            };
            if e.paired_seq.is_some_and(|p| p > seq) {
                return;
            }
            e.withheld_by = Some(withheld_until_after(e.withheld_by, seq));
            let Some(s) = e.session.clone() else { return };
            e.withheld = true;
            (core.adapters[e.adapter.0].clone(), s)
        };
        adapter.set_binding(attachment, None);
        self.log(BindingLogEntry {
            record: BindingRecord {
                kind: RecordKind::Finding,
                requirement: "SC-ID-154",
            },
            result: BindingResult::FailedClosed,
            attachment: Some(attachment.clone()),
            session: Some(session),
        });
    }

    /// The signal `seq` paired with `attachment`: when it arrived after every signal that
    /// withheld it, delivery resumes if it was withheld and it is still bound
    /// ([SC-ID-154]). An earlier signal, decided while a later one was dropped, releases
    /// nothing.
    fn release_withheld(&self, attachment: &Attachment, seq: u64) {
        let released = {
            let mut guard = self.lock();
            let core = &mut *guard;
            let Some(e) = core.attachments.get_mut(attachment) else {
                return;
            };
            if !answers(e.withheld_by, Some(seq)) {
                return;
            }
            e.withheld_by = None;
            if !e.withheld {
                return;
            }
            e.withheld = false;
            e.session
                .clone()
                .map(|s| (core.adapters[e.adapter.0].clone(), s))
        };
        if let Some((adapter, session)) = released {
            adapter.set_binding(attachment, Some(session));
        }
    }

    fn log_decision(
        &self,
        decision: &BindingDecision<Attachment>,
        attachment: Option<Attachment>,
        session: Option<SessionId>,
    ) {
        for record in &decision.records {
            self.log(BindingLogEntry {
                record: *record,
                result: decision.result,
                attachment: attachment.clone(),
                session: session.clone(),
            });
        }
    }

    fn log(&self, entry: BindingLogEntry) {
        self.binding_log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .record(entry);
    }

    fn capabilities_changed(&self, adapter: AdapterId, attachment: &Attachment) {
        let (a, session, descriptor) = {
            let core = self.lock();
            let Some(e) = core
                .attachments
                .get(attachment)
                .filter(|e| e.adapter == adapter)
            else {
                return;
            };
            let Some(s) = e.session.clone() else { return };
            let Some(d) = core.registry.own_descriptor(&s).cloned() else {
                return;
            };
            (core.adapters[adapter.0].clone(), s, d)
        };
        let caps = a.capabilities(attachment);
        let mut entry = CapabilitiesEntry::new(IMPLEMENTED_VERSION, caps.active_inbound);
        if !caps.content_types.is_empty() {
            match entry.with_content_types(caps.content_types) {
                Some(e) => entry = e,
                None => return,
            }
        }
        if let Some(m) = caps.max_envelope_octets {
            match entry.with_max_envelope_octets(m) {
                Some(e) => entry = e,
                None => return,
            }
        }
        let Some(declaration) = SessionCapabilities::declare([(EXTENSION_ID_V0, entry)]) else {
            return;
        };
        let Some(updated) = SessionDescriptor::new(
            session.clone(),
            declaration,
            descriptor.display_name(),
            descriptor.harness_label().as_ref(),
        ) else {
            return;
        };
        let devices: Vec<KeyId> = {
            let mut core = self.lock();
            core.registry.register_own(updated);
            core.announced
                .keys()
                .filter(|(s, _)| s == &session)
                .map(|(_, d)| d.clone())
                .collect()
        };
        // A changed declaration is announced again ([SC-DLV-052]).
        for d in devices {
            self.announce_to(&session, &d, true);
        }
    }

    fn unbind(&self, attachment: &Attachment) {
        let (adapter, session) = {
            let core = self.lock();
            let Some(e) = core.attachments.get(attachment) else {
                return;
            };
            let Some(s) = e.session.clone() else { return };
            (core.adapters[e.adapter.0].clone(), s)
        };
        adapter.set_binding(attachment, None);
        let (sub, withdrawals) = {
            let mut core = self.lock();
            if let Some(e) = core.attachments.get_mut(attachment) {
                e.session = None;
                e.native_id = None;
                e.withheld = false;
            }
            core.sessions.remove(&session);
            core.engine.end_session(&session);
            core.engine.prune();
            core.registry.deregister_own(&session);
            let sub = core.subscriptions.remove(&session);
            let devices: Vec<KeyId> = core
                .announced
                .keys()
                .filter(|(s, _)| s == &session)
                .map(|(_, d)| d.clone())
                .collect();
            core.announced.retain(|(s, _), _| s != &session);
            let now = self.clock.now();
            let withdrawals: Vec<(Destination, Payload)> =
                match core.issuer.withdraw(&session, now, true) {
                    Some(record) => devices
                        .iter()
                        .filter_map(|d| {
                            AuthenticatedPresenceRecord::issue(&self.identity, &record, d)
                                .to_payload()
                        })
                        .collect(),
                    None => Vec::new(),
                };
            (sub, withdrawals)
        };
        if let Some(s) = sub {
            s.end();
        }
        for (destination, payload) in withdrawals {
            let _ = self
                .transport
                .send_presence(&destination, payload, self.control_deadline());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AdapterId, SignalHolder, answers, fair_share_victim, withheld_until_after};

    /// PR #337 ([SC-ID-154], "a later signal"): only a signal that arrived after every
    /// withholding one answers it; a binding decided elsewhere (no signal) answers nothing;
    /// with nothing withheld, anything does.
    #[test]
    fn only_a_later_signal_answers_a_withholding() {
        assert!(answers(None, None));
        assert!(answers(None, Some(0)));
        assert!(answers(Some(4), Some(5)));
        assert!(!answers(Some(4), Some(4)));
        assert!(!answers(Some(4), Some(3)));
        assert!(!answers(Some(4), None));
        // Applied in either order, two withholdings wait for the later one.
        assert_eq!(withheld_until_after(None, 7), 7);
        assert_eq!(withheld_until_after(Some(9), 7), 9);
        assert_eq!(withheld_until_after(Some(7), 9), 9);
    }
    use crate::adapter::Connection;
    use crate::session_binding::PairingKey;

    /// #335: a signal counts against its observed key under its adapter, whatever
    /// connection it came on; without a key, against its connection; with neither, against
    /// the one shared holder. The same key under another adapter is another holder, as it
    /// pairs separately (PR #337 review, N5).
    #[test]
    fn a_signal_counts_against_its_key_else_its_connection() {
        let key = |s: &str| PairingKey::from_observation(s.as_bytes().to_vec());
        let (p, q) = (AdapterId(0), AdapterId(1));
        let of = |k: Option<&PairingKey>, c| SignalHolder::of(p, k, c);
        let c1 = Connection::accept(std::io::empty(), std::io::sink());
        let c2 = Connection::accept(std::io::empty(), std::io::sink());
        let (c1, c2) = (c1.handle(), c2.handle());
        let k = key("k");
        assert!(
            of(Some(&k), Some(c1)) == of(Some(&k), Some(c2)),
            "one key over two connections is one holder"
        );
        assert!(of(Some(&k), Some(c1)) == of(Some(&k), None));
        assert!(of(Some(&k), None) != of(Some(&key("j")), None));
        assert!(
            SignalHolder::of(p, Some(&k), Some(c1)) != SignalHolder::of(q, Some(&k), Some(c1)),
            "one key under two adapters is two holders"
        );
        assert!(
            of(None, Some(c1)) != of(None, Some(c2)),
            "two unkeyed connections are two holders"
        );
        assert!(of(None, Some(c1)) == SignalHolder::of(q, None, Some(c1)));
        assert!(of(None, Some(c1)) != of(None, None));
        assert!(of(None, None) == SignalHolder::of(q, None, None));
        assert!(of(Some(&k), Some(c1)) != of(None, Some(c1)));
    }

    /// The victim for a newcomer from `holder`, over `pending` given as (holder, seq).
    fn victim(pending: &[(char, u64)], holder: char) -> Option<u64> {
        fair_share_victim(pending.iter().copied(), &holder)
    }

    /// #335, boundary: another holder pays only when it has at least two more than the
    /// newcomer; with one more, the newcomer gives up its own oldest, or itself.
    #[test]
    fn fair_share_boundary() {
        // `a` has 2, the newcomer `b` has 0: 2 >= 0 + 2, so `a`'s oldest goes.
        assert_eq!(victim(&[('a', 0), ('a', 1)], 'b'), Some(0));
        // `a` has 2, `b` has 1: 2 < 1 + 2, so `b` gives up its own oldest.
        assert_eq!(victim(&[('a', 0), ('a', 1), ('b', 2)], 'b'), Some(2));
        // `a` has 1, `b` has 0: `b`'s new signal is refused.
        assert_eq!(victim(&[('a', 0)], 'b'), None);
        // `a` has 3, `b` has 1: 3 >= 1 + 2, so `a`'s oldest goes, not `b`'s.
        assert_eq!(
            victim(&[('b', 0), ('a', 1), ('a', 2), ('a', 3)], 'b'),
            Some(1)
        );
        // `b` has 2 (seq 0 and 4), `a` has 3: `b` gives up its own oldest.
        assert_eq!(
            victim(&[('b', 0), ('a', 1), ('a', 2), ('a', 3), ('b', 4)], 'b'),
            Some(0)
        );
        // The flooding holder at the most only ever gives up its own.
        assert_eq!(
            victim(&[('b', 0), ('a', 1), ('a', 2), ('a', 3)], 'a'),
            Some(1)
        );
        // Nothing pending at all: refused.
        assert_eq!(victim(&[], 'a'), None);
    }

    /// #335, tie-break: among holders tied at the most, the one whose latest signal arrived
    /// last pays, with its oldest, whatever the holders' names or order.
    #[test]
    fn fair_share_tie_break_is_by_recency() {
        // `z` and `a` tie at 2; `a` reported last.
        let pending = [('z', 0), ('z', 1), ('a', 2), ('a', 3)];
        assert_eq!(victim(&pending, 'n'), Some(2));
        // The names swapped: still the one that reported last.
        let pending = [('a', 0), ('a', 1), ('z', 2), ('z', 3)];
        assert_eq!(victim(&pending, 'n'), Some(2));
        // Interleaved: `z` holds 0 and 3, `a` holds 1 and 2. `z`'s latest is newest, so
        // its oldest (0) goes: not the overall newest, and not `a`'s.
        let pending = [('z', 0), ('a', 1), ('a', 2), ('z', 3)];
        assert_eq!(victim(&pending, 'n'), Some(0));
        // A lighter holder that reported last is not among the heaviest and never pays.
        let pending = [('z', 0), ('z', 1), ('a', 2), ('a', 3), ('m', 4)];
        assert_eq!(victim(&pending, 'n'), Some(2));
    }
}
