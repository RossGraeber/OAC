// SPDX-License-Identifier: Apache-2.0

//! The adapter contract suite (#59, F10): one suite, written against
//! `dyn oac_core::adapter::ProviderAdapter`, that every adapter runs unchanged against the
//! fake harnesses (`spec/interfaces.md` §5, Table 5.2; frozen at revision 0.1).
//!
//! # Shape
//!
//! The suite plays the core: a [`CoreSide`] accepts the local connections, collects the
//! adapter's events, answers its requests with scripted results, and builds verified
//! hand-offs. An [`AdapterHarness`] plays the harness: it owns a fake harness, opens live
//! sessions on it, makes the harness busy, has it send requests, and reports what the fake
//! observed ([`Observations`]). The adapter sits between them, exactly as in the product.
//!
//! - [`claude::ClaudeHarness`] is a complete harness over the fake Claude Code channel
//!   endpoint (F8): on the channel path the harness's connection carries the adapter's MCP
//!   traffic, so any channel adapter plugs in as it is. Run it under both
//!   [`oac_fake_claude::MidTurnRelease`] settings ([`oac_fake_claude::MidTurnRelease::BOTH`]).
//! - [`codex::CodexFake`] spawns the fake Codex app-server (F9) and reads its call log; a
//!   Codex adapter's harness wires its sessions to it ([`codex::PROFILE`] names the holding
//!   hand-off and the steering operations of `spec/bindings/mcp.md` §8.2.1).
//!
//! # What it checks
//!
//! [`run`] returns one [`Row`] per check, under the requirement id it evidences: every
//! requirement Appendix C of `spec/interfaces.md` gives the `adapter` in area `IFC-ADP`
//! ([IFC-ADP-010]), the no-polling assertion of `docs/planning/v0.1/09-test-strategy.md`
//! §5 (named test `contract/adapter/no-polling`, under [IFC-ADP-040]), the never-steer and
//! holding hand-off rules ([SEC-AUZ-022], [SEC-AUZ-025] to [SEC-AUZ-027]; for the Codex
//! profile [MCPB-CDX-002] to [MCPB-CDX-005]), the outcome a binding gives a harness's
//! refusal ([SC-DLV-009] for the Codex profile, `spec/bindings/mcp.md` §8.2.1; see
//! [`Profile::turned_away_code`]), [IFC-NEU-002], and the adapter halves of
//! [IFC-TYP-090] to [IFC-TYP-092]. "Routes through core policy rather than straight to a
//! transport" is asserted twice: every harness request must reach the core's request sink
//! ([IFC-ADP-003]), and the adapter's source must not reach a transport, sign or verify, or
//! decide bindings ([`source`]; [IFC-ADP-001], [IFC-ADP-002], [IFC-ADP-007]).
//!
//! A check the fake harness cannot exercise is [`Verdict::NotApplicable`] with the reason,
//! never a pass.
//!
//! # What a harness cannot change (#351)
//!
//! The harness is written by the adapter's task, so the suite does not take its word for
//! the binding. Each route below fails rather than drops a check:
//!
//! - **The profile.** The binding is identified from the operations of the hand-off calls
//!   the fake recorded ([`identify_binding`] over [`BINDINGS`]), and every check runs under
//!   the suite's own profile for it. A harness profile that differs from it (a weakened
//!   copy, another binding's, or one no binding has) fails [IFC-ADP-010]
//!   `harness-profile-is-the-bindings`; so does a run whose hand-off calls identify no
//!   single binding. Later hand-off calls off that binding's surface fail [IFC-ADP-010]
//!   `hand-offs-on-one-binding`.
//! - **A [`Gap`] where the binding's profile makes the step mandatory.** Each
//!   `Gap`-returning step, and the rows a gap there would make not applicable:
//!
//!   | Step | Rows | Mandatory when | Claude | Codex |
//!   |---|---|---|---|---|
//!   | `refuse_hand_offs` | [SEC-AUZ-027], [SC-DLV-009] | [`Profile::turned_away_code`] names a code | n/a: a channel notification gets no answer, so there is no refusal | mandatory: the recorded archived refusal |
//!   | `start_turn` | [SEC-AUZ-025], [SEC-AUZ-026] | [`Profile::runs_own_turns`] | mandatory | mandatory |
//!   | `request` | [IFC-ADP-003], [IFC-ADP-031], [IFC-ADP-060] | [`Profile::makes_requests`] | mandatory | n/a: the fake app-server does not model the MCP tool path Codex requests travel |
//!   | `open_session`, `session_ready`, `end_session`, `drain`, `allow_hand_offs` | the whole run | always | the run fails | the run fails |
//!
//!   A [`Gap::Broken`] step always fails.
//! - **A `start_turn` that starts nothing.** Neither message handed off to the busy session
//!   may become input before [`AdapterHarness::drain`]; one that does fails [SEC-AUZ-025]
//!   and [SEC-AUZ-026].
//! - **Omitted inputs.** Every adapter has source, so an empty
//!   [`AdapterHarness::source_files`] fails [IFC-ADP-001], [IFC-ADP-002], [IFC-ADP-007] and
//!   [IFC-ADP-013] rather than making the static checks not applicable. Every fake harness
//!   knows native ids once a session is open, so an empty [`AdapterHarness::native_ids`]
//!   (a required method) fails [IFC-TYP-092].
//!
//! # Where a harness lives
//!
//! An adapter takes this suite as a dev-dependency, and the suite never depends on an
//! adapter: 07 section 3, `scripts/check-crate-deps.mjs` rule 5, and this crate's
//! `Cargo.toml`. So **the harness a real adapter runs under lives in that adapter, at
//! exactly one fixed path: `adapters/<name>/tests/contract.rs`**. It may use what this
//! crate gives it, such as [`claude::ClaudeHarness`] or [`codex::CodexFake`].
//!
//! The suite's own checks bound what a harness can do, as above. What they cannot see is a
//! harness that fabricates or filters [`Observations`], or returns `Ok` from a step it did
//! not do in a way the observations do not show. That residual is covered by reviewing
//! the one fixed file. Gate S4 evidence for an adapter must cite it, at the commit run.
//! The review covers everything `contract.rs` calls outside this suite, including the
//! adapter's own `src/` helpers. A hostile edit to an allowed dependency (`oac-core` or
//! `oac-fake-claude`) that re-exports an `include` macro, e.g. `pub use std::include as load`,
//! is covered by review like any other hostile edit to reviewed code; plain `include_str!`
//! in `oac-fake-claude` stays allowed.
//!
//! `tests/harness_location.rs` keeps the rule mechanical:
//! - Only an adapter, `adapters/mcp-tools` and its listed packages (`oac-transport-memory`)
//!   depend on this crate. The adapter and the tool crate do so as a dev-dependency only,
//!   and none under another name.
//! - In an adapter, only `tests/contract.rs` names `AdapterHarness` or this crate, or
//!   reaches [`run`]. No other file outside this crate and the listed packages names
//!   either.
//! - The harness file and an adapter's other test files hold no `self as` import,
//!   `macro_rules!`, `include!` or `#[path]`. The harness file also holds no file module and
//!   no glob or renamed import.
//! - A listed package may drive the fakes' harnesses, but may not implement or rename
//!   `AdapterHarness`, reach [`run`], or hold those shapes.
//! - An adapter takes no dev-dependency but this crate, `oac-core` and `oac-fake-claude`.
//!   This excludes an identifier-pasting proc macro.
//! - The files read are git's (`git ls-files -co --exclude-standard`), not a directory
//!   walk's, minus untracked files under `target/`. Any `CACHEDIR.TAG` among them fails.
//!   Cargo's target directory is `target/` or outside the repository.
//! - What cargo compiles for an adapter is checked too:
//!   - no workspace member has a build script;
//!   - every test, example and bench target is the harness file or passes the name rules;
//!   - no `.rs` file under `adapters/` is git-ignored;
//!   - the lib target has `doctest = false`, and its `src/` files are held to the hiding
//!     shapes;
//!   - every adapter file, of any extension, is held to the name rule;
//!   - its normal and build dependencies are the vetted ones.
//!
//! Those are text rules. What really bounds a listed transport is
//! `scripts/check-crate-deps.mjs`: a transport can never reach an adapter, so a harness
//! hidden in it could drive only a stand-in.

pub mod claude;
pub mod codex;
#[doc(hidden)]
pub mod plant;
pub mod source;

use oac_core::connection::{ConnectionReader, ConnectionWriter};
use std::collections::VecDeque;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use oac_core::adapter::{
    AdapterEvent, AdapterEventHandler, Attachment, Connection, ConnectionHandle, DiscoveryRequest,
    DiscoveryRequestResult, HandOff, HandOffOutcome, ProviderAdapter, ReceiptStream, RequestSink,
    SendRequest, SendRequestResult,
};
use oac_core::clock::{Clock, SystemClock};
use oac_core::delivery::ErrorCode;
use oac_core::envelope::{
    ChannelMessage, EnvelopeDraft, EnvelopeLimits, TextPart, receive_envelope,
};
use oac_core::ids::{SessionId, Token};
use oac_core::keys::{DeviceIdentity, DeviceKey};
use oac_core::signing::authenticate;
use oac_core::trust::TrustedKeySet;

