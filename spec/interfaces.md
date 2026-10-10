# OAC Session Channels Interfaces

**Document:** `spec/interfaces.md`, the normative interface contracts of OAC Session
Channels: the core neutral types, the provider adapter contract and the transport contract.
**Revision:** 0.4 (proposed; lead PR approval pending), a minor revision of the 0.1 frozen
at Gate S2 (signed off 2026-10-06, in
force from the merge of PR #276; E7, #47), made under
`docs/planning/decisions/E7-interface-freeze.md` §7: 0.2 (issue #69, PR #350), 0.3
(sealing transports, §6.10; the lead's ruling of 2026-10-09 on PR #364), and 0.4
(bounded connection closure and local identity requests, #376 and #66). Written by #273.
Appendix B records the changes.
**Companion documents:** `spec/session-channels.md` (the protocol), `spec/security.md` (the
security model) and `spec/bindings/mcp.md` (the MCP binding). This document does not restate
their rules. It says which part of an implementation carries out each of them (Appendix C),
and what crosses between those parts.

This document is normative-of-record for the three contracts it defines. It adds no rule
about the wire format, message semantics or security. Where this document and
`spec/session-channels.md` or `spec/security.md` disagree about such a rule, those
documents govern. A binding document (for one harness, or for one transport) maps a contract
onto a concrete surface and is packaging.

This document supersedes the M0 planning draft of the same contracts,
`docs/planning/v0.1/05-interfaces.md` §13-§15. Section 9 lists what changed and why.

---

## 1. Introduction

### 1.1 What this document defines

An implementation of OAC Session Channels has three kinds of part:

- the **core**, which holds identity, keys, policy, presence and delivery state;
- one **adapter** per harness, which connects the core to that harness's supported
  surfaces;
- one or more **transports**, which carry payloads between implementations.

```text
harness <-> adapter <-> core <-> transport <-> (another implementation)
```

The three contracts are the boundaries in that line:

| Section | Contract | Between |
|---|---|---|
| 4 | Core neutral types | every part; the values that cross the other two contracts |
| 5 | Provider adapter contract (`ProviderAdapter`) | an adapter and the core |
| 6 | Transport contract (`Transport`) | the core and a transport |

Section 7 states the neutrality rules that keep harness and transport concepts inside their
own parts. Section 8 covers threats that sit on these boundaries. Appendix C gives every
`MUST` and `MUST NOT` of the four specification documents exactly one owner.

An adapter never talks to a transport. Everything an adapter receives from a harness reaches
a transport only through the core, after the core has applied the protocol and security
rules ([IFC-ADP-001], [IFC-ADP-003]).

### 1.2 How the contracts are written

The contracts are language-neutral. They describe **operations** and **abstract types**,
not functions, classes or a serialization.

- An **operation** has a name, a caller, inputs and an output. An implementation can realize
  it as a function, a method, a message, a callback or a stream. Conformance is about the
  values that cross and the behaviour observed, not about the language form.
- An **event stream** is an output that delivers values over time, on the producer's own
  initiative, until it ends. Reading an event stream is not polling (§7.1.1 of
  `spec/session-channels.md`): the consumer waits for values that the producer pushes.
- A **record type** has named members. Each member has a type and a presence: exactly
  once, or zero or once ("optional").
- A member is a **wire member** when it belongs to a JSON form that
  `spec/session-channels.md` or `spec/security.md` defines. Any other member is a **local
  member**: it exists inside one implementation and never crosses to a peer ([IFC-TYP-002]).
- "One of" lists the only values a type admits.
- A **handle** is an opaque value that one part creates and another part only stores,
  compares for equality and passes back. Nothing reads the inside of a handle (§7).

`docs/planning/DESIGN.md` ("Provider adapter contract", "Transport contract") sketches each
contract as a list of conceptual operations. An operation here keeps its conceptual name
only where its meaning is unchanged. Where the merged specifications changed what an
operation does, the operation has a new name that says what it now does, and the
conceptual name appears only in the mapping table of §9.

### 1.3 Out of scope

- How an adapter reaches one harness, and what that harness's surfaces are called: the
  adapter's binding document ([IFC-ADP-080]).
- How a transport frames, addresses and secures payloads on its network: the transport's
  binding document ([IFC-TRN-090]).
- How a language binding of these contracts looks, including module layout, threading and
  error types.

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
- A `MAY` sentence is followed by what a part that does not take the option still does.

### 2.2 Reference implementation notes

A paragraph labelled as below is informative. It describes a choice of the v0.1 reference
implementation and binds no other implementation:

> **Reference implementation note:** an implementation choice, not binding on other
> conformant implementations.

### 2.3 Terminology

Terms defined in `spec/session-channels.md` §2.3, §6.7.1, §7.1.1 and §8.1.1 and in
`spec/security.md` §2.3 keep their meaning here. In addition:

- **Core, adapter, transport:** the three kinds of part of §1.1.
- **Core process:** the process of an implementation that holds the device key, the
  registration records and the binding table. A harness can neither run code in it nor
  choose the values it observes.
- **Harness-side process:** a process of an implementation that a harness starts or talks to
  directly, outside the core process. It is part of an adapter, but nothing it sends is
  trusted as an identity, an attachment or a pairing key (§5.2).
- **Local connection:** a connection from a harness-side process, or from a process a
  harness started, to the core process, over the operating system's local IPC.
- **Adapter binding document:** the document that maps the adapter contract onto one
  harness's surfaces ([IFC-ADP-080]).
- **Transport binding document:** the document that maps the transport contract onto one
  transport ([IFC-TRN-090]).
- **Payload:** the octets of one envelope, one authenticated presence record or one
  authenticated receipt, as the core passes them to a transport (§6.1).
- **In flight:** a copy of a payload is in flight from the moment the core passes it to a
  transport until the transport hands it to the destination's handler, or drops it.
- **Carrier handle:** an opaque handle by which a transport names the far end of the link
  over which a payload arrived. It names a link, not a device or a session.
- **Request:** a send, reply, discovery or identity request that a harness makes through an adapter.
  It is not an envelope; the core creates the envelope, if any.
- **Owner:** the one part that Appendix C makes responsible for a requirement: the
  `adapter`, the `core`, the `transport`, or a `binding` document.

---

## 3. Conformance

### 3.1 Requirement ids

This document uses the requirement-id scheme of `spec/session-channels.md` §3.2 and
registers the document prefix `IFC` ("interface contracts") for itself. `IFC` does not
collide with `SC`, `SEC` or `MCPB`. Its areas are:

| Area | Section | Subject |
|---|---|---|
| `TYP` | 4 | core neutral types |
| `ADP` | 5 | provider adapter contract |
| `TRN` | 6 | transport contract |
| `NEU` | 7 | neutrality and containment |

Ids are allocated in increasing order within an area, gaps are allowed, and the stability
rules of `spec/session-channels.md` §3.2 apply unchanged.

### 3.2 Conformance fixtures

Fixtures follow `spec/session-channels.md` §3.3. They live under
`tests/protocol/ifc-<area>/` and carry `"spec": "spec/interfaces.md"`. This document defines
no fixture stage of its own. A fixture here uses a stage that `spec/session-channels.md`
(§3.3, §6.10, §7.5, §8.5) or `spec/security.md` (§3.3) defines, with that stage's
`context`, `input` and `expected` members.

Most requirements of this document concern how a running adapter or transport behaves, or
how an implementation is built. No data fixture can show those. Appendix A marks them
`TODO(fixture)` and names the Stage 3 or Stage 4 task that tests them: chiefly F10, the
adapter and transport contract suites, which by their own acceptance run unchanged against
the fake and the real adapters, and against the in-memory and the v0.1 transports.

### 3.3 Owners

Appendix C lists every `MUST` and `MUST NOT` requirement of `spec/session-channels.md`,
`spec/security.md`, `spec/bindings/mcp.md` and this document once, under exactly one owner.
There is no default: a requirement that Appendix C does not list has no owner, and the
reference runner reports it (§3.3 note of `spec/session-channels.md`).

[IFC-ADP-010] An adapter MUST satisfy each requirement that Appendix C assigns to the
`adapter`.

[IFC-ADP-011] The core MUST satisfy each requirement that Appendix C assigns to the `core`.

[IFC-TRN-003] A transport MUST satisfy each requirement that Appendix C assigns to the
`transport`.

A requirement assigned to a `binding` is met by the adapter or transport binding document
that §5.8 or §6.9 requires. An owner can rely on another part's output. For example, the
adapter renders a provenance set from the verified message the core gives it. The owner
alone answers for the requirement.

---

## 4. Core neutral types

### 4.1 General rules

Each type below is either the in-implementation form of a JSON value that
`spec/session-channels.md` or `spec/security.md` defines, or a local type that never leaves
one implementation. Table 4.1 lists them.

Table 4.1.

| Type | Kind | Wire form, where it has one |
|---|---|---|
| `SessionIdentity` | wire | a session id, `spec/session-channels.md` §6.1 |
| `SessionDescriptor` | wire | the session descriptor, `spec/session-channels.md` §6.3 |
| `SessionCapabilities` | wire | the capability declaration, `spec/session-channels.md` §6.4 |
| `ChannelMessage` | wire, with a local member | the envelope, `spec/session-channels.md` §4 |
| `DeliveryReceipt` | wire | the receipt, `spec/session-channels.md` §8.1.4 |
| `PresenceRecord` | wire | the presence record, `spec/session-channels.md` §7.2.2 |
| `PresenceState` | local | none (`spec/session-channels.md` §7.2.1) |
| `SecurityPrincipal` | local | none; built from `spec/security.md` §5.3 |
| `AuthorizationRequest`, `AuthorizationDecision` | local | none (`spec/security.md` §9) |
| Adapter-boundary types (§4.10) | local | none |
| Transport-boundary types (§4.11) | local; a payload's octets are a wire form | the payloads of §6.1 |

[IFC-TYP-001] A core type that Table 4.1 maps to a wire form MUST have, as its wire members,
every member that the wire form defines, with the same names, types and presence.

[IFC-TYP-003] A core type that holds a received wire value MUST keep, as received, each member
that the wire form tells a receiver to ignore.

A later minor revision of a wire form can add an optional member (`spec/session-channels.md`
§5.2). A receiver ignores such a member, under [SC-ENV-090], [SC-ID-044], [SC-DLV-041]
and [SC-RCP-029]. The signature still covers it ([SEC-SIG-011]), so the type keeps it.

[IFC-TYP-002] An implementation MUST NOT place a local member of a core type, or a value of a
local type, inside a payload (§6.1) or inside any other value that it sends to a peer.

Local values do cross the contracts of this document: a `Destination` is an input of
`publish`, and a `RequestResult` reaches a harness through its adapter. They never travel
inside what a transport carries.

### 4.2 `SessionIdentity`

A `SessionIdentity` names one session. It is the opaque session id of
`spec/session-channels.md` §6.1.

[IFC-TYP-010] A `SessionIdentity` MUST be a session id and nothing else.

It carries no key id, key fingerprint, device label, harness label, harness-native id,
display form, alias, working directory or transport address. Which device key a session id
is bound to is not part of its identity. It is a fact the core holds: in its registration
records for its own sessions (`spec/security.md` §5.4) and in its binding table for others
(`spec/security.md` §11.3). Two `SessionIdentity` values are equal only when their strings
are equal ([SC-ID-002]).

### 4.3 `SessionDescriptor`

A `SessionDescriptor` is the session descriptor of `spec/session-channels.md` §6.3, with the
members `session_id` (a `SessionIdentity`), `capabilities` (a `SessionCapabilities`), and
optionally `display_name` and `harness_label`. Section 6.3 there governs every member.

[IFC-TYP-020] A `SessionDescriptor` MUST NOT have a member that holds the session's working
directory, its harness-native id, a cross-check value, its display form or its registration
time.

[SC-ID-045], [SC-ID-006] and [SC-ID-144] keep those values out of the descriptor on the
wire. This rule keeps them out of the type as well, so that no serialization of the type can
leak one. The working directory and the registration time belong to the registration record
(`spec/security.md` §5.4), which is never published ([SEC-KEY-042]).

### 4.4 `SessionCapabilities`

A `SessionCapabilities` is a capability declaration of `spec/session-channels.md` §6.4: a map
from extension identifier to capabilities entry. A capabilities entry has `revision`,
`active_inbound`, and optionally `content_types` and `max_envelope_octets`.

[IFC-TYP-030] A `SessionCapabilities` value MUST be keyed by extension identifier, with one
capabilities entry of `spec/session-channels.md` §6.4 as the value of each key.

A flat record that holds `active_inbound`, a revision and an extension identifier side by
side is not a declaration: a consumer finds in it no member named by an extension identifier
it implements, and agrees no version ([SC-ID-082]; fixture `ifc-typ/IFC-TYP-030.n01`). How the
core builds the entry for one of its own sessions is [IFC-ADP-042].

### 4.5 `ChannelMessage`

A `ChannelMessage` is one envelope inside an implementation.

| Member | Kind | Type | Presence |
|---|---|---|---|
| `envelope` | wire | the envelope as a JSON object: the members of `spec/session-channels.md` §4.2, and every other top-level member it carries | exactly once |
| `verified_by` | local | `SecurityPrincipal` (§4.8) | zero or once |

[IFC-TYP-040] The `envelope` of a `ChannelMessage` MUST hold every top-level member, with its
value, that the envelope had when it was signed, for an envelope the implementation sends, or
when it was received, for an envelope a transport delivered.

The signature covers the envelope as received, every unrecognized member included
([SEC-SIG-011]). A type that dropped a member it does not define, or normalized or
defaulted a value, would verify something other than what was sent. The order of members
does not matter: the signing input is canonicalized (`spec/security.md` §6.2). An
implementation can meet this rule by keeping the received octets beside the parsed value.

[IFC-TYP-041] A `ChannelMessage` MUST NOT have a `verified_by` member until security steps 1
and 2 (`spec/security.md` Table 7.1) have succeeded for it.

[IFC-TYP-042] The `verified_by` member MUST be the trusted-key-set entry that step 1 resolved
and under which step 2 verified the signature.

Before step 2, `security.principal` and `security.key_id` are only claims ([SEC-STG-003]).
`content` stays untrusted after verification too ([SEC-KEY-001], `spec/security.md` §1.2).

### 4.6 `DeliveryReceipt`, `DeliveryState` and `ErrorCode`

A `DeliveryReceipt` is the receipt of `spec/session-channels.md` §8.1.4: `envelope_id`,
`envelope_from`, `state`, `observer`, `error` (exactly when the state carries one) and
`observed_at`.

[IFC-TYP-050] A `DeliveryReceipt` MUST have the members of the receipt of
`spec/session-channels.md` §8.1.4.

A receipt that lacks any one of `envelope_from`, `observer` and `observed_at` is discarded
(fixtures `ifc-typ/IFC-TYP-050.n01` to `.n03`, each the receipt of `.p01` with that one member
removed). `envelope_from` is needed because an envelope id is unique only per sending session
([SC-ENV-027]). The M0 draft's receipt lacked all three (fixture `ifc-typ/IFC-TYP-050.n04`).
A receipt that crosses to another implementation travels inside an authenticated receipt
(`spec/security.md` §10.1), as a payload of §6.1.

A `DeliveryState` is one of the eight values of `spec/session-channels.md` Table 8.1:
`accepted-by-adapter`, `handed-to-harness`, `unknown`, `rejected`, `expired`, `duplicate`,
`unreachable` and `failed`.

[IFC-TYP-051] A `DeliveryState` MUST be one of the eight states of
`spec/session-channels.md` Table 8.1.

The conceptual state `accepted` of `docs/planning/DESIGN.md` is not one of them
(`spec/session-channels.md` §8.1.2 splits it; fixture `ifc-typ/IFC-TYP-051.n01`).

An `ErrorCode` is a string of the form of [SC-RCP-027]. An implementation emits only the codes
of `spec/session-channels.md` Table 8.3 ([SC-RCP-074]), and processes a code it does not know
as [SC-RCP-030] says.

### 4.7 `PresenceRecord` and `PresenceState`

A `PresenceRecord` is the presence record of `spec/session-channels.md` §7.2.2: `session_id`,
`seq`, `present`, `issued_at`, and, in an announcement, `lifetime_ms` and `descriptor`. It is
what an issuer sends. A record that crosses to another implementation travels inside an
authenticated presence record (`spec/security.md` §11.1), as a payload of §6.1.

[IFC-TYP-060] A `PresenceRecord` MUST have the members of the presence record of
`spec/session-channels.md` §7.2.2.

A record without `seq`, or without `present`, is discarded (fixtures `ifc-typ/IFC-TYP-060.n02`
and `.n03`, each the announcement of `.p01` with that one member removed).

A `PresenceState` is one of `online`, `unreachable` and `unknown`
(`spec/session-channels.md` §7.2.1). It is what one observer concludes from the records it
accepted, its clock and carrier loss. It is not a record and it is not sent. A record shaped as
a state, with a `state` and an `observed_at` in place of `seq`, `present` and `issued_at`, as
the M0 draft had it, is discarded (fixture `ifc-typ/IFC-TYP-060.n01`).

[IFC-TYP-061] An implementation MUST NOT pass a `PresenceState` to a transport or to a peer.

A peer computes its own presence states from the records it accepts. A state received from
someone else would be a claim with no `seq`, no lifetime and no signature.

### 4.8 `SecurityPrincipal`

A `SecurityPrincipal` is the identity under which an envelope, a presence record or a
receipt was verified. It has two members: `principal`, a principal label, and `key_id`, a key
id (`spec/security.md` §5.2, §5.3).

[IFC-TYP-070] A `SecurityPrincipal` MUST be the `principal` and the `key_id` of one entry of
the implementation's trusted key set.

A principal label alone is not an identity and never an authorization subject
([SEC-AUZ-004]). A key fingerprint is the key id; there is no second fingerprint form.

### 4.9 `AuthorizationRequest` and `AuthorizationDecision`

An `AuthorizationRequest` asks the core one question of `spec/security.md` §9. An
`AuthorizationDecision` answers it. Table 4.9 lists the kinds of question and the only inputs
each may use.

Two inputs recur. The **own-session facts** are, for each session the implementation binds,
its session id and its working-directory scope (`spec/security.md` §9.3), as the
implementation's registration records hold them at the time of the decision. They let a grant
whose target or writer is a working-directory scope, or the whole device, be evaluated: such a
grant covers the sessions bound at the time of the check (`spec/security.md` §9.2, dated note
after [SEC-AUZ-006]). The **grants** are every grant the implementation holds, in the form of
`spec/security.md` §9.2.

Table 4.9.

| Kind | Question | Inputs | Defined in |
|---|---|---|---|
| `deliver` | Does an inbound grant or a live reply right cover this verified envelope? | the verifying key id; the envelope's `from`, `to` and `reply_to`; the binding table; the grants; the own-session facts; the reply rights | `spec/security.md` §9.1, §9.2, §9.5, Table 7.1 step 4 |
| `discover` | May this requester discover this session? | the requester (an own session id, or a peer device's key id); the session id; the binding table; the grants; the own-session facts; the hand-off records within the reply period | `spec/security.md` §9.4, §9.5 |
| `release-presence` | May this own session's presence record be released to this device? | the session id; the device's key id; the grants; the own-session facts | `spec/security.md` [SEC-AUZ-011] |
| `accept-presence` | Is this announcement's signing key related to this consumer for this session? | the signing key id; the session id; the grants; the sent and hand-off records within the reply period | `spec/security.md` [SEC-AUZ-017] |
| `relay-permission` | May a peer message answer this session's permission request? | the session id; the operator's relay setting for it | `spec/security.md` [SEC-AUZ-021] |

Every input is either authenticated (a key id that verified a signature, a member inside the
signed scope), or a fact the implementation recorded itself (grants, reply rights, binding
table, hand-off and sent records, own-session facts, operator settings). There is no kind for
steering: no decision enables it ([SEC-AUZ-022]).

[IFC-TYP-080] An `AuthorizationRequest` MUST take its inputs only from the inputs that Table
4.9 lists for its kind.

This is the type-level form of [SEC-AUZ-004] and [SEC-AUZ-024]: a display form, an alias, a
`display_name`, a harness label, a principal label alone, a transport peer identifier, a
harness-native id, a cross-check value, a memory reference and anything from `content` have
no slot to enter by.

An `AuthorizationDecision` has `outcome`, one of `permit` and `deny`, and, for `permit`,
`basis`. A `deny` decision of kind `deliver` fails security step 4 with the code
`unauthorized`.

[IFC-TYP-081] An `AuthorizationDecision` MUST be `deny` unless it names, as its basis, the
recorded fact among its kind's inputs in Table 4.9 that permits it.

The basis is a grant, a live reply right, a hand-off or sent record within the reply period,
or an operator setting, as the rule cited for the kind allows. This is default deny
([SEC-AUZ-001]) made structural: a decision with no basis cannot be `permit`.

[IFC-TYP-082] An implementation MUST NOT use the outcome of a decision of one kind as the
outcome of a decision of another kind.

A grant that permits delivery does not permit relay ([SEC-AUZ-020]), and a binding or a
presence state is no decision at all ([SC-ID-157], [SC-ID-181]).

### 4.10 Adapter-boundary types

These local types cross the adapter contract (§5). None of them crosses a transport.

**`Connection`** — a handle that the core creates for one local connection, once it has
authenticated the process at the other end ([IFC-ADP-012]). An **attachment**
(`spec/session-channels.md` §6.7.1) is a `Connection` that the adapter reports as one
(§5.4); an `Attachment` is the `Connection` handle of an attachment.

A `Connection` also supplies its local byte stream, a `close` operation callable by the
core or adapter independently of a blocked stream operation, and `close_bound_ms`, a
finite positive integer giving the maximum elapsed milliseconds for `close` to return.
Closing ends both directions of this connection; it does not change its handle or close
another connection. All copies of its handle and all stream halves refer to the same
closure state. Section 5.2 defines the closure barrier.

**`NativeSignal`** — what an adapter observed at one native signal
(`spec/session-channels.md` §6.7.1):

| Member | Type | Presence |
|---|---|---|
| `native_id` | string, the harness-native id N | exactly once |
| `start_kind` | one of `fresh`, `transition`, `unmapped` | zero or once; absent when the harness reported none |
| `cross_check` | string, the signal's cross-check value | zero or once |
| `connection` | the `Connection` on which the signal arrived | zero or once; absent when no local connection carried it |
| `revealed` | string, a pairing value (§5.6) that the adapter found in the harness's own report | zero or once; present only when the adapter binding document makes the signal a reveal |

It has no pairing key. The pairing key is what the core process observes from the operating
system about the peer of a `Connection` ([SC-ID-121], [IFC-ADP-012]); a signal that arrived
on no local connection has none, unless the adapter binding document names one that the core
process observes itself. `revealed` is such a key: a value the core itself issued on one
attachment (§5.6), which the harness reported back. It narrows the candidates that the
operating-system key leaves to the attachment it was issued on ([SC-ID-125]); it never
replaces that key, and it is never a value the attaching process chose.

**`AdapterCapabilities`** — what an adapter can do for one attachment: `active_inbound` (a
boolean), `content_types` (part types it can hand off besides `text`, optional) and
`max_envelope_octets` (an integer, optional). It has no `revision` and no extension
identifier; the core adds those ([IFC-ADP-042]).

**`SendRequest`** — a harness's request to send:

| Member | Type | Presence |
|---|---|---|
| `attachment` | the `Attachment` on which the core process received the request ([IFC-ADP-031]) | exactly once |
| `to` | `SessionIdentity` | exactly once |
| `content` | content parts (`spec/session-channels.md` §4.5) | exactly once |
| `requested_target` | string: the message the harness says this request answers | zero or once; present only for a reply |
| `conversation_id`, `correlation_id` | strings | zero or once; for a new message |

`requested_target` is untrusted input from the harness, and through it possibly from a model.
It need not be an identifier token. The core uses it only as `spec/session-channels.md`
§8.2.2 allows: a value that names no hand-off record the core holds sends the reply
uncorrelated ([SC-RCP-050] to [SC-RCP-052]).

**`DiscoveryRequest`** — a harness's request for a discovery result: `attachment`.

**`IdentityRequest`** — a harness's request for its own local identity: `attachment`,
with the same attribution as a `DiscoveryRequest`. It has no caller-supplied session id,
fingerprint, principal or key member.

**`LocalIdentity`** — `session_id`, a `SessionIdentity`, and `device_fingerprint`, the
public device signing-key id of `spec/security.md` §5.2 (the full lower-case hexadecimal
SHA-256 digest, 64 digits). These are local result members, not additions to `SessionIdentity` or
to any peer payload. This record holds no key material, including public key bytes.

**`RequestResult`** — the core's answer to a request. For a `SendRequest`, it is one of three:

| Outcome | Members | When (`spec/bindings/mcp.md` §5.3 gives the same split) |
|---|---|---|
| `refused` | `error`, an `ErrorCode` whose scope in Table 8.3 includes `request`; optionally `pairing_value`, a pairing value (§5.6), only with `unauthorized` | no envelope was created ([SC-RCP-075], Table 8.3.3) |
| `not-passed` | `id`, the envelope's `id`; `state`, `failed`; `error`, `transport-failure` or `internal-error` | an envelope was created but not passed to a transport (`spec/session-channels.md` §8.4.1; [IFC-TRN-032]) |
| `sent` | `id`; `state`, `accepted-by-adapter`; for a reply, `correlated` or `uncorrelated` ([SC-RCP-055]); an event stream of the `DeliveryReceipt` values that the core later holds for the envelope | the envelope was passed to a transport |

For a `DiscoveryRequest`: the discovery result (an array of `SessionDescriptor`), or
`refused` with an `ErrorCode` and, as for a `SendRequest`, optionally a `pairing_value`.

For an `IdentityRequest`: a `LocalIdentity`, or `refused` with an `ErrorCode` and,
as for a `SendRequest`, optionally a `pairing_value`. It creates no envelope or receipt.

**`ProvenanceSet`** — the provenance set of `spec/security.md` §12.1: sender, device,
session, message id and reply target.

**`HandOff`** — what the core gives an adapter to hand off: `attachment`, and `message`, a
`ChannelMessage` that passed the security stage and the delivery stage's checks, with its
`verified_by`.

**`HandOffOutcome`** — one of `completed`, `not-now`, `failed`, `indeterminate` and
`refused` (§5.5).

**`HealthStatus`** — `state`, one of `healthy`, `degraded` and `unavailable`, and optionally
`detail`, a diagnostic string for an operator.

[IFC-TYP-090] A `SendRequest` MUST NOT have a member that names the requesting session.

The core attributes a request by its attachment ([SC-ID-160]) and never by a session id the
request carries ([SC-ID-162]). With no such member in the type, an adapter has nowhere to put
one (fixture `sc-id/SC-ID-162.p01` shows a core ignoring one that a request asserts).

[IFC-TYP-091] A `HandOff` MUST carry its `ChannelMessage` with the `verified_by` member that
the security stage set.

The adapter takes every provenance value from that message and its `verified_by`, never from
`content` ([SEC-PRV-002]).

[IFC-TYP-092] A `HealthStatus` MUST NOT contain a credential, private key material, a
transport-native address, a harness-native id or a working directory.

### 4.11 Transport-boundary types

These types cross the transport contract (§6).

**`Destination`** — where a payload goes: `session`, a `SessionIdentity`, for an envelope;
or `device`, a key id, for an authenticated presence record or an authenticated receipt.

**`Payload`** — `kind`, one of `envelope`, `presence`, `receipt` and `sealed` (§6.1), and
`octets`.

**`Deadline`** — an instant, read on the clock of the implementation that passes the
payload, after which no copy of the payload is delivered (§6.4).

**`Inbound`** — what a transport hands to the core: a `Payload` and the `CarrierHandle` of
the link it arrived on.

**`CarrierHandle`** — a handle that names one link of the transport (§2.3).

**`PublishResult`** — one of `taken` and `not-taken` (§6.4).

**`TransportCapabilities`** — the declaration of §6.3.

**`TransportConfiguration`** — a handle that the transport defines and the core passes to
`start` unread (§7).

[IFC-TYP-095] A `Destination` MUST be either a session id or a key id, and nothing else.

A transport-native address is never a `Destination`. The transport derives one from the
`Destination`, inside the transport module ([IFC-NEU-003]).

---

## 5. Provider adapter contract

### 5.1 The adapter's role

An adapter connects the core to one harness. It interprets the traffic of the local
connections the core gives it, observes the harness's sessions through the harness's
supported surfaces, hands content off through the harness's supported input surface, renders
provenance, and takes the harness's requests. It decides nothing about identity, attachment,
authorization, presence or delivery state. Those are the core's.

[IFC-ADP-001] An adapter MUST NOT invoke an operation of a transport.

[IFC-ADP-002] An adapter MUST NOT create, sign, verify or alter an envelope, a presence record,
a registration record or a receipt.

[IFC-ADP-003] An adapter MUST pass each send, reply and discovery request that a harness makes
to the core, through the request sink of `accept_requests` (§5.6).

[IFC-ADP-004] An adapter MUST NOT hand off to a harness any content other than the content of a
`HandOff` that the core passed to it.

[IFC-ADP-007] An adapter MUST NOT create a binding, assign a session id or make an
authorization decision.

Together these route every message through core policy and security: a request becomes an
envelope only in the core, after the checks of `spec/session-channels.md` §6.5-§6.8 and §7.3
and `spec/security.md` §9; and an envelope reaches a harness only after the core's
envelope, security and delivery stages (`spec/session-channels.md` §8.3.2).

[IFC-ADP-005] An adapter MUST NOT read, hold or use a provider credential of the harness it
serves.

[IFC-ADP-006] An adapter MUST NOT call a provider's model interface.

These two state, for the adapter, the `docs/planning/ADR-001.md` "Boundary" rules against
calling provider model APIs as a substitute for a native harness and against reusing another
harness's provider credentials. The harness keeps its own inference, context and login.

### 5.2 Connections and attachments

Requests are attributed by the attachment they arrive on ([SC-ID-160]). An attachment
therefore has to be something no harness-side process can choose or forge. It is a local
connection that the core process accepted and authenticated.

[IFC-ADP-012] The core MUST create every `Connection` handle in the core process, for a local
connection whose peer it has authenticated with an operating-system facility.

That authentication is [SEC-AUZ-030]. The same observation of the peer, made by the core
process, is the pairing key of [SC-ID-121].

[IFC-ADP-013] An adapter MUST NOT create, choose or alter a `Connection` handle.

[IFC-ADP-031] An adapter MUST label each request it passes to the core with the `Connection` on
which the core process received the request, never with a value that a harness-side process
sent.

**Connection closure.** A write settled by closure has a terminal result: `completed` if all requested
octets were transferred, or `closed` if closure stopped it, with the exact number of octets
already transferred (possibly zero). A partial transfer is not a successful harness input
call. The byte-stream result establishes only what the stream transferred; it does not
establish what the harness accepted. A harness-level outcome that remains unknowable is
still `indeterminate` under [IFC-ADP-053], with no retry ([IFC-ADP-057]).

[IFC-ADP-014] The core MUST supply every `Connection` with the closure operation and bound
defined in §4.10.

[IFC-ADP-015] The core's `Connection.close` MUST return within its `close_bound_ms` even
when the peer stops reading or writing.

[IFC-ADP-016] The core's `Connection.close` MUST settle every pending stream operation
before it returns.

Settled writes have the terminal results defined above; settled reads report end-of-stream.
An ordinary I/O failure that preceded closure retains its failure result.

[IFC-ADP-017] The core's `Connection` MUST NOT transfer further octets after `close` returns.

[IFC-ADP-018] The core's `Connection` MUST refuse every stream operation started after
closure begins, without transferring octets.

[IFC-ADP-019] The core's `Connection.close` MUST be idempotent across all handles and stream
halves of that connection.

Completed transfers preceding the closure barrier stay completed. Closure cannot recall
content the harness already holds. A timeout that leaves a writer running, or merely
dropping one stream half while another still writes, does not satisfy this barrier.

A harness-side process can name a session, an attachment or a harness-native id in what it
sends. Each such value is a claim. None of them selects the attachment of a request, and none
is used as a pairing key ([SC-ID-121], [SC-ID-162]).

### 5.3 Operations

Table 5.2. `ProviderAdapter`.

| Operation | Caller | Inputs | Output |
|---|---|---|---|
| `take_connection` | core | a `Connection` | none |
| `watch_attachments` | core | none | an event stream of adapter events (§5.4) |
| `set_binding` | core | an `Attachment`; a `SessionIdentity`, or none | none |
| `capabilities` | core | an `Attachment` | `AdapterCapabilities` |
| `deliver` | core | a `HandOff` | a `HandOffOutcome` |
| `accept_requests` | core | a request sink: an operation that takes a `SendRequest`, a `DiscoveryRequest` or an `IdentityRequest` and returns a `RequestResult` | none |
| `health` | core | none | `HealthStatus` |
| `shutdown` | core | none | none |

The core calls every operation. The adapter speaks to the core only through the outputs of
those calls: the event stream of `watch_attachments`, the result of each call, and the
request sink that `accept_requests` gave it.

### 5.4 `take_connection`, `watch_attachments` and `set_binding`: session binding signals

`take_connection` gives the adapter one local connection that the core accepted and
authenticated. The adapter reads the harness's traffic on it, decides from its binding
document whether the connection is an attachment or a carrier of native signals, and reports
what it observed on `watch_attachments`. That stream does not yield sessions or descriptors:
a session has no session id until the core binds it, and the descriptor is the core's
(`spec/session-channels.md` §6.3). Its events are:

| Event | Members | Meaning |
|---|---|---|
| `attachment-opened` | `attachment`, a `Connection`; optionally `cross_check`, the attachment's cross-check value | the connection is a local path to one live session |
| `native-signal` | `signal`, a `NativeSignal` | a native signal arrived through the surface the adapter binding document names as authoritative for N |
| `capabilities-changed` | `attachment` | `capabilities` for that attachment would now return a different value |
| `attachment-closed` | `attachment` | the attachment ended |
| `attachment-unconfirmed` | `attachment` | the adapter could not confirm, through a surface its binding document names, that a request on the bound attachment comes from the bound session |

The core pairs each native signal with an attachment using the pairing keys it observed for
both connections, and applies the ordered cases of `spec/session-channels.md` §6.7.3.

[IFC-ADP-020] An adapter MUST report attachments and native signals only as it observed them
through a surface that its adapter binding document names.

[IFC-ADP-022] An adapter MUST report `attachment-closed` for each attachment it reported open,
when that attachment ends.

The core then deregisters the binding ([SC-ID-155]) and issues a withdrawal ([SC-DLV-055]).

[IFC-ADP-043] An adapter MUST report `capabilities-changed` when the value that `capabilities`
returns for an attachment changes.

The core then issues a new announcement ([SC-DLV-052]).

[IFC-ADP-091] An adapter that reports `attachment-unconfirmed` for a request MUST report it
before it passes that request to the core.

The core then refuses the request as one from an attachment it does not serve, so a request
the adapter could not confirm is never served. An adapter whose binding document names no
confirmation never reports the event (for one v0.1 harness, `spec/bindings/mcp.md` §4.5
names one).

[IFC-ADP-092] On `attachment-unconfirmed`, the core MUST stop accepting requests from, and
handing off to, that attachment until a later native signal is paired with it.

[IFC-ADP-093] On `attachment-unconfirmed`, the core MUST record a finding.

The stop is the one [SC-ID-154] imposes after an unattributed signal, and it ends the same
way: a later paired signal, which for an attachment already bound to the same harness-native
id leaves its binding unchanged ([SC-ID-130]).

`set_binding` tells the adapter the core's binding for one attachment: the session id it
bound, or none when the attachment is unbound, its binding ended, or delivery to it is
withheld under [SC-ID-154].

[IFC-ADP-030] An adapter MUST NOT hand off to an attachment for which the core's latest
`set_binding` call named no session.

An adapter still passes requests from an unbound attachment to the core, which refuses them
with `unauthorized` ([SC-ID-161]).

### 5.5 `capabilities` and `deliver`: capability declaration and inbound hand-off

**`capabilities`.** The adapter reports what it can do for one attachment. The core turns
that into the capabilities entry it announces.

[IFC-ADP-040] An adapter MUST NOT report `active_inbound` as `true` for an attachment unless
its `deliver` operation hands off to that attachment by active delivery, without polling
(`spec/session-channels.md` §7.1).

[IFC-ADP-041] An adapter MUST NOT report a content part type or a size that it cannot hand off
unchanged.

A part type or size that the adapter would have to drop, split or rewrite is not supported:
the core never changes a message to fit ([SC-ID-103]).

[IFC-ADP-042] The core MUST build each capabilities entry for one of its own sessions from a
revision it implements and the `AdapterCapabilities` that the session's adapter reports.

An implementation that cannot meet active delivery for a session declares `active_inbound`
`false`, and the session is send-only ([SC-ID-104], §6.6 of `spec/session-channels.md`).

**`deliver`.** The core calls `deliver` once it has accepted an envelope for hand-off
(`spec/session-channels.md` §7.1.1), after the delivery stage's re-check of the hand-off
deadline ([SC-RCP-091]). The adapter computes the provenance set from the verified message
([SEC-PRV-002]), renders it, makes the hand-off call through the harness's supported input
surface, and reports what it observed.

The hand-off call never steers a running turn ([SEC-AUZ-022]). Where the harness's input
surface offers a hand-off that holds input for a turn of its own, the adapter uses that
**holding hand-off** whenever the session is running a turn or may be running one
([SEC-AUZ-025], [SEC-AUZ-026]). If the holding hand-off is turned away, the adapter reports
that and makes no other call ([SEC-AUZ-027]). The adapter binding document names the holding
hand-off and every steering operation of its harness ([IFC-ADP-080]); for one v0.1 harness
that is [MCPB-CDX-002] to [MCPB-CDX-005].

[IFC-ADP-050] An adapter's `deliver` operation MUST return exactly one `HandOffOutcome` for each
`HandOff`.

[IFC-ADP-057] An adapter MUST make at most one hand-off call for each `HandOff`.

A second call for the same `HandOff`, through any operation, is how a turned-away holding
hand-off would become a steering one ([SEC-AUZ-027]), and how one message would be handed off
twice.

[IFC-ADP-051] An adapter MUST return `completed` only when the harness surface's input call
completed successfully, as that surface defines completion.

[IFC-ADP-052] An adapter MUST return `not-now` only when the harness surface turned the call
away as unable to take input now, without having taken the input.

[IFC-ADP-053] An adapter MUST return `indeterminate` when the input call returned neither
success nor failure.

[IFC-ADP-054] An adapter MUST return `refused`, without making the input call, when the
harness surface would drop or alter a provenance field, or a provenance value
fails [SEC-PRV-003].

A `refused` outcome is the adapter's report of the state `failed` with the code
`internal-error` ([SEC-PRV-014]).

[IFC-ADP-056] An adapter MUST NOT hold a `HandOff` for a later input call.

A not-now refusal is reported at once, and the sending implementation decides what to do
([SC-DLV-007]). Input that the harness itself holds after a completed call, as a holding
hand-off does, is the harness's.

An adapter returns `failed` for any other failure of the input call. Table 5.3 gives the
state and code the core records for each outcome.

Table 5.3.

| `HandOffOutcome` | Delivery state | Code | Rule |
|---|---|---|---|
| `completed` | `handed-to-harness` | none | [SC-RCP-004] |
| `not-now` | `unreachable` | `destination-unavailable` | [SC-DLV-008] |
| `failed` | `failed` | `handoff-failed` | [SC-DLV-009] |
| `indeterminate` | `unknown` | none | [SC-RCP-006] |
| `refused` | `failed` | `internal-error` | [SEC-PRV-014] |

*Dated note, 2026-10-04 (#273): [SEC-PRV-006] names no code for a refusal because a surface
would drop or alter a provenance field. Table 5.3 gives it `internal-error`, the code
that [SEC-PRV-014] gives the other provenance refusal: no hand-off call is made, so
`handoff-failed` does not fit. This was chosen here and accepted in the review of PR #279.*

[IFC-ADP-055] The core MUST record, for each `HandOffOutcome`, the delivery state and code
that Table 5.3 assigns to it.

No outcome means that a model read, understood or acted on the message
(`spec/session-channels.md` §8.1.3). An adapter whose harness gives no signal stronger than the
end of a write reports `completed` at the end of the write, and nothing more.

### 5.6 `accept_requests`: requests, refusals and receipts

`accept_requests` gives the adapter the core's request sink. The adapter passes each request
a harness makes into the sink ([IFC-ADP-003]) and returns the `RequestResult` to the harness.
A refusal is a `RequestResult` with an `ErrorCode` from Table 8.3 whose scope includes
`request` ([SC-RCP-075]), never a silent drop ([SC-ID-102]). An envelope that the core created
but could not pass to a transport is a `not-passed` result, not a refusal (§4.10).

[IFC-ADP-060] An adapter MUST return to the harness the core's `RequestResult` for each request,
with its outcome and its `ErrorCode` unchanged.

An adapter binding document says how the harness's surface carries the result; the meaning
does not change on the way.

**Local identity.** The identity request is the contract path for returning the caller's
own session id and public device fingerprint, including the result required by
`spec/bindings/mcp.md` [MCPB-TOOL-006]. It does not authorize delivery or any action.
Authenticated peer messages remain untrusted instructions (§8; `spec/security.md` §1.2).

[IFC-ADP-094] An adapter MUST obtain the identity it returns to a harness through an
`IdentityRequest` attributed to that harness's attachment.

[IFC-ADP-095] The core MUST refuse an `IdentityRequest` with `unauthorized` unless it
currently serves a bound session on the request's attachment.

[IFC-ADP-096] The core MUST construct a successful identity result from the session binding
and its local device signing-key fingerprint at one instant during the request.

[IFC-ADP-097] The core MUST return a successful identity result with exactly the two
`LocalIdentity` members defined in §4.10.

[IFC-ADP-098] The core MUST NOT expose key material through the identity request or result.

The session id and fingerprint describe the same binding at that instant; a subsequent
unbinding does not retroactively change an already returned result. There is no fallback
to a previous binding, content, a display form, or a harness-provided identity claim.
The fingerprint is a public digest, never the device's signing or agreement key bytes,
private seed, credential, or a capability to read those values. Existing request refusal
and confirmation rules still apply, including [IFC-ADP-092].

**Pairing values.** An adapter binding document can make the harness's own report of a
refused request a native signal, by having the refusal carry a value the core issued on that
attachment (for one v0.1 harness, `spec/bindings/mcp.md` §4.5). The core issues the value in
the `pairing_value` of an `unauthorized` refusal; the adapter returns it to the harness as its binding document says
(`spec/bindings/mcp.md` [MCPB-ATT-004]) and reports any harness report that carries it back as a `native-signal` whose
`revealed` is that value ([IFC-ADP-020]).

[IFC-ADP-090] The core MUST draw each pairing value it issues from a cryptographically secure
random source, with at least 128 bits of entropy.

[IFC-ADP-062] An adapter MAY present to the harness the `DeliveryReceipt` values that arrive on
a `sent` result's event stream. An adapter that does not still returns the `RequestResult`,
and the harness sees only `accepted-by-adapter`.

### 5.7 `health` and `shutdown`

`health` returns a `HealthStatus` ([IFC-TYP-092]).

The adapter binding document states `shutdown_bound_ms`, a finite positive integer bounding
elapsed milliseconds from entry to return of `shutdown`. The bound covers connection
closure, pending call settlement and attachment-closed reporting; it is independent of
whether the harness reads or responds. The core supplies connections whose closure bounds
fit that shutdown bound. Closing independent connections concurrently is an implementation
choice, not a relaxation of the bound.

[IFC-ADP-072] An adapter MUST stop starting new hand-off calls when `shutdown` begins.

[IFC-ADP-078] An adapter MUST stop accepting new harness requests when `shutdown` begins.

[IFC-ADP-073] An adapter MUST close every `Connection` it received before its `shutdown`
returns.

[IFC-ADP-074] An adapter MUST return from `shutdown` within its binding's
`shutdown_bound_ms`.

[IFC-ADP-075] An adapter MUST settle every in-flight hand-off call
before its `shutdown` returns.

[IFC-ADP-076] An adapter binding document MUST state the finite positive
`shutdown_bound_ms` defined above.

[IFC-ADP-077] The core MUST supply an adapter only connections whose `close_bound_ms`
does not exceed that adapter binding's `shutdown_bound_ms`.

Closing connections precedes waiting for workers that might be blocked on their streams.
Pending `deliver` calls return one outcome under §5.5; cancellation is not evidence of
successful hand-off. Already completed input held by the harness remains the harness's
([IFC-ADP-056]). A detached writer that could finish after return violates [IFC-ADP-070].

[IFC-ADP-071] An adapter MUST report `attachment-closed` for each attachment still open before
its `shutdown` returns.

[IFC-ADP-070] After its `shutdown` returns, an adapter MUST NOT hand off content or pass
requests to the core.

### 5.8 Adapter binding documents

[IFC-ADP-080] An adapter MUST have an adapter binding document that names, for its harness,
the surfaces it uses for attachments, native signals and hand-off; the mapping of each
harness-reported start value to a start kind; what completion means for its input call; its
holding hand-off, if the surface offers one, and each of its steering operations
([SEC-AUZ-022]); and the stability label of each surface it uses.

The binding document is where a harness's own names belong, never this document (§7).

> **Reference implementation note:** the v0.1 per-device process is the core process. It
> holds the core and the transport, and accepts and authenticates the local connections of
> the thin processes that each harness starts (`docs/planning/decisions/C2-process-model.md`
> §1, §4). The adapter for one harness spans that thin process and its half of the
> per-device process. Today the material that [IFC-ADP-080] asks for is split:
> `spec/bindings/mcp.md` covers the surfaces both v0.1 adapters share, with each harness's
> profile in its §8, and `docs/planning/decisions/C4-session-identity.md` §3-§4 records the
> native signals and attachments. The adapter tasks (G4 to G8) bring it together for each
> harness.

---

## 6. Transport contract

### 6.1 The transport's role

A transport carries payloads between implementations. It carries three kinds, each with a
deadline that the core computes and passes with it.

Table 6.1.

| `kind` | Payload | Defined in | `Destination` | `Deadline` |
|---|---|---|---|---|
| `envelope` | the serialized envelope | `spec/session-channels.md` §4 | `session`: the envelope's `to` | the envelope's hand-off deadline (`spec/session-channels.md` §8.1.3) |
| `presence` | the serialized authenticated presence record | `spec/security.md` §11.1 | `device`: the record's `audience` | the end of the replay window for its `issued_at` ([SEC-PRS-006]) |
| `receipt` | the serialized authenticated receipt | `spec/security.md` §10.1 | `device`: the key id that verified the envelope the receipt describes | the hand-off deadline of the envelope it describes |
| `sealed` | a sealed frame holding one payload of one of the three kinds above | `spec/security.md` §14.4 | `device`: the key id of the device the frame is sealed to, which is the device the payload inside would name | the `Deadline` of the payload inside, known to the sending end only ([IFC-TRN-107]) |

The receipt goes to the key that verified the envelope, not to the key that `from` is bound
to: an envelope refused at security step 3 or 4 may have a `from` that is unbound, or bound
to another key ([SEC-AUZ-003], [SEC-PRS-005]), and a receipt for it may still be sent
([SEC-RCT-005] forbids receipts only for steps 1 and 2).

[IFC-TRN-001] A transport MUST carry payloads of each of the three kinds of Table 6.1.

A sealing transport (§6.10) carries the three kinds inside payloads of kind `sealed`, which
meets [IFC-TRN-001]. A transport that does not declare `sealing` never sees that kind.

The presence payload is the whole record, including its descriptor and capability
declaration, not a reachability signal. A transport that only reports whether a peer is
reachable does not meet the contract; it can still report carrier loss (§6.6).

A transport serves the core only. Delivery between two sessions of one implementation follows
the same rules as delivery between devices ([SEC-KEY-031], [SEC-STG-004]).

[IFC-TRN-002] An implementation MAY deliver envelopes between two of its own sessions through a
transport that carries payloads only within that implementation. An implementation that does
not uses the same transport as for other implementations.

> **Reference implementation note:** the in-memory transport planned as task F7 is a
> transport of that kind: it implements this contract with no network, and its fault
> injection serves the transport suite of task F10.

### 6.2 What a transport is not trusted for

A transport is not trusted for confidentiality, integrity, authenticity, order, delivery, or
delivery at most once. `spec/session-channels.md` §7.4 and §8 already assume a transport that
can lose, delay, duplicate and reorder; `spec/security.md` §6.6 makes envelope verification
independent of any transport security. Confidentiality of content against a transport comes
only from payload sealing (`spec/security.md` §14), which the core does itself for a sealing
transport (§6.10); for any other transport it is out of scope for this revision
(`spec/security.md` §1.3).

[IFC-TRN-010] The core MUST NOT rely on a transport for the confidentiality, the integrity or
the authenticity of a payload.

[IFC-TRN-011] The core MUST stay conformant to `spec/session-channels.md` and
`spec/security.md` when a transport loses, delays, duplicates or reorders payloads.

Duplication and reordering are absorbed by rules the core already has: `seq` for presence
records ([SC-DLV-042]; fixture `ifc-trn/IFC-TRN-011.n01`), the duplicate store for envelopes
([SEC-RPL-021]; fixture `ifc-trn/IFC-TRN-011.n02`), the binding of a receipt to one sent
envelope ([SEC-RCT-003]), and the hand-off deadline for late copies ([SC-RCP-091]).

[IFC-TRN-012] The core MUST NOT treat a carrier handle, a transport peer identifier or a
transport-native address as a device, a session, a principal or an authorization input.

A transport's own peer identifier names a connection, not a device key
(`spec/session-channels.md` §7.2.4; [SEC-AUZ-004]).

[IFC-TRN-013] The core MUST NOT treat a transport peer that the transport's own discovery found
as a session, as a presence record or as evidence of either.

Session discovery is `spec/session-channels.md` §7.3, from accepted presence records.
Transport-level discovery finds transport peers only.

### 6.3 Capability declaration

`start` returns a `TransportCapabilities` declaration. It has one boolean for each of the six
optional capabilities that `docs/planning/DESIGN.md` names ("Transport contract"), and four
further members.

Table 6.3.

| Member | Type | Meaning when `true`, or value |
|---|---|---|
| `reliability` | boolean | the transport retransmits a payload in flight until the far end has it, its deadline passes, or it reports that it could not |
| `persistence` | boolean | the transport keeps payloads across a restart; always `false` in this revision ([IFC-TRN-026]) |
| `offline_queueing` | boolean | the transport holds a payload for a destination that is not reachable, and delivers it later; always `false` in this revision ([IFC-TRN-026]) |
| `ordering` | boolean | the transport delivers payloads from one source to one destination in the order passed |
| `multicast_discovery` | boolean | the transport finds transport peers without configured addresses |
| `routing_federation` | boolean | the transport relays payloads through intermediaries between networks |
| `reach` | one of `local-only`, `cross-implementation` | whether the transport reaches other implementations at all |
| `destination_restricted` | boolean | [IFC-TRN-080] |
| `max_payload_octets` | integer | the largest payload the transport carries |
| `sealing` | boolean | the transport takes and yields only payloads of kind `sealed` (§6.10) |

[IFC-TRN-020] A transport MUST declare, in the result of `start`, each of the six optional
capabilities of Table 6.3 as present or absent.

[IFC-TRN-021] A transport MUST NOT declare present an optional capability that it does not
provide.

[IFC-TRN-026] A transport MUST declare `persistence` and `offline_queueing` absent.

*Dated note, 2026-10-04 (#273): this applies the operator decision on #43 (2026-10-03):
refuse right away, OAC never holds messages, and no offline mailbox in v0.1. The receiver
already follows it under [SC-DLV-007]. The review of PR #279 applied the same decision to the
transport. Persistence and offline queueing are a durable offline mailbox,
which `docs/planning/ADR-001.md` ("v0.1 scope") defers. They stay in the declaration so that a
later revision can define them; in this revision they are refused, and [IFC-TRN-033]
to [IFC-TRN-036] state what a transport may hold.*

[IFC-TRN-022] The core MUST NOT depend on any of the optional capabilities being present.

The core works the same with all six absent. A declared capability can make delivery more
likely or more timely. It never satisfies, relaxes or replaces a rule of the companion
documents.

[IFC-TRN-023] A transport MUST declare a `max_payload_octets` of at least 65536.

[IFC-TRN-024] An implementation MUST NOT declare a `max_envelope_octets` for a session larger
than the `max_payload_octets` of the transport that carries envelopes to that session.

*Dated note, 2026-10-04 (#273): the 65536-octet floor is the default envelope limit
of [SC-ENV-004]. It, and the members of Table 6.3 beyond DESIGN's six, were chosen here and
accepted in the review of PR #279.*

*Dated note, 2026-10-09 (revision 0.3): `sealing` is added to Table 6.3. A declaration made
under an earlier revision has no `sealing` member, and the core reads it as `false`.*

### 6.4 Operations

Table 6.4. `Transport`.

| Operation | Caller | Inputs | Output |
|---|---|---|---|
| `start` | core | the local device's key id; a `TransportConfiguration` | `TransportCapabilities` |
| `publish` | core | a `Destination`; a `Payload` of kind `envelope`, `receipt` or `sealed` (§6.10); a `Deadline` | `PublishResult` |
| `subscribe` | core | a `Destination` naming a local session or the local device; a handler that takes `Inbound` values | a subscription that the core can end |
| `send_presence` | core | a `Destination` of kind `device`; a `Payload` of kind `presence`; a `Deadline` | `PublishResult` |
| `watch_presence` | core | a handler that takes presence events (§6.6) | none |
| `health` | core | none | `HealthStatus` |
| `shutdown` | core | none | none |

The key id passed to `start` is public. The transport uses it to receive payloads addressed to
the local device.

[IFC-TRN-025] The core MUST NOT pass to a transport the device key's private seed, or any other
secret of `spec/security.md`.

A transport's own credentials, such as those of an encrypted link, are its own configuration.
They are not the device key and authenticate nothing at the protocol layer ([SEC-SIG-030]).

**`publish` and `send_presence`.** The result says whether the transport took the payload.

[IFC-TRN-030] A transport MUST deliver a payload's octets, when it delivers them, exactly as the
core passed them.

A transport that altered a payload would cause `signature-invalid` refusals, not a forgery
(`spec/security.md` §6). This rule makes such a transport non-conformant as well.

[IFC-TRN-031] A transport MUST NOT return `not-taken` for a payload of which any copy can still
be delivered.

[IFC-TRN-032] The core MUST treat an envelope as passed to a transport when `publish` returns
`taken`, and as not passed when it returns `not-taken`.

The distinction drives the combined state of `spec/session-channels.md` §8.4.1: an envelope
that was not passed is reported `failed` with `transport-failure`, and may be retried at once;
one that was passed is reported `accepted-by-adapter` and may not be retried before its retry
deadline ([SC-RCP-007], [SC-RCP-086]). A transport that returned `not-taken` while a copy was
still on its way could therefore cause the content to be handed off twice. When a transport
cannot tell, it returns `taken`.

**What a transport may hold.** A transport may keep a copy of a payload while the copy is in
flight, for example to retransmit it. It keeps nothing else.

[IFC-TRN-033] A transport MUST NOT hold a copy of a payload except while that copy is in flight.

[IFC-TRN-034] A transport MUST NOT hold, or deliver, a copy of a payload at or after the
payload's `Deadline`, except that the receiving end of a sealing transport (§6.10), which holds
no `Deadline` ([IFC-TRN-107]), is bound only by [IFC-TRN-033] and [IFC-TRN-036].

The receiving end of a sealing transport hands a late copy over, and the receiving core drops
it silently on opening (`spec/security.md` [SEC-SEL-036]). The obligation moves from one part
of the receiving implementation to another, and only for an implementation that uses a
sealing transport (`spec/session-channels.md` §5.2 item 9).

[IFC-TRN-035] A transport MUST NOT keep a payload across a restart of the transport, or deliver
a payload again after the destination's implementation restarted.

A copy redelivered after a receiver restart would meet an empty duplicate store, and the
receiver would hand the content off a second time within the window: the residual
that [SEC-RPL-025] accepts for the receiver's own restart, caused here by the transport.

[IFC-TRN-036] A transport MUST NOT keep a payload for later delivery to a destination that is
not reachable when the payload is in flight.

[IFC-TRN-037] The core MUST pass with each payload the `Deadline` that Table 6.1 gives for its
kind.

**`subscribe`.**

[IFC-TRN-040] A transport MUST hand each inbound payload to the handler of a subscription whose
`Destination` the payload names, on the transport's own initiative.

[IFC-TRN-041] The core MUST NOT obtain inbound payloads by asking a transport, on a timer or in
a loop, whether payloads are waiting.

That would be polling (`spec/session-channels.md` §7.1.1), which [SC-DLV-002] forbids for a
session that declares active inbound. A transport's own keepalives are push mechanisms, not
polling (§7.1.1 there).

[IFC-TRN-042] The core MUST end its subscription for a session id when that session's binding
ends.

**Not revealing subscriptions.** A subscription for a session id says that the session exists
and is bound. Discovery reveals that only to requesters authorized to discover the session
([SC-DLV-061], [SC-DLV-066]), and a send request does not reveal it either
([SC-DLV-075], [SC-DLV-076]; [MCPB-TOOL-021] for one binding). The rules below keep a
transport from revealing it by other means: by announcing interest, by routing state, or by
answering a publisher differently.

[IFC-TRN-043] A transport MUST NOT make observable to any implementation other than the one that
made a subscription whether that subscription exists, through a declaration of interest,
through routing state, or through the result, timing or errors of another implementation's
`publish`.

[IFC-TRN-044] A transport MUST NOT let the `PublishResult` of a `publish` depend on whether the
`Destination` is subscribed.

What a transport may expose is the traffic it carries: that a payload of some size went from
one transport peer to another at some time. A transport binding states what that reveals and
how it limits it ([IFC-TRN-090]). Full confidentiality of traffic is out of scope for this
revision (`spec/security.md` §1.3). A sealing transport keeps the content, the kind and the
destination of a payload from every implementation but the recipient's (§6.10;
`spec/security.md` §14.8).

### 6.5 Carrying presence records and receipts

**Presence records.** The core issues an authenticated presence record to one device at a time
([SEC-PRS-011]) and passes it to `send_presence` with that device as the `Destination`.
Withdrawals travel the same way as announcements.

[IFC-TRN-050] A transport MUST carry each authenticated presence record, as one whole payload,
to the device that its `Destination` names.

[IFC-TRN-051] The core MUST pass to `send_presence` only an authenticated presence record whose
`audience` is the device that the `Destination` names.

Within one implementation no presence record is needed: an implementation knows its own
sessions' presence (`spec/session-channels.md` §7.2.1).

**Receipts.** A receiver that sends a receipt to another implementation ([SC-RCP-042])
passes the authenticated receipt to `publish`, with the `device` of the key id that verified
the envelope as the `Destination` (Table 6.1). The sending implementation receives it on its
device subscription and accepts it only under [SEC-RCT-003].

### 6.6 `watch_presence` and carrier loss

`watch_presence` yields two kinds of presence event, on the transport's own initiative:

| Event | Members |
|---|---|
| `record` | `payload` (an authenticated presence record) and `carrier` (a `CarrierHandle`) |
| `carrier-loss` | `carrier` (a `CarrierHandle`) |

[IFC-TRN-060] A transport MUST hand each presence record it receives, and each carrier loss it
reports, to the `watch_presence` handler on its own initiative.

[IFC-TRN-061] A transport MAY report a carrier loss when the link that a carrier handle names
ends. A transport that never reports one still conforms: announcements then go stale by their
lifetime ([SC-DLV-045]) and the cap of [SEC-PRS-007].

Under [SC-DLV-046], a consumer treats each accepted announcement from an issuer as stale when
the carrier reports carrier loss for that issuer. A carrier handle is how the transport contract
names that issuer's link.

[IFC-TRN-062] The core MUST take a carrier loss for a carrier handle as the carrier loss
of [SC-DLV-046] for each issuer whose accepted announcements arrived with that carrier handle.

A carrier loss only moves sessions toward `unreachable` (§7.2.4 there). A forged or mistaken
one can at worst deny service until the issuer's next announcement.

### 6.7 Crossing implementations

A presence record reveals that a session exists and what it accepts. [SC-DLV-066] requires
that only peers authorized to discover a session receive its records. The core decides who is
authorized and signs the audience ([SEC-AUZ-011], [SEC-PRS-013]). The transport has to keep the
record from everyone else.

[IFC-TRN-080] A transport that declares `destination_restricted` as `true` MUST deliver each
payload only to the implementation that holds the device key its `Destination` names, or
carry it so that no other implementation can read it.

[IFC-TRN-081] The core MUST NOT pass a presence record for another implementation to a
transport whose declaration does not have `reach` `cross-implementation` and either
`destination_restricted` `true` or `sealing` `true`.

Without presence records, nothing crosses: a sender holds a capability declaration only from
an accepted announcement ([SC-DLV-070]), so it sends no envelope to a session of another
implementation, and no receipt follows. [IFC-TRN-081] is therefore the gate for all
cross-implementation traffic.

A sealing transport meets the second half of [IFC-TRN-080] through the core: every payload it
carries is sealed to the device its `Destination` names, so no other implementation can read
it (`spec/security.md` §14). That covers payloads between two sessions of one implementation
too, which such a transport carries like any other ([IFC-TRN-002]): they are sealed to that
implementation's own device ([SEC-SEL-016]).

*Dated note, 2026-10-09 (the lead's ruling on PR #364, review finding 2): revision 0.2 let only
a `destination_restricted` transport carry presence records between implementations. The
review of PR #364 found that a transport whose every peer receives every payload cannot be
made `destination_restricted` by access control on subscriptions, and that a core publishes
payloads between its own sessions over the same transport, where any other implementation on
it could read them. The lead ruled that each payload be encrypted for its recipient. Revision
0.3 therefore adds `sealing` (§6.10) as a second way through [IFC-TRN-081], and [IFC-TRN-111]
asks every transport that reaches other implementations unrestricted to declare it.*

*Dated note, 2026-10-04 (#273): this restates, as a contract rule, the ruling recorded on #43
and kept by #266 (`spec/session-channels.md` §7.3.2, dated notes): presence, discovery and
sending stay within one implementation until a transport binding meets [SC-DLV-066].
`spec/security.md` §10 and §11 supply the authentication half. The v0.1 transport mapping,
Decision C7 (`docs/planning/decisions/`; §4, §7), carries reachability only, not presence
records, and its local mode has no transport-layer authorization. It therefore meets neither
the carriage rule of §6.5 nor [IFC-TRN-080] yet. How well the v0.1 transport would carry
presence records is UNVERIFIED (`spec/session-channels.md` §7.2.5 note;
`docs/planning/STATUS.md`, "Open UNVERIFIED items"; owners F6, F10 and the transport
binding).*

### 6.8 `health` and `shutdown`

`health` returns a `HealthStatus` ([IFC-TYP-092]).

[IFC-TRN-071] After its `shutdown` returns, a transport MUST NOT invoke a handler.

### 6.9 Transport binding documents

[IFC-TRN-090] A transport MUST have a transport binding document that states how it maps each
`Destination` to its own addressing, its `TransportCapabilities` declaration with the evidence
for each member, how it meets [IFC-TRN-043] and [IFC-TRN-044], how it meets [IFC-TRN-080] when
it declares `destination_restricted`, and how it detects carrier loss, if it reports any.

> **Reference implementation note:** the v0.1 transport's mapping is recorded in Decision C7
> (`docs/planning/decisions/`; §2 the containment boundary, §3 addressing, §4 presence). That
> decision predates this contract. It becomes the transport binding document once it states
> the members of Table 6.3, the carriage of §6.5 and the non-disclosure of §6.4 (tasks G1,
> G2).

### 6.10 Sealing transports

A **sealing transport** declares `sealing` `true` (Table 6.3). The core seals every payload
it passes to such a transport to the one device it is for, and opens every payload it takes
from it (`spec/security.md` §14). The transport carries frames whose content, kind, signer and
recipient it cannot read, and it shows the destination to no other implementation. That is
what lets a transport deliver every payload to every implementation it reaches, and leave each
receiver to discard what does not open, without one implementation reading another's
payloads.

**The core.**

[IFC-TRN-100] The core MUST pass to a sealing transport only payloads of kind `sealed`.

It passes them through `publish`, which takes that kind (Table 6.4). Presence records
therefore travel sealed through `publish`, like envelopes and receipts. A
core does not call `send_presence` on a sealing transport, and receives presence records in
sealed payloads on its device subscription. It still takes carrier losses from
`watch_presence` (§6.6), and maps them to issuers by the carrier handle that came with each
sealed payload ([IFC-TRN-062]).

[IFC-TRN-101] The core MUST pass each sealed payload with the `device` `Destination` of the
device it is sealed to.

Its `Deadline` is the one Table 6.1 gives for kind `sealed`, that of the payload inside
([IFC-TRN-037]).

[IFC-TRN-102] The core MUST NOT pass a payload of kind `sealed` to a transport that does not
declare `sealing` `true`.

[IFC-TRN-103] The core MUST discard an inbound payload from a sealing transport whose kind is
not `sealed`.

[IFC-TRN-104] The core MUST NOT declare a `max_envelope_octets` for a session larger than the
`max_payload_octets` of the sealing transport that carries envelopes to that session, less 54.

Fifty-four octets is the smallest overhead of a sealed frame (`spec/security.md` §14.4). Padding
never makes a frame longer than the transport carries ([SEC-SEL-025]).

**The transport.**

[IFC-TRN-105] A sealing transport MUST hand each inbound payload of kind `sealed` to the handler
of the local device's subscription.

For a sealed payload, the local device's subscription is the subscription that the payload
names under [IFC-TRN-040], whatever device it was sealed to. The core opens the frame and
discards it when it does not open ([SEC-SEL-030]).

[IFC-TRN-106] A sealing transport MUST NOT make the `Destination` of a sealed payload, or any
value derived from it, observable to any implementation other than the one that holds the
device key it names.

A transport that delivers every sealed payload to every implementation it reaches meets
[IFC-TRN-106] by addressing them all alike. A transport that routes each payload only to the
implementation that holds the named key meets it as well.

[IFC-TRN-107] A sealing transport MUST NOT carry, with a sealed payload, any value derived from
the payload's octets, its `Destination` or its `Deadline`, other than the octets themselves.

A deadline beside a frame would show its kind, since each kind's deadline is a different
offset from its sending time, and would pair a receipt with its envelope, whose deadline it
shares (Table 6.1). The transport therefore keeps the `Deadline` at the sending end only. It
holds a copy no later than that `Deadline`, and the receiving core discards a payload whose
deadline has passed after opening it (`spec/security.md` [SEC-SEL-036]). The transport can
still carry values of its own link, such as a carrier handle (§6.6), within [IFC-TRN-108].

[IFC-TRN-108] A sealing transport MUST NOT make the local device's key id, a session id, or any
value derived from either, observable to any other implementation, except inside the octets of
a sealed payload.

The key id passed to `start` is public (§6.4), but a link that carried it, or a value derived
from it, would name the device behind every frame sent over that link, and a session id would
name the session. The rule covers the transport's frame headers, its link and peer
identifiers, its carrier handles, and any signal of liveness or carrier loss (§6.6). A
liveness signal that names a session is not a sealed payload's `Destination`, so
[IFC-TRN-106] alone would not cover it.

[IFC-TRN-109] A sealing transport MUST declare a `max_payload_octets` of at least 65590.

That is the 65536 octets of [IFC-TRN-023] plus the 54 octets of a frame's overhead, so that an
envelope of the default limit of [SC-ENV-004] fits in one frame.

[IFC-TRN-110] A transport whose transport binding document states a `sealing` value MUST
declare that value.

A binding document that states none is read as stating `false`. Every implementation that uses
one binding of a transport therefore agrees on whether payloads on it are sealed: the
declaration is made once, for the whole binding. A transport whose binding states `sealing`
`true` carries sealed payloads between all of them.

[IFC-TRN-113] A sealing transport MUST return `not-taken` for a payload of any kind other than
`sealed`, from `publish` and from `send_presence`.

The core never passes one ([IFC-TRN-100]). This rule keeps a defect in the core from putting
an unsealed payload on the transport.

[IFC-TRN-111] A transport that declares `reach` `cross-implementation` and
`destination_restricted` `false` SHOULD declare `sealing` `true`. A transport that does not
deviates: every implementation it reaches can read every payload it carries, including
payloads between two sessions of one implementation ([IFC-TRN-002]).

[IFC-TRN-112] The transport binding document of a sealing transport MUST state what the
transport carries with each sealed payload, and what that reveals to the implementations it
reaches.

---

## 7. Neutrality and containment

`docs/planning/ADR-001.md` ("Boundary") keeps transport-specific concepts out of the neutral
protocol, and `docs/planning/DESIGN.md` ("OAC Session Channels specification") keeps transport
addressing and provider-specific method names out of the specification. The rules below
extend both to every contract of this document, so that a third adapter or a second transport
fits the same signatures.

[IFC-NEU-001] An implementation MUST NOT add to a type or an operation of this document a
member, a parameter or a value that is specific to one harness or one transport.

[IFC-NEU-002] An adapter MUST NOT pass to the core a harness-native method name, surface name,
identifier or type, except the `native_id` and `cross_check` of a `NativeSignal` and the
`cross_check` of an `attachment-opened` event.

[IFC-NEU-003] A transport MUST NOT pass to the core a transport-native address, peer
identifier or type, except as an opaque `CarrierHandle`.

[IFC-NEU-004] The core MUST NOT read the inside of a `CarrierHandle` or a
`TransportConfiguration`.

Table 7.1 lists every value that crosses a contract and is not fully neutral, with the reason
it is allowed. There are no others.

Table 7.1.

| Value | Crosses | Why it is allowed | What keeps it contained |
|---|---|---|---|
| `NativeSignal.native_id`, `NativeSignal.cross_check`, `attachment-opened.cross_check` | adapter to core | Binding a session (§6.7 of `spec/session-channels.md`) compares them, and nothing else can | They are compared as strings and never interpreted; they go into the registration record only, which is never published ([SEC-KEY-042]); never into an envelope or descriptor ([SC-ID-006], [SC-ID-144]) |
| `Connection` (and so `Attachment`) | core to adapter, and back | The core attributes requests by attachment ([SC-ID-160]) | Created by the core process only ([IFC-ADP-012]); the adapter only stores and returns it ([IFC-ADP-013]) |
| `CarrierHandle` | transport to core | Carrier loss is per link ([SC-DLV-046]) | Opaque handle; never an identity ([IFC-TRN-012]) |
| `TransportConfiguration` | core to transport | A transport needs its own settings | Opaque; the core passes it to `start` unread |
| `HealthStatus.detail` | either part to core | Diagnostics for an operator | Free text for people; no code path reads it; [IFC-TYP-092] limits its content |

> **Reference implementation note:** the CI boundary lint (`.github/workflows/boundary-lint.yml`)
> scans every file under `spec/`, this one included, for transport and harness vocabulary,
> and checks 1 and 2 of `oac-boundaries` will scan the core module once it exists. The rules
> above are what those scans approximate.

Since Gate S2 froze this document with the other Stage 2 documents (task E7, #47; signed
off 2026-10-06, in force from the merge of PR #276), a change to a type or an operation
here is made only by a pull request that contains the change and a version bump, and that
the lead approves
(`docs/planning/decisions/E7-interface-freeze.md` §7; `oac-spec-authoring` §7). No separate
amendment record is needed: the versioned change records itself. A change that touches a
wire form takes the version `spec/session-channels.md` §5.2-§5.3 assign to it. A change that
touches no wire form, such as renaming an operation or moving a requirement to another owner
in Appendix C, takes a minor version under the same extension identifier.

### 7.1 Revision 0.4 classification and implementation work

Both changes in 0.4 touch only the local adapter contract:

| Change | Classification against `spec/session-channels.md` §5.2 / §5.3 |
|---|---|
| Connection closure barrier and bounded adapter shutdown (#376) | Minor interface revision under E7 §7 item 2's no-wire-form rule. This is not a §5.2 item 9 optional wire capability: closure is required of every updated local implementation. No peer envelope, delivery state, error code, signature scope or verification procedure changes (§5.3 items 1-10); §5.3 item 11 would make a new obligation on an older **wire** implementation breaking, but E7 explicitly assigns local contract changes a minor bump. |
| Identity request/result through the core sink (#66) | Minor interface revision under the same E7 rule. The new local request kind requires sink implementations to change; it is not a negotiated peer capability under §5.2 item 9. The harness-facing session id and fingerprint already exist under [MCPB-TOOL-006], so no binding wire form changes. No §5.3 wire-breaking case is introduced. |

**These are breaking changes to the local implementation API/contract**: an old plain
blocking stream or a sink supporting only send/discovery does not implement revision 0.4.
They are **not breaking protocol changes** and need no new major version or extension
identifier. An earlier peer still observes the existing protocol outcomes. Revision 0.4
does not assert that existing code already meets the new local obligations.

Implementation work after lead approval (outside this spec-only change):

- Core connection constructors and all stream backends: independently callable closure,
  shared closure state, terminal write results and bounded read/write settlement (#376,
  G9 #70); extend the F10 (#59) fakes and common adapter contract suite first.
- Adapter shutdown paths: stop new work, close connections before waiting for blocked
  workers, settle in-flight hand-offs and report attachment closure within the declared
  bound; update each adapter binding's bound. G4 #65 owns the stalled-write regression
  and removal of its residual risk only once this behaviour is proven.
- Core request sink and all sink implementations: attachment-attributed identity requests,
  atomic binding/fingerprint snapshots, default-deny refusals and public-digest-only results.
  G5 #66 and G8 #69 map the result to the existing identity tool, using existing confirmation
  rules; F10 #59 and F11 #60 prove the cases in Appendix A.

Issue #375 is deferred in full to a separate frozen-spec PR: binding an agreement statement
or its sequence into pairing changes the pairing commitment/verification procedure
(`spec/session-channels.md` §5.3 item 6), requiring its own compatibility classification.
That PR also owns the related §5.2 item 9 definition of equal obligation, maximum-sequence
re-pairing wording/integer spelling, and the security §13 evidence rows. Nothing in 0.4
changes pairing or claims those risks are resolved.

---

## 8. Security considerations

This section adds no requirement. It lists the threats that sit on the boundaries this
document draws, and the rules that answer them. The threats of the protocol itself are in
`spec/security.md` §13.

| Threat | Answered by |
|---|---|
| An adapter, a harness or a model reaches a transport directly, skipping signing, authorization or presence checks | [IFC-ADP-001], [IFC-ADP-002], [IFC-ADP-003], [IFC-ADP-007] |
| A harness-side process names another session, or forges or picks another session's attachment, to send as it | [IFC-ADP-012], [IFC-ADP-013], [IFC-ADP-031], [IFC-TYP-090]; [SC-ID-160], [SC-ID-162], [SEC-AUZ-030] |
| An adapter overstates a hand-off ("the model saw it") or hides an indeterminate one | [IFC-ADP-051] to [IFC-ADP-055]; [SC-RCP-005] |
| A turned-away holding hand-off is retried through a steering operation | [IFC-ADP-057]; [SEC-AUZ-022], [SEC-AUZ-027] |
| A transport identity, address or discovered peer is taken as a device or a session | [IFC-TRN-012], [IFC-TRN-013], [IFC-NEU-003] |
| A transport reports a failure for a payload it may still deliver, inviting a double hand-off | [IFC-TRN-031], [IFC-TRN-032] |
| A transport stores a payload and delivers it late, or again after the receiver restarts, causing a second hand-off | [IFC-TRN-026], [IFC-TRN-033] to [IFC-TRN-037]; [SC-RCP-091] |
| A transport reveals which session ids are subscribed, outside the authorization path | [IFC-TRN-043], [IFC-TRN-044] |
| A presence record reaches a peer that may not discover the session | [IFC-TRN-080], [IFC-TRN-081]; [SEC-AUZ-011], [SEC-PRS-013] |
| A received presence state is trusted as if it were a record | [IFC-TYP-061] |
| A type carries the working directory or a native id, and a serializer leaks it | [IFC-TYP-020], [IFC-TYP-002]; [SC-ID-045] |
| An unverified envelope's principal is treated as verified | [IFC-TYP-041], [IFC-TYP-042] |
| A delivery grant is reused to approve a permission request | [IFC-TYP-082]; [SEC-AUZ-021] |
| Health output exposes secrets or addresses | [IFC-TYP-092] |
| A transport is handed the device key | [IFC-TRN-025]; [SEC-KEY-004] |
| An implementation on a shared transport reads another implementation's payloads, or learns their kind, destination or deadline | [IFC-TRN-100] to [IFC-TRN-107], [IFC-TRN-111], [IFC-TRN-113]; `spec/security.md` §14 |
| A sealing transport names the device behind a link, in a frame header, a carrier handle or a liveness signal | [IFC-TRN-108] |
| A transport is handed an agreement private key | [IFC-TRN-025]; [SEC-SEL-004] |
| A payload goes onto a sealing transport unsealed when no agreement key is held | [IFC-TRN-100]; [SEC-SEL-024] |

---

## 9. What changed from the M0 draft

Informative. The M0 draft, `docs/planning/v0.1/05-interfaces.md` §13-§15, was written before
the Stage 2 specifications and contradicts them. This table records each change and its
reason. It also maps the conceptual operations of `docs/planning/DESIGN.md` to the operations
of this document, which E7 (#47) asks to document. A conceptual name appears only here when
the operation's meaning changed.

| M0 draft or DESIGN sketch | This document | Reason |
|---|---|---|
| `SessionIdentity{session_id, device_key_fingerprint}` | the session id alone (§4.2) | The id is opaque ([SC-ID-004], [SC-ID-005]); the key binding is the registration record and the binding table (`spec/security.md` §5.4, §11.3) |
| `SessionDescriptor{identity, display_uri, working_directory, registered_at}` | the §6.3 descriptor: `session_id`, `capabilities`, `display_name?`, `harness_label?` (§4.3) | [SC-ID-045] bans the working directory; the display form is never on the wire ([SC-ID-020], [SC-ID-021]); capabilities travel in the descriptor ([SC-ID-041]) |
| `SessionCapabilities{active_inbound, spec_revision, extension_id, negotiated_capabilities}` | the §6.4 declaration keyed by extension identifier (§4.4) | One entry per implemented major version; a flat record negotiates nothing (fixture `ifc-typ/IFC-TYP-030.n01`) |
| `ChannelMessage` with `principal` replacing `security` | the envelope as signed or received, unrecognized members included, plus a local `verified_by` (§4.5) | The signature covers the envelope as received ([SEC-SIG-011]) |
| `DeliveryReceipt{state, envelope_id, error?}` | the §8.1.4 receipt, with `envelope_from`, `observer`, `observed_at` (§4.6) | An id is unique only per sending session ([SC-ENV-027]); every state has an observer (§8.1.1 there) |
| `PresenceRecord{session_id, state, observed_at}` | the §7.2.2 record; the observer's conclusion is the local `PresenceState` (§4.7) | The record is what an issuer signs and sends ([SEC-PRS-001]); the state is a consumer's own judgement |
| `SecurityPrincipal{principal_ref, device_key_fingerprint}` | `principal` and `key_id` of a trusted-key-set entry (§4.8) | `spec/security.md` §5.2-§5.3; the key id is the fingerprint |
| Authorization decision `{outcome, reason}` | kinds, closed input lists, `permit` with a basis (§4.9) | One-way grants, scope and device grants, reply rights and default deny (`spec/security.md` §9) |
| `discover_sessions() -> [SessionDescriptor]` | `take_connection(Connection)` and `watch_attachments()`, an event stream of attachments and native signals (§5.4) | Renamed: it no longer discovers sessions. A session has no id or descriptor until the core binds it (`spec/session-channels.md` §6.7), and attachments are connections the core process authenticated (§5.2) |
| `attach(session) -> Result` | `set_binding(attachment, session or none)` (§5.4) | Renamed: the adapter does not attach. The core binds and tells the adapter ([IFC-ADP-007]) |
| `capabilities(session) -> SessionCapabilities` | `capabilities(attachment) -> AdapterCapabilities`; the core adds revision and identifier (§5.5) | Same meaning, the adapter's capability report; the core implements the revisions |
| `deliver(session, message) -> DeliveryReceipt` | `deliver(HandOff) -> HandOffOutcome`, mapped by the core (§5.5) | Same meaning, one hand-off; the adapter reports what it observed, and the core owns states and receipts ([SC-RCP-003]). It uses the holding hand-off and never steers ([SEC-AUZ-022], [SEC-AUZ-025]) |
| `publish_output(callback)` | `accept_requests(request sink)`, returning a `RequestResult` and later receipts (§5.6) | Renamed. The conceptual name reads as the adapter publishing output, which [IFC-ADP-001] forbids; the operation registers the core's sink for what the harness requests, which is refused or turned into an envelope by the core |
| `Transport.start(identity, config)` | `start(device key id, configuration) -> TransportCapabilities` (§6.4) | Same meaning; the identity is the device's key id, and the capability declaration is returned, not assumed |
| `publish(destination, envelope)` | `publish(Destination, Payload, Deadline)` for envelopes and receipts, returning `taken` or `not-taken` (§6.4) | Same meaning, widened to receipts (`spec/session-channels.md` §8.1.5); the deadline bounds what a transport may hold ([IFC-TRN-034]) |
| `subscribe(address, handler)` | `subscribe(Destination, handler)` returning an endable subscription (§6.4) | Same meaning; bindings end ([IFC-TRN-042]), and a subscription is not revealed ([IFC-TRN-043]) |
| `announce_presence(record)` | `send_presence(device, authenticated presence record, Deadline)` (§6.5) | Renamed: it is not a broadcast announcement but a directed send of one record, announcement or withdrawal, to one audience ([SEC-PRS-011]); reachability alone is not a record (C7 §4 gap) |
| `watch_presence(handler)` | records and carrier losses, each with a carrier handle (§6.6) | Same meaning; carrier loss is per link ([SC-DLV-046]) |
| "Optional capabilities: reliability, persistence, offline queueing, ordering, multicast discovery, routing/federation" | declared booleans, plus `reach`, `destination_restricted`, `max_payload_octets` (§6.3) | Declared, not assumed (E7 acceptance); cross-implementation gating ([IFC-TRN-081]) |
| "persistence", "offline queueing" | declared, and always absent in this revision ([IFC-TRN-026]) | v0.1 refuses them: OAC never holds messages (operator decision on #43, applied in the review of PR #279; `docs/planning/ADR-001.md` "v0.1 scope" defers offline mailboxes) |
| A "binding/mapping annex" naming the v0.1 harnesses and transport | adapter and transport binding documents ([IFC-ADP-080], [IFC-TRN-090]) | Neutral text names no harness or transport (§7) |

---

## 10. References

### 10.1 Normative references

- [RFC2119] Bradner, S., "Key words for use in RFCs to Indicate Requirement Levels",
  BCP 14, RFC 2119. https://www.rfc-editor.org/rfc/rfc2119
- [RFC8174] Leiba, B., "Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words",
  BCP 14, RFC 8174. https://www.rfc-editor.org/rfc/rfc8174
- `spec/session-channels.md`, the OAC Session Channels specification.
- `spec/security.md`, OAC Session Channels security, including payload sealing (§14 there).
- `spec/bindings/mcp.md`, the MCP binding, for Appendix C.

### 10.2 Informative references

- `docs/planning/ADR-001.md`, "Boundary" and "v0.1 scope".
- `docs/planning/DESIGN.md`, "Core", "Provider adapter contract" and "Transport contract".
- `docs/planning/v0.1/05-interfaces.md` §13-§15, the M0 draft this document supersedes.
- `docs/planning/decisions/C2-process-model.md` §1, §4 (per-device process, local IPC).
- `docs/planning/decisions/C4-session-identity.md` §3-§4 (native signals, attachments,
  pairing observed by the per-device process).
- Decision C7, the v0.1 transport mapping (`docs/planning/decisions/`; §2, §3, §4, §7).
- `docs/planning/backlog/04-tasks-EF.json`, tasks E7, F7 and F10.

---

## Appendix A. Requirement index

Fixture paths are relative to `tests/protocol/`. `TODO(fixture)` marks a requirement with no
conformance fixture yet, and names the task expected to supply the test. `covered by` names
the requirement whose fixtures exercise it. Appendix C gives each requirement's owner.

| Id | Level | Section | Fixtures |
|---|---|---|---|
| IFC-TYP-001 | MUST | 4.1 | covered by IFC-TYP-030, IFC-TYP-050 and IFC-TYP-060 (`ifc-typ/IFC-TYP-030.n01`, `ifc-typ/IFC-TYP-050.n01`, `ifc-typ/IFC-TYP-060.n02`); for every type: TODO(fixture), F2 serialization tests |
| IFC-TYP-002 | MUST NOT | 4.1 | TODO(fixture): wire output of every part; F2, F10 |
| IFC-TYP-003 | MUST | 4.1 | covered by SEC-SIG-011 (`sec-sig/SEC-SIG-011.p01`, unknown members verified as received); the type itself: TODO(fixture), F2 |
| IFC-TYP-010 | MUST | 4.2 | TODO(fixture): by construction of the type; F2 |
| IFC-TYP-020 | MUST NOT | 4.3 | TODO(fixture): by construction of the type; F2 |
| IFC-TYP-030 | MUST | 4.4 | `ifc-typ/IFC-TYP-030.p01`, `.n01` |
| IFC-TYP-040 | MUST | 4.5 | TODO(fixture): an envelope passed through the core unchanged, unknown members included; F2, F4 |
| IFC-TYP-041 | MUST NOT | 4.5 | covered by SEC-STG-003 (`sec-stg/SEC-STG-002.n02`); the type itself: TODO(fixture), F2, F4 |
| IFC-TYP-042 | MUST | 4.5 | TODO(fixture): F4 verification tests |
| IFC-TYP-050 | MUST | 4.6 | `ifc-typ/IFC-TYP-050.p01`, `.n01` to `.n04` |
| IFC-TYP-051 | MUST | 4.6 | `ifc-typ/IFC-TYP-051.n01` |
| IFC-TYP-060 | MUST | 4.7 | `ifc-typ/IFC-TYP-060.p01`, `.n01` to `.n03` |
| IFC-TYP-061 | MUST NOT | 4.7 | TODO(fixture): transport payloads carry no state; F10 transport suite |
| IFC-TYP-070 | MUST | 4.8 | covered by SEC-KEY-030 (`sec-key/SEC-KEY-030.n01`, `.n02`) |
| IFC-TYP-080 | MUST | 4.9 | TODO(fixture): by construction of the authorization engine; F5, F11 |
| IFC-TYP-081 | MUST | 4.9 | covered by SEC-AUZ-001 (`sec-auz/SEC-AUZ-001.n01`) and SEC-AUZ-014 (`sec-auz/SEC-AUZ-014.p01`); the basis member: TODO(fixture), F5 |
| IFC-TYP-082 | MUST NOT | 4.9 | TODO(fixture): F5, F11 |
| IFC-TYP-090 | MUST NOT | 4.10 | by construction: TODO(fixture), F2, F10; a core ignoring an asserted id: `sc-id/SC-ID-162.p01` |
| IFC-TYP-091 | MUST | 4.10 | TODO(fixture): F10, F11 |
| IFC-TYP-092 | MUST NOT | 4.10 | TODO(fixture): F10 adapter and transport suites |
| IFC-TYP-095 | MUST | 4.11 | TODO(fixture): F7, F10 |
| IFC-ADP-001 | MUST NOT | 5.1 | TODO(fixture): F10 adapter suite (routes through core, by its acceptance); F1 module dependency direction |
| IFC-ADP-002 | MUST NOT | 5.1 | TODO(fixture): F10 adapter suite |
| IFC-ADP-003 | MUST | 5.1 | TODO(fixture): F10 adapter suite |
| IFC-ADP-004 | MUST NOT | 5.1 | TODO(fixture): F10 adapter suite |
| IFC-ADP-005 | MUST NOT | 5.1 | TODO(fixture): review and boundary lint check 3; H2, G4-G8 |
| IFC-ADP-006 | MUST NOT | 5.1 | TODO(fixture): review and boundary lint check 3; H2, G4-G8 |
| IFC-ADP-007 | MUST NOT | 5.1 | TODO(fixture): F10 adapter suite |
| IFC-ADP-010 | MUST | 3.3 | TODO(fixture): each assigned requirement's own fixtures and tests, run against an adapter; F10, F11 |
| IFC-ADP-011 | MUST | 3.3 | TODO(fixture): each assigned requirement's own fixtures, run against the core; F12 |
| IFC-ADP-012 | MUST | 5.2 | TODO(fixture): needs the platform facilities; G9, F11 |
| IFC-ADP-013 | MUST NOT | 5.2 | TODO(fixture): F10 adapter suite; F11 |
| IFC-ADP-014 | MUST | 5.2 | TODO(fixture): owner F10 (#59) with G9 (#70), every supplied connection exposes independent close and a finite positive bound |
| IFC-ADP-015 | MUST | 5.2 | TODO(fixture): owner F10 (#59) with G9 (#70), block the peer's reads and writes; close returns by the declared bound on a scripted clock |
| IFC-ADP-016 | MUST | 5.2 | TODO(fixture): owner F10 (#59) with G9 (#70), race close with zero/partial/full writes and blocked reads; all pending operations terminate before close returns, writes report exact transferred counts |
| IFC-ADP-017 | MUST NOT | 5.2 | TODO(fixture): owner F10 (#59) with G9 (#70), release a stalled peer after close returns; its transferred-octet count never increases |
| IFC-ADP-018 | MUST | 5.2 | TODO(fixture): owner F10 (#59) with G9 (#70), new read/write begun during and after close is refused without transfer |
| IFC-ADP-019 | MUST | 5.2 | TODO(fixture): owner F10 (#59) with G9 (#70), repeated/concurrent close through copies and split halves preserves one terminal state and leaves another connection open |
| IFC-ADP-020 | MUST | 5.4 | TODO(fixture): F10 adapter suite against the fakes (F8, F9); G4-G8 |
| IFC-ADP-022 | MUST | 5.4 | TODO(fixture): F10 adapter suite |
| IFC-ADP-030 | MUST NOT | 5.4 | TODO(fixture): F10 adapter suite |
| IFC-ADP-031 | MUST | 5.2 | TODO(fixture): F10 adapter suite, F11 spoofing cases; H2 |
| IFC-ADP-040 | MUST NOT | 5.5 | TODO(fixture): F10 adapter suite asserts the no-polling rule; H1 |
| IFC-ADP-041 | MUST NOT | 5.5 | TODO(fixture): F10 adapter suite |
| IFC-ADP-042 | MUST | 5.5 | TODO(fixture): F6, F10 |
| IFC-ADP-043 | MUST | 5.4 | TODO(fixture): F10 adapter suite |
| IFC-ADP-050 | MUST | 5.5 | TODO(fixture): F10 adapter suite |
| IFC-ADP-051 | MUST | 5.5 | TODO(fixture): F10 adapter suite against the fakes; G4, G7 |
| IFC-ADP-052 | MUST | 5.5 | TODO(fixture): F10 adapter suite against the fakes; G4, G7 |
| IFC-ADP-053 | MUST | 5.5 | TODO(fixture): F10 adapter suite against the fakes; G4, G7 |
| IFC-ADP-054 | MUST | 5.5 | TODO(fixture): F10 adapter suite, G4, G7, F11 |
| IFC-ADP-055 | MUST | 5.5 | TODO(fixture): F6 receipt state machine, F10 |
| IFC-ADP-056 | MUST NOT | 5.5 | TODO(fixture): F10 adapter suite |
| IFC-ADP-057 | MUST | 5.5 | TODO(fixture): F10 adapter suite against the F9 fake; G7 (#68) |
| IFC-ADP-060 | MUST | 5.6 | TODO(fixture): F10 adapter suite |
| IFC-ADP-062 | MAY | 5.6 | none (MAY) |
| IFC-ADP-070 | MUST NOT | 5.7 | TODO(fixture): owner F10 (#59) with G4 (#65), stalled write races shutdown; releasing the peer after shutdown returns causes no late hand-off or request passed to the core |
| IFC-ADP-071 | MUST | 5.7 | TODO(fixture): F10 adapter suite |
| IFC-ADP-072 | MUST | 5.7 | TODO(fixture): owner F10 (#59) with G4 (#65), race shutdown with new delivery and harness requests; no new hand-off begins |
| IFC-ADP-073 | MUST | 5.7 | TODO(fixture): owner F10 (#59) with G4 (#65), shutdown closes every supplied connection, including native-signal-only connections |
| IFC-ADP-074 | MUST | 5.7 | TODO(fixture): owner F10 (#59) with G4 (#65), permanently stalled peer and multiple connections; shutdown returns within the binding's declared bound |
| IFC-ADP-075 | MUST | 5.7 | TODO(fixture): owner F10 (#59) with G4 (#65), blocked delivery races shutdown; one truthful outcome precedes shutdown return; releasing peer afterwards cannot complete a late hand-off |
| IFC-ADP-076 | MUST | 5.7 | TODO(fixture): owner G4 (#65) with G7 (#68), binding document review checks a finite positive bound covering all shutdown work |
| IFC-ADP-077 | MUST | 5.7 | TODO(fixture): owner F10 (#59) with G9 (#70), a connection whose close bound exceeds the adapter's shutdown bound is never supplied |
| IFC-ADP-078 | MUST | 5.7 | TODO(fixture): owner F10 (#59) with G4 (#65), race shutdown with new harness requests; no new request is accepted |
| IFC-ADP-080 | MUST | 5.8 | TODO(fixture): document review at each adapter's task; G4-G8 |
| IFC-ADP-090 | MUST | 5.6 | TODO(fixture): randomness is not decided by a data fixture; G8 (#69) review of the core's source of pairing values. The value's form is fixtured under `spec/bindings/mcp.md` [MCPB-ATT-005] |
| IFC-ADP-091 | MUST | 5.4 | TODO(fixture): F10 adapter suite with G8 (#69): a request that fails confirmation → the event is reported before the request reaches the sink |
| IFC-ADP-092 | MUST | 5.4 | TODO(fixture): F10 with G8 (#69): after the event, requests from the attachment are refused with `unauthorized` and nothing is handed off to it, until a paired native signal |
| IFC-ADP-093 | MUST | 5.4 | TODO(fixture): F10 with G8 (#69): the event → one finding |
| IFC-ADP-094 | MUST | 5.6 | TODO(fixture): owner F10 (#59) with G5 (#66) and G8 (#69), identity tool invokes sink with the actual attachment; injected identity claims are not used |
| IFC-ADP-095 | MUST | 5.6 | TODO(fixture): owner F10 (#59) with F11 (#60), unbound/closed/unconfirmed attachments get unauthorized and no identity; rebinding never returns the previous session |
| IFC-ADP-096 | MUST | 5.6 | TODO(fixture): owner F10 (#59) with G5 (#66), identity request races rebind/revoke; successful session and local signing fingerprint belong to one current binding snapshot |
| IFC-ADP-097 | MUST | 5.6 | TODO(fixture): owner F10 (#59) with G5 (#66), successful result has exactly session_id and the public fingerprint in security §5.2's form |
| IFC-ADP-098 | MUST NOT | 5.6 | TODO(fixture): owner F11 (#60) with G5 (#66), synthetic signing/agreement key sentinels never appear in identity requests/results; only the digest is exposed |
| IFC-TRN-001 | MUST | 6.1 | TODO(fixture): F10 transport suite against F7, then G1-G2 |
| IFC-TRN-002 | MAY | 6.1 | none (MAY) |
| IFC-TRN-003 | MUST | 3.3 | TODO(fixture): each assigned requirement's own tests, run against a transport; F10 |
| IFC-TRN-010 | MUST NOT | 6.2 | covered by SEC-SIG-030 (TODO(fixture) there); F11, H2 |
| IFC-TRN-011 | MUST | 6.2 | `ifc-trn/IFC-TRN-011.n01`, `.n02`; loss and delay: TODO(fixture), F10 with F7's fault injection |
| IFC-TRN-012 | MUST NOT | 6.2 | TODO(fixture): F11, H2 |
| IFC-TRN-013 | MUST NOT | 6.2 | tested, no fixture (a behaviour, not a wire form): two transport peers that found each other hand the core no presence event and no payload until one is sent, the reference transport's test `discovery_alone_hands_the_core_nothing` (G1, #62) |
| IFC-TRN-020 | MUST | 6.3 | TODO(fixture): F7, F10 transport suite |
| IFC-TRN-021 | MUST NOT | 6.3 | TODO(fixture): F10 transport suite, against each declared capability |
| IFC-TRN-022 | MUST NOT | 6.3 | TODO(fixture): F10, the core run against F7 with every capability absent |
| IFC-TRN-023 | MUST | 6.3 | TODO(fixture): F10 transport suite |
| IFC-TRN-024 | MUST NOT | 6.3 | TODO(fixture): F6, F10 |
| IFC-TRN-025 | MUST NOT | 6.4 | TODO(fixture): F11, H2 |
| IFC-TRN-026 | MUST | 6.3 | TODO(fixture): F7, F10 transport suite |
| IFC-TRN-030 | MUST | 6.4 | TODO(fixture): F10 transport suite |
| IFC-TRN-031 | MUST NOT | 6.4 | TODO(fixture): F10 transport suite with F7's fault injection |
| IFC-TRN-032 | MUST | 6.4 | covered by SC-RCP-085 for the state it drives (`sc-rcp/SC-RCP-085.p06`); the mapping: TODO(fixture), F6, F10 |
| IFC-TRN-033 | MUST NOT | 6.4 | TODO(fixture): F7, F10 transport suite |
| IFC-TRN-034 | MUST NOT | 6.4 | TODO(fixture): F7, F10 transport suite with a scripted clock |
| IFC-TRN-035 | MUST NOT | 6.4 | TODO(fixture): F10 transport suite, a restart of each side; H3 |
| IFC-TRN-036 | MUST NOT | 6.4 | TODO(fixture): F7, F10 transport suite |
| IFC-TRN-037 | MUST | 6.4 | TODO(fixture): F6, F10 |
| IFC-TRN-040 | MUST | 6.4 | TODO(fixture): F10 transport suite |
| IFC-TRN-041 | MUST NOT | 6.4 | TODO(fixture): F10 transport suite asserts no polling; H1 |
| IFC-TRN-042 | MUST | 6.4 | TODO(fixture): F6, F10 |
| IFC-TRN-043 | MUST NOT | 6.4 | tested, no fixture (a behaviour, not a wire form): F10 transport suite check `subscriptions-not-revealed`, run unchanged against the reference transport (G1, #62); the differential test over a subscribed and an unsubscribed destination, as seen by a third party holding both session ids, the reference transport's test `native_interest_does_not_depend_on_subscriptions`; H2. Once the reference transport declares `sealing` (§6.10), this evidence is stale: every frame then goes to the device subscription, so the reference transport's test must be re-run, or replaced, against sealed frames (G1, #62) |
| IFC-TRN-044 | MUST NOT | 6.4 | tested, no fixture (a behaviour, not a wire form): F10 transport suite check `result-independent-of-subscription`, run unchanged against the in-memory transport (F7) and the reference transport (G1, #62) |
| IFC-TRN-050 | MUST | 6.5 | tested, no fixture (a behaviour, not a wire form): F10 transport suite check `presence-whole-to-named-device`, run unchanged against the in-memory transport (F7) and the reference transport (G1, #62); the reference transport's test `frames_reach_only_their_own_local_consumer`. Once the reference transport declares `sealing` (§6.10), this evidence is stale: presence records then travel sealed through `publish` to the device subscription, so `presence-whole-to-named-device` and `frames_reach_only_their_own_local_consumer` must be replaced by a check on sealed frames (G1, #62) |
| IFC-TRN-051 | MUST | 6.5 | TODO(fixture): F6, F10 |
| IFC-TRN-060 | MUST | 6.6 | TODO(fixture): F10 transport suite |
| IFC-TRN-061 | MAY | 6.6 | none (MAY) |
| IFC-TRN-062 | MUST | 6.6 | covered by SC-DLV-046 (`sc-dlv/SC-DLV-046.n01`: loss for one carrier leaves another's session online); the mapping from handle to issuer: TODO(fixture), F6 |
| IFC-TRN-071 | MUST NOT | 6.8 | TODO(fixture): F10 transport suite |
| IFC-TRN-080 | MUST | 6.7 | TODO(fixture): F10 transport suite; G3 |
| IFC-TRN-081 | MUST NOT | 6.7 | TODO(fixture): F6, F10, H2 |
| IFC-TRN-090 | MUST | 6.9 | TODO(fixture): document review; G1, G2 |
| IFC-TRN-100 | MUST | 6.10 | tested, no fixture (a behaviour, not a wire form; the core side, #369): every payload the core passes to a sealing test transport is of kind `sealed`, and `send_presence` is never called, `core/tests/sealing.rs` (each test's `all_sealed`), F11 `s13_only_the_recipient_device_opens_a_frame`, `s13_nothing_crosses_unsealed_without_a_statement`; against the reference transport: TODO(fixture), F10 with G1 (#62) |
| IFC-TRN-101 | MUST | 6.10 | tested, no fixture (the core side, #369): each sealed payload goes with the `device` destination it is sealed to, `core/tests/sealing.rs` `only_the_recipient_opens_and_a_third_device_stays_silent`, `own_sessions_are_sealed_to_the_own_statement`; against the reference transport: TODO(fixture), F10 with G1 (#62) |
| IFC-TRN-102 | MUST NOT | 6.10 | tested, no fixture (the core side, #369): a transport that does not declare `sealing` is passed no `sealed` payload, `core/tests/pipeline.rs` `two_devices_exchange_a_message_and_a_correlated_reply`; F10 with G1 (#62) |
| IFC-TRN-103 | MUST | 6.10 | tested, no fixture (#369): payloads of the other kinds, from the device subscription and from `watch_presence`, are not taken, `core/tests/sealing.rs` `unsealed_payloads_from_a_sealing_transport_are_discarded`, F11 `s13_an_unsealed_payload_from_a_sealing_transport_is_not_taken` |
| IFC-TRN-104 | MUST NOT | 6.10 | tested, no fixture (#369): a larger adapter limit is declared as `max_payload_octets` less 54, `core/tests/sealing.rs` `the_declared_envelope_limit_leaves_room_for_the_frame` |
| IFC-TRN-105 | MUST | 6.10 | TODO(fixture): F10 transport suite; G1 (#62) |
| IFC-TRN-106 | MUST NOT | 6.10 | TODO(fixture): F10 transport suite, a third implementation holding both destinations sees the same addressing for each; G1 (#62), H2 |
| IFC-TRN-107 | MUST NOT | 6.10 | TODO(fixture): document review of the binding's framing and an F10 check; G1 (#62) |
| IFC-TRN-108 | MUST NOT | 6.10 | TODO(fixture): F10 transport suite, a third implementation's view of frame headers, link and carrier identifiers and liveness signals holds no value derived from a device key id or a session id; G1 (#62) for the frame header, G2 (#63) for liveness and carrier loss |
| IFC-TRN-109 | MUST | 6.10 | TODO(fixture): F10 transport suite; G1 (#62) |
| IFC-TRN-110 | MUST | 6.10 | TODO(fixture): document review; G1 (#62), G3 (#64) |
| IFC-TRN-111 | SHOULD | 6.10 | none (SHOULD) |
| IFC-TRN-112 | MUST | 6.10 | TODO(fixture): document review; G1 (#62) |
| IFC-TRN-113 | MUST | 6.10 | TODO(fixture): F10 transport suite, each non-sealed kind passed to a sealing transport is `not-taken`; G1 (#62) |
| IFC-NEU-001 | MUST NOT | 7 | TODO(fixture): F1 workspace boundaries, boundary lint checks 1-2 over `core/` |
| IFC-NEU-002 | MUST NOT | 7 | TODO(fixture): F10 adapter suite, boundary lint check 2 over `core/` |
| IFC-NEU-003 | MUST NOT | 7 | TODO(fixture): F10 transport suite, boundary lint check 1 over `core/`; G1 |
| IFC-NEU-004 | MUST NOT | 7 | TODO(fixture): F1, F10 |

Retired ids: none. One id appeared only in the draft under review (#279) and was dropped before
this document was first merged, so it was never published and is not retired: IFC-ADP-021.
Its rule moved to [IFC-ADP-012], with the pairing key now observed by the core process.
IFC-TRN-108 first named, in the draft of PR #367, a `SHOULD` to round a deadline carried beside a
sealed frame. That draft was never merged, and the review of PR #367 dropped the deadline beside
a frame, so the id was reused for the rule it holds now and is not retired.

## Appendix B. Revision history

| Revision | Date | Change |
|---|---|---|
| 0.1 (draft) | 2026-10-04 | #273 (E7 blocker B1, #47): document written from the merged Stage 2 specifications. Core neutral types mapped to their wire forms, with local members kept out of payloads; the adapter contract with binding signals, hand-off outcomes, the request sink and the owner index; the transport contract carrying envelopes, authenticated presence records and authenticated receipts, with the declared capability set and the cross-implementation gate; neutrality and containment with the listed exceptions; requirement prefix `IFC`; fixtures under `tests/protocol/ifc-typ/` and `tests/protocol/ifc-trn/`. Supersedes `docs/planning/v0.1/05-interfaces.md` §13-§15. |
| 0.1 (draft) | 2026-10-04 | Review of PR #279 and the operator rulings on #273: operations whose meaning changed renamed (`take_connection`, `watch_attachments`, `set_binding`, `accept_requests`, `send_presence`); persistence and offline queueing declared absent, and a transport holds a copy only in flight and never past its deadline or across a restart (IFC-TRN-026, IFC-TRN-033 to IFC-TRN-037); the `not-passed` request result; Appendix C gives every `MUST` and `MUST NOT` of the four documents one owner, checked by the reference runner; connections are created and authenticated by the core process (IFC-ADP-012, IFC-ADP-013); transport non-disclosure of subscriptions (IFC-TRN-043, IFC-TRN-044); `ChannelMessage` keeps unrecognized members (IFC-TYP-003); scope and device grants in Table 4.9; receipts addressed to the verifying key; the hand-off never steers and is made at most once (IFC-ADP-057, after #278); single-member negative fixtures for IFC-TYP-050 and IFC-TYP-060. |
| 0.1 | 2026-10-06 | Frozen at Gate S2 (E7, #47): signed off on this date, in force from the merge of PR #276. |
| 0.2 | 2026-10-08 | #69, PR #350, with `spec/bindings/mcp.md` 0.2 (the issued-value pairing for one v0.1 harness, §4.5 there): `NativeSignal` gains `revealed` and a `refused` result gains `pairing_value` (§4.10); the `attachment-unconfirmed` event (§5.4); IFC-ADP-090 (pairing values drawn from a secure random source), IFC-ADP-091 (the event comes before the request), IFC-ADP-092 and IFC-ADP-093 (the core stops serving the attachment until it is paired again, and records a finding); Appendix C gives owners to the `MUST` and `MUST NOT` requirements among MCPB-ATT-004 to MCPB-ATT-026, and to MCPB-CDX-006. Minor revision under `docs/planning/decisions/E7-interface-freeze.md` §7: no wire form changes, the added members are optional, and every new `MUST` binds only an implementation that issues pairing values or receives the new event, so an implementation conformant to 0.1 stays conformant. |
| 0.3 | 2026-10-09 | Sealing transports, with `spec/security.md` 0.3 (payload sealing, §14 there), on the lead's ruling of 2026-10-09 on PR #364 (review finding 2): `Payload` gains the kind `sealed` (§4.11, Table 6.1); `TransportCapabilities` gains `sealing` (Table 6.3); §6.10 adds IFC-TRN-100 to IFC-TRN-104 (the core passes and takes only sealed payloads on a sealing transport, addressed to the recipient device, with room for the frame's overhead), IFC-TRN-105 to IFC-TRN-110 and IFC-TRN-113 (the transport hands sealed payloads to the local device's subscription, shows no destination, carries nothing beside a frame, deadline included, shows no device key id or session id, declares a size floor of 65590, declares the `sealing` value its binding states, and refuses any other payload kind), IFC-TRN-111 (`SHOULD`: seal on every unrestricted cross-implementation transport) and IFC-TRN-112 (the binding document states what travels beside a frame); [IFC-TRN-034] no longer binds a sealing transport's receiving end, in the rule's own text, and the receiving core drops late payloads in its place; [IFC-TRN-081] admits a sealing transport as a second way across implementations, with a dated note; §6.2, §6.4 and §8 updated; Appendix C gives owners to the new `MUST` and `MUST NOT` requirements and to those of SEC-SEL-001 to SEC-SEL-043. Minor revision under `docs/planning/decisions/E7-interface-freeze.md` §7 and `spec/session-channels.md` §5.2 item 9: the relaxed [IFC-TRN-081] forbids nothing it allowed, and every new `MUST`, and the narrower reach of [IFC-TRN-034], whose obligation moves to the receiving core, binds only a transport that declares `sealing` or a core that uses one, which no earlier revision defined. |
| 0.4 (proposed; lead approval pending) | 2026-10-10 | Refs #376, Refs #66, Refs #375. Adds Connection.close and close_bound_ms with a terminal write/closure barrier (IFC-ADP-014 to IFC-ADP-019); bounded shutdown using closure (IFC-ADP-072 to IFC-ADP-078); IdentityRequest and LocalIdentity through the core request sink (IFC-ADP-094 to IFC-ADP-098), with only the public signing-key fingerprint exposed. Appendix A records owned TODO(fixture) cells and Appendix C assigns owners. §7.1 classifies both as minor no-wire-form amendments under E7 §7 item 2, explicitly breaking for local implementations but not for the protocol, lists implementation work, and defers all #375 items to a separate security amendment. The existing identity tool wire form needs no change; each transport/binding document still needs to state its applicable shutdown/close bound under E7 change control. |

## Appendix C. Owner index

Every `MUST` and `MUST NOT` requirement of `spec/session-channels.md` (`SC`),
`spec/security.md` (`SEC`), `spec/bindings/mcp.md` (`MCPB`) and this document (`IFC`) appears
exactly once below, under exactly one owner (§3.3). A row lists the requirements of one area
that one owner carries out, by number. `SHOULD`, `SHOULD NOT` and `MAY` requirements are not
listed. The reference conformance runner checks that every listed id is a `MUST` or `MUST NOT`
that its document defines, and that every such id is listed exactly once.

Some requirements of the companion documents say "a receiver", "a sender" or "an
implementation". That names the implementation as a whole; this index says which part of it
carries the requirement out.

| Area | Owner | Requirements |
|---|---|---|
| SC-ENV | adapter | 064, 082, 091, 092 |
| SC-ENV | core | 001, 002, 003, 010, 011, 020, 021, 022, 023, 025, 026, 027, 030, 040, 041, 050, 060, 061, 062, 063, 065, 066, 070, 071, 072, 073, 080, 081, 083, 090, 100, 101, 102, 103, 104 |
| SC-VER | core | 001, 002, 003 |
| SC-ID | core | 001, 002, 003, 004, 005, 006, 007, 008, 009, 020, 021, 022, 023, 024, 040, 041, 043, 044, 045, 060, 061, 062, 064, 066, 067, 068, 069, 070, 080, 081, 082, 083, 084, 085, 086, 087, 100, 101, 102, 103, 104, 105, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 150, 151, 152, 153, 154, 155, 156, 157, 160, 161, 162, 180, 181, 182 |
| SC-DLV | adapter | 001, 002, 003, 005, 006, 007, 008, 009 |
| SC-DLV | core | 020, 021, 022, 023, 024, 025, 026, 027, 028, 029, 030, 031, 032, 040, 041, 042, 043, 044, 045, 046, 047, 050, 051, 052, 053, 055, 060, 061, 062, 063, 064, 065, 066, 067, 070, 071, 072, 073, 074, 075, 076 |
| SC-RCP | adapter | 004, 005, 006 |
| SC-RCP | core | 001, 002, 003, 007, 008, 009, 020, 021, 022, 023, 024, 025, 026, 027, 028, 029, 030, 031, 032, 033, 040, 041, 050, 051, 052, 053, 054, 060, 061, 062, 070, 071, 072, 073, 074, 075, 076, 077, 078, 079, 080, 081, 085, 086, 090, 091, 092 |
| SEC-KEY | core | 001, 002, 003, 004, 005, 010, 011, 020, 030, 031, 032, 033, 034, 035, 040, 041, 042, 043 |
| SEC-SIG | core | 001, 002, 003, 004, 010, 011, 012, 013, 020, 021, 022, 023, 024, 030 |
| SEC-STG | core | 001, 002, 003, 004 |
| SEC-RPL | core | 001, 002, 003, 010, 011, 020, 021, 022, 023, 026, 030 |
| SEC-AUZ | adapter | 021, 022, 023, 025, 026, 027 |
| SEC-AUZ | core | 001, 002, 003, 004, 005, 006, 007, 008, 010, 011, 012, 013, 014, 015, 016, 017, 018, 020, 024, 030 |
| SEC-RCT | core | 001, 002, 003, 004, 005 |
| SEC-PRS | core | 001, 002, 003, 004, 005, 006, 007, 010, 011, 012, 013, 014, 015 |
| SEC-PRV | adapter | 001, 002, 003, 004, 005, 006, 007, 008, 009, 010, 012, 013, 014, 015, 016, 017 |
| SEC-SEL | core | 001, 002, 003, 004, 010, 011, 012, 013, 014, 015, 016, 020, 021, 022, 023, 024, 030, 031, 032, 033, 034, 035, 036, 037, 040, 041, 042, 043 |
| MCPB-DLV | adapter | 001, 002 |
| MCPB-EXT | adapter | 001, 002, 003, 004 |
| MCPB-ERA | adapter | 001, 002, 003, 004, 005, 007, 008, 009, 011 |
| MCPB-ATT | adapter | 004, 006, 008, 013, 014, 015, 018, 020, 021 |
| MCPB-ATT | core | 001, 002, 005, 007, 009, 010, 011, 012, 016, 019, 022, 023, 025, 026 |
| MCPB-TOOL | adapter | 001, 002, 003, 004, 005, 006, 008, 009, 010, 011, 012, 013, 014, 015, 016, 017, 018, 019, 020, 021 |
| MCPB-META | adapter | 001, 002, 003, 004, 005, 006, 007 |
| MCPB-FBK | adapter | 001 |
| MCPB-CLD | adapter | 001, 002, 003 |
| MCPB-CDX | adapter | 001, 002, 003, 004, 005, 006 |
| IFC-ADP | adapter | 001, 002, 003, 004, 005, 006, 007, 010, 013, 020, 022, 030, 031, 040, 041, 043, 050, 051, 052, 053, 054, 056, 057, 060, 070, 071, 072, 073, 074, 075, 078, 091, 094 |
| IFC-ADP | core | 011, 012, 014, 015, 016, 017, 018, 019, 042, 055, 077, 090, 092, 093, 095, 096, 097, 098 |
| IFC-ADP | binding | 076, 080 |
| IFC-TRN | core | 010, 011, 012, 013, 022, 024, 025, 032, 037, 041, 042, 051, 062, 081, 100, 101, 102, 103, 104 |
| IFC-TRN | transport | 001, 003, 020, 021, 023, 026, 030, 031, 033, 034, 035, 036, 040, 043, 044, 050, 060, 071, 080, 105, 106, 107, 108, 109, 110, 113 |
| IFC-TRN | binding | 090, 112 |
| IFC-TYP | core | 001, 002, 003, 010, 020, 030, 040, 041, 042, 050, 051, 060, 061, 070, 080, 081, 082, 090, 091, 092, 095 |
| IFC-NEU | adapter | 002 |
| IFC-NEU | core | 001, 004 |
| IFC-NEU | transport | 003 |
