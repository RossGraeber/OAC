// SPDX-License-Identifier: Apache-2.0

//! The Codex MCP server and its issued-value pairing (`spec/bindings/mcp.md` §4.5; G8, #69,
//! #403), against the real fake Codex app-server process (F9, `tests/fakes/codex-app-server/`,
//! with its `oacFake/mcp/*` frames recorded from the S1 capture) and a stand-in for the core.
//!
//! The stand-in plays the core's side of the [`CallPort`] seam with the core's own
//! [`PairingWindows`] (issue, window, single use, conflict, the cap of three windows), on a
//! clock the test moves. That makes the windows decidable without waiting: only a call's wait
//! for its confirmation takes real time, and every such wait is bounded (400 ms here; 5 s in
//! the reference settings). CI-default tier: no Codex install, no credential, no network
//! beyond loopback.

mod support;

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use oac_adapter_codex::client::{CodexClient, Options};
use oac_adapter_codex::mcp::{ServerHandle, serve_connection};
use oac_adapter_codex::pairing::{
    Answer, CallPort, CodexPairing, Config, LogFn, MAX_REGISTERED_NAME, RegisteredNameError,
    Reveal, validate_registered_name,
};
use oac_adapter_codex::serving::Clock;
use oac_core::delivery::ErrorCode;
use oac_core::ids::EXTENSION_ID_V0;
use oac_core::issued_pairing::{PairingWindows, RevealOutcome};
use oac_core::json::{self, Json};
use oac_mcp_tools::{ToolCall, discovery_result, send_result};
use support::{Fake, get, get_str, js};

const NAME: &str = "oac-test";
const WINDOW: Duration = Duration::from_secs(30);
const CONFIRM_WAIT: Duration = Duration::from_millis(400);
const BOUND: Duration = Duration::from_secs(15);
/// What the stand-in core returns for a served call: the core's own empty discovery result.
const SERVED: &str = "{\"sessions\":[]}";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ---- a clock the test moves ---------------------------------------------------------------

struct TestClock {
    base: Instant,
    offset_ms: AtomicU64,
}

impl TestClock {
    fn new() -> Arc<TestClock> {
        Arc::new(TestClock {
            base: Instant::now(),
            offset_ms: AtomicU64::new(0),
        })
    }

    fn now(&self) -> Instant {
        self.base + Duration::from_millis(self.offset_ms.load(Ordering::SeqCst))
    }

    fn advance(&self, d: Duration) {
        self.offset_ms
            .fetch_add(d.as_millis() as u64, Ordering::SeqCst);
    }

    fn clock(self: &Arc<Self>) -> Clock {
        let me = self.clone();
        Arc::new(move || me.now())
    }
}

// ---- a stand-in for the core ---------------------------------------------------------------

#[derive(Default)]
struct Core {
    windows: Option<PairingWindows<u8>>,
    bound: HashMap<u8, String>,
    stopped: HashSet<u8>,
    /// What the core did with each reveal, in order.
    outcomes: Vec<&'static str>,
    findings: Vec<&'static str>,
    revealed: Vec<(u8, String)>,
    /// A connection the core cannot pair ([MCPB-ATT-022]).
    unpairable: bool,
}

struct TestPort {
    clock: Arc<TestClock>,
    core: Mutex<Core>,
    changed: Condvar,
    /// "listed", "request", "unconfirmed", "reveal", "ended", in order.
    log: Mutex<Vec<&'static str>>,
    pairing: OnceLock<CodexPairing<u8>>,
    requests: AtomicU64,
    /// Bind in the core but do not call `set_bound` yet: the window between the two.
    hold_set_bound: AtomicBool,
}

impl TestPort {
    fn new(clock: &Arc<TestClock>) -> Arc<TestPort> {
        Arc::new(TestPort {
            clock: clock.clone(),
            core: Mutex::new(Core {
                windows: Some(PairingWindows::new(WINDOW)),
                ..Core::default()
            }),
            changed: Condvar::new(),
            log: Mutex::default(),
            pairing: OnceLock::new(),
            requests: AtomicU64::new(0),
            hold_set_bound: AtomicBool::new(false),
        })
    }

