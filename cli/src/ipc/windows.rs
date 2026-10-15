// SPDX-License-Identifier: Apache-2.0

//! The Windows local IPC: named pipes whose DACL admits the daemon's own user alone (C2 §4).
//!
//! Microsoft Learn, `CreateNamedPipeW`: the default security descriptor of a pipe "grant[s]
//! read access to members of the Everyone group and the anonymous account", so every pipe
//! here is created with a descriptor of its own, `D:P(A;;GA;;;<user SID>)`: a protected DACL
//! (no inherited entries) with one access-allowed ACE, for the user the daemon runs as.

use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, PipeMode, ServerOptions};
use windows_sys::Win32::Foundation::{HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::Pipes::{
    GetNamedPipeClientProcessId, GetNamedPipeServerProcessId, WaitNamedPipeW,
};

use super::{Accept, Accepted, AsyncStream, BindError, ConnectError, PeerIdentity, Rejection};
use crate::peer::{current_user_sid_string, process_started, process_user_matches};

/// The blocking client stream: a synchronous pipe handle.
pub type Blocking = File;

const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_PIPE_BUSY: i32 = 231;
/// `SECURITY_IDENTIFICATION`: the server may learn the client's identity but never
/// impersonate it. (`tokio`'s client sets the same by default; `std`'s needs it said.)
const SECURITY_IDENTIFICATION: u32 = 0x0001_0000;
/// How long a client waits for a pipe instance to become free: a server makes its next
/// instance right after each accept, so a wait is short.
const BUSY_WAIT: Duration = Duration::from_secs(5);
const BUSY_STEP: Duration = Duration::from_millis(25);

/// A user's pipe namespace.
#[derive(Clone, Debug)]
pub struct Scope {
    /// The SID whose ACE the pipes carry: always the daemon's own user.
    sid: Arc<str>,
    /// The part of the pipe names that tells scopes apart.
    infix: Arc<str>,
    /// The test-isolation namespace, if any (`ipc::Scope::for_user_in`).
    namespace: Option<String>,
}

static NEXT_SCOPE: AtomicU64 = AtomicU64::new(0);

impl Scope {
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    pub fn for_user(namespace: Option<&str>) -> io::Result<Scope> {
        let sid = current_user_sid_string()
            .ok_or_else(|| io::Error::other("the current user's SID cannot be read"))?;
        let infix = match namespace {
            Some(n) => format!("{sid}-{n}"),
            None => sid.clone(),
        };
        Ok(Scope {
            infix: infix.into(),
            sid: sid.into(),
            namespace: namespace.map(str::to_owned),
        })
    }

    pub fn unique() -> io::Result<Scope> {
        let mut scope = Scope::for_user(None)?;
        let n = NEXT_SCOPE.fetch_add(1, Ordering::Relaxed);
        scope.infix = format!("test{}x{n}", std::process::id()).into();
        Ok(scope)
    }

    pub fn endpoint_name(&self, label: &str) -> String {
        self.pipe_name(label)
    }

    fn pipe_name(&self, label: &str) -> String {
        format!(r"\\.\pipe\oac-{}-{label}", self.infix)
    }

    fn lock_name(&self) -> String {
        self.pipe_name("lock")
    }
}

/// A self-relative security descriptor allocated by the system, freed on drop.
struct SecurityDescriptor(*mut c_void);

// SAFETY: the descriptor is immutable memory that no thread writes after construction.
unsafe impl Send for SecurityDescriptor {}
// SAFETY: as above.
unsafe impl Sync for SecurityDescriptor {}

impl SecurityDescriptor {
    /// `D:P(A;;GA;;;<sid>)`: one ACE, for `sid`, and nothing inherited.
    fn only_user(sid: &str) -> io::Result<SecurityDescriptor> {
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})")
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
        Ok(SecurityDescriptor(descriptor))
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        }
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated by the system call that made it, and is freed
        // once.
        unsafe { LocalFree(self.0) };
    }
}

fn create_instance(
    name: &str,
    descriptor: &SecurityDescriptor,
    first: bool,
) -> io::Result<NamedPipeServer> {
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .pipe_mode(PipeMode::Byte);
    let mut attributes = descriptor.attributes();
    // SAFETY: `attributes` is a valid SECURITY_ATTRIBUTES for the call, and the descriptor
    // it points to outlives it.
    unsafe {
        options.create_with_security_attributes_raw(
            name,
            (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
        )
    }
}

/// `ERROR_ACCESS_DENIED` on the first instance of a name that exists: someone holds it.
fn bind_error(e: io::Error) -> BindError {
    if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) {
        BindError::AlreadyRunning
    } else {
        BindError::Io(e)
    }
}

/// A served pipe name.
pub struct Listener {
    name: String,
    descriptor: SecurityDescriptor,
    next: NamedPipeServer,
}

