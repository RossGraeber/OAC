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
//! [`CodexHarness`] is a complete harness over the fake, for an adapter that takes the
//! binding's two connections (`spec/bindings/mcp.md` §4.5): the **carrier**, a socket to the
//! fake's listener that the daemon designates ([`Designate`]) and gives to the adapter
//! first, and one **shim connection** per session, the stdio of the OAC MCP server that
//! Codex started, over which the harness speaks MCP as Codex does. A session opens
//! the way Codex's does: `initialize`, `tools/list`, then a call that the core side refuses
//! with a pairing value ([`Scripted::RefusePairing`]), which the fake reports as an
//! `mcpToolCall` item (`oacFake/mcp/start`, `oacFake/mcp/complete`, replaying the S1 capture)
//! so that the adapter sees its reveal on the carrier. Every later call is reported the same
//! way before it is made, which is Codex's own confirmation.
//!
//! [`PROFILE`] is `spec/bindings/mcp.md` §8.2.1: `thread/queue/add` is the holding
//! hand-off; `turn/start` and `turn/steer` are steering operations; a hand-off carries no
//! setting override ([MCPB-CDX-002] to [MCPB-CDX-005]).
//!
//! No Codex install, credential or network beyond loopback (`oac-testing` §2). `node` must
//! be on `PATH`, as it is on every CI image.
//!
//! Startup waits up to [`STARTUP_WAIT`] for the listener, apart from the per-call
//! [`WAIT`], and fails at once if `node` exits first. On any startup error the child is
//! killed and reaped before the error returns (on Unix its whole process group with it; on
//! Windows the direct child only, see `kill_group`), and the error carries the child's exit
//! status and its stderr so far, read for at most a second more, so that a descendant
//! holding the pipe cannot hang it (#340, PR #341 review).

use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oac_core::delivery::ErrorCode;
use oac_core::json::{self, Json};
use oac_fake_claude::evidence::{member, member_str};

use oac_core::adapter::{AdapterEvent, Connection, ConnectionHandle, ProviderAdapter};

use crate::claude::{FETCH_CAPABILITIES, OAC_TOOLS, PipeWriter, json_string, pipe};
use crate::{
    AdapterHarness, CoreSide, Gap, HandOffCall, HarnessRequest, Observations, ObservedResult,
    Profile, Rule, Scripted, Step, WAIT, peer_session,
};

/// The holding hand-off ([SEC-AUZ-025]; `spec/bindings/mcp.md` §8.2.1).
pub const HOLDING_HAND_OFF: &str = "thread/queue/add";

