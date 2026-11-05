// SPDX-License-Identifier: Apache-2.0

//! The durable agreement-key store (#437 S1b; `docs/planning/decisions/G-437-pairing.md` §7):
//! the file that keeps this device's agreement private keys and its highest issued `seq`
//! across a restart ([SEC-SEL-043]), so that `oac start` no longer issues `seq` 0 under a new
//! key every time.
//!
//! **This module handles ciphertext only.** It is given an [`AtRest`] by the daemon, which
//! `oac-core` derived from the device key's seed under labels of its own; that handle
//! serializes and seals the state ([`AtRest::seal_agreement_state`]) and opens it
//! ([`AtRest::open_agreement_state`]), and fixes the device key id the file is named and
//! authenticated by. Nothing here sees the seed, an agreement private key, or the plaintext
//! of the state.
//!
//! # Where
//!
//! `agreement-<device key id>.sealed` in the directory the device key lives in: Windows
//! `%LOCALAPPDATA%\OAC\`, Unix `$XDG_DATA_HOME/oac/`, else `~/.oac/` (the directory the age key
//! file already uses). A test namespace (`OAC_SCOPE`) uses `ns-<name>/` inside it. Only the
//! file of the **current** device key id is read; the files of other key ids (a regenerated
//! device key starts its own count at `seq` 0, lead decision L7) are ignored and never
//! deleted.
//!
//! # Who can read it, and how it is written
//!
//! The same rules as the log and the trust store (`crate::private_file`): on Unix the directory
//! is `0700` and the file `0600`, owned by the user; on Windows both carry a protected DACL
//! with one ACE, for the user. A directory or file that is a link, not the user's, or open to
//! others is **refused, never repaired**. A write goes to a temporary file in the same
//! directory (created exclusively), is `sync_all`'d, and is renamed over the target; on Unix the
//! directory is then synced. A failed write leaves the previous file whole. **The rename is the
//! commit point:** a directory sync that fails after it is a warning, not a failed save, so the
//! store never reports a failure for a file that is in place.
//!
//! # Missing, unreadable
//!
//! A missing file is "nothing saved yet" (a first start; [`AgreementKeyStore::load`] is
//! `None`). A file that exists and cannot be opened (edited, not sealed by this device, another
//! version, too large, loose permissions) is an error, and **it is never reset, repaired or
//! renamed away**: the daemon does not start, because resetting would issue `seq` 0 silently.
//! The operator moves it aside by hand.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use oac_core::at_rest::{AtRest, MAX_FILE_LEN};
use oac_core::keys::KeyStoreError;
use oac_core::sealing::{AgreementKeyState, AgreementKeyStore};

use crate::daemon::AgreementStoreSource;
use crate::trust_store::{OsSteps, WriteSteps, create_private_exclusive, default_directories};

/// What the file's directory is called in a refusal.
const DIRECTORY: &str = "agreement-key directory";

/// What the file is called in a refusal.
const FILE: &str = "agreement-key file";

/// How many temporary names a save tries before it gives up.
const TEMP_NAME_ATTEMPTS: usize = 16;

/// The steps of a save: [`OsSteps`], except that a refusal to make the temporary file private
/// names the agreement-key file, not the trust file.
struct AgreementSteps;

impl WriteSteps for AgreementSteps {
    fn create(&self, path: &Path) -> io::Result<File> {
        create_private_exclusive(path, FILE)
    }

    fn write(&self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
        OsSteps.write(file, bytes)
    }

    fn sync(&self, file: &File) -> io::Result<()> {
        OsSteps.sync(file)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        OsSteps.rename(from, to)
    }

    fn sync_directory(&self, directory: &Path) -> io::Result<()> {
        OsSteps.sync_directory(directory)
    }
}

/// The agreement-key store of one device, in one directory. See the module documentation.
pub struct AgreementFileStore {
    directory: PathBuf,
    top: PathBuf,
    at_rest: AtRest,
    steps: Arc<dyn WriteSteps>,
    warn: Arc<dyn Fn(&str) + Send + Sync>,
    /// One load or save at a time: the stale-temporary sweep is sound only with one writer.
    busy: Mutex<()>,
}

