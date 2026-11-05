// SPDX-License-Identifier: Apache-2.0

//! `oac pair`, `oac pair --respond`, `oac pair --list` and `oac unpair <key_id>` (#437, slice
//! S4; `docs/planning/decisions/G-437-pairing.md` §3 and §6): the operator's end of the
//! daemon's pairing ops (`crate::device_pairing`).
//!
//! **This is the client path, and it holds no key.** It opens the daemon's `control` endpoint,
//! sends and receives lines, and shows the operator what came back. It builds no key store,
//! reads no credential, starts no daemon and runs none of the core's pairing types: the
//! initiator's nonce, the statements and the device key stay in the daemon, and a blob is the
//! public text the operator carries between two devices. (`cli/tests/pair_verbs.rs` checks
//! this file's imports.) The one daemon-side item it reads is the blob decoder, so that a
//! wrong kind of blob is diagnosed before anything is sent.
//!
//! **Streams.** Standard output holds what the operator keeps or passes on: the blobs
//! (printed wrapped at [`WRAP`] characters, then an empty line), the codes, the statement
//! values and the result. Standard error holds the prompts, the instructions and the
//! failures. Under `--json` standard output holds only canonical JSON objects with
//! `"oac_cli":1`, one per line (a stream, one per step, the last the result), and no prompt
//! is printed: a program that drives the verb reads the steps. A paste ends at an empty line
//! or at the end of input; whitespace inside it is ignored, so a blob wrapped over several
//! lines is one blob.
//!
//! **Never printed.** A blob, code or statement is never put in a diagnostic; a pasted text
//! that is not a blob is never echoed.
//!
//! **Exit status** (`crate::verbs`, `08-cli-and-deployment.md` §5): 0 paired, unpaired or
//! listed; 1 the pairing ended, the code did not match or was refused, the result was not
//! saved, or the key is already trusted; 2 usage; 3 no daemon; 4 the endpoint is another
//! user's, busy (all control places taken), stopping, or speaks another control version.

use std::io::{self, BufRead, Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use oac_core::json::{self, Json, JsonObject};

use crate::control::{canonical_text, cli_object, is_hostile, json_string};
use crate::device_pairing::{Blob, MAX_LINE, PAIRING_DEADLINE, decode_blob};
use crate::ipc::{CONTROL_LABEL, ClientStream, ConnectError, Scope};
use crate::verbs::{EXIT_FAILED, EXIT_NOT_RUNNING, EXIT_OK, EXIT_UNREACHABLE, EXIT_USAGE};

/// The width a blob is printed at: the line limit of a macOS terminal is 1024 bytes in
/// canonical mode, and a wrapped blob is easier to carry than one 1 KiB line.
pub const WRAP: usize = 64;

/// How long `oac unpair` and `oac pair --list` wait for the daemon's answer, and how long
/// `oac pair` waits for the answer to its first line.
pub const ANSWER_WAIT: Duration = Duration::from_secs(30);

/// The longest pasted blob, in characters: the control line the daemon takes ([`MAX_LINE`])
/// less the request around the blob.
const MAX_BLOB: usize = MAX_LINE - 64;
/// The longest reply line read from the daemon.
const MAX_REPLY: usize = 1 << 20;
/// Time added to the pairing window while waiting for a reply: the daemon ends the pairing
/// at the window, this only bounds a daemon that never answers.
const REPLY_SLACK: Duration = Duration::from_secs(5);

const USAGE_PAIR: &str = "usage: oac pair [--respond | --list] [--json]\n\
     (to pair two devices run `oac pair --respond` on one, then `oac pair` on the other)";
const USAGE_UNPAIR: &str = "usage: oac unpair <key_id> [--yes] [--json]";

// ---- the arguments ----------------------------------------------------------------------------

/// Which side of a pairing this device plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairRole {
    /// `oac pair`: prints the offer first.
    Initiator,
    /// `oac pair --respond`: waits for an offer.
    Responder,
}

impl PairRole {
    fn name(self) -> &'static str {
        match self {
            PairRole::Initiator => "initiator",
            PairRole::Responder => "responder",
        }
    }
}

/// What `oac pair` was asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairCommand {
    /// Pair with another device, in a role.
    Pair(PairRole),
    /// List the trusted keys and the recent events.
    List,
}

/// The arguments of `oac pair`: a command and whether `--json` was given. `None` is a usage
/// error.
pub fn parse_pair_args(args: &[String]) -> Option<(PairCommand, bool)> {
    let mut command = None;
    let mut json = false;
    for arg in args {
        let wanted = match arg.as_str() {
            "--json" if !json => {
                json = true;
                continue;
            }
            "--respond" => PairCommand::Pair(PairRole::Responder),
            "--list" => PairCommand::List,
            _ => return None,
        };
        if command.replace(wanted).is_some() {
            return None;
        }
    }
    Some((
        command.unwrap_or(PairCommand::Pair(PairRole::Initiator)),
        json,
    ))
}

/// The arguments of `oac unpair`: the key id, `--yes` and `--json`. `None` is a usage error
/// (a key id is the full 64 lower-case hexadecimal digits; there is no prefix matching).
pub fn parse_unpair_args(args: &[String]) -> Option<(String, bool, bool)> {
    let (mut key, mut yes, mut json) = (None, false, false);
    for arg in args {
        match arg.as_str() {
            "--yes" if !yes => yes = true,
            "--json" if !json => json = true,
            other if !other.starts_with("--") && key.is_none() => key = Some(other.to_owned()),
            _ => return None,
        }
    }
    let key = key?;
    oac_core::ids::KeyId::parse(&key).map(|_| (key, yes, json))
}

// ---- the streams and the wire -------------------------------------------------------------------

/// The operator's streams of one invocation.
pub struct Terminal<'a> {
    /// Standard input: pasted blobs, typed codes and the confirmation of an unpair.
    pub stdin: &'a mut dyn BufRead,
    /// Standard output: blobs, codes, results.
    pub stdout: &'a mut dyn Write,
    /// Standard error: prompts, instructions, failures.
    pub stderr: &'a mut dyn Write,
}

