// SPDX-License-Identifier: Apache-2.0

//! The `oac` verbs that start a process or talk to the daemon (G9, #70, G10, #71;
//! `docs/planning/decisions/C2-process-model.md` §6): `start`, `mcp-shim`, `stop`, and the
//! connector `oac hook` uses. The binary wires them; the daemon itself is [`crate::daemon`], which is
//! given everything it needs, and this module is where the real things are given to it: the
//! reference transport's peer, the operating system's key store, the system clocks, the
//! Claude adapter.
//!
//! **Key material is read here and nowhere near a shim or a hook handler.** Only `start`
//! builds a key store; `mcp-shim` and `hook` connect to the daemon and nothing else.

use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::{Arc, mpsc};
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

use crate::control::{self, StatusOutcome, StopOutcome};
use crate::daemon::{Daemon, LogSink, Parts, StartError};
use crate::doctor;
use crate::ipc::{CONTROL_LABEL, ConnectError, Scope, harness_label_is_valid};
use crate::keystore::{self, FallbackKeyStore, OsKeyStore};
use crate::logfile;
use crate::shim;
use crate::status;
use crate::verbs::{EXIT_FAILED, EXIT_NOT_RUNNING, EXIT_OK, EXIT_UNREACHABLE};

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
        [flag, label] if flag == "--harness" && harness_label_is_valid(label) => label.clone(),
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
                // E, the harness's own session id, for a Claude Code shim (its hello line).
                let hello = shim::hello_for(
                    &label,
                    std::env::var_os("CLAUDE_CODE_SESSION_ID").as_deref(),
                );
                let ended = shim::relay(stream, hello, io::stdin(), io::stdout()).await;
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
///
/// **How long a hook can take.** Windows: connecting waits at most `BUSY_WAIT` (5 s) while
/// every pipe instance is busy, writing one line is bounded by the pipe's buffer, and the
/// linger adds at most [`LINGER`] (3 s). Unix: the connect itself is not bounded here (a
/// full backlog of a daemon that does not accept blocks it), then the same write and linger.
/// So the handler can outlive 8 s only on a daemon that is stuck; Claude Code's own hook
/// timeout is the backstop that ends the handler (the session start is not held past it).
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

/// Whether `OAC_SCOPE` asks for a namespace: set and not empty (an empty value counts as
/// unset, as in `Scope::for_user`).
fn namespace_requested() -> bool {
    names_a_namespace(std::env::var_os("OAC_SCOPE").as_deref())
}

fn names_a_namespace(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|v| !v.is_empty())
}

/// `oac start [--detach]`.
pub fn start(args: &[String]) -> i32 {
    let mut stderr = io::stderr();
    let detach = match args {
        [] => false,
        [flag] if flag == "--detach" => true,
        [flag] if flag == "--help" || flag == "-h" => {
            let _ = io::stdout().write_all(start_help().as_bytes());
            return EXIT_OK;
        }
        _ => {
            let _ = writeln!(stderr, "usage: oac start [--detach]");
            return EXIT_USAGE;
        }
    };
    // The test namespace is for the in-process daemon of a test (`Scope::for_user_in`), never
    // for the binary: the device key is the user's one in every namespace, so a second daemon
    // in another namespace beside the real one would be a second peer with the same
    // identity. Refused before anything reads the key store.
    if namespace_requested() {
        let _ = writeln!(
            stderr,
            "oac start: OAC_SCOPE is set; the daemon does not run in a test namespace (it would share the device key with the real daemon). Unset OAC_SCOPE"
        );
        return 1;
    }
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

/// `oac start --help`: what the verb does and where its log is.
fn start_help() -> String {
    let log = logfile::default_log_path(None).map_or_else(
        || "(the per-user state directory cannot be found)".to_owned(),
        |p| p.display().to_string(),
    );
    format!(
        "usage: oac start [--detach]

         Runs the per-device daemon: in the foreground, or with --detach as a background process
         that this command leaves once it serves (stop it with `oac stop`).

         The daemon's log lines go to standard error and also to a file that survives a crash:
           {log}
         It is private to your user, and rotated at {} KiB to {} (one previous file is kept).
         It holds no message content, session id, working directory or key.
",
        logfile::MAX_LOG_BYTES / 1024,
        logfile::ROTATED_FILE_NAME
    )
}

/// How long `oac stop` waits for the daemon to say it has stopped.
pub const STOP_WAIT: Duration = Duration::from_secs(15);

/// `oac stop [--json]`: asks the running daemon to stop cleanly and waits for it to say so.
///
/// Exit status (`verbs`): 0 stopped, or there was no daemon to stop; 1 the daemon did not
/// confirm in [`STOP_WAIT`] (its pid is printed; nothing is killed) or failed; 2 usage;
/// 4 the endpoint is another user's, busy, or the daemon speaks another version.
pub fn stop(args: &[String]) -> i32 {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let json = match args {
        [] => false,
        [flag] if flag == "--json" => true,
        _ => {
            let _ = writeln!(stderr, "usage: oac stop [--json]");
            return EXIT_USAGE;
        }
    };
    let scope = match user_scope("stop", &mut stderr) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(stderr, "oac stop: {e}");
            return EXIT_FAILED;
        }
    };
    stop_with(&scope, json, STOP_WAIT, &mut stdout, &mut stderr)
}

