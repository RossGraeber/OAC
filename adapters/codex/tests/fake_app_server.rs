// SPDX-License-Identifier: Apache-2.0

//! The client against the fake Codex app-server (F9, `tests/fakes/codex-app-server/`),
//! spawned with `node` on stdio and a loopback WebSocket listener. CI-default tier: no
//! Codex install, no credential, no network beyond loopback (`oac-testing` §2).
//!
//! The test plays `cli/` (D8): it opens the TCP stream to the fake's listener and hands
//! both halves to the client. On stdio it drives the fake's own control methods
//! (`oacFake/*`) and a "TUI user" whose turns make a thread busy, and it reads the fake's
//! call log for what the client sent, filtered by the client's `clientInfo.name`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use oac_adapter_codex::client::{CLIENT_NAME, ClientError, CodexClient, Options};
use oac_adapter_codex::events::{CodexEvent, CompletedItem};
use oac_adapter_codex::threads::{ThreadState, TurnEnd};
use oac_core::health::HealthState;
use oac_core::json::{self, Json};

const WAIT: Duration = Duration::from_secs(10);
const STARTUP: Duration = Duration::from_secs(60);
const TUI: &str = "oac-g6-test-tui";

fn get<'a>(v: &'a Json, name: &str) -> Option<&'a Json> {
    v.as_object()?.get(name)
}

fn get_str<'a>(v: &'a Json, name: &str) -> Option<&'a str> {
    get(v, name)?.as_str()
}

fn js(s: &str) -> String {
    Json::String(s.to_owned()).to_compact()
}

type Waiters = Arc<Mutex<HashMap<u64, Sender<Result<Json, Json>>>>>;

