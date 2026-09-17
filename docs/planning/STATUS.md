# OAC status

The single source of truth for where the project is. The `oac` router skill reads this file
rather than restating it. Update it when a stage opens or closes, when a gate returns a
verdict, or when a pin moves.

**Last updated:** 2026-09-16 (B4: gate re-run policy and evidence store)

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

| Gate | Verdict | Decides | Pins relied on | Result file |
|---|---|---|---|---|
| G1 Claude wake | NOT RUN | Claude adapter viability. go/no-go, no fallback. | Claude Code (Channels); MCP — current era; MCP — legacy era | `docs/planning/gates/G1-result.md` |
| G2 Codex live inject | NOT RUN | Codex adapter viability. Fallback: OAC-owned app-server with `codex --remote`. | Codex CLI / app-server | `docs/planning/gates/G2-result.md` |
| G3 Zenoh local peer | NOT RUN | Loopback peer discovery on Windows, macOS, Linux. Fallback: fixed local endpoint, no scouting. | Zenoh; Rust toolchain | `docs/planning/gates/G3-result.md` |
| G4 MCP dual-era server | NOT RUN | One process serving both MCP eras. Fallback: two entry points, one core. | Claude Code (Channels); MCP — current era; MCP — legacy era | `docs/planning/gates/G4-result.md` |
| G5 Provenance | NOT RUN | Machine-set provenance contradicts a spoofing claim on both providers. | Codex CLI / app-server; Claude Code (Channels) | `docs/planning/gates/G5-result.md` |

Re-run/invalidation policy (what moves a verdict back to `NOT RUN`, and the pin-move
checklist): `docs/planning/gates/README.md`.

## Pins

Confirmed. Detailed record, sources, and constraint floors: `docs/planning/PINS.md`.

| Surface | Pinned version | Source |
|---|---|---|
| Claude Code | `v2.1.274` (Channels research preview; permission relay `>= v2.1.234` satisfied) | PINS.md — Claude Code Channels |
| MCP | current `2026-07-28`; legacy `2025-11-25` | PINS.md — MCP revisions |
| Codex CLI | `@openai/codex@0.154.0`, commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` (2026-09-09) | PINS.md — Codex CLI and app-server |
| Zenoh | `1.10.1` (2026-09-07); `>= 1.10.0` required for loopback discovery | PINS.md — Zenoh |
| ACP | protocol version `1` (schema v2 alpha); not a v0.1 dependency | PINS.md — ACP |
| Rust toolchain | `1.98.1` (2026-09-03); `rust-toolchain.toml` enforces it | PINS.md — Rust toolchain |

## Open conflicts (oac-evidence §6)

- **PLANNING-PROMPT.md §9 item 3 vs. the per-gate evidence store (issue #33).** §9
  item 3 states G1-G5 land in one file, `docs/planning/v0.1/02-gating-findings.md`.
  This task's evidence-store design instead makes `docs/planning/gates/G<n>-result.md`
  (one file per gate) the record, with `02-gating-findings.md` generated from those
  five files rather than hand-authored. Not an ADR-001 claim, so no ADR amendment is
  proposed. Full reconciliation: `docs/planning/gates/README.md` §"Reconciliation with
  PLANNING-PROMPT.md §9 item 3". Per `oac-evidence` §6 step 5, this is flagged here
  because `docs/planning/v0.1/03-decisions-and-amendments.md` does not exist yet; move
  this entry there once it does.

## Open UNVERIFIED items

Carried from PLANNING-PROMPT.md §3; closing them is Stage 0 work. Until closed, no plan or
skill may rely on them without an UNVERIFIED label.

- Claude channel behaviour across `--resume`; more than one logical channel per server;
  a `CLAUDE_SESSION_ID` environment variable; Agent SDK support for Channels.
- Whether implicit Codex daemon attach is in released 0.154.0 (commit
  `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`) or only on `main` (UNVERIFIED — G2
  go/no-go, resolved by task D2, not by B1).
- Whether Codex Desktop exposes the control socket in current builds.
- Zenoh public-key auth semantics; the 5-15 MB binary size estimate.
- The named compatibility shim boundary for the Claude Code Channels preview surface
  (UNVERIFIED — DESIGN.md names no such module; see PINS.md "Open questions carried
  into B2").
- The named compatibility shim boundary for the Codex experimental live-inject surface
  (UNVERIFIED — same reason; see PINS.md "Open questions carried into B2").
- SEP-2133's stated finalization date (PLANNING-PROMPT.md §3.3 says 2026-01-26; the SEP
  page itself shows only a creation date and a Final status badge) (UNVERIFIED — see
  PINS.md "Open questions carried into B2").
- SEP-2133's reserved-prefix rule for extension prefixes whose second label is
  `modelcontextprotocol` or `mcp` (UNVERIFIED — not independently re-confirmed verbatim
  on the SEP-2133 page this pass; see PINS.md).
- ACP schema v2 "alpha" status (UNVERIFIED — carried from PLANNING-PROMPT.md §3.5 only,
  not independently re-confirmed on agentclientprotocol.com this pass; low priority,
  ACP is not a v0.1 dependency).
- Zenoh crate version/date read from GitHub releases rather than crates.io directly,
  because the crates.io page did not return content this pass (UNVERIFIED — re-confirm
  on crates.io when reachable; see PINS.md).
