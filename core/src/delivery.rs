// SPDX-License-Identifier: Apache-2.0

//! Delivery states (`spec/session-channels.md` Table 8.1), observers (§8.1.1) and the
//! closed error taxonomy (Table 8.3). `spec/interfaces.md` §4.6 calls these
//! `DeliveryState` and `ErrorCode`.

use std::fmt;

/// One of the eight delivery states of Table 8.1 ([IFC-TYP-051]). The conceptual
/// `accepted` of DESIGN is not one of them: §8.1.2 splits it into the first three.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeliveryState {
    /// The sending implementation created a valid envelope and passed it to a transport.
    AcceptedByAdapter,
    /// The receiver's input call to the harness completed. Not "seen by the model"
    /// (§8.1.3).
    HandedToHarness,
    /// The observer cannot determine whether the content was handed off.
    Unknown,
    /// The receiver refused the envelope.
    Rejected,
    /// The expiry instant or the replay window passed before hand-off.
    Expired,
    /// A copy of an envelope already handed off, possibly handed off, or being handed off.
    Duplicate,
    /// The addressed session is unknown, or known but not accepting input.
    Unreachable,
    /// An error unrelated to validity stopped delivery; the content was not handed off.
    Failed,
}

impl DeliveryState {
    /// Every state, in Table 8.1 order.
    pub const ALL: [DeliveryState; 8] = [
        DeliveryState::AcceptedByAdapter,
        DeliveryState::HandedToHarness,
        DeliveryState::Unknown,
        DeliveryState::Rejected,
        DeliveryState::Expired,
        DeliveryState::Duplicate,
        DeliveryState::Unreachable,
        DeliveryState::Failed,
    ];

    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryState::AcceptedByAdapter => "accepted-by-adapter",
            DeliveryState::HandedToHarness => "handed-to-harness",
            DeliveryState::Unknown => "unknown",
            DeliveryState::Rejected => "rejected",
            DeliveryState::Expired => "expired",
            DeliveryState::Duplicate => "duplicate",
            DeliveryState::Unreachable => "unreachable",
            DeliveryState::Failed => "failed",
        }
    }

    /// The state named by `s`, compared exactly.
    pub fn parse(s: &str) -> Option<DeliveryState> {
        DeliveryState::ALL.into_iter().find(|st| st.as_str() == s)
    }

    /// Whether `observer` may report this state (Table 8.1, "Observer"; [SC-RCP-002]).
    pub fn allowed_for(self, observer: Observer) -> bool {
        use DeliveryState::*;
        match observer {
            Observer::Sender => matches!(self, AcceptedByAdapter | Unknown | Unreachable | Failed),
            Observer::Receiver => !matches!(self, AcceptedByAdapter),
        }
    }

    /// Whether a receipt with this state carries an `error` ([SC-RCP-025],
    /// [SC-RCP-026]).
    pub fn carries_error(self) -> bool {
        matches!(
            self,
            DeliveryState::Rejected
                | DeliveryState::Expired
                | DeliveryState::Duplicate
                | DeliveryState::Unreachable
                | DeliveryState::Failed
        )
    }
}

impl fmt::Display for DeliveryState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who observed a delivery state (§8.1.1, §8.1.4 `observer`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Observer {
    /// The sending implementation.
    Sender,
    /// The receiving implementation.
    Receiver,
}

impl Observer {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            Observer::Sender => "sender",
            Observer::Receiver => "receiver",
        }
    }

    /// The observer named by `s` ([SC-RCP-023]).
    pub fn parse(s: &str) -> Option<Observer> {
        match s {
            "sender" => Some(Observer::Sender),
            "receiver" => Some(Observer::Receiver),
            _ => None,
        }
    }
}

/// The stage a code belongs to (§8.3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Envelope-stage validation.
    Envelope,
    /// Verification, replay and authorization.
    Security,
    /// Routing and hand-off.
    Delivery,
    /// A harness's request, refused before any envelope exists.
    Request,
    /// `internal-error`: any stage.
    Any,
}

/// Where a code may appear (§8.3.1, "Scope").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// In a receipt observed by the sending implementation.
    Sender,
    /// In a receipt observed by the receiver.
    Receiver,
    /// As an error returned to a harness for a refused request.
    Request,
}

/// An error code of Table 8.3, the closed set an implementation emits ([SC-RCP-074]).
/// A code a peer sends that is not in this set is held as
/// [`crate::receipt::ReceiptError::Unrecognized`], never as an `ErrorCode`, so this crate
/// cannot emit one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// `envelope-too-large`
    EnvelopeTooLarge,
    /// `malformed-envelope`
    MalformedEnvelope,
    /// `unsupported-version`
    UnsupportedVersion,
    /// `unsupported-content-type`
    UnsupportedContentType,
    /// `expired`
    Expired,
    /// `unknown-key`
    UnknownKey,
    /// `signature-invalid`
    SignatureInvalid,
    /// `outside-replay-window`
    OutsideReplayWindow,
    /// `duplicate`
    Duplicate,
    /// `unauthorized`
    Unauthorized,
    /// `unknown-destination`
    UnknownDestination,
    /// `destination-unavailable`
    DestinationUnavailable,
    /// `unsupported-capability`
    UnsupportedCapability,
    /// `handoff-failed`
    HandoffFailed,
    /// `transport-failure`
    TransportFailure,
    /// `internal-error`
    InternalError,
    /// `invalid-request`
    InvalidRequest,
}

