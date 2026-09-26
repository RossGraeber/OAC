# G2 — Codex live injection

Source: PLANNING-PROMPT.md §4 G2, §3.2. Backlog: `docs/planning/backlog/03-tasks-CD.json` D2.

## Disposition

**v0.1 go/no-go if both the primary path and the fallback fail.** A fallback exists and must
be attempted before recording a FAIL.

## What the spike proves

That a second client can deliver input into a live Codex thread the user is sitting in, on
the pinned released version.

## Pass criteria (evaluate each individually)

- [ ] A TUI launched normally (no config overrides) attaches to a running
      `codex app-server daemon` (started via `codex app-server daemon start`, control socket
      `CODEX_HOME/app-server-control/app-server-control.sock`).
- [ ] A second client of the same daemon delivers a message using `thread/queue/add` (queued
      until idle) or `turn/start` (when idle) — or `turn/steer` if delivering into an active
      turn.
- [ ] The TUI user sees the delivered message and the model answers it.
- [ ] OAC holds no OpenAI credentials at any point in the exchange; the app-server uses the
      saved CLI login (`CODEX_HOME/auth.json` or OS keyring) throughout.

## Failure criteria

Any of the four unmet on the primary (implicit daemon attach) path is a failure of that path —
run the fallback before recording the gate FAIL. The gate only fails outright if both paths
fail.

## Fallback

OAC owns the app-server; the user runs `codex --remote ws://…` to attach the TUI to the
OAC-owned app-server. Record which path (implicit attach or `--remote`) actually passed in the
gate result.

## Surfaces and version pins

- Version: **floating** (`docs/planning/PINS.md`, "Floating-version policy"). There is no
  fixed pin. Record the observed CLI (`codex --version`), daemon
  (`codex app-server daemon version`) and client `clientInfo` versions in the result, and
  re-verify the §3.2 facts on that version first. The original baseline was
  `@openai/codex` 0.154.0 (2026-09-09), PLANNING-PROMPT.md §3.2.
- `codex app-server` speaks JSON-RPC 2.0 (no header on the wire) over stdio (JSONL), WebSocket
  (`--listen ws://127.0.0.1:PORT`), or Unix socket. Handshake: `initialize` then `initialized`;
  no numeric protocol version.
- Experimental methods require `capabilities.experimentalApi` and are documented as not
  durable — `thread/queue/add` is explicitly called out as experimental.
- Auth boundary: WebSocket listeners support capability-token or signed-bearer auth; Unix
  sockets validate peers. An OAC adapter never holds OpenAI credentials — this is also a
  boundary in `oac-boundaries`, not just a pass criterion here.
- Schema artifacts are checked in at `codex-rs/app-server-protocol/schema/json` and
  `schema/typescript`, regenerable with `codex app-server generate-json-schema`.

## §3.2 facts this spike must confirm or refute

- **The go/no-go UNVERIFIED item this gate closes:** whether implicit Codex daemon attach
  (TUI launched without config overrides attaching to a running daemon) is enabled in the
  released build versus only on `main`. The spike ran on 0.154.0 and passed on Windows. Each
  re-run answers it for the currently observed release, never for `main`.
- Also record:
  - Cross-process resume does **not** attach: confirm that a second app-server process
    resuming a thread another process holds loads history from disk and appends silently, with
    the live process not notified (open issue #21743) — re-confirm this is still true on the
    pinned version.
  - `codex mcp-server` (Codex as an MCP server) was deprecated 2026-08-20 and deleted
    2026-09-05 — confirm it is absent and must not be planned on; do not substitute it for the
    app-server surface (`oac-boundaries` mechanical check 4 greps for this).
  - Control-socket location (`CODEX_HOME/app-server-control/app-server-control.sock`) and peer
    validation behavior.
  - Whether Codex Desktop exposes the control socket in current builds (UNVERIFIED per
    STATUS.md) — record what you observe, even if inconclusive.

## Fixtures to capture (for Stage 3's fake Codex endpoint)

Per D6: `initialize`/`initialized` handshake, `thread/start`, `thread/resume`, `turn/start`,
`thread/queue/add`, and the event stream including `item/completed` and `turn/completed`.
Reference the checked-in schema (`codex-rs/app-server-protocol/schema`) rather than
hand-transcribing method shapes. Capture the pinned version and date; redact any credential or
private-path data before committing.
