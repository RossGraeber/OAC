// SPDX-License-Identifier: Apache-2.0

//! [`CodexAdapter`]: `ProviderAdapter` (`spec/interfaces.md` Table 5.2) for the Codex App
//! Server path (G8 slice S4, #69, #403, with G7's `deliver`, #68).
//!
//! Two kinds of connection reach `take_connection`, and the adapter tells them apart by
//! the handle alone:
//!
//! - **The carrier**: the one connection to Codex's app-server (D8: `cli/` opened the
//!   stream). It is exactly the handle that the daemon designated with
//!   [`CodexAdapter::designate_carrier`] before it gave the connection over
//!   (`oac_core::pipeline::Pipelines::connect_carrier`), and no message, signal or tool call
//!   can designate one ([MCPB-ATT-006]). The adapter runs a [`CodexClient`] on it
//!   ([`crate::client`]), with the pairing ([`crate::pairing`]) listening to the client's
//!   events. It is never an attachment and never reported as one.
//! - **A shim connection**: the stdio of one OAC MCP server that Codex started
//!   (`codex mcp add oac -- oac mcp-shim --harness codex`). Each is served by
//!   [`crate::mcp`], whose every call goes through the pairing, and is one attachment: it
//!   becomes `attachment-opened` when the first `tools/list` answer is written, and
//!   `attachment-closed` when the stream ends.
//!
//! **Pairing, and what the adapter adds.** The core issues the pairing values, ends the windows
//! and binds ([`oac_core::issued_pairing`]). This adapter implements [`CallPort`], the seam
//! of [`crate::pairing`]: it passes each call to the core's request sink and renders the
//! answer unchanged ([IFC-ADP-060]), reports `attachment-unconfirmed` before it passes on a
//! request it could not confirm ([IFC-ADP-091]), and reports a reveal as a `native-signal`
//! on the carrier whose `native_id` is the notification's `threadId` and whose `revealed`
//! is the value ([IFC-ADP-020]; start kind `fresh`, no cross-check, `spec/bindings/mcp.md`
//! §4.5.3). It binds nothing itself ([SC-ID-120], [IFC-ADP-007]).
//!
//! The thread a reveal named is kept per attachment, **uncommitted**. The core binds on the
//! reveal and then tells the adapter, through `set_binding`, the session it bound; only that
//! call commits the thread, and it also tells the pairing the connection is bound
//! ([`crate::pairing::CodexPairing::set_bound`]). A `set_binding` naming no session clears the
//! thread and the reveal ([IFC-ADP-030]): a binding that ended needs a new reveal.
//!
//! **Delivery** ([`ProviderAdapter::deliver`]): one `thread/queue/add` for the attachment's
//! committed thread, with the C6 §5.0 frame ([`crate::frame`]), and nothing else
//! ([MCPB-CDX-002] to [MCPB-CDX-005]). It never calls `turn/start` or `turn/steer`, which are
//! not even on the client's allowlist, and it never answers a server request (a permission
//! relay): the client leaves every one unanswered. The call's result maps to the outcome of
//! `spec/bindings/mcp.md` §8.2.1:
//!
//! | `thread/queue/add` | Outcome | Recorded as |
//! |---|---|---|
//! | result with `queuedSubmission` | `completed` | `handed-to-harness` |
//! | JSON-RPC error (an archived, ephemeral or subagent thread; no queue service) | `failed` | `failed`, `handoff-failed` ([SC-DLV-009]) |
//! | no answer in time, an answer lost with the carrier, a stream error, an answer of unknown shape | `indeterminate` | `unknown` ([IFC-ADP-053]) |
//! | not sent: the carrier is closed, the thread is not served, the attachment is unbound | `failed` | `failed`, `handoff-failed` |
//! | a provenance value that is not a whole token ([SEC-PRV-014]) | `refused` | `failed`, `internal-error` |
//!
//! `not-now` is never returned: no refusal of the add is known to mean "cannot take input
//! now" (§8.2.1, UNVERIFIED). No other method is tried after a refusal ([SEC-AUZ-027]), and
//! the adapter asks the app-server for nothing between deliveries ([IFC-ADP-040]): it
//! subscribes to a thread once, when pairing needs it, and holds the subscription while the
//! thread's attachment is paired.
//!
//! **An unprompted `thread/archived`.** Archiving a thread sends its subscribers
//! `thread/status/changed` (`notLoaded`) and then `thread/archived` (S3 capture L1024-L1029,
//! the fake's `oacFake/thread/setArchived`). The adapter takes no action on either: they
//! are not a reported end of the attachment (the attachment is the MCP connection, whose end
//! the stream shows, [IFC-ADP-022]), they change no capability, and a binding is the core's
//! to end. The client drops `thread/archived` ([`crate::events::Dropped::Unused`]) and tracks
//! the status. The next delivery makes its one add and gets the recorded refusal, which is
//! `handoff-failed`; if the thread is unarchived, the next one is `completed`. This is
//! pinned by `tests/adapter.rs` against the recorded frames.
//!
//! **Subscription ownership.** [`crate::serving::Serving`] unsubscribes the threads it
//! subscribed tentatively when a window ends, unless a connection holds them. Delivery never
//! subscribes on its own account: the thread of a delivery is the thread a reveal named, held
//! by that very connection from the reveal until the connection is unbound or ends, and a
//! delivery to an attachment whose thread is no longer served is `failed` without a call.
//! So the hold of the pairing and the use of the thread by delivery have one owner, the
//! attachment, and a window's end cannot unsubscribe a thread under a bound attachment.
//!
//! **Budgets and shutdown.** Interfaces 0.6's cooperative local-call surface
//! (`configure_local_calls`, `watch_local_attachments`, `set_binding_local`) is not
//! implemented: the adapter uses the legacy surface, which the pipeline accepts. Shutdown
//! sets the stop, cancels waits for confirmations and in-flight app-server calls, closes
//! every connection it was given, reports `attachment-closed` for each open attachment, and
//! joins its threads; nothing is handed off and no request is passed after it returns
//! ([IFC-ADP-070], [IFC-ADP-071]).

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, Weak};
use std::thread::JoinHandle;

