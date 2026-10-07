// SPDX-License-Identifier: Apache-2.0

//! OAC core: the neutral types, policy and authorization of OAC Session Channels.
//!
//! This crate depends on nothing in-repo, and no provider or transport crate may be
//! reachable from it (`docs/planning/v0.1/07-repository-and-dependencies.md` section 3).
//!
//! Implemented so far (#51, F2), against the interfaces frozen at revision 0.1:
//!
//! - [`json`]: strict I-JSON reading and compact writing ([SC-ENV-001] to [SC-ENV-003]).
//! - [`canonical`]: the RFC 8785 canonical form and signing input of `spec/security.md`
//!   §6.2 ([SEC-SIG-011], [SEC-SIG-013]).
//! - [`ids`]: identifier tokens, session ids, versions, timestamps, part types, key ids.
//! - [`envelope`]: the envelope and `ChannelMessage`, envelope-stage validation and
//!   outbound drafts (`spec/session-channels.md` §4, §5.4; `spec/interfaces.md` §4.5).
//! - [`capabilities`]: `SessionCapabilities`, version agreement and `SessionDescriptor`
//!   (§6.3 to §6.5; `spec/interfaces.md` §4.3, §4.4).
//! - [`presence`]: `PresenceRecord` and `PresenceState` (§7.2.1, §7.2.2).
//! - [`receipt`] and [`delivery`]: `DeliveryReceipt`, `DeliveryState` and `ErrorCode`
//!   (§8.1, §8.3).
//!
//! Added by #52 (F3), against `spec/security.md` revision 0.1:
//!
//! - [`keys`]: the device key, its public key and key id, the device identity that signs
//!   envelopes, and the [`keys::KeyStore`] seam for the private seed (§5.1, §5.2).
//! - [`trust`]: the trusted key set and `SecurityPrincipal`'s only source (§5.3).
//! - [`signing`]: strict Ed25519 verification and security-stage steps 1 and 2 (§6, §7).
//! - [`registration`]: the registration record that binds a session to the device key
//!   (§5.4).
//!
//! Added by #56 (F7): [`transport`], the `Transport` contract and the transport-boundary
//! types (`spec/interfaces.md` §4.11, §6), and [`health`], `HealthStatus` (§4.10).
//!
//! Added by #53 (F4), against `spec/security.md` revision 0.1:
//!
//! - [`replay`]: security-stage step 3 (the replay window), step 5 (the duplicate store)
//!   and the hand-off deadline (§8; `spec/session-channels.md` §8.1.3). Step 4,
//!   authorization, is [`authorization`]'s.
//! - [`clock`]: the injectable receiver clock those checks read.
//!
//! Added by #54 (F5), against `spec/security.md` revision 0.1 and `spec/interfaces.md` §4.9:
//!
//! - [`authorization`]: grants, the binding table, reply rights, `AuthorizationRequest` and
//!   `AuthorizationDecision`, security step 4, key removal, and the decision log (§9).
//! - [`pairing`]: the operator-confirmed pairing exchange and the [`pairing::PairingStore`]
//!   seam (§5.3).
//!
//! Added by #59 (F10): [`adapter`], the `ProviderAdapter` contract and the adapter-boundary
//! types (`spec/interfaces.md` §4.10, §5), so that the adapter contract suite can run against
//! any adapter.
//!
//! Added by #55 (F6), against `spec/session-channels.md` §7 and §8 and `spec/security.md`
//! §8.4, §10 and §11 at revision 0.1:
//!
//! - [`registry`]: the presence registry (states, staleness, carrier loss, the
//!   cross-implementation cap), the issuer of own sessions' records, discovery and the
//!   presence step of a send.
//! - [`presence_auth`]: the authenticated presence record and the consumer's checks.
//! - [`sender`]: the send decision of §8.3.3 and the per-envelope delivery-state machine.
//! - [`receiver`]: security step 5 and the delivery stage for one copy, hand-off outcomes,
//!   and the receipt gate with its rate limit.
//! - [`receipt_auth`]: the authenticated receipt.
//! - [`reply`]: reply headers and reply correlation (§8.2).
//!
//! Requirement ids in square brackets name the requirement a rule implements.

pub mod adapter;
pub mod authorization;
mod base64url;
pub mod canonical;
pub mod capabilities;
pub mod clock;
pub mod delivery;
pub mod envelope;
pub mod health;
pub mod ids;
pub mod json;
pub mod keys;
pub mod pairing;
pub mod presence;
pub mod presence_auth;
pub mod receipt;
pub mod receipt_auth;
pub mod receiver;
pub mod registration;
pub mod registry;
pub mod replay;
pub mod reply;
pub mod sender;
pub mod signing;
pub mod transport;
pub mod trust;
