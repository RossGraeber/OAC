// SPDX-License-Identifier: Apache-2.0

//! The named pipe's security (G9, #70; C2 §4; `docs/planning/v0.1/06-security.md` §14 row 13):
//! its DACL holds one access-allowed ACE, for the daemon's own user, and a caller that cannot
//! act as that user is refused at open. Written before the module.
//!
//! No second account is needed to see the refusal: a thread that impersonates a **restricted
//! token** of the same user, with the user's SID marked deny-only, is a caller to whom the
//! user's ACE does not apply. This is a real Windows access check, not a model of one. A
//! second real account is the live leg recorded under `RISK-LOCAL-IPC` (11-risks row 20).

#![cfg(windows)]

use std::ffi::c_void;
use std::fs::OpenOptions;
use std::os::windows::io::AsRawHandle;

use oac_cli::ipc::Scope;
use oac_cli::peer::current_user_sid_string;
use windows_sys::Win32::Foundation::{HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetSecurityInfo, SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, CreateRestrictedToken, DACL_SECURITY_INFORMATION, DuplicateTokenEx,
    GetAce, SID_AND_ATTRIBUTES, SecurityImpersonation, TOKEN_DUPLICATE, TOKEN_IMPERSONATE,
    TOKEN_QUERY, TokenImpersonation,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken, SetThreadToken};

const ERROR_ACCESS_DENIED: i32 = 5;
/// `ACCESS_ALLOWED_ACE_TYPE` (`winnt.h`).
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;

fn open(name: &str) -> std::io::Result<std::fs::File> {
    OpenOptions::new().read(true).write(true).open(name)
}

/// The DACL of the pipe behind `file`: its ACEs as (type, flags, mask, SID string).
fn dacl_of(file: &std::fs::File) -> Vec<(u8, u8, u32, String)> {
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut descriptor: *mut c_void = std::ptr::null_mut();
    // SAFETY: the handle is a live pipe handle; the out-pointers are locals.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle() as HANDLE,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    assert_eq!(status, 0, "GetSecurityInfo");
    assert!(!dacl.is_null(), "a NULL DACL would admit everyone");
    // SAFETY: `dacl` points into `descriptor`, which stays allocated until the LocalFree.
    let count = unsafe { (*dacl).AceCount };
    let mut aces = Vec::new();
    for i in 0..u32::from(count) {
        let mut ace: *mut c_void = std::ptr::null_mut();
        // SAFETY: `i` is below the ACE count; `ace` is a local out-pointer.
        assert_ne!(unsafe { GetAce(dacl, i, &mut ace) }, 0);
        let ace = ace.cast::<ACCESS_ALLOWED_ACE>();
        // SAFETY: an ACE of this layout begins at `ace`; the SID follows from `SidStart`.
        let (kind, flags, mask, sid) = unsafe {
            let header = (*ace).Header;
            let mut text: *mut u16 = std::ptr::null_mut();
            assert_ne!(
                ConvertSidToStringSidW(std::ptr::addr_of_mut!((*ace).SidStart).cast(), &mut text),
                0
            );
            let mut len = 0usize;
            while *text.add(len) != 0 {
                len += 1;
            }
            let sid = String::from_utf16(std::slice::from_raw_parts(text, len)).unwrap();
            LocalFree(text.cast());
            (header.AceType, header.AceFlags, (*ace).Mask, sid)
        };
        aces.push((kind, flags, mask, sid));
    }
    // SAFETY: `descriptor` was allocated by GetSecurityInfo and is freed once.
    unsafe { LocalFree(descriptor) };
    aces
}

#[tokio::test]
async fn the_pipes_dacl_has_exactly_one_ace_and_it_is_for_the_daemons_user() {
    let scope = Scope::unique().unwrap();
    let endpoint = scope.endpoint("claude");
    let _listener = endpoint.bind().await.unwrap();
    let me = current_user_sid_string().unwrap();
    assert!(me.starts_with("S-1-"), "{me}");

    let file = open(&endpoint.display_name()).expect("this user may open the pipe");
    let aces = dacl_of(&file);
    assert_eq!(
        aces.len(),
        1,
        "one ACE, no Everyone, no anonymous: {aces:?}"
    );
    let (kind, flags, mask, sid) = &aces[0];
    assert_eq!(*kind, ACCESS_ALLOWED_ACE_TYPE, "access-allowed");
    assert_eq!(*flags, 0, "nothing inherited, nothing to inherit");
    assert_ne!(*mask, 0);
    assert_eq!(sid, &me, "for the user the daemon runs as");

    // The lock pipe carries the same descriptor.
    let _lock = scope.acquire_lock().await.unwrap();
}

/// A thread's access check with the user's SID deny-only: what a caller who is not that user
/// looks like to the pipe's DACL.
fn open_as_restricted(name: String) -> std::io::Result<std::fs::File> {
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
        // SAFETY: the SID was allocated by ConvertStringSidToSidW and is freed once.
        unsafe { LocalFree(sid) };
        result
    })
    .join()
    .unwrap()
}

#[tokio::test]
async fn a_caller_who_cannot_act_as_the_user_is_refused_at_open() {
    let scope = Scope::unique().unwrap();
    let endpoint = scope.endpoint("claude");
    let _listener = endpoint.bind().await.unwrap();
    let name = endpoint.display_name();

    // The control: a thread with the user's own token opens the pipe.
    let own = name.clone();
    std::thread::spawn(move || open(&own).expect("the user's own token is admitted"))
        .join()
        .unwrap();

    // The same user, with the user's SID deny-only, is not admitted by the user's ACE.
    let denied = tokio::task::spawn_blocking(move || open_as_restricted(name))
        .await
        .unwrap();
    match denied {
        Err(e) => assert_eq!(e.raw_os_error(), Some(ERROR_ACCESS_DENIED), "{e}"),
        Ok(_) => panic!("a caller that is not the user was admitted"),
    }
}
