// SPDX-License-Identifier: Apache-2.0

//! What the daemon observes about the peer of a local connection from the operating system
//! (G9, #70; `spec/session-channels.md` §6.7.2, [SC-ID-121]; `docs/planning/decisions/
//! C4-session-identity.md` §3 "Pairing requirement"; lead decisions D2, D3, 2026-10-14).
//!
//! The core pairs a hook payload with an attachment by a **pairing key** the daemon observes
//! itself, never a value the peer sent, and makes a registration record only from a harness
//! label and a **working directory** it observed too (`oac_core::session_binding::
//! PeerObservation`). This module makes that observation from the peer's process id, which
//! the local IPC reports (the named pipe's client process, a Unix socket's peer credentials):
//!
//! 1. It looks the peer up in the operating system's process table, and walks its parents.
//! 2. The harness process is the **nearest ancestor whose executable is the harness's**
//!    (`claude` or `codex`, with or without `.exe`, in any case). A hook handler is usually a
//!    grandchild of Claude Code (a shell between them) and its MCP shim a child or
//!    grandchild; the nearest named ancestor is the same process for both. A Claude Code
//!    that is not a `claude` executable (one launched as `node cli.js`, say) matches nothing:
//!    its sessions are **unpairable and stay unbound** (D3; the cost is availability, never
//!    authority).
//! 3. The pairing key is that process's identity, its id **and its start time**, so a
//!    recycled process id is another process. A parent that started after its child is a
//!    recycled id and ends the walk.
//! 4. The working directory is that harness process's, read from the operating system.
//!
//! Anything that cannot be observed leaves its field `None`. With no key, a native signal
//! on the connection is unpairable and binds nothing ([SC-ID-125]); with no working
//! directory nothing can be bound either (a registration record needs one). Both fail
//! closed.
//!
//! **What this does not do.** A process that descends from the harness, a tool subprocess
//! under Claude Code say, has the same key as the harness's own shim. If it opens a second
//! attachment, the core withholds the real one and binds neither (the same-key denial of
//! service, [SC-ID-154]; accepted by the lead as a documented residual, D5, recorded under
//! `RISK-LOCAL-IPC`). The key is a process's identity, not its trustworthiness: peer
//! authentication (the local IPC's user check) decides who may connect at all.
//!
//! The process table is a trait, so the walk is tested against tables of known shape; the
//! real tables are the Windows, Linux and macOS ones in `os`.

use std::collections::HashMap;

use oac_core::ids::Token;
use oac_core::session_binding::{PairingKey, PeerObservation};

mod os;

pub use os::SystemTable;
#[cfg(windows)]
pub use os::{current_user_sid_string, process_is_current_user};

/// The deepest chain of parents the walk follows. Real chains are a handful long; a longer
/// one is a loop or not a harness's.
pub const MAX_ANCESTRY: usize = 64;

/// A process: its id and the time it started, so that a recycled id is another process.
///
/// The start time is an opaque, platform-specific number (Windows `FILETIME` ticks; Linux
/// clock ticks since boot; macOS microseconds since the epoch). It is compared only with
/// another from the same table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProcessId {
    /// The process id.
    pub pid: u32,
    /// When the process started.
    pub started: u64,
}

/// What the table knows of one process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessInfo {
    /// The process.
    pub id: ProcessId,
    /// Its parent's process id, when it has one.
    pub parent: Option<u32>,
    /// Its executable's file name (not the path), as the table gives it.
    pub image: String,
}

/// The operating system's process table, as far as the walk needs it.
pub trait ProcessTable {
    /// The process `pid`, or `None` when there is none or it cannot be read.
    fn lookup(&self, pid: u32) -> Option<ProcessInfo>;

    /// The working directory of `process`, as a string. `None` when it cannot be read, or
    /// when `process` is no longer the process that started at `process.started`.
    fn working_directory(&self, process: &ProcessId) -> Option<String>;

    /// Whether process `pid` belongs to the user this daemon runs as. A process of another
    /// user is never taken for a harness.
    fn same_user(&self, pid: u32) -> bool;
}

/// The executable names of the harness a label names, normalized by [`normalize_image`];
/// `None` for a label that is not a harness.
pub fn harness_images(label: &str) -> Option<&'static [&'static str]> {
    match label {
        "claude" => Some(&["claude"]),
        "codex" => Some(&["codex"]),
        _ => None,
    }
}

