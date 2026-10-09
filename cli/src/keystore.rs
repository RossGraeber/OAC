// SPDX-License-Identifier: Apache-2.0

//! The device-key stores of the `oac` binary (`docs/planning/decisions/C4-session-identity.md`
//! §10-§12): the operating system's credential store through `keyring` 4.2.0, and the
//! `age`-encrypted file that is the fallback on a host with no credential service, such as
//! headless Linux without a Secret Service session bus.
//!
//! Each is a [`KeyStore`] that `oac-core` reads the device key's seed through; this module
//! constructs them and hands them to the core, and owns no key material itself
//! (`docs/planning/v0.1/07-repository-and-dependencies.md` §2-§3).
//!
//! **Credential boundary (C4 §12).** These stores read and write one entry: OAC's own
//! device key, under [`SERVICE`] / [`ACCOUNT`] or at the file path the caller names. They
//! never read a harness's credential entry or file.
//!
//! The real credential store is exercised by opt-in tests only (`oac-testing` §2): see the
//! `#[ignore]` tests at the end of this file, which run with
//! `OAC_TEST_REAL_KEYRING=1 cargo test -p oac-cli -- --ignored`.

use oac_core::keys::{KeyStore, KeyStoreError, SecretSeed};
use zeroize::Zeroizing;

/// The credential-store service name of OAC's device key.
pub const SERVICE: &str = "io.github.rossgraeber.oac";

/// The credential-store account name of OAC's device key.
pub const ACCOUNT: &str = "device-key";

/// The operating system's credential store: Windows Credential Manager, macOS Keychain, or
/// the Secret Service on other Unix hosts (`keyring` 4.2.0, feature `v1`; C1 §8).
pub struct OsKeyStore {
    service: String,
    account: String,
}

impl OsKeyStore {
    /// OAC's device-key entry.
    pub fn new() -> OsKeyStore {
        OsKeyStore::with_entry(SERVICE, ACCOUNT)
    }

    /// Another entry of OAC's own, for tests that must not touch the real device key.
    pub fn with_entry(service: &str, account: &str) -> OsKeyStore {
        OsKeyStore {
            service: service.to_owned(),
            account: account.to_owned(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, KeyStoreError> {
        keyring::Entry::new(&self.service, &self.account).map_err(map_error)
    }

    /// Deletes this store's entry. For the opt-in tests' own entry only.
    #[cfg(test)]
    fn delete(&self) -> Result<(), KeyStoreError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_error(e)),
        }
    }
}

impl Default for OsKeyStore {
    fn default() -> Self {
        OsKeyStore::new()
    }
}

/// A `keyring` error as a [`KeyStoreError`], never with the stored octets that two of its
/// variants carry.
///
/// Only the two errors that mean "this host has no credential store" are
/// [`KeyStoreError::Unavailable`], the one error [`FallbackKeyStore`] falls back on:
/// `NoDefaultStore` (`keyring` 4.2.0 `v1.rs`: store initialisation failed, as on a host
/// with no session bus) and `Invalid("platform", _)` (an unsupported platform).
///
/// `NoStorageAccess` is [`KeyStoreError::Locked`]. `keyring-core` 1.0.0 documents it as
/// the store being inaccessible, "for example, ... the credential store is locked", and
/// `zbus-secret-service-keyring-store` 1.0.1 returns it for a locked collection and a
/// dismissed unlock prompt. Such a store may hold the device key, so falling back would
/// create a second one ([SEC-KEY-002]; #315 review B1). Every other error is
/// [`KeyStoreError::Failed`] or, for unreadable content, [`KeyStoreError::Corrupt`].
fn map_error(e: keyring::Error) -> KeyStoreError {
    use keyring::Error as E;
    match e {
        E::NoDefaultStore => KeyStoreError::Unavailable("no credential store on this host".into()),
        E::Invalid(attr, _) if attr == "platform" => {
            KeyStoreError::Unavailable("no credential store for this platform".into())
        }
        E::NoStorageAccess(p) => {
            KeyStoreError::Locked(format!("credential store not accessible: {p}"))
        }
        E::BadEncoding(_) | E::BadDataFormat(..) => {
            KeyStoreError::Corrupt("the credential entry is not a device key".into())
        }
        E::PlatformFailure(p) => KeyStoreError::Failed(format!("credential store: {p}")),
        other => KeyStoreError::Failed(format!("credential store: {other}")),
    }
}

