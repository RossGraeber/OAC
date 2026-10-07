// SPDX-License-Identifier: Apache-2.0

//! The authorization and pairing stages of `spec/security.md` §3.3, run through
//! `oac_core::authorization` (#54, F5).
//!
//! - `security`, step 4 of Table 7.1, for every fixture that passes steps 1 and 2 and whose
//!   outcome step 3 does not decide ([`step_4`]).
//! - `discovery-auth` (§9.4, §9.5), in full.
//! - `key-removal` (§5.3, [SEC-KEY-035]), in full.
//! - `exchange` (§9), with steps 3 and 5 of the security stage taken as passed: no
//!   `exchange` fixture is decided by the replay window or the duplicate store.
//! - `presence-auth` runs in `presence_receipts` (#55, F6), through the engine built here
//!   ([`engine`]) and `oac_core::presence_auth`; an `exchange` step `accept-presence` runs
//!   the same path.
//!
//! Every engine is built the way a running implementation fills one: its own device key
//! from the fixture's test key seed (`tests/protocol/sec-test-keys.json`), its own sessions
//! from registration records that key signs, its paired keys through
//! [`PairedPeer::by_key_id_comparison`] (the operator compared each key id), and its grants
//! through [`AuthorizationEngine::add_grant`].

use super::{Fixture, delivery_outcome, obj, protocol_dir, security_steps, str_of};
use oac_core::authorization::{
    AuthorizationEngine, AuthorizationRequest, BindOutcome, Binding, Grant, HandOffRecord, Kind,
    LocalSide, MemoryDecisionLog, OperatorConfirmed, PeerSide, Requester, SentRecord,
    WorkingDirectoryScope,
};

use oac_core::clock::{Clock, ManualClock};
use oac_core::delivery::DeliveryState;
use oac_core::envelope::{EnvelopeLimits, receive_envelope};
use oac_core::ids::{KeyId, SessionId, Timestamp, Token};
use oac_core::json::{self, Json, JsonObject};
use oac_core::keys::{DeviceIdentity, DeviceKey, PublicKey, SecretSeed};
use oac_core::pairing::{MemoryPairingStore, PairedPeer};
use oac_core::presence_auth::{AuthenticatedPresenceRecord, accept_authenticated_record};
use oac_core::registry::PresenceRegistry;
use oac_core::replay::DuplicateStore;
use oac_core::transport::CarrierHandle;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Instant;

struct TestKey {
    principal: String,
    seed_hex: String,
    public_key: String,
}

