// SPDX-License-Identifier: Apache-2.0

//! Carrier loss and carrier appearance from Zenoh-native liveliness (#63, G2).
//!
//! Each started transport declares one native liveliness token, whose name is an opaque
//! hash of its random native session id ([`crate::addressing::token_name`]), and one
//! liveliness subscriber with `history(true)` on the partition's token keys. Zenoh owns
//! everything else: token propagation, history replay on late start, loss detection on
//! undeclare, closed link or lease expiry, and the resynchronisation after a reconnect. This
//! module keeps no heartbeat, timer, probe or gossip of its own. It only records which
//! remote tokens the native subscriber has seen alive, reports an appearance when a token is
//! first seen (declared live, or replayed from history because it was declared before this
//! transport started) and reports a loss when one of those goes away.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use oac_core::presence::PresenceState;
use oac_core::transport::{CarrierHandle, PresenceEvent, PresenceHandler};
use zenoh::handlers::Callback;
use zenoh::sample::SampleKind;

use crate::addressing::Partition;
use crate::gate::Gate;

/// The `watch_presence` handlers of a started transport.
#[derive(Default)]
pub(crate) struct Watchers {
    handlers: Mutex<Vec<PresenceHandler>>,
}

impl Watchers {
    pub(crate) fn add(&self, handler: PresenceHandler) {
        self.handlers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(handler);
    }

    /// True when at least one `watch_presence` handler is registered.
    pub(crate) fn any(&self) -> bool {
        !self
            .handlers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }

    /// Hand a carrier loss for `carrier` to every watcher, behind the transport's gate
    /// ([IFC-TRN-071]): none runs once `shutdown` has closed it.
    pub(crate) fn carrier_loss(&self, gate: &Arc<Gate>, carrier: &CarrierHandle) {
        self.report(gate, || PresenceEvent::CarrierLoss {
            carrier: carrier.clone(),
        });
    }

    /// Hand a carrier appearance for `carrier` to every watcher, behind the same gate
    /// ([IFC-TRN-121], [IFC-TRN-071]). The event carries the handle only ([IFC-TRN-122]).
    pub(crate) fn carrier_appeared(&self, gate: &Arc<Gate>, carrier: &CarrierHandle) {
        self.report(gate, || PresenceEvent::CarrierAppeared {
            carrier: carrier.clone(),
        });
    }

    fn report(&self, gate: &Arc<Gate>, event: impl Fn() -> PresenceEvent) {
        let handlers = self
            .handlers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if handlers.is_empty() {
            return;
        }
        let Some(_pass) = gate.enter() else { return };
        for handler in handlers {
            handler(event());
        }
    }
}

/// What the native liveliness callback has seen and the reporter thread has not yet handed
/// on, and the wake-up that tells the reporter to look.
///
/// The wake-up is a native one-slot ring: it coalesces, but the two flags do not, so a loss
/// is never hidden by a later appearance nor the other way round. The native callback only
/// sets a flag and wakes; it never runs a core handler. An appearance seen while no
/// `watch_presence` handler is registered stays pending until one is (the core registers
/// after `start`, and history replay can land before that); a loss in that interval is
/// dropped, as no one has asked to hear of it.
pub(crate) struct Signals {
    /// `(lost, appeared)`.
    pending: Mutex<(bool, bool)>,
    wake: Mutex<Option<Callback<()>>>,
}

impl Signals {
    pub(crate) fn new(wake: Callback<()>) -> Signals {
        Signals {
            pending: Mutex::new((false, false)),
            wake: Mutex::new(Some(wake)),
        }
    }

    /// A token this subscriber saw alive has gone.
    pub(crate) fn lost(&self) {
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).0 = true;
        self.wake();
    }

    /// A token not seen alive before has appeared.
    pub(crate) fn appeared(&self) {
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).1 = true;
        self.wake();
    }

    /// Wake the reporter without marking anything (a watcher was added).
    pub(crate) fn wake(&self) {
        if let Some(w) = self.wake.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            w.call(());
        }
    }

    /// `(lost, appeared)` to report now. With no watcher registered nothing is reported, a
    /// pending loss is dropped and a pending appearance is kept.
    pub(crate) fn take(&self, watched: bool) -> (bool, bool) {
        let mut p = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if watched {
            std::mem::take(&mut *p)
        } else {
            p.0 = false;
            (false, false)
        }
    }

    /// Drop this holder of the wake-up's sender, so the reporter's blocking read ends once
    /// the native callback's own copy is gone too.
    pub(crate) fn close(&self) {
        self.wake.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
}