/// [`stop`] for a given scope and wait, writing to the given streams. Standard output holds
/// the result (one canonical JSON object with `"oac_cli":1` under `--json`); standard error
/// holds the diagnostics.
pub fn stop_with(
    scope: &Scope,
    json: bool,
    wait: Duration,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let outcome = control::request_stop(&scope.endpoint(CONTROL_LABEL), wait);
    let pid_member = |pid: Option<u32>| pid.map_or(String::new(), |p| format!(",\"pid\":{p}"));
    let (status, human, object) = match &outcome {
        StopOutcome::NotRunning => (
            EXIT_OK,
            "oac stop: the daemon is not running".to_owned(),
            "{\"oac_cli\":1,\"ok\":true,\"stopped\":false,\"was_running\":false}".to_owned(),
        ),
        StopOutcome::Stopped { pid } => (
            EXIT_OK,
            "oac stop: the daemon stopped".to_owned(),
            format!(
                "{{\"oac_cli\":1,\"ok\":true,\"stopped\":true,\"was_running\":true{}}}",
                pid_member(*pid)
            ),
        ),
        StopOutcome::TimedOut { pid } => (
            EXIT_FAILED,
            format!(
                "oac stop: the daemon{} did not confirm that it stopped within {} s; nothing was killed",
                pid.map_or(String::new(), |p| format!(" (pid {p})")),
                wait.as_secs()
            ),
            format!(
                "{{\"oac_cli\":1,\"ok\":false,\"error\":\"timeout\",\"was_running\":true{}}}",
                pid_member(*pid)
            ),
        ),
        StopOutcome::Version => (
            EXIT_UNREACHABLE,
            "oac stop: the daemon speaks another version of the control protocol; stop it with the `oac` that started it"
                .to_owned(),
            "{\"oac_cli\":1,\"ok\":false,\"error\":\"version\",\"was_running\":true}".to_owned(),
        ),
        StopOutcome::Busy => (
            EXIT_UNREACHABLE,
            "oac stop: the daemon's control endpoint stayed busy; try again".to_owned(),
            "{\"oac_cli\":1,\"ok\":false,\"error\":\"busy\",\"was_running\":true}".to_owned(),
        ),
        StopOutcome::Unreachable(why) => (
            EXIT_UNREACHABLE,
            format!("oac stop: the daemon could not be reached: {why}"),
            format!(
                "{{\"oac_cli\":1,\"ok\":false,\"error\":\"unreachable\",\"detail\":{}}}",
                control::json_string(why)
            ),
        ),
        StopOutcome::Failed(why) => (
            EXIT_FAILED,
            format!("oac stop: {why}"),
            format!(
                "{{\"oac_cli\":1,\"ok\":false,\"error\":\"failed\",\"detail\":{}}}",
                control::json_string(why)
            ),
        ),
    };
    if json {
        let _ = writeln!(stdout, "{}", control::canonical_text(&object));
        if status != EXIT_OK {
            let _ = writeln!(stderr, "{human}");
        }
    } else if status == EXIT_OK {
        let _ = writeln!(stdout, "{human}");
    } else {
        let _ = writeln!(stderr, "{human}");
    }
    status
}

/// How long `oac status` waits for the daemon's answer.
pub const STATUS_WAIT: Duration = Duration::from_secs(10);

