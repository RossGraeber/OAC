// SPDX-License-Identifier: Apache-2.0

//! A daemon for the live legs (G9, #70, slice 7): **a development tool, not the product.**
//!
//! `oac start` is the product's daemon. It keeps the user's real device key in the operating
//! system's credential store and refuses `OAC_SCOPE` (the key is the user's one in every
//! namespace, so a second daemon in a namespace beside the real one would be a second peer
//! with the same identity). A live leg that drives a real harness from a scripted terminal must not
//! touch that key, so it runs this instead: the same `Daemon::start` (same lock, same
//! endpoints, same peer authentication and observation, same relay, same Claude adapter), but
//!
//! - the key is made in memory for this run and kept nowhere;
//! - the transport is the in-memory one, with nothing to announce to;
//! - the endpoints are in the test namespace given on the command line, which is the
//!   `OAC_SCOPE` the harness and its hook command are launched with;
//! - every accept, end and log line is written as a JSON line, so a run's record can be read
//!   by a program: which process the daemon observed, the chain of processes from the peer to
//!   the harness, whether it paired, and the core's binding lines.
//!
//! ```text
//! cargo run -p oac-cli --example live_daemon -- --namespace g9a --events <file> [--state-dir <dir>]
//! ```
//!
//! Its log lines also go to the daemon log file of the namespace (`oac_cli::logfile`, G10 #71):
//! under `<dir>/OAC/logs/ns-<namespace>/` with `--state-dir`, else in the user's state
//! directory. A start that fails says why on standard error and exits 1.
//!
//! It stops on Ctrl-C, when a line `stop` is read on its standard input, or when `oac stop`
//! asks over the control endpoint (G10, #71; a `stop_requested` event), and writes a
//! `stopped` event after the clean shutdown. The end of its standard input (a closed or empty
//! input) does not stop it: a run that gives it no input stops it with Ctrl-C or by ending
//! the process. It is built from `cli/examples/`, which only
//! `cargo` builds on request: the `oac` binary does not contain it, and it depends on the
//! in-memory transport, a dev-dependency (`scripts/check-crate-deps.mjs`).

use std::fs::{File, OpenOptions};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use oac_adapter_claude::ClaudeAdapter;
use oac_cli::daemon::{Daemon, Diagnostic, LogSink, Parts};
use oac_cli::ipc::Scope;
use oac_cli::logfile::{LogFile, MAX_LOG_BYTES};
use oac_cli::peer::{self, ProcessId, ProcessTable};
use oac_core::clock::{Clock, SystemClock};
use oac_core::ids::Token;
use oac_core::keys::MemoryKeyStore;
use oac_core::pairing::MemoryPairingStore;
use oac_core::pipeline::PipelineConfig;
use oac_core::sealing::MemoryAgreementKeyStore;
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};

/// The JSON lines of a run: to the events file when one is given, and to standard output.
#[derive(Clone)]
struct Events {
    file: Option<Arc<Mutex<File>>>,
}

