// SPDX-License-Identifier: Apache-2.0

//! The daemon's control endpoint and the client of it (G10, #71; lead approval 2026-10-18,
//! `docs/planning/decisions/C2-process-model.md` §4/§6): the one way `oac stop` tells a
//! running daemon to stop, over the reserved `control` label of the local IPC.
//!
//! **What it is.** The control endpoint is served under the same lock, through the same
//! `Endpoint::serve` / `Listener::accept` as a harness endpoint, so the owner/DACL check, the
//! Unix modes and `SO_PEERCRED` check and the client's server check all apply unchanged. It
//! is not a harness: **a control connection is never handed to `Pipelines::connect_observed`**.
//! It has no adapter and no session, so it cannot send or receive a message, and nothing a
//! peer writes to it reaches the core.
//!
//! **Protocol** (internal to `cli/`; not a part of the Session Channels specification). A
//! request is one JSON line, `{"oac_control":1,"op":"stop"}`; every reply is a JSON line.
//! Bounds: a request is at most [`MAX_REQUEST`] bytes and must arrive within
//! [`READ_DEADLINE`]; at most [`MAX_CONNECTIONS`] control connections are served at once
//! (a further one is answered `busy` and closed). Scalability beyond that is measured in #438.
//!
//! **Stop.** The daemon answers `{"oac_control":1,"pid":<pid>,"stopping":true}` and signals
//! its owner ([`Control::stop_requested`]); the owner runs the daemon's shutdown (endpoints
//! stop accepting, presence is withdrawn, the transport shuts down, the lock is dropped),
//! and only then [`Control::finish`] writes `{"stopped":true}` to each waiting requester and
//! closes it. So a requester that reads `stopped` may start a new daemon at once. The waiting
//! streams are held here, not in the daemon's task list, which shutdown awaits: a waiter held
//! there would deadlock the shutdown it waits for.
//!
//! **Status** (G10, #71, slice 3). `{"oac_control":1,"op":"status"}` is answered with one JSON
//! line, `{"oac_control":1,"status":{...}}` (members: `crate::status`), and the connection is
//! closed. The snapshot is the core's ([`oac_core::pipeline::Pipelines::status`]) plus what
//! the daemon knows of itself; it holds no harness-native id and no working directory. It is
//! computed on a detached thread of its own (not the runtime's blocking pool, whose tasks a
//! runtime waits for when it is dropped, so a `health` that never returns could keep the
//! process alive after `oac stop`), at most one at a time, and within [`STATUS_DEADLINE`]: an
//! adapter or a transport whose `health` hangs is answered `timeout`, not waited for. While a
//! snapshot is still within its deadline a further request is answered `busy` (and retried by
//! the client); once the one in flight is past its deadline every request is answered
//! `timeout` at once, so there is never more than one stuck thread.
//!
//! A same-user process can already end the daemon by killing it, so this adds no authority
//! the user's processes did not have; it only makes the stop clean (`RISK-LOCAL-IPC`).

