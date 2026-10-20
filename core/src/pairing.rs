// SPDX-License-Identifier: Apache-2.0

//! Pairing (`spec/security.md` §5.3, [SEC-KEY-032]) and the pairing store.
//!
//! Pairing is the operator-confirmed exchange after which another device's key enters the
//! trusted key set. Trust is never established on first use: a key is added only once an
//! operator confirmed it after comparing its key id, or a value derived from it, through a
//! channel other than the transport that delivered it ([SEC-KEY-032]), and never because a
//! signature by it arrived or a transport authenticated a connection ([SEC-KEY-033]). A
//! [`PairedPeer`], the only input of [`crate::authorization::AuthorizationEngine::pair`],
//! comes from one of the two flows below, each ending in an operator's confirmation.
//!
//! Pairing makes a key trusted. It grants nothing: a grant decides what the key may reach
//! ([SEC-AUZ-001]; `docs/planning/decisions/C5-envelope-auth.md` §11).
//!
//! # Same device: no pairing
//!
//! Two harnesses on one device share its one device key, which the trusted key set always
//! holds ([SEC-KEY-031]). There is nothing to pair and nothing to configure
//! (`docs/planning/decisions/C5-envelope-auth.md` §10(a)). Their sessions still need a
//! grant to reach each other ([SEC-AUZ-007]).
//!
//! # Two devices: a six-digit code
//!
//! The reference implementation pairs two devices with a six-digit code derived from both
//! devices' keys, valid for 120 seconds, with five attempts
//! (`spec/security.md` §5.3, reference implementation note;
//! `docs/planning/decisions/C5-envelope-auth.md` §10(b)). `spec/security.md` fixes what
//! pairing establishes, not the wire form of the exchange, so the messages here are plain
//! values that the caller carries over any channel; an attacker on that channel is assumed.
//!
//! A code computed from the two public keys alone would not stop an attacker on that
//! channel: a six-digit code has about 2^20 values, so an attacker can generate key pairs
//! offline until its substituted keys give both devices the same code. The exchange is
//! therefore a commit-then-reveal one, in the manner of a numeric-comparison pairing:
//!
//! 1. The initiator draws a 32-octet nonce `Ni` and sends its principal, its public key and
//!    a commitment `SHA-256("oac-pairing-commit-v2" 0x00 || P(principal_i) || pk_i || S_i ||
//!    Ni)` ([`PairingOffer`]). `S_i` is the initiator's agreement statement value, below.
//! 2. The responder draws its own nonce `Nr` and sends its principal, its public key and
//!    `Nr` ([`PairingResponse`]).
//! 3. The initiator reveals `Ni` ([`PairingReveal`]); the responder checks it against the
//!    commitment.
//! 4. Each side computes the code from both principals, both public keys and both nonces:
//!    the first eight octets of
//!    `SHA-256("oac-pairing-code-v2" 0x00 || P(principal_i) || pk_i || S_i || P(principal_r) ||
//!    pk_r || S_r || Ni || Nr)`, as a big-endian integer, modulo 10^6, written as six digits.
//!    `P(x)` is the octet length of `x` followed by its octets. `S_i` and `S_r` are the
//!    statement values of the two devices, as each side sees them (below).
//!
//! The initiator is bound to `Ni` and to its statement before it sees `Nr`, and the
//! responder reveals `Nr` and its statement before it sees `Ni`, so neither side, nor anyone
//! between them, can choose keys, statements or nonces to reach a given code: a substituted key matches the other side's code with probability
//! 10^-6 per attempt. The operator then either types the code one device shows into the
//! other ([`PairingSession::confirm_entered`]: at most five wrong entries, then the session
//! is aborted), or compares the codes both devices show and confirms they match
//! ([`PairingSession::confirm_match`]). Either way, within 120 seconds of the start.
//!
//! **One offer per operator-started pairing.** The exchange stops grinding only if each
//! exchange is one the operator sees. Otherwise an attacker could finish the initiator's
//! side first, since the initiator shows its code once it has a response. It could then
//! play initiator toward the responder over and over, never revealing, until the
//! responder's code matched. The five-attempt limit counts code entries within one session,
//! not exchanges, so it does not stop this. Two rules apply:
//!
//! - a responder answers one offer per operator-started pairing;
//! - an exchange abandoned before the reveal ends that pairing, and the operator is shown
//!   that it ended.
//!
//! [`ResponderPairing`] enforces both on the responding side: a second offer, an abandoned
//! exchange or a reveal that never comes ends the pairing with a [`PairingEnd`] to show. The
//! initiator sends one offer per [`PairingInitiator::start`]. The calling code (the `oac`
//! pairing verb) must start each pairing only on an operator's request, and show every
//! [`PairingEnd`]. With both rules, a substituted key matches with probability 10^-6 per
//! operator-visible exchange.
//!
//! # Two devices: a compared key id
//!
//! The operator may instead compare the peer's full key id, read from the peer device's
//! own display or from a file the operator carried across, and confirm it
//! ([`PairedPeer::by_key_id_comparison`]): the key id is the device's fingerprint
//! (`spec/security.md` §5.2), so this is the file-exchange form of [SEC-KEY-032].
//!
//! # Agreement statements
//!
//! When either device uses a sealing transport, each gives the other its current agreement
//! statement ([SEC-SEL-050]; `spec/security.md` §14.3, "Pairing"): beside its public key in
//! the [`PairingOffer`] and the [`PairingResponse`] (a device that has one starts its side
//! with [`PairingInitiator::start_with_statement`] or
//! [`ResponderPairing::start_with_statement`]), and beside the key file in the key-id
//! comparison flow ([`PairedPeer::with_agreement_statement`]).
//!
//! The operator's comparison covers both devices' statements, each device's own as well as
//! the peer's ([SEC-SEL-044]). Each side derives a [`StatementValue`] for each of the two:
//! `none` when the device gave no statement, the `agreement_key`, `seq` and `issued_at` when
//! it gave one the other side accepts, and `refused` when it gave one that this side
//! refuses under [SEC-SEL-010] to [SEC-SEL-013] and [SEC-SEL-042], checked before the code
//! is shown ([SEC-SEL-051]). A statement that was removed, replaced or refused on the way
//! gives the two sides different values, so their codes differ. The initiator's value enters
//! the commitment, before the code is derived, so an attacker that holds several old
//! statements cannot choose the one that makes the code match. The values of a statement
//! this side accepted are the pairing's confirmed values, which
//! [`crate::authorization::AuthorizationEngine::pair`] keeps with the key
//! ([`PairingRecord::confirmed`]): they are the floor of every later statement
//! ([SEC-SEL-045], [SEC-SEL-048], [SEC-SEL-049]). A device that uses no sealing transport
//! takes part with `none` for its own value.
//!
//! # The pairing store
//!
//! [`PairingStore`] keeps the paired keys, the grants and the relay settings across
//! restarts (`docs/planning/decisions/C5-envelope-auth.md` §10, "Pairing store's
//! location"). [`MemoryPairingStore`] is the in-process double the CI-default tests use.
//! A store on disk, if any, belongs to the `oac` binary, opt-in, as the device key's
//! stores do.

use crate::authorization::{Grant, OperatorConfirmed};
use crate::ids::{KeyId, SessionId, Timestamp, Token};
use crate::keys::{DeviceIdentity, PublicKey, random_octets};
use crate::sealing::{AgreementStatement, ConfirmedValues, HeldStatement, check_statement};
use crate::trust::TrustedKeySet;
use sha2::{Digest, Sha256};
use std::fmt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use subtle::ConstantTimeEq;

