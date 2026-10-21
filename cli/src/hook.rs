// SPDX-License-Identifier: Apache-2.0

//! `oac hook`: the Claude Code hook handler that carries a `SessionStart` input to the daemon
//! as a native signal (G9, #70; `spec/bindings/mcp.md` 0.3 §8.1, [MCPB-CLD-004] to
//! [MCPB-CLD-010]; `docs/planning/decisions/C4-session-identity.md` §3).
//!
//! Claude Code runs the configured command with a JSON object on its standard input. The
//! Claude adapter classifies a connection by its first line (`oac_adapter_claude::hook`),
//! so the handler must send that object in the one form the adapter recognizes:
//!
//! - **Parse, then write again** ([MCPB-CLD-004], [MCPB-CLD-005], [MCPB-CLD-006]). The input
//!   is parsed with the core's I-JSON reader, the one the adapter reads the line with, and
//!   written as compact text on one line. The input's bytes are never copied through: the
//!   hooks reference does not say the input is one line, and a copy that began `{` and a
//!   newline would be taken for something that is not a hook input.
//! - **Nothing on standard output** ([MCPB-CLD-007]). A `SessionStart` hook's standard
//!   output is added to Claude's context. This module has no handle to it: [`run`] takes
//!   the input, the carrier and a diagnostic sink only.
//! - **Only `SessionStart`** ([MCPB-CLD-008]). Registering the handler for any other event is
//!   the launch script's business (G11). If a configuration gave it another event anyway, the
//!   handler sends nothing for it: the server would not accept it as a native signal
//!   ([MCPB-CLD-009]) and must not end an attachment for it ([MCPB-CLD-010]), so the only
//!   effect of sending it would be a place held on the connection limit.
//!
//! **The handler exits 0 whatever happens**, and says why on its diagnostic stream (standard
//! error) in words that hold none of the input: a hook input names paths and a session id.
//! Per the hooks reference, a hook that exits non-zero only shows a hook-error notice to the
//! user, and nothing it prints reaches Claude's context on standard error; exit 0 keeps a
//! misfire (no daemon yet, an input that is not a signal) quiet. The diagnostic is therefore
//! not visible without debug logging, and whether "the daemon was not reached" should be
//! surfaced is left to the connector slice and G10. A signal that cannot be sent is lost, and
//! the session stays unbound: fail closed, nothing bound (C4 §3). The cost is availability,
//! never authority.
//!
//! **The production connection lingers.** `crate::commands::hook_connector` returns a
//! connection that, when [`run`] drops it, waits (bounded) for the daemon to close its end:
//! the daemon learns which harness process a connection belongs to from the operating
//! system's process table after it accepts, and this process must still be alive then.
//!
//! The handler reads its standard input to end of stream (bounded at 64 KiB); that Claude Code
//! closes the pipe after writing the input is to be confirmed in the live leg.
//!
//! The handler reads no environment variable and no key: key material lives in the daemon
//! only (G9 acceptance, box 2). The cross-check value `CLAUDE_CODE_SESSION_ID` is the shim's
//! to send (11-risks row 87), not this handler's.

use std::fmt;
use std::io::{self, Read, Write};

use oac_adapter_claude::hook::{FirstLine, MAX_HOOK_INPUT, classify};
use oac_core::json;

/// The endpoint label a hook handler connects to: the hook input is Claude Code's.
pub const HARNESS: &str = "claude";

/// Why a hook input is not sent. Nothing is sent, and nothing is bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skip {
    /// The standard input could not be read.
    Unreadable,
    /// The standard input is longer than a hook input may be ([`MAX_HOOK_INPUT`]).
    TooLarge,
    /// The standard input is not an I-JSON object (not UTF-8, not JSON, a repeated member
    /// name, or a value other than an object).
    NotAnObject,
    /// The object is not a hook input: no string `hook_event_name`, or it names `jsonrpc`.
    NotAHookInput,
    /// The event is not `SessionStart`, or its `session_id` is missing or unusable: the
    /// server would not take it as a native signal.
    NotASignal,
}

impl Skip {
    /// The stable name of the reason, for `oac hook --json`.
    pub fn code(self) -> &'static str {
        match self {
            Skip::Unreadable => "unreadable",
            Skip::TooLarge => "too_large",
            Skip::NotAnObject => "not_an_object",
            Skip::NotAHookInput => "not_a_hook_input",
            Skip::NotASignal => "not_a_signal",
        }
    }
}

impl fmt::Display for Skip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Skip::Unreadable => "the hook input could not be read",
            Skip::TooLarge => "the hook input is too large",
            Skip::NotAnObject => "the hook input is not a JSON object",
            Skip::NotAHookInput => "the JSON object is not a hook input",
            Skip::NotASignal => "not a SessionStart input with a usable session_id",
        })
    }
}

/// What [`run`] did.
#[derive(Debug)]
pub enum Outcome {
    /// The line was written to the carrier and flushed.
    Sent,
    /// Nothing was sent, by design.
    Skipped(Skip),
    /// The carrier could not be reached or written. The signal is lost.
    NotSent(io::Error),
}

