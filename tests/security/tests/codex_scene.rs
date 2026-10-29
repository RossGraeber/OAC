// SPDX-License-Identifier: Apache-2.0

//! The Codex scene shared by the security tests that drive the Codex adapter (06 rows 12, 17
//! and 18; `never_steer.rs`, `provenance.rs`): the fake Codex app-server spawned with
//! `node`, a stand-in for the core, a carrier and a shim connection. It holds no test: each
//! test file takes it with `mod codex_scene;` (cargo also builds it as a target of its own,
//! with none to run). CI-default tier: no Codex install, no credential, no network beyond
//! loopback.

#![allow(dead_code, clippy::new_without_default)]

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use oac_adapter_codex::CodexAdapter;
use oac_adapter_codex::client::Options;
use oac_adapter_codex::pairing::Config;
use oac_core::adapter::{
    AdapterEvent, Attachment, Connection, ConnectionHandle, Correlation, DiscoveryRequest,
    DiscoveryRequestResult, HandOff, HandOffOutcome, ProviderAdapter, ReceiptStream, RequestSink,
    SendRequest, SendRequestResult,
};
use oac_core::clock::{Clock, SystemClock};
use oac_core::connection::{ConnectionWriter, pipe};
use oac_core::delivery::ErrorCode;
use oac_core::envelope::{EnvelopeDraft, EnvelopeLimits, TextPart, receive_envelope};
use oac_core::ids::{SessionId, Token};
use oac_core::json::{self, Json};
use oac_core::keys::{DeviceIdentity, DeviceKey};
use oac_core::reply::{EnvelopeRecord, reply_headers};
use oac_core::signing::authenticate;
use oac_core::trust::TrustedKeySet;

// ---- the Codex scene -------------------------------------------------------------------

pub const NAME: &str = "oac-test";
pub const TUI: &str = "oac-g7-test-tui";
pub const WAIT: Duration = Duration::from_secs(10);
pub const STARTUP: Duration = Duration::from_secs(60);
pub const CONFIRM_WAIT: Duration = Duration::from_millis(400);
/// The members of a hand-off's `thread/queue/add` and no others ([MCPB-CDX-005]).
pub const ADD_MEMBERS: [&str; 3] = ["clientUserMessageId", "input", "threadId"];

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn js(s: &str) -> String {
    Json::String(s.to_owned()).to_compact()
}

pub static NULL: Json = Json::Null;

/// `v.a.b`: the member at a dotted path, or `null`.
pub fn at<'a>(v: &'a Json, path: &str) -> &'a Json {
    path.split('.').fold(v, |cur, name| {
        cur.as_object().and_then(|o| o.get(name)).unwrap_or(&NULL)
    })
}

pub fn array(v: &Json) -> &[Json] {
    v.as_array().unwrap_or(&[])
}

pub fn array_len(v: &Json) -> usize {
    array(v).len()
}

/// What a set of calls to the fake breaks of the hand-off rules, as one line each: a
/// hand-off by anything but `thread/queue/add` ([MCPB-CDX-003], [MCPB-CDX-004]), any other
/// `turn/*` call, a number of adds other than `adds` ([IFC-ADP-057]: one per delivery, none
/// repeated after a refusal), an add with other members than the three ([MCPB-CDX-005]), and
/// whatever the fake itself flagged (steering, extra members, malformed frames, calls of its
/// own control methods). `report` is the fake's `oacFake/calls` result for one client.
pub fn queue_only_violations(report: &Json, adds: usize) -> Vec<String> {
    let mut v = Vec::new();
    let mut seen_adds = 0;
    for c in array(at(report, "calls")) {
        let method = at(c, "method").as_str().unwrap_or_default();
        match method {
            "thread/queue/add" => {
                seen_adds += 1;
                let mut members: Vec<&str> = at(c, "params")
                    .as_object()
                    .map(|o| o.iter().map(|(k, _)| k).collect())
                    .unwrap_or_default();
                members.sort_unstable();
                if members != ADD_MEMBERS {
                    v.push(format!("thread/queue/add with members {members:?}"));
                }
            }
            m if m.starts_with("turn/") => v.push(format!("{m} was sent")),
            _ => {}
        }
    }
    if seen_adds != adds {
        v.push(format!("{seen_adds} adds, expected {adds}"));
    }
    for (flag, what) in [
        ("steering", "flagged as steering"),
        (
            "overrideMembers",
            "flagged for members beyond the hand-off's own",
        ),
        ("malformed", "malformed"),
        ("control", "oacFake control calls"),
    ] {
        if array_len(at(report, flag)) > 0 {
            v.push(format!(
                "the fake {what}: {}",
                at(report, flag).to_compact()
            ));
        }
    }
    v
}

