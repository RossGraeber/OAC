// SPDX-License-Identifier: Apache-2.0

//! The named pipe's security (G9, #70; C2 §4; `docs/planning/v0.1/06-security.md` §14 row 13):
//! the pipe is owned by the daemon's user and its DACL holds one access-allowed ACE, for that
//! user; a caller that cannot act as that user is refused at open; and a client refuses a
//! pipe that is not the daemon's before it sends a byte. Written before the module.
//!
//! No second account is needed to see the refusal: a thread that impersonates a **restricted
//! token** of the same user, with the user's SID marked deny-only, is a caller to whom the
//! user's ACE does not apply. This is a real Windows access check, not a model of one. A
//! second real account is the live leg recorded under `RISK-LOCAL-IPC` (11-risks row 20):
//! the one thing no test here can make is a pipe **owned by another user**, so the owner
//! half of the client's check is exercised with pipes whose DACL is wrong, not their owner.

#![cfg(windows)]

use std::ffi::c_void;
use std::fs::OpenOptions;
use std::os::windows::io::AsRawHandle;

use oac_cli::ipc::{ConnectError, Scope};
use oac_cli::peer::current_user_sid_string;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, CreateRestrictedToken, DACL_SECURITY_INFORMATION, DuplicateTokenEx,
    GetAce, OWNER_SECURITY_INFORMATION, SECURITY_ATTRIBUTES, SID_AND_ATTRIBUTES,
    SecurityImpersonation, TOKEN_DUPLICATE, TOKEN_IMPERSONATE, TOKEN_QUERY, TokenImpersonation,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken, SetThreadToken};

const ERROR_ACCESS_DENIED: i32 = 5;
/// `ACCESS_ALLOWED_ACE_TYPE` (`winnt.h`).
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;

fn open(name: &str) -> std::io::Result<std::fs::File> {
    OpenOptions::new().read(true).write(true).open(name)
}

fn open_read_only(name: &str) -> std::io::Result<std::fs::File> {
    OpenOptions::new().read(true).open(name)
}

/// A SID as text.
///
/// # Safety
///
/// `sid` must point to a valid SID.
unsafe fn sid_text(sid: *mut c_void) -> String {
    let mut text: *mut u16 = std::ptr::null_mut();
    // SAFETY: the caller's `sid` is valid; `text` is a local out-pointer.
    assert_ne!(unsafe { ConvertSidToStringSidW(sid, &mut text) }, 0);
    // SAFETY: on success `text` is a NUL-terminated UTF-16 string the call allocated.
    unsafe {
        let mut len = 0usize;
        while *text.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16(std::slice::from_raw_parts(text, len)).unwrap();
        LocalFree(text.cast());
        s
    }
}

/// The owner and the DACL of the pipe behind `file`: the owner's SID text, and the ACEs as
/// (type, flags, mask, SID text).
fn security_of(file: &std::fs::File) -> (String, Vec<(u8, u8, u32, String)>) {
    let mut owner: *mut c_void = std::ptr::null_mut();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut descriptor: *mut c_void = std::ptr::null_mut();
    // SAFETY: the handle is a live pipe handle; the out-pointers are locals.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle() as HANDLE,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    assert_eq!(status, 0, "GetSecurityInfo");
    assert!(!owner.is_null());
    assert!(!dacl.is_null(), "a NULL DACL would admit everyone");
    // SAFETY: `owner` and `dacl` point into `descriptor`, which stays allocated until the
    // LocalFree.
    let owner_text = unsafe { sid_text(owner) };
    // SAFETY: as above.
    let count = unsafe { (*dacl).AceCount };
    let mut aces = Vec::new();
    for i in 0..u32::from(count) {
        let mut ace: *mut c_void = std::ptr::null_mut();
        // SAFETY: `i` is below the ACE count; `ace` is a local out-pointer.
        assert_ne!(unsafe { GetAce(dacl, i, &mut ace) }, 0);
        let ace = ace.cast::<ACCESS_ALLOWED_ACE>();
        // SAFETY: an ACE of this layout begins at `ace`; the SID follows from `SidStart`.
        let entry = unsafe {
            let header = (*ace).Header;
            let sid = sid_text(std::ptr::addr_of_mut!((*ace).SidStart).cast());
            (header.AceType, header.AceFlags, (*ace).Mask, sid)
        };
        aces.push(entry);
    }
    // SAFETY: `descriptor` was allocated by GetSecurityInfo and is freed once.
    unsafe { LocalFree(descriptor) };
    (owner_text, aces)
}