    fn log(&self) -> Vec<&'static str> {
        lock(&self.log).clone()
    }

    fn wait_for(&self, what: &str, pred: impl Fn(&Core) -> bool) {
        let end = Instant::now() + BOUND;
        let mut c = lock(&self.core);
        while !pred(&c) {
            let now = Instant::now();
            assert!(now < end, "no {what}");
            c = self
                .changed
                .wait_timeout(c, end - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    fn is_bound(&self, key: u8) -> bool {
        lock(&self.core).bound.contains_key(&key)
    }
}

impl CallPort<u8> for TestPort {
    fn request(&self, key: &u8, _call: ToolCall) -> Answer {
        lock(&self.log).push("request");
        self.requests.fetch_add(1, Ordering::SeqCst);
        let mut core = lock(&self.core);
        if core.bound.contains_key(key) && !core.stopped.contains(key) {
            return Answer {
                result: discovery_result(
                    &oac_core::adapter::DiscoveryRequestResult::DiscoveryResult(Vec::new()),
                ),
                pairing_value: None,
                unauthorized: false,
            };
        }
        let value = if core.unpairable {
            None
        } else {
            let now = self.clock.now();
            core.windows.as_mut().unwrap().issue(key, now)
        };
        Answer {
            result: send_result(&oac_core::adapter::SendRequestResult::Refused {
                error: ErrorCode::Unauthorized,
                pairing_value: value.clone(),
            }),
            pairing_value: value,
            unauthorized: true,
        }
    }

    fn unconfirmed(&self, key: &u8) {
        lock(&self.log).push("unconfirmed");
        let mut core = lock(&self.core);
        core.stopped.insert(*key);
        core.findings.push("unconfirmed");
        self.changed.notify_all();
    }

    fn reveal(&self, key: &u8, reveal: Reveal) {
        lock(&self.log).push("reveal");
        let now = self.clock.now();
        let mut core = lock(&self.core);
        core.revealed.push((*key, reveal.native_id().to_owned()));
        let outcome =
            core.windows
                .as_mut()
                .unwrap()
                .reveal(reveal.value(), reveal.native_id(), now, |_| true);
        let mut set_bound = None;
        match outcome {
            RevealOutcome::Fresh(a) => {
                core.windows.as_mut().unwrap().paired(&a);
                core.outcomes.push("fresh");
                match core.bound.get(&a) {
                    // Bound to the same thread already: unchanged, and the stop is lifted.
                    Some(n) if *n == reveal.native_id() => {}
                    _ => {
                        core.bound.insert(a, reveal.native_id().to_owned());
                        set_bound = Some((a, true));
                    }
                }
                core.stopped.remove(&a);
            }
            RevealOutcome::Conflict(a) => {
                core.outcomes.push("conflict");
                core.findings.push("conflict");
                core.bound.remove(&a);
                core.stopped.remove(&a);
                set_bound = Some((a, false));
            }
            RevealOutcome::Ended(_) => core.outcomes.push("ended"),
            RevealOutcome::Replayed(_) => core.outcomes.push("replayed"),
            RevealOutcome::Unknown => core.outcomes.push("unknown"),
            RevealOutcome::Refused(_) => core.outcomes.push("refused"),
        }
        drop(core);
        if let Some((a, bound)) = set_bound
            && !(bound && self.hold_set_bound.load(Ordering::SeqCst))
        {
            self.pairing.get().unwrap().set_bound(&a, bound);
        }
        self.changed.notify_all();
    }

    fn listed(&self, _key: &u8) {
        lock(&self.log).push("listed");
    }

    fn ended(&self, _key: &u8) {
        lock(&self.log).push("ended");
    }
}

// ---- an MCP client, as Codex ----------------------------------------------------------------

struct Mcp {
    w: TcpStream,
    r: BufReader<TcpStream>,
    id: u64,
    /// Anything the server sent that was not a response: there must be nothing.
    unsolicited: Vec<Json>,
    server: Option<ServerHandle>,
}

impl Mcp {
    fn connect(pairing: &CodexPairing<u8>, key: u8) -> Mcp {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (server, _) = l.accept().unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let reader = server.try_clone().unwrap();
        let handle = serve_connection(key, Box::new(reader), Box::new(server), pairing.clone())
            .expect("serve");
        Mcp {
            r: BufReader::new(client.try_clone().unwrap()),
            w: client,
            id: 0,
            unsolicited: Vec::new(),
            server: Some(handle),
        }
    }

    fn send(&mut self, frame: &str) {
        self.w.write_all(frame.as_bytes()).unwrap();
        self.w.write_all(b"\n").unwrap();
    }

    /// The next response; anything else the server sends is kept in `unsolicited`.
    fn recv(&mut self) -> Json {
        loop {
            let mut line = String::new();
            let n = self.r.read_line(&mut line).expect("a line from the server");
            assert!(n > 0, "the server closed the connection");
            let v = json::parse(line.trim().as_bytes()).expect("JSON");
            if get(&v, "id").is_some()
                && (get(&v, "result").is_some() || get(&v, "error").is_some())
            {
                return v;
            }
            self.unsolicited.push(v);
        }
    }

    fn request(&mut self, method: &str, params: &str) -> Json {
        self.id += 1;
        let id = self.id;
        self.send(&format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":{},\"params\":{params}}}",
            js(method)
        ));
        let v = self.recv();
        assert_eq!(
            get(&v, "id").and_then(Json::as_number).map(|n| n.raw()),
            Some(id.to_string().as_str())
        );
        v
    }

    /// Open the way Codex 0.163.0 did (S1 server log): `initialize` at `2025-06-18`,
    /// `notifications/initialized`, `tools/list`. Returns the initialize result and the tools.
    fn open(&mut self) -> (Json, Json) {
        let init = self.request(
            "initialize",
            "{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{\"elicitation\":{\"form\":{},\"url\":{}}},\"clientInfo\":{\"name\":\"codex-mcp-client\",\"title\":\"Codex\",\"version\":\"0.163.0\"}}",
        );
        self.send("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}");
        let tools = self.request(
            "tools/list",
            "{\"_meta\":{\"traceparent\":\"00-7c5a69999862913dc2c8d6f1b0507161-4b8ad3c3cb21f1c7-01\",\"progressToken\":0}}",
        );
        (init, tools)
    }

    /// A `tools/call` with Codex's `_meta` members (S1 capture, C1). `meta_thread` is the
    /// value of the self-asserted `threadId`, `sessionId` and `windowId`: never read.
    fn call_frame(&mut self, name: &str, arguments: Option<&str>, meta_thread: &str) -> u64 {
        self.id += 1;
        let id = self.id;
        let args = arguments.map_or(String::new(), |a| format!(",\"arguments\":{a}"));
        self.send(&format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"tools/call\",\"params\":{{\"_meta\":{{\"callId\":\"exec-0\",\"threadId\":{t},\"sessionId\":{t},\"windowId\":{t},\"itemId\":\"ctc_00dd3f42209e6649016ad9890c1b4c8190a465e05cb1347ed2\",\"traceparent\":\"00-483ae8cb972ff59b71ef73f8e12b290d-84a93edd8e7238cc-01\",\"progressToken\":3}},\"name\":{n}{args}}}}}",
            t = js(meta_thread),
            n = js(name)
        ));
        id
    }

    /// Call a tool and wait for the result (the `result` object of the response).
    fn call(&mut self, name: &str, arguments: Option<&str>, meta_thread: &str) -> Json {
        self.call_frame(name, arguments, meta_thread);
        let v = self.recv();
        get(&v, "result")
            .cloned()
            .unwrap_or_else(|| panic!("an error response: {}", v.to_compact()))
    }

    fn stop(mut self) {
        if let Some(s) = self.server.take() {
            s.stop();
            s.join();
        }
    }
}

fn result_text(r: &Json) -> String {
    get(r, "content")
        .and_then(Json::as_array)
        .and_then(|c| c.first())
        .and_then(|b| get_str(b, "text"))
        .unwrap_or_default()
        .to_owned()
}

fn is_error(r: &Json) -> bool {
    get(r, "isError").and_then(Json::as_bool) == Some(true)
}

fn pairing_value_of(text: &str) -> Option<String> {
    let i = text.find("oac-pair-")?;
    Some(text[i..i + "oac-pair-".len() + 32].to_owned())
}

/// The diagnostic lines the pairing wrote.
type Lines = Arc<Mutex<Vec<String>>>;

fn collecting(lines: &Lines) -> LogFn {
    let lines = lines.clone();
    Arc::new(move |l: &str| lock(&lines).push(l.to_owned()))
}

// ---- the scene -----------------------------------------------------------------------------

struct Scene {
    fake: Fake,
    clock: Arc<TestClock>,
    port: Arc<TestPort>,
    pairing: CodexPairing<u8>,
}

impl Scene {
    fn new() -> Scene {
        Scene::with(|c| c)
    }

