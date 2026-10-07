// SPDX-License-Identifier: Apache-2.0

//! Local attachments (06 rows 13, 19 and 24; `spec/security.md` [SEC-AUZ-030];
//! `spec/session-channels.md` §6.7.2): the daemon's local IPC endpoint, its OS peer check,
//! and how it binds a native session signal to an attachment. None of it exists yet: the
//! daemon and its IPC are G9 (#70). Each test holds the place, ignored, and fails if run.

/// 06 row 13 ([SEC-AUZ-030]): the IPC endpoint admits only a peer whose OS-asserted user
/// equals the daemon's (named-pipe security descriptors and `GetNamedPipeClientProcessId`
/// on Windows; `SO_PEERCRED` / `getpeereid()` on Unix).
#[test]
#[ignore = "GATED on #70 (G9, daemon, MCP shims and authenticated local IPC)"]
fn gated_row13_ipc_admits_only_the_same_user() {
    panic!("GATED on #70: connect as another user and assert the daemon refuses the peer");
}

/// 06 row 19: a session's lifetime follows its IPC connection, so a registration held open
/// after the harness session ended binds nothing.
#[test]
#[ignore = "GATED on #70 (G9, daemon, MCP shims and authenticated local IPC)"]
fn gated_row19_session_lifetime_follows_the_ipc_connection() {
    panic!("GATED on #70: drop the shim's connection and assert the session ends with it");
}

/// 06 row 24: a same-user process that sets `CLAUDE_CODE_SESSION_ID` to another session's id
/// binds nothing; an already-bound hook id is never displaced, and an unpairable payload
/// fails closed (C4 §3, revision 2026-10-02).
#[test]
#[ignore = "GATED on #70 (G9, daemon, MCP shims and authenticated local IPC)"]
fn gated_row24_spoofed_session_variable_binds_nothing() {
    panic!(
        "GATED on #70: start a shim with a spoofed session variable and assert it binds nothing"
    );
}
