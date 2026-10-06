// SPDX-License-Identifier: Apache-2.0

//! The presence and receipt fixture stages (#55, F6), driven through `oac-core`'s presence
//! registry, send decision, receiver pipeline and receipt state machine:
//!
//! - `presence` (`spec/session-channels.md` §7.5), in full: `discarded`, `states` and
//!   `send`, through [`PresenceRegistry`] on a scripted monotonic clock and
//!   [`check_send`]. Records are taken as authenticated, and the 300-second cap of
//!   [SEC-PRS-007] does not apply (§7.5), so they enter as
//!   [`RecordOrigin::SameImplementation`].
//! - `discovery` (§7.5): [`discovery_result`].
//! - `send` (§6.10): [`check_send`], with a session that `declarations` holds taken as
//!   `online` and any other as `unknown` (§7.5).
//! - `routing` and `receive` (§8.5): envelope stage, security, then the delivery stage.
//!   `receive` runs security steps 1 to 3, the fixture's step-4 verdict (F5, #54, is not
//!   merged; the module note of `conformance.rs`), and [`deliver`] for step 5 and the whole
//!   delivery stage, with a hand-off call that succeeds. `routing` takes authorization from
//!   `authorized` and has placeholder signatures, so it runs [`delivery_checks`] and the
//!   hand-off-deadline re-check, the delivery-stage steps [`deliver`] runs.
//! - `combine` (§8.5): [`combined_state`] and [`retry_allowed`].
//! - `presence-auth` (`spec/security.md` §3.3): [`accept_authenticated_record`] into a
//!   [`PresenceRegistry`], with a binding table and relation test built from the fixture's
//!   `bindings`, `sessions`, `grants`, `sent` and `handed_off` ([`FixtureBindings`]).
//! - `receipt-auth` (`spec/security.md` §3.3): [`accept_receipt`].
//! - `reply` and `correlation` (§8.5): [`reply_headers`] and [`answered`].
//! - `security`, `receipt_permitted` (§10.3): [`may_send_receipt`].
//!
//! [`FixtureBindings`] is the runner's stand-in for F5's binding table and relation test,
//! read from `spec/security.md` §9.2, §9.4 and §11.3; when F5 lands, its engine implements
//! [`ConsumerBindings`] and replaces it.

use super::{
    Fixture, envelope_limits, implemented, input_octets, limits, obj, str_of, trusted_keys, uint,
};
use oac_core::capabilities::{SessionCapabilities, SessionDescriptor};
use oac_core::clock::ManualClock;
use oac_core::delivery::{DeliveryState, ErrorCode, Observer};
use oac_core::envelope::receive_envelope;
use oac_core::ids::{KeyId, SessionId, Timestamp, Token, Version};
use oac_core::json::{self, Json, JsonObject};
use oac_core::presence::{PresenceRecord, PresenceState};
use oac_core::presence_auth::{
    AuthenticatedPresenceRecord, BindingEntry, ConsumerBindings, accept_authenticated_record,
};
use oac_core::receipt_auth::{AuthenticatedReceipt, SentEnvelopes, accept_receipt};
use oac_core::receiver::{
    DeliveryTarget, HandOffOutcome, ReceiptLimiter, Received, ReceiverReport, deliver,
    delivery_checks, may_send_receipt,
};
use oac_core::registry::{PresenceAcceptance, PresenceRegistry, RecordOrigin, discovery_result};
use oac_core::replay::{DuplicateKey, DuplicateStore, HandOffDeadline, REPLAY_WINDOW_MS};
use oac_core::reply::{EnvelopeRecord, answered, reply_headers};
use oac_core::sender::{HeldState, check_send, combined_state, retry_allowed};
use oac_core::signing::authenticate;
use oac_core::transport::CarrierHandle;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The reply period of `spec/security.md` §9.5, in milliseconds.
const REPLY_PERIOD_MS: i128 = 86_400_000;

fn sid(s: &str) -> Result<SessionId, String> {
    SessionId::parse(s).ok_or_else(|| format!("{s} is not a session id"))
}

fn arr<'a>(o: &'a JsonObject, name: &str) -> &'a [Json] {
    o.get(name).and_then(Json::as_array).unwrap_or(&[])
}

fn state_name(s: PresenceState) -> &'static str {
    match s {
        PresenceState::Online => "online",
        PresenceState::Unreachable => "unreachable",
        PresenceState::Unknown => "unknown",
    }
}

