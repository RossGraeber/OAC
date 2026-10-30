// SPDX-License-Identifier: Apache-2.0

//! The one place the `cli/` tests make a harness-named copy of the test binary and start it.
//!
//! Several tests play a harness (`claude`, `codex`) by running a copy of their own test binary
//! under that name. Copying the file and exec'ing the copy races on Linux: `std::fs::copy`
//! holds a write descriptor on the new file until it returns, tests run in parallel threads,
//! and a sibling thread that forks in that window hands the descriptor to its child until the
//! child's own exec. Exec'ing a file that any process holds open for writing fails with
//! `ETXTBSY` ("Text file busy", `ErrorKind::ExecutableFileBusy`). It showed up as
//! `the codex host starts: Os { code: 26, ... }` in `codex_daemon.rs`.
//!
//! Two defences, both here so that no test repeats them:
//!
//! 1. [`install_exe_copy`] hard-links the test binary, which opens nothing for writing, and
//!    only copies when linking is impossible (another filesystem, or one without hard links).
//! 2. [`SpawnRetrying::spawn_retrying`] retries a spawn that fails with `ExecutableFileBusy`,
//!    a bounded number of times with a short backoff, which covers the copy fallback.
//!
//! [`Existing`] says what happens when the destination is already there: `Reuse` keeps it
//! (`codex_daemon.rs`, `daemon.rs`: one directory per scene, several starts under one name),
//! `Replace` removes it first and installs afresh (`sessions.rs`, `status.rs`: a directory
//! named after a pid can be left behind by a panicked run, and its binary may be from an old
//! build). A `Replace` whose removal fails is an error, never a silent reuse.
//!
//! Hard links work on NTFS and the extension of `dest` is the caller's (`.exe` on Windows).
//!
//! This file is mounted with `#[path = "support/exec_copy.rs"] mod exec_copy;` by each
//! integration test that needs it (the pattern of `transports/zenoh/tests/support/ports.rs`).
//! Do not copy its logic into a test; mount it.
#![allow(dead_code)] // each test mounts the file and uses one of the two `Existing` modes

use std::io;
use std::path::Path;
use std::process::{Child, Command};
use std::time::Duration;

/// How many times a spawn that finds the file busy is retried, and the wait before each retry
/// (doubled every time, so the total is a few seconds at most).
const SPAWN_ATTEMPTS: u32 = 8;
const SPAWN_FIRST_BACKOFF: Duration = Duration::from_millis(10);

/// What [`install_exe_copy`] does when `dest` already exists.
#[derive(Clone, Copy)]
pub enum Existing {
    /// Keep it and install nothing. For a scene that starts several processes under one name in
    /// a directory of its own (a second start finds the first one's binary, possibly running).
    Reuse,
    /// Remove it first (a missing file is fine) and install afresh. For a directory whose name
    /// can be left over from an earlier run (a pid reused after a panic): the binary there may
    /// come from an old build and must not be run. A removal that fails, such as a still
    /// running executable on Windows, is an error and never falls back to reusing the file.
    Replace,
}

/// Makes `dest` a copy of this test binary, as `existing` says when `dest` is already there.
/// Prefers a hard link (no write descriptor, see the module documentation) and falls back to a
/// copy.
pub fn install_exe_copy(dest: &Path, existing: Existing) {
    match existing {
        Existing::Reuse if dest.exists() => return,
        Existing::Reuse => {}
        Existing::Replace => match std::fs::remove_file(dest) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => panic!("cannot replace {}: {e}", dest.display()),
        },
    }
    let exe = std::env::current_exe().unwrap();
    if std::fs::hard_link(&exe, dest).is_err() {
        std::fs::copy(&exe, dest).unwrap();
    }
}

/// `Command::spawn` that retries while the executable is busy (`ETXTBSY`).
pub trait SpawnRetrying {
    fn spawn_retrying(&mut self) -> io::Result<Child>;
}

impl SpawnRetrying for Command {
    fn spawn_retrying(&mut self) -> io::Result<Child> {
        let mut backoff = SPAWN_FIRST_BACKOFF;
        for _ in 1..SPAWN_ATTEMPTS {
            match self.spawn() {
                Err(e) if e.kind() == io::ErrorKind::ExecutableFileBusy => {
                    std::thread::sleep(backoff);
                    backoff *= 2;
                }
                other => return other,
            }
        }
        self.spawn()
    }
}
