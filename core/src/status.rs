// SPDX-License-Identifier: Apache-2.0

//! The core's status snapshot (G10, #71): what [`Pipelines::status`](crate::pipeline::Pipelines::status)
//! returns, for an operator's `status` verb.
//!
//! **What it holds.** The health of the transport and of each provider adapter, as each
//! reported it (`spec/interfaces.md` §4.10, §5.7, §6.8); this device's own sessions (the id
//! the core gave it, the harness label when known, the capabilities agreed for it, and
//! whether it is online or withheld); and counters of what the core holds.
//!
//! **What it never holds.** No harness-native session id and no working directory
//! ([IFC-TYP-020], [IFC-TYP-092]): no type here has a place for either, and
//! [`HealthStatus::detail`] is the only free text, written by the transport or adapter that
//! owns it under the same rule. No key material, no message content, no peer's address.
//!
//! **No lock is held while a transport or an adapter is asked.** The core takes a snapshot of
//! its own state under its lock, releases the lock, and only then calls `health` on the
//! transport and on each adapter (`spec/interfaces.md` Tables 5.2 and 6.4: the core is the
//! caller). A slow or stuck adapter therefore cannot block the core's other work, and a
//! status snapshot taken while one is stuck is as late as that adapter is, not later.

use crate::health::HealthStatus;
use crate::ids::{SessionId, Token};
use crate::pipeline::AdapterId;

/// The capabilities agreed for one of this device's own sessions (`spec/session-channels.md`
/// §6.5), as its announced declaration states them for the version this implementation
/// speaks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgreedCapabilities {
    /// `active_inbound`: whether the session accepts a message as live input.
    pub active_inbound: bool,
    /// The part types it accepts besides `text`, when it declares a list.
    pub content_types: Vec<String>,
    /// The largest envelope, in octets, it accepts.
    pub max_envelope_octets: u64,
}

/// Whether one of this device's own sessions can be reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    /// Announced `online`.
    Online,
    /// Withheld under [SC-ID-154]: no delivery to it, and it reads `unreachable` to peers,
    /// until a later native signal pairs.
    Withheld,
}

impl SessionState {
    /// The state's name for display.
    pub fn as_str(self) -> &'static str {
        match self {
            SessionState::Online => "online",
            SessionState::Withheld => "withheld",
        }
    }
}

/// One of this device's own sessions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionStatus {
    /// The session id the core gave it: its only address.
    pub session_id: SessionId,
    /// The harness label, when the session's descriptor has one. For display only.
    pub harness_label: Option<Token>,
    /// The capabilities agreed for it, `None` when its declaration has no entry this
    /// implementation can read.
    pub capabilities: Option<AgreedCapabilities>,
    /// Whether it is online or withheld.
    pub state: SessionState,
}

/// What one provider adapter reported when asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterStatus {
    /// Which adapter: the value [`Pipelines::add_adapter`](crate::pipeline::Pipelines::add_adapter)
    /// returned for it.
    pub adapter: AdapterId,
    /// Its `health` (`spec/interfaces.md` §5.7).
    pub health: HealthStatus,
}

/// How much of each thing the core holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatusCounts {
    /// Local connections taken and not yet ended.
    pub open_connections: usize,
    /// Open attachments with no session bound.
    pub unbound_attachments: usize,
    /// Native signals waiting for their decision.
    pub pending_signals: usize,
    /// Native signals held as not yet pairable.
    pub held_signals: usize,
    /// Other devices whose keys this device trusts (its own key is not counted).
    pub trusted_devices: usize,
    /// Grants the operator has given.
    pub grants: usize,
    /// Frames from a sealing transport that opened under no agreement key held
    /// ([SEC-SEL-038]).
    pub discarded_frames: u64,
}

/// A snapshot of the core, as [`Pipelines::status`](crate::pipeline::Pipelines::status)
/// takes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreStatus {
    /// The transport's `health` (`spec/interfaces.md` §6.8).
    pub transport: HealthStatus,
    /// Each adapter's `health`, in the order the adapters were added.
    pub adapters: Vec<AdapterStatus>,
    /// This device's own sessions, ordered by session id.
    pub sessions: Vec<SessionStatus>,
    /// What the core holds.
    pub counts: StatusCounts,
}
