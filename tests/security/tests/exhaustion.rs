// SPDX-License-Identifier: Apache-2.0

//! Registry and quota exhaustion (`spec/security.md` §8.3, §8.4, §11; PR #317 and #321
//! threat rows): every store an attacker can feed is bounded, and a full one refuses
//! explicitly instead of evicting what it must keep or growing without limit.

use std::time::{Duration, Instant};

use oac_core::delivery::{DeliveryState, ErrorCode};
use oac_core::envelope::EnvelopeLimits;
use oac_core::presence_auth::{AuthenticatedPresenceRecord, accept_authenticated_record};
use oac_core::receiver::ReceiptLimiter;
use oac_core::registry::PresenceRegistry;
use oac_core::replay::{DuplicateKey, DuplicateStore};
use oac_core::transport::CarrierHandle;
use oac_security_suite::{Device, announcement, granted_pair, identity, sid};

const NANOS_PER_SEC: i128 = 1_000_000_000;

/// [SEC-RPL-023] (PR #317 "Store exhaustion"): an authorized device that floods unique
/// envelopes fills the duplicate store; the next copy is refused `failed` with
/// `internal-error`, and no live entry is evicted to make room, so a replay of an earlier
/// envelope is still a `duplicate`. Once the entries' deadlines pass, there is room again.
#[test]
fn x_full_duplicate_store_refuses_without_evicting() {
    let (alice, mut bob) = granted_pair();
    bob.duplicates = DuplicateStore::with_capacity(bob.clock.clone(), 2);
    let first = alice.sign("m1", &sid(1), &sid(2), "1");
    for (i, env) in [first.clone(), alice.sign("m2", &sid(1), &sid(2), "2")]
        .iter()
        .enumerate()
    {
        assert_eq!(
            bob.receive(env.octets()).state,
            DeliveryState::HandedToHarness,
            "{i}"
        );
    }
    let d = bob.receive(alice.sign("m3", &sid(1), &sid(2), "3").octets());
    assert_eq!(
        d.outcome(),
        (DeliveryState::Failed, Some(ErrorCode::InternalError))
    );
    assert!(d.handed.is_none());
    assert!(bob.duplicates.contains(&DuplicateKey::new(
        alice.key_id().as_str(),
        first.security().nonce()
    )));
    assert_eq!(bob.receive(first.octets()).state, DeliveryState::Duplicate);
    bob.clock.advance_nanos(301 * NANOS_PER_SEC);
    bob.duplicates.evict_expired();
    assert_eq!(
        bob.receive(alice.sign("m4", &sid(1), &sid(2), "4").octets())
            .state,
        DeliveryState::HandedToHarness
    );
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

/// #325: binding-table entries that security step 4 creates for each new `from` are not yet
/// bounded the way presence-created ones are, so an authorized device under a
/// device-wide grant can grow the table with fresh session ids. Gated until #325 bounds it.
#[test]
#[ignore = "GATED on #325 (bound the binding-table entries authorize_delivery creates)"]
fn gated_x_envelope_bindings_are_bounded() {
    std::panic!(
        "GATED on #325: assert the binding table stays within its bound under a flood of fresh `from` ids"
    );
}