/// How long a pairing session stays open, from its start: 120 seconds
/// (`docs/planning/decisions/C5-envelope-auth.md` §10(b)).
pub const PAIRING_VALIDITY_MS: u64 = 120_000;

/// Wrong code entries after which a pairing session is aborted: 5
/// (`docs/planning/decisions/C5-envelope-auth.md` §10(b)).
pub const PAIRING_MAX_ATTEMPTS: u32 = 5;

const COMMIT_DOMAIN: &[u8] = b"oac-pairing-commit-v2";
const CODE_DOMAIN: &[u8] = b"oac-pairing-code-v2";

/// A pairing nonce: 32 octets from the operating system's random number generator.
pub type PairingNonce = [u8; 32];

fn fresh_nonce() -> PairingNonce {
    *random_octets::<32>()
}

fn deadline(start: &Timestamp) -> i128 {
    start.unix_nanos() + i128::from(PAIRING_VALIDITY_MS) * 1_000_000
}

fn put_principal(h: &mut Sha256, p: &Token) {
    // An identifier token is at most 128 octets ([SC-ENV-010]), so one length octet holds it.
    let len = u8::try_from(p.as_str().len()).expect("a token is at most 128 octets");
    h.update([len]);
    h.update(p.as_str().as_bytes());
}

/// What one device's agreement statement counts as in the operator's comparison
/// ([SEC-SEL-044], [SEC-SEL-051]): three values of their own, so that a missing, a refused
/// and a given statement can never be taken for one another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatementValue {
    /// The device gave no statement.
    None,
    /// The device gave a statement this side accepts; its `agreement_key`, `seq` and
    /// `issued_at`.
    Given(ConfirmedValues),
    /// The device gave a statement this side refuses ([SEC-SEL-010] to [SEC-SEL-013],
    /// [SEC-SEL-042]). Shown as a value of its own, so that the operator sees a mismatch.
    Refused,
}

impl StatementValue {
    /// The value of a statement as its holder gave it, unchecked: `none` without one. This
    /// is the value of this device's own statement, and the value a commitment covers.
    pub fn of_own(statement: Option<&AgreementStatement>) -> StatementValue {
        match statement {
            None => StatementValue::None,
            Some(st) => st
                .claimed_values()
                .map_or(StatementValue::Refused, StatementValue::Given),
        }
    }

    /// The values the statement is accepted with, if it is.
    pub fn given(&self) -> Option<&ConfirmedValues> {
        match self {
            StatementValue::Given(v) => Some(v),
            _ => None,
        }
    }
}

impl fmt::Display for StatementValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StatementValue::None => f.write_str("none: no agreement statement given"),
            StatementValue::Given(v) => v.fmt(f),
            StatementValue::Refused => f.write_str("refused: the agreement statement is refused"),
        }
    }
}

/// The value of the statement a peer gave, checked under the rules of an admitted statement
/// ([SEC-SEL-010] to [SEC-SEL-013], [SEC-SEL-042]) with only `own` and the peer's key
/// trusted, at `now`: the check [SEC-SEL-051] asks for before the comparison is shown. Use
/// it also in the key-id comparison flow, to show the peer's value beside its key id.
pub fn judge_statement(
    own: &DeviceIdentity,
    peer_principal: &Token,
    peer_key: &PublicKey,
    statement: Option<&AgreementStatement>,
    now: &Timestamp,
) -> StatementValue {
    judge_with(
        &TrustedKeySet::new(own),
        peer_principal,
        peer_key,
        statement,
        now,
    )
}

fn judge_with(
    own_set: &TrustedKeySet,
    peer_principal: &Token,
    peer_key: &PublicKey,
    statement: Option<&AgreementStatement>,
    now: &Timestamp,
) -> StatementValue {
    let Some(st) = statement else {
        return StatementValue::None;
    };
    let mut trusted = own_set.clone();
    if trusted
        .add_paired_key(peer_principal.clone(), *peer_key)
        .is_err()
    {
        return StatementValue::Refused;
    }
    match check_statement(st, &trusted, |_| None, now) {
        Ok(h) if h.key_id() == &peer_key.key_id() => {
            StatementValue::Given(ConfirmedValues::of_held(&h))
        }
        _ => StatementValue::Refused,
    }
}

fn put_value(h: &mut Sha256, v: &StatementValue) {
    match v {
        StatementValue::None => h.update([0]),
        StatementValue::Given(c) => {
            h.update([1]);
            // A key is 43 octets of base64url and a timestamp at most 30, so one length
            // octet holds each.
            let key = c.agreement_key().to_base64url();
            h.update([u8::try_from(key.len()).expect("43 octets")]);
            h.update(key.as_bytes());
            h.update(c.seq().to_be_bytes());
            let at = c.issued_at().as_str();
            h.update([u8::try_from(at.len()).expect("a timestamp is at most 30 octets")]);
            h.update(at.as_bytes());
        }
        StatementValue::Refused => h.update([2]),
    }
}

fn commitment(
    principal: &Token,
    pk: &PublicKey,
    statement: &StatementValue,
    nonce: &PairingNonce,
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(COMMIT_DOMAIN);
    h.update([0]);
    put_principal(&mut h, principal);
    h.update(pk.to_octets());
    put_value(&mut h, statement);
    h.update(nonce);
    h.finalize().into()
}

type Party<'a> = (
    &'a Token,
    &'a PublicKey,
    &'a StatementValue,
    &'a PairingNonce,
);

fn code(initiator: Party<'_>, responder: Party<'_>) -> String {
    let mut h = Sha256::new();
    h.update(CODE_DOMAIN);
    h.update([0]);
    put_principal(&mut h, initiator.0);
    h.update(initiator.1.to_octets());
    put_value(&mut h, initiator.2);
    put_principal(&mut h, responder.0);
    h.update(responder.1.to_octets());
    put_value(&mut h, responder.2);
    h.update(initiator.3);
    h.update(responder.3);
    let digest: [u8; 32] = h.finalize().into();
    let mut first = [0u8; 8];
    first.copy_from_slice(&digest[..8]);
    format!("{:06}", u64::from_be_bytes(first) % 1_000_000)
}

/// Step 1, initiator to responder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingOffer {
    /// The initiator's principal label.
    pub principal: Token,
    /// The initiator's public key.
    pub public_key: PublicKey,
    /// The commitment to the initiator's nonce.
    pub commitment: [u8; 32],
    /// The initiator's current agreement statement, when it gives one ([SEC-SEL-050]). Its
    /// value is committed to by `commitment`, so a statement removed or replaced on the way
    /// ends the exchange at the reveal.
    pub agreement_statement: Option<AgreementStatement>,
}

/// Step 2, responder to initiator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingResponse {
    /// The responder's principal label.
    pub principal: Token,
    /// The responder's public key.
    pub public_key: PublicKey,
    /// The responder's nonce.
    pub nonce: PairingNonce,
    /// The responder's current agreement statement, when it gives one ([SEC-SEL-050]). Its
    /// value enters the code, which the initiator can derive only after it reveals its
    /// nonce, so it cannot be chosen to reach a given code.
    pub agreement_statement: Option<AgreementStatement>,
}

/// Step 3, initiator to responder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingReveal {
    /// The initiator's nonce.
    pub nonce: PairingNonce,
}

/// Why a pairing exchange failed. Every failure ends the exchange; a new one starts with
/// new nonces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairingError {
    /// The peer presented this device's own key.
    OwnKey,
    /// The revealed nonce does not match the commitment.
    CommitmentMismatch,
    /// 120 seconds have passed since the exchange started.
    Expired,
    /// The operator-started pairing has ended, for this reason; the operator starts a new
    /// one ([`ResponderPairing`]).
    Ended(PairingEnd),
}

