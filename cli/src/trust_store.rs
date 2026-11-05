// SPDX-License-Identifier: Apache-2.0

//! The durable trust store: `trust.json`, the file the device's pairings, grants, relay
//! settings and held agreement statements are kept in across restarts (#437, slice S1;
//! `docs/planning/decisions/G-437-pairing.md` §5). [`FileTrustStore`] is the `PairingStore` of
//! the `oac` binary; the core stays store-agnostic.
//!
//! # The file
//!
//! One canonical (RFC 8785) JSON object, at most [`MAX_TRUST_FILE_BYTES`] (1 MiB):
//!
//! ```text
//! {"device_key_id":...,"grants":[...],"oac_trust_store":1,"paired":[{"confirmed":
//!  {"agreement_key":...,"issued_at":...,"seq":...},"key_id":...,"paired_at":...,
//!  "principal":...,"public_key":...}],"relay_enabled":[...],"statements":[{"agreement_key":...,
//!  "issued_at":...,"key_id":...,"seq":...}]}
//! ```
//!
//! Arrays are sorted (`paired` and `statements` by `key_id`, `relay_enabled` by session id;
//! `grants` keep the engine's order), so equal snapshots give equal bytes. A grant is
//! `{"direction":"inbound"|"outbound","target":..,"writer":..}`; a peer side is
//! `{"key_id":..}` or `{"key_id":..,"session_id":..}`; a local side is
//! `{"kind":"device"}`, `{"kind":"session","session_id":..}` or `{"kind":"scope","scope":..}`.
//!
//! Parsing is strict: an unknown member, an unknown `oac_trust_store` version, a `key_id` that
//! is not the SHA-256 of its `public_key`, or a key that fails [SEC-KEY-034] refuses the file.
//! The reader keeps one place per object that names its members ([`ROOT_MEMBERS`] and the
//! `exact` calls), so a later member (S1c added `agreement_seq_floor`) is one more entry there, not
//! a change of the format's version.
//!
//! `agreement_seq_floor` (optional; #437 S1c) is `{"key_id":..,"seq":..}`: the highest agreement
//! `seq` the device key `key_id` has issued, written **before** the statement with that `seq`
//! exists (G-437 §7). `trust.json` is not bound to the device key, so the floor names the key id
//! it belongs to and is ignored by every other ([`PairingStore::agreement_seq_floor`]). It
//! holds one key id: recording a floor for another replaces it, so going back to an old device
//! key finds its floor gone (the L7 residual: that key's own file still carries its count). A
//! save of the snapshot carries the floor through unchanged; only
//! [`PairingStore::record_agreement_seq_floor`] changes it. It is read strictly like every other
//! member: both members are required, `key_id` is a key id and `seq` an integer in the range of
//! a statement's `seq`.
//!
//! `device_key_id` is the key id of the device key that wrote the file, in plain text, **for
//! diagnosis only** (lead decision L6, L7). It is not checked for integrity and binds the file
//! to nothing: if it differs from the running daemon's key id the trusted peers stay loaded and
//! [`PairingStore::note_device_key`] reports it. The file carries no tag: a process of the
//! same user that can write it can add a trusted key, a grant and the relay switch (accepted
//! same-user residual, G-437 §5.4, T7).
//!
//! # Where, and who can read it
//!
//! Windows `%LOCALAPPDATA%\OAC\trust.json`; Unix `$XDG_DATA_HOME/oac/trust.json`, else
//! `~/.oac/trust.json`. A test namespace (`OAC_SCOPE`) uses `ns-<name>/` inside that
//! directory. The directory and the file are private to the user (`crate::private_file`).
//!
//! - **Unix:** the directory is `0700` and the file `0600`. A link, a file or directory that is
//!   not the user's, or one open to others is **refused, never repaired**, by `load` (when the
//!   file exists) and by `save`.
//! - **Windows:** `load` refuses a file or directory whose DACL is not a single user ACE (when
//!   the file exists), and a link. `save` does **not** refuse a directory: every save replaces
//!   the DACL of the directory and of `top` (`%LOCALAPPDATA%\OAC`) with the protected,
//!   user-only one, and reads it back. The file itself carries the protected one-ACE DACL of
//!   its temporary file across the rename; a loose existing file is still refused by `save`.
//!
//! # Missing, corrupt, unknown
//!
//! A missing file is the empty store (default deny). A file that cannot be parsed, exceeds
//! 1 MiB or has an unknown version is [`PairingStoreError::Corrupt`], and a file that cannot
//! be read or is not private is [`PairingStoreError::Failed`]; either stops `Daemon::start`
//! with an error naming the file. **The file is never reset, repaired or renamed away**: the
//! operator moves it aside by hand. The store creates nothing until the first save or the
//! first recorded floor.
//!
//! # Written atomically
//!
//! A save writes a temporary file in the same directory (a random name, created exclusively,
//! private), `sync_all`, `rename` over the target, then `fsync` of the directory on Unix. A
//! failure of the create, write, sync or rename step returns [`PairingStoreError::Failed`]
//! and leaves the previous file untouched. **The rename is the commit point**: once it has
//! replaced `trust.json` the change is in place and whole, and `save` returns `Ok`. A failure
//! of the directory sync after it then only leaves durability across a power loss unproven (as
//! on Windows, where no directory flush is made): it is counted
//! ([`FileTrustStore::directory_sync_failures`]) and reported as one warning line to the sink
//! given with [`FileTrustStore::with_warning_sink`] (the daemon's log). Returning an error
//! there would make the engine roll back a pairing or grant that the next start loads from the
//! file. The steps are the methods of [`WriteSteps`], so a test makes each one fail.
//!
//! **Windows durability is UNVERIFIED** (G-437 §5.3): that a completed `rename` over an existing
//! file survives a power loss was not established from a first-party statement, and no flush of
//! the directory is made (`MoveFileExW` documents `MOVEFILE_WRITE_THROUGH` only for a move done
//! as copy and delete). The residual: a power loss may bring back the previous file (an older
//! store; after an unpair, the revoked device).

