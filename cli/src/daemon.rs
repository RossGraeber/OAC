// SPDX-License-Identifier: Apache-2.0

//! The per-device daemon (G9, #70; `docs/planning/decisions/C2-process-model.md` §1, §5):
//! the long-lived process that holds the device key, the policy and the transport, runs the
//! provider adapters, and serves the local IPC to the shims and hook handlers.
//!
//! [`Daemon::start`] does, in this order:
//!
//! 1. takes the one-daemon-per-user lock (`crate::ipc::DaemonLock`) **before anything reads
//!    the key**, so a second `oac start` stops here and creates no second peer (box 5);
//! 2. loads or makes the device key, the only place key material is read (box 2: the shims and
//!    the hook handler never see it), and builds the identity and the authorization engine
//!    from the pairing store (empty is default deny; `crate::device_pairing` adds to it, #437);
//! 3. builds the core `Pipelines` over the transport it was given, gives it the agreement
//!    keys a sealing transport needs, and starts the transport;
//! 4. adds each provider adapter, binds one IPC endpoint per harness label, and serves them;
//!    then binds the reserved `control` endpoint (`crate::control`, G10 #71), which has no
//!    adapter: a client asks the daemon to stop over it, and it is never given to the core.
//!
//! The transport, the stores, the clocks and the adapters are **given**, so a test runs the
//! whole daemon over the in-memory transport; the `oac` binary gives it the reference transport.
//!
//! **Each accepted connection** is authenticated by the local IPC (the user check), observed
//! (`crate::peer::observe_process`: the pairing key and working directory of its harness
//! process, from the operating system and never from the peer), relayed through a core
//! `Connection` (`crate::relay`) and handed to `Pipelines::connect_observed` with the
//! observation ([SC-ID-121]). When the IPC layer sees it end, `Pipelines::disconnect` runs
//! (`crate::relay` says when exactly), never because of a timeout. A shim that crashes ends
//! only its own connection: the device's presence and every other session go on (box 3).
//!
//! **Shutdown** stops the endpoints, then `Pipelines::shutdown`: every adapter closes its
//! connections, each announced session's withdrawal is sent, and the transport shuts down
//! (box 4). The lock is released, and only then is a client that asked for the stop over the
//! control endpoint told it has happened.
//!
//! The two timers the core leaves to its owner are one task here: the native-signal window
//! and the presence refresh. It wakes once a second and calls the core's two cheap
//! operations, which do nothing unless something is due: a signal can be held at any moment,
//! so a window ends at most a second late, and every adapter event drops what has expired as
//! well. It asks nobody for anything.

use std::fmt;
use std::future::Future;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oac_core::adapter::{ConnectionHandle, ProviderAdapter};
use oac_core::at_rest::AtRest;
use oac_core::authorization::{AuthorizationEngine, DecisionLog, LogEntry, RestoreError};
use oac_core::clock::Clock;
use oac_core::ids::{KeyId, Token};
use oac_core::keys::{DeviceIdentity, DeviceKey, KeyStore, KeyStoreError};
use oac_core::pairing::PairingStore;
use oac_core::pipeline::{AdapterId, MonotonicClock, PipelineConfig, PipelineError, Pipelines};
use oac_core::sealing::{AgreementKeyError, AgreementKeyStore, AgreementKeys};
use oac_core::session_binding::{BindingLog, BindingLogEntry, PeerObservation};
use oac_core::transport::{Transport, TransportConfiguration};
use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::codex::{CarrierDriver, CarrierWiring};
use crate::control::Control;
use crate::device_pairing::DevicePairing;
use crate::ipc::{
    Accept, Accepted, BindError, CONTROL_LABEL, DaemonLock, Endpoint, Listener, Scope,
    harness_label_is_valid,
};
use crate::logfile::LogFile;
use crate::peer::{self, ProcessTable};
use crate::relay;
use crate::status::DaemonStatus;

/// The longest the timer task sleeps.
const TICK: Duration = Duration::from_secs(1);

