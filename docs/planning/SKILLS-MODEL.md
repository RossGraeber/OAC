# Progressive-disclosure skill model for OAC

**Status:** Proposal · 2026-09-15 · tracked by Epic J in the backlog

## The problem

The OAC plan is roughly 500 lines of ADR, design, and planning prompt, and it will grow into
a thirteen-file planning package plus a normative spec. An agent picking up a work item needs
maybe three percent of that — but the wrong three percent produces a boundary violation, an
invented API name, or a spec document that mentions Zenoh.

Two failure modes to design against:

1. **Under-disclosure.** The agent writes a Claude adapter without knowing that a server
   negotiating MCP `2026-07-28` is silently not registered as a channel. Hours lost.
2. **Over-disclosure.** Everything is loaded into `CLAUDE.md`, the context fills with Zenoh
   ACL trivia while the agent is writing a test, and the boundaries get lost in the noise.

The fix is not "write more documentation". It is to make loading *conditional on the task*,
and to make the condition mechanical rather than a judgement call.

## The model: four tiers

```text
Tier 0  always in context          CLAUDE.md (<40 lines) + skill descriptions
          |                        what OAC is, the boundaries, where to look next
          v
Tier 1  one skill body, on demand  oac (router) — stage status, label -> skill map
          |
          v
Tier 2  two or three skill bodies  a guardrail skill + a stage skill + a surface skill
          |                        <200 lines each; procedures and checklists
          v
Tier 3  reference files, per file  references/*.md, fixtures, templates, transcripts
                                   the expensive verbatim protocol detail
```

Cost intuition: Tier 0 is paid on every turn of every session, so it is capped hard.
Tier 2 is paid once per work item. Tier 3 is paid per question actually asked.

## The skill tree

Fifteen skills: one router, two guardrails, seven stage skills, four surface skills, and one meta skill. A typical work item loads three.

### Tier 1 — router (1)

| Skill | Holds |
|---|---|
| `oac` | Project map, current stage and gate status (read from one status file, not restated), and the label -> skill routing table. Nothing else. |

### Tier 2a — guardrails (2), loaded by almost everything

| Skill | Holds |
|---|---|
| `oac-boundaries` | Every ADR-001 MUST NOT, each with a concrete example of how an agent drifts into it, plus the grep and lint commands that catch the mechanical violations. The "stop, cite the boundary" protocol. |
| `oac-evidence` | First-party sources only; URL + version + retrieval date on every claim; verbatim API names; UNVERIFIED labelling. Appendix B source index as a reference file. |

These two exist because they are the failure modes the planning prompt spends the most words
on — drift into agent-framework territory, and invented API names. They are cheap and they
travel with every task.

### Tier 2b — stage skills (7), one per phase of work

| Skill | Loaded when | Holds |
|---|---|---|
| `oac-planning-package` | `type:docs` in M0 | The §9 file structure, the §11 self-review checklist, house style: decisions not options. |
| `oac-gates` | `type:spike`, any `gate:*` | Gate result template, timebox policy, one reference file per gate G1-G5, fixture capture procedure. |
| `oac-spec-authoring` | `type:spec` | Normative language rules, the neutral-vocabulary rule, conformance fixture format, versioning policy. |
| `oac-implementation` | `type:code` | Workspace conventions, module dependency direction, contract-tests-first, error handling. |
| `oac-security-work` | `area:security` | Threat table format (attack / precondition / mitigation / proving test / residual risk), provenance rendering rules, the "authenticated but untrusted" doctrine. |
| `oac-testing` | `type:test` | The test tier taxonomy, what is CI-default vs opt-in, how to add a fixture, the no-polling assertion. |
| `oac-release` | `stage:6-release` | Packaging, license inventory, gate re-run on version bump, upgrade notes. |

### Tier 2c — surface reference skills (4), loaded only when touching that surface