/// The fake, spawned; its stdio connection for control calls and the TUI user.
struct Fake {
    child: Child,
    stdin: Mutex<ChildStdin>,
    url: String,
    waiters: Waiters,
    next: AtomicU64,
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Fake {
    fn spawn() -> Fake {
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
                    && let Some(u) = get_str(&v, "url")
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
                let Some(id) = get(&v, "id")
                    .and_then(Json::as_number)
                    .and_then(|n| n.plain_integer_in(0, u64::MAX))
                else {
                    continue;
                };
                let reply = match (get(&v, "result"), get(&v, "error")) {
                    (Some(r), _) => Ok(r.clone()),
                    (_, Some(e)) => Err(e.clone()),
                    _ => continue,
                };
                if let Some(tx) = w.lock().unwrap().remove(&id) {
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

    fn call(&self, method: &str, params: &str) -> Result<Json, Json> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.waiters.lock().unwrap().insert(id, tx);
        let frame = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":{},\"params\":{params}}}\n",
            js(method)
        );
        self.stdin
            .lock()
            .unwrap()
            .write_all(frame.as_bytes())
            .unwrap();
        rx.recv_timeout(WAIT).expect("an answer from the fake")
    }

    fn control(&self, method: &str, params: &str) -> Json {
        self.call(method, params)
            .unwrap_or_else(|e| panic!("{method}: {}", e.to_compact()))
    }

    fn create_thread(&self) -> String {
        let r = self.control("oacFake/thread/create", "{\"cwd\":\"/oac-g6\"}");
        get_str(&r, "threadId").unwrap().to_owned()
    }

    fn tui_turn(&self, thread: &str, text: &str) {
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

    fn complete_turn(&self, thread: &str, agent_text: &str) {
        self.control(
            "oacFake/turn/complete",
            &format!(
                "{{\"threadId\":{},\"agentText\":{}}}",
                js(thread),
                js(agent_text)
            ),
        );
    }

    /// The client's own calls, from the fake's call log.
    fn client_calls(&self) -> Vec<Json> {
        let r = self.control(
            "oacFake/calls",
            &format!("{{\"clientName\":{}}}", js(CLIENT_NAME)),
        );
        assert_eq!(
            get(&r, "malformed")
                .and_then(Json::as_array)
                .map(<[Json]>::len),
            Some(0),
            "malformed frames from the client"
        );
        get(&r, "calls")
            .and_then(Json::as_array)
            .unwrap_or(&[])
            .to_vec()
    }

    fn connect(&self, events: &Events) -> CodexClient {
        let host = self
            .url
            .strip_prefix("ws://")
            .unwrap()
            .trim_end_matches('/');
        let s = TcpStream::connect(host).expect("connect to the fake's listener");
        let r = s.try_clone().unwrap();
        CodexClient::connect(r, s, &Options::default(), events.handler()).expect("connect")
    }
}

#[derive(Clone, Default)]
struct Events(Arc<(Mutex<Vec<CodexEvent>>, Condvar)>);

impl Events {
    fn handler(&self) -> oac_adapter_codex::EventHandler {
        let me = self.clone();
        Arc::new(move |e| {
            me.0.0.lock().unwrap().push(e);
            me.0.1.notify_all();
        })
    }

    fn all(&self) -> Vec<CodexEvent> {
        self.0.0.lock().unwrap().clone()
    }

    fn wait_for(&self, what: &str, pred: impl Fn(&[CodexEvent]) -> bool) {
        let end = Instant::now() + WAIT;
        let mut ev = self.0.0.lock().unwrap();
        while !pred(&ev) {
            let now = Instant::now();
            assert!(now < end, "no {what}; events {ev:?}");
            ev = self.0.1.wait_timeout(ev, end - now).unwrap().0;
        }
    }
}

fn method_of(c: &Json) -> &str {
    get_str(c, "method").unwrap_or_default()
}

fn flag(c: &Json, f: &str) -> Option<bool> {
    get(c, "flags")
        .and_then(|x| get(x, f))
        .and_then(Json::as_bool)
}

#[test]
fn initialize_declares_a_stable_name_and_experimental_api() {
    let fake = Fake::spawn();
    let events = Events::default();
    let client = fake.connect(&events);
    let calls = fake.client_calls();
    let init = calls
        .iter()
        .find(|c| method_of(c) == "initialize")
        .expect("an initialize call");
    let p = get(init, "params").unwrap();
    assert_eq!(
        get(p, "clientInfo").and_then(|c| get_str(c, "name")),
        Some(CLIENT_NAME)
    );
    assert_eq!(
        get(p, "capabilities")
            .and_then(|c| get(c, "experimentalApi"))
            .and_then(Json::as_bool),
        Some(true)
    );
    assert!(
        calls.iter().any(|c| method_of(c) == "initialized"),
        "the initialized notification"
    );
    // The version comes from the recorded userAgent; it differs from the shim's, which is a
    // warning only (#216).
    assert!(client.codex_version().is_some());
    let h = client.health();
    assert_eq!(h.state, HealthState::Healthy, "{h:?}");
    assert!(
        !h.detail.unwrap_or_default().contains('/'),
        "no path or address"
    );
    client.shutdown();
    assert!(!client.is_open());
    assert_eq!(client.health().state, HealthState::Unavailable);
}

#[test]
fn a_served_thread_reports_turns_and_authoritative_items_only() {
    let fake = Fake::spawn();
    let events = Events::default();
    let client = fake.connect(&events);
    let thread = fake.create_thread();
    let key = client.subscribe(&thread).expect("subscribe");
    // A second subscribe sends nothing: one subscription per thread, never per message.
    assert_eq!(client.subscribe(&thread).unwrap(), key);
    let resumes: Vec<Json> = fake
        .client_calls()
        .into_iter()
        .filter(|c| method_of(c) == "thread/resume")
        .collect();
    assert_eq!(resumes.len(), 1, "{resumes:?}");
    let mut members: Vec<String> = get(&resumes[0], "params")
        .and_then(Json::as_object)
        .unwrap()
        .names()
        .map(str::to_owned)
        .collect();
    members.sort_unstable();
    assert_eq!(members, ["excludeTurns", "threadId"], "[MCPB-CDX-006]");

    fake.tui_turn(&thread, "the TUI user's own words");
    events.wait_for("the TUI's user message", |ev| {
        ev.iter().any(|e| matches!(e, CodexEvent::ItemCompleted { thread: t, item: CompletedItem::UserMessage { text, .. }, .. } if *t == key && text == "the TUI user's own words"))
    });
    assert!(client.threads()[0].turn_running);
    fake.complete_turn(&thread, "OAC-G6-ACK final text");
    events.wait_for("the turn's end", |ev| {
        ev.iter().any(|e| matches!(e, CodexEvent::TurnCompleted { thread: t, end: TurnEnd::Completed, .. } if *t == key))
    });
    let ev = events.all();
    let agent: Vec<&str> = ev
        .iter()
        .filter_map(|e| match e {
            CodexEvent::ItemCompleted {
                item: CompletedItem::AgentMessage { text, .. },
                ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(agent, ["OAC-G6-ACK final text"], "from item/completed only");
    assert!(client.diagnostics().deltas >= 1, "the delta was dropped");
    assert!(ev.iter().any(|e| matches!(
        e,
        CodexEvent::Status {
            state: ThreadState::Active,
            ..
        }
    )));
    let v = &client.threads()[0];
    assert!(!v.turn_running);
    assert_eq!(v.state, ThreadState::Idle);
    // Nothing the client reports names the thread.
    let shown = format!(
        "{ev:?} {:?} {:?} {client:?}",
        client.threads(),
        client.health()
    );
    assert!(!shown.contains(&thread), "{shown}");
    let d = client.diagnostics();
    assert_eq!(
        (d.malformed, d.unparseable, d.server_requests),
        (0, 0, 0),
        "{d:?}"
    );
    client.shutdown();
}

#[test]
fn the_shim_holds_input_with_thread_queue_add_and_never_steers() {
    let fake = Fake::spawn();
    let events = Events::default();
    let client = fake.connect(&events);
    let thread = fake.create_thread();
    let key = client.subscribe(&thread).unwrap();
    let id = client.next_client_user_message_id();
    assert_ne!(id, client.next_client_user_message_id());
    client
        .queue_add(key, "held for the thread", &id)
        .expect("queued");
    // An idle thread whose last turn completed takes the add at once (S3 L65-L73).
    events.wait_for("the queued input as a user message", |ev| {
        ev.iter().any(|e| matches!(e, CodexEvent::ItemCompleted { item: CompletedItem::UserMessage { text, client_id, .. }, .. } if text == "held for the thread" && client_id.as_deref() == Some(id.as_str())))
    });
    fake.complete_turn(&thread, "done");

    // The recorded archived refusal: reported as a refusal, with no other call after it.
    fake.control(
        "oacFake/thread/setArchived",
        &format!("{{\"threadId\":{},\"archived\":true}}", js(&thread)),
    );
    let before = fake.client_calls().len();
    let e = client
        .queue_add(key, "turned away", &client.next_client_user_message_id())
        .unwrap_err();
    assert!(
        matches!(e, ClientError::Refused { code: -32600, .. }),
        "{e}"
    );
    assert!(!format!("{e} {e:?}").contains(&thread));
    let calls = fake.client_calls();
    assert_eq!(
        calls.len(),
        before + 1,
        "one call for the refused add, nothing after it"
    );

    for c in &calls {
        assert_ne!(flag(c, "steering"), Some(true), "{}", c.to_compact());
        assert_ne!(
            flag(c, "experimentalGateRefused"),
            Some(true),
            "{}",
            c.to_compact()
        );
        let overrides = get(c, "flags")
            .and_then(|f| get(f, "overrideMembers"))
            .and_then(Json::as_array)
            .map_or(0, <[Json]>::len);
        assert_eq!(overrides, 0, "[MCPB-CDX-005]: {}", c.to_compact());
    }
    let adds: Vec<&Json> = calls
        .iter()
        .filter(|c| flag(c, "handOff") == Some(true))
        .collect();
    assert_eq!(adds.len(), 2);
    assert!(adds.iter().all(|c| method_of(c) == "thread/queue/add"));
    client.shutdown();
}

#[test]
fn values_that_are_not_thread_ids_are_never_sent() {
    let fake = Fake::spawn();
    let client = fake.connect(&Events::default());
    let before = fake.client_calls().len();
    for bad in ["", "not-a-thread", "\",\"model\":\"x"] {
        assert_eq!(client.subscribe(bad), Err(ClientError::InvalidThreadId));
    }
    assert_eq!(fake.client_calls().len(), before);
    // A well-formed id no thread has: the recorded refusal, and the thread is not served.
    let e = client
        .subscribe("01a1186d-0000-7000-8000-000000000000")
        .unwrap_err();
    assert!(
        matches!(e, ClientError::Refused { code: -32600, .. }),
        "{e}"
    );
    assert!(client.threads().is_empty());
    client.shutdown();
}

#[test]
fn unsubscribing_stops_the_events() {
    let fake = Fake::spawn();
    let events = Events::default();
    let client = fake.connect(&events);
    let thread = fake.create_thread();
    let key = client.subscribe(&thread).unwrap();
    client.unsubscribe(key).unwrap();
    assert_eq!(client.unsubscribe(key), Err(ClientError::NotServed));
    assert!(client.key_of(&thread).is_none());
    let n = events.all().len();
    // The fake does not model thread/unsubscribe, so it still sends this thread's events
    // to the carrier, as live Codex can (row 70): the client's filter drops them.
    fake.tui_turn(&thread, "after unsubscribe");
    fake.complete_turn(&thread, "x");
    // Frames on the carrier arrive in order, so once the answer to a later call is in,
    // every notification sent before it has been dispatched. No sleep.
    let other = fake.create_thread();
    client.subscribe(&other).unwrap();
    assert!(client.diagnostics().not_served > 0);
    assert_eq!(events.all().len(), n, "{:?}", events.all());
    client.shutdown();
}
