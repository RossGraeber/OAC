// SPDX-License-Identifier: Apache-2.0

//! Suite-side core simulation. The implementation under test sees only real HPKE frames.
use super::*;
use std::collections::HashSet;

/// Exact frames passed by the suite, shared by endpoints of one isolated world.
pub(crate) type FrameBook = Arc<Mutex<HashSet<Vec<u8>>>>;
use oac_core::sealing::{self, AgreementPrivateKey, AgreementPublicKey};

pub(crate) const LOGICAL_OVERHEAD: u64 = sealing::FRAME_OVERHEAD as u64 + 100;
pub(crate) const REQUIRED: &[&str] = &[
    "IFC-TRN-104",
    "IFC-TRN-105",
    "IFC-TRN-107",
    "IFC-TRN-108",
    "IFC-TRN-109",
    "IFC-TRN-113",
    "IFC-TRN-110",
];

// RFC 7748 Alice/Bob test key pairs, exclusively for synthetic suite payloads.
// Sealing hides content, never authenticates: authenticated peer messages remain
// untrusted instructions; these synthetic octets do not claim signature validity.
fn hex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
        .collect()
}
fn public(k: &KeyId) -> AgreementPublicKey {
    let value = if *k == key(2) {
        "de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f"
    } else {
        "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
    };
    AgreementPublicKey::from_octets(hex(value).try_into().unwrap()).unwrap()
}
fn private(k: &KeyId) -> AgreementPrivateKey {
    let value = if *k == key(2) {
        "5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb"
    } else {
        "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a"
    };
    AgreementPrivateKey::from_octets(&mut hex(value).try_into().unwrap())
}
fn frame(k: &KeyId, kind: PayloadKind, bytes: &[u8], cap: u64) -> Payload {
    Payload::new(
        PayloadKind::Sealed,
        sealing::seal(&public(k), kind, bytes, cap).unwrap(),
    )
}

