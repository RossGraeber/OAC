// SPDX-License-Identifier: Apache-2.0

//! Refused-call records and reveal detection (`spec/bindings/mcp.md` §4.5.2, §4.5.3; G8).
//!
//! When the core refuses a call on a Codex connection with a pairing value, the adapter keeps
//! a **record** of the refused call: its tool, its arguments, and the text of the refusal it
//! returned, which carries the value ([MCPB-ATT-004]). A **reveal** is Codex's own report of
//! that call: an `item/completed` of an `mcpToolCall`, from the registered server, whose
//! `tool` and `arguments` match a record's and whose `result.content` is the same JSON value
//! as the refusal's `content` ([`Refusals::find`]). Because the refusal's text carries the
//! connection's own value ([MCPB-ATT-025]) and the value is written to that connection only
//! ([MCPB-ATT-006]), a matching report identifies the connection; the notification's
//! `threadId`, never anything in the call, names the thread ([MCPB-ATT-008]).
//!
//! The records decide nothing about pairing: the window, the single use, the conflict and the
//! cap of three windows are the core's ([`oac_core::issued_pairing`]). A record lives as long
//! as the pairing window ([`crate::pairing::Config::window`]); a report that matches only a
//! record older than that is a **late** reveal, which is counted and dropped here (the core
//! drops one it receives after the window too, [MCPB-ATT-009]).

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::time::{Duration, Instant};

use oac_core::json::Json;

use crate::confirm::arguments_match;

/// The most records kept per connection. Three windows are allowed ([MCPB-ATT-023]) and one
/// value serves a whole window, but any number of calls can be refused in a window, each a
/// record; the oldest goes past the cap (a reveal of it is then unmatched: availability only).
pub const MAX_RECORDS: usize = 16;

/// The most connections that hold records at once.
pub const MAX_CONNECTIONS: usize = 64;

struct Record {
    tool: String,
    arguments: Json,
    text: String,
    value: String,
    at: Instant,
}

/// What [`Refusals::find`] found for a report.
#[derive(Debug, PartialEq, Eq)]
pub enum Found<K> {
    /// A record matches: the connection it was refused on, and the value revealed.
    Reveal {
        /// The connection.
        key: K,
        /// The pairing value the report carried back.
        value: String,
    },
    /// A record matches, but it is older than the pairing window.
    Late,
    /// No record matches.
    None,
}

/// The records of every connection.
pub struct Refusals<K> {
    by_key: HashMap<K, VecDeque<Record>>,
    life: Duration,
}

impl<K: Clone + Eq + Hash> Refusals<K> {
    /// Records that live `life` (the pairing window).
    pub fn new(life: Duration) -> Refusals<K> {
        Refusals {
            by_key: HashMap::new(),
            life,
        }
    }

    /// Record a refused call of connection `key`: the `tool` and `arguments` of the call
    /// (`Json::Null` for none), the `text` of the refusal returned, and the pairing `value`
    /// it carries.
    pub fn record(
        &mut self,
        key: &K,
        tool: &str,
        arguments: Json,
        text: &str,
        value: &str,
        now: Instant,
    ) {
        if !self.by_key.contains_key(key) && self.by_key.len() >= MAX_CONNECTIONS {
            // Drop the connection whose newest record is oldest.
            if let Some(old) = self
                .by_key
                .iter()
                .min_by_key(|(_, q)| q.back().map(|r| r.at))
                .map(|(k, _)| k.clone())
            {
                self.by_key.remove(&old);
            }
        }
        let q = self.by_key.entry(key.clone()).or_default();
        q.retain(|r| now.saturating_duration_since(r.at) < self.life);
        q.push_back(Record {
            tool: tool.to_owned(),
            arguments,
            text: text.to_owned(),
            value: value.to_owned(),
            at: now,
        });
        while q.len() > MAX_RECORDS {
            q.pop_front();
        }
    }

