// SPDX-License-Identifier: Apache-2.0

//! Presence spoofing (`spec/security.md` §11, §13 "Presence forgery" and "Session-id
//! squatting"): a presence record changes what a consumer believes only when it is signed by
//! a trusted, related key, addressed to the consumer, fresh, and claims a session id no
//! other key holds. Every record goes through the consumer's real checks
//! ([`accept_authenticated_record`]) into a real registry.

use std::time::Instant;

use oac_core::authorization::Binding;
use oac_core::delivery::{DeliveryState, ErrorCode};
use oac_core::json;
use oac_core::presence::{PresenceRecord, PresenceState};
use oac_core::presence_auth::{
    AuthenticatedPresenceRecord, PresenceAuthDiscard, PresenceAuthOutcome,
    accept_authenticated_record,
};
use oac_core::registry::PresenceRegistry;
use oac_core::transport::CarrierHandle;
use oac_security_suite::{Device, announcement, granted_pair, rewrite, sid};

const NANOS_PER_SEC: i128 = 1_000_000_000;

/// Alice consumes presence. Bob is paired and related to her through her outbound grant;
/// Carol is paired and related too; Mallory is a stranger.
struct Consumer {
    alice: Device,
    bob: Device,
    carol: Device,
    mallory: Device,
    registry: PresenceRegistry,
    now: Instant,
}

impl Consumer {
    fn new() -> Consumer {
        let (mut alice, bob) = granted_pair();
        let carol = Device::new("carol", alice.clock.clone());
        let mallory = Device::new("mallory", alice.clock.clone());
        alice.pair(&carol.identity);
        Consumer {
            alice,
            bob,
            carol,
            mallory,
            registry: PresenceRegistry::new(),
            now: Instant::now(),
        }
    }

    /// Carol becomes related to Alice through a grant.
    fn relate_carol(&mut self) {
        let k = self.carol.key_id();
        self.alice.allow_to(&sid(1), &k);
    }

    fn issue(&self, by: &Device, record: &PresenceRecord) -> AuthenticatedPresenceRecord {
        AuthenticatedPresenceRecord::issue(&by.identity, record, &self.alice.key_id())
    }

    fn accept(&mut self, r: &AuthenticatedPresenceRecord) -> PresenceAuthOutcome {
        accept_authenticated_record(
            r,
            &mut self.alice.engine,
            &mut self.registry,
            CarrierHandle::from_opaque(b"link".to_vec()),
            self.now,
        )
    }

    fn state(&self, n: u8) -> PresenceState {
        self.registry.state(&sid(n), self.now)
    }

    /// Bob announces his session, and Alice accepts it.
    fn bob_online(&mut self) {
        let r = self.issue(
            &self.bob,
            &announcement(&sid(2), 1, self.alice.clock_now(), 60_000),
        );
        assert!(self.accept(&r).accepted());
        assert_eq!(self.state(2), PresenceState::Online);
    }
}

/// [SEC-PRS-002]: an announcement signed by a key the consumer never paired is discarded,
/// and the session stays unknown.
#[test]
fn s13_presence_signed_by_an_unpaired_key_is_discarded() {
    let mut c = Consumer::new();
    let r = c.issue(
        &c.mallory,
        &announcement(&sid(8), 1, c.alice.clock_now(), 60_000),
    );
    assert_eq!(c.accept(&r).result, Err(PresenceAuthDiscard::Signature));
    assert_eq!(c.state(8), PresenceState::Unknown);
}

/// [SEC-PRS-001], [SEC-PRS-002]: the record is signed whole; one rewritten on the path, here
/// to announce another session id, is discarded.
#[test]
fn s13_tampered_presence_record_is_discarded() {
    let mut c = Consumer::new();
    let r = c.issue(
        &c.bob,
        &announcement(&sid(2), 1, c.alice.clock_now(), 60_000),
    );
    let wire = json::Json::Object(r.as_json().clone()).to_compact();
    let altered = rewrite(wire.as_bytes(), sid(2).as_str(), sid(8).as_str());
    let forged = AuthenticatedPresenceRecord::from_json(&json::parse(&altered).unwrap()).unwrap();
    assert_eq!(
        c.accept(&forged).result,
        Err(PresenceAuthDiscard::Signature)
    );
    assert_eq!(c.state(8), PresenceState::Unknown);
    assert!(c.accept(&r).accepted(), "control");
}

/// [SEC-PRS-011], [SEC-PRS-013]: a record is addressed to one device; Carol forwarding the
/// copy Bob issued to her gets it discarded at Alice.
#[test]
fn s13_forwarded_presence_record_is_discarded() {
    let mut c = Consumer::new();
    let to_carol = AuthenticatedPresenceRecord::issue(
        &c.bob.identity,
        &announcement(&sid(2), 1, c.alice.clock_now(), 60_000),
        &c.carol.key_id(),
    );
    assert_eq!(
        c.accept(&to_carol).result,
        Err(PresenceAuthDiscard::Audience)
    );
    assert_eq!(c.state(2), PresenceState::Unknown);
}

