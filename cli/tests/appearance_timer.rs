// SPDX-License-Identifier: Apache-2.0

//! The daemon's timer begins a carrier-appearance set within the bound (#473 item 1;
//! [IFC-TRN-123], [IFC-TRN-125]).
//!
//! In production the owner of the core's timers is what delivers the 2 s bound: a set held
//! back by the 1 s hold-off begins when `oac_cli::daemon`'s `timers` task next wakes, and
//! that task arms itself at the instant the core names (`Pipelines::next_appearance_set_in`).
//! The core's own tests call `begin_due_appearance_set` by hand at that instant, so they do
//! not show that the daemon does. This test starts the real [`Daemon`] (its `timers` task,
//! its spawn of that task, its wait computation) over a transport double that reports
//! carrier appearances, and shows that the second set begins at the hold-off's end, no later
//! than the bound after the appearance that asked for it.
//!
//! **The clock.** The runtime is a current-thread one with tokio's clock paused
//! (`start_paused`), and the core's monotonic clock (`Parts::monotonic`) reads that same
//! paused clock, so the daemon's sleep and the core's hold-off are one clock, as
//! `Pipelines::next_appearance_set_in` intends. Nothing sleeps for real: virtual time moves
//! only when the runtime has nothing to do but wait for a timer, and then it moves exactly to
//! that timer. A `spawn_blocking` task in flight holds the clock still (tokio's `test-util`
//! does not auto-advance while one runs), and the daemon's timer step is one, so the clock
//! cannot jump past it. The test waits on a channel the transport double sends on, which is
//! the event itself; a virtual-time `timeout` around that wait only turns a daemon that never
//! begins the set into a failure instead of a hang.
//!
//! The production daemon runs on the multi-thread runtime of `commands::daemon_runtime`; the
//! paused clock needs a current-thread one. The `timers` task is the same code on both. The
//! double is not the reference transport, which refuses a current-thread runtime; the double
//! does not, so the current-thread runtime is allowed.
//!
//! Three phases of the daemon's own tick are run (the appearances arrive with the whole tick
//! still to run, with 300 ms of it gone, and 5 ms into it), because the daemon's sleep in
//! flight when an appearance arrives is not shortened: only the sleep after it is. In the
//! last phase the wait that remains is under the timer's 10 ms floor, so the set begins 5 ms
//! after the instant the core named, which is what the allowance below is for.
//!
//! After the second set a third appearance is held back in turn: no set begins less than the
//! hold-off after the one before it ([IFC-TRN-125]).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use oac_cli::daemon::{Daemon, LogSink, Parts};
use oac_cli::ipc::Scope;
use oac_core::adapter::{
    AdapterCapabilities, AdapterEventHandler, Attachment, Connection, HandOff, HandOffOutcome,
    ProviderAdapter, RequestSink,
};
use oac_core::authorization::{Grant, LocalSide, OperatorConfirmed, PeerSide};
use oac_core::clock::{Clock, SystemClock};
use oac_core::health::{HealthState, HealthStatus};
use oac_core::ids::{KeyId, SessionId, Token};
use oac_core::keys::MemoryKeyStore;
use oac_core::keys::{DeviceIdentity, DeviceKey};
use oac_core::pairing::{MemoryPairingStore, PairedPeer};
use oac_core::pipeline::{
    CARRIER_APPEARANCE_BOUND, CARRIER_APPEARANCE_HOLD_OFF, MonotonicClock, PipelineConfig,
};
use oac_core::sealing::MemoryAgreementKeyStore;
use oac_core::transport::{
    CarrierHandle, Deadline, Destination, InboundHandler, Payload, PayloadKind, PresenceEvent,
    PresenceHandler, PublishResult, Reach, Subscription, Transport, TransportCapabilities,
    TransportConfiguration, TransportError,
};

/// How long, in virtual time, the test waits for the daemon to begin the set before it
/// fails: well past the bound, and never waited on a passing run.
const FAILING_RUN_BOUND: Duration = Duration::from_secs(10);

