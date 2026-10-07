// SPDX-License-Identifier: Apache-2.0

//! Drives the conformance fixtures of `tests/protocol/` (E8, #48) through this crate's own
//! code: the tier "Spec conformance" of `oac-testing` §1, CI-default.
//!
//! Fixture format: `spec/session-channels.md` §3.3. Stages run here, the ones whose
//! subject is the core types and the envelope (#51, F2):
//!
//! - `envelope` (§3.3): [`receive_envelope`]; a valid envelope must round-trip.
//! - `receipt` (§8.5): [`DeliveryReceipt::from_json`] and the effective state.
//! - `negotiation` (§6.10): [`SessionCapabilities::agree`].
//! - `presence` (§7.5), `discovery`, `send`, `routing`, `receive`, `combine`, `reply`,
//!   `correlation`, and `spec/security.md`'s `presence-auth` and `receipt-auth` (#55, F6):
//!   see [`presence_receipts`]. The `presence` stage checks `discarded`, `states` and
//!   `send`.
//! - Every fixture, of any stage, whose `expected` holds `canonical`: the canonical text
//!   of `spec/security.md` §6.2 for its signed object.
//!
//! Stages of `spec/security.md` §3.3 run here (#52, F3):
//!
//! - `security`: envelope-stage validation, then Table 7.1: steps 1 and 2 (key resolution,
//!   signature) through [`authenticate`] (#52, F3); step 3 (replay window) through
//!   [`check_replay_window`] (#53, F4); step 4 (authorization) through
//!   `AuthorizationEngine::authorize_delivery` (#54, F5); and step 5 (duplicate store,
//!   preloaded from `context.duplicate_store`) through [`DuplicateStore::admit`] (#53, F4),
//!   which takes only the `AuthorizedMessage` step 4 produces.
//! - `replay` (#53, F4): a sequence of arrivals at one receiver, on a scripted
//!   [`ManualClock`], through the same steps, one [`DuplicateStore`] and one
//!   `AuthorizationEngine`, both reading that clock. An arrival's `grants_add` is added to
//!   the engine just before it. A copy that arrives `during_previous_handoff` must stay
//!   undecided ([`DuplicateStore::try_admit`] returns `None`) until the earlier copy's
//!   reservation is settled with its `delivery`.
//! - `key-id` (§5.2) and `registration` (§5.4), in full.
//! - [`verify_strict_alone_gives_the_sec_sig_verdicts`]: every `sec-sig` fixture through
//!   `ed25519-dalek`'s `VerifyingKey::verify_strict` with no other check, the run that
//!   `spec/security.md` §6.3 left UNVERIFIED.
//! - [`handoff_deadline_fixtures`]: the hand-off-deadline re-check (delivery stage step 4,
//!   [SC-RCP-091], [SC-RCP-092]) of the `routing` fixtures that test it.
//!
//! Stages run in [`authorization`] (#54, F5): `discovery-auth`, `key-removal` and `exchange`
//! in full (an `exchange` receive runs all five steps of Table 7.1; its `accept-presence`
//! runs `oac_core::presence_auth`). A copy refused at step 4 is checked to add no
//! duplicate-store entry, which a later arrival of the same envelope observes
//! (`SEC-RPL-022.p03`). `presence-auth`, every fixture, runs in [`presence_receipts`]
//! through the engine [`authorization`] builds.
//!
//! Other stages (`binding`, `mcp-binding`, `provenance`, `body`) exercise adapter-side
//! logic other tasks own. They are listed by name, so a fixture of an unlisted stage fails
//! instead of silently not running (#61, F12).

use oac_core::authorization::{AuthorizationEngine, HandOffRecord, Kind};
use oac_core::canonical::{SigningDomain, signed_text, signing_input};
use oac_core::capabilities::{Implemented, SessionCapabilities};
use oac_core::clock::{Clock, ManualClock};
use oac_core::delivery::DeliveryState;
use oac_core::envelope::{ChannelMessage, EnvelopeLimits, receive_envelope};
use oac_core::ids::{Timestamp, Token, Version};
use oac_core::json::{self, Json, JsonObject};
use oac_core::keys::{DeviceIdentity, DeviceKey, PublicKey};
use oac_core::receipt::DeliveryReceipt;
use oac_core::registration::RegistrationRecord;
use oac_core::replay::{
    Admission, DuplicateKey, DuplicateStore, HandOffDeadline, REPLAY_WINDOW_MS, Reservation,
    check_replay_window,
};
use oac_core::signing::authenticate;
use oac_core::trust::TrustedKeySet;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// A crate root resolves `mod` beside itself, so the path is given; tests/conformance/ is
// not a test target of its own.
#[path = "conformance/authorization.rs"]
mod authorization;
#[path = "conformance/presence_receipts.rs"]
mod presence_receipts;

fn protocol_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tests")
        .join("protocol")
}

struct Fixture {
    path: String,
    stage: String,
    kind: String,
    v: JsonObject,
}

