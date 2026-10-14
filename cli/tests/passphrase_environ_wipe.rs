// SPDX-License-Identifier: Apache-2.0

//! Reading the passphrase variable overwrites it where the process keeps it (#315 review,
//! #70), so `/proc/<pid>/environ` no longer shows it to another process of the same user.
//! `remove_var` alone left the value in the memory the process was started with.
//!
//! The test runs the test binary again as a child that was started with the variable set (so
//! its value sits in the initial environment block, as a deployment's does), lets the child
//! take the passphrase, and has the child read its own `/proc/self/environ` afterwards.
//! This file holds one test, so the child has no other test thread: setting and removing an
//! environment variable is sound only while no other thread uses the environment.

#![cfg(target_os = "linux")]

use age::secrecy::ExposeSecret;
use oac_cli::keystore::{FileKeyStore, PASSPHRASE_ENV};
use std::process::Command;

const CHILD: &str = "OAC_TEST_ENVIRON_WIPE_CHILD";
const SECRET: &str = "throwaway-passphrase-8f3a9c21-do-not-use";

#[test]
fn the_passphrase_is_gone_from_proc_environ_after_it_is_taken() {
    if std::env::var_os(CHILD).is_some() {
        child();
        return;
    }
    let exe = std::env::current_exe().unwrap();
    let out = Command::new(exe)
        .args([
            "--exact",
            "the_passphrase_is_gone_from_proc_environ_after_it_is_taken",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .env(PASSPHRASE_ENV, SECRET)
        .env_remove("CREDENTIALS_DIRECTORY")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("ENVIRON-WIPED-OK"),
        "{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn child() {
    let environ = |what: &str| std::fs::read("/proc/self/environ").unwrap_or_else(|e| panic!("{what}: {e}"));
    let holds = |bytes: &[u8]| bytes.windows(SECRET.len()).any(|w| w == SECRET.as_bytes());
    // Control: the value is in the initial environment block before it is taken.
    assert!(holds(&environ("before")), "the control must see the secret");

    // SAFETY: this child runs this single test; no other thread reads or writes the
    // environment while it runs.
    let taken = unsafe { FileKeyStore::passphrase_from_environment() }.unwrap();
    assert_eq!(taken.expose_secret(), SECRET);

    let after = environ("after");
    assert!(!holds(&after), "the secret is still in /proc/self/environ");
    // The variable is gone from the environment too.
    assert!(std::env::var_os(PASSPHRASE_ENV).is_none());
    assert!(
        after
            .split(|b| *b == 0)
            .all(|entry| !entry.starts_with(format!("{PASSPHRASE_ENV}=").as_bytes())
                || entry.len() == PASSPHRASE_ENV.len() + 1),
        "no non-empty value of the variable remains"
    );
    println!("ENVIRON-WIPED-OK");
}
