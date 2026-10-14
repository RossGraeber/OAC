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

/// The encrypted-file fallback (C4 §11), built on every Unix host, macOS included, where the
/// Keychain is always present (C1 §9) so it is not expected to engage. Windows has no
/// fallback: Credential Manager is always present.
#[cfg(unix)]
mod file {
    use super::{KeyStore, KeyStoreError, SecretSeed, Zeroizing};
    use age::secrecy::SecretString;
    use std::ffi::CString;
    use std::fs::{self, DirBuilder, File, Metadata, OpenOptions};
    use std::io::{ErrorKind, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;
    use std::time::{Duration, SystemTime};

    /// The environment variable a deployment may use to hand the file's passphrase to the
    /// process (C4 §11: the host's own secret management protects it). A last resort:
    /// [`PASSPHRASE_CREDENTIAL`] is preferred, because an environment variable is readable by
    /// every process of the same user that can read this one's memory or, until it is
    /// overwritten, `/proc/<pid>/environ`. It is overwritten in place and removed from the
    /// process environment when read, so no child process inherits it.
    pub const PASSPHRASE_ENV: &str = "OAC_DEVICE_KEY_PASSPHRASE";

    /// The systemd credential name of the passphrase, read from `$CREDENTIALS_DIRECTORY`.
    pub const PASSPHRASE_CREDENTIAL: &str = "oac-device-key-passphrase";

    /// A key file is a few hundred octets; one larger than this is not one, and is not read.
    const MAX_KEY_FILE: u64 = 64 * 1024;

    /// How old a temporary file left beside the key file must be before it is removed. A
    /// writer holds its temporary file for the length of one encrypt, write and sync.
    const STALE_TEMP_AFTER: Duration = Duration::from_secs(60);

    /// The most random names [`FileKeyStore::save`] tries for its temporary file.
    const TEMP_NAME_ATTEMPTS: usize = 16;

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

    /// The key file's directory, opened without following a link, as the handle every later
    /// check and open of the key file is made against.
    fn open_dir(dir: &Path) -> std::io::Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(dir)
    }

