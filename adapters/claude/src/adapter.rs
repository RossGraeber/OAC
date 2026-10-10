// SPDX-License-Identifier: Apache-2.0

//! [`ClaudeAdapter`]: `ProviderAdapter` (`spec/interfaces.md` Table 5.2) for the Claude Code
//! channel path.
//!
//! Each connection the core gives it ([`ProviderAdapter::take_connection`]) is one channel
//! server's stdio stream, relayed by the shim Claude Code spawned
//! (`docs/planning/decisions/C2-process-model.md` §1). One connection is one attachment, and
//! one attachment presents exactly one channel: the adapter never multiplexes logical
//! channels over one server (whether Claude Code supports that is UNVERIFIED, 11-risks
//! row 2, so nothing here depends on it).
//!
//! Each connection runs its MCP server ([`crate::server`]) on its own thread, with a
//! current-thread `tokio` runtime. Everything inbound is pushed: the harness's frames arrive
//! on the connection, and hand-offs are notifications the server writes. Nothing here asks
//! the harness, or anything else, for pending messages ([IFC-ADP-040]; no polling of any
//! kind).
//!
//! Known residual (lead decision, 2026-10-10; 11-risks row 82): shutdown waits for
//! in-flight writes. The frozen `Connection` supplies a blocking `Write` without
//! cancel/close, so shutdown can block while the harness has stopped reading. Returning
//! earlier could hand off content after shutdown returns, violating [IFC-ADP-070].
//! Owner: #376 (the connection-cancellation spec change).

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;

use oac_core::adapter::{
    AdapterCapabilities, AdapterEvent, AdapterEventHandler, Attachment, Connection,
    DiscoveryRequest, HandOff, HandOffOutcome, ProviderAdapter, RequestSink,
};
use oac_core::delivery::ErrorCode;
use oac_core::health::{HealthState, HealthStatus};
use oac_core::ids::SessionId;
use oac_mcp_tools::{ToolCall, Value};
use rmcp::model::{CallToolResult, CustomNotification, JsonObject, ServerNotification};
use rmcp::service::{Peer, RoleServer, RunningServiceCancellationToken};

use crate::channel::{
    CHANNEL_NOTIFICATION, META_KEYS, ProvenanceRefusal, channel_content, provenance_meta,
};
use crate::io::{ChannelReader, ChannelWriter};
use crate::server::{ChannelServer, ChannelTransport};

/// How long `deliver` waits for the notification's write to complete before it reports
/// `indeterminate` ([IFC-ADP-053]): the write neither completed nor failed in that time.
pub const HANDOFF_WAIT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// The MCP opening is under way: nothing is handed off.
    Opening,
    /// The channel is registered and listed; `attachment-opened` was reported.
    Open,
    /// Ended; `attachment-closed` was reported if it had opened.
    Closed,
}

struct Att {
    phase: Phase,
    bound: Option<SessionId>,
    peer: Option<Peer<RoleServer>>,
    runtime: Option<tokio::runtime::Handle>,
    cancel: Option<RunningServiceCancellationToken>,
    /// Hand-off attempts for a reply's `to`: message id, sender and recipient, for the last
    /// [`HANDED_KEPT`] messages this attachment was handed, oldest first. They only find the
    /// sender for a `reply` that names no `to`; ambiguous attempts require `to`.
    /// Only completed attempts can supply a destination; pending ones can make it ambiguous.
    /// The core still checks every reply target
    /// against its own records (`spec/session-channels.md` §8.2.2).
    handed: VecDeque<(String, SessionId, SessionId, bool)>,
    /// Once records are evicted, implicit resolution cannot exclude an older collision.
    reply_records_evicted: bool,
}

/// How many hand-off records an attachment keeps for `reply`. Older ones are dropped, so a
/// peer cannot grow the adapter's memory without bound. After any eviction all replies need
/// `to`, since a retained id could collide with an evicted sender's id.
pub const HANDED_KEPT: usize = 1024;

#[derive(Default)]
struct State {
    handler: Option<AdapterEventHandler>,
    sink: Option<Arc<dyn RequestSink>>,
    attachments: HashMap<Attachment, Att>,
    connections: HashMap<
        std::thread::ThreadId,
        (
            tokio::sync::watch::Sender<bool>,
            std::thread::JoinHandle<()>,
        ),
    >,
}

