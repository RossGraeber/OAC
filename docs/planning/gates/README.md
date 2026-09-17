# Gate evidence store — policy

This directory is the evidence store for the five Stage 1 go/no-go gates (G1-G5,
PLANNING-PROMPT.md §4). Pinned versions live in `docs/planning/PINS.md`. Verdict
summary lives in `docs/planning/STATUS.md`'s Gate verdicts table. This directory holds
the evidence each verdict rests on: one result file per gate, plus captured fixtures.

## Naming convention

- Directory: `docs/planning/gates/`.
- Result file: `docs/planning/gates/G<n>-result.md`, one per gate —
  `G1-result.md`, `G2-result.md`, `G3-result.md`, `G4-result.md`, `G5-result.md`.
  No date or version in the filename. The file is rewritten in place on each re-run;
  the prior verdict moves into the file's own "Re-run history" table (see
  "Re-run/invalidation policy" below). Git history is the archive — do not create a
  dated copy.
- Fixtures for gate G<n>: `docs/planning/gates/fixtures/G<n>/` (redacted per
  `oac-gates` §Fixture capture). This is the Stage 1 capture location. The path
  Stage 3's fake Claude/Codex endpoints finally load fixtures from is **not** decided
  here — Stage 3 may relocate or copy them.
- Full command transcripts, when kept: `docs/planning/gates/fixtures/G<n>/transcript-<YYYY-MM-DD>.txt`.
  The result file carries only the summary.
- Slug names are fixed to the backlog gate labels — used in headings, never in
  filenames: G1 `claude-wake`, G2 `codex-inject`, G3 `zenoh-peer`, G4 `mcp-dual-era`,
  G5 `provenance`.

## Gate-result template

Canonical source: `.claude/skills/oac-gates/SKILL.md` §Gate-result template. Copied
here **verbatim**, with three fields this evidence store needs added at the end (the
skill's copy is the base template and stays unextended; this is the only place the
extended form is maintained):

```markdown
### G<n> <name>

- **Gate id:** G<n>
- **Pinned version(s):** <exact versions the spike ran against, e.g. Claude Code v2.1.232,
  Codex 0.154.0, Zenoh 1.10.1 — never "latest">
- **Date:** <YYYY-MM-DD the spike ran>
- **Timebox:** <box set> / <elapsed; expired or not>
- **Command transcript summary:** <what was run and observed, condensed; full transcript, if
  kept, lives with the fixtures, not inline here>
- **Pass criteria evaluated:**
  - [x|f] <criterion 1, verbatim from the gate's reference file> — <observed result>
  - [x|f] <criterion 2> — <observed result>
  - ...
- **Verdict:** PASS | PASS (FALLBACK TAKEN) | FAIL | NOT RUN
- **Fallback taken:** <name it, or "none — not needed" or "none — no fallback exists">
- **UNVERIFIED items:** <each item this gate was positioned to close — closed with evidence,
  or still open>
- **Fixtures captured:** <path(s) under the fixtures location, or "none">
```

Added fields (B4, this evidence store only):

- **`Pin rows relied on:`** — the exact `docs/planning/PINS.md` pin-table row names
  this verdict depends on (e.g. `Claude Code (Channels)`, `MCP — legacy era`). This
  set must be the inverse of that row's `Gates affected` cell: a pin row is relied on
  by gate G<n> if and only if its `Gates affected` cell names G<n>.
- **`PINS.md as-of:`** — the `**Last updated:**` date of `docs/planning/PINS.md` at
  run time, plus the commit SHA of `PINS.md` at that point.
- **`Re-run history:`** — table `| Date | Pinned versions | Verdict | Invalidated by |`.

Required fields from the issue #33 acceptance criteria must all be present and named:
gate id, pinned version(s), date, command transcript summary, verdict, fallback taken.
Verdict vocabulary is closed: `PASS` | `PASS (FALLBACK TAKEN)` | `FAIL` | `NOT RUN` —
no fifth value, no hedge (`oac-gates` "never probably" rule).

## Re-run/invalidation policy

A version bump to any pin in `docs/planning/PINS.md` visibly, mechanically invalidates
the gate results that depend on it. This is not a judgement call.