/// [SEC-PRS-006]: a captured announcement replayed after the replay window, for example
/// after the consumer restarted or forgot the session, is discarded.
#[test]
fn s13_replayed_stale_presence_record_is_discarded() {
    let mut c = Consumer::new();
    let r = c.issue(
        &c.bob,
        &announcement(&sid(2), 1, c.alice.clock_now(), 60_000),
    );
    c.alice.clock.advance_nanos(301 * NANOS_PER_SEC);
    assert_eq!(c.accept(&r).result, Err(PresenceAuthDiscard::Stale));
    assert_eq!(c.state(2), PresenceState::Unknown);
}

/// [SEC-AUZ-017]: a paired device the consumer has no grant or exchange with cannot announce
/// sessions to it.
#[test]
fn s13_unrelated_issuer_cannot_announce() {
    let mut c = Consumer::new();
    let r = c.issue(
        &c.carol,
        &announcement(&sid(5), 1, c.alice.clock_now(), 60_000),
    );
    assert_eq!(c.accept(&r).result, Err(PresenceAuthDiscard::Unrelated));
    assert_eq!(c.state(5), PresenceState::Unknown);
}

/// [SEC-PRS-003], [SEC-PRS-005]: a withdrawal of Bob's session signed by a stranger, or by
/// a paired device it is not bound to, is discarded; the session stays online.
#[test]
fn s13_forged_withdrawal_cannot_take_a_session_offline() {
    let mut c = Consumer::new();
    c.bob_online();
    let withdrawal = PresenceRecord::withdrawal(sid(2), 5, c.alice.clock_now()).unwrap();
    let by_mallory = c.issue(&c.mallory, &withdrawal);
    assert_eq!(
        c.accept(&by_mallory).result,
        Err(PresenceAuthDiscard::Signature)
    );
    let by_carol = c.issue(&c.carol, &withdrawal);
    let out = c.accept(&by_carol);
    assert_eq!(out.result, Err(PresenceAuthDiscard::BoundElsewhere));
    assert!(out.finding);
    assert_eq!(
        c.alice.engine.binding(&sid(2)),
        Some(&Binding::Key(c.bob.key_id())),
        "an unrelated claimant locks nothing"
    );
    assert_eq!(c.state(2), PresenceState::Online);
}

/// `spec/security.md` §13 "Session-id squatting" ([SEC-PRS-004], [SEC-PRS-012],
/// [SEC-PRS-014]): a related device announcing a session id bound to another key is refused
/// with a finding, and the id is marked under conflict naming both keys. While the mark
/// stands, envelopes from that id are refused for both devices: the squat fails closed
/// until an operator removes one of the keys.
#[test]
fn s13_squatting_announcement_marks_conflict_and_fails_closed() {
    let mut c = Consumer::new();
    c.relate_carol();
    c.bob_online();
    c.alice.allow_from(&c.bob.key_id(), &sid(1));
    c.alice.allow_from(&c.carol.key_id(), &sid(1));
    let before = c
        .alice
        .receive(c.bob.sign("m1", &sid(2), &sid(1), "hi").octets());
    assert_eq!(before.state, DeliveryState::HandedToHarness);
    let squat = c.issue(
        &c.carol,
        &announcement(&sid(2), 9, c.alice.clock_now(), 60_000),
    );
    let out = c.accept(&squat);
    assert_eq!(out.result, Err(PresenceAuthDiscard::BoundElsewhere));
    assert!(out.finding);
    let Some(Binding::Conflict(keys)) = c.alice.engine.binding(&sid(2)).cloned() else {
        panic!("a conflict mark")
    };
    assert!(keys.contains(&c.bob.key_id()) && keys.contains(&c.carol.key_id()));
    for (who, id) in [(&c.bob, "m2"), (&c.carol, "m3")] {
        let d = c
            .alice
            .receive(who.sign(id, &sid(2), &sid(1), "hi").octets());
        assert_eq!(
            d.outcome(),
            (DeliveryState::Rejected, Some(ErrorCode::Unauthorized)),
            "{id}"
        );
    }
}

/// [SEC-PRS-015]: a related device that announces the consumer's own session id is refused
/// with a finding, and the own session is never put under conflict; it keeps its binding.
#[test]
fn s13_own_session_is_never_marked_under_conflict() {
    let mut c = Consumer::new();
    let r = c.issue(
        &c.bob,
        &announcement(&sid(1), 3, c.alice.clock_now(), 60_000),
    );
    let out = c.accept(&r);
    assert_eq!(out.result, Err(PresenceAuthDiscard::BoundElsewhere));
    assert!(out.finding);
    assert_eq!(
        c.alice.engine.binding(&sid(1)),
        Some(&Binding::Key(c.alice.key_id()))
    );
}
