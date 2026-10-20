// SPDX-License-Identifier: Apache-2.0

//! The local IPC contract (G9, #70, `cli/src/ipc/`), the same on Windows named pipes and
//! Unix sockets: a client of the daemon's user connects, bytes flow both ways, the peer is
//! identified by the operating system, one daemon per user, and a missing daemon is told
//! apart from a busy or foreign one. Written before the module.
//!
//! What is OS-specific (the pipe's DACL, the socket's modes) is in `ipc_windows.rs` and
//! `ipc_unix.rs`.

use std::io::{Read, Write};

use oac_cli::ipc::{
    Accept, Accepted, BindError, ConnectError, Endpoint, Scope, harness_label_is_valid,
    label_is_valid,
};
use oac_cli::peer::ProcessTable as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn scope() -> Scope {
    Scope::unique().expect("a scope of its own")
}

async fn accept_connection(listener: &mut oac_cli::ipc::Listener) -> Accepted {
    match listener.accept().await.expect("the listener accepts") {
        Accept::Connection(a) => a,
        Accept::Rejected(r) => panic!("a peer of this user was rejected: {}", r.reason),
    }
}

#[tokio::test]
async fn bytes_flow_both_ways_and_the_peer_is_identified_by_the_system() {
    let scope = scope();
    let lock = scope.acquire_lock().await.unwrap();
    let endpoint = scope.endpoint("claude");
    let mut listener = endpoint.serve(&lock).await.unwrap();

    // A blocking client (the hook handler's kind).
    let ep = endpoint.clone();
    let client = tokio::task::spawn_blocking(move || {
        let mut s = ep.connect_blocking().unwrap();
        s.write_all(b"ping\n").unwrap();
        let mut reply = [0u8; 5];
        s.read_exact(&mut reply).unwrap();
        reply
    });
    let mut accepted = accept_connection(&mut listener).await;
    // The client is a thread of this process: the system reports this process's id.
    assert_eq!(accepted.peer.pid, Some(std::process::id()));
    // The peer is pinned by its start time, which is this process's.
    let me = oac_cli::peer::SystemTable::new()
        .unwrap()
        .lookup(std::process::id())
        .unwrap()
        .id;
    assert_eq!(accepted.peer.process(), Some(me));
    let mut got = [0u8; 5];
    accepted.stream.read_exact(&mut got).await.unwrap();
    assert_eq!(&got, b"ping\n");
    accepted.stream.write_all(b"pong\n").await.unwrap();
    accepted.stream.flush().await.unwrap();
    assert_eq!(&client.await.unwrap(), b"pong\n");
}

#[tokio::test]
async fn an_async_client_reads_and_writes_at_once() {
    let scope = scope();
    let lock = scope.acquire_lock().await.unwrap();
    let endpoint = scope.endpoint("codex");
    let mut listener = endpoint.serve(&lock).await.unwrap();
    let mut client = endpoint.connect().await.unwrap();
    let accepted = accept_connection(&mut listener).await;
    assert_eq!(accepted.peer.pid, Some(std::process::id()));

    // Both directions carry a megabyte at the same time: neither waits for the other.
    let payload = vec![0xa5u8; 1 << 20];
    let to_server = payload.clone();
    let writer = tokio::spawn(async move {
        client.write_all(&to_server).await.unwrap();
        let mut back = vec![0u8; 1 << 20];
        client.read_exact(&mut back).await.unwrap();
        back
    });
    let mut received = vec![0u8; 1 << 20];
    let to_client = payload.clone();
    let (mut r, mut w) = tokio::io::split(accepted.stream);
    let (read, written) = tokio::join!(r.read_exact(&mut received), w.write_all(&to_client));
    read.unwrap();
    written.unwrap();
    assert_eq!(received, payload);
    assert_eq!(writer.await.unwrap(), payload);
}

#[tokio::test]
async fn the_end_of_a_connection_is_seen_from_both_sides() {
    let scope = scope();
    let lock = scope.acquire_lock().await.unwrap();
    let endpoint = scope.endpoint("claude");
    let mut listener = endpoint.serve(&lock).await.unwrap();

    // The client goes first: the daemon reads end of stream (the shim crash and clean exit
    // of C2 §5 look the same).
    let client = endpoint.connect().await.unwrap();
    let mut accepted = accept_connection(&mut listener).await;
    drop(client);
    let mut rest = Vec::new();
    accepted.stream.read_to_end(&mut rest).await.unwrap();
    assert!(rest.is_empty());

    // The daemon goes first: the client reads end of stream.
    let mut client = endpoint.connect().await.unwrap();
    let accepted = accept_connection(&mut listener).await;
    drop(accepted);
    let mut rest = Vec::new();
    let end = client.read_to_end(&mut rest).await;
    assert!(rest.is_empty());
    // A reset is also an end; what matters is that the read returns.
    let _ = end;
}

