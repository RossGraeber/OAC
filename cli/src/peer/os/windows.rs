// SPDX-License-Identifier: Apache-2.0

//! The Windows process table.
//!
//! - Parents and executable names come from one Toolhelp snapshot taken when the table is
//!   made (`CreateToolhelp32Snapshot`); a process that starts later is not in it, which only
//!   makes a walk end (fail closed). The parent's start time is checked against the child's
//!   by the caller, because a snapshot's parent id can name a recycled process.
//! - The start time is the process's creation time (`GetProcessTimes`, 100 ns ticks since
//!   1601), read through a handle opened for the call.
//! - The owner is compared by the process token's user SID (`OpenProcessToken`,
//!   `GetTokenInformation(TokenUser)`, `EqualSid`).
//! - The working directory is read from the process's PEB: `NtQueryInformationProcess`
//!   gives the PEB's address, and `ReadProcessMemory` reads `ProcessParameters` and, from it,
//!   `CurrentDirectory.DosPath`. No documented call returns another process's working
//!   directory. The offsets below are those of the 64-bit PEB and `RTL_USER_PROCESS_PARAMETERS`
//!   (stable since Windows Vista, public in the symbol files and in debugger output); a
//!   32-bit process (WOW64) has other layouts and gives `None`, as does any failure. The
//!   handle is opened with only `PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ`, which
//!   Windows grants for a process of the same user and refuses for a protected one.

use std::collections::HashMap;
use std::ffi::c_void;
use std::io;
use std::os::windows::io::{FromRawHandle, OwnedHandle, RawHandle};

use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
use windows_sys::Win32::Foundation::{FILETIME, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, IsWow64Process, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};

use super::super::{ProcessId, ProcessInfo, ProcessTable};

/// The Windows process table: a snapshot of the process list, and live reads for the rest.
pub struct SystemTable {
    /// pid -> (parent pid, executable file name).
    processes: HashMap<u32, (u32, String)>,
    /// The daemon's own token user (a `TOKEN_USER` followed by its SID), 8-byte aligned.
    own_user: Vec<u64>,
}

impl std::fmt::Debug for SystemTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemTable")
            .field("processes", &self.processes.len())
            .finish_non_exhaustive()
    }
}

/// The 64-bit `PROCESS_BASIC_INFORMATION` (`ntddk.h`).
#[repr(C)]
struct ProcessBasicInformationData {
    exit_status: i32,
    peb_base_address: *const c_void,
    affinity_mask: usize,
    base_priority: i32,
    unique_process_id: usize,
    inherited_from_unique_process_id: usize,
}

/// `PEB.ProcessParameters` on 64-bit Windows.
const PEB_PROCESS_PARAMETERS: usize = 0x20;
/// `RTL_USER_PROCESS_PARAMETERS.CurrentDirectory.DosPath` (a `UNICODE_STRING`) on 64-bit
/// Windows.
const PARAMETERS_CURRENT_DIRECTORY: usize = 0x38;
/// Within a 64-bit `UNICODE_STRING`: `Length` (bytes) is a `u16` at 0; `Buffer` is a pointer
/// at 8.
const UNICODE_STRING_BUFFER: usize = 8;
/// The longest path Windows allows, in UTF-16 units.
const MAX_PATH_UNITS: usize = 32_768;

fn open_process(pid: u32, access: u32) -> Option<OwnedHandle> {
    // SAFETY: OpenProcess has no memory preconditions; it returns null on failure.
    let h = unsafe { OpenProcess(access, 0, pid) };
    if h.is_null() || h == INVALID_HANDLE_VALUE {
        return None;
    }
    // SAFETY: `h` is a handle OpenProcess just returned and nothing else owns.
    Some(unsafe { OwnedHandle::from_raw_handle(h as RawHandle) })
}

fn raw(h: &OwnedHandle) -> HANDLE {
    std::os::windows::io::AsRawHandle::as_raw_handle(h) as HANDLE
}

/// The creation time of the process behind `h`, as 100 ns ticks since 1601.
fn creation_time(h: HANDLE) -> Option<u64> {
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: `h` is a live process handle; the four out-pointers point to locals.
    let ok = unsafe { GetProcessTimes(h, &mut created, &mut exited, &mut kernel, &mut user) };
    (ok != 0).then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

/// The token user of `process` as a `TOKEN_USER` and the SID it points into, in a buffer
/// aligned for both.
fn token_user(process: HANDLE) -> Option<Vec<u64>> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: `process` is a live process handle; `token` is a local out-pointer.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }
    // SAFETY: OpenProcessToken returned `token`; nothing else owns it.
    let token = unsafe { OwnedHandle::from_raw_handle(token as RawHandle) };
    let mut needed = 0u32;
    // SAFETY: a size query: a null buffer of length 0, `needed` is a local.
    unsafe { GetTokenInformation(raw(&token), TokenUser, std::ptr::null_mut(), 0, &mut needed) };
    if needed == 0 || needed > 4096 {
        return None;
    }
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: `buffer` holds at least `needed` bytes and is 8-byte aligned.
    let ok = unsafe {
        GetTokenInformation(
            raw(&token),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    (ok != 0).then_some(buffer)
}

/// The SID inside a [`token_user`] buffer.
fn sid_of(buffer: &[u64]) -> *mut c_void {
    // SAFETY: the buffer holds a `TOKEN_USER` (filled by GetTokenInformation), aligned.
    unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid }
}

