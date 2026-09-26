### G2 codex-inject

- **Gate id:** G2
- **Pinned version(s):** `@openai/codex` `0.157.1`, commit
  `36650394c5b38c2990ccf2a3457165ca3e9d9726` (Codex CLI / app-server row is **floating**;
  this is the version last observed in `docs/planning/PINS.md`). Reported by all three
  sources `docs/planning/PINS.md`'s "Floating-version policy" names ("as reported by the
  CLI, the daemon (`codex app-server daemon version`), and the client's `clientInfo`"):
  - **In-run, strongest evidence:** during the spike itself, at 2026-09-26T06:24:16.590Z
    (Claude Code session log, tool-result timestamp), `codex --version` and `codex
    app-server daemon version` were run back-to-back against the live daemon and printed
    `codex-cli 0.157.1` and `{"status":"running",...,"managedCodexVersion":"0.157.1",
    "cliVersion":"0.157.1","appServerVersion":"0.157.1"}` — captured while the daemon
    that served this run's turns was the one being queried, not a separate later check.
  - **CLI, re-confirmed post-run:** `codex --version` → `codex-cli 0.157.1` (subagent
    log `subagents/agent-a8d414ff5a8e8cd65.jsonl`, not committed, tool-result timestamp
    2026-09-26T06:58:54Z).
  - **Daemon, re-confirmed post-run:** `codex app-server daemon version` →
    `"cliVersion":"0.157.1","appServerVersion":"0.157.1","managedCodexVersion":"0.157.1"`
    (subagent log `subagents/agent-a8d414ff5a8e8cd65.jsonl`, not committed, line 332,
    captured at 2026-09-26T06:59:29Z).
  - **Client-visible version string:** the daemon's `initialize` result `userAgent`
    reported `codex-tui/0.157.1 (Windows 10.0.26200; x86_64) unknown (oac_g2_spike;
    0.0.1)` (fixture `transcript-2026-09-26-0.157.1.jsonl` line 3) — this is the
    daemon's own self-description on the wire, not a `clientInfo` field the spike sent
    itself, but it names the same `0.157.1` version.
  - Every reading matches the pin exactly; no drift.
  - **Scope of this run:** Windows 11 only (`platformFamily: "windows"`), default
    `CODEX_HOME` (`<USER_HOME>\.codex`), non-elevated terminal — same scope as the
    superseded `0.154.0` run at the end of this file.
  - **Not exercised:** macOS and Linux (unchanged from the `0.154.0` run).
  - Prior pinned version for this gate: `@openai/codex` `0.154.0`, commit
    `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` (see "Re-run history" and the retained
    `0.154.0` transcript summary at the end of this file, which this re-run does not
    repeat).
- **Date:** 2026-09-26
- **Timebox:** 45 minutes of live work, declared before live work began this time.
  Source: Claude Code session log (not committed), tool output at 2026-09-26T06:20:38Z,
  exact wording: "G2 re-run timebox declared 2026-09-26T06:20:37Z (45 min live work)".
  - **Box:** declared 2026-09-26T06:20:37Z.
  - **Live work:** the operator's TUI thread itself was created at 06:22:33Z
    (`thread/list` `createdAt` `1790403753`, fixture line 10 — matches the rollout
    filename `rollout-2026-09-26T01-22-33-...`, local-time-named), 2 minutes into the
    box. The spike's own first client connection follows at 06:24:16.001Z (WebSocket
    handshake, fixture line 1); its last frame is 06:27:18.565Z (`thread/turns/list`
    result, line 40). About 5 minutes of live work end-to-end (06:22:33Z-06:27:18Z),
    well inside the 45-minute box. Not expired.
