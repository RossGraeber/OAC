// SPDX-License-Identifier: Apache-2.0

//! A minimal RFC 6455 WebSocket client over a byte stream that someone else opened.
//!
//! D8 (`docs/planning/decisions/G-7-stage4-dependencies.md` §8.3): `cli/` connects to the
//! app-server's control socket and hands the adapter a connected stream; the adapter does
//! the HTTP Upgrade and the framing itself, with no WebSocket crate. The pattern is the
//! minimal client of `tests/protocol/contract/adapter/tests/stand_in.rs`, extended with
//! what a long-lived client needs: the `Sec-WebSocket-Accept` check, masked client frames,
//! fragmented messages, ping/pong, the close handshake and a message size cap.
//!
//! The control socket proxies raw bytes, so the client must do the Upgrade and frame its
//! messages itself (`oac-codex-appserver`, "The daemon's control socket is WebSocket over
//! UDS, not JSONL").
//!
//! No extension is negotiated, so every frame with a reserved bit set is refused. Only text
//! messages are accepted: the app-server sends JSON-RPC as text.

use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The largest message accepted: 16 MiB, the limit the recorded handshake advertises in
/// `x-codex-websocket-max-unfragmented-message-bytes`
/// (`docs/planning/gates/fixtures/d6-codex-protocol/`, line 2 of every transcript).
pub const MAX_MESSAGE: usize = 16 * 1024 * 1024;

/// The longest HTTP response head accepted during the Upgrade.
const MAX_HEAD: usize = 16 * 1024;

/// The GUID of RFC 6455 §1.3, appended to the key for `Sec-WebSocket-Accept`.
const ACCEPT_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Why the WebSocket layer stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WsError {
    /// The stream failed (its text).
    Io(String),
    /// The Upgrade was not answered as RFC 6455 requires (why).
    Handshake(String),
    /// The peer broke the framing rules (why).
    Protocol(String),
    /// A message was larger than [`MAX_MESSAGE`].
    TooLarge,
    /// The peer closed the connection, or the stream ended.
    Closed,
}

impl fmt::Display for WsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WsError::Io(e) => write!(f, "stream error: {e}"),
            WsError::Handshake(e) => write!(f, "WebSocket upgrade refused: {e}"),
            WsError::Protocol(e) => write!(f, "WebSocket protocol error: {e}"),
            WsError::TooLarge => write!(f, "WebSocket message over {MAX_MESSAGE} bytes"),
            WsError::Closed => f.write_str("WebSocket closed"),
        }
    }
}

impl From<io::Error> for WsError {
    fn from(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            WsError::Closed
        } else {
            WsError::Io(e.to_string())
        }
    }
}

/// Bytes that need not be secret but must vary: the Upgrade key and the frame masks.
///
/// RFC 6455 asks for an unpredictable masking key, to stop a client's payload from steering
/// an intermediary's cache (§10.3). The carrier is a local socket with no intermediary, so
/// the std hasher's per-process random seed, stirred with a counter, is enough here. Nothing
/// that authenticates anything is drawn from it.
struct Noise {
    seed: RandomState,
    counter: AtomicU64,
}

impl Noise {
    fn new() -> Noise {
        Noise {
            seed: RandomState::new(),
            counter: AtomicU64::new(0),
        }
    }

    fn next_u64(&self) -> u64 {
        let mut h = self.seed.build_hasher();
        h.write_u64(self.counter.fetch_add(1, Ordering::Relaxed));
        h.finish()
    }

    fn mask(&self) -> [u8; 4] {
        let v = self.next_u64().to_le_bytes();
        [v[0], v[1], v[2], v[3]]
    }

    fn key(&self) -> [u8; 16] {
        let mut k = [0u8; 16];
        k[..8].copy_from_slice(&self.next_u64().to_le_bytes());
        k[8..].copy_from_slice(&self.next_u64().to_le_bytes());
        k
    }
}

/// The writing half: sends masked frames. Shared by the reader (to answer pings and
/// closes) and by the callers.
#[derive(Clone)]
pub struct WsWriter {
    inner: Arc<Mutex<WriterState>>,
    noise: Arc<Noise>,
}

