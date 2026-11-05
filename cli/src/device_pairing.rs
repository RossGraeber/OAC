// SPDX-License-Identifier: Apache-2.0

//! Device pairing over the control endpoint: the control ops and the daemon's state machine
//! (#437, slice S3; `docs/planning/decisions/G-437-pairing.md` §3, §4, §6; the amendment to
//! `docs/planning/decisions/C2-process-model.md` §4).
//!
//! The protocol of the exchange is the core's (`oac_core::pairing`): commit, response,
//! reveal, a six-digit code each side derives, and the operator typing the code the **other**
//! device shows. This module carries it. The operator carries the exchange between the two
//! devices as pasted **blobs**; the verbs (S4) read and print them and this daemon never sees
//! the operator's terminal.
//!
//! # The ops
//!
//! All are lines of JSON under `"oac_control":1`. `pair` (role `initiator` or `responder`)
//! opens the daemon's one pairing and keeps its connection; on that connection the client
//! sends `pair_offer`, `pair_response`, `pair_reveal` (each with a `blob`) and `pair_confirm`
//! (with the code `entered`), whichever the role takes in turn. `unpair` (with a `key_id`)
//! and `pairings` each answer one line on a connection of their own.
//!
//! # What the state machine keeps
//!
//! - **Deadlines.** The first line of the connection keeps the endpoint's 1 KiB and 2 s
//!   bounds (`crate::control`). After a `pair`, each line may be [`MAX_LINE`] bytes and each
//!   read is bounded by the time left of the 120 s pairing deadline. The pairing ends at the
//!   earlier of the tokio deadline and the core clock's 120 s (`oac_core::pairing`), so both
//!   clocks fail closed.
//! - **One pairing per daemon.** A second `pair` while one is open is answered `busy`. It
//!   holds one of the endpoint's four connection places for up to 120 s (availability only;
//!   #438 sizes the endpoint).
//! - **A disconnect abandons.** The responder is told ([`ResponderPairing::abandon`]), the
//!   initiator is dropped, and the pairing ends with a line in the log.
//! - **A second offer ends the responder's pairing; a second response ends the
//!   initiator's.** The initiator's `Ni` is public once revealed, so a second response could
//!   be ground offline until its code matched the one the responder will show. The core
//!   consumes the initiator in `receive` and has no variant for this, so the end is this
//!   module's own ([`End::SecondResponse`]).
//! - **Shutdown ends a pairing, and no store write follows the lock.** Every task of this
//!   module holds an [`Active`] place, taken while the daemon is not closing; it watches the
//!   daemon's stop signal and ends at once; `Daemon::shutdown` waits for all of them
//!   ([`DevicePairing::close`]) before it closes the core and releases the lock; and the core
//!   refuses `pair`, `unpair` and `retry_save` once it is closed.
//! - **Nothing secret leaves.** A blob is public (a man in the middle who alters one is caught
//!   by the code, not by the blob), but the log never holds a blob, a nonce, a code or a
//!   statement: it holds `pairing started`, `pairing ended: <end>`, `paired key <key_id>`
//!   and `unpaired key <key_id>`. Those lines are the visibility lead decision L5 relies on:
//!   a same-user process can use these ops to add a trusted key with no operator, and the
//!   typed code does not stop it.
//! - **A pairing adds trust, nothing more.** The key is trusted and receives nothing until a
//!   grant names it ([SEC-AUZ-001]; the grant verbs are #492). Pairing is not an
//!   instruction channel; a paired key's messages stay untrusted instructions.
//!
//! The daemon's key material stays in the daemon: a client sends and receives blobs and
//! codes, and the initiator's nonce is in no reply until the reveal, which exists only after
//! the response has arrived.

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use oac_core::authorization::{OperatorConfirmed, PairError, RemoveKeyError};
use oac_core::canonical::canonical;
use oac_core::clock::Clock;
use oac_core::ids::{KeyId, Timestamp, Token};
use oac_core::json::{self, Json};
use oac_core::keys::{DeviceIdentity, PublicKey};
use oac_core::pairing::{
    ConfirmError, PAIRING_VALIDITY_MS, PairedPeer, PairingEnd, PairingError, PairingInitiator,
    PairingOffer, PairingResponse, PairingReveal, PairingSession, PairingStore, ResponderPairing,
    StatementValue,
};
use oac_core::pipeline::{Pipelines, TrustChangeError, Unpaired};
use oac_core::sealing::AgreementStatement;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Notify, OwnedSemaphorePermit, watch};
use tokio::time::Instant;

use crate::control::{BUSY_REPLY, canonical_text, json_string, write_line};
use crate::daemon::LogSink;
use crate::ipc::AsyncStream;

/// The longest line of a pairing connection after its first, in bytes (the newline
/// excluded). A control line is not the operator's terminal: the verb wraps what it prints
/// and joins what it reads (§3 of the design note).
pub const MAX_LINE: usize = 4096;

/// How long a pairing stays open: the core's 120 s (`oac_core::pairing`).
pub const PAIRING_DEADLINE: Duration = Duration::from_millis(PAIRING_VALIDITY_MS);

/// How many pairing and unpair events the daemon keeps in memory (design note §11, Q-C).
pub const MAX_EVENTS: usize = 16;

/// The text every blob starts with: the format, and its version.
pub const BLOB_PREFIX: &str = "oacpair1.";

/// Which side of the exchange a daemon plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Prints the offer first (`oac pair`).
    Initiator,
    /// Waits for an offer (`oac pair --respond`).
    Responder,
}

impl Role {
    fn name(self) -> &'static str {
        match self {
            Role::Initiator => "initiator",
            Role::Responder => "responder",
        }
    }
}

// ---- blobs ---------------------------------------------------------------------------------