- **Command transcript summary (0.157.1 re-run):** a real Codex `0.157.1` daemon plus a
  normally launched TUI. Thread `01a0dc61-364b-7ed3-b686-b04e07eecaf2` was the operator's
  live TUI thread — identified by its `thread/list` entry (fixture line 10: preview
  "Greet me in a single word.", `cwd` matching the spike's own working directory,
  `cliVersion":"0.157.1"`) and by being the sole entry `thread/loaded/list` returned
  (line 7). This re-run is a narrower repeat of the two live-inject paths the `0.154.0`
  run at the end of this file already proved, not a full re-spike: it does not repeat
  the daemon-start/proxy/event-subscription exploration, since those facts were
  independently re-verified at the source level in `docs/planning/REVERIFICATION-B2.md`
  §"§3.2 re-verification at Codex `0.157.1` (floating-pin trigger, 2026-09-26)". Four
  client connections, all in fixture `transcript-2026-09-26-0.157.1.jsonl`:

  | Lines | UTC | Client run | What it did |
  |---|---|---|---|
  | 1-10 | 06:24:16 | `list` | Upgrade, `initialize`/`initialized`, `thread/loaded/list` (returned exactly one loaded thread — the operator's TUI thread, line 7), `thread/list` (full thread object, line 10). |
  | 11-18 | 06:24:30 | `turn` | **Injection 1:** `turn/start` into the idle TUI thread, turn `01a0dc63-00d3-78a3-bdbd-2f0666ca8686`, with "…reply with exactly the words OAC G2 RERUN RECEIVED." |
  | 19-32 | 06:24:39 | `busyqueue` | **Injection 2:** `turn/start` began a long turn, `01a0dc63-229a-79a3-aeae-6eec74b158ca` (a 40-item lighthouse list; not a pre-existing turn). **Injection 3:** 3 seconds later, `thread/queue/add` (line 28) queued submission `01a0dc63-2e68-7633-95c9-46ba70eaeec3` with "…reply with exactly the words OAC G2 QUEUED." |
  | 33-40 | 06:27:18 | `turns` | `thread/turns/list`: the daemon's own record, confirming all three turns `completed` in order — the queued turn (`01a0dc63-5d19-7ad3-a6c3-1d370e082f10`, `agentMessage: "OAC G2 QUEUED"`) started the same second the lighthouse turn completed (`startedAt`/`completedAt` both timestamp `1790403894`-range). |

  The operator's own TUI paste, quoted verbatim (session log timestamp
  2026-09-26T06:29:14.373Z; no path, username, or other private text appears in it, so
  nothing is redacted here):

  > G2 re-run on 0.157.1 (from a second daemon client, not typed in this TUI): reply
  > with exactly the words OAC G2 RERUN RECEIVED.
  >
  > OAC G2 RERUN RECEIVED
  >   1:24 AM
  >
  > G2 spike busy-turn test: write a numbered list of 40 distinct one-sentence facts
  > about lighthouses. Do not use any tools.
  >
  > [the model's 40-item numbered list, rendered in full, matching the daemon's own
  > turn record for `01a0dc63-229a-79a3-aeae-6eec74b158ca` verbatim]
  >   1:24 AM
  >
  > G2 spike queued message (sent via thread/queue/add while the previous turn was
  > still running): after finishing, reply with exactly the words OAC G2 QUEUED.
  >
  > OAC G2 QUEUED

  This confirms all three injected turns rendered in the TUI, in order, matching the
  daemon's own turn record above.
- **Pass criteria evaluated (0.157.1 re-run):**
  - [x] **A TUI launched normally (no config overrides) attaches to a running
        `codex app-server daemon`.**
        - **Evidence:** `thread/loaded/list` returned exactly one loaded thread,
          `01a0dc61-364b-7ed3-b686-b04e07eecaf2` (fixture line 7). `thread/list`
          (line 10) identifies it as the operator's TUI thread: preview "Greet me in a
          single word.", `cwd` matching the spike's own working directory,
          `cliVersion":"0.157.1"`. (`originator`/`source` on this record are not used to
          identify the thread — see the still-open UNVERIFIED item on those fields
          below.)
        - **Scope:** Windows, default `CODEX_HOME`, non-elevated terminal — same as the
          `0.154.0` run.
  - [x] **A second client of the same daemon delivers a message with `thread/queue/add` or
        `turn/start`.** Both re-exercised.
        - `turn/start` into the idle thread (line 16): accepted; turn
          `01a0dc63-00d3-78a3-bdbd-2f0666ca8686` completed with `agentMessage: "OAC G2
          RERUN RECEIVED"` (`thread/turns/list` result, line 40).
        - `thread/queue/add` into a busy thread (line 28): accepted with
          `queuedSubmission.id` `01a0dc63-2e68-7633-95c9-46ba70eaeec3`; the daemon's turn
          record shows it started the same second the lighthouse turn (line 24) completed,
          and its `agentMessage` is `"OAC G2 QUEUED"` — queued until idle, delivered in
          order, same behavior as the `0.154.0` run.
        - `turn/steer` again deliberately not exercised (unchanged rationale).
  - [x] **The TUI user sees the delivered message and the model answers it.** The
        operator's own TUI paste, quoted verbatim above: the "OAC G2 RERUN RECEIVED"
        reply, the full 40-item lighthouse list, and the "OAC G2 QUEUED" reply all
        rendered in the TUI, in that order. The daemon's own turn record
        (`thread/turns/list`, fixture lines 33-40) corroborates all three as
        `status: "completed"` in the same order.
  - [x] **OAC holds no OpenAI credentials at any point.** Same finding as the `0.154.0`
        run: the spike client contains no credential handling and never opens
        `CODEX_HOME`; the committed transcript contains no token, bearer value or API key,
        and a post-redaction scan of `transcript-2026-09-26-0.157.1.jsonl` found no
        residual leaks (username, hostname, and installation id all replaced).
