// SPDX-License-Identifier: Apache-2.0

//! Local cancellation and request commit, interfaces 0.6 section 5.3.1.
//! Callees cooperate at lock acquisition and commit; a waiter is never detached.

use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

use crate::adapter::Attachment;

/// A terminal local outcome has no request or callback value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalResult<T> {
    /// The operation's unchanged value.
    Completed(T),
    /// The associated connection began closing before commit.
    Closed,
    /// Adapter shutdown began before commit.
    ShuttingDown,
    /// The ordinary local budget expired before commit.
    TimedOut,
}

impl<T> LocalResult<T> {
    /// Change only the completed value.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> LocalResult<U> {
        match self {
            Self::Completed(v) => LocalResult::Completed(f(v)),
            Self::Closed => LocalResult::Closed,
            Self::ShuttingDown => LocalResult::ShuttingDown,
            Self::TimedOut => LocalResult::TimedOut,
        }
    }
}

/// Shared, independently observable adapter admission state.
#[derive(Default)]
struct AdmissionState {
    stopped: Option<Instant>,
    pending: usize,
}

/// Maximum concurrent calls per adapter; final closure has reserved admission.
pub const MAX_PENDING_CALLS: usize = 64;
/// Core-owned ordinary call budget, supplied before calls are accepted.
pub const CORE_CALL_BOUND_MS: u64 = 1000;
/// Core-owned stop/final-report budget; no greater than the ordinary budget.
pub const STOP_CALL_BOUND_MS: u64 = 100;

#[derive(Clone, Default)]
pub struct CallAdmission(Arc<Mutex<AdmissionState>>);

impl CallAdmission {
    /// Begin shutdown without acquiring a lock held by executing work.
    pub fn stop(&self) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stopped
            .get_or_insert_with(Instant::now);
    }

    /// Whether ordinary new work must be refused.
    pub fn is_stopping(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stopped
            .is_some()
    }

    /// Enter an ordinary call; the budget includes acquiring the callee's locks.
    pub fn call(&self, attachment: Option<Attachment>, bound_ms: u64) -> CallContext {
        assert!(bound_ms > 0);
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let admitted = state.pending < MAX_PENDING_CALLS;
        if admitted {
            state.pending += 1;
        }
        CallContext {
            admission: self.clone(),
            attachment,
            deadline: Instant::now() + Duration::from_millis(bound_ms),
            committed: false,
            terminal: (!admitted).then_some(LocalResult::TimedOut),
            final_report: false,
            admitted,
        }
    }
}

/// Per-invocation commit state. A committed publication stays on this stack until
/// ownership settles, even with an undeclared transport bound (IFC-ADP-117).
pub struct CallContext {
    admission: CallAdmission,
    attachment: Option<Attachment>,
    deadline: Instant,
    committed: bool,
    terminal: Option<LocalResult<()>>,
    final_report: bool,
    admitted: bool,
}

impl CallContext {
    /// The core binds a supplied invocation to its own admission state and the actual
    /// request attachment. A caller cannot replace shutdown with a fresh live scope.
    pub(crate) fn associate_with(&mut self, admission: &CallAdmission, attachment: Attachment) {
        self.bind_scope(admission);
        self.attachment = Some(attachment);
    }

    pub(crate) fn bind_scope(&mut self, admission: &CallAdmission) {
        assert!(!self.committed);
        let _ = self.cancellation();
        if !Arc::ptr_eq(&self.admission.0, &admission.0) {
            if self.admitted {
                self.admission
                    .0
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .pending -= 1;
            }
            self.admission = admission.clone();
            let mut state = admission.0.lock().unwrap_or_else(|e| e.into_inner());
            self.admitted = state.pending < MAX_PENDING_CALLS;
            if self.admitted {
                state.pending += 1;
            } else {
                self.terminal = Some(LocalResult::TimedOut);
            }
        }
        self.deadline = self
            .deadline
            .min(Instant::now() + Duration::from_millis(CORE_CALL_BOUND_MS));
    }
    /// Final closure is admitted after stop and connection closure.
    pub fn final_report(mut self) -> Self {
        self.final_report = true;
        // Reserved reporting capacity is bounded by the admitted connection count.
        self.terminal = None;
        self
    }

