// SPDX-License-Identifier: Apache-2.0

//! Payload sealing (`spec/security.md` §13 "Reading payloads on a shared transport",
//! "Agreement-key substitution or rollback" and "Clear-text fallback"; §14): devices on a
//! transport that delivers every frame to every device ([`SealBus`]), each running the core's
//! composed pipelines, and the core's admission of agreement statements, fed the recorded
//! `oac-sealing-fixture/1` statements signed with the published test keys.

use std::path::PathBuf;
use std::sync::Arc;

use oac_core::adapter::SendRequestResult;
use oac_core::authorization::{
    AuthorizationEngine, Grant, LocalSide, MemoryDecisionLog, OperatorConfirmed, PeerSide,
};
use oac_core::clock::{Clock, ManualClock, SystemClock};
use oac_core::delivery::{DeliveryState, ErrorCode};
use oac_core::ids::{KeyId, SessionId, Timestamp};
use oac_core::json::{self, Json, JsonObject};
use oac_core::keys::PublicKey;
use oac_core::pairing::{MemoryPairingStore, PairedPeer, PairingStore};
use oac_core::presence_auth::AuthenticatedPresenceRecord;
use oac_core::sealing::{AgreementPublicKey, AgreementStatement, HeldStatement, seal};
use oac_core::transport::{Destination, PayloadKind};
use oac_security_suite::pipeline::send_request;
use oac_security_suite::sealing::{MAX_FRAME, SealBus, SealedDevice};
use oac_security_suite::{announcement, identity, sid, token};

// ---- the wire -----------------------------------------------------------------------------

/// x, y and z on one sealing bus, paired with each other's statements; `sx` on x may write
/// to `sy` on y.
struct Three {
    bus: SealBus,
    x: SealedDevice,
    y: SealedDevice,
    z: SealedDevice,
    xa: oac_core::adapter::Attachment,
    sx: SessionId,
    sy: SessionId,
}

fn three(statements: bool) -> Three {
    let bus = SealBus::default();
    let x = SealedDevice::new(&bus, identity("principal-x"));
    let y = SealedDevice::new(&bus, identity("principal-y"));
    let z = SealedDevice::new(&bus, identity("principal-z"));
    for (a, b) in [(&x, &y), (&y, &x), (&x, &z), (&z, &x), (&y, &z), (&z, &y)] {
        a.pair(b, statements);
    }
    let (sx, sy) = (sid(1), sid(2));
    let xa = x.session(&sx);
    x.grant(Grant::Outbound {
        writer: LocalSide::Session(sx.clone()),
        target: PeerSide::session(y.key(), sy.clone()),
    });
    y.grant(Grant::Inbound {
        writer: PeerSide::session(x.key(), sx.clone()),
        target: LocalSide::Session(sy.clone()),
    });
    y.session(&sy);
    Three {
        bus,
        x,
        y,
        z,
        xa,
        sx,
        sy,
    }
}

/// §13 "Reading payloads on a shared transport": every frame reaches every device, but only
/// the recipient's agreement key opens it ([SEC-SEL-020], [SEC-SEL-030]). Each is a `sealed`
/// payload with a device destination, and neither the content, the key ids nor the session
/// ids appear on the wire ([IFC-TRN-100], [IFC-TRN-101]). The third device opens nothing,
/// hands off nothing and sends nothing ([SEC-SEL-031]).
#[test]
fn s13_only_the_recipient_device_opens_a_frame() {
    let t = three(true);
    let r = t.x.adapter.send(send_request(&t.xa, &t.sy, "for y alone"));
    let SendRequestResult::Sent { receipts, .. } = r else {
        panic!("not sent: {r:?}");
    };
    assert_eq!(t.y.adapter.deliver_calls(), 1);
    assert_eq!(
        receipts
            .0
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .state(),
        DeliveryState::HandedToHarness
    );
    let (refused, send_presence, session_subs) = t.bus.unsealed_offers();
    assert!(refused.is_empty() && send_presence == 0 && session_subs == 0);
    let (yo, zo) = (t.y.opener(), t.z.opener());
    let mut to_y = 0;
    for taken in t.bus.taken() {
        assert_eq!(taken.kind, PayloadKind::Sealed);
        let Destination::Device(to) = &taken.destination else {
            panic!("a session destination on the wire");
        };
        let wire = String::from_utf8_lossy(&taken.octets).into_owned();
        for clear in [
            "for y alone",
            t.x.key().as_str(),
            t.y.key().as_str(),
            t.sx.as_str(),
            t.sy.as_str(),
        ] {
            assert!(!wire.contains(clear), "{clear} on the wire");
        }
        assert!(zo.open(&taken.octets).is_none(), "z opened a frame");
        if to == &t.y.key() {
            to_y += 1;
            assert!(yo.open(&taken.octets).is_some());
        }
    }
    assert!(to_y >= 2, "x's announcement and envelope");
    assert_eq!(t.z.adapter.deliver_calls(), 0);
    assert!(t.bus.from(&t.z.key()).is_empty(), "z sent nothing");
    assert!(t.z.pipes.discarded_frames() >= 3);
}

