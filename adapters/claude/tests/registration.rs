// SPDX-License-Identifier: Apache-2.0

//! Session registration end to end (G5, #66): the real Claude adapter under the real core
//! pipelines, with the harness played by the test (a `SessionStart` hook input, and Claude
//! Code's MCP frames) and a transport that takes everything and carries nothing. CI-default:
//! no live harness, no network.
//!
//! What it shows, with the core's own binding decision in between
//! (`spec/session-channels.md` §6.7; `docs/planning/decisions/C4-session-identity.md` §3, §6):
//!
//! - a channel attachment is unbound, and `whoami` is refused `unauthorized`, until a
//!   `SessionStart` hook input arrives on a connection the core observed with the same
//!   pairing key; the hook's `session_id` is the only source of N;
//! - then the attachment is bound, and `whoami` returns the core's session id and device
//!   fingerprint;
//! - `/clear` (the hook reports a new `session_id`, `source` `clear`) re-binds the attachment
//!   under a new session id;
//! - the harness exiting (the connection ending) deregisters the binding;
//! - `--resume <id>` starts a new process, so a new attachment, and the hook reports `source`
//!   `resume` with the *same* `session_id` as before (captured on Claude Code `2.1.285`): a
//!   transition, bound under a new session id, nothing carried forward (C4 §6; B2: behavior
//!   across `--resume` is UNVERIFIED, so a resumed session is a new one);
//! - a second live process reporting a `session_id` that is already bound is refused, and the
//!   first binding is kept (C4 §3 case 2);
//! - a hook input observed with another key binds nothing.
//!
//! The pairing key here is a value the test gives the core as its own observation of the peer
//! ([`Pipelines::connect_observed`]); the operating-system facility that yields it is G9's
//! (#70) and UNVERIFIED per platform (`spec/session-channels.md` §6.7.2).

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use oac_adapter_claude::ClaudeAdapter;
use oac_core::adapter::{Attachment, Connection};
use oac_core::authorization::{AuthorizationEngine, MemoryDecisionLog};
use oac_core::clock::{Clock, SystemClock};
use oac_core::health::{HealthState, HealthStatus};
use oac_core::ids::{KeyId, SessionId, Token};
use oac_core::json::{self, Json};
use oac_core::keys::{DeviceIdentity, DeviceKey};
use oac_core::pipeline::{AdapterId, PipelineConfig, Pipelines};
use oac_core::session_binding::{MemoryBindingLog, PairingKey, PeerObservation};
use oac_core::transport::{
    Deadline, Destination, InboundHandler, Payload, PresenceHandler, PublishResult, Reach,
    Subscription, Transport, TransportCapabilities, TransportConfiguration, TransportError,
};

const WAIT: Duration = Duration::from_secs(10);

const PROBE: &str = "{\"jsonrpc\":\"2.0\",\"id\":\"server-discover-probe-1\",\"method\":\"server/discover\",\"params\":{\"_meta\":{\"io.modelcontextprotocol/protocolVersion\":\"2026-07-28\",\"io.modelcontextprotocol/clientInfo\":{\"name\":\"claude-code\",\"version\":\"2.1.285\"},\"io.modelcontextprotocol/clientCapabilities\":{}}}}";
const INITIALIZE: &str = "{\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"clientInfo\":{\"name\":\"claude-code\",\"version\":\"2.1.285\"}},\"jsonrpc\":\"2.0\",\"id\":0}";

/// A transport that takes every payload and delivers none.
struct Null;

impl Transport for Null {
    fn start(
        &self,
        _local_device: &KeyId,
        _configuration: TransportConfiguration,
    ) -> Result<TransportCapabilities, TransportError> {
        Ok(TransportCapabilities {
            reliability: false,
            persistence: false,
            offline_queueing: false,
            ordering: true,
            multicast_discovery: false,
            routing_federation: false,
            reach: Reach::CrossImplementation,
            destination_restricted: true,
            max_payload_octets: 65_536 * 4,
            sealing: false,
        })
    }
    fn publish(&self, _: &Destination, _: Payload, _: Deadline) -> PublishResult {
        PublishResult::Taken
    }
    fn subscribe(
        &self,
        _: &Destination,
        _: InboundHandler,
    ) -> Result<Subscription, TransportError> {
        Ok(Subscription::new(|| {}))
    }
    fn send_presence(&self, _: &Destination, _: Payload, _: Deadline) -> PublishResult {
        PublishResult::Taken
    }
    fn watch_presence(&self, _: PresenceHandler) -> Result<(), TransportError> {
        Ok(())
    }
    fn health(&self) -> HealthStatus {
        HealthStatus::new(HealthState::Healthy)
    }
    fn shutdown(&self) {}
}

