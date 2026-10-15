// SPDX-License-Identifier: Apache-2.0

//! The real process tables: one per operating system, each behind the [`ProcessTable`]
//! trait of the parent module.
//!
//! - Windows: a Toolhelp snapshot for parents and executable names; the process handle for
//!   the start time, the owner's token and the working directory (the process's PEB, read
//!   with `ReadProcessMemory`: no documented call gives another process's working
//!   directory, so this relies on the PEB's layout, which has been stable on 64-bit
//!   Windows. It is read for same-user processes only, fails closed on any error, and gives
//!   `None` for a 32-bit process).
//! - Linux: `/proc/<pid>/stat`, `exe`, `cwd`.
//! - macOS: `proc_pidinfo` and `proc_pidpath` (written against the verified `libc`
//!   symbols, not built or run in this repository's checks: no Mac, recorded as a residual,
//!   D9).
//!
//! [`ProcessTable`]: super::ProcessTable

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::SystemTable;
#[cfg(windows)]
pub use windows::{current_user_sid_string, process_is_current_user};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::SystemTable;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::SystemTable;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod other {
    use super::super::{ProcessId, ProcessInfo, ProcessTable};

    /// A process table for an operating system with no reader: it knows no process, so no
    /// peer has a key (fail closed).
    #[derive(Debug, Default)]
    pub struct SystemTable;

    impl SystemTable {
        /// The table.
        pub fn new() -> std::io::Result<SystemTable> {
            Ok(SystemTable)
        }
    }

    impl ProcessTable for SystemTable {
        fn lookup(&self, _: u32) -> Option<ProcessInfo> {
            None
        }
        fn working_directory(&self, _: &ProcessId) -> Option<String> {
            None
        }
        fn same_user(&self, _: u32) -> bool {
            false
        }
    }
}
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub use other::SystemTable;
