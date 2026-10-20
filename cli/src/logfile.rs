// SPDX-License-Identifier: Apache-2.0

//! The daemon's log file (G10, #71, slice 2): the lines `LogSink` writes, kept in a file so
//! that they survive the daemon, a crash included. Lead decision 2026-10-18 (Q2): "logs
//! should always be preserved to survive a crash".
//!
//! # Where
//!
//! `<state>/OAC/logs/daemon.log`, where `<state>` is the base the relay's TLS material uses
//! (`LOCALAPPDATA` on Windows, `~/.local/state` on Unix; `oac_transport_zenoh` keeps its own
//! helper private to the transport crate, so this module repeats the two-line rule rather than
//! reach into it). A test namespace (`OAC_SCOPE`, `ipc::Scope::for_user_in`) writes under
//! `<state>/OAC/logs/ns-<name>/` instead, so it never touches the user's own log.
//!
//! # Who can read it
//!
//! Only the user. Unix: the directory is `0700` and the file `0600`; a directory or file that
//! is not the user's, or is open to others, is **refused, never repaired**, and the file is
//! opened with `O_NOFOLLOW`, so a symlink in its place is refused too. Windows: the directory
//! and the file carry a protected DACL with one ACE, for the user (the pattern of the pipes in
//! `ipc::windows`); the DACL is read back and the file is refused unless it is that. An
//! administrator or root remains inside the OS trust boundary, as for the key and the pipes.
//!
//! # Durability (what a crash costs)
//!
//! Each line is **one `write` call on a file opened for append, then a flush**. The line is in
//! the operating system's file cache when the call returns, so it survives the end of the
//! daemon process at any point, a kill or a crash included: at most the line being written
//! when the process dies can be lost, and a line is never split across two files. It is **not
//! fsynced**: a power loss or an operating-system crash can lose the last seconds. That is a
//! deliberate trade: an fsync per line is a disk round trip (milliseconds, on Windows
//! `FlushFileBuffers`), and the lead's requirement is a daemon crash, not a machine crash.
//!
//! # Bounded
//!
//! At [`MAX_LOG_BYTES`] the file is closed and renamed to `daemon.log.1`, replacing the
//! previous one, and a new `daemon.log` is begun: at most two files and about 2 MiB, however
//! long the daemon runs. A line is cut at [`MAX_LINE_BYTES`]. **The current file is never
//! deleted**: if the rename fails (a sharing violation, a scanner, a viewer holding `.1`) the
//! file keeps growing and each line retries; past [`HARD_CAP_FACTOR`] times the limit the file
//! is truncated and begun again with a first line `log restarted: <N> bytes discarded, rotation
//! failing (<error kind>)`, so the newest lines are always kept and `daemon.log.1` is left
//! alone.
//!
//! # Content
//!
//! The same contract as `daemon::LogSink`: no message content, native session id, working
//! directory or key. This module adds a time to each line and nothing else.
//!
//! A log that cannot be opened or written never stops the daemon (see `LogSink`).

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The current log's file name.
pub const LOG_FILE_NAME: &str = "daemon.log";
/// The previous log's file name (one is kept).
pub const ROTATED_FILE_NAME: &str = "daemon.log.1";
/// The size at which the log is rotated.
pub const MAX_LOG_BYTES: u64 = 1024 * 1024;
/// While rotation keeps failing the log grows past its limit, to this many times the limit; past
/// that, the file is truncated and begun again with a marker line (the newest lines are kept).
pub const HARD_CAP_FACTOR: u64 = 4;
/// The longest line written, in bytes (longer is cut at a character boundary).
pub const MAX_LINE_BYTES: usize = 4096;

/// The per-user state directory the OAC files live under: `LOCALAPPDATA` on Windows,
/// `$HOME/.local/state` elsewhere. `None` when the variable is not set.
pub fn state_base() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state"))
    }
}

/// The directory of the log under `base`, for the user's own namespace (`None`) or a test
/// namespace.
///
/// # Errors
///
/// [`io::ErrorKind::InvalidInput`] for a namespace that is not a scope name.
pub fn log_directory(base: &Path, namespace: Option<&str>) -> io::Result<PathBuf> {
    let dir = base.join("OAC").join("logs");
    match namespace {
        None => Ok(dir),
        Some(n) if crate::ipc::scope_name_is_valid(n) => Ok(dir.join(format!("ns-{n}"))),
        Some(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a scope name is 1 to 24 lower-case letters and digits",
        )),
    }
}

