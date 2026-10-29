// SPDX-License-Identifier: Apache-2.0

//! Permission relay and steering (06 rows 11 and 12; `spec/security.md` §9.6): a delivery
//! permit authorizes delivery into the session's input and nothing else. Relaying a
//! permission answer is a decision of its own, off unless an operator turns it on for one
//! session, and no decision of any kind enables steering a running turn. The Claude
//! adapter's half of row 11 (it declares no permission relay) is in `provenance.rs`, with
//! the other tests that drive that adapter.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use oac_adapter_codex::CodexAdapter;
use oac_adapter_codex::client::Options;
use oac_adapter_codex::pairing::Config;
use oac_core::adapter::{
    AdapterEvent, Attachment, Connection, ConnectionHandle, DiscoveryRequest,
    DiscoveryRequestResult, HandOff, HandOffOutcome, ProviderAdapter, RequestSink, SendRequest,
    SendRequestResult,
};
use oac_core::authorization::OperatorConfirmed;
use oac_core::authorization::{AuthorizationRequest, Basis, Grant, Kind, LocalSide, PeerSide};
use oac_core::clock::{Clock, SystemClock};
use oac_core::connection::{ConnectionWriter, pipe};
use oac_core::delivery::{DeliveryState, ErrorCode};
use oac_core::envelope::{EnvelopeDraft, EnvelopeLimits, TextPart, receive_envelope};
use oac_core::ids::{SessionId, Token};
use oac_core::json::{self, Json};
use oac_core::keys::{DeviceIdentity, DeviceKey};
use oac_core::signing::authenticate;
use oac_core::trust::TrustedKeySet;
use oac_security_suite::{Device, granted_pair, paired_pair, sid};

fn relay(bob: &mut Device, s: SessionId) -> bool {
    bob.engine
        .decide(&AuthorizationRequest::RelayPermission { session: s })
        .permits(Kind::RelayPermission)
}

/// 06 row 11 ([SEC-AUZ-021]; Appendix A C10): permission relay is off by default, and a
/// grant, even one over the whole device, does not turn it on.
#[test]
fn row11_relay_is_off_by_default_even_with_a_device_wide_grant() {
    let (alice, mut bob) = paired_pair();
    bob.grant(Grant::Inbound {
        writer: PeerSide::device(alice.key_id()),
        target: LocalSide::Device,
    });
    assert_eq!(
        bob.receive(alice.sign("m1", &sid(1), &sid(2), "approve it").octets())
            .state,
        DeliveryState::HandedToHarness
    );
    assert!(!relay(&mut bob, sid(2)));
}

/// 06 row 11 ([SEC-AUZ-005], [SEC-AUZ-021]): relay is an operator's decision for one
/// session. Enabled for one, it covers no other; disabled, it is off again; and an enable
/// the store failed to save is undone.
#[test]
fn row11_relay_is_enabled_for_one_session_only_by_the_operator() {
    let (_alice, mut bob) = granted_pair();
    bob.register(&sid(3), "/work/b");
    bob.engine
        .set_relay(
            sid(2),
            true,
            OperatorConfirmed::by_operator(),
            &bob.pairings,
        )
        .unwrap();
    let d = bob
        .engine
        .decide(&AuthorizationRequest::RelayPermission { session: sid(2) });
    assert_eq!(d.basis(), Some(&Basis::RelaySetting(sid(2))));
    assert!(!relay(&mut bob, sid(3)), "another session");
    bob.engine
        .set_relay(
            sid(2),
            false,
            OperatorConfirmed::by_operator(),
            &bob.pairings,
        )
        .unwrap();
    assert!(!relay(&mut bob, sid(2)));
    bob.pairings.fail_saves(true);
    assert!(
        bob.engine
            .set_relay(
                sid(3),
                true,
                OperatorConfirmed::by_operator(),
                &bob.pairings
            )
            .is_err()
    );
    assert!(!relay(&mut bob, sid(3)), "an unsaved enable is undone");
}

/// 06 row 11 ([IFC-TYP-082], [SEC-AUZ-020]): a `deliver` permit is never read as a relay
/// permit, even for a session with relay enabled.
#[test]
fn row11_a_deliver_permit_never_permits_relay() {
    let (alice, mut bob) = granted_pair();
    bob.engine
        .set_relay(
            sid(2),
            true,
            OperatorConfirmed::by_operator(),
            &bob.pairings,
        )
        .unwrap();
    let d = bob.receive(alice.sign("m1", &sid(1), &sid(2), "yes, approve").octets());
    let msg = d.handed.expect("handed off");
    let deliver = bob
        .engine
        .decide(&AuthorizationRequest::deliver(&msg).expect("verified"));
    assert!(deliver.permits(Kind::Deliver));
    assert!(!deliver.permits(Kind::RelayPermission));
}