/// The Codex profile of `spec/bindings/mcp.md` §8.2.1.
pub const PROFILE: Profile = Profile {
    name: "codex-app-server",
    // The calls the fake flags `handOff` (its README, "Call flags").
    surface_operations: &[HOLDING_HAND_OFF, "turn/start", "turn/steer"],
    holding_hand_off: Some(HOLDING_HAND_OFF),
    steering_operations: &["turn/start", "turn/steer"],
    rules: &[
        ("MCPB-CDX-002", Rule::OnlyHolding),
        ("MCPB-CDX-003", Rule::Never("turn/start")),
        ("MCPB-CDX-004", Rule::Never("turn/steer")),
        ("MCPB-CDX-005", Rule::NoOverrides),
    ],
    // A Codex harness turns a hand-off away with the recorded archived refusal
    // ([`CodexFake::set_archived`]), which section 8.2.1 classifies `handoff-failed`
    // ([SC-DLV-009]), not "cannot take input now" ([SC-DLV-008]).
    turned_away_code: Some(ErrorCode::HandoffFailed),
    // The TUI user's turns (`CodexFake::tui_turn`) make a thread busy.
    runs_own_turns: true,
    // Codex's requests travel the MCP tool path shared with Claude (`spec/bindings/mcp.md`
    // section 5). [`CodexHarness`] makes them over the adapter's shim connection, each
    // reported to the carrier as Codex reports a call (`oacFake/mcp/*`), so that the
    // adapter's pairing and confirmation are in the path.
    makes_requests: true,
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

/// How long the fake may take to start and report its listener. Longer than the per-call
/// [`WAIT`]: a cold `node` start on a CI runner, with other tests spawning `node` at the
/// same time, has taken more than 10 s (#340). A `node` that exits first fails at once.
pub const STARTUP_WAIT: Duration = Duration::from_secs(60);

/// How often startup checks whether `node` has exited.
const STARTUP_POLL: Duration = Duration::from_millis(100);

/// How long a failed startup waits for the stderr reader to finish, at most.
const READER_GRACE: Duration = Duration::from_secs(1);

/// On Unix, kill the process group `pid` leads (the child leads its own, `spawn_command`),
/// so that its descendants die with it. Best effort; the caller still kills and reaps the
/// child itself.
///
/// Windows has no such kill here, on purpose. `taskkill /T` walks the tree by each
/// process's recorded `ParentProcessId`, which Windows never clears: once a parent exits
/// its pid can be reused, and a process whose stale parent id names the reused pid is
/// killed as a "child", which can be any unrelated process, `explorer.exe` included (PR #341
/// delta review). On Windows only the direct child is killed (`Child::kill`); a descendant
/// that outlives it is not, but cannot hang the caller either, since the stderr reader is
/// waited for at most [`READER_GRACE`]. A Job Object would kill the tree safely, at the
/// cost of a Windows API dependency.
#[cfg(unix)]
fn kill_group(pid: u32) {
    let _ = Command::new("kill")
        .args(["-s", "KILL", "--", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Why the fake did not start, with what it wrote to stderr so far. The child has been
/// killed and reaped by the time this exists (`exit` is what the reap returned).
#[derive(Debug)]
pub struct StartupError {
    /// What went wrong.
    pub reason: String,
    /// The child's exit status, from the reap; `None` only if it could not be reaped.
    pub exit: Option<ExitStatus>,
    /// The child's process id.
    pub pid: u32,
    /// The stderr lines read before the failure.
    pub stderr: Vec<String>,
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the fake Codex app-server did not start: {}",
            self.reason
        )?;
        if let Some(status) = self.exit {
            write!(f, " (process reaped, {status})")?;
        }
        if self.stderr.is_empty() {
            write!(f, "; it wrote nothing to stderr")
        } else {
            write!(f, "; its stderr so far:\n{}", self.stderr.join("\n"))
        }
    }
}

impl From<StartupError> for io::Error {
    fn from(e: StartupError) -> Self {
        io::Error::other(e.to_string())
    }
}

impl CodexFake {
    /// Spawn the fake and wait for its listener, up to [`STARTUP_WAIT`].
    ///
    /// # Errors
    ///
    /// A [`StartupError`] (as an `io::Error`) when `node` cannot be spawned, exits before
    /// reporting its listener, or does not report it in time. The child never outlives the
    /// error: it is killed and reaped first.
    pub fn spawn() -> io::Result<CodexFake> {
        let mut cmd = Command::new("node");
        cmd.arg(server_path())
            .args(["--stdio", "--listen", "ws://127.0.0.1:0"]);
        Self::spawn_command(cmd, STARTUP_WAIT).map_err(io::Error::from)
    }

    /// [`CodexFake::spawn`] with any command and startup timeout (for the startup tests).
    ///
    /// # Errors
    ///
    /// See [`CodexFake::spawn`].
    #[doc(hidden)]
    pub fn spawn_command(mut cmd: Command, startup: Duration) -> Result<CodexFake, StartupError> {
        // Its own process group on Unix, so that a startup failure can kill the whole tree
        // (PR #341 review N1). Windows has no safe equivalent here (`kill_group`).
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| StartupError {
                reason: format!("cannot spawn it: {e}"),
                exit: None,
                pid: 0,
                stderr: Vec::new(),
            })?;
        let stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let stderr = child.stderr.take().expect("piped");
        let (url_tx, url_rx) = mpsc::channel();
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let seen_w = seen.clone();
        let (reader_done, reader_finished) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            // Dropped when the reader ends, which disconnects `reader_finished`.
            let _done = reader_done;
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
                if !sent {
                    seen_w
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(line.clone());
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
        // Wait for the listener, failing fast if `node` exits first (#340).
        let deadline = Instant::now() + startup;
        let failed = |reason: String, mut child: Child| {
            // Kill and reap on every error path: the child never outlives the error. On Unix
            // its process group first, so that no descendant keeps running (or keeps the
            // pipe); on Windows the direct child only (`kill_group`).
            #[cfg(unix)]
            kill_group(child.id());
            let _ = child.kill();
            let exit = child.wait().ok();
            // Give the reader a moment to finish the lines already written, but no more:
            // a descendant that still holds the pipe must not turn the timeout into a hang
            // (PR #341 review N1). The lines read so far are enough for the message.
            let _ = reader_finished.recv_timeout(READER_GRACE);
            let stderr = seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
            StartupError {
                reason,
                exit,
                pid: child.id(),
                stderr,
            }
        };
        let url = loop {
            match url_rx.recv_timeout(STARTUP_POLL) {
                Ok(url) => break url,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                // stderr closed: `node` is exiting; wait for it below without spinning.
                Err(mpsc::RecvTimeoutError::Disconnected) => std::thread::sleep(STARTUP_POLL),
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Err(failed(
                        format!("node exited before reporting its listener ({status})"),
                        child,
                    ));
                }
                Ok(None) => {}
                Err(e) => return Err(failed(format!("cannot poll node: {e}"), child)),
            }
            if Instant::now() >= deadline {
                return Err(failed(
                    format!("no listener reported within {} s", startup.as_secs_f64()),
                    child,
                ));
            }
        };
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

    /// Archive `thread`, or put it back (`oacFake/thread/setArchived`). Archived, the thread
    /// is in the state the S3 capture recorded after `thread/archive` (archived, not loaded,
    /// `docs/planning/gates/fixtures/s3-codex-capture/transcript-2026-10-07-0.161.0-herdr.jsonl`
    /// L1023-L1030), and every `thread/queue/add` to it gets the recorded refusal, `-32600
    /// "session <id> is archived. ..."` (L1030-L1032), which `spec/bindings/mcp.md` section
    /// 8.2.1 names among the add's refusals and classifies `handoff-failed` ([SC-DLV-009]).
    /// Put back, it is loaded and idle again. The thread must be idle with an empty queue.
    /// The restore leaves a recorded state (loaded and idle, S3 L981-L1018) by a transition
    /// that was not recorded (no `thread/unarchive` capture). Only IFC-ADP-056's detection
    /// of a re-sent refused add uses it; SEC-AUZ-027 and IFC-ADP-057 catch that case
    /// without it.
    pub fn set_archived(&self, thread: &str, archived: bool) {
        self.control(
            "oacFake/thread/setArchived",
            &format!(
                "{{\"threadId\":{},\"archived\":{archived}}}",
                json_string(thread)
            ),
        );
    }

    /// The errors the fake answered to the `thread/queue/add` calls of the client named
    /// `client`, in call order, as `(code, message)`.
    pub fn refused_queue_adds(&self, client: &str) -> Vec<(i64, String)> {
        let report = self.control(
            "oacFake/calls",
            &format!("{{\"clientName\":{}}}", json_string(client)),
        );
        member(&report, "calls")
            .and_then(Json::as_array)
            .unwrap_or(&[])
            .iter()
            .filter(|c| member_str(c, "method") == Some("thread/queue/add"))
            .filter_map(|c| member(c, "error"))
            .map(|e| {
                (
                    member(e, "code")
                        .and_then(Json::as_number)
                        .and_then(|n| n.raw().parse().ok())
                        .unwrap_or_default(),
                    member_str(e, "message").unwrap_or_default().to_owned(),
                )
            })
            .collect()
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
        let to_call = |c: &Json| HandOffCall {
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
        };
        // Every hand-off or steering call of the client, on any thread or none.
        let hand_off_class = |c: &&Json| {
            flag(c, "handOff").and_then(|f| f.as_bool()) == Some(true)
                || flag(c, "steering").and_then(|f| f.as_bool()) == Some(true)
        };
        let client_hand_off_calls: Vec<HandOffCall> =
            calls.iter().filter(hand_off_class).map(to_call).collect();
        let hand_off_calls = calls
            .iter()
            .filter(hand_off_class)
            .filter(|c| member(c, "params").and_then(|p| member_str(p, "threadId")) == Some(thread))
            .map(to_call)
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
            client_hand_off_calls,
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

// ---- the harness --------------------------------------------------------------------------

/// The `clientInfo.name` the Codex adapter presents to the app-server
/// (`oac_adapter_codex::client::CLIENT_NAME`): [`CodexFake::observations`] reads only the
/// calls of the client of this name.
pub const ADAPTER_CLIENT: &str = "oac-codex-adapter";

/// How the daemon designates the carrier: called with the carrier's handle after the core side
/// accepted the connection and before the adapter is given it (for the Codex adapter,
/// `CodexAdapter::designate_carrier`). Returns whether the designation was taken.
pub type Designate = Box<dyn Fn(&ConnectionHandle) -> bool + Send + Sync>;

/// A socket to the fake's listener, moved to and from the carrier connection's pipe by two
/// threads. Dropping it closes the socket.
struct CarrierSocket {
    socket: TcpStream,
    handle: ConnectionHandle,
}

impl Drop for CarrierSocket {
    fn drop(&mut self) {
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
    }
}

/// Codex's side of one shim connection: MCP over newline-delimited JSON on the pipe the
/// adapter reads and writes.
struct McpPipe {
    to_adapter: Option<PipeWriter>,
    from_adapter: Receiver<Vec<u8>>,
    partial: Vec<u8>,
    frames: VecDeque<Json>,
    next_id: u64,
}

impl McpPipe {
    fn send(&mut self, frame: &str) -> Step<()> {
        let w = self
            .to_adapter
            .as_mut()
            .ok_or_else(|| Gap::Broken("the shim connection is closed".into()))?;
        w.write_all(format!("{frame}\n").as_bytes())
            .map_err(|e| Gap::Broken(format!("writing to the adapter: {e}")))
    }

    fn next_frame(&mut self, end: Instant) -> Step<Json> {
        loop {
            if let Some(v) = self.frames.pop_front() {
                return Ok(v);
            }
            let now = Instant::now();
            if now >= end {
                return Err(Gap::Broken("no frame from the adapter in time".into()));
            }
            let chunk = self
                .from_adapter
                .recv_timeout(end - now)
                .map_err(|e| Gap::Broken(format!("reading from the adapter: {e}")))?;
            self.partial.extend_from_slice(&chunk);
            while let Some(i) = self.partial.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.partial.drain(..=i).collect();
                let text = String::from_utf8_lossy(&line);
                let text = text.trim();
                if !text.is_empty() {
                    let v = json::parse(text.as_bytes()).map_err(|_| {
                        Gap::Broken("the adapter sent a frame that is not JSON".into())
                    })?;
                    self.frames.push_back(v);
                }
            }
        }
    }

    /// A request and its response (other frames are skipped).
    fn request(&mut self, method: &str, params: &str) -> Step<Json> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":{},\"params\":{params}}}",
            json_string(method)
        ))?;
        let end = Instant::now() + WAIT;
        loop {
            let v = self.next_frame(end)?;
            if member(&v, "id")
                .and_then(Json::as_number)
                .is_some_and(|n| n.raw() == id.to_string())
            {
                return Ok(v);
            }
        }
    }

    /// A `tools/call`: whether the result is an error, and its first text block.
    fn call(&mut self, tool: &str, args: &str) -> Step<ObservedResult> {
        let v = self.request(
            "tools/call",
            &format!("{{\"name\":{},\"arguments\":{args}}}", json_string(tool)),
        )?;
        let r = member(&v, "result").ok_or_else(|| {
            Gap::Broken(format!(
                "tools/call {tool} was answered with an error: {}",
                v.to_compact()
            ))
        })?;
        Ok(ObservedResult {
            is_error: member(r, "isError").and_then(Json::as_bool) == Some(true),
            text: member(r, "content")
                .and_then(Json::as_array)
                .and_then(|c| c.first())
                .and_then(|b| member_str(b, "text"))
                .unwrap_or_default()
                .to_owned(),
        })
    }
}

