// SPDX-License-Identifier: Apache-2.0

//! The `oac` binary.
//!
//! `oac start`, `oac mcp-shim` and `oac hook` (G9, #70) `oac stop`, `oac status`, `oac sessions` and `oac doctor`
//! (G10, #71). Every verb takes `--json` (G10, slice 7; lead decision 2026-10-20). This crate builds the adapters and the transport and hands them to the daemon,
//! which hands them to the core; it never invokes an adapter or transport contract operation
//! itself (`docs/planning/v0.1/07-repository-and-dependencies.md` section 3).

use std::io::{self, Write};

use oac_cli::commands;
use oac_cli::verbs::{Streams, dispatch};

fn main() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    // The verbs that run a process of their own take no borrowed streams. `start` reads the
    // device key's passphrase and so runs before this process has started any thread.
    match args.first().map(String::as_str) {
        Some("start") => std::process::exit(commands::start(&args[1..])),
        Some("mcp-shim") => std::process::exit(commands::mcp_shim(&args[1..])),
        Some("stop") => std::process::exit(commands::stop(&args[1..])),
        Some("status") => std::process::exit(commands::status(&args[1..])),
        Some("sessions") => std::process::exit(commands::sessions(&args[1..])),
        Some("doctor") => std::process::exit(commands::doctor(&args[1..])),
        _ => {}
    }
    // `oac hook --json` changes the form of the hook's diagnostics on standard error and
    // nothing else (`verbs::dispatch`); the connector then writes no sentence of its own.
    let hook_json = args.first().map(String::as_str) == Some("hook")
        && args.iter().skip(1).any(|a| a == "--json");
    let mut connect = |harness: &str| commands::hook_connect(harness, hook_json);
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let status = dispatch(
        &args,
        Streams {
            stdin: &mut stdin,
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
        &mut connect,
    );
    let _ = stdout.flush();
    std::process::exit(status);
}