use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use oac_core::authorization::{Grant, LocalSide, PeerSide, WorkingDirectoryScope};
use oac_core::canonical::canonical;
use oac_core::ids::{KeyId, SessionId, Timestamp, Token};
use oac_core::json::{self, Json, JsonObject, is_ijson_string};
use oac_core::keys::{PublicKey, SecretSeed};
use oac_core::pairing::{PairingRecord, PairingSnapshot, PairingStore, PairingStoreError};
use oac_core::sealing::{AgreementPublicKey, ConfirmedValues, HeldStatement};

/// The trust file's name.
pub const TRUST_FILE_NAME: &str = "trust.json";
/// The largest trust file read or written, in bytes (1 MiB).
pub const MAX_TRUST_FILE_BYTES: u64 = 1024 * 1024;
/// How many random temporary names a save tries before it gives up.
const TEMP_NAME_ATTEMPTS: usize = 8;
/// What the files and the directory are called in an error.
const FILE: &str = "trust file";
const DIRECTORY: &str = "trust store directory";

/// The format version this reader and writer speak (member `oac_trust_store`).
const FORMAT_VERSION: u64 = 1;
const VERSION_MEMBER: &str = "oac_trust_store";
/// Every member the root object may have, and which of them must be there.
const ROOT_MEMBERS: [&str; 7] = [
    "agreement_seq_floor",
    "device_key_id",
    "grants",
    "oac_trust_store",
    "paired",
    "relay_enabled",
    "statements",
];
const ROOT_REQUIRED: [&str; 5] = [
    "grants",
    "oac_trust_store",
    "paired",
    "relay_enabled",
    "statements",
];

// ---- the write steps ---------------------------------------------------------------------

/// The five steps of an atomic save, as a seam: the real ones are [`OsSteps`]; a test makes
/// any one fail (`tests/trust_store.rs`).
pub trait WriteSteps: Send + Sync {
    /// Creates `path` **exclusively** (`ErrorKind::AlreadyExists` when it exists), private to
    /// the user, and returns it open for writing.
    ///
    /// # Errors
    ///
    /// The operating system's error. A file this call made is removed again when it fails.
    fn create(&self, path: &Path) -> io::Result<File>;

    /// Writes all of `bytes` to `file`.
    ///
    /// # Errors
    ///
    /// The operating system's error.
    fn write(&self, file: &mut File, bytes: &[u8]) -> io::Result<()>;

    /// Makes what was written durable (`sync_all`).
    ///
    /// # Errors
    ///
    /// The operating system's error.
    fn sync(&self, file: &File) -> io::Result<()>;

    /// Renames `from` over `to`, replacing it.
    ///
    /// # Errors
    ///
    /// The operating system's error.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;

    /// Makes the rename durable: `fsync` of `directory` on Unix. On Windows nothing is done
    /// (see the module documentation).
    ///
    /// # Errors
    ///
    /// The operating system's error.
    fn sync_directory(&self, directory: &Path) -> io::Result<()>;
}

/// The operating system's steps.
#[derive(Clone, Copy, Debug, Default)]
pub struct OsSteps;

/// Creates `path` exclusively, private to the user, and returns it open for writing; `what`
/// names the file in a refusal ("trust file", "agreement-key file"). A file this call made is
/// removed again when it fails.
pub(crate) fn create_private_exclusive(path: &Path, what: &str) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    // Harden and check what was made: on Windows the file inherited the directory's one
    // ACE and gets its own protected DACL here (it is the temporary file's descriptor
    // that the final file keeps across the rename); on Unix the mode and owner are
    // checked. The second handle is only for that; the first is the one written.
    match crate::private_file::open_private_file(path, false, what) {
        Ok(_) => Ok(file),
        Err(e) => {
            drop(file);
            let _ = fs::remove_file(path);
            Err(e)
        }
    }
}

impl WriteSteps for OsSteps {
    fn create(&self, path: &Path) -> io::Result<File> {
        create_private_exclusive(path, FILE)
    }

    fn write(&self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
        file.write_all(bytes)?;
        file.flush()
    }

