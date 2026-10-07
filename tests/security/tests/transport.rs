// SPDX-License-Identifier: Apache-2.0

//! A hostile carrying path (06 rows 3, 4, 6, 9, 10): envelopes cross the in-memory
//! transport between three endpoints, one of them the attacker's. The transport gives no
//! authenticity of its own ([IFC-TRN-010]), so whatever it hands over is judged by the
//! receiver's own checks: forged and tampered payloads are refused, duplicated ones handed
//! off once, and a payload is attributed to the key that signed it, never to the endpoint
//! that carried it.
//!
//! The test is the conduit between the transport's handler and the receive path, so that each
//! copy's state and code can be asserted. The composed pipelines (#313) report neither for
//! a refused copy (no receipt goes back for an unverified one); `routing.rs`'s
//! `row02_unauthorized_send_through_the_composed_pipeline` runs the same refusals through
//! them and asserts what they do expose: that the adapter is never called.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use oac_core::delivery::{DeliveryState, ErrorCode};
use oac_core::envelope::Envelope;
use oac_core::ids::KeyId;
use oac_core::transport::{
    Deadline, Destination, Payload, PayloadKind, PublishResult, Reach, Subscription, Transport,
};
use oac_security_suite::{Delivery, Device, granted_pair, rewrite, sid};
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport, ScriptedFaults};

/// Three endpoints on one network, and Bob's session subscribed.
struct Net {
    network: MemoryNetwork,
    faults: ScriptedFaults,
    endpoints: Vec<MemoryTransport>,
    inbox: Arc<Mutex<Vec<Vec<u8>>>>,
    _sub: Subscription,
}

impl Net {
    fn new(keys: [&KeyId; 3]) -> Net {
        let faults = ScriptedFaults::new();
        let network = MemoryNetwork::builder()
            .reach(Reach::CrossImplementation)
            .faults(faults.clone())
            .build();
        let endpoints: Vec<MemoryTransport> = keys
            .iter()
            .map(|k| {
                let t = MemoryTransport::new();
                t.start(k, MemoryConfiguration::wrap(&network))
                    .expect("started");
                t
            })
            .collect();
        let inbox = Arc::new(Mutex::new(Vec::new()));
        let i = inbox.clone();
        // Endpoint 1 is Bob's.
        let sub = endpoints[1]
            .subscribe(
                &Destination::Session(sid(2)),
                Arc::new(move |inbound| {
                    i.lock().unwrap().push(inbound.payload.octets().to_vec());
                }),
            )
            .expect("subscribed");
        Net {
            network,
            faults,
            endpoints,
            inbox,
            _sub: sub,
        }
    }

    /// Endpoint `from` publishes `octets` to Bob's session.
    fn publish(&self, from: usize, octets: &[u8]) {
        let deadline = Deadline::at(self.network.now() + Duration::from_secs(5));
        assert_eq!(
            self.endpoints[from].publish(
                &Destination::Session(sid(2)),
                Payload::new(PayloadKind::Envelope, octets.to_vec()),
                deadline,
            ),
            PublishResult::Taken
        );
    }

    /// Every payload the transport handed to Bob's subscription, through Bob's receive
    /// path, in arrival order.
    fn deliver_all(&self, bob: &mut Device) -> Vec<Delivery> {
        self.network.settle();
        let arrived = std::mem::take(&mut *self.inbox.lock().unwrap());
        arrived.iter().map(|o| bob.receive(o)).collect()
    }
}

fn setup() -> (Device, Device, Device, Net) {
    let (alice, bob) = granted_pair();
    let mallory = Device::new("mallory", bob.clock.clone());
    let net = Net::new([&alice.key_id(), &bob.key_id(), &mallory.key_id()]);
    (alice, bob, mallory, net)
}

fn handed(ds: &[Delivery]) -> Vec<&Delivery> {
    ds.iter()
        .filter(|d| d.state == DeliveryState::HandedToHarness)
        .collect()
}

