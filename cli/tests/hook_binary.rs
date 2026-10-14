// SPDX-License-Identifier: Apache-2.0

//! The real `oac hook` process (G9, #70): what Claude Code starts. It has no daemon to reach
//! in this build, so it must still leave standard output empty and exit 0 for every input
//! ([MCPB-CLD-007]), and say nothing from the input on standard error.

use std::io::Write;
use std::process::{Command, Stdio};

fn run(args: &[&str], stdin: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_oac"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the oac binary starts");
    {
        let mut stdin_pipe = child.stdin.take().unwrap();
        // The child may exit before reading all of it; a closed pipe is not a failure here.
        let _ = stdin_pipe.write_all(stdin);
    }
    child.wait_with_output().unwrap()
}

const START: &[u8] = b"{\n \"session_id\": \"SECRET-SESSION\",\n \"cwd\": \"C:\\secret\",\n \"hook_event_name\": \"SessionStart\",\n \"source\": \"startup\"\n}\n";

#[test]
fn oac_hook_exits_zero_with_empty_standard_output_for_every_input() {
    for input in [
        START,
        b"{\"hook_event_name\":\"SessionEnd\",\"session_id\":\"s\"}".as_slice(),
        b"not json".as_slice(),
        b"".as_slice(),
    ] {
        let out = run(&["hook"], input);
        assert_eq!(out.status.code(), Some(0));
        assert!(
            out.stdout.is_empty(),
            "standard output is added to Claude's context"
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!err.contains("SECRET") && !err.contains("secret"), "{err}");
    }
}

#[test]
fn oac_hook_says_why_when_it_cannot_reach_the_daemon() {
    let out = run(&["hook"], START);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.starts_with("oac hook:"), "{err}");
}

#[test]
fn an_unknown_verb_is_a_usage_error_with_empty_standard_output() {
    let out = run(&["nonsense"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}
