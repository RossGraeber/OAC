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
