### G1 claude-wake

- **Gate id:** G1
- **Pinned version(s):** Claude Code `v2.1.282` (the version actually observed connecting —
  see "Pin drift" below; `docs/planning/PINS.md` recorded `v2.1.274` at spike time), MCP
  legacy revision `2025-11-25`.
- **Date:** 2026-09-25
- **Timebox:** No formal timebox was set before starting (process deviation, noted below);
  elapsed wall-clock was approximately 1.5 hours, most of it spent on setup/tooling
  (framing-protocol misdiagnosis, CLI flag-syntax trial-and-error) before the actual spike
  ran cleanly. All five pass criteria were confirmed well within a reasonable box once the
  correct invocation was found.
- **Command transcript summary:** A throwaway Node.js stdio MCP server
  (`docs/planning/gates/fixtures/g1-claude-wake/channel-server.mjs.throwaway-quarantined`)
  was registered via a project `.mcp.json` and launched with
  `claude --dangerously-load-development-channels server:g1spike` (human operator, real
  terminal — the interactive confirmation dialog cannot be driven from a non-TTY tool).
  The operator approved the "local development" warning dialog at startup. The server
  negotiated MCP `2025-11-25` and declared `capabilities.experimental["claude/channel"]`.
  Four notifications were sent over the session: one to an idle session (woke it as a new
  user turn), two sent during a genuinely busy multi-tool-call turn (delivered together,
  in order, between tool calls, after the busy turn finished), and the operator then asked
  Claude to use the `reply` tool. Full raw JSON-RPC transcript:
  `docs/planning/gates/fixtures/g1-claude-wake/transcript.jsonl`.
- **Pass criteria evaluated:**
  - [x] Server declares `capabilities.experimental["claude/channel"] = {}` and negotiates a
        legacy MCP revision — confirmed directly in the transcript
        (`docs/planning/gates/fixtures/g1-claude-wake/transcript.jsonl` lines 20-21,
        38-39): the real Claude Code client (`clientInfo.name: "claude-code"`) sent
        `protocolVersion: "2025-11-25"` and the server's declared
        `capabilities.experimental["claude/channel"]` was accepted (the session went on to
        register the channel and act on notifications).
  - [x] A `notifications/claude/channel` wakes an idle session as a user turn and appears
        as a `<channel>` tag with the expected attributes — confirmed: Claude's own report
        after the wake notification: *"I got the channel notification
        (g1-spike-wake-test-1 from g1-spike-operator). It came in as a new user turn, so it
        woke this idle session."* A later direct query confirmed the `<channel>` tag's
        exact attribute set: `source="g1spike"`, `oac_message_id="..."`,
        `oac_sender="g1-spike-operator"` — exactly the two server-supplied `meta` keys plus
        the harness-added `source`, nothing else.
  - [x] A second notification sent mid-turn is queued and delivered at the next turn, in
        order — confirmed on the second attempt (the first attempt was not a true mid-turn
        window; see "Process notes" below). During a genuinely busy turn (four sequential
        foreground `Start-Sleep` tool calls), two notifications sent mid-turn arrived
        together between tool calls (not interleaved inside one), in the order sent, and
        Claude explicitly reported finishing the operator's task before acting on them:
        *"both arrived tagged as untrusted external content, so I didn't act on them and
        finished your task first."*
  - [x] Claude replies through an ordinary MCP tool — confirmed in the transcript (line
        73-74): a `tools/call` for `reply` with `message` and `in_reply_to` (echoing the
        `oac_message_id` of the most recent notification), answered normally by the
        server.
  - [x] The `--dangerously-load-development-channels` interactive confirmation dialog is
        actually exercised — confirmed by the operator directly: *"I was required to
        approve using the development channel on startup of the Claude Code CLI. No other
        dialogs or approvals were given past this initial startup dialog."* (The
        channels-reference doc also describes a second, per-project ".mcp.json server"
        consent dialog; the operator did not report seeing it separately in this session,
        plausibly because project-level MCP-server trust was already established from an
        earlier session in the same directory. Not re-tested; noted, not blocking — the
        specific ADR-001 consent step this criterion names is the development-channels
        warning, and that one was exercised.)