/// What reading a reply from the daemon produced.
#[derive(Debug, PartialEq, Eq)]
pub enum Recv {
    /// One reply line, the newline removed.
    Line(String),
    /// The daemon closed the connection.
    Closed,
    /// The time ran out.
    TimedOut,
}

/// One control connection: lines out, lines in. The real one is [`StreamWire`]; a test
/// scripts another.
pub trait Wire {
    /// Sends one line (with its newline).
    ///
    /// # Errors
    ///
    /// The I/O error when the line could not be written.
    fn send(&mut self, line: &str) -> io::Result<()>;
    /// The next reply line, waiting at most `wait`.
    fn recv(&mut self, wait: Duration) -> Recv;
}

/// A control connection over the local IPC.
///
/// A synchronous Windows pipe handle serializes reads and writes made from two threads, so a
/// read is made on a thread that owns the stream for that one read and hands it back: a write
/// is never made while a read is pending, and the wait is on a channel, not on the handle.
pub struct StreamWire {
    stream: Option<ClientStream>,
    pending: Vec<u8>,
}

type ReadDone = (ClientStream, Vec<u8>, io::Result<Option<String>>);

fn read_reply(stream: &mut ClientStream, buf: &mut Vec<u8>) -> io::Result<Option<String>> {
    loop {
        if let Some(i) = buf.iter().position(|b| *b == b'\n') {
            let mut line: Vec<u8> = buf.drain(..=i).collect();
            line.pop();
            return String::from_utf8(line)
                .map(Some)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8"));
        }
        if buf.len() > MAX_REPLY {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
        }
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk)? {
            0 => return Ok(None),
            n => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

impl Wire for StreamWire {
    fn send(&mut self, line: &str) -> io::Result<()> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?;
        stream
            .write_all(line.as_bytes())
            .and_then(|()| stream.flush())
    }

    fn recv(&mut self, wait: Duration) -> Recv {
        let Some(mut stream) = self.stream.take() else {
            return Recv::Closed;
        };
        let mut buf = std::mem::take(&mut self.pending);
        let (tx, rx) = mpsc::channel::<ReadDone>();
        let spawned = std::thread::Builder::new()
            .name("oac-pair-read".into())
            .spawn(move || {
                let result = read_reply(&mut stream, &mut buf);
                let _ = tx.send((stream, buf, result));
            });
        if spawned.is_err() {
            return Recv::Closed;
        }
        match rx.recv_timeout(wait) {
            Ok((stream, buf, Ok(Some(line)))) => {
                self.stream = Some(stream);
                self.pending = buf;
                Recv::Line(line)
            }
            // The stream is dropped: a closed or unreadable connection is over.
            Ok(_) => Recv::Closed,
            // The read thread keeps the stream; the process ends before it matters.
            Err(_) => Recv::TimedOut,
        }
    }
}

fn connect(scope: &Scope) -> Result<StreamWire, Failure> {
    match scope.endpoint(CONTROL_LABEL).connect_blocking() {
        Ok(stream) => Ok(StreamWire {
            stream: Some(stream),
            pending: Vec::new(),
        }),
        Err(ConnectError::NotRunning) => Err(Failure::new(
            "not_running",
            EXIT_NOT_RUNNING,
            "the daemon is not running; start it with `oac start`",
        )),
        Err(ConnectError::Busy) => Err(Failure::new(
            "busy",
            EXIT_UNREACHABLE,
            "the daemon's control endpoint stayed busy; try again",
        )),
        Err(e) => Err(Failure::new(
            "unreachable",
            EXIT_UNREACHABLE,
            format!("the daemon could not be reached: {e}"),
        )
        .detail(e.to_string())),
    }
}

// ---- failures and the mapping of the daemon's words ---------------------------------------------

/// How a verb failed: a stable code (the `error` of `--json`), the exit status, and the
/// operator's message.
#[derive(Debug)]
struct Failure {
    code: String,
    exit: i32,
    message: String,
    detail: Option<String>,
    key_id: Option<String>,
}

impl Failure {
    fn new(code: &str, exit: i32, message: impl Into<String>) -> Failure {
        Failure {
            code: code.to_owned(),
            exit,
            message: message.into(),
            detail: None,
            key_id: None,
        }
    }
    fn detail(mut self, detail: String) -> Failure {
        self.detail = Some(detail);
        self
    }
    fn key(mut self, key_id: Option<String>) -> Failure {
        self.key_id = key_id;
        self
    }
    fn json(&self, extra: &[(&str, String)]) -> String {
        let mut members = vec![
            ("ok", "false".to_owned()),
            ("error", json_string(&self.code)),
        ];
        if let Some(d) = &self.detail {
            members.push(("detail", json_string(d)));
        }
        if let Some(k) = &self.key_id {
            members.push(("key_id", json_string(k)));
        }
        members.extend(extra.iter().map(|(n, v)| (*n, v.clone())));
        cli_object(&members)
    }
}

/// Text from the daemon, made safe to show: no control or direction-changing character.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() || is_hostile(c) {
                '?'
            } else {
                c
            }
        })
        .collect()
}

/// A refusal the daemon gave in place of a step (`{"error":...}`).
fn from_error(code: &str) -> Failure {
    let (exit, message) = match code {
        "busy" => (
            EXIT_UNREACHABLE,
            "the daemon's control endpoint is busy: another pairing is open, or every control place is taken; finish or wait for it, then try again",
        ),
        "version" => (
            EXIT_UNREACHABLE,
            "the daemon speaks another version of the control protocol; use the `oac` that started it",
        ),
        "unknown_op" => (
            EXIT_UNREACHABLE,
            "the daemon does not know this request: it is older than this `oac`; stop it with `oac stop` and start it again",
        ),
        "shutting_down" => (
            EXIT_UNREACHABLE,
            "the daemon is stopping; wait for it to stop, start it again with `oac start`, and retry",
        ),
        "malformed" => (
            EXIT_FAILED,
            "the daemon could not read the request (a bug or a version mismatch)",
        ),
        "too_large" => (
            EXIT_FAILED,
            "the daemon refused a line as too large; what was pasted is bigger than a pairing carries",
        ),
        "wrong_role" => (
            EXIT_FAILED,
            "that step is not for this side of the pairing (a bug; the pairing was dropped)",
        ),
        "no_code_yet" => (
            EXIT_FAILED,
            "the daemon has no code to compare yet (a bug; the pairing was dropped)",
        ),
        "bad_blob" => (
            EXIT_FAILED,
            "the daemon did not accept that blob; nothing changed on either device",
        ),
        "bad_code" => (
            EXIT_FAILED,
            "the daemon says that is not a six-digit code; it was not counted as a try",
        ),
        "not_trusted" => (
            EXIT_FAILED,
            "that key is not trusted; see the keys `oac pair --list` shows",
        ),
        "own_key" => (
            EXIT_FAILED,
            "that is this device's own key; it cannot be removed",
        ),
        other => {
            return Failure::new(
                "failed",
                EXIT_FAILED,
                format!("the daemon refused: {}", clean(other)),
            );
        }
    };
    Failure::new(code, exit, message)
}

