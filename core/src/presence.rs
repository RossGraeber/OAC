// SPDX-License-Identifier: Apache-2.0

//! `PresenceRecord` and `PresenceState` (`spec/interfaces.md` §4.7): the presence record
//! of `spec/session-channels.md` §7.2.2, and the three presence states of §7.2.1.
//!
//! Accepting records, ordering them by `seq` and computing states over time is the
//! presence registry (task F6). This module reads and builds the record.

use crate::capabilities::SessionDescriptor;
use crate::ids::{SessionId, Timestamp};
use crate::json::{self, Json, JsonNumber, JsonObject};

/// The largest `seq` ([SC-DLV-024]).
pub const MAX_SEQ: u64 = 9_007_199_254_740_991;

/// The smallest and largest `lifetime_ms` ([SC-DLV-028]).
pub const LIFETIME_MS_RANGE: (u64, u64) = (1_000, 3_600_000);

/// What one observer concludes about a session (§7.2.1; [SC-DLV-020]). It is local: it
/// has no wire form and no serialization, so it cannot be passed to a transport or a peer
/// ([IFC-TYP-061]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceState {
    /// An accepted, non-stale announcement is held.
    Online,
    /// The latest accepted record is a withdrawal or is stale.
    Unreachable,
    /// No accepted record is held.
    Unknown,
}

/// The requirement a presence record fails; a consumer discards it ([SC-DLV-040]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceViolation {
    /// [SC-DLV-021]: not an I-JSON object, or a `null` inside.
    NotIJsonObject,
    /// [SC-DLV-022]: `session_id`, `seq`, `present` or `issued_at` is missing.
    MissingMember,
    /// [SC-DLV-023]: `session_id` is not a session id.
    BadSessionId,
    /// [SC-DLV-024]: `seq` is not a plain integer from 0 to 9007199254740991.
    BadSeq,
    /// [SC-DLV-025]: `present` is not a boolean.
    BadPresent,
    /// [SC-DLV-026]: `issued_at` is not a timestamp.
    BadIssuedAt,
    /// [SC-DLV-027]: an announcement without `lifetime_ms` or `descriptor`.
    AnnouncementIncomplete,
    /// [SC-DLV-028]: `lifetime_ms` is not a plain integer from 1000 to 3600000.
    BadLifetime,
    /// [SC-DLV-029]: `descriptor` lacks a session id or a `capabilities` member.
    BadDescriptor,
    /// [SC-DLV-030]: the descriptor names another session.
    DescriptorOtherSession,
    /// [SC-DLV-031]: a withdrawal with `lifetime_ms` or `descriptor`.
    WithdrawalWithAnnouncementMember,
}

/// An announcement's or a withdrawal's own members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresenceKind {
    /// `present: true`.
    Announcement {
        /// How long the announcement holds, in milliseconds.
        lifetime_ms: u64,
        /// The session's descriptor.
        descriptor: SessionDescriptor,
    },
    /// `present: false`.
    Withdrawal,
}

/// A presence record (§7.2.2). It keeps the record as received, unrecognized members
/// included ([IFC-TYP-003], [SC-DLV-041]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresenceRecord {
    session_id: SessionId,
    seq: u64,
    issued_at: Timestamp,
    kind: PresenceKind,
    wire: JsonObject,
}

impl PresenceRecord {
    /// Reads a record from its octets ([SC-DLV-021] to [SC-DLV-031]).
    ///
    /// # Errors
    ///
    /// The first requirement the record fails.
    pub fn from_octets(octets: &[u8]) -> Result<PresenceRecord, PresenceViolation> {
        let v = json::parse(octets).map_err(|_| PresenceViolation::NotIJsonObject)?;
        PresenceRecord::from_json(&v)
    }

    /// Reads a record from a parsed I-JSON value, checking [SC-DLV-021] to [SC-DLV-031] in
    /// that order.
    ///
    /// # Errors
    ///
    /// The first requirement the record fails.
    pub fn from_json(v: &Json) -> Result<PresenceRecord, PresenceViolation> {
        use PresenceViolation as V;
        let o = v
            .as_object()
            .filter(|_| !v.contains_null())
            .ok_or(V::NotIJsonObject)?;
        if ["session_id", "seq", "present", "issued_at"]
            .iter()
            .any(|m| !o.contains(m))
        {
            return Err(V::MissingMember);
        }
        let session_id = o
            .get("session_id")
            .and_then(Json::as_str)
            .and_then(SessionId::parse)
            .ok_or(V::BadSessionId)?;
        let seq = o
            .get("seq")
            .and_then(Json::as_number)
            .and_then(|n| n.plain_integer_in(0, MAX_SEQ))
            .ok_or(V::BadSeq)?;
        let present = o
            .get("present")
            .and_then(Json::as_bool)
            .ok_or(V::BadPresent)?;
        let issued_at = o
            .get("issued_at")
            .and_then(Json::as_str)
            .and_then(Timestamp::parse)
            .ok_or(V::BadIssuedAt)?;
        let kind = if present {
            let (Some(lifetime), Some(descriptor)) = (o.get("lifetime_ms"), o.get("descriptor"))
            else {
                return Err(V::AnnouncementIncomplete);
            };
            let lifetime_ms = lifetime
                .as_number()
                .and_then(|n| n.plain_integer_in(LIFETIME_MS_RANGE.0, LIFETIME_MS_RANGE.1))
                .ok_or(V::BadLifetime)?;
            let descriptor = SessionDescriptor::from_json(descriptor).ok_or(V::BadDescriptor)?;
            if descriptor.session_id() != &session_id {
                return Err(V::DescriptorOtherSession);
            }
            PresenceKind::Announcement {
                lifetime_ms,
                descriptor,
            }
        } else {
            if o.contains("lifetime_ms") || o.contains("descriptor") {
                return Err(V::WithdrawalWithAnnouncementMember);
            }
            PresenceKind::Withdrawal
        };
        Ok(PresenceRecord {
            session_id,
            seq,
            issued_at,
            kind,
            wire: o.clone(),
        })
    }