use oac_core::adapter::{
    AdapterCapabilities, AdapterEvent, AdapterEventHandler, Attachment, Connection,
    ConnectionHandle, DiscoveryRequest, HandOff, HandOffOutcome, IdentityRequest, NativeSignal,
    ProviderAdapter, RequestSink, StartKind,
};
use oac_core::delivery::ErrorCode;
use oac_core::health::{HealthState, HealthStatus};
use oac_core::ids::SessionId;
use oac_mcp_tools::ToolCall;

use crate::client::{ClientError, CodexClient, Options};
use crate::frame::{self, FrameRefusal};
use crate::mcp::{ServerHandle, serve_connection};
use crate::pairing::{Answer, CallPort, CodexPairing, Config, Reveal};

/// The name the OAC server is registered under with Codex when nothing else is said
/// (`codex mcp add oac -- ...`, `docs/planning/decisions/C2-process-model.md` §6).
pub const DEFAULT_REGISTERED_NAME: &str = "oac";

/// Shim connections that may be open at once: one per Codex session. A connection that would
/// be one more is refused (closed).
pub const SHIM_PLACES: usize = 6;

/// The carrier's place: one connection to the app-server.
pub const CARRIER_PLACES: usize = 1;

/// Binding shutdown bound, including connection closure: the default (1000 ms). At most
/// seven connections close in 700 ms (`oac_core::connection::CLOSE_BOUND_MS` each).
pub const SHUTDOWN_BOUND_MS: u64 = 1000;

/// How many hand-off records an attachment keeps for `reply`. Older ones are dropped, so a
/// peer cannot grow the adapter's memory without bound. After any eviction all replies need
/// `to`, since a retained id could collide with an evicted sender's id.
pub const HANDED_KEPT: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// The MCP opening is under way, or waits for the carrier: nothing is handed off.
    Opening,
    /// Listed; `attachment-opened` was reported.
    Open,
    /// Ended; `attachment-closed` was reported if it had opened.
    Closed,
}

struct Att {
    phase: Phase,
    /// The connection's server, once the carrier is up.
    server: Option<ServerHandle>,
    /// The distinct native thread ids that reveals named since the last `set_binding`, in
    /// order (bounded). Kept by this module only; not committed. The core decides on a value,
    /// not on a thread, and `set_binding` does not say which reveal it decided on, so a second
    /// thread before the decision makes the choice ambiguous: nothing is committed then.
    pending: Vec<String>,
    /// The session the core's latest `set_binding` named.
    bound: Option<SessionId>,
    /// The thread of a delivery: the one thread `pending` held, committed by the `set_binding` that named a
    /// session. Native; it never leaves this module.
    thread: Option<String>,
    /// Hand-off attempts for a reply's `to`: message id, sender, recipient, and whether the
    /// hand-off completed, for the last [`HANDED_KEPT`] messages, oldest first.
    handed: VecDeque<(String, SessionId, SessionId, bool)>,
    reply_records_evicted: bool,
}

/// The most distinct threads kept between two `set_binding` calls.
const PENDING_KEPT: usize = 4;

impl Att {
    /// A reveal named `thread`.
    fn revealed(&mut self, thread: &str) {
        if !self.pending.iter().any(|t| t == thread) && self.pending.len() < PENDING_KEPT {
            self.pending.push(thread.to_owned());
        }
    }

    /// The core's `set_binding` named `session`: the thread of delivery becomes the one thread
    /// the reveals since the last call named. Two or more make the core's decision ambiguous
    /// (the second reveal could be a forged one for another thread, [MCPB-ATT-011]), so no
    /// thread is committed. With no reveal since, the committed thread stands.
    fn commit_binding(&mut self, session: SessionId) {
        // A different session id is a new session: what the earlier one was handed carries
        // nothing forward ([SC-ID-151]).
        if self.bound.as_ref().is_some_and(|b| *b != session) {
            self.handed.clear();
            self.reply_records_evicted = false;
        }
        self.bound = Some(session);
        match self.pending.len() {
            0 => {}
            1 => self.thread = self.pending.pop(),
            _ => self.thread = None,
        }
        self.pending.clear();
    }

