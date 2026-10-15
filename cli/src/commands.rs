// SPDX-License-Identifier: Apache-2.0

//! The `oac` verbs that start a process or talk to the daemon (G9, #70;
//! `docs/planning/decisions/C2-process-model.md` §6): `start`, `mcp-shim`, and the connector
//! `oac hook` uses. The binary wires them; the daemon itself is [`crate::daemon`], which is
//! given everything it needs, and this module is where the real things are given to it: the
//! reference transport's peer, the operating system's key store, the system clocks, the
//! Claude adapter.
//!
//! **Key material is read here and nowhere near a shim or a hook handler.** Only `start`
//! builds a key store; `mcp-shim` and `hook` connect to the daemon and nothing else.

use std::io::{self, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use oac_adapter_claude::ClaudeAdapter;
use oac_core::adapter::ProviderAdapter;
use oac_core::clock::{Clock, SystemClock};
use oac_core::ids::Token;
use oac_core::keys::KeyStore;
use oac_core::pairing::MemoryPairingStore;
use oac_core::pipeline::{MonotonicClock, PipelineConfig};
use oac_core::sealing::MemoryAgreementKeyStore;
use oac_transport_zenoh::{PeerConfiguration, PeerTransport};

use crate::daemon::{Daemon, LogSink, Parts, StartError};
use crate::ipc::{ConnectError, Scope, label_is_valid};
use crate::keystore::{FallbackKeyStore, OsKeyStore};
use crate::shim;

/// Exit status for a usage error.
pub const EXIT_USAGE: i32 = 2;

/// The user's scope, with the namespace in use noted on `diagnostics` when it is not the
/// ordinary one (it is a test-isolation namespace; see `Scope::for_user_in`).
fn user_scope(who: &str, diagnostics: &mut dyn Write) -> io::Result<Scope> {
    let scope = Scope::for_user()?;
    if let Some(namespace) = scope.namespace() {
        let _ = writeln!(
            diagnostics,
            "oac {who}: using the test namespace {namespace:?}"
        );
    }
    Ok(scope)
}

/// `oac mcp-shim --harness <label>`: the stdio MCP server a harness starts.
pub fn mcp_shim(args: &[String]) -> i32 {
    let mut stderr = io::stderr();
    let label = match args {
        [flag, label] if flag == "--harness" && label_is_valid(label) => label.clone(),
        _ => {
            let _ = writeln!(stderr, "usage: oac mcp-shim --harness <claude|codex>");
            return EXIT_USAGE;
        }
    };
    let scope = match user_scope("mcp-shim", &mut stderr) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(stderr, "oac mcp-shim: {e}");
            return shim::EXIT_FAILED;
        }
    };
    let endpoint = scope.endpoint(&label);
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(stderr, "oac mcp-shim: {e}");
            return shim::EXIT_FAILED;
        }
    };
    runtime.block_on(async {
        match shim::connect(&endpoint).await {
            Ok(stream) => {
                let ended = shim::relay(stream, io::stdin(), io::stdout()).await;
                if ended == shim::Ended::DaemonClosed {
                    let _ = writeln!(
                        io::stderr(),
                        "oac mcp-shim: the daemon ended the connection"
                    );
                }
                shim::exit_status(ended)
            }
            Err(e) => {
                let _ = writeln!(io::stderr(), "{}", shim::unreachable_message(&e));
                shim::EXIT_FAILED
            }
        }
    })
}

/// How long a hook handler waits, after it has sent its line, for the daemon to close the
/// connection.
const LINGER: Duration = Duration::from_secs(3);

/// The hook handler's connection. When it is dropped, after the line is written, it waits
/// (for at most [`LINGER`]) for the daemon to close its end.
///
/// **Why it waits.** The daemon learns which harness process a connection belongs to by
/// reading the operating system's process table about the peer, **after** it accepts the
/// connection. A hook handler writes one line and has nothing more to do; if it exited at once
/// its process could be gone before the daemon looked, and the signal could not be paired
/// (the session would stay unbound, fail closed, but for no reason). The daemon closes a hook
/// connection as soon as the adapter has read the line, which is after the observation, so
/// waiting for that close keeps the handler alive exactly as long as it must be. The bound
/// keeps a daemon that never answers from holding the session's start.
struct Lingering(Option<crate::ipc::ClientStream>);

impl Write for Lingering {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.0.as_mut() {
            Some(s) => s.write(buf),
            None => Err(io::ErrorKind::BrokenPipe.into()),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self.0.as_mut() {
            Some(s) => s.flush(),
            None => Ok(()),
        }
    }
}

impl Drop for Lingering {
    fn drop(&mut self) {
        let Some(mut stream) = self.0.take() else {
            return;
        };
        let (done, closed) = std::sync::mpsc::channel();
        // A blocking read on its own thread: a synchronous Windows pipe handle has no read
        // timeout, and the handler must not wait past the bound.
        let _ = std::thread::Builder::new()
            .name("oac-hook-linger".into())
            .spawn(move || {
                let mut sink = [0u8; 64];
                while matches!(io::Read::read(&mut stream, &mut sink), Ok(n) if n > 0) {}
                let _ = done.send(());
            });
        let _ = closed.recv_timeout(LINGER);
    }
}

/// The connector `oac hook` uses: the blocking client of the daemon's endpoint for the
/// harness. A failure says how to start the daemon.
pub fn hook_connector(harness: &str) -> io::Result<Box<dyn Write>> {
    let mut stderr = io::stderr();
    let scope = user_scope("hook", &mut stderr)?;
    match scope.endpoint(harness).connect_blocking() {
        Ok(stream) => Ok(Box::new(Lingering(Some(stream)))),
        Err(ConnectError::NotRunning) => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the daemon is not running; start it with `oac start`",
        )),
        Err(e) => Err(io::Error::other(e.to_string())),
    }
}

