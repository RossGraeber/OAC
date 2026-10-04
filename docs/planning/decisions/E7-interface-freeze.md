# E7 — Interface freeze record (Gate S2)

- **Date:** drafted 2026-10-04 and re-audited the same day, after #272, #278, #279 and
  #284 merged. Freeze date: `{{SIGN-OFF DATE}}`, filled in at operator sign-off.
- **Issue:** #47 (E7, Epic E #5).
- **Gate:** Gate S2, the interface-freeze half (`docs/planning/v0.1/10-stages.md` §6,
  criteria 1-6).
- **Status:** **READY FOR OPERATOR SIGN-OFF, not frozen.** This record prepares the freeze.
  It does not declare it. All five blockers in §3 are closed, and every check in §2 passes.
  Freezing is the operator's decision. It is recorded in §8, and §6 and §7 take effect only
  after that.
- **Skills:** `oac-spec-authoring` (§7, the interface freeze), `oac-boundaries`,
  `oac-evidence`.

Line citations are to `main` at `3e4471e` (the PR #284 merge, 2026-10-04) unless a line
says otherwise. Every result in §2 was re-run for this revision on the branch that merges
that commit. None is carried over from the first draft.

---

## 1. What the freeze covers

When signed off, these are frozen at revision `0.1`:

| Item | Where | Size at `3e4471e` |
|---|---|---|
| OAC Session Channels protocol | `spec/session-channels.md` (§4-§8, Appendix A) | 245 requirement ids (222 `MUST`/`MUST NOT`), 70 index rows with a `TODO(fixture)` part |
| Security model | `spec/security.md` (§4-§13, Appendix A) | 115 ids (107), 41 rows with `TODO(fixture)` |
| MCP binding | `spec/bindings/mcp.md` (§2-§8, §12.3) | 57 ids (53), 17 rows with `TODO(fixture)` |
| Interface contracts: core neutral types (§4), `ProviderAdapter` (§5), `Transport` (§6), neutrality (§7), owner index (Appendix C) | `spec/interfaces.md` | 93 ids (90), 85 rows with `TODO(fixture)` |
| Extension identifier | `io.github.rossgraeber/oac-session-channels`, major version 0 (`spec/session-channels.md` §5.1) | 1 |
| Requirement-id prefixes | `SC`, `SEC`, `MCPB`, `IFC` (`spec/session-channels.md` §3.2; `spec/bindings/mcp.md` §12.1; `spec/interfaces.md` §3.1) | 4 |
| Fixture formats | `oac-conformance-fixture/1`, `oac-mcpb-fixture/1` | 527 fixtures, all citing `spec_revision` `0.1` |

In total: 510 requirement ids, 472 of them `MUST` or `MUST NOT`. `spec/interfaces.md`
Appendix C gives each of the 472 exactly one owning part (`core`, `adapter`, `transport` or
`binding`), and the runner checks this (`checkOwners`).

The conformance fixtures are not frozen. A fixture can be added, and a `TODO(fixture)`
replaced, after the freeze without changing the interface (`spec/session-channels.md` §5.2
item 7). That is how Stage 3 works.

## 2. Readiness audit

| # | Check | Result | Evidence |
|---|---|---|---|
| 1 | Conformance runner, every fixture plus index cross-checks, including the Appendix C owner index | **PASS** | `node tests/protocol/runner/run.mjs`: "Fixtures: 527/527 pass. Index checks: clean." |
| 2 | Runner self-test (Ed25519 and JCS vectors; planted owner-index edits) | **PASS** | `node tests/protocol/runner/run.mjs --self-test`: "Result: PASS (self-test)" |
| 3 | Every cited requirement id resolves | **PASS** | The scratch audit found every `[SC-…]`, `[SEC-…]`, `[MCPB-…]` and `[IFC-…]` citation in the four documents defined (510 ids). |
| 4 | Every `MUST`/`MUST NOT` has a fixture, or a `TODO(fixture)` naming a task outside Stage 2 | **PASS** | All 472 do. The scratch audit reads every `TODO(fixture)` cell of the four indexes. Every one names an owner outside Stage 2, an F-, G- or H-task or an issue such as #65, #66, #68 or #69. None names a Stage 2 task alone. The 22 MCP binding rows that named no task now each have a fixture or a Stage 3/4 owner (#275, PR #284, merge `3e4471e`). That leaves 17 MCPB rows with `TODO(fixture)`, down from 27. 33 `MUST`/`MUST NOT` rows name no fixture of their own and no `TODO(fixture)`. Each cites the fixtures of other requirements that decide it. 20 of them say so with the words "covered by": SC-ENV-083, SC-ID-020, SC-ID-022, SC-ID-040, SC-ID-041, SC-ID-060, SC-DLV-040, SC-DLV-064, SC-DLV-065, SEC-KEY-002, SEC-KEY-040, SEC-SIG-012, SEC-SIG-020, SEC-STG-003, SEC-STG-004 (in part), SEC-RPL-001, SEC-AUZ-008, SEC-RCT-002, SEC-PRV-004, IFC-TYP-070. The other 13 cite the deciding fixtures directly, such as `sec-*` fixtures or the `expected.error` of other fixtures: SC-ID-009, SC-ID-102, SC-ID-181, SC-DLV-043, SC-RCP-009, SC-RCP-032, SC-RCP-040, SC-RCP-041, SC-RCP-070, SEC-RPL-030, SEC-AUZ-013, SEC-PRS-004, SEC-PRV-014. Across the four indexes, 27 `MUST`/`MUST NOT` rows contain "covered by". The 7 not counted above (IFC-TYP-001, -003, -041, -081, IFC-TRN-010, -032, -062) pair it with a `TODO(fixture)` that has an owner. Counted by the scratch audit with the runner's `indexRows` and `cellReferences`. |
| 5 | Every Table 8.3 error code and Table 8.1 state decided by a fixture (`10-stages.md` §6, executable demonstration), with binding fixtures counted (ruling below) | **PASS** | All 8 states and all 17 codes appear in a fixture that the runner decides. For 15 codes, a fixture has the validator itself produce the code as its decision: `expected.error` of a validation, security, delivery or send fixture. For `envelope-too-large` (`spec/session-channels.md` L2333) these are `sc-rcp/SC-RCP-076.n02`, `.n03`, `SC-RCP-079.n02` and `SC-RCP-071.n07`; `SC-RCP-028.p02`, `.n04` and `mcpb-tool/MCPB-TOOL-012.n08` check how it is carried. **Two codes are decided only as values carried in a receipt or a tool result.** No fixture has the validator emit them itself, because each is produced by behaviour no fixture stage simulates. `transport-failure` (L2347) is a sending implementation failing to reach a transport. `sc-rcp/SC-RCP-028.p01` (a sender-observed receipt carrying it is valid and `failed`) and `.n03` (a receiver-observed one is discarded) decide receipts that carry it. `mcpb-tool/MCPB-TOOL-010.p02`, `.n02` and `MCPB-TOOL-012.n09` decide how a tool result carries it. `invalid-request` (L2349) is a harness request being refused. Only the binding fixtures `mcpb-tool/MCPB-TOOL-005.p01`, `.n01` and `mcpb-att/MCPB-ATT-002.n02` decide it, as a value in a tool result, which counts under the ruling below. The `ttl_ms`-before-replay-window rule is decided by `sc-rcp/SC-RCP-072.n01` (`receive`), `SC-RCP-091.n01` and `SC-RCP-092.n01`. |
| 6 | Neutral vocabulary over `spec/`, with only the binding exempt; `spec/interfaces.md` is included | **PASS** | Checks 1-2 and the zero-hits group find zero hits over all three neutral documents. The read-the-hit group (`initialize`, `notifications/`) also finds zero. `spec/interfaces.md` §7 Table 7.1 lists every non-neutral value that crosses a contract, with the reason it is allowed. This meets the #47 acceptance item "any unavoidable exception is documented with its reason". |
| 7 | No open placeholders, "planned" text or stale stubs | **PASS** | The `spec/security.md` Appendix B dated note (this change) records that its follow-ups are done. `spec/session-channels.md` §8.1.5's "carriage ... still to come" has a #273 dated note pointing at `spec/interfaces.md` §6.5. `05-interfaces.md` §13-§15 carry "Superseded, 2026-10-04 (#273)" (L468, L531, L618). |
| 8 | Draft markers and versions are ready to finalize | **READY** | The markers, now including `spec/interfaces.md` L5, are listed in §6. Version `0.1` and the identifier are final. |
| 9 | The frozen interfaces exist as normative text | **PASS** | `spec/interfaces.md` (#273, PR #279, merge `08ef7c5`). §9 maps every M0 and DESIGN member to its successor, with the reason for each change. |
| 10 | Stage 1 results that contradicted a decision have produced an amendment or decision (Gate S2 criterion 6) | **PASS** | C13 is resolved (C6 §5.0). The #224 decision is in the spec (#274, PR #278, merge `a405bfd`): [SEC-AUZ-022] (`spec/security.md` L1015) is unconditional and behavioural. SEC-AUZ-025 to -027 are added, and so are MCPB-CDX-002 to -005 (`spec/bindings/mcp.md` L986-L995): `thread/queue/add` only, never `turn/start` or `turn/steer`, no override fields. |
| 11 | Operator decisions on #43, #45, #46, #174 and #224 are reflected | **PASS** | #43 (refuse at once, never hold; stale or withdrawn presence refuses; discovery lists `online` only; same-install until E5): dated notes in `spec/session-channels.md` L1405, L1560, L1756, L1785, L1849, and [IFC-TRN-026] (no persistence or offline queueing). #45 (24-hour one-message reply right; explicit grant on the same machine and folder; 5-minute cross-machine presence cap; per-session, folder or machine grants): `spec/security.md` dated notes L821, L871, L968, L1304. #46 (Codex outbound refused until #69): [MCPB-ATT-002] with its dated note, `spec/bindings/mcp.md` L387. #174 (SHOULD NOT secrets; no memory look-up by an implementation; land before the freeze): [SEC-PRV-016] to [SEC-PRV-018], dated notes L1477 and L1494, PR #272 merge `96d0b33`. #224: row 10. |
| 12 | Open UNVERIFIED items that affect the interface are listed with owners | **PASS** | §4.1 |
| 13 | Other CI checks (`boundary-lint.yml`) | **PASS** | Check 3, check 8 and check 11 are clean. Checks 9-10 report PENDING, which is not a pass, because no product paths exist yet; their self-test is 102/103 with 1 skipped. Fixture manifest: 211/211 entries match, self-test 73/73. `check-skills`: all 15 skills are within budget. `sync-agents-skills --check` and `--self-test` pass. |

**Ruling (orchestrator, 2026-10-04), recorded on #47** (https://github.com/RossGraeber/OAC/issues/47#issuecomment-5978246563): fixtures in a binding
document's own format (`oac-mcpb-fixture/1`, `spec/bindings/mcp.md` §12.2) count toward the
Gate S2 conformance demonstration alongside the core `oac-conformance-fixture/1` fixtures.
Both are decided by the same runner (`tests/protocol/runner/run.mjs`, which dispatches on
`fixture_format`). This ruling is what lets `invalid-request` count as decided.

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
| B3 | `envelope-too-large` and `transport-failure` had no fixture that decides them (Gate S2 executable demonstration) | **Closed**, #275 | PR #284, merge `3e4471e` (2026-10-04). Re-run against the merge: §2 row 5. |
| B4 | 22 MCP binding `TODO(fixture)` rows named no owning task, and several could be decided from a single exchange (`spec/bindings/mcp.md` §12.2, L1229-L1232) | **Closed**, #275 | PR #284, merge `3e4471e`. Re-run against the merge: §2 row 4. |
| B5 | L9 (#174): land before the freeze, or wait for the next major version | **Closed**: landed before the freeze, per the operator decision on #174 (2026-10-04) | PR #272, merge `96d0b33`; [SEC-PRV-015] to [SEC-PRV-018] |

No blocker is open. Rows 4 and 5 of §2 were re-run against the #284 merge itself, not taken
from that PR's description. If anything merges into `spec/` or `tests/protocol/` before the
sign-off commit, re-run §2 rows 1, 4, 5 and 6 on that commit.

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

### 4.2 What the freeze carries forward

**For the operator's acceptance (§8 row 2).** These two items go beyond what the recorded
operator decisions say:

- **(a) Cross-install waits for a transport binding, not only for E5.** The operator
  decision on #43 said cross-install presence and messaging wait for E5 (#45) to define
  presence-record authentication. E5 has done so (`spec/security.md` §10-§11). The merged
  text now also requires a transport binding that meets [SC-DLV-066] and [IFC-TRN-080]
  before any presence record crosses implementations ([IFC-TRN-081];
  `spec/session-channels.md` §7.3.2, #266 dated note; `spec/interfaces.md` §6.7). No v0.1
  transport meets that, because C7 carries reachability only. So v0.1 stays same-install
  until a later transport binding.
- **(b) All Codex delivery goes through the queue, not only when the thread is busy.** The
  operator decision on #224 said: when the thread is busy, deliver via `thread/queue/add`,
  and the spec should say which form, noting that check-busy-then-`turn/start` is racy.
  The binding chose the race-free form. Every Codex delivery uses `thread/queue/add`
  ([MCPB-CDX-002]), and `turn/start` is never used for delivery ([MCPB-CDX-003];
  `spec/bindings/mcp.md` §8.2.1, the race reasoning at L982-L984). The consequences are
  stated at `spec/bindings/mcp.md` L1033 onward:
  - OAC cannot deliver to ephemeral, queue-less-host, subagent or archived threads; each is
    `handoff-failed`;
  - after an interrupted turn, deliveries wait until a turn completes uninterrupted;
  - all Codex delivery rests on one experimental method, behind the G6 shim.

**Already decided by the operator, listed for information, not for re-acceptance:**

- Codex sends nothing until #69: [MCPB-ATT-002] refuses Codex tool calls on both eras
  (operator decision on #46; `spec/bindings/mcp.md` L387). Codex sessions still receive.
- Never steer: [SEC-AUZ-022] (operator decision on #224, written by #274).
- Refuse at once, and OAC never holds messages; stale or withdrawn presence refuses;
  discovery lists `online` sessions only (operator decisions on #43; [SC-DLV-007],
  [IFC-TRN-026]).
- Far-side receipts are optional ([SC-RCP-042], operator decision on #44).
- No inferred reply links: a reply links to a message only when the link is checked, and
  otherwise goes out uncorrelated (operator decision on #44; `spec/session-channels.md`
  §8.2.2 dated note, L2249-L2256).
- The #45 grant decisions: a one-message, 24-hour reply right; explicit grants even on the
  same machine and folder; a 5-minute cap on cross-machine presence; per-session, folder or
  machine grants.
- The #174 decisions: L9 landed before the freeze; senders SHOULD NOT put secrets in
  content; an implementation never looks up memory ([SEC-PRV-016] to [SEC-PRV-018]).

**Also carried forward:**

- **The operations are renamed from #47's outcome list** (§8 row 1). `spec/interfaces.md`
  §9 records each change with its reason:
  - `discover_sessions` → `take_connection` / `watch_attachments`;
  - `attach` → `set_binding`;
  - `publish_output` → `accept_requests`;
  - `announce_presence` → `send_presence`;
  - `deliver` and `capabilities` keep their names, with new shapes.

  #47's issue text keeps the DESIGN names as the conceptual list.
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
| 4. Every fixture executable, with a recorded expected outcome | **Met.** 527 fixtures, each decided by the runner against its recorded expectation. The demonstration covers every code and state (§2 row 5). |
| 5. Versioning and unsupported-capability behaviour stated normatively | **Met.** `spec/session-channels.md` §5 and §6.6. |
| 6. Stage 1 contradictions resolved before the freeze | **Met** (§2 row 10) |

## 6. Version and draft markers, edited at sign-off

These edits are part of the freeze. They are applied in the sign-off commit, not in this
draft:

| File:line (`3e4471e`) | Now | At sign-off |
|---|---|---|
| `spec/session-channels.md` L4-L5 | `**Revision:** 0.1 (draft, Stage 2). Sections 4 and 5 (E1, #41), 6 (E2, #42), 7 (E3, #43) and 8 (E4, #44) are written.` | `**Revision:** 0.1, frozen at Gate S2 on {{SIGN-OFF DATE}} (E7, #47; docs/planning/decisions/E7-interface-freeze.md). Changes follow its §7.` |
| `spec/security.md` L4 | `**Revision:** 0.1 (draft, Stage 2). Written by task E5 (#45).` | `**Revision:** 0.1, frozen at Gate S2 on {{SIGN-OFF DATE}} (E7, #47). Written by task E5 (#45).` |
| `spec/interfaces.md` L5 | `**Revision:** 0.1 (draft, Stage 2). Written by #273 for task E7 (#47).` | `**Revision:** 0.1, frozen at Gate S2 on {{SIGN-OFF DATE}} (E7, #47). Written by #273.` |
| `spec/interfaces.md` L1164-L1167 (§7, the paragraph after Table 7.1, "Once Gate S2 freezes this document ...") | future tense | `Since Gate S2 froze this document on {{SIGN-OFF DATE}} ...`, citing §7 of this record |
| `spec/bindings/mcp.md` L4-L6 | `**Status:** Stage 2 draft, normative once Gate S2 freezes the spec surface (...). Every requirement carries an `MCPB` id; the index in §12 lists its fixtures or marks it `TODO(fixture)`.` | `**Status:** normative; frozen at Gate S2 on {{SIGN-OFF DATE}} (E7, #47). Every requirement carries an `MCPB` id; the index in §12 lists its fixtures or marks it `TODO(fixture)`.` |
| `spec/bindings/mcp.md` L7 | `` `0.1` (draft until Gate S2; fixtures cite `spec_revision` `0.1`) `` | `` `0.1` (frozen; fixtures cite `spec_revision` `0.1`) `` |
| The revision-history appendix of `spec/session-channels.md` (Appendix B), `spec/security.md` (Appendix C) and `spec/interfaces.md` (Appendix B) | rows read `0.1 (draft)` | Add a row `0.1` with `{{SIGN-OFF DATE}}`: "Frozen at Gate S2 (E7, #47)". Earlier rows keep `(draft)` as history. `spec/bindings/mcp.md` has no revision-history appendix, and the sign-off commit does not add one. Its Status line (above) records the freeze. |

The version stays `0.1` and the identifier is unchanged. The freeze makes the existing
revision binding. It is not a new one.

## 7. Change control after the freeze

From sign-off on, the frozen items in §1 change only as follows.

- The skill rule is `oac-spec-authoring` §7.
- The classification is `spec/session-channels.md` §5.2-§5.3.
- `spec/interfaces.md` §7 (L1164-L1167) applies that classification to the contracts. A
  change to a type or an operation there is a recorded amendment. Whether it also needs a
  new minor or major version is decided by §5.2-§5.3 "for the wire forms it touches".

This record adds no rule of its own.

1. **Fixture-only and editorial changes.** These are adding a fixture, replacing a
   `TODO(fixture)`, or an editorial change that leaves every requirement's normative meaning
   unchanged (`spec/session-channels.md` §5.2 items 7 and 8). They are published as §5.2
   provides. Because they change no requirement's meaning, they need no amendment.
2. **Any other change to a requirement, a frozen type or a frozen operation is an
   amendment.** This includes renaming an operation or moving a requirement to another
   owner in `spec/interfaces.md` Appendix C. It follows the `oac-evidence` §6 procedure:
   - record the conflict, with what the frozen text says and what the evidence says;
   - propose a numbered amendment with the old text or signature verbatim, the new text,
     the rationale, and (`oac-spec-authoring` §7) which stage's output it invalidates;
   - land it in `docs/planning/v0.1/03-decisions-and-amendments.md`, and flag it in
     `docs/planning/STATUS.md`.

   It lands only after the operator approves it.
3. **The version consequence is decided per wire form.**
   - A change that touches no wire form, such as an owner move or a rename of an internal
     operation, is an amendment. It needs no new version and no new extension identifier.
   - A change that touches a wire form takes the version that §5.2 or §5.3 assigns to that
     form. A compatible change (§5.2 items 1-6) is a minor revision under the same
     identifier. A breaking change (§5.3, including item 11: an added `MUST` that a `0.1`
     implementation would violate) needs a new major version and a new extension
     identifier (§5.1; C3 §8).
4. **Stage 3 and 4 work implements against the frozen text and does not renegotiate it.**
   A `type:code` item that finds a frozen interface does not fit records a finding and
   proposes an amendment. It does not edit the interface (`oac-spec-authoring` §7;
   `oac-boundaries` boundary 10). A change made only to get a module to pass is a Stage 2
   freeze violation (`10-stages.md` §8).

## 8. Operator sign-off

The freeze happens when the operator signs this section and the sign-off commit applies
§6. Until then, Gate S2 criterion 1 is not met. Before signing, read §1 (what is frozen),
§3 (all blockers closed), §4.2 (the two items for acceptance, the renamed operations, and
the already-recorded decisions, listed for information) and §9 (plain-language
commitments).

| Question | Operator's answer |
|---|---|
| I accept the renamed adapter and transport operations (§4.2; `spec/interfaces.md` §9) in place of the DESIGN names in #47's outcome list | `{{OPERATOR: YES / NO}}` |
| I accept the two items of §4.2 that go beyond recorded decisions: (a) cross-install waits for a transport binding that meets [SC-DLV-066] and [IFC-TRN-080], not only for E5; (b) all Codex delivery goes through `thread/queue/add`, not only when busy, with the consequences listed there | `{{OPERATOR: YES / NO}}` |
| Decision | `{{OPERATOR: FREEZE / NOT YET}}` |
| Frozen at commit (the sign-off commit, which applies §6) | `{{SHA}}` |
| Date | `{{SIGN-OFF DATE}}` |
| Recorded on | `{{link to the operator's comment on #47}}` |

A "NO" on either acceptance row means NOT YET: the item goes back as a spec change before
the freeze.

**The sign-off commit** (made after the operator's answer, not in this draft):

1. Fill in this table and the header's freeze date.
2. Apply every row of §6 to the four `spec/` documents.
3. Change this record's status to **FROZEN**, and the `10-stages.md` §6 "Current verdict"
   to Gate S2 criterion 1 met.
4. STATUS.md: a top entry, with the "Current stage" and "Open epics" cells.
5. Re-run the runner, its `--self-test` and the CI spec step on that commit.

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
  operation needs an amendment you approve, recorded in `03-decisions-and-amendments.md`. A
  change to a wire form that would break a `0.1` implementation also needs a new version
  and a new extension identifier. An internal rename or owner move does not.
- **What stays open after the freeze:**
  - Codex can receive but not send (#69).
  - Traffic stays within one installation until a transport binding carries authenticated
    presence and receipts.
  - Codex delivery depends on one experimental method and never steers.
  - The UNVERIFIED items in §4.1 stay open with their owners. None of them changes an
    interface.
- **What continues regardless:** fixtures can be added and `TODO(fixture)` entries
  replaced without an amendment. They are published as `spec/session-channels.md` §5.2
  items 7-8 provide.

## Boundary and evidence pass

- Nothing here proposes OAC owning a turn loop, holding provider credentials, holding
  messages, or polling (`oac-boundaries` pre-commit self-check).
- Provider and method names appear here only to locate binding text and issues. This
  record is not neutral spec text, and the neutral-vocabulary checks run over `spec/`.
- Every result in §2 was re-run on 2026-10-04 against the merge of `3e4471e`. No
  UNVERIFIED label was dropped, and §4.1 adds none.
