---
name: oac-authoring-skills
description: How to write and maintain any OAC skill or CLAUDE.md — tier model, line/token budgets, description rules, when to promote content to references/, the link-don't-copy maintenance rule. Load for area:agent-skills work items or any edit under .claude/skills/.
---

Load for `area:agent-skills` work items (Epic J: J1-J4) or any edit touching
`.claude/skills/**` or `CLAUDE.md`. Specification: `docs/planning/SKILLS-MODEL.md`.
This skill is itself bound by the rules below.

## 1. The four tiers and their cost

| Tier | What | Paid |
|---|---|---|
| 0 | `CLAUDE.md` + all skill descriptions | every turn, every session |
| 1 | `oac` router body | once an agent opens it |
| 2 | guardrail + stage + surface skill bodies | once per work item |
| 3 | `references/*.md`, fixtures, templates, transcripts | per question actually asked |

Design every skill against this cost. Tier 0 content must justify being read on
every single turn forever; Tier 3 content only needs to justify being read once,
by the one agent who needed exactly that fact.

## 2. Budget and what enforces it

| Thing | Cap | Enforced by |
|---|---|---|
| `CLAUDE.md` | 40 lines | `node scripts/check-skills.mjs` (CI once wired) |
| `description` frontmatter field | 2 lines / 280 chars | `node scripts/check-skills.mjs` (CI once wired) |
| `SKILL.md` body (after frontmatter) | 200 lines | `node scripts/check-skills.mjs` (CI once wired) |
| `references/*.md` | none | not size-capped; loaded individually, never wholesale |

Aim well under the cap. A 90-line skill that routes correctly beats a 199-line one.
If a skill body is pushing 200 lines, that is a signal to move detail into
`references/`, not to write tighter prose.

## 3. Writing the description

The description is the only text resident in every session for every skill —
fifteen of them, roughly 350 tokens total. It has to let an agent decide to
load the skill without the router explaining it. Rules:

- Say **what the skill holds** and **when to load it** in the same one or two
  lines.
- Name the trigger: work-item labels (`type:code`, `area:transport-zenoh`),
  file paths, or situations ("any edit under `core/`"). A description that
  only summarizes content without naming a trigger forces the router to do
  the work the description should have done.
- Start with a verb phrase or noun phrase. Never "This skill...".
- Quote real label names from `docs/planning/backlog/*.json`, not invented
  categories.

Good: `Zenoh transport facts pinned to 1.10.1 — peer mode, scouting, ACL
subjects, zid is not an identity. Load for area:transport-zenoh work items or
any change touching the transport crate.`

Bad: `Contains information about the Zenoh transport layer used by OAC,
including configuration and security details that may be useful when working
on transport-related tasks.` — no verb-phrase opening, no label, no
concrete facts, tells the router nothing it couldn't guess from the name.

## 4. When to promote content to `references/`

Tier 3 exists for exactly one reason: expensive verbatim detail that only a
minority of visits to the skill will need. Move content to
`.claude/skills/<name>/references/<topic>.md` when it is:

- a full protocol/API transcript or fixture capture;
- a template filled in with an example instead of just the shape;
- per-gate or per-surface detail where a work item only ever needs one of
  several (e.g. one reference file per gate in `oac-gates`);
- long enough that inlining it would blow the 200-line body cap.

Keep in the body: the procedure, the checklist, the routing/matrix table, and
one line pointing at the reference file. Never restate a reference file's
content in the body "for convenience" — that creates two copies to keep in
sync.

## 5. Maintenance rule: skills hold procedure, the plan holds content

A skill that restates the plan will drift from the plan. So:

1. Skills hold **procedure and constraint** — checklists, decision rules,
   commands, what to check.
2. The planning package (`docs/planning/*.md`) and, later, `spec/` hold
   **content** — the actual decisions, contracts, and normative text.
3. A skill links to that content by repo-relative path instead of copying it.
   If a fact changes in the plan, every skill that links to it is still
   correct; a skill that copied it is now wrong and nobody knows.
4. **The one exception:** version-pinned third-party protocol detail (Claude
   Channels, Codex app-server, MCP, Zenoh facts). An agent cannot look this
   up anywhere else in the repository, so it is copied in — and it must carry
   its pin (version + source + retrieval date) so a version bump visibly
   invalidates it rather than quietly misleading the next agent.

When editing a skill, if you're about to type a sentence that restates a
decision, a table, or a threat mitigation that already exists in the planning
package: stop, link to it instead.