/// An executable name in the form images are compared in: the file name without any
/// directory, in lower case, without a trailing `.exe`.
pub fn normalize_image(image: &str) -> String {
    let name = image.rsplit(['/', '\\']).next().unwrap_or(image);
    let name = name.to_ascii_lowercase();
    name.strip_suffix(".exe").unwrap_or(&name).to_owned()
}

/// The harness process above (or at) `peer_pid`: the nearest ancestor whose executable is
/// one of `images`. `None` when there is none, the chain is broken, loops or is too long, a
/// parent is younger than its child (a recycled process id), or the harness process is
/// another user's.
pub fn harness_ancestor(
    table: &dyn ProcessTable,
    peer_pid: u32,
    images: &[&str],
) -> Option<ProcessId> {
    let mut current = table.lookup(peer_pid)?;
    for _ in 0..MAX_ANCESTRY {
        if images.contains(&normalize_image(&current.image).as_str()) {
            return table.same_user(current.id.pid).then_some(current.id);
        }
        let parent_pid = current.parent.filter(|p| *p != 0 && *p != current.id.pid)?;
        let parent = table.lookup(parent_pid)?;
        if parent.id.started > current.id.started {
            // The parent cannot be younger than its child: the id was recycled.
            return None;
        }
        current = parent;
    }
    None
}

/// The pairing key of the harness process `id` under `label`: opaque, compared as exact
/// octets, never sent anywhere.
pub fn pairing_key(label: &str, id: &ProcessId) -> PairingKey {
    let mut octets = Vec::with_capacity(16 + label.len() + 12);
    octets.extend_from_slice(b"oac-pk1\0");
    octets.extend_from_slice(label.as_bytes());
    octets.push(0);
    octets.extend_from_slice(&id.pid.to_le_bytes());
    octets.extend_from_slice(&id.started.to_le_bytes());
    PairingKey::from_observation(octets)
}

/// What the core is told about the peer at `peer_pid` of a connection accepted on the
/// endpoint of harness `label`: the harness process's key and working directory, and the
/// label. Whatever cannot be observed is `None`.
///
/// `peer_pid` is the process id the local IPC reported for the connection's other end, never
/// a value the peer sent.
pub fn observe(table: &dyn ProcessTable, label: &str, peer_pid: u32) -> PeerObservation {
    let harness_label = Token::parse(label);
    let Some(images) = harness_images(label) else {
        return PeerObservation {
            harness_label,
            ..PeerObservation::none()
        };
    };
    let Some(harness) = harness_ancestor(table, peer_pid, images) else {
        return PeerObservation {
            harness_label,
            ..PeerObservation::none()
        };
    };
    PeerObservation {
        pairing_key: Some(pairing_key(label, &harness)),
        harness_label,
        working_directory: table.working_directory(&harness),
    }
}

/// A process table given as data: for tests, and for a diagnostic that shows what a walk
/// would do. It holds no operating-system state.
#[derive(Clone, Debug, Default)]
pub struct StaticTable {
    processes: HashMap<u32, (ProcessInfo, Option<String>, bool)>,
}

impl StaticTable {
    /// An empty table.
    pub fn new() -> StaticTable {
        StaticTable::default()
    }

    /// Adds process `pid` with parent `parent`, executable `image`, start time `started`
    /// and working directory `cwd`, owned by the daemon's user.
    pub fn with(
        mut self,
        pid: u32,
        parent: Option<u32>,
        image: &str,
        started: u64,
        cwd: Option<&str>,
    ) -> StaticTable {
        self.processes.insert(
            pid,
            (
                ProcessInfo {
                    id: ProcessId { pid, started },
                    parent,
                    image: image.to_owned(),
                },
                cwd.map(str::to_owned),
                true,
            ),
        );
        self
    }

    /// Marks process `pid` as another user's.
    pub fn owned_by_another_user(mut self, pid: u32) -> StaticTable {
        if let Some(entry) = self.processes.get_mut(&pid) {
            entry.2 = false;
        }
        self
    }
}

impl ProcessTable for StaticTable {
    fn lookup(&self, pid: u32) -> Option<ProcessInfo> {
        self.processes.get(&pid).map(|(info, _, _)| info.clone())
    }

    fn working_directory(&self, process: &ProcessId) -> Option<String> {
        let (info, cwd, _) = self.processes.get(&process.pid)?;
        (info.id == *process).then(|| cwd.clone()).flatten()
    }