    fn with(tune: impl FnOnce(Config) -> Config) -> Scene {
        let fake = Fake::spawn();
        let clock = TestClock::new();
        let port = TestPort::new(&clock);
        // A clock the test moves: no thread of the pairing's own reads it, so windows end when
        // the test sweeps them.
        let mut cfg = Config::new(NAME)
            .with_clock(clock.clock())
            .without_serving_thread();
        cfg.window = WINDOW;
        cfg.confirm_wait = CONFIRM_WAIT;
        let cfg = tune(cfg);
        let host = fake
            .url
            .strip_prefix("ws://")
            .unwrap()
            .trim_end_matches('/')
            .to_owned();
        let pairing = CodexPairing::start(cfg, port.clone(), |h| {
            let s = TcpStream::connect(&host).expect("connect to the fake's listener");
            let r = s.try_clone().unwrap();
            CodexClient::connect(r, s, &Options::default(), h)
        })
        .expect("start the pairing");
        port.pairing.set(pairing.clone()).ok();
        Scene {
            fake,
            clock,
            port,
            pairing,
        }
    }

    /// All earlier notifications have been dispatched: the carrier is one ordered stream, and
    /// a call's answer comes after everything sent before it.
    fn barrier(&self) {
        self.pairing.client().loaded_threads().expect("barrier");
    }

    /// Codex reports a call of the registered server, started and completed, in a thread's
    /// running turn (`oacFake/mcp/start`, `oacFake/mcp/complete`).
    fn report(&self, thread: &str, tool: &str, args: &str, failed: bool, text: &str) {
        self.report_as(NAME, thread, tool, args, failed, text);
    }

    /// [`Scene::report`] with the `server` Codex names for the call.
    fn report_as(
        &self,
        server: &str,
        thread: &str,
        tool: &str,
        args: &str,
        failed: bool,
        text: &str,
    ) {
        let item = self.fake.mcp_start(thread, server, tool, args);
        self.fake.mcp_complete(thread, &item, failed, text);
        self.barrier();
    }

    /// The scene of most tests: a busy thread, an open connection 1, and its first call
    /// refused with a pairing value. Returns the thread, the connection and the refusal text.
    fn refused_whoami(&self, thread: &str, args: &str) -> (Mcp, String) {
        let mut mcp = Mcp::connect(&self.pairing, 1);
        mcp.open();
        let r = mcp.call("whoami", Some(args), thread);
        assert!(is_error(&r));
        let text = result_text(&r);
        assert!(pairing_value_of(&text).is_some(), "{text}");
        (mcp, text)
    }

    /// Pair connection 1 with `thread`: refuse, report, and wait for the binding. Returns the
    /// connection and the refusal text that was revealed.
    fn pair_with_text(&self, thread: &str) -> (Mcp, String) {
        let (mcp, text) = self.refused_whoami(thread, "{}");
        self.report(thread, "whoami", "{}", true, &text);
        self.port
            .wait_for("the binding", |c| c.bound.contains_key(&1));
        (mcp, text)
    }

    fn pair(&self, thread: &str) -> Mcp {
        self.pair_with_text(thread).0
    }

    fn forbidden_calls_absent(&self) {
        let calls = self.fake.client_calls();
        for c in &calls {
            let m = get_str(c, "method").unwrap_or_default();
            assert!(
                ![
                    "thread/list",
                    "turn/start",
                    "turn/steer",
                    "thread/queue/add"
                ]
                .contains(&m),
                "{m}"
            );
        }
    }
}

// ---- the server ----------------------------------------------------------------------------

#[test]
fn tools_list_equals_the_shared_definition_and_declares_no_channel() {
    let s = Scene::new();
    let mut mcp = Mcp::connect(&s.pairing, 1);
    // A client that probes first falls back to `initialize` ([MCPB-CLD-003]).
    let probe = mcp.request(
        "server/discover",
        "{\"_meta\":{\"io.modelcontextprotocol/protocolVersion\":\"2026-07-28\",\"io.modelcontextprotocol/clientInfo\":{\"name\":\"codex\",\"version\":\"0\"},\"io.modelcontextprotocol/clientCapabilities\":{}}}",
    );
    assert_eq!(
        get(&probe, "error")
            .and_then(|e| get(e, "code"))
            .map(|c| c.to_compact()),
        Some("-32601".to_owned())
    );
    let (init, tools) = mcp.open();
    let result = get(&init, "result").unwrap();
    let caps = get(result, "capabilities").unwrap();
    // Tools and the extension identifier; no channel capability, nothing pollable.
    let names: Vec<&str> = caps.as_object().unwrap().names().collect();
    for n in &names {
        assert!(["tools", "extensions"].contains(n), "capability {n}");
    }
    assert!(get(caps, "experimental").is_none());
    assert!(
        get(caps, "extensions")
            .and_then(|e| get(e, EXTENSION_ID_V0))
            .is_some()
    );
    assert_eq!(
        get(result, "protocolVersion").and_then(Json::as_str),
        Some("2025-06-18"),
        "the legacy revision the client asked for"
    );
    // [MCPB-TOOL-003]: the same four tools, descriptions and schemas as the shared definition
    // the Claude adapter serves.
    let listed = get(get(&tools, "result").unwrap(), "tools")
        .and_then(Json::as_array)
        .unwrap();
    let expected = oac_mcp_tools::tools();
    assert_eq!(listed.len(), 4);
    for (got, want) in listed.iter().zip(&expected) {
        assert_eq!(get_str(got, "name"), Some(want.name.as_ref()));
        assert_eq!(
            get_str(got, "description"),
            want.description.as_deref(),
            "{}",
            want.name
        );
        let schema = json::parse(
            oac_mcp_tools::Value::from((*want.input_schema).clone())
                .to_string()
                .as_bytes(),
        )
        .unwrap();
        assert!(
            oac_adapter_codex::confirm::json_equal(get(got, "inputSchema").unwrap(), &schema),
            "{}",
            want.name
        );
    }
    assert_eq!(s.port.log(), ["listed"]);
    // Nothing but responses ever went to Codex ([MCPB-CDX-001]).
    assert!(mcp.unsolicited.is_empty());
    mcp.stop();
    assert_eq!(s.port.log(), ["listed", "ended"]);
}

// ---- reveal --------------------------------------------------------------------------------

