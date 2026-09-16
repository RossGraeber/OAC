# OAC

Provider-neutral **MCP Session Channels** so existing AI harnesses (Claude Code, Codex,
Cursor, ...) can push messages into each other's live sessions, without OAC becoming an
agent harness or model router. See `docs/planning/ADR-001.md`.

## Inviolable boundaries (ADR-001)

OAC MUST NOT:

- call provider model APIs as a substitute for a native harness
- implement inference, model routing, or context management
- steal or reuse another harness's provider credentials
- depend on UI/terminal scraping or undocumented private RPCs for supported integrations
- leak Zenoh-specific concepts into the neutral protocol

Full detail, drift examples, and the grep/lint checks: skill `oac-boundaries`.

## Where the plan lives

- `docs/planning/ADR-001.md` — decisions, boundaries, scope
- `docs/planning/STATUS.md` — current stage, gate verdicts, pins (read before acting)
- `docs/planning/backlog/` — labelled work items; each ends with a **Skills:** line

## Finding the right skill

A work item's own **Skills:** line is authoritative. Missing one, or working outside the
backlog? Load skill `oac` — it holds the project map and the label -> skill routing table.