/// How a pairing ended without trusting the key (an `end`, or one of the terminal steps).
fn from_end(end: &str, window: u64) -> Failure {
    let message = match end {
        "second_offer" => {
            "a second offer reached this device, so the pairing was ended. A device you did not start may be sending offers; run `oac pair --respond` again".to_owned()
        }
        "second_response" => {
            "a second response reached this device, so the pairing was ended. A device you did not start may be answering; run `oac pair` again".to_owned()
        }
        "second_reveal" => {
            "a second reveal reached this device after its code was shown, so the pairing was ended; run it again".to_owned()
        }
        "reveal_without_offer" => {
            "a reveal arrived before this device had answered an offer, so the pairing was ended; paste the offer first".to_owned()
        }
        "abandoned" => {
            "the pairing was abandoned: the connection dropped, or the window ran out while this device waited for the next step".to_owned()
        }
        "expired" => format!(
            "the {window} s window ran out; no key was added. Run both verbs again and paste faster (start `oac pair --respond` on the other device first)"
        ),
        "own_key" => {
            "the other device presented this device's own key: both are the same device or share a device key. Pair two different devices".to_owned()
        }
        "commitment_mismatch" => {
            "the reveal does not match the offer it answers: a blob was altered or came from another pairing. No key was added; start over".to_owned()
        }
        "aborted" => {
            "five wrong codes: the pairing was aborted and no key was added on this device. If the other device reported `paired`, it trusts this device and this one does not trust it: run `oac pair --list` there and `oac unpair` this device's key, then pair again".to_owned()
        }
        "already_trusted" => {
            "this key is already trusted; confirming its agreement values without removing it is #430; to pair again now, run `oac unpair <key_id>` (this deletes its grants and bindings)".to_owned()
        }
        "not_saved" => {
            "the codes matched, but the trust store could not be saved, so the key was NOT added. See the daemon's log, fix the cause (disk, permissions) and pair again".to_owned()
        }
        "bad_line" => {
            "the daemon could not read a request of this client (a bug or a version mismatch); the pairing was dropped".to_owned()
        }
        "shutdown" => "the daemon is stopping; the pairing was dropped".to_owned(),
        other => format!("the pairing ended: {}", clean(other)),
    };
    let known = [
        "second_offer",
        "second_response",
        "second_reveal",
        "reveal_without_offer",
        "abandoned",
        "expired",
        "own_key",
        "commitment_mismatch",
        "aborted",
        "already_trusted",
        "not_saved",
        "bad_line",
        "shutdown",
    ];
    Failure::new(
        if known.contains(&end) { end } else { "ended" },
        EXIT_FAILED,
        message,
    )
}

// ---- one reply ------------------------------------------------------------------------------

struct Reply(JsonObject);

impl Reply {
    fn parse(text: &str) -> Result<Reply, Failure> {
        json::parse(text.as_bytes())
            .ok()
            .and_then(Json::into_object)
            .map(Reply)
            .ok_or_else(|| {
                Failure::new(
                    "failed",
                    EXIT_FAILED,
                    "the daemon's answer was not understood",
                )
                .detail("the daemon's answer was not understood".to_owned())
            })
    }
    fn str(&self, name: &str) -> Option<&str> {
        self.0.get(name).and_then(Json::as_str)
    }
    fn error(&self) -> Option<&str> {
        self.str("error")
    }
    fn kind(&self) -> Option<&str> {
        self.str("pair")
    }
    fn flag(&self, name: &str) -> Option<bool> {
        self.0.get(name).and_then(Json::as_bool)
    }
    fn number(&self, name: &str) -> Option<u64> {
        self.0
            .get(name)
            .and_then(Json::as_number)
            .and_then(|n| n.plain_integer_in(0, u64::MAX))
    }
}

fn unexpected() -> Failure {
    Failure::new(
        "unexpected_reply",
        EXIT_FAILED,
        "the daemon answered with a step this verb did not expect (a bug or a version mismatch)",
    )
}

/// An agreement statement value as the daemon reports it, for showing.
enum Statement {
    None,
    Refused,
    Given {
        key: String,
        seq: u64,
        issued: String,
    },
}

impl Statement {
    fn read(object: Option<&JsonObject>) -> Result<Statement, Failure> {
        let o = object.ok_or_else(unexpected)?;
        let text = |n: &str| o.get(n).and_then(Json::as_str).map(clean);
        match o.get("state").and_then(Json::as_str) {
            Some("none") => Ok(Statement::None),
            Some("refused") => Ok(Statement::Refused),
            Some("given") => Ok(Statement::Given {
                key: text("agreement_key").ok_or_else(unexpected)?,
                seq: o
                    .get("seq")
                    .and_then(Json::as_number)
                    .and_then(|n| n.plain_integer_in(0, u64::MAX))
                    .ok_or_else(unexpected)?,
                issued: text("issued_at").ok_or_else(unexpected)?,
            }),
            _ => Err(unexpected()),
        }
    }
    fn json(&self) -> String {
        match self {
            Statement::None => "{\"state\":\"none\"}".to_owned(),
            Statement::Refused => "{\"state\":\"refused\"}".to_owned(),
            Statement::Given { key, seq, issued } => format!(
                "{{\"agreement_key\":{},\"issued_at\":{},\"seq\":{seq},\"state\":\"given\"}}",
                json_string(key),
                json_string(issued)
            ),
        }
    }
    fn human(&self) -> String {
        match self {
            Statement::None => "none".to_owned(),
            Statement::Refused => "REFUSED by this device (the codes will differ)".to_owned(),
            Statement::Given { key, seq, issued } => {
                format!("agreement key {key}, seq {seq}, issued {issued}")
            }
        }
    }
}

