// SPDX-License-Identifier: Apache-2.0

//! Fault injection: loss, duplication, delay and reordering, for tests
//! (`spec/interfaces.md` §6.2; [IFC-TRN-011]).
//!
//! A [`FaultInjector`] decides, for each payload the network takes, how many copies it
//! carries and how long each copy is delayed. No copies is a loss, two or more is a
//! duplication, a delay is a delay, and unequal delays across payloads reorder them. A
//! copy whose delay reaches the payload's deadline is dropped, never held
//! ([IFC-TRN-034]).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use oac_core::transport::{Destination, PayloadKind};

/// What a fault injector sees of a payload the network is about to carry.
#[derive(Clone, Copy, Debug)]
pub struct PublishInfo<'a> {
    /// The destination the payload names.
    pub destination: &'a Destination,
    /// The payload's kind.
    pub kind: PayloadKind,
    /// The payload's length in octets.
    pub len: usize,
}

/// Decides the copies of each payload a network takes.
pub trait FaultInjector: Send {
    /// One delay per copy to carry: empty loses the payload, one entry delivers it once,
    /// more entries duplicate it. Any delay is accepted: one that reaches the payload's
    /// deadline, or that no instant can represent (such as `Duration::MAX`), drops that
    /// copy.
    fn copies(&mut self, info: PublishInfo<'_>) -> Vec<Duration>;

    /// True only if this injector never delivers two payloads from one source to one
    /// destination out of the order passed. The network declares `ordering` present only
    /// when this is true ([IFC-TRN-021]).
    fn preserves_order(&self) -> bool {
        false
    }
}

/// No faults: every payload is carried once, at once. Preserves order.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoFaults;

impl FaultInjector for NoFaults {
    fn copies(&mut self, _: PublishInfo<'_>) -> Vec<Duration> {
        vec![Duration::ZERO]
    }

    fn preserves_order(&self) -> bool {
        true
    }
}

/// Every payload is carried once, after the same delay. Preserves order.
#[derive(Clone, Copy, Debug)]
pub struct FixedDelay(pub Duration);

impl FaultInjector for FixedDelay {
    fn copies(&mut self, _: PublishInfo<'_>) -> Vec<Duration> {
        vec![self.0]
    }

    fn preserves_order(&self) -> bool {
        true
    }
}

/// A script of fates, one per payload taken, in the order taken. A payload taken when the
/// script is empty is carried once, at once. Cloning shares the script, so a test keeps a
/// clone and adds fates while the network runs. Declares no ordering.
#[derive(Clone, Debug, Default)]
pub struct ScriptedFaults {
    script: Arc<Mutex<VecDeque<Vec<Duration>>>>,
}

impl ScriptedFaults {
    /// An empty script.
    pub fn new() -> ScriptedFaults {
        ScriptedFaults::default()
    }

    /// The next payload taken gets one copy per entry of `delays`.
    pub fn push(&self, delays: Vec<Duration>) {
        self.script
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(delays);
    }

    /// The next payload taken is carried once, at once.
    pub fn deliver(&self) {
        self.push(vec![Duration::ZERO]);
    }

    /// The next payload taken is lost.
    pub fn lose(&self) {
        self.push(Vec::new());
    }

    /// The next payload taken is carried once, after `delay`.
    pub fn delay(&self, delay: Duration) {
        self.push(vec![delay]);
    }

    /// The next payload taken is carried `copies` times, at once.
    pub fn duplicate(&self, copies: usize) {
        self.push(vec![Duration::ZERO; copies]);
    }

    /// The number of fates not yet used.
    pub fn pending(&self) -> usize {
        self.script.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

impl FaultInjector for ScriptedFaults {
    fn copies(&mut self, _: PublishInfo<'_>) -> Vec<Duration> {
        self.script
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
            .unwrap_or_else(|| vec![Duration::ZERO])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oac_core::ids::SessionId;

    #[test]
    fn script_is_used_in_order_then_defaults_to_one_copy() {
        let d = Destination::Session(SessionId::from_random_octets([1; 16]));
        let info = PublishInfo {
            destination: &d,
            kind: PayloadKind::Envelope,
            len: 1,
        };
        let s = ScriptedFaults::new();
        let mut injector = s.clone();
        s.lose();
        s.duplicate(3);
        s.delay(Duration::from_millis(5));
        assert_eq!(s.pending(), 3);
        assert!(injector.copies(info).is_empty());
        assert_eq!(injector.copies(info), vec![Duration::ZERO; 3]);
        assert_eq!(injector.copies(info), vec![Duration::from_millis(5)]);
        assert_eq!(injector.copies(info), vec![Duration::ZERO]);
        assert!(!injector.preserves_order());
        assert!(NoFaults.preserves_order());
        assert!(FixedDelay(Duration::from_millis(1)).preserves_order());
        assert_eq!(NoFaults.copies(info), vec![Duration::ZERO]);
    }
}