/// What every connection thread shares with the adapter.
pub(crate) struct Inner {
    state: Mutex<State>,
    /// Held while an event is reported, so that an attachment's `attachment-opened` always
    /// reaches the core before its `attachment-closed`. No operation the core may call from
    /// inside the handler (`set_binding`, `capabilities`) takes it.
    events: Mutex<()>,
    /// `true` once `shutdown` has begun. A hand-off or request holds the read side for its
    /// whole call. Shutdown also joins connection runtimes, including writes that outlived
    /// a deliver timeout, before returning ([IFC-ADP-070]).
    down: RwLock<bool>,
    /// How long `deliver` waits for a write ([`HANDOFF_WAIT`] unless set).
    handoff_wait: Duration,
    /// Serializes shutdown callers through the final connection-thread joins.
    shutdown: Mutex<()>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Inner {
    fn emit(&self, h: Option<AdapterEventHandler>, e: AdapterEvent) {
        if let Some(h) = h {
            h(e);
        }
    }

    /// The first `tools/list` answer is written: the attachment is open.
    pub(crate) fn listed(&self, a: &Attachment) {
        let _ev = lock(&self.events);
        let h = {
            if *self.down.read().unwrap_or_else(|e| e.into_inner()) {
                return;
            }
            let mut st = lock(&self.state);
            let Some(att) = st.attachments.get_mut(a) else {
                return;
            };
            if att.phase != Phase::Opening {
                return;
            }
            att.phase = Phase::Open;
            st.handler.clone()
        };
        self.emit(
            h,
            AdapterEvent::AttachmentOpened {
                attachment: a.clone(),
                cross_check: None,
            },
        );
    }

    /// The connection ended.
    fn ended(&self, a: &Attachment) {
        let _ev = lock(&self.events);
        let h = {
            let mut st = lock(&self.state);
            let was_open = st
                .attachments
                .remove(a)
                .is_some_and(|att| att.phase == Phase::Open);
            if !was_open {
                return;
            }
            st.handler.clone()
        };
        self.emit(
            h,
            AdapterEvent::AttachmentClosed {
                attachment: a.clone(),
            },
        );
    }

    fn running(
        &self,
        a: &Attachment,
        peer: Peer<RoleServer>,
        runtime: tokio::runtime::Handle,
        cancel: RunningServiceCancellationToken,
    ) {
        let mut st = lock(&self.state);
        if let Some(att) = st.attachments.get_mut(a) {
            att.peer = Some(peer);
            att.runtime = Some(runtime);
            att.cancel = Some(cancel);
        } else {
            // Shut down meanwhile.
            cancel.cancel();
        }
    }

    /// A tool call from the harness on attachment `a`, passed to the core's sink unchanged
    /// and answered with the core's result unchanged ([IFC-ADP-003], [IFC-ADP-060]). The
    /// request is labelled with the connection it came on, never with a session the call
    /// claims ([IFC-ADP-031]).
    pub(crate) fn request(&self, a: &Attachment, call: ToolCall) -> CallToolResult {
        let down = self.down.read().unwrap_or_else(|e| e.into_inner());
        if *down {
            // Answered without reaching the core ([IFC-ADP-070]).
            return oac_mcp_tools::refusal(ErrorCode::InternalError);
        }
        let (sink, bound, record) = {
            let st = lock(&self.state);
            let att = st.attachments.get(a);
            let record = match &call {
                ToolCall::Reply { in_reply_to, .. } => att.and_then(|x| {
                    if x.reply_records_evicted {
                        return None;
                    }
                    let mut senders = x
                        .handed
                        .iter()
                        .filter(|(id, _, to, _)| id == in_reply_to && Some(to) == x.bound.as_ref())
                        .map(|(_, from, _, completed)| (from, *completed));
                    let (first, mut completed) = senders.next()?;
                    // IDs are unique only per sender [SC-ENV-027]. Never choose the
                    // newest sender: ambiguity requires an explicit destination.
                    let unambiguous = senders.all(|(from, handed)| {
                        completed |= handed;
                        from == first
                    });
                    (unambiguous && completed).then(|| first.clone())
                }),
                _ => None,
            };
            (st.sink.clone(), att.and_then(|x| x.bound.clone()), record)
        };
        let Some(sink) = sink else {
            return oac_mcp_tools::refusal(ErrorCode::InternalError);
        };
        match call {
            ToolCall::Send {
                to,
                content,
                conversation_id,
                correlation_id,
            } => oac_mcp_tools::send_result(&sink.send(oac_mcp_tools::send_request(
                a.clone(),
                to,
                content,
                conversation_id,
                correlation_id,
            ))),
            ToolCall::Reply {
                in_reply_to,
                to,
                content,
            } => {
                // Attribution refusal precedes implicit destination resolution
                // [MCPB-ATT-002], [SC-RCP-090]. Explicit requests still go to the core.
                if to.is_none() && bound.is_none() {
                    return oac_mcp_tools::refusal(ErrorCode::Unauthorized);
                }
                match to.or(record) {
                    Some(to) => oac_mcp_tools::send_result(&sink.send(
                        oac_mcp_tools::reply_request(a.clone(), to, content, in_reply_to),
                    )),
                    // No `to`, and no record of the message on this attachment.
                    None => oac_mcp_tools::refusal(ErrorCode::InvalidRequest),
                }
            }
            ToolCall::ListSessions => {
                oac_mcp_tools::discovery_result(&sink.discover(DiscoveryRequest {
                    attachment: a.clone(),
                }))
            }
            // [MCPB-ATT-002]: no session to name on an unbound attachment. On a bound one
            // the device fingerprint is not something the frozen adapter interface gives
            // the adapter; `whoami` waits for session registration (G5, #66).
            ToolCall::Whoami => oac_mcp_tools::refusal(if bound.is_none() {
                ErrorCode::Unauthorized
            } else {
                ErrorCode::InternalError
            }),
        }
    }
}

/// The Claude Code channel adapter.
pub struct ClaudeAdapter {
    inner: Arc<Inner>,
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        ClaudeAdapter::new()
    }
}

