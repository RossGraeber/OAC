# E7 — Interface freeze record (Gate S2)

- **Date:** drafted 2026-10-04. Freeze date: `{{SIGN-OFF DATE}}`, filled in at operator
  sign-off.
- **Issue:** #47 (E7, Epic E #5).
- **Gate:** Gate S2, the interface-freeze half (`docs/planning/v0.1/10-stages.md` §6,
  criteria 1-6).
- **Status:** **PROPOSED, not frozen.** This record prepares the freeze for the operator.
  It does not declare it. Section 3 lists the blockers that stand today. Freezing is the
  operator's decision. It is recorded in §8, and §6 and §7 take effect only after that.
- **Skills:** `oac-spec-authoring` (§7, the interface freeze), `oac-boundaries`,
  `oac-evidence`.

Line citations are to `main` at `94f5053` (the PR #270 merge, 2026-10-03), plus this
change's own edits where marked. Every result in §2 was re-run for this record. None is
carried over from an earlier report.

---

## 1. What the freeze covers

When signed off, these are frozen at revision `0.1`:

| Item | Where | Size at `94f5053` |
|---|---|---|
| OAC Session Channels protocol | `spec/session-channels.md` (§4-§8, Appendix A) | 245 requirement ids, 70 with a `TODO(fixture)` part |
| Security model | `spec/security.md` (§4-§13, Appendix A) | 107 requirement ids, 33 with a `TODO(fixture)` part |
| MCP binding | `spec/bindings/mcp.md` (§2-§8, §12.3) | 53 requirement ids, 23 with a `TODO(fixture)` part |
| `ProviderAdapter`, `Transport`, core neutral types | **not yet written** (blocker B1, #273) | none |
| Extension identifier | `io.github.rossgraeber/oac-session-channels`, major version 0 (`spec/session-channels.md` §5.1 L566-L568) | 1 |
| Requirement-id prefixes | `SC`, `SEC`, `MCPB` (`spec/session-channels.md` §3.2; `spec/bindings/mcp.md` §12.1) | 3 |
| Fixture formats | `oac-conformance-fixture/1`, `oac-mcpb-fixture/1` | 461 fixtures, all citing `spec_revision` `0.1` |

The conformance fixtures are not frozen. A fixture can be added, and a `TODO(fixture)`
replaced, after the freeze without changing the interface (`spec/session-channels.md` §5.2
item 7). That is how Stage 3 works.

## 2. Readiness audit

| # | Check | Result | Evidence |
|---|---|---|---|
| 1 | Conformance runner, every fixture plus index cross-checks | **PASS** | `node tests/protocol/runner/run.mjs`: "Fixtures: 461/461 pass. Index checks: clean." |
| 2 | Runner self-test (published Ed25519 and JCS vectors) | **PASS** | `node tests/protocol/runner/run.mjs --self-test`: "Result: PASS (self-test)" |
| 3 | Every cited requirement id resolves | **PASS** | The scratch audit found every `[SC-…]`, `[SEC-…]` and `[MCPB-…]` citation in the three documents defined, out of 405 defined ids. Every repository path quoted in backticks under `spec/` exists. |
| 4 | Every `MUST`/`MUST NOT` has a fixture, or a `TODO(fixture)` with an owner outside Stage 2 | **FAIL** | SC and SEC pass after this change (SC-ID-080, below). `spec/bindings/mcp.md` §12.3 has 22 `TODO(fixture)` rows with no owner (L1051-L1103) → **B4, #275**. Seven rows say "covered by" another requirement's fixtures, which is acceptable: SC-ID-020, SC-DLV-040, SC-DLV-064, SC-RCP-032, SEC-KEY-002, SEC-SIG-020, SEC-RPL-001. |
| 5 | Every Table 8.3 error code and Table 8.1 state decided by a fixture (`10-stages.md` §6, L458-L464) | **FAIL** | All 8 states are decided. 15 of 17 codes are decided, `invalid-request` only by binding fixtures. `envelope-too-large` appears in no fixture. `transport-failure` appears only as input (`sc-rcp/SC-RCP-085.p06`) → **B3, #275**. |
| 6 | Neutral vocabulary over `spec/` (binding exempt) | **PASS** | Checks 1-2, the zero-hits group and the read-the-hit group all exit 1 (zero hits). |
| 7 | No open placeholders, "planned" text or stale stubs | **PASS after this change** | `spec/security.md` Appendix B listed nine follow-ups as open, but all nine were applied by #266. This change adds a dated note recording that. The §8.1.5 "carriage ... still to come" (L2155-L2158, L2187-L2190) is true and belongs to B1. |
| 8 | Draft markers and versions are ready to finalize | **READY** | The markers are listed in §6. Version `0.1` and the identifier are final. No fixture cites a different `spec_revision`. |
| 9 | The frozen interfaces exist as normative text | **FAIL** | → **B1, #273** |
| 10 | Stage 1 results that contradicted a decision have produced an amendment or decision (Gate S2 criterion 6) | **FAIL** | C13 is resolved (C6 §5.0). The #224 operator decision is not reflected in the spec → **B2, #274** |
| 11 | Open UNVERIFIED items that affect the interface are listed with owners | **PASS** | §4.1 |
| 12 | Other CI checks (`boundary-lint.yml`) | **PASS** | Check 3 and check 8 report zero hits. Check 11 is clean. Checks 9-10 report PENDING, which is not a pass, because no product paths exist yet. Fixture manifest: 211/211 entries match, self-test 73/73. `scripts/check-skills.mjs`: all 15 skills are within budget. `sync-agents-skills.mjs --check` and `--self-test` pass. |

Editorial fixes made in this change (no requirement added or changed):

- `spec/session-channels.md` Appendix A, SC-ID-080. Its `TODO(fixture)` named only E3 and
  E6, both closed Stage 2 tasks. It now names F6 and F10, like the other issuer-side rows
  of §7.2.5.
- `spec/security.md` Appendix B. A dated note records that the nine follow-ups are applied.
- `docs/planning/STATUS.md` "Open epics" row (stale since E1) and the top entry.
  `docs/planning/v0.1/10-stages.md` §6 gets a "Current verdict" paragraph.

## 3. Blockers

| # | Blocker | Where | Kind | Issue |
|---|---|---|---|---|
| B1 | `ProviderAdapter`, `Transport` and the core types have no normative text. The only text is the M0 draft (`docs/planning/v0.1/05-interfaces.md` §13-§15, L466-L672), and it contradicts the merged specs: a descriptor working directory against [SC-ID-045], a different capability shape, receipt members missing, a presence record that is the consumer's state rather than the §7.2.2 record, and no receipt carriage. | `05-interfaces.md` L466-L672; `spec/session-channels.md` L788, L1452, L2074, L2155-L2158 | Substantive | #273 |
| B2 | The #224 operator decision (no steering; queue when busy; race-free form; no override fields) is not in the spec. [SEC-AUZ-022] covers only operations a harness *documents* as steering, and the binding lists `turn/steer` and `turn/start` with no selection rule. Widening [SEC-AUZ-022] later would be a breaking change (`spec/session-channels.md` §5.3 item 5). | `spec/security.md` L1003-L1010; `spec/bindings/mcp.md` L99-L101, L867-L887 | Substantive | #274 |
| B3 | `envelope-too-large` and `transport-failure` have no fixture that decides them, which fails the Gate S2 executable demonstration. | `spec/session-channels.md` Table 8.3 (L2324 onward) | Substantive (fixtures only, no interface change) | #275 |
| B4 | 22 MCP binding `TODO(fixture)` rows have no owner. Several can be decided from a single exchange, contrary to §12.2's own rule. | `spec/bindings/mcp.md` L1043-L1045, L1051-L1103 | Substantive (index and fixtures, no interface change) | #275 |
| B5 | **Operator decision:** L9 (#174) would add the memory-reference doctrine to `spec/security.md`. Any new `MUST` it adds that a 0.1 implementation could violate is breaking after the freeze (§5.3 item 11). Either land it before the freeze, or decide that it waits for the next major version. | #174 | Decision | none (#174 exists) |

B1 to B4 must close before sign-off. B3 and B4 only add fixtures and owners, so the
operator may choose to let them close after the freeze. They still block the Gate S2
criterion 4 demonstration, and with it Stage 3 entry.

## 4. Open items that do not block the freeze

### 4.1 UNVERIFIED items touching the interface (`docs/planning/STATUS.md` "Open UNVERIFIED items")

| Item | Where | Effect on the interface | Owner |
|---|---|---|---|
| Which OS facility yields a pairing key on each platform | `spec/session-channels.md` §6.7.2 note, L1004-L1008 | None. Until it is established every signal is unpairable ([SC-ID-125]), which fails closed. | Epic F adapter work, G9 |
| Whether a pairing mechanism can attribute a dropped signal | L1164-L1167 | [SC-ID-154] is conditional on it | F10, G9 |
| Order of several inputs queued on the second harness surface | L1384-L1388 | None. Informative evidence only. | G7 (#68) |
| How well the v0.1 transport carries presence records | L1700-L1706 | B1's `announce_presence` must carry the full record (also the C7 §4 gap) | G1/G2 (Zenoh), #273 |
| `verify_strict` gives the spec's verdicts on every `sec-sig` fixture | `spec/security.md` L576 | None. A library risk, not an interface risk. | F3, F4 |
| OS peer authentication on each platform | `spec/security.md` L1023-L1024 | None ([SEC-AUZ-030] fixed) | G9 |
| MCP binding items (tool-result `_meta` surfacing, `rmcp` legacy channel, Codex default client era, Codex per-request session signal, Codex connection multiplexing, legacy clients ignoring `extensions`) | `spec/bindings/mcp.md` §10, L936-L941 | Binding only. Each is already carried with an owner there. | #65, #69, RISK-G4 |

### 4.2 Decisions already made that the freeze carries forward

- **Codex outbound is refused until #69.** [MCPB-ATT-002] refuses every Codex tool call on
  both eras until a documented Codex pairing exists (`spec/bindings/mcp.md` L382-L390,
  L875-L877). Codex sessions still receive. Lifting the refusal is a binding minor
  revision that relaxes a refusal. It is not a breaking change to the neutral spec.
- **Cross-install is off until the E5 behaviours and a transport binding exist.** Presence,
  discovery and sending stay within one implementation (`spec/session-channels.md` §7.2.3
  note L1558-L1564, §7.3.2 notes L1785-L1797). `spec/security.md` §11 now supplies the
  authentication half. The carriage half waits for B1's `Transport` text and a transport
  binding that meets [SC-DLV-066]. Receipts between implementations wait the same way
  (§8.1.5).
- **#224, Codex steering:** the decision is made (queue when busy, race-free form) but is
  not yet in the spec. It is blocker B2, #274.
- **Far-side receipts are optional in v0.1** ([SC-RCP-042], operator decision on #44).
- **C11** (the shim-boundary module name) stays with Epic F/G implementation. It names a
  module, not an interface.
- **E9** (#49, design-for-replacement proofs) depends on this freeze. It is a Stage 2 exit
  artifact but not a Gate S2 criterion.

## 5. Gate S2 criteria (`10-stages.md` §6)

| Criterion | Status |
|---|---|
| 1. Adapter contract, transport contract and core types frozen | **Not met.** B1. Sign-off is in §8. |
| 2. No transport or provider vocabulary in neutral spec text | **Met** (§2 row 6). It must be re-run over B1's text. |
| 3. Normative text separated from reference-implementation notes | **Met** for the three documents. Notes are labelled blockquotes, and the runner's index check ties every keyword sentence to an id. It must be re-checked for B1's text. |
| 4. Every fixture executable, with a recorded expected outcome | **Met** for the 461 fixtures. The demonstration is short by two codes (B3). |
| 5. Versioning and unsupported-capability behaviour stated normatively | **Met.** `spec/session-channels.md` §5 and §6.6. |
| 6. Stage 1 contradictions resolved before the freeze | **Not met.** B2. |

## 6. Version and draft markers, edited at sign-off

These edits are part of the freeze. They are applied in the sign-off commit, not in this
draft:

| File:line (`94f5053`) | Now | At sign-off |
|---|---|---|
| `spec/session-channels.md` L4-L5 | `**Revision:** 0.1 (draft, Stage 2). Sections 4 and 5 (E1, #41), 6 (E2, #42), 7 (E3, #43) and 8 (E4, #44) are written.` | `**Revision:** 0.1, frozen at Gate S2 on {{SIGN-OFF DATE}} (E7, #47; docs/planning/decisions/E7-interface-freeze.md). Changes follow its §7.` |
| `spec/security.md` L4 | `**Revision:** 0.1 (draft, Stage 2). Written by task E5 (#45).` | `**Revision:** 0.1, frozen at Gate S2 on {{SIGN-OFF DATE}} (E7, #47). Written by task E5 (#45).` |
| `spec/bindings/mcp.md` L4-L5 | `**Status:** Stage 2 draft, normative once Gate S2 freezes the spec surface ...` | `**Status:** normative; frozen at Gate S2 on {{SIGN-OFF DATE}} (E7, #47).` |
| `spec/bindings/mcp.md` L7 | `` `0.1` (draft until Gate S2; fixtures cite `spec_revision` `0.1`) `` | `` `0.1` (frozen; fixtures cite `spec_revision` `0.1`) `` |
| B1's interface document | (not yet written) | The same frozen marker |
| Each Appendix B / C revision table | rows read `0.1 (draft)` | Add a row `0.1` with `{{SIGN-OFF DATE}}`: "Frozen at Gate S2 (E7, #47)". Earlier rows keep `(draft)` as history. |

The version stays `0.1` and the identifier is unchanged. The freeze makes the existing
revision binding. It is not a new one.

## 7. Change control after the freeze

From sign-off on, the frozen items in §1 change only as follows. The skill rule is
`oac-spec-authoring` §7, and the classification is `spec/session-channels.md` §5.2-§5.3.

1. **Fixture-only and editorial changes.** Adding a fixture, replacing a `TODO(fixture)`,
   or an editorial change that leaves every requirement's meaning unchanged (§5.2 items 7
   and 8). These need a revision-history row and nothing more. The revision stays `0.1`.
2. **Any other change to a requirement or a frozen signature is a recorded amendment.**
   It is filed as an issue and logged below as `E7-A<n>` with four fields: old text or
   signature (verbatim), new text or signature, rationale, and which stage's output it
   invalidates. It lands only after the operator approves it on that issue.
   - A compatible change (§5.2 items 1-6) bumps the minor version (`0.2`, …) under the
     same extension identifier.
   - A breaking change (§5.3, including any added `MUST` that a `0.1` implementation would
     violate) needs a new major version and a new extension identifier (§5.1, C3 §8).
3. **Stage 3 and 4 work implements against the frozen text and does not renegotiate it.**
   A `type:code` item that finds a frozen interface does not fit records a finding and
   proposes an amendment. It does not edit the interface (`oac-spec-authoring` §7;
   `oac-boundaries` boundary 10). A change made only to get a module to pass is a Gate S2
   freeze violation (`10-stages.md` §8, L677).

### Amendment log

None.

## 8. Operator sign-off

The freeze happens when the operator fills in this section and the sign-off commit
applies §6. Until then, Gate S2 criterion 1 is not met.

- Blockers B1-B4 closed, or B3/B4 explicitly deferred past the freeze: `{{OPERATOR: list
  or "all closed"}}`
- B5 (#174) decision: `{{OPERATOR: land before freeze / wait for next major}}`
- Decision: `{{OPERATOR: FREEZE / NOT YET}}`
- Frozen at commit: `{{SHA}}`
- Date: `{{SIGN-OFF DATE}}`
- Recorded on: `{{link to the operator's comment on #47}}`

## 9. What freezing commits you to (plain language)

- **The wire format and rules stop moving.** The envelope, ids, presence records,
  delivery states, error codes, signing and replay rules, and the four MCP tools become
  fixed at `0.1`. Stage 3 and 4 code is built against them as written.
- **Any later change costs something and is visible.** A fix to a rule needs an
  amendment you approve. A change that would break a `0.1` implementation also needs a new
  version and a new extension identifier, so old and new peers can tell each other apart.
- **The adapter and transport interfaces become fixed too.** Adapters cannot call a
  transport directly. Every optional transport feature has to be declared. That text does
  not exist yet (B1), so you would be freezing something you have not seen. Do not sign
  off before #273 merges.
- **What freezing does not settle:** Codex can receive but not send until #69. All traffic
  stays within one installation until a transport binding carries authenticated presence
  and receipts. The steering rule (#274) has to be written before the freeze, because
  tightening it afterwards counts as a breaking change. The UNVERIFIED items in §4.1 stay
  open with their owners. None of them changes the interface.
- **What continues regardless:** fixtures can be added and `TODO(fixture)` entries
  replaced at any time without an amendment.

## Boundary and evidence pass

- Nothing here proposes OAC owning a turn loop, holding provider credentials, or polling
  (`oac-boundaries` pre-commit self-check). B2 tightens the no-steering rule.
- Provider and method names appear here only to locate binding text and issues. This
  record is not neutral spec text, and the neutral-vocabulary checks run over `spec/`.
- Every result in §2 was re-run for this record on 2026-10-04. No UNVERIFIED label was
  dropped, and §4.1 adds none.
