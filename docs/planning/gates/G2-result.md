### G2 codex-inject

- **Gate id:** G2
- **Pinned version(s):** `@openai/codex` `0.154.0` — CLI, managed daemon binary, and
  running app-server all reported `0.154.0` (`codex app-server daemon version`:
  `"cliVersion":"0.154.0","appServerVersion":"0.154.0","managedCodexVersion":"0.154.0"`).
  Matches `docs/planning/PINS.md` exactly; no drift. Platform: Windows 11
  (`platformFamily: "windows"`), `CODEX_HOME` default (`<USER_HOME>\.codex`).
- **Date:** 2026-09-25
- **Timebox:** 60 minutes, set before the live part of the spike began (06:32 UTC).
  Live work finished by 06:40 UTC — about 8 minutes elapsed, box not expired. Roughly 25
  minutes of documentation and source research came before the box started (see "Process
  notes").
- **Command transcript summary:** `codex app-server daemon start` started the shared
  daemon. It returned the control socket path
  `<USER_HOME>\.codex\app-server-control\app-server-control.sock`, which is 64 bytes and
  under the 108-byte Windows AF_UNIX limit the daemon README names. The human operator ran
  a plain `codex` (no `-c`, no `--remote`, no flags) in a real non-elevated terminal and
  sent one message ("Greet me in one word."). A throwaway second client
  (`docs/planning/gates/fixtures/g2-codex-inject/client.mjs.throwaway-quarantined`)
  connected to the same daemon through `codex app-server proxy`, which is a raw stdio
  byte relay to the control socket. The client performed the HTTP/1.1 WebSocket Upgrade
  itself, because the control socket speaks WebSocket over UDS (first-party docs plus
  source). It then ran `initialize` with `capabilities.experimentalApi: true`,
  `initialized`, `thread/loaded/list`, `turn/start` (idle thread), `thread/queue/add`
  (busy thread), `thread/turns/list`, and `thread/resume` (event subscription). The redacted
  raw transcript is `docs/planning/gates/fixtures/g2-codex-inject/transcript.jsonl`.
- **Pass criteria evaluated:**
  - [x] A TUI launched normally (no config overrides) attaches to a running
        `codex app-server daemon`. Evidence: after the operator launched plain `codex`,
        `thread/loaded/list` on the daemon returned the TUI's thread
        `01a0d743-f182-7120-b2b6-9238a6d06e06` as loaded (fixture line 16). Before the
        launch, the same call returned an empty list (line 7). The thread's cwd and
        preview ("Greet me in one word.") identify it as the operator's TUI thread. A TUI
        that embeds its own server would not have its thread loaded in the daemon
        process. This closes the go/no-go UNVERIFIED item: **implicit daemon attach is
        active in released `0.154.0`**, not only on `main`, at least on Windows with a
        default `CODEX_HOME`.
  - [x] A second client of the same daemon delivers a message with `thread/queue/add` or
        `turn/start`. Evidence: both were exercised.
        - `turn/start` into the idle thread was accepted, and turn
          `01a0d745-d414-71c1-a9cc-9f2c975bf23c` went to `inProgress` (lines 24-25).
        - `thread/queue/add` was sent 3 seconds into a running turn
          (`01a0d746-b85a-…`, started at line 31). It was accepted with a
          `queuedSubmission` id (lines 34-35).
        - The daemon's own turn record shows the queued turn
          `01a0d746-ea41-…` starting at the same second the running turn completed
          (`startedAt` 1790318144 = previous `completedAt` 1790318144). So the message
          was queued until idle, then delivered in order (line 45,
          `thread/turns/list`).
        - `turn/steer` was deliberately not exercised. It writes into an in-flight turn
          and is a code-execution-adjacent authorization decision (`oac-security-work`).
          G2 does not need it.
  - [x] The TUI user sees the delivered message and the model answers it. The operator
        pasted the TUI output. The injected text appeared as a user turn the operator did
        not type, and the model answered "OAC G2 RECEIVED". The queued message appeared
        after the 40-item lighthouse list finished, not interleaved with it. The daemon's
        turn record shows the queued message was answered "OAC G2 QUEUED" (line 45).
  - [x] OAC holds no OpenAI credentials at any point. The spike client contains no
        credential handling: its only reference to auth is a comment stating it never
        reads `auth.json`. It never opens `CODEX_HOME`. The daemon served every model turn
        from the operator's saved CLI login. The redacted transcript contains no token,
        bearer value, or API key; a raw-transcript scan before redaction found none.
- **Verdict:** PASS
- **Fallback taken:** none — not needed. The primary path (implicit daemon attach) passed.
  The `codex --remote ws://…` fallback was not exercised, which is correct: the gate
  requires the fallback only if the primary path fails.
- **UNVERIFIED items:**
  - **Closed:** whether implicit daemon attach runs by default in released `0.154.0`.
    The runtime answer is yes (criterion 1 evidence above). B2 had confirmed only that
    the code path is present in source.
  - **Closed:** `codex mcp-server` is absent at `0.154.0`. It is not a subcommand;
    `codex mcp-server --help` falls through to top-level help. Do not plan on it.
  - **Recorded, source-level:** control-socket peer validation is filesystem-level. The
    socket is bound inside a private directory (`codex_uds::validate_private_socket_path`)
    with restricted permissions (`set_control_socket_permissions`), in
    `codex-rs/app-server-transport/src/transport/unix_socket.rs` at the pinned commit
    `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`. No per-connection peer-credential check
    was found. Any same-user process that can open the socket is a full client.
  - **Still open:** cross-process resume does not attach (openai/codex #21743). Not
    re-tested. The test would append silently to the operator's real thread history, and
    that risk was not worth taking inside a gate spike.
  - **Still open, inconclusive:** whether Codex Desktop exposes the control socket.
    `thread/list` on the daemon returned Desktop-originated sessions
    (`originator: "Codex Desktop"`) as `notLoaded` saved history. That shows shared
    on-disk history, not live socket exposure.
  - **New, observed:** `thread/queue/add` is experimental. It is absent from the checked-in
    default schema (`codex-rs/app-server-protocol/schema/json`) and present only in
    `codex app-server generate-json-schema --experimental`. Required params:
    `threadId`, `clientUserMessageId`, `input`. The TUI's own `codex queue` command
    issues this method (`codex-rs/tui/src/session_queue_commands.rs`, same commit).
  - **New, observed:** a client that has not called `thread/resume` receives only
    `thread/status/changed` for a thread. It receives none of the `turn/*` or `item/*`
    events. To observe a reply, a delivering client must subscribe with `thread/resume`
    (fixture lines 51-75).
  - **New, observed; mechanism inferred:** the TUI's thread carries
    `originator: "oac_g2_spike"`, the `clientInfo.name` of the first client to initialize
    the fresh daemon. That client was this spike's probe, not the TUI. In a shared
    daemon, `originator` does not identify which client created a thread. Do not use it
    for provenance.
  - **New, security-relevant:** any client of the control socket can read every saved
    session's prompt preview and rollout path (`thread/list`). The daemon also broadcasts
    account metadata (`account/rateLimits/updated`: plan type, credits) to subscribed
    clients. Both stay within the same OS user. Carry both into the local-IPC and
    cross-project-leakage rows of `docs/planning/v0.1/06-security.md` when that file is
    next revised.
  - **New, operator-observed:** launching the TUI on Windows opened several additional
    console windows. Cause not investigated.
- **Fixtures captured:** `docs/planning/gates/fixtures/g2-codex-inject/transcript.jsonl`
  (75 lines). It covers the WebSocket upgrade over the control socket,
  `initialize`/`initialized`, `thread/loaded/list`, `thread/list`, `turn/start`,
  `thread/queue/add`, `thread/turns/list`, `thread/resume`, and the subscribed event
  stream (`turn/started`, `item/started`, `item/completed`, `item/agentMessage/delta`,
  `turn/completed`, `thread/status/changed`, `thread/tokenUsage/updated`). **Gap against
  D6's Codex list: `thread/start` was not captured.** The operator's TUI created the
  thread on its own connection. D6 should capture `thread/start` directly. The fixture
  was redacted by `redact.mjs` (kept with the spike scratch, not committed) as follows:
  username and home paths become `<USER_HOME>`/`<SPIKE_DIR>`, the hostname becomes
  `<HOST>`, the installation id becomes `<INSTALLATION_ID>`, account plan and credit fields
  become `<REDACTED>`, and 9 unrelated sessions (private prompt previews and rollout paths)
  were removed from the `thread/list` responses. A post-redaction scan found 0 residual
  matches. Message shapes should be taken from the pinned schema, not from this fixture.
  Location is provisional; D6 owns the final convention.
- **Pin rows relied on:** `Codex CLI / app-server`
- **PINS.md as-of:** 2026-09-16, commit `a44a7ed7278caab383f264f5bc8e12bb5c8c73e9`
- **Process notes (for future gate spikes, not part of the pass/fail record):**
  - Unlike G1, documentation and source came first. The first-party app-server docs were
    fetched before any code. They document the WebSocket-over-UDS control socket but not
    the daemon, `codex queue`, or `thread/queue/add`. Those were confirmed from the CLI's
    own `--help`, the daemon README, and the source at the pinned commit.
  - One wrong turn remains. The first client sent bare JSONL through
    `codex app-server proxy` and got no reply. The CLI help already said the proxy relays
    "stdio bytes", and the docs already said the socket speaks WebSocket. That combination
    should have been read as "raw byte relay to a WebSocket endpoint" before writing the
    first client. It cost one round, found without operator involvement.
  - `PLANNING-PROMPT.md` §3.2 and the `oac-codex-appserver` skill name `thread/queue/add`
    without noting that it is omitted from the default schema. They also do not say that
    the control socket is WebSocket-over-UDS rather than JSONL. Both facts should reach
    that skill.
- **Re-run history:**

  | Date | Pinned versions | Verdict | Invalidated by |
  |---|---|---|---|
  | 2026-09-25 | Codex CLI / app-server `0.154.0` (matches PINS.md) | PASS | — |