    /// The core's binding ended: a binding that ended needs a new reveal.
    fn end_binding(&mut self) {
        self.bound = None;
        self.thread = None;
        self.pending.clear();
    }

    fn new() -> Att {
        Att {
            phase: Phase::Opening,
            server: None,
            pending: Vec::new(),
            bound: None,
            thread: None,
            handed: VecDeque::new(),
            reply_records_evicted: false,
        }
    }
}

/// Where the adapter's carrier stands ([`CodexAdapter::carrier_state`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarrierState {
    /// No carrier was designated and taken yet.
    None,
    /// The designated connection is being upgraded and initialized.
    Connecting,
    /// The carrier is open.
    Up,
    /// The carrier failed to start, or its connection ended: a new one may be designated.
    Lost,
}

enum Carrier {
    /// No carrier connection yet.
    None,
    /// The designated connection is being upgraded and initialized.
    Connecting,
    /// Up.
    Up(CodexPairing<Attachment>),
    /// Failed to start, or its connection ended: a new designation replaces it.
    Lost,
}

type Streams = (Box<dyn Read + Send>, Box<dyn Write + Send>);

struct State {
    handler: Option<AdapterEventHandler>,
    sink: Option<Arc<dyn RequestSink>>,
    designated: Option<ConnectionHandle>,
    carrier: Carrier,
    carrier_handle: Option<ConnectionHandle>,
    carrier_thread: Option<JoinHandle<()>>,
    attachments: HashMap<Attachment, Att>,
    /// Shim connections that arrived while the carrier was connecting.
    pending: Vec<(Attachment, Streams)>,
}

/// What every connection thread shares with the adapter.
struct Inner {
    state: Mutex<State>,
    /// Held while an event is reported, so that an attachment's `attachment-opened` always
    /// reaches the core before its `attachment-closed`. No operation the core may call from
    /// inside the handler (`set_binding`, `capabilities`) takes it.
    events: crate::gate::Serial,
    /// Set under the state lock when shutdown begins. New calls refuse; calls admitted
    /// settle before shutdown returns ([IFC-ADP-070]).
    down: AtomicBool,
    calls: RwLock<()>,
    config: Config,
    options: Options,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The `CallPort` the pairing talks to. It holds the adapter weakly, so that the pairing
/// (owned by the adapter's state) and the adapter do not keep each other alive.
struct Port(Weak<Inner>);

impl Port {
    fn with<T>(&self, f: impl FnOnce(&Inner) -> T) -> Option<T> {
        self.0.upgrade().map(|i| f(&i))
    }
}

impl CallPort<Attachment> for Port {
    fn request(&self, key: &Attachment, call: ToolCall) -> Answer {
        self.with(|i| i.request(key, call))
            .unwrap_or_else(|| Inner::plain(ErrorCode::InternalError))
    }

    fn unconfirmed(&self, key: &Attachment) {
        self.with(|i| i.unconfirmed(key));
    }

    fn reveal(&self, key: &Attachment, reveal: Reveal) {
        self.with(|i| i.reveal(key, &reveal));
    }

    fn listed(&self, key: &Attachment) {
        self.with(|i| i.listed(key));
    }

    fn ended(&self, key: &Attachment) {
        self.with(|i| i.ended(key));
    }

    fn carrier_closed(&self) {
        self.with(Inner::carrier_closed);
    }
}

impl Inner {
    /// An answer made here, not by the core: a refusal with `code`.
    fn plain(code: ErrorCode) -> Answer {
        Answer {
            result: oac_mcp_tools::refusal(code),
            pairing_value: None,
            unauthorized: code == ErrorCode::Unauthorized,
        }
    }

    fn emit(&self, handler: Option<AdapterEventHandler>, event: AdapterEvent) {
        if let Some(h) = handler {
            h(event);
        }
    }

    /// The first `tools/list` answer is written: the attachment is open.
    fn listed(&self, a: &Attachment) {
        let _ev = self.events.enter();
        let handler = {
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
            st.handler.clone()
        };
        self.emit(
            handler,
            AdapterEvent::AttachmentOpened {
                attachment: a.clone(),
                cross_check: None,
            },
        );
    }

    /// The connection's server ended.
    fn ended(&self, a: &Attachment) {
        let _ev = self.events.enter();
        let handler = {
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
            handler,
            AdapterEvent::AttachmentClosed {
                attachment: a.clone(),
            },
        );
    }

    /// A request the pairing could not confirm: reported before the request reaches the
    /// core ([IFC-ADP-091]). The pairing calls [`Inner::request`] only after this returns.
    fn unconfirmed(&self, a: &Attachment) {
        let _ev = self.events.enter();
        let handler = {
            if self.down.load(Ordering::SeqCst) {
                return;
            }
            let st = lock(&self.state);
            if !st
                .attachments
                .get(a)
                .is_some_and(|att| att.phase == Phase::Open)
            {
                return;
            }
            st.handler.clone()
        };
        self.emit(
            handler,
            AdapterEvent::AttachmentUnconfirmed {
                attachment: a.clone(),
            },
        );
    }

