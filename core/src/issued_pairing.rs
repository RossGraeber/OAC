//! The core's pairing windows for issued values (`spec/interfaces.md` §5.6;
//! `spec/bindings/mcp.md` §4.5).
//!
//! An adapter binding document can make a harness's own report of a refused request a native
//! signal, by having the refusal carry a value the core issued on that attachment. This module
//! is the state machine behind it. It issues a value per attachment and window, remembers
//! which attachment a value was issued on, and decides what a later reveal of a value means:
//! it narrows the candidates the operating-system key leaves to that attachment
//! ([SC-ID-125]), once, and within the window.
//!
//! It holds no clock and no connection; the pipelines give it the instant, and it is generic
//! over the attachment key so that it is decided without a transport or an adapter.
//!
//! Requirements implemented here: [MCPB-ATT-005] (the form), [IFC-ADP-090] (a secure random
//! source of 128 bits), [MCPB-ATT-025] (one value per window), [MCPB-ATT-026] (no value of an
//! ended window), [MCPB-ATT-019] (a window of at most 60 seconds), [MCPB-ATT-023] (at most
//! three windows since the last reveal paired), [MCPB-ATT-009] (no pairing after the window),
//! [MCPB-ATT-010] (single use), [MCPB-ATT-011] (a value revealed for two threads).

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::time::{Duration, Instant};

/// The prefix of a pairing value ([MCPB-ATT-005]).
pub const PAIRING_VALUE_PREFIX: &str = "oac-pair-";

/// The longest a pairing window may last ([MCPB-ATT-019]).
pub const MAX_WINDOW: Duration = Duration::from_secs(60);

/// The most windows opened on one attachment since the last reveal paired on it
/// ([MCPB-ATT-023]).
pub const MAX_WINDOWS: u32 = 3;

/// The records kept per attachment, ended and paired windows included, so that an ended or
/// already paired value is recognised as such. The oldest goes past it; a value no longer
/// recognised is unknown, and an unknown value pairs nothing.
///
/// Residual (`docs/planning/v0.1/11-risks.md`, `RISK-ISSUED-PAIRING`): after 16 records an evicted
/// paired value that is revealed for a second thread is `Unknown`, not `Conflict`, so
/// [MCPB-ATT-011] is lost for a very old value. The reveal still pairs nothing.
const MAX_RECORDS: usize = 16;

/// What a reveal of a pairing value means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RevealOutcome<A> {
    /// A value the core did not issue, or one it no longer remembers. Pairs nothing.
    Unknown,
    /// The value's window had ended when the reveal arrived ([MCPB-ATT-009]). Pairs nothing.
    Ended(A),
    /// The value was already paired by an earlier reveal of the same harness-native id
    /// ([MCPB-ATT-010]). Pairs nothing and changes nothing.
    Replayed(A),
    /// The value was already paired by an earlier reveal of another harness-native id
    /// ([MCPB-ATT-011]): the binding of the attachment it was issued on ends, with a finding
    /// ([MCPB-ATT-012]).
    Conflict(A),
    /// The caller's check refused the attachment the value was issued on (it is not open, or
    /// its key does not match the carrier's); the value is not used up.
    Refused(A),
    /// The value is used up now: narrow the candidates to this attachment.
    Fresh(A),
}

#[derive(Clone, Debug)]
enum State {
    /// Issued, no reveal paired.
    Open,
    /// Paired by a reveal of this harness-native id.
    Paired(String),
}

#[derive(Clone, Debug)]
struct Record {
    value: String,
    opened: Instant,
    state: State,
}

#[derive(Debug)]
struct Windows {
    /// Windows opened since the last reveal paired.
    since_paired: u32,
    /// Oldest first.
    records: VecDeque<Record>,
}

/// The pairing windows of every attachment of one core.
#[derive(Debug)]
pub struct PairingWindows<A> {
    window: Duration,
    by_attachment: HashMap<A, Windows>,
    by_value: HashMap<String, A>,
}

