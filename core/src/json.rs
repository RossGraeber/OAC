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
//! > **Reference implementation note:** nesting deeper than [`MAX_DEPTH`] is refused as
//! > [`JsonError::TooDeep`]. That bounds the reader's stack; no value this specification
//! > defines comes near it, but an unrecognized member could, and such an envelope is
//! > rejected as malformed.

use std::collections::HashSet;
use std::fmt;

/// The deepest nesting of arrays and objects [`parse`] accepts.
pub const MAX_DEPTH: usize = 128;

/// A JSON value read under the I-JSON rules.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// Nesting deeper than [`MAX_DEPTH`].
    TooDeep,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonError::NotUtf8 => f.write_str("not UTF-8"),
            JsonError::Syntax { offset, reason } => write!(f, "{reason} at offset {offset}"),
            JsonError::ForbiddenCodePoint => f.write_str("lone surrogate or noncharacter"),
            JsonError::DuplicateMember(name) => write!(f, "duplicate member name {name:?}"),
            JsonError::TooDeep => write!(f, "nesting deeper than {MAX_DEPTH}"),
        }
    }
}

impl std::error::Error for JsonError {}

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
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.err("trailing characters"));
    }
    Ok(v)
}

/// True when `c` is a Unicode noncharacter (U+FDD0 to U+FDEF, or a code point ending in
/// FFFE or FFFF), which RFC 7493 §2.1 forbids.
fn is_noncharacter(c: char) -> bool {
    let u = c as u32;
    (0xFDD0..=0xFDEF).contains(&u) || (u & 0xFFFE) == 0xFFFE
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

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.ws();
        match self.s.get(self.i) {
            None => Err(self.err("unexpected end")),
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.err("unexpected character")),
        }
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
        match self.s.get(self.i) {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(self.err("bad number")),
        }
        if self.s.get(self.i) == Some(&b'.') {
            self.i += 1;
            if self.digits() == 0 {
                return Err(self.err("bad number fraction"));
            }
        }
        if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
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
            .ok_or_else(|| self.err("bad \\u escape"))?;
        let mut v = 0u32;
        for &b in h {
            let d = (b as char)
                .to_digit(16)
                .ok_or_else(|| self.err("bad \\u escape"))?;
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
            match self.s.get(self.i) {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => {
                    self.i += 1;
                    break;
                }
                Some(b'\\') => {
                    self.i += 1;
                    let e = *self.s.get(self.i).ok_or_else(|| self.err("bad escape"))?;
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
                                if self.s.get(self.i..self.i + 2) != Some(b"\\u") {
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
        if out.chars().any(is_noncharacter) {
            return Err(JsonError::ForbiddenCodePoint);
        }
        Ok(out)
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > MAX_DEPTH {
            return Err(JsonError::TooDeep);
        }
        self.i += 1;
        let mut items = Vec::new();
        self.ws();
        if self.s.get(self.i) == Some(&b']') {
            self.i += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value(depth)?);
            self.ws();
            match self.s.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.err("expected , or ]")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > MAX_DEPTH {
            return Err(JsonError::TooDeep);
        }
        self.i += 1;
        let mut members = Vec::new();
        let mut seen = HashSet::new();
        self.ws();
        if self.s.get(self.i) == Some(&b'}') {
            self.i += 1;
            return Ok(Json::Object(JsonObject { members }));
        }
        loop {
            self.ws();
            if self.s.get(self.i) != Some(&b'"') {
                return Err(self.err("expected member name"));
            }
            let name = self.string()?;
            self.ws();
            if self.s.get(self.i) != Some(&b':') {
                return Err(self.err("expected :"));
            }
            self.i += 1;
            let v = self.value(depth)?;
            if !seen.insert(name.clone()) {
                return Err(JsonError::DuplicateMember(name));
            }
            members.push((name, v));
            self.ws();
            match self.s.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Json::Object(JsonObject { members }));
                }
                _ => return Err(self.err("expected , or }")),
            }
        }
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

    /// True when a JSON `null` appears anywhere in the value, as a member value or an
    /// array element ([SC-ENV-003], [SC-RCP-020], [SC-DLV-021]).
    pub fn contains_null(&self) -> bool {
        match self {
            Json::Null => true,
            Json::Array(a) => a.iter().any(Json::contains_null),
            Json::Object(o) => o.members.iter().any(|(_, v)| v.contains_null()),
            _ => false,
        }
    }

    /// The value as compact JSON text: no insignificant whitespace, object members in
    /// stored order, numbers as spelled, and strings escaped as ECMAScript
    /// `JSON.stringify` escapes them (`"`, `\`, and control characters only).
    pub fn to_compact(&self) -> String {
        let mut out = String::new();
        self.write_compact(&mut out);
        out
    }

    fn write_compact(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::Number(n) => out.push_str(&n.raw),
            Json::String(s) => write_string(s, out),
            Json::Array(a) => {
                out.push('[');
                for (k, v) in a.iter().enumerate() {
                    if k > 0 {
                        out.push(',');
                    }
                    v.write_compact(out);
                }
                out.push(']');
            }
            Json::Object(o) => {
                out.push('{');
                for (k, (name, v)) in o.members.iter().enumerate() {
                    if k > 0 {
                        out.push(',');
                    }
                    write_string(name, out);
                    out.push(':');
                    v.write_compact(out);
                }
                out.push('}');
            }
        }
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
        assert_eq!(
            err(r#"{"a":{"b":1,"b":2}}"#),
            JsonError::DuplicateMember("b".into())
        );
        assert_eq!(err(r#"["\ud800"]"#), JsonError::ForbiddenCodePoint);
        assert_eq!(err(r#"["\udc00"]"#), JsonError::ForbiddenCodePoint);
        assert_eq!(err(r#"["\ud800A"]"#), JsonError::ForbiddenCodePoint);
        assert_eq!(err(r#"["﷐"]"#), JsonError::ForbiddenCodePoint);
        assert_eq!(err("[\"\u{10FFFF}\"]"), JsonError::ForbiddenCodePoint);
        assert_eq!(parse(b"[\"\xff\"]").unwrap_err(), JsonError::NotUtf8);
        // An encoded surrogate (ED A0 80) is not UTF-8.
        assert_eq!(
            parse(b"[\"\xed\xa0\x80\"]").unwrap_err(),
            JsonError::NotUtf8
        );
        assert_eq!(
            parse(r#"["😀"]"#.as_bytes()).unwrap(),
            Json::Array(vec![Json::String("\u{1F600}".into())])
        );
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
        ] {
            assert!(parse(bad.as_bytes()).is_err(), "accepted {bad:?}");
        }
        for good in [
            "0",
            "-0",
            "1.5e+3",
            " [ ] ",
            "{}",
            "\"\\u00e9\\/\"",
            "[true,false,null]",
        ] {
            assert!(parse(good.as_bytes()).is_ok(), "refused {good:?}");
        }
    }

    #[test]
    fn bounds_nesting() {
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(parse(ok.as_bytes()).is_ok());
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        assert_eq!(parse(deep.as_bytes()).unwrap_err(), JsonError::TooDeep);
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
        assert_eq!(v.to_compact(), "\"a\\\"b\\\\c\\n\\u0001\u{7f}\u{2028}é\"");
    }
}