impl ClaudeAdapter {
    /// An adapter with no connection yet.
    pub fn new() -> ClaudeAdapter {
        ClaudeAdapter::with_handoff_wait(HANDOFF_WAIT)
    }

    /// An adapter whose `deliver` waits `wait` for a notification's write before it
    /// reports `indeterminate` ([IFC-ADP-053]).
    pub fn with_handoff_wait(wait: Duration) -> ClaudeAdapter {
        ClaudeAdapter {
            inner: Arc::new(Inner {
                state: Mutex::default(),
                events: Mutex::new(()),
                down: RwLock::new(false),
                handoff_wait: wait,
                shutdown: Mutex::new(()),
            }),
        }
    }
}

/// Serves one connection until it ends, then reports it closed.
fn serve(
    inner: Arc<Inner>,
    attachment: Attachment,
    reader: Box<dyn std::io::Read + Send>,
    writer: Box<dyn std::io::Write + Send>,
    mut stop: tokio::sync::watch::Receiver<bool>,
) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build();
    if let (Ok(runtime), Ok(read)) = (runtime, ChannelReader::spawn(reader)) {
        let handle = runtime.handle().clone();
        runtime.block_on(async {
            let transport = ChannelTransport::new(
                read,
                ChannelWriter(writer),
                inner.clone(),
                attachment.clone(),
            );
            let server = ChannelServer {
                inner: inner.clone(),
                attachment: attachment.clone(),
            };
            // A connection can be stopped even while its MCP opening waits for input.
            let opening = tokio::select! {
                result = rmcp::serve_server(server, transport) => Some(result),
                _ = stop.changed() => None,
            };
            if let Some(Ok(running)) = opening {
                inner.running(
                    &attachment,
                    running.peer().clone(),
                    handle,
                    running.cancellation_token(),
                );
                let cancel = running.cancellation_token();
                let waiting = running.waiting();
                tokio::pin!(waiting);
                tokio::select! {
                    _ = &mut waiting => {},
                    _ = stop.changed() => {
                        cancel.cancel();
                        let _ = waiting.await;
                    }
                }
            }
        });
    }
    inner.ended(&attachment);
}

