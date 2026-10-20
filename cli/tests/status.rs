// SPDX-License-Identifier: Apache-2.0

//! `oac status` and the control endpoint's `status` op, with real processes (G10, #71, slice
//! 3). Written before the verb.
//!
//! The daemon runs in this process, over the in-memory transport, in a test namespace;
//! everything that talks to it is a separate process: a real `oac status`, and, for the
//! session test, a copy of this test binary named `claude` that starts the real `oac
//! mcp-shim` and `oac hook` (as `tests/daemon.rs` does), so a session is really bound.
//! `oac start` itself is never run (it would make the user's device key). Every read from a
//! child is on a thread with a bound, and every child is killed when its guard is dropped
//! (a known Linux hang, #435).
//!
//! Proven here: the verb exits 0 against a running daemon and its `--json` is one canonical
//! object with the fields `docs/planning/v0.1/08-cli-and-deployment.md` §5 lists; exit 3 with
//! no daemon; exit 4 for a daemon of another protocol version or one that stays busy; exit 1
//! for one that does not answer; and a bound session is listed while its harness-native id
//! and its working directory appear nowhere in either output.

use std::future::Future;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use oac_adapter_claude::ClaudeAdapter;
use oac_cli::daemon::{Daemon, LogSink, Options, Parts};
use oac_cli::ipc::Scope;
use oac_cli::logfile::{LogFile, MAX_LOG_BYTES, log_directory};
use oac_core::adapter::{
    AdapterCapabilities, AdapterEventHandler, Attachment, Connection, HandOff, HandOffOutcome,
    LocalEventHandler, ProviderAdapter, RequestSink,
};
use oac_core::clock::{Clock, SystemClock};
use oac_core::health::HealthStatus;
use oac_core::ids::SessionId;
use oac_core::ids::Token;
use oac_core::json::{self, Json, JsonObject};
use oac_core::keys::MemoryKeyStore;
use oac_core::local_call::{CallAdmission, CallContext, LocalResult};
use oac_core::pairing::MemoryPairingStore;
use oac_core::pipeline::PipelineConfig;
use oac_core::sealing::MemoryAgreementKeyStore;
use oac_fake_claude::link::{read_frame, write_outbound};
use oac_fake_claude::{Config, FakeClaude, Phase};
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};
use tokio::runtime::Handle;

const ROLE: &str = "OAC_STATUS_TEST_ROLE";
const SESSION_TEST: &str =
    "a_bound_session_is_listed_and_neither_its_native_id_nor_its_working_directory_is_shown";
const WAIT: Duration = Duration::from_secs(30);
/// The native id and the working directory of the fake session: neither may be shown.
const NATIVE_ID: &str = "native-xyzzy-7731";
const WORK_NAME: &str = "work-distinct-q9z";

fn oac() -> &'static str {
    env!("CARGO_BIN_EXE_oac")
}

/// A current-thread runtime driven by a thread of its own (as in `tests/control.rs`).
struct Rt {
    handle: Handle,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Rt {
    fn new() -> Rt {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = rt.handle().clone();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let thread = std::thread::spawn(move || {
            rt.block_on(async {
                let _ = stopped.await;
            });
        });
        Rt {
            handle,
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    /// Runs `f` on the runtime and waits for it, but not for ever.
    fn run<T: Send + 'static>(&self, f: impl Future<Output = T> + Send + 'static) -> T {
        let (tx, rx) = mpsc::channel();
        self.handle.spawn(async move {
            let _ = tx.send(f.await);
        });
        rx.recv_timeout(WAIT)
            .expect("the runtime finished the task")
    }
}

impl Drop for Rt {
    fn drop(&mut self) {
        let _ = self.stop.take().unwrap().send(());
        let _ = self.thread.take().unwrap().join();
    }
}

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

/// Prints the daemon's log when the test fails.
struct DumpOnPanic(Captured);

impl Drop for DumpOnPanic {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("--- daemon log ---\n{}", self.0.text());
        }
    }
}