struct CodexSession {
    thread: String,
    mcp: McpPipe,
    /// Inbox-style surfaces the adapter's MCP server offered ([MCPB-DLV-002]).
    offered: Vec<String>,
}

/// The adapter contract suite's harness over the fake Codex app-server, for an adapter with
/// the Codex binding's two connections (see the module documentation).
pub struct CodexHarness {
    adapter: Arc<dyn ProviderAdapter>,
    designate: Designate,
    registered_name: String,
    files: Vec<PathBuf>,
    fake: CodexFake,
    core: Option<Arc<CoreSide>>,
    carrier: Option<CarrierSocket>,
    sessions: Vec<CodexSession>,
    own: Vec<String>,
}

impl CodexHarness {
    /// A harness for `adapter`, which is registered with Codex as `registered_name` (the
    /// `server` of every reported call), whose carrier the daemon designates with
    /// `designate`, and whose source `files` the static checks read. Spawns the fake
    /// Codex app-server.
    ///
    /// # Errors
    ///
    /// The fake's startup error.
    pub fn new(
        adapter: Arc<dyn ProviderAdapter>,
        registered_name: &str,
        designate: Designate,
        files: Vec<PathBuf>,
    ) -> io::Result<CodexHarness> {
        Ok(CodexHarness {
            adapter,
            designate,
            registered_name: registered_name.to_owned(),
            files,
            fake: CodexFake::spawn()?,
            core: None,
            carrier: None,
            sessions: Vec::new(),
            own: Vec::new(),
        })
    }

