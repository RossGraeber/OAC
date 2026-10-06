// SPDX-License-Identifier: Apache-2.0

//! Replays each recorded fixture through the fake: the server's recorded frames go in, and
//! what comes out must be what Claude Code recorded (frames) or rendered (tags). Then the
//! paths no fixture shows must stop the fake rather than be guessed.

use oac_core::json::{self, Json};
use oac_fake_claude::evidence::{self, D6, G1_BOX_C, G4, G5_RENDERED, G5_WIRE, member, member_str};
use oac_fake_claude::{
    CallOutcome, ChannelTag, Config, Era, FakeClaude, FakeError, IgnoredReason, MidTurnRelease,
    Phase, RenderGap, SessionEvent,
};

/// A recorded server->client frame, as the server would write it.
fn server(fixture: &str, n: usize) -> String {
    assert_eq!(
        evidence::direction(fixture, n),
        "server->client",
        "line {n}"
    );
    evidence::payload(fixture, n).to_compact()
}

/// A recorded client->server frame.
fn client(fixture: &str, n: usize) -> Json {
    assert_eq!(
        evidence::direction(fixture, n),
        "client->server",
        "line {n}"
    );
    evidence::payload(fixture, n)
}

fn parsed(frames: Vec<String>) -> Vec<Json> {
    frames
        .iter()
        .map(|f| json::parse(f.as_bytes()).expect("the fake writes JSON"))
        .collect()
}

/// Opens a legacy session: probe refused, initialize answered, tools listed. `lines` are the
/// recorded probe error, initialize result and tools/list result; every client frame the
/// fake writes on the way must equal the recorded one at `expect`.
fn open_legacy(fake: &mut FakeClaude, fixture: &str, lines: [usize; 3], expect: [usize; 4]) {
    assert_eq!(
        parsed(fake.take_outbound()),
        vec![client(fixture, expect[0])],
        "probe"
    );
    fake.receive(&server(fixture, lines[0]));
    assert_eq!(
        parsed(fake.take_outbound()),
        vec![client(fixture, expect[1])],
        "initialize"
    );
    fake.receive(&server(fixture, lines[1]));
    assert_eq!(
        parsed(fake.take_outbound()),
        vec![client(fixture, expect[2]), client(fixture, expect[3])],
        "initialized, tools/list"
    );
    fake.receive(&server(fixture, lines[2]));
    assert!(fake.take_outbound().is_empty());
    assert_eq!(fake.phase(), Phase::Ready, "{:?}", fake.halted());
    assert_eq!(fake.era(), Some(Era::Legacy));
    assert!(fake.channel_registered());
}

fn wake(e: &SessionEvent) -> &ChannelTag {
    match e {
        SessionEvent::Wake(t) => t,
        other => panic!("expected a wake, got {other:?}"),
    }
}

fn midturn(e: &SessionEvent) -> &ChannelTag {
    match e {
        SessionEvent::MidTurn { tag, .. } => tag,
        other => panic!("expected a mid-turn release, got {other:?}"),
    }
}

/// `arguments` and `claudecode/toolUseId` of a recorded tools/call.
fn recorded_call(fixture: &str, n: usize) -> (String, String, String) {
    let p = client(fixture, n);
    let params = member(&p, "params").expect("params");
    let name = member_str(params, "name").expect("name").to_owned();
    let args = member(params, "arguments").expect("arguments").to_compact();
    let tuid = member(params, "_meta")
        .and_then(|m| member_str(m, "claudecode/toolUseId"))
        .expect("toolUseId")
        .to_owned();
    (name, args, tuid)
}