use std::future::Future;
use std::io::{self, BufRead, Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oac_core::canonical::canonical;
use oac_core::json::{self, Json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

use crate::daemon::LogSink;
use crate::ipc::{Accept, AsyncStream, ConnectError, Endpoint, Listener};
use crate::status::DaemonStatus;

/// The version of this protocol.
pub const VERSION: u64 = 1;
/// The longest request line, in bytes (the newline excluded).
pub const MAX_REQUEST: usize = 1024;
/// How long a connection has to deliver its request line.
pub const READ_DEADLINE: Duration = Duration::from_secs(2);
/// How long a reply write may take before the connection is dropped.
const WRITE_DEADLINE: Duration = Duration::from_secs(2);
/// The most control connections served at once.
pub const MAX_CONNECTIONS: usize = 4;

/// How long the daemon computes a status snapshot before it answers `timeout`.
pub const STATUS_DEADLINE: Duration = Duration::from_secs(5);
/// The longest status reply the client reads, in bytes.
const MAX_STATUS_REPLY: u64 = 1 << 20;

/// The request `oac status` sends.
pub const STATUS_REQUEST: &str = "{\"oac_control\":1,\"op\":\"status\"}\n";
/// The request `oac stop` sends.
pub const STOP_REQUEST: &str = "{\"oac_control\":1,\"op\":\"stop\"}\n";
/// The reply that confirms the daemon has finished stopping.
pub const STOPPED_REPLY: &str = "{\"stopped\":true}\n";
const BUSY_REPLY: &str = "{\"error\":\"busy\",\"oac_control\":1}\n";
const TOO_LARGE_REPLY: &str = "{\"error\":\"too_large\",\"oac_control\":1}\n";
const MALFORMED_REPLY: &str = "{\"error\":\"malformed\",\"oac_control\":1}\n";
const UNKNOWN_OP_REPLY: &str = "{\"error\":\"unknown_op\",\"oac_control\":1}\n";
const TIMEOUT_REPLY: &str = "{\"error\":\"timeout\",\"oac_control\":1}\n";
const VERSION_REPLY: &str = "{\"error\":\"version\",\"oac_control\":1,\"supported\":1}\n";

/// A request the daemon understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Stop the daemon cleanly.
    Stop,
    /// Answer with a status snapshot.
    Status,
}

/// Why a request is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The line is longer than [`MAX_REQUEST`].
    TooLarge,
    /// Not a JSON object with an integer `oac_control` and a string `op`.
    Malformed,
    /// A protocol version this daemon does not speak.
    Version,
    /// An `op` this daemon does not know.
    UnknownOp,
}

impl Refusal {
    fn reply(self) -> &'static str {
        match self {
            Refusal::TooLarge => TOO_LARGE_REPLY,
            Refusal::Malformed => MALFORMED_REPLY,
            Refusal::Version => VERSION_REPLY,
            Refusal::UnknownOp => UNKNOWN_OP_REPLY,
        }
    }
}

/// Reads a request line (no newline) as a [`Request`].
///
/// # Errors
///
/// See [`Refusal`].
pub fn parse_request(line: &[u8]) -> Result<Request, Refusal> {
    if line.len() > MAX_REQUEST {
        return Err(Refusal::TooLarge);
    }
    let value = json::parse(line).map_err(|_| Refusal::Malformed)?;
    let object = value.as_object().ok_or(Refusal::Malformed)?;
    let version = object
        .get("oac_control")
        .and_then(Json::as_number)
        .and_then(|n| n.plain_integer_in(0, u64::MAX))
        .ok_or(Refusal::Malformed)?;
    if version != VERSION {
        return Err(Refusal::Version);
    }
    match object.get("op").and_then(Json::as_str) {
        Some("stop") => Ok(Request::Stop),
        Some("status") => Ok(Request::Status),
        Some(_) => Err(Refusal::UnknownOp),
        None => Err(Refusal::Malformed),
    }
}

/// How reading a request ended without one.
enum ReadEnd {
    /// The peer closed before sending anything (a liveness probe): nothing to say.
    Silent,
    /// The deadline passed.
    Timeout,
    /// A refusal to answer.
    Refused(Refusal),
}

/// Reads one request line, bounded in size and time.
async fn read_request(stream: &mut dyn AsyncStream) -> Result<Vec<u8>, ReadEnd> {
    let read = async {
        let mut data: Vec<u8> = Vec::with_capacity(128);
        let mut chunk = [0u8; 256];
        loop {
            let n = stream.read(&mut chunk).await.map_err(|_| ReadEnd::Silent)?;
            if n == 0 {
                return Err(if data.is_empty() {
                    ReadEnd::Silent
                } else {
                    ReadEnd::Refused(Refusal::Malformed)
                });
            }
            data.extend_from_slice(&chunk[..n]);
            match data.iter().position(|b| *b == b'\n') {
                Some(i) if i <= MAX_REQUEST => {
                    data.truncate(i);
                    return Ok(data);
                }
                Some(_) => return Err(ReadEnd::Refused(Refusal::TooLarge)),
                None if data.len() > MAX_REQUEST => {
                    return Err(ReadEnd::Refused(Refusal::TooLarge));
                }
                None => {}
            }
        }
    };
    match tokio::time::timeout(READ_DEADLINE, read).await {
        Ok(result) => result,
        Err(_) => Err(ReadEnd::Timeout),
    }
}

