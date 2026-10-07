// SPDX-License-Identifier: Apache-2.0

//! Self-test of the adapter contract suite (#59, F10).
//!
//! No adapter implements `ProviderAdapter` yet: the real Claude and Codex adapters are
//! Epic G (G4 to G8). So that the suite is not left untested until then, this file runs it
//! against two **stand-ins**: minimal test doubles that speak just enough of each fake
//! harness's surface to be driven end to end. They are not adapters, model no binding
//! beyond what the suite exercises, and evidence nothing about the real adapters.
//!
//! - Run as written, each stand-in must pass the whole suite: the Claude one under both
//!   mid-turn release settings of the fake Claude Code endpoint, the Codex one against the
//!   fake Codex app-server over loopback.
//! - Each planted breach must be reported under its requirement id, so that a pass means
//!   something.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use oac_contract_adapter::claude::{ClaudeHarness, PipeWriter, json_string, pipe};
use oac_contract_adapter::codex::{self, CodexFake, TUI_CLIENT};
use oac_contract_adapter::{
    AdapterHarness, CoreSide, Gap, HarnessRequest, Observations, ObservedResult, Profile, Report,
    Step, run,
};
use oac_core::adapter::{
    AdapterCapabilities, AdapterEvent, AdapterEventHandler, Attachment, Connection,
    DiscoveryRequest, DiscoveryRequestResult, HandOff, HandOffOutcome, ProviderAdapter,
    RequestSink, SendRequest, SendRequestResult,
};
use oac_core::envelope::{ContentPart, TextPart};
use oac_core::health::{HealthState, HealthStatus};
use oac_core::ids::SessionId;
use oac_core::json::{self, Json};
use oac_fake_claude::MidTurnRelease;
use oac_fake_claude::evidence::{member, member_str};

// The one planted breach that mints a connection handle lives in the suite's library
// (`plant`), outside the sources the static scan is given and outside every module this
// file declares (the scan follows `mod`, and refuses `#[path]`, #324), so that the
// well-behaved stand-ins scan clean. The `use` below is a finding of the TEST-PLANT row
// only, which the stand-ins' requirement rows do not read.
use oac_contract_adapter::plant;

fn this_file() -> Vec<PathBuf> {
    vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/stand_in.rs")]
}