/// Where the log of `namespace` is, in the user's state directory; `None` when that cannot be
/// told (no `LOCALAPPDATA` or `HOME`, or a namespace that is not a scope name).
pub fn default_log_path(namespace: Option<&str>) -> Option<PathBuf> {
    let dir = log_directory(&state_base()?, namespace).ok()?;
    Some(dir.join(LOG_FILE_NAME))
}

/// The open log file of one daemon.
#[derive(Debug)]
pub struct LogFile {
    directory: PathBuf,
    /// `None` only after a rotation that renamed the file and could not yet open the next.
    file: Option<File>,
    size: u64,
    limit: u64,
}

impl LogFile {
    /// Opens the log of `namespace` in the user's state directory, creating what is missing,
    /// private to the user.
    ///
    /// # Errors
    ///
    /// The operating system's error, [`io::ErrorKind::NotFound`] when there is no state
    /// directory, or [`io::ErrorKind::PermissionDenied`] when the directory or the file is
    /// not private to the user (or is a link): it is refused, never repaired.
    pub fn open(namespace: Option<&str>) -> io::Result<LogFile> {
        let base = state_base().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "neither LOCALAPPDATA nor HOME is set",
            )
        })?;
        LogFile::open_in(&base, namespace, MAX_LOG_BYTES)
    }

    /// [`LogFile::open`] under `base` (the directory that holds `OAC/`), rotating at `limit`
    /// bytes. For a test, or a tool that keeps its state elsewhere.
    ///
    /// # Errors
    ///
    /// As [`LogFile::open`].
    pub fn open_in(base: &Path, namespace: Option<&str>, limit: u64) -> io::Result<LogFile> {
        let directory = log_directory(base, namespace)?;
        os::make_private_directory(&directory, base.join("OAC").join("logs"))?;
        let (file, size) = os::open_private_file(&directory.join(LOG_FILE_NAME), false)?;
        Ok(LogFile {
            directory,
            file: Some(file),
            size,
            limit,
        })
    }

    /// The path of the current log file.
    pub fn path(&self) -> PathBuf {
        self.directory.join(LOG_FILE_NAME)
    }

    /// Appends one line (a newline is added): a time, the text, one `write`, one flush. Rotates
    /// first when the line would take the file past the limit.
    ///
    /// # Errors
    ///
    /// The operating system's error. **An error is transient:** the file is never deleted or
    /// closed for good, and the next call tries again (the caller, `LogSink`, warns once per
    /// run of failures). While rotation keeps failing, the file grows to [`HARD_CAP_FACTOR`]
    /// times the limit and is then truncated and begun again (same private open), its first
    /// line `log restarted: <N> bytes discarded, rotation failing (<error kind>)`: the
    /// newest lines are kept, `daemon.log.1` is not touched.
    pub fn append(&mut self, text: &str) -> io::Result<()> {
        let line = render(text);
        let length = line.len() as u64;
        if self.size > 0 && self.size + length > self.limit {
            // A failed rotation is not an error for this line: the line goes to the current
            // file, which keeps growing until the hard cap, and the next line tries again.
            if let Err(e) = self.rotate() {
                if self.file.is_none() {
                    return Err(e);
                }
                if self.size + length > self.limit.saturating_mul(HARD_CAP_FACTOR) {
                    self.restart(e.kind())?;
                }
            }
        }
        if self.file.is_none() {
            let (file, size) = os::open_private_file(&self.directory.join(LOG_FILE_NAME), false)?;
            self.file = Some(file);
            self.size = size;
        }
        self.write_line(&line)
    }

    fn write_line(&mut self, line: &str) -> io::Result<()> {
        let file = self.file.as_mut().expect("opened by the caller");
        file.write_all(line.as_bytes())?;
        file.flush()?;
        self.size += line.len() as u64;
        Ok(())
    }

    /// Truncates the current file and begins it again with a marker line: the log is past its
    /// hard cap because rotation keeps failing, and the newest lines matter more than the
    /// oldest (lead decision 2026-10-18, Q2). The previous log is not touched.
    fn restart(&mut self, kind: io::ErrorKind) -> io::Result<()> {
        let discarded = self.size;
        // Our handle first; if the open fails the next call reopens the file for append.
        self.file = None;
        let (file, size) = os::open_private_file(&self.directory.join(LOG_FILE_NAME), true)?;
        self.file = Some(file);
        self.size = size;
        let marker = render(&format!(
            "log restarted: {discarded} bytes discarded, rotation failing ({kind:?})"
        ));
        self.write_line(&marker)
    }

    /// Keeps the current file as the one previous log and begins a new one. **The current
    /// file is never deleted:** if it cannot be renamed (a sharing violation on Windows, an
    /// antivirus scan, a viewer holding `daemon.log.1`, a directory in the way), nothing
    /// changes and the caller goes on writing to it.
    fn rotate(&mut self) -> io::Result<()> {
        let current = self.directory.join(LOG_FILE_NAME);
        let previous = self.directory.join(ROTATED_FILE_NAME);
        // The rename replaces the previous log (std's rename does on both systems), so the
        // previous log is not removed first; our handle stays open (it is opened with delete
        // sharing on Windows) and goes with the renamed file.
        fs::rename(&current, &previous)?;
        self.file = None;
        self.size = 0;
        let (file, size) = os::open_private_file(&current, false)?;
        self.file = Some(file);
        self.size = size;
        Ok(())
    }
}