/// Where the daemon writes what an operator might read: findings, decisions and refusals.
/// Lines name connections, session ids and requirement ids only; they hold no message
/// content, native id, working directory or key ([IFC-TYP-092], [SEC-KEY-004]).
///
/// A sink writes to a stream (standard error for `oac start`) and, when it has one, also to
/// the log file (`crate::logfile`, G10 #71), so that the lines outlive the process. **A log
/// never stops the daemon:** a file that cannot be opened is a warning and the sink goes on
/// with the stream alone; a file that fails while the daemon runs is warned about once on the
/// stream and left.
#[derive(Clone)]
pub struct LogSink {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    file: Option<Arc<Mutex<FileLog>>>,
    tap: Option<Tap>,
}

/// Opens the log file when the daemon holds the lock.
type Opener = Box<dyn FnOnce() -> std::io::Result<LogFile> + Send>;

/// The file half of a sink.
struct FileLog {
    /// The file once it is open (never closed for good by a transient error).
    file: Option<LogFile>,
    /// What opens it, until [`LogSink::attach_file`] runs it.
    opener: Option<Opener>,
    /// Whether the current run of failed writes has been warned about.
    warned: bool,
}

/// What a connection's life looked like from the daemon, for a diagnostic tool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Diagnostic {
    /// A connection was accepted and its peer observed.
    Accepted {
        /// The harness label of the endpoint.
        label: String,
        /// The peer process the local IPC pinned at accept, when there is one.
        peer: Option<peer::ProcessId>,
        /// Whether the observation found a harness process (a pairing key); `false` means
        /// a native signal on this connection is unpairable.
        paired: bool,
    },
    /// The IPC layer saw the connection end (shown before the core is told).
    Ended {
        /// The harness label of the endpoint.
        label: String,
        /// The peer process, as at accept.
        peer: Option<peer::ProcessId>,
    },
}

/// A function that is shown each [`Diagnostic`].
pub type Tap = Arc<dyn Fn(&Diagnostic) + Send + Sync>;

impl LogSink {
    /// A sink over `writer`.
    pub fn new(writer: impl Write + Send + 'static) -> LogSink {
        LogSink {
            writer: Arc::new(Mutex::new(Box::new(writer))),
            file: None,
            tap: None,
        }
    }

    /// This sink, and every line also appended to `file` (already open).
    pub fn with_file(mut self, file: LogFile) -> LogSink {
        self.file = Some(Arc::new(Mutex::new(FileLog {
            file: Some(file),
            opener: None,
            warned: false,
        })));
        self
    }

    /// This sink, with a log file that `opener` opens when [`LogSink::attach_file`] is called
    /// (`Daemon::start` does, **once it holds the lock**): until then lines go to the stream
    /// only, so a start that loses the race to a running daemon never touches that daemon's
    /// file. If the opener fails, **one warning on the stream** and the daemon runs without it.
    pub fn with_file_opener(
        mut self,
        opener: impl FnOnce() -> std::io::Result<LogFile> + Send + 'static,
    ) -> LogSink {
        self.file = Some(Arc::new(Mutex::new(FileLog {
            file: None,
            opener: Some(Box::new(opener)),
            warned: false,
        })));
        self
    }

    /// [`LogSink::with_file_opener`] for the log file of `namespace` (`None`: the user's own)
    /// in the user's state directory.
    pub fn with_default_file_after_lock(self, namespace: Option<&str>) -> LogSink {
        let namespace = namespace.map(str::to_owned);
        self.with_file_opener(move || LogFile::open(namespace.as_deref()))
    }

    /// The path of the log file, once it is open; `None` for a sink with no file, one whose
    /// file is not open yet, and one whose file could not be opened.
    pub fn file_path(&self) -> Option<std::path::PathBuf> {
        let file = self.file.as_ref()?;
        let log = file.lock().unwrap_or_else(|e| e.into_inner());
        log.file.as_ref().map(LogFile::path)
    }

    /// Opens the log file, if the sink was given an opener. Called by `Daemon::start` right
    /// after it takes the lock.
    pub fn attach_file(&self) {
        let Some(file) = &self.file else { return };
        let failed = {
            let mut log = file.lock().unwrap_or_else(|e| e.into_inner());
            match log.opener.take().map(|open| open()) {
                Some(Ok(opened)) => {
                    log.file = Some(opened);
                    None
                }
                Some(Err(e)) => Some(e),
                None => None,
            }
        };
        if let Some(e) = failed {
            self.write_stream(&format!(
                "oac-daemon: warning: the log file could not be opened ({e}); continuing without it"
            ));
        }
    }