    fn sync(&self, file: &File) -> io::Result<()> {
        file.sync_all()
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn sync_directory(&self, directory: &Path) -> io::Result<()> {
        #[cfg(unix)]
        {
            File::open(directory)?.sync_all()
        }
        #[cfg(not(unix))]
        {
            let _ = directory;
            Ok(())
        }
    }
}

// ---- the store ---------------------------------------------------------------------------

#[derive(Default)]
struct State {
    /// The running daemon's key id, once told: written into every later save.
    device_key_id: Option<KeyId>,
    /// The `device_key_id` the last load found in the file.
    recorded: Option<KeyId>,
    /// The `agreement_seq_floor` the file holds: `None` while no read has told, then
    /// `Some(None)` or `Some(floor)`. Every save writes it back, so a save of the snapshot
    /// never loses it.
    floor: Option<Option<SeqFloor>>,
}

/// The `agreement_seq_floor` member: the highest agreement `seq` issued by the device key
/// `key_id` (#437 S1c; G-437 §7).
#[derive(Clone, Debug, PartialEq, Eq)]
struct SeqFloor {
    key_id: KeyId,
    seq: u64,
}

/// The `PairingStore` that keeps `trust.json` (see the module documentation).
pub struct FileTrustStore {
    directory: PathBuf,
    top: PathBuf,
    steps: Arc<dyn WriteSteps>,
    state: Mutex<State>,
    /// Where a durability warning goes (the daemon's log), if anywhere.
    warning_sink: Option<WarningSink>,
    /// How many saves replaced the file but could not sync the directory.
    directory_sync_failures: AtomicU64,
}

/// Receives the text of a warning: one line, with no content of the store in it.
pub type WarningSink = Arc<dyn Fn(&str) + Send + Sync>;

impl FileTrustStore {
    /// The store whose file is `directory/trust.json`. `top` is the directory OAC owns that
    /// `directory` is or is inside. Every save makes `directory` and `top` private (Unix:
    /// created `0700` and checked, never repaired; Windows: **their DACLs are replaced** with
    /// the protected user-only one, on every save). Nothing is read or made here.
    pub fn new(directory: PathBuf, top: PathBuf) -> FileTrustStore {
        FileTrustStore::with_steps(directory, top, Arc::new(OsSteps))
    }

    /// This store, with `sink` given a line whenever a save replaced the file but could not
    /// sync the directory (see the module documentation).
    #[must_use]
    pub fn with_warning_sink(mut self, sink: WarningSink) -> FileTrustStore {
        self.warning_sink = Some(sink);
        self
    }

    /// How many saves replaced the file but failed to sync the directory since this store was
    /// made: their durability across a power loss is unproven. For the daemon's `status` and
    /// `doctor` to report (S5).
    pub fn directory_sync_failures(&self) -> u64 {
        self.directory_sync_failures.load(Ordering::SeqCst)
    }

    /// [`FileTrustStore::new`] with `steps` as the write steps, for a test that makes one fail.
    pub fn with_steps(
        directory: PathBuf,
        top: PathBuf,
        steps: Arc<dyn WriteSteps>,
    ) -> FileTrustStore {
        FileTrustStore {
            directory,
            top,
            steps,
            state: Mutex::new(State::default()),
            warning_sink: None,
            directory_sync_failures: AtomicU64::new(0),
        }
    }

    /// The store in the user's own data directory, for `namespace` (`OAC_SCOPE`) or the
    /// user's ordinary one (`None`). Nothing is read or made here.
    ///
    /// **A test must not save to this store**: on Windows a save replaces the DACL of the
    /// real user's `%LOCALAPPDATA%\OAC`, and on every platform it writes the real user's
    /// data directory. Tests use [`FileTrustStore::new`] over a temporary path.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::NotFound`] when there is no data directory (no `LOCALAPPDATA`, or
    /// neither `XDG_DATA_HOME` nor `HOME`), [`io::ErrorKind::InvalidInput`] for a namespace
    /// that is not a scope name.
    pub fn for_user(namespace: Option<&str>) -> io::Result<FileTrustStore> {
        let (directory, top) = default_directories(namespace)?;
        Ok(FileTrustStore::new(directory, top))
    }

    /// The trust file's path.
    pub fn path(&self) -> PathBuf {
        self.directory.join(TRUST_FILE_NAME)
    }

    fn failed(&self, what: &str, e: impl std::fmt::Display) -> PairingStoreError {
        PairingStoreError::Failed(format!("{} ({what}): {e}", self.path().display()))
    }

    fn corrupt(&self, why: impl std::fmt::Display) -> PairingStoreError {
        PairingStoreError::Corrupt(format!(
            "{}: {why}; the file is not changed and not reset: move it aside to start with an empty store",
            self.path().display()
        ))
    }

    fn not_private(&self, what: &str) -> PairingStoreError {
        PairingStoreError::Failed(format!(
            "{}: the {what} is accessible to other users (or is a link, or is not yours); it is not repaired: restrict it to your user or move it aside",
            self.path().display()
        ))
    }

    /// Whether the existing `path` is a regular file private to the user; `None` when it is
    /// not there.
    fn existing_file_is_private(&self, path: &Path) -> Result<Option<bool>, PairingStoreError> {
        let meta = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(self.failed("inspecting the file", e)),
        };
        if !meta.file_type().is_file() {
            return Err(PairingStoreError::Failed(format!(
                "{}: not a regular file (a link or directory is not followed or replaced)",
                path.display()
            )));
        }
        let private = crate::private_file::is_private(path)
            .map_err(|e| self.failed("checking the file's access", e))?;
        Ok(Some(private))
    }

    fn read(&self) -> Result<Vec<u8>, PairingStoreError> {
        let path = self.path();
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(&path)
            .map_err(|e| self.failed("opening the file", e))?;
        let mut octets = Vec::new();
        file.take(MAX_TRUST_FILE_BYTES + 1)
            .read_to_end(&mut octets)
            .map_err(|e| self.failed("reading the file", e))?;
        if octets.len() as u64 > MAX_TRUST_FILE_BYTES {
            return Err(self.corrupt("larger than 1 MiB"));
        }
        Ok(octets)
    }

    /// Removes the temporary files a save that ended before its rename left behind. One
    /// daemon of the user keeps the store and saves one at a time, so a file of this name that
    /// is not the current save's is stale.
    fn sweep_stale_temp_files(&self) {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !is_temp_name(name) {
                continue;
            }
            let path = entry.path();
            if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file()) {
                let _ = fs::remove_file(path);
            }
        }
    }

    fn create_temp(&self) -> Result<(PathBuf, File), PairingStoreError> {
        for _ in 0..TEMP_NAME_ATTEMPTS {
            let path = self.directory.join(random_temp_name());
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

impl std::fmt::Debug for FileTrustStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileTrustStore")
            .field("path", &self.path())
            .finish_non_exhaustive()
    }
}

