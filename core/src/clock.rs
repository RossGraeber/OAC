// SPDX-License-Identifier: Apache-2.0

//! The receiver's clock, injectable (#53, F4).
//!
//! The replay window and the hand-off deadline are read on the receiver's own clock
//! (`spec/security.md` §8.1; `spec/session-channels.md` [SC-RCP-091]). Code that needs the
//! time takes a [`Clock`] rather than reading the system time itself, so tests and the
//! conformance runner can drive it with a [`ManualClock`]: no test waits for real time to
//! pass.

use crate::ids::Timestamp;
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// A source of the current instant.
pub trait Clock: Send + Sync {
    /// The current instant.
    fn now(&self) -> Timestamp;
}

/// The UTC wall clock of the operating system.
///
/// The window is read on a wall clock because `created_at` is a wall-clock instant
/// (`spec/security.md` §8.1, reference implementation note). A wall clock can step
/// backwards; a receiver whose clock is far wrong refuses good envelopes as
/// `outside-replay-window`, which the sender sees.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let nanos = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => i128::try_from(d.as_nanos()).unwrap_or(i128::MAX),
            Err(e) => -i128::try_from(e.duration().as_nanos()).unwrap_or(i128::MAX),
        };
        // A system clock outside the years 0000 to 9999 cannot be written as a timestamp;
        // the nearest writable instant keeps every comparison on the right side.
        Timestamp::from_unix_nanos(nanos)
            .or_else(|| {
                Timestamp::parse(if nanos < 0 {
                    "0000-01-01T00:00:00Z"
                } else {
                    "9999-12-31T23:59:59.999999999Z"
                })
            })
            .expect("a constant timestamp")
    }
}

/// A clock that moves only when told to: for tests, and for the conformance runner's
/// scripted clock (`spec/security.md` §3.3, note).
#[derive(Debug)]
pub struct ManualClock {
    now: Mutex<Timestamp>,
}

impl ManualClock {
    /// A clock reading `start`.
    pub fn new(start: Timestamp) -> ManualClock {
        ManualClock {
            now: Mutex::new(start),
        }
    }

    /// Sets the reading to `t`, which may be earlier than the current one, as a wall clock
    /// can be.
    pub fn set(&self, t: Timestamp) {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner) = t;
    }

    /// Moves the reading by `nanos` nanoseconds, forwards or backwards.
    ///
    /// # Panics
    ///
    /// When the result is outside the years 0000 to 9999.
    pub fn advance_nanos(&self, nanos: i128) {
        let mut now = self.now.lock().unwrap_or_else(PoisonError::into_inner);
        *now = Timestamp::from_unix_nanos(now.unix_nanos() + nanos)
            .expect("the clock stays within the years 0000 to 9999");
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        self.now
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nanosecond_timestamps_round_trip() {
        for s in [
            "2026-10-03T12:05:00.000000001Z",
            "1970-01-01T00:00:00.000000000Z",
            "1969-12-31T23:59:59.999999999Z",
            "0000-01-01T00:00:00.000000000Z",
            "9999-12-31T23:59:59.999999999Z",
        ] {
            let t = Timestamp::parse(s).unwrap();
            let back = Timestamp::from_unix_nanos(t.unix_nanos()).unwrap();
            assert_eq!(back.as_str(), s);
            assert_eq!(back.unix_nanos(), t.unix_nanos());
        }
        let last = Timestamp::parse("9999-12-31T23:59:59.999999999Z").unwrap();
        assert!(Timestamp::from_unix_nanos(last.unix_nanos() + 1).is_none());
        let first = Timestamp::parse("0000-01-01T00:00:00Z").unwrap();
        assert!(Timestamp::from_unix_nanos(first.unix_nanos() - 1).is_none());
    }

    #[test]
    fn manual_clock_moves_only_when_told() {
        let c = ManualClock::new(Timestamp::parse("2026-10-03T12:00:00.000Z").unwrap());
        assert_eq!(c.now().as_str(), "2026-10-03T12:00:00.000Z");
        c.advance_nanos(300_000_000_001);
        assert_eq!(c.now().as_str(), "2026-10-03T12:05:00.000000001Z");
        c.advance_nanos(-1);
        assert_eq!(c.now().as_str(), "2026-10-03T12:05:00.000000000Z");
        c.set(Timestamp::parse("2026-10-03T11:00:00Z").unwrap());
        assert_eq!(c.now().as_str(), "2026-10-03T11:00:00Z");
    }

    #[test]
    fn system_clock_reads_a_plausible_instant() {
        let t = SystemClock.now();
        // After this file was written, and with nine fraction digits.
        assert!(
            t.unix_nanos()
                > Timestamp::parse("2026-01-01T00:00:00Z")
                    .unwrap()
                    .unix_nanos()
        );
        assert_eq!(t.as_str().len(), "2026-10-03T12:00:00.000000000Z".len());
    }
}
