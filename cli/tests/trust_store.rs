// SPDX-License-Identifier: Apache-2.0

//! The durable trust store, `trust.json` (#437, slice S1; `docs/planning/decisions/G-437-pairing.md`
//! §5, §10). The store is tested alone, in temporary directories (never the user's data
//! directory), and under real daemons started one after another over the in-memory transport,
//! each over its own namespace: the restart cases ([SEC-SEL-046], [SEC-SEL-047]) and the file
//! that stops a start. No test sleeps or polls.
//!
//! The write steps are made to fail one at a time through the `WriteSteps` seam.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use oac_cli::daemon::{Daemon, LogSink, Parts, StartError};
use oac_cli::ipc::Scope;
use oac_cli::trust_store::{
    FileTrustStore, MAX_TRUST_FILE_BYTES, OsSteps, TRUST_FILE_NAME, WriteSteps,
};
use oac_core::authorization::{
    Grant, LocalSide, OperatorConfirmed, PeerSide, WorkingDirectoryScope,
};
use oac_core::clock::{Clock, SystemClock};
use oac_core::ids::{SessionId, Timestamp, Token};
use oac_core::keys::{DeviceIdentity, DeviceKey, KeyStore, KeyStoreError, MemoryKeyStore};
use oac_core::pairing::{
    PairedPeer, PairingRecord, PairingSnapshot, PairingStore, PairingStoreError,
};
use oac_core::pipeline::PipelineConfig;
use oac_core::sealing::{
    AgreementKeys, AgreementPublicKey, AgreementStatement, ConfirmedValues, HeldStatement,
    MemoryAgreementKeyStore, StatementRefusal,
};
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};

