// SPDX-License-Identifier: Apache-2.0

//! A gate in front of the core's handlers ([IFC-TRN-071], and the "once ending returns"
//! promise of `oac_core::transport::Subscription`).
//!
//! Zenoh calls a subscriber's callback on its own threads. The gate lets a callback through
//! only while it is open, counts the callbacks inside, and `close` waits until none is left.
//! So once `close` returns, no handler behind the gate runs again. A handler that closes the
//! gate it is running behind (a handler that calls `shutdown`, or ends its own subscription)
//! does not wait for itself: `close` does not count passes held by the calling thread.

use std::cell::RefCell;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

pub(crate) struct Gate {
    state: Mutex<State>,
    idle: Condvar,
}

struct State {
    open: bool,
    inside: usize,
}

thread_local! {
    /// The gates this thread holds a pass for, by address, once per pass.
    static HELD: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

impl Gate {
    pub(crate) fn new() -> Arc<Gate> {
        Arc::new(Gate {
            state: Mutex::new(State {
                open: true,
                inside: 0,
            }),
            idle: Condvar::new(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn id(&self) -> usize {
        self as *const Gate as usize
    }

    /// A pass through the gate, if it is open. The pass is held until dropped.
    pub(crate) fn enter(self: &Arc<Self>) -> Option<Pass> {
        let mut s = self.lock();
        if !s.open {
            return None;
        }
        s.inside += 1;
        drop(s);
        HELD.with(|h| h.borrow_mut().push(self.id()));
        Some(Pass(self.clone()))
    }

    /// Close the gate, then wait until every pass held by another thread is dropped.
    pub(crate) fn close(&self) {
        let mine = HELD.with(|h| h.borrow().iter().filter(|g| **g == self.id()).count());
        let mut s = self.lock();
        s.open = false;
        while s.inside > mine {
            s = self.idle.wait(s).unwrap_or_else(|e| e.into_inner());
        }
    }

    #[cfg(test)]
    pub(crate) fn is_open(&self) -> bool {
        self.lock().open
    }
}

/// A pass through a [`Gate`].
pub(crate) struct Pass(Arc<Gate>);

impl Drop for Pass {
    fn drop(&mut self) {
        let id = self.0.id();
        HELD.with(|h| {
            let mut h = h.borrow_mut();
            if let Some(i) = h.iter().rposition(|g| *g == id) {
                h.swap_remove(i);
            }
        });
        let mut s = self.0.lock();
        s.inside -= 1;
        drop(s);
        self.0.idle.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn a_closed_gate_lets_nothing_through() {
        let g = Gate::new();
        assert!(g.enter().is_some());
        g.close();
        assert!(g.enter().is_none());
        assert!(!g.is_open());
    }

    #[test]
    fn close_waits_for_a_pass_held_elsewhere() {
        let g = Gate::new();
        let done = Arc::new(AtomicBool::new(false));
        let (g2, d2) = (g.clone(), done.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        let t = thread::spawn(move || {
            let _p = g2.enter().unwrap();
            tx.send(()).unwrap();
            thread::sleep(Duration::from_millis(150));
            d2.store(true, Ordering::SeqCst);
        });
        rx.recv().unwrap();
        g.close();
        assert!(
            done.load(Ordering::SeqCst),
            "close returned while a pass was held"
        );
        t.join().unwrap();
    }

    #[test]
    fn close_from_inside_does_not_wait_for_itself() {
        let g = Gate::new();
        let p = g.enter().unwrap();
        g.close();
        drop(p);
        assert!(g.enter().is_none());
    }
}