fn carrier(label: &str) -> CarrierHandle {
    CarrierHandle::from_opaque(label.as_bytes().to_vec())
}

/// `discoverable` (§7.5): `None` when omitted, meaning every pair passes.
fn discoverable(context: &JsonObject) -> Option<BTreeSet<(String, String)>> {
    context
        .get("discoverable")
        .and_then(Json::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Json::as_object)
                .map(|p| {
                    (
                        str_of(p, "requester").unwrap_or_default().to_owned(),
                        str_of(p, "session").unwrap_or_default().to_owned(),
                    )
                })
                .collect()
        })
}

fn may_discover(pairs: &Option<BTreeSet<(String, String)>>, r: &SessionId, s: &SessionId) -> bool {
    pairs
        .as_ref()
        .is_none_or(|p| p.contains(&(r.as_str().to_owned(), s.as_str().to_owned())))
}

/// The size of the envelope a sender would build for a request, with each header member at
/// its largest plausible length, as the reference runner estimates it (only a request near
/// the limit depends on the estimate).
fn estimate_octets(from: &str, to: &str, version: Version, content: &Json) -> u64 {
    let x = |n| "x".repeat(n);
    let text = format!(
        "{{\"version\":\"{version}\",\"id\":\"{}\",\"from\":\"{from}\",\"to\":\"{to}\",\
         \"created_at\":\"2026-10-03T12:00:00.000000000Z\",\"ttl_ms\":86400000,\"content\":{},\
         \"security\":{{\"principal\":\"{}\",\"key_id\":\"{}\",\"nonce\":\"{}\",\"signature\":\"{}\"}}}}",
        x(128),
        content.to_compact(),
        x(128),
        x(64),
        x(22),
        x(86)
    );
    text.len() as u64
}

fn part_types(content: &Json) -> Vec<String> {
    content
        .as_array()
        .unwrap_or(&[])
        .iter()
        .filter_map(|p| {
            p.as_object()
                .and_then(|o| str_of(o, "type"))
                .map(str::to_owned)
        })
        .collect()
}

/// Compares a send decision with a fixture's expected `send` object.
fn compare_send(
    got: Result<Version, ErrorCode>,
    from: &str,
    want: &JsonObject,
) -> Result<(), String> {
    match (got, str_of(want, "result")) {
        (Ok(v), Some("sent")) => {
            let version = v.to_string();
            if str_of(want, "version").is_some_and(|w| w != version) {
                return Err(format!("sent under {version}; expected {want:?}"));
            }
            if str_of(want, "from").is_some_and(|w| w != from) {
                return Err(format!("sent from {from}; expected {want:?}"));
            }
            Ok(())
        }
        (Err(e), Some("refused")) if str_of(want, "error") == Some(e.as_str()) => Ok(()),
        (got, _) => Err(format!("send {got:?}; expected {want:?}")),
    }
}