/// Removes the namespace directory a Unix test made (Windows: nothing is made).
struct NamespaceDir(#[cfg(unix)] Option<PathBuf>);

impl NamespaceDir {
    fn of(scope: &Scope) -> NamespaceDir {
        #[cfg(unix)]
        return NamespaceDir(
            Path::new(&scope.endpoint("claude").display_name())
                .parent()
                .map(Path::to_path_buf),
        );
        #[cfg(not(unix))]
        {
            let _ = scope;
            NamespaceDir()
        }
    }
}

impl Drop for NamespaceDir {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(dir) = self.0.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// A directory removed when the test ends however it ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("oac-status-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A daemon in this process, in its own namespace, with an owner loop like `oac start`'s.
struct Rig {
    _rt: Rt,
    namespace: String,
    log_file: PathBuf,
    log: Captured,
    _dump: DumpOnPanic,
    _dir: NamespaceDir,
    _state: TempDir,
}

impl Rig {
    fn start(tag: &str) -> Rig {
        Rig::start_with(tag, Arc::new(ClaudeAdapter::new()), Options::default())
    }

    fn start_with(tag: &str, adapter: Arc<dyn ProviderAdapter>, options: Options) -> Rig {
        let namespace = format!("st{}{tag}", std::process::id());
        let scope = Scope::for_user_in(Some(&namespace)).unwrap();
        let dir = NamespaceDir::of(&scope);
        let state = TempDir::new(&format!("state-{tag}"));
        let log = Captured::default();
        let dump = DumpOnPanic(log.clone());
        let opener_state = state.0.clone();
        let opener_namespace = namespace.clone();
        let sink = LogSink::new(log.clone()).with_file_opener(move || {
            LogFile::open_in(&opener_state, Some(&opener_namespace), MAX_LOG_BYTES)
        });
        let log_file = log_directory(&state.0, Some(&namespace))
            .unwrap()
            .join("daemon.log");
        let network = MemoryNetwork::new();
        let net = network.clone();
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let parts = Parts {
            scope: scope.clone(),
            key_store: Box::new(MemoryKeyStore::new()),
            pairing_store: Box::new(MemoryPairingStore::new()),
            agreement_store: Box::new(MemoryAgreementKeyStore::new()),
            principal: Token::parse("test").unwrap(),
            transport: Arc::new(MemoryTransport::new()),
            transport_configuration: MemoryConfiguration::wrap(&network),
            clock,
            monotonic: Arc::new(move || net.now()),
            config: PipelineConfig::default(),
            harnesses: vec![("claude".to_owned(), adapter)],
            log: sink,
        };
        let rt = Rt::new();
        let daemon = rt
            .run(Daemon::start_with(parts, options))
            .unwrap_or_else(|e| panic!("the daemon starts: {e}"));
        let requested = daemon.stop_requested();
        rt.handle.spawn(async move {
            requested.await;
            daemon.shutdown().await;
        });
        Rig {
            _rt: rt,
            namespace,
            log_file,
            log,
            _dump: dump,
            _dir: dir,
            _state: state,
        }
    }

    fn oac(&self, args: &[&str]) -> Finished {
        run_bounded(
            Command::new(oac())
                .args(args)
                .env("OAC_SCOPE", &self.namespace)
                .stdin(Stdio::null()),
            WAIT,
        )
    }
}

struct Finished {
    status: Option<ExitStatus>,
    stdout: String,
    stderr: String,
}

impl Finished {
    fn code(&self) -> Option<i32> {
        self.status.expect("the command ended in time").code()
    }
}

/// Runs a command with piped output, reading it on threads, and kills it after `wait`.
fn run_bounded(command: &mut Command, wait: Duration) -> Finished {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the command starts");
    let (out, err) = (
        output_reader(&mut child, true),
        output_reader(&mut child, false),
    );
    let status = wait_for_exit(&mut child, wait);
    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Finished {
        status,
        stdout: out.recv_timeout(Duration::from_secs(5)).unwrap_or_default(),
        stderr: err.recv_timeout(Duration::from_secs(5)).unwrap_or_default(),
    }
}

fn output_reader(child: &mut Child, stdout: bool) -> mpsc::Receiver<String> {
    let mut source: Box<dyn Read + Send> = if stdout {
        Box::new(child.stdout.take().unwrap())
    } else {
        Box::new(child.stderr.take().unwrap())
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = source.read_to_string(&mut text);
        let _ = tx.send(text);
    });
    rx
}

fn wait_for_exit(child: &mut Child, wait: Duration) -> Option<ExitStatus> {
    let end = Instant::now() + wait;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        if Instant::now() >= end {
            return None;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// A child killed and reaped when the test ends however it ends.
struct Guard(Child);

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// ---- reading the report ---------------------------------------------------------------------

fn object(text: &str) -> JsonObject {
    json::parse(text.trim_end().as_bytes())
        .unwrap_or_else(|e| panic!("not JSON ({e}): {text:?}"))
        .into_object()
        .unwrap_or_else(|| panic!("not an object: {text:?}"))
}

fn member<'a>(o: &'a JsonObject, name: &str) -> &'a Json {
    o.get(name)
        .unwrap_or_else(|| panic!("no member {name:?} in {:?}", o.names().collect::<Vec<_>>()))
}

fn string<'a>(o: &'a JsonObject, name: &str) -> &'a str {
    member(o, name).as_str().unwrap_or_else(|| panic!("{name}"))
}

