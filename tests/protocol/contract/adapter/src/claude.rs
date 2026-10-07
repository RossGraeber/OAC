// SPDX-License-Identifier: Apache-2.0

//! The harness over the fake Claude Code channel endpoint (F8, `tests/fakes/claude`).
//!
//! On the channel path Claude Code starts the channel server and speaks MCP to it over
//! stdio; on the v0.1 process model that byte stream reaches the core as one local
//! connection (`docs/planning/decisions/C2-process-model.md` §1). [`ClaudeHarness`] gives
//! the adapter exactly that: a [`Connection`] whose bytes are the fake's newline-delimited
//! JSON frames. Any channel adapter therefore plugs in unchanged.
//!
//! The only binding detail the harness does not fix is the tool arguments: the
//! `inputSchema` of each tool is the adapter's (`spec/bindings/mcp.md` [MCPB-TOOL-004]),
//! so a [`ToolEncoder`] turns a [`HarnessRequest`] into a tool name and its arguments.
//! Results are read the binding's way: an error result has `isError: true`, and the text
//! content carries the code, or the message id and `accepted-by-adapter`
//! ([MCPB-TOOL-006], [MCPB-TOOL-008], [MCPB-TOOL-010], [MCPB-TOOL-011]).

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use oac_core::adapter::{Connection, ProviderAdapter};
use oac_core::json::{self, Json};
use oac_fake_claude::evidence::{member, member_str};
use oac_fake_claude::{
    CHANNEL_NOTIFICATION, CallOutcome, Config, Direction, FakeClaude, MidTurnRelease, Phase,
    SessionEvent,
};

use crate::{
    AdapterHarness, CoreSide, Gap, HandOffCall, HarnessRequest, Observations, ObservedResult,
    Profile, Step, WAIT,
};

/// How long a harness request waits for the adapter's answer.
pub const REQUEST_WAIT: Duration = Duration::from_secs(3);

/// The four tools of `spec/bindings/mcp.md` §5.1 ([MCPB-TOOL-001]). None of them returns
/// pending messages on demand.
pub const OAC_TOOLS: [&str; 4] = ["send", "reply", "list_sessions", "whoami"];

/// MCP methods that read a resource, a prompt or a subscription on demand: the polling
/// surfaces of [MCPB-DLV-002].
pub const FETCH_METHODS: [&str; 6] = [
    "resources/list",
    "resources/read",
    "resources/subscribe",
    "prompts/list",
    "prompts/get",
    "subscriptions/listen",
];

/// Server capabilities that offer something a session could read repeatedly: an inbox
/// shape that [MCPB-DLV-002] forbids while active inbound is claimed.
pub const FETCH_CAPABILITIES: [&str; 3] = ["resources", "prompts", "subscriptions"];

/// The opening methods: the first no-polling class.
pub const ESTABLISHMENT_METHODS: [&str; 3] = ["server/discover", "initialize", "tools/list"];

/// The Claude channel path's profile: the channel notification is its only input surface;
/// it has no holding hand-off and no steering operation (`spec/bindings/mcp.md` §8.1).
pub const PROFILE: Profile = Profile {
    name: "claude-channel",
    holding_hand_off: None,
    steering_operations: &[],
    rules: &[],
};

/// Turns a request into a `tools/call`: the tool name and the arguments as JSON object
/// text.
pub type ToolEncoder = fn(&HarnessRequest) -> (String, String);

/// A JSON string literal.
pub fn json_string(s: &str) -> String {
    Json::String(s.to_owned()).to_compact()
}

// ---- an in-process byte pipe ------------------------------------------------------------

/// The writing end of a pipe.
pub struct PipeWriter(Sender<Vec<u8>>);