- **Verdict:** PASS
- **Fallback taken:** none — not needed. The primary path (implicit daemon attach)
  passed again; `codex --remote ws://…` was not exercised.

#### Current-result fields (apply to the 0.157.1 verdict above)

- **Sources and surface labels** (all retrieved 2026-09-25, `@openai/codex` `0.154.0`,
  commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`; re-checked against `0.157.1`, commit
  `36650394c5b38c2990ccf2a3457165ca3e9d9726`, in `docs/planning/REVERIFICATION-B2.md`
  §"§3.2 re-verification at Codex `0.157.1` (floating-pin trigger, 2026-09-26)" — **no
  drift affecting any fact G2's PASS rested on.** Two additive drifts recorded there
  (neither exercised or needed by this run): (a) a `--no-daemon` opt-out flag, which
  includes a matching rejection added to `codex-rs/tui/src/session_queue_commands.rs`;
  and (b) an opt-in `mcp_2026_07_28` MCP client mode. One behavior-preserving rename
  (`accept_hdr_async` → `accept_hdr_async_with_config`) was also recorded there):
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
  - **Closed (re-confirmed at `0.157.1`):** implicit daemon attach runs at runtime in
    released `0.157.1` — **on Windows, with default `CODEX_HOME`, from a non-elevated
    terminal.** This re-run's own transcript (see criterion 1 above) shows the same
    behavior as `0.154.0`. Source-level: `docs/planning/REVERIFICATION-B2.md`'s
    0.157.1 fact table found the daemon README's attach-or-embed sentence unchanged
    verbatim, and the Windows control-socket protection files
    (`windows_socket_validation.rs`, `windows_peer.rs`) byte-identical between
    `6b9826e3` (0.154.0) and `36650394` (0.157.1). A new `--no-daemon` opt-out flag
    exists in source at `0.157.1` (`codex-rs/tui/src/cli.rs`, `codex-rs/cli/src/main.rs`)
    but was not passed by either run. macOS and Linux remain not exercised.
  - **Closed:** implicit daemon attach runs at runtime in released `0.154.0` — **on
    Windows, with default `CODEX_HOME`, from a non-elevated terminal.** B2 had confirmed only
    that the code path is present in source. macOS and Linux were not exercised.
  - **Closed:** `codex mcp-server` is absent at `0.154.0`, and again at `0.157.1` (local
    `codex mcp-server --help` at `0.157.1` still falls through to top-level help; see
    `docs/planning/REVERIFICATION-B2.md`'s 0.157.1 fact table, fact 7). It is not a
    subcommand at either version. Its deprecation and deletion dates remain unverified.
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
    - Confirmed byte-identical at `0.157.1` (`docs/planning/REVERIFICATION-B2.md`'s
      0.157.1 fact table, fact 6): the analysis above still applies unchanged.
  - **Still open:** cross-process resume does not attach (openai/codex #21743). Not
    re-tested at `0.154.0` or `0.157.1`, because the test appends silently to the
    operator's real thread history.
  - **Still open, inconclusive:** whether Codex Desktop exposes the control socket. The
    evidence is in the uncommitted raw transcript; redaction removed it from fixture
    lines 9 and 18 of the `0.154.0` transcript. There, the daemon's `thread/list` returned
    Desktop-originated sessions (`originator: "Codex Desktop"`) as `notLoaded` saved
    history. That shows shared on-disk history, not live socket exposure.
  - **New, UNVERIFIED — unidentified second loaded thread.** Fixture line 16 of the
    `0.154.0` transcript lists two loaded threads.
    - The second, `01a0d744-b34a-7c92-9011-20d95fe5f98a`, appears **nowhere else** in the
      raw transcript.
    - It is absent from both `thread/list` responses, so redaction did not remove it.
      The line-18 response held 5 entries: the spike thread plus 4 unrelated Desktop
      sessions.
    - Its UUIDv7 prefix places its creation milliseconds after the operator's first
      message in the TUI. It is probably a TUI-spawned, unlisted side thread. Its
      purpose is not identified.
    - **Not reproduced at `0.157.1`:** this re-run's `thread/loaded/list` (fixture
      `transcript-2026-09-26-0.157.1.jsonl` line 7) returned exactly one loaded thread.
      A single non-reproduction on a fresh daemon does not close this item — it stays
      open, unidentified.
  - **New, observed:** `thread/queue/add` requires `threadId`, `clientUserMessageId` and
    `input` (generated experimental schema). The TUI's own `codex queue` command issues
    it (`codex-rs/tui/src/session_queue_commands.rs`, `THREAD_QUEUE_ADD_METHOD =
    "thread/queue/add"`). At `0.157.1` the same command additionally refuses to run
    under `--no-daemon` (`docs/planning/REVERIFICATION-B2.md`'s 0.157.1 fact table).
  - **New, observed:** a client that has not called `thread/resume` receives only
    `thread/status/changed` for a thread, with no `turn/*` or `item/*` events
    (`0.154.0` transcript lines 33-38). To observe a reply, a delivering client must
    subscribe with `thread/resume` (lines 51-75) — or, as this re-run did instead, poll
    `thread/turns/list` once after both turns finish.
  - **UNVERIFIED — mechanism inferred from two fresh-daemon observations, one per
    version.** In a shared daemon, a thread's `originator` and `source` fields do not
    reliably identify the client that created it.
    - At `0.154.0`, the TUI's thread carried `originator: "oac_g2_spike"` — the
      `clientInfo.name` of the first client to initialize that fresh daemon (this
      spike's own probe), not the TUI, plus `"source": "vscode"` although a terminal TUI
      created it (`0.154.0` transcript lines 18, 52).
    - At `0.157.1`, on a separately fresh daemon, the same TUI thread instead carried
      `originator: "codex-tui"` and `"source": "vscode"` (`thread/list`, fixture line
      10) — this time matching the TUI itself. This is consistent with the
      first-initializing-client hypothesis: this re-run's TUI was the first client to
      initialize the `0.157.1` daemon (created at 06:22:33Z, before the spike's probe
      connected at 06:24:16Z), whereas at `0.154.0` the spike's probe connected first.
    - **Conclusion unchanged:** across two versions and two fresh daemons, `originator`
      tracked the first-connecting client, not the thread's actual creator, in one case,
      and coincided with it in the other only because the TUI happened to connect
      first. Do not use either field for provenance; use `thread/list`'s `preview` and
      `cwd`, corroborated by `thread/loaded/list`, as this result does throughout.
  - **New, security-relevant:** any control-socket client can read every saved session's
    prompt preview and rollout path through `thread/list`. The evidence is in the raw
    `0.154.0` transcript, redacted out of fixture lines 9 and 18.
    - Subscribed clients also receive `account/rateLimits/updated`, which carries plan
      type, credits and usage.
    - Both stay within the same non-elevated OS user.
    - Carry both into the local-IPC and cross-project-leakage rows of
      `docs/planning/v0.1/06-security.md` when that file is next revised.
  - **New, operator-observed:** launching the TUI on Windows opened several extra
    console windows. The cause was not investigated.
  - **New, from the 0.157.1 source re-verification** (full detail:
    `docs/planning/REVERIFICATION-B2.md` §"§3.2 re-verification at Codex `0.157.1`
    (floating-pin trigger, 2026-09-26)"): a new, still opt-in MCP `2026-07-28` client
    mode exists in source (feature flag `mcp_2026_07_28`, stage `UnderDevelopment`,
    `default_enabled: false`); a new `--no-daemon` opt-out flag exists
    (`codex-rs/tui/src/cli.rs`, absent at `0.154.0`); and a new `v2` schema surface was
    added alongside the existing one. None of the three is exercised by G2's pass
    criteria; the MCP mode is load-bearing background for a future G4 run. None is a new
    UNVERIFIED item for G2 itself — recorded here only for cross-reference.
- **Fixtures captured:**
  - `docs/planning/gates/fixtures/g2-codex-inject/transcript-2026-09-26-0.157.1.jsonl`,
    40 lines — this re-run, redacted by the same `redact.mjs` script (kept with the
    uncommitted spike scratch, not committed). Covers the WebSocket upgrade,
    `initialize`/`initialized`, `thread/loaded/list`, `thread/list`, `turn/start`,
    `thread/queue/add` and `thread/turns/list`.
    - **Redaction, exactly as `redact.mjs` performs it on this transcript:** username
      and home paths in `codexHome`/`path`/`cwd`/`runtimeWorkspaceRoots` replaced with
      `<USER_HOME>`; the spike's own scratchpad path further replaced with `<SPIKE_DIR>`
      where it appears standalone; the hostname (`remoteControl/status/changed`'s
      `serverName`) replaced with `<HOST>`; the installation id replaced with
      `<INSTALLATION_ID>`; `planType` (in every `account/updated` notification, 4
      occurrences) replaced with `<REDACTED>`. The `thread/list` response at line 10
      originally held 5 entries (the spike's own TUI thread plus 4 unrelated
      Codex-Desktop-originated sessions with their own private previews/rollout paths);
      redaction kept only the spike's own thread and marked the response with
      `_fixtureNote: "unrelated sessions removed for privacy"`.
    - **Residual check:** a post-redaction grep for the username, hostname, and
      installation id found 0 matches.
    - **Deliberately kept (not a secret):** the kept thread's rollout filename,
      `rollout-2026-09-26T01-22-33-01a0dc61-364b-7ed3-b686-b04e07eecaf2.jsonl`, whose
      embedded local timestamp (`01-22-33`, i.e. 01:22:33 local time against the UTC
      `createdAt` of 06:22:33Z) reveals the capturing machine's UTC offset — the same
      category of information the `0.154.0` fixture deliberately kept and documented,
      for consistency between the two fixtures. Not redacted, stated here explicitly
      rather than silently kept.
  - `docs/planning/gates/fixtures/g2-codex-inject/transcript.jsonl`, 75 lines — the
    `0.154.0` baseline run, kept unchanged.
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
  - Take message shapes from the pinned schema, not from either fixture.
  - The location is provisional; D6 owns the final convention.
- **Pin rows relied on:** `Codex CLI / app-server`
- **PINS.md as-of:** 2026-09-26, commit `93151b712a47a8430192d3bbf1abafdedc3341ea`
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
  - The `0.154.0` run's timebox was declared about six minutes after live work began
    (see the `0.154.0` section below). This re-run declared its box first, before any
    live work.
  - `PLANNING-PROMPT.md` §3.2 and the `oac-codex-appserver` skill named
    `thread/queue/add` without noting that it is absent from the default schema. They
    also did not say that the control socket is WebSocket over UDS. The skill is updated
    in the same change as this result.
  - **Review correction (this re-run's own record):** a review of this file's first
    draft found two "absent from the compare diff" claims that were wrong, because
    `gh api repos/openai/codex/compare/rust-v0.154.0...rust-v0.157.1` caps its `files`
    array at 300 out of 869 commits' worth of changes and does not flag the truncation.
    Both are corrected above and in `docs/planning/REVERIFICATION-B2.md`; see that
    file's "Note on the GitHub compare API's 300-file cap."
- **Re-run history:**

  | Date | Pinned versions | Verdict | Invalidated by |
  |---|---|---|---|
  | 2026-09-25 | Codex CLI / app-server `0.154.0` (matches PINS.md); Windows only | PASS | Codex CLI / app-server pin 0.154.0 -> floating (last observed 0.157.1), 2026-09-26 |
  | 2026-09-26 | Codex CLI / app-server `0.157.1` (floating row, last observed version; matches PINS.md and the environment); Windows only | PASS | — (current) |

#### 0.154.0 baseline run (superseded by the current 0.157.1 verdict above; retained for its source-level detail)

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
- Historical verdict at `0.154.0`: PASS (superseded by the current `**Verdict:**` field
  above; primary path (implicit daemon attach) passed, `codex --remote ws://…` fallback
  not exercised — the gate requires the fallback only if the primary path fails).
