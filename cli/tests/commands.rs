// SPDX-License-Identifier: Apache-2.0

//! The `oac start` and `oac mcp-shim` command lines, on the real binary (G9, #70). What they
//! do with a daemon is `tests/daemon.rs`. `oac start` itself is not run here: it would make
//! the user's real device key in the operating system's credential store, which no
//! CI-default test may do; its live run is the operator leg.

use std::process::{Command, Stdio};

/// A private runtime directory of this call, so that no test makes anything in the user's
/// own (Unix: `XDG_RUNTIME_DIR`), removed when the call is done.
#[cfg(unix)]
struct RuntimeDir(std::path::PathBuf);

#[cfg(unix)]
impl RuntimeDir {
    fn new() -> RuntimeDir {
        use std::os::unix::fs::PermissionsExt;
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("oac-cmd-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        RuntimeDir(dir)
    }
}

#[cfg(unix)]
impl Drop for RuntimeDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn oac(args: &[&str], scope: Option<&str>) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_oac"));
    command
        .args(args)
        .stdin(Stdio::null())
        // Never the user's own endpoints, whatever these tests do.
        .env("OAC_SCOPE", scope.unwrap_or("commandstest"));
    #[cfg(unix)]
    let runtime = RuntimeDir::new();
    #[cfg(unix)]
    command.env("XDG_RUNTIME_DIR", &runtime.0);
    command.output().unwrap()
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn start_takes_nothing_but_detach() {
    for args in [&["start", "--bogus"][..], &["start", "--detach", "x"][..]] {
        let out = oac(args, None);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty());
        assert!(
            stderr(&out).contains("usage: oac start [--detach]"),
            "{}",
            stderr(&out)
        );
    }
}

#[test]
fn the_shim_needs_its_harness_and_names_it_as_a_label() {
    for args in [
        &["mcp-shim"][..],
        &["mcp-shim", "--harness"][..],
        &["mcp-shim", "--harness", "Claude"][..],
        &["mcp-shim", "--harness", "lock"][..],
        // The daemon's own control endpoint is not a harness (G10, #71).
        &["mcp-shim", "--harness", "control"][..],
        &["mcp-shim", "--harness", "a/b"][..],
        &["mcp-shim", "claude"][..],
    ] {
        let out = oac(args, None);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(
            stderr(&out).contains("usage: oac mcp-shim --harness"),
            "{args:?}"
        );
    }
}

#[test]
fn stop_takes_nothing_but_json() {
    for args in [
        &["stop", "--bogus"][..],
        &["stop", "--json", "x"][..],
        &["stop", "x"][..],
    ] {
        let out = oac(args, None);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty());
        assert!(
            stderr(&out).contains("usage: oac stop [--json]"),
            "{}",
            stderr(&out)
        );
    }
}

#[test]
fn stop_with_no_daemon_exits_zero_and_says_not_running() {
    let out = oac(&["stop"], Some("stopnodaemon"));
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let said = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(said.contains("not running"), "{said}");
    // The namespace in use is a diagnostic, on standard error.
    assert!(stderr(&out).contains("stopnodaemon"), "{}", stderr(&out));

    let out = oac(&["stop", "--json"], Some("stopnodaemon"));
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim_end(),
        r#"{"oac_cli":1,"ok":true,"stopped":false,"was_running":false}"#
    );
}

#[test]
fn a_shim_with_no_daemon_says_how_to_start_one_and_writes_nothing_to_the_harness() {
    let out = oac(&["mcp-shim", "--harness", "claude"], Some("nodaemonhere"));
    assert_eq!(out.status.code(), Some(1));
    assert!(
        out.stdout.is_empty(),
        "standard output is the harness's, not ours"
    );
    let e = stderr(&out);
    assert!(e.contains("the daemon is not running"), "{e}");
    assert!(e.contains("`oac start`"), "{e}");
    // The test namespace in use is said.
    assert!(e.contains("nodaemonhere"), "{e}");
}

#[test]
fn a_scope_that_is_not_a_scope_name_is_refused_not_used() {
    for bad in ["UPPER", "has space", "a/b", "..", &"x".repeat(25)] {
        let out = oac(&["mcp-shim", "--harness", "claude"], Some(bad));
        assert_eq!(out.status.code(), Some(1), "{bad:?}");
        assert!(out.stdout.is_empty());
        assert!(
            stderr(&out).contains("scope name"),
            "{bad:?}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn the_hook_with_no_daemon_exits_zero_with_nothing_on_stdout_and_says_how_to_start_one() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_oac"));
    command.arg("hook").env("OAC_SCOPE", "nodaemonhere");
    #[cfg(unix)]
    let runtime = RuntimeDir::new();
    #[cfg(unix)]
    command.env("XDG_RUNTIME_DIR", &runtime.0);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"session_id\":\"s1\",\"hook_event_name\":\"SessionStart\",\"source\":\"startup\"}\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(stderr(&out).contains("`oac start`"), "{}", stderr(&out));
}

#[test]
fn start_refuses_a_test_namespace_before_it_reads_any_key() {
    // The device key is the user's one in every namespace, so a daemon started in a namespace
    // beside the real one would be a second peer with the same identity. The binary refuses
    // before it builds a key store; the refusal is the whole of what it does (no endpoint is
    // made, nothing is written to standard output).
    for args in [&["start"][..], &["start", "--detach"][..]] {
        let out = oac(args, Some("refusedns"));
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        let e = stderr(&out);
        assert!(e.contains("OAC_SCOPE is set"), "{args:?}: {e}");
        assert!(!e.contains("is running"), "{args:?}: {e}");
    }
}

#[test]
fn start_with_json_still_refuses_a_test_namespace_and_says_so_as_the_error_object() {
    // `--json` changes what standard output holds, never the refusal or the exit status.
    for args in [
        &["start", "--json"][..],
        &["start", "--detach", "--json"][..],
        &["start", "--json", "--detach"][..],
    ] {
        let out = oac(args, Some("refusedns"));
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        let said = String::from_utf8_lossy(&out.stdout).into_owned();
        assert_eq!(said.lines().count(), 1, "{args:?}: {said:?}");
        assert!(
            said.starts_with("{\"detail\":\"OAC_SCOPE is set;"),
            "{args:?}: {said}"
        );
        assert!(said.contains("\"error\":\"failed\""), "{said}");
        assert!(said.contains("\"oac_cli\":1,\"ok\":false"), "{said}");
        // The words stay on standard error.
        assert!(stderr(&out).contains("OAC_SCOPE is set"), "{args:?}");
    }
}

#[test]
fn start_usage_names_json_and_help_prints_to_standard_output() {
    let out = oac(&["start", "--json", "--json"], None);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(
        stderr(&out).contains("usage: oac start [--detach] [--json]"),
        "{}",
        stderr(&out)
    );
    let out = oac(&["start", "--help"], None);
    assert_eq!(out.status.code(), Some(0));
    let help = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        help.contains("usage: oac start [--detach] [--json]"),
        "{help}"
    );
    assert!(help.contains("--json"), "{help}");
}