#[test]
fn replays_the_d6_capture() {
    let mut fake = FakeClaude::new(Config::new("d6claude").expect("valid"));
    open_legacy(&mut fake, D6, [4, 6, 10], [3, 5, 7, 9]);
    assert_eq!(fake.tools(), ["reply"]);

    // Idle wake (line 12): no acknowledgement of any kind, and a turn starts.
    fake.receive(&server(D6, 12));
    assert!(fake.take_outbound().is_empty(), "no acknowledgement");
    let events = fake.take_events();
    assert_eq!(events.len(), 1);
    let t = wake(&events[0]);
    assert_eq!(
        t.attribute_names(),
        ["source", "oac_sender", "oac_message_id"]
    );
    assert_eq!(t.attribute("oac_message_id"), ["d6-wake-1"]);
    assert!(
        t.rendered
            .as_ref()
            .expect("evidenced content")
            .starts_with("<channel source=\"d6claude\" ")
    );
    assert!(fake.in_turn());

    // The model's reply (line 13), byte for byte the recorded frame, and its result.
    let (name, args, tuid) = recorded_call(D6, 13);
    let id = fake.call_tool(&name, &args, Some(&tuid)).expect("call");
    assert_eq!(parsed(fake.take_outbound()), vec![client(D6, 13)]);
    fake.receive(&server(D6, 14));
    assert!(matches!(
        fake.call_outcome(id),
        Some(CallOutcome::Result(_))
    ));
    fake.tool_boundary().expect("boundary");
    fake.end_turn().expect("idle again");

    // Mid-turn pair (lines 16, 18): queued, then released in order at boundaries.
    fake.start_turn().expect("user turn");
    fake.receive(&server(D6, 16));
    fake.receive(&server(D6, 18));
    assert!(fake.take_outbound().is_empty(), "no acknowledgement");
    assert!(
        fake.take_events().is_empty(),
        "nothing interleaves inside a tool call"
    );
    assert_eq!(fake.queued(), 2);
    assert!(
        matches!(fake.end_turn(), Err(FakeError::Unsupported(_))),
        "queued input at turn end is not recorded"
    );
    fake.tool_boundary().expect("boundary");
    fake.tool_boundary().expect("boundary");
    let ids: Vec<_> = fake
        .take_events()
        .iter()
        .map(|e| midturn(e).attribute("oac_message_id")[0].to_owned())
        .collect();
    assert_eq!(ids, ["d6-midturn-1", "d6-midturn-2"]);
    fake.end_turn().expect("idle");

    // Every client frame the fake wrote is one Claude Code recorded, in order.
    let recorded: Vec<Json> = (1..=18)
        .filter(|&n| evidence::direction(D6, n) == "client->server")
        .map(|n| client(D6, n))
        .collect();
    let sent: Vec<Json> = fake
        .transcript()
        .iter()
        .filter(|f| f.direction == oac_fake_claude::Direction::ToServer)
        .map(|f| json::parse(f.text.as_bytes()).expect("json"))
        .collect();
    assert_eq!(sent, recorded);
    assert!(fake.halted().is_none());
}

#[test]
fn replays_g1_box_c_under_both_release_choices() {
    for release in MidTurnRelease::BOTH {
        let mut fake =
            FakeClaude::new(Config::new("g1spike").expect("valid").with_release(release));
        open_legacy(&mut fake, G1_BOX_C, [14, 16, 20], [13, 15, 17, 19]);

        // The server named itself `g1-spike-channel-server`; the tag says the load name.
        let init = evidence::payload(G1_BOX_C, 16);
        let server_name = member(&init, "result")
            .and_then(|r| member(r, "serverInfo"))
            .and_then(|s| member_str(s, "name"));
        assert_eq!(server_name, Some("g1-spike-channel-server"));

        fake.receive(&server(G1_BOX_C, 21));
        let events = fake.take_events();
        let t = wake(&events[0]);
        // G1-result criterion 2: exactly source, oac_message_id, oac_sender; the unsafe key dropped.
        assert_eq!(
            t.attribute_names(),
            ["source", "oac_message_id", "oac_sender"]
        );
        assert_eq!(t.attribute("source"), ["g1spike"]);
        assert_eq!(t.dropped_keys, ["not identifier safe!"]);
        // Its content holds an em dash, which no recorded render contains: text not guessed.
        assert!(matches!(t.rendered, Err(RenderGap::Content(_))));

        // Both mid-turn notifications arrive during the first sleep of the turn.
        fake.receive(&server(G1_BOX_C, 22));
        fake.receive(&server(G1_BOX_C, 23));
        assert!(fake.take_events().is_empty());
        fake.tool_boundary().expect("sleep 1 done");
        let first: Vec<_> = fake
            .take_events()
            .iter()
            .map(|e| midturn(e).attribute("oac_message_id")[0].to_owned())
            .collect();
        fake.tool_boundary().expect("sleep 2 done");
        let second: Vec<_> = fake
            .take_events()
            .iter()
            .map(|e| midturn(e).attribute("oac_message_id")[0].to_owned())
            .collect();
        match release {
            // Box C: one at each boundary.
            MidTurnRelease::OnePerBoundary => {
                assert_eq!(first, ["g1-spike-midturn-test-2"]);
                assert_eq!(second, ["g1-spike-midturn-test-3"]);
            }
            // Original run: together, in order, at one boundary.
            MidTurnRelease::AllAtBoundary => {
                assert_eq!(
                    first,
                    ["g1-spike-midturn-test-2", "g1-spike-midturn-test-3"]
                );
                assert!(second.is_empty());
            }
        }

        let (name, args, tuid) = recorded_call(G1_BOX_C, 24);
        let id = fake.call_tool(&name, &args, Some(&tuid)).expect("call");
        assert_eq!(id, 2);
        assert_eq!(parsed(fake.take_outbound()), vec![client(G1_BOX_C, 24)]);
        fake.receive(&server(G1_BOX_C, 25));
        assert!(matches!(fake.call_outcome(2), Some(CallOutcome::Result(_))));
        assert!(fake.halted().is_none());
    }
}