/// Stage `presence` (§7.5).
pub(super) fn run_presence(fx: &Fixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    let input = obj(&fx.v, "input");
    let expected = obj(&fx.v, "expected");
    let mut registry = PresenceRegistry::new();
    if let Some(own) = context.get("own_sessions").and_then(Json::as_object) {
        for (_, d) in own.iter() {
            let d = SessionDescriptor::from_json(d).ok_or("own_sessions: not a descriptor")?;
            registry.register_own(d);
        }
    }
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let mut discarded = Vec::new();
    for (idx, ev) in arr(input, "events").iter().enumerate() {
        let ev = ev.as_object().ok_or("event")?;
        let when = at(uint(ev.get("at_ms").ok_or("at_ms")?));
        if let Some(lost) = str_of(ev, "carrier_lost") {
            registry.carrier_loss(&carrier(lost));
            continue;
        }
        let rec = ev
            .get("record")
            .ok_or("event without record or carrier_lost")?;
        let issuer = str_of(ev, "issuer").ok_or("issuer")?;
        // The record round-trips through its own wire form.
        if let Ok(r) = PresenceRecord::from_json(rec) {
            let back = PresenceRecord::from_octets(&r.to_octets())
                .map_err(|e| format!("event {idx}: round trip refused: {e:?}"))?;
            if back != r {
                return Err(format!("event {idx}: record does not round-trip"));
            }
        }
        match registry.accept_json(rec, carrier(issuer), RecordOrigin::SameImplementation, when) {
            PresenceAcceptance::Accepted { .. } => {}
            PresenceAcceptance::Discarded(_) => discarded.push(idx as u64),
        }
    }
    let want: Vec<u64> = arr(expected, "discarded").iter().map(uint).collect();
    if discarded != want {
        return Err(format!("discarded {discarded:?}, expected {want:?}"));
    }
    let query = at(uint(input.get("query_at_ms").ok_or("query_at_ms")?));
    let states = expected
        .get("states")
        .and_then(Json::as_object)
        .ok_or("expected.states")?;
    for (s, want) in states.iter() {
        let got = state_name(registry.state(&sid(s)?, query));
        if Some(got) != want.as_str() {
            return Err(format!("state of {s}: {got}, expected {want:?}"));
        }
    }
    match (
        input.get("send").and_then(Json::as_object),
        expected.get("send"),
    ) {
        (None, None) => Ok(()),
        (Some(send), Some(want)) => {
            let from = str_of(send, "from").ok_or("send.from")?;
            let to = str_of(send, "to").ok_or("send.to")?;
            let content = send.get("content").ok_or("send.content")?;
            let types = part_types(content);
            let types: Vec<&str> = types.iter().map(String::as_str).collect();
            let pairs = discoverable(context);
            let to_id = sid(to)?;
            let got = check_send(
                Some(&sid(from)?),
                &to_id,
                |r, t| registry.send_presence(r, t, |r, t| may_discover(&pairs, r, t), query),
                &implemented(context),
                &types,
                |v| estimate_octets(from, to, v, content),
            )
            .map(|a| a.revision);
            compare_send(got, from, want.as_object().ok_or("expected.send")?)
        }
        _ => Err("input.send and expected.send must come together".into()),
    }
}

/// The session an attachment of an attachment list is bound to (§6.10).
fn attached(context: &JsonObject, attachment: &str) -> Option<SessionId> {
    arr(context, "attachments")
        .iter()
        .filter_map(Json::as_object)
        .find(|a| str_of(a, "attachment") == Some(attachment))
        .and_then(|a| a.get("binding"))
        .and_then(Json::as_object)
        .and_then(|b| str_of(b, "session_id"))
        .and_then(SessionId::parse)
}

/// Stage `discovery` (§7.5).
pub(super) fn run_discovery(fx: &Fixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    let request = obj(obj(&fx.v, "input"), "request");
    let expected = obj(&fx.v, "expected");
    let requester = attached(context, str_of(request, "attachment").ok_or("attachment")?);
    let mut sessions = Vec::new();
    for (s, v) in obj(context, "sessions").iter() {
        let v = v.as_object().ok_or("session")?;
        let state = match str_of(v, "presence") {
            Some("online") => PresenceState::Online,
            Some("unreachable") => PresenceState::Unreachable,
            Some("unknown") => PresenceState::Unknown,
            p => return Err(format!("presence {p:?}")),
        };
        let d = v
            .get("descriptor")
            .and_then(SessionDescriptor::from_json)
            .ok_or("descriptor")?;
        sessions.push((sid(s)?, state, d));
    }
    let pairs = discoverable(context);
    let got = discovery_result(
        requester.as_ref(),
        sessions.iter().map(|(s, st, d)| (s, *st, Some(d))),
        |r, s| may_discover(&pairs, r, s),
    );
    match (got, str_of(expected, "result")) {
        (Ok(list), Some("listed")) => {
            let got: BTreeSet<String> = list.iter().map(|d| d.session_id().to_string()).collect();
            let want: BTreeSet<String> = arr(expected, "sessions")
                .iter()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect();
            if got != want {
                return Err(format!("listed {got:?}, expected {want:?}"));
            }
            for d in &list {
                let given = sessions
                    .iter()
                    .find(|(s, _, _)| s == d.session_id())
                    .map(|(_, _, g)| g.as_json());
                if given != Some(d.as_json()) {
                    return Err(format!("{} listed with another descriptor", d.session_id()));
                }
            }
            Ok(())
        }
        (Err(e), Some("refused")) if str_of(expected, "error") == Some(e.as_str()) => Ok(()),
        (got, want) => Err(format!("got {got:?}, expected {want:?}")),
    }
}

