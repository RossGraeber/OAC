// SPDX-License-Identifier: Apache-2.0

//! [`ClaudeAdapter`]: `ProviderAdapter` (`spec/interfaces.md` Table 5.2) for the Claude Code
//! Channels path (research preview).
//!
//! Each connection the core gives it ([`ProviderAdapter::take_connection`]) is one channel
//! server's stdio stream, relayed by the shim Claude Code spawned
//! (`docs/planning/decisions/C2-process-model.md` §1). One connection is one attachment, and
//! one attachment presents exactly one channel: the adapter never multiplexes logical
//! channels over one server (whether Claude Code supports that is UNVERIFIED, 11-risks
//! row 2, so nothing here depends on it).
//!
//! A connection is either that, or a **carrier of one native signal**: the hook handler's
//! connection, whose first line is a hook input rather than an MCP frame ([`crate::hook`]).
//! The adapter reports a `SessionStart` input as a `native-signal` on the connection it came
//! on, never as an attachment, and closes the carrier. It binds nothing itself.
//!
//! Each connection runs its MCP server ([`crate::server`]) on its own thread, with a
//! current-thread `tokio` runtime. Everything inbound is pushed: the harness's frames arrive
//! on the connection, and hand-offs are notifications the server writes. Nothing here asks
//! the harness, or anything else, for pending messages ([IFC-ADP-040]; no polling of any
//! kind).
//!
//! Interfaces 0.6: the local adapter budget is `shutdown_bound_ms = 1000`.
//! `event_callback_bound_ms = 11000`, `stop_callback_bound_ms = 200`.
//! At most seven connections and 64 core invocations are admitted: [`SHIM_PLACES`] (four) for
//! channel servers, which hold a place for the life of a session, and [`CARRIER_PLACES`]
//! (three) that no channel server can take, for hook carriers (which hold one only while they
//! send) and for connections that have not yet said what they are. A connection is a shim
//! once its first line is not a hook input; a shim that would be the fifth is refused then.
//! Stream closure costs at most 700 ms sequentially (seven closures of
//! [`oac_core::connection::CLOSE_BOUND_MS`]), the stop budget 200 ms: 900 ms of the 1000 ms
//! bound, leaving 100 ms for callback/worker coordination. Eight places would reach the bound
//! (the core asserts 8 * 100 + 200 < 1000 at construction) and so are not possible without
//! raising `shutdown_bound_ms`.
//! The core adds its own aggregate reporting/settlement budget. Awaited publication adds
//! the transport's OPTIONAL bound; an undeclared contribution is unbounded. Every
//! committed request still settles before shutdown returns, with no post-return input,
//! report or callback (IFC-ADP-109..125). Legacy event handlers are test compatibility;
//! production installs the cooperative local handler before admitting connections.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;

use oac_core::adapter::{
    AdapterCapabilities, AdapterEvent, AdapterEventHandler, Attachment, Connection,
    DiscoveryRequest, HandOff, HandOffOutcome, IdentityRequest, LocalEventHandler, NativeSignal,
    ProviderAdapter, RequestSink,
};
use oac_core::delivery::ErrorCode;
use oac_core::health::{HealthState, HealthStatus};
use oac_core::ids::SessionId;
use oac_core::local_call::{CallAdmission, CallContext, LocalResult};
use oac_mcp_tools::{ToolCall, Value};
use rmcp::model::{CallToolResult, CustomNotification, JsonObject, ServerNotification};
use rmcp::service::{Peer, RoleServer, RunningServiceCancellationToken};

use crate::channel::{
    CHANNEL_NOTIFICATION, META_KEYS, ProvenanceRefusal, channel_content, provenance_meta,
};
use crate::hook::{self, FirstLine, MAX_HOOK_INPUT, SessionStart};
use crate::io::{ChannelReader, ChannelWriter, Head};
use crate::server::{ChannelServer, ChannelTransport};

/// The tail of the `health` detail that says connections did not negotiate a legacy MCP
/// revision (`oac doctor` recognises the detail by it).
pub const UNNEGOTIATED_DETAIL: &str = "did not complete a legacy MCP revision negotiation";

/// How long `deliver` waits for the notification's write to complete before it reports
/// `indeterminate` ([IFC-ADP-053]): the write neither completed nor failed in that time.
/// Binding shutdown bound, including connection closure and pending call settlement.
pub const SHUTDOWN_BOUND_MS: u64 = 1000;

