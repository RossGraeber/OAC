// SPDX-License-Identifier: Apache-2.0

//! The daemon, end to end, with real processes (G9, #70): the acceptance boxes of the
//! issue, and the residuals it owns. Written before the daemon.
//!
//! **What is real.** The daemon runs in this process, over the in-memory transport, with the
//! Claude adapter. Everything that talks to it is a separate process: each "Claude Code
//! session" is a copy of this test binary named `claude` (so it is a harness process to the
//! operating system, with a working directory of its own), which starts the real `oac
//! mcp-shim --harness claude` and the real `oac hook`, and drives the shim's standard input
//! and output with the fake Claude Code endpoint (`tests/fakes/claude`, replayed from the
//! recorded fixtures). The daemon's peer authentication, the observation of the harness
//! process, the relay, the adapter, the hook carrier and the core's pairing are all the
//! production code; only the transport and the key stores are the in-memory ones.
//!
//! The `OAC_SCOPE` namespace keeps the test's endpoints apart from the user's own.
//!
//! Boxes proven here: key material in one process (the shim and the hook handler run with no
//! key store and a poisoned passphrase variable, and the session binds); a shim crash leaves
//! another session bound; clean shutdown ends the endpoints and the sessions; a second start
//! creates no second daemon. Each session role is selected by `OAC_DAEMON_TEST_ROLE`.

use std::future::Future;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use oac_adapter_claude::ClaudeAdapter;
use oac_cli::daemon::{Daemon, LogSink, Parts, StartError};
use oac_cli::ipc::Scope;
use oac_cli::logfile::{LogFile, MAX_LOG_BYTES, log_directory};
use oac_core::clock::{Clock, SystemClock};
use oac_core::ids::Token;
use oac_core::keys::MemoryKeyStore;
use oac_core::pairing::MemoryPairingStore;
use oac_core::pipeline::PipelineConfig;
use oac_core::sealing::MemoryAgreementKeyStore;
use oac_fake_claude::link::{read_frame, write_outbound};
use oac_fake_claude::{CallOutcome, Config, FakeClaude, Phase};
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};
use tokio::runtime::Handle;

const ROLE: &str = "OAC_DAEMON_TEST_ROLE";
const TEST: &str = "a_daemon_serves_real_sessions_isolates_a_crash_and_shuts_down_cleanly";
const WAIT: Duration = Duration::from_secs(20);

fn oac() -> &'static str {
    env!("CARGO_BIN_EXE_oac")
}

/// A current-thread runtime driven by a thread of its own, with a way to run a future from
/// the test thread and wait for it. The daemon lives on it while the test thread talks to
/// child processes with blocking I/O.
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

    fn run<T: Send + 'static>(&self, f: impl Future<Output = T> + Send + 'static) -> T {
        let (tx, rx) = mpsc::channel();
        self.handle.spawn(async move {
            let _ = tx.send(f.await);
        });
        rx.recv().unwrap()
    }
}

impl Drop for Rt {
    fn drop(&mut self) {
        let _ = self.stop.take().unwrap().send(());
        let _ = self.thread.take().unwrap().join();
    }
}

/// A writer into a shared buffer, for the daemon's log.
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

/// Prints the daemon's log when the test fails.
struct DumpOnPanic(Captured);

impl Drop for DumpOnPanic {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "--- daemon log ---
{}",
                self.0.text()
            );
        }
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