/// How long the suite waits for something the adapter does on its own threads.
pub const WAIT: Duration = Duration::from_secs(10);

// ---- harness interface ----------------------------------------------------------------

/// Why a harness step did not happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gap {
    /// The fake harness cannot do this (with why); the checks that need it are not
    /// applicable.
    Unsupported(String),
    /// The step failed.
    Broken(String),
}

impl fmt::Display for Gap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Gap::Unsupported(w) => write!(f, "unsupported: {w}"),
            Gap::Broken(w) => write!(f, "{w}"),
        }
    }
}

/// The result of a harness step.
pub type Step<T> = Result<T, Gap>;

/// One call the adapter made on the harness's input surface to hand content off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandOffCall {
    /// The surface operation (the harness's own name for it).
    pub operation: String,
    /// The text the call carried.
    pub text: String,
    /// True when the binding classes the operation as steering ([SEC-AUZ-022]).
    pub steering: bool,
    /// Members of the call that set a thread or turn setting ([MCPB-CDX-005]).
    pub override_members: Vec<String>,
    /// True when the harness took the call (it did not refuse it).
    pub accepted: bool,
}

/// What a fake harness observed of one session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Observations {
    /// Every hand-off call the adapter made for this session, in order.
    pub hand_off_calls: Vec<HandOffCall>,
    /// Every hand-off call the adapter made on the harness's surface, for any session or
    /// none (a call naming no thread, or another thread, included). The never-steer and
    /// binding rules read this list.
    pub client_hand_off_calls: Vec<HandOffCall>,
    /// The text of each input the session took into a turn, in order. The harness leaves
    /// out input it made itself (its own user's turns).
    pub inputs: Vec<String>,
    /// Input the harness holds and has not yet taken into a turn.
    pub held: usize,
    /// Calls that establish an attachment or a subscription (the first no-polling class).
    pub establishment_calls: usize,
    /// Calls that ask for pending messages and return them on demand (the second class).
    pub fetch_calls: Vec<String>,
    /// Inbox-style surfaces the adapter offers the harness to read repeatedly
    /// ([MCPB-DLV-002] on the MCP surface).
    pub offered_fetch_surfaces: Vec<String>,
    /// The fake stopped on conduct no fixture records, with what.
    pub halted: Option<String>,
}

/// A request the suite has the harness make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HarnessRequest {
    /// Send `text` to `to`. `claimed_from` is a session id the harness asserts as its own:
    /// a claim the core must never use ([SC-ID-162], [IFC-ADP-031]).
    Send {
        /// The destination.
        to: SessionId,
        /// The text.
        text: String,
        /// A spoofed sender, when set.
        claimed_from: Option<SessionId>,
    },
    /// Ask for the discovery result.
    Discover,
}

/// What the harness got back for a request, as the binding carries it: whether it is an
/// error, and its text (which carries the code, or the id and state; [MCPB-TOOL-006],
/// [MCPB-TOOL-011]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedResult {
    /// True for an error result.
    pub is_error: bool,
    /// The result's text.
    pub text: String,
}

/// A rule of a harness profile, checked over every hand-off call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// Every hand-off call uses the holding hand-off.
    OnlyHolding,
    /// No hand-off call uses this operation.
    Never(&'static str),
    /// No hand-off call carries an override member.
    NoOverrides,
}

/// What the adapter binding document says of the harness's input surface
/// ([IFC-ADP-080]): its holding hand-off, its steering operations, and the binding rules
/// over them, each under its requirement id.
///
/// The suite owns one profile per binding ([`BINDINGS`]). A harness's
/// [`AdapterHarness::profile`] is only compared with it: the suite identifies the binding
/// from the hand-off calls the fake recorded ([`identify_binding`]), runs every check under
/// that binding's own profile, and fails [IFC-ADP-010] `harness-profile-is-the-bindings`
/// when the harness's profile differs (#351).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    /// The harness.
    pub name: &'static str,
    /// Every operation of the harness's input surface that the fake records as a hand-off
    /// call. The suite identifies the binding by these, never by what the harness claims.
    pub surface_operations: &'static [&'static str],
    /// The holding hand-off, if the surface has one ([SEC-AUZ-025]).
    pub holding_hand_off: Option<&'static str>,
    /// The steering operations ([SEC-AUZ-022]).
    pub steering_operations: &'static [&'static str],
    /// Binding rules, as (requirement id, rule).
    pub rules: &'static [(&'static str, Rule)],
    /// The code the binding gives the refusal that [`AdapterHarness::refuse_hand_offs`]
    /// makes the harness answer, when the harness can refuse a hand-off call at all. The
    /// suite checks the adapter's outcome for that refusal against it through Table 5.3 of
    /// `spec/interfaces.md` ([`HandOffOutcome::recorded`]): `handoff-failed`
    /// ([SC-DLV-009]) or `destination-unavailable` ([SC-DLV-008]).
    ///
    /// When it names a code, the harness can refuse, so `refuse_hand_offs` returning a
    /// [`Gap`] fails [SEC-AUZ-027] and [SC-DLV-009] rather than making them not applicable.
    pub turned_away_code: Option<ErrorCode>,
    /// True when the harness's sessions run turns of their own, so that a hand-off can meet
    /// a running turn ([SEC-AUZ-025], [SEC-AUZ-026]). When true, `start_turn` returning a
    /// [`Gap`] fails those rows rather than making them not applicable.
    pub runs_own_turns: bool,
    /// True when the harness makes its requests (send, discovery) through the surface its
    /// fake models, so that the suite can drive them ([IFC-ADP-003], [IFC-ADP-031],
    /// [IFC-ADP-060]). When true, `request` returning a [`Gap`] fails those rows rather than
    /// making them not applicable.
    pub makes_requests: bool,
}

/// The suite's own profile of every binding it knows, one per binding.
pub const BINDINGS: [Profile; 2] = [claude::PROFILE, codex::PROFILE];

/// The binding whose input surface every operation in `operations` belongs to, from
/// [`BINDINGS`]: the operations of the hand-off calls the fake recorded. Fails when there
/// is no operation, or no single binding has them all.
pub fn identify_binding<'a>(
    operations: impl IntoIterator<Item = &'a str>,
) -> Result<Profile, String> {
    let mut ops: Vec<&str> = operations.into_iter().collect();
    ops.sort_unstable();
    ops.dedup();
    if ops.is_empty() {
        return Err(
            "the fake recorded no hand-off call, so the binding cannot be identified".into(),
        );
    }
    let fits: Vec<Profile> = BINDINGS
        .iter()
        .filter(|p| ops.iter().all(|op| p.surface_operations.contains(op)))
        .copied()
        .collect();
    match fits.as_slice() {
        [p] => Ok(*p),
        [] => Err(format!(
            "no binding's input surface has every hand-off operation the fake recorded: {ops:?}"
        )),
        many => Err(format!(
            "hand-off operations {ops:?} fit more than one binding: {:?}",
            many.iter().map(|p| p.name).collect::<Vec<_>>()
        )),
    }
}

/// What an adapter under test supplies: the adapter, wired to a fake harness.
pub trait AdapterHarness {
    /// A short name for reports.
    fn name(&self) -> String;

    /// The adapter under test, the same one for the harness's life.
    fn adapter(&self) -> Arc<dyn ProviderAdapter>;

    /// The harness profile. It must be the suite's own profile for the binding
    /// ([`BINDINGS`]); the suite checks under its own and fails a profile that differs.
    fn profile(&self) -> Profile;

    /// Start one live session on the fake harness and return its index and the local
    /// connections it opened, each accepted through `core` ([`CoreSide::accept`]). The
    /// suite passes each to `take_connection`.
    fn open_session(&mut self, core: &CoreSide) -> Step<(usize, Vec<Connection>)>;

    /// Wait until the session's harness side has finished opening with the adapter.
    fn session_ready(&mut self, s: usize) -> Step<()>;

    /// End the session from the harness side.
    fn end_session(&mut self, s: usize) -> Step<()>;

    /// Make the session busy: a turn of the harness's own starts. Hand-offs made next must
    /// not become input until [`AdapterHarness::drain`]; one that does fails [SEC-AUZ-025]
    /// and [SEC-AUZ-026].
    fn start_turn(&mut self, s: usize) -> Step<()>;

    /// Release everything the harness holds and let every turn end.
    fn drain(&mut self, s: usize) -> Step<()>;

    /// Make the harness turn away the next hand-off call, with the refusal that
    /// [`Profile::turned_away_code`] classifies (a Codex harness: the recorded archived
    /// refusal, through [`codex::CodexFake::set_archived`]).
    fn refuse_hand_offs(&mut self, s: usize) -> Step<()>;

