// SPDX-License-Identifier: Apache-2.0

//! Strict I-JSON reading and writing.
//!
//! Every wire value of `spec/session-channels.md` (envelope, receipt, presence record,
//! session descriptor, capability declaration) is an I-JSON message ([SC-ENV-002],
//! [SC-RCP-020], [SC-DLV-021]; [RFC7493]). This module reads one octet string into a
//! [`Json`] tree and refuses, as a [`JsonError`], everything I-JSON forbids:
//!
//! - octets that are not UTF-8 (RFC 7493 §2.1);
//! - text outside the RFC 8259 grammar;
//! - a lone surrogate or a Unicode noncharacter in a member name or string value
//!   (RFC 7493 §2.1);
//! - a duplicate member name in any object (RFC 7493 §2.3).
//!
//! The tree keeps what the wire rules need and a general-purpose JSON value loses:
//!
//! - each number's exact spelling ([`JsonNumber::raw`]), because [SC-ENV-050],
//!   [SC-ID-066], [SC-DLV-024] and [SC-DLV-028] constrain how an integer is written
//!   ("no sign, fraction or exponent"), and [SEC-SIG-013] canonicalizes a number from its
//!   decimal value;
//! - each object's members in received order, so a value can be written back member for
//!   member ([IFC-TYP-003], [IFC-TYP-040]).
//!
//! [`Json::to_compact`] writes a tree back as JSON text. [`crate::canonical`] writes the
//! RFC 8785 canonical form used for signing.
//!
//! # No limit on nesting
//!
//! Nothing in this module recurses over the tree: the reader, the writers, the walk
//! ([`Json::events`]), `Clone`, `PartialEq`, `Debug` and `Drop` each keep an explicit
//! stack on the heap. Nesting depth is therefore bounded only by the input's size, and an
//! envelope within the receiver's size limit is never refused for its depth
//! ([SC-ENV-004], [SC-ENV-090]).

use std::collections::HashSet;
use std::fmt;

/// A JSON value read under the I-JSON rules.
///
/// `Clone`, `PartialEq`, `Debug` and `Drop` are implemented without recursion, so a value
/// of any depth can be copied, compared, printed and dropped on a small stack.
#[derive(Eq)]
pub enum Json {
    /// `null`. Valid JSON, but no wire value of this specification may contain it
    /// ([SC-ENV-003]); see [`Json::contains_null`].
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number, with its spelling as received.
    Number(JsonNumber),
    /// A string, decoded. It holds no lone surrogate and no noncharacter.
    String(String),
    /// An array.
    Array(Vec<Json>),
    /// An object.
    Object(JsonObject),
}

/// A JSON number, kept as its RFC 8259 spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonNumber {
    raw: String,
}

/// A JSON object: members in received order, names unique (RFC 7493 §2.3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JsonObject {
    members: Vec<(String, Json)>,
}

/// Why octets are not an I-JSON message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonError {
    /// The octets are not well-formed UTF-8 (RFC 7493 §2.1).
    NotUtf8,
    /// The text breaks the RFC 8259 grammar at `offset` (an octet offset).
    Syntax {
        /// Octet offset of the failure.
        offset: usize,
        /// What was wrong.
        reason: &'static str,
    },
    /// A member name or string value holds a lone surrogate or a noncharacter
    /// (RFC 7493 §2.1).
    ForbiddenCodePoint,
    /// An object repeats a member name (RFC 7493 §2.3).
    DuplicateMember(String),
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonError::NotUtf8 => f.write_str("not UTF-8"),
            JsonError::Syntax { offset, reason } => write!(f, "{reason} at offset {offset}"),
            JsonError::ForbiddenCodePoint => f.write_str("lone surrogate or noncharacter"),
            JsonError::DuplicateMember(name) => write!(f, "duplicate member name {name:?}"),
        }
    }
}

impl std::error::Error for JsonError {}

/// True when `c` is a Unicode noncharacter (U+FDD0 to U+FDEF, or a code point ending in
/// FFFE or FFFF), which RFC 7493 §2.1 forbids.
fn is_noncharacter(c: char) -> bool {
    let u = c as u32;
    (0xFDD0..=0xFDEF).contains(&u) || (u & 0xFFFE) == 0xFFFE
}

