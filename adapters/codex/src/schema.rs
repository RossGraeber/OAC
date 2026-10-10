// SPDX-License-Identifier: Apache-2.0

//! Message shapes, from the checked-in Codex app-server schema (G6 acceptance item 1; D5,
//! `docs/planning/decisions/G-7-stage4-dependencies.md` §8.2).
//!
//! No Codex crate is a dependency ([ADR-001 Boundary]; G-7 §2), so each shape the client
//! sends or reads is written out here by hand, as a [`Shape`] that names the vendored
//! schema file and definition it comes from:
//! `docs/planning/vendor/codex-app-server-protocol/rust-v0.161.0/json/`. The test
//! `tests/schema.rs` reads those files at run time and fails when a shape here differs from
//! them: a `required` list that is not the schema's, a member the client relies on that the
//! schema does not define with that type, or a member the client sends that the schema does
//! not have. The schema is therefore the source of truth, and this file a checked copy.
//!
//! Experimental methods are not in that default schema. Their shapes are in
//! [`crate::shim`], with where each was read.
//!
//! **At run time** a message is checked twice ([`Shape::check`]): a member the client
//! relies on that is missing or of the wrong type rejects the message; any other required
//! member that is missing is schema drift, counted and reported in `health` as a count,
//! never a gate. Codex floats (#216), so drift is a warning to follow up, not a failure.

use oac_core::json::Json;

/// The schema snapshot the shapes were checked against.
pub const SCHEMA_SNAPSHOT: &str = "rust-v0.161.0";

/// The JSON type of a member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A string.
    Str,
    /// An object.
    Obj,
    /// An array.
    Arr,
    /// A number.
    Num,
    /// A boolean.
    Bool,
}

impl Kind {
    /// The JSON Schema `type` name.
    pub fn schema_name(self) -> &'static str {
        match self {
            Kind::Str => "string",
            Kind::Obj => "object",
            Kind::Arr => "array",
            Kind::Num => "integer",
            Kind::Bool => "boolean",
        }
    }

    fn holds(self, v: &Json) -> bool {
        match self {
            Kind::Str => v.as_str().is_some(),
            Kind::Obj => v.as_object().is_some(),
            Kind::Arr => v.as_array().is_some(),
            Kind::Num => v.as_number().is_some(),
            Kind::Bool => v.as_bool().is_some(),
        }
    }
}

/// One message shape: where in the vendored schema it is, what the schema requires, which
/// members the client relies on, and which members the client sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// A short name for diagnostics.
    pub name: &'static str,
    /// The schema file, relative to the snapshot's `json/` directory.
    pub file: &'static str,
    /// The definition inside the file (`#/definitions/<name>`), or `None` for the file's
    /// top-level schema. For a `oneOf` variant, `name/type-value` picks the variant whose
    /// `type` enum is that value (`ThreadItem/userMessage`).
    pub definition: Option<&'static str>,
    /// The schema's `required` list, sorted.
    pub required: &'static [&'static str],
    /// Members the client reads, with their type. Each must be defined by the schema.
    pub relied_on: &'static [(&'static str, Kind)],
    /// Members the client sends, when the client builds this shape. Each must be defined.
    pub sent: &'static [&'static str],
}

/// What [`Shape::check`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Checked {
    /// Relied-on members that are missing or of the wrong type: the message is rejected.
    pub broken: Vec<&'static str>,
    /// Other required members that are missing: schema drift, counted only.
    pub drift: Vec<&'static str>,
}

impl Checked {
    /// True when the message may be used.
    pub fn usable(&self) -> bool {
        self.broken.is_empty()
    }
}

impl Shape {
    /// Check `v` against the shape.
    pub fn check(&self, v: &Json) -> Checked {
        let mut out = Checked::default();
        let Some(o) = v.as_object() else {
            out.broken.push("(not an object)");
            return out;
        };
        for (name, kind) in self.relied_on {
            if !o.get(name).is_some_and(|m| kind.holds(m)) {
                out.broken.push(name);
            }
        }
        for name in self.required {
            if !o.contains(name) && !out.broken.contains(name) {
                out.drift.push(name);
            }
        }
        out
    }
}

// ---- requests the client sends ---------------------------------------------------------

/// `initialize` params (`v1/InitializeParams.json`).
pub const INITIALIZE_PARAMS: Shape = Shape {
    name: "InitializeParams",
    file: "v1/InitializeParams.json",
    definition: None,
    required: &["clientInfo"],
    relied_on: &[],
    sent: &["capabilities", "clientInfo"],
};

/// `clientInfo` of `initialize`.
pub const CLIENT_INFO: Shape = Shape {
    name: "ClientInfo",
    file: "v1/InitializeParams.json",
    definition: Some("ClientInfo"),
    required: &["name", "version"],
    relied_on: &[],
    sent: &["name", "title", "version"],
};

