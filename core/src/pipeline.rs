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
//!    [SEC-AUZ-011]) and the transport may carry it ([IFC-TRN-081]);
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
//! 4. the hand-off record ([SEC-AUZ-016]);
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
//! # What is not here
//!
//! Choosing which session an attachment is bound to (the native-signal procedure of
//! `spec/session-channels.md` §6.7) is the caller's: it calls [`Pipelines::bind`] with a
//! registration record. Accepting and authenticating local connections is the daemon's
//! (G9); it passes each to [`Pipelines::connect`].

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

use crate::adapter::{
    AdapterEvent, Attachment, Connection, DiscoveryRequest, DiscoveryRequestResult, HandOff,
    HandOffOutcome, ProviderAdapter, ReceiptStream, RequestSink, SendRequest, SendRequestResult,
};
use crate::authorization::{
    AuthorizationEngine, AuthorizationRequest, AuthorizedMessage, Binding, Kind, Requester,
    SentRecord,
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
    may_send_receipt, record_outcome, security_steps,
};
use crate::registration::RegistrationRecord;
use crate::registry::{CROSS_IMPLEMENTATION_PRESENCE_CAP_MS, PresenceIssuer, PresenceRegistry};
use crate::replay::{DuplicateStore, HandOffDeadline};
use crate::sender::{EnvelopeTracker, ObservedReceipt, prepare_send};
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
    /// ([IFC-ADP-012]), to `adapter`. Only a handle given to an adapter here can become
    /// one of its attachments ([IFC-ADP-013]).
    ///
    /// # Errors
    ///
    /// [`PipelineError::UnknownAdapter`], or [`PipelineError::TooManyConnections`].
    pub fn connect(&self, adapter: AdapterId, connection: Connection) -> Result<(), PipelineError> {
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
            core.connections
                .insert(connection.handle().clone(), adapter);
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
    /// adds ([IFC-ADP-042]).
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
        let inner = &self.inner;
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
            }
        }
        adapter.set_binding(attachment, Some(sid.clone()));
        inner.release_presence(&sid);
        Ok(())
    }

    /// Ends the binding of `attachment`, if it has one: the adapter is told first, so it
    /// hands nothing more off to it ([IFC-ADP-030]); the transport subscription ends
    /// ([IFC-TRN-042]); the engine forgets the session's reply rights ([SEC-AUZ-015]); the
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
    /// reported it and it is open (attribution, [SC-ID-160], [IFC-ADP-031]).
    fn requester(core: &Core, adapter: AdapterId, attachment: &Attachment) -> Option<SessionId> {
        core.attachments
            .get(attachment)
            .filter(|e| e.adapter == adapter && e.open)
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
        let (out, record) = deliver_unrecorded(
            authorized,
            &self.store,
            &*self.clock,
            |to| self.target(to),
            |m| self.hand_off(m),
        );
        record_outcome(&mut self.lock().engine, record, &out);
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
                entry.open && online,
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
    fn hand_off(&self, msg: &ChannelMessage) -> HandOffOutcome {
        let target = {
            let core = self.lock();
            core.sessions.get(msg.envelope().to()).and_then(|a| {
                let e = core.attachments.get(a)?;
                (e.open && e.session.is_some())
                    .then(|| (core.adapters[e.adapter.0].clone(), a.clone()))
            })
        };
        let Some((adapter, attachment)) = target else {
            return HandOffOutcome::NotNow;
        };
        match HandOff::new(attachment, msg.clone()) {
            Some(h) => adapter.deliver(h),
            // [IFC-TYP-091]: only a verified message is handed off; this one is, so this
            // arm records a fault, never a hand-off.
            None => HandOffOutcome::Refused,
        }
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

    fn on_adapter_event(&self, adapter: AdapterId, event: AdapterEvent) {
        match event {
            AdapterEvent::AttachmentOpened { attachment, .. } => {
                let mut core = self.lock();
                // [IFC-ADP-013]: only a handle given to this adapter becomes its attachment.
                if core.connections.get(&attachment) == Some(&adapter) {
                    core.attachments
                        .entry(attachment)
                        .or_insert(AttachmentEntry {
                            adapter,
                            open: true,
                            session: None,
                        });
                }
            }
            AdapterEvent::AttachmentClosed { attachment } => {
                let owned = self
                    .lock()
                    .attachments
                    .get(&attachment)
                    .is_some_and(|e| e.adapter == adapter);
                if owned {
                    self.unbind(&attachment);
                    let mut core = self.lock();
                    core.attachments.remove(&attachment);
                    core.connections.remove(&attachment);
                }
            }
            AdapterEvent::CapabilitiesChanged { attachment } => {
                self.capabilities_changed(adapter, &attachment);
            }
            // Binding from native signals (§6.7) is the caller's ([`Pipelines::bind`]).
            AdapterEvent::NativeSignal(_) => {}
        }
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
            }
            core.sessions.remove(&session);
            core.engine.end_session(&session);
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
