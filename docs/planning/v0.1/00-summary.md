# 00 — Summary

One page. This file cites the other twelve `docs/planning/v0.1/` files and
`docs/planning/ADR-001.md`/`docs/planning/DESIGN.md` by repo-relative path; it does not
restate their prose. Naming resolution applied throughout: **Open Agent Channel (OAC)**,
**OAC Session Channels**, binary **`oac`** (`.claude/skills/oac-planning-package/SKILL.md`
§4).

## Structural finding

Neither Claude Code nor Codex supports attaching an external channel to an arbitrary,
already-running session through a supported interface. Both support live, full-duplex
injection into a session **launched OAC-enabled**: Claude Code by starting with
`--channels`, Codex through the shared local `app-server` daemon or an OAC-owned
`app-server` with the TUI attached via `--remote`. Source: `docs/planning/
PLANNING-PROMPT.md` §4; landed as `ADR-001-A2`, `docs/planning/v0.1/
03-decisions-and-amendments.md` §ADR-001-A2, also `docs/planning/ADR-001-AMENDMENTS.md`.
The boundary is unchanged by this finding: OAC still never calls a provider model API in
place of the native harness, never holds a harness's provider credentials, and never owns
a turn loop (`docs/planning/ADR-001.md` — Boundary).

## What v0.1 proves

The `ADR-001.md` line 74 validation criterion, as amended by `ADR-001-A2`
(`docs/planning/v0.1/03-decisions-and-amendments.md` §ADR-001-A2), one leg per line:

- A Claude Code session, launched OAC-enabled, sends over OAC Session Channels and
  Zenoh to a Codex harness session, also launched OAC-enabled.
- Delivery to Codex happens without receiver polling.
- Codex responds, and Claude receives the response actively.
- Neither side invokes or holds credentials for the other's model API.
- Sender identity and authorization are enforceable, not inferred from content.

The ten DESIGN v0.1 acceptance criteria this proves against are reproduced, not
re-derived here, at `docs/planning/v0.1/10-stages.md` §11, each already mapped to a named
test in `docs/planning/v0.1/09-test-strategy.md` §11 and a stage in `docs/planning/v0.1/
10-stages.md`.

## Language and process-model decisions

**Language and runtime.** Rust, built as a single self-contained `oac` binary per host
platform, pinned toolchain `1.98.1`. Decisive reason: the Rust MCP SDK `rmcp` still
defaults to the pinned legacy MCP revision as a server, and first-party Codex app-server
client crates exist only in Rust. Reversal condition: a disqualifying gap in the Rust MCP
SDK's legacy-revision support, or in Windows credential-store access. Rejected
alternatives (Go, Python, TypeScript/Node, C++/JVM), one line each: `docs/planning/v0.1/
03-decisions-and-amendments.md` §Decision 1.

**Process model.** One long-lived per-device `oac` daemon (Zenoh peer, device
identity/keys, policy, Codex app-server client) plus thin `oac mcp-shim` stdio child
processes; local IPC over a Windows named pipe or Unix `AF_UNIX` socket with OS-level
peer authentication. Decisive reason: Claude Code forces a per-session spawned process
regardless of what sits behind it, and Zenoh liveliness must outlive a single session.
Reversal condition: local IPC cannot be peer-authenticated on a supported platform, or a
spawned shim cannot reliably locate the daemon. Rejected alternatives (a per-session
embedded Zenoh peer, a key/peer hybrid, loopback TCP), one line each: `docs/planning/v0.1/
03-decisions-and-amendments.md` §Decision 2.

## Top three risks

Named as the first three rows of `docs/planning/v0.1/11-risks.md` §R1 in that file's own
presentation order, per its §Ranking rule: that row order (G1, G2, G4, G3, G5) "is
presentation order... not itself a ranking," and the two no-fallback gates, G1 and G5,
"carry equal weight regardless of position." This file does not re-rank the five R1 risks.

- **RISK-G1 — Claude wake fails.** Signal: G1 spike records a FAIL against any of its
  five pass criteria. Response: none — go/no-go, no fallback; blocks the Claude adapter
  entirely. `docs/planning/v0.1/11-risks.md` §RISK-G1.
- **RISK-G2 — Codex live inject fails.** Signal: G2 spike records a FAIL on the primary
  (implicit daemon attach) path. Response: run the named fallback — OAC owns the
  app-server, user runs `codex --remote ws://…` — before recording the gate result.
  `docs/planning/v0.1/11-risks.md` §RISK-G2.
- **RISK-G4 — MCP dual-era fails.** Signal: G4 spike records a FAIL against any of its
  five pass criteria, including the legacy-channel-registration criterion. Response: take
  the fallback — two server entry points sharing one core — and record its cost in the
  gate result. `docs/planning/v0.1/11-risks.md` §RISK-G4.

The equal-weight no-fallback gate not in this list, RISK-G5 (Provenance), and RISK-G3
(Zenoh loopback discovery) complete R1; full risk detail, all five tiers, and the
self-review record for the whole package: `docs/planning/v0.1/11-risks.md`,
`docs/planning/STATUS.md` §"Epic A closing self-review".