struct WriterState {
    stream: Box<dyn Write + Send>,
    closed: bool,
}

impl WsWriter {
    /// Send one text message.
    ///
    /// # Errors
    ///
    /// [`WsError::Closed`] after a close was sent; otherwise the stream's error.
    pub fn send_text(&self, text: &str) -> Result<(), WsError> {
        if text.len() > MAX_MESSAGE {
            return Err(WsError::TooLarge);
        }
        self.send(0x1, text.as_bytes())
    }

    /// Send a close frame (status 1000) once; later sends fail with [`WsError::Closed`].
    pub fn close(&self) {
        let _ = self.send(0x8, &1000u16.to_be_bytes());
    }

    fn send(&self, opcode: u8, payload: &[u8]) -> Result<(), WsError> {
        let frame = encode_frame(opcode, payload, self.noise.mask());
        let mut st = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if st.closed {
            return Err(WsError::Closed);
        }
        if opcode == 0x8 {
            st.closed = true;
        }
        st.stream.write_all(&frame)?;
        st.stream.flush()?;
        Ok(())
    }
}

/// One client frame: FIN set, masked with `mask` (RFC 6455 §5.3: a client masks every
/// frame).
pub fn encode_frame(opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut f = Vec::with_capacity(payload.len() + 14);
    f.push(0x80 | (opcode & 0x0f));
    let n = payload.len();
    if n < 126 {
        f.push(0x80 | u8::try_from(n).unwrap_or(0));
    } else if let Ok(n16) = u16::try_from(n) {
        f.push(0x80 | 126);
        f.extend_from_slice(&n16.to_be_bytes());
    } else {
        f.push(0x80 | 127);
        f.extend_from_slice(&(n as u64).to_be_bytes());
    }
    f.extend_from_slice(&mask);
    f.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i & 3]));
    f
}

/// The reading half: assembles text messages, answers pings and closes.
pub struct WsReader {
    stream: Box<dyn Read + Send>,
    writer: WsWriter,
}

impl WsReader {
    /// The next text message.
    ///
    /// # Errors
    ///
    /// [`WsError::Closed`] at a close frame (answered) or the end of the stream; any other
    /// variant for a broken peer, after which the connection is unusable. A protocol error
    /// is answered with a close frame (1002, or 1009 for a message over the cap).
    pub fn next_text(&mut self) -> Result<String, WsError> {
        let mut message: Option<Vec<u8>> = None;
        loop {
            let (fin, opcode, payload) = match self.frame() {
                Ok(f) => f,
                Err(e) => {
                    self.fail(&e);
                    return Err(e);
                }
            };
            match opcode {
                0x1 => {
                    if message.is_some() {
                        let e =
                            WsError::Protocol("a text frame inside a fragmented message".into());
                        self.fail(&e);
                        return Err(e);
                    }
                    message = Some(payload);
                }
                0x0 => match message.as_mut() {
                    Some(m) => {
                        if m.len() + payload.len() > MAX_MESSAGE {
                            self.fail(&WsError::TooLarge);
                            return Err(WsError::TooLarge);
                        }
                        m.extend_from_slice(&payload);
                    }
                    None => {
                        let e = WsError::Protocol("a continuation frame with no message".into());
                        self.fail(&e);
                        return Err(e);
                    }
                },
                0x8 => {
                    if payload.len() == 1 {
                        let e = WsError::Protocol("a one-byte close payload".into());
                        self.fail(&e);
                        return Err(e);
                    }
                    // Echo the status code, as RFC 6455 §5.5.1 asks, then stop.
                    let code: Vec<u8> = payload.iter().take(2).copied().collect();
                    let _ = self.writer.send(0x8, &code);
                    return Err(WsError::Closed);
                }
                0x9 => {
                    let _ = self.writer.send(0xa, &payload);
                    continue;
                }
                0xa => continue,
                0x2 => {
                    let e = WsError::Protocol("a binary message; the app-server sends text".into());
                    self.fail(&e);
                    return Err(e);
                }
                other => {
                    let e = WsError::Protocol(format!("unknown opcode {other:#x}"));
                    self.fail(&e);
                    return Err(e);
                }
            }
            if fin {
                let bytes = message.take().unwrap_or_default();
                return String::from_utf8(bytes).map_err(|_| {
                    let e = WsError::Protocol("a text message that is not UTF-8".into());
                    self.fail(&e);
                    e
                });
            }
        }
    }

