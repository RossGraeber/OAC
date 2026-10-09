// SPDX-License-Identifier: Apache-2.0

//! The provider adapter contract of `spec/interfaces.md` §5 (Table 5.2, `ProviderAdapter`)
//! and the adapter-boundary types of §4.10, in Rust form (#59, F10; deferred here from F2,
//! PR #312).
//!
//! The core is the caller of every operation (§5.3). An adapter module implements
//! [`ProviderAdapter`]; it depends on this crate, never the other way round, and it never
//! holds a transport ([IFC-ADP-001]): no operation here takes or returns one. Nothing here
//! names a harness-native concept ([IFC-NEU-002]): the only harness-supplied strings that
//! reach the core are the `native_id` and `cross_check` of a [`NativeSignal`] and the
//! `cross_check` of an [`AdapterEvent::AttachmentOpened`] event, and the untrusted request
//! members of §4.10.
//!
//! The Rust form adds what `spec/interfaces.md` §1.3 leaves to a language binding: a
//! [`Connection`] owns the byte stream of the local connection it names, and the event
//! stream of `watch_attachments` is a handler the adapter calls, as the transport
//! contract's handlers are (`crate::transport`).
//!
//! What is not here yet: `ProvenanceSet` (§4.10; `spec/security.md` §12.1), which the
//! adapter computes and renders inside its own module (G4 to G8), and the operating-system
//! peer authentication that [IFC-ADP-012] asks of the core before [`Connection::accept`]
//! (G9).

use std::fmt;
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;

use crate::capabilities::SessionDescriptor;
use crate::delivery::{DeliveryState, ErrorCode};
use crate::envelope::{ChannelMessage, ContentPart};
use crate::health::HealthStatus;
use crate::ids::{SessionId, Token};
use crate::receipt::DeliveryReceipt;

/// The handle of one `Connection` (`spec/interfaces.md` §4.10): what names a local
/// connection in events, requests and hand-offs. An [`Attachment`] is the handle of a
/// connection that the adapter reported as an attachment.
///
/// The only way to obtain one is [`Connection::accept`], which the core calls
/// ([IFC-ADP-012]). Each call issues a handle never issued before in this process; there is
/// no constructor from a value, and no operation changes a handle ([IFC-ADP-013]). Cloning
/// copies the same handle.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnectionHandle(u64);

impl fmt::Debug for ConnectionHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ConnectionHandle(#{})", self.0)
    }
}

/// `Attachment` of `spec/interfaces.md` §4.10: the [`ConnectionHandle`] of an attachment.
pub type Attachment = ConnectionHandle;

static NEXT_CONNECTION: AtomicU64 = AtomicU64::new(1);

/// `Connection` of `spec/interfaces.md` §4.10: one local connection that the core process
/// accepted, with its handle and its byte stream. `take_connection` gives it to the
/// adapter, which reads the harness's traffic on it (§5.4).
pub struct Connection {
    handle: ConnectionHandle,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
}

impl Connection {
    /// For the core only: a connection over `reader` and `writer`, with a fresh handle.
    ///
    /// [IFC-ADP-012]: the core calls this in the core process, for a local connection whose
    /// peer it has authenticated with an operating-system facility (G9). [IFC-ADP-013]: an
    /// adapter never calls it; the adapter contract suite fails an adapter that reports a
    /// handle the core did not give it.
    pub fn accept(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
    ) -> Connection {
        Connection {
            handle: ConnectionHandle(NEXT_CONNECTION.fetch_add(1, Ordering::Relaxed)),
            reader: Box::new(reader),
            writer: Box::new(writer),
        }
    }

    /// The handle.
    pub fn handle(&self) -> &ConnectionHandle {
        &self.handle
    }

    /// The handle and the two halves of the byte stream.
    pub fn into_parts(
        self,
    ) -> (
        ConnectionHandle,
        Box<dyn Read + Send>,
        Box<dyn Write + Send>,
    ) {
        (self.handle, self.reader, self.writer)
    }
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("handle", &self.handle)
            .finish_non_exhaustive()
    }
}

/// The `start_kind` of a [`NativeSignal`] (`spec/session-channels.md` §6.7.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StartKind {
    /// `fresh`.
    Fresh,
    /// `transition`.
    Transition,
    /// `unmapped`.
    Unmapped,
}

impl StartKind {
    /// The value's name in `spec/interfaces.md` §4.10.
    pub fn as_str(self) -> &'static str {
        match self {
            StartKind::Fresh => "fresh",
            StartKind::Transition => "transition",
            StartKind::Unmapped => "unmapped",
        }
    }
}