/// Writes `text` and flushes, within [`WRITE_DEADLINE`]; whether it was written.
async fn write_line(stream: &mut dyn AsyncStream, text: &str) -> bool {
    let write = async {
        stream.write_all(text.as_bytes()).await?;
        stream.flush().await
    };
    matches!(
        tokio::time::timeout(WRITE_DEADLINE, write).await,
        Ok(Ok(()))
    )
}

struct Waiters {
    /// Set once the daemon has stopped: a later requester is answered at once.
    closed: bool,
    streams: Vec<(Box<dyn AsyncStream>, OwnedSemaphorePermit)>,
}

/// The state of the control endpoint of one daemon.
pub struct Control {
    permits: Arc<Semaphore>,
    stop: watch::Sender<bool>,
    waiters: Mutex<Waiters>,
    /// One status snapshot is computed at a time.
    /// When the snapshot in flight began; `None` when none is.
    status_inflight: Arc<Mutex<Option<Instant>>>,
    status_deadline: Duration,
    status: Arc<DaemonStatus>,
    log: LogSink,
}

impl Control {
    /// A control endpoint's state.
    pub fn new(log: LogSink, status: Arc<DaemonStatus>, status_deadline: Duration) -> Arc<Control> {
        Arc::new(Control {
            permits: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
            stop: watch::channel(false).0,
            waiters: Mutex::new(Waiters {
                closed: false,
                streams: Vec::new(),
            }),
            status_inflight: Arc::new(Mutex::new(None)),
            status_deadline,
            status,
            log,
        })
    }

    /// A future that completes when a client has asked the daemon to stop. The owner selects
    /// on it beside its signals, then runs the daemon's shutdown.
    pub fn stop_requested(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut requested = self.stop.subscribe();
        async move {
            // A closed channel means the daemon is gone; either way the owner stops waiting.
            let _ = requested.wait_for(|asked| *asked).await;
        }
    }

    /// Serves control connections until `stopped` changes. Each is authenticated by the
    /// local IPC before it is accepted; none reaches the core.
    pub async fn accept_loop(
        self: Arc<Self>,
        mut listener: Listener,
        mut stopped: watch::Receiver<bool>,
    ) {
        loop {
            let accepted = tokio::select! {
                a = listener.accept() => a,
                _ = stopped.changed() => return,
            };
            match accepted {
                Ok(Accept::Connection(connection)) => {
                    let mut stream = connection.stream;
                    match self.permits.clone().try_acquire_owned() {
                        Ok(permit) => {
                            let control = self.clone();
                            tokio::spawn(async move { control.serve(stream, permit).await });
                        }
                        Err(_) => {
                            self.log
                                .line("refused a control connection: too many at once");
                            tokio::spawn(async move {
                                write_line(&mut *stream, BUSY_REPLY).await;
                            });
                        }
                    }
                }
                Ok(Accept::Rejected(r)) => {
                    self.log
                        .line(&format!("refused a connection on control: {}", r.reason));
                }
                Err(e) => {
                    self.log
                        .line(&format!("the control endpoint can no longer accept: {e}"));
                    return;
                }
            }
        }
    }

