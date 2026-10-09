// SPDX-License-Identifier: Apache-2.0

//! The app-server client: JSON-RPC 2.0 over the WebSocket carrier (G6, #67).
//!
//! **Connecting (D8).** `cli/` opens the byte stream to the app-server's control socket
//! and hands both halves to [`CodexClient::connect`]; the client does the WebSocket
//! Upgrade ([`crate::ws`]), the JSON-RPC and the `initialize`/`initialized` handshake. It
//! opens nothing itself, and it never reads `CODEX_HOME` (`auth.json`, the keyring or the
//! rollout files): it speaks only the documented app-server protocol over the stream it
//! is handed ([ADR-001 Boundary]; G-7 §8.3).
//!
//! **What it may send.** A fixed allowlist: [`STABLE_METHODS`], the `initialized`
//! notification, and the experimental methods of [`crate::shim`], each only through the
//! shim. `turn/start` and `turn/steer` are steering operations (`spec/bindings/mcp.md`
//! §8.2.1, [MCPB-CDX-003], [MCPB-CDX-004], `spec/security.md` [SEC-AUZ-022]); neither is on
//! the list, so no code path here can send one. `thread/list`, whose entries carry
//! previews (`11-risks.md` row 43), is not on it either.
//!
//! **What it reads.** Responses to its own calls, and notifications, translated by
//! [`crate::events`]: only for served threads (row 70), only `item/completed` as an item's
//! content, deltas dropped. A server-to-client request (an approval) is never answered:
//! approving or declining is the harness user's decision, never the adapter's
//! (`oac-security-work` §3). It is counted in [`Diagnostics`].
//!
//! **Identity.** The client presents a stable `clientInfo.name`, [`CLIENT_NAME`], so a
//! test or an operator can tell its calls from other clients' (the fake app-server's call
//! log filters by it). It reads no identity from the app-server: `userAgent` gives the
//! Codex version only, and no originator is ever read (rows 37, 38).
//!
//! What it does not do: hand off messages (G7, #68, through [`CodexClient::queue_add`]),
//! pair connections with threads (G8, #69, from the `mcpToolCall` items of
//! [`crate::events::CompletedItem`]), or decide which threads to serve (the caller does,
//! [`CodexClient::subscribe`]).

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use oac_core::health::{HealthState, HealthStatus};
use oac_core::json::{self, Json};

use crate::events::{self, CodexEvent, Dropped};
use crate::schema;
use crate::shim::{self, ExperimentalCall};
use crate::threads::{Registry, ThreadKey, ThreadView};
use crate::ws::{self, WsError, WsWriter};

/// The `clientInfo.name` the client presents: stable across versions.
pub const CLIENT_NAME: &str = "oac-codex-adapter";

/// The `clientInfo.title`.
pub const CLIENT_TITLE: &str = "OAC Codex adapter";

/// The stable (non-experimental) methods the client may call.
pub const STABLE_METHODS: &[&str] = &["initialize", "thread/resume", "thread/unsubscribe"];

/// The notifications the client may send.
pub const NOTIFICATIONS_SENT: &[&str] = &["initialized"];

/// The handler events are passed to, on the client's reader thread. It must not block for
/// long; the client holds no lock of its own while it runs.
pub type EventHandler = Arc<dyn Fn(CodexEvent) + Send + Sync>;

/// How to connect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The `Host` header of the Upgrade. The control socket is a local socket, so any
    /// fixed name serves; the default is `localhost`.
    pub host: String,
    /// How long a call waits for its response before it is reported as unanswered.
    pub call_timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            host: "localhost".into(),
            call_timeout: Duration::from_secs(10),
        }
    }
}