/// `oac status [--json]`: asks the running daemon for its status.
///
/// Exit status (`verbs`): 0 the daemon answered; 1 it answered something else, or not in time;
/// 2 usage; 3 the daemon is not running; 4 the endpoint is another user's, busy after retries,
/// or the daemon speaks another version of the control protocol.
pub fn status(args: &[String]) -> i32 {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let json = match args {
        [] => false,
        [flag] if flag == "--json" => true,
        _ => {
            let _ = writeln!(stderr, "usage: oac status [--json]");
            return EXIT_USAGE;
        }
    };
    let scope = match user_scope("status", &mut stderr) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(stderr, "oac status: {e}");
            return EXIT_FAILED;
        }
    };
    status_with(&scope, json, STATUS_WAIT, &mut stdout, &mut stderr)
}

/// [`status`] for a given scope and wait, writing to the given streams. Standard output holds
/// the result (the human form, or under `--json` one canonical object with `"oac_cli":1`);
/// standard error holds the diagnostics, and, for a daemon that is not running or not
/// reachable, the human message.
pub fn status_with(
    scope: &Scope,
    json: bool,
    wait: Duration,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    ask_status(
        "status",
        scope,
        json,
        wait,
        &status::render_json,
        &status::render_human,
        stdout,
        stderr,
    )
}

/// `oac sessions [--json]`: lists this device's own sessions, from the daemon's `status`
/// reply (the same client as `oac status`: same bounds, retry and exit codes).
///
/// Exit status (`verbs`): 0 the daemon answered, an empty list included; 1 it answered
/// something else, or not in time; 2 usage; 3 the daemon is not running; 4 the endpoint is
/// another user's, busy (after retries) or another control-protocol version.
pub fn sessions(args: &[String]) -> i32 {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let json = match args {
        [] => false,
        [flag] if flag == "--json" => true,
        _ => {
            let _ = writeln!(stderr, "usage: oac sessions [--json]");
            return EXIT_USAGE;
        }
    };
    let scope = match user_scope("sessions", &mut stderr) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(stderr, "oac sessions: {e}");
            return EXIT_FAILED;
        }
    };
    sessions_with(&scope, json, STATUS_WAIT, &mut stdout, &mut stderr)
}

/// [`sessions`] for a given scope and wait, writing to the given streams.
pub fn sessions_with(
    scope: &Scope,
    json: bool,
    wait: Duration,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    ask_status(
        "sessions",
        scope,
        json,
        wait,
        &status::render_sessions_json,
        &status::render_sessions_human,
        stdout,
        stderr,
    )
}

/// `oac doctor [--json]`: a preflight report, one line per check (PASS, WARN, FAIL or SKIP).
///
/// Exit status (`verbs`): 0 when no check FAILs (warnings and skips do not count); 1 when one
/// does; 2 usage. See [`crate::doctor`].
pub fn doctor(args: &[String]) -> i32 {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let json = match args {
        [] => false,
        [flag] if flag == "--json" => true,
        _ => {
            let _ = writeln!(stderr, "usage: oac doctor [--json]");
            return EXIT_USAGE;
        }
    };
    // A scope that cannot be made (a run directory that is not private) is a finding, not a
    // reason to stop: the report says so.
    let scope = Scope::for_user();
    let requested = std::env::var_os("OAC_SCOPE");
    if let Ok(s) = &scope
        && let Some(namespace) = s.namespace()
    {
        let _ = writeln!(stderr, "oac doctor: using the test namespace {namespace:?}");
    }
    // The credential service is decided from the raw `OAC_SCOPE`, not from whether the scope
    // could be made: a namespace that was asked for and failed must never reach the real
    // service (`doctor::credential_outcome`).
    let disable = std::env::var_os(doctor::NO_CREDENTIAL_PROBE_VARIABLE);
    let credential = move || {
        doctor::credential_outcome(
            requested.as_deref(),
            disable.as_deref(),
            &keystore::probe_credential_store,
        )
    };
    let path = std::env::var_os("PATH");
    let claude = move || doctor::probe_claude(path.as_deref(), doctor::CLAUDE_WAIT);
    let probes = doctor::Probes {
        status_wait: STATUS_WAIT,
        negotiation: std::env::var_os(doctor::NEGOTIATION_VARIABLE),
        credential: &credential,
        fallback_file: keystore::fallback_key_file_exists(),
        claude: &claude,
    };
    doctor_with(&scope, &probes, json, &mut stdout)
}

