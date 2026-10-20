// SPDX-License-Identifier: Apache-2.0

//! The test-port claim every test of this crate takes its fixed loopback port from
//! (`tests/support/ports.rs`; #393): deterministic checks of the claim itself. Loopback only.

#[path = "support/ports.rs"]
mod ports;

use std::fs::{OpenOptions, TryLockError};
use std::net::{Ipv4Addr, TcpListener};
use std::thread;

use ports::{PortClaim, RENDEZVOUS_PORTS, claim_path};

fn open(port: u16) -> std::fs::File {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(claim_path(port))
        .expect("the lock file of a claimed port opens")
}

/// While a claim is held, a new handle on its lock file cannot lock it (the exclusion every
/// other process and thread meets); once the claim is dropped, it can.
#[test]
fn a_held_claim_excludes_every_other_handle_until_it_is_dropped() {
    let claim = PortClaim::claim();
    let port = claim.port();
    assert!(RENDEZVOUS_PORTS.contains(&port));
    let other = open(port);
    assert!(matches!(other.try_lock(), Err(TryLockError::WouldBlock)));
    // A second claim, however many are made, never hands out the held port.
    let others: Vec<PortClaim> = (0..16).map(|_| PortClaim::claim()).collect();
    assert!(others.iter().all(|r| r.port() != port));
    drop(claim);
    assert!(other.try_lock().is_ok(), "the claim was released on drop");
}

/// Claims made at once by many threads (the tests of one binary run in parallel) are
/// pairwise distinct, and each port is free to bind while its claim is held.
#[test]
fn concurrent_claims_are_distinct_and_bindable() {
    let claims: Vec<PortClaim> = thread::scope(|scope| {
        let handles: Vec<_> = (0..32).map(|_| scope.spawn(PortClaim::claim)).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut ports: Vec<u16> = claims.iter().map(PortClaim::port).collect();
    ports.sort_unstable();
    ports.dedup();
    assert_eq!(ports.len(), claims.len(), "two claims shared a port");
    for claim in &claims {
        TcpListener::bind((Ipv4Addr::LOCALHOST, claim.port()))
            .expect("a claimed port can be bound by its holder");
    }
}

/// A claimed value keeps its claim for as long as it lives, and gives the port back after.
#[test]
fn a_claimed_value_keeps_the_claim_until_it_is_dropped() {
    let value = ports::claimed(|port| format!("rendezvous-{port}"));
    let port = value.port();
    assert_eq!(*value, format!("rendezvous-{port}"));
    let other = open(port);
    assert!(matches!(other.try_lock(), Err(TryLockError::WouldBlock)));
    drop(value);
    assert!(other.try_lock().is_ok());
}

/// The claimed range lies below this host's ephemeral range, so no connection or `:0`
/// listener can be on a claimed port. Checked where the range is readable (Linux, where
/// it is configurable); Windows and macOS defaults start at 49152, above
/// the 32768 asserted at compile time in `tests/support/ports.rs`.
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

/// `start_first` (#432) moves to a fresh claimed port on the foreign-holder refusal and on no
/// other error, and gives up after the bounded number of attempts.
#[test]
fn start_first_retries_on_the_foreign_holder_refusal_only_and_is_bounded() {
    let refusal = format!("port 1 {}", ports::FOREIGN_HOLDER_REFUSAL);

    // Refused once: the second attempt runs on another port, whose claim is held.
    let mut value = ports::claimed_rebuildable(|port| port);
    let first = value.port();
    let mut seen = Vec::new();
    let outcome = value.start_first(|port| {
        seen.push(*port);
        if seen.len() == 1 {
            Err(refusal.clone())
        } else {
            Ok(())
        }
    });
    assert_eq!(outcome, Ok(()));
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0], first);
    assert_ne!(seen[1], first);
    assert_eq!(*value, seen[1], "the value was rebuilt on the new port");
    assert_eq!(value.port(), seen[1]);
    let other = open(value.port());
    assert!(matches!(other.try_lock(), Err(TryLockError::WouldBlock)));

    // Any other error is returned at once, never retried.
    let mut value = ports::claimed_rebuildable(|port| port);
    let mut calls = 0;
    let outcome = value.start_first(|_| {
        calls += 1;
        Err::<(), _>("the peer session did not open".to_owned())
    });
    assert_eq!((calls, outcome.is_err()), (1, true));

    // A refusal that persists is retried a bounded number of times, then returned.
    let mut value = ports::claimed_rebuildable(|port| port);
    let mut calls = 0;
    let outcome = value.start_first(|_| {
        calls += 1;
        Err::<(), _>(refusal.clone())
    });
    assert_eq!(calls, ports::FOREIGN_HOLDER_ATTEMPTS);
    assert_eq!(outcome, Err(refusal));
}