    /// Codex reported a call whose result is a pairing refusal of `a`: a `native-signal` on the
    /// carrier whose `native_id` is the notification's `threadId`. The thread is kept for
    /// `a`, uncommitted, before the event: the core can call `set_binding` from inside the
    /// handler.
    fn reveal(&self, a: &Attachment, reveal: &Reveal) {
        let _ev = self.events.enter();
        let (handler, carrier) = {
            if self.down.load(Ordering::SeqCst) {
                return;
            }
            let mut st = lock(&self.state);
            let Some(att) = st.attachments.get_mut(a) else {
                return;
            };
            att.revealed(reveal.native_id());
            (st.handler.clone(), st.carrier_handle.clone())
        };
        self.emit(
            handler,
            AdapterEvent::NativeSignal(NativeSignal {
                native_id: reveal.native_id().to_owned(),
                // Every reveal is `fresh` (`spec/bindings/mcp.md` §4.5.3), and Codex reports
                // no cross-check value.
                start_kind: Some(StartKind::Fresh),
                cross_check: None,
                connection: carrier,
                revealed: Some(reveal.value().to_owned()),
            }),
        );
    }

    /// The carrier closed: `capabilities` for every open attachment now reports no active
    /// inbound ([IFC-ADP-043]).
    fn carrier_closed(&self) {
        let _ev = self.events.enter();
        let (handler, open) = {
            if self.down.load(Ordering::SeqCst) {
                return;
            }
            let st = lock(&self.state);
            let open: Vec<Attachment> = st
                .attachments
                .iter()
                .filter(|(_, att)| att.phase == Phase::Open)
                .map(|(a, _)| a.clone())
                .collect();
            (st.handler.clone(), open)
        };
        for attachment in open {
            self.emit(
                handler.clone(),
                AdapterEvent::CapabilitiesChanged { attachment },
            );
        }
    }

    fn render_send(r: &oac_core::adapter::SendRequestResult) -> Answer {
        Answer {
            result: oac_mcp_tools::send_result(r),
            pairing_value: r.pairing_value().map(str::to_owned),
            unauthorized: r.error() == Some(ErrorCode::Unauthorized),
        }
    }

