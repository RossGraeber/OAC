// SPDX-License-Identifier: Apache-2.0

//! Authorization (`spec/security.md` §9; `spec/interfaces.md` §4.9): grants, the binding
//! table, reply rights, the five kinds of decision, and security step 4 of Table 7.1.
//!
//! # Default deny
//!
//! Nothing is permitted until a recorded fact permits it ([SEC-AUZ-001]). An
//! [`AuthorizationEngine`] starts with no grant, no reply right, no hand-off record and no
//! operator setting, so every decision it makes is `deny` until an operator adds a grant
//! or the engine's own sessions send or are handed messages. A [`Outcome::Permit`] cannot
//! be built without the [`Basis`] that permits it ([IFC-TYP-081]), and only the engine
//! builds decisions.
//!
//! # What a decision may read
//!
//! An [`AuthorizationRequest`] holds, for its kind, only the per-question inputs Table 4.9
//! lists ([IFC-TYP-080]). Everything else a decision reads is a fact the engine recorded
//! itself: the grants, the binding table, the own-session facts, and the sent and hand-off
//! records. No request has a slot for a display form, an alias, a `display_name`, a harness
//! label, a principal label on its own, a transport peer identifier, a harness-native id, a
//! cross-check value, a memory reference or anything from `content` ([SEC-AUZ-004],
//! [SEC-AUZ-024]). A `deliver` request is built only from a verified [`ChannelMessage`]
//! ([`AuthorizationRequest::deliver`]), so the key id it carries is the one that verified
//! the signature, never a claimed one.
//!
//! # Authenticated but untrusted
//!
//! A permit of kind `deliver` authorizes delivery of the message into the addressed
//! session's input, and nothing else. It never authorizes an action the message's content
//! asks for ([SEC-AUZ-020]): approving a permission request is its own kind,
//! `relay-permission`, off unless an operator enabled it for the session ([SEC-AUZ-021]),
//! and a decision of one kind is never read as a decision of another ([IFC-TYP-082];
//! [`AuthorizationDecision::permits`]). There is no kind that enables steering a running
//! turn, because no decision may enable it ([SEC-AUZ-022]).
//!
//! # Logging
//!
//! Every decision the engine makes is written to its [`DecisionLog`], with the principal it
//! concerns and the session ids involved, never with a message body: a [`DecisionRecord`]
//! is built from the request, which has no slot for content.

use crate::clock::Clock;
use crate::delivery::{DeliveryState, ErrorCode};
use crate::envelope::{ChannelMessage, Envelope, SecurityPrincipal};
use crate::ids::{KeyId, SessionId, Timestamp, Token};
use crate::keys::DeviceIdentity;
use crate::pairing::{PairedPeer, PairingRecord, PairingSnapshot, PairingStore, PairingStoreError};
use crate::registration::RegistrationRecord;
use crate::reply::{EnvelopeRecord, ReplyHeaders, reply_headers};
use crate::trust::{AddKeyError, TrustedKeySet};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};

/// The reply period of `spec/security.md` §9.5: 86400000 milliseconds (24 hours).
pub const REPLY_PERIOD_MS: u64 = 86_400_000;

/// The most sent records, or hand-off records, one partition holds (#313). Both live in
/// process memory ([SEC-AUZ-013], [SEC-AUZ-016]), so they are bounded, and partitioned so
/// that no writer can evict another's records:
///
/// - sent records by the own session that sent them: one local session's sends evict only
///   its own reply rights;
/// - hand-off records by the key that verified the envelope, and, for this device's own
///   key, by the sending session too: one peer device, or one local session, evicts only
///   the records of what it sent itself.
///
/// A full partition forgets its own records whose reply period has ended first, then its
/// oldest. Forgetting fails closed: a reply right, a discovery right or a correlation ends
/// early for that writer alone (`docs/planning/v0.1/11-risks.md` RISK-RECORD-PARTITIONS).
pub const MAX_RECORDS_PER_PARTITION: usize = 4096;

/// The most partitions each record list holds at once. A partition goes as soon as it is
/// empty, when its session ends (sent records, and an own sender's hand-off records), when
/// its key is removed from trust, and as soon as every record in it is past its reply
/// period (found in order of expiry, #328). A record for a new partition is not kept
/// (fail-closed) only while this many partitions all hold a live record; no live partition
/// is evicted for it.
///
/// A hand-off record from an own session that is no longer bound is not kept either
/// (#328): a late copy from an ended own session would otherwise make that session's
/// partition again, after [`AuthorizationEngine::end_session`] dropped it.
pub const MAX_RECORD_PARTITIONS: usize = 4096;

/// The most records each record list holds in all, across its partitions. Past it, the
/// largest partition gives up its oldest record, so a writer keeps at least its fair share
/// (this over the number of partitions) whatever the others do.
pub const MAX_RECORDS_TOTAL: usize = 262_144;

/// The most binding-table entries that security step 4 creates (#325) one key holds:
/// twice [`MAX_RECORDS_PER_PARTITION`], so a key at it always holds an entry that no
/// hand-off record is looked up through.
pub const MAX_ENVELOPE_BINDINGS_PER_KEY: usize = 2 * MAX_RECORDS_PER_PARTITION;

/// The most binding-table entries that security step 4 creates (#325), in all.
///
/// An envelope that passes step 4 binds its unbound `from` to the verifying key
/// ([SEC-PRS-005]). Such an entry is counted here until something else refers to it: an
/// own session's registration replaces it, a presence record confirms it
/// ([`AuthorizationEngine::bind`], after which the presence registry bounds it and
/// [`AuthorizationEngine::forget_binding`] removes it), or it becomes a conflict mark. To
/// add one:
///
/// - **Share.** A key holds at most [`MAX_ENVELOPE_BINDINGS_PER_KEY`]; at it, the key's
///   own oldest entry goes.
/// - **Fair share.** In a full table, a key holding `n` takes the oldest entry of a key that
///   holds at least `n + 2`, the most of any; among the keys holding the most, the one whose
///   latest entry is newest gives it up, so no key can steer the eviction onto another by
///   its key id or its session ids.
/// - **Kept.** An entry a hand-off record is looked up through ([SEC-AUZ-016],
///   [SC-RCP-053], [SC-RCP-054]) is never evicted. A conflict mark and an own session's
///   binding are never counted, so never evicted.
///
/// With no entry to evict, the envelope is refused with `failed` and `internal-error`, as
/// a full duplicate store refuses it, and nothing is bound. Removing an entry is what
/// [SEC-PRS-009] permits once the session is forgotten: the presence registry holds no
/// record of it. A later envelope or announcement binds it again.
pub const MAX_ENVELOPE_BINDINGS: usize = 65_536;

/// A record that carries the instant its reply period starts and the session that sent its
/// envelope.
trait Dated {
    fn created_at(&self) -> &Timestamp;
    fn sender(&self) -> &SessionId;
}

impl Dated for SentRecord {
    fn created_at(&self) -> &Timestamp {
        &self.created_at
    }
    fn sender(&self) -> &SessionId {
        &self.from
    }
}

impl Dated for HandOffRecord {
    fn created_at(&self) -> &Timestamp {
        &self.created_at
    }
    fn sender(&self) -> &SessionId {
        &self.from
    }
}

/// The instant, in Unix nanoseconds, at which a record created at `t` stops counting.
fn period_end(t: &Timestamp) -> i128 {
    t.unix_nanos() + i128::from(REPLY_PERIOD_MS) * NANOS_PER_MS
}

/// Records per (partition, sending session), kept for the hand-off records only, and the
/// changes the engine reads to keep envelope-created bindings it must not evict (#325).
#[derive(Debug)]
struct SenderIndex<K> {
    enabled: bool,
    counts: BTreeMap<(K, SessionId), usize>,
    changes: Vec<(K, SessionId, bool)>,
}

impl<K: Ord + Clone> SenderIndex<K> {
    fn add(&mut self, key: &K, sender: &SessionId) {
        if !self.enabled {
            return;
        }
        let n = self
            .counts
            .entry((key.clone(), sender.clone()))
            .or_default();
        *n += 1;
        if *n == 1 {
            self.changes.push((key.clone(), sender.clone(), true));
        }
    }

    fn remove(&mut self, key: &K, sender: &SessionId) {
        let k = (key.clone(), sender.clone());
        let Some(n) = self.counts.get_mut(&k) else {
            return;
        };
        *n -= 1;
        if *n == 0 {
            self.counts.remove(&k);
            self.changes.push((key.clone(), sender.clone(), false));
        }
    }
}

#[derive(Debug)]
struct Partition<R> {
    records: VecDeque<R>,
    /// The latest reply-period end of any record it ever held: once that is past, every
    /// record in it is.
    ends: i128,
}

/// A record list in partitions ([`MAX_RECORDS_PER_PARTITION`], [`MAX_RECORD_PARTITIONS`],
/// [`MAX_RECORDS_TOTAL`]). Adding a record touches its own partition, in amortized constant
/// time plus logarithmic index upkeep.
///
/// Whole expired partitions are found through `by_end`, the partitions ordered by the
/// instant they expire: a priority queue keyed on partition expiry whose entries are moved,
/// not duplicated, when a partition's expiry moves (#328). Each push looks at its first
/// entry only and drops the partitions that have expired, so finding them costs
/// `O(log n)` per partition dropped and nothing for the ones still live, whatever order
/// their writers refresh them in.
#[derive(Debug)]
struct RecordBook<K: Ord, R> {
    parts: BTreeMap<K, Partition<R>>,
    /// Partition sizes, for the largest one.
    by_size: BTreeSet<(usize, K)>,
    /// Partition expiries (`Partition::ends`), earliest first.
    by_end: BTreeSet<(i128, K)>,
    /// How many records each partition holds from each sending session (hand-off records
    /// only): a binding that a record is looked up through is not evicted (#325).
    senders: SenderIndex<K>,
    total: usize,
    /// [`MAX_RECORD_PARTITIONS`], or a smaller cap in a test.
    max_partitions: usize,
}

impl<K: Ord + Clone, R: Dated> RecordBook<K, R> {
    /// An empty list; `index_senders` keeps [`RecordBook::holds_sender`] and its changes.
    fn new(index_senders: bool) -> Self {
        RecordBook::with_max_partitions(MAX_RECORD_PARTITIONS, index_senders)
    }

    /// A list of at most `max_partitions` partitions: [`MAX_RECORD_PARTITIONS`] outside
    /// tests.
    fn with_max_partitions(max_partitions: usize, index_senders: bool) -> Self {
        RecordBook {
            parts: BTreeMap::new(),
            by_size: BTreeSet::new(),
            by_end: BTreeSet::new(),
            senders: SenderIndex {
                enabled: index_senders,
                counts: BTreeMap::new(),
                changes: Vec::new(),
            },
            total: 0,
            max_partitions,
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.total
    }

    #[cfg(test)]
    fn partitions(&self) -> usize {
        self.parts.len()
    }

    /// Whether partition `key` holds a record whose envelope `sender` sent (an indexed
    /// list only).
    fn holds_sender(&self, key: &K, sender: &SessionId) -> bool {
        self.senders
            .counts
            .contains_key(&(key.clone(), sender.clone()))
    }

    /// The (partition, sender) pairs that gained their first record (`true`) or lost their
    /// last (`false`) since the last call, in order.
    fn take_sender_changes(&mut self) -> Vec<(K, SessionId, bool)> {
        std::mem::take(&mut self.senders.changes)
    }

    /// Removes partition `key` from the indexes once it has gone from `parts`.
    fn forget_partition(&mut self, key: &K, len: usize, ends: i128) {
        self.by_size.remove(&(len, key.clone()));
        self.by_end.remove(&(ends, key.clone()));
    }

    /// Notes that partition `key` went from `before` records to `after`, dropping it when it
    /// is empty.
    fn resized(&mut self, key: &K, before: usize, after: usize) {
        if before == after {
            return;
        }
        self.total -= before - after;
        self.by_size.remove(&(before, key.clone()));
        if after > 0 {
            self.by_size.insert((after, key.clone()));
        } else if let Some(p) = self.parts.remove(key) {
            self.by_end.remove(&(p.ends, key.clone()));
        }
    }

    /// Removes one record from the front of partition `key`, dropping the partition when
    /// it empties.
    fn pop_front(&mut self, key: &K) {
        let Some(p) = self.parts.get_mut(key) else {
            return;
        };
        let n = p.records.len();
        let Some(r) = p.records.pop_front() else {
            return;
        };
        self.senders.remove(key, r.sender());
        self.resized(key, n, n - 1);
    }

    fn drop_partition(&mut self, key: &K) {
        if let Some(p) = self.parts.remove(key) {
            for r in &p.records {
                self.senders.remove(key, r.sender());
            }
            self.total -= p.records.len();
            self.forget_partition(key, p.records.len(), p.ends);
        }
    }

    /// Drops every partition whose records are all past their reply period at `now`: the
    /// first entries of `by_end`, and no others.
    fn sweep(&mut self, now: i128) {
        while self.by_end.first().is_some_and(|(ends, _)| *ends <= now) {
            // Taken off first, so each pass removes an entry and the loop ends even if an
            // entry outlived its partition.
            if let Some((_, key)) = self.by_end.pop_first() {
                self.drop_partition(&key);
            }
        }
    }

    /// Adds `r` to partition `key` at `now`; false, keeping nothing, when a new partition
    /// would pass the partition cap after the expired ones are dropped.
    fn push(&mut self, key: K, r: R, now: &Timestamp) -> bool {
        let now = now.unix_nanos();
        self.sweep(now);
        if !self.parts.contains_key(&key) && self.parts.len() >= self.max_partitions {
            return false;
        }
        // Its own expired records first, then its own oldest at the cap.
        while self
            .parts
            .get(&key)
            .and_then(|p| p.records.front())
            .is_some_and(|f| period_end(f.created_at()) <= now)
        {
            self.pop_front(&key);
        }
        if self
            .parts
            .get(&key)
            .is_some_and(|p| p.records.len() >= MAX_RECORDS_PER_PARTITION)
        {
            self.pop_front(&key);
        }
        // Past the total, the largest partition gives one up.
        if self.total >= MAX_RECORDS_TOTAL
            && let Some((_, largest)) = self.by_size.last().cloned()
        {
            self.pop_front(&largest);
        }
        let ends = period_end(r.created_at());
        self.senders.add(&key, r.sender());
        let p = self.parts.entry(key.clone()).or_insert(Partition {
            records: VecDeque::new(),
            ends,
        });
        let n = p.records.len();
        let before = p.ends;
        p.records.push_back(r);
        p.ends = p.ends.max(ends);
        let after = p.ends;
        if n > 0 {
            self.by_size.remove(&(n, key.clone()));
            if after != before {
                self.by_end.remove(&(before, key.clone()));
                self.by_end.insert((after, key.clone()));
            }
        } else {
            self.by_end.insert((after, key.clone()));
        }
        self.by_size.insert((n + 1, key));
        self.total += 1;
        true
    }

    fn iter(&self) -> impl Iterator<Item = &R> {
        self.parts.values().flat_map(|p| p.records.iter())
    }

    fn partition(&self, key: &K) -> impl Iterator<Item = &R> + use<'_, K, R> {
        self.parts
            .get(key)
            .into_iter()
            .flat_map(|p| p.records.iter())
    }

    /// Keeps only the records `keep` accepts, dropping emptied partitions. A full pass: for
    /// [`AuthorizationEngine::prune`] and session ends, never per record.
    fn retain(&mut self, mut keep: impl FnMut(&R) -> bool) {
        let keys: Vec<K> = self.parts.keys().cloned().collect();
        for k in keys {
            let Some(p) = self.parts.get_mut(&k) else {
                continue;
            };
            let before = p.records.len();
            let senders = &mut self.senders;
            p.records.retain(|r| {
                let kept = keep(r);
                if !kept {
                    senders.remove(&k, r.sender());
                }
                kept
            });
            let after = p.records.len();
            self.resized(&k, before, after);
        }
    }

    /// Drops every partition whose key `matches`.
    fn drop_partitions(&mut self, matches: impl Fn(&K) -> bool) {
        let keys: Vec<K> = self.parts.keys().filter(|k| matches(k)).cloned().collect();
        for k in &keys {
            self.drop_partition(k);
        }
    }

    /// Removes the first record of partition `key` that `matches`.
    fn remove_first(&mut self, key: &K, matches: impl Fn(&R) -> bool) -> bool {
        let Some(p) = self.parts.get_mut(key) else {
            return false;
        };
        let Some(i) = p.records.iter().position(matches) else {
            return false;
        };
        let n = p.records.len();
        if let Some(r) = p.records.remove(i) {
            self.senders.remove(key, r.sender());
        }
        self.resized(key, n, n - 1);
        true
    }
}

/// One binding-table entry an envelope created (#325).
#[derive(Debug)]
struct EnvelopeBinding {
    key: KeyId,
    /// Its place in creation order.
    seq: u64,
    /// A hand-off record is looked up through it, so it is not evicted.
    pinned: bool,
}

