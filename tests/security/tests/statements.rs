// SPDX-License-Identifier: Apache-2.0

//! Agreement statements at and after pairing (`spec/security.md` §13 "Stale, stripped or
//! substituted agreement statement at and after pairing"; §14.3 "Pairing" and "Recovery",
//! [SEC-SEL-044] to [SEC-SEL-052], #409): the operator's comparison covers both devices'
//! statements, a pairing confirms the values of the statement it compared, and a key admits
//! no statement below those values, or none at all when its pairing confirmed none.

use std::sync::Arc;

use oac_core::authorization::{AuthorizationEngine, MemoryDecisionLog, OperatorConfirmed};
use oac_core::clock::{Clock, ManualClock};
use oac_core::ids::Timestamp;
use oac_core::keys::DeviceIdentity;
use oac_core::pairing::{
    ConfirmError, MemoryPairingStore, PairedPeer, PairingEnd, PairingError, PairingInitiator,
    PairingResponse, PairingSession, PairingStore, ResponderPairing, StatementValue,
    judge_statement,
};
use oac_core::sealing::{
    AgreementKeyState, AgreementKeyStore, AgreementKeys, AgreementPrivateKey, AgreementPublicKey,
    AgreementStatement, MAX_STATEMENT_SEQ, MemoryAgreementKeyStore, StatementRefusal,
    StoredAgreementKey,
};
use oac_security_suite::{T0, identity, ts};

const MINUTE: i128 = 60_000_000_000;

/// `T0` plus `minutes`.
fn at(minutes: i128) -> Timestamp {
    Timestamp::from_unix_nanos(ts(T0).unix_nanos() + minutes * MINUTE).expect("a timestamp")
}

/// A statement of `issuer` for an agreement key named by `octet`, with `seq` and `issued_at`.
fn statement(
    issuer: &DeviceIdentity,
    octet: u8,
    seq: u64,
    issued_at: &Timestamp,
) -> AgreementStatement {
    let key = AgreementPublicKey::from_octets([octet; 32]).expect("an acceptable test key");
    AgreementStatement::issue(issuer, &key, seq, issued_at).expect("seq in range")
}

/// A consumer: its engine, its pairing store and its clock.
struct Consumer {
    own: DeviceIdentity,
    engine: AuthorizationEngine,
    store: MemoryPairingStore,
    clock: Arc<ManualClock>,
}

fn consumer() -> Consumer {
    let own = identity("consumer");
    let clock = Arc::new(ManualClock::new(ts(T0)));
    let engine = AuthorizationEngine::new(&own, clock.clone(), Box::new(MemoryDecisionLog::new()));
    Consumer {
        own,
        engine,
        store: MemoryPairingStore::new(),
        clock,
    }
}

impl Consumer {
    /// The operator pairs `issuer` by its key id, with `given` read beside its key file.
    fn pair(&mut self, issuer: &DeviceIdentity, given: Option<AgreementStatement>) {
        let mut peer = PairedPeer::by_key_id_comparison(
            issuer.principal().clone(),
            *issuer.public_key(),
            issuer.key_id(),
            self.clock.now(),
            OperatorConfirmed::by_operator(),
        )
        .expect("the key id matches");
        if let Some(st) = given {
            peer = peer.with_agreement_statement(st);
        }
        self.engine.pair(peer, &self.store).expect("paired");
    }

    /// The operator removes `issuer`'s key, and pairs it again with `given`.
    fn repair(&mut self, issuer: &DeviceIdentity, given: Option<AgreementStatement>) {
        self.engine
            .remove_key(
                issuer.key_id(),
                OperatorConfirmed::by_operator(),
                &self.store,
            )
            .expect("removed");
        self.pair(issuer, given);
    }

    fn admit(&mut self, st: &AgreementStatement) -> Result<(u64, Timestamp), StatementRefusal> {
        self.engine
            .admit_statement(st, &self.store)
            .map(|h| (h.seq(), h.issued_at().clone()))
    }