// ---- pasted input ---------------------------------------------------------------------------

enum Line {
    Eof,
    Text(String),
    TooLong,
}

/// One line of input, of which at most `keep` characters are kept; the rest of a longer line
/// is read and dropped, so a pasted wall of text costs no memory.
fn read_line_bounded(input: &mut dyn BufRead, keep: usize) -> Line {
    let mut line: Vec<u8> = Vec::new();
    let (mut seen, mut over) = (false, false);
    loop {
        let available = match input.fill_buf() {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => &[],
        };
        if available.is_empty() {
            return match (seen, over) {
                (false, _) => Line::Eof,
                (true, true) => Line::TooLong,
                (true, false) => Line::Text(String::from_utf8_lossy(&line).into_owned()),
            };
        }
        seen = true;
        let end = available.iter().position(|b| *b == b'\n');
        let take = end.unwrap_or(available.len());
        if line.len() + take > keep {
            over = true;
        } else {
            line.extend_from_slice(&available[..take]);
        }
        input.consume(end.map_or(take, |i| i + 1));
        if end.is_some() {
            return if over {
                Line::TooLong
            } else {
                Line::Text(String::from_utf8_lossy(&line).into_owned())
            };
        }
    }
}

enum BlobInput {
    Eof,
    Text(String),
    TooLarge,
}

/// A pasted blob: its lines joined with all whitespace removed, up to an empty line or the
/// end of input. Empty lines before the first text are skipped.
fn read_blob(input: &mut dyn BufRead) -> BlobInput {
    let (mut text, mut started, mut too_large) = (String::new(), false, false);
    let finish = |text: String, too_large: bool| {
        if too_large {
            BlobInput::TooLarge
        } else {
            BlobInput::Text(text)
        }
    };
    loop {
        match read_line_bounded(input, MAX_BLOB + 1) {
            Line::Eof if started => return finish(text, too_large),
            Line::Eof => return BlobInput::Eof,
            Line::TooLong => {
                started = true;
                too_large = true;
            }
            Line::Text(l) => {
                let squeezed: String = l.chars().filter(|c| !c.is_whitespace()).collect();
                if squeezed.is_empty() {
                    if started {
                        return finish(text, too_large);
                    }
                    continue;
                }
                started = true;
                if text.len() + squeezed.len() > MAX_BLOB {
                    too_large = true;
                } else {
                    text.push_str(&squeezed);
                }
            }
        }
    }
}

/// `blob` wrapped at [`WRAP`] characters per line, then an empty line.
fn wrapped(blob: &str) -> String {
    let mut out = String::new();
    for chunk in blob.as_bytes().chunks(WRAP) {
        out.push_str(&String::from_utf8_lossy(chunk));
        out.push('\n');
    }
    out.push('\n');
    out
}

fn kind_name(blob: &Blob) -> &'static str {
    match blob {
        Blob::Offer(_) => "offer",
        Blob::Response(_) => "response",
        Blob::Reveal(_) => "reveal",
    }
}

fn six_digits(text: &str) -> bool {
    text.len() == 6 && text.bytes().all(|b| b.is_ascii_digit())
}

// ---- requests ---------------------------------------------------------------------------------

fn pair_line(role: PairRole) -> String {
    format!(
        "{{\"oac_control\":1,\"op\":\"pair\",\"role\":\"{}\"}}\n",
        role.name()
    )
}

fn blob_line(op: &str, blob: &str) -> String {
    format!(
        "{{\"blob\":{},\"oac_control\":1,\"op\":\"{op}\"}}\n",
        json_string(blob)
    )
}

fn confirm_line(entered: &str) -> String {
    format!(
        "{{\"entered\":{},\"oac_control\":1,\"op\":\"pair_confirm\"}}\n",
        json_string(entered)
    )
}

fn unpair_line(key_id: &str) -> String {
    format!(
        "{{\"key_id\":{},\"oac_control\":1,\"op\":\"unpair\"}}\n",
        json_string(key_id)
    )
}

const PAIRINGS_LINE: &str = "{\"oac_control\":1,\"op\":\"pairings\"}\n";

/// Asks one question of the daemon on a connection of its own and returns the reply.
fn ask(wire: &mut dyn Wire, line: &str, wait: Duration) -> Result<Reply, Failure> {
    let sent = wire.send(line);
    match wire.recv(wait) {
        Recv::Line(text) => Reply::parse(&text),
        Recv::Closed => Err(closed(sent.is_err())),
        Recv::TimedOut => Err(Failure::new(
            "timeout",
            EXIT_FAILED,
            "the daemon did not answer in time; check `oac pair --list` before you retry",
        )),
    }
}

fn closed(send_failed: bool) -> Failure {
    let message = if send_failed {
        "the request could not be sent: the daemon closed the connection"
    } else {
        "the daemon closed the connection without an answer; the pairing is over (its window may have run out)"
    };
    Failure::new("connection_closed", EXIT_FAILED, message).detail(message.to_owned())
}

// ---- oac pair -----------------------------------------------------------------------------------

/// The state of one `oac pair` run.
struct Run<'a, 'b> {
    wire: &'a mut dyn Wire,
    t: &'a mut Terminal<'b>,
    json: bool,
    first_wait: Duration,
    window_end: Option<Instant>,
    window: u64,
    peer_key: Option<String>,
}

