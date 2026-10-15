// SPDX-License-Identifier: Apache-2.0

//! The `oac` binary.
//!
//! `oac start`, `oac mcp-shim` and `oac hook` (G9, #70; `status`, `sessions` and `doctor` are
//! G10's). This crate builds the adapters and the transport and hands them to the daemon,
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
        _ => {}
    }
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
        &mut commands::hook_connector,
    );
    let _ = stdout.flush();
    std::process::exit(status);
}
