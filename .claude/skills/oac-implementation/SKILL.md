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

`docs/planning/STATUS.md` ("Current stage", "Open epics", "Blocked") decides whether code
may start; read it, not this skill, before starting. The stage exits are recorded
decisions: Stage 1 via D7 (`docs/planning/decisions/D7-stage1-exit.md`), Stage 2 via Gate
S2 (`docs/planning/decisions/E-5-stage2-exit.md`, which opens Stage 3); the interfaces
change only under `docs/planning/decisions/E7-interface-freeze.md` §7. Language/runtime
and process model are recorded decisions (`docs/planning/decisions/C1-language-runtime.md`,
`C2-process-model.md`);
follow them, not PLANNING-PROMPT.md §5's presumptions. Check the task's `depends` list in
`docs/planning/backlog/04-tasks-EF.json` / `05-tasks-GHIJ.json`: if a dependency (a
decision, a spike's fixtures such as D6, or E7) is not closed, stop and route the item to
its owner — do not start code because a task "looks like" scaffolding or fixture work.

## 1. Workspace layout

The resolved v0.1 layout, and each module's single responsibility and ownership boundary,
are `docs/planning/v0.1/07-repository-and-dependencies.md` §1-§2: `core/`, `cli/`,
`adapters/claude/`, `adapters/codex/`, `transports/zenoh/`, `spec/`,
`tests/{protocol,security,integration}/`, `examples/`, `docs/`, `ADR/`. There is no
`daemon/` directory (07 §1). `tools/herdr/` is dev/test tooling, never a workspace member
(07 §1; `oac-boundaries` checks 9-10). F1 builds the layout. The frozen `spec/interfaces.md`
fixes the contracts, not these paths. Do not create modules outside this layout without
recording why in the work item.

Additional transports are siblings of `transports/zenoh/` under the identical containment
rule — e.g. F7's in-memory transport at `transports/memory/`, a NATS/MQTT transport at
`transports/<name>/`.

**Process model and CLI are decided** (`docs/planning/decisions/C2-process-model.md`): one
per-device `oac` daemon plus thin `oac mcp-shim` stdio processes (C2 §1), local IPC and
peer authentication (C2 §4), and the `oac` verb set with exit codes (C2 §6). G9/G10
implement them. Language/runtime is Rust (`docs/planning/decisions/C1-language-runtime.md`
§1), which discharges DESIGN's "do not choose implementation language from this sketch"
caveat (07 §1).

## 2. Dependency direction — mechanically checkable

Rule an agent can check by reading imports / each crate's `Cargo.toml` (C1 §1: Rust), not
by judgment call:

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
  contract (the operations of `spec/interfaces.md` §6.4, Table 6.4) must be pluggable
  without editing adapter code. An optional capability the new transport lacks is
  declared absent through the capability declaration of `spec/interfaces.md` §6.3, not
  hidden behind an adapter special-case.
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

- Errors cross a module boundary only as the neutral `core/` types of the frozen
  `spec/interfaces.md` (§4 core types, including §4.6 `DeliveryReceipt`/`ErrorCode`, and the
  §4.10-§4.11 boundary types). No provider-specific error type (a Codex JSON-RPC error
  object, a Claude MCP error) and no transport-specific error type (a Zenoh error) may cross
  out of its own adapter/transport module — translate at the boundary.
- No silent failure of a delivery path. A `Transport.publish` or adapter `deliver` failure
  must surface as one of the defined delivery states, not be swallowed or logged-only.
- Report delivery state honestly against what is actually knowable (PLANNING-PROMPT.md §5
  decision 5, Appendix A C6): report only a state actually observed, never "delivered" or
  "seen by the model," because several providers (Claude Code channels) give no
  acknowledgement. The delivery-state vocabulary is the frozen `spec/session-channels.md`
  Table 8.1 (§8.1.2; [SC-RCP-003]; §8.1.3 says what each state proves; `spec/interfaces.md`
  [IFC-TYP-051]); error codes are its Table 8.3. Implemented by F6. Do not substitute
  DESIGN's "Delivery semantics" set or any other vocabulary.

## 6. Dependency policy (PLANNING-PROMPT.md §5 decision 12)

For every third-party dependency added in any module: record its license, confirm
Apache-2.0 compatibility for the shipped artifact, and flag copyleft explicitly rather than
letting it pass silently. Zenoh is dual EPL-2.0/Apache-2.0, and OAC elects the Apache-2.0
arm (C1 §7). Record the inventory entry in the table of
`docs/planning/v0.1/07-repository-and-dependencies.md` §5 (C1 §10 is its source). The
CI-enforced license check is task I2 (license inventory and third-party audit), not F12
(F12 is "stand up CI: default tier green with no providers" and is a separate exit
condition, §4 above). Do not add a dependency without an inventory entry.

## 7. Exit criteria for a `type:code` work item

- [ ] Checked `docs/planning/STATUS.md` shows the task's stage open, and every item in the
      task's `depends` list is closed (§0).
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

- `spec/interfaces.md` (core types, adapter and transport contracts) and
  `spec/session-channels.md` (delivery states, errors), frozen at revision 0.1 (E7, #47).
- `docs/planning/v0.1/07-repository-and-dependencies.md` — resolved layout (§1), module
  ownership (§2), dependency direction (§3), Zenoh containment (§4), inventory (§5).
- `docs/planning/decisions/C1-language-runtime.md` (Rust, crates, licenses) and
  `C2-process-model.md` (daemon plus shims, IPC, CLI).
- `docs/planning/DESIGN.md` — background only; its repository sketch and contract sketches
  are superseded by 07 and the frozen `spec/` text.
- `docs/planning/PLANNING-PROMPT.md` §5 (decisions 5, 10, 11, 12), §6 (design-for-
  replacement), §8 Stages 3-4, Appendix A (C6).
- `docs/planning/ADR-001.md` — Boundary.
- `docs/planning/backlog/01-epics.json` — Epics F, G.
- `docs/planning/backlog/04-tasks-EF.json`, `05-tasks-GHIJ.json` — F1-F12, G1-G11 tasks,
  their `depends` lists, and their Acceptance checklists; also I2 (license inventory) and
  F12 (CI default tier) in that file set.
- `docs/planning/STATUS.md` — current stage, open epics, blocked stages, gate verdicts.
- `oac-boundaries` — the ADR-001 MUST NOTs and mechanical checks (not restated here).
- `oac-testing` — test tier taxonomy, fixtures, CI-default vs. opt-in (not restated here).
- `oac-claude-channels`, `oac-codex-appserver`, `oac-mcp`, `oac-zenoh` — surface protocol
  detail for the area you are touching (not restated here).
