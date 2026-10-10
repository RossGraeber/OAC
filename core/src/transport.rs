// SPDX-License-Identifier: Apache-2.0

//! The transport contract of `spec/interfaces.md` §6 (Table 6.4, `Transport`) and the
//! transport-boundary types of §4.11, in Rust form.
//!
//! The core is the caller of every operation ([IFC-ADP-001]; 07 section 3). A transport
//! module implements [`Transport`]; it depends on this crate, never the other way round.
//! Nothing here names a transport-native concept ([IFC-NEU-001]): a [`CarrierHandle`] and a
//! [`TransportConfiguration`] are opaque, and the core never reads their inside
//! ([IFC-NEU-004]).
//!
//! The Rust form adds what `spec/interfaces.md` §1.3 leaves to a language binding: error
//! values ([`TransportError`]) for an operation called out of order or with a configuration
//! the transport does not recognize, and a [`Subscription`] value that ends the
//! subscription when it is ended or dropped.

use std::any::Any;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use crate::health::HealthStatus;
use crate::ids::{KeyId, SessionId};

/// `Destination` of `spec/interfaces.md` §4.11: where a payload goes.
///
/// [IFC-TYP-095]: a destination is either a session id or a key id, and nothing else. The
/// enum has exactly those two variants, so no other value, and no transport-native
/// address, can be a destination.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Destination {
    /// `session`: a session id, for an envelope (Table 6.1).
    Session(SessionId),
    /// `device`: a key id, for an authenticated presence record or an authenticated
    /// receipt (Table 6.1).
    Device(KeyId),
}

/// The `kind` of a [`Payload`] (`spec/interfaces.md` §4.11, Table 6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PayloadKind {
    /// `envelope`: the serialized envelope.
    Envelope,
    /// `presence`: the serialized authenticated presence record.
    Presence,
    /// `receipt`: the serialized authenticated receipt.
    Receipt,
    /// `sealed`: a sealed frame holding one payload of one of the three kinds above
    /// (`spec/security.md` §14.4). Only a transport that declares `sealing` carries it
    /// ([IFC-TRN-102]), and it carries no other kind ([IFC-TRN-100], [IFC-TRN-113]).
    Sealed,
}

impl PayloadKind {
    /// The kind's name in `spec/interfaces.md` Table 6.1.
    pub fn as_str(self) -> &'static str {
        match self {
            PayloadKind::Envelope => "envelope",
            PayloadKind::Presence => "presence",
            PayloadKind::Receipt => "receipt",
            PayloadKind::Sealed => "sealed",
        }
    }

    /// True when Table 6.1 sends a payload of this kind to `destination`'s kind:
    /// `envelope` to a `session`, `presence`, `receipt` and `sealed` to a `device`.
    pub fn fits(self, destination: &Destination) -> bool {
        matches!(
            (self, destination),
            (PayloadKind::Envelope, Destination::Session(_))
                | (PayloadKind::Presence, Destination::Device(_))
                | (PayloadKind::Receipt, Destination::Device(_))
                | (PayloadKind::Sealed, Destination::Device(_))
        )
    }
}

/// `Payload` of `spec/interfaces.md` §4.11: a `kind` and the octets the core passes
/// (§2.3). The octets are a wire form; a transport delivers them exactly as passed
/// ([IFC-TRN-030]). Cloning shares the octets.
#[derive(Clone, PartialEq, Eq)]
pub struct Payload {
    kind: PayloadKind,
    octets: Arc<[u8]>,
}

impl Payload {
    /// A payload of `kind` with `octets`.
    pub fn new(kind: PayloadKind, octets: impl Into<Arc<[u8]>>) -> Payload {
        Payload {
            kind,
            octets: octets.into(),
        }
    }

    /// `kind`.
    pub fn kind(&self) -> PayloadKind {
        self.kind
    }

    /// `octets`.
    pub fn octets(&self) -> &[u8] {
        &self.octets
    }

    /// The number of octets.
    pub fn len(&self) -> usize {
        self.octets.len()
    }

    /// True when there are no octets.
    pub fn is_empty(&self) -> bool {
        self.octets.is_empty()
    }
}

impl fmt::Debug for Payload {
    /// The kind and length only: the octets are content, not diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Payload")
            .field("kind", &self.kind)
            .field("len", &self.octets.len())
            .finish()
    }
}

