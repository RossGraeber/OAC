// SPDX-License-Identifier: Apache-2.0

//! Core-owned cancellable byte streams (interfaces 0.4 section 5.2).
//!
//! Bytes arrive through a local relay's channel. No arbitrary blocking Read/Write
//! implementation can enter a Connection. Transfer commits under the closure lock;
//! a stalled writer waits outside it, and reads use a bounded wait. Close marks the
//! barrier first, wakes writers, then waits for pending operations to settle. It never
//! leaves an underlying writer running. OS peer authentication remains the caller's
//! responsibility (G9); possession of a channel is not authentication.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

/// Closure bound for the in-process relay backend, independent of peer progress.
pub const CLOSE_BOUND_MS: u64 = 100;
const READ_WAIT: Duration = Duration::from_millis(1);

#[derive(Default)]
struct State {
    pending: usize,
}
#[derive(Default)]
pub(crate) struct Closure {
    closed: AtomicBool,
    state: Mutex<State>,
    changed: Condvar,
}
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
impl Closure {
    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    pub(crate) fn begin_close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.changed.notify_all();
    }
    pub(crate) fn close(&self) {
        self.begin_close();
        let mut s = lock(&self.state);
        while s.pending != 0 {
            s = self.changed.wait(s).unwrap_or_else(|e| e.into_inner());
        }
    }
    fn begin(self: &Arc<Self>) -> Option<Pending> {
        let mut s = lock(&self.state);
        if self.closed.load(Ordering::SeqCst) {
            return None;
        }
        s.pending += 1;
        Some(Pending(self.clone()))
    }
}
struct Pending(Arc<Closure>);
impl Drop for Pending {
    fn drop(&mut self) {
        lock(&self.0.state).pending -= 1;
        self.0.changed.notify_all();
    }
}

/// Terminal byte transfer result; closure carries the exact transferred count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    /// All requested octets transferred before closure.
    Completed { transferred: usize },
    /// Closure stopped transfer; this is never a successful harness input call.
    Closed { transferred: usize },
}

/// The exact transferred count on the standard Write error caused by closure.
#[derive(Debug)]
pub struct ClosedWrite {
    /// Octets transferred by the interrupted write.
    pub transferred: usize,
}
impl std::fmt::Display for ClosedWrite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "connection closed; transferred {} octets",
            self.transferred
        )
    }
}
impl std::error::Error for ClosedWrite {}

