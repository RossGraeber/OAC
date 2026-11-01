// SPDX-License-Identifier: Apache-2.0

//! The carrier-appearance rows (`spec/interfaces.md` §6.6, revision 0.13; #473).
//!
//! A transport MAY report a carrier appearance ([IFC-TRN-120]), so these rows apply only to
//! a transport whose binding says it does ([`TransportHarness::reports_carrier_appearance`]);
//! for any other they are [`Verdict::NotApplicable`], never a pass. They also need a second
//! implementation whose link can appear, so a single-implementation medium reports them not
//! applicable too.
//!
//! What each row covers, and what stays outside a transport suite:
//!
//! - [IFC-TRN-121]: the appearance of a peer that starts while this transport runs, and of a
//!   peer already usable before the watch was made, each reaches the `watch_presence`
//!   handler with no call from the test in between except the medium's own settle barrier.
//! - [IFC-TRN-122]: the event is destructured with no `..`, so a second member breaks this
//!   file at compile time; the handle is the one that the frames of that link carry; and the
//!   appearance is not a presence record. That the handle derives from no device or session
//!   value cannot be read from outside, because `CarrierHandle` has no accessor, so that
//!   part rests on the binding document ([IFC-TRN-108] for a sealing transport).
//! - [IFC-TRN-123], the transport half: the announcements the core sends are payloads, and
//!   sending payloads over a link that is already up reports no further appearance. The core
//!   half (a set of announcements within 2 seconds of an appearance) is not observable here.
//! - [IFC-TRN-125], the transport half: a peer that cycles its links is reported afresh each
//!   time. The hold-off of 1 second is the core's and is not observable here.

use super::*;

/// Why a row does not apply to this harness or medium, if it does not.
fn gate(h: &dyn TransportHarness) -> Option<Verdict> {
    if !h.reports_carrier_appearance() {
        return Some(Verdict::NotApplicable(
            "the binding does not report a carrier appearance ([IFC-TRN-120] is a MAY)".into(),
        ));
    }
    None
}

fn single(w: &World) -> Option<Verdict> {
    (w.a.caps.reach != Reach::CrossImplementation).then(|| {
        Verdict::NotApplicable(
            "one implementation: no second implementation whose link could appear".into(),
        )
    })
}

impl World {
    /// A medium with the first endpoint only; [`World::start_peer`] starts another later.
    fn first_only(h: &dyn TransportHarness) -> World {
        let m: Arc<dyn Medium> = h.medium().into();
        let frames = sealed::FrameBook::default();
        let epoch = m.now();
        let a = start(m.clone(), 1, frames.clone(), epoch).expect("start of the first transport");
        World {
            m,
            a,
            b: None,
            frames,
            epoch,
        }
    }

    /// Start a transport for device `n`, as a peer that reaches the first endpoint.
    fn start_peer(&self, n: u8) -> Ep {
        start(self.m.clone(), n, self.frames.clone(), self.epoch).expect("start of a peer")
    }
}

impl Seen<PresenceEvent> {
    /// The carrier handle of each `carrier-appeared` event, in the order handed over. The
    /// pattern names the only member the event has ([IFC-TRN-122]).
    fn appearances(&self) -> Vec<CarrierHandle> {
        self.get()
            .into_iter()
            .filter_map(|e| match e {
                PresenceEvent::CarrierAppeared { carrier } => Some(carrier),
                PresenceEvent::Record { .. } | PresenceEvent::CarrierLoss { .. } => None,
            })
            .collect()
    }

    fn losses(&self) -> usize {
        self.get()
            .iter()
            .filter(|e| matches!(e, PresenceEvent::CarrierLoss { .. }))
            .count()
    }
}

/// [IFC-TRN-121]: a transport that reports a carrier appearance hands it to the
/// `watch_presence` handler on its own initiative, both for a peer that starts while the
/// watch is in place and for a peer that was already usable before the watch was made.
pub(crate) fn handed_on_own_initiative(h: &dyn TransportHarness) -> Verdict {
    if let Some(v) = gate(h) {
        return v;
    }
    let w = World::first_only(h);
    if let Some(v) = single(&w) {
        return v;
    }
    // A peer that starts while the watch is in place.
    let live = Seen::default();
    w.watch(&w.a, live.watcher());
    let _peer = w.start_peer(2);
    fail_if!(
        !w.eventually(|| !live.appearances().is_empty()),
        "no carrier-appeared event was handed over after a peer started ({} event(s) of any kind)",
        live.len()
    );
    // A peer that was usable before any watch existed: the watch is made after the link is
    // up, and the transport still hands the appearance over (the core registers its watch
    // after `start`).
    let w2 = World::first_only(h);
    let _peer2 = w2.start_peer(2);
    w2.quiet();
    let late = Seen::default();
    w2.watch(&w2.a, late.watcher());
    fail_if!(
        !w2.eventually(|| !late.appearances().is_empty()),
        "no carrier-appeared event was handed over for a link already usable when the watch \
         was made ({} event(s) of any kind)",
        late.len()
    );
    Verdict::Pass(
        "carrier-appeared handed over for a peer that started under the watch and for a link \
         already usable before it"
            .into(),
    )
}

