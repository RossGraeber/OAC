# OAC status

The single source of truth for where the project is. The `oac` router skill reads this file
rather than restating it. Update it when a stage opens or closes, when a gate returns a
verdict, or when a pin moves.

**Last updated:** 2026-09-16

## Current stage

| | |
|---|---|
| Milestone | M0 — Planning package v0.1 |
| Stage | Pre-Stage 0. The §9 planning package is not yet written. |
| Open epics | A (planning package), C (decisions), J (agent skills) |
| Blocked | Stages 1-6. No substantial core or transport code starts before Stage 0 and Stage 1 complete. |

## Gate verdicts

No gate has been run. Every verdict below is `NOT RUN`, and every task labelled `gate:*`
is blocked until the corresponding spike in Epic D executes.

| Gate | Verdict | Decides |
|---|---|---|
| G1 Claude wake | NOT RUN | Claude adapter viability. go/no-go, no fallback. |
| G2 Codex live inject | NOT RUN | Codex adapter viability. Fallback: OAC-owned app-server with `codex --remote`. |
| G3 Zenoh local peer | NOT RUN | Loopback peer discovery on Windows, macOS, Linux. Fallback: fixed local endpoint, no scouting. |
| G4 MCP dual-era server | NOT RUN | One process serving both MCP eras. Fallback: two entry points, one core. |
| G5 Provenance | NOT RUN | Machine-set provenance contradicts a spoofing claim on both providers. |

## Pins

**None of these are confirmed pins.** They are the pre-verified baseline recorded in
`PLANNING-PROMPT.md` §3, retrieved 2026-09-15. Pinning them is Stage 0 / Epic B work.

| Surface | Baseline version | Source |
|---|---|---|
| Claude Code | v2.1.232+ (Channels research preview); permission relay v2.1.234+ | PLANNING-PROMPT.md §3.1 |
| MCP | current `2026-07-28`; legacy `2025-11-25` and earlier | PLANNING-PROMPT.md §3.3 |
| Codex CLI | `@openai/codex` 0.154.0 (2026-09-09) | PLANNING-PROMPT.md §3.2 |
| Zenoh | 1.10.1 (2026-09-07); >= 1.10.0 required for loopback discovery | PLANNING-PROMPT.md §3.4 |
| ACP | protocol version `1` (schema v2 alpha) | PLANNING-PROMPT.md §3.5 |
| Rust toolchain | not pinned | Stage 0 |

## Open UNVERIFIED items

Carried from PLANNING-PROMPT.md §3; closing them is Stage 0 work. Until closed, no plan or
skill may rely on them without an UNVERIFIED label.

- Claude channel behaviour across `--resume`; more than one logical channel per server;
  a `CLAUDE_SESSION_ID` environment variable; Agent SDK support for Channels.
- Whether implicit Codex daemon attach is in released 0.154.0 or only on `main` (G2 go/no-go).
- Whether Codex Desktop exposes the control socket in current builds.
- Zenoh public-key auth semantics; the 5-15 MB binary size estimate.
