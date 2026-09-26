### G2 codex-inject

> **INVALIDATED (2026-09-26).** The PASS below was recorded on Codex `0.154.0`. The
> environment is now on `0.157.1`, and the Codex row in `docs/planning/PINS.md` is now
> **floating** by operator decision (see its "Floating-version policy"). G2 is `NOT RUN`
> for the current environment until it is re-run on the observed version. The record below
> is kept unchanged as the `0.154.0` result.

- **Gate id:** G2
- **Pinned version(s):** `@openai/codex` `0.154.0`. The CLI, the managed daemon binary and
  the running app-server all reported `0.154.0` (`codex app-server daemon version`:
  `"cliVersion":"0.154.0","appServerVersion":"0.154.0","managedCodexVersion":"0.154.0"`).
  This matches `docs/planning/PINS.md` exactly; no drift.
  - **Scope of this run:** Windows 11 only (`platformFamily: "windows"`), default
    `CODEX_HOME` (`<USER_HOME>\.codex`), non-elevated terminal.
  - **Not exercised:** macOS and Linux.
- **Date:** 2026-09-25
- **Timebox:** 60 minutes, **declared late** — this is a process deviation.
  - **Before the box:** live work began at 06:26:15 UTC, when `codex app-server daemon
    start` ran (daemon pid/lock file mtimes). Two probe rounds also ran before the box was
    declared:
    - a silent bare-JSONL attempt through the proxy, uncommitted
    - fixture line 1 at 06:31:32
  - **Box:** declared at 06:32 UTC. The last live exchange was at 06:40:20 UTC.
  - **Total:** about 14 minutes of live work (06:26:15 to 06:40:20) — well inside 60
    minutes had the box been set at the start. It was not expired.
  - **Also before:** about 25 minutes of documentation and source research preceded any
    live work.