/// The remote transports whose native liveliness token this transport has seen alive.
///
/// The three states of C7 §4, as the transport can know them:
///
/// * `Online`: the token was seen declared, by history replay or a live event.
/// * `Unreachable`: a token that was `Online` has gone, whether Zenoh saw it undeclared, its
///   link close, or its lease run out.
/// * `Unknown`: Zenoh reported the token gone but this subscriber never saw it declared (a
///   deletion with no earlier declaration). Nothing is known about it, so nothing is
///   reported.
pub(crate) struct Links {
    partition: Partition,
    own: String,
    online: Mutex<HashSet<String>>,
}

impl Links {
    pub(crate) fn new(partition: Partition, own_token: String) -> Links {
        Links {
            partition,
            own: own_token,
            online: Mutex::new(HashSet::new()),
        }
    }

    /// What one native liveliness sample on `key` says. `None` when `key` is not another
    /// OAC transport's token of this partition (this transport's own token, a malformed
    /// name or a foreign key): such a sample says nothing about a carrier.
    #[cfg(test)]
    pub(crate) fn observe(&self, kind: SampleKind, key: &str) -> Option<PresenceState> {
        self.transition(kind, key).map(|(state, _)| state)
    }

    /// As [`Links::observe`], and whether the sample changed what is known: a token first
    /// seen alive (an appearance), or a token seen alive that has gone (a loss). A token
    /// declared again without a deletion in between, as when history is replayed or a
    /// reconnect re-delivers the live set, changes nothing.
    pub(crate) fn transition(&self, kind: SampleKind, key: &str) -> Option<(PresenceState, bool)> {
        let name = self.partition.token_of(key)?;
        if name == self.own {
            return None;
        }
        let mut online = self.online.lock().unwrap_or_else(|e| e.into_inner());
        Some(match kind {
            SampleKind::Put => (PresenceState::Online, online.insert(name.to_owned())),
            SampleKind::Delete => {
                if online.remove(name) {
                    (PresenceState::Unreachable, true)
                } else {
                    (PresenceState::Unknown, false)
                }
            }
        })
    }

    /// The number of remote transports now seen alive (tests only).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn online(&self) -> usize {
        self.online.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addressing::token_name;

    fn links() -> (Links, Partition, String) {
        let p = Partition::new("links");
        let own = token_name("own");
        (Links::new(p.clone(), own.clone()), p, own)
    }

    #[test]
    fn a_seen_token_that_goes_is_unreachable_and_an_unseen_one_is_unknown() {
        let (l, p, _) = links();
        let a = p.token_key(&token_name("a"));
        let b = p.token_key(&token_name("b"));
        // Never declared to this subscriber: nothing is known, so nothing is claimed.
        assert_eq!(
            l.observe(SampleKind::Delete, &a),
            Some(PresenceState::Unknown)
        );
        assert_eq!(l.observe(SampleKind::Put, &a), Some(PresenceState::Online));
        assert_eq!(l.observe(SampleKind::Put, &b), Some(PresenceState::Online));
        assert_eq!(l.online(), 2);
        assert_eq!(
            l.observe(SampleKind::Delete, &a),
            Some(PresenceState::Unreachable)
        );
        assert_eq!(l.online(), 1);
        // A second deletion of the same token is no new loss.
        assert_eq!(
            l.observe(SampleKind::Delete, &a),
            Some(PresenceState::Unknown)
        );
        // History replay and a reconnect re-declare the same token: still one link.
        assert_eq!(l.observe(SampleKind::Put, &b), Some(PresenceState::Online));
        assert_eq!(l.online(), 1);
    }