    /// Records that the process panicked, in the log file and on the stream, then lets the
    /// previous panic hook run. **Only the source location is written** (`file:line`), never
    /// the panic's message, which could carry what the log must not hold. The hook takes no
    /// lock it could wait on: a panic inside the sink must not hang.
    pub fn install_panic_hook(&self) {
        let sink = self.clone();
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let place = info.location().map_or_else(
                || "an unknown place".to_owned(),
                |l| format!("{}:{}", l.file(), l.line()),
            );
            let text = format!("oac-daemon: panicked at {place}");
            if let Some(file) = &sink.file
                && let Ok(mut log) = file.try_lock()
                && let Some(f) = log.file.as_mut()
            {
                let _ = f.append(&text);
            }
            if let Ok(mut writer) = sink.writer.try_lock() {
                let _ = writer.write_all(format!("{text}\n").as_bytes());
                let _ = writer.flush();
            }
            previous(info);
        }));
    }

    /// This sink, and `tap` shown every connection's accept and end. **For a diagnostic tool
    /// that watches a daemon it runs itself** (the live-leg daemon in `examples/`); `oac
    /// start` installs none. A tap is given process ids and whether the peer paired, never
    /// the pairing key, the working directory or anything a peer sent.
    pub fn with_tap(mut self, tap: Tap) -> LogSink {
        self.tap = Some(tap);
        self
    }

    fn diagnostic(&self, diagnostic: &Diagnostic) {
        if let Some(tap) = &self.tap {
            tap(diagnostic);
        }
    }

    /// A sink that drops everything.
    pub fn discard() -> LogSink {
        LogSink::new(std::io::sink())
    }

    /// Writes one line, ignoring a failing writer: a log must not stop the daemon.
    pub fn line(&self, text: &str) {
        self.plain(&format!("oac-daemon: {text}"));
    }

    /// Writes `text` as one line as it is (a line that is not the daemon's own, such as `oac
    /// start`'s). Each destination gets the whole line in **one write**, then a flush; the
    /// file first, so that a line the stream's reader never sees is still kept.
    pub fn plain(&self, text: &str) {
        if let Some(file) = &self.file {
            let warn = {
                let mut log = file.lock().unwrap_or_else(|e| e.into_inner());
                match log.file.as_mut().map(|f| f.append(text)) {
                    Some(Err(e)) if !log.warned => {
                        log.warned = true;
                        Some(e)
                    }
                    Some(Ok(())) => {
                        log.warned = false;
                        None
                    }
                    _ => None,
                }
            };
            if let Some(e) = warn {
                // Once per run of failures; the file is kept and tried again on the next line.
                self.write_stream(&format!(
                    "oac-daemon: warning: writing the log file failed ({e}); will keep trying"
                ));
            }
        }
        self.write_stream(text);
    }

    fn write_stream(&self, text: &str) {
        let mut writer = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        let _ = writer.write_all(format!("{text}\n").as_bytes());
        let _ = writer.flush();
    }
}

impl BindingLog for LogSink {
    fn record(&mut self, entry: BindingLogEntry) {
        self.line(&format!(
            "binding {:?} {} {:?} attachment={:?} session={:?}",
            entry.record.kind,
            entry.record.requirement,
            entry.result,
            entry.attachment,
            entry.session
        ));
    }
}

impl DecisionLog for LogSink {
    fn record(&mut self, entry: LogEntry) {
        self.line(&format!("decision {entry:?}"));
    }
}

/// Where the daemon's agreement keys come from (#437 S1b). The durable store is sealed under
/// values derived from the device key, which the daemon reads once, inside `Daemon::start`; so
/// the store cannot be built before the daemon has the identity, and `Parts` is given this
/// instead. Every [`AgreementKeyStore`] is one (it ignores the handle), so a test passes
/// `Box::new(MemoryAgreementKeyStore::new())` as before.
pub trait AgreementStoreSource: Send {
    /// The store, given the handle of the device whose keys it keeps. The handle is opaque: it
    /// seals and opens the store's file and fixes the device key id; it is not the seed.
    fn open(self: Box<Self>, at_rest: AtRest) -> Box<dyn AgreementKeyStore + Send>;
}