fn number(o: &JsonObject, name: &str) -> u64 {
    member(o, name)
        .as_number()
        .and_then(|n| n.plain_integer_in(0, u64::MAX))
        .unwrap_or_else(|| panic!("{name} is not a plain integer"))
}

fn sub<'a>(o: &'a JsonObject, name: &str) -> &'a JsonObject {
    member(o, name)
        .as_object()
        .unwrap_or_else(|| panic!("{name} is not an object"))
}

// ---- the verb against a running daemon ------------------------------------------------------

#[test]
fn a_real_oac_status_reports_a_running_daemon_in_json_and_in_words() {
    let rig = Rig::start("a");

    let out = rig.oac(&["status", "--json"]);
    assert_eq!(out.code(), Some(0), "{}{}", out.stdout, out.stderr);
    // One canonical object on one line; the diagnostics are on standard error.
    assert_eq!(out.stdout.matches('\n').count(), 1, "{}", out.stdout);
    let report = object(&out.stdout);
    assert_eq!(
        oac_cli::control::canonical_text(out.stdout.trim_end()),
        out.stdout.trim_end(),
        "canonical"
    );
    assert!(out.stderr.contains(&rig.namespace), "{}", out.stderr);
    assert_eq!(number(&report, "oac_cli"), 1);
    assert_eq!(member(&report, "ok").as_bool(), Some(true));
    assert_eq!(member(&report, "running").as_bool(), Some(true));
    assert_eq!(number(&report, "pid"), u64::from(std::process::id()));
    assert_eq!(string(&report, "version"), env!("CARGO_PKG_VERSION"));
    assert_eq!(string(&report, "namespace"), rig.namespace);
    assert!(!string(&report, "device_id").is_empty());
    let _ = number(&report, "uptime_ms");
    let endpoints: Vec<&str> = member(&report, "endpoints")
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Json::as_str)
        .collect();
    assert_eq!(endpoints, ["claude", "control"]);
    let transport = sub(&report, "transport");
    assert_eq!(string(transport, "state"), "healthy");
    let adapters = member(&report, "adapters").as_array().unwrap();
    assert_eq!(adapters.len(), 1);
    let adapter = adapters[0].as_object().unwrap();
    assert_eq!(string(adapter, "label"), "claude");
    assert_eq!(string(adapter, "state"), "healthy");
    assert_eq!(number(&report, "sessions_total"), 0);
    assert!(member(&report, "sessions").as_array().unwrap().is_empty());
    let counts = sub(&report, "counts");
    for name in [
        "open_connections",
        "unbound_attachments",
        "pending_signals",
        "held_signals",
        "trusted_devices",
        "grants",
        "discarded_frames",
    ] {
        assert_eq!(number(counts, name), 0, "{name}");
    }
    assert_eq!(
        string(&report, "log"),
        rig.log_file.display().to_string(),
        "the daemon's log path"
    );
    assert!(rig.log_file.exists(), "the log file is there");

    // Words.
    let human = rig.oac(&["status"]);
    assert_eq!(human.code(), Some(0), "{}{}", human.stdout, human.stderr);
    let pid_text = format!("pid {}", std::process::id());
    let log_text = rig.log_file.display().to_string();
    for expected in [
        "oac daemon: running",
        pid_text.as_str(),
        "endpoints: claude, control",
        "transport: healthy",
        "claude: healthy",
        "sessions: 0",
        log_text.as_str(),
    ] {
        assert!(
            human.stdout.contains(expected),
            "{expected:?} in\n{}",
            human.stdout
        );
    }
    assert!(rig.log.text().contains("serving"), "{}", rig.log.text());

    // A bad argument is a usage error, and asks nobody.
    let usage = rig.oac(&["status", "--bogus"]);
    assert_eq!(usage.code(), Some(2), "{}", usage.stderr);
    assert!(
        usage.stderr.contains("usage: oac status [--json]"),
        "{}",
        usage.stderr
    );

    // The daemon was not hurt by any of it: a stop still stops it.
    let stop = rig.oac(&["stop"]);
    assert_eq!(stop.code(), Some(0), "{}", stop.stderr);
}

