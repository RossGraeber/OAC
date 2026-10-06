// SPDX-License-Identifier: Apache-2.0

//! The encrypted-file fallback's passphrase environment variable is read once and removed
//! from the process environment, so no child process inherits it (#315 review N-a).
//!
//! This file holds one test, so it runs as its own process with no other test thread:
//! setting and removing an environment variable is sound only while no other thread uses
//! the environment.

#![cfg(unix)]

use age::secrecy::ExposeSecret;
use oac_cli::keystore::{FileKeyStore, PASSPHRASE_ENV};
use std::process::Command;

#[test]
fn passphrase_variable_is_taken_once_and_not_inherited() {
    // SAFETY: this test binary runs this single test; no other thread reads or writes the
    // environment while it runs.
    unsafe {
        std::env::remove_var("CREDENTIALS_DIRECTORY");
        std::env::set_var(PASSPHRASE_ENV, "throwaway test passphrase");
    }
    // SAFETY: as above.
    let first = unsafe { FileKeyStore::passphrase_from_environment() }.unwrap();
    assert_eq!(first.expose_secret(), "throwaway test passphrase");
    assert!(std::env::var_os(PASSPHRASE_ENV).is_none());

    // A child process does not see it.
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!("printf %s \"${{{PASSPHRASE_ENV}-unset}}\""))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "unset");

    // Later calls return the value taken on the first.
    // SAFETY: as above.
    let again = unsafe { FileKeyStore::passphrase_from_environment() }.unwrap();
    assert_eq!(again.expose_secret(), "throwaway test passphrase");
}
