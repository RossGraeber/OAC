// SPDX-License-Identifier: Apache-2.0

//! `oac mcp-shim`: the stdio process a harness starts as its MCP server (G9, #70;
//! `docs/planning/decisions/C2-process-model.md` §1, §6).
//!
//! The shim is a **byte relay** and nothing more: it connects to the daemon's endpoint for
//! its harness, copies its standard input to the daemon and the daemon's answers to its
//! standard output, and exits when either ends. It parses no MCP frame, holds no key, no
//! policy and no session state, and decides nothing. It reads the endpoint's scope and, for a
//! Claude Code shim only, one environment variable (below): the MCP server is the harness's adapter inside the daemon
//! (`adapters/claude`, `adapters/codex`), which sees exactly the bytes the harness wrote.
//!
//! **The hello line** (`spec/bindings/mcp.md` [MCPB-CLD-011], [MCPB-CLD-012]). A shim of the
//! `claude` harness whose environment holds a non-empty `CLAUDE_CODE_SESSION_ID` (E) sends
//! one line first, before any byte of the harness: `{"oac_hello":1,"session_id":"<E>"}`. The
//! daemon's Claude adapter consumes it and reports E as the attachment's cross-check value,
//! which the core compares with the session id a `SessionStart` hook reports (case 3(a) and
//! 3(b) of `spec/session-channels.md` §6.7.3). It is a hint and never an identity
//! (C4 §3 "Terms"); the shim reads no other session id. The line is built by the adapter's
//! crate ([`oac_adapter_claude::hook::hello_line`]), so the form is defined in one place.
//!
//! A shim that is killed or crashes closes its end of the connection, and the daemon sees the
//! end of stream and ends that one connection ([`crate::daemon`]); no other session notices.
//!
//! **It does not start the daemon** (lead decision D4, 2026-10-14): with no daemon running it
//! says so, and names the command, and exits non-zero, so the harness reports its MCP server
//! failed to start. A harness is not made to depend on a background process starting itself,
//! and no job-object or session lifetime can then take the daemon down with a session.

use std::io::{Read, Write};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::ipc::{AsyncStream, ConnectError, Endpoint};

/// Exit status when the harness ended the session (its input closed).
pub const EXIT_ENDED: i32 = 0;
/// Exit status when the shim could not reach the daemon or the daemon ended the connection.
pub const EXIT_FAILED: i32 = 1;

const CHUNK: usize = 8192;
const QUEUE: usize = 8;

/// How a relay ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    /// The harness closed the shim's standard input.
    InputClosed,
    /// The daemon closed the connection (it stopped, or ended this session).
    DaemonClosed,
}

/// The hello line for a shim of harness `label` whose environment holds `session_id`
/// (`CLAUDE_CODE_SESSION_ID`): only the `claude` harness sends one, only for a non-empty
/// value the adapter can read, and never for a value that is not valid text.
pub fn hello_for(label: &str, session_id: Option<&std::ffi::OsStr>) -> Option<Vec<u8>> {
    if label != "claude" {
        return None;
    }
    oac_adapter_claude::hook::hello_line(session_id?.to_str()?)
}

/// The message for a daemon that cannot be reached, with the command to start it.
pub fn unreachable_message(e: &ConnectError) -> String {
    match e {
        ConnectError::NotRunning => {
            "oac mcp-shim: the daemon is not running; start it with `oac start`".to_owned()
        }
        other => format!("oac mcp-shim: {other}"),
    }
}

/// Connects to `endpoint`.
///
/// # Errors
///
/// [`ConnectError`]; [`unreachable_message`] words it.
pub async fn connect(endpoint: &Endpoint) -> Result<Box<dyn AsyncStream>, ConnectError> {
    endpoint.connect().await
}

/// Copies `input` to `stream` and `stream` to `output` until one ends, after writing `hello`
/// (when there is one) as the first bytes of the stream. The blocking ends run on threads of
/// their own, so neither can hold up the other.
pub async fn relay(
    stream: Box<dyn AsyncStream>,
    hello: Option<Vec<u8>>,
    mut input: impl Read + Send + 'static,
    mut output: impl Write + Send + 'static,
) -> Ended {
    let (mut daemon_reads, mut daemon_writes) = tokio::io::split(stream);
    let (in_tx, mut in_rx) = mpsc::channel::<Vec<u8>>(QUEUE);
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(QUEUE);

    // The harness's standard input: a blocking read in a thread.
    let _ = std::thread::Builder::new()
        .name("oac-shim-in".into())
        .spawn(move || {
            let mut buf = vec![0u8; CHUNK];
            loop {
                match input.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => {
                        if in_tx.blocking_send(buf[..n].to_vec()).is_err() {
                            return;
                        }
                    }
                }
            }
        });
    // The harness's standard output: a blocking write in a thread.
    let writer = std::thread::Builder::new()
        .name("oac-shim-out".into())
        .spawn(move || {
            while let Some(chunk) = out_rx.blocking_recv() {
                if output.write_all(&chunk).is_err() || output.flush().is_err() {
                    return;
                }
            }
        });

    let to_daemon = async move {
        if let Some(hello) = hello
            && (daemon_writes.write_all(&hello).await.is_err()
                || daemon_writes.flush().await.is_err())
        {
            return Ended::DaemonClosed;
        }
        while let Some(chunk) = in_rx.recv().await {
            if daemon_writes.write_all(&chunk).await.is_err()
                || daemon_writes.flush().await.is_err()
            {
                // The daemon can no longer be written to: it is gone, not the harness.
                return Ended::DaemonClosed;
            }
        }
        // Input closed: the session is over; closing our side tells the daemon.
        let _ = daemon_writes.shutdown().await;
        Ended::InputClosed
    };
    let from_daemon = async move {
        let mut buf = vec![0u8; CHUNK];
        loop {
            match daemon_reads.read(&mut buf).await {
                Ok(0) | Err(_) => return Ended::DaemonClosed,
                Ok(n) => {
                    if out_tx.send(buf[..n].to_vec()).await.is_err() {
                        return Ended::DaemonClosed;
                    }
                }
            }
        }
    };
    tokio::pin!(to_daemon);
    tokio::pin!(from_daemon);
    let ended = tokio::select! {
        ended = &mut to_daemon => ended,
        ended = &mut from_daemon => ended,
    };
    // What the daemon had already sent reaches the harness before the shim exits.
    if ended == Ended::DaemonClosed
        && let Ok(w) = writer
    {
        let _ = tokio::task::spawn_blocking(move || w.join()).await;
    }
    ended
}