/// `unix_ms=<time> <text>\n`, the text cut at [`MAX_LINE_BYTES`] and its line breaks made
/// spaces, so that one call is one line.
fn render(text: &str) -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let mut line = format!("unix_ms={ms} ");
    let mut room = MAX_LINE_BYTES;
    for c in text.chars() {
        let c = if c == '\n' || c == '\r' { ' ' } else { c };
        if c.len_utf8() > room {
            break;
        }
        room -= c.len_utf8();
        line.push(c);
    }
    line.push('\n');
    line
}

/// Whether `path` (a file or a directory) is private to the user: on Unix owned by the user
/// with no access for group or others, on Windows owned by the user or by Administrators
/// with a DACL of exactly one ACE, for the user. For a test and for the checks that open the
/// file.
///
/// # Errors
///
/// The operating system's error.
pub fn is_private(path: &Path) -> io::Result<bool> {
    os::is_private(path)
}

#[cfg(unix)]
mod os {
    use std::fs::{self, DirBuilder, File, OpenOptions};
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    use std::path::Path;

    fn euid() -> u32 {
        // SAFETY: geteuid has no preconditions, cannot fail and touches no memory of ours.
        unsafe { libc::geteuid() }
    }

    fn denied(why: &'static str) -> io::Error {
        io::Error::new(io::ErrorKind::PermissionDenied, why)
    }

    pub fn is_private(path: &Path) -> io::Result<bool> {
        let meta = fs::symlink_metadata(path)?;
        Ok(!meta.file_type().is_symlink() && meta.uid() == euid() && meta.mode() & 0o077 == 0)
    }

    /// Creates `directory` (and the parents) `0700` and checks it, and `top`, the log
    /// directory, which `directory` is or is inside.
    pub fn make_private_directory(directory: &Path, top: std::path::PathBuf) -> io::Result<()> {
        for dir in [top.as_path(), directory] {
            match fs::symlink_metadata(dir) {
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
                }
                Err(e) => return Err(e),
            }
            let meta = fs::symlink_metadata(dir)?;
            if !meta.is_dir() {
                return Err(denied("the log directory is not a directory"));
            }
            if meta.uid() != euid() {
                return Err(denied("the log directory is not owned by this user"));
            }
            if meta.mode() & 0o077 != 0 {
                return Err(denied("the log directory is accessible to other users"));
            }
        }
        Ok(())
    }

    /// Opens `path` for append (`truncate`: emptied, for writing), `0600`, created if
    /// missing; a link in its place is refused (`O_NOFOLLOW`), and so is a file that is not
    /// the user's or is open to others.
    pub fn open_private_file(path: &Path, truncate: bool) -> io::Result<(File, u64)> {
        let mut options = OpenOptions::new();
        if truncate {
            // Append and truncate cannot be asked together; the one writer then writes in
            // order from the start.
            options.write(true).truncate(true);
        } else {
            options.append(true);
        }
        let file = options
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|e| {
                if e.raw_os_error() == Some(libc::ELOOP) {
                    denied("the log file is a link")
                } else {
                    e
                }
            })?;
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(denied("the log file is not a regular file"));
        }
        if meta.uid() != euid() {
            return Err(denied("the log file is not owned by this user"));
        }
        if meta.mode() & 0o077 != 0 {
            return Err(denied("the log file is accessible to other users"));
        }
        Ok((file, meta.len()))
    }
}