#[test]
fn with_no_daemon_oac_status_is_exit_3() {
    let namespace = format!("st{}none", std::process::id());
    for (args, json) in [(&["status"][..], false), (&["status", "--json"][..], true)] {
        let out = run_bounded(
            Command::new(oac())
                .args(args)
                .env("OAC_SCOPE", &namespace)
                .stdin(Stdio::null()),
            WAIT,
        );
        assert_eq!(out.code(), Some(3), "{}{}", out.stdout, out.stderr);
        assert!(
            out.stderr.contains("the daemon is not running"),
            "{}",
            out.stderr
        );
        assert!(out.stderr.contains("`oac start`"), "{}", out.stderr);
        if json {
            assert_eq!(
                out.stdout.trim_end(),
                r#"{"error":"not_running","oac_cli":1,"ok":false,"running":false}"#
            );
        } else {
            assert!(out.stdout.is_empty(), "{}", out.stdout);
        }
    }
}

// ---- the client against a daemon that misbehaves --------------------------------------------

/// A control endpoint that answers every request with `replies` and keeps each connection
/// open until the test ends (as in `tests/control.rs`).
fn fake_control(scope: &Scope, rt: &Rt, replies: &'static [&'static str]) {
    let scope = scope.clone();
    let (ready_tx, ready) = mpsc::channel();
    rt.handle.spawn(async move {
        use oac_cli::ipc::Accept;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let lock = scope.acquire_lock().await.unwrap();
        let mut listener = scope.endpoint("control").serve(&lock).await.unwrap();
        let _ = ready_tx.send(());
        let mut held = Vec::new();
        while let Ok(Accept::Connection(mut c)) = listener.accept().await {
            let mut buf = [0u8; 256];
            let _ = c.stream.read(&mut buf).await;
            for r in replies {
                let _ = c.stream.write_all(r.as_bytes()).await;
                let _ = c.stream.flush().await;
            }
            held.push(c);
        }
        drop((held, lock));
    });
    ready.recv_timeout(WAIT).expect("the fake serves");
}

