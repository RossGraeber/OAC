// SPDX-License-Identifier: Apache-2.0

//! The daemon's control endpoint and `oac stop`, with real processes (G10, #71): a real `oac
//! stop` against a daemon that serves a real `oac mcp-shim`; the requester is told only after
//! the shutdown and the lock release; and the endpoint's bounds (one request line of at most
//! 1 KiB within 2 s, at most 4 connections) hold without hurting the harness endpoints or a
//! later stop. Written before the endpoint.
//!
//! The daemon runs in this process, over the in-memory transport, in a test namespace; `oac
//! start` itself is never run (it would make the user's device key). Every read from a child
//! or a pipe is on a thread with a bound: there is a known Linux hang (#435).

use std::future::Future;
use std::io::{Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use oac_adapter_claude::ClaudeAdapter;
use oac_cli::daemon::{Daemon, Diagnostic, LogSink, Parts};
use oac_cli::ipc::{ClientStream, Scope};
use oac_core::clock::{Clock, SystemClock};
use oac_core::ids::Token;
use oac_core::keys::MemoryKeyStore;
use oac_core::pairing::MemoryPairingStore;
use oac_core::pipeline::PipelineConfig;
use oac_core::sealing::MemoryAgreementKeyStore;
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};
use tokio::runtime::Handle;

const WAIT: Duration = Duration::from_secs(30);

fn oac() -> &'static str {
    env!("CARGO_BIN_EXE_oac")
}