/// Stage `send` (§6.10).
pub(super) fn run_send(fx: &Fixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    let request = obj(obj(&fx.v, "input"), "request");
    let requester = attached(context, str_of(request, "attachment").ok_or("attachment")?);
    let to = str_of(request, "to").ok_or("to")?;
    let content = request.get("content").ok_or("content")?;
    let types = part_types(content);
    let types: Vec<&str> = types.iter().map(String::as_str).collect();
    let declarations: BTreeMap<String, SessionCapabilities> = context
        .get("declarations")
        .and_then(Json::as_object)
        .map(|d| {
            d.iter()
                .map(|(k, v)| (k.to_owned(), SessionCapabilities::from_json(v)))
                .collect()
        })
        .unwrap_or_default();
    let to_id = SessionId::parse(to);
    let from = requester
        .as_ref()
        .map(|s| s.to_string())
        .unwrap_or_default();
    let got = match &to_id {
        Some(to_id) => check_send(
            requester.as_ref(),
            to_id,
            |_, t| {
                declarations
                    .get(t.as_str())
                    .ok_or(ErrorCode::UnknownDestination)
            },
            &implemented(context),
            &types,
            |v| estimate_octets(&from, to, v, content),
        )
        .map(|a| a.revision),
        None => Err(ErrorCode::InvalidRequest),
    };
    compare_send(got, &from, obj(&fx.v, "expected"))
}

/// The `sessions` (routing) or `delivery` (receive) map of §8.5, as delivery targets.
fn targets(o: &JsonObject) -> Result<BTreeMap<String, DeliveryTarget>, String> {
    o.iter()
        .map(|(k, v)| {
            let v = v.as_object().ok_or("session")?;
            Ok((
                k.to_owned(),
                DeliveryTarget {
                    accepting: v.get("accepting") == Some(&Json::Bool(true)),
                    active_inbound: v.get("active_inbound") != Some(&Json::Bool(false)),
                    content_types: arr(v, "content_types")
                        .iter()
                        .filter_map(|t| t.as_str().map(str::to_owned))
                        .collect(),
                    max_envelope_octets: None,
                },
            ))
        })
        .collect()
}

fn receiver_limits(context: &JsonObject) -> oac_core::envelope::EnvelopeLimits {
    let types: Vec<String> = arr(context, "receiver_content_types")
        .iter()
        .filter_map(|t| t.as_str().map(str::to_owned))
        .collect();
    envelope_limits(context)
        .with_part_types(types)
        .expect("receiver_content_types are part types")
}

fn compare_result(got: (&str, Option<&str>), expected: &JsonObject) -> Result<(), String> {
    let want = (
        str_of(expected, "result").unwrap_or_default(),
        str_of(expected, "error"),
    );
    if got != want {
        return Err(format!("got {got:?}; expected {want:?}"));
    }
    Ok(())
}

/// Stage `routing` (§8.5).
pub(super) fn run_routing(fx: &Fixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    if context.get("replay_window_ms").map(uint) != Some(REPLAY_WINDOW_MS) {
        return Err("context.replay_window_ms is not 300000".into());
    }
    let (_, now) = limits(context);
    let l = receiver_limits(context);
    let expected = obj(&fx.v, "expected");
    let msg = match receive_envelope(&input_octets(obj(&fx.v, "input")), &l, &now) {
        Ok(m) => m,
        Err(rej) => {
            return compare_result((rej.state.as_str(), Some(rej.error.as_str())), expected);
        }
    };
    let env = msg.envelope();
    let authorized = arr(context, "authorized")
        .iter()
        .filter_map(Json::as_object)
        .any(|p| {
            str_of(p, "from") == Some(env.from().as_str())
                && str_of(p, "to") == Some(env.to().as_str())
        });
    if !authorized {
        return compare_result(("rejected", Some("unauthorized")), expected);
    }
    let sessions = targets(obj(context, "sessions"))?;
    if let Err((s, e)) = delivery_checks(env, sessions.get(env.to().as_str())) {
        return compare_result((s.as_str(), Some(e.as_str())), expected);
    }
    let handoff = str_of(context, "handoff_time")
        .map(|t| Timestamp::parse(t).ok_or("handoff_time"))
        .transpose()?
        .unwrap_or(now);
    match HandOffDeadline::of(env).refusal_at(&handoff) {
        Some((s, e)) => compare_result((s.as_str(), Some(e.as_str())), expected),
        None => compare_result(("valid", None), expected),
    }
}

