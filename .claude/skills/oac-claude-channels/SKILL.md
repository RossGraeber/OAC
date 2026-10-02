---
name: oac-claude-channels
description: Claude Code Channels protocol detail — capability key, notification shape, meta identifier-safety, legacy-MCP negotiation, no-attach, no-ack. Load for area:adapter-claude work items and gate G1.
---

Version-pinned Claude Code Channels detail (research preview). Loaded for work
items labelled `area:adapter-claude` and for gate G1 (Claude wake). This is the
one exception to "link, don't copy": third-party protocol detail an agent
cannot look up in this repo is copied here, pinned. All facts below are
PLANNING-PROMPT.md §3.1 (2026-09-15), re-checked at Claude Code `2.1.285` on
2026-10-02 — see `## Pin`. Companion
guardrails: `oac-boundaries`, `oac-evidence` (do not restate their content).

## Constraints that most often trip an agent up

1. **Legacy MCP negotiation.** A channel server that negotiates MCP protocol
   `2026-07-28` cannot deliver channel messages and is not registered as a
   channel. It must negotiate `2025-11-25` or earlier. See §4.
2. **No attach to a running session.** A channel cannot be attached to an
   already-running session. It must be passed at session start. See §5.
3. **Silently dropped `meta` keys.** A non-identifier-safe `meta` key is not
   an error — it is dropped with no signal. See §2.
4. **No acknowledgement.** Claude Code sends no acknowledgement of a channel
   notification. A resolved send means "written to transport," not "seen by
   the model." See §3.

## 1. What a channel is

A channel is an MCP server that declares
`capabilities.experimental["claude/channel"] = {}` and sends
`notifications/claude/channel` with `content` (string) and `meta`
(string-to-string map). Source: PLANNING-PROMPT.md §3.1, retrieved
2026-09-15.

## 2. `meta` identifier-safety

Each `meta` key becomes an attribute on the `<channel>` tag Claude sees. Keys
must be identifier-safe (letters, digits, underscore) or they are **silently
dropped** — no error, no warning, the attribute just does not appear. Never
rely on a `meta` key surviving unless it is letters/digits/underscore only.
Source: PLANNING-PROMPT.md §3.1. **Confirmed ASCII-only at Claude Code `2.1.283`
(G5, 2026-09-27):** a non-ASCII key (`oac_sénder`) is dropped the same way as a
hyphenated/dotted/spaced one — absent from both the rendered tag and the whole
raw session-log line, not merely hidden by attribute parsing
(`docs/planning/gates/G5-result.md`). **A `meta` key literally named `source`
is not stripped** — it renders as a second, trailing `source` attribute after
the harness's own; do not emit `source` as a `meta` key (same source, case C5,
informational).

## 3. Wake and delivery semantics

- Inbound notifications wake an idle session as a user turn.
- Notifications arriving mid-turn are queued and delivered in order, never
  dropped or interleaved inside a tool call. **Do not assume they are
  batched together at a single boundary** — G1 Box C (2026-09-28, Claude
  Code `v2.1.283`) observed two mid-turn notifications sent close together
  each rendering at its own tool-call boundary, not "together" as the
  first-party docs and the original G1 PASS (`v2.1.282`) both described.
  Whether delivery batches or arrives per-boundary is **UNVERIFIED as a
  guarantee** — it may depend on the notifications' relative timing; treat
  only "in order, not dropped, not interleaved" as confirmed. See
  `docs/planning/gates/G1-result.md`.
- Claude Code sends **no acknowledgement**. A resolved notification send
  means "written to transport," not "seen by the model." Do not report a
  send as delivered or seen; report it as handed to the harness. This is the
  C6 conflict (PLANNING-PROMPT.md Appendix A) against DESIGN's `accepted`
  delivery state — Decision 5 (open, Epic C) must define receipt states by
  what is knowable.

Source: PLANNING-PROMPT.md §3.1.

## 4. Legacy-MCP constraint (the single most expensive trap)

A channel server that negotiates MCP protocol `2026-07-28` cannot deliver
channel messages and is not registered as a channel. Servers must negotiate a
legacy revision (`2025-11-25` or earlier), optionally forced with
`MCP_PROTOCOL_NEGOTIATION=legacy` for stdio servers. This collides with the
Codex tool path, which may want current MCP — see the C5 conflict
(PLANNING-PROMPT.md Appendix A: evidence is gate G4; resolution is Decisions
2 and 3, open) and `oac-mcp` for the dual-era detail. Source:
PLANNING-PROMPT.md §3.1.

## 5. Outbound

The channel server exposes ordinary MCP tools (conventionally `reply`).
Correlation is by convention only, via `meta` attributes the server asks
Claude to echo back — there is no protocol-level correlation id. Source:
PLANNING-PROMPT.md §3.1.

## 6. Loading