/// True when `s` may be an I-JSON member name or string value: it holds no noncharacter
/// (RFC 7493 §2.1). A Rust string cannot hold a lone surrogate.
pub fn is_ijson_string(s: &str) -> bool {
    !s.chars().any(is_noncharacter)
}

/// Reads `octets` as one I-JSON message.
///
/// # Errors
///
/// Returns a [`JsonError`] for anything that is not an I-JSON message.
pub fn parse(octets: &[u8]) -> Result<Json, JsonError> {
    // std::str::from_utf8 rejects overlong forms, encoded surrogates and truncation.
    let text = std::str::from_utf8(octets).map_err(|_| JsonError::NotUtf8)?;
    let mut p = Parser {
        s: text.as_bytes(),
        text,
        i: 0,
    };
    let v = p.document()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.err("trailing characters"));
    }
    Ok(v)
}

/// A container the reader has opened and not yet closed.
enum Open {
    Array(Vec<Json>),
    Object {
        members: Vec<(String, Json)>,
        seen: HashSet<String>,
        name: String,
    },
}

struct Parser<'a> {
    s: &'a [u8],
    text: &'a str,
    i: usize,
}

impl Parser<'_> {
    fn err(&self, reason: &'static str) -> JsonError {
        JsonError::Syntax {
            offset: self.i,
            reason,
        }
    }

    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    /// One value, read with an explicit stack of open containers: no recursion.
    fn document(&mut self) -> Result<Json, JsonError> {
        let mut stack: Vec<Open> = Vec::new();
        'value: loop {
            self.ws();
            let mut v = match self.peek() {
                None => return Err(self.err("unexpected end")),
                Some(b'[') => {
                    self.i += 1;
                    self.ws();
                    if self.peek() == Some(b']') {
                        self.i += 1;
                        Json::Array(Vec::new())
                    } else {
                        stack.push(Open::Array(Vec::new()));
                        continue 'value;
                    }
                }
                Some(b'{') => {
                    self.i += 1;
                    self.ws();
                    if self.peek() == Some(b'}') {
                        self.i += 1;
                        Json::Object(JsonObject::default())
                    } else {
                        let name = self.member_name()?;
                        stack.push(Open::Object {
                            members: Vec::new(),
                            seen: HashSet::new(),
                            name,
                        });
                        continue 'value;
                    }
                }
                Some(b'"') => Json::String(self.string()?),
                Some(b't') => self.literal("true", Json::Bool(true))?,
                Some(b'f') => self.literal("false", Json::Bool(false))?,
                Some(b'n') => self.literal("null", Json::Null)?,
                Some(b'-' | b'0'..=b'9') => self.number()?,
                Some(_) => return Err(self.err("unexpected character")),
            };
            // `v` is complete: add it to the innermost open container, closing every
            // container that ends here.
            loop {
                let Some(top) = stack.last_mut() else {
                    return Ok(v);
                };
                match top {
                    Open::Array(items) => {
                        items.push(v);
                        self.ws();
                        match self.peek() {
                            Some(b',') => {
                                self.i += 1;
                                continue 'value;
                            }
                            Some(b']') => self.i += 1,
                            _ => return Err(self.err("expected , or ]")),
                        }
                    }
                    Open::Object {
                        members,
                        seen,
                        name,
                    } => {
                        let name = std::mem::take(name);
                        if !seen.insert(name.clone()) {
                            return Err(JsonError::DuplicateMember(name));
                        }
                        members.push((name, v));
                        self.ws();
                        match self.peek() {
                            Some(b',') => {
                                self.i += 1;
                                let next = self.member_name()?;
                                if let Some(Open::Object { name, .. }) = stack.last_mut() {
                                    *name = next;
                                }
                                continue 'value;
                            }
                            Some(b'}') => self.i += 1,
                            _ => return Err(self.err("expected , or }")),
                        }
                    }
                }
                v = match stack.pop() {
                    Some(Open::Array(items)) => Json::Array(items),
                    Some(Open::Object { members, .. }) => Json::Object(JsonObject { members }),
                    None => unreachable!("the loop above saw a container"),
                };
            }
        }
    }

    /// A member name and its `:`.
    fn member_name(&mut self) -> Result<String, JsonError> {
        self.ws();
        if self.peek() != Some(b'"') {
            return Err(self.err("expected member name"));
        }
        let name = self.string()?;
        self.ws();
        if self.peek() != Some(b':') {
            return Err(self.err("expected :"));
        }
        self.i += 1;
        Ok(name)
    }

    fn literal(&mut self, word: &str, v: Json) -> Result<Json, JsonError> {
        if self.s[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(self.err("bad literal"))
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
            self.i += 1;
        }
        self.i - start
    }

    // RFC 8259 §6: -?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?
    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.i;
        if self.s[self.i] == b'-' {
            self.i += 1;
        }
        match self.peek() {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(self.err("bad number")),
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            if self.digits() == 0 {
                return Err(self.err("bad number fraction"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if self.digits() == 0 {
                return Err(self.err("bad number exponent"));
            }
        }
        Ok(Json::Number(JsonNumber {
            raw: self.text[start..self.i].to_owned(),
        }))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let h = self
            .s
            .get(self.i..self.i + 4)
            .ok_or_else(|| self.err("bad escape"))?;
        let mut v = 0u32;
        for &b in h {
            let d = (b as char)
                .to_digit(16)
                .ok_or_else(|| self.err("bad escape"))?;
            v = v * 16 + d;
        }
        self.i += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.i += 1; // opening quote
        let mut out = String::new();
        loop {
            let start = self.i;
            while self.i < self.s.len() && !matches!(self.s[self.i], b'"' | b'\\' | 0x00..=0x1f) {
                self.i += 1;
            }
            out.push_str(&self.text[start..self.i]);
            match self.peek() {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => {
                    self.i += 1;
                    break;
                }
                Some(b'\\') => {
                    self.i += 1;
                    let e = self.peek().ok_or_else(|| self.err("bad escape"))?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let cp = if (0xD800..=0xDBFF).contains(&hi) {
                                // A high surrogate must be followed by an escaped low one;
                                // anything else is a lone surrogate (RFC 7493 §2.1).
                                if self.s.get(self.i..self.i + 2) != Some(b"\\u".as_slice()) {
                                    return Err(JsonError::ForbiddenCodePoint);
                                }
                                self.i += 2;
                                let lo = self.hex4()?;
                                if !(0xDC00..=0xDFFF).contains(&lo) {
                                    return Err(JsonError::ForbiddenCodePoint);
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else if (0xDC00..=0xDFFF).contains(&hi) {
                                return Err(JsonError::ForbiddenCodePoint);
                            } else {
                                hi
                            };
                            out.push(char::from_u32(cp).ok_or(JsonError::ForbiddenCodePoint)?);
                        }
                        _ => return Err(self.err("bad escape")),
                    }
                }
                Some(_) => return Err(self.err("control character in string")),
            }
        }
        if !is_ijson_string(&out) {
            return Err(JsonError::ForbiddenCodePoint);
        }
        Ok(out)
    }
}

impl JsonNumber {
    /// The number as written, an RFC 8259 `number`.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// A non-negative integer written with no sign, fraction or exponent, as
    /// [SC-ENV-050], [SC-ID-066], [SC-DLV-024] and [SC-DLV-028] require, within
    /// `min..=max`. `None` for any other spelling or value.
    pub fn plain_integer_in(&self, min: u64, max: u64) -> Option<u64> {
        let r = self.raw.as_bytes();
        let plain =
            !r.is_empty() && r.iter().all(u8::is_ascii_digit) && (r.len() == 1 || r[0] != b'0');
        if !plain {
            return None;
        }
        let v: u64 = self.raw.parse().ok()?;
        (min..=max).contains(&v).then_some(v)
    }

    /// The IEEE 754 double nearest to the number's decimal value, ties to even
    /// ([SEC-SIG-013]). Rust's `f64` parser rounds correctly. Infinite when the value is
    /// beyond the double range.
    pub fn to_f64(&self) -> f64 {
        // The RFC 8259 grammar is a subset of what `f64::from_str` accepts.
        self.raw.parse().unwrap_or(f64::NAN)
    }

    /// A number with the plain spelling of `v`.
    pub fn from_u64(v: u64) -> Self {
        JsonNumber { raw: v.to_string() }
    }
}

impl JsonObject {
    /// An empty object.
    pub fn new() -> Self {
        JsonObject::default()
    }

    /// The value of member `name`, if present.
    pub fn get(&self, name: &str) -> Option<&Json> {
        self.members.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    /// True when the object has a member `name`.
    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// Members in order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Json)> {
        self.members.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Member names in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.members.iter().map(|(k, _)| k.as_str())
    }

    /// Number of members.
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// True when the object has no members.
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// Sets member `name`, replacing an existing one in place or appending a new one, so
    /// names stay unique. Used only when building a value this implementation sends.
    pub(crate) fn insert(&mut self, name: &str, v: Json) {
        if let Some(slot) = self.members.iter_mut().find(|(k, _)| k == name) {
            slot.1 = v;
        } else {
            self.members.push((name.to_owned(), v));
        }
    }

    /// Removes member `name`, returning its value.
    pub(crate) fn remove(&mut self, name: &str) -> Option<Json> {
        let pos = self.members.iter().position(|(k, _)| k == name)?;
        Some(self.members.remove(pos).1)
    }
}

/// One step of a depth-first walk of a [`Json`] value ([`Json::events`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event<'a> {
    /// `null`, a boolean, a number or a string.
    Scalar(&'a Json),
    /// `[`.
    ArrayStart,
    /// `]`.
    ArrayEnd,
    /// `{`.
    ObjectStart,
    /// A member name; the member's value follows.
    Name(&'a str),
    /// `}`.
    ObjectEnd,
}

enum Frame<'a> {
    Array(std::slice::Iter<'a, Json>),
    Object(std::vec::IntoIter<(&'a str, &'a Json)>),
}

/// A depth-first walk of a [`Json`] value with an explicit stack.
pub struct Events<'a> {
    stack: Vec<Frame<'a>>,
    pending: Option<&'a Json>,
    sorted: bool,
}

impl<'a> Iterator for Events<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Event<'a>> {
        if let Some(v) = self.pending.take() {
            return Some(match v {
                Json::Array(a) => {
                    self.stack.push(Frame::Array(a.iter()));
                    Event::ArrayStart
                }
                Json::Object(o) => {
                    let mut m: Vec<(&str, &Json)> = o.iter().collect();
                    if self.sorted {
                        // RFC 8785 §3.2.3: by member name, as arrays of UTF-16 code units.
                        m.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
                    }
                    self.stack.push(Frame::Object(m.into_iter()));
                    Event::ObjectStart
                }
                scalar => Event::Scalar(scalar),
            });
        }
        match self.stack.last_mut()? {
            Frame::Array(it) => match it.next() {
                Some(v) => {
                    self.pending = Some(v);
                    self.next()
                }
                None => {
                    self.stack.pop();
                    Some(Event::ArrayEnd)
                }
            },
            Frame::Object(it) => match it.next() {
                Some((k, v)) => {
                    self.pending = Some(v);
                    Some(Event::Name(k))
                }
                None => {
                    self.stack.pop();
                    Some(Event::ObjectEnd)
                }
            },
        }
    }
}

