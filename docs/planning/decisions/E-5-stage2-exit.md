# E-5 — Stage 2 exit: Gate S2 and the Stage 3 go/no-go

- **Date:** 2026-10-06
- **Issue:** #5 (Epic E, Stage 2, milestone M3). Stage 2 has no exit task of its own in
  the backlog (Epic D had D7, #40), so this record is named after the epic and its issue,
  as `K-196-driver-accepts-dialogs.md` is.
- **Gate:** Gate S2 (`docs/planning/v0.1/10-stages.md` §6, L483-L504).
- **Status:** **Decided: go.** Gate S2 holds on all six criteria (§2), every Stage 2 exit
  artifact exists (§1), and Stage 3 (Epic F, #6, milestone M4) opens (§4).
- **Owns:** its own text, the STATUS top entry for this exit, the STATUS "Current stage"
  cells, and the `10-stages.md` §6 "Current verdict" paragraph. It changes no file under
  `spec/` (frozen, E7 §7) and no gate verdict, pin or ADR text.
- **Skills:** `oac-planning-package`, `oac-spec-authoring`, `oac-boundaries`.

This record does not restate the freeze or the proofs. `docs/planning/decisions/E7-interface-freeze.md`
stays authoritative for what is frozen, the readiness audit and the change-control rule.
`docs/planning/decisions/E9-replacement-proofs.md` stays authoritative for the
design-for-replacement proofs. This file adds the Gate S2 checklist re-run on current
`main`, the exit-artifact inventory, the Epic E close-out and the Stage 3 entry check.

Line citations are to `main` at `98ad455` (the PR #306 merge, 2026-10-06) unless a line
says otherwise. Every check in §2 was re-run on that commit for this record.

---

## 1. Stage 2 exit artifacts (`10-stages.md` §6 "Exit artifacts", L466-L481)

| Artifact | Where | Evidence |
|---|---|---|
| The OAC Session Channels specification (E1-E4, E6) | `spec/session-channels.md`, `spec/bindings/mcp.md` | #41, #42, #43, #44 and #46 closed. Both headers read "frozen at Gate S2: signed off 2026-10-06, in force from the merge of PR #276" (`session-channels.md` L4-L5; `bindings/mcp.md` L4-L6). |
| The security specification (E5) | `spec/security.md` | #45 closed. Header L4-L5 carries the same freeze marker. |
| The frozen adapter contract, transport contract and core types (E7) | `spec/interfaces.md` §4-§6 | #47 closed by PR #276 (merge `20482f1`, 2026-10-06, merged by the lead). Header L5-L6 carries the freeze marker. No file under `spec/` has changed since: `git log 20482f1..98ad455 -- spec/` is empty. |
| The conformance fixture set (E8) | `tests/protocol/` (22 area directories, runner in `tests/protocol/runner/`) | #48 closed. 527 fixtures, all decided by the runner (§2, criterion 4). |
| The design-for-replacement proofs (E9) | `docs/planning/decisions/E9-replacement-proofs.md` | #49 closed by PR #306 (merge `98ad455`, 2026-10-06, merged by the lead). All four #49 acceptance items are met (record §7). No amendment to a frozen item is proposed (record §8). |

**The executable demonstration** (`10-stages.md` §6, L458-L464) holds. The runner decides
every fixture against its recorded expectation, and every Table 8.3 error code (17), every
Table 8.1 delivery state (8) and the `ttl_ms`-before-replay-window rule are decided by at
least one fixture (E7 §2 row 5). The fixture set is unchanged since that audit, and the
re-run below still reads 527/527.

## 2. Gate S2 acceptance criteria (`10-stages.md` §6, L483-L498)

E7 §5 recorded criteria 1-6 as met at sign-off. Each is re-checked here against `98ad455`.

1. **The adapter contract, the transport contract and the core types are frozen: holds.**
   - The operator decided FREEZE at revision 0.1 on #47
     (https://github.com/RossGraeber/OAC/issues/47#issuecomment-6007805771, 2026-10-06).
   - PR #276 carries the sign-off commit `c1a6296`. The lead merged it at `20482f1`
     (2026-10-06T02:38Z), which is the approval E7 §7 item 1 and §8 name as the final
     sign-off. GitHub records no formal review on PR #276, because the author and the
     merger are the same account. The merge is the lead's approval.
   - The change rule is E7 §7: a frozen item changes only by a PR with the change and a
     version bump, approved by the lead. This replaces the M0 "spec-revision event under
     `05-interfaces.md` §11" (`10-stages.md` §6 dated note, L527-L533).
   - Nothing has changed under `spec/` since `20482f1` (§1).
2. **No transport or provider vocabulary in neutral spec text: holds.** The
   `boundary-lint.yml` step "Checks 1-2 and spec neutral vocabulary", run verbatim on
   Windows (Git Bash, ripgrep 15.2.0): "checks 1-2 and spec neutral vocabulary clean (4
   spec/ files, 0 core/ files)". It exempts only `spec/bindings/mcp.md` from check 2 and the
   zero-hits group, as the workflow says. The `boundary-lint` workflow on the `98ad455`
   push is `success`.
3. **Normative text is separated from reference-implementation notes: holds.** Each of
   the four documents defines a labelled, informative "Reference implementation note"
   blockquote: `spec/session-channels.md` §2.2 (L57-L64), `spec/security.md` §2.2 (L93-L100),
   `spec/interfaces.md` §2.2 (L106-L112) and `spec/bindings/mcp.md` §1 (L54). Every
   keyword sentence carries a requirement id (`session-channels.md` §3.2), and the runner's
   index checks tie each id to its fixtures or to an owned `TODO(fixture)`.
4. **Every conformance fixture is executable and carries a recorded expected outcome:
   holds.** `node tests/protocol/runner/run.mjs`: "Fixtures: 527/527 pass. Index checks:
   clean." `node tests/protocol/runner/run.mjs --self-test`: "Result: PASS (self-test)".
   The CI `conformance` job runs both. Binding-format fixtures count, by the orchestrator
   ruling recorded on #47 (E7 §2, after the audit table).
5. **Versioning and unsupported-capability behaviour are stated normatively: holds.**
   Versioning is `spec/session-channels.md` §5 (L548-L640): the minor and breaking lists
   (§5.2, §5.3) and the receiver rules [SC-VER-001] to [SC-VER-003] (L625-L632).
   Unsupported-capability behaviour is §6.6 (L920-L951), [SC-ID-100] to [SC-ID-105].
6. **Each Stage 1 result that contradicted a decision produced an amendment before the
   freeze: holds.** Two Stage 1 results contradicted a decision.
   - G5's 2026-09-27 FAIL produced C13 and C6 §5.0 (Option C), landed 2026-10-02 (#220),
     before any Stage 2 work (D7 §5).
   - #224 (Codex `turn/start` steers a running turn) produced the no-steering decision in
     [SEC-AUZ-022] and MCPB-CDX-002 to -005 (#274, PR #278, merged 2026-10-04T07:25Z).
   Both landed before the freeze merge (2026-10-06T02:38Z). E7 §2 row 10 has the detail.

**Go/no-go (`10-stages.md` §6, L500-L504): go.** Criterion 2 holds, so the freeze stands.
Criterion 4 holds, so Stage 3 can load the fixtures as its CI-default spec-conformance tier.

## 3. Epic E close-out

All nine Epic E tasks are closed: #41-#46 (E1-E6), #47 (E7, PR #276), #48 (E8) and #49
(E9, PR #306). So are the E7 blockers #273, #274 and #275, and every other issue labelled
`stage:2-spec` except #308. GitHub lists no sub-issues under #5. The task checkboxes in
#5's body are left for the operator to tick, as D7 left Epic D's.

**#308 is outside Epic E's scope.** It collects editorial amendments to the frozen
`spec/` text (first item: a stale Codex pin pointer in `spec/bindings/mcp.md` §8.2) so they
land together in the next minor version. Under E7 §7 items 1-2 that is a post-freeze change:
one PR with the edits and a version bump of each changed document, approved by the lead. It
is not a Stage 2 deliverable and does not block this exit. It stays open, and it is not
part of #5's task list. Its `stage:2-spec` label names the documents it touches, not
an open Stage 2 task.

## 4. Stage 3 go/no-go (`10-stages.md` §7, L545-L586)

**Go. Stage 2 exits, and Stage 3 (Epic F, #6, milestone M4) opens.** Each §7 entry
condition is met:

- **Gate S2 met (§6): contracts frozen, fixtures executable.** §2 above.
- **Stage 1 fixtures exist (D6).** #39 is closed. The D6 Claude and Codex fixtures are in
  `docs/planning/gates/fixtures/d6-claude-protocol/` and `d6-codex-protocol/`, and
  `node scripts/check-fixture-manifest.mjs` passes at `98ad455` (224 of 224 MANIFEST
  entries match). Its one warning is a version-float notice, never a gate (#216).
- **Gate S0 and Gate S1 both met.** S0 was declared 2026-10-02 (#228; `10-stages.md` §4,
  L273). S1 was met by D7 (#40, closed; `10-stages.md` §5, L367).

The §7 prerequisite decisions (1, 2, 5, 6, 7 and 12) are all made
(`docs/planning/v0.1/03-decisions-and-amendments.md` §1, L69-L76), and Epic C (#3) is
closed. Every dependency Epic F's tasks name outside Epic F is closed:
C1, C4, C5, D6, E1, E3, E4, E7 and E8 (`docs/planning/backlog/04-tasks-EF.json`, F1-F12
`depends`).

Stages 4-6 stay blocked, each behind its own gate. Stage 4 starts only after Gate S3
(`10-stages.md` §7, L608-L626).

## 5. Continuing items (not exit blockers)

None of these can change Gate S2. Each has an owner outside Epic E.

| Item | What it is | Status |
|---|---|---|
| #308 | Editorial amendments to frozen text, batched for the next minor version | Open. Lands under E7 §7 (§3 above). |
| #224 | Codex `turn/start` steers a running turn | Open. The decision is in the frozen text (§2, criterion 6). The issue stays with backlog G7. |
| E7 §4.1 | UNVERIFIED items that touch the interface | Open, each with an Epic F or G owner. None changes an interface. |
| E9 §8, §9 | Findings F-A1 to F-T5, and E9's UNVERIFIED items | Each belongs to a binding document or a deferred item. None is a gap in a frozen interface. |
| #131, #124, #303 | herdr test tooling (Epic K, #123) | Open. Cannot change a gate verdict. |
| #175, #176 | Beacon L10 and L11 (Epic L, #165) | Blocked on Stages 5 and 6. |

Open UNVERIFIED items stay in `docs/planning/STATUS.md` "Open UNVERIFIED items" and
`docs/planning/v0.1/11-risks.md`. This record closes none of them.

## 6. Checks run on this change

On Windows at `98ad455`, then re-run on this branch:

- `node tests/protocol/runner/run.mjs`: 527/527 pass, index checks clean.
  `--self-test`: PASS.
- `node scripts/check-fixture-manifest.mjs`: 224/224 entries match. `--self-test`: 73/73.
- `node scripts/check-herdr-containment.mjs`: PENDING, 0 violations, 6 of 10 targets not
  built yet. PENDING is not a pass. `--self-test`: 102/103, 1 skipped.
- `node scripts/check-skills.mjs`: all 15 skills within budget.
- `node scripts/sync-agents-skills.mjs --check` and `--self-test`: pass.
- `boundary-lint.yml` checks 3, 8, 1-2 with the spec neutral-vocabulary group, and 11,
  each run verbatim from the workflow: all clean (check 11 over 4 tracked files).

---

## Boundary and evidence pass

- No boundary in `oac-boundaries` is touched. This record decides no provider integration,
  credential use, transport vocabulary or model routing, and edits no neutral spec text.
- Every claim cites a repo file and line at `98ad455`, a check run on it, or an issue or
  PR. There are no new external provider claims, so no new retrieval dates and no new
  UNVERIFIED items.
