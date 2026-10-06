// SPDX-License-Identifier: Apache-2.0

//! The fake Claude Code channel endpoint (#57, F8): a test double of the **harness** side
//! of the Claude channel path, replayed from the recorded Stage 1 fixtures.
//!
//! On the real path Claude Code starts an OAC channel server over stdio, opens MCP, and
//! then receives `notifications/claude/channel` from it and calls its tools. [`FakeClaude`]
//! plays Claude Code in that exchange, so the contract suite (F10, #59) and the security
//! suite (F11, #60) can drive a real or fake OAC channel server with no Claude Code
//! install, no API key, no account, and no network at all: the fake is sans-I/O (frames in
//! through [`FakeClaude::receive`], frames out through [`FakeClaude::take_outbound`]), and
//! [`link`] moves those frames over any byte stream, such as a child process's stdio.
//!
//! # Behaviour, and the fixture behind each part
//!
//! The fixtures are compiled in from `docs/planning/gates/fixtures/` ([`evidence`]).
//! Claude Code version on every fixture used: `2.1.283`.
//!
//! | Behaviour | Fixture |
//! |---|---|
//! | Opening: a `server/discover` probe at MCP `2026-07-28`, then, on `-32601`, legacy `initialize` at `2025-11-25`, `notifications/initialized`, `tools/list` | D6 lines 3-10; G1 Box C lines 13-20; G5 wire lines 11-18 |
//! | A successful probe keeps the client modern: `tools/list` with per-request `_meta`, no `initialize`, and the server's channel notifications are ignored | G4 lines 5, 7, 12-13, 22 (`docs/planning/gates/G4-result.md` criterion 5) |
//! | Newline-delimited JSON framing on stdio | G1 Box C line 12 |
//! | No acknowledgement: a channel notification gets no response and no frame of any kind | D6 lines 12-13 (the next client frame is the model's reply, a minute later); first-party: "Claude Code doesn't acknowledge notifications" (`channels-reference.md`, `docs/planning/REVERIFICATION-B2.md` row 6) |
//! | An idle session wakes: the notification starts a turn | D6 line 12; G1 Box C line 21 (G1-result criterion 2) |
//! | Mid-turn notifications queue and are released at tool-call boundaries, in order, never dropped, never inside a tool call | D6 lines 16, 18; G1 Box C lines 22-23 (G1-result criterion 3) |
//! | Rendering: the `<channel>` tag, `source`, kept and dropped `meta` keys, escapes | G5 wire lines 21-36 against G5 rendered lines 1-11 (idle); G1 Box C line 21 ([`render`]) |
//! | Mid-turn wrapper around the tag | G5 wire line 39 against G5 rendered line 13 |
//! | Tool replies: `tools/call` with `_meta` `claudecode/toolUseId` and `progressToken` equal to the request id | D6 lines 13-14; G1 Box C lines 24-25 (both id 2; see below for later ids) |
//!
//! # What the fake does not invent
//!
//! Where no fixture (or first-party document, cited where used) shows what Claude Code
//! does, the fake does not guess:
//!
//! - **It halts** ([`FakeClaude::halted`]) on anything from the server whose handling is
//!   not recorded: a server-initiated request (`roots/list`, `ping`, sampling, ...); any
//!   notification other than `notifications/claude/channel`; a channel notification before
//!   `tools/list` has completed, one without a string `content` and an object `meta` of
//!   string values, or one with an empty `meta` key; a probe answered with an error other
//!   than `-32601`, or with a result whose `supportedVersions` lacks `2026-07-28`; an
//!   `initialize` result at any revision but `2025-11-25`, or one that does not declare
//!   `capabilities.experimental["claude/channel"]` (what Claude Code does with a channel
//!   notification from such a server is not recorded); a frame without `"jsonrpc":"2.0"`;
//!   a frame that is not I-JSON, including one that repeats a member name (a duplicate
//!   `meta` key); a response with an unknown id. Once halted it processes nothing more,
//!   and records why.
//! - **A server `ping` halts it too.** Keepalives are allowed on the no-polling rule
//!   (`docs/planning/v0.1/09-test-strategy.md` §5), but no fixture records Claude Code
//!   answering one, so an adapter that pings the harness cannot run against this fake
//!   until a capture shows the answer.
//! - **It refuses**, as a [`FakeError`], a driver step it cannot replay: a tool call on the
//!   modern era (no stdio modern `tools/call` is recorded), and ending a turn while
//!   notifications are still queued (the docs say they are delivered "on the next turn";
//!   how that turn renders is not recorded).
//! - **It leaves the text unfixed** ([`render::RenderGap`]) where the attribute set is
//!   evidenced but the exact text is not (content or values with characters no recorded
//!   render contains).
//! - **Mid-turn release granularity is a choice, not a fact** ([`MidTurnRelease`]): G1 Box C
//!   (2.1.283) released two queued notifications at two boundaries, the original G1 run
//!   (2.1.282) released two at one. Which happens is UNVERIFIED as a guarantee
//!   (`docs/planning/v0.1/11-risks.md` row 49). **The F10 and F11 suites must run every
//!   mid-turn test under both settings** (loop over [`MidTurnRelease::BOTH`]);
//!   [`Config::new`]'s default exercises only one. Row 49 also says the outcome may depend
//!   on send timing, so a mix (some notifications released together, some apart) is
//!   possible; neither setting models that, and a test must not depend on the grouping at
//!   all, only on order.
//! - The `claudecode/toolUseId` value is the model's; the fake writes a synthetic one in
//!   the recorded `toolu_` form unless the driver supplies one.
//! - **Inferred, not recorded:** both recorded calls are the session's first, with id 2 and
//!   `progressToken` 2 (D6 line 13, G1 Box C line 24). That calls continue at 3, 4, ... and
//!   that `progressToken` keeps equal to the id is the fake's inference from those two.
//! - Not modelled at all: `--resume`, multiple channels per server, permission relay, the
//!   development-channels consent dialog, the original `2.1.282` opening (no probe), and
//!   the HTTP transport.
//!
//! # Driving it
//!
//! The driver plays both the wire (feeding the server's frames in, writing the fake's
//! frames out) and the session (the terminal user's turns, the model's tool calls and
//! tool-call boundaries). For the security suite, each [`ChannelTag`] exposes the
//! attributes the model would see ([`ChannelTag::attribute`]), the keys the harness dropped
//! ([`ChannelTag::dropped_keys`], a signal the real harness never gives), and the text.
//!
//! ```
//! use oac_fake_claude::{Config, FakeClaude, Phase, SessionEvent};
//!
//! let mut fake = FakeClaude::new(Config::new("oac").unwrap());
//! assert_eq!(fake.take_outbound().len(), 1); // the server/discover probe
//! fake.receive(r#"{"jsonrpc":"2.0","id":"server-discover-probe-1","error":{"code":-32601,"message":"no"}}"#);
//! fake.take_outbound(); // initialize
//! fake.receive(r#"{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":"2025-11-25","capabilities":{"experimental":{"claude/channel":{}},"tools":{}},"serverInfo":{"name":"s","version":"0"}}}"#);
//! fake.take_outbound(); // notifications/initialized, tools/list
//! fake.receive(r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"reply","inputSchema":{"type":"object"}}]}}"#);
//! assert_eq!(fake.phase(), Phase::Ready);
//!
//! fake.receive(r#"{"jsonrpc":"2.0","method":"notifications/claude/channel","params":{"content":"hello","meta":{"oac_sender":"abc","oac-device":"dropped"}}}"#);
//! assert!(fake.take_outbound().is_empty()); // no acknowledgement
//! let SessionEvent::Wake(tag) = &fake.take_events()[0] else { panic!() };
//! assert_eq!(tag.attribute("oac_sender"), ["abc"]);
//! assert_eq!(tag.dropped_keys, ["oac-device"]);
//! assert_eq!(tag.rendered.as_deref(), Ok("<channel source=\"oac\" oac_sender=\"abc\">\nhello\n</channel>"));
//! ```