impl Run<'_, '_> {
    fn say(&mut self, text: &str) {
        if !self.json {
            let _ = writeln!(self.t.stderr, "{text}");
        }
    }
    fn diagnose(&mut self, text: &str) {
        let _ = writeln!(self.t.stderr, "oac pair: {text}");
    }
    fn print(&mut self, text: &str) {
        if !self.json {
            let _ = writeln!(self.t.stdout, "{text}");
        }
    }
    fn print_blob(&mut self, blob: &str) {
        if !self.json {
            let _ = self.t.stdout.write_all(wrapped(blob).as_bytes());
            let _ = self.t.stdout.flush();
        }
    }
    fn step(&mut self, name: &str, members: &[(&str, String)]) {
        if self.json {
            let mut all = vec![("step", json_string(name))];
            all.extend(members.iter().map(|(n, v)| (*n, v.clone())));
            let _ = writeln!(self.t.stdout, "{}", cli_object(&all));
            let _ = self.t.stdout.flush();
        }
    }
    fn left(&self) -> u64 {
        self.window_end
            .map_or(0, |e| e.saturating_duration_since(Instant::now()).as_secs())
    }
    fn prompt(&mut self, text: &str) {
        if !self.json {
            let left = self.left();
            let _ = write!(self.t.stderr, "{text} ({left} s left): ");
            let _ = self.t.stderr.flush();
        }
    }

    /// Sends `line` and reads the reply; a refusal or a terminal step is the failure.
    /// `bad_blob` and `bad_code` are returned as replies for the caller to ask again.
    fn request(&mut self, line: &str) -> Result<Reply, Failure> {
        let sent = self.wire.send(line);
        let wait = self.window_end.map_or(self.first_wait, |e| {
            e.saturating_duration_since(Instant::now()) + REPLY_SLACK
        });
        let reply = match self.wire.recv(wait) {
            Recv::Line(text) => Reply::parse(&text)?,
            Recv::Closed => return Err(closed(sent.is_err())),
            Recv::TimedOut => {
                return Err(Failure::new(
                    "timeout",
                    EXIT_FAILED,
                    "the daemon did not answer in time; the pairing is over",
                ));
            }
        };
        if matches!(reply.error(), Some("bad_blob" | "bad_code")) {
            return Ok(reply);
        }
        if let Some(code) = reply.error() {
            return Err(from_error(code));
        }
        match reply.kind() {
            Some("ended") => Err(from_end(reply.str("end").unwrap_or("ended"), self.window)),
            Some(end @ ("aborted" | "expired" | "already_trusted" | "not_saved")) => {
                Err(from_end(end, self.window).key(if end == "already_trusted" {
                    self.peer_key.clone()
                } else {
                    None
                }))
            }
            _ => Ok(reply),
        }
    }

    fn expect(&self, reply: Reply, kind: &str) -> Result<Reply, Failure> {
        if reply.kind() == Some(kind) {
            Ok(reply)
        } else {
            Err(unexpected())
        }
    }

    fn open_window(&mut self, reply: &Reply) {
        let ms = reply.number("expires_ms").unwrap_or(0);
        self.window_end = Some(Instant::now() + Duration::from_millis(ms));
        self.window = PAIRING_DEADLINE.as_secs();
    }

    /// A pasted blob of kind `expected`; text that is not one is explained, never echoed,
    /// and nothing is sent for it.
    fn ask_blob(&mut self, expected: &str, from: &str) -> Result<String, Failure> {
        loop {
            self.prompt(&format!(
                "Paste the {expected} blob {from}, then press Enter on an empty line to finish it"
            ));
            let text = match read_blob(self.t.stdin) {
                BlobInput::Eof => {
                    return Err(Failure::new(
                        "input_ended",
                        EXIT_FAILED,
                        "the input ended before the pairing finished; nothing was added",
                    ));
                }
                BlobInput::TooLarge => {
                    self.diagnose(&format!(
                        "that paste is longer than a pairing blob ({MAX_BLOB} characters); paste only the {expected} blob. Nothing was sent."
                    ));
                    continue;
                }
                BlobInput::Text(t) => t,
            };
            match decode_blob(&text) {
                Ok(blob) if kind_name(&blob) == expected => return Ok(text),
                Ok(blob) => self.diagnose(&format!(
                    "that is a {} blob, and this step needs the {expected} blob {from}. Nothing was sent; nothing changed.",
                    kind_name(&blob)
                )),
                Err(e) => self.diagnose(&format!(
                    "that is not a pairing blob ({e}); paste the whole {expected} blob {from}, wrapped lines included. Nothing was sent."
                )),
            }
        }
    }

    /// The six digits the operator reads off the other device.
    fn ask_code(&mut self) -> Result<String, Failure> {
        loop {
            self.prompt("Type the 6-digit code the OTHER device shows");
            let text = match read_line_bounded(self.t.stdin, 64) {
                Line::Eof => {
                    return Err(Failure::new(
                        "input_ended",
                        EXIT_FAILED,
                        "the input ended before the pairing finished; nothing was added",
                    ));
                }
                Line::TooLong => String::new(),
                Line::Text(t) => t,
            };
            let code = text.trim();
            if six_digits(code) {
                return Ok(code.to_owned());
            }
            self.diagnose(
                "a code is exactly six digits (0-9): type the code the other device shows. Nothing was sent and no try was used.",
            );
        }
    }

    fn show_code(&mut self, reply: &Reply) -> Result<(), Failure> {
        let code = reply.str("code").ok_or_else(unexpected)?;
        let peer = reply.str("peer_key_id").ok_or_else(unexpected)?;
        let own = Statement::read(reply.0.get("own_statement").and_then(Json::as_object))?;
        let theirs = Statement::read(reply.0.get("peer_statement").and_then(Json::as_object))?;
        let trusted = reply.flag("already_trusted").ok_or_else(unexpected)?;
        self.peer_key = Some(peer.to_owned());
        let mut members = Vec::new();
        if let Some(blob) = reply.str("blob") {
            members.push(("blob", json_string(blob)));
        }
        members.extend([
            ("code", json_string(code)),
            ("peer_key_id", json_string(peer)),
            ("own_statement", own.json()),
            ("peer_statement", theirs.json()),
            ("already_trusted", trusted.to_string()),
        ]);
        self.step("code", &members);
        if !self.json {
            if let Some(blob) = reply.str("blob") {
                self.say("Give the other device this reveal blob; it then shows its own code:");
                self.print_blob(blob);
            }
            self.print(&format!("This device's code: {code}"));
            self.say("Type this device's code on the other device when it asks.");
            self.print(&format!("The other device's key: {peer}"));
            self.print(
                "Agreement statements (the other device must show these two the other way round):",
            );
            self.print(&format!("  this device: {}", own.human()));
            self.print(&format!("  other device: {}", theirs.human()));
            if trusted {
                self.say("This key is already trusted on this device. The pairing can still finish on the other device, but this device keeps its record and this run exits 1 (#430 would confirm new agreement values; to pair again now run `oac unpair <key_id>` with that key).");
            }
        }
        Ok(())
    }