    /// The same consumer after a restart from its pairing store.
    fn restart(&mut self) {
        self.engine = AuthorizationEngine::restore(
            &self.own,
            &self.store,
            self.clock.clone(),
            Box::new(MemoryDecisionLog::new()),
        )
        .expect("restored");
    }

    fn confirmed_seq(&self, issuer: &DeviceIdentity) -> Option<u64> {
        self.engine
            .snapshot()
            .paired
            .iter()
            .find(|r| r.key_id() == *issuer.key_id())
            .and_then(|r| r.confirmed().map(|c| c.seq()))
    }
}

// ---- the pairing exchange: the comparison covers both statements -------------------------

fn start(
    id: &DeviceIdentity,
    st: Option<AgreementStatement>,
    now: &Timestamp,
) -> (PairingInitiator, oac_core::pairing::PairingOffer) {
    match st {
        Some(st) => PairingInitiator::start_with_statement(id, st, now.clone()),
        None => PairingInitiator::start(id, now.clone()),
    }
}

fn responder(st: Option<AgreementStatement>, now: &Timestamp) -> ResponderPairing {
    match st {
        Some(st) => ResponderPairing::start_with_statement(now.clone(), st),
        None => ResponderPairing::start(now.clone()),
    }
}

/// One honest exchange: `a` initiates, `b` responds, each giving the statement it has.
fn exchange(
    a: (&DeviceIdentity, Option<AgreementStatement>),
    b: (&DeviceIdentity, Option<AgreementStatement>),
    now: &Timestamp,
) -> (PairingSession, PairingSession) {
    let (init, offer) = start(a.0, a.1, now);
    let mut pairing = responder(b.1, now);
    let response = pairing.answer(b.0, offer, now).expect("answered");
    let (sa, reveal) = init.receive(response, now).expect("response taken");
    let sb = pairing.reveal(reveal, now).expect("revealed");
    (sa, sb)
}

/// The attacker sits between the devices and may rewrite the offer and the response. B sees
/// the (rewritten) offer and the honest reveal; A sees the (rewritten) response.
fn exchange_with_attacker(
    a: (&DeviceIdentity, Option<AgreementStatement>),
    b: (&DeviceIdentity, Option<AgreementStatement>),
    now: &Timestamp,
    alter_offer: impl FnOnce(&mut oac_core::pairing::PairingOffer),
    alter_response: impl FnOnce(&mut PairingResponse),
) -> (PairingSession, Result<PairingSession, PairingError>) {
    let (init, mut offer) = start(a.0, a.1, now);
    alter_offer(&mut offer);
    let mut pairing = responder(b.1, now);
    let mut response = pairing.answer(b.0, offer, now).expect("answered");
    alter_response(&mut response);
    let (sa, reveal) = init.receive(response, now).expect("response taken");
    (sa, pairing.reveal(reveal, now))
}

/// [SEC-SEL-044], [SEC-SEL-050]: with both devices giving a statement, both operators are
/// shown both statements, and the codes they compare are the same.
#[test]
fn s13_the_comparison_covers_both_devices_statements() {
    let (a, b) = (identity("alice"), identity("bob"));
    let (sa, sb) = (statement(&a, 3, 4, &at(-30)), statement(&b, 5, 9, &at(-20)));
    let (x, y) = exchange((&a, Some(sa.clone())), (&b, Some(sb.clone())), &at(0));
    assert_eq!(x.code(), y.code());
    // Each side shows its own statement and the peer's, and they agree across the two.
    assert_eq!(x.own_statement_value(), y.peer_statement_value());
    assert_eq!(y.own_statement_value(), x.peer_statement_value());
    let shown = x
        .peer_statement_value()
        .given()
        .expect("the peer's statement");
    assert_eq!((shown.seq(), shown.issued_at()), (9, &at(-20)));
    assert_eq!(shown.agreement_key().to_octets(), [5; 32]);
    let text = x.peer_statement_value().to_string();
    assert!(
        text.contains("seq 9") && text.contains("agreement_key"),
        "{text}"
    );
}