impl std::fmt::Debug for AgreementFileStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgreementFileStore")
            .field("path", &self.path())
            .finish_non_exhaustive()
    }
}

impl AgreementFileStore {
    /// The store of the device `at_rest` belongs to, in `directory`; `top` is `directory` or
    /// the OAC directory that holds it, and is the first made private. `warn` is told of a
    /// problem that does not fail a save.
    pub fn new(
        directory: PathBuf,
        top: PathBuf,
        at_rest: AtRest,
        warn: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> AgreementFileStore {
        AgreementFileStore {
            directory,
            top,
            at_rest,
            steps: Arc::new(AgreementSteps),
            warn,
            busy: Mutex::new(()),
        }
    }

    /// [`AgreementFileStore::new`] with the file operations of `steps`.
    #[cfg(test)]
    pub(crate) fn with_steps(mut self, steps: Arc<dyn WriteSteps>) -> AgreementFileStore {
        self.steps = steps;
        self
    }

    /// The path of this device's file.
    pub fn path(&self) -> PathBuf {
        self.directory.join(self.at_rest.file_name())
    }

    fn failed(&self, what: &str, e: impl std::fmt::Display) -> KeyStoreError {
        KeyStoreError::Failed(format!("{} ({what}): {e}", self.path().display()))
    }

    fn not_private(&self, what: &str) -> KeyStoreError {
        KeyStoreError::Failed(format!(
            "{}: the {what} is accessible to other users (or is a link, or is not yours); it is not repaired: restrict it to your user or move it aside",
            self.path().display()
        ))
    }

    /// Whether the existing `path` is a regular file private to the user; `None` when it is
    /// not there.
    fn existing_file_is_private(&self, path: &Path) -> Result<Option<bool>, KeyStoreError> {
        let meta = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(self.failed("inspecting the file", e)),
        };
        if !meta.file_type().is_file() {
            return Err(KeyStoreError::Failed(format!(
                "{}: not a regular file (a link or directory is not followed or replaced)",
                path.display()
            )));
        }
        let private = crate::private_file::is_private(path)
            .map_err(|e| self.failed("checking the file's access", e))?;
        Ok(Some(private))
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>, KeyStoreError> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(path)
            .map_err(|e| self.failed("opening the file", e))?;
        let mut octets = Vec::new();
        file.take(MAX_FILE_LEN as u64 + 1)
            .read_to_end(&mut octets)
            .map_err(|e| self.failed("reading the file", e))?;
        Ok(octets)
    }

    /// Removes the temporary files a save that ended before its rename left behind. One daemon
    /// of the user keeps the store and saves one at a time, so a file of this name that is not
    /// the current save's is stale.
    fn sweep_stale_temp_files(&self) {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return;
        };
        let prefix = self.at_rest.file_name();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !is_temp_name(&prefix, name) {
                continue;
            }
            let path = entry.path();
            if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file()) {
                let _ = fs::remove_file(path);
            }
        }
    }

    /// Creates this save's temporary file: exclusive, private to the user, under a name that
    /// is not taken. A name that is taken is skipped, never reused, and its file never touched.
    fn create_temp(&self) -> Result<(PathBuf, File), KeyStoreError> {
        for _ in 0..TEMP_NAME_ATTEMPTS {
            let path = self.directory.join(temp_name(&self.at_rest.file_name()));
            // Exclusive and private; on Windows the protected user-only DACL goes on the
            // temporary file before any octet is written to it and travels with it through the
            // rename, so the final file never carries an inherited ACE.
            match self.steps.create(&path) {
                Ok(file) => return Ok((path, file)),
                // A name that is taken is never reused and its file never touched.
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(self.failed("creating the temporary file", e)),
            }
        }
        Err(self.failed("creating the temporary file", "no free temporary file name"))
    }
}

