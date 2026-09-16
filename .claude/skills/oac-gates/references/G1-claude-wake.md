# G1 — Claude wake

Source: PLANNING-PROMPT.md §4 G1, §3.1. Backlog: `docs/planning/backlog/03-tasks-CD.json` D1.

## Disposition

**v0.1 go/no-go. No supported fallback exists.** Failure blocks the Claude adapter outright.

## What the spike proves

A throwaway development-flag channel server proves the Claude wake semantics on the pinned
Claude Code version.

## Pass criteria (evaluate each individually)

- [ ] Server declares `capabilities.experimental["claude/channel"] = {}` and negotiates a
      legacy MCP revision (`2025-11-25` or earlier); use `MCP_PROTOCOL_NEGOTIATION=legacy` for
      stdio servers if needed.
- [ ] A `notifications/claude/channel` carrying `content` (string) and `meta`
      (string-to-string map) wakes an idle session as a user turn and appears as a
      `<channel>` tag with the expected attributes (one attribute per identifier-safe `meta`
      key).
- [ ] A second notification sent mid-turn is queued and delivered at the next turn, in order
      (not dropped, not interleaved out of order).
- [ ] Claude replies through an ordinary MCP tool (conventionally `reply`).
- [ ] The `--dangerously-load-development-channels` interactive confirmation dialog is
      actually exercised during the spike — not bypassed, scripted around, or skipped.

## Failure criteria

Any pass criterion above unmet is a FAIL. There is no partial pass: a server that wakes the
session but loses mid-turn ordering, or that never triggers the confirmation dialog because
the spike bypassed it, is a FAIL, not a qualified pass.

## Fallback

None. PLANNING-PROMPT.md §4: "there is no supported fallback, so this is v0.1 go/no-go."

## Surfaces and version pins

- Claude Code Channels research preview, pinned baseline v2.1.232+ (PLANNING-PROMPT.md §3.1).
  Permission relay (`claude/channel/permission`) needs v2.1.234+ but is out of scope for this
  gate — it is proposed off by default in v0.1 (Decision 8 / C10, not yet decided; Epic C is
  still open per STATUS.md).
- MCP protocol revision: legacy only (`2025-11-25` or earlier). The channel server MUST NOT
  negotiate `2026-07-28` — see G4 for the dual-era interaction.
- Not available on Bedrock, Vertex, or Foundry (record which backend the spike ran against).
- Distribution gate: only allowlisted plugins skip the flag; anything else requires
  `--dangerously-load-development-channels` plus the interactive confirmation — this is the
  one user consent step the spike must exercise, per PLANNING-PROMPT.md §7 ("the plan must
  not weaken it").

## §3.1 facts this spike must confirm or refute

- Confirmed by design (must observe directly, not assume):
  - Inbound notifications wake an idle session as a user turn; notifications mid-turn are
    queued and delivered together, in order, at the next turn.
  - Claude Code sends **no acknowledgement** — a resolved notification send means "written to
    transport," not "seen by the model." Record what the spike can and cannot observe about
    delivery given this.
  - `meta` keys must be identifier-safe (letters, digits, underscore) or are silently dropped
    — verify this by including a non-identifier-safe key and confirming it does not appear as
    an attribute.
  - Loading is `--channels plugin:<name>@<marketplace>` or `--channels server:<name>`, at
    session start only.
- UNVERIFIED items this gate is positioned to close (record whichever you actually test; if
  untested, they remain open per STATUS.md):
  - Channel behavior across `--resume`.
  - Whether one server can present more than one logical channel.
  - Whether a `CLAUDE_SESSION_ID` environment variable exists (baseline says none is
    documented).
  - Agent SDK support for Channels (baseline presumes absent).

## Fixtures to capture (for Stage 3's fake Claude endpoint)

Per D6: initialize/negotiation handshake (showing the legacy revision), the
`notifications/claude/channel` payload and the resulting `<channel>` tag rendering, the
mid-turn queueing sequence (two notifications, one turn boundary), and a tool-based reply.
Capture the pinned version and date on the fixture; redact any session-identifying data before
committing.