    fn fail(&self, e: &WsError) {
        let code: u16 = match e {
            WsError::TooLarge => 1009,
            WsError::Protocol(_) => 1002,
            _ => return,
        };
        let _ = self.writer.send(0x8, &code.to_be_bytes());
    }

    fn frame(&mut self) -> Result<(bool, u8, Vec<u8>), WsError> {
        let mut h = [0u8; 2];
        self.stream.read_exact(&mut h)?;
        let fin = h[0] & 0x80 != 0;
        if h[0] & 0x70 != 0 {
            return Err(WsError::Protocol(
                "a reserved bit is set and no extension was negotiated".into(),
            ));
        }
        let opcode = h[0] & 0x0f;
        if h[1] & 0x80 != 0 {
            return Err(WsError::Protocol("a masked frame from the server".into()));
        }
        let mut len = u64::from(h[1] & 0x7f);
        if len == 126 {
            let mut x = [0u8; 2];
            self.stream.read_exact(&mut x)?;
            len = u64::from(u16::from_be_bytes(x));
        } else if len == 127 {
            let mut x = [0u8; 8];
            self.stream.read_exact(&mut x)?;
            len = u64::from_be_bytes(x);
        }
        if opcode >= 0x8 && (!fin || len > 125) {
            return Err(WsError::Protocol(
                "a control frame that is fragmented or over 125 bytes".into(),
            ));
        }
        let len = usize::try_from(len).map_err(|_| WsError::TooLarge)?;
        if len > MAX_MESSAGE {
            return Err(WsError::TooLarge);
        }
        let mut p = vec![0u8; len];
        self.stream.read_exact(&mut p)?;
        Ok((fin, opcode, p))
    }
}

/// Do the HTTP Upgrade on `reader`/`writer` (one connected stream, split), with `host` as
/// the `Host` header, and check the answer (RFC 6455 §4.1): status 101, `Upgrade:
/// websocket`, `Connection: Upgrade` and the `Sec-WebSocket-Accept` for the key sent.
///
/// # Errors
///
/// [`WsError::Handshake`] for an answer that is not a valid Upgrade, or the stream's error.
pub fn handshake(
    mut reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    host: &str,
) -> Result<(WsReader, WsWriter), WsError> {
    if host.is_empty() || host.bytes().any(|b| b.is_ascii_control() || b == b' ') {
        return Err(WsError::Handshake("an empty or malformed host".into()));
    }
    let noise = Arc::new(Noise::new());
    let key = base64(&noise.key());
    let w = WsWriter {
        inner: Arc::new(Mutex::new(WriterState {
            stream: writer,
            closed: false,
        })),
        noise,
    };
    {
        let mut st = w.inner.lock().unwrap_or_else(|e| e.into_inner());
        let request = format!(
            "GET / HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        st.stream.write_all(request.as_bytes())?;
        st.stream.flush()?;
    }
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() >= MAX_HEAD {
            return Err(WsError::Handshake("response head too long".into()));
        }
        reader.read_exact(&mut b)?;
        head.push(b[0]);
    }
    check_upgrade_response(&head, &key)?;
    Ok((
        WsReader {
            stream: reader,
            writer: w.clone(),
        },
        w,
    ))
}