impl<S: AgreementKeyStore + Send + 'static> AgreementStoreSource for S {
    fn open(self: Box<Self>, _at_rest: AtRest) -> Box<dyn AgreementKeyStore + Send> {
        self
    }
}

/// What a daemon is made of.
pub struct Parts {
    /// The user's namespace for the IPC endpoints and the lock.
    pub scope: Scope,
    /// Where the device key lives.
    pub key_store: Box<dyn KeyStore + Send>,
    /// Where pairings, grants and relay settings live; empty means default deny.
    pub pairing_store: Box<dyn PairingStore + Send + Sync>,
    /// Where the agreement keys of a sealing transport live: an [`AgreementKeyStore`] as it is
    /// (the in-memory double of the tests), or a source that builds the store once the daemon
    /// has the device identity (`crate::agreement_file::AgreementFileSource`).
    pub agreement_store: Box<dyn AgreementStoreSource>,
    /// The device's principal (`spec/security.md` §5): an identifier token.
    pub principal: Token,
    /// The transport.
    pub transport: Arc<dyn Transport>,
    /// The transport's configuration.
    pub transport_configuration: TransportConfiguration,
    /// The wall clock the checks read.
    pub clock: Arc<dyn Clock>,
    /// The monotonic clock the transport's deadlines are read on.
    pub monotonic: MonotonicClock,
    /// The core's bounds.
    pub config: PipelineConfig,
    /// The adapters, each with the harness label of its endpoint.
    pub harnesses: Vec<(String, Arc<dyn ProviderAdapter>)>,
    /// Where findings and decisions go.
    pub log: LogSink,
    /// The Codex adapter's carrier opener (`crate::codex`, G8 S5): given, so that a test opens
    /// a carrier to a fake. `None` serves the `codex` endpoint (if there is one) with no
    /// carrier, which closes every shim connection. The adapter must be one of `harnesses`.
    pub codex: Option<CarrierWiring>,
}

/// Knobs of a daemon that a test turns; `oac start` uses the defaults.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Options {
    /// How long a `status` request waits for the transport's and the adapters' `health`
    /// before it is answered `timeout`.
    pub status_deadline: Duration,
}

impl Options {
    /// These options with `status_deadline` set.
    pub fn with_status_deadline(mut self, status_deadline: Duration) -> Options {
        self.status_deadline = status_deadline;
        self
    }
}

impl Default for Options {
    fn default() -> Options {
        Options {
            status_deadline: crate::control::STATUS_DEADLINE,
        }
    }
}

/// Why a daemon did not start.
#[derive(Debug)]
pub enum StartError {
    /// Another daemon of this user holds the lock.
    AlreadyRunning,
    /// The device key could not be read or made.
    Key(KeyStoreError),
    /// The pairing store could not be read.
    Pairing(RestoreError),
    /// The agreement keys could not be read or made.
    Agreement(AgreementKeyError),
    /// The core refused (the transport did not start, or its declaration is invalid).
    Pipelines(PipelineError),
    /// An endpoint could not be bound or the operating system refused.
    Io(String),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartError::AlreadyRunning => f.write_str("a daemon of this user is already running"),
            StartError::Key(e) => write!(f, "the device key: {e}"),
            StartError::Pairing(e) => write!(f, "the pairing store: {e}"),
            StartError::Agreement(e) => write!(f, "the agreement keys: {e}"),
            StartError::Pipelines(e) => write!(f, "the core: {e}"),
            StartError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StartError {}

impl From<BindError> for StartError {
    fn from(e: BindError) -> StartError {
        match e {
            BindError::AlreadyRunning => StartError::AlreadyRunning,
            BindError::Io(e) => StartError::Io(e.to_string()),
        }
    }
}

/// A running daemon.
pub struct Daemon {
    pipelines: Arc<Pipelines>,
    stop: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
    endpoints: Vec<String>,
    control: Arc<Control>,
    /// The pairing ops' state (#437, S3): shutdown waits for its tasks before the core is
    /// closed and the lock released.
    pairing: Arc<DevicePairing>,
    log: LogSink,
    /// The agreement-key store the daemon was started with, kept for every later save
    /// (`AgreementKeys::replace` and `erase_replaced` take it; #437 S1b).
    agreement_store: Box<dyn AgreementKeyStore + Send>,
    lock: DaemonLock,
    /// The store the daemon was started with, kept for every later save (#437, S1).
    pairing_store: Arc<dyn PairingStore + Send + Sync>,
    /// The key id a store written under another device key recorded, when it differs from
    /// this daemon's.
    trust_store_foreign_key: Option<KeyId>,
}

impl Daemon {
    /// Starts the daemon (see the module documentation for the order) and returns once every
    /// endpoint is bound.
    ///
    /// # Errors
    ///
    /// See [`StartError`]. Nothing keeps running after an error.
    pub async fn start(parts: Parts) -> Result<Daemon, StartError> {
        Daemon::start_with(parts, Options::default()).await
    }

