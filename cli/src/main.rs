// SPDX-License-Identifier: Apache-2.0

//! The `oac` binary.
//!
//! Only `oac hook` exists so far (G9, #70). This crate builds the adapters and the transport
//! and hands them to the core; it never invokes an adapter or transport contract operation
//! itself (`docs/planning/v0.1/07-repository-and-dependencies.md` section 3).

use std::io::{self, Write};

use oac_cli::verbs::{Streams, dispatch};

/// The way to the daemon. The local IPC (`oac_cli::ipc`, #411) is merged, but this binary does
/// not connect through it yet: the connector arrives with the daemon (#413). Until then no
/// connection can be opened, so a hook input is dropped with a diagnostic and the session
/// stays unbound (fail closed).
fn connect(_harness: &str) -> io::Result<Box<dyn Write>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "this build has no local connection to the daemon yet",
    ))
}

fn main() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
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