/// Why a client operation did not succeed. Adapter-internal: it never crosses into the
/// core (`oac-implementation` §5); G7 maps it to a hand-off outcome.
#[derive(Clone, PartialEq, Eq)]
pub enum ClientError {
    /// The WebSocket Upgrade or the stream failed.
    Connect(WsError),
    /// The carrier is closed, or the client was shut down.
    Closed,
    /// No response came within the call timeout. Whether the app-server acted is unknown.
    NoAnswer,
    /// The app-server answered with a JSON-RPC error. Its `message` can name a thread, so
    /// `Display` shows the code only.
    Refused {
        /// The JSON-RPC error code.
        code: i64,
        /// The error message, as sent. Never put it in health or a log.
        message: String,
    },
    /// A response lacked a member the client relies on (the shape's name).
    Shape(&'static str),
    /// A method the client may not send (not in its allowlist).
    NotAllowed(String),
    /// The thread is not served.
    NotServed,
    /// The value is not a Codex thread id.
    InvalidThreadId,
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Connect(e) => write!(f, "cannot connect to the app-server: {e}"),
            ClientError::Closed => f.write_str("the app-server connection is closed"),
            ClientError::NoAnswer => f.write_str("no answer from the app-server in time"),
            ClientError::Refused { code, .. } => write!(f, "the app-server refused (code {code})"),
            ClientError::Shape(s) => write!(
                f,
                "the app-server's {s} lacks a member the client relies on"
            ),
            ClientError::NotAllowed(m) => write!(f, "{m:?} is not a method this client sends"),
            ClientError::NotServed => f.write_str("the thread is not served"),
            ClientError::InvalidThreadId => f.write_str("not a Codex thread id"),
        }
    }
}

impl fmt::Debug for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The same as Display: a refusal's message can name a thread.
        write!(f, "ClientError({self})")
    }
}

impl std::error::Error for ClientError {}

/// Counters of what the client dropped or could not use. Counts only: nothing here names
/// a thread, and all of it may go in `health`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Diagnostics {
    /// Required members the schema has and a message lacked (drift; never a gate).
    pub schema_drift: u64,
    /// Messages dropped for lacking a member the client relies on.
    pub malformed: u64,
    /// Frames that were not a JSON-RPC message.
    pub unparseable: u64,
    /// Notifications for threads the client does not serve, dropped.
    pub not_served: u64,
    /// Delta notifications, dropped.
    pub deltas: u64,
    /// Server-to-client requests (approvals), never answered.
    pub server_requests: u64,
    /// Responses to no call the client is waiting on (late, after a timeout).
    pub stray_responses: u64,
}

type Reply = Result<Json, (i64, String)>;

struct Inner {
    writer: WsWriter,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, Sender<Reply>>>,
    registry: Mutex<Registry>,
    handler: EventHandler,
    open: AtomicBool,
    down: AtomicBool,
    stats: Mutex<Diagnostics>,
    codex_version: Mutex<Option<String>>,
    timeout: Duration,
    message_prefix: String,
    next_message: AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A connected, initialized app-server client.
#[derive(Clone)]
pub struct CodexClient {
    inner: Arc<Inner>,
}

impl fmt::Debug for CodexClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodexClient")
            .field("open", &self.is_open())
            .field("threads", &lock(&self.inner.registry).len())
            .finish_non_exhaustive()
    }
}

impl CodexClient {
    /// Upgrade the stream whose halves are `reader` and `writer` to a WebSocket, start the
    /// reader thread, and do the `initialize`/`initialized` handshake, declaring
    /// `capabilities.experimentalApi` for [`crate::shim`]. Events for served threads go to
    /// `handler`.
    ///
    /// # Errors
    ///
    /// [`ClientError::Connect`] when the Upgrade fails; otherwise the `initialize` call's
    /// error. The connection is closed on any error.
    pub fn connect(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        options: &Options,
        handler: EventHandler,
    ) -> Result<CodexClient, ClientError> {
        let (mut ws_reader, ws_writer) =
            ws::handshake(Box::new(reader), Box::new(writer), &options.host)
                .map_err(ClientError::Connect)?;
        let inner = Arc::new(Inner {
            writer: ws_writer,
            next_id: AtomicU64::new(1),
            pending: Mutex::default(),
            registry: Mutex::default(),
            handler,
            open: AtomicBool::new(true),
            down: AtomicBool::new(false),
            stats: Mutex::default(),
            codex_version: Mutex::new(None),
            timeout: options.call_timeout,
            message_prefix: random_hex(),
            next_message: AtomicU64::new(1),
        });
        let r = inner.clone();
        std::thread::Builder::new()
            .name("oac-codex-carrier".into())
            .spawn(move || {
                while let Ok(text) = ws_reader.next_text() {
                    r.dispatch(&text);
                }
                r.closed();
            })
            .map_err(|e| ClientError::Connect(WsError::Io(e.to_string())))?;
        let client = CodexClient { inner };
        if let Err(e) = client.initialize() {
            client.shutdown();
            return Err(e);
        }
        Ok(client)
    }

