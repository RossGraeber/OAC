// SPDX-License-Identifier: Apache-2.0

//! Carrier loss from Zenoh-native liveliness (#63, G2).
//!
//! Each started transport declares one native liveliness token, whose name is an opaque
//! hash of its random native session id ([`crate::addressing::token_name`]), and one
//! liveliness subscriber with `history(true)` on the partition's token keys. Zenoh owns
//! everything else: token propagation, history replay on late start, loss detection on
//! undeclare, closed link or lease expiry, and the resynchronisation after a reconnect. This
//! module keeps no heartbeat, timer, probe or gossip of its own. It only records which
//! remote tokens the native subscriber has seen alive and reports a loss when one of those
//! goes away.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use oac_core::presence::PresenceState;
use oac_core::transport::{CarrierHandle, PresenceEvent, PresenceHandler};
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

    /// Hand a carrier loss for `carrier` to every watcher, behind the transport's gate
    /// ([IFC-TRN-071]): none runs once `shutdown` has closed it.
    pub(crate) fn carrier_loss(&self, gate: &Arc<Gate>, carrier: &CarrierHandle) {
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
            handler(PresenceEvent::CarrierLoss {
                carrier: carrier.clone(),
            });
        }
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
    pub(crate) fn observe(&self, kind: SampleKind, key: &str) -> Option<PresenceState> {
        let name = self.partition.token_of(key)?;
        if name == self.own {
            return None;
        }
        let mut online = self.online.lock().unwrap_or_else(|e| e.into_inner());
        Some(match kind {
            SampleKind::Put => {
                online.insert(name.to_owned());
                PresenceState::Online
            }
            SampleKind::Delete => {
                if online.remove(name) {
                    PresenceState::Unreachable
                } else {
                    PresenceState::Unknown
                }
            }
        })
    }

    /// The number of remote transports now seen alive.
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