- **Command transcript summary:**
  - **Daemon:** `codex app-server daemon start` returned the control socket path
    `<USER_HOME>\.codex\app-server-control\app-server-control.sock`. The path is 64
    bytes, under the 108-byte Windows AF_UNIX limit that the daemon README names.
  - **TUI:** the human operator ran plain `codex` (no `-c`, no `--remote`, no flags) in a
    real non-elevated terminal. They sent one message: "Greet me in one word."
  - **Second client:** a throwaway client
    (`docs/planning/gates/fixtures/g2-codex-inject/client.mjs.throwaway-quarantined`)
    connected to the same daemon through `codex app-server proxy`. It is a raw
    stdio-to-UDS byte relay (`codex_stdio_to_uds::run` in `codex-rs/cli/src/main.rs`).
    The client performs the HTTP/1.1 WebSocket Upgrade itself.
  - Seven separate client connections were made. The table below lists them all; line
    numbers refer to the committed, redacted transcript
    `docs/planning/gates/fixtures/g2-codex-inject/transcript.jsonl`.

  | Lines | UTC | Client run | What it did |
  |---|---|---|---|
  | — | 06:26:15 | (CLI) | `codex app-server daemon start` |
  | — | ~06:29 | bare JSONL | First client attempt: sent JSONL through the byte-relay proxy with no WebSocket upgrade, and got no reply. Wrong turn #1, not committed. |
  | 1-9 | 06:31:32 | `list` | Upgrade, `initialize`/`initialized`, `thread/loaded/list` (empty), `thread/list`. This was before the operator launched the TUI. |
  | 10-18 | 06:34:03 | `list` | Run after the TUI launch. `thread/loaded/list` returned two loaded threads (see criterion 1, and the unidentified second thread). |
  | 19-25 | 06:34:32 | `turn` | **Injection 1:** `turn/start` into the idle TUI thread with "…reply with exactly the words OAC G2 RECEIVED." |
  | 26-38 | 06:35:31 | `busyqueue` | **Injection 2:** the spike itself started a long turn with `turn/start` (a 40-item lighthouse list); it was not a pre-existing turn. **Injection 3:** 3 seconds later, `thread/queue/add` with "…reply with exactly the words OAC G2 QUEUED." The client then waited for `turn/completed` events that could never arrive, because it had never called `thread/resume` (wrong turn #2, below). |
  | 39-45 | 06:39:35 | `turns` | `thread/turns/list`: the daemon's own record of all turns. |
  | 46-53, 61-75 | 06:40:12 | `watch` | `thread/resume` (event subscription), then the full subscribed event stream for injection 4. |
  | 54-60 | 06:40:18 | `turn` | **Injection 4:** `turn/start` with "…reply with exactly the words OAC G2 EVENTS.", sent to capture event fixtures. |

  Four messages the operator did not type were injected into the operator's real thread
  (injections 1-4).
- **Pass criteria evaluated:**
  - [x] **A TUI launched normally (no config overrides) attaches to a running
        `codex app-server daemon`.**
        - **Evidence:** after the operator launched plain `codex`, `thread/loaded/list`
          on the daemon returned the TUI's thread `01a0d743-f182-7120-b2b6-9238a6d06e06`
          (fixture line 16). Before the launch the same call returned an empty list
          (line 7). The thread's cwd and preview ("Greet me in one word.") identify it
          as the operator's thread.
        - **Why this proves attach:** a TUI that embeds its own server would not have
          its thread loaded in the daemon process.
        - **Corroboration:** the TUI showed turns that a different process delivered
          through the daemon (criterion 3). Openai/codex #21743 says a TUI with a
          separate embedded server would not show them.
        - **Scope:** Windows, default `CODEX_HOME`, non-elevated terminal.
  - [x] **A second client of the same daemon delivers a message with `thread/queue/add` or
        `turn/start`.** Both were exercised.
        - `turn/start` into the idle thread: accepted; turn
          `01a0d745-d414-71c1-a9cc-9f2c975bf23c` became `inProgress` (lines 24-25).
        - `thread/queue/add` into a busy thread:
          - The lighthouse turn `01a0d746-b85a-…` started at `startedAt` 1790318131.
            Lines 32-33 show it `inProgress`.
          - At 06:35:34, three seconds later, `thread/queue/add` was accepted with a
            `queuedSubmission` id (lines 34-35).
          - The daemon's turn record (line 45) shows the queued turn `01a0d746-ea41-…`
            with `startedAt` 1790318144. That is the same second as the lighthouse turn's
            `completedAt` (1790318144).
          - Lines 36-37 show the thread going idle and then active again 32 ms later.
          - So the message was queued until idle, then delivered in order.
        - `turn/steer` was **deliberately not exercised**. It writes into an in-flight
          turn, which makes it a code-execution-adjacent authorization decision
          (`oac-security-work`). G2 does not need it.
  - [x] **The TUI user sees the delivered message and the model answers it.** The
        operator's pasted TUI output, quoted verbatim:
        ```
        › G2 spike test (from a second client of the local daemon, not typed in
          this TUI): reply with exactly the words OAC G2 RECEIVED.

        • OAC G2 RECEIVED
        ```
        A second paste showed the injected lighthouse turn "› G2 spike busy-turn test: …"
        with its full 40-item answer. It was followed, after item 40 and not interleaved,
        by:
        ```
        › G2 spike queued message (sent via thread/queue/add while the previous
          turn was still running): after finishing, reply with exactly the
          words OAC G2 QUEUED.
        ```
        That paste ended before the queued turn's reply. The daemon's turn record
        (line 45) shows that reply as `agentMessage: "OAC G2 QUEUED"`. The operator was not
        asked to confirm injection 4 (EVENTS) in the TUI. Its completion is shown only
        by the daemon event stream (lines 62-75).
  - [x] **OAC holds no OpenAI credentials at any point.**
        - **Verified:** the spike client contains no credential handling. Its only
          mention of auth is a comment saying it never reads `auth.json`. It never opens
          `CODEX_HOME`, and the client passed no credential of any kind to the daemon.
          The committed transcript contains no token, bearer value or API key, and a
          raw-transcript scan before redaction found none.
        - **Inferred:** the daemon served each model turn from its own configured
          auth, the operator's saved CLI login. Nothing in the protocol exchange shows
          the auth source directly.
- **Verdict:** NOT RUN for the current environment (invalidated 2026-09-26; see the callout
  at the top). The verdict recorded for Codex `0.154.0` was PASS, and it is kept in the
  re-run history.
- **Fallback taken:** none — the primary path (implicit daemon attach) passed. The
  `codex --remote ws://…` fallback was not exercised. That is correct: the gate requires
  the fallback only if the primary path fails.
- **Sources and surface labels** (all retrieved 2026-09-25, `@openai/codex` `0.154.0`,
  commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`):
  - **First-party app-server docs:** https://learn.chatgpt.com/docs/app-server, no version
    stated on the page. It documents the stdio JSONL transport, the WebSocket transport,
    and the Unix-socket transport ("WebSocket connections over Codex's default app-server
    control socket … using the standard HTTP Upgrade handshake"). It documents
    `thread/start`, `thread/resume`, `thread/list`, `thread/loaded/list`, `turn/start`,
    `turn/steer` and the event names. It does **not** document the daemon, `codex queue`,
    `app-server proxy` or `thread/queue/add`.
  - **Labels per surface:**

    | Surface | Label | Basis |
    |---|---|---|
    | app-server protocol | experimental | CLI help: "[experimental] Run the app server" |
    | Control-socket WebSocket transport | supported | first-party docs above |
    | Daemon, `app-server proxy`, `codex queue` | experimental | CLI `--help` and `codex-rs/app-server-daemon/README.md` only; the README says "`codex-app-server-daemon` is experimental" |
    | `thread/queue/add` | experimental | absent from the default checked-in schema `codex-rs/app-server-protocol/schema/json`; present only in `codex app-server generate-json-schema --experimental`; requires `capabilities.experimentalApi` |
- **UNVERIFIED items:**
  - **Closed:** implicit daemon attach runs at runtime in released `0.154.0` — **on
    Windows, with default `CODEX_HOME`, from a non-elevated terminal.** B2 had confirmed only
    that the code path is present in source. macOS and Linux were not exercised.
  - **Closed:** `codex mcp-server` is absent at `0.154.0`. It is not a subcommand, and
    `codex mcp-server --help` falls through to top-level help. Its deprecation and
    deletion dates remain unverified.
  - **Recorded, source-level:** how the control socket is protected on Windows, at the
    pinned commit.
    - `set_control_socket_permissions` does nothing on Windows (`#[cfg(not(unix))] … Ok(())`,
      `codex-rs/app-server-transport/src/transport/unix_socket.rs` lines 241-244). Its
      chmod applies only on Unix.
    - The Windows protection is the socket **directory's DACL**.
      `codex_uds::validate_private_socket_path` (`codex-rs/uds/src/windows_socket_validation.rs`)
      accepts only a directory owned by the current user's SID. The DACL must be
      protected and inheritable and grant only that user ("Accept exactly the protected,
      inheritable user-only ACL set at daemon bind").
    - `codex-rs/uds/src/windows_peer.rs` also defines `ensure_non_elevated_peer`
      ("implicit daemon connection requires non-elevated current-user tokens"). It
      compares the peer's SID and elevation. Its call sites were not traced at the pin.
      The review located them on the client side (TUI, `app-server-client`) on the
      current tree.
    - No server-side per-connection peer check was found.
    - Net effect: any non-elevated process of the same OS user that can open the socket
      is a full client.
  - **Still open:** cross-process resume does not attach (openai/codex #21743). Not
    re-tested, because the test appends silently to the operator's real thread history.
  - **Still open, inconclusive:** whether Codex Desktop exposes the control socket. The
    evidence is in the uncommitted raw transcript; redaction removed it from fixture
    lines 9 and 18. There, the daemon's `thread/list` returned Desktop-originated
    sessions (`originator: "Codex Desktop"`) as `notLoaded` saved history. That shows
    shared on-disk history, not live socket exposure.
  - **New, UNVERIFIED — unidentified second loaded thread.** Fixture line 16 lists two
    loaded threads.
    - The second, `01a0d744-b34a-7c92-9011-20d95fe5f98a`, appears **nowhere else** in the
      raw transcript.
    - It is absent from both `thread/list` responses, so redaction did not remove it.
      The line-18 response held 5 entries: the spike thread plus 4 unrelated Desktop
      sessions.
    - Its UUIDv7 prefix places its creation milliseconds after the operator's first
      message in the TUI. It is probably a TUI-spawned, unlisted side thread. Its
      purpose is not identified.
  - **New, observed:** `thread/queue/add` requires `threadId`, `clientUserMessageId` and
    `input` (generated experimental schema). The TUI's own `codex queue` command issues
    it (`codex-rs/tui/src/session_queue_commands.rs`, `THREAD_QUEUE_ADD_METHOD =
    "thread/queue/add"`).
  - **New, observed:** a client that has not called `thread/resume` receives only
    `thread/status/changed` for a thread, with no `turn/*` or `item/*` events (lines
    33-38). To observe a reply, a delivering client must subscribe with `thread/resume`
    (lines 51-75).
  - **New, UNVERIFIED — mechanism inferred from one fresh-daemon observation.** The TUI's
    thread carries `originator: "oac_g2_spike"`. That is the `clientInfo.name` of the
    first client to initialize the fresh daemon: this spike's probe, not the TUI.
    - The thread also carries `"source": "vscode"` (lines 18, 52), although a terminal
      TUI created it.
    - Conclusion: in a shared daemon, neither field identifies the creating client.
      Do not use either one for provenance.
  - **New, security-relevant:** any control-socket client can read every saved session's
    prompt preview and rollout path through `thread/list`. The evidence is in the raw
    transcript, redacted out of fixture lines 9 and 18.
    - Subscribed clients also receive `account/rateLimits/updated`, which carries plan
      type, credits and usage.
    - Both stay within the same non-elevated OS user.
    - Carry both into the local-IPC and cross-project-leakage rows of
      `docs/planning/v0.1/06-security.md` when that file is next revised.
  - **New, operator-observed:** launching the TUI on Windows opened several extra
    console windows. The cause was not investigated.
- **Fixtures captured:** `docs/planning/gates/fixtures/g2-codex-inject/transcript.jsonl`,
  75 lines.
  - **Covers:** the WebSocket upgrade over the control socket,
    `initialize`/`initialized`, `thread/loaded/list`, `thread/list`, `turn/start`,
    `thread/queue/add`, `thread/turns/list` and `thread/resume`. It also covers the
    subscribed event stream: `turn/started`, `item/started`, `item/completed`,
    `item/agentMessage/delta`, `turn/completed`, `thread/status/changed` and
    `thread/tokenUsage/updated`.
  - **Gap against D6's Codex list: `thread/start` was not captured.** The operator's
    TUI created the thread on its own connection. D6 should capture `thread/start`
    directly.
  - **Redaction** was done by `redact.mjs`, which is kept with the uncommitted spike
    scratch:
    - username and home paths replaced with `<USER_HOME>` and `<SPIKE_DIR>`
    - the hostname replaced with `<HOST>`
    - the installation id replaced with `<INSTALLATION_ID>`
    - account plan and credit fields replaced with `<REDACTED>`
    - 9 unrelated sessions removed from the `thread/list` responses: 5 at line 9 and 4
      at line 18. They carried private prompt previews and rollout paths.
  - **Residual check:** a post-redaction scan found 0 matches.
  - **Deliberately kept (not secrets):** a rollout filename timestamp that reveals the
    local UTC offset, and `usedPercent`/`resetsAt` usage values (line 73).
  - Take message shapes from the pinned schema, not from this fixture.
  - The location is provisional; D6 owns the final convention.
- **Pin rows relied on:** `Codex CLI / app-server`
- **PINS.md as-of:** 2026-09-16, commit `a44a7ed7278caab383f264f5bc8e12bb5c8c73e9`
- **Process notes (for future gate spikes, not part of the pass/fail record):**
  - Unlike G1, documentation and source came first. The first-party app-server docs were
    fetched before any code. Where they fall short (the daemon, `codex queue`,
    `thread/queue/add`), the facts came from the CLI's own `--help`, the daemon README
    and the source at the pinned commit.
  - **Wrong turn #1:** the first client sent bare JSONL through `codex app-server proxy`
    and got no reply. The CLI help already said the proxy relays "stdio bytes", and the
    docs already said the socket speaks WebSocket. Read together, those two facts meant a
    raw byte relay to a WebSocket endpoint.
  - **Wrong turn #2:** the `busyqueue` run waited up to 150 s for `turn/completed` events
    it could never receive, because it had not subscribed with `thread/resume`
    (06:35:45 to about 06:39). Working out why produced the "only `thread/resume`
    subscribers get `turn/*`/`item/*` events" finding.
  - Neither wrong turn needed the operator.
  - The timebox was declared about six minutes after live work began (see "Timebox").
    Set it before `daemon start` next time.
  - `PLANNING-PROMPT.md` §3.2 and the `oac-codex-appserver` skill named
    `thread/queue/add` without noting that it is absent from the default schema. They
    also did not say that the control socket is WebSocket over UDS. The skill is updated
    in the same change as this result.
- **Re-run history:**

  | Date | Pinned versions | Verdict | Invalidated by |
  |---|---|---|---|
  | 2026-09-25 | Codex CLI / app-server `0.154.0` (matches PINS.md); Windows only | PASS | Codex pin `0.154.0` -> floating (last observed `0.157.1`), 2026-09-26 |