fn fixtures() -> Vec<Fixture> {
    let mut out = Vec::new();
    let mut dirs: Vec<_> = std::fs::read_dir(protocol_dir())
        .expect("tests/protocol exists")
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        for f in files {
            let text = std::fs::read(&f).unwrap();
            let v = json::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
            let o = v.as_object().unwrap().clone();
            if str_of(&o, "fixture_format") != Some("oac-conformance-fixture/1") {
                continue;
            }
            out.push(Fixture {
                path: f
                    .strip_prefix(protocol_dir())
                    .unwrap()
                    .display()
                    .to_string(),
                stage: str_of(&o, "stage").unwrap_or_default().to_owned(),
                kind: str_of(&o, "kind").unwrap_or_default().to_owned(),
                v: o,
            });
        }
    }
    assert!(
        !out.is_empty(),
        "no fixtures found under {}",
        protocol_dir().display()
    );
    out
}

fn str_of<'a>(o: &'a JsonObject, name: &str) -> Option<&'a str> {
    o.get(name).and_then(Json::as_str)
}

fn obj<'a>(o: &'a JsonObject, name: &str) -> &'a JsonObject {
    o.get(name)
        .and_then(Json::as_object)
        .unwrap_or_else(|| panic!("member {name} is not an object"))
}

fn uint(v: &Json) -> u64 {
    v.as_number()
        .and_then(|n| n.plain_integer_in(0, u64::MAX))
        .expect("a plain integer")
}

fn base64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => panic!("not base64: {c}"),
    };
    let s: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
    let mut out = Vec::new();
    for chunk in s.chunks(4) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |a, (k, &c)| a | (u32::from(val(c)) << (18 - 6 * k)));
        for k in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * k)) as u8);
        }
    }
    out
}

/// The exact octets of an envelope-stage `input` (§3.3): `envelope` (a JSON value, written
/// compactly with each number as spelled), `envelope_text` or `envelope_base64`.
fn input_octets(input: &JsonObject) -> Vec<u8> {
    if let Some(v) = input.get("envelope") {
        v.to_compact().into_bytes()
    } else if let Some(t) = str_of(input, "envelope_text") {
        t.as_bytes().to_vec()
    } else if let Some(b) = str_of(input, "envelope_base64") {
        base64_decode(b)
    } else {
        panic!("input holds none of envelope, envelope_text, envelope_base64")
    }
}

fn limits(context: &JsonObject) -> (EnvelopeLimits, Timestamp) {
    let now = Timestamp::parse(str_of(context, "receiver_time").unwrap()).unwrap();
    (envelope_limits(context), now)
}

/// The receiver-wide limits of a fixture's `context` (§3.3).
fn envelope_limits(context: &JsonObject) -> EnvelopeLimits {
    let majors = context
        .get("supported_major_versions")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(|m| u16::try_from(uint(m)).unwrap())
        .collect();
    let mut l = EnvelopeLimits::default()
        .with_supported_majors(majors)
        .expect("§3.3: supported_major_versions is not empty");
    if let Some(m) = context.get("max_envelope_octets") {
        l = l
            .with_max_envelope_octets(uint(m))
            .expect("§3.3: max_envelope_octets from 65536 to 9007199254740991");
    }
    l
}

fn run_envelope(fx: &Fixture) -> Result<(), String> {
    let (l, now) = limits(obj(&fx.v, "context"));
    let octets = input_octets(obj(&fx.v, "input"));
    let expected = obj(&fx.v, "expected");
    let result = str_of(expected, "result").unwrap();
    match receive_envelope(&octets, &l, &now) {
        Ok(msg) => {
            if result != "valid" {
                return Err(format!(
                    "accepted; expected {result} {:?}",
                    str_of(expected, "error")
                ));
            }
            let env = msg.envelope();
            if msg.verified_by().is_some() {
                return Err("verified_by set before the security stage".into());
            }
            // [SC-ENV-080]: the trusted values are the top-level `security` member's.
            if let Some(ts) = expected.get("trusted_security").and_then(Json::as_object) {
                let s = env.security();
                let got = [
                    ("principal", s.principal()),
                    ("key_id", s.key_id()),
                    ("nonce", s.nonce()),
                    ("signature", s.signature()),
                ];
                if ts.len() != 4 || got.iter().any(|(k, v)| str_of(ts, k) != Some(v)) {
                    return Err(format!(
                        "trusted_security {got:?} differs from the fixture's"
                    ));
                }
            }
            // Round trip ([IFC-TYP-040]): the octets are kept as received, and writing the
            // kept members back reads as the same envelope, unrecognized members included.
            if env.octets() != octets.as_slice() {
                return Err("received octets not kept".into());
            }
            let rewritten = Json::Object(env.as_json().clone())
                .to_compact()
                .into_bytes();
            let again = receive_envelope(&rewritten, &l, &now)
                .map_err(|e| format!("rewritten envelope refused: {e}"))?;
            let a = again.envelope();
            if a.as_json() != env.as_json()
                || a.content() != env.content()
                || a.security() != env.security()
                || a.id() != env.id()
                || a.ttl_ms() != env.ttl_ms()
            {
                return Err("rewritten envelope reads differently".into());
            }
            if a.signing_input().map_err(|e| e.to_string())?
                != env.signing_input().map_err(|e| e.to_string())?
            {
                return Err("signing input changed on rewrite".into());
            }
            Ok(())
        }
        Err(rej) => {
            let want_error = str_of(expected, "error");
            if rej.state.as_str() != result || want_error != Some(rej.error.as_str()) {
                return Err(format!(
                    "got {} {} ({}); expected {result} {want_error:?}",
                    rej.state, rej.error, rej.requirement
                ));
            }
            Ok(())
        }
    }
}

