// SPDX-License-Identifier: Apache-2.0

//! The registration record (`spec/security.md` §5.4): it binds one session id to the device
//! key, so a session's authority comes from the device key and the session holds no key of
//! its own ([SEC-KEY-005]; `spec/session-channels.md` [SC-ID-009]).
//!
//! The record holds `native_id` and `working_directory`, which nothing on the wire may carry,
//! so it is never transmitted ([SEC-KEY-042]): this module gives no way to serialize one,
//! and [`RegistrationRecord`]'s `Debug` leaves both values out. It lives in the memory of the
//! process that holds the device key and ends with its session
//! (`docs/planning/decisions/C4-session-identity.md` §5).

use crate::canonical::SigningDomain;
use crate::ids::{SessionId, Timestamp, Token};
use crate::json::{self, Json, JsonObject};
use crate::keys::DeviceIdentity;
use crate::signing::{VerifyError, verify_signed};
use crate::trust::{TrustedKey, TrustedKeySet};
use std::fmt;

/// The members of a registration record ([SEC-KEY-040]).
pub const REGISTRATION_MEMBERS: [&str; 6] = [
    "session_id",
    "harness_label",
    "native_id",
    "working_directory",
    "registered_at",
    "security",
];

/// A registration record, signed or read back. Fields are private; a record exists only if
/// it has exactly the members of [`REGISTRATION_MEMBERS`] ([SEC-KEY-040]), each of its
/// form, and a `security` object of `principal`, `key_id` and `signature`.
#[derive(Clone, PartialEq, Eq)]
pub struct RegistrationRecord {
    session_id: SessionId,
    harness_label: Token,
    registered_at: Timestamp,
    wire: JsonObject,
}

impl fmt::Debug for RegistrationRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegistrationRecord")
            .field("session_id", &self.session_id)
            .field("harness_label", &self.harness_label)
            .field("registered_at", &self.registered_at)
            .finish_non_exhaustive()
    }
}

impl RegistrationRecord {
    /// A record read from `v`, not yet verified. `None` unless `v` is an object with exactly
    /// the members of [`REGISTRATION_MEMBERS`] ([SEC-KEY-040]), `session_id` a session id,
    /// `harness_label` an identifier token, `native_id` and `working_directory` strings,
    /// `registered_at` a timestamp, and `security` an object of exactly `principal`,
    /// `key_id` and `signature`, all strings.
    pub fn from_json(v: &Json) -> Option<RegistrationRecord> {
        let o = v.as_object()?;
        if o.len() != REGISTRATION_MEMBERS.len()
            || REGISTRATION_MEMBERS.iter().any(|m| !o.contains(m))
        {
            return None;
        }
        let s = |m| o.get(m).and_then(Json::as_str);
        let session_id = SessionId::parse(s("session_id")?)?;
        let harness_label = Token::parse(s("harness_label")?)?;
        s("native_id")?;
        s("working_directory")?;
        let registered_at = Timestamp::parse(s("registered_at")?)?;
        let sec = o.get("security")?.as_object()?;
        let names = ["principal", "key_id", "signature"];
        if sec.len() != names.len()
            || names
                .iter()
                .any(|m| sec.get(m).and_then(Json::as_str).is_none())
        {
            return None;
        }
        Some(RegistrationRecord {
            session_id,
            harness_label,
            registered_at,
            wire: o.clone(),
        })
    }

    /// The session id the record binds.
    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// The harness label, for display only (`spec/session-channels.md` [SC-ID-043]).
    pub fn harness_label(&self) -> &Token {
        &self.harness_label
    }

    /// The registration time.
    pub fn registered_at(&self) -> &Timestamp {
        &self.registered_at
    }

    /// The harness-native id. Local: never transmitted ([SEC-KEY-042]).
    pub fn native_id(&self) -> &str {
        self.wire
            .get("native_id")
            .and_then(Json::as_str)
            .unwrap_or("")
    }

    /// The working-directory scope as recorded. Local: never transmitted ([SEC-KEY-042]).
    pub fn working_directory(&self) -> &str {
        self.wire
            .get("working_directory")
            .and_then(Json::as_str)
            .unwrap_or("")
    }

    /// The trusted-key-set entry whose key signed the record over the signing input of §6.2
    /// under `oac-registration-v1` ([SEC-KEY-041]).
    ///
    /// # Errors
    ///
    /// [`VerifyError`] when no trusted key names the signer or the signature does not
    /// verify.
    pub fn verify<'k>(&self, keys: &'k TrustedKeySet) -> Result<&'k TrustedKey, VerifyError> {
        verify_signed(keys, SigningDomain::Registration, &self.wire)
    }

    /// Whether this device may use the record's binding: for attributing a send request,
    /// issuing a presence record or answering a binding-table lookup for its own session
    /// ([SEC-KEY-043]). True only when the record's signature verifies under `own`'s device
    /// key.
    pub fn binding_usable(&self, own: &DeviceIdentity) -> bool {
        let own_only = TrustedKeySet::new(own);
        self.verify(&own_only)
            .is_ok_and(|e| e.key_id() == own.key_id())
    }
}

