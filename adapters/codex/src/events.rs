// SPDX-License-Identifier: Apache-2.0

//! App-server notifications, translated into events about served threads (G6 acceptance
//! item 2).
//!
//! - **`item/completed` is authoritative; deltas are not.** An item's content is taken
//!   only from its `item/completed`. `item/agentMessage/delta` and every other `*/delta`
//!   notification are counted and dropped. `item/started` is passed on with the item's id
//!   and kind only, never its content: it says an item began (the confirmation of
//!   `spec/bindings/mcp.md` §4.5.2 is one, G8), not what it holds.
//! - **Only served threads.** A notification is used only when its `threadId` is a thread
//!   the client subscribed to. Live Codex sends some thread notifications to connections
//!   that did not subscribe to the thread, and after `thread/unsubscribe`
//!   (`11-risks.md` row 70); [MCPB-ATT-018] makes the subscription the rule. Anything else
//!   is dropped and counted, never surfaced.
//! - **No identity from the harness's own labels.** `thread/started` and every thread
//!   listing carry `preview`, `source` and originator fields. None is read: an originator
//!   does not identify the client that made a thread (rows 37, 38), and a preview can hold
//!   another harness's prompt text (row 43). The client never calls `thread/list`.

use oac_core::json::Json;
use std::fmt;

use crate::schema::{self, Shape};
use crate::threads::{NativeRef, Registry, ThreadKey, ThreadState, TurnEnd};

/// What a completed item was, as far as the adapter reads it.
#[derive(Clone, PartialEq, Eq)]
pub enum CompletedItem {
    /// A `userMessage`: the text of its text parts, joined with newlines, and its
    /// `clientId` (the `clientUserMessageId` of a queued add, when it came from one).
    UserMessage {
        /// The item's native id.
        id: NativeRef,
        /// The text.
        text: String,
        /// The `clientId`.
        client_id: Option<String>,
    },
    /// An `agentMessage`: its final text.
    AgentMessage {
        /// The item's native id.
        id: NativeRef,
        /// The text.
        text: String,
    },
    /// An `mcpToolCall`: what the pairing of `spec/bindings/mcp.md` §4.5 reads (G8). The
    /// members are passed on as Codex reported them, for G8 to compare; nothing here
    /// trusts them.
    McpToolCall {
        /// The item's native id.
        id: NativeRef,
        /// The registered server name.
        server: String,
        /// The tool name.
        tool: String,
        /// `arguments` as reported (`null` included).
        arguments: Option<Json>,
        /// `result` as reported.
        result: Option<Json>,
    },
    /// Any other item type: only its type.
    Other {
        /// The item's native id.
        id: NativeRef,
        /// The `type`.
        kind: String,
    },
}

/// Debug reports only the variant: text and tool payloads may contain pairing values.
impl fmt::Debug for CompletedItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::UserMessage { .. } => "UserMessage",
            Self::AgentMessage { .. } => "AgentMessage",
            Self::McpToolCall { .. } => "McpToolCall",
            Self::Other { .. } => "Other",
        };
        f.debug_struct(name).finish_non_exhaustive()
    }
}

/// An event about a served thread.
#[derive(Clone, PartialEq, Eq)]
pub enum CodexEvent {
    /// `thread/status/changed`.
    Status {
        /// The thread.
        thread: ThreadKey,
        /// The new state.
        state: ThreadState,
    },
    /// `turn/started`.
    TurnStarted {
        /// The thread.
        thread: ThreadKey,
        /// The turn.
        turn: NativeRef,
    },
    /// `turn/completed`.
    TurnCompleted {
        /// The thread.
        thread: ThreadKey,
        /// The turn.
        turn: NativeRef,
        /// How it ended.
        end: TurnEnd,
    },
    /// `item/started`: the item began. Its content is not read here.
    ItemStarted {
        /// The thread.
        thread: ThreadKey,
        /// The turn.
        turn: NativeRef,
        /// The item.
        item: NativeRef,
        /// Its `type`.
        kind: String,
    },
    /// `item/completed`: the authoritative item.
    ItemCompleted {
        /// The thread.
        thread: ThreadKey,
        /// The turn.
        turn: NativeRef,
        /// The item.
        item: CompletedItem,
    },
    /// The carrier closed. Nothing more will arrive.
    Closed,
}

/// Keep every harness-supplied string out of diagnostic formatting.
impl fmt::Debug for CodexEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Status { .. } => "Status",
            Self::TurnStarted { .. } => "TurnStarted",
            Self::TurnCompleted { .. } => "TurnCompleted",
            Self::ItemStarted { .. } => "ItemStarted",
            Self::ItemCompleted { .. } => "ItemCompleted",
            Self::Closed => "Closed",
        };
        f.debug_struct(name).finish_non_exhaustive()
    }
}