/// A current-thread runtime driven by a thread of its own (as in `tests/daemon.rs`).
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
struct NamespaceDir(#[cfg(unix)] Option<std::path::PathBuf>);

impl NamespaceDir {
    fn of(scope: &Scope) -> NamespaceDir {
        #[cfg(unix)]
        return NamespaceDir(
            std::path::Path::new(&scope.endpoint("claude").display_name())
                .parent()
                .map(std::path::Path::to_path_buf),
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

/// A daemon in this process, in its own namespace, with an owner loop like `oac start`'s: it
/// waits for a stop request and then runs the daemon's shutdown.
struct Rig {
    rt: Rt,
    scope: Scope,
    namespace: String,
    log: Captured,
    accepted: mpsc::Receiver<String>,
    _dump: DumpOnPanic,
    _dir: NamespaceDir,
}

impl Rig {
    fn start(tag: &str) -> Rig {
        let namespace = format!("ctl{}{tag}", std::process::id());
        let scope = Scope::for_user_in(Some(&namespace)).unwrap();
        let dir = NamespaceDir::of(&scope);
        let log = Captured::default();
        let dump = DumpOnPanic(log.clone());
        let (seen, accepted) = mpsc::channel();
        let sink = LogSink::new(log.clone()).with_tap(Arc::new(move |d| {
            if let Diagnostic::Accepted { label, .. } = d {
                let _ = seen.send(label.clone());
            }
        }));
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
            harnesses: vec![("claude".to_owned(), Arc::new(ClaudeAdapter::new()) as _)],
            log: sink,
        };
        let rt = Rt::new();
        let daemon = rt
            .run(Daemon::start(parts))
            .unwrap_or_else(|e| panic!("the daemon starts: {e}"));
        let endpoints = daemon.endpoints().to_vec();
        assert!(
            endpoints.iter().any(|e| e.contains("control")),
            "the control endpoint is served: {endpoints:?}"
        );
        let requested = daemon.stop_requested();
        rt.handle.spawn(async move {
            requested.await;
            daemon.shutdown().await;
        });
        Rig {
            rt,
            scope,
            namespace,
            log,
            accepted,
            _dump: dump,
            _dir: dir,
        }
    }

    /// Waits until the daemon has accepted a connection on `label`.
    fn wait_accepted(&self, label: &str) {
        let end = Instant::now() + WAIT;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.accepted.recv_timeout(left) {
                Ok(l) if l == label => return,
                Ok(_) => {}
                Err(_) => panic!("no connection accepted on {label:?}"),
            }
        }
    }

    fn control(&self) -> ClientStream {
        self.scope
            .endpoint("control")
            .connect_blocking()
            .expect("the control endpoint answers")
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

    fn wait_for_log(&self, needle: &str) {
        let end = Instant::now() + WAIT;
        while !self.log.text().contains(needle) {
            assert!(Instant::now() < end, "never logged {needle:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
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

/// Reads one reply line (or the end of the stream) from `stream` within `wait`:
/// `Some(line)`, or `None` when the other end closed it first. Panics on a timeout.
fn read_reply(mut stream: ClientStream, wait: Duration) -> Option<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match stream.read(&mut byte) {
                Ok(1) if byte[0] != b'\n' => text.push(byte[0]),
                Ok(1) => break,
                _ => break,
            }
        }
        let _ = tx.send(text);
    });
    let text = rx.recv_timeout(wait).expect("a reply or a close in time");
    (!text.is_empty()).then(|| String::from_utf8_lossy(&text).into_owned())
}

#[test]
fn a_real_oac_stop_stops_the_daemon_and_the_lock_is_free_when_it_returns() {
    let rig = Rig::start("a");
    // A real shim is connected to the daemon.
    let mut shim = Command::new(oac())
        .args(["mcp-shim", "--harness", "claude"])
        .env("OAC_SCOPE", &rig.namespace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the shim starts");
    let shim_stderr = output_reader(&mut shim, false);
    let _shim_stdout = output_reader(&mut shim, true);
    let shim_stdin = shim.stdin.take().unwrap();
    let mut shim = Guard(shim);
    rig.wait_accepted("claude");

    let out = rig.oac(&["stop"]);
    assert_eq!(out.code(), Some(0), "{}", out.stderr);
    assert!(out.stdout.contains("stopped"), "{}", out.stdout);

    // `stopped` is written after the lock is released: a new daemon could start now.
    let scope = rig.scope.clone();
    let lock = rig
        .rt
        .run(async move { scope.acquire_lock().await.map(drop) });
    assert!(lock.is_ok(), "the lock is free: {:?}", lock.err());

    // The shim saw the daemon close, and the log shows the order of the shutdown.
    let status = wait_for_exit(&mut shim.0, WAIT).expect("the shim ended");
    assert_eq!(status.code(), Some(1));
    let said = shim_stderr.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(said.contains("the daemon ended the connection"), "{said}");
    drop(shim_stdin);
    let log = rig.log.text();
    let requested = log
        .find("stopping (requested over the local IPC)")
        .expect(&log);
    let shut = log.find("shut down: presence withdrawn").expect(&log);
    assert!(requested < shut, "{log}");

    // Nothing serves any endpoint now, and a second stop finds nothing to stop.
    for label in ["claude", "control"] {
        assert!(
            rig.scope.endpoint(label).connect_blocking().is_err(),
            "{label}"
        );
    }
    let again = rig.oac(&["stop", "--json"]);
    assert_eq!(again.code(), Some(0), "{}", again.stderr);
    assert_eq!(
        again.stdout.trim_end(),
        r#"{"oac_cli":1,"ok":true,"stopped":false,"was_running":false}"#
    );
}

#[test]
fn oac_stop_json_is_one_canonical_object_with_the_daemons_pid() {
    let rig = Rig::start("j");
    let out = rig.oac(&["stop", "--json"]);
    assert_eq!(out.code(), Some(0), "{}", out.stderr);
    let expected = format!(
        r#"{{"oac_cli":1,"ok":true,"pid":{},"stopped":true,"was_running":true}}"#,
        std::process::id()
    );
    assert_eq!(out.stdout.trim_end(), expected);
    // Diagnostics (the namespace in use) are on standard error, not in the object.
    assert!(out.stderr.contains(&rig.namespace), "{}", out.stderr);
}

#[test]
fn bad_requests_are_refused_and_never_block_the_harness_endpoint_or_a_later_stop() {
    let rig = Rig::start("b");
    let reply_to = |bytes: &[u8]| {
        let mut s = rig.control();
        s.write_all(bytes).unwrap();
        s.flush().unwrap();
        read_reply(s, Duration::from_secs(10))
    };
    // Oversized: more than 1 KiB without a newline, and a long line with one.
    let big = reply_to(&vec![b'x'; 3000]).expect("a refusal");
    assert!(big.contains("too_large"), "{big}");
    let mut long = vec![b'y'; 2000];
    long.push(b'\n');
    let long = reply_to(&long).expect("a refusal");
    assert!(long.contains("too_large"), "{long}");
    // Garbage, in the several ways it can be wrong.
    for (bytes, error) in [
        (&b"garbage\n"[..], "malformed"),
        (&b"\xff\xfe\x00\n"[..], "malformed"),
        (&b"{\"oac_control\":1}\n"[..], "malformed"),
        (&b"{\"oac_control\":2,\"op\":\"stop\"}\n"[..], "version"),
        (
            &b"{\"oac_control\":1,\"op\":\"restart\"}\n"[..],
            "unknown_op",
        ),
    ] {
        let r = reply_to(bytes).expect("a refusal");
        assert!(r.contains(error), "{bytes:?}: {r}");
    }
    // Idle (nothing sent) and a request that stops half way: closed at the deadline.
    let idle = rig.control();
    let mut half = rig.control();
    half.write_all(b"{\"oac_control\":1,").unwrap();
    half.flush().unwrap();
    // While those are open the harness endpoint serves, and a stop would be served.
    let shim = rig.scope.endpoint("claude").connect_blocking().unwrap();
    rig.wait_accepted("claude");
    let began = Instant::now();
    assert_eq!(read_reply(idle, Duration::from_secs(10)), None);
    assert_eq!(read_reply(half, Duration::from_secs(10)), None);
    assert!(
        began.elapsed() >= Duration::from_secs(1),
        "closed by the deadline, not at once"
    );
    drop(shim);
    rig.wait_for_log("closed a control connection: no request in time");

    // None of that stopped the daemon: a real stop still does.
    let out = rig.oac(&["stop"]);
    assert_eq!(out.code(), Some(0), "{}", out.stderr);
    assert!(out.stdout.contains("stopped"), "{}", out.stdout);
}

#[test]
fn a_fifth_concurrent_control_connection_is_refused() {
    let rig = Rig::start("f");
    let held: Vec<ClientStream> = (0..4).map(|_| rig.control()).collect();
    let fifth = rig.control();
    let refused = read_reply(fifth, Duration::from_secs(10)).expect("a refusal");
    assert!(refused.contains("busy"), "{refused}");
    rig.wait_for_log("too many at once");
    // The four are still served (they have not been refused or closed yet): when they go,
    // the places are free again and a stop is served.
    drop(held);
    let end = Instant::now() + WAIT;
    loop {
        // A place frees when the daemon sees the close; a stop in the meantime may be
        // answered `busy` and exits 1, so ask again until it is served.
        let out = rig.oac(&["stop", "--json"]);
        if out.code() == Some(0) {
            assert!(out.stdout.contains("\"stopped\":true"), "{}", out.stdout);
            return;
        }
        assert!(
            Instant::now() < end,
            "never served: {}{}",
            out.stdout,
            out.stderr
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ---- the client against a daemon that misbehaves ---------------------------------------

/// A control endpoint that answers `reply` to the first request and, for `hold`, keeps the
/// connection open.
fn fake_control(scope: &Scope, rt: &Rt, replies: &'static [&'static str]) {
    let scope = scope.clone();
    let (ready_tx, ready) = mpsc::channel();
    rt.handle.spawn(async move {
        use oac_cli::ipc::Accept;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let lock = scope.acquire_lock().await.unwrap();
        let mut listener = scope.endpoint("control").serve(&lock).await.unwrap();
        let _ = ready_tx.send(());
        if let Ok(Accept::Connection(mut c)) = listener.accept().await {
            let mut buf = [0u8; 256];
            let _ = c.stream.read(&mut buf).await;
            for r in replies {
                let _ = c.stream.write_all(r.as_bytes()).await;
                let _ = c.stream.flush().await;
            }
            // Keep the connection and the lock until the test ends.
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
        drop(lock);
    });
    ready.recv_timeout(WAIT).expect("the fake serves");
}

#[test]
fn a_daemon_that_never_confirms_is_a_timeout_that_names_its_pid_and_kills_nothing() {
    let rt = Rt::new();
    for json in [false, true] {
        let scope = Scope::unique().unwrap();
        fake_control(
            &scope,
            &rt,
            &["{\"oac_control\":1,\"pid\":4242,\"stopping\":true}
"],
        );
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let began = Instant::now();
        let wait = Duration::from_millis(400);
        let code = oac_cli::commands::stop_with(&scope, json, wait, &mut out, &mut err);
        assert_eq!(code, 1);
        assert!(began.elapsed() < Duration::from_secs(10));
        let (out, err) = (
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        );
        assert!(err.contains("pid 4242"), "{err}");
        assert!(err.contains("nothing was killed"), "{err}");
        if json {
            assert_eq!(
                out.trim_end(),
                r#"{"error":"timeout","oac_cli":1,"ok":false,"pid":4242,"was_running":true}"#
            );
        } else {
            assert!(out.is_empty(), "{out}");
        }
    }
}

#[test]
fn a_daemon_of_another_protocol_version_is_exit_4() {
    let rt = Rt::new();
    let scope = Scope::unique().unwrap();
    fake_control(
        &scope,
        &rt,
        &["{\"error\":\"version\",\"oac_control\":9,\"supported\":9}\n"],
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code =
        oac_cli::commands::stop_with(&scope, true, Duration::from_secs(10), &mut out, &mut err);
    assert_eq!(code, 4);
    assert_eq!(
        String::from_utf8(out).unwrap().trim_end(),
        r#"{"error":"version","oac_cli":1,"ok":false,"was_running":true}"#
    );
    assert!(String::from_utf8(err).unwrap().contains("another version"));
}

#[test]
fn a_daemon_that_never_answers_is_a_timeout_with_no_pid() {
    let rt = Rt::new();
    let scope = Scope::unique().unwrap();
    fake_control(&scope, &rt, &[]);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = oac_cli::commands::stop_with(
        &scope,
        false,
        Duration::from_millis(400),
        &mut out,
        &mut err,
    );
    // The fake never answers and never closes: that is a timeout (exit 1), and no pid is known.
    assert_eq!(code, 1);
    let err = String::from_utf8(err).unwrap();
    assert!(err.contains("did not confirm"), "{err}");
    assert!(!err.contains("pid"), "{err}");
}