/// A core-owned reading half. Close settles a pending read as end-of-stream.
pub struct ConnectionReader {
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
    pub(crate) closure: Arc<Closure>,
    pub(crate) attached: bool,
}
impl From<Receiver<Vec<u8>>> for ConnectionReader {
    fn from(rx: Receiver<Vec<u8>>) -> Self {
        Self {
            rx,
            buf: Vec::new(),
            pos: 0,
            closure: Arc::default(),
            attached: false,
        }
    }
}
impl Default for ConnectionReader {
    fn default() -> Self {
        Self::from(mpsc::channel().1)
    }
}
impl ConnectionReader {
    /// Close the connection, including its other direction.
    pub fn close(&self) {
        self.closure.close();
    }
}
impl Read for ConnectionReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let Some(_pending) = self.closure.begin() else {
            return Ok(0);
        };
        if out.is_empty() {
            return Ok(0);
        }
        loop {
            let s = lock(&self.closure.state);
            if self.closure.is_closed() {
                return Ok(0);
            }
            if self.pos < self.buf.len() {
                let n = out.len().min(self.buf.len() - self.pos);
                out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
                self.pos += n;
                return Ok(n);
            }
            drop(s);
            match self.rx.recv_timeout(READ_WAIT) {
                Ok(b) => {
                    self.buf = b;
                    self.pos = 0;
                }
                Err(RecvTimeoutError::Disconnected) => return Ok(0),
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

/// Controls relay backpressure. A paused writer waits until resume or connection close.
/// Used by relay flow control and by tests that model a peer that stopped reading.
#[derive(Clone)]
pub struct WriteControl(Arc<Flow>);
struct Flow {
    paused: AtomicBool,
    after: AtomicUsize,
    pending: AtomicUsize,
}
impl Default for WriteControl {
    fn default() -> Self {
        Self(Arc::new(Flow {
            paused: AtomicBool::new(false),
            after: AtomicUsize::new(usize::MAX),
            pending: AtomicUsize::new(0),
        }))
    }
}
struct FlowPending(WriteControl);
impl Drop for FlowPending {
    fn drop(&mut self) {
        self.0.0.pending.fetch_sub(1, Ordering::SeqCst);
    }
}
impl WriteControl {
    /// Number of relay transfers admitted and not yet settled.
    pub fn pending_transfers(&self) -> usize {
        self.0.pending.load(Ordering::SeqCst)
    }

    /// Pause/resume transfer; closure always takes priority over resume.
    pub fn pause(&self, paused: bool) {
        self.0.paused.store(paused, Ordering::SeqCst);
    }
    /// Backpressure after at least this many octets of a transfer (in 8192-octet chunks).
    pub fn pause_after(&self, octets: usize) {
        self.0.after.store(octets, Ordering::SeqCst);
    }
}

/// A core-owned writing half. Each chunk commits under the closure barrier; transfer reports the exact count.
/// It never wraps an arbitrary writer or launches detached I/O.
pub struct ConnectionWriter {
    tx: Sender<Vec<u8>>,
    control: WriteControl,
    pub(crate) closure: Arc<Closure>,
    pub(crate) attached: bool,
}
impl Default for ConnectionWriter {
    fn default() -> Self {
        Self {
            tx: mpsc::channel().0,
            control: WriteControl::default(),
            closure: Arc::default(),
            attached: false,
        }
    }
}
impl From<Sender<Vec<u8>>> for ConnectionWriter {
    fn from(tx: Sender<Vec<u8>>) -> Self {
        Self {
            tx,
            ..Self::default()
        }
    }
}
impl ConnectionWriter {
    /// The relay's backpressure control.
    pub fn control(&self) -> WriteControl {
        self.control.clone()
    }
    /// Close both directions through this stream half.
    pub fn close(&self) {
        self.closure.close();
    }
    /// Write with the explicit terminal result required by [IFC-ADP-016].
    pub fn transfer(&mut self, buf: &[u8]) -> io::Result<WriteOutcome> {
        let Some(_pending) = self.closure.begin() else {
            return Ok(WriteOutcome::Closed { transferred: 0 });
        };
        self.control.0.pending.fetch_add(1, Ordering::SeqCst);
        let _flow = FlowPending(self.control.clone());
        let mut transferred = 0;
        loop {
            let mut s = lock(&self.closure.state);
            while !self.closure.is_closed()
                && (self.control.0.paused.load(Ordering::SeqCst)
                    || transferred >= self.control.0.after.load(Ordering::SeqCst))
            {
                s = self
                    .closure
                    .changed
                    .wait_timeout(s, READ_WAIT)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
            if self.closure.is_closed() {
                return Ok(WriteOutcome::Closed { transferred });
            }
            let end = (transferred + 8192).min(buf.len());
            self.tx
                .send(buf[transferred..end].to_vec())
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "relay ended"))?;
            transferred = end;
            if transferred == buf.len() {
                return Ok(WriteOutcome::Completed { transferred });
            }
            drop(s);
        }
    }
}
impl Write for ConnectionWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.transfer(buf)? {
            WriteOutcome::Completed { transferred } => Ok(transferred),
            WriteOutcome::Closed { transferred } => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                ClosedWrite { transferred },
            )),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.closure.is_closed() {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "connection closed",
            ))
        } else {
            Ok(())
        }
    }
}

