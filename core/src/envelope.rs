// SPDX-License-Identifier: Apache-2.0

//! The message envelope (`spec/session-channels.md` §4) and `ChannelMessage`
//! (`spec/interfaces.md` §4.5): envelope-stage validation of a received envelope
//! (§4, §5.4, §6.1, in the step order of §8.3.2), and building an envelope to send.
//!
//! # The trust partition, by construction (§4.7)
//!
//! - A received envelope's trusted metadata is read from the top-level `security` member
//!   and nowhere else ([SC-ENV-080]). A member named `security` inside a content part is
//!   part of that content part, and no accessor reaches it.
//! - [`SecurityMetadata`] has no public constructor. A received one comes from
//!   [`receive_envelope`]; an outbound envelope's comes from
//!   [`crate::keys::DeviceIdentity::sign_envelope`], which signs the draft with the device
//!   key and fills `security` from the device's own trusted-key-set entry, a fresh
//!   [`Nonce`] and the signature it computed. No content type converts into any of them,
//!   and an [`EnvelopeDraft`] has no member through which a request could add a top-level
//!   `security` member or any other top-level member ([SC-ENV-081]).
//! - Content is [`ContentPart`] values, a type of its own; nothing derives provenance from
//!   it ([SC-ENV-082]).
//! - A top-level member this revision does not define is kept in the envelope, because
//!   the signature covers it ([IFC-TYP-040], [SEC-SIG-011]), but no accessor returns it,
//!   so it is neither rendered nor used as provenance ([SC-ENV-090] to [SC-ENV-092]).
//! - [`ChannelMessage::verified_by`] is empty until security steps 1 and 2 succeed
//!   ([IFC-TYP-041]); only [`crate::signing::authenticate`] sets it.
//!
//! ```compile_fail,E0451
//! // SecurityMetadata cannot be built outside this crate, from content or anything else.
//! let _ = oac_core::envelope::SecurityMetadata { principal: String::new(), key_id: String::new(), nonce: String::new(), signature: String::new() };
//! ```

use crate::base64url::encode;
use crate::canonical::{CanonicalError, SigningDomain, signing_input};
use crate::capabilities::{DEFAULT_MAX_ENVELOPE_OCTETS, MAX_ENVELOPE_OCTETS_LIMIT};
use crate::delivery::{DeliveryState, ErrorCode};
use crate::ids::{
    IMPLEMENTED_VERSION, KeyId, SessionId, Timestamp, Token, Version, is_core_type,
    is_extension_type,
};
use crate::json::{self, Json, JsonNumber, JsonObject};
use std::fmt;

/// The largest `ttl_ms` ([SC-ENV-050]): 24 hours.
pub const MAX_TTL_MS: u64 = 86_400_000;

/// The eleven top-level members §4.2 defines.
pub const DEFINED_MEMBERS: [&str; 11] = [
    "version",
    "id",
    "from",
    "to",
    "conversation_id",
    "reply_to",
    "correlation_id",
    "created_at",
    "ttl_ms",
    "content",
    "security",
];

const SECURITY_MEMBERS: [&str; 4] = ["principal", "key_id", "nonce", "signature"];

/// A `SecurityPrincipal` (`spec/interfaces.md` §4.8): the `principal` and `key_id` of one
/// entry of the implementation's trusted key set ([IFC-TYP-070]).
///
/// Only a trusted-key-set entry produces one
/// ([`crate::trust::TrustedKey::security_principal`]): the constructor is private to this
/// crate, so no caller can mint a principal from an arbitrary token and hand it to
/// [`crate::keys::DeviceIdentity::sign_envelope`] or put it in
/// [`ChannelMessage::verified_by`] ([IFC-TYP-042], [SC-ENV-081]).
///
/// ```compile_fail,E0624
/// use oac_core::ids::{KeyId, Token};
/// let _ = oac_core::envelope::SecurityPrincipal::new(
///     Token::parse("principal-a").unwrap(),
///     KeyId::parse(&"ab".repeat(32)).unwrap(),
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SecurityPrincipal {
    principal: Token,
    key_id: KeyId,
}

impl SecurityPrincipal {
    /// The principal of a trusted-key-set entry. The principal label is an identifier
    /// token ([SEC-KEY-020]).
    pub(crate) fn new(principal: Token, key_id: KeyId) -> SecurityPrincipal {
        SecurityPrincipal { principal, key_id }
    }