#[cfg(windows)]
mod os {
    use std::ffi::c_void;
    use std::fs::{self, File, OpenOptions};
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        GetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
    };
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACL, DACL_SECURITY_INFORMATION, GetAce, GetSecurityDescriptorDacl,
        OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    };

    /// `BUILTIN\Administrators`: the default owner of what an elevated administrator makes.
    const ADMINISTRATORS: &str = "S-1-5-32-544";

    fn denied(why: &'static str) -> io::Error {
        io::Error::new(io::ErrorKind::PermissionDenied, why)
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn own_sid() -> io::Result<String> {
        crate::peer::current_user_sid_string()
            .ok_or_else(|| io::Error::other("the current user's SID cannot be read"))
    }

    /// Replaces the DACL of `path` with a protected one of one ACE for the user (`inherit`:
    /// the ACE passes to what is created inside, for a directory).
    fn protect(path: &Path, inherit: bool) -> io::Result<()> {
        let sid = own_sid()?;
        let flags = if inherit { "OICI" } else { "" };
        let sddl: Vec<u16> = format!("D:P(A;{flags};FA;;;{sid})")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut descriptor: *mut c_void = std::ptr::null_mut();
        // SAFETY: `sddl` is a NUL-terminated UTF-16 string; `descriptor` is a local out-pointer.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || descriptor.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl: *mut ACL = std::ptr::null_mut();
        // SAFETY: `descriptor` was just made; the out-pointers are locals; `dacl` points
        // into `descriptor`, which is freed after the last use below.
        let got = unsafe {
            GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted)
        };
        let result = if got == 0 || present == 0 || dacl.is_null() {
            Err(io::Error::last_os_error())
        } else {
            let name = wide(path);
            // SAFETY: `name` is NUL-terminated; `dacl` is valid until the free below. Only
            // the DACL is set, and it is protected (no inherited entry).
            let status = unsafe {
                SetNamedSecurityInfoW(
                    name.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    dacl,
                    std::ptr::null(),
                )
            };
            if status == 0 {
                Ok(())
            } else {
                Err(io::Error::from_raw_os_error(status as i32))
            }
        };
        // SAFETY: allocated by the conversion call, freed once.
        unsafe { LocalFree(descriptor) };
        result
    }

    /// A SID as its string form.
    ///
    /// # Safety
    ///
    /// `sid` must point to a valid SID.
    unsafe fn sid_string(sid: *mut c_void) -> Option<String> {
        let mut text: *mut u16 = std::ptr::null_mut();
        // SAFETY: the caller's `sid` is valid; `text` is a local out-pointer.
        if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
            return None;
        }
        // SAFETY: on success `text` is a NUL-terminated UTF-16 string the call allocated.
        let result = unsafe {
            let mut len = 0usize;
            while *text.add(len) != 0 {
                len += 1;
            }
            String::from_utf16(std::slice::from_raw_parts(text, len)).ok()
        };
        // SAFETY: allocated by ConvertSidToStringSidW, freed once.
        unsafe { LocalFree(text.cast()) };
        result
    }

    pub fn is_private(path: &Path) -> io::Result<bool> {
        if fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Ok(false);
        }
        let own = own_sid()?;
        let name = wide(path);
        let mut owner: *mut c_void = std::ptr::null_mut();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut descriptor: *mut c_void = std::ptr::null_mut();
        // SAFETY: `name` is NUL-terminated; the out-pointers are locals.
        let status = unsafe {
            GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let private = (|| {
            if owner.is_null() || dacl.is_null() {
                return false;
            }
            // SAFETY: `owner` and `dacl` point into `descriptor`, allocated until the
            // LocalFree below; the one ACE read is within the ACL's count.
            unsafe {
                let owner_sid = sid_string(owner);
                if !matches!(owner_sid.as_deref(), Some(s) if s == own || s == ADMINISTRATORS)
                    || (*dacl).AceCount != 1
                {
                    return false;
                }
                let mut ace: *mut c_void = std::ptr::null_mut();
                if GetAce(dacl, 0, &mut ace) == 0 || ace.is_null() {
                    return false;
                }
                let ace = ace.cast::<ACCESS_ALLOWED_ACE>();
                // `AceType` 0 is ACCESS_ALLOWED_ACE_TYPE.
                (*ace).Header.AceType == 0
                    && sid_string(std::ptr::addr_of_mut!((*ace).SidStart).cast()).as_deref()
                        == Some(own.as_str())
            }
        })();
        // SAFETY: allocated by GetNamedSecurityInfoW, freed once.
        unsafe { LocalFree(descriptor) };
        Ok(private)
    }

    /// Makes `directory` (and the parents), gives the log directory `top` and `directory` the
    /// user-only DACL, and checks both.
    pub fn make_private_directory(directory: &Path, top: PathBuf) -> io::Result<()> {
        fs::create_dir_all(directory)?;
        for dir in [top.as_path(), directory] {
            if fs::symlink_metadata(dir)?.file_type().is_symlink() {
                return Err(denied("the log directory is a link"));
            }
            protect(dir, true)?;
            if !is_private(dir)? {
                return Err(denied("the log directory is not private to this user"));
            }
        }
        Ok(())
    }

    /// Opens `path` for append (`truncate`: emptied, for writing), created if missing, with
    /// the user-only DACL on it; a link in its place is refused.
    pub fn open_private_file(path: &Path, truncate: bool) -> io::Result<(File, u64)> {
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(denied("the log file is a link"));
            }
            Ok(_) | Err(_) => {}
        }
        let mut options = OpenOptions::new();
        if truncate {
            options.write(true).truncate(true);
        } else {
            options.append(true);
        }
        let file = options.create(true).open(path)?;
        protect(path, false)?;
        if !is_private(path)? {
            return Err(denied("the log file is not private to this user"));
        }
        let size = file.metadata()?.len();
        Ok((file, size))
    }
}

