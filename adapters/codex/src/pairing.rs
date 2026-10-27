// SPDX-License-Identifier: Apache-2.0

//! The adapter's half of the Codex issued-value pairing (`spec/bindings/mcp.md` §4.5; G8, #69,
//! #403): thread serving, refused-call records, reveal detection, and the confirmation of
//! every call on a paired connection.
//!
//! The core owns the pairing itself: it issues each value, ends each window, uses each value
//! once, caps the windows, and binds ([`oac_core::issued_pairing`], interfaces 0.2 §5.6). This
//! module does what only the adapter can, because it alone sees Codex's own reports on the
//! carrier ([`CodexClient`]):
//!
//! | Step | Where |
//! |---|---|
//! | A connection that is not paired makes a call: serve the loaded threads, then ask the core | [`CodexPairing::serve`], [`crate::serving`] |
//! | The core refused it with a pairing value: keep a record of the refused call | [`crate::reveal::Refusals`] |
//! | Codex reports a call whose result is that refusal: report a reveal ([`CallPort::reveal`]) | [`CodexPairing::on_event`] |
//! | The connection is paired: serve a call only on a live confirmation from its thread | [`crate::confirm::Confirmations`] |
//! | No confirmation in the wait: report `attachment-unconfirmed` first, then ask the core | [`CallPort::unconfirmed`] |
//!
//! **The seam to the provider adapter.** [`CallPort`] is what the `ProviderAdapter` of G8's
//! next slice implements (S4): passing a parsed call to the core's request sink, reporting a
//! reveal as a `native-signal` whose `revealed` is the value (interfaces §4.10), and reporting
//! `attachment-unconfirmed` before the request it concerns ([IFC-ADP-091]). This module
//! implements none of `ProviderAdapter`. It is generic over the connection key `K` (S4 uses
//! the core's `Attachment`) so that it is tested without a core connection.
//!
//! **What it never does.** It never answers a call itself except by refusing it
//! (`unauthorized`); it signs, verifies and binds nothing ([IFC-ADP-002]); it reads no
//! `_meta`, no rollout file and no credential; it sends only the allowlisted app-server
//! methods of [`crate::client`]; and it never calls `turn/start` or `turn/steer`
//! ([MCPB-CDX-003], [MCPB-CDX-004]). The thread of a reveal is the notification's `threadId`
//! of a subscribed thread, nothing a call carries ([MCPB-ATT-008], [MCPB-ATT-018]).
//!
//! **Fail closed.** Every failure here costs availability, never authority: a call with no
//! live confirmation is refused ([MCPB-ATT-015]); a refusal whose pairing cannot complete
//! (no thread to serve) carries no value ([MCPB-ATT-022]); a report that does not equal the
//! refusal is not a reveal.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::Hash;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use oac_core::json::Json;
use oac_mcp_tools::{ToolCall, single_text};
use rmcp::model::CallToolResult;

use crate::client::{ClientError, CodexClient, EventHandler};
use crate::confirm::{Bounded, CONFIRM_LIFE, CONFIRM_WAIT, Confirmations};
use crate::events::{CodexEvent, CompletedItem};
use crate::reveal::{Found, Refusals, single_text_content};
use crate::serving::{Clock, Serving};
use crate::threads::{NativeRef, ThreadKey};

/// The default pairing window, as the core's (`PipelineConfig::pairing_window`, 30 s;
/// [MCPB-ATT-019] allows up to 60 s). Set the core's and this one from one value.
pub const WINDOW: Duration = Duration::from_secs(30);

/// The longest a call may wait for its confirmation ([MCPB-ATT-020]); a longer
/// [`Config::confirm_wait`] is cut to it.
pub const MAX_CONFIRM_WAIT: Duration = Duration::from_secs(10);

/// The longest a pairing window may last ([MCPB-ATT-019]); a longer [`Config::window`] is cut
/// to it.
pub const MAX_WINDOW: Duration = Duration::from_secs(60);