pub(crate) fn logical(
    raw: Box<dyn Transport>,
    local: &KeyId,
    caps: TransportCapabilities,
    frames: FrameBook,
    medium: Arc<dyn Medium>,
    epoch: Instant,
) -> Box<dyn Transport> {
    if !caps.sealing {
        return raw;
    }
    Box::new(Logical {
        raw,
        local: local.clone(),
        caps,
        watches: Mutex::new(Vec::new()),
        frames,
        epoch,
        medium,
    })
}
struct Logical {
    raw: Box<dyn Transport>,
    local: KeyId,
    caps: TransportCapabilities,
    watches: Mutex<Vec<Subscription>>,
    frames: FrameBook,
    epoch: Instant,
    medium: Arc<dyn Medium>,
}
fn opened(
    k: &KeyId,
    i: &Inbound,
    d: &Destination,
    presence: bool,
    epoch: Instant,
    now: Instant,
) -> Option<Payload> {
    if i.payload.kind() != PayloadKind::Sealed {
        return None;
    }
    let o = sealing::open(&private(k), i.payload.octets())?;
    if o.payload.len() < 100 || (o.kind == PayloadKind::Presence) != presence {
        return None;
    }
    let label = format!("{d:?}");
    // The receiver's core owns expiry (SEC-SEL-036); no deadline rides beside the frame.
    let deadline = u64::from_be_bytes(o.payload[92..100].try_into().ok()?);
    if now.saturating_duration_since(epoch).as_nanos() >= u128::from(deadline) {
        return None;
    }
    let end = o.payload[..92].iter().position(|&b| b == 0)?;
    if &o.payload[..end] != label.as_bytes() {
        return None;
    }
    Some(Payload::new(o.kind, o.payload[100..].to_vec()))
}
impl Transport for Logical {
    fn start(
        &self,
        k: &KeyId,
        c: TransportConfiguration,
    ) -> Result<TransportCapabilities, TransportError> {
        self.raw.start(k, c)
    }
    fn publish(&self, d: &Destination, p: Payload, dl: Deadline) -> PublishResult {
        self.medium.audit_destination(d);
        let target = match d {
            Destination::Device(k) => k.clone(),
            Destination::Session(_) => key(if self.caps.reach == Reach::CrossImplementation {
                2
            } else {
                1
            }),
        };
        let mut bytes = vec![0; 100];
        let label = format!("{d:?}");
        bytes[..label.len()].copy_from_slice(label.as_bytes());
        let deadline = u64::try_from(
            dl.instant()
                .saturating_duration_since(self.epoch)
                .as_nanos(),
        )
        .unwrap();
        bytes[92..100].copy_from_slice(&deadline.to_be_bytes());
        bytes.extend_from_slice(p.octets());
        let Ok(f) = sealing::seal(
            &public(&target),
            p.kind(),
            &bytes,
            self.caps.max_payload_octets,
        ) else {
            return PublishResult::NotTaken;
        };
        self.frames.lock().unwrap().insert(f.clone());
        self.raw.publish(
            &Destination::Device(target),
            Payload::new(PayloadKind::Sealed, f),
            dl,
        )
    }
    fn subscribe(
        &self,
        d: &Destination,
        h: InboundHandler,
    ) -> Result<Subscription, TransportError> {
        self.medium.audit_destination(d);
        let d = d.clone();
        let k = self.local.clone();
        let frames = self.frames.clone();
        let (epoch, medium) = (self.epoch, self.medium.clone());
        self.raw.subscribe(
            &Destination::Device(self.local.clone()),
            Arc::new(move |mut i| {
                if !frames.lock().unwrap().contains(i.payload.octets()) {
                    return;
                }
                if let Some(p) = opened(&k, &i, &d, false, epoch, medium.now()) {
                    i.payload = p;
                    h(i);
                }
            }),
        )
    }
    fn send_presence(&self, d: &Destination, p: Payload, dl: Deadline) -> PublishResult {
        self.publish(d, p, dl)
    }
    fn watch_presence(&self, h: PresenceHandler) -> Result<(), TransportError> {
        let d = Destination::Device(self.local.clone());
        let h2 = h.clone();
        let k = self.local.clone();
        let frames = self.frames.clone();
        let (epoch, medium) = (self.epoch, self.medium.clone());
        let s = self.raw.subscribe(
            &d.clone(),
            Arc::new(move |i| {
                if !frames.lock().unwrap().contains(i.payload.octets()) {
                    return;
                }
                if let Some(payload) = opened(&k, &i, &d, true, epoch, medium.now()) {
                    h2(PresenceEvent::Record {
                        payload,
                        carrier: i.carrier,
                    });
                }
            }),
        )?;
        self.watches.lock().unwrap().push(s);
        self.raw.watch_presence(Arc::new(move |e| {
            if matches!(e, PresenceEvent::CarrierLoss { .. }) {
                h(e);
            }
        }))
    }
    fn health(&self) -> HealthStatus {
        self.raw.health()
    }
    fn shutdown(&self) {
        self.raw.shutdown();
    }
}

fn raw(h: &dyn TransportHarness) -> (Box<dyn Medium>, Box<dyn Transport>, TransportCapabilities) {
    let m = h.medium();
    let t = m.transport();
    let c = t.start(&key(1), m.configuration()).unwrap();
    (m, t, c)
}
fn absent() -> Verdict {
    Verdict::Pass("sealing absent; plain contract path unchanged".into())
}

