// SPDX-License-Identifier: Apache-2.0

//! The Unix socket's permissions and the run directory's checks (G9, #70; C2 §4): the
//! directory is `0700`, owned by the user and not a link; the socket is `0600`; a directory
//! other users can enter is refused by the daemon and by clients; a stale socket file does
//! not stop a new daemon; a live one does. Written before the module.
//!
//! The peer-credential refusal of another user needs another uid (root, or a second
//! account); its decision is unit-tested in `cli/src/ipc/unix.rs` (`same_user`), and the live
//! leg is recorded under `RISK-LOCAL-IPC`.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

use oac_cli::ipc::{BindError, ConnectError, Scope};

fn mode(path: &std::path::Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// The directory of a unique scope, from the socket path of one of its endpoints.
fn dir_of(endpoint: &oac_cli::ipc::Endpoint) -> PathBuf {
    PathBuf::from(endpoint.display_name())
        .parent()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn the_run_directory_is_0700_and_the_socket_0600() {
    let scope = Scope::unique().unwrap();
    let lock = scope.acquire_lock().await.unwrap();
    let endpoint = scope.endpoint("claude");
    let _listener = endpoint.serve(&lock).await.unwrap();
    // The daemon's control endpoint (G10, #71) is served the same way: the same mode, owner.
    let control = scope.endpoint("control");
    let _control_listener = control.serve(&lock).await.unwrap();
    let dir = dir_of(&endpoint);
    let socket = PathBuf::from(endpoint.display_name());
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&socket), 0o600);
    let control_socket = PathBuf::from(control.display_name());
    assert_eq!(mode(&control_socket), 0o600);
    assert_eq!(
        fs::metadata(&control_socket).unwrap().uid(),
        unsafe { libc::geteuid() },
        "the control socket is the user's"
    );
    assert!(
        fs::symlink_metadata(&control_socket)
            .unwrap()
            .file_type()
            .is_socket()
    );
    // The umask does not decide it.
    assert_eq!(fs::metadata(&dir).unwrap().uid(), unsafe {
        libc::geteuid()
    });
    assert!(
        fs::symlink_metadata(&socket)
            .unwrap()
            .file_type()
            .is_socket()
    );
}

#[tokio::test]
async fn a_run_directory_other_users_can_enter_is_refused_by_the_daemon_and_the_clients() {
    let scope = Scope::unique().unwrap();
    let lock = scope.acquire_lock().await.unwrap();
    let endpoint = scope.endpoint("claude");
    let listener = endpoint.serve(&lock).await.unwrap();
    let dir = dir_of(&endpoint);

    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    // A client does not trust a socket in a directory others can write.
    assert!(matches!(
        endpoint.connect().await,
        Err(ConnectError::Foreign)
    ));
    let ep = endpoint.clone();
    let blocking = tokio::task::spawn_blocking(move || ep.connect_blocking().err())
        .await
        .unwrap();
    assert!(matches!(blocking, Some(ConnectError::Foreign)));
    // A daemon does not take the lock there, so it cannot serve there either (serving takes
    // the lock).
    drop((listener, lock));
    match scope.acquire_lock().await {
        Err(BindError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied),
        other => panic!("{:?}", other.err()),
    }
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let lock = scope.acquire_lock().await.unwrap();
    assert!(scope.endpoint("codex").serve(&lock).await.is_ok());
}

/// `serve` checks the directory itself, not only the lock: a lock taken in a private
/// directory does not let a socket be made once the directory is open to others.
#[tokio::test]
async fn serve_refuses_a_directory_that_stopped_being_private() {
    let scope = Scope::unique().unwrap();
    let lock = scope.acquire_lock().await.unwrap();
    let endpoint = scope.endpoint("claude");
    let dir = dir_of(&endpoint);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    match endpoint.serve(&lock).await {
        Err(BindError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied),
        other => panic!("{:?}", other.err()),
    }
    assert!(
        !PathBuf::from(endpoint.display_name()).exists(),
        "no socket was made"
    );
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(endpoint.serve(&lock).await.is_ok());
}

#[tokio::test]
async fn a_stale_socket_file_does_not_stop_a_new_daemon_but_a_live_one_does() {
    let scope = Scope::unique().unwrap();
    let lock = scope.acquire_lock().await.unwrap();
    let endpoint = scope.endpoint("claude");
    let socket = PathBuf::from(endpoint.display_name());

    // A daemon that died without cleaning up: the socket file stays, nobody listens.
    drop(UnixListener::bind(&socket).unwrap());
    assert!(socket.exists());
    let listener = endpoint
        .serve(&lock)
        .await
        .expect("a stale socket is replaced");
    assert_eq!(mode(&socket), 0o600);
    // A live one is not.
    assert!(matches!(
        endpoint.serve(&lock).await,
        Err(BindError::AlreadyRunning)
    ));
    // Dropping the listener removes its socket file (a clean shutdown leaves nothing).
    drop(listener);
    assert!(!socket.exists());
}

#[tokio::test]
async fn the_lock_file_is_private_and_a_crash_leaves_nothing_stale() {
    let scope = Scope::unique().unwrap();
    let lock = scope.acquire_lock().await.unwrap();
    let path = dir_of(&scope.endpoint("claude")).join("daemon.lock");
    assert_eq!(mode(&path), 0o600);
    assert!(matches!(
        scope.acquire_lock().await,
        Err(BindError::AlreadyRunning)
    ));
    // The kernel drops an flock with the descriptor, as it does when the holder dies; the
    // file stays, and is not what holds the lock.
    drop(lock);
    assert!(path.exists());
    drop(scope.acquire_lock().await.expect("free again"));
}

#[tokio::test]
async fn a_socket_path_too_long_for_the_system_is_refused_with_a_reason() {
    // The scope's directory plus a label that fits `label_is_valid` but not `sun_path`.
    let scope = Scope::unique().unwrap();
    let lock = scope.acquire_lock().await.unwrap();
    let long = "a".repeat(32);
    let dir = dir_of(&scope.endpoint("claude"));
    if dir.as_os_str().len() + long.len() + 6 <= 100 {
        return; // The path fits here; the check has nothing to refuse.
    }
    match scope.endpoint(&long).serve(&lock).await {
        Err(BindError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput),
        other => panic!("{:?}", other.err()),
    }
}
