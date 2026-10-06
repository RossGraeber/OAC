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
//! - `presence` (§7.5): the `discarded` list only. A record is discarded when
//!   [`PresenceRecord::from_json`] refuses it ([SC-DLV-040]) or its `seq` is not above
//!   the latest accepted one for its session ([SC-DLV-042]). The presence states, which
//!   turn on lifetimes and carrier loss, are the presence registry's (task F6).
//! - Every fixture, of any stage, whose `expected` holds `canonical`: the canonical text
//!   of `spec/security.md` §6.2 for its signed object.
//!
//! Other stages (binding, send, routing, security, ...) exercise logic later tasks own.

use oac_core::canonical::signed_text;
use oac_core::capabilities::{Implemented, SessionCapabilities};
use oac_core::delivery::DeliveryState;
use oac_core::envelope::{EnvelopeLimits, receive_envelope};
use oac_core::ids::{Timestamp, Version};
use oac_core::json::{self, Json, JsonObject};
use oac_core::presence::PresenceRecord;
use oac_core::receipt::DeliveryReceipt;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

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
    let majors = context
        .get("supported_major_versions")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(|m| u16::try_from(uint(m)).unwrap())
        .collect();
    let mut l = EnvelopeLimits {
        supported_majors: majors,
        ..EnvelopeLimits::default()
    };
    if let Some(m) = context.get("max_envelope_octets") {
        l.max_envelope_octets = uint(m);
    }
    let now = Timestamp::parse(str_of(context, "receiver_time").unwrap()).unwrap();
    (l, now)
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

fn run_presence(fx: &Fixture) -> Result<(), String> {
    let mut latest: HashMap<String, u64> = HashMap::new();
    let mut discarded = Vec::new();
    for (idx, ev) in obj(&fx.v, "input")
        .get("events")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .enumerate()
    {
        let Some(rec) = ev.as_object().unwrap().get("record") else {
            continue; // a carrier-loss event
        };
        match PresenceRecord::from_json(rec) {
            Err(_) => discarded.push(idx as u64), // [SC-DLV-040]
            Ok(r) => {
                let back = PresenceRecord::from_octets(&r.to_octets())
                    .map_err(|e| format!("event {idx}: round trip refused: {e:?}"))?;
                if back != r {
                    return Err(format!("event {idx}: record does not round-trip"));
                }
                match latest.get(r.session_id().as_str()) {
                    Some(&seq) if r.seq() <= seq => discarded.push(idx as u64), // [SC-DLV-042]
                    _ => {
                        latest.insert(r.session_id().as_str().to_owned(), r.seq());
                    }
                }
            }
        }
    }
    let want: Vec<u64> = obj(&fx.v, "expected")
        .get("discarded")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(uint)
        .collect();
    if discarded != want {
        return Err(format!("discarded {discarded:?}, expected {want:?}"));
    }
    Ok(())
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
    let mut failures = Vec::new();
    for fx in fixtures() {
        let outcome = match fx.stage.as_str() {
            "envelope" => Some(run_envelope(&fx)),
            "receipt" => Some(run_receipt(&fx)),
            "negotiation" => Some(run_negotiation(&fx)),
            "presence" => Some(run_presence(&fx)),
            _ => None,
        };
        if let Some(r) = outcome {
            *counts
                .entry(format!("{} {}", fx.stage, fx.kind))
                .or_default() += 1;
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
        "canonical",
    ] {
        assert!(
            counts.get(stage).copied().unwrap_or(0) > 0,
            "no {stage} fixture ran"
        );
    }
    assert!(
        failures.is_empty(),
        "{} fixture(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
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