- **Verdict:** PASS
- **Fallback taken:** not applicable — G1 has no fallback; the primary path passed outright.
- **UNVERIFIED items:**
  - Channel behavior across `--resume` — not tested this spike, still open.
  - Whether one server can present more than one logical channel — not tested, still open.
  - `CLAUDE_SESSION_ID` environment variable — not tested this spike (already closed by B2:
    confirmed absent).
  - Agent SDK support for Channels — not tested this spike (already closed by B2: confirmed
    absent).
  - New, from this spike: the exact wire framing for Claude Code's MCP stdio transport is
    newline-delimited JSON (NDJSON), not `Content-Length`-prefixed framing — confirmed
    directly (see "Process notes"). This was previously unstated in any OAC document;
    recording it here as new first-party evidence, not carried from PLANNING-PROMPT.md.
  - New, from this spike: a notification delivered while Claude is genuinely mid-turn
    arrives wrapped with an additional "message arrived from `<server>`, untrusted content"
    framing distinct from the bare `<channel>` tag an idle-wake notification gets. Exact
    wrapper text not fully captured (the operator's transcription of it was partially
    garbled); flagged here as a real but incompletely documented observation for a future,
    more careful spike or for Stage 3 adapter work to re-confirm against the fixture
    transcript's raw `<channel>` rendering if a debug log becomes available.
- **Fixtures captured:** `docs/planning/gates/fixtures/g1-claude-wake/transcript.jsonl`
  (75 lines, full raw JSON-RPC exchange covering initialize/negotiation, one idle-wake
  notification, two mid-turn-queued notifications, and a tool-based reply — satisfies D6's
  Claude fixture coverage list). Location is provisional; D6 (not yet executed) owns the
  final fixture-store convention and may relocate this. No credentials, tokens, or private
  filesystem paths appear in the transcript's JSON-RPC payloads (verified by inspection —
  only the throwaway spike's own synthetic operator/session identifiers appear).
- **Pin rows relied on:** `Claude Code (Channels)`, `MCP — current era`, `MCP — legacy era`,
  `Rust MCP SDK (rmcp)`
- **PINS.md as-of:** 2026-09-16, commit `a44a7ed7278caab383f264f5bc8e12bb5c8c73e9`
- **Pin drift found during this spike:** the Claude Code binary that actually connected
  reported `clientInfo.version: "2.1.282"` (transcript line 20), not the `v2.1.274` PINS.md
  had on record. The installed VSCode extension directory also churned during the session
  (`anthropic.claude-code-2.1.274-win32-x64` → `...-2.1.276-win32-x64` on disk, while the
  running client reported `2.1.282`) — this surface auto-updates faster than it can be
  pinned. Per `docs/planning/PINS.md`'s own trigger-event rule, this is a real pin move,
  not a typo: **B1's pin (v2.1.274) is now stale.** This gate result is evidence for the
  new version (`2.1.282`), but a full B2-style re-verification of every §3.1 fact against
  `2.1.282` has NOT been done here — only the five G1 pass criteria were checked. Recording
  the new version as an open item in `docs/planning/STATUS.md` rather than silently
  bumping `docs/planning/PINS.md`'s pin table myself, since that re-verification sweep is
  out of this task's scope.
- **Process notes (for future gate spikes, not part of the pass/fail record):**
  - No formal timebox was set before starting — a real deviation from `oac-gates`'
    timebox policy. Retroactively, the actual working time was reasonable, but the policy
    should be followed going forward: set the box before the spike starts.
  - The first implementation attempt guessed at Claude's MCP stdio wire framing
    (LSP-style `Content-Length` headers) instead of checking first-party documentation,
    causing a real connection failure the operator spent significant time debugging with
    me before I fetched `https://code.claude.com/docs/en/channels-reference.md` and found
    the actual framing (NDJSON) and the actual CLI invocation
    (`claude --dangerously-load-development-channels server:<name>` — `--channels` and
    `--dangerously-load-development-channels` must NOT be combined; the doc states
    combining them does not extend the bypass). This should have been checked before
    writing any spike code. See memory `feedback_verify_docs_before_spiking.md`.
  - The first mid-turn-queueing attempt was invalid (the trigger fired after Claude had
    already gone idle, not during a busy turn) and had to be redone with an explicit,
    longer, genuinely-foreground busy task before it produced valid evidence.
- **Re-run history:**

  | Date | Pinned versions | Verdict | Invalidated by |
  |---|---|---|---|
  | 2026-09-25 | Claude Code v2.1.282 (observed; PINS.md pin was v2.1.274, now stale — see "Pin drift") | PASS | — |