/// A step of the exchange as the operator carries it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Blob {
    /// Step 1, initiator to responder.
    Offer(PairingOffer),
    /// Step 2, responder to initiator.
    Response(PairingResponse),
    /// Step 3, initiator to responder.
    Reveal(PairingReveal),
}

/// Why a text is not a blob this daemon takes. It never repeats the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlobError {
    /// It does not start with [`BLOB_PREFIX`].
    NotABlob,
    /// The rest is not unpadded base64url of an I-JSON object.
    Encoding,
    /// The object is not in its canonical (RFC 8785) form.
    NotCanonical,
    /// A member is missing, unknown or of the wrong shape.
    Shape,
}

impl fmt::Display for BlobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BlobError::NotABlob => "not a pairing blob",
            BlobError::Encoding => "the blob is not valid base64url of a JSON object",
            BlobError::NotCanonical => "the blob's JSON is not canonical",
            BlobError::Shape => "the blob has a missing, unknown or malformed member",
        })
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn b64_encode(octets: &[u8]) -> String {
    let mut out = String::with_capacity(octets.len().div_ceil(3) * 4);
    for chunk in octets.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |a, (k, &b)| a | (u32::from(b) << (16 - 8 * k)));
        for k in 0..=chunk.len() {
            out.push(B64[((n >> (18 - 6 * k)) & 63) as usize] as char);
        }
    }
    out
}

fn b64_value(c: u8) -> Option<u32> {
    Some(u32::from(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'-' => 62,
        b'_' => 63,
        _ => return None,
    }))
}

/// The octets `s` is the one canonical unpadded base64url encoding of.
fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.as_bytes();
    if s.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3 + 2);
    for chunk in s.chunks(4) {
        let mut n = 0u32;
        for (k, &c) in chunk.iter().enumerate() {
            n |= b64_value(c)? << (18 - 6 * k);
        }
        let octets = chunk.len() - 1;
        // The bits below the last whole octet must be zero: one encoding per octet string.
        let unused = 24 - 8 * octets;
        if unused < 24 && n & ((1 << unused) - 1) != 0 {
            return None;
        }
        for k in 0..octets {
            out.push((n >> (16 - 8 * k)) as u8);
        }
    }
    Some(out)
}

/// One canonical JSON object from members whose values are already JSON text.
fn object(members: &[(&str, String)]) -> String {
    let mut text = String::from("{");
    for (i, (name, value)) in members.iter().enumerate() {
        if i > 0 {
            text.push(',');
        }
        text.push_str(&json_string(name));
        text.push(':');
        text.push_str(value);
    }
    text.push('}');
    canonical_text(&text)
}

fn blob_of(members: &[(&str, String)]) -> String {
    format!("{BLOB_PREFIX}{}", b64_encode(object(members).as_bytes()))
}

fn statement_json(statement: &AgreementStatement) -> String {
    Json::Object(statement.as_json().clone()).to_compact()
}

/// The offer as a blob: `oacpair1.` and the base64url of one canonical JSON object.
pub fn encode_offer(offer: &PairingOffer) -> String {
    let mut m = vec![
        ("kind", json_string("offer")),
        ("principal", json_string(offer.principal.as_str())),
        ("public_key", json_string(&offer.public_key.to_base64url())),
        ("commitment", json_string(&b64_encode(&offer.commitment))),
    ];
    if let Some(statement) = &offer.agreement_statement {
        m.push(("statement", statement_json(statement)));
    }
    blob_of(&m)
}

/// The response as a blob.
pub fn encode_response(response: &PairingResponse) -> String {
    let mut m = vec![
        ("kind", json_string("response")),
        ("principal", json_string(response.principal.as_str())),
        (
            "public_key",
            json_string(&response.public_key.to_base64url()),
        ),
        ("nonce", json_string(&b64_encode(&response.nonce))),
    ];
    if let Some(statement) = &response.agreement_statement {
        m.push(("statement", statement_json(statement)));
    }
    blob_of(&m)
}

/// The reveal as a blob.
pub fn encode_reveal(reveal: &PairingReveal) -> String {
    blob_of(&[
        ("kind", json_string("reveal")),
        ("nonce", json_string(&b64_encode(&reveal.nonce))),
    ])
}

fn only_members(o: &json::JsonObject, required: &[&str], optional: &[&str]) -> bool {
    required.iter().all(|r| o.contains(r))
        && o.names()
            .all(|n| required.contains(&n) || optional.contains(&n))
}

fn octets32(o: &json::JsonObject, name: &str) -> Option<[u8; 32]> {
    b64_decode(o.get(name)?.as_str()?)?.try_into().ok()
}

