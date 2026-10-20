// SPDX-License-Identifier: Apache-2.0

//! `oac stop` on a daemon that was started the way `oac start --detach` starts one (G10,
//! #71): no console, its own process group, standard streams closed. On Windows such a
//! process gets no console event, so before `stop` it could be ended only by killing it,
//! with no presence withdrawal (`RISK-LOCAL-IPC`).
//!
//! `oac start` is never run here (it would make the user's device key): the stand-in is the
//! `live_daemon` example (the same `Daemon::start`, an in-memory key and transport) in a test
//! namespace, spawned with `oac_cli::commands::spawn_detached`, the function `--detach`
//! uses. Every wait is bounded (#435).

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(30);

fn example() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // deps
    path.pop(); // debug
    path.push("examples");
    path.push(format!("live_daemon{}", std::env::consts::EXE_SUFFIX));
    path
}

/// The detached daemon, killed and reaped when the test ends however it ends; on Unix the
/// namespace directory it made is removed.
struct Detached {
    child: Child,
    events: std::path::PathBuf,
    state: std::path::PathBuf,
    #[cfg(unix)]
    namespace_dir: Option<std::path::PathBuf>,
}

impl Detached {
    fn events(&self) -> String {
        std::fs::read_to_string(&self.events).unwrap_or_default()
    }

    fn wait_for(&self, event: &str) {
        let needle = format!("\"event\":\"{event}\"");
        let end = Instant::now() + WAIT;
        while !self.events().contains(&needle) {
            assert!(
                Instant::now() < end,
                "no {event:?} event: {}",
                self.events()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Detached {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.events);
        let _ = std::fs::remove_dir_all(&self.state);
        #[cfg(unix)]
        if let Some(dir) = self.namespace_dir.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[test]
fn oac_stop_stops_a_detached_daemon_cleanly() {
    let namespace = format!("sd{}x", std::process::id());
    let events =
        std::env::temp_dir().join(format!("oac-stop-detached-{}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&events);
    // The daemon's log file goes under a state directory of this test's, not the user's.
    let state =
        std::env::temp_dir().join(format!("oac-stop-detached-state-{}", std::process::id()));
    let mut command = Command::new(example());
    command
        .args(["--namespace", &namespace, "--events"])
        .arg(&events)
        .arg("--state-dir")
        .arg(&state)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = oac_cli::commands::spawn_detached(&mut command)
        .expect("the example is built by `cargo test`");
    let mut daemon = Detached {
        child,
        events,
        state,
        #[cfg(unix)]
        namespace_dir: None,
    };
    #[cfg(unix)]
    {
        let scope = oac_cli::ipc::Scope::for_user_in(Some(&namespace)).unwrap();
        daemon.namespace_dir = std::path::Path::new(&scope.endpoint("claude").display_name())
            .parent()
            .map(std::path::Path::to_path_buf);
    }
    daemon.wait_for("ready");

    // The real `oac stop`, bounded.
    let mut stop = Command::new(env!("CARGO_BIN_EXE_oac"))
        .args(["stop", "--json"])
        .env("OAC_SCOPE", &namespace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("oac starts");
    let (tx, out) = std::sync::mpsc::channel();
    let mut stdout = stop.stdout.take().unwrap();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut stdout, &mut text);
        let _ = tx.send(text);
    });
    let end = Instant::now() + WAIT;
    let status = loop {
        if let Some(s) = stop.try_wait().unwrap() {
            break s;
        }
        if Instant::now() >= end {
            let _ = stop.kill();
            let _ = stop.wait();
            panic!("`oac stop` did not return in {WAIT:?}");
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    assert_eq!(status.code(), Some(0));
    let said = out.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(said.contains("\"stopped\":true"), "{said}");
    assert!(
        said.contains(&format!("\"pid\":{}", daemon.child.id())),
        "{said}"
    );

    // `stop` returned after the shutdown: the events record it, and the process exits 0.
    // (The example writes `stopped` after the reply to the requester: wait for it.)
    daemon.wait_for("stopped");
    let recorded = daemon.events();
    assert!(
        recorded.contains("\"event\":\"stop_requested\""),
        "{recorded}"
    );
    assert!(recorded.contains("\"event\":\"stopped\""), "{recorded}");
    let end = Instant::now() + WAIT;
    let exit = loop {
        if let Some(s) = daemon.child.try_wait().unwrap() {
            break s;
        }
        assert!(Instant::now() < end, "the detached daemon did not exit");
        std::thread::sleep(Duration::from_millis(25));
    };
    assert!(exit.success(), "{exit:?}");
}