pub mod evidence;
pub mod link;
pub mod render;

use std::collections::{HashMap, VecDeque};
use std::fmt;

use oac_core::json::{self, Json};

use evidence::{Evidence, member, member_str};
pub use render::{ChannelTag, KeyFate, RenderGap};

/// The legacy revision Claude Code negotiates on the channel path (D6 line 5).
pub const LEGACY_REVISION: &str = "2025-11-25";
/// The modern revision Claude Code probes with (D6 line 3).
pub const MODERN_REVISION: &str = "2026-07-28";
/// The method of a channel notification.
pub const CHANNEL_NOTIFICATION: &str = "notifications/claude/channel";

/// How many queued notifications one tool-call boundary releases. Both are recorded; which
/// one real Claude Code does is UNVERIFIED as a guarantee (module docs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MidTurnRelease {
    /// One per boundary, as G1 Box C observed at `2.1.283`.
    #[default]
    OnePerBoundary,
    /// Every queued notification at the next boundary, as the original G1 run observed at
    /// `2.1.282`.
    AllAtBoundary,
}

impl MidTurnRelease {
    /// Both settings, for a suite to loop over: a mid-turn test must pass under each.
    pub const BOTH: [MidTurnRelease; 2] = [
        MidTurnRelease::OnePerBoundary,
        MidTurnRelease::AllAtBoundary,
    ];
}