/// `tests/protocol/sec-test-keys.json`, by key id.
fn test_keys() -> BTreeMap<String, TestKey> {
    let text = std::fs::read(protocol_dir().join("sec-test-keys.json")).unwrap();
    let v = json::parse(&text).unwrap();
    v.as_object()
        .unwrap()
        .get("keys")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(|k| {
            let k = k.as_object().unwrap();
            (
                str_of(k, "key_id").unwrap().to_owned(),
                TestKey {
                    principal: str_of(k, "principal").unwrap().to_owned(),
                    seed_hex: str_of(k, "seed_hex").unwrap().to_owned(),
                    public_key: str_of(k, "public_key").unwrap().to_owned(),
                },
            )
        })
        .collect()
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn sid(s: &str) -> SessionId {
    SessionId::parse(s).unwrap_or_else(|| panic!("not a session id: {s}"))
}

fn kid(s: &str) -> KeyId {
    KeyId::parse(s).unwrap_or_else(|| panic!("not a key id: {s}"))
}

fn ts(s: &str) -> Timestamp {
    Timestamp::parse(s).unwrap_or_else(|| panic!("not a timestamp: {s}"))
}

fn ok() -> OperatorConfirmed {
    OperatorConfirmed::by_operator()
}

/// The device whose key id is `key_id`, from its test seed; a fresh one when the fixture
/// names no own key.
pub(super) fn own_identity(key_id: Option<&str>) -> DeviceIdentity {
    let Some(k) = key_id else {
        return DeviceIdentity::new(DeviceKey::generate(), Token::parse("oac-test-own").unwrap());
    };
    let keys = test_keys();
    let t = keys
        .get(k)
        .unwrap_or_else(|| panic!("own key {k} is not a test key"));
    let seed = SecretSeed::from_slice(&hex(&t.seed_hex)).unwrap();
    let id = DeviceIdentity::new(
        DeviceKey::from_seed(&seed),
        Token::parse(&t.principal).unwrap(),
    );
    assert_eq!(id.key_id().as_str(), k, "test seed gives another key id");
    id
}

/// A grant in the fixture form of §3.3.
fn grant(v: &Json) -> Grant {
    let g = v.as_object().unwrap();
    let local = |o: &JsonObject| {
        if let Some(s) = str_of(o, "session_id") {
            LocalSide::Session(sid(s))
        } else if let Some(s) = str_of(o, "working_directory_scope") {
            LocalSide::Scope(WorkingDirectoryScope::new(s))
        } else {
            assert_eq!(o.get("device"), Some(&Json::Bool(true)), "local side {o:?}");
            LocalSide::Device
        }
    };
    let peer = |o: &JsonObject| PeerSide {
        key_id: kid(str_of(o, "key_id").unwrap()),
        session_id: str_of(o, "session_id").map(sid),
    };
    let (w, t) = (obj(g, "writer"), obj(g, "target"));
    match str_of(g, "direction").unwrap() {
        "inbound" => Grant::Inbound {
            writer: peer(w),
            target: local(t),
        },
        "outbound" => Grant::Outbound {
            writer: local(w),
            target: peer(t),
        },
        d => panic!("direction {d}"),
    }
}

/// Adds the grants of a grant list (§3.3), as an operator would.
pub(super) fn add_grants(
    e: &mut AuthorizationEngine,
    store: &MemoryPairingStore,
    list: Option<&Json>,
) {
    for g in list.and_then(Json::as_array).into_iter().flatten() {
        e.add_grant(grant(g), ok(), store).unwrap();
    }
}

/// The engine of one implementation in a fixture: `own`, the trusted keys (the context's
/// `trusted_keys`, or every test key), the own sessions of `sessions`, the bindings of
/// `bindings` for other sessions, the `grants`, and the `sent` and `handed_off` records.
pub(super) fn engine(
    own: &DeviceIdentity,
    context: &JsonObject,
    clock: Arc<dyn Clock>,
) -> (AuthorizationEngine, MemoryPairingStore) {
    let store = MemoryPairingStore::new();
    let mut e = AuthorizationEngine::new(own, clock, Box::new(MemoryDecisionLog::new()));
    let listed: Vec<(String, String, String)> = match context.get("trusted_keys") {
        Some(list) => list
            .as_array()
            .unwrap()
            .iter()
            .map(|k| {
                let k = k.as_object().unwrap();
                (
                    str_of(k, "key_id").unwrap().to_owned(),
                    str_of(k, "principal").unwrap().to_owned(),
                    str_of(k, "public_key").unwrap().to_owned(),
                )
            })
            .collect(),
        None => test_keys()
            .into_iter()
            .map(|(id, t)| (id, t.principal, t.public_key))
            .collect(),
    };
    for (key_id, principal, public) in listed {
        if key_id == own.key_id().as_str() {
            continue; // [SEC-KEY-031]: already trusted.
        }
        let public = PublicKey::from_base64url(&public).expect("admission ([SEC-KEY-034])");
        let peer = PairedPeer::by_key_id_comparison(
            Token::parse(&principal).unwrap(),
            public,
            &kid(&key_id),
            ts("2026-10-03T00:00:00Z"),
            ok(),
        )
        .expect("the listed key id is the public key's");
        e.pair(peer, &store).unwrap();
    }
    if let Some(sessions) = context.get("sessions").and_then(Json::as_object) {
        for (s, v) in sessions.iter() {
            let scope = str_of(v.as_object().unwrap(), "working_directory_scope").unwrap();
            let record = own
                .register(
                    sid(s),
                    Token::parse("harness-test").unwrap(),
                    "native-test",
                    scope,
                    ts("2026-10-03T00:00:00Z"),
                )
                .unwrap();
            assert!(e.register_session(&record, own), "own session {s}");
        }
    }
    if let Some(bindings) = context.get("bindings").and_then(Json::as_object) {
        for (s, v) in bindings.iter() {
            let s = sid(s);
            if e.scope_of(&s).is_some() {
                assert_eq!(
                    v.as_str(),
                    Some(own.key_id().as_str()),
                    "own session {s} bound to another key"
                );
                continue;
            }
            match v {
                Json::String(k) => assert_eq!(e.bind(&s, &kid(k)), BindOutcome::Bound),
                Json::Object(c) => {
                    let keys = c.get("conflict").and_then(Json::as_array).unwrap();
                    let mut keys = keys.iter().map(|k| kid(k.as_str().unwrap()));
                    assert_eq!(e.bind(&s, &keys.next().unwrap()), BindOutcome::Bound);
                    for k in keys {
                        assert!(e.mark_conflict(&s, &k));
                    }
                }
                _ => panic!("binding {v:?}"),
            }
        }
    }
    add_grants(&mut e, &store, context.get("grants"));
    for r in context
        .get("sent")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let r = r.as_object().unwrap();
        let to = sid(str_of(r, "to").unwrap());
        // A `receipt-auth` sent list names neither `to_key_id` nor `created_at` (§3.3: the
        // members "a stage needs"): the key `to` is bound to stands in for the first, and a
        // fixed instant, read by no `receipt-auth` check, for the second.
        let to_key_id = match str_of(r, "to_key_id") {
            Some(k) => kid(k),
            None => match e.binding(&to) {
                Some(Binding::Key(k)) => k.clone(),
                _ => own.key_id().clone(),
            },
        };
        e.record_sent(SentRecord {
            id: Token::parse(str_of(r, "id").unwrap()).unwrap(),
            from: sid(str_of(r, "from").unwrap()),
            to,
            to_key_id,
            created_at: ts(str_of(r, "created_at").unwrap_or("2026-10-03T12:00:00Z")),
            nonce: str_of(r, "nonce").map(str::to_owned),
        });
    }
    for r in context
        .get("handed_off")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let r = r.as_object().unwrap();
        e.record_handoff(
            HandOffRecord {
                id: Token::parse(str_of(r, "id").unwrap()).unwrap(),
                from: sid(str_of(r, "from").unwrap()),
                to: sid(str_of(r, "to").unwrap()),
                created_at: ts(str_of(r, "created_at").unwrap()),
                conversation_id: str_of(r, "conversation_id").and_then(Token::parse),
                correlation_id: str_of(r, "correlation_id").and_then(Token::parse),
                key_id: None,
            },
            DeliveryState::HandedToHarness,
        );
    }
    (e, store)
}

