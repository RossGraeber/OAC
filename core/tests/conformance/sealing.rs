// SPDX-License-Identifier: Apache-2.0

//! The fixtures of `spec/security.md` §14.10, format `oac-sealing-fixture/1`, through this
//! crate's own code (#369): `tests/protocol/sec-sel/`.
//!
//! - `agreement` (§14.3): a consumer built as its `context` says (the trusted keys paired,
//!   the `admitted` statements held, restored through the pairing store as after a restart)
//!   admits the statement through `AuthorizationEngine::admit_statement`, and
//!   `sealing::check_statement` gives the same verdict.
//! - `seal` (§14.4): the sender seals only to a statement its engine holds for the
//!   recipient's key; with none, `refused` with `transport-failure` ([SEC-SEL-023],
//!   [SEC-SEL-024]). A sealed fixture fixes its ephemeral key, which the `hpke` crate's
//!   public API does not take: it draws each ephemeral key itself ([SEC-SEL-021]). So the
//!   fixture's frame is checked against this crate's sealing: its `enc` is the fixture's
//!   ephemeral public key, it opens under the recipient's test key to exactly the plaintext
//!   this crate builds from the input (kind, length, payload, the padding), which, with
//!   `enc`, fixes every octet of the AEAD output; and a frame this crate seals from the same
//!   input has the same length and opens to the same payload.
//! - `open` (§14.5): `sealing::open` under the context's private key, then, when
//!   `own_key_id` is given, `sealing::recipient_of` against an engine holding the context's
//!   `bindings` and `sent` ([SEC-SEL-035]).

use super::{obj, protocol_dir, str_of};
use curve25519_dalek::montgomery::MontgomeryPoint;
use oac_core::authorization::{AuthorizationEngine, MemoryDecisionLog, OperatorConfirmed};
use oac_core::clock::{Clock, ManualClock, SystemClock};
use oac_core::ids::{KeyId, Timestamp, Token};
use oac_core::json::{self, Json, JsonObject};
use oac_core::keys::{DeviceIdentity, DeviceKey, PublicKey};
use oac_core::pairing::{MemoryPairingStore, PairedPeer, PairingStore};
use oac_core::sealing::{
    AgreementPrivateKey, AgreementPublicKey, AgreementStatement, HeldStatement, Recipient,
    check_statement, open, recipient_of, seal_padded,
};
use oac_core::transport::PayloadKind;
use std::collections::BTreeMap;
use std::sync::Arc;

pub(super) struct SealFixture {
    pub(super) path: String,
    pub(super) stage: String,
    pub(super) kind: String,
    v: JsonObject,
}

