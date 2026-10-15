// SPDX-License-Identifier: Apache-2.0

//! The relay between an authenticated local IPC stream and the core's `Connection`
//! (G9, #70; `spec/interfaces.md` §5.2, [IFC-ADP-012]).
//!
//! The core accepts only core-owned, cancellable channel halves
//! (`oac_core::connection`): possession of a channel is not authentication, so the daemon
//! authenticates the peer first (`crate::ipc`) and only then relays its stream through them.
//! [`attach`] makes the two directions, hands the halves to `Connection::accept`, and runs the
//! relay on the daemon's runtime:
//!
//! - **Inbound** (peer to adapter): an async read task, a bounded queue, and a thread that
//!   blocks on a bounded channel into the connection's reader. When the adapter reads slowly
//!   the queue fills, the read task stops reading, and the peer is held back by its own
//!   operating system: memory stays bounded however much a peer sends.
//! - **Outbound** (adapter to peer): a thread that takes the adapter's writes and an async
//!   write task. When the peer reads slowly the queue fills and the connection's writer is
//!   paused (`WriteControl`), so the adapter's write waits and its hand-off is reported
//!   `indeterminate` rather than buffered without bound ([IFC-ADP-053]). The writer is
//!   resumed only once everything it had already handed over is in the queue, so pausing and
//!   resuming again does not pile a backlog up. **The bound is not a hard one:** the core's
//!   writer hands chunks to an unbounded channel and looks at the pause flag once per chunk,
//!   so what the adapter writes between the queue filling and this thread pausing it (the
//!   thread's wake-up latency) is held in memory, once per stall, never more. A hard bound
//!   needs a credit or bounded writer in the core (recorded in `11-risks.md`, RISK-LOCAL-IPC).
//!
//! **End of connection.** End of stream or a broken pipe on the stream is how the daemon
//! learns a connection ended (C2 §5): never a timeout of its own. The end reaches the adapter
//! as end of stream, *after* everything the peer sent: a hook handler writes one line and
//! leaves at once, and the adapter must still read the line. When the adapter has finished
//! with the connection and dropped its halves (or the stream failed), `on_end` runs exactly
//! once; the daemon passes `Pipelines::disconnect` to it, which frees the connection's place
//! and, for an attachment, ends its binding ([SC-ID-155]). Calling `disconnect` first would
//! remove what the core observed about the peer before the adapter reported the line's native
//! signal, and lose it. An adapter that does not finish within [`DRAIN`] of end of stream is
//! not waited for: the end *was* observed, and `on_end` runs then.

use std::sync::mpsc::{self, Receiver, sync_channel};
use std::time::Duration;

use oac_core::adapter::{Connection, ConnectionHandle};
use oac_core::connection::{ConnectionReader, ConnectionWriter, WriteControl};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::runtime::Handle;
use tokio::sync::mpsc as tokio_mpsc;

use crate::ipc::AsyncStream;

/// The size of one read from the stream.
const CHUNK: usize = 8192;
/// Chunks queued in each of the two inbound stages.
const INBOUND_QUEUE: usize = 8;
/// Chunks queued between the adapter's writes and the stream.
const OUTBOUND_QUEUE: usize = 16;
/// How long the adapter has to finish with a connection after its stream ended.
pub const DRAIN: Duration = Duration::from_secs(5);

type OnEnd = Box<dyn FnOnce(&ConnectionHandle) + Send>;

/// Relays `stream` through a new core `Connection`, which the caller gives to
/// `Pipelines::connect_observed`. `on_end` runs once, when the connection has ended (see the
/// module documentation).
pub fn attach(runtime: &Handle, stream: Box<dyn AsyncStream>, on_end: OnEnd) -> Connection {
    attach_with(runtime, stream, on_end, DRAIN)
}