impl fmt::Display for PairingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PairingError::OwnKey => f.write_str("the peer presented this device's own key"),
            PairingError::CommitmentMismatch => f.write_str("the revealed nonce does not match"),
            PairingError::Expired => f.write_str("the pairing exchange expired"),
            PairingError::Ended(end) => write!(f, "the pairing ended: {end}"),
        }
    }
}

/// Why an operator-started pairing on the responding device ended without a code. Each is
/// shown to the operator, who starts a new pairing to try again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairingEnd {
    /// A second offer arrived after the first was answered. A responder answers one offer
    /// per operator-started pairing; a second one may be an attacker restarting the
    /// exchange to try another code.
    SecondOffer,
    /// The exchange was abandoned before the reveal: the caller reported it, or the
    /// 120-second window ran out while waiting for the reveal.
    Abandoned,
    /// No offer arrived within the 120-second window.
    Expired,
    /// The offer carried this device's own key.
    OwnKey,
    /// The revealed nonce did not match the commitment.
    CommitmentMismatch,
}

impl fmt::Display for PairingEnd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PairingEnd::SecondOffer => "a second offer arrived, so the pairing is ended",
            PairingEnd::Abandoned => "the exchange was abandoned before the code was shown",
            PairingEnd::Expired => "no offer arrived in time",
            PairingEnd::OwnKey => "the offer carried this device's own key",
            PairingEnd::CommitmentMismatch => "the revealed nonce does not match",
        })
    }
}

impl std::error::Error for PairingError {}

/// The initiator's side, between steps 1 and 3.
#[derive(Debug)]
pub struct PairingInitiator {
    principal: Token,
    public_key: PublicKey,
    nonce: PairingNonce,
    started: Timestamp,
    own_set: TrustedKeySet,
    own_value: StatementValue,
}

impl PairingInitiator {
    /// Starts an exchange at `now` with a fresh nonce. Call it once per operator-started
    /// pairing: the initiator sends one offer, and an exchange that does not reach its code
    /// ends that pairing; the operator starts a new one to try again.
    pub fn start(own: &DeviceIdentity, now: Timestamp) -> (PairingInitiator, PairingOffer) {
        PairingInitiator::start_inner(own, None, fresh_nonce(), now)
    }

    /// [`PairingInitiator::start`] for a device that uses a sealing transport: the offer
    /// carries its current agreement statement `statement` ([SEC-SEL-050]), and the
    /// commitment covers its value.
    pub fn start_with_statement(
        own: &DeviceIdentity,
        statement: AgreementStatement,
        now: Timestamp,
    ) -> (PairingInitiator, PairingOffer) {
        PairingInitiator::start_inner(own, Some(statement), fresh_nonce(), now)
    }

    /// [`PairingInitiator::start`] with a given nonce, for tests only: a fixed or reused
    /// nonce removes the commitment's hiding property.
    #[cfg(test)]
    pub(crate) fn start_with_nonce(
        own: &DeviceIdentity,
        nonce: PairingNonce,
        now: Timestamp,
    ) -> (PairingInitiator, PairingOffer) {
        PairingInitiator::start_inner(own, None, nonce, now)
    }

    /// [`PairingInitiator::start_with_statement`] with a given nonce, for tests only.
    #[cfg(test)]
    pub(crate) fn start_with_statement_and_nonce(
        own: &DeviceIdentity,
        statement: AgreementStatement,
        nonce: PairingNonce,
        now: Timestamp,
    ) -> (PairingInitiator, PairingOffer) {
        PairingInitiator::start_inner(own, Some(statement), nonce, now)
    }

    fn start_inner(
        own: &DeviceIdentity,
        statement: Option<AgreementStatement>,
        nonce: PairingNonce,
        now: Timestamp,
    ) -> (PairingInitiator, PairingOffer) {
        let own_value = StatementValue::of_own(statement.as_ref());
        let offer = PairingOffer {
            principal: own.principal().clone(),
            public_key: *own.public_key(),
            commitment: commitment(own.principal(), own.public_key(), &own_value, &nonce),
            agreement_statement: statement,
        };
        (
            PairingInitiator {
                principal: own.principal().clone(),
                public_key: *own.public_key(),
                nonce,
                started: now,
                own_set: TrustedKeySet::new(own),
                own_value,
            },
            offer,
        )
    }

    /// Step 3: takes the response and reveals the nonce. The session's code is then known.
    ///
    /// # Errors
    ///
    /// [`PairingError::OwnKey`] or [`PairingError::Expired`].
    pub fn receive(
        self,
        response: PairingResponse,
        now: &Timestamp,
    ) -> Result<(PairingSession, PairingReveal), PairingError> {
        if response.public_key == self.public_key {
            return Err(PairingError::OwnKey);
        }
        if now.unix_nanos() >= deadline(&self.started) {
            return Err(PairingError::Expired);
        }
        // [SEC-SEL-051]: the statement is checked before the comparison exists.
        let peer_value = judge_with(
            &self.own_set,
            &response.principal,
            &response.public_key,
            response.agreement_statement.as_ref(),
            now,
        );
        let code = code(
            (
                &self.principal,
                &self.public_key,
                &self.own_value,
                &self.nonce,
            ),
            (
                &response.principal,
                &response.public_key,
                &peer_value,
                &response.nonce,
            ),
        );
        Ok((
            PairingSession::new(
                response.principal,
                response.public_key,
                code,
                self.started,
                self.own_value,
                peer_value,
                response.agreement_statement,
            ),
            PairingReveal { nonce: self.nonce },
        ))
    }
}

/// The responder's side of one exchange, between steps 2 and 3. Crate-private: a caller
/// answers offers only through [`ResponderPairing`], which answers one per pairing.
#[derive(Debug)]
pub(crate) struct PairingResponder {
    principal: Token,
    public_key: PublicKey,
    nonce: PairingNonce,
    offer: PairingOffer,
    started: Timestamp,
    own_set: TrustedKeySet,
    own_value: StatementValue,
}

impl PairingResponder {
    /// Step 2: answers `offer` with a fresh nonce, giving no statement. `now` starts the
    /// 120-second window.
    #[cfg(test)]
    pub(crate) fn respond(
        own: &DeviceIdentity,
        offer: PairingOffer,
        now: Timestamp,
    ) -> Result<(PairingResponder, PairingResponse), PairingError> {
        PairingResponder::respond_inner(own, None, offer, fresh_nonce(), now)
    }

    /// [`PairingResponder::respond`] for a device that gives `statement` ([SEC-SEL-050]).
    pub(crate) fn respond_with_statement(
        own: &DeviceIdentity,
        statement: Option<AgreementStatement>,
        offer: PairingOffer,
        now: Timestamp,
    ) -> Result<(PairingResponder, PairingResponse), PairingError> {
        PairingResponder::respond_inner(own, statement, offer, fresh_nonce(), now)
    }

    /// [`PairingResponder::respond`] with a given nonce, for tests only.
    #[cfg(test)]
    pub(crate) fn respond_with_nonce(
        own: &DeviceIdentity,
        offer: PairingOffer,
        nonce: PairingNonce,
        now: Timestamp,
    ) -> Result<(PairingResponder, PairingResponse), PairingError> {
        PairingResponder::respond_inner(own, None, offer, nonce, now)
    }

    /// [`PairingResponder::respond_with_statement`] with a given nonce, for tests only.
    #[cfg(test)]
    pub(crate) fn respond_with_statement_and_nonce(
        own: &DeviceIdentity,
        statement: Option<AgreementStatement>,
        offer: PairingOffer,
        nonce: PairingNonce,
        now: Timestamp,
    ) -> Result<(PairingResponder, PairingResponse), PairingError> {
        PairingResponder::respond_inner(own, statement, offer, nonce, now)
    }

