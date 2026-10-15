// SPDX-License-Identifier: Apache-2.0

//! The live-leg daemon (`examples/live_daemon.rs`, G9 #70 slice 7): it starts in its test
//! namespace, reports a connection it accepted with the process it observed, and stops
//! cleanly on a `stop` line. This process is the peer, and it is not a harness, so the
//! daemon says it did not pair (the observation fails closed).

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use oac_cli::ipc::Scope;

fn example() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // deps
    path.pop(); // debug
    path.push("examples");
    path.push(format!("live_daemon{}", std::env::consts::EXE_SUFFIX));
    path
}

fn wait_for(reader: &mut impl BufRead, event: &str) -> String {
    let end = Instant::now() + Duration::from_secs(30);
    let mut line = String::new();
    while Instant::now() < end {
        line.clear();
        if reader.read_line(&mut line).unwrap() == 0 {
            panic!("the daemon's output ended before the {event:?} event");
        }
        if line.contains(&format!("\"event\":\"{event}\"")) {
            return line;
        }
    }
    panic!("no {event:?} event in time");
}

#[test]
fn the_live_daemon_reports_what_it_observed_and_stops_cleanly() {
    let namespace = format!("ld{}x", std::process::id());
    let mut child = Command::new(example())
        .args(["--namespace", &namespace])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the example is built by `cargo test`");
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let ready = wait_for(&mut out, "ready");
    assert!(ready.contains(&namespace), "{ready}");

    // A peer that is not a harness: accepted, observed, not paired.
    let scope = Scope::for_user_in(Some(&namespace)).unwrap();
    let stream = scope.endpoint("claude").connect_blocking().unwrap();
    let accepted = wait_for(&mut out, "accepted");
    assert!(accepted.contains("\"label\":\"claude\""), "{accepted}");
    assert!(
        accepted.contains(&format!("\"pid\":{}", std::process::id())),
        "{accepted}"
    );
    assert!(accepted.contains("\"paired\":false"), "{accepted}");
    assert!(accepted.contains("\"harness\":null"), "{accepted}");
    drop(stream);
    wait_for(&mut out, "ended");

    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "stop").unwrap();
    stdin.flush().unwrap();
    wait_for(&mut out, "stopped");
    assert!(child.wait().unwrap().success());
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
