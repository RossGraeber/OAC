// SPDX-License-Identifier: Apache-2.0

//! The recorded fixtures the fake replays, and the values it takes from them.
//!
//! Every fixture is compiled in from its Stage 1 capture location,
//! `docs/planning/gates/fixtures/`, in place: nothing is copied, so a fixture and the fake
//! cannot drift apart. Each value the fake sends is read out of a named line of a named
//! fixture, and the line's method is checked as it is read, so an edit that moves or
//! changes a line fails loudly ([`Evidence::get`] panics) instead of changing what the
//! fake does.
//!
//! | Value | Fixture and line |
//! |---|---|
//! | `server/discover` probe | D6 line 3 (and G4 line 5, same client version) |
//! | legacy `initialize` request | D6 line 5 |
//! | `notifications/initialized` | D6 line 7 |
//! | legacy `tools/list` request | D6 line 9 |
//! | modern `tools/list` request, after a successful probe | G4 line 12 |
//! | `tools/call` shape (`_meta` with `claudecode/toolUseId`, `progressToken`) | D6 line 13 (and G1 Box C line 24) |
//! | mid-turn wrapper around a `<channel>` tag | G5 rendered line 13 |
//!
//! The fixture paths and versions are listed in
//! `docs/planning/gates/fixtures/MANIFEST.json`.

use std::sync::OnceLock;

use oac_core::json::{self, Json};

/// D6 Claude protocol capture, Claude Code `2.1.283`, 2026-09-28.
pub const D6: &str = include_str!(
    "../../../../docs/planning/gates/fixtures/d6-claude-protocol/transcript-2026-09-28.jsonl"
);
/// G1 Box C, the verdict-bearing G1 run, Claude Code `2.1.283`, 2026-09-28.
pub const G1_BOX_C: &str = include_str!(
    "../../../../docs/planning/gates/fixtures/g1-claude-wake/transcript-2026-09-28-2.1.283-boxC.jsonl"
);
/// G4 dual-era run, Claude Code `2.1.283`, 2026-09-26.
pub const G4: &str = include_str!(
    "../../../../docs/planning/gates/fixtures/g4-mcp-dual-era/transcript-2026-09-26.jsonl"
);
/// G5 Claude wire transcript, Claude Code `2.1.283`, 2026-09-27.
pub const G5_WIRE: &str = include_str!(
    "../../../../docs/planning/gates/fixtures/g5-provenance/transcript-claude-2026-09-27.jsonl"
);
/// G5 harness-rendered `<channel>` records, extracted from the same session's log.
pub const G5_RENDERED: &str = include_str!(
    "../../../../docs/planning/gates/fixtures/g5-provenance/claude-rendered-2026-09-27.jsonl"
);

/// Line `n` (1-based) of a JSONL fixture, parsed.
///
/// # Panics
///
/// When the line does not exist or is not JSON: the fixture changed under the fake.
pub fn line(fixture: &str, n: usize) -> Json {
    let text = fixture
        .lines()
        .nth(n.checked_sub(1).expect("fixture lines are 1-based"))
        .unwrap_or_else(|| panic!("fixture has no line {n}"));
    json::parse(text.as_bytes()).unwrap_or_else(|e| panic!("fixture line {n} is not JSON: {e:?}"))
}

/// The `payload` of line `n` of a wire transcript: one JSON-RPC message.
///
/// # Panics
///
/// When the line has no `payload` object.
pub fn payload(fixture: &str, n: usize) -> Json {
    let rec = line(fixture, n)
        .into_object()
        .expect("fixture line is an object");
    let p = rec
        .get("payload")
        .unwrap_or_else(|| panic!("fixture line {n} has no payload"));
    p.clone()
}

/// The `direction` of line `n` of a wire transcript (`client->server`, `server->client`
/// or `spike`).
pub fn direction(fixture: &str, n: usize) -> String {
    let rec = line(fixture, n);
    member_str(&rec, "direction").unwrap_or_default().to_owned()
}

/// A string member of an object value.
pub fn member_str<'a>(v: &'a Json, name: &str) -> Option<&'a str> {
    v.as_object()?.get(name)?.as_str()
}

/// A member of an object value.
pub fn member<'a>(v: &'a Json, name: &str) -> Option<&'a Json> {
    v.as_object()?.get(name)
}

fn expect_method(fixture: &str, n: usize, method: &str) -> Json {
    let p = payload(fixture, n);
    assert_eq!(
        member_str(&p, "method"),
        Some(method),
        "fixture line {n} is no longer a `{method}` frame"
    );
    p
}