    /// The principal label. Alone, it is not an identity ([SEC-AUZ-004]).
    pub fn principal(&self) -> &Token {
        &self.principal
    }

    /// The key id.
    pub fn key_id(&self) -> &KeyId {
        &self.key_id
    }
}

/// A `security.nonce`: the unpadded base64url encoding of 16 octets ([SEC-SIG-003]). The
/// octets come from the caller's cryptographically secure random number generator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nonce(String);

impl Nonce {
    /// The nonce for `octets`.
    pub fn from_octets(octets: [u8; 16]) -> Nonce {
        Nonce(encode(&octets))
    }

    /// The encoded nonce.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A `security.signature`: the unpadded base64url encoding of 64 octets ([SEC-SIG-004]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature(String);

impl Signature {
    /// The signature for `octets`.
    pub fn from_octets(octets: [u8; 64]) -> Signature {
        Signature(encode(&octets))
    }

    /// The encoded signature.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The top-level `security` member of an envelope (§4.6). The values are what the envelope
/// carries; before the security stage verifies them they are claims ([SEC-STG-003],
/// [SC-ENV-083]). Their forms (`spec/security.md` §6.1) are checked by the security stage,
/// not here (§3.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecurityMetadata {
    principal: String,
    key_id: String,
    nonce: String,
    signature: String,
}

impl SecurityMetadata {
    /// `security.principal`.
    pub fn principal(&self) -> &str {
        &self.principal
    }

    /// `security.key_id`.
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// `security.nonce`.
    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    /// `security.signature`.
    pub fn signature(&self) -> &str {
        &self.signature
    }
}

/// A `text` content part (§4.5.1). Untrusted, always (§4.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextPart {
    text: String,
    wire: JsonObject,
}

impl TextPart {
    /// A text part for `text`. `None` when `text` is empty ([SC-ENV-062]) or holds a
    /// Unicode noncharacter, which I-JSON forbids ([SC-ENV-002]).
    pub fn new(text: impl Into<String>) -> Option<TextPart> {
        let text = text.into();
        let ijson = json::is_ijson_string(&text);
        if text.is_empty() || !ijson {
            return None;
        }
        let mut wire = JsonObject::new();
        wire.insert("type", "text".into());
        wire.insert("text", text.as_str().into());
        Some(TextPart { text, wire })
    }

    /// The message text. A member of the part that this revision does not define is not
    /// returned by any accessor ([SC-ENV-063]).
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// A content part whose type is not `text`, which the receiver supports (§4.5.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OtherPart {
    part_type: String,
    wire: JsonObject,
}

impl OtherPart {
    /// The part's `type`.
    pub fn part_type(&self) -> &str {
        &self.part_type
    }

    /// The part as received. Untrusted content.
    pub fn as_json(&self) -> &JsonObject {
        &self.wire
    }
}

/// One element of `content`, in order ([SC-ENV-064]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentPart {
    /// A `text` part.
    Text(TextPart),
    /// A part of another type the receiver supports.
    Other(OtherPart),
}

impl ContentPart {
    /// The part's `type`.
    pub fn part_type(&self) -> &str {
        match self {
            ContentPart::Text(_) => "text",
            ContentPart::Other(o) => &o.part_type,
        }
    }
}

/// The receiver's envelope-stage settings. Both limits are receiver-wide: they do not
/// depend on the addressed session ([SC-RCP-076]).
///
/// The fields are private and every setter checks its value, so the size limit can never
/// fall below the 65536 octets a receiver accepts ([SC-ENV-004]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvelopeLimits {
    supported_majors: Vec<u16>,
    max_envelope_octets: u64,
    part_types: Vec<String>,
}

impl Default for EnvelopeLimits {
    /// Major version 0, the 65536-octet default, `text` only.
    fn default() -> Self {
        EnvelopeLimits {
            supported_majors: vec![IMPLEMENTED_VERSION.major],
            max_envelope_octets: DEFAULT_MAX_ENVELOPE_OCTETS,
            part_types: Vec::new(),
        }
    }
}

