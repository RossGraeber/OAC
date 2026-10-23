// SPDX-License-Identifier: Apache-2.0

//! Which threads the carrier serves while a connection is being paired (`spec/bindings/mcp.md`
//! §4.5, [MCPB-ATT-018]; G8, #69).
//!
//! A reveal or a confirmation counts only when its `threadId` is a subscribed thread of the
//! carrier ([MCPB-ATT-018]); the client drops every other notification
//! ([`crate::client::Diagnostics::not_served`]). So before a pairing refusal goes out, the
//! threads that might be its caller are subscribed:
//!
//! 1. [`Serving::open_window`] asks the app-server which threads it has loaded
//!    (`thread/loaded/list`: stable, ids only; `thread/list` is never called, because it
//!    returns previews and the S1 capture found its `readStates` naming other saved threads),
//!    and subscribes each one the carrier does not serve yet, with `thread/resume` carrying
//!    only `threadId` and `excludeTurns` ([MCPB-CDX-006]). These are **tentative**: they
//!    stay subscribed until the window ends.
//! 2. A reveal makes the thread **held** by the connection it paired with
//!    ([`Serving::hold`]): it stays subscribed while any connection holds it
//!    ([`Serving::release`]).
//! 3. When the window ends, every tentative thread that no reveal held is unsubscribed
//!    ([`Serving::sweep`], run by a thread of its own at the window's end). A thread the
//!    caller of this crate subscribed on its own account (one launched OAC-enabled, per
//!    `[ADR-001-A2 Amendment]`) is never touched.
//!
//! Subscribing is not polling: it is done once per window per connection, when a connection
//! that is not paired makes its first call, and never per message.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::client::{ClientError, CodexClient};
use crate::threads::ThreadKey;

/// The most threads one window subscribes. A daemon with more loaded threads is served
/// partially: a caller among the rest cannot be paired (availability only).
pub const MAX_NEW_PER_WINDOW: usize = 64;

/// A time source, replaceable in tests.
pub type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// What one [`Serving::open_window`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Opened {
    /// Threads newly subscribed for this window.
    pub subscribed: usize,
    /// Threads the carrier serves now, whoever subscribed them.
    pub served: usize,
    /// Subscriptions that failed (the thread is not served).
    pub failed: usize,
}

struct State<K> {
    /// Subscribed for a window by [`Serving::open_window`]: until this instant.
    tentative: HashMap<ThreadKey, Instant>,
    /// Subscribed by [`Serving::open_window`] and held by these connections.
    held: HashMap<ThreadKey, HashSet<K>>,
    stop: bool,
}

struct Shared<K> {
    st: Mutex<State<K>>,
    wake: Condvar,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The thread-serving manager of one carrier.
pub struct Serving<K> {
    client: CodexClient,
    shared: Arc<Shared<K>>,
    window: Duration,
    clock: Clock,
    unsubscribed: Arc<Mutex<Vec<ThreadKey>>>,
}

impl<K: Clone + Eq + Hash + Send + 'static> Serving<K> {
    /// A manager over `client`, whose windows last `window`. With `thread` it starts the thread
    /// that ends windows ([`Serving::shutdown`] stops it); without, windows end only when
    /// [`Serving::sweep`] is called, for a caller that moves its own clock.
    pub fn new(client: CodexClient, window: Duration, clock: Clock, thread: bool) -> Serving<K> {
        let shared = Arc::new(Shared {
            st: Mutex::new(State {
                tentative: HashMap::new(),
                held: HashMap::new(),
                stop: false,
            }),
            wake: Condvar::new(),
        });
        let s = Serving {
            client,
            shared,
            window,
            clock,
            unsubscribed: Arc::default(),
        };
        if thread {
            s.spawn_sweeper();
        }
        s
    }