fn hand_off_text(h: &HandOff) -> String {
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

/// What the stand-ins share: the core's handler and sink, each attachment's state, and
/// whether `shutdown` has returned.
#[derive(Default)]
struct Common {
    events: Option<AdapterEventHandler>,
    sink: Option<Arc<dyn RequestSink>>,
    bound: HashMap<Attachment, Option<SessionId>>,
    open: Vec<Attachment>,
    down: bool,
    last_tool_use: Option<String>,
}

impl Common {
    fn emit(me: &Mutex<Common>, e: AdapterEvent) {
        let h = me.lock().unwrap().events.clone();
        if let Some(h) = h {
            h(e);
        }
    }

    fn opened(me: &Mutex<Common>, a: &Attachment) {
        {
            let mut c = me.lock().unwrap();
            c.open.push(a.clone());
            c.bound.insert(a.clone(), None);
        }
        Common::emit(
            me,
            AdapterEvent::AttachmentOpened {
                attachment: a.clone(),
                cross_check: None,
            },
        );
    }

    fn closed(me: &Mutex<Common>, a: &Attachment) {
        let was_open = {
            let mut c = me.lock().unwrap();
            let n = c.open.len();
            c.open.retain(|x| x != a);
            n != c.open.len()
        };
        if was_open {
            Common::emit(
                me,
                AdapterEvent::AttachmentClosed {
                    attachment: a.clone(),
                },
            );
        }
    }

    fn shutdown(me: &Mutex<Common>, close: bool) {
        let open = {
            let mut c = me.lock().unwrap();
            c.down = true;
            c.open.clone()
        };
        if close {
            for a in open {
                Common::closed(me, &a);
            }
        }
    }

    /// The attachment's binding, or `None` when it may not be handed off to.
    fn may_hand_off(me: &Mutex<Common>, a: &Attachment, ignore_binding: bool) -> bool {
        let c = me.lock().unwrap();
        !c.down && (ignore_binding || matches!(c.bound.get(a), Some(Some(_))))
    }
}

// ---- the channel stand-in (fake Claude Code endpoint) -------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChannelBreach {
    None,
    /// Lists an inbox tool for the session to read repeatedly ([IFC-ADP-040]).
    OffersInbox,
    /// Hands off with no binding ([IFC-ADP-030]).
    IgnoresBinding,
    /// Two notifications per hand-off ([IFC-ADP-057]).
    DoubleCall,
    /// Labels requests with a handle it made ([IFC-ADP-031], [IFC-ADP-013]).
    ForgesAttachment,
    /// No attachment-closed at shutdown ([IFC-ADP-071]).
    KeepsAttachmentsAtShutdown,
    /// Answers send requests itself ([IFC-ADP-003]).
    BypassesTheCore,
    /// Reports every result as sent ([IFC-ADP-060]).
    AltersResults,
    /// Rewrites the content it hands off ([IFC-ADP-004]).
    RewritesContent,
    /// Keeps handing off after shutdown ([IFC-ADP-070]).
    WorksAfterShutdown,
    /// Declares an MCP `resources` capability, an inbox the session could read
    /// ([IFC-ADP-040]; [MCPB-DLV-002]).
    DeclaresResources,
    /// Names the last tool-use id it saw in its health detail ([IFC-TYP-092]).
    HealthNamesToolUse,
}

struct ChannelStandIn {
    common: Arc<Mutex<Common>>,
    writers: Arc<Mutex<HashMap<Attachment, Box<dyn Write + Send>>>>,
    breach: ChannelBreach,
}

impl ChannelStandIn {
    fn new(breach: ChannelBreach) -> ChannelStandIn {
        ChannelStandIn {
            common: Arc::default(),
            writers: Arc::default(),
            breach,
        }
    }
}

fn send_line(
    w: &Mutex<HashMap<Attachment, Box<dyn Write + Send>>>,
    a: &Attachment,
    line: &str,
) -> bool {
    let mut ws = w.lock().unwrap();
    match ws.get_mut(a) {
        Some(w) => w.write_all(format!("{line}\n").as_bytes()).is_ok(),
        None => false,
    }
}

fn respond(id: &Json, body: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{},{body}}}", id.to_compact())
}

fn tool_result(is_error: bool, text: &str) -> String {
    format!(
        "\"result\":{{\"content\":[{{\"type\":\"text\",\"text\":{}}}],\"isError\":{is_error}}}",
        json_string(text)
    )
}