    /// Cancellation is sticky. No terminal outcome can replace a committed result.
    pub fn cancellation(&mut self) -> Option<LocalResult<()>> {
        if self.committed {
            return None;
        }
        if let Some(v) = &self.terminal {
            return Some(v.clone());
        }
        let stopped = self
            .admission
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stopped;
        let closed = self.attachment.as_ref().and_then(Attachment::closed_at);
        let mut transitions = Vec::with_capacity(3);
        if !self.final_report {
            if let Some(at) = closed {
                transitions.push((at, LocalResult::Closed));
            }
            if let Some(at) = stopped {
                transitions.push((at, LocalResult::ShuttingDown));
            }
        }
        if Instant::now() >= self.deadline {
            transitions.push((self.deadline, LocalResult::TimedOut));
        }
        let value = transitions
            .into_iter()
            .min_by_key(|(at, _)| *at)
            .map(|(_, value)| value);
        self.terminal = value.clone();
        value
    }

    /// Acquire a local lock cooperatively, without leaving blocked work behind.
    pub fn lock<'a, T>(
        &mut self,
        mutex: &'a Mutex<T>,
    ) -> Result<MutexGuard<'a, T>, LocalResult<()>> {
        loop {
            if let Some(v) = self.cancellation() {
                return Err(v);
            }
            match mutex.try_lock() {
                Ok(v) => return Ok(v),
                Err(TryLockError::Poisoned(v)) => return Ok(v.into_inner()),
                Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(1)),
            }
        }
    }

    /// Select a snapshot/refusal, or create and record an envelope, atomically against
    /// shutdown. The closure must contain only finite local work, never publication.
    pub fn commit<T>(&mut self, select: impl FnOnce() -> T) -> LocalResult<T> {
        assert!(!self.committed, "one commit per local invocation");
        if let Some(v) = self.cancellation() {
            return v.map(|()| unreachable!());
        }
        let stopped = self.admission.0.lock().unwrap_or_else(|e| e.into_inner());
        let mut transitions = Vec::with_capacity(3);
        if !self.final_report {
            if let Some(at) = stopped.stopped {
                transitions.push((at, LocalResult::ShuttingDown));
            }
            if let Some(at) = self.attachment.as_ref().and_then(Attachment::closed_at) {
                transitions.push((at, LocalResult::Closed));
            }
        }
        if Instant::now() >= self.deadline {
            transitions.push((self.deadline, LocalResult::TimedOut));
        }
        if let Some((_, result)) = transitions.into_iter().min_by_key(|(at, _)| *at) {
            self.terminal = Some(result.clone());
            return result.map(|()| unreachable!());
        }
        let value = if self.final_report {
            select()
        } else if let Some(attachment) = &self.attachment {
            match attachment.if_open(select) {
                Some(v) => v,
                None => return LocalResult::Closed,
            }
        } else {
            select()
        };
        self.committed = true;
        LocalResult::Completed(value)
    }
}

impl Drop for CallContext {
    fn drop(&mut self) {
        if self.admitted {
            self.admission
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pending -= 1;
        }
    }
}

/// Local composition report, distinct from request results and peer receipts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShutdownGuarantee {
    /// Adapter-owned coordination/closure/callback budget.
    pub adapter_ms: u64,
    /// Aggregate local core settlement and reporting budget.
    pub core_ms: u64,
    /// Per-publication transport contribution; None explicitly means unbounded.
    pub publish_ms: Option<u64>,
    /// Maximum number of awaited sequential publications.
    pub max_publications: u64,
}

