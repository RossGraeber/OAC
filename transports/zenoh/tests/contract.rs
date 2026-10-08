// SPDX-License-Identifier: Apache-2.0

//! The shared transport contract suite (#59, F10; `tests/protocol/contract/transport/`) run
//! against the real Zenoh transport, unchanged, over loopback (#62, G1 acceptance item 1;
//! Gate S4 criterion 1). Two media, one per discovery path:
//!
//! - **multicast**: [`PeerConfiguration::local`], the production default (C7 §5, gate G3);
//! - **rendezvous**: [`PeerConfiguration::rendezvous`], a fixed loopback port with
//!   multicast scouting off, the G3 fallback.
//!
//! A host that cannot send to the multicast group at all (the Linux CI test step runs in a
//! loopback-only network namespace) skips the multicast leg with a printed `GAP:` line; the
//! rendezvous leg runs everywhere.
//!
//! Each medium is a fresh partition, so nothing started on one medium reaches another. The
//! media use the real clock and have no fault control: the checks that need a copy held in
//! flight report what they could not run, as the suite defines.
//!
//! `subscribed` waits for real interest: it subscribes a probe on every running transport
//! of the medium and has every running transport publish to it until each probe has heard
//! from every transport. Only then is a payload published next certain to have a path.
//!
//! `health_must_not_contain` lists every native peer id, fixed endpoint and key expression
//! prefix the medium's transports held, and the multicast group address.

use std::collections::HashSet;
use std::net::{TcpListener, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use oac_contract_transport::{Medium, Report, TransportHarness, Verdict, run};
use oac_core::health::HealthState;
use oac_core::ids::SessionId;
use oac_core::transport::{
    CarrierHandle, Deadline, Destination, Payload, PayloadKind, Transport, TransportConfiguration,
};
use oac_transport_zenoh::{PeerConfiguration, PeerTransport};

/// How long `subscribed` waits for every probe to hear from every transport.
const READY: Duration = Duration::from_secs(15);

static MEDIA: AtomicU64 = AtomicU64::new(0);

fn unique() -> u64 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    t ^ (u64::from(std::process::id()) << 32) ^ MEDIA.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Discovery {
    Multicast,
    Rendezvous,
}

struct PeerHarness(Discovery);

struct PeerMedium {
    conf: PeerConfiguration,
    made: Mutex<Vec<PeerTransport>>,
    probes: AtomicU64,
}

impl TransportHarness for PeerHarness {
    fn name(&self) -> String {
        match self.0 {
            Discovery::Multicast => "zenoh peer, loopback, multicast discovery".into(),
            Discovery::Rendezvous => "zenoh peer, loopback, fixed rendezvous port".into(),
        }
    }

    fn medium(&self) -> Box<dyn Medium> {
        let conf = match self.0 {
            Discovery::Multicast => PeerConfiguration::local(),
            Discovery::Rendezvous => PeerConfiguration::rendezvous(free_port()),
        };
        Box::new(PeerMedium {
            conf: conf.with_partition(format!("contract-{:016x}", unique())),
            made: Mutex::new(Vec::new()),
            probes: AtomicU64::new(1),
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

impl PeerMedium {
    fn running(&self) -> Vec<PeerTransport> {
        self.made
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.health().state == HealthState::Healthy)
            .cloned()
            .collect()
    }

    fn probe_session(&self) -> Destination {
        let n =
            u128::from(unique()) << 64 | u128::from(self.probes.fetch_add(1, Ordering::Relaxed));
        Destination::Session(SessionId::from_random_octets(n.to_be_bytes()))
    }
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
        let running = self.running();
        let start = Instant::now();
        for r in &running {
            let probe = self.probe_session();
            let heard: Arc<Mutex<HashSet<CarrierHandle>>> = Arc::default();
            let h = heard.clone();
            let _sub = r
                .subscribe(
                    &probe,
                    Arc::new(move |i| {
                        h.lock().unwrap().insert(i.carrier);
                    }),
                )
                .expect("probe subscription");
            loop {
                for s in &running {
                    s.publish(
                        &probe,
                        Payload::new(PayloadKind::Envelope, b"probe".to_vec()),
                        Deadline::at(Instant::now() + Duration::from_secs(5)),
                    );
                }
                thread::sleep(Duration::from_millis(50));
                if heard.lock().unwrap().len() >= running.len() {
                    break;
                }
                assert!(
                    start.elapsed() < READY,
                    "interest did not propagate: a probe heard {} of {} transports in {READY:?}",
                    heard.lock().unwrap().len(),
                    running.len()
                );
            }
        }
        // Let the last probes drain before the suite publishes.
        thread::sleep(Duration::from_millis(50));
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

/// Why this host cannot send a multicast datagram at all, if it cannot: for example a
/// network namespace whose only interface is loopback, with no multicast route (the Linux
/// CI test step runs in one, `scripts/loopback-only.sh`). Port 9 is the discard port, so the
/// probe reaches no scouting socket.
fn no_multicast_route() -> Option<String> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.send_to(b"oac multicast route probe", "224.0.0.224:9")
        .err()
        .map(|e| e.to_string())
}

#[test]
fn contract_suite_with_multicast_discovery() {
    if let Some(why) = no_multicast_route() {
        // Recorded, not passed silently: the rendezvous leg below covers the transport on
        // this host, and the multicast leg runs on every host that has a multicast route.
        eprintln!(
            "GAP: multicast leg not run on this host: sending to the scouting group fails ({why}). \
             The fixed rendezvous leg covers this host."
        );
        return;
    }
    let report = run(&PeerHarness(Discovery::Multicast));
    check(&report);
}

#[test]
fn contract_suite_with_a_fixed_rendezvous_port() {
    let report = run(&PeerHarness(Discovery::Rendezvous));
    check(&report);
    // Scouting off: multicast_discovery is declared absent, so the check passes outright.
    assert!(matches!(
        report.verdicts("IFC-TRN-021")[2],
        Verdict::Pass(_)
    ));
}
