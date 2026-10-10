// SPDX-License-Identifier: Apache-2.0

//! Threads and turns, mapped onto keys of the adapter's own (G6 acceptance item 3).
//!
//! A Codex thread id is a harness-native id. It never appears in public output or in
//! `health` ([IFC-TYP-092]): the rest of the adapter names a thread by a [`ThreadKey`], an
//! opaque number this process issued, and the registry is the one place that holds the
//! native id. Neutral session ids are not minted here: the core binds a session id to an
//! attachment ([IFC-ADP-007]); the thread's native id reaches the core only as the
//! `native_id` of a native signal, which is what that member is for (`spec/interfaces.md`
//! §4.10, §5.4; the reveal of `spec/bindings/mcp.md` §4.5.3, G8).
//!
//! The thread's `id` is its native id, never `sessionId` (`11-risks.md` row 22: whether
//! the two are always equal is open). Nothing here reads a thread's `preview`, `name`,
//! `source` or originator: an originator is not an identity (rows 37, 38), and previews can
//! carry another harness's prompt text (row 43).

use std::collections::HashMap;
use std::fmt;

/// An opaque key for one thread the client serves. It carries no native id; its
/// `Display` and `Debug` show only the number this process gave it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ThreadKey(u64);

impl fmt::Debug for ThreadKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ThreadKey(#{})", self.0)
    }
}

impl fmt::Display for ThreadKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "thread #{}", self.0)
    }
}

/// A native id of a turn or an item: kept for matching (`turn/completed` against
/// `turn/started`; G8's confirmations by item id), never printed.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct NativeRef(String);

impl NativeRef {
    pub(crate) fn new(s: &str) -> NativeRef {
        NativeRef(s.to_owned())
    }

    /// The native value, for matching only. Never put it in health, a log or anything
    /// that leaves the adapter.
    pub fn native(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for NativeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeRef(..)")
    }
}

/// A thread's state, from `thread/status/changed` (`ThreadStatus` of the schema).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadState {
    /// No status seen yet.
    Unknown,
    /// `notLoaded`.
    NotLoaded,
    /// `idle`.
    Idle,
    /// `active`.
    Active,
    /// `systemError`.
    SystemError,
}

impl ThreadState {
    pub(crate) fn from_type(t: &str) -> Option<ThreadState> {
        Some(match t {
            "notLoaded" => ThreadState::NotLoaded,
            "idle" => ThreadState::Idle,
            "active" => ThreadState::Active,
            "systemError" => ThreadState::SystemError,
            _ => return None,
        })
    }
}

/// How a turn ended (`TurnStatus` of the schema, less `inProgress`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnEnd {
    /// `completed`.
    Completed,
    /// `interrupted`.
    Interrupted,
    /// `failed`.
    Failed,
}

impl TurnEnd {
    pub(crate) fn from_status(s: &str) -> Option<TurnEnd> {
        Some(match s {
            "completed" => TurnEnd::Completed,
            "interrupted" => TurnEnd::Interrupted,
            "failed" => TurnEnd::Failed,
            _ => return None,
        })
    }
}

/// What the registry knows of one thread.
#[derive(Clone)]
struct Entry {
    native: String,
    state: ThreadState,
    active_turn: Option<NativeRef>,
    last_turn: Option<TurnEnd>,
}

/// What the client can say of one served thread, without its native id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadView {
    /// The key.
    pub key: ThreadKey,
    /// The last status seen.
    pub state: ThreadState,
    /// True while a turn the client saw start has not been seen to end.
    pub turn_running: bool,
    /// How the last turn the client saw end ended.
    pub last_turn: Option<TurnEnd>,
}

/// The served threads: the subscribed threads of the carrier (`spec/bindings/mcp.md`
/// §4.5.2). A notification for any other thread is dropped by the client (row 70).
#[derive(Default)]
pub struct Registry {
    by_key: HashMap<ThreadKey, Entry>,
    by_native: HashMap<String, ThreadKey>,
    next: u64,
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("threads", &self.by_key.len())
            .finish_non_exhaustive()
    }
}