/// [`doctor`] for a given scope and probes, writing the report to `stdout`.
pub fn doctor_with(
    scope: &io::Result<Scope>,
    probes: &doctor::Probes<'_>,
    json: bool,
    stdout: &mut dyn Write,
) -> i32 {
    let checks = doctor::run(scope, probes);
    let report = if json {
        let mut line = doctor::render_json(&checks);
        line.push('\n');
        line
    } else {
        doctor::render_human(&checks)
    };
    let _ = stdout.write_all(report.as_bytes());
    if doctor::all_clear(&checks) {
        EXIT_OK
    } else {
        EXIT_FAILED
    }
}

type Render = dyn Fn(&oac_core::json::JsonObject) -> String;

#[allow(clippy::too_many_arguments)]
fn ask_status(
    verb: &str,
    scope: &Scope,
    json: bool,
    wait: Duration,
    render_json: &Render,
    render_human: &Render,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let outcome = control::request_status(&scope.endpoint(CONTROL_LABEL), wait);
    let (code, human, object) = match &outcome {
        StatusOutcome::Running(report) => {
            if json {
                let _ = writeln!(stdout, "{}", render_json(report));
            } else {
                let _ = stdout.write_all(render_human(report).as_bytes());
            }
            return EXIT_OK;
        }
        StatusOutcome::NotRunning => (
            EXIT_NOT_RUNNING,
            format!("oac {verb}: the daemon is not running; start it with `oac start`"),
            "{\"oac_cli\":1,\"ok\":false,\"running\":false,\"error\":\"not_running\"}".to_owned(),
        ),
        StatusOutcome::Version => (
            EXIT_UNREACHABLE,
            format!(
                "oac {verb}: the daemon speaks another version of the control protocol; use the `oac` that started it"
            ),
            "{\"oac_cli\":1,\"ok\":false,\"running\":true,\"error\":\"version\"}".to_owned(),
        ),
        StatusOutcome::Busy => (
            EXIT_UNREACHABLE,
            format!("oac {verb}: the daemon's control endpoint stayed busy; try again"),
            "{\"oac_cli\":1,\"ok\":false,\"running\":true,\"error\":\"busy\"}".to_owned(),
        ),
        StatusOutcome::Foreign => (
            EXIT_UNREACHABLE,
            format!(
                "oac {verb}: the daemon could not be reached: {}",
                ConnectError::Foreign
            ),
            format!(
                "{{\"oac_cli\":1,\"ok\":false,\"error\":\"unreachable\",\"detail\":{}}}",
                control::json_string(&ConnectError::Foreign.to_string())
            ),
        ),
        StatusOutcome::Unreachable(why) => (
            EXIT_UNREACHABLE,
            format!("oac {verb}: the daemon could not be reached: {why}"),
            format!(
                "{{\"oac_cli\":1,\"ok\":false,\"error\":\"unreachable\",\"detail\":{}}}",
                control::json_string(why)
            ),
        ),
        StatusOutcome::Failed(why) => (
            EXIT_FAILED,
            format!("oac {verb}: {why}"),
            format!(
                "{{\"oac_cli\":1,\"ok\":false,\"error\":\"failed\",\"detail\":{}}}",
                control::json_string(why)
            ),
        ),
    };
    if json {
        let _ = writeln!(stdout, "{}", control::canonical_text(&object));
    }
    let _ = writeln!(stderr, "{human}");
    code
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

/// The runtime `oac start` serves on: **multi-thread, with one worker, not current-thread.**
///
/// The reference transport's runtime panics on any thread that is inside a current-thread
/// tokio runtime (its synchronous calls `block_in_place`; threads of `spawn_blocking`
/// included), and the core calls the transport from the daemon's blocking tasks and, in
/// `Daemon::start`, from `Pipelines::start` inline on the thread that drives the future
/// (`block_on`'s). The in-memory transport of most tests has no such rule, so the panic is seen only with the
/// real transport: the first live run of `oac start` found it (G9 #70, slice 7, leg C), and
/// `tests/start_runtime.rs` starts the daemon over the real transport on this runtime.
///
/// # Errors
///
/// The operating system's error when the runtime's threads cannot be made.
pub fn daemon_runtime() -> io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
}