/// `oac start [--detach]`.
pub fn start(args: &[String]) -> i32 {
    let mut stderr = io::stderr();
    let detach = match args {
        [] => false,
        [flag] if flag == "--detach" => true,
        _ => {
            let _ = writeln!(stderr, "usage: oac start [--detach]");
            return EXIT_USAGE;
        }
    };
    let scope = match user_scope("start", &mut stderr) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(stderr, "oac start: {e}");
            return 1;
        }
    };
    if detach {
        return start_detached(&scope, &mut stderr);
    }
    serve_in_foreground(scope, &mut stderr)
}

/// The device key store: the operating system's credential store and, on a Unix host with no
/// credential service, the encrypted file (C4 §10-§11).
///
/// Reads the passphrase variable and so must run before this process starts a thread.
fn key_store() -> Box<dyn KeyStore + Send> {
    #[cfg(unix)]
    {
        // SAFETY: called from `main` before any thread (the runtime included) is started.
        let file = unsafe { crate::keystore::FileKeyStore::from_environment() };
        Box::new(FallbackKeyStore::new(OsKeyStore::new(), file))
    }
    #[cfg(not(unix))]
    {
        Box::new(FallbackKeyStore::<OsKeyStore, OsKeyStore>::new(
            OsKeyStore::new(),
            None,
        ))
    }
}

fn serve_in_foreground(scope: Scope, stderr: &mut dyn Write) -> i32 {
    // Before any thread starts (the passphrase variable is overwritten and removed).
    let key_store = key_store();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(stderr, "oac start: {e}");
            return 1;
        }
    };
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let monotonic: MonotonicClock = Arc::new(Instant::now);
    let claude: Arc<dyn ProviderAdapter> = Arc::new(ClaudeAdapter::new());
    let parts = Parts {
        scope,
        key_store,
        pairing_store: Box::new(MemoryPairingStore::new()),
        agreement_store: Box::new(MemoryAgreementKeyStore::new()),
        principal: Token::parse("device").expect("a valid token"),
        transport: Arc::new(PeerTransport::new()),
        transport_configuration: PeerConfiguration::local().wrap(),
        clock,
        monotonic,
        config: PipelineConfig::default(),
        harnesses: vec![("claude".to_owned(), claude)],
        log: LogSink::new(io::stderr()),
    };
    runtime.block_on(async {
        match Daemon::start(parts).await {
            Ok(daemon) => {
                let _ = writeln!(
                    io::stderr(),
                    "oac start: the daemon is running ({})",
                    daemon.endpoints().join(", ")
                );
                wait_for_stop().await;
                let _ = writeln!(io::stderr(), "oac start: stopping");
                daemon.shutdown().await;
                0
            }
            Err(StartError::AlreadyRunning) => {
                let _ = writeln!(io::stderr(), "oac start: the daemon is already running");
                0
            }
            Err(e) => {
                let _ = writeln!(io::stderr(), "oac start: {e}");
                1
            }
        }
    })
}

/// Waits for Ctrl-C or, on Unix, SIGTERM.
async fn wait_for_stop() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

/// How long `--detach` waits for the daemon it started to serve.
const DETACH_WAIT: Duration = Duration::from_secs(20);

/// `oac start --detach`: runs `oac start` as a background process and returns once it serves.
///
/// Best effort on Windows (lead decision D4): a harness or a shell may have put this process
/// in a job object that ends its members with the session; the child asks to leave the job,
/// and where the system refuses, it runs in the job and ends with it.
fn start_detached(scope: &Scope, stderr: &mut dyn Write) -> i32 {
    // Already running: nothing to start.
    if scope.endpoint("claude").connect_blocking().is_ok() {
        let _ = writeln!(stderr, "oac start: the daemon is already running");
        return 0;
    }
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            let _ = writeln!(stderr, "oac start: {e}");
            return 1;
        }
    };
    let mut command = std::process::Command::new(exe);
    command
        .arg("start")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
        command.creation_flags(flags | CREATE_BREAKAWAY_FROM_JOB);
        if command.spawn().is_err() {
            // The job does not allow breakaway: start inside it.
            command.creation_flags(flags);
            return finish_detach(command.spawn(), scope, stderr);
        }
        finish_detach_started(scope, stderr)
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe and touches no memory of ours.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        finish_detach(command.spawn(), scope, stderr)
    }
}

#[cfg(windows)]
fn finish_detach_started(scope: &Scope, stderr: &mut dyn Write) -> i32 {
    wait_until_serving(scope, stderr)
}

fn finish_detach(
    spawned: io::Result<std::process::Child>,
    scope: &Scope,
    stderr: &mut dyn Write,
) -> i32 {
    match spawned {
        Ok(_child) => wait_until_serving(scope, stderr),
        Err(e) => {
            let _ = writeln!(stderr, "oac start: the daemon could not be started: {e}");
            1
        }
    }
}

/// Waits for the daemon just started to serve. This waits for a start-up to finish, once; it
/// is not polling for anything the daemon holds.
fn wait_until_serving(scope: &Scope, stderr: &mut dyn Write) -> i32 {
    let end = Instant::now() + DETACH_WAIT;
    while Instant::now() < end {
        if scope.endpoint("claude").connect_blocking().is_ok() {
            let _ = writeln!(stderr, "oac start: the daemon is running");
            return 0;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = writeln!(stderr, "oac start: the daemon did not start in time");
    1
}