impl ChannelStandIn {
    fn on_frame(
        common: &Mutex<Common>,
        writers: &Mutex<HashMap<Attachment, Box<dyn Write + Send>>>,
        breach: ChannelBreach,
        a: &Attachment,
        v: &Json,
    ) {
        let id = member(v, "id");
        match (member_str(v, "method"), id) {
            (Some("server/discover"), Some(id)) => {
                send_line(
                    writers,
                    a,
                    &respond(
                        id,
                        "\"error\":{\"code\":-32601,\"message\":\"Method not found\"}",
                    ),
                );
            }
            (Some("initialize"), Some(id)) => {
                send_line(
                    writers,
                    a,
                    &respond(
                        id,
                        &format!(
                            "\"result\":{{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{{\"experimental\":{{\"claude/channel\":{{}}}},\"tools\":{{}}{}}},\"serverInfo\":{{\"name\":\"oac-stand-in\",\"version\":\"0\"}}}}",
                            if breach == ChannelBreach::DeclaresResources {
                                ",\"resources\":{}"
                            } else {
                                ""
                            }
                        ),
                    ),
                );
            }
            (Some("notifications/initialized"), None) => Common::opened(common, a),
            (Some("tools/list"), Some(id)) => {
                let mut tools = vec!["send", "reply", "list_sessions", "whoami"];
                if breach == ChannelBreach::OffersInbox {
                    tools.push("inbox");
                }
                let list = tools
                    .iter()
                    .map(|t| {
                        format!("{{\"name\":\"{t}\",\"inputSchema\":{{\"type\":\"object\"}}}}")
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                send_line(
                    writers,
                    a,
                    &respond(id, &format!("\"result\":{{\"tools\":[{list}]}}")),
                );
            }
            (Some("tools/call"), Some(id)) => {
                let (down, sink) = {
                    let c = common.lock().unwrap();
                    (c.down, c.sink.clone())
                };
                if down || sink.is_none() {
                    send_line(
                        writers,
                        a,
                        &respond(id, &tool_result(true, "internal-error")),
                    );
                    return;
                }
                let sink = sink.unwrap();
                let params = member(v, "params");
                let name = params
                    .and_then(|p| member_str(p, "name"))
                    .unwrap_or_default();
                let args = params.and_then(|p| member(p, "arguments"));
                common.lock().unwrap().last_tool_use = params
                    .and_then(|p| member(p, "_meta"))
                    .and_then(|m| member_str(m, "claudecode/toolUseId"))
                    .map(str::to_owned);
                let label = if breach == ChannelBreach::ForgesAttachment {
                    plant::forged_attachment()
                } else {
                    a.clone()
                };
                let body = match name {
                    "send" => {
                        let to = args
                            .and_then(|x| member_str(x, "to"))
                            .and_then(SessionId::parse);
                        let text = args
                            .and_then(|x| member_str(x, "text"))
                            .and_then(TextPart::new);
                        let (Some(to), Some(text)) = (to, text) else {
                            send_line(
                                writers,
                                a,
                                &respond(id, &tool_result(true, "invalid-request")),
                            );
                            return;
                        };
                        if breach == ChannelBreach::BypassesTheCore {
                            tool_result(false, "m0 accepted-by-adapter")
                        } else {
                            let r = sink.send(SendRequest {
                                attachment: label,
                                to,
                                content: vec![ContentPart::Text(text)],
                                requested_target: None,
                                conversation_id: None,
                                correlation_id: None,
                            });
                            match (&r, breach) {
                                (_, ChannelBreach::AltersResults) => {
                                    tool_result(false, "m0 accepted-by-adapter")
                                }
                                (SendRequestResult::Sent { id: mid, .. }, _) => tool_result(
                                    false,
                                    &format!("{} accepted-by-adapter", mid.as_str()),
                                ),
                                (SendRequestResult::Refused { error }, _) => {
                                    tool_result(true, error.as_str())
                                }
                                (SendRequestResult::NotPassed { id: mid, error }, _) => {
                                    tool_result(
                                        true,
                                        &format!("{} {}", error.as_str(), mid.as_str()),
                                    )
                                }
                            }
                        }
                    }
                    "list_sessions" => {
                        match sink.discover(DiscoveryRequest { attachment: label }) {
                            DiscoveryRequestResult::DiscoveryResult(d) => {
                                tool_result(false, &format!("{} sessions", d.len()))
                            }
                            DiscoveryRequestResult::Refused { error } => {
                                tool_result(true, error.as_str())
                            }
                        }
                    }
                    _ => {
                        send_line(
                            writers,
                            a,
                            &respond(
                                id,
                                "\"error\":{\"code\":-32602,\"message\":\"unknown tool\"}",
                            ),
                        );
                        return;
                    }
                };
                send_line(writers, a, &respond(id, &body));
            }
            _ => {}
        }
    }
}

impl ProviderAdapter for ChannelStandIn {
    fn take_connection(&self, connection: Connection) {
        let (a, reader, writer) = connection.into_parts();
        self.writers.lock().unwrap().insert(a.clone(), writer);
        let (common, writers, breach) = (self.common.clone(), self.writers.clone(), self.breach);
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                if let Ok(v) = json::parse(line.as_bytes()) {
                    ChannelStandIn::on_frame(&common, &writers, breach, &a, &v);
                }
            }
            writers.lock().unwrap().remove(&a);
            Common::closed(&common, &a);
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
            max_envelope_octets: Some(65536),
        }
    }

