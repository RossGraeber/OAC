// SPDX-License-Identifier: Apache-2.0

//! The client against the fake Codex app-server (F9, `tests/fakes/codex-app-server/`),
//! spawned with `node` on stdio and a loopback WebSocket listener. CI-default tier: no
//! Codex install, no credential, no network beyond loopback (`oac-testing` §2).
//!
//! The test plays `cli/` (D8): it opens the TCP stream to the fake's listener and hands
//! both halves to the client. On stdio it drives the fake's own control methods
//! (`oacFake/*`) and a "TUI user" whose turns make a thread busy, and it reads the fake's
//! call log for what the client sent, filtered by the client's `clientInfo.name`.

use oac_adapter_codex::client::{CLIENT_NAME, ClientError};
use oac_adapter_codex::events::{CodexEvent, CompletedItem};
use oac_adapter_codex::threads::{ThreadState, TurnEnd};
use oac_core::health::HealthState;
use oac_core::json::Json;

mod support;
use support::*;

fn method_of(c: &Json) -> &str {
    get_str(c, "method").unwrap_or_default()
}

fn flag(c: &Json, f: &str) -> Option<bool> {
    get(c, "flags")
        .and_then(|x| get(x, f))
        .and_then(Json::as_bool)
}

#[test]
fn initialize_declares_a_stable_name_and_experimental_api() {
    let fake = Fake::spawn();
    let events = Events::default();
    let client = fake.connect(&events);
    let calls = fake.client_calls();
    let init = calls
        .iter()
        .find(|c| method_of(c) == "initialize")
        .expect("an initialize call");
    let p = get(init, "params").unwrap();
    assert_eq!(
        get(p, "clientInfo").and_then(|c| get_str(c, "name")),
        Some(CLIENT_NAME)
    );
    assert_eq!(
        get(p, "capabilities")
            .and_then(|c| get(c, "experimentalApi"))
            .and_then(Json::as_bool),
        Some(true)
    );
    assert!(
        calls.iter().any(|c| method_of(c) == "initialized"),
        "the initialized notification"
    );
    // The version comes from the recorded userAgent; it differs from the shim's, which is a
    // warning only (#216).
    assert!(client.codex_version().is_some());
    let h = client.health();
    assert_eq!(h.state, HealthState::Healthy, "{h:?}");
    assert!(
        !h.detail.unwrap_or_default().contains('/'),
        "no path or address"
    );
    client.shutdown();
    assert!(!client.is_open());
    assert_eq!(client.health().state, HealthState::Unavailable);
}