    fn respond_inner(
        own: &DeviceIdentity,
        statement: Option<AgreementStatement>,
        offer: PairingOffer,
        nonce: PairingNonce,
        now: Timestamp,
    ) -> Result<(PairingResponder, PairingResponse), PairingError> {
        if &offer.public_key == own.public_key() {
            return Err(PairingError::OwnKey);
        }
        let own_value = StatementValue::of_own(statement.as_ref());
        let response = PairingResponse {
            principal: own.principal().clone(),
            public_key: *own.public_key(),
            nonce,
            agreement_statement: statement,
        };
        Ok((
            PairingResponder {
                principal: own.principal().clone(),
                public_key: *own.public_key(),
                nonce,
                offer,
                started: now,
                own_set: TrustedKeySet::new(own),
                own_value,
            },
            response,
        ))
    }

    /// Takes the initiator's reveal and checks it against the commitment.
    pub(crate) fn receive(
        self,
        reveal: PairingReveal,
        now: &Timestamp,
    ) -> Result<PairingSession, PairingError> {
        if now.unix_nanos() >= deadline(&self.started) {
            return Err(PairingError::Expired);
        }
        // The commitment covers the statement as the initiator gave it, so a statement
        // removed or replaced on the way fails here.
        let claimed = StatementValue::of_own(self.offer.agreement_statement.as_ref());
        let expected = commitment(
            &self.offer.principal,
            &self.offer.public_key,
            &claimed,
            &reveal.nonce,
        );
        if !bool::from(expected.ct_eq(&self.offer.commitment)) {
            return Err(PairingError::CommitmentMismatch);
        }
        // [SEC-SEL-051]: the statement is checked before the comparison exists.
        let peer_value = judge_with(
            &self.own_set,
            &self.offer.principal,
            &self.offer.public_key,
            self.offer.agreement_statement.as_ref(),
            now,
        );
        let code = code(
            (
                &self.offer.principal,
                &self.offer.public_key,
                &peer_value,
                &reveal.nonce,
            ),
            (
                &self.principal,
                &self.public_key,
                &self.own_value,
                &self.nonce,
            ),
        );
        Ok(PairingSession::new(
            self.offer.principal,
            self.offer.public_key,
            code,
            self.started,
            self.own_value,
            peer_value,
            self.offer.agreement_statement,
        ))
    }
}

#[derive(Debug)]
enum ResponderState {
    AwaitingOffer,
    AwaitingReveal(Box<PairingResponder>),
    Ended(PairingEnd),
    Completed,
}

/// One operator-started pairing on the responding device. It answers **one** offer, and an
/// exchange that does not reach its code ends the pairing visibly ([`PairingEnd`]). To try
/// again, the operator starts a new pairing.
///
/// This stops an attacker on the exchange channel from grinding codes by abandoning
/// exchanges. Without it, the attacker could complete the initiator's side first (the
/// initiator shows its code as soon as it has a response). It could then play initiator
/// toward the responder again and again, never revealing, until the responder's code
/// matched. With one offer per operator-visible pairing, each try costs the attacker an
/// exchange the operator sees end. A substituted key then matches with probability 10^-6 per
/// operator-visible exchange.
#[derive(Debug)]
pub struct ResponderPairing {
    started: Timestamp,
    state: ResponderState,
    statement: Option<AgreementStatement>,
}

impl ResponderPairing {
    /// The operator started a pairing on this device at `now`. It stays open for 120
    /// seconds.
    pub fn start(now: Timestamp) -> ResponderPairing {
        ResponderPairing {
            started: now,
            state: ResponderState::AwaitingOffer,
            statement: None,
        }
    }

    /// [`ResponderPairing::start`] for a device that uses a sealing transport: the response
    /// carries its current agreement statement `statement` ([SEC-SEL-050]), and the code
    /// covers its value.
    pub fn start_with_statement(now: Timestamp, statement: AgreementStatement) -> ResponderPairing {
        ResponderPairing {
            statement: Some(statement),
            ..ResponderPairing::start(now)
        }
    }

    fn end(&mut self, why: PairingEnd) -> PairingError {
        self.state = ResponderState::Ended(why);
        PairingError::Ended(why)
    }

    /// Answers `offer`: step 2. Only the first offer is answered.
    ///
    /// # Errors
    ///
    /// [`PairingError::Ended`]. A second offer ends the pairing
    /// ([`PairingEnd::SecondOffer`]), as do an offer after the window
    /// ([`PairingEnd::Expired`]) and one carrying this device's own key. An ended or
    /// completed pairing answers nothing.
    pub fn answer(
        &mut self,
        own: &DeviceIdentity,
        offer: PairingOffer,
        now: &Timestamp,
    ) -> Result<PairingResponse, PairingError> {
        match &self.state {
            ResponderState::AwaitingOffer => {}
            ResponderState::AwaitingReveal(_) | ResponderState::Completed => {
                return Err(self.end(PairingEnd::SecondOffer));
            }
            ResponderState::Ended(why) => return Err(PairingError::Ended(*why)),
        }
        if now.unix_nanos() >= deadline(&self.started) {
            return Err(self.end(PairingEnd::Expired));
        }
        match PairingResponder::respond_with_statement(
            own,
            self.statement.clone(),
            offer,
            self.started.clone(),
        ) {
            Ok((responder, response)) => {
                self.state = ResponderState::AwaitingReveal(Box::new(responder));
                Ok(response)
            }
            Err(_) => Err(self.end(PairingEnd::OwnKey)),
        }
    }

    /// Takes the initiator's reveal: step 3. The code is then known.
    ///
    /// # Errors
    ///
    /// [`PairingError::Ended`]: with no answered offer, after the window
    /// ([`PairingEnd::Abandoned`]), or when the reveal does not match the commitment.
    pub fn reveal(
        &mut self,
        reveal: PairingReveal,
        now: &Timestamp,
    ) -> Result<PairingSession, PairingError> {
        let state = std::mem::replace(&mut self.state, ResponderState::Completed);
        let responder = match state {
            ResponderState::AwaitingReveal(r) => r,
            ResponderState::AwaitingOffer => return Err(self.end(PairingEnd::Abandoned)),
            ResponderState::Ended(why) => return Err(self.end(why)),
            ResponderState::Completed => return Err(self.end(PairingEnd::SecondOffer)),
        };
        match responder.receive(reveal, now) {
            Ok(session) => Ok(session),
            Err(PairingError::CommitmentMismatch) => Err(self.end(PairingEnd::CommitmentMismatch)),
            Err(_) => Err(self.end(PairingEnd::Abandoned)),
        }
    }

    /// The caller reports that the exchange was dropped before the reveal: the peer went
    /// away, or the channel closed. The pairing ends, and the operator sees why.
    pub fn abandon(&mut self) {
        if !matches!(
            self.state,
            ResponderState::Completed | ResponderState::Ended(_)
        ) {
            self.state = ResponderState::Ended(PairingEnd::Abandoned);
        }
    }

    /// Why the pairing ended, checked at `now`. An answered offer whose reveal has not come
    /// within the window is abandoned, and a pairing that never got an offer has expired.
    /// `None` while it is still open, and once it has produced its code.
    pub fn ended(&mut self, now: &Timestamp) -> Option<PairingEnd> {
        if now.unix_nanos() >= deadline(&self.started) {
            match self.state {
                ResponderState::AwaitingOffer => {
                    self.state = ResponderState::Ended(PairingEnd::Expired);
                }
                ResponderState::AwaitingReveal(_) => {
                    self.state = ResponderState::Ended(PairingEnd::Abandoned);
                }
                _ => {}
            }
        }
        match self.state {
            ResponderState::Ended(why) => Some(why),
            _ => None,
        }
    }
}