/// Reads a blob. It is public input from a hostile channel: the text must be exactly the
/// canonical form this module writes, with exactly the members of its `kind`.
///
/// # Errors
///
/// See [`BlobError`].
pub fn decode_blob(text: &str) -> Result<Blob, BlobError> {
    let encoded = text.strip_prefix(BLOB_PREFIX).ok_or(BlobError::NotABlob)?;
    let octets = b64_decode(encoded).ok_or(BlobError::Encoding)?;
    let value = json::parse(&octets).map_err(|_| BlobError::Encoding)?;
    match canonical(&value) {
        Ok(c) if c.as_bytes() == octets.as_slice() => {}
        _ => return Err(BlobError::NotCanonical),
    }
    let o = value.as_object().ok_or(BlobError::Shape)?;
    let principal = |o: &json::JsonObject| o.get("principal")?.as_str().and_then(Token::parse);
    let public_key = |o: &json::JsonObject| {
        o.get("public_key")?
            .as_str()
            .and_then(PublicKey::from_base64url)
    };
    let statement = |o: &json::JsonObject| -> Result<Option<AgreementStatement>, BlobError> {
        match o.get("statement") {
            None => Ok(None),
            Some(v) => AgreementStatement::from_json(v)
                .map(Some)
                .ok_or(BlobError::Shape),
        }
    };
    match o.get("kind").and_then(Json::as_str) {
        Some("offer")
            if only_members(
                o,
                &["kind", "principal", "public_key", "commitment"],
                &["statement"],
            ) =>
        {
            Ok(Blob::Offer(PairingOffer {
                principal: principal(o).ok_or(BlobError::Shape)?,
                public_key: public_key(o).ok_or(BlobError::Shape)?,
                commitment: octets32(o, "commitment").ok_or(BlobError::Shape)?,
                agreement_statement: statement(o)?,
            }))
        }
        Some("response")
            if only_members(
                o,
                &["kind", "principal", "public_key", "nonce"],
                &["statement"],
            ) =>
        {
            Ok(Blob::Response(PairingResponse {
                principal: principal(o).ok_or(BlobError::Shape)?,
                public_key: public_key(o).ok_or(BlobError::Shape)?,
                nonce: octets32(o, "nonce").ok_or(BlobError::Shape)?,
                agreement_statement: statement(o)?,
            }))
        }
        Some("reveal") if only_members(o, &["kind", "nonce"], &[]) => {
            Ok(Blob::Reveal(PairingReveal {
                nonce: octets32(o, "nonce").ok_or(BlobError::Shape)?,
            }))
        }
        _ => Err(BlobError::Shape),
    }
}

// ---- the lines of a pairing connection -----------------------------------------------------

/// What a client sends after `pair`.
#[derive(Debug, PartialEq, Eq)]
enum Followup {
    Offer(String),
    Response(String),
    Reveal(String),
    Confirm(String),
}

/// Why a line is not a request. Each ends the pairing: the framing cannot be trusted after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BadLine {
    Malformed,
    Version,
    UnknownOp,
}

impl BadLine {
    fn reply(self) -> String {
        error_reply(match self {
            BadLine::Malformed => "malformed",
            BadLine::Version => "version",
            BadLine::UnknownOp => "unknown_op",
        })
    }
}

fn parse_followup(line: &[u8]) -> Result<Followup, BadLine> {
    let value = json::parse(line).map_err(|_| BadLine::Malformed)?;
    let o = value.as_object().ok_or(BadLine::Malformed)?;
    let version = o
        .get("oac_control")
        .and_then(Json::as_number)
        .and_then(|n| n.plain_integer_in(0, u64::MAX))
        .ok_or(BadLine::Malformed)?;
    if version != crate::control::VERSION {
        return Err(BadLine::Version);
    }
    let text = |name: &str| {
        o.get(name)
            .and_then(Json::as_str)
            .map(str::to_owned)
            .ok_or(BadLine::Malformed)
    };
    match o.get("op").and_then(Json::as_str) {
        Some("pair_offer") => Ok(Followup::Offer(text("blob")?)),
        Some("pair_response") => Ok(Followup::Response(text("blob")?)),
        Some("pair_reveal") => Ok(Followup::Reveal(text("blob")?)),
        Some("pair_confirm") => Ok(Followup::Confirm(text("entered")?)),
        Some(_) => Err(BadLine::UnknownOp),
        None => Err(BadLine::Malformed),
    }
}

/// A reply line: `"oac_control":1` and `members` (values as JSON text), canonical, with its
/// newline.
fn reply(members: &[(&str, String)]) -> String {
    let mut all = vec![("oac_control", "1".to_owned())];
    all.extend(members.iter().map(|(n, v)| (*n, v.clone())));
    let mut line = object(&all);
    line.push('\n');
    line
}

fn error_reply(code: &str) -> String {
    reply(&[("error", json_string(code))])
}

fn statement_value_json(value: &StatementValue) -> String {
    match value {
        StatementValue::None => object(&[("state", json_string("none"))]),
        StatementValue::Refused => object(&[("state", json_string("refused"))]),
        StatementValue::Given(c) => object(&[
            ("state", json_string("given")),
            (
                "agreement_key",
                json_string(&c.agreement_key().to_base64url()),
            ),
            ("seq", c.seq().to_string()),
            ("issued_at", json_string(c.issued_at().as_str())),
        ]),
    }
}

/// Reads lines of at most [`MAX_LINE`] bytes, keeping what follows a line for the next.
struct Lines {
    buf: Vec<u8>,
}

enum LineEnd {
    /// The peer closed or the stream failed.
    Gone,
    TooLarge,
    /// The pairing deadline passed.
    Deadline,
}

impl Lines {
    async fn read(
        &mut self,
        stream: &mut dyn AsyncStream,
        deadline: Instant,
    ) -> Result<Vec<u8>, LineEnd> {
        loop {
            if let Some(i) = self.buf.iter().position(|b| *b == b'\n') {
                if i > MAX_LINE {
                    return Err(LineEnd::TooLarge);
                }
                let mut line: Vec<u8> = self.buf.drain(..=i).collect();
                line.pop();
                return Ok(line);
            }
            if self.buf.len() > MAX_LINE {
                return Err(LineEnd::TooLarge);
            }
            let mut chunk = [0u8; 1024];
            match tokio::time::timeout_at(deadline, stream.read(&mut chunk)).await {
                Err(_) => return Err(LineEnd::Deadline),
                Ok(Err(_) | Ok(0)) => return Err(LineEnd::Gone),
                Ok(Ok(n)) => self.buf.extend_from_slice(&chunk[..n]),
            }
        }
    }
}

// ---- how a pairing ends --------------------------------------------------------------------

