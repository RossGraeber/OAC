# 11 — Risks

This file is self-contained: every citation below is a repo-relative path plus
section, per `.claude/skills/oac-planning-package/SKILL.md` §2. Gate pass/fail/
fallback text stays owned by `docs/planning/v0.1/02-gating-findings.md`; stage
ordering and timeboxes stay owned by `docs/planning/v0.1/10-stages.md`; threat
rows stay owned by `docs/planning/v0.1/06-security.md`; license findings stay
owned by `docs/planning/v0.1/07-repository-and-dependencies.md`. This file does
not restate any of that text, only cites it.

## Ranking rule

Risks are ranked by **ability to invalidate v0.1**, judged against two fixed
references:

- `docs/planning/ADR-001.md` line 74's validation criterion — v0.1 is proven when
  an existing Claude Code session sends a message, over OAC Session Channels and
  Zenoh, to an existing Codex harness session without receiver polling, Codex
  responds and Claude receives the response actively, neither side invokes or
  holds credentials for the other's model API, and sender identity/
  authorization are enforceable rather than inferred from content (source text
  at that line still reads pre-rename "Session Channels/Zenoh," per
  `docs/planning/STATUS.md`'s note that `ADR-001.md`'s body is unchanged; this
  file uses the ADR-001-A1 name).
- `docs/planning/DESIGN.md` "v0.1 acceptance criteria" 1-10 (verbatim list
  reproduced, not re-derived, in `docs/planning/v0.1/10-stages.md` §11).

A reader can re-derive the tier order below from one rule: **a risk that removes
an ADR-001-required leg of the validation criterion, with no fallback, outranks
a risk that only degrades a preview surface, which outranks a risk that is only
a citation-source or pin-provenance gap.**

- **R1 — gate-decided viability risks.** G1, G2, G3, G4, G5
  (`docs/planning/v0.1/02-gating-findings.md` §3-§7). Each gate's `NOT RUN`
  verdict decides a named leg of the ADR-001 validation criterion or a DESIGN
  acceptance criterion outright; G1 and G5 have no fallback.
- **R2 — preview/experimental surface drift.** The Claude Code Channels research
  preview and the Codex experimental live-inject surface, including their
  unnamed compatibility-shim boundaries (conflict-register C11,
  `docs/planning/v0.1/03-decisions-and-amendments.md` §4). These surfaces are
  load-bearing for the R1 gates but are not themselves gate verdicts — they are
  the ground the gates stand on.
- **R3 — evidence/pin drift.** Facts pinned at a specific version or schema era
  that a future pin move or re-fetch could silently invalidate, without changing
  the gate mechanics themselves.
- **R4 — design-parameter and platform-runtime risks.** OAC's own chosen
  parameters (above a stated floor) and platform-specific runtime behaviour not
  yet exercised live. These affect Stage 3/4 implementation quality, not v0.1
  viability.
- **R5 — low-impact / non-dependency risks.** Facts that inform packaging,
  citation hygiene, or transports/protocols not in the v0.1 dependency set.
  Ranked last because nothing in the ADR-001 validation criterion or the ten
  DESIGN acceptance criteria depends on them.

Every risk row below carries four mandatory fields — **risk**, **what it
invalidates**, **early-warning signal**, **response** — and every response names
a trigger-to-action link, not a bare mitigation, per the issue #32 acceptance
list.

## R1 — Gate-decided viability risks

### RISK-G1 — Claude wake fails

- **Risk.** The Claude Code Channels research-preview surface does not wake an
  idle session as a user turn, or loses mid-turn message ordering, or the
  `--dangerously-load-development-channels` consent dialog cannot actually be
  exercised.
- **What it invalidates.** `docs/planning/ADR-001.md` line 74's validation
  criterion (the Claude-sends-and-Codex-receives leg depends on Claude waking at
  all); `docs/planning/DESIGN.md` acceptance criteria 4 and 6; the Claude adapter
  (`adapters/claude/`) entirely.
- **Early-warning signal.** G1 spike (task D1) records a `FAIL` against any of
  its five pass criteria in `docs/planning/gates/G1-result.md`, per
  `docs/planning/v0.1/02-gating-findings.md` §3.
