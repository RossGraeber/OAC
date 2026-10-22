// SPDX-License-Identifier: Apache-2.0

//! Matching a call with what Codex reports of it, and the confirmations of
//! `spec/bindings/mcp.md` §4.5 (G8, #69, #403).
//!
//! A **confirmation** is Codex's own report, on the carrier, that thread T is making a call
//! now: an `item/started` of an `mcpToolCall` for T, from the registered server, with the
//! call's tool and arguments ([`McpCallStart`](crate::events::McpCallStart)). The engine here
//! keeps the live ones, keyed by thread and item id, and lets a call claim one:
//!
//! - **Live** ([MCPB-ATT-021]): until the first of the item's `item/completed`
//!   ([`Confirmations::completed`]), the thread's `turn/completed` for the confirmation's
//!   turn ([`Confirmations::turn_completed`]), and the life limit (600 s by default) after it
//!   arrived. An orphaned confirmation, whose turn ended with the call in flight and no
//!   `item/completed`, therefore ends with the turn, or with the limit.
//! - **Single use** ([MCPB-ATT-014]): a claimed confirmation is not claimable again. It stays
//!   (claimed) until it ends as above, so the same `item/started` cannot confirm a second call.
//! - **Either order, bounded wait** ([MCPB-ATT-013], [MCPB-ATT-020]): a call that reaches the
//!   server before its `item/started` waits for it, up to a bound that defaults to 5 s
//!   ([`Confirmations::claim_wait`]). The S1 capture saw `item/started` come 0.9 to 4.1 s
//!   before the server saw the call, because the approval step sits between them; the
//!   reverse order was not seen, and is covered by the wait.
//!
//! **Matching** ([`arguments_match`]): a call's `arguments` and an item's match when they are
//! equal JSON values (object members in any order), and also when the call has no
//! `arguments` member and the item's is `null`; here `null` and absent are one, on either
//! side. An empty object `{}` matches only `{}` (spec §4.5.2). The S1 capture showed Codex
//! sending `{}` and reporting `{}` for a call the model made with no arguments, so that case
//! is an ordinary equal match; `{}` against `null` is not.
//!
//! **`_meta` is not read.** The `tools/call` `_meta` carries `threadId`, `sessionId`,
//! `windowId`, `callId`, `x-codex-turn-metadata` and, at Codex 0.163.0, `itemId`,
//! `traceparent` and `progressToken` (S1 capture, C1). None strengthens the match:
//! `itemId` is a `ctc_...` id that no notification carries (the item's own id is
//! `exec-<uuid>`, and equals `callId`), so there is nothing to compare it with; and `callId`,
//! which does equal the item id in all three recorded calls, is a value the caller wrote
//! (`spec/bindings/mcp.md` threat table, "self-asserted identity"). Neither narrows or widens
//! a match here. If a later Codex reports the `_meta` id in `item/started`, it could be used
//! as an additional narrowing check, never as attribution ([MCPB-ATT-008]).

use std::collections::HashMap;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use oac_core::json::Json;

use crate::threads::{NativeRef, ThreadKey};

/// The default wait for a confirmation ([MCPB-ATT-020] allows up to 10 s; the reference
/// implementation note says 5 s).
pub const CONFIRM_WAIT: Duration = Duration::from_secs(5);

/// The default life of a confirmation ([MCPB-ATT-021]).
pub const CONFIRM_LIFE: Duration = Duration::from_secs(600);

/// The most confirmations kept. A thread that starts calls without end cannot grow the
/// adapter's memory: the oldest goes past the cap (availability only: its call is refused).
pub const MAX_CONFIRMATIONS: usize = 1024;

/// How deep [`json_equal`] looks. Deeper values are unequal (fail closed).
const MAX_DEPTH: usize = 64;