/// Register before running, then release the handle only after all runtime work ends.
fn spawn_runtime(
    inner: Arc<Inner>,
    stop: tokio::sync::watch::Sender<bool>,
    run: impl FnOnce() + Send + 'static,
) -> std::io::Result<()> {
    // Immediate EOF must not finish before its handle is registered.
    let mut st = lock(&inner.state);
    let completing = inner.clone();
    let thread = std::thread::Builder::new()
        .name("oac-claude-channel".into())
        .spawn(move || {
            run();
            // Runtime, writes and ended() are finished. Shutdown joins any runtime
            // it took first; otherwise release our own registry entry now.
            lock(&completing.state)
                .connections
                .remove(&std::thread::current().id());
        })?;
    st.connections.insert(thread.thread().id(), (stop, thread));
    Ok(())
}

impl ProviderAdapter for ClaudeAdapter {
    fn take_connection(&self, connection: Connection) {
        let (attachment, reader, writer) = connection.into_parts();
        let down = self.inner.down.read().unwrap_or_else(|e| e.into_inner());
        if *down {
            return;
        }
        {
            lock(&self.inner.state).attachments.insert(
                attachment.clone(),
                Att {
                    phase: Phase::Opening,
                    bound: None,
                    peer: None,
                    runtime: None,
                    cancel: None,
                    handed: VecDeque::new(),
                    reply_records_evicted: false,
                },
            );
        }
        let inner = self.inner.clone();
        let a = attachment.clone();
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        if spawn_runtime(self.inner.clone(), stop_tx, move || {
            serve(inner, a, reader, writer, stop_rx);
        })
        .is_err()
        {
            lock(&self.inner.state).attachments.remove(&attachment);
        }
    }

    fn watch_attachments(&self, handler: AdapterEventHandler) {
        lock(&self.inner.state).handler = Some(handler);
    }

    fn set_binding(&self, attachment: &Attachment, session: Option<SessionId>) {
        if let Some(att) = lock(&self.inner.state).attachments.get_mut(attachment) {
            att.bound = session;
        }
    }

    fn capabilities(&self, _attachment: &Attachment) -> AdapterCapabilities {
        AdapterCapabilities {
            // Hand-off is the channel notification, pushed into the live session.
            active_inbound: true,
            // Text only; no size is stated, since the surface documents none.
            content_types: Vec::new(),
            max_envelope_octets: None,
        }
    }

