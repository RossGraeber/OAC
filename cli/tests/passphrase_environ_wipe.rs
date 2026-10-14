// SPDX-License-Identifier: Apache-2.0

//! Reading the passphrase variable overwrites it where the process keeps it (#315 review,
//! #70), so `/proc/<pid>/environ` no longer shows it to another process of the same user.
//! `remove_var` alone left the value in the memory the process was started with.
//!
//! The test runs the test binary again as a child that was started with the variable set (so
//! its value sits in the initial environment block, as a deployment's does), lets the child
//! take the passphrase, and has the child read its own `/proc/self/environ` afterwards. A
//! second child is started with the variable set TWICE (an `envp` may repeat a name; the
//! shadowed entry is not the one `getenv` returns) and must lose both values.
//! This file holds one test, so each child has no other test thread: setting and removing an
//! environment variable is sound only while no other thread uses the environment.

#![cfg(target_os = "linux")]

use age::secrecy::ExposeSecret;
use oac_cli::keystore::{FileKeyStore, PASSPHRASE_ENV};
use std::ffi::CString;
use std::os::unix::process::CommandExt;
use std::process::Command;

const CHILD: &str = "OAC_TEST_ENVIRON_WIPE_CHILD";
const SECRET: &str = "throwaway-passphrase-8f3a9c21-do-not-use";
const SHADOWED: &str = "second-throwaway-passphrase-51d07b6e-do-not-use";
const TEST: &str = "the_passphrase_is_gone_from_proc_environ_after_it_is_taken";

#[test]
fn the_passphrase_is_gone_from_proc_environ_after_it_is_taken() {
    if let Some(mode) = std::env::var_os(CHILD) {
        child(mode == "duplicate");
        return;
    }
    let exe = std::env::current_exe().unwrap();

    // One entry.
    let single = Command::new(&exe)
        .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD, "single")
        .env(PASSPHRASE_ENV, SECRET)
        .env_remove("CREDENTIALS_DIRECTORY")
        .output()
        .unwrap();
    report("single", &single);

    // The same name twice in the child's `envp`: `Command` cannot build that, so the forked
    // child calls `execve` itself with an `envp` it chose.
    let c = |s: &str| CString::new(s).unwrap();
    let path = c(exe.to_str().unwrap());
    let argv = [
        path.clone(),
        c("--exact"),
        c(TEST),
        c("--nocapture"),
        c("--test-threads=1"),
    ];
    let envp = [
        c(&format!("{CHILD}=duplicate")),
        c(&format!("{PASSPHRASE_ENV}={SECRET}")),
        c(&format!("{PASSPHRASE_ENV}={SHADOWED}")),
    ];
    let mut command = Command::new(&exe);
    // SAFETY: the closure runs in the forked child before exec and calls only `execve` on
    // arrays built before the fork.
    unsafe {
        command.pre_exec(move || {
            let mut argp: Vec<*const libc::c_char> = argv.iter().map(|a| a.as_ptr()).collect();
            argp.push(std::ptr::null());
            let mut envs: Vec<*const libc::c_char> = envp.iter().map(|a| a.as_ptr()).collect();
            envs.push(std::ptr::null());
            libc::execve(path.as_ptr(), argp.as_ptr(), envs.as_ptr());
            Err(std::io::Error::last_os_error())
        });
    }
    let duplicate = command.output().unwrap();
    report("duplicate", &duplicate);
}

fn report(what: &str, out: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("ENVIRON-WIPED-OK"),
        "{what}: {stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn child(duplicate: bool) {
    let environ =
        |what: &str| std::fs::read("/proc/self/environ").unwrap_or_else(|e| panic!("{what}: {e}"));
    let holds =
        |bytes: &[u8], secret: &str| bytes.windows(secret.len()).any(|w| w == secret.as_bytes());
    // Control: the values are in the initial environment block before they are taken.
    let before = environ("before");
    assert!(holds(&before, SECRET), "the control must see the secret");
    if duplicate {
        assert!(
            holds(&before, SHADOWED),
            "the control must see the shadowed entry"
        );
    }

    // SAFETY: this child runs this single test; no other thread reads or writes the
    // environment while it runs.
    let taken = unsafe { FileKeyStore::passphrase_from_environment() }.unwrap();
    assert_eq!(
        taken.expose_secret(),
        SECRET,
        "the first entry is the one read"
    );

    let after = environ("after");
    assert!(
        !holds(&after, SECRET),
        "the secret is still in /proc/self/environ"
    );
    assert!(
        !holds(&after, SHADOWED),
        "a shadowed entry is still in /proc/self/environ"
    );
    // The variable is gone from the environment too.
    assert!(std::env::var_os(PASSPHRASE_ENV).is_none());
    assert!(
        after.split(|b| *b == 0).all(|entry| {
            !entry.starts_with(format!("{PASSPHRASE_ENV}=").as_bytes())
                || entry.len() == PASSPHRASE_ENV.len() + 1
        }),
        "no non-empty value of the variable remains"
    );
    println!("ENVIRON-WIPED-OK");
}