fn status_against(
    replies: &'static [&'static str],
    json: bool,
    wait: Duration,
) -> (i32, String, String) {
    let rt = Rt::new();
    let scope = Scope::unique().unwrap();
    fake_control(&scope, &rt, replies);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = oac_cli::commands::status_with(&scope, json, wait, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn a_daemon_of_another_protocol_version_is_exit_4() {
    let (code, out, err) = status_against(
        &["{\"error\":\"version\",\"oac_control\":9,\"supported\":9}\n"],
        true,
        Duration::from_secs(10),
    );
    assert_eq!(code, 4);
    assert_eq!(
        out.trim_end(),
        r#"{"error":"version","oac_cli":1,"ok":false,"running":true}"#
    );
    assert!(err.contains("another version"), "{err}");
}

#[test]
fn a_daemon_that_stays_busy_is_asked_again_and_then_exit_4() {
    let began = Instant::now();
    let wait = Duration::from_millis(600);
    let (code, out, _) = status_against(&["{\"error\":\"busy\",\"oac_control\":1}\n"], true, wait);
    assert_eq!(code, 4);
    assert!(began.elapsed() >= wait, "it asked again until the deadline");
    assert_eq!(
        out.trim_end(),
        r#"{"error":"busy","oac_cli":1,"ok":false,"running":true}"#
    );
}

#[test]
fn a_daemon_that_never_answers_is_exit_1_within_the_wait() {
    let began = Instant::now();
    let (code, out, err) = status_against(&[], false, Duration::from_millis(400));
    assert_eq!(code, 1);
    assert!(began.elapsed() < Duration::from_secs(10));
    assert!(out.is_empty(), "{out}");
    assert!(err.contains("did not answer in time"), "{err}");
}

#[test]
fn an_answer_that_is_not_a_status_is_exit_1() {
    let (code, _, err) = status_against(
        &["{\"oac_control\":1,\"stopping\":true}\n"],
        false,
        Duration::from_secs(10),
    );
    assert_eq!(code, 1);
    assert!(err.contains("not a status"), "{err}");
}

// ---- a bound session ------------------------------------------------------------------------

/// One Claude Code session: a child process named `claude`, its command pipe, and the lines
/// it prints, read on a thread so that a wait for one is bounded.
struct Session {
    _child: Guard,
    commands: ChildStdin,
    lines: mpsc::Receiver<String>,
}

impl Session {
    fn start(dir: &Path, namespace: &str) -> Session {
        let bin = dir.join("bin");
        let work = dir.join(WORK_NAME);
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        let claude = bin.join(if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        });
        std::fs::copy(std::env::current_exe().unwrap(), &claude).unwrap();
        let mut child = Command::new(&claude)
            .args(["--exact", SESSION_TEST, "--nocapture", "--test-threads=1"])
            .env(ROLE, "session")
            .env("OAC_SCOPE", namespace)
            .env("CLAUDE_CODE_SESSION_ID", NATIVE_ID)
            .env("OAC_SESSION_WORK", &work)
            .env(
                "OAC_DEVICE_KEY_PASSPHRASE",
                "poisoned-passphrase-must-not-be-read",
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the session starts");
        let commands = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        if tx.send(line).is_err() {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
        });
        let mut s = Session {
            _child: Guard(child),
            commands,
            lines,
        };
        let ready = s.reply_with("READY");
        assert!(ready.starts_with("READY"), "{ready}");
        s
    }

    /// Reads lines until one holds `prefix` (the harness prints its own lines too), for at
    /// most [`WAIT`].
    fn reply_with(&mut self, prefix: &str) -> String {
        let end = Instant::now() + WAIT;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) => {
                    if let Some(i) = line.find(prefix) {
                        return line[i..].trim_end().to_owned();
                    }
                }
                Err(_) => panic!("no `{prefix}` from the session in time"),
            }
        }
    }

    fn command(&mut self, command: &str, expect: &str) -> String {
        writeln!(self.commands, "{command}").unwrap();
        self.commands.flush().unwrap();
        self.reply_with(expect)
    }
}

#[test]
fn a_bound_session_is_listed_and_neither_its_native_id_nor_its_working_directory_is_shown() {
    match std::env::var(ROLE).as_deref() {
        Ok("session") => return session(),
        Ok(other) => panic!("unknown role {other}"),
        Err(_) => {}
    }
    let dir = TempDir::new("session");
    let rig = Rig::start("s");
    let mut session = Session::start(&dir.0, &rig.namespace);
    assert_eq!(session.command("hook", "HOOK-EXIT"), "HOOK-EXIT 0");

    // The binding is made asynchronously: ask until the session is listed.
    let end = Instant::now() + WAIT;
    let (json_out, human_out) = loop {
        let json_out = rig.oac(&["status", "--json"]);
        assert_eq!(json_out.code(), Some(0), "{}", json_out.stderr);
        if number(&object(&json_out.stdout), "sessions_total") == 1 {
            let human_out = rig.oac(&["status"]);
            assert_eq!(human_out.code(), Some(0), "{}", human_out.stderr);
            break (json_out, human_out);
        }
        assert!(Instant::now() < end, "never bound: {}", json_out.stdout);
        std::thread::sleep(Duration::from_millis(100));
    };
    let report = object(&json_out.stdout);
    let sessions = member(&report, "sessions").as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    let listed = sessions[0].as_object().unwrap();
    assert!(!string(listed, "session_id").is_empty());
    assert_eq!(string(listed, "harness"), "claude");
    assert_eq!(string(listed, "state"), "online");
    assert_eq!(member(listed, "active_inbound").as_bool(), Some(true));
    let counts = sub(&report, "counts");
    assert!(number(counts, "open_connections") >= 1);

    let dir_text = dir.0.to_string_lossy().into_owned();
    for (name, shown) in [
        ("json", &json_out.stdout),
        ("human", &human_out.stdout),
        ("json diagnostics", &json_out.stderr),
        ("human diagnostics", &human_out.stderr),
    ] {
        for forbidden in [NATIVE_ID, WORK_NAME, dir_text.as_str()] {
            assert!(
                !shown.contains(forbidden),
                "{name} shows {forbidden:?}:\n{shown}"
            );
        }
    }
    assert!(
        human_out.stdout.contains("(claude): online"),
        "{}",
        human_out.stdout
    );

    let _ = writeln!(session.commands, "quit");
}

