// SPDX-License-Identifier: Apache-2.0

//! Registry and quota exhaustion (`spec/security.md` §8.3, §8.4, §9.5, §11; PR #317 and
//! #321 threat rows; #320, #325, #328): every store an attacker can feed is bounded, one
//! writer's share of it does not crowd out another's, and a full one refuses explicitly
//! instead of evicting what it must keep or growing without limit.

use std::time::{Duration, Instant};

use oac_core::adapter::HandOffOutcome;
use oac_core::authorization::{
    AuthorizationRequest, Binding, HandOffRecord, Kind, MAX_RECORD_PARTITIONS, Requester,
};
use oac_core::delivery::{DeliveryState, ErrorCode};
use oac_core::envelope::EnvelopeLimits;
use oac_core::ids::{KeyId, Timestamp};
use oac_core::presence_auth::{AuthenticatedPresenceRecord, accept_authenticated_record};
use oac_core::receiver::ReceiptLimiter;
use oac_core::registry::PresenceRegistry;
use oac_core::replay::{DuplicateKey, DuplicateStore};
use oac_core::transport::CarrierHandle;
use oac_security_suite::{Delivery, Device, announcement, granted_pair, identity, sid, token};

const NANOS_PER_SEC: i128 = 1_000_000_000;

/// A third device on `bob`'s clock, paired with `bob` and granted to write to `sid(2)`.
fn granted_peer(bob: &mut Device, principal: &str) -> Device {
    let peer = Device::new(principal, bob.clock.clone());
    bob.pair(&peer.identity);
    bob.allow_from(&peer.key_id(), &sid(2));
    peer
}

/// One envelope from `by`'s session `from` to `sid(2)`, received by `bob` with the hand-off
/// call returning `outcome`.
fn send(bob: &mut Device, by: &Device, id: &str, from: u8, outcome: HandOffOutcome) -> Delivery {
    bob.receive_with(by.sign(id, &sid(from), &sid(2), id).octets(), outcome)
}

fn internal_error(d: &Delivery) -> bool {
    d.outcome() == (DeliveryState::Failed, Some(ErrorCode::InternalError))
}

/// [SEC-RPL-023] (PR #317 "Store exhaustion"): a duplicate store full of live entries
/// refuses the next copy `failed` with `internal-error`, and no live entry is evicted to
/// make room, so a replay of an earlier envelope is still a `duplicate`. Once the entries'
/// deadlines pass, there is room again.
#[test]
fn x_full_duplicate_store_refuses_without_evicting() {
    let (alice, mut bob) = granted_pair();
    let carol = granted_peer(&mut bob, "carol");
    bob.duplicates = DuplicateStore::with_limits(bob.clock.clone(), 2, 2);
    let first = alice.sign("m1", &sid(1), &sid(2), "1");
    assert_eq!(
        bob.receive(first.octets()).state,
        DeliveryState::HandedToHarness
    );
    let c = send(&mut bob, &carol, "c1", 9, HandOffOutcome::Completed);
    assert_eq!(c.state, DeliveryState::HandedToHarness);
    let d = bob.receive(alice.sign("m2", &sid(1), &sid(2), "2").octets());
    assert!(internal_error(&d), "{:?}", d.outcome());
    assert!(d.handed.is_none());
    assert!(bob.duplicates.contains(&DuplicateKey::new(
        alice.key_id().as_str(),
        first.security().nonce()
    )));
    assert_eq!(bob.receive(first.octets()).state, DeliveryState::Duplicate);
    bob.clock.advance_nanos(301 * NANOS_PER_SEC);
    bob.duplicates.evict_expired();
    assert_eq!(
        bob.receive(alice.sign("m3", &sid(1), &sid(2), "3").octets())
            .state,
        DeliveryState::HandedToHarness
    );
}