/// [`attach`], with the adapter's drain time given. For the tests of the bound.
pub fn attach_with(
    runtime: &Handle,
    stream: Box<dyn AsyncStream>,
    on_end: OnEnd,
    drain: Duration,
) -> Connection {
    let (to_adapter, adapter_reads) = sync_channel::<Vec<u8>>(INBOUND_QUEUE);
    let (adapter_writes, from_adapter) = mpsc::channel::<Vec<u8>>();
    let writer = ConnectionWriter::from(adapter_writes);
    let control = writer.control();
    let connection = Connection::accept(ConnectionReader::from(adapter_reads), writer);
    let handle = connection.handle().clone();

    let (read_half, write_half) = tokio::io::split(stream);
    let (read_queue_tx, read_queue_rx) = tokio_mpsc::channel::<Vec<u8>>(INBOUND_QUEUE);
    let (write_queue_tx, write_queue_rx) = tokio_mpsc::channel::<Vec<u8>>(OUTBOUND_QUEUE);

    // The adapter's side, on threads: they block on the bounded channels the adapter's
    // blocking halves share.
    let _ = std::thread::Builder::new()
        .name("oac-relay-in".into())
        .spawn(move || forward_inbound(read_queue_rx, to_adapter));
    let _ = std::thread::Builder::new()
        .name("oac-relay-out".into())
        .spawn(move || forward_outbound(from_adapter, write_queue_tx, control));

    let reading = runtime.spawn(read_stream(read_half, read_queue_tx));
    let writing = runtime.spawn(write_stream(write_half, write_queue_rx));
    runtime.spawn(supervise(reading, writing, handle, on_end, drain));
    connection
}

/// Reads the stream into the first inbound queue until it ends. Dropping the queue's sender
/// at the end is what tells the forwarding thread, and so the adapter, that the stream ended.
async fn read_stream<R: tokio::io::AsyncRead + Unpin>(
    mut stream: R,
    queue: tokio_mpsc::Sender<Vec<u8>>,
) {
    let mut chunk = vec![0u8; CHUNK];
    loop {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                // A full queue holds this task, and the stream goes unread.
                if queue.send(chunk[..n].to_vec()).await.is_err() {
                    return;
                }
            }
        }
    }
}

/// Moves chunks from the first inbound queue to the adapter's reader, blocking when the
/// adapter is not reading. Returning drops the sender: end of stream for the adapter.
fn forward_inbound(
    mut queue: tokio_mpsc::Receiver<Vec<u8>>,
    to_adapter: mpsc::SyncSender<Vec<u8>>,
) {
    while let Some(chunk) = queue.blocking_recv() {
        if to_adapter.send(chunk).is_err() {
            // The adapter dropped its reader.
            return;
        }
    }
}

/// Moves the adapter's writes to the write queue. A full queue pauses the connection's
/// writer; it is resumed only when the chunk that did not fit **and everything the adapter
/// had already handed over meanwhile** are in the queue (the pause flag is looked at once per
/// chunk by the writer, so those are the chunks written before it took effect). Returning
/// drops the sender, which ends the write task: the adapter dropped its writer, or the stream
/// failed.
fn forward_outbound(
    from_adapter: Receiver<Vec<u8>>,
    queue: tokio_mpsc::Sender<Vec<u8>>,
    control: WriteControl,
) {
    forward_chunks(&from_adapter, &queue, &control);
    // However this ended, a writer still held back must not wait for a relay that is gone:
    // closed behind it, its next write fails instead of waiting for ever.
    drop(from_adapter);
    control.pause(false);
}

