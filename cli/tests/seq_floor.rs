// SPDX-License-Identifier: Apache-2.0

//! The agreement `seq` floor in `trust.json` (#437 S1c; `docs/planning/decisions/G-437-pairing.md`
//! §7, T10): a rolled-back or lost agreement-key file must not make the device issue a `seq` it
//! has already used, and the floor must be durable before the statement exists ([SEC-SEL-043]).
//!
//! Every test uses a temporary directory and a test device key; nothing touches the user's own
//! data directory or credential store. No test sleeps or polls.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use oac_adapter_claude::ClaudeAdapter;
use oac_cli::agreement_file::{AgreementFileSource, AgreementFileStore};
use oac_cli::daemon::{Daemon, LogSink, Parts};
use oac_cli::ipc::Scope;
use oac_cli::seq_floor::FlooredAgreementStore;
use oac_cli::trust_store::{FileTrustStore, OsSteps, TRUST_FILE_NAME, WriteSteps};
use oac_core::clock::{Clock, SystemClock};
use oac_core::ids::{KeyId, Timestamp, Token};
use oac_core::keys::{DeviceIdentity, DeviceKey, KeyStore, KeyStoreError, SecretSeed};
use oac_core::pairing::{PairingSnapshot, PairingStore, PairingStoreError};
use oac_core::pipeline::PipelineConfig;
use oac_core::sealing::{
    AgreementKeyError, AgreementKeyState, AgreementKeyStore, AgreementKeys, AgreementStatement,
    MAX_STATEMENT_SEQ, MemoryAgreementKeyStore,
};
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A fresh directory under the system's temporary one, removed on drop.
struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!(
            "oac-seq-floor-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Temp(dir)
    }

    fn dir(&self) -> &Path {
        &self.0
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn identity(n: u8) -> DeviceIdentity {
    let key = DeviceKey::from_seed(&SecretSeed::from_octets(&mut [n; 32]));
    DeviceIdentity::new(key, Token::parse("device").unwrap())
}

fn at(text: &str) -> Timestamp {
    Timestamp::parse(text).unwrap()
}

fn seq_of(s: &AgreementStatement) -> u64 {
    s.claimed_values().expect("a well-formed statement").seq()
}

fn file_store(dir: &Path, id: &DeviceIdentity) -> AgreementFileStore {
    AgreementFileStore::new(
        dir.to_path_buf(),
        dir.to_path_buf(),
        id.at_rest(),
        Arc::new(|_| {}),
    )
}

fn trust(dir: &Path) -> Arc<FileTrustStore> {
    Arc::new(FileTrustStore::new(dir.to_path_buf(), dir.to_path_buf()))
}

/// The real agreement-key file and `trust.json` in `dir`, joined as the daemon joins them.
fn floored(dir: &Path, id: &DeviceIdentity, trust: &Arc<FileTrustStore>) -> FlooredAgreementStore {
    FlooredAgreementStore::new(
        Box::new(file_store(dir, id)),
        trust.clone(),
        id.key_id().clone(),
    )
}

/// A start: the keys the floored store gives at `when`.
fn start(
    dir: &Path,
    id: &DeviceIdentity,
    trust: &Arc<FileTrustStore>,
    when: &str,
) -> Result<AgreementKeys, AgreementKeyError> {
    AgreementKeys::load_or_generate(&floored(dir, id, trust), id, &at(when))
}

/// **T10, [SEC-SEL-040].** A validly sealed older agreement file comes back: the floor makes
/// the next statement `floor + 1`, not a `seq` the peers already hold.
#[test]
fn a_rolled_back_file_issues_the_floor_plus_one() {
    let temp = Temp::new("rollback");
    let dir = temp.dir();
    let id = identity(1);
    let trust = trust(dir);

    let mut keys = start(dir, &id, &trust, "2026-11-04T10:00:00Z").unwrap();
    assert_eq!(seq_of(keys.statement()), 0);
    let file = file_store(dir, &id).path();
    let old = fs::read(&file).unwrap();
    let floored = floored(dir, &id, &trust);
    keys.replace(&floored, &id, &at("2026-11-04T11:00:00Z"))
        .unwrap();
    keys.replace(&floored, &id, &at("2026-11-04T12:00:00Z"))
        .unwrap();
    assert_eq!(keys.highest_seq(), 2);

    // The backup restored: the file is the one that held seq 0 (it opens, and cannot be told
    // from the current one).
    fs::write(&file, &old).unwrap();
    assert_eq!(
        file_store(dir, &id).load().unwrap().unwrap().highest_seq,
        Some(0),
        "the older file opens and holds the lower count"
    );
    // Without the floor, the next start would reuse seq 1.
    let unprotected =
        AgreementKeys::load_or_generate(&file_store(dir, &id), &id, &at("2026-11-04T13:00:00Z"))
            .unwrap();
    assert_eq!(
        seq_of(unprotected.statement()),
        0,
        "(the control: no floor)"
    );
    fs::write(&file, &old).unwrap();

    // With it, the start issues seq 3 under a new key, and saves it.
    let keys = start(dir, &id, &trust, "2026-11-04T13:00:00Z").unwrap();
    assert_eq!(seq_of(keys.statement()), 3);
    assert_eq!(
        file_store(dir, &id).load().unwrap().unwrap().highest_seq,
        Some(3)
    );
    assert_eq!(
        trust.agreement_seq_floor(id.key_id()).unwrap(),
        Some(3),
        "the floor follows the count"
    );
    // The next start finds the file and the floor agree.
    let keys = start(dir, &id, &trust, "2026-11-04T14:00:00Z").unwrap();
    assert_eq!(seq_of(keys.statement()), 3);
}

/// **T10.** The agreement file is gone but the floor stays: `floor + 1`, not a first start.
#[test]
fn a_floor_without_a_file_issues_the_floor_plus_one() {
    let temp = Temp::new("nofile");
    let dir = temp.dir();
    let id = identity(1);
    let trust = trust(dir);

    let mut keys = start(dir, &id, &trust, "2026-11-04T10:00:00Z").unwrap();
    keys.replace(&floored(dir, &id, &trust), &id, &at("2026-11-04T11:00:00Z"))
        .unwrap();
    assert_eq!(keys.highest_seq(), 1);
    fs::remove_file(file_store(dir, &id).path()).unwrap();

    let keys = start(dir, &id, &trust, "2026-11-04T12:00:00Z").unwrap();
    assert_eq!(seq_of(keys.statement()), 2);
}

/// A step list that records what the agreement store and the floor were asked, in order.
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<String>>>);