/// Stage `receive` (§8.5).
pub(super) fn run_receive(fx: &Fixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    let (_, now) = limits(context);
    let l = receiver_limits(context);
    let expected = obj(&fx.v, "expected");
    let refused =
        |s: DeliveryState, e: ErrorCode| compare_result((s.as_str(), Some(e.as_str())), expected);
    let msg = match receive_envelope(&input_octets(obj(&fx.v, "input")), &l, &now) {
        Ok(m) => m,
        Err(rej) => return refused(rej.state, rej.error),
    };
    let keys = trusted_keys(context)?;
    let msg = match authenticate(msg, &keys) {
        Ok(m) => m,
        Err(rej) => return refused(rej.state, rej.error),
    };
    if let Err(rej) = oac_core::replay::check_replay_window(&msg, &now) {
        return refused(rej.state, rej.error);
    }
    // Step 4: F5 (#54); the fixture's verdict until then.
    if str_of(expected, "error") == Some("unauthorized") {
        return refused(DeliveryState::Rejected, ErrorCode::Unauthorized);
    }
    let clock = Arc::new(ManualClock::new(now.clone()));
    let store = DuplicateStore::new(clock.clone());
    let until =
        Timestamp::from_unix_nanos(now.unix_nanos() + i128::from(REPLAY_WINDOW_MS) * 1_000_000)
            .ok_or("window end")?;
    for e in arr(context, "duplicate_store")
        .iter()
        .filter_map(Json::as_object)
    {
        store
            .insert_handed_off(
                DuplicateKey::new(
                    str_of(e, "key_id").ok_or("key_id")?,
                    str_of(e, "nonce").ok_or("nonce")?,
                ),
                &until,
            )
            .map_err(|e| e.to_string())?;
    }
    let delivery = targets(obj(context, "delivery"))?;
    if let Some(h) = str_of(context, "handoff_time") {
        clock.set(Timestamp::parse(h).ok_or("handoff_time")?);
    }
    let mut called = false;
    let got = deliver(
        &store,
        &msg,
        |to| delivery.get(to.as_str()).cloned(),
        &*clock,
        |_| {
            called = true;
            HandOffOutcome::Completed
        },
    )
    .map_err(|e| e.to_string())?;
    let report = match got {
        Received::InFlight => return Err("in flight with no earlier copy".into()),
        Received::Reported(r) => r,
    };
    if called != (report.state == DeliveryState::HandedToHarness) {
        return Err("the hand-off call and the reported state disagree".into());
    }
    let state = match report.state {
        DeliveryState::HandedToHarness => "valid",
        s => s.as_str(),
    };
    compare_result((state, report.error.map(ErrorCode::as_str)), expected)
}

/// Stage `combine` (§8.5).
pub(super) fn run_combine(fx: &Fixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    let copies = uint(context.get("copies_passed").ok_or("copies_passed")?);
    let flag = |n| match context.get(n) {
        Some(Json::Bool(b)) => Some(*b),
        _ => None,
    };
    let deadline_passed = flag("deadline_passed").ok_or("deadline_passed")?;
    let mut held = Vec::new();
    for h in arr(obj(&fx.v, "input"), "held") {
        let h = h.as_object().ok_or("held")?;
        let state = str_of(h, "state")
            .and_then(DeliveryState::parse)
            .ok_or("state")?;
        let observer = str_of(h, "observer")
            .and_then(Observer::parse)
            .ok_or("observer")?;
        // [SC-RCP-030]: an unrecognized code is processed as `failed`.
        let state = match str_of(h, "error") {
            Some(e) if ErrorCode::parse(e).is_none() && state != DeliveryState::Duplicate => {
                DeliveryState::Failed
            }
            _ => state,
        };
        held.push(HeldState { state, observer });
    }
    let state = match flag("handoff_deadline_passed") {
        Some(b) => combined_state(copies, &held, b),
        None => {
            let (a, b) = (
                combined_state(copies, &held, false),
                combined_state(copies, &held, true),
            );
            if a != b {
                return Err("rule 6 decides, but handoff_deadline_passed is missing".into());
            }
            a
        }
    };
    let retry = retry_allowed(copies, &held, state, deadline_passed);
    let expected = obj(&fx.v, "expected");
    let want = (
        str_of(expected, "state").unwrap_or_default(),
        expected.get("retry_allowed").and_then(Json::as_bool),
    );
    if (state.as_str(), Some(retry)) != want {
        return Err(format!("got ({state}, {retry}); expected {want:?}"));
    }
    Ok(())
}