/// #320, quota: one granted device that sends many unique envelopes fills only its own
/// share of the duplicate store. Its own copies past the share are refused `failed` with
/// `internal-error`; another device's copy is still handed off; and once the device's
/// entries reach their deadline it has its share back.
#[test]
fn x_one_key_cannot_fill_the_duplicate_store() {
    let (alice, mut bob) = granted_pair();
    let carol = granted_peer(&mut bob, "carol");
    bob.duplicates = DuplicateStore::with_limits(bob.clock.clone(), 8, 2);
    let flood: Vec<Delivery> = (0..5)
        .map(|i| {
            send(
                &mut bob,
                &alice,
                &format!("f{i}"),
                1,
                HandOffOutcome::Completed,
            )
        })
        .collect();
    let handed = flood
        .iter()
        .filter(|d| d.state == DeliveryState::HandedToHarness)
        .count();
    assert_eq!(handed, 2, "Alice's share");
    assert!(flood[2..].iter().all(internal_error));
    assert_eq!(bob.duplicates.held_by(alice.key_id().as_str()), 2);
    let c = send(&mut bob, &carol, "c1", 9, HandOffOutcome::Completed);
    assert_eq!(c.state, DeliveryState::HandedToHarness, "Carol keeps hers");
    bob.clock.advance_nanos(301 * NANOS_PER_SEC);
    let again = send(&mut bob, &alice, "f9", 1, HandOffOutcome::Completed);
    assert_eq!(
        again.state,
        DeliveryState::HandedToHarness,
        "the share is back"
    );
}

/// #320, headroom: a device never holds more entries than the store has free, so even with
/// no quota in its way one device takes at most half the store and leaves room for a
/// device that holds nothing.
#[test]
fn x_duplicate_store_headroom_leaves_room_for_another_key() {
    let (alice, mut bob) = granted_pair();
    let carol = granted_peer(&mut bob, "carol");
    bob.duplicates = DuplicateStore::with_limits(bob.clock.clone(), 6, 6);
    let handed = (0..6)
        .map(|i| {
            send(
                &mut bob,
                &alice,
                &format!("f{i}"),
                1,
                HandOffOutcome::Completed,
            )
        })
        .filter(|d| d.state == DeliveryState::HandedToHarness)
        .count();
    assert_eq!(handed, 3, "half the store");
    let c = send(&mut bob, &carol, "c1", 9, HandOffOutcome::Completed);
    assert_eq!(c.state, DeliveryState::HandedToHarness);
}

/// [SC-DLV-047] and the per-issuer share (PR #321 third review, note 1): one trusted,
/// related device that announces many sessions fills only its own share of the presence
/// registry, so another device's announcement is still accepted.
#[test]
fn x_one_issuer_cannot_fill_the_presence_registry() {
    let (mut alice, bob) = granted_pair();
    let carol = Device::new("carol", alice.clock.clone());
    alice.pair(&carol.identity);
    alice.allow_to(&sid(1), &carol.key_id());
    let mut registry = PresenceRegistry::with_limits(8, 2);
    let now = Instant::now();
    let mut accept = |by: &Device, n: u8, alice: &mut Device| {
        let r = AuthenticatedPresenceRecord::issue(
            &by.identity,
            &announcement(&sid(n), 1, alice.clock_now(), 60_000),
            &alice.key_id(),
        );
        accept_authenticated_record(
            &r,
            &mut alice.engine,
            &mut registry,
            CarrierHandle::from_opaque(b"link".to_vec()),
            now,
        )
        .accepted()
    };
    let flood: Vec<bool> = (10..20).map(|n| accept(&bob, n, &mut alice)).collect();
    assert_eq!(
        flood.iter().filter(|a| **a).count(),
        2,
        "Bob's share: {flood:?}"
    );
    assert!(accept(&carol, 30, &mut alice), "Carol still has hers");
}

/// [SC-DLV-047]: the registry never holds more than its capacity of other sessions, however
/// many devices announce.
#[test]
fn x_presence_registry_capacity_is_bounded() {
    let mut alice = Device::new("alice", oac_security_suite::clock());
    alice.register(&sid(1), "/work/a");
    let mut registry = PresenceRegistry::with_limits(3, 1);
    let now = Instant::now();
    for n in 0..6u8 {
        let issuer = identity(&format!("issuer{n}"));
        alice.pair(&issuer);
        alice.allow_to(&sid(1), issuer.key_id());
        let r = AuthenticatedPresenceRecord::issue(
            &issuer,
            &announcement(&sid(40 + n), 1, alice.clock_now(), 60_000),
            &alice.key_id(),
        );
        accept_authenticated_record(
            &r,
            &mut alice.engine,
            &mut registry,
            CarrierHandle::from_opaque(vec![n]),
            now,
        );
        assert!(registry.len() <= 3, "after {n}: {}", registry.len());
    }
}