impl EnvelopeLimits {
    /// The limits with `majors` as the supported major versions ([SC-VER-001]). `None`
    /// when `majors` is empty: a receiver that supports no major version would refuse
    /// every envelope with `unsupported-version`.
    pub fn with_supported_majors(mut self, majors: Vec<u16>) -> Option<Self> {
        if majors.is_empty() {
            return None;
        }
        self.supported_majors = majors;
        Some(self)
    }

    /// The limits with a receiver-wide size limit of `octets`. `None` below 65536, which
    /// [SC-ENV-004] makes the floor, or above 9007199254740991 (the range of
    /// [SC-RCP-076] and §3.3).
    pub fn with_max_envelope_octets(mut self, octets: u64) -> Option<Self> {
        if !(DEFAULT_MAX_ENVELOPE_OCTETS..=MAX_ENVELOPE_OCTETS_LIMIT).contains(&octets) {
            return None;
        }
        self.max_envelope_octets = octets;
        Some(self)
    }

    /// The limits with `types`, besides `text`, as the part types the receiver supports
    /// for at least one session. `None` when a type is neither a core type nor an
    /// extension type (§4.5.2).
    pub fn with_part_types(mut self, types: Vec<String>) -> Option<Self> {
        if !types
            .iter()
            .all(|t| is_core_type(t) || is_extension_type(t))
        {
            return None;
        }
        self.part_types = types;
        Some(self)
    }

    /// The supported major versions.
    pub fn supported_majors(&self) -> &[u16] {
        &self.supported_majors
    }

    /// The receiver-wide size limit, in octets.
    pub fn max_envelope_octets(&self) -> u64 {
        self.max_envelope_octets
    }

    /// The part types supported besides `text`.
    pub fn part_types(&self) -> &[String] {
        &self.part_types
    }
}

/// Why envelope-stage validation did not pass an envelope: the delivery state (`rejected`
/// or `expired`), the Table 8.3 code ([SC-RCP-070], [SC-RCP-071]), and, for a diagnostic,
/// the requirement that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvelopeRejection {
    /// `rejected` or `expired`.
    pub state: DeliveryState,
    /// The code of the earliest failed step of §8.3.2.
    pub error: ErrorCode,
    /// The requirement id that failed, for a log line. Not sent to a peer.
    pub requirement: &'static str,
}

impl fmt::Display for EnvelopeRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}): {}", self.state, self.error, self.requirement)
    }
}

impl std::error::Error for EnvelopeRejection {}

fn rejected(error: ErrorCode, requirement: &'static str) -> EnvelopeRejection {
    EnvelopeRejection {
        state: DeliveryState::Rejected,
        error,
        requirement,
    }
}

/// An envelope that passed envelope-stage validation, or that this implementation built.
/// It keeps the exact octets and every top-level member ([IFC-TYP-040]); the typed
/// accessors cover the members §4.2 defines ([IFC-TYP-001]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Envelope {
    octets: Vec<u8>,
    wire: JsonObject,
    version: Version,
    id: Token,
    from: SessionId,
    to: SessionId,
    conversation_id: Option<Token>,
    reply_to: Option<Token>,
    correlation_id: Option<Token>,
    created_at: Timestamp,
    ttl_ms: Option<u64>,
    content: Vec<ContentPart>,
    security: SecurityMetadata,
}

impl Envelope {
    /// `version`.
    pub fn version(&self) -> Version {
        self.version
    }
    /// `id`. Unique only per `from` session ([SC-ENV-027]).
    pub fn id(&self) -> &Token {
        &self.id
    }
    /// `from`, as asserted by the sender: not authenticated before the security stage
    /// ([SC-ENV-083]).
    pub fn from(&self) -> &SessionId {
        &self.from
    }
    /// `to`.
    pub fn to(&self) -> &SessionId {
        &self.to
    }
    /// `conversation_id`.
    pub fn conversation_id(&self) -> Option<&Token> {
        self.conversation_id.as_ref()
    }
    /// `reply_to`.
    pub fn reply_to(&self) -> Option<&Token> {
        self.reply_to.as_ref()
    }
    /// `correlation_id`.
    pub fn correlation_id(&self) -> Option<&Token> {
        self.correlation_id.as_ref()
    }
    /// `created_at`, by the sender's clock.
    pub fn created_at(&self) -> &Timestamp {
        &self.created_at
    }
    /// `ttl_ms`.
    pub fn ttl_ms(&self) -> Option<u64> {
        self.ttl_ms
    }
    /// `content`, in order. Untrusted, always (§4.7).
    pub fn content(&self) -> &[ContentPart] {
        &self.content
    }
    /// The top-level `security` member ([SC-ENV-080]).
    pub fn security(&self) -> &SecurityMetadata {
        &self.security
    }