fn parts(scope: Scope, network: &MemoryNetwork, log: &Captured, state: &Path) -> Parts {
    let net = network.clone();
    let scope_namespace = scope.namespace().map(str::to_owned);
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    Parts {
        scope,
        key_store: Box::new(MemoryKeyStore::new()),
        pairing_store: Box::new(MemoryPairingStore::new()),
        agreement_store: Box::new(MemoryAgreementKeyStore::new()),
        principal: Token::parse("test").unwrap(),
        transport: Arc::new(MemoryTransport::new()),
        transport_configuration: MemoryConfiguration::wrap(network),
        clock,
        monotonic: Arc::new(move || net.now()),
        config: PipelineConfig::default(),
        harnesses: vec![("claude".to_owned(), Arc::new(ClaudeAdapter::new()) as _)],
        // The daemon's lines also go to the log file of the test's namespace, under the
        // test's own state directory (G10 #71): checked for forbidden content at the end.
        log: LogSink::new(log.clone()).with_file_opener({
            let state = state.to_path_buf();
            move || LogFile::open_in(&state, scope_namespace.as_deref(), MAX_LOG_BYTES)
        }),
        codex: None,
    }
}

// ---- the parent -------------------------------------------------------------------------

/// One Claude Code session: a child process named `claude`, and its command pipe.
struct Session {
    child: Child,
    commands: ChildStdin,
    replies: BufReader<ChildStdout>,
}