#[tokio::test]
async fn the_name_stays_served_between_connections() {
    let scope = scope();
    let lock = scope.acquire_lock().await.unwrap();
    let endpoint = scope.endpoint("claude");
    let mut listener = endpoint.serve(&lock).await.unwrap();
    // Twenty clients at once, before any is accepted: none finds the endpoint missing.
    let mut clients = Vec::new();
    for i in 0..20u8 {
        let ep = endpoint.clone();
        clients.push(tokio::spawn(async move {
            let mut c = ep.connect().await.expect("the endpoint is served");
            c.write_all(&[i]).await.unwrap();
            c.shutdown().await.ok();
            // Hold the connection until the server has read it.
            let mut rest = Vec::new();
            let _ = c.read_to_end(&mut rest).await;
        }));
    }
    let mut seen = Vec::new();
    for _ in 0..20 {
        let mut accepted = accept_connection(&mut listener).await;
        let mut b = [0u8; 1];
        accepted.stream.read_exact(&mut b).await.unwrap();
        seen.push(b[0]);
        drop(accepted);
    }
    for c in clients {
        c.await.unwrap();
    }
    seen.sort_unstable();
    assert_eq!(seen, (0..20u8).collect::<Vec<_>>());
}

#[tokio::test]
async fn a_second_listener_on_the_same_endpoint_is_refused_until_the_first_ends() {
    let scope = scope();
    let lock = scope.acquire_lock().await.unwrap();
    let endpoint = scope.endpoint("claude");
    let first = endpoint.serve(&lock).await.unwrap();
    assert!(matches!(
        endpoint.serve(&lock).await,
        Err(BindError::AlreadyRunning)
    ));
    // Another harness's endpoint is its own.
    let other = scope.endpoint("codex").serve(&lock).await.unwrap();
    drop(first);
    let again = endpoint.serve(&lock).await.expect("the name is free again");
    drop((other, again));
}

#[tokio::test]
async fn the_daemon_lock_is_held_by_one_holder_at_a_time() {
    let scope = scope();
    let lock = scope.acquire_lock().await.unwrap();
    assert!(matches!(
        scope.acquire_lock().await,
        Err(BindError::AlreadyRunning)
    ));
    // It does not stand in the way of the endpoints it guards.
    let listener = scope.endpoint("claude").serve(&lock).await.unwrap();
    // A clone of the scope is the same user's namespace: the same lock.
    assert!(matches!(
        scope.clone().acquire_lock().await,
        Err(BindError::AlreadyRunning)
    ));
    // Another scope is another namespace.
    let elsewhere = Scope::unique().unwrap();
    drop(elsewhere.acquire_lock().await.unwrap());
    drop((listener, lock));
    drop(scope.acquire_lock().await.expect("released when dropped"));
}

#[tokio::test]
async fn a_client_of_a_daemon_that_is_not_running_is_told_so() {
    let scope = scope();
    let lock = scope.acquire_lock().await.unwrap();
    let endpoint = scope.endpoint("claude");
    assert!(matches!(
        endpoint.connect().await,
        Err(ConnectError::NotRunning)
    ));
    let ep = endpoint.clone();
    let blocking = tokio::task::spawn_blocking(move || ep.connect_blocking().err()).await;
    assert!(matches!(blocking.unwrap(), Some(ConnectError::NotRunning)));
    // And once it runs, the same clients connect.
    let _listener = endpoint.serve(&lock).await.unwrap();
    assert!(endpoint.connect().await.is_ok());
}

#[tokio::test]
async fn a_client_that_connects_and_leaves_does_not_stop_the_listener() {
    let scope = scope();
    let lock = scope.acquire_lock().await.unwrap();
    let endpoint = scope.endpoint("claude");
    let mut listener = endpoint.serve(&lock).await.unwrap();
    for _ in 0..3 {
        drop(endpoint.connect().await.unwrap());
        let mut accepted = accept_connection(&mut listener).await;
        let mut rest = Vec::new();
        accepted.stream.read_to_end(&mut rest).await.unwrap();
        assert!(rest.is_empty());
    }
}

#[test]
fn endpoint_labels_are_lower_case_words_and_lock_is_reserved() {
    for ok in ["claude", "codex", "a", "gemini2"] {
        assert!(label_is_valid(ok), "{ok}");
    }
    for bad in [
        "", "Claude", "2fast", "lock", "a-b", "a b", "a.b", "..", "a/b", "a\\b",
    ] {
        assert!(!label_is_valid(bad), "{bad:?}");
    }
    assert!(!label_is_valid(&"a".repeat(33)));
}

/// `control` is the daemon's own endpoint (G10, #71): a valid endpoint label, never a harness
/// label, so `oac mcp-shim --harness control` is refused.
#[test]
fn control_is_an_endpoint_label_but_not_a_harness_label() {
    assert!(label_is_valid("control"));
    assert!(!harness_label_is_valid("control"));
    assert!(!harness_label_is_valid("lock"));
    for ok in ["claude", "codex", "controlx", "a"] {
        assert!(harness_label_is_valid(ok), "{ok}");
    }
    for bad in ["", "Control", "2fast", "a-b"] {
        assert!(!harness_label_is_valid(bad), "{bad:?}");
    }
}

#[test]
#[should_panic(expected = "endpoint label")]
fn the_lock_name_cannot_be_an_endpoint() {
    let _ = scope().endpoint("lock");
}

#[test]
fn an_endpoint_has_a_name_for_diagnostics() {
    let scope = scope();
    let e: Endpoint = scope.endpoint("claude");
    assert_eq!(e.label(), "claude");
    assert!(e.display_name().contains("claude"), "{}", e.display_name());
}