/// §13 "Reading payloads on a shared transport" ([SEC-SEL-035]): y seals what it opened again
/// to z. z binds the envelope's `to` to y, is not the announcement's audience, and never
/// sent the envelope the receipt describes: it drops all three before any other check,
/// answering nothing, though it trusts x and grants x's device everything.
#[test]
fn s13_a_payload_forwarded_to_a_third_device_is_dropped() {
    let t = three(true);
    let r = t.x.adapter.send(send_request(&t.xa, &t.sy, "forward me"));
    assert!(matches!(r, SendRequestResult::Sent { .. }), "{r:?}");
    let yk = t.y.key();
    t.z.pipes.with_engine(|e| e.bind(&t.sy, &yk));
    t.z.grant(Grant::Inbound {
        writer: PeerSide::device(t.x.key()),
        target: LocalSide::Device,
    });
    let (yo, xo) = (t.y.opener(), t.x.opener());
    let mut opened: Vec<_> = t
        .bus
        .from(&t.x.key())
        .iter()
        .filter_map(|f| yo.open(f))
        .collect();
    opened.extend(
        t.bus
            .from(&t.y.key())
            .iter()
            .filter_map(|f| xo.open(f))
            .filter(|o| o.kind == PayloadKind::Receipt),
    );
    for kind in [
        PayloadKind::Envelope,
        PayloadKind::Presence,
        PayloadKind::Receipt,
    ] {
        assert!(opened.iter().any(|o| o.kind == kind), "{kind:?}");
    }
    let before = t.bus.from(&t.z.key()).len();
    for o in &opened {
        t.bus.inject(
            PayloadKind::Sealed,
            &seal(&t.z.agreement_key(), o.kind, &o.payload, MAX_FRAME).unwrap(),
        );
    }
    assert_eq!(t.z.adapter.deliver_calls(), 0);
    assert_eq!(t.bus.from(&t.z.key()).len(), before, "z answered nothing");
}

/// §13 "Clear-text fallback" ([SEC-SEL-024]): x trusts y but holds no agreement statement
/// for it. The envelope is `not-passed` with `transport-failure`, and nothing from x reaches
/// the transport at all, sealed or in the clear ([IFC-TRN-100]).
#[test]
fn s13_nothing_crosses_unsealed_without_a_statement() {
    let t = three(false);
    // y holds x's statement (so y's announcement of sy reaches x); x holds none for y.
    t.y.pipes
        .admit_statement(&t.x.pipes.agreement_statement().unwrap(), &t.y.pairings)
        .unwrap();
    let r =
        t.x.adapter
            .send(send_request(&t.xa, &t.sy, "in the clear?"));
    assert!(
        matches!(
            r,
            SendRequestResult::NotPassed {
                error: ErrorCode::TransportFailure,
                ..
            }
        ),
        "{r:?}"
    );
    assert!(t.bus.from(&t.x.key()).is_empty());
    let (refused, send_presence, _) = t.bus.unsealed_offers();
    assert!(refused.is_empty(), "offered in the clear: {refused:?}");
    assert_eq!(send_presence, 0);
    assert_eq!(t.y.adapter.deliver_calls(), 0);
}