/// A binding-table entry as a string: the key id, or `conflict:` and the key ids.
fn binding_text(v: &Json) -> String {
    match v {
        Json::String(k) => k.clone(),
        Json::Object(o) => {
            let keys: Vec<&str> = o
                .get("conflict")
                .and_then(Json::as_array)
                .unwrap()
                .iter()
                .map(|k| k.as_str().unwrap())
                .collect();
            format!("conflict:{}", keys.join(","))
        }
        _ => panic!("binding {v:?}"),
    }
}

/// The binding table, each entry as [`binding_text`] writes it.
fn bindings_of(e: &AuthorizationEngine) -> BTreeMap<String, String> {
    e.bindings()
        .map(|(s, b)| {
            let v = match b {
                Binding::Key(k) => k.as_str().to_owned(),
                Binding::Conflict(keys) => format!(
                    "conflict:{}",
                    keys.iter().map(KeyId::as_str).collect::<Vec<_>>().join(",")
                ),
            };
            (s.as_str().to_owned(), v)
        })
        .collect()
}

/// Compares the binding table with `expected.bindings_after`. The stages differ on whether
/// their binding map lists the implementation's own sessions (§3.3): `security` does,
/// `presence-auth` does not, so `include_own` says which.
pub(super) fn check_bindings_after(
    e: &AuthorizationEngine,
    expected: &JsonObject,
    include_own: bool,
) -> Result<(), String> {
    let Some(want) = expected.get("bindings_after").and_then(Json::as_object) else {
        return Ok(());
    };
    let want: BTreeMap<String, String> = want
        .iter()
        .map(|(k, v)| (k.to_owned(), binding_text(v)))
        .collect();
    let mut got = bindings_of(e);
    if !include_own {
        got.retain(|s, _| e.scope_of(&sid(s)).is_none());
    }
    if got != want {
        return Err(format!("bindings_after {got:?}, expected {want:?}"));
    }
    Ok(())
}