impl PairingStore for FileTrustStore {
    fn load(&self) -> Result<PairingSnapshot, PairingStoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| PairingStoreError::Failed("lock poisoned".into()))?;
        match self.read_file()? {
            // A missing file is the empty store: default deny.
            None => {
                state.recorded = None;
                state.floor = Some(None);
                Ok(PairingSnapshot::default())
            }
            Some(file) => {
                state.recorded = file.device_key_id;
                state.floor = Some(file.agreement_seq_floor);
                Ok(file.snapshot)
            }
        }
    }

    fn save(&self, snapshot: &PairingSnapshot) -> Result<(), PairingStoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| PairingStoreError::Failed("lock poisoned".into()))?;
        // The floor the file holds is carried through: a save of the snapshot must not lose it.
        let floor = self.known_floor(&mut state)?;
        self.write_file(&TrustFile {
            device_key_id: state.device_key_id.clone(),
            snapshot: snapshot.clone(),
            agreement_seq_floor: floor,
        })
    }

    fn note_device_key(&self, key_id: &KeyId) -> Option<KeyId> {
        // The state is plain values, each set whole, so a lock poisoned by a panic
        // elsewhere holds nothing half-written: its contents are used as they are.
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.device_key_id = Some(key_id.clone());
        state.recorded.clone().filter(|recorded| recorded != key_id)
    }

    fn agreement_seq_floor(&self, key_id: &KeyId) -> Result<Option<u64>, PairingStoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| PairingStoreError::Failed("lock poisoned".into()))?;
        // A floor that names another key id is not this key's: ignored (L7).
        Ok(self
            .known_floor(&mut state)?
            .filter(|f| &f.key_id == key_id)
            .map(|f| f.seq))
    }

    fn record_agreement_seq_floor(
        &self,
        key_id: &KeyId,
        seq: u64,
    ) -> Result<(), PairingStoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| PairingStoreError::Failed("lock poisoned".into()))?;
        // Monotonic for this key id: a lower `seq` leaves the floor where it is, except at the
        // maximum, the only count the [SEC-SEL-052] restart starts again from. Another key
        // id's floor is replaced.
        let current = self.known_floor(&mut state)?;
        let seq = match &current {
            Some(f)
                if &f.key_id == key_id
                    && f.seq > seq
                    && f.seq < oac_core::sealing::MAX_STATEMENT_SEQ =>
            {
                f.seq
            }
            _ => seq,
        };
        if current
            .as_ref()
            .is_some_and(|f| &f.key_id == key_id && f.seq == seq)
        {
            return Ok(());
        }
        // The file is rewritten whole with the snapshot it holds now (the engine's own save
        // writes the snapshot); a missing file is the empty snapshot.
        let (snapshot, held_device_key_id) = match self.read_file()? {
            Some(file) => (file.snapshot, file.device_key_id),
            None => (PairingSnapshot::default(), None),
        };
        let floor = SeqFloor {
            key_id: key_id.clone(),
            seq,
        };
        self.write_file(&TrustFile {
            device_key_id: state.device_key_id.clone().or(held_device_key_id),
            snapshot,
            agreement_seq_floor: Some(floor.clone()),
        })?;
        state.floor = Some(Some(floor));
        Ok(())
    }
}

impl FileTrustStore {
    /// The floor the file holds: the one the last read or write found, else read now.
    fn known_floor(&self, state: &mut State) -> Result<Option<SeqFloor>, PairingStoreError> {
        if state.floor.is_none() {
            state.floor = Some(self.read_file()?.and_then(|f| f.agreement_seq_floor));
        }
        Ok(state.floor.clone().flatten())
    }

    /// The file, checked and parsed; `None` when it is not there.
    fn read_file(&self) -> Result<Option<TrustFile>, PairingStoreError> {
        let path = self.path();
        match self.existing_file_is_private(&path)? {
            None => return Ok(None),
            Some(false) => return Err(self.not_private("trust file")),
            Some(true) => {}
        }
        let directory_private = crate::private_file::is_private(&self.directory)
            .map_err(|e| self.failed("checking the directory's access", e))?;
        if !directory_private {
            return Err(self.not_private("trust store directory"));
        }
        if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_TRUST_FILE_BYTES) {
            return Err(self.corrupt("larger than 1 MiB"));
        }
        let octets = self.read()?;
        decode(&octets).map(Some).map_err(|why| self.corrupt(why))
    }

    /// Writes `file` atomically (see the module documentation).
    fn write_file(&self, contents: &TrustFile) -> Result<(), PairingStoreError> {
        let text = encode(contents).map_err(|why| self.failed("encoding the snapshot", why))?;
        if text.len() as u64 > MAX_TRUST_FILE_BYTES {
            return Err(self.failed(
                "encoding the snapshot",
                "the store would be larger than 1 MiB",
            ));
        }
        crate::private_file::make_private_directory(&self.directory, &self.top, DIRECTORY)
            .map_err(|e| self.failed("making the directory private", e))?;
        let target = self.path();
        // A loose file is refused, not replaced: the rename below would otherwise make it
        // private without anyone being told.
        if self.existing_file_is_private(&target)? == Some(false) {
            return Err(self.not_private("trust file"));
        }
        self.sweep_stale_temp_files();

        let (temp, mut file) = self.create_temp()?;
        let written = self
            .steps
            .write(&mut file, text.as_bytes())
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
        // The rename is the commit point: the new file is in place and whole, so the change
        // stands (the engine's memory and the file agree). A directory sync that fails
        // afterwards only leaves its durability across a power loss unproven, as on Windows,
        // where no directory flush is made at all: a warning and a count, not an error that
        // would make the engine roll back a change the next start would load.
        if let Err(e) = self.steps.sync_directory(&self.directory) {
            self.directory_sync_failures.fetch_add(1, Ordering::SeqCst);
            if let Some(sink) = &self.warning_sink {
                sink(&format!(
                    "warning: the trust store was saved but the directory could not be synced, so the change may not survive a power loss ({}): {e}",
                    self.path().display()
                ));
            }
        }
        Ok(())
    }
}

