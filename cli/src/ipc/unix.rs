// SPDX-License-Identifier: Apache-2.0

//! The Unix local IPC: `AF_UNIX` stream sockets in a private run directory (C2 §4).

use std::ffi::OsStr;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream as StdStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::net::{UnixListener, UnixStream};

use super::{Accept, Accepted, AsyncStream, BindError, ConnectError, PeerIdentity, Rejection};

/// The blocking client stream.
pub type Blocking = StdStream;

/// Socket paths must fit `sun_path` (104 octets on macOS, 108 on Linux), with room.
const MAX_SOCKET_PATH: usize = 100;

fn euid() -> u32 {
    // SAFETY: geteuid has no preconditions, cannot fail and touches no memory of ours.
    unsafe { libc::geteuid() }
}

/// The run directory for the given `$XDG_RUNTIME_DIR` and `$HOME`: `<xdg>/oac` when `xdg` is
/// set, absolute and not empty, else `<home>/.oac/run`; `None` with neither.
pub fn run_dir_from(xdg: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    if let Some(x) = xdg.filter(|x| !x.is_empty())
        && Path::new(x).is_absolute()
    {
        return Some(Path::new(x).join("oac"));
    }
    let home = home.filter(|h| !h.is_empty())?;
    Some(Path::new(home).join(".oac").join("run"))
}

/// `dir` checked as private: a real directory (not a link), owned by this user, closed to
/// everyone else.
fn check_private(dir: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(dir)?;
    if !meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the run directory is not a directory",
        ));
    }
    if meta.uid() != euid() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the run directory is not owned by this user",
        ));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the run directory is accessible to other users",
        ));
    }
    Ok(())
}

/// Creates `dir` (and any missing parent) `0700` and checks it.
fn ensure_private(dir: &Path) -> io::Result<()> {
    match fs::symlink_metadata(dir) {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        }
        Err(e) => return Err(e),
    }
    check_private(dir)
}

/// Removes a unique scope's directory when the last clone drops.
#[derive(Debug)]
struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A user's run directory.
#[derive(Clone, Debug)]
pub struct Scope {
    dir: PathBuf,
    _cleanup: Option<Arc<Cleanup>>,
}

static NEXT_SCOPE: AtomicU64 = AtomicU64::new(0);

impl Scope {
    pub fn for_user() -> io::Result<Scope> {
        let dir = run_dir_from(
            std::env::var_os("XDG_RUNTIME_DIR").as_deref(),
            std::env::var_os("HOME").as_deref(),
        )
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "neither XDG_RUNTIME_DIR nor HOME is set",
            )
        })?;
        ensure_private(&dir)?;
        Ok(Scope {
            dir,
            _cleanup: None,
        })
    }

    pub fn unique() -> io::Result<Scope> {
        let n = NEXT_SCOPE.fetch_add(1, Ordering::Relaxed);
        // Short: a socket path must fit `sun_path`.
        let dir = std::env::temp_dir().join(format!("oac-i{}-{n}", std::process::id()));
        ensure_private(&dir)?;
        Ok(Scope {
            dir: dir.clone(),
            _cleanup: Some(Arc::new(Cleanup(dir))),
        })
    }

    pub fn endpoint_name(&self, label: &str) -> String {
        self.dir.join(format!("{label}.sock")).display().to_string()
    }

    fn socket(&self, label: &str) -> io::Result<PathBuf> {
        let path = self.dir.join(format!("{label}.sock"));
        if path.as_os_str().len() > MAX_SOCKET_PATH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the socket path is too long for the system",
            ));
        }
        Ok(path)
    }
}

/// A bound socket; the socket file is removed when it drops.
pub struct Listener {
    inner: UnixListener,
    path: PathBuf,
}