/// The own key id of a `security` fixture: the key its own sessions are bound to.
fn security_own_key(context: &JsonObject) -> Option<String> {
    let sessions = context.get("sessions").and_then(Json::as_object)?;
    let bindings = context.get("bindings").and_then(Json::as_object)?;
    let keys: BTreeSet<&str> = sessions
        .iter()
        .filter_map(|(s, _)| bindings.get(s).and_then(Json::as_str))
        .collect();
    assert!(keys.len() <= 1, "own sessions bound to several keys");
    keys.into_iter().next().map(str::to_owned)
}

/// The receiver's engine for a `security` or `replay` fixture: its own key is the one its
/// own sessions are bound to (a fresh key when the fixture names none), and it reads
/// `clock`, the same clock the replay checks read.
pub(super) fn security_engine(
    context: &JsonObject,
    clock: Arc<dyn Clock>,
) -> (AuthorizationEngine, MemoryPairingStore) {
    let own = own_identity(security_own_key(context).as_deref());
    engine(&own, context, clock)
}

/// The `record` and `bindings_after` members of a `security` fixture's `expected`, after
/// the envelope: `finding` is whether step 4 recorded one ([SEC-PRS-004]).
pub(super) fn check_step_4_record(
    expected: &JsonObject,
    e: &AuthorizationEngine,
    finding: bool,
) -> Result<(), String> {
    let record = if finding { "finding" } else { "none" };
    if let Some(w) = str_of(expected, "record")
        && w != record
    {
        return Err(format!("record {record}, expected {w}"));
    }
    check_bindings_after(e, expected, true)
}

/// Stage `discovery-auth` (§9.4, §9.5).
pub(super) fn run_discovery_auth(fx: &Fixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    let input = obj(&fx.v, "input");
    let own = own_identity(str_of(context, "own_key_id"));
    let clock = Arc::new(ManualClock::new(ts(str_of(context, "now").unwrap())));
    let (mut e, _) = engine(&own, context, clock);
    let requester = obj(input, "requester");
    let requester = match (str_of(requester, "session_id"), str_of(requester, "device")) {
        (Some(s), None) => Requester::Session(sid(s)),
        (None, Some(k)) => Requester::Device(kid(k)),
        _ => return Err(format!("requester {requester:?}")),
    };
    let d = e.decide(&AuthorizationRequest::Discover {
        requester,
        session: sid(str_of(input, "session").unwrap()),
    });
    let want = obj(&fx.v, "expected").get("discoverable") == Some(&Json::Bool(true));
    if d.permits(Kind::Discover) != want {
        return Err(format!("decision {d:?}, expected discoverable {want}"));
    }
    Ok(())
}