/// 06 row 12 ([SEC-AUZ-022]): the decision kinds are exactly the five of Table 4.9, and
/// none enables steering a running turn. This is a compile-time guard, not a runtime proof:
/// every arm is `false`, so the assertion cannot fail at run time; the match is exhaustive,
/// so adding a kind fails to compile here until this test says what it is. Row 12's runtime
/// proof in this suite is `row12_hand_off_is_made_at_most_once_and_no_outcome_steers`.
#[test]
fn row12_no_decision_kind_enables_steering() {
    let all = [
        Kind::Deliver,
        Kind::Discover,
        Kind::ReleasePresence,
        Kind::AcceptPresence,
        Kind::RelayPermission,
    ];
    for k in all {
        let steering = match k {
            Kind::Deliver
            | Kind::Discover
            | Kind::ReleasePresence
            | Kind::AcceptPresence
            | Kind::RelayPermission => false,
        };
        assert!(!steering, "{k}");
        assert!(!k.as_str().contains("steer") && !k.as_str().contains("turn"));
    }
}

/// 06 row 12 ([IFC-ADP-057], Table 5.3): the core makes the hand-off call at most once per
/// copy, and no hand-off outcome means anything stronger than "handed to the harness": none
/// starts, interrupts or steers a turn. A copy whose hand-off did not happen may be
/// retransmitted, and is again offered once.
#[test]
fn row12_hand_off_is_made_at_most_once_and_no_outcome_steers() {
    let names: Vec<&str> = HandOffOutcome::ALL.iter().map(|o| o.as_str()).collect();
    assert_eq!(
        names,
        ["completed", "not-now", "failed", "indeterminate", "refused"]
    );
    for (n, outcome) in HandOffOutcome::ALL.into_iter().enumerate() {
        let (alice, mut bob) = granted_pair();
        let env = alice.sign(&format!("m{n}"), &sid(1), &sid(2), "steer the turn now");
        let first = bob.receive_with(env.octets(), outcome);
        assert!(first.handed.is_some(), "{outcome:?}: one call");
        assert_eq!(first.outcome(), outcome.recorded());
        let again = bob.receive_with(env.octets(), HandOffOutcome::Completed);
        if outcome.may_be_handed_off() {
            assert!(again.handed.is_none(), "{outcome:?}: never twice");
            assert_eq!(again.state, DeliveryState::Duplicate);
        } else {
            assert!(
                again.handed.is_some(),
                "{outcome:?}: a retransmission is offered"
            );
        }
    }
}

// ---- 06 row 12, adapter half: the Codex adapter hands off queue-only ---------------------
//
// [SEC-AUZ-025] to [SEC-AUZ-027], [MCPB-CDX-002] to [MCPB-CDX-005]: every delivery is one
// `thread/queue/add` with the three members of the hand-off, whatever state the thread is
// in, and nothing is sent in its place after a refusal. The tests drive the real
// `CodexAdapter` (a dev-dependency since #68; `scripts/check-crate-deps.mjs` rule 5, as
// `provenance.rs` does for the Claude adapter's row 11 half) against the fake Codex
// app-server process (F9), and read what the adapter sent from the fake's call log, which
// records every request before the fake decides anything and flags `turn/start` and
// `turn/steer` as steering whatever the thread's state. The scene is the adapter's own
// integration scene (`adapters/codex/tests/adapter.rs`), reduced to what a delivery needs:
// a stand-in for the core that issues a pairing value and binds, a carrier to the fake, and
// a shim connection through which the "Codex" session makes the one refused call that
// reveals its thread.

/// 06 row 12, adapter half ([MCPB-CDX-002], [SEC-AUZ-025], [SEC-AUZ-026]): a delivery to an
/// idle thread is one `thread/queue/add` of the three allowed members, and nothing else
/// hands off. The queue starts the turn (the fake dispatches the head at once, as the S3
/// capture recorded), so the adapter needs no idle check and has no `turn/start` to race.
#[test]
fn row12_codex_delivery_to_an_idle_thread_is_one_queue_add() {
    let s = Scene::new();
    let mut sess = s.session();
    s.pair(&mut sess);
    let running = s.running_turn(&sess);
    s.fake.end_turn(&sess.thread, "completed", "ok");
    assert_eq!(at(&s.state(&sess), "activeTurnId"), &Json::Null);

    let h = s.hand_off(&sess, "idle delivery");
    assert_eq!(s.adapter.deliver(h), HandOffOutcome::Completed);

    s.assert_queue_only(1);
    // The head went off the queue into a turn of its own, not into one that was running.
    let state = s.state(&sess);
    assert_eq!(
        array_len(at(&state, "queue")),
        0,
        "taken off the queue at once"
    );
    let started = at(&state, "activeTurnId")
        .as_str()
        .expect("the add started a turn");
    assert_ne!(started, running);
    sess.shim.end();
}