/// §13 "Clear-text fallback" ([IFC-TRN-103]): a fresh announcement y signed for x, which x
/// would accept sealed, is not taken in the clear, through `watch_presence` or the device
/// subscription; sealed to x, it is.
#[test]
fn s13_an_unsealed_payload_from_a_sealing_transport_is_not_taken() {
    let t = three(true);
    let s5 = sid(5);
    t.x.grant(Grant::Outbound {
        writer: LocalSide::Session(t.sx.clone()),
        target: PeerSide::session(t.y.key(), s5.clone()),
    });
    let record = announcement(&s5, 1, SystemClock.now(), 60_000);
    let (_, payload) = AuthenticatedPresenceRecord::issue(t.y.pipes.device(), &record, &t.x.key())
        .to_payload()
        .unwrap();
    t.bus.inject_presence(payload.octets());
    t.bus.inject(PayloadKind::Presence, payload.octets());
    let r = t.x.adapter.send(send_request(&t.xa, &s5, "early"));
    assert!(
        matches!(
            r,
            SendRequestResult::Refused {
                error: ErrorCode::UnknownDestination
            }
        ),
        "{r:?}"
    );
    let frame = seal(
        &t.x.agreement_key(),
        PayloadKind::Presence,
        payload.octets(),
        MAX_FRAME,
    )
    .unwrap();
    t.bus.inject(PayloadKind::Sealed, &frame);
    let r = t.x.adapter.send(send_request(&t.xa, &s5, "now"));
    assert!(matches!(r, SendRequestResult::Sent { .. }), "{r:?}");
}

/// §13 "Agreement-key substitution or rollback" ([SEC-SEL-015]): removing a device's key
/// removes its statement in the same step; a later announcement from it is not sealed to
/// any key.
#[test]
fn s13_removing_a_key_removes_its_statement() {
    let t = three(true);
    let yk = t.y.key();
    let removal = t.x.pipes.with_engine(|e| {
        e.remove_key(&yk, OperatorConfirmed::by_operator(), &t.x.pairings)
            .unwrap()
    });
    assert!(removal.agreement_statement.is_some());
    assert!(t.x.pipes.with_engine(|e| e.held_statement(&yk).is_none()));
    assert!(
        t.x.pairings
            .load()
            .unwrap()
            .agreement_statements
            .iter()
            .all(|h| h.key_id() != &yk)
    );
}

// ---- statements: the recorded fixtures through the engine ---------------------------------

fn protocol_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../protocol")
}

fn load(path: &std::path::Path) -> JsonObject {
    json::parse(&std::fs::read(path).unwrap())
        .unwrap()
        .into_object()
        .unwrap()
}

fn s<'a>(o: &'a JsonObject, m: &str) -> &'a str {
    o.get(m).and_then(Json::as_str).unwrap()
}

/// A consumer as an `agreement`-stage fixture's `context` describes it: the trusted keys
/// paired, the `admitted` statements held, on a clock at `consumer_time`, built through the
/// pairing store as a restarted consumer would be ([SEC-SEL-041]).
fn consumer(context: &JsonObject) -> (AuthorizationEngine, MemoryPairingStore) {
    let at = Timestamp::parse(s(context, "consumer_time")).unwrap();
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(at.clone()));
    let own = identity("oac-test-own");
    let store = MemoryPairingStore::new();
    let mut e = AuthorizationEngine::new(&own, clock.clone(), Box::new(MemoryDecisionLog::new()));
    for k in context
        .get("trusted_keys")
        .and_then(Json::as_array)
        .unwrap()
    {
        let k = k.as_object().unwrap();
        let public = PublicKey::from_base64url(s(k, "public_key")).unwrap();
        let peer = PairedPeer::by_key_id_comparison(
            token(s(k, "principal")),
            public,
            &KeyId::parse(s(k, "key_id")).unwrap(),
            at.clone(),
            OperatorConfirmed::by_operator(),
        )
        .unwrap();
        e.pair(peer, &store).unwrap();
    }
    let mut snapshot = store.load().unwrap();
    if let Some(admitted) = context.get("admitted").and_then(Json::as_object) {
        for (key, h) in admitted.iter() {
            let h = h.as_object().unwrap();
            snapshot.agreement_statements.push(
                HeldStatement::from_store(
                    KeyId::parse(key).unwrap(),
                    AgreementPublicKey::from_base64url(s(h, "agreement_key")).unwrap(),
                    h.get("seq")
                        .and_then(Json::as_number)
                        .and_then(|n| n.plain_integer_in(0, u64::MAX))
                        .unwrap(),
                    Timestamp::parse(s(h, "issued_at")).unwrap(),
                )
                .unwrap(),
            );
        }
    }
    store.save(&snapshot).unwrap();
    let e = AuthorizationEngine::restore(&own, &store, clock, Box::new(MemoryDecisionLog::new()))
        .unwrap();
    (e, store)
}