    /// The expiry instant, `created_at` plus `ttl_ms`, in nanoseconds since the epoch;
    /// `None` without `ttl_ms` (§4.9).
    pub fn expiry_unix_nanos(&self) -> Option<i128> {
        self.ttl_ms
            .map(|t| self.created_at.unix_nanos() + i128::from(t) * 1_000_000)
    }

    /// Whether the expiry instant is at or before `now`, the receiver's clock
    /// ([SC-ENV-100]). A receiver that holds an envelope asks again at hand-off
    /// ([SC-ENV-101]).
    pub fn is_expired_at(&self, now: &Timestamp) -> bool {
        self.expiry_unix_nanos()
            .is_some_and(|e| e <= now.unix_nanos())
    }

    /// The envelope's octets: as received, or as built. A retransmission sends these
    /// unchanged ([SC-ENV-102]).
    pub fn octets(&self) -> &[u8] {
        &self.octets
    }

    /// Every top-level member, as received or built, unrecognized ones included.
    pub fn as_json(&self) -> &JsonObject {
        &self.wire
    }

    /// The signing input of `spec/security.md` §6.2 under `oac-envelope-v1`, computed from
    /// the envelope as received, every unrecognized member included ([SEC-SIG-011]).
    ///
    /// # Errors
    ///
    /// [`CanonicalError`] when a number has no canonical form.
    pub fn signing_input(&self) -> Result<Vec<u8>, CanonicalError> {
        signing_input(SigningDomain::Envelope, &self.wire)
    }
}

/// `ChannelMessage` (`spec/interfaces.md` §4.5): one envelope inside an implementation,
/// with the local member `verified_by`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelMessage {
    envelope: Envelope,
    verified_by: Option<SecurityPrincipal>,
}

impl ChannelMessage {
    /// The envelope.
    pub fn envelope(&self) -> &Envelope {
        &self.envelope
    }

    /// The trusted-key-set entry under which security steps 1 and 2 verified the envelope
    /// ([IFC-TYP-042]); `None` until they have ([IFC-TYP-041]). Local: it is never written
    /// into a payload ([IFC-TYP-002]), since only [`Envelope::octets`] is sent.
    pub fn verified_by(&self) -> Option<&SecurityPrincipal> {
        self.verified_by.as_ref()
    }

    /// The envelope, without the local member.
    pub fn into_envelope(self) -> Envelope {
        self.envelope
    }

    /// The message with `verified_by` set. Called only by the security stage, once steps 1
    /// and 2 have succeeded under `entry` ([IFC-TYP-041], [IFC-TYP-042]).
    pub(crate) fn verified(self, entry: SecurityPrincipal) -> ChannelMessage {
        ChannelMessage {
            envelope: self.envelope,
            verified_by: Some(entry),
        }
    }
}

fn token_member(o: &JsonObject, name: &str) -> Result<Option<Token>, EnvelopeRejection> {
    match o.get(name) {
        None => Ok(None),
        Some(v) => v
            .as_str()
            .and_then(Token::parse)
            .map(Some)
            .ok_or(rejected(ErrorCode::MalformedEnvelope, "SC-ENV-010")),
    }
}