/// The most claimed confirmations kept for the content check after the call was served.
pub const MAX_CLAIMS: usize = 1024;

/// The longest registered name this crate accepts. Codex sets no length limit on the name of
/// an MCP server; this is OAC's bound, so that a name that cannot be a real registration is
/// refused at start and a name in a log line is short.
pub const MAX_REGISTERED_NAME: usize = 64;

/// The most distinct other-server names whose first sight is logged; later ones are only
/// counted.
const MAX_LOGGED_OTHER_SERVERS: usize = 16;

/// Where the pairing writes its few diagnostic lines (the daemon's log). A line names no
/// thread, no pairing value and nothing a call carries.
pub type LogFn = Arc<dyn Fn(&str) + Send + Sync>;

/// Why a registered name was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisteredNameError {
    /// The name is empty.
    Empty,
    /// The name is longer than [`MAX_REGISTERED_NAME`] bytes.
    TooLong,
    /// The name holds a character outside ASCII letters, digits and `-`, `_`, `:`, `@`, `/`,
    /// `.` (what `codex mcp add` accepts in a server name).
    BadCharacter,
}

impl fmt::Display for RegisteredNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the registered name is empty"),
            Self::TooLong => write!(
                f,
                "the registered name is longer than {MAX_REGISTERED_NAME} characters"
            ),
            Self::BadCharacter => f.write_str(
                "the registered name may hold only ASCII letters, digits and '-', '_', ':', '@', '/', '.'",
            ),
        }
    }
}

impl std::error::Error for RegisteredNameError {}

fn name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '@' | '/' | '.')
}

/// Check a registered name: non-empty, at most [`MAX_REGISTERED_NAME`] bytes, and only the
/// characters `codex mcp add` accepts in an MCP server name (ASCII letters and digits, and
/// `-`, `_`, `:`, `@`, `/`, `.`; first-party source `codex-rs/cli/src/mcp_cmd.rs`
/// `validate_server_name`, read 2026-10-26 on the main branch: UNVERIFIED against a Codex
/// build, and this check is only an early refusal of a name that cannot be a registration).
///
/// # Errors
///
/// The first rule the name breaks.
pub fn validate_registered_name(name: &str) -> Result<(), RegisteredNameError> {
    if name.is_empty() {
        Err(RegisteredNameError::Empty)
    } else if name.len() > MAX_REGISTERED_NAME {
        Err(RegisteredNameError::TooLong)
    } else if !name.chars().all(name_char) {
        Err(RegisteredNameError::BadCharacter)
    } else {
        Ok(())
    }
}

/// A name Codex reported, made safe and short for a log line: characters outside the allowed
/// set become `?`, and it is cut to [`MAX_REGISTERED_NAME`] characters.
fn sanitised(name: &str) -> String {
    let mut out: String = name
        .chars()
        .take(MAX_REGISTERED_NAME)
        .map(|c| if name_char(c) { c } else { '?' })
        .collect();
    if name.chars().count() > MAX_REGISTERED_NAME {
        out.push_str("...");
    }
    out
}

/// How the pairing is set. [`Config::new`] gives the reference values of the spec's
/// reference implementation note: a 30 s window, a 5 s wait and a 600 s confirmation life.
#[derive(Clone)]
pub struct Config {
    /// The name the operator registered the OAC server under with Codex (`codex mcp add
    /// <name> ...`); Codex reports it as an item's `server`. Only items of this server are
    /// reveals or confirmations.
    pub registered_name: String,
    /// The pairing window; the core's must not be longer than this.
    pub window: Duration,
    /// How long a call on a paired connection waits for its confirmation ([MCPB-ATT-020]:
    /// at most 10 s).
    pub confirm_wait: Duration,
    /// How long a confirmation lives ([MCPB-ATT-021]).
    pub confirm_life: Duration,
    clock: Clock,
    serving_thread: bool,
    log: Option<LogFn>,
}