## 6. Layout convention

```
.claude/skills/<name>/SKILL.md
.claude/skills/<name>/references/*.md   (optional)
```

```yaml
---
name: oac-example
description: Trigger-rich one-or-two-line summary naming what it holds and when to load it.
---
```

- `name` in frontmatter matches the directory name exactly. `name` and
  `description` are the only frontmatter fields `scripts/check-skills.mjs`
  reads; anything else in frontmatter is ignored by the checker.
- House body structure, in order:
  1. One line stating what the skill is for and the labels/situations that
     load it.
  2. The procedure or checklist — the bulk of the file.
  3. A short "Where the content lives" section of repo-relative links.
  4. Surface skills only: a `## Pin` section (see §7).
- Tables for routing/matrix data, numbered lists for procedures, `- [ ]` for
  checklists. Plain prose, decisions not options, no emoji, no "this document
  describes...".

## 7. Surface skills: the pin section

Every surface skill (`oac-claude-channels`, `oac-codex-appserver`, `oac-mcp`,
`oac-zenoh`) ends with a `## Pin` section naming the exact version the facts
were written against, its first-party source, and the retrieval date, plus
the instruction to re-verify per `oac-evidence` when the pin moves.

Stage 0 (Epic B) has not run yet. The versions in `PLANNING-PROMPT.md` §3 are
a **pre-verified baseline retrieved 2026-09-15**, not confirmed pins. Until
Stage 0 closes:

- A surface skill's `## Pin` section declares that 2026-09-15 baseline, cites
  the exact `PLANNING-PROMPT.md` §3.x subsection as source, and states
  explicitly that Stage 0 must re-verify before the skill is trusted.
- Every `UNVERIFIED` label attached to a §3 fact is preserved verbatim in the
  skill. Never silently promote an `UNVERIFIED` fact to a stated one.
- Current confirmed status of stage and pins lives in `docs/planning/STATUS.md`
  — link to it rather than restating "pre-Stage 0" prose that will go stale.

## 8. Checklist: adding a new skill

- [ ] Confirm it's needed: does an existing skill already cover this, or does
      this duplicate `oac-boundaries` / `oac-evidence` guardrail content?
- [ ] Pick the tier (guardrail / stage / surface / meta) and confirm it
      matches one of the fifteen skills named in `SKILLS-MODEL.md` and listed
      in the `EXPECTED` array in `scripts/check-skills.mjs` — that array is
      the enforced authority; a directory present but missing from it fails
      the checker.
- [ ] Create `.claude/skills/<name>/SKILL.md` with frontmatter `name` matching
      the directory.
- [ ] Add the new skill's name to `EXPECTED` in `scripts/check-skills.mjs`,
      or the checker fails it as undeclared even though the file is correct.
- [ ] Write a trigger-rich description per §3, under 2 lines / 280 chars.
- [ ] Write the body per the house structure in §6, under 200 lines.
- [ ] Every fact traces to a file in this repo (or, for surface skills, to a
      cited first-party source pinned per §7). No invented API names.
- [ ] Move expensive verbatim detail to `references/` per §4.
- [ ] Add the skill to the `oac` router's label -> skill table if it's loaded
      by a label.
- [ ] Run `node scripts/check-skills.mjs` (or wait for CI once wired) before
      calling it done.

## 9. Checklist: reviewing an existing skill

- [ ] Does it restate content that lives in the planning package or spec
      instead of linking to it? (§5)
- [ ] Does every fact have a traceable source — a repo file, or for surface
      skills a cited first-party source with version and retrieval date?
- [ ] For surface skills: is the `## Pin` stale relative to
      `docs/planning/STATUS.md`? Has the version moved since the pin date?
- [ ] Does it duplicate anything already covered by `oac-boundaries` or
      `oac-evidence` instead of linking to them by name?
- [ ] Is the description still trigger-rich, or has it drifted into a
      content summary that doesn't name a label or situation?
- [ ] Is it under the caps in §2?
- [ ] Does anything in it contradict ADR-001's Boundary section?

## Where the content lives

- Specification: `docs/planning/SKILLS-MODEL.md`
- Evidence/pin rules: `.claude/skills/oac-evidence/SKILL.md`
- Current stage and pins: `docs/planning/STATUS.md`
- Acceptance criteria for this skill and its siblings:
  `docs/planning/backlog/05-tasks-GHIJ.json` (Epic J, tasks J1-J4)
- Budget checker: `scripts/check-skills.mjs`
