// SPDX-License-Identifier: Apache-2.0

//! The OAC MCP tool surface: `send`, `reply`, `list_sessions` and `whoami`
//! (`spec/bindings/mcp.md` §5), shared by both provider adapters so that every harness sees
//! the same tool names, `inputSchema` and result shape ([MCPB-TOOL-003]). Adapters may not
//! depend on each other (07 §3), so the one definition lives here.
//!
//! It depends on `oac-core` and on `rmcp` (the MCP server side of both adapters, G-7 §3.1),
//! holds no provider-specific type, and invokes no transport operation ([IFC-ADP-001]). It
//! decides nothing: every request it parses goes to the core's request sink, and every result
//! it renders is the core's, unchanged ([IFC-ADP-003], [IFC-ADP-060]).
//!
//! What is here (G4, #65): the tool list ([`tools`], [MCPB-TOOL-001], [MCPB-TOOL-004]),
//! argument parsing with `invalid-request` for a schema failure ([`parse`],
//! [MCPB-TOOL-005]), and the result shapes ([`send_result`], [`discovery_result`],
//! [`refusal`]; [MCPB-TOOL-006] to [MCPB-TOOL-011], [MCPB-TOOL-015]). What is not: session
//! registration, reply correlation through hand-off records and `whoami`'s device
//! fingerprint, which G5 (#66) owns, and the Codex pairing of `spec/bindings/mcp.md` §4.5,
//! which G8 (#69) owns.

use std::sync::Arc;

use oac_core::adapter::{
    Attachment, Correlation, DiscoveryRequestResult, SendRequest, SendRequestResult,
};
use oac_core::delivery::{DeliveryState, ErrorCode};
use oac_core::envelope::{ContentPart, TextPart};
use oac_core::ids::SessionId;
use oac_core::json::Json;
use rmcp::ErrorData;
use rmcp::model::{CallToolResult, ContentBlock, JsonObject, Tool};

/// `send`: send a new message to an addressed session.
pub const SEND: &str = "send";
/// `reply`: reply to a received message.
pub const REPLY: &str = "reply";
/// `list_sessions`: the discovery result for the calling session.
pub const LIST_SESSIONS: &str = "list_sessions";
/// `whoami`: the caller's own session id and device fingerprint, never a credential.
pub const WHOAMI: &str = "whoami";

/// The four tool names, in listed order ([MCPB-TOOL-001]).
pub const TOOL_NAMES: [&str; 4] = [SEND, REPLY, LIST_SESSIONS, WHOAMI];

/// Names the JSON value type of the `rmcp` model through its object type, so that this
/// crate takes no JSON crate of its own (`rmcp` re-exports none without `macros`, which is
/// refused, G-7 §5).
pub trait MapValue {
    /// The value type of the map.
    type Value;
}

impl<V> MapValue for JsonObject<V> {
    type Value = V;
}

/// The JSON value type of the `rmcp` model.
pub type Value = <JsonObject as MapValue>::Value;

// ---- tool definitions ---------------------------------------------------------------------

fn object(members: Vec<(&str, Value)>) -> JsonObject {
    let mut o = JsonObject::new();
    for (k, v) in members {
        o.insert(k.to_owned(), v);
    }
    o
}

fn string_schema(description: &str) -> Value {
    Value::from(object(vec![
        ("type", Value::from("string")),
        ("description", Value::from(description)),
    ]))
}

fn content_schema() -> Value {
    let part = object(vec![
        ("type", Value::from("object")),
        (
            "properties",
            Value::from(object(vec![
                (
                    "type",
                    Value::from(object(vec![("const", Value::from("text"))])),
                ),
                (
                    "text",
                    Value::from(object(vec![
                        ("type", Value::from("string")),
                        ("minLength", Value::from(1u64)),
                    ])),
                ),
            ])),
        ),
        (
            "required",
            Value::from(vec![Value::from("type"), Value::from("text")]),
        ),
    ]);
    Value::from(object(vec![
        ("type", Value::from("array")),
        ("minItems", Value::from(1u64)),
        ("items", Value::from(part)),
        (
            "description",
            Value::from("The message: one or more text parts. It is sent as written."),
        ),
    ]))
}

fn schema(properties: Vec<(&str, Value)>, required: &[&str]) -> Arc<JsonObject> {
    let mut members = vec![
        ("type", Value::from("object")),
        ("properties", Value::from(object(properties))),
    ];
    if !required.is_empty() {
        members.push((
            "required",
            Value::from(required.iter().map(|r| Value::from(*r)).collect::<Vec<_>>()),
        ));
    }
    Arc::new(object(members))
}