impl Config {
    /// The reference settings for a server registered as `registered_name`.
    pub fn new(registered_name: &str) -> Config {
        Config {
            registered_name: registered_name.to_owned(),
            window: WINDOW,
            confirm_wait: CONFIRM_WAIT,
            confirm_life: CONFIRM_LIFE,
            clock: Arc::new(Instant::now),
            serving_thread: true,
            log: None,
        }
    }

    /// [`Config::new`] for a name checked with [`validate_registered_name`].
    ///
    /// # Errors
    ///
    /// The first rule the name breaks.
    pub fn try_new(registered_name: &str) -> Result<Config, RegisteredNameError> {
        validate_registered_name(registered_name)?;
        Ok(Config::new(registered_name))
    }

    /// Write the pairing's few diagnostic lines (a window opened or not, a call from a server
    /// that is not the registered one, a reveal for a thread the carrier does not serve) to
    /// `log`. Without it they are not written; the counts ([`PairingDiagnostics`]) are kept
    /// either way. A line holds no thread id and no pairing value.
    #[must_use]
    pub fn with_log(mut self, log: LogFn) -> Config {
        self.log = Some(log);
        self
    }

    /// Do not start the thread that ends pairing windows by itself: they end only on
    /// [`CodexPairing::sweep`]. For tests that move their own clock, where a thread reading
    /// that clock would race the test.
    #[must_use]
    pub fn without_serving_thread(mut self) -> Config {
        self.serving_thread = false;
        self
    }

    /// Replace the time source. For tests that need to move time without waiting; the waits
    /// themselves (a call waiting for its confirmation) still take real time, bounded by
    /// [`Config::confirm_wait`].
    #[must_use]
    pub fn with_clock(mut self, clock: Clock) -> Config {
        self.clock = clock;
        self
    }
}

/// The core's answer to a call, as the adapter renders it for Codex.
pub struct Answer {
    /// The result to return to Codex: the core's, rendered by `oac-mcp-tools` unchanged
    /// ([IFC-ADP-060]); a refusal that carries a pairing value is a pairing refusal
    /// ([`oac_mcp_tools::pairing_refusal`]).
    pub result: CallToolResult,
    /// The pairing value the core issued for this refusal, when it did.
    pub pairing_value: Option<String>,
    /// True when the core refused the call with `unauthorized`.
    pub unauthorized: bool,
}

/// A harness report that carries a pairing value back: what the adapter tells the core, as a
/// `native-signal` whose `native_id` is [`Reveal::native_id`] and whose `revealed` is
/// [`Reveal::value`] (interfaces §4.10).
pub struct Reveal {
    native_id: String,
    value: String,
}

impl Reveal {
    /// The harness-native thread id: the `threadId` of the notification ([MCPB-ATT-008]).
    pub fn native_id(&self) -> &str {
        &self.native_id
    }

    /// The pairing value the report carried.
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Debug shows neither: a thread id is native ([IFC-TYP-092]) and a value is a credential of
/// one window.
impl fmt::Debug for Reveal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reveal").finish_non_exhaustive()
    }
}

/// What the pairing needs from the provider adapter. `K` names a connection.
pub trait CallPort<K>: Send + Sync + 'static {
    /// Pass `call`, which arrived on connection `key`, to the core's request sink and return
    /// its answer, rendered. Called after any confirmation the connection needs.
    fn request(&self, key: &K, call: ToolCall) -> Answer;

    /// Report `attachment-unconfirmed` for `key` ([IFC-ADP-091]). Called before the
    /// [`CallPort::request`] of the call that could not be confirmed.
    fn unconfirmed(&self, key: &K);

    /// Report a reveal for `key` as a `native-signal` ([IFC-ADP-020]). Called on the
    /// carrier's reader thread, with no lock of this module held; it must not block long.
    fn reveal(&self, key: &K, reveal: Reveal);

    /// The connection listed its tools: it may be reported open. Called once.
    fn listed(&self, _key: &K) {}

    /// The connection ended.
    fn ended(&self, _key: &K) {}

    /// The carrier closed: the app-server went away, or its stream ended. Nothing more
    /// arrives on it, so no connection can be confirmed or paired from now on. Called once,
    /// on the carrier's reader thread, with no lock of this module held; not called for a
    /// client that its owner shut down.
    fn carrier_closed(&self) {}
}

