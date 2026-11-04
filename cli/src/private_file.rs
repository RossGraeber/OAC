// SPDX-License-Identifier: Apache-2.0

//! Files and directories private to the user (G10, #71; lifted out of `logfile` by #437 S0.5 so
//! that the pairing trust store and the agreement-key file reuse them). Unix: a directory is
//! `0700` and a file `0600`, owned by the user; one that is a link, not the user's, or open to
//! others is **refused, never repaired**, and a file is opened with `O_NOFOLLOW`. Windows: the
//! directory and the file carry a protected DACL with one ACE, for the user (the pattern of the
//! pipes in `ipc::windows`), read back and refused unless it is that. An administrator or root
//! remains inside the OS trust boundary, as for the key and the pipes.
//!
//! `what` names the thing in the error text ("log directory", "log file"), so a refusal says
//! which file it refused.

use std::fs::File;
use std::io;
use std::path::Path;

/// Whether `path` (a file or a directory) is private to the user: on Unix owned by the user
/// with no access for group or others, on Windows owned by the user or by Administrators
/// with a DACL of exactly one ACE, for the user.
///
/// # Errors
///
/// The operating system's error.
pub(crate) fn is_private(path: &Path) -> io::Result<bool> {
    os::is_private(path)
}

/// Creates `directory` (and the parents) private to the user and checks it, and `top`, which
/// `directory` is or is inside. Unix: `0700`. Windows: the user-only DACL.
///
/// # Errors
///
/// The operating system's error, or [`io::ErrorKind::PermissionDenied`] when a directory is a
/// link, is not the user's or is open to others: it is refused, never repaired.
pub(crate) fn make_private_directory(directory: &Path, top: &Path, what: &str) -> io::Result<()> {
    os::make_private_directory(directory, top, what)
}

/// Opens `path` for append (`truncate`: emptied, for writing), created if missing, private to
/// the user; returns the file and its size.
///
/// # Errors
///
/// The operating system's error, or [`io::ErrorKind::PermissionDenied`] when `path` is a link,
/// is not the user's or is open to others.
pub(crate) fn open_private_file(
    path: &Path,
    truncate: bool,
    what: &str,
) -> io::Result<(File, u64)> {
    os::open_private_file(path, truncate, what)
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

    fn denied(why: String) -> io::Error {
        io::Error::new(io::ErrorKind::PermissionDenied, why)
    }

    pub(super) fn is_private(path: &Path) -> io::Result<bool> {
        let meta = fs::symlink_metadata(path)?;
        Ok(!meta.file_type().is_symlink() && meta.uid() == euid() && meta.mode() & 0o077 == 0)
    }

    /// Creates `directory` (and the parents) `0700` and checks it, and `top`, the log
    /// directory, which `directory` is or is inside.
    pub(super) fn make_private_directory(
        directory: &Path,
        top: &Path,
        what: &str,
    ) -> io::Result<()> {
        for dir in [top, directory] {
            match fs::symlink_metadata(dir) {
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
                }
                Err(e) => return Err(e),
            }
            let meta = fs::symlink_metadata(dir)?;
            if !meta.is_dir() {
                return Err(denied(format!("the {what} is not a directory")));
            }
            if meta.uid() != euid() {
                return Err(denied(format!("the {what} is not owned by this user")));
            }
            if meta.mode() & 0o077 != 0 {
                return Err(denied(format!("the {what} is accessible to other users")));
            }
        }
        Ok(())
    }

    /// Opens `path` for append (`truncate`: emptied, for writing), `0600`, created if
    /// missing; a link in its place is refused (`O_NOFOLLOW`), and so is a file that is not
    /// the user's or is open to others.
    pub(super) fn open_private_file(
        path: &Path,
        truncate: bool,
        what: &str,
    ) -> io::Result<(File, u64)> {
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
                    denied(format!("the {what} is a link"))
                } else {
                    e
                }
            })?;
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(denied(format!("the {what} is not a regular file")));
        }
        if meta.uid() != euid() {
            return Err(denied(format!("the {what} is not owned by this user")));
        }
        if meta.mode() & 0o077 != 0 {
            return Err(denied(format!("the {what} is accessible to other users")));
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
    use std::path::Path;

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

    fn denied(why: String) -> io::Error {
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

    pub(super) fn is_private(path: &Path) -> io::Result<bool> {
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
    pub(super) fn make_private_directory(
        directory: &Path,
        top: &Path,
        what: &str,
    ) -> io::Result<()> {
        fs::create_dir_all(directory)?;
        for dir in [top, directory] {
            if fs::symlink_metadata(dir)?.file_type().is_symlink() {
                return Err(denied(format!("the {what} is a link")));
            }
            protect(dir, true)?;
            if !is_private(dir)? {
                return Err(denied(format!("the {what} is not private to this user")));
            }
        }
        Ok(())
    }

    /// Opens `path` for append (`truncate`: emptied, for writing), created if missing, with
    /// the user-only DACL on it; a link in its place is refused.
    pub(super) fn open_private_file(
        path: &Path,
        truncate: bool,
        what: &str,
    ) -> io::Result<(File, u64)> {
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(denied(format!("the {what} is a link")));
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
            return Err(denied(format!("the {what} is not private to this user")));
        }
        let size = file.metadata()?.len();
        Ok((file, size))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    /// A fresh directory under the system's temporary one, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(tag: &str) -> Temp {
            let dir =
                std::env::temp_dir().join(format!("oac-private-file-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_loose_mode_is_refused_not_repaired() {
        use super::{make_private_directory, open_private_file};
        use std::os::unix::fs::PermissionsExt;
        let temp = Temp::new("loose");
        let dir = temp.0.join("private");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let e = make_private_directory(&dir, &dir, "directory").unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o755,
            "not repaired"
        );

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        make_private_directory(&dir, &dir, "directory").unwrap();
        let file = dir.join("data");
        std::fs::write(&file, "old\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let e = open_private_file(&file, false, "file").unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o644,
            "not repaired"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "old\n");
    }

    #[cfg(windows)]
    #[test]
    fn the_dacl_is_user_only() {
        use super::{is_private, make_private_directory, open_private_file};
        let temp = Temp::new("dacl");
        let dir = temp.0.join("private");
        make_private_directory(&dir, &dir, "directory").unwrap();
        assert!(is_private(&dir).unwrap());
        let file = dir.join("data");
        let (_handle, size) = open_private_file(&file, false, "file").unwrap();
        assert_eq!(size, 0);
        assert!(is_private(&file).unwrap());
    }
}
