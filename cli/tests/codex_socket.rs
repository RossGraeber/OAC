// SPDX-License-Identifier: Apache-2.0

//! The Codex control socket as the daemon and `oac doctor` meet it (G8 slice S5, #433): a real
//! `AF_UNIX` stream socket on both platforms (Winsock on Windows), served here by a fake that
//! only accepts and counts. CI-default tier: no Codex install, loopback-local sockets only.
//!
//! Shown: the socket's absence, a stale socket and a live one are told apart; the server is
//! authenticated as this user's process and its process id is the one the operating system
//! reports (on Windows this is `SIO_AF_UNIX_GETPEERPID`, undocumented on Microsoft Learn: the
//! module documentation of `oac_cli::codex` says so); the probe, the opener and the doctor
//! check send and read nothing.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use oac_cli::codex::{
    OpenError, SocketProbe, control_socket_opener, control_socket_server, probe_control_socket,
};
use oac_cli::doctor::{self, CheckResult, ClaudeVersion, CredentialOutcome, Probes};
use oac_cli::keystore::CredentialProbe;

const WAIT: Duration = Duration::from_secs(10);

/// A fresh directory for one test's socket path, removed when it drops.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        // Short: an AF_UNIX path must fit `sun_path` (108 octets, 104 on macOS).
        let dir = std::env::temp_dir().join(format!("oac-cs-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn socket(&self) -> PathBuf {
        self.0.join("s.sock")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What the fake server saw.
#[derive(Default)]
struct Seen {
    connections: AtomicUsize,
    octets: AtomicUsize,
    /// Connections whose client closed (the server read end of stream).
    closed: AtomicUsize,
    stop: AtomicBool,
}

/// The bytes a fake server sends: a pattern that shows loss, repeats and reordering.
fn pattern(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

fn eventually(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + WAIT;
    while !f() {
        assert!(Instant::now() < end, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
mod server {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;

    pub struct Server {
        pub seen: Arc<Seen>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Server {
        pub fn serve_at(path: &Path, payload: usize) -> Server {
            let listener = UnixListener::bind(path).expect("bind the fake control socket");
            listener.set_nonblocking(true).unwrap();
            let seen = Arc::new(Seen::default());
            let s = seen.clone();
            let thread = std::thread::spawn(move || {
                while !s.stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((mut conn, _)) => {
                            s.connections.fetch_add(1, Ordering::SeqCst);
                            conn.set_nonblocking(false).unwrap();
                            conn.write_all(&pattern(payload)).unwrap();
                            let mut buf = [0u8; 64];
                            while let Ok(n) = conn.read(&mut buf) {
                                if n == 0 {
                                    break;
                                }
                                s.octets.fetch_add(n, Ordering::SeqCst);
                            }
                            s.closed.fetch_add(1, Ordering::SeqCst);
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(10)),
                    }
                }
            });
            Server {
                seen,
                thread: Some(thread),
            }
        }

        pub fn stop(mut self) {
            self.seen.stop.store(true, Ordering::SeqCst);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}

#[cfg(windows)]
mod server {
    use super::*;
    use windows_sys::Win32::Networking::WinSock::{
        AF_UNIX, FIONBIO, SOCK_STREAM, SOCKADDR, SOCKADDR_UN, WSADATA, WSAStartup, accept, bind,
        closesocket, ioctlsocket, listen, recv, send, socket,
    };

    pub struct Server {
        pub seen: Arc<Seen>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Server {
        pub fn serve_at(path: &Path, payload: usize) -> Server {
            let bytes = path.to_str().unwrap().as_bytes();
            // SAFETY: plain Winsock calls on sockets this function owns; `address` is a live
            // SOCKADDR_UN of at least the length passed.
            let listener = unsafe {
                let mut data: WSADATA = std::mem::zeroed();
                WSAStartup(0x0202, &mut data);
                let mut address: SOCKADDR_UN = std::mem::zeroed();
                address.sun_family = AF_UNIX;
                for (slot, b) in address.sun_path.iter_mut().zip(bytes) {
                    *slot = *b as i8;
                }
                let s = socket(i32::from(AF_UNIX), SOCK_STREAM, 0);
                assert_ne!(s, usize::MAX, "an AF_UNIX socket");
                let length = (std::mem::size_of::<u16>() + bytes.len() + 1) as i32;
                assert_eq!(
                    bind(
                        s,
                        (&address as *const SOCKADDR_UN).cast::<SOCKADDR>(),
                        length
                    ),
                    0,
                    "bind the fake control socket"
                );
                assert_eq!(listen(s, 8), 0);
                let mut nonblocking: u32 = 1;
                ioctlsocket(s, FIONBIO, &mut nonblocking);
                s
            };
            let seen = Arc::new(Seen::default());
            let s = seen.clone();
            let thread = std::thread::spawn(move || {
                while !s.stop.load(Ordering::SeqCst) {
                    // SAFETY: `listener` is open until this thread closes it below.
                    let conn =
                        unsafe { accept(listener, std::ptr::null_mut(), std::ptr::null_mut()) };
                    if conn == usize::MAX {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    s.connections.fetch_add(1, Ordering::SeqCst);
                    let mut blocking: u32 = 0;
                    // SAFETY: `conn` is ours; the buffer is a live local.
                    unsafe { ioctlsocket(conn, FIONBIO, &mut blocking) };
                    let data = pattern(payload);
                    let mut sent = 0;
                    while sent < data.len() {
                        // SAFETY: the slice is live for the length passed.
                        let n = unsafe {
                            send(conn, data[sent..].as_ptr(), (data.len() - sent) as i32, 0)
                        };
                        assert!(n > 0, "the fake server could not send");
                        sent += n as usize;
                    }
                    let mut buf = [0u8; 64];
                    loop {
                        // SAFETY: `buf` is a live buffer of the length passed.
                        let n = unsafe { recv(conn, buf.as_mut_ptr(), buf.len() as i32, 0) };
                        if n <= 0 {
                            break;
                        }
                        s.octets.fetch_add(n as usize, Ordering::SeqCst);
                    }
                    s.closed.fetch_add(1, Ordering::SeqCst);
                    // SAFETY: `conn` is ours and closed once.
                    unsafe { closesocket(conn) };
                }
                // SAFETY: `listener` is ours and closed once.
                unsafe { closesocket(listener) };
            });
            Server {
                seen,
                thread: Some(thread),
            }
        }

        pub fn stop(mut self) {
            self.seen.stop.store(true, Ordering::SeqCst);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}

use server::Server;

fn check_of(probe: SocketProbe) -> doctor::Check {
    let credential = || CredentialOutcome::Probed(CredentialProbe::Works);
    let claude = || ClaudeVersion::NotOnPath;
    let socket = move || probe.clone();
    let probes = Probes {
        status_wait: Duration::from_millis(50),
        negotiation: None,
        credential: &credential,
        fallback_file: None,
        claude: &claude,
        control_socket: &socket,
        environment: &|_| None,
    };
    // No usable scope: only the codex check is read.
    let scope = Err(std::io::Error::other("no scope for this test"));
    doctor::run(&scope, &probes)
        .into_iter()
        .find(|c| c.name == "codex")
        .expect("a codex check")
}

/// A socket that is not there is absent, a live one is reachable and served by this user's
/// process, and one left behind by a server that is gone is unreachable. The probe sends and
/// reads nothing, and the doctor's `codex` check follows each.
#[test]
fn the_doctor_tells_an_absent_a_live_and_a_stale_control_socket_apart_without_sending_anything() {
    let dir = Dir::new("d");
    let path = dir.socket();
    assert_eq!(probe_control_socket(Some(&path)), SocketProbe::Absent);
    assert_eq!(check_of(SocketProbe::Absent).result, CheckResult::Skip);

    let server = Server::serve_at(&path, 0);
    assert_eq!(probe_control_socket(Some(&path)), SocketProbe::Reachable);
    let seen = server.seen.clone();
    eventually("the server accepted the probe", || {
        seen.connections.load(Ordering::SeqCst) == 1
    });
    // Nothing was sent: the probe connected and closed.
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(seen.octets.load(Ordering::SeqCst), 0);
    let live = check_of(SocketProbe::Reachable);
    assert_eq!(live.result, CheckResult::Pass, "{}", live.detail);

    server.stop();
    // A server that went away leaves its socket file: present, and refusing.
    assert!(path.exists(), "the socket file is left behind");
    match probe_control_socket(Some(&path)) {
        SocketProbe::Unreachable(why) => assert!(!why.contains(path.to_str().unwrap()), "{why}"),
        other => panic!("a stale socket: {other:?}"),
    }
    let stale = check_of(SocketProbe::Unreachable("connect failed".into()));
    assert_eq!(stale.result, CheckResult::Warn, "{}", stale.detail);
    // Another user's server is a failure, and none of the details names a path.
    let other = check_of(SocketProbe::OtherUser);
    assert_eq!(other.result, CheckResult::Fail);
    assert_eq!(check_of(SocketProbe::NoPath).result, CheckResult::Skip);
}

/// The opener connects, authenticates the server as this user's, and observes it: the server's
/// process id is the one the operating system reports for the other end (this process), and a
/// server that is not a `codex` executable has no pairing key.
#[test]
fn the_opener_authenticates_the_server_and_reads_its_process_id_from_the_operating_system() {
    let dir = Dir::new("o");
    let path = dir.socket();
    let server = Server::serve_at(&path, 0);
    assert_eq!(
        control_socket_server(&path),
        Ok(Some(std::process::id())),
        "the server is this process"
    );

    // The opener, on a runtime of its own (the stream pump needs one).
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let opener = control_socket_opener(path.clone());
    let opened = opener(rt.handle()).expect("the control socket opens");
    assert!(
        opened.observation.pairing_key.is_none(),
        "this test binary is not a codex executable, so it has no pairing key"
    );
    assert_eq!(
        opened.observation.harness_label,
        oac_core::ids::Token::parse("codex")
    );
    drop(opened);
    server.stop();
    // And a socket path with nothing at it.
    match control_socket_opener(dir.0.join("none.sock"))(rt.handle()) {
        Err(OpenError::Absent) => {}
        other => panic!("{:?}", other.err()),
    }
}

/// A stream that spans many reads arrives whole and in order (reads smaller and larger than
/// one receive), the writer reaches the server, and `close` wakes a read that is blocked in
/// another thread (which returns the end of the stream) and ends the connection for the server.
/// On Windows this is the overlapped receive and its wake event, not a receive timeout.
#[test]
fn a_long_stream_arrives_whole_and_close_wakes_a_blocked_reader() {
    use std::io::{Read, Write};
    const TOTAL: usize = 300_007;
    let dir = Dir::new("w");
    let path = dir.socket();
    let server = Server::serve_at(&path, TOTAL);
    let seen = server.seen.clone();
    let mut socket = oac_cli::codex::open_blocking(&path).expect("the control socket opens");

    let mut got = Vec::with_capacity(TOTAL);
    let mut sizes = [1usize, 7, 1000, 8191, 8192, 20_000].into_iter().cycle();
    let end = Instant::now() + WAIT;
    while got.len() < TOTAL {
        assert!(Instant::now() < end, "the stream stalled at {}", got.len());
        let mut buf = vec![0u8; sizes.next().unwrap()];
        let n = socket.reader.read(&mut buf).expect("a read");
        assert!(n > 0, "the stream ended early at {}", got.len());
        got.extend_from_slice(&buf[..n]);
    }
    assert_eq!(got.len(), TOTAL);
    assert!(
        got == pattern(TOTAL),
        "bytes were lost, repeated or reordered"
    );

    // The writer reaches the server.
    socket.writer.write_all(b"hello").unwrap();
    eventually("the server read what was written", || {
        seen.octets.load(Ordering::SeqCst) == 5
    });

    // The server sends nothing more: the reader blocks in a receive in another thread.
    let mut reader = socket.reader;
    let (tx, rx) = std::sync::mpsc::channel();
    let blocked = std::thread::spawn(move || {
        let mut buf = [0u8; 64];
        let _ = tx.send(reader.read(&mut buf));
    });
    assert!(
        rx.recv_timeout(Duration::from_millis(300)).is_err(),
        "the read returned with nothing to read"
    );
    (socket.close)();
    let woken = rx
        .recv_timeout(WAIT)
        .expect("close woke the blocked reader")
        .expect("a read result");
    assert_eq!(woken, 0, "the end of the stream");
    blocked.join().unwrap();
    eventually("the server saw the connection end", || {
        seen.closed.load(Ordering::SeqCst) == 1
    });
    drop(socket.writer);
    server.stop();
}
