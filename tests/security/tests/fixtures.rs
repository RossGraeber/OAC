// SPDX-License-Identifier: Apache-2.0

//! Signature forms (`spec/security.md` §13 "Signature malleability and weak or mixed-order
//! points", "Cross-protocol reuse", "Canonicalization mismatch between signer and
//! verifier"). These attacks need signatures no device key can make through the public API:
//! a non-reduced scalar, a small-order or mixed-order point, a signature under another
//! domain string, a signing input in another member order. The recorded conformance fixtures
//! under `tests/protocol/` carry exactly such envelopes, signed with the published test keys
//! (`tests/protocol/sec-test-keys.json`). Here each one arrives, as its octets, at a
//! receiving device that paired the fixture's trusted keys, and goes through the same receive
//! path as every other copy in this suite.

use std::path::PathBuf;

use oac_core::delivery::{DeliveryState, ErrorCode};
use oac_core::ids::Timestamp;
use oac_core::json::{self, Json, JsonObject};
use oac_core::keys::{DeviceIdentity, DeviceKey, PublicKey, SecretSeed};
use oac_core::registration::RegistrationRecord;
use oac_security_suite::{Device, token};
use std::sync::Arc;

fn protocol_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../protocol")
}

fn load(rel: &str) -> JsonObject {
    let path = protocol_dir().join(rel);
    let octets = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    json::parse(&octets)
        .expect("a fixture is JSON")
        .into_object()
        .expect("a fixture is an object")
}

fn obj<'a>(o: &'a JsonObject, name: &str) -> &'a JsonObject {
    o.get(name)
        .and_then(Json::as_object)
        .unwrap_or_else(|| panic!("no object {name}"))
}

fn str_of<'a>(o: &'a JsonObject, name: &str) -> &'a str {
    o.get(name)
        .and_then(Json::as_str)
        .unwrap_or_else(|| panic!("no string {name}"))
}

/// A receiving device at the fixture's receiver time that paired every trusted key of its
/// context, and the envelope's octets as they arrive.
fn arrive(fx: &JsonObject) -> (Device, Vec<u8>) {
    let ctx = obj(fx, "context");
    let clock = Arc::new(oac_core::clock::ManualClock::new(
        Timestamp::parse(str_of(ctx, "receiver_time")).unwrap(),
    ));
    let mut dev = Device::new("receiver", clock);
    for k in ctx.get("trusted_keys").and_then(Json::as_array).unwrap() {
        let k = k.as_object().unwrap();
        let pk = PublicKey::from_base64url(str_of(k, "public_key")).expect("a public key");
        assert_eq!(pk.key_id().as_str(), str_of(k, "key_id"));
        dev.pair_key(str_of(k, "principal"), pk);
    }
    let input = obj(fx, "input");
    let octets = match input.get("envelope") {
        Some(env) => env.to_compact().into_bytes(),
        None => str_of(input, "envelope_text").as_bytes().to_vec(),
    };
    (dev, octets)
}

fn signing_key_id(octets: &[u8]) -> String {
    let env = json::parse(octets).unwrap().into_object().unwrap();
    str_of(obj(&env, "security"), "key_id").to_owned()
}

/// Each fixture must be refused at security step 2 with `signature-invalid`, nothing handed
/// off.
fn assert_all_rejected(fixtures: &[&str]) {
    for rel in fixtures {
        let fx = load(rel);
        assert_eq!(str_of(obj(&fx, "expected"), "result"), "rejected", "{rel}");
        let (mut dev, octets) = arrive(&fx);
        let d = dev.receive(&octets);
        assert_eq!(
            d.outcome(),
            (DeliveryState::Rejected, Some(ErrorCode::SignatureInvalid)),
            "{rel}"
        );
        assert!(d.handed.is_none() && d.verified_by.is_none(), "{rel}");
    }
}