- **Response.** None — go/no-go, no fallback. Per
  `docs/planning/v0.1/02-gating-findings.md` §3, failure "blocks the Claude
  adapter entirely," and per `docs/planning/v0.1/10-stages.md` §2, "a FAIL on G1
  or G5 does not reorder the pipeline — it stops it."

### RISK-G2 — Codex live inject fails

- **Risk.** Neither the implicit daemon-attach path nor the `codex --remote`
  fallback delivers an actively-injected message into a live Codex session
  without OAC holding OpenAI credentials.
- **What it invalidates.** `docs/planning/DESIGN.md` acceptance criteria 5 and 8;
  the Codex-responds leg of `docs/planning/ADR-001.md` line 74's validation
  criterion; the Codex adapter (`adapters/codex/`).
- **Early-warning signal.** G2 spike (task D2) records a `FAIL` on the primary
  (implicit attach) path in `docs/planning/gates/G2-result.md`, per
  `docs/planning/v0.1/02-gating-findings.md` §4.
- **Response.** Run the named fallback before recording the gate result — OAC
  owns the app-server, user runs `codex --remote ws://…`
  (`docs/planning/v0.1/02-gating-findings.md` §4). Failure of both paths is v0.1
  go/no-go and stops the pipeline per `docs/planning/v0.1/10-stages.md` §2.

### RISK-G4 — MCP dual-era fails

- **Risk.** One process cannot serve both the legacy (`2025-11-25`) Claude-
  channel path and the current (`2026-07-28`) MCP path concurrently without one
  degrading the other, or the `rmcp` SDK does not register a legacy-era live
  channel at runtime (`docs/planning/STATUS.md` "Open UNVERIFIED items" —
  "`rmcp`-based OAC server ... registers as a channel").
- **What it invalidates.** `docs/planning/DESIGN.md` acceptance criterion 9's
  single-process assumption; Decision 3's dual-era element
  (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 3); conflict-
  register row C5.
- **Early-warning signal.** G4 spike (task D4) records a `FAIL` against any of
  its five pass criteria — including the legacy-channel-registration criterion —
  in `docs/planning/gates/G4-result.md`, per
  `docs/planning/v0.1/02-gating-findings.md` §6.
- **Response.** Take the fallback — two server entry points sharing one core —
  and record the fallback's cost (extra process, duplicated negotiation code) in
  the gate result (`docs/planning/v0.1/02-gating-findings.md` §6).

### RISK-G3 — Zenoh loopback discovery fails

- **Risk.** Multicast-scouting loopback peer discovery fails on Windows, macOS,
  or Linux at the pinned Zenoh `1.10.1`.
- **What it invalidates.** `docs/planning/DESIGN.md` acceptance criteria 1 and 3;
  the zero-container local deployment story
  (`docs/planning/v0.1/08-cli-and-deployment.md` §2).
- **Early-warning signal.** G3 spike (task D3) records a `FAIL` for a platform's
  multicast path in `docs/planning/gates/G3-result.md`, per
  `docs/planning/v0.1/02-gating-findings.md` §5.
- **Response.** Take the fallback per failing platform — fixed local rendezvous
  endpoint, multicast scouting disabled — recorded as `PASS (FALLBACK TAKEN)`
  for that platform (`docs/planning/v0.1/02-gating-findings.md` §5).

### RISK-G5 — Provenance fails

- **Risk.** Sender provenance can be forged from message content on Claude or
  Codex, or a non-identifier-safe `meta` key silently carries a security-
  relevant attribute.
- **What it invalidates.** `docs/planning/DESIGN.md` acceptance criterion 6
  directly — `docs/planning/v0.1/02-gating-findings.md` §7 states this verbatim;
  the "sender identity/authorization are enforceable rather than inferred from
  content" clause of `docs/planning/ADR-001.md` line 74.
- **Early-warning signal.** G5 spike (task D5) records a `FAIL` for either
  provider in `docs/planning/gates/G5-result.md`, per
  `docs/planning/v0.1/02-gating-findings.md` §7.