impl DeviceIdentity {
    /// Registers a session: the record binding `session_id` to this device's key, signed
    /// with it under `oac-registration-v1` ([SEC-KEY-041]). `None` when `native_id` or
    /// `working_directory` is not a string I-JSON allows ([SC-ENV-002]).
    pub fn register(
        &self,
        session_id: SessionId,
        harness_label: Token,
        native_id: &str,
        working_directory: &str,
        registered_at: Timestamp,
    ) -> Option<RegistrationRecord> {
        if !json::is_ijson_string(native_id) || !json::is_ijson_string(working_directory) {
            return None;
        }
        let mut o = JsonObject::new();
        o.insert("session_id", session_id.as_str().into());
        o.insert("harness_label", harness_label.as_str().into());
        o.insert("native_id", native_id.into());
        o.insert("working_directory", working_directory.into());
        o.insert("registered_at", registered_at.as_str().into());
        let mut sec = JsonObject::new();
        sec.insert("principal", self.principal().as_str().into());
        sec.insert("key_id", self.key_id().as_str().into());
        o.insert("security", Json::Object(sec.clone()));
        let signature = self
            .sign_object(SigningDomain::Registration, &o)
            .expect("a registration record holds strings only, so it canonicalizes");
        sec.insert("signature", signature.as_str().into());
        o.insert("security", Json::Object(sec));
        Some(RegistrationRecord {
            session_id,
            harness_label,
            registered_at,
            wire: o,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::DeviceKey;

    fn identity() -> DeviceIdentity {
        DeviceIdentity::new(DeviceKey::generate(), Token::parse("principal-a").unwrap())
    }

    fn record(id: &DeviceIdentity) -> RegistrationRecord {
        id.register(
            SessionId::parse("7gq3m8z2c5k9t1w4x6b0n2r8vd").unwrap(),
            Token::parse("harness-a").unwrap(),
            "native-thread-0001",
            "/home/test/repo-a",
            Timestamp::parse("2026-10-03T11:59:00.000Z").unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn a_registered_session_is_bound_to_the_device_key() {
        let me = identity();
        let rec = record(&me);
        assert!(rec.binding_usable(&me));
        assert_eq!(
            rec.verify(&TrustedKeySet::new(&me)).unwrap().key_id(),
            me.key_id()
        );
        assert_eq!(rec.native_id(), "native-thread-0001");
        assert_eq!(rec.working_directory(), "/home/test/repo-a");
        // Read back from its JSON form, it is the same record.
        let back = RegistrationRecord::from_json(&Json::Object(rec.wire.clone())).unwrap();
        assert_eq!(back, rec);
        // [SEC-KEY-043]: another device's record is not usable here, even if its key is
        // trusted.
        let other = identity();
        assert!(!record(&other).binding_usable(&me));
        // [SEC-KEY-042]: Debug shows neither local value.
        let shown = format!("{rec:?}");
        assert!(!shown.contains("native-thread") && !shown.contains("/home/test"));
    }

    #[test]
    fn a_tampered_record_is_not_usable() {
        let me = identity();
        let mut wire = record(&me).wire;
        wire.insert("working_directory", "/home/test/repo-b".into());
        let tampered = RegistrationRecord::from_json(&Json::Object(wire)).unwrap();
        assert!(!tampered.binding_usable(&me));
        assert_eq!(
            tampered.verify(&TrustedKeySet::new(&me)),
            Err(VerifyError::SignatureInvalid)
        );
    }

    #[test]
    fn records_with_other_members_are_refused() {
        let me = identity();
        let wire = record(&me).wire;
        let mut extra = wire.clone();
        extra.insert("display_name", "x".into());
        assert!(RegistrationRecord::from_json(&Json::Object(extra)).is_none());
        let mut missing = wire.clone();
        missing.remove("native_id");
        assert!(RegistrationRecord::from_json(&Json::Object(missing)).is_none());
        let mut bad_security = wire;
        bad_security.insert("security", "s".into());
        assert!(RegistrationRecord::from_json(&Json::Object(bad_security)).is_none());
        assert!(
            me.register(
                SessionId::parse("7gq3m8z2c5k9t1w4x6b0n2r8vd").unwrap(),
                Token::parse("h").unwrap(),
                "\u{ffff}",
                "/",
                Timestamp::parse("2026-10-03T11:59:00Z").unwrap(),
            )
            .is_none()
        );
    }
}
