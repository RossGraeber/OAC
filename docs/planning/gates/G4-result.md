### G4 mcp-dual-era

- **Gate id:** G4
- **Pinned version(s):** Claude Code `v2.1.283` (client-reported on the wire; the
  `docs/planning/PINS.md` pin is `v2.1.274`, already flagged stale by G1 at `v2.1.282`);
  MCP current era `2026-07-28`; MCP legacy era `2025-11-25` (both negotiated live, on the
  same server process); `@openai/codex` `0.157.1` (Codex leg; matches the Codex row's
  current last-observed version in `docs/planning/PINS.md`, commit
  `36650394c5b38c2990ccf2a3457165ca3e9d9726`). Node.js `25.2.1`, Windows only.
  - Client-reported wire versions, taken from the fixture, not assumed: Claude's
    `clientInfo.version`/`user-agent: claude-code/2.1.283 (cli)` both read `2.1.283`
    (`transcript-2026-09-26.jsonl` lines 5, 6, 10, 16, 24, 53); Codex's HTTP
    `user-agent` header reads `codex-mcp-client/0.157.1` on every HTTP request (lines
    26, 31, 48, 50); its stdio `initialize` (line 42, no HTTP headers over stdio) carries
    the same version only in `clientInfo.version`; and `x-codex-turn-metadata.
    codex_version` also reads `0.157.1`, present only on the two `tools/call` requests
    (lines 48, 50).
  - **Rust MCP SDK (`rmcp`) `3.4.0` was not exercised.** The throwaway spike server
    (`g4-server.mjs`, quarantined, not committed) is hand-rolled Node.js, not built on
    `rmcp`. `docs/planning/PINS.md`'s "Gates affected" column still names `rmcp` as
    relied on by G4 because the eventual production server will be built on it; this
    spike proves the *protocol* coexistence, not that `rmcp` itself negotiates the
    legacy era correctly at runtime. `docs/planning/STATUS.md`'s open item "Whether an
    `rmcp`-based OAC server ... registers as a channel" stays open (see "UNVERIFIED
    items").
- **Date:** 2026-09-26 (fresh, redeclared timebox; supersedes the 2026-09-25/26
  out-of-box run below).
