// SPDX-License-Identifier: Apache-2.0

//! The fake Codex app-server (F9, `tests/fakes/codex-app-server`), spawned with `node` on
//! stdio and a loopback WebSocket listener, and read through its call log.
//!
//! A Codex adapter connects to the app-server itself, at [`CodexFake::url`]. The suite
//! keeps the stdio connection for the fake's own control methods (`oacFake/*`, logged
//! apart from Codex calls) and for the "TUI user" whose turns make a thread busy; those
//! Codex calls carry the client name [`TUI_CLIENT`], and [`CodexFake::observations`] reads
//! only the adapter's own (by its `clientInfo.name`).
//!
//! [`PROFILE`] is `spec/bindings/mcp.md` §8.2.1: `thread/queue/add` is the holding
//! hand-off; `turn/start` and `turn/steer` are steering operations; a hand-off carries no
//! setting override ([MCPB-CDX-002] to [MCPB-CDX-005]).
//!
//! No Codex install, credential or network beyond loopback (`oac-testing` §2). `node` must
//! be on `PATH`, as it is on every CI image.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use oac_core::json::{self, Json};
use oac_fake_claude::evidence::{member, member_str};

use crate::claude::json_string;
use crate::{HandOffCall, Observations, Profile, Rule, WAIT};

/// The holding hand-off ([SEC-AUZ-025]; `spec/bindings/mcp.md` §8.2.1).
pub const HOLDING_HAND_OFF: &str = "thread/queue/add";

/// The Codex profile of `spec/bindings/mcp.md` §8.2.1.
pub const PROFILE: Profile = Profile {
    name: "codex-app-server",
    holding_hand_off: Some(HOLDING_HAND_OFF),
    steering_operations: &["turn/start", "turn/steer"],
    rules: &[
        ("MCPB-CDX-002", Rule::OnlyHolding),
        ("MCPB-CDX-003", Rule::Never("turn/start")),
        ("MCPB-CDX-004", Rule::Never("turn/steer")),
        ("MCPB-CDX-005", Rule::NoOverrides),
    ],
};

/// The client name of the suite's own "TUI user" connection.
pub const TUI_CLIENT: &str = "oac-contract-tui";

/// Methods that establish a connection or subscribe to a thread: the first no-polling
/// class.
pub const ESTABLISHMENT_METHODS: [&str; 3] = ["initialize", "thread/start", "thread/resume"];

/// Methods that return a thread's messages on demand: the second no-polling class.
/// `thread/list` and `thread/loaded/list` return thread ids and previews, not pending
/// input, and are not counted.
pub const FETCH_METHODS: [&str; 3] = ["thread/turns/list", "thread/read", "thread/queue/list"];

/// The path of the fake's entry point.
pub fn server_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fakes/codex-app-server/server.mjs")
}

type Waiters = Arc<Mutex<HashMap<u64, Sender<Result<Json, Json>>>>>;

/// A running fake Codex app-server.
pub struct CodexFake {
    child: Child,
    stdin: Mutex<ChildStdin>,
    url: String,
    waiters: Waiters,
    next: AtomicU64,
    tui_ready: Mutex<bool>,
}

impl CodexFake {
    /// Spawn the fake and wait for its listener.
    pub fn spawn() -> io::Result<CodexFake> {
        let mut child = Command::new("node")
            .arg(server_path())
            .args(["--stdio", "--listen", "ws://127.0.0.1:0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let stderr = child.stderr.take().expect("piped");
        let (url_tx, url_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut sent = false;
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if !sent
                    && let Ok(v) = json::parse(line.as_bytes())
                    && let Some(u) = member_str(&v, "url")
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
                let Some(id) = member(&v, "id")
                    .and_then(Json::as_number)
                    .and_then(|n| n.plain_integer_in(0, u64::MAX))
                else {
                    continue; // a notification to the TUI connection
                };
                let reply = match (member(&v, "result"), member(&v, "error")) {
                    (Some(r), _) => Ok(r.clone()),
                    (_, Some(e)) => Err(e.clone()),
                    _ => continue,
                };
                if let Some(tx) = w.lock().unwrap_or_else(|e| e.into_inner()).remove(&id) {
                    let _ = tx.send(reply);
                }
            }
        });
        let url = url_rx.recv_timeout(WAIT).map_err(|_| {
            io::Error::other("the fake Codex app-server did not report its listener")
        })?;
        Ok(CodexFake {
            child,
            stdin: Mutex::new(stdin),
            url,
            waiters,
            next: AtomicU64::new(1),
            tui_ready: Mutex::new(false),
        })
    }