    /// [IFC-TRN-120]: an appearance is a token first seen alive, not a token declared again.
    #[test]
    fn only_a_token_not_seen_alive_before_is_an_appearance() {
        let (l, p, _) = links();
        let a = p.token_key(&token_name("a"));
        assert_eq!(
            l.transition(SampleKind::Put, &a),
            Some((PresenceState::Online, true))
        );
        // History replayed again, or the same live set after a reconnect: no new appearance.
        assert_eq!(
            l.transition(SampleKind::Put, &a),
            Some((PresenceState::Online, false))
        );
        assert_eq!(
            l.transition(SampleKind::Delete, &a),
            Some((PresenceState::Unreachable, true))
        );
        // After a loss the same token is a new appearance.
        assert_eq!(
            l.transition(SampleKind::Put, &a),
            Some((PresenceState::Online, true))
        );
        // A deletion nobody saw the declaration of is no change at all.
        let b = p.token_key(&token_name("b"));
        assert_eq!(
            l.transition(SampleKind::Delete, &b),
            Some((PresenceState::Unknown, false))
        );
    }

    #[test]
    fn an_appearance_waits_for_a_watcher_and_a_loss_does_not() {
        use zenoh::handlers::{IntoHandler, RingChannel};
        let (wake, _rx) = RingChannel::new(1).into_handler();
        let s = Signals::new(wake);
        s.appeared();
        s.lost();
        assert_eq!(s.take(false), (false, false));
        assert_eq!(s.take(true), (false, true));
        s.lost();
        s.appeared();
        assert_eq!(s.take(true), (true, true));
        assert_eq!(s.take(true), (false, false));
    }

    #[test]
    fn own_malformed_and_foreign_keys_say_nothing() {
        let (l, p, own) = links();
        for kind in [SampleKind::Put, SampleKind::Delete] {
            assert_eq!(l.observe(kind, &p.token_key(&own)), None);
            assert_eq!(
                l.observe(kind, &format!("{}/live/not-a-token", p.prefix())),
                None
            );
            assert_eq!(l.observe(kind, &p.key()), None);
            assert_eq!(
                l.observe(kind, &Partition::new("other").token_key(&token_name("a"))),
                None
            );
        }
        assert_eq!(l.online(), 0);
    }

    /// [IFC-TRN-121] and [IFC-TRN-122]: every watcher gets the event, whose only member is
    /// the carrier handle, and none once the gate has closed.
    #[test]
    fn an_appearance_reaches_every_watcher_with_the_handle_only_until_the_gate_closes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let w = Watchers::default();
        let n = Arc::new(AtomicUsize::new(0));
        let carrier = CarrierHandle::from_opaque(vec![1]);
        for _ in 0..2 {
            let (n, expected) = (n.clone(), carrier.clone());
            w.add(Arc::new(move |e| {
                assert_eq!(
                    e,
                    PresenceEvent::CarrierAppeared {
                        carrier: expected.clone()
                    }
                );
                n.fetch_add(1, Ordering::SeqCst);
            }));
        }
        let gate = Gate::new();
        w.carrier_appeared(&gate, &carrier);
        assert_eq!(n.load(Ordering::SeqCst), 2);
        gate.close();
        w.carrier_appeared(&gate, &carrier);
        assert_eq!(n.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_loss_reaches_every_watcher_until_the_gate_closes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let w = Watchers::default();
        let n = Arc::new(AtomicUsize::new(0));
        for _ in 0..2 {
            let n = n.clone();
            w.add(Arc::new(move |e| {
                assert!(matches!(e, PresenceEvent::CarrierLoss { .. }));
                n.fetch_add(1, Ordering::SeqCst);
            }));
        }
        let gate = Gate::new();
        let carrier = CarrierHandle::from_opaque(vec![1]);
        w.carrier_loss(&gate, &carrier);
        assert_eq!(n.load(Ordering::SeqCst), 2);
        gate.close();
        w.carrier_loss(&gate, &carrier);
        assert_eq!(n.load(Ordering::SeqCst), 2);
    }
}