- **Response.** None stated at the gate level. A failing provider's provenance
  rendering becomes a required design-change finding before Stage 2 spec freeze
  (`docs/planning/v0.1/02-gating-findings.md` §7); this also stops the pipeline
  per `docs/planning/v0.1/10-stages.md` §2's G5 no-reorder rule.

## R2 — Preview/experimental surface drift

### RISK-CLAUDE-PREVIEW — Claude Channels research-preview surface drift

- **Risk.** The Claude Code Channels research-preview surface changes shape
  around G1: channel behaviour across `--resume`/`--continue` is undocumented: whether
  one MCP server can present more than one logical channel is undocumented; and
  the compatibility-shim boundary for this surface stays unnamed (conflict-
  register C11).
- **What it invalidates.** `docs/planning/v0.1/01-capability-matrix.md` §1's
  Claude research-preview label and its pinned-version assumption;
  `docs/planning/v0.1/07-repository-and-dependencies.md` §4(b)'s `adapters/claude/`
  shim-boundary containment claim.
- **Early-warning signal.** A Claude Code pin move in `docs/planning/STATUS.md`'s
  Pins table, or `channels.md` gaining or removing text on `--resume`/
  `--continue` or multi-channel support at the next re-verification pass
  (`.claude/skills/oac-evidence/SKILL.md` §7 trigger).
- **Response.** Re-verify per `.claude/skills/oac-evidence/SKILL.md` §7 on the
  pin move. Until the shim boundary is named, containment already holds by
  construction: every Claude-specific type stays inside `adapters/claude/`
  (`docs/planning/v0.1/07-repository-and-dependencies.md` §4(b)) — no interim
  workaround is needed because the surface is already isolated by module
  boundary, only its own name is open.

### RISK-CODEX-EXPERIMENTAL — Codex experimental live-inject surface drift

- **Risk.** The Codex experimental live-inject surface changes: whether implicit
  daemon attach executes by default at runtime in `0.154.0`; whether Codex
  Desktop exposes the control socket; the compatibility-shim boundary for this
  surface stays unnamed (conflict-register C11); whether Codex reliably
  reproduces a header-supplied `oac_message_id` in a subsequent `reply` tool
  call's `in_reply_to` argument; and whether Codex's `thread.sessionId` field
  always equals `thread.id`.
- **What it invalidates.** `docs/planning/v0.1/01-capability-matrix.md` §1's
  Codex experimental label; `docs/planning/v0.1/07-repository-and-dependencies.md`
  §4(b)'s `adapters/codex/` shim-boundary containment claim; Decision 9's layered
  reply-correlation rule
  (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 9).
- **Early-warning signal.** A Codex CLI pin move in `docs/planning/STATUS.md`'s
  Pins table; the 2026-09-17 `app-server` documentation-drift signal recurring or
  worsening at the next re-fetch
  (`docs/planning/v0.1/08-cli-and-deployment.md` §10); the G2 or G4 spike
  surfacing unexpected `in_reply_to`/`sessionId` behaviour.
- **Response.** Re-verify per `.claude/skills/oac-evidence/SKILL.md` §7 on the
  pin move. Decision 9 already treats a model-echoed `in_reply_to` as untrusted
  content, validated against independently-tracked thread/turn state — drift
  here degrades to the already-named inferred/uncorrelated downgrade rather than
  silent misattribution
  (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 9).

## R3 — Evidence/pin drift

### RISK-FLOOR — `>= v2.1.232` floor unverifiable

- **Risk.** The "research preview on Claude Code v2.1.232+" floor is not
  confirmable against `channels.md` at the pinned version (`v2.1.274`).
- **What it invalidates.** `docs/planning/PINS.md` floor 1's stated minimum-
  version claim; it does not invalidate G1 itself, since G1's pin reliance is on
  the pinned version actually installed, not on the floor text
  (`docs/planning/v0.1/02-gating-findings.md` §3).
- **Early-warning signal.** A re-fetch of `channels.md` at the next Claude Code
  pin move still omits `2.1.232` (per `REVERIFICATION-B2.md` §3.1 box 7).
