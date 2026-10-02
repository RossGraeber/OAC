# G1 — Claude wake

Source: PLANNING-PROMPT.md §4 G1, §3.1. Backlog: `docs/planning/backlog/03-tasks-CD.json` D1.

## Disposition

**v0.1 go/no-go. No supported fallback exists.** Failure blocks the Claude adapter outright.

## What the spike proves

A throwaway development-flag channel server proves the Claude wake semantics on the
observed Claude Code version — this row is floating, not pinned; record the full observed
version triple (see "Surfaces and version pins" below).

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

- Claude Code Channels research preview. Version: **floating** (`docs/planning/PINS.md`,
  Claude Code (Channels), "Version policy", #216). There is no fixed pin: PINS.md records a
  minimum (`v2.1.282`) and a last tested version. Record the observed CLI (`claude
  --version`), wire `initialize` result `clientInfo.version`, and the transport user-agent
  (when the transport carries one) in the result. A version other than the last tested one,
  or below the minimum, is a warning, never a stop and never by itself a reason to
  invalidate the verdict. The §3.1 facts were re-checked once, at `2.1.285` (2026-10-02,
  #122; `oac-claude-channels` `## Pin`). Newer versions are warnings only. The capability
  floors — channels-exist `>= v2.1.232` (unsupported, drift D6: the changelog says
  `2.1.80`) and permission-relay `>= v2.1.234` (PLANNING-PROMPT.md §3.1) — sit below the
  minimum; permission relay itself stays out of scope for this gate,
  proposed off by default in v0.1 (Decision 8 / C10, not yet decided; Epic C is still open
  per STATUS.md).
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
    queued and delivered in order, not dropped, not interleaved into an in-flight tool
    call. **Amendment, 2026-09-28 (G1 Box C, `v2.1.283`):** do not assume "delivered
    together, at the next turn" means batched at one boundary — Box C observed two
    close-together mid-turn notifications delivered at two separate tool-call boundaries,
    still in order and un-dropped. Delivery granularity (batched vs. per-boundary) is
    UNVERIFIED as a guarantee; may depend on relative send timing. See
    `docs/planning/gates/G1-result.md` "Re-run attempt 2 (Box C)".
  - Claude Code sends **no acknowledgement** — a resolved notification send means "written to
    transport," not "seen by the model." Record what the spike can and cannot observe about
    delivery given this.
  - `meta` keys must be identifier-safe (letters, digits, underscore) or are silently dropped
    — verify this by including a non-identifier-safe key and confirming it does not appear as
    an attribute.
  - Loading is `--channels plugin:<name>@<marketplace>` or `--channels server:<name>`, at
    session start only. (Dated note, 2026-10-02, #122: `--channels` takes `plugin:`
    entries only; a bare server loads with `--dangerously-load-development-channels
    server:<name>` — drift D4, `docs/planning/REVERIFICATION-B2.md`.)
- UNVERIFIED items this gate is positioned to close (record whichever you actually test; if
  untested, they remain open per STATUS.md):
  - Channel behavior across `--resume`.
  - Whether one server can present more than one logical channel.
  - Whether a `CLAUDE_SESSION_ID` environment variable exists (baseline says none is
    documented). (Dated note, 2026-10-02, #122: closed in B2. A differently named
    `CLAUDE_CODE_SESSION_ID` is documented, drift D5; see `oac-claude-channels` §8.)
  - Agent SDK support for Channels (baseline presumes absent).

## Fixtures to capture (for Stage 3's fake Claude endpoint)

Per D6: initialize/negotiation handshake (showing the legacy revision), the
`notifications/claude/channel` payload and the resulting `<channel>` tag rendering, the
mid-turn queueing sequence (two notifications, in order, not dropped — not necessarily one
turn boundary; G1 Box C observed two separate boundaries, see the amendment above), and a
tool-based reply. Capture the pinned version and date on the fixture; redact any
session-identifying data before committing.
