// SPDX-License-Identifier: Apache-2.0

//! Presence records between implementations (#55, F6): the authenticated presence record of
//! `spec/security.md` §11.1, how an issuer builds one, and the consumer's checks of §11.3 and
//! §11.4 that come before the presence registry's own rules (`spec/session-channels.md`
//! §7.2.3).
//!
//! # Order of checks (§11.4, last paragraph)
//!
//! 1. signature, under a trusted key, with the domain `oac-presence-v1` ([SEC-PRS-002]);
//! 2. audience: the consumer's own key id ([SEC-PRS-013]);
//! 3. freshness: `issued_at` inside the replay window at receipt ([SEC-PRS-006]);
//! 4. conflict: a session id bound to another key is refused with a finding, and is marked
//!    under conflict only when the claimant is related and the session is not the
//!    consumer's own ([SEC-PRS-003], [SEC-PRS-004], [SEC-PRS-012], [SEC-PRS-014],
//!    [SEC-PRS-015]);
//! 5. relation, for an announcement ([SEC-AUZ-017]);
//! 6. the record itself, `seq` order and the first binding ([SC-DLV-040], [SC-DLV-042],
//!    [SEC-PRS-005]), with the 300-second cap on what is accepted ([SEC-PRS-007]).
//!
//! # The binding table and the relation test
//!
//! Both belong to authorization (`spec/security.md` §9, §11.3; task F5). This module reaches
//! them only through [`ConsumerBindings`], which the authorization engine implements, so the
//! presence checks hold no second copy of either.
//!
//! An accepted record is still untrusted content as far as what it says goes (§1.2): its
//! descriptor is display and capability data, never authority.

use crate::canonical::SigningDomain;
use crate::ids::{KeyId, SessionId, Timestamp};
use crate::json::{Json, JsonObject};
use crate::keys::DeviceIdentity;
use crate::presence::PresenceRecord;
use crate::registry::{PresenceAcceptance, PresenceRegistry, RecordOrigin};
use crate::replay::inside_replay_window;
use crate::signing::verify_signed;
use crate::transport::{CarrierHandle, Destination, Payload, PayloadKind};
use crate::trust::TrustedKeySet;
use std::time::Instant;

/// A binding-table entry for one session id (§11.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingEntry {
    /// The table holds nothing for the session id.
    Unbound,
    /// The session id is bound to this key.
    Bound(KeyId),
    /// The session id is under a conflict mark: bound to no key ([SEC-PRS-012]).
    Conflict,
}

/// What the presence and receipt checks need from the binding table and the authorization
/// engine (`spec/security.md` §9, §11.3). The authorization engine of task F5 implements
/// it.
pub trait ConsumerBindings {
    /// The entry for `session`.
    fn binding(&self, session: &SessionId) -> BindingEntry;

    /// Whether `session` is one the consumer binds by a registration record of its own
    /// ([SEC-PRS-015]).
    fn is_own_session(&self, session: &SessionId) -> bool;

    /// The relation test of [SEC-AUZ-017] at `now`: whether a grant names `key` (with
    /// `session`, or with no session) as an inbound grant's writer or an outbound grant's
    /// target, or the consumer sent an envelope to `session` under `key`, or handed off one
    /// from `session`, within the reply period.
    fn related(&self, key: &KeyId, session: &SessionId, now: &Timestamp) -> bool;

    /// Binds an unbound `session` to `key` ([SEC-PRS-005]).
    fn bind(&mut self, session: &SessionId, key: &KeyId);

    /// Marks `session`, bound to `bound`, as under conflict, naming `bound` and `claimant`
    /// ([SEC-PRS-014]).
    fn mark_conflict(&mut self, session: &SessionId, bound: &KeyId, claimant: &KeyId);
}

/// An authenticated presence record (§11.1): a presence record, the one device it is issued
/// to, and the issuer's signature over both. It keeps the object as received.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedPresenceRecord {
    wire: JsonObject,
}