/// G5: the wire notification (`wire` line) against what the session rendered (`rendered`
/// line of `claude-rendered-2026-09-27.jsonl`), for every recorded case.
#[test]
fn renders_every_g5_case_as_recorded() {
    let mut fake = FakeClaude::new(Config::new("g5spike").expect("valid"));
    open_legacy(&mut fake, G5_WIRE, [12, 14, 18], [11, 13, 15, 17]);

    // Idle cases C1, C2, C3, C4, C4b, C5.
    for (wire, rendered) in [(21, 1), (24, 3), (27, 5), (30, 7), (33, 9), (36, 11)] {
        fake.receive(&server(G5_WIRE, wire));
        assert!(
            fake.take_outbound().is_empty(),
            "no acknowledgement (line {wire})"
        );
        let events = fake.take_events();
        let t = wake(&events[0]);
        let rec = evidence::line(G5_RENDERED, rendered);
        assert_eq!(member_str(&rec, "kind"), Some("idle"));
        assert_eq!(
            t.rendered.as_deref(),
            Ok(member_str(&rec, "rendered").expect("rendered")),
            "line {wire}"
        );
        fake.end_turn().expect("idle");
    }

    // C6, mid-turn: the tag inside the recorded wrapper.
    fake.start_turn().expect("turn");
    fake.receive(&server(G5_WIRE, 39));
    fake.tool_boundary().expect("boundary");
    let events = fake.take_events();
    let rec = evidence::line(G5_RENDERED, 13);
    let SessionEvent::MidTurn { tag, rendered } = &events[0] else {
        panic!("expected a mid-turn release");
    };
    assert_eq!(
        tag.rendered.as_deref(),
        Ok(member_str(&rec, "prompt").expect("prompt"))
    );
    let want = member(&rec, "rendered")
        .and_then(Json::as_array)
        .and_then(|a| a[0].as_str())
        .expect("rendered[0]");
    assert_eq!(rendered.as_deref(), Ok(want));
    assert!(fake.halted().is_none());
}

#[test]
fn g5_meta_key_cases() {
    let mut fake = FakeClaude::new(Config::new("g5spike").expect("valid"));
    open_legacy(&mut fake, G5_WIRE, [12, 14, 18], [11, 13, 15, 17]);

    // C4: four look-alike keys, all dropped; one sender.
    fake.receive(&server(G5_WIRE, 30));
    let events = fake.take_events();
    let t = wake(&events[0]);
    assert_eq!(
        t.dropped_keys,
        ["oac-sender", "oac.sender", "oac sender", "oac_s\u{e9}nder"]
    );
    assert_eq!(t.attribute("oac_sender"), ["d5sm08qy8w80j52v1hxmaw79sd"]);
    fake.end_turn().expect("idle");

    // C4b: the sender only under an unsafe key, so no sender at all.
    fake.receive(&server(G5_WIRE, 33));
    let events = fake.take_events();
    let t = wake(&events[0]);
    assert!(t.attribute("oac_sender").is_empty());
    fake.end_turn().expect("idle");

    // C5: a meta key named source is kept as a second, trailing source.
    fake.receive(&server(G5_WIRE, 36));
    let events = fake.take_events();
    assert_eq!(wake(&events[0]).attribute("source"), ["g5spike", "alice"]);
}