    /// [`Daemon::start`] with `options`.
    ///
    /// # Errors
    ///
    /// See [`StartError`].
    pub async fn start_with(parts: Parts, options: Options) -> Result<Daemon, StartError> {
        let Parts {
            scope,
            key_store,
            pairing_store,
            agreement_store,
            principal,
            transport,
            transport_configuration,
            clock,
            monotonic,
            config,
            harnesses,
            log,
            codex,
        } = parts;

        // A harness cannot take the label of the control endpoint (or the lock's).
        if let Some((label, _)) = harnesses.iter().find(|(l, _)| !harness_label_is_valid(l)) {
            return Err(StartError::Io(format!(
                "{label:?} is not a harness label (`lock` and `{CONTROL_LABEL}` are reserved)"
            )));
        }

        // 1. One daemon per user, before the key is read.
        let started = Instant::now();
        let lock = scope.acquire_lock().await?;
        // The log file is opened only now: a start that lost the race to a running daemon
        // must not rotate, repair or otherwise touch that daemon's file.
        log.attach_file();

        // 2. Identity and policy: the one place the key is read.
        let key = DeviceKey::load_or_generate(&*key_store).map_err(StartError::Key)?;
        let identity = DeviceIdentity::new(key, principal);
        // The agreement store is sealed under values derived from this device key (#437 S1b).
        let agreement_store = agreement_store.open(identity.at_rest());
        let engine = AuthorizationEngine::restore(
            &identity,
            &*pairing_store,
            clock.clone(),
            Box::new(log.clone()),
        )
        .map_err(StartError::Pairing)?;
        // The daemon keeps the store for its lifetime: every later change of trust is saved
        // through it (#437). A file written under another device key is reported, not
        // refused: the peers in it stay trusted (lead decisions L6, L7).
        let pairing_store: Arc<dyn PairingStore + Send + Sync> = Arc::from(pairing_store);
        let trust_store_foreign_key = pairing_store.note_device_key(engine.own_key_id());
        if let Some(foreign) = &trust_store_foreign_key {
            log.line(&format!(
                "warning: the trust store was written under device key {foreign}, and this daemon runs {}; its peers trust the old key id and must pair with the new one again",
                engine.own_key_id()
            ));
        }
        let agreement = AgreementKeys::load_or_generate(&*agreement_store, &identity, &clock.now())
            .map_err(StartError::Agreement)?;

        // 3. The core, over the transport.
        let pairing_clock = clock.clone();
        let pipelines = Arc::new(Pipelines::new(
            identity, engine, clock, monotonic, transport, config,
        ));
        pipelines.set_binding_log(Box::new(log.clone()));
        pipelines
            .set_agreement_keys(agreement)
            .map_err(StartError::Pipelines)?;
        pipelines
            .start(transport_configuration)
            .map_err(StartError::Pipelines)?;

        // 4. The adapters and their endpoints.
        let runtime = Handle::current();
        let (stop, stopped) = watch::channel(false);
        let mut tasks: Vec<JoinHandle<()>> = Vec::new();
        let mut endpoints = Vec::new();
        let mut labelled: Vec<(AdapterId, String)> = Vec::new();
        let mut carriers: Vec<Arc<CarrierDriver>> = Vec::new();
        for (label, adapter) in harnesses {
            let id = pipelines.add_adapter(adapter.clone());
            // The Codex adapter, when its carrier opener was given: the driver connects its
            // carrier before a Codex shim is handed over (`crate::codex`).
            let carrier = codex
                .as_ref()
                .filter(|w| {
                    std::ptr::eq(
                        Arc::as_ptr(&w.adapter).cast::<()>(),
                        Arc::as_ptr(&adapter).cast::<()>(),
                    )
                })
                .map(|w| {
                    CarrierDriver::new(
                        w.clone(),
                        id,
                        pipelines.clone(),
                        runtime.clone(),
                        log.clone(),
                        stopped.clone(),
                    )
                });
            carriers.extend(carrier.clone());
            labelled.push((id, label.clone()));
            let endpoint = scope.endpoint(&label);
            let listener = match endpoint.serve(&lock).await {
                Ok(l) => l,
                Err(e) => {
                    let _ = stop.send(true);
                    for t in tasks {
                        t.abort();
                    }
                    shutdown_pipelines(pipelines).await;
                    return Err(e.into());
                }
            };
            endpoints.push(endpoint.display_name());
            tasks.push(runtime.spawn(accept_loop(Serving {
                endpoint,
                listener,
                adapter: id,
                carrier,
                pipelines: pipelines.clone(),
                runtime: runtime.clone(),
                stopped: stopped.clone(),
                log: log.clone(),
            })));
        }
        // 5. The control endpoint: the same lock, the same serve and accept, no adapter.
        let mut served: Vec<String> = labelled.iter().map(|(_, l)| l.clone()).collect();
        served.push(CONTROL_LABEL.to_owned());
        let status = Arc::new(DaemonStatus::new(
            pipelines.clone(),
            labelled,
            served,
            scope.namespace().map(str::to_owned),
            started,
            log.clone(),
        ));
        let pairing = DevicePairing::new(
            pipelines.clone(),
            pairing_store.clone(),
            pairing_clock,
            log.clone(),
            stopped.clone(),
        );
        let control = Control::new(
            log.clone(),
            status,
            options.status_deadline,
            pairing.clone(),
        );
        let control_endpoint = scope.endpoint(CONTROL_LABEL);
        match control_endpoint.serve(&lock).await {
            Ok(listener) => {
                endpoints.push(control_endpoint.display_name());
                tasks.push(runtime.spawn(control.clone().accept_loop(listener, stopped.clone())));
            }
            Err(e) => {
                let _ = stop.send(true);
                for t in tasks {
                    t.abort();
                }
                shutdown_pipelines(pipelines).await;
                return Err(e.into());
            }
        }
        tasks.push(runtime.spawn(timers(pipelines.clone(), stopped, log.clone())));
        log.line(&format!("serving {}", endpoints.join(", ")));
        // The Codex carrier at start, if the control socket is there: once, then only when a
        // Codex shim connects, never on a timer.
        for driver in carriers {
            tasks.push(runtime.spawn(async move {
                driver.ensure().await;
            }));
        }
        Ok(Daemon {
            pipelines,
            stop,
            tasks,
            endpoints,
            control,
            pairing,
            log,
            agreement_store,
            lock,
            pairing_store,
            trust_store_foreign_key,
        })
    }

