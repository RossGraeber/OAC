// SPDX-License-Identifier: Apache-2.0

//! Moving the fake's frames over a byte stream, framed as Claude Code frames stdio MCP:
//! one JSON message per line (G1 Box C line 12, "framing detected: ndjson"). Use it with a
//! child process's stdin and stdout, or with any in-memory pipe. Blocking reads only: the
//! fake never asks the server whether anything is pending.

use std::io::{self, BufRead, Write};

use crate::FakeClaude;

/// Writes every frame the fake has queued, one per line, and flushes. Returns the count.
pub fn write_outbound<W: Write>(fake: &mut FakeClaude, w: &mut W) -> io::Result<usize> {
    let frames = fake.take_outbound();
    for f in &frames {
        w.write_all(f.as_bytes())?;
        w.write_all(b"\n")?;
    }
    w.flush()?;
    Ok(frames.len())
}

/// Reads one line, without its terminator (`\n` or `\r\n`). `None` at end of stream.
pub fn read_frame<R: BufRead>(r: &mut R) -> io::Result<Option<String>> {
    let mut line = String::new();
    if r.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    Ok(Some(line))
}

/// Reads one frame into the fake. `false` at end of stream.
pub fn receive_one<R: BufRead>(fake: &mut FakeClaude, r: &mut R) -> io::Result<bool> {
    match read_frame(r)? {
        Some(f) => {
            fake.receive(&f);
            Ok(true)
        }
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, Phase};
    use std::io::Cursor;

    #[test]
    fn frames_round_trip_as_lines() {
        let mut fake = FakeClaude::new(Config::new("s").expect("valid"));
        let mut out = Vec::new();
        assert_eq!(write_outbound(&mut fake, &mut out).expect("write"), 1);
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.ends_with('\n') && text.matches('\n').count() == 1);
        assert!(text.contains("\"server/discover\""));

        let mut input = Cursor::new(
            b"{\"jsonrpc\":\"2.0\",\"id\":\"server-discover-probe-1\",\"error\":{\"code\":-32601,\"message\":\"m\"}}\r\n".to_vec(),
        );
        assert!(receive_one(&mut fake, &mut input).expect("read"));
        assert_eq!(fake.phase(), Phase::Initializing);
        assert!(!receive_one(&mut fake, &mut input).expect("eof"));
    }
}