impl Write for PipeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .send(buf.to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"))?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The reading end of a pipe. It blocks until bytes arrive, and reads end of file once
/// the writer is gone.
pub struct PipeReader {
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for PipeReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.pos >= self.buf.len() {
            match self.rx.recv() {
                Ok(b) => {
                    self.buf = b;
                    self.pos = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// A pipe: what is written to the writer is read from the reader.
pub fn pipe() -> (PipeWriter, PipeReader) {
    let (tx, rx) = mpsc::channel();
    (
        PipeWriter(tx),
        PipeReader {
            rx,
            buf: Vec::new(),
            pos: 0,
        },
    )
}

// ---- the harness --------------------------------------------------------------------------

struct ClaudeSession {
    fake: FakeClaude,
    to_adapter: Option<PipeWriter>,
    from_adapter: Receiver<Vec<u8>>,
    partial: Vec<u8>,
}

impl ClaudeSession {
    fn take_chunk(&mut self, chunk: Vec<u8>) {
        self.partial.extend_from_slice(&chunk);
        while let Some(i) = self.partial.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=i).collect();
            let text = String::from_utf8_lossy(&line);
            let text = text.trim_end_matches(['\r', '\n']);
            if !text.is_empty() {
                self.fake.receive(text);
            }
        }
    }

    /// Move every frame that is ready, both ways.
    fn pump(&mut self) {
        while let Ok(c) = self.from_adapter.try_recv() {
            self.take_chunk(c);
        }
        let out = self.fake.take_outbound();
        if let Some(w) = self.to_adapter.as_mut() {
            for f in out {
                let _ = w.write_all(format!("{f}\n").as_bytes());
            }
        }
    }

    /// Pump until `cond` holds, or `timeout` passes.
    fn pump_until(&mut self, cond: impl Fn(&FakeClaude) -> bool, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        loop {
            self.pump();
            if cond(&self.fake) {
                return true;
            }
            let now = Instant::now();
            if now >= end {
                return false;
            }
            match self
                .from_adapter
                .recv_timeout((end - now).min(Duration::from_millis(20)))
            {
                Ok(c) => self.take_chunk(c),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.pump();
                    return cond(&self.fake);
                }
            }
        }
    }

    fn release_and_end(&mut self) -> Step<()> {
        self.pump();
        let err = |e: oac_fake_claude::FakeError| Gap::Broken(e.to_string());
        if self.fake.in_turn() {
            while self.fake.queued() > 0 {
                self.fake.tool_boundary().map_err(err)?;
            }
            self.fake.end_turn().map_err(err)?;
        }
        Ok(())
    }
}

/// The adapter contract suite's harness over the fake Claude Code channel endpoint.
pub struct ClaudeHarness {
    adapter: Arc<dyn ProviderAdapter>,
    release: MidTurnRelease,
    encode: ToolEncoder,
    files: Vec<PathBuf>,
    sessions: Vec<ClaudeSession>,
}

impl ClaudeHarness {
    /// A harness for `adapter`, with the fake releasing queued notifications per
    /// `release`, tool arguments from `encode`, and the adapter's source `files` for the
    /// static checks.
    pub fn new(
        adapter: Arc<dyn ProviderAdapter>,
        release: MidTurnRelease,
        encode: ToolEncoder,
        files: Vec<PathBuf>,
    ) -> ClaudeHarness {
        ClaudeHarness {
            adapter,
            release,
            encode,
            files,
            sessions: Vec::new(),
        }
    }

    fn session(&mut self, s: usize) -> Step<&mut ClaudeSession> {
        self.sessions
            .get_mut(s)
            .ok_or_else(|| Gap::Broken(format!("no session {s}")))
    }
}

/// The channel notifications a server sent to `f`: its hand-off calls.
fn channel_calls(f: &FakeClaude) -> Vec<HandOffCall> {
    f.transcript()
        .iter()
        .filter(|fr| fr.direction == Direction::FromServer)
        .filter_map(|fr| json::parse(fr.text.as_bytes()).ok())
        .filter(|v| member_str(v, "method") == Some(CHANNEL_NOTIFICATION))
        .map(|v| HandOffCall {
            operation: CHANNEL_NOTIFICATION.to_owned(),
            text: member(&v, "params")
                .and_then(|p| member_str(p, "content"))
                .unwrap_or_default()
                .to_owned(),
            steering: false,
            override_members: Vec::new(),
            accepted: f.halted().is_none(),
        })
        .collect()
}

fn halted(fake: &FakeClaude) -> Option<String> {
    fake.halted().map(ToString::to_string)
}

impl AdapterHarness for ClaudeHarness {
    fn name(&self) -> String {
        format!("fake Claude Code channel endpoint ({:?})", self.release)
    }

    fn adapter(&self) -> Arc<dyn ProviderAdapter> {
        self.adapter.clone()
    }

    fn profile(&self) -> Profile {
        PROFILE
    }

    fn open_session(&mut self, core: &CoreSide) -> Step<(usize, Vec<Connection>)> {
        let config = Config::new("oac")
            .map_err(|e| Gap::Broken(e.to_string()))?
            .with_release(self.release);
        let (to_adapter, adapter_reads) = pipe();
        let (adapter_writes, from_adapter) = mpsc::channel();
        let conn = core.accept(adapter_reads, PipeWriter(adapter_writes));
        let mut s = ClaudeSession {
            fake: FakeClaude::new(config),
            to_adapter: Some(to_adapter),
            from_adapter,
            partial: Vec::new(),
        };
        s.pump();
        self.sessions.push(s);
        Ok((self.sessions.len() - 1, vec![conn]))
    }

    fn session_ready(&mut self, s: usize) -> Step<()> {
        let sess = self.session(s)?;
        let ok = sess.pump_until(|f| matches!(f.phase(), Phase::Ready | Phase::Halted), WAIT);
        if let Some(h) = halted(&sess.fake) {
            return Err(Gap::Broken(format!("the fake halted while opening: {h}")));
        }
        if !ok {
            return Err(Gap::Broken(format!(
                "the MCP opening did not complete (phase {:?})",
                sess.fake.phase()
            )));
        }
        Ok(())
    }

    fn end_session(&mut self, s: usize) -> Step<()> {
        let sess = self.session(s)?;
        sess.pump();
        sess.to_adapter = None;
        Ok(())
    }

    fn start_turn(&mut self, s: usize) -> Step<()> {
        let sess = self.session(s)?;
        sess.pump();
        if sess.fake.in_turn() {
            return Ok(());
        }
        sess.fake
            .start_turn()
            .map_err(|e| Gap::Broken(e.to_string()))
    }

    fn drain(&mut self, s: usize) -> Step<()> {
        self.session(s)?.release_and_end()
    }

    fn refuse_hand_offs(&mut self, _s: usize) -> Step<()> {
        Err(Gap::Unsupported(
            "Claude Code gives no answer to a channel notification, so it cannot turn one away"
                .into(),
        ))
    }

    fn allow_hand_offs(&mut self, _s: usize) -> Step<()> {
        Ok(())
    }

    fn request(&mut self, s: usize, request: &HarnessRequest) -> Step<ObservedResult> {
        let (tool, args) = (self.encode)(request);
        let sess = self.session(s)?;
        sess.pump();
        let started = !sess.fake.in_turn();
        if started {
            sess.fake
                .start_turn()
                .map_err(|e| Gap::Broken(e.to_string()))?;
        }
        let id = sess
            .fake
            .call_tool(&tool, &args, None)
            .map_err(|e| Gap::Broken(e.to_string()))?;
        let answered = sess.pump_until(
            |f| !matches!(f.call_outcome(id), Some(CallOutcome::Pending) | None),
            REQUEST_WAIT,
        );
        let outcome = sess.fake.call_outcome(id).cloned();
        if started {
            // The tool call's end is a tool-call boundary; then the turn ends.
            sess.release_and_end()?;
        }
        if !answered {
            return Err(Gap::Broken(format!("no answer to tools/call {tool}")));
        }
        Ok(match outcome {
            Some(CallOutcome::Result(r)) => ObservedResult {
                is_error: member(&r, "isError").and_then(Json::as_bool) == Some(true),
                text: member(&r, "content")
                    .and_then(Json::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|c| member_str(c, "text"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default(),
            },
            Some(CallOutcome::Error(e)) => ObservedResult {
                is_error: true,
                text: e.to_compact(),
            },
            _ => return Err(Gap::Broken("no outcome".into())),
        })
    }

    fn observe(&mut self, s: usize) -> Observations {
        for other in &mut self.sessions {
            other.pump();
        }
        let client_hand_off_calls: Vec<HandOffCall> = self
            .sessions
            .iter()
            .flat_map(|x| channel_calls(&x.fake))
            .collect();
        let Ok(sess) = self.session(s) else {
            return Observations::default();
        };
        let f = &sess.fake;
        let hand_off_calls = channel_calls(f);
        let inputs = f
            .events()
            .iter()
            .filter_map(|e| match e {
                SessionEvent::Wake(tag) | SessionEvent::MidTurn { tag, .. } => {
                    Some(tag.content.clone())
                }
                SessionEvent::Ignored { .. } => None,
            })
            .collect();
        let sent = f.methods_sent();
        let mut fetch_calls: Vec<String> = sent
            .iter()
            .filter(|m| FETCH_METHODS.contains(&m.as_str()))
            .cloned()
            .collect();
        fetch_calls.extend(
            f.tools_called()
                .into_iter()
                .filter(|t| !OAC_TOOLS.contains(&t.as_str()))
                .map(|t| format!("tools/call {t}")),
        );
        // [MCPB-DLV-002]: a server capability that offers resources, prompts or
        // subscriptions is an inbox the session could poll, whether or not it ever does.
        let mut offered_fetch_surfaces: Vec<String> = f
            .tools()
            .iter()
            .filter(|t| !OAC_TOOLS.contains(&t.as_str()))
            .map(|t| format!("tool {t}"))
            .collect();
        for fr in f.transcript() {
            if fr.direction != Direction::FromServer {
                continue;
            }
            let Ok(v) = json::parse(fr.text.as_bytes()) else {
                continue;
            };
            let caps = member(&v, "result").and_then(|r| member(r, "capabilities"));
            for c in FETCH_CAPABILITIES {
                if caps.and_then(|x| member(x, c)).is_some() {
                    offered_fetch_surfaces.push(format!("capability {c}"));
                }
            }
        }
        Observations {
            hand_off_calls,
            client_hand_off_calls,
            inputs,
            held: f.queued(),
            establishment_calls: sent
                .iter()
                .filter(|m| ESTABLISHMENT_METHODS.contains(&m.as_str()))
                .count(),
            fetch_calls,
            offered_fetch_surfaces,
            halted: halted(f),
        }
    }

    fn source_files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }

    /// The Claude path's native ids that reach the server: every `claudecode/toolUseId`
    /// seen, and the `toolu_` prefix itself so that a leak of one never seen is caught too.
    /// The JSON-RPC request ids are bare numbers, which no text check can tell from other
    /// numbers, and the channel path gives the server no session id (no fixture records
    /// one), so neither is listed.
    fn native_ids(&self) -> Vec<String> {
        let mut ids = vec!["toolu_".to_owned()];
        for s in &self.sessions {
            for fr in s.fake.transcript() {
                if fr.direction != Direction::ToServer {
                    continue;
                }
                let Ok(v) = json::parse(fr.text.as_bytes()) else {
                    continue;
                };
                if let Some(t) = member(&v, "params")
                    .and_then(|p| member(p, "_meta"))
                    .and_then(|m| member_str(m, "claudecode/toolUseId"))
                {
                    ids.push(t.to_owned());
                }
            }
        }
        ids
    }
}
