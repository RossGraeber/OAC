# OAC Session Channels

**Document:** `spec/session-channels.md`, the normative OAC Session Channels specification.
**Revision:** 0.1 (draft, Stage 2). Sections 4 and 5 are written (E1, #41), and so is
section 8 (E4, #44). Sections 6 and 7 are titled stubs that tasks E2 (#42) and E3 (#43)
fill.
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
| `expected` | `result` (one of `valid`, `rejected`, `expired`); for a negative fixture, `error` (the §8.3 error code the receiver reports); and optionally `trusted_security` (the `security` member values a receiver extracts, §4.7). Section 8.5 adds the members of the stages §8 defines. |

An envelope-stage fixture's `security` members hold placeholder strings. Envelope-stage
validation checks their presence and type (§4.6), never their values. Signature vectors and
replay cases are `spec/security.md` fixtures (E5, E8).

The `result` values `rejected` and `expired` are the delivery states that §8.1 defines.
Each negative envelope-stage fixture carries `expected.error`, the code from the closed
error taxonomy of §8.3 that the receiver reports ([SC-RCP-070], [SC-RCP-071]).

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
message content again as a new envelope. A receiver that has handed off the original, may
have handed it off, or is handing it off treats a retransmission as a duplicate
([SC-RCP-009], `spec/security.md`).

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

*Owned by E4 (#44). Requirement area: `RCP`.* Sources: `docs/planning/DESIGN.md`,
"Delivery semantics"; `docs/planning/decisions/C5-envelope-auth.md` §7-§9 (conflict C6);
`docs/planning/decisions/C6-trust-rendering.md` §8, §10 and §11 (conflict C9). This section
supersedes the M0 draft, `docs/planning/v0.1/05-interfaces.md` §8-§10.

This section defines what an implementation reports about a message after it is sent, how
a reply names the message it answers, and which errors exist. One rule runs through it: an
implementation reports only what it can observe.

This document does not promise exactly-once delivery. A message can be handed off more
than once, for example when a receiver restarts and loses its duplicate-suppression state
(`docs/planning/decisions/C5-envelope-auth.md` §8). This document requires these measures
instead: unique envelope ids ([SC-ENV-023]), unchanged retransmissions ([SC-ENV-102]),
duplicate suppression (`spec/security.md`), and the receipt and retry rules below.

### 8.1 Delivery states

#### 8.1.1 Observers

Two implementations take part in one delivery:

- the **sending implementation**, which serves the sending session, creates the envelope
  and passes it to a transport;
- the **receiver** (§2.3), which serves the addressed session and hands the content off to
  that session's harness.

Each delivery state is seen first-hand by one of them, its **observer**. On one device the
two roles can belong to the same implementation. The states keep the same meaning.

#### 8.1.2 The state set

Table 8.1. The last column says what the state asserts about hand-off of this copy of the
envelope.

| State | Observer | Meaning | Handed off |
|---|---|---|---|
| `accepted-by-adapter` | sending implementation | The sending implementation created a valid envelope for the message and passed it to a transport. | not known |
| `handed-to-harness` | receiver | The receiver passed the content to the harness through the harness's supported input surface, and the input call completed successfully as that surface defines completion. For a surface that returns no response, completion is the end of the write to it. | yes |
| `unknown` | either | The observer cannot determine whether the content was handed off. | not known |
| `rejected` | receiver | The receiver refused the envelope: it failed validation, verification, authorization or a capability check. | no |
| `expired` | receiver | The receiver did not hand off the envelope because its expiry instant (§4.9) or the replay window of `spec/security.md` had passed. | no |
| `duplicate` | receiver | The receiver recognized the envelope as a copy of one that it has handed off, may have handed off, or is handing off ([SC-RCP-009]). | no, not this copy |
| `unreachable` | either | The addressed session is unknown to the observer, or is known but not accepting input. | no |
| `failed` | either | An error unrelated to the envelope's validity stopped delivery, and the observer knows that the content was not handed off. | no |

DESIGN "Delivery semantics" lists `accepted`, `rejected`, `unreachable`, `expired`,
`duplicate` and `failed`. This set keeps the last five. It splits `accepted` into
`accepted-by-adapter`, `handed-to-harness` and `unknown`, as
`docs/planning/decisions/C5-envelope-auth.md` §9 requires to resolve conflict C6. A bare
`accepted` could mean that an implementation took the message or that the harness got it,
and those are different facts.

*Dated note, 2026-10-03 (#44): C5 §9 describes `accepted-by-adapter` as acceptance on the
receiving side, after verification and authorization. C6 §8 returns it as the result of a
send, before any receiver has the envelope. This section adopts the C6 §8 reading, because
a sending implementation can observe it on every topology. Receiver-side acceptance is not
reported as a state of its own: a receiver reports the outcome, which is
`handed-to-harness`, `unknown` or an error state.*

[SC-RCP-001] A receipt (§8.1.4) MUST carry exactly one state from Table 8.1.

[SC-RCP-002] A receipt MUST NOT carry a state that Table 8.1 does not allow for the
receipt's observer.

[SC-RCP-003] An implementation MUST NOT report a delivery state that it has not observed.

Relaying a state is not observing it. A sending implementation that passes on a receiver's
report keeps the receiver as the observer (§8.1.5).

#### 8.1.3 What a state proves, and what it does not

No state in Table 8.1 means that a model read, understood, processed or acted on a
message. `handed-to-harness` is the strongest state. It proves only that the input call to
the harness's input surface completed. On a surface that returns no response, that means
only that the write completed, not that the harness acknowledged anything. The harness then
owns the input and decides when, and whether, a model sees it.

This limit comes from the harnesses, not from a choice in this document.
`docs/planning/decisions/C5-envelope-auth.md` §9 fixes the strongest observable point of
each harness surface the v0.1 bindings use as the success of the input call itself:

- On one surface, a send completes when the message is written to the transport, and the
  harness sends no acknowledgement that it processed the input
  (`docs/planning/REVERIFICATION-B2.md` §3.1, re-verified HOLDS against first-party
  documentation retrieved 2026-09-16; `docs/planning/PLANNING-PROMPT.md` §3.1).
- On the other, C5 §9 reads a success response to the input call as confirming that the
  harness accepted the call, not that a model processed the resulting turn. That reading
  is the decision's own; C5 §9 claims no stronger observable point.

[SC-RCP-004] A receiver MUST NOT report `handed-to-harness` before the hand-off call has
completed successfully, as the harness's input surface defines completion.

[SC-RCP-005] An implementation MUST NOT present a delivery state to a user, a harness or a
model as meaning that a model read, processed or acted on the message.

[SC-RCP-006] A receiver whose hand-off call has an indeterminate outcome MUST report
`unknown`.

An outcome is indeterminate when the call returned neither success nor failure, for
example because it timed out or its connection closed. The harness may hold the input.
Reporting `failed` would invite a retry that hands the content off twice.

[SC-RCP-007] A sending implementation MUST NOT report `unreachable` or `failed` from its
own observation for an envelope it has already passed to a transport.

A transport that holds the envelope may still deliver it. From then on, only a receiver's
receipt (§8.1.5) or `unknown` ([SC-RCP-010]) changes the sending implementation's state.

[SC-RCP-008] A receiver MUST NOT hand off an envelope for which it reports `rejected`,
`expired`, `duplicate`, `unreachable` or `failed`.

[SC-RCP-009] A receiver MUST NOT report `duplicate` for an envelope when it reported
`rejected`, `expired`, `unreachable` or `failed` for every earlier copy of that envelope.

`duplicate` therefore means that an earlier copy was handed off, may have been handed off
(`unknown`), or is being handed off now. A copy that was never handed off does not block a
later retransmission (§8.4). How a receiver recognizes a copy is defined in
`spec/security.md`.

An envelope's **hand-off deadline** is the earlier of its expiry instant (§4.9), when it
has one, and the end of the replay window of `spec/security.md` for its `created_at`. The
replay window bounds `created_at` whatever `ttl_ms` says (§4.9), so no receiver can hand off
any copy of an envelope after its hand-off deadline.

[SC-RCP-010] A sending implementation SHOULD report `unknown` for an envelope once its
hand-off deadline has passed, when its combined state (§8.4.1) is still
`accepted-by-adapter`. A sending implementation that keeps reporting `accepted-by-adapter`
after that point deviates: its caller waits for a change that cannot come.

[SC-RCP-011] A sending implementation MAY recompute its reported state when a receipt
arrives after it reported `unknown` under [SC-RCP-010]. A sending implementation that does
not keeps reporting `unknown`, which stays true.

#### 8.1.4 Receipts

A **receipt** reports one delivery state for one envelope. It is a JSON object with these
members:

| Member | Value | Presence |
|---|---|---|
| `envelope_id` | the envelope's `id`, an identifier token | exactly once |
| `envelope_from` | the envelope's `from`, an identifier token | exactly once |
| `state` | a state from Table 8.1 | exactly once |
| `observer` | `sender` (the sending implementation) or `receiver` | exactly once |
| `error` | an error code (§8.3) | exactly when `state` is `rejected`, `expired`, `duplicate`, `unreachable` or `failed` |
| `observed_at` | a timestamp in the form of §4.4.6, by the observer's clock | exactly once |

The envelope is named by `envelope_id` and `envelope_from` together, because an `id` is
unique only per sending session ([SC-ENV-023], [SC-ENV-027]).

Example (informative):

```json
{
  "envelope_id": "msg-01hzx3k8q2v6w9m4t7p0r5s1ya",
  "envelope_from": "01harn7x9k2m4p6q8r0s2t4v6w",
  "state": "rejected",
  "observer": "receiver",
  "error": "unauthorized",
  "observed_at": "2026-10-03T12:00:01.250Z"
}
```

[SC-RCP-020] A receipt MUST satisfy [SC-ENV-001], [SC-ENV-002] and [SC-ENV-003], read with
"receipt" in place of "envelope".

[SC-RCP-021] A receipt MUST contain the members `envelope_id`, `envelope_from`, `state`,
`observer` and `observed_at`.

[SC-RCP-022] The values of `envelope_id` and `envelope_from` MUST be identifier tokens
(§4.3).

[SC-RCP-023] The value of `observer` MUST be the string `sender` or the string `receiver`.

[SC-RCP-024] The value of `observed_at` MUST be a string in the timestamp form of §4.4.6.

[SC-RCP-025] A receipt whose `state` is `rejected`, `expired`, `duplicate`, `unreachable`
or `failed` MUST contain an `error` member.

[SC-RCP-026] A receipt whose `state` is `accepted-by-adapter`, `handed-to-harness` or
`unknown` MUST NOT contain an `error` member.

[SC-RCP-027] The value of `error` MUST be a string matching `[a-z][a-z0-9-]{0,63}` as a
whole value.

[SC-RCP-028] When the value of `error` is a code in Table 8.3, it MUST be a code that
Table 8.3 lists for the receipt's `state` and `observer`.

[SC-RCP-029] A peer MUST ignore a receipt member that this revision does not define.

[SC-RCP-030] A peer that processes a receipt whose `error` is not a code in Table 8.3, and
whose `state` is not `duplicate`, MUST process the receipt as if its `state` were `failed`.

This is the rule of §5.2 item 5: a later minor revision may add an error code, and an
older peer treats it as `failed`. The older peer still knows that this copy was not handed
off, because every state that carries an error asserts that. A `duplicate` receipt keeps
its state, so that the rule of [SC-RCP-081] still holds.

[SC-RCP-031] A receipt MUST NOT contain any value taken from the envelope's `content`.

A receipt is trusted metadata about a delivery. Copying content into it would carry
untrusted text (§4.7) into a structure that no peer treats as content.

[SC-RCP-032] A peer MUST discard a receipt that fails any of [SC-RCP-020] to
[SC-RCP-028].

[SC-RCP-033] A peer MUST NOT send a receipt or an error about a receipt.

#### 8.1.5 Receipts between implementations

When the sending implementation and the receiver are different implementations, a
receiver-observed state reaches the sending implementation only in a receipt that crosses a
transport.

This revision defines a receipt's content and meaning only. It does not define how a
receipt is framed, signed or authenticated in transit. `spec/security.md` (E5, #45) owns
its authentication, and the transport contract owns its carriage. Until both exist, there
is no conformant cross-implementation receipt path, and a sending implementation reports
only the states it observes itself and `unknown`.

[SC-RCP-040] A sending implementation MUST NOT report a receiver-observed state for an
envelope unless it holds a receipt for that envelope that it has authenticated, per
`spec/security.md`, as coming from the receiver that serves the envelope's `to` session.

[SC-RCP-041] A receiver MUST NOT send a receipt for an envelope whose `security` metadata it
has not verified per `spec/security.md`.

An unverified envelope's `from` is only a claim. A receipt sent to it would let anyone who
forges a `from` aim receipts at another session.

[SC-RCP-042] A receiver MAY send a receipt to the sending implementation for each envelope
it has verified. A receiver that sends none still conforms, and its senders report
`unknown` ([SC-RCP-010]).

*Dated note, 2026-10-03 (#44): receipts from the receiving side back to the sender are
optional for v0.1, as [SC-RCP-042] states. This is an operator decision recorded on #44.
The sending implementation always knows the states it observes itself. The decision is
revisited when a binding can show far-side receipts to the sending harness.*

*Dated note, 2026-10-03 (#44, for E5 #45): anyone who captures a verified envelope can
replay it inside the replay window. Each replay is a `duplicate`, and a receiver that sends
a receipt for each one sends an unbounded number of receipts to the real sender.
`spec/security.md` is expected to bound this, for example by sending at most one
`duplicate` receipt per envelope or by rate-limiting them. This section sets no bound.*

### 8.2 Replies and correlation

#### 8.2.1 Meanings

- `conversation_id` groups envelopes into one conversation. The sender of the first
  envelope chooses it.
- `reply_to` makes an envelope a **reply**. It names the `id` of the envelope that the
  reply answers, the **answered envelope**.
- `correlation_id` is a value that a requester chooses and that every reply carries back,
  so that the requester can match replies to a request without tracking ids.

All three are header members (§4.7). They are signed and syntax-checked, and none of them
proves a relationship between envelopes. An envelope with none of them is a valid message
outside any thread ([SC-ENV-030]).

`reply_to` names the answered envelope only together with the reply's addressing. The
answered envelope was sent from the reply's `to` session to the reply's `from` session.
`reply_to` alone is not enough, because an `id` is unique only per sending session
([SC-ENV-027]).

#### 8.2.2 Building a reply

A harness asks its implementation to send a reply, and it may name the message being
answered, the **requested target**. The requested target comes from the harness and,
through it, possibly from a model, so it is untrusted input. Some harnesses carry no
structured reply tag at all: a target exists only if the model repeats an id it was
shown. The rules below therefore rely only on the replying implementation's own records
of the envelopes it handed off, never on a harness-side tagging convention
(`docs/planning/decisions/C6-trust-rendering.md` §10, conflict C9).

A **hand-off record** is an implementation's own record that it handed off an envelope. It
holds that envelope's `id`, `from`, `to`, and its `conversation_id` and `correlation_id`
when present.

[SC-RCP-050] A sending implementation MUST set `reply_to` only to the `id` of an envelope
for which it holds a hand-off record whose `to` is the reply's `from`.

[SC-RCP-051] A sending implementation MUST set `reply_to` only to the `id` of an envelope
for which it holds a hand-off record whose `from` is the reply's `to`.

[SC-RCP-052] A sending implementation MUST NOT set `reply_to` to any value other than the
requested target.

Together these set `reply_to` exactly when the harness named a target and that target is a
message the implementation handed off to the replying session, from the session the reply
goes to. Otherwise the reply is sent **uncorrelated**, without `reply_to`. A requested
target that fails the check downgrades the reply to uncorrelated. It never selects another
envelope, and the implementation never guesses one, even when only one candidate exists.

*Dated note, 2026-10-03 (#44): C6 §10 also allows an "inferred" correlation, in which the
implementation picks the single plausible candidate and marks the reply as inferred. This
revision has no envelope member for that mark, and an unmarked guess on the wire would look
the same as a checked target. Inferred correlations are therefore not sent in this
revision: a reply links to a message only when the link is checked, and otherwise goes out
uncorrelated. This is an operator decision recorded on #44, to be revisited after the live
reply-correlation leg of the reference adapters (backlog G8). Adding an optional marker
member then would be a minor revision (§5.2).*

[SC-RCP-053] A sending implementation that sets `reply_to` MUST set the reply's
`conversation_id` to the answered envelope's `conversation_id` when the hand-off record
holds one.

[SC-RCP-054] A sending implementation that sets `reply_to` MUST set the reply's
`correlation_id` to the answered envelope's `correlation_id` when the hand-off record holds
one.

The implementation copies both values from its own record, not from anything the harness
or the model supplies. Correlation therefore survives when a model repeats nothing but the
target.

[SC-RCP-055] A sending implementation SHOULD tell the requesting harness whether a reply
was sent correlated or uncorrelated. One that does not leaves the harness, and its user,
believing that an uncorrelated reply is threaded.

> **Reference implementation note:** hand-off records live in process memory, like the
> duplicate-suppression store (`docs/planning/decisions/C5-envelope-auth.md` §8). A reply
> whose answered envelope is no longer recorded, for example after a restart, is sent
> uncorrelated. How a harness passes the requested target is binding detail (E6, #46).

#### 8.2.3 Receiving a reply

The implementation that receives a reply matches it against the envelopes it sent itself.

[SC-RCP-060] A receiver MUST treat a reply as answering an envelope only when it holds a
record of having sent an envelope whose `id` equals the reply's `reply_to`, from the
reply's `to` session to the reply's `from` session.

[SC-RCP-061] A receiver MUST NOT reject a reply because its `reply_to` matches no envelope
the receiver sent.

An unmatched reply is still a valid message. The receiver hands it off as a message
outside any thread.

[SC-RCP-062] A receiver MUST NOT treat a matching `correlation_id` or `conversation_id`
alone as evidence that one envelope answers another.

The link is checked at both ends. The replying side links only to an envelope it handed off
([SC-RCP-050], [SC-RCP-051]). The receiving side accepts the link only to an envelope it
sent ([SC-RCP-060]). Neither end trusts a value that a model chose.

### 8.3 Error taxonomy

#### 8.3.1 The closed set

Table 8.3 lists every error code that a conformant implementation emits ([SC-RCP-074]). A
later minor revision may add a code (§5.2 item 5). Changing a code's meaning is a breaking
change (§5.3 item 5). Codes are compared exactly, case-sensitively.

Each code has a **stage**:

- `envelope`: envelope-stage validation (§2.3);
- `security`: the verification, replay and authorization checks whose conditions
  `spec/security.md` defines;
- `delivery`: routing and hand-off, after both of the above;
- `request`: a harness's request, refused before any envelope exists.

The **scope** column lists where a code may appear: in a receipt observed by the `sender`
or by the `receiver`, or as a `request` error returned to a harness. The **state** column
applies to receipts only. The last column is guidance; §8.4 holds the requirements.

Table 8.3.

| Code | Stage | State | Scope | Condition | Sender's next step |
|---|---|---|---|---|---|
| `envelope-too-large` | envelope | `rejected` | receiver, request | The serialized envelope is larger than the receiver-wide limit ([SC-ENV-004], [SC-RCP-076]); as a request error, larger than the addressed session accepts ([SC-ENV-005]). | Retry with smaller content. |
| `malformed-envelope` | envelope | `rejected` | receiver | The envelope fails a requirement of §4 or §5.4 that no other code in this table covers. | None: the sender is defective. |
| `unsupported-version` | envelope | `rejected` | receiver, request | The major version is not one the receiver supports ([SC-VER-001]); as a request error, no version is agreed with the addressed session (§6.5). | Retry under a major version the receiver supports (§6.5). |
| `unsupported-content-type` | envelope | `rejected` | receiver, request | A content part's `type` is one the receiver supports for no session ([SC-ENV-065], [SC-RCP-076]); as a request error, the addressed session has not advertised it ([SC-ENV-066]). | Retry with supported part types. |
| `expired` | envelope | `expired` | receiver | The expiry instant has passed ([SC-ENV-100], [SC-ENV-101]). | Retry, if the message is still wanted. |
| `unknown-key` | security | `rejected` | receiver | `security.key_id` names no key the receiver trusts. | None until the devices are paired. |
| `signature-invalid` | security | `rejected` | receiver | The signature does not verify. | None: investigate. |
| `outside-replay-window` | security | `expired` | receiver | `created_at` is outside the receiver's replay window. | Retry once the clocks agree. |
| `duplicate` | security | `duplicate` | receiver | The envelope is a copy of one that was handed off, may have been, or is being handed off ([SC-RCP-009]). | None ([SC-RCP-081]). |
| `unauthorized` | security | `rejected` | receiver, request | The sender, or the requesting harness, is not authorized for the operation. | None until authorized. |
| `unknown-destination` | delivery | `unreachable` | sender, receiver, request | No session with the addressed id is known to the observer. | None. |
| `destination-unavailable` | delivery | `unreachable` | sender, receiver, request | The addressed session is known but is not accepting input now. | Retransmit, or retry later. |
| `unsupported-capability` | delivery | `rejected` | receiver, request | The addressed session lacks a capability the message requires (§6.6), including a size or part type that the receiver accepts for some session but not for this one ([SC-RCP-077]). | None. |
| `handoff-failed` | delivery | `failed` | receiver | The harness's input surface reported that the hand-off call failed. | Retransmit. |
| `transport-failure` | delivery | `failed` | sender | The sending implementation could not pass the envelope to a transport. | Retransmit. |
| `internal-error` | any | `failed` | sender, receiver, request | An internal error unrelated to the envelope's validity. | Retransmit. |
| `invalid-request` | request | none | request | A harness's request is malformed, or names no operation the implementation offers. | Correct the request. |

[SC-RCP-070] A receiver that does not hand off an envelope because a check failed MUST
report the code that Table 8.3 assigns to that check.

The binding document (E6, #46) names three errors by role. Its authorization-failure error
is `unauthorized`, its malformed-input error is `invalid-request`, and its
unknown-destination error is `unknown-destination`.

#### 8.3.2 Precedence

An envelope can fail several checks at once. A receiver reports one code, and these rules
choose it, so that every implementation reports the same code for the same input.

Within the envelope stage, the checks run in this order:

1. the receiver-wide size limit: `envelope-too-large`;
2. encoding ([SC-ENV-001], [SC-ENV-002]) and the `version` member ([SC-ENV-020]):
   `malformed-envelope`;
3. the major version ([SC-VER-001]): `unsupported-version`;
4. every other requirement of §4 and §5.4 that envelope-stage validation checks, except the
   two below: `malformed-envelope`;
5. content part types the receiver supports for no session ([SC-ENV-065]):
   `unsupported-content-type`;
6. expiry ([SC-ENV-100]): `expired`.

Step 3 comes before step 4 because a receiver cannot judge an envelope's structure by the
rules of a major version it does not implement.

[SC-RCP-071] A receiver MUST report the code of the earliest failed step when an envelope
fails more than one envelope-stage check.

The envelope stage runs before the sender is authenticated or authorized. A check there
that depended on the addressed session would tell an unauthorized sender whether that
session exists and what it accepts.

[SC-RCP-076] A receiver MUST apply the size check of step 1 and the part-type check of
step 5 with limits that do not depend on the addressed session.

The receiver-wide size limit is the largest size the receiver accepts for any session. The
receiver-wide part types are those it supports for at least one session.

[SC-RCP-077] A receiver MUST report an envelope whose size or part types the receiver
accepts, but the addressed session does not, as `unsupported-capability` at the delivery
stage.

Within the delivery stage, the checks run in this order:

1. whether the addressed session is known: `unknown-destination`;
2. whether it is accepting input now: `destination-unavailable`;
3. whether it has every capability the envelope needs, including its size and part types:
   `unsupported-capability`;
4. expiry, repeated at hand-off ([SC-ENV-101]): `expired`;
5. the hand-off call: `handoff-failed`.

[SC-RCP-078] A receiver MUST report the code of the earliest failed step when an envelope
fails more than one delivery-stage check.

[SC-RCP-072] A receiver MUST NOT report a security-stage code for an envelope that failed
envelope-stage validation.

[SC-RCP-073] A receiver MUST NOT report a delivery-stage code for an envelope that has not
passed the security stage.

The order within the security stage is defined in `spec/security.md`. [SC-RCP-073] also
limits what an unauthorized sender learns: whether a session exists, or is available, is
reported only to a sender that passed authorization. The expiry check that [SC-ENV-101]
repeats at hand-off still reports `expired`.

[SC-RCP-074] An implementation MUST NOT emit an error code that Table 8.3 of the revision
it implements does not list.

[SC-RCP-075] An implementation that refuses a harness's request before creating an
envelope MUST report a code whose scope in Table 8.3 includes `request`.

### 8.4 Retransmission, retry and receipts

Section 4.9 defines a **retransmission** (the same envelope, unchanged) and a **retry**
(the same content in a new envelope, with a new `id` and a new nonce). They behave
differently against duplicate suppression. A receiver recognizes a retransmission as a
copy and suppresses it when an earlier copy was handed off (`spec/security.md`,
[SC-RCP-009]). A retry is a new envelope, which no receiver can recognize as a repeat.
Retransmission is therefore the safe recovery when the outcome is not known; a retry can
hand the content off twice.

A retry "on its own initiative" is one the implementation starts without a new request
from the harness. When a harness or its user asks to send the message again, that is a new
message and the harness's decision. Each retry gets its own receipts, under its own `id`.

#### 8.4.1 The combined state of an envelope

A receipt names an envelope, not a copy of it. Every copy of a retransmitted envelope has
the same `envelope_id` and `envelope_from`, so a receipt cannot say which copy it
describes, and receipts can be lost or arrive in any order ([SC-RCP-042]). An error state
proves that the content was not handed off only for the copy it describes. A sending
implementation that passed an envelope to a transport more than once cannot tell which
copy an error receipt describes.

A copy is **passed** when the sending implementation handed it to a transport. A copy that
the sending implementation itself saw fail before reaching a transport (`unreachable`, or
`failed` with `transport-failure`) is not passed.

[SC-RCP-085] A sending implementation MUST report, as the state of an envelope, the first
of the following that applies to the states it holds for that envelope:

1. `handed-to-harness`, when it holds that state for any copy;
2. `duplicate`, when it holds that state for any copy;
3. `unknown`, when it holds a receiver-observed `unknown` for any copy;
4. the error state it received first, when it passed exactly one copy and holds an error
   state;
5. its own error state, when it passed no copy;
6. otherwise `accepted-by-adapter` until the hand-off deadline (§8.1.3), and `unknown`
   after it ([SC-RCP-010]).

This is the envelope's **combined state**. In the rest of this section, the state a
sending implementation "holds" for a message is the combined state of the message's
envelope. Rule 1 means a late `duplicate` never overwrites `handed-to-harness`, and rule 4
means an error receipt settles an envelope only when it can describe just one copy.

[SC-RCP-086] A sending implementation that passed an envelope more than once MUST NOT
retry the message on its own initiative.

A retry is safe only when the sending implementation knows that no copy was handed off. It
knows that only when one copy was passed and a receipt for it shows an error state, or when
no copy was passed. A retransmission removes that knowledge for good, because no later
receipt can be tied to a copy.

#### 8.4.2 Retry and retransmission rules

[SC-RCP-080] A sending implementation MUST NOT retry, on its own initiative, a message for
which it holds `handed-to-harness`.

[SC-RCP-081] A sending implementation MUST NOT retry, on its own initiative, a message for
which it holds `duplicate`.

[SC-RCP-082] A sending implementation SHOULD NOT retry, on its own initiative, a message
for which it holds `accepted-by-adapter` or `unknown`. A sending implementation that does
deviates: the content can be handed off twice.

[SC-RCP-083] A sending implementation MAY retransmit an envelope for which it holds
`accepted-by-adapter`, `unknown`, `unreachable` or `failed`, before its hand-off deadline
(§8.1.3). A sending implementation that does not retransmit keeps reporting the state it
holds.

[SC-RCP-084] A sending implementation SHOULD NOT retransmit an envelope for which it holds
`rejected` or `expired`. A sending implementation that does deviates: a retransmission is
unchanged ([SC-ENV-102]), so the receiver repeats the same result.

A retry after `rejected`, `expired`, `unreachable` or `failed` cannot hand off the content
a second time, but only when that combined state came from rule 4 or rule 5 of
[SC-RCP-085]: one copy passed, or none. After a retransmission, [SC-RCP-086] applies
instead, whatever error receipts arrive.

### 8.5 Conformance fixtures for this section

Fixtures for this section follow §3.3 and live under `tests/protocol/sc-rcp/`. Besides the
`envelope` stage, they use five more `stage` values, with these members. In an
`envelope`-stage fixture, the receiver supports the part type `text` and no other.

| `stage` | `context` | `input` | `expected` |
|---|---|---|---|
| `receipt` | an empty object | `receipt`: the receipt as a JSON value | `result`: `valid` or `discarded` ([SC-RCP-032]); for `valid`, `effective_state`: the state a peer processes the receipt as ([SC-RCP-030]) |
| `reply` | `handed_off`: an array of hand-off records, each an object with `id`, `from`, `to` and, when present, `conversation_id` and `correlation_id` | `reply_request`: an object with `from` (the replying session), `to` (the addressed session) and, optionally, `requested_target` | `reply_headers`: an object holding exactly those of `reply_to`, `conversation_id` and `correlation_id` that the implementation sets; `correlation`: `correlated` or `uncorrelated` |
| `correlation` | `receiver_time` and `supported_major_versions` as in §3.3, and `sent`: an array of sent-envelope records, with the members of a hand-off record | `envelope`, as in §3.3 | `result`, as in §3.3; `correlation`: `matched` or `unmatched`; for `matched`, `answers`: an object with the `id` and `from` of the answered envelope |
| `combine` | `copies_passed`: the number of copies passed to a transport; `deadline_passed`: whether the hand-off deadline has passed | `held`: an array of the states held for the envelope, in arrival order, each an object with `state`, `observer` and, when present, `error` | `state`: the combined state ([SC-RCP-085]); `retry_allowed`: whether a retry on the implementation's own initiative is permitted, which is true only when `state` is an error state and `copies_passed` is 0 or 1 ([SC-RCP-080] to [SC-RCP-082], [SC-RCP-086]) |
| `routing` | `receiver_time` and `supported_major_versions` as in §3.3; `receiver_content_types`: the part types the receiver supports for at least one session; `sessions`: an object whose members are the session ids the receiver knows, each an object with `accepting` (a boolean) and `content_types` (an array); `authorized`: an array of objects with `from` and `to`, the sender-to-session pairs that pass authorization. Every other security-stage check is taken as passed. | `envelope`, as in §3.3 | `result`: `valid`, `rejected`, `expired` or `unreachable`; for a result other than `valid`, `error` |

Every negative `envelope`-stage fixture, in `sc-rcp/` and in `sc-env/` and `sc-ver/`,
carries `expected.error`: the code that [SC-RCP-070] and [SC-RCP-071] require.

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
| SC-RCP-001 | MUST | 8.1.2 | `sc-rcp/SC-RCP-001.p01`, `.n01` to `.n03` |
| SC-RCP-002 | MUST NOT | 8.1.2 | `sc-rcp/SC-RCP-002.p01`, `.p02`, `.n01` to `.n03` |
| SC-RCP-003 | MUST NOT | 8.1.2 | TODO(fixture): observation is behaviour; F6 receipt state machine, F10 adapter contract suite |
| SC-RCP-004 | MUST NOT | 8.1.3 | TODO(fixture): hand-off behaviour; F10 against the F8/F9 fakes, then live legs G4 and G7, H1 |
| SC-RCP-005 | MUST NOT | 8.1.3 | TODO(fixture): rendering of states; F10, F11, live legs G5, G8, G10 |
| SC-RCP-006 | MUST | 8.1.3 | TODO(fixture): indeterminate hand-off; F10 with a fake that times out, live leg H3 |
| SC-RCP-007 | MUST NOT | 8.1.3 | TODO(fixture): sender state machine; F6 |
| SC-RCP-008 | MUST NOT | 8.1.3 | TODO(fixture): hand-off behaviour; F6, F10 |
| SC-RCP-009 | MUST NOT | 8.1.3 | TODO(fixture): needs the duplicate-suppression rules of `spec/security.md`; E5, F4 |
| SC-RCP-010 | SHOULD | 8.1.3 | none (SHOULD) |
| SC-RCP-011 | MAY | 8.1.3 | none (MAY) |
| SC-RCP-020 | MUST | 8.1.4 | `sc-rcp/SC-RCP-020.n01`, `.n02` |
| SC-RCP-021 | MUST | 8.1.4 | `sc-rcp/SC-RCP-021.n01`, `.n02` |
| SC-RCP-022 | MUST | 8.1.4 | `sc-rcp/SC-RCP-022.n01` |
| SC-RCP-023 | MUST | 8.1.4 | `sc-rcp/SC-RCP-023.n01` |
| SC-RCP-024 | MUST | 8.1.4 | `sc-rcp/SC-RCP-024.n01` |
| SC-RCP-025 | MUST | 8.1.4 | `sc-rcp/SC-RCP-025.p01`, `.n01` |
| SC-RCP-026 | MUST NOT | 8.1.4 | `sc-rcp/SC-RCP-026.n01` |
| SC-RCP-027 | MUST | 8.1.4 | `sc-rcp/SC-RCP-027.n01` |
| SC-RCP-028 | MUST | 8.1.4 | `sc-rcp/SC-RCP-028.n01`, `.n02` |
| SC-RCP-029 | MUST | 8.1.4 | `sc-rcp/SC-RCP-029.p01` |
| SC-RCP-030 | MUST | 8.1.4 | `sc-rcp/SC-RCP-030.p01`, `.p02` |
| SC-RCP-031 | MUST NOT | 8.1.4 | TODO(fixture): needs the envelope beside the receipt; F6 |
| SC-RCP-032 | MUST | 8.1.4 | every negative `receipt`-stage fixture in `sc-rcp/` (SC-RCP-001 to SC-RCP-028) |
| SC-RCP-033 | MUST NOT | 8.1.4 | TODO(fixture): peer behaviour; F6 |
| SC-RCP-040 | MUST NOT | 8.1.5 | TODO(fixture): receipt authentication; E5, E8 |
| SC-RCP-041 | MUST NOT | 8.1.5 | TODO(fixture): needs verification vectors; E5, F4, H2 |
| SC-RCP-042 | MAY | 8.1.5 | none (MAY) |
| SC-RCP-050 | MUST | 8.2.2 | `sc-rcp/SC-RCP-050.p01`, `.n01`, `.n02` |
| SC-RCP-051 | MUST | 8.2.2 | `sc-rcp/SC-RCP-051.n01` |
| SC-RCP-052 | MUST NOT | 8.2.2 | `sc-rcp/SC-RCP-052.n01` |
| SC-RCP-053 | MUST | 8.2.2 | `sc-rcp/SC-RCP-053.p01`, `.n01` |
| SC-RCP-054 | MUST | 8.2.2 | `sc-rcp/SC-RCP-054.p01` |
| SC-RCP-055 | SHOULD | 8.2.2 | none (SHOULD) |
| SC-RCP-060 | MUST | 8.2.3 | `sc-rcp/SC-RCP-060.p01`, `.n01`, `.n02` |
| SC-RCP-061 | MUST NOT | 8.2.3 | `sc-rcp/SC-RCP-061.p01` |
| SC-RCP-062 | MUST NOT | 8.2.3 | `sc-rcp/SC-RCP-062.n01` |
| SC-RCP-070 | MUST | 8.3.1 | `expected.error` of every negative `envelope`-stage fixture in `sc-env/` and `sc-ver/`; `sc-rcp/SC-RCP-071.n01` to `.n06` |
| SC-RCP-071 | MUST | 8.3.2 | `sc-rcp/SC-RCP-071.n01` to `.n06` |
| SC-RCP-072 | MUST NOT | 8.3.2 | TODO(fixture): needs security-stage vectors; E5, E8 |
| SC-RCP-073 | MUST NOT | 8.3.2 | `sc-rcp/SC-RCP-073.n01`, `.n02` (authorization only; signature and replay vectors: E5, E8, H2) |
| SC-RCP-074 | MUST NOT | 8.3.2 | TODO(fixture): a runner check that every emitted code is in Table 8.3; E8, F12 |
| SC-RCP-075 | MUST | 8.3.2 | TODO(fixture): request errors; E6 binding hooks, G5, G8 |
| SC-RCP-076 | MUST | 8.3.2 | `sc-rcp/SC-RCP-076.p01`, `.n01` |
| SC-RCP-077 | MUST | 8.3.2 | `sc-rcp/SC-RCP-077.n01` |
| SC-RCP-078 | MUST | 8.3.2 | `sc-rcp/SC-RCP-078.n01`, `.n02` |
| SC-RCP-080 | MUST NOT | 8.4.2 | `sc-rcp/SC-RCP-080.p01` (`retry_allowed`); live retry behaviour: F6 |
| SC-RCP-081 | MUST NOT | 8.4.2 | `sc-rcp/SC-RCP-081.p01` (`retry_allowed`); live retry behaviour: F6 |
| SC-RCP-082 | SHOULD NOT | 8.4.2 | none (SHOULD NOT) |
| SC-RCP-083 | MAY | 8.4.2 | none (MAY) |
| SC-RCP-084 | SHOULD NOT | 8.4.2 | none (SHOULD NOT) |
| SC-RCP-085 | MUST | 8.4.1 | `sc-rcp/SC-RCP-085.p01` to `.p06`, `.n01`, `.n02` |
| SC-RCP-086 | MUST NOT | 8.4.1 | `sc-rcp/SC-RCP-086.p01`, `.n01`; live retry behaviour: F6 |

Retired ids: none.

## Appendix B. Revision history

| Revision | Date | Change |
|---|---|---|
| 0.1 (draft) | 2026-10-03 | E1 (#41): document skeleton for sections 1-10; sections 4 (envelope) and 5 (versioning) written; requirement-id scheme and fixture format (§3); envelope-stage fixtures under `tests/protocol/sc-env/` and `tests/protocol/sc-ver/`. Review of #258: SC-ENV-027, SC-ENV-103, SC-ENV-104 and SC-VER-003 added (retransmission and retry defined); dated notes for the operator decisions on #41. |
| 0.1 (draft) | 2026-10-03 | E4 (#44): section 8 written: delivery states, receipts, replies and correlation, the closed error taxonomy with precedence, and the retransmission and retry rules including the combined state of an envelope; requirement area `RCP`; fixtures under `tests/protocol/sc-rcp/`; `expected.error` added to every negative envelope-stage fixture (§3.3); §4.9's duplicate wording aligned with [SC-RCP-009]; dated notes for the operator decisions on #44. |
