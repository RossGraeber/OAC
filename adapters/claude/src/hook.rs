// SPDX-License-Identifier: Apache-2.0

//! The Claude Code hook surface: the supported source of the harness-native id N
//! (`docs/planning/decisions/C4-session-identity.md` §3; G5, #66).
//!
//! Claude Code runs a configured command hook with a JSON object on its standard input. For
//! `SessionStart` that object carries `session_id` ("Current session identifier"),
//! `hook_event_name` and `source` ("How the session started": `startup`, `resume`, `clear`,
//! `compact` or `fork`), beside fields this adapter does not read (`transcript_path`, `cwd`,
//! `model`, ...). Source: https://code.claude.com/docs/en/hooks.md, "Common input fields"
//! and "SessionStart input", retrieved 2026-10-12; the payload shape was also captured from
//! Claude Code `2.1.285` that day (`SessionStart` with `source` `startup`, and again with
//! `resume` for `--resume <id>`, the same `session_id` in both).
//!
//! The adapter reads N nowhere else. It reads no environment variable, no session file and
//! no transcript: `CLAUDE_CODE_SESSION_ID`, which Claude Code documents too, is a
//! cross-check value and the core's business (C4 §3 "Revision, 2026-10-02"), never N.
//!
//! # The carrier connection
//!
//! The hook handler is an `oac` command (G9/G10, `oac hook`) that Claude Code starts. It
//! sends the hook's input to the daemon over local IPC, and the core process hands that
//! connection to this adapter like any other ([`ProviderAdapter::take_connection`]). A
//! connection is a **carrier of native signals** rather than an attachment when its first
//! line is not an MCP frame but a hook input: a JSON object with a `hook_event_name` and no
//! `jsonrpc` member.
//!
//! What the handler must do (`spec/bindings/mcp.md` [MCPB-CLD-004] to [MCPB-CLD-008], §8.1),
//! because this adapter classifies by the first line:
//!
//! - **Parse its standard input as JSON and send it again as one compact line**, ended by a
//!   newline or by end of stream. It must not copy the bytes through: the hooks reference
//!   does not say the input is one line (its examples are written over several lines),
//!   and a copy that began `{` and a newline would be taken for something that is not a
//!   hook input, so the signal would be lost (fail closed, nothing bound). Claude Code
//!   `2.1.285` wrote `SessionStart` as one line when captured on 2026-10-12, but nothing
//!   documents that.
//! - **Write nothing to its own standard output** (and exit 0): a `SessionStart` hook's
//!   standard output is added to Claude's context. This adapter never writes to a carrier.
//! - Configure only `SessionStart`: it is the only event that is a signal (below).
//!
//! Only `SessionStart` is a native signal. Any other event on a carrier is not one and is
//! ignored: in particular `SessionEnd` does not deregister anything, because it is not
//! guaranteed to run (its budget is 1.5 s by default, and a killed process runs none). An
//! attachment ends when its connection ends ([IFC-ADP-022], [SC-ID-155]), whatever hooks ran.
//!
//! What the adapter does not decide ([IFC-ADP-020], [SC-ID-120]): which attachment the
//! signal belongs to, whether it binds, and under which session id. The signal carries N and
//! the start kind; the core pairs it by what it observed itself about the connection and
//! applies the ordered cases of `spec/session-channels.md` §6.7.3.
//!
//! # Start kinds
//!
//! C4 §3 and §6: `startup` is a fresh start. `resume`, `clear`, `fork` and `compact` are
//! transitions: a new OAC session id, nothing carried forward, and the cross-check value is
//! not compared. Claude Code reports `source` `resume` for `--resume <id>` with the *same*
//! `session_id` as before, so N alone never says whether the OAC session is a continuation;
//! the adapter does not try to tell (B2: behavior across `--resume` is UNVERIFIED, so a
//! resumed session is a new one). A `source` that is absent is reported as no start kind; one
//! that is not among the five documented values is reported as `unmapped`. The core treats
//! both as `fresh` ([SC-ID-135]), which is the fail-closed default.

use oac_core::adapter::StartKind;
use oac_core::json::{self, Json};

/// The most bytes of a hook input the adapter reads. Claude Code's `SessionStart` input is a
/// few hundred bytes plus paths.
pub const MAX_HOOK_INPUT: usize = 64 * 1024;

/// The hook event whose input is a native signal.
pub const SESSION_START: &str = "SessionStart";

/// The longest harness-native id the adapter reports. Claude's ids are 36 characters; the
/// value is only ever compared as an exact string ([SC-ID-143]).
pub const MAX_NATIVE_ID: usize = 256;

/// What the first line of a connection is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FirstLine {
    /// An MCP frame, or anything that is not a hook input: served as the channel server.
    Mcp,
    /// A hook input.
    Hook(HookInput),
}

/// A hook input, reduced to what the adapter reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookInput {
    event: String,
    session_id: Option<String>,
    source: Option<Json>,
}

/// The native signal a `SessionStart` input is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionStart {
    /// N: the hook input's `session_id`.
    pub native_id: String,
    /// The start kind, from `source`.
    pub start_kind: Option<StartKind>,
}

/// Classifies the first line of a connection.
pub fn classify(line: &[u8]) -> FirstLine {
    let Ok(value) = json::parse(trim_line_end(line)) else {
        return FirstLine::Mcp;
    };
    let Some(object) = value.as_object() else {
        return FirstLine::Mcp;
    };
    if object.contains("jsonrpc") {
        return FirstLine::Mcp;
    }
    let Some(event) = object.get("hook_event_name").and_then(Json::as_str) else {
        return FirstLine::Mcp;
    };
    FirstLine::Hook(HookInput {
        event: event.to_owned(),
        session_id: object
            .get("session_id")
            .and_then(Json::as_str)
            .map(str::to_owned),
        source: object.get("source").cloned(),
    })
}