impl SystemTable {
    /// A table of the processes running now.
    ///
    /// # Errors
    ///
    /// The operating system's error when the process list or the daemon's own token cannot
    /// be read.
    pub fn new() -> io::Result<SystemTable> {
        // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing.
        let own_user = token_user(unsafe { GetCurrentProcess() })
            .ok_or_else(|| io::Error::other("the daemon's own token cannot be read"))?;
        // SAFETY: a snapshot of the process list; the result is checked below.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `snapshot` is a handle just returned and nothing else owns.
        let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot as RawHandle) };
        let mut processes = HashMap::new();
        // SAFETY: PROCESSENTRY32W is plain data; zero is a valid starting value.
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        // SAFETY: `snapshot` is live and `entry` is a local with `dwSize` set.
        let mut more = unsafe { Process32FirstW(raw(&snapshot), &mut entry) } != 0;
        while more {
            let len = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            let image = String::from_utf16_lossy(&entry.szExeFile[..len]);
            processes.insert(entry.th32ProcessID, (entry.th32ParentProcessID, image));
            // SAFETY: as above.
            more = unsafe { Process32NextW(raw(&snapshot), &mut entry) } != 0;
        }
        Ok(SystemTable {
            processes,
            own_user,
        })
    }

    /// Reads `size` bytes at `address` in the process behind `h` into `out`.
    fn read(h: HANDLE, address: usize, out: &mut [u8]) -> bool {
        let mut got = 0usize;
        // SAFETY: `out` is a writable buffer of `out.len()` bytes; the other process's
        // memory is only read through the call, which reports failure for a bad address.
        let ok = unsafe {
            ReadProcessMemory(
                h,
                address as *const c_void,
                out.as_mut_ptr().cast(),
                out.len(),
                &mut got,
            )
        };
        ok != 0 && got == out.len()
    }

    fn read_usize(h: HANDLE, address: usize) -> Option<usize> {
        let mut bytes = [0u8; std::mem::size_of::<usize>()];
        Self::read(h, address, &mut bytes).then(|| usize::from_le_bytes(bytes))
    }
}

impl ProcessTable for SystemTable {
    fn lookup(&self, pid: u32) -> Option<ProcessInfo> {
        let (parent, image) = self.processes.get(&pid)?;
        let handle = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        let started = creation_time(raw(&handle))?;
        Some(ProcessInfo {
            id: ProcessId { pid, started },
            parent: Some(*parent),
            image: image.clone(),
        })
    }

    fn working_directory(&self, process: &ProcessId) -> Option<String> {
        if std::mem::size_of::<usize>() != 8 {
            return None;
        }
        let handle = open_process(
            process.pid,
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
        )?;
        let h = raw(&handle);
        // The handle pins the process; it must be the one observed.
        if creation_time(h)? != process.started {
            return None;
        }
        // A 32-bit process has other layouts.
        let mut wow64 = 0i32;
        // SAFETY: `h` is a live process handle; `wow64` is a local.
        if unsafe { IsWow64Process(h, &mut wow64) } == 0 || wow64 != 0 {
            return None;
        }
        // SAFETY: plain data; zero is a valid starting value.
        let mut basic: ProcessBasicInformationData = unsafe { std::mem::zeroed() };
        let mut returned = 0u32;
        // SAFETY: `basic` is a writable local of the size passed.
        let status = unsafe {
            NtQueryInformationProcess(
                h,
                ProcessBasicInformation,
                (&mut basic as *mut ProcessBasicInformationData).cast(),
                std::mem::size_of::<ProcessBasicInformationData>() as u32,
                &mut returned,
            )
        };
        if status != 0 || basic.peb_base_address.is_null() {
            return None;
        }
        let params = Self::read_usize(h, basic.peb_base_address as usize + PEB_PROCESS_PARAMETERS)?;
        if params == 0 {
            return None;
        }
        let string = params + PARAMETERS_CURRENT_DIRECTORY;
        let mut length = [0u8; 2];
        if !Self::read(h, string, &mut length) {
            return None;
        }
        let bytes = usize::from(u16::from_le_bytes(length));
        let buffer = Self::read_usize(h, string + UNICODE_STRING_BUFFER)?;
        if bytes == 0 || bytes % 2 != 0 || bytes / 2 > MAX_PATH_UNITS || buffer == 0 {
            return None;
        }
        let mut raw_units = vec![0u8; bytes];
        if !Self::read(h, buffer, &mut raw_units) {
            return None;
        }
        // Still the same process after the reads.
        if creation_time(h)? != process.started {
            return None;
        }
        let (pairs, _) = raw_units.as_chunks::<2>();
        let units: Vec<u16> = pairs.iter().map(|c| u16::from_le_bytes(*c)).collect();
        let mut path = String::from_utf16(&units).ok()?;
        // `C:\work\` -> `C:\work`; a root (`C:\`) keeps its separator.
        while path.len() > 3 && path.ends_with('\\') {
            path.pop();
        }
        Some(path)
    }

    fn same_user(&self, pid: u32) -> bool {
        let Some(handle) = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION) else {
            return false;
        };
        let Some(theirs) = token_user(raw(&handle)) else {
            return false;
        };
        // SAFETY: both SIDs are inside their own live, aligned buffers.
        unsafe { EqualSid(sid_of(&self.own_user), sid_of(&theirs)) != 0 }
    }
}
