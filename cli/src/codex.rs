// SPDX-License-Identifier: Apache-2.0

//! The Codex carrier: the daemon opens the byte stream to Codex's app-server control socket and
//! gives it to the Codex adapter (G8 slice S5, #433 boxes 1-2, #69 box 2;
//! `docs/planning/decisions/G-7-stage4-dependencies.md` §8.3: `cli/` opens the stream, the
//! adapter owns everything above bytes).
//!
//! **What this module reads.** The control socket's path, from the explicit override
//! ([`SOCKET_OVERRIDE_VARIABLE`]) or the Codex home default; metadata of that one path, to
//! tell an absent socket from a refused connection; and the connection itself. **Nothing else
//! under the Codex home**: no credential file, no database, no rollout, no configuration
//! (ADR-001 Boundary; `scripts/check-doctor-boundary.mjs` allows this file to name the Codex
//! home on two lines, and nothing else in `cli/src`).
//!
//! **The path** is `<Codex home>/app-server-control/app-server-control.sock`, where the Codex
//! home is the environment's own variable when set, else Codex's default directory under the
//! user's home directory (`codex app-server daemon version` reports the same path).
//!
//! **Authenticating the server.** The control socket is an `AF_UNIX` stream socket on every
//! platform, Windows included, and a connection is refused unless its server belongs to the
//! user this daemon runs as:
//!
//! - Unix: `SO_PEERCRED` (Linux) or `getpeereid` and `LOCAL_PEERPID` (macOS) give the server's
//!   user id and process id; the user id must be ours.
//! - Windows: neither std nor tokio opens an `AF_UNIX` socket there, so this uses Winsock
//!   through `windows-sys` (the `Win32_Networking_WinSock` feature, approved by the lead
//!   2026-10-21). The server's process id comes from `WSAIoctl` with
//!   `SIO_AF_UNIX_GETPEERPID`, and that process's token owner must be this user's
//!   (`crate::peer::process_user_matches`, pinned by the process's start time, read before the
//!   check and used again for the observation, so a recycled id is not mistaken for it; an
//!   id or start time that cannot be read is [`OpenError::Unverified`], not "another user").
//!   **UNVERIFIED in first-party documentation:**
//!   the Microsoft Learn pages for `WSAIoctl`, the Winsock IOCTL list and the 2018 `AF_UNIX`
//!   announcement do not mention this control code, and the announcement says the Windows
//!   implementation has no ancillary data. The constant is defined in the Windows SDK
//!   (`afunix.h`) and so in the `windows-sys` bindings generated from Microsoft's own
//!   metadata, and this crate's test runs it against a real Winsock server and checks the
//!   process id it returns. The call is used for **no more than** the same-user check and the
//!   pairing key; an id that cannot be read fails closed (no carrier).
//!
//! The process is then observed as the Codex harness process (`crate::peer::observe_process`
//! under the label `codex`), so that its pairing key is compared with that of each shim's
//! nearest `codex` ancestor ([MCPB-ATT-022]). A server whose executable is not named `codex`
//! has no key, and no shim pairs with it (availability only, never authority).
//!
//! **The carrier's life (`docs/planning/v0.1/11-risks.md` row 90).** The adapter serves one
//! carrier at a time and the daemon never starts, stops or restarts Codex's app-server
//! (Q11). [`CarrierDriver::ensure`] runs when the daemon starts and **each time a Codex shim
//! connects**, and never on a timer: if the adapter has no carrier, or has lost it, the driver
//! opens the socket, designates the connection as the carrier and gives it to the core
//! (`Pipelines::connect_carrier`), in that order and before the shim is handed over (with no
//! carrier a shim connection is closed; while one connects, shims wait). When the socket is
//! not there the shim connection is simply closed by the adapter, and the next shim tries
//! again.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use oac_adapter_codex::pairing::{Config, LogFn};
use oac_adapter_codex::{
    CarrierState, CodexAdapter, DEFAULT_REGISTERED_NAME, Options, RegisteredNameError,
    validate_registered_name,
};
use oac_core::pipeline::{AdapterId, Pipelines};
use oac_core::session_binding::PeerObservation;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::runtime::Handle;

use crate::daemon::LogSink;
use crate::ipc::AsyncStream;
use crate::peer::{self, ProcessId, SystemTable};
use crate::relay;

/// The harness label of the Codex endpoint and of the carrier's observation.
pub const LABEL: &str = "codex";

/// The environment variable that overrides the control socket's path: for tests, and for a
/// host whose Codex keeps its socket elsewhere. Only where to connect changes; the server is
/// still authenticated.
pub const SOCKET_OVERRIDE_VARIABLE: &str = "OAC_CODEX_CONTROL_SOCKET";

/// The environment variable that sets the registered name: the name the operator gave `codex mcp
/// add <name> -- oac mcp-shim --harness codex`, which Codex reports as the `server` of each of
/// the shim's calls. The Codex pairing recognises a call only by it
/// (`spec/bindings/mcp.md` §4.5.2), so the two must be equal. Unset or empty: the default,
/// [`DEFAULT_REGISTERED_NAME`] (`oac`). Like [`SOCKET_OVERRIDE_VARIABLE`], an environment
/// variable (`08-cli-and-deployment.md` §4: flag > env > file > default; `oac start` has no
/// flag for it).
pub const REGISTERED_NAME_VARIABLE: &str = "OAC_CODEX_REGISTERED_NAME";