impl<A: Clone + Eq + Hash> PairingWindows<A> {
    /// Windows of `window` (at most [`MAX_WINDOW`]; a longer one is cut to it, a zero one
    /// issues nothing useful and is raised to a millisecond).
    pub fn new(window: Duration) -> PairingWindows<A> {
        PairingWindows {
            window: window.clamp(Duration::from_millis(1), MAX_WINDOW),
            by_attachment: HashMap::new(),
            by_value: HashMap::new(),
        }
    }

    /// The length of a window.
    pub fn window(&self) -> Duration {
        self.window
    }

    fn live(&self, r: &Record, now: Instant) -> bool {
        matches!(r.state, State::Open) && now < r.opened + self.window
    }

    /// The pairing value of `attachment`'s window at `now`: the open window's value
    /// ([MCPB-ATT-025]), else the value of a new window; `None` when three windows have been
    /// opened since the last reveal paired ([MCPB-ATT-023]) or the random source failed (no
    /// value is better than a weak one).
    ///
    /// The caller has decided that the attachment can be paired ([MCPB-ATT-022]).
    pub fn issue(&mut self, attachment: &A, now: Instant) -> Option<String> {
        if let Some(w) = self.by_attachment.get(attachment) {
            if let Some(r) = w.records.back().filter(|r| self.live(r, now)) {
                return Some(r.value.clone());
            }
            if w.since_paired >= MAX_WINDOWS {
                return None;
            }
        }
        let value = fresh_value()?;
        if self.by_value.contains_key(&value) {
            // 128 random bits never repeat in practice; if they do, issue nothing.
            return None;
        }
        let w = self
            .by_attachment
            .entry(attachment.clone())
            .or_insert_with(|| Windows {
                since_paired: 0,
                records: VecDeque::new(),
            });
        w.since_paired += 1;
        w.records.push_back(Record {
            value: value.clone(),
            opened: now,
            state: State::Open,
        });
        while w.records.len() > MAX_RECORDS {
            if let Some(old) = w.records.pop_front() {
                self.by_value.remove(&old.value);
            }
        }
        self.by_value.insert(value.clone(), attachment.clone());
        Some(value)
    }

    /// What a reveal of `value`, naming the harness-native id `native_id`, means at `now`.
    ///
    /// `acceptable` is asked, for a value that is open and in its window, whether the
    /// attachment it was issued on may be paired now; when it says no the value is left as it
    /// was. The caller has already checked that the reveal came on a carrier ([MCPB-ATT-018]
    /// is the adapter's).
    pub fn reveal(
        &mut self,
        value: &str,
        native_id: &str,
        now: Instant,
        acceptable: impl FnOnce(&A) -> bool,
    ) -> RevealOutcome<A> {
        let Some(attachment) = self.by_value.get(value).cloned() else {
            return RevealOutcome::Unknown;
        };
        let window = self.window;
        let Some(w) = self.by_attachment.get_mut(&attachment) else {
            return RevealOutcome::Unknown;
        };
        let Some(r) = w.records.iter_mut().find(|r| r.value == value) else {
            return RevealOutcome::Unknown;
        };
        match &r.state {
            State::Paired(n) if n == native_id => return RevealOutcome::Replayed(attachment),
            State::Paired(_) => return RevealOutcome::Conflict(attachment),
            State::Open => {}
        }
        if now >= r.opened + window {
            return RevealOutcome::Ended(attachment);
        }
        if !acceptable(&attachment) {
            return RevealOutcome::Refused(attachment);
        }
        // The value is used up here, at the reveal's arrival, so that two reveals of one
        // value are told apart in arrival order. A reveal the core's cap then turns away has
        // used it up without pairing; that fails closed (the thread is refused again, with
        // a new window only while fewer than 3 have opened, else finally, [MCPB-ATT-023];
        // availability only, `RISK-ISSUED-PAIRING`). The count of windows is not reset here, but by
        // [`PairingWindows::paired`] when a reveal does pair ([MCPB-ATT-023]).
        r.state = State::Paired(native_id.to_owned());
        RevealOutcome::Fresh(attachment)
    }

    /// A reveal paired with `attachment`: the count of windows opened since the last paired
    /// reveal starts again ([MCPB-ATT-023]).
    pub fn paired(&mut self, attachment: &A) {
        if let Some(w) = self.by_attachment.get_mut(attachment) {
            w.since_paired = 0;
        }
    }