pub const HANDOFF_WAIT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// The MCP opening is under way: nothing is handed off.
    Opening,
    /// The channel is registered and listed; `attachment-opened` was reported.
    Open,
    /// A carrier of native signals (a hook input, [`crate::hook`]), not an attachment: it is
    /// never opened or handed off to.
    Carrier,
    /// Ended; `attachment-closed` was reported if it had opened.
    Closed,
}

/// How long a connection's whole first line may take, counted from its first byte (a
/// stalled or trickling hook input must not hold one of the few connection places). A
/// connection that has sent nothing yet is not bounded, as before.
pub const FIRST_LINE_STALL: Duration = Duration::from_secs(10);

/// Connections that may be channel servers (attachments) at once. A shim holds its place for
/// the life of its session.
pub const SHIM_PLACES: usize = 4;

/// Places that a channel server cannot take: for hook carriers, which hold one only while
/// they send a hook input, and for connections that have not yet sent their first line. So a
/// `startup` hook of a new session, or a `clear`, `resume` or `compact` hook of a live one, is
/// admitted whatever the shims hold (11-risks row 86).
pub const CARRIER_PLACES: usize = 3;

struct Att {
    phase: Phase,
    /// Whether this connection has taken one of the [`SHIM_PLACES`]: its first line said it
    /// is a channel server.
    shim: bool,
    /// The cross-check value E of the shim's hello line, when it sent a valid one.
    cross_check: Option<String>,
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

impl Att {
    /// The core's binding for this attachment changed. A different session id is a new
    /// session: what the earlier one was handed carries nothing forward, so no `reply` of
    /// the new session finds an earlier session's message by its id ([SC-ID-151]; C4 §6).
    /// Unbinding and binding the same session again keep the records.
    fn rebind(&mut self, session: Option<SessionId>) {
        if session.is_some() && self.bound.is_some() && session != self.bound {
            self.handed.clear();
            self.reply_records_evicted = false;
        }
        self.bound = session;
    }
}

/// How many hand-off records an attachment keeps for `reply`. Older ones are dropped, so a
/// peer cannot grow the adapter's memory without bound. After any eviction all replies need
/// `to`, since a retained id could collide with an evicted sender's id.
pub const HANDED_KEPT: usize = 1024;

#[derive(Default)]
struct State {
    handler: Option<AdapterEventHandler>,
    local_handler: Option<LocalEventHandler>,
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
    events: crate::gate::Serial,
    /// Set under state lock when shutdown begins. New calls refuse; already admitted
    /// calls settle after stream closure before shutdown returns ([IFC-ADP-070]).
    down: AtomicBool,
    calls: RwLock<()>,
    /// How long `deliver` waits for a write ([`HANDOFF_WAIT`] unless set).
    handoff_wait: Duration,
    admission: Mutex<(CallAdmission, u64, u64)>,
    /// How many connections since this adapter was built sent the `server/discover` probe or
    /// asked for a revision this server does not speak, and ended without completing a
    /// legacy `initialize` (`Inner::note_unnegotiated`). `oac doctor` reports it through
    /// `health` (G10, #71).
    unnegotiated: AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Inner {
    /// Notes one connection that asked for the stateless revision (or probed for it) and
    /// ended with no legacy `initialize` completed: Claude Code does not register such a
    /// server as a channel (`spec/bindings/mcp.md` [MCPB-CLD-001]).
    pub(crate) fn note_unnegotiated(&self) {
        self.unnegotiated.fetch_add(1, Ordering::SeqCst);
    }

    fn emit(&self, h: Option<AdapterEventHandler>, e: AdapterEvent) {
        let (handler, admission, bound) = {
            let st = lock(&self.state);
            let (admission, ordinary, stop) = &*lock(&self.admission);
            (
                st.local_handler.clone(),
                admission.clone(),
                if self.down.load(Ordering::SeqCst) {
                    *stop
                } else {
                    *ordinary
                },
            )
        };
        if let Some(handler) = handler {
            let (attachment, final_report) = match &e {
                AdapterEvent::AttachmentClosed { attachment } => (Some(attachment.clone()), true),
                AdapterEvent::AttachmentOpened { attachment, .. }
                | AdapterEvent::CapabilitiesChanged { attachment } => {
                    (Some(attachment.clone()), false)
                }
                _ => (None, false),
            };
            let mut call = admission.call(attachment, bound);
            if final_report {
                call = call.final_report();
            }
            let _ = handler(&mut call, e);
            return;
        }
        if let Some(h) = h {
            h(e);
        }
    }

    /// The first `tools/list` answer is written: the attachment is open.
    pub(crate) fn listed(&self, a: &Attachment) {
        let _ev = self.events.enter();
        let (h, cross_check) = {
            if self.down.load(Ordering::SeqCst) {
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
            let cross_check = att.cross_check.clone();
            (st.handler.clone(), cross_check)
        };
        self.emit(
            h,
            AdapterEvent::AttachmentOpened {
                attachment: a.clone(),
                cross_check,
            },
        );
    }

    /// The connection's first line says it is a channel server (not a hook input). It takes
    /// one of the [`SHIM_PLACES`], or is refused when they are all taken; `cross_check` is
    /// its hello's value, kept for `attachment-opened`. Decided under the state lock, so two
    /// connections cannot both take the last place.
    pub(crate) fn admit_shim(&self, a: &Attachment, cross_check: Option<String>) -> bool {
        let mut st = lock(&self.state);
        if st.attachments.values().filter(|att| att.shim).count() >= SHIM_PLACES {
            return false;
        }
        match st.attachments.get_mut(a) {
            Some(att) => {
                att.shim = true;
                att.cross_check = cross_check;
                true
            }
            // Ended or shut down meanwhile.
            None => false,
        }
    }

    /// The connection is a carrier of native signals ([`crate::hook`]). A `SessionStart`
    /// input is reported as one `native-signal` on this connection ([IFC-ADP-020]); anything
    /// else it carried is not a native signal and is dropped. The adapter decides nothing
    /// about pairing or binding ([SC-ID-120]): the core does, from the connection.
    pub(crate) fn carrier(&self, a: &Attachment, signal: Option<SessionStart>) {
        let _ev = self.events.enter();
        let h = {
            if self.down.load(Ordering::SeqCst) {
                return;
            }
            let mut st = lock(&self.state);
            let Some(att) = st.attachments.get_mut(a) else {
                return;
            };
            if att.phase != Phase::Opening {
                return;
            }
            att.phase = Phase::Carrier;
            st.handler.clone()
        };
        if let Some(signal) = signal {
            self.emit(
                h,
                AdapterEvent::NativeSignal(NativeSignal {
                    native_id: signal.native_id,
                    start_kind: signal.start_kind,
                    cross_check: None,
                    connection: Some(a.clone()),
                    revealed: None,
                }),
            );
        }
    }

    /// The connection ended.
    fn ended(&self, a: &Attachment) {
        let _ev = self.events.enter();
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
        let _call = self.calls.read().unwrap_or_else(|e| e.into_inner());
        if self.down.load(Ordering::SeqCst) {
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
        let (admission, ordinary, _) = lock(&self.admission).clone();
        let mut local = admission.call(Some(a.clone()), ordinary);
        let render_send = |result| match result {
            LocalResult::Completed(value) => oac_mcp_tools::send_result(&value),
            _ => oac_mcp_tools::refusal(ErrorCode::InternalError),
        };
        match call {
            ToolCall::Send {
                to,
                content,
                conversation_id,
                correlation_id,
            } => render_send(sink.send_local(
                &mut local,
                oac_mcp_tools::send_request(
                    a.clone(),
                    to,
                    content,
                    conversation_id,
                    correlation_id,
                ),
            )),
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
                    Some(to) => render_send(sink.send_local(
                        &mut local,
                        oac_mcp_tools::reply_request(a.clone(), to, content, in_reply_to),
                    )),
                    // No `to`, and no record of the message on this attachment.
                    None => oac_mcp_tools::refusal(ErrorCode::InvalidRequest),
                }
            }
            ToolCall::ListSessions => {
                match sink.discover_local(
                    &mut local,
                    DiscoveryRequest {
                        attachment: a.clone(),
                    },
                ) {
                    LocalResult::Completed(value) => oac_mcp_tools::discovery_result(&value),
                    _ => oac_mcp_tools::refusal(ErrorCode::InternalError),
                }
            }
            // [IFC-ADP-094]: the identity comes from an `IdentityRequest` attributed to this
            // attachment, never from a value this adapter holds. An attachment the core does
            // not serve a bound session on is refused `unauthorized` by the core
            // ([IFC-ADP-095], [MCPB-ATT-002]).
            ToolCall::Whoami => match sink.identity_local(
                &mut local,
                IdentityRequest {
                    attachment: a.clone(),
                },
            ) {
                LocalResult::Completed(value) => oac_mcp_tools::whoami_result(&value),
                _ => oac_mcp_tools::refusal(ErrorCode::InternalError),
            },
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
        assert!(
            wait <= HANDOFF_WAIT,
            "handoff wait exceeds the declared callback budget"
        );
        ClaudeAdapter {
            inner: Arc::new(Inner {
                state: Mutex::default(),
                events: crate::gate::Serial::default(),
                down: AtomicBool::new(false),
                calls: RwLock::new(()),
                handoff_wait: wait,
                admission: Mutex::new((CallAdmission::default(), 1000, 100)),
                unnegotiated: AtomicU64::new(0),
            }),
        }
    }
}

/// Serves one connection until it ends, then reports it closed.
///
/// The first line says what the connection is: a hook input makes it a carrier of one native
/// signal ([`crate::hook`]); anything else is the harness's MCP stream, served as the channel
/// server. A hook input is not an MCP frame, so nothing is lost by looking first: the bytes
/// stay in the reader for `rmcp`.
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
    if let (Ok(runtime), Ok(mut read)) = (runtime, ChannelReader::spawn(reader)) {
        let handle = runtime.handle().clone();
        runtime.block_on(async {
            // A connection can be stopped even while it waits for its first byte.
            let head = tokio::select! {
                head = read.peek_line(MAX_HOOK_INPUT, FIRST_LINE_STALL) => head,
                _ = stop.changed() => return,
            };
            let mut cross_check = None;
            match head {
                Head::Stalled => return,
                Head::Line => {
                    let received = read.received();
                    let end = received
                        .iter()
                        .position(|&b| b == b'\n')
                        .unwrap_or(received.len());
                    match hook::classify(&received[..end]) {
                        FirstLine::Hook(input) => {
                            inner.carrier(&attachment, input.session_start());
                            return;
                        }
                        // The shim's hello: E is kept for `attachment-opened`, and the line is
                        // not the MCP server's.
                        FirstLine::Hello(e) => {
                            read.consume_first_line();
                            cross_check = Some(e);
                        }
                        FirstLine::Mcp => {}
                    }
                }
                // An empty stream, or a first line too long for a hook input, is for the MCP
                // server to end, as it always was.
                Head::Empty | Head::TooLong => {}
            }
            // Not a carrier, so a channel server: it takes one of the shim places, or is
            // refused when the shims hold them all (a carrier is admitted whatever they hold).
            if !inner.admit_shim(&attachment, cross_check) {
                return;
            }
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
    attachment.close();
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
    fn local_calls(&self) -> bool {
        true
    }
    fn connection_limit(&self) -> usize {
        SHIM_PLACES + CARRIER_PLACES
    }
    fn set_binding_local(
        &self,
        call: &mut CallContext,
        attachment: &Attachment,
        session: Option<SessionId>,
    ) -> LocalResult<()> {
        let mut state = match call.lock(&self.inner.state) {
            Ok(v) => v,
            Err(v) => return v,
        };
        call.commit(|| {
            if let Some(att) = state.attachments.get_mut(attachment) {
                att.rebind(session);
            }
        })
    }

    fn capabilities_local(
        &self,
        call: &mut CallContext,
        attachment: &Attachment,
    ) -> LocalResult<AdapterCapabilities> {
        call.commit(|| self.capabilities(attachment))
    }

    fn deliver_local(
        &self,
        call: &mut CallContext,
        hand_off: HandOff,
    ) -> LocalResult<HandOffOutcome> {
        if let Some(v) = call.cancellation() {
            return v.map(|()| unreachable!());
        }
        LocalResult::Completed(self.deliver(hand_off))
    }
    fn configure_local_calls(&self, admission: CallAdmission, ordinary_ms: u64, stop_ms: u64) {
        assert!(
            ordinary_ms > 0
                && stop_ms > 0
                && stop_ms <= ordinary_ms
                && ordinary_ms <= oac_core::local_call::CORE_CALL_BOUND_MS
                && stop_ms <= oac_core::local_call::STOP_CALL_BOUND_MS,
            "call costs exceed the declared core settlement budget"
        );
        *lock(&self.inner.admission) = (admission, ordinary_ms, stop_ms);
    }

    fn watch_local_attachments(&self, handler: LocalEventHandler) {
        lock(&self.inner.state).local_handler = Some(handler);
    }
    fn shutdown_bound_ms(&self) -> u64 {
        SHUTDOWN_BOUND_MS
    }

    fn take_connection(&self, connection: Connection) {
        let (attachment, reader, writer) = connection.into_parts();
        let _call = self.inner.calls.read().unwrap_or_else(|e| e.into_inner());
        if self.inner.down.load(Ordering::SeqCst) {
            attachment.close();
            return;
        }
        {
            let mut state = lock(&self.inner.state);
            if self.inner.down.load(Ordering::SeqCst)
                || state.attachments.len() >= self.connection_limit()
            {
                attachment.close();
                return;
            }
            state.attachments.insert(
                attachment.clone(),
                Att {
                    phase: Phase::Opening,
                    shim: false,
                    cross_check: None,
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
            attachment.close();
            lock(&self.inner.state).attachments.remove(&attachment);
        }
    }

    fn watch_attachments(&self, handler: AdapterEventHandler) {
        lock(&self.inner.state).handler = Some(handler);
    }

    fn set_binding(&self, attachment: &Attachment, session: Option<SessionId>) {
        if let Some(att) = lock(&self.inner.state).attachments.get_mut(attachment) {
            att.rebind(session);
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
        let _call = self.inner.calls.read().unwrap_or_else(|e| e.into_inner());
        if self.inner.down.load(Ordering::SeqCst) {
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
        {
            let _st = lock(&self.inner.state);
            if self.inner.down.load(Ordering::SeqCst) {
                return HandOffOutcome::Failed;
            }
            runtime.spawn(async move {
                let _ = tx.send(peer.send_notification(notification).await.is_ok());
            });
        }
        let end = std::time::Instant::now() + self.inner.handoff_wait;
        let outcome = loop {
            match rx.recv_timeout(Duration::from_millis(1)) {
                Ok(true) => break HandOffOutcome::Completed,
                Ok(false) => break HandOffOutcome::Failed,
                Err(RecvTimeoutError::Disconnected) => break HandOffOutcome::Indeterminate,
                Err(RecvTimeoutError::Timeout) => {
                    if self.inner.down.load(Ordering::SeqCst) {
                        break HandOffOutcome::Indeterminate;
                    }
                    if std::time::Instant::now() >= end {
                        break HandOffOutcome::Indeterminate;
                    }
                }
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
        if self.inner.down.load(Ordering::SeqCst) {
            HealthStatus::new(HealthState::Unavailable)
        } else {
            match self.inner.unnegotiated.load(Ordering::SeqCst) {
                0 => HealthStatus::new(HealthState::Healthy),
                n => HealthStatus::with_detail(
                    HealthState::Degraded,
                    format!("{n} connection(s) {UNNEGOTIATED_DETAIL}"),
                ),
            }
        }
    }

    fn shutdown(&self) {
        lock(&self.inner.admission).0.stop();
        {
            let _st = lock(&self.inner.state);
            self.inner.down.store(true, Ordering::SeqCst);
        }
        // Revoke I/O before waiting for calls that might be blocked on it.
        let handles: Vec<_> = lock(&self.inner.state)
            .attachments
            .keys()
            .cloned()
            .collect();
        for h in &handles {
            h.begin_close();
        }
        for h in handles {
            h.close();
        }
        let calls = self.inner.calls.write().unwrap_or_else(|e| e.into_inner());
        drop(calls);
        let events = self.inner.events.enter();
        let (handler, open, cancels, stops) = {
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
                st.connections
                    .values()
                    .map(|(stop, _)| stop.clone())
                    .collect::<Vec<_>>(),
            )
        };
        for stop in stops {
            let _ = stop.send(true);
        }
        for cancel in cancels {
            cancel.cancel();
        }
        // No call lock across a callback; handlers may re-enter shutdown.
        for attachment in open {
            self.inner.emit(
                handler.clone(),
                AdapterEvent::AttachmentClosed { attachment },
            );
        }
        drop(events);
        let me = std::thread::current().id();
        let workers = {
            let mut st = lock(&self.inner.state);
            let ids: Vec<_> = st
                .connections
                .keys()
                .copied()
                .filter(|id| *id != me)
                .collect();
            ids.into_iter()
                .filter_map(|id| st.connections.remove(&id))
                .collect::<Vec<_>>()
        };
        for (_, worker) in workers {
            let _ = worker.join();
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