const HOME_VARIABLE: &str = "CODEX_HOME";
const HOME_DIRECTORY: &str = ".codex";
const SOCKET_DIRECTORY: &str = "app-server-control";
const SOCKET_FILE: &str = "app-server-control.sock";

/// How long the daemon waits for the control socket to connect and be authenticated.
pub const OPEN_WAIT: Duration = Duration::from_secs(10);

/// The bytes the stream pump holds in each direction.
const PUMP_BUFFER: usize = 64 * 1024;

/// The control socket's path for the given override, Codex-home variable and user home.
/// An empty value counts as unset. `None` when none of them gives a path.
pub fn control_socket_path(
    explicit: Option<&OsStr>,
    home_variable: Option<&OsStr>,
    user_home: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(path) = explicit.filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let home = match home_variable.filter(|v| !v.is_empty()) {
        Some(h) => PathBuf::from(h),
        None => user_home?.join(HOME_DIRECTORY),
    };
    Some(home.join(SOCKET_DIRECTORY).join(SOCKET_FILE))
}

/// [`control_socket_path`] for this process's environment.
pub fn default_control_socket_path() -> Option<PathBuf> {
    control_socket_path(
        std::env::var_os(SOCKET_OVERRIDE_VARIABLE).as_deref(),
        std::env::var_os(HOME_VARIABLE).as_deref(),
        std::env::home_dir().as_deref(),
    )
}

/// Why the registered name setting was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum RegisteredNameSettingError {
    /// The value is not text.
    NotText,
    /// The value is not a name Codex accepts for an MCP server.
    Invalid(RegisteredNameError),
}

impl std::fmt::Display for RegisteredNameSettingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegisteredNameSettingError::NotText => {
                write!(f, "{REGISTERED_NAME_VARIABLE} is not valid text")
            }
            RegisteredNameSettingError::Invalid(why) => {
                write!(f, "{REGISTERED_NAME_VARIABLE}: {why}")
            }
        }
    }
}

/// The registered name for the given value of [`REGISTERED_NAME_VARIABLE`]: the default when it
/// is unset or empty, else the value if it is a name Codex accepts for an MCP server.
///
/// # Errors
///
/// The value is not text, is longer than `oac_adapter_codex::MAX_REGISTERED_NAME`, or holds a
/// character Codex does not accept in a server name. The error text does not repeat the value.
pub fn registered_name(value: Option<&OsStr>) -> Result<String, RegisteredNameSettingError> {
    let Some(value) = value.filter(|v| !v.is_empty()) else {
        return Ok(DEFAULT_REGISTERED_NAME.to_owned());
    };
    let name = value.to_str().ok_or(RegisteredNameSettingError::NotText)?;
    validate_registered_name(name).map_err(RegisteredNameSettingError::Invalid)?;
    Ok(name.to_owned())
}

/// [`registered_name`] for this process's environment.
///
/// # Errors
///
/// As [`registered_name`].
pub fn registered_name_from_environment() -> Result<String, RegisteredNameSettingError> {
    registered_name(std::env::var_os(REGISTERED_NAME_VARIABLE).as_deref())
}

/// The Codex adapter for the server registered with Codex as `registered_name`, whose pairing
/// diagnostics go to the daemon's `log`. Both daemon wirings (`oac start` and the live-leg
/// example daemon) build it here.
///
/// # Errors
///
/// `registered_name` is not a name Codex accepts.
pub fn adapter(
    registered_name: &str,
    log: &LogSink,
) -> Result<CodexAdapter, RegisteredNameSettingError> {
    let sink = log.clone();
    let line: LogFn = Arc::new(move |text: &str| sink.line(text));
    let config = Config::try_new(registered_name)
        .map_err(RegisteredNameSettingError::Invalid)?
        .with_log(line);
    Ok(CodexAdapter::with_config(config, Options::default()))
}

/// Why the control socket was not opened. The text holds no path and nothing the server sent.
#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    /// There is no socket where it would be: Codex's app-server daemon is not running.
    Absent,
    /// Something is there, and the connection failed.
    Unreachable(String),
    /// The server's owner was established and is another user.
    NotThisUser,
    /// The server's owner could not be established (its user or process id could not be
    /// read). The same outcome on every platform: no carrier, a warning in doctor.
    Unverified,
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Absent => f.write_str("there is no Codex control socket"),
            OpenError::Unreachable(why) => write!(f, "the Codex control socket: {why}"),
            OpenError::NotThisUser => {
                f.write_str("the Codex control socket is not served by a process of this user")
            }
            OpenError::Unverified => {
                f.write_str("could not verify the owner of the Codex control socket's server")
            }
        }
    }
}

impl std::error::Error for OpenError {}

/// An opened, authenticated carrier stream and what the daemon observed about its server.
pub struct OpenedCarrier {
    /// The byte stream (see [`blocking_stream`]).
    pub stream: Box<dyn AsyncStream>,
    /// The server process as the Codex harness process: its pairing key, when it has one.
    pub observation: PeerObservation,
}

