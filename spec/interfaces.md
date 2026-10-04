# OAC Session Channels Interfaces

**Document:** `spec/interfaces.md`, the normative interface contracts of OAC Session
Channels: the core neutral types, the provider adapter contract and the transport contract.
**Revision:** 0.1 (draft, Stage 2). Written by #273 for task E7 (#47).
**Companion documents:** `spec/session-channels.md` (the protocol) and `spec/security.md`
(the security model). This document does not restate their rules. It says which part of an
implementation carries each of them out, and what crosses between those parts.

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

- the **core**, which holds identity, keys, policy, presence and delivery state, and
  applies every rule of `spec/session-channels.md` and `spec/security.md` that is not
  assigned elsewhere (§5.2);
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
own parts. Section 8 covers threats that sit on these boundaries.

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

The operation names of §5 and §6 are the conceptual names of `docs/planning/DESIGN.md`
("Provider adapter contract", "Transport contract"). Their inputs and outputs are new: they
follow the merged specifications, not the conceptual sketch. Section 9 records each
difference.

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
- **Adapter binding document:** the document that maps the adapter contract onto one
  harness's surfaces ([IFC-ADP-080]).
- **Transport binding document:** the document that maps the transport contract onto one
  transport ([IFC-TRN-090]).
- **Payload:** the octets of one envelope, one authenticated presence record or one
  authenticated receipt, as the core passes them to a transport (§6.1).
- **Carrier handle:** an opaque handle by which a transport names the far end of the link
  over which a payload arrived. It names a link, not a device or a session.
- **Request:** a send, reply or discovery request that a harness makes through an adapter.
  It is not an envelope; the core creates the envelope, if any.

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
| `ChannelMessage` | wire, with local members | the envelope, `spec/session-channels.md` §4 |
| `DeliveryReceipt` | wire | the receipt, `spec/session-channels.md` §8.1.4 |
| `PresenceRecord` | wire | the presence record, `spec/session-channels.md` §7.2.2 |
| `PresenceState` | local | none (`spec/session-channels.md` §7.2.1) |
| `SecurityPrincipal` | local | none; built from `spec/security.md` §5.3 |
| `AuthorizationRequest`, `AuthorizationDecision` | local | none (`spec/security.md` §9) |
| Adapter-boundary types (§4.10) | local | none |
| Transport-boundary types (§4.11) | local, except the payload octets | the payloads of §6.1 |

[IFC-TYP-001] A core type that Table 4.1 maps to a wire form MUST have, as its wire members,
exactly the members that the wire form defines, with the same names, types and presence.

The wire form governs. A wire form's later minor revision can add an optional member
(`spec/session-channels.md` §5.2), and the type follows it.

[IFC-TYP-002] An implementation MUST NOT place a local member of a core type, or a value of a
local type, in any value that it passes to a transport or to a peer.

Adapter-boundary values (§4.10) do reach a harness, through its adapter: a `RequestResult`, and
the provenance set an adapter renders (`spec/security.md` §12). They never reach a transport.

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
| the eleven envelope members of `spec/session-channels.md` §4.2 | wire | as §4.2 there | as §4.2 there |
| `verified_by` | local | `SecurityPrincipal` (§4.8) | zero or once |

[IFC-TYP-040] The wire members of a `ChannelMessage` MUST hold the values that the envelope
had when it was signed, for an envelope the implementation sends, or when it was received,
for an envelope a transport delivered.

The signature covers the envelope as received (`spec/security.md` §6.2). A type that
normalized, reordered or defaulted a member would verify something other than what was sent.

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

A receipt without `envelope_from` names no envelope, because an envelope id is unique only per
sending session ([SC-ENV-027]); a peer discards it (fixture `ifc-typ/IFC-TYP-050.n01`). A
receipt that crosses to another implementation travels inside an authenticated receipt
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

A `PresenceState` is one of `online`, `unreachable` and `unknown`
(`spec/session-channels.md` §7.2.1). It is what one observer concludes from the records it
accepted, its clock and carrier loss. It is not a record and it is not sent. A record shaped as
a state, with a `state` and an `observed_at` in place of `seq`, `present` and `issued_at`, is
discarded (fixture `ifc-typ/IFC-TYP-060.n01`).

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

Table 4.9.