/// Stage `key-removal` (§5.3, [SEC-KEY-035]).
pub(super) fn run_key_removal(fx: &Fixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    let expected = obj(&fx.v, "expected");
    let own = own_identity(None);
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(ts("2026-10-03T12:00:00Z")));
    let (mut e, store) = engine(&own, context, clock.clone());
    let key = kid(str_of(obj(&fx.v, "input"), "remove_key_id").unwrap());
    e.remove_key(&key, ok(), &store)
        .map_err(|r| format!("remove_key: {r}"))?;
    check_bindings_after(&e, expected, true)?;
    let want: Vec<Grant> = expected
        .get("grants_after")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(grant)
        .collect();
    if e.grants() != want.as_slice() {
        return Err(format!("grants_after {:?}, expected {want:?}", e.grants()));
    }
    // The store holds the removal too.
    let back =
        AuthorizationEngine::restore(&own, &store, clock, Box::new(MemoryDecisionLog::new()))
            .map_err(|r| r.to_string())?;
    if back.trusted_keys().get(&key).is_some() || back.grants() != want.as_slice() {
        return Err("the store still holds the removed key or its grants".into());
    }
    Ok(())
}

/// An `exchange` step `accept-presence`: the consumer checks of `spec/security.md` §11
/// through `oac_core::presence_auth`, into the implementation's presence registry, with the
/// engine's binding table and relation test ([SEC-AUZ-017]).
fn accept_presence(
    e: &mut AuthorizationEngine,
    registry: &mut PresenceRegistry,
    record: &JsonObject,
) -> &'static str {
    let ar =
        AuthenticatedPresenceRecord::from_json(&Json::Object(record.clone())).expect("an object");
    let out = accept_authenticated_record(
        &ar,
        e,
        registry,
        CarrierHandle::from_opaque(b"exchange".to_vec()),
        Instant::now(),
    );
    if out.accepted() {
        "accepted"
    } else {
        "discarded"
    }
}

fn envelope_octets(step: &JsonObject) -> Vec<u8> {
    step.get("envelope").unwrap().to_compact().into_bytes()
}