#[tokio::test]
async fn the_pipes_are_owned_by_the_daemons_user_with_exactly_one_ace_for_that_user() {
    let scope = Scope::unique().unwrap();
    let lock = scope.acquire_lock().await.unwrap();
    let endpoint = scope.endpoint("claude");
    let _listener = endpoint.serve(&lock).await.unwrap();
    let me = current_user_sid_string().unwrap();
    assert!(me.starts_with("S-1-"), "{me}");

    let name = endpoint.display_name();
    // The lock pipe is the same shape, under the same scope (`oac-<scope>-lock`).
    let lock_name = name
        .rsplit_once('-')
        .map(|(prefix, _)| format!("{prefix}-lock"));
    for pipe in [Some(name), lock_name].into_iter().flatten() {
        let file = open(&pipe).expect("this user may open the pipe");
        let (owner, aces) = security_of(&file);
        assert_eq!(owner, me, "{pipe}: owned by the user, not a default owner");
        assert_eq!(
            aces.len(),
            1,
            "{pipe}: one ACE, no Everyone, no anonymous: {aces:?}"
        );
        let (kind, flags, mask, sid) = &aces[0];
        assert_eq!(*kind, ACCESS_ALLOWED_ACE_TYPE, "{pipe}: access-allowed");
        assert_eq!(*flags, 0, "{pipe}: nothing inherited, nothing to inherit");
        assert_ne!(*mask, 0);
        assert_eq!(sid, &me, "{pipe}: for the user the daemon runs as");
    }
}

/// A thread's access check with the user's SID deny-only: what a caller who is not that
/// user looks like to the pipe's DACL. `open` is what it tries.
fn open_as_restricted(
    name: String,
    open: fn(&str) -> std::io::Result<std::fs::File>,
) -> std::io::Result<std::fs::File> {
    std::thread::spawn(move || {
        let mut process_token: HANDLE = std::ptr::null_mut();
        // SAFETY: the current-process pseudo-handle; `process_token` is a local out-pointer.
        let ok = unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_IMPERSONATE,
                &mut process_token,
            )
        };
        assert_ne!(ok, 0, "OpenProcessToken");
        let mut impersonation: HANDLE = std::ptr::null_mut();
        // SAFETY: `process_token` is a live token handle; the out-pointer is a local.
        let ok = unsafe {
            DuplicateTokenEx(
                process_token,
                0,
                std::ptr::null(),
                SecurityImpersonation,
                TokenImpersonation,
                &mut impersonation,
            )
        };
        assert_ne!(ok, 0, "DuplicateTokenEx");

        // The user's SID, as a SID_AND_ATTRIBUTES for the sids to disable.
        let user = current_user_sid_string().unwrap();
        let wide: Vec<u16> = user.encode_utf16().chain(std::iter::once(0)).collect();
        let mut sid: *mut c_void = std::ptr::null_mut();
        // SAFETY: `wide` is a NUL-terminated UTF-16 SID string; `sid` is a local out-pointer.
        let ok = unsafe {
            windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW(
                wide.as_ptr(),
                &mut sid,
            )
        };
        assert_ne!(ok, 0, "ConvertStringSidToSidW");
        let disable = SID_AND_ATTRIBUTES {
            Sid: sid,
            Attributes: 0,
        };
        let mut restricted: HANDLE = std::ptr::null_mut();
        // SAFETY: `impersonation` is a live token; `disable` is one valid entry.
        let ok = unsafe {
            CreateRestrictedToken(
                impersonation,
                0,
                1,
                &disable,
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                &mut restricted,
            )
        };
        assert_ne!(ok, 0, "CreateRestrictedToken");

        // SAFETY: a null thread handle means the calling thread.
        assert_ne!(unsafe { SetThreadToken(std::ptr::null(), restricted) }, 0);
        let result = open(&name);
        // SAFETY: as above; a null token reverts to the process's own.
        unsafe { SetThreadToken(std::ptr::null(), std::ptr::null_mut()) };
        // SAFETY: the SID was allocated by ConvertStringSidToSidW and is freed once; the
        // three token handles were opened or made above and are closed once.
        unsafe {
            LocalFree(sid);
            CloseHandle(restricted);
            CloseHandle(impersonation);
            CloseHandle(process_token);
        }
        result
    })
    .join()
    .unwrap()
}