impl Registry {
    /// Serve `native`, or return its key if already served.
    pub fn serve(&mut self, native: &str) -> (ThreadKey, bool) {
        if let Some(k) = self.by_native.get(native) {
            return (*k, false);
        }
        self.next += 1;
        let k = ThreadKey(self.next);
        self.by_native.insert(native.to_owned(), k);
        self.by_key.insert(
            k,
            Entry {
                native: native.to_owned(),
                state: ThreadState::Unknown,
                active_turn: None,
                last_turn: None,
            },
        );
        (k, true)
    }

    /// Stop serving `key`; its native id, when it was served.
    pub fn drop_key(&mut self, key: ThreadKey) -> Option<String> {
        let e = self.by_key.remove(&key)?;
        self.by_native.remove(&e.native);
        Some(e.native)
    }

    /// The key of a served thread.
    pub fn key_of(&self, native: &str) -> Option<ThreadKey> {
        self.by_native.get(native).copied()
    }

    /// The native id of a served thread: crate-internal (the queue add, G8's native
    /// signal).
    pub(crate) fn native_of(&self, key: ThreadKey) -> Option<&str> {
        self.by_key.get(&key).map(|e| e.native.as_str())
    }

    /// How many threads are served.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    /// True when no thread is served.
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// Every served thread's view.
    pub fn views(&self) -> Vec<ThreadView> {
        let mut v: Vec<ThreadView> = self.by_key.keys().filter_map(|k| self.view(*k)).collect();
        v.sort_by_key(|x| x.key);
        v
    }

    /// One thread's view.
    pub fn view(&self, key: ThreadKey) -> Option<ThreadView> {
        self.by_key.get(&key).map(|e| ThreadView {
            key,
            state: e.state,
            turn_running: e.active_turn.is_some(),
            last_turn: e.last_turn,
        })
    }

    pub(crate) fn set_state(&mut self, key: ThreadKey, s: ThreadState) {
        if let Some(e) = self.by_key.get_mut(&key) {
            e.state = s;
        }
    }

    pub(crate) fn turn_started(&mut self, key: ThreadKey, turn: &NativeRef) {
        if let Some(e) = self.by_key.get_mut(&key) {
            e.active_turn = Some(turn.clone());
        }
    }

    pub(crate) fn turn_ended(&mut self, key: ThreadKey, turn: &NativeRef, end: TurnEnd) {
        if let Some(e) = self.by_key.get_mut(&key) {
            if e.active_turn.as_ref() == Some(turn) {
                e.active_turn = None;
            }
            e.last_turn = Some(end);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "01a1186d-b327-74f2-ac20-e3c5d0a9e81c";

    #[test]
    fn keys_hide_the_native_id() {
        let mut r = Registry::default();
        let (k, new) = r.serve(T);
        assert!(new);
        assert_eq!(r.serve(T), (k, false));
        for shown in [format!("{k}"), format!("{k:?}"), format!("{r:?}")] {
            assert!(!shown.contains("01a1186d"), "{shown}");
        }
        assert!(!format!("{:?}", r.views()).contains("01a1186d"));
        assert!(!format!("{:?}", NativeRef::new(T)).contains("01a1186d"));
        assert_eq!(r.native_of(k), Some(T));
        assert_eq!(r.drop_key(k).as_deref(), Some(T));
        assert!(r.key_of(T).is_none());
        assert!(r.is_empty());
    }

    #[test]
    fn turns_are_tracked_by_their_native_ref() {
        let mut r = Registry::default();
        let (k, _) = r.serve(T);
        let a = NativeRef::new("turn-a");
        r.turn_started(k, &a);
        assert!(r.view(k).unwrap().turn_running);
        r.turn_ended(k, &NativeRef::new("turn-b"), TurnEnd::Completed);
        assert!(r.view(k).unwrap().turn_running, "another turn's end");
        r.turn_ended(k, &a, TurnEnd::Interrupted);
        let v = r.view(k).unwrap();
        assert!(!v.turn_running);
        assert_eq!(v.last_turn, Some(TurnEnd::Interrupted));
    }
}