    async fn serve(
        self: Arc<Self>,
        mut stream: Box<dyn AsyncStream>,
        permit: OwnedSemaphorePermit,
    ) {
        let refusal = match read_request(&mut *stream).await {
            Ok(line) => match parse_request(&line) {
                Ok(Request::Stop) => return self.stop_requested_by(stream, permit).await,
                Ok(Request::Status) => return self.status_requested_by(stream, permit).await,
                Err(refusal) => refusal,
            },
            Err(ReadEnd::Silent) => return,
            Err(ReadEnd::Timeout) => {
                self.log
                    .line("closed a control connection: no request in time");
                return;
            }
            Err(ReadEnd::Refused(refusal)) => refusal,
        };
        self.log
            .line(&format!("refused a control request: {refusal:?}"));
        write_line(&mut *stream, refusal.reply()).await;
    }

    async fn status_requested_by(
        &self,
        mut stream: Box<dyn AsyncStream>,
        _permit: OwnedSemaphorePermit,
    ) {
        let (done, result) = tokio::sync::oneshot::channel::<String>();
        let refusal = {
            let mut inflight = self
                .status_inflight
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            match *inflight {
                Some(began) if began.elapsed() >= self.status_deadline => Some(TIMEOUT_REPLY),
                Some(_) => Some(BUSY_REPLY),
                None => {
                    *inflight = Some(Instant::now());
                    None
                }
            }
        };
        if let Some(reply) = refusal {
            write_line(&mut *stream, reply).await;
            return;
        }
        let (status, inflight) = (self.status.clone(), self.status_inflight.clone());
        // A detached thread: if `health` never returns it is left behind, and the process can
        // still end. It marks itself done only when it really ends.
        let spawned = std::thread::Builder::new()
            .name("oac-status".into())
            .spawn(move || {
                // Cleared when the thread ends however it ends: a `health` that panics must
                // not leave every later request answered `timeout`.
                struct Clear(Arc<Mutex<Option<Instant>>>);
                impl Drop for Clear {
                    fn drop(&mut self) {
                        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    }
                }
                let _clear = Clear(inflight);
                let _ = done.send(status.reply());
            });
        if spawned.is_err() {
            *self
                .status_inflight
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
        }
        let reply = match spawned {
            Ok(_) => match tokio::time::timeout(self.status_deadline, result).await {
                Ok(Ok(text)) => text,
                _ => String::new(),
            },
            Err(_) => String::new(),
        };
        let reply = if reply.is_empty() {
            self.log.line(
                "a status request was not answered in time (the transport or an adapter did not report its health)",
            );
            TIMEOUT_REPLY.to_owned()
        } else {
            reply
        };
        write_line(&mut *stream, &reply).await;
    }

    async fn stop_requested_by(
        &self,
        mut stream: Box<dyn AsyncStream>,
        permit: OwnedSemaphorePermit,
    ) {
        let stopping = format!(
            "{{\"oac_control\":1,\"pid\":{},\"stopping\":true}}\n",
            std::process::id()
        );
        if !write_line(&mut *stream, &stopping).await {
            return;
        }
        {
            let mut waiters = self.waiters.lock().unwrap_or_else(|e| e.into_inner());
            if !waiters.closed {
                waiters.streams.push((stream, permit));
                self.log.line("stopping (requested over the local IPC)");
                self.stop.send_replace(true);
                return;
            }
        }
        // The daemon has already stopped (a request that raced the shutdown): say so.
        write_line(&mut *stream, STOPPED_REPLY).await;
    }

    /// Writes `stopped` to every waiting requester and closes it. Call after the daemon has
    /// shut down and released its lock; a requester that arrives later is answered at once.
    pub async fn finish(&self) {
        let waiting = {
            let mut waiters = self.waiters.lock().unwrap_or_else(|e| e.into_inner());
            waiters.closed = true;
            std::mem::take(&mut waiters.streams)
        };
        for (mut stream, _permit) in waiting {
            if write_line(&mut *stream, STOPPED_REPLY).await {
                let _ = stream.shutdown().await;
            }
        }
    }
}

// ---- the client ---------------------------------------------------------------------------

