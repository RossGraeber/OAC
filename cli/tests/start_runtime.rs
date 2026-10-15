// SPDX-License-Identifier: Apache-2.0

//! The runtime `oac start` serves on (G9, #70, slice 7, leg C).
//!
//! The first live run of `oac start` panicked at start-up: the reference transport's runtime
//! refuses to run on a thread inside a current-thread tokio runtime, and the daemon was built
//! on one. Every other test of the daemon uses the in-memory transport, which has no such
//! rule, so none could see it. These two tests start the daemon over the **real** transport,
//! on loopback, in a test namespace with an in-memory key (nothing of the user's is touched):
//!
//! - on [`oac_cli::commands::daemon_runtime`], the runtime `oac start` uses, it starts and
//!   shuts down cleanly;
//! - on a current-thread runtime it panics with the transport's message. This control shows
//!   the first test would have caught the fault; if the transport ever supports a
//!   current-thread runtime, it fails, and the runtime choice should be looked at again.

#[path = "../../transports/zenoh/tests/support/ports.rs"]
mod ports;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::Instant;

use oac_adapter_claude::ClaudeAdapter;
use oac_cli::commands::daemon_runtime;
use oac_cli::daemon::{Daemon, LogSink, Parts};
use oac_cli::ipc::Scope;
use oac_core::clock::{Clock, SystemClock};
use oac_core::ids::Token;
use oac_core::keys::MemoryKeyStore;
use oac_core::pairing::MemoryPairingStore;
use oac_core::pipeline::{MonotonicClock, PipelineConfig};
use oac_core::sealing::MemoryAgreementKeyStore;
use oac_transport_zenoh::{PeerConfiguration, PeerTransport};

/// A daemon over the real transport: loopback rendezvous on a claimed port, its own
/// partition, an in-memory key, and the test namespace for the endpoints.
fn parts(tag: &str) -> (Parts, ports::PortClaim) {
    let claim = ports::PortClaim::claim();
    let port = claim.port();
    let namespace = format!("sr{}{tag}", std::process::id());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let monotonic: MonotonicClock = Arc::new(Instant::now);
    let parts = Parts {
        scope: Scope::for_user_in(Some(&namespace)).unwrap(),
        key_store: Box::new(MemoryKeyStore::new()),
        pairing_store: Box::new(MemoryPairingStore::new()),
        agreement_store: Box::new(MemoryAgreementKeyStore::new()),
        principal: Token::parse("startruntime").unwrap(),
        transport: Arc::new(PeerTransport::new()),
        transport_configuration: PeerConfiguration::rendezvous(port)
            .with_partition(format!("start-runtime-{port}"))
            .wrap(),
        clock,
        monotonic,
        config: PipelineConfig::default(),
        harnesses: vec![("claude".to_owned(), Arc::new(ClaudeAdapter::new()) as _)],
        log: LogSink::discard(),
    };
    (parts, claim)
}

#[test]
fn the_daemon_starts_and_stops_over_the_real_transport_on_the_runtime_oac_start_uses() {
    let (parts, _claim) = parts("a");
    let runtime = daemon_runtime().expect("the runtime");
    runtime.block_on(async {
        let daemon = Daemon::start(parts)
            .await
            .unwrap_or_else(|e| panic!("the daemon starts over the real transport: {e}"));
        assert!(!daemon.endpoints().is_empty());
        daemon.shutdown().await;
    });
}

#[test]
fn on_a_current_thread_runtime_the_real_transport_refuses_to_start() {
    let (parts, _claim) = parts("b");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        runtime.block_on(async {
            if let Ok(daemon) = Daemon::start(parts).await {
                daemon.shutdown().await;
            }
        })
    }));
    let message = match outcome {
        Ok(()) => panic!("it started on a current-thread runtime: the runtime choice is moot"),
        Err(payload) => payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_default(),
    };
    assert!(
        message.contains("current thread scheduler"),
        "unexpected panic: {message}"
    );
}
