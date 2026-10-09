// SPDX-License-Identifier: Apache-2.0

//! Two Zenoh transports on one host, over a fixed loopback rendezvous port: one subscribes
//! to a session, the other publishes an envelope to it.
//!
//!     cargo run -p oac-transport-zenoh --example loopback
//!
//! Its release build is also the measured size of the transport embedded in a binary
//! (`docs/planning/v0.1/11-risks.md`, RISK-BIN-SIZE).

use std::net::TcpListener;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use oac_core::ids::{KeyId, SessionId};
use oac_core::transport::{Deadline, Destination, Payload, PayloadKind, Transport};
use oac_transport_zenoh::{PeerConfiguration, PeerTransport};

fn main() {
    let port = TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("a free loopback port")
        .port();
    let conf = PeerConfiguration::rendezvous(port).with_partition("example");
    let (a, b) = (PeerTransport::new(), PeerTransport::new());
    a.start(&KeyId::parse(&"a".repeat(64)).unwrap(), conf.clone().wrap())
        .expect("start a");
    b.start(&KeyId::parse(&"b".repeat(64)).unwrap(), conf.wrap())
        .expect("start b");
    let session = Destination::Session(SessionId::from_random_octets([42; 16]));
    let (tx, rx) = mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    let _sub = b
        .subscribe(
            &session,
            Arc::new(move |i| {
                let _ = tx.lock().unwrap().send(i.payload.octets().to_vec());
            }),
        )
        .expect("subscribe");
    let start = Instant::now();
    let got = loop {
        a.publish(
            &session,
            Payload::new(PayloadKind::Envelope, b"hello".to_vec()),
            Deadline::at(Instant::now() + Duration::from_secs(5)),
        );
        if let Ok(o) = rx.recv_timeout(Duration::from_millis(200)) {
            break o;
        }
        assert!(start.elapsed() < Duration::from_secs(15), "no delivery");
    };
    println!("received {} octets", got.len());
    a.shutdown();
    b.shutdown();
}