- **Timebox:** 60 minutes, declared 2026-09-26T20:46:03Z (Claude Code session log,
  tool-result timestamp, exact wording: "G4 re-run timebox declared
  2026-09-26T20:46:03Z (60 min live work)"). Live work closed at 2026-09-26T20:54:30Z —
  about 8.5 minutes elapsed, well inside the 60-minute box. **Not expired.** Stale
  sessions were closed and their servers exited (confirmed with a port/process check
  before proceeding), and the previous (out-of-box) transcript archived before the box
  opened, so this run started clean.
- **Command transcript summary.** One throwaway Node.js process, `g4-server.mjs`
  (quarantined; not committed — architecture unchanged from the prior run: one stdio
  surface answering as a legacy channel or, under `G4_STDIO_MODERN=1`, as a modern-only
  server for the negative case; one HTTP surface at `127.0.0.1:17448/mcp` speaking both
  eras, dispatching on `_meta["io.modelcontextprotocol/protocolVersion"]` vs.
  `initialize`/`Mcp-Session-Id`). Launched with
  `claude --dangerously-load-development-channels server:g4spike server:g4modern` from
  the `g4-spike` scratchpad directory, `$env:MCP_SDK_GENERATION="v2"`. Fixture
  `docs/planning/gates/fixtures/g4-mcp-dual-era/transcript-2026-09-26.jsonl` (57 lines;
  every line cite below independently re-checked against this file):

  | Lines | UTC | pid | What happened |
  |---|---|---|---|
  | 1-2 | 20:47:20.455-.467 | 19680 | `g4spike` starts: stdio (legacy-capable) + HTTP on `127.0.0.1:17448/mcp`. This is the pid that serves both eras for the rest of the run. |
  | 3-4, 5, 7, 9 | 20:47:20.471-.507 | 18032 | `g4modern` (negative-case server, `G4_STDIO_MODERN=1`) starts on port 17450, answers stdio `server/discover` as modern-only: `supportedVersions: ["2026-07-28"]`, still declaring `experimental: {"claude/channel": {}}` (line 7); arms its wake trigger (line 9). |
  | 6, 8 | 20:47:20.508-.510 | 19680 | Claude probes `g4spike`'s **stdio** surface with `server/discover` carrying `_meta["io.modelcontextprotocol/protocolVersion"]: "2026-07-28"` — answered `-32601 Method not found` (the stdio surface behaves as a legacy server pre-`initialize`). |
  | 10-11 | 20:47:20.559-.563 | 19680 | Claude's **HTTP** `server/discover` succeeds, modern, `resultType: "complete"`, `supportedVersions: ["2026-07-28","2025-11-25"]`. |
  | 12-13 | 20:47:20.625-.626 | 18032 | Claude's stdio `tools/list` against `g4modern`, modern era. |
  | 14-15 | 20:47:20.643-.644 | 19680 | Claude falls back to **legacy stdio `initialize`** on `g4spike`: negotiates `2025-11-25`, gets `capabilities.experimental["claude/channel"]`. |
  | 16-17 | 20:47:20.743-.756 | 19680 | Claude's HTTP-modern `tools/list` on `g4spike`, `ttlMs`/`cacheScope` present (no repeat-polling this run — the earlier caching-hint bug stays fixed). |
  | 18-21 | 20:47:20.801-.862 | 19680 | `notifications/initialized`; wake trigger armed; legacy `tools/list`. |
  | 22 | 20:48:28.883 | 18032 | `g4modern` (modern-only stdio) pushes `notifications/claude/channel`, `oac_message_id: "g4-1"`. **First wake, modern side.** |
  | 23 | 20:48:29.123 | 19680 | `g4spike` (legacy stdio) pushes the same kind of wake 240ms later, also `oac_message_id: "g4-1"` (independent per-server counters — not a duplicate of line 22). **First wake, legacy side.** |
  | 24-25 | 20:50:02.530-.532 | 19680 | Claude's HTTP-modern `tools/call` for `g4_echo("hello from claude over http")`. Request headers carry `mcp-method`, `mcp-name`, `mcp-protocol-version: 2026-07-28`; body `_meta` carries `io.modelcontextprotocol/protocolVersion: "2026-07-28"` (line 24). Response `_meta` carries `io.github.rossgraeber/oac-session-channels: {"served_by_pid":19680,"surface":"http-modern",...}` (line 25). |
  | 26-39 | 20:51:21.114-.152 | 19680 | Two independent `codex-mcp-client/0.157.1` HTTP sessions each `initialize` with `protocolVersion: "2025-06-18"` — negotiated down to legacy (`Mcp-Session-Id` issued, server answers `protocolVersion: "2025-11-25"`), `notifications/initialized`, a `GET` (405, legacy SSE probe), `tools/list`. Neither ever sends a modern (`2026-07-28`) request. |
  | 40-47 | 20:51:21.322-.337 | 17580 | A **separate** `codex-mcp-client/0.157.1` process spawns `g4spike` over **stdio**, configured via the scratchpad's own `.codex/config.toml` (`[mcp_servers.g4spike]`, `command`/`args` pointing at `g4-server.mjs`) — a different registration from the `codex mcp add g4 --url http://127.0.0.1:17448/mcp` (global, HTTP) registration that produced pid 19680's Codex traffic above. Its own HTTP listen attempt fails `EADDRINUSE` on line 41, because pid 19680 already holds port 17448 — confirming this is a distinct connection, not the same process. Negotiates legacy the same way (`protocolVersion: "2025-06-18"` -> `2025-11-25`), arms its own wake trigger. |
  | 48-49 | 20:51:52.588-.589 | 19680 | Codex (HTTP legacy, session `6c12f2bc...`) calls `g4_echo("hello from codex 0.157.1")`; result carries OAC `_meta` provenance. |
  | 50, 52 | 20:51:52.612-.613 | 19680 | Codex calls `g4_relay_to_claude("codex relay during modern session")`; the tool result acknowledges the relay. |
  | 51 | 20:51:52.612 | 19680 | **The same pid's stdio legacy channel** pushes `notifications/claude/channel` with `meta.relay_from: "http_legacy"`, `oac_message_id: "g4-2"` — the same millisecond as the triggering call (line 50), not 1ms after as initially estimated; both timestamps round to `20:51:52.612Z`. |
  | 53-54 | 20:53:10.108-.109 | 19680 | Claude's HTTP-modern `tools/call` **again**, after all the Codex traffic above: `g4_echo("modern still healthy after codex traffic")`, still `resultType: "complete"` with correct `_meta` provenance. **This is the no-degradation check** — the same pid's modern path is unaffected by the intervening legacy/Codex activity. |
  | 55 | 20:53:54.299 | 19680 | Second wake, legacy: `oac_message_id: "g4-3"` (`g4spike`). |
  | 56 | 20:53:54.307 | 17580 | Second wake, legacy, from Codex's own spawned `g4spike` copy: `oac_message_id: "g4-1"` (this process's own independent counter). Codex's copy has no channel consumer — it is not registered with Claude Code — so this push has nowhere to arrive; recorded on the wire only. |
  | 57 | 20:53:54.375 | 18032 | Second wake, modern (`g4modern`): `oac_message_id: "g4-2"`. |

  **Operator-observed in the Claude UI, quoted verbatim from the session log** (pasted
  content, `<pasted_content id="df6f">` blocks and assistant turns, 2026-09-26T20:4x-20:5x):
  1. After launch: `"Channel messages from \"g4modern\" are unavailable: this
     connection's prot…"`. The same `/mcp` paste listed `g4http`, `g4modern` and
     `g4spike`, each `"√ … 2 tools"` — i.e. `g4modern` still works as an ordinary MCP
     server, just not as a channel.
  2. First wake: `"← g4spike: G4 wake test from the LEGACY (2025-11-25 channel) stdio
     sur…"`. Claude identified it as message `g4-1`, from pid `19680`. **Only the
     legacy wake (line 23) arrived; the modern push (line 22, pid 18032) was not
     observed arriving.**
  3. The operator's prompt for this call itself asked to "show me the full result
     including `_meta`" (same request, not a separate follow-up). Claude reported:
     `"The call to mcp__g4http__g4_echo worked, but no _meta reached me. The whole
     result I got was: g4 echo: hello from claude over http"` — **Claude Code does not
     surface a tool result's `_meta` to the
     model**, even though it is present on the wire (line 25). Recorded as a new
     observation, not a criterion failure (see criterion 2 below).
  4. Codex's own transcript, pasted by the operator: `"• Called g4.g4_echo
     └ g4 echo: hello from codex 0.157.1"` and `"• Called g4.g4_relay_to_claude
     └ relayed to Claude channel (written to stdio transport; no ack exists)"`.
  5. Relay arrival: `"← g4spike: relayed from http-legacy: codex relay during modern
     session"`, identified by Claude as message `g4-2`, `relay_from="http_legacy"`.
  6. After the Codex traffic, Claude's second modern echo returned:
     `"g4 echo: modern still healthy after codex traffic"`.
  7. Second wake: Claude reported `"Wake test g4-3 from the legacy stdio channel
     arrived. It comes from the same process as g4-1 (pid 19680, stdio era legacy)"` —
     **again only the legacy push (line 55) arrived; the modern push (line 57, pid
     18032) and Codex's own spawned copy's push (line 56, pid 17580, no channel
     consumer) were not observed arriving.**
