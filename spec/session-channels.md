# OAC Session Channels

**Document:** `spec/session-channels.md`, the normative OAC Session Channels specification.
**Revision:** 0.1 (draft, Stage 2). Sections 4 and 5 are written (E1, #41), section 6
(E2, #42) and section 7 (E3, #43). Section 8 is a titled stub that task E4 (#44) fills.
**Companion document:** `spec/security.md` (task E5, #45) holds the identity hierarchy,
signing, replay defence, authorization and provenance rules. This document does not restate
them.

This document is normative-of-record for the OAC Session Channels protocol
(`docs/planning/decisions/C3-spec-packaging.md` §1). A binding document that carries the
protocol over a particular integration surface (task E6, #46) is packaging. Where a binding
document and this document disagree, this document governs.

---

## 1. Introduction

OAC Session Channels lets a live session in one harness send a message into a live session
in another harness. This document defines the message format and the protocol rules that
every conformant implementation follows, independent of any harness and of any transport.

This document is deliberately neutral. It names no harness product, no transport product and
no binding-specific method or key. A harness-specific or transport-specific mapping belongs
in an adapter, a transport module or a binding document, never here.

### 1.1 Scope of this document

| Section | Subject | Owner |
|---|---|---|
| 4 | Message envelope and content model | E1 (#41) |
| 5 | Versioning and compatibility | E1 (#41) |
| 6 | Session identity, addressing, and capability negotiation | E2 (#42) |
| 7 | Active delivery, presence, and discovery | E3 (#43) |
| 8 | Delivery receipts, replies, correlation, and errors | E4 (#44) |

Security (identity hierarchy, signing, replay, authorization, provenance) is specified in
`spec/security.md` (E5, #45). Section 9 of this document only points to it.

---

## 2. Conventions and terminology

### 2.1 Requirement keywords

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT",
"RECOMMENDED", "NOT RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be
interpreted as described in BCP 14 [RFC2119] [RFC8174] when, and only when, they appear in
all capitals, as shown here.

This document uses only `MUST`, `MUST NOT`, `SHOULD`, `SHOULD NOT` and `MAY`. Each sentence
carrying one of them states one requirement and starts with a requirement id (§3.2).

- A `SHOULD` or `SHOULD NOT` sentence is followed by what a deviation looks like.
- A `MAY` sentence is followed by what a peer that does not take the option still does.

### 2.2 Reference implementation notes

A paragraph labelled as below is informative. It describes a choice of the v0.1 reference
implementation and binds no other implementation:

> **Reference implementation note:** an implementation choice, not binding on other
> conformant implementations.

### 2.3 Terminology

- **Harness:** an existing program that hosts model-driven sessions and owns their
  inference, context, tools and credentials.
- **Session:** one live conversation inside a harness, addressed by an OAC session id
  (§6.1).
- **Device:** one installation of an OAC implementation, holding one device key
  (`spec/security.md`).
- **Implementation:** software that claims conformance to this document.
- **Sender:** the implementation that creates and signs an envelope.
- **Receiver:** the implementation that accepts an envelope from a transport and hands it
  to the addressed session's harness.
- **Hand-off:** the moment a receiver passes an envelope's content to the harness through
  the harness's own supported input surface. What a receiver reports about hand-off is
  defined in §8.1.
- **Envelope:** the message unit defined in §4.
- **Envelope-stage validation:** the checks of §4 and §5 that a receiver applies to an
  envelope's structure, syntax, version and expiry. Signature verification, replay defence
  and authorization are separate stages defined in `spec/security.md`.
- **Identifier token:** a string that satisfies §4.3.

---

## 3. Conformance

### 3.1 What conformance means

An implementation conforms to this document when it satisfies every `MUST` and `MUST NOT`
that applies to its role (sender, receiver, or both). Conformance is demonstrated against
the conformance fixtures of §3.3. A requirement marked `TODO(fixture)` in Appendix A has no
fixture yet. It is stated, but no fixture checks it yet.

### 3.2 Requirement ids

Every normative sentence carries a stable requirement id in square brackets at its start,
for example `[SC-ENV-001]`.

- **Form:** `<DOC>-<AREA>-<NNN>`.
  - `<DOC>` names the specification document: `SC` for this document, `SEC` for
    `spec/security.md`. A binding document (E6) registers its own `<DOC>` prefix in its
    own text.
  - `<AREA>` is upper-case letters naming a section's subject. This document's areas are:
    `ENV` (§4), `VER` (§5), `ID` (§6), `DLV` (§7) and `RCP` (§8). Each area is owned by the
    task that owns its section.
  - `<NNN>` is three decimal digits. The owner of an area allocates numbers in increasing
    order, and gaps are allowed.
- **Stability:** an id names exactly one requirement for the life of the document.
  - An id is never renumbered and never reused.
  - A requirement whose normative meaning changes receives a new id. Its old id is
    retired.
  - A retired id stays listed in Appendix A with the revision that retired it.
  - An editorial change that leaves the normative meaning unchanged keeps the id.
- **Ids and versioning:** retiring an id or adding one is not itself a version event.
  Whether the underlying change needs a new minor or a new major version is decided by §5
  alone.

### 3.3 Conformance fixtures

A conformance fixture is a data file, not code. Each fixture proves exactly one
requirement and is readable by any independent implementation. Fixtures live under
`tests/protocol/`, in one directory per requirement area, named
`tests/protocol/<doc>-<area>/<REQUIREMENT-ID>.<p|n><NN>-<slug>.json`, where `p` marks a
positive fixture and `n` a negative one. A conformance runner (Stage 3, tasks E8 and F12)
executes them.

A fixture is a JSON object with these members:

| Member | Content |
|---|---|
| `fixture_format` | The string `oac-conformance-fixture/1`. |
| `requirement` | The requirement id the fixture proves (§3.2). |
| `spec` | The path of the document that holds the requirement. |
| `spec_revision` | The document revision the fixture was written against, as `<major>.<minor>`. |
| `kind` | `positive` or `negative`. |
| `failure_mode` | Negative fixtures only: a short phrase naming the failure the fixture exercises. |
| `description` | One or two sentences, for a human reader. |
| `stage` | The validation stage the fixture exercises. Section 4 and 5 fixtures use `envelope` (envelope-stage validation, §2.3). |
| `context` | The receiver's state for the test: `receiver_time` (a timestamp in the §4.4.6 form) and `supported_major_versions` (an array of integers). |
| `input` | Exactly one of: `envelope` (the envelope as a JSON value), `envelope_text` (the exact serialized text, for inputs that no JSON value expresses, such as duplicate member names), or `envelope_base64` (the exact octets, base64 per [RFC4648] §4, for inputs that are not valid UTF-8). |
| `expected` | `result` (one of `valid`, `rejected`, `expired`), and optionally `trusted_security` (the `security` member values a receiver extracts, §4.7). |

The `context`, `input` and `expected` rows above define the `envelope` stage. A section of
this document can define further stages, each with its own `context`, `input` and
`expected` members, as §6.10 does.

An envelope-stage fixture's `security` members hold placeholder strings. Envelope-stage
validation checks their presence and type (§4.6), never their values. Signature vectors and
replay cases are `spec/security.md` fixtures (E5, E8).

The `result` values `rejected` and `expired` are the delivery states that §8.1 (E4) defines.
When E4 lands the closed error taxonomy (§8.3), each negative fixture gains an
`expected.error` member naming the error it emits.

> **Reference implementation note:** the v0.1 reference workspace runs these fixtures from
> its conformance runner (task E8, wired into CI by task F12). The fixture set in this
> revision covers sections 4 and 5. Task E8 extends it to every section.

---

## 4. Message envelope

*Owned by E1 (#41).*

### 4.1 Encoding

[SC-ENV-001] An envelope MUST be a JSON object [RFC8259] at the top level.

[SC-ENV-002] An envelope MUST be an I-JSON message [RFC7493].

This incorporates I-JSON's rules: UTF-8 encoding (RFC 7493 §2.1), no surrogate or
noncharacter code points in member names or string values (§2.1), and no duplicate member
names in any object (§2.3). The signing procedure in `spec/security.md` canonicalizes the
envelope, and that canonicalization is defined only over I-JSON input
(`docs/planning/decisions/C5-envelope-auth.md` §3).

[SC-ENV-003] An envelope MUST NOT contain a member whose value is JSON `null`.

An absent optional member is represented only by omitting it. This keeps exactly one
encoding for "absent", so a signed envelope has one meaning.

[SC-ENV-004] A receiver SHOULD accept an envelope whose serialized form is at most 65536
octets. A receiver that rejects an envelope at or under that size, on size alone, deviates.

[SC-ENV-005] A sender SHOULD NOT send an envelope whose serialized form exceeds 65536
octets unless the addressed session has advertised a larger limit (§6.4). A sender that
sends a larger envelope without that advertisement deviates. Such an envelope is liable
to be rejected.

*Dated note, 2026-10-03 (#41): the 65536-octet default in [SC-ENV-004] and [SC-ENV-005] is
an operator decision recorded on #41.*

### 4.2 Members

The envelope has eleven defined top-level members. The table is a summary. The
requirements in §4.3 to §4.9 govern.

| Member | Type | Presence | Class (§4.7) | Defined in |
|---|---|---|---|---|
| `version` | string, `<major>.<minor>` | exactly once | header | §4.4.1, §5 |
| `id` | identifier token | exactly once | header | §4.4.2 |
| `from` | identifier token, session id | exactly once | header | §4.4.3 |
| `to` | identifier token, session id | exactly once | header | §4.4.3 |
| `conversation_id` | identifier token | zero or once | header | §4.4.4 |
| `reply_to` | identifier token | zero or once | header | §4.4.5 |
| `correlation_id` | identifier token | zero or once | header | §4.4.5 |
| `created_at` | string, timestamp | exactly once | header | §4.4.6 |
| `ttl_ms` | integer, 1 to 86400000 | zero or once | header | §4.4.7, §4.9 |
| `content` | array of content parts, at least one | exactly once | content | §4.5 |
| `security` | object, four string members | exactly once | security | §4.6 |

Example (informative; the `security` values are placeholders, `spec/security.md` defines
their real form):

```json
{
  "version": "0.1",
  "id": "msg-01hzx3k8q2v6w9m4t7p0r5s1ya",
  "from": "01harn7x9k2m4p6q8r0s2t4v6w",
  "to": "7gq3m8z2c5k9t1w4x6b0n2r8vd",
  "conversation_id": "conv-4d2a",
  "created_at": "2026-10-03T12:00:00.000Z",
  "ttl_ms": 300000,
  "content": [{ "type": "text", "text": "Please review the scheduler module." }],
  "security": {
    "principal": "placeholder-principal",
    "key_id": "placeholder-key-id",
    "nonce": "placeholder-nonce",
    "signature": "placeholder-signature"
  }
}
```

### 4.3 Identifier tokens

An identifier token is a JSON string whose entire decoded value matches the pattern
`[A-Za-z0-9._:-]{1,128}`: one to 128 characters, each an ASCII letter, an ASCII digit, `.`,
`_`, `:` or `-`.

[SC-ENV-010] Each of `id`, `from`, `to`, `conversation_id`, `reply_to` and
`correlation_id`, when present, MUST be an identifier token.

[SC-ENV-011] A receiver MUST test the identifier-token pattern against the whole decoded
string value, without trimming, case-folding or any other normalization.

A match anchored at line boundaries is not a whole-value match. In some regular-expression
engines `$` also matches before a trailing line feed, and in others `^` and `$` are always
line anchors. An implementation uses a whole-string match instead (for example `\A…\z`, or
a full-match API).

The token grammar exists so that an adapter carries these values next to a harness's
input as machine-set provenance without any value being able to start a new line or forge a
field. It adopts, as an envelope rule, the whole-value validation that decision C13 (§4,
"F1") and `docs/planning/decisions/C6-trust-rendering.md` §5.0 step 1 required of an
adapter until this section fixed the format. A value that fails the grammar never reaches
an adapter, because the receiver rejects the envelope first.

### 4.4 Header members

#### 4.4.1 `version`

[SC-ENV-020] An envelope MUST contain a `version` member whose value is a string matching
`(0|[1-9][0-9]{0,3})\.(0|[1-9][0-9]{0,3})` as a whole value.

The two numbers are the major and minor version of the revision of this document whose
rules the envelope follows (§5). Leading zeros are not allowed. The value is a string, never
a JSON number, so `"0.10"` and `"0.1"` stay distinct.

[SC-ENV-021] A sender MUST set `version` to the major and minor version of the revision of
this document that it implements.

#### 4.4.2 `id`

`id` identifies one envelope. Its reply and correlation semantics are defined in §8.2.

[SC-ENV-022] An envelope MUST contain an `id` member.

[SC-ENV-023] A sender MUST give every envelope it creates an `id` it has not used on any
earlier envelope sent from the same `from` session.

[SC-ENV-027] A receiver MUST NOT treat `id` as unique across envelopes from different
`from` sessions.

Duplicate suppression keys on values defined in `spec/security.md`, not on `id`
(`docs/planning/decisions/C5-envelope-auth.md` §7).

[SC-ENV-024] A sender SHOULD generate each `id` with at least 122 bits of output from a
cryptographically secure random number generator. A sender whose ids are sequential or
otherwise predictable deviates: a third party that observes one id predicts the next.

#### 4.4.3 `from` and `to`

`from` is the OAC session id of the sending session. `to` is the OAC session id of the one
addressed session. Section 6.1 (E2) defines the session id's syntax. Every session id that
§6.1 permits is also an identifier token (§4.3).

[SC-ENV-025] An envelope MUST contain a `from` member.

[SC-ENV-026] An envelope MUST contain a `to` member.

`to` names exactly one session. Group addressing and broadcast are out of scope for this
revision (`docs/planning/ADR-001.md`, "v0.1 scope"). A `to` value that is an array or any
other non-string value fails [SC-ENV-010].

`from` is asserted by the sender. A receiver treats it as authenticated only after the
verification steps of `spec/security.md` succeed.

#### 4.4.4 `conversation_id`

`conversation_id` groups related envelopes into one conversation. Its semantics are
defined in §8.2 (E4).

#### 4.4.5 `reply_to` and `correlation_id`

`reply_to` names the `id` of an envelope that this envelope answers. `correlation_id`
carries a sender-chosen value that ties a request to its replies. Their semantics are
defined in §8.2 (E4).

[SC-ENV-030] A receiver MUST NOT treat the absence of `conversation_id`, `reply_to`,
`correlation_id` or `ttl_ms` as making an envelope invalid.

[SC-ENV-031] A sender MAY include any of `conversation_id`, `reply_to` and
`correlation_id`. A sender that omits them still sends a valid envelope.

#### 4.4.6 `created_at`

`created_at` is the instant the sender created the envelope, by the sender's clock.

[SC-ENV-040] An envelope MUST contain a `created_at` member.

[SC-ENV-041] The value of `created_at` MUST be a string in the timestamp form defined
below.

The timestamp form is the `date-time` production of [RFC3339] §5.6, restricted as follows:

- the separator between date and time is upper-case `T`;
- the offset is upper-case `Z` (UTC); a numeric offset is not allowed;
- `time-secfrac`, when present, has one to nine digits;
- `time-second` is `00` to `59` (a leap second, `60`, is not allowed);
- the date is a real calendar date (RFC 3339 §5.7).

Example: `2026-10-03T12:00:00.000Z`.

#### 4.4.7 `ttl_ms`

`ttl_ms` is the sender's declared validity period for the envelope, in milliseconds,
counted from `created_at`. Section 4.9 defines its effect.

[SC-ENV-050] When `ttl_ms` is present, its value MUST be an integer from 1 to 86400000
inclusive, written as a JSON number with no sign, fraction or exponent.

[SC-ENV-051] A sender MAY include `ttl_ms`. A sender that omits it declares no expiry of
its own, and the receiver still applies the replay window of `spec/security.md`.

The upper bound is 24 hours. This revision defines live delivery only. Durable offline
mailboxes are out of scope (`docs/planning/ADR-001.md`, "v0.1 scope"), so a longer
validity period has no meaning. The bound also keeps `created_at + ttl_ms` exact in
every common number representation.

*Dated note, 2026-10-03 (#41): the 24-hour cap is an operator decision recorded on #41.*

### 4.5 Content model

`content` carries the message body. It is the only member of an envelope that carries
data a user or a model chose (§4.7).

[SC-ENV-060] An envelope MUST contain a `content` member whose value is an array with at
least one element.

[SC-ENV-061] Each element of `content` MUST be a JSON object with a `type` member whose
value is a string.

Each element is a **content part**. Parts are ordered. The message body is the parts in
array order.

#### 4.5.1 The `text` part

This revision defines exactly one part type, `text`.

[SC-ENV-062] A part whose `type` is `text` MUST have a `text` member whose value is a
string of at least one character.

The string is the message text, as Unicode. It is untrusted (§4.7). An adapter renders it to
a harness as opaque text, as `spec/security.md` and the adapter's own rendering rules
require. This document places no other restriction on its characters beyond §4.1.

[SC-ENV-063] A receiver MUST ignore a member of a `text` part that this revision does not
define.

Ignoring means the member is neither rendered to the harness nor used for any decision.
This keeps a later minor revision free to add an optional member to the `text` part (§5.2).

[SC-ENV-064] A receiver MUST preserve the order of `text` parts when it hands the message
body to a harness.

#### 4.5.2 Extension point for other part types

A `type` value is either a **core type** or an **extension type**.

- A core type matches `[a-z][a-z0-9-]{0,31}`. Core types are reserved for this document.
  The only core type in this revision is `text`.
- An extension type has the form `{reverse-dns-prefix}/{name}`. The prefix is a reversed
  domain name that the type's author owns or controls. `name` matches
  `[a-z][a-z0-9-]{0,63}`.

[SC-ENV-065] A receiver MUST reject an envelope that contains a content part whose `type`
the receiver does not support.

A part type is not ignorable. Silently dropping a part would hand the harness a different
message from the one the sender signed. Rejection makes the mismatch visible instead
(§8.3, E4).

*Dated note, 2026-10-03 (#41): rejecting the whole envelope, rather than dropping the
unsupported part, is an operator decision recorded on #41. It supersedes the "ignore the
entry" rule of the M0 draft, `docs/planning/v0.1/05-interfaces.md` §3.*

[SC-ENV-066] A sender MUST NOT send a content part whose `type` is anything other than
`text` unless the addressed session has advertised support for that type (§6.4).

This extension point is a typed slot for future text-like content. It does not define,
reserve or imply attachments, binary payloads, file transfer or references to external
resources. Attachments are out of scope for this revision (`docs/planning/ADR-001.md`,
"v0.1 scope"). A future part type that carried any of these is a new capability, negotiated
under §6.4, and would need its own security analysis in `spec/security.md`.

### 4.6 Security metadata

`security` carries trusted metadata that the sender's implementation sets: who signed the
envelope, with which key, the replay nonce, and the signature. `spec/security.md` (E5)
defines each member's syntax, how it is produced and how it is verified. This section
defines only the member's structure.

[SC-ENV-070] An envelope MUST contain a `security` member whose value is a JSON object.

[SC-ENV-071] The `security` object MUST contain the members `principal`, `key_id`, `nonce`
and `signature`.

[SC-ENV-072] Each of `principal`, `key_id`, `nonce` and `signature` MUST be a string.

[SC-ENV-073] The `security` object MUST NOT contain any member other than `principal`,
`key_id`, `nonce` and `signature`.

The `security` object is closed, unlike the top level (§4.8). A verifier relies on every
security member, so a member it did not understand could not be relied on. Adding a member
is a breaking change (§5.3).

### 4.7 Trust partition

Every defined top-level member belongs to exactly one class. The class decides who sets the
value and how far a receiver trusts it.

| Class | Members | Set by | Trust |
|---|---|---|---|
| security | `security` | the sender's implementation only, never from content or from user or model input | trusted once verified per `spec/security.md` |
| header | `version`, `id`, `from`, `to`, `conversation_id`, `reply_to`, `correlation_id`, `created_at`, `ttl_ms` | the sender's implementation; `reply_to` and `correlation_id` often echo values a peer chose | covered by the signature; syntax-constrained (§4.3); never instructions |
| content | `content` | a user or a model, through the sender's harness | untrusted, always, even from an authenticated sender |

The classes are distinguishable by structure alone: trusted metadata sits only in the
top-level `security` object, and user- or model-controlled data sits only in `content`. No
parser has to inspect a string to tell them apart.

[SC-ENV-080] A receiver MUST take trusted security metadata only from the top-level
`security` member.

A member named `security`, or anything shaped like one, inside `content` is content.

[SC-ENV-081] A sending implementation MUST NOT set any `security` member from `content`,
or from any other value that a user or a model supplied.

[SC-ENV-082] A receiver MUST NOT derive the sender's identity, device, session or any other
provenance from `content`.

[SC-ENV-083] A receiver MUST NOT treat a header member as authenticated before the
verification steps of `spec/security.md` have succeeded for the envelope.

An authenticated envelope is still untrusted instruction content, and it is a possible carrier of prompt
injection. Authentication answers who sent a message, never whether to obey it
(`docs/planning/ADR-001.md`, "Security model"; `spec/security.md`).

### 4.8 Unrecognized top-level members

[SC-ENV-090] A receiver MUST ignore a top-level member that the revision it implements
does not define.

[SC-ENV-091] A receiver MUST NOT render an unrecognized top-level member to a harness.

[SC-ENV-092] A receiver MUST NOT use an unrecognized top-level member as provenance.

An unrecognized member is still inside the signed scope. The signature covers every
member except the signature itself (`docs/planning/decisions/C5-envelope-auth.md` §5), so
`spec/security.md` verifies the envelope as received, with its unrecognized members in
place.

### 4.9 Expiry

An envelope with `ttl_ms` has an **expiry instant**: `created_at` plus `ttl_ms`
milliseconds. An envelope without `ttl_ms` has no sender-declared expiry instant.

[SC-ENV-100] A receiver MUST NOT hand off an envelope whose expiry instant is at or before
the receiver's current time.

A receiver that does not hand off an envelope for this reason reports the `expired`
delivery state (§8.1, E4).

[SC-ENV-101] A receiver that holds an envelope before hand-off MUST evaluate the expiry
rule of [SC-ENV-100] again at the moment of hand-off.

Expiry ends at hand-off. Once the harness has the content, expiry has no further effect: no
expiry rule recalls, withdraws or hides content a harness already holds.

The expiry instant is computed from the sender's `created_at`. It is a separate check from
the replay window of `spec/security.md`, which bounds `created_at` against the receiver's
own clock whatever `ttl_ms` says (`docs/planning/decisions/C5-envelope-auth.md` §7). An
envelope that passes one check is still subject to the other.

A **retransmission** resends an envelope that was already signed, octet for octet: the same
`id`, `created_at`, `ttl_ms`, `security.nonce` and signature. A **retry** sends the same
message content again as a new envelope. A receiver that already accepted the original
treats a retransmission as a duplicate (`spec/security.md`).

[SC-ENV-102] A sender that retransmits an envelope MUST send it unchanged.

[SC-ENV-103] A sender that retries a message MUST give the new envelope a new `id`.

[SC-ENV-104] A sender that retries a message MUST give the new envelope a new
`security.nonce`.

Requirements [SC-ENV-103] and [SC-ENV-104] follow
`docs/planning/decisions/C5-envelope-auth.md` §7: "a retry is a new envelope with a new
`id` and a new `nonce`". A sender that wants a new validity period retries. Every member
of the envelope is inside the signed scope, so a change by an intermediary makes the
signature fail.

---

## 5. Versioning and compatibility

*Owned by E1 (#41). Negotiation of versions and extensions between peers is §6.5 (E2).*

### 5.1 Version numbers and extension identifiers

A revision of this document carries a version `<major>.<minor>` (§4.4.1).

- The **major** version names a wire-compatible generation of the protocol. Peers with the
  same major version exchange envelopes. Peers with different major versions do not.
- The **minor** version counts compatible revisions within one major version.

Each major version has exactly one **extension identifier**, of the form
`{reverse-dns-prefix}/{name}`. The extension identifier is what peers declare and negotiate
(§6.5). A breaking change requires a new identifier; the identifier is never reused across
a breaking revision. This adopts the extension-identifier rule recorded in
`docs/planning/decisions/C3-spec-packaging.md` §6(a) and §8.

| Major version | Extension identifier | Status |
|---|---|---|
| 0 | `io.github.rossgraeber/oac-session-channels` | current |

The prefix derivation and its ownership evidence are in
`docs/planning/decisions/C3-spec-packaging.md` §2-§3.

*Dated note, 2026-10-03 (#41): naming the identifier in this document, and not only in the
binding document (E6), is a ruling recorded on #41. The identifier names no provider or
transport.*

The rules of §5.2 and §5.3 apply from revision 0.1 onward. A major version of 0 does not
mean that breaking changes are allowed within it.

### 5.2 What a minor revision changes

A minor revision is published under the same major version and the same extension
identifier. It contains only changes that a receiver implementing any earlier minor
revision of the same major version handles correctly without being updated:

1. Adding an optional top-level member, which older receivers ignore (§4.8).
2. Adding an optional member to the `text` part, which older receivers ignore
   ([SC-ENV-063]).
3. Adding a content part type that a sender uses only after the addressed session
   advertises it ([SC-ENV-066]).
4. Adding an optional capability (a `MAY`) to the capability set of §6.4.
5. Adding an error code that older peers treat as the `failed` delivery state (§8).
6. Adding a `SHOULD`, `SHOULD NOT` or `MAY` requirement.
7. Adding a fixture for an existing requirement, or replacing a `TODO(fixture)` with one.
8. Editorial changes that leave every requirement's normative meaning unchanged.

### 5.3 What forces a new major version and a new extension identifier

Any of the following is a breaking change. A breaking change is published only under a new
major version and a new extension identifier:

1. Removing or renaming a defined member.
2. Making an optional member required.
3. Narrowing the syntax or value range of a member, so that a previously valid envelope
   becomes invalid.
4. Widening the syntax or value range of a member that receivers validate, so that an
   envelope valid under the new revision is invalid under an older one. Examples: a longer
   identifier token, a new character in the token set, a new timestamp form.
5. Changing the meaning of a member, a delivery state or an error code.
6. Changing the signed field set, the signature algorithm, the canonicalization or the
   verification procedure (`spec/security.md`).
7. Adding a member to, or removing one from, the `security` object (§4.6).
8. Moving a member from one trust class to another (§4.7).
9. Changing how a receiver handles unrecognized members (§4.8) or unsupported part types
   ([SC-ENV-065]).
10. Changing the expiry rules of §4.9.
11. Adding a `MUST` or `MUST NOT` that an implementation conformant to the previous minor
    revision would violate.

A change that matches neither list is treated as breaking until a revision of this
section classifies it.

### 5.4 Receiver rules

[SC-VER-001] A receiver MUST reject an envelope whose major version is not one the receiver
supports.

[SC-VER-002] A receiver MUST NOT reject an envelope solely because its minor version is
higher than the minor version the receiver implements.

[SC-VER-003] A receiver MUST validate an envelope whose minor version is higher than its
own against the rules of the revision the receiver implements.

A higher minor version contains only additions that §5.2 permits. Those additions are
members the receiver ignores (§4.8) or part types it rejects ([SC-ENV-065]).

### 5.5 Version and extension negotiation

*Owned by E2 (#42), in §6.5.* What peers declare, how they agree on a major version and an
extension identifier, and what a peer does on an unknown version during negotiation.

---

## 6. Session identity, addressing, and capability negotiation

*Owned by E2 (#42). Requirement area: `ID`.* Sources:
`docs/planning/decisions/C4-session-identity.md` (as revised by #236),
`docs/planning/decisions/C3-spec-packaging.md`, and the backlog E2 acceptance criteria.
Sections 6.1 to 6.6 keep the numbering of the E1 skeleton. Sections 6.7 to 6.10 are added
after them.

This section uses four layers of identity. Only the first two carry authority
(`docs/planning/decisions/C4-session-identity.md` §1):

1. the **device key**, defined in `spec/security.md`;
2. the **session id**, an opaque value bound to one device key (§6.1);
3. the **display form**, a string for people to read (§6.2);
4. the **alias**, a local label a person chooses (§6.2).

Routing, authorization and provenance use the device key and the session id only.

### 6.1 Session identifiers

A **session id** names one session for as long as that session is bound (§6.7). It is
128 bits of random output, written as 26 characters of lower-case Crockford Base32: the
value, most significant bit first, after two leading zero bits. As a whole value it
matches:

```
[0-7][0-9a-hjkmnp-tv-z]{25}
```

The alphabet is the digits and the lower-case letters except `i`, `l`, `o` and `u`. The
first character is limited to `0` to `7` because 26 characters hold 130 bits and the top
two bits are zero. Every session id is an identifier token (§4.3).

The value carries no structure. It encodes no device, harness, time, working directory or
transport address.

[SC-ID-001] The values of `from` and `to` MUST each be a session id.

A receiver rejects an envelope that fails [SC-ID-001], as it does an envelope that fails
[SC-ENV-010]. E4 (#44) assigns the §8.3 error code for a failure of §6.1.

[SC-ID-002] A receiver MUST test the session-id pattern against the whole decoded string
value, without trimming, case-folding or any other normalization.

Some Base32 decoders accept upper case, map `i` and `l` to `1` and `o` to `0`, and skip
hyphens. A receiver does none of these. Two session ids are equal only when their strings
are equal.

[SC-ID-003] An implementation MUST generate each session id from 128 bits of output of a
cryptographically secure random number generator.

[SC-ID-004] An implementation MUST NOT derive a session id from a device key, a
harness-native session identifier, a working directory, a time, or any other input.

An id derived from the device key would let anyone who sees two ids from one device link
them to that device (`docs/planning/decisions/C4-session-identity.md` §14).

[SC-ID-005] An implementation MUST NOT infer a device, harness, time or any other property
of a session from the characters of its session id.

[SC-ID-006] An implementation MUST NOT place a harness-native session identifier or a
transport-native address in any envelope member, or in a session descriptor (§6.3), in
place of a session id.

[SC-ID-007] An implementation MUST NOT change the session id of a binding while that
binding lasts.

A binding lasts from registration until deregistration (§6.7). A change of harness-native
identity ends one binding and starts another, under a new session id (§6.7, case 4).

[SC-ID-008] An implementation MUST NOT assign a session id that it has already assigned to
an earlier registration.

[SC-ID-009] An implementation MUST bind each session id to exactly one device key, by a
registration record signed with that device key.

The registration record and its signature are defined in `spec/security.md`. Verifying a
binding means checking that signature, never recomputing the id
(`docs/planning/decisions/C4-session-identity.md` §2, §5).

### 6.2 Display form and human aliases

The **display form** of a session is a string of the shape
`session://<device>/<harness>/<session-id>`. `<device>` is a short device label, `<harness>`
a harness label, and `<session-id>` the session id. It exists for listings, logs and
diagnostics. This document defines no grammar for the labels, because no conformant
implementation parses them.

[SC-ID-020] The values of `from` and `to` MUST NOT hold a display form.

[SC-ID-021] An implementation MUST NOT use a display form, or any part of one, to route a
message, to make an authorization decision, or as provenance.

This resolves conflict C8 as `docs/planning/decisions/C4-session-identity.md` §8 records:
the opaque id and the display form are separate values, and only the opaque id is on the
wire.

An **alias** is a label a person assigns, on one device, to a session id. It is local to
that device and can change at any time. An implementation resolves an alias to a session
id on the device where it was made, and the envelope carries the session id.

[SC-ID-022] The values of `from` and `to` MUST NOT hold an alias.

[SC-ID-023] An implementation MUST NOT transmit an alias to a peer.

This covers every member an implementation sets, including a session descriptor's
`display_name` (§6.3), which is not an alias. Only `content`, which a user or a model
writes, can carry an alias's text, and there it is content.

[SC-ID-024] An implementation MUST NOT use an alias as an authorization subject.

An alias or a display form that appears in `content` is content. It is untrusted, and it is
never provenance ([SC-ENV-082]).

### 6.3 Session descriptor

A **session descriptor** is the JSON object that describes one session to a peer, for
example in a discovery result (§7.3). Its members:

| Member | Type | Presence | Authority |
|---|---|---|---|
| `session_id` | session id (§6.1) | exactly once | authoritative address |
| `capabilities` | capability declaration (§6.4) | exactly once | what the session supports |
| `display_name` | string | zero or once | none; display only |
| `harness_label` | identifier token (§4.3) | zero or once | none; display only |

[SC-ID-040] A session descriptor MUST contain a `session_id` member whose value is a session
id.

[SC-ID-041] A session descriptor MUST contain a `capabilities` member whose value is a
capability declaration (§6.4).

[SC-ID-042] An implementation MAY include `display_name` and `harness_label` in a
session descriptor. A descriptor without them is still complete, and a consumer still
identifies the session by `session_id` alone.

[SC-ID-043] A consumer MUST NOT use `display_name` or `harness_label` to route a message,
to make an authorization decision, or as provenance.

[SC-ID-044] A consumer MUST ignore a session-descriptor member that this revision does not
define.

A descriptor carries no harness-native identifier ([SC-ID-006]).

[SC-ID-045] A session descriptor MUST NOT contain the session's working directory, or any
part of it.

The working directory limits who is allowed to learn of a session at all, which is a
discovery and authorization rule (§7.3, `spec/security.md`;
`docs/planning/decisions/C4-session-identity.md` §5).

### 6.4 Session capabilities

A **capability declaration** states what one session supports. It is a JSON object with
one member per extension identifier (§5.1) that the declaring implementation implements
for the session. Each member's value is a **capabilities entry**:

| Member | Type | Presence | Meaning |
|---|---|---|---|
| `revision` | string, `<major>.<minor>` | exactly once | the revision of this document the declarer implements for this identifier |
| `active_inbound` | boolean | exactly once | whether the session accepts envelopes by active delivery (§7.1) |
| `content_types` | array of strings | zero or once | content part types the session accepts, beyond `text` |
| `max_envelope_octets` | integer | zero or once | the largest serialized envelope the session accepts |

Example (informative):

```json
{
  "io.github.rossgraeber/oac-session-channels": {
    "revision": "0.1",
    "active_inbound": true
  }
}
```

A binding document carries the declaration in its own form. For example, a binding whose
negotiation has a per-extension settings object places each entry there.

[SC-ID-060] A capability declaration MUST be a JSON object.

[SC-ID-070] A consumer MUST treat a capability declaration that is not a JSON object as
holding no entries.

A consumer ignores a member whose name is not an extension identifier it implements
([SC-ID-082]).

[SC-ID-061] The `revision` member of a capabilities entry MUST be a string that matches the
`version` pattern of [SC-ENV-020] and whose major version is the one §5.1 assigns to the
entry's extension identifier.

[SC-ID-062] The `active_inbound` member of a capabilities entry MUST be a boolean.

[SC-ID-063] A declarer MAY include `content_types` in a capabilities entry. A consumer that
finds no `content_types` member treats `text` as the only supported part type.

[SC-ID-064] When `content_types` is present, its value MUST be an array of strings, each a
core type or an extension type (§4.5.2).

A session always accepts `text`. Listing it in `content_types` is allowed and changes
nothing.

[SC-ID-065] A declarer MAY include `max_envelope_octets` in a capabilities entry. A
consumer that finds no `max_envelope_octets` member applies the default of [SC-ENV-004].

[SC-ID-066] When `max_envelope_octets` is present, its value MUST be an integer from 65536
to 9007199254740991 inclusive.

The lower bound is the default of [SC-ENV-004], so a declaration only ever raises it. The
upper bound is the largest integer that I-JSON represents exactly ([RFC7493] §2.2).

[SC-ID-067] A consumer MUST ignore a member of a capabilities entry that this revision does
not define.

[SC-ID-068] A consumer MUST treat a capabilities entry that fails any of [SC-ID-061],
[SC-ID-062], [SC-ID-064] and [SC-ID-066] as absent.

[SC-ID-069] A consumer MUST NOT discard the other entries of a declaration because one entry
is invalid.

### 6.5 Version and extension negotiation

Peers do not exchange a handshake message in this revision. A sender learns a session's
capability declaration from a session descriptor (§6.3) or from a binding document's own
negotiation, and it decides alone what to send.

[SC-ID-080] An implementation MUST make a capability declaration available for each
session it exposes to peers.

[SC-ID-081] An implementation MUST NOT include an entry for an extension identifier that it
does not implement for that session.

[SC-ID-082] A consumer MUST ignore a declaration member whose name is not an extension
identifier that the consumer implements.

This is the rule for an unknown version during negotiation. A declaration that names a
newer extension identifier, and so a newer major version, still works with an older peer
if it also names one the peer implements. A declaration that names only identifiers the
peer does not implement leaves the two with no common version.

[SC-ID-083] A sender MUST select, as the **agreed version**, the highest major version
whose extension identifier the sender implements and the session's declaration holds a
valid entry for.

[SC-ID-084] A sender MUST NOT send an envelope to a session with which it has no agreed
version.

The sender reports the refusal to the requesting session ([SC-ID-102]). Which §8.3 error
code that report carries is set by E4 (#44).

[SC-ID-085] A sender MUST NOT refuse to send solely because the minor version in the
session's entry differs from its own.

A receiver validates a higher minor version under its own rules ([SC-VER-003]). A sender
uses only what the session's entry declares (§6.6), so a lower minor version on the
receiver is safe too.

[SC-ID-086] A sender MUST NOT send an envelope to a session for which it holds no
capability declaration.

*Dated note, 2026-10-03 (#42): [SC-ID-086] is an operator decision recorded on #42. Until
E3 (#43) or E6 (#46) defines how a declaration reaches a sender, no conformant send is
possible. A reply (§8.2) is a send too, so the replier needs the original sender's
declaration.*

[SC-ID-087] A sender MUST set the envelope's `version` to the revision it implements for
the agreed major version.

On receipt, an envelope whose major version the receiver does not support is rejected by
[SC-VER-001]. That is the rule for an unknown version on the wire.

### 6.6 Unsupported-capability behaviour

The **agreed entry** is the capabilities entry for the agreed version (§6.5). A sender
checks every envelope against it before sending.

[SC-ID-100] A sender MUST NOT send an envelope to a session whose agreed entry has
`active_inbound` set to `false`.

A session that does not accept active delivery can still send. It cannot receive in this
revision: no inbox, mailbox or polling path stands in for active delivery (§7.1;
`docs/planning/ADR-001.md`, "v0.1 scope").

[SC-ID-101] A sender MUST treat a content part type as advertised, for [SC-ENV-066], only
when the type is `text` or the agreed entry's `content_types` lists it.

[SC-ID-102] A sender that refuses to send under §6.5 or §6.6 MUST report the refusal to the
requesting session.

The report carries an error code from §8.3. E4 (#44) maps each refusal cause in this
section to its code. A silent drop is not conformant.

[SC-ID-103] A sender MUST NOT change a requested message to fit a session's capabilities.

Dropping a content part, splitting an envelope or choosing another session are all
changes. The requesting session decides what to send next.

[SC-ID-104] An implementation MUST NOT declare `active_inbound` as `true` for a session
unless it delivers to that session as §7.1 requires.

[SC-ID-105] A receiver MUST reject an envelope addressed to a local session whose own
capabilities entry, for the envelope's major version, has `active_inbound` set to `false`.

### 6.7 Binding a session id to a live session

A **binding** ties one session id to one live harness session. This section states when an
implementation creates, keeps, refuses and ends bindings. It folds in
`docs/planning/decisions/C4-session-identity.md` §3 ("Revision, 2026-10-02", #236), §5, §6
and §7 in neutral terms. Harness-specific capture and pairing detail belongs in each
adapter's binding document.

#### 6.7.1 Terms

- **Harness-native id (N):** the identifier a harness gives one live conversation. An
  adapter captures it through a supported harness surface.
- **Native signal:** an event, received through the surface that an adapter's binding
  document names as authoritative for N, that carries N and a start kind.
- **Start kind (S):** `fresh` for a newly started conversation, or `transition` for a
  conversation that was resumed, cleared, forked, compacted or otherwise carried on from an
  earlier one. The adapter's binding document maps each harness-reported start value to
  one of the two.
- **Attachment:** the local path through which an implementation hands envelopes to one
  live session and takes send requests from it.
- **Cross-check value (E):** a second identifier, reported by the harness to a process of
  the implementation, that the harness documents as equal to N on a fresh start. It is
  never authoritative. An attachment has at most one, reported to the attachment's own
  process. A native signal can also carry one, reported to the process that received the
  signal: the **signal's cross-check value**.
- **Pairing:** the implementation's decision about which attachment a native signal belongs
  to.
- **Registration record:** the record of a binding: the session id, the device, N, a
  harness label, the working-directory scope and the registration time, signed by the
  device key ([SC-ID-009]).
- **Finding:** a local record that an operator is meant to review, because it might show a
  misconfiguration or an attack. **Diagnostic:** a local record for troubleshooting only.

> **Reference implementation note:** `docs/planning/decisions/C4-session-identity.md` §3
> records the v0.1 mapping for the first adapter, its native signal, its cross-check value
> and its attachment, and §4 the capture path for the second.

#### 6.7.2 Pairing

[SC-ID-120] An implementation MUST create a binding only on a paired native signal.

[SC-ID-121] An implementation MUST pair a native signal with an attachment using a key that
the implementation observes itself from the operating system, never a value that the
attaching process supplies.

[SC-ID-122] An implementation MUST NOT use a cross-check value as the only pairing key.

[SC-ID-127] An implementation MUST NOT use a cross-check value in pairing a `transition`
signal.

After a transition the attachment's cross-check value is stale by design.

*Dated note, 2026-10-03 (#42): which operating-system facility yields such a key for each
supported platform is UNVERIFIED. It is carried as an open item in
`docs/planning/decisions/C4-session-identity.md` §3 ("Pairing requirement") and
`docs/planning/STATUS.md`, owned by the adapter work in Epic F and G9. Until it is
established, every native signal is unpairable ([SC-ID-125]). That costs availability,
never authority.*

A native signal can arrive before its attachment exists. It is then **not yet pairable**.

[SC-ID-123] An implementation MUST hold a not-yet-pairable native signal for at most a
bounded window.

[SC-ID-124] An implementation MUST drop a native signal, binding nothing, when the window
ends before the signal is paired.

[SC-ID-128] An implementation that drops a native signal under [SC-ID-124] MUST record a
diagnostic.

This document does not fix the window's length.

A signal is **unpairable** when no implementation-observed key is available, or when more
than one attachment is a candidate.

[SC-ID-125] An implementation MUST NOT bind a native signal that it cannot pair with
certainty.

[SC-ID-129] An implementation that does not bind an unpairable native signal MUST record a
finding.

Before any signal is paired with it, an attachment's cross-check value is a hint for logs
only.

[SC-ID-126] An implementation MUST NOT use an unpaired attachment's cross-check value to
create a registration record, to select a session id, to make the session discoverable or
present, to route or deliver a message, as provenance, as an allowlist or authorization
subject, in pairing with a peer, or to authorize anything.

[SC-ID-144] An implementation MUST NOT place a cross-check value in any envelope member or
session descriptor.

#### 6.7.3 The ordered cases

An implementation applies these cases, in order, to each paired native signal. The first
case that matches decides.

[SC-ID-143] An implementation MUST compare native ids and cross-check values as exact
strings, without trimming, case-folding or any other normalization.

Each case ends in one **binding result**: `unchanged` (case 1), `refused` (case 2),
`failed-closed` (case 3(b), and an unpairable signal under [SC-ID-125]), `bound` (cases
3(a), 3(c) and 4), or `dropped` (a signal dropped under [SC-ID-124]).

**Case 1, same N.** The attachment is already bound to the signal's N.

[SC-ID-130] When the attachment is already bound to the signal's N, the implementation MUST
leave the binding unchanged, whatever the start kind and the cross-check value.

Case 1 covers a harness whose native id survives a resume that the implementation observes
on the same attachment: the session keeps its session id
(`docs/planning/decisions/C4-session-identity.md` §7).

*Dated note, 2026-10-03 (#42): this narrows C4 §7, which says the session id "MAY be
re-bound to the same thread". Here the id is kept only while the same attachment stays
live. A conversation resumed after its binding was deregistered gets a new session id
([SC-ID-008], [SC-ID-155]). The narrowing is deliberate, not an omission: a record never
outlives its attachment (C4 §5).*

**Case 2, duplicate N.** N is bound to a different attachment that is still live.

[SC-ID-131] When N is bound to a different live attachment, the implementation MUST NOT
bind the signal's attachment.

[SC-ID-132] An implementation that refuses a signal under case 2 MUST record a finding.

[SC-ID-133] An implementation MUST NOT withdraw or change the other attachment's binding
under case 2.

[SC-ID-134] An implementation MUST compare only N, never a cross-check value, when it tests
for a duplicate.

An attachment whose conversation moved on from N to another id still holds its old
cross-check value. Keying on that value would refuse a later, legitimate resume of N.

**Case 3, fresh start.** S is `fresh`.

[SC-ID-135] An implementation MUST treat a native signal whose start kind is missing, or is
a value the adapter's binding document does not map, as `fresh`.

This is the fail-closed default: an unexplained mismatch at a fresh start blocks.

At a fresh start, a cross-check value **differs** when the attachment's cross-check value,
or the signal's cross-check value, is present and not equal to N.

[SC-ID-136] When S is `fresh`, the attachment's cross-check value equals N, and no
cross-check value differs (case 3(a)), the implementation MUST bind N under a new session
id.

[SC-ID-137] When S is `fresh` and a cross-check value differs (case 3(b)), the
implementation MUST NOT bind either value.

A signal's own cross-check value that differs from N therefore fails closed at a fresh
start, as `docs/planning/decisions/C4-session-identity.md` §3 ("Hook handler's own
environment") requires.

[SC-ID-138] An implementation that fails a signal closed under case 3(b) MUST record a
finding.

[SC-ID-139] When S is `fresh`, the attachment has no cross-check value, and no cross-check
value differs (case 3(c)), the implementation MUST bind N under a new session id.

[SC-ID-141] An implementation that binds under case 3(c) MUST record a diagnostic.

The diagnostic notes the missing cross-check value. It is not a finding.

**Case 4, transition.** S is `transition`.

[SC-ID-140] When S is `transition`, the implementation MUST bind N under a new session id,
without comparing any cross-check value.

[SC-ID-142] An implementation that binds under case 4 while the attachment's or the
signal's cross-check value differs from N MUST record a diagnostic.

The session keeps working, under a new session id. Peers that address the old session id
no longer reach it.

#### 6.7.4 Re-binding and stale bindings

[SC-ID-150] When case 3(a), 3(c) or 4 binds an attachment that is already bound to a
different N, the implementation MUST first deregister the attachment's earlier
registration record.

[SC-ID-151] An implementation MUST NOT carry authorization state from an earlier session id
to a new one.

Allowlist entries and pairing grants name the earlier session id. They do not follow the
conversation to its new id (`docs/planning/decisions/C4-session-identity.md` §6).

A **stale binding** arises when a paired native signal carries an N that differs from the
N its attachment is bound to, and the signal is refused under case 2 or fails closed under
case 3(b).

[SC-ID-152] On a stale binding, the implementation MUST deregister the attachment's
existing registration record.

[SC-ID-153] On a stale binding, the implementation MUST record a finding.

The attachment ends unbound. An unbound attachment is better than one that delivers into a
conversation the session has left. The other attachment's binding in case 2 is untouched
([SC-ID-133]).

A dropped or unpairable signal cannot be attributed to an attachment, so [SC-ID-152] cannot
fire for it, and the attachment's old binding can survive a transition it never saw.

[SC-ID-154] An implementation that can attribute a dropped or unpairable native signal to
the harness process behind a bound attachment MUST stop delivering to, and accepting send
requests from, that attachment until a later signal is paired with it.

Otherwise its messages can reach a conversation the session has left. C4 §3 ("Residual")
and §13 make closing this gap a requirement on the implementation.

*Dated note, 2026-10-03 (#42): whether any pairing mechanism can make that attribution is
UNVERIFIED, tied to the pairing-mechanism item in the dated note of §6.7.2
(`docs/planning/decisions/C4-session-identity.md` §3, "Residual"). An implementation that
cannot make it is not bound by [SC-ID-154].*

An implementation binds only on signals it receives. A transition that happens elsewhere
and never reaches it leaves the binding as it was last observed.

#### 6.7.5 Lifetime and authority

[SC-ID-155] An implementation MUST deregister a binding when its attachment ends.

[SC-ID-156] An implementation MUST NOT keep a registration record after the implementation
itself stops.

A record that outlived its implementation would be a durable store, which this revision
does not define (`docs/planning/ADR-001.md`, "v0.1 scope").

[SC-ID-157] An implementation MUST NOT treat a binding as an authorization.

A bound session is registered, not trusted. Whether a message is delivered to it, and
whether a sender is allowed to reach it, is decided by `spec/security.md`, which denies by
default.

### 6.8 Attributing send requests

An implementation that sends an envelope on a session's behalf decides which session asked.
This applies the operator decision recorded on #46 (2026-10-03) in neutral terms: a request
the implementation cannot tie to exactly one bound session is refused, and a harness's own
claim about which session it is does not count.

[SC-ID-160] A sender MUST set `from` to the session id bound to the attachment on which the
send request arrived.

[SC-ID-161] A sender MUST refuse a send request that arrives on an attachment that is not
bound to exactly one session.

The refusal carries the authorization-failure error of §8.3 ([SC-ID-102]).

[SC-ID-162] A sender MUST NOT attribute a send request using a session id or harness-native
id that the request itself carries.

### 6.9 Identity and presence

Presence (§7.2) says whether a session is reachable. It does not decide identity.

[SC-ID-180] An implementation MUST NOT change a session id, or end a binding, because of a
change in presence alone.

[SC-ID-181] An implementation MUST NOT treat a session's presence as evidence that its
session id is bound to a device key.

The binding is evidenced only by the registration record's signature ([SC-ID-009]).

[SC-ID-182] An implementation MUST NOT announce presence for, or return in discovery, an
attachment that is not bound.

### 6.10 Conformance fixtures for this section

Section 6 fixtures live in `tests/protocol/sc-id/` and use the members of §3.3. The
fixtures for [SC-ID-001] and [SC-ID-002] use the `envelope` stage exactly as §3.3 defines
it. The other fixtures use one of three further stages. For those, `context`, `input` and
`expected` hold the members below instead of the ones §3.3 lists.

**Common values.** An **implemented list** is an array of objects, each with `extension`
(an extension identifier), `major` (an integer) and `revision` (a `<major>.<minor>`
string): the versions the implementation under test implements. A fixture can name a
hypothetical identifier for a major version that §5.1 does not list yet. An **attachment
list** is an array of objects, each with `attachment` (a label), optionally `cross_check`
(a string) and optionally `binding` (an object with `native_id` and `session_id`). An
attachment without `binding` is unbound.

**Stage `negotiation`** (§6.4, §6.5).

| Member | Content |
|---|---|
| `context` | `implemented`: an implemented list. |
| `input` | `declaration`: the session's capability declaration, as a JSON value. |
| `expected` | `result`: `agreed` or `no-common-version`. With `agreed`: `extension` and `major`, the agreed version. |

**Stage `binding`** (§6.7).

| Member | Content |
|---|---|
| `context` | `attachments`: an attachment list, before the signal. |
| `input` | `signal`: an object with `native_id`; `start_kind` (`fresh`, `transition`, or `unmapped` for a value the binding document does not map), omitted when the signal has none; optionally `cross_check`, the signal's cross-check value; `pairing` (`paired`, `window-expired` or `unpairable`); and, when `pairing` is `paired`, `attachment`. |
| `expected` | `result`: the binding result of §6.7.3, which maps each value to its case: `unchanged` (case 1), `refused` (case 2), `failed-closed` (case 3(b), or `pairing` `unpairable`), `bound` (cases 3(a), 3(c), 4) or `dropped` (`pairing` `window-expired`). `attachments`: the attachment list after the signal, where a `session_id` of `new` means a freshly generated session id equal to none in `context`. `record`: `none`, `diagnostic` or `finding`, the local record the rules of §6.7 require. |

**Stage `send`** (§6.5, §6.6, §6.8).

| Member | Content |
|---|---|
| `context` | `implemented`: an implemented list. `attachments`: an attachment list. `declarations`: an object whose members are session ids and whose values are capability declarations the sender holds. |
| `input` | `request`: an object with `attachment`, `to` (a session id), `content` (as §4.5) and optionally `asserted_from` (a session id the request itself claims). |
| `expected` | `result`: `sent` or `refused`. With `sent`: `from` and `version`, the values the envelope carries. |

The `refused` result of the `send` stage is a sender's refusal, never a delivery state of
§8.1. The binding results are local outcomes, never delivery states either. E4 (#44)
decides which `sc-id` fixtures gain an `expected.error` member.

> **Reference implementation note:** the v0.1 conformance runner (tasks E8 and F12) drives
> the `binding` and `send` stages through the implementation's own binding and send logic
> with a scripted attachment list, so no live harness takes part. What a fixture cannot
> show, such as how the pairing key is obtained or that ids are random, is marked
> `TODO(fixture)` in Appendix A with the task that tests it.

---

## 7. Active delivery, presence, and discovery

*Owned by E3 (#43). Requirement area: `DLV`.* Sources: `docs/planning/DESIGN.md`,
"Presence/discovery", "Delivery semantics" and "Transport contract";
`docs/planning/ADR-001.md`, "Decision" and "Boundary"; `docs/planning/decisions/C2-process-model.md`
§5 (presence lifetime); `docs/planning/decisions/C4-session-identity.md` §5 (cross-project
scoping); `docs/planning/decisions/C5-envelope-auth.md` §11 (default deny);
`docs/planning/decisions/C6-trust-rendering.md` §8 (session listing); and Decision C7, the
v0.1 transport mapping (`docs/planning/decisions/`; §4 presence, §7 the local-mode
authorization carve-out). C7 §4 maps presence onto one transport and records two gaps that
this section closes at the neutral layer: no carriage for a session's descriptor, and no
discovery path.

This section defines how a receiver delivers to a live session, how an implementation
announces and withdraws a session's presence, and how a sender learns which sessions exist
and what they accept. It defines the content and meaning of a presence record (§7.2.2) and
of a discovery result (§7.3). It does not define how either is framed or carried: the
transport contract carries presence records (DESIGN "Transport contract",
`announce_presence` and `watch_presence`), and a transport binding maps them onto one
transport. `spec/security.md` (E5, #45) authenticates them.

### 7.1 The active-inbound obligation and the no-polling rule

#### 7.1.1 Terms

- **Active delivery:** a receiver hands an accepted envelope's content off to the addressed
  session on its own initiative, through the harness's supported input surface (§2.3),
  without any request from the session, its harness or a model. An envelope is
  **accepted for hand-off** when it has passed every check that precedes the hand-off call
  (§8.3.2). This is a receiver's judgement about one envelope. It is neither the
  acceptance of a presence record (§7.2.3) nor the delivery state `accepted-by-adapter`
  (§8.1).
- **Polling:** issuing a request repeatedly, on a timer or in a loop, to learn whether an
  envelope is waiting. An inbox, a mailbox, a message list that a session is expected to
  re-read, and a receiver loop that asks a transport on a timer for new envelopes are all
  polling.
- **Push mechanism:** a streaming connection, a subscription, an event loop or a keepalive
  that a harness or a transport supports. Each one waits for an event that the other side
  pushes, or only keeps a connection open. None of them is polling. A keepalive that asks
  nothing about waiting envelopes is not polling.

The obligation below is DESIGN's normative concept ("OAC Session Channels specification,
packaged as an MCP extension"): "a harness advertising active inbound OAC Session Channels
support accepts an authorized external channel message as input to the addressed live
session without application-level polling." It serves the `docs/planning/ADR-001.md`
("Context") requirement of "push rather than application polling". The rule against
polling is DESIGN "Delivery semantics": "Provider-supported streaming connections,
subscriptions, event loops, and keepalives are acceptable; application-level inbox polling
is not for adapters claiming active inbound support."

#### 7.1.2 Requirements

[SC-DLV-001] A receiver MUST hand off, by active delivery, each envelope that it accepts
for hand-off for a local session whose own capabilities entry, for the envelope's major
version, has `active_inbound` set to `true`.

What the receiver then reports is §8.1: `handed-to-harness`, `unknown`, or an error state.

[SC-DLV-002] An implementation that declares `active_inbound` as `true` for a session MUST
NOT use polling to obtain envelopes for that session or to hand them off.

[SC-DLV-003] An implementation that declares `active_inbound` as `true` for a session MUST
NOT offer that session a surface from which the session is expected to retrieve envelopes
by repeated request.

A binding document states this rule in its own terms. The MCP binding does so in
`spec/bindings/mcp.md` §2 ([MCPB-DLV-002]).

[SC-DLV-004] An implementation MAY use any push mechanism that a harness or a transport
supports to receive envelopes and to hand them off. An implementation that uses none still
meets [SC-DLV-001] through the harness's supported input surface.

An implementation that can reach a session only by polling does not claim active inbound
for it: it declares `active_inbound` as `false` ([SC-ID-104]), and the session is then
send-only (§6.6). Polling silently while claiming active inbound is the drift that
`oac-boundaries` boundary 14 describes.

[SC-DLV-005] A receiver MUST NOT hand off by writing to a terminal or a user interface, by
editing a harness's stored history, or through an interface that the harness does not
document.

This restates, for hand-off, the `docs/planning/ADR-001.md` "Boundary" rule against "UI/terminal
scraping or undocumented private RPCs", and `docs/planning/PLANNING-PROMPT.md` §10: "Do not
substitute model APIs, UI automation, terminal scraping, credential reuse, private RPCs, or
rollout-file manipulation for a supported interface." A harness's stored history (its
rollout file, where it has one) is not an input surface.

#### 7.1.3 Accepting input

A session is **accepting input** when all of the following hold:

- it is bound (§6.7) and its attachment is live;
- the implementation is not withholding delivery from it under [SC-ID-154];
- the harness's input surface does not turn a hand-off call away as unable to take input
  now.

A receiver judges the first two conditions from its own state, before any call. It learns
the third only from the surface: a hand-off call that the surface turns away as unable to
take input now, without having taken it, is the evidence that the session is not
accepting input.

A session that is running a turn can still be accepting input. Both harness input surfaces
that the v0.1 bindings use took input submitted while a turn was running, in the gate runs:

- On one surface, two inputs sent during a running turn were each delivered at a later
  tool-call boundary inside that same turn, in submission order, with nothing dropped
  (`docs/planning/gates/G1-result.md`, criterion 3, re-run Box C, current verdict
  2026-09-28). That harness's first-party documentation states that queued events are
  processed in order (`docs/planning/REVERIFICATION-B2.md`, "§3.1 re-check", row 5, HOLDS,
  retrieved 2026-10-02).
- On the other, one input queued during a running turn ran after that turn completed
  (`docs/planning/gates/G2-result.md`, the `busyqueue` step). Only one input was queued,
  so order among several queued inputs was not exercised, and no first-party statement of
  order is cited in this repository. That this surface keeps order is UNVERIFIED
  (`docs/planning/STATUS.md`, "Open UNVERIFIED items").

[SC-DLV-006] A receiver MUST NOT treat a session as not accepting input solely because the
session's harness is running a turn, when the harness's input surface accepts input during
a turn.

[SC-DLV-007] A receiver MUST NOT hold an envelope for later hand-off because the addressed
session is not accepting input.

The receiver reports `destination-unavailable` instead (§8.3, delivery stage step 2), and
the sending implementation decides whether to retransmit or retry (§8.4). An envelope held
until a session came back would be an offline mailbox, which is out of scope for this
revision (`docs/planning/ADR-001.md`, "v0.1 scope"). Input that a harness itself holds after
a completed hand-off is held by the harness, not by the receiver, and this rule does not
touch it.

*Dated note, 2026-10-03 (#43): refusing at once, rather than holding an envelope in memory
for a short window, is an operator decision recorded on #43.*

[SC-DLV-008] A receiver whose hand-off call the harness's input surface turns away as unable
to take input now, without having taken the input, MUST report `destination-unavailable`.

[SC-DLV-009] A receiver whose hand-off call the harness's input surface reports as failed
for any other reason MUST report `handoff-failed`.

The two codes tell the sending implementation different things (§8.3, Table 8.3): a session
that is not accepting input now, or a hand-off that was tried and failed. A receiver that
already knows, before calling, that the first or second condition above fails reports
`destination-unavailable` without calling. A call whose outcome is indeterminate is neither:
it is `unknown` (§8.1).

### 7.2 Presence

#### 7.2.1 Presence states

**Presence** is what one implementation, the **observer**, knows about whether a session
is reachable. It has exactly three states, from DESIGN "Presence/discovery":

| State | Meaning |
|---|---|
| `online` | The observer holds an accepted announcement (§7.2.2) for the session that is not stale (§7.2.4). |
| `unreachable` | The observer has accepted a presence record for the session, and the latest one it accepted is a withdrawal or is stale. |
| `unknown` | The observer holds no accepted presence record for the session. |

For a session of its own, an implementation needs no presence record: the session is
`online` to it while the session is bound and delivery to it is not withheld under
[SC-ID-154], and `unreachable` after that.

The presence state `unreachable` describes a session. The delivery state `unreachable`
(§8.1) describes one envelope. A sender that refuses a request because of presence creates
no envelope, so it reports an error code (§7.3.3), not a delivery state.

[SC-DLV-020] An implementation MUST NOT report a presence state other than `online`,
`unreachable` and `unknown`.

A presence state says nothing about what a harness or a model is doing. Whether a turn is
running, whether a model is thinking, and whether a user is typing are harness activity,
which differs from harness to harness. DESIGN "Presence/discovery" asks that such activity
not be over-normalized, so this revision does not carry it at all. Presence is not
identity (§6.9) and not authorization ([SC-ID-157]).

#### 7.2.2 Presence records

A **presence record** is the JSON object an implementation issues to announce or withdraw
one of its sessions. A record whose `present` member is `true` is an **announcement**. A
record whose `present` member is `false` is a **withdrawal**. The implementation that
issues a record is its **issuer**. An implementation that receives a record is a
**consumer**.

| Member | Type | Presence |
|---|---|---|
| `session_id` | session id (§6.1) | exactly once |
| `seq` | integer, 0 to 9007199254740991 | exactly once |
| `present` | boolean | exactly once |
| `issued_at` | timestamp (§4.4.6), by the issuer's clock | exactly once |
| `lifetime_ms` | integer, 1000 to 3600000 | exactly once in an announcement; never in a withdrawal |
| `descriptor` | session descriptor (§6.3) | exactly once in an announcement; never in a withdrawal |

Example (informative):

```json
{
  "session_id": "7gq3m8z2c5k9t1w4x6b0n2r8vd",
  "seq": 4,
  "present": true,
  "issued_at": "2026-10-03T12:00:00.000Z",
  "lifetime_ms": 60000,
  "descriptor": {
    "session_id": "7gq3m8z2c5k9t1w4x6b0n2r8vd",
    "capabilities": {
      "io.github.rossgraeber/oac-session-channels": {
        "revision": "0.1",
        "active_inbound": true
      }
    },
    "harness_label": "harness-b"
  }
}
```

The announcement carries the session descriptor, and through it the session's capability
declaration. That is how a declaration reaches a sender on another implementation, which
[SC-ID-080] and [SC-ID-086] need (§7.3.3).

[SC-DLV-021] A presence record MUST satisfy [SC-ENV-001], [SC-ENV-002] and [SC-ENV-003],
read with "presence record" in place of "envelope".

[SC-DLV-022] A presence record MUST contain the members `session_id`, `seq`, `present` and
`issued_at`.

[SC-DLV-023] The value of `session_id` MUST be a session id (§6.1).

[SC-DLV-024] The value of `seq` MUST be an integer from 0 to 9007199254740991 inclusive,
written as a JSON number with no sign, fraction or exponent.

[SC-DLV-025] The value of `present` MUST be a boolean.

[SC-DLV-026] The value of `issued_at` MUST be a string in the timestamp form of §4.4.6.

[SC-DLV-027] An announcement MUST contain the members `lifetime_ms` and `descriptor`.

[SC-DLV-028] The value of `lifetime_ms` MUST be an integer from 1000 to 3600000 inclusive,
written as a JSON number with no sign, fraction or exponent.

[SC-DLV-029] The value of `descriptor` MUST be a JSON object that satisfies [SC-ID-040] and
contains a `capabilities` member.

A `capabilities` value that is not a JSON object leaves the record valid. The consumer then
treats the declaration as holding no entries ([SC-ID-070]).

[SC-DLV-030] The `session_id` of an announcement's `descriptor` MUST equal the record's
`session_id`.

[SC-DLV-031] A withdrawal MUST NOT contain a `lifetime_ms` or a `descriptor` member.

[SC-DLV-032] A presence record MUST NOT contain the session's working directory, or any
part of it.

[SC-ID-045] already keeps the working directory out of the descriptor. This rule keeps it
out of the rest of the record.

*Dated note, 2026-10-03 (#43): the `lifetime_ms` range, one second to one hour, is a
decision recorded on #43.*

#### 7.2.3 Accepting a presence record

A consumer **accepts** a presence record when it keeps the record as the latest one for its
session. A record that is not accepted is discarded and changes nothing.

[SC-DLV-040] A consumer MUST discard a presence record that fails any of [SC-DLV-021] to
[SC-DLV-031].

[SC-DLV-041] A consumer MUST ignore a presence-record member that this revision does not
define.

A descriptor's own unknown members are ignored under [SC-ID-044].

[SC-DLV-042] A consumer MUST discard a presence record whose `seq` is not greater than the
`seq` of the latest record it accepted for the same session id.

`seq` orders one issuer's records for one session without comparing clocks. An
announcement delayed in transit cannot undo a later withdrawal, and a second copy of a
record is discarded, so a duplicated record changes nothing.

[SC-DLV-043] A consumer MUST NOT accept a presence record from another implementation
unless it has authenticated the record, per `spec/security.md`, as issued by the device key
to which the record's session id is bound ([SC-ID-009]).

*Dated note, 2026-10-03 (#43): `spec/security.md` (E5, #45) does not yet define how a
presence record is authenticated. Until it does, a consumer accepts no record from another
implementation, so a sender holds a capability declaration only for its own
implementation's sessions, and no conformant send crosses implementations. That presence,
discovery and sending stay within one implementation until E5 lands is an operator decision
recorded on #43. It is the same dependency §8.1.5 records for receipts. The conformance
fixtures of §7.5 take authentication as passed.*

*Dated note, 2026-10-03 (#43), constraints on E5 (#45):*

- *Replay. [SC-DLV-042] protects a consumer only while it remembers the latest `seq`. A
  consumer that restarts, or forgets a session under [SC-DLV-048], would accept a captured
  old announcement, and report a session that has ended as `online` for up to that
  announcement's `lifetime_ms`, which can be one hour. `spec/security.md` is expected to
  bound the replay of presence records across a consumer restart and a forget, not only
  while `seq` is remembered.*
- *Binding proof. [SC-DLV-043] needs a consumer to check that a session id is bound to the
  issuing device key. The registration record that binds them is held by its
  implementation and not published, and it holds the working directory and the
  harness-native id (`docs/planning/decisions/C4-session-identity.md` §5), which
  [SC-DLV-032], [SC-ID-045] and [SC-ID-006] keep off the wire. `spec/security.md` is
  expected to define a publishable proof of the binding that reveals neither, rather than
  publishing the registration record.*

`issued_at` serves diagnostics and the replay rules that `spec/security.md` defines.
Staleness does not use it (§7.2.4).

#### 7.2.4 Liveness and staleness

An accepted announcement becomes **stale** when either of the following happens first:

- `lifetime_ms` milliseconds pass, by the consumer's clock, from the moment the consumer
  accepted it, without the consumer accepting a newer record for the session;
- the carrier of the record reports that the issuer is no longer reachable (**carrier
  loss**).

An announcement that a consumer accepted at instant *t*, with a `lifetime_ms` of *L*, is
stale from *t* + *L* onward, unless the consumer accepted a newer record for the session
before then.

The **carrier** is the transport, behind the transport contract, that delivered the record.
How a transport detects that an issuer is gone, for example when a connection closes, is a
transport binding's concern.

[SC-DLV-044] A consumer MUST measure an announcement's lifetime by its own clock, from the
moment it accepted the announcement.

The issuer's and the consumer's clocks can differ. A lifetime counted from `issued_at`
would make a session look stale, or fresh, by the size of that difference.

[SC-DLV-049] A consumer SHOULD measure lifetimes on a monotonic clock. A consumer that
measures them on a wall clock deviates: a step in that clock, for example a time-zone or
synchronization correction, stretches or cuts every lifetime it is measuring.

[SC-DLV-045] A consumer MUST treat an accepted announcement as stale once its lifetime has
passed without a newer accepted record for the session.

[SC-DLV-046] A consumer MUST treat each accepted announcement from an issuer as stale when
the carrier reports carrier loss for that issuer.

The carrier's link from a transport peer to an issuer is not authentication. A transport's
own peer identifier names a connection, not a device key. Carrier loss can therefore only
move a session toward `unreachable`. It never makes a session `online`, so a spoofed
carrier loss can at worst deny service until the issuer's next announcement is accepted.

[SC-DLV-047] A consumer MUST report the presence state of each session as the table of
§7.2.1 defines it.

[SC-DLV-048] A consumer MAY forget a session whose presence state is `unreachable`. A
consumer that forgets it then reports `unknown` for it. A consumer that does not keeps
reporting `unreachable`.

A consumer that forgets a session also forgets its latest `seq`, so [SC-DLV-042] no longer
protects it against an older record. Bounding the replay of an old record is a rule of
`spec/security.md`.

#### 7.2.5 Issuing presence records

[SC-DLV-050] An implementation MUST give each presence record it issues for a session a
`seq` greater than that of every earlier record it issued for that session id.

`seq` need not survive a restart of the issuer. Session ids do not survive one
([SC-ID-156]) and are never reused ([SC-ID-008]), so a restarted issuer announces only new
session ids, and no consumer holds an earlier `seq` for them.

[SC-DLV-051] An implementation MUST issue an announcement for a session when it starts to
expose the session to peers.

[SC-DLV-052] An implementation MUST issue a new announcement for a session when the
session's capability declaration changes.

[SC-DLV-053] The `descriptor` of each announcement MUST carry the capability declaration
that the issuer makes available for the session under [SC-ID-080].

[SC-DLV-054] An implementation that keeps a session announced SHOULD issue a new
announcement before half of the previous announcement's lifetime has passed. An
implementation that does not deviates: consumers see the session go stale, and report it
`unreachable`, while it is still bound.

The repeated announcement is a keepalive (§7.1.1). It answers no question about waiting
envelopes.

[SC-DLV-055] An implementation MUST issue a withdrawal for a session when it deregisters the
session's binding.

An implementation that stops without a chance to withdraw, for example because it crashed,
leaves its announcements to go stale or to end by carrier loss. A session ends no later
than the implementation that holds its binding (`docs/planning/decisions/C2-process-model.md`
§5; [SC-ID-156]).

[SC-DLV-056] An implementation SHOULD issue a withdrawal for a session as soon as it starts
to withhold delivery from the session under [SC-ID-154]. An implementation that does not
deviates: peers see the session `online` and send envelopes that the receiver refuses with
`destination-unavailable`.

When the implementation resumes delivery to that session, it issues a new announcement,
with a greater `seq` ([SC-DLV-050]).

[SC-DLV-057] An implementation SHOULD issue a withdrawal for each session it has announced
before it stops. An implementation that does not deviates: peers report those sessions
`online` until the announcements go stale or the carrier reports carrier loss.

> **Reference implementation note:** the v0.1 reference implementation's per-device process
> holds device presence and the session bookkeeping
> (`docs/planning/decisions/C2-process-model.md` §5), so it is the issuer of every presence
> record on its device. It learns that a session's attachment has ended from the end of the
> local connection, not from a poll. Gate G3 verified peer discovery between two and three
> peers on one host on all three target platforms (`docs/planning/gates/G3-result.md`,
> verdict PASS, 2026-10-02). It did not exercise presence records, staleness or carrier
> loss, so how well the v0.1 transport carries them is UNVERIFIED.

### 7.3 Discovery

#### 7.3.1 What discovery answers

**Discovery** tells a session which other sessions it can reach. DESIGN
"Presence/discovery" asks four questions of it, and a discovery result answers each from a
session descriptor (§6.3):

| Question | Answered by |
|---|---|
| Which sessions are reachable? | The descriptors listed: each is a session that is `online` (§7.2.1). |
| Which harness owns each one? | `harness_label` and `display_name`, when present. Both are display values only ([SC-ID-043]). |
| What are its capabilities? | `capabilities` ([SC-ID-041]). |
| Does it support active inbound? | `active_inbound` in the capabilities entry (§6.4). |

A **discovery request** is a session's request, through its attachment, for a discovery
result. The session that asks is the **requester**. A **discovery result** is a JSON array
of session descriptors.

#### 7.3.2 Answering a discovery request

[SC-DLV-060] An implementation MUST refuse a discovery request that arrives on an attachment
that is not bound to exactly one session.

The refusal carries `unauthorized` (§8.3), as a refused send request does under §6.8.

[SC-DLV-061] A discovery result MUST NOT list a session that the requester is not authorized
to discover.

Which sessions a requester is allowed to discover is decided by `spec/security.md`, which
denies by default. A session in one working directory is not discoverable by a peer that is not
authorized for that directory (`docs/planning/decisions/C4-session-identity.md` §5;
`docs/planning/decisions/C5-envelope-auth.md` §11; `docs/planning/decisions/C6-trust-rendering.md`
§8).

[SC-DLV-062] A discovery result MUST list every session that is `online` to the
implementation and that the requester is authorized to discover.

[SC-DLV-063] A discovery result MUST NOT list a session whose presence state is not
`online`.

*Dated note, 2026-10-03 (#43): listing only `online` sessions, and not recently withdrawn
or stale ones, is an operator decision recorded on #43. A discovery result is an array of
descriptors with no presence member, so listing `unreachable` sessions later would need a
new member as well as a change to [SC-DLV-063].*

[SC-DLV-064] A discovery result MUST NOT list a session more than once.

[SC-DLV-065] Each descriptor in a discovery result MUST be the descriptor of the latest
announcement the implementation accepted for the session, or, for a session of its own, the
descriptor it currently announces.

A descriptor never comes from `content`, from a display form or from an alias (§4.7, §6.2).

[SC-DLV-066] An implementation MUST NOT make a session's presence records available to a
peer that is not authorized to discover the session.

An announcement reveals that a session exists and what it accepts. [SC-DLV-066] binds
whatever carries the records. A transport binding meets it by delivering presence records
only to authorized peers, or by carrying them so that no other peer can read them. Which of
the two it does is the binding's concern.

[SC-DLV-067] The implementation that holds a session's binding MUST apply discovery
authorization for that session, including its working-directory scoping, before any
presence record or discovery result for that session leaves it.

Only that implementation knows the session's working directory: [SC-DLV-032] and
[SC-ID-045] keep it out of everything it sends. A consumer on another implementation
therefore cannot apply working-directory scoping to a remote session, and the issuer has to.

*Dated note, 2026-10-03 (#43): Decision C7 (`docs/planning/decisions/`; §7) ships local mode
with no transport-layer authorization, as a deliberate exception to default deny, and full
end-to-end encryption is out of scope for this revision (`docs/planning/ADR-001.md`, "v0.1
scope"). Neither of the two ways to meet [SC-DLV-066] across implementations therefore
exists in v0.1. With the #43 decision that presence and discovery stay within one
implementation until `spec/security.md` (E5, #45) authenticates presence records
([SC-DLV-043]), v0.1 presence and discovery are same-install only, and discovery
authorization, including working-directory scoping, is enforced by the implementation that
holds the session's binding ([SC-DLV-067]). The transport options of [SC-DLV-066] apply once
records cross implementations, after E5. This is a ruling recorded on #43.*

#### 7.3.3 Capability declarations for sending

This subsection is how a sender comes to hold the capability declaration that [SC-ID-086]
requires.

[SC-DLV-070] A sender MUST take the capability declaration it uses under §6.5 and §6.6
only from the descriptor of the latest announcement it accepted for the addressed session,
or, for a session of its own, from the descriptor it currently announces.

A sender therefore holds a declaration for a session exactly when it has accepted an
announcement for it, or the session is its own. A reply is a send too (§6.5), so the
replying implementation needs an accepted announcement for the session it answers. A
send-only session, with `active_inbound` set to `false`, cannot receive the reply (§6.6).

[SC-DLV-071] A sender MUST NOT send an envelope to a session whose presence state is not
`online`.

[SC-DLV-072] A sender that refuses a request under [SC-DLV-071] because the session's
presence state is `unknown` MUST report `unknown-destination` (§8.3).

[SC-DLV-073] A sender that refuses a request under [SC-DLV-071] because the session's
presence state is `unreachable` MUST report `destination-unavailable` (§8.3).

[SC-DLV-074] A sender MUST apply [SC-DLV-071] before the checks of §6.5 and §6.6.

The order matches the delivery stage of §8.3.2: whether the session is known, then whether
it is available, then what it accepts. A session that is `unknown` to the sender also has no
declaration, and the sender reports `unknown-destination`, not a negotiation failure.

*Dated note, 2026-10-03 (#43): refusing a send to a session that is `unreachable`, rather
than passing the envelope to a transport in case the session is still there, is an operator
decision recorded on #43. A session that is `unknown` is refused in any case, because the
sender holds no declaration for it ([SC-ID-086]).*

### 7.4 Ordering, loss and duplication

This revision guarantees no order between envelopes, and no delivery. A transport can
reorder, lose or duplicate envelopes: ordering and reliability are optional capabilities of
the transport contract (DESIGN "Transport contract"). Section 8 states that delivery is not
exactly-once, and how duplicates are handled (§8, §8.4; `spec/security.md`). A sender that
needs to know the outcome reads its delivery states (§8.1). A session that needs its
messages taken in a particular order says so in `content`.

Within one envelope, the order of content parts is kept ([SC-ENV-064]). Presence records
carry their own order, `seq` ([SC-DLV-042]).

[SC-DLV-080] A receiver SHOULD hand off the envelopes for one session in the order in which
it accepted them. A receiver that does not deviates: two envelopes that arrived in one order
reach the harness in the other, a reordering the transport did not cause.

What a harness does with input after hand-off is the harness's own. Of the two v0.1
harness surfaces, one documents in-order processing and kept submission order in its gate
run. On the other, order among several queued inputs has not been exercised (§7.1.3).

### 7.5 Conformance fixtures for this section

Section 7 fixtures live in `tests/protocol/sc-dlv/` and use the members of §3.3, with the
two stages below. For those, `context`, `input` and `expected` hold the members below
instead of the ones §3.3 lists. Implemented lists and attachment lists are as §6.10 defines
them. In both stages, every presence record is taken as authenticated ([SC-DLV-043]) and
every other check of `spec/security.md` as passed.

**Stage `presence`** (§7.2, §7.3.3).

| Member | Content |
|---|---|
| `context` | `implemented`: an implemented list. `own_sessions`: an object whose members are the session ids the consumer binds itself, each a session descriptor. |
| `input` | `events`: an array, in the order the consumer receives them, of objects of two kinds: a record event, with `at_ms` (the consumer's clock, in milliseconds), `issuer` (a label for the issuing implementation) and `record` (a presence record, as a JSON value); and a carrier-loss event, with `at_ms` and `carrier_lost` (an issuer label). `query_at_ms`: the consumer's clock when the states are read, not earlier than any `at_ms`. Optionally `send`: an object with `from` (one of `own_sessions`), `to` (a session id) and `content` (as §4.5), a send request made at `query_at_ms`. |
| `expected` | `discarded`: the zero-based indexes, in `events`, of the record events the consumer discards. `states`: an object whose members are session ids and whose values are the presence states at `query_at_ms`. With `send` in the input, `send`: an object with `result` (`sent` or `refused`), with `sent` also `version` (the envelope's `version`), and with `refused` also `error` (the §8.3 code). |

**Stage `discovery`** (§7.3.2).

| Member | Content |
|---|---|
| `context` | `attachments`: an attachment list. `sessions`: an object whose members are the session ids known to the implementation, each an object with `presence` (a presence state) and `descriptor` (the descriptor that [SC-DLV-065] names). `discoverable`: an array of objects with `requester` and `session`, the session-id pairs that pass discovery authorization. |
| `input` | `request`: an object with `attachment`, the attachment on which the discovery request arrives. |
| `expected` | `result`: `listed` or `refused`. With `listed`: `sessions`, the session ids the result lists, in any order, each listed with the descriptor that `context` gives it. With `refused`: `error`, the §8.3 code. |

> **Reference implementation note:** the v0.1 conformance runner (tasks E8 and F12) drives
> both stages through the implementation's presence registry and discovery logic, with a
> scripted clock and scripted events, so no transport and no harness takes part. Whether
> hand-off is active and free of polling needs a running adapter, so those requirements are
> `TODO(fixture)` in Appendix A, owned by the adapter contract suite (task F10), which by its
> own acceptance asserts the no-polling rule.

---

## 8. Delivery receipts, replies, correlation, and errors

*Owned by E4 (#44). Requirement area: `RCP`.* Stub: E4 fills this section without
renumbering it. Sections 4 and 5 already refer to the delivery states `rejected` and
`expired`, and to the error taxonomy, defined here.

### 8.1 Delivery states

*Owned by E4 (#44).*

### 8.2 Replies and correlation

*Owned by E4 (#44).* Semantics of `conversation_id`, `reply_to` and `correlation_id`
(§4.4.4, §4.4.5).

### 8.3 Error taxonomy

*Owned by E4 (#44).* The closed set of errors, including the errors for an invalid envelope
(§4), an unsupported part type ([SC-ENV-065]), an unsupported major version ([SC-VER-001])
and expiry ([SC-ENV-100]).

---

## 9. Security considerations

The security model is normative in `spec/security.md` (E5, #45): the identity hierarchy,
signing, replay defence, duplicate suppression, authorization and provenance. This
section adds no requirement. It lists what sections 4, 5 and 7 contribute:

- The trust partition (§4.7) gives trusted metadata a single structural home, so content
  never poses as provenance.
- The identifier-token grammar (§4.3) stops a header value from carrying a line break or
  forged field into a provenance rendering.
- Rejecting unsupported part types ([SC-ENV-065]) stops a receiver from delivering a
  message that differs from the one the sender signed.
- The closed `security` object (§4.6) leaves nothing in trusted metadata that a verifier
  does not understand.
- Expiry (§4.9) is the sender's own bound. It does not replace the replay window of
  `spec/security.md`.
- `seq` ordering of presence records ([SC-DLV-042]) stops a delayed or duplicated record
  from undoing a later one, while the consumer remembers the session. Replay across a
  consumer restart is left to `spec/security.md` (§7.2.3 dated note).
- Presence records from another implementation count only once authenticated
  ([SC-DLV-043]). Carrier loss is not authentication and can only make a session
  `unreachable` (§7.2.4).
- Discovery is default-deny per requester ([SC-DLV-061]), scoped by the implementation
  that holds the binding ([SC-DLV-067]), and presence records reach only authorized peers
  ([SC-DLV-066]).

---

## 10. References

### 10.1 Normative references

- [RFC2119] Bradner, S., "Key words for use in RFCs to Indicate Requirement Levels",
  BCP 14, RFC 2119. https://www.rfc-editor.org/rfc/rfc2119
- [RFC8174] Leiba, B., "Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words",
  BCP 14, RFC 8174. https://www.rfc-editor.org/rfc/rfc8174 (boilerplate in §2.1 quoted
  from RFC 8174 §2, retrieved 2026-10-03)
- [RFC8259] Bray, T., Ed., "The JavaScript Object Notation (JSON) Data Interchange
  Format", STD 90, RFC 8259. https://www.rfc-editor.org/rfc/rfc8259
- [RFC7493] Bray, T., Ed., "The I-JSON Message Format", RFC 7493.
  https://www.rfc-editor.org/rfc/rfc7493 (§2.1 and §2.3 rules as summarized in §4.1,
  retrieved 2026-10-03)
- [RFC3339] Klyne, G. and C. Newman, "Date and Time on the Internet: Timestamps",
  RFC 3339. https://www.rfc-editor.org/rfc/rfc3339
- [RFC4648] Josefsson, S., "The Base16, Base32, and Base64 Data Encodings", RFC 4648.
  https://www.rfc-editor.org/rfc/rfc4648
- `spec/security.md`, OAC Session Channels security (E5, #45; not yet written).

### 10.2 Informative references

- `docs/planning/ADR-001.md` and `docs/planning/ADR-001-AMENDMENTS.md`.
- `docs/planning/DESIGN.md`, "Preliminary message envelope".
- `docs/planning/decisions/C3-spec-packaging.md` (extension identifier, versioning rule).
- `docs/planning/decisions/C4-session-identity.md` (session identity, revised by #236).
- `docs/planning/decisions/C5-envelope-auth.md` (signed field set, replay window).
- `docs/planning/decisions/C6-trust-rendering.md` (§5.0, provenance framing).
- Decision C13, provenance framing (`docs/planning/decisions/`; §4, the whole-value rule
  §4.3 adopts).
- `docs/planning/v0.1/05-interfaces.md` §3 and §11, the M0 draft this document supersedes.
- `docs/planning/decisions/C2-process-model.md` (§5, presence lifetime).
- Decision C7, the v0.1 transport mapping (`docs/planning/decisions/`; §4 presence, §7 the
  local-mode authorization carve-out).
- `docs/planning/gates/G1-result.md`, `docs/planning/gates/G2-result.md` and
  `docs/planning/gates/G3-result.md` (harness input during a running turn; peer discovery).
- `docs/planning/REVERIFICATION-B2.md` (§3.1 re-check, row 5).

---

## Appendix A. Requirement index

Fixture paths are relative to `tests/protocol/`. `TODO(fixture)` marks a requirement with
no conformance fixture yet, and names the task expected to supply the test. `covered by`
marks a requirement that no fixture can break on its own at its stage, and names the
requirement whose fixtures exercise it.

| Id | Level | Section | Fixtures |
|---|---|---|---|
| SC-ENV-001 | MUST | 4.1 | `sc-env/SC-ENV-001.p01`, `.n01` |
| SC-ENV-002 | MUST | 4.1 | `sc-env/SC-ENV-002.n01` to `.n06` |
| SC-ENV-003 | MUST NOT | 4.1 | `sc-env/SC-ENV-003.n01` to `.n04` |
| SC-ENV-004 | SHOULD | 4.1 | none (SHOULD) |
| SC-ENV-005 | SHOULD NOT | 4.1 | none (SHOULD NOT) |
| SC-ENV-010 | MUST | 4.3 | `sc-env/SC-ENV-010.p01`, `.p02`, `.n01` to `.n08` |
| SC-ENV-011 | MUST | 4.3 | `sc-env/SC-ENV-011.n01`, `.n02` |
| SC-ENV-020 | MUST | 4.4.1 | `sc-env/SC-ENV-020.n01` to `.n04` |
| SC-ENV-021 | MUST | 4.4.1 | TODO(fixture): sender-side behaviour; E8 |
| SC-ENV-022 | MUST | 4.4.2 | `sc-env/SC-ENV-022.n01` |
| SC-ENV-023 | MUST | 4.4.2 | TODO(fixture): sender-side behaviour; F2 unit tests |
| SC-ENV-024 | SHOULD | 4.4.2 | none (SHOULD) |
| SC-ENV-025 | MUST | 4.4.3 | `sc-env/SC-ENV-025.n01` |
| SC-ENV-026 | MUST | 4.4.3 | `sc-env/SC-ENV-026.n01` |
| SC-ENV-027 | MUST NOT | 4.4.2 | TODO(fixture): receiver bookkeeping; E4 correlation, F4 duplicate suppression |
| SC-ENV-030 | MUST NOT | 4.4.5 | `sc-env/SC-ENV-030.p01` |
| SC-ENV-031 | MAY | 4.4.5 | none (MAY) |
| SC-ENV-040 | MUST | 4.4.6 | `sc-env/SC-ENV-040.n01` |
| SC-ENV-041 | MUST | 4.4.6 | `sc-env/SC-ENV-041.p01`, `.p02`, `.n01` to `.n06` |
| SC-ENV-050 | MUST | 4.4.7 | `sc-env/SC-ENV-050.p01`, `.p02`, `.n01` to `.n05` |
| SC-ENV-051 | MAY | 4.4.7 | none (MAY) |
| SC-ENV-060 | MUST | 4.5 | `sc-env/SC-ENV-060.n01`, `.n02`, `.n03` |
| SC-ENV-061 | MUST | 4.5 | `sc-env/SC-ENV-061.n01`, `.n02` |
| SC-ENV-062 | MUST | 4.5.1 | `sc-env/SC-ENV-062.p01`, `.n01`, `.n02`, `.n03` |
| SC-ENV-063 | MUST | 4.5.1 | `sc-env/SC-ENV-063.p01` |
| SC-ENV-064 | MUST | 4.5.1 | TODO(fixture): hand-off behaviour; F10 adapter contract suite |
| SC-ENV-065 | MUST | 4.5.2 | `sc-env/SC-ENV-065.n01`, `.n02`, `.n03` |
| SC-ENV-066 | MUST NOT | 4.5.2 | TODO(fixture): sender-side; E8. §6.4 now defines "advertised" and `sc-id/SC-ID-101` exercises it |
| SC-ENV-070 | MUST | 4.6 | `sc-env/SC-ENV-070.n01`, `.n02`, `.n03` |
| SC-ENV-071 | MUST | 4.6 | `sc-env/SC-ENV-071.n01` |
| SC-ENV-072 | MUST | 4.6 | `sc-env/SC-ENV-072.n01` |
| SC-ENV-073 | MUST NOT | 4.6 | `sc-env/SC-ENV-073.n01` |
| SC-ENV-080 | MUST | 4.7 | `sc-env/SC-ENV-080.p01` |
| SC-ENV-081 | MUST NOT | 4.7 | TODO(fixture): sender-side, by construction; F2 |
| SC-ENV-082 | MUST NOT | 4.7 | TODO(fixture): provenance rendering; E5, F11 |
| SC-ENV-083 | MUST NOT | 4.7 | TODO(fixture): verification order; E5 signature vectors |
| SC-ENV-090 | MUST | 4.8 | `sc-env/SC-ENV-090.p01` |
| SC-ENV-091 | MUST NOT | 4.8 | TODO(fixture): hand-off behaviour; F10 |
| SC-ENV-092 | MUST NOT | 4.8 | TODO(fixture): provenance rendering; E5, F11 |
| SC-ENV-100 | MUST NOT | 4.9 | `sc-env/SC-ENV-100.p01`, `.n01`, `.n02` |
| SC-ENV-101 | MUST | 4.9 | TODO(fixture): held-envelope timing; F6 receipt state machine |
| SC-ENV-102 | MUST | 4.9 | TODO(fixture): sender-side behaviour; F2 |
| SC-ENV-103 | MUST | 4.9 | TODO(fixture): sender-side behaviour; F2 |
| SC-ENV-104 | MUST | 4.9 | TODO(fixture): sender-side behaviour; F2, E5 |
| SC-VER-001 | MUST | 5.4 | `sc-ver/SC-VER-001.n01`, `.n02` |
| SC-VER-002 | MUST NOT | 5.4 | `sc-ver/SC-VER-002.p01` |
| SC-VER-003 | MUST | 5.4 | `sc-ver/SC-VER-003.n01` |
| SC-ID-001 | MUST | 6.1 | `sc-id/SC-ID-001.p01`, `.p02`, `.n01`, `.n02`, `.n03`, `.n04`, `.n05`, `.n06` |
| SC-ID-002 | MUST | 6.1 | `sc-id/SC-ID-002.n01`, `.n02`, `.n03` |
| SC-ID-003 | MUST | 6.1 | TODO(fixture): sender-side randomness; F2 unit tests |
| SC-ID-004 | MUST NOT | 6.1 | TODO(fixture): sender-side, by construction; F2, F11 |
| SC-ID-005 | MUST NOT | 6.1 | TODO(fixture): receiver bookkeeping; F11 |
| SC-ID-006 | MUST NOT | 6.1 | TODO(fixture): sender-side; F2 (`sc-id/SC-ID-001.n05` shows a receiver rejecting one such value) |
| SC-ID-007 | MUST NOT | 6.1 | `sc-id/SC-ID-007.p01` |
| SC-ID-008 | MUST NOT | 6.1 | TODO(fixture): sender-side; F2 |
| SC-ID-009 | MUST | 6.1 | TODO(fixture): registration-record signature vectors; E5, E8 |
| SC-ID-020 | MUST NOT | 6.2 | covered by SC-ENV-010: every display form contains `/`, which no identifier token allows, so no fixture breaks this rule alone |
| SC-ID-021 | MUST NOT | 6.2 | TODO(fixture): routing and authorization paths; F11, H2 |
| SC-ID-022 | MUST NOT | 6.2 | covered by SC-ID-001 (`sc-id/SC-ID-001.n06`): an alias that is not a session id fails it, and one shaped like a session id cannot be told apart on the wire |
| SC-ID-023 | MUST NOT | 6.2 | TODO(fixture): sender-side; F2 |
| SC-ID-024 | MUST NOT | 6.2 | TODO(fixture): authorization store; F5 |
| SC-ID-040 | MUST | 6.3 | covered by SC-DLV-029 (`sc-dlv/SC-DLV-029.n02`): a consumer can only show a descriptor without `session_id` being discarded with its presence record |
| SC-ID-041 | MUST | 6.3 | covered by SC-DLV-029 (`sc-dlv/SC-DLV-029.n01`): a consumer can only show a descriptor without `capabilities` being discarded with its presence record |
| SC-ID-042 | MAY | 6.3 | none (MAY) |
| SC-ID-043 | MUST NOT | 6.3 | TODO(fixture): routing and authorization paths; F11 |
| SC-ID-044 | MUST | 6.3 | `sc-id/SC-ID-044.p01` |
| SC-ID-045 | MUST NOT | 6.3 | TODO(fixture): issuer-side, descriptors carried in presence records (§7.2.2); F6, H2 |
| SC-ID-060 | MUST | 6.4 | covered by SC-ID-070 (`sc-id/SC-ID-070.n01`, `.n02`): a consumer can only show a non-object declaration holding no entries |
| SC-ID-061 | MUST | 6.4 | `sc-id/SC-ID-061.n01`, `.n02` |
| SC-ID-062 | MUST | 6.4 | `sc-id/SC-ID-062.n01`, `.n02` |
| SC-ID-063 | MAY | 6.4 | none (MAY) |
| SC-ID-064 | MUST | 6.4 | `sc-id/SC-ID-064.n01`, `.n02` |
| SC-ID-065 | MAY | 6.4 | none (MAY) |
| SC-ID-066 | MUST | 6.4 | `sc-id/SC-ID-066.p01`, `.n01`, `.n02` |
| SC-ID-067 | MUST | 6.4 | `sc-id/SC-ID-067.p01` |
| SC-ID-068 | MUST | 6.4 | `sc-id/SC-ID-068.n01` |
| SC-ID-069 | MUST NOT | 6.4 | `sc-id/SC-ID-069.p01` |
| SC-ID-070 | MUST | 6.4 | `sc-id/SC-ID-070.n01`, `.n02` |
| SC-ID-080 | MUST | 6.5 | TODO(fixture): declaration carriage; E3 discovery, E6 binding |
| SC-ID-081 | MUST NOT | 6.5 | TODO(fixture): declarer-side; F10 adapter contract suite |
| SC-ID-082 | MUST | 6.5 | `sc-id/SC-ID-082.p01`, `.p02`, `.n01` |
| SC-ID-083 | MUST | 6.5 | `sc-id/SC-ID-083.p01` |
| SC-ID-084 | MUST NOT | 6.5 | `sc-id/SC-ID-084.n01` |
| SC-ID-085 | MUST NOT | 6.5 | `sc-id/SC-ID-085.p01`, `.p02` |
| SC-ID-086 | MUST NOT | 6.5 | `sc-id/SC-ID-086.n01` |
| SC-ID-087 | MUST | 6.5 | `sc-id/SC-ID-087.p01` |
| SC-ID-100 | MUST NOT | 6.6 | `sc-id/SC-ID-100.n01` |
| SC-ID-101 | MUST | 6.6 | `sc-id/SC-ID-101.p01`, `.p02`, `.n01` |
| SC-ID-102 | MUST | 6.6 | TODO(fixture): needs the §8.3 error taxonomy; E4, F10 |
| SC-ID-103 | MUST NOT | 6.6 | TODO(fixture): sender-side; F2 |
| SC-ID-104 | MUST NOT | 6.6 | TODO(fixture): needs a live harness; F10 adapter contract suite, E3 |
| SC-ID-105 | MUST | 6.6 | TODO(fixture): hand-off behaviour; F10 |
| SC-ID-120 | MUST | 6.7.2 | TODO(fixture): needs a live harness; F10 adapter contract suite |
| SC-ID-121 | MUST | 6.7.2 | TODO(fixture): needs a live harness and the pairing mechanism (UNVERIFIED); Epic F adapter work, G9 |
| SC-ID-122 | MUST NOT | 6.7.2 | TODO(fixture): needs a live harness; F11, G9 |
| SC-ID-123 | MUST | 6.7.2 | TODO(fixture): window timing; F10 |
| SC-ID-124 | MUST | 6.7.2 | `sc-id/SC-ID-124.n01` |
| SC-ID-125 | MUST NOT | 6.7.2 | `sc-id/SC-ID-125.n01` |
| SC-ID-126 | MUST NOT | 6.7.2 | TODO(fixture): needs a live harness; F11 |
| SC-ID-127 | MUST NOT | 6.7.2 | TODO(fixture): needs a live harness; F11, G9 |
| SC-ID-128 | MUST | 6.7.2 | `sc-id/SC-ID-128.n01` |
| SC-ID-129 | MUST | 6.7.2 | `sc-id/SC-ID-129.n01` |
| SC-ID-130 | MUST | 6.7.3 | `sc-id/SC-ID-130.p01`, `.p02` |
| SC-ID-131 | MUST NOT | 6.7.3 | `sc-id/SC-ID-131.n01` |
| SC-ID-132 | MUST | 6.7.3 | `sc-id/SC-ID-132.n01` |
| SC-ID-133 | MUST NOT | 6.7.3 | `sc-id/SC-ID-133.n01` |
| SC-ID-134 | MUST | 6.7.3 | `sc-id/SC-ID-134.p01` |
| SC-ID-135 | MUST | 6.7.3 | `sc-id/SC-ID-135.n01`, `.n02` |
| SC-ID-136 | MUST | 6.7.3 | `sc-id/SC-ID-136.p01` |
| SC-ID-137 | MUST NOT | 6.7.3 | `sc-id/SC-ID-137.n01`, `.n02` |
| SC-ID-138 | MUST | 6.7.3 | `sc-id/SC-ID-138.n01` |
| SC-ID-139 | MUST | 6.7.3 | `sc-id/SC-ID-139.p01` |
| SC-ID-140 | MUST | 6.7.3 | `sc-id/SC-ID-140.p01` |
| SC-ID-141 | MUST | 6.7.3 | `sc-id/SC-ID-141.p01` |
| SC-ID-142 | MUST | 6.7.3 | `sc-id/SC-ID-142.p01` |
| SC-ID-143 | MUST | 6.7.3 | `sc-id/SC-ID-143.n01` |
| SC-ID-144 | MUST NOT | 6.7.2 | TODO(fixture): sender-side; F2 |
| SC-ID-150 | MUST | 6.7.4 | `sc-id/SC-ID-150.p01` |
| SC-ID-151 | MUST NOT | 6.7.4 | TODO(fixture): authorization store; E5, F5 |
| SC-ID-152 | MUST | 6.7.4 | `sc-id/SC-ID-152.n01`, `.n02` |
| SC-ID-153 | MUST | 6.7.4 | `sc-id/SC-ID-153.n01` |
| SC-ID-154 | MUST | 6.7.4 | TODO(fixture): needs attribution of a dropped signal (UNVERIFIED); F10, G9 |
| SC-ID-155 | MUST | 6.7.5 | TODO(fixture): needs a live attachment; F10 |
| SC-ID-156 | MUST NOT | 6.7.5 | TODO(fixture): restart behaviour; F2 |
| SC-ID-157 | MUST NOT | 6.7.5 | TODO(fixture): authorization; E5, F5 |
| SC-ID-160 | MUST | 6.8 | `sc-id/SC-ID-160.p01` |
| SC-ID-161 | MUST | 6.8 | `sc-id/SC-ID-161.n01`, `.n02` |
| SC-ID-162 | MUST NOT | 6.8 | `sc-id/SC-ID-162.p01` |
| SC-ID-180 | MUST NOT | 6.9 | TODO(fixture): presence registry; F6 |
| SC-ID-181 | MUST NOT | 6.9 | TODO(fixture): registration-record verification; E5 |
| SC-ID-182 | MUST NOT | 6.9 | TODO(fixture): presence registry and discovery; F6, F11 |
| SC-DLV-001 | MUST | 7.1.2 | TODO(fixture): needs a running adapter; F10 adapter contract suite |
| SC-DLV-002 | MUST NOT | 7.1.2 | TODO(fixture): needs a running adapter; F10 (its acceptance asserts the no-polling rule) |
| SC-DLV-003 | MUST NOT | 7.1.2 | TODO(fixture): surface listing per binding; F10, and `spec/bindings/mcp.md` MCPB-DLV-002 |
| SC-DLV-004 | MAY | 7.1.2 | none (MAY) |
| SC-DLV-005 | MUST NOT | 7.1.2 | TODO(fixture): by construction and review; F10, boundary lint checks 4-5 (`oac-boundaries`) |
| SC-DLV-006 | MUST NOT | 7.1.3 | TODO(fixture): needs a harness that takes input mid-turn; F10 against the F8 and F9 fakes |
| SC-DLV-007 | MUST NOT | 7.1.3 | TODO(fixture): the report is E4's delivery stage (§8.5, once #44 lands); that no copy is held needs a timed run; F6, F10 |
| SC-DLV-008 | MUST | 7.1.3 | TODO(fixture): needs a surface that turns a call away; F10 against the F8 and F9 fakes |
| SC-DLV-009 | MUST | 7.1.3 | TODO(fixture): needs a surface that reports a failed call; F10 against the F8 and F9 fakes |
| SC-DLV-020 | MUST NOT | 7.2.1 | TODO(fixture): presence registry output; F6 (every `sc-dlv` presence fixture expects only the three states) |
| SC-DLV-021 | MUST | 7.2.2 | `sc-dlv/SC-DLV-021.n01` |
| SC-DLV-022 | MUST | 7.2.2 | `sc-dlv/SC-DLV-022.n01`, `.n02` |
| SC-DLV-023 | MUST | 7.2.2 | `sc-dlv/SC-DLV-023.n01` |
| SC-DLV-024 | MUST | 7.2.2 | `sc-dlv/SC-DLV-024.p01`, `.n01`, `.n02` |
| SC-DLV-025 | MUST | 7.2.2 | `sc-dlv/SC-DLV-025.n01` |
| SC-DLV-026 | MUST | 7.2.2 | `sc-dlv/SC-DLV-026.n01` |
| SC-DLV-027 | MUST | 7.2.2 | `sc-dlv/SC-DLV-027.n01`, `.n02` |
| SC-DLV-028 | MUST | 7.2.2 | `sc-dlv/SC-DLV-028.p01`, `.n01`, `.n02` |
| SC-DLV-029 | MUST | 7.2.2 | `sc-dlv/SC-DLV-029.p01`, `.n01`, `.n02` |
| SC-DLV-030 | MUST | 7.2.2 | `sc-dlv/SC-DLV-030.n01` |
| SC-DLV-031 | MUST NOT | 7.2.2 | `sc-dlv/SC-DLV-031.n01` |
| SC-DLV-032 | MUST NOT | 7.2.2 | TODO(fixture): issuer-side; F6, H2 (cross-project disclosure) |
| SC-DLV-040 | MUST | 7.2.3 | covered by the negative fixtures of SC-DLV-021 to SC-DLV-031, each of which expects the record discarded |
| SC-DLV-041 | MUST | 7.2.3 | `sc-dlv/SC-DLV-041.p01` |
| SC-DLV-042 | MUST | 7.2.3 | `sc-dlv/SC-DLV-042.p01`, `.n01`, `.n02` |
| SC-DLV-043 | MUST NOT | 7.2.3 | TODO(fixture): presence-record authentication vectors; E5, E8 |
| SC-DLV-044 | MUST | 7.2.4 | `sc-dlv/SC-DLV-044.p01` |
| SC-DLV-045 | MUST | 7.2.4 | `sc-dlv/SC-DLV-045.p01`, `.n01` |
| SC-DLV-046 | MUST | 7.2.4 | `sc-dlv/SC-DLV-046.n01` |
| SC-DLV-047 | MUST | 7.2.4 | `sc-dlv/SC-DLV-047.p01` |
| SC-DLV-048 | MAY | 7.2.4 | none (MAY) |
| SC-DLV-049 | SHOULD | 7.2.4 | none (SHOULD) |
| SC-DLV-050 | MUST | 7.2.5 | TODO(fixture): issuer-side; F6 |
| SC-DLV-051 | MUST | 7.2.5 | TODO(fixture): issuer-side, needs a live attachment; F6, F10 |
| SC-DLV-052 | MUST | 7.2.5 | TODO(fixture): issuer-side; F6 |
| SC-DLV-053 | MUST | 7.2.5 | TODO(fixture): issuer-side; F6, F10 |
| SC-DLV-054 | SHOULD | 7.2.5 | none (SHOULD) |
| SC-DLV-055 | MUST | 7.2.5 | TODO(fixture): needs a live attachment ending; F6, F10 |
| SC-DLV-056 | SHOULD | 7.2.5 | none (SHOULD) |
| SC-DLV-057 | SHOULD | 7.2.5 | none (SHOULD) |
| SC-DLV-060 | MUST | 7.3.2 | `sc-dlv/SC-DLV-060.n01` |
| SC-DLV-061 | MUST NOT | 7.3.2 | `sc-dlv/SC-DLV-061.n01` |
| SC-DLV-062 | MUST | 7.3.2 | `sc-dlv/SC-DLV-062.p01`, `.p02` |
| SC-DLV-063 | MUST NOT | 7.3.2 | `sc-dlv/SC-DLV-063.n01` |
| SC-DLV-064 | MUST NOT | 7.3.2 | covered by SC-DLV-062: every `listed` fixture expects each session id once, so no fixture breaks this rule alone |
| SC-DLV-065 | MUST | 7.3.2 | covered by SC-DLV-062: every `listed` fixture expects each descriptor exactly as `context` gives it; which announcement is latest is SC-DLV-070's `sc-dlv/SC-DLV-070.n01` |
| SC-DLV-066 | MUST NOT | 7.3.2 | TODO(fixture): needs a transport binding's carriage, after E5; F11, H2 |
| SC-DLV-067 | MUST | 7.3.2 | TODO(fixture): authorization store and working-directory scope; F5, F11, H2 |
| SC-DLV-070 | MUST | 7.3.3 | `sc-dlv/SC-DLV-070.p01`, `.n01` |
| SC-DLV-071 | MUST NOT | 7.3.3 | `sc-dlv/SC-DLV-071.n01` |
| SC-DLV-072 | MUST | 7.3.3 | `sc-dlv/SC-DLV-072.n01` |
| SC-DLV-073 | MUST | 7.3.3 | `sc-dlv/SC-DLV-073.n01` |
| SC-DLV-074 | MUST | 7.3.3 | `sc-dlv/SC-DLV-074.n01` |
| SC-DLV-080 | SHOULD | 7.4 | none (SHOULD) |

Retired ids: none.

## Appendix B. Revision history

| Revision | Date | Change |
|---|---|---|
| 0.1 (draft) | 2026-10-03 | E1 (#41): document skeleton for sections 1-10; sections 4 (envelope) and 5 (versioning) written; requirement-id scheme and fixture format (§3); envelope-stage fixtures under `tests/protocol/sc-env/` and `tests/protocol/sc-ver/`. Review of #258: SC-ENV-027, SC-ENV-103, SC-ENV-104 and SC-VER-003 added (retransmission and retry defined); dated notes for the operator decisions on #41. |
| 0.1 (draft) | 2026-10-03 | E2 (#42): section 6 (session identity, addressing and capability negotiation) written; area `ID`; `negotiation`, `binding` and `send` fixture stages (§6.10, with a §3.3 sentence allowing section-defined stages); fixtures under `tests/protocol/sc-id/`. Review of #260: signal cross-check value, record rules and exact comparison (SC-ID-127 to SC-ID-129, SC-ID-141 to SC-ID-144), SC-ID-045, SC-ID-070, SC-ID-154 made a conditional MUST, SC-ID-023 widened, binding results mapped to cases. |
| 0.1 (draft) | 2026-10-03 | E3 (#43): section 7 (active delivery, presence and discovery) written; area `DLV`; the active-inbound obligation and the no-polling rule, accepting input, the three presence states, presence records (announcement and withdrawal, `seq`, consumer-clock lifetime, carrier loss), discovery results, and where a sender takes a capability declaration from (makes SC-ID-086 satisfiable); `presence` and `discovery` fixture stages (§7.5); fixtures under `tests/protocol/sc-dlv/`; SC-ID-040 and SC-ID-041 now covered by SC-DLV-029. Review of #263: operator decisions on #43 recorded as dated notes; SC-DLV-008, SC-DLV-009 (not-now vs failed hand-off), SC-DLV-049 (monotonic clock) and SC-DLV-067 (scoping by the binding holder; v0.1 same-install only) added; evidence for input during a running turn corrected; E5 constraints recorded; `sc-id/SC-ID-044.p01` added. |