impl Listener {
    pub async fn serve(scope: &Scope, label: &str) -> Result<Listener, BindError> {
        let path = scope.socket(label)?;
        // The directory is checked again: it is what keeps other users away from the socket.
        check_private(&scope.dir)?;
        for _ in 0..2 {
            match UnixListener::bind(&path) {
                Ok(inner) => {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
                    return Ok(Listener { inner, path });
                }
                Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
                    // A socket file is stale when nothing listens on it (its daemon died).
                    match StdStream::connect(&path) {
                        Ok(_) => return Err(BindError::AlreadyRunning),
                        Err(c)
                            if matches!(
                                c.kind(),
                                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                            ) =>
                        {
                            let _ = fs::remove_file(&path);
                        }
                        Err(c) => return Err(BindError::Io(c)),
                    }
                }
                Err(e) => return Err(BindError::Io(e)),
            }
        }
        Err(BindError::AlreadyRunning)
    }

    pub async fn accept(&mut self) -> io::Result<Accept> {
        let (stream, _) = self.inner.accept().await?;
        let Ok(cred) = stream.peer_cred() else {
            return Ok(Accept::Rejected(Rejection {
                reason: "the peer's credentials could not be read",
            }));
        };
        if !same_user(cred.uid(), euid()) {
            return Ok(Accept::Rejected(Rejection {
                reason: "the peer is another user",
            }));
        }
        let pid = cred.pid().and_then(|p| u32::try_from(p).ok());
        Ok(Accept::Connection(Accepted {
            stream: Box::new(stream),
            peer: PeerIdentity {
                pid,
                // Pinned at accept (see `PeerIdentity::started`).
                started: pid.and_then(crate::peer::process_started),
            },
        }))
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// The peer-authentication decision: the peer's user is this user's.
pub(crate) fn same_user(peer_uid: u32, my_uid: u32) -> bool {
    peer_uid == my_uid
}

/// The one-daemon-per-user lock: an exclusive `flock` on `<run dir>/daemon.lock`.
pub struct Lock {
    _file: File,
}

impl Lock {
    pub async fn acquire(scope: &Scope) -> Result<Lock, BindError> {
        check_private(&scope.dir)?;
        // Not through a link: the directory is private, but a link left in it is refused.
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(scope.dir.join("daemon.lock"))?;
        // SAFETY: `file` is an open descriptor owned by this scope.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::WouldBlock {
                Err(BindError::AlreadyRunning)
            } else {
                Err(BindError::Io(e))
            };
        }
        Ok(Lock { _file: file })
    }
}

fn not_running_or(e: io::Error) -> ConnectError {
    match e.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => ConnectError::NotRunning,
        _ => ConnectError::Io(e),
    }
}

/// The directory must be this user's before a client trusts a socket in it.
fn check_dir_for_client(dir: &Path) -> Result<(), ConnectError> {
    match check_private(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(ConnectError::NotRunning),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => Err(ConnectError::Foreign),
        Err(e) => Err(ConnectError::Io(e)),
    }
}

pub async fn connect_async(
    scope: &Scope,
    label: &str,
) -> Result<Box<dyn AsyncStream>, ConnectError> {
    check_dir_for_client(&scope.dir)?;
    let stream = UnixStream::connect(scope.socket(label)?)
        .await
        .map_err(not_running_or)?;
    let cred = stream.peer_cred()?;
    if !same_user(cred.uid(), euid()) {
        return Err(ConnectError::Foreign);
    }
    Ok(Box::new(stream))
}

/// The user id of the process serving `fd`.
#[cfg(target_os = "linux")]
fn server_uid(fd: std::os::fd::RawFd) -> io::Result<u32> {
    // SAFETY: ucred is plain data; zero is a valid starting value.
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` are locals of the sizes passed.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(cred.uid)
}

/// The user id of the process serving `fd`.
#[cfg(not(target_os = "linux"))]
fn server_uid(fd: std::os::fd::RawFd) -> io::Result<u32> {
    let (mut uid, mut gid) = (0 as libc::uid_t, 0 as libc::gid_t);
    // SAFETY: `uid` and `gid` are locals.
    if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(uid)
}

pub fn connect_blocking(scope: &Scope, label: &str) -> Result<Blocking, ConnectError> {
    check_dir_for_client(&scope.dir)?;
    let stream = StdStream::connect(scope.socket(label)?).map_err(not_running_or)?;
    if !same_user(server_uid(stream.as_raw_fd())?, euid()) {
        return Err(ConnectError::Foreign);
    }
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn the_run_dir_is_xdg_oac_else_home_dot_oac_run() {
        let o = |s: &str| OsString::from(s);
        assert_eq!(
            run_dir_from(Some(&o("/run/user/1000")), Some(&o("/home/u"))),
            Some(PathBuf::from("/run/user/1000/oac"))
        );
        // Unset, empty or relative XDG_RUNTIME_DIR falls back to the home directory.
        for xdg in [None, Some(o("")), Some(o("relative/dir"))] {
            assert_eq!(
                run_dir_from(xdg.as_deref(), Some(&o("/home/u"))),
                Some(PathBuf::from("/home/u/.oac/run"))
            );
        }
        assert_eq!(run_dir_from(None, None), None);
        assert_eq!(run_dir_from(Some(&o("")), Some(&o(""))), None);
    }

    #[test]
    fn only_the_daemons_own_user_is_accepted() {
        assert!(same_user(1000, 1000));
        assert!(!same_user(0, 1000), "not even root");
        assert!(!same_user(1001, 1000));
    }
}
