// SPDX-License-Identifier: Apache-2.0

//! End to end through the core's send and receive pipelines (#313): the fake Claude Code
//! channel endpoint (F8) and the fake Codex app-server (F9), each behind a test adapter
//! that implements `ProviderAdapter`, with the in-memory transport (F7) between them and
//! `oac_core::pipeline::Pipelines` the only path from one to the other ([IFC-ADP-001]).
//! Fake-harness integration tier: CI-default, no live provider, no network beyond loopback
//! (the fake Codex app-server runs under `node`, as every CI image has it).
//!
//! The two test adapters are test doubles in this file, not the Epic G adapters: they speak
//! just enough of each fake's surface to carry a message each way (the channel notification
//! on the Claude side, the holding hand-off `thread/queue/add` on the Codex side). The fake
//! Codex app-server models no tool surface (the shared MCP tool path,
//! `spec/bindings/mcp.md` §5), so the Codex side's requests enter the adapter's request
//! sink from the test, as that tool path would pass them.
//!
//! Every outcome the run produces is checked against Table 8.1 (its state, for its observer)
//! and Table 8.3 (its code, for that state and observer, or `request` scope for a refusal).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oac_contract_adapter::claude::{ClaudeHarness, PipeWriter, json_string, pipe};
use oac_contract_adapter::codex::CodexFake;
use oac_contract_adapter::{AdapterHarness, CoreSide, HarnessRequest};
use oac_core::adapter::{
    AdapterCapabilities, AdapterEvent, AdapterEventHandler, Attachment, Connection, Correlation,
    HandOff, HandOffOutcome, ProviderAdapter, ReceiptStream, RequestSink, SendRequest,
    SendRequestResult,
};
use oac_core::authorization::{
    AuthorizationEngine, Grant, LocalSide, MemoryDecisionLog, OperatorConfirmed, PeerSide,
};
use oac_core::clock::{Clock, SystemClock};
use oac_core::delivery::{DeliveryState, ErrorCode, Observer, Scope};
use oac_core::envelope::{ContentPart, TextPart};
use oac_core::health::{HealthState, HealthStatus};
use oac_core::ids::{SessionId, Token};
use oac_core::json::{self, Json};
use oac_core::keys::{DeviceIdentity, DeviceKey};
use oac_core::pairing::{MemoryPairingStore, PairedPeer};
use oac_core::pipeline::{PipelineConfig, Pipelines};
use oac_core::receipt::DeliveryReceipt;
use oac_fake_claude::MidTurnRelease;
use oac_fake_claude::evidence::{member, member_str};
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};

const WAIT: Duration = Duration::from_secs(10);

