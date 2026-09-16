---
name: oac-implementation
description: Workspace conventions, module dependency direction, contract-tests-first, error handling, and dependency licensing for type:code work items (Epics F and G, stages 3-4). The surface skill for the area under work loads alongside this one.
---

Loaded for any `type:code` work item — Stage 3 (Epic F, core and fakes) and Stage 4
(Epic G, adapters and Zenoh transport). It holds the rules that apply to *any* module;
it does not hold provider or Zenoh protocol detail — that is `oac-claude-channels`,
`oac-codex-appserver`, `oac-mcp`, `oac-zenoh`, loaded alongside this skill for the area
you are touching. Boundary text lives in `oac-boundaries`; test taxonomy lives in
`oac-testing`; both are separate loads, not restated here.

## 0. Check before writing code

`docs/planning/STATUS.md` shows Stage 0 and Stage 1 as not complete, and PLANNING-PROMPT.md
§5 decisions 1 (language/runtime) and 2 (process model) are **presumptive, not recorded**:
Rust / single static binary is the working assumption (§5.1), reversible if the Rust MCP
SDK's legacy-revision support or Windows keychain access turns out disqualifying; the
process model (daemon-plus-shims vs. self-contained per-session server, §5.2) is not chosen
at all. Read `docs/planning/STATUS.md` "Current stage" and "Open epics" before starting.
If Stage 0/1 are still open or these decisions are still undecided, no substantial `core/`
or `transports/zenoh/` code starts. Check the task's `depends` list in
`docs/planning/backlog/04-tasks-EF.json` / `05-tasks-GHIJ.json` before assuming any Epic F
or G task is startable: today F1 depends on `["C1","E7"]` and C1 is exactly the
language/runtime/packaging decision, F2 depends on F1, and F8/F9/F10 chain through F2 and
D6 (Stage 1 fixtures, not yet captured) — every current F task chains to C1 and/or D6,
including scaffolding, which is the most C1-dependent task of all. The correct action is to
stop and route the item to the blocking decision or spike instead (`type:decision` /
`oac-evidence` for C1/C2, or the Stage 0/1 owner for D6) — do not start code because a task
"looks like" scaffolding or fixture work.

## 1. Workspace layout (DESIGN "Suggested repository shape" — suggested, not frozen)

| Path | Responsibility |
|---|---|
| `spec/session-channels.md`, `spec/security.md` | Normative OAC Session Channels + security spec text; no provider/transport names |
| `ADR/` | Architecture decision records |
| `core/` | Neutral types, policy, identity, duplicate suppression, authorization, receipts |
| `cli/` | Config loading and supervisor wiring (verb set below) |
| `transports/zenoh/` | The Zenoh reference transport; Zenoh types live and die here |
| `adapters/claude/` | Claude Code Channels adapter |
| `adapters/codex/` | Codex App Server adapter |
| `tests/protocol/` | Envelope/spec conformance fixtures and runner |
| `tests/security/` | Spoof/replay/duplicate/unauthorized-routing tests |
| `tests/integration/` | Fake-endpoint and (opt-in) live-provider integration tests |
| `examples/` | Runnable usage examples, not shipped as library surface |
| `docs/` | User/contributor docs (distinct from `docs/planning/`) |

Additional transports are siblings of `transports/zenoh/` under the identical containment
rule — e.g. F7's in-memory transport lives at `transports/memory/`, a NATS/MQTT transport
at `transports/<name>/`.