/// Every `oac-sealing-fixture/1` fixture under `tests/protocol/`, in path order.
pub(super) fn fixtures() -> Vec<SealFixture> {
    let mut out = Vec::new();
    let mut dirs: Vec<_> = std::fs::read_dir(protocol_dir())
        .unwrap()
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
            let v = json::parse(&std::fs::read(&f).unwrap())
                .unwrap_or_else(|e| panic!("{}: {e}", f.display()));
            let o = v.into_object().unwrap();
            if str_of(&o, "fixture_format") != Some("oac-sealing-fixture/1") {
                continue;
            }
            out.push(SealFixture {
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
    out
}

pub(super) fn run(fx: &SealFixture) -> Result<(), String> {
    match fx.stage.as_str() {
        "agreement" => run_agreement(fx),
        "seal" => run_seal(fx),
        "open" => run_open(fx),
        other => Err(format!("unknown oac-sealing-fixture/1 stage `{other}`")),
    }
}

fn b64(s: &str) -> Vec<u8> {
    super::base64url_decode(s)
}

fn key32(s: &str) -> Result<[u8; 32], String> {
    b64(s)
        .try_into()
        .map_err(|_| format!("{s} is not 32 octets"))
}

fn kind_of(s: &str) -> Result<PayloadKind, String> {
    Ok(match s {
        "envelope" => PayloadKind::Envelope,
        "presence" => PayloadKind::Presence,
        "receipt" => PayloadKind::Receipt,
        other => return Err(format!("unknown kind {other}")),
    })
}

/// The test agreement private keys of `tests/protocol/sec-test-keys.json`, by public key.
fn agreement_test_keys() -> BTreeMap<String, String> {
    let v =
        json::parse(&std::fs::read(protocol_dir().join("sec-test-keys.json")).unwrap()).unwrap();
    v.as_object()
        .and_then(|o| o.get("agreement_keys"))
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(|k| {
            let k = k.as_object().unwrap();
            (
                str_of(k, "agreement_key").unwrap().to_owned(),
                str_of(k, "private_key_hex").unwrap().to_owned(),
            )
        })
        .collect()
}

fn hex32(s: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
    }
    out
}

/// The held statements of a context's `admitted`.
fn admitted(context: &JsonObject) -> Result<Vec<HeldStatement>, String> {
    let Some(a) = context.get("admitted") else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (key, h) in a.as_object().ok_or("admitted")?.iter() {
        let h = h.as_object().ok_or("admitted entry")?;
        out.push(
            HeldStatement::from_store(
                KeyId::parse(key).ok_or("admitted key id")?,
                AgreementPublicKey::from_base64url(str_of(h, "agreement_key").unwrap())
                    .ok_or("an admitted agreement key is not acceptable")?,
                h.get("seq")
                    .and_then(Json::as_number)
                    .and_then(|n| n.plain_integer_in(0, u64::MAX))
                    .ok_or("admitted seq")?,
                Timestamp::parse(str_of(h, "issued_at").unwrap()).ok_or("admitted issued_at")?,
            )
            .ok_or("admitted seq out of range")?,
        );
    }
    Ok(out)
}

/// A consumer as a context describes it, on a clock at `now`: the trusted keys paired, and
/// the `admitted` statements of trusted keys held, restored from its pairing store
/// ([SEC-SEL-041]). A held statement for a key the context does not trust cannot be
/// restored at all ([SEC-SEL-015]).
fn consumer(
    context: &JsonObject,
    now: Timestamp,
) -> Result<(AuthorizationEngine, MemoryPairingStore), String> {
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(now.clone()));
    let own = DeviceIdentity::new(DeviceKey::generate(), Token::parse("oac-test-own").unwrap());
    let store = MemoryPairingStore::new();
    let mut e = AuthorizationEngine::new(&own, clock.clone(), Box::new(MemoryDecisionLog::new()));
    let mut trusted = Vec::new();
    for k in context
        .get("trusted_keys")
        .and_then(Json::as_array)
        .ok_or("context.trusted_keys")?
    {
        let k = k.as_object().ok_or("trusted key")?;
        let public = PublicKey::from_base64url(str_of(k, "public_key").unwrap())
            .ok_or("trusted key refused at admission")?;
        let key_id = KeyId::parse(str_of(k, "key_id").unwrap()).ok_or("key id")?;
        let peer = PairedPeer::by_key_id_comparison(
            Token::parse(str_of(k, "principal").unwrap()).ok_or("principal")?,
            public,
            &key_id,
            now.clone(),
            OperatorConfirmed::by_operator(),
        )
        .ok_or("the listed key id is not the key's")?;
        e.pair(peer, &store).map_err(|e| e.to_string())?;
        trusted.push(key_id);
    }
    let mut snapshot = store.load().map_err(|e| e.to_string())?;
    for h in admitted(context)? {
        if trusted.contains(h.key_id()) {
            snapshot.agreement_statements.push(h);
        } else {
            // [SEC-SEL-015]: a statement for an untrusted key is never held, even restored.
            let mut with = snapshot.clone();
            with.agreement_statements.push(h);
            let probe = MemoryPairingStore::new();
            probe.save(&with).map_err(|e| e.to_string())?;
            if AuthorizationEngine::restore(
                &own,
                &probe,
                clock.clone(),
                Box::new(MemoryDecisionLog::new()),
            )
            .is_ok()
            {
                return Err("a statement for an untrusted key was restored".into());
            }
        }
    }
    store.save(&snapshot).map_err(|e| e.to_string())?;
    let e = AuthorizationEngine::restore(&own, &store, clock, Box::new(MemoryDecisionLog::new()))
        .map_err(|e| e.to_string())?;
    Ok((e, store))
}

/// Stage `agreement` (§14.3).
fn run_agreement(fx: &SealFixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    let now = Timestamp::parse(str_of(context, "consumer_time").ok_or("consumer_time")?)
        .ok_or("consumer_time")?;
    let (mut e, store) = consumer(context, now.clone())?;
    let statement = AgreementStatement::from_json(
        obj(&fx.v, "input")
            .get("statement")
            .ok_or("input.statement")?,
    )
    .ok_or("statement is not an object")?;
    let want = str_of(obj(&fx.v, "expected"), "result").ok_or("expected.result")?;
    let direct = check_statement(
        &statement,
        e.trusted_keys(),
        |k| e.held_statement(k).cloned(),
        &now,
    );
    let got = e.admit_statement(&statement, &store).cloned();
    if direct.is_ok() != got.is_ok() {
        return Err(format!("check_statement {direct:?}, the engine {got:?}"));
    }
    match (want, &got) {
        ("admitted", Ok(h)) => {
            if !store
                .load()
                .map_err(|e| e.to_string())?
                .agreement_statements
                .contains(h)
            {
                return Err("admitted, but not saved ([SEC-SEL-041])".into());
            }
            Ok(())
        }
        ("refused", Err(_)) => Ok(()),
        (w, g) => Err(format!("expected {w}, got {g:?}")),
    }
}