    fn initialize(&self) -> Result<(), ClientError> {
        let s = |v: &str| Json::String(v.to_owned()).to_compact();
        let params = format!(
            "{{\"clientInfo\":{{\"name\":{},\"title\":{},\"version\":{}}},\"capabilities\":{{\"experimentalApi\":{}}}}}",
            s(CLIENT_NAME),
            s(CLIENT_TITLE),
            s(env!("CARGO_PKG_VERSION")),
            shim::DECLARES_EXPERIMENTAL_API
        );
        let result = self.call("initialize", &params)?;
        let checked = schema::INITIALIZE_RESPONSE.check(&result);
        self.inner.count_drift(checked.drift.len());
        if !checked.usable() {
            return Err(ClientError::Shape(schema::INITIALIZE_RESPONSE.name));
        }
        // Only the version is kept: never `codexHome` (a directory) or the rest of the
        // user agent.
        *lock(&self.inner.codex_version) = result
            .as_object()
            .and_then(|o| o.get("userAgent"))
            .and_then(Json::as_str)
            .and_then(version_of_user_agent);
        self.notify("initialized", "{}")
    }

    /// Send a notification on the allowlist.
    fn notify(&self, method: &str, params: &str) -> Result<(), ClientError> {
        if !NOTIFICATIONS_SENT.contains(&method) {
            return Err(ClientError::NotAllowed(method.to_owned()));
        }
        let frame = format!(
            "{{\"jsonrpc\":\"2.0\",\"method\":{},\"params\":{params}}}",
            Json::String(method.to_owned()).to_compact()
        );
        self.inner.send(&frame)
    }

    /// Call a stable method on the allowlist.
    fn call(&self, method: &str, params: &str) -> Result<Json, ClientError> {
        if !STABLE_METHODS.contains(&method) {
            return Err(ClientError::NotAllowed(method.to_owned()));
        }
        self.inner.call(method, params)
    }

    /// Call an experimental method, built by the shim.
    fn call_experimental(&self, call: &ExperimentalCall) -> Result<Json, ClientError> {
        if !shim::EXPERIMENTAL_METHODS.contains(&call.method()) {
            return Err(ClientError::NotAllowed(call.method().to_owned()));
        }
        self.inner.call(call.method(), call.params())
    }

