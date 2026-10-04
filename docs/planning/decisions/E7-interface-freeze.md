# E7 — Interface freeze record (Gate S2)

- **Date:** drafted 2026-10-04 and re-audited the same day, after #272, #278 and #279
  merged. Freeze date: `{{SIGN-OFF DATE}}`, filled in at operator sign-off.
- **Issue:** #47 (E7, Epic E #5).
- **Gate:** Gate S2, the interface-freeze half (`docs/planning/v0.1/10-stages.md` §6,
  criteria 1-6).
- **Status:** **PROPOSED, not frozen.** This record prepares the freeze for the operator.
  It does not declare it. Section 3 lists the blockers. B1, B2 and B5 are closed. B3 and B4
  (#275) are open on `main`, and PR #284 addresses them but is not merged. Freezing is the operator's decision. It is recorded in §8, and §6 and §7
  take effect only after that.
- **Skills:** `oac-spec-authoring` (§7, the interface freeze), `oac-boundaries`,
  `oac-evidence`.

Line citations are to `main` at `08ef7c5` (the PR #279 merge, 2026-10-04) unless a line
says otherwise. Every result in §2 was re-run for this revision on the branch that merges
that commit. None is carried over from the first draft.

---

## 1. What the freeze covers

When signed off, these are frozen at revision `0.1`:

| Item | Where | Size at `08ef7c5` |
|---|---|---|
| OAC Session Channels protocol | `spec/session-channels.md` (§4-§8, Appendix A) | 245 requirement ids (222 `MUST`/`MUST NOT`), 70 index rows with a `TODO(fixture)` part |
| Security model | `spec/security.md` (§4-§13, Appendix A) | 115 ids (107), 41 rows with `TODO(fixture)` |
| MCP binding | `spec/bindings/mcp.md` (§2-§8, §12.3) | 57 ids (53), 27 rows with `TODO(fixture)` |
| Interface contracts: core neutral types (§4), `ProviderAdapter` (§5), `Transport` (§6), neutrality (§7), owner index (Appendix C) | `spec/interfaces.md` | 93 ids (90), 85 rows with `TODO(fixture)` |
| Extension identifier | `io.github.rossgraeber/oac-session-channels`, major version 0 (`spec/session-channels.md` §5.1) | 1 |
| Requirement-id prefixes | `SC`, `SEC`, `MCPB`, `IFC` (`spec/session-channels.md` §3.2; `spec/bindings/mcp.md` §12.1; `spec/interfaces.md` §3.1) | 4 |
| Fixture formats | `oac-conformance-fixture/1`, `oac-mcpb-fixture/1` | 475 fixtures, all citing `spec_revision` `0.1` |

In total: 510 requirement ids, 472 of them `MUST` or `MUST NOT`. `spec/interfaces.md`
Appendix C gives each of the 472 exactly one owning part (`core`, `adapter`, `transport` or
`binding`), and the runner checks this (`checkOwners`).

The conformance fixtures are not frozen. A fixture can be added, and a `TODO(fixture)`
replaced, after the freeze without changing the interface (`spec/session-channels.md` §5.2
item 7). That is how Stage 3 works.

## 2. Readiness audit

| # | Check | Result | Evidence |
|---|---|---|---|
| 1 | Conformance runner, every fixture plus index cross-checks, including the Appendix C owner index | **PASS** | `node tests/protocol/runner/run.mjs`: "Fixtures: 475/475 pass. Index checks: clean." |
| 2 | Runner self-test (Ed25519 and JCS vectors; planted owner-index edits) | **PASS** | `node tests/protocol/runner/run.mjs --self-test`: "Result: PASS (self-test)" |
| 3 | Every cited requirement id resolves | **PASS** | The scratch audit found every `[SC-…]`, `[SEC-…]`, `[MCPB-…]` and `[IFC-…]` citation in the four documents defined (510 ids). |
| 4 | Every `MUST`/`MUST NOT` has a fixture, or a `TODO(fixture)` naming a task outside Stage 2 | **FAIL, B4** | SC, SEC and IFC pass. Every IFC row names an owner (F-, G- or H-tasks), and so do the new SEC-AUZ-025 to -027 (G7), SEC-PRV-015 to -018 and MCPB-CDX-002 to -005 (G7). The remaining gap is the same 22 `spec/bindings/mcp.md` §12.3 rows, which name no task (L1229-L1281) → **#275**. Seven rows say "covered by" another requirement's fixtures, which is acceptable: SC-ID-020, SC-DLV-040, SC-DLV-064, SC-RCP-032, SEC-KEY-002, SEC-SIG-020, SEC-RPL-001. |
| 5 | Every Table 8.3 error code and Table 8.1 state decided by a fixture (`10-stages.md` §6, executable demonstration) | **FAIL on `main`, B3** | All 8 states are decided. 15 of 17 codes are decided. `invalid-request` is decided only by binding fixtures (`mcpb-tool/MCPB-TOOL-005.p01`, `.n01`; `mcpb-att/MCPB-ATT-002.n02`), which count (ruling below). `envelope-too-large` (`spec/session-channels.md` L2333) appears in no fixture. `transport-failure` (L2347) appears only as input to `sc-rcp/SC-RCP-085.p06` → **#275**, addressed by PR #284 (open). |
| 6 | Neutral vocabulary over `spec/`, with only the binding exempt; `spec/interfaces.md` is included | **PASS** | Checks 1-2 and the zero-hits group find zero hits over all three neutral documents. The read-the-hit group (`initialize`, `notifications/`) also finds zero. `spec/interfaces.md` §7 Table 7.1 lists every non-neutral value that crosses a contract, with the reason it is allowed. This meets the #47 acceptance item "any unavoidable exception is documented with its reason". |
| 7 | No open placeholders, "planned" text or stale stubs | **PASS** | The `spec/security.md` Appendix B dated note (this change) records that its follow-ups are done. `spec/session-channels.md` §8.1.5's "carriage ... still to come" has a #273 dated note pointing at `spec/interfaces.md` §6.5. `05-interfaces.md` §13-§15 carry "Superseded, 2026-10-04 (#273)" (L468, L531, L618). |
| 8 | Draft markers and versions are ready to finalize | **READY** | The markers, now including `spec/interfaces.md` L5, are listed in §6. Version `0.1` and the identifier are final. |
| 9 | The frozen interfaces exist as normative text | **PASS** | `spec/interfaces.md` (#273, PR #279, merge `08ef7c5`). §9 maps every M0 and DESIGN member to its successor, with the reason for each change. |
| 10 | Stage 1 results that contradicted a decision have produced an amendment or decision (Gate S2 criterion 6) | **PASS** | C13 is resolved (C6 §5.0). The #224 decision is in the spec (#274, PR #278, merge `a405bfd`): [SEC-AUZ-022] (`spec/security.md` L1015) is unconditional and behavioural. SEC-AUZ-025 to -027 are added, and so are MCPB-CDX-002 to -005 (`spec/bindings/mcp.md` L977-L986): `thread/queue/add` only, never `turn/start` or `turn/steer`, no override fields. |
| 11 | Operator decisions on #43, #45, #46, #174 and #224 are reflected | **PASS** | #43 (refuse at once, never hold; stale or withdrawn presence refuses; discovery lists `online` only; same-install until E5): dated notes in `spec/session-channels.md` L1405, L1560, L1756, L1785, L1849, and [IFC-TRN-026] (no persistence or offline queueing). #45 (24-hour one-message reply right; explicit grant on the same machine and folder; 5-minute cross-machine presence cap; per-session, folder or machine grants): `spec/security.md` dated notes L821, L871, L968, L1304. #46 (Codex outbound refused until #69): [MCPB-ATT-002] with its dated note, `spec/bindings/mcp.md` L387. #174 (SHOULD NOT secrets; no memory look-up by an implementation; land before the freeze): [SEC-PRV-016] to [SEC-PRV-018], dated notes L1477 and L1494, PR #272 merge `96d0b33`. #224: row 10. |
| 12 | Open UNVERIFIED items that affect the interface are listed with owners | **PASS** | §4.1 |
| 13 | Other CI checks (`boundary-lint.yml`) | **PASS** | Check 3, check 8 and check 11 are clean. Checks 9-10 report PENDING, which is not a pass, because no product paths exist yet; their self-test is 102/103 with 1 skipped. Fixture manifest: 211/211 entries match, self-test 73/73. `check-skills`: all 15 skills are within budget. `sync-agents-skills --check` and `--self-test` pass. |

**Ruling (orchestrator, 2026-10-04, relayed on #47):** fixtures in a binding document's own
format (`oac-mcpb-fixture/1`, `spec/bindings/mcp.md` §12.2) count toward the Gate S2
executable demonstration, just as `oac-conformance-fixture/1` fixtures do. Both are read and
decided by the same runner (`tests/protocol/runner/run.mjs`, which dispatches on
`fixture_format`). So a code decided only by binding fixtures, such as `invalid-request`,
counts as covered.

Editorial fixes made in this change (no requirement added or changed):

- `spec/session-channels.md` Appendix A, SC-ID-080. Its `TODO(fixture)` named only E3 and
  E6, both closed Stage 2 tasks. It now names F6 and F10, like the other issuer-side rows
  of §7.2.5.
- `spec/security.md` Appendix B. A dated note records that the nine follow-ups are applied.
- `docs/planning/STATUS.md` "Open epics" cell and the top entry.
  `docs/planning/v0.1/10-stages.md` §6 gets a "Current verdict" paragraph.

## 3. Blockers

| # | Blocker | Status | Evidence |
|---|---|---|---|
| B1 | `ProviderAdapter`, `Transport` and the core types had no normative text; the M0 draft contradicted the merged specs | **Closed**, #273 | `spec/interfaces.md`, PR #279, merge `08ef7c5` (2026-10-04) |
| B2 | The #224 operator decision (never steer; queue; race-free form; no override fields) was not in the spec | **Closed**, #274 | PR #278, merge `a405bfd`. See §2 row 10. |
| B3 | `envelope-too-large` and `transport-failure` have no fixture that decides them, which fails the Gate S2 executable demonstration | **Open on `main`**: #275, addressed by PR #284 (open, not merged). That PR adds deciding `sc-rcp` and `mcpb` fixtures for both codes. | §2 row 5 |
| B4 | 22 MCP binding `TODO(fixture)` rows name no owning task. Several can be decided from a single exchange, contrary to `spec/bindings/mcp.md` §12.2 (L1221-L1223). | **Open on `main`**: #275, addressed by PR #284 (open). That PR gives each row a fixture or an owner from a Stage 3/4 task. | §2 row 4 |
| B5 | L9 (#174): land before the freeze, or wait for the next major version | **Closed**: landed before the freeze, per the operator decision on #174 (2026-10-04) | PR #272, merge `96d0b33`; [SEC-PRV-015] to [SEC-PRV-018] |

B3 and B4 only add fixtures and owners. They change no requirement or signature. When PR
#284 merges, §2 rows 4 and 5 must be re-run against the merged text before sign-off. A
reviewer should not rely on the PR's own description of its effect. The operator may choose
either to sign off after #275 closes, or to sign off now and let #275
close after the freeze (§5.2 item 7). In the second case, Gate S2 criterion 4's
demonstration, and with it Stage 3 entry, still waits for #275.

## 4. Open items that do not block the freeze

### 4.1 UNVERIFIED items touching the interface (`docs/planning/STATUS.md` "Open UNVERIFIED items")

| Item | Where | Effect on the interface | Owner |
|---|---|---|---|
| Which OS facility yields a pairing key on each platform | `spec/session-channels.md` §6.7.2 dated note | None. Until it is established every signal is unpairable ([SC-ID-125]), which fails closed. | Epic F adapter work, G9 |
| Whether a pairing mechanism can attribute a dropped signal | §6.7.4 dated note | [SC-ID-154] is conditional on it | F10, G9 |
| Order of several inputs queued on the second harness surface | §7.1.3 | None. Informative evidence only. | G7 (#68) |
| How well the v0.1 transport would carry presence records. C7 carries reachability only, so it meets neither `spec/interfaces.md` §6.5 nor [IFC-TRN-080] yet. | `spec/interfaces.md` §6.7 note (L1095-L1102) | None for the contract. It gates cross-implementation traffic ([IFC-TRN-081]). | F6, F10, the transport binding (G1, G2) |
| `verify_strict` gives the spec's verdicts on every `sec-sig` fixture | `spec/security.md` §6 note | None. A library risk, not an interface risk. | F3, F4 |
| OS peer authentication on each platform | `spec/security.md` §9.7 | None ([SEC-AUZ-030] fixed) | G9 |
| Queue "not now" errors and queue runtime caveats (#274) | `spec/bindings/mcp.md` §8.2.1; `11-risks.md` rows 65-66 | Binding only | G6, G7 (#68) |
| Other MCP binding items (tool-result `_meta` surfacing, `rmcp` legacy channel, Codex default client era, Codex per-request session signal, Codex connection multiplexing, legacy clients ignoring `extensions`) | `spec/bindings/mcp.md` §10 | Binding only. Each is carried with an owner there. | #65, #69, RISK-G4 |

### 4.2 Decisions the freeze carries forward

- **The operations are renamed from #47's outcome list.** `spec/interfaces.md` §9 records
  each change with its reason:
  - `discover_sessions` → `take_connection` / `watch_attachments`;
  - `attach` → `set_binding`;
  - `publish_output` → `accept_requests`;
  - `announce_presence` → `send_presence`;
  - `deliver` and `capabilities` keep their names, with new shapes.

  Freezing accepts these names. #47's issue text keeps the DESIGN names as the conceptual
  list.
- **Codex: receive only, until #69.** [MCPB-ATT-002] refuses every Codex tool call on both
  eras until a documented Codex pairing exists. Codex sessions still receive, through
  `thread/queue/add` only (MCPB-CDX-002). Lifting the refusal is a binding change that
  relaxes a refusal. It does not touch the neutral spec.
- **Same-install only.** Presence, discovery and sending stay within one implementation
  until a transport binding meets [SC-DLV-066] and [IFC-TRN-080]. `spec/security.md` §10
  and §11 supply the authentication half. `spec/interfaces.md` §6.5-§6.7 now defines the
  carriage contract and the cross-implementation gate ([IFC-TRN-081]). No v0.1 transport
  meets that gate yet.
- **Never steer, and all Codex delivery rests on one experimental method.** Per #274,
  Codex delivery cannot reach ephemeral, queue-less, subagent or archived threads (each is
  `handoff-failed`), and deliveries wait after an interrupted turn. Implementation stays
  with G6 and G7 (#68).
- **Far-side receipts are optional in v0.1** ([SC-RCP-042], operator decision on #44).
- **No holding:** OAC never holds messages (operator decision on #43; [IFC-TRN-026]).
- **C11** (the shim-boundary module name) stays with Epic F/G implementation. It names a
  module, not an interface.
- **E9** (#49, design-for-replacement proofs) now has frozen-interface text to argue from.
  It is a Stage 2 exit artifact but not a Gate S2 criterion.

## 5. Gate S2 criteria (`10-stages.md` §6)

| Criterion | Status |
|---|---|
| 1. Adapter contract, transport contract and core types frozen | **Ready for sign-off.** The text exists (`spec/interfaces.md`). The freeze itself is §8. |
| 2. No transport or provider vocabulary in neutral spec text | **Met** (§2 row 6), including `spec/interfaces.md` |
| 3. Normative text separated from reference-implementation notes | **Met.** Notes are labelled blockquotes in all four documents, and the runner ties every keyword sentence to an indexed id. |
| 4. Every fixture executable, with a recorded expected outcome | **Met** for the 475 fixtures. The demonstration is short by two codes (B3, #275). |
| 5. Versioning and unsupported-capability behaviour stated normatively | **Met.** `spec/session-channels.md` §5 and §6.6. |
| 6. Stage 1 contradictions resolved before the freeze | **Met** (§2 row 10) |

## 6. Version and draft markers, edited at sign-off

These edits are part of the freeze. They are applied in the sign-off commit, not in this
draft:

| File:line (`08ef7c5`) | Now | At sign-off |
|---|---|---|
| `spec/session-channels.md` L4-L5 | `**Revision:** 0.1 (draft, Stage 2). Sections 4 and 5 (E1, #41), 6 (E2, #42), 7 (E3, #43) and 8 (E4, #44) are written.` | `**Revision:** 0.1, frozen at Gate S2 on {{SIGN-OFF DATE}} (E7, #47; docs/planning/decisions/E7-interface-freeze.md). Changes follow its §7.` |
| `spec/security.md` L4 | `**Revision:** 0.1 (draft, Stage 2). Written by task E5 (#45).` | `**Revision:** 0.1, frozen at Gate S2 on {{SIGN-OFF DATE}} (E7, #47). Written by task E5 (#45).` |
| `spec/interfaces.md` L5 | `**Revision:** 0.1 (draft, Stage 2). Written by #273 for task E7 (#47).` | `**Revision:** 0.1, frozen at Gate S2 on {{SIGN-OFF DATE}} (E7, #47). Written by #273.` |
| `spec/interfaces.md` §7, the paragraph after Table 7.1 ("Once Gate S2 freezes this document ...") | future tense | `Since Gate S2 froze this document on {{SIGN-OFF DATE}} ...`, citing §7 of this record |
| `spec/bindings/mcp.md` L4-L5 | `**Status:** Stage 2 draft, normative once Gate S2 freezes the spec surface ...` | `**Status:** normative; frozen at Gate S2 on {{SIGN-OFF DATE}} (E7, #47).` |
| `spec/bindings/mcp.md` L7 | `` `0.1` (draft until Gate S2; fixtures cite `spec_revision` `0.1`) `` | `` `0.1` (frozen; fixtures cite `spec_revision` `0.1`) `` |
| The revision-history appendix of each of the four documents | rows read `0.1 (draft)` | Add a row `0.1` with `{{SIGN-OFF DATE}}`: "Frozen at Gate S2 (E7, #47)". Earlier rows keep `(draft)` as history. |

The version stays `0.1` and the identifier is unchanged. The freeze makes the existing
revision binding. It is not a new one.

## 7. Change control after the freeze

From sign-off on, the frozen items in §1 change only as follows. The skill rule is
`oac-spec-authoring` §7. The classification is `spec/session-channels.md` §5.2-§5.3, and
`spec/interfaces.md` §7 (last paragraph) applies it to the contracts.

1. **Fixture-only and editorial changes.** Adding a fixture, replacing a `TODO(fixture)`,
   or an editorial change that leaves every requirement's meaning unchanged (§5.2 items 7
   and 8). These need a revision-history row and nothing more. The revision stays `0.1`.
2. **Any other change to a requirement, a frozen type or a frozen operation is a recorded
   amendment.** This includes moving a requirement to a different owner in Appendix C. It
   is filed as an issue and logged below as `E7-A<n>` with four fields: old text or
   signature (verbatim), new text or signature, rationale, and which stage's output it
   invalidates. It lands only after the operator approves it on that issue.
   - A compatible change (§5.2 items 1-6) bumps the minor version (`0.2`, …) under the
     same extension identifier.
   - A breaking change (§5.3, including any added `MUST` that a `0.1` implementation would
     violate) needs a new major version and a new extension identifier (§5.1, C3 §8).
3. **Stage 3 and 4 work implements against the frozen text and does not renegotiate it.**
   A `type:code` item that finds a frozen interface does not fit records a finding and
   proposes an amendment. It does not edit the interface (`oac-spec-authoring` §7;
   `oac-boundaries` boundary 10). A change made only to get a module to pass is a Stage 2
   freeze violation (`10-stages.md` §8).

### Amendment log

None.

## 8. Operator sign-off

The freeze happens when the operator fills in this section and the sign-off commit
applies §6. Until then, Gate S2 criterion 1 is not met.

- B3/B4 (#275, PR #284): `{{OPERATOR: closed at <merge sha>, §2 rows 4-5 re-run / deferred past the freeze}}`
- Renamed operations (§4.2) accepted: `{{OPERATOR: yes / no}}`
- Decision: `{{OPERATOR: FREEZE / NOT YET}}`
- Frozen at commit: `{{SHA}}`
- Date: `{{SIGN-OFF DATE}}`
- Recorded on: `{{link to the operator's comment on #47}}`

## 9. What freezing commits you to (plain language)

- **The wire format and rules stop moving.** These become fixed at `0.1`, and Stage 3 and
  4 code is built against them as written:
  - the envelope, ids, presence records, delivery states and error codes;
  - the signing, replay and authorization rules;
  - the four MCP tools and the Codex delivery rules.
- **The internal interfaces stop moving too.** The core types, the adapter contract and the
  transport contract in `spec/interfaces.md` become fixed, and so does the list of which
  part (core, adapter, transport or binding) is responsible for each rule. An adapter can
  never call a transport directly. A transport has to declare what it supports. The
  operations carry new names (§4.2), and freezing accepts them.
- **Any later change costs something and is visible.** Changing a rule, a type or an
  operation needs an amendment you approve. A change that would break a `0.1`
  implementation also needs a new version and a new extension identifier.
- **What stays open after the freeze:**
  - Codex can receive but not send (#69).
  - Traffic stays within one installation until a transport binding carries authenticated
    presence and receipts.
  - Codex delivery depends on one experimental method and never steers.
  - The UNVERIFIED items in §4.1 stay open with their owners. None of them changes an
    interface.
- **What continues regardless:** fixtures can be added and `TODO(fixture)` entries
  replaced at any time without an amendment. That includes #275.

## Boundary and evidence pass

- Nothing here proposes OAC owning a turn loop, holding provider credentials, holding
  messages, or polling (`oac-boundaries` pre-commit self-check).
- Provider and method names appear here only to locate binding text and issues. This
  record is not neutral spec text, and the neutral-vocabulary checks run over `spec/`.
- Every result in §2 was re-run on 2026-10-04 against the merge of `08ef7c5`. No
  UNVERIFIED label was dropped, and §4.1 adds none.