/// `capabilities` of `initialize`. Only `experimentalApi` is sent, and only because the
/// experimental shim needs it ([`crate::shim`]).
pub const INITIALIZE_CAPABILITIES: Shape = Shape {
    name: "InitializeCapabilities",
    file: "v1/InitializeParams.json",
    definition: Some("InitializeCapabilities"),
    required: &[],
    relied_on: &[],
    sent: &["experimentalApi"],
};

/// `thread/resume` params. Only `threadId` and `excludeTurns`: no member that sets a thread
/// or turn setting (`spec/bindings/mcp.md` [MCPB-CDX-006]); G2 subscribed with exactly these
/// two.
pub const THREAD_RESUME_PARAMS: Shape = Shape {
    name: "ThreadResumeParams",
    file: "v2/ThreadResumeParams.json",
    definition: None,
    required: &["threadId"],
    relied_on: &[],
    sent: &["excludeTurns", "threadId"],
};

/// `thread/unsubscribe` params.
pub const THREAD_UNSUBSCRIBE_PARAMS: Shape = Shape {
    name: "ThreadUnsubscribeParams",
    file: "v2/ThreadUnsubscribeParams.json",
    definition: None,
    required: &["threadId"],
    relied_on: &[],
    sent: &["threadId"],
};

/// A text input item (`UserInput` variant `text`), as the experimental queue add sends it.
pub const TEXT_USER_INPUT: Shape = Shape {
    name: "TextUserInput",
    file: "v2/TurnStartParams.json",
    definition: Some("UserInput/text"),
    required: &["text", "type"],
    relied_on: &[],
    sent: &["text", "type"],
};

// ---- results and notifications the client reads ----------------------------------------

/// The `initialize` result. Only `userAgent` is read, for the Codex version; `codexHome`
/// is a directory and is never kept ([IFC-TYP-092]).
pub const INITIALIZE_RESPONSE: Shape = Shape {
    name: "InitializeResponse",
    file: "v1/InitializeResponse.json",
    definition: None,
    required: &["codexHome", "platformFamily", "platformOs", "userAgent"],
    relied_on: &[("userAgent", Kind::Str)],
    sent: &[],
};

/// The `thread/resume` result. Only `thread` is read; its `cwd` and settings are not kept.
pub const THREAD_RESUME_RESPONSE: Shape = Shape {
    name: "ThreadResumeResponse",
    file: "v2/ThreadResumeResponse.json",
    definition: None,
    required: &[
        "approvalPolicy",
        "approvalsReviewer",
        "cwd",
        "model",
        "modelProvider",
        "sandbox",
        "thread",
    ],
    relied_on: &[("thread", Kind::Obj)],
    sent: &[],
};

/// A `Thread`. Only `id` is read: never `preview`, `name`, `cwd`, `source` or any
/// originator (`11-risks.md` rows 37, 38, 43), and never `sessionId` (row 22).
pub const THREAD: Shape = Shape {
    name: "Thread",
    file: "v2/ThreadResumeResponse.json",
    definition: Some("Thread"),
    required: &[
        "cliVersion",
        "createdAt",
        "cwd",
        "ephemeral",
        "id",
        "modelProvider",
        "preview",
        "projectId",
        "sessionId",
        "source",
        "status",
        "turns",
        "updatedAt",
    ],
    relied_on: &[("id", Kind::Str)],
    sent: &[],
};

/// `item/started`.
pub const ITEM_STARTED: Shape = Shape {
    name: "ItemStartedNotification",
    file: "v2/ItemStartedNotification.json",
    definition: None,
    required: &["item", "startedAtMs", "threadId", "turnId"],
    relied_on: &[
        ("item", Kind::Obj),
        ("threadId", Kind::Str),
        ("turnId", Kind::Str),
    ],
    sent: &[],
};

/// `item/completed`: the authoritative report of an item.
pub const ITEM_COMPLETED: Shape = Shape {
    name: "ItemCompletedNotification",
    file: "v2/ItemCompletedNotification.json",
    definition: None,
    required: &["completedAtMs", "item", "threadId", "turnId"],
    relied_on: &[
        ("item", Kind::Obj),
        ("threadId", Kind::Str),
        ("turnId", Kind::Str),
    ],
    sent: &[],
};

/// `turn/started`.
pub const TURN_STARTED: Shape = Shape {
    name: "TurnStartedNotification",
    file: "v2/TurnStartedNotification.json",
    definition: None,
    required: &["threadId", "turn"],
    relied_on: &[("threadId", Kind::Str), ("turn", Kind::Obj)],
    sent: &[],
};

/// `turn/completed`.
pub const TURN_COMPLETED: Shape = Shape {
    name: "TurnCompletedNotification",
    file: "v2/TurnCompletedNotification.json",
    definition: None,
    required: &["threadId", "turn"],
    relied_on: &[("threadId", Kind::Str), ("turn", Kind::Obj)],
    sent: &[],
};