    /// Look for the record a report matches, at `now`: `tool`, the item's `arguments`
    /// (`None` when absent), and the `result.content` of the report, as the text of its one
    /// text block ([`single_text_content`]). A live match is consumed: one refused call is
    /// revealed once.
    pub fn find(
        &mut self,
        tool: &str,
        arguments: Option<&Json>,
        content_text: &str,
        now: Instant,
    ) -> Found<K> {
        let mut late = false;
        let mut hit: Option<(K, usize)> = None;
        'outer: for (k, q) in &self.by_key {
            for (i, r) in q.iter().enumerate() {
                if r.tool == tool
                    && r.text == content_text
                    && arguments_match(&r.arguments, arguments)
                {
                    if now.saturating_duration_since(r.at) < self.life {
                        hit = Some((k.clone(), i));
                        break 'outer;
                    }
                    late = true;
                }
            }
        }
        match hit {
            Some((k, i)) => {
                let r = self
                    .by_key
                    .get_mut(&k)
                    .and_then(|q| q.remove(i))
                    .expect("the record just found");
                Found::Reveal {
                    key: k,
                    value: r.value,
                }
            }
            None if late => Found::Late,
            None => Found::None,
        }
    }

    /// Forget every record of connection `key`: it ended.
    pub fn forget(&mut self, key: &K) {
        self.by_key.remove(key);
    }

    /// How many records are kept for `key`.
    pub fn len(&self, key: &K) -> usize {
        self.by_key.get(key).map_or(0, VecDeque::len)
    }

    /// True when no record is kept for `key`.
    pub fn is_empty(&self, key: &K) -> bool {
        self.len(key) == 0
    }
}