/// [SEC-SEL-044]: a statement the attacker strips from the response leaves the two devices
/// with different values, so the operator is shown two different codes.
#[test]
fn s13_a_statement_stripped_from_the_response_is_a_mismatch() {
    let (a, b) = (identity("alice"), identity("bob"));
    let sb = statement(&b, 5, 9, &at(-20));
    let (x, y) = exchange_with_attacker(
        (&a, Some(statement(&a, 3, 4, &at(-30)))),
        (&b, Some(sb)),
        &at(0),
        |_| {},
        |r| r.agreement_statement = None,
    );
    let y = y.expect("B completes its side");
    assert_eq!(*x.peer_statement_value(), StatementValue::None);
    assert_ne!(x.code(), y.code(), "the operator sees two codes");
    let mut x = x;
    assert!(matches!(
        x.confirm_entered(y.code(), &at(1)),
        Err(ConfirmError::Mismatch { .. })
    ));
}

/// [SEC-SEL-044]: a statement the attacker strips from the offer fails the commitment, so
/// the exchange ends before any code is shown.
#[test]
fn s13_a_statement_stripped_from_the_offer_ends_the_exchange() {
    let (a, b) = (identity("alice"), identity("bob"));
    let (_, y) = exchange_with_attacker(
        (&a, Some(statement(&a, 3, 4, &at(-30)))),
        (&b, Some(statement(&b, 5, 9, &at(-20)))),
        &at(0),
        |o| o.agreement_statement = None,
        |_| {},
    );
    assert!(matches!(
        y,
        Err(PairingError::Ended(PairingEnd::CommitmentMismatch))
    ));
}

/// [SEC-SEL-044]: an older statement of the responder, valid and signed by its device key,
/// given in place of the current one makes the two codes differ; so does an older statement
/// of the initiator in the offer, which also breaks the commitment.
#[test]
fn s13_an_older_statement_given_in_place_of_the_current_one_is_a_mismatch() {
    let (a, b) = (identity("alice"), identity("bob"));
    let old_b = statement(&b, 6, 2, &at(-600));
    let (x, y) = exchange_with_attacker(
        (&a, Some(statement(&a, 3, 4, &at(-30)))),
        (&b, Some(statement(&b, 5, 9, &at(-20)))),
        &at(0),
        |_| {},
        |r| r.agreement_statement = Some(old_b.clone()),
    );
    // A takes the old statement as an acceptable one, and shows its values ...
    assert_eq!(x.peer_statement_value().given().map(|g| g.seq()), Some(2));
    // ... which differ from what B compares.
    assert_ne!(x.code(), y.expect("B completes").code());
    // The initiator's older statement in the offer: the commitment does not match.
    let old_a = statement(&a, 4, 1, &at(-900));
    let (_, y) = exchange_with_attacker(
        (&a, Some(statement(&a, 3, 4, &at(-30)))),
        (&b, Some(statement(&b, 5, 9, &at(-20)))),
        &at(0),
        |o| o.agreement_statement = Some(old_a.clone()),
        |_| {},
    );
    assert!(matches!(
        y,
        Err(PairingError::Ended(PairingEnd::CommitmentMismatch))
    ));
}