/// Check an Upgrade response head against the key that was sent.
///
/// # Errors
///
/// [`WsError::Handshake`] with what is wrong.
pub fn check_upgrade_response(head: &[u8], key: &str) -> Result<(), WsError> {
    let text = std::str::from_utf8(head)
        .map_err(|_| WsError::Handshake("response head is not UTF-8".into()))?;
    let mut lines = text.split("\r\n");
    let status = lines.next().unwrap_or_default();
    let mut parts = status.splitn(3, ' ');
    let (version, code) = (parts.next(), parts.next());
    if version != Some("HTTP/1.1") || code != Some("101") {
        return Err(WsError::Handshake(format!("status line {status:?}")));
    }
    let mut upgrade = false;
    let mut connection = false;
    let mut accept = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "upgrade" => upgrade = value.eq_ignore_ascii_case("websocket"),
            "connection" => {
                connection = value
                    .split(',')
                    .any(|t| t.trim().eq_ignore_ascii_case("upgrade"));
            }
            "sec-websocket-accept" => accept = Some(value.to_owned()),
            "sec-websocket-extensions" => {
                return Err(WsError::Handshake(
                    "an extension the client did not offer".into(),
                ));
            }
            "sec-websocket-protocol" => {
                return Err(WsError::Handshake(
                    "a subprotocol the client did not offer".into(),
                ));
            }
            _ => {}
        }
    }
    if !upgrade || !connection {
        return Err(WsError::Handshake(
            "no Upgrade: websocket / Connection: Upgrade".into(),
        ));
    }
    let want = accept_value(key);
    if accept.as_deref() != Some(want.as_str()) {
        return Err(WsError::Handshake("wrong Sec-WebSocket-Accept".into()));
    }
    Ok(())
}

/// `Sec-WebSocket-Accept` for `key` (RFC 6455 §4.2.2): base64 of the SHA-1 of the key and
/// the GUID.
pub fn accept_value(key: &str) -> String {
    base64(&sha1(format!("{key}{ACCEPT_GUID}").as_bytes()))
}