fn serve_in_foreground(scope: Scope, stderr: &mut dyn Write) -> i32 {
    // Before any thread starts (the passphrase variable is overwritten and removed).
    let key_store = key_store();
    let runtime = match daemon_runtime() {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(stderr, "oac start: {e}");
            return 1;
        }
    };
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let monotonic: MonotonicClock = Arc::new(Instant::now);
    let claude: Arc<dyn ProviderAdapter> = Arc::new(ClaudeAdapter::new());
    // The log file: the lines also go where a crash does not take them (G10, #71). It is
    // opened by `Daemon::start` once it holds the lock (a start that loses the race never
    // touches the running daemon's file); one that cannot be opened is one warning on
    // standard error, and the daemon runs without it. A panic leaves a line in it.
    let log = LogSink::new(io::stderr()).with_default_file_after_lock(None);
    log.install_panic_hook();
    let log_path = logfile::default_log_path(None);
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
        log: log.clone(),
    };
    runtime.block_on(async {
        match Daemon::start(parts).await {
            Ok(daemon) => {
                log.plain(&format!(
                    "oac start: the daemon is running ({}){}",
                    daemon.endpoints().join(", "),
                    log_path.map_or_else(String::new, |p| format!("; log: {}", p.display()))
                ));
                let requested = daemon.stop_requested();
                let why = tokio::select! {
                    () = wait_for_stop() => "stopping",
                    () = requested => "stopping (requested over the local IPC)",
                };
                log.plain(&format!("oac start: {why}"));
                daemon.shutdown().await;
                0
            }
            Err(StartError::AlreadyRunning) => {
                log.plain("oac start: the daemon is already running");
                0
            }
            Err(e) => {
                log.plain(&format!("oac start: {e}"));
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
///
/// The detached daemon has no console. Its standard error is a pipe that this command reads
/// until the daemon serves or ends, so a start that fails shows the daemon's own reason, and
/// the daemon's log file (`crate::logfile`) is named. It is stopped cleanly with `oac stop`
/// (G10, #71), over the control endpoint: a detached process on Windows gets no console
/// event, so that is the only clean shutdown there.
fn start_detached(scope: &Scope, stderr: &mut dyn Write) -> i32 {
    // Already running: nothing to start.
    // The probe is the control endpoint, which has no adapter: it takes no harness's slot.
    if scope.endpoint(CONTROL_LABEL).connect_blocking().is_ok() {
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
        .stdout(std::process::Stdio::null());
    let log = logfile::default_log_path(scope.namespace());
    start_detached_command(&mut command, scope, DETACH_WAIT, log.as_deref(), stderr)
}

/// Starts `command` as `--detach` starts the daemon (see [`spawn_detached`]) with its
/// standard error piped, and waits for the daemon to serve on `scope`'s control endpoint, for
/// at most `wait`. What the process writes to standard error is relayed to `stderr` (at most
/// [`RELAY_LIMIT`] bytes) until it serves or ends, and a failure names the log file of the
/// scope. Public so that a test runs the failure path with a stand-in daemon that fails;
/// `log` is the path a failure names.
#[doc(hidden)]
pub fn start_detached_command(
    command: &mut std::process::Command,
    scope: &Scope,
    wait: Duration,
    log: Option<&Path>,
    stderr: &mut dyn Write,
) -> i32 {
    command.stderr(std::process::Stdio::piped());
    match spawn_detached(command) {
        Ok(child) => wait_until_serving(
            child,
            || scope.endpoint(CONTROL_LABEL).connect_blocking().is_ok(),
            wait,
            log,
            stderr,
        ),
        Err(e) => {
            let _ = writeln!(stderr, "oac start: the daemon could not be started: {e}");
            1
        }
    }
}

/// The most the start relays of a child's standard error.
pub const RELAY_LIMIT: usize = 16 * 1024;

/// A child's standard error, read on a thread of its own so that no wait here depends on it.
/// The channel disconnects when the stream ends.
fn read_in_background(child: &mut std::process::Child) -> Option<mpsc::Receiver<Vec<u8>>> {
    let mut stream = child.stderr.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 2048];
        while let Ok(n) = stream.read(&mut buffer) {
            if n == 0 || tx.send(buffer[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    Some(rx)
}

/// What has been read of a child's standard error, written to `out` with control characters
/// other than line breaks and tabs made `?` (the text is a child's, shown on a terminal),
/// within `budget` bytes. `wait` is how long to wait for the first chunk when there is none;
/// returns whether the stream has ended.
fn relay(
    rx: &mpsc::Receiver<Vec<u8>>,
    budget: &mut usize,
    wait: Duration,
    out: &mut dyn Write,
) -> bool {
    let mut next = rx.recv_timeout(wait);
    loop {
        match next {
            Ok(chunk) => {
                if *budget > 0 {
                    let shown = chunk.len().min(*budget);
                    *budget -= shown;
                    let text: String = String::from_utf8_lossy(&chunk[..shown])
                        .chars()
                        .map(|c| {
                            if c.is_control() && !"\n\t".contains(c) {
                                '?'
                            } else {
                                c
                            }
                        })
                        .collect();
                    let _ = out.write_all(text.as_bytes());
                    if *budget == 0 {
                        let _ = writeln!(out, "\n(the daemon's output is cut here)");
                    }
                }
                next = rx.try_recv().map_err(|e| match e {
                    mpsc::TryRecvError::Empty => mpsc::RecvTimeoutError::Timeout,
                    mpsc::TryRecvError::Disconnected => mpsc::RecvTimeoutError::Disconnected,
                });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => return false,
            Err(mpsc::RecvTimeoutError::Disconnected) => return true,
        }
    }
}

/// Starts `command` as `--detach` starts the daemon: no console, its own process group (a
/// session on Unix), and on Windows outside the caller's job where the system allows it.
/// Public so that a test starts a stand-in daemon exactly as `oac start --detach` does.
#[cfg(windows)]
pub fn spawn_detached(command: &mut std::process::Command) -> io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
    command.creation_flags(flags | CREATE_BREAKAWAY_FROM_JOB);
    command.spawn().or_else(|_| {
        // The job does not allow breakaway: start inside it.
        command.creation_flags(flags);
        command.spawn()
    })
}

/// Starts `command` as `--detach` starts the daemon: no console, its own process group (a
/// session on Unix), and on Windows outside the caller's job where the system allows it.
/// Public so that a test starts a stand-in daemon exactly as `oac start --detach` does.
#[cfg(unix)]
pub fn spawn_detached(command: &mut std::process::Command) -> io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    // SAFETY: setsid is async-signal-safe and touches no memory of ours.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    command.spawn()
}

/// Waits for the daemon just started to serve (`serving`), or for the process to fail.
/// This waits for a start-up to finish, once; it is not polling for anything the daemon holds.
///
/// Holding the child lets a start-up that fails be reported at once, with its exit status,
/// and reaps it (no zombie) when it does. **A child that ended successfully is not a failure
/// yet:** a second `oac start` that finds the lock held exits 0 while the first daemon, which
/// takes the lock before it reads the key and starts the transport, may still be starting;
/// that one is waited for like our own. Only a non-zero status fails at once, and then only
/// when no daemon answers.
///
/// **The child's standard error is relayed** to `stderr` while it runs, and a failure says
/// where its log is (`log`), so the reason is the child's own and not a guess.
fn wait_until_serving(
    mut child: std::process::Child,
    mut serving: impl FnMut() -> bool,
    wait: Duration,
    log: Option<&Path>,
    stderr: &mut dyn Write,
) -> i32 {
    let output = read_in_background(&mut child);
    let mut budget = RELAY_LIMIT;
    let mut show = |stderr: &mut dyn Write, wait: Duration| -> bool {
        output
            .as_ref()
            .is_none_or(|rx| relay(rx, &mut budget, wait, stderr))
    };
    let log_hint = log.map_or_else(String::new, |p| format!("; its log: {}", p.display()));
    let end = Instant::now() + wait;
    let mut ended: Option<std::process::ExitStatus> = None;
    while Instant::now() < end {
        show(stderr, Duration::ZERO);
        if ended.is_none() {
            ended = child.try_wait().ok().flatten();
        }
        if serving() {
            let _ = match ended {
                Some(status) => writeln!(
                    stderr,
                    "oac start: the daemon is running (this start ended with {status}: another one was first)"
                ),
                None => writeln!(stderr, "oac start: the daemon is running"),
            };
            return 0;
        }
        if let Some(status) = ended
            && !status.success()
        {
            // The child has ended: take what it wrote last (its end closes the stream), but
            // do not wait for a grandchild that kept the stream open.
            let drain_end = Instant::now() + Duration::from_secs(2);
            while Instant::now() < drain_end && !show(stderr, Duration::from_millis(100)) {}
            let _ = writeln!(
                stderr,
                "oac start: the daemon ended at start-up ({status}){log_hint}"
            );
            return 1;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    show(stderr, Duration::ZERO);
    let _ = writeln!(
        stderr,
        "oac start: the daemon did not start in time (it may still be starting){log_hint}"
    );
    1
}

#[cfg(test)]
mod tests {
    use super::{names_a_namespace, wait_until_serving};
    use std::ffi::OsStr;
    use std::path::Path;
    use std::process::{Child, Command};
    use std::time::Duration;

    fn exits_with(code: i32) -> Child {
        #[cfg(windows)]
        let mut c = {
            let mut c = Command::new("cmd");
            c.args(["/C", &format!("exit {code}")]);
            c
        };
        #[cfg(unix)]
        let mut c = {
            let mut c = Command::new("sh");
            c.args(["-c", &format!("exit {code}")]);
            c
        };
        c.spawn().unwrap()
    }

    fn waited(child: Child, mut serving: impl FnMut() -> bool, wait: Duration) -> (i32, String) {
        let mut err = Vec::new();
        let status = wait_until_serving(child, &mut serving, wait, None, &mut err);
        (status, String::from_utf8(err).unwrap())
    }

    fn says_then_exits(code: i32, said: &str) -> Child {
        #[cfg(windows)]
        let mut c = {
            let mut c = Command::new("cmd");
            c.args(["/C", &format!("echo {said} 1>&2 & exit {code}")]);
            c
        };
        #[cfg(unix)]
        let mut c = {
            let mut c = Command::new("sh");
            c.args(["-c", &format!("echo '{said}' >&2; exit {code}")]);
            c
        };
        c.stderr(std::process::Stdio::piped());
        c.spawn().unwrap()
    }

    #[test]
    fn a_start_that_failed_relays_the_childs_reason_and_names_the_log() {
        let mut err = Vec::new();
        let status = wait_until_serving(
            says_then_exits(3, "the device key: not here"),
            || false,
            Duration::from_secs(20),
            Some(Path::new("some").join("daemon.log").as_path()),
            &mut err,
        );
        let said = String::from_utf8(err).unwrap();
        assert_eq!(status, 1);
        assert!(said.contains("the device key: not here"), "{said}");
        assert!(said.contains("ended at start-up"), "{said}");
        assert!(said.contains("daemon.log"), "{said}");
        assert!(!said.contains("in a terminal"), "{said}");
    }

    #[test]
    fn a_start_that_failed_is_reported_at_once_with_its_status() {
        let began = std::time::Instant::now();
        let (status, said) = waited(exits_with(3), || false, Duration::from_secs(20));
        assert_eq!(status, 1);
        assert!(said.contains("ended at start-up"), "{said}");
        assert!(began.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_start_that_exited_cleanly_because_another_daemon_is_first_waits_for_that_daemon() {
        // The first daemon is still starting (it holds the lock; it is not serving yet): this
        // start ended with 0, and the wait goes on until the endpoint answers.
        let mut calls = 0;
        // Already ended when the wait begins, so that no host speed changes what is said.
        let mut child = exits_with(0);
        child.wait().unwrap();
        let (status, said) = waited(
            child,
            || {
                calls += 1;
                calls > 8
            },
            Duration::from_secs(20),
        );
        assert_eq!(status, 0, "{said}");
        assert!(said.contains("another one was first"), "{said}");
    }

    #[test]
    fn a_start_that_exited_cleanly_and_nothing_answers_does_not_report_a_daemon() {
        let (status, said) = waited(exits_with(0), || false, Duration::from_millis(600));
        assert_eq!(status, 1);
        assert!(said.contains("did not start in time"), "{said}");
    }

    #[test]
    fn a_failed_start_with_a_daemon_answering_is_the_daemon_running() {
        let (status, _) = waited(exits_with(3), || true, Duration::from_secs(5));
        assert_eq!(status, 0);
    }

    #[test]
    fn an_unset_or_empty_scope_is_no_namespace_and_anything_else_is() {
        assert!(!names_a_namespace(None));
        assert!(!names_a_namespace(Some(OsStr::new(""))));
        assert!(names_a_namespace(Some(OsStr::new("x"))));
        assert!(names_a_namespace(Some(OsStr::new("NOT A NAME"))));
    }
}