/// `trust.json.<32 hexadecimal digits>.tmp`
///
/// The random bytes come from `SecretSeed::generate` (the operating system's generator), as
/// the age key file's temporary names do: this crate depends on no other random source and
/// the name is not a secret.
fn random_temp_name() -> String {
    let seed = SecretSeed::generate();
    let mut name = format!("{TRUST_FILE_NAME}.");
    for byte in &seed.expose()[..16] {
        let _ = write!(name, "{byte:02x}");
    }
    name.push_str(".tmp");
    name
}

fn is_temp_name(name: &str) -> bool {
    name.strip_prefix(TRUST_FILE_NAME)
        .and_then(|rest| rest.strip_prefix('.'))
        .and_then(|rest| rest.strip_suffix(".tmp"))
        .is_some_and(|hex| hex.len() == 32 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The data directory of the user, and the one of OAC's own that is the first made private.
pub(crate) fn default_directories(namespace: Option<&str>) -> io::Result<(PathBuf, PathBuf)> {
    let top = {
        #[cfg(windows)]
        {
            std::env::var_os("LOCALAPPDATA")
                .filter(|v| !v.is_empty())
                .map(|base| PathBuf::from(base).join("OAC"))
        }
        #[cfg(not(windows))]
        {
            // As the age key file (`FileKeyStore::default_path`).
            match std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
                Some(d) => Some(PathBuf::from(d).join("oac")),
                None => std::env::var_os("HOME")
                    .filter(|v| !v.is_empty())
                    .map(|h| PathBuf::from(h).join(".oac")),
            }
        }
    }
    .ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no per-user data directory (LOCALAPPDATA, XDG_DATA_HOME or HOME is not set)",
        )
    })?;
    match namespace {
        None => Ok((top.clone(), top)),
        Some(n) if crate::ipc::scope_name_is_valid(n) => Ok((top.join(format!("ns-{n}")), top)),
        Some(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a scope name is 1 to 24 lower-case letters and digits",
        )),
    }
}

// ---- the format --------------------------------------------------------------------------

/// What the file holds: the snapshot and the diagnostic key id.
struct TrustFile {
    device_key_id: Option<KeyId>,
    snapshot: PairingSnapshot,
    agreement_seq_floor: Option<SeqFloor>,
}