/// 06 row 3 over a transport: a node on the path that rewrites a member of Alice's envelope
/// and passes it on gets it refused; Alice's own copy is handed off.
#[test]
fn row03_tampered_bytes_over_the_transport_are_rejected() {
    let (alice, mut bob, _mallory, net) = setup();
    let env = alice.sign("m1", &sid(1), &sid(2), "pay invoice 17");
    let tampered = rewrite(env.octets(), "invoice 17", "invoice 99");
    net.publish(2, &tampered);
    net.publish(0, env.octets());
    let ds = net.deliver_all(&mut bob);
    assert_eq!(ds.len(), 2);
    let h = handed(&ds);
    assert_eq!(h.len(), 1);
    assert!(oac_security_suite::text_of(h[0].handed.as_ref().unwrap()).contains("invoice 17"));
    assert!(
        ds.iter()
            .any(|d| d.outcome() == (DeliveryState::Rejected, Some(ErrorCode::SignatureInvalid)))
    );
}

/// 06 row 4 over a transport ([IFC-TRN-011]): a transport that duplicates a payload, here
/// three copies of one publish, gets it handed off once; the other copies are `duplicate`.
#[test]
fn row04_transport_duplicates_are_handed_off_once() {
    let (alice, mut bob, _mallory, net) = setup();
    net.faults.duplicate(3);
    net.publish(0, alice.sign("m1", &sid(1), &sid(2), "once").octets());
    let ds = net.deliver_all(&mut bob);
    assert_eq!(ds.len(), 3, "the transport carried three copies");
    assert_eq!(handed(&ds).len(), 1);
    assert_eq!(
        ds.iter()
            .filter(|d| d.state == DeliveryState::Duplicate)
            .count(),
        2
    );
}

/// 06 rows 6 and 9 ([SEC-SIG-030]): an endpoint on the network that injects envelopes of
/// its own, signed with its own key under Alice's name or under its own, gets them refused;
/// joining the transport gives it nothing.
#[test]
fn row06_forged_envelope_injected_on_the_transport_is_rejected() {
    let (_alice, mut bob, mallory, net) = setup();
    let own = mallory.sign("m1", &sid(1), &sid(2), "from mallory");
    let as_alice = rewrite(
        mallory.sign("m2", &sid(1), &sid(2), "from alice").octets(),
        "\"principal\":\"mallory\"",
        "\"principal\":\"alice\"",
    );
    net.publish(2, own.octets());
    net.publish(2, &as_alice);
    let ds = net.deliver_all(&mut bob);
    assert_eq!(ds.len(), 2);
    for d in &ds {
        assert_eq!(
            d.outcome(),
            (DeliveryState::Rejected, Some(ErrorCode::UnknownKey))
        );
        assert!(d.handed.is_none());
    }
}

/// 06 rows 9 and 10 ([SEC-AUZ-004]): a payload is judged by its signature alone. Alice's
/// genuine envelope, relayed by Mallory's endpoint, is attributed to Alice's key and handed
/// off once; the carrying endpoint plays no part in who sent it, and Alice's own later copy
/// is a duplicate.
#[test]
fn row09_a_payload_from_any_endpoint_is_judged_by_its_signature_alone() {
    let (alice, mut bob, _mallory, net) = setup();
    let env: Envelope = alice.sign("m1", &sid(1), &sid(2), "relayed");
    net.publish(2, env.octets());
    let first = net.deliver_all(&mut bob);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].state, DeliveryState::HandedToHarness);
    assert_eq!(first[0].verified_by, Some(alice.key_id()));
    let msg = first[0].handed.as_ref().unwrap();
    assert_eq!(msg.verified_by().unwrap().key_id(), &alice.key_id());
    net.publish(0, env.octets());
    let second = net.deliver_all(&mut bob);
    assert_eq!(second[0].state, DeliveryState::Duplicate);
}
