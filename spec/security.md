# OAC Session Channels Security

**Document:** `spec/security.md`, the normative security model of OAC Session Channels.
**Revision:** 0.1 (draft, Stage 2). Written by task E5 (#45).
**Companion document:** `spec/session-channels.md`, which defines the envelope (§4),
versioning (§5), session identity (§6), presence and discovery (§7), and delivery states,
receipts and errors (§8). This document does not restate those rules. It defines what that
document leaves to it: device keys, signing, verification, replay defence, duplicate
suppression, authorization, receipt and presence-record authentication, and provenance
rendering.

This document is normative-of-record for OAC Session Channels security, as
`spec/session-channels.md` is for the protocol (`docs/planning/decisions/C3-spec-packaging.md`
§1). A binding document is packaging. Where a binding document and this document disagree,
this document governs.

---

## 1. Introduction

### 1.1 What this document protects

OAC Session Channels carries messages between live harness sessions. Every message crosses
at least one boundary that the receiving side does not control: another device, another
implementation, or a transport. This document makes three things checkable at that
boundary, using only the envelope and the records defined here:

1. **Who sent it.** A message is signed by the sending device's key. Nothing else proves a
   sender: no transport identity, no network address, and no text inside the message.
2. **Whether it may be delivered.** Delivery is denied by default and opened only by an
   explicit grant.
3. **What the receiving model is told about it.** Provenance is machine-set, separate from
   content, and never derived from content.

The security model does not depend on any transport's own security features. Transport
encryption and transport access control MAY be present. They never replace the checks of
this document (§6.6).

### 1.2 The doctrine: authenticated but untrusted

An authenticated peer message is still an untrusted instruction, and it can carry prompt
injection. Authentication answers who sent a message. It never answers whether to obey it
(`docs/planning/ADR-001.md`, "Security model": "Authenticated peer messages remain untrusted
instructions and may contain prompt injection").

Everything below follows from that. A verified signature and a matching grant authorize
one thing only: handing the message's content to the addressed session's input. They never
authorize an action that the content asks for. Any further power, such as approving a
harness permission request, is a separate authorization decision with its own default
(§9.6).

### 1.3 Scope

| Section | Subject |
|---|---|
| 4 | Identity hierarchy |
| 5 | Device keys, key ids, the trusted key set, and the registration record |
| 6 | Envelope signing and verification |
| 7 | The security stage: order of checks and error codes |
| 8 | Replay window, nonce, and duplicate suppression |
| 9 | Authorization |
| 10 | Receipt authentication |
| 11 | Presence-record authentication and the publishable binding proof |
| 12 | Provenance rendering |
| 13 | Threat-to-requirement traceability |

Out of scope for this revision: confidentiality of message content against a transport
(full end-to-end encryption is deferred, `docs/planning/ADR-001.md`, "v0.1 scope"); the
wire format of a pairing exchange between two devices (§5.3); online key rotation (§5.5).

---

## 2. Conventions and terminology

### 2.1 Requirement keywords

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT",
"RECOMMENDED", "NOT RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be
interpreted as described in BCP 14 [RFC2119] [RFC8174] when, and only when, they appear in
all capitals, as shown here.

This document uses only `MUST`, `MUST NOT`, `SHOULD`, `SHOULD NOT` and `MAY`. Each sentence
carrying one of them states one requirement and starts with a requirement id (§3.1).

- A `SHOULD` or `SHOULD NOT` sentence is followed by what a deviation looks like.
- A `MAY` sentence is followed by what a peer that does not take the option still does.

### 2.2 Reference implementation notes

A paragraph labelled as below is informative. It describes a choice of the v0.1 reference
implementation and binds no other implementation:

> **Reference implementation note:** an implementation choice, not binding on other
> conformant implementations.

### 2.3 Terminology

Terms defined in `spec/session-channels.md` §2.3 (harness, session, device, implementation,
sender, receiver, hand-off, envelope, envelope-stage validation, identifier token) keep
their meaning here. In addition:

- **Security principal:** the person or organization that controls one or more devices.
- **Device key:** the one Ed25519 key pair [RFC8032] of a device (§5.1).
- **Key id:** the identifier of a device key's public key (§5.2).
- **Trusted key set:** the device keys an implementation accepts signatures from (§5.3).
- **Binding table:** an implementation's record of which device key each known session id
  is bound to (§11.3).
- **Grant:** an explicit, one-way authorization entry: a writer may send to a target (§9.2).
- **Reply right:** the automatic right to answer one sent message (§9.5).
- **Signing input:** the exact octets that a signature covers (§6.2).
- **Security stage:** the checks of §7, run after envelope-stage validation and before the
  delivery stage of `spec/session-channels.md` §8.3.2.
- **Replay window, `W`:** the time interval of §8.1.
- **Duplicate store:** a receiver's record of envelopes whose hand-off was attempted (§8.3).
- **Provenance set:** the machine-set values an adapter renders with each message (§12.1).
- **Adapter:** the part of an implementation that hands content to one harness's input
  surface.

---

## 3. Conformance

### 3.1 Requirement ids

This document uses the requirement-id scheme of `spec/session-channels.md` §3.2 with the
document prefix `SEC` that §3.2 there registers. Its areas are:

| Area | Section | Subject |
|---|---|---|
| `KEY` | 4, 5 | identity hierarchy, device keys, trusted key set, registration record |
| `SIG` | 6 | envelope signing and verification |
| `STG` | 7 | the security stage |
| `RPL` | 8 | replay window, nonce, duplicate suppression |
| `AUZ` | 9 | authorization |
| `RCT` | 10 | receipt authentication |
| `PRS` | 11 | presence-record authentication and binding proof |
| `PRV` | 12 | provenance rendering |

Ids are allocated in increasing order within an area, gaps are allowed, and the stability
rules of `spec/session-channels.md` §3.2 apply unchanged.

### 3.2 Conformance fixtures

Fixtures follow `spec/session-channels.md` §3.3. They live under
`tests/protocol/sec-<area>/` and carry `"spec": "spec/security.md"`. Their stages are listed
in §3.3. Appendix A indexes every requirement and its fixtures.

**Test keys.** `tests/protocol/sec-test-keys.json` lists the device keys the fixtures use.
They are **test keys only**: each private seed is the SHA-256 of a published label, so anyone
can recompute it, and no real device holds any of them. The file gives each key's label,
principal, seed, public key and key id, the rule that derives the fixtures' nonces, and the
label of every nonce the fixtures use (`nonce_labels`). An implementation uses the seeds only
to check that its signing produces the fixtures' signatures, which Ed25519's deterministic
signing makes reproducible.

Public keys appear in fixtures as unpadded base64url [RFC4648] §5 of the 32-octet encoding.

### 3.3 Fixture stages

Every stage below uses `context`, `input` and `expected` with the members its table lists,
in place of those of `spec/session-channels.md` §3.3. Timestamps are in the form of
`spec/session-channels.md` §4.4.6.

**Common values.**

- A **trusted-key list** is an array of objects with `principal`, `key_id` and `public_key`.
- A **binding map** is an object whose members are session ids and whose values are key ids,
  or, for a session id under conflict, an object `{"conflict": [...]}` listing the claimant
  key ids in ascending order (§11.3).
- A **grant list** is an array of grants in the form of §9.2: objects with `direction`
  (`inbound` or `outbound`), `writer` and `target`. The side on the implementation's own
  device is `{"session_id": …}`, `{"working_directory_scope": …}` or `{"device": true}`; the
  side on another device is `{"key_id": …}`, optionally with `session_id`.
- A **session map** is an object whose members are the implementation's own session ids,
  each an object with `working_directory_scope`, an opaque label for the session's
  working-directory scope (§9.3).
- A **sent list** is an array of the implementation's own sent-envelope records (§9.5), each
  with `id`, `from`, `to`, `to_key_id` (the key `to` was bound to when sent), `created_at`
  and, where a stage needs it, `nonce`.
- A **hand-off list** is an array of the implementation's own hand-off records, each with
  `id`, `from`, `to` and `created_at`.

**Stage `security`** (§6, §7, §8, §9). The runner applies envelope-stage validation, then the
security stage, to one envelope at the receiver. Every delivery-stage check is taken as
passed.

| Member | Content |
|---|---|
| `context` | `receiver_time`; `supported_major_versions`; `trusted_keys`: a trusted-key list; `bindings`: a binding map, which includes the receiver's own sessions; `sessions`: a session map; `grants`: a grant list; `duplicate_store`: an array of objects with `key_id` and `nonce`, the entries the store holds; optionally `sent`: a sent list. |
| `input` | `envelope`, or `envelope_text`, as in `spec/session-channels.md` §3.3. |
| `expected` | `result`: `passed`, `rejected`, `expired` or `duplicate`. For a result other than `passed`, `error`: the code. Optionally `canonical`: the JCS text of §6.2 for the envelope; `receipt_permitted`: `false` when no receipt may be sent for the envelope at all (§10.3); `bindings_after`: the binding map after the envelope (§11.3); `record`: `none` or `finding`, the local record [SEC-PRS-004] requires. |

**Stage `replay`** (§8). A sequence of arrivals at one receiver, which starts with an empty
duplicate store.

| Member | Content |
|---|---|
| `context` | As for `security`, without `receiver_time` and `duplicate_store`, plus `replay_window_ms`: 300000. |
| `input` | `envelopes`: an object of labelled envelopes. `arrivals`: an array, in order, of objects with `at` (the receiver's clock), `envelope` (a label), optionally `grants_add` (grants added just before this arrival), optionally `during_previous_handoff`: `true` when the copy arrives while the previous arrival's hand-off call is still running (§8.3), and `delivery`: the outcome of the delivery stage and the hand-off call if the security stage passes, one of `handed-to-harness`, `unknown`, `destination-unavailable` or `handoff-failed`. |
| `expected` | `results`: one object per arrival with `result` (a delivery state of `spec/session-channels.md` §8.1, or `duplicate`), `error` when the state carries one, and for a `duplicate` result `duplicate_receipt_allowed` (§8.4). |

**Stage `key-id`** (§5.2). `input`: `public_key`. `expected`: `key_id`.

**Stage `key-removal`** (§5.3). `context`: `bindings` and `grants`. `input`:
`remove_key_id`. `expected`: `bindings_after` and `grants_after`, after the key is removed
from the trusted key set ([SEC-KEY-035]).

**Stage `registration`** (§5.4). `context`: `trusted_keys`. `input`: `record`, a
registration record. `expected`: `result` (`verified` or `invalid`); with `verified`,
optionally `canonical`; with `invalid`, `binding_usable`: `false`.

**Stage `receipt-auth`** (§10). `context`: `trusted_keys`; `bindings`; `sent`: a sent list
with `nonce`. `input`: `authenticated_receipt`. `expected`: `result` (`authenticated` or
`discarded`), optionally `canonical`.

**Stage `presence-auth`** (§11). The consumer receives one authenticated presence record.

| Member | Content |
|---|---|
| `context` | `consumer_time`; `own_key_id`: the consumer's device key id; `trusted_keys`; `sessions`; `bindings`; `grants`; `latest_seq`: an object mapping session ids to the latest accepted `seq`; optionally `sent` and `handed_off`. |
| `input` | `authenticated_record`. |
| `expected` | `result` (`accepted` or `discarded`); `record`: `none` or `finding`, the local record §11 requires; optionally `bindings_after`, `canonical`, and, for an accepted announcement, `effective_lifetime_ms` (§11.4). |

**Stage `discovery-auth`** (§9.4, §9.5). `context`: `now`, `own_key_id`, `sessions`,
`bindings`, `grants`, optionally `handed_off`. `input`: `requester` and `session`.
`requester` is `{"session_id": …}`, one of the implementation's own sessions asking to
discover `session`; or `{"device": …}`, a peer device's key id, asking whether the
implementation may release `session`'s presence record to that device. `session` is one of
`sessions`, or a session of another implementation whose binding `bindings` holds.
`expected`: `discoverable`, a boolean.

**Stage `exchange`** (§9). Two implementations, one sequence of operations.

| Member | Content |
|---|---|
| `context` | `implementations`: an object of labelled implementations, each with `own_key_id`, `trusted_keys`, `sessions` and `grants`. Each starts with its own sessions in its binding map and no other state. |
| `input` | `steps`: an array, in order, of objects with `at`, `actor` (an implementation label), `op`, and the operation's members: `release` (`session`, `to_device`: may the actor release `session`'s presence record to that device, and if so it issues it); `accept-presence` (`authenticated_record`); `discover` (`requester`, `session`); `send` (`envelope`: the actor passes it to a transport if its requester may discover `to` and the actor has issued the requester's announcement to `to`'s device, and records a reply right); `receive` (`envelope`, optionally `delivery`). |
| `expected` | `results`: one object per step: `released`; `result` for `accept-presence`; `discoverable`; `result` (`sent`, or `refused` with `error`) for `send`; `result` and `error` as in stage `replay` for `receive`. |

**Stage `provenance`** (§12.2). `input`: `fields`, an object with `sender`, `device`,
`session`, `message_id` and `reply_to`. `expected`: `result` (`rendered` or `refused`); with
`refused`, `state` and `error`.

**Stage `body`** (§12.3). `input`: `body`, a string. `expected`: `quoted_lines`, the array of
quoted lines.

> **Reference implementation note:** the v0.1 conformance runner (tasks E8 and F12) is
> expected to run these stages from the implementation's own verification, replay,
> authorization and rendering code, with a scripted clock, so no transport and no harness
> takes part. The E5 change checked every fixture with a separate script that shares no code
> with the script that generated them: its own JCS, its own Ed25519 arithmetic, and its own
> evaluator for each stage. Neither script is committed; the fixtures are the artifact.

---

## 4. Identity hierarchy

Authority in OAC Session Channels derives from one chain
(`docs/planning/ADR-001.md`, "Security model"):

```text
Security principal -> Device -> Harness -> Session
```

- A **security principal** controls devices. It is named by a principal label (§5.3) that
  an operator confirms when pairing.
- A **device** is one installation of an implementation. It holds one device key (§5.1). The
  device key is the cryptographic root: every signature in this document is made with one.
- A **harness** runs on a device. It holds no key, and it is not an authority of its own: a
  harness label is a display value only (`spec/session-channels.md` [SC-ID-043]).
- A **session** is named by a session id, bound to exactly one device key by a registration
  record signed with that key (`spec/session-channels.md` [SC-ID-009]; §5.4 here).

[SEC-KEY-001] An implementation MUST derive every authority it grants to a message, a
session or a peer from this chain, and from nothing a message's content states.

A display form, an alias, a `display_name`, a harness label, a transport's peer identifier,
a harness-native identifier and a cross-check value are outside the chain. None of them
carries authority (`spec/session-channels.md` §6.1-§6.3, §6.7).

---

## 5. Device keys, key ids, the trusted key set, and the registration record

### 5.1 The device key

The signature algorithm is Ed25519 [RFC8032] §5.1, chosen in
`docs/planning/decisions/C5-envelope-auth.md` §2. RFC 8032 states that Ed25519 is "intended
to operate at around the 128-bit security level" (retrieved 2026-09-17, as quoted in C5 §2).

[SEC-KEY-002] An implementation MUST hold exactly one device key, an Ed25519 key pair.

[SEC-KEY-003] An implementation MUST generate its device key's 32-octet private seed with a
cryptographically secure random number generator.

[SEC-KEY-004] An implementation MUST NOT disclose its device key's private seed to a peer, a
harness, a session or a model.

[SEC-KEY-005] An implementation MUST NOT give a session a signing key of its own.

A session's authority comes from the device key that signs its registration record
(`docs/planning/decisions/C5-envelope-auth.md` §4). How an implementation stores the private
seed is its own choice.

> **Reference implementation note:** the v0.1 reference implementation keeps the seed in the
> operating system's credential store, with an encrypted-file fallback
> (`docs/planning/decisions/C4-session-identity.md` §10-§11).

### 5.2 Key ids

The **key id** of a device key is the SHA-256 [FIPS180-4] digest of the key's 32-octet
public-key encoding (RFC 8032 §5.1.5), written as 64 lower-case hexadecimal digits.

The key id is the device's fingerprint. It is the full digest, not a truncation, so it is
above the 128-bit floor of `docs/planning/decisions/C5-envelope-auth.md` §10(b) by
construction. It is an identifier token (`spec/session-channels.md` §4.3), so an adapter can
render it as provenance without escaping (§12.2;
Decision C13 (`docs/planning/decisions/`; §14) hands that encoding choice to
this document).

[SEC-KEY-010] An implementation MUST compute a key id as the lower-case hexadecimal SHA-256
digest of the 32-octet public-key encoding.

[SEC-KEY-011] An implementation MUST compare key ids as exact strings, without
case-folding or any other normalization.

### 5.3 Principals and the trusted key set

`security.principal` names the security principal of the device that signed. It is a label,
not a credential: it is authenticated only together with a key, by the trusted key set.

[SEC-KEY-020] The value of `security.principal` MUST be an identifier token
(`spec/session-channels.md` §4.3).

An implementation's **trusted key set** holds entries of the form (principal, key id,
public key). An entry says: this public key belongs to a device of this principal.

[SEC-KEY-030] A receiver MUST treat the pair (`security.principal`, `security.key_id`) as
naming a trusted key only when its trusted key set holds an entry with exactly that
principal and that key id.

A key id listed under one principal does not name a trusted key when an envelope claims it
under another.

[SEC-KEY-031] An implementation MUST include its own device key in its trusted key set.

This makes delivery between two sessions of one implementation follow the same rules as
delivery between devices (§7, [SEC-STG-004]). It does not authorize anything by itself
([SEC-AUZ-007]).

[SEC-KEY-032] An implementation MUST NOT add another device's key to its trusted key set
unless an operator confirmed the addition after comparing the key id, or a value derived
from it, through a channel other than the transport that delivered the key.

[SEC-KEY-033] An implementation MUST NOT add a key to its trusted key set because a
signature by that key was received, or because a transport authenticated the connection
that carried it.

Trust is never established on first use. **Pairing** is the operator-confirmed exchange that
the rule [SEC-KEY-032] requires. This revision fixes what pairing establishes, not the wire format of the
exchange.

> **Reference implementation note:** the v0.1 reference implementation pairs two devices
> with a six-digit code derived from both devices' key ids, valid for 120 seconds, with five
> attempts (`docs/planning/decisions/C5-envelope-auth.md` §10(b)). Two devices of one
> implementation on one machine need no exchange: they are one device, with one key
> (C5 §10(a)).

[SEC-KEY-034] An implementation MUST NOT add to its trusted key set a public key whose
encoding is not canonical (RFC 8032 §5.1.3: a y-coordinate not less than p, or a zero
x-coordinate with its sign bit set) or whose point has small order (its order divides 8).

A small-order public key "can be used to generate a signature that's valid for almost every
message" (`ed25519-dalek` 3.0.0, `VerifyingKey::is_weak`,
https://docs.rs/ed25519-dalek/3.0.0/ed25519_dalek/struct.VerifyingKey.html, retrieved
2026-10-03).