a. **The rule.** Changing any cell of any row in the `docs/planning/PINS.md` pin table
   (version, release date, or the row's presence) invalidates every gate named in that
   row's **`Gates affected`** column. Invalidation is immediate and independent of
   whether the spike would still pass if re-run.

b. **Required same-change edits.** When a pin moves, all three of the following land
   in the **same commit** as the pin move:
   1. Each affected `docs/planning/gates/G<n>-result.md` gets:
      - its `**Verdict:**` set to `NOT RUN`,
      - the superseded verdict appended to its `Re-run history` table with
        `Invalidated by: <surface> pin <old> -> <new>, <YYYY-MM-DD>`,
      - a `> INVALIDATED` callout at the top of the file.
   2. The `docs/planning/STATUS.md` Gate verdicts row for that gate reverts to
      `NOT RUN`.
   3. `docs/planning/PINS.md`'s `**Last updated:**` is bumped.

c. **The partial move is forbidden.** A commit that edits the PINS.md pin table
   without the step-b edits is incomplete and must be rejected in review. This is the
   **pin-move checklist** — a copy of it lives in `docs/planning/PINS.md` itself
   (its "Pin-move checklist" section) so a reviewer sees it beside the table being
   changed.

d. **ACP exemption.** The ACP row's `Gates affected` cell is `none`, so an ACP pin
   move invalidates nothing. This is read directly off the column — not a judgement
   call.

e. **Cross-reference to fact re-verification.** Per `oac-evidence` §7, a pin move also
   triggers §3 re-verification (Epic B2, `docs/planning/REVERIFICATION-B2.md`). Gate
   re-run (this policy) and fact re-verification (`oac-evidence` §7) are two separate
   obligations of the same trigger — a pin move requires both, not one in place of the
   other.

## Volatility note — the two preview surfaces most likely to move

Per `oac-evidence` §4, every preview/experimental surface carries its stability label
at first mention. Both surfaces below already carry a pinned version and a named (or
explicitly unnamed) shim boundary in `docs/planning/PINS.md`; this section only names
which are likeliest to force a re-run.

- **Claude Code Channels — `research preview`** (`v2.1.274`; Channels floor
  `v2.1.232`, permission-relay floor `v2.1.234` — per `docs/planning/PINS.md`, Claude
  Code Channels, retrieved 2026-09-16). Affects **G1** and **G4** (legacy-MCP
  negotiation). Expect the most frequent invalidation here: Claude Code ships releases
  at high cadence and the channel surface is preview.
- **Codex CLI / app-server — `experimental` (per-method gating)**
  (`@openai/codex@0.154.0`, commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` — per
  `docs/planning/PINS.md`, Codex CLI and app-server, retrieved 2026-09-16). Affects
  **G2** and **G5**. The daemon-attach question is an open UNVERIFIED item resolved
  only by the G2 spike against the pinned build.
- Contrast: Zenoh, MCP revisions, and the Rust toolchain are `supported` and move on
  slower, announced cadences.
- Both preview surfaces still have `shim boundary: UNNAMED — see DESIGN.md`
  (conflict-register C11). This is not resolved here — cited only.
- Every version string above is cited per `docs/planning/PINS.md`. Invent no new API
  name; if one is needed, it must already appear in `PINS.md` or
  `docs/planning/PLANNING-PROMPT.md` §3.

## Reconciliation with PLANNING-PROMPT.md §9 item 3

`.claude/skills/oac-gates/SKILL.md` and PLANNING-PROMPT.md §9 item 3 both state that
G1-G5 land in **one** file, `docs/planning/v0.1/02-gating-findings.md`. Issue #33
proposes per-gate files instead. This is a conflict between a settled deliverable
(§9 item 3) and this task's evidence-store design — recorded per `oac-evidence` §6 as
a conflict note. It is not an ADR-001 claim, so no ADR amendment is proposed.

- **What §9 item 3 says:** "`02-gating-findings.md` — G1 to G5 with pass, fail,
  fallback, and current verdict," as one of the thirteen files under
  `docs/planning/v0.1/`.
- **What this task proposes:** per-gate files under `docs/planning/gates/`.
- **Reconciliation:** the per-gate files (`docs/planning/gates/G<n>-result.md`) are
  the **evidence store of record** — the place a gate re-run actually writes.
  `docs/planning/v0.1/02-gating-findings.md` is a **derived summary**, assembled from
  the five per-gate files, one section per gate (verdict, date, pins, a link back to
  the per-gate file). §9's deliverable is not dropped — it is made generated-from
  rather than hand-authored. Assembling it is Epic A's task, not B4's (see
  "Do not create `docs/planning/v0.1/`" below).
- This directory (`docs/planning/gates/`) did not exist before this change; Epic A
  reads it once it exists.

`docs/planning/v0.1/` is Epic A's deliverable and Epic A is the currently open epic.
B4 does not create it — B4 only records that `02-gating-findings.md`, once written,
is derived from these files.

## Where the content lives

- Gate definitions, pass/fail criteria, fallback table: PLANNING-PROMPT.md §4.
- Timebox policy, throwaway rule, per-gate reference files: `.claude/skills/oac-gates/SKILL.md`.
- Stage 0/Stage 1 entry-exit criteria and the fixture-capture rationale: PLANNING-PROMPT.md §8.
- The one-file gating-findings deliverable this policy reconciles with: PLANNING-PROMPT.md §9 item 3.
- Pinned versions, `Gates affected` column, pin-move checklist: `docs/planning/PINS.md`.
- Gate verdicts table, pins-relied-on/result-file columns: `docs/planning/STATUS.md`.
- Gate procedure, template, fixture capture, exit criteria: skill `oac-gates`.
