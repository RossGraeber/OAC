// SPDX-License-Identifier: Apache-2.0

//! The one place this crate's tests get a fixed loopback port from (#393, after #387).
//!
//! This file is compiled into every test unit of the crate that needs a port: the unit tests
//! of `src/` mount it (`#[cfg(test)] #[path] mod ports` in `src/lib.rs`) and each
//! integration test of `tests/` mounts it the same way. It is a test-only path include and
//! not a `test-support` module of the library on purpose: the library's public surface
//! gains nothing, and no test double is placed in product code (scripts/check-containment.mjs
//! check 13). Every unit compiles its own copy, and the claims below are the operating
//! system's file locks, so the copies exclude one another across threads, test binaries,
//! parallel runs of the suite and worktrees.
//!
//! Do not copy this file's logic into a test; mount it.
#![allow(dead_code)] // each compilation unit uses a different part of this file

use std::fs::{File, OpenOptions, TryLockError};
use std::net::{Ipv4Addr, TcpListener};
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// The first and one past the last port a rendezvous is claimed from. All of it lies below
/// the default ephemeral ranges of Windows and macOS (from 49152) and Linux (from 32768), so
/// no socket the operating system hands out for a connection or for a `:0` listener (a
/// joiner's listener, `PeerConfiguration`) is on one of these ports by default. A Linux host
/// that lowered `ip_local_port_range` below this range is checked for in
/// `tests/port_claims.rs`.
///
/// One range for every test of the crate, so no two sites can overlap: the claim namespace
/// is the file name of [`claim_path`]. Before #393 the ranges 20000-27999 and 23000-24098
/// overlapped, and this range is the one the contract harness already used (#387).
///
/// The range is split by operating system: Windows claims from 28000-29999 and every other
/// system from 30000-31999. The lock files live in each system's own temporary directory, so
/// a Windows run and a WSL run do not exclude each other; under mirrored networking they
/// share loopback, and disjoint ranges keep their claims from meeting (#432).
#[cfg(windows)]
pub const RENDEZVOUS_PORTS: std::ops::Range<u16> = 28_000..30_000;
#[cfg(not(windows))]
pub const RENDEZVOUS_PORTS: std::ops::Range<u16> = 30_000..32_000;

/// The lowest port of the default ephemeral ranges of Linux, Windows and macOS.
const LOWEST_DEFAULT_EPHEMERAL_PORT: u16 = 32_768;
const _: () = assert!(RENDEZVOUS_PORTS.end <= LOWEST_DEFAULT_EPHEMERAL_PORT);

/// The file whose exclusive lock is the claim on `port`. The name is the one the contract
/// harness used from #387 on, so a checkout that still has its own copy of the claim code
/// excludes this one.
pub fn claim_path(port: u16) -> PathBuf {
    std::env::temp_dir().join(format!("oac-contract-rendezvous-{port}.lock"))
}

fn unique() -> u64 {
    static CLAIMS: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    t ^ (u64::from(std::process::id()) << 32) ^ CLAIMS.fetch_add(1, Ordering::Relaxed)
}

/// A loopback port, claimed for as long as this value lives.
///
/// The rendezvous is a fixed port by design: every transport of a medium must be configured
/// with it before the first of them starts, so it cannot be left to Zenoh to pick with `:0`
/// the way a joiner's own listener is (and `Session::info().locators()`, which reports a
/// bound locator, is `#[zenoh_macros::unstable]`, zenoh-1.10.1/src/api/info.rs:122; this
/// crate does not enable `unstable`, Cargo.toml). Probing a port and releasing it left a
/// window in which another test took it (an ephemeral socket, or a parallel test or run),
/// and the rendezvous then refused the port as held by a program that is not an OAC
/// transport.
///
/// The port is claimed instead of probed: an exclusive lock on a file named for the port is
/// held until the claim is dropped. The lock is the operating system's, shared by every
/// process on the host and released when the holder dies, so concurrent tests never claim
/// the same port and the lock of a crashed run is not left held. The port is also bound once
/// to leave out one that a foreign program holds; that probe cannot race, as nothing that is
/// not a foreign program binds in this range.
///
/// Residuals:
/// - The lock files are never deleted: each is empty, at most one per port of the range
///   (2000 per system), and stays in the temporary directory.
/// - A Linux host whose `ip_local_port_range` starts below the end of the range is not
///   protected; the range test fails there loudly instead.
/// - A port the operating system excludes from binding (Windows reserved port ranges) is
///   skipped by the bind probe.
/// - The lock files live in the operating system's temporary directory, so two runs that
///   share loopback but not that directory (Windows and WSL under mirrored networking, or
///   containers) do not exclude each other. The window between the claim and the transport's
///   own bind is closed by retrying the first start (`Claimed::start_first`, #432).
/// - The claim holds against code that claims. A program that binds a port of the range
///   without claiming it is the foreign program the probe looks for, and is not excluded
///   for the life of the claim.
pub struct PortClaim {
    port: u16,
    _lock: File,
}