fn run_receipt(fx: &Fixture) -> Result<(), String> {
    let r = obj(&fx.v, "input").get("receipt").unwrap();
    let expected = obj(&fx.v, "expected");
    match (
        DeliveryReceipt::from_json(r),
        str_of(expected, "result").unwrap(),
    ) {
        (Ok(rec), "valid") => {
            let want =
                str_of(expected, "effective_state").map(|s| DeliveryState::parse(s).unwrap());
            if want.is_some_and(|w| w != rec.effective_state()) {
                return Err(format!(
                    "effective state {} differs from {want:?}",
                    rec.effective_state()
                ));
            }
            let back = DeliveryReceipt::from_octets(&rec.to_octets())
                .map_err(|e| format!("round trip refused: {e:?}"))?;
            if back != rec {
                return Err("receipt does not round-trip".into());
            }
            Ok(())
        }
        (Err(_), "discarded") => Ok(()),
        (got, want) => Err(format!("got {got:?}, expected {want}")),
    }
}

fn implemented(context: &JsonObject) -> Vec<Implemented> {
    context
        .get("implemented")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(|i| {
            let i = i.as_object().unwrap();
            Implemented {
                extension: str_of(i, "extension").unwrap().to_owned(),
                major: u16::try_from(uint(i.get("major").unwrap())).unwrap(),
                revision: Version::parse(str_of(i, "revision").unwrap()).unwrap(),
            }
        })
        .collect()
}

fn run_negotiation(fx: &Fixture) -> Result<(), String> {
    let caps = SessionCapabilities::from_json(obj(&fx.v, "input").get("declaration").unwrap());
    let expected = obj(&fx.v, "expected");
    match (
        caps.agree(&implemented(obj(&fx.v, "context"))),
        str_of(expected, "result").unwrap(),
    ) {
        (None, "no-common-version") => Ok(()),
        (Some(a), "agreed")
            if Some(a.extension.as_str()) == str_of(expected, "extension")
                && u64::from(a.major) == uint(expected.get("major").unwrap()) =>
        {
            // The agreed entry round-trips through its own wire form.
            let back = SessionCapabilities::from_json(
                &json::parse(caps.as_json().to_compact().as_bytes()).unwrap(),
            );
            if back.agree(&implemented(obj(&fx.v, "context"))).as_ref() != Some(&a) {
                return Err("declaration does not round-trip".into());
            }
            Ok(())
        }
        (got, want) => Err(format!("got {got:?}, expected {want}")),
    }
}

/// The trusted key set of a fixture's `context.trusted_keys` (`spec/security.md` §3.3).
/// Each entry is added as a paired key; its public key must pass admission ([SEC-KEY-034])
/// and hash to the listed key id ([SEC-KEY-010]). The set's own device key is a fresh one
/// that no fixture names, so it changes no verdict.
fn trusted_keys(context: &JsonObject) -> Result<TrustedKeySet, String> {
    let own = DeviceIdentity::new(DeviceKey::generate(), Token::parse("oac-test-own").unwrap());
    let mut set = TrustedKeySet::new(&own);
    for k in context
        .get("trusted_keys")
        .and_then(Json::as_array)
        .ok_or("context.trusted_keys")?
    {
        let k = k.as_object().ok_or("trusted key is not an object")?;
        let public = PublicKey::from_base64url(str_of(k, "public_key").unwrap())
            .ok_or_else(|| format!("trusted key {:?} refused at admission", k.get("key_id")))?;
        if Some(public.key_id().as_str()) != str_of(k, "key_id") {
            return Err(format!("key id of {:?} differs", k.get("key_id")));
        }
        let principal = Token::parse(str_of(k, "principal").unwrap()).ok_or("principal")?;
        set.add_paired_key(principal, public)
            .map_err(|e| e.to_string())?;
    }
    Ok(set)
}

