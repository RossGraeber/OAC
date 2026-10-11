// SPDX-License-Identifier: Apache-2.0

//! Core ownership of callback admission and the adapter shutdown barrier.
use std::sync::{Arc, RwLock};

use crate::adapter::*;
use crate::health::HealthStatus;
use crate::ids::SessionId;
use crate::local_call::{CallAdmission, LocalResult};

pub(crate) struct AdapterBoundary {
    pub(crate) adapter: Arc<dyn ProviderAdapter>,
    pub(crate) admission: CallAdmission,
    callbacks: RwLock<()>,
}

impl AdapterBoundary {
    #[cfg(test)]
    pub(crate) fn new(adapter: Arc<dyn ProviderAdapter>) -> Self {
        Self::with_admission(adapter, CallAdmission::default())
    }

    pub(crate) fn with_admission(
        adapter: Arc<dyn ProviderAdapter>,
        admission: CallAdmission,
    ) -> Self {
        assert!(
            adapter.event_callback_bound_ms() > 0
                && adapter.stop_callback_bound_ms() > 0
                && adapter.stop_callback_bound_ms() <= adapter.event_callback_bound_ms()
                && (adapter.connection_limit() as u64)
                    .checked_mul(crate::connection::CLOSE_BOUND_MS)
                    .and_then(|close| close.checked_add(adapter.stop_callback_bound_ms()))
                    .is_some_and(|cost| cost < adapter.shutdown_bound_ms()),
            "local callback and closure costs exceed the adapter budget"
        );
        adapter.configure_local_calls(
            admission.clone(),
            crate::local_call::CORE_CALL_BOUND_MS,
            crate::local_call::STOP_CALL_BOUND_MS,
        );
        Self {
            adapter,
            admission,
            callbacks: RwLock::new(()),
        }
    }

    fn callback<T>(
        &self,
        attachment: &Attachment,
        run: impl FnOnce(&mut crate::local_call::CallContext) -> LocalResult<T>,
    ) -> LocalResult<T> {
        let mut call = self.admission.call(
            Some(attachment.clone()),
            self.adapter.event_callback_bound_ms(),
        );
        let guard = loop {
            if let Some(v) = call.cancellation() {
                return v.map(|()| unreachable!());
            }
            match self.callbacks.try_read() {
                Ok(g) => break g,
                Err(std::sync::TryLockError::Poisoned(e)) => break e.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(std::time::Duration::from_millis(1))
                }
            }
        };
        let value = run(&mut call);
        drop(guard);
        value
    }
}