/// Stage `exchange` (§9): two implementations, one sequence of operations.
pub(super) fn run_exchange(fx: &Fixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    // Each implementation: its engine, pairing store, clock and duplicate store.
    struct Impl {
        engine: AuthorizationEngine,
        store: MemoryPairingStore,
        clock: Arc<ManualClock>,
        duplicates: DuplicateStore,
        registry: PresenceRegistry,
    }
    let start = ts("2026-01-01T00:00:00Z");
    let mut impls: BTreeMap<String, Impl> = BTreeMap::new();
    for (label, c) in obj(context, "implementations").iter() {
        let c = c.as_object().unwrap();
        let own = own_identity(str_of(c, "own_key_id"));
        let clock = Arc::new(ManualClock::new(start.clone()));
        let (engine, store) = engine(&own, c, clock.clone());
        let duplicates = DuplicateStore::new(clock.clone());
        impls.insert(
            label.to_owned(),
            Impl {
                engine,
                store,
                clock,
                duplicates,
                registry: PresenceRegistry::new(),
            },
        );
    }
    // (actor, session, device): announcements each actor issued ([SEC-PRS-010]).
    let mut issued: BTreeSet<(String, String, String)> = BTreeSet::new();
    let limits = EnvelopeLimits::default();
    let mut results = Vec::new();
    for (i, step) in obj(&fx.v, "input")
        .get("steps")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .enumerate()
    {
        let step = step.as_object().unwrap();
        let at = ts(str_of(step, "at").unwrap());
        let actor = str_of(step, "actor").unwrap().to_owned();
        let imp = impls.get_mut(&actor).unwrap();
        imp.clock.set(at.clone());
        let (e, store) = (&mut imp.engine, &imp.store);
        let mut out: BTreeMap<String, String> = BTreeMap::new();
        match str_of(step, "op").unwrap() {
            "release" => {
                let (s, k) = (
                    str_of(step, "session").unwrap(),
                    str_of(step, "to_device").unwrap(),
                );
                let d = e.decide(&AuthorizationRequest::ReleasePresence {
                    session: sid(s),
                    device: kid(k),
                });
                let released = d.permits(Kind::ReleasePresence);
                if released {
                    issued.insert((actor.clone(), s.to_owned(), k.to_owned()));
                }
                out.insert("released".into(), released.to_string());
            }
            "accept-presence" => {
                let r = accept_presence(e, &mut imp.registry, obj(step, "authenticated_record"));
                out.insert("result".into(), r.to_string());
            }
            "discover" => {
                let d = e.decide(&AuthorizationRequest::Discover {
                    requester: Requester::Session(sid(str_of(step, "requester").unwrap())),
                    session: sid(str_of(step, "session").unwrap()),
                });
                out.insert("discoverable".into(), d.permits(Kind::Discover).to_string());
            }
            "remove-grant" => {
                let removed = e
                    .remove_grant(&grant(step.get("grant").unwrap()), store)
                    .map_err(|r| r.to_string())?;
                out.insert("removed".into(), removed.to_string());
            }
            "send" => {
                let msg = receive_envelope(&envelope_octets(step), &limits, &at)
                    .map_err(|r| format!("step {i}: envelope stage {r}"))?;
                let env = msg.envelope();
                let d = e.decide(&AuthorizationRequest::Discover {
                    requester: Requester::Session(env.from().clone()),
                    session: env.to().clone(),
                });
                let to_key = match e.binding(env.to()) {
                    Some(Binding::Key(k)) => Some(k.clone()),
                    _ => None,
                };
                // [SC-DLV-075] and [SEC-AUZ-018]: a session the requester may not discover
                // gets no envelope. The requester's announcement must have gone to the
                // target's device first ([SEC-PRS-010]).
                match to_key {
                    Some(k)
                        if d.permits(Kind::Discover)
                            && issued.contains(&(
                                actor.clone(),
                                env.from().as_str().to_owned(),
                                k.as_str().to_owned(),
                            )) =>
                    {
                        e.record_sent(SentRecord::of(env, k));
                        out.insert("result".into(), "sent".to_string());
                    }
                    _ => {
                        out.insert("result".into(), "refused".to_string());
                        out.insert("error".into(), "unknown-destination".to_string());
                    }
                }
            }
            "receive" => {
                // All five steps of Table 7.1, then the delivery outcome.
                let steps = security_steps(
                    &envelope_octets(step),
                    &limits,
                    e,
                    &at,
                    &imp.duplicates,
                    || Some("an entry is in flight with no earlier copy".into()),
                )
                .map_err(|r| format!("step {i}: {r}"))?;
                match (steps.reservation, steps.message) {
                    (Some(r), Some(msg)) => {
                        let delivery = str_of(step, "delivery")
                            .ok_or(format!("step {i}: passed, but no delivery outcome"))?;
                        let (state, error, handed_off) = delivery_outcome(delivery)?;
                        if handed_off {
                            r.handed_off();
                        } else {
                            r.not_handed_off();
                        }
                        e.record_handoff(
                            HandOffRecord::of(msg.envelope()),
                            DeliveryState::parse(state).unwrap(),
                        );
                        out.insert("result".into(), state.to_string());
                        if let Some(err) = error {
                            out.insert("error".into(), err.to_string());
                        }
                    }
                    _ => {
                        out.insert("result".into(), steps.state);
                        if let Some(err) = steps.error {
                            out.insert("error".into(), err);
                        }
                    }
                }
            }
            op => return Err(format!("step {i}: op {op}")),
        }
        results.push(out);
    }
    let want: Vec<BTreeMap<String, String>> = obj(&fx.v, "expected")
        .get("results")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(|r| {
            r.as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| {
                    let v = match v {
                        Json::Bool(b) => b.to_string(),
                        Json::String(s) => s.clone(),
                        _ => panic!("result member {v:?}"),
                    };
                    (k.to_owned(), v)
                })
                .collect()
        })
        .collect();
    if results != want {
        return Err(format!("results\n got  {results:?}\n want {want:?}"));
    }
    Ok(())
}