impl ErrorCode {
    /// Every code, in Table 8.3 order.
    pub const ALL: [ErrorCode; 17] = [
        ErrorCode::EnvelopeTooLarge,
        ErrorCode::MalformedEnvelope,
        ErrorCode::UnsupportedVersion,
        ErrorCode::UnsupportedContentType,
        ErrorCode::Expired,
        ErrorCode::UnknownKey,
        ErrorCode::SignatureInvalid,
        ErrorCode::OutsideReplayWindow,
        ErrorCode::Duplicate,
        ErrorCode::Unauthorized,
        ErrorCode::UnknownDestination,
        ErrorCode::DestinationUnavailable,
        ErrorCode::UnsupportedCapability,
        ErrorCode::HandoffFailed,
        ErrorCode::TransportFailure,
        ErrorCode::InternalError,
        ErrorCode::InvalidRequest,
    ];

    /// The wire string.
    pub fn as_str(self) -> &'static str {
        self.row().0
    }

    /// The code named by `s`, compared exactly and case-sensitively (§8.3.1).
    pub fn parse(s: &str) -> Option<ErrorCode> {
        ErrorCode::ALL.into_iter().find(|c| c.as_str() == s)
    }

    /// The code's stage (Table 8.3, "Stage").
    pub fn stage(self) -> Stage {
        self.row().1
    }

    /// The delivery state a receipt carrying this code has (Table 8.3, "State"); `None`
    /// for `invalid-request`, which appears in no receipt.
    pub fn state(self) -> Option<DeliveryState> {
        self.row().2
    }

    /// Where the code may appear (Table 8.3, "Scope").
    pub fn scope(self) -> &'static [Scope] {
        self.row().3
    }

    /// Whether a receipt with this code may carry `state` from `observer` ([SC-RCP-028]).
    pub fn fits_receipt(self, state: DeliveryState, observer: Observer) -> bool {
        let scope = match observer {
            Observer::Sender => Scope::Sender,
            Observer::Receiver => Scope::Receiver,
        };
        self.state() == Some(state) && self.scope().contains(&scope)
    }

    fn row(self) -> (&'static str, Stage, Option<DeliveryState>, &'static [Scope]) {
        use DeliveryState as D;
        use Scope::{Receiver as R, Request as Q, Sender as S};
        match self {
            ErrorCode::EnvelopeTooLarge => (
                "envelope-too-large",
                Stage::Envelope,
                Some(D::Rejected),
                &[R, Q],
            ),
            ErrorCode::MalformedEnvelope => (
                "malformed-envelope",
                Stage::Envelope,
                Some(D::Rejected),
                &[R],
            ),
            ErrorCode::UnsupportedVersion => (
                "unsupported-version",
                Stage::Envelope,
                Some(D::Rejected),
                &[R, Q],
            ),
            ErrorCode::UnsupportedContentType => (
                "unsupported-content-type",
                Stage::Envelope,
                Some(D::Rejected),
                &[R, Q],
            ),
            ErrorCode::Expired => ("expired", Stage::Envelope, Some(D::Expired), &[R]),
            ErrorCode::UnknownKey => ("unknown-key", Stage::Security, Some(D::Rejected), &[R]),
            ErrorCode::SignatureInvalid => (
                "signature-invalid",
                Stage::Security,
                Some(D::Rejected),
                &[R],
            ),
            ErrorCode::OutsideReplayWindow => (
                "outside-replay-window",
                Stage::Security,
                Some(D::Expired),
                &[R],
            ),
            ErrorCode::Duplicate => ("duplicate", Stage::Security, Some(D::Duplicate), &[R]),
            ErrorCode::Unauthorized => {
                ("unauthorized", Stage::Security, Some(D::Rejected), &[R, Q])
            }
            ErrorCode::UnknownDestination => (
                "unknown-destination",
                Stage::Delivery,
                Some(D::Unreachable),
                &[S, R, Q],
            ),
            ErrorCode::DestinationUnavailable => (
                "destination-unavailable",
                Stage::Delivery,
                Some(D::Unreachable),
                &[S, R, Q],
            ),
            ErrorCode::UnsupportedCapability => (
                "unsupported-capability",
                Stage::Delivery,
                Some(D::Rejected),
                &[R, Q],
            ),
            ErrorCode::HandoffFailed => ("handoff-failed", Stage::Delivery, Some(D::Failed), &[R]),
            ErrorCode::TransportFailure => {
                ("transport-failure", Stage::Delivery, Some(D::Failed), &[S])
            }
            ErrorCode::InternalError => ("internal-error", Stage::Any, Some(D::Failed), &[S, R, Q]),
            ErrorCode::InvalidRequest => ("invalid-request", Stage::Request, None, &[Q]),
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// True when `s` has the form of an error code: `[a-z][a-z0-9-]{0,63}` as a whole value
/// ([SC-RCP-027]).
pub fn is_error_code_form(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=64).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_names() {
        for s in DeliveryState::ALL {
            assert_eq!(DeliveryState::parse(s.as_str()), Some(s));
        }
        for c in ErrorCode::ALL {
            assert_eq!(ErrorCode::parse(c.as_str()), Some(c));
            assert!(is_error_code_form(c.as_str()));
        }
        assert_eq!(DeliveryState::parse("accepted"), None);
        assert_eq!(ErrorCode::parse("Expired"), None);
    }
}