    /// The loopback WebSocket URL an adapter connects to.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Call `method` with `params` (JSON object text) on the stdio connection.
    ///
    /// # Errors
    ///
    /// The JSON-RPC error object, or a synthetic one when no answer came.
    pub fn call(&self, method: &str, params: &str) -> Result<Json, Json> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.waiters
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, tx);
        let frame = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":{},\"params\":{params}}}\n",
            json_string(method)
        );
        let wrote = self
            .stdin
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .write_all(frame.as_bytes());
        let none = || json::parse(b"{\"message\":\"no answer from the fake\"}").expect("json");
        if wrote.is_err() {
            return Err(none());
        }
        rx.recv_timeout(WAIT).map_err(|_| none())?
    }

    fn control(&self, method: &str, params: &str) -> Json {
        self.call(method, params)
            .unwrap_or_else(|e| panic!("{method}: {}", e.to_compact()))
    }

    /// A loaded, materialized, idle thread; its id.
    pub fn create_thread(&self) -> String {
        let r = self.control("oacFake/thread/create", "{\"cwd\":\"/oac-contract\"}");
        member_str(&r, "threadId").expect("threadId").to_owned()
    }

    fn tui(&self) {
        let mut ready = self.tui_ready.lock().unwrap_or_else(|e| e.into_inner());
        if !*ready {
            self.call(
                "initialize",
                &format!(
                    "{{\"clientInfo\":{{\"name\":{},\"version\":\"0\"}},\"capabilities\":{{\"experimentalApi\":true}}}}",
                    json_string(TUI_CLIENT)
                ),
            )
            .expect("initialize the TUI connection");
            *ready = true;
        }
    }

    /// The TUI user starts a turn on `thread` with `text`.
    ///
    /// # Errors
    ///
    /// The fake's error.
    pub fn tui_turn(&self, thread: &str, text: &str) -> Result<(), Json> {
        self.tui();
        self.call(
            "turn/start",
            &format!(
                "{{\"threadId\":{},\"input\":[{{\"type\":\"text\",\"text\":{}}}]}}",
                json_string(thread),
                json_string(text)
            ),
        )
        .map(|_| ())
    }

    /// The thread's state (`oacFake/thread/state`).
    pub fn thread_state(&self, thread: &str) -> Json {
        self.control(
            "oacFake/thread/state",
            &format!("{{\"threadId\":{}}}", json_string(thread)),
        )
    }

    /// Complete turns until the thread is idle with an empty queue (bounded).
    pub fn drain(&self, thread: &str) {
        for _ in 0..64 {
            let st = self.thread_state(thread);
            let active = member(&st, "activeTurnId").and_then(Json::as_str).is_some();
            if !active {
                // Idle. A queue left on an idle thread would mean its last turn ended
                // interrupted; nothing in the suite interrupts, so there is nothing to do.
                return;
            }
            self.control(
                "oacFake/turn/complete",
                &format!("{{\"threadId\":{}}}", json_string(thread)),
            );
        }
    }

    /// Whether `thread/queue/add` is served (`oacFake/queue/setAvailable`).
    pub fn set_queue_available(&self, available: bool) {
        self.control(
            "oacFake/queue/setAvailable",
            &format!("{{\"available\":{available}}}"),
        );
    }

    /// What the fake observed of `thread` from the client named `client`. Inputs the TUI
    /// user made (`own`) are left out.
    pub fn observations(&self, client: &str, thread: &str, own: &[String]) -> Observations {
        let report = self.control(
            "oacFake/calls",
            &format!("{{\"clientName\":{}}}", json_string(client)),
        );
        let calls: &[Json] = member(&report, "calls")
            .and_then(Json::as_array)
            .unwrap_or(&[]);
        let method = |c: &Json| member_str(c, "method").unwrap_or_default().to_owned();
        let flag = |c: &Json, f: &str| member(c, "flags").and_then(|x| member(x, f)).cloned();
        let text_of = |v: Option<&Json>| -> String {
            v.and_then(Json::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|i| member_str(i, "text"))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default()
        };
        let hand_off_calls = calls
            .iter()
            .filter(|c| flag(c, "handOff").and_then(|f| f.as_bool()) == Some(true))
            .filter(|c| member(c, "params").and_then(|p| member_str(p, "threadId")) == Some(thread))
            .map(|c| HandOffCall {
                operation: method(c),
                text: text_of(member(c, "params").and_then(|p| member(p, "input"))),
                steering: flag(c, "steering").and_then(|f| f.as_bool()) == Some(true),
                override_members: flag(c, "overrideMembers")
                    .and_then(|f| {
                        f.as_array().map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(str::to_owned))
                                .collect()
                        })
                    })
                    .unwrap_or_default(),
                accepted: member_str(c, "outcome") == Some("result"),
            })
            .collect();
        let st = self.thread_state(thread);
        let mut inputs = Vec::new();
        for t in member(&st, "turns").and_then(Json::as_array).unwrap_or(&[]) {
            for item in member(t, "items").and_then(Json::as_array).unwrap_or(&[]) {
                if member_str(item, "type") == Some("userMessage") {
                    let text = text_of(member(item, "content"));
                    if !own.contains(&text) {
                        inputs.push(text);
                    }
                }
            }
        }
        let mut odd = Vec::new();
        let malformed = member(&report, "malformed")
            .and_then(Json::as_array)
            .map_or(0, <[Json]>::len);
        if malformed > 0 {
            odd.push(format!("{malformed} malformed frame(s) from the adapter"));
        }
        let control = member(&report, "control")
            .and_then(Json::as_array)
            .map_or(0, <[Json]>::len);
        if control > 0 && client != TUI_CLIENT {
            odd.push(format!("{control} oacFake/* call(s) from the adapter"));
        }
        Observations {
            hand_off_calls,
            inputs,
            held: member(&st, "queue")
                .and_then(Json::as_array)
                .map_or(0, <[Json]>::len),
            establishment_calls: calls
                .iter()
                .filter(|c| ESTABLISHMENT_METHODS.contains(&method(c).as_str()))
                .count(),
            fetch_calls: calls
                .iter()
                .map(method)
                .filter(|m| FETCH_METHODS.contains(&m.as_str()))
                .collect(),
            offered_fetch_surfaces: Vec::new(),
            halted: (!odd.is_empty()).then(|| odd.join("; ")),
        }
    }
}

impl Drop for CodexFake {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
