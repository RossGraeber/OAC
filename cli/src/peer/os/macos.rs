// SPDX-License-Identifier: Apache-2.0

//! The macOS process table, from `proc_pidinfo` and `proc_pidpath` (`libproc`).
//!
//! **Not built or run in this repository's checks**: there is no Mac here, so this file is
//! written against the symbols as `libc` 0.2.190 declares them (`proc_pidinfo`, `proc_pidpath`,
//! `proc_bsdinfo`, `proc_vnodepathinfo`, `PROC_PIDTBSDINFO`, `PROC_PIDVNODEPATHINFO`), and the
//! gap is a recorded residual (lead decision D9, 2026-10-14). The first Mac run is its test.

use std::ffi::{CStr, c_void};
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

impl ProcessTable for SystemTable {
    fn lookup(&self, pid: u32) -> Option<ProcessInfo> {
        let info = bsd_info(pid)?;
        let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: `path` is a writable buffer of the length passed.
        let n = unsafe {
            libc::proc_pidpath(
                pid as libc::c_int,
                path.as_mut_ptr().cast(),
                path.len() as u32,
            )
        };
        let image = if n > 0 {
            let full = String::from_utf8_lossy(&path[..n as usize]).into_owned();
            full.rsplit('/').next().unwrap_or(&full).to_owned()
        } else {
            // The 16-octet name in the BSD info.
            // SAFETY: `pbi_comm` is a NUL-terminated array inside `info`.
            unsafe { CStr::from_ptr(info.pbi_comm.as_ptr()) }
                .to_string_lossy()
                .into_owned()
        };
        Some(ProcessInfo {
            id: ProcessId {
                pid,
                started: started(&info),
            },
            parent: Some(info.pbi_ppid),
            image,
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

    fn same_user(&self, pid: u32) -> bool {
        // SAFETY: geteuid has no preconditions and cannot fail.
        let me = unsafe { libc::geteuid() };
        bsd_info(pid).is_some_and(|i| i.pbi_uid == me)
    }
}
