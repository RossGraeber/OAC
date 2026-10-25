// SPDX-License-Identifier: Apache-2.0

//! `oac sessions` with real processes (G10, #71, slice 4). Written before the verb.
//!
//! As in `tests/status.rs`: the daemon runs in this process over the in-memory transport in a
//! test namespace, and everything that talks to it is a separate process: a real `oac
//! sessions`, and a copy of this test binary named `claude` that starts the real `oac
//! mcp-shim` and `oac hook`, so a session is really bound. `oac start` is never run. Every
//! read from a child is on a thread with a bound, and every child is killed when its guard is
//! dropped (a known Linux hang, #435).
//!
//! Proven here: an empty list exits 0; a bound session's row shows harness `claude`, the
//! agreed extension and `active_inbound: true`; after the shim is killed the row is gone
//! (waited on the daemon's "freed a connection" log line, which the daemon writes after the
//! core ended the binding: the absence is not polled for); with no daemon the verb exits 3;
//! and the session's native id and working directory appear in neither output form.

use std::future::Future;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use oac_adapter_claude::ClaudeAdapter;
use oac_cli::daemon::{Daemon, LogSink, Options, Parts};
use oac_cli::ipc::Scope;
use oac_core::clock::{Clock, SystemClock};
use oac_core::ids::Token;
use oac_core::json::{self, Json, JsonObject};
use oac_core::keys::MemoryKeyStore;
use oac_core::pairing::MemoryPairingStore;
use oac_core::pipeline::PipelineConfig;
use oac_core::sealing::MemoryAgreementKeyStore;
use oac_fake_claude::link::{read_frame, write_outbound};
use oac_fake_claude::{Config, FakeClaude, Phase};
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};
use tokio::runtime::Handle;

const ROLE: &str = "OAC_SESSIONS_TEST_ROLE";
const SESSION_TEST: &str = "a_bound_session_is_listed_and_its_row_goes_when_its_shim_is_killed";
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
        let dir = std::env::temp_dir().join(format!("oac-sessions-{tag}-{}", std::process::id()));
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
    log: Captured,
    _dump: DumpOnPanic,
    _dir: NamespaceDir,
    _state: TempDir,
}

impl Rig {
    fn start(tag: &str) -> Rig {
        let (adapter, options) = (Arc::new(ClaudeAdapter::new()), Options::default());
        let namespace = format!("se{}{tag}", std::process::id());
        let scope = Scope::for_user_in(Some(&namespace)).unwrap();
        let dir = NamespaceDir::of(&scope);
        let state = TempDir::new(&format!("state-{tag}"));
        let log = Captured::default();
        let dump = DumpOnPanic(log.clone());
        let sink = LogSink::new(log.clone());
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
            harnesses: vec![(
                "claude".to_owned(),
                adapter as Arc<dyn oac_core::adapter::ProviderAdapter>,
            )],
            log: sink,
            codex: None,
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

fn number(o: &JsonObject, name: &str) -> u64 {
    member(o, name)
        .as_number()
        .and_then(|n| n.plain_integer_in(0, u64::MAX))
        .unwrap_or_else(|| panic!("{name} is not a plain integer"))
}

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
    let mut shim = Guard(shim);
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
            Some("crash-shim") => {
                let pid = shim.0.id();
                let _ = shim.0.kill();
                println!("SHIM-KILLED {pid}");
            }
            Some("quit") | None => break,
            Some(other) => panic!("unknown command {other}"),
        }
    }
}

