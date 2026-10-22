// SPDX-License-Identifier: Apache-2.0

//! A blocking byte stream as the halves `rmcp` reads and writes.
//!
//! The same shape as the Claude adapter's (`adapters/claude/src/io.rs`; adapters may not
//! depend on each other, 07 §3), without its first-line look: a Codex connection has no hook
//! input to tell apart.
//!
//! - **Reading.** A thread blocks on the stream's reader and passes each chunk on through a
//!   channel. It waits for bytes the harness writes; it never asks the harness for anything,
//!   so it is not polling (`docs/planning/v0.1/09-test-strategy.md` §5). End of file, or a
//!   read error, ends the stream.
//! - **Writing.** Each write goes straight to the stream's writer, and a flush is a flush of
//!   it.

use std::io::{self, Read, Write};
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

/// The reading half: chunks from the reading thread.
pub struct StreamReader {
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

impl StreamReader {
    /// Starts the reading thread over `reader` and returns the half that `rmcp` reads.
    ///
    /// # Errors
    ///
    /// The error of spawning the thread.
    pub fn spawn(mut reader: Box<dyn Read + Send>) -> io::Result<StreamReader> {
        let (tx, rx) = mpsc::unbounded_channel();
        std::thread::Builder::new()
            .name("oac-codex-mcp-read".into())
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
        Ok(StreamReader {
            rx,
            buf: Vec::new(),
            pos: 0,
        })
    }
}

impl AsyncRead for StreamReader {
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

/// The writing half: the stream's writer, written and flushed in place.
pub struct StreamWriter(pub Box<dyn Write + Send>);

impl AsyncWrite for StreamWriter {
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
