# OAC Session Channels

**Document:** `spec/session-channels.md`, the normative OAC Session Channels specification.
**Revision:** 0.1 (draft, Stage 2). Sections 4 and 5 are written (E1, #41). Sections 6, 7
and 8 are titled stubs that tasks E2 (#42), E3 (#43) and E4 (#44) fill.
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

*Owned by E2 (#42). Requirement area: `ID`.* Stub: E2 fills this section without
renumbering it. Source: `docs/planning/decisions/C4-session-identity.md` (as revised by
#236), C3, and the backlog E2 acceptance criteria.

### 6.1 Session identifiers

*Owned by E2 (#42).* The opaque, stable session id bound to a device key. Its syntax is a
subset of the identifier token (§4.3), as §4.4.3 requires.

### 6.2 Display form and human aliases

*Owned by E2 (#42).* The non-authoritative display form and local aliases.

### 6.3 Session descriptor

*Owned by E2 (#42).*

### 6.4 Session capabilities

*Owned by E2 (#42).* Includes whether active inbound delivery is supported, the content
part types a session accepts ([SC-ENV-066]), and any envelope size limit above the default
of [SC-ENV-004].

### 6.5 Version and extension negotiation

*Owned by E2 (#42).* Builds on §5.

### 6.6 Unsupported-capability behaviour

*Owned by E2 (#42).*

---

## 7. Active delivery, presence, and discovery

*Owned by E3 (#43). Requirement area: `DLV`.* Stub: E3 fills this section without
renumbering it.

### 7.1 The active-inbound obligation and the no-polling rule

*Owned by E3 (#43).*

### 7.2 Presence

*Owned by E3 (#43).*

### 7.3 Discovery

*Owned by E3 (#43).*

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
section adds no requirement. It lists what sections 4 and 5 contribute:

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

---

## Appendix A. Requirement index

Fixture paths are relative to `tests/protocol/`. `TODO(fixture)` marks a requirement with
no conformance fixture yet, and names the task expected to supply the test.

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
| SC-ENV-066 | MUST NOT | 4.5.2 | TODO(fixture): needs the §6.4 capability model; E2, E8 |
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

Retired ids: none.

## Appendix B. Revision history

| Revision | Date | Change |
|---|---|---|
| 0.1 (draft) | 2026-10-03 | E1 (#41): document skeleton for sections 1-10; sections 4 (envelope) and 5 (versioning) written; requirement-id scheme and fixture format (§3); envelope-stage fixtures under `tests/protocol/sc-env/` and `tests/protocol/sc-ver/`. Review of #258: SC-ENV-027, SC-ENV-103, SC-ENV-104 and SC-VER-003 added (retransmission and retry defined); dated notes for the operator decisions on #41. |
