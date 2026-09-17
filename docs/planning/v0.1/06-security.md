# 06 — Security

**Source:** `docs/planning/PLANNING-PROMPT.md` §9 item 7, §7; `docs/planning/DESIGN.md`
"Security," "Addressing," "Delivery semantics"; `docs/planning/decisions/
C4-session-identity.md` (C4); `docs/planning/decisions/C5-envelope-auth.md` (C5);
`docs/planning/decisions/C6-trust-rendering.md` (C6); `docs/planning/backlog/
02-tasks-AB.json` task A7 (issue #27).

**Scope.** This file states OAC's identity model, trust boundaries, pairing,
authorization, transport security, replay/duplicate handling, per-provider provenance
rendering, and the consolidated threat table. It is a **synthesis that cites**, not a
copy: C4, C5, and C6 already hold the full evidence, rejected alternatives, and
per-decision threat rows; this file merges the C4 §13 / C5 §13 / C6 §12 tables into one
§14-complete table, deduplicating overlapping rows, and points at each decision's
section for the reasoning behind every mitigation stated here. It does not define the
normative spec surface (`docs/planning/v0.1/05-interfaces.md`'s job, landed, A6) or the
test-tier breakdown (`docs/planning/v0.1/09-test-strategy.md`'s job, A10, not yet
landed) — it cross-references each by path.

**Proof caveat, stated once, applying to every section below.** Per
`docs/planning/STATUS.md`'s Gate verdicts table, **every gate verdict (G1-G5) is
`NOT RUN`**, and per `docs/planning/STATUS.md`'s "Current stage" table the project is at
**Pre-Stage 0** — no F/G/H test tier is built. This is the identical caveat C5's and C6's
opening sections state for their own threat rows. Every mitigation described below is
**designed**, not **proven**. No sentence in this file asserts that a signature verifies
correctly in practice, that provenance renders distinctly on a live provider, or that any
other mechanism here has been exercised end to end — each such claim is a design this
project has committed to and named a proving test for (§14), not a result. This document
cites C4/C5/C6; it does not re-derive their evidence or their rejected alternatives.

---

## 1. Identities

The four-layer identity model, from C4 §1: only two of the four layers carry authority.

1. **Device key** — the cryptographic root, one Ed25519 keypair per device (C5 §4),
   held in the OS credential store or its encrypted-file fallback (C4 §10-§11).
2. **Opaque stable OAC session id** — a random, non-derivable identifier bound to the
   device key by a signed registration record (C4 §2, §5); the address OAC's own
   protocol carries on the wire.
3. **Display URI** (`session://<device>/<harness>/<id>`) — for humans only; never
   parsed for authorization, never a wire field, never a routing key (C4 §8).
4. **Human alias** — a local, mutable, per-device label; never transmitted as an
   identity claim (C4 §9).

Only (1) the device key and (2) the opaque session id carry authority. The identity
hierarchy, quoted from `docs/planning/ADR-001.md` "Security model":

```
Security principal -> Device -> Harness -> Session
```

The device key is Ed25519, one keypair per device; sessions hold no keys of their own —
a session's authenticity derives entirely from the device key signing its registration
record (C5 §4). Construction, capture path per harness, resume behaviour, and key
storage detail are C4's, cited here, not re-derived: C4 §2 (opaque id construction), §3
(Claude `session_id` capture), §4 (Codex `thread.id` capture), §6-§7 (resume behaviour
per harness), §10-§11 (key storage: `keyring` `4.2.0` plus `age` `0.12.1` encrypted-file
fallback).

## 2. Model-generated text never establishes identity

An alias or a sender claim appearing in a message's **content** is content, never
provenance. Quoted, C4 §9: "an alias appearing in message *content* is content, not
provenance... the authenticated-but-untrusted doctrine applies without exception." A
model-supplied `in_reply_to` value is likewise untrusted content: it is validated against
independent adapter-side state — the daemon's own thread-id/turn-id binding — and never
trusted on its face (C6 §10). A mismatch between a model-claimed `in_reply_to` and that
independent state **downgrades** the correlation (to "inferred" or "uncorrelated"); it
never overrides the independent state with the model's claim (C6 §10).

This is the same doctrine `oac-security-work` §3 states for authorization, applied here
to rendering and identity: **authentication answers "who sent it." It never answers
"should this be obeyed."** A verified envelope signature plus a matched allowlist entry
authorizes **delivery into the addressed session's input**, and nothing else — never an
action the message's content requests, and never treating the content's own claims about
who sent it as if they were the machine-set provenance (§8-§10 below).

## 3. Trust boundaries

Each boundary crossing in the process topology (`docs/planning/v0.1/04-architecture.md`
§5's process-boundary diagram, cited by path, not redrawn here), with what is
authenticated at it and what is not:

- **(a) Harness process <-> shim.** In-process (the shim is spawned by the harness); no
  OAC-level authentication crosses this boundary — the harness's own process boundary is
  the only isolation.
- **(b) Shim <-> daemon, local IPC.** OS peer authentication: Windows named-pipe
  security descriptors (`SECURITY_ATTRIBUTES.lpSecurityDescriptor`,
  `GetNamedPipeClientProcessId`) or Unix socket permissions plus
  `SO_PEERCRED`/`getpeereid()` (C2 §4, restated in full at §12 below). This proves the
  connecting process belongs to the same OS user as the daemon; it proves nothing about
  which OAC session or harness that process claims to be.
- **(c) Daemon <-> daemon, over Zenoh.** Transport TLS/QUIC plus an ACL pre-filter keyed
  on authenticated certificate common name or username — **no payload authentication**
  (C5 §12, §6 below).
- **(d) Envelope layer.** Ed25519 signature (C5 §2-§5) — **the only authenticity proof**
  for a message's actual content and claimed sender; every other boundary above is
  either in-process, OS-peer, or transport-level, none of which authenticates the
  envelope's payload.
- **(e) Model-facing rendering boundary.** Provenance (machine-set metadata) versus
  content (untrusted body) — §8-§10 below; this is the boundary that keeps a verified
  envelope's sender claim visibly distinct from whatever the message body itself claims.

## 4. Pairing

Two flows, from C5 §10:

- **Same-user multi-harness on one device — zero-config.** Falls out of (b)'s local-IPC
  peer authentication: both harnesses' shim processes connect to the same daemon as the
  same OS user, so no separate identity or exchanged secret is needed (C5 §10(a)). This
  does not touch, weaken, or pre-answer the Claude-side
  `--dangerously-load-development-channels` confirmation, which remains a separate,
  provider-side consent step (C5 §10(a)).
- **Two devices on a LAN — short numeric code.** 6 decimal digits, a 120-second window,
  a 5-attempt limit, binding the two devices' **exchanged device-public-key
  fingerprints** — a truncated SHA-256 hash with a fixed **>= 128-bit** floor (C5 §10(b)).
  MITM defeat: the code binds to the fingerprint of the *exchanged key*, not the network
  path, so a substituted key produces a different fingerprint-derived code the person
  confirming on the other device would see does not match (C5 §10(b)).

**Pairing seeds allowlist entries; it is not itself a per-session grant.** Completed
pairing establishes that two devices (or two local harnesses under the same daemon)
trust each other's key — it does not, by itself, grant blanket access from every session
on one device to every session on the other. The per-session allowlist entry (§5 below)
is the separate, explicit authorization step pairing feeds but does not substitute for
(C5 §11).

## 5. Authorization

**Default-deny is the standing posture.** Quoted, `docs/planning/DESIGN.md`, restated by
`oac-security-work` §4: every new routing rule, ACL subject, or authorization path starts
denied and is opened only by an explicit grant (C5 §11).

**Granularity: per-OAC-session-id sender allowlists, scoped by `working_directory`.** An
allowlist entry names a specific addressed OAC session id, not "any session on this
paired device" and not "any session for this harness." The session's `working_directory`
field, recorded at registration (C4 §5), is what a discovery or allowlist check filters
on before a grant applies (C5 §11).

**Revocation and eviction.** An allowlist entry is removed (a) explicitly, by an operator
action, or (b) implicitly, when the session id it names is deregistered — an entry
naming a session with no live registration record is inert even before explicit
deletion. Revoking a *pairing* cascades to remove every allowlist entry that pairing had
seeded (C5 §11). **A new session gets a new opaque id and inherits no prior grant** — a
fresh session, even from the same harness and working directory, requires its own
allowlist entry; a prior session's grant does not carry forward automatically (C5 §11,
C4 §5).

**Two separate checks, restated here, detailed at §7 and §8 below.** A verified
signature plus a matched allowlist entry authorizes delivery only. Whether that
delivered content may then steer an in-flight Codex turn (`turn/steer`) is a second,
separate authorization decision (§12, C5 §11's "message-delivery authorization and
steer-authorization are two separate checks by design"). Whether a Claude sender may
approve a tool call via permission relay is likewise a second, separate decision, off by
default in v0.1 (§11 below).

## 6. Transport security

TLS/mTLS and QUIC listeners are used for LAN mode; local mode binds loopback and leaves
multicast scouting on by default, disablable, with no manual certificate management
required locally (`docs/planning/DESIGN.md` "Security"; C7 §5).

**The one-sentence relationship, stated so it cannot be misread, quoted from C5 §12:**
**the Zenoh ACL is a coarse pre-filter, the envelope signature is the authenticity
proof, and Zenoh performs no payload authentication of its own.** A Zenoh ACL rule
permitting a given subject to publish or subscribe on a given key expression stops
obviously-unauthorized traffic from reaching OAC's own verification code; it never
substitutes for the envelope signature check that is the actual authenticity proof for
any envelope that does reach that code (C5 §12).

**ACL subjects are drawn only from certificate common name or username — never `zid`.**
Quoted, C5 §12 (citing `oac-zenoh` §5, §2): `zid` "is not an identity... explicitly
unauthenticated and unfit for production." No OAC allowlist entry, pairing record, or
ACL rule is ever keyed on a Zenoh `zid` (C5 §12). Where pairing issues LAN certificates,
the certificate's common name is derived from the same device-public-key fingerprint the
pairing flow (§4 above) already computes (C5 §12).

Zenoh vocabulary is confined to this section and to the transport-layer threat rows in
§14; the neutral surface stays clean of it, per `[ADR-001 Boundary]` "MUST NOT leak
Zenoh-specific concepts into the neutral protocol" — the same containment rule C5 §12
and `docs/planning/v0.1/05-interfaces.md` §2, §21 already apply.

## 7. Replay and duplicate handling

From C5 §7-§9:

- **`created_at` accept-window: +/-300 seconds**, equal to the envelope's own default
  `ttl_ms` (300000ms). Clock skew is folded into this window (the `+300s` half), not
  layered on as a separate tolerance (C5 §7).
- **Nonce: 128-bit CSPRNG value**, fresh per envelope, never reused across retries
  (C5 §7).
- **Dedup key: `(security.key_id, security.nonce)` — not `id` alone.** An envelope's
  `id` is sender-chosen and not required to be globally unique; only the `(key_id,
  nonce)` pair, both CSPRNG-random and inside the signed field set, is the
  collision-resistant dedup key (C5 §7).
- **Store: in-memory, per-device, daemon-held**, bounded by the replay window (roughly a
  600-second worst-case retention per entry), time-evicted, not a growing log (C5 §8).
- **Cold-restart gap, stated honestly.** A daemon restart empties the store. For the
  interval immediately following a restart, up to the accept-window's own duration, the
  window itself — not the dedup store — is the only defence against a replay of an
  envelope whose `created_at` still falls inside that window. This is a named, accepted
  residual risk, not a defence this design claims to close (C5 §8).
- **Exactly-once is not promised.** Quoted, `docs/planning/DESIGN.md` "Delivery
  semantics": "Do not promise exactly-once delivery. Use unique IDs, idempotency, and
  duplicate suppression." The dedup store suppresses observed duplicates; it cannot
  guarantee a receiver never sees one under every failure mode (C5 §8).
- **This is not a durable offline mailbox.** Per `oac-boundaries` boundary 11: the store
  holds no message content, is bounded to a rolling ~10-minute window, and exists only
  to detect replay — it is not a queue that lets a message wait for an offline receiver
  (C5 §8).
- **Rejections map onto DESIGN's existing delivery states.** An envelope outside the
  accept-window is rejected as `expired`; a duplicate `(key_id, nonce)` pair is rejected
  as `duplicate` — both from DESIGN's own state list (`accepted`, `rejected`,
  `unreachable`, `expired`, `duplicate`, `failed`). No new delivery state is invented
  for replay handling (C5 §7).

## 8. Provenance rendering per provider — Claude

**Surface label: research preview**, pinned `v2.1.274` (C4 §16, C6 §15).

A channel is an MCP server declaring `capabilities.experimental["claude/channel"] = {}`
and sending `notifications/claude/channel` with `content` (string) and `meta`
(string-to-string map) as separate wire fields (C6 §2, §4). Five identifier-safe `meta`
keys carry provenance, pattern `^[A-Za-z0-9_]+$`: `oac_sender`, `oac_device`,
`oac_session`, `oac_message_id`, `oac_reply_to` (C6 §2). All five are **ASCII constants,
declared once in the module that constructs outbound `meta` maps, never derived from
message content and never derived from a peer-supplied string** (C6 §2-§3).

**The silent-drop hazard, stated verbatim.** A `meta` key that is not identifier-safe is
**silently dropped** — no error, no warning, the attribute just does not appear (C6 §2-§3,
citing `oac-claude-channels` §2). Left unmitigated this is a silent downgrade from
"provenance present" to "provenance missing."

**OAC's fail-closed response, not left to hope.** A const key table (the five keys
above, never string-interpolated); a contract test asserting every emitted key matches
`^[A-Za-z0-9_]+$`; and a refusal fixture that feeds a deliberately non-conforming key and
asserts the adapter refuses to send rather than deliver an under-labelled message (C6
§3). Protocol mechanics beyond this — the `<channel>` tag, negotiation, no-attach, no-ack
— are deferred to `oac-claude-channels`.

## 9. Provenance rendering per provider — Codex

**Surface label: experimental, per-method gating via `capabilities.experimentalApi`**,
pinned `@openai/codex@0.154.0` (C4 §16, C6 §15).

Codex has no side-channel metadata field — provenance rides inside the one
`{type:"text",text}` item's `text` string, in three parts: a machine-generated header
block carrying the same five field names as §8 (`oac_sender`, `oac_device`,
`oac_session`, `oac_message_id`, `oac_reply_to`); a **receiver-generated,
sender-unpredictable 128-bit delimiter**, opening and closing the untrusted body
section; then the untrusted body (C6 §5).

**Why a sender-derived or static delimiter fails, stated explicitly.** The threat actor
is the *sending* peer. A delimiter derived from the envelope's own signed nonce, or any
other sender-computable value, is computable by that same sender before it signs — the
signature verifying proves nothing about whether the sender also pre-planted a forged
header or an early fence-close inside the body using a delimiter it could predict. A
fixed static delimiter fails for the identical reason. The delimiter must instead be
generated by the *receiving* adapter, from its own CSPRNG, after the signed envelope has
already arrived — a value the sender never sees or computes, closing this gap
(C6 §5, citing its own corrected derivation over an earlier, rejected sender-nonce-derived
design).

**The inbound path uses only `turn/start` and `thread/queue/add` — never
`turn/steer`.** Per the decision-rule table (C6 §5): `turn/start` for an idle thread,
`thread/queue/add` when a turn may be in flight and the input should queue until idle,
`turn/steer` only for appending into an *actively in-flight* turn. Ordinary inbound
message delivery never routes through `turn/steer`; any future routing that would needs
task G7's own separate authorization gate (C5 §11, C6 §5).

## 10. What "visibly distinct" means — one rule, both providers

One rule, satisfied by both renderings above even though the carrier differs:
provenance is **machine-set, structurally separate from the content field, non-derivable
from or overridable by content, and identical in field set across providers** (C6 §6).

Ties to `docs/planning/ADR-001.md` "Security model": "Provenance must remain
machine-enforced and distinct from message content." Ties to G5's pass criterion, quoted
verbatim, PLANNING-PROMPT.md §4: "A message whose text claims a different sender is
rendered to the model with machine-set provenance that contradicts the claim, on both
providers." **G5's verdict is `NOT RUN`** (`docs/planning/STATUS.md`); this section
states the design G5 will exercise, not a proven result (C6 §6).

## 11. Permission relay off by default in v0.1, with justification

**OAC does not declare `capabilities.experimental['claude/channel/permission']` in
v0.1.** Three justification strands, from C6 §7:

- **(a) The C10 finding.** Any allowlisted sender could approve or deny tool use if
  permission relay were on — an allowlist entry authorizes a sender's *message* to be
  delivered, and turning relay on would let that same authorization silently extend to
  approving *tool calls*, collapsing two decisions §5 above deliberately keeps separate.
- **(b) The authenticated-but-untrusted doctrine.** A verified signature and a matched
  allowlist entry authorize delivery into the addressed session's input, never an
  action the message's content requests (§2, §5 above). Permission relay, enabled,
  would make "who sent it, verified" stand in for "this tool call is approved" — exactly
  the collapse the doctrine forbids.
- **(c) `--dangerously-load-development-channels` must not be diluted.** This flag is
  the only user consent step on the Claude side until OAC is on an allowlist. Leaving
  permission relay off by default keeps that flag's confirmation as the one and only
  consent step a user grants when loading OAC as a development channel — an on-by-default
  relay would add a second, silent grant (tool-approval authority for any allowlisted
  sender) behind the same single confirmation.

**A chosen default, not an availability accident.** The pinned version (`v2.1.274`)
satisfies the `>= v2.1.234` floor for `claude/channel/permission` — permission relay is
*available* at the pinned version. This document's off-by-default posture is a decision
made in spite of availability, not a limitation imposed by it (C6 §7).

**Enabling it requires its own decision record.** If a deployment turns relay on, "any
allowlisted sender for that session becomes able to approve tool use" — H2's own
consequence wording. A future change to enable it is a separate, explicit decision, not
a configuration flag flipped inside this file's scope and not a side effect of any
allowlist grant (C6 §7).

## 12. Local IPC peer authentication

Named mechanisms, verbatim, per C2 §4 and restated by C5 §10(a):

- **Windows:** named-pipe security descriptors —
  `SECURITY_ATTRIBUTES.lpSecurityDescriptor`, `GetNamedPipeClientProcessId`.
- **Unix:** socket permissions plus `SO_PEERCRED`/`getpeereid()`.

Both mechanisms assert the connecting peer's UID equals the daemon's own UID — this is
what makes same-device multi-harness pairing zero-config (§4 above): the OS itself
authenticates the connecting process before any OAC-level pairing step could run (C5
§10(a)).

**Inherited UNVERIFIED item.** Named-pipe DACL peer-authentication behaviour has not
been exercised on a live Windows host — API shape verified against Microsoft Learn only
(`docs/planning/STATUS.md` "Open UNVERIFIED items"; C2 §4, §10-§11; C5 §13, §16). Owner:
**G9**, the local IPC peer-auth verification task.

## 13. Cross-project leakage

`working_directory`, recorded in the registration record at registration time (C4 §5),
scopes discovery and allowlist matching. `list_sessions` returns only sessions the
caller is authorized to see — filtered by the same `working_directory`-scoped,
default-deny allowlist §5 above fixes — **never an unfiltered directory of every session
OAC knows about** (C6 §8, §12).

Stated verbatim, per `oac-security-work` §2's checked item: **a session registered from
one working directory must not be discoverable by a peer that is not authorized for
it.** Owner: **H2**'s fourth acceptance item (cross-project leakage).

## 14. Threat table

Per `oac-security-work` §1's template, all five columns, no blank "proving test."
Merged from C4 §13, C5 §13, and C6 §12, deduplicated, with the DESIGN "Security" list
and every PLANNING-PROMPT.md §7 addition covered. Each row cites the decision section it
derives from.

| # | Attack | Precondition | Mitigation | Proving test | Residual risk |
|---|---|---|---|---|---|
| 1 | Impersonation (forged envelope) | Attacker constructs an envelope claiming a `from`/`security.principal` it does not control | Ed25519 signature over the full signed field set including `from` and `security.principal`; `verify_strict` rejects malformed/small-order keys (C5 §2, §5) | F11 security suite; H2 | F11/H2 `NOT RUN`; designed, not proven |
| 2 | Unauthorized routing/discovery | Peer not on a session's allowlist attempts delivery or discovery | Default-deny; `working_directory`-scoped, per-session-id allowlist entries seeded only by completed pairing (C5 §10-§11) | F5; H2 | F5/H2 `NOT RUN`; allowlist-check implementation not yet built |
| 3 | Tampering (field mutation in flight) | Attacker on the transport path rewrites envelope bytes | Every signed field is inside the JCS-canonicalized, domain-separated signed scope; any mutation invalidates the signature (C5 §3, §5) | F11; F4 | F11/F4 `NOT RUN`; only `security.signature` itself is unsigned, by construction, and that is expected |
| 4 | Replay | Attacker captures a validly-signed envelope and re-sends it later | `created_at` accept-window (+/-300s) plus `(key_id, nonce)` dedup against the in-memory duplicate-suppression store (C5 §7-§8) | F4 | F4 `NOT RUN`; cold-restart gap (§7 above) is a named, accepted residual risk even once F4 passes |
| 5 | Malicious peer prompt injection despite valid signature | Attacker is a validly-paired, allowlisted peer whose message content is itself adversarial | None at the envelope-authenticity layer, by design — mitigation is doctrine plus rendering (§2, §8-§10 above), not envelope authentication (C5 §13; `oac-security-work` §3) | G5 (provenance rendering keeps the doctrine visible to the model); F11 | G5 `NOT RUN`; this is the doctrine's own stated limit, not a gap this document claims to close |
| 6 | Compromised transport infrastructure | A Zenoh router or peer on the path is compromised or malicious | Envelope signature verified independent of transport state; Zenoh performs no payload authentication, so a compromised path cannot forge or silently tamper with an envelope that still verifies (C5 §12-§13; §6 above) | F11; D3/G3 | F11 `NOT RUN`; drop/delay/duplicate availability risk from a compromised path is not eliminated by signature verification, only forgery risk is |
| 7 | Accidental cross-project disclosure | Two sessions exist under different `working_directory` values | `working_directory` field scopes the registration record and every discovery/allowlist check before a grant applies (C4 §5; C5 §11; C6 §8; §5, §13 above) | H2 (fourth acceptance item) | H2 `NOT RUN`; query/grant logic not yet built |
| 8 | Leaked credentials / device-key exfiltration (OS store and `age` fallback file) | Attacker gains filesystem read access to the credential store or the `age`-encrypted fallback file | OS credential store per platform, or `age`-encrypted file with directory/file permissions as a second layer; no v0.1 automatic rotation, remediation is manual re-pair (C4 §10-§11, §13; C5 §4, §13) | F5; F11 | F5/F11 `NOT RUN`; a leaked key remains valid for every peer that has not yet manually re-paired until the operator notices — a stated v0.1 scope limit |
| 9 | Zenoh has no payload authentication (transport-only authenticity assumed) | An attacker relies on transport security alone to forge a sender claim | Envelope signature is the sole authenticity proof; Zenoh ACL is a coarse pre-filter only (C5 §12; §6 above) | F11 | F11 `NOT RUN`; designed, not proven |
| 10 | Zenoh `zid` used as an identity/ACL subject | An implementation mistakenly keys policy on `zid` | ACL subjects are drawn only from authenticated certificate common name or username, never `zid` (C5 §12; §6 above) | G3 ("ACL subjects are authenticated ones only, never `zid`") | G3 `NOT RUN`; enforced by design and by the rule stated here, not yet verified against a running transport |
| 11 | Permission-relay abuse (`claude/channel/permission`) | Permission relay is enabled for a deployment | Off by default in v0.1; enabling it is its own explicit decision, never a side effect of allowlisting (C6 §7, §12; §11 above) | G4 ("Permission relay is off by default"); H2 ("confirmed off... documented") | G4/H2 `NOT RUN`; a deployment that opts in accepts the documented consequence, an accepted named risk for that deployment |
| 12 | Unauthorized Codex `turn/steer` (code-execution risk) | Attacker's message is delivered (passes signature and allowlist checks) to a Codex session | Message-delivery authorization and steer-authorization are two separate checks by design; inbound framing never routes through `turn/steer` (C5 §11, §13; C6 §5, §12; §9 above) | F11; G7; G2 | F11/G7/G2 `NOT RUN`; the separate steer-gate this row relies on is not built yet |
| 13 | Local IPC peer spoofing | An attacker-controlled local process attempts to connect to the daemon's IPC endpoint pretending to be a legitimate shim | OS-level peer authentication — named-pipe DACL / `GetNamedPipeClientProcessId` (Windows), `SO_PEERCRED`/`getpeereid()` (Unix) — peer UID must equal the daemon's own UID (C2 §4; C5 §10(a), §13; §12 above) | F11; G9 | F11/G9 `NOT RUN`; named-pipe DACL behaviour not yet exercised on a live Windows host (§12 above) |
| 14 | Cross-project leakage via `list_sessions` | A caller invokes `list_sessions` while sessions exist under multiple `working_directory` values | Result filtered by the same `working_directory`-scoped, default-deny allowlist (C6 §8, §12; §13 above) | H2 (fourth acceptance item) | H2 `NOT RUN`; same open item C4/C5's own tables already name for this threat class |
| 15 | Silently dropped `meta` key yielding unlabelled provenance | A bug emits a non-identifier-safe Claude `meta` key | Const key table; incomplete provenance detected pre-send; message refused, not delivered unlabelled (C6 §3, §12; §8 above) | Contract test and refusal fixture, built by **F8/G4** | Neither test exists yet; carried as an open item, not a closed mitigation |
| 16 | Provenance spoofing via message body, Claude | Attacker controls a validly-signed envelope's `content` text and crafts it to look like a `<channel>` tag or forged `meta`-shaped claim | `content`/`meta` are separate wire fields; `meta` is machine-set from daemon state, never content-derived (C6 §2, §4, §12; §8, §10 above) | G1 (Claude wake, exercises real rendering); G5 | G1/G5 `NOT RUN`; designed, not proven |
| 17 | Provenance spoofing via forged header/delimiter, Codex | Attacker crafts a message body shaped like the header block or delimiter, hoping a reader treats it as the real one | Delimiter generated by the receiving adapter from its own CSPRNG at delivery time, never derivable from any sender-controlled field; body scanned for collision before framing (C6 §5, §12; §9 above) | G2 (Codex live inject); G5 | G2/G5 `NOT RUN`; residual risk only if a future implementation ever exposes the delimiter to sender-side code before it is used, which this design does not do |
| 18 | Reply misattribution via forged `in_reply_to` | Attacker's message content instructs the model to claim an incorrect `in_reply_to` value | (a) explicit `in_reply_to` is never trusted alone; validated against (b) the adapter's independently-tracked thread-id/turn-id binding; a mismatch downgrades to inferred/uncorrelated, never silently accepted (C6 §10, §12; §2, §9 above) | G8 (Codex adapter reply-correlation) | G8 `NOT RUN`; whether Codex reliably echoes a header-supplied id back at all is itself UNVERIFIED (`docs/planning/STATUS.md`) |
| 19 | Stale-registration replay after resume | A held-open registration record is presented after the harness session it named has ended or been superseded | Session lifetime tied to the IPC connection's life; Claude resumes default to a new registration; Codex re-binds only through the daemon's own authoritative client observation (C4 §5-§7, §13) | F4; H2 | F4/H2 `NOT RUN`; the Codex cross-process silent-append hazard (issue #21743) remains invisible to OAC by construction, not mitigated by a test |
| 20 | Session-id spoofing | Attacker constructs or guesses a 128-bit opaque id and presents it as its own | Ids are CSPRNG-random, never derivable; a session id alone is never an authorization credential — the registration record's device-key signature is checked, not the id string (C4 §2, §5, §13) | F11; H2 | F11/H2 `NOT RUN`; designed, not proven |

## 15. Unproven-mitigation disposition

Per `oac-security-work` §1: **a mitigation with no proving test is not a mitigation.**
Every row in §14 names a real proving test — a named test tier (F4, F5, F8, F11), gate
(G1-G5), or backlog task (G2, G4, G7, G8, G9, H2) — and every one of those named tests
currently has a verdict of `NOT RUN`, or the task that builds it does not exist yet
(`docs/planning/STATUS.md`, Pre-Stage 0). No row's "proving test" cell is blank or "TBD";
none was invented to fill the column. Because none has run, every row above describes a
**designed** mitigation, not a **proven** one. Each row is carried forward as an open
item for `docs/planning/v0.1/11-risks.md` (task A12, not yet written) rather than
presented as a closed mitigation — A12 inherits this file's §14 table in full when it is
written.

## 16. Cross-reference block

Every reference below is a repo-relative path; no prior context is assumed.

- `docs/planning/ADR-001.md`
- `docs/planning/ADR-001-AMENDMENTS.md` (C4, C6, C8, C9, C10 all `RESOLVED-IN-DECISION`)
- `docs/planning/DESIGN.md`
- `docs/planning/PLANNING-PROMPT.md` §7, §9 item 7
- `docs/planning/decisions/C2-process-model.md`
- `docs/planning/decisions/C4-session-identity.md`
- `docs/planning/decisions/C5-envelope-auth.md`
- `docs/planning/decisions/C6-trust-rendering.md`
- `docs/planning/PINS.md`
- `docs/planning/STATUS.md`
- `docs/planning/v0.1/03-decisions-and-amendments.md`
- `docs/planning/v0.1/04-architecture.md`
- `docs/planning/v0.1/05-interfaces.md`
- `docs/planning/v0.1/09-test-strategy.md` (A10, not yet landed — future test-tier detail
  for the proving tests named in §14)
- `docs/planning/v0.1/11-risks.md` (A12, not yet landed — inherits §14/§15's disposition)
- `docs/planning/v0.1/12-deferred.md` (not yet landed — v0.1 exclusions this file assumes,
  e.g. full E2E encryption, online key rotation)
- `spec/security.md` (Epic E, `oac-spec-authoring` — normative landing site for the
  signing/replay/authorization language this file states in decision-document prose)

## 17. Evidence pass

Per `oac-evidence` §8, checked against this file:

- Every provider surface is labelled once at first mention, with its pin: Claude Code
  Channels = **research preview** (`v2.1.274`, §8); Codex app-server = **experimental
  (per-method gating via `capabilities.experimentalApi`)** (`@openai/codex@0.154.0`, §9).
- Method names, capability keys, and flags are quoted verbatim wherever used:
  `notifications/claude/channel` (§8); `claude/channel/permission` (§11); `turn/start`,
  `thread/queue/add`, `turn/steer` (§9, §14 row 12); `--dangerously-load-development-
  channels` (§4, §11); `zid` (§6, §14 row 10); `capabilities.experimentalApi` (§9).
- Facts unchanged from C4/C5/C6 and the DESIGN/ADR-001 baseline are cited by section
  throughout, not re-derived; no new fact is asserted here without a C4/C5/C6 or
  DESIGN/ADR-001 citation.
- **No new UNVERIFIED item is introduced by this file.** Every UNVERIFIED item this file
  relies on — named-pipe DACL live-host behaviour (§12), whether Codex echoes a
  header-supplied id back reliably (§9, §14 row 18), the exact byte-truncation length
  above C5 §10(b)'s 128-bit floor (§4), the 6-digit/120s/5-attempt pairing parameters'
  live-network behaviour (§4) — already appears in `docs/planning/STATUS.md`'s "Open
  UNVERIFIED items" list, added there by C4/C5/C6; this file only inherits and cites
  them.
- Gate verdicts (all `NOT RUN`) are cited from `docs/planning/STATUS.md`, not restated
  from memory, at every point where a claim's proof status matters (opening caveat, §10,
  §15).

## 18. Boundary pass

Per `oac-boundaries`' pre-commit self-check, confirmed for this file:

- **No provider model API substitution.** Nothing in this file has OAC calling a
  provider's model API in place of the native harness; §8-§9 describe delivery framing
  only, never inference.
- **No provider credential read.** `CODEX_HOME/auth.json`, any Codex OS-keyring login
  entry, and any Claude Code OAuth token are explicitly out of bounds — this file's key
  storage discussion (§1) covers only OAC-issued device keys (C4 §12's credential
  boundary self-check, cited, not re-derived).
- **No scraping, private RPC, or rollout-file path.** §8-§9's provenance rendering uses
  only the documented `notifications/claude/channel` notification and the documented
  `{type:"text",text}` turn-input item — no UI/terminal scraping, no undocumented RPC,
  no `CODEX_HOME/sessions/` rollout-file read.
- **No Zenoh vocabulary in the neutral surface.** Zenoh is named only in §6's transport
  mapping and the transport-layer rows of §14 (rows 6, 9, 10) — no other section names
  `zid`, a key expression, or a liveliness term.
- **`--dangerously-load-development-channels` is not weakened, automated past, or
  diluted.** §4 and §11 both state explicitly that nothing here touches, pre-answers, or
  adds a second silent grant behind that confirmation.
- **No durable offline mailbox introduced by the dedup store.** §7 states the store's
  bound (a rolling ~10-minute window) and explicitly disclaims durability, citing
  `oac-boundaries` boundary 11.
- **No polling by an adapter claiming active inbound.** Nothing in this file proposes an
  adapter polling an inbox while advertising active-inbound support — provenance
  rendering (§8-§9) describes framing of a delivered message, not a delivery mechanism.

## 19. Acceptance close-out

Checked against issue #27's four acceptance boxes, section numbers named:

- [x] **Threat table rows: attack, precondition, mitigation, which test proves it,
      residual risk** — §14 (twenty rows, all five columns, merged from C4 §13 / C5 §13
      / C6 §12, no blank proving-test cell), §15 (the unproven-mitigation disposition
      rule applied to every row).
- [x] **Covers the DESIGN threat list plus: Zenoh has no payload authentication; Zenoh
      `zid` is not an identity; Claude permission relay lets any allowlisted sender
      approve tools; unauthorized Codex `turn/steer` is a code-execution risk; local IPC
      peer authentication; cross-project leakage** — §14 rows 1-3, 5-8 (DESIGN list:
      impersonation, unauthorized routing/discovery, tampering, malicious peer prompt
      injection, compromised transport infrastructure, accidental cross-project
      disclosure, leaked credentials), row 4 (replay), row 9 (Zenoh no payload
      authentication), row 10 (Zenoh `zid`), row 11 (permission-relay abuse), row 12
      (unauthorized `turn/steer`), row 13 (local IPC peer spoofing), row 14 (cross-project
      leakage via `list_sessions`); §6 (transport security section stating the Zenoh
      rules in prose), §12 (local IPC peer authentication section), §13 (cross-project
      leakage section).
- [x] **States that model-generated text never establishes identity** — §2 (its own
      named section, not a bullet: alias-in-content and model-supplied `in_reply_to`
      both stated as untrusted, the authenticated-but-untrusted doctrine quoted and
      applied).
- [x] **Permission relay off by default in v0.1, with justification** — §11 (the three
      justification strands, the chosen-default-not-availability-accident statement, the
      separate-decision-record requirement).

---

## 20. Cross-file updates in this change

- `docs/planning/STATUS.md`: "Last updated" bumped; an A7 line added noting
  `docs/planning/v0.1/06-security.md` landed (issue #27), synthesizing C4/C5/C6 into the
  merged §14 threat table, no pin moved, no gate verdict changed, no new UNVERIFIED item
  added.
- No conflict-register row changes are needed: C4, C6, C8, C9, and C10 are already
  `RESOLVED-IN-DECISION` per `docs/planning/ADR-001-AMENDMENTS.md`, and this file
  surfaced no new conflict requiring a numbered `ADR-001-A4` amendment.
