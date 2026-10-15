// SPDX-License-Identifier: Apache-2.0

//! The local IPC between the daemon and the processes that talk to it, with the peer
//! authenticated by the operating system (G9, #70; `docs/planning/decisions/
//! C2-process-model.md` §4; `docs/planning/v0.1/06-security.md` §14 row 13; lead decisions D1
//! and D2, 2026-10-14).
//!
//! One **endpoint per harness label** (`claude`, `codex`): the label of the registration
//! record is the label of the endpoint a connection was accepted on
//! (`oac_core::session_binding::PeerObservation`). All endpoints of a user live in one
//! **scope**, the user's own namespace:
//!
//! - **Windows:** named pipes `\\.\pipe\oac-<user SID>-<label>`. Each pipe is created with a
//!   security descriptor whose DACL grants one ACE, to the daemon's own user SID: no
//!   `Everyone`, no anonymous, no other account, which the default descriptor of
//!   `CreateNamedPipeW` would admit (C2 §4). The first instance is created with
//!   `FILE_FLAG_FIRST_PIPE_INSTANCE`, so a pipe of that name that already exists is never
//!   joined, and remote clients are rejected. After each connection the daemon asks the
//!   operating system which process is at the other end (`GetNamedPipeClientProcessId`) and
//!   checks that its token's user is the daemon's own: belt and braces over the DACL.
//! - **Unix:** `AF_UNIX` stream sockets `<run dir>/<label>.sock`, with the run directory
//!   `$XDG_RUNTIME_DIR/oac` or else `~/.oac/run`, created `0700` and refused unless it is a
//!   real directory owned by the user and closed to everyone else; the socket is `0600`.
//!   After each connection the daemon reads the peer's credentials from the kernel
//!   (`SO_PEERCRED`, or `LOCAL_PEERCRED` on macOS) and refuses a peer whose user id is not
//!   its own.
//!
//! **Clients check the server too.** A named pipe name is global, so another account could
//! create `oac-<your SID>-claude` before the daemon exists and wait for a shim to send it
//! the session's traffic. A client therefore checks that the process serving the endpoint
//! belongs to its own user before it sends a byte ([`ConnectError::Foreign`]), and on
//! Windows opens the pipe at the identification impersonation level, so even a
//! same-user squatter cannot act as the client. On Unix the run directory's ownership and
//! mode do the same job, with the socket's credentials checked as well.
//!
//! **One daemon per user.** [`DaemonLock`] is taken before the daemon reads its key: on
//! Windows by creating the first instance of a lock pipe, on Unix by holding `flock` on
//! `<run dir>/daemon.lock`. A process that already holds it makes the next one
//! [`BindError::AlreadyRunning`]. The kernel drops it when the holder dies, so a crash
//! leaves nothing stale.
//!
//! What this authenticates is the **user** of the peer, never its trustworthiness: a
//! process of the same user is inside the boundary, and what it may do is the core's
//! decision (default deny; authenticated but untrusted, `oac-security-work` §3).

use std::fmt;
use std::io;

use tokio::io::{AsyncRead, AsyncWrite};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as platform;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;

#[cfg(unix)]
pub use unix::run_dir_from;

/// A byte stream a connection runs over, from either platform.
pub trait AsyncStream: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin> AsyncStream for T {}

/// Why an endpoint could not be bound or a lock taken.
#[derive(Debug)]
pub enum BindError {
    /// Another daemon of this user holds it (or something else holds the name).
    AlreadyRunning,
    /// The operating system refused.
    Io(io::Error),
}

impl fmt::Display for BindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BindError::AlreadyRunning => {
                f.write_str("another daemon of this user is running, or the endpoint name is taken")
            }
            BindError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BindError {}

impl From<io::Error> for BindError {
    fn from(e: io::Error) -> BindError {
        BindError::Io(e)
    }
}

/// Why a client could not reach the daemon.
#[derive(Debug)]
pub enum ConnectError {
    /// Nothing is serving the endpoint: the daemon is not running.
    NotRunning,
    /// Something serves the endpoint, and it is not this user's: nothing was sent.
    Foreign,
    /// Every instance stayed busy for the whole wait.
    Busy,
    /// The operating system refused.
    Io(io::Error),
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectError::NotRunning => f.write_str("the daemon is not running"),
            ConnectError::Foreign => {
                f.write_str("the endpoint is served by a process of another user; nothing was sent")
            }
            ConnectError::Busy => f.write_str("the daemon is busy"),
            ConnectError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ConnectError {}

impl From<io::Error> for ConnectError {
    fn from(e: io::Error) -> ConnectError {
        ConnectError::Io(e)
    }
}

/// A user's namespace for the daemon's endpoints and lock.
#[derive(Clone, Debug)]
pub struct Scope(platform::Scope);

impl Scope {
    /// The scope of the user this process runs as. On Unix this creates the run directory
    /// when it is missing, and refuses one that is not private.
    ///
    /// # Errors
    ///
    /// The operating system's error, or [`io::ErrorKind::PermissionDenied`] for a run
    /// directory another user could enter.
    pub fn for_user() -> io::Result<Scope> {
        platform::Scope::for_user().map(Scope)
    }

