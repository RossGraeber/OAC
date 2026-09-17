# OAC status

The single source of truth for where the project is. The `oac` router skill reads this file
rather than restating it. Update it when a stage opens or closes, when a gate returns a
verdict, or when a pin moves.

**Last updated:** 2026-09-16 (B3: conflict register resolved, ADR-001 amendments A1-A3 issued)

## Current stage

| | |
|---|---|
| Milestone | M0 — Planning package v0.1 |
| Stage | Pre-Stage 0. The §9 planning package is not yet written. |
| Open epics | A (planning package), C (decisions), J (agent skills) |
| Blocked | Stages 1-6. No substantial core or transport code starts before Stage 0 and Stage 1 complete. |

## ADR amendments

ADR amendments: A1-A3 issued, see `docs/planning/ADR-001-AMENDMENTS.md`. Resolves
conflict register entries C1-C3 directly (`RESOLVED-HERE`); C4-C10 assigned or
resolved-by-evidence per that file's conflict register table; new entries C11-C12 added,
both open (see below). `docs/planning/ADR-001.md` carries a one-line pointer to the
amendments file; its body text is unchanged.

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

Confirmed. Detailed record, sources, and constraint floors: `docs/planning/PINS.md`.

| Surface | Pinned version | Source |
|---|---|---|
| Claude Code | `v2.1.274` (Channels research preview; permission relay `>= v2.1.234` satisfied) | PINS.md — Claude Code Channels |
| MCP | current `2026-07-28`; legacy `2025-11-25` | PINS.md — MCP revisions |
| Codex CLI | `@openai/codex@0.154.0`, commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` (2026-09-09) | PINS.md — Codex CLI and app-server |
| Zenoh | `1.10.1` (2026-09-07); `>= 1.10.0` required for loopback discovery | PINS.md — Zenoh |
| ACP | protocol version `1` (schema v2 alpha); not a v0.1 dependency | PINS.md — ACP |
| Rust toolchain | `1.98.1` (2026-09-03); `rust-toolchain.toml` enforces it | PINS.md — Rust toolchain |

## Open UNVERIFIED items

Carried from PLANNING-PROMPT.md §3, re-verified against the B1 pins in B2
(`docs/planning/REVERIFICATION-B2.md`). Until closed, no plan or skill may rely on them
without an UNVERIFIED label.

- Claude channel behaviour across `--resume`/`--continue` (UNVERIFIED — docs silent at
  v2.1.274; see REVERIFICATION-B2.md §3.1 box 1).
- Whether one MCP server can present more than one logical channel (UNVERIFIED — docs
  silent at v2.1.274; see REVERIFICATION-B2.md §3.1 box 2).
- Whether implicit Codex daemon attach executes by default at runtime in the pinned
  release `0.154.0` (commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`) (UNVERIFIED for
  runtime behaviour — the code path is now source-confirmed present at this commit, see
  REVERIFICATION-B2.md §3.2 box 4; runtime verdict is G2 go/no-go, owned by task D2, not
  by B2).
- Whether Codex Desktop exposes the control socket in current builds (UNVERIFIED — no
  first-party statement found; see REVERIFICATION-B2.md §3.2 box 5).
- Zenoh `auth.pubkey` semantics (UNVERIFIED — see REVERIFICATION-B2.md §3.4 box 6; the
  six key names themselves are now CLOSED, confirmed verbatim in `DEFAULT_CONFIG.json5`
  at tag 1.10.1).
- The 5-15 MB Zenoh binary size estimate (UNVERIFIED — derived estimate, resolved by the
  first G3 build artifact, task D3; see REVERIFICATION-B2.md §3.4 box 7).
- The named compatibility shim boundary for the Claude Code Channels preview surface
  (UNVERIFIED — DESIGN.md names no such module; out of scope for B2, needs a C-series
  decision or a DESIGN.md update; see REVERIFICATION-B2.md "Carried to 11-risks.md").
- The named compatibility shim boundary for the Codex experimental live-inject surface
  (UNVERIFIED — same reason; see REVERIFICATION-B2.md "Carried to 11-risks.md").
- ACP schema v2 "alpha" status (UNVERIFIED — carried from PLANNING-PROMPT.md §3.5 only,
  not independently re-confirmed on agentclientprotocol.com in B1 or B2; low priority,
  ACP is not a v0.1 dependency).
- Zenoh crate version/date read from GitHub releases rather than crates.io directly,
  because the crates.io page did not return content in B1 and was not re-attempted in B2
  (UNVERIFIED — re-confirm on crates.io when reachable; see PINS.md).
- `codex mcp-server` deprecation date (2026-08-20) and deletion date (2026-09-05)
  (UNVERIFIED — carried unchanged from PLANNING-PROMPT.md §3.2, not independently
  re-confirmed against the CLI reference in B1 or B2; see REVERIFICATION-B2.md §3.2
  table).
- No SEP or working-group item for agent-to-agent messaging (UNVERIFIED — carried
  unchanged from PLANNING-PROMPT.md §3.3, not independently re-searched against the SEP
  index in B1 or B2; see REVERIFICATION-B2.md §3.3 table and "Carried to 11-risks.md"
  item 12).
- "Research preview on Claude Code v2.1.232+" floor (UNVERIFIED — not confirmable on
  `channels.md` at `v2.1.274`; `2.1.232` does not appear in its fetched text; see
  REVERIFICATION-B2.md §3.1 box 7 and PINS.md floor 1).
- Whether MCP `experimental` capabilities still exist at the current era `2026-07-28`
  (UNVERIFIED — re-labelled from HOLDS in B2; the prior inference cited Claude Code's own
  client capability, not the `2026-07-28` schema itself, and Claude Code does not
  register a channel server negotiating `2026-07-28`; see REVERIFICATION-B2.md §3.3
  table and "Carried to 11-risks.md" item 13).
- C11: the named compatibility shim boundary for the Claude Code Channels
  research-preview surface and the Codex experimental live-inject surface is UNNAMED
  (same item as the two shim-boundary rows above, restated as a register entry in B3; see
  `ADR-001-AMENDMENTS.md` "New register entries"). Owner: a C-series decision or a
  DESIGN.md update.
- C12: `DESIGN.md` still carries the retired names (`sessionchannels`, "Session
  Channels", "MCP Session Channels extension") after ADR-001-A1 (see
  `ADR-001-AMENDMENTS.md` "Carried to later tasks"). Owner: Epic A task A9 plus a
  DESIGN.md follow-up edit.

**Closed in B2** (removed from this list; see REVERIFICATION-B2.md "Closed UNVERIFIED
items" for citations): Agent SDK does not support Channels (confirmed absent from the
Agent SDK's own capability table); SEP-2133's finalization date `2026-01-26` (confirmed
via its PR's `merged_at`); no documented `CLAUDE_SESSION_ID` environment variable
(confirmed absent from `hooks.md`, `session_id` is the supported path); Codex issue
#21743 status (confirmed still open, no drift to the "no attach" premise). The floor-1
`>= v2.1.232` sentence was re-checked and remains UNVERIFIED, not closed — still not
present on `channels.md` verbatim — see REVERIFICATION-B2.md §3.1 box 7; it stays in the
"Open UNVERIFIED items" list above. SEP-2133's reserved-prefix rule for extension
prefixes whose second label is `modelcontextprotocol` or `mcp` is **drift, not
UNVERIFIED** — no such clause exists in the SEP text; see REVERIFICATION-B2.md Drift
register D2.