/// Standard base64 with padding (RFC 4648 §4).
pub fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= c.len() {
                out.push(char::from(A[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// SHA-1 (RFC 3174), for `Sec-WebSocket-Accept` only. It authenticates nothing: RFC 6455
/// uses it to show the server read the handshake, not for security.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn sha1_and_base64_match_known_values() {
        // RFC 3174 test 1 and the empty string.
        let hex = |d: [u8; 20]| d.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(
            hex(sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(hex(sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
    }

    #[test]
    fn accept_value_is_the_rfc_6455_example() {
        // RFC 6455 §1.3.
        assert_eq!(
            accept_value("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn upgrade_response_is_checked() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let ok = b"HTTP/1.1 101 Switching Protocols\r\nconnection: Upgrade\r\nupgrade: websocket\r\nsec-websocket-accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n";
        assert_eq!(check_upgrade_response(ok, key), Ok(()));
        let wrong = b"HTTP/1.1 101 Switching Protocols\r\nconnection: Upgrade\r\nupgrade: websocket\r\nsec-websocket-accept: AAAA\r\n\r\n";
        assert!(check_upgrade_response(wrong, key).is_err());
        let not101 = b"HTTP/1.1 200 OK\r\n\r\n";
        assert!(check_upgrade_response(not101, key).is_err());
    }

    #[test]
    fn client_frames_are_masked() {
        let f = encode_frame(0x1, b"hi", [1, 2, 3, 4]);
        assert_eq!(f, vec![0x81, 0x82, 1, 2, 3, 4, b'h' ^ 1, b'i' ^ 2]);
        let long = encode_frame(0x1, &[0u8; 300], [0; 4]);
        assert_eq!(&long[..4], &[0x81, 0x80 | 126, 1, 44]);
    }

    pub(crate) fn recording_writer() -> (WsWriter, Arc<Mutex<Vec<u8>>>) {
        let (reader, out) = reader_over(Vec::new());
        (reader.writer, out)
    }

    fn reader_over(bytes: Vec<u8>) -> (WsReader, Arc<Mutex<Vec<u8>>>) {
        #[derive(Clone)]
        struct Sink(Arc<Mutex<Vec<u8>>>);
        impl Write for Sink {
            fn write(&mut self, b: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let out = Arc::new(Mutex::new(Vec::new()));
        let w = WsWriter {
            inner: Arc::new(Mutex::new(WriterState {
                stream: Box::new(Sink(out.clone())),
                closed: false,
            })),
            noise: Arc::new(Noise::new()),
        };
        (
            WsReader {
                stream: Box::new(io::Cursor::new(bytes)),
                writer: w,
            },
            out,
        )
    }

    #[test]
    fn fragments_are_joined_and_pings_answered() {
        let mut b = vec![0x01, 3];
        b.extend_from_slice(b"{\"a");
        b.extend_from_slice(&[0x89, 1, b'p']);
        b.extend_from_slice(&[0x80, 4]);
        b.extend_from_slice(b"\":1}");
        let (mut r, out) = reader_over(b);
        assert_eq!(r.next_text().unwrap(), "{\"a\":1}");
        let sent = out.lock().unwrap().clone();
        assert_eq!(sent[0], 0x8a, "a pong answers the ping");
        assert_eq!(r.next_text(), Err(WsError::Closed));
    }

    #[test]
    fn broken_frames_are_refused() {
        for (bytes, why) in [
            (vec![0x81, 0x81, 0, 0, 0, 0, b'x'], "masked server frame"),
            (vec![0xc1, 1, b'x'], "reserved bit"),
            (vec![0x82, 1, b'x'], "binary"),
            (vec![0x80, 1, b'x'], "lone continuation"),
            (vec![0x09, 0], "fragmented control frame"),
            (vec![0x88, 1, 0], "one-byte close payload"),
        ] {
            let (mut r, out) = reader_over(bytes);
            assert!(matches!(r.next_text(), Err(WsError::Protocol(_))), "{why}");
            let sent = out.lock().unwrap();
            assert_eq!(sent[0], 0x88, "{why}: a close follows");
            assert_eq!(
                [sent[6] ^ sent[2], sent[7] ^ sent[3]],
                1002u16.to_be_bytes()
            );
        }
        let mut big = vec![0x81, 127];
        big.extend_from_slice(&(MAX_MESSAGE as u64 + 1).to_be_bytes());
        let (mut r, _) = reader_over(big);
        assert_eq!(r.next_text(), Err(WsError::TooLarge));

        // Each frame fits, but the cumulative message does not. The last frame is
        // final so removing the cumulative cap returns text, rather than EOF.
        let mut fragments = Vec::new();
        for i in 0..17 {
            fragments.push(if i == 0 {
                0x01
            } else if i == 16 {
                0x80
            } else {
                0x00
            });
            fragments.push(127);
            fragments.extend_from_slice(&(1024u64 * 1024).to_be_bytes());
            fragments.resize(fragments.len() + 1024 * 1024, b'x');
        }
        let (mut r, out) = reader_over(fragments);
        assert_eq!(r.next_text(), Err(WsError::TooLarge));
        let sent = out.lock().unwrap();
        assert_eq!(sent[0], 0x88);
        assert_eq!(
            [sent[6] ^ sent[2], sent[7] ^ sent[3]],
            1009u16.to_be_bytes()
        );
        assert_eq!(r.writer.send_text("later"), Err(WsError::Closed));
    }

    #[test]
    fn upgrade_headers_are_bounded() {
        let err = handshake(
            Box::new(io::Cursor::new(vec![b'x'; MAX_HEAD + 1])),
            Box::new(io::sink()),
            "localhost",
        )
        .err()
        .unwrap();
        assert_eq!(err, WsError::Handshake("response head too long".into()));
    }

    #[test]
    fn an_unoffered_subprotocol_is_refused() {
        let key = "test";
        let head = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\nSec-WebSocket-Protocol: rpc\r\n\r\n",
            accept_value(key)
        );
        assert!(matches!(
            check_upgrade_response(head.as_bytes(), key),
            Err(WsError::Handshake(_))
        ));
    }

    #[test]
    fn a_close_is_echoed_and_ends_the_stream() {
        let (mut r, out) = reader_over(vec![0x88, 2, 0x03, 0xe8]);
        assert_eq!(r.next_text(), Err(WsError::Closed));
        assert_eq!(out.lock().unwrap()[0], 0x88);
        let w = r.writer.clone();
        assert_eq!(w.send_text("x"), Err(WsError::Closed));
    }
}