/// `Deadline` of `spec/interfaces.md` §4.11: an instant, read on the clock of the
/// implementation that passes the payload, after which no copy of the payload is delivered
/// (§6.4; [IFC-TRN-034]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Deadline(Instant);

impl Deadline {
    /// The deadline at `instant`.
    pub fn at(instant: Instant) -> Deadline {
        Deadline(instant)
    }

    /// The instant.
    pub fn instant(self) -> Instant {
        self.0
    }

    /// True at or after the deadline: no copy may then be held or delivered
    /// ([IFC-TRN-034]).
    pub fn has_passed_at(self, now: Instant) -> bool {
        now >= self.0
    }
}

/// `CarrierHandle` of `spec/interfaces.md` §4.11 and §2.3: an opaque handle that names one
/// link of a transport. It names a link, not a device or a session ([IFC-TRN-012]).
///
/// The transport that creates a handle chooses its octets. Nothing else reads them: the
/// type has no accessor, and the core only stores, compares and passes back a handle
/// ([IFC-NEU-003], [IFC-NEU-004]). Its `Debug` form does not print them.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CarrierHandle(Arc<[u8]>);

impl CarrierHandle {
    /// A handle with the transport's own opaque octets.
    pub fn from_opaque(octets: impl Into<Arc<[u8]>>) -> CarrierHandle {
        CarrierHandle(octets.into())
    }
}

impl fmt::Debug for CarrierHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CarrierHandle(..)")
    }
}

/// `Inbound` of `spec/interfaces.md` §4.11: what a transport hands to the core, a payload
/// and the carrier handle of the link it arrived on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inbound {
    /// The payload, with its octets exactly as the sender's core passed them
    /// ([IFC-TRN-030]).
    pub payload: Payload,
    /// The link the payload arrived on.
    pub carrier: CarrierHandle,
}

/// A presence event of `spec/interfaces.md` §6.6, yielded by `watch_presence`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresenceEvent {
    /// `record`: an authenticated presence record and the link it arrived on.
    Record {
        /// The payload, of kind `presence`.
        payload: Payload,
        /// The link it arrived on.
        carrier: CarrierHandle,
    },
    /// `carrier-loss`: the link the handle names ended ([IFC-TRN-061]).
    CarrierLoss {
        /// The link that ended.
        carrier: CarrierHandle,
    },
}

/// `PublishResult` of `spec/interfaces.md` §4.11: whether the transport took the payload
/// (§6.4; [IFC-TRN-031], [IFC-TRN-032]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PublishResult {
    /// `taken`.
    Taken,
    /// `not-taken`: no copy of the payload can be delivered ([IFC-TRN-031]).
    NotTaken,
}

/// The `reach` member of `spec/interfaces.md` Table 6.3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reach {
    /// `local-only`: the transport carries payloads only within one implementation
    /// ([IFC-TRN-002]).
    LocalOnly,
    /// `cross-implementation`: the transport reaches other implementations.
    CrossImplementation,
}

impl Reach {
    /// The value's name in Table 6.3.
    pub fn as_str(self) -> &'static str {
        match self {
            Reach::LocalOnly => "local-only",
            Reach::CrossImplementation => "cross-implementation",
        }
    }
}

/// The floor of `max_payload_octets` ([IFC-TRN-023]).
pub const MIN_MAX_PAYLOAD_OCTETS: u64 = 65536;

/// The floor of `max_payload_octets` for a sealing transport: 65536 plus the 54 octets of a
/// frame's overhead ([IFC-TRN-109]).
pub const MIN_SEALING_MAX_PAYLOAD_OCTETS: u64 = 65590;