/// Counts of what the pairing did or refused. Counts only: nothing here names a thread, a
/// value or a call, and all of it may go in `health`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PairingDiagnostics {
    /// Windows opened: calls on an unpaired connection that served threads first.
    pub windows: u64,
    /// Calls refused without a pairing value because no thread could be served
    /// ([MCPB-ATT-022]).
    pub unpairable: u64,
    /// Reveals reported.
    pub reveals: u64,
    /// Reports that matched a refused call older than the window: dropped ([MCPB-ATT-009]).
    pub late_reveals: u64,
    /// Calls served on a confirmation.
    pub confirmed: u64,
    /// Calls with no live confirmation in the wait: refused ([MCPB-ATT-015]).
    pub unconfirmed: u64,
    /// Served calls whose reported content differed from what was returned ([MCPB-ATT-017]).
    pub content_mismatches: u64,
    /// `mcpToolCall` items Codex reported complete whose `server` is not the registered name:
    /// none of them is a reveal or a confirmation. A count that grows while the operator's
    /// calls are never paired means the name given to `codex mcp add` differs from the
    /// registered name this adapter was set with.
    pub other_server: u64,
    /// Completed calls of the registered server whose result matched no refused call and no
    /// served call: not a reveal, and not explained by a confirmation either.
    pub unmatched_reports: u64,
    /// Reveals dropped because the thread they came from is not served by the carrier.
    pub unknown_threads: u64,
}

#[derive(Default)]
struct Conn {
    /// The thread of the last reveal on this connection.
    thread: Option<ThreadKey>,
    /// The core bound this connection ([`CodexPairing::set_bound`]).
    bound: bool,
    /// A call was not confirmed: no call is served until a reveal ([MCPB-ATT-016]).
    stopped: bool,
    /// Threads were served for this connection until this instant.
    window_until: Option<Instant>,
}

struct Claimed<K> {
    key: K,
    expected: Option<String>,
}

enum Route {
    /// Not paired (or stopped): ask the core, which refuses with a pairing value.
    Unbound { list: bool },
    /// Bound, with a thread: the call needs a confirmation from it.
    Confirm(ThreadKey),
    /// Bound at the core but this module holds no thread for it: not confirmable.
    Missing,
}

struct Inner<K> {
    cfg: Config,
    port: Arc<dyn CallPort<K>>,
    client: OnceLock<CodexClient>,
    serving: OnceLock<Serving<K>>,
    confirmations: Confirmations,
    conns: Mutex<HashMap<K, Conn>>,
    refusals: Mutex<Refusals<K>>,
    claims: Mutex<Bounded<Claimed<K>>>,
    diag: Mutex<PairingDiagnostics>,
    /// The other-server names already logged.
    other_servers: Mutex<HashSet<String>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The pairing of the Codex connections of one carrier.
pub struct CodexPairing<K> {
    inner: Arc<Inner<K>>,
}

impl<K> Clone for CodexPairing<K> {
    fn clone(&self) -> Self {
        CodexPairing {
            inner: self.inner.clone(),
        }
    }
}

impl<K> fmt::Debug for CodexPairing<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodexPairing").finish_non_exhaustive()
    }
}