    fn deliver(&self, hand_off: HandOff) -> HandOffOutcome {
        let a = hand_off.attachment();
        let ok = Common::may_hand_off(
            &self.common,
            a,
            self.breach == ChannelBreach::IgnoresBinding,
        ) || (self.breach == ChannelBreach::WorksAfterShutdown
            && self
                .common
                .lock()
                .unwrap()
                .bound
                .get(a)
                .is_some_and(Option::is_some));
        if !ok {
            return HandOffOutcome::Failed;
        }
        let mut text = hand_off_text(&hand_off);
        if self.breach == ChannelBreach::RewritesContent {
            text = text.to_uppercase();
        }
        let from = hand_off.message().envelope().from().as_str().to_owned();
        let frame = format!(
            "{{\"jsonrpc\":\"2.0\",\"method\":\"notifications/claude/channel\",\"params\":{{\"content\":{},\"meta\":{{\"oac_sender\":{}}}}}}}",
            json_string(&text),
            json_string(&from)
        );
        let calls = if self.breach == ChannelBreach::DoubleCall {
            2
        } else {
            1
        };
        let mut sent = true;
        for _ in 0..calls {
            sent &= send_line(&self.writers, a, &frame);
        }
        if sent {
            HandOffOutcome::Completed
        } else {
            HandOffOutcome::Failed
        }
    }

    fn accept_requests(&self, sink: Arc<dyn RequestSink>) {
        self.common.lock().unwrap().sink = Some(sink);
    }

    fn health(&self) -> HealthStatus {
        if self.breach == ChannelBreach::HealthNamesToolUse {
            let t = self.common.lock().unwrap().last_tool_use.clone();
            return HealthStatus::with_detail(
                HealthState::Healthy,
                format!("last call {}", t.unwrap_or_default()),
            );
        }
        HealthStatus::new(HealthState::Healthy)
    }

    fn shutdown(&self) {
        Common::shutdown(
            &self.common,
            self.breach != ChannelBreach::KeepsAttachmentsAtShutdown,
        );
    }
}

fn encode(r: &HarnessRequest) -> (String, String) {
    match r {
        HarnessRequest::Send {
            to,
            text,
            claimed_from,
        } => (
            "send".into(),
            format!(
                "{{\"to\":{},\"text\":{}{}}}",
                json_string(to.as_str()),
                json_string(text),
                claimed_from
                    .as_ref()
                    .map(|f| format!(",\"from\":{}", json_string(f.as_str())))
                    .unwrap_or_default()
            ),
        ),
        HarnessRequest::Discover => ("list_sessions".into(), "{}".into()),
    }
}

fn channel_report(breach: ChannelBreach, release: MidTurnRelease) -> Report {
    let mut h = ClaudeHarness::new(
        Arc::new(ChannelStandIn::new(breach)),
        release,
        encode,
        this_file(),
    );
    run(&mut h)
}

#[test]
fn the_channel_stand_in_passes_under_both_mid_turn_release_settings() {
    for release in MidTurnRelease::BOTH {
        let report = channel_report(ChannelBreach::None, release);
        println!("{report}");
        report.assert_conformant();
        assert_eq!(
            report.not_applicable(),
            ["IFC-ADP-043", "IFC-ADP-053", "IFC-ADP-054", "SEC-AUZ-027"],
            "{report}"
        );
    }
}