/// [SEC-RPL-031] (PR #321): the receipt allowance is per sending device, so one device that
/// spends its own leaves another's intact, and the limiter tracks a bounded number of
/// devices, refusing a receipt to a new one rather than growing.
#[test]
fn x_receipt_allowance_is_per_device_and_bounded() {
    let mut limiter = ReceiptLimiter::new(2, Duration::from_secs(60), 2);
    let now = Instant::now();
    let (a, b, c) = (
        identity("a").key_id().clone(),
        identity("b").key_id().clone(),
        identity("c").key_id().clone(),
    );
    assert!(limiter.allow(&a, now) && limiter.allow(&a, now));
    assert!(!limiter.allow(&a, now), "a has spent its burst");
    assert!(limiter.allow(&b, now), "b is untouched");
    assert!(
        !limiter.allow(&c, now),
        "two devices tracked, none idle: no room for c"
    );
    assert_eq!(limiter.max_devices(), 2);
}

/// [SC-ENV-004], [SC-RCP-076]: an envelope over the receiver-wide limit is refused
/// `envelope-too-large` on its length alone, before it is parsed, so its content costs the
/// receiver nothing; not even an unverified `from` is read.
#[test]
fn x_oversized_envelope_is_refused_before_parsing() {
    let (_alice, mut bob) = granted_pair();
    bob.limits = EnvelopeLimits::default()
        .with_max_envelope_octets(65_536)
        .expect("a limit");
    let mut junk = vec![b'['; 70_000];
    junk[0] = b'{';
    let d = bob.receive(&junk);
    assert_eq!(
        d.outcome(),
        (DeliveryState::Rejected, Some(ErrorCode::EnvelopeTooLarge))
    );
    assert!(d.verified_by.is_none() && !d.looked_up);
}

/// The binding-table entries bound to `key`.
fn bound_to(bob: &Device, key: &KeyId) -> usize {
    bob.engine
        .bindings()
        .filter(|(_, b)| **b == Binding::Key(key.clone()))
        .count()
}

/// #325 ([SEC-PRS-005], [SEC-PRS-009]): a granted device that sends from fresh session ids
/// grows the binding table only up to its share. Each new id evicts the device's own
/// oldest entry, never the one a hand-off record is looked up through, so the reply to the
/// handed-off envelope still correlates ([SC-RCP-053]) and its sender stays discoverable
/// ([SEC-AUZ-016]).
#[test]
fn x_envelope_bindings_are_bounded() {
    let (alice, mut bob) = granted_pair();
    bob.engine = bob.engine.with_envelope_binding_limits(4, 2);
    let first = send(&mut bob, &alice, "m0", 1, HandOffOutcome::Completed);
    assert_eq!(first.state, DeliveryState::HandedToHarness);
    for n in 10..40 {
        let d = send(
            &mut bob,
            &alice,
            &format!("f{n}"),
            n,
            HandOffOutcome::Failed,
        );
        assert!(!internal_error(&d), "{n}: {:?}", d.outcome());
        assert!(bound_to(&bob, &alice.key_id()) <= 2, "after {n}");
        assert!(bob.engine.envelope_bindings() <= 4);
    }
    assert_eq!(
        bob.engine.binding(&sid(1)),
        Some(&Binding::Key(alice.key_id()))
    );
    assert!(
        bob.engine
            .reply_headers(&sid(2), &sid(1), Some("m0"))
            .correlated()
    );
    assert!(
        bob.engine
            .decide(&AuthorizationRequest::Discover {
                requester: Requester::Session(sid(2)),
                session: sid(1),
            })
            .permits(Kind::Discover)
    );
}