    /// A tool call from Codex on attachment `a`, passed to the core's sink unchanged and
    /// answered with the core's result unchanged ([IFC-ADP-003], [IFC-ADP-060]). The request
    /// is labelled with the connection it came on, never with a session the call claims
    /// ([IFC-ADP-031]).
    fn request(&self, a: &Attachment, call: ToolCall) -> Answer {
        let _call = self.calls.read().unwrap_or_else(|e| e.into_inner());
        if self.down.load(Ordering::SeqCst) {
            // Answered without reaching the core ([IFC-ADP-070]).
            return Inner::plain(ErrorCode::InternalError);
        }
        let (sink, bound, record) = {
            let st = lock(&self.state);
            let att = st.attachments.get(a);
            let record = match &call {
                // A reply that names no `in_reply_to` has no record to look up: it names its
                // `to` ([`oac_mcp_tools::parse`]) and goes uncorrelated.
                ToolCall::Reply {
                    in_reply_to: Some(in_reply_to),
                    ..
                } => att.and_then(|x| {
                    if x.reply_records_evicted {
                        return None;
                    }
                    let mut senders = x
                        .handed
                        .iter()
                        .filter(|(id, _, to, _)| id == in_reply_to && Some(to) == x.bound.as_ref())
                        .map(|(_, from, _, completed)| (from, *completed));
                    let (first, mut completed) = senders.next()?;
                    // Ids are unique only per sender [SC-ENV-027]: never choose the newest
                    // sender, ambiguity requires an explicit destination.
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
            return Inner::plain(ErrorCode::InternalError);
        };
        match call {
            ToolCall::Send {
                to,
                content,
                conversation_id,
                correlation_id,
            } => Inner::render_send(&sink.send(oac_mcp_tools::send_request(
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
                // [MCPB-ATT-002], [SC-RCP-090]. An explicit `to` still goes to the core.
                if to.is_none() && bound.is_none() {
                    return Inner::plain(ErrorCode::Unauthorized);
                }
                match to.or(record) {
                    Some(to) => Inner::render_send(&sink.send(oac_mcp_tools::reply_request(
                        a.clone(),
                        to,
                        content,
                        in_reply_to,
                    ))),
                    // No `to`, and no record of the message on this attachment.
                    None => Inner::plain(ErrorCode::InvalidRequest),
                }
            }
            ToolCall::ListSessions => {
                let r = sink.discover(DiscoveryRequest {
                    attachment: a.clone(),
                });
                Answer {
                    result: oac_mcp_tools::discovery_result(&r),
                    pairing_value: r.pairing_value().map(str::to_owned),
                    unauthorized: matches!(
                        r,
                        oac_core::adapter::DiscoveryRequestResult::Refused {
                            error: ErrorCode::Unauthorized,
                            ..
                        }
                    ),
                }
            }
            // [IFC-ADP-094]: the identity comes from an `IdentityRequest` attributed to this
            // attachment, never from a value this adapter holds.
            ToolCall::Whoami => {
                let r = sink.identity(IdentityRequest {
                    attachment: a.clone(),
                });
                Answer {
                    result: oac_mcp_tools::whoami_result(&r),
                    pairing_value: r.pairing_value().map(str::to_owned),
                    unauthorized: matches!(
                        r,
                        oac_core::adapter::IdentityRequestResult::Refused {
                            error: ErrorCode::Unauthorized,
                            ..
                        }
                    ),
                }
            }
        }
    }

    /// Serves attachment `a`'s MCP connection on its own thread. The attachment has been
    /// registered; if it was removed meanwhile (shutdown) the connection is closed.
    fn serve_shim(&self, a: &Attachment, streams: Streams, pairing: CodexPairing<Attachment>) {
        let (reader, writer) = streams;
        match serve_connection(a.clone(), reader, writer, pairing) {
            Ok(handle) => {
                let mut st = lock(&self.state);
                match st.attachments.get_mut(a) {
                    Some(att) if !self.down.load(Ordering::SeqCst) => att.server = Some(handle),
                    _ => {
                        drop(st);
                        handle.stop();
                        a.close();
                        handle.join();
                    }
                }
            }
            Err(_) => {
                lock(&self.state).attachments.remove(a);
                a.close();
            }
        }
    }

    /// The carrier connection finished starting.
    fn carrier_started(
        self: &Arc<Inner>,
        handle: &ConnectionHandle,
        started: Result<CodexPairing<Attachment>, ClientError>,
    ) {
        let (pairing, pending) = {
            let mut st = lock(&self.state);
            match started {
                Ok(p) if !self.down.load(Ordering::SeqCst) => {
                    st.carrier = Carrier::Up(p.clone());
                    (Some(p), std::mem::take(&mut st.pending))
                }
                other => {
                    // Shut down meanwhile, or the upgrade or `initialize` failed.
                    if let Ok(p) = &other {
                        p.shutdown();
                        p.client().shutdown();
                    }
                    st.carrier = Carrier::Lost;
                    let pending = std::mem::take(&mut st.pending);
                    for (a, _) in &pending {
                        st.attachments.remove(a);
                    }
                    drop(st);
                    for (a, _) in &pending {
                        a.close();
                    }
                    handle.close();
                    return;
                }
            }
        };
        if let Some(p) = pairing {
            for (a, streams) in pending {
                self.serve_shim(&a, streams, p.clone());
            }
        }
    }
}

/// The Codex App Server adapter.
pub struct CodexAdapter {
    inner: Arc<Inner>,
}

impl Default for CodexAdapter {
    fn default() -> Self {
        CodexAdapter::new()
    }
}

impl CodexAdapter {
    /// An adapter for the server registered with Codex as [`DEFAULT_REGISTERED_NAME`], with
    /// the reference pairing settings and connection options.
    pub fn new() -> CodexAdapter {
        CodexAdapter::with_config(Config::new(DEFAULT_REGISTERED_NAME), Options::default())
    }

    /// An adapter with the given pairing settings (the registered name, the windows and
    /// waits) and carrier connection options.
    pub fn with_config(config: Config, options: Options) -> CodexAdapter {
        CodexAdapter {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    handler: None,
                    sink: None,
                    designated: None,
                    carrier: Carrier::None,
                    carrier_handle: None,
                    carrier_thread: None,
                    attachments: HashMap::new(),
                    pending: Vec::new(),
                }),
                events: crate::gate::Serial::default(),
                down: AtomicBool::new(false),
                calls: RwLock::new(()),
                config,
                options,
            }),
        }
    }

    /// Designates `handle` as the carrier: the connection to Codex's app-server. Only the
    /// daemon calls this (`cli/`, through the same step that registers the connection with the
    /// core as a carrier, `oac_core::pipeline::Pipelines::connect_carrier`), after it
    /// accepted and authenticated the connection and **before** it gives the connection to
    /// `take_connection`. Nothing a message, a signal or a tool call says designates one
    /// ([MCPB-ATT-006]).
    ///
    /// One carrier serves the adapter at a time. A designation is refused (`false`) while a
    /// carrier is connecting or open, and after the adapter shut down. It is accepted when no
    /// carrier was taken yet, and **again after the carrier was lost** ([`CarrierState::Lost`]:
    /// its start failed, or its connection ended), so that the daemon can connect a new one the
    /// next time a Codex shim connects (`docs/planning/v0.1/11-risks.md` row 90). A designation
    /// replaces an earlier one that was never taken. The lost carrier's pairing and client are
    /// shut down; the attachments that were served through it keep their place until their own
    /// connections end, but cannot hand anything off (their threads are not served by the new
    /// carrier) and pair nothing.
    pub fn designate_carrier(&self, handle: &ConnectionHandle) -> bool {
        let (old_pairing, old_handle) = {
            let mut st = lock(&self.inner.state);
            if self.inner.down.load(Ordering::SeqCst) {
                return false;
            }
            let lost_pairing = match &st.carrier {
                Carrier::None => None,
                Carrier::Lost => None,
                Carrier::Up(p) if !p.client().is_open() => Some(p.clone()),
                Carrier::Connecting | Carrier::Up(_) => return false,
            };
            let old_handle = if matches!(st.carrier, Carrier::None) {
                None
            } else {
                st.carrier_handle.take()
            };
            st.carrier = Carrier::None;
            st.designated = Some(handle.clone());
            (lost_pairing, old_handle)
        };
        if let Some(p) = old_pairing {
            p.shutdown();
            p.client().shutdown();
        }
        if let Some(h) = old_handle {
            h.close();
        }
        true
    }

    /// Where the carrier stands. [`CarrierState::Lost`] is a carrier that failed to start and
    /// one whose connection ended after it was up; the daemon designates a new one then, on the
    /// next Codex shim connection (never on a timer).
    pub fn carrier_state(&self) -> CarrierState {
        match &lock(&self.inner.state).carrier {
            Carrier::None => CarrierState::None,
            Carrier::Connecting => CarrierState::Connecting,
            Carrier::Up(p) if p.client().is_open() => CarrierState::Up,
            Carrier::Up(_) | Carrier::Lost => CarrierState::Lost,
        }
    }

    /// How many threads the carrier serves for pairing and how many it has unsubscribed
    /// (counts only), when the carrier is up.
    pub fn serving_counts(&self) -> Option<(usize, usize)> {
        match &lock(&self.inner.state).carrier {
            Carrier::Up(p) => Some(p.serving_counts()),
            _ => None,
        }
    }

    /// The pairing's counts, when the carrier is up.
    pub fn pairing_diagnostics(&self) -> Option<crate::pairing::PairingDiagnostics> {
        match &lock(&self.inner.state).carrier {
            Carrier::Up(p) => Some(p.diagnostics()),
            _ => None,
        }
    }

    /// End the pairing windows that have ended, at once. For tests that move no clock.
    pub fn sweep(&self) {
        let pairing = match &lock(&self.inner.state).carrier {
            Carrier::Up(p) => Some(p.clone()),
            _ => None,
        };
        if let Some(p) = pairing {
            p.sweep();
        }
    }
}

/// The hand-off outcome for the app-server's answer to the add (see the table in the module
/// documentation).
pub fn outcome_of(result: &Result<(), ClientError>) -> HandOffOutcome {
    match result {
        Ok(()) => HandOffOutcome::Completed,
        // The app-server turned the add away: a recorded refusal says nothing like "not now"
        // (`spec/bindings/mcp.md` §8.2.1, [SC-DLV-009]).
        Err(ClientError::Refused { .. }) => HandOffOutcome::Failed,
        // The request may have been sent and acted on, or not: neither completed nor failed
        // ([IFC-ADP-053]).
        Err(
            ClientError::NoAnswer
            | ClientError::LostAnswer
            | ClientError::Connect(_)
            | ClientError::Shape(_),
        ) => HandOffOutcome::Indeterminate,
        // Nothing was sent.
        Err(
            ClientError::Closed
            | ClientError::NotServed
            | ClientError::NotAllowed(_)
            | ClientError::InvalidThreadId,
        ) => HandOffOutcome::Failed,
    }
}

impl ProviderAdapter for CodexAdapter {
    fn connection_limit(&self) -> usize {
        SHIM_PLACES + CARRIER_PLACES
    }

