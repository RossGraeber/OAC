// SPDX-License-Identifier: Apache-2.0

//! The RFC 8785 (JCS) canonical form and the signing input of `spec/security.md` §6.2.
//!
//! RFC 8785 fixes three things: member order (by name, as arrays of UTF-16 code units,
//! §3.2.3), string serialization (§3.2.2.2) and number serialization, as ECMAScript writes
//! an IEEE 754 double (§3.2.2.3). This module walks the tree with an explicit stack
//! ([`Json::events`], members sorted as §3.2.3 requires), so a value of any depth
//! canonicalizes without recursion. Each member name, string and number is written by
//! `serde_jcs` 0.2.0, the crate decision C5 §3 and `PINS.md` fix for canonicalization.
//!
//! Every JSON number is handed to `serde_jcs` as the IEEE 754 double nearest to its decimal
//! value ([SEC-SIG-013]), never as an integer type, so `9007199254740993` canonicalizes as
//! `9007199254740992`, and `1E-7`, `0.10` and `-0` as `1e-7`, `0.1` and `0`.
//!
//! Signing and verification themselves are task F3. This module only produces the octets
//! they work on.

use crate::json::{Json, JsonObject, write_events};
use std::fmt;

/// The domain strings of `spec/security.md` §6.2. The pairing domain, which that section
/// reserves, has no variant: nothing in this revision is signed under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigningDomain {
    /// `oac-envelope-v1`, an envelope ([SEC-SIG-010]).
    Envelope,
    /// `oac-registration-v1`, a registration record (`spec/security.md` §5.4).
    Registration,
    /// `oac-receipt-v1`, an authenticated receipt (`spec/security.md` §10).
    Receipt,
    /// `oac-presence-v1`, an authenticated presence record (`spec/security.md` §11).
    Presence,
}

impl SigningDomain {
    /// The domain string, in ASCII.
    pub fn as_str(self) -> &'static str {
        match self {
            SigningDomain::Envelope => "oac-envelope-v1",
            SigningDomain::Registration => "oac-registration-v1",
            SigningDomain::Receipt => "oac-receipt-v1",
            SigningDomain::Presence => "oac-presence-v1",
        }
    }
}

/// Why a value has no canonical form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalError {
    /// A number is beyond the IEEE 754 double range; RFC 8785 §3.2.2.3 has no form for it.
    NumberOutOfRange(String),
    /// The canonicalizer failed for another reason.
    Serializer(String),
}

impl fmt::Display for CanonicalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CanonicalError::NumberOutOfRange(n) => {
                write!(f, "number {n} is beyond the double range")
            }
            CanonicalError::Serializer(e) => write!(f, "canonicalization failed: {e}"),
        }
    }
}

impl std::error::Error for CanonicalError {}

fn jcs<E: fmt::Display>(out: &mut String, r: Result<String, E>) -> Result<(), CanonicalError> {
    out.push_str(&r.map_err(|e| CanonicalError::Serializer(e.to_string()))?);
    Ok(())
}

fn scalar(v: &Json, out: &mut String) -> Result<(), CanonicalError> {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::String(s) => jcs(out, serde_jcs::to_string(s.as_str()))?,
        Json::Number(n) => {
            let x = n.to_f64();
            if !x.is_finite() {
                return Err(CanonicalError::NumberOutOfRange(n.raw().to_owned()));
            }
            // ECMAScript writes negative zero as "0" (RFC 8785 §3.2.2.3).
            jcs(out, serde_jcs::to_string(&if x == 0.0 { 0.0 } else { x }))?;
        }
        Json::Array(_) | Json::Object(_) => unreachable!("the walk yields containers as events"),
    }
    Ok(())
}

/// The RFC 8785 canonical form of `v`, as UTF-8 text.
///
/// # Errors
///
/// [`CanonicalError::NumberOutOfRange`] when a number is beyond the double range.
pub fn canonical(v: &Json) -> Result<String, CanonicalError> {
    write_events(v.events_sorted(), scalar, |k, out| {
        jcs(out, serde_jcs::to_string(k))
    })
}