- **Pass criteria evaluated** (verbatim, `.claude/skills/oac-gates/references/G4-mcp-dual-era.md`):
  - [x] **The legacy path registers as a channel (negotiates `2025-11-25` or earlier) and
        delivers notifications successfully.** `g4spike` (pid 19680) negotiated
        `initialize` with `protocolVersion: "2025-11-25"` and
        `capabilities.experimental["claude/channel"]` (lines 14-15). Two pushed
        notifications reached Claude's live session and rendered, both confirmed
        verbatim in the operator's UI paste: the first wake as message `g4-1` (line 23,
        UI observation 2) and the Codex-triggered relay as message `g4-2` (line 51, UI
        observation 5).
  - [x] **The current-revision (`2026-07-28`) path serves `tools/call` correctly,
        carrying OAC `_meta` provenance** (an OAC-defined `_meta` extension key on the
        response, not the protocol-version mechanism below). Claude's HTTP-modern
        `tools/call` for `g4_echo` (lines 24-25, repeated at lines 53-54 after Codex
        traffic) returned `_meta["io.github.rossgraeber/oac-session-channels"]` both
        times. This key's second label is `session-channels`, not `modelcontextprotocol`
        or `mcp`, so it does not collide with the reserved-prefix concern the criterion
        names — though that concern's own premise is itself unconfirmed (`oac-mcp`'s pin
        record: no such reservation clause was found in the SEP-2133 text during B2
        re-verification; this does not change the call here either way).
        - **New observation (not a criterion failure):** Claude Code does not surface
          this `_meta` to the model — confirmed on the wire (line 25) but reported by
          Claude itself as absent when asked (UI observation 3). The criterion is about
          the server carrying the provenance, which it does; whether a harness's own
          client exposes it to the model is outside what this criterion tests.
        - **On the Codex leg's era — read against the reference's own words.** The G4
          reference (`.claude/skills/oac-gates/references/G4-mcp-dual-era.md`, "Surfaces
          and version pins") states verbatim: "`codex mcp add` registers **external**
          MCP servers Codex calls as tools — this is the supported outbound surface the
          current-revision path in this gate must work against." Codex **did** work
          against this server that way — `codex mcp add g4 --url
          http://127.0.0.1:17448/mcp` was run (globally) before this transcript, and the
          resulting HTTP traffic (lines 26-39, 48-52) is what that registration produced
          — but every one of those requests negotiated `protocolVersion: "2025-06-18"`,
          never `2026-07-28` (lines 26, 31). Its opt-in, `stage: UnderDevelopment`,
          `default_enabled: false` `mcp_2026_07_28` client mode
          (`docs/planning/PINS.md`, `docs/planning/REVERIFICATION-B2.md`) was not
          exercised: a `codex --enable mcp_2026_07_28` probe was already on the spike's
          own pending-tasks list, with about 51 minutes of the 60-minute box still
          remaining when this run's live work ended, and it was **not attempted** —
          recorded here plainly, not omitted. **So the current-revision (`2026-07-28`)
          leg of this criterion is proven with the Claude Code client only; the
          reference's own text expects Codex to be the one working against this path,
          and in this run Codex reached the server exactly as the reference describes
          but stayed on the legacy era throughout.** This criterion is still evaluated
          [x] because its own text ("serves `tools/call` correctly, carrying OAC `_meta`
          provenance") is satisfied by the modern path functioning correctly for the
          client that did negotiate it — but the gap above is real, not a technicality,
          and it stays a tracked open item (`docs/planning/v0.1/11-risks.md` row 41,
          RISK-G4; `docs/planning/v0.1/03-decisions-and-amendments.md` conflict-register
          row C5, which stays `ASSIGNED` for exactly this reason).
  - [x] **Separately, every request on the `2026-07-28` path carries
        `_meta["io.modelcontextprotocol/protocolVersion"]`.** All seven modern-era
        requests in this run carry this key set to `"2026-07-28"`, individually
        verified: three over stdio (line 5, Claude's `server/discover` probe against
        `g4modern`, pid 18032; line 6, the equivalent probe against `g4spike`'s stdio
        surface, pid 19680; line 12, Claude's `tools/list` against `g4modern`, pid
        18032) and four over HTTP (lines 10, 16, 24, 53, all Claude against `g4spike`'s
        HTTP surface, pid 19680). Not applicable to Codex's requests,
        since Codex never negotiated the modern era.
  - [x] **Neither path degrades the other across a full session (run both concurrently,
        not sequentially, and confirm neither breaks).** A single process, pid 19680,
        held the legacy stdio channel open continuously from 20:47:20.643Z
        (`initialize`, line 14) through 20:53:54.299Z (second wake, line 55) — about 6
        minutes 34 seconds — while the same pid's HTTP surface served Claude's modern
        `tools/call` twice (lines 24-25 and, **after** all of Codex's legacy HTTP
        traffic, lines 53-54) and Codex's two independent legacy sessions (lines 26-39,
        48-52). The second Claude modern call (lines 53-54) succeeding with correct
        `_meta` immediately after the Codex relay (line 52) is the direct
        no-degradation evidence the criterion asks for — not just concurrent presence,
        but the modern path proven still-correct *after* legacy/Codex activity
        interleaved with it.
  - [x] **A server negotiating `2026-07-28` is confirmed to be rejected as a channel** —
        run this negative case explicitly. `g4modern` (pid 18032) declared
        `capabilities.experimental["claude/channel"]` and answered `server/discover`
        with `supportedVersions: ["2026-07-28"]` only (line 7). The operator's own
        Claude Code UI, pasted verbatim: `"Channel messages from \"g4modern\" are
        unavailable: this connection's prot…"` (UI observation 1), while `g4modern`
        remained listed and functional as an ordinary MCP server (`"√ … 2 tools"`).
        **Registration was refused once** (the single UI confirmation above), and
        **delivery failed twice**: both of its wake-test pushes (lines 22 and 57) were
        not observed arriving in Claude, in contrast to the legacy server's equivalent
        pushes (lines 23 and 55), which both arrived.
- **Verdict:** PASS. **Caveat, prominent by design, not a footnote:** the
  current-revision (`2026-07-28`) leg of criterion 2 is proven with **Claude Code as the
  modern client only**. Codex `0.157.1` reached this server exactly the way the
  reference expects — `codex mcp add g4 --url http://127.0.0.1:17448/mcp`, matching the
  reference's own "supported outbound surface" language — but negotiated the legacy era
  (`2025-06-18`) on every request in both G4 runs. A `codex --enable mcp_2026_07_28`
  probe was on the spike's own pending list with ~51 minutes of this run's box still
  remaining, and was not attempted. See criterion 2 above and
  `docs/planning/v0.1/11-risks.md` row 41 (RISK-G4).
- **Fallback taken:** none — not needed. The primary single-process design passed on
  every criterion within a redeclared, unexpired timebox.
- **UNVERIFIED items:**
  - **Codex's modern-era (`2026-07-28`) leg remains untested at the default client
    behavior.** Codex `0.157.1` never negotiated the current era against this server in
    either the out-of-box run or this one; its opt-in `mcp_2026_07_28` client mode
    (`stage: UnderDevelopment`, `default_enabled: false`) was not exercised in this run.
    **Partially narrowed by the 2026-09-27 "Row-41 probe addendum" below:** behind the
    opt-in flag, Codex `0.157.1` **can** negotiate `2026-07-28` against this server — the
    default (flag-off) behavior stays untested and unchanged. Tracked at
    `docs/planning/v0.1/11-risks.md` row 41 / RISK-G4 and the matching
    `docs/planning/STATUS.md` open item; this PASS does not close it, because criterion
    2 is satisfied via Claude as the modern client (see criterion 2 above), not because
    Codex's default leg was confirmed.
  - **`rmcp`-based server registering as a legacy-era channel** stays open
    (`docs/planning/STATUS.md`) — this spike used a hand-rolled Node.js server, not
    `rmcp`.
  - **New observation, not previously recorded:** Claude Code's tool-call interface does
    not surface a result's `_meta` to the model, even though it is present on the wire.
    This matters for OAC design: any provenance or correlation data OAC's own tools put
    in `_meta` is not visible to Claude's model through an ordinary `tools/call` result
    and must be delivered another way (e.g. in the visible `content`, or via the channel
    mechanism) if the model needs to act on it. UNVERIFIED whether this is universal or
    specific to this tool-call path. Added as `docs/planning/v0.1/11-risks.md` row 44
    (RISK-CLAUDE-PREVIEW) and `docs/planning/STATUS.md`'s open-items list in this same
    change; flagged here for whoever next touches Decision 8/9's provenance-rendering
    design (`docs/planning/decisions/C6-trust-rendering.md`).
  - **Carried unchanged from the superseded out-of-box run below** (still open, not
    reopened by this PASS): the docs-discrepancy item (Claude's v2 runtime probing
    stdio `server/discover` with `MCP_PROTOCOL_NEGOTIATION` unset — reproduced again
    this run at lines 6/8, same behavior) and the Codex-Desktop-threads-show-Claude-prompts
    security observation. Both are recorded in `docs/planning/STATUS.md` and
    `docs/planning/v0.1/11-risks.md` and are not restated in full here. **New addition
    to the Codex Desktop item:** twice while the out-of-box run's server (pid 16712)
    sat idle after that run ended — at 09:08:19Z and again at 15:57:32Z, both well
    before this PASS run's own 20:46Z-20:54Z window — a client reporting user-agent
    `codex-mcp-client/0.155.0-alpha.16.4` connected via the (likely shared) global
    `codex mcp add` registration, initialized and listed tools (legacy era, same as
    every other Codex connection in this gate); its MCP OAuth well-known discovery
    probes all returned 404. Attribution to Codex Desktop is inferred from the
    user-agent string alone, and the cause of these two isolated connections is
    UNVERIFIED. Cited to the uncommitted archive `scratchpad/g4-spike/
    transcript-2026-09-26-outofbox.jsonl` lines 60-73 (initialize through tools/list)
    and 76-89 (the same sequence repeated).
- **Fixtures captured:**
  - `docs/planning/gates/fixtures/g4-mcp-dual-era/transcript-2026-09-26.jsonl` (57
    lines) — **the primary fixture**, this PASS's evidence, described in full above.
    - **Redaction:** `scratchpad/g4-spike/redact.mjs` (kept with the uncommitted spike
      scratch, not committed) scrubs the spike's scratchpad path, the user home path, a
      hostname placeholder, an installation-id placeholder, and email addresses. Run
      against this transcript: 57 lines out, one residual string family
      (`rossgraeber`), confirmed to be only the intentional public OAC MCP extension
      identifier `io.github.rossgraeber/oac-session-channels` (already committed
      elsewhere, e.g. `docs/planning/PINS.md`) — not a leak. The raw transcript had no
      filesystem path, hostname, or account-metadata field to begin with (the server
      logs only protocol JSON), so this is a near no-op pass, run anyway per the
      redaction procedure.
  - `docs/planning/gates/fixtures/g4-mcp-dual-era/transcript-2026-09-26-outofbox.jsonl`
    (58 lines, renamed from this file's prior `transcript.jsonl`) — **kept, not
    dropped.** This covers **lines 1-58 of the archived scratchpad transcript**
    (`scratchpad/g4-spike/transcript-2026-09-26-outofbox.jsonl`, uncommitted, 90 lines
    total). Lines 59-90 of that archive are two isolated connections the same idle
    server process (pid 16712) received later that day, at 09:08:19Z and 15:57:32Z —
    both well after this run's own evidence window and well before the PASS run's
    20:46Z-20:54Z window; the process itself stayed alive (confirmed alive at 20:40:51Z
    in the session log) until the operator closed sessions around 20:45Z, not at
    15:57:32Z — stale-server traffic, not part of this run's own evidence, and **not
    committed**; see the Codex Desktop item under "UNVERIFIED items" above for what it
    contains. This is the superseded 2026-09-25/26 out-of-box run: every one of the
    five criteria individually confirmed the same way, but ruled `NOT RUN` because the
    confirming evidence was gathered after that run's declared timebox had expired (see
    "Re-run history" above). Kept for the same reason G2's superseded `0.154.0` baseline
    fixture was kept alongside its re-run fixture
    (`docs/planning/gates/G2-result.md`): it is independently redacted, cited by the
    "Re-run history" row above, and documents a real process-discipline finding (the
    expired-timebox case) that later spikes should not repeat. Same redaction pass, same
    result (no leaks beyond the extension identifier).
  - `docs/planning/gates/fixtures/g4-mcp-dual-era/transcript-run1.jsonl` (10 lines) —
    **kept, not dropped.** Unrelated to the timebox question: it documents a distinct,
    already-fixed bug (missing `ttlMs`/`cacheScope` caching hints causing Claude to
    re-issue the same HTTP-modern `tools/list` four times, Claude Code `2.1.282`, pid
    17880). Cheap to keep, already redacted, and is the only committed evidence of that
    bug's reproduction. **Label correction, kept for the record:** this file was
    originally described (in the task that requested this gate result) as "run 1 (Codex
    `0.154.0`)"; its actual content is Claude-only, dated 2026-09-26, with no Codex
    request anywhere in it. The real Codex-`0.154.0`-speaks-legacy-only evidence lives in
    the uncommitted `selftest2-transcript.jsonl` (see below), not in this file — recorded
    here rather than silently repeating the mismatched label, per `oac-evidence` §1's
    "never guess" instruction.
  - **Not committed, per the task's fixture list:** `selftest-transcript.jsonl` (local
    self-test, not gate evidence) and `selftest2-transcript.jsonl` (mixed self-test plus
    real, in-box Codex `0.154.0`/`0.157.0` HTTP-legacy traffic, cited in the superseded
    section below for the "Codex speaks legacy by default" finding). Both remain in
    `scratchpad/g4-spike/` for reference.
  - `g4-server.mjs` (the spike server) is **not committed** — quarantined in the
    scratchpad only, per the `oac-gates` throwaway rule.
  - `docs/planning/gates/fixtures/g4-mcp-dual-era/transcript-row41-2026-09-27.jsonl`
    (18 lines) — the row-41 probe addendum's evidence (2026-09-27, separate 20-minute
    box, after this gate's own `PASS`; does not change the verdict). See "Row-41 probe
    addendum" above.
