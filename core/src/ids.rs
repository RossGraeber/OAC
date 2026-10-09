// SPDX-License-Identifier: Apache-2.0

//! Identifier and scalar forms: identifier tokens (`spec/session-channels.md` §4.3),
//! session ids (§6.1), versions (§4.4.1), timestamps (§4.4.6), content part types
//! (§4.5.2), extension identifiers (§5.1) and key ids (`spec/security.md` §5.2).
//!
//! Each type is a validated newtype. A value exists only if it matched its form as a whole
//! value, with no trimming, case-folding or other normalization ([SC-ENV-011],
//! [SC-ID-002], [SEC-KEY-011]); equality is exact string equality.

use std::fmt;

/// The extension identifier of major version 0 (`spec/session-channels.md` §5.1).
pub const EXTENSION_ID_V0: &str = "io.github.rossgraeber/oac-session-channels";

/// The revision of `spec/session-channels.md` this crate implements.
pub const IMPLEMENTED_VERSION: Version = Version { major: 0, minor: 1 };

macro_rules! string_newtype {
    ($t:ident) => {
        impl $t {
            /// The value as a string.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl fmt::Display for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl AsRef<str> for $t {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

/// An identifier token: `[A-Za-z0-9._:-]{1,128}` as a whole value
/// (`spec/session-channels.md` §4.3; [SC-ENV-010], [SC-ENV-011]).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Token(String);
string_newtype!(Token);

/// True when `s` is an identifier token. Every byte is tested, so no line anchor can
/// stop the match early ([SC-ENV-011]).
pub fn is_token(s: &str) -> bool {
    (1..=128).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

impl Token {
    /// `s` as a token, if it is one.
    pub fn parse(s: &str) -> Option<Token> {
        is_token(s).then(|| Token(s.to_owned()))
    }
}

/// A session id: `[0-7][0-9a-hjkmnp-tv-z]{25}` as a whole value, the 26-character
/// lower-case Crockford Base32 form of 128 random bits (`spec/session-channels.md` §6.1;
/// [SC-ID-001], [SC-ID-002]). A `SessionIdentity` of `spec/interfaces.md` §4.2 is exactly
/// this and nothing else ([IFC-TYP-010]).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(String);
string_newtype!(SessionId);

/// `SessionIdentity` of `spec/interfaces.md` §4.2.
pub type SessionIdentity = SessionId;

const CROCKFORD: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// True when `s` is a session id.
pub fn is_session_id(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 26 && (b'0'..=b'7').contains(&b[0]) && b[1..].iter().all(|c| CROCKFORD.contains(c))
}

impl SessionId {
    /// `s` as a session id, if it is one.
    pub fn parse(s: &str) -> Option<SessionId> {
        is_session_id(s).then(|| SessionId(s.to_owned()))
    }

    /// The session id that encodes `octets`, most significant bit first, after two leading
    /// zero bits (`spec/session-channels.md` §6.1).
    ///
    /// [SC-ID-003] requires the octets to be 128 bits of output of a cryptographically
    /// secure random number generator, and [SC-ID-004] forbids deriving them from any
    /// input. The caller supplies them; this crate holds no random number generator.
    pub fn from_random_octets(octets: [u8; 16]) -> SessionId {
        let v = u128::from_be_bytes(octets);
        let s = (0..26)
            .rev()
            .map(|k| CROCKFORD[((v >> (5 * k)) & 31) as usize] as char)
            .collect();
        SessionId(s)
    }
}

impl From<SessionId> for Token {
    /// Every session id is an identifier token (§6.1).
    fn from(s: SessionId) -> Token {
        Token(s.0)
    }
}

/// A key id: the 64 lower-case hexadecimal digits of a SHA-256 digest
/// (`spec/security.md` §5.2). Compared as an exact string ([SEC-KEY-011]).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyId(String);
string_newtype!(KeyId);

impl KeyId {
    /// `s` as a key id, if it is 64 lower-case hexadecimal digits.
    pub fn parse(s: &str) -> Option<KeyId> {
        (s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .then(|| KeyId(s.to_owned()))
    }
}

/// A `<major>.<minor>` version (`spec/session-channels.md` §4.4.1, §5.1; [SC-ENV-020]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Version {
    /// The major version: a wire-compatible generation.
    pub major: u16,
    /// The minor version: compatible revisions within the major version.
    pub minor: u16,
}

fn version_part(s: &str) -> Option<u16> {
    let b = s.as_bytes();
    let ok = (1..=4).contains(&b.len())
        && b.iter().all(u8::is_ascii_digit)
        && (b.len() == 1 || b[0] != b'0');
    if ok { s.parse().ok() } else { None }
}

impl Version {
    /// `s` as a version, if it matches `(0|[1-9][0-9]{0,3})\.(0|[1-9][0-9]{0,3})` as a
    /// whole value ([SC-ENV-020]).
    pub fn parse(s: &str) -> Option<Version> {
        let (a, b) = s.split_once('.')?;
        Some(Version {
            major: version_part(a)?,
            minor: version_part(b)?,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// A timestamp in the form of `spec/session-channels.md` §4.4.6: an RFC 3339 `date-time`
/// with upper-case `T` and `Z`, one to nine fraction digits, seconds `00` to `59`, and a
/// real calendar date ([SC-ENV-041]). It keeps its spelling, and the instant it names in
/// nanoseconds since 1970-01-01T00:00:00Z.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Timestamp {
    text: String,
    unix_nanos: i128,
}

fn digits(b: &[u8]) -> Option<i64> {
    if b.is_empty() || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(b.iter().fold(0i64, |a, d| a * 10 + i64::from(d - b'0')))
}

// Days since 1970-01-01 of a proleptic Gregorian date (H. Hinnant, "days_from_civil").
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ][(m - 1) as usize]
}

const NANOS_PER_SEC: i128 = 1_000_000_000;

impl Timestamp {
    /// `s` as a timestamp, if it has the §4.4.6 form.
    pub fn parse(s: &str) -> Option<Timestamp> {
        let b = s.as_bytes();
        // YYYY-MM-DDTHH:MM:SS[.f{1,9}]Z
        if b.len() < 20
            || b[4] != b'-'
            || b[7] != b'-'
            || b[10] != b'T'
            || b[13] != b':'
            || b[16] != b':'
            || *b.last()? != b'Z'
        {
            return None;
        }
        let (y, mo, d) = (digits(&b[0..4])?, digits(&b[5..7])?, digits(&b[8..10])?);
        let (h, mi, sec) = (
            digits(&b[11..13])?,
            digits(&b[14..16])?,
            digits(&b[17..19])?,
        );
        let frac = &b[19..b.len() - 1];
        let frac_nanos = match frac {
            [] => 0,
            [b'.', f @ ..] if (1..=9).contains(&f.len()) => {
                digits(f)? * 10i64.pow(9 - f.len() as u32)
            }
            _ => return None,
        };
        if !(1..=12).contains(&mo)
            || h > 23
            || mi > 59
            || sec > 59
            || d < 1
            || d > days_in_month(y, mo)
        {
            return None;
        }
        let secs =
            i128::from(days_from_civil(y, mo, d)) * 86_400 + i128::from(h * 3600 + mi * 60 + sec);
        Some(Timestamp {
            text: s.to_owned(),
            unix_nanos: secs * NANOS_PER_SEC + i128::from(frac_nanos),
        })
    }

    /// The timestamp for `unix_millis` milliseconds since the epoch, written with three
    /// fraction digits, for example `2026-10-03T12:00:00.000Z`. `None` outside the years
    /// 0000 to 9999, which the form cannot write.
    pub fn from_unix_millis(unix_millis: i64) -> Option<Timestamp> {
        let secs = unix_millis.div_euclid(1000);
        let ms = unix_millis.rem_euclid(1000);
        let (days, sod) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
        let (y, m, d) = civil_from_days(days);
        if !(0..=9999).contains(&y) {
            return None;
        }
        let text = format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{ms:03}Z",
            sod / 3600,
            sod / 60 % 60,
            sod % 60
        );
        Timestamp::parse(&text)
    }

    /// The timestamp for `unix_nanos` nanoseconds since the epoch, written with nine fraction
    /// digits, for example `2026-10-03T12:00:00.000000001Z`, so no precision is lost
    /// ([SEC-RPL-003]). `None` outside the years 0000 to 9999.
    pub fn from_unix_nanos(unix_nanos: i128) -> Option<Timestamp> {
        let secs = i64::try_from(unix_nanos.div_euclid(NANOS_PER_SEC)).ok()?;
        let ns = unix_nanos.rem_euclid(NANOS_PER_SEC);
        let (days, sod) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
        let (y, m, d) = civil_from_days(days);
        if !(0..=9999).contains(&y) {
            return None;
        }
        let text = format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{ns:09}Z",
            sod / 3600,
            sod / 60 % 60,
            sod % 60
        );
        Timestamp::parse(&text)
    }

    /// The timestamp as written.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The instant, in nanoseconds since 1970-01-01T00:00:00Z.
    pub fn unix_nanos(&self) -> i128 {
        self.unix_nanos
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// True when `t` is a core content part type: `[a-z][a-z0-9-]{0,31}` (§4.5.2).
pub fn is_core_type(t: &str) -> bool {
    let b = t.as_bytes();
    (1..=32).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn is_dns_label(l: &[u8]) -> bool {
    !l.is_empty()
        && l[0].is_ascii_alphanumeric()
        && l[l.len() - 1].is_ascii_alphanumeric()
        && l.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-')
}

fn is_reverse_dns(p: &str) -> bool {
    let labels: Vec<&str> = p.split('.').collect();
    labels.len() >= 2 && labels.iter().all(|l| is_dns_label(l.as_bytes()))
}

/// True when `t` is an extension content part type: `{reverse-dns-prefix}/{name}`, with
/// `name` matching `[a-z][a-z0-9-]{0,63}` (§4.5.2).
pub fn is_extension_type(t: &str) -> bool {
    let Some((prefix, name)) = t.split_once('/') else {
        return false;
    };
    let n = name.as_bytes();
    is_reverse_dns(prefix)
        && (1..=64).contains(&n.len())
        && n[0].is_ascii_lowercase()
        && n.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_whole_value() {
        assert!(is_token(&"a".repeat(128)));
        assert!(!is_token(&"a".repeat(129)));
        assert!(is_token("Az09._:-"));
        for bad in ["", "a\n", " a", "a b", "é", "a/b"] {
            assert!(!is_token(bad), "{bad:?}");
        }
    }

    #[test]
    fn session_ids() {
        assert!(is_session_id("01harn7x9k2m4p6q8r0s2t4v6w"));
        for bad in [
            "81harn7x9k2m4p6q8r0s2t4v6w",
            "01HARN7X9K2M4P6Q8R0S2T4V6W",
            "01harn7x9k2m4p6q8r0s2t4v6u",
            "01harn7x9k2m4p6q8r0s2t4v6",
            "01harn7x9k2m4p6q8r0s2t4v6w\n",
        ] {
            assert!(!is_session_id(bad), "{bad:?}");
        }
        assert_eq!(
            SessionId::from_random_octets([0; 16]).as_str(),
            "0".repeat(26)
        );
        assert_eq!(
            SessionId::from_random_octets([0xff; 16]).as_str(),
            format!("7{}", "z".repeat(25))
        );
        let id = SessionId::from_random_octets(
            *b"\x01\x23\x45\x67\x89\xab\xcd\xef\xfe\xdc\xba\x98\x76\x54\x32\x10",
        );
        assert!(is_session_id(id.as_str()));
    }

    #[test]
    fn versions() {
        assert_eq!(Version::parse("0.1"), Some(Version { major: 0, minor: 1 }));
        assert_eq!(
            Version::parse("9999.10"),
            Some(Version {
                major: 9999,
                minor: 10
            })
        );
        for bad in [
            "01.1", "0.01", "0.1.0", "10000.0", "0", ".1", "0.", "+0.1", "0.1\n",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn timestamps() {
        let t = Timestamp::parse("2026-10-03T12:00:00.000Z").unwrap();
        assert_eq!(t.unix_nanos(), 1_791_028_800 * NANOS_PER_SEC);
        assert_eq!(
            Timestamp::parse("2024-02-29T23:59:59.123456789Z")
                .unwrap()
                .unix_nanos()
                % NANOS_PER_SEC,
            123_456_789
        );
        for bad in [
            "2026-10-03t12:00:00Z",
            "2026-10-03T12:00:00z",
            "2026-10-03T12:00:00+00:00",
            "2026-10-03",
            "2026-02-30T00:00:00Z",
            "2025-02-29T00:00:00Z",
            "2026-10-03T12:00:60Z",
            "2026-10-03T24:00:00Z",
            "2026-10-03T12:00:00.Z",
            "2026-10-03T12:00:00.1234567890Z",
        ] {
            assert_eq!(Timestamp::parse(bad), None, "{bad:?}");
        }
        assert_eq!(
            Timestamp::from_unix_millis(1_791_028_800_250)
                .unwrap()
                .as_str(),
            "2026-10-03T12:00:00.250Z"
        );
        assert_eq!(
            Timestamp::from_unix_millis(-1).unwrap().as_str(),
            "1969-12-31T23:59:59.999Z"
        );
    }

    #[test]
    fn part_types() {
        assert!(is_core_type("text"));
        assert!(!is_core_type("Text"));
        assert!(is_extension_type("com.example/rich-text"));
        for bad in [
            "example/x",
            "com.example/X",
            "com..example/x",
            "com.example-/x",
            "com.example/",
            "text",
        ] {
            assert!(!is_extension_type(bad), "{bad:?}");
        }
    }

    #[test]
    fn key_ids() {
        assert!(KeyId::parse(&"a".repeat(64)).is_some());
        assert!(KeyId::parse(&"A".repeat(64)).is_none());
        assert!(KeyId::parse(&"a".repeat(63)).is_none());
    }
}