impl<K: Clone + Eq + Hash + Send + Sync + 'static> CodexPairing<K> {
    /// Build the pairing and its carrier client together: `connect` is given the handler that
    /// feeds the pairing the carrier's events, and returns the connected client
    /// ([`CodexClient::connect`]).
    ///
    /// # Errors
    ///
    /// `connect`'s error.
    pub fn start(
        mut cfg: Config,
        port: Arc<dyn CallPort<K>>,
        connect: impl FnOnce(EventHandler) -> Result<CodexClient, ClientError>,
    ) -> Result<CodexPairing<K>, ClientError> {
        // The bounds of the spec are not settings: a longer value is cut to them.
        cfg.window = cfg.window.clamp(Duration::from_millis(1), MAX_WINDOW);
        cfg.confirm_wait = cfg.confirm_wait.min(MAX_CONFIRM_WAIT);
        cfg.confirm_life = cfg.confirm_life.min(CONFIRM_LIFE);
        let inner = Arc::new(Inner {
            confirmations: Confirmations::new(cfg.confirm_life),
            refusals: Mutex::new(Refusals::new(cfg.window)),
            claims: Mutex::new(Bounded::new(MAX_CLAIMS)),
            cfg,
            port,
            client: OnceLock::new(),
            serving: OnceLock::new(),
            conns: Mutex::default(),
            diag: Mutex::default(),
            other_servers: Mutex::default(),
        });
        let weak = Arc::downgrade(&inner);
        let handler: EventHandler = Arc::new(move |e| {
            if let Some(i) = weak.upgrade() {
                i.on_event(e);
            }
        });
        let client = connect(handler)?;
        let serving = Serving::new(
            client.clone(),
            inner.cfg.window,
            inner.cfg.clock.clone(),
            inner.cfg.serving_thread,
        );
        let _ = inner.client.set(client);
        let _ = inner.serving.set(serving);
        Ok(CodexPairing { inner })
    }

    /// The settings in force, after the bounds of the spec were applied.
    pub fn config(&self) -> &Config {
        &self.inner.cfg
    }

    /// The carrier's client.
    pub fn client(&self) -> &CodexClient {
        self.inner.client()
    }

    /// Serve one parsed call of `tool` on connection `key`, with the call's `arguments`
    /// ([`oac_mcp_tools::arguments_json`]; `None` when unreadable). Blocks, up to
    /// [`Config::confirm_wait`] on a paired connection and for one `thread/loaded/list` and
    /// the subscriptions on an unpaired one, so the caller runs it off its I/O thread.
    pub fn serve(
        &self,
        key: &K,
        tool: &str,
        arguments: Option<Json>,
        call: ToolCall,
    ) -> CallToolResult {
        self.inner.serve(key, tool, arguments, call)
    }

    /// The core bound (`true`) or unbound (`false`) connection `key`: the provider adapter's
    /// `set_binding`. Only a bound connection needs confirmations; a connection the core
    /// unbound forgets its thread and releases its subscription.
    pub fn set_bound(&self, key: &K, bound: bool) {
        self.inner.set_bound(key, bound);
    }

    /// The connection ended: its records and its thread hold go.
    pub fn connection_closed(&self, key: &K) {
        self.inner.connection_closed(key);
    }

    /// Feed one carrier event. The handler [`CodexPairing::start`] installs does this; it is
    /// public for a carrier that delivers events another way.
    pub fn on_event(&self, e: CodexEvent) {
        self.inner.on_event(e);
    }

    /// End the windows that have ended: unsubscribe the threads no reveal held. Done by
    /// the serving thread at each window's end; callable at once by a caller with its own
    /// clock.
    pub fn sweep(&self) {
        let now = (self.inner.cfg.clock)();
        self.inner.serving().sweep(now);
    }

    /// The connection listed its tools (the first `tools/list` answer was written).
    pub(crate) fn port_listed(&self, key: &K) {
        self.inner.port.listed(key);
    }

    /// The connection's server ended: forget it, and tell the port.
    pub(crate) fn connection_closed_and_ended(&self, key: &K) {
        self.inner.connection_closed(key);
        self.inner.port.ended(key);
    }

    /// The counts.
    pub fn diagnostics(&self) -> PairingDiagnostics {
        *lock(&self.inner.diag)
    }

    /// How many threads the serving manager holds subscribed for pairing, and how many it has
    /// since unsubscribed.
    pub fn serving_counts(&self) -> (usize, usize) {
        let s = self.inner.serving();
        (s.managed(), s.unsubscribed())
    }

    /// Stop waiting: every call waiting for a confirmation returns now, refused, and the
    /// serving thread stops. The client is shut down by its owner.
    pub fn shutdown(&self) {
        self.inner.confirmations.close();
        self.inner.serving().shutdown();
    }
}