/// Every `agreement`-stage fixture under `tests/protocol/sec-sel/`.
fn agreement_fixtures() -> Vec<(String, JsonObject)> {
    let mut out = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(protocol_dir().join("sec-sel"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    for f in files {
        let fx = load(&f);
        if fx.get("stage").and_then(Json::as_str) == Some("agreement") {
            out.push((f.file_name().unwrap().to_string_lossy().into_owned(), fx));
        }
    }
    out
}

/// §13 "Agreement-key substitution or rollback": each recorded statement is admitted or
/// refused as its fixture expects, by the engine a sender takes recipient keys from: an
/// untrusted or wrong-domain signer ([SEC-SEL-012]), a weak, non-canonical or high-bit key
/// ([SEC-SEL-013]), a `seq` not above the held one, whatever `issued_at` says ([SEC-SEL-014]),
/// and a statement from too far ahead ([SEC-SEL-042]). An admitted one replaces the held
/// statement and is saved; a refused one changes nothing.
#[test]
fn s13_agreement_statements_are_admitted_only_by_their_rules() {
    let fixtures = agreement_fixtures();
    assert!(fixtures.len() >= 14, "{}", fixtures.len());
    for (name, fx) in fixtures {
        let (mut e, store) = consumer(fx.get("context").and_then(Json::as_object).unwrap());
        let st = AgreementStatement::from_json(
            fx.get("input")
                .and_then(Json::as_object)
                .and_then(|i| i.get("statement"))
                .unwrap(),
        )
        .unwrap();
        let want = s(
            fx.get("expected").and_then(Json::as_object).unwrap(),
            "result",
        );
        let before = store.load().unwrap();
        let got = e.admit_statement(&st, &store).cloned();
        match (want, got) {
            ("admitted", Ok(h)) => {
                assert!(
                    store.load().unwrap().agreement_statements.contains(&h),
                    "{name}: not saved"
                );
            }
            ("refused", Err(_)) => {
                assert_eq!(
                    store.load().unwrap(),
                    before,
                    "{name}: a refusal changed the store"
                );
            }
            (w, g) => panic!("{name}: expected {w}, got {g:?}"),
        }
    }
}

/// §13 "Agreement-key substitution or rollback" ([SEC-SEL-041], [SEC-SEL-014]): a consumer
/// that restarts still holds the statement it admitted, so a replay of an older one, whose
/// private key an attacker may since hold, is refused after the restart as before it.
#[test]
fn s13_a_restart_does_not_readmit_an_older_statement() {
    let dir = protocol_dir().join("sec-sel");
    let older = load(&dir.join("SEC-SEL-011.p01-statement-admitted.json"));
    let newer = load(&dir.join("SEC-SEL-014.p01-higher-seq-replaces.json"));
    let statement = |fx: &JsonObject| {
        AgreementStatement::from_json(
            fx.get("input")
                .and_then(Json::as_object)
                .and_then(|i| i.get("statement"))
                .unwrap(),
        )
        .unwrap()
    };
    let context = older.get("context").and_then(Json::as_object).unwrap();
    let (mut e, store) = consumer(context);
    // Admit seq 1, then the higher one.
    e.admit_statement(&statement(&older), &store).unwrap();
    e.admit_statement(&statement(&newer), &store).unwrap();
    // Restart from the store: the newer one is held, and seq 1 is refused again.
    let at = Timestamp::parse(s(context, "consumer_time")).unwrap();
    let mut restarted = AuthorizationEngine::restore(
        &identity("oac-test-own"),
        &store,
        Arc::new(ManualClock::new(at)),
        Box::new(MemoryDecisionLog::new()),
    )
    .unwrap();
    assert!(
        restarted
            .admit_statement(&statement(&older), &store)
            .is_err()
    );
    assert!(
        restarted
            .admit_statement(&statement(&newer), &store)
            .is_err()
    );
}