/// One local connection as the harness side sees it.
struct Wire {
    to_adapter: Option<Sender<Vec<u8>>>,
    from_adapter: Receiver<Vec<u8>>,
    partial: Vec<u8>,
    handle: Attachment,
}

impl Wire {
    fn send(&self, line: &str) {
        self.to_adapter
            .as_ref()
            .expect("open")
            .send(format!("{line}\n").into_bytes())
            .expect("the adapter reads");
    }

    fn frame(&mut self) -> Json {
        let end = Instant::now() + WAIT;
        loop {
            if let Some(i) = self.partial.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.partial.drain(..=i).collect();
                return json::parse(&line[..line.len() - 1]).expect("a JSON frame");
            }
            let now = Instant::now();
            assert!(now < end, "no frame from the server");
            match self.from_adapter.recv_timeout(end - now) {
                Ok(b) => self.partial.extend_from_slice(&b),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                    panic!("no frame from the server")
                }
            }
        }
    }

    fn tool(&mut self, id: u64, name: &str) -> Json {
        self.send(&format!(
            "{{\"method\":\"tools/call\",\"params\":{{\"name\":\"{name}\",\"arguments\":{{}}}},\"jsonrpc\":\"2.0\",\"id\":{id}}}"
        ));
        self.frame()
    }

    fn hang_up(&mut self) {
        self.to_adapter = None;
    }
}

fn text_at<'a>(v: &'a Json, path: &[&str]) -> Option<&'a str> {
    let mut cur = v;
    for p in path {
        cur = cur.as_object()?.get(p)?;
    }
    cur.as_str()
}

/// The first content block's text of a tool result.
fn result_text(v: &Json) -> String {
    v.as_object()
        .and_then(|o| o.get("result"))
        .and_then(Json::as_object)
        .and_then(|o| o.get("content"))
        .and_then(Json::as_array)
        .and_then(|a| a.first())
        .and_then(|c| text_at(c, &["text"]))
        .unwrap_or_default()
        .to_owned()
}

struct Device {
    pipes: Pipelines,
    adapter: AdapterId,
    fingerprint: KeyId,
    log: MemoryBindingLog,
}

impl Device {
    fn new() -> Device {
        let identity = DeviceIdentity::new(DeviceKey::generate(), Token::parse("dev").unwrap());
        let fingerprint = identity.key_id().clone();
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let engine =
            AuthorizationEngine::new(&identity, clock.clone(), Box::new(MemoryDecisionLog::new()));
        let pipes = Pipelines::new(
            identity,
            engine,
            clock,
            Arc::new(Instant::now),
            Arc::new(Null),
            PipelineConfig::default(),
        );
        pipes.start(TransportConfiguration::new(())).expect("start");
        let log = MemoryBindingLog::new(64);
        pipes.set_binding_log(Box::new(log.clone()));
        let adapter = pipes.add_adapter(Arc::new(ClaudeAdapter::new()));
        Device {
            pipes,
            adapter,
            fingerprint,
            log,
        }
    }

    /// A connection the core observed with pairing key `key`, handed to the Claude adapter.
    fn connect(&self, key: &str) -> Wire {
        let (to_tx, to_rx) = mpsc::channel();
        let (from_tx, from_rx) = mpsc::channel();
        let writer = oac_core::connection::ConnectionWriter::from(from_tx);
        let conn = Connection::accept(to_rx.into(), writer);
        let handle = conn.handle().clone();
        self.pipes
            .connect_observed(
                self.adapter,
                conn,
                PeerObservation {
                    pairing_key: Some(PairingKey::from_observation(key.as_bytes().to_vec())),
                    harness_label: Some(Token::parse("claude").unwrap()),
                    working_directory: Some("/work".into()),
                },
            )
            .expect("connected");
        Wire {
            to_adapter: Some(to_tx),
            from_adapter: from_rx,
            partial: Vec::new(),
            handle,
        }
    }

    /// Claude Code's opening of a channel server, ending with the first `tools/list`.
    fn shim(&self, key: &str) -> Wire {
        let mut w = self.connect(key);
        w.send(PROBE);
        let _ = w.frame();
        w.send(INITIALIZE);
        let _ = w.frame();
        w.send("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}");
        w.send("{\"method\":\"tools/list\",\"jsonrpc\":\"2.0\",\"id\":1}");
        let _ = w.frame();
        w
    }