/// `TransportCapabilities` of `spec/interfaces.md` §4.11: the declaration of §6.3, Table
/// 6.3, that `start` returns.
///
/// The six booleans are the optional capabilities: a transport declares each present or
/// absent ([IFC-TRN-020]) and declares none present that it does not provide
/// ([IFC-TRN-021]). The core depends on none of them ([IFC-TRN-022]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TransportCapabilities {
    /// `reliability`.
    pub reliability: bool,
    /// `persistence`; always `false` in this revision ([IFC-TRN-026]).
    pub persistence: bool,
    /// `offline_queueing`; always `false` in this revision ([IFC-TRN-026]).
    pub offline_queueing: bool,
    /// `ordering`.
    pub ordering: bool,
    /// `multicast_discovery`.
    pub multicast_discovery: bool,
    /// `routing_federation`.
    pub routing_federation: bool,
    /// `reach`.
    pub reach: Reach,
    /// `destination_restricted` ([IFC-TRN-080]).
    pub destination_restricted: bool,
    /// `max_payload_octets`, at least [`MIN_MAX_PAYLOAD_OCTETS`] ([IFC-TRN-023]), and at
    /// least [`MIN_SEALING_MAX_PAYLOAD_OCTETS`] for a sealing transport ([IFC-TRN-109]).
    pub max_payload_octets: u64,
    /// `sealing`: the transport takes and yields only payloads of kind `sealed`
    /// (`spec/interfaces.md` §6.10). A declaration made under a revision before 0.3 has no
    /// such member, and is read as `false`.
    pub sealing: bool,
}

impl TransportCapabilities {
    /// The requirement ids of `spec/interfaces.md` §6.3 and §6.10 that this declaration
    /// breaks on its face: [IFC-TRN-026] (persistence or offline queueing present),
    /// [IFC-TRN-023] (`max_payload_octets` below the floor) and [IFC-TRN-109] (a sealing
    /// transport's `max_payload_octets` below 65590). Empty when it breaks none. Whether a
    /// capability declared present is really provided ([IFC-TRN-021]) is a question about
    /// the transport's behaviour, not about the declaration.
    pub fn contract_violations(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.persistence || self.offline_queueing {
            v.push("IFC-TRN-026");
        }
        if self.max_payload_octets < MIN_MAX_PAYLOAD_OCTETS {
            v.push("IFC-TRN-023");
        }
        if self.sealing && self.max_payload_octets < MIN_SEALING_MAX_PAYLOAD_OCTETS {
            v.push("IFC-TRN-109");
        }
        v
    }
}

/// `TransportConfiguration` of `spec/interfaces.md` §4.11: a value that the transport
/// defines and the core passes to `start` unread (§7; [IFC-NEU-004]).
///
/// Whoever constructs the transport wraps the transport's own configuration type with
/// [`TransportConfiguration::new`]. Only the transport that defined that type unwraps it,
/// with [`TransportConfiguration::into_inner`]; the core never does. The `Debug` form does
/// not show the inside.
pub struct TransportConfiguration(Box<dyn Any + Send>);

impl TransportConfiguration {
    /// Wrap a transport's own configuration value.
    pub fn new<T: Any + Send>(value: T) -> TransportConfiguration {
        TransportConfiguration(Box::new(value))
    }

    /// For the transport that defined `T` only: the value, if it is a `T`; otherwise the
    /// configuration back, unchanged.
    pub fn into_inner<T: Any>(self) -> Result<T, TransportConfiguration> {
        // `Box<dyn Any + Send>::downcast` is the only way back to `T`.
        match self.0.downcast::<T>() {
            Ok(v) => Ok(*v),
            Err(b) => Err(TransportConfiguration(b)),
        }
    }
}

impl fmt::Debug for TransportConfiguration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TransportConfiguration(..)")
    }
}

/// A handler that takes `Inbound` values (`subscribe`, Table 6.4). A transport calls it on
/// its own initiative ([IFC-TRN-040]).
pub type InboundHandler = Arc<dyn Fn(Inbound) + Send + Sync>;

/// A handler that takes presence events (`watch_presence`, Table 6.4; §6.6). A transport
/// calls it on its own initiative ([IFC-TRN-060]).
pub type PresenceHandler = Arc<dyn Fn(PresenceEvent) + Send + Sync>;

/// The output of `subscribe`: a subscription that the core can end (Table 6.4).
///
/// [`Subscription::end`] ends it, and so does dropping it. Once ending returns, the
/// transport does not call the subscription's handler again.
pub struct Subscription {
    end: Option<Box<dyn FnOnce() + Send>>,
}

impl Subscription {
    /// A subscription that `end` ends. For a transport implementation.
    pub fn new(end: impl FnOnce() + Send + 'static) -> Subscription {
        Subscription {
            end: Some(Box::new(end)),
        }
    }

    /// End the subscription ([IFC-TRN-042] says when the core does).
    pub fn end(mut self) {
        self.finish();
    }

