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
    })
}
struct Logical {
    raw: Box<dyn Transport>,
    local: KeyId,
    caps: TransportCapabilities,
    watches: Mutex<Vec<Subscription>>,
    frames: FrameBook,
}
fn opened(k: &KeyId, i: &Inbound, d: &Destination, presence: bool) -> Option<Payload> {
    if i.payload.kind() != PayloadKind::Sealed {
        return None;
    }
    let o = sealing::open(&private(k), i.payload.octets())?;
    if o.payload.len() < 100 || (o.kind == PayloadKind::Presence) != presence {
        return None;
    }
    let label = format!("{d:?}");
    let end = o.payload[..100].iter().position(|&b| b == 0)?;
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
        let d = d.clone();
        let k = self.local.clone();
        let frames = self.frames.clone();
        self.raw.subscribe(
            &Destination::Device(self.local.clone()),
            Arc::new(move |mut i| {
                if !frames.lock().unwrap().contains(i.payload.octets()) {
                    return;
                }
                if let Some(p) = opened(&k, &i, &d, false) {
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
        let s = self.raw.subscribe(
            &d.clone(),
            Arc::new(move |i| {
                if !frames.lock().unwrap().contains(i.payload.octets()) {
                    return;
                }
                if let Some(payload) = opened(&k, &i, &d, true) {
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
        self.watches.lock().unwrap().clear();
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
    let Destination::Session(s) = session(7) else {
        unreachable!()
    };
    let needles = [
        key(1).as_str().as_bytes().to_vec(),
        hex(key(1).as_str()),
        key(2).as_str().as_bytes().to_vec(),
        hex(key(2).as_str()),
        s.as_str().as_bytes().to_vec(),
        vec![7; 16],
    ];
    for o in obs {
        for id in o.identifiers {
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