    fn session(&mut self, s: usize) -> Step<&mut CodexSession> {
        self.sessions
            .get_mut(s)
            .ok_or_else(|| Gap::Broken(format!("no session {s}")))
    }

    fn tui_text(&mut self, what: &str) -> String {
        let t = format!("oac contract tui {what} #{}#", self.own.len());
        self.own.push(t.clone());
        t
    }

    /// The listener's socket, as the core side's connection: the carrier.
    fn open_carrier(&mut self, core: &CoreSide) -> Step<Connection> {
        let host = self
            .fake
            .url()
            .strip_prefix("ws://")
            .ok_or_else(|| Gap::Broken("the fake's url is not ws://".into()))?
            .trim_end_matches('/')
            .to_owned();
        let socket =
            TcpStream::connect(host).map_err(|e| Gap::Broken(format!("connecting: {e}")))?;
        let (mut to_adapter, adapter_reads) = pipe();
        let (adapter_writes, from_adapter) = mpsc::channel::<Vec<u8>>();
        let conn = core.accept(adapter_reads, PipeWriter::from(adapter_writes));
        let mut read = socket.try_clone().map_err(|e| Gap::Broken(e.to_string()))?;
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 8192];
            while let Ok(n) = read.read(&mut buf) {
                if n == 0 || to_adapter.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
        });
        let mut write = socket.try_clone().map_err(|e| Gap::Broken(e.to_string()))?;
        std::thread::spawn(move || {
            for chunk in from_adapter {
                if write.write_all(&chunk).is_err() {
                    break;
                }
            }
        });
        // The daemon designates the carrier after accepting it and before the adapter has it.
        if !(self.designate)(conn.handle()) {
            return Err(Gap::Broken(
                "the adapter did not take the carrier designation".into(),
            ));
        }
        self.carrier = Some(CarrierSocket {
            socket,
            handle: conn.handle().clone(),
        });
        Ok(conn)
    }

    fn mcp_control(&self, method: &str, params: &str) -> Step<Json> {
        self.fake
            .call(method, params)
            .map_err(|e| Gap::Broken(format!("{method}: {}", e.to_compact())))
    }

    /// Codex reports the call it is about to make: `item/started` of an `mcpToolCall` in the
    /// thread's running turn. Returns the item.
    fn report_started(&self, thread: &str, tool: &str, args: &str) -> Step<String> {
        let r = self.mcp_control(
            "oacFake/mcp/start",
            &format!(
                "{{\"threadId\":{},\"server\":{},\"tool\":{},\"arguments\":{args}}}",
                json_string(thread),
                json_string(&self.registered_name),
                json_string(tool)
            ),
        )?;
        member_str(&r, "itemId")
            .map(str::to_owned)
            .ok_or_else(|| Gap::Broken("oacFake/mcp/start named no item".into()))
    }

    /// The call returned `result`: `item/completed`, a refusal as `failed`.
    fn report_completed(&self, thread: &str, item: &str, result: &ObservedResult) -> Step<()> {
        self.mcp_control(
            "oacFake/mcp/complete",
            &format!(
                "{{\"threadId\":{},\"itemId\":{},\"outcome\":{},\"text\":{}}}",
                json_string(thread),
                json_string(item),
                json_string(if result.is_error {
                    "failed"
                } else {
                    "completed"
                }),
                json_string(&result.text)
            ),
        )
        .map(|_| ())
    }

    fn running(&self, thread: &str) -> bool {
        member_str(&self.fake.thread_state(thread), "activeTurnId").is_some()
    }
}