/// How the fake is set up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    source: String,
    release: MidTurnRelease,
}

impl Config {
    /// The fake as Claude Code started with the channel server loaded under `source`
    /// (`server:<source>`): the name the `<channel>` tag's `source` attribute carries.
    /// Only lowercase ASCII letters and digits are accepted, the shape of every recorded
    /// load name (`d6claude`, `g1spike`, `g5spike`).
    pub fn new(source: &str) -> Result<Config, FakeError> {
        if source.is_empty()
            || !source
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        {
            return Err(FakeError::InvalidSource(source.to_owned()));
        }
        Ok(Config {
            source: source.to_owned(),
            release: MidTurnRelease::default(),
        })
    }

    /// The same set-up with mid-turn release `release`.
    pub fn with_release(mut self, release: MidTurnRelease) -> Config {
        self.release = release;
        self
    }
}

/// Which MCP era the opening settled on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Era {
    /// `initialize` at `2025-11-25`: the only era on which a channel is registered.
    Legacy,
    /// Per-request `_meta` at `2026-07-28`: never a channel.
    Modern,
}

/// Where the opening is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The `server/discover` probe is out.
    Probing,
    /// `initialize` is out.
    Initializing,
    /// `tools/list` is out.
    Listing,
    /// Open: tools listed; channel notifications are taken (legacy, registered) or
    /// ignored.
    Ready,
    /// Stopped on something no fixture shows ([`FakeClaude::halted`]).
    Halted,
}

/// What the session showed the model, or chose not to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    /// A notification woke the idle session: it is the input of a new turn.
    Wake(ChannelTag),
    /// A queued notification released at a tool-call boundary of the running turn.
    /// `rendered` is the tag inside the recorded mid-turn wrapper.
    MidTurn {
        /// The tag.
        tag: ChannelTag,
        /// The text the model sees, or why it is not fixed.
        rendered: Result<String, RenderGap>,
    },
    /// A notification the harness does not take, because the server is not a channel.
    Ignored {
        /// Why.
        reason: IgnoredReason,
        /// The notification's `content`, if it had one, for test assertions.
        content: Option<String>,
    },
}

/// Why a channel notification was not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IgnoredReason {
    /// The opening settled on the modern era (G4 criterion 5).
    ModernEra,
}

/// Why the fake stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsupported {
    /// What arrived that no fixture shows the handling of.
    pub what: String,
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not evidenced by any fixture: {}", self.what)
    }
}

/// A driver step the fake refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FakeError {
    /// [`Config::new`]: a load name outside the recorded shape.
    InvalidSource(String),
    /// The fake has halted.
    Halted(Unsupported),
    /// The opening has not completed.
    NotReady,
    /// The step needs a running turn, and the session is idle.
    NotInTurn,
    /// A turn is already running.
    AlreadyInTurn,
    /// No tool of that name was listed.
    UnknownTool(String),
    /// Tool arguments that are not a JSON object.
    BadArguments,
    /// Not evidenced: a step the fixtures do not show.
    Unsupported(Unsupported),
}

