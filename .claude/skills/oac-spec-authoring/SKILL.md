---
name: oac-spec-authoring
description: Normative-language rules, the neutral-vocabulary ban, conformance fixture format, versioning policy, and the interface freeze for OAC Session Channels. Load for type:spec, area:spec, or stage:2-spec work items (Epic E).
---

Writing or reviewing normative OAC spec text (`spec/session-channels.md`, `spec/security.md`,
the task E6 MCP binding document, or the frozen adapter/transport/core interfaces). Triggers:
labels `type:spec`, `area:spec`, `stage:2-spec`. Companion guardrails, not restated here:
`oac-boundaries` (ADR-001 MUST NOTs), `oac-evidence` (citation standard).

## 1. Normative language rules

- One requirement per sentence; split any sentence joined by "and" into two.
- `MUST`/`MUST NOT`: needs a conformance fixture (§5) in the same change, or it is
  **not yet specified** — mark it `TODO(fixture)`, not spec text.
- `SHOULD`/`SHOULD NOT`: a deviation-permitted recommendation; still state what "deviated"
  looks like even where no fixture enforces it in v0.1.
- `MAY`: explicitly optional; state what an omitting peer must still do to stay conformant.
- Never substitute lowercase "should"/"can"/"will"/"is expected to" for the four keywords;
  lowercase modal words belong only in reference-implementation notes (§2).

## 2. Separating normative text from reference-implementation notes

Mark reference-implementation material with a labelled blockquote immediately after the
normative paragraph it illustrates:

```
> **Reference implementation note:** <implementation choice, not binding on other
> conformant implementations>.
```

- The blockquote marker is mandatory; prose tone alone does not signal the split.
- A note may name the v0.1 Rust workspace's own choices (a signing library, a fixture
  directory layout) but must not introduce a requirement absent from the text above it —
  a fact every implementation must follow is normative, phrased with a §1 keyword, outside
  the blockquote.
- A reference-implementation note MUST NOT contain `MUST`, `MUST NOT`, `SHOULD`, or `MAY` —
  those keywords are normative-text-only; a note needing one is normative text misplaced.

## 3. The neutral-vocabulary rule (hardest rule in this skill)

`[ADR-001 Boundary]`: "MUST NOT leak Zenoh-specific concepts into the neutral protocol."
`[DESIGN §MCP Session Channels extension]`: "The specification MUST NOT mention Zenoh keys,
MQTT topics, NATS subjects, or provider-specific method names." Applies to spec prose *and*
every neutral interface name (core types, `ProviderAdapter`, `Transport`).

`oac-boundaries` "Mechanical checks" 1-2 already grep `zenoh|zid|key[_-]?expr|liveliness` and
the provider method names over `spec/ core/` — run those first; do not re-derive them here.
This skill adds only the spec-specific words (bare `claude`, `mqtt`, `nats`, `scouting`,
`codex`, `app[ -]server`, MCP method-name forms), the combined command, and its known gaps:
`references/neutral-vocabulary-check.md`.

Any hit in normative text or a neutral interface signature is a boundary violation — stop and
follow the `oac-boundaries` "Stop, cite the boundary" protocol, citing `[ADR-001 Boundary]` or
`[DESIGN §MCP Session Channels extension]`. The one exemption is task-scoped, not
filename-scoped: the task E6 binding document (path not yet fixed) may quote MCP and provider
identifiers by design, since it carries provider-facing binding detail out of the neutral
spec — the neutral spec and frozen interfaces (§7) stay clean.

## 4. What the spec surface must cover

The Stage 2 spec surface lands in `spec/session-channels.md` (PLANNING-PROMPT.md §8 Stage 2;
DESIGN §Suggested repository shape) — not in `05-interfaces.md`, the M0 planning-package file
under `docs/planning/v0.1/` (§9 item 6; task A6, milestone M0) this document supersedes. Per
DESIGN §MCP Session Channels extension and §9 item 6, the surface is exactly:

1. Message envelope and content model, plus versioning (task E1).
2. Session identity, addressing, and capability/extension negotiation (task E2).
3. Presence and discovery (task E3).
4. Active-delivery semantics and the no-polling rule (task E3).
5. Replies and correlation (task E4).
6. Delivery states (task E4).
7. Error taxonomy — closed: every error a conformant peer may emit is enumerated (task E4).
8. Versioning and unsupported-capability behaviour (tasks E1, E2).

Security (identity hierarchy, signing, replay, authorization, provenance) is its own document,
`spec/security.md` (task E5) — not folded into the envelope/addressing documents, and not
restated here; see `oac-security-work`.

## 5. Conformance fixture format

A fixture is a data file, not code, proving exactly one normative requirement, readable by a
second independent implementation (task E8 acceptance). Fixtures live under `tests/protocol/`
(DESIGN §Suggested repository shape); a conformance runner (Stage 3, task F12) executes them
in CI. Content shape, the required negative-fixture set, and the requirement-id scheme (fixed
by task E1 — no fixture cites an id before E1 lands): `references/conformance-fixtures.md`.

**The rule, restated from §1:** a `MUST` with no fixture is not yet specified. Treat a missing
fixture as a blocking gap in the same change that adds the `MUST` — mark it `TODO(fixture)`.

