// SPDX-License-Identifier: Apache-2.0

//! The trusted key set (`spec/security.md` §5.3): the device keys an implementation accepts
//! signatures from, each with the principal it belongs to.
//!
//! A [`TrustedKey`] has no public constructor. One comes from this device's own key
//! ([`crate::keys::DeviceIdentity`], [SEC-KEY-031]) or from
//! [`TrustedKeySet::add_paired_key`], and it is the only source of a
//! [`SecurityPrincipal`] ([IFC-TYP-070]).
//!
//! Pairing, the operator-confirmed exchange that leads to [`TrustedKeySet::add_paired_key`]
//! ([SEC-KEY-032]), is [`crate::pairing`] (#54, F5). Removing a key must remove every grant
//! and binding that names it in the same step ([SEC-KEY-035]), so the set's own removal is
//! crate-private: the one public way to remove a key is
//! [`crate::authorization::AuthorizationEngine::remove_key`], which holds the grants and the
//! binding table beside this set.

use crate::envelope::SecurityPrincipal;
use crate::ids::{KeyId, Token};
use crate::keys::{DeviceIdentity, PublicKey};
use std::collections::BTreeMap;
use std::fmt;

/// One entry of the trusted key set: this public key belongs to a device of this principal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedKey {
    principal: Token,
    key_id: KeyId,
    public_key: PublicKey,
}

impl TrustedKey {
    pub(crate) fn new(principal: Token, public_key: PublicKey) -> TrustedKey {
        TrustedKey {
            principal,
            key_id: public_key.key_id(),
            public_key,
        }
    }

    /// The principal label ([SEC-KEY-020]).
    pub fn principal(&self) -> &Token {
        &self.principal
    }

    /// The key id ([SEC-KEY-010]).
    pub fn key_id(&self) -> &KeyId {
        &self.key_id
    }

    /// The public key, admitted under [SEC-KEY-034].
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// The `SecurityPrincipal` of this entry ([IFC-TYP-070]).
    pub fn security_principal(&self) -> SecurityPrincipal {
        SecurityPrincipal::new(self.principal.clone(), self.key_id.clone())
    }
}

/// Why a key was not added to the trusted key set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddKeyError {
    /// The key is already trusted, under the principal given here. A key belongs to one
    /// device of one principal, so it has one entry.
    AlreadyTrusted(Token),
}

impl fmt::Display for AddKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AddKeyError::AlreadyTrusted(p) => write!(f, "key already trusted under {p}"),
        }
    }
}

impl std::error::Error for AddKeyError {}

/// The trusted key set. It always holds this device's own key ([SEC-KEY-031]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedKeySet {
    entries: BTreeMap<KeyId, TrustedKey>,
}

impl TrustedKeySet {
    /// The set that holds `own`'s key only ([SEC-KEY-031]). Default deny: no other device's
    /// key is trusted until pairing adds it.
    pub fn new(own: &DeviceIdentity) -> TrustedKeySet {
        let entry = own.trusted_entry().clone();
        TrustedKeySet {
            entries: BTreeMap::from([(entry.key_id().clone(), entry)]),
        }
    }

    /// Adds another device's key under `principal`. Call it only once an operator has
    /// confirmed the addition after comparing the key id, or a value derived from it,
    /// through a channel other than the transport that delivered the key ([SEC-KEY-032]);
    /// never because a signature by the key arrived or a transport authenticated a
    /// connection ([SEC-KEY-033]). The key passed [SEC-KEY-034] when its [`PublicKey`] was
    /// built.
    ///
    /// # Errors
    ///
    /// [`AddKeyError::AlreadyTrusted`] when the key is already in the set.
    pub fn add_paired_key(
        &mut self,
        principal: Token,
        public_key: PublicKey,
    ) -> Result<&TrustedKey, AddKeyError> {
        let entry = TrustedKey::new(principal, public_key);
        if let Some(existing) = self.entries.get(entry.key_id()) {
            return Err(AddKeyError::AlreadyTrusted(existing.principal.clone()));
        }
        let id = entry.key_id().clone();
        Ok(self.entries.entry(id).or_insert(entry))
    }

    /// Step 1 of the security stage, key resolution (`spec/security.md` Table 7.1): the
    /// entry named by exactly this principal and this key id ([SEC-KEY-030]), compared as
    /// exact strings ([SEC-KEY-011]). `None` when either value is not of its form
    /// ([SEC-KEY-020], §5.2), when no entry has that key id, or when the entry's principal
    /// differs.
    pub fn resolve(&self, principal: &str, key_id: &str) -> Option<&TrustedKey> {
        let principal = Token::parse(principal)?;
        let key_id = KeyId::parse(key_id)?;
        self.entries
            .get(&key_id)
            .filter(|e| e.principal == principal)
    }

    /// Removes the entry for `key_id`. Crate-private: a key leaves the set only together with
    /// every grant and binding that names it ([SEC-KEY-035]), through
    /// [`crate::authorization::AuthorizationEngine::remove_key`], which also refuses this
    /// device's own key ([SEC-KEY-031]).
    pub(crate) fn remove(&mut self, key_id: &KeyId) -> Option<TrustedKey> {
        self.entries.remove(key_id)
    }

    /// The entry for `key_id`, under whatever principal.
    pub fn get(&self, key_id: &KeyId) -> Option<&TrustedKey> {
        self.entries.get(key_id)
    }

    /// Every entry, in key-id order.
    pub fn iter(&self) -> impl Iterator<Item = &TrustedKey> {
        self.entries.values()
    }

    /// The number of entries, this device's own included.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Always false: the set holds this device's own key.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::DeviceKey;

    fn token(s: &str) -> Token {
        Token::parse(s).unwrap()
    }

    #[test]
    fn own_key_is_trusted_and_others_are_not() {
        let me = DeviceIdentity::new(DeviceKey::generate(), token("principal-a"));
        let other = DeviceKey::generate();
        let mut set = TrustedKeySet::new(&me);
        assert_eq!(set.len(), 1);
        assert!(!set.is_empty());
        assert_eq!(
            set.resolve("principal-a", me.key_id().as_str()),
            Some(me.trusted_entry())
        );
        assert!(
            set.resolve("principal-b", other.key_id().as_str())
                .is_none()
        );
        set.add_paired_key(token("principal-b"), *other.public_key())
            .unwrap();
        assert!(
            set.resolve("principal-b", other.key_id().as_str())
                .is_some()
        );
        // [SEC-KEY-030]: the key id under another principal names nothing.
        assert!(
            set.resolve("principal-a", other.key_id().as_str())
                .is_none()
        );
        // [SEC-KEY-011]: no case folding.
        assert!(
            set.resolve("principal-b", &other.key_id().as_str().to_uppercase())
                .is_none()
        );
        assert!(
            set.resolve("Principal-b", other.key_id().as_str())
                .is_none()
        );
        // [SEC-KEY-020]: a principal that is not a token names nothing.
        assert!(
            set.resolve("principal b", other.key_id().as_str())
                .is_none()
        );
        assert_eq!(
            set.add_paired_key(token("principal-c"), *other.public_key()),
            Err(AddKeyError::AlreadyTrusted(token("principal-b")))
        );
        assert_eq!(
            set.get(&other.key_id()).unwrap().principal(),
            &token("principal-b")
        );
        assert_eq!(set.iter().count(), 2);
    }
}