/// [SEC-SEL-044]: a device that uses no sealing transport takes part in the comparison with
/// its own absence of a statement as a value: its code matches the sealing device's.
#[test]
fn s13_a_device_that_gives_no_statement_counts_as_a_value() {
    let (a, b) = (identity("alice"), identity("bob"));
    let sb = statement(&b, 5, 9, &at(-20));
    // A (no sealing) initiates, B (sealing) responds.
    let (x, y) = exchange((&a, None), (&b, Some(sb.clone())), &at(0));
    assert_eq!(x.code(), y.code());
    assert_eq!(*x.own_statement_value(), StatementValue::None);
    assert_eq!(*y.peer_statement_value(), StatementValue::None);
    assert!(x.peer_statement_value().given().is_some());
    // Reversed roles.
    let (x, y) = exchange((&b, Some(sb)), (&a, None), &at(0));
    assert_eq!(x.code(), y.code());
    // And neither giving one is a code of its own: the value `none` is not a statement.
    let (n1, n2) = exchange((&a, None), (&b, None), &at(0));
    assert_eq!(n1.code().len(), 6);
    assert_eq!(*n1.peer_statement_value(), StatementValue::None);
    assert_eq!(*n2.peer_statement_value(), StatementValue::None);
}

/// [SEC-SEL-051]: a statement the device refuses (signed by another key, issued from the
/// future) is checked before the code is shown and shown as a value of its own, so the codes
/// differ; the key it is paired with after a confirmation anyway has no confirmed values.
#[test]
fn s13_a_refused_statement_is_shown_as_its_own_value() {
    let (a, b, m) = (identity("alice"), identity("bob"), identity("mallory"));
    let honest = (&a, Some(statement(&a, 3, 4, &at(-30))));
    let sb = statement(&b, 5, 9, &at(-20));
    for (why, bad) in [
        ("signed by another key", statement(&m, 5, 9, &at(-20))),
        ("issued from the future", statement(&b, 5, 9, &at(600))),
    ] {
        let (x, y) = exchange_with_attacker(
            (honest.0, honest.1.clone()),
            (&b, Some(sb.clone())),
            &at(0),
            |_| {},
            |r| r.agreement_statement = Some(bad.clone()),
        );
        assert_eq!(*x.peer_statement_value(), StatementValue::Refused, "{why}");
        assert!(x.peer_statement_value().to_string().contains("refused"));
        assert_ne!(x.code(), y.expect("B completes").code(), "{why}");
        // Confirmed anyway, the key is paired with no statement and no confirmed values.
        let mut x = x;
        let peer = x
            .confirm_match(&at(1), OperatorConfirmed::by_operator())
            .unwrap();
        assert!(peer.agreement_statement().is_none(), "{why}");
        let mut c = consumer();
        c.engine.pair(peer, &c.store).unwrap();
        assert_eq!(c.confirmed_seq(&b), None, "{why}");
        assert_eq!(
            c.admit(&statement(&b, 5, 10, &at(-10))),
            Err(StatementRefusal::NoConfirmedValues),
            "{why}"
        );
    }
    // The initiator's statement is checked by the responder in the same way: a statement
    // signed by another key is committed to and passes the reveal, but is shown as refused,
    // so the two codes differ.
    let (x, y) = exchange(
        (&a, Some(statement(&m, 3, 4, &at(-30)))),
        (&b, Some(sb.clone())),
        &at(0),
    );
    assert_eq!(*y.peer_statement_value(), StatementValue::Refused);
    assert_ne!(x.code(), y.code());
    // The same check is available to the key-id flow, to show beside the key id.
    let own = identity("consumer");
    let shown = judge_statement(
        &own,
        b.principal(),
        b.public_key(),
        Some(&statement(&m, 5, 9, &at(-20))),
        &at(0),
    );
    assert_eq!(shown, StatementValue::Refused);
}

/// [SEC-SEL-044] with [SEC-SEL-046]: a completed pairing confirms the values of the
/// statement it compared, and the engine keeps them with the key, saved in the store.
#[test]
fn s13_a_pairing_confirms_the_statement_it_compared() {
    let (a, b) = (identity("alice"), identity("bob"));
    let sb = statement(&b, 5, 9, &at(-20));
    let (mut x, y) = exchange((&a, None), (&b, Some(sb)), &at(0));
    let mut c = consumer();
    let peer = x.confirm_entered(y.code(), &at(1)).unwrap();
    c.engine.pair(peer, &c.store).unwrap();
    let saved = c.store.load().unwrap();
    let confirmed = saved.paired[0].confirmed().expect("confirmed values");
    assert_eq!(
        (
            confirmed.seq(),
            confirmed.issued_at(),
            confirmed.agreement_key().to_octets()
        ),
        (9, &at(-20), [5; 32])
    );
    assert_eq!(
        c.engine.held_statement(b.key_id()).map(|h| h.seq()),
        Some(9)
    );
}