    fn spawn_sweeper(&self) {
        let shared = self.shared.clone();
        let client = self.client.clone();
        let clock = self.clock.clone();
        let log = self.unsubscribed.clone();
        let _ = std::thread::Builder::new()
            .name("oac-codex-serving".into())
            .spawn(move || {
                let mut st = lock(&shared.st);
                loop {
                    if st.stop {
                        return;
                    }
                    let now = clock();
                    let due = Self::take_due(&mut st, now);
                    if !due.is_empty() {
                        drop(st);
                        Self::unsubscribe(&client, &log, due);
                        st = lock(&shared.st);
                        continue;
                    }
                    // Wait for the next window's end, or for a new window or a stop. Not a
                    // poll: the wait is on the deadline and on the condition.
                    let wait = st
                        .tentative
                        .values()
                        .map(|d| d.saturating_duration_since(now))
                        .min()
                        .unwrap_or(Duration::from_secs(3600))
                        .max(Duration::from_millis(1));
                    st = shared
                        .wake
                        .wait_timeout(st, wait)
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
            });
    }

    /// Remove and return the tentative threads whose window has ended and that no connection
    /// holds.
    fn take_due(st: &mut State<K>, now: Instant) -> Vec<ThreadKey> {
        let due: Vec<ThreadKey> = st
            .tentative
            .iter()
            .filter(|(_, d)| **d <= now)
            .map(|(t, _)| *t)
            .collect();
        for t in &due {
            st.tentative.remove(t);
        }
        due.into_iter()
            .filter(|t| !st.held.contains_key(t))
            .collect()
    }

    fn unsubscribe(client: &CodexClient, log: &Mutex<Vec<ThreadKey>>, threads: Vec<ThreadKey>) {
        for t in threads {
            if client.unsubscribe(t).is_ok() {
                lock(log).push(t);
            }
        }
    }

    /// A connection that is not paired makes a call: subscribe the loaded threads the
    /// carrier does not serve, until `now` plus the window, before the refusal goes out.
    ///
    /// # Errors
    ///
    /// The `thread/loaded/list` call's error. Nothing is subscribed then.
    pub fn open_window(&self, now: Instant) -> Result<Opened, ClientError> {
        let loaded = self.client.loaded_threads()?;
        let deadline = now + self.window;
        let mut out = Opened::default();
        for id in loaded {
            match self.client.key_of(&id) {
                Some(key) => {
                    // Already served. If it is one this manager subscribed, the new window
                    // keeps it; a thread served on the caller's own account is not ours.
                    let mut st = lock(&self.shared.st);
                    if let Some(d) = st.tentative.get_mut(&key) {
                        *d = (*d).max(deadline);
                    }
                }
                None if out.subscribed >= MAX_NEW_PER_WINDOW => {}
                None => match self.client.subscribe(&id) {
                    Ok(key) => {
                        lock(&self.shared.st).tentative.insert(key, deadline);
                        out.subscribed += 1;
                    }
                    Err(_) => out.failed += 1,
                },
            }
        }
        out.served = self.client.threads().len();
        self.shared.wake.notify_all();
        Ok(out)
    }

    /// A reveal paired `thread` with connection `holder`: the thread stays subscribed while
    /// any connection holds it, whatever the window. A thread this manager did not subscribe
    /// is not tracked.
    pub fn hold(&self, holder: &K, thread: ThreadKey) {
        let mut st = lock(&self.shared.st);
        let ours = st.tentative.remove(&thread).is_some() || st.held.contains_key(&thread);
        if ours {
            st.held.entry(thread).or_default().insert(holder.clone());
        }
    }

    /// `holder` ended or was unbound: it holds no thread. A thread nobody holds any more is
    /// unsubscribed.
    pub fn release(&self, holder: &K) {
        let free: Vec<ThreadKey> = {
            let mut st = lock(&self.shared.st);
            let mut free = Vec::new();
            st.held.retain(|t, holders| {
                holders.remove(holder);
                if holders.is_empty() {
                    free.push(*t);
                    false
                } else {
                    true
                }
            });
            free
        };
        Self::unsubscribe(&self.client, &self.unsubscribed, free);
    }

    /// End every window that has ended at `now`: unsubscribe the tentative threads that no
    /// reveal held. The manager's own thread does this at the window's end; this is the same
    /// step, for a caller with its own clock.
    pub fn sweep(&self, now: Instant) {
        let due = Self::take_due(&mut lock(&self.shared.st), now);
        Self::unsubscribe(&self.client, &self.unsubscribed, due);
    }

    /// The threads this manager subscribed and has not released: tentative or held.
    pub fn managed(&self) -> usize {
        let st = lock(&self.shared.st);
        let held: HashSet<&ThreadKey> = st.held.keys().collect();
        st.tentative.keys().filter(|t| !held.contains(t)).count() + held.len()
    }

    /// How many threads this manager has unsubscribed.
    pub fn unsubscribed(&self) -> usize {
        lock(&self.unsubscribed).len()
    }

    /// Stop the window-ending thread. Subscriptions are left to the client's shutdown.
    pub fn shutdown(&self) {
        lock(&self.shared.st).stop = true;
        self.shared.wake.notify_all();
    }
}

impl<K> Drop for Serving<K> {
    fn drop(&mut self) {
        lock(&self.shared.st).stop = true;
        self.shared.wake.notify_all();
    }
}