fn trim_line_end(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

impl HookInput {
    /// The native signal, when this is a `SessionStart` input with a usable `session_id`.
    /// Any other event, a missing, empty or over-long `session_id`, or one that is not a
    /// string, is no signal.
    pub fn session_start(&self) -> Option<SessionStart> {
        if self.event != SESSION_START {
            return None;
        }
        let native_id = self.session_id.as_deref()?;
        if native_id.is_empty() || native_id.len() > MAX_NATIVE_ID {
            return None;
        }
        Some(SessionStart {
            native_id: native_id.to_owned(),
            start_kind: start_kind(self.source.as_ref()),
        })
    }
}

/// The start kind for a `source` value (C4 §3; `spec/session-channels.md` §6.7.1).
fn start_kind(source: Option<&Json>) -> Option<StartKind> {
    let source = source?;
    Some(match source.as_str() {
        Some("startup") => StartKind::Fresh,
        Some("resume" | "clear" | "fork" | "compact") => StartKind::Transition,
        _ => StartKind::Unmapped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `SessionStart` input as Claude Code `2.1.285` wrote it on 2026-10-12 (paths
    /// shortened): the fields beyond the three the adapter reads are ignored.
    const STARTUP: &str = "{\"session_id\":\"f40e86db-b0cb-439a-bb2d-17f1dd86c1d5\",\"transcript_path\":\"C:\\\\Users\\\\u\\\\.claude\\\\projects\\\\p\\\\f40e86db-b0cb-439a-bb2d-17f1dd86c1d5.jsonl\",\"cwd\":\"C:\\\\work\",\"hook_event_name\":\"SessionStart\",\"source\":\"startup\"}\n";

    fn signal(line: &str) -> Option<SessionStart> {
        match classify(line.as_bytes()) {
            FirstLine::Hook(h) => h.session_start(),
            FirstLine::Mcp => panic!("not a hook input: {line}"),
        }
    }

    #[test]
    fn a_recorded_session_start_is_a_fresh_signal_with_its_session_id() {
        let s = signal(STARTUP).expect("a signal");
        assert_eq!(s.native_id, "f40e86db-b0cb-439a-bb2d-17f1dd86c1d5");
        assert_eq!(s.start_kind, Some(StartKind::Fresh));
        // Ended by end of stream rather than a newline, and by a carriage return too.
        assert_eq!(signal(STARTUP.trim_end()), Some(s.clone()));
        assert_eq!(signal(&STARTUP.replace('\n', "\r\n")), Some(s));
    }

    #[test]
    fn every_documented_source_maps_and_nothing_else_is_guessed() {
        let with = |source: &str| {
            signal(&format!(
                "{{\"session_id\":\"s1\",\"hook_event_name\":\"SessionStart\"{source}}}"
            ))
            .expect("a signal")
            .start_kind
        };
        assert_eq!(with(",\"source\":\"startup\""), Some(StartKind::Fresh));
        for transition in ["resume", "clear", "fork", "compact"] {
            assert_eq!(
                with(&format!(",\"source\":\"{transition}\"")),
                Some(StartKind::Transition),
                "{transition}"
            );
        }
        assert_eq!(with(""), None, "no source: no start kind");
        assert_eq!(with(",\"source\":\"teleport\""), Some(StartKind::Unmapped));
        assert_eq!(with(",\"source\":7"), Some(StartKind::Unmapped));
        assert_eq!(with(",\"source\":\"Startup\""), Some(StartKind::Unmapped));
    }

    #[test]
    fn only_a_session_start_with_a_usable_id_is_a_signal() {
        for other in [
            "SessionEnd",
            "UserPromptSubmit",
            "PreToolUse",
            "sessionstart",
        ] {
            let line = format!(
                "{{\"session_id\":\"s1\",\"hook_event_name\":\"{other}\",\"reason\":\"other\"}}"
            );
            assert_eq!(signal(&line), None, "{other}");
        }
        for bad in [
            "{\"hook_event_name\":\"SessionStart\",\"source\":\"startup\"}".to_owned(),
            "{\"session_id\":\"\",\"hook_event_name\":\"SessionStart\"}".to_owned(),
            "{\"session_id\":12,\"hook_event_name\":\"SessionStart\"}".to_owned(),
            "{\"session_id\":null,\"hook_event_name\":\"SessionStart\"}".to_owned(),
            format!(
                "{{\"session_id\":\"{}\",\"hook_event_name\":\"SessionStart\"}}",
                "x".repeat(MAX_NATIVE_ID + 1)
            ),
        ] {
            assert_eq!(signal(&bad), None, "{bad}");
        }
    }

    #[test]
    fn mcp_frames_and_noise_are_not_hook_inputs() {
        for mcp in [
            "{\"jsonrpc\":\"2.0\",\"id\":\"server-discover-probe-1\",\"method\":\"server/discover\"}\n",
            "{\"method\":\"initialize\",\"params\":{},\"jsonrpc\":\"2.0\",\"id\":0}\n",
            // A hook-shaped object that claims to be JSON-RPC is an MCP frame.
            "{\"jsonrpc\":\"2.0\",\"hook_event_name\":\"SessionStart\",\"session_id\":\"s\"}\n",
            "not json\n",
            "[1,2]\n",
            "{\"session_id\":\"s\"}\n",
            "{\"hook_event_name\":5}\n",
            "\n",
        ] {
            assert_eq!(classify(mcp.as_bytes()), FirstLine::Mcp, "{mcp}");
        }
    }
}