/// Why a notification was not turned into an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dropped {
    /// A delta: never authoritative.
    Delta,
    /// For a thread the client does not serve (or with no thread at all).
    NotServed,
    /// A shape the client relies on was missing ([`schema::Checked::broken`]).
    Malformed,
    /// A method the client does not use.
    Unused,
}

/// The outcome of translating one notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Translated {
    /// The event, or why there is none.
    pub event: Result<CodexEvent, Dropped>,
    /// Required members the schema has and the message lacked (drift, counted only).
    pub drift: Vec<&'static str>,
}

fn served(reg: &Registry, params: &Json) -> Option<ThreadKey> {
    params
        .as_object()?
        .get("threadId")?
        .as_str()
        .and_then(|t| reg.key_of(t))
}

fn member<'a>(v: &'a Json, name: &str) -> Option<&'a Json> {
    v.as_object()?.get(name)
}

fn member_str<'a>(v: &'a Json, name: &str) -> Option<&'a str> {
    member(v, name)?.as_str()
}

/// Translate one notification. `reg` is updated with what it says of a served thread's
/// turns and status.
pub fn translate(method: &str, params: Option<&Json>, reg: &mut Registry) -> Translated {
    let none = Json::Null;
    let params = params.unwrap_or(&none);
    let mut drift = Vec::new();
    let mut checked = |shape: &Shape, v: &Json| -> bool {
        let c = shape.check(v);
        drift.extend(c.drift.iter().copied());
        c.usable()
    };
    let event = (|| {
        if method.ends_with("/delta") {
            return Err(Dropped::Delta);
        }
        let shape = match method {
            "thread/status/changed" => &schema::THREAD_STATUS_CHANGED,
            "turn/started" => &schema::TURN_STARTED,
            "turn/completed" => &schema::TURN_COMPLETED,
            "item/started" => &schema::ITEM_STARTED,
            "item/completed" => &schema::ITEM_COMPLETED,
            _ => return Err(Dropped::Unused),
        };
        // The thread first: a notification for a thread not served is dropped before its
        // shape is read, so another thread's traffic is never even checked.
        let thread = served(reg, params).ok_or(Dropped::NotServed)?;
        if !checked(shape, params) {
            return Err(Dropped::Malformed);
        }
        match method {
            "thread/status/changed" => {
                let status = member(params, "status").ok_or(Dropped::Malformed)?;
                let state = member_str(status, "type")
                    .and_then(ThreadState::from_type)
                    .ok_or(Dropped::Malformed)?;
                reg.set_state(thread, state);
                Ok(CodexEvent::Status { thread, state })
            }
            "turn/started" | "turn/completed" => {
                let t = member(params, "turn").ok_or(Dropped::Malformed)?;
                if !checked(&schema::TURN, t) {
                    return Err(Dropped::Malformed);
                }
                let turn = NativeRef::new(member_str(t, "id").ok_or(Dropped::Malformed)?);
                if method == "turn/started" {
                    reg.turn_started(thread, &turn);
                    Ok(CodexEvent::TurnStarted { thread, turn })
                } else {
                    let end = member_str(t, "status")
                        .and_then(TurnEnd::from_status)
                        .ok_or(Dropped::Malformed)?;
                    reg.turn_ended(thread, &turn, end);
                    Ok(CodexEvent::TurnCompleted { thread, turn, end })
                }
            }
            "item/started" => {
                let it = member(params, "item").ok_or(Dropped::Malformed)?;
                if !checked(&schema::ANY_ITEM, it) {
                    return Err(Dropped::Malformed);
                }
                Ok(CodexEvent::ItemStarted {
                    thread,
                    turn: NativeRef::new(member_str(params, "turnId").unwrap_or_default()),
                    item: NativeRef::new(member_str(it, "id").unwrap_or_default()),
                    kind: member_str(it, "type").unwrap_or_default().to_owned(),
                })
            }
            _ => {
                let it = member(params, "item").ok_or(Dropped::Malformed)?;
                if !checked(&schema::ANY_ITEM, it) {
                    return Err(Dropped::Malformed);
                }
                let item = completed_item(it, &mut checked).ok_or(Dropped::Malformed)?;
                Ok(CodexEvent::ItemCompleted {
                    thread,
                    turn: NativeRef::new(member_str(params, "turnId").unwrap_or_default()),
                    item,
                })
            }
        }
    })();
    Translated { event, drift }
}