/// Envelope-stage validation of a received envelope (§2.3), in the step order of §8.3.2
/// ([SC-RCP-071]): size, encoding and `version`, major version, structure, part types,
/// expiry. `now` is the receiver's clock.
///
/// The minor version plays no part: a higher minor version is validated by this
/// revision's rules ([SC-VER-002], [SC-VER-003]).
///
/// # Errors
///
/// The state and code of the earliest failed step.
pub fn receive_envelope(
    octets: &[u8],
    limits: &EnvelopeLimits,
    now: &Timestamp,
) -> Result<ChannelMessage, EnvelopeRejection> {
    use ErrorCode::MalformedEnvelope as Malformed;
    // Step 1: the receiver-wide size limit ([SC-ENV-004], [SC-RCP-076]).
    if octets.len() as u64 > limits.max_envelope_octets {
        return Err(rejected(ErrorCode::EnvelopeTooLarge, "SC-RCP-076"));
    }
    // Step 2: encoding ([SC-ENV-001], [SC-ENV-002]) and `version` ([SC-ENV-020]).
    let v = json::parse(octets).map_err(|_| rejected(Malformed, "SC-ENV-002"))?;
    let o = v.into_object().ok_or(rejected(Malformed, "SC-ENV-001"))?;
    let version = o
        .get("version")
        .and_then(Json::as_str)
        .and_then(Version::parse)
        .ok_or(rejected(Malformed, "SC-ENV-020"))?;
    // Step 3: the major version ([SC-VER-001]).
    if !limits.supported_majors.contains(&version.major) {
        return Err(rejected(ErrorCode::UnsupportedVersion, "SC-VER-001"));
    }
    // Step 4: every other requirement of §4, §5.4 and §6.1.
    let envelope = structure(octets, o, version)?;
    // Step 5: part types the receiver supports for no session ([SC-ENV-065], [SC-RCP-076]).
    for part in &envelope.content {
        let t = part.part_type();
        if t != "text" && !limits.part_types.iter().any(|x| x == t) {
            return Err(rejected(ErrorCode::UnsupportedContentType, "SC-ENV-065"));
        }
    }
    // Step 6: expiry ([SC-ENV-100]).
    if envelope.is_expired_at(now) {
        return Err(EnvelopeRejection {
            state: DeliveryState::Expired,
            error: ErrorCode::Expired,
            requirement: "SC-ENV-100",
        });
    }
    Ok(ChannelMessage {
        envelope,
        verified_by: None,
    })
}

// Step 4 of §8.3.2. Every failure here is `malformed-envelope`.
fn structure(
    octets: &[u8],
    o: JsonObject,
    version: Version,
) -> Result<Envelope, EnvelopeRejection> {
    use ErrorCode::MalformedEnvelope as Malformed;
    let fail = |req| rejected(Malformed, req);
    if o.iter().any(|(_, v)| v.contains_null()) {
        return Err(fail("SC-ENV-003"));
    }
    for (m, req) in [
        ("id", "SC-ENV-022"),
        ("from", "SC-ENV-025"),
        ("to", "SC-ENV-026"),
        ("created_at", "SC-ENV-040"),
        ("content", "SC-ENV-060"),
        ("security", "SC-ENV-070"),
    ] {
        if !o.contains(m) {
            return Err(fail(req));
        }
    }
    let id = token_member(&o, "id")?.ok_or(fail("SC-ENV-022"))?;
    let from = token_member(&o, "from")?.ok_or(fail("SC-ENV-025"))?;
    let to = token_member(&o, "to")?.ok_or(fail("SC-ENV-026"))?;
    let conversation_id = token_member(&o, "conversation_id")?;
    let reply_to = token_member(&o, "reply_to")?;
    let correlation_id = token_member(&o, "correlation_id")?;
    let from = SessionId::parse(from.as_str()).ok_or(fail("SC-ID-001"))?;
    let to = SessionId::parse(to.as_str()).ok_or(fail("SC-ID-001"))?;
    let created_at = o
        .get("created_at")
        .and_then(Json::as_str)
        .and_then(Timestamp::parse)
        .ok_or(fail("SC-ENV-041"))?;
    let ttl_ms = match o.get("ttl_ms") {
        None => None,
        Some(t) => Some(
            t.as_number()
                .and_then(|n| n.plain_integer_in(1, MAX_TTL_MS))
                .ok_or(fail("SC-ENV-050"))?,
        ),
    };
    let parts = o
        .get("content")
        .and_then(Json::as_array)
        .filter(|a| !a.is_empty())
        .ok_or(fail("SC-ENV-060"))?;
    let mut content = Vec::with_capacity(parts.len());
    for part in parts {
        let p = part.as_object().ok_or(fail("SC-ENV-061"))?;
        let part_type = p
            .get("type")
            .and_then(Json::as_str)
            .ok_or(fail("SC-ENV-061"))?;
        content.push(if part_type == "text" {
            let text = p
                .get("text")
                .and_then(Json::as_str)
                .filter(|t| !t.is_empty())
                .ok_or(fail("SC-ENV-062"))?;
            ContentPart::Text(TextPart {
                text: text.to_owned(),
                wire: p.clone(),
            })
        } else {
            ContentPart::Other(OtherPart {
                part_type: part_type.to_owned(),
                wire: p.clone(),
            })
        });
    }
    let sec = o
        .get("security")
        .and_then(Json::as_object)
        .ok_or(fail("SC-ENV-070"))?;
    if SECURITY_MEMBERS.iter().any(|m| !sec.contains(m)) {
        return Err(fail("SC-ENV-071"));
    }
    let s = |m| {
        sec.get(m)
            .and_then(Json::as_str)
            .map(str::to_owned)
            .ok_or(fail("SC-ENV-072"))
    };
    let security = SecurityMetadata {
        principal: s("principal")?,
        key_id: s("key_id")?,
        nonce: s("nonce")?,
        signature: s("signature")?,
    };
    if sec.names().any(|n| !SECURITY_MEMBERS.contains(&n)) {
        return Err(fail("SC-ENV-073"));
    }
    Ok(Envelope {
        octets: octets.to_vec(),
        wire: o,
        version,
        id,
        from,
        to,
        conversation_id,
        reply_to,
        correlation_id,
        created_at,
        ttl_ms,
        content,
        security,
    })
}