// ---- the session (a child process named `claude`) -------------------------------------------

/// Drives the fake Claude Code over the shim's stdio until `done`.
fn drive(
    fake: &mut FakeClaude,
    to_shim: &mut ChildStdin,
    from_shim: &mut BufReader<ChildStdout>,
    done: impl Fn(&FakeClaude) -> bool,
) {
    loop {
        write_outbound(fake, to_shim).expect("the shim takes the frames");
        if done(fake) {
            return;
        }
        let frame = read_frame(from_shim)
            .expect("the shim gives a frame")
            .expect("the shim is still there");
        fake.receive(&frame);
    }
}

fn session() {
    let work = PathBuf::from(std::env::var("OAC_SESSION_WORK").unwrap());
    std::env::set_current_dir(&work).unwrap();
    let mut shim = Command::new(oac())
        .args(["mcp-shim", "--harness", "claude"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the shim starts");
    let mut to_shim = shim.stdin.take().unwrap();
    let mut from_shim = BufReader::new(shim.stdout.take().unwrap());
    let shim = Guard(shim);
    let mut fake = FakeClaude::new(Config::new("oac").unwrap());
    drive(&mut fake, &mut to_shim, &mut from_shim, |f| {
        f.phase() == Phase::Ready
    });
    assert!(fake.channel_registered(), "the channel registered");
    fake.start_turn().expect("a turn");
    println!("READY {}", shim.0.id());

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        match line.split_whitespace().next() {
            Some("hook") => {
                let input = format!(
                    "{{\n  \"session_id\": \"{NATIVE_ID}\",\n  \"cwd\": {:?},\n  \"hook_event_name\": \"SessionStart\",\n  \"source\": \"startup\"\n}}\n",
                    work.display().to_string()
                );
                let mut hook = Command::new(oac())
                    .arg("hook")
                    .env_remove("OAC_DEVICE_KEY_PASSPHRASE")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .expect("the hook handler starts");
                hook.stdin
                    .take()
                    .unwrap()
                    .write_all(input.as_bytes())
                    .unwrap();
                let mut hook = Guard(hook);
                let code = wait_for_exit(&mut hook.0, WAIT).map_or(-1, |s| s.code().unwrap_or(-1));
                println!("HOOK-EXIT {code}");
            }
            Some("quit") | None => break,
            Some(other) => panic!("unknown command {other}"),
        }
    }
}

// ---- a `health` that never returns -----------------------------------------------------------

/// The Claude adapter, except that `health` blocks until `release` is set.
struct Hung {
    inner: ClaudeAdapter,
    release: Arc<std::sync::atomic::AtomicBool>,
}

impl ProviderAdapter for Hung {
    fn local_calls(&self) -> bool {
        self.inner.local_calls()
    }
    fn connection_limit(&self) -> usize {
        self.inner.connection_limit()
    }
    fn event_callback_bound_ms(&self) -> u64 {
        self.inner.event_callback_bound_ms()
    }
    fn stop_callback_bound_ms(&self) -> u64 {
        self.inner.stop_callback_bound_ms()
    }
    fn configure_local_calls(&self, admission: CallAdmission, ordinary_ms: u64, stop_ms: u64) {
        self.inner
            .configure_local_calls(admission, ordinary_ms, stop_ms);
    }
    fn watch_local_attachments(&self, handler: LocalEventHandler) {
        self.inner.watch_local_attachments(handler);
    }
    fn set_binding_local(
        &self,
        call: &mut CallContext,
        attachment: &Attachment,
        session: Option<SessionId>,
    ) -> LocalResult<()> {
        self.inner.set_binding_local(call, attachment, session)
    }
    fn capabilities_local(
        &self,
        call: &mut CallContext,
        attachment: &Attachment,
    ) -> LocalResult<AdapterCapabilities> {
        self.inner.capabilities_local(call, attachment)
    }
    fn deliver_local(
        &self,
        call: &mut CallContext,
        hand_off: HandOff,
    ) -> LocalResult<HandOffOutcome> {
        self.inner.deliver_local(call, hand_off)
    }
    fn shutdown_bound_ms(&self) -> u64 {
        self.inner.shutdown_bound_ms()
    }
    fn take_connection(&self, connection: Connection) {
        self.inner.take_connection(connection);
    }
    fn watch_attachments(&self, handler: AdapterEventHandler) {
        self.inner.watch_attachments(handler);
    }
    fn set_binding(&self, attachment: &Attachment, session: Option<SessionId>) {
        self.inner.set_binding(attachment, session);
    }
    fn capabilities(&self, attachment: &Attachment) -> AdapterCapabilities {
        self.inner.capabilities(attachment)
    }
    fn deliver(&self, hand_off: HandOff) -> HandOffOutcome {
        self.inner.deliver(hand_off)
    }
    fn accept_requests(&self, sink: Arc<dyn RequestSink>) {
        self.inner.accept_requests(sink);
    }
    fn health(&self) -> HealthStatus {
        while !self.release.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(10));
        }
        self.inner.health()
    }
    fn shutdown(&self) {
        self.inner.shutdown();
    }
}