    /// Serve the thread whose native id is `thread`: subscribe the carrier to it with
    /// `thread/resume`, carrying only `threadId` and `excludeTurns: true`
    /// ([MCPB-CDX-006]). Serving a thread already served returns its key and sends
    /// nothing, so a thread is subscribed once, never per message (the no-polling rule,
    /// `09-test-strategy.md` §5).
    ///
    /// Which threads to serve is the caller's decision: the threads launched OAC-enabled
    /// (`[ADR-001-A2 Amendment]`).
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidThreadId`] for a value that is not a thread id; otherwise the
    /// call's error. The thread is not served after an error.
    pub fn subscribe(&self, thread: &str) -> Result<ThreadKey, ClientError> {
        if !is_thread_id(thread) {
            return Err(ClientError::InvalidThreadId);
        }
        // Served before the call, so that notifications the app-server sends before its
        // response (`thread/status/changed` on a reload: S3 capture L968-L1019) are kept.
        let (key, new) = lock(&self.inner.registry).serve(thread);
        if !new {
            return Ok(key);
        }
        let params = format!(
            "{{\"threadId\":{},\"excludeTurns\":true}}",
            Json::String(thread.to_owned()).to_compact()
        );
        let outcome = self.call("thread/resume", &params).and_then(|r| {
            let c = schema::THREAD_RESUME_RESPONSE.check(&r);
            self.inner.count_drift(c.drift.len());
            if !c.usable() {
                return Err(ClientError::Shape(schema::THREAD_RESUME_RESPONSE.name));
            }
            let none = Json::Null;
            let t = r.as_object().and_then(|o| o.get("thread")).unwrap_or(&none);
            let c = schema::THREAD.check(t);
            self.inner.count_drift(c.drift.len());
            let id = t
                .as_object()
                .and_then(|o| o.get("id"))
                .and_then(Json::as_str);
            if !c.usable() || id != Some(thread) {
                return Err(ClientError::Shape(schema::THREAD.name));
            }
            Ok(())
        });
        match outcome {
            Ok(()) => Ok(key),
            Err(e) => {
                lock(&self.inner.registry).drop_key(key);
                Err(e)
            }
        }
    }

    /// Stop serving `key`: its notifications are dropped from now on, and
    /// `thread/unsubscribe` is sent. Its answer is not waited on, since live Codex goes on
    /// sending some of a thread's notifications after it (row 70); the filter, not the
    /// unsubscribe, is what keeps them out.
    ///
    /// # Errors
    ///
    /// [`ClientError::NotServed`] when `key` is not served.
    pub fn unsubscribe(&self, key: ThreadKey) -> Result<(), ClientError> {
        let native = lock(&self.inner.registry)
            .drop_key(key)
            .ok_or(ClientError::NotServed)?;
        let params = format!("{{\"threadId\":{}}}", Json::String(native).to_compact());
        let _ = self.inner.send_call("thread/unsubscribe", &params);
        Ok(())
    }

    /// Hold `text` for the served thread `key` with `thread/queue/add`, through the
    /// experimental shim, with `client_user_message_id`.
    ///
    /// For G7 (#68) only: it frames nothing and authorizes nothing. G7 frames the text with
    /// its provenance header (`spec/security.md` §12), and maps the result to a hand-off
    /// outcome under `spec/bindings/mcp.md` §8.2.1: `Ok` is `completed`; a
    /// [`ClientError::Refused`] is `handoff-failed`; [`ClientError::NoAnswer`] is
    /// `indeterminate`. No other method is ever tried after a refusal ([SEC-AUZ-027]).
    ///
    /// # Errors
    ///
    /// [`ClientError::NotServed`]; otherwise the call's error, or
    /// [`ClientError::Shape`] when the result has no `queuedSubmission` object.
    pub fn queue_add(
        &self,
        key: ThreadKey,
        text: &str,
        client_user_message_id: &str,
    ) -> Result<(), ClientError> {
        let call = {
            let reg = lock(&self.inner.registry);
            let native = reg.native_of(key).ok_or(ClientError::NotServed)?;
            shim::queue_add(native, text, client_user_message_id)
        };
        let r = self.call_experimental(&call)?;
        if r.as_object()
            .and_then(|o| o.get(shim::QUEUE_ADD_RESULT_MEMBER))
            .and_then(Json::as_object)
            .is_none()
        {
            return Err(ClientError::Shape("ThreadQueueAddResponse"));
        }
        Ok(())
    }

    /// A fresh `clientUserMessageId`: unique in this process, and unpredictable across
    /// processes. It names no session and no thread.
    pub fn next_client_user_message_id(&self) -> String {
        format!(
            "oac-{}-{}",
            self.inner.message_prefix,
            self.inner.next_message.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// The key of a served thread, by its native id.
    pub fn key_of(&self, thread: &str) -> Option<ThreadKey> {
        lock(&self.inner.registry).key_of(thread)
    }

    /// The native id of a served thread: crate-internal (G7's add, G8's native signal).
    #[allow(dead_code)]
    pub(crate) fn native_of(&self, key: ThreadKey) -> Option<String> {
        lock(&self.inner.registry).native_of(key).map(str::to_owned)
    }

    /// What the client knows of each served thread.
    pub fn threads(&self) -> Vec<ThreadView> {
        lock(&self.inner.registry).views()
    }

    /// The counters.
    pub fn diagnostics(&self) -> Diagnostics {
        *lock(&self.inner.stats)
    }

    /// The running Codex version, from the `initialize` result's `userAgent`.
    pub fn codex_version(&self) -> Option<String> {
        lock(&self.inner.codex_version).clone()
    }

    /// A warning when the running Codex is not the version the shim's shapes were read at
    /// (#216: Codex floats; a different version warns, never gates).
    pub fn version_note(&self) -> Option<String> {
        match self.codex_version() {
            Some(v) if v == shim::SHIM_CODEX_VERSION => None,
            Some(v) => Some(format!(
                "Codex {v} is running; {} was read at {}: re-check its shapes (warning only)",
                shim::SHIM_LABEL,
                shim::SHIM_CODEX_VERSION
            )),
            None => Some(format!(
                "the Codex version is unknown; {} was read at {} (warning only)",
                shim::SHIM_LABEL,
                shim::SHIM_CODEX_VERSION
            )),
        }
    }

    /// True while the carrier is open and the client not shut down.
    pub fn is_open(&self) -> bool {
        self.inner.open.load(Ordering::SeqCst) && !self.inner.down.load(Ordering::SeqCst)
    }

    /// The client's health ([IFC-TYP-092]): its state and counts, never a thread id, a
    /// directory or an address.
    pub fn health(&self) -> HealthStatus {
        if !self.is_open() {
            return HealthStatus::with_detail(
                HealthState::Unavailable,
                "Codex app-server connection closed",
            );
        }
        let d = self.diagnostics();
        let served = lock(&self.inner.registry).len();
        let version = self.codex_version().unwrap_or_else(|| "unknown".to_owned());
        let mut detail = format!(
            "Codex app-server connected (Codex {version}, {}); {served} thread(s) served",
            shim::SHIM_LABEL
        );
        let state = if d.schema_drift > 0 || d.malformed > 0 {
            detail.push_str(&format!(
                "; {} schema drift, {} malformed message(s)",
                d.schema_drift, d.malformed
            ));
            HealthState::Degraded
        } else {
            HealthState::Healthy
        };
        HealthStatus::with_detail(state, detail)
    }

    /// Stop: no event is passed on after this returns, every waiting call fails with
    /// [`ClientError::Closed`], and a WebSocket close is sent. The reader thread ends when
    /// the stream does; `cli/`, which opened the stream, closes it.
    pub fn shutdown(&self) {
        self.inner.down.store(true, Ordering::SeqCst);
        self.inner.writer.close();
        self.inner.fail_pending();
    }
}

impl Inner {
    fn send(&self, frame: &str) -> Result<(), ClientError> {
        if self.down.load(Ordering::SeqCst) || !self.open.load(Ordering::SeqCst) {
            return Err(ClientError::Closed);
        }
        self.writer.send_text(frame).map_err(|e| match e {
            WsError::Closed => ClientError::Closed,
            other => ClientError::Connect(other),
        })
    }

    fn frame(id: u64, method: &str, params: &str) -> String {
        format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":{},\"params\":{params}}}",
            Json::String(method.to_owned()).to_compact()
        )
    }

    /// Send a call on the stable allowlist whose answer nobody waits for (its response is
    /// counted as stray).
    fn send_call(&self, method: &str, params: &str) -> Result<(), ClientError> {
        if !STABLE_METHODS.contains(&method) {
            return Err(ClientError::NotAllowed(method.to_owned()));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.send(&Inner::frame(id, method, params))
    }

    fn call(&self, method: &str, params: &str) -> Result<Json, ClientError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        lock(&self.pending).insert(id, tx);
        if let Err(e) = self.send(&Inner::frame(id, method, params)) {
            lock(&self.pending).remove(&id);
            return Err(e);
        }
        match rx.recv_timeout(self.timeout) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err((code, message))) => Err(ClientError::Refused { code, message }),
            Err(RecvTimeoutError::Timeout) => {
                lock(&self.pending).remove(&id);
                Err(ClientError::NoAnswer)
            }
            Err(RecvTimeoutError::Disconnected) => Err(ClientError::Closed),
        }
    }

    fn count_drift(&self, n: usize) {
        lock(&self.stats).schema_drift += n as u64;
    }

    fn fail_pending(&self) {
        // Dropping the senders wakes every waiting call with `Closed`.
        lock(&self.pending).clear();
    }

    fn closed(&self) {
        self.open.store(false, Ordering::SeqCst);
        self.fail_pending();
        if !self.down.load(Ordering::SeqCst) {
            (self.handler)(CodexEvent::Closed);
        }
    }

    fn dispatch(&self, text: &str) {
        let Ok(v) = json::parse(text.as_bytes()) else {
            lock(&self.stats).unparseable += 1;
            return;
        };
        let Some(o) = v.as_object() else {
            lock(&self.stats).unparseable += 1;
            return;
        };
        let method = o.get("method").and_then(Json::as_str);
        let id = o.get("id");
        match (method, id) {
            (Some(_), Some(_)) => {
                // A server-to-client request (an approval). Never answered: the decision is
                // the harness user's, and an error answer could be read as a decline.
                lock(&self.stats).server_requests += 1;
            }
            (Some(m), None) => self.notification(m, o.get("params")),
            (None, Some(id)) => self.response(id, o.get("result"), o.get("error")),
            (None, None) => lock(&self.stats).unparseable += 1,
        }
    }

    fn response(&self, id: &Json, result: Option<&Json>, error: Option<&Json>) {
        let Some(id) = id.as_number().and_then(|n| n.plain_integer_in(0, u64::MAX)) else {
            lock(&self.stats).unparseable += 1;
            return;
        };
        let reply = match (result, error) {
            (Some(r), None) => Ok(r.clone()),
            (None, Some(e)) => {
                let eo = e.as_object();
                let code = eo
                    .and_then(|o| o.get("code"))
                    .and_then(Json::as_number)
                    .and_then(|n| n.raw().parse::<i64>().ok())
                    .unwrap_or(0);
                let message = eo
                    .and_then(|o| o.get("message"))
                    .and_then(Json::as_str)
                    .unwrap_or_default()
                    .to_owned();
                Err((code, message))
            }
            _ => {
                lock(&self.stats).unparseable += 1;
                return;
            }
        };
        match lock(&self.pending).remove(&id) {
            Some(tx) => {
                let _ = tx.send(reply);
            }
            None => lock(&self.stats).stray_responses += 1,
        }
    }

    fn notification(&self, method: &str, params: Option<&Json>) {
        let t = {
            let mut reg = lock(&self.registry);
            events::translate(method, params, &mut reg)
        };
        {
            let mut s = lock(&self.stats);
            s.schema_drift += t.drift.len() as u64;
            match t.event {
                Err(Dropped::Delta) => s.deltas += 1,
                Err(Dropped::NotServed) => s.not_served += 1,
                Err(Dropped::Malformed) => s.malformed += 1,
                Err(Dropped::Unused) | Ok(_) => {}
            }
        }
        if let Ok(e) = t.event
            && !self.down.load(Ordering::SeqCst)
        {
            // No lock of the client's is held here (re-entrancy).
            (self.handler)(e);
        }
    }
}

/// True for a Codex thread id: UUIDv7 text (36 characters of hex digits and hyphens, as
/// `Thread.id` documents), so nothing else is ever sent as one.
pub fn is_thread_id(s: &str) -> bool {
    s.len() == 36
        && s.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// The Codex version in a `userAgent` such as `codex-tui/0.157.1 (Windows ...) ...`: the
/// part after the first `/` of the first word, when it is a plain version.
pub fn version_of_user_agent(ua: &str) -> Option<String> {
    let first = ua.split_whitespace().next()?;
    let (_, v) = first.split_once('/')?;
    let plain = !v.is_empty()
        && v.len() <= 32
        && v.starts_with(|c: char| c.is_ascii_digit())
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    plain.then(|| v.to_owned())
}

fn random_hex() -> String {
    let mut h = RandomState::new().build_hasher();
    h.write_u8(0);
    format!("{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_ids_are_uuid_text_only() {
        assert!(is_thread_id("01a1186d-b327-74f2-ac20-e3c5d0a9e81c"));
        for bad in [
            "",
            "01a1186d-b327-74f2-ac20-e3c5d0a9e81",
            "01a1186d_b327-74f2-ac20-e3c5d0a9e81c",
            "01a1186d-b327-74f2-ac20-e3c5d0a9e81g",
            "\"},\"model\":\"x\",\"a\":\"0000000000000",
        ] {
            assert!(!is_thread_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_version_comes_from_the_user_agent_only() {
        assert_eq!(
            version_of_user_agent(
                "codex-tui/0.157.1 (Windows 10.0.26200; x86_64) unknown (oac_d6_spike; 0.0.1)"
            )
            .as_deref(),
            Some("0.157.1")
        );
        assert_eq!(version_of_user_agent("codex"), None);
        assert_eq!(version_of_user_agent("x/../../etc"), None);
    }

    #[test]
    fn steering_methods_are_not_on_any_list() {
        for m in ["turn/start", "turn/steer", "thread/list", "thread/start"] {
            assert!(!STABLE_METHODS.contains(&m), "{m}");
            assert!(!shim::EXPERIMENTAL_METHODS.contains(&m), "{m}");
            assert!(!NOTIFICATIONS_SENT.contains(&m), "{m}");
        }
    }

    #[test]
    fn errors_never_print_a_refusal_message() {
        let e = ClientError::Refused {
            code: -32600,
            message: "session 01a1186d-b327-74f2-ac20-e3c5d0a9e81c is archived".into(),
        };
        assert!(!format!("{e}").contains("01a1186d"));
        assert!(!format!("{e:?}").contains("01a1186d"));
    }
}