    /// Undo [`AdapterHarness::refuse_hand_offs`].
    fn allow_hand_offs(&mut self, s: usize) -> Step<()>;

    /// Have the harness make `request` through its supported surface.
    fn request(&mut self, s: usize, request: &HarnessRequest) -> Step<ObservedResult>;

    /// What the fake observed of session `s`.
    fn observe(&mut self, s: usize) -> Observations;

    /// The adapter's source files, for the static checks of [`source`]
    /// ([`source::crate_files`] gives a crate's `src/` and `build.rs`). None at all fails
    /// those checks.
    fn source_files(&self) -> Vec<PathBuf>;

    /// The harness-native ids the fake harness knows for its sessions (thread ids,
    /// session ids): none of them may appear in the adapter's `health` detail
    /// ([IFC-TYP-092]). Every fake harness knows some once a session is open, so none at
    /// all (or only empty ones) fails that row.
    fn native_ids(&self) -> Vec<String>;
}

// ---- the core side ----------------------------------------------------------------------

/// The result the core side gives the next send request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scripted {
    /// `sent`.
    Send,
    /// `refused` with this code.
    Refuse(ErrorCode),
    /// `not-passed` with this code.
    NotPass(ErrorCode),
}

/// A request the adapter passed into the sink.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// A `SendRequest`.
    Send(SendRequest),
    /// A `DiscoveryRequest`.
    Discovery(DiscoveryRequest),
}

impl Request {
    /// The attachment the request is labelled with.
    pub fn attachment(&self) -> &Attachment {
        match self {
            Request::Send(r) => &r.attachment,
            Request::Discovery(r) => &r.attachment,
        }
    }
}

/// One request the adapter passed into the sink, and what the core side answered.
#[derive(Clone, Debug)]
pub struct RecordedRequest {
    /// The request.
    pub request: Request,
    /// The outcome returned: `refused`, `not-passed` or `sent` for a send (§4.10), and
    /// `discovery-result` for a discovery (the suite's own label).
    pub outcome: &'static str,
    /// The code returned, if any.
    pub error: Option<ErrorCode>,
    /// The envelope id returned, if any.
    pub id: Option<String>,
}

/// The suite's stand-in for the core.
pub struct CoreSide {
    identity: DeviceIdentity,
    trusted: TrustedKeySet,
    issued: Mutex<Vec<ConnectionHandle>>,
    controls: Mutex<Vec<(ConnectionHandle, oac_core::connection::WriteControl)>>,
    events: Mutex<Vec<AdapterEvent>>,
    changed: Condvar,
    requests: Mutex<Vec<RecordedRequest>>,
    script: Mutex<VecDeque<Scripted>>,
    next_id: AtomicU64,
}

impl CoreSide {
    /// A core side with a fresh device identity.
    pub fn new() -> Arc<CoreSide> {
        let identity = DeviceIdentity::new(
            DeviceKey::generate(),
            Token::parse("oac-contract-core").expect("a token"),
        );
        let trusted = TrustedKeySet::new(&identity);
        Arc::new(CoreSide {
            identity,
            trusted,
            issued: Mutex::default(),
            controls: Mutex::default(),
            events: Mutex::default(),
            changed: Condvar::new(),
            requests: Mutex::default(),
            script: Mutex::default(),
            next_id: AtomicU64::new(1),
        })
    }

    /// Accept a local connection, as the core does after authenticating its peer
    /// ([IFC-ADP-012]), and record its handle as issued.
    pub fn accept(&self, reader: ConnectionReader, writer: ConnectionWriter) -> Connection {
        let control = writer.control();
        let c = Connection::accept(reader, writer);
        lock(&self.controls).push((c.handle().clone(), control));
        lock(&self.issued).push(c.handle().clone());
        c
    }

    /// True when the core side issued `h`.
    pub fn issued(&self, h: &ConnectionHandle) -> bool {
        lock(&self.issued).contains(h)
    }

    /// Every adapter event so far.
    pub fn events(&self) -> Vec<AdapterEvent> {
        lock(&self.events).clone()
    }

    /// Wait up to [`WAIT`] for `pred` to hold over the events.
    pub fn wait_for(&self, pred: impl Fn(&[AdapterEvent]) -> bool) -> bool {
        let end = Instant::now() + WAIT;
        let mut ev = lock(&self.events);
        loop {
            if pred(&ev) {
                return true;
            }
            let now = Instant::now();
            if now >= end {
                return false;
            }
            ev = self
                .changed
                .wait_timeout(ev, end - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// Every request so far.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        lock(&self.requests).clone()
    }

    /// The result for the next send request (by default `sent`).
    pub fn script(&self, s: Scripted) {
        lock(&self.script).push_back(s);
    }

    /// The event handler for `watch_attachments`.
    pub fn handler(self: &Arc<Self>) -> AdapterEventHandler {
        let me = self.clone();
        Arc::new(move |e| {
            lock(&me.events).push(e);
            me.changed.notify_all();
        })
    }

    /// The request sink for `accept_requests`.
    pub fn sink(self: &Arc<Self>) -> Arc<dyn RequestSink> {
        Arc::new(Sink(self.clone()))
    }

    /// A verified message from `from` to `to` with one text part.
    pub fn message(&self, from: &SessionId, to: &SessionId, text: &str) -> ChannelMessage {
        self.message_with_limits(from, to, text, &EnvelopeLimits::default())
    }

    fn message_with_limits(
        &self,
        from: &SessionId,
        to: &SessionId,
        text: &str,
        limits: &EnvelopeLimits,
    ) -> ChannelMessage {
        let now = SystemClock.now();
        let id = Token::parse(&format!(
            "h{}",
            self.next_id.fetch_add(1, Ordering::Relaxed)
        ))
        .expect("a token");
        let draft = EnvelopeDraft::new(
            id,
            from.clone(),
            to.clone(),
            now.clone(),
            vec![TextPart::new(text).expect("non-empty I-JSON text")],
        )
        .expect("content");
        let env = self.identity.sign_envelope(draft);
        let msg = receive_envelope(env.octets(), limits, &now).expect("a valid envelope");
        authenticate(msg, &self.trusted).expect("signed by the core side's own key")
    }

    /// A hand-off of `text` to `attachment`, addressed to `to`.
    pub fn hand_off(&self, attachment: &Attachment, to: &SessionId, text: &str) -> HandOff {
        let msg = self.message(&peer_session(), to, text);
        HandOff::new(attachment.clone(), msg).expect("a verified message")
    }

    /// A hand-off whose envelope is as close to `octets` as a text of ASCII letters can
    /// make it, without going over; and its text.
    pub fn hand_off_of_size(
        &self,
        attachment: &Attachment,
        to: &SessionId,
        octets: u64,
    ) -> (HandOff, String) {
        let limits = EnvelopeLimits::default()
            .with_max_envelope_octets(octets.max(65536))
            .expect("a size limit in range");
        let probe = self.message_with_limits(&peer_session(), to, "x", &limits);
        let overhead = probe.envelope().octets().len() as u64 - 1;
        // Ids are drawn from a counter, so later envelopes can be a few octets longer.
        let n = octets.saturating_sub(overhead + 8).max(1);
        let text = "a".repeat(usize::try_from(n).unwrap_or(1));
        let msg = self.message_with_limits(&peer_session(), to, &text, &limits);
        (
            HandOff::new(attachment.clone(), msg).expect("a verified message"),
            text,
        )
    }
}

/// The core side's request sink.
struct Sink(Arc<CoreSide>);

impl RequestSink for Sink {
    fn send(&self, request: SendRequest) -> SendRequestResult {
        let me = &self.0;
        let id = Token::parse(&format!("m{}", me.next_id.fetch_add(1, Ordering::Relaxed)))
            .expect("a token");
        let result = match lock(&me.script).pop_front().unwrap_or(Scripted::Send) {
            Scripted::Send => SendRequestResult::Sent {
                id,
                correlation: None,
                receipts: ReceiptStream(std::sync::mpsc::channel().1),
            },
            Scripted::Refuse(error) => SendRequestResult::Refused { error },
            Scripted::NotPass(error) => SendRequestResult::NotPassed { id, error },
        };
        lock(&me.requests).push(RecordedRequest {
            request: Request::Send(request),
            outcome: result.outcome(),
            error: result.error(),
            id: result.id().map(|t| t.as_str().to_owned()),
        });
        result
    }

    fn discover(&self, request: DiscoveryRequest) -> DiscoveryRequestResult {
        lock(&self.0.requests).push(RecordedRequest {
            request: Request::Discovery(request),
            outcome: "discovery-result",
            error: None,
            id: None,
        });
        DiscoveryRequestResult::DiscoveryResult(Vec::new())
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The session the suite's hand-offs come from: a session of another implementation.
pub fn peer_session() -> SessionId {
    SessionId::from_random_octets([0xee; 16])
}

// ---- report -------------------------------------------------------------------------------

/// The verdict of one check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The requirement held, with what was exercised.
    Pass(String),
    /// The check could not apply, with why. Not a pass.
    NotApplicable(String),
    /// The requirement did not hold, with what was seen.
    Fail(String),
}

impl Verdict {
    /// True for [`Verdict::Fail`].
    pub fn is_fail(&self) -> bool {
        matches!(self, Verdict::Fail(_))
    }
}

/// One row of a [`Report`].
#[derive(Clone, Debug)]
pub struct Row {
    /// The requirement id.
    pub id: &'static str,
    /// The check's name.
    pub check: &'static str,
    /// The verdict.
    pub verdict: Verdict,
}

/// What [`run`] found.
#[derive(Clone, Debug)]
pub struct Report {
    /// The harness's name.
    pub harness: String,
    /// The rows, in the order the checks ran; [IFC-ADP-010] last.
    pub rows: Vec<Row>,
}

impl Report {
    /// The failed rows.
    pub fn failures(&self) -> Vec<&Row> {
        self.rows.iter().filter(|r| r.verdict.is_fail()).collect()
    }

    /// True when any row for `id` failed.
    pub fn failed(&self, id: &str) -> bool {
        self.rows.iter().any(|r| r.id == id && r.verdict.is_fail())
    }

    /// The ids with at least one row.
    pub fn ids(&self) -> Vec<&'static str> {
        let mut v: Vec<_> = self.rows.iter().map(|r| r.id).collect();
        v.dedup();
        v
    }

    /// The ids whose every row is not applicable.
    pub fn not_applicable(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = self
            .ids()
            .into_iter()
            .filter(|id| {
                self.rows
                    .iter()
                    .filter(|r| r.id == *id)
                    .all(|r| matches!(r.verdict, Verdict::NotApplicable(_)))
            })
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Panic, printing the report, when any row failed.
    pub fn assert_conformant(&self) {
        assert!(self.failures().is_empty(), "{self}");
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "adapter contract suite: {}", self.harness)?;
        for r in &self.rows {
            let (tag, why) = match &r.verdict {
                Verdict::Pass(w) => ("pass", w),
                Verdict::NotApplicable(w) => ("n/a ", w),
                Verdict::Fail(w) => ("FAIL", w),
            };
            writeln!(f, "  {tag}  {:<13} {:<34} {why}", r.id, r.check)?;
        }
        Ok(())
    }
}

// ---- the run ----------------------------------------------------------------------------

struct Session {
    index: usize,
    attachment: Attachment,
    session: SessionId,
    /// Texts handed off to this session, with the outcome.
    delivered: Vec<(String, HandOffOutcome)>,
}

struct Ctx<'h> {
    h: &'h mut dyn AdapterHarness,
    core: Arc<CoreSide>,
    adapter: Arc<dyn ProviderAdapter>,
    rows: Vec<Row>,
    next_text: u32,
}

impl Ctx<'_> {
    fn row(&mut self, id: &'static str, check: &'static str, verdict: Verdict) {
        self.rows.push(Row { id, check, verdict });
    }