/// The tool and its arguments for a request, in the shape of `oac-mcp-tools`' `inputSchema`
/// (`spec/bindings/mcp.md` §5), with the spoofed sender, when there is one, as an extra
/// `from` member that the adapter must ignore ([SC-ID-162]).
fn encode(r: &HarnessRequest) -> (&'static str, String) {
    match r {
        HarnessRequest::Send {
            to,
            text,
            claimed_from,
        } => (
            "send",
            format!(
                "{{\"to\":{},\"content\":[{{\"type\":\"text\",\"text\":{}}}]{}}}",
                json_string(to.as_str()),
                json_string(text),
                claimed_from
                    .as_ref()
                    .map(|f| format!(",\"from\":{}", json_string(f.as_str())))
                    .unwrap_or_default()
            ),
        ),
        HarnessRequest::Discover => ("list_sessions", "{}".into()),
    }
}

impl AdapterHarness for CodexHarness {
    fn name(&self) -> String {
        "fake Codex app-server (carrier and shim connections)".into()
    }

    fn adapter(&self) -> Arc<dyn ProviderAdapter> {
        self.adapter.clone()
    }

    fn profile(&self) -> Profile {
        PROFILE
    }

    fn open_session(&mut self, core: &CoreSide) -> Step<(usize, Vec<Connection>)> {
        self.core.get_or_insert_with(|| core.shared());
        let mut conns = Vec::new();
        if self.carrier.is_none() {
            conns.push(self.open_carrier(core)?);
        }
        let thread = self.fake.create_thread();
        let (to_adapter, adapter_reads) = pipe();
        let (adapter_writes, from_adapter) = mpsc::channel::<Vec<u8>>();
        conns.push(core.accept(adapter_reads, PipeWriter::from(adapter_writes)));
        self.sessions.push(CodexSession {
            thread,
            mcp: McpPipe {
                to_adapter: Some(to_adapter),
                from_adapter,
                partial: Vec::new(),
                frames: VecDeque::new(),
                next_id: 0,
            },
            offered: Vec::new(),
        });
        Ok((self.sessions.len() - 1, conns))
    }