/// The four OAC tools, identical on every harness and era ([MCPB-TOOL-001] to
/// [MCPB-TOOL-004], [MCPB-ERA-007]). Unknown arguments are ignored, so a client that adds a
/// member it is not asked for is still served; the sender is never taken from an argument
/// ([SC-ID-162]).
pub fn tools() -> Vec<Tool> {
    vec![
        Tool::new(
            SEND,
            "Send a new OAC message to another session. The result carries the message id and the state accepted-by-adapter: the message was passed on, not that it was read.",
            schema(
                vec![
                    ("to", string_schema("The addressed session id.")),
                    ("content", content_schema()),
                    (
                        "conversation_id",
                        string_schema("Optional: a conversation id to group messages."),
                    ),
                    (
                        "correlation_id",
                        string_schema(
                            "Optional: a value every reply to this message carries back.",
                        ),
                    ),
                ],
                &["to", "content"],
            ),
        ),
        Tool::new(
            REPLY,
            "Reply to an OAC message you received. Name it by its oac_message_id in in_reply_to.",
            schema(
                vec![
                    (
                        "in_reply_to",
                        string_schema("The oac_message_id of the message you are answering."),
                    ),
                    ("content", content_schema()),
                    (
                        "to",
                        string_schema(
                            "Optional: the session to reply to (the oac_sender of the message).",
                        ),
                    ),
                ],
                &["in_reply_to", "content"],
            ),
        ),
        Tool::new(
            LIST_SESSIONS,
            "List the OAC sessions this session may discover.",
            schema(Vec::new(), &[]),
        ),
        Tool::new(
            WHOAMI,
            "Return this session's own OAC session id and device fingerprint. Never a credential.",
            schema(Vec::new(), &[]),
        ),
    ]
}

// ---- arguments ----------------------------------------------------------------------------

/// A parsed tool call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolCall {
    /// `send`.
    Send {
        /// The addressed session.
        to: SessionId,
        /// The content parts, as the harness wrote them.
        content: Vec<ContentPart>,
        /// An optional conversation id. Untrusted harness input.
        conversation_id: Option<String>,
        /// An optional correlation id. Untrusted harness input.
        correlation_id: Option<String>,
    },
    /// `reply`.
    Reply {
        /// The requested target: the message the harness says this answers. Untrusted.
        in_reply_to: String,
        /// The session the harness names, when it names one.
        to: Option<SessionId>,
        /// The content parts.
        content: Vec<ContentPart>,
    },
    /// `list_sessions`.
    ListSessions,
    /// `whoami`.
    Whoami,
}

/// Why a tool call could not be parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    /// A tool outside §5.1: an MCP protocol error, not an OAC one ([MCPB-TOOL-018]).
    UnknownTool(String),
    /// Arguments that fail the tool's `inputSchema`: `invalid-request` ([MCPB-TOOL-005]).
    Invalid(&'static str),
}

fn str_arg<'a>(args: Option<&'a JsonObject>, name: &str) -> Result<Option<&'a str>, CallError> {
    match args.and_then(|a| a.get(name)) {
        None => Ok(None),
        Some(v) => v.as_str().map(Some).ok_or(CallError::Invalid(
            "an argument that must be a string is not one",
        )),
    }
}

fn session_arg(args: Option<&JsonObject>, name: &str) -> Result<Option<SessionId>, CallError> {
    match str_arg(args, name)? {
        None => Ok(None),
        Some(s) => SessionId::parse(s)
            .map(Some)
            .ok_or(CallError::Invalid("a session argument is not a session id")),
    }
}

fn content_arg(args: Option<&JsonObject>) -> Result<Vec<ContentPart>, CallError> {
    let parts = args
        .and_then(|a| a.get("content"))
        .and_then(|v| v.as_array())
        .ok_or(CallError::Invalid("content must be an array of text parts"))?;
    if parts.is_empty() {
        return Err(CallError::Invalid("content must hold at least one part"));
    }
    parts
        .iter()
        .map(|p| {
            let p = p
                .as_object()
                .ok_or(CallError::Invalid("a content part must be an object"))?;
            if p.get("type").and_then(|t| t.as_str()) != Some("text") {
                return Err(CallError::Invalid("only text parts are carried"));
            }
            let text = p
                .get("text")
                .and_then(|t| t.as_str())
                .ok_or(CallError::Invalid("a text part must have a string text"))?;
            TextPart::new(text)
                .map(ContentPart::Text)
                .ok_or(CallError::Invalid(
                    "a text part must be non-empty I-JSON text",
                ))
        })
        .collect()
}