/// The runner's binding table and relation test (see the module documentation), read from
/// a `spec/security.md` §3.3 context.
pub(super) struct FixtureBindings {
    own: BTreeSet<String>,
    table: BTreeMap<String, Json>,
    grants: Vec<JsonObject>,
    sent: Vec<JsonObject>,
    handed_off: Vec<JsonObject>,
}

impl FixtureBindings {
    pub(super) fn from_context(c: &JsonObject) -> FixtureBindings {
        let objs = |n| {
            arr(c, n)
                .iter()
                .filter_map(Json::as_object)
                .cloned()
                .collect()
        };
        FixtureBindings {
            own: c
                .get("sessions")
                .and_then(Json::as_object)
                .map(|s| s.names().map(str::to_owned).collect())
                .unwrap_or_default(),
            table: c
                .get("bindings")
                .and_then(Json::as_object)
                .map(|b| b.iter().map(|(k, v)| (k.to_owned(), v.clone())).collect())
                .unwrap_or_default(),
            grants: objs("grants"),
            sent: objs("sent"),
            handed_off: objs("handed_off"),
        }
    }

    /// The table as a binding map (§3.3), for `bindings_after`.
    fn as_map(&self) -> BTreeMap<String, String> {
        self.table
            .iter()
            .map(|(k, v)| (k.clone(), v.to_compact()))
            .collect()
    }
}

fn within_reply_period(created_at: Option<&str>, now: &Timestamp) -> bool {
    created_at
        .and_then(Timestamp::parse)
        .is_some_and(|c| now.unix_nanos() < c.unix_nanos() + REPLY_PERIOD_MS * 1_000_000)
}

impl ConsumerBindings for FixtureBindings {
    fn binding(&self, session: &SessionId) -> BindingEntry {
        match self.table.get(session.as_str()) {
            None => BindingEntry::Unbound,
            Some(Json::String(k)) => {
                KeyId::parse(k).map_or(BindingEntry::Conflict, BindingEntry::Bound)
            }
            Some(_) => BindingEntry::Conflict,
        }
    }

    fn is_own_session(&self, session: &SessionId) -> bool {
        self.own.contains(session.as_str())
    }

    fn related(&self, key: &KeyId, session: &SessionId, now: &Timestamp) -> bool {
        let names = |side: Option<&Json>| {
            side.and_then(Json::as_object).is_some_and(|s| {
                str_of(s, "key_id") == Some(key.as_str())
                    && str_of(s, "session_id").is_none_or(|r| r == session.as_str())
            })
        };
        self.grants.iter().any(|g| match str_of(g, "direction") {
            Some("inbound") => names(g.get("writer")),
            Some("outbound") => names(g.get("target")),
            _ => false,
        }) || self.sent.iter().any(|e| {
            str_of(e, "to") == Some(session.as_str())
                && str_of(e, "to_key_id") == Some(key.as_str())
                && within_reply_period(str_of(e, "created_at"), now)
        }) || self.handed_off.iter().any(|h| {
            str_of(h, "from") == Some(session.as_str())
                && within_reply_period(str_of(h, "created_at"), now)
        })
    }

    fn bind(&mut self, session: &SessionId, key: &KeyId) {
        self.table
            .insert(session.to_string(), Json::String(key.to_string()));
    }

    fn mark_conflict(&mut self, session: &SessionId, bound: &KeyId, claimant: &KeyId) {
        let mut keys = [bound.to_string(), claimant.to_string()];
        keys.sort();
        let text = format!("{{\"conflict\":[\"{}\",\"{}\"]}}", keys[0], keys[1]);
        self.table.insert(
            session.to_string(),
            json::parse(text.as_bytes()).expect("a conflict mark"),
        );
    }
}

impl SentEnvelopes for FixtureBindings {
    fn was_sent(&self, id: &str, from: &str, to: &str, nonce: &str) -> bool {
        self.sent.iter().any(|e| {
            str_of(e, "id") == Some(id)
                && str_of(e, "from") == Some(from)
                && str_of(e, "to") == Some(to)
                && str_of(e, "nonce") == Some(nonce)
        })
    }
}