impl fmt::Display for FakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FakeError::InvalidSource(s) => {
                write!(f, "load name {s:?} is outside the recorded shape")
            }
            FakeError::Halted(u) => write!(f, "halted: {u}"),
            FakeError::NotReady => f.write_str("the MCP opening has not completed"),
            FakeError::NotInTurn => f.write_str("no turn is running"),
            FakeError::AlreadyInTurn => f.write_str("a turn is already running"),
            FakeError::UnknownTool(n) => write!(f, "no tool {n:?} was listed"),
            FakeError::BadArguments => f.write_str("tool arguments must be a JSON object"),
            FakeError::Unsupported(u) => write!(f, "{u}"),
        }
    }
}

impl std::error::Error for FakeError {}

/// The outcome of one `tools/call`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallOutcome {
    /// No response yet.
    Pending,
    /// The `result` member of the response.
    Result(Json),
    /// The `error` member of the response.
    Error(Json),
}

/// Which way a frame went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// From the fake (the harness) to the server.
    ToServer,
    /// From the server to the fake.
    FromServer,
}

/// One frame, as sent or received.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Which way it went.
    pub direction: Direction,
    /// The JSON text, without the line terminator.
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    Discover,
    Initialize,
    ToolsList,
    ToolsCall,
}

/// The fake harness. See the crate docs.
#[derive(Debug)]
pub struct FakeClaude {
    config: Config,
    phase: Phase,
    era: Option<Era>,
    registered: bool,
    tools: Vec<String>,
    in_turn: bool,
    queue: VecDeque<ChannelTag>,
    outbound: VecDeque<String>,
    transcript: Vec<Frame>,
    events: Vec<SessionEvent>,
    pending: HashMap<String, Pending>,
    calls: HashMap<u64, CallOutcome>,
    next_call_id: u64,
    halted: Option<Unsupported>,
}

impl FakeClaude {
    /// A session starting with the channel server loaded: the probe is queued at once,
    /// as Claude Code sends it first (D6 line 3).
    pub fn new(config: Config) -> FakeClaude {
        let mut fake = FakeClaude {
            config,
            phase: Phase::Probing,
            era: None,
            registered: false,
            tools: Vec::new(),
            in_turn: false,
            queue: VecDeque::new(),
            outbound: VecDeque::new(),
            transcript: Vec::new(),
            events: Vec::new(),
            pending: HashMap::new(),
            calls: HashMap::new(),
            // D6: initialize is id 0 and tools/list id 1; the first tools/call is id 2.
            next_call_id: 2,
            halted: None,
        };
        let probe = Evidence::get().discover_probe.clone();
        fake.send_request(&probe, Pending::Discover);
        fake
    }

