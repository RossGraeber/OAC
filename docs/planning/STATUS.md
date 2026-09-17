# OAC status

The single source of truth for where the project is. The `oac` router skill reads this file
rather than restating it. Update it when a stage opens or closes, when a gate returns a
verdict, or when a pin moves.

**Last updated:** 2026-09-17 (C2: process model/local IPC/CLI surface/config model
decision landed, see `docs/planning/decisions/C2-process-model.md`; C1:
language/runtime/packaging/dependency-inventory decision landed, see
`docs/planning/decisions/C1-language-runtime.md`; Rust MCP SDK (`rmcp`) pin added; B4:
gate re-run policy and evidence store; B3: conflict register resolved, ADR-001
amendments A1-A3 issued)

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

| Gate | Verdict | Decides | Pins relied on | Result file |
|---|---|---|---|---|
| G1 Claude wake | NOT RUN | Claude adapter viability. go/no-go, no fallback. | Claude Code (Channels); MCP — current era; MCP — legacy era; Rust MCP SDK (rmcp) | `docs/planning/gates/G1-result.md` |
| G2 Codex live inject | NOT RUN | Codex adapter viability. Fallback: OAC-owned app-server with `codex --remote`. | Codex CLI / app-server | `docs/planning/gates/G2-result.md` |
| G3 Zenoh local peer | NOT RUN | Loopback peer discovery on Windows, macOS, Linux. Fallback: fixed local endpoint, no scouting. | Zenoh; Rust toolchain | `docs/planning/gates/G3-result.md` |
| G4 MCP dual-era server | NOT RUN | One process serving both MCP eras. Fallback: two entry points, one core. | Claude Code (Channels); MCP — current era; MCP — legacy era; Rust MCP SDK (rmcp) | `docs/planning/gates/G4-result.md` |
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
| Rust MCP SDK | `rmcp` `3.4.0` (2026-09-15); legacy revision `2025-11-25` supported and is the SDK's default | PINS.md — Rust MCP SDK (`rmcp`) |

## Decisions landed

- **C1 — language, runtime, packaging, dependency inventory** (issue #13): decided.
  Rust, single self-contained binary (dynamically linked against OS system libraries
  only — not bit-for-bit static, see the file's §9). Full decision, evidence, and
  dependency inventory: `docs/planning/decisions/C1-language-runtime.md`. Folds into
  `docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4) once that file
  exists.
- **C2 — process model, local IPC, CLI surface, config model** (issue #14): decided.
  One long-lived per-device `oac` daemon (Zenoh peer, device identity/keys, policy,
  Codex app-server client) plus thin `oac mcp-shim` stdio child processes; Windows named
  pipe / Unix `AF_UNIX` socket IPC with OS-level peer authentication (candidate crate
  `interprocess` `2.4.4` pinned; final IPC crate a Stage 3 detail, see C2 §4); zero-file
  local default. Full decision and evidence:
  `docs/planning/decisions/C2-process-model.md`. Folds into
  `docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4) once that file
  exists.
- **C3 — spec packaging and the MCP extension identifier** (issue #16): decided.
  Standalone normative document (`OAC Session Channels`, under `spec/`, normative-of-
  record) plus MCP extension identifier `io.github.rossgraeber/oac-session-channels`
  for capability negotiation, tool surface, and `_meta` provenance only; MCP is not the
  delivery mechanism. Full decision and evidence:
  `docs/planning/decisions/C3-spec-packaging.md`. Folds into
  `docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4) once that file
  exists.

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

## Open conflict-register items

Verified, directly observable open work items from the conflict register — not
UNVERIFIED claims (per `oac-evidence` §5, that list is for claims no first-party source
states or that are inferred/stale). Closed when the named resolution lands.

- C11: the named compatibility shim boundary for the Claude Code Channels
  research-preview surface and the Codex experimental live-inject surface is UNNAMED
  (register entry restating the two shim-boundary rows in "Open UNVERIFIED items"
  above; see `ADR-001-AMENDMENTS.md` "New register entries"). Owner: a C-series decision
  or a DESIGN.md update.
- C12: `DESIGN.md` still carries the retired names (`sessionchannels`, "Session
  Channels", "MCP Session Channels extension") after ADR-001-A1 (exact sites listed in
  `ADR-001-AMENDMENTS.md` "Carried to later tasks"). Owner: Epic A task A9 plus a
  DESIGN.md follow-up edit.

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

- Zenoh's default TLS stack being `rustls` rather than OpenSSL (UNVERIFIED — carried
  from PLANNING-PROMPT.md §3.4 unchanged; not independently re-fetched from Zenoh's own
  `Cargo.toml`/feature docs; see `docs/planning/decisions/C1-language-runtime.md` §9).
- Whether an `rmcp`-based OAC server, run end-to-end against a live Claude Code
  instance with `MCP_PROTOCOL_NEGOTIATION=legacy`, actually registers as a channel
  (UNVERIFIED — SDK capability verified, runtime behaviour is gate G4's job, verdict
  `NOT RUN`; see `docs/planning/decisions/C1-language-runtime.md` §5, §13).
- Whether the Windows `windows-native-keyring-store` `keyring` backend has been
  exercised end-to-end against live Windows Credential Manager (UNVERIFIED — declared
  feature/build target verified only; runtime confirmation belongs to a future
  `oac-implementation`/`oac-testing` task; see
  `docs/planning/decisions/C1-language-runtime.md` §8, §12, §13).
- Whether `oac mcp-shim`, spawned by Claude Code as a child stdio process, inherits an
  environment sufficient to locate the daemon's IPC path without extra configuration
  (UNVERIFIED — depends on Claude Code's channel-spawn environment passthrough, not
  established by any cited source; see
  `docs/planning/decisions/C2-process-model.md` §10, §11).
- Whether the Codex daemon's implicit attach is enabled by default in released
  `0.154.0`, plus a possible documentation-drift signal: a 2026-09-17 re-fetch of
  `https://learn.chatgpt.com/docs/app-server` did not surface the `codex app-server
  daemon start` command or the `app-server-control.sock` control-socket path that
  PLANNING-PROMPT.md §3.2's pre-verified baseline states (UNVERIFIED — already an open
  item per gate G2/task D2; this is one added data point, not a resolution; see
  `docs/planning/decisions/C2-process-model.md` §2, §10).
- Named-pipe DACL peer-authentication behaviour not yet exercised on a live Windows
  host (UNVERIFIED — API shape verified against Microsoft Learn only; see
  `docs/planning/decisions/C2-process-model.md` §4, §10, §11).
- Whether `interprocess` `2.4.4` (or an alternative IPC crate) exposes a first-party
  peer-credential accessor (UNVERIFIED — not surfaced in the fetched crate docs; the
  daemon is expected to call the raw OS API directly instead; see
  `docs/planning/decisions/C2-process-model.md` §4, §10).

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