// The fake Codex app-server (F9, tests/fakes/codex-app-server/), spawned with node.

pub type Waiters = Arc<Mutex<HashMap<u64, Sender<Result<Json, Json>>>>>;

/// The fake, spawned, with its stdio connection for control calls and the "TUI user" (a
/// second client of the same app-server, as Codex's TUI is of its daemon).
pub struct Fake {
    pub child: Child,
    pub stdin: Mutex<ChildStdin>,
    pub url: String,
    pub waiters: Waiters,
    pub next: AtomicU64,
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Fake {
    pub fn spawn() -> Fake {
        let server = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fakes/codex-app-server/server.mjs");
        let mut child = Command::new("node")
            .arg(server)
            .args(["--stdio", "--listen", "ws://127.0.0.1:0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn the fake Codex app-server with node");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (url_tx, url_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut sent = false;
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if !sent
                    && let Ok(v) = json::parse(line.as_bytes())
                    && let Some(u) = at(&v, "url").as_str()
                {
                    let _ = url_tx.send(u.to_owned());
                    sent = true;
                    continue;
                }
                eprintln!("fake codex: {line}");
            }
        });
        let waiters: Waiters = Arc::default();
        let w = waiters.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Ok(v) = json::parse(line.as_bytes()) else {
                    continue;
                };
                let Some(id) = at(&v, "id")
                    .as_number()
                    .and_then(|n| n.plain_integer_in(0, u64::MAX))
                else {
                    continue;
                };
                let reply = match (at(&v, "result"), at(&v, "error")) {
                    (Json::Null, Json::Null) => continue,
                    (Json::Null, e) => Err(e.clone()),
                    (r, _) => Ok(r.clone()),
                };
                if let Some(tx) = lock(&w).remove(&id) {
                    let _ = tx.send(reply);
                }
            }
        });
        let url = url_rx
            .recv_timeout(STARTUP)
            .expect("the fake reports its listener");
        let fake = Fake {
            child,
            stdin: Mutex::new(stdin),
            url,
            waiters,
            next: AtomicU64::new(1),
        };
        fake.call(
            "initialize",
            &format!(
                "{{\"clientInfo\":{{\"name\":{},\"version\":\"0\"}},\"capabilities\":{{\"experimentalApi\":true}}}}",
                js(TUI)
            ),
        )
        .expect("initialize the TUI connection");
        fake
    }

    /// A call as the TUI user (the stdio connection).
    pub fn call(&self, method: &str, params: &str) -> Result<Json, Json> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        lock(&self.waiters).insert(id, tx);
        let frame = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":{},\"params\":{params}}}\n",
            js(method)
        );
        lock(&self.stdin).write_all(frame.as_bytes()).unwrap();
        rx.recv_timeout(WAIT).expect("an answer from the fake")
    }

    pub fn control(&self, method: &str, params: &str) -> Json {
        self.call(method, params)
            .unwrap_or_else(|e| panic!("{method}: {}", e.to_compact()))
    }

    pub fn create_thread(&self) -> String {
        let r = self.control("oacFake/thread/create", "{\"cwd\":\"/oac-g7\"}");
        at(&r, "threadId").as_str().unwrap().to_owned()
    }

    /// The TUI user's `turn/start`: the harness's own user typing.
    pub fn tui_turn(&self, thread: &str, text: &str) {
        self.call(
            "turn/start",
            &format!(
                "{{\"threadId\":{},\"input\":[{{\"type\":\"text\",\"text\":{}}}]}}",
                js(thread),
                js(text)
            ),
        )
        .expect("the TUI's turn starts");
    }

    /// A thread whose turn is running, so that an MCP tool call can be made in it.
    pub fn busy_thread(&self) -> String {
        let t = self.create_thread();
        self.tui_turn(&t, "the TUI user's words");
        t
    }

    /// Ends the running turn (`completed` or `interrupted`); returns the fake's answer, whose
    /// `dispatched` is the id of the turn the queue started, or `null`.
    pub fn end_turn(&self, thread: &str, status: &str, agent_text: &str) -> Json {
        self.control(
            "oacFake/turn/complete",
            &format!(
                "{{\"threadId\":{},\"status\":{},\"agentText\":{}}}",
                js(thread),
                js(status),
                js(agent_text)
            ),
        )
    }

    pub fn archive(&self, thread: &str, archived: bool) {
        self.control(
            "oacFake/thread/setArchived",
            &format!("{{\"threadId\":{},\"archived\":{archived}}}", js(thread)),
        );
    }

    /// `oacFake/mcp/start`: the thread's running turn makes a tool call. Returns the item id.
    pub fn mcp_start(&self, thread: &str, server: &str, tool: &str, arguments: &str) -> String {
        let r = self.control(
            "oacFake/mcp/start",
            &format!(
                "{{\"threadId\":{},\"server\":{},\"tool\":{},\"arguments\":{arguments}}}",
                js(thread),
                js(server),
                js(tool)
            ),
        );
        at(&r, "itemId").as_str().unwrap().to_owned()
    }

    /// `oacFake/mcp/complete`: the call returned `text` (a refusal is `failed`).
    pub fn mcp_complete(&self, thread: &str, item: &str, failed: bool, text: &str) {
        self.control(
            "oacFake/mcp/complete",
            &format!(
                "{{\"threadId\":{},\"itemId\":{},\"outcome\":{},\"text\":{}}}",
                js(thread),
                js(item),
                js(if failed { "failed" } else { "completed" }),
                js(text)
            ),
        );
    }

    /// The fake's call log for one client (`clientInfo.name`): `{calls, steering,
    /// overrideMembers, handOffs, malformed, control}`.
    pub fn report(&self, client: &str) -> Json {
        self.control(
            "oacFake/calls",
            &format!("{{\"clientName\":{}}}", js(client)),
        )
    }
}

