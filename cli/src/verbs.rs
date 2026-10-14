// SPDX-License-Identifier: Apache-2.0

//! The `oac` verbs that exist so far (G9, #70), and their dispatch.
//!
//! Only `hook` is here. `start`, `mcp-shim` and the rest of the C2 §6 table arrive with the
//! daemon and the local IPC; until then they are reported as unknown rather than faked. The
//! dispatch takes its streams and the way to reach the daemon as arguments, so a test drives
//! it with in-memory ones and the binary passes the process's own.

use std::io::{self, Read, Write};

use crate::hook;

/// Exit status for a verb that is missing or unknown (a usage error).
pub const EXIT_USAGE: i32 = 2;

/// The streams of one `oac` invocation.
pub struct Streams<'a> {
    /// Standard input.
    pub stdin: &'a mut dyn Read,
    /// Standard output. `oac hook` never writes it ([MCPB-CLD-007]).
    pub stdout: &'a mut dyn Write,
    /// Standard error: diagnostics only.
    pub stderr: &'a mut dyn Write,
}

/// How a verb reaches the daemon: opens the local connection for the harness label, with the
/// peer authentication of the local IPC. The binary has none until that slice lands.
pub type Connector<'a> = &'a mut dyn FnMut(&str) -> io::Result<Box<dyn Write>>;

/// Runs `args` (everything after the program name) and returns the exit status.
pub fn dispatch(args: &[String], streams: Streams<'_>, connect: Connector<'_>) -> i32 {
    let Streams {
        stdin,
        stdout: _,
        stderr,
    } = streams;
    match args.first().map(String::as_str) {
        Some("hook") => {
            // The handler is configured by OAC's own launch script and takes no argument;
            // one that appears is noted and ignored, so a hook is never failed for it.
            if args.len() > 1 {
                let _ = writeln!(stderr, "oac hook: arguments are ignored");
            }
            let outcome = hook::run(stdin, connect);
            if let Some(line) = outcome.diagnostic() {
                let _ = writeln!(stderr, "{line}");
            }
            // Always 0 (see `hook`): a non-zero exit would only add a hook-error notice, and a
            // misfire is better quiet.
            0
        }
        Some(other) => {
            let _ = writeln!(stderr, "oac: unknown command {other:?}");
            EXIT_USAGE
        }
        None => {
            let _ = writeln!(stderr, "usage: oac <command>");
            EXIT_USAGE
        }
    }
}
