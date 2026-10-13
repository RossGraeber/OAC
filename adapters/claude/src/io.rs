// SPDX-License-Identifier: Apache-2.0

//! The core-issued `Connection` as the byte stream `rmcp` reads and writes.
//!
//! A `Connection` carries blocking `std::io` halves ([IFC-ADP-012]: the core accepted and
//! authenticated it; the adapter never opens one, [IFC-ADP-013]). `rmcp`'s
//! `transport-async-rw` wants `tokio` halves, and the adapter takes no `tokio` I/O driver
//! (G-7 §5: no `net`, `fs` or `process`). So:
//!
//! - **Reading.** A thread blocks on the connection's reader and passes each chunk on through
//!   a channel. It waits for bytes the harness writes; it never asks the harness for
//!   anything, so it is not polling (`docs/planning/v0.1/09-test-strategy.md` §5). End of
//!   file, or a read error, ends the stream.
//! - **Writing.** Each write goes straight to the connection's writer, and a flush is a
//!   flush of it. So when `rmcp` reports a frame sent, its bytes have been written: the
//!   strongest fact the channel surface gives, since Claude Code acknowledges nothing
//!   (`spec/bindings/mcp.md` §8.1). The write blocks the connection's own runtime thread
//!   only; the reading thread keeps draining the harness meanwhile, so neither side can wait
//!   on the other.

use std::io::{self, Read, Write};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

/// The reading half: chunks from the reading thread.
pub struct ChannelReader {
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

impl ChannelReader {
    /// Starts the reading thread over `reader` and returns the half that `rmcp` reads.
    pub fn spawn(mut reader: Box<dyn Read + Send>) -> io::Result<ChannelReader> {
        let (tx, rx) = mpsc::unbounded_channel();
        std::thread::Builder::new()
            .name("oac-claude-read".into())
            .spawn(move || {
                let mut chunk = vec![0u8; 8192];
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if tx.send(chunk[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
            })?;
        Ok(ChannelReader {
            rx,
            buf: Vec::new(),
            pos: 0,
        })
    }
}

/// How the first line of a connection ended ([`ChannelReader::peek_line`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Head {
    /// A complete line, newline-terminated or ended by end of file.
    Line,
    /// The stream ended with nothing on it.
    Empty,
    /// More than the limit with no newline.
    TooLong,
    /// Some bytes arrived and the line was not complete within the stall bound of the first.
    Stalled,
}

impl ChannelReader {
    /// Waits for the first line of the stream without consuming it: the bytes stay in the
    /// reader and `rmcp` reads them as usual afterwards. It waits for bytes the peer writes
    /// (no request is made of it, so it is not polling). Nothing is bounded until the first
    /// byte arrives, as for any idle connection; from then on the whole line must be complete
    /// within `stall` of that first byte (one deadline for the line, not one per chunk, so a
    /// peer that trickles bytes cannot hold the place) and fit in `limit`.
    pub async fn peek_line(&mut self, limit: usize, stall: Duration) -> Head {
        // Nothing has been consumed yet: `buf` holds exactly what has been received.
        debug_assert_eq!(self.pos, 0);
        let mut deadline = None;
        loop {
            if self.buf.contains(&b'\n') {
                return Head::Line;
            }
            if self.buf.len() > limit {
                return Head::TooLong;
            }
            let next = match deadline {
                None => self.rx.recv().await,
                Some(deadline) => match tokio::time::timeout_at(deadline, self.rx.recv()).await {
                    Ok(next) => next,
                    Err(_) => return Head::Stalled,
                },
            };
            match next {
                Some(chunk) => {
                    if deadline.is_none() && !chunk.is_empty() {
                        deadline = Some(tokio::time::Instant::now() + stall);
                    }
                    self.buf.extend_from_slice(&chunk);
                }
                None if self.buf.is_empty() => return Head::Empty,
                None => return Head::Line,
            }
        }
    }

    /// The bytes received so far and not yet read, after [`ChannelReader::peek_line`]: the
    /// first line, and whatever followed it in the same chunk.
    pub fn received(&self) -> &[u8] {
        &self.buf[self.pos..]
    }
}

impl AsyncRead for ChannelReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let me = &mut *self;
        while me.pos >= me.buf.len() {
            match me.rx.poll_recv(cx) {
                Poll::Ready(Some(b)) => {
                    me.buf = b;
                    me.pos = 0;
                }
                // End of file: the harness closed its side.
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Pending => return Poll::Pending,
            }
        }
        let n = out.remaining().min(me.buf.len() - me.pos);
        out.put_slice(&me.buf[me.pos..me.pos + n]);
        me.pos += n;
        Poll::Ready(Ok(()))
    }
}