/// Writes a walk as JSON text with no insignificant whitespace, using `scalar` and `name`
/// to write leaves. No recursion.
pub(crate) fn write_events<'a, E>(
    events: Events<'a>,
    mut scalar: impl FnMut(&'a Json, &mut String) -> Result<(), E>,
    mut name: impl FnMut(&'a str, &mut String) -> Result<(), E>,
) -> Result<String, E> {
    // For each open container: (is an array, no element written yet).
    let mut levels: Vec<(bool, bool)> = Vec::new();
    let mut out = String::new();
    for ev in events {
        match ev {
            Event::ArrayEnd => {
                levels.pop();
                out.push(']');
                continue;
            }
            Event::ObjectEnd => {
                levels.pop();
                out.push('}');
                continue;
            }
            Event::Name(k) => {
                if let Some(level) = levels.last_mut() {
                    if !level.1 {
                        out.push(',');
                    }
                    level.1 = false;
                }
                name(k, &mut out)?;
                out.push(':');
                continue;
            }
            _ => {}
        }
        if let Some(level) = levels.last_mut().filter(|l| l.0) {
            if !level.1 {
                out.push(',');
            }
            level.1 = false;
        }
        match ev {
            Event::Scalar(v) => scalar(v, &mut out)?,
            Event::ArrayStart => {
                out.push('[');
                levels.push((true, true));
            }
            Event::ObjectStart => {
                out.push('{');
                levels.push((false, true));
            }
            _ => unreachable!("handled above"),
        }
    }
    Ok(out)
}

impl Json {
    /// The string, if this is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    /// The object, if this is one.
    pub fn as_object(&self) -> Option<&JsonObject> {
        match self {
            Json::Object(o) => Some(o),
            _ => None,
        }
    }

    /// The array, if this is one.
    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The boolean, if this is one.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The number, if this is one.
    pub fn as_number(&self) -> Option<&JsonNumber> {
        match self {
            Json::Number(n) => Some(n),
            _ => None,
        }
    }

    /// The object, taken out of this value, if this is one. (`Json` implements `Drop`, so
    /// its fields cannot be moved out by a pattern.)
    pub fn into_object(mut self) -> Option<JsonObject> {
        match &mut self {
            Json::Object(o) => Some(std::mem::take(o)),
            _ => None,
        }
    }

    /// A depth-first walk of the value, object members in stored order.
    pub fn events(&self) -> Events<'_> {
        Events {
            stack: Vec::new(),
            pending: Some(self),
            sorted: false,
        }
    }

    /// A depth-first walk with object members sorted by their UTF-16 code units, the
    /// order of RFC 8785 §3.2.3.
    pub(crate) fn events_sorted(&self) -> Events<'_> {
        Events {
            stack: Vec::new(),
            pending: Some(self),
            sorted: true,
        }
    }

    /// True when a JSON `null` appears anywhere in the value, as a member value or an
    /// array element ([SC-ENV-003], [SC-RCP-020], [SC-DLV-021]).
    pub fn contains_null(&self) -> bool {
        self.events()
            .any(|e| matches!(e, Event::Scalar(Json::Null)))
    }

    /// The value as compact JSON text: no insignificant whitespace, object members in
    /// stored order, numbers as spelled, and strings escaped as ECMAScript
    /// `JSON.stringify` escapes them (`"`, `\`, and control characters only).
    pub fn to_compact(&self) -> String {
        let r: Result<String, std::convert::Infallible> = write_events(
            self.events(),
            |v, out| {
                write_scalar(v, out);
                Ok(())
            },
            |k, out| {
                write_string(k, out);
                Ok(())
            },
        );
        match r {
            Ok(s) => s,
        }
    }

    fn scalar_clone(&self) -> Json {
        match self {
            Json::Null => Json::Null,
            Json::Bool(b) => Json::Bool(*b),
            Json::Number(n) => Json::Number(n.clone()),
            Json::String(s) => Json::String(s.clone()),
            Json::Array(_) | Json::Object(_) => unreachable!("not a scalar"),
        }
    }
}