// ---- after the pairing: the floors --------------------------------------------------------

/// [SEC-SEL-049], [SEC-SEL-050]: a key paired when its device gave no statement admits none
/// later, however valid and however new: an attacker that had the statement removed at
/// pairing cannot deliver an older one afterwards. Its first statement comes from a pairing
/// that confirms it.
#[test]
fn s13_a_key_paired_without_a_statement_admits_none() {
    let b = identity("bob");
    let mut c = consumer();
    c.pair(&b, None);
    assert_eq!(c.confirmed_seq(&b), None);
    let older = statement(&b, 6, 2, &at(-600));
    assert_eq!(c.admit(&older), Err(StatementRefusal::NoConfirmedValues));
    assert_eq!(
        c.admit(&statement(&b, 7, 50, &at(-1))),
        Err(StatementRefusal::NoConfirmedValues)
    );
    assert!(c.engine.held_statement(b.key_id()).is_none());
    // The same after a restart.
    c.restart();
    assert_eq!(c.admit(&older), Err(StatementRefusal::NoConfirmedValues));
    // Removed and paired again with the current statement, the key admits its rotation.
    let current = statement(&b, 5, 9, &at(-20));
    c.repair(&b, Some(current));
    assert_eq!(c.admit(&statement(&b, 8, 10, &at(-5))), Ok((10, at(-5))));
}

/// [SEC-SEL-045]: with no held statement (it was lost), a statement below the confirmed
/// `seq`, or at it under another agreement key, is refused; the confirmed statement itself,
/// and any higher `seq` with a later `issued_at`, are admitted.
#[test]
fn s13_a_statement_below_the_confirmed_seq_is_refused_without_a_held_one() {
    let b = identity("bob");
    let mut c = consumer();
    c.pair(&b, Some(statement(&b, 5, 9, &at(-20))));
    // The held statement is lost: the snapshot keeps the paired key and its values only.
    let mut snapshot = c.store.load().unwrap();
    snapshot.agreement_statements.clear();
    c.store.save(&snapshot).unwrap();
    c.restart();
    assert!(c.engine.held_statement(b.key_id()).is_none());
    assert_eq!(
        c.admit(&statement(&b, 6, 8, &at(-10))),
        Err(StatementRefusal::BelowConfirmed),
        "a lower seq"
    );
    assert_eq!(
        c.admit(&statement(&b, 6, 9, &at(-10))),
        Err(StatementRefusal::BelowConfirmed),
        "the confirmed seq under another agreement key"
    );
    assert!(
        c.engine.held_statement(b.key_id()).is_none(),
        "nothing changed"
    );
    assert_eq!(
        c.admit(&statement(&b, 5, 9, &at(-20))),
        Ok((9, at(-20))),
        "the confirmed one"
    );
    assert_eq!(c.admit(&statement(&b, 7, 10, &at(-10))), Ok((10, at(-10))));
}