    fn same_user(&self, pid: u32) -> bool {
        self.processes.get(&pid).is_some_and(|(_, _, mine)| *mine)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CWD: &str = "/work/repo";

    /// The shape of a Claude Code session on Windows (observed 2026-10-14): the desktop app's
    /// `claude.exe`, the CLI's `claude.exe`, a shell for a tool or hook, then the command.
    fn session() -> StaticTable {
        StaticTable::new()
            .with(10, Some(1), "sihost.exe", 100, Some("C:\\Windows"))
            .with(20, Some(10), "claude.exe", 200, Some("C:\\app"))
            .with(30, Some(20), "claude.exe", 300, Some(CWD))
            // A hook handler through a shell, a shim directly under Claude Code, and a
            // shim through a launcher: all in the same session.
            .with(40, Some(30), "cmd.exe", 400, Some(CWD))
            .with(41, Some(40), "oac.exe", 410, Some(CWD))
            .with(50, Some(30), "oac.exe", 500, Some(CWD))
            .with(60, Some(30), "node.exe", 600, Some(CWD))
            .with(61, Some(60), "oac.exe", 610, Some(CWD))
    }

    fn key_of(table: &StaticTable, label: &str, pid: u32) -> Option<PairingKey> {
        observe(table, label, pid).pairing_key
    }

    #[test]
    fn a_hook_and_its_shims_share_the_nearest_claude_process_and_its_directory() {
        let t = session();
        let hook = observe(&t, "claude", 41);
        let shim = observe(&t, "claude", 50);
        let launched = observe(&t, "claude", 61);
        assert!(hook.pairing_key.is_some());
        assert_eq!(hook.pairing_key, shim.pairing_key);
        assert_eq!(hook.pairing_key, launched.pairing_key);
        assert_eq!(hook.working_directory.as_deref(), Some(CWD));
        assert_eq!(hook.harness_label, Token::parse("claude"));
        // The nearest wins: not the desktop app's outer claude.exe (pid 20).
        assert_eq!(
            harness_ancestor(&t, 41, &["claude"]),
            Some(ProcessId {
                pid: 30,
                started: 300
            })
        );
    }

    #[test]
    fn the_peer_may_itself_be_the_harness() {
        let t = session();
        assert_eq!(
            harness_ancestor(&t, 30, &["claude"]).map(|p| p.pid),
            Some(30)
        );
    }

    #[test]
    fn two_sessions_do_not_share_a_key() {
        let t = session()
            .with(130, Some(10), "claude.exe", 1300, Some("/work/other"))
            .with(150, Some(130), "oac.exe", 1500, Some("/work/other"));
        assert_ne!(key_of(&t, "claude", 50), key_of(&t, "claude", 150));
        assert_eq!(
            observe(&t, "claude", 150).working_directory.as_deref(),
            Some("/work/other")
        );
    }

    #[test]
    fn a_process_with_no_harness_above_it_has_no_key_and_no_directory() {
        let t = session().with(70, Some(10), "oac.exe", 700, Some("/tmp"));
        let o = observe(&t, "claude", 70);
        assert_eq!(o.pairing_key, None);
        assert_eq!(o.working_directory, None);
        // The label is still the endpoint's.
        assert_eq!(o.harness_label, Token::parse("claude"));
        assert_eq!(observe(&t, "claude", 999).pairing_key, None, "unknown pid");
    }

    #[test]
    fn a_harness_that_is_not_a_claude_executable_is_unpairable() {
        // Claude Code started as `node cli.js` is a node process (D3): no match, no key.
        let t = StaticTable::new()
            .with(1, None, "systemd", 1, None)
            .with(2, Some(1), "node", 2, Some(CWD))
            .with(3, Some(2), "oac", 3, Some(CWD));
        assert_eq!(key_of(&t, "claude", 3), None);
    }

    #[test]
    fn the_label_picks_the_harness_so_a_codex_ancestor_is_no_claude() {
        let t = StaticTable::new()
            .with(1, None, "init", 1, None)
            .with(2, Some(1), "codex", 2, Some(CWD))
            .with(3, Some(2), "oac", 3, Some(CWD));
        assert_eq!(key_of(&t, "claude", 3), None);
        assert!(key_of(&t, "codex", 3).is_some());
        // A label that is no harness has no executable to look for.
        assert_eq!(key_of(&t, "gemini", 3), None);
        assert_eq!(
            observe(&t, "gemini", 3).harness_label,
            Token::parse("gemini")
        );
    }

    #[test]
    fn image_names_match_in_any_case_with_or_without_exe_and_directory() {
        for image in [
            "claude",
            "Claude.EXE",
            "CLAUDE.exe",
            "C:\\bin\\claude.exe",
            "/usr/bin/claude",
        ] {
            assert_eq!(normalize_image(image), "claude", "{image}");
        }
        for image in ["claudette", "claude-code", "node", "claude.exe.bak", ""] {
            assert_ne!(normalize_image(image), "claude", "{image}");
        }
    }

    #[test]
    fn a_parent_younger_than_its_child_is_a_recycled_id_and_ends_the_walk() {
        // pid 20 now names a claude.exe that started after the shim: it is not its parent.
        let t = StaticTable::new()
            .with(10, None, "sihost.exe", 100, None)
            .with(20, Some(10), "claude.exe", 900, Some(CWD))
            .with(30, Some(20), "oac.exe", 300, Some(CWD));
        assert_eq!(key_of(&t, "claude", 30), None);
    }

    #[test]
    fn a_broken_looping_or_endless_chain_ends_the_walk() {
        // The parent is gone.
        let gone = StaticTable::new().with(3, Some(2), "oac", 3, None);
        assert_eq!(key_of(&gone, "claude", 3), None);
        // A loop.
        let looped =
            StaticTable::new()
                .with(1, Some(2), "a", 1, None)
                .with(2, Some(1), "b", 1, None);
        assert_eq!(key_of(&looped, "claude", 1), None);
        // Its own parent, and parent 0.
        let own = StaticTable::new()
            .with(1, Some(1), "a", 1, None)
            .with(2, Some(0), "b", 1, None);
        assert_eq!(key_of(&own, "claude", 1), None);
        assert_eq!(key_of(&own, "claude", 2), None);
        // Longer than the bound, with a harness at the top.
        let mut deep = StaticTable::new().with(1, None, "claude", 1, Some(CWD));
        for pid in 2..=(MAX_ANCESTRY as u32 + 5) {
            deep = deep.with(pid, Some(pid - 1), "sh", u64::from(pid), None);
        }
        assert_eq!(key_of(&deep, "claude", MAX_ANCESTRY as u32 + 5), None);
        assert!(key_of(&deep, "claude", MAX_ANCESTRY as u32 - 5).is_some());
    }

    #[test]
    fn another_users_harness_process_is_never_taken() {
        let t = session().owned_by_another_user(30);
        assert_eq!(key_of(&t, "claude", 50), None);
    }

    #[test]
    fn the_key_names_the_process_and_its_start_not_the_directory_or_the_peer() {
        let t = session();
        let a = key_of(&t, "claude", 41);
        let b = key_of(&t, "claude", 61);
        assert_eq!(a, b, "peers of one harness share the key");
        // The same pid with another start time is another process, so another key.
        let recycled = session().with(30, Some(20), "claude.exe", 301, Some(CWD));
        assert_ne!(key_of(&recycled, "claude", 50), a);
        // Another label under the same process is another key.
        let both = StaticTable::new()
            .with(1, None, "claude", 1, Some(CWD))
            .with(2, Some(1), "oac", 2, Some(CWD));
        let id = ProcessId { pid: 1, started: 1 };
        assert_ne!(pairing_key("claude", &id), pairing_key("codex", &id));
        assert_eq!(key_of(&both, "claude", 2), Some(pairing_key("claude", &id)));
        // Debug shows a length only (core's PairingKey).
        assert!(format!("{:?}", pairing_key("claude", &id)).contains("octets"));
    }

    #[test]
    fn a_directory_that_cannot_be_read_still_gives_the_key() {
        let t =
            StaticTable::new()
                .with(1, None, "claude", 1, None)
                .with(2, Some(1), "oac", 2, None);
        let o = observe(&t, "claude", 2);
        assert!(o.pairing_key.is_some());
        assert_eq!(o.working_directory, None);
    }

    #[test]
    fn the_directory_is_read_only_while_the_process_is_the_one_observed() {
        let t = session();
        let stale = ProcessId {
            pid: 30,
            started: 299,
        };
        assert_eq!(t.working_directory(&stale), None);
    }
}
