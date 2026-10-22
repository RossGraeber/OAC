// SPDX-License-Identifier: Apache-2.0

//! The fake Codex app-server (F9, `tests/fakes/codex-app-server/`) spawned with `node`, its
//! control calls, and an event recorder: shared by the integration tests of this crate.
//! CI-default tier: no Codex install, no credential, no network beyond loopback.

#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use oac_adapter_codex::client::{CLIENT_NAME, CodexClient, Options};
use oac_adapter_codex::events::CodexEvent;
use oac_core::json::{self, Json};

pub const WAIT: Duration = Duration::from_secs(10);
pub const STARTUP: Duration = Duration::from_secs(60);
pub const TUI: &str = "oac-g6-test-tui";

pub fn get<'a>(v: &'a Json, name: &str) -> Option<&'a Json> {
    v.as_object()?.get(name)
}

pub fn get_str<'a>(v: &'a Json, name: &str) -> Option<&'a str> {
    get(v, name)?.as_str()
}

pub fn js(s: &str) -> String {
    Json::String(s.to_owned()).to_compact()
}

pub type Waiters = Arc<Mutex<HashMap<u64, Sender<Result<Json, Json>>>>>;

/// The fake, spawned; its stdio connection for control calls and the TUI user.
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

    pub fn call(&self, method: &str, params: &str) -> Result<Json, Json> {
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

    pub fn control(&self, method: &str, params: &str) -> Json {
        self.call(method, params)
            .unwrap_or_else(|e| panic!("{method}: {}", e.to_compact()))
    }

    pub fn create_thread(&self) -> String {
        let r = self.control("oacFake/thread/create", "{\"cwd\":\"/oac-g6\"}");
        get_str(&r, "threadId").unwrap().to_owned()
    }

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

    pub fn complete_turn(&self, thread: &str, agent_text: &str) {
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
    pub fn client_calls(&self) -> Vec<Json> {
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

    pub fn connect(&self, events: &Events) -> CodexClient {
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
pub struct Events(Arc<(Mutex<Vec<CodexEvent>>, Condvar)>);

impl Events {
    pub fn handler(&self) -> oac_adapter_codex::EventHandler {
        let me = self.clone();
        Arc::new(move |e| {
            me.0.0.lock().unwrap().push(e);
            me.0.1.notify_all();
        })
    }

    pub fn all(&self) -> Vec<CodexEvent> {
        self.0.0.lock().unwrap().clone()
    }

    pub fn wait_for(&self, what: &str, pred: impl Fn(&[CodexEvent]) -> bool) {
        let end = Instant::now() + WAIT;
        let mut ev = self.0.0.lock().unwrap();
        while !pred(&ev) {
            let now = Instant::now();
            assert!(now < end, "no {what}; events {ev:?}");
            ev = self.0.1.wait_timeout(ev, end - now).unwrap().0;
        }
    }
}

impl Fake {
    /// A thread whose turn is running: the TUI user's turn, so that an MCP tool call can be
    /// made in it (`oacFake/mcp/start` needs a running turn).
    pub fn busy_thread(&self) -> String {
        let t = self.create_thread();
        self.tui_turn(&t, "the TUI user's words");
        t
    }

    /// `oacFake/mcp/start`: the thread's running turn makes a tool call, as the S1 capture
    /// recorded it (`item/started`). Returns the item id.
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
        get_str(&r, "itemId").unwrap().to_owned()
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

    /// The client's own calls of one method, from the fake's call log.
    pub fn client_calls_of(&self, method: &str) -> Vec<Json> {
        self.client_calls()
            .into_iter()
            .filter(|c| get_str(c, "method") == Some(method))
            .collect()
    }
}