impl KeyStore for OsKeyStore {
    fn load(&self) -> Result<Option<SecretSeed>, KeyStoreError> {
        match self.entry()?.get_secret() {
            Ok(secret) => {
                let secret = Zeroizing::new(secret);
                SecretSeed::from_slice(&secret).map(Some).ok_or_else(|| {
                    KeyStoreError::Corrupt("the credential entry is not 32 octets".into())
                })
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_error(e)),
        }
    }

    /// Refuses when the entry already holds a key. `keyring` offers no create-only write, so
    /// this is a read, then a write, and two writers could race between them. One process
    /// per device holds key material: the daemon, which `oac start` runs once per user
    /// (`docs/planning/decisions/C2-process-model.md` §1, §5), so OAC has no second writer.
    /// [`oac_core::keys::DeviceKey::load_or_generate`] reads the key back after saving, so
    /// a lost race is still detected before the key is used.
    fn save(&self, seed: &SecretSeed) -> Result<(), KeyStoreError> {
        if self.load()?.is_some() {
            return Err(KeyStoreError::Failed(
                "a device key is already stored".into(),
            ));
        }
        self.entry()?.set_secret(seed.expose()).map_err(map_error)
    }
}

/// The store that tries `primary` first and uses `fallback` only while `primary` reports
/// itself [`KeyStoreError::Unavailable`] (C4 §11, "When the fallback engages"). Any other
/// error of `primary`, [`KeyStoreError::Locked`] included, stops there, so a locked or
/// failing credential store never silently produces a second device key in the file.
pub struct FallbackKeyStore<P, F> {
    primary: P,
    fallback: Option<F>,
}

impl<P: KeyStore, F: KeyStore> FallbackKeyStore<P, F> {
    /// `primary`, falling back to `fallback` when there is one.
    pub fn new(primary: P, fallback: Option<F>) -> Self {
        FallbackKeyStore { primary, fallback }
    }

    fn or_fallback<T>(
        &self,
        primary: Result<T, KeyStoreError>,
        f: impl FnOnce(&F) -> Result<T, KeyStoreError>,
    ) -> Result<T, KeyStoreError> {
        match (primary, &self.fallback) {
            (Err(KeyStoreError::Unavailable(_)), Some(fallback)) => f(fallback),
            (r, _) => r,
        }
    }
}

impl<P: KeyStore, F: KeyStore> KeyStore for FallbackKeyStore<P, F> {
    fn load(&self) -> Result<Option<SecretSeed>, KeyStoreError> {
        self.or_fallback(self.primary.load(), KeyStore::load)
    }

    fn save(&self, seed: &SecretSeed) -> Result<(), KeyStoreError> {
        self.or_fallback(self.primary.save(seed), |f| f.save(seed))
    }
}

#[cfg(unix)]
pub use file::{FileKeyStore, PASSPHRASE_CREDENTIAL, PASSPHRASE_ENV};

/// The encrypted-file fallback (C4 §11), on Unix hosts. On Windows and macOS the operating
/// system's credential store is always present (C1 §9), so the fallback is not built there.
#[cfg(unix)]
mod file {
    use super::{KeyStore, KeyStoreError, SecretSeed, Zeroizing};
    use age::secrecy::SecretString;
    use std::fs::{self, DirBuilder, File, Metadata, OpenOptions};
    use std::io::{ErrorKind, Write};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    /// The environment variable a deployment may use to hand the file's passphrase to the
    /// process (C4 §11: the host's own secret management protects it). A last resort:
    /// [`PASSPHRASE_CREDENTIAL`] is preferred. It is removed from the process environment
    /// when read, so no child process inherits it.
    pub const PASSPHRASE_ENV: &str = "OAC_DEVICE_KEY_PASSPHRASE";

    /// The systemd credential name of the passphrase, read from `$CREDENTIALS_DIRECTORY`.
    pub const PASSPHRASE_CREDENTIAL: &str = "oac-device-key-passphrase";

    /// [`PASSPHRASE_ENV`]'s value, taken once.
    static ENV_PASSPHRASE: OnceLock<Option<SecretString>> = OnceLock::new();