/// Stage `presence-auth` (`spec/security.md` §3.3).
pub(super) fn run_presence_auth(fx: &Fixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    let expected = obj(&fx.v, "expected");
    let now = Timestamp::parse(str_of(context, "consumer_time").ok_or("consumer_time")?)
        .ok_or("consumer_time")?;
    let own_key =
        KeyId::parse(str_of(context, "own_key_id").ok_or("own_key_id")?).ok_or("own_key_id")?;
    let keys = trusted_keys(context)?;
    let mut bindings = FixtureBindings::from_context(context);
    let mut registry = PresenceRegistry::new();
    let t = Instant::now();
    // `latest_seq`: the latest record accepted earlier, as a registry holds it.
    if let Some(latest) = context.get("latest_seq").and_then(Json::as_object) {
        for (s, seq) in latest.iter() {
            let r = PresenceRecord::withdrawal(sid(s)?, uint(seq), now.clone())
                .map_err(|e| format!("latest_seq {s}: {e:?}"))?;
            registry.accept(r, carrier("earlier"), RecordOrigin::OtherImplementation, t);
        }
    }
    let ar = obj(&fx.v, "input")
        .get("authenticated_record")
        .and_then(AuthenticatedPresenceRecord::from_json)
        .ok_or("authenticated_record is not an object")?;
    let out = accept_authenticated_record(
        &ar,
        &keys,
        &own_key,
        &mut bindings,
        &mut registry,
        carrier("peer"),
        &now,
        t,
    );
    let result = if out.accepted() {
        "accepted"
    } else {
        "discarded"
    };
    if Some(result) != str_of(expected, "result") {
        return Err(format!(
            "{result} ({:?}); expected {:?}",
            out.result,
            str_of(expected, "result")
        ));
    }
    let record = if out.finding { "finding" } else { "none" };
    if str_of(expected, "record").is_some_and(|w| w != record) {
        return Err(format!(
            "record {record}; expected {:?}",
            str_of(expected, "record")
        ));
    }
    if let Some(want) = expected.get("effective_lifetime_ms") {
        let got = match out.result {
            Ok(PresenceAcceptance::Accepted {
                effective_lifetime_ms: Some(ms),
            }) => Some(ms),
            _ => None,
        };
        if got != Some(uint(want)) {
            return Err(format!("effective lifetime {got:?}; expected {want:?}"));
        }
        // The registry holds it for exactly that long ([SEC-PRS-007]).
        let rec = ar
            .as_json()
            .get("record")
            .and_then(Json::as_object)
            .ok_or("record")?;
        let s = sid(str_of(rec, "session_id").ok_or("session_id")?)?;
        let end = t + Duration::from_millis(uint(want));
        if registry.state(&s, end - Duration::from_millis(1)) != PresenceState::Online
            || registry.state(&s, end) != PresenceState::Unreachable
        {
            return Err("the registry does not apply the effective lifetime".into());
        }
    }
    if let Some(want) = expected.get("bindings_after").and_then(Json::as_object) {
        let want: BTreeMap<String, String> = want
            .iter()
            .map(|(k, v)| (k.to_owned(), v.to_compact()))
            .collect();
        if bindings.as_map() != want {
            return Err(format!(
                "bindings after {:?}; expected {want:?}",
                bindings.as_map()
            ));
        }
    }
    Ok(())
}

/// Stage `receipt-auth` (`spec/security.md` §3.3).
pub(super) fn run_receipt_auth(fx: &Fixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    let keys = trusted_keys(context)?;
    let bindings = FixtureBindings::from_context(context);
    let ar = obj(&fx.v, "input")
        .get("authenticated_receipt")
        .and_then(AuthenticatedReceipt::from_json);
    let got = match &ar {
        Some(ar) => accept_receipt(ar, &keys, &bindings, &bindings).map(|_| ()),
        None => Err(oac_core::receipt_auth::ReceiptDiscard::Malformed),
    };
    let result = if got.is_ok() {
        "authenticated"
    } else {
        "discarded"
    };
    let want = str_of(obj(&fx.v, "expected"), "result");
    if Some(result) != want {
        return Err(format!("{result} ({got:?}); expected {want:?}"));
    }
    Ok(())
}