- **Pin rows relied on:** `Claude Code (Channels)`, `Codex CLI / app-server`,
  `MCP — current era`, `MCP — legacy era`, `Rust MCP SDK (rmcp)` — exactly the set of
  `docs/planning/PINS.md` pin-table rows whose `Gates affected` cell names G4.
- **PINS.md as-of:** 2026-09-26, commit `b40dcdef5ef3e28dc22d325d2adb6c1f413c0540`.
- **Re-run history:**

  | Date | Pinned versions | Verdict | Invalidated by |
  |---|---|---|---|
  | timebox declared 2026-09-25T07:20:35Z (120 min); evidence gathered 2026-09-26, ~21-23h after box close | Claude Code `v2.1.283`; Codex CLI / app-server `0.157.1`; MCP current `2026-07-28` / legacy `2025-11-25`; `rmcp` not exercised; Windows only | NOT RUN — timebox expired before the confirming evidence was gathered; all five pass criteria individually confirmed, none failed | Superseded by a fresh, redeclared timebox, 2026-09-26 |
  | 2026-09-26 (box declared 2026-09-26T20:46:03Z, 60 min; live work 20:46:03Z-20:54:30Z, ~8.5 min, not expired) | Claude Code `v2.1.283`; Codex CLI / app-server `0.157.1`; MCP current `2026-07-28` / legacy `2025-11-25`; `rmcp` not exercised; Windows only | **PASS** | — (current) |