#[test]
fn the_suite_catches_each_planted_channel_breach() {
    let cases: [(ChannelBreach, &[&str]); 11] = [
        (ChannelBreach::HealthNamesToolUse, &["IFC-TYP-092"]),
        (ChannelBreach::DeclaresResources, &["IFC-ADP-040"]),
        (ChannelBreach::OffersInbox, &["IFC-ADP-040"]),
        (ChannelBreach::IgnoresBinding, &["IFC-ADP-030"]),
        (ChannelBreach::DoubleCall, &["IFC-ADP-057"]),
        (
            ChannelBreach::ForgesAttachment,
            &["IFC-ADP-031", "IFC-ADP-013"],
        ),
        (ChannelBreach::KeepsAttachmentsAtShutdown, &["IFC-ADP-071"]),
        (ChannelBreach::BypassesTheCore, &["IFC-ADP-003"]),
        (ChannelBreach::AltersResults, &["IFC-ADP-060"]),
        (ChannelBreach::RewritesContent, &["IFC-ADP-004"]),
        (ChannelBreach::WorksAfterShutdown, &["IFC-ADP-070"]),
    ];
    for (breach, ids) in cases {
        let report = channel_report(breach, MidTurnRelease::OnePerBoundary);
        for id in ids {
            assert!(
                report.failed(id),
                "{breach:?} not caught as {id}:\n{report}"
            );
        }
        assert!(report.failed("IFC-ADP-010"), "{report}");
    }
}

// ---- the queue stand-in (fake Codex app-server) --------------------------------------------

/// A minimal WebSocket client for loopback text frames (RFC 6455), enough to reach the
/// fake's listener.
struct Ws {
    s: TcpStream,
    next: u64,
}

impl Ws {
    fn connect(url: &str) -> Ws {
        let host = url
            .strip_prefix("ws://")
            .expect("a ws:// url")
            .trim_end_matches('/');
        let mut s = TcpStream::connect(host).expect("connect to the fake");
        write!(
            s,
            "GET / HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: b2FjLWNvbnRyYWN0LXN1aXRl\r\nSec-WebSocket-Version: 13\r\n\r\n"
        )
        .unwrap();
        let mut head = Vec::new();
        let mut b = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            s.read_exact(&mut b).unwrap();
            head.push(b[0]);
        }
        assert!(
            head.starts_with(b"HTTP/1.1 101"),
            "{}",
            String::from_utf8_lossy(&head)
        );
        Ws { s, next: 1 }
    }

    fn send(&mut self, text: &str) {
        let p = text.as_bytes();
        let mut f = vec![0x81u8];
        match p.len() {
            n if n < 126 => f.push(0x80 | n as u8),
            n if n <= 0xffff => {
                f.push(0x80 | 126);
                f.extend_from_slice(&(n as u16).to_be_bytes());
            }
            n => {
                f.push(0x80 | 127);
                f.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        f.extend_from_slice(&[0, 0, 0, 0]); // a zero mask key leaves the payload as it is
        f.extend_from_slice(p);
        self.s.write_all(&f).unwrap();
    }

    fn recv(&mut self) -> String {
        loop {
            let mut h = [0u8; 2];
            self.s.read_exact(&mut h).unwrap();
            let mut len = u64::from(h[1] & 0x7f);
            if len == 126 {
                let mut x = [0u8; 2];
                self.s.read_exact(&mut x).unwrap();
                len = u64::from(u16::from_be_bytes(x));
            } else if len == 127 {
                let mut x = [0u8; 8];
                self.s.read_exact(&mut x).unwrap();
                len = u64::from_be_bytes(x);
            }
            let mut p = vec![0u8; usize::try_from(len).unwrap()];
            self.s.read_exact(&mut p).unwrap();
            if h[0] & 0x0f == 0x1 {
                return String::from_utf8(p).unwrap();
            }
        }
    }

    /// One JSON-RPC call; notifications in between are skipped.
    fn call(&mut self, method: &str, params: &str) -> Result<Json, Json> {
        let id = self.next;
        self.next += 1;
        self.send(&format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":{},\"params\":{params}}}",
            json_string(method)
        ));
        loop {
            let v = json::parse(self.recv().as_bytes()).unwrap();
            if member(&v, "id").map(Json::to_compact) == Some(id.to_string()) {
                return match (member(&v, "result"), member(&v, "error")) {
                    (Some(r), _) => Ok(r.clone()),
                    (_, Some(e)) => Err(e.clone()),
                    _ => panic!("a response with neither result nor error"),
                };
            }
        }
    }
}