/// Why a code was not confirmed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmError {
    /// The entered code differs. `attempts_left` more wrong entries are allowed; at zero the
    /// session is aborted.
    Mismatch {
        /// Wrong entries still allowed.
        attempts_left: u32,
    },
    /// 120 seconds have passed since the exchange started.
    Expired,
    /// Five wrong entries aborted the session.
    Aborted,
    /// The session already produced its [`PairedPeer`].
    Completed,
}

impl fmt::Display for ConfirmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfirmError::Mismatch { attempts_left } => {
                write!(f, "the code differs; {attempts_left} attempt(s) left")
            }
            ConfirmError::Expired => f.write_str("the pairing session expired"),
            ConfirmError::Aborted => f.write_str("the pairing session was aborted"),
            ConfirmError::Completed => f.write_str("the pairing session already completed"),
        }
    }
}

impl std::error::Error for ConfirmError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionState {
    Open { attempts_left: u32 },
    Aborted,
    Completed,
}

/// One side of a pairing exchange once its six-digit code is known: show the code, then
/// confirm it.
#[derive(Debug)]
pub struct PairingSession {
    peer_principal: Token,
    peer_public_key: PublicKey,
    code: String,
    started: Timestamp,
    state: SessionState,
    own_value: StatementValue,
    peer_value: StatementValue,
    peer_statement: Option<AgreementStatement>,
}

impl PairingSession {
    fn new(
        peer_principal: Token,
        peer_public_key: PublicKey,
        code: String,
        started: Timestamp,
        own_value: StatementValue,
        peer_value: StatementValue,
        peer_statement: Option<AgreementStatement>,
    ) -> PairingSession {
        PairingSession {
            peer_principal,
            peer_public_key,
            code,
            started,
            state: SessionState::Open {
                attempts_left: PAIRING_MAX_ATTEMPTS,
            },
            own_value,
            // A refused statement is not carried on: the key is paired with none
            // ([SEC-SEL-049]).
            peer_statement: peer_statement.filter(|_| peer_value.given().is_some()),
            peer_value,
        }
    }

    /// This device's own statement value, which the code covers ([SEC-SEL-044]): show it
    /// beside the code.
    pub fn own_statement_value(&self) -> &StatementValue {
        &self.own_value
    }

    /// The peer's statement value, which the code covers ([SEC-SEL-044]): show it beside the
    /// code. A statement refused under [SEC-SEL-010] to [SEC-SEL-013] or [SEC-SEL-042] is
    /// [`StatementValue::Refused`], checked before the code is shown ([SEC-SEL-051]).
    pub fn peer_statement_value(&self) -> &StatementValue {
        &self.peer_value
    }

    /// The six-digit code to show the operator.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The peer's principal label, to show the operator.
    pub fn peer_principal(&self) -> &Token {
        &self.peer_principal
    }

    /// The peer's key id, to show the operator.
    pub fn peer_key_id(&self) -> KeyId {
        self.peer_public_key.key_id()
    }

    fn check_open(&self, now: &Timestamp) -> Result<(), ConfirmError> {
        match self.state {
            SessionState::Aborted => Err(ConfirmError::Aborted),
            SessionState::Completed => Err(ConfirmError::Completed),
            SessionState::Open { .. } if now.unix_nanos() >= deadline(&self.started) => {
                Err(ConfirmError::Expired)
            }
            SessionState::Open { .. } => Ok(()),
        }
    }

    fn complete(&mut self, now: &Timestamp) -> PairedPeer {
        self.state = SessionState::Completed;
        PairedPeer {
            record: PairingRecord {
                principal: self.peer_principal.clone(),
                public_key: self.peer_public_key,
                paired_at: now.clone(),
                confirmed: None,
            },
            statement: self.peer_statement.take(),
        }
    }

    /// The operator typed `entered`, the code the other device shows. Five wrong entries
    /// abort the session.
    ///
    /// # Errors
    ///
    /// [`ConfirmError`].
    pub fn confirm_entered(
        &mut self,
        entered: &str,
        now: &Timestamp,
    ) -> Result<PairedPeer, ConfirmError> {
        self.check_open(now)?;
        if bool::from(entered.as_bytes().ct_eq(self.code.as_bytes())) {
            return Ok(self.complete(now));
        }
        let SessionState::Open { attempts_left } = self.state else {
            unreachable!("check_open passed")
        };
        let attempts_left = attempts_left - 1;
        self.state = if attempts_left == 0 {
            SessionState::Aborted
        } else {
            SessionState::Open { attempts_left }
        };
        Err(ConfirmError::Mismatch { attempts_left })
    }

    /// The operator compared the codes both devices show and confirmed they are the same.
    ///
    /// # Errors
    ///
    /// [`ConfirmError::Expired`], [`ConfirmError::Aborted`] or
    /// [`ConfirmError::Completed`].
    pub fn confirm_match(
        &mut self,
        now: &Timestamp,
        _: OperatorConfirmed,
    ) -> Result<PairedPeer, ConfirmError> {
        self.check_open(now)?;
        Ok(self.complete(now))
    }
}

/// A key an operator confirmed for pairing ([SEC-KEY-032]), ready for
/// [`crate::authorization::AuthorizationEngine::pair`]. Built only by a pairing session's
/// confirmation or by [`PairedPeer::by_key_id_comparison`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairedPeer {
    record: PairingRecord,
    statement: Option<AgreementStatement>,
}

impl PairedPeer {
    /// The file-exchange flow: the operator compared `confirmed_key_id`, read from the peer
    /// device through a channel other than the one that delivered `public_key`, with the
    /// key id of `public_key`, and confirmed it. `None` when they differ.
    pub fn by_key_id_comparison(
        principal: Token,
        public_key: PublicKey,
        confirmed_key_id: &KeyId,
        now: Timestamp,
        _: OperatorConfirmed,
    ) -> Option<PairedPeer> {
        (&public_key.key_id() == confirmed_key_id).then_some(PairedPeer {
            record: PairingRecord {
                principal,
                public_key,
                paired_at: now,
                confirmed: None,
            },
            statement: None,
        })
    }

    /// The same pairing, carrying the peer's agreement statement, read beside its key file
    /// ([SEC-SEL-050]; `spec/security.md` §14.3, "Pairing"). The operator compares the
    /// values of both devices' statements beside the key id ([SEC-SEL-044]); show the
    /// peer's with [`judge_statement`] first ([SEC-SEL-051]). It adds no trust: the engine
    /// admits it only if it verifies under the key being paired, and then keeps its values
    /// as the pairing's confirmed values. Without a statement the key is paired with no
    /// confirmed values and admits none ([SEC-SEL-049]).
    pub fn with_agreement_statement(mut self, statement: AgreementStatement) -> PairedPeer {
        self.statement = Some(statement);
        self
    }

    #[cfg(test)]
    pub(crate) fn confirmed(principal: Token, public_key: PublicKey, now: Timestamp) -> PairedPeer {
        PairedPeer {
            record: PairingRecord {
                principal,
                public_key,
                paired_at: now,
                confirmed: None,
            },
            statement: None,
        }
    }

    /// The agreement statement the pairing carried, if any.
    pub fn agreement_statement(&self) -> Option<&AgreementStatement> {
        self.statement.as_ref()
    }