    /// One `notifications/claude/channel` per hand-off, or none ([IFC-ADP-057]).
    ///
    /// `completed` means the notification's bytes were written to the connection: Claude
    /// Code acknowledges nothing, so that is all a channel hand-off can show, and the core
    /// records it as `handed-to-harness`, never as seen by the model ([IFC-ADP-051]; C6
    /// §11). Mid-turn, Claude Code queues the notification and takes it in at a later
    /// boundary, in order; the adapter neither waits for nor depends on how many it takes
    /// at once (11-risks row 49; `spec/bindings/mcp.md` §8.1).
    fn deliver(&self, hand_off: HandOff) -> HandOffOutcome {
        let down = self.inner.down.read().unwrap_or_else(|e| e.into_inner());
        if *down {
            return HandOffOutcome::Failed;
        }
        let message = hand_off.message();
        let meta = match provenance_meta(&META_KEYS, message) {
            Ok(m) => m,
            Err(ProvenanceRefusal::Unverified) => return HandOffOutcome::Failed,
            Err(_) => return HandOffOutcome::Refused,
        };
        let Some(content) = channel_content(message) else {
            return HandOffOutcome::Failed;
        };
        let (peer, runtime) = {
            let st = lock(&self.inner.state);
            let Some(att) = st.attachments.get(hand_off.attachment()) else {
                return HandOffOutcome::Failed;
            };
            // [IFC-ADP-030]: never to an attachment the core's latest binding left unbound.
            if att.phase != Phase::Open || att.bound.is_none() {
                return HandOffOutcome::Failed;
            }
            match (&att.peer, &att.runtime) {
                (Some(p), Some(r)) => (p.clone(), r.clone()),
                _ => return HandOffOutcome::Failed,
            }
        };
        let mut meta_object = JsonObject::new();
        for (k, v) in meta {
            meta_object.insert(k, Value::from(v));
        }
        let mut params = JsonObject::new();
        params.insert("content".to_owned(), Value::from(content));
        params.insert("meta".to_owned(), Value::from(meta_object));
        let notification = ServerNotification::CustomNotification(CustomNotification::new(
            CHANNEL_NOTIFICATION,
            Some(Value::from(params)),
        ));
        // Include indeterminate attempts: their bytes may still reach the harness.
        {
            let env = message.envelope();
            if let Some(att) = lock(&self.inner.state)
                .attachments
                .get_mut(hand_off.attachment())
            {
                if att.handed.len() >= HANDED_KEPT {
                    att.handed.pop_front();
                    att.reply_records_evicted = true;
                }
                att.handed.push_back((
                    env.id().as_str().to_owned(),
                    env.from().clone(),
                    env.to().clone(),
                    false,
                ));
            }
        }
        let (tx, rx) = std::sync::mpsc::channel();
        runtime.spawn(async move {
            let _ = tx.send(peer.send_notification(notification).await.is_ok());
        });
        let outcome = match rx.recv_timeout(self.inner.handoff_wait) {
            Ok(true) => HandOffOutcome::Completed,
            Ok(false) => HandOffOutcome::Failed,
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                HandOffOutcome::Indeterminate
            }
        };
        if outcome == HandOffOutcome::Completed {
            let env = message.envelope();
            if let Some(att) = lock(&self.inner.state)
                .attachments
                .get_mut(hand_off.attachment())
            {
                for (id, from, to, completed) in &mut att.handed {
                    if id == env.id().as_str() && from == env.from() && to == env.to() {
                        *completed = true;
                    }
                }
            }
        }
        outcome
    }

    fn accept_requests(&self, sink: Arc<dyn RequestSink>) {
        lock(&self.inner.state).sink = Some(sink);
    }

    fn health(&self) -> HealthStatus {
        if *self.inner.down.read().unwrap_or_else(|e| e.into_inner()) {
            HealthStatus::new(HealthState::Unavailable)
        } else {
            HealthStatus::new(HealthState::Healthy)
        }
    }

    fn shutdown(&self) {
        let _shutdown = lock(&self.inner.shutdown);
        let _ev = lock(&self.inner.events);
        let mut down = self.inner.down.write().unwrap_or_else(|e| e.into_inner());
        *down = true;
        let (h, open, cancels, connections) = {
            let mut st = lock(&self.inner.state);
            let mut open = Vec::new();
            let mut cancels = Vec::new();
            for (a, att) in st.attachments.iter_mut() {
                if att.phase == Phase::Open {
                    open.push(a.clone());
                }
                att.phase = Phase::Closed;
                if let Some(c) = att.cancel.take() {
                    cancels.push(c);
                }
            }
            (
                st.handler.clone(),
                open,
                cancels,
                std::mem::take(&mut st.connections),
            )
        };
        drop(down);
        for (stop, _) in connections.values() {
            let _ = stop.send(true);
        }
        for c in cancels {
            c.cancel();
        }
        for a in open {
            self.inner
                .emit(h.clone(), AdapterEvent::AttachmentClosed { attachment: a });
        }
        // No callback lock may be held while joining: a connection finishes through ended().
        drop(_ev);
        // Blocking std::io writes cannot be interrupted. Join the runtime that owns them,
        // including its pending notification tasks, before shutdown returns [IFC-ADP-070].
        for (_, thread) in connections.into_values() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_runtimes_release_their_registry_entries_without_shutdown() {
        let adapter = ClaudeAdapter::new();
        let (release, held) = std::sync::mpsc::channel();
        let (stop, _rx) = tokio::sync::watch::channel(false);
        spawn_runtime(adapter.inner.clone(), stop, move || {
            held.recv().unwrap();
        })
        .unwrap();
        assert_eq!(
            lock(&adapter.inner.state).connections.len(),
            1,
            "unfinished runtime must remain available to shutdown"
        );
        release.send(()).unwrap();
        for _ in 0..100 {
            let (stop, _rx) = tokio::sync::watch::channel(false);
            spawn_runtime(adapter.inner.clone(), stop, || {}).unwrap();
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let st = lock(&adapter.inner.state);
            if st.attachments.is_empty() && st.connections.is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{} attachments and {} runtime entries retained",
                st.attachments.len(),
                st.connections.len()
            );
            drop(st);
            std::thread::sleep(Duration::from_millis(5));
        }
        adapter.shutdown();
    }
}