- **Response.** Re-verify per `.claude/skills/oac-evidence/SKILL.md` §7 on every
  Claude Code pin move. Until confirmed, `docs/planning/PINS.md` keeps the
  actually-pinned version (`v2.1.274`) as the operative floor rather than the
  unconfirmed `2.1.232` text.

### RISK-MCP-EXPERIMENTAL — `experimental` capability existence at `2026-07-28`

- **Risk.** MCP `experimental` capabilities may not exist in the current-era
  schema (`2026-07-28`) the way the Claude-channel registration path assumes.
- **What it invalidates.** G1 and G4's shared reliance on
  `capabilities.experimental["claude/channel"]` negotiating correctly
  (`docs/planning/v0.1/02-gating-findings.md` §3, §6).
- **Early-warning signal.** G4's concurrent legacy-vs-current test
  (`docs/planning/v0.1/02-gating-findings.md` §6) surfaces a negotiation failure
  tied to the `experimental` key at `2026-07-28`.
- **Response.** Re-verify per `.claude/skills/oac-evidence/SKILL.md` §7 against
  the MCP `2026-07-28` schema on the next pin move. G4's own fallback (two
  server entry points) already isolates the legacy negotiation path from any
  current-era schema change (`docs/planning/v0.1/02-gating-findings.md` §6).

## R4 — Design-parameter and platform-runtime risks

### RISK-KEYRING — Windows keyring runtime unverified

- **Risk.** The `windows-native-keyring-store` `keyring` backend has not been
  exercised end-to-end against live Windows Credential Manager.
- **What it invalidates.** Decision 7's Windows key-storage claim
  (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 7).
- **Early-warning signal.** The first live-Windows Stage 3/4 test
  (`docs/planning/v0.1/09-test-strategy.md`) fails to read or write a keyring
  entry.
- **Response.** Fall back to the already-decided `age` `0.12.1` encrypted-file
  store — the fallback Decision 7 already names for key storage
  (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 7).

### RISK-LOCAL-IPC — Local IPC peer authentication unverified on Windows

- **Risk.** Named-pipe DACL peer authentication is unexercised on a live Windows
  host; `interprocess` `2.4.4` exposes no first-party peer-credential accessor;
  `oac mcp-shim`'s inherited environment may not locate the daemon's IPC path
  without extra configuration.
- **What it invalidates.** Decision 2's OS-level peer-authentication claim
  (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 2); the zero-
  container launch story's "no extra configuration" assumption
  (`docs/planning/v0.1/08-cli-and-deployment.md` §2).
- **Early-warning signal.** The first live-Windows Stage 3/4 IPC contract test
  fails to authenticate a peer, or the shim cannot locate the daemon socket
  without explicit configuration.
- **Response.** Call the raw OS API directly instead of relying on
  `interprocess`'s accessor — the substitute already named
  (`docs/planning/decisions/C2-process-model.md` §4, §10). If shim environment
  inheritance fails, fall back to explicit IPC-path configuration, the
  alternative already scoped (`docs/planning/decisions/C2-process-model.md`
  §10-§11).

### RISK-CRYPTO-BUILD — Signing-crate Windows build and MSRV policy unverified

- **Risk.** `ed25519-dalek` `3.0.0` may not build cleanly on
  `x86_64-pc-windows-msvc`; `curve25519-dalek`'s repository-level MSRV *policy*
  is unstated.
- **What it invalidates.** Decision 5's envelope-signature dependency choice on
  Windows (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 5); the
  Rust toolchain pin's (`1.98.1`) compatibility assumption for this dependency.
- **Early-warning signal.** The first Windows CI build of the signing crate
  fails, or a future toolchain bump breaks against an undocumented MSRV.
- **Response.** The crate-level `rust-version` field (already confirmed,
  distinct from the unstated repository-level policy) governs the toolchain
  floor (`docs/planning/decisions/C5-envelope-auth.md` §2, §16); a build failure
  is a Stage 3 blocker resolved by pinning the last-working toolchain version,
  not a dependency-choice change.