/// Parses a `tools/call` of tool `name` with `arguments`.
pub fn parse(name: &str, arguments: Option<&JsonObject>) -> Result<ToolCall, CallError> {
    match name {
        SEND => Ok(ToolCall::Send {
            to: session_arg(arguments, "to")?.ok_or(CallError::Invalid("to is required"))?,
            content: content_arg(arguments)?,
            conversation_id: str_arg(arguments, "conversation_id")?.map(str::to_owned),
            correlation_id: str_arg(arguments, "correlation_id")?.map(str::to_owned),
        }),
        REPLY => Ok(ToolCall::Reply {
            in_reply_to: str_arg(arguments, "in_reply_to")?
                .ok_or(CallError::Invalid("in_reply_to is required"))?
                .to_owned(),
            to: session_arg(arguments, "to")?,
            content: content_arg(arguments)?,
        }),
        LIST_SESSIONS => Ok(ToolCall::ListSessions),
        WHOAMI => Ok(ToolCall::Whoami),
        other => Err(CallError::UnknownTool(other.to_owned())),
    }
}

/// The protocol error for a tool outside §5.1 ([MCPB-TOOL-018]): JSON-RPC `-32602`, as MCP
/// gives "Unknown tools". It carries no OAC code.
pub fn unknown_tool() -> ErrorData {
    ErrorData::invalid_params("unknown tool", None)
}

/// The `SendRequest` for a `send` call, labelled with the attachment it arrived on
/// ([IFC-ADP-031]). No member names the requester ([IFC-TYP-090]).
pub fn send_request(
    attachment: Attachment,
    to: SessionId,
    content: Vec<ContentPart>,
    conversation_id: Option<String>,
    correlation_id: Option<String>,
) -> SendRequest {
    SendRequest {
        attachment,
        to,
        content,
        requested_target: None,
        conversation_id,
        correlation_id,
    }
}

/// The `SendRequest` for a `reply` call: `requested_target` is the harness's
/// `in_reply_to`, untrusted. The core decides whether the reply is correlated
/// (`spec/session-channels.md` §8.2.2).
pub fn reply_request(
    attachment: Attachment,
    to: SessionId,
    content: Vec<ContentPart>,
    in_reply_to: String,
) -> SendRequest {
    SendRequest {
        attachment,
        to,
        content,
        requested_target: Some(in_reply_to),
        conversation_id: None,
        correlation_id: None,
    }
}

// ---- results ------------------------------------------------------------------------------

fn result(value: Value, is_error: bool) -> CallToolResult {
    let text = value.to_string();
    let mut r = if is_error {
        CallToolResult::error(vec![ContentBlock::text(text)])
    } else {
        CallToolResult::success(vec![ContentBlock::text(text)])
    };
    r.structured_content = Some(value);
    r
}

/// The tool execution error for an OAC code ([MCPB-TOOL-010], [MCPB-TOOL-011],
/// [MCPB-TOOL-012]). Its text and structured content depend on the code alone, never on
/// the call's arguments, so two refusals with one code are the same value
/// ([MCPB-TOOL-017], [MCPB-TOOL-020]).
pub fn refusal(code: ErrorCode) -> CallToolResult {
    let mut r = CallToolResult::error(vec![ContentBlock::text(format!(
        "{}: the request was refused.",
        code.as_str()
    ))]);
    r.structured_content = Some(Value::from(object(vec![(
        "error",
        Value::from(code.as_str()),
    )])));
    r
}

/// The result of a `send` or `reply`, from the core's result unchanged ([IFC-ADP-060]).
/// `sent` reports the message id and `accepted-by-adapter`, never a later state
/// ([MCPB-TOOL-006], [MCPB-TOOL-008]); a reply also says whether it went correlated
/// ([SC-RCP-055]). `refused` and `not-passed` are tool execution errors carrying the code.
pub fn send_result(r: &SendRequestResult) -> CallToolResult {
    match r {
        SendRequestResult::Refused { error } => refusal(*error),
        SendRequestResult::NotPassed { id, error } => {
            let mut out = CallToolResult::error(vec![ContentBlock::text(format!(
                "{}: message {} was created but not passed to a transport.",
                error.as_str(),
                id.as_str()
            ))]);
            out.structured_content = Some(Value::from(object(vec![
                ("error", Value::from(error.as_str())),
                ("id", Value::from(id.as_str())),
                ("state", Value::from(DeliveryState::Failed.as_str())),
            ])));
            out
        }
        SendRequestResult::Sent {
            id, correlation, ..
        } => {
            let mut members = vec![
                ("id", Value::from(id.as_str())),
                (
                    "state",
                    Value::from(DeliveryState::AcceptedByAdapter.as_str()),
                ),
            ];
            if let Some(c) = correlation {
                members.push((
                    "correlation",
                    Value::from(match c {
                        Correlation::Correlated => "correlated",
                        Correlation::Uncorrelated => "uncorrelated",
                    }),
                ));
            }
            result(Value::from(object(members)), false)
        }
    }
}