    fn shutdown_bound_ms(&self) -> u64 {
        SHUTDOWN_BOUND_MS
    }

    fn take_connection(&self, connection: Connection) {
        let (handle, reader, writer) = connection.into_parts();
        let inner = &self.inner;
        let _call = inner.calls.read().unwrap_or_else(|e| e.into_inner());
        let mut st = lock(&inner.state);
        if inner.down.load(Ordering::SeqCst) {
            drop(st);
            handle.close();
            return;
        }
        // The carrier is the handle the daemon designated, and nothing else.
        if st.designated.as_ref() == Some(&handle) {
            if !matches!(st.carrier, Carrier::None) {
                drop(st);
                handle.close();
                return;
            }
            st.carrier = Carrier::Connecting;
            st.carrier_handle = Some(handle.clone());
            let me = inner.clone();
            let h = handle.clone();
            let spawned = std::thread::Builder::new()
                .name("oac-codex-carrier-start".into())
                .spawn(move || {
                    let port: Arc<dyn CallPort<Attachment>> = Arc::new(Port(Arc::downgrade(&me)));
                    let started = CodexPairing::start(me.config.clone(), port, |events| {
                        CodexClient::connect(reader, writer, &me.options, events)
                    });
                    me.carrier_started(&h, started);
                });
            match spawned {
                Ok(t) => st.carrier_thread = Some(t),
                Err(_) => {
                    st.carrier = Carrier::Lost;
                    drop(st);
                    handle.close();
                }
            }
            return;
        }
        let shims = st.attachments.len();
        if shims >= SHIM_PLACES {
            drop(st);
            handle.close();
            return;
        }
        match &st.carrier {
            Carrier::Up(p) => {
                let p = p.clone();
                st.attachments.insert(handle.clone(), Att::new());
                drop(st);
                inner.serve_shim(&handle, (reader, writer), p);
            }
            Carrier::Connecting => {
                st.attachments.insert(handle.clone(), Att::new());
                st.pending.push((handle, (reader, writer)));
            }
            // No carrier, or a lost one: nothing could confirm or pair a call, so the
            // connection is not served (Codex sees its MCP server fail to start).
            Carrier::None | Carrier::Lost => {
                drop(st);
                handle.close();
            }
        }
    }