    fn finish(&mut self) {
        if let Some(end) = self.end.take() {
            end();
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.finish();
    }
}

impl fmt::Debug for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subscription")
            .field("active", &self.end.is_some())
            .finish()
    }
}

/// Why a transport refused an operation. `spec/interfaces.md` §1.3 leaves error types to
/// the language binding; these cover an operation called out of order or with inputs the
/// contract does not allow. None of them depends on another implementation's subscriptions
/// ([IFC-TRN-043]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransportError {
    /// The operation needs a started transport, and this one is not started, or has shut
    /// down.
    NotStarted,
    /// `start` was called on a transport that is already started.
    AlreadyStarted,
    /// `start` was given a configuration the transport does not recognize or cannot use.
    InvalidConfiguration(String),
    /// `subscribe` was given a `device` destination other than the local device (Table 6.4:
    /// "a `Destination` naming a local session or the local device").
    NotLocalDevice,
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::NotStarted => f.write_str("transport not started"),
            TransportError::AlreadyStarted => f.write_str("transport already started"),
            TransportError::InvalidConfiguration(why) => {
                write!(f, "invalid transport configuration: {why}")
            }
            TransportError::NotLocalDevice => {
                f.write_str("a device subscription must name the local device")
            }
        }
    }
}

impl std::error::Error for TransportError {}

/// `Transport` of `spec/interfaces.md` Table 6.4. The core calls every operation.
///
/// An implementation satisfies each requirement that Appendix C of `spec/interfaces.md`
/// assigns to the `transport` ([IFC-TRN-003]), and has a transport binding document
/// ([IFC-TRN-090]).
pub trait Transport: Send + Sync {
    /// `start`: begin carrying payloads for the local device `local_device`, a public key
    /// id ([IFC-TRN-025]), and return the capability declaration ([IFC-TRN-020]).
    fn start(
        &self,
        local_device: &KeyId,
        configuration: TransportConfiguration,
    ) -> Result<TransportCapabilities, TransportError>;

    /// `publish`: carry a payload of kind `envelope` or `receipt`, or of kind `sealed` on a
    /// sealing transport (`spec/interfaces.md` §6.10), to `destination`, with
    /// no copy delivered at or after `deadline`. The result never depends on whether the
    /// destination is subscribed ([IFC-TRN-044]).
    fn publish(
        &self,
        destination: &Destination,
        payload: Payload,
        deadline: Deadline,
    ) -> PublishResult;

    /// `subscribe`: hand each inbound payload whose destination is `destination` (a local
    /// session or the local device) to `handler`, on the transport's own initiative
    /// ([IFC-TRN-040]).
    fn subscribe(
        &self,
        destination: &Destination,
        handler: InboundHandler,
    ) -> Result<Subscription, TransportError>;

    /// `send_presence`: carry an authenticated presence record, as one whole payload, to
    /// the device `destination` names ([IFC-TRN-050]).
    fn send_presence(
        &self,
        destination: &Destination,
        payload: Payload,
        deadline: Deadline,
    ) -> PublishResult;

    /// `watch_presence`: hand each presence record received, and each carrier loss
    /// reported, to `handler` on the transport's own initiative ([IFC-TRN-060]).
    fn watch_presence(&self, handler: PresenceHandler) -> Result<(), TransportError>;

    /// `health` (§6.8; [IFC-TYP-092]).
    fn health(&self) -> HealthStatus;