- **Process notes (for future gate spikes, not part of the pass/fail record):**
  - This re-run is the direct fix for the prior run's own worst finding: the operator
    closed the prior sessions, their servers exited, and the previous transcript was
    archived *before* the fresh timebox was declared, and the box was declared before
    any live work began (unlike the earlier
    run's mid-day resumption without redeclaring). Elapsed time (8.5 minutes) was
    computed and checked against the box before writing this result, not after.
  - The "1ms after" framing for the relay push (line 51) versus the triggering call
    (line 50) was corrected during this write-up: both timestamps round to the same
    millisecond in the fixture; recorded as "same millisecond," not a specific sub-ms
    gap that isn't actually measurable at this log resolution.
  - Codex's own spawned `g4spike` copy (pid 17580) again armed a wake trigger and pushed
    a notification (line 56) that has no channel consumer — Codex is not a Claude
    channel client, so this is expected, not a bug; recorded to make clear line 56 is
    not a missed delivery.
  - **"C5" is ambiguous in this project and was checked, not assumed.** A prior
    instruction to consult "decisions/C5" for how this gate's evidence bears on an
    MCP-era design choice was checked against `docs/planning/decisions/
    C5-envelope-auth.md` (envelope signatures/replay/pairing — no MCP-era content at
    all) and found inapplicable. The actual relevant record is conflict-register **row**
    C5 ("Claude needs legacy MCP; Codex tool path may negotiate current MCP",
    `docs/planning/v0.1/03-decisions-and-amendments.md` §3 conflict-register table, and
    `docs/planning/ADR-001-AMENDMENTS.md`). **C5 stays `ASSIGNED`** in both files — this
    PASS confirms the one-process topology (criteria 1, 4, and 5) but not the "Codex tool
    path may negotiate current MCP" element, since Codex never negotiated the modern era
    in either G4 run and no `rmcp`-based server has been tested; both files' Evidence
    cell is updated with a partial-resolution note citing this PASS, without changing the
    status. `docs/planning/decisions/C3-spec-packaging.md` §7 is updated the same way, in
    this same change. Do not conflate the envelope-auth decision document with the
    conflict-register row that happens to share the same letter-number.

#### Row-41 probe addendum (2026-09-27) — does not change G4's verdict

**Purpose.** `docs/planning/v0.1/11-risks.md` row 41 tracks Codex's opt-in
`mcp_2026_07_28` client mode (`stage: UnderDevelopment`, `default_enabled: false`) as
untested against a G4-shaped server. This probe exercises it once, on the same
throwaway `g4-server.mjs`, under its own separate timebox — it is not part of G5's
provenance verdict and is recorded here, on G4, because it is G4's own open item.

- **Timebox:** 20 minutes, declared 2026-09-27T06:42:35Z, closed 2026-09-27T06:42:59Z —
  well inside the box.
- **First attempt refused.** `codex exec --enable mcp_2026_07_28 "Call the g4_echo tool
  with text 'row41 modern probe' and print its result."` was refused outright:
  "Not inside a trusted directory and `--skip-git-repo-check` was not specified." The
  scratchpad directory is not a git repository, so `codex exec`'s trust check blocked
  the run before any MCP traffic occurred.
- **Rerun.** `codex exec --skip-git-repo-check --enable mcp_2026_07_28 "Call the
  g4_echo tool with text 'row41 modern probe' and print its result."` This is the run
  the evidence below comes from. `codex exec` printed the warning "Under-development
  features enabled: mcp_2026_07_28", used model `gpt-6-luna`, and ran with
  `approval: never`.
- **Server transcript:** `docs/planning/gates/fixtures/g4-mcp-dual-era/
  transcript-row41-2026-09-27.jsonl` (18 lines, pid `26152` for HTTP, pid `30452` for
  stdio). Earlier, unrelated server-activity lines sitting in the scratchpad transcript
  before this probe's own window were moved to the (uncommitted)
  `scratchpad/g4-spike/transcript-pre-row41-064223.jsonl` (83 lines), described here
  precisely:
  - Lines 1-57: the full, unrelated 2026-09-26 G4 re-run transcript (pid `19680`) —
    byte-identical to the raw scratchpad `transcript-2026-09-26-rerun.jsonl` (confirmed
    by direct diff), the uncommitted source the primary PASS fixture above was copied
    and redacted from.
  - Lines 58-73: a **new, previously unrecorded connection** at 2026-09-27T05:17:04Z
    (pid `19680`, still that same idle server, well before this probe's own window) from
    a client reporting `user-agent: codex-mcp-client/0.155.0-alpha.16.4` — a `GET -> 405`
    probe (line 58), **seven** rejected OAuth/OIDC discovery `GET`s (lines 59-65, in
    order: `/.well-known/oauth-protected-resource/mcp`, `/mcp/.well-known/oauth-
    protected-resource`, `/.well-known/oauth-protected-resource`, `/.well-known/oauth-
    authorization-server/mcp`, `/.well-known/openid-configuration/mcp`, `/mcp/.well-
    known/openid-configuration`, `/.well-known/oauth-authorization-server`), an
    `initialize` **sent** as
    `protocolVersion: "2025-06-18"` (line 66) and **answered** `protocolVersion:
    "2025-11-25"` (line 67, legacy negotiation, same as every other Codex client in this
    project), `notifications/initialized`, a `GET -> 405`, `tools/list`, and a
    `DELETE -> 405`. This
    is the same `0.155.0-alpha.16.4` user-agent G4's own "UNVERIFIED items" already
    record connecting to an idle server at 09:08:19Z and 15:57:32Z on 2026-09-26
    (attributed there to Codex Desktop, inferred from user-agent alone, cause
    UNVERIFIED) — see `docs/planning/v0.1/11-risks.md` row 43, updated with this
    recurrence below.
  - Lines 74-75: pid `26152`'s own startup at 06:41:33.798Z-.809Z (stdio and HTTP
    listeners coming up) — the same process this probe's evidence window uses.
  - Lines 76-83: an 8-request burst at 06:42:23.519Z-.534Z from `codex-mcp-client/
    0.157.1` (a `GET -> 405` plus 7 rejected OAuth/OIDC discovery probes, each carrying
    `mcp-protocol-version: 2024-11-05`) — strictly before this probe's own 06:42:35Z
    timebox declaration, so not part of this probe's own evidence window either.
- **What the wire shows, HTTP surface (pid `26152`):**
  - Two `server/discover` requests (lines 1, 3) and two `tools/list` requests (lines 5,
    7) — each pair from a **separate Codex MCP registration** pointed at the same URL:
    `[mcp_servers.g4]` (global, `url = "http://127.0.0.1:17448/mcp"`, in the
    **user-level** `~/.codex/config.toml`) and `[mcp_servers.g4http]` (project-scoped,
    same URL, in the **scratchpad project's own** `.codex/config.toml`) — confirmed by
    reading both config files directly. This is why `server/discover` and `tools/list`
    each appear twice: two independent Codex clients dialed the same HTTP endpoint.
    Every request carries header `mcp-protocol-version: 2026-07-28` and body
    `_meta["io.modelcontextprotocol/protocolVersion"]: "2026-07-28"`, `user-agent:
    codex-mcp-client/0.157.1`.
  - One `tools/call` (line 17, at 06:42:49.027Z) for `g4_echo("row41 modern probe")`,
    same headers/`_meta`, plus `_meta["x-codex-turn-metadata"]` carrying
    `codex_version: "0.157.1"` and `model: "gpt-6-luna"`. The response (line 18) returns
    `content[0].text: "g4 echo: row41 modern probe"` and OAC
    `_meta["io.github.rossgraeber/oac-session-channels"]` provenance
    (`served_by_pid: 26152, surface: "http-modern"`).
    *Dated correction, 2026-10-03 (#46): the line above under-reports
    `x-codex-turn-metadata`. On line 17 it also carries `session_id`, `thread_id` and
    `turn_id` (plus `reasoning_effort`, `thread_source`, `turn_trigger`, sandbox fields
    and `turn_started_at_unix_ms`). The legacy-era calls in
    `transcript-2026-09-26.jsonl` lines 48 and 50 carry the same fields. The verdict is
    unchanged. `spec/bindings/mcp.md` §4.4 records why OAC does not attribute calls by
    this undocumented, client-asserted field.*
  - **Every request on this leg negotiated the current MCP revision, `2026-07-28`, this
    time** — the first time any Codex client has done so against this server in either
    G4 run.
- **What the wire shows, stdio surface (pid `30452`, the project-scoped `.codex/
  config.toml`'s `[mcp_servers.g4spike]` registration, distinct from both the global
  `g4` and project `g4http` HTTP registrations above):** `initialize`
  (line 11) sent `protocolVersion: "2025-06-18"`, answered `protocolVersion:
  "2025-11-25"` (line 12) — **still legacy**, same negotiation Codex has used on every
  stdio connection in both prior G4 runs. This confirms the opt-in flag changed only
  the HTTP-registered clients' behavior in this run, not the separate stdio
  registration's.
- **Stdout.** `codex exec`'s own stdout printed only the model's final content —
  `{"content":[{"type":"text","text":"g4 echo: row41 modern probe"}], ...}`-shaped
  model output, not `_meta` — this is `codex exec`'s ordinary output shape for any tool
  call and is not itself evidence about whether `_meta` reaches the model; it is not
  restated here as a second data point for G4's Claude-side `_meta`-surfacing finding,
  since the two code paths (Claude Code's tool-result rendering vs. `codex exec`'s CLI
  output format) are not comparable mechanisms.
- **Disposition.** This does not change G4's `PASS` verdict — G4's own criterion 2 was
  already satisfied via Claude as the modern client. It closes the on-the-wire half of
  `docs/planning/v0.1/11-risks.md` row 41 for the opt-in, feature-flagged path only:
  Codex `0.157.1` **can** negotiate `2026-07-28` against an OAC-shaped server, but only
  behind `--enable mcp_2026_07_28`/`-c features.mcp_2026_07_28=true`, a flag documented
  as `stage: UnderDevelopment`, `default_enabled: false`. It does not close conflict-
  register row C5 (`docs/planning/v0.1/03-decisions-and-amendments.md` §3): C5's
  "Codex tool path may negotiate current MCP" element still requires the **default**
  client behavior to do this, or an `rmcp`-based server to be tested, neither of which
  this probe changes. See `docs/planning/v0.1/11-risks.md` row 41 and the C5 conflict-
  register row for the updated evidence note.
- **Fixture reproduction.** The fixture is the driver's own transcript file, copied
  verbatim: `Copy-Item scratchpad/g4-spike/transcript.jsonl
  docs/planning/gates/fixtures/g4-mcp-dual-era/transcript-row41-2026-09-27.jsonl` (the
  server logs only protocol JSON to this file — the same shape `redact.mjs` already
  produces byte-for-byte from a fresh run of the same probe steps above).
- **Fixture:** `docs/planning/gates/fixtures/g4-mcp-dual-era/
  transcript-row41-2026-09-27.jsonl` (18 lines). **Nothing in it needed redacting**: the
  server logs only protocol JSON (method names, headers, `_meta`), the same as the
  primary G4 fixture's own near-no-op redaction pass — re-scanned during this write-up
  anyway (`rossg`, `RossG`, the operator's hostname, `@gmail`, `sk-`, `Bearer `,
  `systemPrompt`, `prompt_snapshot`, with the public
  `io.github.rossgraeber/oac-session-channels` identifier masked first and restored) —
  zero residual hits, confirming there was nothing to redact rather than that a
  redaction pass found and removed something.

#### Scripted re-run through herdr (K8) — pointer only, not verdict-bearing

A herdr-driven re-run of this gate's 2026-09-26 re-run exists as test tooling (Epic K, K8
issue #131): `tools/herdr/scenarios/g4-mcp-dual-era.mjs`, compared against
`fixtures/g4-mcp-dual-era/transcript-2026-09-26.jsonl` by `tools/herdr/lib/g4-report.mjs`.
It runs `tools/herdr/gate-servers/g4-server.mjs`, a **reconstruction** of this gate's spike
server built from the architecture described above and the committed fixture, because the
original `g4-server.mjs` was never committed; it is not the server that produced this gate's
fixtures and cannot be verified identical to it. Codex's MCP registration there is per
invocation (the operator's global Codex config is never edited); because this run's own
global `g4` entry for `127.0.0.1:17448` may still be present, the scenario refuses this run's
ports and its report requires exactly one Codex HTTP session before attributing any Codex
traffic to the per-invocation registration. It has **never run live**:
it is exercised only against test doubles (`node tools/herdr/run.mjs --self-test`), so no
`-herdr` fixture and no `docs/planning/gates/herdr-runs/G4-<date>.md` record exist. When one
does, it is linked here and changes nothing above: this gate's verdict comes only from the
human-run procedure (`oac-gates` `references/scripted-runs.md` "Verdict eligibility").

---

#### Superseded: 2026-09-25/26 out-of-box run (NOT RUN)

- **Verdict:** NOT RUN — timebox expired before the confirming evidence was gathered.
- **Why.** The 120-minute box declared 2026-09-25T07:20:35Z closed at 09:20:35Z the same
  day with zero live-Claude evidence gathered (only a local self-test, explicitly not
  gate evidence, plus a real but partial Codex `0.154.0`/`0.157.0` HTTP-legacy-only
  check, both inside the box). All of the confirming evidence — Claude negotiating both
  eras, the concurrency check, the negative case, the Codex relay — was captured
  roughly 21-23 hours later, on 2026-09-26, after the box had already expired, with no
  new timebox declared before that work resumed. Per `.claude/skills/oac-gates/SKILL.md`:
  "An expired timebox is a result, not a licence to keep going or to guess... never a
  plain `PASS` on an incomplete run." All five criteria were individually confirmed by
  that out-of-box evidence and none failed — this was a process finding, not a technical
  one, and the fresh re-run above bears that out.
- **Fixture:** `docs/planning/gates/fixtures/g4-mcp-dual-era/transcript-2026-09-26-outofbox.jsonl`
  (58 lines, pid 16712 for both eras; kept per "Fixtures captured" above). The
  transcript-mislabel correction is under "Fixtures captured" above
  (`transcript-run1.jsonl`'s "Label correction" note). Full line-by-line detail and UI
  quotes for this run's own evidence live in the Claude Code session log for this branch
  (not committed) rather than restated a second time here; nothing here depends on git
  history, since no revision of this file has been committed yet.
- **Pinned version(s) at that run:** Claude Code `v2.1.283`; `@openai/codex` `0.157.1`;
  MCP current `2026-07-28` / legacy `2025-11-25`; `rmcp` not exercised; Windows only.
- **Date:** timebox declared 2026-09-25; evidence captured 2026-09-26.
- **Fallback taken:** none — not needed on the merits, and not exercised (the primary
  path was still being tested when the box expired).
- **UNVERIFIED items surfaced (carried forward, still open):** Codex's modern-era leg
  untested (same item the current PASS also leaves open); the docs discrepancy about
  Claude's v2 runtime probing stdio `server/discover` with `MCP_PROTOCOL_NEGOTIATION`
  unset (reproduced again in the current run, still open); the Codex-Desktop-threads-show-
  Claude-prompts security observation (still open, both in `docs/planning/STATUS.md`
  and `docs/planning/v0.1/11-risks.md`).