fn write_scalar(v: &Json, out: &mut String) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Number(n) => out.push_str(&n.raw),
        Json::String(s) => write_string(s, out),
        Json::Array(_) | Json::Object(_) => unreachable!("not a scalar"),
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

enum Building {
    Array(Vec<Json>),
    Object(Vec<(String, Json)>, String),
}

impl Clone for Json {
    /// Copies the value with an explicit stack: no recursion.
    fn clone(&self) -> Json {
        let mut stack: Vec<Building> = Vec::new();
        for ev in self.events() {
            let done = match ev {
                Event::ArrayStart => {
                    stack.push(Building::Array(Vec::new()));
                    continue;
                }
                Event::ObjectStart => {
                    stack.push(Building::Object(Vec::new(), String::new()));
                    continue;
                }
                Event::Name(k) => {
                    if let Some(Building::Object(_, name)) = stack.last_mut() {
                        *name = k.to_owned();
                    }
                    continue;
                }
                Event::Scalar(v) => v.scalar_clone(),
                Event::ArrayEnd | Event::ObjectEnd => match stack.pop() {
                    Some(Building::Array(items)) => Json::Array(items),
                    Some(Building::Object(members, _)) => Json::Object(JsonObject { members }),
                    None => unreachable!("balanced walk"),
                },
            };
            match stack.last_mut() {
                None => return done,
                Some(Building::Array(items)) => items.push(done),
                Some(Building::Object(members, name)) => members.push((std::mem::take(name), done)),
            }
        }
        unreachable!("a walk ends with its root")
    }
}

