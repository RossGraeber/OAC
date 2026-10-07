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
//! Both belong to authorization (`spec/security.md` §9, §11.3; #54, F5). This module reads
//! and changes them only through [`AuthorizationEngine`]: the binding table
//! ([`AuthorizationEngine::binding`], [`AuthorizationEngine::bind`],
//! [`AuthorizationEngine::mark_conflict`], which refuses an own session, [SEC-PRS-015]) and
//! the relation test, the engine's `accept-presence` decision
//! ([`AuthorizationEngine::decide`] with [`AuthorizationRequest::AcceptPresence`]). Every
//! time-dependent check reads the engine's clock, the receiver clock the replay checks read.
//!
//! An accepted record is still untrusted content as far as what it says goes (§1.2): its
//! descriptor is display and capability data, never authority.

use crate::authorization::{AuthorizationEngine, AuthorizationRequest, Binding, Kind};
use crate::canonical::SigningDomain;
use crate::ids::{KeyId, SessionId, Timestamp};
use crate::json::{Json, JsonObject};
use crate::keys::DeviceIdentity;
use crate::presence::PresenceRecord;
use crate::registry::{PresenceAcceptance, PresenceRegistry, RecordOrigin};
use crate::replay::inside_replay_window;
use crate::signing::verify_signed;
use crate::transport::{CarrierHandle, Destination, Payload, PayloadKind};
use std::time::Instant;

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
/// - `engine`: the trusted key set, this device's key id, the binding table and the
///   relation test, and the clock freshness is read on.
/// - `now`: the engine clock's instant on the monotonic clock the registry measures
///   lifetimes on.
pub fn accept_authenticated_record(
    record: &AuthenticatedPresenceRecord,
    engine: &mut AuthorizationEngine,
    registry: &mut PresenceRegistry,
    carrier: CarrierHandle,
    now: Instant,
) -> PresenceAuthOutcome {
    use PresenceAuthDiscard as D;
    let o = &record.wire;
    let (Some(inner), Some(audience)) = (o.get("record"), o.get("audience")) else {
        return PresenceAuthOutcome::discard(D::Malformed);
    };
    // Signature ([SEC-PRS-002]); `verify_signed` refuses a malformed `security`.
    let key = match verify_signed(engine.trusted_keys(), SigningDomain::Presence, o) {
        Ok(entry) => entry.key_id().clone(),
        Err(crate::signing::VerifyError::MalformedSecurity) => {
            return PresenceAuthOutcome::discard(D::Malformed);
        }
        Err(_) => return PresenceAuthOutcome::discard(D::Signature),
    };
    // Audience ([SEC-PRS-013]).
    if audience.as_str() != Some(engine.own_key_id().as_str()) {
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
        .is_some_and(|t| inside_replay_window(&t, &engine.now()));
    if !fresh {
        return PresenceAuthOutcome::discard(D::Stale);
    }
    let session = inner_obj
        .get("session_id")
        .and_then(Json::as_str)
        .and_then(SessionId::parse);
    let present = inner_obj.get("present").and_then(Json::as_bool);
    // Conflict ([SEC-PRS-003], [SEC-PRS-012], [SEC-PRS-014], [SEC-PRS-015]).
    let entry = session.as_ref().and_then(|s| engine.binding(s).cloned());
    match (&entry, &session) {
        (Some(Binding::Conflict(_)), _) => {
            return PresenceAuthOutcome::discard(D::UnderConflict);
        }
        (Some(Binding::Key(b)), Some(s)) if b != &key => {
            // Only a related claimant locks the id ([SEC-PRS-014]); the engine never marks an
            // own session ([SEC-PRS-015]).
            if engine.scope_of(s).is_none() && related(engine, &key, s) {
                engine.mark_conflict(s, &key);
            }
            return PresenceAuthOutcome {
                result: Err(D::BoundElsewhere),
                finding: true,
            };
        }
        _ => {}
    }
    // Relation, for an announcement ([SEC-AUZ-017]).
    if present == Some(true) && !session.as_ref().is_some_and(|s| related(engine, &key, s)) {
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
    if !parsed.present() && entry.is_none() {
        return PresenceAuthOutcome::discard(D::WithdrawalUnbound);
    }
    let sid = parsed.session_id().clone();
    let acceptance = registry.accept(parsed, carrier, RecordOrigin::OtherImplementation, now);
    if !matches!(acceptance, PresenceAcceptance::Accepted { .. }) {
        return PresenceAuthOutcome::discard(D::Registry);
    }
    if entry.is_none() {
        engine.bind(&sid, &key);
    }
    PresenceAuthOutcome {
        result: Ok(acceptance),
        finding: false,
    }
}

/// The relation test of [SEC-AUZ-017]: the engine's `accept-presence` decision for `key`
/// and `session`, at the engine clock's time, logged with its basis.
fn related(engine: &mut AuthorizationEngine, key: &KeyId, session: &SessionId) -> bool {
    engine
        .decide(&AuthorizationRequest::AcceptPresence {
            signing_key: key.clone(),
            session: session.clone(),
        })
        .permits(Kind::AcceptPresence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authorization::{Grant, LocalSide, MemoryDecisionLog, OperatorConfirmed, PeerSide};
    use crate::capabilities::{CapabilitiesEntry, SessionCapabilities, SessionDescriptor};
    use crate::clock::ManualClock;
    use crate::ids::{EXTENSION_ID_V0, Timestamp, Token, Version};
    use crate::keys::DeviceKey;
    use crate::pairing::{MemoryPairingStore, PairedPeer};
    use crate::presence::PresenceState;
    use std::sync::Arc;
    use std::time::Duration;

    fn ident(p: &str) -> DeviceIdentity {
        DeviceIdentity::new(DeviceKey::generate(), Token::parse(p).unwrap())
    }

    fn ts(s: &str) -> Timestamp {
        Timestamp::parse(s).unwrap()
    }

    fn pair(e: &mut AuthorizationEngine, peer: &DeviceIdentity, store: &MemoryPairingStore) {
        let p = PairedPeer::by_key_id_comparison(
            peer.principal().clone(),
            *peer.public_key(),
            peer.key_id(),
            ts("2026-10-03T00:00:00Z"),
            OperatorConfirmed::by_operator(),
        )
        .unwrap();
        e.pair(p, store).unwrap();
    }

    fn grant_to(e: &mut AuthorizationEngine, peer: &DeviceIdentity, store: &MemoryPairingStore) {
        let g = Grant::Outbound {
            writer: LocalSide::Device,
            target: PeerSide::device(peer.key_id().clone()),
        };
        e.add_grant(g, OperatorConfirmed::by_operator(), store)
            .unwrap();
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
            ts(at),
            lifetime_ms,
            SessionDescriptor::new(sid, caps, None, None).unwrap(),
        )
        .unwrap()
    }

    /// Through the real engine: an unrelated announcement is refused ([SEC-AUZ-017]); with
    /// an outbound grant naming the issuer it is accepted, binds, and is capped; a forwarded
    /// copy and a stale one are refused; a related second claimant is refused with a finding
    /// and locks the session id.
    #[test]
    fn issue_and_accept_through_the_engine() {
        let (me, peer, other) = (ident("me"), ident("peer"), ident("other"));
        let clock = Arc::new(ManualClock::new(ts("2026-10-03T12:00:01Z")));
        let store = MemoryPairingStore::new();
        let mut e =
            AuthorizationEngine::new(&me, clock.clone(), Box::new(MemoryDecisionLog::new()));
        pair(&mut e, &peer, &store);
        pair(&mut e, &other, &store);
        let t = Instant::now();
        let rec = announcement(4, 3_600_000, "2026-10-03T12:00:00Z");
        let sid = rec.session_id().clone();
        let carrier = CarrierHandle::from_opaque(b"c".to_vec());
        let mut reg = PresenceRegistry::new();
        let ar = AuthenticatedPresenceRecord::issue(&peer, &rec, me.key_id());

        let out = accept_authenticated_record(&ar, &mut e, &mut reg, carrier.clone(), t);
        assert_eq!(out.result, Err(PresenceAuthDiscard::Unrelated));

        grant_to(&mut e, &peer, &store);
        grant_to(&mut e, &other, &store);
        let forwarded = AuthenticatedPresenceRecord::issue(&peer, &rec, other.key_id());
        let out = accept_authenticated_record(&forwarded, &mut e, &mut reg, carrier.clone(), t);
        assert_eq!(out.result, Err(PresenceAuthDiscard::Audience));

        clock.set(ts("2026-10-03T12:05:00Z"));
        let out = accept_authenticated_record(&ar, &mut e, &mut reg, carrier.clone(), t);
        assert_eq!(out.result, Err(PresenceAuthDiscard::Stale));
        clock.set(ts("2026-10-03T12:00:01Z"));

        let out = accept_authenticated_record(&ar, &mut e, &mut reg, carrier.clone(), t);
        assert_eq!(
            out.result,
            Ok(PresenceAcceptance::Accepted {
                effective_lifetime_ms: Some(300_000)
            })
        );
        assert_eq!(e.binding(&sid), Some(&Binding::Key(peer.key_id().clone())));
        assert_eq!(
            reg.state(&sid, t + Duration::from_millis(300_000)),
            PresenceState::Unreachable
        );

        let squat = AuthenticatedPresenceRecord::issue(
            &other,
            &announcement(9, 60_000, "2026-10-03T12:00:00Z"),
            me.key_id(),
        );
        let out = accept_authenticated_record(&squat, &mut e, &mut reg, carrier, t);
        assert_eq!(out.result, Err(PresenceAuthDiscard::BoundElsewhere));
        assert!(out.finding);
        assert!(matches!(e.binding(&sid), Some(Binding::Conflict(_))));
        let (dest, payload) = ar.to_payload().unwrap();
        assert_eq!(dest, Destination::Device(me.key_id().clone()));
        assert_eq!(payload.kind(), PayloadKind::Presence);
    }
}
