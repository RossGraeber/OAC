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
//!    from the pairing store, which is empty (default deny) until the pairing verbs exist (D8);
//! 3. builds the core `Pipelines` over the transport it was given, gives it the agreement
//!    keys a sealing transport needs, and starts the transport;
//! 4. adds each provider adapter, binds one IPC endpoint per harness label, and serves them.
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
//! (box 4). The lock is released last.
//!
//! The two timers the core leaves to its owner are one task here: the native-signal window
//! and the presence refresh. It wakes once a second and calls the core's two cheap
//! operations, which do nothing unless something is due: a signal can be held at any moment,
//! so a window ends at most a second late, and every adapter event drops what has expired as
//! well. It asks nobody for anything.

use std::fmt;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use oac_core::adapter::ProviderAdapter;
use oac_core::authorization::{AuthorizationEngine, DecisionLog, LogEntry, RestoreError};
use oac_core::clock::Clock;
use oac_core::ids::Token;
use oac_core::keys::{DeviceIdentity, DeviceKey, KeyStore, KeyStoreError};
use oac_core::pairing::PairingStore;
use oac_core::pipeline::{AdapterId, MonotonicClock, PipelineConfig, PipelineError, Pipelines};
use oac_core::sealing::{AgreementKeyError, AgreementKeyStore, AgreementKeys};
use oac_core::session_binding::{BindingLog, BindingLogEntry, PeerObservation};
use oac_core::transport::{Transport, TransportConfiguration};
use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::ipc::{Accept, Accepted, BindError, DaemonLock, Endpoint, Listener, Scope};
use crate::peer::{self, ProcessTable};
use crate::relay;

/// The longest the timer task sleeps.
const TICK: Duration = Duration::from_secs(1);

/// Where the daemon writes what an operator might read: findings, decisions and refusals.
/// Lines name connections, session ids and requirement ids only; they hold no message
/// content, native id, working directory or key ([IFC-TYP-092], [SEC-KEY-004]).
#[derive(Clone)]
pub struct LogSink(Arc<Mutex<Box<dyn Write + Send>>>);

impl LogSink {
    /// A sink over `writer`.
    pub fn new(writer: impl Write + Send + 'static) -> LogSink {
        LogSink(Arc::new(Mutex::new(Box::new(writer))))
    }

    /// A sink that drops everything.
    pub fn discard() -> LogSink {
        LogSink::new(std::io::sink())
    }

    /// Writes one line, ignoring a failing writer: a log must not stop the daemon.
    pub fn line(&self, text: &str) {
        let mut writer = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let _ = writeln!(writer, "oac-daemon: {text}");
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

/// What a daemon is made of.
pub struct Parts {
    /// The user's namespace for the IPC endpoints and the lock.
    pub scope: Scope,
    /// Where the device key lives.
    pub key_store: Box<dyn KeyStore + Send>,
    /// Where pairings, grants and relay settings live; empty means default deny.
    pub pairing_store: Box<dyn PairingStore + Send>,
    /// Where the agreement keys of a sealing transport live.
    pub agreement_store: Box<dyn AgreementKeyStore + Send>,
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
    lock: DaemonLock,
}

impl Daemon {
    /// Starts the daemon (see the module documentation for the order) and returns once every
    /// endpoint is bound.
    ///
    /// # Errors
    ///
    /// See [`StartError`]. Nothing keeps running after an error.
    pub async fn start(parts: Parts) -> Result<Daemon, StartError> {
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
        } = parts;

        // 1. One daemon per user, before the key is read.
        let lock = scope.acquire_lock().await?;

        // 2. Identity and policy: the one place the key is read.
        let key = DeviceKey::load_or_generate(&*key_store).map_err(StartError::Key)?;
        let identity = DeviceIdentity::new(key, principal);
        let engine = AuthorizationEngine::restore(
            &identity,
            &*pairing_store,
            clock.clone(),
            Box::new(log.clone()),
        )
        .map_err(StartError::Pairing)?;
        let agreement = AgreementKeys::load_or_generate(&*agreement_store, &identity, &clock.now())
            .map_err(StartError::Agreement)?;

        // 3. The core, over the transport.
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
        for (label, adapter) in harnesses {
            let id = pipelines.add_adapter(adapter);
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
                pipelines: pipelines.clone(),
                runtime: runtime.clone(),
                stopped: stopped.clone(),
                log: log.clone(),
            })));
        }
        tasks.push(runtime.spawn(timers(pipelines.clone(), stopped, log.clone())));
        log.line(&format!("serving {}", endpoints.join(", ")));
        Ok(Daemon {
            pipelines,
            stop,
            tasks,
            endpoints,
            lock,
        })
    }

    /// The core, for status and for tests.
    pub fn pipelines(&self) -> &Pipelines {
        &self.pipelines
    }

    /// The names of the endpoints served, for a diagnostic.
    pub fn endpoints(&self) -> &[String] {
        &self.endpoints
    }

    /// Stops serving, withdraws every announced session, shuts the adapters and the
    /// transport down, and releases the lock.
    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        for task in self.tasks {
            let _ = task.await;
        }
        shutdown_pipelines(self.pipelines).await;
        drop(self.lock);
    }
}

/// `Pipelines::shutdown` blocks (adapters close their connections, withdrawals are sent), so
/// it runs off the runtime's thread.
async fn shutdown_pipelines(pipelines: Arc<Pipelines>) {
    let _ = tokio::task::spawn_blocking(move || pipelines.shutdown()).await;
}

/// What an accept loop serves.
struct Serving {
    endpoint: Endpoint,
    listener: Listener,
    adapter: AdapterId,
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
                let (adapter, pipelines, runtime, log) = (
                    s.adapter,
                    s.pipelines.clone(),
                    s.runtime.clone(),
                    s.log.clone(),
                );
                s.runtime.spawn(async move {
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
    let ending = pipelines.clone();
    let ending_runtime = runtime.clone();
    let core_connection = relay::attach(
        &runtime,
        stream,
        Box::new(move |handle| {
            // The IPC layer saw this connection end: the core frees its place (and, for an
            // attachment, ends its binding). `disconnect` blocks, so not on the runtime.
            let (pipelines, handle) = (ending, handle.clone());
            ending_runtime.spawn_blocking(move || pipelines.disconnect(&handle));
        }),
    );
    let label = label.to_owned();
    let handed = tokio::task::spawn_blocking(move || {
        pipelines.connect_observed(adapter, core_connection, observation)
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

/// The owner's timers: native-signal windows and presence refreshes.
async fn timers(pipelines: Arc<Pipelines>, mut stopped: watch::Receiver<bool>, log: LogSink) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(TICK) => {}
            _ = stopped.changed() => return,
        }
        let pipelines = pipelines.clone();
        // Both calls take the core's locks and may run adapter callbacks.
        if tokio::task::spawn_blocking(move || {
            pipelines.expire_native_signals();
            pipelines.refresh_presence();
        })
        .await
        .is_err()
        {
            log.line("a timer task failed");
        }
    }
}