/// Why a pairing ended without trusting the key. Each is logged and, for the operator, shown
/// by the verb.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// A second offer reached the responder ([`PairingEnd::SecondOffer`]).
    SecondOffer,
    /// A second response reached the initiator (this module's own end).
    SecondResponse,
    /// The connection dropped, or a window ran out waiting for the reveal.
    Abandoned,
    /// The 120 s passed.
    Expired,
    /// The peer presented this device's own key.
    OwnKey,
    /// The revealed nonce did not match the commitment.
    CommitmentMismatch,
    /// Five wrong entries.
    Aborted,
    /// The code matched, and the key is already trusted (#430 is not implemented).
    AlreadyTrusted,
    /// The code matched, and the store did not save the key: it was not added.
    NotSaved,
    /// A line could not be read as a request.
    BadLine,
    /// The daemon is stopping.
    Shutdown,
}

impl End {
    /// A short name, for the log and the replies.
    pub fn name(self) -> &'static str {
        match self {
            End::SecondOffer => "second_offer",
            End::SecondResponse => "second_response",
            End::Abandoned => "abandoned",
            End::Expired => "expired",
            End::OwnKey => "own_key",
            End::CommitmentMismatch => "commitment_mismatch",
            End::Aborted => "aborted",
            End::AlreadyTrusted => "already_trusted",
            End::NotSaved => "not_saved",
            End::BadLine => "bad_line",
            End::Shutdown => "shutdown",
        }
    }

    /// The reply: `{"pair":"aborted"}` and its kind for the ends that follow a code, and
    /// `{"pair":"ended","end":...}` for the rest.
    fn reply(self) -> String {
        match self {
            End::Aborted | End::Expired | End::AlreadyTrusted | End::NotSaved => {
                reply(&[("pair", json_string(self.name()))])
            }
            _ => reply(&[
                ("pair", json_string("ended")),
                ("end", json_string(self.name())),
            ]),
        }
    }
}

fn end_of(e: PairingError) -> End {
    match e {
        PairingError::OwnKey | PairingError::Ended(PairingEnd::OwnKey) => End::OwnKey,
        PairingError::CommitmentMismatch | PairingError::Ended(PairingEnd::CommitmentMismatch) => {
            End::CommitmentMismatch
        }
        PairingError::Expired | PairingError::Ended(PairingEnd::Expired) => End::Expired,
        PairingError::Ended(PairingEnd::SecondOffer) => End::SecondOffer,
        PairingError::Ended(PairingEnd::Abandoned) => End::Abandoned,
    }
}

// ---- the state machine of one pairing ------------------------------------------------------

/// What one line did to a pairing.
enum Step {
    /// Answer with this line; the pairing goes on.
    Reply(String),
    /// The pairing ended without trusting the key.
    End(End),
    /// The right code was entered: the key can be added.
    Confirmed(Box<PairedPeer>),
}

/// One operator-started pairing on this device, without any I/O: the daemon's half of the
/// exchange in a role. The core's types hold the protocol; this holds what the connection
/// adds (which op fits which role, the second response, the time).
struct Machine {
    role: Role,
    started: Timestamp,
    initiator: Option<PairingInitiator>,
    responder: Option<ResponderPairing>,
    session: Option<PairingSession>,
    /// The initiator has taken a response (and revealed its nonce).
    responded: bool,
}

impl Machine {
    /// Opens a pairing at `now`; the initiator's offer is returned for the first reply.
    fn open(
        role: Role,
        own: &DeviceIdentity,
        statement: Option<AgreementStatement>,
        now: Timestamp,
    ) -> (Machine, Option<PairingOffer>) {
        let (initiator, responder, offer) = match role {
            Role::Initiator => {
                let (init, offer) = match statement {
                    Some(s) => PairingInitiator::start_with_statement(own, s, now.clone()),
                    None => PairingInitiator::start(own, now.clone()),
                };
                (Some(init), None, Some(offer))
            }
            Role::Responder => {
                let responder = match statement {
                    Some(s) => ResponderPairing::start_with_statement(now.clone(), s),
                    None => ResponderPairing::start(now.clone()),
                };
                (None, Some(responder), None)
            }
        };
        (
            Machine {
                role,
                started: now,
                initiator,
                responder,
                session: None,
                responded: false,
            },
            offer,
        )
    }

    fn expired(&self, now: &Timestamp) -> bool {
        now.unix_nanos() >= self.started.unix_nanos() + i128::from(PAIRING_VALIDITY_MS) * 1_000_000
    }

    /// The connection went away.
    fn abandon(&mut self) {
        if let Some(r) = self.responder.as_mut() {
            r.abandon();
        }
        self.initiator = None;
    }

    fn code_reply(
        session: &PairingSession,
        reveal: Option<&PairingReveal>,
        already_trusted: bool,
    ) -> String {
        let mut m = vec![
            ("pair", json_string("code")),
            ("code", json_string(session.code())),
            ("peer_key_id", json_string(session.peer_key_id().as_str())),
            (
                "peer_principal",
                json_string(session.peer_principal().as_str()),
            ),
            (
                "own_statement",
                statement_value_json(session.own_statement_value()),
            ),
            (
                "peer_statement",
                statement_value_json(session.peer_statement_value()),
            ),
            ("already_trusted", already_trusted.to_string()),
        ];
        if let Some(reveal) = reveal {
            m.push(("blob", json_string(&encode_reveal(reveal))));
        }
        reply(&m)
    }

