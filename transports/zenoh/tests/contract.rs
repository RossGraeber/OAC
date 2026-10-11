// SPDX-License-Identifier: Apache-2.0

//! The shared transport contract suite (#59, F10; `tests/protocol/contract/transport/`) run
//! against the real Zenoh transport, unchanged, over loopback (#62, G1 acceptance item 1;
//! Gate S4 criterion 1). Local mode has one discovery path, a fixed loopback rendezvous
//! with multicast scouting and gossip off ([`PeerConfiguration::local`] is
//! [`PeerConfiguration::rendezvous`] on the default port; PR #364 review finding 1). Each
//! medium uses a fresh free port, so media never meet, and the suite runs on every host,
//! the Linux loopback-only namespace included.
//!
//! Each medium is a fresh partition, so nothing started on one medium reaches another. The
//! media use the real clock and have no fault control: the checks that need a copy held in
//! flight report what they could not run, as the suite defines.
//!
//! Start waits for native declarations. The fixed frame subscription never changes;
//! `subscribed` allows propagation to settle, without publishing probe frames to core.
//! Raw capture is at each transport's actual sample send/receive boundary. Fixed frame
//! key and encoding are independently checked against this binding's constants, excluded
//! from accompanying application values. Carrier bytes and native identifier snapshots
//! stay in the audit. Native packets/ephemeral locators are not exposed by the stable API;
//! derivations and capture completeness remain binding-review obligations.

use std::net::TcpListener;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use oac_contract_transport::{Medium, Report, SealingObservation, TransportHarness, Verdict, run};
use oac_core::transport::{Transport, TransportConfiguration};
use oac_transport_zenoh::{PeerConfiguration, PeerTransport};

static MEDIA: AtomicU64 = AtomicU64::new(0);

fn unique() -> u64 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    t ^ (u64::from(std::process::id()) << 32) ^ MEDIA.fetch_add(1, Ordering::Relaxed)
}

struct PeerHarness;

struct PeerMedium {
    conf: PeerConfiguration,
    made: Mutex<Vec<PeerTransport>>,
}

impl TransportHarness for PeerHarness {
    fn binding_sealing(&self) -> bool {
        true
    }

    fn name(&self) -> String {
        "zenoh peer, local mode (loopback rendezvous)".into()
    }

    fn medium(&self) -> Box<dyn Medium> {
        Box::new(PeerMedium {
            conf: PeerConfiguration::rendezvous(free_port())
                .with_partition(format!("contract-{:016x}", unique())),
            made: Mutex::new(Vec::new()),
        })
    }
}

/// A loopback port free at the time of asking.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("a free loopback port")
}

impl Medium for PeerMedium {
    fn transport(&self) -> Box<dyn Transport> {
        let t = PeerTransport::new();
        self.made.lock().unwrap().push(t.clone());
        Box::new(t)
    }

    fn configuration(&self) -> TransportConfiguration {
        self.conf.clone().wrap()
    }

    fn now(&self) -> Instant {
        Instant::now()
    }

    fn subscribed(&self) {
        // start waits for native declarations; the fixed subscription never changes.
        thread::sleep(Duration::from_millis(300));
    }

    fn sealing_observations(&self) -> Option<Vec<SealingObservation>> {
        let transports = self.made.lock().unwrap();
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        hash.update(b"oac transport partition v1\0");
        hash.update(self.conf.partition().as_bytes());
        let prefix: String = hash.finalize()[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let expected_key = format!("oac/1/{prefix}/frames");

        Some(
            transports
                .iter()
                .flat_map(|t| t.carriage_for_tests())
                .map(|o| {
                    assert_eq!(o.key, expected_key);
                    assert_eq!(o.encoding, zenoh::bytes::Encoding::ZENOH_BYTES.to_string());
                    SealingObservation {
                        frame: o.frame,
                        accompanying: o.accompanying,
                        identifiers: o.identifiers,
                    }
                })
                .collect(),
        )
    }

    fn health_must_not_contain(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .made
            .lock()
            .unwrap()
            .iter()
            .flat_map(|t| t.identifiers_for_leak_checks())
            .collect();
        v.push("224.0.0.224".into());
        v
    }
}

fn check(report: &Report) {
    println!("{report}");
    report.assert_conformant();
    // [IFC-TRN-050]: carried, not "not applicable" (the brief for G1, #62).
    for v in report.verdicts("IFC-TRN-050") {
        assert!(matches!(v, Verdict::Pass(_)), "IFC-TRN-050: {v:?}");
    }
    // Two implementations reach each other: the two-sided checks ran.
    for id in ["IFC-TRN-043", "IFC-TRN-044", "IFC-TRN-001", "IFC-TRN-031"] {
        for v in report.verdicts(id) {
            assert!(matches!(v, Verdict::Pass(_)), "{id}: {v:?}");
        }
    }
}

#[test]
fn contract_suite_in_local_mode() {
    let report = run(&PeerHarness);
    check(&report);
    // Scouting off: multicast_discovery is declared absent, so the check passes outright.
    assert!(matches!(
        report.verdicts("IFC-TRN-021")[2],
        Verdict::Pass(_)
    ));
}
