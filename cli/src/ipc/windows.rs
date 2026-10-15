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
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, DACL_SECURITY_INFORMATION, GetAce, OWNER_SECURITY_INFORMATION,
    SECURITY_ATTRIBUTES,
};
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
    /// `O:<sid>D:P(A;;GA;;;<sid>)`: owned by `sid` (not by the creator's default owner, which
    /// is `BUILTIN\Administrators` for an elevated admin), and one ACE, for `sid`, nothing
    /// inherited.
    fn only_user(sid: &str) -> io::Result<SecurityDescriptor> {
        let sddl: Vec<u16> = format!("O:{sid}D:P(A;;GA;;;{sid})")
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
    pub async fn serve(scope: &Scope, label: &str) -> Result<Listener, BindError> {
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
        // The name must stay served while this connection is handled. The next instance is
        // made before the connected one is taken, so a failure here leaves the listener as
        // it was.
        let following = create_instance(&self.name, &self.descriptor, false)?;
        let connected = std::mem::replace(&mut self.next, following);
        let mut pid = 0u32;
        // SAFETY: the handle is a live pipe handle owned by `connected`; `pid` is a local.
        let got =
            unsafe { GetNamedPipeClientProcessId(connected.as_raw_handle() as HANDLE, &mut pid) };
        if got == 0 || pid == 0 {
            return Ok(Accept::Rejected(Rejection {
                reason: "the peer could not be identified",
            }));
        }
        // The process id is read here, at accept, not at connect: the longer the daemon
        // took to get here (under load), the likelier the client has exited and its id has
        // been reused. A reused id names the new process, which is mis-attributed (it is
        // not refused); the new process is the same user's (the user check below), which
        // puts this inside the accepted same-user residual (D5). Pinning by the start time
        // makes the later observation and the user check about one process.
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
    // SAFETY: `text` was allocated by ConvertSidToStringSidW and is freed once.
    unsafe { LocalFree(text.cast()) };
    result
}

/// Whether the pipe object behind `handle` is the daemon's: **owned by `own_sid`** and
/// carrying exactly the descriptor [`SecurityDescriptor::only_user`] makes (one access-allowed
/// ACE, for `own_sid`).
///
/// This is the check that binds the pipe to its user. The process id of the instance's
/// creator (`GetNamedPipeServerProcessId`) is not enough: another account can create
/// `oac-<victim SID>-claude` with a permissive descriptor, pass the handle on and exit, and
/// the id can later be reused by a process of the victim's. The owner and the DACL belong to
/// the object, whoever holds it.
fn pipe_is_ours(handle: RawHandle, own_sid: &str) -> io::Result<bool> {
    let mut owner: *mut c_void = std::ptr::null_mut();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut descriptor: *mut c_void = std::ptr::null_mut();
    // SAFETY: `handle` is a live pipe handle held by the caller, opened with read control;
    // the out-pointers are locals.
    let status = unsafe {
        GetSecurityInfo(
            handle as HANDLE,
            SE_KERNEL_OBJECT,
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
    let ours = (|| {
        if owner.is_null() || dacl.is_null() {
            return false;
        }
        // SAFETY: `owner` and `dacl` point into `descriptor`, which is allocated until the
        // LocalFree below; the single ACE read is within the ACL's count.
        unsafe {
            if sid_string(owner).as_deref() != Some(own_sid) || (*dacl).AceCount != 1 {
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
                    == Some(own_sid)
        }
    })();
    // SAFETY: `descriptor` was allocated by GetSecurityInfo and is freed once.
    unsafe { LocalFree(descriptor) };
    Ok(ours)
}

/// Whether the pipe behind `handle` is the daemon's: its object is (see [`pipe_is_ours`]),
/// and the process that created this instance is this user's, pinned by its start time (a
/// second check, not the first).
fn server_is_ours(handle: RawHandle, own_sid: &str) -> Result<bool, io::Error> {
    if !pipe_is_ours(handle, own_sid)? {
        return Ok(false);
    }
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
                if !server_is_ours(client.as_raw_handle(), &scope.sid)? {
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
                if !server_is_ours(file.as_raw_handle(), &scope.sid)? {
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