/// How `oac stop` ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopOutcome {
    /// No daemon serves the control endpoint.
    NotRunning,
    /// The daemon confirmed it had finished stopping.
    Stopped {
        /// The daemon's process id, as it reported it.
        pid: Option<u32>,
    },
    /// The daemon accepted the request and did not confirm in time. Nothing was killed.
    TimedOut {
        /// The daemon's process id, as it reported it.
        pid: Option<u32>,
    },
    /// The daemon speaks another protocol version.
    Version,
    /// Every control place of the daemon stayed in use until the deadline.
    Busy,
    /// The endpoint is served by another user's process, or every instance stayed busy, or
    /// the operating system refused: nothing was sent.
    Unreachable(String),
    /// The daemon answered, and the stop did not complete.
    Failed(String),
}

enum Line {
    Text(String),
    Closed,
}

/// Asks the daemon at `endpoint` to stop and waits at most `wait` for it to say it has.
/// Never kills anything.
///
/// A daemon that answers `busy` (four control connections are in use) is asked again until
/// the deadline; only a daemon still busy then is [`StopOutcome::Busy`].
pub fn request_stop(endpoint: &Endpoint, wait: Duration) -> StopOutcome {
    let deadline = Instant::now() + wait;
    let mut was_busy = false;
    loop {
        match attempt(endpoint, deadline) {
            StopOutcome::Busy if Instant::now() < deadline => {
                was_busy = true;
                std::thread::sleep(Duration::from_millis(100));
            }
            // The deadline passed while a retry waited for its first answer: the daemon was
            // busy as far as we know.
            StopOutcome::TimedOut { pid: None } if was_busy => return StopOutcome::Busy,
            other => return other,
        }
    }
}

fn attempt(endpoint: &Endpoint, deadline: Instant) -> StopOutcome {
    let mut stream = match endpoint.connect_blocking() {
        Ok(s) => s,
        Err(ConnectError::NotRunning) => return StopOutcome::NotRunning,
        Err(e) => return StopOutcome::Unreachable(e.to_string()),
    };
    if let Err(e) = stream
        .write_all(STOP_REQUEST.as_bytes())
        .and_then(|()| stream.flush())
    {
        return StopOutcome::Failed(format!("the request could not be sent: {e}"));
    }
    // A synchronous Windows pipe handle has no read timeout: the reads are on a thread, and
    // the wait is on the channel. The total read is capped, so a daemon cannot feed it forever.
    let (tx, rx) = std::sync::mpsc::channel::<Line>();
    let spawned = std::thread::Builder::new()
        .name("oac-stop-read".into())
        .spawn(move || {
            let mut reader = io::BufReader::new(stream.take(8192));
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(Line::Closed);
                        return;
                    }
                    Ok(_) => {
                        if tx.send(Line::Text(line)).is_err() {
                            return;
                        }
                    }
                }
            }
        });
    if let Err(e) = spawned {
        return StopOutcome::Failed(format!("the reply could not be read: {e}"));
    }
    let mut pid: Option<u32> = None;
    let mut stopping = false;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Err(_) => return StopOutcome::TimedOut { pid },
            Ok(Line::Closed) => {
                return StopOutcome::Failed(if stopping {
                    "the daemon closed the connection before it confirmed it had stopped".to_owned()
                } else {
                    "the daemon closed the connection without an answer".to_owned()
                });
            }
            Ok(Line::Text(text)) => {
                let Some(value) = json::parse(text.trim_end().as_bytes())
                    .ok()
                    .and_then(Json::into_object)
                else {
                    return StopOutcome::Failed(
                        "the daemon's answer was not understood".to_owned(),
                    );
                };
                if let Some(error) = value.get("error").and_then(Json::as_str) {
                    return match error {
                        "version" => StopOutcome::Version,
                        "busy" => StopOutcome::Busy,
                        other => StopOutcome::Failed(format!("the daemon refused: {other}")),
                    };
                }
                if value.get("stopped").and_then(Json::as_bool) == Some(true) {
                    return StopOutcome::Stopped { pid };
                }
                if value.get("stopping").and_then(Json::as_bool) == Some(true) {
                    stopping = true;
                    pid = value
                        .get("pid")
                        .and_then(Json::as_number)
                        .and_then(|n| n.plain_integer_in(0, u64::from(u32::MAX)))
                        .and_then(|p| u32::try_from(p).ok());
                }
            }
        }
    }
}

