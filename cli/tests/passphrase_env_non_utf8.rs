// SPDX-License-Identifier: Apache-2.0

//! A passphrase variable that is not UTF-8 is read as no passphrase, and is still removed from
//! the environment (#70 review): the bytes are held only in zeroizing storage and the
//! variable is wiped like any other value, so a bad value does not linger.
//!
//! This file holds one test, so it runs as its own process with no other test thread:
//! setting and removing an environment variable is sound only while no other thread uses the
//! environment.

#![cfg(unix)]

use oac_cli::keystore::{FileKeyStore, PASSPHRASE_ENV};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;

#[test]
fn a_non_utf8_passphrase_is_no_passphrase_and_is_removed() {
    // SAFETY: this test binary runs this single test; no other thread reads or writes the
    // environment while it runs.
    unsafe {
        std::env::remove_var("CREDENTIALS_DIRECTORY");
        std::env::set_var(
            PASSPHRASE_ENV,
            OsString::from_vec(vec![b'p', 0xff, 0xfe, b'q']),
        );
    }
    // SAFETY: as above.
    assert!(unsafe { FileKeyStore::passphrase_from_environment() }.is_none());
    assert!(std::env::var_os(PASSPHRASE_ENV).is_none());
    // Taken once: the value is not read again.
    // SAFETY: as above.
    assert!(unsafe { FileKeyStore::passphrase_from_environment() }.is_none());
}