/// Stage `security` (`spec/security.md` §3.3): envelope-stage validation, then the five
/// steps of Table 7.1 in order, the first failing step's code reported ([SEC-STG-002]).
/// Steps 1 and 2: [`authenticate`], whose `verified_by` must be the entry the envelope
/// names. Step 3: [`check_replay_window`] at `receiver_time`. Step 4:
/// `AuthorizationEngine::authorize_delivery`, with the fixture's sessions, bindings, grants
/// and sent records, on the same clock. Step 5: [`DuplicateStore::admit`] on a store holding
/// `context.duplicate_store`.
fn run_security(fx: &Fixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    if context.contains("max_envelope_octets") {
        return Err("context.max_envelope_octets is not a member of the security stage".into());
    }
    let (l, now) = limits(context);
    let octets = input_octets(obj(&fx.v, "input"));
    let expected = obj(&fx.v, "expected");
    let want = (
        str_of(expected, "result").unwrap(),
        str_of(expected, "error"),
    );
    // The trusted keys pass admission and hash to their listed ids (checked here as in
    // F3); the engine below pairs the same keys.
    trusted_keys(context)?;
    let clock = Arc::new(ManualClock::new(now));
    let (mut engine, _) = authorization::security_engine(context, clock.clone());
    let store = DuplicateStore::new(clock.clone());
    // The fixture gives an entry no deadline; it is live until the window has passed.
    let until = Timestamp::from_unix_nanos(
        clock.now().unix_nanos() + i128::from(REPLAY_WINDOW_MS) * 1_000_000,
    )
    .unwrap();
    for e in context
        .get("duplicate_store")
        .and_then(Json::as_array)
        .ok_or("context.duplicate_store")?
    {
        let e = e
            .as_object()
            .ok_or("duplicate_store entry is not an object")?;
        let key = DuplicateKey::new(
            str_of(e, "key_id").ok_or("entry key_id")?,
            str_of(e, "nonce").ok_or("entry nonce")?,
        );
        store
            .insert_handed_off(key, &until)
            .map_err(|e| e.to_string())?;
    }
    let before = store.len();
    let steps = security_steps(&octets, &l, &mut engine, &clock.now(), &store, || {
        Some("an entry is in flight with no earlier copy".into())
    })?;
    let got = (steps.state.as_str(), steps.error.as_deref());
    if got != want {
        return Err(format!("got {got:?}; expected {want:?}"));
    }
    authorization::check_step_4_record(expected, &engine, steps.finding)?;
    match steps.reservation {
        Some(r) => {
            if store.len() != before + 1 {
                return Err("a passed envelope added no store entry".into());
            }
            r.handed_off();
        }
        None if store.len() != before => {
            return Err("a refused envelope changed the duplicate store".into());
        }
        None => {}
    }
    Ok(())
}

/// What the receiver did with one copy, up to the end of the security stage.
struct Steps {
    state: String,
    error: Option<String>,
    /// For `duplicate`: whether the copy may draw the entry's one receipt ([SEC-RPL-030]).
    receipt_allowed: Option<bool>,
    /// For a copy that passed all five steps: the entry step 5 added, in flight.
    reservation: Option<Reservation>,
    /// For a copy that passed all five steps: the message, for the hand-off record.
    message: Option<ChannelMessage>,
    /// Whether step 4 recorded a finding ([SEC-PRS-004]).
    finding: bool,
}

impl Steps {
    fn refused(state: &str, error: &str) -> Steps {
        Steps {
            state: state.to_owned(),
            error: Some(error.to_owned()),
            receipt_allowed: None,
            reservation: None,
            message: None,
            finding: false,
        }
    }

    /// Whether the copy reached step 5.
    fn reached_step_5(&self) -> bool {
        self.reservation.is_some() || self.state == "duplicate"
    }
}

/// Envelope stage, then Table 7.1, for one copy arriving at `now`, which is also the time
/// `engine`'s clock reads. `in_flight` is called when step 5 finds the entry of an earlier
/// copy whose hand-off is still running: it must let that hand-off return and give `None`,
/// after which step 5 decides ([SEC-RPL-026]); `Some` is a runner error.
fn security_steps(
    octets: &[u8],
    l: &EnvelopeLimits,
    engine: &mut AuthorizationEngine,
    now: &Timestamp,
    store: &DuplicateStore,
    in_flight: impl FnOnce() -> Option<String>,
) -> Result<Steps, String> {
    let msg = match receive_envelope(octets, l, now) {
        Ok(msg) => msg,
        Err(rej) => return Ok(Steps::refused(rej.state.as_str(), rej.error.as_str())),
    };
    // Steps 1 and 2.
    let msg: ChannelMessage = match authenticate(msg, engine.trusted_keys()) {
        Ok(msg) => msg,
        Err(rej) => return Ok(Steps::refused(rej.state.as_str(), rej.error.as_str())),
    };
    let sec = msg.envelope().security();
    let by = msg.verified_by().ok_or("verified_by not set")?;
    if (by.principal().as_str(), by.key_id().as_str()) != (sec.principal(), sec.key_id()) {
        return Err(format!(
            "verified_by {by:?} is not the entry the envelope names"
        ));
    }
    // Step 3.
    if let Err(rej) = check_replay_window(&msg, now) {
        return Ok(Steps::refused(rej.state.as_str(), rej.error.as_str()));
    }
    // Step 4. A refused copy has no `AuthorizedMessage`, so it cannot reach step 5.
    let msg = match engine.authorize_delivery(msg) {
        Ok(authorized) => authorized,
        Err(rej) => {
            let mut steps = Steps::refused(rej.state.as_str(), rej.error.as_str());
            steps.finding = rej.finding.is_some();
            return Ok(steps);
        }
    };
    if msg.decision().basis().is_none() || !msg.decision().permits(Kind::Deliver) {
        return Err("step 4 passed without a deliver basis".into());
    }
    // Step 5.
    let admission = match store.try_admit(&msg).map_err(|e| e.to_string())? {
        Some(a) => a,
        None => {
            if let Some(e) = in_flight() {
                return Err(e);
            }
            store.admit(&msg).map_err(|e| e.to_string())?
        }
    };
    Ok(match admission {
        Admission::Admitted(r) => Steps {
            state: "passed".into(),
            error: None,
            receipt_allowed: None,
            reservation: Some(r),
            message: Some(msg.into_message()),
            finding: false,
        },
        Admission::Duplicate { receipt_allowed } => Steps {
            state: "duplicate".into(),
            error: Some("duplicate".into()),
            receipt_allowed: Some(receipt_allowed),
            reservation: None,
            message: None,
            finding: false,
        },
    })
}