DESIGN calls this shape "suggested," not decided: it is F1's output, not yet built
(STATUS.md: Pre-Stage 0; interfaces don't freeze until Stage 2). Do not create modules
outside this shape without recording why in the work item, but treat the shape itself as
pending F1, and carry DESIGN.md:142 verbatim when reasoning about it: "Do not choose
implementation language solely from this sketch."

**`cli/` verb set (PLANNING-PROMPT §5.11):** `start`, `status`, `sessions`, `doctor`, plus
whatever the process model needs. The binary name and the daemon-vs-embedded-per-session
shape are **C2 output, not yet decided** — DESIGN's own sketch spells the binary
`sessionchannels`, elsewhere the repo uses `oac`; G9/G10 (`depends: ["C2"]`) settle it. Do
not assert either spelling or shape in code or docs ahead of C2.

## 2. Dependency direction — mechanically checkable

Rule an agent can check by reading imports / the module's declared dependency manifest
(`Cargo.toml` only if C1 lands on Rust), not by judgment call:

- `adapters/*` and `transports/*` may depend on `core/`. `core/` depends on neither.
- `cli/` may depend on `core/`, on adapters, and on transports. Nothing may depend on `cli/`.
- Nothing in `adapters/claude/` may depend on `adapters/codex/`, or vice versa. No transport
  may depend on another transport (e.g. `transports/zenoh/` and `transports/memory/` stay
  siblings, neither depending on the other). No adapter may depend on any transport module.
  No sibling adapter/transport dependency, ever.
- An adapter routes an outbound message through `core/` policy and security (authorization,
  signing, duplicate suppression) and only then to a `Transport`. An adapter must never call
  a transport module directly, and must never call another adapter.
- Provider-specific types (Claude `meta` shapes, Codex JSON-RPC method payloads) stay inside
  their own adapter module. Zenoh-specific types (key expressions, `zid`, liveliness tokens)
  stay inside `transports/zenoh/`. Neither may appear in a `core/` signature or a `spec/`
  document. If a leak is truly unavoidable, document the exception and the reason inline —
  do not leave it implicit (DESIGN "Provider adapter contract", "Zenoh transport";
  PLANNING-PROMPT.md §5 preamble: "Provider-specific or Zenoh-specific concepts may not
  appear in neutral core interfaces. If unavoidable, document the exception and the
  reason.").
- Run the `oac-boundaries` mechanical checks (Zenoh-vocabulary grep, provider-method-name
  grep, no direct provider-SDK-import grep) against every file you touch under `spec/` and
  `core/` before calling a work item done.

## 3. Design-for-replacement — acceptance conditions, not aspiration

From PLANNING-PROMPT.md §6, restated as conditions any code change must satisfy:

- **A third provider adapter is addable with no change to any `transports/` module.**
  Concretely: adding `adapters/acp/` (or any future adapter) touches only `adapters/acp/`
  and, if a genuinely new core concept is needed, `core/` — never `transports/zenoh/`.
- **NATS or MQTT can replace Zenoh with no change to any `adapters/*` module or to
  `spec/`.** Concretely: a second transport module implementing the same `Transport`
  contract (`start`, `publish`, `subscribe`, `announce_presence`, `watch_presence`,
  `health`, `shutdown`, per DESIGN "Transport contract") must be pluggable without editing
  adapter code. Any optional capability (reliability, persistence, offline queueing,
  ordering, multicast discovery, routing/federation) that the new transport lacks must be
  declared through the contract's capability negotiation, not hidden behind an adapter
  special-case.
- If a change you are making would violate either condition, it is a boundary drift — stop
  and follow the `oac-boundaries` "stop, cite the boundary" protocol rather than landing it.

## 4. Contract-tests-first

1. Write the contract test suite for an interface (`ProviderAdapter`, `Transport`, or a
   `core/` policy surface) before the real module that implements it exists.
2. Build the fakes the suite runs against from the Stage 1 recorded fixtures
   (`docs/planning/backlog/04-tasks-EF.json` F8/F9 — fake Claude channel endpoint, fake
   Codex app-server endpoint, replayed from D6 fixtures), in that order: fixtures before
   fakes, fakes before the real module. See `oac-testing` for fixture provenance and
   fidelity rules — not restated here.
2a. A real module (a real adapter, or the Zenoh transport) is **done** only when it passes
   the identical contract suite the corresponding fake already passes — same suite, no
   suite fork per implementation.
3. CI default tier must be green with **no live provider, no API key, and no network beyond
   loopback** (PLANNING-PROMPT.md §6, §9.10; Epic F exit condition). Provider integration
   tests sit behind an explicit opt-in flag and run only against pinned versions.
4. See `oac-testing` for the full tier taxonomy and how to add a fixture; not restated here.

## 5. Error handling conventions

- Errors cross a module boundary only as the neutral `core/` error/result types defined by
  the frozen spec interfaces (DESIGN "Core", "Provider adapter contract", "Transport
  contract"). No provider-specific error type (a Codex JSON-RPC error object, a Claude MCP
  error) and no transport-specific error type (a Zenoh error) may cross out of its own
  adapter/transport module — translate at the boundary.
- No silent failure of a delivery path. A `Transport.publish` or adapter `deliver` failure
  must surface as one of the defined delivery states, not be swallowed or logged-only.
- Report delivery state honestly against what is actually knowable (PLANNING-PROMPT.md §5
  decision 5, Appendix A C6): a resolved "handed to transport/harness" call is reported as
  such, never as "delivered" or "seen by the model," because several providers (Claude Code
  channels) give no acknowledgement. The authoritative delivery-state vocabulary is the
  frozen spec's set (DESIGN "Delivery semantics": `accepted`, `rejected`, `unreachable`,
  `expired`, `duplicate`, `failed` — normative, C5/Stage 2 output, implemented by F6); do
  not substitute a different vocabulary. Within that set, "accepted by adapter," "handed to
  harness," and "unknown" are the minimum honesty distinctions §5 decision 5 demands when
  reporting what actually happened — not a replacement for the spec's states.

## 6. Dependency policy (PLANNING-PROMPT.md §5 decision 12)

For every third-party dependency added in any module: record its license, confirm
Apache-2.0 compatibility for the shipped artifact, and flag copyleft explicitly rather than
letting it pass silently. Zenoh itself is dual EPL-2.0/Apache-2.0 (PLANNING-PROMPT.md §3.4)
— use the Apache-2.0 option. Record the inventory entry in
`docs/planning/v0.1/07-repository-and-dependencies.md` — this is Epic A output; if it does
not exist yet, create it rather than skip the record. The CI-enforced license check is
task I2 (license inventory and third-party audit), not F12 (F12 is "stand up CI: default
tier green with no providers" and is a separate exit condition, §4 above). Do not add a
dependency without an inventory entry.

## 7. Exit criteria for a `type:code` work item

- [ ] Checked `docs/planning/STATUS.md`; if Stage 0/1 are open or §5 decisions 1-2 are
      still undecided, confirmed the specific task does not require them before proceeding.
- [ ] New/changed files sit in the module their responsibility (§1) says they belong in.
- [ ] Dependency direction (§2) holds; ran the `oac-boundaries` mechanical checks with zero
      unexplained hits.
- [ ] No provider-specific or Zenoh-specific type appears in a `core/` or `spec/` signature
      without a documented exception.
- [ ] The design-for-replacement conditions (§3) still hold after the change.
- [ ] A contract test suite exists and was written before the real module it now covers, or
      already existed and the module passes it unchanged.
- [ ] CI default tier passes with no live provider, no API key, no network beyond loopback;
      any new provider-integration test is opt-in and pinned.
- [ ] Delivery-state and error-type rules in §5 hold for every new delivery/error path.
- [ ] Every new third-party dependency has a recorded license and Apache-2.0 compatibility
      check, with copyleft flagged if present.
- [ ] The task's own Acceptance checklist (in `04-tasks-EF.json` or `05-tasks-GHIJ.json`)
      is satisfied.

## Where the content lives

- `docs/planning/DESIGN.md` — "Suggested repository shape," adapter/transport contracts,
  Zenoh containment rule.
- `docs/planning/PLANNING-PROMPT.md` §5 (decisions 1, 2, 10, 11, 12), §6 (design-for-
  replacement), §8 Stages 3-4, Appendix A (C6).
- `docs/planning/ADR-001.md` — Boundary.
- `docs/planning/backlog/01-epics.json` — Epics F, G.
- `docs/planning/backlog/04-tasks-EF.json`, `05-tasks-GHIJ.json` — F1-F12, G1-G11 tasks,
  their `depends` lists, and their Acceptance checklists; also I2 (license inventory) and
  F12 (CI default tier) in that file set.
- `docs/planning/STATUS.md` — current stage, gate verdicts, whether §5 decisions 1-2 have
  landed.
- `oac-boundaries` — the ADR-001 MUST NOTs and mechanical checks (not restated here).
- `oac-testing` — test tier taxonomy, fixtures, CI-default vs. opt-in (not restated here).
- `oac-claude-channels`, `oac-codex-appserver`, `oac-mcp`, `oac-zenoh` — surface protocol
  detail for the area you are touching (not restated here).