/// One relay direction; accepts only concrete cancellable halves.
pub fn pipe() -> (ConnectionWriter, ConnectionReader) {
    let (tx, rx) = mpsc::channel();
    (tx.into(), rx.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::Connection;
    use std::time::Instant;

    fn pending(h: &crate::adapter::ConnectionHandle, n: usize) {
        let end = Instant::now() + Duration::from_secs(5);
        while lock(&h.1.state).pending < n {
            assert!(Instant::now() < end);
            std::thread::yield_now();
        }
    }

    #[test]
    fn stalled_read_and_write_settle_within_bound_and_never_transfer_later() {
        let (_in_tx, in_rx) = mpsc::channel();
        let (out_tx, out_rx) = mpsc::channel();
        let writer = ConnectionWriter::from(out_tx);
        let flow = writer.control();
        flow.pause(true);
        let c = Connection::accept(in_rx.into(), writer);
        let (h, mut r, mut w) = c.into_streams();
        let read = std::thread::spawn(move || r.read(&mut [0; 8]));
        let write = std::thread::spawn(move || w.transfer(b"never"));
        pending(&h, 2);
        let start = Instant::now();
        h.close();
        assert!(start.elapsed() < Duration::from_millis(h.close_bound_ms()));
        assert_eq!(
            lock(&h.1.state).pending,
            0,
            "all operations settled at barrier"
        );
        assert_eq!(read.join().unwrap().unwrap(), 0);
        assert_eq!(
            write.join().unwrap().unwrap(),
            WriteOutcome::Closed { transferred: 0 }
        );
        flow.pause(false);
        assert!(out_rx.try_recv().is_err());
        h.close();
    }

    #[test]
    fn partial_and_completed_transfers_keep_the_exact_count() {
        let (tx, rx) = mpsc::channel();
        let w = ConnectionWriter::from(tx);
        let flow = w.control();
        flow.pause_after(8192);
        let c = Connection::accept(Default::default(), w);
        let (h, _, mut w) = c.into_streams();
        let t = std::thread::spawn(move || w.transfer(&[7; 16384]));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap().len(), 8192);
        h.close();
        assert_eq!(
            t.join().unwrap().unwrap(),
            WriteOutcome::Closed { transferred: 8192 }
        );
        flow.pause_after(usize::MAX);
        assert!(rx.try_recv().is_err());
        let (tx, _rx) = mpsc::channel();
        let c = Connection::accept(Default::default(), tx.into());
        let (h, _, mut w) = c.into_streams();
        assert_eq!(
            w.transfer(b"complete").unwrap(),
            WriteOutcome::Completed { transferred: 8 }
        );
        h.close();
        assert_eq!(
            w.transfer(b"later").unwrap(),
            WriteOutcome::Closed { transferred: 0 }
        );
        assert_eq!(
            w.write(b"later")
                .unwrap_err()
                .get_ref()
                .unwrap()
                .downcast_ref::<ClosedWrite>()
                .unwrap()
                .transferred,
            0
        );
    }

    #[test]
    fn concurrent_close_and_stream_halves_share_one_barrier_only() {
        let c = Connection::accept(Default::default(), Default::default());
        let other = Connection::accept(Default::default(), Default::default());
        let (h, r, w) = c.into_streams();
        let h2 = h.clone();
        let a = std::thread::spawn(move || h2.close());
        let b = std::thread::spawn(move || r.close());
        w.close();
        h.close();
        a.join().unwrap();
        b.join().unwrap();
        assert!(h.is_closed());
        assert!(!other.handle().is_closed());
    }

    #[test]
    fn close_racing_a_write_has_only_a_definite_terminal_result() {
        for _ in 0..50 {
            let (tx, rx) = mpsc::channel();
            let c = Connection::accept(Default::default(), tx.into());
            let (h, _, mut w) = c.into_streams();
            let t = std::thread::spawn(move || w.transfer(b"race"));
            h.close();
            match t.join().unwrap().unwrap() {
                WriteOutcome::Completed { transferred: 4 } => {
                    assert_eq!(rx.try_recv().unwrap(), b"race")
                }
                WriteOutcome::Closed { transferred: 0 } => assert!(rx.try_recv().is_err()),
                other => panic!("{other:?}"),
            }
        }
    }
}

#[cfg(test)]
mod barrier_tests {
    use super::*;
    use crate::adapter::Connection;
    #[test]
    fn closure_begun_refuses_new_operations_even_before_close_waits() {
        let (tx, rx) = mpsc::channel();
        let c = Connection::accept(Default::default(), tx.into());
        let (h, mut r, mut w) = c.into_streams();
        h.begin_close();
        assert_eq!(
            w.transfer(b"no").unwrap(),
            WriteOutcome::Closed { transferred: 0 }
        );
        assert_eq!(r.read(&mut [0; 8]).unwrap(), 0);
        assert!(rx.try_recv().is_err());
        h.close();
    }
    #[test]
    fn an_io_failure_before_close_retains_its_failure() {
        let c = Connection::accept(Default::default(), Default::default());
        let (h, _, mut w) = c.into_streams();
        let error = w.transfer(b"gone").unwrap_err();
        h.close();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(error.to_string(), "relay ended");
        assert!(
            error
                .get_ref()
                .unwrap()
                .downcast_ref::<ClosedWrite>()
                .is_none()
        );
    }
    #[test]
    fn previously_issued_halves_cannot_change_their_closure_state() {
        let c = Connection::accept(Default::default(), Default::default());
        let (h, r, w) = c.into_streams();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Connection::accept(r, w)))
                .is_err()
        );
        h.close();
        assert!(h.is_closed());
    }
}