impl Outcome {
    /// The line for the diagnostic stream, or `None` for [`Outcome::Sent`]. It holds nothing
    /// from the input.
    pub fn diagnostic(&self) -> Option<String> {
        match self {
            Outcome::Sent => None,
            Outcome::Skipped(why) => Some(format!("oac hook: nothing sent: {why}")),
            // The error text is the operating system's (a path or an errno), not the input's.
            Outcome::NotSent(e) => Some(format!("oac hook: the daemon was not reached: {e}")),
        }
    }
}

impl Outcome {
    /// The `--json` form of [`Outcome::diagnostic`] (G10, #71, slice 7): one canonical object
    /// with `"oac_cli":1`, for **standard error only**, and one for every outcome. It holds
    /// nothing from the input, only the reason's fixed name or the operating system's error.
    ///
    /// - sent: `{"ok":true,"sent":true}`
    /// - nothing sent, by design: `{"ok":true,"sent":false,"reason":"<Skip::code>"}`
    /// - the daemon was not reached: `{"ok":false,"sent":false,"error":"not_running"|"unreachable","detail":"..."}`
    pub fn diagnostic_json(&self) -> String {
        use crate::control::{cli_object, json_string};
        match self {
            Outcome::Sent => cli_object(&[("ok", "true".to_owned()), ("sent", "true".to_owned())]),
            Outcome::Skipped(why) => cli_object(&[
                ("ok", "true".to_owned()),
                ("sent", "false".to_owned()),
                ("reason", json_string(why.code())),
            ]),
            Outcome::NotSent(e) => cli_object(&[
                ("ok", "false".to_owned()),
                ("sent", "false".to_owned()),
                (
                    "error",
                    json_string(if e.kind() == io::ErrorKind::NotFound {
                        "not_running"
                    } else {
                        "unreachable"
                    }),
                ),
                ("detail", json_string(&e.to_string())),
            ]),
        }
    }
}

/// The line to send for `input`, the bytes read from standard input: the object as compact
/// JSON, ended by one newline, with no line feed or carriage return before it
/// ([MCPB-CLD-005]).
///
/// # Errors
///
/// A [`Skip`] when the input is not a `SessionStart` hook input the server would take as a
/// native signal.
pub fn carrier_line(input: &[u8]) -> Result<Vec<u8>, Skip> {
    let value = json::parse(input).map_err(|_| Skip::NotAnObject)?;
    if value.as_object().is_none() {
        return Err(Skip::NotAnObject);
    }
    // Written again, never copied ([MCPB-CLD-006]). String escapes cover every control
    // character, so the text holds no line feed or carriage return.
    let mut line = value.to_compact().into_bytes();
    line.push(b'\n');
    debug_assert!(
        !line[..line.len() - 1]
            .iter()
            .any(|b| matches!(b, b'\n' | b'\r'))
    );
    if line.len() > MAX_HOOK_INPUT {
        return Err(Skip::TooLarge);
    }
    // The server's own recognition rule decides what is a signal, so the two cannot drift.
    match classify(&line) {
        FirstLine::Hook(h) if h.session_start().is_some() => Ok(line),
        FirstLine::Hook(_) => Err(Skip::NotASignal),
        FirstLine::Mcp | FirstLine::Hello(_) => Err(Skip::NotAHookInput),
    }
}

/// Reads at most [`MAX_HOOK_INPUT`] bytes of `input` and no more, so a standard input that
/// never ends cannot grow the handler.
fn read_bounded(input: &mut dyn Read) -> Result<Vec<u8>, Skip> {
    let mut bytes = Vec::new();
    input
        .take(MAX_HOOK_INPUT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Skip::Unreadable)?;
    if bytes.len() > MAX_HOOK_INPUT {
        return Err(Skip::TooLarge);
    }
    Ok(bytes)
}

/// Runs the handler: reads `input` (the hook's standard input), and when it is a
/// `SessionStart` signal, calls `connect` once with [`HARNESS`] and writes the line to what
/// it returns, in one write, then flushes. `connect` is not called for anything else, so a
/// hook that carries no signal never opens a connection.
///
/// The carrier is dropped when this returns, which ends the connection: the adapter reads
/// one line from a hook connection and then closes it.
pub fn run(
    input: &mut dyn Read,
    connect: &mut dyn FnMut(&str) -> io::Result<Box<dyn Write>>,
) -> Outcome {
    let line = match read_bounded(input).and_then(|bytes| carrier_line(&bytes)) {
        Ok(line) => line,
        Err(why) => return Outcome::Skipped(why),
    };
    let sent = connect(HARNESS).and_then(|mut carrier| {
        carrier.write_all(&line)?;
        carrier.flush()
    });
    match sent {
        Ok(()) => Outcome::Sent,
        Err(e) => Outcome::NotSent(e),
    }
}
