// SPDX-License-Identifier: Apache-2.0

//! The agreement `seq` floor (#437 S1c; `docs/planning/decisions/G-437-pairing.md` §7, T10).
//!
//! The sealed agreement-key file cannot tell an older valid copy of itself from the current
//! one: a restored backup or an undone rename brings back a lower `highest_seq`, and the device
//! would issue a `seq` its peers already hold, which they refuse ([SEC-SEL-014]). The floor is a
//! second record of the highest issued `seq`, kept in `trust.json` (a different file, so both
//! have to roll back together to defeat it) and written **before** the statement exists.
//!
//! [`FlooredAgreementStore`] wraps the agreement-key store the daemon opened and a
//! [`PairingStore`] that keeps the floor:
//!
//! - **Load.** The stored `highest_seq` is `max(file, floor)` for this device key's id. When
//!   the file is missing or holds a lower count, the state is the floor with no keys, which
//!   `AgreementKeys::load_or_generate` handles as "every key was erased, the count stays": it
//!   issues `floor + 1` under a new key ([SEC-SEL-040]: higher than any before), with no
//!   re-pairing. A floor that names another key id is not this key's and is ignored (a
//!   regenerated device key starts at `seq` 0, L7); the store makes that call.
//! - **Save.** The floor is raised to the state's `highest_seq` first, and the file is
//!   written only after that succeeded. A failed floor write fails the save, so no statement
//!   is issued ([SEC-SEL-043]). If the file write then fails, the floor is ahead of the file.
//!   The next start skips a `seq`, and, because the floor is above the file's count, it
//!   treats the file as rolled back: `load` gives the floor with **no keys**, so the file's
//!   current and replaced keys are dropped and frames sealed to them no longer open. That is
//!   the price of a failed write between the two files, and it is bounded: peers admit the
//!   new statement and seal to the new key.
//! - **Monotonic.** The floor is never lowered ([`PairingStore::record_agreement_seq_floor`]):
//!   a stale save, with a `highest_seq` below the floor, leaves it where it is. The one
//!   exception is a floor at the maximum `seq`, which the restart replaces:
//!   `AgreementKeys::restart_count` ([SEC-SEL-052]) saves a state at `seq` 0 and the floor
//!   follows, which is how a floor at the maximum is cleared.
//! - **One key id.** The floor holds one key id. Going back to an older device key finds its
//!   floor gone (a later key's start replaced it), and only that key's own file carries its
//!   count: the L7 residual.
//! - **At the maximum.** A floor at `MAX_STATEMENT_SEQ` makes `next_seq` fail with
//!   `SeqExhausted`, so the start is refused until the operator takes the restart. A same-user
//!   write to `trust.json` can set it: a denial of service inside the accepted same-user
//!   residual (T7, L6).
//!
//! It does not help when both files return together or are deleted together (T10 residual).

use std::sync::Arc;

use oac_core::ids::KeyId;
use oac_core::keys::KeyStoreError;
use oac_core::pairing::{PairingStore, PairingStoreError};
use oac_core::sealing::{AgreementKeyState, AgreementKeyStore};

/// An [`AgreementKeyStore`] whose count is also kept as a floor in a [`PairingStore`]. See the
/// module documentation.
pub struct FlooredAgreementStore {
    inner: Box<dyn AgreementKeyStore + Send>,
    floor: Arc<dyn PairingStore + Send + Sync>,
    key_id: KeyId,
}

impl std::fmt::Debug for FlooredAgreementStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlooredAgreementStore")
            .field("key_id", &self.key_id)
            .finish_non_exhaustive()
    }
}

impl FlooredAgreementStore {
    /// `inner`, with its count also kept as the floor of the device key `key_id` in `floor`.
    pub fn new(
        inner: Box<dyn AgreementKeyStore + Send>,
        floor: Arc<dyn PairingStore + Send + Sync>,
        key_id: KeyId,
    ) -> FlooredAgreementStore {
        FlooredAgreementStore {
            inner,
            floor,
            key_id,
        }
    }

    /// Makes sure the floor is at least `highest`, the count the daemon runs with. A store
    /// that held its count before the floor existed (an upgrade), or whose floor was lost, has
    /// no floor until the next statement is issued; this closes that window at start.
    ///
    /// # Errors
    ///
    /// [`PairingStoreError`] when the floor cannot be read or written.
    pub fn ensure_floor(&self, highest: u64) -> Result<(), PairingStoreError> {
        let current = self.floor.agreement_seq_floor(&self.key_id)?;
        if current.is_none_or(|f| f < highest) {
            self.floor
                .record_agreement_seq_floor(&self.key_id, highest)?;
        }
        Ok(())
    }
}

fn store_error(e: PairingStoreError) -> KeyStoreError {
    match e {
        PairingStoreError::Corrupt(m) => KeyStoreError::Corrupt(m),
        PairingStoreError::Failed(m) => KeyStoreError::Failed(m),
    }
}

impl AgreementKeyStore for FlooredAgreementStore {
    fn load(&self) -> Result<Option<AgreementKeyState>, KeyStoreError> {
        let floor = self
            .floor
            .agreement_seq_floor(&self.key_id)
            .map_err(store_error)?;
        let state = self.inner.load()?;
        // Keys with no count are a damaged file, not an older one: the floor does not hide it.
        if state
            .as_ref()
            .is_some_and(|s| s.highest_seq.is_none() && !s.keys.is_empty())
        {
            return Err(KeyStoreError::Corrupt(
                "the agreement-key state holds keys but no highest seq".into(),
            ));
        }
        Ok(match (state, floor) {
            (state, None) => state,
            // The file is missing: a floor without a file.
            (None, Some(floor)) => Some(empty_at(floor)),
            (Some(state), Some(floor)) => {
                if state.highest_seq.is_some_and(|h| h >= floor) {
                    Some(state)
                } else {
                    // The file is older than the floor: a rollback. Its keys are not kept; the
                    // count is the floor's.
                    Some(empty_at(floor))
                }
            }
        })
    }

    fn save(&self, state: &AgreementKeyState) -> Result<(), KeyStoreError> {
        // [SEC-SEL-043]: the count is durable before the statement exists. The floor first, so
        // that a failure of either write leaves a floor at or ahead of the file, never behind.
        // The store keeps it monotonic.
        if let Some(highest) = state.highest_seq {
            self.floor
                .record_agreement_seq_floor(&self.key_id, highest)
                .map_err(store_error)?;
        }
        self.inner.save(state)
    }
}

fn empty_at(floor: u64) -> AgreementKeyState {
    AgreementKeyState {
        keys: Vec::new(),
        highest_seq: Some(floor),
    }
}