/// Opens a carrier. **Blocking**: the daemon runs it off its runtime, bounded by
/// [`OPEN_WAIT`]. Given to the daemon so that a test can open a carrier to a fake with a
/// synthetic observation.
pub type CarrierOpener = Arc<dyn Fn(&Handle) -> Result<OpenedCarrier, OpenError> + Send + Sync>;

/// The Codex adapter and what opens its carrier: the daemon's wiring for the `codex` endpoint.
#[derive(Clone)]
pub struct CarrierWiring {
    /// The adapter. It must be the one given in `Parts::harnesses`.
    pub adapter: Arc<CodexAdapter>,
    /// What opens the carrier.
    pub opener: CarrierOpener,
}

/// The opener of the real control socket at `path`.
pub fn control_socket_opener(path: PathBuf) -> CarrierOpener {
    Arc::new(move |runtime| {
        let socket = platform::connect(&path)?;
        let observation = observe_server(socket.process.as_ref());
        Ok(OpenedCarrier {
            stream: blocking_stream(runtime, socket.reader, socket.writer, socket.close),
            observation,
        })
    })
}

/// The server process as the Codex harness process, or the label alone. The process is the
/// one pinned (id and start time) when its owner was checked.
fn observe_server(process: Option<&ProcessId>) -> PeerObservation {
    match (process, SystemTable::new()) {
        (Some(process), Ok(table)) => peer::observe_process(&table, LABEL, process),
        _ => PeerObservation {
            harness_label: oac_core::ids::Token::parse(LABEL),
            ..PeerObservation::none()
        },
    }
}

/// An authenticated blocking connection to the control socket.
pub struct Socket {
    /// Reads the server's bytes; `Ok(0)` is the end of the stream, and also what a blocked
    /// read returns once `close` has been called.
    pub reader: Box<dyn Read + Send>,
    /// Writes to the server.
    pub writer: Box<dyn Write + Send>,
    /// Ends the connection and wakes a read blocked in another thread.
    pub close: Box<dyn Fn() + Send + Sync>,
    /// The server process, pinned by its start time, when the platform gives its id.
    pub process: Option<ProcessId>,
}

/// Opens the control socket at `path` as a blocking [`Socket`], authenticating its server as
/// this user's.
///
/// # Errors
///
/// As [`OpenError`].
pub fn open_blocking(path: &Path) -> Result<Socket, OpenError> {
    platform::connect(path)
}

/// A blocking stream as an async one, for [`relay::attach`]: two threads move bytes between
/// the stream and a bounded in-memory pipe. `close` shuts the stream down when the relay is
/// done with it (or the pump's other thread is).
///
/// End of the stream reads as end of file on the returned side, after everything it sent.
pub fn blocking_stream(
    runtime: &Handle,
    mut reader: Box<dyn Read + Send>,
    mut writer: Box<dyn Write + Send>,
    close: Box<dyn Fn() + Send + Sync>,
) -> Box<dyn AsyncStream> {
    let (near, far) = tokio::io::duplex(PUMP_BUFFER);
    let (mut from_relay, mut to_relay) = tokio::io::split(far);
    let close = Arc::new(close);
    let inbound_close = close.clone();
    let rt = runtime.clone();
    let _ = std::thread::Builder::new()
        .name("oac-codex-in".into())
        .spawn(move || {
            let mut chunk = vec![0u8; 8192];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if rt.block_on(to_relay.write_all(&chunk[..n])).is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = rt.block_on(to_relay.shutdown());
            inbound_close();
        });
    let rt = runtime.clone();
    let _ = std::thread::Builder::new()
        .name("oac-codex-out".into())
        .spawn(move || {
            let mut chunk = vec![0u8; 8192];
            loop {
                match rt.block_on(from_relay.read(&mut chunk)) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if writer.write_all(&chunk[..n]).is_err() || writer.flush().is_err() {
                            break;
                        }
                    }
                }
            }
            close();
        });
    Box::new(near)
}

/// The process id of the server on the control socket at `path`, when the platform gives
/// it: connects, authenticates the server as this user's, and closes at once, sending and
/// reading nothing. For diagnostics and tests.
///
/// # Errors
///
/// As [`OpenError`].
pub fn control_socket_server(path: &Path) -> Result<Option<u32>, OpenError> {
    let socket = platform::connect(path)?;
    let pid = socket.process.map(|p| p.pid);
    (socket.close)();
    Ok(pid)
}

/// What the control socket looks like to a read-only probe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SocketProbe {
    /// No path could be worked out (no home directory).
    NoPath,
    /// No socket at the path.
    Absent,
    /// A connection was accepted and the server is a process of this user.
    Reachable,
    /// The server is another user's process (confirmed).
    OtherUser,
    /// The server's owner could not be verified.
    Unverified,
    /// A socket is there and the connection failed (a stale socket, or a server that is
    /// not accepting).
    Unreachable(String),
}

/// Connects to the control socket at `path` and closes the connection at once: **sends and
/// reads nothing**. This is what `oac doctor` asks.
pub fn probe_control_socket(path: Option<&Path>) -> SocketProbe {
    let Some(path) = path else {
        return SocketProbe::NoPath;
    };
    match platform::connect(path) {
        Ok(socket) => {
            (socket.close)();
            SocketProbe::Reachable
        }
        Err(OpenError::Absent) => SocketProbe::Absent,
        Err(OpenError::NotThisUser) => SocketProbe::OtherUser,
        Err(OpenError::Unverified) => SocketProbe::Unverified,
        Err(OpenError::Unreachable(why)) => SocketProbe::Unreachable(why),
    }
}