// ---- fixtures ----------------------------------------------------------------------------

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A fresh directory under the system's temporary one (it does not exist yet), removed on drop.
struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!(
            "oac-trust-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        Temp(dir)
    }

    fn store(&self) -> FileTrustStore {
        FileTrustStore::new(self.0.clone(), self.0.clone())
    }

    fn file(&self) -> PathBuf {
        self.0.join(TRUST_FILE_NAME)
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn identity(principal: &str) -> DeviceIdentity {
    DeviceIdentity::new(DeviceKey::generate(), Token::parse(principal).unwrap())
}

fn at(text: &str) -> Timestamp {
    Timestamp::parse(text).unwrap()
}

fn now() -> Timestamp {
    SystemClock.now()
}

/// An agreement key that passes [SEC-SEL-013], made the way a device makes its own.
fn agreement_key(owner: &DeviceIdentity) -> AgreementPublicKey {
    let keys =
        AgreementKeys::load_or_generate(&MemoryAgreementKeyStore::new(), owner, &now()).unwrap();
    *keys.statement().claimed_values().unwrap().agreement_key()
}

fn sid(n: u8) -> SessionId {
    SessionId::from_random_octets([n; 16])
}

fn record(who: &DeviceIdentity, confirmed: Option<(u64, &str)>) -> PairingRecord {
    PairingRecord::from_store(
        who.principal().clone(),
        *who.public_key(),
        at("2026-11-04T10:00:00Z"),
    )
    .with_confirmed(confirmed.map(|(seq, issued)| {
        ConfirmedValues::from_store(agreement_key(who), seq, at(issued)).unwrap()
    }))
}

fn held(who: &DeviceIdentity, seq: u64) -> HeldStatement {
    HeldStatement::from_store(
        who.key_id().clone(),
        agreement_key(who),
        seq,
        at("2026-11-04T11:00:00Z"),
    )
    .unwrap()
}

/// A snapshot with every kind of thing the file keeps, in the order the file writes it.
fn full_snapshot() -> PairingSnapshot {
    let (a, b, c) = (identity("alpha"), identity("bravo"), identity("charlie"));
    let mut paired = vec![
        record(&a, Some((5, "2026-11-04T09:00:00Z"))),
        record(&b, None),
        record(&c, Some((0, "2026-11-04T08:30:00.250Z"))),
    ];
    paired.sort_by_key(PairingRecord::key_id);
    let mut statements = vec![held(&a, 5), held(&c, 7)];
    statements.sort_by(|x, y| x.key_id().cmp(y.key_id()));
    let (ka, kb) = (a.key_id().clone(), b.key_id().clone());
    let scope = WorkingDirectoryScope::new("C:\\work\\dir \"quoted\" \u{e9}\t/tab");
    let mut relay = vec![sid(9), sid(2), sid(5)];
    relay.sort();
    PairingSnapshot {
        paired,
        grants: vec![
            Grant::Inbound {
                writer: PeerSide::device(ka.clone()),
                target: LocalSide::Session(sid(1)),
            },
            Grant::Inbound {
                writer: PeerSide::session(ka.clone(), sid(3)),
                target: LocalSide::Scope(scope.clone()),
            },
            Grant::Inbound {
                writer: PeerSide::device(kb.clone()),
                target: LocalSide::Device,
            },
            Grant::Outbound {
                writer: LocalSide::Session(sid(4)),
                target: PeerSide::device(kb.clone()),
            },
            Grant::Outbound {
                writer: LocalSide::Scope(scope),
                target: PeerSide::session(kb.clone(), sid(6)),
            },
            Grant::Outbound {
                writer: LocalSide::Device,
                target: PeerSide::device(ka),
            },
        ],
        relay_enabled: relay,
        agreement_statements: statements,
    }
}

/// Writes `bytes` over the trust file the store already made (so that the mode, or the DACL,
/// is the store's).
fn overwrite(temp: &Temp, bytes: &[u8]) {
    temp.store().save(&PairingSnapshot::default()).unwrap();
    fs::write(temp.file(), bytes).unwrap();
}

#[cfg(unix)]
fn assert_failed(r: Result<PairingSnapshot, PairingStoreError>) -> String {
    match r {
        Err(PairingStoreError::Failed(m)) => m,
        other => panic!("expected Failed, got {other:?}"),
    }
}

fn assert_corrupt(r: Result<PairingSnapshot, PairingStoreError>) -> String {
    match r {
        Err(PairingStoreError::Corrupt(m)) => m,
        other => panic!("expected Corrupt, got {other:?}"),
    }
}

// ---- the store alone ---------------------------------------------------------------------

#[test]
fn snapshot_round_trips_every_grant_and_record() {
    let temp = Temp::new("roundtrip");
    let snapshot = full_snapshot();
    temp.store().save(&snapshot).unwrap();
    // A fresh store over the same directory reads back what was written.
    assert_eq!(temp.store().load().unwrap(), snapshot);

    // The bytes are canonical (RFC 8785) and do not depend on the order given: the same
    // snapshot, shuffled, gives the same file.
    let first = fs::read(temp.file()).unwrap();
    let text = String::from_utf8(first.clone()).unwrap();
    let parsed = oac_core::json::parse(text.as_bytes()).unwrap();
    assert_eq!(oac_core::canonical::canonical(&parsed).unwrap(), text);
    let mut shuffled = snapshot.clone();
    shuffled.paired.reverse();
    shuffled.relay_enabled.reverse();
    shuffled.agreement_statements.reverse();
    temp.store().save(&shuffled).unwrap();
    assert_eq!(fs::read(temp.file()).unwrap(), first);
    assert_eq!(temp.store().load().unwrap(), snapshot);

    // Every kind is there: a confirmed and an unconfirmed record, all six grant shapes, a
    // statement, a scope with characters that need escaping.
    assert!(
        snapshot.paired.iter().any(|r| r.confirmed().is_some())
            && snapshot.paired.iter().any(|r| r.confirmed().is_none())
    );
    assert_eq!(snapshot.grants.len(), 6);
}

#[test]
fn relay_enabled_round_trips() {
    let temp = Temp::new("relay");
    let snapshot = PairingSnapshot {
        relay_enabled: vec![sid(7), sid(1)],
        ..PairingSnapshot::default()
    };
    temp.store().save(&snapshot).unwrap();
    let loaded = temp.store().load().unwrap();
    assert_eq!(
        loaded.relay_enabled.iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([&sid(1), &sid(7)])
    );
    // Nothing else came with it: no peer, no grant (default deny).
    assert!(loaded.paired.is_empty() && loaded.grants.is_empty());
}

#[test]
fn a_missing_file_is_an_empty_store() {
    let temp = Temp::new("missing");
    assert_eq!(temp.store().load().unwrap(), PairingSnapshot::default());
    assert!(!temp.0.exists(), "a load makes nothing");
    // The directory exists, the file does not: the same.
    fs::create_dir_all(&temp.0).unwrap();
    assert_eq!(temp.store().load().unwrap(), PairingSnapshot::default());
}

#[test]
fn a_corrupt_or_unknown_version_file_is_refused_and_left_as_it_is() {
    for (why, bytes) in [
        ("garbage", b"\x00\x01 not json".to_vec()),
        ("empty", Vec::new()),
        ("truncated", br#"{"grants":[],"oac_tr"#.to_vec()),
        (
            "unknown version",
            br#"{"grants":[],"oac_trust_store":2,"paired":[],"relay_enabled":[],"statements":[]}"#
                .to_vec(),
        ),
        (
            "unknown member",
            br#"{"grants":[],"oac_trust_store":1,"paired":[],"relay_enabled":[],"statements":[],"x":1}"#
                .to_vec(),
        ),
        ("array", b"[]".to_vec()),
    ] {
        let temp = Temp::new("corrupt");
        overwrite(&temp, &bytes);
        let message = assert_corrupt(temp.store().load());
        assert!(message.contains(TRUST_FILE_NAME), "{why}: {message}");
        // Never reset, repaired or renamed away: not by the load, not by a save.
        assert_eq!(fs::read(temp.file()).unwrap(), bytes, "{why}");
        assert_eq!(temp.names(), vec![TRUST_FILE_NAME.to_owned()], "{why}");
    }
}

#[test]
fn an_oversize_file_is_refused() {
    let temp = Temp::new("oversize");
    let bytes = vec![b' '; usize::try_from(MAX_TRUST_FILE_BYTES).unwrap() + 1];
    overwrite(&temp, &bytes);
    let message = assert_corrupt(temp.store().load());
    assert!(message.contains("1 MiB"), "{message}");
    assert_eq!(fs::read(temp.file()).unwrap(), bytes, "left as it was");

    // At the limit the size is not what refuses it (the content is).
    let at_limit = vec![b' '; usize::try_from(MAX_TRUST_FILE_BYTES).unwrap()];
    fs::write(temp.file(), &at_limit).unwrap();
    let message = assert_corrupt(temp.store().load());
    assert!(!message.contains("1 MiB"), "{message}");

    // A snapshot that would not fit is not written, and the previous file stays.
    let small = PairingSnapshot {
        relay_enabled: vec![sid(1)],
        ..PairingSnapshot::default()
    };
    temp.store().save(&small).unwrap();
    let before = fs::read(temp.file()).unwrap();
    let too_big = PairingSnapshot {
        relay_enabled: (0..=u32::MAX)
            .take(40_000)
            .map(|n| {
                let mut octets = [0u8; 16];
                octets[..4].copy_from_slice(&n.to_be_bytes());
                SessionId::from_random_octets(octets)
            })
            .collect(),
        ..PairingSnapshot::default()
    };
    let e = temp.store().save(&too_big).unwrap_err();
    assert!(matches!(e, PairingStoreError::Failed(_)), "{e}");
    assert_eq!(fs::read(temp.file()).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn a_loose_mode_is_refused_not_repaired() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let temp = Temp::new("loose");
    let store = temp.store();
    let snapshot = full_snapshot();
    store.save(&snapshot).unwrap();
    assert_eq!(mode(&temp.0), 0o700);
    assert_eq!(mode(&temp.file()), 0o600);
    let before = fs::read(temp.file()).unwrap();

    // A loose file: refused by load and by save, and left exactly as it was (not repaired,
    // not replaced by a private copy of a different content).
    fs::set_permissions(temp.file(), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(assert_failed(store.load()).contains("accessible to other users"));
    let e = store.save(&PairingSnapshot::default()).unwrap_err();
    assert!(matches!(e, PairingStoreError::Failed(_)), "{e}");
    assert_eq!(mode(&temp.file()), 0o644, "not repaired");
    assert_eq!(fs::read(temp.file()).unwrap(), before, "not replaced");
    assert_eq!(
        temp.names(),
        vec![TRUST_FILE_NAME.to_owned()],
        "no leftovers"
    );

    // A loose directory: the same.
    fs::set_permissions(temp.file(), fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&temp.0, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(assert_failed(store.load()).contains("accessible to other users"));
    assert!(store.save(&PairingSnapshot::default()).is_err());
    assert_eq!(mode(&temp.0), 0o755, "not repaired");
    assert_eq!(fs::read(temp.file()).unwrap(), before, "not replaced");

    // Private again: it works, and the mode of the new file is 0600.
    fs::set_permissions(&temp.0, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(store.load().unwrap(), snapshot);
    store.save(&PairingSnapshot::default()).unwrap();
    assert_eq!(mode(&temp.file()), 0o600);
}

#[cfg(unix)]
#[test]
fn a_link_in_place_of_the_file_is_refused_and_not_followed() {
    let temp = Temp::new("link");
    let store = temp.store();
    store.save(&PairingSnapshot::default()).unwrap();
    let elsewhere = temp.0.join("elsewhere");
    fs::write(&elsewhere, b"untouched").unwrap();
    fs::remove_file(temp.file()).unwrap();
    std::os::unix::fs::symlink(&elsewhere, temp.file()).unwrap();
    assert!(matches!(
        store.load(),
        Err(PairingStoreError::Failed(m)) if m.contains("not a regular file")
    ));
    assert!(store.save(&full_snapshot()).is_err());
    assert_eq!(fs::read(&elsewhere).unwrap(), b"untouched");
}

#[test]
fn a_directory_in_place_of_the_file_is_refused() {
    let temp = Temp::new("dirfile");
    let store = temp.store();
    store.save(&PairingSnapshot::default()).unwrap();
    fs::remove_file(temp.file()).unwrap();
    fs::create_dir(temp.file()).unwrap();
    assert!(store.load().is_err());
    assert!(store.save(&full_snapshot()).is_err());
    assert!(temp.file().is_dir());
}

#[cfg(windows)]
mod windows_dacl {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACL, DACL_SECURITY_INFORMATION, GetAce, GetSecurityDescriptorControl,
        SE_DACL_PROTECTED,
    };

    /// `INHERITED_ACE`: the ACE came from the parent.
    const INHERITED_ACE: u8 = 0x10;

    /// Whether the DACL of `path` is protected (inherits nothing), and its ACEs' count and
    /// whether any is inherited.
    pub fn dacl(path: &Path) -> (bool, u16, bool) {
        let name: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut descriptor: *mut c_void = std::ptr::null_mut();
        // SAFETY: `name` is NUL-terminated; the out-pointers are locals.
        let status = unsafe {
            GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(status, 0, "GetNamedSecurityInfoW");
        let mut control = 0u16;
        let mut revision = 0u32;
        // SAFETY: `descriptor` was just returned and is freed below.
        assert_ne!(
            unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) },
            0
        );
        // SAFETY: `dacl` points into `descriptor`; each ACE read is within the ACL's count.
        let (count, inherited) = unsafe {
            let count = (*dacl).AceCount;
            let mut inherited = false;
            for i in 0..u32::from(count) {
                let mut ace: *mut c_void = std::ptr::null_mut();
                assert_ne!(GetAce(dacl, i, &mut ace), 0);
                let ace = ace.cast::<ACCESS_ALLOWED_ACE>();
                inherited |= (*ace).Header.AceFlags & INHERITED_ACE != 0;
            }
            (count, inherited)
        };
        // SAFETY: allocated by GetNamedSecurityInfoW, freed once.
        unsafe { LocalFree(descriptor) };
        (control & SE_DACL_PROTECTED != 0, count, inherited)
    }
}

#[cfg(windows)]
#[test]
fn the_dacl_on_trust_json_is_user_only() {
    let temp = Temp::new("dacl");
    let store = temp.store();
    store.save(&full_snapshot()).unwrap();
    // The file got its descriptor from the temporary file made inside the private directory,
    // which inherited the directory's one ACE: it must carry the protected, user-only DACL of
    // `open_private_file`, not the inherited ACE. Twice: the second save replaces the file.
    for round in 0..2 {
        let file = temp.file();
        assert!(
            oac_cli::logfile::is_private(&file).unwrap(),
            "round {round}"
        );
        let (protected, aces, inherited) = windows_dacl::dacl(&file);
        assert!(protected, "round {round}: the DACL is protected");
        assert_eq!(aces, 1, "round {round}: one ACE");
        assert!(!inherited, "round {round}: the ACE is not inherited");
        assert!(oac_cli::logfile::is_private(&temp.0).unwrap());
        store.save(&PairingSnapshot::default()).unwrap();
    }
    assert_eq!(temp.names(), vec![TRUST_FILE_NAME.to_owned()]);
}

// ---- the write steps ---------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Create,
    Write,
    Sync,
    Rename,
    SyncDirectory,
}

/// The real steps, recording each call, and making one fail.
struct Steps {
    fail: Option<Step>,
    calls: Mutex<Vec<Step>>,
}

impl Steps {
    fn new(fail: Option<Step>) -> Arc<Steps> {
        Arc::new(Steps {
            fail,
            calls: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> Vec<Step> {
        self.calls.lock().unwrap().clone()
    }

    fn enter(&self, step: Step) -> io::Result<()> {
        self.calls.lock().unwrap().push(step);
        if self.fail == Some(step) {
            Err(io::Error::other(format!("injected failure of {step:?}")))
        } else {
            Ok(())
        }
    }
}

impl WriteSteps for Steps {
    fn create(&self, path: &Path) -> io::Result<fs::File> {
        self.enter(Step::Create)?;
        OsSteps.create(path)
    }

    fn write(&self, file: &mut fs::File, bytes: &[u8]) -> io::Result<()> {
        // A failing write has written part of the file: what the target must never show.
        if self.fail == Some(Step::Write) {
            file.write_all(&bytes[..bytes.len() / 2])?;
        }
        self.enter(Step::Write)?;
        OsSteps.write(file, bytes)
    }

    fn sync(&self, file: &fs::File) -> io::Result<()> {
        self.enter(Step::Sync)?;
        OsSteps.sync(file)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.enter(Step::Rename)?;
        OsSteps.rename(from, to)
    }

    fn sync_directory(&self, directory: &Path) -> io::Result<()> {
        self.enter(Step::SyncDirectory)?;
        OsSteps.sync_directory(directory)
    }
}

fn store_with(temp: &Temp, steps: &Arc<Steps>) -> FileTrustStore {
    FileTrustStore::with_steps(temp.0.clone(), temp.0.clone(), steps.clone())
}

#[test]
fn a_save_runs_the_five_steps_once_each_in_order() {
    let temp = Temp::new("steps");
    let steps = Steps::new(None);
    store_with(&temp, &steps).save(&full_snapshot()).unwrap();
    assert_eq!(
        steps.calls(),
        [
            Step::Create,
            Step::Write,
            Step::Sync,
            Step::Rename,
            Step::SyncDirectory
        ]
    );
}

#[test]
fn a_failed_write_leaves_the_previous_file_whole() {
    let previous = PairingSnapshot {
        relay_enabled: vec![sid(1)],
        ..PairingSnapshot::default()
    };
    let next = full_snapshot();
    for step in [Step::Create, Step::Write, Step::Sync, Step::Rename] {
        let temp = Temp::new("failsave");
        temp.store().save(&previous).unwrap();
        let before = fs::read(temp.file()).unwrap();

        let steps = Steps::new(Some(step));
        let e = store_with(&temp, &steps).save(&next).unwrap_err();
        assert!(
            matches!(&e, PairingStoreError::Failed(m) if m.contains("injected failure")),
            "{step:?}: {e}"
        );
        assert_eq!(
            fs::read(temp.file()).unwrap(),
            before,
            "{step:?}: the previous file is byte for byte as it was"
        );
        assert_eq!(temp.store().load().unwrap(), previous, "{step:?}");
        assert_eq!(
            temp.names(),
            vec![TRUST_FILE_NAME.to_owned()],
            "{step:?}: no temporary file is left"
        );
        // The steps stop at the failure: nothing after it ran.
        assert_eq!(steps.calls().last(), Some(&step), "{step:?}");
    }
}

#[test]
fn a_failed_first_write_leaves_no_file() {
    for step in [Step::Create, Step::Write, Step::Sync, Step::Rename] {
        let temp = Temp::new("failfirst");
        let steps = Steps::new(Some(step));
        assert!(store_with(&temp, &steps).save(&full_snapshot()).is_err());
        assert!(!temp.file().exists(), "{step:?}");
        assert_eq!(temp.store().load().unwrap(), PairingSnapshot::default());
        assert!(temp.names().is_empty(), "{step:?}: {:?}", temp.names());
    }
}

#[test]
fn a_failed_directory_sync_is_reported_and_leaves_a_whole_file() {
    // The rename has happened: the new file is in place and whole. The save is still an
    // error, because it is not known to be durable (the revocation or pairing must not be
    // reported as saved).
    let previous = PairingSnapshot {
        relay_enabled: vec![sid(1)],
        ..PairingSnapshot::default()
    };
    let next = full_snapshot();
    let temp = Temp::new("faildir");
    temp.store().save(&previous).unwrap();
    let steps = Steps::new(Some(Step::SyncDirectory));
    let e = store_with(&temp, &steps).save(&next).unwrap_err();
    assert!(matches!(&e, PairingStoreError::Failed(m) if m.contains("may not be durable")));
    assert_eq!(steps.calls().last(), Some(&Step::SyncDirectory));
    assert_eq!(temp.store().load().unwrap(), next, "whole, and the new one");
    assert_eq!(temp.names(), vec![TRUST_FILE_NAME.to_owned()]);
}

#[test]
fn a_temporary_file_a_crash_left_is_swept_and_other_files_are_not() {
    let temp = Temp::new("sweep");
    let store = temp.store();
    store.save(&PairingSnapshot::default()).unwrap();
    let stale = temp
        .0
        .join("trust.json.00112233445566778899aabbccddeeff.tmp");
    let other = temp.0.join("trust.json.not-ours.tmp");
    fs::write(&stale, b"half a snapshot").unwrap();
    fs::write(&other, b"not ours").unwrap();
    store.save(&full_snapshot()).unwrap();
    assert!(!stale.exists(), "swept");
    assert!(other.exists(), "a name that is not ours is not touched");
}

#[test]
fn a_taken_temporary_name_is_skipped_and_its_file_not_touched() {
    // The first name is taken (by a file that is not ours to touch): the save goes on to the
    // next. The seam's `create` is exclusive, so the existing file is never opened.
    struct TakenFirst {
        taken: Mutex<Option<PathBuf>>,
    }
    impl WriteSteps for TakenFirst {
        fn create(&self, path: &Path) -> io::Result<fs::File> {
            let mut taken = self.taken.lock().unwrap();
            if taken.is_none() {
                fs::write(path, b"someone else's").unwrap();
                *taken = Some(path.to_owned());
            }
            OsSteps.create(path)
        }
        fn write(&self, f: &mut fs::File, b: &[u8]) -> io::Result<()> {
            OsSteps.write(f, b)
        }
        fn sync(&self, f: &fs::File) -> io::Result<()> {
            OsSteps.sync(f)
        }
        fn rename(&self, a: &Path, b: &Path) -> io::Result<()> {
            OsSteps.rename(a, b)
        }
        fn sync_directory(&self, d: &Path) -> io::Result<()> {
            OsSteps.sync_directory(d)
        }
    }
    let temp = Temp::new("taken");
    let steps = Arc::new(TakenFirst {
        taken: Mutex::new(None),
    });
    let store = FileTrustStore::with_steps(temp.0.clone(), temp.0.clone(), steps.clone());
    let snapshot = full_snapshot();
    store.save(&snapshot).unwrap();
    assert_eq!(temp.store().load().unwrap(), snapshot);
    // The file that held the first name was made after the sweep, so it is still there, and
    // the save wrote nowhere near it.
    let taken = steps.taken.lock().unwrap().clone().unwrap();
    assert_eq!(fs::read(taken).unwrap(), b"someone else's");
}

#[test]
fn a_foreign_device_key_id_is_reported_not_refused_by_the_store() {
    let temp = Temp::new("foreign-store");
    let (old, new) = (
        DeviceKey::generate().key_id(),
        DeviceKey::generate().key_id(),
    );
    let snapshot = full_snapshot();

    // The writer says which key it runs with; a later save carries the id.
    let writer = temp.store();
    assert_eq!(writer.device_key_in_use(&old), None, "no file yet");
    writer.save(&snapshot).unwrap();
    let text = fs::read_to_string(temp.file()).unwrap();
    assert!(
        text.contains(&format!("\"device_key_id\":\"{old}\"")),
        "{text}"
    );

    // The same key: nothing to report. Another key: reported, and the peers stay loaded.
    let same = temp.store();
    assert_eq!(same.load().unwrap(), snapshot);
    assert_eq!(same.device_key_in_use(&old), None);
    let other = temp.store();
    assert_eq!(other.load().unwrap(), snapshot, "not refused");
    assert_eq!(other.device_key_in_use(&new), Some(old));
    // From then on the file is written under the new id.
    other.save(&snapshot).unwrap();
    let text = fs::read_to_string(temp.file()).unwrap();
    assert!(
        text.contains(&format!("\"device_key_id\":\"{new}\"")),
        "{text}"
    );
    assert_eq!(temp.store().load().unwrap(), snapshot);

    // A file without the member (older, or hand-made) is not foreign to anyone.
    overwrite(
        &temp,
        br#"{"grants":[],"oac_trust_store":1,"paired":[],"relay_enabled":[],"statements":[]}"#,
    );
    let bare = temp.store();
    assert!(bare.load().is_ok());
    assert_eq!(bare.device_key_in_use(&new), None);
    // A member that is not a key id refuses the file.
    overwrite(
        &temp,
        br#"{"device_key_id":"nope","grants":[],"oac_trust_store":1,"paired":[],"relay_enabled":[],"statements":[]}"#,
    );
    assert_corrupt(temp.store().load());
}

#[test]
fn the_store_in_the_user_directory_is_namespaced() {
    // Only the names: nothing is read or made.
    let ordinary = FileTrustStore::for_user(None);
    let test = FileTrustStore::for_user(Some("abc1"));
    if let (Ok(ordinary), Ok(test)) = (ordinary, test) {
        let (o, t) = (ordinary.path(), test.path());
        assert_eq!(t.parent().unwrap().file_name().unwrap(), "ns-abc1");
        assert_eq!(t.parent().unwrap().parent(), o.parent());
        assert_eq!(o.file_name().unwrap(), TRUST_FILE_NAME);
    }
    let e = FileTrustStore::for_user(Some("Not A Scope!")).unwrap_err();
    assert!(matches!(
        e.kind(),
        io::ErrorKind::InvalidInput | io::ErrorKind::NotFound
    ));
}

// ---- under a daemon ----------------------------------------------------------------------

/// Hands the same device key to the daemons of one test, one after another.
struct SharedKeys(Arc<MemoryKeyStore>);

impl KeyStore for SharedKeys {
    fn load(&self) -> Result<Option<oac_core::keys::SecretSeed>, KeyStoreError> {
        self.0.load()
    }

    fn save(&self, seed: &oac_core::keys::SecretSeed) -> Result<(), KeyStoreError> {
        self.0.save(seed)
    }
}

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

/// Removes the namespace directory a Unix test made (Windows: nothing is made).
struct NamespaceDir(#[cfg(unix)] Option<PathBuf>);

impl Drop for NamespaceDir {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(dir) = self.0.take() {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

fn namespace_dir(scope: &Scope) -> NamespaceDir {
    #[cfg(unix)]
    {
        NamespaceDir(
            Path::new(&scope.endpoint("claude").display_name())
                .parent()
                .map(Path::to_path_buf),
        )
    }
    #[cfg(not(unix))]
    {
        let _ = scope;
        NamespaceDir()
    }
}

/// One daemon over `store` and the device key `keys`, in a namespace of its own.
struct Running {
    daemon: Daemon,
    log: Captured,
    _dir: NamespaceDir,
}

async fn start(store: FileTrustStore, keys: &Arc<MemoryKeyStore>) -> Result<Running, StartError> {
    let namespace = format!(
        "ts{}n{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    );
    let scope = Scope::for_user_in(Some(&namespace)).unwrap();
    let dir = namespace_dir(&scope);
    let network = MemoryNetwork::new();
    let net = network.clone();
    let log = Captured::default();
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let daemon = Daemon::start(Parts {
        scope,
        key_store: Box::new(SharedKeys(keys.clone())),
        pairing_store: Box::new(store),
        agreement_store: Box::new(MemoryAgreementKeyStore::new()),
        principal: Token::parse("test").unwrap(),
        transport: Arc::new(MemoryTransport::new()),
        transport_configuration: MemoryConfiguration::wrap(&network),
        clock,
        monotonic: Arc::new(move || net.now()),
        config: PipelineConfig::default(),
        harnesses: Vec::new(),
        log: LogSink::new(log.clone()),
        codex: None,
    })
    .await?;
    Ok(Running {
        daemon,
        log,
        _dir: dir,
    })
}

impl Running {
    fn engine<R>(
        &self,
        f: impl FnOnce(&mut oac_core::authorization::AuthorizationEngine) -> R,
    ) -> R {
        self.daemon.pipelines().with_engine(f)
    }

    /// Pairs `peer` (with `statement`, if any) through the daemon's own store.
    fn pair(&self, peer: &DeviceIdentity, statement: Option<AgreementStatement>) {
        let mut paired = PairedPeer::by_key_id_comparison(
            peer.principal().clone(),
            *peer.public_key(),
            peer.key_id(),
            now(),
            OperatorConfirmed::by_operator(),
        )
        .unwrap();
        if let Some(s) = statement {
            paired = paired.with_agreement_statement(s);
        }
        let store = self.daemon.pairing_store().clone();
        self.engine(|e| e.pair(paired, &*store).unwrap());
    }

    fn admit(&self, statement: &AgreementStatement) -> Result<u64, StatementRefusal> {
        let store = self.daemon.pairing_store().clone();
        self.engine(|e| {
            e.admit_statement(statement, &*store)
                .map(HeldStatement::seq)
        })
    }
}

fn start_blocking_error(r: Result<Running, StartError>) -> StartError {
    match r {
        Err(e) => e,
        Ok(_) => panic!("the daemon started"),
    }
}

#[tokio::test]
async fn confirmed_values_survive_a_restart_and_an_older_statement_is_refused() {
    // [SEC-SEL-046], [SEC-SEL-045]
    let temp = Temp::new("sel046");
    let keys = Arc::new(MemoryKeyStore::new());
    let peer = identity("peer");
    let key = agreement_key(&peer);
    let statement = |seq: u64| AgreementStatement::issue(&peer, &key, seq, &now()).unwrap();

    // Pair with a statement (seq 5), and stop the daemon.
    let first = start(temp.store(), &keys).await.unwrap();
    first.pair(&peer, Some(statement(5)));
    first.daemon.shutdown().await;

    // Another daemon on the same file: the key is back with the values its pairing
    // confirmed, and the statement it held.
    let second = start(temp.store(), &keys).await.unwrap();
    second.engine(|e| {
        assert!(
            e.trusted_keys().get(peer.key_id()).is_some(),
            "still paired"
        );
        assert_eq!(
            e.held_statement(peer.key_id()).map(HeldStatement::seq),
            Some(5)
        );
        let snapshot = e.snapshot();
        let confirmed = snapshot.paired[0].confirmed().expect("confirmed values");
        assert_eq!((confirmed.seq(), confirmed.agreement_key()), (5, &key));
    });
    // An older statement is refused: as not newer than the held one ...
    assert_eq!(second.admit(&statement(3)), Err(StatementRefusal::NotNewer));
    // ... and a newer one is admitted and saved.
    assert_eq!(second.admit(&statement(6)), Ok(6));
    second.daemon.shutdown().await;

    // With the held statement lost from the file, the confirmed values alone are the floor:
    // a lower `seq` is `BelowConfirmed` (the statement that was held cannot be the reason).
    let mut snapshot = temp.store().load().unwrap();
    assert_eq!(snapshot.agreement_statements[0].seq(), 6);
    snapshot.agreement_statements.clear();
    temp.store().save(&snapshot).unwrap();
    let third = start(temp.store(), &keys).await.unwrap();
    third.engine(|e| assert!(e.held_statement(peer.key_id()).is_none()));
    assert_eq!(
        third.admit(&statement(3)),
        Err(StatementRefusal::BelowConfirmed)
    );
    assert_eq!(
        third.admit(&statement(4)),
        Err(StatementRefusal::BelowConfirmed),
        "below the confirmed seq 5"
    );
    assert_eq!(
        third.admit(&statement(5)),
        Ok(5),
        "the confirmed one itself"
    );
    third.daemon.shutdown().await;
}

#[tokio::test]
async fn after_removal_and_restart_the_record_and_confirmed_values_are_gone() {
    // [SEC-SEL-047]
    let temp = Temp::new("sel047");
    let keys = Arc::new(MemoryKeyStore::new());
    let peer = identity("peer");
    let key = agreement_key(&peer);

    let first = start(temp.store(), &keys).await.unwrap();
    first.pair(
        &peer,
        Some(AgreementStatement::issue(&peer, &key, 5, &now()).unwrap()),
    );
    let store = first.daemon.pairing_store().clone();
    let removal = first
        .engine(|e| e.remove_key(peer.key_id(), OperatorConfirmed::by_operator(), &*store))
        .unwrap();
    assert_eq!(removal.confirmed.map(|c| c.seq()), Some(5));
    first.daemon.shutdown().await;

    // The file holds neither the record, nor the confirmed values, nor the statement.
    let on_disk = temp.store().load().unwrap();
    assert!(on_disk.paired.is_empty() && on_disk.agreement_statements.is_empty());
    let text = fs::read_to_string(temp.file()).unwrap();
    assert!(!text.contains(peer.key_id().as_str()), "{text}");
    assert!(!text.contains(&key.to_base64url()), "{text}");

    // A restart on the file: the key is not trusted, nothing is held, and the key admits no
    // statement ([SEC-SEL-049]) until it is paired again; a repeated pairing confirms new
    // values, and the removed floor does not come back.
    let second = start(temp.store(), &keys).await.unwrap();
    second.engine(|e| {
        assert!(e.trusted_keys().get(peer.key_id()).is_none());
        assert!(e.snapshot().paired.is_empty());
        assert!(e.held_statement(peer.key_id()).is_none());
    });
    let fresh = AgreementStatement::issue(&peer, &key, 2, &now()).unwrap();
    assert_eq!(second.admit(&fresh), Err(StatementRefusal::Signature));
    second.pair(&peer, Some(fresh));
    second.engine(|e| {
        let snapshot = e.snapshot();
        assert_eq!(
            snapshot.paired[0].confirmed().map(ConfirmedValues::seq),
            Some(2)
        );
    });
    second.daemon.shutdown().await;
}

#[tokio::test]
async fn a_corrupt_or_unknown_version_file_stops_the_daemon_and_is_not_reset() {
    for (why, bytes) in [
        ("garbage", b"this is not json".to_vec()),
        (
            "unknown version",
            br#"{"grants":[],"oac_trust_store":2,"paired":[],"relay_enabled":[],"statements":[]}"#
                .to_vec(),
        ),
        (
            "oversize",
            vec![b' '; usize::try_from(MAX_TRUST_FILE_BYTES).unwrap() + 1],
        ),
    ] {
        let temp = Temp::new("stops");
        let keys = Arc::new(MemoryKeyStore::new());
        overwrite(&temp, &bytes);
        let e = start_blocking_error(start(temp.store(), &keys).await);
        let message = e.to_string();
        assert!(
            matches!(e, StartError::Pairing(_)),
            "{why}: expected the pairing store to stop the start, got {message}"
        );
        assert!(message.contains(TRUST_FILE_NAME), "{why}: {message}");
        assert_eq!(fs::read(temp.file()).unwrap(), bytes, "{why}: not reset");
        assert_eq!(temp.names(), vec![TRUST_FILE_NAME.to_owned()], "{why}");
    }
}

#[tokio::test]
async fn a_loose_file_stops_the_daemon_and_is_left_as_it_is() {
    // A file open to others cannot be told from one an attacker wrote: refused. On Windows
    // the equivalent is a DACL with more than the user's ACE, which a copy made outside the
    // private directory has.
    let temp = Temp::new("loosestart");
    let keys = Arc::new(MemoryKeyStore::new());
    temp.store().save(&full_snapshot()).unwrap();
    let before = fs::read(temp.file()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(temp.file(), fs::Permissions::from_mode(0o666)).unwrap();
    }
    #[cfg(windows)]
    {
        let outside = std::env::temp_dir().join(format!(
            "oac-trust-loose-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::write(&outside, &before).unwrap();
        if oac_cli::logfile::is_private(&outside).unwrap() {
            // This temporary directory is itself user-only: nothing loose to make.
            let _ = fs::remove_file(&outside);
            return;
        }
        fs::remove_file(temp.file()).unwrap();
        fs::rename(&outside, temp.file()).unwrap();
    }
    let e = start_blocking_error(start(temp.store(), &keys).await);
    assert!(matches!(e, StartError::Pairing(_)), "{e}");
    assert_eq!(fs::read(temp.file()).unwrap(), before);
}

#[tokio::test]
async fn a_foreign_device_key_id_is_reported_not_refused() {
    let temp = Temp::new("foreign");
    let (keys_a, keys_b) = (
        Arc::new(MemoryKeyStore::new()),
        Arc::new(MemoryKeyStore::new()),
    );
    let peer = identity("peer");

    let first = start(temp.store(), &keys_a).await.unwrap();
    assert_eq!(first.daemon.trust_store_foreign_key(), None, "no file yet");
    first.pair(&peer, None);
    let own_a = first.engine(|e| e.own_key_id().clone());
    first.daemon.shutdown().await;
    assert!(
        fs::read_to_string(temp.file())
            .unwrap()
            .contains(own_a.as_str())
    );

    // The same key again: nothing to report.
    let again = start(temp.store(), &keys_a).await.unwrap();
    assert_eq!(again.daemon.trust_store_foreign_key(), None);
    again.daemon.shutdown().await;

    // Another device key (a regenerated one): the daemon starts, the peer is still trusted,
    // one warning is logged and the daemon reports the old key id.
    let second = start(temp.store(), &keys_b).await.unwrap();
    let own_b = second.engine(|e| e.own_key_id().clone());
    assert_ne!(own_a, own_b);
    assert_eq!(second.daemon.trust_store_foreign_key(), Some(&own_a));
    second.engine(|e| assert!(e.trusted_keys().get(peer.key_id()).is_some()));
    let log = second.log.text();
    assert_eq!(
        log.matches("warning: the trust store was written under")
            .count(),
        1,
        "{log}"
    );
    assert!(
        log.contains(own_a.as_str()) && log.contains(own_b.as_str()),
        "{log}"
    );
    // The next save writes the new key id.
    let other = identity("other");
    second.pair(&other, None);
    second.daemon.shutdown().await;
    let text = fs::read_to_string(temp.file()).unwrap();
    assert!(
        text.contains(&format!("\"device_key_id\":\"{own_b}\"")),
        "{text}"
    );
    assert!(!text.contains(own_a.as_str()), "{text}");
}

#[tokio::test]
async fn the_daemon_keeps_the_store_it_was_started_with() {
    // What a later save goes through: the store itself, not a copy that was read once.
    let temp = Temp::new("keeps");
    let keys = Arc::new(MemoryKeyStore::new());
    let running = start(temp.store(), &keys).await.unwrap();
    assert!(!temp.file().exists(), "a start saves nothing");
    let granted = identity("granted");
    running.pair(&granted, None);
    assert!(temp.file().exists());
    let on_disk = running.daemon.pairing_store().load().unwrap();
    assert_eq!(on_disk.paired.len(), 1);
    assert_eq!(on_disk.paired[0].key_id(), granted.key_id().clone());
    running.daemon.shutdown().await;
}