    fn failed(e: std::io::Error) -> KeyStoreError {
        KeyStoreError::Failed(format!("key file: {e}"))
    }

    /// This process's effective user id.
    fn euid() -> u32 {
        // SAFETY: geteuid has no preconditions, cannot fail and touches no memory of ours.
        unsafe { libc::geteuid() }
    }

    /// `meta` is owned by this user and gives no access to anyone else.
    fn check_private(meta: &Metadata, what: &str) -> Result<(), KeyStoreError> {
        if meta.uid() != euid() {
            return Err(KeyStoreError::Failed(format!(
                "{what} is not owned by this user"
            )));
        }
        if meta.mode() & 0o077 != 0 {
            return Err(KeyStoreError::Failed(format!(
                "{what} is accessible to other users"
            )));
        }
        Ok(())
    }

    /// The key file's directory, checked: a real directory (not a link), owned by this
    /// user, mode `0700` or stricter.
    fn check_dir(dir: &Path) -> Result<(), KeyStoreError> {
        let meta = fs::symlink_metadata(dir).map_err(failed)?;
        if !meta.is_dir() {
            return Err(KeyStoreError::Failed(
                "key directory is not a directory".into(),
            ));
        }
        check_private(&meta, "key directory")
    }

    /// The device key's seed in an `age` file (`age-encryption.org/v1`), encrypted to an
    /// `age::scrypt` passphrase recipient (C4 §11).
    ///
    /// The file's directory is OAC's own: [`KeyStore::save`] creates it `0700` when it does
    /// not exist and refuses one that exists but is not owned by this user or is open to
    /// others; it never changes the mode of an existing directory. The file is `0600`,
    /// written to a temporary file in the same directory, synced, then linked into place,
    /// so a failed write never leaves a partial key file, and an existing key file is never
    /// replaced. [`KeyStore::load`] refuses a key file or directory that is a link, is not
    /// owned by this user, or is open to others.
    pub struct FileKeyStore {
        path: PathBuf,
        passphrase: SecretString,
        work_factor: Option<u8>,
    }

    impl FileKeyStore {
        /// The store at `path`, encrypted under `passphrase`. `path`'s directory is OAC's
        /// own (above).
        pub fn new(path: PathBuf, passphrase: SecretString) -> FileKeyStore {
            FileKeyStore {
                path,
                passphrase,
                work_factor: None,
            }
        }

        /// The store with scrypt work factor `2^log_n` for new files, in place of `age`'s
        /// default of about one second. For tests.
        pub fn with_work_factor(mut self, log_n: u8) -> FileKeyStore {
            self.work_factor = Some(log_n);
            self
        }

        /// `$XDG_DATA_HOME/oac/device.key.age`, or `~/.oac/device.key.age` when
        /// `$XDG_DATA_HOME` is unset (C4 §11).
        pub fn default_path() -> Option<PathBuf> {
            match std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
                Some(d) => Some(PathBuf::from(d).join("oac").join("device.key.age")),
                None => std::env::var_os("HOME")
                    .filter(|v| !v.is_empty())
                    .map(|h| PathBuf::from(h).join(".oac").join("device.key.age")),
            }
        }

        /// The passphrase the deployment provides: the systemd credential
        /// [`PASSPHRASE_CREDENTIAL`], else the environment variable [`PASSPHRASE_ENV`].
        /// `None` when neither is set: the fallback is then not available.
        ///
        /// The environment variable is read once, on the first call, and removed from the
        /// process environment then, whichever source wins, so no child process this
        /// process starts (a harness, and any shell a model runs inside it) inherits the
        /// passphrase (#315 review N-a; [SEC-KEY-004]). Later calls return the value taken
        /// on the first.
        ///
        /// # Safety
        ///
        /// The first call runs `std::env::remove_var`, which is sound only while no other
        /// thread reads or writes the process environment. Call this at start-up, before
        /// the process starts any other thread; after the first call it touches the
        /// environment only to read `$CREDENTIALS_DIRECTORY`.
        pub unsafe fn passphrase_from_environment() -> Option<SecretString> {
            let from_env = ENV_PASSPHRASE.get_or_init(|| {
                let value = std::env::var(PASSPHRASE_ENV).ok();
                if std::env::var_os(PASSPHRASE_ENV).is_some() {
                    // SAFETY: the caller guarantees no other thread uses the environment
                    // during this first call (see the function's Safety section).
                    unsafe { std::env::remove_var(PASSPHRASE_ENV) };
                }
                value.filter(|p| !p.is_empty()).map(SecretString::from)
            });
            let credential = std::env::var_os("CREDENTIALS_DIRECTORY")
                .and_then(|dir| {
                    fs::read_to_string(Path::new(&dir).join(PASSPHRASE_CREDENTIAL)).ok()
                })
                .map(Zeroizing::new);
            if let Some(p) = credential {
                let p = p.trim_end_matches(['\r', '\n']);
                if !p.is_empty() {
                    return Some(SecretString::from(p.to_owned()));
                }
            }
            from_env.clone()
        }