impl Events {
    /// Writes one event: an object with `event`, `unix_ms` and the members in `body`.
    fn emit(&self, event: &str, body: &str) {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let comma = if body.is_empty() { "" } else { "," };
        let line = format!(
            "{{\"event\":{},\"unix_ms\":{ms}{comma}{body}}}\n",
            json_string(event)
        );
        // A closed standard output must not stop the daemon: the events file still gets it.
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(line.as_bytes());
        let _ = stdout.flush();
        if let Some(file) = &self.file {
            let mut file = file.lock().unwrap_or_else(|e| e.into_inner());
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
    }
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The daemon's log lines, each as a `log` event.
struct LogLines {
    events: Events,
    partial: Vec<u8>,
}

impl Write for LogLines {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.partial.extend_from_slice(buf);
        while let Some(end) = self.partial.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=end).collect();
            let text = String::from_utf8_lossy(&line[..line.len() - 1]).into_owned();
            self.events
                .emit("log", &format!("\"line\":{}", json_string(&text)));
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The processes from `pid` up to the root, as JSON: id, start time, name, resolved name.
fn chain_json(table: &dyn ProcessTable, pid: u32) -> String {
    let mut parts = Vec::new();
    let mut next = Some(pid);
    for _ in 0..peer::MAX_ANCESTRY {
        let Some(info) = next.and_then(|p| table.lookup(p)) else {
            break;
        };
        parts.push(format!(
            "{{\"pid\":{},\"started\":{},\"image\":{},\"resolved\":{}}}",
            info.id.pid,
            info.id.started,
            json_string(&info.image),
            info.resolved_image
                .as_deref()
                .map_or("null".to_owned(), json_string)
        ));
        next = info.parent.filter(|p| *p != 0 && *p != info.id.pid);
    }
    format!("[{}]", parts.join(","))
}

fn process_json(id: Option<ProcessId>) -> String {
    id.map_or("null".to_owned(), |p| {
        format!("{{\"pid\":{},\"started\":{}}}", p.pid, p.started)
    })
}

/// What the daemon's tap reports, with the process chain read now (the peer is alive: the
/// hook handler waits for the daemon, and the shim runs for the session).
fn tap(events: Events) -> oac_cli::daemon::Tap {
    Arc::new(move |diagnostic| match diagnostic {
        Diagnostic::Accepted {
            label,
            peer: process,
            paired,
        } => {
            let table = peer::SystemTable::new().ok();
            let chain = match (&table, process) {
                (Some(table), Some(p)) => chain_json(table, p.pid),
                _ => "[]".to_owned(),
            };
            let harness = match (&table, process, peer::harness_images(label)) {
                (Some(table), Some(p), Some(images)) => {
                    process_json(peer::harness_ancestor(table, p.pid, images))
                }
                _ => "null".to_owned(),
            };
            events.emit(
                "accepted",
                &format!(
                    "\"label\":{},\"peer\":{},\"chain\":{chain},\"harness\":{harness},\"paired\":{paired}",
                    json_string(label),
                    process_json(*process)
                ),
            );
        }
        Diagnostic::Ended {
            label,
            peer: process,
        } => events.emit(
            "ended",
            &format!(
                "\"label\":{},\"peer\":{}",
                json_string(label),
                process_json(*process)
            ),
        ),
    })
}

struct Arguments {
    namespace: String,
    events: Option<PathBuf>,
    state_dir: Option<PathBuf>,
}

fn arguments() -> Result<Arguments, String> {
    let mut namespace = None;
    let mut events = None;
    let mut state_dir = None;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--namespace" => namespace = args.next(),
            "--events" => events = args.next().map(PathBuf::from),
            "--state-dir" => state_dir = args.next().map(PathBuf::from),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(Arguments {
        namespace: namespace.ok_or("--namespace <name> is required")?,
        events,
        state_dir,
    })
}

fn main() {
    let args = match arguments() {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "live_daemon: {e}\nusage: live_daemon --namespace <name> [--events <file>] [--state-dir <dir>]"
            );
            std::process::exit(2);
        }
    };
    let file = args.events.as_ref().map(|path| {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap_or_else(|e| {
                eprintln!("live_daemon: {}: {e}", path.display());
                std::process::exit(1);
            });
        Arc::new(Mutex::new(file))
    });
    let events = Events { file };
    let scope = match Scope::for_user_in(Some(&args.namespace)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("live_daemon: {e}");
            std::process::exit(1);
        }
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let network = MemoryNetwork::new();
    let net = network.clone();
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let log = LogSink::new(LogLines {
        events: events.clone(),
        partial: Vec::new(),
    })
    .with_tap(tap(events.clone()));
    // The log file; a daemon runs without it when it cannot be opened (one warning).
    // It is opened by `Daemon::start` once the lock is held, as for `oac start`.
    let namespace = args.namespace.clone();
    let state_dir = args.state_dir.clone();
    let log = log.with_file_opener(move || match &state_dir {
        Some(dir) => LogFile::open_in(dir, Some(&namespace), MAX_LOG_BYTES),
        None => LogFile::open(Some(&namespace)),
    });
    let failure_log = log.clone();
    let parts = Parts {
        scope,
        key_store: Box::new(MemoryKeyStore::new()),
        pairing_store: Box::new(MemoryPairingStore::new()),
        agreement_store: Box::new(MemoryAgreementKeyStore::new()),
        principal: Token::parse("livedaemon").expect("a token"),
        transport: Arc::new(MemoryTransport::new()),
        transport_configuration: MemoryConfiguration::wrap(&network),
        clock,
        monotonic: Arc::new(move || net.now()),
        config: PipelineConfig::default(),
        harnesses: vec![("claude".to_owned(), Arc::new(ClaudeAdapter::new()) as _)],
        log,
    };
    // A line `stop` on standard input stops the daemon as Ctrl-C does. The end of the input
    // does not: the sender is kept alive, because a dropped sender would complete the receiver
    // and stop the daemon.
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            if matches!(line, Ok(l) if l.trim() == "stop") {
                let _ = stop_tx.send(());
                return;
            }
        }
        // End of input (or a read error): nothing more will arrive; keep `stop_tx` alive.
        loop {
            std::thread::park();
        }
    });
    let code = runtime.block_on(async {
        match Daemon::start(parts).await {
            Ok(daemon) => {
                events.emit(
                    "ready",
                    &format!(
                        "\"namespace\":{},\"endpoints\":[{}],\"pid\":{}",
                        json_string(&args.namespace),
                        daemon
                            .endpoints()
                            .iter()
                            .map(|e| json_string(e))
                            .collect::<Vec<_>>()
                            .join(","),
                        std::process::id()
                    ),
                );
                let requested = daemon.stop_requested();
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = stop_rx => {}
                    () = requested => {
                        // `oac stop` over the control endpoint, as for `oac start`.
                        events.emit("stop_requested", "");
                    }
                }
                daemon.shutdown().await;
                events.emit("stopped", "");
                0
            }
            Err(e) => {
                eprintln!("live_daemon: the daemon did not start: {e}");
                failure_log.plain(&format!("live_daemon: the daemon did not start: {e}"));
                events.emit(
                    "failed",
                    &format!("\"error\":{}", json_string(&e.to_string())),
                );
                1
            }
        }
    });
    std::process::exit(code);
}