/// The text of a reported `result` whose `content` is exactly one block that is exactly
/// `{"type":"text","text":...}`: the form every refusal of this adapter has, which is what a
/// reveal is compared with (the same JSON value, [MCPB-ATT-007]). Anything else, including a
/// block with further members, is `None`: it is not a refusal this adapter returned, and
/// costs availability only.
pub fn single_text_content(result: &Json) -> Option<&str> {
    let content = result.as_object()?.get("content")?.as_array()?;
    let [block] = content else {
        return None;
    };
    let block = block.as_object()?;
    if block.len() != 2 || block.get("type").and_then(Json::as_str) != Some("text") {
        return None;
    }
    block.get("text")?.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use oac_core::json::parse;

    const L: Duration = Duration::from_secs(30);
    const V1: &str = "oac-pair-11111111111111111111111111111111";
    const V2: &str = "oac-pair-22222222222222222222222222222222";

    fn j(s: &str) -> Json {
        parse(s.as_bytes()).unwrap()
    }

    fn text(v: &str) -> String {
        format!("unauthorized: pairing value {v}.")
    }

    #[test]
    fn a_report_that_matches_a_refused_call_reveals_its_value_once() {
        let mut r = Refusals::<u8>::new(L);
        let t = Instant::now();
        r.record(&1, "send", j(r#"{"to":"x"}"#), &text(V1), V1, t);
        r.record(&2, "send", j(r#"{"to":"x"}"#), &text(V2), V2, t);
        // The text carries each connection's own value, so identical calls tell apart.
        assert_eq!(
            r.find("send", Some(&j(r#"{"to":"x"}"#)), &text(V2), t),
            Found::Reveal {
                key: 2,
                value: V2.into()
            }
        );
        // Once: the same report again finds nothing.
        assert_eq!(
            r.find("send", Some(&j(r#"{"to":"x"}"#)), &text(V2), t),
            Found::None
        );
        assert_eq!(r.len(&1), 1);
        assert_eq!(r.len(&2), 0);
    }

    #[test]
    fn a_different_tool_arguments_or_content_reveals_nothing() {
        let mut r = Refusals::<u8>::new(L);
        let t = Instant::now();
        r.record(&1, "send", j(r#"{"to":"x"}"#), &text(V1), V1, t);
        let args = j(r#"{"to":"x"}"#);
        assert_eq!(r.find("reply", Some(&args), &text(V1), t), Found::None);
        assert_eq!(
            r.find("send", Some(&j(r#"{"to":"y"}"#)), &text(V1), t),
            Found::None
        );
        assert_eq!(r.find("send", None, &text(V1), t), Found::None);
        assert_eq!(r.find("send", Some(&args), &text(V2), t), Found::None);
        assert_eq!(r.find("send", Some(&args), "", t), Found::None);
        assert_eq!(r.len(&1), 1, "nothing was consumed");
    }

    #[test]
    fn null_absent_and_empty_object_arguments_follow_the_spec() {
        let mut r = Refusals::<u8>::new(L);
        let t = Instant::now();
        // No arguments member on the call; the item says null or has no member.
        r.record(&1, "whoami", Json::Null, &text(V1), V1, t);
        assert!(matches!(
            r.find("whoami", Some(&Json::Null), &text(V1), t),
            Found::Reveal { .. }
        ));
        r.record(&1, "whoami", Json::Null, &text(V1), V1, t);
        assert!(matches!(
            r.find("whoami", None, &text(V1), t),
            Found::Reveal { .. }
        ));
        // The S1 capture: `{}` sent, `{}` reported.
        r.record(&1, "whoami", j("{}"), &text(V1), V1, t);
        assert!(matches!(
            r.find("whoami", Some(&j("{}")), &text(V1), t),
            Found::Reveal { .. }
        ));
        // `{}` matches only `{}`.
        r.record(&1, "whoami", j("{}"), &text(V1), V1, t);
        assert_eq!(
            r.find("whoami", Some(&Json::Null), &text(V1), t),
            Found::None
        );
        assert_eq!(r.find("whoami", None, &text(V1), t), Found::None);
    }

    #[test]
    fn a_report_after_the_window_is_late_and_reveals_nothing() {
        let mut r = Refusals::<u8>::new(L);
        let t = Instant::now();
        r.record(&1, "x", Json::Null, &text(V1), V1, t);
        assert_eq!(
            r.find("x", None, &text(V1), t + L + Duration::from_secs(1)),
            Found::Late
        );
        // Just inside the window it is a reveal.
        assert!(matches!(
            r.find("x", None, &text(V1), t + L - Duration::from_millis(1)),
            Found::Reveal { .. }
        ));
    }

    #[test]
    fn records_are_bounded_per_connection_and_overall() {
        let mut r = Refusals::<u32>::new(L);
        let t = Instant::now();
        for i in 0..MAX_RECORDS + 5 {
            r.record(&1, "x", Json::Null, &format!("t{i}"), V1, t);
        }
        assert_eq!(r.len(&1), MAX_RECORDS);
        assert_eq!(r.find("x", None, "t0", t), Found::None, "the oldest went");
        for k in 0..MAX_CONNECTIONS as u32 + 3 {
            r.record(
                &(100 + k),
                "x",
                Json::Null,
                "t",
                V1,
                t + Duration::from_millis(u64::from(k)),
            );
        }
        let kept = (0..MAX_CONNECTIONS as u32 + 3)
            .filter(|k| !r.is_empty(&(100 + k)))
            .count();
        assert!(kept <= MAX_CONNECTIONS, "{kept}");
        r.forget(&150);
        assert!(r.is_empty(&150));
    }

    #[test]
    fn only_a_lone_plain_text_block_is_a_comparable_content() {
        let ok =
            j(r#"{"content":[{"type":"text","text":"a"}],"structuredContent":null,"_meta":null}"#);
        assert_eq!(single_text_content(&ok), Some("a"));
        for bad in [
            r#"{"content":[]}"#,
            r#"{"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}"#,
            r#"{"content":[{"type":"text","text":"a","annotations":null}]}"#,
            r#"{"content":[{"type":"image","text":"a"}]}"#,
            r#"{"content":[{"type":"text"}]}"#,
            r#"{"content":{"type":"text","text":"a"}}"#,
            r#"{}"#,
            r#"[]"#,
        ] {
            assert_eq!(single_text_content(&j(bad)), None, "{bad}");
        }
    }
}