impl<K: Clone + Eq + Hash + Send + Sync + 'static> Inner<K> {
    fn now(&self) -> Instant {
        (self.cfg.clock)()
    }

    /// One diagnostic line, when the configuration has somewhere to write it. Never called
    /// with a lock of this module held.
    fn log(&self, line: &str) {
        if let Some(log) = &self.cfg.log {
            log(line);
        }
    }

    /// Codex reported an `mcpToolCall` of a server that is not the registered one: it is
    /// neither a reveal nor a confirmation. Counted once per completed item (`count`), and
    /// logged once per distinct name.
    fn other_server(&self, server: &str, count: bool) {
        if count {
            lock(&self.diag).other_server += 1;
        }
        let first = {
            let mut seen = lock(&self.other_servers);
            seen.len() < MAX_LOGGED_OTHER_SERVERS && seen.insert(server.to_owned())
        };
        if first {
            self.log(&format!(
                "an mcpToolCall from server {}, not the registered name {}: it pairs nothing \
                 (the name given to `codex mcp add` must equal the registered name)",
                sanitised(server),
                sanitised(&self.cfg.registered_name)
            ));
        }
    }

    fn client(&self) -> &CodexClient {
        self.client.get().expect("the client is set before any use")
    }

    fn serving(&self) -> &Serving<K> {
        self.serving
            .get()
            .expect("the serving manager is set before any use")
    }

    fn serve(
        &self,
        key: &K,
        tool: &str,
        arguments: Option<Json>,
        call: ToolCall,
    ) -> CallToolResult {
        // Arguments that are not I-JSON match nothing, and a pairing refusal could never be
        // revealed for them: refuse them as invalid, without asking the core.
        let Some(args) = arguments else {
            return oac_mcp_tools::refusal(oac_core::delivery::ErrorCode::InvalidRequest);
        };
        let now = self.now();
        let route = {
            let mut conns = lock(&self.conns);
            let c = conns.entry(key.clone()).or_default();
            // A thread from a reveal is enough to need a confirmation, whatever `bound` says:
            // the core binds inside `CallPort::reveal`, before `set_bound` arrives, and a call
            // in between must not reach a bound core unconfirmed ([MCPB-ATT-013]). If the core
            // did not bind, it refuses the confirmed call and `unpair` follows.
            match (c.bound, c.stopped, c.thread) {
                (_, false, Some(t)) => Route::Confirm(t),
                (true, false, None) => Route::Missing,
                _ => Route::Unbound {
                    list: c.window_until.is_none_or(|u| now >= u),
                },
            }
        };
        match route {
            Route::Confirm(thread) => self.confirmed_call(key, thread, tool, args, call),
            Route::Missing => self.unconfirmed_call(key, tool, args, call),
            Route::Unbound { list } => self.unbound_call(key, tool, args, call, list, now),
        }
    }

    /// A call on a connection that is not paired: the threads are served first, so that the
    /// reveal can arrive on a subscription, and then the core refuses it.
    fn unbound_call(
        &self,
        key: &K,
        tool: &str,
        args: Json,
        call: ToolCall,
        list: bool,
        now: Instant,
    ) -> CallToolResult {
        if list {
            match self.serving().open_window(now) {
                Ok(o) if o.served > 0 => {
                    if let Some(c) = lock(&self.conns).get_mut(key) {
                        c.window_until = Some(now + self.cfg.window);
                    }
                    lock(&self.diag).windows += 1;
                    self.log(&format!(
                        "a Codex pairing window opened: {} threads subscribed, {} served, {} failed",
                        o.subscribed, o.served, o.failed
                    ));
                }
                // No thread to pair with, or the listing failed: the pairing cannot complete,
                // so the refusal carries no value and the core opens no window
                // ([MCPB-ATT-022]).
                other => {
                    lock(&self.diag).unpairable += 1;
                    match other {
                        Ok(o) => self.log(&format!(
                            "a Codex call is unpairable, no pairing window: no thread served \
                             ({} subscribed, {} failed)",
                            o.subscribed, o.failed
                        )),
                        Err(_) => self.log(
                            "a Codex call is unpairable, no pairing window: the loaded-thread \
                             listing failed",
                        ),
                    }
                    return oac_mcp_tools::refusal(oac_core::delivery::ErrorCode::Unauthorized);
                }
            }
        }
        let answer = self.port.request(key, call);
        self.keep_record(key, tool, args, &answer, now);
        answer.result
    }

    /// A call on a paired connection: served only on a live confirmation from its thread.
    fn confirmed_call(
        &self,
        key: &K,
        thread: ThreadKey,
        tool: &str,
        args: Json,
        call: ToolCall,
    ) -> CallToolResult {
        let clock = &*self.cfg.clock;
        let claim =
            self.confirmations
                .claim_wait(thread, tool, &args, self.cfg.confirm_wait, clock);
        let Some(claim) = claim else {
            return self.unconfirmed_call(key, tool, args, call);
        };
        lock(&self.diag).confirmed += 1;
        let answer = self.port.request(key, call);
        // The content check of [MCPB-ATT-017] happens when Codex reports the item complete,
        // which is after this result is returned.
        lock(&self.claims).insert(
            (claim.thread, claim.item.native().to_owned()),
            Claimed {
                key: key.clone(),
                expected: single_text(&answer.result).map(str::to_owned),
            },
        );
        if answer.unauthorized {
            // The core does not serve this connection although this module held a thread for
            // it: the pairing is over; the next call starts again.
            self.unpair(key);
        }
        self.keep_record(key, tool, args, &answer, self.now());
        answer.result
    }

    /// No live confirmation: report it first ([IFC-ADP-091]), stop the connection until a
    /// reveal ([MCPB-ATT-016]), and let the core refuse the call ([MCPB-ATT-015]).
    fn unconfirmed_call(&self, key: &K, tool: &str, args: Json, call: ToolCall) -> CallToolResult {
        lock(&self.diag).unconfirmed += 1;
        if let Some(c) = lock(&self.conns).get_mut(key) {
            c.stopped = true;
        }
        self.port.unconfirmed(key);
        let answer = self.port.request(key, call);
        self.keep_record(key, tool, args, &answer, self.now());
        answer.result
    }

    /// Keep a record of a refusal that carries a pairing value, so that Codex's report of
    /// the call can be recognised as its reveal.
    fn keep_record(&self, key: &K, tool: &str, args: Json, answer: &Answer, now: Instant) {
        if let (Some(value), Some(text)) = (&answer.pairing_value, single_text(&answer.result)) {
            lock(&self.refusals).record(key, tool, args, text, value, now);
        }
    }

    fn unpair(&self, key: &K) {
        if let Some(c) = lock(&self.conns).get_mut(key) {
            c.bound = false;
            c.thread = None;
            c.stopped = false;
        }
        self.serving().release(key);
    }

    fn set_bound(&self, key: &K, bound: bool) {
        if bound {
            lock(&self.conns).entry(key.clone()).or_default().bound = true;
        } else {
            self.unpair(key);
        }
    }

    fn connection_closed(&self, key: &K) {
        lock(&self.conns).remove(key);
        lock(&self.refusals).forget(key);
        self.serving().release(key);
    }

    fn on_event(&self, e: CodexEvent) {
        if self.client.get().is_none() {
            return;
        }
        let now = self.now();
        let name = self.cfg.registered_name.as_str();
        match e {
            CodexEvent::ItemStarted {
                thread,
                turn,
                item,
                call: Some(c),
                ..
            } if c.server == name => {
                self.confirmations.started(
                    thread,
                    &turn,
                    &item,
                    &c.tool,
                    c.arguments.as_ref(),
                    now,
                );
            }
            CodexEvent::TurnCompleted { thread, turn, .. } => {
                self.confirmations.turn_completed(thread, &turn);
            }
            CodexEvent::Closed => self.port.carrier_closed(),
            // A call of some other server: no reveal and no confirmation. Named in the log,
            // since a registration under another name than the registered one otherwise
            // fails without a sign. Counted once per item, when it completes.
            CodexEvent::ItemStarted { call: Some(c), .. } => self.other_server(&c.server, false),
            CodexEvent::ItemCompleted {
                thread,
                item:
                    CompletedItem::McpToolCall {
                        id,
                        server,
                        tool,
                        arguments,
                        result,
                    },
                ..
            } if server == name => {
                self.call_completed(thread, &id, &tool, arguments.as_ref(), result.as_ref(), now);
            }
            CodexEvent::ItemCompleted {
                item: CompletedItem::McpToolCall { server, .. },
                ..
            } => self.other_server(&server, true),
            _ => {}
        }
    }

    /// Codex reports a call of the registered server complete: it may be a reveal
    /// ([MCPB-ATT-007]), it ends the item's confirmation ([MCPB-ATT-021]), and it is the
    /// content check of a served call ([MCPB-ATT-017]).
    fn call_completed(
        &self,
        thread: ThreadKey,
        id: &NativeRef,
        tool: &str,
        arguments: Option<&Json>,
        result: Option<&Json>,
        now: Instant,
    ) {
        let reported = result.and_then(single_text_content);
        if let Some(text) = reported {
            let found = lock(&self.refusals).find(tool, arguments, text, now);
            match found {
                Found::Reveal { key, value } => self.reveal(&key, thread, value),
                Found::Late => lock(&self.diag).late_reveals += 1,
                // The report of a served call has a claim and matches no refusal; one with
                // neither is a report of a call this adapter did not refuse or serve.
                Found::None => {
                    if !lock(&self.claims).contains(&(thread, id.native().to_owned())) {
                        lock(&self.diag).unmatched_reports += 1;
                    }
                }
            }
        }
        let claimed = lock(&self.claims).remove(&(thread, id.native().to_owned()));
        if let Some(Claimed {
            key,
            expected: Some(expected),
        }) = claimed
            && reported != Some(expected.as_str())
        {
            // The server's own content is not what Codex reports for the confirmed call:
            // treated as a missing confirmation, after the fact ([MCPB-ATT-017]).
            lock(&self.diag).content_mismatches += 1;
            if let Some(c) = lock(&self.conns).get_mut(&key) {
                c.stopped = true;
            }
            self.port.unconfirmed(&key);
        }
        self.confirmations.completed(thread, id);
    }

    /// Report a reveal of `value` for `key`, naming `thread`.
    fn reveal(&self, key: &K, thread: ThreadKey, value: String) {
        let Some(native_id) = self.client().native_of(thread) else {
            lock(&self.diag).unknown_threads += 1;
            self.log("a Codex reveal was dropped: its thread is not served by the carrier");
            return;
        };
        {
            let mut conns = lock(&self.conns);
            let c = conns.entry(key.clone()).or_default();
            c.thread = Some(thread);
            // A reveal lifts the stop of [MCPB-ATT-016]; the core lifts its own on pairing.
            c.stopped = false;
        }
        self.serving().hold(key, thread);
        lock(&self.diag).reveals += 1;
        self.port.reveal(key, Reveal { native_id, value });
    }
}