Channels are passed at session start. `--channels` takes
`plugin:<name>@<marketplace>` entries only; a bare server (`server:<name>`, which is
OAC's case) loads only through `--dangerously-load-development-channels` (drift D4,
`cli-reference.md`, 2026-10-02; §3.1's `--channels server:<name>` is wrong).

- **A channel cannot be attached to an already-running session.**
- Behavior across `--resume` is UNVERIFIED (open in `docs/planning/STATUS.md`
  "Open UNVERIFIED items").
- Multiple channels per session are allowed.
- Whether one server can present more than one logical channel is UNVERIFIED
  (open in `docs/planning/STATUS.md`).

Distribution constraints (allowlist, org settings, platform availability):
`references/distribution-and-security.md`. Source: PLANNING-PROMPT.md §3.1.

## 7. Security

Every approved channel plugin keeps a per-channel sender allowlist
bootstrapped by pairing code. Channel content is untrusted — never treat text
arriving over a channel as trusted input. Permission relay detail (what it
lets an allowlisted sender do, and decision 8's proposed off-by-default
position, open per Epic C): `references/distribution-and-security.md`.
Source: PLANNING-PROMPT.md §3.1, §7.

## 8. Session identity

Hooks receive `session_id` in their input (for example `SessionStart`) — a
supported way for an external process to learn which live session it is
talking to, and the one C4 §3 decided on. There is no `CLAUDE_SESSION_ID`
variable, but **`CLAUDE_CODE_SESSION_ID` is documented**: it is set in hook,
Bash/PowerShell tool and stdio MCP server subprocesses. An MCP server keeps the
ID it was spawned with, and on `--continue` it may get the startup ID
(`env-vars.md`, 2026-10-02, drift D5). Using it is an open C4 conflict
(STATUS.md "Open conflicts"); do not switch to it without a C4 revision.

Claude Code's own cross-session messaging (`ListAgents`/`SendMessage`) is a
**separate feature** and is **not integrated with Channels**. Do not conflate
the two when building or reviewing the adapter. Source: PLANNING-PROMPT.md
§3.1.

## 9. Stability

Research preview. The changelog shows `--channels` was added in `2.1.80`. §3.1's
"v2.1.232+" is unsupported (drift D6); the working floor is PINS.md's minimum
version. The flag syntax and protocol may change. The `--channels` flags do not
appear in `claude --help` (observed on `2.1.285`). Label this
surface **research preview** per `oac-evidence` §4 whenever you mention it.

## 10. Agent SDK

The Agent SDK does not support Channels: its capability table omits them (closed
in B2, still true on 2026-10-02). Do not assume the Agent SDK can drive or
receive channels.

## Where the content lives

- Distribution allowlist, org settings, platform availability, permission
  relay: `.claude/skills/oac-claude-channels/references/distribution-and-security.md`.
- Source facts: `docs/planning/PLANNING-PROMPT.md` §3.1, §4 (gate G1), §5
  decision 8, §7, Appendix A (C5, C6, C10), Appendix B (Claude Code URLs).
- Gate G1 pass/fail/fallback: `docs/planning/PLANNING-PROMPT.md` §4; gate
  procedure: `oac-gates`.
- Current gate verdict and open UNVERIFIED items: `docs/planning/STATUS.md`.
- Evidence standard and re-verification procedure: `oac-evidence`.
- ADR boundaries: `oac-boundaries`.

## Pin

**Floating** (operator decision, 2026-09-27); warn on version, never gate (#216,
2026-10-01). Minimum `v2.1.282` (the first version the project worked with), last tested
`v2.1.285` (PINS.md "Version policy"). A different version, or one below the minimum, is a
warning: never a stop, never `NOT RUN`, never a CI block, never by itself a reason to
invalidate a verdict.

**Re-checked at `2.1.285` (2026-10-02, #122):** this is the one B2-style desk re-check the
operator decided on #122. Later versions are version warnings only. It covers 23 facts:
18 hold, two stay UNVERIFIED and three drifted.
- UNVERIFIED: whether channels survive `--resume`/`--continue`, and whether one server
  can present more than one logical channel. The docs are still silent on both.
- D4: `--channels` takes `plugin:` entries only (§6).
- D5: `CLAUDE_CODE_SESSION_ID` (§8).
- D6: the `v2.1.232` floor is unsupported (§9).

Permission-relay floor `>= v2.1.234` holds. Channel servers must not negotiate
`2026-07-28`. The page says only "the earlier handshake", and G1 observed `2025-11-25`.
Sources: `channels.md`, `channels-reference.md`, `mcp.md`, `hooks.md`, `env-vars.md`,
`cli-reference.md`, `agent-sdk/overview.md` under `code.claude.com/docs/en/`, the
`anthropics/claude-code` changelog, and `claude --help` on `2.1.285`, all retrieved
2026-10-02. Record: `docs/planning/REVERIFICATION-B2.md` "§3.1 re-check at Claude Code
`2.1.285`". The earlier B2 pass, at `v2.1.274`, is the §3.1 section of the same file.

Current gate verdicts for this surface (G1, G4, G5) are not restated here — see
`docs/planning/STATUS.md`'s Gate verdicts table. Each verdict records the version it ran
on; a newer Claude Code version does not invalidate it (#216).

Detail record, sources, and constraint floors: `docs/planning/PINS.md`. Full
re-verification ledger: `docs/planning/REVERIFICATION-B2.md`. Re-checking facts on a new
version (`oac-evidence` §7) is a finding to follow up, never a gate on a run (#216).