/// A JSON object under construction: members are written sorted by name (RFC 8785).
#[derive(Default)]
struct Members(Vec<(&'static str, String)>);

impl Members {
    fn put(&mut self, name: &'static str, rendered: String) {
        self.0.push((name, rendered));
    }

    fn string(&mut self, name: &'static str, value: &str) -> Result<(), String> {
        self.put(name, quoted(value)?);
        Ok(())
    }

    fn finish(mut self) -> String {
        self.0.sort_by_key(|(name, _)| *name);
        let mut out = String::from("{");
        for (i, (name, value)) in self.0.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(out, "\"{name}\":{value}");
        }
        out.push('}');
        out
    }
}

/// `value` as a JSON string, escaped as RFC 8785 requires.
fn quoted(value: &str) -> Result<String, String> {
    if !is_ijson_string(value) {
        return Err("a value holds a Unicode noncharacter, which I-JSON forbids".into());
    }
    canonical(&Json::String(value.to_owned())).map_err(|e| e.to_string())
}

fn array(items: Vec<String>) -> String {
    format!("[{}]", items.join(","))
}

fn encode_peer_side(side: &PeerSide) -> Result<String, String> {
    let mut o = Members::default();
    o.string("key_id", side.key_id.as_str())?;
    if let Some(session) = &side.session_id {
        o.string("session_id", session.as_str())?;
    }
    Ok(o.finish())
}

fn encode_local_side(side: &LocalSide) -> Result<String, String> {
    let mut o = Members::default();
    match side {
        LocalSide::Session(id) => {
            o.string("kind", "session")?;
            o.string("session_id", id.as_str())?;
        }
        LocalSide::Scope(scope) => {
            o.string("kind", "scope")?;
            o.string("scope", scope.as_str())?;
        }
        LocalSide::Device => o.string("kind", "device")?,
    }
    Ok(o.finish())
}

fn encode_grant(grant: &Grant) -> Result<String, String> {
    let mut o = Members::default();
    match grant {
        Grant::Inbound { writer, target } => {
            o.string("direction", "inbound")?;
            o.put("writer", encode_peer_side(writer)?);
            o.put("target", encode_local_side(target)?);
        }
        Grant::Outbound { writer, target } => {
            o.string("direction", "outbound")?;
            o.put("writer", encode_local_side(writer)?);
            o.put("target", encode_peer_side(target)?);
        }
    }
    Ok(o.finish())
}

fn encode_confirmed(confirmed: &ConfirmedValues) -> Result<String, String> {
    let mut o = Members::default();
    o.string("agreement_key", &confirmed.agreement_key().to_base64url())?;
    o.string("issued_at", confirmed.issued_at().as_str())?;
    o.put("seq", confirmed.seq().to_string());
    Ok(o.finish())
}

fn encode_record(record: &PairingRecord) -> Result<String, String> {
    let mut o = Members::default();
    o.string("key_id", record.key_id().as_str())?;
    o.string("paired_at", record.paired_at().as_str())?;
    o.string("principal", record.principal().as_str())?;
    o.string("public_key", &record.public_key().to_base64url())?;
    if let Some(confirmed) = record.confirmed() {
        o.put("confirmed", encode_confirmed(confirmed)?);
    }
    Ok(o.finish())
}

fn encode_statement(held: &HeldStatement) -> Result<String, String> {
    let mut o = Members::default();
    o.string("agreement_key", &held.agreement_key().to_base64url())?;
    o.string("issued_at", held.issued_at().as_str())?;
    o.string("key_id", held.key_id().as_str())?;
    o.put("seq", held.seq().to_string());
    Ok(o.finish())
}

/// The file's text for `file`. Arrays are sorted as the module documentation says.
fn encode(file: &TrustFile) -> Result<String, String> {
    let snapshot = &file.snapshot;
    let mut root = Members::default();
    if let Some(floor) = &file.agreement_seq_floor {
        let mut o = Members::default();
        o.string("key_id", floor.key_id.as_str())?;
        o.put("seq", floor.seq.to_string());
        root.put("agreement_seq_floor", o.finish());
    }
    if let Some(id) = &file.device_key_id {
        root.string("device_key_id", id.as_str())?;
    }
    root.put(VERSION_MEMBER, FORMAT_VERSION.to_string());

    let mut paired: Vec<&PairingRecord> = snapshot.paired.iter().collect();
    paired.sort_by_key(|r| r.key_id());
    root.put(
        "paired",
        array(
            paired
                .into_iter()
                .map(encode_record)
                .collect::<Result<_, _>>()?,
        ),
    );
    root.put(
        "grants",
        array(
            snapshot
                .grants
                .iter()
                .map(encode_grant)
                .collect::<Result<_, _>>()?,
        ),
    );
    let mut relay: Vec<&SessionId> = snapshot.relay_enabled.iter().collect();
    relay.sort();
    relay.dedup();
    root.put(
        "relay_enabled",
        array(
            relay
                .into_iter()
                .map(|s| quoted(s.as_str()))
                .collect::<Result<_, _>>()?,
        ),
    );
    let mut statements: Vec<&HeldStatement> = snapshot.agreement_statements.iter().collect();
    statements.sort_by(|a, b| a.key_id().cmp(b.key_id()));
    root.put(
        "statements",
        array(
            statements
                .into_iter()
                .map(encode_statement)
                .collect::<Result<_, _>>()?,
        ),
    );
    Ok(root.finish())
}

/// Checks that `object` has every member of `required` and none outside `required` and
/// `optional`.
fn exact(
    object: &JsonObject,
    required: &[&str],
    optional: &[&str],
    what: &str,
) -> Result<(), String> {
    for name in object.names() {
        if !required.contains(&name) && !optional.contains(&name) {
            let shown: String = name.chars().take(40).collect();
            return Err(format!("{what}: unknown member {shown:?}"));
        }
    }
    for name in required {
        if !object.contains(name) {
            return Err(format!("{what}: missing member {name:?}"));
        }
    }
    Ok(())
}

fn text<'a>(object: &'a JsonObject, name: &str, what: &str) -> Result<&'a str, String> {
    object
        .get(name)
        .and_then(Json::as_str)
        .ok_or_else(|| format!("{what}: {name:?} is not a string"))
}

fn object<'a>(object: &'a JsonObject, name: &str, what: &str) -> Result<&'a JsonObject, String> {
    object
        .get(name)
        .and_then(Json::as_object)
        .ok_or_else(|| format!("{what}: {name:?} is not an object"))
}

fn list<'a>(object: &'a JsonObject, name: &str, what: &str) -> Result<&'a [Json], String> {
    object
        .get(name)
        .and_then(Json::as_array)
        .ok_or_else(|| format!("{what}: {name:?} is not an array"))
}

fn element<'a>(value: &'a Json, what: &str) -> Result<&'a JsonObject, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{what}: an element is not an object"))
}

fn key_id(object: &JsonObject, name: &str, what: &str) -> Result<KeyId, String> {
    KeyId::parse(text(object, name, what)?)
        .ok_or_else(|| format!("{what}: {name:?} is not a key id"))
}

fn session_id(object: &JsonObject, name: &str, what: &str) -> Result<SessionId, String> {
    SessionId::parse(text(object, name, what)?)
        .ok_or_else(|| format!("{what}: {name:?} is not a session id"))
}

fn timestamp(object: &JsonObject, name: &str, what: &str) -> Result<Timestamp, String> {
    Timestamp::parse(text(object, name, what)?)
        .ok_or_else(|| format!("{what}: {name:?} is not a timestamp"))
}

fn agreement_key(object: &JsonObject, what: &str) -> Result<AgreementPublicKey, String> {
    AgreementPublicKey::from_base64url(text(object, "agreement_key", what)?)
        .ok_or_else(|| format!("{what}: \"agreement_key\" is not an acceptable key"))
}

fn seq(object: &JsonObject, what: &str) -> Result<u64, String> {
    object
        .get("seq")
        .and_then(Json::as_number)
        .and_then(|n| n.plain_integer_in(0, oac_core::sealing::MAX_STATEMENT_SEQ))
        .ok_or_else(|| format!("{what}: \"seq\" is not an integer in range"))
}

