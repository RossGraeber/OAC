---
name: oac
description: Router for OAC work. Load when a work item has no Skills line, when arriving without a task, or to find the project map and current stage/gate status. Names which skills a work-item label routes to.
---

Router only. Read `docs/planning/STATUS.md` for current stage, gate verdicts, and pins —
this skill does not restate them, because they change more often than this file does.

## Project map

| Path | Holds |
|---|---|
| `docs/planning/ADR-001.md` | Decisions, boundaries, v0.1 scope, security posture |
| `docs/planning/DESIGN.md` | Components, contracts, envelope, acceptance criteria |
| `docs/planning/PLANNING-PROMPT.md` | Evidence baseline (§3), gates (§4), decisions (§5), output package spec (§9) |
| `docs/planning/SKILLS-MODEL.md` | How this skill tree is organized |
| `docs/planning/STATUS.md` | Current stage, gate verdicts, pins — single source of truth |
| `docs/planning/backlog/*.json` | Labels (`00-meta.json`), epics (`01-epics.json`), tasks |
| `.claude/skills/<name>/SKILL.md` | The skill bodies this table routes to |

## Precedence

A backlog task's own **Skills:** line wins when present — it is pre-computed per task.
Use this router's table only when an agent arrives without one, or needs a label this
router covers that no open task line already names.

## Stage narrowing

Before routing into a stage-specific skill (any `stage:*` row below, or `oac-gates`,
`oac-spec-authoring`, `oac-implementation`, `oac-security-work`, `oac-testing`,
`oac-release`), check `docs/planning/STATUS.md` — "Open epics" and "Blocked". If the
target stage is not open, do not route there and do not proceed into that work anyway;
say which stage is currently open instead and let the agent pick a work item that
belongs to it.

## Label -> skill routing table

`oac-boundaries` and `oac-evidence` are guardrails: load them for almost any task
regardless of the row matched below. The `area:agent-skills` row wins over the generic
`type:docs` row below when both match (Epic J: skill-authoring work is `type:docs` but
is never Epic A planning-package work).

| Work-item label | Load |
|---|---|
| `type:docs` in M0, Epic A planning package — no `stage:*` or `area:agent-skills` label | `oac-planning-package`, `oac-evidence`, plus the relevant surface or area skill (for example `oac-security-work` for `area:security`, `oac-gates` for a gate item) |
| `type:docs` with a `stage:*` label | Do not use the stage's implementation skill. Take the surface skill(s) for its `area:*` label(s), plus `oac-release` when the doc covers launch/delivery. Example: `stage:4-adapters,type:docs,area:cli` -> `oac-claude-channels`, `oac-codex-appserver`, `oac-release`, never `oac-implementation` |
| `type:decision` | `oac-evidence`, `oac-boundaries`, plus the relevant surface skill |
| `stage:0-evidence` | `oac-evidence`, plus the surface skill being pinned; add `oac-gates` too if the item also carries `type:infra` (gate re-run policy) |
| `type:spike`, `stage:1-spikes` | `oac-gates`, plus the surface skill for that gate |
| `gate:G1-claude-wake` (Epic D spike) | `oac-gates`, `oac-claude-channels` |
| `gate:G2-codex-inject` (Epic D spike) | `oac-gates`, `oac-codex-appserver` |
| `gate:G3-zenoh-peer` (Epic D spike) | `oac-gates`, `oac-zenoh` |
| `gate:G4-mcp-dual-era` (Epic D spike) | `oac-gates`, `oac-mcp` |
| `gate:G5-provenance` (Epic D spike) | `oac-gates`, `oac-security-work` |
| `type:spec`, `stage:2-spec`, `area:spec` | `oac-spec-authoring`, `oac-boundaries`, `oac-mcp` |
| `type:code`, `stage:3-core`, `area:core` | `oac-implementation`, plus the surface skill for that area |
| `type:infra`, `area:ci` at `stage:0-evidence` | `oac-gates`, `oac-evidence` |
| `type:infra`, `area:ci` at `stage:3-core` | `oac-implementation`, `oac-testing` |
| `type:infra`, `area:ci` at `stage:6-release` | `oac-release` |
| `stage:4-adapters` | `oac-implementation`, plus `oac-claude-channels` / `oac-codex-appserver` / `oac-zenoh` for the area |
| `area:cli` | `oac-implementation` |
| `type:test` | `oac-testing` |
| `stage:5-e2e` | `oac-testing`, `oac-security-work` |
| `area:security` | `oac-security-work` |
| `area:adapter-claude` | `oac-claude-channels` |
| `area:adapter-codex` | `oac-codex-appserver` |
| `area:transport-zenoh` | `oac-zenoh` |
| `stage:6-release` | `oac-release` |
| `area:agent-skills` | `oac-authoring-skills` |

`epic`, `go-no-go`, `risk:high`, and `blocked-external` are modifiers, not routing keys —
they ride alongside one of the labels above; route on that label instead.

A bare `gate:*` label on a work item that is not itself the Epic D spike is a blocker, not
a load instruction: check that gate's verdict in `docs/planning/STATUS.md` before
proceeding, and do not add `oac-gates` for it. Example: `stage:4-adapters` tasks carrying
`gate:G1-claude-wake` route on `stage:4-adapters` (`oac-implementation` + surface skill),
not on the `gate:G1-claude-wake` row above.

## Where the content lives

- Routing source: `docs/planning/SKILLS-MODEL.md` §"Routing: labels are the trigger"
- Full label list: `docs/planning/backlog/00-meta.json`
- Per-task overrides: `docs/planning/backlog/01-epics.json`, `02-tasks-AB.json`,
  `03-tasks-CD.json`, `04-tasks-EF.json`, `05-tasks-GHIJ.json` (each task's **Skills:** line)