    /// Asks for the other device's code until it is accepted: the key id once paired.
    fn confirm(&mut self) -> Result<String, Failure> {
        loop {
            let entered = self.ask_code()?;
            let reply = self.request(&confirm_line(&entered))?;
            match (reply.error(), reply.kind()) {
                (Some("bad_code"), _) => self.diagnose(
                    "the daemon says that is not a six-digit code; it was not counted as a try.",
                ),
                (_, Some("paired")) => {
                    return reply
                        .str("key_id")
                        .map(str::to_owned)
                        .ok_or_else(unexpected);
                }
                (_, Some("mismatch")) => {
                    let left = reply.number("attempts_left").unwrap_or(0);
                    self.step("mismatch", &[("attempts_left", left.to_string())]);
                    self.diagnose(&format!(
                        "that code does not match the one the other device shows. {left} tr{} left; read the other device's screen again.",
                        if left == 1 { "y" } else { "ies" }
                    ));
                }
                _ => return Err(unexpected()),
            }
        }
    }

    fn pair(&mut self, role: PairRole) -> Result<String, Failure> {
        let first = self.request(&pair_line(role))?;
        match role {
            PairRole::Initiator => {
                let first = self.expect(first, "offer")?;
                let blob = first.str("blob").ok_or_else(unexpected)?.to_owned();
                self.open_window(&first);
                self.step(
                    "offer",
                    &[
                        ("blob", json_string(&blob)),
                        (
                            "expires_ms",
                            first.number("expires_ms").unwrap_or(0).to_string(),
                        ),
                    ],
                );
                self.say(&format!(
                    "Pairing started: this pairing window closes in {} s. If `oac pair --respond` is not waiting on the other device yet, start it there now.",
                    self.left()
                ));
                self.say("Give the other device this offer blob:");
                self.print_blob(&blob);
                let reply = loop {
                    let text = self.ask_blob(
                        "response",
                        "that `oac pair --respond` printed on the other device",
                    )?;
                    let reply = self.request(&blob_line("pair_response", &text))?;
                    if reply.error() == Some("bad_blob") {
                        self.diagnose("the daemon did not accept that blob; nothing changed. Check that it is the response to this offer.");
                        continue;
                    }
                    break self.expect(reply, "code")?;
                };
                self.show_code(&reply)?;
            }
            PairRole::Responder => {
                let first = self.expect(first, "waiting")?;
                self.open_window(&first);
                self.step(
                    "waiting",
                    &[(
                        "expires_ms",
                        first.number("expires_ms").unwrap_or(0).to_string(),
                    )],
                );
                self.say(&format!(
                    "Waiting for an offer: this pairing window closes in {} s. Now run `oac pair` on the other device.",
                    self.left()
                ));
                let response = loop {
                    let text =
                        self.ask_blob("offer", "that `oac pair` printed on the other device")?;
                    let reply = self.request(&blob_line("pair_offer", &text))?;
                    if reply.error() == Some("bad_blob") {
                        self.diagnose("the daemon did not accept that blob; nothing changed. Check that it is a fresh offer.");
                        continue;
                    }
                    break self.expect(reply, "response")?;
                };
                let blob = response.str("blob").ok_or_else(unexpected)?.to_owned();
                self.step("response", &[("blob", json_string(&blob))]);
                self.say("Give the other device this response blob:");
                self.print_blob(&blob);
                let reply = loop {
                    let text = self.ask_blob(
                        "reveal",
                        "that `oac pair` prints on the other device after it has the response",
                    )?;
                    let reply = self.request(&blob_line("pair_reveal", &text))?;
                    if reply.error() == Some("bad_blob") {
                        self.diagnose("the daemon did not accept that blob; nothing changed. Check that it is the reveal for this response.");
                        continue;
                    }
                    break self.expect(reply, "code")?;
                };
                self.show_code(&reply)?;
            }
        }
        self.confirm()
    }
}

/// Runs one pairing over `wire` in `role`; the exit status. `first_wait` bounds the wait for
/// the answer to the first line.
pub fn pair_over(
    wire: &mut dyn Wire,
    role: PairRole,
    json: bool,
    first_wait: Duration,
    t: &mut Terminal<'_>,
) -> i32 {
    let mut run = Run {
        wire,
        t,
        json,
        first_wait,
        window_end: None,
        window: PAIRING_DEADLINE.as_secs(),
        peer_key: None,
    };
    match run.pair(role) {
        Ok(key_id) => {
            let key_id = clean(&key_id);
            if json {
                let _ = writeln!(
                    run.t.stdout,
                    "{}",
                    cli_object(&[
                        ("ok", "true".to_owned()),
                        ("paired", "true".to_owned()),
                        ("key_id", json_string(&key_id)),
                    ])
                );
            } else {
                let _ = writeln!(
                    run.t.stdout,
                    "oac pair: paired. This device now trusts key {key_id}.\n\
                     Pairing grants nothing: no message is delivered to or from that device until a grant allows it.\n\
                     The other device trusts this one only once its operator has typed this device's code there; if that fails, run `oac unpair {key_id}` here and pair again."
                );
            }
            EXIT_OK
        }
        Err(failure) => report(run.t, "pair", json, &failure, &[]),
    }
}

fn report(
    t: &mut Terminal<'_>,
    verb: &str,
    json: bool,
    failure: &Failure,
    extra: &[(&str, String)],
) -> i32 {
    if json {
        let _ = writeln!(t.stdout, "{}", failure.json(extra));
    }
    let message = match &failure.key_id {
        Some(k) => failure.message.replace("<key_id>", k),
        None => failure.message.clone(),
    };
    let _ = writeln!(t.stderr, "oac {verb}: {message}");
    failure.exit
}