    /// The Claude Code version the fake replays (`clientInfo.version`, D6 line 5).
    pub fn client_version() -> &'static str {
        member(&Evidence::get().initialize, "params")
            .and_then(|p| member(p, "clientInfo"))
            .and_then(|c| member_str(c, "version"))
            .expect("D6 line 5 carries clientInfo.version")
    }

    /// The load name the `source` attribute carries.
    pub fn source(&self) -> &str {
        &self.config.source
    }

    /// Where the opening is.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The era the opening settled on, once known.
    pub fn era(&self) -> Option<Era> {
        self.era
    }

    /// True when the server is registered as a channel: the legacy opening completed with
    /// `capabilities.experimental["claude/channel"]` declared (without it the fake halts).
    pub fn channel_registered(&self) -> bool {
        self.registered
    }

    /// Tool names from `tools/list`, in listed order.
    pub fn tools(&self) -> &[String] {
        &self.tools
    }

    /// True while a turn is running.
    pub fn in_turn(&self) -> bool {
        self.in_turn
    }

    /// Notifications queued for the running turn.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Why the fake halted, if it did.
    pub fn halted(&self) -> Option<&Unsupported> {
        self.halted.as_ref()
    }

    /// Every frame sent and received, in order.
    pub fn transcript(&self) -> &[Frame] {
        &self.transcript
    }

    /// The methods of every request and notification the fake sent, in order. A harness
    /// never asks a channel server for pending messages: on the channel path, inbound
    /// arrives only as notifications the server pushes (the no-polling shape of
    /// `oac-testing` §4 and `docs/planning/v0.1/09-test-strategy.md` §5).
    pub fn methods_sent(&self) -> Vec<String> {
        self.transcript
            .iter()
            .filter(|f| f.direction == Direction::ToServer)
            .filter_map(|f| json::parse(f.text.as_bytes()).ok())
            .filter_map(|v| member_str(&v, "method").map(str::to_owned))
            .collect()
    }

    /// The tool name of every `tools/call` the fake sent, in order. With
    /// [`FakeClaude::methods_sent`], this lets a suite count inbox-fetch style tool calls
    /// directly: a channel harness makes none unless the driver (the model) asks it to.
    pub fn tools_called(&self) -> Vec<String> {
        self.transcript
            .iter()
            .filter(|f| f.direction == Direction::ToServer)
            .filter_map(|f| json::parse(f.text.as_bytes()).ok())
            .filter(|v| member_str(v, "method") == Some("tools/call"))
            .filter_map(|v| {
                member(&v, "params")
                    .and_then(|p| member_str(p, "name"))
                    .map(str::to_owned)
            })
            .collect()
    }

    /// Session events so far.
    pub fn events(&self) -> &[SessionEvent] {
        &self.events
    }

    /// Takes the session events so far.
    pub fn take_events(&mut self) -> Vec<SessionEvent> {
        std::mem::take(&mut self.events)
    }

    /// Takes the frames to write to the server, each one line of JSON text without its
    /// terminator.
    pub fn take_outbound(&mut self) -> Vec<String> {
        self.outbound.drain(..).collect()
    }

    /// The outcome of the `tools/call` with request id `id`.
    pub fn call_outcome(&self, id: u64) -> Option<&CallOutcome> {
        self.calls.get(&id)
    }

    /// Takes one frame from the server. Nothing is ever sent in answer to a channel
    /// notification.
    pub fn receive(&mut self, frame: &str) {
        self.transcript.push(Frame {
            direction: Direction::FromServer,
            text: frame.to_owned(),
        });
        if self.halted.is_some() {
            return;
        }
        let v = match json::parse(frame.as_bytes()) {
            Ok(v) => v,
            Err(json::JsonError::DuplicateMember(name)) => {
                return self.halt(&format!(
                    "a frame that repeats member name {name:?} (for example a duplicate meta key)"
                ));
            }
            Err(e) => return self.halt(&format!("a frame that is not I-JSON ({e})")),
        };
        if v.as_object().is_none() {
            return self.halt("a frame that is not a JSON object");
        }
        if member_str(&v, "jsonrpc") != Some("2.0") {
            return self.halt("a frame without \"jsonrpc\":\"2.0\"");
        }
        match (member_str(&v, "method"), member(&v, "id")) {
            (Some(m), Some(_)) => self.halt(&format!("a server-initiated request `{m}`")),
            (Some(CHANNEL_NOTIFICATION), None) => self.on_channel(&v),
            (Some(m), None) => self.halt(&format!("a server notification `{m}`")),
            (None, Some(id)) => {
                let key = id.to_compact();
                self.on_response(&key, &v)
            }
            (None, None) => self.halt("a frame with neither `method` nor `id`"),
        }
    }

    /// The terminal user starts a turn.
    pub fn start_turn(&mut self) -> Result<(), FakeError> {
        self.check_live()?;
        if self.in_turn {
            return Err(FakeError::AlreadyInTurn);
        }
        self.in_turn = true;
        Ok(())
    }

    /// A tool call of the running turn completed: the point where Claude Code takes
    /// queued channel input (G1-result criterion 3). Releases per [`MidTurnRelease`].
    pub fn tool_boundary(&mut self) -> Result<(), FakeError> {
        self.check_live()?;
        if !self.in_turn {
            return Err(FakeError::NotInTurn);
        }
        let n = match self.config.release {
            MidTurnRelease::OnePerBoundary => self.queue.len().min(1),
            MidTurnRelease::AllAtBoundary => self.queue.len(),
        };
        let wrapper = &Evidence::get().midturn;
        for tag in self.queue.drain(..n) {
            let rendered = tag
                .rendered
                .clone()
                .map(|t| wrapper.wrap(&self.config.source, &t));
            self.events.push(SessionEvent::MidTurn { tag, rendered });
        }
        Ok(())
    }

    /// The running turn ends. Refused while notifications are queued: what the harness
    /// does with them then is not recorded.
    pub fn end_turn(&mut self) -> Result<(), FakeError> {
        self.check_live()?;
        if !self.in_turn {
            return Err(FakeError::NotInTurn);
        }
        if !self.queue.is_empty() {
            return Err(FakeError::Unsupported(Unsupported {
                what: format!(
                    "a turn ending with {} channel notification(s) still queued (call tool_boundary first)",
                    self.queue.len()
                ),
            }));
        }
        self.in_turn = false;
        Ok(())
    }

    /// The model calls tool `name` with `arguments` (JSON object text) during the running
    /// turn. Returns the request id. `tool_use_id` is the model-assigned
    /// `claudecode/toolUseId`; a synthetic one is written when it is `None`.
    pub fn call_tool(
        &mut self,
        name: &str,
        arguments: &str,
        tool_use_id: Option<&str>,
    ) -> Result<u64, FakeError> {
        self.check_live()?;
        if self.phase != Phase::Ready {
            return Err(FakeError::NotReady);
        }
        if self.era == Some(Era::Modern) {
            return Err(FakeError::Unsupported(Unsupported {
                what: "a tools/call on the modern era over stdio".into(),
            }));
        }
        if !self.in_turn {
            return Err(FakeError::NotInTurn);
        }
        if !self.tools.iter().any(|t| t == name) {
            return Err(FakeError::UnknownTool(name.to_owned()));
        }
        let args = json::parse(arguments.as_bytes()).map_err(|_| FakeError::BadArguments)?;
        if args.as_object().is_none() {
            return Err(FakeError::BadArguments);
        }
        let id = self.next_call_id;
        self.next_call_id += 1;
        let synthetic;
        let tool_use_id = match tool_use_id {
            Some(t) => t,
            None => {
                synthetic = format!("toolu_fake{id:020}");
                &synthetic
            }
        };
        // Member order as recorded (D6 line 13): `Evidence::get` checks the recorded order,
        // and `replays_the_d6_capture` checks this frame against the recorded one.
        let frame = format!(
            "{{\"method\":\"tools/call\",\"params\":{{\"name\":{},\"arguments\":{},\"_meta\":{{\"claudecode/toolUseId\":{},\"progressToken\":{id}}}}},\"jsonrpc\":\"2.0\",\"id\":{id}}}",
            jstr(name),
            args.to_compact(),
            jstr(tool_use_id),
        );
        self.calls.insert(id, CallOutcome::Pending);
        self.pending.insert(id.to_string(), Pending::ToolsCall);
        self.push_out(frame);
        Ok(id)
    }

    fn check_live(&self) -> Result<(), FakeError> {
        match &self.halted {
            Some(u) => Err(FakeError::Halted(u.clone())),
            None => Ok(()),
        }
    }

    fn halt(&mut self, what: &str) {
        self.phase = Phase::Halted;
        self.halted = Some(Unsupported {
            what: what.to_owned(),
        });
    }

    fn push_out(&mut self, text: String) {
        self.transcript.push(Frame {
            direction: Direction::ToServer,
            text: text.clone(),
        });
        self.outbound.push_back(text);
    }

    fn send_request(&mut self, recorded: &Json, kind: Pending) {
        let id = member(recorded, "id")
            .expect("a recorded request has an id")
            .to_compact();
        self.pending.insert(id, kind);
        self.push_out(recorded.to_compact());
    }

    fn on_response(&mut self, id: &str, v: &Json) {
        let Some(kind) = self.pending.remove(id) else {
            return self.halt(&format!("a response with unknown id {id}"));
        };
        let result = member(v, "result");
        let error = member(v, "error");
        match kind {
            Pending::Discover => self.on_discover(result, error),
            Pending::Initialize => self.on_initialize(result),
            Pending::ToolsList => self.on_tools_list(result),
            Pending::ToolsCall => {
                let outcome = match (result, error) {
                    (Some(r), None) => CallOutcome::Result(r.clone()),
                    (None, Some(e)) => CallOutcome::Error(e.clone()),
                    _ => {
                        return self.halt(
                            "a tools/call response with neither or both of result and error",
                        );
                    }
                };
                let n: u64 = id
                    .parse()
                    .expect("tool call ids are numbers the fake chose");
                self.calls.insert(n, outcome);
            }
        }
    }

    fn on_discover(&mut self, result: Option<&Json>, error: Option<&Json>) {
        match (result, error) {
            // D6 lines 4-5, G1 Box C 14-15, G5 12-13, G4 8 then 14: legacy fallback.
            (None, Some(e))
                if member(e, "code").and_then(Json::as_number).map(|n| n.raw())
                    == Some("-32601") =>
            {
                self.era = Some(Era::Legacy);
                self.phase = Phase::Initializing;
                let req = Evidence::get().initialize.clone();
                self.send_request(&req, Pending::Initialize);
            }
            (None, Some(_)) => self.halt("a server/discover error other than -32601"),
            // G4 lines 7 then 12: a modern result keeps the client modern.
            (Some(r), None) => {
                let modern = member(r, "supportedVersions")
                    .and_then(Json::as_array)
                    .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(MODERN_REVISION)));
                if !modern {
                    return self
                        .halt("a server/discover result whose supportedVersions lacks 2026-07-28");
                }
                self.era = Some(Era::Modern);
                self.phase = Phase::Listing;
                let req = Evidence::get().tools_list_modern.clone();
                self.send_request(&req, Pending::ToolsList);
            }
            _ => self.halt("a server/discover response with neither or both of result and error"),
        }
    }

    fn on_initialize(&mut self, result: Option<&Json>) {
        let Some(r) = result else {
            return self.halt("an initialize response that is not a result");
        };
        if member_str(r, "protocolVersion") != Some(LEGACY_REVISION) {
            return self.halt("an initialize result at a revision other than 2025-11-25");
        }
        let declared = member(r, "capabilities")
            .and_then(|c| member(c, "experimental"))
            .and_then(|x| member(x, "claude/channel"))
            .is_some_and(|c| c.as_object().is_some());
        if !declared {
            // Every recorded legacy server declared it; what Claude Code does with a
            // channel notification from one that does not is not recorded.
            return self.halt(
                "an initialize result without capabilities.experimental[\"claude/channel\"]",
            );
        }
        self.registered = true;
        self.phase = Phase::Listing;
        let ev = Evidence::get();
        let initialized = ev.initialized.to_compact();
        self.push_out(initialized);
        let req = ev.tools_list_legacy.clone();
        self.send_request(&req, Pending::ToolsList);
    }

    fn on_tools_list(&mut self, result: Option<&Json>) {
        let names: Option<Vec<String>> = result
            .and_then(|r| member(r, "tools"))
            .and_then(Json::as_array)
            .and_then(|a| {
                a.iter()
                    .map(|t| member_str(t, "name").map(str::to_owned))
                    .collect()
            });
        match names {
            Some(n) => {
                self.tools = n;
                self.phase = Phase::Ready;
            }
            None => self.halt("a tools/list response without a tools array of named tools"),
        }
    }

    fn on_channel(&mut self, v: &Json) {
        if self.phase != Phase::Ready {
            return self.halt("a channel notification before tools/list completed");
        }
        let params = member(v, "params");
        let content = params
            .and_then(|p| member_str(p, "content"))
            .map(str::to_owned);
        if self.era == Some(Era::Modern) {
            self.events.push(SessionEvent::Ignored {
                reason: IgnoredReason::ModernEra,
                content,
            });
            return;
        }
        let Some(content) = content else {
            return self.halt("a channel notification without a string content");
        };
        let meta: Option<Vec<(String, String)>> = params
            .and_then(|p| member(p, "meta"))
            .and_then(Json::as_object)
            .and_then(|o| {
                o.iter()
                    .map(|(k, v)| v.as_str().map(|s| (k.to_owned(), s.to_owned())))
                    .collect()
            });
        let Some(meta) = meta else {
            return self.halt("a channel notification without a meta object of string values");
        };
        let tag = match render::render(&self.config.source, &content, &meta) {
            Ok(t) => t,
            Err(key) => {
                return self.halt(&format!(
                    "a meta key {key:?} whose fate no fixture or document fixes"
                ));
            }
        };
        if self.in_turn {
            self.queue.push_back(tag);
        } else {
            self.in_turn = true;
            self.events.push(SessionEvent::Wake(tag));
        }
    }
}

/// A JSON string literal.
fn jstr(s: &str) -> String {
    Json::String(s.to_owned()).to_compact()
}