impl AgreementKeyStore for AgreementFileStore {
    fn load(&self) -> Result<Option<AgreementKeyState>, KeyStoreError> {
        let _busy = self
            .busy
            .lock()
            .map_err(|_| KeyStoreError::Failed("lock poisoned".into()))?;
        let path = self.path();
        match self.existing_file_is_private(&path)? {
            // A missing file is a first start: nothing saved yet.
            None => return Ok(None),
            Some(false) => return Err(self.not_private(FILE)),
            Some(true) => {}
        }
        let directory_private = crate::private_file::is_private(&self.directory)
            .map_err(|e| self.failed("checking the directory's access", e))?;
        if !directory_private {
            return Err(self.not_private(DIRECTORY));
        }
        let octets = self.read(&path)?;
        self.at_rest
            .open_agreement_state(&octets)
            .map(Some)
            .map_err(|why| {
                KeyStoreError::Corrupt(format!(
                    "{}: {why}; the file is not changed and not reset: move it aside to start with a new agreement key (peers that hold a higher seq refuse its statement until they pair again)",
                    path.display()
                ))
            })
    }

    fn save(&self, state: &AgreementKeyState) -> Result<(), KeyStoreError> {
        let _busy = self
            .busy
            .lock()
            .map_err(|_| KeyStoreError::Failed("lock poisoned".into()))?;
        let bytes = self
            .at_rest
            .seal_agreement_state(state)
            .map_err(|e| self.failed("sealing the state", e))?;
        crate::private_file::make_private_directory(&self.directory, &self.top, DIRECTORY)
            .map_err(|e| self.failed("making the directory private", e))?;
        let target = self.path();
        // A loose file is refused, not replaced: the rename below would otherwise make it
        // private without anyone being told.
        if self.existing_file_is_private(&target)? == Some(false) {
            return Err(self.not_private(FILE));
        }
        self.sweep_stale_temp_files();

        let (temp, mut file) = self.create_temp()?;
        let written = self
            .steps
            .write(&mut file, &bytes)
            .map_err(|e| self.failed("writing the temporary file", e))
            .and_then(|()| {
                self.steps
                    .sync(&file)
                    .map_err(|e| self.failed("syncing the temporary file", e))
            });
        drop(file);
        if let Err(e) = written {
            let _ = fs::remove_file(&temp);
            return Err(e);
        }
        if let Err(e) = self.steps.rename(&temp, &target) {
            let _ = fs::remove_file(&temp);
            return Err(self.failed("replacing the file", e));
        }
        // The rename is the commit point: the new file is in place and whole, and a load now
        // reads it. A directory sync that fails after it is a warning; reporting the save as
        // failed would leave the caller's idea of the state behind the disk's.
        if let Err(e) = self.steps.sync_directory(&self.directory) {
            (self.warn)(&format!(
                "warning: the agreement-key file {} is in place, but syncing its directory failed ({e}); it may not survive a power loss",
                target.display()
            ));
        }
        Ok(())
    }
}

/// What the daemon is given in place of a finished store: it builds the store once it has
/// the device identity, because the store is sealed under values derived from the device key
/// (`Daemon::start` calls [`AgreementStoreSource::open`] with the identity's handle).
pub struct AgreementFileSource {
    directory: PathBuf,
    top: PathBuf,
    warn: Arc<dyn Fn(&str) + Send + Sync>,
}

impl AgreementFileSource {
    /// The source for `directory`; see [`AgreementFileStore::new`].
    pub fn new(
        directory: PathBuf,
        top: PathBuf,
        warn: impl Fn(&str) + Send + Sync + 'static,
    ) -> AgreementFileSource {
        AgreementFileSource {
            directory,
            top,
            warn: Arc::new(warn),
        }
    }

    /// The source in the user's own data directory, for `namespace` (`OAC_SCOPE`) or the
    /// user's ordinary one (`None`). Nothing is read or made here.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::NotFound`] when there is no data directory (no `LOCALAPPDATA`, or
    /// neither `XDG_DATA_HOME` nor `HOME`), [`io::ErrorKind::InvalidInput`] for a namespace
    /// that is not a scope name.
    pub fn for_user(
        namespace: Option<&str>,
        warn: impl Fn(&str) + Send + Sync + 'static,
    ) -> io::Result<AgreementFileSource> {
        let (directory, top) = default_directories(namespace)?;
        Ok(AgreementFileSource::new(directory, top, warn))
    }
}

impl AgreementStoreSource for AgreementFileSource {
    fn open(self: Box<Self>, at_rest: AtRest) -> Box<dyn AgreementKeyStore + Send> {
        Box::new(AgreementFileStore::new(
            self.directory,
            self.top,
            at_rest,
            self.warn,
        ))
    }
}