/// Stage `seal` (§14.4); see the module documentation.
fn run_seal(fx: &SealFixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    let input = obj(&fx.v, "input");
    let expected = obj(&fx.v, "expected");
    let (e, _) = consumer(context, SystemClock.now())?;
    let recipient = KeyId::parse(str_of(input, "recipient_key_id").ok_or("recipient")?)
        .ok_or("recipient key id")?;
    let want = (
        str_of(expected, "result").ok_or("expected.result")?,
        str_of(expected, "error"),
    );
    // [SEC-SEL-023]: only a statement the engine holds for that key; [SEC-SEL-024]: none.
    let Some(held) = e.held_statement(&recipient) else {
        return match want {
            ("refused", Some("transport-failure")) => Ok(()),
            w => Err(format!("no statement held; expected {w:?}")),
        };
    };
    if want.0 != "sealed" {
        return Err(format!("a statement is held; expected {want:?}"));
    }
    let kind = kind_of(str_of(input, "kind").ok_or("kind")?)?;
    let payload = b64(str_of(input, "payload").ok_or("payload")?);
    let padding = input
        .get("padding")
        .and_then(Json::as_number)
        .and_then(|n| n.plain_integer_in(0, u64::from(u32::MAX)))
        .ok_or("padding")? as usize;
    let eph = key32(str_of(input, "ephemeral_private_key").ok_or("ephemeral key")?)?;
    let frame = b64(str_of(expected, "frame").ok_or("expected.frame")?);
    let private_hex = agreement_test_keys()
        .get(&held.agreement_key().to_base64url())
        .cloned()
        .ok_or("the held agreement key is not a test key")?;
    let private = AgreementPrivateKey::from_octets(&mut hex32(&private_hex));
    // The frame: version, the fixture's ephemeral public key, and the length the plaintext
    // of Table 14.1 gives.
    if frame.len() != 1 + 32 + 5 + payload.len() + padding + 16 || frame[0] != 0x01 {
        return Err(format!("frame length {} or version", frame.len()));
    }
    if frame[1..33] != MontgomeryPoint::mul_base_clamped(eph).to_bytes() {
        return Err("enc is not the ephemeral key's public key".into());
    }
    let opened = open(&private, &frame).ok_or("the fixture's frame does not open here")?;
    if opened.kind != kind || opened.payload.as_slice() != payload.as_slice() {
        return Err("the fixture's frame opens to another plaintext".into());
    }
    // This crate's own frame for the same input.
    let ours =
        seal_padded(held.agreement_key(), kind, &payload, padding).map_err(|e| e.to_string())?;
    if ours.len() != frame.len() {
        return Err(format!(
            "our frame is {} octets, the fixture's {}",
            ours.len(),
            frame.len()
        ));
    }
    let back = open(&private, &ours).ok_or("our frame does not open")?;
    if back.kind != kind || back.payload.as_slice() != payload.as_slice() {
        return Err("our frame opens to another plaintext".into());
    }
    Ok(())
}

/// Stage `open` (§14.5).
fn run_open(fx: &SealFixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    let expected = obj(&fx.v, "expected");
    let private = AgreementPrivateKey::from_octets(&mut key32(
        str_of(context, "own_agreement_private_key").ok_or("own private key")?,
    )?);
    let frame = b64(str_of(obj(&fx.v, "input"), "frame").ok_or("input.frame")?);
    let want = str_of(expected, "result").ok_or("expected.result")?;
    let discarded = || match (want, str_of(expected, "record")) {
        ("discarded", Some("none")) => Ok(()),
        w => Err(format!("discarded; expected {w:?}")),
    };
    // [SEC-SEL-030], [SEC-SEL-031].
    let Some(opened) = open(&private, &frame) else {
        return discarded();
    };
    // [SEC-SEL-035].
    if let Some(own) = str_of(context, "own_key_id") {
        let own = super::authorization::own_identity(Some(own));
        let (e, _) = super::authorization::engine(
            &own,
            context,
            Arc::new(ManualClock::new(SystemClock.now())),
        );
        if recipient_of(opened.kind, &opened.payload, &e) == Recipient::Another {
            return discarded();
        }
    }
    let want_kind = str_of(expected, "kind").unwrap_or_default();
    let want_payload = b64(str_of(expected, "payload").unwrap_or_default());
    if want != "opened"
        || opened.kind.as_str() != want_kind
        || opened.payload.as_slice() != want_payload.as_slice()
    {
        return Err(format!(
            "opened as {}; expected {want} {want_kind}",
            opened.kind.as_str()
        ));
    }
    Ok(())
}