impl ProviderAdapter for AdapterBoundary {
    fn local_calls(&self) -> bool {
        true
    }
    fn capabilities_local(
        &self,
        _call: &mut crate::local_call::CallContext,
        attachment: &Attachment,
    ) -> LocalResult<AdapterCapabilities> {
        let result = self.callback(attachment, |call| {
            self.adapter.capabilities_local(call, attachment)
        });
        if !matches!(result, LocalResult::Completed(_)) {
            attachment.begin_close();
        }
        result
    }
    fn deliver_local(
        &self,
        _call: &mut crate::local_call::CallContext,
        hand_off: HandOff,
    ) -> LocalResult<HandOffOutcome> {
        let attachment = hand_off.attachment().clone();
        self.callback(&attachment, |call| {
            self.adapter.deliver_local(call, hand_off)
        })
    }
    fn connection_limit(&self) -> usize {
        self.adapter.connection_limit()
    }
    fn shutdown_bound_ms(&self) -> u64 {
        self.adapter.shutdown_bound_ms()
    }
    fn take_connection(&self, connection: Connection) {
        self.adapter.take_connection(connection);
    }
    fn watch_attachments(&self, handler: AdapterEventHandler) {
        self.adapter.watch_attachments(handler);
    }
    fn watch_local_attachments(&self, handler: LocalEventHandler) {
        self.adapter.watch_local_attachments(handler);
    }
    fn accept_requests(&self, sink: Arc<dyn RequestSink>) {
        self.adapter.accept_requests(sink);
    }
    fn health(&self) -> HealthStatus {
        self.adapter.health()
    }
    fn set_binding(&self, attachment: &Attachment, session: Option<SessionId>) {
        if !matches!(
            self.callback(attachment, |call| self
                .adapter
                .set_binding_local(call, attachment, session)),
            LocalResult::Completed(())
        ) {
            attachment.begin_close();
        }
    }
    fn capabilities(&self, attachment: &Attachment) -> AdapterCapabilities {
        match self.callback(attachment, |call| {
            self.adapter.capabilities_local(call, attachment)
        }) {
            LocalResult::Completed(v) => v,
            _ => {
                attachment.begin_close();
                panic!("the bounded core consumes capability failures through capabilities_local")
            }
        }
    }
    fn deliver(&self, hand_off: HandOff) -> HandOffOutcome {
        let attachment = hand_off.attachment().clone();
        match self.callback(&attachment, |call| {
            self.adapter.deliver_local(call, hand_off)
        }) {
            LocalResult::Completed(v) => v,
            _ => HandOffOutcome::Failed,
        }
    }
    fn shutdown(&self) {
        self.admission.stop();
        self.adapter.shutdown();
        drop(self.callbacks.write().unwrap_or_else(|e| e.into_inner()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, mpsc};
    use std::time::{Duration, Instant};
    struct Probe {
        blocker: Arc<Mutex<()>>,
        entered: mpsc::Sender<()>,
        settled: mpsc::Sender<LocalResult<()>>,
        calls: AtomicUsize,
    }
    impl ProviderAdapter for Probe {
        fn connection_limit(&self) -> usize {
            4
        }
        fn take_connection(&self, _: Connection) {}
        fn watch_attachments(&self, _: AdapterEventHandler) {}
        fn set_binding(&self, _: &Attachment, _: Option<SessionId>) {}
        fn capabilities(&self, _: &Attachment) -> AdapterCapabilities {
            panic!("legacy callback")
        }
        fn deliver(&self, _: HandOff) -> HandOffOutcome {
            HandOffOutcome::Failed
        }
        fn accept_requests(&self, _: Arc<dyn RequestSink>) {}
        fn health(&self) -> HealthStatus {
            HealthStatus::new(crate::health::HealthState::Healthy)
        }
        fn shutdown(&self) {}
        fn capabilities_local(
            &self,
            call: &mut crate::local_call::CallContext,
            _: &Attachment,
        ) -> LocalResult<AdapterCapabilities> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.send(()).unwrap();
            let result = match call.lock(&self.blocker) {
                Ok(_) => panic!("unexpected release"),
                Err(v) => v,
            };
            self.settled.send(result.clone()).unwrap();
            result.map(|()| unreachable!())
        }
    }

    #[test]
    fn blocked_callback_settles_before_shutdown_and_no_callback_is_invoked_after_return() {
        let blocker = Arc::new(Mutex::new(()));
        let held = blocker.lock().unwrap();
        let (entered, entered_rx) = mpsc::channel();
        let (settled, settled_rx) = mpsc::channel();
        let probe = Arc::new(Probe {
            blocker: blocker.clone(),
            entered,
            settled,
            calls: AtomicUsize::new(0),
        });
        let boundary = Arc::new(AdapterBoundary::new(probe.clone()));
        let connection = Connection::accept(
            crate::connection::ConnectionReader::default(),
            crate::connection::ConnectionWriter::default(),
        );
        let attachment = connection.handle().clone();
        let worker_boundary = boundary.clone();
        let a = attachment.clone();
        let worker = std::thread::spawn(move || {
            let mut call = CallAdmission::default().call(Some(a.clone()), 1000);
            worker_boundary.capabilities_local(&mut call, &a)
        });
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let start = Instant::now();
        boundary.shutdown();
        assert!(start.elapsed() < Duration::from_millis(1000));
        assert_eq!(settled_rx.try_recv().unwrap(), LocalResult::ShuttingDown);
        assert!(matches!(worker.join().unwrap(), LocalResult::ShuttingDown));
        drop(held);
        let mut call = CallAdmission::default().call(Some(attachment.clone()), 1000);
        assert!(!matches!(
            boundary.capabilities_local(&mut call, &attachment),
            LocalResult::Completed(_)
        ));
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
        assert!(settled_rx.try_recv().is_err());
    }

    #[test]
    fn ordinary_callback_timeout_has_no_completed_capability_value() {
        let blocker = Arc::new(Mutex::new(()));
        let held = blocker.lock().unwrap();
        let (entered, _entered_rx) = mpsc::channel();
        let (settled, settled_rx) = mpsc::channel();
        let probe = Probe {
            blocker: blocker.clone(),
            entered,
            settled,
            calls: AtomicUsize::new(0),
        };
        let connection = Connection::accept(
            crate::connection::ConnectionReader::default(),
            crate::connection::ConnectionWriter::default(),
        );
        let mut call = CallAdmission::default().call(Some(connection.handle().clone()), 10);
        assert!(matches!(
            probe.capabilities_local(&mut call, connection.handle()),
            LocalResult::TimedOut
        ));
        assert_eq!(settled_rx.try_recv().unwrap(), LocalResult::TimedOut);
        drop(held);
    }
}