/// `<file name>.<32 hexadecimal digits>.tmp`: the process, a counter and the time, so a name is
/// not reused within or across runs; the exclusive create is what guarantees it.
fn temp_name(file_name: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{file_name}.{:08x}{:08x}{nanos:016x}.tmp",
        std::process::id(),
        count as u32
    )
}

fn is_temp_name(file_name: &str, name: &str) -> bool {
    name.strip_prefix(file_name)
        .and_then(|rest| rest.strip_prefix('.'))
        .and_then(|rest| rest.strip_suffix(".tmp"))
        .is_some_and(|hex| hex.len() == 32 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use oac_core::ids::{Timestamp, Token};
    use oac_core::keys::{DeviceIdentity, DeviceKey, SecretSeed};
    use oac_core::sealing::{AgreementPrivateKey, StoredAgreementKey};

    use super::*;

    /// A fresh directory under the system's temporary one, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(tag: &str) -> Temp {
            let dir = std::env::temp_dir()
                .join(format!("oac-agreement-file-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }

        fn store_dir(&self) -> PathBuf {
            self.0.join("data")
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

    fn state(n: u8, seq: u64) -> AgreementKeyState {
        AgreementKeyState {
            keys: vec![StoredAgreementKey {
                private_key: AgreementPrivateKey::from_octets(&mut [n; 32]),
                seq,
                issued_at: Timestamp::parse("2026-11-03T10:00:00Z").unwrap(),
            }],
            highest_seq: Some(seq),
        }
    }

    fn store_in(dir: &Path, id: &DeviceIdentity) -> AgreementFileStore {
        let dir = dir.to_path_buf();
        AgreementFileStore::new(dir.clone(), dir, id.at_rest(), Arc::new(|_| {}))
    }

    /// A step of a save, so that a test can make each one fail.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Step {
        Write,
        Sync,
        Rename,
        SyncDirectory,
    }

    /// Makes one step fail, with an error.
    struct Failing(Step, AtomicBool);

    impl WriteSteps for Failing {
        fn create(&self, path: &Path) -> io::Result<File> {
            AgreementSteps.create(path)
        }
        fn write(&self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
            self.check(Step::Write)?;
            OsSteps.write(file, bytes)
        }
        fn sync(&self, file: &File) -> io::Result<()> {
            self.check(Step::Sync)?;
            OsSteps.sync(file)
        }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.check(Step::Rename)?;
            OsSteps.rename(from, to)
        }
        fn sync_directory(&self, directory: &Path) -> io::Result<()> {
            self.check(Step::SyncDirectory)?;
            OsSteps.sync_directory(directory)
        }
    }

    impl Failing {
        fn new(step: Step) -> Failing {
            Failing(step, AtomicBool::new(true))
        }

        fn check(&self, step: Step) -> io::Result<()> {
            if step == self.0 && self.1.load(Ordering::SeqCst) {
                Err(io::Error::other("injected"))
            } else {
                Ok(())
            }
        }
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_missing_file_is_a_first_start() {
        let temp = Temp::new("missing");
        let id = identity(1);
        // Neither the directory nor the file exists.
        let store = store_in(&temp.store_dir(), &id);
        assert!(store.load().unwrap().is_none());
        assert!(!temp.store_dir().exists(), "loading made nothing");
        // The directory exists, the file does not.
        fs::create_dir_all(temp.store_dir()).unwrap();
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn the_file_is_private_and_written_atomically() {
        let temp = Temp::new("private");
        let id = identity(1);
        let store = store_in(&temp.store_dir(), &id);
        store.save(&state(0x61, 0)).unwrap();
        let path = store.path();
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            id.at_rest().file_name()
        );
        // Private: the file and its directory; and no temporary file is left behind.
        assert!(crate::private_file::is_private(&path).unwrap());
        assert!(crate::private_file::is_private(&temp.store_dir()).unwrap());
        assert_eq!(files(&temp.store_dir()), [id.at_rest().file_name()]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(temp.store_dir()).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        // A second save replaces it whole and the first save's state is gone.
        store.save(&state(0x62, 1)).unwrap();
        assert_eq!(files(&temp.store_dir()), [id.at_rest().file_name()]);
        assert_eq!(store.load().unwrap().unwrap().highest_seq, Some(1));
    }

    #[test]
    fn a_failed_write_leaves_the_previous_file_whole() {
        for step in [Step::Write, Step::Sync, Step::Rename] {
            let temp = Temp::new(&format!("fail-{step:?}").to_lowercase());
            let id = identity(1);
            let dir = temp.store_dir();
            store_in(&dir, &id).save(&state(0x61, 0)).unwrap();
            let before = fs::read(store_in(&dir, &id).path()).unwrap();

            let failing = store_in(&dir, &id).with_steps(Arc::new(Failing::new(step)));
            let e = failing.save(&state(0x62, 1)).unwrap_err();
            assert!(matches!(e, KeyStoreError::Failed(_)), "{step:?}: {e}");
            assert_eq!(
                fs::read(failing.path()).unwrap(),
                before,
                "{step:?}: the previous file is whole"
            );
            assert_eq!(
                files(&dir),
                [id.at_rest().file_name()],
                "{step:?}: no temp left"
            );
            assert_eq!(
                store_in(&dir, &id).load().unwrap().unwrap().highest_seq,
                Some(0),
                "{step:?}"
            );
        }
    }

    #[test]
    fn a_directory_sync_that_fails_after_the_rename_is_a_warning_not_a_failed_save() {
        let temp = Temp::new("dirsync");
        let id = identity(1);
        let warned = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = warned.clone();
        let dir = temp.store_dir();
        let store = AgreementFileStore::new(
            dir.clone(),
            dir,
            id.at_rest(),
            Arc::new(move |m| sink.lock().unwrap().push(m.to_owned())),
        )
        .with_steps(Arc::new(Failing::new(Step::SyncDirectory)));
        store.save(&state(0x61, 4)).unwrap();
        // The file is in place and is what a load reads: memory and disk agree.
        assert_eq!(store.load().unwrap().unwrap().highest_seq, Some(4));
        let warned = warned.lock().unwrap();
        assert_eq!(warned.len(), 1, "{warned:?}");
        assert!(warned[0].contains("in place"), "{warned:?}");
    }

    #[test]
    fn a_file_that_does_not_open_is_an_error_and_is_not_changed() {
        let temp = Temp::new("tamper");
        let id = identity(1);
        let store = store_in(&temp.store_dir(), &id);
        store.save(&state(0x61, 2)).unwrap();
        let path = store.path();
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&path, &bytes).unwrap();
        // `fs::write` keeps the private mode of an existing file.
        match store.load() {
            Err(KeyStoreError::Corrupt(why)) => {
                assert!(why.contains("not changed and not reset"), "{why}");
            }
            other => panic!("expected a corrupt-file refusal, got {other:?}"),
        }
        assert_eq!(fs::read(&path).unwrap(), bytes, "not reset or repaired");
        // And a save is not what repairs it: the next start still stops.
        assert!(matches!(store.load(), Err(KeyStoreError::Corrupt(_))));
    }

    #[test]
    fn an_oversize_file_is_refused_without_reading_all_of_it() {
        let temp = Temp::new("oversize");
        let id = identity(1);
        let store = store_in(&temp.store_dir(), &id);
        store.save(&state(0x61, 2)).unwrap();
        fs::write(store.path(), vec![0u8; MAX_FILE_LEN + 4096]).unwrap();
        assert!(matches!(store.load(), Err(KeyStoreError::Corrupt(_))));
    }

    #[test]
    fn a_second_device_key_id_gets_its_own_file_unit() {
        let temp = Temp::new("regenerated");
        let dir = temp.store_dir();
        let (old, new) = (identity(1), identity(2));
        let old_store = store_in(&dir, &old);
        old_store.save(&state(0x61, 5)).unwrap();
        let old_bytes = fs::read(old_store.path()).unwrap();

        // The new device key reads nothing of the old file: its own file is missing.
        let new_store = store_in(&dir, &new);
        assert!(new_store.load().unwrap().is_none());
        new_store.save(&state(0x62, 0)).unwrap();
        assert_eq!(new_store.load().unwrap().unwrap().highest_seq, Some(0));
        // The old file is untouched, and still opens under the old key.
        assert_eq!(fs::read(old_store.path()).unwrap(), old_bytes);
        assert_eq!(old_store.load().unwrap().unwrap().highest_seq, Some(5));
        assert_eq!(files(&dir).len(), 2);
        assert_ne!(old_store.path(), new_store.path());
    }

    #[test]
    fn a_file_renamed_to_another_key_id_is_refused_on_disk() {
        let temp = Temp::new("renamed");
        let dir = temp.store_dir();
        let (a, b) = (identity(1), identity(2));
        let a_store = store_in(&dir, &a);
        a_store.save(&state(0x61, 3)).unwrap();
        let b_store = store_in(&dir, &b);
        fs::rename(a_store.path(), b_store.path()).unwrap();
        assert!(matches!(b_store.load(), Err(KeyStoreError::Corrupt(_))));
    }

    #[cfg(unix)]
    #[test]
    fn a_loose_mode_is_refused_not_repaired() {
        use std::os::unix::fs::PermissionsExt;
        let temp = Temp::new("loose");
        let id = identity(1);
        let dir = temp.store_dir();
        let store = store_in(&dir, &id);
        store.save(&state(0x61, 1)).unwrap();
        let path = store.path();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        for result in [store.load().map(drop), store.save(&state(0x61, 2))] {
            match result {
                Err(KeyStoreError::Failed(why)) => {
                    assert!(why.contains("accessible to other users"), "{why}");
                }
                other => panic!("expected a refusal, got {other:?}"),
            }
        }
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644,
            "not repaired"
        );
    }

    #[test]
    fn a_stale_temporary_file_is_swept_and_a_foreign_one_is_not() {
        let temp = Temp::new("sweep");
        let id = identity(1);
        let dir = temp.store_dir();
        let store = store_in(&dir, &id);
        store.save(&state(0x61, 1)).unwrap();
        let name = id.at_rest().file_name();
        let stale = dir.join(format!("{name}.{}.tmp", "0".repeat(32)));
        let other_device = dir.join(format!(
            "agreement-{}.sealed.{}.tmp",
            "f".repeat(64),
            "1".repeat(32)
        ));
        let unrelated = dir.join("notes.txt");
        for p in [&stale, &other_device, &unrelated] {
            fs::write(p, b"x").unwrap();
        }
        store.save(&state(0x61, 2)).unwrap();
        assert!(!stale.exists());
        assert!(
            other_device.exists(),
            "another key id's temporary file is not ours"
        );
        assert!(unrelated.exists());
    }

    #[test]
    fn a_namespace_has_its_own_directory_inside_the_data_directory() {
        let (plain, top) = default_directories(None).unwrap();
        assert_eq!(plain, top);
        let (ns, top2) = default_directories(Some("abc1")).unwrap();
        assert_eq!(top, top2);
        assert_eq!(ns, top.join("ns-abc1"));
        assert!(default_directories(Some("NOT valid")).is_err());
    }

    #[test]
    fn the_source_holds_no_seed_and_no_plaintext_key() {
        // A source check over the whole binary side of the store: this module, the daemon's seam
        // (`daemon.rs`) and the place `oac start` wires it (`commands.rs`), production code
        // only (comments and everything from `mod tests` on are left out). They handle
        // ciphertext and the opaque `AtRest` handle only: no seed type, no accessor that
        // exposes a private key, no key type. The device-key stores in `keystore.rs` take a
        // `SecretSeed` by design and are not in scope.
        let production = |text: &str| -> String {
            text.split("mod tests {")
                .next()
                .unwrap()
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        for (name, source) in [
            ("agreement_file.rs", include_str!("agreement_file.rs")),
            ("daemon.rs", include_str!("daemon.rs")),
            ("commands.rs", include_str!("commands.rs")),
        ] {
            let source = production(source);
            for forbidden in ["SecretSeed", "expose(", "AgreementPrivateKey"] {
                assert!(!source.contains(forbidden), "{forbidden} in {name}");
            }
        }
        // `DeviceKey` is how the daemon reads the key (once, `DeviceKey::load_or_generate`);
        // the agreement store itself never names it.
        assert!(!production(include_str!("agreement_file.rs")).contains("DeviceKey"));
    }
}