// ---- oac pair --list --------------------------------------------------------------------------

fn list_over(wire: &mut dyn Wire, json: bool, wait: Duration, t: &mut Terminal<'_>) -> i32 {
    let result = ask(wire, PAIRINGS_LINE, wait).and_then(|reply| {
        if let Some(code) = reply.error() {
            return Err(from_error(code));
        }
        reply
            .0
            .get("pairings")
            .and_then(Json::as_object)
            .cloned()
            .ok_or_else(unexpected)
    });
    let body = match result {
        Ok(b) => b,
        Err(failure) => return report(t, "pair --list", json, &failure, &[]),
    };
    let text = |o: &JsonObject, n: &str| o.get(n).and_then(Json::as_str).map(clean);
    let objects = |n: &str| -> Vec<&JsonObject> {
        body.get(n)
            .and_then(Json::as_array)
            .map(|a| a.iter().filter_map(Json::as_object).collect())
            .unwrap_or_default()
    };
    let saved = body.get("saved").and_then(Json::as_bool).unwrap_or(false);
    let (keys, events) = (objects("keys"), objects("events"));
    if json {
        let key_json: Vec<String> = keys
            .iter()
            .map(|k| {
                format!(
                    "{{\"confirmed\":{},\"key_id\":{},\"paired_at\":{},\"principal\":{}}}",
                    k.contains("confirmed"),
                    json_string(&text(k, "key_id").unwrap_or_default()),
                    json_string(&text(k, "paired_at").unwrap_or_default()),
                    json_string(&text(k, "principal").unwrap_or_default()),
                )
            })
            .collect();
        let event_json: Vec<String> = events
            .iter()
            .map(|e| {
                let mut m = vec![
                    ("at", json_string(&text(e, "at").unwrap_or_default())),
                    ("kind", json_string(&text(e, "kind").unwrap_or_default())),
                ];
                if let Some(end) = text(e, "end") {
                    m.push(("end", json_string(&end)));
                }
                if let Some(k) = text(e, "key_id") {
                    m.push(("key_id", json_string(&k)));
                }
                canonical_text(&format!(
                    "{{{}}}",
                    m.iter()
                        .map(|(n, v)| format!("{}:{v}", json_string(n)))
                        .collect::<Vec<_>>()
                        .join(",")
                ))
            })
            .collect();
        let _ = writeln!(
            t.stdout,
            "{}",
            cli_object(&[
                ("ok", "true".to_owned()),
                ("keys", format!("[{}]", key_json.join(","))),
                ("events", format!("[{}]", event_json.join(","))),
                ("saved", saved.to_string()),
            ])
        );
        return EXIT_OK;
    }
    let mut out = format!("Trusted keys ({}):\n", keys.len());
    for k in &keys {
        out.push_str(&format!(
            "  {}  principal {}  paired {}  agreement values {}\n",
            text(k, "key_id").unwrap_or_default(),
            text(k, "principal").unwrap_or_default(),
            text(k, "paired_at").unwrap_or_default(),
            if k.contains("confirmed") {
                "confirmed"
            } else {
                "not confirmed"
            }
        ));
    }
    out.push_str(if saved {
        "Trust store: saved.\n"
    } else {
        "Trust store: NOT saved. A change is held in memory and the daemon is retrying; a restart before it succeeds loses it.\n"
    });
    if events.is_empty() {
        out.push_str("Recent events since the daemon started: none.\n");
    } else {
        out.push_str("Recent events since the daemon started:\n");
        for e in &events {
            let mut line = format!(
                "  {}  {}",
                text(e, "at").unwrap_or_default(),
                text(e, "kind").unwrap_or_default()
            );
            if let Some(end) = text(e, "end") {
                line.push_str(&format!(" ({end})"));
            }
            if let Some(k) = text(e, "key_id") {
                line.push_str(&format!("  {k}"));
            }
            out.push_str(&line);
            out.push('\n');
        }
    }
    let _ = t.stdout.write_all(out.as_bytes());
    EXIT_OK
}

// ---- oac unpair ---------------------------------------------------------------------------------

/// Removes `key_id` over `wire`; the exit status.
pub fn unpair_over(
    wire: &mut dyn Wire,
    key_id: &str,
    json: bool,
    wait: Duration,
    t: &mut Terminal<'_>,
) -> i32 {
    let key = ("key_id", json_string(key_id));
    let result = ask(wire, &unpair_line(key_id), wait).and_then(|reply| {
        if let Some(code) = reply.error() {
            return Err(from_error(code));
        }
        match (reply.flag("unpaired"), reply.flag("durable")) {
            (Some(true), Some(durable)) => Ok(durable),
            _ => Err(unexpected()),
        }
    });
    match result {
        Ok(true) => {
            if json {
                let _ = writeln!(
                    t.stdout,
                    "{}",
                    cli_object(&[
                        ("ok", "true".to_owned()),
                        ("unpaired", "true".to_owned()),
                        ("durable", "true".to_owned()),
                        key,
                    ])
                );
            } else {
                let _ = writeln!(
                    t.stdout,
                    "oac unpair: removed key {key_id}, with every grant and binding that named it, its held statement and its confirmed values."
                );
            }
            EXIT_OK
        }
        Ok(false) => {
            let failure = Failure::new(
                "not_durable",
                EXIT_FAILED,
                format!(
                    "key {key_id} was removed in memory, but NOT saved: the revocation is NOT durable and a restart before it is saved would restore this device. The daemon keeps retrying; after any restart check `oac pair --list` and unpair again if the key is back."
                ),
            );
            report(
                t,
                "unpair",
                json,
                &failure,
                &[
                    ("unpaired", "true".to_owned()),
                    ("durable", "false".to_owned()),
                    key,
                ],
            )
        }
        Err(failure) => report(t, "unpair", json, &failure, &[key]),
    }
}

// ---- the entry points -------------------------------------------------------------------------