/// [SEC-SEL-048]: an issuer that lost its count restarts it ([SEC-SEL-052]); its consumer
/// removes the key and pairs again with the new statement (seq 0). The older statement, with
/// a higher `seq` (it would replace the new one under [SEC-SEL-014] alone), is refused for
/// its earlier `issued_at`, both from the pairing and from the transport. A normal rotation
/// after the restart is admitted.
#[test]
fn s13_a_lost_count_repairing_refuses_an_older_statement_with_a_higher_seq() {
    let b = identity("bob");
    // B's statements before it lost its count: seq 0, then seq 7 (agreement key 6).
    let store = MemoryAgreementKeyStore::new();
    let first = AgreementKeys::load_or_generate(&store, &b, &at(-600)).unwrap();
    assert_eq!(first.highest_seq(), 0);
    let old = statement(&b, 6, 7, &at(-300));
    let mut c = consumer();
    c.pair(&b, Some(old.clone()));
    assert_eq!(c.confirmed_seq(&b), Some(7));
    // B loses its count: a store with nothing in it gives a new key with seq 0, issued now.
    let fresh = MemoryAgreementKeyStore::new();
    let mut keys = AgreementKeys::load_or_generate(&fresh, &b, &at(-1)).unwrap();
    assert_eq!(keys.highest_seq(), 0);
    let current = keys.statement().clone();
    // The consumer's operator removes B's key and pairs it again, comparing the statements.
    c.repair(&b, Some(current.clone()));
    assert_eq!(c.confirmed_seq(&b), Some(0));
    // The attacker, who holds the old statement's private key, delivers it: higher seq, but
    // issued before the one the pairing confirmed.
    assert_eq!(c.admit(&old), Err(StatementRefusal::EarlierThanConfirmed));
    assert_eq!(
        c.engine.held_statement(b.key_id()).map(|h| h.seq()),
        Some(0)
    );
    // The same with nothing held (a restart that lost the held statement).
    let mut snapshot = c.store.load().unwrap();
    snapshot.agreement_statements.clear();
    c.store.save(&snapshot).unwrap();
    c.restart();
    assert_eq!(c.admit(&old), Err(StatementRefusal::EarlierThanConfirmed));
    // A normal rotation, seq 1 issued later, is admitted.
    c.clock.set(at(10));
    let rotated = keys.replace(&fresh, &b, &at(5)).unwrap().clone();
    assert_eq!(c.admit(&rotated), Ok((1, at(5))));
    // A clock that ran back below the floor fails closed: a rotation issued before the
    // confirmed statement is refused until the key is paired again.
    let behind = keys.replace(&fresh, &b, &at(-30)).unwrap().clone();
    assert_eq!(
        c.admit(&behind),
        Err(StatementRefusal::EarlierThanConfirmed)
    );
}

/// [SEC-SEL-048], [SEC-SEL-052]: an issuer whose statement carries the maximum `seq` has no
/// higher value left. It restarts its count, its consumer removes the key and pairs again,
/// and the maximum-`seq` statement is refused for its earlier `issued_at`, where
/// [SEC-SEL-014] alone would admit nothing over it and [SEC-SEL-045] alone would let it
/// replace the restarted one.
#[test]
fn s13_the_maximum_seq_repairing_refuses_an_older_statement() {
    let b = identity("bob");
    // The issuer's store holds a key at the maximum seq: no replacement is possible.
    let store = MemoryAgreementKeyStore::new();
    store
        .save(&AgreementKeyState {
            keys: vec![StoredAgreementKey {
                private_key: AgreementPrivateKey::from_octets(&mut [7; 32]),
                seq: MAX_STATEMENT_SEQ,
                issued_at: at(-300),
            }],
            highest_seq: Some(MAX_STATEMENT_SEQ),
        })
        .unwrap();
    let mut keys = AgreementKeys::load_or_generate(&store, &b, &at(0)).unwrap();
    let at_max = keys.statement().clone();
    assert_eq!(
        keys.replace(&store, &b, &at(1)).map(|_| ()),
        Err(oac_core::sealing::AgreementKeyError::SeqExhausted)
    );
    // The consumer holds the maximum-seq statement, and refuses every later one.
    let mut c = consumer();
    c.pair(&b, Some(at_max.clone()));
    assert_eq!(c.confirmed_seq(&b), Some(MAX_STATEMENT_SEQ));
    // The issuer restarts its count ([SEC-SEL-052]); its first statement has seq 0.
    let mut keys = AgreementKeys::restart_count(&store, &b, &at(-1)).unwrap();
    assert_eq!(keys.highest_seq(), 0);
    let restarted = keys.statement().clone();
    assert_eq!(
        c.admit(&restarted),
        Err(StatementRefusal::NotNewer),
        "a consumer that did not remove the key refuses the lower seq"
    );
    c.repair(&b, Some(restarted));
    assert_eq!(c.confirmed_seq(&b), Some(0));
    assert_eq!(
        c.admit(&at_max),
        Err(StatementRefusal::EarlierThanConfirmed)
    );
    // After the restart only the statements issued since count: seq 1, later, is a rotation.
    c.clock.set(at(10));
    let next = keys.replace(&store, &b, &at(5)).unwrap().clone();
    assert_eq!(c.admit(&next), Ok((1, at(5))));
}