/// Keep the sender obligation observable even though the simulated receiving core
/// drops expired opened payloads. This check observes raw frames, not Logical.
pub(crate) fn sender_expiry(h: &dyn TransportHarness) -> Verdict {
    let (m, t, c) = raw(h);
    if !c.sealing {
        return absent();
    }
    let d = Destination::Device(key(1));
    let seen = Seen::default();
    let _s = t.subscribe(&d, seen.handler()).unwrap();
    m.subscribed();
    let at = m.now();
    let p = frame(
        &key(1),
        PayloadKind::Envelope,
        b"expired-sender-control",
        c.max_payload_octets,
    );
    t.publish(&d, p, Deadline::at(at));
    m.settle();
    m.advance(Duration::from_secs(3));
    m.settle();
    fail_if!(
        seen.len() != 0,
        "sender delivered an already-expired raw frame"
    );
    if let Some(f) = m.faults()
        && f.next_sender_hold(Duration::from_secs(2))
    {
        let p = frame(
            &key(1),
            PayloadKind::Envelope,
            b"sender-held-control",
            c.max_payload_octets,
        );
        fail_if!(
            t.publish(&d, p, Deadline::at(m.now() + Duration::from_secs(1)))
                != PublishResult::Taken,
            "sender hold control refused"
        );
        m.advance(Duration::from_secs(3));
        m.settle();
        fail_if!(
            seen.len() != 0,
            "sender emitted a frame retained past its deadline"
        );
        return Verdict::Pass(
            "expired input and sender-retained frame never delivered at raw boundary".into(),
        );
    }
    Verdict::Pass(
        "sender never emitted already-expired input; sender holding unsupported/not exercised; receiving-core filtering bypassed".into(),
    )
}

pub(crate) fn reject_plain(h: &dyn TransportHarness) -> Verdict {
    let (m, t, c) = raw(h);
    if !c.sealing {
        return absent();
    }
    let seen = Seen::default();
    let prs = Seen::default();
    let _s = t
        .subscribe(&Destination::Device(key(1)), seen.handler())
        .unwrap();
    t.watch_presence(prs.watcher()).unwrap();
    m.subscribed();
    for kind in [
        PayloadKind::Envelope,
        PayloadKind::Receipt,
        PayloadKind::Presence,
    ] {
        for d in [session(1), Destination::Device(key(1))] {
            for presence in [false, true] {
                let p = payload(kind, b"plain-must-not-escape");
                let dl = Deadline::at(m.now() + LONG);
                let r = if presence {
                    t.send_presence(&d, p, dl)
                } else {
                    t.publish(&d, p, dl)
                };
                fail_if!(
                    r != PublishResult::NotTaken,
                    "{kind:?} via {} to {d:?} returned {r:?}",
                    if presence { "send_presence" } else { "publish" }
                );
            }
        }
    }
    m.settle();
    fail_if!(
        seen.len() > 0 || !prs.records().is_empty(),
        "a refused plain payload was delivered"
    );
    Verdict::Pass("every plain kind refused by both operations, no delivery".into())
}

pub(crate) fn floor(h: &dyn TransportHarness) -> Verdict {
    let (_, _, c) = raw(h);
    if !c.sealing {
        return absent();
    }
    fail_if!(c.max_payload_octets < 65590, "sealing cap below 65590");
    Verdict::Pass("sealing floor 65590 met".into())
}

pub(crate) fn frame_cap(h: &dyn TransportHarness) -> Verdict {
    let (m, t, c) = raw(h);
    if !c.sealing {
        return absent();
    }
    let cap = usize::try_from(c.max_payload_octets).unwrap();
    fail_if!(
        cap < sealing::FRAME_OVERHEAD,
        "cap cannot hold frame overhead"
    );
    let d = Destination::Device(key(1));
    let seen = Seen::default();
    let _s = t.subscribe(&d, seen.handler()).unwrap();
    m.subscribed();
    let exact = frame(
        &key(1),
        PayloadKind::Envelope,
        &vec![42; cap - sealing::FRAME_OVERHEAD],
        c.max_payload_octets,
    );
    fail_if!(exact.len() != cap, "core frame did not reach cap");
    let dl = Deadline::at(m.now() + LONG);
    fail_if!(
        t.publish(&d, exact.clone(), dl) != PublishResult::Taken,
        "frame at cap refused"
    );
    for _ in 0..20 {
        if seen.len() >= 1 {
            break;
        }
        m.settle();
    }
    m.settle();
    fail_if!(
        seen.get().iter().map(|i| &i.payload).collect::<Vec<_>>() != vec![&exact],
        "frame at cap changed or was not delivered"
    );
    let over = frame(
        &key(1),
        PayloadKind::Envelope,
        &vec![43; cap + 1 - sealing::FRAME_OVERHEAD],
        c.max_payload_octets + 1,
    );
    fail_if!(
        t.publish(&d, over, dl) != PublishResult::NotTaken,
        "frame one octet over cap accepted"
    );
    m.settle();
    fail_if!(seen.len() != 1, "over-cap frame delivered despite refusal");
    Verdict::Pass("54-octet overhead: exact cap carried unchanged, cap+1 refused".into())
}