/// What [`CarrierDriver::ensure`] came to, for a log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ensured {
    /// A carrier was up or connecting already.
    Present,
    /// A carrier was opened and given to the core.
    Connected,
    /// The control socket is not there.
    Absent,
    /// It could not be opened or authenticated, or the core refused it.
    Failed,
}

/// Connects the adapter's carrier when there is none (see the module documentation).
pub struct CarrierDriver {
    adapter: Arc<CodexAdapter>,
    id: AdapterId,
    pipelines: Arc<Pipelines>,
    opener: CarrierOpener,
    runtime: Handle,
    log: LogSink,
    serial: tokio::sync::Mutex<()>,
    /// Set when the daemon shuts down: an attempt in flight is abandoned, not waited for.
    stopped: tokio::sync::watch::Receiver<bool>,
}

impl CarrierDriver {
    /// A driver for `adapter`, registered with the core as `id`.
    pub fn new(
        wiring: CarrierWiring,
        id: AdapterId,
        pipelines: Arc<Pipelines>,
        runtime: Handle,
        log: LogSink,
        stopped: tokio::sync::watch::Receiver<bool>,
    ) -> Arc<CarrierDriver> {
        Arc::new(CarrierDriver {
            adapter: wiring.adapter,
            id,
            pipelines,
            opener: wiring.opener,
            runtime,
            log,
            serial: tokio::sync::Mutex::new(()),
            stopped,
        })
    }

    /// Makes sure the adapter has a carrier, opening one if it has none or has lost it. Called
    /// at start and for each Codex shim connection, **before** the shim is given to the core.
    /// Calls are serialized, so shims that arrive together open one carrier.
    pub async fn ensure(self: &Arc<Self>) -> Ensured {
        let mut stopped = self.stopped.clone();
        if *stopped.borrow() {
            return Ensured::Failed;
        }
        // Shutdown abandons the attempt (the blocking open finishes on its own thread and
        // its stream is dropped) rather than waiting for it: up to `OPEN_WAIT` otherwise.
        tokio::select! {
            ensured = self.ensure_once() => ensured,
            _ = stopped.changed() => Ensured::Failed,
        }
    }