/// `spec/security.md` §13 "Signature malleability and weak or mixed-order points"
/// ([SEC-SIG-021] to [SEC-SIG-024]): a scalar that is not reduced, a small-order or
/// non-canonical `R`, and a mixed-order `R` or public key are each refused, so only a
/// strict, cofactorless verifier passes this test.
#[test]
fn s13_malleable_and_weak_point_signatures_are_rejected() {
    assert_all_rejected(&[
        "sec-sig/SEC-SIG-021.n01-scalar-not-reduced.json",
        "sec-sig/SEC-SIG-021.n02-scalar-equals-l.json",
        "sec-sig/SEC-SIG-022.n01-small-order-r.json",
        "sec-sig/SEC-SIG-022.n02-small-order-r-order-2.json",
        "sec-sig/SEC-SIG-022.n03-non-canonical-r.json",
        "sec-sig/SEC-SIG-024.n04-mixed-order-r.json",
        "sec-sig/SEC-SIG-024.n05-mixed-order-a.json",
    ]);
}

/// `spec/security.md` §13 "Cross-protocol reuse" ([SEC-SIG-010], §6.2): an envelope signed
/// over the right members but under another domain string is refused.
#[test]
fn s13_an_envelope_signed_under_another_domain_is_rejected() {
    assert_all_rejected(&["sec-sig/SEC-SIG-010.n01-wrong-domain-string.json"]);
}

/// `spec/security.md` §13 "Cross-protocol reuse" ([SEC-KEY-041], [SEC-KEY-043]): a
/// registration record signed under the envelope domain does not verify as a registration,
/// so its own signer cannot use the binding; the record signed under the registration domain
/// can.
#[test]
fn s13_a_registration_signed_under_the_envelope_domain_binds_nothing() {
    let keys = load("sec-test-keys.json");
    let signer = |key_id: &str| {
        let k = keys
            .get("keys")
            .and_then(Json::as_array)
            .unwrap()
            .iter()
            .map(|k| k.as_object().unwrap())
            .find(|k| str_of(k, "key_id") == key_id)
            .expect("a published test key");
        let hex = str_of(k, "seed_hex");
        let octets: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let seed = SecretSeed::from_slice(&octets).expect("32 octets");
        DeviceIdentity::new(DeviceKey::from_seed(&seed), token(str_of(k, "principal")))
    };
    for (rel, usable) in [
        (
            "sec-key/SEC-KEY-041.n01-registration-signed-under-envelope-domain.json",
            false,
        ),
        (
            "sec-key/SEC-KEY-041.p01-registration-record-verifies.json",
            true,
        ),
    ] {
        let fx = load(rel);
        let record = obj(obj(&fx, "input"), "record");
        let own = signer(str_of(obj(record, "security"), "key_id"));
        let r = RegistrationRecord::from_json(&Json::Object(record.clone())).expect("a record");
        assert_eq!(r.binding_usable(&own), usable, "{rel}");
    }
}

/// `spec/security.md` §13 "Canonicalization mismatch between signer and verifier"
/// ([SEC-SIG-010], [SEC-SIG-011], [SEC-SIG-013]): envelopes whose members arrive in another
/// order, with UTF-16 member order, escapes and raw characters, unknown members, and
/// number spellings verify, because the verifier canonicalizes what it received; an envelope
/// signed over a non-canonical input, or with a member added or removed after signing, does
/// not.
#[test]
fn s13_canonical_forms_verify_and_altered_forms_do_not() {
    for rel in [
        "sec-sig/SEC-SIG-010.p01-signature-verifies.json",
        "sec-sig/SEC-SIG-010.p02-member-order-irrelevant.json",
        "sec-sig/SEC-SIG-010.p03-utf16-member-order.json",
        "sec-sig/SEC-SIG-010.p04-escapes-and-raw-characters.json",
        "sec-sig/SEC-SIG-011.p01-unknown-member-and-unicode-signed.json",
        "sec-sig/SEC-SIG-013.p01-number-forms.json",
        "sec-sig/SEC-SIG-013.p02-number-not-a-double.json",
    ] {
        let fx = load(rel);
        assert_eq!(str_of(obj(&fx, "expected"), "result"), "passed", "{rel}");
        let (mut dev, octets) = arrive(&fx);
        let d = dev.receive(&octets);
        assert_eq!(
            d.verified_by.as_ref().map(|k| k.as_str().to_owned()),
            Some(signing_key_id(&octets)),
            "{rel}: {d:?}"
        );
    }
    assert_all_rejected(&[
        "sec-sig/SEC-SIG-010.n02-non-canonical-signing-input.json",
        "sec-sig/SEC-SIG-011.n01-unknown-member-removed.json",
        "sec-sig/SEC-SIG-011.n02-member-added-after-signing.json",
    ]);
}