#[test]
fn g4_a_successful_probe_is_never_a_channel() {
    // Process 18032: answers the probe as modern-only (line 7).
    let mut fake = FakeClaude::new(Config::new("g4spike").expect("valid"));
    assert_eq!(parsed(fake.take_outbound()), vec![client(G4, 5)]);
    fake.receive(&server(G4, 7));
    assert_eq!(
        parsed(fake.take_outbound()),
        vec![client(G4, 12)],
        "modern tools/list, no initialize"
    );
    fake.receive(&server(G4, 13));
    assert_eq!(fake.phase(), Phase::Ready);
    assert_eq!(fake.era(), Some(Era::Modern));
    assert!(!fake.channel_registered());
    fake.receive(&server(G4, 22));
    assert!(fake.take_outbound().is_empty());
    assert!(matches!(
        fake.take_events()[..],
        [SessionEvent::Ignored {
            reason: IgnoredReason::ModernEra,
            ..
        }]
    ));
    assert!(!fake.in_turn(), "an ignored notification wakes nothing");
    fake.start_turn().expect("turn");
    assert!(matches!(
        fake.call_tool("g4_echo", "{\"text\":\"x\"}", None),
        Err(FakeError::Unsupported(_))
    ));

    // Process 19680: refuses the probe (line 8) and is a channel (lines 14-21, 23).
    let mut fake = FakeClaude::new(Config::new("g4spike").expect("valid"));
    assert_eq!(parsed(fake.take_outbound()), vec![client(G4, 6)]);
    fake.receive(&server(G4, 8));
    assert_eq!(parsed(fake.take_outbound()), vec![client(G4, 14)]);
    fake.receive(&server(G4, 15));
    assert_eq!(
        parsed(fake.take_outbound()),
        vec![client(G4, 18), client(G4, 20)]
    );
    fake.receive(&server(G4, 21));
    fake.receive(&server(G4, 23));
    assert!(fake.take_outbound().is_empty());
    let events = fake.take_events();
    assert_eq!(wake(&events[0]).attribute("g4_stdio_era"), ["legacy"]);
}

#[test]
fn a_synthetic_tool_use_id_keeps_the_recorded_shape() {
    let mut fake = FakeClaude::new(Config::new("d6claude").expect("valid"));
    open_legacy(&mut fake, D6, [4, 6, 10], [3, 5, 7, 9]);
    assert!(matches!(
        fake.call_tool("reply", "{}", None),
        Err(FakeError::NotInTurn)
    ));
    fake.start_turn().expect("turn");
    assert!(matches!(
        fake.call_tool("nope", "{}", None),
        Err(FakeError::UnknownTool(_))
    ));
    assert!(matches!(
        fake.call_tool("reply", "[]", None),
        Err(FakeError::BadArguments)
    ));
    let id = fake
        .call_tool("reply", "{\"message\":\"hi\"}", None)
        .expect("call");
    let frame = &parsed(fake.take_outbound())[0];
    let meta = member(frame, "params")
        .and_then(|p| member(p, "_meta"))
        .expect("_meta");
    let tuid = member_str(meta, "claudecode/toolUseId").expect("toolUseId");
    assert!(tuid.starts_with("toolu_") && tuid.len() == "toolu_".len() + 24);
    assert_eq!(
        member(meta, "progressToken")
            .and_then(Json::as_number)
            .map(|n| n.raw().to_owned()),
        Some(id.to_string())
    );
    assert_eq!(FakeClaude::client_version(), "2.1.283");
}

/// A legacy-opened fake, from the D6 capture.
fn ready() -> FakeClaude {
    let mut fake = FakeClaude::new(Config::new("d6claude").expect("valid"));
    open_legacy(&mut fake, D6, [4, 6, 10], [3, 5, 7, 9]);
    fake
}

