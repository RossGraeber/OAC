// SPDX-License-Identifier: Apache-2.0

//! The Linux process table, from `/proc`.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use super::super::{ProcessId, ProcessInfo, ProcessTable};

/// The Linux process table. It reads `/proc` on every call, so it holds no state.
#[derive(Debug, Default)]
pub struct SystemTable;

impl SystemTable {
    /// The table.
    pub fn new() -> std::io::Result<SystemTable> {
        Ok(SystemTable)
    }
}

/// The fields of `/proc/<pid>/stat` the walk needs.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Stat {
    pub comm: String,
    pub parent: u32,
    pub started: u64,
}

/// Parses the text of `/proc/<pid>/stat` (`proc_pid_stat(5)`): `pid (comm) state ppid ...
/// starttime ...`. `comm` may hold spaces and parentheses, so the fields are counted from
/// the **last** `)`.
pub(crate) fn parse_stat(text: &str) -> Option<Stat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close < open {
        return None;
    }
    let comm = text[open + 1..close].to_owned();
    let rest: Vec<&str> = text[close + 1..].split_whitespace().collect();
    // After `)`: state (3), ppid (4), ..., starttime (22) at index 19.
    let parent = rest.get(1)?.parse().ok()?;
    let started = rest.get(19)?.parse().ok()?;
    Some(Stat {
        comm,
        parent,
        started,
    })
}

fn stat(pid: u32) -> Option<Stat> {
    parse_stat(&fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

impl ProcessTable for SystemTable {
    fn lookup(&self, pid: u32) -> Option<ProcessInfo> {
        let s = stat(pid)?;
        // The executable's file name; the kernel's 15-character `comm` when the link cannot
        // be read.
        let image = fs::read_link(format!("/proc/{pid}/exe"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or(s.comm);
        Some(ProcessInfo {
            id: ProcessId {
                pid,
                started: s.started,
            },
            parent: Some(s.parent),
            image,
        })
    }

    fn working_directory(&self, process: &ProcessId) -> Option<String> {
        let pid = process.pid;
        // The process must be the one observed before and after the read.
        let same = || stat(pid).is_some_and(|s| s.started == process.started);
        if !same() {
            return None;
        }
        let cwd: PathBuf = fs::read_link(format!("/proc/{pid}/cwd")).ok()?;
        if !same() {
            return None;
        }
        cwd.to_str().map(str::to_owned)
    }

    fn same_user(&self, pid: u32) -> bool {
        // SAFETY: geteuid has no preconditions and cannot fail.
        let me = unsafe { libc::geteuid() };
        fs::metadata(format!("/proc/{pid}")).is_ok_and(|m| m.uid() == me)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_is_parsed_from_the_last_paren() {
        // A real stat line, with a comm that holds spaces and parentheses.
        let line = "4242 (my (odd) cmd) S 4000 4242 4242 34816 4242 4194304 100 0 0 0 1 2 0 0 20 0 1 0 987654 12345678 900 18446744073709551615 1 1 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0\n";
        let s = parse_stat(line).unwrap();
        assert_eq!(s.comm, "my (odd) cmd");
        assert_eq!(s.parent, 4000);
        assert_eq!(s.started, 987_654);
    }

    #[test]
    fn short_or_garbled_stat_is_no_process() {
        for bad in ["", "1 (a)", "1 a S 2", "1 (a) S x 3", ") (", "1 (a) S 2 3"] {
            assert_eq!(parse_stat(bad), None, "{bad:?}");
        }
    }
}