/// 06 row 12, adapter half ([MCPB-CDX-002], [MCPB-CDX-004], [SEC-AUZ-025]): a delivery to a
/// thread with a running turn is one `thread/queue/add`; it waits, the running turn is not
/// touched, and its input arrives in a new turn after `turn/completed`, never under the
/// running turn's id.
#[test]
fn row12_codex_delivery_to_a_busy_thread_waits_in_the_queue_and_never_joins_the_turn() {
    let s = Scene::new();
    let mut sess = s.session();
    s.pair(&mut sess);
    let running = s.running_turn(&sess);

    let h = s.hand_off(&sess, "busy delivery");
    assert_eq!(s.adapter.deliver(h), HandOffOutcome::Completed);

    s.assert_queue_only(1);
    let state = s.state(&sess);
    assert_eq!(at(&state, "activeTurnId").as_str(), Some(running.as_str()));
    assert_eq!(array_len(at(&state, "queue")), 1, "held for the idle");
    assert!(
        !at(&state, "turns").to_compact().contains("busy delivery"),
        "nothing joined a turn"
    );
    // The running turn ends; only then does the queue start a turn, a new one.
    let done = s.fake.end_turn(&sess.thread, "completed", "ok");
    let next = at(&done, "dispatched").as_str().expect("the queue's turn");
    assert_ne!(
        next, running,
        "the input is not under the running turn's id"
    );
    let next = next.to_owned();
    s.fake.end_turn(&sess.thread, "completed", "ok");
    let turns = s.state(&sess);
    let items_of = |id: &str| {
        array(at(&turns, "turns"))
            .iter()
            .find(|t| at(t, "id").as_str() == Some(id))
            .map(|t| at(t, "items").to_compact())
            .unwrap_or_else(|| panic!("no turn {id}"))
    };
    assert!(!items_of(&running).contains("busy delivery"));
    assert!(items_of(&next).contains("busy delivery"));
    s.assert_queue_only(1);
    sess.shim.end();
}

/// 06 row 12, adapter half ([MCPB-CDX-003], [SEC-AUZ-026]): a turn that starts just after the
/// thread went idle (the harness's own user, as `SEC-AUZ-026` has it) is the race a "check,
/// then `turn/start`" rule would lose and steer on. The adapter makes no check and no
/// `turn/start`: its add queues behind that turn.
#[test]
fn row12_codex_delivery_when_a_turn_starts_just_after_an_idle_status_still_only_queues() {
    let s = Scene::new();
    let mut sess = s.session();
    s.pair(&mut sess);
    s.fake.end_turn(&sess.thread, "completed", "ok");
    s.fake
        .tui_turn(&sess.thread, "the harness user types at once");
    let running = s.running_turn(&sess);

    let h = s.hand_off(&sess, "raced delivery");
    assert_eq!(s.adapter.deliver(h), HandOffOutcome::Completed);

    s.assert_queue_only(1);
    let state = s.state(&sess);
    assert_eq!(at(&state, "activeTurnId").as_str(), Some(running.as_str()));
    assert_eq!(array_len(at(&state, "queue")), 1);
    assert!(!at(&state, "turns").to_compact().contains("raced delivery"));
    sess.shim.end();
}

/// 06 row 12, adapter half ([MCPB-CDX-002]): a delivery to an idle thread whose last turn
/// ended interrupted is still one `thread/queue/add`. The harness holds it (nothing is
/// dispatched after an interrupt), and the adapter does not start a turn to get it through;
/// it runs once a turn completes uninterrupted.
#[test]
fn row12_codex_delivery_to_an_idle_thread_after_an_interrupted_turn_only_queues() {
    let s = Scene::new();
    let mut sess = s.session();
    s.pair(&mut sess);
    s.fake.end_turn(&sess.thread, "interrupted", "half an ans");
    let state = s.state(&sess);
    assert_eq!(at(&state, "lastTurnInterrupted").as_bool(), Some(true));
    assert_eq!(at(&state, "activeTurnId"), &Json::Null);

    let h = s.hand_off(&sess, "after the interrupt");
    assert_eq!(s.adapter.deliver(h), HandOffOutcome::Completed);

    s.assert_queue_only(1);
    let state = s.state(&sess);
    assert_eq!(array_len(at(&state, "queue")), 1, "held, not dispatched");
    assert_eq!(
        at(&state, "activeTurnId"),
        &Json::Null,
        "no turn was started"
    );
    // A turn the harness's user starts and that completes uninterrupted releases it.
    s.fake.tui_turn(&sess.thread, "the harness user goes on");
    let done = s.fake.end_turn(&sess.thread, "completed", "ok");
    assert!(at(&done, "dispatched").as_str().is_some(), "{done:?}");
    s.assert_queue_only(1);
    sess.shim.end();
}