    fn handle(
        &mut self,
        own: &DeviceIdentity,
        now: &Timestamp,
        is_trusted: &dyn Fn(&KeyId) -> bool,
        followup: Followup,
    ) -> Step {
        if self.expired(now) {
            return Step::End(End::Expired);
        }
        // The ops that do not fit this role are diagnosed and change nothing.
        let wrong = |s: &str| Step::Reply(error_reply(s));
        match followup {
            Followup::Offer(blob) => {
                let Some(responder) = self.responder.as_mut() else {
                    return wrong("wrong_role");
                };
                let Ok(Blob::Offer(offer)) = decode_blob(&blob) else {
                    return wrong("bad_blob");
                };
                match responder.answer(own, offer, now) {
                    Ok(response) => Step::Reply(reply(&[
                        ("pair", json_string("response")),
                        ("blob", json_string(&encode_response(&response))),
                    ])),
                    Err(e) => Step::End(end_of(e)),
                }
            }
            Followup::Reveal(blob) => {
                let Some(responder) = self.responder.as_mut() else {
                    return wrong("wrong_role");
                };
                let Ok(Blob::Reveal(reveal)) = decode_blob(&blob) else {
                    return wrong("bad_blob");
                };
                match responder.reveal(reveal, now) {
                    Ok(session) => {
                        let trusted = is_trusted(&session.peer_key_id());
                        let line = Machine::code_reply(&session, None, trusted);
                        self.session = Some(session);
                        Step::Reply(line)
                    }
                    Err(e) => Step::End(end_of(e)),
                }
            }
            Followup::Response(blob) => {
                if self.role != Role::Initiator {
                    return wrong("wrong_role");
                }
                if self.responded {
                    // `Ni` is public: a second response could be ground until its code
                    // matched the one the responder will show.
                    return Step::End(End::SecondResponse);
                }
                let Ok(Blob::Response(response)) = decode_blob(&blob) else {
                    return wrong("bad_blob");
                };
                let Some(initiator) = self.initiator.take() else {
                    return Step::End(End::Abandoned);
                };
                self.responded = true;
                match initiator.receive(response, now) {
                    Ok((session, reveal)) => {
                        let trusted = is_trusted(&session.peer_key_id());
                        let line = Machine::code_reply(&session, Some(&reveal), trusted);
                        self.session = Some(session);
                        Step::Reply(line)
                    }
                    Err(e) => Step::End(end_of(e)),
                }
            }
            Followup::Confirm(entered) => {
                let Some(session) = self.session.as_mut() else {
                    return wrong("no_code_yet");
                };
                match session.confirm_entered(&entered, now) {
                    Ok(peer) => Step::Confirmed(Box::new(peer)),
                    Err(ConfirmError::Mismatch { attempts_left: 0 } | ConfirmError::Aborted) => {
                        Step::End(End::Aborted)
                    }
                    Err(ConfirmError::Mismatch { attempts_left }) => Step::Reply(reply(&[
                        ("pair", json_string("mismatch")),
                        ("attempts_left", attempts_left.to_string()),
                    ])),
                    Err(ConfirmError::Expired) => Step::End(End::Expired),
                    Err(ConfirmError::Completed) => Step::End(End::Abandoned),
                }
            }
        }
    }
}

// ---- the daemon's pairing state ------------------------------------------------------------

/// What happened, for `oac pair --list` and doctor (S5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// A key was paired.
    Paired,
    /// A key was removed.
    Unpaired,
    /// A pairing ended without trusting a key.
    Ended(End),
}

/// One pairing or unpair event, kept in memory (the last [`MAX_EVENTS`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingEvent {
    /// What happened.
    pub kind: EventKind,
    /// The key it concerns, when there is one.
    pub key_id: Option<KeyId>,
    /// When, on the core clock.
    pub at: Timestamp,
}

/// What [`DevicePairing::unpair`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnpairResult {
    /// The key is removed and the store holds the removal.
    Durable,
    /// The key is removed in memory, but the store did not save it: a restart before a later
    /// save succeeds would restore the revoked device. The daemon retries on a bounded
    /// backoff. The text is the store's.
    NotDurable(String),
    /// The key is not trusted.
    NotTrusted,
    /// The key is this device's own.
    OwnKey,
    /// The daemon is stopping.
    Closed,
}

#[derive(Default)]
struct Tracking {
    closed: bool,
    active: usize,
}

/// A place in the count of running tasks, taken only while the daemon is not closing.
struct Active(Arc<DevicePairing>);

impl Drop for Active {
    fn drop(&mut self) {
        let mut t = self
            .0
            .tracking
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        t.active -= 1;
        if t.active == 0 {
            self.0.idle.notify_waiters();
        }
    }
}

/// The one open pairing of a daemon.
struct Slot(Arc<DevicePairing>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.slot.store(false, Ordering::SeqCst);
    }
}

/// The pairing ops of one daemon: its state machine, the unpair, the save retry and the
/// recent events.
pub struct DevicePairing {
    pipelines: Arc<Pipelines>,
    store: Arc<dyn PairingStore + Send + Sync>,
    clock: Arc<dyn Clock>,
    log: LogSink,
    stopped: watch::Receiver<bool>,
    slot: AtomicBool,
    tracking: Mutex<Tracking>,
    idle: Notify,
    events: Mutex<VecDeque<PairingEvent>>,
    events_changed: watch::Sender<u64>,
    retrying: AtomicBool,
}

impl DevicePairing {
    /// The pairing ops over `pipelines`, saving to `store`. `clock` is the core's clock;
    /// `stopped` is the daemon's stop signal.
    pub fn new(
        pipelines: Arc<Pipelines>,
        store: Arc<dyn PairingStore + Send + Sync>,
        clock: Arc<dyn Clock>,
        log: LogSink,
        stopped: watch::Receiver<bool>,
    ) -> Arc<DevicePairing> {
        Arc::new(DevicePairing {
            pipelines,
            store,
            clock,
            log,
            stopped,
            slot: AtomicBool::new(false),
            tracking: Mutex::new(Tracking::default()),
            idle: Notify::new(),
            events: Mutex::new(VecDeque::new()),
            events_changed: watch::channel(0).0,
            retrying: AtomicBool::new(false),
        })
    }