fn observations(h: &dyn TransportHarness) -> Result<Vec<SealingObservation>, Verdict> {
    let (m, t, c) = raw(h);
    m.audit_destination(&session(7));
    if !c.sealing {
        return Err(absent());
    }
    let receiver = m.transport();
    let target = if c.reach == Reach::CrossImplementation {
        key(2)
    } else {
        key(1)
    };
    let r: &dyn Transport = if c.reach == Reach::CrossImplementation {
        receiver.start(&target, m.configuration()).unwrap();
        &*receiver
    } else {
        &*t
    };
    let seen = Seen::default();
    let _s = r
        .subscribe(&Destination::Device(target.clone()), seen.handler())
        .unwrap();
    m.subscribed();
    let mut sent = Vec::new();
    for (n, kind) in [
        PayloadKind::Envelope,
        PayloadKind::Receipt,
        PayloadKind::Presence,
    ]
    .into_iter()
    .enumerate()
    {
        let p = frame(
            &target,
            kind,
            format!("{:?} {:?} {}", session(7), key(1), kind.as_str()).as_bytes(),
            c.max_payload_octets,
        );
        let r = t.publish(
            &Destination::Device(target.clone()),
            p.clone(),
            Deadline::at(m.now() + Duration::from_secs(10 + n as u64)),
        );
        if r != PublishResult::Taken {
            return Err(Verdict::Fail("observation control frame refused".into()));
        }
        sent.push(p.octets().to_vec());
    }
    for _ in 0..20 {
        if seen.len() >= 3 {
            break;
        }
        m.settle();
    }
    t.shutdown();
    m.settle();
    let Some(obs) = m.sealing_observations() else {
        return Err(Verdict::Fail(
            "sealing harness omitted raw carriage observations".into(),
        ));
    };
    if sent.iter().any(|f| !obs.iter().any(|o| &o.frame == f)) {
        return Err(Verdict::Fail(
            "observations omitted or changed a control frame".into(),
        ));
    }
    Ok(obs)
}
pub(crate) fn carriage(h: &dyn TransportHarness) -> Verdict {
    let obs = match observations(h) {
        Ok(v) => v,
        Err(v) => return v,
    };
    check_carriage(&obs)
}
pub(crate) fn check_carriage(obs: &[SealingObservation]) -> Verdict {
    fail_if!(
        obs.iter().any(|o| !o.accompanying.is_empty()),
        "a frame carried an accompanying application value"
    );
    Verdict::Pass("observed frames have no kind/destination/deadline side values".into())
}
pub(crate) fn identifiers(h: &dyn TransportHarness) -> Verdict {
    let obs = match observations(h) {
        Ok(v) => v,
        Err(v) => return v,
    };
    let needles: Vec<_> = [
        Destination::Device(key(1)),
        Destination::Device(key(2)),
        session(7),
    ]
    .iter()
    .flat_map(identifier_needles)
    .collect();
    check_identifiers(&obs, &needles)
}
pub(crate) fn identifier_needles(d: &Destination) -> Vec<Vec<u8>> {
    let (text, octets) = match d {
        Destination::Device(k) => (k.as_str(), hex(k.as_str())),
        Destination::Session(s) => {
            // Canonical SessionId is Crockford base32 encoding of 128 bits,
            // with two leading zero bits (SC-ID-003). Decode the actual id, not a sample.
            let alphabet = b"0123456789abcdefghjkmnpqrstvwxyz";
            let value = s.as_str().bytes().fold(0u128, |v, b| {
                (v << 5) | alphabet.iter().position(|&c| c == b).unwrap() as u128
            });
            (s.as_str(), value.to_be_bytes().to_vec())
        }
    };
    vec![text.as_bytes().to_vec(), octets]
}
pub(crate) fn check_identifiers(obs: &[SealingObservation], needles: &[Vec<u8>]) -> Verdict {
    for o in obs {
        for id in &o.identifiers {
            fail_if!(
                needles.iter().any(|n| id.windows(n.len()).any(|w| w == n)),
                "link/peer/carrier/liveness identifier exposed a device or session id"
            );
        }
    }
    Verdict::Pass("observed identifiers contain no test device/session id (text or octets); arbitrary derivations need binding review".into())
}