| Kind | Question | Inputs | Defined in |
|---|---|---|---|
| `deliver` | Does an inbound grant or a live reply right cover this verified envelope? | the verifying key id; the envelope's `from`, `to` and `reply_to`; the binding table; the grants; the reply rights | `spec/security.md` §9.1, §9.2, §9.5, Table 7.1 step 4 |
| `discover` | May this requester discover this session? | the requester (an own session id, or a peer device's key id); the session id; the binding table; the grants; the hand-off records within the reply period | `spec/security.md` §9.4, §9.5 |
| `release-presence` | May this own session's presence record be released to this device? | the session id; the device's key id; the grants | `spec/security.md` [SEC-AUZ-011] |
| `accept-presence` | Is this announcement's signing key related to this consumer for this session? | the signing key id; the session id; the grants; the sent and hand-off records within the reply period | `spec/security.md` [SEC-AUZ-017] |
| `relay-permission` | May a peer message answer this session's permission request? | the session id; the operator's relay setting for it | `spec/security.md` [SEC-AUZ-021] |
| `steer` | May a peer message be handed off through a steering operation? | the session id; the operator's steering setting for it | `spec/security.md` [SEC-AUZ-022] |

Every input is either authenticated (a key id that verified a signature, a member inside the
signed scope), or a fact the implementation recorded itself (grants, reply rights, binding
table, hand-off and sent records, working-directory scopes of its own sessions, operator
settings).

[IFC-TYP-080] An `AuthorizationRequest` MUST take its inputs only from the inputs that Table
4.9 lists for its kind.

This is the type-level form of [SEC-AUZ-004]: a display form, an alias, a `display_name`, a
harness label, a principal label alone, a transport peer identifier, a harness-native id, a
cross-check value and anything from `content` have no slot to enter by.

An `AuthorizationDecision` has `outcome`, one of `permit` and `deny`, and, for `permit`,
`basis`: the grant or the reply right that permits it. A `deny` decision of kind `deliver`
fails security step 4 with the code `unauthorized`.

[IFC-TYP-081] An `AuthorizationDecision` MUST be `deny` unless it names, as its basis, the
recorded fact among its kind's inputs in Table 4.9 that permits it.

The basis is a grant, a live reply right, a hand-off or sent record within the reply period,
or an operator setting, as the rule cited for the kind allows.

This is default deny ([SEC-AUZ-001]) made structural: a decision with no basis cannot be
`permit`.

[IFC-TYP-082] An implementation MUST NOT use the outcome of a decision of one kind as the
outcome of a decision of another kind.

A grant that permits delivery does not permit relay or steering ([SEC-AUZ-020]), and a
binding or a presence state is no decision at all ([SC-ID-157], [SC-ID-181]).

### 4.10 Adapter-boundary types

These local types cross the adapter contract (§5). None of them crosses a transport.

**`Attachment`** — a handle that the adapter creates for one attachment
(`spec/session-channels.md` §6.7.1).

**`NativeSignal`** — what an adapter observed at one native signal
(`spec/session-channels.md` §6.7.1):

| Member | Type | Presence |
|---|---|---|
| `native_id` | string, the harness-native id N | exactly once |
| `start_kind` | one of `fresh`, `transition`, `unmapped` | zero or once; absent when the harness reported none |
| `cross_check` | string, the signal's cross-check value | zero or once |
| `pairing_key` | a value the adapter observed from the operating system, or absent when it observed none | zero or once |

**`AdapterCapabilities`** — what an adapter can do for one attachment: `active_inbound` (a
boolean), `content_types` (part types it can hand off besides `text`, optional) and
`max_envelope_octets` (an integer, optional). It has no `revision` and no extension
identifier; the core adds those ([IFC-ADP-042]).

**`SendRequest`** — a harness's request to send: `attachment` (the `Attachment` it arrived
on), `to` (a `SessionIdentity`), `content` (content parts, `spec/session-channels.md` §4.5)
and, for a reply, `requested_target` (the identifier token the harness named,
`spec/session-channels.md` §8.2.2). `conversation_id` and `correlation_id` are optional, for a
new message.

**`DiscoveryRequest`** — a harness's request for a discovery result: `attachment`.

**`RequestResult`** — the core's answer to a request. For a send: `sent`, with the
envelope's `id`, its `DeliveryState` (`accepted-by-adapter`), and, for a reply, `correlated`
or `uncorrelated` ([SC-RCP-055]); or `refused`, with an `ErrorCode` whose scope in Table 8.3
includes `request`. For a discovery request: the discovery result (an array of
`SessionDescriptor`), or `refused` with an `ErrorCode`. A `sent` result also carries an event
stream of the `DeliveryReceipt` values that the core later holds for the envelope.

**`ProvenanceSet`** — the provenance set of `spec/security.md` §12.1: sender, device,
session, message id and reply target.

**`HandOff`** — what the core gives an adapter to hand off: `attachment`, `message` (a
verified, authorized `ChannelMessage`) and `provenance` (its `ProvenanceSet`).

**`HandOffOutcome`** — one of `completed`, `not-now`, `failed`, `indeterminate` and
`refused` (§5.5).

**`HealthStatus`** — `state`, one of `healthy`, `degraded` and `unavailable`, and optionally
`detail`, a diagnostic string for an operator.

[IFC-TYP-090] A `SendRequest` MUST NOT have a member that names the requesting session.

The core attributes a request by its attachment ([SC-ID-160]) and never by a session id the
request carries ([SC-ID-162]). With no such member in the type, an adapter has nowhere to put
one (fixture `sc-id/SC-ID-162.p01` shows a core ignoring one that a request asserts).

[IFC-TYP-091] The core MUST compute the `ProvenanceSet` of a `HandOff` from the verified
`ChannelMessage` and its `verified_by`, as [SEC-PRV-002] requires.

[IFC-TYP-092] A `HealthStatus` MUST NOT contain a credential, private key material, a
transport-native address, a harness-native id or a working directory.

### 4.11 Transport-boundary types

These types cross the transport contract (§6).

**`Destination`** — where a payload goes: `session`, a `SessionIdentity`, for an envelope;
or `device`, a key id, for an authenticated presence record or an authenticated receipt.

**`Payload`** — `kind`, one of `envelope`, `presence` and `receipt` (§6.1), and `octets`.

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

An adapter connects the core to one harness. It observes the harness's sessions through the
harness's supported surfaces, hands content off through the harness's supported input
surface, renders provenance, and takes the harness's requests. It decides nothing about
identity, authorization, presence or delivery state. Those are the core's.

[IFC-ADP-001] An adapter MUST NOT invoke an operation of a transport.

[IFC-ADP-002] An adapter MUST NOT create, sign, verify or alter an envelope, a presence record,
a registration record or a receipt.

[IFC-ADP-003] An adapter MUST pass each send, reply and discovery request that a harness makes
to the core, through `publish_output` (§5.6).

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

### 5.2 Which part carries out which rule

Table 5.1 assigns each group of rules of the companion documents to the part of an
implementation that carries it out. A rule that the table does not list is the core's.

Table 5.1.