impl AuthenticatedPresenceRecord {
    /// Issues `record` to the device whose key id is `audience`, signed with `identity`
    /// under `oac-presence-v1` ([SEC-PRS-001], [SEC-PRS-011]). A record released to several
    /// devices is issued once for each.
    ///
    /// The caller decides the release under [SEC-AUZ-011] first, and issues the sender's
    /// announcement to a device before the first envelope to it ([SEC-PRS-010]). An
    /// announcement for another implementation should not carry a `lifetime_ms` above
    /// 300000 ([SEC-PRS-008]); [`crate::registry::CROSS_IMPLEMENTATION_PRESENCE_CAP_MS`].
    pub fn issue(
        identity: &DeviceIdentity,
        record: &PresenceRecord,
        audience: &KeyId,
    ) -> AuthenticatedPresenceRecord {
        let mut o = JsonObject::new();
        o.insert("record", Json::Object(record.as_json().clone()));
        o.insert("audience", audience.as_str().into());
        let mut sec = JsonObject::new();
        sec.insert("principal", identity.principal().as_str().into());
        sec.insert("key_id", identity.key_id().as_str().into());
        o.insert("security", Json::Object(sec.clone()));
        let signature = identity
            .sign_object(SigningDomain::Presence, &o)
            .expect("a presence record holds strings, booleans and integers only");
        sec.insert("signature", signature.as_str().into());
        o.insert("security", Json::Object(sec));
        AuthenticatedPresenceRecord { wire: o }
    }

    /// Wraps a received value. Any object is kept; [`accept_authenticated_record`] checks
    /// it.
    pub fn from_json(v: &Json) -> Option<AuthenticatedPresenceRecord> {
        Some(AuthenticatedPresenceRecord {
            wire: v.as_object()?.clone(),
        })
    }

    /// The object as held.
    pub fn as_json(&self) -> &JsonObject {
        &self.wire
    }

    /// The `presence` payload to send to the audience device (`spec/interfaces.md` Table
    /// 6.1). `None` when `audience` is not a key id.
    pub fn to_payload(&self) -> Option<(Destination, Payload)> {
        let audience = self
            .wire
            .get("audience")
            .and_then(Json::as_str)
            .and_then(KeyId::parse)?;
        Some((
            Destination::Device(audience),
            Payload::new(
                PayloadKind::Presence,
                Json::Object(self.wire.clone()).to_compact().into_bytes(),
            ),
        ))
    }
}

/// Why a consumer discarded an authenticated presence record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceAuthDiscard {
    /// Not an object of `record`, `audience` and `security`, or `security` not exactly
    /// `principal`, `key_id` and `signature`.
    Malformed,
    /// No trusted key, or the signature does not verify ([SEC-PRS-002]).
    Signature,
    /// Issued to another device ([SEC-PRS-013]).
    Audience,
    /// `issued_at` missing, or outside the replay window ([SEC-PRS-006]).
    Stale,
    /// The session id is under a conflict mark ([SEC-PRS-012]).
    UnderConflict,
    /// The session id is bound to another key ([SEC-PRS-003]); a finding is recorded.
    BoundElsewhere,
    /// An announcement from a key the consumer has no relation to ([SEC-AUZ-017]).
    Unrelated,
    /// A withdrawal for a session id with no binding ([SEC-PRS-005]).
    WithdrawalUnbound,
    /// The registry discarded the record (§7.2.3).
    Registry,
}

/// What a consumer did with an authenticated presence record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresenceAuthOutcome {
    /// The registry's acceptance, or why the record was discarded before or by it.
    pub result: Result<PresenceAcceptance, PresenceAuthDiscard>,
    /// Whether the consumer must record a finding: a claim on a session id bound to another
    /// key ([SEC-PRS-004]). The finding names key ids and session ids, never a body.
    pub finding: bool,
}

impl PresenceAuthOutcome {
    fn discard(why: PresenceAuthDiscard) -> PresenceAuthOutcome {
        PresenceAuthOutcome {
            result: Err(why),
            finding: false,
        }
    }

    /// Whether the record was accepted.
    pub fn accepted(&self) -> bool {
        matches!(self.result, Ok(PresenceAcceptance::Accepted { .. }))
    }
}