    /// A scope of its own, not the user's, for tests and diagnostics: fresh endpoints that
    /// no other scope shares. On Unix its directory is removed when the last clone drops.
    ///
    /// # Errors
    ///
    /// The operating system's error.
    pub fn unique() -> io::Result<Scope> {
        platform::Scope::unique().map(Scope)
    }

    /// The endpoint for harness `label` (lower-case letters and digits, starting with a
    /// letter; `lock` is reserved).
    pub fn endpoint(&self, label: &str) -> Endpoint {
        assert!(label_is_valid(label), "an endpoint label is [a-z][a-z0-9]*");
        Endpoint {
            scope: self.clone(),
            label: label.to_owned(),
        }
    }

    /// Takes the one-daemon-per-user lock. Hold the returned guard for the daemon's life.
    ///
    /// # Errors
    ///
    /// [`BindError::AlreadyRunning`] when it is held.
    pub async fn acquire_lock(&self) -> Result<DaemonLock, BindError> {
        platform::Lock::acquire(&self.0).await.map(DaemonLock)
    }
}

/// True for a label an endpoint can carry.
pub fn label_is_valid(label: &str) -> bool {
    let mut chars = label.chars();
    label != "lock"
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && label.len() <= 32
}

/// One endpoint: a scope and a harness label.
#[derive(Clone, Debug)]
pub struct Endpoint {
    scope: Scope,
    label: String,
}

impl Endpoint {
    /// The harness label of the endpoint.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The endpoint's name for a diagnostic: the pipe name on Windows, the socket path on
    /// Unix.
    pub fn display_name(&self) -> String {
        self.scope.0.endpoint_name(&self.label)
    }

    /// Starts serving the endpoint.
    ///
    /// # Errors
    ///
    /// [`BindError::AlreadyRunning`] when something already serves it.
    pub async fn bind(&self) -> Result<Listener, BindError> {
        platform::Listener::bind(&self.scope.0, &self.label)
            .await
            .map(Listener)
    }

    /// Connects, for a process that talks and listens at once (the shim). The server is
    /// checked before the stream is returned.
    ///
    /// # Errors
    ///
    /// See [`ConnectError`].
    pub async fn connect(&self) -> Result<Box<dyn AsyncStream>, ConnectError> {
        platform::connect_async(&self.scope.0, &self.label).await
    }

    /// Connects with blocking I/O, for a process that only writes (the hook handler) or
    /// only probes (`oac start`). On Windows the handle is synchronous: do not read and
    /// write it from two threads at once (the system serializes them); use
    /// [`Endpoint::connect`] for a relay.
    ///
    /// # Errors
    ///
    /// See [`ConnectError`].
    pub fn connect_blocking(&self) -> Result<ClientStream, ConnectError> {
        platform::connect_blocking(&self.scope.0, &self.label).map(ClientStream)
    }
}

/// A served endpoint.
pub struct Listener(platform::Listener);

impl Listener {
    /// Waits for the next connection and authenticates its peer. A refused peer is
    /// [`Accept::Rejected`] and the listener goes on.
    ///
    /// # Errors
    ///
    /// The operating system's error when the listener itself can no longer accept.
    pub async fn accept(&mut self) -> io::Result<Accept> {
        self.0.accept().await
    }
}

/// What was observed about the process at the other end, by the operating system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerIdentity {
    /// The peer's process id, when the operating system reports one. It is the input of
    /// `crate::peer::observe`.
    pub pid: Option<u32>,
}

/// An authenticated connection.
pub struct Accepted {
    /// The byte stream.
    pub stream: Box<dyn AsyncStream>,
    /// Who is at the other end.
    pub peer: PeerIdentity,
}

/// A connection refused at authentication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rejection {
    /// Why, in words that hold nothing of the peer's.
    pub reason: &'static str,
}

/// The outcome of [`Listener::accept`].
pub enum Accept {
    /// A peer of this user.
    Connection(Accepted),
    /// A peer that failed authentication; nothing was read from it.
    Rejected(Rejection),
}

/// The one-daemon-per-user lock; released when dropped.
pub struct DaemonLock(#[allow(dead_code)] platform::Lock);

/// A blocking client stream.
pub struct ClientStream(platform::Blocking);

impl io::Read for ClientStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        io::Read::read(&mut self.0, buf)
    }
}

impl io::Write for ClientStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        io::Write::write(&mut self.0, buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(&mut self.0)
    }
}