| Rules | Part | What that part does |
|---|---|---|
| `spec/session-channels.md` [SC-ID-121] | adapter | observes the pairing key from the operating system and reports it in `NativeSignal` |
| `spec/session-channels.md` §6.7.2-§6.7.5 (pairing decisions, the ordered cases, re-binding, lifetime), §6.8 (attribution) | core | pairs, binds, deregisters and attributes, from what the adapter reports |
| `spec/session-channels.md` [SC-DLV-002], [SC-DLV-003], [SC-DLV-005] | adapter | hands off through a supported input surface, without polling, and offers no retrieval surface |
| `spec/session-channels.md` [SC-DLV-008], [SC-DLV-009], [SC-RCP-004], [SC-RCP-006] | adapter | reports the hand-off call's outcome truthfully (§5.5) |
| `spec/session-channels.md` §8.1 (states and receipts), §8.3 (codes), §8.4 (retry) | core | turns outcomes into states, codes and receipts (Table 5.3) |
| `spec/session-channels.md` [SC-RCP-005], [SC-RCP-055] | adapter and core | each presents states, and the correlation of a reply, without overstating them |
| `spec/security.md` §12.1-§12.4 (rendering, carriers, presentation), except [SEC-PRV-002] | adapter | renders the `ProvenanceSet` it is given |
| `spec/security.md` [SEC-PRV-002] | core | computes the `ProvenanceSet` ([IFC-TYP-091]) |
| `spec/security.md` [SEC-AUZ-021], [SEC-AUZ-022] | core decides, adapter acts | the core makes the `relay-permission` and `steer` decisions; the adapter uses a relay or steering operation only on a `permit` |
| `spec/security.md` [SEC-AUZ-023] | adapter | never automates a harness's own consent step |
| `spec/security.md` [SEC-AUZ-030] | the part that ends the attachment's local connection | authenticates the local process at the other end |
| `spec/session-channels.md` [SC-DLV-046], §7.2.4 (carrier loss) | transport reports, core applies | §6.6 |
| `spec/session-channels.md` [SC-DLV-066] (records only to authorized peers) | core releases, transport restricts | [SEC-AUZ-011] in the core; [IFC-TRN-080], [IFC-TRN-081] for the transport |

[IFC-ADP-010] An adapter MUST satisfy each rule that Table 5.1 assigns to the adapter.

[IFC-ADP-011] The core MUST satisfy each rule that Table 5.1 assigns to the core.

### 5.3 Operations

Table 5.2. `ProviderAdapter`.

| Operation | Caller | Inputs | Output |
|---|---|---|---|
| `discover_sessions` | core | none | an event stream of adapter events (§5.4) |
| `attach` | core | an `Attachment`; a `SessionIdentity`, or none | none |
| `capabilities` | core | an `Attachment` | `AdapterCapabilities` |
| `deliver` | core | a `HandOff` | a `HandOffOutcome` |
| `publish_output` | core | a request sink: an operation that takes a `SendRequest` or a `DiscoveryRequest` and returns a `RequestResult` | none |
| `health` | core | none | `HealthStatus` |
| `shutdown` | core | none | none |

The core calls every operation. The adapter speaks to the core only through the outputs of
those calls: the event stream of `discover_sessions`, the result of each call, and the
request sink that `publish_output` gave it.

### 5.4 `discover_sessions` and `attach`: session binding signals

`discover_sessions` yields what the adapter observes about live harness sessions. It does not
yield session descriptors: a session has no session id until the core binds it, and the
descriptor is the core's (§6.3 of `spec/session-channels.md`). Its events are:

| Event | Members | Meaning |
|---|---|---|
| `attachment-opened` | `attachment`; optionally `cross_check`, the attachment's cross-check value | a local path to one live session opened |
| `native-signal` | `signal`, a `NativeSignal` | a native signal arrived through the surface the adapter binding document names as authoritative for N |
| `capabilities-changed` | `attachment` | `capabilities` for that attachment would now return a different value |
| `attachment-closed` | `attachment` | the attachment ended |

[IFC-ADP-020] An adapter MUST report attachments and native signals only as it observed them
through a surface that its adapter binding document names.

[IFC-ADP-021] An adapter MUST report, with each native signal, the pairing key it observed from
the operating system for that signal, or no pairing key when it observed none.

A pairing key that the attaching process supplied is not observed ([SC-ID-121]). With no
pairing key the core cannot pair the signal, and binds nothing ([SC-ID-125]). That costs
availability, never authority.

[IFC-ADP-022] An adapter MUST report `attachment-closed` for each attachment it reported open,
when that attachment ends.

The core then deregisters the binding ([SC-ID-155]) and issues a withdrawal ([SC-DLV-055]).

[IFC-ADP-043] An adapter MUST report `capabilities-changed` when the value that `capabilities`
returns for an attachment changes.

The core then issues a new announcement ([SC-DLV-052]).

`attach` tells the adapter the core's binding for one attachment: the session id it bound, or
none when the attachment is unbound, its binding ended, or delivery to it is withheld under
[SC-ID-154].

[IFC-ADP-030] An adapter MUST NOT hand off to an attachment for which the core's latest
`attach` call named no session.

An adapter still passes requests from an unbound attachment to the core, which refuses them
with `unauthorized` ([SC-ID-161]).

[IFC-ADP-031] An adapter MUST label each request it passes to the core with the attachment on
which the request arrived.

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
deadline ([SC-RCP-091]). The adapter renders the provenance set, makes the hand-off call
through the harness's supported input surface, and reports what it observed.

[IFC-ADP-050] An adapter's `deliver` operation MUST return exactly one `HandOffOutcome` for each
`HandOff`.

[IFC-ADP-051] An adapter MUST return `completed` only when the harness surface's input call
completed successfully, as that surface defines completion.

[IFC-ADP-052] An adapter MUST return `not-now` only when the harness surface turned the call
away as unable to take input now, without having taken the input.

[IFC-ADP-053] An adapter MUST return `indeterminate` when the input call returned neither
success nor failure.

[IFC-ADP-054] An adapter MUST return `refused`, without making the input call, when the
harness surface would drop or alter a provenance field, or a provenance value fails
[SEC-PRV-003].