/// [SEC-SEL-045], [SEC-SEL-048]: a normal rotation (seq s+1, a later `issued_at`) is still
/// admitted, as is a higher `seq` whose `issued_at` is earlier than the held statement's but
/// not earlier than the confirmed one ([SEC-SEL-014]: order from `seq`, never from the clock).
#[test]
fn s13_normal_rotation_is_still_admitted() {
    let b = identity("bob");
    let mut c = consumer();
    c.pair(&b, Some(statement(&b, 5, 4, &at(-100))));
    c.clock.set(at(30));
    assert_eq!(c.admit(&statement(&b, 6, 5, &at(10))), Ok((5, at(10))));
    assert_eq!(c.admit(&statement(&b, 7, 6, &at(20))), Ok((6, at(20))));
    // Higher seq, issued before the held statement but after the confirmed one.
    assert_eq!(c.admit(&statement(&b, 8, 7, &at(-50))), Ok((7, at(-50))));
    // The same seq as the held statement is still refused ([SEC-SEL-014]).
    assert_eq!(
        c.admit(&statement(&b, 9, 7, &at(25))),
        Err(StatementRefusal::NotNewer)
    );
}

/// [SEC-SEL-046], [SEC-SEL-047]: the confirmed values are saved with the key and survive a
/// restart, where an older statement is still refused; removing the key removes them in the
/// same step, from memory and from the saved snapshot, and a repeated pairing confirms new
/// ones.
#[test]
fn s13_confirmed_values_survive_a_restart_and_leave_with_the_key() {
    let b = identity("bob");
    let mut c = consumer();
    c.pair(&b, Some(statement(&b, 5, 9, &at(-20))));
    c.restart();
    assert_eq!(c.confirmed_seq(&b), Some(9));
    assert_eq!(
        c.admit(&statement(&b, 6, 3, &at(-5))),
        Err(StatementRefusal::NotNewer),
        "the held statement is back too"
    );
    // The same restart with the held statement dropped from the store: the floor remains.
    let mut snapshot = c.store.load().unwrap();
    snapshot.agreement_statements.clear();
    c.store.save(&snapshot).unwrap();
    c.restart();
    assert_eq!(
        c.admit(&statement(&b, 6, 3, &at(-5))),
        Err(StatementRefusal::BelowConfirmed)
    );
    // Removal takes the confirmed values in the same step.
    let removal = c
        .engine
        .remove_key(b.key_id(), OperatorConfirmed::by_operator(), &c.store)
        .unwrap();
    assert_eq!(removal.confirmed.map(|v| v.seq()), Some(9));
    assert!(c.engine.snapshot().paired.is_empty());
    assert!(c.store.load().unwrap().paired.is_empty());
    // A repeated pairing confirms new values; the old floor is gone.
    c.pair(&b, Some(statement(&b, 8, 2, &at(-10))));
    assert_eq!(c.confirmed_seq(&b), Some(2));
    assert_eq!(c.admit(&statement(&b, 9, 3, &at(-5))), Ok((3, at(-5))));
}