### RISK-PAIRING — Pairing and fingerprint parameters unvalidated

- **Risk.** The device-key fingerprint truncation length and the 6-digit/120-
  second/5-attempt LAN pairing-code parameters are OAC's own unvalidated design
  choices.
- **What it invalidates.** Decision 6's pairing-flow parameters
  (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 6); the 128-bit
  minimum floor the fingerprint length must stay above
  (`docs/planning/decisions/C5-envelope-auth.md` §10(b)).
- **Early-warning signal.** A Stage 3/4 pairing contract test shows brute-force
  feasibility below the 128-bit floor, or measured LAN latency exceeds the
  120-second window in practice.
- **Response.** Both parameters sit above a fixed floor and are adjustable
  without an ADR amendment — tune the truncation length or the window/attempt
  counts during Stage 3/4 implementation; only the 128-bit minimum floor itself
  is fixed (`docs/planning/decisions/C5-envelope-auth.md` §10, §16).

### RISK-ZENOH-AUTH — Zenoh auth and TLS-stack assumptions unverified

- **Risk.** Zenoh's `auth.pubkey` semantics are unconfirmed, and the default TLS
  stack (`rustls` vs. an alternative) is unverified against Zenoh's own
  `Cargo.toml`/feature docs.
- **What it invalidates.** Decision 10's LAN-mode TLS/ACL assumptions
  (`docs/planning/decisions/C7-zenoh-transport.md` §5, §12); the Apache-2.0
  compatibility verdict in
  `docs/planning/v0.1/07-repository-and-dependencies.md` §7 (a different TLS
  backend could carry a different license).
- **Early-warning signal.** G3's TLS-listener pass criterion
  (`docs/planning/v0.1/02-gating-findings.md` §5) surfaces an unexpected
  cipher/backend, or a Zenoh pin move changes the default TLS feature.