    async fn ensure_once(self: &Arc<Self>) -> Ensured {
        let _one = self.serial.lock().await;
        match self.adapter.carrier_state() {
            CarrierState::Up | CarrierState::Connecting => return Ensured::Present,
            CarrierState::None | CarrierState::Lost => {}
        }
        let (opener, runtime) = (self.opener.clone(), self.runtime.clone());
        let opening = tokio::task::spawn_blocking(move || opener(&runtime));
        let opened = match tokio::time::timeout(OPEN_WAIT, opening).await {
            Ok(Ok(Ok(opened))) => opened,
            Ok(Ok(Err(OpenError::Absent))) => {
                self.log
                    .line("no Codex control socket: Codex shim connections are not served");
                return Ensured::Absent;
            }
            Ok(Ok(Err(e))) => {
                self.log
                    .line(&format!("the Codex carrier was not opened: {e}"));
                return Ensured::Failed;
            }
            Ok(Err(_)) | Err(_) => {
                self.log
                    .line("the Codex carrier was not opened: the attempt did not finish");
                return Ensured::Failed;
            }
        };
        let had_key = opened.observation.pairing_key.is_some();
        let (pipelines, runtime) = (self.pipelines.clone(), self.runtime.clone());
        let connection = relay::attach(
            &self.runtime,
            opened.stream,
            Box::new(move |handle| {
                // The carrier's stream ended: the core frees its place (and forgets it as a
                // carrier). Nothing reconnects until the next Codex shim connects.
                let handle = handle.clone();
                runtime.spawn_blocking(move || pipelines.disconnect(&handle));
            }),
        );
        let handle = connection.handle().clone();
        // The order the adapter requires: designate, then give the connection over.
        if !self.adapter.designate_carrier(&handle) {
            connection.close();
            self.log
                .line("the Codex adapter refused a carrier designation");
            return Ensured::Failed;
        }
        let (pipelines, id, observation) = (self.pipelines.clone(), self.id, opened.observation);
        let given = tokio::task::spawn_blocking(move || {
            pipelines.connect_carrier(id, connection, observation)
        })
        .await;
        match given {
            Ok(Ok(())) => {
                let name = self.adapter.registered_name();
                self.log.line(&if had_key {
                    format!("the Codex carrier is connected (registered name {name})")
                } else {
                    format!(
                        "the Codex carrier is connected without a pairing key: no shim can \
                         pair (registered name {name})"
                    )
                });
                Ensured::Connected
            }
            Ok(Err(e)) => {
                self.log
                    .line(&format!("the core did not take the Codex carrier: {e}"));
                Ensured::Failed
            }
            Err(_) => {
                self.log
                    .line("the Codex carrier could not be handed to the core");
                Ensured::Failed
            }
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::io;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::path::Path;

    use super::{OpenError, Socket};
    use crate::peer::{self, ProcessId};

    fn euid() -> u32 {
        // SAFETY: geteuid has no preconditions, cannot fail and touches no memory of ours.
        unsafe { libc::geteuid() }
    }

    /// The user id and, when the platform gives it, the process id of the server on `fd`.
    #[cfg(target_os = "linux")]
    fn server(fd: std::os::fd::RawFd) -> io::Result<(u32, Option<u32>)> {
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
        Ok((cred.uid, u32::try_from(cred.pid).ok().filter(|p| *p != 0)))
    }

    /// macOS: `getpeereid` for the user, `LOCAL_PEERPID` (0x002 at level `SOL_LOCAL`, 0) for
    /// the process.
    #[cfg(target_os = "macos")]
    fn server(fd: std::os::fd::RawFd) -> io::Result<(u32, Option<u32>)> {
        let (mut uid, mut gid) = (0 as libc::uid_t, 0 as libc::gid_t);
        // SAFETY: `uid` and `gid` are locals.
        if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut pid: libc::pid_t = 0;
        let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
        // SAFETY: `pid` and `len` are locals of the sizes passed.
        let rc = unsafe {
            libc::getsockopt(
                fd,
                0,
                0x002,
                (&mut pid as *mut libc::pid_t).cast(),
                &mut len,
            )
        };
        let pid = (rc == 0).then_some(pid).and_then(|p| u32::try_from(p).ok());
        Ok((uid, pid.filter(|p| *p != 0)))
    }

    /// Other Unixes: the user only. There is no process id here, so no pairing key (never a
    /// guessed one).
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn server(fd: std::os::fd::RawFd) -> io::Result<(u32, Option<u32>)> {
        let (mut uid, mut gid) = (0 as libc::uid_t, 0 as libc::gid_t);
        // SAFETY: `uid` and `gid` are locals.
        if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((uid, None))
    }

    pub fn connect(path: &Path) -> Result<Socket, OpenError> {
        if let Err(e) = std::fs::symlink_metadata(path)
            && e.kind() == io::ErrorKind::NotFound
        {
            return Err(OpenError::Absent);
        }
        let stream = UnixStream::connect(path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => OpenError::Absent,
            _ => OpenError::Unreachable(format!("connect failed ({:?})", e.kind())),
        })?;
        let (uid, pid) = server(stream.as_raw_fd()).map_err(|_| OpenError::Unverified)?;
        if uid != euid() {
            return Err(OpenError::NotThisUser);
        }
        // The start time pins the process the id names now.
        let process = pid
            .and_then(|pid| peer::process_started(pid).map(|started| ProcessId { pid, started }));
        let reader = stream
            .try_clone()
            .map_err(|e| OpenError::Unreachable(format!("{:?}", e.kind())))?;
        let writer = stream
            .try_clone()
            .map_err(|e| OpenError::Unreachable(format!("{:?}", e.kind())))?;
        Ok(Socket {
            reader: Box::new(reader),
            writer: Box::new(writer),
            close: Box::new(move || {
                // Already disconnected is the usual answer when the peer closed first.
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }),
            process,
        })
    }
}

#[cfg(windows)]
mod platform {
    //! Winsock `AF_UNIX`, overlapped.
    //!
    //! Microsoft Learn, "SOL_SOCKET Socket Options", `SO_RCVTIMEO`: when a blocking receive
    //! times out, the connection is in an indeterminate state and should be closed, so a
    //! receive timeout is not used to wake a reader. The socket is created overlapped
    //! (`WSASocketW` with `WSA_FLAG_OVERLAPPED`); the reader issues `WSARecv` with an event
    //! and waits (`WSAWaitForMultipleEvents`) on that event and on a wake event the closer
    //! sets. A receive still pending when the reader is woken stays registered with the
    //! socket and is reaped when the socket is closed: `closesocket` cancels outstanding
    //! overlapped operations (Learn, `closesocket`, Remarks). The descriptor may be reused by
    //! the system as soon as it is closed, so nothing is asked of it afterwards: the reaper
    //! waits on the receive's own event, which is signalled when the cancelled operation
    //! completes, and only then frees the buffer and the `OVERLAPPED` (Learn, `closesocket`:
    //! resources are not to be cleaned up until the I/O requests have completed). If that wait
    //! fails the memory is leaked, never freed under a receive that might still write to it.
    //! The byte count is read with `WSAGetOverlappedResult` on the open socket (Learn,
    //! `WSARecv`: pass NULL for `lpNumberOfBytesRecvd` when an `OVERLAPPED` is used).
    use std::io::{self, Read, Write};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, Once};

    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Networking::WinSock::{
        AF_UNIX, SD_BOTH, SIO_AF_UNIX_GETPEERPID, SOCK_STREAM, SOCKADDR, SOCKADDR_UN, SOCKET,
        WSA_FLAG_NO_HANDLE_INHERIT, WSA_FLAG_OVERLAPPED, WSA_INFINITE, WSA_IO_PENDING,
        WSA_WAIT_EVENT_0, WSA_WAIT_FAILED, WSABUF, WSACloseEvent, WSACreateEvent, WSADATA,
        WSAGetLastError, WSAGetOverlappedResult, WSAIoctl, WSARecv, WSASetEvent, WSASocketW,
        WSAStartup, WSAWaitForMultipleEvents, closesocket, connect as winsock_connect, send,
        shutdown,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;

    use super::{OpenError, Socket};
    use crate::peer::{self, ProcessId};

    const INVALID: SOCKET = usize::MAX;
    /// The bytes one receive takes.
    const RECV_BYTES: usize = 8192;

    fn last_error(what: &str) -> io::Error {
        // SAFETY: no preconditions.
        io::Error::other(format!("{what} failed ({})", unsafe { WSAGetLastError() }))
    }

    /// One receive in flight: the buffer and the `OVERLAPPED` must not move or be freed until
    /// the operation has ended, so they are boxed and travel together.
    struct Pending {
        overlapped: Box<OVERLAPPED>,
        data: Box<[u8; RECV_BYTES]>,
        wsabuf: Box<WSABUF>,
        event: isize,
    }

    // SAFETY: the raw pointers inside point only into this value's own boxes, and the value
    // is used by one thread at a time (the reader, then whoever drops the socket).
    unsafe impl Send for Pending {}

    impl Drop for Pending {
        fn drop(&mut self) {
            // SAFETY: the event was made by WSACreateEvent and is closed once.
            unsafe { WSACloseEvent(self.event) };
        }
    }

    /// A Winsock socket, closed when the last reference goes. A receive left pending is
    /// reaped after the close.
    struct Owned {
        socket: SOCKET,
        pending: Mutex<Option<Pending>>,
    }

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: the socket is ours and is closed once; this cancels the pending receive.
            let closed = unsafe { closesocket(self.socket) } == 0;
            if let Some(p) = self
                .pending
                .get_mut()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                // Wait on the receive's own event, not on the closed descriptor.
                let event = p.event as HANDLE;
                // SAFETY: one live event handle, owned by `p`.
                // Only a socket that closed has cancelled its receive: otherwise the operation
                // may never complete, and is not waited for.
                let r = if closed {
                    unsafe { WSAWaitForMultipleEvents(1, &event, 1, WSA_INFINITE, 0) }
                } else {
                    WSA_WAIT_FAILED
                };
                if r == WSA_WAIT_EVENT_0 as u32 {
                    drop(p);
                } else {
                    // The operation may still write to its buffer: leak it rather than free it.
                    std::mem::forget(p);
                }
            }
        }
    }

    /// The event that wakes a blocked reader.
    struct Wake {
        event: isize,
        set: AtomicBool,
    }

    impl Drop for Wake {
        fn drop(&mut self) {
            // SAFETY: the event was made by WSACreateEvent and is closed once.
            unsafe { WSACloseEvent(self.event) };
        }
    }

    struct Reader {
        socket: Arc<Owned>,
        wake: Arc<Wake>,
        /// Received and not yet handed on.
        carry: Vec<u8>,
        carried: usize,
    }

    impl Reader {
        /// Receives into the carry; `Ok(false)` is the end of the stream (or a wake).
        fn receive(&mut self) -> io::Result<bool> {
            if self.wake.set.load(Ordering::SeqCst) {
                return Ok(false);
            }
            // A receive left pending by an earlier wake or failed wait is still in flight: a
            // second would be a second operation on the socket, and parking it would drop the
            // first. The slot is never overwritten.
            if self
                .socket
                .pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_some()
            {
                return Err(io::Error::other(
                    "a receive is still pending on this socket",
                ));
            }
            // SAFETY: plain event creation.
            let event = unsafe { WSACreateEvent() };
            if event == 0 {
                return Err(last_error("WSACreateEvent"));
            }
            // SAFETY: OVERLAPPED is plain data; zero is its documented starting value.
            let overlapped = Box::new(unsafe { std::mem::zeroed::<OVERLAPPED>() });
            let mut pending = Pending {
                overlapped,
                data: Box::new([0u8; RECV_BYTES]),
                wsabuf: Box::new(WSABUF {
                    len: RECV_BYTES as u32,
                    buf: std::ptr::null_mut(),
                }),
                event,
            };
            pending.wsabuf.buf = pending.data.as_mut_ptr();
            pending.overlapped.hEvent = event as HANDLE;
            let mut flags = 0u32;
            // SAFETY: `wsabuf`, `data` and `overlapped` are boxed and outlive the operation
            // (they move into the socket's pending slot if the reader is woken first).
            let rc = unsafe {
                WSARecv(
                    self.socket.socket,
                    &*pending.wsabuf,
                    1,
                    std::ptr::null_mut(),
                    &mut flags,
                    &mut *pending.overlapped,
                    None,
                )
            };
            // SAFETY: no preconditions.
            if rc != 0 && unsafe { WSAGetLastError() } != WSA_IO_PENDING {
                return Ok(false); // reset or closed: the end of the stream here
            }
            if rc != 0 {
                let events = [event as HANDLE, self.wake.event as HANDLE];
                // SAFETY: two live event handles.
                let r = unsafe { WSAWaitForMultipleEvents(2, events.as_ptr(), 0, WSA_INFINITE, 0) };
                if r == WSA_WAIT_EVENT_0 as u32 + 1 {
                    // Woken to close. Take a receive that finished at the same time.
                    // SAFETY: as above, a zero timeout on the receive's event only.
                    let done = unsafe { WSAWaitForMultipleEvents(1, events.as_ptr(), 0, 0, 0) };
                    if done != WSA_WAIT_EVENT_0 as u32 {
                        // Still pending: the socket reaps it when it is closed.
                        *self
                            .socket
                            .pending
                            .lock()
                            .unwrap_or_else(|e| e.into_inner()) = Some(pending);
                        return Ok(false);
                    }
                } else if r != WSA_WAIT_EVENT_0 as u32 {
                    // The wait failed with the receive still in flight: park it as above.
                    let error = last_error("WSAWaitForMultipleEvents");
                    *self
                        .socket
                        .pending
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()) = Some(pending);
                    return Err(error);
                }
            }
            let (mut n, mut f) = (0u32, 0u32);
            // SAFETY: the operation has completed (an immediate completion, or its event is set),
            // on a socket that is still open.
            let ok = unsafe {
                WSAGetOverlappedResult(self.socket.socket, &*pending.overlapped, &mut n, 0, &mut f)
            };
            if ok == 0 {
                return Ok(false); // aborted or reset: the end of the stream here
            }
            let received = n;
            if received == 0 {
                return Ok(false);
            }
            self.carry = pending.data[..received as usize].to_vec();
            self.carried = 0;
            Ok(true)
        }
    }

    impl Read for Reader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.carried >= self.carry.len() && !self.receive()? {
                return Ok(0);
            }
            let n = (self.carry.len() - self.carried).min(buf.len());
            buf[..n].copy_from_slice(&self.carry[self.carried..self.carried + n]);
            self.carried += n;
            Ok(n)
        }
    }

    struct Writer(Arc<Owned>);

    impl Write for Writer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let len = buf.len().min(i32::MAX as usize) as i32;
            // SAFETY: `buf` is a live buffer of at least `len` octets.
            let n = unsafe { send(self.0.socket, buf.as_ptr(), len, 0) };
            if n < 0 {
                return Err(last_error("send"));
            }
            Ok(n as usize)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn startup() -> Result<(), OpenError> {
        static ONCE: Once = Once::new();
        static OK: AtomicBool = AtomicBool::new(false);
        ONCE.call_once(|| {
            // SAFETY: WSADATA is plain data that the call fills; version 2.2.
            let mut data: WSADATA = unsafe { std::mem::zeroed() };
            OK.store(
                unsafe { WSAStartup(0x0202, &mut data) } == 0,
                Ordering::SeqCst,
            );
        });
        if OK.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(OpenError::Unreachable("Winsock did not start".to_owned()))
        }
    }

    /// The process id of the server on `s`, from `SIO_AF_UNIX_GETPEERPID` (see the module
    /// documentation of `crate::codex` for what is and is not documented about it). The byte
    /// count the call reports is not trusted (it is reported as 0 on some builds): the id
    /// is, when it is not 0.
    fn server_pid(s: SOCKET) -> Option<u32> {
        let mut pid: u32 = 0;
        let mut returned: u32 = 0;
        // SAFETY: `pid` and `returned` are locals of the sizes passed; no input, no overlapped.
        let rc = unsafe {
            WSAIoctl(
                s,
                SIO_AF_UNIX_GETPEERPID,
                std::ptr::null(),
                0,
                (&mut pid as *mut u32).cast(),
                std::mem::size_of::<u32>() as u32,
                &mut returned,
                std::ptr::null_mut(),
                None,
            )
        };
        (rc == 0 && pid != 0).then_some(pid)
    }

    pub fn connect(path: &Path) -> Result<Socket, OpenError> {
        startup()?;
        if let Err(e) = std::fs::symlink_metadata(path)
            && e.kind() == io::ErrorKind::NotFound
        {
            return Err(OpenError::Absent);
        }
        let text = path.to_str().ok_or_else(|| {
            OpenError::Unreachable("the socket path is not valid UTF-8".to_owned())
        })?;
        let bytes = text.as_bytes();
        // SAFETY: SOCKADDR_UN is plain data; zero is a valid starting value.
        let mut address: SOCKADDR_UN = unsafe { std::mem::zeroed() };
        if bytes.len() >= address.sun_path.len() {
            return Err(OpenError::Unreachable(
                "the socket path is too long for AF_UNIX".to_owned(),
            ));
        }
        address.sun_family = AF_UNIX;
        for (slot, b) in address.sun_path.iter_mut().zip(bytes) {
            *slot = *b as i8;
        }
        // SAFETY: plain Winsock call; the socket it returns is owned below.
        let s = unsafe {
            WSASocketW(
                i32::from(AF_UNIX),
                SOCK_STREAM,
                0,
                std::ptr::null(),
                0,
                WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT,
            )
        };
        if s == INVALID {
            return Err(OpenError::Unreachable("no AF_UNIX socket".to_owned()));
        }
        let owned = Arc::new(Owned {
            socket: s,
            pending: Mutex::new(None),
        });
        let length = (std::mem::size_of::<u16>() + bytes.len() + 1) as i32;
        // SAFETY: `address` is a live SOCKADDR_UN of at least `length` octets.
        let rc = unsafe {
            winsock_connect(
                s,
                (&address as *const SOCKADDR_UN).cast::<SOCKADDR>(),
                length,
            )
        };
        if rc != 0 {
            // SAFETY: no preconditions.
            let code = unsafe { WSAGetLastError() };
            return Err(OpenError::Unreachable(format!("connect failed ({code})")));
        }
        // The id first, then the start time that pins the process, then the owner check on
        // that same process: a recycled id is another process and fails the pin.
        let Some(pid) = server_pid(s) else {
            return Err(OpenError::Unverified);
        };
        let Some(started) = peer::process_started(pid) else {
            return Err(OpenError::Unverified);
        };
        if !peer::process_user_matches(pid, Some(started)) {
            return Err(OpenError::NotThisUser);
        }
        // SAFETY: plain event creation.
        let event = unsafe { WSACreateEvent() };
        if event == 0 {
            return Err(OpenError::Unreachable("no wake event".to_owned()));
        }
        let wake = Arc::new(Wake {
            event,
            set: AtomicBool::new(false),
        });
        let closer = wake.clone();
        let closing = owned.clone();
        Ok(Socket {
            reader: Box::new(Reader {
                socket: owned.clone(),
                wake,
                carry: Vec::new(),
                carried: 0,
            }),
            writer: Box::new(Writer(owned)),
            close: Box::new(move || {
                closer.set.store(true, Ordering::SeqCst);
                // SAFETY: the wake event is open while `closer` is referenced. A failure
                // leaves the reader blocked until the peer closes: nothing else can be done.
                unsafe { WSASetEvent(closer.event) };
                // SAFETY: the socket is open while `closing` is referenced. The usual failure
                // is "not connected" (the peer closed first), which is the state wanted.
                unsafe { shutdown(closing.socket, SD_BOTH) };
            }),
            process: Some(ProcessId { pid, started }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(s: &str) -> std::ffi::OsString {
        std::ffi::OsString::from(s)
    }

    fn tail() -> PathBuf {
        Path::new(SOCKET_DIRECTORY).join(SOCKET_FILE)
    }

    #[test]
    fn the_registered_name_is_the_default_unless_set_and_a_set_value_is_checked() {
        assert_eq!(registered_name(None), Ok("oac".to_owned()));
        assert_eq!(registered_name(Some(&os(""))), Ok("oac".to_owned()));
        assert_eq!(
            registered_name(Some(&os("oac-g8live"))),
            Ok("oac-g8live".to_owned())
        );
        for bad in ["a b", "a\nb", "caf\u{e9}", "a=b"] {
            assert_eq!(
                registered_name(Some(&os(bad))),
                Err(RegisteredNameSettingError::Invalid(
                    RegisteredNameError::BadCharacter
                )),
                "{bad:?}"
            );
        }
        let long = "n".repeat(65);
        assert_eq!(
            registered_name(Some(&os(&long))),
            Err(RegisteredNameSettingError::Invalid(
                RegisteredNameError::TooLong
            ))
        );
        // The words name the variable and never repeat the value.
        let e = registered_name(Some(&os("bad name")))
            .unwrap_err()
            .to_string();
        assert!(
            e.contains(REGISTERED_NAME_VARIABLE) && !e.contains("bad name"),
            "{e}"
        );
    }

    #[test]
    fn the_adapter_is_built_for_a_checked_name_only() {
        let log = LogSink::new(std::io::sink());
        assert_eq!(
            adapter("oac-g8live", &log).unwrap().registered_name(),
            "oac-g8live"
        );
        assert!(adapter("", &log).is_err());
        assert!(adapter("a b", &log).is_err());
    }

    #[test]
    fn the_path_is_the_override_else_the_home_variable_else_the_user_home_default() {
        let user = PathBuf::from("user-home");
        // The explicit override wins, as it is.
        assert_eq!(
            control_socket_path(Some(&os("x/y.sock")), Some(&os("h")), Some(&user)),
            Some(PathBuf::from("x/y.sock"))
        );
        // The Codex home variable, when set.
        assert_eq!(
            control_socket_path(None, Some(&os("custom-home")), Some(&user)),
            Some(Path::new("custom-home").join(tail()))
        );
        // The default under the user's home directory.
        assert_eq!(
            control_socket_path(None, None, Some(&user)),
            Some(user.join(HOME_DIRECTORY).join(tail()))
        );
        // Empty values count as unset.
        assert_eq!(
            control_socket_path(Some(&os("")), Some(&os("")), Some(&user)),
            Some(user.join(HOME_DIRECTORY).join(tail()))
        );
        // No home at all: no path.
        assert_eq!(control_socket_path(None, None, None), None);
        assert_eq!(
            control_socket_path(Some(&os("")), Some(&os("")), None),
            None
        );
    }

    #[test]
    fn no_path_and_a_missing_socket_are_probed_without_connecting_to_anything_else() {
        assert_eq!(probe_control_socket(None), SocketProbe::NoPath);
        let missing = std::env::temp_dir()
            .join("oac-codex-unit-no-such")
            .join(tail());
        assert_eq!(probe_control_socket(Some(&missing)), SocketProbe::Absent);
        assert_eq!(control_socket_server(&missing), Err(OpenError::Absent));
    }
}