    fn watch_attachments(&self, handler: AdapterEventHandler) {
        lock(&self.inner.state).handler = Some(handler);
    }

    fn set_binding(&self, attachment: &Attachment, session: Option<SessionId>) {
        let (pairing, bound) = {
            let mut st = lock(&self.inner.state);
            let pairing = match &st.carrier {
                Carrier::Up(p) => Some(p.clone()),
                _ => None,
            };
            let Some(att) = st.attachments.get_mut(attachment) else {
                return;
            };
            let bound = session.is_some();
            match session {
                Some(session) => att.commit_binding(session),
                None => att.end_binding(),
            }
            (pairing, bound)
        };
        if let Some(p) = pairing {
            p.set_bound(attachment, bound);
        }
    }

    fn capabilities(&self, _attachment: &Attachment) -> AdapterCapabilities {
        let up = match &lock(&self.inner.state).carrier {
            Carrier::Up(p) => p.client().is_open(),
            _ => false,
        };
        AdapterCapabilities {
            // Hand-off is a `thread/queue/add` through the carrier, pushed into the live
            // session's queue (`active_inbound` only while the carrier is open).
            active_inbound: up,
            // Text only; no size is stated, since the surface documents none.
            content_types: Vec::new(),
            max_envelope_octets: None,
        }
    }