    /// A hook handler's connection: one `SessionStart` input, as Claude Code writes it. The
    /// adapter ends the carrier once it has reported the input; the daemon (here, the test)
    /// then tells the core the connection is gone, which frees its place
    /// ([`Pipelines::disconnect`]).
    fn hook(&self, key: &str, session_id: &str, source: &str) {
        let w = self.connect(key);
        w.send(&format!(
            "{{\"session_id\":\"{session_id}\",\"transcript_path\":\"/home/u/.claude/projects/p/{session_id}.jsonl\",\"cwd\":\"/work\",\"hook_event_name\":\"SessionStart\",\"source\":\"{source}\"}}"
        ));
        let end = Instant::now() + WAIT;
        while !w.handle.is_closed() {
            assert!(Instant::now() < end, "the carrier was not closed");
            std::thread::sleep(Duration::from_millis(5));
        }
        // The core takes the pairing key of a signal on its own worker, after the adapter's
        // report returned, so a daemon that ended the connection at once could lose the key
        // (reported on the PR). Here the connection is released once the signal is decided.
        std::thread::sleep(Duration::from_millis(300));
        self.pipes.disconnect(&w.handle);
    }

    fn binding(&self, a: &Attachment) -> Option<SessionId> {
        self.pipes.binding(a)
    }

    fn wait_binding(&self, what: &str, a: &Attachment, want: impl Fn(Option<&SessionId>) -> bool) {
        let end = Instant::now() + WAIT;
        while !want(self.binding(a).as_ref()) {
            assert!(
                Instant::now() < end,
                "{what}: binding is {:?}; log {:?}",
                self.binding(a),
                self.log.entries()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// G5 acceptance: `session_id` from the hook input registers the session, `whoami` answers
/// from the core's binding, `/clear` and `--resume` give new sessions, and exit deregisters.
#[test]
fn the_hook_session_id_registers_the_session_across_start_resume_and_exit() {
    let d = Device::new();
    let n1 = "f40e86db-b0cb-439a-bb2d-17f1dd86c1d5";
    let n2 = "0a0b0c0d-1111-4222-8333-444455556666";
    let mut shim = d.shim("p1");
    let attachment = shim.handle.clone();
    let whoami = |w: &mut Wire, id| {
        let r = w.tool(id, "whoami");
        text_at(&r, &["result", "structuredContent", "session_id"]).map(str::to_owned)
    };

    // Before any hook input: an attachment, unbound. `whoami` is the core's refusal.
    assert_eq!(d.binding(&attachment), None);
    let r = shim.tool(2, "whoami");
    assert!(result_text(&r).starts_with("unauthorized"), "{r:?}");

    // A hook input observed with another key pairs with no attachment: nothing is bound.
    d.hook("p-other", "11111111-2222-3333-4444-555555555555", "startup");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(d.binding(&attachment), None);

    // `SessionStart` with `source` `startup`, from the same process: bound.
    d.hook("p1", n1, "startup");
    d.wait_binding("start", &attachment, |b| b.is_some());
    let first = d.binding(&attachment).unwrap();
    let r = shim.tool(3, "whoami");
    assert_eq!(
        text_at(&r, &["result", "structuredContent", "device_fingerprint"]),
        Some(d.fingerprint.as_str())
    );
    assert_eq!(
        text_at(&r, &["result", "structuredContent", "session_id"]),
        Some(first.as_str())
    );
    // The harness-native id is not the OAC id and appears nowhere in the result.
    assert!(!result_text(&r).contains("f40e86db"), "{r:?}");

    // `/clear`: the hook reports a new `session_id` with `source` `clear` on the same process.
    d.hook("p1", n2, "clear");
    d.wait_binding("clear", &attachment, |b| b.is_some_and(|b| *b != first));
    let cleared = d.binding(&attachment).unwrap();
    assert_eq!(whoami(&mut shim, 4).as_deref(), Some(cleared.as_str()));

    // The harness exits: the connection ends and the binding ends with it ([SC-ID-155]).
    shim.hang_up();
    d.wait_binding("exit", &attachment, |b| b.is_none());

    // `claude --resume <n1>`: a new process, so a new attachment; the same `session_id`.
    let mut resumed = d.shim("p2");
    let resumed_at = resumed.handle.clone();
    assert_eq!(d.binding(&resumed_at), None);
    d.hook("p2", n1, "resume");
    d.wait_binding("resume", &resumed_at, |b| b.is_some());
    let second = d.binding(&resumed_at).unwrap();
    assert!(second != first && second != cleared, "a new session id");
    assert_eq!(whoami(&mut resumed, 2).as_deref(), Some(second.as_str()));

    // A second live process reporting `n1` is refused; the first binding is kept.
    let mut twin = d.shim("p3");
    let twin_at = twin.handle.clone();
    d.hook("p3", n1, "resume");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(d.binding(&twin_at), None);
    assert_eq!(d.binding(&resumed_at), Some(second));
    let r = twin.tool(2, "whoami");
    assert!(result_text(&r).starts_with("unauthorized"), "{r:?}");

    resumed.hang_up();
    d.wait_binding("resumed exit", &resumed_at, |b| b.is_none());
    d.pipes.shutdown();
}