    /// The directory handle `dir` as a checked key directory: a real directory (not a link),
    /// owned by this user, mode `0700` or stricter. `None` when it does not exist.
    fn checked_dir(dir: &Path) -> Result<Option<File>, KeyStoreError> {
        match open_dir(dir) {
            Ok(handle) => {
                let meta = handle.metadata().map_err(failed)?;
                check_private(&meta, "key directory")?;
                Ok(Some(handle))
            }
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            // ELOOP (a link) or ENOTDIR: not a directory of ours.
            Err(e) if matches!(e.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR)) => Err(
                KeyStoreError::Failed("key directory is not a directory".into()),
            ),
            Err(e) => Err(failed(e)),
        }
    }

    /// Opens `name` in `dir` for reading, without following a link and without blocking on a
    /// pipe, so what is checked is what is read (#315 follow-up). `Ok(None)` when it does
    /// not exist.
    fn open_in(dir: &File, name: &std::ffi::OsStr) -> Result<Option<File>, KeyStoreError> {
        let name = CString::new(name.as_bytes())
            .map_err(|_| KeyStoreError::Failed("key file name holds a NUL".into()))?;
        // SAFETY: `name` is a valid NUL-terminated string that outlives the call, and
        // `dir.as_raw_fd()` is an open descriptor owned by `dir`.
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            let e = std::io::Error::last_os_error();
            return match e.raw_os_error() {
                Some(libc::ENOENT) => Ok(None),
                Some(libc::ELOOP) => Err(KeyStoreError::Failed(
                    "key file is not a regular file".into(),
                )),
                _ => Err(failed(e)),
            };
        }
        // SAFETY: `fd` is a descriptor `openat` just returned and nothing else owns.
        Ok(Some(unsafe { File::from_raw_fd(fd) }))
    }

    /// A passphrase's text for `seed`: its 32 octets as 64 hexadecimal digits.
    fn hex_of(seed: &SecretSeed) -> SecretString {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut text = String::with_capacity(64);
        for byte in seed.expose() {
            text.push(DIGITS[usize::from(byte >> 4)] as char);
            text.push(DIGITS[usize::from(byte & 0x0f)] as char);
        }
        SecretString::from(text)
    }

    /// The process's `environ` array: the pointers to its `NAME=value` strings.
    ///
    /// # Safety
    ///
    /// No other thread may change the environment while the array is read.
    unsafe fn environ_array() -> *mut *mut libc::c_char {
        #[cfg(target_os = "macos")]
        {
            // SAFETY: `_NSGetEnviron` returns a valid pointer to the process's `environ`.
            unsafe { *libc::_NSGetEnviron() }
        }
        #[cfg(not(target_os = "macos"))]
        {
            unsafe extern "C" {
                static mut environ: *mut *mut libc::c_char;
            }
            // SAFETY: `environ` is the POSIX process environment; read by value under the
            // caller's no-concurrent-change guarantee.
            unsafe { environ }
        }
    }

    /// Overwrites the value of every entry of the environment variable `name` where the
    /// process keeps it, then removes the variable.
    ///
    /// `remove_var` alone leaves the value in the memory the process was started with, which
    /// `/proc/<pid>/environ` shows to any process of the same user (#315 review). An `envp`
    /// may repeat a name; `getenv` returns the first, so every entry of the array is walked,
    /// and the entries that follow the first are overwritten too.
    ///
    /// # Safety
    ///
    /// As `std::env::remove_var`: no other thread may use the environment during the call.
    unsafe fn wipe_and_remove_env(name: &str) {
        let prefix = format!("{name}=").into_bytes();
        // SAFETY: the caller guarantees no other thread changes the environment, so the
        // array and the strings it points to stay valid while they are walked. Each string
        // is NUL-terminated process memory (the stack the process started with, or a
        // `setenv` allocation); a write stays within one entry's value, after its
        // `NAME=` prefix, so the entry still names the variable for `unsetenv`.
        unsafe {
            let mut entry = environ_array();
            while !entry.is_null() && !(*entry).is_null() {
                let text = *entry;
                let len = libc::strlen(text);
                let bytes = std::slice::from_raw_parts(text.cast::<u8>(), len);
                if bytes.starts_with(&prefix) {
                    std::ptr::write_bytes(
                        text.cast::<u8>().add(prefix.len()),
                        0,
                        len - prefix.len(),
                    );
                }
                entry = entry.add(1);
            }
        }
        // SAFETY: the caller guarantees no other thread uses the environment.
        unsafe { std::env::remove_var(name) };
    }

    /// Removes the temporary files a crashed [`FileKeyStore::save`] left in `dir`: regular
    /// files of ours, named like its temporary files, not modified for
    /// [`STALE_TEMP_AFTER`]. A link, a directory, another user's file, a recent file (a
    /// writer in progress) and any other name are left alone. Best effort: a failure is not
    /// an error.
    fn sweep_stale_temp_files(dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let now = SystemTime::now();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !(name.starts_with(".device-key.") && name.ends_with(".tmp")) {
                continue;
            }
            let Ok(meta) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            let old = meta
                .modified()
                .ok()
                .and_then(|m| now.duration_since(m).ok())
                .is_some_and(|age| age >= STALE_TEMP_AFTER);
            if meta.file_type().is_file() && meta.uid() == euid() && old {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    /// A fresh name for a temporary file: this process and 64 random bits.
    fn random_temp_name() -> String {
        let seed = SecretSeed::generate();
        let mut name = format!(".device-key.{}.", std::process::id());
        for byte in &seed.expose()[..8] {
            name.push_str(&format!("{byte:02x}"));
        }
        name.push_str(".tmp");
        name
    }

    /// The device key's seed in an `age` file (`age-encryption.org/v1`), encrypted to an
    /// `age::scrypt` passphrase recipient (C4 §11).
    ///
    /// The file's directory is OAC's own: [`KeyStore::save`] creates it `0700` when it does
    /// not exist and refuses one that exists but is not owned by this user or is open to
    /// others; it never changes the mode of an existing directory. The file is `0600`,
    /// written to a temporary file in the same directory, synced, then linked into place,
    /// so a failed write never leaves a partial key file, and an existing key file is never
    /// replaced. The temporary file's name is random and created exclusively; a name that is
    /// taken is never reused and its file never touched. A temporary file a crash left
    /// behind is removed once it is [`STALE_TEMP_AFTER`] old (by `save` and by `load`).
    ///
    /// [`KeyStore::load`] opens the directory and then the file without following links, and
    /// checks the open handles: a key file or directory that is a link, is not owned by this
    /// user, or is open to others is refused, and the file that was checked is the file that
    /// is read, however the path changes meanwhile.
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

        /// A new passphrase for a key file that does not exist yet: 256 bits from the
        /// operating system's random source, as 64 hexadecimal digits (C4 §11, "Passphrase/KDF
        /// source": OAC generates the passphrase itself on first fallback-path key creation).
        pub fn generate_passphrase() -> SecretString {
            hex_of(&SecretSeed::generate())
        }

        /// The store for a first key at `path`, under a passphrase OAC generates (C4 §11).
        ///
        /// The passphrase is the one secret that protects the file, and it must live apart
        /// from it (a systemd credential, or the host's own secret store). So `handover`
        /// receives it first, to put wherever the caller decides (for example to show it
        /// once on an operator's terminal), and the store is returned only if `handover`
        /// succeeds: a passphrase nobody holds would make the key it protects unreadable on
        /// the next start. Nothing is written here; the key file appears at the first
        /// [`KeyStore::save`].
        ///
        /// # Errors
        ///
        /// [`KeyStoreError::Failed`] when a key file (or anything else) already exists at
        /// `path`, since its passphrase is the deployment's and is never replaced, and when
        /// `handover` fails. `handover` is not called in the first case.
        pub fn provision(
            path: PathBuf,
            handover: impl FnOnce(&SecretString) -> std::io::Result<()>,
        ) -> Result<FileKeyStore, KeyStoreError> {
            match fs::symlink_metadata(&path) {
                Ok(_) => {
                    return Err(KeyStoreError::Failed(
                        "a key file already exists; its passphrase is not generated".into(),
                    ));
                }
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                Err(e) => return Err(failed(e)),
            }
            let passphrase = FileKeyStore::generate_passphrase();
            handover(&passphrase).map_err(|e| {
                KeyStoreError::Failed(format!(
                    "the generated passphrase was not handed over, so no key file is made: {e}"
                ))
            })?;
            Ok(FileKeyStore::new(path, passphrase))
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
        /// The environment variable is read once, on the first call, then overwritten where
        /// the process keeps it and removed from the environment, whichever source wins, so
        /// no child process this process starts (a harness, and any shell a model runs
        /// inside it) inherits the passphrase, and `/proc/<pid>/environ` no longer shows it
        /// (#315 review N-a; [SEC-KEY-004]). Later calls return the value taken on the
        /// first. A credentials file is still the stronger channel: until this call the
        /// value sat in the environment the process was started with, which the parent that
        /// started it, and anything that can read its memory, could read.
        ///
        /// # Safety
        ///
        /// The first call overwrites and removes an environment variable, which is sound only
        /// while no other thread reads or writes the process environment. Call this at
        /// start-up, before the process starts any other thread (the daemon's `main`, before
        /// it builds a runtime); after the first call it touches the environment only to read
        /// `$CREDENTIALS_DIRECTORY`.
        pub unsafe fn passphrase_from_environment() -> Option<SecretString> {
            let from_env = ENV_PASSPHRASE.get_or_init(|| {
                // One read, kept in zeroizing storage: `var_os` copies the value out of the
                // environment once, `into_vec` moves that copy without another, and the
                // buffer is zeroized when it drops, whether or not the value is UTF-8.
                let raw = std::env::var_os(PASSPHRASE_ENV)
                    .map(|v| Zeroizing::new(std::os::unix::ffi::OsStringExt::into_vec(v)));
                if raw.is_some() {
                    // SAFETY: the caller guarantees no other thread uses the environment
                    // during this first call (see the function's Safety section).
                    unsafe { wipe_and_remove_env(PASSPHRASE_ENV) };
                }
                raw.as_deref()
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .filter(|p| !p.is_empty())
                    .map(|p| SecretString::from(p.to_owned()))
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

        fn file_name(&self) -> Result<&std::ffi::OsStr, KeyStoreError> {
            self.path
                .file_name()
                .ok_or_else(|| KeyStoreError::Failed("key file path has no file name".into()))
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
            checked_dir(dir)?
                .map(drop)
                .ok_or_else(|| KeyStoreError::Failed("key directory vanished".into()))
        }

        /// Creates this save's temporary file: exclusive, `0600`, under a random name. A name
        /// that is taken is skipped, never reused, and its file never touched.
        fn create_temp(
            dir: &Path,
            mut next_name: impl FnMut() -> String,
        ) -> Result<(PathBuf, File), KeyStoreError> {
            for _ in 0..TEMP_NAME_ATTEMPTS {
                let tmp = dir.join(next_name());
                match OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&tmp)
                {
                    Ok(f) => return Ok((tmp, f)),
                    Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(failed(e)),
                }
            }
            Err(KeyStoreError::Failed(
                "no free temporary file name in the key directory".into(),
            ))
        }

        /// [`KeyStore::save`], with the temporary file names drawn from `next_name`. For the
        /// tests of name collisions.
        fn save_named(
            &self,
            seed: &SecretSeed,
            next_name: impl FnMut() -> String,
        ) -> Result<(), KeyStoreError> {
            self.ensure_dir()?;
            let dir = self.dir()?;
            sweep_stale_temp_files(dir);
            let mut recipient = age::scrypt::Recipient::new(self.passphrase.clone());
            if let Some(log_n) = self.work_factor {
                recipient.set_work_factor(log_n);
            }
            let ciphertext = age::encrypt(&recipient, seed.expose())
                .map_err(|e| KeyStoreError::Failed(format!("encrypting the key file: {e}")))?;
            let (tmp, mut file) = FileKeyStore::create_temp(dir, next_name)?;
            // From here the temporary file is ours, and is removed whatever happens.
            let written = (|| {
                file.write_all(&ciphertext)?;
                file.sync_all()?;
                // A hard link never replaces: an existing key file makes it fail
                // ([SEC-KEY-002]), and the key file appears complete or not at all.
                fs::hard_link(&tmp, &self.path)
            })();
            drop(file);
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

    impl KeyStore for FileKeyStore {
        fn load(&self) -> Result<Option<SecretSeed>, KeyStoreError> {
            let dir_path = self.dir()?;
            let name = self.file_name()?;
            let dir = match open_dir(dir_path) {
                Ok(d) => d,
                Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
                Err(e) if matches!(e.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR)) => {
                    return Err(KeyStoreError::Failed(
                        "key directory is not a directory".into(),
                    ));
                }
                Err(e) => return Err(failed(e)),
            };
            // Checked on the open handles, the directory before anything is opened in it: the
            // file read below is the file checked here, in the directory checked here.
            check_private(&dir.metadata().map_err(failed)?, "key directory")?;
            let Some(file) = open_in(&dir, name)? else {
                return Ok(None);
            };
            let meta = file.metadata().map_err(failed)?;
            if !meta.file_type().is_file() {
                return Err(KeyStoreError::Failed(
                    "key file is not a regular file".into(),
                ));
            }
            check_private(&meta, "key file")?;
            if meta.len() > MAX_KEY_FILE {
                return Err(KeyStoreError::Corrupt(
                    "key file is not a device key".into(),
                ));
            }
            sweep_stale_temp_files(dir_path);
            let mut ciphertext = Vec::new();
            file.take(MAX_KEY_FILE + 1)
                .read_to_end(&mut ciphertext)
                .map_err(failed)?;
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
            self.save_named(seed, random_temp_name)
        }
    }

    #[cfg(test)]
    pub(super) mod internals {
        //! The file store's private parts, for `keystore.rs`'s tests.
        use super::*;

        pub fn save_named(
            store: &FileKeyStore,
            seed: &SecretSeed,
            next_name: impl FnMut() -> String,
        ) -> Result<(), KeyStoreError> {
            store.save_named(seed, next_name)
        }

        pub const STALE: Duration = STALE_TEMP_AFTER;
        pub const LIMIT: u64 = MAX_KEY_FILE;
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

        // The file store's follow-ups from the #315 reviews (#70).

        fn names(dir: &std::path::Path) -> Vec<String> {
            let mut v: Vec<String> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            v.sort();
            v
        }

        /// Sets a file's modification time to `age` ago.
        fn age_file(path: &std::path::Path, age: std::time::Duration) {
            let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
            f.set_modified(std::time::SystemTime::now() - age).unwrap();
        }

        #[test]
        fn a_generated_passphrase_is_256_bits_of_hex_and_never_repeats() {
            use age::secrecy::ExposeSecret;
            let a = FileKeyStore::generate_passphrase();
            let b = FileKeyStore::generate_passphrase();
            assert_eq!(a.expose_secret().len(), 64);
            assert!(a.expose_secret().bytes().all(|c| c.is_ascii_hexdigit()));
            assert!(a.expose_secret().bytes().all(|c| !c.is_ascii_uppercase()));
            assert_ne!(a.expose_secret(), b.expose_secret());
        }

        /// C4 section 11: OAC generates the passphrase on first use. The caller gets it
        /// first, and the key file it protects opens with it and with nothing else.
        #[test]
        fn a_provisioned_store_hands_over_its_passphrase_then_makes_the_key() {
            use age::secrecy::ExposeSecret;
            let dir = temp_dir("provision");
            let path = dir.join("oac").join("device.key.age");
            let mut handed = None;
            let store = FileKeyStore::provision(path.clone(), |p| {
                handed = Some(p.expose_secret().to_owned());
                Ok(())
            })
            .unwrap()
            .with_work_factor(10);
            let handed = handed.expect("the passphrase was handed over");
            // Nothing is written until the first save.
            assert!(!dir.exists());
            let key = DeviceKey::load_or_generate(&store).unwrap();
            // The handed-over passphrase opens the file in a later process; another does not.
            let later = self::store(path.clone(), &handed);
            assert_eq!(
                DeviceKey::load_or_generate(&later).unwrap().key_id(),
                key.key_id()
            );
            assert!(matches!(
                self::store(path.clone(), "something else").load(),
                Err(KeyStoreError::Corrupt(_))
            ));
            // The passphrase is not written beside the file it protects.
            assert_eq!(names(path.parent().unwrap()), ["device.key.age"]);
            let bytes = std::fs::read(&path).unwrap();
            assert!(!bytes.windows(handed.len()).any(|w| w == handed.as_bytes()));
            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn provisioning_is_refused_where_a_key_file_exists_and_without_a_handover() {
            let dir = temp_dir("provision-refused");
            let path = dir.join("device.key.age");
            let existing = store(path.clone(), "deployment passphrase");
            DeviceKey::load_or_generate(&existing).unwrap();
            let before = std::fs::read(&path).unwrap();
            let mut called = false;
            let refused = FileKeyStore::provision(path.clone(), |_| {
                called = true;
                Ok(())
            });
            assert!(matches!(refused, Err(KeyStoreError::Failed(_))));
            assert!(!called, "an existing file's passphrase is never replaced");
            assert_eq!(std::fs::read(&path).unwrap(), before);
            std::fs::remove_dir_all(&dir).unwrap();

            // A handover that fails leaves no store and no file.
            let dir = temp_dir("provision-unheld");
            let path = dir.join("oac").join("device.key.age");
            let refused =
                FileKeyStore::provision(path, |_| Err(std::io::Error::other("nowhere to put it")));
            match refused {
                Err(KeyStoreError::Failed(why)) => assert!(why.contains("no key file")),
                other => panic!("{:?}", other.map(|_| ())),
            }
            assert!(!dir.exists());
        }

        /// #315 follow-up: a temporary name that is taken is not reported as "a device key is
        /// already stored", and the file that holds it is never deleted.
        #[test]
        fn a_taken_temporary_name_is_skipped_and_its_file_left_alone() {
            let dir = temp_dir("tmp-collision");
            let path = dir.join("device.key.age");
            let s = store(path.clone(), "pass");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            let taken = dir.join(".device-key.7.aaaa.tmp");
            std::fs::write(&taken, b"somebody else's file").unwrap();
            let mut offered = vec![
                ".device-key.7.aaaa.tmp".to_owned(),
                ".device-key.7.bbbb.tmp".to_owned(),
            ]
            .into_iter();
            file::internals::save_named(&s, &SecretSeed::from_octets(&mut [7; 32]), || {
                offered.next().expect("a name")
            })
            .unwrap();
            assert_eq!(std::fs::read(&taken).unwrap(), b"somebody else's file");
            assert_eq!(s.load().unwrap().unwrap().expose(), &[7; 32]);
            // Ours is gone; theirs and the key file remain.
            assert_eq!(
                names(&dir),
                [".device-key.7.aaaa.tmp", "device.key.age"],
                "only the pre-existing file is left beside the key file"
            );
            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn names_that_are_all_taken_fail_without_touching_any_of_them() {
            let dir = temp_dir("tmp-all-taken");
            let path = dir.join("device.key.age");
            let s = store(path.clone(), "pass");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            let taken = dir.join(".device-key.7.same.tmp");
            std::fs::write(&taken, b"keep").unwrap();
            let r = file::internals::save_named(&s, &SecretSeed::from_octets(&mut [7; 32]), || {
                ".device-key.7.same.tmp".to_owned()
            });
            match r {
                Err(KeyStoreError::Failed(why)) => assert!(why.contains("temporary file name")),
                other => panic!("{other:?}"),
            }
            assert_eq!(std::fs::read(&taken).unwrap(), b"keep");
            assert!(!path.exists());
            std::fs::remove_dir_all(&dir).unwrap();
        }

        /// A real "already stored" is still reported as that, and leaves no temporary file.
        #[test]
        fn a_key_file_that_exists_is_still_reported_and_leaves_no_temporary_file() {
            let dir = temp_dir("already");
            let path = dir.join("device.key.age");
            let s = store(path.clone(), "pass");
            DeviceKey::load_or_generate(&s).unwrap();
            match s.save(&SecretSeed::from_octets(&mut [9; 32])) {
                Err(KeyStoreError::Failed(why)) => assert!(why.contains("already stored")),
                other => panic!("{other:?}"),
            }
            assert_eq!(names(&dir), ["device.key.age"]);
            std::fs::remove_dir_all(&dir).unwrap();
        }

        /// #315 follow-up: a crash leaves an encrypted `.tmp` file; nothing used to clean it.
        #[test]
        fn a_stale_temporary_file_is_removed_by_save_and_by_load() {
            for via_load in [false, true] {
                let dir = temp_dir("stale");
                let path = dir.join("device.key.age");
                let s = store(path.clone(), "pass");
                DeviceKey::load_or_generate(&s).unwrap();
                let old = file::internals::STALE + std::time::Duration::from_secs(5);
                let stale = dir.join(".device-key.123.0011223344556677.tmp");
                let fresh = dir.join(".device-key.124.8899aabbccddeeff.tmp");
                let other = dir.join("notes.tmp");
                let linked = dir.join(".device-key.125.link.tmp");
                let subdir = dir.join(".device-key.126.dir.tmp");
                for f in [&stale, &fresh, &other] {
                    std::fs::write(f, b"ciphertext").unwrap();
                }
                std::os::unix::fs::symlink(&other, &linked).unwrap();
                std::fs::create_dir(&subdir).unwrap();
                age_file(&stale, old);
                age_file(&other, old);
                if via_load {
                    s.load().unwrap().unwrap();
                } else {
                    // The save is refused (a key exists), after the key directory is checked.
                    assert!(s.save(&SecretSeed::from_octets(&mut [1; 32])).is_err());
                }
                assert!(
                    !stale.exists(),
                    "a stale temporary file goes (load: {via_load})"
                );
                assert!(fresh.exists(), "a writer in progress is left alone");
                assert!(other.exists(), "another name is left alone");
                assert!(
                    linked.symlink_metadata().is_ok(),
                    "a link is not followed or removed"
                );
                assert!(subdir.is_dir(), "a directory is left alone");
                std::fs::remove_dir_all(&dir).unwrap();
            }
        }

        /// #315 follow-up: `load` checks the open file, not the path. A key file that is a
        /// pipe is refused rather than opened and waited on.
        #[test]
        fn a_key_file_that_is_a_pipe_is_refused_without_blocking() {
            let dir = temp_dir("fifo");
            let path = dir.join("device.key.age");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
            // SAFETY: `c` is a valid C string for the call.
            assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
            let s = store(path, "pass");
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(matches!(s.load(), Err(KeyStoreError::Failed(_))));
            });
            assert_eq!(
                rx.recv_timeout(std::time::Duration::from_secs(10)),
                Ok(true),
                "a pipe is refused, not waited on"
            );
            std::fs::remove_dir_all(&dir).unwrap();
        }

        /// A directory that is a link is refused, wherever it points (checked on the open
        /// directory, not by path).
        #[test]
        fn a_linked_key_directory_is_refused() {
            let dir = temp_dir("dirlink");
            let real = store(dir.join("real").join("device.key.age"), "pass");
            DeviceKey::load_or_generate(&real).unwrap();
            std::os::unix::fs::symlink(dir.join("real"), dir.join("linked")).unwrap();
            let linked = store(dir.join("linked").join("device.key.age"), "pass");
            assert!(matches!(linked.load(), Err(KeyStoreError::Failed(_))));
            std::fs::remove_dir_all(&dir).unwrap();
        }

        /// The directory is checked before anything is opened in it, so a key directory open
        /// to others is refused on `load` whether or not a key file is in it.
        #[test]
        fn an_open_key_directory_is_refused_before_any_file_is_looked_for() {
            let dir = temp_dir("open-empty");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            let s = store(dir.join("device.key.age"), "pass");
            assert!(matches!(s.load(), Err(KeyStoreError::Failed(_))));
            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn a_key_file_far_larger_than_a_key_is_not_read() {
            let dir = temp_dir("huge");
            let path = dir.join("device.key.age");
            let s = store(path.clone(), "pass");
            DeviceKey::load_or_generate(&s).unwrap();
            let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            f.set_len(file::internals::LIMIT + 1).unwrap();
            drop(f);
            assert!(matches!(s.load(), Err(KeyStoreError::Corrupt(_))));
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