/// The envelope-created entries of one key.
#[derive(Debug, Default)]
struct KeyBindings {
    /// Entries no hand-off record is looked up through, oldest first: the ones that may go.
    free: BTreeMap<u64, SessionId>,
    pinned: usize,
    /// The creation order of the key's latest entry: among keys holding the most, the one
    /// that added last gives an entry up.
    latest: u64,
}

impl KeyBindings {
    fn len(&self) -> usize {
        self.free.len() + self.pinned
    }
}

/// The binding-table entries security step 4 created and nothing else refers to: not an
/// own session's, not a conflict mark, and not one a presence record has since confirmed
/// ([`AuthorizationEngine::bind`]), which the presence registry bounds. The rules are at
/// [`MAX_ENVELOPE_BINDINGS`].
#[derive(Debug)]
struct EnvelopeBindings {
    sessions: BTreeMap<SessionId, EnvelopeBinding>,
    keys: BTreeMap<KeyId, KeyBindings>,
    next: u64,
    per_key: usize,
    capacity: usize,
}

impl EnvelopeBindings {
    fn new(capacity: usize, per_key: usize) -> EnvelopeBindings {
        let capacity = capacity.max(1);
        EnvelopeBindings {
            sessions: BTreeMap::new(),
            keys: BTreeMap::new(),
            next: 0,
            per_key: per_key.clamp(1, capacity),
            capacity,
        }
    }

    fn held_by(&self, key: &KeyId) -> usize {
        self.keys.get(key).map_or(0, KeyBindings::len)
    }

    fn track(&mut self, session: SessionId, key: KeyId, pinned: bool) {
        self.untrack(&session);
        self.next += 1;
        let seq = self.next;
        let kb = self.keys.entry(key.clone()).or_default();
        kb.latest = seq;
        if pinned {
            kb.pinned += 1;
        } else {
            kb.free.insert(seq, session.clone());
        }
        self.sessions
            .insert(session, EnvelopeBinding { key, seq, pinned });
    }

    fn untrack(&mut self, session: &SessionId) {
        let Some(e) = self.sessions.remove(session) else {
            return;
        };
        if let Some(kb) = self.keys.get_mut(&e.key) {
            if e.pinned {
                kb.pinned -= 1;
            } else {
                kb.free.remove(&e.seq);
            }
            if kb.len() == 0 {
                self.keys.remove(&e.key);
            }
        }
    }

    fn set_pinned(&mut self, session: &SessionId, key: &KeyId, pinned: bool) {
        let Some(e) = self.sessions.get_mut(session) else {
            return;
        };
        if &e.key != key || e.pinned == pinned {
            return;
        }
        e.pinned = pinned;
        let Some(kb) = self.keys.get_mut(key) else {
            return;
        };
        if pinned {
            kb.free.remove(&e.seq);
            kb.pinned += 1;
        } else {
            kb.pinned -= 1;
            kb.free.insert(e.seq, session.clone());
        }
    }

    /// Whether `key` may add an entry with nothing evicted.
    fn has_room(&self, key: &KeyId) -> bool {
        self.held_by(key) < self.per_key && self.sessions.len() < self.capacity
    }

    /// The entry to evict so that `key` may add one; `None` when there is none to evict.
    ///
    /// - At its share, `key` gives up its own oldest unpinned entry.
    /// - Below its share in a full table, holding `n`: the oldest unpinned entry of the key
    ///   that holds the most of the keys holding at least `n + 2` (so that it still holds at
    ///   least as many as `key` after the insert) with an unpinned entry; among those, the
    ///   key whose latest entry is newest.
    fn victim(&self, key: &KeyId) -> Option<SessionId> {
        let n = self.held_by(key);
        if n >= self.per_key {
            return self.keys.get(key)?.free.values().next().cloned();
        }
        self.keys
            .iter()
            .filter(|(k, kb)| *k != key && kb.len() >= n + 2 && !kb.free.is_empty())
            .max_by_key(|(_, kb)| (kb.len(), kb.latest))
            .and_then(|(_, kb)| kb.free.values().next().cloned())
    }
}

/// The partition of a hand-off record: the verifying key, and the sending session when that
/// key is this device's own.
type HandOffPartition = (Option<KeyId>, Option<SessionId>);

const NANOS_PER_MS: i128 = 1_000_000;

/// True while `now` is before `created_at` plus the reply period. The period ends at that
/// instant: a record created exactly 24 hours before `now` no longer counts
/// (`sec-auz/SEC-AUZ-015.n01`, `SEC-AUZ-016.n02`).
fn within_reply_period(created_at: &Timestamp, now: &Timestamp) -> bool {
    now.unix_nanos() < created_at.unix_nanos() + i128::from(REPLY_PERIOD_MS) * NANOS_PER_MS
}

/// A working-directory scope (`spec/security.md` §9.3): the working directory a session's
/// registration record holds. Two scopes are the same only when equal as recorded, so a
/// subdirectory is another scope ([SEC-AUZ-008]). Local: never sent to a peer
/// ([SEC-KEY-042]), and never written into a [`DecisionRecord`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkingDirectoryScope(String);

impl WorkingDirectoryScope {
    /// The scope recorded as `s`, unchanged.
    pub fn new(s: impl Into<String>) -> WorkingDirectoryScope {
        WorkingDirectoryScope(s.into())
    }

    /// The scope as recorded.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for WorkingDirectoryScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WorkingDirectoryScope(..)")
    }
}

/// The side of a grant on this implementation's own device (`spec/security.md` §9.2): one of
/// its sessions, one of its working-directory scopes, or the whole device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalSide {
    /// One session. As a target it names that id, and covers it until an operator removes
    /// the grant, even after the id's binding has ended (§9.2, dated note after
    /// [SEC-AUZ-006]); it never covers another id ([SEC-AUZ-006]).
    Session(SessionId),
    /// Every session registered with this scope at the time of the check ("folder-wide").
    Scope(WorkingDirectoryScope),
    /// Every session this implementation binds at the time of the check ("machine-wide").
    Device,
}

impl LocalSide {
    /// Whether this side includes the session `s`, given the own-session facts.
    fn includes(&self, s: &SessionId, own: &BTreeMap<SessionId, WorkingDirectoryScope>) -> bool {
        match self {
            LocalSide::Session(id) => id == s,
            LocalSide::Scope(scope) => own.get(s) == Some(scope),
            LocalSide::Device => own.contains_key(s),
        }
    }
}

/// The side of a grant on another device (`spec/security.md` §9.2): a key id (any session
/// bound to that device key), or a key id and a session id (that one session).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerSide {
    /// The device's key id.
    pub key_id: KeyId,
    /// One session of that device, or `None` for any session bound to its key.
    pub session_id: Option<SessionId>,
}

impl PeerSide {
    /// Every session bound to `key_id`.
    pub fn device(key_id: KeyId) -> PeerSide {
        PeerSide {
            key_id,
            session_id: None,
        }
    }

    /// The one session `session_id` of the device `key_id`.
    pub fn session(key_id: KeyId, session_id: SessionId) -> PeerSide {
        PeerSide {
            key_id,
            session_id: Some(session_id),
        }
    }

    /// Whether this side names the key `key` together with `session`, or `key` with no
    /// session.
    fn names(&self, key: &KeyId, session: &SessionId) -> bool {
        &self.key_id == key && self.session_id.as_ref().is_none_or(|s| s == session)
    }
}

/// A grant (`spec/security.md` §9.2): a **writer** may send to a **target**. One-way: it
/// never lets the target send to the writer. An operator records it on both
/// implementations involved, each in the form it evaluates. Between two sessions of one
/// implementation the inbound grant alone serves, with the implementation's own key id as
/// the writer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Grant {
    /// On the target's implementation: a peer (or this device's own key) may write to a
    /// local session, scope or the device.
    Inbound {
        /// Who may write.
        writer: PeerSide,
        /// What it may write to.
        target: LocalSide,
    },
    /// On the writer's implementation: a local session, scope or the device may write to a
    /// peer device or one of its sessions.
    Outbound {
        /// Who may write.
        writer: LocalSide,
        /// What it may write to.
        target: PeerSide,
    },
}

impl Grant {
    /// Whether the grant names the key id `key` ([SEC-KEY-035]).
    pub fn names_key(&self, key: &KeyId) -> bool {
        match self {
            Grant::Inbound { writer, .. } => &writer.key_id == key,
            Grant::Outbound { target, .. } => &target.key_id == key,
        }
    }
}

/// An entry of the binding table (`spec/security.md` §11.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Binding {
    /// The session id is bound to this key id.
    Key(KeyId),
    /// A conflict mark: these key ids each claimed the session id. While it stands, the
    /// session id is bound to no key and nothing claiming it is accepted ([SEC-PRS-012]).
    Conflict(BTreeSet<KeyId>),
}

impl Binding {
    /// Whether the entry names `key`.
    fn names(&self, key: &KeyId) -> bool {
        match self {
            Binding::Key(k) => k == key,
            Binding::Conflict(keys) => keys.contains(key),
        }
    }
}

/// What [`AuthorizationEngine::bind`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindOutcome {
    /// The session id had no entry, and is now bound to the key.
    Bound,
    /// The session id was already bound to the same key.
    AlreadyBound,
    /// The session id is bound to another key. Nothing changed; the caller records a
    /// finding ([SEC-PRS-004]).
    BoundToOther(KeyId),
    /// The session id is under a conflict mark. Nothing changed ([SEC-PRS-012]).
    UnderConflict,
}

/// A record of an envelope one of this implementation's own sessions passed to a
/// transport ([SEC-AUZ-013]): the source of a reply right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SentRecord {
    /// The envelope's `id`.
    pub id: Token,
    /// The sending session, one of this implementation's own.
    pub from: SessionId,
    /// The addressed session.
    pub to: SessionId,
    /// The key `to` was bound to when the envelope was sent.
    pub to_key_id: KeyId,
    /// The envelope's `created_at`.
    pub created_at: Timestamp,
    /// The envelope's `security.nonce`: with `id`, `from` and `to`, what an authenticated
    /// receipt must name to be accepted (`spec/security.md` [SEC-RCT-003], check 3; #55, F6).
    /// `None` only for a record built without its envelope, which then matches no receipt.
    pub nonce: Option<String>,
}

impl SentRecord {
    /// The record for `env`, sent while `to` was bound to `to_key`.
    pub fn of(env: &Envelope, to_key: KeyId) -> SentRecord {
        SentRecord {
            id: env.id().clone(),
            from: env.from().clone(),
            to: env.to().clone(),
            to_key_id: to_key,
            created_at: env.created_at().clone(),
            nonce: Some(env.security().nonce().to_owned()),
        }
    }
}

/// A record of an envelope this implementation handed off to one of its own sessions with
/// the outcome `handed-to-harness` or `unknown` ([SEC-AUZ-016]).
///
/// It is also the hand-off record of `spec/session-channels.md` §8.2.2, so it keeps the
/// envelope's `conversation_id` and `correlation_id`: a reply copies them from here, never
/// from the harness ([SC-RCP-053], [SC-RCP-054]; #313).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandOffRecord {
    /// The envelope's `id`.
    pub id: Token,
    /// The envelope's `from`.
    pub from: SessionId,
    /// The receiving session, one of this implementation's own.
    pub to: SessionId,
    /// The envelope's `created_at`.
    pub created_at: Timestamp,
    /// The envelope's `conversation_id`, when present.
    pub conversation_id: Option<Token>,
    /// The envelope's `correlation_id`, when present.
    pub correlation_id: Option<Token>,
    /// The key id that verified the envelope: the partition the record is kept in
    /// ([`MAX_RECORDS_PER_PARTITION`]). `None` for a record built without one.
    pub key_id: Option<KeyId>,
}

impl HandOffRecord {
    /// The record for `env`, an envelope that passed security steps 1 and 2, so its
    /// `security.key_id` is the key that verified it.
    pub fn of(env: &Envelope) -> HandOffRecord {
        HandOffRecord {
            id: env.id().clone(),
            from: env.from().clone(),
            to: env.to().clone(),
            created_at: env.created_at().clone(),
            conversation_id: env.conversation_id().cloned(),
            correlation_id: env.correlation_id().cloned(),
            key_id: KeyId::parse(env.security().key_id()),
        }
    }

    /// The record as [`crate::reply`] reads it.
    pub fn envelope_record(&self) -> EnvelopeRecord {
        EnvelopeRecord {
            id: self.id.clone(),
            from: self.from.clone(),
            to: self.to.clone(),
            conversation_id: self.conversation_id.clone(),
            correlation_id: self.correlation_id.clone(),
        }
    }
}

/// The kinds of question of `spec/interfaces.md` Table 4.9.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// Does an inbound grant or a live reply right cover this verified envelope?
    Deliver,
    /// May this requester discover this session?
    Discover,
    /// May this own session's presence record be released to this device?
    ReleasePresence,
    /// Is this announcement's signing key related to this consumer for this session?
    AcceptPresence,
    /// May a peer message answer this session's permission request?
    RelayPermission,
}

impl Kind {
    /// The kind's name in Table 4.9.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Deliver => "deliver",
            Kind::Discover => "discover",
            Kind::ReleasePresence => "release-presence",
            Kind::AcceptPresence => "accept-presence",
            Kind::RelayPermission => "relay-permission",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The per-question inputs of a `deliver` request. Built only from a verified message, so
/// the key id is the one that verified the signature ([SEC-STG-003]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliverQuestion {
    verifying_key: KeyId,
    from: SessionId,
    to: SessionId,
    reply_to: Option<Token>,
}

impl DeliverQuestion {
    /// The key id that verified the envelope.
    pub fn verifying_key(&self) -> &KeyId {
        &self.verifying_key
    }

    /// The envelope's `from`.
    pub fn from(&self) -> &SessionId {
        &self.from
    }

    /// The envelope's `to`.
    pub fn to(&self) -> &SessionId {
        &self.to
    }

    /// The envelope's `reply_to`, if any.
    pub fn reply_to(&self) -> Option<&Token> {
        self.reply_to.as_ref()
    }
}

/// Who asks to discover a session (Table 4.9, kind `discover`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Requester {
    /// One of this implementation's own sessions.
    Session(SessionId),
    /// A peer device, by key id: may this implementation release the session's presence
    /// record to it ([SEC-AUZ-011])?
    Device(KeyId),
}

/// One question of `spec/security.md` §9 (`spec/interfaces.md` §4.9). Each kind holds only
/// the per-question inputs Table 4.9 lists for it ([IFC-TYP-080]); the recorded facts it
/// may also read are the engine's own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthorizationRequest {
    /// Kind `deliver`: the verifying key id and the envelope's `from`, `to` and
    /// `reply_to`.
    Deliver(DeliverQuestion),
    /// Kind `discover`: the requester and the session id.
    Discover {
        /// An own session id, or a peer device's key id.
        requester: Requester,
        /// The session to discover.
        session: SessionId,
    },
    /// Kind `release-presence`: the session id and the device's key id.
    ReleasePresence {
        /// One of this implementation's own sessions.
        session: SessionId,
        /// The peer device.
        device: KeyId,
    },
    /// Kind `accept-presence`: the signing key id and the session id.
    AcceptPresence {
        /// The key that signed the announcement (after it verified).
        signing_key: KeyId,
        /// The session the announcement is for.
        session: SessionId,
    },
    /// Kind `relay-permission`: the session id.
    RelayPermission {
        /// The session whose permission request a peer message would answer.
        session: SessionId,
    },
}

impl AuthorizationRequest {
    /// The `deliver` request for `msg`: its verifying key id, `from`, `to` and `reply_to`,
    /// and nothing from `content`. `None` when `msg` has not passed security steps 1 and 2
    /// ([IFC-TYP-041], [SEC-STG-003]).
    pub fn deliver(msg: &ChannelMessage) -> Option<AuthorizationRequest> {
        let by = msg.verified_by()?;
        let env = msg.envelope();
        Some(AuthorizationRequest::Deliver(DeliverQuestion {
            verifying_key: by.key_id().clone(),
            from: env.from().clone(),
            to: env.to().clone(),
            reply_to: env.reply_to().cloned(),
        }))
    }

    /// The request's kind.
    pub fn kind(&self) -> Kind {
        match self {
            AuthorizationRequest::Deliver(_) => Kind::Deliver,
            AuthorizationRequest::Discover { .. } => Kind::Discover,
            AuthorizationRequest::ReleasePresence { .. } => Kind::ReleasePresence,
            AuthorizationRequest::AcceptPresence { .. } => Kind::AcceptPresence,
            AuthorizationRequest::RelayPermission { .. } => Kind::RelayPermission,
        }
    }
}