/// The daemon's timer waits at least 10 ms ([`oac_cli::daemon`]'s clamp), and tokio's timer
/// has 1 ms granularity: the set begins at the hold-off's end plus at most this.
const SLACK: Duration = Duration::from_millis(11);

/// The tick of the daemon's timer: a copy of `TICK` in `cli/src/daemon.rs`, which is private.
/// The three phases below are chosen against it (the first sleep is one tick long), so a
/// change to `TICK` there needs this and the phases changed with it.
const TICK: Duration = Duration::from_secs(1);

/// The core's monotonic clock: tokio's clock, which is paused in these tests. Blocking
/// tasks run with the runtime entered, so they read the same paused clock.
fn monotonic() -> MonotonicClock {
    Arc::new(|| tokio::time::Instant::now().into_std())
}

// ---- a transport that reports appearances -------------------------------------------------

/// A transport double: it takes presence records and sends the monotonic instant of each on
/// a channel, and it reports a carrier appearance to the watching core when told to.
struct AppearingTransport {
    handler: Mutex<Option<PresenceHandler>>,
    began: UnboundedSender<Instant>,
    now: MonotonicClock,
}

impl AppearingTransport {
    fn new(now: MonotonicClock) -> (Arc<AppearingTransport>, UnboundedReceiver<Instant>) {
        let (tx, rx) = unbounded_channel();
        (
            Arc::new(AppearingTransport {
                handler: Mutex::new(None),
                began: tx,
                now,
            }),
            rx,
        )
    }

    /// The transport's report that the link `link` became usable ([IFC-TRN-120]); it carries
    /// the handle only ([IFC-TRN-122]). Runs the core's handler on this thread, as a
    /// transport does on its own.
    fn appear(&self, link: &str) {
        let handler = self.handler.lock().unwrap().clone().expect("watched");
        handler(PresenceEvent::CarrierAppeared {
            carrier: CarrierHandle::from_opaque(link.as_bytes().to_vec()),
        });
    }
}

impl Transport for AppearingTransport {
    fn start(
        &self,
        _local_device: &KeyId,
        _configuration: TransportConfiguration,
    ) -> Result<TransportCapabilities, TransportError> {
        Ok(TransportCapabilities {
            reliability: false,
            persistence: false,
            offline_queueing: false,
            ordering: true,
            multicast_discovery: false,
            routing_federation: false,
            reach: Reach::CrossImplementation,
            // [IFC-TRN-081]: presence is kept off on a transport that cannot restrict it.
            destination_restricted: true,
            max_payload_octets: 65_536 * 4,
            sealing: false,
        })
    }

    fn publish(&self, _: &Destination, _: Payload, _: Deadline) -> PublishResult {
        PublishResult::NotTaken
    }

    fn subscribe(
        &self,
        _destination: &Destination,
        _handler: InboundHandler,
    ) -> Result<Subscription, TransportError> {
        Ok(Subscription::new(|| {}))
    }

    fn send_presence(
        &self,
        destination: &Destination,
        payload: Payload,
        _deadline: Deadline,
    ) -> PublishResult {
        if payload.kind() != PayloadKind::Presence || !matches!(destination, Destination::Device(_))
        {
            return PublishResult::NotTaken;
        }
        // The instant is read on the clock the core reads, at the time the record is taken.
        let _ = self.began.send((self.now)());
        PublishResult::Taken
    }

    fn watch_presence(&self, handler: PresenceHandler) -> Result<(), TransportError> {
        *self.handler.lock().unwrap() = Some(handler);
        Ok(())
    }

    fn health(&self) -> HealthStatus {
        HealthStatus::new(HealthState::Healthy)
    }

    fn shutdown(&self) {
        *self.handler.lock().unwrap() = None;
    }
}

// ---- an adapter that takes a connection as an attachment ----------------------------------

/// The least adapter that lets a session bind: every connection it takes is an attachment.
struct BindingAdapter {
    events: Mutex<Option<AdapterEventHandler>>,
}