/// How `oac status` ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusOutcome {
    /// No daemon serves the control endpoint.
    NotRunning,
    /// The daemon answered: the members of its `status` object.
    Running(json::JsonObject),
    /// The daemon speaks another protocol version.
    Version,
    /// Every control place of the daemon stayed in use until the deadline.
    Busy,
    /// The endpoint is served by another user's process, or the operating system refused:
    /// nothing was sent.
    Unreachable(String),
    /// The daemon answered, and not with a status.
    Failed(String),
}

/// Asks the daemon at `endpoint` for its status and waits at most `wait` for the answer.
///
/// A daemon that answers `busy` (four control connections are in use, or a snapshot is
/// being computed) is asked again until the deadline; only a daemon still busy then is
/// [`StatusOutcome::Busy`].
pub fn request_status(endpoint: &Endpoint, wait: Duration) -> StatusOutcome {
    let deadline = Instant::now() + wait;
    let mut was_busy = false;
    loop {
        match attempt_status(endpoint, deadline, was_busy) {
            StatusOutcome::Busy if Instant::now() < deadline => {
                was_busy = true;
                std::thread::sleep(Duration::from_millis(100));
            }
            other => return other,
        }
    }
}

fn attempt_status(endpoint: &Endpoint, deadline: Instant, was_busy: bool) -> StatusOutcome {
    let mut stream = match endpoint.connect_blocking() {
        Ok(s) => s,
        Err(ConnectError::NotRunning) => return StatusOutcome::NotRunning,
        Err(ConnectError::Busy) => return StatusOutcome::Busy,
        Err(e) => return StatusOutcome::Unreachable(e.to_string()),
    };
    if let Err(e) = stream
        .write_all(STATUS_REQUEST.as_bytes())
        .and_then(|()| stream.flush())
    {
        return StatusOutcome::Failed(format!("the request could not be sent: {e}"));
    }
    // As in `attempt`: a synchronous Windows pipe handle has no read timeout, so the read is
    // on a thread, the wait is on the channel, and the read is capped.
    let (tx, rx) = std::sync::mpsc::channel::<Option<String>>();
    let spawned = std::thread::Builder::new()
        .name("oac-status-read".into())
        .spawn(move || {
            let mut reader = io::BufReader::new(stream.take(MAX_STATUS_REPLY));
            let mut line = String::new();
            let _ = match reader.read_line(&mut line) {
                Ok(0) | Err(_) => tx.send(None),
                Ok(_) => tx.send(Some(line)),
            };
        });
    if let Err(e) = spawned {
        return StatusOutcome::Failed(format!("the reply could not be read: {e}"));
    }
    let left = deadline.saturating_duration_since(Instant::now());
    let text = match rx.recv_timeout(left) {
        // The deadline passed while a retry waited for its first answer: the daemon was busy
        // as far as we know.
        Err(_) if was_busy => return StatusOutcome::Busy,
        Err(_) => {
            return StatusOutcome::Failed("the daemon did not answer in time".to_owned());
        }
        Ok(None) => {
            return StatusOutcome::Failed(
                "the daemon closed the connection without an answer".to_owned(),
            );
        }
        Ok(Some(text)) => text,
    };
    let Some(value) = json::parse(text.trim_end().as_bytes())
        .ok()
        .and_then(Json::into_object)
    else {
        return StatusOutcome::Failed("the daemon's answer was not understood".to_owned());
    };
    if let Some(error) = value.get("error").and_then(Json::as_str) {
        return match error {
            "version" => StatusOutcome::Version,
            "busy" => StatusOutcome::Busy,
            "timeout" => {
                StatusOutcome::Failed("the daemon could not collect its status in time".to_owned())
            }
            other => StatusOutcome::Failed(format!("the daemon refused: {other}")),
        };
    }
    match value.get("status").and_then(Json::as_object) {
        Some(status) => StatusOutcome::Running(status.clone()),
        None => StatusOutcome::Failed("the daemon's answer was not a status".to_owned()),
    }
}