/// The state and code a receiver reports for a copy that passed the security stage, by
/// the arrival's `delivery` outcome (`spec/security.md` §3.3, stage `replay`), and whether
/// the copy counts as handed off for the duplicate store ([SEC-RPL-022]).
type Outcome = (&'static str, Option<&'static str>, bool);

fn delivery_outcome(delivery: &str) -> Result<Outcome, String> {
    Ok(match delivery {
        "handed-to-harness" => ("handed-to-harness", None, true),
        "unknown" => ("unknown", None, true),
        "destination-unavailable" => ("unreachable", Some("destination-unavailable"), false),
        "handoff-failed" => ("failed", Some("handoff-failed"), false),
        other => return Err(format!("unknown delivery outcome {other}")),
    })
}

/// Settles an admitted copy's reservation: its hand-off call has returned.
fn settle(pending: Option<(Reservation, bool)>) {
    if let Some((r, handed_off)) = pending {
        if handed_off {
            r.handed_off();
        } else {
            r.not_handed_off();
        }
    }
}

/// Stage `replay` (`spec/security.md` §3.3): arrivals at one receiver, in order, from an
/// empty duplicate store, on a [`ManualClock`] set to each arrival's `at`.
///
/// An admitted copy's hand-off call returns just before the next arrival, unless that
/// arrival is `during_previous_handoff`: then step 5 must find the earlier copy's entry in
/// flight and leave the copy undecided ([`DuplicateStore::try_admit`] gives `None`), and it
/// decides only once the earlier call has returned ([SEC-RPL-026]).
fn run_replay(fx: &Fixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    if context.get("replay_window_ms").map(uint) != Some(REPLAY_WINDOW_MS) {
        return Err("context.replay_window_ms is not 300000 ([SEC-RPL-001])".into());
    }
    let l = envelope_limits(context);
    trusted_keys(context)?;
    let input = obj(&fx.v, "input");
    let envelopes = obj(input, "envelopes");
    let arrivals = input
        .get("arrivals")
        .and_then(Json::as_array)
        .ok_or("input.arrivals")?;
    let results = obj(&fx.v, "expected")
        .get("results")
        .and_then(Json::as_array)
        .ok_or("expected.results")?;
    if arrivals.len() != results.len() {
        return Err("one expected result per arrival".into());
    }
    let first = arrivals
        .first()
        .and_then(Json::as_object)
        .and_then(|a| str_of(a, "at"))
        .and_then(Timestamp::parse)
        .ok_or("no arrival")?;
    let clock = Arc::new(ManualClock::new(first));
    let store = DuplicateStore::new(clock.clone());
    // One engine across the arrivals: a copy that passes binds `from` before step 5, and
    // `grants_add` adds to it.
    let (mut engine, grant_store) = authorization::security_engine(context, clock.clone());
    let mut pending: Option<(Reservation, bool)> = None;
    for (i, (a, want)) in arrivals.iter().zip(results).enumerate() {
        let (a, want) = (
            a.as_object().ok_or("arrival")?,
            want.as_object().ok_or("result")?,
        );
        let at = str_of(a, "at")
            .and_then(Timestamp::parse)
            .ok_or("arrival at")?;
        let during = a.get("during_previous_handoff") == Some(&Json::Bool(true));
        if !during {
            settle(pending.take());
        }
        clock.set(at.clone());
        authorization::add_grants(&mut engine, &grant_store, a.get("grants_add"));
        let label = str_of(a, "envelope").ok_or("arrival envelope")?;
        let octets = envelopes
            .get(label)
            .ok_or_else(|| format!("no envelope {label}"))?
            .to_compact()
            .into_bytes();
        let want_error = str_of(want, "error");
        let had_earlier = pending.is_some();
        let mut waited = false;
        let before = store.len();
        let steps = security_steps(&octets, &l, &mut engine, &at, &store, || {
            waited = true;
            if had_earlier {
                settle(pending.take());
                None
            } else {
                Some("an entry is in flight with no earlier hand-off running".into())
            }
        })
        .map_err(|e| format!("arrival {i}: {e}"))?;
        // A copy refused before step 5 leaves the earlier call running; it returns now.
        settle(pending.take());
        // A copy that reached step 5 while an earlier hand-off was running must have waited
        // for it ([SEC-RPL-026]).
        if had_earlier && steps.reached_step_5() && !waited {
            return Err(format!(
                "arrival {i}: decided while the earlier hand-off was running"
            ));
        }
        // A copy refused at step 4, or earlier, adds no store entry ([SEC-RPL-022]).
        if !steps.reached_step_5() && store.len() > before {
            return Err(format!(
                "arrival {i}: a refused copy changed the duplicate store"
            ));
        }
        let (state, error, receipt) = match (steps.reservation, steps.message) {
            (Some(r), Some(msg)) => {
                let delivery = str_of(a, "delivery")
                    .ok_or_else(|| format!("arrival {i}: passed, but no delivery outcome"))?;
                let (state, error, handed_off) = delivery_outcome(delivery)?;
                // [SEC-AUZ-016]: the receiving session may now discover the sender.
                engine.record_handoff(
                    HandOffRecord::of(msg.envelope()),
                    DeliveryState::parse(state).unwrap(),
                );
                pending = Some((r, handed_off));
                (state.to_owned(), error.map(str::to_owned), None)
            }
            _ => (steps.state, steps.error, steps.receipt_allowed),
        };
        let want_receipt = match want.get("duplicate_receipt_allowed") {
            Some(Json::Bool(b)) => Some(*b),
            _ => None,
        };
        let want_all = (str_of(want, "result"), want_error, want_receipt);
        if (Some(state.as_str()), error.as_deref(), receipt) != want_all {
            return Err(format!(
                "arrival {i}: got ({state}, {error:?}, receipt {receipt:?}); expected {want_all:?}"
            ));
        }
    }
    settle(pending);
    Ok(())
}

