---
name: oac-planning-package
description: Procedure and house style for writing the docs/planning/v0.1/ output package. Load for any work item labelled type:docs in milestone M0 (Epic A tasks A1-A12), and for the final self-review before closing Epic A.
---

Produces `docs/planning/v0.1/`: thirteen self-contained, cross-referenced files that are
the build phase's blueprint. Loaded when a work item carries `type:docs` and sits in
milestone M0. Content lives in `docs/planning/PLANNING-PROMPT.md` (§2, §9, §10, §11) and
in ADR-001 / DESIGN; this skill does not restate that content, only the procedure for
producing files against it. For evidence sourcing use `oac-evidence`; for ADR-001 MUST NOTs
use `oac-boundaries` — both loaded alongside this skill for `type:docs` + M0 per
`docs/planning/SKILLS-MODEL.md`.

## 1. The thirteen files (PLANNING-PROMPT §9)

All under `docs/planning/v0.1/`. One line each — the job, not the content.

| # | File | Job |
|---|---|---|
| 1 | `00-summary.md` | One page: what v0.1 proves, the §4 structural finding, the language and process-model decisions, top three risks. |
| 2 | `01-capability-matrix.md` | One row per capability across Claude Code, Codex, MCP, Zenoh, ACP with status/version/source/date/gap. |
| 3 | `02-gating-findings.md` | G1-G5 pass/fail/fallback criteria and current verdict. |
| 4 | `03-decisions-and-amendments.md` | Every §5 decision made; ADR-001-A1 onward; the resolved Appendix A conflict register. |
| 5 | `04-architecture.md` | Components, process boundaries, inbound/outbound data and control flow per provider, deployment topologies. Diagrams in text. |
| 6 | `05-interfaces.md` | OAC spec surface, adapter contract, transport contract, core types, normative vs reference-implementation text. |
| 7 | `06-security.md` | Identities, trust boundaries, pairing, authorization, transport security, replay and duplicate handling, provenance rendering, threat table. |
| 8 | `07-repository-and-dependencies.md` | Module layout, ownership boundaries, third-party dependency and license inventory. |
| 9 | `08-cli-and-deployment.md` | Zero-container local launch story per harness; LAN hooks only where they shape v0.1 architecture. |
| 10 | `09-test-strategy.md` | Test tiers, what runs in CI by default vs opt-in. |
| 11 | `10-stages.md` | The §8 stage-gate pipeline. |
| 12 | `11-risks.md` | Risks ranked by ability to invalidate v0.1, each with an early-warning signal and a response. |
| 13 | `12-deferred.md` | Every ADR non-goal and v0.1 exclusion restated as an explicit "not in v0.1" with reason. |

Each file is self-contained: a reader opening only that file must be able to follow it,
which means cross-referencing other package files and repo docs by repo-relative path
rather than assuming prior context.

## 2. House style (PLANNING-PROMPT §10) — apply to every file