| Skill | Loaded when | Holds |
|---|---|---|
| `oac-claude-channels` | `area:adapter-claude` | `capabilities.experimental["claude/channel"]`, `notifications/claude/channel`, `meta` identifier-safety, the legacy-MCP constraint, `--channels` loading and the allowlist, permission relay, no-acknowledgement semantics. |
| `oac-codex-appserver` | `area:adapter-codex` | JSON-RPC methods, thread/turn model, the daemon and control socket, `thread/queue/add` vs `turn/steer` vs `turn/start`, auth boundary, schema artifacts. |
| `oac-mcp` | `area:spec`, `gate:G4` | Revision history and the dual-era matrix, SEP-2133 extensions, what MCP cannot do (server-initiated push into model context). |
| `oac-zenoh` | `area:transport-zenoh` | Peer mode, scouting and the >= 1.10.0 loopback fix, ACL subjects, TLS/QUIC, `zid` is not an identity, no payload signing, containment rule. |

These four are where the expensive detail lives, and they are exactly the detail that goes
stale when a pin moves. Each declares the pin it was written against, so a version bump
visibly invalidates it rather than quietly misleading the next agent.

### Tier 2d — meta (1)

`oac-authoring-skills`: how to write and maintain the skills above — the tier budget, the
description-writing rules, when to promote content to a reference file.

## Routing: labels are the trigger

The backlog labels double as the routing key, so an agent does not have to guess what to
load. The `oac` router carries this table:

| Work-item label | Load |
|---|---|
| `type:docs` + M0 | `oac-planning-package`, `oac-evidence` |
| `type:spike` / `gate:*` | `oac-gates`, plus the surface skill for that gate |
| `type:decision` | `oac-evidence`, `oac-boundaries`, plus the relevant surface skill |
| `type:spec` | `oac-spec-authoring`, `oac-boundaries` |
| `type:code` | `oac-implementation`, plus the surface skill for that area |
| `type:test` | `oac-testing` |
| `area:security` | `oac-security-work` |
| `area:adapter-claude` | `oac-claude-channels` |
| `area:adapter-codex` | `oac-codex-appserver` |
| `area:transport-zenoh` | `oac-zenoh` |
| `stage:6-release` | `oac-release` |

Every task issue in the backlog already ends with a **Skills:** line naming its skills, so
the routing is pre-computed per work item and the router is only needed when the agent
arrives without one.

## Disclosure over time, not just over topic

Topic routing is half of it. The other half is *stage*: an agent working during Stage 1 must
not be reading the Stage 4 implementation conventions, because Stage 4 decisions are not made
yet and reading them invites premature commitment.

A single `docs/planning/STATUS.md` holds the current stage, the gate verdicts, and the pinned
versions. The router reads it and narrows what it offers. This is what keeps the risk-first
ordering of §8 honest: an agent cannot accidentally start Stage 4 work while G2 is unresolved,
because the router will not route it there and `oac-gates` says why.

## Budget and enforcement

| Tier | Cap | Enforced by |
|---|---|---|
| `CLAUDE.md` | 40 lines | CI line-count check |
| Skill description | 2 lines | CI check |
| `SKILL.md` body | 200 lines | CI check |
| `references/*.md` | no cap | loaded individually, never wholesale |

Roughly 350 tokens of always-on cost for fifteen descriptions, against a plan that would
otherwise want to be resident in full.

## Maintenance rule

A skill that restates the plan will drift from the plan. So: skills hold **procedure and
constraint**; the planning package and the spec hold **content**; skills link to them by path
rather than copying. The only content that lives inside a skill is version-pinned protocol
detail from a third party, because that is precisely what an agent cannot look up in this
repository — and it carries its pin so staleness is visible.

## Build order

Epic J in the backlog: J1 router and Tier 0, J2 guardrails, J3 stage skills, J4 surface
skills. J1 and J2 are worth building before Stage 0 work starts, since they guard the
evidence standard that Stage 0 is entirely about. J4 depends on B1 — the surface skills
cannot declare a pin before the pins exist.