/// `NativeSignal` of `spec/interfaces.md` §4.10: what an adapter observed at one native
/// signal. It has no pairing key: the core observes that itself (§5.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeSignal {
    /// `native_id`, the harness-native id N.
    pub native_id: String,
    /// `start_kind`; `None` when the harness reported none.
    pub start_kind: Option<StartKind>,
    /// `cross_check`, the signal's cross-check value.
    pub cross_check: Option<String>,
    /// `connection`, the connection the signal arrived on; `None` when no local connection
    /// carried it.
    pub connection: Option<ConnectionHandle>,
}

/// An adapter event on the `watch_attachments` stream (`spec/interfaces.md` §5.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdapterEvent {
    /// `attachment-opened`: the connection is a local path to one live session.
    AttachmentOpened {
        /// The connection.
        attachment: Attachment,
        /// The attachment's cross-check value, when the adapter observed one.
        cross_check: Option<String>,
    },
    /// `native-signal`: a native signal arrived through the surface the adapter binding
    /// document names as authoritative for N.
    NativeSignal(NativeSignal),
    /// `capabilities-changed`: `capabilities` for the attachment would now return a
    /// different value ([IFC-ADP-043]).
    CapabilitiesChanged {
        /// The attachment.
        attachment: Attachment,
    },
    /// `attachment-closed`: the attachment ended ([IFC-ADP-022], [IFC-ADP-071]).
    AttachmentClosed {
        /// The attachment.
        attachment: Attachment,
    },
}

/// `AdapterCapabilities` of `spec/interfaces.md` §4.10: what an adapter can do for one
/// attachment. No `revision` and no extension identifier: the core adds those
/// ([IFC-ADP-042]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterCapabilities {
    /// `active_inbound` ([IFC-ADP-040]).
    pub active_inbound: bool,
    /// `content_types`: part types it can hand off besides `text` ([IFC-ADP-041]).
    pub content_types: Vec<String>,
    /// `max_envelope_octets`, when the adapter states one ([IFC-ADP-041]).
    pub max_envelope_octets: Option<u64>,
}

/// `SendRequest` of `spec/interfaces.md` §4.10: a harness's request to send.
///
/// [IFC-TYP-090]: no member names the requesting session. The core attributes the request
/// by `attachment` ([SC-ID-160]) and never by a session id the request carries
/// ([SC-ID-162]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendRequest {
    /// `attachment`: the attachment on which the core process received the request
    /// ([IFC-ADP-031]).
    pub attachment: Attachment,
    /// `to`.
    pub to: SessionId,
    /// `content` (`spec/session-channels.md` §4.5).
    pub content: Vec<ContentPart>,
    /// `requested_target`: the message the harness says this request answers, for a reply.
    /// Untrusted; it need not be an identifier token (§4.10).
    pub requested_target: Option<String>,
    /// `conversation_id`, for a new message. Untrusted harness input.
    pub conversation_id: Option<String>,
    /// `correlation_id`, for a new message. Untrusted harness input.
    pub correlation_id: Option<String>,
}

/// `DiscoveryRequest` of `spec/interfaces.md` §4.10.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryRequest {
    /// `attachment` ([IFC-ADP-031]).
    pub attachment: Attachment,
}

/// Whether a reply was correlated (`spec/session-channels.md` [SC-RCP-055]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Correlation {
    /// `correlated`.
    Correlated,
    /// `uncorrelated`.
    Uncorrelated,
}

/// The event stream of a `sent` result: the `DeliveryReceipt` values the core later holds
/// for the envelope ([IFC-ADP-062]).
pub struct ReceiptStream(pub Receiver<DeliveryReceipt>);

impl fmt::Debug for ReceiptStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReceiptStream(..)")
    }
}

/// `RequestResult` of `spec/interfaces.md` §4.10 for a `SendRequest`: one of its three
/// outcomes.
///
/// §4.10 defines one `RequestResult` whose forms depend on the request's kind: three
/// outcomes for a `SendRequest`, and the discovery result or `refused` for a
/// `DiscoveryRequest`. This binding gives each kind its own type
/// ([`SendRequestResult`], [`DiscoveryRequestResult`]), so that no value pairs a result with
/// the wrong kind of request.
#[derive(Debug)]
pub enum SendRequestResult {
    /// `refused`: no envelope was created; `error` has `request` in its Table 8.3 scope.
    Refused {
        /// The refusal's code.
        error: ErrorCode,
    },
    /// `not-passed`: an envelope was created but not passed to a transport; its state is
    /// `failed`, its code `transport-failure` or `internal-error`.
    NotPassed {
        /// The envelope's `id`.
        id: Token,
        /// The code.
        error: ErrorCode,
    },
    /// `sent`: the envelope was passed to a transport; its state is `accepted-by-adapter`.
    Sent {
        /// The envelope's `id`.
        id: Token,
        /// For a reply, whether it was correlated.
        correlation: Option<Correlation>,
        /// The receipts the core later holds for the envelope.
        receipts: ReceiptStream,
    },
}