/// Stage `key-id` (`spec/security.md` §5.2).
fn run_key_id(fx: &Fixture) -> Result<(), String> {
    let public = PublicKey::from_base64url(str_of(obj(&fx.v, "input"), "public_key").unwrap())
        .ok_or("public key refused at admission")?;
    let want = str_of(obj(&fx.v, "expected"), "key_id").unwrap();
    if public.key_id().as_str() != want {
        return Err(format!("key id {}, expected {want}", public.key_id()));
    }
    Ok(())
}

/// Stage `registration` (`spec/security.md` §5.4).
fn run_registration(fx: &Fixture) -> Result<(), String> {
    let keys = trusted_keys(obj(&fx.v, "context"))?;
    let record = obj(&fx.v, "input").get("record").unwrap();
    let verified = RegistrationRecord::from_json(record).is_some_and(|r| r.verify(&keys).is_ok());
    let want = str_of(obj(&fx.v, "expected"), "result").unwrap();
    match (verified, want) {
        (true, "verified") | (false, "invalid") => Ok(()),
        (got, _) => Err(format!("verified: {got}; expected {want}")),
    }
}

fn run_canonical(fx: &Fixture, want: &str) -> Result<(), String> {
    let input = obj(&fx.v, "input");
    if input.len() != 1 {
        return Err("input with more than one member".into());
    }
    let (_, signed) = input.iter().next().unwrap();
    let parsed;
    let signed = match signed {
        Json::String(text) => {
            parsed = json::parse(text.as_bytes()).map_err(|e| e.to_string())?;
            &parsed
        }
        v => v,
    };
    let got = signed_text(signed.as_object().ok_or("signed object is not an object")?)
        .map_err(|e| e.to_string())?;
    if got != want {
        return Err(format!(
            "canonical form differs:\n got  {got}\n want {want}"
        ));
    }
    Ok(())
}

