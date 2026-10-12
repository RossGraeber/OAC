// SPDX-License-Identifier: Apache-2.0

//! Serializes event ordering and shutdown, allowing same-thread handler re-entry.
use std::sync::{Condvar, Mutex};
use std::thread::ThreadId;
#[derive(Default)]
pub(crate) struct Serial {
    state: Mutex<(Option<ThreadId>, usize)>,
    changed: Condvar,
}
impl Serial {
    pub(crate) fn enter(&self) -> Guard<'_> {
        let me = std::thread::current().id();
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        while s.0.is_some_and(|owner| owner != me) {
            s = self.changed.wait(s).unwrap_or_else(|e| e.into_inner());
        }
        s.0 = Some(me);
        s.1 += 1;
        Guard(self)
    }
}
pub(crate) struct Guard<'a>(&'a Serial);
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        let mut s = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        s.1 -= 1;
        if s.1 == 0 {
            s.0 = None;
            self.0.changed.notify_all();
        }
    }
}