#[test]
fn a_refused_call_is_revealed_by_codexs_report_and_pairs_the_connection() {
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let (mut mcp, text) = s.refused_whoami(&thread, "{}");
    // Before the refusal went out, the loaded threads were listed (ids only) and the busy
    // thread subscribed with `thread/resume` carrying only `threadId` and `excludeTurns`
    // ([MCPB-CDX-006]); `thread/list` was never called.
    let methods: Vec<String> = s
        .fake
        .client_calls()
        .iter()
        .map(|c| get_str(c, "method").unwrap_or_default().to_owned())
        .collect();
    let list = methods
        .iter()
        .position(|m| m == "thread/loaded/list")
        .expect("loaded list");
    let resume = methods
        .iter()
        .position(|m| m == "thread/resume")
        .expect("resume");
    assert!(list < resume);
    s.forbidden_calls_absent();
    // The refusal is unauthorized, with the value in a text block ([MCPB-ATT-004]).
    assert!(text.starts_with("unauthorized: "), "{text}");
    assert!(!s.port.is_bound(1));
    assert_eq!(s.port.log(), ["listed", "request"]);

    // Codex reports the refused call complete, with the refusal as its result (S1 L125): the
    // reveal. `{}` is what Codex reported for a call made with no arguments (S1 L120).
    s.report(&thread, "whoami", "{}", true, &text);
    s.port.wait_for("the binding", |c| c.bound.contains_key(&1));
    {
        let core = lock(&s.port.core);
        // The thread id is the notification's `threadId`, never the call's `_meta` ([MCPB-ATT-008]).
        assert_eq!(core.revealed, [(1, thread.clone())]);
        assert_eq!(core.outcomes, ["fresh"]);
        assert_eq!(core.bound.get(&1), Some(&thread));
    }
    assert_eq!(s.pairing.diagnostics().reveals, 1);
    // The thread stays subscribed while the connection holds it, whatever the window.
    s.clock.advance(WINDOW * 3);
    s.pairing.sweep();
    assert_eq!(s.pairing.serving_counts(), (1, 0));
    // Nothing the server sent was a notification ([MCPB-CDX-001]).
    mcp.call("whoami", None, "x");
    assert!(mcp.unsolicited.is_empty());
    // The connection ends: it holds the thread no longer, and the thread is unsubscribed.
    mcp.stop();
    let end = Instant::now() + BOUND;
    while s.pairing.serving_counts() != (0, 1) {
        assert!(Instant::now() < end, "{:?}", s.pairing.serving_counts());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(s.fake.client_calls_of("thread/unsubscribe").len(), 1);
}

#[test]
fn the_specs_bounds_are_not_settings() {
    // [MCPB-ATT-019], [MCPB-ATT-020], [MCPB-ATT-021]: a longer setting is cut to the bound.
    let s = Scene::with(|mut c| {
        c.window = Duration::from_secs(3600);
        c.confirm_wait = Duration::from_secs(3600);
        c.confirm_life = Duration::from_secs(3600);
        c
    });
    let cfg = s.pairing.config();
    assert_eq!(cfg.window, Duration::from_secs(60));
    assert_eq!(cfg.confirm_wait, Duration::from_secs(10));
    assert_eq!(cfg.confirm_life, Duration::from_secs(600));
}

#[test]
fn a_call_made_with_no_arguments_matches_the_empty_object_codex_reported() {
    // S1 capture L120, L125: the model's call "with no arguments" reached the server as
    // `arguments: {}` and Codex reported `{}`. It is an equal match.
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let (_mcp, text) = s.refused_whoami(&thread, "{}");
    s.report(&thread, "whoami", "{}", true, &text);
    s.port.wait_for("the binding", |c| c.bound.contains_key(&1));
}

#[test]
fn a_call_with_no_arguments_member_does_not_match_a_reported_empty_object() {
    // Spec §4.5.2: `{}` matches only `{}`, and `null` or absent matches absent. A call sent
    // with no `arguments` member and reported as `{}` is therefore not a reveal (fail
    // closed; availability only). Recorded as a finding for S4's live legs.
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let mut mcp = Mcp::connect(&s.pairing, 1);
    mcp.open();
    let r = mcp.call("whoami", None, &thread);
    let text = result_text(&r);
    assert!(pairing_value_of(&text).is_some());
    s.report(&thread, "whoami", "{}", true, &text);
    assert_eq!(s.pairing.diagnostics().reveals, 0);
    assert!(!s.port.is_bound(1));
    // The same refusal reported as it was called is a reveal (absent matches absent: the
    // fake can only report objects, so the unit tests of the matcher cover `null`).
}

#[test]
fn a_report_that_is_not_the_refusal_reveals_nothing() {
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let (_mcp, text) = s.refused_whoami(&thread, "{}");
    let other = text.replace("oac-pair-", "oac-pair-0");
    // A different value, a different tool, a different server, different arguments.
    s.report(&thread, "whoami", "{}", true, &other);
    s.report(&thread, "list_sessions", "{}", true, &text);
    s.report(&thread, "whoami", "{\"x\":1}", true, &text);
    let item = s.fake.mcp_start(&thread, "someone-else", "whoami", "{}");
    s.fake.mcp_complete(&thread, &item, true, &text);
    s.barrier();
    assert_eq!(s.pairing.diagnostics().reveals, 0);
    assert!(!s.port.is_bound(1));
    // The real report still works: nothing above used the record up.
    s.report(&thread, "whoami", "{}", true, &text);
    s.port.wait_for("the binding", |c| c.bound.contains_key(&1));
}

#[test]
fn a_late_reveal_is_not_reported() {
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let (_mcp, text) = s.refused_whoami(&thread, "{}");
    // Codex's report arrives after the window: dropped here with a count ([MCPB-ATT-009]).
    s.clock.advance(WINDOW + Duration::from_secs(1));
    s.report(&thread, "whoami", "{}", true, &text);
    assert_eq!(s.pairing.diagnostics().late_reveals, 1);
    assert_eq!(s.pairing.diagnostics().reveals, 0);
    assert!(lock(&s.port.core).revealed.is_empty());
    assert!(!s.port.is_bound(1));
}

#[test]
fn a_reveal_after_the_cores_shorter_window_is_the_cores_to_refuse() {
    // The adapter's records live as long as the window; the core's check is the second guard.
    // A stand-in whose window is shorter than the adapter's shows the core refusing a
    // reveal the adapter passed on ([MCPB-ATT-009]).
    let s = Scene::new();
    lock(&s.port.core).windows = Some(PairingWindows::new(Duration::from_secs(5)));
    let thread = s.fake.busy_thread();
    let (_mcp, text) = s.refused_whoami(&thread, "{}");
    s.clock.advance(Duration::from_secs(10));
    s.report(&thread, "whoami", "{}", true, &text);
    s.port
        .wait_for("the core's answer", |c| !c.outcomes.is_empty());
    assert_eq!(lock(&s.port.core).outcomes, ["ended"]);
    assert!(!s.port.is_bound(1));
}

#[test]
fn a_replayed_report_pairs_nothing_twice() {
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let (mcp, text) = s.pair_with_text(&thread);
    assert_eq!(s.pairing.diagnostics().reveals, 1);
    // The same report again, as a second item with the same result: the record of the
    // refused call was used up by the first, so nothing more is reported and the core is not
    // asked ([MCPB-ATT-010]).
    s.report(&thread, "whoami", "{}", true, &text);
    s.report(&thread, "whoami", "{}", true, &text);
    assert_eq!(s.pairing.diagnostics().reveals, 1);
    assert_eq!(lock(&s.port.core).outcomes, ["fresh"]);
    assert!(s.port.is_bound(1));
    mcp.stop();
}

#[test]
fn a_reveal_on_another_thread_ends_the_binding_and_no_meta_id_is_read() {
    let s = Scene::new();
    let t1 = s.fake.busy_thread();
    let t2 = s.fake.busy_thread();
    let mut mcp = Mcp::connect(&s.pairing, 1);
    mcp.open();
    // Two calls refused in one window carry one value ([MCPB-ATT-025]). The `_meta` names
    // t2 for both; it is never read ([MCPB-ATT-008]).
    let a = mcp.call("whoami", Some("{}"), &t2);
    let b = mcp.call("whoami", Some("{}"), &t2);
    let (a, b) = (result_text(&a), result_text(&b));
    assert_eq!(a, b);
    // t1 reports the first: the binding is to t1, whatever the `_meta` said.
    s.report(&t1, "whoami", "{}", true, &a);
    s.port.wait_for("the binding", |c| c.bound.contains_key(&1));
    assert_eq!(lock(&s.port.core).bound.get(&1), Some(&t1));
    // t2 reports the same value ([MCPB-ATT-011]): the binding ends, with a finding
    // ([MCPB-ATT-012]).
    s.report(&t2, "whoami", "{}", true, &b);
    s.port
        .wait_for("the conflict", |c| c.outcomes.contains(&"conflict"));
    {
        let core = lock(&s.port.core);
        assert!(!core.bound.contains_key(&1));
        assert_eq!(core.findings, ["conflict"]);
    }
    // Unbound again: the next call is refused with a pairing value, not served.
    let r = mcp.call("whoami", Some("{}"), &t1);
    assert!(is_error(&r));
    assert!(pairing_value_of(&result_text(&r)).is_some());
    mcp.stop();
}

#[test]
fn a_thread_that_is_no_longer_subscribed_is_neither_a_reveal_nor_a_confirmation() {
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let (_mcp, text) = s.refused_whoami(&thread, "{}");
    assert_eq!(s.pairing.serving_counts().0, 1);
    // The window ends unpaired: the thread is unsubscribed. The fake, like live Codex
    // (row 70), still sends the thread's notifications to the carrier; the client drops them.
    s.clock.advance(WINDOW + Duration::from_secs(1));
    s.pairing.sweep();
    assert_eq!(s.pairing.serving_counts(), (0, 1));
    assert_eq!(s.fake.client_calls_of("thread/unsubscribe").len(), 1);
    let dropped = s.pairing.client().diagnostics().not_served;
    s.report(&thread, "whoami", "{}", true, &text);
    assert!(s.pairing.client().diagnostics().not_served > dropped);
    assert_eq!(s.pairing.diagnostics().reveals, 0);
    assert!(!s.port.is_bound(1));
}

#[test]
fn a_thread_the_carrier_never_subscribed_is_neither_a_reveal_nor_a_confirmation() {
    let s = Scene::new();
    let t1 = s.fake.busy_thread();
    let (_mcp, text) = s.refused_whoami(&t1, "{}");
    // A thread that started after the window opened was never listed, so never subscribed:
    // the fake sends its items to its subscribers only.
    let late = s.fake.busy_thread();
    s.report(&late, "whoami", "{}", true, &text);
    assert_eq!(s.pairing.diagnostics().reveals, 0);
    assert!(!s.port.is_bound(1));
    // Only the subscribed thread's report counts.
    s.report(&t1, "whoami", "{}", true, &text);
    s.port.wait_for("the binding", |c| c.bound.contains_key(&1));
    assert_eq!(lock(&s.port.core).bound.get(&1), Some(&t1));
}

// ---- windows -------------------------------------------------------------------------------

#[test]
fn at_most_three_windows_open_on_one_connection() {
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let mut mcp = Mcp::connect(&s.pairing, 1);
    mcp.open();
    let mut values = Vec::new();
    for window in 1..=3 {
        let r = mcp.call("whoami", Some("{}"), &thread);
        let v = pairing_value_of(&result_text(&r)).unwrap_or_else(|| panic!("window {window}"));
        values.push(v);
        // The window ends with no reveal: the thread is unsubscribed.
        s.clock.advance(WINDOW + Duration::from_secs(1));
        s.pairing.sweep();
    }
    values.sort();
    values.dedup();
    assert_eq!(
        values.len(),
        3,
        "a new value for each window [MCPB-ATT-026]"
    );
    // The fourth refusal carries no value and does not invite a retry ([MCPB-ATT-023],
    // [MCPB-ATT-024]).
    let r = mcp.call("whoami", Some("{}"), &thread);
    let text = result_text(&r);
    assert!(is_error(&r));
    assert!(text.starts_with("unauthorized: "), "{text}");
    assert!(pairing_value_of(&text).is_none(), "{text}");
    assert!(
        !text.contains("again") && !text.contains("once more"),
        "{text}"
    );
    // Each window subscribed the thread, and each end unsubscribed it.
    assert_eq!(s.fake.client_calls_of("thread/resume").len(), 4);
    assert_eq!(s.fake.client_calls_of("thread/unsubscribe").len(), 3);
    assert_eq!(s.pairing.diagnostics().windows, 4);
    s.forbidden_calls_absent();
    mcp.stop();
}

#[test]
fn one_window_lists_the_threads_once_however_many_calls_it_refuses() {
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let mut mcp = Mcp::connect(&s.pairing, 1);
    mcp.open();
    let mut texts = Vec::new();
    for _ in 0..3 {
        texts.push(result_text(&mcp.call("whoami", Some("{}"), &thread)));
    }
    texts.dedup();
    assert_eq!(texts.len(), 1, "one value per window");
    assert_eq!(s.fake.client_calls_of("thread/loaded/list").len(), 1);
    assert_eq!(s.fake.client_calls_of("thread/resume").len(), 1);
    mcp.stop();
}

#[test]
fn the_window_end_unsubscribes_the_unpaired_threads_by_itself() {
    // Real time and a short window: the serving thread ends it, nobody calls `sweep`.
    let fake = Fake::spawn();
    let clock = TestClock::new();
    let port = TestPort::new(&clock);
    let mut cfg = Config::new(NAME);
    cfg.window = Duration::from_millis(300);
    let host = fake
        .url
        .strip_prefix("ws://")
        .unwrap()
        .trim_end_matches('/')
        .to_owned();
    let pairing = CodexPairing::start(cfg, port.clone(), |h| {
        let s = TcpStream::connect(&host).unwrap();
        let r = s.try_clone().unwrap();
        CodexClient::connect(r, s, &Options::default(), h)
    })
    .unwrap();
    port.pairing.set(pairing.clone()).ok();
    let thread = fake.busy_thread();
    let mut mcp = Mcp::connect(&pairing, 1);
    mcp.open();
    let r = mcp.call("whoami", Some("{}"), &thread);
    assert!(pairing_value_of(&result_text(&r)).is_some());
    let end = Instant::now() + BOUND;
    while pairing.serving_counts().1 == 0 {
        assert!(Instant::now() < end, "the window never ended");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(pairing.serving_counts(), (0, 1));
    assert_eq!(fake.client_calls_of("thread/unsubscribe").len(), 1);
    mcp.stop();
    pairing.shutdown();
}

#[test]
fn nothing_loaded_means_a_plain_refusal_and_no_window() {
    let s = Scene::new();
    let mut mcp = Mcp::connect(&s.pairing, 1);
    mcp.open();
    let before = s.port.requests.load(Ordering::SeqCst);
    let r = mcp.call("whoami", Some("{}"), "x");
    let text = result_text(&r);
    assert!(is_error(&r) && text.starts_with("unauthorized: "));
    assert!(pairing_value_of(&text).is_none());
    // The pairing could not complete, so the core was not asked and opened no window
    // ([MCPB-ATT-022]).
    assert_eq!(s.port.requests.load(Ordering::SeqCst), before);
    assert_eq!(s.pairing.diagnostics().unpairable, 1);
    mcp.stop();
}

#[test]
fn a_connection_the_core_cannot_pair_gets_the_plain_refusal() {
    let s = Scene::new();
    lock(&s.port.core).unpairable = true;
    let thread = s.fake.busy_thread();
    let mut mcp = Mcp::connect(&s.pairing, 1);
    mcp.open();
    let r = mcp.call("whoami", Some("{}"), &thread);
    assert!(is_error(&r));
    assert!(pairing_value_of(&result_text(&r)).is_none());
    mcp.stop();
}

// ---- confirmation ---------------------------------------------------------------------------

#[test]
fn a_paired_call_is_served_on_a_confirmation_that_came_first() {
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let mut mcp = s.pair(&thread);
    // S1 C5: `item/started` came 0.9 to 4.1 s before the server saw the call, with the
    // approval step between. The pairing consumes only item/started, item/completed and
    // turn/completed, so the other traffic of that step is not in its way.
    let item = s.fake.mcp_start(&thread, NAME, "list_sessions", "{}");
    s.barrier();
    let key = s.pairing.client().key_of(&thread).unwrap();
    s.pairing.on_event(oac_adapter_codex::CodexEvent::Status {
        thread: key,
        state: oac_adapter_codex::ThreadState::Active,
    });
    let t0 = Instant::now();
    let r = mcp.call("list_sessions", Some("{}"), &thread);
    assert!(!is_error(&r), "{}", r.to_compact());
    assert_eq!(result_text(&r), SERVED);
    assert!(t0.elapsed() < CONFIRM_WAIT, "served without waiting");
    // The report of the completed call carries the content that was returned ([MCPB-ATT-017]).
    s.fake.mcp_complete(&thread, &item, false, SERVED);
    s.barrier();
    assert_eq!(s.pairing.diagnostics().content_mismatches, 0);
    assert!(!s.port.log().contains(&"unconfirmed"));
    mcp.stop();
}

#[test]
fn a_call_between_the_cores_binding_and_set_bound_still_needs_a_confirmation() {
    // The core binds inside `CallPort::reveal`; `set_bound` arrives later. A call in that
    // window must not reach a bound core unconfirmed ([MCPB-ATT-013]).
    let s = Scene::new();
    s.port.hold_set_bound.store(true, Ordering::SeqCst);
    let thread = s.fake.busy_thread();
    let (mut mcp, text) = s.refused_whoami(&thread, "{}");
    s.report(&thread, "whoami", "{}", true, &text);
    s.port
        .wait_for("the core's binding", |c| c.bound.contains_key(&1));
    // No `set_bound(true)` yet, and no confirmation: refused, not served.
    let r = mcp.call("list_sessions", Some("{}"), &thread);
    assert!(is_error(&r), "served unconfirmed: {}", r.to_compact());
    assert_eq!(s.pairing.diagnostics().unconfirmed, 1);
    assert_eq!(
        s.port.log().iter().filter(|l| **l == "unconfirmed").count(),
        1
    );
    mcp.stop();
}

#[test]
fn a_paired_call_waits_for_a_confirmation_that_comes_after_it() {
    let s = Scene::with(|mut c| {
        c.confirm_wait = Duration::from_secs(10);
        c
    });
    let thread = s.fake.busy_thread();
    let mut mcp = s.pair(&thread);
    mcp.call_frame("list_sessions", Some("{}"), &thread);
    std::thread::sleep(Duration::from_millis(150));
    // The call is waiting; no refusal has gone out.
    assert!(!s.port.log().contains(&"unconfirmed"));
    let item = s.fake.mcp_start(&thread, NAME, "list_sessions", "{}");
    let r = mcp.recv();
    let r = get(&r, "result").unwrap();
    assert_eq!(result_text(r), SERVED);
    s.fake.mcp_complete(&thread, &item, false, SERVED);
    mcp.stop();
}

#[test]
fn a_confirmation_serves_one_call_only() {
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let mut mcp = s.pair(&thread);
    s.fake.mcp_start(&thread, NAME, "list_sessions", "{}");
    s.barrier();
    let first = mcp.call("list_sessions", Some("{}"), &thread);
    assert_eq!(result_text(&first), SERVED);
    // The second identical call has no confirmation of its own: refused after the bounded
    // wait ([MCPB-ATT-014], [MCPB-ATT-015]).
    let t0 = Instant::now();
    let second = mcp.call("list_sessions", Some("{}"), &thread);
    let waited = t0.elapsed();
    assert!(is_error(&second));
    assert!(text_is_pairing_refusal(&second));
    assert!(
        waited >= CONFIRM_WAIT && waited < CONFIRM_WAIT + Duration::from_secs(8),
        "{waited:?}"
    );
    mcp.stop();
}

fn text_is_pairing_refusal(r: &Json) -> bool {
    let t = result_text(r);
    t.starts_with("unauthorized: ") && pairing_value_of(&t).is_some()
}

#[test]
fn a_missing_confirmation_is_reported_before_the_request_and_stops_the_connection() {
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let mut mcp = s.pair(&thread);
    let log_before = s.port.log().len();
    let t0 = Instant::now();
    let r = mcp.call("list_sessions", Some("{}"), &thread);
    let waited = t0.elapsed();
    // Refused, with a new pairing value so the next call can re-pair ([MCPB-ATT-015]); the
    // wait was bounded ([MCPB-ATT-020]).
    assert!(is_error(&r) && text_is_pairing_refusal(&r));
    assert!(
        waited >= CONFIRM_WAIT && waited < CONFIRM_WAIT + Duration::from_secs(8),
        "{waited:?}"
    );
    // [IFC-ADP-091]: `attachment-unconfirmed` came before the request it concerns.
    assert_eq!(s.port.log()[log_before..], ["unconfirmed", "request"]);
    assert_eq!(s.pairing.diagnostics().unconfirmed, 1);
    // [MCPB-ATT-016]: further calls are refused without the wait, even with a confirmation.
    s.fake.mcp_start(&thread, NAME, "list_sessions", "{}");
    s.barrier();
    let t0 = Instant::now();
    let r2 = mcp.call("list_sessions", Some("{}"), &thread);
    assert!(is_error(&r2) && text_is_pairing_refusal(&r2));
    assert!(t0.elapsed() < CONFIRM_WAIT, "no second wait");
    // A reveal for the bound thread lifts the stop, and then the call is served on its
    // confirmation.
    let text = result_text(&r2);
    s.report(&thread, "list_sessions", "{}", true, &text);
    s.port
        .wait_for("the stop lifted", |c| !c.stopped.contains(&1));
    assert!(s.port.is_bound(1));
    s.fake.mcp_start(&thread, NAME, "list_sessions", "{}");
    s.barrier();
    let r3 = mcp.call("list_sessions", Some("{}"), &thread);
    assert_eq!(result_text(&r3), SERVED);
    mcp.stop();
}

#[test]
fn a_confirmation_ends_with_its_item_its_turn_or_600_seconds() {
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let mut mcp = s.pair(&thread);
    let refused = |mcp: &mut Mcp| {
        let r = mcp.call("list_sessions", Some("{}"), &thread);
        assert!(
            is_error(&r) && text_is_pairing_refusal(&r),
            "{}",
            r.to_compact()
        );
        result_text(&r)
    };
    let relift = |text: &str| {
        s.report(&thread, "list_sessions", "{}", true, text);
        s.port
            .wait_for("the stop lifted", |c| !c.stopped.contains(&1));
    };
    // The item's item/completed.
    let item = s.fake.mcp_start(&thread, NAME, "list_sessions", "{}");
    s.fake.mcp_complete(&thread, &item, false, SERVED);
    s.barrier();
    let text = refused(&mut mcp);
    relift(&text);
    // The turn's turn/completed.
    s.fake.mcp_start(&thread, NAME, "list_sessions", "{}");
    s.fake.complete_turn(&thread, "done");
    s.barrier();
    let text = refused(&mut mcp);
    // (The thread needs a running turn again for Codex to report a call in it.)
    s.fake.tui_turn(&thread, "next turn");
    relift(&text);
    // 600 seconds.
    s.fake.mcp_start(&thread, NAME, "list_sessions", "{}");
    s.barrier();
    s.clock.advance(Duration::from_secs(601));
    let text = refused(&mut mcp);
    relift(&text);
    mcp.stop();
}

#[test]
fn a_confirmation_needs_the_same_thread_tool_and_arguments() {
    let s = Scene::new();
    let t1 = s.fake.busy_thread();
    let t2 = s.fake.busy_thread();
    let mut mcp = s.pair(&t1);
    // Another thread's report does not confirm t1's connection (the impersonation row).
    s.fake.mcp_start(&t2, NAME, "list_sessions", "{}");
    s.barrier();
    let r = mcp.call("list_sessions", Some("{}"), &t1);
    assert!(is_error(&r));
    assert_eq!(s.pairing.diagnostics().confirmed, 0);
}

#[test]
fn a_report_from_another_server_is_not_a_confirmation() {
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let mut mcp = s.pair(&thread);
    s.fake
        .mcp_start(&thread, "another-server", "list_sessions", "{}");
    s.fake.mcp_start(&thread, NAME, "whoami", "{}");
    s.fake
        .mcp_start(&thread, NAME, "list_sessions", "{\"x\":1}");
    s.barrier();
    let r = mcp.call("list_sessions", Some("{}"), &thread);
    assert!(is_error(&r));
    mcp.stop();
}

#[test]
fn a_served_call_whose_reported_content_differs_is_treated_as_unconfirmed() {
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let mut mcp = s.pair(&thread);
    let item = s.fake.mcp_start(&thread, NAME, "list_sessions", "{}");
    s.barrier();
    let r = mcp.call("list_sessions", Some("{}"), &thread);
    assert_eq!(result_text(&r), SERVED);
    // Codex reports other content for the confirmed call ([MCPB-ATT-017]): the connection
    // stops, as for a missing confirmation.
    s.fake
        .mcp_complete(&thread, &item, false, "{\"sessions\":[\"someone else's\"]}");
    s.port.wait_for("the stop", |c| c.stopped.contains(&1));
    assert_eq!(s.pairing.diagnostics().content_mismatches, 1);
    assert_eq!(s.port.log().last().copied(), Some("unconfirmed"));
    mcp.stop();
}

#[test]
fn the_pairing_never_steers_and_never_lists_threads_with_previews() {
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let mcp = s.pair(&thread);
    s.forbidden_calls_absent();
    for c in s.fake.client_calls() {
        let flags = get(&c, "flags");
        for f in ["steering", "handOff", "experimentalGateRefused"] {
            assert_ne!(
                flags.and_then(|x| get(x, f)).and_then(Json::as_bool),
                Some(true),
                "{}",
                c.to_compact()
            );
        }
    }
    mcp.stop();
}

// ---- the registered name ---------------------------------------------------------------------

#[test]
fn a_report_under_another_name_than_the_registered_one_pairs_nothing_and_is_counted() {
    let lines = Lines::default();
    let s = Scene::with(|c| c.with_log(collecting(&lines)));
    let thread = s.fake.busy_thread();
    let (_mcp, text) = s.refused_whoami(&thread, "{}");
    // The operator registered the shim under another name than the one this adapter was set
    // with: Codex reports the refused call, with the refusal as its result, as that server's.
    s.report_as("oac-g8live", &thread, "whoami", "{}", true, &text);
    s.report_as("oac-g8live", &thread, "whoami", "{}", true, &text);
    assert!(!s.port.is_bound(1));
    let d = s.pairing.diagnostics();
    assert_eq!((d.reveals, d.other_server), (0, 2));
    assert_eq!(s.port.log(), ["listed", "request"]);
    // One line for the name, not one per call; it names both names and holds no thread id
    // and no pairing value.
    let named: Vec<String> = lock(&lines)
        .iter()
        .filter(|l| l.contains("not the registered name"))
        .cloned()
        .collect();
    assert_eq!(named.len(), 1, "{named:?}");
    assert!(
        named[0].contains("oac-g8live") && named[0].contains(NAME),
        "{named:?}"
    );
    assert!(!named[0].contains(&thread), "{named:?}");
    assert!(!named[0].contains("oac-pair-"), "{named:?}");
    // A second name is a second line.
    s.report_as("another", &thread, "whoami", "{}", true, &text);
    let count = lock(&lines)
        .iter()
        .filter(|l| l.contains("not the registered name"))
        .count();
    assert_eq!(count, 2);
    assert_eq!(s.pairing.diagnostics().other_server, 3);
}

#[test]
fn an_unsafe_other_name_is_sanitised_and_bounded_in_the_log() {
    let lines = Lines::default();
    let s = Scene::with(|c| c.with_log(collecting(&lines)));
    let thread = s.fake.busy_thread();
    let (_mcp, _text) = s.refused_whoami(&thread, "{}");
    let nasty = format!("a b\n\u{1b}[31m{}", "x".repeat(500));
    let item = s.fake.mcp_start(&thread, &nasty, "whoami", "{}");
    s.fake.mcp_complete(&thread, &item, true, "x");
    s.barrier();
    let all = lock(&lines).join("\n");
    let line = all
        .lines()
        .find(|l| l.contains("not the registered name"))
        .unwrap_or_else(|| panic!("no line: {all}"));
    assert!(line.len() < 400, "{}", line.len());
    assert!(!line.contains('\u{1b}') && line.contains("a?b?"), "{line}");
    // The name is one line: its newline became a character of the name.
    assert_eq!(
        all.lines()
            .filter(|l| l.contains("not the registered name"))
            .count(),
        1,
        "{all}"
    );
}

#[test]
fn a_registered_name_other_than_the_default_pairs_the_connection() {
    let lines = Lines::default();
    let s = Scene::with(|mut c| {
        c.registered_name = "oac-g8live".to_owned();
        c.with_log(collecting(&lines))
    });
    assert_eq!(s.pairing.config().registered_name, "oac-g8live");
    let thread = s.fake.busy_thread();
    let (_mcp, text) = s.refused_whoami(&thread, "{}");
    // The name the default would use is another server's now.
    s.report_as("oac", &thread, "whoami", "{}", true, &text);
    assert!(!s.port.is_bound(1));
    assert_eq!(s.pairing.diagnostics().other_server, 1);
    // The configured name reveals.
    s.report_as("oac-g8live", &thread, "whoami", "{}", true, &text);
    s.port.wait_for("the binding", |c| c.bound.contains_key(&1));
    let d = s.pairing.diagnostics();
    assert_eq!((d.reveals, d.other_server, d.unmatched_reports), (1, 1, 0));
    assert_eq!(lock(&s.port.core).bound.get(&1), Some(&thread));
}

#[test]
fn the_window_and_the_unpairable_call_are_logged_with_counts_only() {
    let lines = Lines::default();
    let s = Scene::with(|c| c.with_log(collecting(&lines)));
    // Nothing loaded: unpairable.
    let mut mcp = Mcp::connect(&s.pairing, 1);
    mcp.open();
    let r = mcp.call("whoami", Some("{}"), "x");
    assert!(is_error(&r));
    // A busy thread: a window.
    let thread = s.fake.busy_thread();
    let r = mcp.call("whoami", Some("{}"), &thread);
    assert!(pairing_value_of(&result_text(&r)).is_some());
    let all = lock(&lines).join("\n");
    assert!(all.contains("unpairable"), "{all}");
    assert!(
        all.contains("window opened: 1 threads subscribed, 1 served, 0 failed"),
        "{all}"
    );
    assert!(
        !all.contains(&thread) && !all.contains("oac-pair-"),
        "{all}"
    );
    mcp.stop();
}

#[test]
fn a_report_of_the_registered_server_that_matches_no_refusal_is_counted() {
    let s = Scene::new();
    let thread = s.fake.busy_thread();
    let (_mcp, text) = s.refused_whoami(&thread, "{}");
    // Not the refusal this adapter returned: nothing to reveal, nothing it served.
    s.report(&thread, "whoami", "{}", true, "unauthorized: not the text");
    assert_eq!(s.pairing.diagnostics().unmatched_reports, 1);
    // The refusal itself is a reveal, not an unmatched report.
    s.report(&thread, "whoami", "{}", true, &text);
    s.port.wait_for("the binding", |c| c.bound.contains_key(&1));
    let d = s.pairing.diagnostics();
    assert_eq!((d.reveals, d.unmatched_reports), (1, 1));
}

#[test]
fn a_registered_name_is_checked() {
    assert_eq!(validate_registered_name("oac"), Ok(()));
    assert_eq!(validate_registered_name("oac-g8live_2.a:b@c/d"), Ok(()));
    assert_eq!(
        validate_registered_name(&"n".repeat(MAX_REGISTERED_NAME)),
        Ok(())
    );
    assert_eq!(
        validate_registered_name(""),
        Err(RegisteredNameError::Empty)
    );
    assert_eq!(
        validate_registered_name(&"n".repeat(MAX_REGISTERED_NAME + 1)),
        Err(RegisteredNameError::TooLong)
    );
    for bad in ["a b", "a\nb", "a=b", "oac\"", "caf\u{e9}", "a;b", "a\\b"] {
        assert_eq!(
            validate_registered_name(bad),
            Err(RegisteredNameError::BadCharacter),
            "{bad:?}"
        );
        assert!(Config::try_new(bad).is_err());
    }
    assert_eq!(
        Config::try_new("oac-g8live").unwrap().registered_name,
        "oac-g8live"
    );
}