impl PortClaim {
    pub fn claim() -> PortClaim {
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
                Err(TryLockError::Error(e)) => panic!("locking a port claim: {e}"),
            }
            if TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok() {
                return PortClaim { port, _lock: file };
            }
        }
        panic!("no loopback port could be claimed in {RENDEZVOUS_PORTS:?}");
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

/// The refusal the transport answers a rendezvous port with when something that is not an
/// OAC transport accepts connections on it (`PeerTransport::start`, `InvalidConfiguration`).
/// The product keeps this refusal (a test of it is in `tests/peer_transport.rs`); a test
/// that only needs a free port may retry on exactly this text, never on another error.
pub const FOREIGN_HOLDER_REFUSAL: &str = "is held by a program that is not an OAC transport";

/// How many ports a test tries before it gives up on the refusal above.
pub const FOREIGN_HOLDER_ATTEMPTS: u32 = 5;

/// Something built around a claimed port that keeps the claim for as long as it lives, and
/// reads as the thing itself (`Deref`), so a test's `conf()` returns a configuration that
/// owns its port.
pub struct Claimed<T> {
    value: T,
    claim: PortClaim,
    rebuild: Option<Box<dyn Fn(u16) -> T + Send + Sync>>,
}

impl<T> Claimed<T> {
    pub fn port(&self) -> u16 {
        self.claim.port
    }

    /// Runs `start`, the first start of a transport on this port. A claim cannot be handed to
    /// the transport (the rendezvous is a fixed port the transport binds itself), so between
    /// the claim and that bind a program outside the claim protocol (another host's test run
    /// over a shared loopback, or any service) may take the port, and the transport then
    /// rightly refuses it as foreign (#432). When, and only when, `start` fails with that
    /// refusal, the port is given up, a fresh one is claimed and `start` runs again, at most
    /// [`FOREIGN_HOLDER_ATTEMPTS`] times in all; each retry is logged to stderr (the harness shows it for a failing test; run with `--nocapture` to see it on a passing one). Any other
    /// error, and the last refusal, is returned. Only valid for a value built with
    /// [`claimed_rebuildable`], and only for the first start: a joiner that is refused has
    /// no business on another port.
    pub fn start_first<R, E: std::fmt::Debug>(
        &mut self,
        mut start: impl FnMut(&T) -> Result<R, E>,
    ) -> Result<R, E> {
        let mut attempt = 1;
        loop {
            let outcome = start(&self.value);
            let refused =
                matches!(&outcome, Err(e) if format!("{e:?}").contains(FOREIGN_HOLDER_REFUSAL));
            if !refused || attempt >= FOREIGN_HOLDER_ATTEMPTS {
                return outcome;
            }
            let rebuild = self
                .rebuild
                .as_ref()
                .expect("start_first needs a value built with claimed_rebuildable");
            let claim = PortClaim::claim();
            eprintln!(
                "ports: loopback port {} was taken by a foreign program before the transport bound it \
                 (attempt {attempt} of {FOREIGN_HOLDER_ATTEMPTS}); retrying on port {}",
                self.claim.port, claim.port
            );
            self.value = rebuild(claim.port);
            self.claim = claim;
            attempt += 1;
        }
    }
}

impl<T> Deref for Claimed<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

/// Claims a port and builds a value from it; the value keeps the claim.
pub fn claimed<T>(build: impl FnOnce(u16) -> T) -> Claimed<T> {
    let claim = PortClaim::claim();
    Claimed {
        value: build(claim.port),
        claim,
        rebuild: None,
    }
}

/// As [`claimed`], and the value can be rebuilt on another port (`Claimed::start_first`).
pub fn claimed_rebuildable<T>(build: impl Fn(u16) -> T + Send + Sync + 'static) -> Claimed<T> {
    let claim = PortClaim::claim();
    Claimed {
        value: build(claim.port),
        claim,
        rebuild: Some(Box::new(build)),
    }
}