/// The recorded fact that permits a decision ([IFC-TYP-081]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Basis {
    /// An inbound grant.
    InboundGrant(Grant),
    /// An outbound grant.
    OutboundGrant(Grant),
    /// A live reply right, from this sent record ([SEC-AUZ-014], [SEC-AUZ-015]).
    ReplyRight(SentRecord),
    /// A sent record within the reply period ([SEC-AUZ-017]).
    Sent(SentRecord),
    /// A hand-off record within the reply period ([SEC-AUZ-016], [SEC-AUZ-017]).
    HandOff(HandOffRecord),
    /// The operator enabled permission relay for this session ([SEC-AUZ-021]).
    RelaySetting(SessionId),
}

impl Basis {
    /// A short label for a log line.
    pub fn label(&self) -> &'static str {
        match self {
            Basis::InboundGrant(_) => "inbound-grant",
            Basis::OutboundGrant(_) => "outbound-grant",
            Basis::ReplyRight(_) => "reply-right",
            Basis::Sent(_) => "sent-record",
            Basis::HandOff(_) => "hand-off-record",
            Basis::RelaySetting(_) => "relay-setting",
        }
    }
}

/// `permit`, with its basis, or `deny`.
// A decision is a short-lived value; boxing the basis would change a public shape for no
// gain once `HandOffRecord` carries its partition key (#313).
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Permitted, because of this recorded fact.
    Permit(Basis),
    /// Denied: no recorded fact permits it. The default.
    Deny,
}

/// The answer to one [`AuthorizationRequest`] (`spec/interfaces.md` §4.9). Only the engine
/// builds one, so a `permit` always names its basis ([IFC-TYP-081]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizationDecision {
    kind: Kind,
    outcome: Outcome,
}

impl AuthorizationDecision {
    /// The kind of question this decision answers.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// `permit` with its basis, or `deny`.
    pub fn outcome(&self) -> &Outcome {
        &self.outcome
    }

    /// The basis of a `permit`; `None` for `deny`.
    pub fn basis(&self) -> Option<&Basis> {
        match &self.outcome {
            Outcome::Permit(b) => Some(b),
            Outcome::Deny => None,
        }
    }

    /// Whether this decision permits a question of kind `kind`. False for a decision of any
    /// other kind, whatever its outcome: a decision of one kind is never the outcome of a
    /// decision of another ([IFC-TYP-082]). A `deliver` permit does not permit relay
    /// ([SEC-AUZ-020]).
    pub fn permits(&self, kind: Kind) -> bool {
        self.kind == kind && matches!(self.outcome, Outcome::Permit(_))
    }
}

/// An operator's confirmation, required to create a grant ([SEC-AUZ-005]), to enable
/// permission relay ([SEC-AUZ-021]) and to remove a key ([SEC-KEY-035]).
///
/// The core cannot see an operator. This value is the caller's statement that one
/// confirmed the change: only an operator-facing path (the command line) builds it, never
/// an adapter acting on a harness's, a session's or a model's request. A model that asks
/// for access, in content or through a harness, never grants it to itself.
#[derive(Clone, Copy, Debug)]
pub struct OperatorConfirmed(());

impl OperatorConfirmed {
    /// States that an operator confirmed the change at hand.
    pub fn by_operator() -> OperatorConfirmed {
        OperatorConfirmed(())
    }
}

/// A local record the security stage requires: a claim on a session id bound to another
/// key ([SEC-PRS-004]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingFinding {
    /// The session id claimed.
    pub session_id: SessionId,
    /// The key it is bound to.
    pub bound_to: KeyId,
    /// The key that claimed it.
    pub claimed_by: KeyId,
}

/// One decision, as logged: the kind, the outcome and its basis, the principal it concerns
/// and the session ids involved. It has no member that could hold a message body or any
/// other content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionRecord {
    /// The engine's clock at the decision.
    pub at: Timestamp,
    /// The kind.
    pub kind: Kind,
    /// `true` for `permit`.
    pub permitted: bool,
    /// The basis label of a `permit` ([`Basis::label`]).
    pub basis: Option<&'static str>,
    /// The principal the decision concerns, from the trusted key set: the verifying key's
    /// for `deliver`, the signing key's for `accept-presence`, the device's for
    /// `release-presence` and a device requester. `None` when the key is not trusted or the
    /// kind names none.
    pub principal: Option<SecurityPrincipal>,
    /// The key id the decision concerns, when it names one.
    pub key_id: Option<KeyId>,
    /// The sending or requesting session, when there is one.
    pub from: Option<SessionId>,
    /// The addressed, discovered or released session.
    pub session: Option<SessionId>,
}

/// One line of the decision log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogEntry {
    /// A decision.
    Decision(DecisionRecord),
    /// A finding ([SEC-PRS-004]).
    Finding(BindingFinding),
    /// An operator change: a key paired or removed, a grant added or removed, relay
    /// enabled or disabled. The text names key ids and session ids only.
    Change(String),
}

/// Where an [`AuthorizationEngine`] writes its decisions, findings and operator changes.
pub trait DecisionLog: Send {
    /// Writes one entry.
    fn record(&mut self, entry: LogEntry);
}

/// A [`DecisionLog`] in memory. Clones share one list, so a test (or a status command) can
/// keep a clone and read what the engine wrote.
#[derive(Clone, Debug, Default)]
pub struct MemoryDecisionLog {
    entries: Arc<Mutex<Vec<LogEntry>>>,
}

impl MemoryDecisionLog {
    /// An empty log.
    pub fn new() -> MemoryDecisionLog {
        MemoryDecisionLog::default()
    }

    /// Every entry written so far.
    pub fn entries(&self) -> Vec<LogEntry> {
        self.entries.lock().map(|e| e.clone()).unwrap_or_default()
    }
}

impl DecisionLog for MemoryDecisionLog {
    fn record(&mut self, entry: LogEntry) {
        if let Ok(mut e) = self.entries.lock() {
            e.push(entry);
        }
    }
}

/// Why security step 4 refused an envelope: `rejected` with `unauthorized` (Table 7.1), the
/// requirement that failed, for a log line, and the finding [SEC-PRS-004] requires, if any.
/// The one exception is an envelope that passed authorization but found no room to bind
/// its `from` ([`MAX_ENVELOPE_BINDINGS`]): `failed` with `internal-error`, as a full
/// duplicate store refuses one, which only an authorized sender can see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizationRefusal {
    /// `rejected`; `failed` for no room in the binding table.
    pub state: DeliveryState,
    /// `unauthorized`, whatever the reason, so a sender learns nothing else ([SC-RCP-073]);
    /// `internal-error` for no room in the binding table.
    pub error: ErrorCode,
    /// The requirement that failed. Not sent to a peer.
    pub requirement: &'static str,
    /// The finding, when `from` is bound to another key.
    pub finding: Option<BindingFinding>,
}

impl fmt::Display for AuthorizationRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}): {}", self.state, self.error, self.requirement)
    }
}

impl std::error::Error for AuthorizationRefusal {}

/// Why [`AuthorizationEngine::remove_key`] refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoveKeyError {
    /// The key is this device's own; the set always holds it ([SEC-KEY-031]).
    OwnKey,
    /// The key is not in the trusted key set.
    NotTrusted,
    /// The key, its grants and its bindings were removed in memory, but the store did not
    /// save the change. The engine stays without them (fail closed), but the store still
    /// holds them, so a restart before a successful save would restore the revoked device.
    /// The caller must report this to the operator loudly and retry the save with
    /// [`AuthorizationEngine::save`] until it succeeds (the `oac` pairing verb, #71).
    NotSaved(PairingStoreError),
}

impl fmt::Display for RemoveKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemoveKeyError::OwnKey => f.write_str("this device's own key cannot be removed"),
            RemoveKeyError::NotTrusted => f.write_str("the key is not trusted"),
            RemoveKeyError::NotSaved(e) => write!(f, "removed, but not saved: {e}"),
        }
    }
}

impl std::error::Error for RemoveKeyError {}

/// Why [`AuthorizationEngine::pair`] refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PairError {
    /// The key is already trusted.
    AlreadyTrusted(AddKeyError),
    /// The store did not save the pairing; the key was not added.
    NotSaved(PairingStoreError),
}

impl fmt::Display for PairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PairError::AlreadyTrusted(e) => e.fmt(f),
            PairError::NotSaved(e) => write!(f, "pairing not saved: {e}"),
        }
    }
}

impl std::error::Error for PairError {}

/// Why [`AuthorizationEngine::restore`] refused a snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestoreError {
    /// The store could not be read.
    Store(PairingStoreError),
    /// The snapshot pairs a key twice, or pairs this device's own key.
    DuplicateKey(KeyId),
}

impl fmt::Display for RestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RestoreError::Store(e) => e.fmt(f),
            RestoreError::DuplicateKey(k) => write!(f, "key {k} paired twice"),
        }
    }
}

impl std::error::Error for RestoreError {}

/// What a successful [`AuthorizationEngine::remove_key`] removed, in the same step
/// ([SEC-KEY-035]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyRemoval {
    /// The grants that named the key.
    pub grants: Vec<Grant>,
    /// The session ids whose binding or conflict mark named the key.
    pub bindings: Vec<SessionId>,
}

/// A message that passed security step 4: the only input step 5,
/// [`crate::replay::DuplicateStore::admit`], takes. Only
/// [`AuthorizationEngine::authorize_delivery`] builds one, so an envelope refused at step 4
/// can never reserve a duplicate-store entry ([SEC-RPL-022]: "the entry is added only once
/// the copy has passed authorization").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizedMessage {
    message: ChannelMessage,
    decision: AuthorizationDecision,
}

impl AuthorizedMessage {
    /// The verified message.
    pub fn message(&self) -> &ChannelMessage {
        &self.message
    }

    /// The `deliver` decision that permitted it, with its basis.
    pub fn decision(&self) -> &AuthorizationDecision {
        &self.decision
    }

    /// The message, for the delivery stage.
    pub fn into_message(self) -> ChannelMessage {
        self.message
    }

    /// A message taken as authorized without a decision, for unit tests of later steps.
    #[cfg(test)]
    pub(crate) fn for_tests(message: ChannelMessage) -> AuthorizedMessage {
        let key = message.verified_by().map_or_else(
            || KeyId::parse(&"0".repeat(64)).unwrap(),
            |p| p.key_id().clone(),
        );
        AuthorizedMessage {
            message,
            decision: AuthorizationDecision {
                kind: Kind::Deliver,
                outcome: Outcome::Permit(Basis::InboundGrant(Grant::Inbound {
                    writer: PeerSide::device(key),
                    target: LocalSide::Device,
                })),
            },
        }
    }
}

/// The authorization engine: the trusted key set, the grants, the binding table, the
/// own-session facts, the sent and hand-off records and the operator's relay settings,
/// and the decisions of Table 4.9 over them.
///
/// The trusted key set lives here, beside the grants and the binding table, so that
/// removing a key removes every grant and binding that names it in the same step
/// ([SEC-KEY-035]).
pub struct AuthorizationEngine {
    own_key: KeyId,
    trusted: TrustedKeySet,
    paired: BTreeMap<KeyId, PairingRecord>,
    own_sessions: BTreeMap<SessionId, WorkingDirectoryScope>,
    grants: Vec<Grant>,
    bindings: BTreeMap<SessionId, Binding>,
    /// The entries of `bindings` that security step 4 created ([`MAX_ENVELOPE_BINDINGS`]).
    envelope_bound: EnvelopeBindings,
    sent: RecordBook<SessionId, SentRecord>,
    handed_off: RecordBook<HandOffPartition, HandOffRecord>,
    relay: BTreeSet<SessionId>,
    clock: Arc<dyn Clock>,
    log: Box<dyn DecisionLog>,
}

impl fmt::Debug for AuthorizationEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthorizationEngine")
            .field("own_key", &self.own_key)
            .field("trusted", &self.trusted.len())
            .field("own_sessions", &self.own_sessions.len())
            .field("grants", &self.grants.len())
            .field("bindings", &self.bindings.len())
            .finish_non_exhaustive()
    }
}

impl AuthorizationEngine {
    /// An engine for the device `own` with no configuration: only `own`'s key is trusted
    /// ([SEC-KEY-031]), and there is no grant, no session, no record and no relay setting,
    /// so every decision is `deny` ([SEC-AUZ-001]).
    ///
    /// Two harnesses on this device need no pairing: their sessions are bound to this one
    /// device key, which is always trusted (`docs/planning/decisions/C5-envelope-auth.md`
    /// §10(a)). They still need a grant to reach each other ([SEC-AUZ-007]).
    ///
    /// Every decision reads the time from `clock`, the receiver clock the replay checks read
    /// too ([`crate::replay`]).
    pub fn new(
        own: &DeviceIdentity,
        clock: Arc<dyn Clock>,
        log: Box<dyn DecisionLog>,
    ) -> AuthorizationEngine {
        AuthorizationEngine {
            own_key: own.key_id().clone(),
            trusted: TrustedKeySet::new(own),
            paired: BTreeMap::new(),
            own_sessions: BTreeMap::new(),
            grants: Vec::new(),
            bindings: BTreeMap::new(),
            envelope_bound: EnvelopeBindings::new(
                MAX_ENVELOPE_BINDINGS,
                MAX_ENVELOPE_BINDINGS_PER_KEY,
            ),
            sent: RecordBook::new(false),
            handed_off: RecordBook::new(true),
            relay: BTreeSet::new(),
            clock,
            log,
        }
    }

    /// An engine for `own` with the paired keys, grants and relay settings `store` holds.
    /// An empty store gives [`AuthorizationEngine::new`]'s default-deny engine.
    ///
    /// # Errors
    ///
    /// [`RestoreError`] when the store cannot be read or pairs a key twice.
    pub fn restore(
        own: &DeviceIdentity,
        store: &dyn PairingStore,
        clock: Arc<dyn Clock>,
        log: Box<dyn DecisionLog>,
    ) -> Result<AuthorizationEngine, RestoreError> {
        let snapshot = store.load().map_err(RestoreError::Store)?;
        let mut engine = AuthorizationEngine::new(own, clock, log);
        for record in snapshot.paired {
            let key_id = record.key_id();
            engine
                .trusted
                .add_paired_key(record.principal().clone(), *record.public_key())
                .map_err(|_| RestoreError::DuplicateKey(key_id.clone()))?;
            engine.paired.insert(key_id, record);
        }
        for grant in snapshot.grants {
            if !engine.grants.contains(&grant) {
                engine.grants.push(grant);
            }
        }
        engine.relay = snapshot.relay_enabled.into_iter().collect();
        Ok(engine)
    }

    /// The paired keys, grants and relay settings, the state a [`PairingStore`] keeps.
    /// Binding tables and sent and hand-off records live in memory only
    /// (`spec/security.md` §11.3, §9.5).
    pub fn snapshot(&self) -> PairingSnapshot {
        PairingSnapshot {
            paired: self.paired.values().cloned().collect(),
            grants: self.grants.clone(),
            relay_enabled: self.relay.iter().cloned().collect(),
        }
    }

    /// Saves [`AuthorizationEngine::snapshot`] to `store`.
    ///
    /// # Errors
    ///
    /// [`PairingStoreError`] from the store.
    pub fn save(&self, store: &dyn PairingStore) -> Result<(), PairingStoreError> {
        store.save(&self.snapshot())
    }

    /// This device's key id.
    pub fn own_key_id(&self) -> &KeyId {
        &self.own_key
    }

    /// The trusted key set, for security steps 1 and 2 ([`crate::signing::authenticate`]).
    pub fn trusted_keys(&self) -> &TrustedKeySet {
        &self.trusted
    }

    /// The grants, in the order they were added.
    pub fn grants(&self) -> &[Grant] {
        &self.grants
    }

    /// The engine clock's current reading: the instant every decision is made at.
    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    /// The sent records ([SEC-AUZ-013]) still held: by sending session, each in the order
    /// recorded.
    pub fn sent_records(&self) -> impl Iterator<Item = &SentRecord> {
        self.sent.iter()
    }

