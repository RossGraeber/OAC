// SPDX-License-Identifier: Apache-2.0

//! `oac status` against a daemon over the **real** transport (G10, #71, slice 3), in the
//! style of `tests/start_runtime.rs`: the daemon starts on [`oac_cli::commands::daemon_runtime`],
//! the runtime `oac start` uses, over loopback on a claimed port (`ports::PortClaim`, #432),
//! with an in-memory key and a test namespace (nothing of the user's is touched), and a real
//! `oac status --json` process reports the transport healthy. The in-memory transport of the
//! other status tests cannot show that the real one answers `health` through the daemon's
//! control endpoint on this runtime.
//!
//! The child's output is read on threads with a bound, and the child is killed when the test
//! ends however it ends (#435).

#[path = "../../transports/zenoh/tests/support/ports.rs"]
mod ports;

use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use oac_adapter_claude::ClaudeAdapter;
use oac_cli::commands::daemon_runtime;
use oac_cli::daemon::{Daemon, LogSink, Parts};
use oac_cli::ipc::Scope;
use oac_core::clock::{Clock, SystemClock};
use oac_core::ids::Token;
use oac_core::json::{self, Json};
use oac_core::keys::MemoryKeyStore;
use oac_core::pairing::MemoryPairingStore;
use oac_core::pipeline::{MonotonicClock, PipelineConfig};
use oac_core::sealing::MemoryAgreementKeyStore;
use oac_transport_zenoh::{PeerConfiguration, PeerTransport};

const WAIT: Duration = Duration::from_secs(30);

/// Removes the namespace directory a Unix test made under the user's run directory (Windows:
/// nothing is made), when the test ends however it ends.
struct NamespaceDir(#[cfg(unix)] Option<std::path::PathBuf>);

impl Drop for NamespaceDir {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(dir) = self.0.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

fn namespace_dir(scope: &Scope) -> NamespaceDir {
    #[cfg(unix)]
    {
        NamespaceDir(
            std::path::Path::new(&scope.endpoint("claude").display_name())
                .parent()
                .map(std::path::Path::to_path_buf),
        )
    }
    #[cfg(not(unix))]
    {
        let _ = scope;
        NamespaceDir()
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

fn reader(source: impl Read + Send + 'static) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut source = source;
        let mut text = String::new();
        let _ = source.read_to_string(&mut text);
        let _ = tx.send(text);
    });
    rx
}

/// Runs `oac status --json` in `namespace`: its exit status, standard output and standard
/// error, each read within a bound.
fn run_status(namespace: &str) -> (Option<ExitStatus>, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_oac"))
        .args(["status", "--json"])
        .env("OAC_SCOPE", namespace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("oac status starts");
    let out = reader(child.stdout.take().unwrap());
    let err = reader(child.stderr.take().unwrap());
    let mut child = Guard(child);
    let end = Instant::now() + WAIT;
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= end {
            break None;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    (
        status,
        out.recv_timeout(Duration::from_secs(5)).unwrap_or_default(),
        err.recv_timeout(Duration::from_secs(5)).unwrap_or_default(),
    )
}

#[test]
fn oac_status_reports_the_real_transport_healthy() {
    let claim = ports::PortClaim::claim();
    let port = claim.port();
    let namespace = format!("ss{}", std::process::id());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let monotonic: MonotonicClock = Arc::new(Instant::now);
    let scope = Scope::for_user_in(Some(&namespace)).unwrap();
    let _dir = namespace_dir(&scope);
    let parts = Parts {
        scope,
        key_store: Box::new(MemoryKeyStore::new()),
        pairing_store: Box::new(MemoryPairingStore::new()),
        agreement_store: Box::new(MemoryAgreementKeyStore::new()),
        principal: Token::parse("statusruntime").unwrap(),
        transport: Arc::new(PeerTransport::new()),
        transport_configuration: PeerConfiguration::rendezvous(port)
            .with_partition(format!("status-runtime-{port}"))
            .wrap(),
        clock,
        monotonic,
        config: PipelineConfig::default(),
        harnesses: vec![("claude".to_owned(), Arc::new(ClaudeAdapter::new()) as _)],
        log: LogSink::discard(),
    };
    let runtime = daemon_runtime().expect("the runtime");
    let (status, stdout, stderr) = runtime.block_on(async {
        let daemon = Daemon::start(parts)
            .await
            .unwrap_or_else(|e| panic!("the daemon starts over the real transport: {e}"));
        let ns = namespace.clone();
        let result = tokio::task::spawn_blocking(move || run_status(&ns))
            .await
            .expect("the status run");
        daemon.shutdown().await;
        result
    });
    let status = status.expect("oac status ended in time");
    assert_eq!(status.code(), Some(0), "{stdout}{stderr}");
    let report = json::parse(stdout.trim_end().as_bytes())
        .expect("one JSON object")
        .into_object()
        .expect("an object");
    let transport = report
        .get("transport")
        .and_then(Json::as_object)
        .expect("a transport member");
    assert_eq!(
        transport.get("state").and_then(Json::as_str),
        Some("healthy"),
        "{stdout}"
    );
    let adapters = report.get("adapters").and_then(Json::as_array).unwrap();
    assert_eq!(adapters.len(), 1, "{stdout}");
    drop(claim);
}