    /// One `thread/queue/add` for the attachment's thread, or none ([IFC-ADP-057]).
    ///
    /// `completed` means the app-server accepted the add: the input is held by Codex, which
    /// runs it when its thread is idle, so the core records `handed-to-harness`, never that
    /// the model saw it ([IFC-ADP-051]; `spec/bindings/mcp.md` §8.2.1).
    fn deliver(&self, hand_off: HandOff) -> HandOffOutcome {
        let inner = &self.inner;
        let _call = inner.calls.read().unwrap_or_else(|e| e.into_inner());
        if inner.down.load(Ordering::SeqCst) {
            return HandOffOutcome::Failed;
        }
        let message = hand_off.message();
        let text = match frame::frame(message) {
            Ok(t) => t,
            Err(FrameRefusal::Unverified | FrameRefusal::Content) => return HandOffOutcome::Failed,
            // [SEC-PRV-003], [SEC-PRV-014]: refused, with no call.
            Err(FrameRefusal::Value(_)) => return HandOffOutcome::Refused,
        };
        let (pairing, native) = {
            let st = lock(&inner.state);
            let Some(att) = st.attachments.get(hand_off.attachment()) else {
                return HandOffOutcome::Failed;
            };
            // [IFC-ADP-030]: never to an attachment the core's latest binding left unbound.
            if att.phase != Phase::Open || att.bound.is_none() {
                return HandOffOutcome::Failed;
            }
            match (&st.carrier, &att.thread) {
                (Carrier::Up(p), Some(t)) => (p.clone(), t.clone()),
                _ => return HandOffOutcome::Failed,
            }
        };
        let client = pairing.client();
        // The thread must still be served: its subscription is held by this attachment's
        // pairing (see the module documentation). A thread that is not is not asked.
        let Some(key) = client.key_of(&native) else {
            return HandOffOutcome::Failed;
        };
        // Include indeterminate attempts: the input may still run.
        {
            let env = message.envelope();
            if let Some(att) = lock(&inner.state)
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
        let id = client.next_client_user_message_id();
        let outcome = outcome_of(&client.queue_add(key, &text, &id));
        if outcome == HandOffOutcome::Completed {
            let env = message.envelope();
            if let Some(att) = lock(&inner.state)
                .attachments
                .get_mut(hand_off.attachment())
            {
                for (rid, from, to, completed) in &mut att.handed {
                    if rid == env.id().as_str() && from == env.from() && to == env.to() {
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
            return HealthStatus::new(HealthState::Unavailable);
        }
        match &lock(&self.inner.state).carrier {
            Carrier::None => HealthStatus::with_detail(
                HealthState::Healthy,
                "no Codex app-server connection yet",
            ),
            Carrier::Connecting => HealthStatus::with_detail(
                HealthState::Healthy,
                "connecting to the Codex app-server",
            ),
            Carrier::Up(p) => p.client().health(),
            Carrier::Lost => HealthStatus::with_detail(
                HealthState::Unavailable,
                "the Codex app-server connection could not be started",
            ),
        }
    }

    fn shutdown(&self) {
        let inner = &self.inner;
        let (handles, pairing) = {
            let st = lock(&inner.state);
            inner.down.store(true, Ordering::SeqCst);
            let mut handles: Vec<ConnectionHandle> = st.attachments.keys().cloned().collect();
            handles.extend(st.carrier_handle.clone());
            let pairing = match &st.carrier {
                Carrier::Up(p) => Some(p.clone()),
                _ => None,
            };
            (handles, pairing)
        };
        // Revoke I/O and waits before waiting for calls that might be blocked on them: a wait
        // for a confirmation returns refused, and an in-flight add returns at once.
        if let Some(p) = &pairing {
            p.shutdown();
            p.client().shutdown();
        }
        for h in &handles {
            h.begin_close();
        }
        for h in &handles {
            h.close();
        }
        drop(inner.calls.write().unwrap_or_else(|e| e.into_inner()));
        let events = inner.events.enter();
        let (handler, open, servers, carrier_thread) = {
            let mut st = lock(&inner.state);
            let mut open = Vec::new();
            let mut servers = Vec::new();
            for (a, att) in st.attachments.iter_mut() {
                if att.phase == Phase::Open {
                    open.push(a.clone());
                }
                att.phase = Phase::Closed;
                servers.extend(att.server.take());
            }
            st.pending.clear();
            (st.handler.clone(), open, servers, st.carrier_thread.take())
        };
        // No state lock across a callback; handlers may re-enter.
        for attachment in open {
            inner.emit(
                handler.clone(),
                AdapterEvent::AttachmentClosed { attachment },
            );
        }
        drop(events);
        for s in &servers {
            s.stop();
        }
        for s in servers {
            s.join();
        }
        if let Some(t) = carrier_thread
            && t.thread().id() != std::thread::current().id()
        {
            let _ = t.join();
        }
        lock(&inner.state).attachments.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(n: u8) -> SessionId {
        // Parsed, not minted: an adapter's source mints no session id ([IFC-ADP-007]).
        SessionId::parse(if n == 1 {
            "01harn7x9k2m4p6q8r0s2t4v6w"
        } else {
            "7gq3m8z2c5k9t1w4x6b0n2r8vd"
        })
        .unwrap()
    }

    /// The commit follows the reveals since the last decision: one thread commits, a second
    /// thread before the decision commits nothing (it could be a forged reveal, [MCPB-ATT-011]),
    /// no reveal leaves the committed thread, and an ended binding needs a new reveal.
    #[test]
    fn the_core_decision_commits_only_an_unambiguous_thread() {
        let mut a = Att::new();
        a.revealed("t1");
        a.revealed("t1");
        a.commit_binding(session(1));
        assert_eq!(a.thread.as_deref(), Some("t1"));
        // Re-bound with no new reveal: unchanged.
        a.commit_binding(session(1));
        assert_eq!(a.thread.as_deref(), Some("t1"));
        // Two threads before one decision: nothing is committed, not even the earlier one.
        a.revealed("t2");
        a.revealed("t3");
        a.commit_binding(session(1));
        assert_eq!(a.thread, None);
        assert!(a.pending.is_empty());
        // Ended: a binding that ended needs a new reveal.
        a.revealed("t4");
        a.commit_binding(session(2));
        assert_eq!(a.thread.as_deref(), Some("t4"));
        a.end_binding();
        a.commit_binding(session(2));
        assert_eq!(a.thread, None);
        // Bounded.
        for i in 0..20 {
            a.revealed(&format!("t{i}"));
        }
        assert_eq!(a.pending.len(), PENDING_KEPT);
    }

    /// The table of the module documentation: every answer of the add maps to one outcome.
    #[test]
    fn the_add_maps_to_the_outcomes_of_section_8_2_1() {
        use crate::ws::WsError;
        let refused = || ClientError::Refused {
            code: -32600,
            message: "session x is archived".into(),
        };
        let rows: [(Result<(), ClientError>, HandOffOutcome); 11] = [
            (Ok(()), HandOffOutcome::Completed),
            (Err(refused()), HandOffOutcome::Failed),
            (Err(ClientError::NoAnswer), HandOffOutcome::Indeterminate),
            (Err(ClientError::LostAnswer), HandOffOutcome::Indeterminate),
            (
                Err(ClientError::Connect(WsError::Closed)),
                HandOffOutcome::Indeterminate,
            ),
            (
                Err(ClientError::Shape("ThreadQueueAddResponse")),
                HandOffOutcome::Indeterminate,
            ),
            (Err(ClientError::Closed), HandOffOutcome::Failed),
            (Err(ClientError::NotServed), HandOffOutcome::Failed),
            (Err(ClientError::InvalidThreadId), HandOffOutcome::Failed),
            (
                Err(ClientError::NotAllowed("turn/steer".into())),
                HandOffOutcome::Failed,
            ),
            (Err(refused()), HandOffOutcome::Failed),
        ];
        for (result, want) in rows {
            assert_eq!(outcome_of(&result), want, "{result:?}");
        }
        // Never `not-now` and never `refused` for an answer: `not-now` is the evidence of a
        // surface that says it cannot take input now, which no add refusal says.
        for e in [
            ClientError::NoAnswer,
            ClientError::LostAnswer,
            ClientError::Closed,
            refused(),
        ] {
            assert!(!matches!(
                outcome_of(&Err(e)),
                HandOffOutcome::NotNow | HandOffOutcome::Refused | HandOffOutcome::Completed
            ));
        }
    }
}