#[cfg(test)]
mod tests {
    use super::{LogFile, MAX_LINE_BYTES, log_directory, render};
    use std::path::Path;

    #[test]
    fn a_namespace_has_a_directory_of_its_own() {
        let base = Path::new("base");
        let own = log_directory(base, None).unwrap();
        let test = log_directory(base, Some("abc1")).unwrap();
        assert_ne!(own, test);
        assert!(test.starts_with(&own));
        assert!(log_directory(base, Some("../x")).is_err());
        assert!(log_directory(base, Some("")).is_err());
    }

    #[test]
    fn a_line_is_one_line_and_bounded() {
        let line = render("one\ntwo\r\nthree");
        assert_eq!(line.matches('\n').count(), 1, "{line:?}");
        assert!(line.ends_with('\n'));
        let long = render(&"é".repeat(MAX_LINE_BYTES));
        assert!(long.len() <= MAX_LINE_BYTES + 40, "{}", long.len());
        assert!(long.ends_with('\n'));
    }

    #[test]
    fn rotation_keeps_one_previous_file() {
        let dir = std::env::temp_dir().join(format!("oac-logfile-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut log = LogFile::open_in(&dir, Some("unit"), 200).unwrap();
        for i in 0..30 {
            log.append(&format!("line {i}")).unwrap();
        }
        let logs = log_directory(&dir, Some("unit")).unwrap();
        let current = std::fs::metadata(logs.join("daemon.log")).unwrap().len();
        let previous = std::fs::metadata(logs.join("daemon.log.1")).unwrap().len();
        assert!(current <= 200 && previous <= 200, "{current} {previous}");
        assert!(!logs.join("daemon.log.2").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