impl PartialEq for Json {
    /// Structural equality, object members compared in stored order, with no recursion.
    fn eq(&self, other: &Json) -> bool {
        let mut a = self.events();
        let mut b = other.events();
        loop {
            match (a.next(), b.next()) {
                (None, None) => return true,
                (Some(Event::Scalar(x)), Some(Event::Scalar(y))) => {
                    let same = match (x, y) {
                        (Json::Null, Json::Null) => true,
                        (Json::Bool(p), Json::Bool(q)) => p == q,
                        (Json::Number(p), Json::Number(q)) => p == q,
                        (Json::String(p), Json::String(q)) => p == q,
                        _ => false,
                    };
                    if !same {
                        return false;
                    }
                }
                (Some(x), Some(y)) if x == y => {}
                _ => return false,
            }
        }
    }
}

impl fmt::Debug for Json {
    /// The compact text, written without recursion.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_compact())
    }
}

impl Drop for Json {
    /// Drops the value with an explicit stack, so a deep value cannot overflow the call
    /// stack on drop.
    fn drop(&mut self) {
        fn take(v: &mut Json, stack: &mut Vec<Json>) {
            match v {
                Json::Array(a) => stack.append(a),
                Json::Object(o) => stack.extend(o.members.drain(..).map(|(_, v)| v)),
                _ => {}
            }
        }
        let mut stack: Vec<Json> = Vec::new();
        take(self, &mut stack);
        while let Some(mut v) = stack.pop() {
            take(&mut v, &mut stack);
        }
    }
}