impl SendRequestResult {
    /// The outcome's name in §4.10: `refused`, `not-passed` or `sent`.
    pub fn outcome(&self) -> &'static str {
        match self {
            SendRequestResult::Refused { .. } => "refused",
            SendRequestResult::NotPassed { .. } => "not-passed",
            SendRequestResult::Sent { .. } => "sent",
        }
    }

    /// The error code, for `refused` and `not-passed`.
    pub fn error(&self) -> Option<ErrorCode> {
        match self {
            SendRequestResult::Refused { error } | SendRequestResult::NotPassed { error, .. } => {
                Some(*error)
            }
            SendRequestResult::Sent { .. } => None,
        }
    }

    /// The delivery state the result reports: `failed` for `not-passed`,
    /// `accepted-by-adapter` for `sent`, none for `refused` (§4.10).
    pub fn state(&self) -> Option<DeliveryState> {
        match self {
            SendRequestResult::Refused { .. } => None,
            SendRequestResult::NotPassed { .. } => Some(DeliveryState::Failed),
            SendRequestResult::Sent { .. } => Some(DeliveryState::AcceptedByAdapter),
        }
    }

    /// The envelope id, for `not-passed` and `sent`.
    pub fn id(&self) -> Option<&Token> {
        match self {
            SendRequestResult::NotPassed { id, .. } | SendRequestResult::Sent { id, .. } => {
                Some(id)
            }
            SendRequestResult::Refused { .. } => None,
        }
    }
}

/// `RequestResult` of `spec/interfaces.md` §4.10 for a `DiscoveryRequest`: the discovery
/// result, or `refused` (see [`SendRequestResult`] for why the kinds are split).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryRequestResult {
    /// The discovery result: an array of `SessionDescriptor`.
    DiscoveryResult(Vec<SessionDescriptor>),
    /// `refused`, with an `ErrorCode`.
    Refused {
        /// The refusal's code.
        error: ErrorCode,
    },
}

/// The request sink of `accept_requests` (Table 5.2): "an operation that takes a
/// `SendRequest` or a `DiscoveryRequest` and returns a `RequestResult`". In this binding it
/// is one method per kind of request, each returning that kind's result
/// ([IFC-ADP-003], [IFC-ADP-060]).
pub trait RequestSink: Send + Sync {
    /// Take a `SendRequest`.
    fn send(&self, request: SendRequest) -> SendRequestResult;

    /// Take a `DiscoveryRequest`.
    fn discover(&self, request: DiscoveryRequest) -> DiscoveryRequestResult;
}

/// `HandOff` of `spec/interfaces.md` §4.10: what the core gives an adapter to hand off.
///
/// [IFC-TYP-091]: the message carries the `verified_by` the security stage set.
/// [`HandOff::new`] refuses a message without one, so no `HandOff` exists for a message
/// that did not pass security steps 1 and 2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandOff {
    attachment: Attachment,
    message: ChannelMessage,
}

impl HandOff {
    /// A hand-off of `message` to `attachment`; `None` when `message` has no
    /// `verified_by` ([IFC-TYP-091]).
    pub fn new(attachment: Attachment, message: ChannelMessage) -> Option<HandOff> {
        message.verified_by()?;
        Some(HandOff {
            attachment,
            message,
        })
    }

    /// `attachment`.
    pub fn attachment(&self) -> &Attachment {
        &self.attachment
    }

    /// `message`, with its `verified_by`. The adapter takes every provenance value from it,
    /// never from `content` ([SEC-PRV-002]).
    pub fn message(&self) -> &ChannelMessage {
        &self.message
    }
}

/// `HandOffOutcome` of `spec/interfaces.md` §4.10 and §5.5.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HandOffOutcome {
    /// `completed` ([IFC-ADP-051]).
    Completed,
    /// `not-now` ([IFC-ADP-052]).
    NotNow,
    /// `failed`.
    Failed,
    /// `indeterminate` ([IFC-ADP-053]).
    Indeterminate,
    /// `refused` ([IFC-ADP-054]).
    Refused,
}