#[tokio::test]
async fn a_caller_who_cannot_act_as_the_user_is_refused_at_open_even_to_read() {
    let scope = Scope::unique().unwrap();
    let lock = scope.acquire_lock().await.unwrap();
    let endpoint = scope.endpoint("claude");
    let _listener = endpoint.serve(&lock).await.unwrap();
    let name = endpoint.display_name();

    // The control: a thread with the user's own token opens the pipe.
    let own = name.clone();
    std::thread::spawn(move || open(&own).expect("the user's own token is admitted"))
        .join()
        .unwrap();

    // The same user, with the user's SID deny-only, is not admitted by the user's ACE: not
    // to read and write, and not to read alone (the default descriptor of a pipe grants
    // Everyone read access, which would admit the second).
    for (what, opener) in [
        ("read and write", open as fn(&str) -> _),
        ("read only", open_read_only as fn(&str) -> _),
    ] {
        let n = name.clone();
        let denied = tokio::task::spawn_blocking(move || open_as_restricted(n, opener))
            .await
            .unwrap();
        match denied {
            Err(e) => assert_eq!(e.raw_os_error(), Some(ERROR_ACCESS_DENIED), "{what}: {e}"),
            Ok(_) => panic!("{what}: a caller that is not the user was admitted"),
        }
    }
}

/// A pipe on `name` made with `sddl` (the test makes pipes the daemon never would).
fn pipe_with(name: &str, sddl: &str) -> NamedPipeServer {
    let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
    let mut descriptor: *mut c_void = std::ptr::null_mut();
    // SAFETY: `wide` is a NUL-terminated UTF-16 string; `descriptor` is a local out-pointer.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(ok, 0, "{sddl}");
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    // SAFETY: `attributes` is a valid SECURITY_ATTRIBUTES for the call.
    let server = unsafe {
        ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                name,
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
            )
    }
    .expect("the test pipe");
    // SAFETY: the descriptor was allocated by the call above; the pipe holds its own copy.
    unsafe { LocalFree(descriptor) };
    server
}

#[tokio::test]
async fn a_client_refuses_a_pipe_that_is_not_the_daemons_before_it_sends_a_byte() {
    let me = current_user_sid_string().unwrap();
    // Pipes a squatter could make under the daemon's name. Their owner here is this user
    // (no second account is available), so it is the DACL half of the check that is shown:
    // one that excludes the user, one that admits Everyone instead of the user, and one that
    // adds Everyone to the user's own ACE.
    let squatters = [
        "D:P(A;;GA;;;SY)".to_owned(),
        format!("O:{me}D:P(A;;GA;;;WD)"),
        format!("O:{me}D:P(A;;GA;;;{me})(A;;GA;;;WD)"),
    ];
    for sddl in squatters {
        // A fresh pipe for each client: the test pipe has one instance, which the first
        // client to open it uses up.
        let scope = Scope::unique().unwrap();
        let endpoint = scope.endpoint("claude");
        let _server = pipe_with(&endpoint.display_name(), &sddl);
        assert!(
            matches!(endpoint.connect().await, Err(ConnectError::Foreign)),
            "{sddl}: async"
        );
        let scope = Scope::unique().unwrap();
        let endpoint = scope.endpoint("claude");
        let _server = pipe_with(&endpoint.display_name(), &sddl);
        let ep = endpoint.clone();
        let blocking = tokio::task::spawn_blocking(move || ep.connect_blocking().err())
            .await
            .unwrap();
        assert!(
            matches!(blocking, Some(ConnectError::Foreign)),
            "{sddl}: blocking"
        );
    }

    // The control: a pipe the daemon's own way (owner and one ACE for the user) is accepted.
    let scope = Scope::unique().unwrap();
    let endpoint = scope.endpoint("claude");
    let _server = pipe_with(
        &endpoint.display_name(),
        &format!("O:{me}D:P(A;;GA;;;{me})"),
    );
    assert!(endpoint.connect().await.is_ok());
}