/// The exit status for how the relay ended.
pub fn exit_status(ended: Ended) -> i32 {
    match ended {
        Ended::InputClosed => EXIT_ENDED,
        Ended::DaemonClosed => EXIT_FAILED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::duplex;

    /// A writer into a shared buffer.
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    /// Bytes that are not text, and no frame: the relay copies them as they are.
    const SAMPLE: &[u8] = b"{\"id\":0}\n  odd \x00\xff bytes";

    #[test]
    fn input_goes_to_the_daemon_unchanged_and_the_end_of_input_ends_the_relay() {
        rt().block_on(async {
            let (daemon, shim_side) = duplex(1 << 16);
            let out = Shared::default();
            let input = std::io::Cursor::new(SAMPLE.to_vec());
            let relay_task = tokio::spawn(relay(Box::new(shim_side), None, input, out.clone()));
            let (mut daemon_r, _daemon_w) = tokio::io::split(daemon);
            // The input was a finite file: the relay ends when it is closed, and the daemon
            // reads everything that was sent, then end of stream.
            assert_eq!(relay_task.await.unwrap(), Ended::InputClosed);
            let mut got = Vec::new();
            daemon_r.read_to_end(&mut got).await.unwrap();
            assert_eq!(got, SAMPLE);
        });
    }

    #[test]
    fn the_daemon_closing_ends_the_shim_after_its_answer_reached_the_harness() {
        rt().block_on(async {
            let (daemon, shim_side) = duplex(1 << 16);
            let out = Shared::default();
            // An input that never closes.
            struct Never;
            impl Read for Never {
                fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                    std::thread::sleep(std::time::Duration::from_secs(30));
                    Ok(0)
                }
            }
            let relay_task = tokio::spawn(relay(Box::new(shim_side), None, Never, out.clone()));
            let (_daemon_r, mut daemon_w) = tokio::io::split(daemon);
            daemon_w.write_all(b"last words\n").await.unwrap();
            drop(daemon_w);
            drop(_daemon_r);
            assert_eq!(relay_task.await.unwrap(), Ended::DaemonClosed);
            assert_eq!(out.0.lock().unwrap().as_slice(), b"last words\n");
        });
    }

    #[test]
    fn a_failed_write_to_the_daemon_is_the_daemon_gone_not_the_harness_done() {
        rt().block_on(async {
            let (daemon, shim_side) = duplex(1 << 16);
            drop(daemon);
            let input = std::io::Cursor::new(SAMPLE.to_vec());
            let ended = relay(Box::new(shim_side), None, input, Shared::default()).await;
            assert_eq!(ended, Ended::DaemonClosed);
            assert_eq!(exit_status(ended), 1);
        });
    }

    #[test]
    fn the_hello_line_is_the_first_thing_on_the_stream_and_the_input_follows_it() {
        rt().block_on(async {
            let (daemon, shim_side) = duplex(1 << 16);
            let hello = hello_for("claude", Some("E-1".as_ref())).expect("a hello");
            let input = std::io::Cursor::new(SAMPLE.to_vec());
            let ended = relay(
                Box::new(shim_side),
                Some(hello.clone()),
                input,
                Shared::default(),
            )
            .await;
            assert_eq!(ended, Ended::InputClosed);
            let (mut daemon_r, _w) = tokio::io::split(daemon);
            let mut got = Vec::new();
            daemon_r.read_to_end(&mut got).await.unwrap();
            let mut expected = hello;
            expected.extend_from_slice(SAMPLE);
            assert_eq!(got, expected);
        });
    }

    #[test]
    fn only_a_claude_shim_with_a_usable_value_sends_a_hello() {
        use std::ffi::OsStr;
        let line = hello_for("claude", Some(OsStr::new("E-1"))).unwrap();
        assert_eq!(line, b"{\"oac_hello\":1,\"session_id\":\"E-1\"}\n");
        assert_eq!(hello_for("claude", None), None);
        assert_eq!(hello_for("claude", Some(OsStr::new(""))), None);
        assert_eq!(
            hello_for("claude", Some(OsStr::new(&"x".repeat(257)))),
            None
        );
        assert_eq!(hello_for("codex", Some(OsStr::new("E-1"))), None);
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            assert_eq!(
                hello_for("claude", Some(OsStr::from_bytes(b"\xff\xfe"))),
                None
            );
        }
    }

    #[test]
    fn exit_statuses_and_the_not_running_message() {
        assert_eq!(exit_status(Ended::InputClosed), 0);
        assert_eq!(exit_status(Ended::DaemonClosed), 1);
        let m = unreachable_message(&ConnectError::NotRunning);
        assert!(m.contains("`oac start`"), "{m}");
        assert!(unreachable_message(&ConnectError::Foreign).contains("another user"));
    }
}