/// The result of a `list_sessions` call: `{"sessions": [...]}` in text and in structured
/// content ([MCPB-TOOL-014], [MCPB-TOOL-015]), or the refusal.
pub fn discovery_result(r: &DiscoveryRequestResult) -> CallToolResult {
    match r {
        DiscoveryRequestResult::Refused { error } => refusal(*error),
        DiscoveryRequestResult::DiscoveryResult(sessions) => {
            let list: Vec<Value> = sessions
                .iter()
                .map(|d| Value::from(object_of(d.as_json())))
                .collect();
            result(
                Value::from(object(vec![("sessions", Value::from(list))])),
                false,
            )
        }
    }
}

fn object_of(o: &oac_core::json::JsonObject) -> JsonObject {
    let mut out = JsonObject::new();
    for (k, v) in o.iter() {
        out.insert(k.to_owned(), value_of(v));
    }
    out
}

fn value_of(j: &Json) -> Value {
    match j {
        Json::Null => Value::from(()),
        Json::Bool(b) => Value::from(*b),
        Json::Number(n) => {
            let raw = n.raw();
            if let Ok(u) = raw.parse::<u64>() {
                Value::from(u)
            } else if let Ok(i) = raw.parse::<i64>() {
                Value::from(i)
            } else {
                Value::from(n.to_f64())
            }
        }
        Json::String(s) => Value::from(s.as_str()),
        Json::Array(a) => Value::from(a.iter().map(value_of).collect::<Vec<_>>()),
        Json::Object(o) => Value::from(object_of(o)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> JsonObject {
        let mut o = JsonObject::new();
        o.insert("to".into(), Value::from("0123456789abcdefghjkmnpqrs"));
        o.insert(
            "content".into(),
            Value::from(vec![Value::from(object(vec![
                ("type", Value::from("text")),
                ("text", Value::from(text)),
            ]))]),
        );
        o
    }

    #[test]
    fn four_tools_under_their_names_with_object_schemas() {
        let t = tools();
        let names: Vec<&str> = t.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(names, TOOL_NAMES);
        for tool in &t {
            assert_eq!(
                tool.input_schema.get("type").and_then(|v| v.as_str()),
                Some("object")
            );
        }
        // One definition: a second call gives the same value (MCPB-TOOL-003).
        assert_eq!(t, tools());
    }

    #[test]
    fn send_parses_and_ignores_unknown_members() {
        let mut a = args("hello");
        a.insert("from".into(), Value::from("7777777777777777777777777z"));
        match parse(SEND, Some(&a)).unwrap() {
            ToolCall::Send { to, content, .. } => {
                assert_eq!(to.as_str(), "0123456789abcdefghjkmnpqrs");
                assert_eq!(content.len(), 1);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn schema_failures_are_invalid_requests_and_unknown_tools_are_not() {
        let mut a = args("x");
        a.remove("to");
        assert!(matches!(parse(SEND, Some(&a)), Err(CallError::Invalid(_))));
        assert!(matches!(parse(SEND, None), Err(CallError::Invalid(_))));
        let mut a = args("x");
        a.insert("to".into(), Value::from("not a session id"));
        assert!(matches!(parse(SEND, Some(&a)), Err(CallError::Invalid(_))));
        let mut a = args("x");
        a.insert("content".into(), Value::from(Vec::<Value>::new()));
        assert!(matches!(parse(SEND, Some(&a)), Err(CallError::Invalid(_))));
        assert!(matches!(
            parse(REPLY, Some(&args("x"))),
            Err(CallError::Invalid(_))
        ));
        assert_eq!(
            parse("read_inbox", None),
            Err(CallError::UnknownTool("read_inbox".into()))
        );
    }

    #[test]
    fn refusal_text_carries_the_code_and_nothing_from_the_call() {
        for code in ErrorCode::ALL {
            let r = refusal(code);
            assert_eq!(r.is_error, Some(true));
            let text = match &r.content[0] {
                ContentBlock::Text(t) => t.text.clone(),
                other => panic!("{other:?}"),
            };
            assert!(text.starts_with(code.as_str()), "{text}");
            assert_eq!(r, refusal(code));
        }
    }
}