/// An envelope this implementation is about to send, before it is signed. Its members are
/// the header members and `text` content of §4.2; it has no `security` member and no way
/// to add any other top-level member, so nothing from a request or from content reaches
/// the `security` object ([SC-ENV-081]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvelopeDraft {
    version: Version,
    id: Token,
    from: SessionId,
    to: SessionId,
    conversation_id: Option<Token>,
    reply_to: Option<Token>,
    correlation_id: Option<Token>,
    created_at: Timestamp,
    ttl_ms: Option<u64>,
    content: Vec<TextPart>,
}

impl EnvelopeDraft {
    /// A draft with the revision this crate implements as `version` ([SC-ENV-021]).
    /// `None` when `content` is empty ([SC-ENV-060]).
    ///
    /// [SC-ENV-023] and [SC-ENV-024] ask the caller for an `id` never used before from
    /// `from`, generated from a cryptographically secure random number generator.
    pub fn new(
        id: Token,
        from: SessionId,
        to: SessionId,
        created_at: Timestamp,
        content: Vec<TextPart>,
    ) -> Option<EnvelopeDraft> {
        if content.is_empty() {
            return None;
        }
        Some(EnvelopeDraft {
            version: IMPLEMENTED_VERSION,
            id,
            from,
            to,
            conversation_id: None,
            reply_to: None,
            correlation_id: None,
            created_at,
            ttl_ms: None,
            content,
        })
    }

    /// Sets `version` to the revision implemented for the agreed major version
    /// ([SC-ID-087]).
    pub fn with_version(mut self, version: Version) -> Self {
        self.version = version;
        self
    }

    /// Sets `conversation_id` ([SC-ENV-031]).
    pub fn with_conversation_id(mut self, v: Token) -> Self {
        self.conversation_id = Some(v);
        self
    }

    /// Sets `reply_to` ([SC-ENV-031]).
    pub fn with_reply_to(mut self, v: Token) -> Self {
        self.reply_to = Some(v);
        self
    }

    /// Sets `correlation_id` ([SC-ENV-031]).
    pub fn with_correlation_id(mut self, v: Token) -> Self {
        self.correlation_id = Some(v);
        self
    }

    /// Sets `ttl_ms` ([SC-ENV-051]). `None` outside 1 to 86400000 ([SC-ENV-050]).
    pub fn with_ttl_ms(mut self, ttl_ms: u64) -> Option<Self> {
        if !(1..=MAX_TTL_MS).contains(&ttl_ms) {
            return None;
        }
        self.ttl_ms = Some(ttl_ms);
        Some(self)
    }