/// 06 row 12, adapter half ([SEC-AUZ-027], [MCPB-CDX-003], [MCPB-CDX-004]): when the add is
/// turned away (here the recorded archived-thread refusal) the adapter reports the hand-off
/// failed and sends nothing in its place: no `turn/start`, no `turn/steer`, no second add of
/// the same message. A later, separate delivery is again one add.
#[test]
fn row12_codex_a_turned_away_add_is_not_followed_by_any_other_hand_off() {
    let s = Scene::new();
    let mut sess = s.session();
    s.pair(&mut sess);
    s.fake.end_turn(&sess.thread, "completed", "ok");
    s.fake.archive(&sess.thread, true);

    let out = s
        .adapter
        .deliver(s.hand_off(&sess, "to an archived thread"));
    assert_eq!(out, HandOffOutcome::Failed);
    assert_eq!(out.recorded().1, Some(ErrorCode::HandoffFailed));
    // Exactly the refused add: the one hand-off call, answered with the recorded refusal.
    let report = s.report();
    s.assert_queue_only(1);
    let refusal: Vec<String> = array(at(&report, "calls"))
        .iter()
        .filter(|c| at(c, "method").as_str() == Some("thread/queue/add"))
        .map(|c| {
            at(c, "error.message")
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert_eq!(refusal.len(), 1);
    assert!(refusal[0].contains("is archived"), "{refusal:?}");

    s.fake.archive(&sess.thread, false);
    let again = s.adapter.deliver(s.hand_off(&sess, "after unarchive"));
    assert_eq!(again, HandOffOutcome::Completed);
    s.assert_queue_only(2);
    sess.shim.end();
}

/// 06 row 12, adapter half ([MCPB-CDX-005]): the negative cases. The check the tests above
/// apply to the adapter's calls reports a `turn/start`, a `turn/steer`, and a
/// `thread/queue/add` that carries a thread setting such as `approvalPolicy`; so a regression
/// in the adapter into any of them would fail those tests, not pass. Three such calls are
/// made by a second client of the same fake (the "TUI user"), which the fake logs and flags
/// as it does the adapter's.
#[test]
fn row12_codex_a_steering_call_or_a_setting_member_is_reported_nonconformant() {
    let fake = Fake::spawn();
    let thread = fake.create_thread();
    // turn/start on an idle thread, and again on a running one (steers it).
    fake.tui_turn(&thread, "first");
    fake.tui_turn(&thread, "joins the running turn");
    // turn/steer is answered NOT_MODELLED; it is logged and flagged regardless.
    let _ = fake.call(
        "turn/steer",
        &format!(
            "{{\"threadId\":{},\"expectedTurnId\":\"t\",\"input\":[{{\"type\":\"text\",\"text\":\"steer\"}}]}}",
            js(&thread)
        ),
    );
    // An add with a thread setting beside the three members of the hand-off.
    let _ = fake.call(
        "thread/queue/add",
        &format!(
            "{{\"threadId\":{},\"input\":[{{\"type\":\"text\",\"text\":\"q\"}}],\"clientUserMessageId\":\"c1\",\"approvalPolicy\":\"never\"}}",
            js(&thread)
        ),
    );
    let report = fake.report(TUI);
    let violations = queue_only_violations(&report, 1);
    for needle in ["turn/start", "turn/steer", "approvalPolicy"] {
        assert!(
            violations.iter().any(|v| v.contains(needle)),
            "{needle} not reported: {violations:?}"
        );
    }
    assert_eq!(
        array_len(at(&report, "steering")),
        3,
        "the fake flagged each"
    );
    assert_eq!(array_len(at(&report, "overrideMembers")), 1);
    // And the same check passes a hand-off that is exactly one conformant add.
    let ok = json::parse(
        b"{\"calls\":[{\"method\":\"thread/queue/add\",\"params\":{\"threadId\":\"t\",\"input\":[],\"clientUserMessageId\":\"c\"}}],\"steering\":[],\"overrideMembers\":[],\"malformed\":[],\"control\":[]}",
    )
    .expect("JSON");
    assert_eq!(queue_only_violations(&ok, 1), Vec::<String>::new());
}

/// `spec/security.md` §13 "Bypass of a harness's own consent step" ([SEC-AUZ-023]): no
/// implementation automates past, suppresses or pre-answers a harness's consent prompt.
/// The spec names H2's review of the launch path as the proving test; nothing in the core
/// or the fakes launches a harness. Gated: H2.
#[test]
#[ignore = "GATED on #74 (H2, security verification against the real harnesses)"]
fn gated_s13_no_harness_consent_step_is_automated() {
    std::panic!(
        "GATED on #74: review the OAC-enabled launch path and assert no consent step is answered for the operator"
    );
}

// ---- the Codex scene -------------------------------------------------------------------

const NAME: &str = "oac-test";
const TUI: &str = "oac-g7-test-tui";
const WAIT: Duration = Duration::from_secs(10);
const STARTUP: Duration = Duration::from_secs(60);
const CONFIRM_WAIT: Duration = Duration::from_millis(400);
/// The members of a hand-off's `thread/queue/add` and no others ([MCPB-CDX-005]).
const ADD_MEMBERS: [&str; 3] = ["clientUserMessageId", "input", "threadId"];

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn js(s: &str) -> String {
    Json::String(s.to_owned()).to_compact()
}

static NULL: Json = Json::Null;

/// `v.a.b`: the member at a dotted path, or `null`.
fn at<'a>(v: &'a Json, path: &str) -> &'a Json {
    path.split('.').fold(v, |cur, name| {
        cur.as_object().and_then(|o| o.get(name)).unwrap_or(&NULL)
    })
}

fn array(v: &Json) -> &[Json] {
    v.as_array().unwrap_or(&[])
}

fn array_len(v: &Json) -> usize {
    array(v).len()
}