const STAND_IN_CLIENT: &str = "oac-stand-in";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueueBreach {
    None,
    /// Hands off with `turn/start` ([SEC-AUZ-022], [MCPB-CDX-002], [MCPB-CDX-003]).
    StartsTurns,
    /// Falls back to `turn/steer` when the queue turns it away ([SEC-AUZ-027],
    /// [MCPB-CDX-004]).
    FallsBackToSteer,
    /// Sends a setting override with the hand-off ([MCPB-CDX-005]).
    OverridesSettings,
    /// Reads the thread's turns after every hand-off ([IFC-ADP-040]).
    PollsTurns,
    /// Re-subscribes to the thread for every hand-off ([IFC-ADP-040]).
    ResubscribesPerMessage,
    /// Steers with no thread named, besides the proper hand-off ([SEC-AUZ-022],
    /// [MCPB-CDX-004]).
    SteersElsewhere,
    /// Names a thread id in its health detail ([IFC-TYP-092]).
    HealthNamesThread,
}

struct QueueStandIn {
    common: Arc<Mutex<Common>>,
    threads: Arc<Mutex<HashMap<Attachment, String>>>,
    ws: Mutex<Ws>,
    breach: QueueBreach,
    next: AtomicU64,
}

impl QueueStandIn {
    fn new(url: &str, breach: QueueBreach) -> QueueStandIn {
        let mut ws = Ws::connect(url);
        ws.call(
            "initialize",
            &format!(
                "{{\"clientInfo\":{{\"name\":\"{STAND_IN_CLIENT}\",\"version\":\"0\"}},\"capabilities\":{{\"experimentalApi\":true}}}}"
            ),
        )
        .expect("initialize");
        QueueStandIn {
            common: Arc::default(),
            threads: Arc::default(),
            ws: Mutex::new(ws),
            breach,
            next: AtomicU64::new(1),
        }
    }
}