    fn text(&mut self, tag: &str) -> String {
        self.next_text += 1;
        format!("oac contract {tag} #{}#", self.next_text)
    }

    fn open(&mut self, n: u8) -> Result<Session, String> {
        let (index, conns) = self.h.open_session(&self.core).map_err(|g| g.to_string())?;
        let handles: Vec<ConnectionHandle> = conns.iter().map(|c| c.handle().clone()).collect();
        for c in conns {
            self.adapter.take_connection(c);
        }
        self.h.session_ready(index).map_err(|g| g.to_string())?;
        let opened = |ev: &[AdapterEvent]| {
            ev.iter().find_map(|e| match e {
                AdapterEvent::AttachmentOpened { attachment, .. }
                    if handles.contains(attachment) =>
                {
                    Some(attachment.clone())
                }
                _ => None,
            })
        };
        if !self.core.wait_for(|ev| opened(ev).is_some()) {
            return Err(format!(
                "no attachment-opened for the session's connection(s) {handles:?}; events {:?}",
                self.core.events()
            ));
        }
        let attachment = opened(&self.core.events()).expect("seen above");
        let session = SessionId::from_random_octets([n; 16]);
        self.adapter.set_binding(&attachment, Some(session.clone()));
        Ok(Session {
            index,
            attachment,
            session,
            delivered: Vec::new(),
        })
    }

    fn deliver(&mut self, s: &mut Session, tag: &str) -> (String, HandOffOutcome) {
        let text = self.text(tag);
        let out = self
            .adapter
            .deliver(self.core.hand_off(&s.attachment, &s.session, &text));
        s.delivered.push((text.clone(), out));
        (text, out)
    }
}

fn calls_with<'a>(o: &'a Observations, text: &str) -> Vec<&'a HandOffCall> {
    o.hand_off_calls
        .iter()
        .filter(|c| c.text.contains(text))
        .collect()
}

fn took(o: &Observations, text: &str) -> bool {
    o.inputs.iter().any(|i| i.contains(text))
}

/// Every adapter has source, so a harness that gives none has dropped the static checks
/// (#351).
const NO_SOURCE: &str = "the harness gave no source files to scan; every adapter has source, so the static checks cannot be skipped";

/// The verdict for checks whose harness step returned `g`. A broken step fails. An
/// unsupported step fails too when `required_by` names the binding whose profile makes the
/// step mandatory (`because` says why); otherwise the checks are not applicable (#351).
fn gap_verdict(g: &Gap, required_by: Option<&str>, step: &str, because: &str) -> Verdict {
    match (g, required_by) {
        (Gap::Broken(w), _) => Verdict::Fail(format!("the harness could not {step}: {w}")),
        (Gap::Unsupported(_), Some(name)) => Verdict::Fail(format!(
            "the {name} binding makes this check mandatory (its {because}), but the harness says it cannot {step}: {g}"
        )),
        (Gap::Unsupported(_), None) => {
            Verdict::NotApplicable(format!("the fake harness cannot {step}: {g}"))
        }
    }
}

macro_rules! check {
    ($ctx:expr, $id:literal, $name:literal, $body:expr) => {{
        let v: Verdict = $body;
        $ctx.row($id, $name, v);
    }};
}

/// Run the suite against `harness`. A panic inside the run is reported as a failure of
/// [IFC-ADP-010] with the panic message, after the rows already recorded.
pub fn run(harness: &mut dyn AdapterHarness) -> Report {
    let name = harness.name();
    let core = CoreSide::new();
    let adapter = harness.adapter();
    let mut ctx = Ctx {
        h: harness,
        core,
        adapter,
        rows: Vec::new(),
        next_text: 0,
    };
    let result = catch_unwind(AssertUnwindSafe(|| scenarios(&mut ctx)));
    let mut rows = std::mem::take(&mut ctx.rows);
    let aborted = match result {
        Ok(Ok(())) => None,
        Ok(Err(why)) => Some(why),
        Err(p) => Some(format!(
            "panicked: {}",
            p.downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_default()
        )),
    };
    let failed: Vec<_> = rows
        .iter()
        .filter(|r| r.verdict.is_fail())
        .map(|r| r.id)
        .collect();
    let verdict = match (&aborted, failed.is_empty()) {
        (Some(why), _) => Verdict::Fail(format!("the suite could not finish: {why}")),
        (None, true) => Verdict::Pass("no adapter-owned check failed".into()),
        (None, false) => Verdict::Fail(format!("failed: {}", failed.join(", "))),
    };
    rows.push(Row {
        id: "IFC-ADP-010",
        check: "every-adapter-requirement",
        verdict,
    });
    Report {
        harness: name,
        rows,
    }
}