#[test]
fn conformance_fixtures() {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut passed: BTreeSet<String> = BTreeSet::new();
    let mut failures = Vec::new();
    for fx in fixtures() {
        let outcome = match fx.stage.as_str() {
            "envelope" => Some(run_envelope(&fx)),
            "receipt" => Some(run_receipt(&fx)),
            "negotiation" => Some(run_negotiation(&fx)),
            "presence" => Some(presence_receipts::run_presence(&fx)),
            "discovery" => Some(presence_receipts::run_discovery(&fx)),
            "send" => Some(presence_receipts::run_send(&fx)),
            "routing" => Some(presence_receipts::run_routing(&fx)),
            "receive" => Some(presence_receipts::run_receive(&fx)),
            "combine" => Some(presence_receipts::run_combine(&fx)),
            "reply" => Some(presence_receipts::run_reply(&fx)),
            "correlation" => Some(presence_receipts::run_correlation(&fx)),
            "presence-auth" => Some(presence_receipts::run_presence_auth(&fx)),
            "receipt-auth" => Some(presence_receipts::run_receipt_auth(&fx)),
            "security" => Some(
                run_security(&fx)
                    .and_then(|()| presence_receipts::run_security_through_receive(&fx)),
            ),
            "replay" => Some(run_replay(&fx)),
            "key-id" => Some(run_key_id(&fx)),
            "registration" => Some(run_registration(&fx)),
            "discovery-auth" => Some(authorization::run_discovery_auth(&fx)),
            "key-removal" => Some(authorization::run_key_removal(&fx)),
            "exchange" => Some(authorization::run_exchange(&fx)),
            // Adapter-side stages (Epic G): session binding (`binding`, `mcp-binding`) and
            // provenance rendering (`provenance`, `body`). The reference runner
            // (`tests/protocol/runner/`, CI on every OS) evaluates them from the spec text.
            "binding" | "mcp-binding" | "provenance" | "body" => None,
            // A stage nobody named fails, rather than its fixtures silently not running.
            other => Some(Err(format!(
                "stage `{other}` is neither run here nor listed as owned elsewhere"
            ))),
        };
        if let Some(r) = outcome {
            *counts
                .entry(format!("{} {}", fx.stage, fx.kind))
                .or_default() += 1;
            if r.is_ok() {
                passed.insert(fixture_name(&fx.path).to_owned());
            }
            if let Err(e) = r {
                failures.push(format!("{}: {e}", fx.path));
            }
        }
        if let Some(want) = str_of(obj(&fx.v, "expected"), "canonical") {
            *counts.entry("canonical".into()).or_default() += 1;
            if let Err(e) = run_canonical(&fx, want) {
                failures.push(format!("{} (canonical): {e}", fx.path));
            }
        }
    }
    eprintln!("conformance fixtures run: {counts:?}");
    for stage in [
        "envelope positive",
        "envelope negative",
        "receipt positive",
        "receipt negative",
        "negotiation positive",
        "negotiation negative",
        "presence positive",
        "presence negative",
        "discovery positive",
        "discovery negative",
        "send positive",
        "send negative",
        "routing positive",
        "routing negative",
        "receive positive",
        "receive negative",
        "combine positive",
        "combine negative",
        "reply positive",
        "reply negative",
        "correlation positive",
        "correlation negative",
        "presence-auth positive",
        "presence-auth negative",
        "receipt-auth positive",
        "receipt-auth negative",
        "security positive",
        "security negative",
        "replay positive",
        "replay negative",
        "key-id positive",
        "registration positive",
        "registration negative",
        "discovery-auth positive",
        "discovery-auth negative",
        "key-removal positive",
        "exchange positive",
        "exchange negative",
        "canonical",
    ] {
        assert!(
            counts.get(stage).copied().unwrap_or(0) > 0,
            "no {stage} fixture ran"
        );
    }
    // The two `send`-stage envelope fixtures deferred from F2 (#51, PR #312) to F12 (#61):
    // they must run, and pass, through the core's own send logic, not merely exist.
    for name in SEND_STAGE_PINNED {
        assert!(passed.contains(name), "{name} did not run and pass");
    }
    assert!(
        failures.is_empty(),
        "{} fixture(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The F2 deferrals F12 (#61) checks by name: [SC-ENV-021] and [SC-ENV-066].
const SEND_STAGE_PINNED: [&str; 2] = [
    "SC-ENV-021.p01-version-not-lowered-to-peer.json",
    "SC-ENV-066.n01-type-listed-only-under-unimplemented-identifier.json",
];

/// A fixture's file name, whichever separator the platform's path display used.
fn fixture_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn base64url_decode(s: &str) -> Vec<u8> {
    base64_decode(&s.replace('-', "+").replace('_', "/"))
}

/// `spec/security.md` §6.3 states that `VerifyingKey::verify_strict` of `ed25519-dalek`
/// 3.0.0 meets [SEC-SIG-020] to [SEC-SIG-024] on every `sec-sig` fixture, UNVERIFIED until a
/// Rust build ran them. This runs them: for every `sec-sig` fixture whose signature and
/// nonce have the forms of [SEC-SIG-003] and [SEC-SIG-004] (the form fixtures are a
/// separate check, made before any arithmetic), the trusted key the envelope names is
/// decoded with `VerifyingKey::from_bytes` and the signature checked with `verify_strict`
/// alone, over this crate's signing input. Its verdict must be the fixture's:
/// `signature-invalid` exactly when it rejects. Each trusted key must also pass admission
/// ([SEC-KEY-034]), which holds [SEC-SIG-023]'s canonical-encoding half.
///
/// This is evidence about the library. The production path, `oac_core::signing`, is guarded
/// by [`conformance_fixtures`] (stage `security`, every `sec-sig` fixture through
/// [`authenticate`]) and by `signing.rs`'s unit tests, which build an unreduced scalar and
/// small-order `R` values. Of the fixtures, `SEC-SIG-022.n01` (`R` the identity) is the one
/// the library's non-strict `verify` also accepts, so it is what tells the two calls apart.
#[test]
fn verify_strict_alone_gives_the_sec_sig_verdicts() {
    let mut ran = Vec::new();
    for fx in fixtures() {
        if !fx.path.starts_with("sec-sig") || fx.stage != "security" {
            continue;
        }
        let context = obj(&fx.v, "context");
        let (l, now) = limits(context);
        let msg = receive_envelope(&input_octets(obj(&fx.v, "input")), &l, &now)
            .unwrap_or_else(|e| panic!("{}: envelope stage {e}", fx.path));
        let env = msg.envelope();
        let sec = env.security();
        let form = |s: &str, n: usize, last: &str| {
            s.len() == n
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                && last.contains(&s[n - 1..])
        };
        if !form(sec.nonce(), 22, "AQgw") || !form(sec.signature(), 86, "AQgw") {
            continue; // SEC-SIG-003.n*, SEC-SIG-004.n01: refused on form.
        }
        let key = context
            .get("trusted_keys")
            .and_then(Json::as_array)
            .unwrap()
            .iter()
            .map(|k| k.as_object().unwrap())
            .find(|k| {
                str_of(k, "principal") == Some(sec.principal())
                    && str_of(k, "key_id") == Some(sec.key_id())
            })
            .unwrap_or_else(|| panic!("{}: the envelope names no trusted key", fx.path));
        let public = str_of(key, "public_key").unwrap();
        assert!(
            PublicKey::from_base64url(public).is_some(),
            "{}: trusted key refused at admission",
            fx.path
        );
        let vk =
            ed25519_dalek::VerifyingKey::from_bytes(&base64url_decode(public).try_into().unwrap())
                .unwrap();
        let sig = ed25519_dalek::Signature::from_bytes(
            &base64url_decode(sec.signature()).try_into().unwrap(),
        );
        let input = signing_input(SigningDomain::Envelope, env.as_json()).unwrap();
        let accepted = vk.verify_strict(&input, &sig).is_ok();
        let want_reject = str_of(obj(&fx.v, "expected"), "error") == Some("signature-invalid");
        assert_eq!(
            accepted, !want_reject,
            "{}: verify_strict gives the wrong verdict",
            fx.path
        );
        ran.push(fx.path);
    }
    eprintln!("verify_strict ran {} sec-sig fixtures: {ran:?}", ran.len());
    for id in [
        "SEC-SIG-010.p01",
        "SEC-SIG-021.n01",
        "SEC-SIG-021.n02",
        "SEC-SIG-022.n01",
        "SEC-SIG-022.n02",
        "SEC-SIG-022.n03",
        "SEC-SIG-024.n03",
        "SEC-SIG-024.n04",
        "SEC-SIG-024.n05",
    ] {
        assert!(
            ran.iter().any(|p| p.contains(id)),
            "{id} did not run through verify_strict"
        );
    }
}

/// Table 8.3 of `spec/session-channels.md`, read from the spec text, matches
/// [`oac_core::delivery::ErrorCode`] row for row: code, stage, state and scope.
#[test]
fn error_table_matches_the_spec() {
    use oac_core::delivery::{ErrorCode, Scope, Stage};
    let spec = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../spec/session-channels.md"),
    )
    .unwrap();
    let start = spec
        .find("Table 8.3.\n")
        .or_else(|| spec.find("Table 8.3.\r\n"))
        .expect("Table 8.3 in the spec");
    let mut rows = Vec::new();
    for line in spec[start..].lines().skip(1) {
        let line = line.trim();
        if line.is_empty() {
            if rows.is_empty() {
                continue;
            }
            break;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() < 4 || !cells[0].starts_with('`') {
            continue;
        }
        rows.push((
            cells[0].trim_matches('`').to_owned(),
            cells[1].to_owned(),
            cells[2].trim_matches('`').to_owned(),
            cells[3].to_owned(),
        ));
    }
    assert_eq!(rows.len(), ErrorCode::ALL.len(), "Table 8.3 rows");
    for (row, code) in rows.iter().zip(ErrorCode::ALL) {
        let stage = match code.stage() {
            Stage::Envelope => "envelope",
            Stage::Security => "security",
            Stage::Delivery => "delivery",
            Stage::Request => "request",
            Stage::Any => "any",
        };
        let state = code.state().map_or("none", DeliveryState::as_str);
        let scope: Vec<&str> = code
            .scope()
            .iter()
            .map(|s| match s {
                Scope::Sender => "sender",
                Scope::Receiver => "receiver",
                Scope::Request => "request",
            })
            .collect();
        assert_eq!(
            (
                row.0.as_str(),
                row.1.as_str(),
                row.2.as_str(),
                row.3.as_str()
            ),
            (code.as_str(), stage, state, scope.join(", ").as_str())
        );
    }
}

/// The hand-off-deadline re-check of the `routing` fixtures that test it
/// (`spec/session-channels.md` [SC-RCP-091], [SC-RCP-092]; delivery stage, step 4): the
/// envelope passes the envelope stage at `receiver_time`, and [`HandOffDeadline`] at
/// `handoff_time` gives the expected state and code, or lets it through for `valid`. The
/// rest of the `routing` stage (sessions, availability, capabilities) runs in
/// `presence_receipts::run_routing`; in these fixtures every one of those checks passes.
/// Their signatures are placeholders, since the stage takes the sender's authorization as
/// given, so the security stage is not run.
#[test]
fn handoff_deadline_fixtures() {
    let mut ran = Vec::new();
    for fx in fixtures() {
        let name = fx.path.replace('\\', "/");
        if !(name.starts_with("sc-rcp/SC-RCP-091.") || name.starts_with("sc-rcp/SC-RCP-092.")) {
            continue;
        }
        assert_eq!(fx.stage, "routing", "{name}");
        let context = obj(&fx.v, "context");
        assert_eq!(
            context.get("replay_window_ms").map(uint),
            Some(REPLAY_WINDOW_MS),
            "{name}"
        );
        let (l, now) = limits(context);
        let msg = receive_envelope(&input_octets(obj(&fx.v, "input")), &l, &now)
            .unwrap_or_else(|e| panic!("{name}: envelope stage {e}"));
        let handoff = Timestamp::parse(str_of(context, "handoff_time").unwrap()).unwrap();
        let got = HandOffDeadline::of(msg.envelope())
            .refusal_at(&handoff)
            .map(|(s, e)| (s.as_str(), e.as_str()));
        let expected = obj(&fx.v, "expected");
        let want = match str_of(expected, "result").unwrap() {
            "valid" => None,
            r => Some((r, str_of(expected, "error").unwrap())),
        };
        assert_eq!(got, want, "{name}");
        ran.push(name);
    }
    assert_eq!(ran.len(), 4, "hand-off-deadline fixtures run: {ran:?}");
}