// A stand-in for the core, as in adapters/codex/tests/adapter.rs.

#[derive(Default)]
pub struct CoreState {
    pub events: Vec<AdapterEvent>,
    /// The value issued to each attachment, until it is revealed.
    pub issued: HashMap<Attachment, String>,
    pub bound: HashMap<Attachment, SessionId>,
    /// The hand-off records the core keeps (`spec/session-channels.md` §8.2.2): one for each
    /// envelope this scene made a hand-off of.
    pub records: Vec<EnvelopeRecord>,
    /// Every send request the adapter passed to the core.
    pub sends: Vec<SendRequest>,
}

/// The core's side of the adapter: it issues a pairing value to a refused request, binds on
/// the reveal and calls `set_binding` from inside the event handler (as the core does), and
/// stops an attachment on `attachment-unconfirmed`.
pub struct Core {
    pub st: Mutex<CoreState>,
    pub changed: Condvar,
    pub adapter: OnceLock<Arc<CodexAdapter>>,
    pub carrier: OnceLock<ConnectionHandle>,
    pub seq: AtomicU64,
}

impl Core {
    pub fn new() -> Arc<Core> {
        Arc::new(Core {
            st: Mutex::default(),
            changed: Condvar::new(),
            adapter: OnceLock::new(),
            carrier: OnceLock::new(),
            seq: AtomicU64::new(1),
        })
    }