    fn wire(&self, security: JsonObject) -> JsonObject {
        let mut o = JsonObject::new();
        o.insert("version", self.version.to_string().as_str().into());
        o.insert("id", self.id.as_str().into());
        o.insert("from", self.from.as_str().into());
        o.insert("to", self.to.as_str().into());
        for (name, v) in [
            ("conversation_id", &self.conversation_id),
            ("reply_to", &self.reply_to),
            ("correlation_id", &self.correlation_id),
        ] {
            if let Some(v) = v {
                o.insert(name, v.as_str().into());
            }
        }
        o.insert("created_at", self.created_at.as_str().into());
        if let Some(t) = self.ttl_ms {
            o.insert("ttl_ms", Json::Number(JsonNumber::from_u64(t)));
        }
        o.insert(
            "content",
            Json::Array(
                self.content
                    .iter()
                    .map(|p| Json::Object(p.wire.clone()))
                    .collect(),
            ),
        );
        o.insert("security", Json::Object(security));
        o
    }

    fn security(signer: &SecurityPrincipal, nonce: &Nonce) -> JsonObject {
        let mut s = JsonObject::new();
        s.insert("principal", signer.principal.as_str().into());
        s.insert("key_id", signer.key_id.as_str().into());
        s.insert("nonce", nonce.as_str().into());
        s
    }

    /// The signing input (`spec/security.md` §6.2, domain `oac-envelope-v1`) of the
    /// envelope that [`EnvelopeDraft::seal`] builds with `signer` and `nonce`.
    ///
    /// # Errors
    ///
    /// [`CanonicalError`]; it does not occur for a draft, whose numbers are all integers.
    pub fn signing_input(
        &self,
        signer: &SecurityPrincipal,
        nonce: &Nonce,
    ) -> Result<Vec<u8>, CanonicalError> {
        signing_input(
            SigningDomain::Envelope,
            &self.wire(Self::security(signer, nonce)),
        )
    }