impl ShutdownGuarantee {
    /// None means an awaited publication has no declared bound.
    pub fn composed_ms(&self) -> Option<u64> {
        if self.max_publications == 0 {
            return self.adapter_ms.checked_add(self.core_ms);
        }
        self.publish_ms?
            .checked_mul(self.max_publications)?
            .checked_add(self.core_ms)?
            .checked_add(self.adapter_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::Connection;

    #[test]
    fn blocked_lock_cancels_without_a_worker_or_late_commit() {
        let admission = CallAdmission::default();
        let mutex = Arc::new(Mutex::new(()));
        let held = mutex.lock().unwrap();
        let blocker = mutex.clone();
        let scope = admission.clone();
        let worker = std::thread::spawn(move || {
            let mut call = scope.call(None, 1000);
            let result = call.lock(&blocker);
            assert!(matches!(result, Err(LocalResult::ShuttingDown)));
        });
        admission.stop();
        worker.join().unwrap();
        drop(held);
    }

    #[test]
    fn commit_and_shutdown_races_have_exactly_one_truthful_value() {
        for _ in 0..100 {
            let admission = CallAdmission::default();
            let scope = admission.clone();
            let selected = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let count = selected.clone();
            let worker = std::thread::spawn(move || {
                let mut call = scope.call(None, 1000);
                let result = call.commit(|| {
                    count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    42
                });
                if matches!(result, LocalResult::Completed(_)) {
                    assert_eq!(call.cancellation(), None);
                }
                result
            });
            admission.stop();
            let result = worker.join().unwrap();
            match result {
                LocalResult::Completed(42) => {
                    assert_eq!(selected.load(std::sync::atomic::Ordering::SeqCst), 1)
                }
                LocalResult::ShuttingDown => {
                    assert_eq!(selected.load(std::sync::atomic::Ordering::SeqCst), 0)
                }
                v => panic!("unexpected race outcome: {v:?}"),
            }
        }
    }

    #[test]
    fn closure_commit_and_first_cancellation_transition_are_preserved() {
        let admission = CallAdmission::default();
        let c = Connection::accept(
            crate::connection::ConnectionReader::default(),
            crate::connection::ConnectionWriter::default(),
        );
        let h = c.handle();
        let mut call = admission.call(Some(h.clone()), 1000);
        assert_eq!(
            call.commit(|| "snapshot"),
            LocalResult::Completed("snapshot")
        );
        h.close();
        admission.stop();
        assert_eq!(call.cancellation(), None);
        let mut cancelled = CallAdmission::default().call(Some(h.clone()), 1000);
        assert_eq!(
            cancelled.commit(|| panic!("late selection")),
            LocalResult::<()>::Closed
        );
        let admission = CallAdmission::default();
        let c = Connection::accept(
            crate::connection::ConnectionReader::default(),
            crate::connection::ConnectionWriter::default(),
        );
        let h = c.handle();
        let mut call = admission.call(Some(h.clone()), 1000);
        admission.stop();
        h.close();
        assert_eq!(call.cancellation(), Some(LocalResult::ShuttingDown));
    }

    #[test]
    fn ordinary_timeout_and_connection_scoping_and_reserved_final_reports() {
        let scope = CallAdmission::default();
        let a = Connection::accept(
            crate::connection::ConnectionReader::default(),
            crate::connection::ConnectionWriter::default(),
        );
        let b = Connection::accept(
            crate::connection::ConnectionReader::default(),
            crate::connection::ConnectionWriter::default(),
        );
        let mut first = scope.call(Some(a.handle().clone()), 1000);
        let mut other = scope.call(Some(b.handle().clone()), 1000);
        a.handle().close();
        assert_eq!(first.cancellation(), Some(LocalResult::Closed));
        assert_eq!(other.commit(|| 7), LocalResult::Completed(7));
        let mut expired = scope.call(None, 1);
        std::thread::sleep(Duration::from_millis(2));
        scope.stop();
        assert_eq!(expired.cancellation(), Some(LocalResult::TimedOut));
        let mut final_report = scope.call(Some(a.handle().clone()), 100).final_report();
        assert_eq!(final_report.commit(|| ()), LocalResult::Completed(()));
    }

    #[test]
    fn undeclared_publication_is_explicitly_unbounded_and_limits_are_finite() {
        let mut guarantee = ShutdownGuarantee {
            adapter_ms: 1000,
            core_ms: 6800,
            publish_ms: None,
            max_publications: 128,
        };
        assert_eq!(guarantee.composed_ms(), None);
        guarantee.publish_ms = Some(10);
        assert_eq!(guarantee.composed_ms(), Some(9080));
        guarantee.max_publications = 0;
        guarantee.publish_ms = None;
        assert_eq!(guarantee.composed_ms(), Some(7800));
        let admission = CallAdmission::default();
        let calls: Vec<_> = (0..MAX_PENDING_CALLS)
            .map(|_| admission.call(None, 1000))
            .collect();
        assert_eq!(
            admission.call(None, 1000).cancellation(),
            Some(LocalResult::TimedOut)
        );
        drop(calls);
        assert_eq!(admission.call(None, 1000).cancellation(), None);
    }
}
