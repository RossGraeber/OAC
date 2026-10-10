// SPDX-License-Identifier: Apache-2.0

//! Run-wide observations at the raw transport boundary, before logical filtering.
use super::*;

pub(crate) struct Harness<'a> {
    pub inner: &'a dyn TransportHarness,
    media: Mutex<Vec<Arc<AuditedMedium>>>,
}
impl<'a> Harness<'a> {
    pub fn new(inner: &'a dyn TransportHarness) -> Self {
        Self {
            inner,
            media: Mutex::default(),
        }
    }
    pub fn sealing_observed(&self) -> bool {
        self.media
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.declarations.lock().unwrap().iter().any(|&s| s))
    }
    pub fn declaration(&self) -> Verdict {
        let media = self.media.lock().unwrap();
        let declarations: Vec<_> = media
            .iter()
            .flat_map(|m| m.declarations.lock().unwrap().clone())
            .collect();
        fail_if!(
            declarations.is_empty(),
            "no successful start declaration observed"
        );
        fail_if!(
            declarations
                .iter()
                .any(|&s| s != self.inner.binding_sealing()),
            "start sealing declaration disagrees with independently reviewed binding"
        );
        Verdict::Pass("every start agrees with binding sealing value".into())
    }
    pub fn shutdown(&self) -> Verdict {
        let n: usize = self
            .media
            .lock()
            .unwrap()
            .iter()
            .map(|m| m.late.load(Ordering::SeqCst))
            .sum();
        fail_if!(
            n > 0,
            "{n} raw handler invocation(s) after shutdown returned"
        );
        Verdict::Pass("no raw callback after shutdown, before recognition/opening".into())
    }
    pub fn observations(&self) -> Result<Vec<SealingObservation>, Verdict> {
        let mut all = Vec::new();
        for m in self.media.lock().unwrap().iter() {
            let declarations = m.declarations.lock().unwrap();
            if !self.inner.binding_sealing() && !declarations.iter().any(|&s| s) {
                continue;
            }
            let Some(obs) = m.inner.sealing_observations() else {
                return Err(Verdict::Fail(
                    "an exercised medium omitted raw carriage observations".into(),
                ));
            };
            let sent = m.sent.lock().unwrap();
            if sent.iter().any(|f| !obs.iter().any(|o| &o.frame == f)) {
                return Err(Verdict::Fail(
                    "capture omitted or changed an exercised taken frame".into(),
                ));
            }
            all.extend(obs);
        }
        Ok(all)
    }
}
impl TransportHarness for Harness<'_> {
    fn binding_sealing(&self) -> bool {
        self.inner.binding_sealing()
    }
    fn name(&self) -> String {
        self.inner.name()
    }
    fn medium(&self) -> Box<dyn Medium> {
        let m = Arc::new(AuditedMedium {
            inner: self.inner.medium(),
            declarations: Mutex::default(),
            sent: Arc::default(),
            late: Arc::default(),
        });
        self.media.lock().unwrap().push(m.clone());
        Box::new(m)
    }
}
struct AuditedMedium {
    inner: Box<dyn Medium>,
    declarations: Mutex<Vec<bool>>,
    sent: Arc<Mutex<Vec<Vec<u8>>>>,
    late: Arc<AtomicUsize>,
}
impl Medium for Arc<AuditedMedium> {
    fn transport(&self) -> Box<dyn Transport> {
        Box::new(Raw {
            inner: self.inner.transport(),
            medium: self.clone(),
            down: Mutex::new(Arc::default()),
        })
    }
    fn configuration(&self) -> TransportConfiguration {
        self.inner.configuration()
    }
    fn now(&self) -> Instant {
        self.inner.now()
    }
    fn advance(&self, by: Duration) {
        self.inner.advance(by);
    }
    fn settle(&self) {
        self.inner.settle();
    }
    fn subscribed(&self) {
        self.inner.subscribed();
    }
    fn faults(&self) -> Option<&dyn FaultControl> {
        self.inner.faults()
    }
    fn health_must_not_contain(&self) -> Vec<String> {
        self.inner.health_must_not_contain()
    }
    fn sealing_observations(&self) -> Option<Vec<SealingObservation>> {
        self.inner.sealing_observations()
    }
}
struct Raw {
    inner: Box<dyn Transport>,
    medium: Arc<AuditedMedium>,
    down: Mutex<Arc<AtomicBool>>,
}
impl Raw {
    fn record(&self, p: &Payload, r: PublishResult) -> PublishResult {
        if p.kind() == PayloadKind::Sealed && r == PublishResult::Taken {
            self.medium.sent.lock().unwrap().push(p.octets().to_vec());
        }
        r
    }
}
impl Transport for Raw {
    fn start(
        &self,
        k: &KeyId,
        c: TransportConfiguration,
    ) -> Result<TransportCapabilities, TransportError> {
        let caps = self.inner.start(k, c)?;
        self.medium.declarations.lock().unwrap().push(caps.sealing);
        let mut down = self.down.lock().unwrap();
        if down.load(Ordering::SeqCst) {
            *down = Arc::default();
        }
        Ok(caps)
    }
    fn publish(&self, d: &Destination, p: Payload, dl: Deadline) -> PublishResult {
        let r = self.inner.publish(d, p.clone(), dl);
        self.record(&p, r)
    }
    fn send_presence(&self, d: &Destination, p: Payload, dl: Deadline) -> PublishResult {
        let r = self.inner.send_presence(d, p.clone(), dl);
        self.record(&p, r)
    }
    fn subscribe(
        &self,
        d: &Destination,
        h: InboundHandler,
    ) -> Result<Subscription, TransportError> {
        let (down, late) = (self.down.lock().unwrap().clone(), self.medium.late.clone());
        self.inner.subscribe(
            d,
            Arc::new(move |i| {
                if down.load(Ordering::SeqCst) {
                    late.fetch_add(1, Ordering::SeqCst);
                }
                h(i);
            }),
        )
    }
    fn watch_presence(&self, h: PresenceHandler) -> Result<(), TransportError> {
        let (down, late) = (self.down.lock().unwrap().clone(), self.medium.late.clone());
        self.inner.watch_presence(Arc::new(move |e| {
            if down.load(Ordering::SeqCst) {
                late.fetch_add(1, Ordering::SeqCst);
            }
            h(e);
        }))
    }
    fn health(&self) -> HealthStatus {
        self.inner.health()
    }
    fn shutdown(&self) {
        self.inner.shutdown();
        self.down.lock().unwrap().store(true, Ordering::SeqCst);
    }
}