/// What a set of calls to the fake breaks of the hand-off rules, as one line each: a
/// hand-off by anything but `thread/queue/add` ([MCPB-CDX-003], [MCPB-CDX-004]), any other
/// `turn/*` call, a number of adds other than `adds` ([IFC-ADP-057]: one per delivery, none
/// repeated after a refusal), an add with other members than the three ([MCPB-CDX-005]), and
/// whatever the fake itself flagged (steering, extra members, malformed frames, calls of its
/// own control methods). `report` is the fake's `oacFake/calls` result for one client.
fn queue_only_violations(report: &Json, adds: usize) -> Vec<String> {
    let mut v = Vec::new();
    let mut seen_adds = 0;
    for c in array(at(report, "calls")) {
        let method = at(c, "method").as_str().unwrap_or_default();
        match method {
            "thread/queue/add" => {
                seen_adds += 1;
                let mut members: Vec<&str> = at(c, "params")
                    .as_object()
                    .map(|o| o.iter().map(|(k, _)| k).collect())
                    .unwrap_or_default();
                members.sort_unstable();
                if members != ADD_MEMBERS {
                    v.push(format!("thread/queue/add with members {members:?}"));
                }
            }
            m if m.starts_with("turn/") => v.push(format!("{m} was sent")),
            _ => {}
        }
    }
    if seen_adds != adds {
        v.push(format!("{seen_adds} adds, expected {adds}"));
    }
    for (flag, what) in [
        ("steering", "flagged as steering"),
        (
            "overrideMembers",
            "flagged for members beyond the hand-off's own",
        ),
        ("malformed", "malformed"),
        ("control", "oacFake control calls"),
    ] {
        if array_len(at(report, flag)) > 0 {
            v.push(format!(
                "the fake {what}: {}",
                at(report, flag).to_compact()
            ));
        }
    }
    v
}

// The fake Codex app-server (F9, tests/fakes/codex-app-server/), spawned with node.

type Waiters = Arc<Mutex<HashMap<u64, Sender<Result<Json, Json>>>>>;

/// The fake, spawned, with its stdio connection for control calls and the "TUI user" (a
/// second client of the same app-server, as Codex's TUI is of its daemon).
struct Fake {
    child: Child,
    stdin: Mutex<ChildStdin>,
    url: String,
    waiters: Waiters,
    next: AtomicU64,
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Fake {
    fn spawn() -> Fake {
        let server = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fakes/codex-app-server/server.mjs");
        let mut child = Command::new("node")
            .arg(server)
            .args(["--stdio", "--listen", "ws://127.0.0.1:0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn the fake Codex app-server with node");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (url_tx, url_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut sent = false;
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if !sent
                    && let Ok(v) = json::parse(line.as_bytes())
                    && let Some(u) = at(&v, "url").as_str()
                {
                    let _ = url_tx.send(u.to_owned());
                    sent = true;
                    continue;
                }
                eprintln!("fake codex: {line}");
            }
        });
        let waiters: Waiters = Arc::default();
        let w = waiters.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Ok(v) = json::parse(line.as_bytes()) else {
                    continue;
                };
                let Some(id) = at(&v, "id")
                    .as_number()
                    .and_then(|n| n.plain_integer_in(0, u64::MAX))
                else {
                    continue;
                };
                let reply = match (at(&v, "result"), at(&v, "error")) {
                    (Json::Null, Json::Null) => continue,
                    (Json::Null, e) => Err(e.clone()),
                    (r, _) => Ok(r.clone()),
                };
                if let Some(tx) = lock(&w).remove(&id) {
                    let _ = tx.send(reply);
                }
            }
        });
        let url = url_rx
            .recv_timeout(STARTUP)
            .expect("the fake reports its listener");
        let fake = Fake {
            child,
            stdin: Mutex::new(stdin),
            url,
            waiters,
            next: AtomicU64::new(1),
        };
        fake.call(
            "initialize",
            &format!(
                "{{\"clientInfo\":{{\"name\":{},\"version\":\"0\"}},\"capabilities\":{{\"experimentalApi\":true}}}}",
                js(TUI)
            ),
        )
        .expect("initialize the TUI connection");
        fake
    }

    /// A call as the TUI user (the stdio connection).
    fn call(&self, method: &str, params: &str) -> Result<Json, Json> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        lock(&self.waiters).insert(id, tx);
        let frame = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":{},\"params\":{params}}}\n",
            js(method)
        );
        lock(&self.stdin).write_all(frame.as_bytes()).unwrap();
        rx.recv_timeout(WAIT).expect("an answer from the fake")
    }

    fn control(&self, method: &str, params: &str) -> Json {
        self.call(method, params)
            .unwrap_or_else(|e| panic!("{method}: {}", e.to_compact()))
    }

    fn create_thread(&self) -> String {
        let r = self.control("oacFake/thread/create", "{\"cwd\":\"/oac-g7\"}");
        at(&r, "threadId").as_str().unwrap().to_owned()
    }

    /// The TUI user's `turn/start`: the harness's own user typing.
    fn tui_turn(&self, thread: &str, text: &str) {
        self.call(
            "turn/start",
            &format!(
                "{{\"threadId\":{},\"input\":[{{\"type\":\"text\",\"text\":{}}}]}}",
                js(thread),
                js(text)
            ),
        )
        .expect("the TUI's turn starts");
    }

    /// A thread whose turn is running, so that an MCP tool call can be made in it.
    fn busy_thread(&self) -> String {
        let t = self.create_thread();
        self.tui_turn(&t, "the TUI user's words");
        t
    }

    /// Ends the running turn (`completed` or `interrupted`); returns the fake's answer, whose
    /// `dispatched` is the id of the turn the queue started, or `null`.
    fn end_turn(&self, thread: &str, status: &str, agent_text: &str) -> Json {
        self.control(
            "oacFake/turn/complete",
            &format!(
                "{{\"threadId\":{},\"status\":{},\"agentText\":{}}}",
                js(thread),
                js(status),
                js(agent_text)
            ),
        )
    }

    fn archive(&self, thread: &str, archived: bool) {
        self.control(
            "oacFake/thread/setArchived",
            &format!("{{\"threadId\":{},\"archived\":{archived}}}", js(thread)),
        );
    }

    /// `oacFake/mcp/start`: the thread's running turn makes a tool call. Returns the item id.
    fn mcp_start(&self, thread: &str, server: &str, tool: &str, arguments: &str) -> String {
        let r = self.control(
            "oacFake/mcp/start",
            &format!(
                "{{\"threadId\":{},\"server\":{},\"tool\":{},\"arguments\":{arguments}}}",
                js(thread),
                js(server),
                js(tool)
            ),
        );
        at(&r, "itemId").as_str().unwrap().to_owned()
    }

    /// `oacFake/mcp/complete`: the call returned `text` (a refusal is `failed`).
    fn mcp_complete(&self, thread: &str, item: &str, failed: bool, text: &str) {
        self.control(
            "oacFake/mcp/complete",
            &format!(
                "{{\"threadId\":{},\"itemId\":{},\"outcome\":{},\"text\":{}}}",
                js(thread),
                js(item),
                js(if failed { "failed" } else { "completed" }),
                js(text)
            ),
        );
    }

    /// The fake's call log for one client (`clientInfo.name`): `{calls, steering,
    /// overrideMembers, handOffs, malformed, control}`.
    fn report(&self, client: &str) -> Json {
        self.control(
            "oacFake/calls",
            &format!("{{\"clientName\":{}}}", js(client)),
        )
    }
}