impl Events {
    fn push(&self, e: String) {
        self.0.lock().unwrap().push(e);
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

/// A [`PairingStore`] that keeps only a floor, and records and can fail its writes.
struct RecordingFloor {
    events: Events,
    floor: Mutex<Option<(KeyId, u64)>>,
    fail: AtomicBool,
}

impl PairingStore for RecordingFloor {
    fn load(&self) -> Result<PairingSnapshot, PairingStoreError> {
        Ok(PairingSnapshot::default())
    }

    fn save(&self, _snapshot: &PairingSnapshot) -> Result<(), PairingStoreError> {
        Ok(())
    }

    fn agreement_seq_floor(&self, key_id: &KeyId) -> Result<Option<u64>, PairingStoreError> {
        Ok(self
            .floor
            .lock()
            .unwrap()
            .clone()
            .filter(|(k, _)| k == key_id)
            .map(|(_, s)| s))
    }

    fn record_agreement_seq_floor(
        &self,
        key_id: &KeyId,
        seq: u64,
    ) -> Result<(), PairingStoreError> {
        if self.fail.load(Ordering::SeqCst) {
            self.events.push(format!("floor {seq} failed"));
            return Err(PairingStoreError::Failed("the floor write failed".into()));
        }
        self.events.push(format!("floor {seq}"));
        *self.floor.lock().unwrap() = Some((key_id.clone(), seq));
        Ok(())
    }
}

/// An [`AgreementKeyStore`] that records its saves.
struct RecordingKeys {
    events: Events,
    inner: MemoryAgreementKeyStore,
}

impl AgreementKeyStore for RecordingKeys {
    fn load(&self) -> Result<Option<AgreementKeyState>, KeyStoreError> {
        self.inner.load()
    }

    fn save(&self, state: &AgreementKeyState) -> Result<(), KeyStoreError> {
        self.events
            .push(format!("file {}", state.highest_seq.unwrap()));
        self.inner.save(state)
    }
}

/// **[SEC-SEL-043].** The floor is written before the agreement file, which is written before
/// the statement is returned; a floor that cannot be written means no statement.
#[test]
fn the_floor_is_written_before_the_statement_exists() {
    let id = identity(1);
    let events = Events::default();
    let floor = Arc::new(RecordingFloor {
        events: events.clone(),
        floor: Mutex::new(None),
        fail: AtomicBool::new(false),
    });
    let store = FlooredAgreementStore::new(
        Box::new(RecordingKeys {
            events: events.clone(),
            inner: MemoryAgreementKeyStore::new(),
        }),
        floor.clone(),
        id.key_id().clone(),
    );

    // A first start and a replace: each writes the floor, then the file.
    let mut keys =
        AgreementKeys::load_or_generate(&store, &id, &at("2026-11-04T10:00:00Z")).unwrap();
    assert_eq!(events.take(), ["floor 0", "file 0"]);
    keys.replace(&store, &id, &at("2026-11-04T11:00:00Z"))
        .unwrap();
    assert_eq!(events.take(), ["floor 1", "file 1"]);

    // The floor cannot be written: the replace fails, the file is not written, and the keys
    // still hold the statement they held.
    floor.fail.store(true, Ordering::SeqCst);
    let before = seq_of(keys.statement());
    let e = keys
        .replace(&store, &id, &at("2026-11-04T12:00:00Z"))
        .unwrap_err();
    assert!(
        matches!(e, AgreementKeyError::Store(KeyStoreError::Failed(_))),
        "{e}"
    );
    assert_eq!(
        events.take(),
        ["floor 2 failed"],
        "the file was not written"
    );
    assert_eq!(seq_of(keys.statement()), before);
    assert_eq!(keys.highest_seq(), 1);

    // A first start with an unwritable floor issues no statement at all.
    let other = identity(2);
    let events = Events::default();
    let floor = Arc::new(RecordingFloor {
        events: events.clone(),
        floor: Mutex::new(None),
        fail: AtomicBool::new(true),
    });
    let store = FlooredAgreementStore::new(
        Box::new(RecordingKeys {
            events: events.clone(),
            inner: MemoryAgreementKeyStore::new(),
        }),
        floor,
        other.key_id().clone(),
    );
    assert!(AgreementKeys::load_or_generate(&store, &other, &at("2026-11-04T10:00:00Z")).is_err());
    assert_eq!(events.take(), ["floor 0 failed"]);
}

/// Write steps that are the operating system's, except that the rename can be made to fail.
struct FailRename(AtomicBool);

impl WriteSteps for FailRename {
    fn create(&self, path: &Path) -> io::Result<fs::File> {
        OsSteps.create(path)
    }

    fn write(&self, file: &mut fs::File, bytes: &[u8]) -> io::Result<()> {
        OsSteps.write(file, bytes)
    }

    fn sync(&self, file: &fs::File) -> io::Result<()> {
        OsSteps.sync(file)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        if self.0.load(Ordering::SeqCst) {
            Err(io::Error::other("injected rename failure"))
        } else {
            OsSteps.rename(from, to)
        }
    }

    fn sync_directory(&self, directory: &Path) -> io::Result<()> {
        OsSteps.sync_directory(directory)
    }
}

/// The same, over the real files: a `trust.json` that cannot be replaced leaves the agreement
/// file as it was and issues no statement; the next start still finds the counts consistent.
#[test]
fn a_failed_floor_write_leaves_the_agreement_file_untouched() {
    let temp = Temp::new("failed-floor");
    let dir = temp.dir();
    let id = identity(1);
    let steps = Arc::new(FailRename(AtomicBool::new(false)));
    let trust = Arc::new(FileTrustStore::with_steps(
        dir.to_path_buf(),
        dir.to_path_buf(),
        steps.clone(),
    ));
    let mut keys = start(dir, &id, &trust, "2026-11-04T10:00:00Z").unwrap();
    let file = file_store(dir, &id).path();
    let before = fs::read(&file).unwrap();

    steps.0.store(true, Ordering::SeqCst);
    assert!(
        keys.replace(&floored(dir, &id, &trust), &id, &at("2026-11-04T11:00:00Z"))
            .is_err()
    );
    assert_eq!(fs::read(&file).unwrap(), before, "the file is untouched");
    assert_eq!(
        trust.agreement_seq_floor(id.key_id()).unwrap(),
        Some(0),
        "the floor is untouched"
    );
    assert_eq!(keys.highest_seq(), 0);
    steps.0.store(false, Ordering::SeqCst);
    let keys = start(dir, &id, &trust, "2026-11-04T12:00:00Z").unwrap();
    assert_eq!(seq_of(keys.statement()), 0);
}

/// A floor from before the count (a store that had a count when S1c arrived) is recorded at
/// the start, so a rollback before the next statement is noticed too.
#[test]
fn a_count_kept_before_the_floor_is_recorded_at_the_start() {
    let temp = Temp::new("ensure");
    let dir = temp.dir();
    let id = identity(1);
    // The count exists in the file alone: no floor, as before S1c.
    let mut keys =
        AgreementKeys::load_or_generate(&file_store(dir, &id), &id, &at("2026-11-04T10:00:00Z"))
            .unwrap();
    keys.replace(&file_store(dir, &id), &id, &at("2026-11-04T11:00:00Z"))
        .unwrap();
    let trust = trust(dir);
    assert_eq!(trust.agreement_seq_floor(id.key_id()).unwrap(), None);

    let floored = floored(dir, &id, &trust);
    let keys = AgreementKeys::load_or_generate(&floored, &id, &at("2026-11-04T12:00:00Z")).unwrap();
    assert_eq!(keys.highest_seq(), 1);
    floored.ensure_floor(keys.highest_seq()).unwrap();
    assert_eq!(trust.agreement_seq_floor(id.key_id()).unwrap(), Some(1));
    // Once recorded, it is not written again.
    let text = fs::read(dir.join(TRUST_FILE_NAME)).unwrap();
    floored.ensure_floor(1).unwrap();
    assert_eq!(fs::read(dir.join(TRUST_FILE_NAME)).unwrap(), text);
}

/// **L7.** A floor that names another key id is ignored: a regenerated device key starts at
/// `seq` 0, and its own floor replaces the old one.
#[test]
fn a_floor_for_another_key_id_is_ignored() {
    let temp = Temp::new("other-key");
    let dir = temp.dir();
    let (old, new) = (identity(1), identity(2));
    let trust = trust(dir);

    let mut keys = start(dir, &old, &trust, "2026-11-04T10:00:00Z").unwrap();
    let floored_old = floored(dir, &old, &trust);
    keys.replace(&floored_old, &old, &at("2026-11-04T11:00:00Z"))
        .unwrap();
    keys.replace(&floored_old, &old, &at("2026-11-04T12:00:00Z"))
        .unwrap();
    assert_eq!(trust.agreement_seq_floor(old.key_id()).unwrap(), Some(2));
    assert_eq!(
        trust.agreement_seq_floor(new.key_id()).unwrap(),
        None,
        "the old key's floor is not the new key's"
    );

    // The regenerated key starts its own count, from the same trust.json.
    let keys = start(dir, &new, &trust, "2026-11-04T13:00:00Z").unwrap();
    assert_eq!(seq_of(keys.statement()), 0);
    assert_eq!(trust.agreement_seq_floor(new.key_id()).unwrap(), Some(0));
    // A fresh store over the same file reads the same.
    let reopened = FileTrustStore::new(dir.to_path_buf(), dir.to_path_buf());
    assert_eq!(reopened.agreement_seq_floor(new.key_id()).unwrap(), Some(0));
    assert_eq!(reopened.agreement_seq_floor(old.key_id()).unwrap(), None);
    // The old key's own file was left where it was.
    assert!(file_store(dir, &old).path().is_file());
}

/// **[SEC-SEL-052], L6.** A floor at the maximum `seq` refuses the start; only the restart
/// clears it, and it clears the floor with the count.
#[test]
fn a_floor_at_the_maximum_seq_refuses_the_start_until_the_restart() {
    let temp = Temp::new("maximum");
    let dir = temp.dir();
    let id = identity(1);
    let trust = trust(dir);
    start(dir, &id, &trust, "2026-11-04T10:00:00Z").unwrap();

    // A same-user write raises the floor to the maximum.
    trust
        .record_agreement_seq_floor(id.key_id(), MAX_STATEMENT_SEQ)
        .unwrap();
    let e = start(dir, &id, &trust, "2026-11-04T11:00:00Z").unwrap_err();
    assert!(matches!(e, AgreementKeyError::SeqExhausted), "{e}");
    // Refused again, not reset: the file is as it was.
    assert_eq!(
        file_store(dir, &id).load().unwrap().unwrap().highest_seq,
        Some(0)
    );

    // The restart is allowed at the maximum, and starts the count again.
    let keys =
        AgreementKeys::restart_count(&floored(dir, &id, &trust), &id, &at("2026-11-04T12:00:00Z"))
            .unwrap();
    assert_eq!(seq_of(keys.statement()), 0);
    assert_eq!(trust.agreement_seq_floor(id.key_id()).unwrap(), Some(0));
    let keys = start(dir, &id, &trust, "2026-11-04T13:00:00Z").unwrap();
    assert_eq!(seq_of(keys.statement()), 0);

    // Below the maximum, the restart is refused.
    let e =
        AgreementKeys::restart_count(&floored(dir, &id, &trust), &id, &at("2026-11-04T14:00:00Z"))
            .unwrap_err();
    assert!(matches!(e, AgreementKeyError::RestartNotAllowed), "{e}");
}

/// A save of the snapshot carries the floor, and recording the floor carries the snapshot.
#[test]
fn the_floor_and_the_snapshot_do_not_lose_each_other() {
    let temp = Temp::new("carry");
    let dir = temp.dir();
    let id = identity(1);
    let key = id.key_id();

    let store = trust(dir);
    // Recording with no file yet makes one, with the empty snapshot.
    store.record_agreement_seq_floor(key, 4).unwrap();
    assert_eq!(store.load().unwrap(), PairingSnapshot::default());

    // The engine's saves (here, an equal snapshot) leave the floor in the file.
    store.save(&PairingSnapshot::default()).unwrap();
    let reopened = FileTrustStore::new(dir.to_path_buf(), dir.to_path_buf());
    assert_eq!(reopened.agreement_seq_floor(key).unwrap(), Some(4));

    // A store that never loaded still carries the file's floor through a save.
    let fresh = FileTrustStore::new(dir.to_path_buf(), dir.to_path_buf());
    fresh.save(&PairingSnapshot::default()).unwrap();
    let again = FileTrustStore::new(dir.to_path_buf(), dir.to_path_buf());
    assert_eq!(again.agreement_seq_floor(key).unwrap(), Some(4));

    // A floor in a file nothing else is in does not make the snapshot non-empty.
    assert_eq!(again.load().unwrap(), PairingSnapshot::default());
}

/// A file with a floor in the wrong shape stops the load, and is not changed.
#[test]
fn a_malformed_floor_refuses_the_file() {
    let temp = Temp::new("malformed");
    let dir = temp.dir();
    let id = identity(1);
    let key = id.key_id();
    let path = dir.join(TRUST_FILE_NAME);
    let store = trust(dir);
    store.record_agreement_seq_floor(key, 1).unwrap();
    let good = fs::read_to_string(&path).unwrap();
    assert!(
        good.starts_with(r#"{"agreement_seq_floor":{"key_id":""#),
        "{good}"
    );

    for (why, bad) in [
        (
            "seq above the maximum",
            good.replace(r#""seq":1}"#, r#""seq":9007199254740992}"#),
        ),
        (
            "a negative seq",
            good.replace(r#""seq":1}"#, r#""seq":-1}"#),
        ),
        (
            "a fractional seq",
            good.replace(r#""seq":1}"#, r#""seq":1.5}"#),
        ),
        (
            "a seq that is a string",
            good.replace(r#""seq":1}"#, r#""seq":"1"}"#),
        ),
        (
            "a key id that is not one",
            good.replace(key.as_str(), "nope"),
        ),
        (
            "an extra member",
            good.replace(r#""seq":1}"#, r#""seq":1,"x":1}"#),
        ),
        ("a missing seq", good.replace(r#","seq":1}"#, "}")),
        (
            "a floor that is not an object",
            good.replace(&good[23..good.find('}').unwrap() + 1], "1"),
        ),
    ] {
        assert_ne!(bad, good, "{why}: the edit changed nothing");
        fs::write(&path, &bad).unwrap();
        let fresh = FileTrustStore::new(dir.to_path_buf(), dir.to_path_buf());
        assert!(
            matches!(fresh.load(), Err(PairingStoreError::Corrupt(_))),
            "{why}"
        );
        assert!(fresh.agreement_seq_floor(key).is_err(), "{why}");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            bad,
            "{why}: not changed"
        );
    }
}

// ---- the daemon ------------------------------------------------------------------------

struct SharedKey(Arc<oac_core::keys::MemoryKeyStore>);

impl KeyStore for SharedKey {
    fn load(&self) -> Result<Option<SecretSeed>, KeyStoreError> {
        self.0.load()
    }

    fn save(&self, seed: &SecretSeed) -> Result<(), KeyStoreError> {
        self.0.save(seed)
    }
}

fn parts(namespace: &str, key: &Arc<oac_core::keys::MemoryKeyStore>, dir: &Path) -> Parts {
    let network = MemoryNetwork::new();
    let net = network.clone();
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    Parts {
        scope: Scope::for_user_in(Some(namespace)).unwrap(),
        key_store: Box::new(SharedKey(key.clone())),
        pairing_store: Box::new(FileTrustStore::new(dir.to_path_buf(), dir.to_path_buf())),
        agreement_store: Box::new(AgreementFileSource::new(
            dir.to_path_buf(),
            dir.to_path_buf(),
            |_| {},
        )),
        principal: Token::parse("test").unwrap(),
        transport: Arc::new(MemoryTransport::new()),
        transport_configuration: MemoryConfiguration::wrap(&network),
        clock,
        monotonic: Arc::new(move || net.now()),
        config: PipelineConfig::default(),
        harnesses: vec![("claude".to_owned(), Arc::new(ClaudeAdapter::new()) as _)],
        log: LogSink::discard(),
        codex: None,
    }
}

/// Whole: a daemon records its count as the floor at its first start, and a daemon whose
/// agreement file is gone but whose floor stays issues `floor + 1`.
#[tokio::test]
async fn a_daemon_keeps_the_floor_and_starts_above_it_when_the_file_is_gone() {
    let temp = Temp::new("daemon");
    let dir = temp.dir().join("data");
    let seed_key = Arc::new(oac_core::keys::MemoryKeyStore::new());
    seed_key
        .save(&SecretSeed::from_octets(&mut [9; 32]))
        .unwrap();
    let id = DeviceIdentity::new(
        DeviceKey::from_seed(&SecretSeed::from_octets(&mut [9; 32])),
        Token::parse("test").unwrap(),
    );
    let namespace = format!("sf{}", std::process::id());
    #[cfg(unix)]
    let _cleanup = {
        let scope = Scope::for_user_in(Some(&namespace)).unwrap();
        let dir = Path::new(&scope.endpoint("claude").display_name())
            .parent()
            .map(Path::to_path_buf);
        struct Remove(Option<PathBuf>);
        impl Drop for Remove {
            fn drop(&mut self) {
                if let Some(d) = self.0.take() {
                    let _ = fs::remove_dir_all(d);
                }
            }
        }
        Remove(dir)
    };

    let daemon = Daemon::start(parts(&namespace, &seed_key, &dir))
        .await
        .unwrap_or_else(|e| panic!("the daemon starts: {e}"));
    assert_eq!(
        seq_of(
            &daemon
                .pipelines()
                .agreement_statement()
                .expect("a statement")
        ),
        0
    );
    daemon.shutdown().await;
    let trust = FileTrustStore::new(dir.clone(), dir.clone());
    assert_eq!(
        trust.agreement_seq_floor(id.key_id()).unwrap(),
        Some(0),
        "the first start records its count"
    );

    // The count is higher than the file's: the floor is 5, and the file is gone.
    trust.record_agreement_seq_floor(id.key_id(), 5).unwrap();
    fs::remove_file(file_store(&dir, &id).path()).unwrap();
    let daemon = Daemon::start(parts(&namespace, &seed_key, &dir))
        .await
        .unwrap_or_else(|e| panic!("the daemon starts again: {e}"));
    assert_eq!(
        seq_of(
            &daemon
                .pipelines()
                .agreement_statement()
                .expect("a statement")
        ),
        6
    );
    daemon.shutdown().await;
    // (A new store: the daemon's own wrote the file, and a store keeps what it last read.)
    let trust = FileTrustStore::new(dir.clone(), dir.clone());
    assert_eq!(trust.agreement_seq_floor(id.key_id()).unwrap(), Some(6));
}