/// Waits until `f` gives a value, for up to [`WAIT`]. Test-side only: the pipelines and the
/// adapters never wait this way.
fn eventually<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let end = Instant::now() + WAIT;
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn text_of(h: &HandOff) -> String {
    h.message()
        .envelope()
        .content()
        .iter()
        .filter_map(|p| match p {
            ContentPart::Text(t) => Some(t.text().to_owned()),
            ContentPart::Other(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What both test adapters share: the core's event handler and request sink, each
/// attachment's binding, and the receipt streams the core returned for `sent` requests
/// ([IFC-ADP-062]).
#[derive(Default)]
struct Common {
    events: Option<AdapterEventHandler>,
    sink: Option<Arc<dyn RequestSink>>,
    bound: HashMap<Attachment, Option<SessionId>>,
    receipts: HashMap<Token, ReceiptStream>,
}

impl Common {
    fn emit(me: &Mutex<Common>, e: AdapterEvent) {
        let h = me.lock().unwrap().events.clone();
        if let Some(h) = h {
            h(e);
        }
    }

    fn opened(me: &Mutex<Common>, a: &Attachment) {
        me.lock().unwrap().bound.insert(a.clone(), None);
        Common::emit(
            me,
            AdapterEvent::AttachmentOpened {
                attachment: a.clone(),
                cross_check: None,
            },
        );
    }

    fn bound(me: &Mutex<Common>, a: &Attachment) -> bool {
        me.lock().unwrap().bound.get(a).is_some_and(Option::is_some)
    }

    /// Passes `request` into the core's sink ([IFC-ADP-003]) and keeps the receipt stream.
    fn send(me: &Mutex<Common>, request: SendRequest) -> SendRequestResult {
        let sink = me.lock().unwrap().sink.clone().expect("accept_requests");
        let result = sink.send(request);
        check_result_tables(&result);
        match result {
            SendRequestResult::Sent {
                id,
                correlation,
                receipts,
            } => {
                me.lock().unwrap().receipts.insert(id.clone(), receipts);
                SendRequestResult::Sent {
                    id,
                    correlation,
                    receipts: ReceiptStream(std::sync::mpsc::channel().1),
                }
            }
            other => other,
        }
    }

    fn receipt(me: &Mutex<Common>, id: &Token) -> DeliveryReceipt {
        let r = {
            let c = me.lock().unwrap();
            c.receipts
                .get(id)
                .expect("a receipt stream")
                .0
                .recv_timeout(WAIT)
                .expect("a receipt")
        };
        check_receipt_tables(&r);
        r
    }
}

// ---- the channel test adapter (fake Claude Code endpoint) ---------------------------------

/// A test double of a channel adapter: it answers the MCP opening, offers a `send` tool
/// whose calls go to the core, and hands off with one channel notification.
#[derive(Default)]
struct ChannelAdapter {
    common: Arc<Mutex<Common>>,
    writers: Arc<Mutex<HashMap<Attachment, Box<dyn Write + Send>>>>,
}

fn write_line(
    w: &Mutex<HashMap<Attachment, Box<dyn Write + Send>>>,
    a: &Attachment,
    s: &str,
) -> bool {
    w.lock()
        .unwrap()
        .get_mut(a)
        .is_some_and(|w| w.write_all(format!("{s}\n").as_bytes()).is_ok())
}

fn tool_result(id: &Json, is_error: bool, text: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{{\"content\":[{{\"type\":\"text\",\"text\":{}}}],\"isError\":{is_error}}}}}",
        id.to_compact(),
        json_string(text)
    )
}

impl ChannelAdapter {
    fn on_frame(
        common: &Mutex<Common>,
        writers: &Mutex<HashMap<Attachment, Box<dyn Write + Send>>>,
        a: &Attachment,
        v: &Json,
    ) {
        let respond = |id: &Json, body: &str| {
            write_line(
                writers,
                a,
                &format!("{{\"jsonrpc\":\"2.0\",\"id\":{},{body}}}", id.to_compact()),
            );
        };
        match (member_str(v, "method"), member(v, "id")) {
            (Some("server/discover"), Some(id)) => respond(
                id,
                "\"error\":{\"code\":-32601,\"message\":\"Method not found\"}",
            ),
            (Some("initialize"), Some(id)) => respond(
                id,
                "\"result\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{\"experimental\":{\"claude/channel\":{}},\"tools\":{}},\"serverInfo\":{\"name\":\"oac-e2e\",\"version\":\"0\"}}",
            ),
            (Some("notifications/initialized"), None) => Common::opened(common, a),
            (Some("tools/list"), Some(id)) => respond(
                id,
                "\"result\":{\"tools\":[{\"name\":\"send\",\"inputSchema\":{\"type\":\"object\"}}]}",
            ),
            (Some("tools/call"), Some(id)) => {
                let args = member(v, "params").and_then(|p| member(p, "arguments"));
                let to = args
                    .and_then(|x| member_str(x, "to"))
                    .and_then(SessionId::parse);
                let text = args
                    .and_then(|x| member_str(x, "text"))
                    .and_then(TextPart::new);
                let line = match (to, text) {
                    (Some(to), Some(text)) => match Common::send(
                        common,
                        SendRequest {
                            attachment: a.clone(),
                            to,
                            content: vec![ContentPart::Text(text)],
                            requested_target: None,
                            conversation_id: None,
                            correlation_id: None,
                        },
                    ) {
                        SendRequestResult::Sent { id: m, .. } => {
                            tool_result(id, false, &format!("{} accepted-by-adapter", m.as_str()))
                        }
                        SendRequestResult::Refused { error } => {
                            tool_result(id, true, error.as_str())
                        }
                        SendRequestResult::NotPassed { id: m, error } => {
                            tool_result(id, true, &format!("{} {}", error.as_str(), m.as_str()))
                        }
                    },
                    _ => tool_result(id, true, "invalid-request"),
                };
                write_line(writers, a, &line);
            }
            _ => {}
        }
    }
}

impl ProviderAdapter for ChannelAdapter {
    fn take_connection(&self, connection: Connection) {
        let (a, reader, writer) = connection.into_parts();
        self.writers.lock().unwrap().insert(a.clone(), writer);
        let (common, writers) = (self.common.clone(), self.writers.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                if let Ok(v) = json::parse(line.as_bytes()) {
                    ChannelAdapter::on_frame(&common, &writers, &a, &v);
                }
            }
            writers.lock().unwrap().remove(&a);
            Common::emit(&common, AdapterEvent::AttachmentClosed { attachment: a });
        });
    }

    fn watch_attachments(&self, handler: AdapterEventHandler) {
        self.common.lock().unwrap().events = Some(handler);
    }

    fn set_binding(&self, attachment: &Attachment, session: Option<SessionId>) {
        self.common
            .lock()
            .unwrap()
            .bound
            .insert(attachment.clone(), session);
    }

    fn capabilities(&self, _attachment: &Attachment) -> AdapterCapabilities {
        AdapterCapabilities {
            active_inbound: true,
            content_types: Vec::new(),
            max_envelope_octets: None,
        }
    }

    fn deliver(&self, hand_off: HandOff) -> HandOffOutcome {
        let a = hand_off.attachment();
        if !Common::bound(&self.common, a) {
            return HandOffOutcome::Failed;
        }
        // Provenance from the verified envelope, never from content ([SEC-PRV-002]).
        let from = hand_off.message().envelope().from().as_str().to_owned();
        let frame = format!(
            "{{\"jsonrpc\":\"2.0\",\"method\":\"notifications/claude/channel\",\"params\":{{\"content\":{},\"meta\":{{\"oac_sender\":{}}}}}}}",
            json_string(&text_of(&hand_off)),
            json_string(&from)
        );
        // The harness gives no acknowledgement: the end of the write is the end of the call
        // (§8.1.3).
        if write_line(&self.writers, a, &frame) {
            HandOffOutcome::Completed
        } else {
            HandOffOutcome::Failed
        }
    }

    fn accept_requests(&self, sink: Arc<dyn RequestSink>) {
        self.common.lock().unwrap().sink = Some(sink);
    }

    fn health(&self) -> HealthStatus {
        HealthStatus::new(HealthState::Healthy)
    }

    fn shutdown(&self) {
        let open: Vec<Attachment> = self.common.lock().unwrap().bound.keys().cloned().collect();
        for a in open {
            Common::emit(
                &self.common,
                AdapterEvent::AttachmentClosed { attachment: a },
            );
        }
    }
}

// ---- the queue test adapter (fake Codex app-server) ---------------------------------------

const E2E_CLIENT: &str = "oac-e2e-codex";

/// A test double of a Codex adapter: each session's connection names its thread on its
/// first line, and a hand-off is one holding `thread/queue/add`, never a steering call
/// ([SEC-AUZ-022]).
struct QueueAdapter {
    fake: Arc<CodexFake>,
    common: Arc<Mutex<Common>>,
    threads: Arc<Mutex<HashMap<Attachment, String>>>,
    next: AtomicU64,
}

impl QueueAdapter {
    fn new(fake: Arc<CodexFake>) -> QueueAdapter {
        fake.call(
            "initialize",
            &format!(
                "{{\"clientInfo\":{{\"name\":\"{E2E_CLIENT}\",\"version\":\"0\"}},\"capabilities\":{{\"experimentalApi\":true}}}}"
            ),
        )
        .expect("initialize the fake Codex app-server");
        QueueAdapter {
            fake,
            common: Arc::default(),
            threads: Arc::default(),
            next: AtomicU64::new(1),
        }
    }

    /// The session's harness asks to send (through the tool path the fake does not model).
    fn request(
        &self,
        attachment: &Attachment,
        to: &SessionId,
        text: &str,
        requested_target: Option<&Token>,
    ) -> SendRequestResult {
        Common::send(
            &self.common,
            SendRequest {
                attachment: attachment.clone(),
                to: to.clone(),
                content: vec![ContentPart::Text(TextPart::new(text).unwrap())],
                requested_target: requested_target.map(|t| t.as_str().to_owned()),
                conversation_id: None,
                correlation_id: None,
            },
        )
    }
}

impl ProviderAdapter for QueueAdapter {
    fn take_connection(&self, connection: Connection) {
        let (a, reader, _writer) = connection.into_parts();
        let (common, threads) = (self.common.clone(), self.threads.clone());
        std::thread::spawn(move || {
            let mut lines = BufReader::new(reader).lines().map_while(Result::ok);
            if let Some(first) = lines.next()
                && let Some(t) = json::parse(first.as_bytes())
                    .ok()
                    .and_then(|v| member_str(&v, "thread").map(str::to_owned))
            {
                threads.lock().unwrap().insert(a.clone(), t);
                Common::opened(&common, &a);
            }
            for _ in lines {}
            Common::emit(&common, AdapterEvent::AttachmentClosed { attachment: a });
        });
    }

    fn watch_attachments(&self, handler: AdapterEventHandler) {
        self.common.lock().unwrap().events = Some(handler);
    }

    fn set_binding(&self, attachment: &Attachment, session: Option<SessionId>) {
        self.common
            .lock()
            .unwrap()
            .bound
            .insert(attachment.clone(), session);
    }

    fn capabilities(&self, _attachment: &Attachment) -> AdapterCapabilities {
        AdapterCapabilities {
            active_inbound: true,
            content_types: Vec::new(),
            max_envelope_octets: None,
        }
    }

    fn deliver(&self, hand_off: HandOff) -> HandOffOutcome {
        let a = hand_off.attachment();
        if !Common::bound(&self.common, a) {
            return HandOffOutcome::Failed;
        }
        let Some(thread) = self.threads.lock().unwrap().get(a).cloned() else {
            return HandOffOutcome::Failed;
        };
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let params = format!(
            "{{\"threadId\":{},\"input\":[{{\"type\":\"text\",\"text\":{}}}],\"clientUserMessageId\":\"oac-{n}\"}}",
            json_string(&thread),
            json_string(&text_of(&hand_off))
        );
        match self.fake.call("thread/queue/add", &params) {
            Ok(_) => HandOffOutcome::Completed,
            // `spec/bindings/mcp.md` §8.2.1: no known refusal means "not now", and a refused
            // holding hand-off is never retried by steering.
            Err(_) => HandOffOutcome::Failed,
        }
    }

    fn accept_requests(&self, sink: Arc<dyn RequestSink>) {
        self.common.lock().unwrap().sink = Some(sink);
    }

    fn health(&self) -> HealthStatus {
        HealthStatus::new(HealthState::Healthy)
    }

    fn shutdown(&self) {
        let open: Vec<Attachment> = self.common.lock().unwrap().bound.keys().cloned().collect();
        for a in open {
            Common::emit(
                &self.common,
                AdapterEvent::AttachmentClosed { attachment: a },
            );
        }
    }
}

/// The user-message inputs the fake Codex app-server's thread took into its turns, once
/// the running turn, if any, has completed (the fake lists finished turns only).
fn codex_inputs(fake: &CodexFake, thread: &str) -> Vec<String> {
    fake.drain(thread);
    let st = fake.thread_state(thread);
    let mut out = Vec::new();
    for t in member(&st, "turns").and_then(Json::as_array).unwrap_or(&[]) {
        for item in member(t, "items").and_then(Json::as_array).unwrap_or(&[]) {
            if member_str(item, "type") == Some("userMessage") {
                let text = member(item, "content")
                    .and_then(Json::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|i| member_str(i, "text"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                out.push(text);
            }
        }
    }
    out
}

// ---- Tables 8.1 and 8.3 -------------------------------------------------------------------

fn check_result_tables(r: &SendRequestResult) {
    match r {
        SendRequestResult::Refused { error } => {
            assert!(error.scope().contains(&Scope::Request), "{error}");
            assert_eq!(r.state(), None);
        }
        SendRequestResult::NotPassed { error, .. } => {
            assert_eq!(r.state(), Some(DeliveryState::Failed));
            assert!(error.fits_receipt(DeliveryState::Failed, Observer::Sender));
        }
        SendRequestResult::Sent { .. } => {
            assert_eq!(r.state(), Some(DeliveryState::AcceptedByAdapter));
        }
    }
}

fn check_receipt_tables(r: &DeliveryReceipt) {
    assert_eq!(r.observer(), Observer::Receiver);
    assert!(r.state().allowed_for(Observer::Receiver), "{}", r.state());
    match r.error() {
        None => assert!(!r.state().carries_error(), "{}", r.state()),
        Some(e) => {
            let code = ErrorCode::parse(e.as_str()).expect("a Table 8.3 code");
            assert!(code.fits_receipt(r.state(), Observer::Receiver), "{code}");
        }
    }
}

fn code(r: &DeliveryReceipt) -> (DeliveryState, Option<String>) {
    (r.state(), r.error().map(|e| e.as_str().to_owned()))
}

// ---- the run ------------------------------------------------------------------------------

fn encode(r: &HarnessRequest) -> (String, String) {
    match r {
        HarnessRequest::Send { to, text, .. } => (
            "send".into(),
            format!(
                "{{\"to\":{},\"text\":{}}}",
                json_string(to.as_str()),
                json_string(text)
            ),
        ),
        HarnessRequest::Discover => ("list_sessions".into(), "{}".into()),
    }
}

/// Binds `attachment` as a session of this device once its adapter has reported it open.
fn bind(pipes: &Pipelines, attachment: &Attachment, seed: u8, label: &str) -> SessionId {
    let sid = SessionId::from_random_octets([seed; 16]);
    let record = pipes
        .device()
        .register(
            sid.clone(),
            Token::parse(label).unwrap(),
            &format!("native-{seed}"),
            "/oac-e2e",
            SystemClock.now(),
        )
        .unwrap();
    eventually("the attachment to open", || {
        pipes.bind(attachment, &record, None).ok()
    });
    sid
}

fn run(release: MidTurnRelease) {
    // One device: one key, one in-memory network, one pipeline.
    let network = MemoryNetwork::new();
    let identity = DeviceIdentity::new(DeviceKey::generate(), Token::parse("oac-e2e").unwrap());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let engine =
        AuthorizationEngine::new(&identity, clock.clone(), Box::new(MemoryDecisionLog::new()));
    let net = network.clone();
    let pipes = Pipelines::new(
        identity,
        engine,
        clock,
        Arc::new(move || net.now()),
        Arc::new(MemoryTransport::new()),
        PipelineConfig::default(),
    );
    pipes.start(MemoryConfiguration::wrap(&network)).unwrap();

    // The Claude side: the fake Claude Code endpoint opens MCP with the channel adapter.
    let channel = Arc::new(ChannelAdapter::default());
    let claude_id = pipes.add_adapter(channel.clone());
    let mut claude = ClaudeHarness::new(channel.clone(), release, encode, Vec::new());
    let accept = CoreSide::new();
    let (cs, conns) = claude.open_session(&accept).unwrap();
    let claude_att = conns[0].handle().clone();
    for c in conns {
        pipes.connect(claude_id, c).unwrap();
    }
    claude.session_ready(cs).unwrap();
    let claude_sid = bind(&pipes, &claude_att, 1, "claude-code");

    // The Codex side: a thread on the fake app-server, named on the session's connection.
    let fake = Arc::new(CodexFake::spawn().expect("spawn the fake Codex app-server with node"));
    let thread = fake.create_thread();
    let queue = Arc::new(QueueAdapter::new(fake.clone()));
    let codex_id = pipes.add_adapter(queue.clone());
    let (mut to_adapter, adapter_reads) = pipe();
    let (writer, _never_read): (PipeWriter, _) = pipe();
    to_adapter
        .write_all(format!("{{\"thread\":{}}}\n", json_string(&thread)).as_bytes())
        .unwrap();
    let conn = Connection::accept(adapter_reads, writer);
    let codex_att = conn.handle().clone();
    pipes.connect(codex_id, conn).unwrap();
    let codex_sid = bind(&pipes, &codex_att, 2, "codex");

    // Default deny: before any grant, Claude cannot even see the Codex session.
    let r = claude
        .request(
            cs,
            &HarnessRequest::Send {
                to: codex_sid.clone(),
                text: "too early".into(),
                claimed_from: None,
            },
        )
        .unwrap();
    assert!(r.is_error && r.text == "unknown-destination", "{r:?}");
    assert!(codex_inputs(&fake, &thread).is_empty());

    // One grant, "the Claude session may write to the Codex session" (spec/security.md
    // §9.5): it carries the message and the correlated reply.
    let store = MemoryPairingStore::new();
    pipes.with_engine(|e| {
        let own = e.own_key_id().clone();
        e.add_grant(
            Grant::Inbound {
                writer: PeerSide::session(own, claude_sid.clone()),
                target: LocalSide::Session(codex_sid.clone()),
            },
            OperatorConfirmed::by_operator(),
            &store,
        )
        .unwrap();
    });

    // Claude -> Codex: the tool call reaches the core through the adapter's sink, the
    // envelope crosses the in-memory transport, and the Codex adapter hands it off with one
    // holding call.
    let r = claude
        .request(
            cs,
            &HarnessRequest::Send {
                to: codex_sid.clone(),
                text: "Please review the scheduler module.".into(),
                claimed_from: None,
            },
        )
        .unwrap();
    assert!(!r.is_error, "{r:?}");
    let id = Token::parse(r.text.split(' ').next().unwrap()).unwrap();
    assert!(r.text.ends_with(" accepted-by-adapter"), "{r:?}");
    let receipt = Common::receipt(&channel.common, &id);
    assert_eq!(code(&receipt), (DeliveryState::HandedToHarness, None));
    assert_eq!(
        eventually("the Codex input", || {
            let i = codex_inputs(&fake, &thread);
            (!i.is_empty()).then_some(i)
        }),
        ["Please review the scheduler module."]
    );
    assert_eq!(
        pipes.delivery_state(&id, &claude_sid),
        Some(DeliveryState::HandedToHarness)
    );

    // Codex -> Claude: a correlated reply under the reply right ([SEC-AUZ-014]); Claude's
    // idle session wakes on the channel notification, actively, with no polling.
    let reply = queue.request(&codex_att, &claude_sid, "Reviewed: two issues.", Some(&id));
    let reply_id = match reply {
        SendRequestResult::Sent {
            id, correlation, ..
        } => {
            assert_eq!(correlation, Some(Correlation::Correlated));
            id
        }
        other => panic!("{other:?}"),
    };
    let receipt = Common::receipt(&queue.common, &reply_id);
    assert_eq!(code(&receipt), (DeliveryState::HandedToHarness, None));
    let woke = eventually("Claude to wake", || {
        let o = claude.observe(cs);
        (!o.inputs.is_empty()).then_some(o)
    });
    assert_eq!(woke.inputs, ["Reviewed: two issues."]);
    assert!(woke.fetch_calls.is_empty(), "{:?}", woke.fetch_calls);
    assert_eq!(woke.hand_off_calls.len(), 1);
    assert!(woke.halted.is_none(), "{:?}", woke.halted);

    // An uncorrelated message from Codex to Claude has no grant: the core sends it, and the
    // receiving side refuses it at authorization ([SEC-AUZ-001]).
    let r = queue.request(&codex_att, &claude_sid, "unsolicited", None);
    let SendRequestResult::Sent { id: lone, .. } = r else {
        panic!("{r:?}")
    };
    let receipt = Common::receipt(&queue.common, &lone);
    assert_eq!(
        code(&receipt),
        (DeliveryState::Rejected, Some("unauthorized".into()))
    );

    // A request to a session that does not exist is refused before any envelope.
    let ghost = SessionId::from_random_octets([9; 16]);
    let r = queue.request(&codex_att, &ghost, "anyone?", None);
    assert!(
        matches!(
            r,
            SendRequestResult::Refused {
                error: ErrorCode::UnknownDestination
            }
        ),
        "{r:?}"
    );

    // The Codex harness turns the holding hand-off away: `failed` with `handoff-failed`,
    // and no steering fallback.
    fake.set_queue_available(false);
    let r = claude
        .request(
            cs,
            &HarnessRequest::Send {
                to: codex_sid.clone(),
                text: "while unavailable".into(),
                claimed_from: None,
            },
        )
        .unwrap();
    let refused_id = Token::parse(r.text.split(' ').next().unwrap()).unwrap();
    let receipt = Common::receipt(&channel.common, &refused_id);
    assert_eq!(
        code(&receipt),
        (DeliveryState::Failed, Some("handoff-failed".into()))
    );
    fake.set_queue_available(true);
    assert_eq!(codex_inputs(&fake, &thread).len(), 1);

    // Claude's session ends: the core unbinds it, and the Codex side's next send to it finds
    // it gone (`unknown` to the requester once unbound, [SC-DLV-075]).
    claude.end_session(cs).unwrap();
    eventually("the Claude attachment to close", || {
        pipes.binding(&claude_att).is_none().then_some(())
    });
    let r = queue.request(&codex_att, &claude_sid, "still there?", Some(&id));
    assert!(matches!(r, SendRequestResult::Refused { .. }), "{r:?}");

    pipes.shutdown();
    drop(to_adapter);
}

/// The end-to-end run under each mid-turn release setting of the fake Claude Code endpoint
/// (`oac_fake_claude::MidTurnRelease::BOTH`).
#[test]
fn claude_and_codex_fakes_exchange_through_the_core_pipelines() {
    for release in MidTurnRelease::BOTH {
        run(release);
    }
}

fn device(network: &MemoryNetwork, principal: &str) -> Pipelines {
    let identity = DeviceIdentity::new(DeviceKey::generate(), Token::parse(principal).unwrap());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let engine =
        AuthorizationEngine::new(&identity, clock.clone(), Box::new(MemoryDecisionLog::new()));
    let net = network.clone();
    let pipes = Pipelines::new(
        identity,
        engine,
        clock,
        Arc::new(move || net.now()),
        Arc::new(MemoryTransport::new()),
        PipelineConfig::default(),
    );
    pipes.start(MemoryConfiguration::wrap(network)).unwrap();
    pipes
}

fn pair_with(me: &Pipelines, peer: &Pipelines, store: &MemoryPairingStore) {
    let p = PairedPeer::by_key_id_comparison(
        peer.device().principal().clone(),
        *peer.device().public_key(),
        peer.device().key_id(),
        SystemClock.now(),
        OperatorConfirmed::by_operator(),
    )
    .unwrap();
    me.with_engine(|e| e.pair(p, store).unwrap());
}

/// Two devices over one `cross-implementation` in-memory network, paired, with the grant
/// "the Claude session on x may write to the Codex session on y" recorded on both.
///
/// The in-memory transport does not declare `destination_restricted`: it cannot keep a
/// payload to the device its `Destination` names, so it may not claim to ([IFC-TRN-021]).
/// [IFC-TRN-081] then keeps every presence record off it for another implementation, so x
/// never holds y's declaration and nothing crosses: x's send is refused before any envelope
/// exists, and y's harness gets nothing. The announcement, the wire receipt and the send
/// order between two devices are exercised over a destination-restricted test transport in
/// `core/tests/pipeline.rs` (`two_devices_...`, `an_announcement_...`, `a_send_does_not_...`).
#[test]
fn two_devices_over_the_memory_transport_exchange_no_presence() {
    let network = MemoryNetwork::builder()
        .reach(oac_core::transport::Reach::CrossImplementation)
        .build();
    let (x, y) = (device(&network, "device-x"), device(&network, "device-y"));
    let (xs, ys) = (MemoryPairingStore::new(), MemoryPairingStore::new());
    pair_with(&x, &y, &xs);
    pair_with(&y, &x, &ys);

    let channel = Arc::new(ChannelAdapter::default());
    let claude_id = x.add_adapter(channel.clone());
    let mut claude = ClaudeHarness::new(
        channel.clone(),
        MidTurnRelease::OnePerBoundary,
        encode,
        Vec::new(),
    );
    let accept = CoreSide::new();
    let (cs, conns) = claude.open_session(&accept).unwrap();
    let claude_att = conns[0].handle().clone();
    for c in conns {
        x.connect(claude_id, c).unwrap();
    }
    claude.session_ready(cs).unwrap();

    let fake = Arc::new(CodexFake::spawn().expect("spawn the fake Codex app-server with node"));
    let thread = fake.create_thread();
    let queue = Arc::new(QueueAdapter::new(fake.clone()));
    let codex_id = y.add_adapter(queue.clone());
    let (mut to_adapter, adapter_reads) = pipe();
    let (writer, _never_read): (PipeWriter, _) = pipe();
    to_adapter
        .write_all(format!("{{\"thread\":{}}}\n", json_string(&thread)).as_bytes())
        .unwrap();
    let conn = Connection::accept(adapter_reads, writer);
    let codex_att = conn.handle().clone();
    y.connect(codex_id, conn).unwrap();

    let claude_sid = SessionId::from_random_octets([1; 16]);
    let codex_sid = SessionId::from_random_octets([2; 16]);
    let (xk, yk) = (x.device().key_id().clone(), y.device().key_id().clone());
    x.with_engine(|e| {
        e.add_grant(
            Grant::Outbound {
                writer: LocalSide::Session(claude_sid.clone()),
                target: PeerSide::session(yk, codex_sid.clone()),
            },
            OperatorConfirmed::by_operator(),
            &xs,
        )
        .unwrap();
    });
    y.with_engine(|e| {
        e.add_grant(
            Grant::Inbound {
                writer: PeerSide::session(xk, claude_sid.clone()),
                target: LocalSide::Session(codex_sid.clone()),
            },
            OperatorConfirmed::by_operator(),
            &ys,
        )
        .unwrap();
    });
    assert_eq!(bind(&x, &claude_att, 1, "claude-code"), claude_sid);
    assert_eq!(bind(&y, &codex_att, 2, "codex"), codex_sid);
    network.settle();

    let r = claude
        .request(
            cs,
            &HarnessRequest::Send {
                to: codex_sid.clone(),
                text: "across devices".into(),
                claimed_from: None,
            },
        )
        .unwrap();
    assert!(r.is_error && r.text == "unknown-destination", "{r:?}");
    network.settle();
    assert!(codex_inputs(&fake, &thread).is_empty());
    assert_eq!(network.in_flight(), 0);

    x.shutdown();
    y.shutdown();
    drop(to_adapter);
}