/// Equal JSON values: object members compared by name in any order, arrays in order,
/// numbers and strings as written. Values nested deeper than 64 levels are unequal.
pub fn json_equal(a: &Json, b: &Json) -> bool {
    fn eq(a: &Json, b: &Json, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        match (a, b) {
            (Json::Null, Json::Null) => true,
            (Json::Bool(x), Json::Bool(y)) => x == y,
            (Json::Number(x), Json::Number(y)) => x == y,
            (Json::String(x), Json::String(y)) => x == y,
            (Json::Array(x), Json::Array(y)) => {
                x.len() == y.len() && x.iter().zip(y).all(|(p, q)| eq(p, q, depth + 1))
            }
            (Json::Object(x), Json::Object(y)) => {
                x.len() == y.len()
                    && x.iter()
                        .all(|(k, v)| y.get(k).is_some_and(|w| eq(v, w, depth + 1)))
            }
            _ => false,
        }
    }
    eq(a, b, 0)
}

/// A call's `arguments` and an item's match ([MCPB-ATT-007]'s "matching arguments"): equal
/// JSON values, with `null` and absent the same on both sides. `call` is `Json::Null` for a
/// call with no `arguments` member ([`oac_mcp_tools::arguments_json`]); `item` is `None`
/// when the item has no `arguments` member.
pub fn arguments_match(call: &Json, item: Option<&Json>) -> bool {
    json_equal(call, item.unwrap_or(&Json::Null))
}

/// One confirmation: an `item/started` of a call by a served thread.
struct Entry {
    thread: ThreadKey,
    turn: NativeRef,
    item: NativeRef,
    tool: String,
    arguments: Json,
    arrived: Instant,
    claimed: bool,
}

/// A claimed confirmation: which item confirmed the call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claim {
    /// The thread whose report confirmed it.
    pub thread: ThreadKey,
    /// The confirming item's native id (matching only; never printed).
    pub item: NativeRef,
}

#[derive(Default)]
struct State {
    /// Oldest first.
    entries: Vec<Entry>,
    closed: bool,
}