impl ProviderAdapter for QueueStandIn {
    fn take_connection(&self, connection: Connection) {
        // The stand-in's own wiring: the harness names the session's thread on the
        // connection, then holds it open for the session's life.
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
            Common::closed(&common, &a);
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
            max_envelope_octets: Some(65536),
        }
    }

    fn deliver(&self, hand_off: HandOff) -> HandOffOutcome {
        let a = hand_off.attachment();
        if !Common::may_hand_off(&self.common, a, false) {
            return HandOffOutcome::Failed;
        }
        let Some(thread) = self.threads.lock().unwrap().get(a).cloned() else {
            return HandOffOutcome::Failed;
        };
        let input = format!(
            "[{{\"type\":\"text\",\"text\":{}}}]",
            json_string(&hand_off_text(&hand_off))
        );
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let mut ws = self.ws.lock().unwrap();
        if self.breach == QueueBreach::ResubscribesPerMessage {
            let _ = ws.call(
                "thread/resume",
                &format!(
                    "{{\"threadId\":{},\"excludeTurns\":true}}",
                    json_string(&thread)
                ),
            );
        }
        let (method, params) = match self.breach {
            QueueBreach::StartsTurns => (
                "turn/start",
                format!(
                    "{{\"threadId\":{},\"input\":{input}}}",
                    json_string(&thread)
                ),
            ),
            QueueBreach::OverridesSettings => (
                codex::HOLDING_HAND_OFF,
                format!(
                    "{{\"threadId\":{},\"input\":{input},\"clientUserMessageId\":\"oac-{n}\",\"model\":\"other\"}}",
                    json_string(&thread)
                ),
            ),
            _ => (
                codex::HOLDING_HAND_OFF,
                format!(
                    "{{\"threadId\":{},\"input\":{input},\"clientUserMessageId\":\"oac-{n}\"}}",
                    json_string(&thread)
                ),
            ),
        };
        let r = ws.call(method, &params);
        if self.breach == QueueBreach::SteersElsewhere {
            let _ = ws.call("turn/steer", &format!("{{\"input\":{input}}}"));
        }
        if self.breach == QueueBreach::PollsTurns {
            let _ = ws.call("thread/turns/list", &format!("{{\"threadId\":{},\"limit\":1,\"sortDirection\":\"desc\",\"itemsView\":\"full\"}}", json_string(&thread)));
        }
        match r {
            Ok(_) => HandOffOutcome::Completed,
            Err(_) if self.breach == QueueBreach::FallsBackToSteer => {
                let _ = ws.call(
                    "turn/steer",
                    &format!(
                        "{{\"threadId\":{},\"input\":{input}}}",
                        json_string(&thread)
                    ),
                );
                HandOffOutcome::Failed
            }
            // spec/bindings/mcp.md §8.2.1: none of the known refusals means "not now".
            Err(_) => HandOffOutcome::Failed,
        }
    }

    fn accept_requests(&self, sink: Arc<dyn RequestSink>) {
        self.common.lock().unwrap().sink = Some(sink);
    }

    fn health(&self) -> HealthStatus {
        if self.breach == QueueBreach::HealthNamesThread {
            let t = self.threads.lock().unwrap().values().next().cloned();
            return HealthStatus::with_detail(
                HealthState::Healthy,
                format!("serving {}", t.unwrap_or_default()),
            );
        }
        HealthStatus::new(HealthState::Healthy)
    }

    fn shutdown(&self) {
        Common::shutdown(&self.common, true);
    }
}

/// The stand-in's harness over the fake Codex app-server. Its session wiring (a thread made
/// with the fake's control method, named on the session's connection) is the stand-in's
/// own; a real Codex adapter's harness (G7) wires its sessions the way its binding says.
struct QueueHarness {
    fake: Arc<CodexFake>,
    adapter: Arc<QueueStandIn>,
    sessions: Vec<(String, Option<PipeWriter>)>,
    own: Vec<String>,
}

