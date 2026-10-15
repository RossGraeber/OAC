// SPDX-License-Identifier: Apache-2.0

//! The real process tables (G9, #70, `cli/src/peer/`): Windows' Toolhelp, token and PEB reads
//! and Linux's `/proc`, against real processes.
//!
//! The chain test builds the shape of a Claude Code session out of real processes:
//!
//! ```text
//! this test  ->  claude[.exe]  (a copy of this test binary, in its own directory and with
//!                               its own working directory)
//!                  ->  a shell  ->  the peer (this test binary again)   (twice)
//! ```
//!
//! and has each peer observe itself. Both must name the copy as the harness (the nearest
//! ancestor called `claude`, past the shell), with one key, and the copy's working
//! directory, which is not the peer's own. The roles are chosen by the `OAC_PEER_ROLE`
//! environment variable, because a test binary has no other way to be several programs.

#![cfg(any(windows, target_os = "linux"))]

use std::path::{Path, PathBuf};
use std::process::Command;

use oac_cli::peer::{
    ProcessId, ProcessTable, SystemTable, harness_ancestor, normalize_image, observe, pairing_key,
};

const ROLE: &str = "OAC_PEER_ROLE";
const CHAIN: &str = "the_real_chain_names_the_nearest_claude_process_for_every_peer";

fn exe() -> PathBuf {
    std::env::current_exe().unwrap()
}

/// A path in a form two spellings of one directory share: canonical, without Windows'
/// extended-path prefix, in lower case on Windows.
fn plain(path: &str) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));
    let text = canonical.to_string_lossy().replace("\\\\?\\", "");
    let text = text.trim_end_matches(['\\', '/']).to_owned();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

#[test]
fn this_process_is_in_the_table_with_its_image_user_and_directory() {
    let table = SystemTable::new().unwrap();
    let me = std::process::id();
    let info = table.lookup(me).expect("this process");
    assert_eq!(info.id.pid, me);
    let expected = normalize_image(exe().file_name().unwrap().to_str().unwrap());
    // The name started under: the whole image name on Windows; the kernel's `comm`, cut to
    // 15 octets, on Linux. The resolved name (Linux) is the full file name.
    let started_as = normalize_image(&info.image);
    assert!(
        expected.starts_with(&started_as) && (cfg!(unix) || started_as == expected),
        "{:?} for {expected}",
        info.image
    );
    if cfg!(target_os = "linux") {
        assert_eq!(
            info.resolved_image.as_deref().map(normalize_image),
            Some(expected)
        );
    }
    assert!(info.id.started > 0);
    assert!(table.same_user(&info.id));
    // A recycled id is not the process that had it.
    assert!(!table.same_user(&ProcessId {
        pid: me,
        started: info.id.started + 1,
    }));
    let cwd = table
        .working_directory(&info.id)
        .expect("own working directory");
    assert_eq!(
        plain(&cwd),
        plain(std::env::current_dir().unwrap().to_str().unwrap())
    );
    // A start time that is not this process's reads nothing.
    let other = ProcessId {
        pid: me,
        started: info.id.started + 1,
    };
    assert_eq!(table.working_directory(&other), None);
}

#[test]
fn a_parent_started_no_later_than_its_child() {
    let table = SystemTable::new().unwrap();
    let me = table.lookup(std::process::id()).unwrap();
    // The parent may have exited on Linux (the child is then reparented); then there is
    // nothing to compare.
    if let Some(parent) = me.parent.and_then(|p| table.lookup(p)) {
        assert!(parent.id.started <= me.id.started, "{parent:?} {me:?}");
    }
}

#[test]
fn a_process_that_does_not_exist_is_not_found_and_is_nobodys_user() {
    let table = SystemTable::new().unwrap();
    let none = u32::MAX - 1;
    assert_eq!(table.lookup(none), None);
    assert!(!table.same_user(&ProcessId {
        pid: none,
        started: 1,
    }));
    assert_eq!(
        table.working_directory(&ProcessId {
            pid: none,
            started: 1
        }),
        None
    );
}