#[test]
fn a_health_that_never_returns_is_a_timeout_every_time_and_does_not_keep_the_daemon_alive() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let release = Arc::new(AtomicBool::new(false));
    let adapter = Arc::new(Hung {
        inner: ClaudeAdapter::new(),
        release: release.clone(),
    });
    let rig = Rig::start_with(
        "h",
        adapter,
        Options {
            status_deadline: Duration::from_millis(500),
        },
    );
    // The first request is answered `timeout` once the deadline passes: exit 1, not a hang.
    let began = Instant::now();
    let first = rig.oac(&["status"]);
    assert_eq!(first.code(), Some(1), "{}{}", first.stdout, first.stderr);
    assert!(
        first
            .stderr
            .contains("could not collect its status in time"),
        "{}",
        first.stderr
    );
    assert!(began.elapsed() < Duration::from_secs(20));
    // The stuck snapshot is still in flight; later requests are answered `timeout` at once,
    // not `busy` (exit 4), and start no further thread.
    for _ in 0..2 {
        let began = Instant::now();
        let again = rig.oac(&["status", "--json"]);
        assert_eq!(again.code(), Some(1), "{}{}", again.stdout, again.stderr);
        assert_eq!(
            again.stdout.trim_end(),
            r#"{"detail":"the daemon could not collect its status in time","error":"failed","oac_cli":1,"ok":false}"#
        );
        assert!(began.elapsed() < Duration::from_secs(10));
    }
    assert!(
        rig.log.text().contains("not answered in time"),
        "{}",
        rig.log.text()
    );
    // `oac stop` still stops the daemon, and ending its runtime does not wait for the stuck
    // thread (it would, were the snapshot a blocking task of the runtime).
    let stop = rig.oac(&["stop"]);
    assert_eq!(stop.code(), Some(0), "{}", stop.stderr);
    let (tx, ended) = mpsc::channel();
    std::thread::spawn(move || {
        drop(rig);
        let _ = tx.send(());
    });
    assert!(
        ended.recv_timeout(Duration::from_secs(20)).is_ok(),
        "the daemon's runtime did not end while a health call was stuck"
    );
    release.store(true, Ordering::SeqCst);
}