    /// A future that completes when a client has asked this daemon to stop over the control
    /// endpoint (`oac stop`). The owner selects on it beside its own signals and then calls
    /// [`Daemon::shutdown`], which tells the requester when the stop has happened.
    pub fn stop_requested(&self) -> impl Future<Output = ()> + Send + 'static {
        self.control.stop_requested()
    }

    /// The agreement-key store the daemon keeps: the argument of every later save of the
    /// agreement keys (`AgreementKeys::replace`, `erase_replaced`).
    pub fn agreement_store(&self) -> &(dyn AgreementKeyStore + Send) {
        &*self.agreement_store
    }

    /// The pairing ops' state: the recent pairing events, and the operations the control ops
    /// are made of (#437, S3).
    pub fn pairing(&self) -> &Arc<DevicePairing> {
        &self.pairing
    }

    /// The core, for status and for tests.
    pub fn pipelines(&self) -> &Pipelines {
        &self.pipelines
    }

    /// The pairing store the daemon keeps: the argument of every engine call that saves
    /// (`pair`, `remove_key`, `admit_statement`, `add_grant` and the rest).
    pub fn pairing_store(&self) -> &Arc<dyn PairingStore + Send + Sync> {
        &self.pairing_store
    }

    /// The key id recorded in a trust store that was written under another device key, when
    /// it is not this daemon's: its peers trust a key id this device no longer has, and must
    /// pair with the new one (#437, L7). `None` when the store was written under this key, or
    /// records none.
    pub fn trust_store_foreign_key(&self) -> Option<&KeyId> {
        self.trust_store_foreign_key.as_ref()
    }

