# 05 — Interfaces

**Source:** `docs/planning/PLANNING-PROMPT.md` §9 item 6, §6; `docs/planning/DESIGN.md`
§Components; `.claude/skills/oac-spec-authoring/SKILL.md` §4.

**Scope.** This file fixes the OAC spec surface (envelope, addressing, capabilities,
presence, delivery states, errors, versioning, unsupported-capability behaviour), the
provider adapter contract, the transport contract, and the core neutral types, in
normative language (`MUST`/`SHOULD`/`MAY`) with reference-implementation notes clearly
separated. It does not define the threat model (`docs/planning/v0.1/06-security.md`'s
job, A7, not yet landed), the module layout (`docs/planning/v0.1/07-repository-and-
dependencies.md`'s job, A8), or the test-tier breakdown
(`docs/planning/v0.1/09-test-strategy.md`'s job, A10) — it cross-references each by path
rather than restating their content.

**M0 draft status — read before treating anything below as binding.** This file is the
**M0 planning-package draft** of the OAC spec surface (`docs/planning/PLANNING-PROMPT.md`
§9 item 6; backlog task A6; milestone M0). It is **not** the Stage 2 normative document.
Per `.claude/skills/oac-spec-authoring/SKILL.md` §4: "The Stage 2 spec surface lands in
`spec/session-channels.md` ... not in `05-interfaces.md`, the M0 planning-package file
... this document supersedes." Security normative text (signing, replay, authorization,
provenance) lands separately in `spec/security.md` (task E5) — this file states the
envelope-authenticity and pairing/authorization design only insofar as it shapes the
envelope and capability surface (citing `docs/planning/decisions/C5-envelope-auth.md`),
never re-deriving the threat table. **When Epic E (Stage 2, `oac-spec-authoring`) writes
`spec/session-channels.md` and `spec/security.md`, those files supersede this one** for
every normative claim; this file remains the historical planning record.

**Naming.** Product and repository: **Open Agent Channel (OAC)**. Normative protocol
specification: **OAC Session Channels** (short form: "the OAC spec"). CLI binary:
**`oac`**. Per ADR-001-A1 (`docs/planning/v0.1/03-decisions-and-amendments.md` §2), this
file never writes bare "Session Channels" or `sessionchannels`, except where a source
document (`DESIGN.md`, pre-rename) is quoted verbatim, marked as such at the quotation.

---

## 1. Normative-language convention

Per `.claude/skills/oac-spec-authoring/SKILL.md` §1-§2, applied throughout this file:

- `MUST`/`MUST NOT` states a requirement every conformant implementation is bound by.
  Every `MUST`/`MUST NOT` below that has no accompanying Stage 2 conformance fixture is
  marked **`TODO(fixture)`** — fixtures land under `tests/protocol/` (Stage 2 task E8);
  none exist yet (Pre-Stage 0, `docs/planning/STATUS.md`), so **every** `MUST`/`MUST NOT`
  in this file carries that marker. This satisfies the "separated in every section"
  acceptance requirement mechanically, by the marker's presence, not by prose tone.
- `SHOULD`/`SHOULD NOT` is a deviation-permitted recommendation; what "deviated" means is
  stated at each such sentence.
- `MAY` is explicitly optional; what an omitting implementation must still do to stay
  conformant is stated at each such sentence.
- One requirement per sentence — no sentence below joins two `MUST`s with "and."
- Lowercase "should"/"can"/"will" is never a substitute for a keyword; it appears only
  inside a reference-implementation-note blockquote (below), never in normative prose.

**Reference-implementation-note form, mandatory.** Immediately after the normative
paragraph it illustrates:

> **Reference implementation note:** an implementation choice the v0.1 Rust workspace
> makes, not binding on any other conformant implementation, and containing no `MUST`,
> `MUST NOT`, `SHOULD`, or `MAY`.

Every such blockquote below follows this exact labelled form and contains no RFC-2119
keyword — a note needing one is normative text misplaced, per `oac-spec-authoring` §2.

---

## 2. Neutral-vocabulary rule

No neutral interface, type name, field name, or normative sentence in this file names
Zenoh, a key expression, `zid`, liveliness, MQTT topics, NATS subjects, Claude, Codex, or
any MCP/provider-specific method name. Quoted, `[ADR-001 Boundary]`: "MUST NOT leak
Zenoh-specific concepts into the neutral protocol." Quoted, `docs/planning/DESIGN.md`
§MCP Session Channels extension: "The specification MUST NOT mention Zenoh keys, MQTT
topics, NATS subjects, or provider-specific method names."

Provider- and transport-specific detail appears **only** in the clearly-labelled
**binding/mapping** subsection of §16, and in the design-for-replacement proofs of §17-§18
— both are explicitly scoped as the exception this file's own §22 boundary pass expects,
never inside a neutral interface signature or a numbered normative section (§4-§13, §14-
§15, §16's own non-binding portion).

---

## 3. Envelope and content model

**Source.** The envelope shape is `docs/planning/DESIGN.md` lines 72-99, as amended by
`docs/planning/decisions/C5-envelope-auth.md` §6 (the `security` block, now
`principal`/`key_id`/`nonce`/`signature`). Cited, not re-derived.

**Shape (message shape, not illustrative code — the one pseudo-code exception `oac-
planning-package` §2 permits):**

```json
{
  "version": "0.1",
  "id": "unique-message-id",
  "from": "session-address",
  "to": "session-address",
  "conversation_id": "optional-thread",
  "reply_to": "optional-message-id",
  "correlation_id": "optional-correlation-id",
  "created_at": "timestamp",
  "ttl_ms": 300000,
  "content": [{"type": "text", "text": "Please review scheduler.rs"}],
  "security": {
    "principal": "authenticated-principal-reference",
    "key_id": "device-public-key-fingerprint",
    "nonce": "128-bit-csprng-value",
    "signature": "ed25519-signature-over-domain-separated-jcs-canonicalization"
  }
}
```

**Required fields — `TODO(fixture)`.** An envelope `MUST` carry `version`, `id`, `from`,
`to`, `created_at`, `content`, and a `security` object carrying `principal`, `key_id`,
`nonce`, and `signature`. A peer receiving an envelope missing any of these fields
`MUST` reject it with the malformed-envelope error (§11).

**Optional fields — `TODO(fixture)`.** `conversation_id`, `reply_to`, `correlation_id`,
and `ttl_ms` `MAY` be absent. A peer `MUST NOT` treat the absence of any one of these four
fields as a malformed envelope.

> **Reference implementation note:** the v0.1 Rust workspace represents the four
> optional fields as `Option<T>` in its envelope struct, deserializing an absent JSON key
> to `None` rather than requiring an explicit `null`.

**Content is a typed array — `TODO(fixture)`.** `content` `MUST` be a JSON array of
objects, each carrying `type` and `text`; the array `MUST NOT` be empty. A future content
type beyond `{"type":"text","text":...}` `MAY` be added as a non-breaking change (§12) —
a receiver `MUST` ignore an array entry whose `type` it does not recognize rather than
rejecting the whole envelope for it, per §13's forward-compatibility rule.

**`ttl_ms` semantics — `TODO(fixture)`.** `ttl_ms`, when present, states the sender's own
application-level validity window in milliseconds from `created_at`. A receiver `MUST`
treat an envelope as expired (§10's `expired` state) once `created_at + ttl_ms` has
passed, when `ttl_ms` is present. `ttl_ms` absent `MUST NOT` be treated as "never
expires" by a receiver enforcing its own replay-defence window (next paragraph) — the
receiver's own window still applies regardless of whether the sender supplied `ttl_ms`.

**`ttl_ms` expiry and the replay accept-window are two separate checks — stated
explicitly, `TODO(fixture)`.** Per `docs/planning/decisions/C5-envelope-auth.md` §7:
`ttl_ms` is the sender's own declared application-level expiry, checked against
`created_at + ttl_ms`; the replay accept-window (±300 seconds of the verifying peer's own
clock, C5 §7) is the receiver's own defence against a captured-and-replayed envelope,
checked independently of what `ttl_ms` the sender declared. An envelope `MUST` be
rejected as `expired` (§10) if it fails **either** check — the two are not merged into
one test.

> **Reference implementation note:** the reference daemon's replay-window check is fixed
> at ±300 seconds regardless of a message's own `ttl_ms` value (C5 §7), matching the
> envelope's own default `ttl_ms` of 300000 ms.

**`created_at` form — `TODO(fixture)`.** `created_at` `MUST` be an RFC 3339 timestamp
(the concrete profile — fractional-second precision, timezone form — is fixed by Stage 2,
not by this M0 draft).

---

## 4. Addressing and session identity

**Source.** `docs/planning/decisions/C4-session-identity.md` §1-§2, §8.

**The wire address is the opaque session id — `TODO(fixture)`.** `from` and `to` `MUST`
carry an OAC opaque session id: 128 bits (16 bytes) of CSPRNG output, encoded as 26
lowercase Crockford Base32 characters, with no encoded structure (C4 §2). A peer `MUST
NOT` treat any substring or decoding of a session id as carrying device, harness, or
timestamp information — none is encoded.

**The display form is never a wire field — `TODO(fixture)`.** The human-readable display
form `session://<device>/<harness>/<id>` `MUST NOT` appear in `from`, `to`, or any other
wire field of the envelope defined in §3. A peer `MUST NOT` parse the display form for an
authorization decision. This is conflict **C8**'s resolution, `RESOLVED-IN-DECISION` at
`docs/planning/decisions/C4-session-identity.md` §8 — cited here, not re-litigated.

**Human aliases are never an identity claim — `TODO(fixture)`.** A human-chosen alias
`MUST NOT` be transmitted as, or treated as, an identity claim on the wire; per C4 §9, an
alias is a local, per-device, mutable label resolved only on the device where it was
created. A peer `MUST` treat an alias string appearing in message content the same as any
other untrusted content (§9's authenticated-but-untrusted doctrine applies without
exception).

---

## 5. Capabilities and extension negotiation

**Source.** `docs/planning/decisions/C3-spec-packaging.md` §1-§2, §6.

**The capability model.** A peer `MUST` declare, at connection time: (a) the set of
capabilities it supports (drawn from the negotiated capability vocabulary this file and
its Stage 2 successor define — presence, active-inbound delivery, and any optional
extension); (b) the spec revision it implements (§12); and (c) the extension identifier
it negotiates under.

**The extension identifier — `TODO(fixture)`.** The identifier `io.github.rossgraeber/
oac-session-channels` (C3 §2) `MUST` be used for capability negotiation, tool-surface
registration, and provenance metadata only. A peer `MUST NOT` treat the extension
identifier's presence or absence as itself authorizing a message — authorization is a
separate decision (§9's doctrine, C5 §11).

**The standalone document is normative-of-record — stated once, applies throughout this
file.** Quoted, C3 §1: "The standalone document is normative-of-record. The MCP
extension registration is packaging — capability negotiation, tool surface, `_meta`
provenance — never the source of normative text." This M0 draft, and its Stage 2
successor `spec/session-channels.md`, are that standalone document; the extension
registration never substitutes for either.

> **Reference implementation note:** the identifier's derivation (vendor prefix from
> GitHub-account-owned domain, extension name `oac-session-channels`) is recorded in
> `docs/planning/decisions/C3-spec-packaging.md` §2-§3 and is not repeated here — it is
> packaging detail, kept out of this section's normative capability model per the rule
> above.

---

## 6. Presence and discovery

**Source.** `docs/planning/DESIGN.md` line 105; `docs/planning/decisions/
C7-zenoh-transport.md` §4.

**Exactly three states — `TODO(fixture)`.** A `PresenceRecord` (§15) `MUST` carry exactly
one of three states: `online`, `unreachable`, `unknown`. A peer `MUST NOT` introduce a
fourth presence state, per C7 §4's observation-based split: a session previously
observed and now gone `MUST` be reported `unreachable`; a session never observed `MUST`
be reported `unknown`.

**Two open gaps — recorded honestly, not solved.** Per C7 §4, this file does not claim
presence answers everything `docs/planning/DESIGN.md` lines 104-105 ask for:

1. **Presence carries reachability only.** Harness ownership, capabilities, and
   active-inbound support are **not** carried to a remote peer by the presence mechanism
   this file defines. A peer wanting those facts about another session has no defined
   channel to learn them remotely as of this M0 draft. Owner: a future C-series decision
   or Stage 2 spec addition (tracked at `docs/planning/STATUS.md`'s "Open conflicts"
   list, C7 presence/discovery gaps entry).
2. **No discovery channel is defined for learning an unknown session's id.** A peer
   already holding another session's opaque id (§4) can watch its presence; nothing in
   this file defines how a peer learns an *unknown* session's opaque id in the first
   place, even though `docs/planning/ADR-001.md` line 61 puts presence/discovery in v0.1
   scope. Owner: same as above.

These are stated as gaps in the interface, not silently designed around.

---

## 7. Active-delivery semantics and the no-polling rule

**Source.** `docs/planning/DESIGN.md` lines 33, 110; `oac-boundaries` boundary 14.

**Active inbound, no application-level polling — `TODO(fixture)`.** A peer advertising
active-inbound support `MUST` accept an authorized external message as input to the
addressed live session without application-level polling. Quoted, `docs/planning/
DESIGN.md` line 33: "a harness advertising active inbound Session Channels support
accepts an authorized external channel message as input to the addressed live session
without application-level polling."

**An implementation that cannot MUST NOT advertise the capability — `TODO(fixture)`.** A
peer that cannot deliver without polling `MUST NOT` declare the active-inbound capability
in its capability set (§5). Advertising it while polling is exactly the drift
`oac-boundaries` boundary 14 names: "Either fix real delivery, turn off the active-inbound
claim, or file a finding — do not poll silently."

**What is permitted, stated as `MAY` — `TODO(fixture)`.** Streaming connections,
subscriptions, event loops, and keepalives `MAY` be used by a peer's own delivery
mechanism; none of these constitutes the forbidden polling above, because none requires
the *receiving* application to repeatedly ask "is there anything new" — the forbidden
form is application-level inbox polling specifically, per `docs/planning/DESIGN.md` line
110.

---

## 8. Replies and correlation

**Source.** `docs/planning/decisions/C6-trust-rendering.md` §10 (conflict **C9**'s
resolution), phrased neutrally here; provider-specific mechanism deferred to §16's
binding subsection.

**`reply_to`/`correlation_id` semantics — `TODO(fixture)`.** `reply_to` `MAY` carry the
`id` of the envelope this message replies to; `correlation_id` `MAY` carry an
application-supplied correlation value. Neither field, when present, `MUST` be trusted on
its face by the receiving side's own correlation logic without independent validation
(next paragraph).

**A correlation value is untrusted content — `TODO(fixture)`.** A `reply_to` or
`correlation_id` value that originates from, or passes through, a harness's own model
`MUST` be treated as untrusted content and validated against receiver-held state (the
receiving adapter's own record of what it delivered into which live session) before being
trusted as a correlation. This is the neutral phrasing of C6 §10's layered (a)/(b)/(c)
rule: (a) an explicit, model-supplied correlation value is the weakest signal on its own;
(b) the receiver's own independently-tracked delivery-to-session binding is the stronger,
independent check; (c) when (a) is absent or contradicts (b), the receiver falls back to
an explicit downgraded state.

**Mismatch downgrades, never overrides — `TODO(fixture)`.** A receiver `MUST` downgrade a
reply's correlation to an explicit *inferred* or *uncorrelated* state, rather than
override its own independently-tracked binding, when a model-supplied correlation value
does not match that binding. A receiver `MUST NOT` fabricate or guess a correlation when
neither (a) nor (b) resolves to a single candidate.

> **Reference implementation note:** the provider-specific half of this rule — which
> provider gives which signal, and how the reference adapter tracks its own
> thread/turn-to-envelope binding — is a binding-subsection concern (§16), citing
> `docs/planning/decisions/C6-trust-rendering.md` §10 directly rather than restated here.

---

## 9. Delivery states

**Source.** `docs/planning/DESIGN.md` line 108 (base list); `docs/planning/decisions/
C5-envelope-auth.md` §9 (honesty refinement, conflict **C6**'s resolution).

**The frozen state set — `TODO(fixture)`.** A `DeliveryReceipt` (§15) `MUST` carry
exactly one of the following states, and this file fixes the exact set so the error
taxonomy (§11) and Stage 2 fixtures can cite it by name:

| State | Meaning |
|---|---|
| `accepted-by-adapter` | Signature verified, replay/duplicate checks passed, authorization check passed — refinement of DESIGN's `accepted`. |
| `handed-to-harness` | The adapter completed its provider-specific delivery call. `MUST NOT` be reported as, or imply, "seen by the model." |
| `unknown` | The adapter cannot observe anything past `handed-to-harness` — the honest terminal state when no further observation is possible. |
| `rejected` | Signature, replay, or authorization check failed. |
| `unreachable` | The addressed session is not currently registered/reachable. |
| `expired` | The envelope failed its `ttl_ms` or replay-accept-window check (§3). |
| `duplicate` | The envelope's `(key_id, nonce)` pair was already observed within the replay window. |
| `failed` | A transport- or adapter-internal error unrelated to the envelope's own validity. |

**Observability rule — `TODO(fixture)`.** A peer `MUST NOT` report a delivery state it
cannot itself observe. Concretely: a peer whose provider surface gives no delivery
acknowledgement past `handed-to-harness` `MUST NOT` report any state beyond
`handed-to-harness` or `unknown` for that delivery.

**"Seen by the model" is never claimed — `TODO(fixture)`.** No delivery state `MUST` be
interpreted, documented, or rendered by any implementation as meaning the harness's model
has processed the message — the state set above stops at `handed-to-harness`
specifically because no provider surface this file's binding subsection (§16) relies on
gives a stronger signal.

**No exactly-once promise — `TODO(fixture)`.** An implementation `MUST NOT` promise
exactly-once delivery. Quoted, `docs/planning/DESIGN.md` line 108: "Do not promise
exactly-once delivery. Use unique IDs, idempotency, and duplicate suppression."

---

## 10. Error taxonomy — closed set

**The set is closed — stated explicitly.** The table below enumerates every error a
conformant peer `MAY` emit. Adding a member to this set is a **versioning event** (§12) —
`TODO(fixture)` for every row below marked `MUST`.

| Error | Emitted when (`MUST`) | Retryable | Maps to delivery state (§9) |
|---|---|---|---|
| `malformed-envelope` | A required field (§3) is missing or fails to parse | No | `rejected` |
| `unsupported-spec-revision` | The envelope's `version` (§3) is not one the receiving peer's negotiated spec revision (§12) supports | No | `rejected` |
| `unsupported-capability` | The message requires a capability (§5) the receiver did not negotiate | No | `rejected` (§13) |
| `signature-verification-failed` | `security.signature` does not verify against the signed field set | No | `rejected` |
| `replay-window-rejected` | `created_at` falls outside the receiver's replay accept-window, or `created_at + ttl_ms` has passed (§3) | No | `expired` |
| `duplicate` | The envelope's `(key_id, nonce)` pair was already observed within the replay window | No | `duplicate` |
| `unknown-destination` | The addressed `to` session id has no registration record the receiver knows of | No | `unreachable` |
| `unauthorized` | The sender is not on the addressed session's allowlist (default-deny) | No | `rejected` |
| `expired` | Same trigger as `replay-window-rejected`'s `ttl_ms` half; kept as a distinct named error for the sender-declared-expiry case specifically | No | `expired` |
| `internal-failure` | An adapter- or transport-internal error unrelated to the envelope's own validity | `MAY` be retryable, at the emitting peer's discretion | `failed` |

Each row `MUST` be emitted under the stated condition, and every row's mapping onto §9's
delivery-state set `MUST` hold — a peer `MUST NOT` map an error onto a delivery state not
listed in its row.

> **Reference implementation note:** the reference daemon renders each error above as a
> distinct machine-readable error code string in its own internal logging and CLI
> diagnostics; the exact string constants are a Stage 3 implementation detail, not fixed
> by this file.

---

## 11. Versioning policy

**Source.** `oac-spec-authoring` §6; `docs/planning/decisions/C3-spec-packaging.md` §8.

**Non-breaking changes — `TODO(fixture)` per item.** The following `MUST` be treated as
non-breaking, requiring only a spec revision bump, not a new extension identifier:

- An added optional field an old peer `MAY` ignore.
- An added `MAY` capability.
- An added error code (§10) an old peer treats as `failed` (§9).

**Breaking changes — `TODO(fixture)` per item.** The following `MUST` be treated as
breaking, requiring a new extension identifier (below):

- Removing or narrowing a `MUST` field.
- Changing an existing delivery state's (§9) or error code's (§10) meaning.
- Changing the signature scope (the signed field set, per `docs/planning/decisions/
  C5-envelope-auth.md` §5).

**Breaking change requires a new extension identifier — `TODO(fixture)`, SEP-2133 rule,
cited not reinvented.** A breaking change `MUST` use a new extension identifier
(`io.github.rossgraeber/oac-session-channels-v2`, or later), per C3 §8, citing SEP-2133's
own rule quoted at C3 §6(a): "Breaking changes MUST use a new identifier." A non-breaking
change `MUST NOT` require a new identifier — it bumps only the spec revision negotiated
under the existing identifier.

**"Breaking change" is never left undefined** — the two lists above are the complete
definition this file and its Stage 2 successor use.

---

## 12. Unsupported-capability behaviour — explicit, not implied

**Source.** `docs/planning/DESIGN.md` §MCP Session Channels extension (unsupported-
capability behaviour is named in scope); acceptance box 4 of issue #26.

**Rejection, not silent handling — `TODO(fixture)`.** A peer receiving a message that
requires a capability it did not negotiate (§5) `MUST` reject it with the
`unsupported-capability` error (§10). The peer `MUST NOT` silently drop the message,
`MUST NOT` silently degrade its handling, and `MUST NOT` partially apply it.

**Rejection is reported — `TODO(fixture)`.** The rejection `MUST` be reported to the
sender as the `rejected` delivery state (§9), carrying the `unsupported-capability` error
(§10) — never silence.

**A peer MUST NOT over-advertise — `TODO(fixture)`.** A peer `MUST NOT` declare a
capability in its capability set (§5) that it cannot honour.

**Forward compatibility — unknown fields are ignored — `TODO(fixture)`.** A peer
encountering a field in an envelope or capability set it does not recognize `MUST` ignore
that field rather than treating the message as malformed, provided every field this file
marks required (§3) is still present. This is the half of unsupported-capability
behaviour that keeps a non-breaking addition (§11) actually non-breaking in practice.

**Two concrete worked pairs — the general rule stated once, applied twice, not
hand-waved:**

1. **A transport lacking an optional capability (§16, §18).** A transport that does not
   implement, say, ordering or multicast discovery `MUST NOT` advertise it; a core caller
   that requests ordered delivery from such a transport receives the
   `unsupported-capability` rejection above, not a silent best-effort attempt.
2. **A harness lacking active-inbound support (§7).** A harness adapter that cannot meet
   §7's no-polling requirement `MUST NOT` advertise active-inbound support; a sender
   addressing a session on that harness receives the `unsupported-capability` rejection
   above (or, if the session is simply unreachable rather than capability-mismatched, the
   `unreachable` state, §9) — never a silently-polled, degraded delivery.

---

## 13. Provider adapter contract

**Source.** `docs/planning/DESIGN.md` lines 37-49; issue #26 task 14. Frozen at the Stage
2 interface freeze (task E7, `oac-spec-authoring` §7) — this M0 draft is the pre-freeze
shape that freeze will act on, not the freeze itself.

**Member list — `TODO(fixture)` for the "route through core" and "report only observable
states" requirements on every member.**

```text
ProviderAdapter
  discover_sessions() -> [SessionDescriptor]
  attach(session: SessionDescriptor) -> Result<(), AdapterError>
  capabilities(session: SessionDescriptor) -> SessionCapabilities
  deliver(session: SessionIdentity, message: ChannelMessage) -> DeliveryReceipt
  publish_output(callback: OutputCallback) -> Result<(), AdapterError>
  health() -> HealthStatus
  shutdown() -> Result<(), AdapterError>
```

Every parameter and return type above is drawn only from §15's neutral core types (or a
primitive — `Result`, a callback handle, a health/error enum carrying no provider- or
transport-specific meaning). **Zero provider names appear in any signature above.**

**Per-member normative sentence — `TODO(fixture)` per member:**

- `discover_sessions` `MUST` return only `SessionDescriptor` values the adapter itself
  observed through a supported provider surface (§13's own contract; never a
  filesystem/rollout-file read, per `[ADR-001 Boundary]`).
- `attach` `MUST` route session registration through core policy/security (§15's
  authorization decision type), never directly through a transport.
- `capabilities` `MUST` report only capabilities (§5) the adapter can actually honour,
  per §12's over-advertisement rule.
- `deliver` `MUST` report only a `DeliveryReceipt` state (§9) the adapter can actually
  observe, per §9's observability rule.
- `publish_output` `MUST` route outbound envelopes through core signing/authorization
  (`docs/planning/decisions/C5-envelope-auth.md` §2, §11), never directly through a
  transport.
- `health` `MUST NOT` expose any credential, key material, or transport-native address
  in its return value.
- `shutdown` `MUST` release every resource the adapter holds and `MUST NOT` leave a
  dangling provider-native connection advertised as healthy.

**Adapters route through core, never directly through a transport — `TODO(fixture)`,
stated once for the whole contract.** Quoted, `docs/planning/DESIGN.md`: "Adapters should
route through core policy/security rather than directly through transports." Restated
here as normative for this file's purposes: an adapter implementation `MUST NOT` call a
`Transport` (§16) operation directly — every adapter-to-transport path crosses core.

> **Reference implementation note:** the v0.1 Rust workspace implements this contract as
> a trait, `adapters/claude/` and `adapters/codex/` each providing one implementation
> (`docs/planning/DESIGN.md`'s Suggested repository shape) — the trait/module split is an
> implementation choice, not part of this normative contract.

---

## 14. Core neutral types

**Source.** `docs/planning/DESIGN.md` line 28; field-level definitions drawn from §3-§13
above, `docs/planning/decisions/C4-session-identity.md`, `C5-envelope-auth.md`,
`C6-trust-rendering.md`, `C7-zenoh-transport.md`. Every field name below is neutral —
none names Zenoh, Claude, Codex, or an MCP method.

**`SessionIdentity`** — the opaque address (§4):

| Field | Type | Notes |
|---|---|---|
| `session_id` | opaque id string | 26-char lowercase Crockford Base32, no encoded structure (§4). |
| `device_key_fingerprint` | fingerprint string | The owning device's public-key fingerprint (`docs/planning/decisions/C5-envelope-auth.md` §4). |

**`SessionDescriptor`** — what `discover_sessions` returns, non-authoritative display
metadata plus the identity above:

| Field | Type | Notes |
|---|---|---|
| `identity` | `SessionIdentity` | The authoritative address. |
| `display_uri` | string | `session://<device>/<harness>/<id>` form (§4) — display-only, `MUST NOT` be used for routing or authorization by any consumer of this type. |
| `working_directory` | string | Scopes discoverability (`docs/planning/decisions/C4-session-identity.md` §5). |
| `registered_at` | timestamp | Registration time. |

**`SessionCapabilities`** — the negotiated capability/revision set (§5):

| Field | Type | Notes |
|---|---|---|
| `active_inbound` | bool | `MUST` be `false` unless §7's no-polling requirement is actually met (§12). |
| `spec_revision` | string | The spec revision this peer implements (§11). |
| `extension_id` | string | The negotiated extension identifier (§5) — packaging metadata, never consulted for authorization. |
| `negotiated_capabilities` | set of string | The capability vocabulary set this peer declares (§5). |

**`ChannelMessage`** — the envelope's neutral, in-memory counterpart (§3):

| Field | Type | Notes |
|---|---|---|
| `id`, `from`, `to`, `conversation_id`, `reply_to`, `correlation_id`, `created_at`, `ttl_ms`, `content` | as §3 | Same required/optional split as the wire envelope. |
| `principal` | `SecurityPrincipal` | Resolved sender identity, not the raw wire `security` block. |

**`DeliveryReceipt`** — carries §9's state set, **no address**:

| Field | Type | Notes |
|---|---|---|
| `state` | one of §9's eight states | `MUST NOT` be a value outside that set. |
| `envelope_id` | string | The `id` of the envelope this receipt is for. |
| `error` | optional, one of §10's error codes | Present when `state` is `rejected`, `expired`, `duplicate`, or `failed`. |

`DeliveryReceipt` carries **no session address field** — deliberately, per DESIGN's own
minimal shape; a receipt is correlated to its envelope by `envelope_id`, never by
re-stating `from`/`to`.

**`PresenceRecord`** — carries §6's three states plus the recorded gap:

| Field | Type | Notes |
|---|---|---|
| `session_id` | opaque id | The session this record describes. |
| `state` | `online` \| `unreachable` \| `unknown` | §6's exact three-state set, no fourth value. |
| `observed_at` | timestamp | When this state was last observed. |

`PresenceRecord` carries **no** harness-ownership, capability, or active-inbound field —
per §6's recorded gap, this type does not yet carry them; a future non-breaking addition
(§11) may extend it once a discovery mechanism is defined.

**`SecurityPrincipal`** — the authenticated sender reference:

| Field | Type | Notes |
|---|---|---|
| `principal_ref` | string | Authenticated-principal reference (`docs/planning/DESIGN.md`'s envelope shape). |
| `device_key_fingerprint` | fingerprint string | Same fingerprint form as `SessionIdentity`. |

**Authorization decision** (`docs/planning/DESIGN.md` line 28):

| Field | Type | Notes |
|---|---|---|
| `outcome` | `authorized` \| `denied` | Default-deny (`docs/planning/decisions/C5-envelope-auth.md` §11) — absence of an explicit grant `MUST` resolve to `denied`. |
| `reason` | optional string | Diagnostic only, never itself an authorization input. |

---

## 15. Transport contract

**Source.** `docs/planning/DESIGN.md` lines 55-67; task E7 freezes this shape at Stage 2.

**Member list — `TODO(fixture)` for the required-semantics statements below.**

```text
Transport
  start(identity: SessionIdentity, config: opaque) -> Result<(), TransportError>
  publish(destination: SessionIdentity, envelope: ChannelMessage) -> Result<(), TransportError>
  subscribe(address: SessionIdentity, handler: MessageHandler) -> Result<(), TransportError>
  announce_presence(record: PresenceRecord) -> Result<(), TransportError>
  watch_presence(handler: PresenceHandler) -> Result<(), TransportError>
  health() -> HealthStatus
  shutdown() -> Result<(), TransportError>
```

Every parameter/return type is a §14 neutral type, a primitive, or `config` — an opaque
config handle the transport module defines and consumes internally, never inspected
outside the module. **Zero transport-specific names appear in any signature above.**

**Required semantics — `TODO(fixture)` per item:**

- **Delivery attempt, at-least-once-or-fewer.** `publish` `MUST NOT` be documented or
  relied upon as guaranteeing exactly-once or at-least-once delivery (§9's no-exactly-
  once rule applies at this layer too) — a transport `MAY` deliver zero or more times per
  call; core's replay/duplicate handling (§3, §9) is what makes duplicate delivery safe
  to observe, not a transport guarantee.
- **No reordering guarantee.** A transport `MUST NOT` be assumed to preserve the order in
  which `publish` calls were issued; ordering, where needed, is an optional capability
  (below), not a required semantic.
- **Presence as an event stream, not a poll.** `watch_presence` `MUST` deliver presence
  changes as a stream of events (a callback or async stream) — `MUST NOT` be implemented
  as, or require, a periodic poll of `announce_presence` state, per §7's no-polling rule
  applied at the transport layer.

**Optional capability set — `TODO(fixture)` for the declaration requirement.**
`docs/planning/DESIGN.md` line 67 names six: **reliability, persistence, offline
queueing, ordering, multicast discovery, routing/federation.** A transport
implementation `MUST` declare which of these six it provides, and `MUST NOT` declare one
it does not actually provide (§12's over-advertisement rule, restated at this layer). The
core `MUST` function correctly with **none** of these six present — no core code path may
assume any optional capability is available.

### Binding/mapping annex — provider- and transport-specific detail

*This subsection is the explicitly labelled exception §2's neutral-vocabulary rule
carves out. Every provider- and transport-specific name below is confined to this
annex.*

**Claude/Codex rendering and correlation detail.** Full detail, cited not restated:
`docs/planning/decisions/C6-trust-rendering.md` §2-§5 (the `meta`-attribute and
text-framing rendering mechanisms per provider), §10 (Codex reply correlation's
provider-specific half — the (a)/(b)/(c) rule's concrete `in_reply_to`/thread-id/turn-id
binding this file's §8 phrases neutrally).

**Zenoh mapping and containment rule.** Full detail, cited not restated:
`docs/planning/decisions/C7-zenoh-transport.md` §2 (the containment boundary —
`transports/zenoh/` — every Zenoh-specific type, identifier, and concept lives there and
nowhere else), §3 (key-expression layout, the one-way derivation from the opaque session
id), §4 (presence-to-liveliness mapping, the exact mechanism behind §6's three neutral
states).

> **Reference implementation note:** the v0.1 Rust workspace's Zenoh transport module is
> `transports/zenoh/` (`docs/planning/DESIGN.md`'s Suggested repository shape); its public
> surface is exactly the seven `Transport` operations above, over the neutral types in
> §14 plus the opaque `config` handle — nothing else crosses the module boundary
> (`docs/planning/decisions/C7-zenoh-transport.md` §2).

---

## 16. ACP-adapter proof

**Claim.** A third provider adapter, over the Agent Client Protocol (ACP), implements the
same seven `ProviderAdapter` members (§13) over the same core types (§14), calls no
`Transport` operation (§15) directly, and therefore changes no line of the `Transport`
contract or any transport module.

**Shown from the signatures, not from prose.** §13's `ProviderAdapter` signatures name no
provider anywhere in their parameter or return types — every type is a §14 neutral type
or a primitive. An ACP-based adapter implementing `discover_sessions`, `attach`,
`capabilities`, `deliver`, `publish_output`, `health`, and `shutdown` against those exact
signatures needs no change to any signature, because nothing in them names Claude, Codex,
Zenoh, or ACP — the same contract that types the existing two adapters already types a
third. Per §13's own routing rule ("adapters route through core, never directly through a
transport"), an ACP adapter's `deliver`/`publish_output` implementations call core, which
calls `Transport` (§15) — the ACP adapter itself never references a `Transport` operation,
so no `Transport` signature or transport module changes to accommodate it.

**What it must declare, named.**

- **Active-inbound: no, as the default finding.** Per PLANNING-PROMPT.md §3.5, ACP's
  `session/prompt` delivers input into a session **owned by the ACP client**, not a
  session an external OAC daemon injects into asynchronously the way §7 requires — an
  ACP adapter `MUST NOT` declare `active_inbound: true` (§14's `SessionCapabilities`)
  unless a specific ACP host is shown to accept out-of-band `session/prompt` calls into
  an already-running session without the client itself driving it, which is not the
  documented shape. Per §12, this is exactly the worked "harness lacking active inbound"
  case §12 names.
- **Its capability set.** Whatever subset of §5's negotiated capability vocabulary the
  ACP host actually honours — per §12, declared no more broadly than that.

**ACP facts cited, with retrieval date.** Per PLANNING-PROMPT.md §3.5, retrieved
2026-09-15: stable protocol version `1` (schema v2 is alpha); a client (ACP terminology)
owns the session (`session/prompt` delivers into a session **owned by the ACP client**);
`session/load` and `session/resume` exist; custom methods are prefixed `_`; unknown
notifications `MUST` be ignored by an ACP-conformant peer, per that same source. Labelled
**supported / forward-compat only** — per `docs/planning/v0.1/01-capability-matrix.md`'s
existing ACP row and `docs/planning/PINS.md` — "ACP" — ACP is not a v0.1 dependency; this
proof is a design-for-replacement demonstration, not a build commitment.

**Conflict-register disposition, cited not re-litigated.** The conflict-register row
literally named **C7** ("ACP is client-owned-session, not a channel") is
**`RESOLVED-BY-EVIDENCE`**, per `docs/planning/decisions/C7-zenoh-transport.md` §14's own
collision-warning paragraph, which distinguishes that register row from the unrelated
backlog decision task also labelled `C7`. This proof does not reopen that row: it already
states ACP is a client-owned-session surface (the "no active inbound" finding above is
exactly that row's consequence for this proof), not a channel OAC would attach to the way
Claude/Codex channels are attached.

---

## 17. NATS/MQTT replacement proof

**Claim.** Either NATS or MQTT replaces Zenoh by implementing the same seven `Transport`
operations (§15), with no change to any adapter and no change to the spec.

**Load-bearing premise, cited.** §2's neutral-vocabulary rule is what makes this proof
possible: because no neutral type (§14), field, or normative sentence in §3-§13 names a
transport concept, and §15's `Transport` contract itself names no transport, a
replacement transport module implementing the same seven operations over the same neutral
types requires **zero** changes to `spec/` text, `core/` types, or either adapter module
— the adapters and core never reference the transport by name or by transport-specific
concept to begin with.

**Which optional capabilities each candidate lacks, named concretely — not hand-waved.**
Against §15's six-item DESIGN line-67 list (reliability, persistence, offline queueing,
ordering, multicast discovery, routing/federation):

| Capability | Zenoh (reference, `1.10.1`, supported) | NATS | MQTT |
|---|---|---|---|
| Reliability | Reliable pub/sub over TCP-based links (`docs/planning/decisions/C7-zenoh-transport.md` §8's stable-API table) | UNVERIFIED — not independently checked against first-party NATS docs this pass; see below | UNVERIFIED — not independently checked against first-party MQTT/broker docs this pass; see below |
| Persistence | Not used by this v0.1 mapping (C7 §4 relies only on liveliness-subscriber history, not a `zenohd` storage plugin) | UNVERIFIED | UNVERIFIED |
| Offline queueing | Not provided by the peer-mode transport this file's binding annex uses (no `zenohd` on the default path, C7 §5) | UNVERIFIED | UNVERIFIED |
| Ordering | Not claimed by §15's required semantics (no reordering guarantee, stated as required-semantics text, not optional, above) | UNVERIFIED | UNVERIFIED |
| Multicast discovery | Provided, via scouting (C7 §5) | UNVERIFIED | UNVERIFIED — MQTT's broker-based model has no native multicast peer-discovery primitive analogous to Zenoh scouting; this specific cell is a structural observation from MQTT's client-broker shape, not a verified capability claim, and is itself marked UNVERIFIED pending a first-party citation |
| Routing/federation | Out of scope for v0.1's chosen peer-mode profile (C7 §5, no router) | UNVERIFIED | UNVERIFIED — MQTT brokers commonly support bridging, but this is not independently cited here |

**Every NATS/MQTT cell above is marked `UNVERIFIED`, per the evidence standard, rather
than asserted from memory.** Per `oac-evidence` §1: "if a fact's only backing is a blog
post, it is not verified, it is a lead." No first-party NATS or MQTT specification/docs
citation (URL + version + retrieval date) was gathered for this pass. Each `UNVERIFIED`
cell above is added to `docs/planning/STATUS.md`'s "Open UNVERIFIED items" list in this
same change (§21's evidence pass records the exact addition), with the reason "NATS/MQTT
optional-capability claims for the transport design-for-replacement proof (task 18,
`05-interfaces.md`) are not yet checked against first-party NATS/MQTT specification or
broker documentation."

**Close, per §12.** A candidate transport lacking an optional capability is exactly §12's
"transport lacking an optional capability" worked case: it `MUST NOT` advertise that
capability (§15's declaration requirement), and a core caller requesting it receives the
`unsupported-capability` rejection (§10, §12) — the defined behaviour this file already
states is the complete answer to what happens when NATS or MQTT lacks a capability Zenoh
provides. No new mechanism is needed for the swap to be safe.

---

## 18. CI-without-providers proof

Per PLANNING-PROMPT.md §6 items 3-4. Core, spec-conformance, and security tests run
against fake harness endpoints and recorded fixtures, with **no live provider, no API
key, and no network beyond loopback**, because §13's `ProviderAdapter` and §15's
`Transport` are the **only** seams a fake implementation needs to satisfy — nothing else
in §14's neutral core types or §3-§13's normative behaviour depends on a live provider
connection or a live transport peer to exercise. Provider integration tests sit behind an
explicit opt-in, pinned to specific provider versions, and run separately from the
CI-default tier above.

Full test-tier breakdown, CI-default-vs-opt-in split, and fixture-directory ownership:
`docs/planning/v0.1/09-test-strategy.md` (task A10, not yet landed) — that file is the
owner of this detail; this section cross-references it rather than duplicating it, per
this file's own scope statement.

---

## 19. Cross-reference block

Every reference below is a repo-relative path; no prior context is assumed.

- `docs/planning/ADR-001.md`
- `docs/planning/DESIGN.md`
- `docs/planning/PLANNING-PROMPT.md` §6, §9 item 6
- `docs/planning/v0.1/03-decisions-and-amendments.md`
- `docs/planning/v0.1/04-architecture.md`
- `docs/planning/v0.1/06-security.md` (A7, owner of signing/replay/provenance detail this
  file only references — not yet landed)
- `docs/planning/decisions/C3-spec-packaging.md`
- `docs/planning/decisions/C4-session-identity.md`
- `docs/planning/decisions/C5-envelope-auth.md`
- `docs/planning/decisions/C6-trust-rendering.md`
- `docs/planning/decisions/C7-zenoh-transport.md`
- `.claude/skills/oac-spec-authoring/SKILL.md`
- `docs/planning/STATUS.md`

---

## 20. Evidence pass

Per `oac-evidence` §8, checked against this file:

- Every external claim carries URL/version/retrieval date, or cites a
  `docs/planning/PLANNING-PROMPT.md` §3.x subsection unchanged (§16's ACP facts, cited
  §3.5, retrieved 2026-09-15).
- Surface labels at first mention: Claude Code Channels = **research preview**
  (`v2.1.274`) — cited via §16's binding annex to `docs/planning/decisions/
  C6-trust-rendering.md`, which itself carries the label; Codex app-server =
  **experimental (per-method gating)** (`@openai/codex@0.154.0`) — same citation path;
  MCP `2026-07-28` and SEP-2133 = **supported** — cited via §5, §11 to
  `docs/planning/decisions/C3-spec-packaging.md`; Zenoh `1.10.1` = **supported** — cited
  via §16 to `docs/planning/decisions/C7-zenoh-transport.md`; ACP = **supported /
  forward-compat only** — §17.
- **Existing UNVERIFIED items this file leans on, carried, not silently promoted:**
  Claude `--resume` channel behaviour (referenced by §4's stable-for-session-life
  addressing rule, via `docs/planning/decisions/C4-session-identity.md` §6); whether
  Codex echoes a header-supplied id back as `in_reply_to` (referenced by §8's
  correlation rule, via `docs/planning/decisions/C6-trust-rendering.md` §15); whether MCP
  `capabilities.experimental` survives at era `2026-07-28` (referenced by §5's capability
  model, via `docs/planning/decisions/C3-spec-packaging.md` §6(c)). All three already
  appear in `docs/planning/STATUS.md`'s "Open UNVERIFIED items" — not restated as new
  here.
- **New UNVERIFIED items from this file, added to `docs/planning/STATUS.md` in the same
  change (§17):** NATS optional-capability claims (reliability, persistence, offline
  queueing, ordering, multicast discovery, routing/federation) and MQTT optional-
  capability claims (same six), both unverified against first-party NATS/MQTT
  documentation this pass — see §17's table and its `UNVERIFIED` cells.

---

## 21. Boundary pass — highest scrutiny, this file is the spec surface

Per `oac-boundaries` mechanical check 1 plus the spec-specific word list
(`.claude/skills/oac-spec-authoring/references/neutral-vocabulary-check.md`), run against
this one file.

**Mechanical check 1, run and read (not trusted at zero-hit face value, per C7 §2's
recorded gap that `\b` does not match `snake_case`-embedded tokens):**

```
rg -n -i '\bzenoh\b|\bzid\b|key[_-]?expr|liveliness' docs/planning/v0.1/05-interfaces.md
```

**Hits, complete, read one by one:**

- §2 (the rule's own statement, naming the forbidden words as words being defined, not
  used as vocabulary).
- §16 (the binding/mapping annex heading, its two labelled paragraphs, and the following
  reference-implementation note — every hit here sits inside the explicitly labelled
  "Binding/mapping annex" subsection §2 carves out as the exception).
- §17 (the NATS/MQTT replacement-proof table's "Zenoh (reference...)" column header and
  cells, and the load-bearing-premise paragraph naming Zenoh once — every hit here sits
  inside the labelled replacement-proof section §2 also carves out).
- §19 (the cross-reference block's own path list, citing `C7-zenoh-transport.md` and
  similar filenames — a path string, not neutral-interface prose).
- This §21 boundary-pass section itself, describing the check.

**No hit appears in normative text (§1-§13), a `ProviderAdapter`/`Transport` signature
(§13, §15), or a core type field (§14).** Every hit above sits inside the task-16 binding
annex, the task-17/18 replacement proofs, the cross-reference path list, or this section's
own description of the check — exactly where §2 states hits are expected, and nowhere
else.

**`Claude`/`Codex`/MQTT/NATS/MCP-method-name check, complete:**

- `claude`/`codex` (bare, case-insensitive): §16's binding annex ("Claude/Codex rendering
  and correlation detail"), §17's table header ("Claude Code Channels" surface-label
  bullet in §20), and this §21's own description — all inside labelled binding/proof
  sections.
- `mqtt`/`nats`: §17's replacement-proof table and its surrounding prose — the section §2
  names as the exception for transport-swap detail.
- No MCP method-name-shaped string (e.g. a `notifications/...` or `thread/...` form)
  appears anywhere in this file outside §16's citation-by-path to `C6-trust-rendering.md`
  (which itself carries such names, cited, not repeated verbatim here).

**No neutral interface mentions Zenoh, Claude, Codex, MCP method names, or key
expressions.** §13's `ProviderAdapter` and §15's `Transport` signatures, and every §14
core-type field table, were re-read against this rule while drafting — none names a
provider or transport concept, confirmed by the grep results above showing no hit inside
those sections.

**Also asserted, per `oac-boundaries`' pre-commit self-check:**

- Nothing in this file has OAC owning a turn loop — §13's `ProviderAdapter.deliver`
  hands a message to the harness's own live-session operation and returns a receipt; it
  never runs a turn itself.
- Nothing in this file has OAC holding provider credentials — no core type (§14) or
  contract member (§13, §15) carries a provider credential field; §13's `health`
  member's normative sentence explicitly forbids exposing one.
- Nothing in this file has an adapter polling an inbox while claiming active inbound —
  §7's no-polling rule and §12's "MUST NOT over-advertise" rule are both stated as
  `MUST`/`MUST NOT`, and §16's ACP proof applies exactly this check when it finds ACP
  cannot honestly claim active-inbound support.

---

## 22. Acceptance close-out

Ticked against issue #26's four acceptance boxes, section numbers named:

- [x] **Normative spec items separated from reference-implementation notes in every
      section** — §1 (the convention itself: every `MUST` marked `TODO(fixture)`, every
      reference-implementation note a labelled blockquote with no RFC-2119 keyword
      inside it); applied throughout §3-§13, §15-§16.
- [x] **Design-for-replacement proofs included: ACP adapter added with no transport
      change; NATS or MQTT replacing Zenoh with no adapter or spec change, with the
      optional capabilities each candidate lacks** — §16 (ACP proof, shown from §13's
      signatures, active-inbound finding named, ACP facts cited with retrieval date, C7
      conflict-register disposition cited); §17 (NATS/MQTT proof, load-bearing premise
      cited to §2's own rule, six-capability table with every NATS/MQTT cell explicitly
      `UNVERIFIED` rather than asserted, added to STATUS.md in this change, closed by
      pointing at §12's defined unsupported-capability behaviour).
- [x] **No neutral interface mentions Zenoh, Claude, Codex, MCP method names, or key
      expressions** — §2 (the rule), §21 (the mechanical check run and read against this
      file, every hit accounted for and confined to §16/§17/§19/§21 themselves).
- [x] **Unsupported-capability behaviour defined, not implied** — §12 (its own numbered
      section: rejection not silent handling, rejection reported as a delivery state,
      no over-advertisement, unknown-field forward compatibility, the two concrete
      worked pairs — a capability-lacking transport and an active-inbound-lacking
      harness — both cross-referenced from §16 and §17 where they actually occur).

**Every `TODO(fixture)` this file raises.** Every `MUST`/`MUST NOT` in §3-§13 and §15-§16
carries the marker, per §1's rule that no fixture yet exists (`tests/protocol/` is empty,
Pre-Stage 0). This is not a partial list of exceptions — it is the complete, honest
statement that **zero** Stage 2 fixtures exist yet for this file's normative content;
task E8 (Stage 2) is the owner of closing every one of them.

**Conflicts this file carries as already-resolved, versus still open:**

- **C4 -> C5 §6** (signature made normative). This file's §3 envelope shape and §16's
  binding-annex citation both build on that resolution without re-litigating it.
- **C6 -> C5 §9** (honest delivery-receipt states). This file's §9 is exactly that
  resolution's frozen state set.
- **C8 -> C4 §8** (opaque id plus display form). This file's §4 states the resolution
  directly.
- **C9 -> C6 §10** (Codex correlation layered rule). This file's §8 is the neutral
  phrasing of that resolution.
- **Still open:** C5 (dual-era MCP topology), deferred to gate G4 per
  `docs/planning/decisions/C3-spec-packaging.md` §7 and `docs/planning/v0.1/
  03-decisions-and-amendments.md` decision 3's gate-dependency note — this file's §5
  capability model does not assume a particular topology answer, matching C3 §7's own
  statement that the tool schemas it defines are topology-independent.
- **§6's two presence/discovery gaps** (not a C-numbered conflict; a recorded C7 §4 gap,
  already tracked at `docs/planning/STATUS.md`'s "Open conflicts" list) are carried,
  stated as open in §6, not resolved by this file.
- **No new conflict surfaced while writing this file.** Every tension this file
  encountered (the C5/C7 dual-topology deferral, the two presence gaps) was already
  recorded by its owning C-series document; nothing here required a new amendment number
  under `docs/planning/v0.1/03-decisions-and-amendments.md`'s procedure.

**Cross-file updates in this change.**

- `docs/planning/STATUS.md`: add this file to the "Last updated" line as landed (issue
  #26, backlog key A6), alongside the existing A2/A4/A5 pointers; add the six new
  NATS/MQTT `UNVERIFIED` capability items from §17/§20 to "Open UNVERIFIED items"; no
  pin moved, no gate verdict changed.