/// `receipt_permitted` of a `security` fixture (`spec/security.md` §10.3): `false` exactly
/// when no receipt may be sent for the envelope at all. The receipt gate is asked with the
/// key id the envelope verified under, if any; a copy refused at the envelope stage or at
/// step 1 or 2 has none ([SC-RCP-041], [SEC-RCT-005]).
pub(super) fn run_receipt_permitted(fx: &Fixture) -> Result<(), String> {
    let expected = obj(&fx.v, "expected");
    let Some(want) = expected.get("receipt_permitted").and_then(Json::as_bool) else {
        return Ok(());
    };
    let context = obj(&fx.v, "context");
    let (l, now) = limits(context);
    let keys = trusted_keys(context)?;
    let verified = receive_envelope(&input_octets(obj(&fx.v, "input")), &l, &now)
        .ok()
        .and_then(|m| authenticate(m, &keys).ok())
        .and_then(|m| m.verified_by().map(|p| p.key_id().clone()));
    let report = ReceiverReport {
        state: DeliveryState::Rejected,
        error: Some(ErrorCode::Unauthorized),
        duplicate_receipt_allowed: None,
    };
    let got = may_send_receipt(
        verified.as_ref(),
        &report,
        &mut ReceiptLimiter::default(),
        Instant::now(),
    )
    .is_ok();
    if got != want {
        return Err(format!("receipt permitted: {got}; expected {want}"));
    }
    Ok(())
}

/// The hand-off or sent records of a `reply` or `correlation` context (§8.5).
fn records(context: &JsonObject, name: &str) -> Result<Vec<EnvelopeRecord>, String> {
    arr(context, name)
        .iter()
        .map(|r| {
            let r = r.as_object().ok_or("record")?;
            let token = |m| str_of(r, m).and_then(Token::parse);
            Ok(EnvelopeRecord {
                id: token("id").ok_or("record id")?,
                from: sid(str_of(r, "from").ok_or("record from")?)?,
                to: sid(str_of(r, "to").ok_or("record to")?)?,
                conversation_id: token("conversation_id"),
                correlation_id: token("correlation_id"),
            })
        })
        .collect()
}

/// Stage `reply` (§8.5).
pub(super) fn run_reply(fx: &Fixture) -> Result<(), String> {
    let handed = records(obj(&fx.v, "context"), "handed_off")?;
    let req = obj(obj(&fx.v, "input"), "reply_request");
    let h = reply_headers(
        &handed,
        &sid(str_of(req, "from").ok_or("from")?)?,
        &sid(str_of(req, "to").ok_or("to")?)?,
        str_of(req, "requested_target"),
    );
    let mut got = BTreeMap::new();
    for (k, v) in [
        ("reply_to", &h.reply_to),
        ("conversation_id", &h.conversation_id),
        ("correlation_id", &h.correlation_id),
    ] {
        if let Some(v) = v {
            got.insert(k.to_owned(), v.to_string());
        }
    }
    let expected = obj(&fx.v, "expected");
    let want: BTreeMap<String, String> = obj(expected, "reply_headers")
        .iter()
        .map(|(k, v)| (k.to_owned(), v.as_str().unwrap_or_default().to_owned()))
        .collect();
    let correlation = if h.correlated() {
        "correlated"
    } else {
        "uncorrelated"
    };
    if got != want || Some(correlation) != str_of(expected, "correlation") {
        return Err(format!("got {got:?} {correlation}; expected {expected:?}"));
    }
    Ok(())
}

/// Stage `correlation` (§8.5).
pub(super) fn run_correlation(fx: &Fixture) -> Result<(), String> {
    let context = obj(&fx.v, "context");
    let expected = obj(&fx.v, "expected");
    let (l, now) = limits(context);
    let msg = match receive_envelope(&input_octets(obj(&fx.v, "input")), &l, &now) {
        Ok(m) => m,
        Err(rej) => {
            return compare_result((rej.state.as_str(), Some(rej.error.as_str())), expected);
        }
    };
    if str_of(expected, "result") != Some("valid") {
        return Err(format!("valid; expected {expected:?}"));
    }
    let sent = records(context, "sent")?;
    match (
        answered(&sent, msg.envelope()),
        str_of(expected, "correlation"),
    ) {
        (None, Some("unmatched")) => Ok(()),
        (Some(r), Some("matched")) => {
            let a = obj(expected, "answers");
            if str_of(a, "id") == Some(r.id.as_str()) && str_of(a, "from") == Some(r.from.as_str())
            {
                Ok(())
            } else {
                Err(format!("answers {r:?}; expected {a:?}"))
            }
        }
        (got, want) => Err(format!("answers {got:?}; expected {want:?}")),
    }
}