/// The values the fake takes from the fixtures, read once.
#[derive(Debug)]
pub struct Evidence {
    /// The `server/discover` probe Claude Code opens with (D6 line 3).
    pub discover_probe: Json,
    /// The legacy `initialize` request sent after the probe is refused (D6 line 5).
    pub initialize: Json,
    /// `notifications/initialized` (D6 line 7).
    pub initialized: Json,
    /// The legacy `tools/list` request (D6 line 9).
    pub tools_list_legacy: Json,
    /// The modern `tools/list` request sent after a successful probe (G4 line 12).
    pub tools_list_modern: Json,
    /// The recorded `tools/call` frame whose shape every call reproduces (D6 line 13).
    pub tools_call: Json,
    /// The mid-turn wrapper: the text before the `<channel>` tag, with the configured
    /// source name split out, and the text after it (G5 rendered line 13).
    pub midturn: MidTurnWrapper,
}

/// The mid-turn wrapper, split around the tag and the source name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MidTurnWrapper {
    /// Text before the source name.
    pub before_source: String,
    /// Text between the source name and the tag.
    pub after_source: String,
    /// Text after the tag.
    pub after_tag: String,
}

impl MidTurnWrapper {
    /// The wrapper around `tag`, naming `source`.
    pub fn wrap(&self, source: &str, tag: &str) -> String {
        format!(
            "{}{source}{}{tag}{}",
            self.before_source, self.after_source, self.after_tag
        )
    }
}

impl Evidence {
    /// The evidence, read and checked on first use.
    ///
    /// # Panics
    ///
    /// When a fixture line no longer has the shape this module reads from it.
    pub fn get() -> &'static Evidence {
        static EVIDENCE: OnceLock<Evidence> = OnceLock::new();
        EVIDENCE.get_or_init(Evidence::read)
    }

    fn read() -> Evidence {
        let discover_probe = expect_method(D6, 3, "server/discover");
        let g4_probe = expect_method(G4, 5, "server/discover");
        assert_eq!(
            discover_probe, g4_probe,
            "D6 line 3 and G4 line 5 record the same probe"
        );
        let initialize = expect_method(D6, 5, "initialize");
        let initialized = expect_method(D6, 7, "notifications/initialized");
        let tools_list_legacy = expect_method(D6, 9, "tools/list");
        let tools_list_modern = expect_method(G4, 12, "tools/list");
        let tools_call = expect_method(D6, 13, "tools/call");
        // The member order `FakeClaude::call_tool` writes.
        let names = |v: Option<&Json>| -> Vec<String> {
            v.and_then(Json::as_object)
                .map(|o| o.names().map(str::to_owned).collect())
                .unwrap_or_default()
        };
        let params = member(&tools_call, "params");
        assert_eq!(
            (
                names(Some(&tools_call)),
                names(params),
                names(params.and_then(|p| member(p, "_meta")))
            ),
            (
                vec![
                    "method".into(),
                    "params".into(),
                    "jsonrpc".into(),
                    "id".into()
                ],
                vec!["name".into(), "arguments".into(), "_meta".into()],
                vec!["claudecode/toolUseId".into(), "progressToken".into()]
            ),
            "D6 line 13 tools/call member order"
        );
        for (n, d) in [
            (3, "client->server"),
            (5, "client->server"),
            (7, "client->server"),
            (9, "client->server"),
            (13, "client->server"),
        ] {
            assert_eq!(direction(D6, n), d, "D6 line {n} direction");
        }
        assert_eq!(direction(G4, 12), "client->server", "G4 line 12 direction");
        Evidence {
            discover_probe,
            initialize,
            initialized,
            tools_list_legacy,
            tools_list_modern,
            tools_call,
            midturn: read_midturn_wrapper(),
        }
    }
}

/// G5 rendered line 13 is the one recorded mid-turn render: `prompt` is the `<channel>`
/// tag, `rendered[0]` the text the session showed the model. The wrapper is what
/// surrounds the tag, with the source name (`g5spike`) split out of the text before it.
fn read_midturn_wrapper() -> MidTurnWrapper {
    let rec = line(G5_RENDERED, 13);
    assert_eq!(
        member_str(&rec, "kind"),
        Some("midturn"),
        "G5 rendered line 13 kind"
    );
    let tag = member_str(&rec, "prompt").expect("G5 rendered line 13 prompt");
    let rendered = member(&rec, "rendered")
        .and_then(Json::as_array)
        .and_then(|a| a.first())
        .and_then(Json::as_str)
        .expect("G5 rendered line 13 rendered[0]");
    let source = member(&rec, "channelTag")
        .and_then(|t| member_str(t, "source"))
        .expect("G5 rendered line 13 channelTag.source");
    let (before_tag, after_tag) = rendered
        .split_once(tag)
        .expect("the tag appears in the render");
    assert!(!after_tag.contains(tag), "the tag appears once");
    let mut parts = before_tag.split(source);
    let (before_source, after_source) = match (parts.next(), parts.next(), parts.next()) {
        (Some(a), Some(b), None) => (a.to_owned(), b.to_owned()),
        _ => panic!("the source name appears exactly once before the tag"),
    };
    assert!(
        !after_tag.contains(source),
        "the source name does not appear after the tag"
    );
    MidTurnWrapper {
        before_source,
        after_source,
        after_tag: after_tag.to_owned(),
    }
}