    /// `shutdown`. After it returns, the transport invokes no handler ([IFC-TRN-071]).
    fn shutdown(&self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn session() -> SessionId {
        SessionId::from_random_octets([7; 16])
    }

    fn key() -> KeyId {
        KeyId::parse(&"ab".repeat(32)).unwrap()
    }

    #[test]
    fn destination_is_a_session_id_or_a_key_id() {
        // [IFC-TYP-095]: the only two forms; each matches exactly one variant.
        let d = [Destination::Session(session()), Destination::Device(key())];
        for x in &d {
            let n = match x {
                Destination::Session(s) => s.as_str().len(),
                Destination::Device(k) => k.as_str().len(),
            };
            assert!(n == 26 || n == 64);
        }
        assert_ne!(d[0], d[1]);
    }

    #[test]
    fn payload_kinds_fit_table_6_1() {
        let s = Destination::Session(session());
        let k = Destination::Device(key());
        assert!(PayloadKind::Envelope.fits(&s));
        assert!(!PayloadKind::Envelope.fits(&k));
        assert!(PayloadKind::Presence.fits(&k));
        assert!(!PayloadKind::Presence.fits(&s));
        assert!(PayloadKind::Receipt.fits(&k));
        assert!(!PayloadKind::Receipt.fits(&s));
        assert!(PayloadKind::Sealed.fits(&k));
        assert!(!PayloadKind::Sealed.fits(&s));
        assert_eq!(PayloadKind::Sealed.as_str(), "sealed");
        assert_eq!(PayloadKind::Envelope.as_str(), "envelope");
        assert_eq!(PayloadKind::Presence.as_str(), "presence");
        assert_eq!(PayloadKind::Receipt.as_str(), "receipt");
    }

    #[test]
    fn payload_keeps_its_octets_and_debug_hides_them() {
        let p = Payload::new(PayloadKind::Envelope, b"secret-content".to_vec());
        assert_eq!(p.octets(), b"secret-content");
        assert_eq!(p.len(), 14);
        assert!(!p.is_empty());
        assert!(!format!("{p:?}").contains("secret"));
    }

    #[test]
    fn deadline_has_passed_at_and_after_its_instant() {
        let t = Instant::now();
        let d = Deadline::at(t + Duration::from_secs(1));
        assert!(!d.has_passed_at(t));
        assert!(d.has_passed_at(t + Duration::from_secs(1)));
        assert!(d.has_passed_at(t + Duration::from_secs(2)));
        assert_eq!(d.instant(), t + Duration::from_secs(1));
    }

    #[test]
    fn carrier_handle_is_opaque() {
        let a = CarrierHandle::from_opaque(vec![1, 2, 3]);
        assert_eq!(a, CarrierHandle::from_opaque(vec![1, 2, 3]));
        assert_ne!(a, CarrierHandle::from_opaque(vec![1, 2, 4]));
        assert_eq!(format!("{a:?}"), "CarrierHandle(..)");
    }

    #[test]
    fn configuration_round_trips_only_to_its_own_type() {
        #[derive(Debug, PartialEq)]
        struct Mine(u8);
        let c = TransportConfiguration::new(Mine(4));
        assert_eq!(format!("{c:?}"), "TransportConfiguration(..)");
        let c = c.into_inner::<String>().unwrap_err();
        assert_eq!(c.into_inner::<Mine>().unwrap(), Mine(4));
    }

    #[test]
    fn declaration_violations() {
        let ok = TransportCapabilities {
            reliability: false,
            persistence: false,
            offline_queueing: false,
            ordering: true,
            multicast_discovery: false,
            routing_federation: false,
            reach: Reach::LocalOnly,
            destination_restricted: false,
            max_payload_octets: MIN_MAX_PAYLOAD_OCTETS,
            sealing: false,
        };
        assert!(ok.contract_violations().is_empty());
        // [IFC-TRN-109]: a sealing transport carries a default-limit envelope in one frame.
        assert_eq!(
            TransportCapabilities {
                sealing: true,
                ..ok
            }
            .contract_violations(),
            ["IFC-TRN-109"]
        );
        assert!(
            TransportCapabilities {
                sealing: true,
                max_payload_octets: MIN_SEALING_MAX_PAYLOAD_OCTETS,
                ..ok
            }
            .contract_violations()
            .is_empty()
        );
        assert_eq!(
            TransportCapabilities {
                persistence: true,
                ..ok
            }
            .contract_violations(),
            ["IFC-TRN-026"]
        );
        assert_eq!(
            TransportCapabilities {
                offline_queueing: true,
                max_payload_octets: 1,
                ..ok
            }
            .contract_violations(),
            ["IFC-TRN-026", "IFC-TRN-023"]
        );
        assert_eq!(Reach::LocalOnly.as_str(), "local-only");
        assert_eq!(Reach::CrossImplementation.as_str(), "cross-implementation");
    }

    #[test]
    fn subscription_ends_once_on_end_or_drop() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let n = Arc::new(AtomicUsize::new(0));
        let m = n.clone();
        Subscription::new(move || {
            m.fetch_add(1, Ordering::SeqCst);
        })
        .end();
        assert_eq!(n.load(Ordering::SeqCst), 1);
        let m = n.clone();
        drop(Subscription::new(move || {
            m.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(n.load(Ordering::SeqCst), 2);
    }
}