- **Decisions, not options.** If a choice is routine (PLANNING-PROMPT §5's own framing),
  make it and record the rejected alternatives in one line each plus the reversal
  condition. Do not leave it open as "either X or Y."
- **Concise beats complete-looking.** A short list of verified facts and decisions outranks
  a polished-sounding section that pads. Cut restatement of ADR-001/DESIGN prose; cite the
  section instead.
- **Plain prose.** No tickets, no code. Pseudo-code is allowed only as interface signatures
  or message shapes (used in `05-interfaces.md`), never as illustrative code blocks.
- **Self-contained, cross-referenced by path.** Link to `docs/planning/ADR-001.md`,
  `docs/planning/DESIGN.md`, other `v0.1/*.md` files, and `docs/planning/PLANNING-PROMPT.md`
  sections by repo-relative path — never assume the reader already loaded them.
- **Separate normative from reference-implementation text.** In every interface section
  (chiefly `05-interfaces.md`), state OAC's normative semantics (MUST/SHOULD/MAY) apart from
  any note about how the reference implementation happens to realize them. A reader must be
  able to tell which parts bind a future non-Rust implementation and which do not.
- **Smallest implementation that proves v0.1.** When two designs both satisfy the ADR,
  choose the one with fewer moving parts and say so in the deciding file (typically
  `03-decisions-and-amendments.md`).
- **Diagrams in text.** Any architecture diagram (chiefly in `04-architecture.md`) is
  rendered as text (e.g. an ASCII/box diagram or a labelled flow list), not an embedded
  image.

## 3. Naming resolution (PLANNING-PROMPT §2) — fixed, apply everywhere

Resolved once, recorded as **ADR-001-A1**, then used identically in all thirteen files and
never reopened within the package:

- Product and repository: **Open Agent Channel (OAC)**.
- Normative protocol specification: **OAC Session Channels** (short form: "the OAC spec").
- CLI binary: **`oac`**, unless a conflict with an existing widely used tool is found —
  state the collision check performed, and its result, in
  `03-decisions-and-amendments.md` alongside ADR-001-A1.

Never write "Session Channels" or `sessionchannels` (the ADR-001/DESIGN legacy names) or any
other spelling in new package text; where an old file is quoted verbatim for context, mark
the quote as pre-rename.

## 4. Amendment procedure

When verified evidence contradicts a settled ADR-001 decision, do not silently redesign
around it. Instead:

1. Record the conflict in `03-decisions-and-amendments.md`: what the current text says,
   which evidence contradicts it (cite per `oac-evidence`), and where the conflict surfaced.
2. Propose a numbered amendment — `ADR-001-A1`, `A2`, `A3`, … in strict sequence, never
   reusing or skipping a number — with three fields: **old text**, **new text**,
   **rationale**.
3. Leave dependent decisions blocked on the amendment rather than assuming it will be
   accepted; do not build downstream package text on an amendment still in draft.

This is also how the Appendix A conflict register (C1-C10 and anything discovered while
writing the package) gets resolved: each entry becomes either an amendment or an explicit
note that no amendment is needed and why.

## 5. Self-review before declaring the package done (PLANNING-PROMPT §11, verbatim)

Run this checklist; fix whatever fails before closing Epic A.

- [ ] Every ADR-001 boundary and every DESIGN non-goal is either respected or has a numbered amendment.
- [ ] Every DESIGN v0.1 acceptance criterion (1 to 10) maps to a named test in `09-test-strategy.md` and a stage in `10-stages.md`.
- [ ] Every §5 decision is made, not deferred, or has a gate whose result decides it.
- [ ] Every gate G1 to G5 has pass, fail, and fallback text.
- [ ] Every §3 fact you rely on has been re-verified against the pinned version, and every UNVERIFIED item is closed or listed as a risk.
- [ ] No neutral interface mentions Zenoh, Claude, Codex, MCP method names, or key expressions.
- [ ] No section proposes owning a harness's turn loop, holding provider credentials, or polling an inbox from an adapter that claims active inbound.
- [ ] The naming resolution in §2 is applied consistently.

For boundary/non-goal detail behind item 1, and the mechanical grep checks behind items 6-7,
use `oac-boundaries` rather than re-deriving them here. For the citation/UNVERIFIED standard
behind item 5, use `oac-evidence`.

## 6. Exit criteria for a work item using this skill

A `type:docs` / M0 work item (an Epic A task, per `docs/planning/backlog/02-tasks-AB.json`)
is done when:

- The named file exists under `docs/planning/v0.1/` with the content its task's Acceptance
  checklist describes, and nothing in it restates ADR-001/DESIGN prose that a path reference
  would serve instead.
- House style (§2 above) holds throughout the file: no open routine choices, no code beyond
  interface signatures/message shapes, normative text separated from reference-implementation
  notes wherever interfaces are described.
- The naming resolution (§3) is applied with no stale spelling.
- Any conflict the task surfaced is either resolved as a numbered ADR-001 amendment in
  `03-decisions-and-amendments.md` or explicitly noted as not needing one.
- If this is the last remaining Epic A task, the full §5 self-review checklist has been run
  against the whole package, not just the one file, and every item passes.

## Where the content lives

- `docs/planning/PLANNING-PROMPT.md` §2 (naming), §9 (output package), §10 (style),
  §11 (self-review), Appendix A (conflict register).
- `docs/planning/ADR-001.md`, `docs/planning/DESIGN.md` — source content the package draws on.
- `docs/planning/backlog/02-tasks-AB.json` — Epic A task bodies and per-file Acceptance checklists.
- `docs/planning/STATUS.md` — current stage and whether M0 is still open.
- `oac-evidence` — citation and UNVERIFIED-labelling standard.
- `oac-boundaries` — ADR-001 MUST NOTs and the mechanical checks behind self-review items 1, 6, 7.