/// The canonical text that `spec/security.md` §6.2 signs: `obj` with the member
/// `security.signature` removed, every other member kept, unrecognized ones included
/// ([SEC-SIG-011]).
///
/// # Errors
///
/// As [`canonical`].
pub fn signed_text(obj: &JsonObject) -> Result<String, CanonicalError> {
    let mut copy = obj.clone();
    if let Some(mut sec) = copy.remove("security").and_then(Json::into_object) {
        sec.remove("signature");
        copy.insert("security", Json::Object(sec));
    }
    canonical(&Json::Object(copy))
}

/// The signing input of `spec/security.md` §6.2: the domain string in ASCII, one zero
/// octet, then [`signed_text`] in UTF-8.
///
/// # Errors
///
/// As [`canonical`].
pub fn signing_input(domain: SigningDomain, obj: &JsonObject) -> Result<Vec<u8>, CanonicalError> {
    let text = signed_text(obj)?;
    let mut out = Vec::with_capacity(domain.as_str().len() + 1 + text.len());
    out.extend_from_slice(domain.as_str().as_bytes());
    out.push(0);
    out.extend_from_slice(text.as_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    fn c(text: &str) -> String {
        canonical(&parse(text.as_bytes()).unwrap()).unwrap()
    }

    #[test]
    fn numbers_as_nearest_doubles() {
        assert_eq!(
            c(
                "[1E-7,0.10,1e21,-0,15e-1,9007199254740993,300000,1e-6,123456789012345680000,5e-324]"
            ),
            "[1e-7,0.1,1e+21,0,1.5,9007199254740992,300000,0.000001,123456789012345680000,5e-324]"
        );
    }

    #[test]
    fn members_sorted_by_utf16_code_units() {
        // U+1F600 (surrogates D83D DE00) sorts before U+FB33 in UTF-16, after it in UTF-8.
        assert_eq!(
            c("{\"\u{fb33}\":1,\"\u{1f600}\":2,\"b\":3,\"a\":{\"y\":[],\"x\":{}}}"),
            "{\"a\":{\"x\":{},\"y\":[]},\"b\":3,\"\u{1f600}\":2,\"\u{fb33}\":1}"
        );
    }

    #[test]
    fn strings_escaped_as_rfc8785() {
        let b = '\\';
        let input =
            format!("[\"{b}u0001{b}u001F{b}\"{b}{b}{b}/{b}b{b}f{b}n{b}r{b}t\u{7f}\u{2028}é\"]");
        let want = format!("[\"{b}u0001{b}u001f{b}\"{b}{b}/{b}b{b}f{b}n{b}r{b}t\u{7f}\u{2028}é\"]");
        assert_eq!(c(&input), want);
    }

    #[test]
    fn out_of_range_number_has_no_form() {
        assert!(matches!(
            canonical(&parse(b"[1e400]").unwrap()),
            Err(CanonicalError::NumberOutOfRange(_))
        ));
    }

    #[test]
    fn any_depth_canonicalizes() {
        let depth = 200_000;
        let text = format!(
            "{}0{}",
            "[{\"b\":1,\"a\":".repeat(depth),
            "}]".repeat(depth)
        );
        let want = format!(
            "{}0{}",
            "[{\"a\":".repeat(depth),
            ",\"b\":1}]".repeat(depth)
        );
        assert_eq!(c(&text), want);
    }

    #[test]
    fn signing_input_drops_only_the_signature() {
        let obj = parse(br#"{"x":1,"security":{"signature":"s","nonce":"n"}}"#).unwrap();
        let input = signing_input(SigningDomain::Envelope, obj.as_object().unwrap()).unwrap();
        assert_eq!(
            input,
            b"oac-envelope-v1\0{\"security\":{\"nonce\":\"n\"},\"x\":1}"
        );
    }
}