// A stand-in for the core, as in adapters/codex/tests/adapter.rs.

#[derive(Default)]
struct CoreState {
    events: Vec<AdapterEvent>,
    /// The value issued to each attachment, until it is revealed.
    issued: HashMap<Attachment, String>,
    bound: HashMap<Attachment, SessionId>,
}

/// The core's side of the adapter: it issues a pairing value to a refused request, binds on
/// the reveal and calls `set_binding` from inside the event handler (as the core does), and
/// stops an attachment on `attachment-unconfirmed`.
struct Core {
    st: Mutex<CoreState>,
    changed: Condvar,
    adapter: OnceLock<Arc<CodexAdapter>>,
    carrier: OnceLock<ConnectionHandle>,
    seq: AtomicU64,
}

impl Core {
    fn new() -> Arc<Core> {
        Arc::new(Core {
            st: Mutex::default(),
            changed: Condvar::new(),
            adapter: OnceLock::new(),
            carrier: OnceLock::new(),
            seq: AtomicU64::new(1),
        })
    }

    fn adapter(&self) -> Arc<CodexAdapter> {
        self.adapter.get().expect("adapter").clone()
    }

    fn wait_for(&self, what: &str, pred: impl Fn(&CoreState) -> bool) {
        let end = Instant::now() + WAIT;
        let mut st = lock(&self.st);
        while !pred(&st) {
            let now = Instant::now();
            assert!(now < end, "no {what}; events {:?}", st.events);
            st = self
                .changed
                .wait_timeout(st, end - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    fn handler(self: &Arc<Self>) -> oac_core::adapter::AdapterEventHandler {
        let me = self.clone();
        Arc::new(move |e| me.on_event(e))
    }

    fn on_event(&self, e: AdapterEvent) {
        lock(&self.st).events.push(e.clone());
        self.changed.notify_all();
        match e {
            AdapterEvent::NativeSignal(s) => {
                let Some(value) = s.revealed.clone() else {
                    return;
                };
                // A reveal counts only on the carrier, and for a value this core issued.
                if self.carrier.get() != s.connection.as_ref() {
                    return;
                }
                let mut st = lock(&self.st);
                let Some(att) = st
                    .issued
                    .iter()
                    .find(|(_, v)| **v == value)
                    .map(|(a, _)| a.clone())
                else {
                    return;
                };
                st.issued.remove(&att);
                let n = self.seq.fetch_add(1, Ordering::SeqCst);
                let session = SessionId::from_random_octets([n as u8; 16]);
                st.bound.insert(att.clone(), session.clone());
                drop(st);
                self.adapter().set_binding(&att, Some(session));
                self.changed.notify_all();
            }
            AdapterEvent::AttachmentUnconfirmed { attachment } => {
                let was = lock(&self.st).bound.remove(&attachment);
                if was.is_some() {
                    self.adapter().set_binding(&attachment, None);
                }
            }
            AdapterEvent::AttachmentClosed { attachment } => {
                lock(&self.st).bound.remove(&attachment);
                self.adapter().set_binding(&attachment, None);
            }
            _ => {}
        }
    }

    fn refusal(&self, a: &Attachment) -> SendRequestResult {
        let mut st = lock(&self.st);
        let value = st
            .issued
            .entry(a.clone())
            .or_insert_with(|| format!("oac-pair-{:032x}", self.seq.fetch_add(1, Ordering::SeqCst)))
            .clone();
        SendRequestResult::Refused {
            error: ErrorCode::Unauthorized,
            pairing_value: Some(value),
        }
    }
}

/// The request sink: an unpaired session's `send` is refused with its pairing value. (The
/// tests send nothing after the pairing.)
struct CoreSink(Arc<Core>);

impl RequestSink for CoreSink {
    fn send(&self, request: SendRequest) -> SendRequestResult {
        self.0.refusal(&request.attachment)
    }

    fn discover(&self, request: DiscoveryRequest) -> DiscoveryRequestResult {
        match self.0.refusal(&request.attachment) {
            SendRequestResult::Refused {
                error,
                pairing_value,
            } => DiscoveryRequestResult::Refused {
                error,
                pairing_value,
            },
            _ => DiscoveryRequestResult::DiscoveryResult(Vec::new()),
        }
    }
}

/// A TCP connection to the fake's listener as a core-owned connection: two threads move bytes
/// between the socket and the connection's pipe halves.
struct Carrier {
    socket: TcpStream,
}

impl Drop for Carrier {
    fn drop(&mut self) {
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
    }
}

fn carrier_connection(fake: &Fake) -> (Connection, Carrier) {
    let host = fake
        .url
        .strip_prefix("ws://")
        .unwrap()
        .trim_end_matches('/')
        .to_owned();
    let socket = TcpStream::connect(host).expect("connect to the fake's listener");
    let (mut to_adapter, adapter_reads) = pipe();
    let (adapter_writes, from_adapter) = mpsc::channel::<Vec<u8>>();
    let conn = Connection::accept(adapter_reads, ConnectionWriter::from(adapter_writes));
    let mut read = socket.try_clone().unwrap();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 8192];
        while let Ok(n) = read.read(&mut buf) {
            if n == 0 || to_adapter.write_all(&buf[..n]).is_err() {
                break;
            }
        }
    });
    let mut write = socket.try_clone().unwrap();
    std::thread::spawn(move || {
        for chunk in from_adapter {
            if write.write_all(&chunk).is_err() {
                break;
            }
        }
    });
    (conn, Carrier { socket })
}