/// The consumer side of §11 for one authenticated presence record that arrived on
/// `carrier`, in the order of §11.4; an accepted record goes to `registry` as
/// [`RecordOrigin::OtherImplementation`].
///
/// - `keys`: the trusted key set; `own_key_id`: this device's key id.
/// - `bindings`: the binding table and the relation test.
/// - `now_wall`: the consumer's clock, for freshness and the relation test.
/// - `now`: the same instant on the monotonic clock the registry measures lifetimes on.
#[allow(clippy::too_many_arguments)]
pub fn accept_authenticated_record(
    record: &AuthenticatedPresenceRecord,
    keys: &TrustedKeySet,
    own_key_id: &KeyId,
    bindings: &mut dyn ConsumerBindings,
    registry: &mut PresenceRegistry,
    carrier: CarrierHandle,
    now_wall: &Timestamp,
    now: Instant,
) -> PresenceAuthOutcome {
    use PresenceAuthDiscard as D;
    let o = &record.wire;
    let (Some(inner), Some(audience)) = (o.get("record"), o.get("audience")) else {
        return PresenceAuthOutcome::discard(D::Malformed);
    };
    // Signature ([SEC-PRS-002]); `verify_signed` refuses a malformed `security`.
    let key = match verify_signed(keys, SigningDomain::Presence, o) {
        Ok(entry) => entry.key_id().clone(),
        Err(crate::signing::VerifyError::MalformedSecurity) => {
            return PresenceAuthOutcome::discard(D::Malformed);
        }
        Err(_) => return PresenceAuthOutcome::discard(D::Signature),
    };
    // Audience ([SEC-PRS-013]).
    if audience.as_str() != Some(own_key_id.as_str()) {
        return PresenceAuthOutcome::discard(D::Audience);
    }
    let Some(inner_obj) = inner.as_object() else {
        return PresenceAuthOutcome::discard(D::Registry);
    };
    // Freshness ([SEC-PRS-006]).
    let fresh = inner_obj
        .get("issued_at")
        .and_then(Json::as_str)
        .and_then(Timestamp::parse)
        .is_some_and(|t| inside_replay_window(&t, now_wall));
    if !fresh {
        return PresenceAuthOutcome::discard(D::Stale);
    }
    let session = inner_obj
        .get("session_id")
        .and_then(Json::as_str)
        .and_then(SessionId::parse);
    let present = inner_obj.get("present").and_then(Json::as_bool);
    // Conflict ([SEC-PRS-003], [SEC-PRS-012], [SEC-PRS-014], [SEC-PRS-015]).
    let entry = session
        .as_ref()
        .map_or(BindingEntry::Unbound, |s| bindings.binding(s));
    match (&entry, &session) {
        (BindingEntry::Conflict, _) => return PresenceAuthOutcome::discard(D::UnderConflict),
        (BindingEntry::Bound(b), Some(s)) if b != &key => {
            if !bindings.is_own_session(s) && bindings.related(&key, s, now_wall) {
                let b = b.clone();
                bindings.mark_conflict(s, &b, &key);
            }
            return PresenceAuthOutcome {
                result: Err(D::BoundElsewhere),
                finding: true,
            };
        }
        _ => {}
    }
    // Relation, for an announcement ([SEC-AUZ-017]).
    if present == Some(true)
        && !session
            .as_ref()
            .is_some_and(|s| bindings.related(&key, s, now_wall))
    {
        return PresenceAuthOutcome::discard(D::Unrelated);
    }
    // The record itself (§7.2.3).
    let parsed = match PresenceRecord::from_json(inner) {
        Ok(r) => r,
        Err(_) => return PresenceAuthOutcome::discard(D::Registry),
    };
    if registry
        .latest_seq(parsed.session_id())
        .is_some_and(|seq| parsed.seq() <= seq)
    {
        return PresenceAuthOutcome::discard(D::Registry);
    }
    // [SEC-PRS-005]: a withdrawal never binds.
    if !parsed.present() && entry == BindingEntry::Unbound {
        return PresenceAuthOutcome::discard(D::WithdrawalUnbound);
    }
    let sid = parsed.session_id().clone();
    let acceptance = registry.accept(parsed, carrier, RecordOrigin::OtherImplementation, now);
    if !matches!(acceptance, PresenceAcceptance::Accepted { .. }) {
        return PresenceAuthOutcome::discard(D::Registry);
    }
    if entry == BindingEntry::Unbound {
        bindings.bind(&sid, &key);
    }
    PresenceAuthOutcome {
        result: Ok(acceptance),
        finding: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{CapabilitiesEntry, SessionCapabilities, SessionDescriptor};
    use crate::ids::{EXTENSION_ID_V0, Token, Version};
    use crate::keys::DeviceKey;
    use crate::presence::PresenceState;
    use std::collections::HashMap;
    use std::time::Duration;

    #[derive(Default)]
    struct Table {
        map: HashMap<SessionId, BindingEntry>,
        related: bool,
    }

    impl ConsumerBindings for Table {
        fn binding(&self, s: &SessionId) -> BindingEntry {
            self.map.get(s).cloned().unwrap_or(BindingEntry::Unbound)
        }
        fn is_own_session(&self, _: &SessionId) -> bool {
            false
        }
        fn related(&self, _: &KeyId, _: &SessionId, _: &Timestamp) -> bool {
            self.related
        }
        fn bind(&mut self, s: &SessionId, k: &KeyId) {
            self.map.insert(s.clone(), BindingEntry::Bound(k.clone()));
        }
        fn mark_conflict(&mut self, s: &SessionId, _: &KeyId, _: &KeyId) {
            self.map.insert(s.clone(), BindingEntry::Conflict);
        }
    }

    fn ident(p: &str) -> DeviceIdentity {
        DeviceIdentity::new(DeviceKey::generate(), Token::parse(p).unwrap())
    }

    fn announcement(seq: u64, lifetime_ms: u64, at: &str) -> PresenceRecord {
        let sid = SessionId::parse("7gq3m8z2c5k9t1w4x6b0n2r8vd").unwrap();
        let caps = SessionCapabilities::declare([(
            EXTENSION_ID_V0,
            CapabilitiesEntry::new(Version { major: 0, minor: 1 }, true),
        )])
        .unwrap();
        PresenceRecord::announcement(
            seq,
            Timestamp::parse(at).unwrap(),
            lifetime_ms,
            SessionDescriptor::new(sid, caps, None, None).unwrap(),
        )
        .unwrap()
    }

    /// Issue, accept, bind; the cap applies; a forwarded copy, a stale one and an unrelated
    /// one are refused.
    #[test]
    fn issue_and_accept() {
        let (me, peer, other) = (ident("me"), ident("peer"), ident("other"));
        let mut keys = TrustedKeySet::new(&me);
        keys.add_paired_key(peer.principal().clone(), *peer.public_key())
            .unwrap();
        keys.add_paired_key(other.principal().clone(), *other.public_key())
            .unwrap();
        let now_wall = Timestamp::parse("2026-10-03T12:00:01Z").unwrap();
        let t = Instant::now();
        let rec = announcement(4, 3_600_000, "2026-10-03T12:00:00Z");
        let sid = rec.session_id().clone();
        let carrier = CarrierHandle::from_opaque(b"c".to_vec());
        let mut reg = PresenceRegistry::new();

        let mut unrelated = Table::default();
        let ar = AuthenticatedPresenceRecord::issue(&peer, &rec, me.key_id());
        let out = accept_authenticated_record(
            &ar,
            &keys,
            me.key_id(),
            &mut unrelated,
            &mut reg,
            carrier.clone(),
            &now_wall,
            t,
        );
        assert_eq!(out.result, Err(PresenceAuthDiscard::Unrelated));

        let mut table = Table {
            related: true,
            ..Table::default()
        };
        let forwarded = AuthenticatedPresenceRecord::issue(&peer, &rec, other.key_id());
        let out = accept_authenticated_record(
            &forwarded,
            &keys,
            me.key_id(),
            &mut table,
            &mut reg,
            carrier.clone(),
            &now_wall,
            t,
        );
        assert_eq!(out.result, Err(PresenceAuthDiscard::Audience));

        let late = Timestamp::parse("2026-10-03T12:05:00Z").unwrap();
        let out = accept_authenticated_record(
            &ar,
            &keys,
            me.key_id(),
            &mut table,
            &mut reg,
            carrier.clone(),
            &late,
            t,
        );
        assert_eq!(out.result, Err(PresenceAuthDiscard::Stale));

        let out = accept_authenticated_record(
            &ar,
            &keys,
            me.key_id(),
            &mut table,
            &mut reg,
            carrier.clone(),
            &now_wall,
            t,
        );
        assert_eq!(
            out.result,
            Ok(PresenceAcceptance::Accepted {
                effective_lifetime_ms: Some(300_000)
            })
        );
        assert_eq!(
            table.binding(&sid),
            BindingEntry::Bound(peer.key_id().clone())
        );
        assert_eq!(
            reg.state(&sid, t + Duration::from_millis(300_000)),
            PresenceState::Unreachable
        );

        // A related second claimant: discarded, finding, conflict mark.
        let squat = AuthenticatedPresenceRecord::issue(
            &other,
            &announcement(9, 60_000, "2026-10-03T12:00:00Z"),
            me.key_id(),
        );
        let out = accept_authenticated_record(
            &squat,
            &keys,
            me.key_id(),
            &mut table,
            &mut reg,
            carrier,
            &now_wall,
            t,
        );
        assert_eq!(out.result, Err(PresenceAuthDiscard::BoundElsewhere));
        assert!(out.finding);
        assert_eq!(table.binding(&sid), BindingEntry::Conflict);
        let (dest, payload) = ar.to_payload().unwrap();
        assert_eq!(dest, Destination::Device(me.key_id().clone()));
        assert_eq!(payload.kind(), PayloadKind::Presence);
    }
}