    /// The signed envelope: the draft with `security` set from `signer`, `nonce` and
    /// `signature` ([SEC-SIG-001], [SEC-SIG-002]). It is serialized once, here, and never
    /// changed afterwards ([SEC-SIG-012]). Private to this crate: the one caller is
    /// [`crate::keys::DeviceIdentity::sign_envelope`], so every envelope this crate builds
    /// carries a signature its device key made ([SEC-SIG-010]).
    pub(crate) fn seal(
        self,
        signer: &SecurityPrincipal,
        nonce: Nonce,
        signature: Signature,
    ) -> Envelope {
        let mut sec = Self::security(signer, &nonce);
        sec.insert("signature", signature.as_str().into());
        let wire = self.wire(sec);
        let octets = Json::Object(wire.clone()).to_compact().into_bytes();
        Envelope {
            octets,
            wire,
            version: self.version,
            id: self.id,
            from: self.from,
            to: self.to,
            conversation_id: self.conversation_id,
            reply_to: self.reply_to,
            correlation_id: self.correlation_id,
            created_at: self.created_at,
            ttl_ms: self.ttl_ms,
            content: self.content.into_iter().map(ContentPart::Text).collect(),
            security: SecurityMetadata {
                principal: signer.principal.as_str().to_owned(),
                key_id: signer.key_id.as_str().to_owned(),
                nonce: nonce.0,
                signature: signature.0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> EnvelopeDraft {
        EnvelopeDraft::new(
            Token::parse("msg-1").unwrap(),
            SessionId::parse("01harn7x9k2m4p6q8r0s2t4v6w").unwrap(),
            SessionId::parse("7gq3m8z2c5k9t1w4x6b0n2r8vd").unwrap(),
            Timestamp::parse("2026-10-03T12:00:00.000Z").unwrap(),
            vec![TextPart::new("Hello, \"security\": {}").unwrap()],
        )
        .unwrap()
    }

    fn signer() -> SecurityPrincipal {
        SecurityPrincipal::new(
            Token::parse("principal-a").unwrap(),
            KeyId::parse(&"ab".repeat(32)).unwrap(),
        )
    }

    #[test]
    fn base64url_forms() {
        assert_eq!(
            Nonce::from_octets([0; 16]).as_str(),
            "AAAAAAAAAAAAAAAAAAAAAA"
        );
        assert_eq!(
            Nonce::from_octets([0xff; 16]).as_str(),
            "_____________________w"
        );
        assert_eq!(Signature::from_octets([0xfb; 64]).as_str().len(), 86);
    }

    #[test]
    fn sealed_envelope_passes_the_envelope_stage_unchanged() {
        let d = draft()
            .with_ttl_ms(300_000)
            .unwrap()
            .with_conversation_id(Token::parse("conv-1").unwrap());
        let input = d
            .signing_input(&signer(), &Nonce::from_octets([7; 16]))
            .unwrap();
        let env = d.seal(
            &signer(),
            Nonce::from_octets([7; 16]),
            Signature::from_octets([9; 64]),
        );
        assert_eq!(env.signing_input().unwrap(), input);
        let now = Timestamp::parse("2026-10-03T12:00:01.000Z").unwrap();
        let got = receive_envelope(env.octets(), &EnvelopeLimits::default(), &now).unwrap();
        assert_eq!(got.envelope(), &env);
        assert_eq!(got.verified_by(), None);
        assert_eq!(got.envelope().security().principal(), "principal-a");
    }

    #[test]
    fn drafts_refuse_invalid_values() {
        assert!(draft().with_ttl_ms(0).is_none());
        assert!(draft().with_ttl_ms(MAX_TTL_MS + 1).is_none());
        assert!(TextPart::new("").is_none());
        assert!(TextPart::new("\u{ffff}").is_none());
    }

    #[test]
    fn expiry_is_at_or_before_now() {
        let env = draft().with_ttl_ms(1000).unwrap().seal(
            &signer(),
            Nonce::from_octets([0; 16]),
            Signature::from_octets([0; 64]),
        );
        assert!(!env.is_expired_at(&Timestamp::parse("2026-10-03T12:00:00.999999999Z").unwrap()));
        assert!(env.is_expired_at(&Timestamp::parse("2026-10-03T12:00:01Z").unwrap()));
    }

    #[test]
    fn deep_unrecognized_member_within_the_size_limit_is_accepted() {
        // [SC-ENV-004], [SC-ENV-090]: an envelope of at most 65536 octets whose only oddity
        // is a deeply nested unrecognized member passes the envelope stage, and every path
        // over it (signing input, copy, comparison, rewrite, drop) works on a test
        // thread's default stack.
        let head = r#"{"version":"0.1","id":"msg-1","from":"01harn7x9k2m4p6q8r0s2t4v6w","to":"7gq3m8z2c5k9t1w4x6b0n2r8vd","created_at":"2026-10-03T12:00:00.000Z","content":[{"type":"text","text":"Hi."}],"security":{"principal":"p","key_id":"k","nonce":"n","signature":"s"},"x-deep":"#;
        let depth = (65_536 - head.len() - 1) / 2;
        let deep = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        let text = format!("{head}{deep}}}");
        assert!(text.len() <= 65_536 && text.len() > 65_000);
        let now = Timestamp::parse("2026-10-03T12:00:01.000Z").unwrap();
        let msg = receive_envelope(text.as_bytes(), &EnvelopeLimits::default(), &now).unwrap();
        let env = msg.envelope();
        assert_eq!(env.octets(), text.as_bytes());
        let input = env.signing_input().unwrap();
        assert!(input.ends_with(format!("\"x-deep\":{deep}}}").as_bytes()));
        let copy = msg.clone();
        assert_eq!(copy, msg);
        assert_eq!(Json::Object(env.as_json().clone()).to_compact(), text);
        drop(copy);
    }

    #[test]
    fn limits_never_fall_below_the_default() {
        let l = EnvelopeLimits::default();
        assert_eq!(l.max_envelope_octets(), 65_536);
        assert!(l.clone().with_max_envelope_octets(65_535).is_none());
        assert!(
            l.clone()
                .with_max_envelope_octets(9_007_199_254_740_992)
                .is_none()
        );
        // #52, PR #312 review R2-2: a receiver supports at least one major version.
        assert!(l.clone().with_supported_majors(vec![]).is_none());
        assert_eq!(
            l.clone()
                .with_supported_majors(vec![0, 1])
                .unwrap()
                .supported_majors(),
            &[0, 1]
        );
        let raised = l.clone().with_max_envelope_octets(131_072).unwrap();
        assert_eq!(raised.max_envelope_octets(), 131_072);
        assert!(l.clone().with_part_types(vec!["Text".into()]).is_none());
        assert!(l.with_part_types(vec!["com.example/rich".into()]).is_some());
    }
}