/// The MCP client side of one shim connection: Codex's stdio, over a pipe.
struct Shim {
    to_adapter: Option<ConnectionWriter>,
    from_adapter: Receiver<Vec<u8>>,
    partial: Vec<u8>,
    lines: VecDeque<Json>,
    id: u64,
}

fn shim_connection() -> (Connection, Shim) {
    let (to_adapter, adapter_reads) = pipe();
    let (adapter_writes, from_adapter) = mpsc::channel::<Vec<u8>>();
    let conn = Connection::accept(adapter_reads, ConnectionWriter::from(adapter_writes));
    (
        conn,
        Shim {
            to_adapter: Some(to_adapter),
            from_adapter,
            partial: Vec::new(),
            lines: VecDeque::new(),
            id: 0,
        },
    )
}

impl Shim {
    fn send(&mut self, frame: &str) {
        let w = self.to_adapter.as_mut().expect("open");
        w.write_all(frame.as_bytes()).unwrap();
        w.write_all(b"\n").unwrap();
    }

    fn next_frame(&mut self) -> Json {
        let end = Instant::now() + WAIT;
        loop {
            if let Some(v) = self.lines.pop_front() {
                return v;
            }
            let now = Instant::now();
            assert!(now < end, "no frame from the adapter");
            match self.from_adapter.recv_timeout(end - now) {
                Ok(chunk) => {
                    self.partial.extend_from_slice(&chunk);
                    while let Some(i) = self.partial.iter().position(|&b| b == b'\n') {
                        let line: Vec<u8> = self.partial.drain(..=i).collect();
                        let text = String::from_utf8_lossy(&line);
                        if !text.trim().is_empty() {
                            self.lines
                                .push_back(json::parse(text.trim().as_bytes()).expect("JSON"));
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => panic!("no frame from the adapter"),
                Err(RecvTimeoutError::Disconnected) => panic!("the adapter closed the stream"),
            }
        }
    }

    fn request(&mut self, method: &str, params: &str) -> Json {
        self.id += 1;
        let id = self.id;
        self.send(&format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":{},\"params\":{params}}}",
            js(method)
        ));
        loop {
            let v = self.next_frame();
            if at(&v, "id").as_number().map(|n| n.raw()) == Some(&id.to_string()) {
                return v;
            }
        }
    }

    /// `initialize`, `notifications/initialized`, `tools/list`: the way Codex opens.
    fn open(&mut self) {
        self.request(
            "initialize",
            "{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},\"clientInfo\":{\"name\":\"codex-mcp-client\",\"version\":\"0.163.0\"}}",
        );
        self.send("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}");
        self.request("tools/list", "{}");
    }

    /// A tool call: `(is_error, text)`.
    fn call(&mut self, tool: &str, args: &str) -> (bool, String) {
        let v = self.request(
            "tools/call",
            &format!("{{\"name\":{},\"arguments\":{args}}}", js(tool)),
        );
        let r = at(&v, "result");
        assert!(*r != Json::Null, "an error response: {}", v.to_compact());
        (
            at(r, "isError").as_bool() == Some(true),
            at(&array(at(r, "content"))[0], "text")
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        )
    }

    fn end(&mut self) {
        self.to_adapter = None;
    }
}

/// One adapter on the fake: the carrier up, a core stand-in behind it.
struct Scene {
    fake: Fake,
    core: Arc<Core>,
    adapter: Arc<CodexAdapter>,
    _carrier: Carrier,
}

/// A Codex session: a thread with a running turn, and its shim connection.
struct Session {
    thread: String,
    shim: Shim,
    handle: Attachment,
}

impl Scene {
    fn new() -> Scene {
        let fake = Fake::spawn();
        let mut cfg = Config::new(NAME).without_serving_thread();
        cfg.confirm_wait = CONFIRM_WAIT;
        let adapter = Arc::new(CodexAdapter::with_config(cfg, Options::default()));
        let core = Core::new();
        core.adapter.set(adapter.clone()).ok();
        adapter.watch_attachments(core.handler());
        adapter.accept_requests(Arc::new(CoreSink(core.clone())));
        let (conn, carrier) = carrier_connection(&fake);
        let carrier_handle = conn.handle().clone();
        core.carrier.set(carrier_handle.clone()).ok();
        assert!(adapter.designate_carrier(&carrier_handle));
        adapter.take_connection(conn);
        let end = Instant::now() + WAIT;
        while adapter.serving_counts().is_none() {
            assert!(Instant::now() < end, "the carrier did not come up");
            std::thread::sleep(Duration::from_millis(5));
        }
        Scene {
            fake,
            core,
            adapter,
            _carrier: carrier,
        }
    }

    /// A Codex session: a thread with a running turn, its shim connection opened and listed.
    fn session(&self) -> Session {
        let thread = self.fake.busy_thread();
        let (conn, mut shim) = shim_connection();
        let handle = conn.handle().clone();
        self.adapter.take_connection(conn);
        shim.open();
        self.core.wait_for("attachment-opened", |s| {
            s.events.iter().any(|e| {
                matches!(e, AdapterEvent::AttachmentOpened { attachment, .. } if *attachment == handle)
            })
        });
        Session {
            thread,
            shim,
            handle,
        }
    }

    /// The pairing: Codex makes a `send` that the core refuses with a value, the refusal is
    /// revealed on the carrier, and the core binds the session to the thread.
    fn pair(&self, s: &mut Session) {
        let args = format!(
            "{{\"to\":\"{}\",\"content\":[{{\"type\":\"text\",\"text\":\"hello\"}}]}}",
            SessionId::from_random_octets([0xee; 16]).as_str()
        );
        let item = self.fake.mcp_start(&s.thread, NAME, "send", &args);
        let (err, text) = s.shim.call("send", &args);
        self.fake.mcp_complete(&s.thread, &item, err, &text);
        assert!(err && text.contains("oac-pair-"), "{text}");
        let h = s.handle.clone();
        self.core
            .wait_for("a binding", |st| st.bound.contains_key(&h));
    }

    /// A verified hand-off of `text` to `s`, from a peer, for the session the core bound.
    fn hand_off(&self, s: &Session, text: &str) -> HandOff {
        let identity =
            DeviceIdentity::new(DeviceKey::generate(), Token::parse("principal").unwrap());
        let trusted = TrustedKeySet::new(&identity);
        let now = SystemClock.now();
        let session = lock(&self.core.st)
            .bound
            .get(&s.handle)
            .cloned()
            .expect("bound");
        let id = Token::parse(&format!(
            "h{}",
            self.core.seq.fetch_add(1, Ordering::SeqCst)
        ))
        .unwrap();
        let draft = EnvelopeDraft::new(
            id,
            SessionId::from_random_octets([0xee; 16]),
            session,
            now.clone(),
            vec![TextPart::new(text).unwrap()],
        )
        .unwrap();
        let env = identity.sign_envelope(draft);
        let msg = receive_envelope(env.octets(), &EnvelopeLimits::default(), &now).unwrap();
        HandOff::new(s.handle.clone(), authenticate(msg, &trusted).unwrap()).unwrap()
    }

    /// The fake's log of the adapter's own calls (its `clientInfo.name`).
    fn report(&self) -> Json {
        self.fake.report(oac_adapter_codex::client::CLIENT_NAME)
    }

    /// Every call the adapter has made breaks none of the hand-off rules, and exactly `adds`
    /// of them were a `thread/queue/add`.
    fn assert_queue_only(&self, adds: usize) {
        let report = self.report();
        let violations = queue_only_violations(&report, adds);
        assert!(
            violations.is_empty(),
            "{violations:?}\n{}",
            report.to_compact()
        );
        assert_eq!(
            array_len(at(&report, "handOffs")),
            adds,
            "one hand-off call per add"
        );
    }

    fn state(&self, s: &Session) -> Json {
        self.fake.control(
            "oacFake/thread/state",
            &format!("{{\"threadId\":{}}}", js(&s.thread)),
        )
    }

    /// The id of the turn running on the session's thread.
    fn running_turn(&self, s: &Session) -> String {
        at(&self.state(s), "activeTurnId")
            .as_str()
            .expect("a running turn")
            .to_owned()
    }
}