#[test]
fn the_real_chain_names_the_nearest_claude_process_for_every_peer() {
    match std::env::var(ROLE).as_deref() {
        Ok("harness") => return harness(),
        Ok("peer") => return peer(),
        _ => {}
    }
    // A copy of the test binary that is itself named `claude`.
    chain(false);
    // The native install's shape: `claude` is a symlink to a differently named file
    // (`claude -> .../versions/2.1.287`). The process is `claude`; its resolved executable
    // is not. Windows has no such launcher.
    if cfg!(unix) {
        chain(true);
    }
}

/// One run of the chain; `symlink` makes the executable `claude` a link to the copy.
fn chain(symlink: bool) {
    let dir = std::env::temp_dir().join(format!(
        "oac-peer-real-{}-{}",
        std::process::id(),
        if symlink { "link" } else { "copy" }
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let bin = dir.join("bin");
    let work = dir.join("work");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    let claude = bin.join(if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    });
    if symlink {
        #[cfg(unix)]
        {
            let version = bin.join("2.1.287");
            std::fs::copy(exe(), &version).unwrap();
            std::os::unix::fs::symlink(&version, &claude).unwrap();
        }
    } else {
        std::fs::copy(exe(), &claude).unwrap();
    }
    let child = Command::new(&claude)
        .args(["--exact", CHAIN, "--nocapture", "--test-threads=1"])
        .env(ROLE, "harness")
        .env("OAC_PEER_ORIGINAL", exe())
        .env("OAC_PEER_WORK", &work)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the copy starts");
    let harness_pid = child.id();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "{stdout}\n{stderr}");

    let results: Vec<Vec<&str>> = stdout
        .lines()
        // The test harness may print its own `test ... ` prefix on the same line.
        .filter_map(|l| l.split_once("PEER-RESULT ").map(|(_, rest)| rest))
        .map(|l| l.splitn(3, ' ').collect())
        .collect();
    assert_eq!(results.len(), 2, "two peers reported: {stdout}\n{stderr}");
    for r in &results {
        assert_eq!(
            r[0],
            harness_pid.to_string(),
            "the harness is the copy named claude"
        );
        assert_ne!(r[1], "-", "a key was observed");
    }
    assert_eq!(results[0][1], results[1][1], "both peers share one key");
    // The directory is the harness's (`work`), not the peers' own (the test's).
    assert_eq!(plain(results[0][2]), plain(work.to_str().unwrap()));
    assert_eq!(plain(results[1][2]), plain(work.to_str().unwrap()));
}

/// The copy named `claude`: moves to its working directory and runs the peer twice, each
/// through a shell.
fn harness() {
    let original = std::env::var("OAC_PEER_ORIGINAL").unwrap();
    let work = std::env::var("OAC_PEER_WORK").unwrap();
    std::env::set_current_dir(&work).unwrap();
    for _ in 0..2 {
        let out = through_a_shell(Path::new(&original)).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        print!("{}", String::from_utf8_lossy(&out.stdout));
    }
}

#[cfg(windows)]
fn through_a_shell(original: &Path) -> Command {
    let mut c = Command::new("cmd");
    c.arg("/c")
        .arg(original)
        .args(["--exact", CHAIN, "--nocapture", "--test-threads=1"]);
    c.env(ROLE, "peer");
    c
}

#[cfg(not(windows))]
fn through_a_shell(original: &Path) -> Command {
    let mut c = Command::new("sh");
    // Not the last word of a `-c` string, so the shell stays between this process and the
    // peer.
    c.args(["-c", "\"$0\" \"$@\"; exit $?"])
        .arg(original)
        .args(["--exact", CHAIN, "--nocapture", "--test-threads=1"]);
    c.env(ROLE, "peer");
    c
}

/// The peer: observes itself as the daemon would observe a connection from it.
fn peer() {
    let table = SystemTable::new().unwrap();
    let me = std::process::id();
    let harness = harness_ancestor(&table, me, &["claude"]);
    let observation = observe(&table, "claude", me);
    // `PairingKey` shows only its length, so the key's identity is the harness process's: the
    // same key must be the key of that process under the label.
    let key = match (&observation.pairing_key, harness) {
        (Some(k), Some(h)) => {
            assert_eq!(*k, pairing_key("claude", &h));
            format!("{}:{}", h.pid, h.started)
        }
        _ => "-".to_owned(),
    };
    println!(
        "PEER-RESULT {} {} {}",
        harness.map_or("-".to_owned(), |h| h.pid.to_string()),
        key,
        observation.working_directory.unwrap_or_else(|| "-".into())
    );
}