fn decode_peer_side(o: &JsonObject) -> Result<PeerSide, String> {
    const WHAT: &str = "a grant's peer side";
    exact(o, &["key_id"], &["session_id"], WHAT)?;
    let key = key_id(o, "key_id", WHAT)?;
    Ok(match o.contains("session_id") {
        true => PeerSide::session(key, session_id(o, "session_id", WHAT)?),
        false => PeerSide::device(key),
    })
}

fn decode_local_side(o: &JsonObject) -> Result<LocalSide, String> {
    const WHAT: &str = "a grant's local side";
    match text(o, "kind", WHAT)? {
        "session" => {
            exact(o, &["kind", "session_id"], &[], WHAT)?;
            Ok(LocalSide::Session(session_id(o, "session_id", WHAT)?))
        }
        "scope" => {
            exact(o, &["kind", "scope"], &[], WHAT)?;
            Ok(LocalSide::Scope(WorkingDirectoryScope::new(text(
                o, "scope", WHAT,
            )?)))
        }
        "device" => {
            exact(o, &["kind"], &[], WHAT)?;
            Ok(LocalSide::Device)
        }
        _ => Err(format!("{WHAT}: unknown kind")),
    }
}

fn decode_grant(o: &JsonObject) -> Result<Grant, String> {
    const WHAT: &str = "a grant";
    exact(o, &["direction", "target", "writer"], &[], WHAT)?;
    let writer = object(o, "writer", WHAT)?;
    let target = object(o, "target", WHAT)?;
    match text(o, "direction", WHAT)? {
        "inbound" => Ok(Grant::Inbound {
            writer: decode_peer_side(writer)?,
            target: decode_local_side(target)?,
        }),
        "outbound" => Ok(Grant::Outbound {
            writer: decode_local_side(writer)?,
            target: decode_peer_side(target)?,
        }),
        _ => Err(format!("{WHAT}: unknown direction")),
    }
}

fn decode_record(o: &JsonObject) -> Result<PairingRecord, String> {
    const WHAT: &str = "a paired record";
    exact(
        o,
        &["key_id", "paired_at", "principal", "public_key"],
        &["confirmed"],
        WHAT,
    )?;
    let public_key = PublicKey::from_base64url(text(o, "public_key", WHAT)?)
        .ok_or_else(|| format!("{WHAT}: \"public_key\" is not an acceptable key"))?;
    if public_key.key_id() != key_id(o, "key_id", WHAT)? {
        return Err(format!(
            "{WHAT}: \"key_id\" is not the SHA-256 of \"public_key\""
        ));
    }
    let principal = Token::parse(text(o, "principal", WHAT)?)
        .ok_or_else(|| format!("{WHAT}: \"principal\" is not an identifier token"))?;
    let confirmed = match o.get("confirmed") {
        None => None,
        Some(value) => {
            const CONFIRMED: &str = "confirmed values";
            let c = element(value, CONFIRMED)?;
            exact(c, &["agreement_key", "issued_at", "seq"], &[], CONFIRMED)?;
            Some(
                ConfirmedValues::from_store(
                    agreement_key(c, CONFIRMED)?,
                    seq(c, CONFIRMED)?,
                    timestamp(c, "issued_at", CONFIRMED)?,
                )
                .ok_or_else(|| format!("{CONFIRMED}: \"seq\" is out of range"))?,
            )
        }
    };
    Ok(
        PairingRecord::from_store(principal, public_key, timestamp(o, "paired_at", WHAT)?)
            .with_confirmed(confirmed),
    )
}

fn decode_statement(o: &JsonObject) -> Result<HeldStatement, String> {
    const WHAT: &str = "a held statement";
    exact(
        o,
        &["agreement_key", "issued_at", "key_id", "seq"],
        &[],
        WHAT,
    )?;
    HeldStatement::from_store(
        key_id(o, "key_id", WHAT)?,
        agreement_key(o, WHAT)?,
        seq(o, WHAT)?,
        timestamp(o, "issued_at", WHAT)?,
    )
    .ok_or_else(|| format!("{WHAT}: \"seq\" is out of range"))
}

fn decode_all<T>(
    items: &[Json],
    what: &str,
    one: impl Fn(&JsonObject) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    items.iter().map(|v| one(element(v, what)?)).collect()
}