- **Response.** Re-verify per `.claude/skills/oac-evidence/SKILL.md` §7 against
  Zenoh's own `Cargo.toml`/feature docs on the next pin move
  (`docs/planning/decisions/C1-language-runtime.md` §9). ACL subjects already
  never map to `zid` regardless of the auth mechanism confirmed
  (`docs/planning/STATUS.md`, C5 decision: "ACL subjects are certificate common
  name or username only, never `zid`"), so this drift cannot silently weaken
  authorization.

## R5 — Low-impact / non-dependency risks

### RISK-ACP — ACP schema v2 alpha status unconfirmed

- **Risk.** ACP schema v2's "alpha" status is carried from
  `docs/planning/PLANNING-PROMPT.md` §3.5 only, not independently re-confirmed.
- **What it invalidates.** Nothing load-bearing — ACP is explicitly not a v0.1
  dependency (`docs/planning/v0.1/05-interfaces.md` §16).
- **Early-warning signal.** None tracked before v0.1; low priority per
  `docs/planning/STATUS.md`.
- **Response.** No action required before v0.1; re-confirm on
  agentclientprotocol.com only if ACP becomes a dependency in a later milestone
  (`docs/planning/STATUS.md` "Open UNVERIFIED items").

### RISK-ZENOH-SOURCE — Zenoh crate version read from GitHub, not crates.io

- **Risk.** The pinned Zenoh crate version/date (`1.10.1`, 2026-09-07) was read
  from GitHub releases because crates.io did not return content when fetched.
- **What it invalidates.** Nothing load-bearing — the pinned value itself is not
  disputed, only its source path (`docs/planning/PINS.md`).
- **Early-warning signal.** crates.io becomes reachable and shows a different
  version/date than `docs/planning/PINS.md`.
- **Response.** Re-confirm on crates.io when reachable
  (`docs/planning/STATUS.md`); no design change unless the re-confirmation
  contradicts the pin.

### RISK-CODEX-MCP-DATES — `codex mcp-server` deprecation/deletion dates unconfirmed

- **Risk.** The `codex mcp-server` deprecation (2026-08-20) and deletion
  (2026-09-05) dates are carried unchanged from
  `docs/planning/PLANNING-PROMPT.md` §3.2, not independently re-confirmed against
  the CLI reference.
- **What it invalidates.** Nothing load-bearing — Decision 9 already routes
  Codex reachability through `codex mcp add`, not the deleted `codex
  mcp-server` (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 9).
- **Early-warning signal.** `codex mcp-server` reappears or behaves differently
  than "deleted" at the next Codex CLI pin move.
- **Response.** Re-verify per `.claude/skills/oac-evidence/SKILL.md` §7 on the
  next Codex CLI pin move; no interim action needed since the design does not
  depend on `codex mcp-server`.

### RISK-SEP — No confirmed SEP for agent-to-agent messaging

- **Risk.** No SEP or working-group item for agent-to-agent messaging was found,
  carried unchanged from `docs/planning/PLANNING-PROMPT.md` §3.3, not
  independently re-searched.
- **What it invalidates.** Nothing load-bearing — this affects only whether
  OAC's own MCP extension identifier
  (`io.github.rossgraeber/oac-session-channels`,
  `docs/planning/decisions/C3-spec-packaging.md`) could later collide with a
  future standard, not v0.1 function.
- **Early-warning signal.** A new SEP for agent-to-agent messaging is published
  naming a conflicting extension identifier.
- **Response.** Re-search the SEP index at the next MCP pin move
  (`REVERIFICATION-B2.md` §3.3); rename the extension identifier only if a
  conflict is actually found.

### RISK-BIN-SIZE — Zenoh binary size estimate unmeasured

- **Risk.** The 5-15 MB Zenoh binary size figure is a derived estimate, not a
  measurement.
- **What it invalidates.** Nothing load-bearing — informs packaging expectations
  only (`docs/planning/v0.1/08-cli-and-deployment.md`), not v0.1 function.
- **Early-warning signal.** The first G3 build artifact (task D3) measures
  outside the 5-15 MB range.
- **Response.** Replace the estimate with the measured value from the first G3
  build artifact; no design change either way
  (`docs/planning/STATUS.md`).

### RISK-NATS — NATS capability claims unverified

- **Risk.** NATS reliability, persistence, offline-queueing, ordering,
  multicast-discovery, and routing/federation capability claims are unverified
  against first-party NATS documentation.
- **What it invalidates.** Only `docs/planning/v0.1/05-interfaces.md` §17's NATS
  replacement-proof table cells — NATS is not a v0.1 dependency.
- **Early-warning signal.** None tracked before v0.1; would matter only if NATS
  became a second-transport candidate.
- **Response.** No action required for v0.1; check every claimed cell against
  first-party NATS specification/documentation before any NATS transport module
  is built (`docs/planning/v0.1/05-interfaces.md` §17).

### RISK-MQTT — MQTT capability claims unverified

- **Risk.** MQTT reliability, persistence, offline-queueing, ordering,
  multicast-discovery, and routing/federation capability claims are unverified
  against first-party MQTT/broker documentation, including a structural
  (unverified) observation that MQTT's broker-based client model lacks a
  Zenoh-style multicast-discovery primitive.
- **What it invalidates.** Only `docs/planning/v0.1/05-interfaces.md` §17's MQTT
  replacement-proof table cells — MQTT is not a v0.1 dependency.
- **Early-warning signal.** None tracked before v0.1; would matter only if MQTT
  became a second-transport candidate.
- **Response.** No action required for v0.1; check every claimed cell against
  first-party MQTT specification/broker documentation before any MQTT transport
  module is built (`docs/planning/v0.1/05-interfaces.md` §17).

## Traceability — every `docs/planning/STATUS.md` "Open UNVERIFIED items" entry

Mechanical proof for issue #32's first acceptance box: all 28 entries in
`docs/planning/STATUS.md`'s "Open UNVERIFIED items" list, disposed of here. None
were closed by evidence found while writing this file (per `oac-evidence` §5,
"never silently promoted" — closing an item requires a re-verification citation
in the same change, and none of the 28 had one available). Every row below
therefore carries a risk id, not a closing file+section; no blank cells.

| # | STATUS.md item (short) | Disposition |
|---|---|---|
| 1 | Claude channel behaviour across `--resume`/`--continue` | RISK-CLAUDE-PREVIEW |
| 2 | One MCP server presenting more than one logical channel | RISK-CLAUDE-PREVIEW |
| 3 | Implicit Codex daemon attach default at runtime (`0.154.0`) | RISK-CODEX-EXPERIMENTAL |
| 4 | Codex Desktop control-socket exposure | RISK-CODEX-EXPERIMENTAL |
| 5 | Zenoh `auth.pubkey` semantics | RISK-ZENOH-AUTH |
| 6 | 5-15 MB Zenoh binary size estimate | RISK-BIN-SIZE |
| 7 | Claude Channels compatibility-shim boundary unnamed (C11) | RISK-CLAUDE-PREVIEW |
| 8 | Codex live-inject compatibility-shim boundary unnamed (C11) | RISK-CODEX-EXPERIMENTAL |
| 9 | ACP schema v2 "alpha" status | RISK-ACP |
| 10 | Zenoh crate version/date read from GitHub, not crates.io | RISK-ZENOH-SOURCE |
| 11 | `codex mcp-server` deprecation/deletion dates | RISK-CODEX-MCP-DATES |
| 12 | No SEP for agent-to-agent messaging | RISK-SEP |
| 13 | "Research preview on Claude Code v2.1.232+" floor | RISK-FLOOR |
| 14 | MCP `experimental` capabilities at current era `2026-07-28` | RISK-MCP-EXPERIMENTAL |
| 15 | Zenoh default TLS stack `rustls` | RISK-ZENOH-AUTH |
| 16 | `rmcp`-based server registering as a legacy-era live channel | RISK-G4 |
| 17 | Codex reproducing `oac_message_id` in `in_reply_to` | RISK-CODEX-EXPERIMENTAL |
| 18 | Windows `windows-native-keyring-store` runtime | RISK-KEYRING |
| 19 | `oac mcp-shim` environment inheritance for the IPC path | RISK-LOCAL-IPC |
| 20 | Named-pipe DACL peer auth on live Windows | RISK-LOCAL-IPC |
| 21 | `interprocess` `2.4.4` peer-credential accessor | RISK-LOCAL-IPC |
| 22 | Codex `thread.sessionId` vs. `thread.id` | RISK-CODEX-EXPERIMENTAL |
| 23 | `ed25519-dalek` `3.0.0` on `x86_64-pc-windows-msvc` | RISK-CRYPTO-BUILD |
| 24 | `curve25519-dalek` repository MSRV policy | RISK-CRYPTO-BUILD |
| 25 | Device-key fingerprint truncation length | RISK-PAIRING |
| 26 | 6-digit/120s/5-attempt pairing parameters | RISK-PAIRING |
| 27 | NATS capability claims | RISK-NATS |
| 28 | MQTT capability claims | RISK-MQTT |

## Self-check (`oac-evidence` §8, `oac-planning-package` §6)

- Every `§n` cross-reference above was re-swept against each target file's final
  section numbering as read for this task (`02-gating-findings.md`,
  `10-stages.md`, `03-decisions-and-amendments.md`, `07-repository-and-
  dependencies.md`, `05-interfaces.md`, `06-security.md`, `08-cli-and-
  deployment.md`); none pointed at a stale number.
- Every UNVERIFIED label carried forward keeps its original reason from
  `docs/planning/STATUS.md`; none is restated as settled.
- Naming resolution (`.claude/skills/oac-planning-package/SKILL.md` §4) applied:
  "Open Agent Channel (OAC)", "OAC Session Channels", `oac` throughout; no
  `sessionchannels` or bare "Session Channels" spelling introduced.
- No neutral-interface text in this file names Zenoh/Claude/Codex/MCP method
  names where neutral text is required (this file discusses gate surfaces by
  name deliberately, as `02-gating-findings.md` and the capability matrix
  already do — it is not spec surface text, per `.claude/skills/oac-boundaries/
  SKILL.md`).
- No risk response above proposes calling a provider model API, holding
  provider credentials, scraping a UI, or polling an inbox from an adapter
  claiming active inbound.
