// SPDX-License-Identifier: Apache-2.0

//! The RFC 8785 (JCS) canonical form and the signing input of `spec/security.md` §6.2.
//!
//! Canonicalization goes through `serde_jcs` 0.2.0, the crate decision C5 §3 and `PINS.md`
//! fix for it. `serde_jcs` sorts members by their UTF-16 code units and writes strings and
//! floating-point numbers as ECMAScript does (RFC 8785 §3.2.2-§3.2.3). This module hands it
//! every JSON number as the IEEE 754 double nearest to the number's decimal value
//! ([SEC-SIG-013]), never as an integer type, so `9007199254740993` canonicalizes as
//! `9007199254740992` and `1E-7`, `0.10` and `-0` as `1e-7`, `0.1` and `0`.
//!
//! Signing and verification themselves are task F3. This module only produces the octets
//! they work on.

use crate::json::{Json, JsonObject};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
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

/// Serializes a [`Json`] tree for `serde_jcs`: numbers as `f64`, everything else as is.
struct Jcs<'a>(&'a Json);

impl Serialize for Jcs<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Json::Null => s.serialize_unit(),
            Json::Bool(b) => s.serialize_bool(*b),
            Json::Number(n) => {
                let x = n.to_f64();
                // ECMAScript writes negative zero as "0" (RFC 8785 §3.2.2.3).
                s.serialize_f64(if x == 0.0 { 0.0 } else { x })
            }
            Json::String(v) => s.serialize_str(v),
            Json::Array(a) => {
                let mut seq = s.serialize_seq(Some(a.len()))?;
                for v in a {
                    seq.serialize_element(&Jcs(v))?;
                }
                seq.end()
            }
            Json::Object(o) => {
                let mut map = s.serialize_map(Some(o.len()))?;
                for (k, v) in o.iter() {
                    map.serialize_entry(k, &Jcs(v))?;
                }
                map.end()
            }
        }
    }
}

fn check_numbers(v: &Json) -> Result<(), CanonicalError> {
    match v {
        Json::Number(n) if !n.to_f64().is_finite() => {
            Err(CanonicalError::NumberOutOfRange(n.raw().to_owned()))
        }
        Json::Array(a) => a.iter().try_for_each(check_numbers),
        Json::Object(o) => o.iter().try_for_each(|(_, v)| check_numbers(v)),
        _ => Ok(()),
    }
}

/// The RFC 8785 canonical form of `v`, as UTF-8 text.
///
/// # Errors
///
/// [`CanonicalError::NumberOutOfRange`] when a number is beyond the double range.
pub fn canonical(v: &Json) -> Result<String, CanonicalError> {
    check_numbers(v)?;
    serde_jcs::to_string(&Jcs(v)).map_err(|e| CanonicalError::Serializer(e.to_string()))
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
    if let Some(Json::Object(mut sec)) = copy.remove("security") {
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
            c("[1E-7,0.10,1e21,-0,15e-1,9007199254740993,300000,1e-6,123456789012345680000]"),
            "[1e-7,0.1,1e+21,0,1.5,9007199254740992,300000,0.000001,123456789012345680000]"
        );
    }

    #[test]
    fn members_sorted_by_utf16_code_units() {
        // U+1F600 (surrogates D83D DE00) sorts before U+FB33 in UTF-16, after it in UTF-8.
        assert_eq!(
            c("{\"\u{fb33}\":1,\"\u{1f600}\":2,\"b\":3,\"a\":4}"),
            "{\"a\":4,\"b\":3,\"\u{1f600}\":2,\"\u{fb33}\":1}"
        );
    }

    #[test]
    fn out_of_range_number_has_no_form() {
        assert!(matches!(
            canonical(&parse(b"[1e400]").unwrap()),
            Err(CanonicalError::NumberOutOfRange(_))
        ));
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