#[allow(clippy::too_many_lines)]
fn scenarios(ctx: &mut Ctx<'_>) -> Result<(), String> {
    let claimed = ctx.h.profile();
    ctx.adapter.watch_attachments(ctx.core.handler());
    ctx.adapter.accept_requests(ctx.core.sink());

    // ---- attachments ----------------------------------------------------------------------
    let mut s1 = match ctx.open(1) {
        Ok(s) => s,
        Err(why) => {
            ctx.row(
                "IFC-ADP-020",
                "reports-attachments-observed",
                Verdict::Fail(why.clone()),
            );
            return Err(why);
        }
    };
    check!(ctx, "IFC-ADP-020", "reports-attachments-observed", {
        let opened = ctx
            .core
            .events()
            .iter()
            .filter(|e| matches!(e, AdapterEvent::AttachmentOpened { attachment, .. } if *attachment == s1.attachment))
            .count();
        if opened == 1 {
            Verdict::Pass("one attachment-opened for the session's connection".into())
        } else {
            Verdict::Fail(format!("{opened} attachment-opened events for one session"))
        }
    });
    let caps = ctx.adapter.capabilities(&s1.attachment);

    // ---- no polling ([IFC-ADP-040]; 09-test-strategy §5) ---------------------------------
    let before = ctx.h.observe(s1.index);
    let mut idle = Vec::new();
    for _ in 0..3 {
        idle.push(ctx.deliver(&mut s1, "idle"));
        ctx.h.drain(s1.index).map_err(|g| g.to_string())?;
    }
    let after = ctx.h.observe(s1.index);
    check!(ctx, "IFC-ADP-040", "contract/adapter/no-polling", {
        if !caps.active_inbound {
            Verdict::NotApplicable(
                "the adapter reports active_inbound false for the attachment".into(),
            )
        } else {
            let mut bad = Vec::new();
            if after.establishment_calls != before.establishment_calls {
                bad.push(format!(
                    "establishment calls went from {} to {} across 3 deliveries",
                    before.establishment_calls, after.establishment_calls
                ));
            }
            let fetched: Vec<_> =
                after.fetch_calls[before.fetch_calls.len().min(after.fetch_calls.len())..].to_vec();
            if !fetched.is_empty() {
                bad.push(format!("message-fetch calls between arrivals: {fetched:?}"));
            }
            if !after.offered_fetch_surfaces.is_empty() {
                bad.push(format!(
                    "inbox-style surfaces offered: {:?}",
                    after.offered_fetch_surfaces
                ));
            }
            for (t, out) in &idle {
                if *out != HandOffOutcome::Completed {
                    bad.push(format!("{t:?} to an idle session: {}", out.as_str()));
                } else if !took(&after, t) {
                    bad.push(format!("{t:?} never became session input"));
                }
            }
            if bad.is_empty() {
                Verdict::Pass(format!(
                    "3 messages pushed into an idle session; {} establishment call(s), flat; no fetch call; no inbox surface",
                    after.establishment_calls
                ))
            } else {
                Verdict::Fail(bad.join("; "))
            }
        }
    });

    // ---- the binding (#351) ---------------------------------------------------------------
    // The binding is identified from the hand-off calls the fake recorded, not from the
    // harness's profile, and every later check runs under the suite's own profile for it.
    // A harness cannot drop a binding's checks by naming another binding, an invented one,
    // or a weakened copy of its own.
    let identified = identify_binding(
        after
            .client_hand_off_calls
            .iter()
            .map(|c| c.operation.as_str()),
    );
    let profile = identified.as_ref().map_or(claimed, |p| *p);
    check!(
        ctx,
        "IFC-ADP-010",
        "harness-profile-is-the-bindings",
        match &identified {
            Err(why) => Verdict::Fail(why.clone()),
            Ok(p) if *p == claimed => Verdict::Pass(format!(
                "the harness's profile is the suite's own {} profile, the binding its hand-off calls identify",
                p.name
            )),
            Ok(p) => Verdict::Fail(format!(
                "the hand-off calls identify the {} binding, and the harness's profile differs from the suite's own for it: {claimed:?}; the suite's is used",
                p.name
            )),
        }
    );

    // ---- capabilities ([IFC-ADP-041]) ------------------------------------------------------
    check!(ctx, "IFC-ADP-041", "reported-sizes-handed-off-unchanged", {
        match caps.max_envelope_octets {
            None => Verdict::NotApplicable(format!(
                "no max_envelope_octets reported; content_types {:?} (only text is exercised)",
                caps.content_types
            )),
            Some(max) => {
                let (h, text) = ctx.core.hand_off_of_size(&s1.attachment, &s1.session, max);
                let out = ctx.adapter.deliver(h);
                s1.delivered.push((text.clone(), out));
                ctx.h.drain(s1.index).map_err(|g| g.to_string())?;
                let o = ctx.h.observe(s1.index);
                if out == HandOffOutcome::Completed && took(&o, &text) {
                    Verdict::Pass(format!(
                        "a message at the reported {max}-octet limit handed off unchanged"
                    ))
                } else {
                    Verdict::Fail(format!(
                        "a message at the reported {max}-octet limit: {} and {}",
                        out.as_str(),
                        if took(&o, &text) {
                            "taken"
                        } else {
                            "not taken unchanged"
                        }
                    ))
                }
            }
        }
    });
    check!(ctx, "IFC-ADP-043", "reports-capability-changes", Verdict::NotApplicable(
        "neither fake harness can change what an attachment can do; exercised by the adapter tasks (G4-G8)".into()
    ));

    // ---- a busy session: the holding hand-off ([SEC-AUZ-025], [SEC-AUZ-026]) -------------
    let busy = match ctx.h.start_turn(s1.index) {
        Ok(()) => {
            let a = ctx.deliver(&mut s1, "busy");
            let b = ctx.deliver(&mut s1, "busy");
            let during = ctx.h.observe(s1.index);
            ctx.h.drain(s1.index).map_err(|g| g.to_string())?;
            let o = ctx.h.observe(s1.index);
            Ok((a, b, during, o))
        }
        Err(g) => Err(g),
    };
    match &busy {
        Ok((a, b, during, o)) => {
            let holding = profile.holding_hand_off;
            // Neither message may become input while the turn `start_turn` started is
            // running: a harness whose `start_turn` returns Ok without starting one would
            // make both rows pass without a busy session (#351, PR #355 review R2).
            // While the turn runs the harness holds both, so it must report at least two
            // held inputs and neither among the inputs taken. The held count is what the
            // planted TurnNoop pins. The inputs check is defence in depth: no planted breach
            // pins it, since only a harness that misreports Observations (the declared
            // residual) could pass the held count and still let a message through.
            let mut not_busy: Vec<String> = [a, b]
                .iter()
                .filter(|(t, _)| took(during, t))
                .map(|(t, _)| {
                    format!(
                        "{t:?} became input while the turn start_turn started should have been running"
                    )
                })
                .collect();
            if during.held < 2 {
                not_busy.push(format!(
                    "the harness held {} input(s) while its turn should have been running, not both messages",
                    during.held
                ));
            }
            check!(ctx, "SEC-AUZ-025", "holding-hand-off-while-running", {
                let mut bad = not_busy.clone();
                for (t, out) in [a, b] {
                    let calls = calls_with(o, t);
                    if calls.iter().any(|c| c.steering) {
                        bad.push(format!("{t:?} was handed off by a steering operation"));
                    }
                    if let Some(op) = holding
                        && calls.iter().any(|c| c.operation != op)
                    {
                        bad.push(format!("{t:?} was not handed off through {op}"));
                    }
                    if *out == HandOffOutcome::Completed && !took(o, t) {
                        bad.push(format!("{t:?} completed but never became input"));
                    }
                }
                if bad.is_empty() {
                    Verdict::Pass(format!(
                        "2 messages to a running turn held by the harness ({}) and taken after it",
                        holding.unwrap_or("its only input surface")
                    ))
                } else {
                    Verdict::Fail(bad.join("; "))
                }
            });
            check!(ctx, "SEC-AUZ-026", "order-kept-through-the-hold", {
                let pos = |t: &str| o.inputs.iter().position(|i| i.contains(t));
                if !not_busy.is_empty() {
                    Verdict::Fail(not_busy.join("; "))
                } else {
                    match (pos(&a.0), pos(&b.0)) {
                        (Some(x), Some(y)) if x < y => {
                            Verdict::Pass("taken in the order handed off".into())
                        }
                        (Some(_), Some(_)) => {
                            Verdict::Fail("taken out of the order handed off".into())
                        }
                        _ => Verdict::Fail(format!("not both taken: inputs {:?}", o.inputs)),
                    }
                }
            });
        }
        Err(g) => {
            let v = gap_verdict(
                g,
                profile.runs_own_turns.then_some(profile.name),
                "start a turn",
                "sessions run turns of their own",
            );
            ctx.row("SEC-AUZ-025", "holding-hand-off-while-running", v.clone());
            ctx.row("SEC-AUZ-026", "order-kept-through-the-hold", v);
        }
    }

    // ---- a turned-away hand-off ([SEC-AUZ-027], [IFC-ADP-052], [IFC-ADP-056]) ------------
    let refused = match ctx.h.refuse_hand_offs(s1.index) {
        Ok(()) => {
            let r = ctx.deliver(&mut s1, "refused");
            ctx.h.allow_hand_offs(s1.index).map_err(|g| g.to_string())?;
            let later = ctx.deliver(&mut s1, "after-refusal");
            ctx.h.drain(s1.index).map_err(|g| g.to_string())?;
            Ok((r, later, ctx.h.observe(s1.index)))
        }
        Err(g) => Err(g),
    };
    match &refused {
        Ok(((t, out), _, o)) => {
            let calls = calls_with(o, t);
            check!(ctx, "SEC-AUZ-027", "turned-away-makes-no-other-call", {
                if calls.len() == 1 && !calls[0].accepted && !calls[0].steering {
                    Verdict::Pass(format!(
                        "one {} call, turned away, and no other call",
                        calls[0].operation
                    ))
                } else {
                    Verdict::Fail(format!("calls for the turned-away message: {calls:?}"))
                }
            });
            check!(ctx, "IFC-ADP-051", "completed-only-on-success", {
                if *out == HandOffOutcome::Completed {
                    Verdict::Fail("completed for a hand-off the harness turned away".into())
                } else {
                    Verdict::Pass(format!("{} for a turned-away hand-off", out.as_str()))
                }
            });
            // The binding names the refusal's code; the adapter's outcome must map to it
            // through Table 5.3. For the Codex profile the refusal is the recorded archived
            // one, which spec/bindings/mcp.md §8.2.1 classifies `handoff-failed`
            // ([SC-DLV-009]), not "cannot take input now" ([SC-DLV-008]): an adapter that
            // reports `not-now` for it passes every other row (#347).
            check!(ctx, "SC-DLV-009", "refusal-reported-as-binding-says", {
                match profile.turned_away_code {
                    None => Verdict::NotApplicable(format!(
                        "the {} binding gives the harness's refusal no code",
                        profile.name
                    )),
                    Some(want) => {
                        let got = out.recorded().1;
                        if got == Some(want) {
                            Verdict::Pass(format!(
                                "{} ({}) for the refusal the binding classifies {}",
                                out.as_str(),
                                want.as_str(),
                                want.as_str()
                            ))
                        } else {
                            Verdict::Fail(format!(
                                "{} ({}) for a refusal the binding classifies {}",
                                out.as_str(),
                                got.map_or("no code", ErrorCode::as_str),
                                want.as_str()
                            ))
                        }
                    }
                }
            });
        }
        Err(g) => {
            let v = gap_verdict(
                g,
                profile.turned_away_code.map(|_| profile.name),
                "turn a hand-off away",
                "harness refuses hand-offs with a refusal the binding classifies",
            );
            ctx.row("SEC-AUZ-027", "turned-away-makes-no-other-call", v.clone());
            ctx.row("SC-DLV-009", "refusal-reported-as-binding-says", v);
        }
    }

    // ---- an unbound attachment ([IFC-ADP-030]) ---------------------------------------------
    ctx.adapter.set_binding(&s1.attachment, None);
    let (unbound, out) = ctx.deliver(&mut s1, "unbound");
    ctx.adapter
        .set_binding(&s1.attachment, Some(s1.session.clone()));
    ctx.h.drain(s1.index).map_err(|g| g.to_string())?;
    let o = ctx.h.observe(s1.index);
    check!(ctx, "IFC-ADP-030", "no-hand-off-when-unbound", {
        let n = calls_with(&o, &unbound).len();
        if n == 0 && !took(&o, &unbound) && out != HandOffOutcome::Completed {
            Verdict::Pass(format!("{} and no hand-off call", out.as_str()))
        } else {
            Verdict::Fail(format!(
                "{} with {n} hand-off call(s) after set_binding(None)",
                out.as_str()
            ))
        }
    });

    // ---- requests ([IFC-ADP-003], [IFC-ADP-031], [IFC-ADP-060]) ---------------------------
    let target = peer_session();
    let cases = [
        (Scripted::Send, false),
        (Scripted::Refuse(ErrorCode::Unauthorized), false),
        (Scripted::NotPass(ErrorCode::TransportFailure), false),
        (Scripted::Send, true),
    ];
    let mut req_results = Vec::new();
    let mut unsupported = None;
    for (scripted, spoof) in cases {
        ctx.core.script(scripted);
        let before = ctx.core.requests().len();
        let text = ctx.text("request");
        let req = HarnessRequest::Send {
            to: target.clone(),
            text: text.clone(),
            claimed_from: spoof.then(|| SessionId::from_random_octets([0x77; 16])),
        };
        match ctx.h.request(s1.index, &req) {
            Ok(observed) => req_results.push((scripted, spoof, text, before, observed)),
            Err(Gap::Unsupported(w)) => {
                unsupported = Some(w);
                lock(&ctx.core.script).clear();
                break;
            }
            Err(Gap::Broken(w)) => return Err(format!("harness request failed: {w}")),
        }
    }
    let discover = if unsupported.is_none() {
        let before = ctx.core.requests().len();
        Some((before, ctx.h.request(s1.index, &HarnessRequest::Discover)))
    } else {
        None
    };
    if let Some(why) = unsupported {
        let v = gap_verdict(
            &Gap::Unsupported(why),
            profile.makes_requests.then_some(profile.name),
            "make requests",
            "harness makes its requests through the surface its fake models",
        );
        for (id, name) in [
            ("IFC-ADP-003", "requests-reach-the-core"),
            ("IFC-ADP-031", "requests-labelled-by-connection"),
            ("IFC-ADP-060", "results-returned-unchanged"),
        ] {
            ctx.row(id, name, v.clone());
        }
    } else {
        let all = ctx.core.requests();
        let mut missing = Vec::new();
        let mut mislabelled = Vec::new();
        let mut changed = Vec::new();
        for (scripted, spoof, text, before, observed) in &req_results {
            let mine: Vec<&RecordedRequest> = all[*before..]
                .iter()
                .filter(|r| matches!(&r.request, Request::Send(s) if s.content.iter().any(|p| matches!(p, oac_core::envelope::ContentPart::Text(t) if t.text().contains(text.as_str())))))
                .collect();
            if mine.len() != 1 {
                missing.push(format!(
                    "{text:?}: {} request(s) reached the sink",
                    mine.len()
                ));
                continue;
            }
            let rec = mine[0];
            if rec.request.attachment() != &s1.attachment {
                mislabelled.push(format!(
                    "{text:?}{} labelled {:?}",
                    if *spoof { " (spoofed sender)" } else { "" },
                    rec.request.attachment()
                ));
            }
            let ok = match scripted {
                Scripted::Send => {
                    !observed.is_error
                        && rec
                            .id
                            .as_deref()
                            .is_some_and(|id| observed.text.contains(id))
                        && observed.text.contains("accepted-by-adapter")
                }
                Scripted::Refuse(code) | Scripted::NotPass(code) => {
                    observed.is_error && observed.text.contains(code.as_str())
                }
            };
            if !ok {
                changed.push(format!("{scripted:?} came back as {observed:?}"));
            }
        }
        if let Some((before, res)) = &discover {
            match res {
                Ok(observed) => {
                    let mine: Vec<_> = all[*before..]
                        .iter()
                        .filter(|r| matches!(r.request, Request::Discovery(_)))
                        .collect();
                    if mine.len() != 1 {
                        missing.push(format!(
                            "discovery: {} request(s) reached the sink",
                            mine.len()
                        ));
                    } else if mine[0].request.attachment() != &s1.attachment {
                        mislabelled.push(format!(
                            "discovery labelled {:?}",
                            mine[0].request.attachment()
                        ));
                    }
                    if observed.is_error {
                        changed.push(format!("discovery came back as an error: {observed:?}"));
                    }
                }
                Err(g) => missing.push(format!("discovery request failed: {g}")),
            }
        }
        check!(
            ctx,
            "IFC-ADP-003",
            "requests-reach-the-core",
            if missing.is_empty() {
                Verdict::Pass(format!(
                    "{} send and 1 discovery request each reached the sink once",
                    req_results.len()
                ))
            } else {
                Verdict::Fail(missing.join("; "))
            }
        );
        check!(
            ctx,
            "IFC-ADP-031",
            "requests-labelled-by-connection",
            if mislabelled.is_empty() {
                Verdict::Pass(
                    "every request labelled with its connection, a spoofed sender claim included"
                        .into(),
                )
            } else {
                Verdict::Fail(mislabelled.join("; "))
            }
        );
        check!(
            ctx,
            "IFC-ADP-060",
            "results-returned-unchanged",
            if changed.is_empty() {
                Verdict::Pass("sent, refused (unauthorized), not-passed (transport-failure) and discovery results reached the harness unchanged".into())
            } else {
                Verdict::Fail(changed.join("; "))
            }
        );
    }

    // ---- a second session, then its end ([IFC-ADP-022]) -----------------------------------
    let second = ctx.open(2);
    match second {
        Ok(mut s2) => {
            let (t2, out2) = ctx.deliver(&mut s2, "second");
            ctx.h.drain(s2.index).map_err(|g| g.to_string())?;
            let o1 = ctx.h.observe(s1.index);
            let o2 = ctx.h.observe(s2.index);
            check!(ctx, "IFC-ADP-004", "hands-off-only-what-the-core-passed", {
                let mut bad = Vec::new();
                for (s, o) in [(&s1, &o1), (&s2, &o2)] {
                    for i in &o.inputs {
                        if !s.delivered.iter().any(|(t, _)| i.contains(t.as_str())) {
                            bad.push(format!(
                                "session {} took input the core never passed it: {i:?}",
                                s.index
                            ));
                        }
                    }
                }
                if took(&o1, &t2) {
                    bad.push("a hand-off to the second session reached the first".into());
                }
                if out2 == HandOffOutcome::Completed && !took(&o2, &t2) {
                    bad.push("a hand-off to the second session never reached it".into());
                }
                if bad.is_empty() {
                    Verdict::Pass(format!(
                        "{} inputs, each one the core passed to that session",
                        o1.inputs.len() + o2.inputs.len()
                    ))
                } else {
                    Verdict::Fail(bad.join("; "))
                }
            });
            ctx.h.end_session(s2.index).map_err(|g| g.to_string())?;
            let att2 = s2.attachment.clone();
            let closed = ctx.core.wait_for(|ev| ev.iter().any(|e| matches!(e, AdapterEvent::AttachmentClosed { attachment } if *attachment == att2)));
            check!(
                ctx,
                "IFC-ADP-022",
                "reports-attachment-closed",
                if closed {
                    Verdict::Pass("attachment-closed when the harness ended the session".into())
                } else {
                    Verdict::Fail("no attachment-closed after the harness ended the session".into())
                }
            );
            for (t, o) in s2.delivered.drain(..) {
                s1.delivered.push((format!("[s2] {t}"), o));
            }
        }
        Err(why) => return Err(format!("second session: {why}")),
    }

    // ---- every hand-off so far ([IFC-ADP-050] to [IFC-ADP-057], never steer) --------------
    let o = ctx.h.observe(s1.index);
    let delivered: Vec<(String, HandOffOutcome)> = s1
        .delivered
        .iter()
        .filter(|(t, _)| !t.starts_with("[s2] "))
        .cloned()
        .collect();
    check!(
        ctx,
        "IFC-ADP-050",
        "one-outcome-per-hand-off",
        Verdict::Pass(format!(
            "{} hand-offs, {} outcomes (deliver returns exactly one by its signature)",
            delivered.len(),
            delivered.len()
        ))
    );
    check!(ctx, "IFC-ADP-057", "at-most-one-call-per-hand-off", {
        let over: Vec<_> = delivered
            .iter()
            .filter_map(|(t, _)| {
                let n = calls_with(&o, t).len();
                (n > 1).then(|| format!("{t:?}: {n} calls"))
            })
            .collect();
        if over.is_empty() {
            Verdict::Pass(format!(
                "{} hand-offs, none with more than one call",
                delivered.len()
            ))
        } else {
            Verdict::Fail(over.join("; "))
        }
    });
    check!(ctx, "IFC-ADP-051", "completed-only-on-success", {
        let bad: Vec<_> = delivered
            .iter()
            .filter(|(t, out)| {
                *out == HandOffOutcome::Completed && !calls_with(&o, t).iter().any(|c| c.accepted)
            })
            .map(|(t, _)| t.clone())
            .collect();
        if bad.is_empty() {
            Verdict::Pass("every completed hand-off has a call the harness took".into())
        } else {
            Verdict::Fail(format!("completed with no call the harness took: {bad:?}"))
        }
    });
    check!(ctx, "IFC-ADP-052", "not-now-only-when-turned-away", {
        let nn: Vec<_> = delivered
            .iter()
            .filter(|(_, out)| *out == HandOffOutcome::NotNow)
            .collect();
        let bad: Vec<_> = nn
            .iter()
            .filter(|(t, _)| took(&o, t) || calls_with(&o, t).iter().any(|c| c.accepted))
            .map(|(t, _)| t.clone())
            .collect();
        if !bad.is_empty() {
            Verdict::Fail(format!(
                "not-now although the harness took the input: {bad:?}"
            ))
        } else if nn.is_empty() {
            Verdict::Pass("no not-now outcome reported".into())
        } else {
            Verdict::Pass(format!(
                "{} not-now outcome(s), each for input the harness did not take",
                nn.len()
            ))
        }
    });
    check!(ctx, "IFC-ADP-053", "indeterminate-when-no-answer", Verdict::NotApplicable(
        "neither fake harness leaves an input call without success or failure; exercised by the adapter tasks (G4, G7)".into()
    ));
    check!(ctx, "IFC-ADP-054", "refused-without-a-call", {
        let r: Vec<_> = delivered
            .iter()
            .filter(|(_, out)| *out == HandOffOutcome::Refused)
            .collect();
        let bad: Vec<_> = r
            .iter()
            .filter(|(t, _)| !calls_with(&o, t).is_empty())
            .map(|(t, _)| t.clone())
            .collect();
        if !bad.is_empty() {
            Verdict::Fail(format!("refused but an input call was made: {bad:?}"))
        } else if r.is_empty() {
            Verdict::NotApplicable("no provenance-hostile hand-off is exercised (the suite's messages carry well-formed provenance); F11, G4, G7".into())
        } else {
            Verdict::Pass(format!("{} refused, none with an input call", r.len()))
        }
    });
    check!(ctx, "IFC-ADP-056", "no-hand-off-held-for-later", {
        let bad: Vec<_> = delivered
            .iter()
            .filter(|(t, out)| *out != HandOffOutcome::Completed && took(&o, t))
            .map(|(t, _)| t.clone())
            .collect();
        let n = delivered
            .iter()
            .filter(|(_, out)| *out != HandOffOutcome::Completed)
            .count();
        if !bad.is_empty() {
            Verdict::Fail(format!("handed off later although not completed: {bad:?}"))
        } else {
            Verdict::Pass(format!("{n} hand-off(s) not completed; none taken later"))
        }
    });
    check!(ctx, "SEC-AUZ-022", "never-steers", {
        let steering: Vec<_> = o
            .client_hand_off_calls
            .iter()
            .filter(|c| c.steering)
            .map(|c| c.operation.clone())
            .collect();
        if steering.is_empty() {
            Verdict::Pass(format!(
                "{} hand-off call(s) by the adapter, on any session, none steering",
                o.client_hand_off_calls.len()
            ))
        } else {
            Verdict::Fail(format!("steering hand-off calls: {steering:?}"))
        }
    });
    for (id, rule) in profile.rules {
        let v = match rule {
            Rule::OnlyHolding => {
                let op = profile.holding_hand_off.unwrap_or("(none)");
                let other: Vec<_> = o
                    .client_hand_off_calls
                    .iter()
                    .filter(|c| c.operation != op)
                    .map(|c| c.operation.clone())
                    .collect();
                if other.is_empty() {
                    Verdict::Pass(format!("every hand-off call is {op}"))
                } else {
                    Verdict::Fail(format!("hand-off calls other than {op}: {other:?}"))
                }
            }
            Rule::Never(op) => {
                let n = o
                    .client_hand_off_calls
                    .iter()
                    .filter(|c| c.operation == *op)
                    .count();
                if n == 0 {
                    Verdict::Pass(format!("no {op} call"))
                } else {
                    Verdict::Fail(format!("{n} {op} call(s)"))
                }
            }
            Rule::NoOverrides => {
                let m: Vec<_> = o
                    .client_hand_off_calls
                    .iter()
                    .flat_map(|c| c.override_members.clone())
                    .collect();
                if m.is_empty() {
                    Verdict::Pass("no hand-off call carries a setting override".into())
                } else {
                    Verdict::Fail(format!("override members: {m:?}"))
                }
            }
        };
        ctx.row(id, "binding-hand-off-rule", v);
    }
    let files = ctx.h.source_files();
    let findings = source::scan(&files);
    let static_013: Vec<String> = findings
        .iter()
        .filter(|f| f.requirement == "IFC-ADP-013")
        .map(ToString::to_string)
        .collect();
    check!(ctx, "IFC-ADP-013", "only-core-made-connections", {
        let foreign: Vec<_> = ctx
            .core
            .events()
            .iter()
            .flat_map(|e| match e {
                AdapterEvent::AttachmentOpened { attachment, .. }
                | AdapterEvent::CapabilitiesChanged { attachment }
                | AdapterEvent::AttachmentClosed { attachment } => vec![attachment.clone()],
                AdapterEvent::NativeSignal(s) => s.connection.clone().into_iter().collect(),
            })
            .chain(
                ctx.core
                    .requests()
                    .iter()
                    .map(|r| r.request.attachment().clone()),
            )
            .filter(|h| !ctx.core.issued(h))
            .collect();
        if !foreign.is_empty() {
            Verdict::Fail(format!("handles the core never issued: {foreign:?}"))
        } else if files.is_empty() {
            Verdict::Fail(NO_SOURCE.into())
        } else if !static_013.is_empty() {
            Verdict::Fail(format!(
                "the adapter's source makes connections: {}",
                static_013.join("; ")
            ))
        } else {
            Verdict::Pass(
                "every handle in events and requests is one the core issued; no Connection::accept in the source"
                    .into(),
            )
        }
    });
    if identified.is_ok() {
        let off: Vec<_> = o
            .client_hand_off_calls
            .iter()
            .map(|c| c.operation.as_str())
            .filter(|op| !profile.surface_operations.contains(op))
            .collect();
        if !off.is_empty() {
            ctx.row(
                "IFC-ADP-010",
                "hand-offs-on-one-binding",
                Verdict::Fail(format!(
                    "hand-off calls outside the {} binding's input surface: {off:?}",
                    profile.name
                )),
            );
        }
    }
    if let Some(halted) = &o.halted {
        ctx.row(
            "IFC-ADP-010",
            "fake-harness-not-halted",
            Verdict::Fail(format!("the fake harness halted: {halted}")),
        );
    }

    // ---- health ([IFC-TYP-092]) -------------------------------------------------------------
    check!(ctx, "IFC-TYP-092", "health-holds-no-secret-or-native-id", {
        let h = ctx.adapter.health();
        // Sessions were opened, so the fake knows native ids for them; with none the check
        // would pass any detail (#351, PR #355 review R1).
        if ctx.h.native_ids().iter().all(String::is_empty) {
            Verdict::Fail(
                "the harness gave no native ids although sessions were opened, so the detail cannot be checked for them".into(),
            )
        } else {
            match &h.detail {
                None => Verdict::Pass(format!(
                    "{} with no detail (checked for directories, addresses and {} native id(s))",
                    h.state.as_str(),
                    ctx.h.native_ids().len()
                )),
                Some(d) => {
                    let mut forbidden: Vec<String> = Vec::new();
                    if let Ok(c) = std::env::current_dir() {
                        forbidden.push(c.display().to_string());
                    }
                    for v in ["HOME", "USERPROFILE"] {
                        if let Ok(x) = std::env::var(v)
                            && !x.is_empty()
                        {
                            forbidden.push(x);
                        }
                    }
                    forbidden.extend(ctx.h.native_ids().into_iter().filter(|x| !x.is_empty()));
                    match forbidden.iter().find(|f| d.contains(f.as_str())) {
                        Some(f) => Verdict::Fail(format!("health detail {d:?} contains {f:?}")),
                        None if d.contains("://") => {
                            Verdict::Fail(format!("health detail {d:?} holds an address"))
                        }
                        None => Verdict::Pass(format!("{}: {d:?}", h.state.as_str())),
                    }
                }
            }
        }
    });

    // ---- static: routes through the core ([IFC-ADP-001], [IFC-ADP-002], [IFC-ADP-007]) ---
    for (id, name) in [
        ("IFC-ADP-001", "no-transport-operation"),
        ("IFC-ADP-002", "no-envelope-signing-or-verifying"),
        ("IFC-ADP-007", "no-binding-session-id-or-authorization"),
    ] {
        let mine: Vec<_> = findings
            .iter()
            .filter(|f| f.requirement == id)
            .map(|f| f.to_string())
            .collect();
        let v = if files.is_empty() {
            Verdict::Fail(NO_SOURCE.into())
        } else if mine.is_empty() {
            Verdict::Pass(format!(
                "{} source file(s) parsed, paths resolved through their imports; and the adapter's operations take no transport",
                files.len()
            ))
        } else {
            Verdict::Fail(mine.join("; "))
        };
        ctx.row(id, name, v);
    }
    check!(ctx, "IFC-NEU-002", "no-harness-native-names-to-core", Verdict::Pass(
        "by construction: events carry handles and the NativeSignal/cross_check strings only; requests carry the members of SendRequest and DiscoveryRequest".into()
    ));
    check!(
        ctx,
        "IFC-TYP-090",
        "send-request-names-no-requester",
        Verdict::Pass(
            "by construction: SendRequest has no such member (core/src/adapter.rs test)".into()
        )
    );
    check!(ctx, "IFC-TYP-091", "hand-off-carries-verified-by", Verdict::Pass(
        "by construction: HandOff::new refuses a message without verified_by; every hand-off here was verified".into()
    ));

    let open: Vec<Attachment> = {
        let ev = ctx.core.events();
        ev.iter().filter_map(|e| match e { AdapterEvent::AttachmentOpened { attachment, .. } => Some(attachment.clone()), _ => None })
            .filter(|a| !ev.iter().any(|e| matches!(e, AdapterEvent::AttachmentClosed { attachment } if attachment == a)))
            .collect()
    };

    // Revision 0.4 factory probe: old checks below remain unchanged. A cancellable
    // backend must not conceal an adapter that waits before closing, or leaves a
    // timed-out write able to hand off later. Only Claude writes on these connections;
    // the Codex fake's input uses its separate app-server link.
    if profile.name == claude::PROFILE.name {
        let controls = lock(&ctx.core.controls).clone();
        for (_, control) in &controls {
            control.pause(true);
        }
        let text = ctx.text("stalled-at-shutdown");
        let message = ctx.core.message(
            &SessionId::from_random_octets([222; 16]),
            &s1.session,
            &text,
        );
        let hand_off = HandOff::new(s1.attachment.clone(), message).expect("verified");
        let adapter = ctx.adapter.clone();
        let (settled, settle_rx) = std::sync::mpsc::channel();
        let delivering = std::thread::spawn(move || {
            let _ = settled.send(adapter.deliver(hand_off));
        });
        let end = Instant::now() + WAIT;
        while !controls.iter().any(|(_, c)| c.pending_transfers() != 0) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(1));
        }
        let stalled = controls.iter().any(|(_, c)| c.pending_transfers() != 0);
        let adapter = ctx.adapter.clone();
        let bound = adapter.shutdown_bound_ms();
        let (done, done_rx) = std::sync::mpsc::channel();
        let stopping = std::thread::spawn(move || {
            adapter.shutdown();
            let _ = done.send(());
        });
        let returned = bound != 0
            && done_rx
                .recv_timeout(Duration::from_millis(bound.min(10_000)))
                .is_ok();
        let closed = controls.iter().all(|(h, _)| h.is_closed());
        let terminal_at_return = settle_rx.try_recv().ok();
        // Resume only after observing the barrier, so a detached late writer is exposed.
        for (_, c) in &controls {
            c.pause(false);
        }
        let terminal = terminal_at_return.or_else(|| settle_rx.recv_timeout(WAIT).ok());
        if !returned {
            let _ = done_rx.recv_timeout(WAIT);
        }
        let _ = stopping.join();
        let _ = delivering.join();
        std::thread::sleep(Duration::from_millis(50));
        let observed = ctx.h.observe(s1.index);
        check!(ctx, "IFC-ADP-074", "stalled-shutdown-is-bounded", {
            if stalled && returned {
                Verdict::Pass(format!("stalled peer: shutdown returned within {bound} ms"))
            } else {
                Verdict::Fail(format!(
                    "stalled={stalled}, returned-within-bound={returned}, bound={bound}"
                ))
            }
        });
        check!(ctx, "IFC-ADP-073", "closes-all-core-connections", {
            if closed {
                Verdict::Pass("every supplied connection closed before return".into())
            } else {
                Verdict::Fail("a supplied connection remained open at return".into())
            }
        });
        check!(ctx, "IFC-ADP-070", "stalled-write-has-no-late-hand-off", {
            if calls_with(&observed, &text).is_empty()
                && !took(&observed, &text)
                && terminal.is_some_and(|x| x != HandOffOutcome::Completed)
            {
                Verdict::Pass("resuming the peer after return transferred no pending input".into())
            } else {
                Verdict::Fail(format!(
                    "late hand-off or successful stalled input: {terminal:?}"
                ))
            }
        });
    }

    // ---- shutdown ([IFC-ADP-071], [IFC-ADP-070]) -------------------------------------------
    ctx.adapter.shutdown();
    let ev = ctx.core.events();
    check!(ctx, "IFC-ADP-071", "closes-every-attachment-on-shutdown", {
        let left: Vec<_> = open.iter().filter(|a| !ev.iter().any(|e| matches!(e, AdapterEvent::AttachmentClosed { attachment } if attachment == *a))).collect();
        if left.is_empty() {
            Verdict::Pass(format!(
                "{} open attachment(s) closed before shutdown returned",
                open.len()
            ))
        } else {
            Verdict::Fail(format!("still open when shutdown returned: {left:?}"))
        }
    });
    let requests_before = ctx.core.requests().len();
    let (late, out) = ctx.deliver(&mut s1, "after-shutdown");
    let late_req = ctx.h.request(s1.index, &HarnessRequest::Discover);
    std::thread::sleep(Duration::from_millis(200));
    let o = ctx.h.observe(s1.index);
    check!(ctx, "IFC-ADP-070", "nothing-after-shutdown", {
        let mut bad = Vec::new();
        if !calls_with(&o, &late).is_empty() || took(&o, &late) {
            bad.push("handed off after shutdown".to_owned());
        }
        if out == HandOffOutcome::Completed {
            bad.push("deliver after shutdown returned completed".into());
        }
        let passed = ctx.core.requests().len() - requests_before;
        if passed > 0 {
            bad.push(format!(
                "{passed} request(s) passed to the core after shutdown"
            ));
        }
        if bad.is_empty() {
            Verdict::Pass(format!(
                "deliver after shutdown: {}, no call; harness request after shutdown: {}",
                out.as_str(),
                match &late_req {
                    Ok(_) => "answered without reaching the core",
                    Err(_) => "not answered",
                }
            ))
        } else {
            Verdict::Fail(bad.join("; "))
        }
    });
    Ok(())
}