/// Waits (bounded) until the daemon's log holds `needle` at least `times` times.
fn wait_for_log(log: &Captured, needle: &str, times: usize) {
    let end = Instant::now() + WAIT;
    while log.text().matches(needle).count() < times {
        assert!(
            Instant::now() < end,
            "the daemon never logged {needle:?} {times} time(s):\n{}",
            log.text()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

// ---- the verb -------------------------------------------------------------------------------

#[test]
fn an_empty_list_is_exit_0_in_both_forms() {
    let rig = Rig::start("e");
    let json_out = rig.oac(&["sessions", "--json"]);
    assert_eq!(json_out.code(), Some(0), "{}", json_out.stderr);
    let report = object(&json_out.stdout);
    assert_eq!(number(&report, "oac_cli"), 1);
    assert_eq!(member(&report, "ok").as_bool(), Some(true));
    assert_eq!(number(&report, "sessions_total"), 0);
    assert!(member(&report, "sessions").as_array().unwrap().is_empty());
    let unbound = member(&report, "unbound").as_array().unwrap();
    assert_eq!(unbound.len(), 1);
    let claude = unbound[0].as_object().unwrap();
    assert_eq!(member(claude, "harness").as_str(), Some("claude"));
    assert_eq!(number(claude, "unbound_attachments"), 0);
    // Only the session rows: nothing of the daemon's own report.
    assert!(report.get("device_id").is_none() && report.get("pid").is_none());

    let human = rig.oac(&["sessions"]);
    assert_eq!(human.code(), Some(0), "{}", human.stderr);
    assert_eq!(human.stdout, "no sessions\n");

    let usage = rig.oac(&["sessions", "extra"]);
    assert_eq!(usage.code(), Some(2), "{}", usage.stderr);
    assert!(usage.stderr.contains("usage: oac sessions [--json]"));
    let stop = rig.oac(&["stop"]);
    assert_eq!(stop.code(), Some(0), "{}", stop.stderr);
}

#[test]
fn with_no_daemon_oac_sessions_is_exit_3() {
    let namespace = format!("se{}none", std::process::id());
    for (args, json) in [
        (&["sessions"][..], false),
        (&["sessions", "--json"][..], true),
    ] {
        let out = run_bounded(
            Command::new(oac())
                .args(args)
                .env("OAC_SCOPE", &namespace)
                .stdin(Stdio::null()),
            WAIT,
        );
        assert_eq!(out.code(), Some(3), "{}{}", out.stdout, out.stderr);
        assert!(
            out.stderr
                .contains("oac sessions: the daemon is not running"),
            "{}",
            out.stderr
        );
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

#[test]
fn a_bound_session_is_listed_and_its_row_goes_when_its_shim_is_killed() {
    match std::env::var(ROLE).as_deref() {
        Ok("session") => return session(),
        Ok(other) => panic!("unknown role {other}"),
        Err(_) => {}
    }
    let dir = TempDir::new("session");
    let rig = Rig::start("s");
    let mut session = Session::start(&dir.0, &rig.namespace);
    assert_eq!(session.command("hook", "HOOK-EXIT"), "HOOK-EXIT 0");

    // The binding is made asynchronously and nothing marks its end: ask until the session
    // is listed, for at most WAIT.
    let end = Instant::now() + WAIT;
    let (json_out, human_out) = loop {
        let json_out = rig.oac(&["sessions", "--json"]);
        assert_eq!(json_out.code(), Some(0), "{}", json_out.stderr);
        if number(&object(&json_out.stdout), "sessions_total") == 1 {
            let human_out = rig.oac(&["sessions"]);
            assert_eq!(human_out.code(), Some(0), "{}", human_out.stderr);
            break (json_out, human_out);
        }
        assert!(Instant::now() < end, "never bound: {}", json_out.stdout);
        std::thread::sleep(Duration::from_millis(100));
    };
    let report = object(&json_out.stdout);
    let rows = member(&report, "sessions").as_array().unwrap();
    assert_eq!(rows.len(), 1);
    let row = rows[0].as_object().unwrap();
    let id = member(row, "session_id").as_str().unwrap().to_owned();
    assert!(!id.is_empty());
    assert_eq!(member(row, "harness").as_str(), Some("claude"));
    assert_eq!(member(row, "state").as_str(), Some("online"));
    assert_eq!(member(row, "active_inbound").as_bool(), Some(true));
    assert!(
        member(row, "extension")
            .as_str()
            .is_some_and(|e| !e.is_empty())
    );
    let _ = number(row, "major");
    assert!(member(row, "content_types").as_array().is_some());
    assert!(human_out.stdout.contains(&id), "{}", human_out.stdout);
    assert!(
        human_out.stdout.contains("claude") && human_out.stdout.contains("yes  online"),
        "{}",
        human_out.stdout
    );

    // Nothing that identifies a project is shown, in either form.
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

    // The shim is killed: its connection ends (EOF), the core ends the binding, and the
    // daemon logs that it freed the connection. Two freed lines are enough: only the shim and
    // the hook connect to the `claude` endpoint, and control connections never log the line.
    assert!(
        session
            .command("crash-shim", "SHIM-KILLED")
            .starts_with("SHIM-KILLED")
    );
    wait_for_log(&rig.log, "freed a connection on", 2);
    let out = rig.oac(&["sessions", "--json"]);
    assert_eq!(out.code(), Some(0), "{}", out.stderr);
    let after = object(&out.stdout);
    assert_eq!(number(&after, "sessions_total"), 0, "{}", out.stdout);
    assert!(member(&after, "sessions").as_array().unwrap().is_empty());
    let out = rig.oac(&["sessions"]);
    assert_eq!(out.code(), Some(0), "{}", out.stderr);
    assert_eq!(out.stdout, "no sessions\n");
    let _ = writeln!(session.commands, "quit");
}