[SEC-KEY-035] An implementation that removes a key from its trusted key set MUST, in the
same step, remove every grant (§9.2) and every binding-table entry (§11.3) that names that
key's key id, including every conflict mark that names it.

A revoked device loses every grant it held at once, not only future ones
(`docs/planning/decisions/C5-envelope-auth.md` §11). Removing either key named in a conflict
mark is how an operator resolves the conflict: the session id becomes unbound, and the
remaining device's next claim binds it (fixture `sec-key/SEC-KEY-035.p01`).

### 5.4 The registration record

A **registration record** binds one session id to one device key
(`spec/session-channels.md` [SC-ID-009], §6.7.1). It is a JSON object with these members:

| Member | Content |
|---|---|
| `session_id` | the session id (`spec/session-channels.md` §6.1) |
| `harness_label` | an identifier token naming the harness, for display |
| `native_id` | the harness-native id (`spec/session-channels.md` §6.7.1), a string |
| `working_directory` | the session's working-directory scope as the implementation records it, a string |
| `registered_at` | the registration time, a timestamp |
| `security` | an object with exactly `principal`, `key_id` and `signature` |

The record holds `native_id` and `working_directory`, which nothing on the wire may carry
(`spec/session-channels.md` [SC-ID-006], [SC-ID-045]). It is therefore never published. The
publishable proof of a binding is the signed announcement of §11.

[SEC-KEY-040] A registration record MUST contain exactly the members of the table above.

[SEC-KEY-041] An implementation MUST sign a registration record with its device key over the
signing input of §6.2, using the domain string `oac-registration-v1`.

[SEC-KEY-042] An implementation MUST NOT transmit a registration record, or its `native_id`
or `working_directory` value, to a peer.

[SEC-KEY-043] An implementation MUST NOT use a binding, for attributing a send request,
issuing a presence record or answering a binding-table lookup for its own session, unless it
holds a registration record for that binding whose signature verifies under its own device
key.

> **Reference implementation note:** the record lives only in the memory of the process that
> holds the device key, and ends with its session (`spec/session-channels.md` [SC-ID-156];
> `docs/planning/decisions/C4-session-identity.md` §5).

### 5.5 Rotation and loss

This revision defines no online key rotation (`docs/planning/decisions/C5-envelope-auth.md`
§4). A device that replaces its key is a new device: its peers remove the old key
([SEC-KEY-035]) and pair with the new one ([SEC-KEY-032]). Until they do, a leaked key stays
trusted by every peer that has not removed it. Section 13 records this as a residual risk.

---

## 6. Envelope signing and verification

### 6.1 The `security` members

`spec/session-channels.md` §4.6 fixes the structure of the `security` object: exactly the
string members `principal`, `key_id`, `nonce` and `signature`. This section fixes their
values.

[SEC-SIG-001] A sender MUST set `security.key_id` to the key id of its own device key.

[SEC-SIG-002] A sender MUST set `security.principal` to the principal label under which its
device key is paired.

[SEC-SIG-003] The value of `security.nonce` MUST be the unpadded base64url encoding
([RFC4648] §5) of exactly 16 octets, so that it matches `[A-Za-z0-9_-]{21}[AQgw]` as a
whole value.

[SEC-SIG-004] The value of `security.signature` MUST be the unpadded base64url encoding of
exactly 64 octets, so that it matches `[A-Za-z0-9_-]{85}[AQgw]` as a whole value.

The last character of each pattern allows only the values whose unused low-order bits are
zero, so every octet string has exactly one accepted encoding.

These checks belong to the security stage (§7), not to envelope-stage validation:
`spec/session-channels.md` §3.3 keeps envelope-stage fixtures free to carry placeholder
`security` values.

### 6.2 Signing input and canonicalization

The **signing input** of a signed object is the concatenation of:

1. a domain string, in ASCII;
2. one octet with value 0;
3. the JSON Canonicalization Scheme serialization [RFC8785] of the object with the member
   `security.signature` removed, encoded in UTF-8.

Every other member, including members the signer or the verifier does not recognize, stays
in the canonicalized object. RFC 8785 requires its input to be I-JSON
([RFC8785] §3.1: data "MUST be adapted for I-JSON [RFC7493] formatting"); an envelope already
is ([SC-ENV-002]). RFC 8785 sorts object members by their names as arrays of UTF-16 code
units, and serializes strings and numbers as ECMAScript does (RFC 8785 §3.2.2-§3.2.3,
retrieved 2026-10-03).

The zero octet separates the domain string from the JSON text, which cannot contain a raw
zero octet, so no signing input of one kind can be read as one of another kind
(`docs/planning/decisions/C5-envelope-auth.md` §3). This document defines four domain
strings:

| Domain string | Signed object | Section |
|---|---|---|
| `oac-envelope-v1` | envelope | §6 |
| `oac-registration-v1` | registration record | §5.4 |
| `oac-receipt-v1` | authenticated receipt | §10 |
| `oac-presence-v1` | authenticated presence record | §11 |

`oac-pairing-v1` is reserved for a future pairing exchange
(`docs/planning/decisions/C5-envelope-auth.md` §3).

[SEC-SIG-010] A sender MUST sign an envelope with its device key, as Ed25519 (RFC 8032
§5.1.6), over the signing input of the envelope with the domain string `oac-envelope-v1`.

[SEC-SIG-011] A receiver MUST compute the signing input from the envelope as received,
including every member it does not recognize.

An intermediary therefore cannot add, remove or change any member without breaking the
signature, and member order does not matter. `spec/session-channels.md` §4.8 relies on this
for unrecognized top-level members.

[SEC-SIG-012] A signer MUST NOT change any member of an object after computing its
signature.

RFC 8785 serializes a number from its IEEE 754 double-precision value (RFC 8785 §3.2.2.3).
I-JSON only advises against numbers that a double cannot represent exactly ([RFC7493]
§2.2), so an unrecognized member can still carry one, or carry a number spelled in another
form, such as `1E-7`, `0.10` or `-0`.

