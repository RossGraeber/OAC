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
//! Requirement ids in square brackets name the requirement a rule implements.

pub mod canonical;
pub mod capabilities;
pub mod delivery;
pub mod envelope;
pub mod ids;
pub mod json;
pub mod presence;
pub mod receipt;