    pub fn adapter(&self) -> Arc<CodexAdapter> {
        self.adapter.get().expect("adapter").clone()
    }

    pub fn wait_for(&self, what: &str, pred: impl Fn(&CoreState) -> bool) {
        let end = Instant::now() + WAIT;
        let mut st = lock(&self.st);
        while !pred(&st) {
            let now = Instant::now();
            assert!(now < end, "no {what}; events {:?}", st.events);
            st = self
                .changed
                .wait_timeout(st, end - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    pub fn handler(self: &Arc<Self>) -> oac_core::adapter::AdapterEventHandler {
        let me = self.clone();
        Arc::new(move |e| me.on_event(e))
    }

    pub fn on_event(&self, e: AdapterEvent) {
        lock(&self.st).events.push(e.clone());
        self.changed.notify_all();
        match e {
            AdapterEvent::NativeSignal(s) => {
                let Some(value) = s.revealed.clone() else {
                    return;
                };
                // A reveal counts only on the carrier, and for a value this core issued.
                if self.carrier.get() != s.connection.as_ref() {
                    return;
                }
                let mut st = lock(&self.st);
                let Some(att) = st
                    .issued
                    .iter()
                    .find(|(_, v)| **v == value)
                    .map(|(a, _)| a.clone())
                else {
                    return;
                };
                st.issued.remove(&att);
                let n = self.seq.fetch_add(1, Ordering::SeqCst);
                let session = SessionId::from_random_octets([n as u8; 16]);
                st.bound.insert(att.clone(), session.clone());
                drop(st);
                self.adapter().set_binding(&att, Some(session));
                self.changed.notify_all();
            }
            AdapterEvent::AttachmentUnconfirmed { attachment } => {
                let was = lock(&self.st).bound.remove(&attachment);
                if was.is_some() {
                    self.adapter().set_binding(&attachment, None);
                }
            }
            AdapterEvent::AttachmentClosed { attachment } => {
                lock(&self.st).bound.remove(&attachment);
                self.adapter().set_binding(&attachment, None);
            }
            _ => {}
        }
    }

    pub fn refusal(&self, a: &Attachment) -> SendRequestResult {
        let mut st = lock(&self.st);
        let value = st
            .issued
            .entry(a.clone())
            .or_insert_with(|| format!("oac-pair-{:032x}", self.seq.fetch_add(1, Ordering::SeqCst)))
            .clone();
        SendRequestResult::Refused {
            error: ErrorCode::Unauthorized,
            pairing_value: Some(value),
        }
    }
}

/// The request sink. An unpaired session's `send` is refused with its pairing value. A paired
/// session's is recorded and sent; a reply (a request with a target) is correlated by the
/// real core's rule, `oac_core::reply::reply_headers` over the hand-off records of this scene
/// ([SC-RCP-050] to [SC-RCP-052]): the target is set only when a record shows that envelope
/// handed off to the replying session, from the session the reply goes to.
pub struct CoreSink(Arc<Core>);

impl RequestSink for CoreSink {
    fn send(&self, request: SendRequest) -> SendRequestResult {
        let core = &self.0;
        let from = {
            let mut st = lock(&core.st);
            st.sends.push(request.clone());
            st.bound.get(&request.attachment).cloned()
        };
        core.changed.notify_all();
        let Some(from) = from else {
            return core.refusal(&request.attachment);
        };
        let correlation = request.requested_target.as_deref().map(|t| {
            let st = lock(&core.st);
            if reply_headers(st.records.iter(), &from, &request.to, Some(t)).correlated() {
                Correlation::Correlated
            } else {
                Correlation::Uncorrelated
            }
        });
        SendRequestResult::Sent {
            id: Token::parse(&format!("m{}", core.seq.fetch_add(1, Ordering::SeqCst))).unwrap(),
            correlation,
            receipts: ReceiptStream(mpsc::channel().1),
        }
    }

    fn discover(&self, request: DiscoveryRequest) -> DiscoveryRequestResult {
        match self.0.refusal(&request.attachment) {
            SendRequestResult::Refused {
                error,
                pairing_value,
            } => DiscoveryRequestResult::Refused {
                error,
                pairing_value,
            },
            _ => DiscoveryRequestResult::DiscoveryResult(Vec::new()),
        }
    }
}

/// A TCP connection to the fake's listener as a core-owned connection: two threads move bytes
/// between the socket and the connection's pipe halves.
pub struct Carrier {
    pub socket: TcpStream,
}

impl Drop for Carrier {
    fn drop(&mut self) {
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
    }
}

pub fn carrier_connection(fake: &Fake) -> (Connection, Carrier) {
    let host = fake
        .url
        .strip_prefix("ws://")
        .unwrap()
        .trim_end_matches('/')
        .to_owned();
    let socket = TcpStream::connect(host).expect("connect to the fake's listener");
    let (mut to_adapter, adapter_reads) = pipe();
    let (adapter_writes, from_adapter) = mpsc::channel::<Vec<u8>>();
    let conn = Connection::accept(adapter_reads, ConnectionWriter::from(adapter_writes));
    let mut read = socket.try_clone().unwrap();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 8192];
        while let Ok(n) = read.read(&mut buf) {
            if n == 0 || to_adapter.write_all(&buf[..n]).is_err() {
                break;
            }
        }
    });
    let mut write = socket.try_clone().unwrap();
    std::thread::spawn(move || {
        for chunk in from_adapter {
            if write.write_all(&chunk).is_err() {
                break;
            }
        }
    });
    (conn, Carrier { socket })
}