    /// The sent records of the own session `from`, in the order recorded.
    pub fn sent_records_from(
        &self,
        from: &SessionId,
    ) -> impl Iterator<Item = &SentRecord> + use<'_> {
        self.sent.partition(from)
    }

    /// The hand-off records ([SEC-AUZ-016]) still held: by partition, each in the order
    /// recorded.
    pub fn handoff_records(&self) -> impl Iterator<Item = &HandOffRecord> {
        self.handed_off.iter()
    }

    fn handoff_partition(&self, key: Option<&KeyId>, from: &SessionId) -> HandOffPartition {
        let own = key == Some(&self.own_key);
        (key.cloned(), own.then(|| from.clone()))
    }

    /// The reply headers for a reply from the own session `from` to `to` that answers
    /// `requested_target`, read from the hand-off records this engine keeps
    /// ([`crate::reply::reply_headers`]; [SC-RCP-050] to [SC-RCP-054]). `reply_to` is set
    /// only for a target handed off to `from` from `to`, and `conversation_id` and
    /// `correlation_id` are then copied from that record.
    pub fn reply_headers(
        &self,
        from: &SessionId,
        to: &SessionId,
        requested_target: Option<&str>,
    ) -> ReplyHeaders {
        let records: Vec<EnvelopeRecord> = self
            .handoffs_from(to)
            .filter(|r| &r.to == from && &r.from == to)
            .map(HandOffRecord::envelope_record)
            .collect();
        reply_headers(&records, from, to, requested_target)
    }

    /// The binding-table entry for `session`.
    pub fn binding(&self, session: &SessionId) -> Option<&Binding> {
        self.bindings.get(session)
    }

    /// The binding table, in session-id order.
    pub fn bindings(&self) -> impl Iterator<Item = (&SessionId, &Binding)> {
        self.bindings.iter()
    }

    /// The working-directory scope of an own session.
    pub fn scope_of(&self, session: &SessionId) -> Option<&WorkingDirectoryScope> {
        self.own_sessions.get(session)
    }

    fn change(&mut self, text: String) {
        self.log.record(LogEntry::Change(text));
    }

    // ---- Pairing and keys -------------------------------------------------------------

    /// Adds the key of a pairing an operator confirmed ([SEC-KEY-032]; [`crate::pairing`])
    /// to the trusted key set, and saves it to `store`. Pairing makes the key trusted and
    /// grants nothing: a grant decides what the key may reach ([SEC-AUZ-001]).
    ///
    /// # Errors
    ///
    /// [`PairError`]: the key is already trusted, or the store did not save it, in which
    /// case the key is not added.
    pub fn pair(&mut self, peer: PairedPeer, store: &dyn PairingStore) -> Result<(), PairError> {
        let record = peer.into_record();
        let key_id = record.key_id();
        self.trusted
            .add_paired_key(record.principal().clone(), *record.public_key())
            .map_err(PairError::AlreadyTrusted)?;
        self.paired.insert(key_id.clone(), record);
        if let Err(e) = self.save(store) {
            self.trusted.remove(&key_id);
            self.paired.remove(&key_id);
            return Err(PairError::NotSaved(e));
        }
        self.change(format!("paired key {key_id}"));
        Ok(())
    }

    /// Removes `key_id` from the trusted key set and, in the same step, every grant and
    /// every binding-table entry that names it, conflict marks included ([SEC-KEY-035]),
    /// then saves the change to `store`. A conflict mark that names the key is removed
    /// whole, so the session id becomes unbound and the remaining device's next claim binds
    /// it (`sec-key/SEC-KEY-035.p01`).
    ///
    /// # Errors
    ///
    /// [`RemoveKeyError`]. On [`RemoveKeyError::NotSaved`] the removal stands in memory.
    pub fn remove_key(
        &mut self,
        key_id: &KeyId,
        _: OperatorConfirmed,
        store: &dyn PairingStore,
    ) -> Result<KeyRemoval, RemoveKeyError> {
        if key_id == &self.own_key {
            return Err(RemoveKeyError::OwnKey);
        }
        self.trusted
            .remove(key_id)
            .ok_or(RemoveKeyError::NotTrusted)?;
        self.paired.remove(key_id);
        let (removed, kept): (Vec<Grant>, Vec<Grant>) = std::mem::take(&mut self.grants)
            .into_iter()
            .partition(|g| g.names_key(key_id));
        self.grants = kept;
        let bindings: Vec<SessionId> = self
            .bindings
            .iter()
            .filter(|(_, b)| b.names(key_id))
            .map(|(s, _)| s.clone())
            .collect();
        for s in &bindings {
            self.bindings.remove(s);
            self.envelope_bound.untrack(s);
        }
        // Hand-off records the key verified go with it (#313).
        let removed_key = Some(key_id.clone());
        self.handed_off.drop_partitions(|(k, _)| k == &removed_key);
        self.sync_pins();
        self.change(format!(
            "removed key {key_id}: {} grant(s), {} binding(s)",
            removed.len(),
            bindings.len()
        ));
        self.save(store).map_err(RemoveKeyError::NotSaved)?;
        Ok(KeyRemoval {
            grants: removed,
            bindings,
        })
    }

    // ---- Grants and settings ----------------------------------------------------------

    /// Adds a grant an operator confirmed ([SEC-AUZ-005]) and saves it to `store`. Returns
    /// `false`, and changes nothing, when the engine already holds it.
    ///
    /// # Errors
    ///
    /// [`PairingStoreError`] when the store did not save it; the grant is then not added.
    pub fn add_grant(
        &mut self,
        grant: Grant,
        _: OperatorConfirmed,
        store: &dyn PairingStore,
    ) -> Result<bool, PairingStoreError> {
        if self.grants.contains(&grant) {
            return Ok(false);
        }
        self.grants.push(grant);
        if let Err(e) = self.save(store) {
            self.grants.pop();
            return Err(e);
        }
        self.change(format!("added grant {:?}", self.grants.last()));
        Ok(true)
    }

    /// Removes a grant and saves the change to `store`. Returns `false` when the engine
    /// does not hold it.
    ///
    /// # Errors
    ///
    /// [`PairingStoreError`] when the store did not save the change; the grant stays
    /// removed in memory (fail closed).
    pub fn remove_grant(
        &mut self,
        grant: &Grant,
        store: &dyn PairingStore,
    ) -> Result<bool, PairingStoreError> {
        let before = self.grants.len();
        self.grants.retain(|g| g != grant);
        if self.grants.len() == before {
            return Ok(false);
        }
        self.change(format!("removed grant {grant:?}"));
        self.save(store)?;
        Ok(true)
    }

    /// Enables or disables permission relay for `session` ([SEC-AUZ-021]). Off unless an
    /// operator enables it, as a decision of its own, for that one session.
    ///
    /// # Errors
    ///
    /// [`PairingStoreError`] when the store did not save the change. Enabling is then
    /// undone; disabling stands (fail closed).
    pub fn set_relay(
        &mut self,
        session: SessionId,
        enabled: bool,
        _: OperatorConfirmed,
        store: &dyn PairingStore,
    ) -> Result<(), PairingStoreError> {
        if enabled {
            let added = self.relay.insert(session.clone());
            if let Err(e) = self.save(store) {
                if added {
                    self.relay.remove(&session);
                }
                return Err(e);
            }
        } else {
            self.relay.remove(&session);
            self.save(store)?;
        }
        self.change(format!("relay for {session}: {enabled}"));
        Ok(())
    }

    // ---- Sessions and the binding table -----------------------------------------------

    /// Records an own session from its registration record: its working-directory scope
    /// becomes an own-session fact, and the binding table binds it to this device's key.
    /// Returns `false`, recording nothing, unless the record verifies under this device's
    /// own key ([SEC-KEY-043]).
    pub fn register_session(&mut self, record: &RegistrationRecord, own: &DeviceIdentity) -> bool {
        if own.key_id() != &self.own_key || !record.binding_usable(own) {
            return false;
        }
        let s = record.session_id().clone();
        self.own_sessions.insert(
            s.clone(),
            WorkingDirectoryScope::new(record.working_directory()),
        );
        // An own session is never under conflict ([SEC-PRS-015]): its registration record
        // is authoritative over any earlier claim on the id.
        self.envelope_bound.untrack(&s);
        self.bindings.insert(s, Binding::Key(self.own_key.clone()));
        true
    }

    /// Ends an own session's binding. Reply rights from the session end with it
    /// ([SEC-AUZ-015]), and so does its right to discover a sender it was handed a message
    /// from ([SEC-AUZ-016]). A grant that names the session id stays until an operator
    /// removes it.
    pub fn end_session(&mut self, session: &SessionId) -> bool {
        if self.own_sessions.remove(session).is_none() {
            return false;
        }
        self.bindings.remove(session);
        // Its reply rights ([SEC-AUZ-015]), in its own partition.
        let s = session.clone();
        self.sent.drop_partitions(|k| k == &s);
        // What it was handed, and, as a sender that can no longer be addressed, the
        // partition of what it sent to the sessions still bound (#313, PR #326 re-review).
        self.handed_off.retain(|r| &r.to != session);
        let own = Some(self.own_key.clone());
        self.handed_off
            .drop_partitions(|(k, f)| k == &own && f.as_ref() == Some(&s));
        self.sync_pins();
        true
    }

    /// Binds another implementation's session id `session` to `key`, from an accepted
    /// announcement ([SEC-PRS-005]). Only an unbound session id is bound; an envelope binds
    /// through [`AuthorizationEngine::authorize_delivery`]. An entry an envelope created
    /// for the same key is confirmed: from then on the presence registry bounds it, not
    /// [`MAX_ENVELOPE_BINDINGS`].
    pub fn bind(&mut self, session: &SessionId, key: &KeyId) -> BindOutcome {
        match self.bindings.get(session) {
            None => {
                self.bindings
                    .insert(session.clone(), Binding::Key(key.clone()));
                BindOutcome::Bound
            }
            Some(Binding::Key(k)) if k == key => {
                self.envelope_bound.untrack(session);
                BindOutcome::AlreadyBound
            }
            Some(Binding::Key(k)) => BindOutcome::BoundToOther(k.clone()),
            Some(Binding::Conflict(_)) => BindOutcome::UnderConflict,
        }
    }

    /// Marks another implementation's session id as under conflict between its current key
    /// and `claimant` ([SEC-PRS-014]), or adds `claimant` to an existing mark. The caller
    /// has checked that the claimant passes the relation test of [SEC-AUZ-017]
    /// ([`Kind::AcceptPresence`]). Refused, returning `false`, for an own session
    /// ([SEC-PRS-015]) and for a session id with no entry.
    pub fn mark_conflict(&mut self, session: &SessionId, claimant: &KeyId) -> bool {
        if self.own_sessions.contains_key(session) {
            return false;
        }
        let Some(entry) = self.bindings.get_mut(session) else {
            return false;
        };
        let keys = match entry {
            Binding::Key(k) => BTreeSet::from([k.clone(), claimant.clone()]),
            Binding::Conflict(keys) => {
                let mut keys = std::mem::take(keys);
                keys.insert(claimant.clone());
                keys
            }
        };
        *entry = Binding::Conflict(keys);
        self.envelope_bound.untrack(session);
        true
    }

    /// Removes a binding-table entry for a forgotten session of another implementation
    /// ([SEC-PRS-009]). A conflict mark is never removed this way, nor an own session's
    /// binding.
    pub fn forget_binding(&mut self, session: &SessionId) -> bool {
        if self.own_sessions.contains_key(session)
            || matches!(
                self.bindings.get(session),
                Some(Binding::Conflict(_)) | None
            )
        {
            return false;
        }
        self.envelope_bound.untrack(session);
        self.bindings.remove(session).is_some()
    }

    /// The number of binding-table entries that security step 4 created and nothing else
    /// refers to yet ([`MAX_ENVELOPE_BINDINGS`]).
    pub fn envelope_bindings(&self) -> usize {
        self.envelope_bound.sessions.len()
    }

    /// The same engine with the bound on envelope-created binding-table entries set to
    /// `capacity` in all and `per_key` for each key (each at least one, the share at most
    /// the capacity), in place of [`MAX_ENVELOPE_BINDINGS`] and
    /// [`MAX_ENVELOPE_BINDINGS_PER_KEY`]. Call it on a new engine, before any envelope is
    /// authorized: entries made before it are not counted. A share at or below
    /// [`MAX_RECORDS_PER_PARTITION`] can leave a key whose every entry a hand-off record is
    /// looked up through; its next new `from` is then refused, fail-closed.
    pub fn with_envelope_binding_limits(
        mut self,
        capacity: usize,
        per_key: usize,
    ) -> AuthorizationEngine {
        self.envelope_bound = EnvelopeBindings::new(capacity, per_key);
        self
    }

    /// Updates which envelope-created entries a hand-off record is looked up through, after
    /// any change to the hand-off records.
    fn sync_pins(&mut self) {
        for ((key, _), sender, held) in self.handed_off.take_sender_changes() {
            if let Some(k) = key {
                self.envelope_bound.set_pinned(&sender, &k, held);
            }
        }
    }

    /// Binds `from` to `key` for an envelope that passed step 4 ([SEC-PRS-005]), within
    /// [`MAX_ENVELOPE_BINDINGS`]; false, binding nothing, when there is no room.
    fn bind_from_envelope(&mut self, from: &SessionId, key: &KeyId) -> bool {
        if !self.envelope_bound.has_room(key) {
            let Some(victim) = self.envelope_bound.victim(key) else {
                return false;
            };
            self.envelope_bound.untrack(&victim);
            self.bindings.remove(&victim);
        }
        let pinned = self
            .handed_off
            .holds_sender(&self.handoff_partition(Some(key), from), from);
        self.bindings
            .insert(from.clone(), Binding::Key(key.clone()));
        self.envelope_bound.track(from.clone(), key.clone(), pinned);
        true
    }

    // ---- Records ----------------------------------------------------------------------

    /// Records a reply right for an envelope an own session passed to a transport
    /// ([SEC-AUZ-013]); build the record with [`SentRecord::of`].
    ///
    /// Kept in the sending session's partition ([`MAX_RECORDS_PER_PARTITION`]): a full
    /// partition forgets its own expired records, then its oldest, and never another
    /// session's. Forgetting one fails closed: its reply right ends, and a receipt for its
    /// envelope is discarded ([SEC-RCT-003] check 3).
    pub fn record_sent(&mut self, record: SentRecord) {
        let now = self.clock.now();
        let key = record.from.clone();
        self.sent.push(key, record, &now);
    }

    /// Records a hand-off to an own session, when its outcome was `handed-to-harness` or
    /// `unknown` ([SEC-AUZ-016]); any other outcome records nothing. Build the record with
    /// [`HandOffRecord::of`]. Kept in its writer's partition ([`MAX_RECORDS_PER_PARTITION`]),
    /// bounded as [`AuthorizationEngine::record_sent`] is: a forgotten record ends that
    /// sender's discovery right and leaves a reply to that envelope uncorrelated
    /// ([SC-RCP-050]), both fail-closed, and no writer can make another's records go.
    ///
    /// The record is of an envelope that passed security step 4. Two further rules:
    ///
    /// - **A late copy from an ended own session records nothing (#328).** An envelope
    ///   under this device's own key whose `from` is not an own session now came from a
    ///   session that has ended. Its partition went with it
    ///   ([`AuthorizationEngine::end_session`]), and a record could serve nothing, since the
    ///   session can no longer be addressed, so the partition is not made again. This is
    ///   consistent with [SEC-AUZ-016]: the right it grants is the receiving session's, to
    ///   discover the sender, and a sender that can no longer be addressed has nothing to
    ///   discover; `end_session` already drops the same partition (PR #326 re-review).
    /// - **An evicted `from` is bound again (#325).** When the envelope-created binding of
    ///   `from` was evicted between step 4 and this call ([`MAX_ENVELOPE_BINDINGS`]), or
    ///   forgotten with its presence record, `from` is bound again to the verifying key, if
    ///   it is still trusted, so that the record can be looked up ([SEC-AUZ-016],
    ///   [SC-RCP-053]). [SEC-PRS-005] permits it: the envelope passed step 4 with that
    ///   `from`. The entry is kept while a record from `from` is, and room is made for it
    ///   as for any new entry. When there is none, because every entry that could go is in
    ///   use, it is kept above the bound. Such entries number at most the hand-offs that
    ///   were in flight when their bindings went.
    pub fn record_handoff(&mut self, record: HandOffRecord, outcome: DeliveryState) {
        if !matches!(
            outcome,
            DeliveryState::HandedToHarness | DeliveryState::Unknown
        ) {
            return;
        }
        let own = record.key_id.as_ref() == Some(&self.own_key);
        if own && !self.own_sessions.contains_key(&record.from) {
            return;
        }
        let rebind = match (own, record.key_id.clone(), self.bindings.get(&record.from)) {
            (false, Some(k), None) if self.trusted.get(&k).is_some() => Some(k),
            _ => None,
        };
        let from = record.from.clone();
        let now = self.clock.now();
        let key = self.handoff_partition(record.key_id.as_ref(), &record.from);
        self.handed_off.push(key.clone(), record, &now);
        self.sync_pins();
        if let Some(k) = rebind {
            // Within the bound when an entry can go; otherwise above it, as the
            // documentation above says.
            if !self.envelope_bound.has_room(&k)
                && let Some(victim) = self.envelope_bound.victim(&k)
            {
                self.envelope_bound.untrack(&victim);
                self.bindings.remove(&victim);
            }
            // Pinned by what the partition holds now, this record and any earlier one from
            // `from`: no change is emitted for a sender that already had a record (PR #334
            // review B1).
            let pinned = self.handed_off.holds_sender(&key, &from);
            self.bindings.insert(from.clone(), Binding::Key(k.clone()));
            self.envelope_bound.track(from, k, pinned);
        }
    }

    /// Removes a hand-off record, the one [`AuthorizationEngine::record_handoff`] kept for
    /// `record`'s envelope. A receive pipeline records a hand-off before its hand-off call,
    /// so a reply made during the call correlates, and removes it when the call's outcome
    /// is neither `handed-to-harness` nor `unknown` (#313).
    pub fn forget_handoff(&mut self, record: &HandOffRecord) -> bool {
        let key = self.handoff_partition(record.key_id.as_ref(), &record.from);
        let removed = self.handed_off.remove_first(&key, |r| {
            r.id == record.id && r.from == record.from && r.to == record.to
        });
        self.sync_pins();
        removed
    }

    /// The hand-off records of envelopes from `from`: its partition when a binding names
    /// its key, and the partition of records kept with no key. An unbound sender's records
    /// are not looked for: no scan of every partition runs under a decision. A sender is
    /// unbound only once forgotten, removed from trust, or ended (own sessions), and its
    /// records then serve nothing a send to it could use; it is bound again, to the same
    /// partition, by its next announcement or envelope.
    fn handoffs_from<'a>(
        &'a self,
        from: &'a SessionId,
    ) -> Box<dyn Iterator<Item = &'a HandOffRecord> + 'a> {
        match self.bindings.get(from) {
            Some(Binding::Key(k)) => {
                let key = self.handoff_partition(Some(k), from);
                Box::new(self.handed_off.partition(&key).chain(
                    // Records kept before the binding, under no key.
                    self.handed_off.partition(&(None, None)),
                ))
            }
            _ => Box::new(self.handed_off.partition(&(None, None))),
        }
    }

    /// Forgets sent and hand-off records whose reply period has ended on the engine's
    /// clock. They no longer count after that instant anyway. A full pass over both lists,
    /// bounded by [`MAX_RECORDS_TOTAL`] each: the receive and send pipelines run it when a
    /// session ends ([`crate::pipeline::Pipelines::unbind`]), never per record; between
    /// passes each partition drops its own expired records as it grows, and whole expired
    /// partitions go at the partition cap.
    pub fn prune(&mut self) {
        let now = self.clock.now();
        self.prune_at(&now);
    }

    fn prune_at(&mut self, now: &Timestamp) {
        self.sent
            .retain(|r| within_reply_period(&r.created_at, now));
        self.handed_off
            .retain(|r| within_reply_period(&r.created_at, now));
        self.sync_pins();
    }

    // ---- Decisions --------------------------------------------------------------------

    /// Answers `request` at the engine clock's time and logs the decision. Default deny: the
    /// decision is `permit` only with a basis among the recorded facts its kind may read.
    pub fn decide(&mut self, request: &AuthorizationRequest) -> AuthorizationDecision {
        let now = self.clock.now();
        self.decide_at(request, &now)
    }

    fn decide_at(
        &mut self,
        request: &AuthorizationRequest,
        now: &Timestamp,
    ) -> AuthorizationDecision {
        let decision = AuthorizationDecision {
            kind: request.kind(),
            outcome: self.evaluate(request, now),
        };
        let record = self.record_of(request, &decision, now);
        self.log.record(LogEntry::Decision(record));
        decision
    }

    fn record_of(
        &self,
        request: &AuthorizationRequest,
        decision: &AuthorizationDecision,
        now: &Timestamp,
    ) -> DecisionRecord {
        let (key, from, session) = match request {
            AuthorizationRequest::Deliver(q) => {
                (Some(&q.verifying_key), Some(&q.from), Some(&q.to))
            }
            AuthorizationRequest::Discover { requester, session } => match requester {
                Requester::Session(s) => (None, Some(s), Some(session)),
                Requester::Device(k) => (Some(k), None, Some(session)),
            },
            AuthorizationRequest::ReleasePresence { session, device } => {
                (Some(device), None, Some(session))
            }
            AuthorizationRequest::AcceptPresence {
                signing_key,
                session,
            } => (Some(signing_key), None, Some(session)),
            AuthorizationRequest::RelayPermission { session } => (None, None, Some(session)),
        };
        DecisionRecord {
            at: now.clone(),
            kind: decision.kind,
            permitted: matches!(decision.outcome, Outcome::Permit(_)),
            basis: decision.basis().map(Basis::label),
            principal: key
                .and_then(|k| self.trusted.get(k))
                .map(|e| e.security_principal()),
            key_id: key.cloned(),
            from: from.cloned(),
            session: session.cloned(),
        }
    }

    fn evaluate(&self, request: &AuthorizationRequest, now: &Timestamp) -> Outcome {
        let found = match request {
            AuthorizationRequest::Deliver(q) => self.deliver(q, now),
            AuthorizationRequest::Discover { requester, session } => match requester {
                Requester::Session(l) => self.discover(l, session, now),
                Requester::Device(k) => self.release(session, k),
            },
            AuthorizationRequest::ReleasePresence { session, device } => {
                self.release(session, device)
            }
            AuthorizationRequest::AcceptPresence {
                signing_key,
                session,
            } => self.related(signing_key, session, now),
            AuthorizationRequest::RelayPermission { session } => {
                (self.own_sessions.contains_key(session) && self.relay.contains(session))
                    .then(|| Basis::RelaySetting(session.clone()))
            }
        };
        found.map_or(Outcome::Deny, Outcome::Permit)
    }

    /// Kind `deliver` ([SEC-AUZ-002], [SEC-AUZ-003], [SEC-AUZ-014], [SEC-AUZ-015]).
    fn deliver(&self, q: &DeliverQuestion, now: &Timestamp) -> Option<Basis> {
        match self.bindings.get(&q.from) {
            Some(Binding::Key(k)) if k != &q.verifying_key => return None,
            Some(Binding::Conflict(_)) => return None,
            _ => {}
        }
        let own = &self.own_sessions;
        let grant = self.grants.iter().find(|g| match g {
            Grant::Inbound { writer, target } => {
                writer.names(&q.verifying_key, &q.from) && target.includes(&q.to, own)
            }
            Grant::Outbound { .. } => false,
        });
        if let Some(g) = grant {
            return Some(Basis::InboundGrant(g.clone()));
        }
        let reply_to = q.reply_to.as_ref()?;
        self.sent
            .partition(&q.to)
            .find(|e| {
                e.to_key_id == q.verifying_key
                    && e.to == q.from
                    && e.from == q.to
                    && &e.id == reply_to
                    && within_reply_period(&e.created_at, now)
                    && own.contains_key(&e.from)
            })
            .map(|e| Basis::ReplyRight(e.clone()))
    }

    /// Kind `discover` for an own session `l` ([SEC-AUZ-010], [SEC-AUZ-012],
    /// [SEC-AUZ-016]): a session sees the sessions it may write to, and a sender it was
    /// handed a message from, for the reply period.
    fn discover(&self, l: &SessionId, s: &SessionId, now: &Timestamp) -> Option<Basis> {
        let own = &self.own_sessions;
        if !own.contains_key(l) {
            return None;
        }
        let by_grant = if own.contains_key(s) {
            self.grants.iter().find(|g| match g {
                Grant::Inbound { writer, target } => {
                    writer.names(&self.own_key, l) && target.includes(s, own)
                }
                Grant::Outbound { .. } => false,
            })
        } else if let Some(Binding::Key(k)) = self.bindings.get(s) {
            self.grants.iter().find(|g| match g {
                Grant::Outbound { writer, target } => writer.includes(l, own) && target.names(k, s),
                Grant::Inbound { .. } => false,
            })
        } else {
            None
        };
        if let Some(g) = by_grant {
            return Some(match g {
                Grant::Inbound { .. } => Basis::InboundGrant(g.clone()),
                Grant::Outbound { .. } => Basis::OutboundGrant(g.clone()),
            });
        }
        self.handoffs_from(s)
            .find(|h| &h.to == l && &h.from == s && within_reply_period(&h.created_at, now))
            .map(|h| Basis::HandOff(h.clone()))
    }

    /// Kinds `release-presence`, and `discover` with a device requester ([SEC-AUZ-011]):
    /// an inbound grant lets `k`, or a session of `k`, write to `s`, or an outbound grant
    /// lets `s` write to `k` or a session of `k`.
    fn release(&self, s: &SessionId, k: &KeyId) -> Option<Basis> {
        let own = &self.own_sessions;
        if !own.contains_key(s) {
            return None;
        }
        self.grants.iter().find_map(|g| match g {
            Grant::Inbound { writer, target } if &writer.key_id == k && target.includes(s, own) => {
                Some(Basis::InboundGrant(g.clone()))
            }
            Grant::Outbound { writer, target }
                if &target.key_id == k && writer.includes(s, own) =>
            {
                Some(Basis::OutboundGrant(g.clone()))
            }
            _ => None,
        })
    }

    /// Kind `accept-presence`, the relation test of [SEC-AUZ-017]: a grant names `k` (with
    /// `r`, or with no session) as an inbound grant's writer or an outbound grant's target,
    /// or an own session sent an envelope to `r` under `k`, or was handed one from `r`,
    /// within the reply period.
    fn related(&self, k: &KeyId, r: &SessionId, now: &Timestamp) -> Option<Basis> {
        let by_grant = self.grants.iter().find_map(|g| match g {
            Grant::Inbound { writer, .. } if writer.names(k, r) => {
                Some(Basis::InboundGrant(g.clone()))
            }
            Grant::Outbound { target, .. } if target.names(k, r) => {
                Some(Basis::OutboundGrant(g.clone()))
            }
            _ => None,
        });
        by_grant
            .or_else(|| {
                self.sent
                    .iter()
                    .find(|e| {
                        &e.to == r && &e.to_key_id == k && within_reply_period(&e.created_at, now)
                    })
                    .map(|e| Basis::Sent(e.clone()))
            })
            .or_else(|| {
                self.handoffs_from(r)
                    .find(|h| &h.from == r && within_reply_period(&h.created_at, now))
                    .map(|h| Basis::HandOff(h.clone()))
            })
    }

    /// Security step 4 of Table 7.1, authorization, for an envelope that passed steps 1 to
    /// 3. It fails when `from` is bound to another key or is under conflict
    /// ([SEC-AUZ-003]), or when neither an inbound grant nor a live reply right covers the
    /// envelope ([SEC-AUZ-001], [SEC-AUZ-002]). Each failure is `rejected` with
    /// `unauthorized`, whether `to` is unknown, unavailable or simply not granted
    /// (`spec/security.md` §7.1).
    ///
    /// When the envelope passes and `from` has no binding yet, `from` is bound to the
    /// verifying key ([SEC-PRS-005]), within [`MAX_ENVELOPE_BINDINGS`] (#325): when that
    /// bound leaves no entry to evict, the envelope is refused with `failed` and
    /// `internal-error`, and nothing is bound. When it fails, nothing is bound. A claim on a
    /// session id bound to another key returns a finding, also written to the log
    /// ([SEC-PRS-004]). An envelope never sets a conflict mark.
    ///
    /// # Errors
    ///
    /// [`AuthorizationRefusal`].
    pub fn authorize_delivery(
        &mut self,
        msg: ChannelMessage,
    ) -> Result<AuthorizedMessage, AuthorizationRefusal> {
        let now = self.clock.now();
        self.authorize_delivery_at(msg, &now)
    }

    fn authorize_delivery_at(
        &mut self,
        msg: ChannelMessage,
        now: &Timestamp,
    ) -> Result<AuthorizedMessage, AuthorizationRefusal> {
        let refuse = |requirement, finding| AuthorizationRefusal {
            state: DeliveryState::Rejected,
            error: ErrorCode::Unauthorized,
            requirement,
            finding,
        };
        let Some(request) = AuthorizationRequest::deliver(&msg) else {
            return Err(refuse("SEC-STG-003", None));
        };
        let AuthorizationRequest::Deliver(q) = &request else {
            unreachable!("AuthorizationRequest::deliver builds a Deliver request")
        };
        let (from, key) = (q.from.clone(), q.verifying_key.clone());
        let decision = self.decide_at(&request, now);
        match self.bindings.get(&from) {
            Some(Binding::Key(bound)) if bound != &key => {
                let finding = BindingFinding {
                    session_id: from,
                    bound_to: bound.clone(),
                    claimed_by: key,
                };
                self.log.record(LogEntry::Finding(finding.clone()));
                return Err(refuse("SEC-AUZ-003", Some(finding)));
            }
            Some(Binding::Conflict(_)) => return Err(refuse("SEC-PRS-012", None)),
            _ => {}
        }
        if !decision.permits(Kind::Deliver) {
            return Err(refuse("SEC-AUZ-001", None));
        }
        // [SEC-PRS-005], within [`MAX_ENVELOPE_BINDINGS`] (#325).
        if !self.bindings.contains_key(&from) && !self.bind_from_envelope(&from, &key) {
            return Err(AuthorizationRefusal {
                state: DeliveryState::Failed,
                error: ErrorCode::InternalError,
                requirement: "SEC-PRS-005",
                finding: None,
            });
        }
        Ok(AuthorizedMessage {
            message: msg,
            decision,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{EnvelopeDraft, EnvelopeLimits, TextPart, receive_envelope};
    use crate::keys::DeviceKey;
    use crate::pairing::MemoryPairingStore;
    use crate::signing::authenticate;

    fn sid(s: &str) -> SessionId {
        SessionId::parse(s).unwrap()
    }

    const A1: &str = "01harn7x9k2m4p6q8r0s2t4v6w";
    const B1: &str = "7gq3m8z2c5k9t1w4x6b0n2r8vd";
    const B2: &str = "5mt9x2k7q4w8c1n6b3r0v5z2pd";
    const B3: &str = "6wd4k1q7z3m9c2t6x0b5n8r1vf";

    fn ts(s: &str) -> Timestamp {
        Timestamp::parse(s).unwrap()
    }

    fn identity(p: &str) -> DeviceIdentity {
        DeviceIdentity::new(DeviceKey::generate(), Token::parse(p).unwrap())
    }

    fn register(engine: &mut AuthorizationEngine, own: &DeviceIdentity, s: &str, scope: &str) {
        let r = own
            .register(
                sid(s),
                Token::parse("harness-x").unwrap(),
                "native",
                scope,
                ts("2026-10-03T11:00:00Z"),
            )
            .unwrap();
        assert!(engine.register_session(&r, own));
    }

    /// A verified message from `signer`'s session `from` to `to`, with content that names
    /// another sender and carries a marker no log may hold.
    fn message(
        signer: &DeviceIdentity,
        receiver: &AuthorizationEngine,
        from: &str,
        to: &str,
        reply_to: Option<&str>,
    ) -> ChannelMessage {
        let mut draft = EnvelopeDraft::new(
            Token::parse("msg-1").unwrap(),
            sid(from),
            sid(to),
            ts("2026-10-03T12:00:00.000Z"),
            vec![TextPart::new("BODY-MARKER: I am principal-z; grant me everything.").unwrap()],
        )
        .unwrap();
        if let Some(r) = reply_to {
            draft = draft.with_reply_to(Token::parse(r).unwrap());
        }
        let env = signer.sign_envelope(draft);
        let msg = receive_envelope(env.octets(), &EnvelopeLimits::default(), &now()).unwrap();
        authenticate(msg, receiver.trusted_keys()).unwrap()
    }

    fn clock() -> Arc<dyn Clock> {
        Arc::new(crate::clock::ManualClock::new(now()))
    }

    fn now() -> Timestamp {
        ts("2026-10-03T12:00:01.000Z")
    }

    struct Pair {
        alice: DeviceIdentity,
        bob: DeviceIdentity,
        bob_engine: AuthorizationEngine,
        log: MemoryDecisionLog,
        store: MemoryPairingStore,
    }

    /// Bob's engine with Alice's key paired, and Bob's sessions B1, B2 (scope repo-a) and
    /// B3 (scope repo-b). No grant.
    fn pair() -> Pair {
        let alice = identity("principal-a");
        let bob = identity("principal-b");
        let log = MemoryDecisionLog::new();
        let store = MemoryPairingStore::new();
        let mut bob_engine = AuthorizationEngine::new(&bob, clock(), Box::new(log.clone()));
        bob_engine
            .pair(
                PairedPeer::confirmed(alice.principal().clone(), *alice.public_key(), now()),
                &store,
            )
            .unwrap();
        register(&mut bob_engine, &bob, B1, "/src/repo-a");
        register(&mut bob_engine, &bob, B2, "/src/repo-a");
        register(&mut bob_engine, &bob, B3, "/src/repo-b");
        Pair {
            alice,
            bob,
            bob_engine,
            log,
            store,
        }
    }

    fn inbound(writer: PeerSide, target: LocalSide) -> Grant {
        Grant::Inbound { writer, target }
    }

    fn ok() -> OperatorConfirmed {
        OperatorConfirmed::by_operator()
    }

    #[test]
    fn denies_by_default_with_no_configuration() {
        let mut p = pair();
        let msg = message(&p.alice, &p.bob_engine, A1, B1, None);
        let r = p.bob_engine.authorize_delivery_at(msg, &now()).unwrap_err();
        assert_eq!(
            (r.state, r.error, r.requirement),
            (
                DeliveryState::Rejected,
                ErrorCode::Unauthorized,
                "SEC-AUZ-001"
            )
        );
        // No binding was created by the refused envelope ([SEC-PRS-005]).
        assert!(p.bob_engine.binding(&sid(A1)).is_none());
        // Every kind is deny with nothing recorded.
        let mut fresh =
            AuthorizationEngine::new(&p.bob, clock(), Box::new(MemoryDecisionLog::new()));
        register(&mut fresh, &p.bob, B1, "/src/repo-a");
        register(&mut fresh, &p.bob, B2, "/src/repo-a");
        for req in [
            AuthorizationRequest::Discover {
                requester: Requester::Session(sid(B1)),
                session: sid(B2),
            },
            AuthorizationRequest::Discover {
                requester: Requester::Device(p.alice.key_id().clone()),
                session: sid(B1),
            },
            AuthorizationRequest::ReleasePresence {
                session: sid(B1),
                device: p.alice.key_id().clone(),
            },
            AuthorizationRequest::AcceptPresence {
                signing_key: p.alice.key_id().clone(),
                session: sid(A1),
            },
            AuthorizationRequest::RelayPermission { session: sid(B1) },
        ] {
            let d = fresh.decide_at(&req, &now());
            assert_eq!(d.outcome(), &Outcome::Deny, "{req:?}");
            assert!(d.basis().is_none());
        }
    }

    #[test]
    fn same_device_sessions_need_no_pairing_but_need_a_grant() {
        let me = identity("principal-me");
        let store = MemoryPairingStore::new();
        let mut e = AuthorizationEngine::new(&me, clock(), Box::new(MemoryDecisionLog::new()));
        register(&mut e, &me, B1, "/src/repo-a");
        register(&mut e, &me, B2, "/src/repo-a");
        // Steps 1 and 2 pass with no pairing: the device key is trusted ([SEC-KEY-031]).
        let msg = message(&me, &e, B1, B2, None);
        // Same device, same working directory: still no implicit grant ([SEC-AUZ-007]).
        assert!(e.authorize_delivery_at(msg.clone(), &now()).is_err());
        assert!(
            !e.decide_at(
                &AuthorizationRequest::Discover {
                    requester: Requester::Session(sid(B1)),
                    session: sid(B2)
                },
                &now()
            )
            .permits(Kind::Discover)
        );
        let g = inbound(
            PeerSide::device(me.key_id().clone()),
            LocalSide::Scope(WorkingDirectoryScope::new("/src/repo-a")),
        );
        assert!(e.add_grant(g.clone(), ok(), &store).unwrap());
        let d = e
            .authorize_delivery_at(msg, &now())
            .unwrap()
            .decision()
            .clone();
        assert_eq!(d.basis(), Some(&Basis::InboundGrant(g)));
        assert!(
            e.decide_at(
                &AuthorizationRequest::Discover {
                    requester: Requester::Session(sid(B1)),
                    session: sid(B2)
                },
                &now()
            )
            .permits(Kind::Discover)
        );
    }

    #[test]
    fn working_directory_scope_is_exact_and_does_not_leak_across_projects() {
        let mut p = pair();
        let g = inbound(
            PeerSide::device(p.alice.key_id().clone()),
            LocalSide::Scope(WorkingDirectoryScope::new("/src/repo-a")),
        );
        p.bob_engine.add_grant(g, ok(), &p.store).unwrap();
        // B3 is in repo-b: not deliverable, not released, not discoverable by Alice.
        let msg = message(&p.alice, &p.bob_engine, A1, B3, None);
        assert!(p.bob_engine.authorize_delivery_at(msg, &now()).is_err());
        let release = |e: &mut AuthorizationEngine, s: &str, k: &KeyId| {
            e.decide_at(
                &AuthorizationRequest::ReleasePresence {
                    session: sid(s),
                    device: k.clone(),
                },
                &now(),
            )
            .permits(Kind::ReleasePresence)
        };
        let ak = p.alice.key_id().clone();
        assert!(release(&mut p.bob_engine, B1, &ak));
        assert!(!release(&mut p.bob_engine, B3, &ak));
        // A subdirectory is another scope ([SEC-AUZ-008]).
        let bob = p.bob;
        register(
            &mut p.bob_engine,
            &bob,
            "2zq7c4m1x8t5k3w9b6n0r2v7pd",
            "/src/repo-a/sub",
        );
        assert!(!release(
            &mut p.bob_engine,
            "2zq7c4m1x8t5k3w9b6n0r2v7pd",
            &ak
        ));
        // A trusted device the grant does not name sees nothing.
        let carol = identity("principal-c");
        assert!(!release(&mut p.bob_engine, B1, carol.key_id()));
    }

    #[test]
    fn decisions_are_logged_with_the_principal_and_never_the_body() {
        let mut p = pair();
        let g = inbound(
            PeerSide::session(p.alice.key_id().clone(), sid(A1)),
            LocalSide::Session(sid(B1)),
        );
        p.bob_engine.add_grant(g, ok(), &p.store).unwrap();
        let msg = message(&p.alice, &p.bob_engine, A1, B1, None);
        p.bob_engine.authorize_delivery_at(msg, &now()).unwrap();
        let entries = p.log.entries();
        let LogEntry::Decision(d) = entries
            .iter()
            .rev()
            .find(|e| matches!(e, LogEntry::Decision(_)))
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(d.kind, Kind::Deliver);
        assert!(d.permitted);
        assert_eq!(d.basis, Some("inbound-grant"));
        assert_eq!(
            d.principal,
            Some(p.alice.security_principal()),
            "the decision names the verified principal"
        );
        assert_eq!(d.from, Some(sid(A1)));
        assert_eq!(d.session, Some(sid(B1)));
        let text = format!("{entries:?}");
        assert!(!text.contains("BODY-MARKER"), "a log entry holds the body");
        assert!(!text.contains("principal-z"), "a log entry holds content");
        assert!(
            !text.contains("/src/repo-a"),
            "a log entry holds a working directory"
        );
    }

    #[test]
    fn a_decision_of_one_kind_is_not_a_decision_of_another() {
        let mut p = pair();
        let g = inbound(
            PeerSide::device(p.alice.key_id().clone()),
            LocalSide::Device,
        );
        p.bob_engine.add_grant(g, ok(), &p.store).unwrap();
        let msg = message(&p.alice, &p.bob_engine, A1, B1, None);
        let d = p
            .bob_engine
            .authorize_delivery_at(msg, &now())
            .unwrap()
            .decision()
            .clone();
        assert!(d.permits(Kind::Deliver));
        for other in [
            Kind::Discover,
            Kind::ReleasePresence,
            Kind::AcceptPresence,
            Kind::RelayPermission,
        ] {
            assert!(!d.permits(other), "a deliver permit read as {other}");
        }
        // A delivery grant never enables relay ([SEC-AUZ-020], [SEC-AUZ-021]).
        let relay = p.bob_engine.decide_at(
            &AuthorizationRequest::RelayPermission { session: sid(B1) },
            &now(),
        );
        assert_eq!(relay.outcome(), &Outcome::Deny);
        p.bob_engine
            .set_relay(sid(B1), true, ok(), &p.store)
            .unwrap();
        let relay = p.bob_engine.decide_at(
            &AuthorizationRequest::RelayPermission { session: sid(B1) },
            &now(),
        );
        assert_eq!(relay.basis(), Some(&Basis::RelaySetting(sid(B1))));
        assert!(!relay.permits(Kind::Deliver));
        // Relay for B1 does not extend to B2.
        assert!(
            !p.bob_engine
                .decide_at(
                    &AuthorizationRequest::RelayPermission { session: sid(B2) },
                    &now()
                )
                .permits(Kind::RelayPermission)
        );
    }

    #[test]
    fn an_unverified_message_has_no_deliver_request() {
        let p = pair();
        let env = p.alice.sign_envelope(
            EnvelopeDraft::new(
                Token::parse("msg-1").unwrap(),
                sid(A1),
                sid(B1),
                ts("2026-10-03T12:00:00.000Z"),
                vec![TextPart::new("hi").unwrap()],
            )
            .unwrap(),
        );
        let msg = receive_envelope(env.octets(), &EnvelopeLimits::default(), &now()).unwrap();
        assert!(AuthorizationRequest::deliver(&msg).is_none());
        let mut e = p.bob_engine;
        let r = e.authorize_delivery_at(msg, &now()).unwrap_err();
        assert_eq!(r.requirement, "SEC-STG-003");
    }

    #[test]
    fn reply_right_covers_replies_to_one_message_for_24_hours() {
        let mut p = pair();
        // B1 sends msg-b to A1 (bound to Alice's key).
        let to_alice = p.bob.sign_envelope(
            EnvelopeDraft::new(
                Token::parse("msg-b").unwrap(),
                sid(B1),
                sid(A1),
                ts("2026-10-03T11:30:00.000Z"),
                vec![TextPart::new("q").unwrap()],
            )
            .unwrap(),
        );
        p.bob_engine
            .record_sent(SentRecord::of(&to_alice, p.alice.key_id().clone()));
        let reply = message(&p.alice, &p.bob_engine, A1, B1, Some("msg-b"));
        let d = p
            .bob_engine
            .authorize_delivery_at(reply.clone(), &now())
            .unwrap()
            .decision()
            .clone();
        assert!(matches!(d.basis(), Some(Basis::ReplyRight(_))));
        // Uncorrelated, or to another session: refused.
        let other = message(&p.alice, &p.bob_engine, A1, B2, Some("msg-b"));
        assert!(p.bob_engine.authorize_delivery_at(other, &now()).is_err());
        let plain = message(&p.alice, &p.bob_engine, A1, B1, None);
        assert!(p.bob_engine.authorize_delivery_at(plain, &now()).is_err());
        // Exactly 24 hours after msg-b's created_at the right has ended ([SEC-AUZ-015]).
        let late = ts("2026-10-04T11:30:00.000Z");
        assert!(
            p.bob_engine
                .authorize_delivery_at(reply.clone(), &late)
                .is_err()
        );
        assert!(
            p.bob_engine
                .authorize_delivery_at(reply.clone(), &ts("2026-10-04T11:29:59.999Z"))
                .is_ok()
        );
        // It also ends with the sending session's binding.
        p.bob_engine.end_session(&sid(B1));
        assert!(p.bob_engine.authorize_delivery_at(reply, &now()).is_err());
    }

    /// The reply right checks the verifying key itself ([SEC-AUZ-014]), not only through
    /// the binding table: once A1's binding is gone ([SEC-PRS-009]), a reply to msg-b from
    /// "A1" signed by another trusted key is refused, and binds nothing.
    #[test]
    fn reply_right_is_bound_to_the_key_the_message_was_sent_to() {
        let mut p = pair();
        let carol = identity("principal-c");
        p.bob_engine
            .pair(
                PairedPeer::confirmed(carol.principal().clone(), *carol.public_key(), now()),
                &p.store,
            )
            .unwrap();
        let to_alice = p.bob.sign_envelope(
            EnvelopeDraft::new(
                Token::parse("msg-b").unwrap(),
                sid(B1),
                sid(A1),
                ts("2026-10-03T11:30:00.000Z"),
                vec![TextPart::new("q").unwrap()],
            )
            .unwrap(),
        );
        p.bob_engine
            .record_sent(SentRecord::of(&to_alice, p.alice.key_id().clone()));
        assert_eq!(
            p.bob_engine.bind(&sid(A1), p.alice.key_id()),
            BindOutcome::Bound
        );
        assert!(p.bob_engine.forget_binding(&sid(A1)));
        let forged = message(&carol, &p.bob_engine, A1, B1, Some("msg-b"));
        let r = p
            .bob_engine
            .authorize_delivery_at(forged, &now())
            .unwrap_err();
        assert_eq!(r.requirement, "SEC-AUZ-001");
        assert!(
            p.bob_engine.binding(&sid(A1)).is_none(),
            "a refusal binds nothing"
        );
        let genuine = message(&p.alice, &p.bob_engine, A1, B1, Some("msg-b"));
        let d = p
            .bob_engine
            .authorize_delivery_at(genuine, &now())
            .unwrap()
            .decision()
            .clone();
        assert!(matches!(d.basis(), Some(Basis::ReplyRight(_))));
        assert_eq!(
            p.bob_engine.binding(&sid(A1)),
            Some(&Binding::Key(p.alice.key_id().clone()))
        );
    }

    #[test]
    fn envelope_binds_unbound_from_and_never_rebinds() {
        let mut p = pair();
        let carol = identity("principal-c");
        p.bob_engine
            .pair(
                PairedPeer::confirmed(carol.principal().clone(), *carol.public_key(), now()),
                &p.store,
            )
            .unwrap();
        for k in [p.alice.key_id(), carol.key_id()] {
            p.bob_engine
                .add_grant(
                    inbound(PeerSide::device(k.clone()), LocalSide::Session(sid(B1))),
                    ok(),
                    &p.store,
                )
                .unwrap();
        }
        let msg = message(&p.alice, &p.bob_engine, A1, B1, None);
        p.bob_engine.authorize_delivery_at(msg, &now()).unwrap();
        assert_eq!(
            p.bob_engine.binding(&sid(A1)),
            Some(&Binding::Key(p.alice.key_id().clone()))
        );
        // Carol claims A1: refused with a finding, binding unchanged, no conflict mark.
        let spoof = message(&carol, &p.bob_engine, A1, B1, None);
        let r = p
            .bob_engine
            .authorize_delivery_at(spoof, &now())
            .unwrap_err();
        assert_eq!(r.requirement, "SEC-AUZ-003");
        assert_eq!(
            r.finding,
            Some(BindingFinding {
                session_id: sid(A1),
                bound_to: p.alice.key_id().clone(),
                claimed_by: carol.key_id().clone(),
            })
        );
        assert!(
            p.log
                .entries()
                .iter()
                .any(|e| matches!(e, LogEntry::Finding(_)))
        );
        assert_eq!(
            p.bob_engine.binding(&sid(A1)),
            Some(&Binding::Key(p.alice.key_id().clone()))
        );
        // A conflict mark (set from presence) refuses even the first claimant.
        assert!(p.bob_engine.mark_conflict(&sid(A1), carol.key_id()));
        let msg = message(&p.alice, &p.bob_engine, A1, B1, None);
        assert_eq!(
            p.bob_engine
                .authorize_delivery_at(msg, &now())
                .unwrap_err()
                .requirement,
            "SEC-PRS-012"
        );
        // An own session is never marked ([SEC-PRS-015]), nor forgotten.
        assert!(!p.bob_engine.mark_conflict(&sid(B1), carol.key_id()));
        assert!(!p.bob_engine.forget_binding(&sid(B1)));
        assert!(
            !p.bob_engine.forget_binding(&sid(A1)),
            "a conflict mark is not forgotten"
        );
        // Removing Carol's key removes the mark and her grant in the same step.
        let removed = p
            .bob_engine
            .remove_key(carol.key_id(), ok(), &p.store)
            .unwrap();
        assert_eq!(removed.bindings, vec![sid(A1)]);
        assert_eq!(removed.grants.len(), 1);
        assert!(p.bob_engine.binding(&sid(A1)).is_none());
        assert!(
            p.bob_engine
                .grants()
                .iter()
                .all(|g| !g.names_key(carol.key_id()))
        );
        assert!(p.bob_engine.trusted_keys().get(carol.key_id()).is_none());
        // The store holds the change too.
        let back = AuthorizationEngine::restore(
            &p.bob,
            &p.store,
            clock(),
            Box::new(MemoryDecisionLog::new()),
        )
        .unwrap();
        assert!(back.trusted_keys().get(carol.key_id()).is_none());
        assert!(back.trusted_keys().get(p.alice.key_id()).is_some());
        assert_eq!(back.grants(), p.bob_engine.grants());
    }

    #[test]
    fn own_key_cannot_be_removed_and_removal_is_fail_closed() {
        let mut p = pair();
        let own = p.bob.key_id().clone();
        assert_eq!(
            p.bob_engine.remove_key(&own, ok(), &p.store),
            Err(RemoveKeyError::OwnKey)
        );
        let stranger = identity("principal-s");
        assert_eq!(
            p.bob_engine.remove_key(stranger.key_id(), ok(), &p.store),
            Err(RemoveKeyError::NotTrusted)
        );
        p.store.fail_saves(true);
        let ak = p.alice.key_id().clone();
        assert!(matches!(
            p.bob_engine.remove_key(&ak, ok(), &p.store),
            Err(RemoveKeyError::NotSaved(_))
        ));
        assert!(
            p.bob_engine.trusted_keys().get(&ak).is_none(),
            "fail closed"
        );
        // An addition that is not saved is not made.
        assert!(matches!(
            p.bob_engine.pair(
                PairedPeer::confirmed(p.alice.principal().clone(), *p.alice.public_key(), now()),
                &p.store
            ),
            Err(PairError::NotSaved(_))
        ));
        assert!(p.bob_engine.trusted_keys().get(&ak).is_none());
        let g = inbound(PeerSide::device(ak.clone()), LocalSide::Device);
        assert!(p.bob_engine.add_grant(g, ok(), &p.store).is_err());
        assert!(p.bob_engine.grants().is_empty());
    }

    #[test]
    fn discovery_follows_write_rights_and_hand_offs() {
        let mut p = pair();
        let ak = p.alice.key_id().clone();
        // Alice's A1 is bound on Bob's side; an inbound grant (A1 may write to B1) does
        // not let B1 discover A1 ([SEC-AUZ-012]).
        assert_eq!(p.bob_engine.bind(&sid(A1), &ak), BindOutcome::Bound);
        assert_eq!(p.bob_engine.bind(&sid(A1), &ak), BindOutcome::AlreadyBound);
        p.bob_engine
            .add_grant(
                inbound(
                    PeerSide::session(ak.clone(), sid(A1)),
                    LocalSide::Session(sid(B1)),
                ),
                ok(),
                &p.store,
            )
            .unwrap();
        let discover = |e: &mut AuthorizationEngine, l: &str, s: &str, at: &Timestamp| {
            e.decide_at(
                &AuthorizationRequest::Discover {
                    requester: Requester::Session(sid(l)),
                    session: sid(s),
                },
                at,
            )
        };
        assert!(!discover(&mut p.bob_engine, B1, A1, &now()).permits(Kind::Discover));
        // Handed a message from A1, B1 may discover A1 for the reply period
        // ([SEC-AUZ-016]); B2 may not.
        let msg = message(&p.alice, &p.bob_engine, A1, B1, None);
        let msg = p
            .bob_engine
            .authorize_delivery_at(msg, &now())
            .unwrap()
            .into_message();
        p.bob_engine.record_handoff(
            HandOffRecord::of(msg.envelope()),
            DeliveryState::HandedToHarness,
        );
        let d = discover(&mut p.bob_engine, B1, A1, &now());
        assert!(matches!(d.basis(), Some(Basis::HandOff(_))));
        assert!(!discover(&mut p.bob_engine, B2, A1, &now()).permits(Kind::Discover));
        let end = ts("2026-10-04T12:00:00.000Z");
        assert!(!discover(&mut p.bob_engine, B1, A1, &end).permits(Kind::Discover));
        // A failed hand-off records nothing.
        p.bob_engine.prune_at(&end);
        p.bob_engine
            .record_handoff(HandOffRecord::of(msg.envelope()), DeliveryState::Failed);
        assert!(!discover(&mut p.bob_engine, B1, A1, &now()).permits(Kind::Discover));
        // An outbound grant (B2 may write to Alice's device) lets B2 discover A1.
        p.bob_engine
            .add_grant(
                Grant::Outbound {
                    writer: LocalSide::Session(sid(B2)),
                    target: PeerSide::device(ak.clone()),
                },
                ok(),
                &p.store,
            )
            .unwrap();
        assert!(discover(&mut p.bob_engine, B2, A1, &now()).permits(Kind::Discover));
        // ... and relates Alice's key to A1 for accepting its announcement ([SEC-AUZ-017]).
        assert!(
            p.bob_engine
                .decide_at(
                    &AuthorizationRequest::AcceptPresence {
                        signing_key: ak,
                        session: sid(A1)
                    },
                    &now()
                )
                .permits(Kind::AcceptPresence)
        );
    }

    #[test]
    fn grant_kinds_cover_what_they_name() {
        let mut p = pair();
        let ak = p.alice.key_id().clone();
        // A session grant names B1 only; B2 shares its scope and is not covered
        // ([SEC-AUZ-006]).
        p.bob_engine
            .add_grant(
                inbound(PeerSide::device(ak.clone()), LocalSide::Session(sid(B1))),
                ok(),
                &p.store,
            )
            .unwrap();
        for (to, pass) in [(B1, true), (B2, false), (B3, false)] {
            let m = message(&p.alice, &p.bob_engine, A1, to, None);
            assert_eq!(
                p.bob_engine.authorize_delivery_at(m, &now()).is_ok(),
                pass,
                "{to}"
            );
        }
        // A writer naming one session does not cover another session of the same key.
        let mut q = pair();
        let ak = q.alice.key_id().clone();
        q.bob_engine
            .add_grant(
                inbound(PeerSide::session(ak, sid(A1)), LocalSide::Device),
                ok(),
                &q.store,
            )
            .unwrap();
        let other = message(
            &q.alice,
            &q.bob_engine,
            "3kp8w2n6c9t4x1b5m7q0r3s8vz",
            B1,
            None,
        );
        assert!(q.bob_engine.authorize_delivery_at(other, &now()).is_err());
        let m = message(&q.alice, &q.bob_engine, A1, B3, None);
        assert!(q.bob_engine.authorize_delivery_at(m, &now()).is_ok());
        // Duplicate grants are not added twice; removal reports what it did.
        let g = q.bob_engine.grants()[0].clone();
        assert!(!q.bob_engine.add_grant(g.clone(), ok(), &q.store).unwrap());
        assert!(q.bob_engine.remove_grant(&g, &q.store).unwrap());
        assert!(!q.bob_engine.remove_grant(&g, &q.store).unwrap());
    }

    fn kid_n(n: u8) -> KeyId {
        KeyId::parse(&format!("{n:02x}").repeat(32)).unwrap()
    }

    fn sid_n(n: u32) -> SessionId {
        let mut o = [0u8; 16];
        o[..4].copy_from_slice(&n.to_be_bytes());
        o[15] = 1;
        SessionId::from_random_octets(o)
    }

    fn handoff(id: &str, from: &SessionId, to: &SessionId, key: Option<KeyId>) -> HandOffRecord {
        HandOffRecord {
            id: Token::parse(id).unwrap(),
            from: from.clone(),
            to: to.clone(),
            created_at: now(),
            conversation_id: Token::parse("conv"),
            correlation_id: Token::parse("corr"),
            key_id: key,
        }
    }

    /// #313: the hand-off records keep `conversation_id` and `correlation_id` and serve the
    /// reply headers ([SC-RCP-053], [SC-RCP-054]); a forgotten record fails closed (an
    /// uncorrelated reply), and `forget_handoff` removes one.
    #[test]
    fn handoff_records_serve_reply_headers() {
        let me = identity("p");
        let clock = Arc::new(crate::clock::ManualClock::new(now()));
        let mut e = AuthorizationEngine::new(&me, clock, Box::new(MemoryDecisionLog::new()));
        let peer = Some(kid_n(7));
        // A handed-off envelope bound its sender to the key that verified it
        // ([SEC-PRS-005]); the records are looked up through that binding.
        e.bind(&sid(A1), &kid_n(7));
        let r = handoff("m7", &sid(A1), &sid(B1), peer.clone());
        e.record_handoff(r.clone(), DeliveryState::HandedToHarness);
        e.record_handoff(
            handoff("m8", &sid(A1), &sid(B1), peer),
            DeliveryState::Failed,
        );
        let h = e.reply_headers(&sid(B1), &sid(A1), Some("m7"));
        assert!(h.correlated());
        assert_eq!(h.conversation_id, Token::parse("conv"));
        assert_eq!(h.correlation_id, Token::parse("corr"));
        assert!(!e.reply_headers(&sid(B1), &sid(A1), Some("m8")).correlated());
        assert!(!e.reply_headers(&sid(B2), &sid(A1), Some("m7")).correlated());
        assert!(e.forget_handoff(&r));
        assert!(!e.forget_handoff(&r));
        assert!(!e.reply_headers(&sid(B1), &sid(A1), Some("m7")).correlated());
    }

    /// PR #326 review B2, remote side: one peer device handed off more than a partition
    /// holds evicts only its own records; another peer's record, its correlation and its
    /// sender's discovery right ([SEC-AUZ-016]) survive. The flood's own oldest goes.
    #[test]
    fn one_peer_cannot_evict_another_peers_handoff_records() {
        let me = identity("p");
        let clock = Arc::new(crate::clock::ManualClock::new(now()));
        let mut e = AuthorizationEngine::new(&me, clock, Box::new(MemoryDecisionLog::new()));
        register(&mut e, &me, B1, "/w");
        let (good, flood) = (kid_n(1), kid_n(2));
        e.bind(&sid(A1), &good);
        e.bind(&sid(B2), &flood);
        e.record_handoff(
            handoff("keep", &sid(A1), &sid(B1), Some(good)),
            DeliveryState::HandedToHarness,
        );
        for i in 0..=MAX_RECORDS_PER_PARTITION {
            e.record_handoff(
                handoff(&format!("f{i}"), &sid(B2), &sid(B1), Some(flood.clone())),
                DeliveryState::HandedToHarness,
            );
        }
        assert_eq!(e.handoff_records().count(), MAX_RECORDS_PER_PARTITION + 1);
        assert!(
            e.reply_headers(&sid(B1), &sid(A1), Some("keep"))
                .correlated()
        );
        assert!(!e.reply_headers(&sid(B1), &sid(B2), Some("f0")).correlated());
        assert!(e.reply_headers(&sid(B1), &sid(B2), Some("f1")).correlated());
        let d = e.decide(&AuthorizationRequest::Discover {
            requester: Requester::Session(sid(B1)),
            session: sid(A1),
        });
        assert!(matches!(d.basis(), Some(Basis::HandOff(_))));
    }

    /// PR #326 review B2, local side: one own session that sends a flood evicts only its own
    /// sent records, never another own session's reply right ([SEC-AUZ-013]); and one own
    /// session handed off to another evicts only its own hand-off records.
    #[test]
    fn one_local_session_cannot_evict_another_sessions_records() {
        let me = identity("p");
        let clock = Arc::new(crate::clock::ManualClock::new(now()));
        let mut e = AuthorizationEngine::new(&me, clock, Box::new(MemoryDecisionLog::new()));
        register(&mut e, &me, A1, "/w");
        register(&mut e, &me, B1, "/w");
        let sign = |from: &str, id: &str| {
            me.sign_envelope(
                EnvelopeDraft::new(
                    Token::parse(id).unwrap(),
                    sid(from),
                    sid(B3),
                    now(),
                    vec![TextPart::new("x").unwrap()],
                )
                .unwrap(),
            )
        };
        let peer = kid_n(9);
        e.record_sent(SentRecord::of(&sign(A1, "keep"), peer.clone()));
        let flood = sign(B1, "flood");
        for _ in 0..=MAX_RECORDS_PER_PARTITION {
            e.record_sent(SentRecord::of(&flood, peer.clone()));
        }
        assert_eq!(
            e.sent_records_from(&sid(B1)).count(),
            MAX_RECORDS_PER_PARTITION
        );
        assert_eq!(
            e.sent_records_from(&sid(A1))
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            ["keep"]
        );
        // Hand-off records under this device's own key are partitioned by sending session.
        let own = Some(me.key_id().clone());
        e.record_handoff(
            handoff("mine", &sid(A1), &sid(B2), own.clone()),
            DeliveryState::HandedToHarness,
        );
        for i in 0..=MAX_RECORDS_PER_PARTITION {
            e.record_handoff(
                handoff(&format!("f{i}"), &sid(B1), &sid(B2), own.clone()),
                DeliveryState::HandedToHarness,
            );
        }
        assert!(
            e.reply_headers(&sid(B2), &sid(A1), Some("mine"))
                .correlated()
        );
    }

    /// A record for a new partition past [`MAX_RECORD_PARTITIONS`] is not kept; no existing
    /// partition is evicted for it.
    #[test]
    fn partitions_are_bounded_without_eviction() {
        let me = identity("p");
        let clock = Arc::new(crate::clock::ManualClock::new(now()));
        let mut e = AuthorizationEngine::new(&me, clock, Box::new(MemoryDecisionLog::new()));
        for n in 0..=MAX_RECORD_PARTITIONS as u32 {
            let env = me.sign_envelope(
                EnvelopeDraft::new(
                    Token::parse("m").unwrap(),
                    sid_n(n),
                    sid(B3),
                    now(),
                    vec![TextPart::new("x").unwrap()],
                )
                .unwrap(),
            );
            e.record_sent(SentRecord::of(&env, kid_n(1)));
        }
        assert_eq!(e.sent_records().count(), MAX_RECORD_PARTITIONS);
        assert_eq!(e.sent_records_from(&sid_n(0)).count(), 1);
        assert_eq!(
            e.sent_records_from(&sid_n(MAX_RECORD_PARTITIONS as u32))
                .count(),
            0
        );
    }

    fn kid_u32(n: u32) -> KeyId {
        KeyId::parse(&format!("{n:064x}")).unwrap()
    }

    /// PR #326 re-review B2', the reviewer's churn scenario, at a partition cap of 16 so it
    /// runs in every `cargo test` (#328): as many short-lived own sessions as the cap each
    /// send, hand one message to a live own session, and end. Their partitions go with
    /// them, so a new peer's hand-off is still recorded, its reply correlates
    /// ([SC-RCP-053]) and its sender stays discoverable ([SEC-AUZ-016]).
    #[test]
    fn ended_sessions_free_their_partitions() {
        churn_frees_partitions(16);
    }

    /// [`ended_sessions_free_their_partitions`] at the real cap, [`MAX_RECORD_PARTITIONS`].
    /// Each session's registration record takes a signature and a verification, so this
    /// takes about a minute in a debug build and about a second in a release one: it runs
    /// in the opt-in tier `node scripts/local-ci.mjs --tier scale`, or by hand with
    /// `cargo test -p oac-core --release -- --ignored full_scale` (#328).
    #[test]
    #[ignore = "full scale: run with --release -- --ignored full_scale (local-ci.mjs --tier scale)"]
    fn full_scale_ended_sessions_free_their_partitions() {
        churn_frees_partitions(MAX_RECORD_PARTITIONS);
    }

    fn churn_frees_partitions(cap: usize) {
        let me = identity("p");
        let clock = Arc::new(crate::clock::ManualClock::new(now()));
        let mut e = AuthorizationEngine::new(&me, clock, Box::new(MemoryDecisionLog::new()));
        e.sent = RecordBook::with_max_partitions(cap, false);
        e.handed_off = RecordBook::with_max_partitions(cap, true);
        register(&mut e, &me, B1, "/w");
        let own = Some(me.key_id().clone());
        for n in 0..cap as u32 {
            let s = sid_n(n);
            register(&mut e, &me, s.as_str(), "/w");
            let sent = me.sign_envelope(
                EnvelopeDraft::new(
                    Token::parse("s").unwrap(),
                    s.clone(),
                    sid(B3),
                    now(),
                    vec![TextPart::new("x").unwrap()],
                )
                .unwrap(),
            );
            e.record_sent(SentRecord::of(&sent, kid_n(3)));
            e.record_handoff(
                handoff("m", &s, &sid(B1), own.clone()),
                DeliveryState::HandedToHarness,
            );
            assert!(e.end_session(&s));
        }
        assert_eq!(e.handed_off.partitions(), 0);
        assert_eq!(e.sent.partitions(), 0);
        let peer = kid_n(42);
        e.bind(&sid(A1), &peer);
        e.record_handoff(
            handoff("legit", &sid(A1), &sid(B1), Some(peer)),
            DeliveryState::HandedToHarness,
        );
        assert!(
            e.reply_headers(&sid(B1), &sid(A1), Some("legit"))
                .correlated()
        );
        let d = e.decide(&AuthorizationRequest::Discover {
            requester: Requester::Session(sid(B1)),
            session: sid(A1),
        });
        assert!(matches!(d.basis(), Some(Basis::HandOff(_))));
    }

    /// B2': at the partition cap, partitions whose records are all past their reply period
    /// are swept before a new one is refused; one emptied partition can be made again; a
    /// key removed from trust takes its partition with it.
    #[test]
    fn expired_emptied_and_removed_partitions_are_reclaimed() {
        let me = identity("p");
        let clock = Arc::new(crate::clock::ManualClock::new(now()));
        let mut e =
            AuthorizationEngine::new(&me, clock.clone(), Box::new(MemoryDecisionLog::new()));
        for n in 0..MAX_RECORD_PARTITIONS as u32 {
            e.record_handoff(
                handoff("m", &sid(A1), &sid(B1), Some(kid_u32(n))),
                DeliveryState::HandedToHarness,
            );
        }
        // At the cap, all live: a new writer is refused, no live partition evicted.
        let late = handoff("late", &sid(A1), &sid(B1), Some(kid_u32(1 << 20)));
        e.record_handoff(late.clone(), DeliveryState::HandedToHarness);
        assert_eq!(e.handed_off.partitions(), MAX_RECORD_PARTITIONS);
        assert!(!e.handoff_records().any(|r| r.id.as_str() == "late"));
        // One partition emptied by `forget_handoff` is gone, so it can be made again.
        assert!(e.forget_handoff(&handoff("m", &sid(A1), &sid(B1), Some(kid_u32(7)))));
        e.record_handoff(late.clone(), DeliveryState::HandedToHarness);
        assert!(e.handoff_records().any(|r| r.id.as_str() == "late"));
        // Past the reply period every partition is expired: the next new one sweeps them.
        clock.advance_nanos(i128::from(REPLY_PERIOD_MS) * NANOS_PER_MS);
        let mut fresh = handoff("fresh", &sid(A1), &sid(B1), Some(kid_u32(1 << 21)));
        fresh.created_at = e.now();
        e.record_handoff(fresh, DeliveryState::HandedToHarness);
        assert_eq!(e.handed_off.partitions(), 1);
        assert!(e.handoff_records().any(|r| r.id.as_str() == "fresh"));
        // A key removed from trust drops its partition.
        let q = identity("q");
        let store = MemoryPairingStore::new();
        e.pair(
            PairedPeer::confirmed(q.principal().clone(), *q.public_key(), e.now()),
            &store,
        )
        .unwrap();
        let mut theirs = handoff("theirs", &sid(A1), &sid(B1), Some(q.key_id().clone()));
        theirs.created_at = e.now();
        e.record_handoff(theirs, DeliveryState::HandedToHarness);
        assert_eq!(e.handed_off.partitions(), 2);
        e.remove_key(q.key_id(), ok(), &store).unwrap();
        assert_eq!(e.handed_off.partitions(), 1);
        assert!(!e.handoff_records().any(|r| r.id.as_str() == "theirs"));
    }

    /// The total across partitions is bounded ([`MAX_RECORDS_TOTAL`]): past it, the largest
    /// partition gives up its oldest record, never a small writer's.
    #[test]
    fn the_total_is_bounded_by_the_largest_partition() {
        let me = identity("p");
        let clock = Arc::new(crate::clock::ManualClock::new(now()));
        let mut e = AuthorizationEngine::new(&me, clock, Box::new(MemoryDecisionLog::new()));
        let full = MAX_RECORDS_TOTAL / MAX_RECORDS_PER_PARTITION;
        for n in 0..full as u32 {
            for i in 0..MAX_RECORDS_PER_PARTITION {
                e.record_handoff(
                    handoff(&format!("m{i}"), &sid(A1), &sid(B1), Some(kid_u32(n))),
                    DeliveryState::HandedToHarness,
                );
            }
        }
        assert_eq!(e.handed_off.len(), MAX_RECORDS_TOTAL);
        let small = Some(kid_u32(1 << 22));
        e.record_handoff(
            handoff("small", &sid(A1), &sid(B1), small.clone()),
            DeliveryState::HandedToHarness,
        );
        assert_eq!(e.handed_off.len(), MAX_RECORDS_TOTAL);
        assert!(e.handoff_records().any(|r| r.id.as_str() == "small"));
        // The flood continues; the small writer keeps its record.
        for i in 0..100 {
            e.record_handoff(
                handoff(&format!("x{i}"), &sid(A1), &sid(B1), Some(kid_u32(0))),
                DeliveryState::HandedToHarness,
            );
        }
        assert_eq!(e.handed_off.len(), MAX_RECORDS_TOTAL);
        assert!(e.handoff_records().any(|r| r.id.as_str() == "small"));
    }

    const HOUR: i128 = 3_600_000_000_000;

    /// #328: expired partitions are found in order of expiry, and a partition whose writer
    /// refreshes it moves in that order instead of leaving a stale entry. Four writers, one
    /// an hour apart, at a cap of four; the first refreshes after the last.
    #[test]
    fn expired_partitions_go_in_expiry_order() {
        let me = identity("p");
        let clock = Arc::new(crate::clock::ManualClock::new(now()));
        let mut e =
            AuthorizationEngine::new(&me, clock.clone(), Box::new(MemoryDecisionLog::new()));
        e.handed_off = RecordBook::with_max_partitions(4, true);
        let put = |e: &mut AuthorizationEngine, id: &str, n: u32| {
            let mut r = handoff(id, &sid(A1), &sid(B1), Some(kid_u32(n)));
            r.created_at = e.now();
            e.record_handoff(r, DeliveryState::HandedToHarness);
            e.handoff_records().any(|r| r.id.as_str() == id)
        };
        for n in 0..4 {
            assert!(put(&mut e, &format!("w{n}"), n));
            clock.advance_nanos(HOUR);
        }
        // Writer 0 refreshes at hour 4: its partition now expires at hour 28, not 24.
        assert!(put(&mut e, "w0b", 0));
        assert_eq!(e.handed_off.by_end.len(), e.handed_off.partitions());
        // Hour 24: every partition is live, writer 0's too; a new writer is refused.
        clock.advance_nanos(20 * HOUR);
        assert!(!put(&mut e, "late", 9));
        assert!(e.handoff_records().any(|r| r.id.as_str() == "w0b"));
        // Hour 25: writer 1's partition (hour 1 plus 24) has expired; it alone goes.
        clock.advance_nanos(HOUR);
        assert!(put(&mut e, "late", 9));
        let left: Vec<&str> = e.handoff_records().map(|r| r.id.as_str()).collect();
        assert!(!left.contains(&"w1"), "{left:?}");
        for id in ["w0", "w0b", "w2", "w3", "late"] {
            assert!(left.contains(&id), "{id} in {left:?}");
        }
        assert_eq!(e.handed_off.by_end.len(), e.handed_off.partitions());
    }

    /// #328: a late copy from an ended own session does not make that session's partition
    /// again: no hand-off record from a session that is not bound now is kept under this
    /// device's own key. A record from a live own session still is.
    #[test]
    fn a_late_copy_from_an_ended_own_session_records_nothing() {
        let me = identity("p");
        let clock = Arc::new(crate::clock::ManualClock::new(now()));
        let mut e = AuthorizationEngine::new(&me, clock, Box::new(MemoryDecisionLog::new()));
        register(&mut e, &me, A1, "/w");
        register(&mut e, &me, B1, "/w");
        let own = Some(me.key_id().clone());
        e.record_handoff(
            handoff("live", &sid(A1), &sid(B1), own.clone()),
            DeliveryState::HandedToHarness,
        );
        assert_eq!(e.handed_off.partitions(), 1);
        assert!(e.end_session(&sid(A1)));
        assert_eq!(e.handed_off.partitions(), 0);
        e.record_handoff(
            handoff("late", &sid(A1), &sid(B1), own),
            DeliveryState::Unknown,
        );
        assert_eq!(e.handed_off.partitions(), 0);
        assert_eq!(e.handoff_records().count(), 0);
    }

    /// Bob's engine with Alice and Carol paired, each granted to write to any own session,
    /// and the envelope bindings bounded at `capacity` in all and `per_key` each.
    fn bounded(capacity: usize, per_key: usize) -> (Pair, DeviceIdentity) {
        let mut p = pair();
        p.bob_engine = p.bob_engine.with_envelope_binding_limits(capacity, per_key);
        let carol = identity("principal-c");
        p.bob_engine
            .pair(
                PairedPeer::confirmed(carol.principal().clone(), *carol.public_key(), now()),
                &p.store,
            )
            .unwrap();
        for k in [p.alice.key_id().clone(), carol.key_id().clone()] {
            p.bob_engine
                .add_grant(
                    inbound(PeerSide::device(k), LocalSide::Device),
                    ok(),
                    &p.store,
                )
                .unwrap();
        }
        (p, carol)
    }

    fn held(e: &AuthorizationEngine, k: &KeyId) -> usize {
        e.bindings()
            .filter(|(_, b)| **b == Binding::Key(k.clone()))
            .count()
    }

    /// #325: the binding-table entries step 4 creates are bounded per key. A device that
    /// sends from fresh session ids evicts only its own oldest entries; the entry a
    /// hand-off record is looked up through stays, so the reply still correlates
    /// ([SC-RCP-053]) and the sender stays discoverable ([SEC-AUZ-016]); another key still
    /// binds.
    #[test]
    fn envelope_bindings_are_bounded_and_keep_what_records_use() {
        let (mut p, carol) = bounded(4, 2);
        let ak = p.alice.key_id().clone();
        let first = message(&p.alice, &p.bob_engine, A1, B1, None);
        let first = p
            .bob_engine
            .authorize_delivery_at(first, &now())
            .unwrap()
            .into_message();
        p.bob_engine.record_handoff(
            HandOffRecord::of(first.envelope()),
            DeliveryState::HandedToHarness,
        );
        for n in 0..20 {
            let m = message(&p.alice, &p.bob_engine, sid_n(n).as_str(), B1, None);
            p.bob_engine.authorize_delivery_at(m, &now()).unwrap();
            assert!(held(&p.bob_engine, &ak) <= 2);
        }
        assert_eq!(
            p.bob_engine.binding(&sid(A1)),
            Some(&Binding::Key(ak.clone()))
        );
        assert_eq!(
            p.bob_engine.binding(&sid_n(19)),
            Some(&Binding::Key(ak.clone()))
        );
        assert!(p.bob_engine.binding(&sid_n(18)).is_none());
        assert!(
            p.bob_engine
                .reply_headers(&sid(B1), &sid(A1), Some("msg-1"))
                .correlated()
        );
        let m = message(&carol, &p.bob_engine, B2, B1, None);
        assert!(
            p.bob_engine.authorize_delivery_at(m, &now()).is_err(),
            "B2 is own"
        );
        let m = message(&carol, &p.bob_engine, sid_n(100).as_str(), B1, None);
        p.bob_engine.authorize_delivery_at(m, &now()).unwrap();
        assert_eq!(p.bob_engine.envelope_bindings(), 3);
        // Once the record is forgotten, A1's entry may go too.
        assert!(
            p.bob_engine
                .forget_handoff(&HandOffRecord::of(first.envelope()))
        );
        for n in 200..202 {
            let m = message(&p.alice, &p.bob_engine, sid_n(n).as_str(), B1, None);
            p.bob_engine.authorize_delivery_at(m, &now()).unwrap();
        }
        assert!(p.bob_engine.binding(&sid(A1)).is_none());
    }

    /// PR #334 review B1, R4, R5, R7: an entry is kept while a hand-off record from its
    /// session is held, also when the record came before the entry. An announcement binds
    /// `S`, a record from `S` is kept, the registry forgets `S`; a later hand-off from `S`
    /// binds it again, and a later envelope binds another such id. Fresh ids then evict
    /// neither, so discovery ([SEC-AUZ-016]) and correlation ([SC-RCP-053]) still work.
    /// Once `prune` drops the records, the entries may go at once.
    #[test]
    fn entries_whose_records_came_first_are_kept() {
        let (mut p, _carol) = bounded(16, 3);
        let ak = p.alice.key_id().clone();
        let mut fresh = 1000;
        let mut flood = |p: &mut Pair, n: u32| {
            for _ in 0..n {
                fresh += 1;
                let m = message(&p.alice, &p.bob_engine, sid_n(fresh).as_str(), B1, None);
                p.bob_engine.authorize_delivery_at(m, &now()).unwrap();
            }
        };
        // B1: the hand-off binds `S` again.
        let s = sid_n(500);
        assert_eq!(p.bob_engine.bind(&s, &ak), BindOutcome::Bound);
        let r1 = handoff("r1", &s, &sid(B1), Some(ak.clone()));
        p.bob_engine
            .record_handoff(r1, DeliveryState::HandedToHarness);
        assert!(p.bob_engine.forget_binding(&s));
        p.bob_engine.record_handoff(
            handoff("r2", &s, &sid(B1), Some(ak.clone())),
            DeliveryState::HandedToHarness,
        );
        assert_eq!(p.bob_engine.binding(&s), Some(&Binding::Key(ak.clone())));
        assert_eq!(p.bob_engine.envelope_bindings(), 1, "the rebind is counted");
        flood(&mut p, 4);
        assert_eq!(p.bob_engine.binding(&s), Some(&Binding::Key(ak.clone())));
        let d = p.bob_engine.decide(&AuthorizationRequest::Discover {
            requester: Requester::Session(sid(B1)),
            session: s.clone(),
        });
        assert!(matches!(d.basis(), Some(Basis::HandOff(_))));
        assert!(
            p.bob_engine
                .reply_headers(&sid(B1), &s, Some("r1"))
                .correlated()
        );
        // R5: an envelope binds `S2`, whose record came first.
        let s2 = sid_n(600);
        assert_eq!(p.bob_engine.bind(&s2, &ak), BindOutcome::Bound);
        p.bob_engine.record_handoff(
            handoff("r3", &s2, &sid(B1), Some(ak.clone())),
            DeliveryState::HandedToHarness,
        );
        assert!(p.bob_engine.forget_binding(&s2));
        let m = message(&p.alice, &p.bob_engine, s2.as_str(), B1, None);
        p.bob_engine.authorize_delivery_at(m, &now()).unwrap();
        flood(&mut p, 4);
        assert_eq!(p.bob_engine.binding(&s2), Some(&Binding::Key(ak.clone())));
        assert_eq!(p.bob_engine.binding(&s), Some(&Binding::Key(ak.clone())));
        assert_eq!(held(&p.bob_engine, &ak), 3);
        // R4: past the reply period `prune` drops the records, and the entries are free at
        // once: the next fresh id takes the oldest, `S`.
        p.bob_engine.prune_at(&ts("2026-10-04T13:00:00Z"));
        flood(&mut p, 1);
        assert!(p.bob_engine.binding(&s).is_none());
        assert_eq!(held(&p.bob_engine, &ak), 3);
    }

    /// #325: entries that something else now refers to leave the bound: an announcement
    /// confirms one ([`AuthorizationEngine::bind`]), a registration replaces one, and a
    /// conflict mark replaces one. A share filled only with entries hand-off records use
    /// refuses the next new `from` with `failed` / `internal-error`, binding nothing; an
    /// entry evicted between step 4 and the hand-off is bound again by its record.
    #[test]
    fn envelope_bindings_leave_the_bound_when_referred_to() {
        let (mut p, carol) = bounded(8, 2);
        let ak = p.alice.key_id().clone();
        let authorize = |p: &mut Pair, by_carol: bool, from: &SessionId| {
            let signer = if by_carol { &carol } else { &p.alice };
            let m = message(signer, &p.bob_engine, from.as_str(), B1, None);
            p.bob_engine.authorize_delivery_at(m, &now())
        };
        authorize(&mut p, false, &sid_n(1)).unwrap();
        assert_eq!(p.bob_engine.envelope_bindings(), 1);
        assert_eq!(p.bob_engine.bind(&sid_n(1), &ak), BindOutcome::AlreadyBound);
        assert_eq!(p.bob_engine.envelope_bindings(), 0);
        authorize(&mut p, false, &sid_n(2)).unwrap();
        assert!(p.bob_engine.mark_conflict(&sid_n(2), carol.key_id()));
        assert_eq!(p.bob_engine.envelope_bindings(), 0);
        // Two entries in use by hand-off records fill Alice's share: the next is refused.
        for n in [3, 4] {
            let m = authorize(&mut p, false, &sid_n(n)).unwrap().into_message();
            p.bob_engine.record_handoff(
                HandOffRecord::of(m.envelope()),
                DeliveryState::HandedToHarness,
            );
        }
        let r = authorize(&mut p, false, &sid_n(5)).unwrap_err();
        assert_eq!(
            (r.state, r.error, r.requirement),
            (
                DeliveryState::Failed,
                ErrorCode::InternalError,
                "SEC-PRS-005"
            )
        );
        assert!(p.bob_engine.binding(&sid_n(5)).is_none());
        // Evicted between step 4 and the hand-off: the record binds the id again.
        let m = authorize(&mut p, true, &sid_n(6)).unwrap().into_message();
        authorize(&mut p, true, &sid_n(7)).unwrap();
        authorize(&mut p, true, &sid_n(8)).unwrap();
        assert!(p.bob_engine.binding(&sid_n(6)).is_none(), "evicted");
        p.bob_engine.record_handoff(
            HandOffRecord::of(m.envelope()),
            DeliveryState::HandedToHarness,
        );
        assert_eq!(
            p.bob_engine.binding(&sid_n(6)),
            Some(&Binding::Key(carol.key_id().clone()))
        );
        // Room is made for it as for any new entry: Carol's oldest free entry goes, and
        // she stays at her share (PR #334 review N3).
        assert!(p.bob_engine.binding(&sid_n(7)).is_none());
        assert_eq!(held(&p.bob_engine, carol.key_id()), 2);
        let d = p.bob_engine.decide(&AuthorizationRequest::Discover {
            requester: Requester::Session(sid(B1)),
            session: sid_n(6),
        });
        assert!(matches!(d.basis(), Some(Basis::HandOff(_))));
        // A registration replaces an entry.
        let own = sid_n(8);
        let rec = p
            .bob
            .register(
                own.clone(),
                Token::parse("h").unwrap(),
                "n",
                "/w",
                ts("2026-10-03T11:00:00Z"),
            )
            .unwrap();
        let before = p.bob_engine.envelope_bindings();
        assert!(p.bob_engine.register_session(&rec, &p.bob));
        assert_eq!(p.bob_engine.envelope_bindings(), before - 1);
    }
}