impl Listener {
    pub async fn bind(scope: &Scope, label: &str) -> Result<Listener, BindError> {
        let name = scope.pipe_name(label);
        let descriptor = SecurityDescriptor::only_user(&scope.sid)?;
        let next = create_instance(&name, &descriptor, true).map_err(bind_error)?;
        Ok(Listener {
            name,
            descriptor,
            next,
        })
    }

    pub async fn accept(&mut self) -> io::Result<Accept> {
        self.next.connect().await?;
        // The name must stay served while this connection is handled.
        let connected = std::mem::replace(
            &mut self.next,
            create_instance(&self.name, &self.descriptor, false)?,
        );
        let mut pid = 0u32;
        // SAFETY: the handle is a live pipe handle owned by `connected`; `pid` is a local.
        let got =
            unsafe { GetNamedPipeClientProcessId(connected.as_raw_handle() as HANDLE, &mut pid) };
        if got == 0 || pid == 0 {
            return Ok(Accept::Rejected(Rejection {
                reason: "the peer could not be identified",
            }));
        }
        // The peer is pinned by its start time: the user check is about that process, and
        // the observation made later is refused if the id has been recycled meanwhile.
        let Some(started) = process_started(pid) else {
            return Ok(Accept::Rejected(Rejection {
                reason: "the peer could not be identified",
            }));
        };
        // The DACL already refused other users at open; this is the second check (C2 §4).
        if !process_user_matches(pid, Some(started)) {
            return Ok(Accept::Rejected(Rejection {
                reason: "the peer is another user",
            }));
        }
        Ok(Accept::Connection(Accepted {
            stream: Box::new(connected),
            peer: PeerIdentity {
                pid: Some(pid),
                started: Some(started),
            },
        }))
    }
}

/// The one-daemon-per-user lock: the first instance of a pipe nobody serves.
pub struct Lock {
    _pipe: NamedPipeServer,
    _descriptor: SecurityDescriptor,
}

impl Lock {
    pub async fn acquire(scope: &Scope) -> Result<Lock, BindError> {
        let descriptor = SecurityDescriptor::only_user(&scope.sid)?;
        let pipe = create_instance(&scope.lock_name(), &descriptor, true).map_err(bind_error)?;
        Ok(Lock {
            _pipe: pipe,
            _descriptor: descriptor,
        })
    }
}

/// Whether the process serving the pipe behind `handle` is this user's.
fn server_is_ours(handle: RawHandle) -> Result<bool, io::Error> {
    let mut pid = 0u32;
    // SAFETY: `handle` is a live pipe handle held by the caller; `pid` is a local.
    if unsafe { GetNamedPipeServerProcessId(handle as HANDLE, &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(pid != 0 && process_started(pid).is_some_and(|s| process_user_matches(pid, Some(s))))
}

pub async fn connect_async(
    scope: &Scope,
    label: &str,
) -> Result<Box<dyn AsyncStream>, ConnectError> {
    let name = scope.pipe_name(label);
    let started = std::time::Instant::now();
    loop {
        match ClientOptions::new().open(&name) {
            Ok(client) => {
                if !server_is_ours(client.as_raw_handle())? {
                    return Err(ConnectError::Foreign);
                }
                return Ok(Box::new(client));
            }
            Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) => {
                return Err(ConnectError::NotRunning);
            }
            Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) => {
                // The pipe exists and refuses this user: it is not the daemon's.
                return Err(ConnectError::Foreign);
            }
            // All instances are connected; the server makes the next one at once.
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                if started.elapsed() >= BUSY_WAIT {
                    return Err(ConnectError::Busy);
                }
                tokio::time::sleep(BUSY_STEP).await;
            }
            Err(e) => return Err(ConnectError::Io(e)),
        }
    }
}

pub fn connect_blocking(scope: &Scope, label: &str) -> Result<Blocking, ConnectError> {
    let name = scope.pipe_name(label);
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let started = std::time::Instant::now();
    loop {
        let opened = OpenOptions::new()
            .read(true)
            .write(true)
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(&name);
        match opened {
            Ok(file) => {
                if !server_is_ours(file.as_raw_handle())? {
                    return Err(ConnectError::Foreign);
                }
                return Ok(file);
            }
            Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) => {
                return Err(ConnectError::NotRunning);
            }
            Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) => {
                return Err(ConnectError::Foreign);
            }
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                if started.elapsed() >= BUSY_WAIT {
                    return Err(ConnectError::Busy);
                }
                // SAFETY: `wide` is a NUL-terminated UTF-16 pipe name.
                unsafe { WaitNamedPipeW(wide.as_ptr(), BUSY_STEP.as_millis() as u32) };
            }
            Err(e) => return Err(ConnectError::Io(e)),
        }
    }
}
