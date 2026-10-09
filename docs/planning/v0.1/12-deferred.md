# 12 — Deferred

Self-contained per `.claude/skills/oac-planning-package/SKILL.md` §2: every
citation below is a repo-relative path plus section. This file has three parts,
each a different kind of "not in v0.1":

1. **§1 — v0.1 exclusions** (`docs/planning/ADR-001.md` line 63,
   `docs/planning/DESIGN.md` line 15's "production adapters" element): scoped
   out of this milestone, reconsiderable in a later one under a stated
   condition.
2. **§2 — boundary, not backlog** (`docs/planning/ADR-001.md` line 24's
   boundary, `docs/planning/DESIGN.md` line 15's non-goals): permanently out of
   scope for OAC as a project, never a v0.2 candidate.
3. **§3 — package-level exclusions decided elsewhere**: narrower exclusions
   already decided in specific v0.1 package files, gathered here as the single
   deferred-items index.

## 1. v0.1 exclusions — "not in v0.1," reconsiderable

Source: `docs/planning/ADR-001.md` line 63 ("Defer group rooms/broadcast,
attachments, durable offline mailboxes, federation, full E2E encryption, GUI,
production Gemini/ChatGPT/Cursor adapters, and alternative transports.").

### Group rooms / broadcast

**Not in v0.1.** Reason: `docs/planning/ADR-001.md` line 61's v0.1 scope is
direct messaging only ("session identity/addressing, direct messaging"); a
broadcast/room primitive is a distinct addressing and delivery-semantics
problem `docs/planning/v0.1/05-interfaces.md` §4 does not define. Reconsider
when a second v0.1 harness pair is validated and a concrete multi-party use
case is named, since it changes the envelope's addressing model
(`docs/planning/v0.1/05-interfaces.md` §4) and the delivery-state set
(`docs/planning/v0.1/05-interfaces.md` §9), not just add a feature flag.

### Attachments

**Not in v0.1.** Reason: `docs/planning/DESIGN.md` line 4 scopes OAC to
"messaging," and the envelope/content model
(`docs/planning/v0.1/05-interfaces.md` §3) defines a text content model only.
Reconsider when a binary/attachment content type is added to the envelope
content model as a versioned spec change under
`docs/planning/v0.1/05-interfaces.md` §11's versioning policy.

### Durable offline mailboxes

**Not in v0.1.** Reason: `docs/planning/ADR-001.md` line 61 scopes v0.1 to
"active push"; `docs/planning/v0.1/06-security.md` §7's `DeliveryReceipt` states
(`accepted-by-adapter`, `handed-to-harness`, `unknown`) assume a live receiver,
not a store-and-forward mailbox. Reconsider only if a validated use case needs
delivery to an addressed session that is not currently live, since it requires
a new persistence component outside today's daemon-in-memory model
(`docs/planning/decisions/C2-process-model.md` §1).

### Federation

**Not in v0.1.** Reason: `docs/planning/v0.1/04-architecture.md`'s local and LAN
deployment topologies (§12-§13) both assume a single administrative domain; no
cross-domain trust or routing model exists. Reconsider only after a LAN
deployment is validated in practice and a concrete cross-domain trust model is
proposed as its own decision record, per the amendment procedure
(`.claude/skills/oac-evidence/SKILL.md` §6).

### Full E2E encryption

**Not in v0.1.** Reason: `docs/planning/ADR-001.md` line 56 lists "optional
later E2E encryption" as a security layer beyond v0.1's authenticated/encrypted
transport plus cryptographic message authenticity
(`docs/planning/v0.1/06-security.md` §6-§7). Reconsider once transport-level
authenticity (Ed25519 envelope signing, `docs/planning/decisions/
C5-envelope-auth.md`) is validated end-to-end and a threat is named that
transport-level security does not already mitigate
(`docs/planning/v0.1/06-security.md` §14's threat table).

*Dated note, 2026-10-09 (the lead's ruling on PR #364; `ADR-001-A4`, proposed in
`docs/planning/ADR-001-AMENDMENTS.md`, in force from the lead's merge of the payload-sealing
pull request):* that threat was named. On a transport that delivers every payload to every
peer, another installation could read every payload. Per-recipient payload sealing
(`spec/security.md` §14, `spec/interfaces.md` §6.10) is therefore v0.1 work on every
transport that declares `sealing`. What stays deferred is the rest of E2E encryption:
forward secrecy, confidentiality of traffic metadata (size, timing, link), and
confidentiality toward the recipient device's own harnesses.

### GUI

**Not in v0.1.** Reason: `docs/planning/DESIGN.md` line 11 scopes v0.1 to "one-
command local CLI deployment"; `docs/planning/v0.1/08-cli-and-deployment.md`
defines only a CLI surface. Reconsider once the CLI surface and its five-row
command table (`docs/planning/v0.1/08-cli-and-deployment.md` §5) are stable
enough that a GUI would wrap an unchanging contract rather than an evolving one.

### Production Gemini/ChatGPT/Cursor adapters

**Not in v0.1.** Reason: `docs/planning/ADR-001.md` line 61 names only the
Claude and Codex adapters as in-scope; `docs/planning/v0.1/01-capability-matrix.md`
carries no capability row for Gemini or ChatGPT, and Cursor's only v0.1 artifact
is the design-for-replacement proof (§3.6 below), not a production adapter.
Reconsider per-provider, once that harness documents a supported (not
undocumented-RPC) live-session-injection surface analogous to Claude Channels
or the Codex app-server, satisfying the `docs/planning/ADR-001.md` line 24
boundary against undocumented private RPCs.

### ACP adapter

**Not in v0.1.** *(Added 2026-10-06, #49.)* Reason: ACP is not a v0.1 dependency
(PLANNING-PROMPT.md §3.5; `docs/planning/PINS.md` "ACP"), and `docs/planning/ADR-001.md`
line 61 names only the Claude and Codex adapters. The E9 proof
(`docs/planning/decisions/E9-replacement-proofs.md` §2) shows an ACP adapter fits the
frozen adapter contract with no transport, core-type or spec change. It also shows that
ACP is a client-owned-session protocol whose only route that keeps ADR-001 is an ACP proxy,
a Draft RFD, so the adapter would rest on an experimental surface (record F-A1). Until a
binding shows, with evidence, that a hand-off meets [SEC-AUZ-022], it would declare its
sessions send-only (record §2.4, F-A2). Reconsider once the proxy RFD is completed, or ACP
offers another supported route into a client-owned session, and a binding can make the
[SEC-AUZ-022] statement.

### Alternative transports (NATS, MQTT)

**Not in v0.1.** Reason: `docs/planning/ADR-001.md` line 63 names NATS and MQTT
explicitly as deferred; Zenoh is the only transport implemented, per
`docs/planning/v0.1/07-repository-and-dependencies.md` §2's module table. A
design-for-replacement proof exists for both
(`docs/planning/v0.1/05-interfaces.md` §17) showing the `Transport` contract's
seven operations are sufficient to swap transports with no adapter or spec
change, but neither is built. Reconsider once a concrete deployment need names
a capability Zenoh's peer-mode profile lacks (routing/federation, broker-backed
offline queueing) — see `docs/planning/v0.1/11-risks.md` RISK-NATS and
RISK-MQTT for the open evidence gap that must close first.
*(Dated note, 2026-10-06, #49: the proof now argues from the frozen `spec/interfaces.md` §6,
in `docs/planning/decisions/E9-replacement-proofs.md` §3-§6, and RISK-NATS is
narrowed and RISK-MQTT closed. Neither candidate provides offline queueing in a profile the contract
allows ([IFC-TRN-026]), so that reason to reconsider is gone. Alternative transports stay
deferred by ADR-001 "v0.1 scope" (L63). A broker transport needs a server someone runs, and
ADR-001 "Decision" (L21) rules that out for normal local use. Local use need not touch one:
an implementation may keep its own sessions' traffic on a transport that stays inside it
([IFC-TRN-002]), and [IFC-TRN-081] keeps presence records, and so all cross-implementation
traffic, off any transport that is not cross-implementation and destination-restricted. A
broker would therefore serve only cross-installation traffic, and adding one is a deployment
decision under ADR-001 (record F-T1).)*

## 2. Boundary, not backlog — permanent, never a v0.2 candidate

Source: `docs/planning/ADR-001.md` line 24 (the boundary "MUST NOT" clauses) and
`docs/planning/DESIGN.md` line 15 (non-goals: "Model inference/routing, agent
planning, shared context management, replacement provider auth, or unsupported
client impersonation"). These are marked **boundary, not backlog** so a later
reader never mistakes them for a deferred v0.2 feature — no version of OAC adds
these, because adding them would make OAC the agent harness ADR-001's Context
section says it explicitly is not ("without creating another agent harness or
model router").

- **Model inference.** OAC MUST NOT call provider model APIs as a substitute for
  a native harness (`docs/planning/ADR-001.md` line 24). Every message reaches a
  model only by waking or injecting into that provider's own live harness
  session (`docs/planning/ADR-001.md` line 12).
- **Model routing.** OAC MUST NOT implement model routing (`docs/planning/
  ADR-001.md` line 24, `docs/planning/DESIGN.md` line 15). OAC has no concept of
  "which model should answer this" — that decision stays inside each harness.
- **Agent planning / orchestration.** OAC MUST NOT implement agent planning
  (`docs/planning/DESIGN.md` line 15). OAC delivers messages between already-
  running sessions; it does not decide what any session does next.
- **Shared context management.** OAC MUST NOT implement context management
  (`docs/planning/ADR-001.md` line 24, `docs/planning/DESIGN.md` line 15). Each
  harness keeps its own context; OAC's neutral types
  (`spec/interfaces.md` §4) carry no session-context payload,
  only messages, identity, and delivery state. OAC as a shared memory layer
  (storing, fetching, attaching or injecting memory such as Beacon's) is
  boundary, not backlog: each harness reaches such a service natively, and OAC
  never touches it (`docs/planning/decisions/L1-beacon-memory.md` §1, §5).
- **Replacement provider auth.** OAC MUST NOT steal or reuse another harness's
  provider credentials (`docs/planning/ADR-001.md` line 24); `docs/planning/DESIGN.md`
  line 15 names this "replacement provider auth." G2's own pass criteria require
  confirming OAC holds no OpenAI credentials at any point
  (`docs/planning/v0.1/02-gating-findings.md` §4); OAC's own identity layer
  (device key, session id) never substitutes for a provider login
  (`docs/planning/decisions/C4-session-identity.md`).
- **Unsupported client impersonation.** OAC MUST NOT present itself as a client
  identity a provider does not support, to reach a surface that provider does
  not offer it — `docs/planning/DESIGN.md` line 15 names this its own non-goal,
  distinct from the ADR-001 scraping/private-RPC clause below. Every provider
  surface OAC uses is labelled supported/research preview/experimental/
  undocumented per `.claude/skills/oac-evidence/SKILL.md` §4, and an
  "undocumented" label is itself a finding, not a green light to proceed.
- **UI/terminal scraping.** OAC MUST NOT depend on UI/terminal scraping for
  supported integrations (`docs/planning/ADR-001.md` line 24): OAC never reads
  or writes a harness's terminal UI to simulate a user.
- **Undocumented private RPCs.** Same ADR-001 line 24 clause: OAC never depends
  on an RPC surface a provider has not documented, for any integration this
  project supports.
- **Zenoh concepts in the neutral protocol.** OAC MUST NOT leak Zenoh-specific
  concepts into the neutral protocol (`docs/planning/ADR-001.md` line 24,
  restated in `docs/planning/DESIGN.md` line 35: "The specification MUST NOT
  mention Zenoh keys, MQTT topics, NATS subjects, or provider-specific method
  names."). Enforced as a containment boundary, not a style preference, in
  `docs/planning/v0.1/07-repository-and-dependencies.md` §4(a).

## 3. Package-level v0.1 exclusions decided elsewhere

Narrower exclusions already decided in specific package files, indexed here so
this file is the single place a reader checks for "is X deferred."

- **Permission relay off by default.** `capabilities.experimental
  ['claude/channel/permission']` is not declared in v0.1, a chosen default
  (not an availability accident — every recorded Claude Code version, from the
  minimum `v2.1.282` on, satisfies the capability's version floor; see
  `docs/planning/v0.1/06-security.md` §11. Note, 2026-10-01, #216: previously "the pinned
  Claude Code version"). Decided in
  `docs/planning/v0.1/03-decisions-and-amendments.md` Decision 9, resolving
  conflict C10; full three-strand justification in
  `docs/planning/v0.1/06-security.md` §11. Reconsider only via its own explicit
  decision record, not a configuration flag flip
  (`docs/planning/v0.1/06-security.md` §11).
- **Key rotation manual, no automatic rotation.** One Ed25519 keypair per
  device in v0.1, with no automatic rotation — re-pairing is manual. Decided in
  `docs/planning/decisions/C5-envelope-auth.md`, cited in
  `docs/planning/STATUS.md`'s C5 decision entry. Reconsider once a rotation
  trigger and a re-pairing-at-scale story are both named.
- **Transitive license sweep deferred to Stage 6.** `cargo deny`/`cargo license`
  over the resolved dependency graph is not run pre-Stage-0 because no
  workspace or `Cargo.lock` exists yet. Decided in
  `docs/planning/v0.1/07-repository-and-dependencies.md` §8. Runs at Stage 6
  release hygiene (`docs/planning/v0.1/10-stages.md` §10).
- **C7 presence/discovery gap 1 — full `PresenceRecord` not mapped.** The
  liveliness-token presence mapping carries only reachability, not the full
  `PresenceRecord` (harness ownership, capabilities, active-inbound support)
  `docs/planning/DESIGN.md` lines 104-105 ask for. Open gap recorded in
  `docs/planning/decisions/C7-zenoh-transport.md` §4, restated in
  `docs/planning/v0.1/03-decisions-and-amendments.md` Decision 10 and
  `docs/planning/STATUS.md` "Open conflicts." Reconsider when a future decision
  closes it — not scheduled to a stage yet.
- **C7 presence/discovery gap 2 — unknown-session discovery undefined.** No
  discovery path is defined by which a peer learns an *unknown* session's
  opaque id, even though `docs/planning/ADR-001.md` line 61 puts presence/
  discovery in v0.1 scope. Same source and restatement as gap 1 above.
- **ACP as forward-compatibility check only, not a v0.1 dependency.** The
  ACP-adapter proof (`docs/planning/v0.1/05-interfaces.md` §16) shows a third
  provider adapter is possible over the existing `ProviderAdapter` contract
  with zero `Transport`-contract changes, labelled **supported / forward-compat
  only** (`docs/planning/v0.1/01-capability-matrix.md`, `docs/planning/PINS.md`
  "ACP"). Reconsider only if ACP becomes an actual v0.1+ dependency, i.e. a
  concrete ACP-speaking harness is targeted.
- **Cursor adapter design-proof only, not built.** Same source as above
  (`docs/planning/v0.1/05-interfaces.md` §16) — Cursor's expected mapping is via
  ACP (`docs/planning/ADR-001.md` line 46: "Cursor: expected ACP mapping;
  deferred unless needed for v0.1 validation."). No Cursor adapter code exists
  or is planned for v0.1; only the signature-level proof that one could be
  added without changing the `Transport` contract.
- **K6 opt-in CI on self-hosted runners (added 2026-10-02, #228).** **Not in
  v0.1.** The operator decided on #129 (2026-10-02) to defer K6 past v0.1.
  Reason: live harness legs run locally through herdr, driven by an agent while
  the operator signs in and accepts consent dialogs (#187), so a self-hosted
  runner adds no evidence v0.1 needs. Running provider harnesses on a runner
  attached to this public repository would also require installing the pre-job
  hook (`docs/planning/gates/herdr-runner.md` §1). The workflow
  `.github/workflows/herdr-provider-optin.yml`, its entry point
  `tools/herdr/ci.mjs` and the runner hooks under `tools/herdr/runner-hooks/`
  stay in the repository, built and unrun. Their open items stay in
  `docs/planning/STATUS.md` "Open UNVERIFIED items" (the K6 entry). Reconsider
  when an unattended, scheduled provider run becomes a release requirement
  (Stage 6), or when local herdr runs stop being practical. #129 is to be closed
  as not planned when #228 merges.

## Self-check (`oac-evidence` §8, `oac-planning-package` §6)

- Every `§n` cross-reference above was re-swept against each target file's
  final section numbering as read for this task; none pointed at a stale
  number.
- Naming resolution (`.claude/skills/oac-planning-package/SKILL.md` §4) applied
  throughout: "Open Agent Channel (OAC)", "OAC Session Channels", `oac`; no
  `sessionchannels` or bare "Session Channels" spelling introduced.
- No neutral-interface text in this file names a Zenoh/Claude/Codex/MCP method
  name where neutral text is required — §2's boundary restatement quotes
  `docs/planning/DESIGN.md` line 35's own prohibition rather than violating it.
- No entry above proposes calling a provider model API, holding provider
  credentials, scraping a UI, or polling an inbox from an adapter claiming
  active inbound.
- All eight ADR-001-line-63 exclusions present (§1); all nine boundary/non-goal
  items present (§2); all seven package-level exclusions from the task
  breakdown present (§3) — permission relay, manual key rotation, transitive
  license sweep, C7 presence/discovery gap 1, C7 presence/discovery gap 2, ACP
  forward-compat-only, Cursor design-proof-only, counted as separate items.
  *(Dated note, 2026-10-02, #228: §3 now has an eighth item, the K6 deferral.
  It was added after this self-check and is not one of the original seven.)*
