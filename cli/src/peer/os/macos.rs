// SPDX-License-Identifier: Apache-2.0

//! The macOS process table, from `proc_pidinfo` and `proc_pidpath` (`libproc`).
//!
//! **Not built or run in this repository's checks**: there is no Mac here, so this file is
//! written against the symbols as `libc` 0.2.190 declares them (`proc_pidinfo`, `proc_pidpath`,
//! `proc_bsdinfo`, `proc_vnodepathinfo`, `PROC_PIDTBSDINFO`, `PROC_PIDVNODEPATHINFO`), and the
//! gap is a recorded residual (lead decision D9, 2026-10-14). The first Mac run is its test.

use std::ffi::c_void;
use std::io;

use super::super::{ProcessId, ProcessInfo, ProcessTable};

/// The macOS process table. It reads the kernel on every call, so it holds no state.
#[derive(Debug, Default)]
pub struct SystemTable;

impl SystemTable {
    /// The table.
    pub fn new() -> io::Result<SystemTable> {
        Ok(SystemTable)
    }
}

/// The BSD info of `pid`: parent, start time (microseconds since the epoch) and owner.
fn bsd_info(pid: u32) -> Option<libc::proc_bsdinfo> {
    // SAFETY: proc_bsdinfo is plain data; zero is a valid starting value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: `info` is a writable local of `size` bytes.
    let got = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast::<c_void>(),
            size,
        )
    };
    (got == size).then_some(info)
}

fn started(info: &libc::proc_bsdinfo) -> u64 {
    info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec
}

/// A fixed-size, NUL-terminated name field as a string.
fn field_str(field: &[libc::c_char]) -> String {
    // SAFETY: a `c_char` array reinterpreted as bytes of the same length.
    let bytes: &[u8] = unsafe { std::slice::from_raw_parts(field.as_ptr().cast(), field.len()) };
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

impl ProcessTable for SystemTable {
    fn lookup(&self, pid: u32) -> Option<ProcessInfo> {
        let info = bsd_info(pid)?;
        // The name the process started under: `pbi_name` (32 octets) when set, else
        // `pbi_comm` (16), neither of which follows a launcher symlink. The resolved
        // executable is `proc_pidpath`'s, read too.
        let name = field_str(&info.pbi_name);
        let image = if name.is_empty() {
            field_str(&info.pbi_comm)
        } else {
            name
        };
        let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: `path` is a writable buffer of the length passed.
        let n = unsafe {
            libc::proc_pidpath(
                pid as libc::c_int,
                path.as_mut_ptr().cast(),
                path.len() as u32,
            )
        };
        let resolved_image = (n > 0).then(|| {
            let full = String::from_utf8_lossy(&path[..n as usize]).into_owned();
            full.rsplit('/').next().unwrap_or(&full).to_owned()
        });
        // The process must be the same one after the reads (S2).
        if bsd_info(pid).map(|i| started(&i)) != Some(started(&info)) {
            return None;
        }
        Some(ProcessInfo {
            id: ProcessId {
                pid,
                started: started(&info),
            },
            parent: Some(info.pbi_ppid),
            image,
            resolved_image,
        })
    }

    fn working_directory(&self, process: &ProcessId) -> Option<String> {
        let pid = process.pid;
        let same = || bsd_info(pid).is_some_and(|i| started(&i) == process.started);
        if !same() {
            return None;
        }
        // SAFETY: proc_vnodepathinfo is plain data; zero is a valid starting value.
        let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
        // SAFETY: `info` is a writable local of `size` bytes.
        let got = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDVNODEPATHINFO,
                0,
                (&mut info as *mut libc::proc_vnodepathinfo).cast::<c_void>(),
                size,
            )
        };
        if got != size || !same() {
            return None;
        }
        // `vip_path` is a nested array that holds the NUL-terminated path (libc declares it
        // as 32 x 32 octets for old compilers' sake).
        // SAFETY: the array is contiguous plain data inside `info`.
        let bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(
                info.pvi_cdir.vip_path.as_ptr().cast::<u8>(),
                std::mem::size_of_val(&info.pvi_cdir.vip_path),
            )
        };
        let end = bytes.iter().position(|b| *b == 0)?;
        std::str::from_utf8(&bytes[..end]).ok().map(str::to_owned)
    }

    fn same_user(&self, process: &ProcessId) -> bool {
        // SAFETY: geteuid has no preconditions and cannot fail.
        let me = unsafe { libc::geteuid() };
        // The start time is read with the owner, and again after.
        let first = bsd_info(process.pid);
        let mine = first
            .as_ref()
            .is_some_and(|i| started(i) == process.started && i.pbi_uid == me);
        mine && bsd_info(process.pid).is_some_and(|i| started(&i) == process.started)
    }
}