impl HandOffOutcome {
    /// Every outcome, in Table 5.3 order.
    pub const ALL: [HandOffOutcome; 5] = [
        HandOffOutcome::Completed,
        HandOffOutcome::NotNow,
        HandOffOutcome::Failed,
        HandOffOutcome::Indeterminate,
        HandOffOutcome::Refused,
    ];

    /// The outcome's name in `spec/interfaces.md` §5.5.
    pub fn as_str(self) -> &'static str {
        match self {
            HandOffOutcome::Completed => "completed",
            HandOffOutcome::NotNow => "not-now",
            HandOffOutcome::Failed => "failed",
            HandOffOutcome::Indeterminate => "indeterminate",
            HandOffOutcome::Refused => "refused",
        }
    }

    /// The delivery state and code the core records for the outcome: Table 5.3
    /// ([IFC-ADP-055]).
    pub fn recorded(self) -> (DeliveryState, Option<ErrorCode>) {
        match self {
            HandOffOutcome::Completed => (DeliveryState::HandedToHarness, None),
            HandOffOutcome::NotNow => (
                DeliveryState::Unreachable,
                Some(ErrorCode::DestinationUnavailable),
            ),
            HandOffOutcome::Failed => (DeliveryState::Failed, Some(ErrorCode::HandoffFailed)),
            HandOffOutcome::Indeterminate => (DeliveryState::Unknown, None),
            HandOffOutcome::Refused => (DeliveryState::Failed, Some(ErrorCode::InternalError)),
        }
    }
}

/// The handler of the `watch_attachments` event stream (Table 5.2). The adapter calls it.
///
/// Re-entrancy: the core may call back into the adapter from inside the handler, on the
/// adapter's own calling thread, before the handler returns. Deciding a `native-signal`
/// or an `attachment-opened` can call `set_binding` and `capabilities`, and
/// `attachment-closed` calls `set_binding`. An adapter must therefore not hold a lock of
/// its own, that those operations also take, while it calls the handler. The core itself
/// never blocks inside the handler waiting for another binding decision: a decision already
/// running on another thread takes this one over.
pub type AdapterEventHandler = Arc<dyn Fn(AdapterEvent) + Send + Sync>;

/// `ProviderAdapter` of `spec/interfaces.md` Table 5.2. The core calls every operation.
///
/// An implementation satisfies each requirement that Appendix C of `spec/interfaces.md`
/// assigns to the `adapter` ([IFC-ADP-010]), and has an adapter binding document
/// ([IFC-ADP-080]). The adapter contract suite (`tests/protocol/contract/adapter/`, F10)
/// runs against any implementation of this trait.
pub trait ProviderAdapter: Send + Sync {
    /// `take_connection`: one local connection that the core accepted and authenticated
    /// (§5.4).
    fn take_connection(&self, connection: Connection);

    /// `watch_attachments`: the adapter reports what it observed to `handler` (§5.4).
    fn watch_attachments(&self, handler: AdapterEventHandler);

    /// `set_binding`: the core's binding for `attachment`, the session id it bound, or
    /// `None` ([IFC-ADP-030]).
    ///
    /// Advisory: the core calls it after it has changed its own state, outside its lock,
    /// so under concurrent changes (a native signal dropped on another thread while a
    /// binding is made or released) two calls can reach the adapter in the other order, and
    /// its view can lag the core's until the next call. The core does not rely on it: it
    /// refuses a hand-off or a send request for an attachment it withholds by its own
    /// state ([SC-ID-154]; PR #337 re-review, N7).
    fn set_binding(&self, attachment: &Attachment, session: Option<SessionId>);

    /// `capabilities` for `attachment` (§5.5; [IFC-ADP-040], [IFC-ADP-041]).
    fn capabilities(&self, attachment: &Attachment) -> AdapterCapabilities;

    /// `deliver`: hand `hand_off` to the harness through its supported input surface, with
    /// at most one hand-off call, and return exactly one outcome ([IFC-ADP-050],
    /// [IFC-ADP-057]).
    fn deliver(&self, hand_off: HandOff) -> HandOffOutcome;

    /// `accept_requests`: the request sink to pass every harness request into (§5.6).
    fn accept_requests(&self, sink: Arc<dyn RequestSink>);

    /// `health` (§5.7; [IFC-TYP-092]).
    fn health(&self) -> HealthStatus;

