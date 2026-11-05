// SPDX-License-Identifier: Apache-2.0

//! The durable agreement-key store (#437 S1b; `docs/planning/decisions/G-437-pairing.md` §7)
//! under the core's own `AgreementKeys`, and under a whole daemon that is stopped and started
//! again: **SEC-SEL-043**, "the highest `seq` is kept as durably as the device key", so that a
//! restart does not issue `seq` 0 under a new key.
//!
//! Every test uses a temporary directory and a test device key; nothing touches the user's own
//! data directory or credential store. The unit tests of the file itself (atomic writes, modes,
//! the sealing) are in `src/agreement_file.rs` and `oac-core`'s `at_rest` module.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use oac_adapter_claude::ClaudeAdapter;
use oac_cli::agreement_file::{AgreementFileSource, AgreementFileStore};
use oac_cli::daemon::{Daemon, LogSink, Parts};
use oac_cli::ipc::Scope;
use oac_core::clock::{Clock, SystemClock};
use oac_core::ids::{Timestamp, Token};
use oac_core::keys::{
    DeviceIdentity, DeviceKey, KeyStore, KeyStoreError, MemoryKeyStore, SecretSeed,
};
use oac_core::pairing::MemoryPairingStore;
use oac_core::pipeline::PipelineConfig;
use oac_core::sealing::{
    AgreementKeyError, AgreementKeyStore, AgreementKeys, AgreementPublicKey, AgreementStatement,
};
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};