/// [`pair`] for a given scope: `oac pair [--respond | --list]`, reading and writing the given
/// streams. `wait` bounds the daemon's answer to a first line and to `--list`.
pub fn pair_with(
    scope: &Scope,
    command: PairCommand,
    json: bool,
    wait: Duration,
    t: &mut Terminal<'_>,
) -> i32 {
    let verb = if command == PairCommand::List {
        "pair --list"
    } else {
        "pair"
    };
    let mut wire = match connect(scope) {
        Ok(w) => w,
        Err(failure) => return report(t, verb, json, &failure, &[]),
    };
    match command {
        PairCommand::Pair(role) => pair_over(&mut wire, role, json, wait, t),
        PairCommand::List => list_over(&mut wire, json, wait, t),
    }
}

/// [`unpair`] for a given scope: confirms on a terminal (`--yes` skips it and is required
/// without one), then removes the key.
pub fn unpair_with(
    scope: &Scope,
    key_id: &str,
    yes: bool,
    json: bool,
    stdin_is_terminal: bool,
    wait: Duration,
    t: &mut Terminal<'_>,
) -> i32 {
    if !yes {
        if !stdin_is_terminal {
            let _ = writeln!(
                t.stderr,
                "oac unpair: standard input is not a terminal, so there is no one to confirm; pass --yes to remove the key"
            );
            return EXIT_USAGE;
        }
        let _ = write!(
            t.stderr,
            "Remove trusted key {key_id}? This also deletes every grant and binding that names it. Type yes to continue: "
        );
        let _ = t.stderr.flush();
        let answer = match read_line_bounded(t.stdin, 16) {
            Line::Text(a) => a.trim().to_ascii_lowercase(),
            _ => String::new(),
        };
        if answer != "yes" && answer != "y" {
            let failure = Failure::new(
                "declined",
                EXIT_FAILED,
                "not confirmed; nothing was removed",
            );
            return report(
                t,
                "unpair",
                json,
                &failure,
                &[("key_id", json_string(key_id))],
            );
        }
    }
    let mut wire = match connect(scope) {
        Ok(w) => w,
        Err(failure) => return report(t, "unpair", json, &failure, &[]),
    };
    unpair_over(&mut wire, key_id, json, wait, t)
}

/// The user's scope, with a test namespace noted on `stderr`.
fn user_scope(verb: &str, stderr: &mut dyn Write) -> io::Result<Scope> {
    let scope = Scope::for_user()?;
    if let Some(namespace) = scope.namespace() {
        let _ = writeln!(stderr, "oac {verb}: using the test namespace {namespace:?}");
    }
    Ok(scope)
}

/// `oac pair [--respond | --list] [--json]`.
pub fn pair(args: &[String]) -> i32 {
    let mut stderr = io::stderr();
    let Some((command, json)) = parse_pair_args(args) else {
        let _ = writeln!(stderr, "{USAGE_PAIR}");
        return EXIT_USAGE;
    };
    let scope = match user_scope("pair", &mut stderr) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(stderr, "oac pair: {e}");
            return EXIT_FAILED;
        }
    };
    let (mut stdin, mut stdout) = (io::stdin().lock(), io::stdout().lock());
    let code = pair_with(
        &scope,
        command,
        json,
        ANSWER_WAIT,
        &mut Terminal {
            stdin: &mut stdin,
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );
    let _ = stdout.flush();
    code
}

/// `oac unpair <key_id> [--yes] [--json]`.
pub fn unpair(args: &[String]) -> i32 {
    use std::io::IsTerminal;
    let mut stderr = io::stderr();
    let Some((key_id, yes, json)) = parse_unpair_args(args) else {
        let _ = writeln!(
            stderr,
            "{USAGE_UNPAIR}\n(the key id is the full 64-digit id `oac pair --list` shows; prefixes are not matched)"
        );
        return EXIT_USAGE;
    };
    let scope = match user_scope("unpair", &mut stderr) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(stderr, "oac unpair: {e}");
            return EXIT_FAILED;
        }
    };
    let is_terminal = io::stdin().is_terminal();
    let (mut stdin, mut stdout) = (io::stdin().lock(), io::stdout().lock());
    let code = unpair_with(
        &scope,
        &key_id,
        yes,
        json,
        is_terminal,
        ANSWER_WAIT,
        &mut Terminal {
            stdin: &mut stdin,
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );
    let _ = stdout.flush();
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_pairing::BLOB_PREFIX;

    #[test]
    fn a_blob_is_wrapped_at_the_width_and_ends_with_an_empty_line() {
        let blob = format!("{BLOB_PREFIX}{}", "A".repeat(200));
        let text = wrapped(&blob);
        assert!(text.ends_with("\n\n"));
        assert!(text.lines().all(|l| l.len() <= WRAP));
        let joined: String = text.lines().collect();
        assert_eq!(joined, blob);
    }

    #[test]
    fn arguments_are_parsed_strictly() {
        let a = |s: &[&str]| -> Vec<String> { s.iter().map(|x| (*x).to_owned()).collect() };
        assert_eq!(
            parse_pair_args(&a(&[])),
            Some((PairCommand::Pair(PairRole::Initiator), false))
        );
        assert_eq!(
            parse_pair_args(&a(&["--json", "--respond"])),
            Some((PairCommand::Pair(PairRole::Responder), true))
        );
        assert_eq!(
            parse_pair_args(&a(&["--list"])),
            Some((PairCommand::List, false))
        );
        for bad in [
            &["--list", "--respond"][..],
            &["--respond", "--respond"],
            &["--json", "--json"],
            &["x"],
            &["--yes"],
        ] {
            assert_eq!(parse_pair_args(&a(bad)), None, "{bad:?}");
        }
        let key = "a".repeat(64);
        assert_eq!(
            parse_unpair_args(&a(&[&key, "--yes", "--json"])),
            Some((key.clone(), true, true))
        );
        for bad in [&[][..], &["abc"], &["--yes"]] {
            assert_eq!(parse_unpair_args(&a(bad)), None);
        }
        assert_eq!(parse_unpair_args(&a(&[&key, &key])), None);
        assert_eq!(parse_unpair_args(&a(&[&"A".repeat(64)])), None);
    }
}