/// #325, fair share: in a full binding table, a device holding `n` entries takes one from
/// a device holding at least `n + 2`, the most of any, and otherwise is refused `failed`
/// with `internal-error`, binding nothing. Among the devices holding the most, the one that
/// added last gives an entry up, whatever their key ids.
#[test]
fn x_binding_table_fair_share_takes_from_the_heaviest() {
    let (alice, mut bob) = granted_pair();
    bob.engine = bob.engine.with_envelope_binding_limits(5, 3);
    let carol = granted_peer(&mut bob, "carol");
    let dave = granted_peer(&mut bob, "dave");
    let erin = granted_peer(&mut bob, "erin");
    // `hi` has the greater key id and adds first; `lo` adds last, so a tie-break by key-id
    // order would pick the wrong one.
    let (hi, lo) = if alice.key_id().as_str() > carol.key_id().as_str() {
        (&alice, &carol)
    } else {
        (&carol, &alice)
    };
    for n in [10, 11, 12] {
        send(&mut bob, hi, &format!("h{n}"), n, HandOffOutcome::Failed);
    }
    for n in [20, 21] {
        send(&mut bob, lo, &format!("l{n}"), n, HandOffOutcome::Failed);
    }
    assert_eq!(bob.engine.envelope_bindings(), 5);
    // `lo` holds 2; `hi` holds 3, not 4: no room, nothing bound.
    let d = send(&mut bob, lo, "l22", 22, HandOffOutcome::Failed);
    assert!(internal_error(&d), "{:?}", d.outcome());
    assert!(bob.engine.binding(&sid(22)).is_none());
    // Dave holds none: `hi`, holding the most, gives up its oldest.
    send(&mut bob, &dave, "d30", 30, HandOffOutcome::Failed);
    assert!(bob.engine.binding(&sid(10)).is_none());
    assert!(bob.engine.binding(&sid(30)).is_some());
    // `hi` and `lo` now hold 2 each: `lo` added last, so Erin takes `lo`'s oldest.
    send(&mut bob, &erin, "e40", 40, HandOffOutcome::Failed);
    assert!(bob.engine.binding(&sid(20)).is_none(), "lo pays");
    assert!(
        bob.engine.binding(&sid(11)).is_some(),
        "hi keeps its entries"
    );
    assert_eq!(bob.engine.envelope_bindings(), 5);
}

/// #328: a late copy from an own session that has ended is still handed off if a grant
/// covers it, but it does not make the ended session's hand-off partition again.
#[test]
fn x_late_copy_from_an_ended_session_records_nothing() {
    let (_alice, mut bob) = granted_pair();
    bob.register(&sid(3), "/work/b");
    bob.allow_from(&bob.key_id(), &sid(2));
    let late = bob.sign("late", &sid(3), &sid(2), "late");
    bob.end_session(&sid(3));
    let d = bob.receive(late.octets());
    assert_eq!(d.state, DeliveryState::HandedToHarness);
    assert!(bob.engine.handoff_records().all(|r| r.from != sid(3)));
}

/// #328, [SEC-AUZ-016]: the hand-off records stay within [`MAX_RECORD_PARTITIONS`]
/// partitions, and those whose records have all passed the reply period are dropped in
/// order of expiry: a writer that refreshed its partition keeps it, and a new writer is
/// recorded once the others have expired.
#[test]
fn x_expired_record_partitions_are_reclaimed_in_order() {
    let (_alice, mut bob) = granted_pair();
    let record = |id: &str, n: usize, at: Timestamp| HandOffRecord {
        id: token(id),
        from: sid(1),
        to: sid(2),
        created_at: at,
        conversation_id: None,
        correlation_id: None,
        key_id: KeyId::parse(&format!("{n:064x}")),
    };
    let start = bob.clock_now();
    for n in 0..MAX_RECORD_PARTITIONS {
        bob.engine.record_handoff(
            record("m", n, start.clone()),
            DeliveryState::HandedToHarness,
        );
    }
    bob.clock.advance_nanos(3600 * NANOS_PER_SEC);
    bob.engine.record_handoff(
        record("refresh", 0, bob.clock_now()),
        DeliveryState::HandedToHarness,
    );
    let ids = |bob: &Device| -> Vec<String> {
        bob.engine
            .handoff_records()
            .map(|r| r.id.as_str().to_owned())
            .collect()
    };
    bob.engine.record_handoff(
        record("new", MAX_RECORD_PARTITIONS, bob.clock_now()),
        DeliveryState::HandedToHarness,
    );
    assert!(!ids(&bob).contains(&"new".to_owned()), "all live: refused");
    // A day after `start`: every partition but the refreshed one has expired.
    bob.clock.advance_nanos(23 * 3600 * NANOS_PER_SEC);
    bob.engine.record_handoff(
        record("new", MAX_RECORD_PARTITIONS, bob.clock_now()),
        DeliveryState::HandedToHarness,
    );
    let mut left = ids(&bob);
    left.sort();
    assert_eq!(left, ["m", "new", "refresh"]);
}