impl From<JsonObject> for Json {
    fn from(o: JsonObject) -> Self {
        Json::Object(o)
    }
}

impl From<&str> for Json {
    fn from(s: &str) -> Self {
        Json::String(s.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(text: &str) -> JsonError {
        parse(text.as_bytes()).unwrap_err()
    }

    // A JSON `\u` escape of the four hex digits `h`, built at run time.
    fn u(h: &str) -> String {
        format!("{}u{h}", '\\')
    }

    #[test]
    fn keeps_number_spelling_and_member_order() {
        let v = parse(br#"{"b":1E-7,"a":0.10,"c":-0}"#).unwrap();
        assert_eq!(v.to_compact(), r#"{"b":1E-7,"a":0.10,"c":-0}"#);
    }

    #[test]
    fn refuses_what_ijson_forbids() {
        assert_eq!(
            err(r#"{"a":1,"a":2}"#),
            JsonError::DuplicateMember("a".into())
        );
        // A duplicate after escape decoding.
        let escaped_b = format!(r#"{{"a":{{"b":1,"{}":2}}}}"#, u("0062"));
        assert_eq!(err(&escaped_b), JsonError::DuplicateMember("b".into()));
        for lone in [
            format!(r#"["{}"]"#, u("d800")),
            format!(r#"["{}"]"#, u("dc00")),
            format!(r#"["{}{}"]"#, u("d800"), u("0041")),
            format!(r#"["{}{}"]"#, u("de00"), u("d83d")),
            format!(r#"["{}"]"#, u("fdd0")),
            format!(r#"["{}"]"#, u("ffff")),
            format!(r#"["{}{}"]"#, u("dbff"), u("dfff")),
            "[\"\u{fdd0}\"]".to_owned(),
            "[\"\u{10FFFF}\"]".to_owned(),
        ] {
            assert_eq!(err(&lone), JsonError::ForbiddenCodePoint, "{lone}");
        }
        assert_eq!(parse(b"[\"\xff\"]").unwrap_err(), JsonError::NotUtf8);
        // An encoded surrogate (ED A0 80) is not UTF-8.
        assert_eq!(
            parse(b"[\"\xed\xa0\x80\"]").unwrap_err(),
            JsonError::NotUtf8
        );
        let pair = format!(r#"["{}{}"]"#, u("d83d"), u("de00"));
        assert_eq!(
            parse(pair.as_bytes()).unwrap(),
            Json::Array(vec![Json::String("\u{1F600}".into())])
        );
        assert!(!is_ijson_string("a\u{fffe}"));
        assert!(is_ijson_string("a\u{1F600}"));
    }

    #[test]
    fn follows_the_rfc8259_grammar() {
        for bad in [
            "",
            "01",
            "1.",
            ".1",
            "1e",
            "+1",
            "[1,]",
            "{\"a\":1,}",
            "[\"\t\"]",
            "nul",
            "[1] x",
            "\u{feff}[]",
            "'a'",
            "[",
            "{",
            "{\"a\"",
            "{\"a\":",
            "[1 2]",
            "{\"a\":1 \"b\":2}",
            "{1:2}",
            "]",
            "[}",
        ] {
            assert!(parse(bad.as_bytes()).is_err(), "accepted {bad:?}");
        }
        for good in [
            "0",
            "-0",
            "1.5e+3",
            " [ ] ",
            "{}",
            "\"\\/\"",
            "[true,false,null]",
            " { \"a\" : [ 1 , { } , [ ] ] , \"b\" : { \"c\" : \"d\" } } ",
        ] {
            assert!(parse(good.as_bytes()).is_ok(), "refused {good:?}");
        }
        let v = parse(br#"{"a":[1,{"b":[]},[[2]]],"c":{"d":"e"}}"#).unwrap();
        assert_eq!(v.to_compact(), r#"{"a":[1,{"b":[]},[[2]]],"c":{"d":"e"}}"#);
    }

    #[test]
    fn any_depth_reads_copies_compares_writes_and_drops() {
        // Far deeper than any call stack holds frames for; this runs on a test thread's
        // default stack.
        let depth = 300_000;
        let text = format!("{}0{}", "[{\"a\":".repeat(depth), "}]".repeat(depth));
        let v = parse(text.as_bytes()).unwrap();
        assert_eq!(v.to_compact(), text);
        let copy = v.clone();
        assert!(copy == v);
        assert!(!v.contains_null());
        let other = parse(text.replacen("0}", "1}", 1).as_bytes()).unwrap();
        assert!(other != v);
        drop(copy);
        drop(other);
        drop(v);
    }

    #[test]
    fn plain_integers_only() {
        let n = |s: &str| JsonNumber { raw: s.into() };
        assert_eq!(n("300000").plain_integer_in(1, 86_400_000), Some(300_000));
        assert_eq!(n("0").plain_integer_in(0, 1), Some(0));
        for bad in ["300000.0", "3e5", "-1", "-0", "007", "86400001"] {
            assert_eq!(n(bad).plain_integer_in(1, 86_400_000), None, "{bad}");
        }
        assert_eq!(
            n("99999999999999999999999").plain_integer_in(0, u64::MAX),
            None
        );
    }

    #[test]
    fn compact_escapes_like_json_stringify() {
        let v = Json::String("a\"b\\c\n\u{1}\u{7f}\u{2028}é".into());
        let want = format!("\"a\\\"b\\\\c\\n{}\u{7f}\u{2028}é\"", u("0001"));
        assert_eq!(v.to_compact(), want);
    }
}