        /// The default store, when the deployment provides a passphrase and a home or data
        /// directory.
        ///
        /// # Safety
        ///
        /// As [`FileKeyStore::passphrase_from_environment`].
        pub unsafe fn from_environment() -> Option<FileKeyStore> {
            // SAFETY: the caller upholds passphrase_from_environment's contract.
            let passphrase = unsafe { FileKeyStore::passphrase_from_environment() }?;
            Some(FileKeyStore::new(FileKeyStore::default_path()?, passphrase))
        }

        fn dir(&self) -> Result<&Path, KeyStoreError> {
            self.path
                .parent()
                .filter(|d| !d.as_os_str().is_empty())
                .ok_or_else(|| KeyStoreError::Failed("key file path has no directory".into()))
        }

        /// Creates the key directory `0700` if it does not exist; otherwise checks it.
        fn ensure_dir(&self) -> Result<(), KeyStoreError> {
            let dir = self.dir()?;
            match fs::symlink_metadata(dir) {
                Ok(_) => {}
                Err(e) if e.kind() == ErrorKind::NotFound => {
                    if let Some(up) = dir.parent().filter(|u| !u.as_os_str().is_empty()) {
                        fs::create_dir_all(up).map_err(failed)?;
                    }
                    match DirBuilder::new().mode(0o700).create(dir) {
                        Ok(()) => {}
                        Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
                        Err(e) => return Err(failed(e)),
                    }
                }
                Err(e) => return Err(failed(e)),
            }
            check_dir(dir)
        }
    }

    impl KeyStore for FileKeyStore {
        fn load(&self) -> Result<Option<SecretSeed>, KeyStoreError> {
            let meta = match fs::symlink_metadata(&self.path) {
                Ok(m) => m,
                Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(failed(e)),
            };
            check_dir(self.dir()?)?;
            if !meta.file_type().is_file() {
                return Err(KeyStoreError::Failed(
                    "key file is not a regular file".into(),
                ));
            }
            check_private(&meta, "key file")?;
            let ciphertext = fs::read(&self.path).map_err(failed)?;
            let identity = age::scrypt::Identity::new(self.passphrase.clone());
            let plain = Zeroizing::new(
                age::decrypt(&identity, &ciphertext)
                    .map_err(|_| KeyStoreError::Corrupt("key file does not decrypt".into()))?,
            );
            SecretSeed::from_slice(&plain)
                .map(Some)
                .ok_or_else(|| KeyStoreError::Corrupt("key file is not a device key".into()))
        }

        fn save(&self, seed: &SecretSeed) -> Result<(), KeyStoreError> {
            self.ensure_dir()?;
            let dir = self.dir()?;
            let mut recipient = age::scrypt::Recipient::new(self.passphrase.clone());
            if let Some(log_n) = self.work_factor {
                recipient.set_work_factor(log_n);
            }
            let ciphertext = age::encrypt(&recipient, seed.expose())
                .map_err(|e| KeyStoreError::Failed(format!("encrypting the key file: {e}")))?;
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let tmp = dir.join(format!(".device-key.{}.{nanos}.tmp", std::process::id()));
            let written = (|| {
                let mut f = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&tmp)?;
                f.write_all(&ciphertext)?;
                f.sync_all()?;
                // A hard link never replaces: an existing key file makes it fail
                // ([SEC-KEY-002]), and the key file appears complete or not at all.
                fs::hard_link(&tmp, &self.path)
            })();
            let _ = fs::remove_file(&tmp);
            match written {
                Ok(()) => {}
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    return Err(KeyStoreError::Failed(
                        "a device key is already stored".into(),
                    ));
                }
                Err(e) => return Err(failed(e)),
            }
            File::open(dir).and_then(|d| d.sync_all()).map_err(failed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oac_core::keys::{DeviceKey, MemoryKeyStore};

    /// A store that reports itself unavailable, as a host with no credential service does.
    struct NoService;

    impl KeyStore for NoService {
        fn load(&self) -> Result<Option<SecretSeed>, KeyStoreError> {
            Err(KeyStoreError::Unavailable("test".into()))
        }
        fn save(&self, _: &SecretSeed) -> Result<(), KeyStoreError> {
            Err(KeyStoreError::Unavailable("test".into()))
        }
    }

    /// A store that is present but failing, as a locked credential store is.
    struct Failing;

    impl KeyStore for Failing {
        fn load(&self) -> Result<Option<SecretSeed>, KeyStoreError> {
            Err(KeyStoreError::Failed("locked".into()))
        }
        fn save(&self, _: &SecretSeed) -> Result<(), KeyStoreError> {
            Err(KeyStoreError::Failed("locked".into()))
        }
    }

    #[test]
    fn fallback_engages_only_when_the_primary_is_unavailable() {
        let store = FallbackKeyStore::new(NoService, Some(MemoryKeyStore::new()));
        let key = DeviceKey::load_or_generate(&store).unwrap();
        assert_eq!(
            DeviceKey::load_or_generate(&store).unwrap().key_id(),
            key.key_id()
        );

        let none: FallbackKeyStore<NoService, MemoryKeyStore> =
            FallbackKeyStore::new(NoService, None);
        assert!(matches!(
            DeviceKey::load_or_generate(&none),
            Err(KeyStoreError::Unavailable(_))
        ));

        let fallback = MemoryKeyStore::new();
        let locked = FallbackKeyStore::new(Failing, Some(fallback));
        assert!(matches!(
            DeviceKey::load_or_generate(&locked),
            Err(KeyStoreError::Failed(_))
        ));
        assert!(locked.fallback.as_ref().unwrap().load().unwrap().is_none());

        let primary = MemoryKeyStore::new();
        let both = FallbackKeyStore::new(primary, Some(MemoryKeyStore::new()));
        DeviceKey::load_or_generate(&both).unwrap();
        assert!(both.fallback.as_ref().unwrap().load().unwrap().is_none());
    }

    /// Every `keyring` error variant's classification, pinned (#315 review B1). Only the two
    /// "no credential store here" errors may fall back.
    #[test]
    fn map_error_classifies_every_keyring_error() {
        use keyring::Error as E;
        let unavailable = |e| matches!(map_error(e), KeyStoreError::Unavailable(_));
        let locked = |e| matches!(map_error(e), KeyStoreError::Locked(_));
        let corrupt = |e| matches!(map_error(e), KeyStoreError::Corrupt(_));
        let failed = |e| matches!(map_error(e), KeyStoreError::Failed(_));
        assert!(unavailable(E::NoDefaultStore));
        assert!(unavailable(E::Invalid("platform".into(), "x".into())));
        assert!(locked(E::NoStorageAccess("locked".into())));
        assert!(corrupt(E::BadEncoding(vec![1])));
        assert!(corrupt(E::BadDataFormat(vec![1], "x".into())));
        assert!(failed(E::PlatformFailure("x".into())));
        assert!(failed(E::NoEntry));
        assert!(failed(E::BadStoreFormat("x".into())));
        assert!(failed(E::TooLong("service".into(), 1)));
        assert!(failed(E::Invalid("service".into(), "x".into())));
        assert!(failed(E::Ambiguous(Vec::new())));
        assert!(failed(E::NotSupportedByStore("x".into())));
    }

    /// An OS store that holds a device key but is locked, as a Secret Service collection
    /// is before its unlock prompt is answered.
    struct LockableOsStore {
        inner: MemoryKeyStore,
        locked: std::cell::Cell<bool>,
    }

    impl KeyStore for LockableOsStore {
        fn load(&self) -> Result<Option<SecretSeed>, KeyStoreError> {
            if self.locked.get() {
                return Err(map_error(keyring::Error::NoStorageAccess("locked".into())));
            }
            self.inner.load()
        }
        fn save(&self, seed: &SecretSeed) -> Result<(), KeyStoreError> {
            if self.locked.get() {
                return Err(map_error(keyring::Error::NoStorageAccess("locked".into())));
            }
            self.inner.save(seed)
        }
    }

    /// #315 review B1: a locked store that holds the device key never leads to a second key
    /// in the fallback; once unlocked, the original key is the one used.
    #[test]
    fn a_locked_store_never_falls_back_to_a_second_key() {
        let os = LockableOsStore {
            inner: MemoryKeyStore::new(),
            locked: std::cell::Cell::new(false),
        };
        let original = DeviceKey::load_or_generate(&os.inner).unwrap().key_id();
        os.locked.set(true);
        let store = FallbackKeyStore::new(os, Some(MemoryKeyStore::new()));
        for _ in 0..2 {
            assert!(matches!(
                DeviceKey::load_or_generate(&store),
                Err(KeyStoreError::Locked(_))
            ));
        }
        assert!(store.fallback.as_ref().unwrap().load().unwrap().is_none());
        store.primary.locked.set(false);
        assert_eq!(
            DeviceKey::load_or_generate(&store).unwrap().key_id(),
            original
        );
        assert!(store.fallback.as_ref().unwrap().load().unwrap().is_none());
    }

    #[test]
    fn errors_never_carry_stored_octets() {
        let secret = vec![0x42u8; 32];
        for e in [
            keyring::Error::BadEncoding(secret.clone()),
            keyring::Error::BadDataFormat(secret.clone(), "x".into()),
        ] {
            let mapped = map_error(e);
            let shown = format!("{mapped:?} {mapped}");
            assert!(matches!(mapped, KeyStoreError::Corrupt(_)));
            assert!(!shown.contains("66"), "{shown}");
        }
    }

    #[cfg(unix)]
    mod file_store {
        use super::super::*;
        use age::secrecy::SecretString;
        use oac_core::keys::DeviceKey;
        use std::os::unix::fs::PermissionsExt;

        fn temp_dir(name: &str) -> std::path::PathBuf {
            let d = std::env::temp_dir().join(format!(
                "oac-keystore-{name}-{}-{}",
                std::process::id(),
                DeviceKey::generate().key_id()
            ));
            let _ = std::fs::remove_dir_all(&d);
            d
        }

        fn store(path: std::path::PathBuf, pass: &str) -> FileKeyStore {
            FileKeyStore::new(path, SecretString::from(pass.to_owned())).with_work_factor(10)
        }

        #[test]
        fn file_round_trip_with_owner_only_permissions() {
            let dir = temp_dir("rt");
            let path = dir.join("oac").join("device.key.age");
            let s = store(path.clone(), "correct horse");
            assert!(s.load().unwrap().is_none());
            let key = DeviceKey::load_or_generate(&s).unwrap();
            assert_eq!(
                DeviceKey::load_or_generate(&s).unwrap().key_id(),
                key.key_id()
            );
            let mode =
                |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(path.parent().unwrap()), 0o700);
            // The file is ciphertext: the seed's octets do not appear in it.
            let bytes = std::fs::read(&path).unwrap();
            assert!(bytes.starts_with(b"age-encryption.org/v1\n"));
            let seed = s.load().unwrap().unwrap();
            assert!(!bytes.windows(32).any(|w| w == seed.expose()));
            // A wrong passphrase does not decrypt it.
            assert!(matches!(
                store(path.clone(), "wrong").load(),
                Err(KeyStoreError::Corrupt(_))
            ));
            // An existing file is never replaced.
            assert!(s.save(&SecretSeed::from_octets(&mut [1; 32])).is_err());
            // No temporary file is left beside the key file.
            let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert_eq!(names, vec![std::ffi::OsString::from("device.key.age")]);
            // A key directory other users can enter is refused on load.
            let parent = path.parent().unwrap();
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(matches!(s.load(), Err(KeyStoreError::Failed(_))));
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).unwrap();
            // A file other users can read is refused.
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(matches!(s.load(), Err(KeyStoreError::Failed(_))));
            std::fs::remove_dir_all(&dir).unwrap();
        }

        /// #315 review N-c: an existing directory open to others is refused, and its mode is
        /// left as it was.
        #[test]
        fn an_open_existing_directory_is_refused_not_changed() {
            let dir = temp_dir("open");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            let s = store(dir.join("device.key.age"), "pass");
            assert!(matches!(
                s.save(&SecretSeed::from_octets(&mut [3; 32])),
                Err(KeyStoreError::Failed(_))
            ));
            let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
            assert!(!dir.join("device.key.age").exists());
            std::fs::remove_dir_all(&dir).unwrap();
        }

        /// A key file that is a link is refused, wherever it points.
        #[test]
        fn a_linked_key_file_is_refused() {
            let dir = temp_dir("link");
            let real = store(dir.join("real").join("device.key.age"), "pass");
            DeviceKey::load_or_generate(&real).unwrap();
            let linked_dir = dir.join("linked");
            std::fs::create_dir(&linked_dir).unwrap();
            std::fs::set_permissions(&linked_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::os::unix::fs::symlink(
                dir.join("real").join("device.key.age"),
                linked_dir.join("device.key.age"),
            )
            .unwrap();
            let linked = store(linked_dir.join("device.key.age"), "pass");
            assert!(matches!(linked.load(), Err(KeyStoreError::Failed(_))));
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }

    /// Opt-in (`oac-testing` §2): the real operating-system credential store. Writes and
    /// then deletes a test entry of OAC's own, never the device key's entry.
    #[test]
    #[ignore = "opt-in: touches the real credential store; set OAC_TEST_REAL_KEYRING=1 and pass --ignored"]
    fn real_credential_store_round_trip() {
        assert_eq!(
            std::env::var("OAC_TEST_REAL_KEYRING").as_deref(),
            Ok("1"),
            "set OAC_TEST_REAL_KEYRING=1 to run the real credential-store test"
        );
        let account = format!("test-{}", DeviceKey::generate().key_id());
        let store = OsKeyStore::with_entry(&format!("{SERVICE}.test"), &account);
        match store.load() {
            Ok(None) => {}
            other => panic!("this host's credential store is not usable: {other:?}"),
        }
        let key = DeviceKey::load_or_generate(&store).unwrap();
        let again = DeviceKey::load_or_generate(&OsKeyStore::with_entry(
            &format!("{SERVICE}.test"),
            &account,
        ))
        .unwrap();
        let result = (
            key.key_id() == again.key_id(),
            store.save(&SecretSeed::from_octets(&mut [1; 32])).is_err(),
        );
        store.delete().unwrap();
        assert_eq!(
            result,
            (true, true),
            "same key read back; no second key saved"
        );
        assert!(store.load().unwrap().is_none());
        eprintln!(
            "real credential store: {} round trip passed",
            std::env::consts::OS
        );
    }

    /// Opt-in: a headless Linux host, with no Secret Service session bus, falls back to the
    /// encrypted file (C4 §11). Run it with no `DBUS_SESSION_BUS_ADDRESS`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "opt-in: needs a headless Linux host; set OAC_TEST_REAL_KEYRING=1 and pass --ignored"]
    fn headless_linux_falls_back_to_the_encrypted_file() {
        use age::secrecy::SecretString;
        assert_eq!(
            std::env::var("OAC_TEST_REAL_KEYRING").as_deref(),
            Ok("1"),
            "set OAC_TEST_REAL_KEYRING=1 to run the real credential-store test"
        );
        let account = format!("test-{}", DeviceKey::generate().key_id());
        let os = OsKeyStore::with_entry(&format!("{SERVICE}.test"), &account);
        assert!(
            matches!(os.load(), Err(KeyStoreError::Unavailable(_))),
            "this host has a credential store; run on a headless host"
        );
        let dir = std::env::temp_dir().join(format!("oac-headless-{account}"));
        let path = dir.join("device.key.age");
        let file = FileKeyStore::new(path.clone(), SecretString::from("test pass".to_owned()))
            .with_work_factor(10);
        let store = FallbackKeyStore::new(os, Some(file));
        let key = DeviceKey::load_or_generate(&store).unwrap();
        assert!(path.exists());
        assert_eq!(
            DeviceKey::load_or_generate(&store).unwrap().key_id(),
            key.key_id()
        );
        std::fs::remove_dir_all(&dir).unwrap();
        eprintln!("headless fallback: the encrypted file holds the device key");
    }
}