[SEC-SIG-013] An implementation MUST canonicalize every JSON number, for signing and for
verification, as the IEEE 754 double nearest to its decimal value (ties to even), serialized
as RFC 8785 §3.2.2.3 requires.

Signer and verifier therefore agree on every number, whatever its spelling and whether or not
a double represents it exactly: `9007199254740993` canonicalizes as `9007199254740992`
(fixtures `sec-sig/SEC-SIG-013.p01` and `.p02`). The members this document and
`spec/session-channels.md` define carry only integers that a double represents exactly.

### 6.3 Verification

Verification follows RFC 8032 §5.1.7, with four rules that close the gaps different
libraries leave open. The public key `A` is the trusted key named by the envelope (§5.3).

Below, `R` is the first 32 octets of the decoded signature, `S` the last 32 octets read as a
little-endian integer, and `M` the signing input.

[SEC-SIG-020] A verifier MUST compute `k` as `SHA-512(R || A || M)`, read as a little-endian
integer and reduced modulo `L`, as RFC 8032 §5.1.7 step 2 does.

[SEC-SIG-021] A verifier MUST reject a signature whose `S` is not less than `L`, the order of
the base point.

RFC 8032 §5.1.7 decodes `S` "in the range 0 <= s < L" (retrieved 2026-10-03). Without the
check, `S + L` is a second valid signature for the same message.

[SEC-SIG-022] A verifier MUST reject a signature whose `R` is not a canonical point encoding
or is a point of small order.

[SEC-SIG-023] A verifier MUST reject a public key `A` that is not a canonical point encoding
or is a point of small order.

[SEC-SIG-024] A verifier MUST accept a signature that passes [SEC-SIG-020] to [SEC-SIG-023]
only when the cofactorless group equation `[S]B = R + [k]A` holds.

RFC 8032 §5.1.7 states the cofactored equation `[8][S]B = [8]R + [8][k]A'` and adds that
"It's sufficient, but not required, to instead check [S]B = R + [k]A'" (retrieved
2026-10-03). The two differ only when `R` or `A` has a small-order component: the cofactored
form then accepts signatures that the cofactorless form rejects (fixtures
`sec-sig/SEC-SIG-024.n04`, mixed-order `R`, and `.n05`, mixed-order `A`). This document fixes
the cofactorless form, so every conformant verifier gives the same verdict, and that verdict
matches the verifier the reference implementation uses (note below).

The strict rules matter in practice. In the E5 vector check (2026-10-03), the Ed25519 verify
of Node.js 25.2.1, built on OpenSSL 3.5.4, accepted fixture `sec-sig/SEC-SIG-022.n01`, whose
`R` is the identity point, and rejected the other `sec-sig` negative fixtures, including both
mixed-order ones. An implementation cannot rely on a library's default verify to meet the
rule [SEC-SIG-022].

*Dated note, 2026-10-03 (#45, review of PR #265): the first draft of this section required
the cofactored equation. It is replaced by the cofactorless one, which agrees with
`verify_strict` below and with OpenSSL on every fixture except the small-order `R` that the
rule [SEC-SIG-022] rejects.*

> **Reference implementation note:** the v0.1 reference implementation calls
> `VerifyingKey::verify_strict` of `ed25519-dalek` 3.0.0
> (`docs/planning/decisions/C5-envelope-auth.md` §2). Its source rejects a signature when
> `signature_R.is_small_order() || self.point.is_small_order()`, then recomputes `R` as
> `vartime_double_scalar_mul_basepoint(&k, &(minus_A), &self.signature.s)`, compresses it and
> compares the octets with the signature's `R`: the cofactorless equation, under which a
> non-canonical `R` encoding never compares equal
> (https://docs.rs/ed25519-dalek/3.0.0/src/ed25519_dalek/verifying.rs.html, `verify_strict`
> and `RCompute::finish`, retrieved 2026-10-03). The same source notes that
> `VerifyingKey::from_bytes` keeps a non-canonical public-key encoding rather than rejecting
> it, so [SEC-SIG-023]'s canonical-encoding rule for `A` is met at key admission
> ([SEC-KEY-034]), not by that call. That the call meets [SEC-SIG-021] to [SEC-SIG-024] on
> every fixture is UNVERIFIED until a Rust build runs them (task F4).

### 6.4 Example

Informative. Fixture `sec-sig/SEC-SIG-010.p01-signature-verifies.json` holds a signed
envelope, its `expected.canonical` text, and the key in `tests/protocol/sec-test-keys.json`
that signed it. Its signing input is `oac-envelope-v1`, the octet `00`, and that canonical
text in UTF-8.

### 6.5 Retransmission and retry

`spec/session-channels.md` §4.9 defines retransmission (the same signed envelope, unchanged)
and retry (a new envelope with a new `id` and a new nonce). A retransmission needs no new
signature. A retry is signed afresh.

### 6.6 Transport security is not a substitute

A transport may authenticate or encrypt its connections. That proves which transport peer
sent bytes, not which device signed an envelope, and it ends when the envelope leaves the
transport (`docs/planning/decisions/C5-envelope-auth.md` §12, §14).

[SEC-SIG-030] A receiver MUST NOT skip or relax any check of §6 or §7 because a transport
authenticated, encrypted or access-controlled the connection that delivered the envelope.

---

## 7. The security stage

### 7.1 Order of checks

`spec/session-channels.md` §8.3.2 runs envelope-stage validation, then the security stage,
then the delivery stage. This section defines the security stage.

[SEC-STG-001] A receiver MUST NOT begin the security stage for an envelope that failed
envelope-stage validation.

Such an envelope gets its envelope-stage code only ([SC-RCP-072]).

[SEC-STG-002] A receiver whose envelope fails one or more security-stage checks MUST report
the code of the earliest failing step of Table 7.1.

Table 7.1.

| Step | Check | Fails when | Code | State |
|---|---|---|---|---|
| 1 | key resolution | (`security.principal`, `security.key_id`) names no trusted key ([SEC-KEY-030]), including when either value is not of the form §5 requires | `unknown-key` | `rejected` |
| 2 | signature | `security.nonce` or `security.signature` is not of the form of [SEC-SIG-003] or [SEC-SIG-004], or the signature does not verify (§6.3) | `signature-invalid` | `rejected` |
| 3 | replay window on arrival | `created_at` is outside the replay window (§8.1, [SEC-RPL-002]) | `outside-replay-window` | `expired` |
| 4 | authorization | `from` is bound to another key or is under conflict ([SEC-AUZ-003]), or neither an inbound grant nor a live reply right covers the envelope ([SEC-AUZ-002], §9.5) | `unauthorized` | `rejected` |
| 5 | duplicate suppression | the duplicate store holds an entry for the envelope (§8.3) | `duplicate` | `duplicate` |

The order has two reasons. A value is not trusted before it is verified, so the timestamp,
`from` and `to` are read only after steps 1 and 2. And nothing about the addressed session
is revealed before step 4: steps 1 to 3 read nothing about `to`, and step 4 reports one code
whether `to` is unknown, unavailable or simply not granted. With [SC-RCP-073], a sender that
is not authorized learns `unauthorized` and nothing else.

[SEC-STG-003] A receiver MUST NOT treat any header member, or `security.principal`, as
authenticated before steps 1 and 2 have succeeded.

This is [SC-ENV-083], made precise: an envelope is **verified** once steps 1 and 2 succeed.

[SEC-STG-004] A receiver MUST apply the security stage to every envelope, including one
signed by its own device key.

### 7.2 After the security stage

An envelope that passes all five steps goes to the delivery stage of
`spec/session-channels.md` §8.3.2. That stage re-checks the hand-off deadline, which
includes the end of the replay window, immediately before the hand-off call ([SC-RCP-091],
[SC-RCP-092]).

---

## 8. Replay window, nonce, and duplicate suppression

### 8.1 The replay window

The **replay window width** `W` is 300 seconds (300000 milliseconds). It is the value
`docs/planning/decisions/C5-envelope-auth.md` §7 chose, equal to the envelope's 300-second
default validity period.

An envelope is **inside the replay window** at receiver time `t` when its `created_at`
instant `c` satisfies `t - W < c < t + W`. Both ends are open. The **end of the replay
window** for `c` is the instant `c + W`: from that instant on, by the receiver's clock, the
envelope is outside the window. This is the instant `spec/session-channels.md` §8.1.3 uses
for the hand-off deadline.

The half of the window after `t` absorbs clock skew: a sender's clock may run ahead of the
receiver's by less than `W`. The **replay-window skew allowance** that
`spec/session-channels.md` §8.4.2 adds to a sender's retry deadline is therefore `W`: **300
seconds**. This is the one place that fixes it.

*Dated note, 2026-10-03 (#45): C5 §7 writes the window as the closed interval
`[now - 300s, now + 300s]`. This document makes both ends open, so that "outside the window
from `c + W`" agrees with [SC-RCP-091], which forbids hand-off at or after the deadline. The
difference is one instant at each edge.*

[SEC-RPL-001] Every implementation MUST use exactly 300000 milliseconds as `W`.

A receiver with a wider window would accept what others refuse, and a sender's retry
deadline (§8.4.2 there) would no longer be safe.

[SEC-RPL-002] A receiver MUST reject, at security step 3, an envelope whose `created_at` is
not inside the replay window at the receiver's time of arrival.

[SEC-RPL-003] A receiver MUST compare `created_at` with its own clock at the full precision
of both values.

`created_at` can carry nine fractional digits (`spec/session-channels.md` §4.4.6). Truncating
either value can move an envelope across an edge.

> **Reference implementation note:** the window is read on the receiver's UTC wall clock,
> because `created_at` is a wall-clock instant. A receiver whose clock is far wrong refuses
> good envelopes as `outside-replay-window`, which the sender sees and can act on.

### 8.2 Nonces

[SEC-RPL-010] A sender MUST generate each new envelope's nonce from 16 octets of output of a
cryptographically secure random number generator.

[SEC-RPL-011] A sender MUST NOT use a nonce for a second envelope.

A retransmission is the same envelope and keeps its nonce ([SC-ENV-102]). A retry is a new
envelope and gets a new one ([SC-ENV-104]).

### 8.3 Duplicate suppression

The **duplicate key** of an envelope is the pair (`security.key_id`, `security.nonce`). It
is not the envelope's `id`, which is unique only per sending session ([SC-ENV-027];
`docs/planning/decisions/C5-envelope-auth.md` §7). Both members are signed and random, so a
copy of an envelope has its duplicate key, and a different envelope does not.

A receiver's **duplicate store** holds entries keyed by duplicate key. An entry records that
a copy of that envelope was handed off, may have been handed off, or is being handed off now.
That is exactly the condition under which [SC-RCP-009] allows `duplicate`.

[SEC-RPL-020] A receiver MUST compare duplicate keys as exact strings.

[SEC-RPL-021] A receiver MUST, at security step 5, test for an entry and, when there is none,
add one, as a single step that no other copy of the same envelope can interleave with.

Two copies arriving together therefore cannot both reach the harness: the second finds the
entry the first added.

[SEC-RPL-022] A receiver MUST remove the entry that a copy added when that copy is not handed
off, that is, when the delivery stage refuses it or the hand-off call fails
(`handoff-failed`).

A copy that was never handed off then does not block a later retransmission
([SC-RCP-009]). An entry stays when the hand-off call succeeds (`handed-to-harness`) or its
outcome is indeterminate (`unknown`), because the harness may hold the content. The entry is
added only once the copy has passed authorization, so a copy refused at steps 1 to 4 adds
nothing.

A copy can find an entry whose own copy is still being handed off: the hand-off call has
started and has not returned. Reporting `duplicate` then could tell the sender `duplicate`
for a message that, a moment later, turns out never to have been handed off.

[SEC-RPL-026] A receiver whose step-5 test finds an entry whose hand-off outcome is not yet
known MUST wait for that outcome before deciding, and MUST then treat the copy as a duplicate
only if the entry remains.

If the earlier hand-off fails, its entry is removed ([SEC-RPL-022]) and the later copy
proceeds as if it had found none (fixture `sec-rpl/SEC-RPL-026.p01`). The wait is bounded by
the hand-off call itself, and the hand-off deadline is checked again before the later copy's
own hand-off ([SC-RCP-091]). It is not a hold for an unavailable session, which
[SC-DLV-007] forbids. With it, `duplicate` is reported only when an earlier copy was handed
off, or its outcome was indeterminate, as [SC-RCP-009] requires.

[SEC-RPL-023] A receiver MUST keep each entry until at least the envelope's hand-off
deadline (`spec/session-channels.md` §8.1.3), read on its own clock.

[SEC-RPL-024] A receiver MAY remove an entry at or after the envelope's hand-off deadline. A
receiver that keeps it longer still reports correctly: every later copy is refused as
`expired` or `outside-replay-window` before step 5.

The eviction bound and the hand-off bound are the same instant. No copy is handed off at or
after the deadline ([SC-RCP-091]), so forgetting the entry then cannot let a duplicate
through.

[SEC-RPL-025] A receiver MAY keep its duplicate store across a restart, for no longer than the
eviction rule of [SEC-RPL-024] allows. A receiver that does not starts with an empty store,
and for up to the replay window after a restart can hand off again a copy of an envelope it
handed off before the restart.

That is the residual `docs/planning/decisions/C5-envelope-auth.md` §8 accepts, and one reason
delivery is not exactly-once (`spec/session-channels.md` §8). A store kept for no longer than
the window holds no content and is not a mailbox.

### 8.4 Receipts for duplicates

Anyone who captures a verified envelope can replay it until its hand-off deadline. Each
replay is a `duplicate`. A receiver that sent a receipt for each one would send the real
sender as many receipts as the attacker sends copies (#45, from E4).

[SEC-RPL-030] A receiver MUST send at most one receipt with state `duplicate` for each
duplicate-store entry.

[SEC-RPL-031] A receiver SHOULD limit the rate of receipts it sends to any one sending
device. A receiver that does not deviates: copies replayed after their window has ended,
which no store entry covers, can each draw an `outside-replay-window` receipt to the real
sender.

---

## 9. Authorization

### 9.1 Default deny

[SEC-AUZ-001] A receiver MUST NOT pass security step 4 for an envelope that neither an
inbound grant nor a live reply right (§9.5) covers.

Nothing is permitted until a grant permits it, apart from the narrow, automatic reply right
of §9.5, which answers only a message the receiver's own session chose to send. That holds
between two devices and between two sessions of one device ([SEC-AUZ-007]). A binding is not
an authorization ([SC-ID-157]), presence is not an authorization ([SC-ID-181]), and pairing
is not an authorization: pairing makes a key trusted, and a grant decides what that key may
reach (`docs/planning/decisions/C5-envelope-auth.md` §11).

### 9.2 Grants

A **grant** says that a **writer** may send to a **target**. It is one-way: it never lets the
target send to the writer. An operator records it on both implementations involved, in the
form each one evaluates:

- an **inbound grant**, on the target's implementation: the writer is a key id (any session
  bound to that device key) or a key id and a session id (that one session); the target is
  one of the implementation's own sessions, one of its working-directory scopes (every
  session registered with that scope), or the whole device (every session it binds);