    /// Forgets everything issued on `attachment`: it ended.
    pub fn forget(&mut self, attachment: &A) {
        if let Some(w) = self.by_attachment.remove(attachment) {
            for r in w.records {
                self.by_value.remove(&r.value);
            }
        }
    }

    /// Whether any value is remembered for `attachment`.
    pub fn knows(&self, attachment: &A) -> bool {
        self.by_attachment.contains_key(attachment)
    }
}

/// `oac-pair-` and 32 lower-case hexadecimal digits of 128 bits from the operating system's
/// random source ([MCPB-ATT-005], [IFC-ADP-090]); `None` when the source fails.
fn fresh_value() -> Option<String> {
    let mut octets = [0u8; 16];
    getrandom::fill(&mut octets).ok()?;
    let mut s = String::with_capacity(PAIRING_VALUE_PREFIX.len() + 32);
    s.push_str(PAIRING_VALUE_PREFIX);
    for o in octets {
        s.push(char::from_digit(u32::from(o >> 4), 16)?);
        s.push(char::from_digit(u32::from(o & 15), 16)?);
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Duration = Duration::from_secs(1);

    fn w() -> (PairingWindows<u8>, Instant) {
        (PairingWindows::new(Duration::from_secs(10)), Instant::now())
    }

    fn yes(_: &u8) -> bool {
        true
    }

    /// [MCPB-ATT-005], [IFC-ADP-090]: `oac-pair-` and 32 lower-case hex digits, never the
    /// same twice.
    #[test]
    fn a_value_has_the_form_of_att_005_and_differs_each_window() {
        let (mut p, t) = w();
        let mut seen = std::collections::HashSet::new();
        for a in 0..50u8 {
            let v = p.issue(&a, t).unwrap();
            let digits = v.strip_prefix(PAIRING_VALUE_PREFIX).unwrap();
            assert_eq!(digits.len(), 32, "{v}");
            assert!(
                digits
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{v}"
            );
            assert!(seen.insert(v));
        }
    }

    /// [MCPB-ATT-025]: one value per window, however many refusals; [MCPB-ATT-026]: a new
    /// window never reuses the value of an ended one.
    #[test]
    fn one_value_per_window_and_a_new_one_after_it_ends() {
        let (mut p, t) = w();
        let v1 = p.issue(&1, t).unwrap();
        assert_eq!(p.issue(&1, t + S).unwrap(), v1);
        assert_eq!(p.issue(&1, t + S * 9).unwrap(), v1);
        let v2 = p.issue(&1, t + S * 10).unwrap();
        assert_ne!(v1, v2);
        // The other attachment's value is its own.
        assert_ne!(p.issue(&2, t).unwrap(), v1);
    }

    /// [MCPB-ATT-019]: no window is longer than 60 seconds, whatever was configured.
    #[test]
    fn a_window_is_cut_to_sixty_seconds() {
        let mut p = PairingWindows::<u8>::new(Duration::from_secs(3600));
        assert_eq!(p.window(), MAX_WINDOW);
        let t = Instant::now();
        let v = p.issue(&1, t).unwrap();
        assert_eq!(p.issue(&1, t + Duration::from_secs(59)).unwrap(), v);
        assert_eq!(
            p.reveal(&v, "T", t + Duration::from_secs(61), yes),
            RevealOutcome::Ended(1)
        );
        assert_ne!(p.issue(&1, t + Duration::from_secs(61)).unwrap(), v);
    }

    /// [MCPB-ATT-023]: three windows since the last reveal paired, then none; a paired
    /// reveal starts the count again.
    #[test]
    fn at_most_three_windows_until_a_reveal_pairs() {
        let (mut p, mut t) = w();
        let mut last = String::new();
        for _ in 0..3 {
            last = p.issue(&1, t).unwrap();
            t += S * 11;
        }
        assert_eq!(p.issue(&1, t), None);
        assert_eq!(p.issue(&1, t + S * 100), None);
        // Another attachment is unaffected.
        assert!(p.issue(&2, t).is_some());
        // The third window's value is ended; it cannot pair, so the count stands.
        assert_eq!(p.reveal(&last, "T", t, yes), RevealOutcome::Ended(1));
        assert_eq!(p.issue(&1, t), None);
    }

    #[test]
    fn a_paired_reveal_resets_the_window_count() {
        let (mut p, mut t) = w();
        p.issue(&1, t).unwrap();
        t += S * 11;
        p.issue(&1, t).unwrap();
        t += S * 11;
        let v = p.issue(&1, t).unwrap();
        assert_eq!(p.reveal(&v, "T", t + S, yes), RevealOutcome::Fresh(1));
        p.paired(&1);
        t += S * 2;
        let again = p.issue(&1, t).unwrap();
        assert_ne!(again, v, "a paired window is over");
    }

    /// [MCPB-ATT-010]: single use. [MCPB-ATT-011]: another thread's reveal of the same value.
    #[test]
    fn a_value_pairs_once_and_a_second_thread_is_a_conflict() {
        let (mut p, t) = w();
        let v = p.issue(&1, t).unwrap();
        assert_eq!(p.reveal(&v, "T", t + S, yes), RevealOutcome::Fresh(1));
        assert_eq!(p.reveal(&v, "T", t + S, yes), RevealOutcome::Replayed(1));
        assert_eq!(p.reveal(&v, "U", t + S, yes), RevealOutcome::Conflict(1));
        // Still a replay, a conflict, after the window has ended.
        assert_eq!(
            p.reveal(&v, "T", t + S * 99, yes),
            RevealOutcome::Replayed(1)
        );
        assert_eq!(
            p.reveal(&v, "U", t + S * 99, yes),
            RevealOutcome::Conflict(1)
        );
    }

    /// [MCPB-ATT-009]: a reveal after the window pairs nothing; an unknown value pairs
    /// nothing either, and a value is not read off another attachment's.
    #[test]
    fn a_late_or_unknown_value_pairs_nothing() {
        let (mut p, t) = w();
        let v = p.issue(&1, t).unwrap();
        assert_eq!(p.reveal(&v, "T", t + S * 10, yes), RevealOutcome::Ended(1));
        assert_eq!(
            p.reveal("oac-pair-00000000000000000000000000000000", "T", t, yes),
            RevealOutcome::Unknown
        );
        assert_eq!(p.reveal("", "T", t, yes), RevealOutcome::Unknown);
    }

    /// A value the caller finds unacceptable is not used up.
    #[test]
    fn a_refused_attachment_leaves_the_value_unused() {
        let (mut p, t) = w();
        let v = p.issue(&1, t).unwrap();
        assert_eq!(p.reveal(&v, "T", t, |_| false), RevealOutcome::Refused(1));
        assert_eq!(p.reveal(&v, "T", t, yes), RevealOutcome::Fresh(1));
    }

    #[test]
    fn forgetting_an_attachment_forgets_its_values() {
        let (mut p, t) = w();
        let v = p.issue(&1, t).unwrap();
        assert!(p.knows(&1));
        p.forget(&1);
        assert!(!p.knows(&1));
        assert_eq!(p.reveal(&v, "T", t, yes), RevealOutcome::Unknown);
        // A new attachment of the same key starts from nothing.
        assert!(p.issue(&1, t).is_some());
    }

    /// The history is bounded; an evicted value is unknown, never paired.
    #[test]
    fn the_history_is_bounded() {
        let mut p = PairingWindows::<u8>::new(Duration::from_millis(1));
        let mut t = Instant::now();
        let first = p.issue(&1, t).unwrap();
        for _ in 0..MAX_RECORDS + 4 {
            // Pair each so that the window count does not stop issuing.
            t += Duration::from_millis(0);
            let v = p.issue(&1, t).unwrap_or_else(|| {
                t += Duration::from_millis(2);
                p.issue(&1, t).unwrap()
            });
            let _ = p.reveal(&v, "T", t, yes);
            p.paired(&1);
            t += Duration::from_millis(2);
        }
        assert_eq!(p.reveal(&first, "T", t, yes), RevealOutcome::Unknown);
        assert!(p.by_attachment[&1].records.len() <= MAX_RECORDS);
    }
}