    /// The peer's principal label.
    pub fn principal(&self) -> &Token {
        &self.record.principal
    }

    /// The peer's key id.
    pub fn key_id(&self) -> KeyId {
        self.record.key_id()
    }

    pub(crate) fn into_parts(self) -> (PairingRecord, Option<AgreementStatement>) {
        (self.record, self.statement)
    }
}

/// A completed pairing, as the pairing store keeps it: the peer's principal, public key
/// and key id (its fingerprint), and when pairing completed
/// (`docs/planning/decisions/C5-envelope-auth.md` §10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingRecord {
    principal: Token,
    public_key: PublicKey,
    paired_at: Timestamp,
    confirmed: Option<ConfirmedValues>,
}

impl PairingRecord {
    /// A record read back from a store. The key passed [SEC-KEY-034] when its
    /// [`PublicKey`] was built. Only a store implementation calls this, for a record a
    /// confirmed pairing produced earlier.
    pub fn from_store(
        principal: Token,
        public_key: PublicKey,
        paired_at: Timestamp,
    ) -> PairingRecord {
        PairingRecord {
            principal,
            public_key,
            paired_at,
            confirmed: None,
        }
    }

    /// The same record with the values its pairing confirmed ([SEC-SEL-044]), as a store
    /// reads them back, or as the engine sets them from the statement it admitted at the
    /// pairing. They are kept and removed with the record ([SEC-SEL-046], [SEC-SEL-047]).
    #[must_use]
    pub fn with_confirmed(mut self, confirmed: Option<ConfirmedValues>) -> PairingRecord {
        self.confirmed = confirmed;
        self
    }

    /// The values the pairing confirmed: the floor of every later statement of the key
    /// ([SEC-SEL-045], [SEC-SEL-048]). `None` when it confirmed none, and then the key admits
    /// no statement ([SEC-SEL-049]).
    pub fn confirmed(&self) -> Option<&ConfirmedValues> {
        self.confirmed.as_ref()
    }

    /// The peer's principal label.
    pub fn principal(&self) -> &Token {
        &self.principal
    }

    /// The peer's public key.
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// The peer's key id.
    pub fn key_id(&self) -> KeyId {
        self.public_key.key_id()
    }

    /// When pairing completed.
    pub fn paired_at(&self) -> &Timestamp {
        &self.paired_at
    }
}

/// What a [`PairingStore`] keeps: the paired keys with the values their pairings confirmed
/// ([SEC-SEL-046]), the grants, the sessions for which an operator enabled permission relay,
/// and the agreement statements held for paired keys, as durably as the keys
/// ([SEC-SEL-041]). The binding table and the sent and hand-off
/// records are not kept: they live in memory (`spec/security.md` §11.3, §9.5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PairingSnapshot {
    /// The paired keys, each with the values its pairing confirmed, if any.
    pub paired: Vec<PairingRecord>,
    /// The grants.
    pub grants: Vec<Grant>,
    /// The sessions with permission relay enabled ([SEC-AUZ-021]).
    pub relay_enabled: Vec<SessionId>,
    /// The agreement statement held for each paired key that has one (`spec/security.md`
    /// §14.3).
    pub agreement_statements: Vec<HeldStatement>,
}

/// Why a [`PairingStore`] could not read or write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PairingStoreError {
    /// The store holds something that is not a pairing snapshot.
    Corrupt(String),
    /// The store failed while reading or writing.
    Failed(String),
}

impl fmt::Display for PairingStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PairingStoreError::Corrupt(e) => write!(f, "pairing store unusable: {e}"),
            PairingStoreError::Failed(e) => write!(f, "pairing store failed: {e}"),
        }
    }
}

impl std::error::Error for PairingStoreError {}

/// Where an implementation keeps its pairings, grants and relay settings across restarts.
/// An empty store is the default-deny configuration: no peer, no grant, no relay.
pub trait PairingStore {
    /// The stored snapshot; an empty one when nothing was saved yet.
    ///
    /// # Errors
    ///
    /// [`PairingStoreError`] when the store cannot be read.
    fn load(&self) -> Result<PairingSnapshot, PairingStoreError>;

    /// Replaces the stored snapshot with `snapshot`, as one write.
    ///
    /// # Errors
    ///
    /// [`PairingStoreError`] when the store cannot be written.
    fn save(&self, snapshot: &PairingSnapshot) -> Result<(), PairingStoreError>;
}

/// A [`PairingStore`] in this process's memory: nothing outlives the process. The test
/// double; [`MemoryPairingStore::fail_saves`] makes its writes fail.
#[derive(Debug, Default)]
pub struct MemoryPairingStore {
    snapshot: Mutex<PairingSnapshot>,
    fail: AtomicBool,
}

impl MemoryPairingStore {
    /// An empty store.
    pub fn new() -> MemoryPairingStore {
        MemoryPairingStore::default()
    }

    /// Makes every later save fail (`true`) or succeed (`false`).
    pub fn fail_saves(&self, fail: bool) {
        self.fail.store(fail, Ordering::SeqCst);
    }
}

impl PairingStore for MemoryPairingStore {
    fn load(&self) -> Result<PairingSnapshot, PairingStoreError> {
        self.snapshot
            .lock()
            .map(|s| s.clone())
            .map_err(|_| PairingStoreError::Failed("lock poisoned".into()))
    }