    /// The names of the endpoints served (the harness endpoints, then `control`), for a
    /// diagnostic.
    pub fn endpoints(&self) -> &[String] {
        &self.endpoints
    }

    /// Stops serving, withdraws every announced session, shuts the adapters and the
    /// transport down, releases the lock, and then tells each client that asked for the stop
    /// (`oac stop`) that it has happened.
    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        for task in self.tasks {
            let _ = task.await;
        }
        // Every pairing, unpair and save-retry task ends (they watch the stop signal), and
        // none starts again, before the core is closed and the lock released: none writes
        // the trust store after the lock is gone (#437, T11).
        self.pairing.close().await;
        if shutdown_pipelines(self.pipelines).await {
            self.log
                .line("shut down: withdrawals sent, transport shut down");
        } else {
            self.log
                .line("shutdown of the core failed: its task did not complete");
        }
        drop(self.lock);
        self.control.finish().await;
    }
}

/// `Pipelines::shutdown` blocks (adapters close their connections, withdrawals are sent), so
/// it runs off the runtime's thread.
/// Whether the blocking task completed.
async fn shutdown_pipelines(pipelines: Arc<Pipelines>) -> bool {
    tokio::task::spawn_blocking(move || pipelines.shutdown())
        .await
        .is_ok()
}

/// What an accept loop serves.
struct Serving {
    endpoint: Endpoint,
    listener: Listener,
    adapter: AdapterId,
    /// The Codex endpoint's carrier driver.
    carrier: Option<Arc<CarrierDriver>>,
    pipelines: Arc<Pipelines>,
    runtime: Handle,
    stopped: watch::Receiver<bool>,
    log: LogSink,
}

async fn accept_loop(mut s: Serving) {
    loop {
        let accepted = tokio::select! {
            a = s.listener.accept() => a,
            _ = s.stopped.changed() => return,
        };
        match accepted {
            Ok(Accept::Connection(connection)) => {
                let label = s.endpoint.label().to_owned();
                let (adapter, pipelines, runtime, log, carrier) = (
                    s.adapter,
                    s.pipelines.clone(),
                    s.runtime.clone(),
                    s.log.clone(),
                    s.carrier.clone(),
                );
                s.runtime.spawn(async move {
                    // The Codex carrier first, so that a shim is never handed to an adapter
                    // with no carrier when one can be had (a shim under no carrier is closed).
                    if let Some(carrier) = carrier {
                        carrier.ensure().await;
                    }
                    serve_connection(&label, adapter, connection, pipelines, runtime, log).await;
                });
            }
            Ok(Accept::Rejected(r)) => {
                s.log.line(&format!(
                    "refused a connection on {}: {}",
                    s.endpoint.label(),
                    r.reason
                ));
            }
            Err(e) => {
                s.log.line(&format!(
                    "the {} endpoint can no longer accept: {e}",
                    s.endpoint.label()
                ));
                return;
            }
        }
    }
}

/// Whether the core has been given a connection and whether it ended before that.
#[derive(Default)]
struct Gate {
    handed: bool,
    ended: Option<(ConnectionHandle, (LogSink, String))>,
}

/// Logs that the core has freed a connection's place and ended its binding, after
/// `disconnect` returned: the line carries the endpoint's label only, so a test or an
/// operator can wait on it instead of polling for the binding to go.
fn connection_freed(log: &LogSink, label: &str) {
    log.line(&format!("freed a connection on {label}"));
}