fn completed_item(
    it: &Json,
    checked: &mut impl FnMut(&Shape, &Json) -> bool,
) -> Option<CompletedItem> {
    let id = NativeRef::new(member_str(it, "id")?);
    let kind = member_str(it, "type")?;
    Some(match kind {
        "userMessage" => {
            if !checked(&schema::USER_MESSAGE_ITEM, it) {
                return None;
            }
            let text = member(it, "content")?
                .as_array()?
                .iter()
                .filter(|p| member_str(p, "type") == Some("text"))
                .filter_map(|p| member_str(p, "text"))
                .collect::<Vec<_>>()
                .join("\n");
            CompletedItem::UserMessage {
                id,
                text,
                client_id: member_str(it, "clientId").map(str::to_owned),
            }
        }
        "agentMessage" => {
            if !checked(&schema::AGENT_MESSAGE_ITEM, it) {
                return None;
            }
            CompletedItem::AgentMessage {
                id,
                text: member_str(it, "text")?.to_owned(),
            }
        }
        "mcpToolCall" => {
            if !checked(&schema::MCP_TOOL_CALL_ITEM, it) {
                return None;
            }
            CompletedItem::McpToolCall {
                id,
                server: member_str(it, "server")?.to_owned(),
                tool: member_str(it, "tool")?.to_owned(),
                arguments: member(it, "arguments").cloned(),
                result: member(it, "result").cloned(),
            }
        }
        other => CompletedItem::Other {
            id,
            kind: other.to_owned(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_content_and_pairing_values() {
        let secret = "oac-pair-private-content";
        let id = NativeRef::new(secret);
        let mut registry = Registry::default();
        let (thread, _) = registry.serve(secret);
        for item in [
            CompletedItem::UserMessage {
                id: id.clone(),
                text: secret.into(),
                client_id: Some(secret.into()),
            },
            CompletedItem::AgentMessage {
                id: id.clone(),
                text: secret.into(),
            },
            CompletedItem::McpToolCall {
                id: id.clone(),
                server: secret.into(),
                tool: secret.into(),
                arguments: Some(Json::String(secret.into())),
                result: Some(Json::String(secret.into())),
            },
            CompletedItem::Other {
                id: id.clone(),
                kind: secret.into(),
            },
        ] {
            assert!(!format!("{item:?}").contains(secret));
            let event = CodexEvent::ItemCompleted {
                thread,
                turn: id.clone(),
                item,
            };
            assert!(!format!("{event:?}").contains(secret));
        }
        let started = CodexEvent::ItemStarted {
            thread,
            turn: id.clone(),
            item: id,
            kind: secret.into(),
        };
        assert!(!format!("{started:?}").contains(secret));
    }
    use oac_core::json::parse;

    const T: &str = "01a0e550-1921-7000-93ef-383c5acff39f";
    const OTHER: &str = "01a0e550-0000-7000-93ef-383c5acff39f";

    fn p(s: &str) -> Json {
        parse(s.as_bytes()).unwrap()
    }

    fn reg() -> (Registry, ThreadKey) {
        let mut r = Registry::default();
        let (k, _) = r.serve(T);
        (r, k)
    }

    #[test]
    fn item_completed_is_the_authoritative_text_and_deltas_are_dropped() {
        let (mut r, k) = reg();
        // The recorded D6 shapes (transcript-conn1-2026-09-28.jsonl).
        let delta = p(&format!(
            r#"{{"threadId":"{T}","turnId":"u","itemId":"i","delta":"partial"}}"#
        ));
        let t = translate("item/agentMessage/delta", Some(&delta), &mut r);
        assert_eq!(t.event, Err(Dropped::Delta));
        let done = p(&format!(
            r#"{{"item":{{"type":"agentMessage","id":"msg_1","text":"OAC-D6-T5-ACK","phase":"final_answer"}},"threadId":"{T}","turnId":"u","completedAtMs":1}}"#
        ));
        let t = translate("item/completed", Some(&done), &mut r);
        assert_eq!(
            t.event,
            Ok(CodexEvent::ItemCompleted {
                thread: k,
                turn: NativeRef::new("u"),
                item: CompletedItem::AgentMessage {
                    id: NativeRef::new("msg_1"),
                    text: "OAC-D6-T5-ACK".into()
                }
            })
        );
        assert!(t.drift.is_empty());
    }

    #[test]
    fn notifications_for_threads_not_served_are_dropped() {
        let (mut r, _) = reg();
        for (m, body) in [
            (
                "thread/status/changed",
                r#"{"status":{"type":"idle"},"threadId":"X"}"#,
            ),
            (
                "turn/started",
                r#"{"threadId":"X","turn":{"id":"u","items":[],"status":"inProgress"}}"#,
            ),
            (
                "item/completed",
                r#"{"item":{"type":"agentMessage","id":"m","text":"t"},"threadId":"X","turnId":"u","completedAtMs":1}"#,
            ),
        ] {
            let v = p(&body.replace('X', OTHER));
            assert_eq!(
                translate(m, Some(&v), &mut r).event,
                Err(Dropped::NotServed),
                "{m}"
            );
        }
        // Thread-less notifications are not used either.
        let v = p(r#"{"status":"disabled"}"#);
        assert_eq!(
            translate("remoteControl/status/changed", Some(&v), &mut r).event,
            Err(Dropped::Unused)
        );
        // thread/started carries preview and originator: never used.
        let v = p(r#"{"thread":{"id":"x","preview":"secret prompt","originator":"codex-tui"}}"#);
        assert_eq!(
            translate("thread/started", Some(&v), &mut r).event,
            Err(Dropped::Unused)
        );
    }

    #[test]
    fn turns_and_status_update_the_registry() {
        let (mut r, k) = reg();
        let v = p(&format!(
            r#"{{"status":{{"type":"active","activeFlags":[]}},"threadId":"{T}"}}"#
        ));
        assert_eq!(
            translate("thread/status/changed", Some(&v), &mut r).event,
            Ok(CodexEvent::Status {
                thread: k,
                state: ThreadState::Active
            })
        );
        let v = p(&format!(
            r#"{{"threadId":"{T}","turn":{{"id":"u","items":[],"status":"inProgress"}}}}"#
        ));
        translate("turn/started", Some(&v), &mut r).event.unwrap();
        assert!(r.view(k).unwrap().turn_running);
        let v = p(&format!(
            r#"{{"threadId":"{T}","turn":{{"id":"u","items":[],"status":"interrupted"}}}}"#
        ));
        assert_eq!(
            translate("turn/completed", Some(&v), &mut r).event,
            Ok(CodexEvent::TurnCompleted {
                thread: k,
                turn: NativeRef::new("u"),
                end: TurnEnd::Interrupted
            })
        );
        assert!(!r.view(k).unwrap().turn_running);
    }

    #[test]
    fn user_messages_carry_their_client_id_and_malformed_items_are_dropped() {
        let (mut r, k) = reg();
        let v = p(&format!(
            r#"{{"item":{{"type":"userMessage","id":"i","clientId":"oac-7","content":[{{"type":"text","text":"a","text_elements":[]}},{{"type":"image","url":"x"}},{{"type":"text","text":"b"}}]}},"threadId":"{T}","turnId":"u","completedAtMs":1}}"#
        ));
        assert_eq!(
            translate("item/completed", Some(&v), &mut r).event,
            Ok(CodexEvent::ItemCompleted {
                thread: k,
                turn: NativeRef::new("u"),
                item: CompletedItem::UserMessage {
                    id: NativeRef::new("i"),
                    text: "a\nb".into(),
                    client_id: Some("oac-7".into())
                }
            })
        );
        let v = p(&format!(
            r#"{{"item":{{"type":"agentMessage","id":"i"}},"threadId":"{T}","turnId":"u","completedAtMs":1}}"#
        ));
        assert_eq!(
            translate("item/completed", Some(&v), &mut r).event,
            Err(Dropped::Malformed)
        );
        let v = p(&format!(
            r#"{{"item":{{"type":"agentMessage","id":"i","text":"t"}},"threadId":"{T}","turnId":"u"}}"#
        ));
        let t = translate("item/completed", Some(&v), &mut r);
        assert!(t.event.is_ok());
        assert_eq!(t.drift, ["completedAtMs"]);
    }

    #[test]
    fn item_started_passes_no_content() {
        let (mut r, k) = reg();
        let v = p(&format!(
            r#"{{"item":{{"type":"mcpToolCall","id":"c","server":"oac","tool":"send","arguments":{{"to":"x"}},"status":"inProgress"}},"threadId":"{T}","turnId":"u","startedAtMs":1}}"#
        ));
        assert_eq!(
            translate("item/started", Some(&v), &mut r).event,
            Ok(CodexEvent::ItemStarted {
                thread: k,
                turn: NativeRef::new("u"),
                item: NativeRef::new("c"),
                kind: "mcpToolCall".into()
            })
        );
    }
}