/// The writing half: the connection's writer, written and flushed in place.
pub struct ChannelWriter(pub Box<dyn Write + Send>);

impl AsyncWrite for ChannelWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(self.0.write(buf))
    }

    fn poll_flush(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(self.0.flush())
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(self.0.flush())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// A reader fed by the test, and its feed. Dropping the feed is end of file.
    fn reader() -> (mpsc::Sender<Vec<u8>>, ChannelReader) {
        struct Fed(mpsc::Receiver<Vec<u8>>, Vec<u8>);
        impl Read for Fed {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                if self.1.is_empty() {
                    match self.0.recv() {
                        Ok(b) => self.1 = b,
                        Err(_) => return Ok(0),
                    }
                }
                let n = out.len().min(self.1.len());
                out[..n].copy_from_slice(&self.1[..n]);
                self.1.drain(..n);
                Ok(n)
            }
        }
        let (tx, rx) = mpsc::channel();
        let r = ChannelReader::spawn(Box::new(Fed(rx, Vec::new()))).expect("a reader");
        (tx, r)
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("a runtime")
    }

    #[test]
    fn the_first_line_is_looked_at_and_still_read_afterwards() {
        use tokio::io::AsyncReadExt;
        rt().block_on(async {
            let (tx, mut r) = reader();
            // A line that arrives in two chunks, with the next frame's start behind it.
            tx.send(b"{\"a\":".to_vec()).unwrap();
            tx.send(b"1}\n{\"b\"".to_vec()).unwrap();
            assert_eq!(r.peek_line(1024, Duration::from_secs(5)).await, Head::Line);
            assert!(r.received().starts_with(b"{\"a\":1}\n"));
            drop(tx);
            let mut all = Vec::new();
            r.read_to_end(&mut all).await.unwrap();
            assert_eq!(all, b"{\"a\":1}\n{\"b\"", "nothing was consumed by looking");
        });
    }

    #[test]
    fn end_of_file_ends_a_line_and_an_empty_stream_is_empty() {
        rt().block_on(async {
            let (tx, mut r) = reader();
            tx.send(b"{\"a\":1}".to_vec()).unwrap();
            drop(tx);
            assert_eq!(r.peek_line(1024, Duration::from_secs(5)).await, Head::Line);
            let (tx, mut r) = reader();
            drop(tx);
            assert_eq!(r.peek_line(1024, Duration::from_secs(5)).await, Head::Empty);
        });
    }

    /// One deadline for the whole line: a peer that sends a byte now and then, each inside the
    /// bound, still runs out the bound counted from its first byte.
    #[test]
    fn a_line_that_trickles_runs_out_one_deadline() {
        rt().block_on(async {
            let (tx, mut r) = reader();
            let feeder = std::thread::spawn(move || {
                for _ in 0..40 {
                    if tx.send(b"x".to_vec()).is_err() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            });
            let start = std::time::Instant::now();
            assert_eq!(
                r.peek_line(1024, Duration::from_millis(150)).await,
                Head::Stalled
            );
            assert!(
                start.elapsed() < Duration::from_millis(600),
                "bounded from the first byte, not renewed by each chunk: {:?}",
                start.elapsed()
            );
            drop(r);
            feeder.join().unwrap();
        });
    }

    #[test]
    fn a_line_that_stalls_or_overruns_is_reported_and_an_idle_stream_waits() {
        rt().block_on(async {
            // Some bytes, then nothing: stalled.
            let (tx, mut r) = reader();
            tx.send(b"{\"a\":".to_vec()).unwrap();
            assert_eq!(
                r.peek_line(1024, Duration::from_millis(50)).await,
                Head::Stalled
            );
            // Too many bytes with no newline.
            let (tx2, mut r2) = reader();
            tx2.send(vec![b'x'; 2048]).unwrap();
            assert_eq!(
                r2.peek_line(1024, Duration::from_secs(5)).await,
                Head::TooLong
            );
            // Nothing at all is not a stall: the wait has no bound until a byte arrives.
            let (tx3, mut r3) = reader();
            let waited = tokio::time::timeout(
                Duration::from_millis(200),
                r3.peek_line(1024, Duration::from_millis(10)),
            )
            .await;
            assert!(waited.is_err(), "an idle connection is not a stalled line");
            drop((tx, tx2, tx3));
        });
    }
}