pub(crate) fn inbound(h: &dyn TransportHarness) -> Verdict {
    let (m, a, c) = raw(h);
    if !c.sealing {
        return absent();
    }
    let b = m.transport();
    let k = if c.reach == Reach::CrossImplementation {
        key(2)
    } else {
        key(1)
    };
    let t: &dyn Transport = if c.reach == Reach::CrossImplementation {
        b.start(&k, m.configuration()).unwrap();
        &*b
    } else {
        &*a
    };
    let seen = Seen::default();
    let _s = t
        .subscribe(&Destination::Device(k.clone()), seen.handler())
        .unwrap();
    m.subscribed();
    let mut sent = Vec::new();
    for kind in [
        PayloadKind::Envelope,
        PayloadKind::Receipt,
        PayloadKind::Presence,
    ] {
        let p = frame(
            &k,
            kind,
            b"device-subscription-control",
            c.max_payload_octets,
        );
        fail_if!(
            a.publish(
                &Destination::Device(k.clone()),
                p.clone(),
                Deadline::at(m.now() + LONG)
            ) != PublishResult::Taken,
            "sealed control refused"
        );
        sent.push(p);
    }
    for _ in 0..20 {
        if seen.len() >= 3 {
            break;
        }
        m.settle();
    }
    let got = seen.get();
    fail_if!(
        got.len() != 3 || sent.iter().any(|p| !got.iter().any(|i| &i.payload == p)),
        "device subscription did not receive all three unchanged sealed frames"
    );
    Verdict::Pass(
        "all three kinds arrive as unchanged sealed frames on local device subscription".into(),
    )
}

#[cfg(test)]
mod identifier_tests {
    use super::*;

    #[test]
    fn actual_session_octets_and_every_suite_identifier_are_scanned_at_any_frame_size() {
        let octets = std::array::from_fn(|i| (i as u8).wrapping_mul(17));
        let d = Destination::Session(SessionId::from_random_octets(octets));
        assert_eq!(identifier_needles(&d)[1], octets);
        let destinations: Vec<_> = (1..=3)
            .map(|n| Destination::Device(key(n)))
            .chain((1..=15).map(session))
            .chain([d])
            .collect();
        let needles: Vec<_> = destinations.iter().flat_map(identifier_needles).collect();
        for destination in &destinations {
            for needle in identifier_needles(destination) {
                for size in [54, 4401, 65590] {
                    let mut id = b"prefix/".to_vec();
                    id.extend(&needle);
                    id.extend(b"/suffix");
                    let observation = SealingObservation {
                        frame: vec![0; size],
                        accompanying: Vec::new(),
                        identifiers: vec![id],
                    };
                    assert!(check_identifiers(&[observation], &needles).is_fail());
                }
            }
        }
    }
}
