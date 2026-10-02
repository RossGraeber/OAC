### G1 claude-wake

> **PASS, current, Claude Code `v2.1.283` (Box C, 2026-09-28).** Operator decision
> 2026-09-28 (issue #39, chat): G1 = **PASS**, judged directly against criterion 3's
> literal text ("queued and delivered at the next turn, in order (not dropped, not
> interleaved out of order)") — order preserved, nothing dropped, nothing interleaved
> inside a tool call. Box C's delivery-at-two-separate-boundaries pattern (vs. the
> original PASS's delivery-together-at-one-boundary) is recorded as an **observed
> `2.1.283` behavior change**, not a criterion failure — every "delivered together"
> statement elsewhere in this repo has been amended with a dated note (not silently
> rewritten) and the delivery-granularity question is tracked as
> UNVERIFIED-as-a-guarantee (`docs/planning/v0.1/11-risks.md` row 49,
> `docs/planning/STATUS.md`). The fields below describe Box C, the current
> verdict-bearing run. The original `v2.1.282` PASS (2026-09-25) is superseded and kept
> as history in "### Original run (2026-09-25, v2.1.282) — superseded" below. Box B
> (2026-09-28, earlier the same day as Box C) was an incomplete re-run attempt, not
> verdict-bearing — see "### Re-run attempt 1 (Box B) — incomplete attempt" below.

- **Gate id:** G1
- **Pinned version(s):** Claude Code `v2.1.283` (observed via `claude --version` and wire
  `clientInfo.version`, pre- and post-box), MCP legacy revision `2025-11-25`.
- **Date:** 2026-09-28 (Box C; same HIL sitting as the D6 Claude protocol capture and
  Box B, issue #39 T6/T7).
- **Timebox:** Declared 45 minutes, `2026-09-28T05:33:29Z`-`05:44:27Z` — not expired
  (elapsed ~11 minutes).
- **Command transcript summary:** Same throwaway server as the original PASS,
  unmodified: `docs/planning/gates/fixtures/g1-claude-wake/
  channel-server.mjs.throwaway-quarantined`. Two server instances appear inside the
  declared box window: a first launch (pid 24128, lines 1-10) that the operator
  interrupted — confirmed exited by `05:41:17Z` (only pid 27760 was found alive at that
  check), consistent with an interrupted first launch; only one wake frame exists in
  the whole transcript, and it belongs to the second instance — and a second launch
  (pid 27760, lines 11-25) that served the rest of the box; only the second instance's
  lines carry pass-criteria evidence. Changes from Box B, per the operator's explicit
  decision: capture the dev-channels dialog text *before* accepting (criterion 5);
  query the `<channel>` tag attribute set after the wake (criterion 2, matching the
  original PASS); use four foreground `Start-Sleep` tool calls and **two** mid-turn
  triggers (criterion 3 ordering, matching the original PASS's two-notification test);
  reply tool call (criterion 4). Full raw transcript, redacted, 25 lines:
  `docs/planning/gates/fixtures/g1-claude-wake/transcript-2026-09-28-2.1.283-boxC.jsonl`.
- **Pass criteria evaluated:**
  - [x] Server declares `capabilities.experimental["claude/channel"] = {}` and negotiates
        a legacy MCP revision — **evidenced, same version-level behavior change as Box
        B.** Lines 13-14 (the serving instance's `server/discover` probe) show the
        client sending `_meta["io.modelcontextprotocol/protocolVersion"]: "2026-07-28"`,
        answered with a legacy-shaped `-32601` error, *before* falling back to the
        legacy `initialize` at line 15; lines 15-17 are the legacy `initialize`/
        `notifications/initialized`, `capabilities.experimental["claude/channel"]`
        accepted. **Different from the original `v2.1.282` PASS:** that transcript,
        captured with the same launch command in the same directory, carries **zero**
        `server/discover` frames (grep-confirmed against the committed
        `docs/planning/gates/fixtures/g1-claude-wake/transcript.jsonl`). This is a
        same-launch, cross-version
        protocol difference on Claude Code's side, not itself a criterion failure (the
        legacy negotiation still succeeds after the probe is refused) — recorded as a
        version-level behavior change, consistent with Box B.
  - [x] A `notifications/claude/channel` wakes an idle session as a user turn and
        appears as a `<channel>` tag with the expected attributes — **fully
        re-confirmed, closing Box B's gap.** Wake at line 21 (`g1-spike-wake-test-1`,
        t=`2026-09-28T05:40:55.489Z`); operator paste: "It arrived while the session
        was idle and came in as a new turn, so the wake test worked." The operator then
        queried the `<channel>` tag's attributes and got exactly
        `source=g1spike, oac_message_id=g1-spike-wake-test-1, oac_sender=g1-spike-operator`
        — the same three-attribute set the original PASS confirmed — plus confirmation
        the non-identifier-safe `meta` key ("not identifier safe!") was silently
        dropped. Box B had only re-confirmed the wake itself, not this attribute set.
  - [x] A second notification sent mid-turn is queued and delivered at the next turn, in
        order (not dropped, not interleaved out of order) — **PASS, with an observed
        `2.1.283` behavior change recorded.** Two mid-turn notifications were sent
        ~1.85s apart during the turn's first `Start-Sleep`
        (`g1-spike-midturn-test-2` at line 22, t=`2026-09-28T05:42:21.313Z`;
        `g1-spike-midturn-test-3` at line 23, t=`2026-09-28T05:42:23.159Z`). The
        operator's paste of the rendered sequence: "Ran 1 shell command" -> notification
        (test-2) -> "Ran 1 shell command" -> notification (test-3) -> "Ran 2 shell
        commands" -> DONE; Claude's own table records test-2 arriving after Sleep 1 and
        test-3 after Sleep 2, "they arrived in id order," "each one landed between tool
        calls, not in the middle of one," no action taken until the turn finished.
        Judged directly against the criterion's literal text, every clause is met: both
        notifications arrived, in order (test-2 before test-3), neither was dropped, and
        neither was interleaved inside a tool call. **Operator decision, 2026-09-28:
        PASS on the criterion as worded.**

        What is genuinely different from the original PASS, recorded as an observed
        behavior change rather than smoothed over: this box's two notifications landed
        at **two separate** tool-call boundaries (one after Sleep 1, one after Sleep 2),
        not **together at a single** boundary. The original PASS's own account states
        plainly: "two notifications sent mid-turn arrived together between tool calls"
        — and this gate's own reference, `G1-claude-wake.md`'s "Fixtures to capture"
        line, previously described a captured mid-turn sequence as showing "two
        notifications, one turn boundary"; `oac-claude-channels`'s §3.1 fact list
        previously stated "notifications mid-turn are queued and delivered together, in
        order, at the next turn." Both references, and `PLANNING-PROMPT.md` §3.1 and
        `docs/planning/v0.1/04-architecture.md`, have been amended (dated note, not
        silently rewritten) to stop asserting "together" as a guarantee — delivery
        batching vs. per-boundary delivery is now tracked as **UNVERIFIED as a
        guarantee** (`docs/planning/v0.1/11-risks.md` row 49), since only two data
        points exist (original PASS: together, one boundary; Box C: in order, two
        boundaries) and relative send timing may be the actual variable, not the Claude
        Code version. This does not affect the criterion's PASS — its own wording never
        required "together," only "in order... not dropped... not interleaved," all of
        which Box C confirms.
  - [x] Claude replies through an ordinary MCP tool — **evidenced.** Lines 24-25: a
        `reply` tool call, "Hello!", `in_reply_to g1-spike-midturn-test-3`; channel
        confirmed receipt.
  - [x] The `--dangerously-load-development-channels` interactive confirmation dialog is
        actually exercised — **fully re-confirmed, closing Box B's gap, stronger
        evidence than the original PASS.** The operator captured the dev-channels
        dialog's exact text *before* accepting it:

        > WARNING: Loading development channels
        > --dangerously-load-development-channels is for local channel development only.
        > Do not use this option to run channels you have downloaded off the internet.
        > Please use --channels to run a list of approved channels.
        > Channels: server:g1spike
        > \> 1. I am using this for local development   2. Exit
        > Enter to confirm · Esc to cancel

        The operator selected option 1. No separate `.mcp.json` server-trust dialog was
        reported — consistent across all three runs in this directory (the original
        PASS, Box B, and Box C), which supports the original PASS's speculative
        explanation that project-level MCP-server trust, once established, persists
        across separate launches rather than being re-prompted per session (see
        "Original run" below, criterion 5).
- **Verdict:** **PASS.** All five criteria are evidenced: criteria 1, 2, 4 as cleanly as
  the original PASS (criterion 1 with the added, non-disqualifying `server/discover`
  behavior note); criterion 5 with stronger evidence than both the original PASS and Box
  B (the dialog's own text, captured verbatim before acceptance); criterion 3 as
  detailed above — order preserved, nothing dropped, nothing interleaved, judged
  directly against the criterion's literal wording. **Operator decision, 2026-09-28
  (issue #39, chat):** G1 = PASS on Claude Code `v2.1.283`, Box C is the verdict-bearing
  run; Box B remains recorded as an incomplete attempt, not part of the pass/fail
  record; the original `v2.1.282` PASS (2026-09-25) is superseded by this run and kept
  as history below. This PASS does **not** claim that mid-turn delivery is always
  batched together at one boundary — that specific claim is retracted to
  UNVERIFIED-as-a-guarantee (`docs/planning/v0.1/11-risks.md` row 49) — only that
  criterion 3's actual wording (order, no drop, no interleave) is satisfied.

  `STATUS.md`'s G1 gate-verdict table, the pins table's G1 pointer, and every document
  that mirrors G1's STATUS cell (`02-gating-findings.md`,
  `03-decisions-and-amendments.md`, `04-architecture.md`, `06-security.md`,
  `08-cli-and-deployment.md`, `09-test-strategy.md`, `10-stages.md`, `11-risks.md`,
  `01-capability-matrix.md`, `decisions/C6-trust-rendering.md`, `PINS.md`, and the
  Stage 1 exit blocker lines) are synced to this PASS verdict in the same change that
  landed this section.
- **Fallback taken:** not applicable — G1 has no fallback; the primary path passed
  outright.
- **UNVERIFIED items:**
  - **Resolved by operator decision, 2026-09-28:** whether criterion 3's wording is
    satisfied by delivery at two separate tool-call boundaries — **yes**, judged against
    the criterion's literal text (see "Pass criteria evaluated" above).
  - **Still UNVERIFIED as a guarantee** (not resolved by the PASS decision above,
    tracked separately): whether mid-turn notification delivery batches together at a
    single tool-call boundary or can arrive at separate boundaries — may depend on
    relative send timing. The original PASS's "arrived together between tool calls" and
    Box C's "two separate boundaries" are each one data point; not enough to say which
    is a guarantee and which is an artifact of timing. `PLANNING-PROMPT.md` §3.1,
    `oac-claude-channels`, `oac-gates/references/G1-claude-wake.md`, and
    `docs/planning/v0.1/04-architecture.md` are each amended with a dated note pointing
    here; see `docs/planning/v0.1/11-risks.md` row 49 and `docs/planning/STATUS.md`.
  - Channel behavior across `--resume` — not tested this spike or any re-run, still
    open.
  - Whether one server can present more than one logical channel — not tested, still
    open.
  - `CLAUDE_SESSION_ID` environment variable — already closed by B2: confirmed absent.
  - Agent SDK support for Channels — already closed by B2: confirmed absent.
  - Separate, non-criterion finding: the `server/discover` probe reproduces again in
    this box (lines 3/13), with `MCP_SDK_GENERATION` confirmed unset in this terminal
    (same terminal session as Box B, where the operator ran `Remove-Item` before
    launch). Occurrences of this probe on 2026-09-28 span two working directories:
    `d6-spike` (D6 Box A, where `_meta.mcp_sdk_generation` was recorded as `"v2"`) and
    `g1-spike` (Boxes B and C, where the env var was confirmed empty) — a third
    directory, `g4-spike`, is where the original G4 spike observed the same probe.
    Box A is evidence the probe fires with `MCP_SDK_GENERATION=v2`; it is not evidence
    for the empty-var case, which rests on Box B and Box C alone. See
    `docs/planning/v0.1/11-risks.md` row 42.
- **Fixtures captured:**
  - `docs/planning/gates/fixtures/g1-claude-wake/transcript-2026-09-28-2.1.283-boxC.jsonl`
    — Box C, 25 lines, this run's own transcript.
  - `docs/planning/gates/fixtures/g1-claude-wake/transcript-2026-09-28-2.1.283.jsonl` —
    Box B, 14 lines, the earlier incomplete attempt (kept, not deleted).
  - Both redacted with the `redact.mjs` that actually ran (sha256
    `975de80c6462bdf1bbc21ee05e37730cde5cdf7ef74e42a556270a46ccbd9a3d`); the committed,
    quarantined, public-safe copy at `docs/planning/gates/fixtures/d6-codex-protocol/
    redact.mjs.throwaway-quarantined` has a different sha256
    (`371d79640a661180e15e3c4afc788531567f1bbbfdf286ac542608aecf381872`) because it no
    longer hardcodes the operator's username/hostname/installation id — see that
    directory's `README.md` "Files". `droppedHazardLines=0`, `residualLeaks=[]`,
    `residualGenericHits=[]` on both files.
- **Pin rows relied on:** `Claude Code (Channels)`, `MCP — current era`, `MCP — legacy
  era`, `Rust MCP SDK (rmcp)` (unchanged from the original PASS).
- **PINS.md as-of:** 2026-09-27, commit `f5adee2ad6595a1e650feda89487ae92e0659d4f` (the
  commit that last changed `PINS.md`, part of merged PR #120/issue #39 T0 — PR #120's
  own merge commit is `2f916d9a0e8e4ccb088e9c9937ab63b611affd3b`, a distinct commit;
  Claude Code (Channels) row is floating, last observed `v2.1.283`).
- **Version triple:** `claude --version` = `2.1.283 (Claude Code)`, checked pre-box and
  post-box for both Box B and Box C; wire `clientInfo.version` = `"2.1.283"` (present
  in both boxes' `initialize` request frames); transport user-agent N/A (stdio carries
  none, same finding as the original PASS).
- **Re-run history:**

  | Date | Pinned versions | Verdict | Invalidated by |
  |---|---|---|---|
  | 2026-09-25 | Claude Code v2.1.282 (observed; PINS.md pin was v2.1.274, now stale — see "Pin drift" under "Original run" below) | PASS | Claude Code pin `v2.1.274` -> floating (last observed `v2.1.283`), 2026-09-27 — superseded 2026-09-28 by the v2.1.283 re-run below, kept as history |
  | 2026-09-28 (Box B) | Claude Code v2.1.283 (observed pre- and post-box; matches PINS.md's floating last-observed) | **INCOMPLETE** — see "Re-run attempt 1 (Box B)" below. Several pass-criteria probes were not attempted; not a verdict. | n/a — recorded as an incomplete attempt, not verdict-bearing |
  | 2026-09-28 (Box C) | Claude Code v2.1.283 (observed pre- and post-box; matches PINS.md's floating last-observed) | **PASS** — see "Pass criteria evaluated" above. Operator decision 2026-09-28: criterion 3 satisfied as worded (order preserved, nothing dropped, nothing interleaved); the delivery-at-two-boundaries pattern is recorded as an observed `2.1.283` behavior change, not a failure. **This is the current, verdict-bearing result** (all top-level fields on this page describe this run). | n/a — current verdict |

  *Dated note, 2026-10-01 (#216):* by operator decision, harness versions now float and
  warn, never gate. The 2026-09-27 invalidation in the first row is history. A later Claude
  Code version (PINS.md last tested `v2.1.285`, minimum `v2.1.282`) does not invalidate
  the current PASS, which stands on `v2.1.283`. The verdict is unchanged.

### Scripted re-run through herdr (K4) — pointer only, not verdict-bearing

A herdr-driven re-run of this gate's Box C probes exists as test tooling (Epic K, K4
issue #127): `tools/herdr/scenarios/g1-claude-wake.mjs`, compared against Box C by
`tools/herdr/lib/g1-report.mjs`. It never changes the verdict above. First live record:
[`herdr-runs/G1-2026-09-29.md`](herdr-runs/G1-2026-09-29.md) (run
`20260929T034856Z-05b135`, herdr 0.9.1, Claude Code 2.1.283, human accept; all five
criteria scored equivalent to Box C), with its run manifest beside it.

### Original run (2026-09-25, v2.1.282) — superseded

- **Pinned version(s):** Claude Code `v2.1.282` (the version actually observed connecting
  — see "Pin drift" below; `docs/planning/PINS.md` recorded `v2.1.274` at spike time),
  MCP legacy revision `2025-11-25`.
- **Date:** 2026-09-25
- **Timebox:** No formal timebox was set before starting (process deviation, noted
  below); elapsed wall-clock was approximately 1.5 hours, most of it spent on
  setup/tooling (framing-protocol misdiagnosis, CLI flag-syntax trial-and-error) before
  the actual spike ran cleanly. All five pass criteria were confirmed well within a
  reasonable box once the correct invocation was found.
- **Command transcript summary:** A throwaway Node.js stdio MCP server
  (`docs/planning/gates/fixtures/g1-claude-wake/channel-server.mjs.throwaway-quarantined`)
  was registered via a project `.mcp.json` and launched with
  `claude --dangerously-load-development-channels server:g1spike` (human operator, real
  terminal — the interactive confirmation dialog cannot be driven from a non-TTY tool).
  The operator approved the "local development" warning dialog at startup. The server
  negotiated MCP `2025-11-25` and declared `capabilities.experimental["claude/channel"]`.

  This took six-plus session restarts to reach a clean pass, not one continuous run —
  recorded honestly here since the fixture transcript shows it plainly. The server
  process was relaunched (fresh `initialize`/`notifications/initialized` handshake each
  time, new pid) repeatedly: first while an incorrect wire-framing guess was being
  debugged (pre-fix, no real Claude connection reached the server at all — see "Process
  notes"), then once more after the CLI invocation was corrected but with the wrong
  flag combination (`--channels` + `--dangerously-load-development-channels` together),
  which connected the server as an ordinary MCP tool provider but never registered it
  as an actual channel — a wake notification was sent at this point
  (transcript.jsonl line 26, `g1-spike-wake-test-1`) and never observed, because the
  session never received channel treatment for it. After correcting the invocation to
  `claude --dangerously-load-development-channels server:g1spike` alone, a fresh session
  registered the channel correctly and a second wake notification (line 61, same id
  reused) was confirmed received. A premature mid-turn notification (line 62,
  `g1-spike-midturn-test-2`) was then sent while Claude was not actually mid-turn — a
  coordination mistake, not evidence either way — and discarded. After one more
  `/mcp reconnect` (to pick up a code fix making the mid-turn trigger re-armable) and a
  deliberately long, genuinely busy foreground turn, the valid mid-turn pair (lines
  71-72) was captured, followed by the tool-based reply (lines 73-74). In total the raw
  transcript records seven `notifications/claude/channel` sends across the session; the
  pass/fail record above is evidenced by the last, valid instance of each test, with the
  earlier invalid or unobserved attempts disclosed here rather than omitted. Full raw
  JSON-RPC transcript: `docs/planning/gates/fixtures/g1-claude-wake/transcript.jsonl`.
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
        finished your task first."* **2026-09-28 addendum:** a later re-run (Box C, see
        above) sent two mid-turn notifications and observed them delivered at two
        *separate* tool-call boundaries instead of together at this one — see the
        top-level "Pass criteria evaluated" section above for the full comparison; this
        historical record is left as originally written.
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
        earlier session in the same directory. Not re-tested at the time this run was
        recorded; **2026-09-28 addendum:** two later re-runs in this same directory, Box
        B and Box C, also did not see a second dialog — see the top-level "Pass criteria
        evaluated" section above. This is now cross-run evidence, not speculation alone,
        though still not a direct confirmation of the mechanism. The specific ADR-001
        consent step this criterion names is the development-channels warning, and that
        one was exercised every time.)
- **Verdict:** PASS on Claude Code `v2.1.282` (2026-09-25) — the original, historical
  verdict. Superseded 2026-09-28 by a `v2.1.283` re-run (Box C, see the top-level
  "Verdict" field above and the callout at the top of this file), which is now the
  **current** verdict: PASS. This record is kept unchanged as history, per the Re-run
  layout convention — its own Date/Timebox/Pinned-version fields describe this
  `v2.1.282` run only.
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
  **Superseded 2026-09-28 by
  `docs/planning/gates/fixtures/g1-claude-wake/transcript-2026-09-28-2.1.283-boxC.jsonl`**
  (`MANIFEST.json`'s `superseded_by` field on this entry now points there).
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
  - Getting from zero to a clean pass took six-plus separate server-process restarts
    across roughly 40 minutes of live back-and-forth with the operator (see the
    restart-by-restart account in "Command transcript summary" above) — a fresh MCP
    handshake each time, most of them spent narrowing down the wrong wire framing and
    the wrong CLI flag combination rather than testing the actual gate criteria. Once
    the invocation was correct, every remaining criterion passed on the first or second
    real attempt.

### Re-run attempt 1 (Box B), 2026-09-28, Claude Code v2.1.283 — incomplete attempt

Same HIL sitting as the D6 Claude protocol capture (`docs/planning/gates/fixtures/
d6-claude-protocol/`), under its own declared 45-minute timebox (opened
2026-09-28T04:44:55Z, closed 04:56:46Z — not expired; ~33 minutes of the box remained
unused at close). Same throwaway server as the original PASS, unmodified:
`docs/planning/gates/fixtures/g1-claude-wake/channel-server.mjs.throwaway-quarantined`.
Full raw transcript, redacted, 14 lines: `docs/planning/gates/fixtures/g1-claude-wake/
transcript-2026-09-28-2.1.283.jsonl`.

Per-criterion evaluation, judged against `G1-claude-wake.md`'s exact wording:

- **Criterion 1** ("Server declares `capabilities.experimental["claude/channel"] = {}`
  and negotiates a legacy MCP revision"): **evidenced, with a version-level behavior
  change worth recording, not a criterion failure.** Transcript lines 5-6: the real
  Claude Code client (`clientInfo: {"name":"claude-code","version":"2.1.283"}`) sent
  `protocolVersion: "2025-11-25"`; the server's
  `capabilities.experimental["claude/channel"]` was accepted. **Different from the
  original PASS:** lines 3-4 of this transcript show a `server/discover` request
  (`_meta["io.modelcontextprotocol/protocolVersion"]: "2026-07-28"`) answered with a
  legacy-shaped `-32601` error *before* the client falls back to the legacy `initialize`
  at line 5. The original `v2.1.282` transcript, captured with the same launch command in
  the same directory, carries **zero** `server/discover` frames (grep-confirmed against
  the committed `docs/planning/gates/fixtures/g1-claude-wake/transcript.jsonl`). This is a
  same-launch, cross-version protocol difference on Claude Code's side, not measurement noise, and not
  itself a criterion-1 failure (the legacy negotiation still succeeds after the probe is
  refused) — recorded as a version-level behavior change.
- **Criterion 2** ("A `notifications/claude/channel` ... wakes an idle session as a user
  turn and appears as a `<channel>` tag with the expected attributes"): **partially
  re-tested.** The wake itself is directly evidenced (transcript line 11,
  `g1-spike-wake-test-1`, t=`2026-09-28T04:47:34.801Z`; the operator's paste confirms
  Claude took a new turn unprompted). **Not re-tested this box:** the exact `<channel>`
  tag attribute set (`source`/`oac_message_id`/`oac_sender`) and the silent drop of the
  non-identifier-safe `meta` key — the original PASS confirmed both explicitly; this box's
  operator was not asked to query them.
- **Criterion 3** ("A second notification sent mid-turn is queued and delivered at the
  next turn, in order (not dropped, not interleaved out of order)"): **narrower evidence
  than the original PASS, not independently conclusive.** During a genuinely busy turn
  (an operator counting task), the single mid-turn notification sent this box
  (`g1-spike-midturn-test-2`, transcript line 12, the *only* mid-turn notification this
  box sent) rendered between count steps 3 and 4 — at a tool-call step boundary inside
  the still-busy turn, not interleaved into step 4 itself; Claude's own summary: "It
  didn't cut into the count: it was held and delivered once that step finished." The
  original PASS specifically sent **two** mid-turn notifications to test relative
  ordering; this box sent only one, so it cannot independently re-confirm the "in order"
  element of the criterion's wording at all.
- **Criterion 4** ("Claude replies through an ordinary MCP tool"): **evidenced.**
  Transcript lines 13-14: a `reply` tool call, `in_reply_to
  g1-spike-midturn-test-2`; channel confirmed receipt.
- **Criterion 5** ("The `--dangerously-load-development-channels` interactive
  confirmation dialog is actually exercised"): **the dialog was exercised, but its exact
  text was not captured before the operator accepted it.** The operator recalls
  confirming a dialog at startup; what was actually captured is the *post-accept* startup
  banner text ("Channels (experimental) messages from server:g1spike inject directly in
  this session · restart without --dangerously-load-development-channels to stop" plus "1
  more notice hidden") — not the dialog's own prompt text, which the original PASS did
  quote directly. The criterion's core requirement (dialog exercised, not bypassed) is
  met; the evidentiary record for exactly what it said is thinner than the original.

**Why this box is recorded as an incomplete attempt, not a verdict:** the run sheet for
this box did not include probes for the `<channel>` attribute set, the dropped-key check,
a second mid-turn notification for ordering, or capturing the dialog text before
acceptance — a run-sheet/procedure gap identified by the operator afterward, not a
time-limit problem (the box closed with roughly 33 of its declared 45 minutes still
available). The operator ordered a fresh re-run (Box C, see the top-level fields above)
rather than treating this box's narrower evidence as sufficient.

**Separate, non-criterion finding:** `$env:MCP_SDK_GENERATION` was confirmed empty in
this Box B terminal (checked directly, after `Remove-Item`), yet the transcript still
shows a `server/discover` probe (line 3) — see `docs/planning/v0.1/11-risks.md` row 42
and `docs/planning/STATUS.md`.