/// A `Turn`.
pub const TURN: Shape = Shape {
    name: "Turn",
    file: "v2/TurnCompletedNotification.json",
    definition: Some("Turn"),
    required: &["id", "items", "status"],
    relied_on: &[("id", Kind::Str), ("status", Kind::Str)],
    sent: &[],
};

/// `thread/status/changed`.
pub const THREAD_STATUS_CHANGED: Shape = Shape {
    name: "ThreadStatusChangedNotification",
    file: "v2/ThreadStatusChangedNotification.json",
    definition: None,
    required: &["status", "threadId"],
    relied_on: &[("status", Kind::Obj), ("threadId", Kind::Str)],
    sent: &[],
};

/// `item/agentMessage/delta`: recognised so that it can be ignored. Deltas are never
/// authoritative; only `item/completed` is.
pub const AGENT_MESSAGE_DELTA: Shape = Shape {
    name: "AgentMessageDeltaNotification",
    file: "v2/AgentMessageDeltaNotification.json",
    definition: None,
    required: &["delta", "itemId", "threadId", "turnId"],
    relied_on: &[("threadId", Kind::Str)],
    sent: &[],
};

/// A `userMessage` item.
pub const USER_MESSAGE_ITEM: Shape = Shape {
    name: "UserMessageThreadItem",
    file: "v2/ItemCompletedNotification.json",
    definition: Some("ThreadItem/userMessage"),
    required: &["content", "id", "type"],
    relied_on: &[("content", Kind::Arr), ("id", Kind::Str)],
    sent: &[],
};

/// An `agentMessage` item.
pub const AGENT_MESSAGE_ITEM: Shape = Shape {
    name: "AgentMessageThreadItem",
    file: "v2/ItemCompletedNotification.json",
    definition: Some("ThreadItem/agentMessage"),
    required: &["id", "text", "type"],
    relied_on: &[("id", Kind::Str), ("text", Kind::Str)],
    sent: &[],
};

/// An `mcpToolCall` item: what the pairing of `spec/bindings/mcp.md` §4.5 reads (G8).
pub const MCP_TOOL_CALL_ITEM: Shape = Shape {
    name: "McpToolCallThreadItem",
    file: "v2/ItemCompletedNotification.json",
    definition: Some("ThreadItem/mcpToolCall"),
    required: &["arguments", "id", "server", "status", "tool", "type"],
    relied_on: &[
        ("id", Kind::Str),
        ("server", Kind::Str),
        ("tool", Kind::Str),
    ],
    sent: &[],
};

/// Any item: only `id` and `type` are read until its type is known.
pub const ANY_ITEM: Shape = Shape {
    name: "ThreadItem",
    file: "v2/ItemCompletedNotification.json",
    definition: Some("ThreadItem/contextCompaction"),
    required: &["id", "type"],
    relied_on: &[("id", Kind::Str), ("type", Kind::Str)],
    sent: &[],
};

/// Every shape above, for the schema test.
pub const ALL: &[Shape] = &[
    INITIALIZE_PARAMS,
    CLIENT_INFO,
    INITIALIZE_CAPABILITIES,
    THREAD_RESUME_PARAMS,
    THREAD_UNSUBSCRIBE_PARAMS,
    TEXT_USER_INPUT,
    INITIALIZE_RESPONSE,
    THREAD_RESUME_RESPONSE,
    THREAD,
    ITEM_STARTED,
    ITEM_COMPLETED,
    TURN_STARTED,
    TURN_COMPLETED,
    TURN,
    THREAD_STATUS_CHANGED,
    AGENT_MESSAGE_DELTA,
    USER_MESSAGE_ITEM,
    AGENT_MESSAGE_ITEM,
    MCP_TOOL_CALL_ITEM,
    ANY_ITEM,
];

#[cfg(test)]
mod tests {
    use super::*;
    use oac_core::json::parse;

    #[test]
    fn a_missing_relied_on_member_rejects_and_other_gaps_are_drift() {
        let v = parse(br#"{"item":{},"threadId":"t","turnId":"u"}"#).unwrap();
        let c = ITEM_COMPLETED.check(&v);
        assert!(c.usable());
        assert_eq!(c.drift, ["completedAtMs"]);
        let v = parse(br#"{"item":{},"threadId":7,"turnId":"u","completedAtMs":1}"#).unwrap();
        let c = ITEM_COMPLETED.check(&v);
        assert_eq!(c.broken, ["threadId"]);
        assert!(c.drift.is_empty());
        assert!(!ITEM_COMPLETED.check(&parse(b"[]").unwrap()).usable());
    }

    #[test]
    fn required_lists_are_sorted_and_relied_on_members_are_typed() {
        for s in ALL {
            let mut r = s.required.to_vec();
            r.sort_unstable();
            assert_eq!(r, s.required, "{}", s.name);
        }
    }
}