    /// Codex starts the session: the MCP opening, then a call refused with a pairing value
    /// and reported back, so that the adapter reveals the thread to the core side; the core
    /// side is waited on for the reveal. The session's turn is then ended.
    fn session_ready(&mut self, s: usize) -> Step<()> {
        let core = self
            .core
            .clone()
            .ok_or_else(|| Gap::Broken("no session was opened".into()))?;
        let carrier_handle = self
            .carrier
            .as_ref()
            .map(|c| c.handle.clone())
            .ok_or_else(|| Gap::Broken("no carrier".into()))?;
        let thread = self.session(s)?.thread.clone();
        // The opening, as Codex 0.163.0 does it (S1 server log).
        {
            let sess = self.session(s)?;
            let init = sess.mcp.request(
                "initialize",
                "{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},\"clientInfo\":{\"name\":\"codex-mcp-client\",\"version\":\"0.163.0\"}}",
            )?;
            let mut offered = Vec::new();
            let caps = member(&init, "result").and_then(|r| member(r, "capabilities"));
            for c in FETCH_CAPABILITIES {
                if caps.and_then(|x| member(x, c)).is_some() {
                    offered.push(format!("capability {c}"));
                }
            }
            sess.mcp
                .send("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}")?;
            let tools = sess.mcp.request("tools/list", "{}")?;
            for t in member(&tools, "result")
                .and_then(|r| member(r, "tools"))
                .and_then(Json::as_array)
                .unwrap_or(&[])
            {
                if let Some(name) = member_str(t, "name")
                    && !OAC_TOOLS.contains(&name)
                {
                    offered.push(format!("tool {name}"));
                }
            }
            sess.offered = offered;
        }
        // The session's own user is mid-turn, as when a model makes a tool call.
        let text = self.tui_text("pairing");
        self.fake
            .tui_turn(&thread, &text)
            .map_err(|e| Gap::Broken(e.to_compact()))?;
        let (tool, args) = encode(&HarnessRequest::Send {
            to: peer_session(),
            text: "oac contract pairing".into(),
            claimed_from: None,
        });
        let item = self.report_started(&thread, tool, &args)?;
        core.script(Scripted::RefusePairing);
        let refusal = self.session(s)?.mcp.call(tool, &args)?;
        self.report_completed(&thread, &item, &refusal)?;
        let value = refusal
            .text
            .find("oac-pair-")
            .and_then(|i| refusal.text.get(i..i + "oac-pair-".len() + 32))
            .map(str::to_owned)
            .filter(|_| refusal.is_error)
            .ok_or_else(|| Gap::Broken("the call was not refused with a pairing value".into()))?;
        if !core.pairing_values().contains(&value) {
            return Err(Gap::Broken(
                "the pairing value in the refusal is not one the core side issued".into(),
            ));
        }
        // The reveal: a native signal on the carrier, naming the thread, carrying the value.
        let seen = |ev: &[AdapterEvent]| {
            ev.iter().find_map(|e| match e {
                AdapterEvent::NativeSignal(n) if n.revealed.as_deref() == Some(value.as_str()) => {
                    Some(n.clone())
                }
                _ => None,
            })
        };
        if !core.wait_for(|ev| seen(ev).is_some()) {
            return Err(Gap::Broken(
                "the adapter reported no reveal of the pairing value".into(),
            ));
        }
        let signal = seen(&core.events()).expect("seen above");
        if signal.native_id != thread || signal.connection.as_ref() != Some(&carrier_handle) {
            return Err(Gap::Broken(
                "the reveal does not name the session's thread, or did not arrive on the carrier"
                    .into(),
            ));
        }
        self.fake.drain(&thread);
        Ok(())
    }