    /// The last [`MAX_EVENTS`] pairing and unpair events since the daemon started, oldest
    /// first.
    pub fn recent_events(&self) -> Vec<PairingEvent> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    /// A receiver whose value changes each time an event is recorded: for a tool or a test
    /// that waits for the end of a pairing without polling.
    pub fn watch_events(&self) -> watch::Receiver<u64> {
        self.events_changed.subscribe()
    }

    fn record(&self, kind: EventKind, key_id: Option<KeyId>) {
        let event = PairingEvent {
            kind,
            key_id,
            at: self.clock.now(),
        };
        {
            let mut events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
            if events.len() == MAX_EVENTS {
                events.pop_front();
            }
            events.push_back(event);
        }
        self.events_changed.send_modify(|n| *n += 1);
    }

    fn begin(self: &Arc<Self>) -> Option<Active> {
        let mut t = self.tracking.lock().unwrap_or_else(PoisonError::into_inner);
        if t.closed {
            return None;
        }
        t.active += 1;
        Some(Active(self.clone()))
    }

    fn take_slot(self: &Arc<Self>) -> Option<Slot> {
        self.slot
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| Slot(self.clone()))
    }

    /// Stops accepting work and waits for every pairing, unpair and retry task to end. The
    /// daemon calls it after it has signalled the stop (the tasks end at once on it) and
    /// before it closes the core and releases the lock.
    pub async fn close(&self) {
        self.tracking
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .closed = true;
        loop {
            let notified = self.idle.notified();
            if self
                .tracking
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .active
                == 0
            {
                return;
            }
            notified.await;
        }
    }

    fn expires_ms(deadline: Instant) -> String {
        deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .to_string()
    }

    /// Serves the connection of a `pair` op to its end. `rest` is what the client sent after
    /// the first line.
    pub(crate) async fn pair_connection(
        self: Arc<Self>,
        role: Role,
        mut stream: Box<dyn AsyncStream>,
        rest: Vec<u8>,
        _permit: OwnedSemaphorePermit,
    ) {
        let Some(_active) = self.begin() else {
            write_line(&mut *stream, &error_reply("shutting_down")).await;
            return;
        };
        let Some(_slot) = self.take_slot() else {
            self.log.line("refused a pairing: another is open");
            write_line(&mut *stream, BUSY_REPLY).await;
            return;
        };
        let deadline = Instant::now() + PAIRING_DEADLINE;
        let own = self.pipelines.device();
        let (mut machine, offer) = Machine::open(
            role,
            own,
            self.pipelines.agreement_statement(),
            self.clock.now(),
        );
        self.log.line(&format!("pairing started ({})", role.name()));
        let first = match &offer {
            Some(offer) => reply(&[
                ("pair", json_string("offer")),
                ("blob", json_string(&encode_offer(offer))),
                ("expires_ms", DevicePairing::expires_ms(deadline)),
            ]),
            None => reply(&[
                ("pair", json_string("waiting")),
                ("expires_ms", DevicePairing::expires_ms(deadline)),
            ]),
        };
        let outcome = if write_line(&mut *stream, &first).await {
            self.converse(&mut machine, &mut *stream, rest, deadline)
                .await
        } else {
            machine.abandon();
            Err(End::Abandoned)
        };
        match outcome {
            Ok(key_id) => {
                self.log.line(&format!("paired key {key_id}"));
                self.record(EventKind::Paired, Some(key_id.clone()));
                write_line(
                    &mut *stream,
                    &reply(&[
                        ("pair", json_string("paired")),
                        ("key_id", json_string(key_id.as_str())),
                    ]),
                )
                .await;
            }
            Err(end) => {
                machine.abandon();
                self.log.line(&format!("pairing ended: {}", end.name()));
                self.record(EventKind::Ended(end), None);
                write_line(&mut *stream, &end.reply()).await;
            }
        }
        let _ = stream.shutdown().await;
    }

    /// The lines after the first, until the key is paired (its key id) or the pairing ends.
    async fn converse(
        self: &Arc<Self>,
        machine: &mut Machine,
        stream: &mut dyn AsyncStream,
        rest: Vec<u8>,
        deadline: Instant,
    ) -> Result<KeyId, End> {
        let mut lines = Lines { buf: rest };
        let mut stopped = self.stopped.clone();
        let own = self.pipelines.device();
        loop {
            let line = tokio::select! {
                biased;
                // `Err` means the daemon is gone: also a stop.
                _ = stopped.wait_for(|s| *s) => return Err(End::Shutdown),
                line = lines.read(stream, deadline) => line,
            };
            let line = match line {
                Ok(line) => line,
                Err(LineEnd::Gone) => return Err(End::Abandoned),
                Err(LineEnd::Deadline) => return Err(End::Expired),
                Err(LineEnd::TooLarge) => {
                    write_line(stream, &error_reply("too_large")).await;
                    return Err(End::BadLine);
                }
            };
            let followup = match parse_followup(&line) {
                Ok(f) => f,
                Err(bad) => {
                    write_line(stream, &bad.reply()).await;
                    return Err(End::BadLine);
                }
            };
            let pipelines = &self.pipelines;
            let trusted = |k: &KeyId| pipelines.with_engine(|e| e.trusted_keys().get(k).is_some());
            match machine.handle(own, &self.clock.now(), &trusted, followup) {
                Step::Reply(text) => {
                    if !write_line(stream, &text).await {
                        return Err(End::Abandoned);
                    }
                }
                Step::End(end) => return Err(end),
                Step::Confirmed(peer) => return self.finish(*peer).await,
            }
        }
    }

    /// The final step: the confirmed key is added. One function, so that #430 (confirming the
    /// agreement values of an already-trusted key) replaces only its `AlreadyTrusted` arm.
    async fn finish(self: &Arc<Self>, peer: PairedPeer) -> Result<KeyId, End> {
        let key_id = peer.key_id();
        let (pipelines, store) = (self.pipelines.clone(), self.store.clone());
        // The save and the announcements can wait on a disk and a transport.
        match tokio::task::spawn_blocking(move || pipelines.pair(peer, &*store)).await {
            Ok(Ok(())) => Ok(key_id),
            Ok(Err(TrustChangeError::Pair(PairError::AlreadyTrusted(_)))) => {
                Err(End::AlreadyTrusted)
            }
            Ok(Err(TrustChangeError::Closed)) => Err(End::Shutdown),
            Ok(Err(e)) => {
                self.log.line(&format!("the pairing was not saved: {e}"));
                Err(End::NotSaved)
            }
            Err(_) => Err(End::Abandoned),
        }
    }

    /// Removes the trusted key `key_id` with its grants and bindings, in one step
    /// (`Pipelines::unpair`). When the store does not save the removal the daemon retries on a
    /// bounded backoff ([`oac_core::pipeline::TRUST_SAVE_RETRY_FIRST`] doubling to
    /// [`oac_core::pipeline::TRUST_SAVE_RETRY_CAP`], on the tokio clock) until a save
    /// succeeds.
    pub async fn unpair(self: &Arc<Self>, key_id: &KeyId) -> UnpairResult {
        let Some(_active) = self.begin() else {
            return UnpairResult::Closed;
        };
        let (pipelines, store, key) = (self.pipelines.clone(), self.store.clone(), key_id.clone());
        let result = tokio::task::spawn_blocking(move || {
            pipelines.unpair(&key, OperatorConfirmed::by_operator(), &*store)
        })
        .await;
        match result {
            Ok(Ok(Unpaired::Durable(_))) => {
                self.log.line(&format!("unpaired key {key_id}"));
                self.record(EventKind::Unpaired, Some(key_id.clone()));
                UnpairResult::Durable
            }
            Ok(Ok(Unpaired::NotDurable(e))) => {
                self.log.line(&format!(
                    "unpaired key {key_id}: the removal is NOT saved and is not durable ({e}); retrying"
                ));
                self.record(EventKind::Unpaired, Some(key_id.clone()));
                self.start_retry();
                UnpairResult::NotDurable(e.to_string())
            }
            Ok(Err(TrustChangeError::Remove(RemoveKeyError::OwnKey))) => UnpairResult::OwnKey,
            Ok(Err(TrustChangeError::Remove(_))) => UnpairResult::NotTrusted,
            Ok(Err(_)) | Err(_) => UnpairResult::Closed,
        }
    }

    /// The reply line to an `unpair` op.
    pub async fn unpair_reply(self: &Arc<Self>, key_id: KeyId) -> String {
        match self.unpair(&key_id).await {
            UnpairResult::Durable => reply(&[
                ("unpaired", "true".to_owned()),
                ("durable", "true".to_owned()),
            ]),
            UnpairResult::NotDurable(_) => reply(&[
                ("unpaired", "true".to_owned()),
                ("durable", "false".to_owned()),
            ]),
            UnpairResult::NotTrusted => error_reply("not_trusted"),
            UnpairResult::OwnKey => error_reply("own_key"),
            UnpairResult::Closed => error_reply("shutting_down"),
        }
    }

    /// Arms the save retry, once.
    fn start_retry(self: &Arc<Self>) {
        if self.retrying.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(active) = self.begin() else {
            self.retrying.store(false, Ordering::SeqCst);
            return;
        };
        let this = self.clone();
        tokio::spawn(async move {
            this.retry_loop().await;
            drop(active);
        });
    }

    async fn retry_loop(self: Arc<Self>) {
        let mut stopped = self.stopped.clone();
        loop {
            while let Some(wait) = self.pipelines.next_trust_retry_in() {
                tokio::select! {
                    biased;
                    _ = stopped.wait_for(|s| *s) => {
                        self.retrying.store(false, Ordering::SeqCst);
                        return;
                    }
                    () = tokio::time::sleep(wait) => {}
                }
                let (pipelines, store) = (self.pipelines.clone(), self.store.clone());
                match tokio::task::spawn_blocking(move || pipelines.retry_save(&*store)).await {
                    Ok(Ok(())) => self.log.line("the trust store was saved again"),
                    Ok(Err(TrustChangeError::Closed)) | Err(_) => {
                        self.retrying.store(false, Ordering::SeqCst);
                        return;
                    }
                    Ok(Err(e)) => self.log.line(&format!(
                        "saving the trust store failed again ({e}); retrying"
                    )),
                }
            }
            // Nothing is unsaved. A removal that failed to save between the last check and
            // here would find the flag set and not arm a second task: look again.
            self.retrying.store(false, Ordering::SeqCst);
            if self.pipelines.next_trust_retry_in().is_none()
                || self.retrying.swap(true, Ordering::SeqCst)
            {
                return;
            }
        }
    }

    /// The reply line to a `pairings` op: the trusted keys (other than this device's) with
    /// their confirmed values, whether the store holds the current state, and the recent
    /// events.
    pub fn pairings_reply(&self) -> String {
        let own = self.pipelines.device().key_id().clone();
        let snapshot = self.pipelines.with_engine(|e| e.snapshot());
        let saved = self.pipelines.next_trust_retry_at().is_none();
        let mut keys: Vec<String> = Vec::new();
        for record in snapshot.paired.iter().filter(|r| r.key_id() != own) {
            let mut m = vec![
                ("key_id", json_string(record.key_id().as_str())),
                ("principal", json_string(record.principal().as_str())),
                ("paired_at", json_string(record.paired_at().as_str())),
            ];
            if let Some(c) = record.confirmed() {
                m.push((
                    "confirmed",
                    statement_value_json(&StatementValue::Given(c.clone())),
                ));
            }
            keys.push(object(&m));
        }
        let events: Vec<String> = self
            .recent_events()
            .iter()
            .map(|e| {
                let mut m = vec![("at", json_string(e.at.as_str()))];
                match e.kind {
                    EventKind::Paired => m.push(("kind", json_string("paired"))),
                    EventKind::Unpaired => m.push(("kind", json_string("unpaired"))),
                    EventKind::Ended(end) => {
                        m.push(("kind", json_string("ended")));
                        m.push(("end", json_string(end.name())));
                    }
                }
                if let Some(k) = &e.key_id {
                    m.push(("key_id", json_string(k.as_str())));
                }
                object(&m)
            })
            .collect();
        let body = object(&[
            ("keys", format!("[{}]", keys.join(","))),
            ("events", format!("[{}]", events.join(","))),
            ("saved", saved.to_string()),
        ]);
        reply(&[("pairings", body)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oac_core::keys::DeviceKey;

    fn identity(principal: &str) -> DeviceIdentity {
        DeviceIdentity::new(DeviceKey::generate(), Token::parse(principal).unwrap())
    }

    fn at(ms: i64) -> Timestamp {
        Timestamp::from_unix_millis(1_790_000_000_000 + ms).unwrap()
    }

    #[test]
    fn base64url_is_strict_and_round_trips() {
        for len in 0..40usize {
            let octets: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let text = b64_encode(&octets);
            assert_eq!(b64_decode(&text), Some(octets), "{len}");
        }
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64_decode("Zm8"), Some(b"fo".to_vec()));
        // Unused low-order bits set, padding, the standard alphabet, a length that no octet
        // string has.
        assert_eq!(b64_decode("Zm9"), None);
        assert_eq!(b64_decode("Zm8="), None);
        assert_eq!(b64_decode("+/+/"), None);
        assert_eq!(b64_decode("Z"), None);
    }

    #[test]
    fn a_blob_round_trips_and_only_in_its_canonical_form() {
        let (a, b) = (identity("alpha"), identity("beta"));
        let (init, offer) = PairingInitiator::start(&a, at(0));
        let blob = encode_offer(&offer);
        assert!(blob.starts_with(BLOB_PREFIX));
        assert_eq!(decode_blob(&blob), Ok(Blob::Offer(offer.clone())));
        let mut pairing = ResponderPairing::start(at(0));
        let response = pairing.answer(&b, offer, &at(1)).unwrap();
        assert_eq!(
            decode_blob(&encode_response(&response)),
            Ok(Blob::Response(response.clone()))
        );
        let (_, reveal) = init.receive(response, &at(2)).unwrap();
        assert_eq!(
            decode_blob(&encode_reveal(&reveal)),
            Ok(Blob::Reveal(reveal.clone()))
        );
        // The same members in another order or with spaces are not the blob this module
        // writes.
        let text = format!(
            "{{ \"nonce\": \"{}\", \"kind\": \"reveal\" }}",
            b64_encode(&reveal.nonce)
        );
        let loose = format!("{BLOB_PREFIX}{}", b64_encode(text.as_bytes()));
        assert_eq!(decode_blob(&loose), Err(BlobError::NotCanonical));
    }

    #[test]
    fn a_blob_with_the_wrong_members_or_kind_is_refused() {
        let nonce = b64_encode(&[7u8; 32]);
        let make = |members: &[(&str, String)]| blob_of(members);
        for (case, blob) in [
            ("no prefix", "reveal".to_owned()),
            ("not base64", format!("{BLOB_PREFIX}!!")),
            (
                "an unknown member",
                make(&[
                    ("kind", json_string("reveal")),
                    ("nonce", json_string(&nonce)),
                    ("extra", "1".to_owned()),
                ]),
            ),
            ("no nonce", make(&[("kind", json_string("reveal"))])),
            (
                "a short nonce",
                make(&[
                    ("kind", json_string("reveal")),
                    ("nonce", json_string(&b64_encode(&[7u8; 31]))),
                ]),
            ),
            (
                "an unknown kind",
                make(&[
                    ("kind", json_string("offerr")),
                    ("nonce", json_string(&nonce)),
                ]),
            ),
            (
                "an offer without a commitment",
                make(&[
                    ("kind", json_string("offer")),
                    ("principal", json_string("alpha")),
                ]),
            ),
        ] {
            assert!(decode_blob(&blob).is_err(), "{case}");
        }
        assert_eq!(decode_blob("reveal"), Err(BlobError::NotABlob));
    }

    #[test]
    fn follow_up_lines_are_parsed_strictly() {
        let ok = br#"{"blob":"x","oac_control":1,"op":"pair_offer"}"#;
        assert_eq!(parse_followup(ok), Ok(Followup::Offer("x".into())));
        assert_eq!(
            parse_followup(br#"{"entered":"123456","oac_control":1,"op":"pair_confirm"}"#),
            Ok(Followup::Confirm("123456".into()))
        );
        assert_eq!(parse_followup(b"x"), Err(BadLine::Malformed));
        assert_eq!(
            parse_followup(br#"{"oac_control":2,"op":"pair_offer","blob":"x"}"#),
            Err(BadLine::Version)
        );
        assert_eq!(
            parse_followup(br#"{"oac_control":1,"op":"stop"}"#),
            Err(BadLine::UnknownOp)
        );
        assert_eq!(
            parse_followup(br#"{"oac_control":1,"op":"pair_offer"}"#),
            Err(BadLine::Malformed)
        );
    }

    #[test]
    fn the_replies_are_canonical_lines() {
        for line in [
            End::SecondOffer.reply(),
            End::Aborted.reply(),
            error_reply("bad_blob"),
            BadLine::Version.reply(),
        ] {
            assert!(line.ends_with('\n'));
            let text = line.trim_end();
            assert_eq!(canonical_text(text), text);
            assert!(text.contains("\"oac_control\":1"), "{text}");
        }
    }
}