    /// `shutdown`. Before it returns the adapter reports `attachment-closed` for each
    /// attachment still open ([IFC-ADP-071]); after it returns it hands off nothing and
    /// passes no request to the core ([IFC-ADP-070]).
    fn shutdown(&self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{EnvelopeDraft, EnvelopeLimits, TextPart, receive_envelope};
    use crate::ids::Timestamp;
    use crate::keys::{DeviceIdentity, DeviceKey};
    use crate::signing::authenticate;
    use crate::trust::TrustedKeySet;

    fn message(verified: bool) -> ChannelMessage {
        let me = DeviceIdentity::new(DeviceKey::generate(), Token::parse("principal-a").unwrap());
        let now = Timestamp::from_unix_millis(1_800_000_000_000).unwrap();
        let draft = EnvelopeDraft::new(
            Token::parse("m1").unwrap(),
            SessionId::from_random_octets([1; 16]),
            SessionId::from_random_octets([2; 16]),
            now.clone(),
            vec![TextPart::new("hi").unwrap()],
        )
        .unwrap();
        let env = me.sign_envelope(draft);
        let msg = receive_envelope(env.octets(), &EnvelopeLimits::default(), &now).unwrap();
        if verified {
            authenticate(msg, &TrustedKeySet::new(&me)).unwrap()
        } else {
            msg
        }
    }

    #[test]
    fn send_request_has_no_member_naming_the_requester() {
        // [IFC-TYP-090]: this exhaustive pattern stops compiling if a member is added, so a
        // member naming the requesting session cannot appear unnoticed.
        let r = SendRequest {
            attachment: Connection::accept(std::io::empty(), std::io::sink())
                .handle()
                .clone(),
            to: SessionId::from_random_octets([3; 16]),
            content: vec![],
            requested_target: None,
            conversation_id: None,
            correlation_id: None,
        };
        let SendRequest {
            attachment: _,
            to: _,
            content: _,
            requested_target: _,
            conversation_id: _,
            correlation_id: _,
        } = r;
    }

    #[test]
    fn hand_off_needs_a_verified_message() {
        // [IFC-TYP-091].
        let a = Connection::accept(std::io::empty(), std::io::sink())
            .handle()
            .clone();
        assert!(HandOff::new(a.clone(), message(false)).is_none());
        let h = HandOff::new(a.clone(), message(true)).unwrap();
        assert!(h.message().verified_by().is_some());
        assert_eq!(h.attachment(), &a);
    }

    #[test]
    fn every_connection_gets_a_fresh_handle() {
        let a = Connection::accept(std::io::empty(), std::io::sink());
        let b = Connection::accept(std::io::empty(), std::io::sink());
        assert_ne!(a.handle(), b.handle());
        assert_eq!(a.handle().clone(), *a.handle());
        assert!(format!("{a:?}").starts_with("Connection { handle: ConnectionHandle(#"));
        let (h, _, _) = b.into_parts();
        assert_ne!(&h, a.handle());
    }

    #[test]
    fn outcomes_are_recorded_as_table_5_3() {
        // [IFC-ADP-055].
        use DeliveryState as S;
        use ErrorCode as E;
        let rows: Vec<_> = HandOffOutcome::ALL
            .iter()
            .map(|o| (o.as_str(), o.recorded()))
            .collect();
        assert_eq!(
            rows,
            [
                ("completed", (S::HandedToHarness, None)),
                ("not-now", (S::Unreachable, Some(E::DestinationUnavailable))),
                ("failed", (S::Failed, Some(E::HandoffFailed))),
                ("indeterminate", (S::Unknown, None)),
                ("refused", (S::Failed, Some(E::InternalError))),
            ]
        );
    }

    #[test]
    fn request_results_report_their_outcome_state_and_code() {
        let id = Token::parse("m1").unwrap();
        let r = SendRequestResult::Refused {
            error: ErrorCode::Unauthorized,
        };
        assert_eq!(
            (r.outcome(), r.error(), r.state(), r.id()),
            ("refused", Some(ErrorCode::Unauthorized), None, None)
        );
        let r = SendRequestResult::NotPassed {
            id: id.clone(),
            error: ErrorCode::TransportFailure,
        };
        assert_eq!(r.outcome(), "not-passed");
        assert_eq!(r.state(), Some(DeliveryState::Failed));
        assert_eq!(r.id(), Some(&id));
        let (_tx, rx) = std::sync::mpsc::channel();
        let r = SendRequestResult::Sent {
            id: id.clone(),
            correlation: Some(Correlation::Uncorrelated),
            receipts: ReceiptStream(rx),
        };
        assert_eq!(
            (r.outcome(), r.error(), r.state()),
            ("sent", None, Some(DeliveryState::AcceptedByAdapter))
        );
        assert_ne!(
            DiscoveryRequestResult::DiscoveryResult(vec![]),
            DiscoveryRequestResult::Refused {
                error: ErrorCode::Unauthorized
            }
        );
        assert_eq!(StartKind::Transition.as_str(), "transition");
    }
}