fn forward_chunks(
    from_adapter: &Receiver<Vec<u8>>,
    queue: &tokio_mpsc::Sender<Vec<u8>>,
    control: &WriteControl,
) {
    while let Ok(chunk) = from_adapter.recv() {
        match queue.try_send(chunk) {
            Ok(()) => {}
            Err(tokio_mpsc::error::TrySendError::Full(chunk)) => {
                control.pause(true);
                if queue.blocking_send(chunk).is_err() {
                    return;
                }
                // Move the backlog while the writer is held; resume it only when none is left.
                loop {
                    match from_adapter.try_recv() {
                        Ok(chunk) => {
                            if queue.blocking_send(chunk).is_err() {
                                return;
                            }
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => return,
                    }
                }
                control.pause(false);
            }
            Err(tokio_mpsc::error::TrySendError::Closed(_)) => return,
        }
    }
}

/// Writes the queue to the stream until the queue's sender is gone or a write fails; then
/// closes the stream's write side, so the peer reads end of stream.
async fn write_stream<W: tokio::io::AsyncWrite + Unpin>(
    mut stream: W,
    mut queue: tokio_mpsc::Receiver<Vec<u8>>,
) {
    while let Some(chunk) = queue.recv().await {
        if stream.write_all(&chunk).await.is_err() || stream.flush().await.is_err() {
            return;
        }
    }
    let _ = stream.shutdown().await;
}

/// Waits for the connection to end and reports it once.
async fn supervise(
    mut reading: tokio::task::JoinHandle<()>,
    mut writing: tokio::task::JoinHandle<()>,
    handle: ConnectionHandle,
    on_end: OnEnd,
    drain: Duration,
) {
    tokio::select! {
        // The adapter has finished with the connection, or the stream broke: nothing more
        // can be written, so stop reading too.
        _ = &mut writing => {
            reading.abort();
        }
        // End of stream, or a read error: the adapter is told (the inbound queue closes)
        // and given [`DRAIN`] to finish with what the peer sent.
        _ = &mut reading => {
            let _ = tokio::time::timeout(drain, &mut writing).await;
            writing.abort();
        }
    }
    on_end(&handle);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    use tokio::io::duplex;

    /// A current-thread runtime driven by a thread of its own, as the daemon's is, with a way
    /// to run a future from a test thread and wait for it.
    struct Rt {
        handle: Handle,
        stop: Option<tokio::sync::oneshot::Sender<()>>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Rt {
        fn new() -> Rt {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let handle = rt.handle().clone();
            let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
            let thread = std::thread::spawn(move || {
                rt.block_on(async {
                    let _ = stopped.await;
                });
            });
            Rt {
                handle,
                stop: Some(stop),
                thread: Some(thread),
            }
        }

        fn run<T: Send + 'static>(&self, f: impl Future<Output = T> + Send + 'static) -> T {
            let (tx, rx) = mpsc::channel();
            self.handle.spawn(async move {
                let _ = tx.send(f.await);
            });
            rx.recv().unwrap()
        }

        fn handle(&self) -> &Handle {
            &self.handle
        }
    }

    impl Drop for Rt {
        fn drop(&mut self) {
            let _ = self.stop.take().unwrap().send(());
            let _ = self.thread.take().unwrap().join();
        }
    }

    /// An `on_end` that counts its calls.
    fn counter() -> (Arc<Mutex<u32>>, OnEnd) {
        let n = Arc::new(Mutex::new(0));
        let c = n.clone();
        (
            n,
            Box::new(move |_| {
                *c.lock().unwrap() += 1;
            }),
        )
    }

    fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
        let end = std::time::Instant::now() + Duration::from_secs(10);
        while !f() {
            assert!(std::time::Instant::now() < end, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn bytes_go_both_ways_and_the_end_is_reported_once_after_the_adapter_lets_go() {
        let rt = Rt::new();
        let (peer, daemon_side) = duplex(1 << 16);
        let (n, on_end) = counter();
        let connection = attach(rt.handle(), Box::new(daemon_side), on_end);
        let (_handle, mut reads, mut writes) = connection.into_parts();
        let (mut peer_r, mut peer_w) = tokio::io::split(peer);

        // Peer to adapter.
        let peer_w = rt.run(async move {
            peer_w.write_all(b"hello adapter\n").await.unwrap();
            peer_w
        });
        let mut got = [0u8; 14];
        reads.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"hello adapter\n");
        // Adapter to peer.
        writes.write_all(b"hello peer\n").unwrap();
        let (back, peer_r) = rt.run(async move {
            let mut back = [0u8; 11];
            peer_r.read_exact(&mut back).await.unwrap();
            (back, peer_r)
        });
        assert_eq!(&back, b"hello peer\n");

        // The peer leaves: the adapter reads end of stream; `on_end` waits for it to let go.
        let mut peer_w = peer_w;
        rt.run(async move { peer_w.shutdown().await.unwrap() });
        let mut rest = Vec::new();
        reads.read_to_end(&mut rest).unwrap();
        assert!(rest.is_empty());
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            *n.lock().unwrap(),
            0,
            "the adapter still holds the connection"
        );
        drop((reads, writes));
        wait_for("the end is reported", || *n.lock().unwrap() == 1);
        // And the peer reads end of stream.
        let mut peer_r = peer_r;
        let tail = rt.run(async move {
            let mut tail = Vec::new();
            peer_r.read_to_end(&mut tail).await.unwrap();
            tail
        });
        assert!(tail.is_empty());
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(*n.lock().unwrap(), 1, "exactly once");
    }

    /// A hook handler writes one line and leaves at once; the adapter must still read it.
    #[test]
    fn what_the_peer_sent_before_leaving_reaches_the_adapter() {
        let rt = Rt::new();
        let (peer, daemon_side) = duplex(1 << 16);
        let (n, on_end) = counter();
        let connection = attach(rt.handle(), Box::new(daemon_side), on_end);
        let (_h, mut reads, writes) = connection.into_parts();
        rt.run(async move {
            let mut peer = peer;
            peer.write_all(b"{\"hook_event_name\":\"SessionStart\"}\n")
                .await
                .unwrap();
            drop(peer);
        });
        // The adapter is slow: the line and the end are both queued before it reads.
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(
            *n.lock().unwrap(),
            0,
            "not reported while the adapter holds it"
        );
        let mut all = Vec::new();
        reads.read_to_end(&mut all).unwrap();
        assert_eq!(all, b"{\"hook_event_name\":\"SessionStart\"}\n");
        drop((reads, writes));
        wait_for("the end is reported", || *n.lock().unwrap() == 1);
    }

    #[test]
    fn an_adapter_that_does_not_finish_is_not_waited_for_forever() {
        let rt = Rt::new();
        let (peer, daemon_side) = duplex(1 << 10);
        let (n, on_end) = counter();
        let connection = attach_with(
            rt.handle(),
            Box::new(daemon_side),
            on_end,
            Duration::from_millis(150),
        );
        drop(peer);
        // The adapter never reads and never lets go: the end was observed, and is reported
        // once the drain bound has passed.
        wait_for("reported after the drain bound", || *n.lock().unwrap() == 1);
        drop(connection);
    }

    #[test]
    fn the_adapter_closing_the_connection_closes_the_stream_and_is_reported() {
        let rt = Rt::new();
        let (peer, daemon_side) = duplex(1 << 16);
        let (n, on_end) = counter();
        let connection = attach(rt.handle(), Box::new(daemon_side), on_end);
        let (_h, reads, writes) = connection.into_parts();
        // A carrier: the adapter reads what it needs and closes.
        drop((reads, writes));
        wait_for("reported", || *n.lock().unwrap() == 1);
        let rest = rt.run(async move {
            let (mut peer_r, _peer_w) = tokio::io::split(peer);
            let mut rest = Vec::new();
            peer_r.read_to_end(&mut rest).await.unwrap();
            rest
        });
        assert!(rest.is_empty(), "the peer sees end of stream");
    }

    /// Chunks the adapter tries to write to a peer that reads nothing.
    const WRITES: usize = 50_000;

    #[test]
    fn a_peer_that_does_not_read_pauses_the_adapters_writes_instead_of_growing_memory() {
        let rt = Rt::new();
        // A small pipe nobody reads: the write task stalls, the queue fills, the writer
        // pauses.
        let (peer, daemon_side) = duplex(1 << 10);
        let (_n, on_end) = counter();
        let connection = attach(rt.handle(), Box::new(daemon_side), on_end);
        let (_h, reads, mut writes) = connection.into_parts();
        let block = vec![7u8; CHUNK];
        let written = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let progress = written.clone();
        let writer = std::thread::spawn(move || {
            // Far more than the queues and the pipe hold, and more than any wake-up delay
            // of the forwarding thread lets a writer produce before it is paused.
            for _ in 0..WRITES {
                if writes.write_all(&block).is_err() {
                    return false;
                }
                progress.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            true
        });
        std::thread::sleep(Duration::from_millis(400));
        assert!(
            !writer.is_finished(),
            "the writes are held back while the peer reads nothing"
        );
        // Held back, not slow: once paused, the writer makes no progress at all.
        let first = written.load(std::sync::atomic::Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(300));
        let second = written.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(first, second, "the writer is paused, not merely slow");
        assert!(second < WRITES / 2, "it got {second} of {WRITES} chunks in");
        // The peer goes away: the held writes fail instead of waiting (and nothing was lost
        // to a hang).
        drop(peer);
        assert!(!writer.join().unwrap());
        drop(reads);
    }

    #[test]
    fn a_writer_that_was_paused_goes_on_when_the_peer_reads_and_nothing_is_lost() {
        let rt = Rt::new();
        let (peer, daemon_side) = duplex(1 << 10);
        let (_n, on_end) = counter();
        let connection = attach(rt.handle(), Box::new(daemon_side), on_end);
        let (_h, reads, mut writes) = connection.into_parts();
        let block = vec![7u8; CHUNK];
        let writer = std::thread::spawn(move || {
            for _ in 0..200 {
                if writes.write_all(&block).is_err() {
                    return false;
                }
            }
            true
        });
        // (Whether the writer is held back by now is the stall test's; this one is about
        // what happens when it resumes.) The peer starts reading after the writer has had time to stall.
        std::thread::sleep(Duration::from_millis(100));
        let total = rt.run(async move {
            let (mut r, _w) = tokio::io::split(peer);
            let mut sink = vec![0u8; 1 << 16];
            let mut total = 0usize;
            while total < 200 * CHUNK {
                match r.read(&mut sink).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => total += n,
                }
            }
            total
        });
        assert!(writer.join().unwrap());
        assert_eq!(total, 200 * CHUNK);
        drop(reads);
    }

    #[test]
    fn a_peer_that_sends_without_the_adapter_reading_is_held_back() {
        let rt = Rt::new();
        let (peer, daemon_side) = duplex(1 << 10);
        let (_n, on_end) = counter();
        let connection = attach(rt.handle(), Box::new(daemon_side), on_end);
        let (_h, mut reads, writes) = connection.into_parts();
        let done = Arc::new(Mutex::new(false));
        let flag = done.clone();
        rt.handle().spawn(async move {
            let mut peer = peer;
            let chunk = vec![1u8; CHUNK];
            for _ in 0..400 {
                if peer.write_all(&chunk).await.is_err() {
                    break;
                }
            }
            *flag.lock().unwrap() = true;
        });
        std::thread::sleep(Duration::from_millis(400));
        assert!(
            !*done.lock().unwrap(),
            "the sender is held back by bounded queues"
        );
        // The adapter starts reading: everything arrives.
        let mut total = 0usize;
        let mut buf = vec![0u8; 1 << 16];
        while total < 400 * CHUNK {
            let n = reads.read(&mut buf).unwrap();
            assert!(n > 0);
            total += n;
        }
        assert_eq!(total, 400 * CHUNK);
        wait_for("the sender finishes", || *done.lock().unwrap());
        drop(writes);
    }
}