/// [IFC-TRN-122]: the event has the carrier handle as its only member, that handle names the
/// link whose frames follow, and the appearance is not itself a presence record.
pub(crate) fn only_its_handle(h: &dyn TransportHarness) -> Verdict {
    if let Some(v) = gate(h) {
        return v;
    }
    let w = World::first_only(h);
    if let Some(v) = single(&w) {
        return v;
    }
    let seen = Seen::default();
    w.watch(&w.a, seen.watcher());
    let peer = w.start_peer(2);
    fail_if!(
        !w.eventually(|| !seen.appearances().is_empty()),
        "no carrier-appeared event to inspect"
    );
    fail_if!(
        !seen.records().is_empty(),
        "an appearance was reported as a presence record"
    );
    // The link's own frames: one presence record from the peer over the link that appeared.
    peer.t.send_presence(
        &Destination::Device(w.a.key.clone()),
        payload(PayloadKind::Presence, b"p-122"),
        w.deadline(LONG),
    );
    fail_if!(
        !w.eventually(|| !seen.records().is_empty()),
        "the peer's presence record did not arrive"
    );
    let frame_carrier = seen
        .get()
        .into_iter()
        .find_map(|e| match e {
            PresenceEvent::Record { carrier, .. } => Some(carrier),
            PresenceEvent::CarrierLoss { .. } | PresenceEvent::CarrierAppeared { .. } => None,
        })
        .expect("a record arrived");
    fail_if!(
        !seen.appearances().contains(&frame_carrier),
        "no carrier-appeared event named the handle that the peer's frames carry"
    );
    fail_if!(
        seen.records().len() != 1,
        "{} presence record(s) arrived for one sent",
        seen.records().len()
    );
    Verdict::Pass(
        "carrier-appeared holds only a handle, equal to the handle of the link's frames, and is \
         not a presence record (that the handle derives from no device or session value is \
         the binding review's: the type has no accessor)"
            .into(),
    )
}

/// [IFC-TRN-123], the transport half: the core's announcements are payloads, and a payload
/// sent over a link that is already up is no link event. Neither side sees a further
/// carrier-appeared event for presence records and envelopes sent in both directions.
pub(crate) fn payloads_are_no_appearance(h: &dyn TransportHarness) -> Verdict {
    if let Some(v) = gate(h) {
        return v;
    }
    let w = World::first_only(h);
    if let Some(v) = single(&w) {
        return v;
    }
    let (on_a, on_b) = (Seen::default(), Seen::default());
    let (env_a, env_b) = (Seen::default(), Seen::default());
    w.watch(&w.a, on_a.watcher());
    let _sa = w.sub(&w.a, &session(7), &env_a);
    let b = w.start_peer(2);
    w.watch(&b, on_b.watcher());
    let _sb = w.sub(&b, &session(7), &env_b);
    fail_if!(
        !w.eventually(|| !on_a.appearances().is_empty() && !on_b.appearances().is_empty()),
        "the link did not appear on both sides (a: {}, b: {})",
        on_a.appearances().len(),
        on_b.appearances().len()
    );
    w.quiet();
    let before = (on_a.appearances().len(), on_b.appearances().len());
    for i in 0..4u8 {
        let to_b = Destination::Device(b.key.clone());
        let to_a = Destination::Device(w.a.key.clone());
        w.a.t.send_presence(
            &to_b,
            payload(PayloadKind::Presence, &[i]),
            w.deadline(LONG),
        );
        b.t.send_presence(
            &to_a,
            payload(PayloadKind::Presence, &[i]),
            w.deadline(LONG),
        );
        w.a.t.publish(
            &session(7),
            payload(PayloadKind::Envelope, &[i]),
            w.deadline(LONG),
        );
        b.t.publish(
            &session(7),
            payload(PayloadKind::Envelope, &[i]),
            w.deadline(LONG),
        );
    }
    fail_if!(
        !w.eventually(|| on_a.records().len() >= 4 && on_b.records().len() >= 4),
        "the presence records did not arrive (a: {}, b: {})",
        on_a.records().len(),
        on_b.records().len()
    );
    w.quiet();
    let after = (on_a.appearances().len(), on_b.appearances().len());
    fail_if!(
        after != before,
        "payloads over a link already up reported {} further appearance(s) on a and {} on b",
        after.0.saturating_sub(before.0),
        after.1.saturating_sub(before.1)
    );
    Verdict::Pass(format!(
        "no carrier-appeared event for 8 presence records and 8 envelopes over an established \
         link ({} appearance(s) on a, {} on b, all from the link coming up)",
        after.0, after.1
    ))
}

/// [IFC-TRN-125], the transport half: a peer that cycles its links (leaves and starts again)
/// is reported again for each new link, so the core's per-transport hold-off has something
/// to apply to. Every cycle adds at least one further appearance, and no appearance is
/// swallowed because the same device reappears.
pub(crate) fn every_new_link_is_reported(h: &dyn TransportHarness) -> Verdict {
    if let Some(v) = gate(h) {
        return v;
    }
    let w = World::first_only(h);
    if let Some(v) = single(&w) {
        return v;
    }
    let seen = Seen::default();
    w.watch(&w.a, seen.watcher());
    let mut peer = w.start_peer(2);
    const CYCLES: usize = 2;
    for cycle in 0..=CYCLES {
        let want = cycle + 1;
        fail_if!(
            !w.eventually(|| seen.appearances().len() >= want),
            "after {cycle} cycle(s) of the peer's link, {} appearance(s) were reported, expected \
             at least {want}",
            seen.appearances().len()
        );
        if cycle < CYCLES {
            peer.t.shutdown();
            w.quiet();
            peer = w.start_peer(2);
        }
    }
    Verdict::Pass(format!(
        "{} appearance(s) over {} link(s) of one cycling peer ({} carrier loss(es) between)",
        seen.appearances().len(),
        CYCLES + 1,
        seen.losses()
    ))
}