    /// An announcement this implementation issues for one of its sessions. The record's
    /// `session_id` is the descriptor's, so [SC-DLV-030] holds by construction; it has no
    /// member for a working directory ([SC-DLV-032]).
    ///
    /// # Errors
    ///
    /// [`PresenceViolation::BadSeq`] or [`PresenceViolation::BadLifetime`] for a value out
    /// of range.
    pub fn announcement(
        seq: u64,
        issued_at: Timestamp,
        lifetime_ms: u64,
        descriptor: SessionDescriptor,
    ) -> Result<PresenceRecord, PresenceViolation> {
        if seq > MAX_SEQ {
            return Err(PresenceViolation::BadSeq);
        }
        if !(LIFETIME_MS_RANGE.0..=LIFETIME_MS_RANGE.1).contains(&lifetime_ms) {
            return Err(PresenceViolation::BadLifetime);
        }
        let session_id = descriptor.session_id().clone();
        let mut wire = Self::header(&session_id, seq, true, &issued_at);
        wire.insert(
            "lifetime_ms",
            Json::Number(JsonNumber::from_u64(lifetime_ms)),
        );
        wire.insert("descriptor", Json::Object(descriptor.as_json().clone()));
        Ok(PresenceRecord {
            session_id,
            seq,
            issued_at,
            kind: PresenceKind::Announcement {
                lifetime_ms,
                descriptor,
            },
            wire,
        })
    }

    /// A withdrawal this implementation issues for one of its sessions.
    ///
    /// # Errors
    ///
    /// [`PresenceViolation::BadSeq`] for a `seq` out of range.
    pub fn withdrawal(
        session_id: SessionId,
        seq: u64,
        issued_at: Timestamp,
    ) -> Result<PresenceRecord, PresenceViolation> {
        if seq > MAX_SEQ {
            return Err(PresenceViolation::BadSeq);
        }
        let wire = Self::header(&session_id, seq, false, &issued_at);
        Ok(PresenceRecord {
            session_id,
            seq,
            issued_at,
            kind: PresenceKind::Withdrawal,
            wire,
        })
    }

    fn header(
        session_id: &SessionId,
        seq: u64,
        present: bool,
        issued_at: &Timestamp,
    ) -> JsonObject {
        let mut wire = JsonObject::new();
        wire.insert("session_id", session_id.as_str().into());
        wire.insert("seq", Json::Number(JsonNumber::from_u64(seq)));
        wire.insert("present", Json::Bool(present));
        wire.insert("issued_at", issued_at.as_str().into());
        wire
    }

    /// The session the record is about.
    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// The issuer's sequence number.
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Whether this is an announcement.
    pub fn present(&self) -> bool {
        matches!(self.kind, PresenceKind::Announcement { .. })
    }

    /// The issuer's clock reading.
    pub fn issued_at(&self) -> &Timestamp {
        &self.issued_at
    }

    /// The announcement's or the withdrawal's own members.
    pub fn kind(&self) -> &PresenceKind {
        &self.kind
    }

    /// The record as a JSON object.
    pub fn as_json(&self) -> &JsonObject {
        &self.wire
    }

    /// The record as compact JSON octets.
    pub fn to_octets(&self) -> Vec<u8> {
        Json::Object(self.wire.clone()).to_compact().into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{CapabilitiesEntry, SessionCapabilities};
    use crate::ids::{EXTENSION_ID_V0, Version};

    #[test]
    fn built_records_read_back() {
        let sid = SessionId::parse("7gq3m8z2c5k9t1w4x6b0n2r8vd").unwrap();
        let caps = SessionCapabilities::declare([(
            EXTENSION_ID_V0,
            CapabilitiesEntry::new(Version { major: 0, minor: 1 }, true),
        )])
        .unwrap();
        let d = SessionDescriptor::new(sid.clone(), caps, None, None).unwrap();
        let at = Timestamp::parse("2026-10-03T12:00:00.000Z").unwrap();
        let a = PresenceRecord::announcement(4, at.clone(), 60_000, d).unwrap();
        assert_eq!(PresenceRecord::from_octets(&a.to_octets()).unwrap(), a);
        let w = PresenceRecord::withdrawal(sid, 5, at.clone()).unwrap();
        assert_eq!(PresenceRecord::from_octets(&w.to_octets()).unwrap(), w);
        assert!(!w.present());
        assert_eq!(
            PresenceRecord::withdrawal(SessionId::from_random_octets([1; 16]), MAX_SEQ + 1, at)
                .unwrap_err(),
            PresenceViolation::BadSeq
        );
    }
}