- an **outbound grant**, on the writer's implementation: the writer is one of its own
  sessions, one of its working-directory scopes or the whole device; the target is a key id,
  or a key id and a session id.

Between two sessions of one implementation the inbound grant alone serves, with the
implementation's own key id as the writer.

An inbound grant **covers** an envelope verified under key id `K` when its writer's key id is
`K`, its writer's session id, if any, is the envelope's `from`, and its target includes the
envelope's `to`.

*Dated note, 2026-10-03 (#45): grants may name one session, one working-directory scope
("folder-wide") or a whole device ("machine-wide"), on either side. This is an operator
decision recorded on #45.*

[SEC-AUZ-002] A receiver MUST pass security step 4 only for an envelope that an inbound grant
or a live reply right (§9.5) covers.

[SEC-AUZ-003] A receiver MUST NOT pass security step 4 for an envelope whose `from` its
binding table (§11.3) binds to a key other than the one that verified the envelope, or marks
as under conflict.

When `from` has no binding yet, the envelope itself is the binding claim: it is signed, and
`from` is inside the signed scope. If the envelope passes step 4, the receiver binds `from`
to the verifying key ([SEC-PRS-005]). If it does not, nothing is bound. An envelope from a
trusted device that claims a session id bound to another key is refused, and the receiver
records a finding ([SEC-PRS-004]). An envelope never sets a conflict mark; only a related
device's presence record can (§11.3, [SEC-PRS-014]).

[SEC-AUZ-004] An implementation MUST NOT use as a grant's writer or target, or as evidence for
a grant decision, a display form, an alias, a `display_name`, a harness label, a principal
label on its own, a transport's peer identifier, a harness-native id, a cross-check value, or
any value taken from `content`.

Each is self-asserted or unauthenticated (`spec/session-channels.md` [SC-ID-021],
[SC-ID-024], [SC-ID-043], [SC-ID-126]; `docs/planning/decisions/C5-envelope-auth.md` §12).

[SEC-AUZ-005] An implementation MUST NOT create a grant on the request of a harness, a
session or a model unless an operator confirms that grant.

A model that asks for access, in content or through a harness, never grants it to itself.

[SEC-AUZ-006] A grant whose target is a session id MUST NOT cover any other session id.

When a conversation moves to a new session id, a session grant does not follow it
([SC-ID-151]). A grant whose target is a working-directory scope or a device covers the new
session because of that scope or device, not because of the old session id.

[SEC-AUZ-007] An implementation MUST NOT treat two sessions as authorized to reach each other
because they belong to the same device, the same harness or the same working-directory
scope.

*Dated note, 2026-10-03 (#45): two sessions on the same machine, in the same project folder,
still need an explicit grant. This is an operator decision recorded on #45.*

### 9.3 Working-directory scoping

A session's **working-directory scope** is the working directory its registration record
holds. Only the implementation holding the binding knows it ([SC-ID-045], and
`spec/session-channels.md` §7 keeps it out of presence records). A grant that names a scope
therefore names one on the implementation that evaluates it: the target side of an inbound
grant, the writer side of an outbound grant.

[SEC-AUZ-008] An implementation MUST treat two working-directory scopes as the same only
when they are equal as the implementation records them.

A grant for one scope does not cover a session in another scope, including a subdirectory of
it. This is the cross-project rule (`docs/planning/decisions/C4-session-identity.md` §5).

### 9.4 Discovery and presence release

`spec/session-channels.md` §7 leaves to this document which sessions a requester is
**authorized to discover**. The answer decides discovery results ([SC-DLV-061]), the release
of presence records ([SC-DLV-066], [SC-DLV-067]), and what a send request may learn about a
session ([SC-DLV-075], [SC-DLV-076]). The rule is one sentence: **a session sees the sessions
it may write to.** Each implementation applies it with the grants it holds.

[SEC-AUZ-010] An implementation MUST NOT treat one of its own sessions as authorized to
discover another of its own sessions unless an inbound grant, with its own key id as the
writer, would cover some envelope from the first to the second.

[SEC-AUZ-011] An implementation MUST NOT release a presence record for one of its own
sessions `S` to a peer device `K` unless an inbound grant lets `K`, or a session of `K`, write
to `S`, or an outbound grant lets `S` write to `K` or a session of `K`.

The first case lets the peer find `S` in order to write to it. The second follows from
sending: a message from `S` reveals `S` to its recipient anyway, and the recipient needs `S`'s
announcement to reply ([SC-DLV-070]).

[SEC-AUZ-012] An implementation MUST NOT treat one of its own sessions `L` as authorized to
discover a session `R` of another implementation unless `R` is bound to a key `K` in its
binding table and an outbound grant lets `L` write to `K` or to `R`, or [SEC-AUZ-016] applies.

An inbound grant does not make the writer discoverable: that `R` may write to `L` does not
let `L` see `R` (fixture `sec-auz/SEC-AUZ-012.n01`), except to reply to a message `R` sent.

[SEC-AUZ-017] A consumer MUST discard an announcement for a session `R` signed by key `K`
unless a grant it holds names `K` (with `R`, or with no session) as an inbound grant's writer
or an outbound grant's target, or it sent an envelope to `R` under `K`, or handed off an
envelope from `R`, within the reply period (§9.5).

A consumer takes in announcements only for sessions it has a reason to know. Unsolicited
records from a trusted device are dropped.

### 9.5 Reply rights, and one grant for a whole exchange

A reply is a send (`spec/session-channels.md` §8.2). With only the grants of §9.2, an answer
would need a second grant in the opposite direction. Instead, sending a message opens a
narrow, automatic, time-limited right to answer that one message (#45, from the PR #263
review).

The **reply period** is 86400000 milliseconds (24 hours), the longest validity
`spec/session-channels.md` allows an envelope ([SC-ENV-050]).

**On the original sender's side.** Let `E` be an envelope that an implementation passed to a
transport, from its session `A` to a session `S` bound to device key `K`.

[SEC-AUZ-013] An implementation that passes an envelope to a transport MUST record a reply
right for it.

[SEC-AUZ-014] A reply right for `E` MUST cover only an envelope that is verified under `K`,
whose `from` is `S`, whose `to` is `A`, and whose `reply_to` is `E`'s `id`.

An uncorrelated message from `S`, or one answering anything else, needs a grant. Correlation
is checked at both ends anyway ([SC-RCP-050], [SC-RCP-060]), so the reply right adds no new
trust in `reply_to`: it accepts only a value matching a record the implementation made
itself.

[SEC-AUZ-015] A reply right for `E` MUST end at the earlier of `E`'s `created_at` plus the
reply period, read on the implementation's own clock, and the end of `A`'s binding.

A reply right is **live** until it ends. It covers an envelope at security step 4 as an
inbound grant does ([SEC-AUZ-002]).

**On the replier's side.** The same exchange, seen from `S`'s implementation, which handed
`E` off to `S`.

[SEC-AUZ-016] An implementation that handed off an envelope to one of its sessions, with the
outcome `handed-to-harness` or `unknown`, MUST treat that session as authorized to discover
the envelope's `from` until the envelope's `created_at` plus the reply period, or until the
receiving session's binding ends, whichever is earlier.

*Dated note, 2026-10-03 (#45): a reply right covers that one message and lasts 24 hours at
most. This is an operator decision recorded on #45.*

**One grant suffices.** Take one grant, "A may write to B", recorded as an outbound grant on
A's implementation and an inbound grant on B's. The rules above then carry a whole request
and reply, with no grant from B to A (fixture `sec-auz/SEC-AUZ-014.p02`):

1. B's implementation releases B's announcement to A's device, because A may write to B
   ([SEC-AUZ-011], first case).
2. A's implementation accepts it, because its outbound grant names B's device
   ([SEC-AUZ-017]), and binds B to B's key. A may now discover B ([SEC-AUZ-012]) and holds
   B's declaration ([SC-DLV-070]).
3. A's implementation issues A's announcement to B's device before the first envelope
   ([SEC-PRS-010]; [SEC-AUZ-011], second case). B's implementation accepts it, because its
   inbound grant names A ([SEC-AUZ-017]).
4. A sends `E`, recording a reply right ([SEC-AUZ-013]). B's implementation accepts `E`
   under its inbound grant. If A's announcement has not arrived yet, `E` binds A by its own
   signature ([SEC-AUZ-003]).
5. B, handed `E`, may discover A ([SEC-AUZ-016]) and send a reply `R` with `reply_to` set to
   `E`'s `id`.
6. A's implementation accepts `R` under the reply right ([SEC-AUZ-014]). An uncorrelated
   message from B is refused, and so is anything from A to a session the grant does not
   name.

### 9.6 Actions beyond delivery

A grant authorizes delivery into the addressed session's input. Some harness surfaces offer
operations that do more. Each is a separate authorization decision, off by default.

[SEC-AUZ-020] An implementation MUST NOT treat a verified signature, a grant, or a
successful pairing as authorizing any action that a message's content requests.

[SEC-AUZ-021] An implementation MUST NOT relay a peer message as an answer to a harness's
permission or approval request unless an operator enabled that relay for the addressed
session as a decision of its own.

With relay enabled, any sender a grant admits could approve the harness's actions. That is
why it is off by default (`docs/planning/decisions/C6-trust-rendering.md` §7, conflict C10).

[SEC-AUZ-022] An implementation MUST NOT hand off a peer message through a **steering
operation**, an operation that a harness documents as amending the instructions of a turn
already running, unless an operator enabled steering for the addressed session as a
decision of its own.

Input that a harness queues and delivers at its own boundaries is ordinary input, not
steering. Whether a particular harness operation steers is a binding's concern; one such
case is an open finding owned by backlog task G7 (#224).

[SEC-AUZ-023] An implementation MUST NOT automate, suppress or pre-answer a consent step that
a harness itself requires before it loads an extension or accepts input from one.

### 9.7 The local attachment

[SEC-AUZ-030] An implementation MUST authenticate the local process at the other end of
each attachment with an operating-system facility before accepting send requests or
discovery requests on it.

`docs/planning/decisions/C2-process-model.md` §4 names the facilities: named-pipe security
descriptors on one platform family, socket permissions and peer credentials on the other.
Whether they behave as designed on every target platform is UNVERIFIED
(`docs/planning/STATUS.md`, "Open UNVERIFIED items"; task G9).

---

## 10. Receipt authentication

### 10.1 The authenticated receipt

`spec/session-channels.md` §8.1.4 defines a receipt's content. A receipt that crosses from
one implementation to another travels inside an **authenticated receipt**, a JSON object with
these members:

| Member | Content |
|---|---|
| `receipt` | the receipt, as `spec/session-channels.md` §8.1.4 defines it |
| `envelope_to` | the `to` of the envelope the receipt describes |
| `envelope_nonce` | the `security.nonce` of that envelope |
| `security` | an object with exactly `principal`, `key_id` and `signature` |

`envelope_to` and `envelope_nonce` tie the receipt to one envelope and one addressed
session. An `id` alone is unique only per sending session, and a nonce is unique per
envelope.

[SEC-RCT-001] A receiver that sends a receipt to another implementation MUST send it as an
authenticated receipt, signed with its device key over the signing input of §6.2 with the
domain string `oac-receipt-v1`.

[SEC-RCT-002] A receiver MUST set `envelope_to` and `envelope_nonce` to the envelope's `to`
and `security.nonce`.

### 10.2 Accepting a receipt

[SEC-RCT-003] A sending implementation MUST discard an authenticated receipt unless all of
the following hold:

1. its `security.principal` and `security.key_id` name a trusted key ([SEC-KEY-030]);
2. its signature verifies under that key (§6.3) with the domain string `oac-receipt-v1`;
3. it holds a record of having sent an envelope whose `id` is the receipt's `envelope_id`,
   whose `from` is the receipt's `envelope_from`, whose `to` is `envelope_to` and whose
   nonce is `envelope_nonce`;
4. `envelope_to` is bound, in its binding table, to the key that signed the receipt (a
   session id under conflict is bound to no key);
5. the receipt's `observer` is `receiver`.

This is the mechanism [SC-RCP-040] names: a receipt is authenticated as coming from the
receiver that serves the envelope's `to` when that session is bound to the signing key.

### 10.3 When a receipt may be sent

[SEC-RCT-004] A receiver MUST NOT let its decision whether to send a receipt, for an envelope
that has not passed security step 4, depend on the envelope's `to` or on any state of the
session `to` names.

Otherwise an unauthorized sender could learn whether a session exists from whether a receipt
arrives (#45, from the PR #261 review). [SC-RCP-042] leaves the decision to send a receipt to
the receiver; this rule only removes `to` from it before authorization.

[SEC-RCT-005] A receiver MUST NOT send a receipt for an envelope that failed security step 1
or 2.

This restates [SC-RCP-041] with the steps named: such an envelope is not verified, and its
`from` is only a claim.

---

## 11. Presence-record authentication and the publishable binding proof

### 11.1 The authenticated presence record

`spec/session-channels.md` §7.2.2 (E3, #43) defines the presence record. A presence record
that crosses from one implementation to another travels inside an **authenticated presence
record**, a JSON object with these members:

| Member | Content |
|---|---|
| `record` | the presence record |
| `audience` | the key id of the one device the record is issued to |
| `security` | an object with exactly `principal`, `key_id` and `signature` |

[SEC-PRS-001] An implementation that issues a presence record to another implementation MUST
issue it as an authenticated presence record, signed with its device key over the signing
input of §6.2 with the domain string `oac-presence-v1`.

[SEC-PRS-011] An issuer MUST set `audience` to the key id of the one device it releases the
record to under [SEC-AUZ-011].

A record released to several devices is therefore issued once for each of them.

[SEC-PRS-013] A consumer MUST discard an authenticated presence record whose `audience` is
not its own device key's key id.

The audience is signed, so a device that received a record cannot pass it on to a third
device that would accept it (fixture `sec-prs/SEC-PRS-013.n01`). The issuer's release
decision is therefore enforced cryptographically, which is how [SC-DLV-066]'s "records reach
only authorized peers" holds even when a transport delivers to others.

[SEC-PRS-010] An implementation MUST issue the sender's announcement to the recipient's
device, under [SEC-AUZ-011], before it passes to a transport the first envelope from that
sender to a session of that device.

A transport need not keep that order (`spec/session-channels.md` §7.4). The receiver does
not need the announcement to accept the envelope: the envelope binds its own `from`
([SEC-AUZ-003]). The announcement is what lets the recipient reply: it carries the sender's
declaration ([SC-DLV-070]).

### 11.2 The binding proof

`spec/session-channels.md` [SC-DLV-043] requires a consumer to authenticate a record "as
issued by the device key to which the record's session id is bound". The registration record
that proves a binding holds the working directory and the harness-native id, so it stays
private ([SEC-KEY-042]).

The publishable proof is a **signed claim** by the device key that names the session id as
its own: an authenticated announcement for it, or a verified envelope whose `from` it is.
Neither carries the working directory ([SC-DLV-032], [SC-ID-045]) or the harness-native id
([SC-ID-006]). Each reveals that the session id belongs to the signing device, and nothing
else. An implementation makes such a claim only for a binding it holds a verified
registration record for ([SEC-KEY-043]).

### 11.3 The binding table

A consumer's **binding table** maps session ids to key ids. It is filled from accepted
announcements and accepted envelopes, and from the implementation's own registration records
for its own sessions. An entry for another implementation's session can instead be a
**conflict mark**, which names the key ids that claimed the session id.

[SEC-PRS-002] A consumer MUST discard an authenticated presence record unless its
`security.principal` and `security.key_id` name a trusted key and its signature verifies
under that key (§6.3) with the domain string `oac-presence-v1`.

[SEC-PRS-003] A consumer MUST discard an authenticated presence record whose `session_id` its
binding table binds to a different key.

[SEC-PRS-004] A consumer that discards a record or an envelope because its session id is
bound to a different key MUST record a finding.

A claim is always noticed, whoever makes it. Whether it also locks the session id depends on
who makes it.

[SEC-PRS-015] A consumer MUST NOT mark as under conflict a session id that it binds by a
registration record of its own.

The consumer knows its own sessions from its verified registration records ([SEC-KEY-043]).
A claim on one of them is false by construction, so it is refused and recorded, and the
session keeps working (fixture `sec-prs/SEC-PRS-015.n01`).

[SEC-PRS-014] A consumer MUST mark a session id of another implementation as under conflict,
naming both key ids, only when the record that conflicts with its binding comes from a key
that passes the relation test of [SEC-AUZ-017] for that session id.

Two related, trusted devices claiming one session id means one of them is misbehaving:
session ids are random and never reused ([SC-ID-003], [SC-ID-008]), and none can be derived
from a key ([SC-ID-004]). The consumer cannot tell which claim is genuine, so it fails closed
for both (fixture `sec-prs/SEC-PRS-003.n01`). A device that nothing relates to the consumer
cannot lock a session id: its claim is refused and recorded without a mark (fixture
`sec-prs/SEC-PRS-014.n01`). An envelope never marks a conflict: an envelope whose `from` is
bound to another key is refused at security step 4 with a finding (rules [SEC-AUZ-003]
and [SEC-PRS-004]; fixture `sec-auz/SEC-AUZ-003.n01`).

[SEC-PRS-012] A consumer MUST NOT bind a session id under a conflict mark to any key, or
accept any record or envelope that claims it, while the mark stands.

The session is then not `online` to the consumer, envelopes from it are refused
([SEC-AUZ-003]), and receipts naming it are discarded ([SEC-RCT-003]). An operator removes
the mark by removing one of the keys it names ([SEC-KEY-035]; fixture
`sec-key/SEC-KEY-035.p01`). The session id is then unbound, and the remaining device's next
announcement binds it again.

[SEC-PRS-005] A consumer MUST add a binding-table entry for an unbound session id only from an
accepted announcement or from an envelope that passed security step 4 with that session id
as its `from`, never from a withdrawal.

A withdrawal for a session id with no binding is discarded. An envelope that fails
authorization binds nothing, so an untrusted or ungranted device cannot claim a session id.

[SEC-PRS-009] A consumer MAY remove a binding-table entry, other than a conflict mark, once
it has forgotten the session ([SC-DLV-048]). A consumer that keeps the entry keeps refusing
other keys' claims on that session id, which is still correct.

The table lives in memory and a restart empties it, conflict marks included. Section 13
records the residual: after a restart, the first related, trusted device to claim another
implementation's session id holds it until a conflicting related claim marks it.

### 11.4 Replay bounding across restart and forgetting

[SC-DLV-042] (`seq` ordering) protects a consumer only while it remembers a session's latest
`seq`. After a restart or a forget, a captured old announcement would be accepted and could
show an ended session as `online` for up to its `lifetime_ms`, one hour at most (#45, from
E3). Two rules bound this without any memory that survives the restart.

[SEC-PRS-006] A consumer MUST discard an authenticated presence record whose `issued_at` is
not inside the replay window (§8.1) at the consumer's time of receipt.

[SEC-PRS-007] A consumer MUST treat an announcement from another implementation as stale no
later than 300000 milliseconds after accepting it, whatever its `lifetime_ms`.

A replayed announcement is then accepted only within `W` of its issue, and counts for at most
`W` after acceptance: an ended session can look `online` for at most 600 seconds after the
announcement was issued, with or without a restart. A live session stays `online` because
its issuer re-announces ([SC-DLV-054]). A replay also reaches only the device named in
`audience` ([SEC-PRS-013]).

[SEC-PRS-008] An issuer SHOULD set `lifetime_ms` to at most 300000 in an announcement for
another implementation. An issuer that sets more deviates: it re-announces at half its stated
lifetime ([SC-DLV-054]), later than [SEC-PRS-007] makes the consumer stale it, so its
sessions flicker to `unreachable` between announcements.

*Dated note, 2026-10-03 (#45): presence between machines is capped at 5 minutes, inside the
one-second-to-one-hour range of [SC-DLV-028]. This is an operator decision recorded on #45.
Records inside one implementation are not affected.*

The checks on an authenticated presence record run in this order: signature
([SEC-PRS-002]), audience ([SEC-PRS-013]), freshness ([SEC-PRS-006]), conflict
([SEC-PRS-003], [SEC-PRS-012], [SEC-PRS-014], [SEC-PRS-015]), relation ([SEC-AUZ-017]),
then the rules of `spec/session-channels.md` §7.2.3. A conflicting claim is recorded even
when it comes from a device the consumer would not otherwise take records from, so a squatter
is noticed, but only a related claimant can lock the session id.

---

## 12. Provenance rendering

### 12.1 The provenance set

An adapter hands a message's content to a harness. With it, the adapter gives the harness
machine-set provenance: values the implementation verified or knows itself, kept apart from
the content. Content is untrusted always, even from an authenticated sender
(`spec/session-channels.md` §4.7). How each harness surface carries provenance is a
binding's concern. The obligations below hold for every adapter.

The **provenance set** of a handed-off envelope is:

| Field | Value |
|---|---|
| sender | the envelope's `from` |
| device | the key id that verified the envelope |
| session | the envelope's `to` |
| message id | the envelope's `id` |
| reply target | the envelope's `reply_to`, or empty when it has none |

[SEC-PRV-001] An adapter MUST render every field of the provenance set with each message it
hands off.

[SEC-PRV-002] An adapter MUST take each provenance value only from the verified envelope's
header and `security` members and the implementation's own verification results, never from
`content`.

### 12.2 Values

[SEC-PRV-003] An adapter MUST refuse to hand off a message any of whose provenance values,
other than an empty reply target, is not an identifier token as a whole value.

[SEC-PRV-004] An adapter MUST NOT escape, truncate or otherwise alter a provenance value to
make it pass [SEC-PRV-003].

[SEC-PRV-014] An adapter that refuses under [SEC-PRV-003] MUST report the state `failed`
with the code `internal-error`.

Envelope-stage validation ([SC-ENV-010], [SC-ENV-011]) and the key-id form (§5.2) already
guarantee the form, so a refusal here means a fault inside the implementation. The rule keeps
a value with a line break from adding a forged provenance line
(Decision C13, `docs/planning/decisions/`, §4 and §8, cases X5 and X5c; gate
G5 refused both in its re-run, `docs/planning/gates/G5-result.md`).

[SEC-PRV-005] An adapter MUST NOT render a display form, an alias, a `display_name` or a
harness label as the sender, device or session of the provenance set.

### 12.3 Carriers

A **separate carrier** is a harness-surface field that holds machine-set values apart from
the content. A **shared carrier** is a single text field that holds both.

[SEC-PRV-006] An adapter MUST refuse to hand off a message when the harness surface would
drop or alter any provenance field.

Some surfaces silently drop a field whose key does not fit their rules
(`docs/planning/decisions/C6-trust-rendering.md` §3). Handing off with partial provenance is
worse than refusing.

When provenance and content share one carrier, the adapter renders a **frame**: a
machine-generated header holding the provenance set, a boundary marker, the quoted body, and
an end marker. The rules below make sure no content line can be read as a header, boundary
or end line (`docs/planning/decisions/C6-trust-rendering.md` §5.0, Option C).

[SEC-PRV-007] An adapter using a shared carrier MUST include in each boundary and end marker
a delimiter of at least 128 bits from a cryptographically secure random number generator,
generated by the receiver for that one hand-off.

[SEC-PRV-008] Before quoting, an adapter using a shared carrier MUST replace each CR LF pair,
and each remaining CR, U+000B, U+000C, U+0085, U+2028 and U+2029, with one LF.

[SEC-PRV-009] Before quoting, an adapter using a shared carrier MUST replace every other
character of general category `Cc` except TAB and LF, and each of U+202A to U+202E and U+2066
to U+2069, with the text `\u{XXXX}`, where `XXXX` is the code point in four upper-case
hexadecimal digits.

[SEC-PRV-010] An adapter using a shared carrier MUST write each body line, as split at LF,
as `| ` followed by the line, or as `|` alone when the line is empty.

A body ending in LF ends with an empty line, which is quoted too. After these steps no body
character can begin a line of the frame.

[SEC-PRV-011] An adapter MAY add a second machine-set carrier that repeats the provenance set
for one hand-off, naming that hand-off's delimiter and message id. An adapter that adds none
still meets every rule of this section with the frame alone, and an adapter that adds one
meets them with the frame alone too.

> **Reference implementation note:** C6 §5.0 fixes the v0.1 frame text and an optional
> anchor carrier for one harness surface. Gate G5 passed against that framing on both v0.1
> harness surfaces (`docs/planning/gates/G5-result.md`, verdict PASS, 2026-10-02/03). G5
> tested a gate client, not OAC's own adapter: in the adapter these rules stay designed until
> the frame-builder and refusal tests of tasks G7 and F11 exist.

### 12.4 Presenting content

[SEC-PRV-012] An adapter MUST present the content of a peer message as untrusted input from
the peer named in the provenance set, never as the words of the harness's user, operator or
system.

[SEC-PRV-013] An adapter MUST NOT place peer content in a carrier that the harness documents
as holding system-level or operator-level instructions.

Even so rendered, the content can still carry prompt injection. Rendering makes the sender
visible; it does not make the content safe (§1.2).

---

## 13. Threat-to-requirement traceability

Each row uses the threat-table template of the `oac-security-work` skill (§1). The
attack column names the matching row of `docs/planning/v0.1/06-security.md` §14 where one
exists; that file keeps the provider- and transport-specific wording, which this document
does not repeat. A proving test is either a committed fixture or a named future test.
Fixtures are data: no runner executes them in CI until tasks E8 and F12. A row whose only
proving test does not exist yet is an open risk, carried as `RISK-SEC-SPEC` in
`docs/planning/v0.1/11-risks.md`, not a closed mitigation.

| Attack | Precondition | Mitigation | Proving test | Residual risk |
|---|---|---|---|---|
| Impersonation: a forged envelope claims a device it does not hold (06 row 1) | Attacker can send to a receiver | Signature over the full envelope with a trusted device key: [SEC-KEY-030], [SEC-SIG-010], [SEC-SIG-024], [SEC-STG-002] | `sec-key/SEC-KEY-030.n01`, `.n02`; `sec-sig/SEC-SIG-024.n03`; F11, H2 | A stolen device key signs validly (row "leaked key" below) |
| Session-id squatting: a trusted device claims another device's session id (in `from` or in an announcement) | Attacker controls a paired device | A bound `from` must match the signing key, and a mismatch is recorded as a finding [SEC-AUZ-003], [SEC-PRS-004]; a second claim by a related device marks the id as under conflict, naming both keys, and fails closed for both until an operator removes one of them [SEC-PRS-003], [SEC-PRS-014], [SEC-PRS-012], [SEC-KEY-035]; an unrelated device's claim is refused without a mark [SEC-PRS-014]; the consumer's own sessions are never marked [SEC-PRS-015]; unauthorized envelopes bind nothing [SEC-PRS-005] | `sec-auz/SEC-AUZ-003.n01` to `.n03`, `.p01`; `sec-prs/SEC-PRS-003.n01`, `SEC-PRS-012.n01`, `SEC-PRS-014.n01`, `SEC-PRS-015.n01`; `sec-key/SEC-KEY-035.p01`; `sec-rct/SEC-RCT-003.n08`; `sec-auz/SEC-AUZ-012.n02` | Until a second related claim arrives, the first related, trusted device to claim another implementation's unbound session id holds it. The table is in memory, so this reopens after every consumer restart and after a forget ([SEC-PRS-009]). A misbehaving device that a grant relates to the consumer can still lock a genuine remote session id until an operator removes its key; an unrelated device cannot |
| Tampering in transit (06 row 3) | Attacker on the transport path rewrites bytes | Every member except the signature is signed: [SEC-SIG-010], [SEC-SIG-011], [SEC-SIG-012] | `sec-sig/SEC-SIG-011.n01`, `.n02`; `sec-sig/SEC-SIG-024.n01`, `.n02` | None beyond the signature itself, by construction |
| Signature malleability and weak or mixed-order points | Attacker alters a valid signature, or offers a small-order, mixed-order or non-canonical `R` or `A` | [SEC-SIG-021] to [SEC-SIG-023]; cofactorless equation [SEC-SIG-024]; [SEC-KEY-034] | `sec-sig/SEC-SIG-021.n01`, `.n02`; `sec-sig/SEC-SIG-022.n01` to `.n03`; `sec-sig/SEC-SIG-024.n04`, `.n05`; F4 | That the reference crate gives these verdicts is checked against its source, not yet by running the fixtures (UNVERIFIED until F4) |
| Cross-protocol reuse: a signature over one kind of object presented as another | Attacker holds a valid signature of one kind | Domain-separated signing input, four distinct domain strings (§6.2) | `sec-sig/SEC-SIG-010.n01`; `sec-key/SEC-KEY-041.n01` | None known |
| Canonicalization mismatch between signer and verifier | Two implementations serialize differently | JCS over I-JSON, computed on the envelope as received: [SEC-SIG-010], [SEC-SIG-011]; numbers as the nearest double [SEC-SIG-013] | `sec-sig/SEC-SIG-010.p01` to `.p04`, `.n02`; `sec-sig/SEC-SIG-011.p01`; `sec-sig/SEC-SIG-013.p01`, `.p02` | Conformance of a given JCS library to RFC 8785 on all inputs is UNVERIFIED; the fixtures cover UTF-16 member order, `\u00XX` escapes, raw U+2028 and DEL, non-ASCII text, number spellings, a number no double represents, and unknown members |
| Replay (06 row 4) | Attacker captured a verified envelope | Replay window [SEC-RPL-001] to [SEC-RPL-003]; duplicate store [SEC-RPL-020] to [SEC-RPL-023], [SEC-RPL-026]; hand-off re-check [SC-RCP-091] | `sec-rpl/SEC-RPL-002.*`, `SEC-RPL-003.n01`, `SEC-RPL-020.*`, `SEC-RPL-021.*`, `SEC-RPL-023.*`, `SEC-RPL-026.*`; F4 | A receiver with no persisted store can hand off a copy again within the window after a restart ([SEC-RPL-025]); delivery is not exactly-once |
| Duplicate suppression that blocks a legitimate retransmission, or reports `duplicate` for a message never handed off | A first copy was refused or failed, or is still being handed off | Entries added at authorization, removed when not handed off, and a copy waits for an in-flight outcome: [SEC-RPL-021], [SEC-RPL-022], [SEC-RPL-026] | `sec-rpl/SEC-RPL-022.p01` to `.p03`; `sec-rpl/SEC-RPL-026.p01`, `.n01` | None known |
| Receipt flooding by replay | Attacker replays a captured envelope many times | [SEC-RPL-030]; rate limit [SEC-RPL-031] | `sec-rpl/SEC-RPL-021.n01`; F6 | Replays outside the window still draw receipts at the rate the receiver allows |
| Unauthorized routing (06 row 2) | A trusted device without a grant sends | Default deny [SEC-AUZ-001], [SEC-AUZ-002], [SEC-AUZ-007]; operator-confirmed grants [SEC-AUZ-005] | `sec-auz/SEC-AUZ-001.n01`, `SEC-AUZ-002.*`, `SEC-AUZ-006.n01`, `SEC-AUZ-007.n01`; F5, H2 | An operator who grants too widely |
| Reply-right abuse: a session that was messaged sends unrelated content, or another session uses the right | A session received an envelope from a session it holds no grant for | Reply right covers only `from` = the original `to`, under the key it was sent to, with `reply_to` = the original `id`, for 24 hours at most: [SEC-AUZ-014], [SEC-AUZ-015], [SEC-AUZ-016] | `sec-auz/SEC-AUZ-014.p01`, `.p02`, `.n01` to `.n03`; `sec-auz/SEC-AUZ-015.n01`; `sec-auz/SEC-AUZ-016.*` | A replier can send any content in its replies, as many as it likes, within the period; content is untrusted anyway (§1.2) |
| Existence oracle: an unauthorized sender learns whether a session exists | Attacker is trusted but not granted | Step order (§7.1) with [SC-RCP-073]; receipt decision independent of `to` [SEC-RCT-004] | `sec-auz/SEC-AUZ-002.n02`; `sec-stg/SEC-STG-002.n03`, `.n04`; the [SEC-RCT-004] differential test is TODO (F6, F11): open risk | Timing differences between steps are not addressed |
| Cross-project disclosure (06 rows 7, 14) | Sessions exist under several working directories | Scope grants by exact scope [SEC-AUZ-008]; discovery and presence release gated by grants [SEC-AUZ-010] to [SEC-AUZ-012]; records addressed to one device [SEC-PRS-013]; unrelated records dropped [SEC-AUZ-017] | `sec-auz/SEC-AUZ-002.n01`, `SEC-AUZ-010.n01`, `SEC-AUZ-011.n01`, `.n02`, `SEC-AUZ-012.n01`, `SEC-AUZ-017.n01`; `sec-prs/SEC-PRS-013.n01`; H2 | Scope equality is as the implementation records the directory; aliases of one directory (links) are not unified |
| Compromised transport, transport-only authenticity, transport peer identifier used as identity (06 rows 6, 9, 10) | A transport node is hostile, or a transport identity is trusted | Envelope verification independent of the transport [SEC-SIG-030]; transport identifiers never name a grant's writer or target [SEC-AUZ-004] | F11, H2: not yet built, open risk | A hostile transport can still drop, delay within the window, duplicate and reorder |
| Model text claims an identity | A peer's content states a sender | Provenance only from verified members [SEC-PRV-002]; never from content [SC-ENV-082] | F11: not yet built, open risk; gate G5 PASS (gate client only) | The model may still believe the content |
| Prompt injection from an authenticated peer (06 row 5) | A trusted, granted peer sends adversarial content | Doctrine §1.2; [SEC-AUZ-020]; untrusted presentation [SEC-PRV-012], [SEC-PRV-013] | G5 PASS (gate client); F11 not yet built, open risk | The model judges. Authentication never makes content safe |
| Provenance forgery in the body of a shared carrier (06 rows 16, 17) | A trusted, granted peer writes frame-shaped text | Receiver-generated delimiter [SEC-PRV-007]; normalization and quoting [SEC-PRV-008] to [SEC-PRV-010] | `sec-prv/SEC-PRV-008.*` to `SEC-PRV-010.*` (body stage); G5 PASS; G7, F11 | A character a model treats as a line break that the closed list omits |
| Header injection through an identifier (C13 X5, X5c) | A value carries a line break | Whole-value check and refusal [SEC-PRV-003], [SEC-PRV-004] | `sec-prv/SEC-PRV-003.n01`, `.n02`; G7, F11 | None in the protocol; adapter code still needs its own tests |
| Silently dropped provenance field (06 row 15) | A harness surface drops a field it cannot carry | Refuse instead of delivering partial provenance [SEC-PRV-006] | F10 contract suite, G4: not yet built, open risk | A surface that drops a field without any observable sign |
| Permission-relay abuse (06 row 11) | Relay of approvals is enabled | Off by default; enabling is a separate operator decision [SEC-AUZ-021] | F11, H2: not yet built, open risk | Once enabled, any granted sender can approve the harness's actions |
| Steering a running turn (06 row 12) | A granted peer's message reaches a harness that offers steering | Steering off unless separately enabled [SEC-AUZ-022] | G7: not yet built, open risk | A harness operation that steers without being documented as steering (#224, G7) |
| Bypass of a harness's own consent step | An implementation automates past a consent prompt | [SEC-AUZ-023] | H2 review: not yet built, open risk | None if the rule holds |
| Local attachment spoofing (06 row 13) | A local process connects pretending to be a harness's attachment | OS-level peer authentication [SEC-AUZ-030] | G9: not yet built, open risk | Platform behaviour UNVERIFIED on Windows (`RISK-LOCAL-IPC`) |
| Leaked device key (06 row 8) | Attacker reads the private seed | Seed never disclosed [SEC-KEY-004]; revocation removes every grant and binding at once [SEC-KEY-035] | F5, F11: not yet built, open risk | No rotation in v0.1: the key stays trusted by every peer until removed |
| Trust on first use: an unconfirmed key becomes trusted | A device offers its key over a transport | Operator-confirmed pairing only [SEC-KEY-032], [SEC-KEY-033]; weak keys refused [SEC-KEY-034] | F5: not yet built, open risk | An operator who confirms without comparing |
| Presence forgery, tampering or forwarding | Attacker sends, alters or forwards a presence record | Signed presence records [SEC-PRS-001], [SEC-PRS-002]; signed audience [SEC-PRS-011], [SEC-PRS-013] | `sec-prs/SEC-PRS-001.p01`, `SEC-PRS-002.n01`, `.n02`, `SEC-PRS-011.n01`, `SEC-PRS-013.n01` | A hostile transport can still suppress records (denial of service) |
| Presence replay after a consumer restart or forget (E3 constraint) | Attacker captured an old announcement | Freshness by the replay window [SEC-PRS-006]; lifetime cap [SEC-PRS-007] | `sec-prs/SEC-PRS-006.n01`, `.p01`; `sec-prs/SEC-PRS-007.p01` | An ended session can look `online` for up to 600 s after its last announcement was issued |
| Receipt forgery, or a receipt from the wrong receiver | Attacker or another trusted device sends a receipt | [SEC-RCT-001] to [SEC-RCT-003] | `sec-rct/SEC-RCT-001.p01`, `SEC-RCT-003.n01` to `.n08` | None known |
| Stale or forged registration binding (06 rows 19, 20, 24) | A binding is used without a valid record | [SEC-KEY-041], [SEC-KEY-043]; binding rules of `spec/session-channels.md` §6.7 | `sec-key/SEC-KEY-041.p01`, `.n01`; `sec-key/SEC-KEY-043.n01` | Pairing of native signals to attachments stays UNVERIFIED per platform (`spec/session-channels.md` §6.7.2) |

Threats that concern external memory services (06 rows 21 to 23) arise outside the protocol
and are not restated here.

---

## 14. References

### 14.1 Normative references

- [RFC2119] Bradner, S., "Key words for use in RFCs to Indicate Requirement Levels",
  BCP 14, RFC 2119. https://www.rfc-editor.org/rfc/rfc2119
- [RFC8174] Leiba, B., "Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words",
  BCP 14, RFC 8174. https://www.rfc-editor.org/rfc/rfc8174
- [RFC8032] Josefsson, S. and I. Liusvaara, "Edwards-Curve Digital Signature Algorithm
  (EdDSA)", RFC 8032. https://www.rfc-editor.org/rfc/rfc8032 (§5.1.3, §5.1.5-§5.1.7,
  retrieved 2026-10-03)
- [RFC8785] Rundgren, A., Jordan, B. and S. Erdtman, "JSON Canonicalization Scheme (JCS)",
  RFC 8785. https://www.rfc-editor.org/rfc/rfc8785 (§3.1, §3.2.2, §3.2.3, retrieved
  2026-10-03)
- [RFC7493] Bray, T., Ed., "The I-JSON Message Format", RFC 7493.
  https://www.rfc-editor.org/rfc/rfc7493
- [RFC4648] Josefsson, S., "The Base16, Base32, and Base64 Data Encodings", RFC 4648
  (§5, base64url). https://www.rfc-editor.org/rfc/rfc4648
- [FIPS180-4] NIST, "Secure Hash Standard (SHS)", FIPS PUB 180-4.
  https://doi.org/10.6028/NIST.FIPS.180-4
- `spec/session-channels.md`, the OAC Session Channels specification.

### 14.2 Informative references

- `docs/planning/ADR-001.md`, "Security model".
- `docs/planning/decisions/C2-process-model.md` §4 (local peer authentication).
- `docs/planning/decisions/C4-session-identity.md` §1, §2, §5, §10-§11.
- `docs/planning/decisions/C5-envelope-auth.md` (algorithm, canonicalization, signed field
  set, replay window, duplicate store, pairing, authorization).
- `docs/planning/decisions/C6-trust-rendering.md` §3, §5.0, §7.
- Decision C13, provenance framing (`docs/planning/decisions/`; §4, §8 and §14, the neutral
  provenance requirements handed to this document).
- `docs/planning/v0.1/06-security.md` §14 (threat table).
- `docs/planning/gates/G5-result.md` (provenance gate verdict).
- `ed25519-dalek` 3.0.0, `VerifyingKey`,
  https://docs.rs/ed25519-dalek/3.0.0/ed25519_dalek/struct.VerifyingKey.html, retrieved
  2026-10-03.

---

## Appendix A. Requirement index

Fixture paths are relative to `tests/protocol/`. `TODO(fixture)` marks a requirement with no
conformance fixture yet and names the task expected to test it. `covered by` names the
requirement whose fixtures exercise it.

| Id | Level | Section | Fixtures |
|---|---|---|---|
| SEC-KEY-001 | MUST | 4 | TODO(fixture): authority derivation is behaviour across the implementation; F11, H2 |
| SEC-KEY-002 | MUST | 5.1 | covered by SEC-SIG-010 and SEC-KEY-041: every fixture signature is Ed25519 |
| SEC-KEY-003 | MUST | 5.1 | TODO(fixture): randomness of key generation; F2 unit tests |
| SEC-KEY-004 | MUST NOT | 5.1 | TODO(fixture): secret handling; F11 |
| SEC-KEY-005 | MUST NOT | 5.1 | TODO(fixture): by construction; F2 |
| SEC-KEY-010 | MUST | 5.2 | `sec-key/SEC-KEY-010.p01` |
| SEC-KEY-011 | MUST | 5.2 | `sec-key/SEC-KEY-011.n01` |
| SEC-KEY-020 | MUST | 5.3 | `sec-key/SEC-KEY-020.n01` |
| SEC-KEY-030 | MUST | 5.3 | `sec-key/SEC-KEY-030.n01`, `.n02` |
| SEC-KEY-031 | MUST | 5.3 | `sec-key/SEC-KEY-031.p01` |
| SEC-KEY-032 | MUST NOT | 5.3 | TODO(fixture): operator pairing flow; F5 |
| SEC-KEY-033 | MUST NOT | 5.3 | TODO(fixture): trust-store admission; F5, H2 |
| SEC-KEY-034 | MUST NOT | 5.3 | TODO(fixture): trust-store admission of weak keys; F5 |
| SEC-KEY-035 | MUST | 5.3 | `sec-key/SEC-KEY-035.p01` |
| SEC-KEY-040 | MUST | 5.4 | covered by SEC-KEY-041 (`sec-key/SEC-KEY-041.p01` carries exactly these members) |
| SEC-KEY-041 | MUST | 5.4 | `sec-key/SEC-KEY-041.p01`, `.n01` |
| SEC-KEY-042 | MUST NOT | 5.4 | TODO(fixture): wire behaviour; F2, H2 |
| SEC-KEY-043 | MUST NOT | 5.4 | `sec-key/SEC-KEY-043.n01` |
| SEC-SIG-001 | MUST | 6.1 | TODO(fixture): sender-side; F2 |
| SEC-SIG-002 | MUST | 6.1 | TODO(fixture): sender-side; F2 |
| SEC-SIG-003 | MUST | 6.1 | `sec-sig/SEC-SIG-003.n01`, `.n02` |
| SEC-SIG-004 | MUST | 6.1 | `sec-sig/SEC-SIG-004.n01` |
| SEC-SIG-010 | MUST | 6.2 | `sec-sig/SEC-SIG-010.p01` to `.p04`, `.n01`, `.n02` |
| SEC-SIG-011 | MUST | 6.2 | `sec-sig/SEC-SIG-011.p01`, `.n01`, `.n02` |
| SEC-SIG-012 | MUST NOT | 6.2 | covered by SEC-SIG-011 (`sec-sig/SEC-SIG-011.n02`, a member added after signing) |
| SEC-SIG-013 | MUST | 6.2 | `sec-sig/SEC-SIG-013.p01`, `.p02` |
| SEC-SIG-020 | MUST | 6.3 | covered by SEC-SIG-024 (every verifying and failing signature fixture) |
| SEC-SIG-021 | MUST | 6.3 | `sec-sig/SEC-SIG-021.n01`, `.n02` |
| SEC-SIG-022 | MUST | 6.3 | `sec-sig/SEC-SIG-022.n01`, `.n02`, `.n03` |
| SEC-SIG-023 | MUST | 6.3 | TODO(fixture): a weak key never enters the trusted key set ([SEC-KEY-034]), so no `security`-stage fixture can present one; F4 unit test |
| SEC-SIG-024 | MUST | 6.3 | `sec-sig/SEC-SIG-024.n01` to `.n05`; positive: `sec-sig/SEC-SIG-010.p01` |
| SEC-SIG-030 | MUST NOT | 6.6 | TODO(fixture): transport configuration; F11, H2 |
| SEC-STG-001 | MUST NOT | 7.1 | `sec-stg/SEC-STG-001.n01` |
| SEC-STG-002 | MUST | 7.1 | `sec-stg/SEC-STG-002.n01` to `.n04` |
| SEC-STG-003 | MUST NOT | 7.1 | covered by SEC-STG-002 (`sec-stg/SEC-STG-002.n02`: a stale timestamp under an invalid signature reports `signature-invalid`) |
| SEC-STG-004 | MUST | 7.1 | `sec-key/SEC-KEY-031.p01` shows the stage on an own-key envelope; failure on one: covered by SEC-AUZ-007 (`sec-auz/SEC-AUZ-007.n01`) |
| SEC-RPL-001 | MUST | 8.1 | covered by SEC-RPL-002 (both window edges at 300 s) |
| SEC-RPL-002 | MUST | 8.1 | `sec-rpl/SEC-RPL-002.p01`, `.p02`, `.n01`, `.n02` |
| SEC-RPL-003 | MUST | 8.1 | `sec-rpl/SEC-RPL-003.n01` |
| SEC-RPL-010 | MUST | 8.2 | TODO(fixture): randomness; F2 |
| SEC-RPL-011 | MUST NOT | 8.2 | TODO(fixture): sender-side; F2 |
| SEC-RPL-020 | MUST | 8.3 | `sec-rpl/SEC-RPL-020.p01`, `.n01`, `.n02` |
| SEC-RPL-021 | MUST | 8.3 | `sec-rpl/SEC-RPL-021.n01`, `.n02`; atomicity under true concurrency: TODO(fixture), F4 |
| SEC-RPL-022 | MUST | 8.3 | `sec-rpl/SEC-RPL-022.p01`, `.p02`, `.p03` |
| SEC-RPL-023 | MUST | 8.3 | `sec-rpl/SEC-RPL-023.n01`, `.p01`, `.p02` |
| SEC-RPL-024 | MAY | 8.3 | none (MAY); `sec-rpl/SEC-RPL-023.p01` holds either way |
| SEC-RPL-025 | MAY | 8.3 | none (MAY) |
| SEC-RPL-026 | MUST | 8.3 | `sec-rpl/SEC-RPL-026.p01`, `.n01` |
| SEC-RPL-030 | MUST | 8.4 | `sec-rpl/SEC-RPL-021.n01` (`duplicate_receipt_allowed`) |
| SEC-RPL-031 | SHOULD | 8.4 | none (SHOULD) |
| SEC-AUZ-001 | MUST NOT | 9.1 | `sec-auz/SEC-AUZ-001.n01` |
| SEC-AUZ-002 | MUST | 9.2 | `sec-auz/SEC-AUZ-002.p01` to `.p04`, `.n01`, `.n02` |
| SEC-AUZ-003 | MUST NOT | 9.2 | `sec-auz/SEC-AUZ-003.n01`, `.n02`, `.n03`, `.p01` |
| SEC-AUZ-004 | MUST NOT | 9.2 | TODO(fixture): grant store by construction; F5, F11 |
| SEC-AUZ-005 | MUST NOT | 9.2 | TODO(fixture): operator confirmation flow; F5 |
| SEC-AUZ-006 | MUST NOT | 9.2 | `sec-auz/SEC-AUZ-006.n01` |
| SEC-AUZ-007 | MUST NOT | 9.2 | `sec-auz/SEC-AUZ-007.n01` |
| SEC-AUZ-008 | MUST | 9.3 | covered by SEC-AUZ-002 (`sec-auz/SEC-AUZ-002.n01`) and SEC-AUZ-010 (`.n01`) |
| SEC-AUZ-010 | MUST NOT | 9.4 | `sec-auz/SEC-AUZ-010.p01`, `.n01` |
| SEC-AUZ-011 | MUST NOT | 9.4 | `sec-auz/SEC-AUZ-011.p01`, `.p02`, `.n01`, `.n02` |
| SEC-AUZ-012 | MUST NOT | 9.4 | `sec-auz/SEC-AUZ-012.p01`, `.n01`, `.n02` |
| SEC-AUZ-013 | MUST | 9.5 | `sec-auz/SEC-AUZ-014.p02` (the `send` step records the right the later `receive` uses) |
| SEC-AUZ-014 | MUST | 9.5 | `sec-auz/SEC-AUZ-014.p01`, `.p02` (a full exchange on one one-way grant), `.n01`, `.n02`, `.n03` |
| SEC-AUZ-015 | MUST | 9.5 | `sec-auz/SEC-AUZ-015.n01`; the end with `A`'s binding: TODO(fixture), F5 |
| SEC-AUZ-016 | MUST | 9.5 | `sec-auz/SEC-AUZ-016.p01`, `.n01`, `.n02` |
| SEC-AUZ-017 | MUST | 9.4 | `sec-auz/SEC-AUZ-017.p01`, `.n01`; in an exchange: `sec-auz/SEC-AUZ-014.p02` |
| SEC-AUZ-020 | MUST NOT | 9.6 | TODO(fixture): behaviour across the implementation; F11 |
| SEC-AUZ-021 | MUST NOT | 9.6 | TODO(fixture): needs a live harness; G4, F11, H2 |
| SEC-AUZ-022 | MUST NOT | 9.6 | TODO(fixture): needs a live harness; G7 |
| SEC-AUZ-023 | MUST NOT | 9.6 | TODO(fixture): review of launch code; H2 |
| SEC-AUZ-030 | MUST | 9.7 | TODO(fixture): needs the platform facilities; G9 |
| SEC-RCT-001 | MUST | 10.1 | `sec-rct/SEC-RCT-001.p01` |
| SEC-RCT-002 | MUST | 10.1 | covered by SEC-RCT-003 (`sec-rct/SEC-RCT-003.n04`, `.n07`) |
| SEC-RCT-003 | MUST | 10.2 | `sec-rct/SEC-RCT-003.n01` to `.n08` |
| SEC-RCT-004 | MUST NOT | 10.3 | TODO(fixture): a differential test over `to`; F6, F11 |
| SEC-RCT-005 | MUST NOT | 10.3 | `sec-rct/SEC-RCT-005.n01`; `receipt_permitted` in every step-1 and step-2 `security` fixture |
| SEC-PRS-001 | MUST | 11.1 | `sec-prs/SEC-PRS-001.p01` |
| SEC-PRS-002 | MUST | 11.3 | `sec-prs/SEC-PRS-002.n01`, `.n02` |
| SEC-PRS-003 | MUST | 11.3 | `sec-prs/SEC-PRS-003.n01`, `SEC-PRS-014.n01`, `SEC-PRS-015.n01` |
| SEC-PRS-004 | MUST | 11.3 | `expected.record` `finding` in `sec-prs/SEC-PRS-003.n01`, `SEC-PRS-014.n01`, `SEC-PRS-015.n01` and `sec-auz/SEC-AUZ-003.n01` |
| SEC-PRS-005 | MUST | 11.3 | `sec-prs/SEC-PRS-005.n01`; from an envelope: `sec-auz/SEC-AUZ-003.p01`, `.n03` |
| SEC-PRS-006 | MUST | 11.4 | `sec-prs/SEC-PRS-006.n01`, `.p01` |
| SEC-PRS-007 | MUST | 11.4 | `sec-prs/SEC-PRS-007.p01` |
| SEC-PRS-008 | SHOULD | 11.4 | none (SHOULD) |
| SEC-PRS-009 | MAY | 11.3 | none (MAY) |
| SEC-PRS-010 | MUST | 11.1 | `sec-auz/SEC-AUZ-014.p02` (a `send` succeeds only after the `release` step issued the announcement); that an implementation issues it first: TODO(fixture), F6 |
| SEC-PRS-011 | MUST | 11.1 | `sec-prs/SEC-PRS-011.n01` (the audience is signed); the issuer's choice of audience: TODO(fixture), F6 |
| SEC-PRS-012 | MUST NOT | 11.3 | `sec-prs/SEC-PRS-012.n01`; `sec-auz/SEC-AUZ-003.n02`, `SEC-AUZ-012.n02`; `sec-rct/SEC-RCT-003.n08` |
| SEC-PRS-013 | MUST | 11.1 | `sec-prs/SEC-PRS-013.n01` |
| SEC-PRS-014 | MUST | 11.3 | `sec-prs/SEC-PRS-014.n01` (unrelated claimant, no mark); `sec-prs/SEC-PRS-003.n01` (related claimant, mark naming both keys) |
| SEC-PRS-015 | MUST NOT | 11.3 | `sec-prs/SEC-PRS-015.n01` |
| SEC-PRV-001 | MUST | 12.1 | TODO(fixture): needs an adapter; F10, F11 |
| SEC-PRV-002 | MUST | 12.1 | TODO(fixture): needs an adapter; F11 |
| SEC-PRV-003 | MUST | 12.2 | `sec-prv/SEC-PRV-003.p01`, `.n01`, `.n02` |
| SEC-PRV-004 | MUST NOT | 12.2 | covered by SEC-PRV-003 (`sec-prv/SEC-PRV-003.n01`, `.n02` expect `refused`, never a rendered altered value) |
| SEC-PRV-005 | MUST NOT | 12.2 | TODO(fixture): needs an adapter; F11 |
| SEC-PRV-006 | MUST | 12.3 | TODO(fixture): needs a harness surface; F10 contract suite, G4 |
| SEC-PRV-007 | MUST | 12.3 | TODO(fixture): delimiter randomness; G7, F11 |
| SEC-PRV-008 | MUST | 12.3 | `sec-prv/SEC-PRV-008.p01`, `.n01` |
| SEC-PRV-009 | MUST | 12.3 | `sec-prv/SEC-PRV-009.p01` |
| SEC-PRV-010 | MUST | 12.3 | `sec-prv/SEC-PRV-010.p01` |
| SEC-PRV-011 | MAY | 12.3 | none (MAY) |
| SEC-PRV-012 | MUST | 12.4 | TODO(fixture): needs a live harness; G5 re-run, F11 |
| SEC-PRV-013 | MUST NOT | 12.4 | TODO(fixture): needs a harness surface; F10, F11 |
| SEC-PRV-014 | MUST | 12.2 | `expected.state` and `expected.error` of `sec-prv/SEC-PRV-003.n01`, `.n02` |

Retired ids: none.

## Appendix B. Follow-ups for other documents

This change does not edit `spec/session-channels.md` or `spec/bindings/mcp.md`. These edits
follow from it and belong to their owners:

1. `spec/session-channels.md` Appendix A: point SC-ID-009, SC-ID-181, SC-ENV-083,
   SC-ENV-104, SC-RCP-009, SC-RCP-040, SC-RCP-041, SC-RCP-072 and SC-DLV-043 at the fixtures
   named here (SEC-KEY-041/043, SEC-STG-002/003, SEC-RPL-021/022, SEC-RCT-003/005,
   SEC-STG-001, SEC-PRS-002).
2. `spec/session-channels.md` §10.1: drop "not yet written" from the `spec/security.md`
   reference; §8.1.5: the "until both exist" paragraph now has its authentication half.
3. `spec/session-channels.md` Table 8.3: widen the conditions of `unknown-key` and
   `signature-invalid` to the malformed-member cases of Table 7.1 here.
4. `spec/session-channels.md` §7.2.3: the dated notes that wait for E5 (no
   cross-implementation records until E5; the replay and binding-proof constraints) can now
   cite §11 here. [SC-DLV-043] stays as written. The same-install-only ruling of the §7.3.2
   dated note stands until a transport binding meets [SC-DLV-066]; this document supplies
   the authentication half only.
5. `spec/session-channels.md` §7.3.2 and §7.3.3: [SC-DLV-061], [SC-DLV-066], [SC-DLV-075]
   and [SC-DLV-076] defer "authorized to discover" to this document; §9.4 and §9.5 here now
   define it, including the one-way-grant rule and the reply right. A sentence there saying that a session handed an
   envelope may discover its sender for the reply period ([SEC-AUZ-016]) would make §8.2's
   reply path readable without this document. The §7.5 `discovery` stage's `discoverable`
   pairs stay a given input.
6. `spec/session-channels.md` §8.2.2: a note that a reply needs the original sender's
   announcement ([SC-DLV-070]), which the sender issues before its first envelope
   ([SEC-PRS-010]), and that an uncorrelated reply needs a grant ([SEC-AUZ-014]).
7. `spec/session-channels.md` §7.2.2: the presence record itself is unchanged; the
   authenticated wrapper of §11.1 here adds a signed `audience`. §7.2.3 could note that a
   consumer also drops records from an unrelated device ([SEC-AUZ-017]) and fails closed on a
   session id claimed by two keys ([SEC-PRS-012]).
8. `spec/session-channels.md` §7.2.4 / [SC-DLV-028]: a cross-reference to the 300-second
   lifetime cap of [SEC-PRS-007] for records from another implementation.
9. `spec/bindings/mcp.md` §6.3: replace "planned, E5" with a citation of §6 here.

## Appendix C. Revision history

| Revision | Date | Change |
|---|---|---|
| 0.1 (draft) | 2026-10-03 | E5 (#45): document written. Device keys, key ids and the registration record; Ed25519 signing over domain-separated JCS with strict verification; the security stage and its codes; the 300-second replay window and skew allowance; duplicate suppression recorded at authorization and released when not handed off; default-deny grants and an automatic, correlated, 24-hour reply right; receipt and presence-record authentication, with the signed announcement as the publishable binding proof and replay bounded across restart; neutral provenance rendering; threat traceability. Fixtures under `tests/protocol/sec-*/` and test keys in `tests/protocol/sec-test-keys.json`. |
| 0.1 (draft) | 2026-10-03 | Review of PR #265: one-way grants, inbound and outbound, with "a session sees the sessions it may write to" and a full request-and-reply on one grant (SEC-AUZ-010 to -017, SEC-PRS-010); a verified, authorized envelope binds its own `from`; the cofactorless equation replaces the cofactored one (SEC-SIG-024), with mixed-order, order-2 and non-canonical `R`, mixed-order `A` and `S = L` fixtures; numbers canonicalize as the nearest double (SEC-SIG-013); presence records carry a signed `audience` (SEC-PRS-011, -013); a session id claimed by two keys fails closed for both (SEC-PRS-003, -012); a copy arriving during an earlier hand-off waits for its outcome (SEC-RPL-026); JCS coverage fixtures; operator decisions on #45 recorded as dated notes. |
| 0.1 (draft) | 2026-10-03 | Second review of PR #265: conflict marks name the claimant keys and are set only by a related claimant (SEC-PRS-014), never on the consumer's own sessions (SEC-PRS-015); an envelope claim is refused with a finding and never marks; removing a key clears marks that name it (SEC-KEY-035); `key-removal` fixture stage; the nonce labels of every fixture are listed in `sec-test-keys.json`. |