impl Session {
    /// `session_env` is the value of `CLAUDE_CODE_SESSION_ID` in the session's environment
    /// (E), which its shim sends as the hello line; `None` leaves the variable unset.
    fn start(dir: &Path, tag: &str, namespace: &str, session_env: Option<&str>) -> Session {
        let bin = dir.join("bin");
        let work = dir.join(format!("work-{tag}"));
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        let claude = bin.join(if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        });
        if !claude.exists() {
            std::fs::copy(std::env::current_exe().unwrap(), &claude).unwrap();
        }
        let mut child = Command::new(&claude)
            .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
            .env(ROLE, "session")
            .env("OAC_SCOPE", namespace)
            .env("OAC_SESSION_TAG", tag)
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .envs(session_env.map(|e| ("CLAUDE_CODE_SESSION_ID", e)))
            .env("OAC_SESSION_WORK", &work)
            // Neither a shim nor a hook handler has any use for the key's passphrase.
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
        let replies = BufReader::new(child.stdout.take().unwrap());
        let mut s = Session {
            child,
            commands,
            replies,
        };
        let ready = s.reply_with("READY");
        assert!(ready.starts_with("READY"), "{ready}");
        s
    }

    /// Reads reply lines until one starts with `prefix`; the harness prints its own lines too.
    fn reply_with(&mut self, prefix: &str) -> String {
        let end = Instant::now() + WAIT;
        loop {
            let mut line = String::new();
            let n = self.replies.read_line(&mut line).expect("a reply");
            assert!(n > 0, "the session ended before `{prefix}`");
            if let Some(i) = line.find(prefix) {
                return line[i..].trim_end().to_owned();
            }
            assert!(Instant::now() < end, "timed out waiting for `{prefix}`");
        }
    }

    fn command(&mut self, command: &str, expect: &str) -> String {
        writeln!(self.commands, "{command}").unwrap();
        self.commands.flush().unwrap();
        self.reply_with(expect)
    }

    /// `whoami` until the session is bound (the signal is paired asynchronously).
    fn bound_identity(&mut self) -> String {
        let end = Instant::now() + WAIT;
        loop {
            let r = self.command("whoami", "WHOAMI");
            if r.contains("session_id") && !r.contains("unauthorized") {
                return r;
            }
            assert!(Instant::now() < end, "never bound: {r}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = writeln!(self.commands, "quit");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Waits (bounded) until the daemon's log holds `needle`.
fn wait_for_log(log: &Captured, needle: &str) {
    let end = Instant::now() + WAIT;
    while !log.text().contains(needle) {
        assert!(
            Instant::now() < end,
            "the daemon never logged {needle:?}:
{}",
            log.text()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Removes the namespace directory a Unix test made (Windows: nothing is made).
struct NamespaceDir(#[cfg(unix)] Option<std::path::PathBuf>);

impl Drop for NamespaceDir {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(dir) = self.0.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[test]
fn a_daemon_serves_real_sessions_isolates_a_crash_and_shuts_down_cleanly() {
    match std::env::var(ROLE).as_deref() {
        Ok("session") => return session(),
        Ok(other) => panic!("unknown role {other}"),
        Err(_) => {}
    }
    let namespace = format!("t{}x1", std::process::id());
    let dir = std::env::temp_dir().join(format!("oac-daemon-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let network = MemoryNetwork::new();
    let log = Captured::default();
    let _dump = DumpOnPanic(log.clone());
    let rt = Rt::new();

    let scope = Scope::for_user_in(Some(&namespace)).unwrap();
    // On Unix the namespace is a directory under the user's real run directory: removed when
    // the test ends, however it ends.
    let _namespace_dir = NamespaceDir(
        #[cfg(unix)]
        std::path::Path::new(&scope.endpoint("claude").display_name())
            .parent()
            .map(std::path::Path::to_path_buf),
    );
    let state = dir.join("state");
    let p = parts(scope.clone(), &network, &log, &state);
    let daemon = rt
        .run(Daemon::start(p))
        .unwrap_or_else(|e| panic!("the daemon starts: {e}"));

    // A second start, while the first runs, creates no second daemon (box 5): it stops at the
    // lock, before it reads a key or starts a transport.
    let second = parts(scope.clone(), &network, &log, &state);
    match rt.run(Daemon::start(second)) {
        Err(StartError::AlreadyRunning) => {}
        Err(other) => panic!("{other}"),
        Ok(_) => panic!("a second daemon started"),
    }

    // Two sessions, in two harness processes with working directories of their own.
    // The hook input of a session with tag `t` carries `session_id` `sess-<t>-<t>`. A's shim
    // sends that same value as its hello (case 3(a)), B's sends none (case 3(c)), and C's
    // sends a different one (case 3(b)): C must not be bound by its own hook.
    let mut a = Session::start(&dir, "a", &namespace, Some("sess-a-a"));
    let mut b = Session::start(&dir, "b", &namespace, None);
    let mut c = Session::start(&dir, "c", &namespace, Some("another-session-id"));

    // Before the hook, neither is bound: no session id, an `unauthorized` refusal.
    let before = a.command("whoami", "WHOAMI");
    assert!(before.contains("unauthorized"), "{before}");

    // The hook handler (the real `oac hook`) carries each session's native id; the daemon
    // pairs it with its session's shim by the harness process, and binds.
    assert_eq!(a.command("hook sess-a", "HOOK-EXIT"), "HOOK-EXIT 0");
    assert_eq!(b.command("hook sess-b", "HOOK-EXIT"), "HOOK-EXIT 0");
    let who_a = a.bound_identity();
    let who_b = b.bound_identity();
    assert_ne!(who_a, who_b, "two sessions, two session ids");
    // B had no hello, so its binding carries the diagnostic of case 3(c) (SC-ID-141).
    wait_for_log(&log, "SC-ID-141");

    // Row 87: the hello's E differs from the hook's N, so the hook binds nothing for C
    // (SC-ID-137): it stays unauthorized, while A and B are bound.
    assert_eq!(c.command("hook sess-c", "HOOK-EXIT"), "HOOK-EXIT 0");
    // The core says why: a finding that the signal failed closed on the differing value
    // (before the hook C was unauthorized too, so only this line shows the hook was
    // refused for E rather than lost).
    wait_for_log(&log, "binding Finding SC-ID-138 FailedClosed");
    let who_c = c.command("whoami", "WHOAMI");
    assert!(who_c.contains("unauthorized"), "C was bound: {who_c}");
    assert_eq!(a.bound_identity(), who_a);

    // A shim crash (box 3): session B's shim is killed. Session A stays bound, with the
    // same session id, and the device keeps serving.
    assert!(
        b.command("crash-shim", "SHIM-KILLED")
            .starts_with("SHIM-KILLED")
    );
    assert_eq!(a.bound_identity(), who_a);

    // Clean shutdown (box 4): the endpoints are gone, the shim of the live session ends, and
    // a new shim is told the daemon is not running, and how to start it (lead decision D4).
    rt.run(daemon.shutdown());
    let ended = a.command("shim-status", "SHIM-STATUS");
    assert_eq!(
        ended, "SHIM-STATUS 1",
        "the shim exits non-zero when the daemon ends"
    );
    let out = Command::new(oac())
        .args(["mcp-shim", "--harness", "claude"])
        .env("OAC_SCOPE", &namespace)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("`oac start`"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "a shim writes only the harness's bytes to stdout"
    );

    let text = log.text();
    assert!(text.contains("serving"), "{text}");

    // The log file kept the lines, and none of what its contract excludes: the sessions'
    // native ids (the hook's `session_id` and the shim's hello) and their working
    // directories (G10 #71, test 6).
    let file = log_directory(&state, Some(&namespace))
        .unwrap()
        .join("daemon.log");
    let kept = std::fs::read_to_string(&file).expect("the log file exists");
    assert!(kept.contains("serving"), "{kept}");
    assert!(kept.contains("shut down"), "{kept}");
    for forbidden in [
        "sess-a",
        "sess-b",
        "sess-c",
        "another-session-id",
        "work-a",
        "work-b",
        "work-c",
        dir.to_string_lossy().as_ref(),
    ] {
        assert!(
            !kept.contains(forbidden),
            "the log file holds {forbidden:?}:\n{kept}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- the session (a child process named `claude`) ---------------------------------------

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
    let mut fake = FakeClaude::new(Config::new("oac").unwrap());
    drive(&mut fake, &mut to_shim, &mut from_shim, |f| {
        f.phase() == Phase::Ready
    });
    assert!(fake.channel_registered(), "the channel registered");
    fake.start_turn().expect("a turn");
    println!("READY {}", shim.id());

    let tag = std::env::var("OAC_SESSION_TAG").unwrap();
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        let mut words = line.split_whitespace();
        match words.next() {
            Some("whoami") => {
                let id = fake.call_tool("whoami", "{}", None).expect("a call");
                drive(&mut fake, &mut to_shim, &mut from_shim, |f| {
                    !matches!(f.call_outcome(id), Some(CallOutcome::Pending) | None)
                });
                let text = match fake.call_outcome(id) {
                    Some(CallOutcome::Result(j) | CallOutcome::Error(j)) => j.to_compact(),
                    _ => unreachable!(),
                };
                println!("WHOAMI {text}");
            }
            Some("hook") => {
                let native = words.next().unwrap_or("sess");
                let input = format!(
                    "{{\n  \"session_id\": \"{native}-{tag}\",\n  \"cwd\": {:?},\n  \"hook_event_name\": \"SessionStart\",\n  \"source\": \"startup\"\n}}\n",
                    work.display().to_string()
                );
                let mut hook = Command::new(oac())
                    .arg("hook")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .expect("the hook handler starts");
                hook.stdin
                    .take()
                    .unwrap()
                    .write_all(input.as_bytes())
                    .unwrap();
                let out = hook.wait_with_output().unwrap();
                assert!(
                    out.stdout.is_empty(),
                    "a hook handler writes nothing to stdout"
                );
                println!("HOOK-EXIT {}", out.status.code().unwrap_or(-1));
            }
            Some("crash-shim") => {
                let pid = shim.id();
                shim.kill().unwrap();
                let _ = shim.wait();
                println!("SHIM-KILLED {pid}");
            }
            Some("shim-status") => {
                // The daemon ended: the shim exits by itself.
                let end = Instant::now() + WAIT;
                let code = loop {
                    if let Some(status) = shim.try_wait().unwrap() {
                        break status.code().unwrap_or(-1);
                    }
                    assert!(Instant::now() < end, "the shim did not end");
                    std::thread::sleep(Duration::from_millis(50));
                };
                println!("SHIM-STATUS {code}");
            }
            Some("quit") | None => break,
            Some(other) => panic!("unknown command {other}"),
        }
        std::io::stdout().flush().unwrap();
    }
    let _ = shim.kill();
}