    fn end_session(&mut self, s: usize) -> Step<()> {
        self.session(s)?.mcp.to_adapter = None;
        Ok(())
    }

    fn start_turn(&mut self, s: usize) -> Step<()> {
        let thread = self.session(s)?.thread.clone();
        let text = self.tui_text("turn");
        self.fake
            .tui_turn(&thread, &text)
            .map_err(|e| Gap::Broken(e.to_compact()))
    }

    fn drain(&mut self, s: usize) -> Step<()> {
        let thread = self.session(s)?.thread.clone();
        self.fake.drain(&thread);
        Ok(())
    }

    /// The thread is archived, as after `thread/archive` in the S3 capture: the recorded
    /// refusal of an add, which the binding classifies `handoff-failed` ([SC-DLV-009]).
    fn refuse_hand_offs(&mut self, s: usize) -> Step<()> {
        let thread = self.session(s)?.thread.clone();
        self.fake.drain(&thread);
        self.fake.set_archived(&thread, true);
        Ok(())
    }

    fn allow_hand_offs(&mut self, s: usize) -> Step<()> {
        let thread = self.session(s)?.thread.clone();
        self.fake.set_archived(&thread, false);
        Ok(())
    }

    /// Codex makes the call in a turn of the session's thread, reporting it first and its
    /// result after, as Codex does (the adapter serves a call on a paired connection only
    /// on that report).
    fn request(&mut self, s: usize, request: &HarnessRequest) -> Step<ObservedResult> {
        let (tool, args) = encode(request);
        let thread = self.session(s)?.thread.clone();
        let started = !self.running(&thread);
        if started {
            let text = self.tui_text("request");
            self.fake
                .tui_turn(&thread, &text)
                .map_err(|e| Gap::Broken(e.to_compact()))?;
        }
        let item = self.report_started(&thread, tool, &args)?;
        let result = self.session(s)?.mcp.call(tool, &args)?;
        self.report_completed(&thread, &item, &result)?;
        if started {
            self.fake.drain(&thread);
        }
        Ok(result)
    }

    fn observe(&mut self, s: usize) -> Observations {
        let Some(sess) = self.sessions.get(s) else {
            return Observations::default();
        };
        let mut o = self
            .fake
            .observations(ADAPTER_CLIENT, &sess.thread, &self.own);
        // [MCPB-DLV-002]: a capability or tool of the adapter's MCP server that offers
        // something a session could read repeatedly.
        o.offered_fetch_surfaces = sess.offered.clone();
        o
    }