/// A fresh directory under the system's temporary one, removed on drop.
struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Temp {
        let dir =
            std::env::temp_dir().join(format!("oac-agreement-store-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Temp(dir)
    }

    fn data(&self) -> PathBuf {
        self.0.join("data")
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A device identity from a test seed: a "restart" builds it again from the same seed.
fn identity(n: u8) -> DeviceIdentity {
    let key = DeviceKey::from_seed(&SecretSeed::from_octets(&mut [n; 32]));
    DeviceIdentity::new(key, Token::parse("device").unwrap())
}

fn store(dir: &Path, id: &DeviceIdentity) -> AgreementFileStore {
    AgreementFileStore::new(
        dir.to_path_buf(),
        dir.to_path_buf(),
        id.at_rest(),
        Arc::new(|_| {}),
    )
}

fn at(text: &str) -> Timestamp {
    Timestamp::parse(text).unwrap()
}

/// The `seq` a statement carries.
fn seq_of(s: &AgreementStatement) -> u64 {
    s.claimed_values().expect("a well-formed statement").seq()
}

/// The agreement key a statement carries.
fn key_of(s: &AgreementStatement) -> AgreementPublicKey {
    *s.claimed_values()
        .expect("a well-formed statement")
        .agreement_key()
}

fn assert_same(a: &AgreementStatement, b: &AgreementStatement) {
    assert_eq!(a, b, "the same statement, octet for octet");
}

/// **SEC-SEL-043.** The key and the `seq` survive a restart: the same statement comes back,
/// and after a `replace` the next start issues `seq` 1, not 0.
#[test]
fn seq_and_key_survive_a_restart() {
    let temp = Temp::new("restart");
    let dir = temp.data();

    // The first start: nothing saved, so a key at seq 0, saved before the statement is used.
    let first = {
        let id = identity(1);
        let keys =
            AgreementKeys::load_or_generate(&store(&dir, &id), &id, &at("2026-11-04T10:00:00Z"))
                .unwrap();
        assert_eq!(seq_of(keys.statement()), 0);
        keys.statement().clone()
    };

    // A restart, later: the same statement (the same key, `seq` and `issued_at`).
    let mut keys = {
        let id = identity(1);
        let keys =
            AgreementKeys::load_or_generate(&store(&dir, &id), &id, &at("2026-11-04T11:00:00Z"))
                .unwrap();
        assert_same(keys.statement(), &first);
        keys
    };

    // A replace issues seq 1 under a new key, saved before the statement is returned.
    let replaced = {
        let id = identity(1);
        let s = keys
            .replace(&store(&dir, &id), &id, &at("2026-11-04T12:00:00Z"))
            .unwrap();
        assert_eq!(seq_of(s), 1);
        assert_ne!(key_of(s), key_of(&first));
        s.clone()
    };

    // The next start issues the replaced statement (seq 1), not seq 0 under yet another key.
    let id = identity(1);
    let keys = AgreementKeys::load_or_generate(&store(&dir, &id), &id, &at("2026-11-04T13:00:00Z"))
        .unwrap();
    assert_same(keys.statement(), &replaced);
    assert_eq!(seq_of(keys.statement()), 1);

    // Erasing the replaced key keeps the count: the start after it is still seq 1, and the
    // replace after that is seq 2.
    let mut keys = keys;
    assert_eq!(keys.erase_replaced(&store(&dir, &id)).unwrap(), 1);
    let id = identity(1);
    let mut keys =
        AgreementKeys::load_or_generate(&store(&dir, &id), &id, &at("2026-11-04T14:00:00Z"))
            .unwrap();
    assert_same(keys.statement(), &replaced);
    let next = keys
        .replace(&store(&dir, &id), &id, &at("2026-11-04T15:00:00Z"))
        .unwrap();
    assert_eq!(seq_of(next), 2);
}

#[test]
fn the_agreement_file_holds_no_plaintext_key() {
    let temp = Temp::new("plaintext");
    let dir = temp.data();
    let id = identity(1);
    let s = store(&dir, &id);
    let mut keys = AgreementKeys::load_or_generate(&s, &id, &at("2026-11-04T10:00:00Z")).unwrap();
    keys.replace(&s, &id, &at("2026-11-04T11:00:00Z")).unwrap();
    let file = fs::read(s.path()).unwrap();
    let state = s.load().unwrap().expect("two keys saved");
    assert_eq!(state.keys.len(), 2);
    for key in &state.keys {
        let octets = key.private_key.expose();
        assert!(
            !file.windows(32).any(|w| w == octets),
            "a private key's octets are in the file"
        );
    }
    // Nor the state's other members: nothing of it is in the clear.
    assert!(!file.windows(10).any(|w| w == b"2026-11-04"));
    // The header names the device key; that is all.
    assert_eq!(&file[..4], b"OACR");
    assert_eq!(&file[6..70], id.key_id().as_str().as_bytes());
}

#[test]
fn a_missing_file_is_a_first_start() {
    let temp = Temp::new("missing");
    let dir = temp.data();
    let id = identity(1);
    let s = store(&dir, &id);
    assert!(!dir.exists());
    let keys = AgreementKeys::load_or_generate(&s, &id, &at("2026-11-04T10:00:00Z")).unwrap();
    assert_eq!(seq_of(keys.statement()), 0);
    assert!(
        s.path().is_file(),
        "the first key is saved before it is used"
    );
}

/// A regenerated device key (a new key id) starts its own file at seq 0 and leaves the old
/// file where it is (lead decision L7).
#[test]
fn a_regenerated_key_starts_its_own_file_at_seq_0_and_leaves_the_old_file() {
    let temp = Temp::new("regenerated");
    let dir = temp.data();
    let (old, new) = (identity(1), identity(2));
    let mut old_keys =
        AgreementKeys::load_or_generate(&store(&dir, &old), &old, &at("2026-11-04T10:00:00Z"))
            .unwrap();
    for hour in 11..14 {
        old_keys
            .replace(
                &store(&dir, &old),
                &old,
                &at(&format!("2026-11-04T{hour}:00:00Z")),
            )
            .unwrap();
    }
    assert_eq!(seq_of(old_keys.statement()), 3);
    let old_file = store(&dir, &old).path();
    let old_bytes = fs::read(&old_file).unwrap();

    // The new device key finds no file of its own and starts at seq 0, not at 4.
    let new_keys =
        AgreementKeys::load_or_generate(&store(&dir, &new), &new, &at("2026-11-04T15:00:00Z"))
            .unwrap();
    assert_eq!(seq_of(new_keys.statement()), 0);
    assert_ne!(store(&dir, &new).path(), old_file);
    // The old file is untouched, and the old key still resumes its count.
    assert_eq!(fs::read(&old_file).unwrap(), old_bytes);
    let resumed =
        AgreementKeys::load_or_generate(&store(&dir, &old), &old, &at("2026-11-04T16:00:00Z"))
            .unwrap();
    assert_eq!(seq_of(resumed.statement()), 3);
}

/// A file that exists and does not open stops the start: it is never reset (a reset would
/// issue seq 0 silently).
#[test]
fn an_unreadable_file_stops_the_start_and_is_not_reset() {
    let temp = Temp::new("unreadable");
    let dir = temp.data();
    let id = identity(1);
    let s = store(&dir, &id);
    AgreementKeys::load_or_generate(&s, &id, &at("2026-11-04T10:00:00Z")).unwrap();
    let mut bytes = fs::read(s.path()).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 1;
    fs::write(s.path(), &bytes).unwrap();
    let id = identity(1);
    let e = AgreementKeys::load_or_generate(&store(&dir, &id), &id, &at("2026-11-04T11:00:00Z"))
        .unwrap_err();
    assert!(
        matches!(e, AgreementKeyError::Store(KeyStoreError::Corrupt(_))),
        "{e}"
    );
    assert_eq!(
        fs::read(s.path()).unwrap(),
        bytes,
        "the file is not changed"
    );
}

// ---- the daemon ------------------------------------------------------------------------

/// A device key store the test keeps across daemons, so that a restart is the same device.
struct SharedKey(Arc<MemoryKeyStore>);

impl KeyStore for SharedKey {
    fn load(&self) -> Result<Option<SecretSeed>, KeyStoreError> {
        self.0.load()
    }

    fn save(&self, seed: &SecretSeed) -> Result<(), KeyStoreError> {
        self.0.save(seed)
    }
}

/// Removes the namespace directory a Unix test made under the user's run directory (Windows:
/// nothing is made), when the test ends however it ends.
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

fn parts(namespace: &str, key: &Arc<MemoryKeyStore>, dir: &Path) -> Parts {
    let network = MemoryNetwork::new();
    let net = network.clone();
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    Parts {
        scope: Scope::for_user_in(Some(namespace)).unwrap(),
        key_store: Box::new(SharedKey(key.clone())),
        pairing_store: Box::new(MemoryPairingStore::new()),
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

/// **SEC-SEL-043**, whole: a daemon on the durable store issues the same statement after it is
/// stopped and started again, and keeps the store it was started with.
#[tokio::test]
async fn a_daemon_issues_the_same_statement_after_a_restart() {
    let temp = Temp::new("daemon");
    let dir = temp.data();
    let key = Arc::new(MemoryKeyStore::new());
    let namespace = format!("ag{}", std::process::id());
    let _ns = namespace_dir(&Scope::for_user_in(Some(&namespace)).unwrap());

    let daemon = Daemon::start(parts(&namespace, &key, &dir))
        .await
        .unwrap_or_else(|e| panic!("the daemon starts: {e}"));
    let first = daemon
        .pipelines()
        .agreement_statement()
        .expect("a statement");
    assert_eq!(seq_of(&first), 0);
    // The daemon keeps its store: it reads back what the start saved.
    let kept = daemon.agreement_store().load().unwrap().expect("saved");
    assert_eq!(kept.highest_seq, Some(0));
    daemon.shutdown().await;

    let daemon = Daemon::start(parts(&namespace, &key, &dir))
        .await
        .unwrap_or_else(|e| panic!("the daemon starts again: {e}"));
    let second = daemon
        .pipelines()
        .agreement_statement()
        .expect("a statement");
    assert_same(&second, &first);
    daemon.shutdown().await;

    // A file that does not open stops the start, with a message that names it.
    let file = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("agreement-") && n.ends_with(".sealed"))
        })
        .expect("the agreement file");
    let mut bytes = fs::read(&file).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(&file, &bytes).unwrap();
    let e = match Daemon::start(parts(&namespace, &key, &dir)).await {
        Ok(_) => panic!("a daemon started on an edited agreement file"),
        Err(e) => e.to_string(),
    };
    assert!(e.contains("agreement"), "{e}");
    assert!(e.contains(&file.display().to_string()), "{e}");
    assert_eq!(fs::read(&file).unwrap(), bytes, "not reset");
}
