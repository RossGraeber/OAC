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
//! withholds delivery to them and their send requests until a signal pairs with one
//! ([SC-ID-154]). Signals are decided one at a time, in arrival order, by whichever thread
//! holds the binding turn; a signal reported, or an attachment opened, while the turn is
//! held makes the holder pass again. The held and queued signals together are bounded by
//! [`PipelineConfig::max_pending_signals`]. A held signal's window is checked on every
//! adapter event and when the owner's timer calls [`Pipelines::expire_native_signals`];
//! nothing polls.
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
    /// The most native signals held or waiting for their decision at once. Past it the
    /// oldest held signal, or a new one when none is held, is dropped with a diagnostic
    /// ([SC-ID-123], [SC-ID-128]), withholding any bound attachment its observed key
    /// attributes it to ([SC-ID-154]). The cap is shared by every key: a per-key share is
    /// #335.
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
    /// Delivery and send requests withheld under [SC-ID-154] until a signal pairs with it.
    withheld: bool,
}

/// A native signal waiting for its decision, with the pairing key observed for its
/// connection when it was reported (`None` when there was none).
struct QueuedSignal {
    adapter: AdapterId,
    signal: NativeSignal,
    key: Option<PairingKey>,
}

/// A native signal that is not yet pairable, held for the window ([SC-ID-123]).
struct HeldSignal {
    adapter: AdapterId,
    signal: NativeSignal,
    key: PairingKey,
    held_at: Instant,
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
    /// for a binding decided elsewhere. It is decided in turn with native signals.
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
            self.inner.bind_record(attachment, record, display_name)
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
    /// The body of [`Pipelines::bind`], with the binding turn held.
    fn bind_record(
        self: &Arc<Self>,
        attachment: &Attachment,
        record: &RegistrationRecord,
        display_name: Option<&str>,
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
        {
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
            if let Some(e) = core.attachments.get_mut(attachment) {
                e.session = Some(sid.clone());
                e.native_id = Some(record.native_id().to_owned());
                e.withheld = false;
            }
        }
        adapter.set_binding(attachment, Some(sid.clone()));
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
                    let full = core.signals.len() + core.held.len()
                        >= self.config.max_pending_signals.max(1);
                    if !full {
                        core.signals.push_back(QueuedSignal {
                            adapter,
                            signal,
                            key,
                        });
                        None
                    } else if let Some(oldest) = core.held.pop_front() {
                        // Past the bound, the oldest held signal goes ([SC-ID-123]).
                        core.signals.push_back(QueuedSignal {
                            adapter,
                            signal,
                            key,
                        });
                        let attempt = Inner::pair_by_key(&core, oldest.adapter, &oldest.key);
                        Some((oldest.signal, attempt))
                    } else {
                        // With none held, this one does.
                        let attempt = Inner::pair(&core, adapter, key.as_ref());
                        Some((signal, attempt))
                    }
                };
                if let Some((s, attempt)) = dropped {
                    self.drop_signal(&s, attempt);
                }
                self.drain_binding();
            }
        }
    }

    /// Drops `signal` unbound, with its diagnostic ([SC-ID-124], [SC-ID-128]). When its
    /// observed key attributes it to bound attachments, delivery to them is withheld
    /// ([SC-ID-154]; PR #333 review, B2).
    fn drop_signal(&self, signal: &NativeSignal, attempt: PairAttempt) {
        let d = decide::<Attachment>(&[], signal, &Pairing::WindowExpired);
        self.log_decision(&d, None, None);
        let attributed = match attempt {
            PairAttempt::Paired(a) => vec![a],
            PairAttempt::Unpairable(bound) => bound,
            PairAttempt::NotYet(_) => Vec::new(),
        };
        for a in attributed {
            self.withhold(&a);
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
    /// ([SC-ID-124]), held signals that now have a candidate are decided, then each signal
    /// queued when the pass began is paired and decided, in arrival order. Work that
    /// arrives during the pass is left for [`Inner::drain_binding`]'s next pass.
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
            self.drop_signal(&h.signal, attempt);
        }
        let held: Vec<HeldSignal> = self.lock().held.drain(..).collect();
        for h in held {
            let attempt = Inner::pair_by_key(&self.lock(), h.adapter, &h.key);
            match attempt {
                PairAttempt::NotYet(_) => self.lock().held.push_back(h),
                attempt => self.resolve(h.adapter, &h.signal, attempt),
            }
        }
        for _ in 0..queued {
            let Some(QueuedSignal {
                adapter,
                signal,
                key,
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
                }),
                attempt => self.resolve(adapter, &signal, attempt),
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

    /// Decides one signal that was paired or found unpairable, and makes the change.
    fn resolve(self: &Arc<Self>, adapter: AdapterId, signal: &NativeSignal, attempt: PairAttempt) {
        let (pairing, attributed) = match attempt {
            PairAttempt::Paired(a) => (Pairing::Paired(a), Vec::new()),
            PairAttempt::Unpairable(bound) => (Pairing::Unpairable, bound),
            PairAttempt::NotYet(_) => return,
        };
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
            self.withhold(&a);
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
                self.bind_from_signal(&attachment, &native_id);
            }
        }
        if let Some(a) = paired {
            self.release_withheld(&a);
        }
    }

    /// Binds `attachment` to N under a new session id, with a registration record this
    /// device signs ([SC-ID-009]) over the scope the core process observed for it. Without
    /// that scope no record can be made, so nothing is bound and a diagnostic says why.
    fn bind_from_signal(self: &Arc<Self>, attachment: &Attachment, native_id: &str) {
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
        let bound = record.is_some_and(|r| self.bind_record(attachment, &r, None).is_ok());
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

    /// [SC-ID-154]: no hand-off to `attachment` and no send request from it; the adapter is
    /// told its binding names no session (`spec/interfaces.md` §5.4).
    fn withhold(&self, attachment: &Attachment) {
        let (adapter, session) = {
            let mut guard = self.lock();
            let core = &mut *guard;
            let Some(e) = core.attachments.get_mut(attachment) else {
                return;
            };
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

    /// A signal paired with `attachment`: delivery to it resumes if it was withheld and it
    /// is still bound ([SC-ID-154]).
    fn release_withheld(&self, attachment: &Attachment) {
        let released = {
            let mut guard = self.lock();
            let core = &mut *guard;
            let Some(e) = core.attachments.get_mut(attachment) else {
                return;
            };
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