#[test]
fn unrecorded_server_behaviour_halts_the_fake() {
    let cases: [(&str, &str); 9] = [
        (
            "no jsonrpc member",
            "{\"method\":\"notifications/claude/channel\",\"params\":{\"content\":\"x\",\"meta\":{\"k\":\"v\"}}}",
        ),
        (
            "jsonrpc 1.0",
            "{\"jsonrpc\":\"1.0\",\"method\":\"notifications/claude/channel\",\"params\":{\"content\":\"x\",\"meta\":{\"k\":\"v\"}}}",
        ),
        (
            "duplicate meta key",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/claude/channel\",\"params\":{\"content\":\"x\",\"meta\":{\"k\":\"a\",\"k\":\"b\"}}}",
        ),
        (
            "server request",
            "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"roots/list\"}",
        ),
        ("ping", "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}"),
        (
            "other notification",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/tools/list_changed\"}",
        ),
        (
            "unknown id",
            "{\"jsonrpc\":\"2.0\",\"id\":99,\"result\":{}}",
        ),
        (
            "empty meta key",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/claude/channel\",\"params\":{\"content\":\"x\",\"meta\":{\"\":\"v\"}}}",
        ),
        (
            "no meta",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/claude/channel\",\"params\":{\"content\":\"x\"}}",
        ),
    ];
    for (name, frame) in cases {
        let mut fake = ready();
        fake.receive(frame);
        assert_eq!(fake.phase(), Phase::Halted, "{name}");
        assert!(fake.take_outbound().is_empty(), "{name}: nothing answered");
        assert!(
            matches!(fake.start_turn(), Err(FakeError::Halted(_))),
            "{name}"
        );
        // Once halted, later frames are logged and not acted on.
        fake.receive(&server(D6, 12));
        assert!(fake.events().is_empty(), "{name}");
    }
    let mut fake = ready();
    fake.receive("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/claude/channel\",\"params\":{\"content\":\"x\",\"meta\":{\"k\":\"a\",\"k\":\"b\"}}}");
    assert!(
        fake.halted()
            .expect("halted")
            .what
            .contains("repeats member name \"k\""),
        "a clear message for a duplicate key"
    );

    // A notification before the opening completes.
    let mut fake = FakeClaude::new(Config::new("d6claude").expect("valid"));
    fake.receive(&server(D6, 12));
    assert_eq!(fake.phase(), Phase::Halted);

    // A probe error other than -32601 (here the recognized modern -32022).
    let mut fake = FakeClaude::new(Config::new("d6claude").expect("valid"));
    fake.receive("{\"jsonrpc\":\"2.0\",\"id\":\"server-discover-probe-1\",\"error\":{\"code\":-32022,\"message\":\"m\"}}");
    assert_eq!(fake.phase(), Phase::Halted);

    // initialize answered at the modern revision.
    let mut fake = FakeClaude::new(Config::new("d6claude").expect("valid"));
    fake.receive(&server(D6, 4));
    fake.take_outbound();
    fake.receive("{\"jsonrpc\":\"2.0\",\"id\":0,\"result\":{\"protocolVersion\":\"2026-07-28\",\"capabilities\":{\"experimental\":{\"claude/channel\":{}}}}}");
    assert_eq!(fake.phase(), Phase::Halted);
}

#[test]
fn a_server_without_the_channel_capability_halts_the_fake() {
    let mut fake = FakeClaude::new(Config::new("d6claude").expect("valid"));
    fake.take_outbound();
    fake.receive(&server(D6, 4));
    fake.take_outbound();
    fake.receive("{\"jsonrpc\":\"2.0\",\"id\":0,\"result\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{\"tools\":{}},\"serverInfo\":{\"name\":\"x\",\"version\":\"0\"}}}");
    assert_eq!(fake.phase(), Phase::Halted);
    assert!(!fake.channel_registered());
    assert!(
        fake.take_outbound().is_empty(),
        "no initialized, no tools/list"
    );
}

#[test]
fn the_harness_never_polls_and_never_acknowledges() {
    let mut fake = ready();
    for n in [12, 16, 18] {
        fake.receive(&server(D6, n));
        assert!(fake.take_outbound().is_empty());
    }
    assert_eq!(
        fake.methods_sent(),
        [
            "server/discover",
            "initialize",
            "notifications/initialized",
            "tools/list"
        ]
    );
    assert!(fake.tools_called().is_empty(), "no inbox-fetch tool call");
    // The wake (line 12) started a turn; the model replies in it.
    fake.call_tool("reply", "{\"message\":\"hi\"}", None)
        .expect("call");
    assert_eq!(fake.tools_called(), ["reply"]);
}

#[test]
fn load_names_outside_the_recorded_shape_are_refused() {
    for bad in ["", "G1spike", "g1-spike", "g1 spike", "g1\"spike"] {
        assert!(
            matches!(Config::new(bad), Err(FakeError::InvalidSource(_))),
            "{bad:?}"
        );
    }
}