## 6. Versioning policy

Per PLANNING-PROMPT.md §3.3 (MCP extension model, SEP-2133): extension identifiers are
`{reverse-dns-prefix}/{name}`, negotiated through the `extensions` map in capabilities, and
**breaking changes require a new extension identifier** — cite this rule, do not reinvent one.

- A spec revision number identifies the wire-compatible envelope/protocol shape negotiated at
  the extension level (task E1's "what forces a new extension identifier").
- Non-breaking (same identifier, revision bump): an added optional field a peer can ignore, an
  added `MAY` capability, an added error code old peers can treat as `failed`.
- Breaking (new extension identifier required): removing/renarrowing a `MUST` field, changing
  an existing delivery state's or error code's meaning, changing signature scope.
- State both lists in `spec/session-channels.md`'s versioning section (the Stage 2 counterpart
  of the M0 `05-interfaces.md` draft, §4) — never leave "breaking change" undefined.

## 7. The interface freeze

At the end of Stage 2 (task E7), the adapter contract (`ProviderAdapter`), the transport
contract (`Transport`), and the core neutral types freeze — see DESIGN §Provider adapter
contract / §Transport contract / §Core for the member lists; not restated here.

What "frozen" obliges an agent to do afterwards:
- Stage 3/4 work (Epics F, G) implements against the frozen signatures; it does not
  renegotiate them.
- A later change to a frozen interface is a recorded amendment, not a silent edit — same
  procedure as an ADR-001 amendment (`oac-evidence` §6): old signature, new signature,
  rationale, and which stage's output it invalidates.
- A `type:code` work item that finds a frozen interface doesn't fit its need is a finding, not
  license to change the interface unilaterally (`oac-boundaries` boundary 10 applies here).

## 8. Known spec-shaping conflicts — resolve, do not gloss

Each is a requirement on the author of the relevant spec section, not an open question.

- **C4 — signature becomes normative.** Appendix A C4: DESIGN's `security.signature` field was
  "implementation-defined"; PLANNING-PROMPT.md §3.4 establishes Zenoh provides no
  application-layer message signing. Requirement: `spec/security.md` (task E5) MUST make the
  signature algorithm, the signed field set, and the verification procedure normative —
  transport-layer security (TLS/QUIC) is not an acceptable substitute for envelope signing.
- **C6 — delivery states defined by what is knowable.** Appendix A C6: DESIGN lists `accepted`
  as a delivery state, but PLANNING-PROMPT.md §3.1 states Claude Code sends no acknowledgement.
  Requirement: task E4's delivery-state definitions MUST distinguish "accepted by adapter"
  (written to transport), "handed to harness" (delivered into the provider's native surface),
  and never claim "seen by the model" for a provider that gives no such signal.
- **C8 — opaque id plus a separate display form.** Appendix A C8: DESIGN's addressing section
  suggests a session id may map to a URI containing device and harness; ADR-001 forbids
  leaking transport concepts. Requirement: task E2 MUST define the stable identity as opaque
  (bound to a device key, no transport-native address exposed) and define the human-readable
  `session://device/harness/id`-shaped form as a separate, explicitly non-authoritative display
  value — never the same field serving both purposes.
- **C9 — Codex correlation defined explicitly.** Appendix A C9: Codex has no channel-tag
  convention for outbound replies (PLANNING-PROMPT.md §3.2), while DESIGN assumes symmetric
  reply/correlation. Requirement: task E4 MUST state exactly how a Codex-side reply is
  correlated back to the originating envelope's `correlation_id`/`reply_to` given that Codex
  provides no native tagging — do not assume the Claude-side convention transfers.

## 9. Exit criteria for a spec work item

- [ ] Every `MUST`/`MUST NOT` has a fixture (§5) or a filed `TODO(fixture)`.
- [ ] Every reference-implementation note is a labelled blockquote (§2) with no RFC-2119
      keyword inside it.
- [ ] The zero-hits group (§3, `references/neutral-vocabulary-check.md`) is zero hits outside
      the task E6 binding document; read-the-hit group hits are read, not assumed guilty.
- [ ] The section's coverage matches its slot in §4 — no item merged into another document.
- [ ] A frozen-interface change (post-E7) is a recorded amendment (§7), not a silent edit.
- [ ] Any §8 conflict touched by this change is a stated requirement, not an open question.
- [ ] `oac-boundaries` pre-commit self-check has run against the changed files.

## Where the content lives

- Envelope, addressing, presence, delivery: `docs/planning/DESIGN.md`.
- Neutral-vocabulary boundary: `docs/planning/ADR-001.md` Boundary; `oac-boundaries`.
- Spec surface, freeze, output structure, versioning model: `docs/planning/PLANNING-PROMPT.md`
  §3.3, §8 Stage 2, §9 item 6, §10, §11 item 6.
- Conflicts C3, C4, C6, C8, C9: `docs/planning/PLANNING-PROMPT.md` Appendix A.
- Task-level acceptance for E1-E9: `docs/planning/backlog/04-tasks-EF.json`.
- Evidence/citation standard: `oac-evidence` (not restated here).
- Banned-word list and combined check command: `references/neutral-vocabulary-check.md`.
- Fixture content shape and requirement-id scheme: `references/conformance-fixtures.md`.