/// Observes the peer, relays its stream and gives the connection to the core.
async fn serve_connection(
    label: &str,
    adapter: AdapterId,
    connection: Accepted,
    pipelines: Arc<Pipelines>,
    runtime: Handle,
    log: LogSink,
) {
    let Accepted { stream, peer } = connection;
    let observation = observation_of(label, peer.process()).await;
    let process = peer.process();
    log.diagnostic(&Diagnostic::Accepted {
        label: label.to_owned(),
        peer: process,
        paired: observation.pairing_key.is_some(),
    });
    // The end of the connection can come before `connect_observed` has returned (a core that
    // takes longer than the relay's drain to take the connection): `disconnect` of a
    // connection the core has not registered yet frees nothing, and the place would leak. So
    // the end waits for the handover: whichever comes second runs the disconnect.
    let gate = Arc::new(Mutex::new(Gate::default()));
    let ending = pipelines.clone();
    let ending_runtime = runtime.clone();
    let ending_gate = gate.clone();
    let ending_log = log.clone();
    let ending_label = label.to_owned();
    let freed_label = label.to_owned();
    let core_connection = relay::attach(
        &runtime,
        stream,
        Box::new(move |handle| {
            // The IPC layer saw this connection end: the core frees its place (and, for an
            // attachment, ends its binding). `disconnect` blocks, so not on the runtime.
            let (pipelines, handle) = (ending.clone(), handle.clone());
            ending_log.diagnostic(&Diagnostic::Ended {
                label: ending_label,
                peer: process,
            });
            let freed = (ending_log.clone(), freed_label);
            let mut state = ending_gate.lock().unwrap_or_else(|e| e.into_inner());
            if state.handed {
                drop(state);
                ending_runtime.spawn_blocking(move || {
                    pipelines.disconnect(&handle);
                    connection_freed(&freed.0, &freed.1);
                });
            } else {
                state.ended = Some((handle, freed));
            }
        }),
    );
    let label = label.to_owned();
    let handing = pipelines.clone();
    let handed = tokio::task::spawn_blocking(move || {
        let result = handing.connect_observed(adapter, core_connection, observation);
        let ended = {
            let mut state = gate.lock().unwrap_or_else(|e| e.into_inner());
            state.handed = true;
            state.ended.take()
        };
        if let Some((handle, freed)) = ended {
            handing.disconnect(&handle);
            connection_freed(&freed.0, &freed.1);
        }
        result
    })
    .await;
    match handed {
        Ok(Ok(())) => {}
        Ok(Err(e)) => log.line(&format!("a connection on {label} was not taken: {e}")),
        Err(_) => log.line(&format!("a connection on {label} could not be handed over")),
    }
}

/// The core's observation of a peer: the harness process's key and working directory, from
/// the operating system. With no pinned peer process, only the label.
async fn observation_of(label: &str, process: Option<peer::ProcessId>) -> PeerObservation {
    let label = label.to_owned();
    let observed = tokio::task::spawn_blocking(move || {
        let table = peer::SystemTable::new().ok()?;
        Some(match process {
            Some(p) => peer::observe_process(&table as &dyn ProcessTable, &label, &p),
            None => PeerObservation {
                harness_label: oac_core::ids::Token::parse(&label),
                ..PeerObservation::none()
            },
        })
    })
    .await;
    observed.ok().flatten().unwrap_or_default()
}

/// The owner's timers: native-signal windows, presence refreshes, and the sets of
/// announcements that carrier appearances wait for ([IFC-TRN-123], [IFC-TRN-125]). A set
/// held back by the hold-off begins when this timer next wakes, which is at most at the
/// instant the hold-off ends (the wait is shortened to it), not at the next whole tick, so
/// that it begins within the 2 second bound. It runs before the other work, so a slow
/// refresh cannot delay it. The wait is read from the core, on the core's own clock.
async fn timers(pipelines: Arc<Pipelines>, mut stopped: watch::Receiver<bool>, log: LogSink) {
    let mut wait = TICK;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = stopped.changed() => return,
        }
        let pipelines = pipelines.clone();
        // These calls take the core's locks and may run adapter callbacks.
        match tokio::task::spawn_blocking(move || {
            pipelines.begin_due_appearance_set();
            pipelines.expire_native_signals();
            pipelines.refresh_presence();
            pipelines
                .next_appearance_set_in()
                .map_or(TICK, |d| d.clamp(Duration::from_millis(10), TICK))
        })
        .await
        {
            Ok(next) => wait = next,
            Err(_) => {
                wait = TICK;
                log.line("a timer task failed");
            }
        }
    }
}