/// The MCP client side of one shim connection: Codex's stdio, over a pipe.
pub struct Shim {
    pub to_adapter: Option<ConnectionWriter>,
    pub from_adapter: Receiver<Vec<u8>>,
    pub partial: Vec<u8>,
    pub lines: VecDeque<Json>,
    pub id: u64,
}

pub fn shim_connection() -> (Connection, Shim) {
    let (to_adapter, adapter_reads) = pipe();
    let (adapter_writes, from_adapter) = mpsc::channel::<Vec<u8>>();
    let conn = Connection::accept(adapter_reads, ConnectionWriter::from(adapter_writes));
    (
        conn,
        Shim {
            to_adapter: Some(to_adapter),
            from_adapter,
            partial: Vec::new(),
            lines: VecDeque::new(),
            id: 0,
        },
    )
}

impl Shim {
    pub fn send(&mut self, frame: &str) {
        let w = self.to_adapter.as_mut().expect("open");
        w.write_all(frame.as_bytes()).unwrap();
        w.write_all(b"\n").unwrap();
    }

    pub fn next_frame(&mut self) -> Json {
        let end = Instant::now() + WAIT;
        loop {
            if let Some(v) = self.lines.pop_front() {
                return v;
            }
            let now = Instant::now();
            assert!(now < end, "no frame from the adapter");
            match self.from_adapter.recv_timeout(end - now) {
                Ok(chunk) => {
                    self.partial.extend_from_slice(&chunk);
                    while let Some(i) = self.partial.iter().position(|&b| b == b'\n') {
                        let line: Vec<u8> = self.partial.drain(..=i).collect();
                        let text = String::from_utf8_lossy(&line);
                        if !text.trim().is_empty() {
                            self.lines
                                .push_back(json::parse(text.trim().as_bytes()).expect("JSON"));
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => panic!("no frame from the adapter"),
                Err(RecvTimeoutError::Disconnected) => panic!("the adapter closed the stream"),
            }
        }
    }

    pub fn request(&mut self, method: &str, params: &str) -> Json {
        self.id += 1;
        let id = self.id;
        self.send(&format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":{},\"params\":{params}}}",
            js(method)
        ));
        loop {
            let v = self.next_frame();
            if at(&v, "id").as_number().map(|n| n.raw()) == Some(&id.to_string()) {
                return v;
            }
        }
    }

    /// `initialize`, `notifications/initialized`, `tools/list`: the way Codex opens.
    pub fn open(&mut self) {
        self.request(
            "initialize",
            "{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},\"clientInfo\":{\"name\":\"codex-mcp-client\",\"version\":\"0.163.0\"}}",
        );
        self.send("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}");
        self.request("tools/list", "{}");
    }

    /// A tool call: `(is_error, text)`.
    pub fn call(&mut self, tool: &str, args: &str) -> (bool, String) {
        let v = self.request(
            "tools/call",
            &format!("{{\"name\":{},\"arguments\":{args}}}", js(tool)),
        );
        let r = at(&v, "result");
        assert!(*r != Json::Null, "an error response: {}", v.to_compact());
        (
            at(r, "isError").as_bool() == Some(true),
            at(&array(at(r, "content"))[0], "text")
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        )
    }

    pub fn end(&mut self) {
        self.to_adapter = None;
    }
}

/// One adapter on the fake: the carrier up, a core stand-in behind it.
pub struct Scene {
    pub fake: Fake,
    pub core: Arc<Core>,
    pub adapter: Arc<CodexAdapter>,
    pub _carrier: Carrier,
}

/// A Codex session: a thread with a running turn, and its shim connection.
pub struct Session {
    pub thread: String,
    pub shim: Shim,
    pub handle: Attachment,
}

impl Scene {
    pub fn new() -> Scene {
        let fake = Fake::spawn();
        let mut cfg = Config::new(NAME).without_serving_thread();
        cfg.confirm_wait = CONFIRM_WAIT;
        let adapter = Arc::new(CodexAdapter::with_config(cfg, Options::default()));
        let core = Core::new();
        core.adapter.set(adapter.clone()).ok();
        adapter.watch_attachments(core.handler());
        adapter.accept_requests(Arc::new(CoreSink(core.clone())));
        let (conn, carrier) = carrier_connection(&fake);
        let carrier_handle = conn.handle().clone();
        core.carrier.set(carrier_handle.clone()).ok();
        assert!(adapter.designate_carrier(&carrier_handle));
        adapter.take_connection(conn);
        let end = Instant::now() + WAIT;
        while adapter.serving_counts().is_none() {
            assert!(Instant::now() < end, "the carrier did not come up");
            std::thread::sleep(Duration::from_millis(5));
        }
        Scene {
            fake,
            core,
            adapter,
            _carrier: carrier,
        }
    }

    /// A Codex session: a thread with a running turn, its shim connection opened and listed.
    pub fn session(&self) -> Session {
        let thread = self.fake.busy_thread();
        let (conn, mut shim) = shim_connection();
        let handle = conn.handle().clone();
        self.adapter.take_connection(conn);
        shim.open();
        self.core.wait_for("attachment-opened", |s| {
            s.events.iter().any(|e| {
                matches!(e, AdapterEvent::AttachmentOpened { attachment, .. } if *attachment == handle)
            })
        });
        Session {
            thread,
            shim,
            handle,
        }
    }

    /// The pairing: Codex makes a `send` that the core refuses with a value, the refusal is
    /// revealed on the carrier, and the core binds the session to the thread.
    pub fn pair(&self, s: &mut Session) {
        let args = format!(
            "{{\"to\":\"{}\",\"content\":[{{\"type\":\"text\",\"text\":\"hello\"}}]}}",
            SessionId::from_random_octets([0xee; 16]).as_str()
        );
        let (err, text) = self.codex_call(s, "send", &args);
        assert!(err && text.contains("oac-pair-"), "{text}");
        let h = s.handle.clone();
        self.core
            .wait_for("a binding", |st| st.bound.contains_key(&h));
    }

    /// A verified hand-off of `text` to `s`, from a peer, for the session the core bound.
    pub fn hand_off(&self, s: &Session, text: &str) -> HandOff {
        self.hand_off_signed(s, text).0
    }

    /// [`Scene::hand_off`], and the key id that verified the message. The core's hand-off
    /// record of the envelope is kept with the hand-off made.
    pub fn hand_off_signed(&self, s: &Session, text: &str) -> (HandOff, String) {
        let identity =
            DeviceIdentity::new(DeviceKey::generate(), Token::parse("principal").unwrap());
        let trusted = TrustedKeySet::new(&identity);
        let now = SystemClock.now();
        let session = lock(&self.core.st)
            .bound
            .get(&s.handle)
            .cloned()
            .expect("bound");
        let id = Token::parse(&format!(
            "h{}",
            self.core.seq.fetch_add(1, Ordering::SeqCst)
        ))
        .unwrap();
        let draft = EnvelopeDraft::new(
            id,
            SessionId::from_random_octets([0xee; 16]),
            session,
            now.clone(),
            vec![TextPart::new(text).unwrap()],
        )
        .unwrap();
        let env = identity.sign_envelope(draft);
        let msg = receive_envelope(env.octets(), &EnvelopeLimits::default(), &now).unwrap();
        let msg = authenticate(msg, &trusted).unwrap();
        lock(&self.core.st)
            .records
            .push(EnvelopeRecord::of(msg.envelope()));
        (
            HandOff::new(s.handle.clone(), msg).unwrap(),
            identity.key_id().as_str().to_owned(),
        )
    }

    /// A hand-off of `text` to `s` that completes; returns the envelope's id.
    pub fn deliver(&self, s: &Session, text: &str) -> String {
        let h = self.hand_off(s, text);
        let id = h.message().envelope().id().as_str().to_owned();
        assert_eq!(self.adapter.deliver(h), HandOffOutcome::Completed);
        id
    }

    /// Every `thread/queue/add` the adapter has sent, in order.
    pub fn adds(&self) -> Vec<Json> {
        array(at(&self.report(), "calls"))
            .iter()
            .filter(|c| at(c, "method").as_str() == Some("thread/queue/add"))
            .cloned()
            .collect()
    }

    /// Codex makes `tool` in the session's thread: reported started, called on the shim,
    /// reported completed with what the shim returned. Returns `(is_error, text)`.
    pub fn codex_call(&self, s: &mut Session, tool: &str, args: &str) -> (bool, String) {
        let item = self.fake.mcp_start(&s.thread, NAME, tool, args);
        let (err, text) = s.shim.call(tool, args);
        self.fake.mcp_complete(&s.thread, &item, err, &text);
        (err, text)
    }

    /// How many send requests have reached the core.
    pub fn sends(&self) -> Vec<SendRequest> {
        lock(&self.core.st).sends.clone()
    }

    /// The session the core bound `s` to.
    pub fn session_of(&self, s: &Session) -> SessionId {
        lock(&self.core.st)
            .bound
            .get(&s.handle)
            .cloned()
            .expect("bound")
    }

    /// The fake's log of the adapter's own calls (its `clientInfo.name`).
    pub fn report(&self) -> Json {
        self.fake.report(oac_adapter_codex::client::CLIENT_NAME)
    }

    /// Every call the adapter has made breaks none of the hand-off rules, and exactly `adds`
    /// of them were a `thread/queue/add`.
    pub fn assert_queue_only(&self, adds: usize) {
        let report = self.report();
        let violations = queue_only_violations(&report, adds);
        assert!(
            violations.is_empty(),
            "{violations:?}\n{}",
            report.to_compact()
        );
        assert_eq!(
            array_len(at(&report, "handOffs")),
            adds,
            "one hand-off call per add"
        );
    }

    pub fn state(&self, s: &Session) -> Json {
        self.fake.control(
            "oacFake/thread/state",
            &format!("{{\"threadId\":{}}}", js(&s.thread)),
        )
    }

    /// The id of the turn running on the session's thread.
    pub fn running_turn(&self, s: &Session) -> String {
        at(&self.state(s), "activeTurnId")
            .as_str()
            .expect("a running turn")
            .to_owned()
    }
}