impl ProviderAdapter for BindingAdapter {
    fn take_connection(&self, connection: Connection) {
        let (attachment, _, _) = connection.into_parts();
        let handler = self.events.lock().unwrap().clone();
        if let Some(h) = handler {
            h(oac_core::adapter::AdapterEvent::AttachmentOpened {
                attachment,
                cross_check: None,
            });
        }
    }

    fn watch_attachments(&self, handler: AdapterEventHandler) {
        *self.events.lock().unwrap() = Some(handler);
    }

    fn set_binding(&self, _attachment: &Attachment, _session: Option<SessionId>) {}

    fn capabilities(&self, _attachment: &Attachment) -> AdapterCapabilities {
        AdapterCapabilities {
            active_inbound: true,
            content_types: Vec::new(),
            max_envelope_octets: None,
        }
    }

    fn deliver(&self, _hand_off: HandOff) -> HandOffOutcome {
        HandOffOutcome::Completed
    }

    fn accept_requests(&self, _sink: Arc<dyn RequestSink>) {}

    fn health(&self) -> HealthStatus {
        HealthStatus::new(HealthState::Healthy)
    }

    fn shutdown(&self) {}
}

// ---- the test -----------------------------------------------------------------------------

/// Removes the namespace directory a Unix test made under the user's run directory (Windows:
/// nothing is made), when the test ends however it ends.
struct NamespaceDir(#[cfg(unix)] Option<std::path::PathBuf>);

impl Drop for NamespaceDir {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(dir) = self.0.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

fn namespace_dir(scope: &Scope) -> NamespaceDir {
    #[cfg(unix)]
    {
        NamespaceDir(
            std::path::Path::new(&scope.endpoint("claude").display_name())
                .parent()
                .map(std::path::Path::to_path_buf),
        )
    }
    #[cfg(not(unix))]
    {
        let _ = scope;
        NamespaceDir()
    }
}

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Runs the scenario with the daemon's tick `offset` of the way gone when the appearances
/// arrive.
async fn the_daemon_begins_the_held_back_set_within_the_bound(offset: Duration) {
    let namespace = format!(
        "at{}n{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    );
    let scope = Scope::for_user_in(Some(&namespace)).unwrap();
    let _dir = namespace_dir(&scope);
    let (transport, mut began) = AppearingTransport::new(monotonic());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let daemon = Daemon::start(Parts {
        scope,
        key_store: Box::new(MemoryKeyStore::new()),
        pairing_store: Box::new(MemoryPairingStore::new()),
        agreement_store: Box::new(MemoryAgreementKeyStore::new()),
        principal: Token::parse("appearance").unwrap(),
        transport: transport.clone(),
        transport_configuration: TransportConfiguration::new(()),
        clock,
        monotonic: monotonic(),
        config: PipelineConfig::default(),
        harnesses: Vec::new(),
        log: LogSink::discard(),
        codex: None,
    })
    .await
    .expect("the daemon starts");
    let pipes = daemon.pipelines();

    // A trusted device, and a session of the daemon's that a grant lets that device's session
    // write to, so that a set has an announcement to pass ([SEC-AUZ-011]).
    let peer_key = DeviceIdentity::new(DeviceKey::generate(), Token::parse("peer").unwrap());
    let peer = PairedPeer::by_key_id_comparison(
        peer_key.principal().clone(),
        *peer_key.public_key(),
        peer_key.key_id(),
        SystemClock.now(),
        OperatorConfirmed::by_operator(),
    )
    .unwrap();
    let store = MemoryPairingStore::new();
    pipes.with_engine(|e| e.pair(peer, &store).unwrap());
    let adapter = pipes.add_adapter(Arc::new(BindingAdapter {
        events: Mutex::new(None),
    }));
    let session = SessionId::from_random_octets([2; 16]);
    pipes.with_engine(|e| {
        e.add_grant(
            Grant::Inbound {
                writer: PeerSide::session(
                    peer_key.key_id().clone(),
                    SessionId::from_random_octets([1; 16]),
                ),
                target: LocalSide::Session(session.clone()),
            },
            OperatorConfirmed::by_operator(),
            &store,
        )
        .unwrap();
    });
    let connection = Connection::accept(Default::default(), Default::default());
    let attachment = connection.handle().clone();
    pipes.connect(adapter, connection).unwrap();
    let record = pipes
        .device()
        .register(
            session,
            Token::parse("test-harness").unwrap(),
            "native-2",
            "/work",
            SystemClock.now(),
        )
        .unwrap();
    pipes.bind(&attachment, &record, None).unwrap();
    // The announcement made when the session bound, inline: nothing else has been sent.
    began.try_recv().expect("the announcement made at bind");
    assert!(began.try_recv().is_err());

    // Let the daemon's `timers` task start its first sleep (a tick), then move the clock
    // `offset` into that tick. The sleep does not end: `offset` is under a tick.
    tokio::task::yield_now().await;
    assert!(offset < TICK);
    tokio::time::advance(offset).await;

    // The first appearance begins a set at once, in the handler ([IFC-TRN-123]); the second
    // arrives in the hold-off, which that set started ([IFC-TRN-125]), so it is held back.
    transport.appear("link-1");
    let first = began
        .try_recv()
        .expect("the first appearance begins a set at once");
    transport.appear("link-2");
    assert!(
        began.try_recv().is_err(),
        "the second appearance is held back by the hold-off"
    );
    let due = pipes
        .next_appearance_set_at()
        .expect("a set is due for the second appearance");
    assert_eq!(due, first + CARRIER_APPEARANCE_HOLD_OFF);
    assert_eq!(
        pipes.next_appearance_set_in(),
        Some(CARRIER_APPEARANCE_HOLD_OFF)
    );

    // Nothing here moves the clock but the runtime waiting on the daemon's timer.
    let second = tokio::time::timeout(FAILING_RUN_BOUND, began.recv())
        .await
        .expect("the daemon's timer begins the held-back set")
        .expect("the transport double is alive");

    // At the hold-off's end, and within the bound after the appearance that asked for it.
    assert!(
        second >= due,
        "the set began {:?} before the hold-off ended",
        due - second
    );
    assert!(
        second - first <= CARRIER_APPEARANCE_BOUND,
        "the set began {:?} after the first, beyond the bound",
        second - first
    );
    // The timer was armed at the instant the core named, not at the next whole tick: the
    // sleep in flight when the appearance arrived ended early in its tick for every offset
    // but zero, and the daemon armed a shorter one after it (at least the timer's 10 ms).
    assert!(
        second - due <= SLACK,
        "the set began {:?} after the instant the core named",
        second - due
    );

    // [IFC-TRN-125]: a set has begun, so an appearance now is held back for the hold-off that
    // set started; no set begins before it ends, and the daemon's timer begins one then.
    transport.appear("link-3");
    assert!(
        began.try_recv().is_err(),
        "a set began in the hold-off after the second"
    );
    let third = tokio::time::timeout(FAILING_RUN_BOUND, began.recv())
        .await
        .expect("the daemon's timer begins the set for the third appearance")
        .expect("the transport double is alive");
    assert!(
        third >= second + CARRIER_APPEARANCE_HOLD_OFF,
        "a set began {:?} before the hold-off after the second ended",
        second + CARRIER_APPEARANCE_HOLD_OFF - third
    );
    assert!(
        third - second <= CARRIER_APPEARANCE_BOUND,
        "the third set began {:?} after the second, beyond the bound",
        third - second
    );
    assert_eq!(pipes.next_appearance_set_at(), None, "nothing is left due");

    daemon.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn the_daemon_begins_the_held_back_set_with_a_whole_tick_still_to_run() {
    the_daemon_begins_the_held_back_set_within_the_bound(Duration::ZERO).await;
}

#[tokio::test(start_paused = true)]
async fn the_daemon_begins_the_held_back_set_with_the_tick_partly_gone() {
    the_daemon_begins_the_held_back_set_within_the_bound(Duration::from_millis(300)).await;
}

#[tokio::test(start_paused = true)]
async fn the_daemon_begins_the_held_back_set_with_the_tick_just_begun() {
    the_daemon_begins_the_held_back_set_within_the_bound(Duration::from_millis(5)).await;
}