/// Reads the file's text. The reasons name no key, scope or value.
fn decode(octets: &[u8]) -> Result<TrustFile, String> {
    const WHAT: &str = "the trust file";
    let parsed = json::parse(octets).map_err(|e| format!("{WHAT} is not a JSON document ({e})"))?;
    let root = parsed
        .as_object()
        .ok_or_else(|| format!("{WHAT} is not a JSON object"))?;
    // The version first: a later format is not read as this one with extras.
    match root.get(VERSION_MEMBER).and_then(Json::as_number) {
        Some(n) if n.plain_integer_in(0, u64::MAX) == Some(FORMAT_VERSION) => {}
        Some(_) => return Err(format!("{WHAT} has an unknown version")),
        None => return Err(format!("{WHAT} has no {VERSION_MEMBER:?} version member")),
    }
    exact(root, &ROOT_REQUIRED, &ROOT_MEMBERS, WHAT)?;
    let device_key_id = match root.contains("device_key_id") {
        true => Some(key_id(root, "device_key_id", WHAT)?),
        false => None,
    };
    let agreement_seq_floor = match root.get("agreement_seq_floor") {
        None => None,
        Some(value) => {
            const FLOOR: &str = "the agreement seq floor";
            let f = element(value, FLOOR)?;
            exact(f, &["key_id", "seq"], &[], FLOOR)?;
            Some(SeqFloor {
                key_id: key_id(f, "key_id", FLOOR)?,
                seq: seq(f, FLOOR)?,
            })
        }
    };
    let snapshot = PairingSnapshot {
        paired: decode_all(list(root, "paired", WHAT)?, "paired", decode_record)?,
        grants: decode_all(list(root, "grants", WHAT)?, "grants", decode_grant)?,
        relay_enabled: list(root, "relay_enabled", WHAT)?
            .iter()
            .map(|v| {
                v.as_str()
                    .and_then(SessionId::parse)
                    .ok_or_else(|| "relay_enabled: an element is not a session id".to_owned())
            })
            .collect::<Result<_, _>>()?,
        agreement_statements: decode_all(
            list(root, "statements", WHAT)?,
            "statements",
            decode_statement,
        )?,
    };
    Ok(TrustFile {
        device_key_id,
        snapshot,
        agreement_seq_floor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY: &str =
        r#"{"grants":[],"oac_trust_store":1,"paired":[],"relay_enabled":[],"statements":[]}"#;

    #[test]
    fn the_empty_store_is_canonical() {
        let text = encode(&TrustFile {
            device_key_id: None,
            snapshot: PairingSnapshot::default(),
            agreement_seq_floor: None,
        })
        .unwrap();
        assert_eq!(text, EMPTY);
        assert_eq!(
            canonical(&json::parse(text.as_bytes()).unwrap()).unwrap(),
            text
        );
    }

    #[test]
    fn the_floor_is_a_canonical_sorted_member_that_round_trips() {
        let key = oac_core::keys::DeviceKey::generate().public_key().key_id();
        let file = TrustFile {
            device_key_id: Some(key.clone()),
            snapshot: PairingSnapshot::default(),
            agreement_seq_floor: Some(SeqFloor {
                key_id: key.clone(),
                seq: 7,
            }),
        };
        let text = encode(&file).unwrap();
        assert_eq!(
            text,
            format!(
                r#"{{"agreement_seq_floor":{{"key_id":"{key}","seq":7}},"device_key_id":"{key}","grants":[],"oac_trust_store":1,"paired":[],"relay_enabled":[],"statements":[]}}"#
            )
        );
        assert_eq!(
            canonical(&json::parse(text.as_bytes()).unwrap()).unwrap(),
            text
        );
        let back = decode(text.as_bytes()).unwrap();
        assert_eq!(back.agreement_seq_floor, file.agreement_seq_floor);
        // Without a floor the member is absent, so an S1 file is read and written unchanged.
        assert_eq!(decode(EMPTY.as_bytes()).unwrap().agreement_seq_floor, None);
    }

    #[test]
    fn strictness_refuses_what_is_not_this_format() {
        assert!(decode(EMPTY.as_bytes()).is_ok());
        for (text, why) in [
            (EMPTY.replace(":1,", ":2,"), "an unknown version"),
            (
                EMPTY.replace(":1,", ":1.0,"),
                "a version spelled other than 1",
            ),
            (EMPTY.replace("{", "{\"extra\":1,"), "an unknown member"),
            (EMPTY.replace(",\"statements\":[]", ""), "a missing member"),
            (
                EMPTY.replace("\"grants\":[]", "\"grants\":{}"),
                "a wrong type",
            ),
            (
                EMPTY.replace("\"grants\":[]", "\"grants\":[1]"),
                "a grant that is not an object",
            ),
            (
                EMPTY.replace(
                    "\"relay_enabled\":[]",
                    "\"relay_enabled\":[\"not-a-session\"]",
                ),
                "a relay entry that is not a session id",
            ),
            (
                EMPTY.replace('{', "{\"oac_trust_store\":1,"),
                "a duplicate member",
            ),
            ("[]".to_owned(), "a root that is not an object"),
            (String::new(), "nothing"),
            (format!("{EMPTY} x"), "trailing text"),
        ] {
            assert!(decode(text.as_bytes()).is_err(), "{why}: {text}");
        }
        assert!(decode(&[0xff, 0xfe]).is_err(), "not UTF-8");
    }

    #[test]
    fn a_key_id_that_is_not_the_digest_of_the_key_is_refused() {
        // A real key, under the id of another.
        let key = oac_core::keys::DeviceKey::generate();
        let other = oac_core::keys::DeviceKey::generate();
        let record = |public: &PublicKey, id: &KeyId| {
            format!(
                r#"{{"grants":[],"oac_trust_store":1,"paired":[{{"key_id":"{id}","paired_at":"2026-11-04T12:00:00Z","principal":"peer","public_key":"{}"}}],"relay_enabled":[],"statements":[]}}"#,
                public.to_base64url()
            )
        };
        let good = record(key.public_key(), &key.public_key().key_id());
        assert!(decode(good.as_bytes()).is_ok(), "{good}");
        let swapped = record(key.public_key(), &other.public_key().key_id());
        assert!(decode(swapped.as_bytes()).is_err());
    }

    #[test]
    fn temp_names_are_recognised_exactly() {
        assert!(is_temp_name(&random_temp_name()));
        assert!(!is_temp_name("trust.json"));
        assert!(!is_temp_name("trust.json.tmp"));
        assert!(!is_temp_name("trust.json.zz.tmp"));
        assert!(!is_temp_name("other.0123456789abcdef0123456789abcdef.tmp"));
        assert!(!is_temp_name(
            "trust.json.0123456789abcdef0123456789abcde.tmp"
        ));
    }
}