    fn source_files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }

    /// The threads' ids, and the pairing values: none of them may appear in `health`.
    fn native_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.sessions.iter().map(|s| s.thread.clone()).collect();
        if let Some(core) = &self.core {
            ids.extend(core.pairing_values());
        }
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Is a `node` process with this pid still running? Asked of the OS, as a second check
    /// after the reap: the primary proof is that `wait()` through our own handle returned
    /// (`StartupError::exit`). The answer can only err towards "running" (a pid reused by
    /// another `node`), never report a leak as gone (PR #341 review N2).
    fn alive(pid: u32) -> bool {
        if cfg!(windows) {
            let out = Command::new("tasklist")
                .args([
                    "/FI",
                    &format!("PID eq {pid}"),
                    "/FI",
                    "IMAGENAME eq node.exe",
                    "/NH",
                    "/FO",
                    "CSV",
                ])
                .output()
                .expect("tasklist");
            String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
        } else {
            Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .status()
                .expect("kill")
                .success()
        }
    }

    fn node(script: &str) -> Command {
        let mut c = Command::new("node");
        c.args(["-e", script]);
        c
    }

    /// The reap, then the OS: the child is gone.
    fn assert_gone(e: &StartupError) {
        assert!(e.exit.is_some(), "not reaped through our handle: {e}");
        assert!(!alive(e.pid), "pid {} is still running", e.pid);
    }

    /// A `node` that exits before reporting its listener fails fast, with its exit status
    /// and its stderr, and is reaped (#340). "Fast" is well inside [`STARTUP_WAIT`]: a cold
    /// `node` can take more than 10 s just to start (PR #341 review N3).
    #[test]
    fn a_node_that_exits_fails_fast_with_status_and_stderr() {
        let started = Instant::now();
        let Err(e) = CodexFake::spawn_command(
            node("console.error('boom: cannot load the server'); process.exit(3)"),
            STARTUP_WAIT,
        ) else {
            panic!("a node that exits must not start");
        };
        assert!(
            started.elapsed() < STARTUP_WAIT - Duration::from_secs(15),
            "took {:?}, not fast",
            started.elapsed()
        );
        assert_eq!(e.exit.and_then(|s| s.code()), Some(3), "{e}");
        assert!(e.reason.contains("exited"), "{e}");
        assert!(e.stderr.iter().any(|l| l.contains("boom")), "{e}");
        assert!(e.to_string().contains("boom"), "{e}");
        assert_gone(&e);
    }

    /// A `node` that never reports a listener is killed and reaped at the timeout (#340).
    /// Whether it wrote anything first depends on how fast `node` starts (a cold Windows
    /// runner took more than 3 s to write a line), so stderr is checked by the test above,
    /// where `node` exits on its own.
    #[test]
    fn a_silent_node_is_killed_and_reaped_at_the_timeout() {
        let Err(e) = CodexFake::spawn_command(
            node("console.error('started, but no listener'); setInterval(() => {}, 1000)"),
            Duration::from_secs(3),
        ) else {
            panic!("a node with no listener must not start");
        };
        assert!(e.reason.contains("no listener reported"), "{e}");
        assert_gone(&e);
    }

    /// PR #341 review N1: a grandchild that inherits stderr and outlives the child must not
    /// stretch a failed startup past its timeout. On Unix it is killed with the child's
    /// process group. On Windows it is not (no tree kill there, see `kill_group`), so this
    /// test only bounds the return and then cleans the grandchild up itself.
    ///
    /// The bound on the return is checked always. The grandchild's own death (Unix) is
    /// checked when its pid was read before the timeout: on a cold runner `node` may not get
    /// as far as starting it within the timeout, and then there is no grandchild to check.
    #[test]
    fn a_grandchild_holding_stderr_does_not_hang_a_failed_startup() {
        let script = "const { spawn } = require('child_process');\
            const g = spawn(process.execPath, ['-e', 'setTimeout(() => {}, 120000)'],\
              { stdio: ['ignore', 'ignore', 'inherit'] });\
            console.error('grandchild ' + g.pid);\
            setInterval(() => {}, 1000);";
        let timeout = Duration::from_secs(8);
        let started = Instant::now();
        let Err(e) = CodexFake::spawn_command(node(script), timeout) else {
            panic!("a node with no listener must not start");
        };
        let took = started.elapsed();
        // The timeout, the reader's grace, and the kills: far short of the grandchild's 120 s.
        assert!(
            took < timeout + Duration::from_secs(20),
            "took {took:?}: {e}"
        );
        assert_gone(&e);
        let grandchild = e
            .stderr
            .iter()
            .find_map(|l| l.strip_prefix("grandchild "))
            .and_then(|p| p.trim().parse::<u32>().ok());
        let Some(g) = grandchild else {
            return;
        };
        if cfg!(windows) {
            // Not killed by the startup error on Windows: clean up this one pid (and only if
            // it is still a `node.exe`), without `/T`.
            if alive(g) {
                let _ = Command::new("taskkill")
                    .args(["/F", "/PID", &g.to_string(), "/FI", "IMAGENAME eq node.exe"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        } else {
            // A group kill is asynchronous to us: allow a moment.
            let deadline = Instant::now() + Duration::from_secs(10);
            while alive(g) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
            assert!(!alive(g), "grandchild {g} is still running");
        }
    }

    /// A program that cannot be spawned is an error with nothing to reap.
    #[test]
    fn a_missing_program_is_an_error() {
        let Err(e) =
            CodexFake::spawn_command(Command::new("oac-no-such-program-340"), STARTUP_WAIT)
        else {
            panic!("a missing program must not start");
        };
        assert!(e.reason.contains("cannot spawn"), "{e}");
        assert!(e.exit.is_none(), "{e}");
    }

    /// The real fake starts within the startup timeout and is reaped on drop.
    #[test]
    fn the_fake_starts_and_is_reaped_on_drop() {
        let fake = CodexFake::spawn().expect("spawn the fake Codex app-server with node");
        let pid = fake.child.id();
        assert!(fake.url().starts_with("ws://127.0.0.1:"), "{}", fake.url());
        assert!(alive(pid));
        drop(fake);
        assert!(!alive(pid), "pid {pid} is still running after drop");
    }
}