[IFC-ADP-056] An adapter MUST NOT hold a `HandOff` for a later input call.

A not-now refusal is reported at once, and the sending implementation decides what to do
([SC-DLV-007]). Input that the harness itself queues after a completed call is the harness's.

An adapter returns `failed` for any other failure of the input call. Table 5.3 gives the
state and code the core reports for each outcome.

Table 5.3.

| `HandOffOutcome` | Delivery state | Code | Rule |
|---|---|---|---|
| `completed` | `handed-to-harness` | none | [SC-RCP-004] |
| `not-now` | `unreachable` | `destination-unavailable` | [SC-DLV-008] |
| `failed` | `failed` | `handoff-failed` | [SC-DLV-009] |
| `indeterminate` | `unknown` | none | [SC-RCP-006] |
| `refused` | `failed` | `internal-error` | [SEC-PRV-014] |

*Dated note, 2026-10-04 (#273): [SEC-PRV-006] names no code for a refusal because a surface
would drop or alter a provenance field. Table 5.3 gives it `internal-error`, the code that
[SEC-PRV-014] gives the other provenance refusal: no hand-off call is made, so
`handoff-failed` does not fit. This is chosen here and left for operator review on #273.*

[IFC-ADP-055] The core MUST report, for each `HandOffOutcome`, the delivery state and code
that Table 5.3 assigns to it.

No outcome means that a model read, understood or acted on the message
(`spec/session-channels.md` §8.1.3). An adapter whose harness gives no signal stronger than the
end of a write reports `completed` at the end of the write, and nothing more.

### 5.6 `publish_output`: requests, refusals and receipts

`publish_output` gives the adapter the core's request sink. The adapter passes each request a
harness makes into the sink ([IFC-ADP-003]) and returns the `RequestResult` to the harness.
A refusal is a `RequestResult` with an `ErrorCode` from Table 8.3 whose scope includes
`request` ([SC-RCP-075]), never a silent drop ([SC-ID-102]).

[IFC-ADP-060] An adapter MUST return to the harness the core's `RequestResult` for each request,
with its outcome and its `ErrorCode` unchanged.

An adapter binding document says how the harness's surface carries the result; the meaning
does not change on the way.

[IFC-ADP-062] An adapter MAY present to the harness the `DeliveryReceipt` values that arrive on
a `sent` result's event stream. An adapter that does not still returns the `RequestResult`,
and the harness sees only `accepted-by-adapter`.

### 5.7 `health` and `shutdown`

`health` returns a `HealthStatus` ([IFC-TYP-092]).

[IFC-ADP-071] An adapter MUST report `attachment-closed` for each attachment still open before
its `shutdown` returns.

[IFC-ADP-070] After its `shutdown` returns, an adapter MUST NOT hand off content or pass
requests to the core.

### 5.8 Adapter binding documents

[IFC-ADP-080] An adapter MUST have an adapter binding document that names, for its harness,
the surfaces it uses for attachments, native signals and hand-off; the mapping of each
harness-reported start value to a start kind; what completion means for its input call; and
the stability label of each surface it uses.

The binding document is where a harness's own names belong, never this document (§7).

> **Reference implementation note:** the v0.1 per-device process holds the core and the
> transport, and each harness starts a thin process that is the far end of an attachment
> over local IPC (`docs/planning/decisions/C2-process-model.md` §1, §4). The adapter for one
> harness spans that thin process and its half of the per-device process. Today the material
> that [IFC-ADP-080] asks for is split: `spec/bindings/mcp.md` covers the surfaces both v0.1
> adapters share, with each harness's profile in its §8, and
> `docs/planning/decisions/C4-session-identity.md` §3-§4 records the native signals and
> attachments. The adapter tasks (G4 to G8) bring it together for each harness.

---

## 6. Transport contract

### 6.1 The transport's role

A transport carries payloads between implementations. It carries three kinds.

Table 6.1.

| `kind` | Payload | Defined in | `Destination` |
|---|---|---|---|
| `envelope` | the serialized envelope | `spec/session-channels.md` §4 | `session`: the envelope's `to` |
| `presence` | the serialized authenticated presence record | `spec/security.md` §11.1 | `device`: the record's `audience` |
| `receipt` | the serialized authenticated receipt | `spec/security.md` §10.1 | `device`: the key bound to the envelope's `from` |

[IFC-TRN-001] A transport MUST carry payloads of each of the three kinds of Table 6.1.

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
independent of any transport security. Full end-to-end confidentiality of content against a
transport is out of scope for this revision (`spec/security.md` §1.3).

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
optional capabilities that `docs/planning/DESIGN.md` names ("Transport contract"), and three
further members.

Table 6.3.

| Member | Type | Meaning when `true`, or value |
|---|---|---|
| `reliability` | boolean | the transport retransmits a payload until the far end has it, or reports that it could not |
| `persistence` | boolean | the transport keeps payloads across a restart of the transport |
| `offline_queueing` | boolean | the transport holds a payload for a destination that is not reachable, and delivers it later |
| `ordering` | boolean | the transport delivers payloads from one source to one destination in the order passed |
| `multicast_discovery` | boolean | the transport finds transport peers without configured addresses |
| `routing_federation` | boolean | the transport relays payloads through intermediaries between networks |
| `reach` | one of `local-only`, `cross-implementation` | whether the transport reaches other implementations at all |
| `destination_restricted` | boolean | [IFC-TRN-080] |
| `max_payload_octets` | integer | the largest payload the transport carries |

[IFC-TRN-020] A transport MUST declare, in the result of `start`, each of the six optional
capabilities of Table 6.3 as present or absent.

[IFC-TRN-021] A transport MUST NOT declare present an optional capability that it does not
provide.

[IFC-TRN-022] The core MUST NOT depend on any of the six optional capabilities being present.

The core works the same with all six absent. A declared capability can make delivery more
likely or more timely. It never satisfies, relaxes or replaces a rule of the companion
documents. In particular, `offline_queueing` and `persistence` never let a receiver hand off
after the hand-off deadline ([SC-RCP-091]), and do not make a receiver hold an envelope
([SC-DLV-007]).

[IFC-TRN-023] A transport MUST declare a `max_payload_octets` of at least 65536.

[IFC-TRN-024] An implementation MUST NOT declare a `max_envelope_octets` for a session larger
than the `max_payload_octets` of the transport that carries envelopes to that session.

*Dated note, 2026-10-04 (#273): the 65536-octet floor is the default envelope limit of
[SC-ENV-004]. It is chosen here, with the other members of Table 6.3 beyond DESIGN's six, and
left for operator review on #273.*

### 6.4 Operations

Table 6.4. `Transport`.

| Operation | Caller | Inputs | Output |
|---|---|---|---|
| `start` | core | the local device's key id; a `TransportConfiguration` | `TransportCapabilities` |
| `publish` | core | a `Destination`; a `Payload` of kind `envelope` or `receipt` | `PublishResult` |
| `subscribe` | core | a `Destination` naming a local session or the local device; a handler that takes `Inbound` values | a subscription that the core can end |
| `announce_presence` | core | a `Destination` of kind `device`; a `Payload` of kind `presence` | `PublishResult` |
| `watch_presence` | core | a handler that takes presence events (§6.6) | none |
| `health` | core | none | `HealthStatus` |
| `shutdown` | core | none | none |

The key id passed to `start` is public. The transport uses it to receive payloads addressed to
the local device.

[IFC-TRN-025] The core MUST NOT pass to a transport the device key's private seed, or any other
secret of `spec/security.md`.

A transport's own credentials, such as those of an encrypted link, are its own configuration.
They are not the device key and authenticate nothing at the protocol layer ([SEC-SIG-030]).

**`publish` and `announce_presence`.** The result says whether the transport took the payload.

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

### 6.5 Carrying presence records and receipts

**Presence records.** The core issues an authenticated presence record to one device at a time
([SEC-PRS-011]) and passes it to `announce_presence` with that device as the `Destination`.

[IFC-TRN-050] A transport MUST carry each authenticated presence record, as one whole payload,
to the device that its `Destination` names.

[IFC-TRN-051] The core MUST pass to `announce_presence` only an authenticated presence record
whose `audience` is the device that the `Destination` names.

Within one implementation no presence record is needed: an implementation knows its own
sessions' presence (`spec/session-channels.md` §7.2.1).

**Receipts.** A receiver that sends a receipt to another implementation ([SC-RCP-042])
passes the authenticated receipt to `publish` with the `device` of the key that the
envelope's `from` is bound to. The sending implementation receives it on its device
subscription and accepts it only under [SEC-RCT-003].

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

[IFC-TRN-062] The core MUST apply a carrier loss only to the announcements that arrived with
the same carrier handle.

A carrier loss only moves sessions toward `unreachable` ([SC-DLV-046]). A forged or mistaken
one can at worst deny service until the issuer's next announcement (§7.2.4 there).

### 6.7 Crossing implementations

A presence record reveals that a session exists and what it accepts. [SC-DLV-066] requires
that only peers authorized to discover a session receive its records. The core decides who is
authorized and signs the audience ([SEC-AUZ-011], [SEC-PRS-013]). The transport has to keep the
record from everyone else.

[IFC-TRN-080] A transport that declares `destination_restricted` as `true` MUST deliver each
payload only to the implementation that holds the device key its `Destination` names, or
carry it so that no other implementation can read it.

[IFC-TRN-081] The core MUST NOT pass a presence record for another implementation to a
transport whose declaration does not have `reach` `cross-implementation` and
`destination_restricted` `true`.

Without presence records, nothing crosses: a sender holds a capability declaration only from
an accepted announcement ([SC-DLV-070]), so it sends no envelope to a session of another
implementation, and no receipt follows. [IFC-TRN-081] is therefore the gate for all
cross-implementation traffic.

*Dated note, 2026-10-04 (#273): this restates, as a contract rule, the ruling recorded on #43
and kept by #266 (`spec/session-channels.md` §7.3.2, dated notes): presence, discovery and
sending stay within one implementation until a transport binding meets [SC-DLV-066].
`spec/security.md` §10 and §11 supply the authentication half. The v0.1 transport mapping,
Decision C7 (`docs/planning/decisions/`; §4, §7), carries reachability only, not presence
records, and its local mode has no transport-layer authorization. It therefore meets neither
[IFC-TRN-050] nor [IFC-TRN-080] yet. How well the v0.1 transport would carry presence records
is UNVERIFIED (`spec/session-channels.md` §7.2.5 note; `docs/planning/STATUS.md`, "Open
UNVERIFIED items"; owners F6, F10 and the transport binding).*

### 6.8 `health` and `shutdown`

`health` returns a `HealthStatus` ([IFC-TYP-092]).

[IFC-TRN-071] After its `shutdown` returns, a transport MUST NOT invoke a handler.

### 6.9 Transport binding documents

[IFC-TRN-090] A transport MUST have a transport binding document that states how it maps each
`Destination` to its own addressing, its `TransportCapabilities` declaration with the evidence
for each member, how it meets [IFC-TRN-080] when it declares `destination_restricted`, and how
it detects carrier loss, if it reports any.

> **Reference implementation note:** the v0.1 transport's mapping is recorded in Decision C7
> (`docs/planning/decisions/`; §2 the containment boundary, §3 addressing, §4 presence). That
> decision predates this contract. It becomes the transport binding document once it states
> the members of Table 6.3 and the carriage of §6.5 (tasks G1, G2).

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

[IFC-NEU-004] The core MUST NOT read the inside of an `Attachment`, a `CarrierHandle` or a
`TransportConfiguration`.

Table 7.1 lists every value that crosses a contract and is not fully neutral, with the reason
it is allowed. There are no others.

Table 7.1.

| Value | Crosses | Why it is allowed | What keeps it contained |
|---|---|---|---|
| `NativeSignal.native_id`, `NativeSignal.cross_check`, `attachment-opened.cross_check` | adapter to core | Binding a session (§6.7 of `spec/session-channels.md`) compares them, and nothing else can | They are compared as strings and never interpreted; they go into the registration record only, which is never published ([SEC-KEY-042]); never into an envelope or descriptor ([SC-ID-006], [SC-ID-144]) |
| `NativeSignal.pairing_key` | adapter to core | Pairing needs an operating-system observation ([SC-ID-121]) | Compared for equality only; local; never sent |
| `Attachment` | both ways | The core attributes requests by attachment ([SC-ID-160]) | Opaque handle ([IFC-NEU-004]) |
| `CarrierHandle` | transport to core | Carrier loss is per link ([SC-DLV-046]) | Opaque handle; never an identity ([IFC-TRN-012]) |
| `TransportConfiguration` | core to transport | A transport needs its own settings | Opaque; the core passes it to `start` unread |
| `HealthStatus.detail` | either part to core | Diagnostics for an operator | Free text for people; no code path reads it; [IFC-TYP-092] limits its content |

> **Reference implementation note:** the CI boundary lint (`.github/workflows/boundary-lint.yml`)
> scans every file under `spec/`, this one included, for transport and harness vocabulary,
> and checks 1 and 2 of `oac-boundaries` will scan the core module once it exists. The rules
> above are what those scans approximate.

Once Gate S2 freezes this document with the other Stage 2 documents (task E7, #47), a change
to a type or an operation here is a recorded amendment, not a silent edit
(`oac-spec-authoring` §7). Whether it also needs a new minor or major version is decided by
`spec/session-channels.md` §5.2-§5.3 for the wire forms it touches.

---

## 8. Security considerations

This section adds no requirement. It lists the threats that sit on the boundaries this
document draws, and the rules that answer them. The threats of the protocol itself are in
`spec/security.md` §13.

| Threat | Answered by |
|---|---|
| An adapter, a harness or a model reaches a transport directly, skipping signing, authorization or presence checks | [IFC-ADP-001], [IFC-ADP-002], [IFC-ADP-003], [IFC-ADP-007] |
| A harness claims to be another session | [IFC-TYP-090], [IFC-ADP-031]; [SC-ID-160], [SC-ID-162] |
| An adapter overstates a hand-off ("the model saw it") or hides an indeterminate one | [IFC-ADP-051] to [IFC-ADP-055]; [SC-RCP-005] |
| A transport identity, address or discovered peer is taken as a device or a session | [IFC-TRN-012], [IFC-TRN-013], [IFC-NEU-003] |
| A transport reports a failure for a payload it may still deliver, inviting a double hand-off | [IFC-TRN-031], [IFC-TRN-032] |
| A presence record reaches a peer that may not discover the session | [IFC-TRN-080], [IFC-TRN-081]; [SEC-AUZ-011], [SEC-PRS-013] |
| A received presence state is trusted as if it were a record | [IFC-TYP-061] |
| A type carries the working directory or a native id, and a serializer leaks it | [IFC-TYP-020], [IFC-TYP-002]; [SC-ID-045] |
| An unverified envelope's principal is treated as verified | [IFC-TYP-041], [IFC-TYP-042] |
| A delivery grant is reused to approve a permission request or to steer | [IFC-TYP-082]; [SEC-AUZ-021], [SEC-AUZ-022] |
| Health output exposes secrets or addresses | [IFC-TYP-092] |
| A transport is handed the device key | [IFC-TRN-025]; [SEC-KEY-004] |

---

## 9. What changed from the M0 draft

Informative. The M0 draft, `docs/planning/v0.1/05-interfaces.md` §13-§15, was written before
the Stage 2 specifications and contradicts them. This table records each change and its
reason. It also records where the operations differ from the conceptual lists of
`docs/planning/DESIGN.md`, which E7 (#47) asks to document.

| M0 draft or DESIGN sketch | This document | Reason |
|---|---|---|
| `SessionIdentity{session_id, device_key_fingerprint}` | the session id alone (§4.2) | The id is opaque ([SC-ID-004], [SC-ID-005]); the key binding is the registration record and the binding table (`spec/security.md` §5.4, §11.3) |
| `SessionDescriptor{identity, display_uri, working_directory, registered_at}` | the §6.3 descriptor: `session_id`, `capabilities`, `display_name?`, `harness_label?` (§4.3) | [SC-ID-045] bans the working directory; the display form is never on the wire ([SC-ID-020], [SC-ID-021]); capabilities travel in the descriptor ([SC-ID-041]) |
| `SessionCapabilities{active_inbound, spec_revision, extension_id, negotiated_capabilities}` | the §6.4 declaration keyed by extension identifier (§4.4) | One entry per implemented major version; a flat record negotiates nothing (fixture `ifc-typ/IFC-TYP-030.n01`) |
| `ChannelMessage` with `principal` replacing `security` | the envelope members as signed or received, plus a local `verified_by` (§4.5) | The signature covers the envelope as received ([SEC-SIG-011]) |
| `DeliveryReceipt{state, envelope_id, error?}` | the §8.1.4 receipt, with `envelope_from`, `observer`, `observed_at` (§4.6) | An id is unique only per sending session ([SC-ENV-027]); every state has an observer (§8.1.1 there) |
| `PresenceRecord{session_id, state, observed_at}` | the §7.2.2 record; the observer's conclusion is the local `PresenceState` (§4.7) | The record is what an issuer signs and sends ([SEC-PRS-001]); the state is a consumer's own judgement |
| `SecurityPrincipal{principal_ref, device_key_fingerprint}` | `principal` and `key_id` of a trusted-key-set entry (§4.8) | `spec/security.md` §5.2-§5.3; the key id is the fingerprint |
| Authorization decision `{outcome, reason}` | kinds, closed input lists, `permit` with a basis (§4.9) | One-way grants, reply rights and default deny (`spec/security.md` §9) |
| `discover_sessions() -> [SessionDescriptor]` | an event stream of attachments and native signals (§5.4) | A session has no id or descriptor until the core binds it (`spec/session-channels.md` §6.7) |
| `attach(session) -> Result` | the core tells the adapter its binding for an attachment (§5.4) | Binding is the core's ([IFC-ADP-007]) |
| `capabilities(session) -> SessionCapabilities` | `capabilities(attachment) -> AdapterCapabilities`; the core adds revision and identifier (§5.5) | The core implements the revisions; the adapter knows the harness |
| `deliver(session, message) -> DeliveryReceipt` | `deliver(HandOff) -> HandOffOutcome`, mapped by the core (§5.5) | The adapter reports what it observed; the core owns states and receipts ([SC-RCP-003]) |
| `publish_output(callback)` | `publish_output(request sink)`, returning a `RequestResult` and later receipts (§5.6) | Requests are refused or turned into envelopes by the core, with request-scope codes |
| `Transport.start(identity, config)` | `start(device key id, configuration) -> TransportCapabilities` (§6.4) | Payloads are addressed to sessions and to devices; the capability declaration is returned, not assumed |
| `publish(destination, envelope)` | `publish(Destination, Payload)` for envelopes and receipts, returning `taken` or `not-taken` (§6.4) | Receipts need carriage (`spec/session-channels.md` §8.1.5); passed or not passed drives §8.4.1 there |
| `subscribe(address, handler)` | `subscribe(Destination, handler)` returning an endable subscription (§6.4) | Bindings end ([IFC-TRN-042]) |
| `announce_presence(PresenceRecord)` | `announce_presence(device, authenticated presence record)` (§6.5) | A record goes to one audience ([SEC-PRS-011]); reachability alone is not a record (C7 §4 gap) |
| `watch_presence(handler)` | records and carrier losses, each with a carrier handle (§6.6) | Carrier loss is per link ([SC-DLV-046]) |
| "Optional capabilities: reliability, persistence, offline queueing, ordering, multicast discovery, routing/federation" | declared booleans, plus `reach`, `destination_restricted`, `max_payload_octets` (§6.3) | Declared, not assumed (E7 acceptance); cross-implementation gating ([IFC-TRN-081]) |
| A "binding/mapping annex" naming the v0.1 harnesses and transport | adapter and transport binding documents ([IFC-ADP-080], [IFC-TRN-090]) | Neutral text names no harness or transport (§7) |

---

## 10. References

### 10.1 Normative references

- [RFC2119] Bradner, S., "Key words for use in RFCs to Indicate Requirement Levels",
  BCP 14, RFC 2119. https://www.rfc-editor.org/rfc/rfc2119
- [RFC8174] Leiba, B., "Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words",
  BCP 14, RFC 8174. https://www.rfc-editor.org/rfc/rfc8174
- `spec/session-channels.md`, the OAC Session Channels specification.
- `spec/security.md`, OAC Session Channels security.

### 10.2 Informative references

- `docs/planning/ADR-001.md`, "Boundary" and "v0.1 scope".
- `docs/planning/DESIGN.md`, "Core", "Provider adapter contract" and "Transport contract".
- `docs/planning/v0.1/05-interfaces.md` §13-§15, the M0 draft this document supersedes.
- `docs/planning/decisions/C2-process-model.md` §1, §4 (per-device process, local IPC).
- Decision C7, the v0.1 transport mapping (`docs/planning/decisions/`; §2, §3, §4, §7).
- `spec/bindings/mcp.md`, the binding of the surfaces the v0.1 adapters share.
- `docs/planning/backlog/04-tasks-EF.json`, tasks E7, F7 and F10.

---

## Appendix A. Requirement index

Fixture paths are relative to `tests/protocol/`. `TODO(fixture)` marks a requirement with no
conformance fixture yet, and names the task expected to supply the test. `covered by` names
the requirement whose fixtures exercise it.

| Id | Level | Section | Fixtures |
|---|---|---|---|
| IFC-TYP-001 | MUST | 4.1 | covered by IFC-TYP-030, IFC-TYP-050 and IFC-TYP-060 (`ifc-typ/IFC-TYP-030.n01`, `ifc-typ/IFC-TYP-050.n01`, `ifc-typ/IFC-TYP-060.n01`); for every type: TODO(fixture), F2 serialization tests |
| IFC-TYP-002 | MUST NOT | 4.1 | TODO(fixture): wire output of every part; F2, F10 |
| IFC-TYP-010 | MUST | 4.2 | TODO(fixture): by construction of the type; F2 |
| IFC-TYP-020 | MUST NOT | 4.3 | TODO(fixture): by construction of the type; F2 |
| IFC-TYP-030 | MUST | 4.4 | `ifc-typ/IFC-TYP-030.p01`, `.n01` |
| IFC-TYP-040 | MUST | 4.5 | TODO(fixture): an envelope passed through the core unchanged; F2, F4 |
| IFC-TYP-041 | MUST NOT | 4.5 | covered by SEC-STG-003 (`sec-stg/SEC-STG-002.n02`); the type itself: TODO(fixture), F2, F4 |
| IFC-TYP-042 | MUST | 4.5 | TODO(fixture): F4 verification tests |
| IFC-TYP-050 | MUST | 4.6 | `ifc-typ/IFC-TYP-050.p01`, `.n01` |
| IFC-TYP-051 | MUST | 4.6 | `ifc-typ/IFC-TYP-051.n01` |
| IFC-TYP-060 | MUST | 4.7 | `ifc-typ/IFC-TYP-060.p01`, `.n01` |
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
| IFC-ADP-010 | MUST | 5.2 | TODO(fixture): each assigned rule's own fixtures, run against an adapter; F10, F11 |
| IFC-ADP-011 | MUST | 5.2 | TODO(fixture): each assigned rule's own fixtures, run against the core; F12 |
| IFC-ADP-020 | MUST | 5.4 | TODO(fixture): F10 adapter suite against the fakes (F8, F9); G4-G8 |
| IFC-ADP-021 | MUST | 5.4 | TODO(fixture): needs the platform facilities; G9, F10 |
| IFC-ADP-022 | MUST | 5.4 | TODO(fixture): F10 adapter suite |
| IFC-ADP-030 | MUST NOT | 5.4 | TODO(fixture): F10 adapter suite |
| IFC-ADP-031 | MUST | 5.4 | TODO(fixture): F10 adapter suite |
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
| IFC-ADP-060 | MUST | 5.6 | TODO(fixture): F10 adapter suite |
| IFC-ADP-062 | MAY | 5.6 | none (MAY) |
| IFC-ADP-070 | MUST NOT | 5.7 | TODO(fixture): F10 adapter suite |
| IFC-ADP-071 | MUST | 5.7 | TODO(fixture): F10 adapter suite |
| IFC-ADP-080 | MUST | 5.8 | TODO(fixture): document review at each adapter's task; G4-G8 |
| IFC-TRN-001 | MUST | 6.1 | TODO(fixture): F10 transport suite against F7, then G1-G2 |
| IFC-TRN-002 | MAY | 6.1 | none (MAY) |
| IFC-TRN-010 | MUST NOT | 6.2 | covered by SEC-SIG-030 (TODO(fixture) there); F11, H2 |
| IFC-TRN-011 | MUST | 6.2 | `ifc-trn/IFC-TRN-011.n01`, `.n02`; loss and delay: TODO(fixture), F10 with F7's fault injection |
| IFC-TRN-012 | MUST NOT | 6.2 | TODO(fixture): F11, H2 |
| IFC-TRN-013 | MUST NOT | 6.2 | TODO(fixture): F10 transport suite; G1 |
| IFC-TRN-020 | MUST | 6.3 | TODO(fixture): F7, F10 transport suite |
| IFC-TRN-021 | MUST NOT | 6.3 | TODO(fixture): F10 transport suite, against each declared capability |
| IFC-TRN-022 | MUST NOT | 6.3 | TODO(fixture): F10, the core run against F7 with every capability absent |
| IFC-TRN-023 | MUST | 6.3 | TODO(fixture): F10 transport suite |
| IFC-TRN-024 | MUST NOT | 6.3 | TODO(fixture): F6, F10 |
| IFC-TRN-025 | MUST NOT | 6.4 | TODO(fixture): F11, H2 |
| IFC-TRN-030 | MUST | 6.4 | TODO(fixture): F10 transport suite |
| IFC-TRN-031 | MUST NOT | 6.4 | TODO(fixture): F10 transport suite with F7's fault injection |
| IFC-TRN-032 | MUST | 6.4 | covered by SC-RCP-085 for the state it drives (`sc-rcp/SC-RCP-085.p06`); the mapping: TODO(fixture), F6, F10 |
| IFC-TRN-040 | MUST | 6.4 | TODO(fixture): F10 transport suite |
| IFC-TRN-041 | MUST NOT | 6.4 | TODO(fixture): F10 transport suite asserts no polling; H1 |
| IFC-TRN-042 | MUST | 6.4 | TODO(fixture): F6, F10 |
| IFC-TRN-050 | MUST | 6.5 | TODO(fixture): F10 transport suite; G2 |
| IFC-TRN-051 | MUST | 6.5 | TODO(fixture): F6, F10 |
| IFC-TRN-060 | MUST | 6.6 | TODO(fixture): F10 transport suite |
| IFC-TRN-061 | MAY | 6.6 | none (MAY) |
| IFC-TRN-062 | MUST | 6.6 | covered by SC-DLV-046 (`sc-dlv/SC-DLV-046.n01`: loss for one carrier leaves another's session online) |
| IFC-TRN-071 | MUST NOT | 6.8 | TODO(fixture): F10 transport suite |
| IFC-TRN-080 | MUST | 6.7 | TODO(fixture): F10 transport suite; G3 |
| IFC-TRN-081 | MUST NOT | 6.7 | TODO(fixture): F6, F10, H2 |
| IFC-TRN-090 | MUST | 6.9 | TODO(fixture): document review; G1, G2 |
| IFC-NEU-001 | MUST NOT | 7 | TODO(fixture): F1 workspace boundaries, boundary lint checks 1-2 over `core/` |
| IFC-NEU-002 | MUST NOT | 7 | TODO(fixture): F10 adapter suite, boundary lint check 2 over `core/` |
| IFC-NEU-003 | MUST NOT | 7 | TODO(fixture): F10 transport suite, boundary lint check 1 over `core/`; G1 |
| IFC-NEU-004 | MUST NOT | 7 | TODO(fixture): F1, F10 |

Retired ids: none.

## Appendix B. Revision history

| Revision | Date | Change |
|---|---|---|
| 0.1 (draft) | 2026-10-04 | #273 (E7 blocker B1, #47): document written from the merged Stage 2 specifications. Core neutral types mapped to their wire forms, with local members kept off the wire; the adapter contract with binding signals, hand-off outcomes, the request sink and the rule-allocation table; the transport contract carrying envelopes, authenticated presence records and authenticated receipts, with the declared capability set and the cross-implementation gate; neutrality and containment with the listed exceptions; requirement prefix `IFC`; fixtures under `tests/protocol/ifc-typ/` and `tests/protocol/ifc-trn/`. Supersedes `docs/planning/v0.1/05-interfaces.md` §13-§15. |