/// Canonical (RFC 8785) text of the JSON object `text`; `text` itself when it is not one.
pub fn canonical_text(text: &str) -> String {
    json::parse(text.as_bytes())
        .ok()
        .and_then(|v| canonical(&v).ok())
        .unwrap_or_else(|| text.to_owned())
}

/// A character that could drive a terminal or reorder text shown to an operator: DEL, the C1
/// controls and the bidirectional overrides and isolates.
pub fn is_hostile(c: char) -> bool {
    matches!(c as u32, 0x7f..=0x9f | 0x61c | 0x200e | 0x200f | 0x202a..=0x202e | 0x2066..=0x2069)
}

/// `text` as a JSON string literal.
pub fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || is_hostile(c) => {
                out.push_str(&format!("\\u{:04x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_constant_lines_are_canonical_json() {
        for line in [
            STOP_REQUEST,
            STATUS_REQUEST,
            TIMEOUT_REPLY,
            STOPPED_REPLY,
            BUSY_REPLY,
            TOO_LARGE_REPLY,
            MALFORMED_REPLY,
            UNKNOWN_OP_REPLY,
            VERSION_REPLY,
        ] {
            assert!(line.ends_with('\n'));
            let text = line.trim_end();
            assert_eq!(canonical_text(text), text, "{text}");
            assert!(json::parse(text.as_bytes()).is_ok(), "{text}");
        }
    }

    #[test]
    fn a_stop_request_parses_and_everything_else_is_refused() {
        assert_eq!(
            parse_request(STOP_REQUEST.trim_end().as_bytes()),
            Ok(Request::Stop)
        );
        assert_eq!(
            parse_request(STATUS_REQUEST.trim_end().as_bytes()),
            Ok(Request::Status)
        );
        // Members this version does not know are ignored.
        assert_eq!(
            parse_request(br#"{"oac_control":1,"op":"stop","x":[1]}"#),
            Ok(Request::Stop)
        );
        let cases: [(&[u8], Refusal); 10] = [
            (b"", Refusal::Malformed),
            (b"garbage", Refusal::Malformed),
            (b"[]", Refusal::Malformed),
            (b"{}", Refusal::Malformed),
            (br#"{"oac_control":"1","op":"stop"}"#, Refusal::Malformed),
            (br#"{"oac_control":1}"#, Refusal::Malformed),
            (br#"{"oac_control":1,"op":3}"#, Refusal::Malformed),
            (
                br#"{"oac_control":1,"oac_control":1,"op":"stop"}"#,
                Refusal::Malformed,
            ),
            (br#"{"oac_control":2,"op":"stop"}"#, Refusal::Version),
            (br#"{"oac_control":1,"op":"restart"}"#, Refusal::UnknownOp),
        ];
        for (line, why) in cases {
            assert_eq!(
                parse_request(line),
                Err(why),
                "{:?}",
                String::from_utf8_lossy(line)
            );
        }
        assert_eq!(
            parse_request(&[b'x'; MAX_REQUEST + 1]),
            Err(Refusal::TooLarge)
        );
    }

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(json_string("a\"b\\c\n"), "\"a\\\"b\\\\c\\u000a\"");
        assert_eq!(
            json_string("\u{7f}\u{85}\u{202e}\u{2066}x"),
            "\"\\u007f\\u0085\\u202e\\u2066x\""
        );
    }
}