    fn save(&self, snapshot: &PairingSnapshot) -> Result<(), PairingStoreError> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(PairingStoreError::Failed(
                "save refused (test double)".into(),
            ));
        }
        let mut slot = self
            .snapshot
            .lock()
            .map_err(|_| PairingStoreError::Failed("lock poisoned".into()))?;
        *slot = snapshot.clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authorization::{AuthorizationEngine, MemoryDecisionLog};
    use crate::clock::SystemClock;
    use crate::keys::DeviceKey;
    use std::sync::Arc;

    fn identity(p: &str) -> DeviceIdentity {
        DeviceIdentity::new(DeviceKey::generate(), Token::parse(p).unwrap())
    }

    fn at(ms: i64) -> Timestamp {
        Timestamp::from_unix_millis(1_790_000_000_000 + ms).unwrap()
    }

    fn ok() -> OperatorConfirmed {
        OperatorConfirmed::by_operator()
    }

    /// One honest exchange: both sides end with the same code, each naming the other.
    fn exchange(a: &DeviceIdentity, b: &DeviceIdentity) -> (PairingSession, PairingSession) {
        let (init, offer) = PairingInitiator::start(a, at(0));
        let mut pairing = ResponderPairing::start(at(0));
        let response = pairing.answer(b, offer, &at(1_000)).unwrap();
        let (sa, reveal) = init.receive(response, &at(2_000)).unwrap();
        let sb = pairing.reveal(reveal, &at(3_000)).unwrap();
        assert_eq!(
            pairing.ended(&at(200_000)),
            None,
            "a completed pairing has not ended"
        );
        (sa, sb)
    }

    #[test]
    fn a_responder_answers_one_offer_per_pairing() {
        let (a, b, m) = (
            identity("principal-a"),
            identity("principal-b"),
            identity("principal-m"),
        );
        // An attacker plays initiator toward B, gets B's nonce, and abandons the exchange
        // before revealing, hoping to try again with a new offer: the second offer ends the
        // pairing, and so does any later one.
        let mut pairing = ResponderPairing::start(at(0));
        let (_, first) = PairingInitiator::start(&m, at(0));
        pairing.answer(&b, first, &at(1_000)).unwrap();
        let (_, second) = PairingInitiator::start(&m, at(1_000));
        assert_eq!(
            pairing.answer(&b, second.clone(), &at(1_500)).unwrap_err(),
            PairingError::Ended(PairingEnd::SecondOffer)
        );
        assert_eq!(pairing.ended(&at(1_500)), Some(PairingEnd::SecondOffer));
        assert_eq!(
            pairing.answer(&b, second, &at(1_600)).unwrap_err(),
            PairingError::Ended(PairingEnd::SecondOffer)
        );
        // Even the honest initiator's reveal is refused now: the operator starts again.
        let (init, offer) = PairingInitiator::start(&a, at(2_000));
        let (_, reveal) = init
            .receive(
                PairingResponse {
                    principal: b.principal().clone(),
                    public_key: *b.public_key(),
                    nonce: [1; 32],
                    agreement_statement: None,
                },
                &at(2_000),
            )
            .unwrap();
        drop(offer);
        assert_eq!(
            pairing.reveal(reveal, &at(2_500)).unwrap_err(),
            PairingError::Ended(PairingEnd::SecondOffer)
        );
        // A completed pairing answers no further offer either.
        let mut done = ResponderPairing::start(at(0));
        let (init, offer) = PairingInitiator::start(&a, at(0));
        let response = done.answer(&b, offer, &at(1)).unwrap();
        let (_, reveal) = init.receive(response, &at(2)).unwrap();
        done.reveal(reveal, &at(3)).unwrap();
        let (_, again) = PairingInitiator::start(&m, at(4));
        assert_eq!(
            done.answer(&b, again, &at(5)).unwrap_err(),
            PairingError::Ended(PairingEnd::SecondOffer)
        );
    }

    #[test]
    fn an_abandoned_exchange_ends_the_pairing_visibly() {
        let (a, b) = (identity("principal-a"), identity("principal-b"));
        // The caller reports the exchange dropped before the reveal.
        let mut pairing = ResponderPairing::start(at(0));
        let (init, offer) = PairingInitiator::start(&a, at(0));
        let response = pairing.answer(&b, offer, &at(1)).unwrap();
        pairing.abandon();
        assert_eq!(pairing.ended(&at(2)), Some(PairingEnd::Abandoned));
        let (_, reveal) = init.receive(response, &at(3)).unwrap();
        assert_eq!(
            pairing.reveal(reveal, &at(4)).unwrap_err(),
            PairingError::Ended(PairingEnd::Abandoned)
        );
        // A reveal that never comes ends the pairing at the end of the window.
        let mut pairing = ResponderPairing::start(at(0));
        let (_, offer) = PairingInitiator::start(&a, at(0));
        pairing.answer(&b, offer, &at(1)).unwrap();
        assert_eq!(pairing.ended(&at(119_999)), None);
        assert_eq!(pairing.ended(&at(120_000)), Some(PairingEnd::Abandoned));
        // No offer at all: expired. An offer after the window: expired too.
        let mut idle = ResponderPairing::start(at(0));
        assert_eq!(idle.ended(&at(120_000)), Some(PairingEnd::Expired));
        let mut late = ResponderPairing::start(at(0));
        let (_, offer) = PairingInitiator::start(&a, at(0));
        assert_eq!(
            late.answer(&b, offer, &at(120_000)).unwrap_err(),
            PairingError::Ended(PairingEnd::Expired)
        );
        // A reveal that does not match the commitment ends it as well.
        let mut pairing = ResponderPairing::start(at(0));
        let (_, offer) = PairingInitiator::start(&a, at(0));
        pairing.answer(&b, offer, &at(1)).unwrap();
        assert_eq!(
            pairing
                .reveal(PairingReveal { nonce: [0; 32] }, &at(2))
                .unwrap_err(),
            PairingError::Ended(PairingEnd::CommitmentMismatch)
        );
        // An offer with this device's own key ends it.
        let mut own = ResponderPairing::start(at(0));
        let (_, offer) = PairingInitiator::start(&b, at(0));
        assert_eq!(
            own.answer(&b, offer, &at(1)).unwrap_err(),
            PairingError::Ended(PairingEnd::OwnKey)
        );
    }

    #[test]
    fn honest_exchange_gives_one_code_and_pairs_with_the_engine() {
        let (a, b) = (identity("principal-a"), identity("principal-b"));
        let (mut sa, mut sb) = exchange(&a, &b);
        assert_eq!(sa.code(), sb.code());
        assert_eq!(sa.code().len(), 6);
        assert!(sa.code().bytes().all(|c| c.is_ascii_digit()));
        assert_eq!(sa.peer_key_id(), b.key_id().clone());
        assert_eq!(sb.peer_key_id(), a.key_id().clone());
        assert_eq!(sb.peer_principal(), a.principal());
        // The operator types A's code into B, and confirms the match on A.
        let code = sa.code().to_owned();
        let peer_a = sb.confirm_entered(&code, &at(10_000)).unwrap();
        let peer_b = sa.confirm_match(&at(10_000), ok()).unwrap();
        assert_eq!(
            sb.confirm_entered(&code, &at(10_000)),
            Err(ConfirmError::Completed)
        );
        // Each engine trusts the other's key, and grants nothing.
        let store = MemoryPairingStore::new();
        let mut eb = AuthorizationEngine::new(
            &b,
            Arc::new(SystemClock),
            Box::new(MemoryDecisionLog::new()),
        );
        eb.pair(peer_a, &store).unwrap();
        assert!(
            eb.trusted_keys()
                .resolve("principal-a", a.key_id().as_str())
                .is_some()
        );
        assert!(eb.grants().is_empty());
        let ea_store = MemoryPairingStore::new();
        let mut ea = AuthorizationEngine::new(
            &a,
            Arc::new(SystemClock),
            Box::new(MemoryDecisionLog::new()),
        );
        ea.pair(peer_b.clone(), &ea_store).unwrap();
        assert!(matches!(
            ea.pair(peer_b, &ea_store),
            Err(crate::authorization::PairError::AlreadyTrusted(_))
        ));
        // The store restores the pairing after a restart.
        let back = AuthorizationEngine::restore(
            &b,
            &store,
            Arc::new(SystemClock),
            Box::new(MemoryDecisionLog::new()),
        )
        .unwrap();
        assert!(back.trusted_keys().get(a.key_id()).is_some());
        assert_eq!(store.load().unwrap().paired[0].paired_at(), &at(10_000));
    }

    #[test]
    fn five_wrong_entries_abort_and_120_seconds_expire() {
        let (a, b) = (identity("principal-a"), identity("principal-b"));
        let (sa, mut sb) = exchange(&a, &b);
        let wrong = if sa.code() == "000000" {
            "000001"
        } else {
            "000000"
        };
        for left in (0..PAIRING_MAX_ATTEMPTS).rev() {
            assert_eq!(
                sb.confirm_entered(wrong, &at(5_000)),
                Err(ConfirmError::Mismatch {
                    attempts_left: left
                })
            );
        }
        // Aborted: even the right code no longer pairs.
        assert_eq!(
            sb.confirm_entered(sa.code(), &at(5_000)),
            Err(ConfirmError::Aborted)
        );
        let (mut sa, mut sb) = exchange(&a, &b);
        let code = sa.code().to_owned();
        // 120 seconds after the start, the code no longer pairs.
        assert_eq!(
            sb.confirm_entered(&code, &at(121_000)),
            Err(ConfirmError::Expired)
        );
        assert_eq!(
            sa.confirm_match(&at(120_000), ok()),
            Err(ConfirmError::Expired)
        );
        assert!(sb.confirm_entered(&code, &at(119_999)).is_ok());
        // An exchange whose response arrives late is refused.
        let (init, offer) = PairingInitiator::start(&a, at(0));
        let (_, response) = PairingResponder::respond(&b, offer, at(0)).unwrap();
        assert_eq!(
            init.receive(response, &at(120_000)).unwrap_err(),
            PairingError::Expired
        );
    }

    #[test]
    fn substituted_key_or_nonce_changes_the_code_or_fails() {
        let seeded = |p: &str, octet: u8| {
            let seed = crate::keys::SecretSeed::from_slice(&[octet; 32]).unwrap();
            DeviceIdentity::new(DeviceKey::from_seed(&seed), Token::parse(p).unwrap())
        };
        let (a, b, m) = (
            seeded("principal-a", 1),
            seeded("principal-b", 2),
            seeded("principal-b", 3),
        );
        // A man in the middle answers A as B, with its own key: A's code differs from the
        // one B computes with A's real offer, except with probability 10^-6 over the
        // nonces. Fixed seeds and nonces keep the test deterministic.
        let (ni, nr) = ([7u8; 32], [9u8; 32]);
        let (init, offer) = PairingInitiator::start_with_nonce(&a, ni, at(0));
        // B's own response never reaches A: the attacker sends A its own instead.
        let (resp, _) = PairingResponder::respond_with_nonce(&b, offer.clone(), nr, at(0)).unwrap();
        let (_, m_response) = PairingResponder::respond_with_nonce(&m, offer, nr, at(0)).unwrap();
        let (sa_with_m, reveal) = init.receive(m_response, &at(1)).unwrap();
        let sb = resp.receive(reveal, &at(1)).unwrap();
        assert_ne!(sa_with_m.code(), sb.code());
        assert_eq!(sa_with_m.peer_key_id(), m.key_id().clone());
        // A reveal that does not match the commitment is refused.
        let (_, offer) = PairingInitiator::start_with_nonce(&a, ni, at(0));
        let (resp, _) = PairingResponder::respond_with_nonce(&b, offer, nr, at(0)).unwrap();
        assert_eq!(
            resp.receive(PairingReveal { nonce: [8u8; 32] }, &at(1))
                .unwrap_err(),
            PairingError::CommitmentMismatch
        );
        // The code binds the principal labels too.
        let (_, mut offer) = PairingInitiator::start_with_nonce(&a, ni, at(0));
        offer.principal = Token::parse("principal-z").unwrap();
        let (resp, _) = PairingResponder::respond_with_nonce(&b, offer, nr, at(0)).unwrap();
        assert_eq!(
            resp.receive(PairingReveal { nonce: ni }, &at(1))
                .unwrap_err(),
            PairingError::CommitmentMismatch
        );
        // A device does not pair with its own key.
        let (_, offer) = PairingInitiator::start(&a, at(0));
        assert_eq!(
            PairingResponder::respond(&a, offer, at(0)).unwrap_err(),
            PairingError::OwnKey
        );
    }

    fn statement_of(id: &DeviceIdentity, octet: u8, seq: u64, ms: i64) -> AgreementStatement {
        let key = crate::sealing::AgreementPublicKey::from_octets([octet; 32]).unwrap();
        AgreementStatement::issue(id, &key, seq, &at(ms)).unwrap()
    }

    /// [SEC-SEL-044] and the reference note of `spec/security.md` §14.3: the initiator's
    /// statement is in its commitment, before the code is derived. An attacker that holds
    /// several old statements of the initiator, all signed by its device key, cannot give
    /// the responder another one under the same commitment, so cannot pick the one that
    /// makes the code match; and for each statement the code differs.
    #[test]
    fn the_statement_enters_the_commitment_before_the_code() {
        let (a, b) = (identity("principal-a"), identity("principal-b"));
        let (ni, nr) = ([7u8; 32], [9u8; 32]);
        let current = statement_of(&a, 3, 4, 1_000);
        let (_, offer) =
            PairingInitiator::start_with_statement_and_nonce(&a, current.clone(), ni, at(2_000));
        // The same key, principal and nonce with no statement, or another, commit differently.
        let (_, bare) = PairingInitiator::start_with_nonce(&a, ni, at(2_000));
        assert_ne!(offer.commitment, bare.commitment);
        let mut codes = Vec::new();
        for (octet, seq) in [(3u8, 4u64), (4, 3), (5, 2), (6, 1)] {
            let old = statement_of(&a, octet, seq, 500 + i64::from(octet));
            let (_, o) =
                PairingInitiator::start_with_statement_and_nonce(&a, old.clone(), ni, at(2_000));
            if old != current {
                assert_ne!(o.commitment, offer.commitment, "seq {seq}");
            }
            // Handed the responder under the commitment of the current statement, an old
            // statement ends the exchange at the reveal, before any code exists.
            let mut swapped = offer.clone();
            swapped.agreement_statement = Some(old.clone());
            let (resp, _) =
                PairingResponder::respond_with_nonce(&b, swapped, nr, at(2_000)).unwrap();
            let outcome = resp.receive(PairingReveal { nonce: ni }, &at(2_100));
            if old == current {
                codes.push(outcome.unwrap().code().to_owned());
            } else {
                assert_eq!(outcome.unwrap_err(), PairingError::CommitmentMismatch);
            }
            // Each statement, committed honestly, gives its own code.
            let (r, _) = PairingResponder::respond_with_nonce(&b, o, nr, at(2_000)).unwrap();
            codes.push(
                r.receive(PairingReveal { nonce: ni }, &at(2_100))
                    .unwrap()
                    .code()
                    .to_owned(),
            );
        }
        // The six-digit codes of four distinct statements are not all one value (the
        // probability that they are is 10^-18).
        codes.sort();
        codes.dedup();
        assert!(codes.len() > 1, "{codes:?}");
        // The responder's statement is in the code, whatever the initiator's.
        let responder = |st: Option<AgreementStatement>| {
            let (r, response) = PairingResponder::respond_with_statement_and_nonce(
                &b,
                st,
                offer.clone(),
                nr,
                at(2_000),
            )
            .unwrap();
            (
                r.receive(PairingReveal { nonce: ni }, &at(2_100)).unwrap(),
                response,
            )
        };
        let (s1, _) = responder(Some(statement_of(&b, 5, 9, 1_500)));
        let (s2, _) = responder(Some(statement_of(&b, 5, 8, 1_500)));
        let (s3, _) = responder(None);
        assert_eq!(s1.own_statement_value().given().map(|g| g.seq()), Some(9));
        assert_ne!(s1.code(), s2.code());
        assert_ne!(s1.code(), s3.code());
    }

    #[test]
    fn key_id_comparison_pairs_only_the_compared_key() {
        let (a, b) = (identity("principal-a"), identity("principal-b"));
        assert!(
            PairedPeer::by_key_id_comparison(
                b.principal().clone(),
                *b.public_key(),
                a.key_id(),
                at(0),
                ok()
            )
            .is_none()
        );
        let peer = PairedPeer::by_key_id_comparison(
            b.principal().clone(),
            *b.public_key(),
            b.key_id(),
            at(0),
            ok(),
        )
        .unwrap();
        assert_eq!(peer.key_id(), b.key_id().clone());
        assert_eq!(peer.principal(), b.principal());
    }

    #[test]
    fn an_empty_store_is_default_deny() {
        let store = MemoryPairingStore::new();
        assert_eq!(store.load().unwrap(), PairingSnapshot::default());
        let me = identity("principal-me");
        let e = AuthorizationEngine::restore(
            &me,
            &store,
            Arc::new(SystemClock),
            Box::new(MemoryDecisionLog::new()),
        )
        .unwrap();
        assert_eq!(e.trusted_keys().len(), 1);
        assert!(e.grants().is_empty());
    }
}