impl AdapterHarness for QueueHarness {
    fn name(&self) -> String {
        format!("fake Codex app-server ({:?} stand-in)", self.adapter.breach)
    }
    fn adapter(&self) -> Arc<dyn ProviderAdapter> {
        self.adapter.clone()
    }
    fn profile(&self) -> Profile {
        codex::PROFILE
    }
    fn open_session(&mut self, core: &CoreSide) -> Step<(usize, Vec<Connection>)> {
        let thread = self.fake.create_thread();
        let (mut to_adapter, adapter_reads) = pipe();
        // The stand-in writes nothing back on this connection.
        let (w, _never_read) = pipe();
        to_adapter
            .write_all(format!("{{\"thread\":{}}}\n", json_string(&thread)).as_bytes())
            .map_err(|e| Gap::Broken(e.to_string()))?;
        let conn = core.accept(adapter_reads, w);
        self.sessions.push((thread, Some(to_adapter)));
        Ok((self.sessions.len() - 1, vec![conn]))
    }
    fn session_ready(&mut self, _s: usize) -> Step<()> {
        Ok(())
    }
    fn end_session(&mut self, s: usize) -> Step<()> {
        self.sessions[s].1 = None;
        Ok(())
    }
    fn start_turn(&mut self, s: usize) -> Step<()> {
        let text = format!("tui turn {}", self.own.len());
        self.own.push(text.clone());
        self.fake
            .tui_turn(&self.sessions[s].0, &text)
            .map_err(|e| Gap::Broken(e.to_compact()))
    }
    fn drain(&mut self, s: usize) -> Step<()> {
        self.fake.drain(&self.sessions[s].0);
        Ok(())
    }
    fn refuse_hand_offs(&mut self, _s: usize) -> Step<()> {
        self.fake.set_queue_available(false);
        Ok(())
    }
    fn allow_hand_offs(&mut self, _s: usize) -> Step<()> {
        self.fake.set_queue_available(true);
        Ok(())
    }
    fn request(&mut self, _s: usize, _r: &HarnessRequest) -> Step<ObservedResult> {
        Err(Gap::Unsupported(
            "the fake Codex app-server models the app-server only; the MCP tool path that Codex requests travel (shared with Claude, spec/bindings/mcp.md section 5) is exercised through the Claude harness".into(),
        ))
    }
    fn observe(&mut self, s: usize) -> Observations {
        self.fake
            .observations(STAND_IN_CLIENT, &self.sessions[s].0, &self.own)
    }
    fn native_ids(&self) -> Vec<String> {
        self.sessions.iter().map(|(t, _)| t.clone()).collect()
    }
    fn source_files(&self) -> Vec<PathBuf> {
        this_file()
    }
}

fn queue_report(breach: QueueBreach) -> Report {
    let fake = Arc::new(CodexFake::spawn().expect("spawn the fake Codex app-server with node"));
    let adapter = Arc::new(QueueStandIn::new(fake.url(), breach));
    let mut h = QueueHarness {
        fake,
        adapter,
        sessions: Vec::new(),
        own: Vec::new(),
    };
    run(&mut h)
}

#[test]
fn the_queue_stand_in_passes_against_the_fake_codex_app_server() {
    let report = queue_report(QueueBreach::None);
    println!("{report}");
    report.assert_conformant();
    assert_eq!(
        report.not_applicable(),
        [
            "IFC-ADP-003",
            "IFC-ADP-031",
            "IFC-ADP-043",
            "IFC-ADP-053",
            "IFC-ADP-054",
            "IFC-ADP-060"
        ],
        "{report}"
    );
    for id in [
        "MCPB-CDX-002",
        "MCPB-CDX-003",
        "MCPB-CDX-004",
        "MCPB-CDX-005",
        "SEC-AUZ-027",
    ] {
        assert!(report.ids().contains(&id), "no {id} row:\n{report}");
    }
}

#[test]
fn the_suite_catches_each_planted_queue_breach() {
    let cases: [(QueueBreach, &[&str]); 7] = [
        (
            QueueBreach::SteersElsewhere,
            &["SEC-AUZ-022", "MCPB-CDX-004"],
        ),
        (QueueBreach::HealthNamesThread, &["IFC-TYP-092"]),
        (
            QueueBreach::StartsTurns,
            &["SEC-AUZ-022", "SEC-AUZ-025", "MCPB-CDX-002", "MCPB-CDX-003"],
        ),
        (
            QueueBreach::FallsBackToSteer,
            &["SEC-AUZ-027", "MCPB-CDX-004"],
        ),
        (QueueBreach::OverridesSettings, &["MCPB-CDX-005"]),
        (QueueBreach::PollsTurns, &["IFC-ADP-040"]),
        (QueueBreach::ResubscribesPerMessage, &["IFC-ADP-040"]),
    ];
    for (breach, ids) in cases {
        let report = queue_report(breach);
        for id in ids {
            assert!(
                report.failed(id),
                "{breach:?} not caught as {id}:\n{report}"
            );
        }
    }
}

#[test]
fn the_tui_client_is_not_the_stand_in() {
    assert_ne!(TUI_CLIENT, STAND_IN_CLIENT);
}
