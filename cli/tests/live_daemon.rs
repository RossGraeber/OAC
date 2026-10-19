// SPDX-License-Identifier: Apache-2.0

//! The live-leg daemon (`examples/live_daemon.rs`, G9 #70 slice 7): it starts in its test
//! namespace, reports a connection it accepted with the process it observed, and stops
//! cleanly on a `stop` line.
//!
//! This process is the peer. Whether the daemon pairs it depends on where the test runs: a
//! test run from inside a Claude Code session has a harness process among its ancestors, and
//! the daemon (correctly) pairs the connection with it; one run from a plain terminal has
//! none, and the observation fails closed. So the expectation is computed here, from this
//! process's own ancestry, with the same walk the daemon uses, and the event is compared
//! with it.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use oac_cli::ipc::Scope;
use oac_cli::peer;

const WAIT: Duration = Duration::from_secs(30);

fn example() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // deps
    path.pop(); // debug
    path.push("examples");
    path.push(format!("live_daemon{}", std::env::consts::EXE_SUFFIX));
    path
}

/// The daemon child, killed and reaped when the test ends however it ends; and, on Unix, the
/// namespace directory it made under the user's run directory, removed.
struct Daemon {
    child: Child,
    lines: Receiver<String>,
    #[cfg(unix)]
    namespace_dir: Option<std::path::PathBuf>,
}

impl Daemon {
    fn start(namespace: &str) -> Daemon {
        Daemon::start_with(namespace, Stdio::piped())
    }

    fn start_with(namespace: &str, stdin: Stdio) -> Daemon {
        let mut child = Command::new(example())
            .args(["--namespace", namespace])
            .stdin(stdin)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the example is built by `cargo test`");
        let stdout: ChildStdout = child.stdout.take().unwrap();
        // The reads block, so they are on a thread: a daemon that never answers is a
        // timeout, not a hang.
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if tx.send(line).is_err() {
                    return;
                }
            }
        });
        Daemon {
            child,
            lines,
            #[cfg(unix)]
            namespace_dir: None,
        }
    }

    /// The next line holding `"event":"<event>"`.
    fn wait_for(&self, event: &str) -> String {
        let end = Instant::now() + WAIT;
        let needle = format!("\"event\":\"{event}\"");
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) if line.contains(&needle) => return line,
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => panic!("no {event:?} event in {WAIT:?}"),
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("the daemon's output ended before the {event:?} event")
                }
            }
        }
    }

    fn stop(&mut self) {
        // (only for a daemon started with a piped standard input)
        let mut stdin = self.child.stdin.take().unwrap();
        writeln!(stdin, "stop").unwrap();
        stdin.flush().unwrap();
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        #[cfg(unix)]
        if let Some(dir) = self.namespace_dir.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[test]
fn the_live_daemon_reports_what_it_observed_and_stops_cleanly() {
    let namespace = format!("ld{}x", std::process::id());
    let scope = Scope::for_user_in(Some(&namespace)).unwrap();
    let mut daemon = Daemon::start(&namespace);
    #[cfg(unix)]
    {
        daemon.namespace_dir = std::path::Path::new(&scope.endpoint("claude").display_name())
            .parent()
            .map(std::path::Path::to_path_buf);
    }
    let ready = daemon.wait_for("ready");
    assert!(ready.contains(&namespace), "{ready}");

    // What the daemon should find: the harness process above this test, if there is one.
    let table = peer::SystemTable::new().unwrap();
    let expected = peer::harness_ancestor(
        &table,
        std::process::id(),
        peer::harness_images("claude").unwrap(),
    );

    let stream = scope.endpoint("claude").connect_blocking().unwrap();
    let accepted = daemon.wait_for("accepted");
    assert!(accepted.contains("\"label\":\"claude\""), "{accepted}");
    assert!(
        accepted.contains(&format!("\"peer\":{{\"pid\":{},", std::process::id())),
        "{accepted}"
    );
    assert!(
        accepted.contains(&format!("\"paired\":{}", expected.is_some())),
        "paired should be {}: {accepted}",
        expected.is_some()
    );
    match expected {
        Some(harness) => assert!(
            accepted.contains(&format!(
                "\"harness\":{{\"pid\":{},\"started\":{}}}",
                harness.pid, harness.started
            )),
            "{accepted}"
        ),
        None => assert!(accepted.contains("\"harness\":null"), "{accepted}"),
    }
    drop(stream);
    daemon.wait_for("ended");

    daemon.stop();
    daemon.wait_for("stopped");
    assert!(daemon.child.wait().unwrap().success());
    // The endpoint is gone.
    assert!(scope.endpoint("claude").connect_blocking().is_err());
}

#[test]
fn the_live_daemon_needs_a_namespace_and_refuses_one_that_is_not_a_name() {
    for args in [&[][..], &["--namespace", "NOT A NAME"][..]] {
        let out = Command::new(example()).args(args).output().unwrap();
        assert!(!out.status.success(), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
    }
}

/// The end of the daemon's standard input is not a stop (a run with no input, such as one in
/// the background, must keep serving): with the input closed from the start it still serves,
/// is still connectable, and stops only when it is ended.
#[test]
fn the_end_of_standard_input_does_not_stop_the_live_daemon() {
    let namespace = format!("ld{}e", std::process::id());
    let scope = Scope::for_user_in(Some(&namespace)).unwrap();
    let mut daemon = Daemon::start_with(&namespace, Stdio::null());
    #[cfg(unix)]
    {
        daemon.namespace_dir = std::path::Path::new(&scope.endpoint("claude").display_name())
            .parent()
            .map(std::path::Path::to_path_buf);
    }
    daemon.wait_for("ready");
    // Long enough for a stop on end of input to have happened and been written.
    std::thread::sleep(Duration::from_millis(800));
    assert!(
        daemon.child.try_wait().unwrap().is_none(),
        "the daemon ended on the end of its standard input"
    );
    let stream = scope.endpoint("claude").connect_blocking().unwrap();
    daemon.wait_for("accepted");
    drop(stream);
}
