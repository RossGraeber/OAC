// SPDX-License-Identifier: Apache-2.0

//! The shared transport contract suite (#59, F10; `tests/protocol/contract/transport/`) run
//! against the real Zenoh transport, unchanged, over loopback (#62, G1 acceptance item 1;
//! Gate S4 criterion 1). Local mode has one discovery path, a fixed loopback rendezvous
//! with multicast scouting and gossip off ([`PeerConfiguration::local`] is
//! [`PeerConfiguration::rendezvous`] on the default port; PR #364 review finding 1). Each
//! medium claims its own rendezvous port ([`Rendezvous`]), so media never meet, and the
//! suite runs on every host, the Linux loopback-only namespace included.
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

use std::fs::{File, OpenOptions, TryLockError};
use std::net::{Ipv4Addr, TcpListener};
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
    /// Keeps the rendezvous port claimed until the medium is dropped.
    _rendezvous: Rendezvous,
}

impl TransportHarness for PeerHarness {
    fn binding_sealing(&self) -> bool {
        true
    }

    fn name(&self) -> String {
        "zenoh peer, local mode (loopback rendezvous)".into()
    }

    fn medium(&self) -> Box<dyn Medium> {
        let rendezvous = Rendezvous::claim();
        Box::new(PeerMedium {
            conf: PeerConfiguration::rendezvous(rendezvous.port)
                .with_partition(format!("contract-{:016x}", unique())),
            made: Mutex::new(Vec::new()),
            _rendezvous: rendezvous,
        })
    }
}

/// The first and one past the last port a rendezvous is claimed from. All of it lies below
/// the default ephemeral ranges of Windows and macOS (from 49152) and Linux (from 32768), so
/// no socket the operating system hands out for a connection or for a `:0` listener (a
/// joiner's listener, [`PeerConfiguration`]) is on one of these ports by default. A Linux
/// host that lowered `ip_local_port_range` below this range is checked for in
/// `the_rendezvous_range_lies_below_the_linux_ephemeral_range`.
const RENDEZVOUS_PORTS: std::ops::Range<u16> = 28_000..32_000;

/// The lowest port of the default ephemeral ranges of Linux, Windows and macOS.
const LOWEST_DEFAULT_EPHEMERAL_PORT: u16 = 32_768;
const _: () = assert!(RENDEZVOUS_PORTS.end <= LOWEST_DEFAULT_EPHEMERAL_PORT);

/// The file whose exclusive lock is the claim on `port`.
fn claim_path(port: u16) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("oac-contract-rendezvous-{port}.lock"))
}

/// A loopback rendezvous port, claimed for one medium for as long as this value lives.
///
/// The rendezvous is a fixed port by design: every transport of a medium must be configured
/// with it before the first of them starts, so it cannot be left to Zenoh to pick with `:0`
/// the way a joiner's own listener is (and `Session::info().locators()`, which reports a
/// bound locator, is `#[zenoh_macros::unstable]`, zenoh-1.10.1/src/api/info.rs:122; this
/// crate does not enable `unstable`, Cargo.toml). Probing a port and releasing it left a
/// window in which another test took it (an ephemeral socket, or a parallel run of this
/// suite), and the rendezvous then refused the port as held by a program that is not an OAC
/// transport.
///
/// The port is claimed instead of probed: an exclusive lock on a file named for the port is
/// held until the medium is dropped. The lock is the operating system's, shared by every
/// process on the host and released when the holder dies, so concurrent runs never claim the
/// same port and the lock of a crashed run is not left held. The port is also bound once to
/// leave out one that a foreign program holds; that probe cannot race, as nothing that is
/// not a foreign program binds in this range.
///
/// Residuals:
/// - The lock files are never deleted: each is empty, at most one per port of the range
///   (4000), and stays in the temporary directory.
/// - A Linux host whose `ip_local_port_range` starts below the end of the range is not
///   protected; the range test fails there loudly instead.
/// - A port the operating system excludes from binding (Windows reserved port ranges) is
///   skipped by the bind probe.
struct Rendezvous {
    port: u16,
    _claim: File,
}

impl Rendezvous {
    fn claim() -> Rendezvous {
        let span = u64::from(RENDEZVOUS_PORTS.end - RENDEZVOUS_PORTS.start);
        let start = unique().wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33;
        for step in 0..span {
            let port = RENDEZVOUS_PORTS.start + ((start + step) % span) as u16;
            let Ok(file) = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(claim_path(port))
            else {
                continue;
            };
            match file.try_lock() {
                Ok(()) => {}
                Err(TryLockError::WouldBlock) => continue,
                Err(TryLockError::Error(e)) => panic!("locking a rendezvous claim: {e}"),
            }
            if TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok() {
                return Rendezvous { port, _claim: file };
            }
        }
        panic!("no loopback rendezvous port could be claimed in {RENDEZVOUS_PORTS:?}");
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

/// While a claim is held, a new handle on its lock file cannot lock it (the exclusion every
/// other process and thread meets); once the claim is dropped, it can.
#[test]
fn a_held_claim_excludes_every_other_handle_until_it_is_dropped() {
    let claim = Rendezvous::claim();
    let port = claim.port;
    let open = || {
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(claim_path(port))
            .expect("the lock file of a claimed port opens")
    };
    let other = open();
    assert!(matches!(other.try_lock(), Err(TryLockError::WouldBlock)));
    // A second claim, however many are made, never hands out the held port.
    let others: Vec<Rendezvous> = (0..16).map(|_| Rendezvous::claim()).collect();
    assert!(others.iter().all(|r| r.port != port));
    drop(claim);
    assert!(other.try_lock().is_ok(), "the claim was released on drop");
}

/// The claimed range lies below this host's ephemeral range, so no connection or `:0`
/// listener can be on a claimed port. Checked where the range is readable (Linux, where
/// it is configurable); Windows and macOS defaults start at 49152, above
/// `LOWEST_DEFAULT_EPHEMERAL_PORT`, asserted at compile time.
#[cfg(target_os = "linux")]
#[test]
fn the_rendezvous_range_lies_below_the_linux_ephemeral_range() {
    let range = std::fs::read_to_string("/proc/sys/net/ipv4/ip_local_port_range")
        .expect("the Linux ephemeral port range is readable");
    let low: u16 = range
        .split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .expect("ip_local_port_range holds two ports");
    assert!(
        RENDEZVOUS_PORTS.end <= low,
        "ip_local_port_range starts at {low}, inside the rendezvous range {RENDEZVOUS_PORTS:?}"
    );
}