#[test]
fn a_served_thread_reports_turns_and_authoritative_items_only() {
    let fake = Fake::spawn();
    let events = Events::default();
    let client = fake.connect(&events);
    let thread = fake.create_thread();
    let key = client.subscribe(&thread).expect("subscribe");
    // A second subscribe sends nothing: one subscription per thread, never per message.
    assert_eq!(client.subscribe(&thread).unwrap(), key);
    let resumes: Vec<Json> = fake
        .client_calls()
        .into_iter()
        .filter(|c| method_of(c) == "thread/resume")
        .collect();
    assert_eq!(resumes.len(), 1, "{resumes:?}");
    let mut members: Vec<String> = get(&resumes[0], "params")
        .and_then(Json::as_object)
        .unwrap()
        .names()
        .map(str::to_owned)
        .collect();
    members.sort_unstable();
    assert_eq!(members, ["excludeTurns", "threadId"], "[MCPB-CDX-006]");

    fake.tui_turn(&thread, "the TUI user's own words");
    events.wait_for("the TUI's user message", |ev| {
        ev.iter().any(|e| matches!(e, CodexEvent::ItemCompleted { thread: t, item: CompletedItem::UserMessage { text, .. }, .. } if *t == key && text == "the TUI user's own words"))
    });
    assert!(client.threads()[0].turn_running);
    fake.complete_turn(&thread, "OAC-G6-ACK final text");
    events.wait_for("the turn's end", |ev| {
        ev.iter().any(|e| matches!(e, CodexEvent::TurnCompleted { thread: t, end: TurnEnd::Completed, .. } if *t == key))
    });
    let ev = events.all();
    let agent: Vec<&str> = ev
        .iter()
        .filter_map(|e| match e {
            CodexEvent::ItemCompleted {
                item: CompletedItem::AgentMessage { text, .. },
                ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(agent, ["OAC-G6-ACK final text"], "from item/completed only");
    assert!(client.diagnostics().deltas >= 1, "the delta was dropped");
    assert!(ev.iter().any(|e| matches!(
        e,
        CodexEvent::Status {
            state: ThreadState::Active,
            ..
        }
    )));
    let v = &client.threads()[0];
    assert!(!v.turn_running);
    assert_eq!(v.state, ThreadState::Idle);
    // Nothing the client reports names the thread.
    let shown = format!(
        "{ev:?} {:?} {:?} {client:?}",
        client.threads(),
        client.health()
    );
    assert!(!shown.contains(&thread), "{shown}");
    let d = client.diagnostics();
    assert_eq!(
        (d.malformed, d.unparseable, d.server_requests),
        (0, 0, 0),
        "{d:?}"
    );
    client.shutdown();
}

#[test]
fn the_shim_holds_input_with_thread_queue_add_and_never_steers() {
    let fake = Fake::spawn();
    let events = Events::default();
    let client = fake.connect(&events);
    let thread = fake.create_thread();
    let key = client.subscribe(&thread).unwrap();
    let id = client.next_client_user_message_id();
    assert_ne!(id, client.next_client_user_message_id());
    client
        .queue_add(key, "held for the thread", &id)
        .expect("queued");
    // An idle thread whose last turn completed takes the add at once (S3 L65-L73).
    events.wait_for("the queued input as a user message", |ev| {
        ev.iter().any(|e| matches!(e, CodexEvent::ItemCompleted { item: CompletedItem::UserMessage { text, client_id, .. }, .. } if text == "held for the thread" && client_id.as_deref() == Some(id.as_str())))
    });
    fake.complete_turn(&thread, "done");

    // The recorded archived refusal: reported as a refusal, with no other call after it.
    fake.control(
        "oacFake/thread/setArchived",
        &format!("{{\"threadId\":{},\"archived\":true}}", js(&thread)),
    );
    let before = fake.client_calls().len();
    let e = client
        .queue_add(key, "turned away", &client.next_client_user_message_id())
        .unwrap_err();
    assert!(
        matches!(e, ClientError::Refused { code: -32600, .. }),
        "{e}"
    );
    assert!(!format!("{e} {e:?}").contains(&thread));
    let calls = fake.client_calls();
    assert_eq!(
        calls.len(),
        before + 1,
        "one call for the refused add, nothing after it"
    );

    for c in &calls {
        assert_ne!(flag(c, "steering"), Some(true), "{}", c.to_compact());
        assert_ne!(
            flag(c, "experimentalGateRefused"),
            Some(true),
            "{}",
            c.to_compact()
        );
        let overrides = get(c, "flags")
            .and_then(|f| get(f, "overrideMembers"))
            .and_then(Json::as_array)
            .map_or(0, <[Json]>::len);
        assert_eq!(overrides, 0, "[MCPB-CDX-005]: {}", c.to_compact());
    }
    let adds: Vec<&Json> = calls
        .iter()
        .filter(|c| flag(c, "handOff") == Some(true))
        .collect();
    assert_eq!(adds.len(), 2);
    assert!(adds.iter().all(|c| method_of(c) == "thread/queue/add"));
    client.shutdown();
}

#[test]
fn values_that_are_not_thread_ids_are_never_sent() {
    let fake = Fake::spawn();
    let client = fake.connect(&Events::default());
    let before = fake.client_calls().len();
    for bad in ["", "not-a-thread", "\",\"model\":\"x"] {
        assert_eq!(client.subscribe(bad), Err(ClientError::InvalidThreadId));
    }
    assert_eq!(fake.client_calls().len(), before);
    // A well-formed id no thread has: the recorded refusal, and the thread is not served.
    let e = client
        .subscribe("01a1186d-0000-7000-8000-000000000000")
        .unwrap_err();
    assert!(
        matches!(e, ClientError::Refused { code: -32600, .. }),
        "{e}"
    );
    assert!(client.threads().is_empty());
    client.shutdown();
}

#[test]
fn unsubscribing_stops_the_events() {
    let fake = Fake::spawn();
    let events = Events::default();
    let client = fake.connect(&events);
    let thread = fake.create_thread();
    let key = client.subscribe(&thread).unwrap();
    client.unsubscribe(key).unwrap();
    assert_eq!(client.unsubscribe(key), Err(ClientError::NotServed));
    assert!(client.key_of(&thread).is_none());
    let n = events.all().len();
    // The fake does not model thread/unsubscribe, so it still sends this thread's events
    // to the carrier, as live Codex can (row 70): the client's filter drops them.
    fake.tui_turn(&thread, "after unsubscribe");
    fake.complete_turn(&thread, "x");
    // Frames on the carrier arrive in order, so once the answer to a later call is in,
    // every notification sent before it has been dispatched. No sleep.
    let other = fake.create_thread();
    client.subscribe(&other).unwrap();
    assert!(client.diagnostics().not_served > 0);
    assert_eq!(events.all().len(), n, "{:?}", events.all());
    client.shutdown();
}

#[test]
fn loaded_threads_lists_ids_only_and_never_calls_thread_list() {
    let fake = Fake::spawn();
    let client = fake.connect(&Events::default());
    assert!(client.loaded_threads().unwrap().is_empty());
    let a = fake.create_thread();
    let b = fake.create_thread();
    let mut ids = client.loaded_threads().unwrap();
    ids.sort();
    let mut want = vec![a, b];
    want.sort();
    assert_eq!(ids, want);
    // Listing serves nothing: subscribing is a separate step.
    assert!(client.threads().is_empty());
    let calls = fake.client_calls();
    assert!(calls.iter().all(|c| method_of(c) != "thread/list"));
    assert_eq!(
        calls
            .iter()
            .filter(|c| method_of(c) == "thread/loaded/list")
            .count(),
        2
    );
    client.shutdown();
}