/// The live confirmations of every served thread.
pub struct Confirmations {
    state: Mutex<State>,
    arrived: Condvar,
    life: Duration,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Confirmations {
    /// An engine whose confirmations live `life` ([`CONFIRM_LIFE`] is the reference).
    pub fn new(life: Duration) -> Confirmations {
        Confirmations {
            state: Mutex::new(State::default()),
            arrived: Condvar::new(),
            life,
        }
    }

    fn live(&self, e: &Entry, now: Instant) -> bool {
        now.saturating_duration_since(e.arrived) < self.life
    }

    /// An `item/started` of an `mcpToolCall` of the registered server arrived at `now`.
    /// `arguments` is the item's, with `None` (absent) kept as it is.
    pub fn started(
        &self,
        thread: ThreadKey,
        turn: &NativeRef,
        item: &NativeRef,
        tool: &str,
        arguments: Option<&Json>,
        now: Instant,
    ) {
        let mut st = lock(&self.state);
        st.entries.retain(|e| self.live(e, now));
        // The same item twice is one confirmation, not two (a replayed notification must not
        // make a second use, [MCPB-ATT-014]).
        if st
            .entries
            .iter()
            .any(|e| e.thread == thread && e.item == *item)
        {
            return;
        }
        if st.entries.len() >= MAX_CONFIRMATIONS {
            st.entries.remove(0);
        }
        st.entries.push(Entry {
            thread,
            turn: turn.clone(),
            item: item.clone(),
            tool: tool.to_owned(),
            arguments: arguments.cloned().unwrap_or(Json::Null),
            arrived: now,
            claimed: false,
        });
        self.arrived.notify_all();
    }

    /// The item's `item/completed` arrived: its confirmation is over ([MCPB-ATT-021]).
    pub fn completed(&self, thread: ThreadKey, item: &NativeRef) {
        lock(&self.state)
            .entries
            .retain(|e| !(e.thread == thread && e.item == *item));
    }

    /// The turn's `turn/completed` arrived: every confirmation of that turn of that thread is
    /// over ([MCPB-ATT-021]).
    pub fn turn_completed(&self, thread: ThreadKey, turn: &NativeRef) {
        lock(&self.state)
            .entries
            .retain(|e| !(e.thread == thread && e.turn == *turn));
    }

    /// Every confirmation of `thread` is over (the thread is no longer served).
    pub fn forget_thread(&self, thread: ThreadKey) {
        lock(&self.state).entries.retain(|e| e.thread != thread);
    }

    fn find(
        &self,
        st: &mut State,
        thread: ThreadKey,
        tool: &str,
        arguments: &Json,
        now: Instant,
    ) -> Option<Claim> {
        let life = self.life;
        let e = st.entries.iter_mut().find(|e| {
            e.thread == thread
                && !e.claimed
                && now.saturating_duration_since(e.arrived) < life
                && e.tool == tool
                && arguments_match(arguments, Some(&e.arguments))
        })?;
        e.claimed = true;
        Some(Claim {
            thread: e.thread,
            item: e.item.clone(),
        })
    }

    /// Claim, at `now`, the oldest live unclaimed confirmation of `thread` for a call of
    /// `tool` with `arguments` (`Json::Null` for none); `None` when there is none. A claim is
    /// single use ([MCPB-ATT-014]).
    pub fn try_claim(
        &self,
        thread: ThreadKey,
        tool: &str,
        arguments: &Json,
        now: Instant,
    ) -> Option<Claim> {
        let mut st = lock(&self.state);
        self.find(&mut st, thread, tool, arguments, now)
    }

    /// As [`Confirmations::try_claim`], waiting up to `wait` for a confirmation that has not
    /// arrived yet ([MCPB-ATT-013], [MCPB-ATT-020]). The wait is on the arrival of a
    /// confirmation, not a poll; it ends at once on [`Confirmations::close`]. `clock` gives
    /// the time liveness is judged at.
    pub fn claim_wait(
        &self,
        thread: ThreadKey,
        tool: &str,
        arguments: &Json,
        wait: Duration,
        clock: &dyn Fn() -> Instant,
    ) -> Option<Claim> {
        let end = Instant::now() + wait;
        let mut st = lock(&self.state);
        loop {
            if let Some(c) = self.find(&mut st, thread, tool, arguments, clock()) {
                return Some(c);
            }
            let left = end.saturating_duration_since(Instant::now());
            if st.closed || left.is_zero() {
                return None;
            }
            st = self
                .arrived
                .wait_timeout(st, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// How many confirmations are live and unclaimed at `now`.
    pub fn unclaimed(&self, now: Instant) -> usize {
        lock(&self.state)
            .entries
            .iter()
            .filter(|e| !e.claimed && self.live(e, now))
            .count()
    }

    /// How many confirmations are kept, claimed or not.
    pub fn kept(&self) -> usize {
        lock(&self.state).entries.len()
    }

    /// Stop: every waiting claim returns `None` now, and later ones do not wait.
    pub fn close(&self) {
        lock(&self.state).closed = true;
        self.arrived.notify_all();
    }
}

/// A map from an item to a value, bounded: the oldest insertion goes past the cap.
pub(crate) struct Bounded<V> {
    cap: usize,
    order: std::collections::VecDeque<(ThreadKey, String)>,
    map: HashMap<(ThreadKey, String), V>,
}

impl<V> Bounded<V> {
    pub(crate) fn new(cap: usize) -> Bounded<V> {
        Bounded {
            cap,
            order: std::collections::VecDeque::new(),
            map: HashMap::new(),
        }
    }

    pub(crate) fn insert(&mut self, k: (ThreadKey, String), v: V) {
        if self.map.insert(k.clone(), v).is_none() {
            self.order.push_back(k);
            while self.order.len() > self.cap {
                if let Some(old) = self.order.pop_front() {
                    self.map.remove(&old);
                }
            }
        }
    }

    pub(crate) fn remove(&mut self, k: &(ThreadKey, String)) -> Option<V> {
        let v = self.map.remove(k)?;
        self.order.retain(|x| x != k);
        Some(v)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::threads::Registry;
    use oac_core::json::parse;

    fn j(s: &str) -> Json {
        parse(s.as_bytes()).unwrap()
    }

    fn keys() -> (ThreadKey, ThreadKey) {
        let mut r = Registry::default();
        (
            r.serve("01a0e550-1921-7000-93ef-383c5acff39f").0,
            r.serve("01a0e550-0000-7000-93ef-383c5acff39f").0,
        )
    }

    fn nr(s: &str) -> NativeRef {
        NativeRef::new(s)
    }

    #[test]
    fn null_and_absent_are_one_and_an_empty_object_matches_only_an_empty_object() {
        // The call has no arguments member (Null), the item reports null or omits it.
        assert!(arguments_match(&Json::Null, Some(&Json::Null)));
        assert!(arguments_match(&Json::Null, None));
        // `{}` is `{}`: the S1 capture, a call with no arguments sent and reported as `{}`.
        assert!(arguments_match(&j("{}"), Some(&j("{}"))));
        // ... and nothing else (spec section 4.5.2).
        assert!(!arguments_match(&j("{}"), Some(&Json::Null)));
        assert!(!arguments_match(&j("{}"), None));
        assert!(!arguments_match(&Json::Null, Some(&j("{}"))));
        // Equal values, members in any order; a different value, a missing or an extra member.
        let a = j(r#"{"to":"x","content":[{"type":"text","text":"hi"}]}"#);
        assert!(arguments_match(
            &a,
            Some(&j(r#"{"content":[{"text":"hi","type":"text"}],"to":"x"}"#))
        ));
        assert!(!arguments_match(
            &a,
            Some(&j(r#"{"to":"y","content":[{"type":"text","text":"hi"}]}"#))
        ));
        assert!(!arguments_match(&a, Some(&j(r#"{"to":"x"}"#))));
        assert!(!arguments_match(
            &j(r#"{"to":"x"}"#),
            Some(&j(r#"{"to":"x","extra":1}"#))
        ));
        // Arrays are ordered; numbers are as written.
        assert!(!arguments_match(&j("[1,2]"), Some(&j("[2,1]"))));
        assert!(!arguments_match(&j("1"), Some(&j("1.0"))));
        // Too deep is unequal, not a stack overflow.
        let deep = format!("{}{}", "[".repeat(100), "]".repeat(100));
        assert!(!json_equal(&j(&deep), &j(&deep)));
    }

    #[test]
    fn a_confirmation_is_claimed_once_in_either_order() {
        let (t, _) = keys();
        let c = Confirmations::new(CONFIRM_LIFE);
        let now = Instant::now();
        // The call finds nothing before the report.
        assert!(c.try_claim(t, "send", &Json::Null, now).is_none());
        c.started(t, &nr("turn"), &nr("i1"), "send", None, now);
        // Claimed by the first call, refused to the second ([MCPB-ATT-014]).
        let first = c.try_claim(t, "send", &Json::Null, now).expect("claimed");
        assert_eq!(first.item, nr("i1"));
        assert!(c.try_claim(t, "send", &Json::Null, now).is_none());
        // A replay of the same item/started is not a new confirmation.
        c.started(t, &nr("turn"), &nr("i1"), "send", None, now);
        assert!(c.try_claim(t, "send", &Json::Null, now).is_none());
        assert_eq!(c.kept(), 1);
        // Two reports, two calls, oldest first.
        c.started(t, &nr("turn"), &nr("i2"), "send", None, now);
        c.started(t, &nr("turn"), &nr("i3"), "send", None, now);
        assert_eq!(
            c.try_claim(t, "send", &Json::Null, now).unwrap().item,
            nr("i2")
        );
        assert_eq!(
            c.try_claim(t, "send", &Json::Null, now).unwrap().item,
            nr("i3")
        );
        assert!(c.try_claim(t, "send", &Json::Null, now).is_none());
    }

    #[test]
    fn a_confirmation_needs_the_same_thread_tool_and_arguments() {
        let (t, u) = keys();
        let c = Confirmations::new(CONFIRM_LIFE);
        let now = Instant::now();
        c.started(
            t,
            &nr("turn"),
            &nr("i"),
            "send",
            Some(&j(r#"{"to":"x"}"#)),
            now,
        );
        assert!(
            c.try_claim(u, "send", &j(r#"{"to":"x"}"#), now).is_none(),
            "another thread"
        );
        assert!(
            c.try_claim(t, "reply", &j(r#"{"to":"x"}"#), now).is_none(),
            "another tool"
        );
        assert!(
            c.try_claim(t, "send", &j(r#"{"to":"y"}"#), now).is_none(),
            "other arguments"
        );
        assert!(
            c.try_claim(t, "send", &Json::Null, now).is_none(),
            "no arguments"
        );
        assert!(c.try_claim(t, "send", &j(r#"{"to":"x"}"#), now).is_some());
    }

    #[test]
    fn a_confirmation_ends_with_its_item_its_turn_or_600_seconds() {
        let (t, u) = keys();
        let c = Confirmations::new(CONFIRM_LIFE);
        let now = Instant::now();
        // The item's item/completed.
        c.started(t, &nr("turn"), &nr("a"), "x", None, now);
        c.completed(t, &nr("a"));
        assert!(c.try_claim(t, "x", &Json::Null, now).is_none());
        // The turn's turn/completed, for that thread and that turn only.
        c.started(t, &nr("turn"), &nr("b"), "x", None, now);
        c.started(t, &nr("other-turn"), &nr("c"), "x", None, now);
        c.started(u, &nr("turn"), &nr("d"), "x", None, now);
        c.turn_completed(t, &nr("turn"));
        assert_eq!(c.try_claim(t, "x", &Json::Null, now).unwrap().item, nr("c"));
        assert_eq!(c.try_claim(u, "x", &Json::Null, now).unwrap().item, nr("d"));
        // 600 seconds: live just before, not at or after.
        c.started(t, &nr("turn"), &nr("e"), "x", None, now);
        c.started(t, &nr("turn"), &nr("f"), "x", None, now);
        let just_before = now + CONFIRM_LIFE - Duration::from_millis(1);
        assert_eq!(c.unclaimed(just_before), 2);
        assert!(c.try_claim(t, "x", &Json::Null, just_before).is_some());
        let late = now + CONFIRM_LIFE + Duration::from_secs(1);
        assert_eq!(c.unclaimed(late), 0);
        assert!(c.try_claim(t, "x", &Json::Null, late).is_none(), "601 s");
    }

    #[test]
    fn a_call_waits_for_its_report_and_gives_up_in_bound() {
        let (t, _) = keys();
        let c = std::sync::Arc::new(Confirmations::new(CONFIRM_LIFE));
        // The report comes while the call waits: claimed, long before the bound.
        let waiter = {
            let c = c.clone();
            std::thread::spawn(move || {
                c.claim_wait(t, "x", &Json::Null, Duration::from_secs(10), &Instant::now)
            })
        };
        std::thread::sleep(Duration::from_millis(30));
        c.started(t, &nr("turn"), &nr("a"), "x", None, Instant::now());
        let t0 = Instant::now();
        assert!(waiter.join().unwrap().is_some());
        assert!(t0.elapsed() < Duration::from_secs(5));
        // No report: given up at the bound, not before and not long after.
        let t0 = Instant::now();
        assert!(
            c.claim_wait(
                t,
                "x",
                &Json::Null,
                Duration::from_millis(150),
                &Instant::now
            )
            .is_none()
        );
        let e = t0.elapsed();
        assert!(
            e >= Duration::from_millis(140) && e < Duration::from_secs(5),
            "{e:?}"
        );
        // Close ends a wait at once.
        let waiter = {
            let c = c.clone();
            std::thread::spawn(move || {
                c.claim_wait(t, "x", &Json::Null, Duration::from_secs(30), &Instant::now)
            })
        };
        std::thread::sleep(Duration::from_millis(30));
        c.close();
        let t0 = Instant::now();
        assert!(waiter.join().unwrap().is_none());
        assert!(t0.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn the_cap_drops_the_oldest() {
        let (t, _) = keys();
        let c = Confirmations::new(CONFIRM_LIFE);
        let now = Instant::now();
        for i in 0..MAX_CONFIRMATIONS + 5 {
            c.started(t, &nr("turn"), &nr(&format!("i{i}")), "x", None, now);
        }
        assert_eq!(c.kept(), MAX_CONFIRMATIONS);
        assert_eq!(
            c.try_claim(t, "x", &Json::Null, now).unwrap().item,
            nr("i5")
        );
    }

    #[test]
    fn the_bounded_map_forgets_its_oldest() {
        let (t, u) = keys();
        let mut b = Bounded::new(2);
        b.insert((t, "a".into()), 1);
        b.insert((t, "b".into()), 2);
        b.insert((u, "c".into()), 3);
        assert_eq!(b.len(), 2);
        assert!(b.remove(&(t, "a".into())).is_none());
        assert_eq!(b.remove(&(t, "b".into())), Some(2));
        assert_eq!(b.remove(&(u, "c".into())), Some(3));
        assert_eq!(b.len(), 0);
    }
}
