# Fake Codex app-server endpoint (F9, #58)

A test double for the Codex `app-server` JSON-RPC surface. It replays the recorded Stage 1
Codex fixtures over stdio (JSONL) and a loopback WebSocket, so the adapter contract suite
(F10, #59), the security suite (F11, #60) and the fake-harness integration tier can run
with no Codex install, no credential, and no network beyond loopback (`oac-testing` §2).

Surface label: the real surface is **experimental** (`thread/queue/add` is gated by
`capabilities.experimentalApi`; `oac-codex-appserver`). Codex floats (#216): the fake
replays what the fixtures recorded, on Codex `0.157.1` (D6) and `0.160.0` (G2, G5).

## Running it

```text
node tests/fakes/codex-app-server/server.mjs --stdio
node tests/fakes/codex-app-server/server.mjs --listen ws://127.0.0.1:0
node tests/fakes/codex-app-server/server.mjs --stdio --listen ws://127.0.0.1:0
node tests/fakes/codex-app-server/self-test.mjs
```

- `--stdio`: one connection on stdin/stdout, one JSON message per line. The process exits
  when stdin closes.
- `--listen ws://127.0.0.1:<port>` (port `0` picks one): a WebSocket listener. Only the
  literal loopback addresses `127.0.0.1` and `[::1]` are accepted. `localhost` is refused,
  because what it binds to depends on name resolution. The first stderr line is
  `{"oacFakeCodex":"listening","url":"ws://127.0.0.1:<port>"}`.
- Size cap: a WebSocket message over 16 MiB (`16777216` bytes, the limit the recorded
  handshake advertises) is refused from its frame header, without buffering it, with close
  code `1009`. A stdin line over the same size is refused too, but only after readline has
  read it. Either way it is logged as `too-large` and answered with the fake's own error.
  There is no `Origin` check; the listener is loopback-only and holds no secret.
- Every connection shares one state, as clients of one Codex daemon do. A test can give
  the adapter the stdio connection and drive the fake (`oacFake/*`) and a "TUI user" over
  WebSocket connections of its own.
- In-process (Node tests): `new FakeCodexAppServer()` from `lib/model.mjs`, then
  `fake.connect(send)` returns `{ receive(msg), close() }`.

Node built-ins only; no `package.json`. Rust tests spawn it with `node` and the path above.

## Control methods (`oacFake/*`)

These are the fake's own methods, never Codex methods. Any connection may call them. They
are logged in a separate control log (`control` in the `oacFake/calls` result, with its own
`seq` and the caller's `clientName`), so a suite can assert that an adapter never makes one.

| Method | Params | Result |
|---|---|---|
| `oacFake/turn/complete` | `threadId`; `status` `"completed"` (default) or `"interrupted"`; `agentText` (optional) | `{threadId, turnId, status, dispatched}`: ends the running turn. With `agentText` it first emits the agent message item. On `"completed"` the queue head is dispatched (`dispatched` is the new turn id, or `null`) |
| `oacFake/thread/create` | `cwd`, `ephemeral`, `archived`, `subagent` (`"multi-agent-v2"` or `"thread-spawn"`), `loaded` (default `true`), `materialized` (default `true`) | `{threadId}`: a thread in a state the fixtures could not create, for the queue refusals |
| `oacFake/queue/setAvailable` | `available` | `{available}`: `false` models a host with no queue service |
| `oacFake/calls` | `clientName` (optional filter) | `{calls, steering, overrideMembers, handOffs, malformed, control}`: the call log (below); `steering` to `malformed` are `seq` lists; `control` is the control log |
| `oacFake/thread/state` | `threadId` | `{loaded, materialized, activeTurnId, lastTurnInterrupted, queue, turns, subscribers}` |
| `oacFake/templates` | none | which fixture file and line each replayed template came from |

Turns never progress on a timer. A turn runs until `oacFake/turn/complete`, so every test
is deterministic and needs no sleep.

## Call log: checking that an adapter never steers

Every Codex request and notification a client sends is logged, before the fake decides
anything, as `{seq, connection, transport, clientName, kind, method, id, params, flags,
outcome, error?}`. `clientName` is the `clientInfo.name` from `initialize`, so a test filters
the adapter's calls from the "TUI user" connection's. Flags:

| Flag | Set on | Rule it lets a test assert |
|---|---|---|
| `steering` | every `turn/steer` and every `turn/start`, whatever the thread state | [MCPB-CDX-003], [MCPB-CDX-004], [SEC-AUZ-022]: the adapter's list must be empty |
| `steered` | a `turn/start` that joined a running turn | the C1 hazard, as recorded (below) |
| `overrideMembers` | a `thread/queue/add` or `turn/start` with members beyond the hand-off's own | [MCPB-CDX-005] |
| `handOff` | `thread/queue/add`, `turn/start`, `turn/steer` | [MCPB-CDX-002]: every adapter hand-off is `thread/queue/add`; [IFC-ADP-057]: at most one per `HandOff` |
| `experimentalGateRefused` | a `thread/queue/add` refused for a missing `experimentalApi` | the G6 shim declares the capability |
| `malformedParams` | a call whose `params` is present and not a JSON object | the adapter sends well-formed requests |
| `malformed` (entry `kind: "malformed"`, `method: null`, the raw `frame`) | `batch`, `not-an-object`, `unparseable`, `no-method`, `unsolicited-response`, `too-large` | the `malformed` list is empty |
| `inBatch` (the batch entry's `seq`) | each member of a JSON-RPC batch that names a method | a steering call cannot hide in a batch |

`turn/steer` itself is answered with the fake's `NOT_MODELLED` error (no fixture records
it), and the call is still logged and flagged.

Nothing a client sends can stop the fake. Every frame is logged before anything else
happens to it, including frames that are not JSON, not objects, or JSON-RPC batches.
Each member of a batch that names a method gets its own entry, with `steering` and the
other flags set as for a single call. No batch member is dispatched, and the batch is
answered with one `NOT_MODELLED` error with `id: null`. A call whose `params` is not an
object gets `NOT_MODELLED`. An unexpected exception in the fake is answered with code
`-32098` (`data.oacFake: "internal-error"`) and written to stderr, and the process keeps
serving.

## Provenance

Templates are read from the fixture files in place, at start-up
(`lib/fixtures.mjs`), and never copied: the files are under
`docs/planning/gates/fixtures/`, each with its `MANIFEST.json` entry, which the self-test
checks. A frame missing from its fixture stops the fake from starting.

### Recorded behaviours

| Behaviour | Fixture (`docs/planning/gates/fixtures/`) |
|---|---|
| `initialize` result, then `remoteControl/status/changed`; `account/updated` after `initialized` | `d6-codex-protocol/transcript-conn1-2026-09-28.jsonl` (`0.157.1`), same order in every D6 file |
| `thread/start` result, then `thread/started`; the connection is subscribed | D6 `transcript-conn1` |
| `thread/resume` (`excludeTurns: true`) result, then `thread/goal/cleared` to every subscriber | D6 `transcript-conn2`, and conn1's copy of the same `thread/goal/cleared` |
| `thread/resume` before the thread's first turn: `-32600 "no rollout found for thread id <id>"` | D6 `transcript-conn2-attempt1-failed-resume-try1-2026-09-27.jsonl` |
| `turn/start` on an idle thread: result (`inProgress`, `startedAt: null`), then `thread/status/changed` active, `turn/started`, `item/started` and `item/completed` for the `userMessage` | D6 `transcript-conn3-turn1`, events from `transcript-conn1` |
| Turn end: `agentMessage` `item/started`, `item/agentMessage/delta`, `item/completed`, then `thread/status/changed` idle, `turn/completed` (`completed`) | D6 `transcript-conn1` |
| `thread/queue/add` result `{queuedSubmission}`; `thread/queue/changed` to subscribers before the adder's response | D6 `transcript-conn3-queue` and `transcript-conn1` |
| Queued until idle: an add during a running turn waits; at `turn/completed` comes `thread/queue/changed`, then a new turn whose `userMessage` carries `clientId` = `clientUserMessageId` | D6 `transcript-conn1`/`conn2`, the "Added while busy" check in `MANIFEST.json`; the same at `0.160.0` in G2 `transcript-2026-10-06-0.160.0-herdr.jsonl` (`busyqueue`) |
| Only `thread/start` and `thread/resume` subscribe a connection; a connection that only sends `turn/start` or `thread/queue/add` gets its response only | D6 `transcript-conn3-*` (see also the open item `11-risks.md` row 50) |
| `turn/start` during a running turn returns the running turn's id, and its input joins that turn | G5 `transcript-codex-2026-10-02-0.160.0-herdr.jsonl` L58 (response) and L66 (`thread/turns/list`) |
| `thread/loaded/list`, `thread/list` (`cursor: null`, `limit`, `sortKey: "created_at"`), `thread/turns/list` (`sortDirection: "desc"`, `itemsView: "full"`) | G2 `transcript-2026-10-06-0.160.0-herdr.jsonl` |
| `thread/list` leaves out a loaded thread that has had no turn | G2 0.160.0 L17-L20 (the thread is loaded but not listed until its first turn) |
| WebSocket 101 header `x-codex-websocket-max-unfragmented-message-bytes: 16777216` | line 2 of every D6 file |

The self-test's replay case drives the fake through the D6 attempt-2 sequence and compares
every frame each connection receives with the recorded transcript: order, kind, and JSON
shape.

### Source-only behaviours (runtime UNVERIFIED)

No fixture records these. Each is modelled from first-party source, `github.com/openai/codex`
tag `rust-v0.160.0`, commit `a956835d020762cb2b570053af06f643a11c0ecc`, retrieved
2026-10-06 (`B` = `https://github.com/openai/codex/blob/a956835d020762cb2b570053af06f643a11c0ecc/codex-rs`).
They are listed in `docs/planning/STATUS.md` "Open UNVERIFIED items" and
`docs/planning/v0.1/11-risks.md` rows 65-67. The owner is G6 (#67) for the
experimental-API gate and G7 (#68) for the rest.

| Behaviour | Source |
|---|---|
| Experimental-API gate: a `thread/queue/add` on a connection whose `initialize` did not set `capabilities.experimentalApi: true` gets `-32600 "thread/queue/add requires experimentalApi capability"` | `B/app-server/src/message_processor.rs` L975-L979; `B/app-server-protocol/src/experimental_api.rs` L30-L32; `#[experimental("thread/queue/add")]` at `B/app-server-protocol/src/protocol/common.rs` L623; `-32600` at `B/app-server/src/error_code.rs` |
| A request before `initialize`: `-32600 "Not initialized"` | `B/app-server/src/message_processor.rs` L971-L972 |
| `thread/resume` of an id the fake has never seen gets the recorded `-32600 "no rollout found for thread id <id>"` (the recording covers only a known thread before its first turn) | `B/thread-store/src/local/read_thread.rs` L97-L102 returns this message whenever no rollout resolves for the id; `B/app-server/src/request_processors/thread_processor.rs` L3194-L3195 maps `ThreadNotFound` to the same message |
| `thread/queue/add` refusals, all `-32600`, in handler order: an ephemeral loaded thread, an unknown thread (`thread not found: <id>`), an archived unloaded thread, a loaded multi-agent v2 subagent, an unloaded spawned subagent, no queue service | `B/app-server/src/request_processors/thread_queue_processor.rs` `add()`, `require_thread()`, `ensure_direct_input_allowed()`, `service()`, and L49-L50; `B/app-server/src/request_processors/thread_input.rs` L8-L9 |
| After a turn ends `interrupted`, nothing is dispatched; an add to an idle thread whose last turn was interrupted also waits, until a turn completes uninterrupted | `B/ext/queue/src/service.rs` `on_thread_idle` (skips `ThreadIdleCause::Interrupted`) and `wake_if_loaded` (skips `AgentStatus::Interrupted`); `spec/bindings/mcp.md` §8.2.1 |
| One queued item per idle, from the head of the queue | `B/ext/queue/src/service.rs` `dispatch_if_idle` (`list_page(.., 0, 1)`); order among several adds stays open as `11-risks.md` row 62 |
| `turn/completed` with status `interrupted`: the recorded `turn/completed` shape with `status: "interrupted"` | `TurnStatus::Interrupted` in `B/app-server-protocol/src/protocol/v2/turn.rs` L33-L38; the shape of an interrupted turn's frames is not recorded |
| An extra member in `thread/queue/add` is accepted and ignored (flagged in the call log) | `spec/bindings/mcp.md` §8.2.1: no `deny_unknown_fields`, "probably ignored; not exercised" |

### Not modelled

Answered with the fake's own error, code `-32099` (`NOT_MODELLED`, in the JSON-RPC
implementation-defined range), message `oac fake Codex app-server: <method>: not modelled
(...)` and `data: {"oacFake":"not-modelled","method":...}`. The fake never invents a Codex
error message or frame for these:

- `turn/steer` (logged and flagged), `turn/interrupt`, `thread/fork`,
  `thread/queue/{list,update,delete,reorder,start}`, and every other method no fixture
  records;
- server-to-client requests (approvals) and client responses to them;
- events for a `turn/start` that joins a running turn: the fake adds the input to that turn
  (G5 L66) and returns its id (G5 L58), but sends subscribers no `item/started` or
  `item/completed` for it. The G5 steering connection was not subscribed, so no such event
  was recorded;
- JSON-RPC batches (logged and flagged, never dispatched);
- `thread/resume` without `excludeTurns: true`, or of an ephemeral, archived or subagent
  thread; `thread/start` with members other than `cwd`; `turn/start` with setting overrides;
  non-text input items; paging by cursor; `thread/turns/list` other than `desc`/`full`;
  a malformed thread id; a missing `clientUserMessageId`; a second `initialize`;
- the Unix-socket control socket of the shared daemon (the fixtures went through it; the
  fake offers the same WebSocket framing on loopback TCP), and WebSocket auth
  (capability token, signed bearer);
- `optOutNotificationMethods` and other `initialize` capabilities besides `experimentalApi`.

### Omitted notifications

Recorded, but not sent, because they describe the operator's own setup or account and no
OAC behaviour depends on them: `mcpServer/startupStatus/updated`,
`thread/tokenUsage/updated`, `account/rateLimits/updated`, `thread/name/updated`,
`thread/closed`. Agent text arrives as one `item/agentMessage/delta`, where the real
harness streams several; the recorded `reasoning` items are not produced. Ids are fresh
UUIDv7 strings and times are the current clock; recorded placeholders (`<USER_HOME>`,
`<SPIKE_DIR>`, `<HOST>`, `<REDACTED>`) are replayed as recorded.

## Containment

Dev/test only (`docs/planning/v0.1/07-repository-and-dependencies.md` §2, `tests/` row):

- not a workspace member and not a crate; no `package.json`; no manifest references it, so
  `scripts/check-crate-deps.mjs` and `scripts/check-licenses.mjs` never see it;
- no third-party dependency (Node built-ins only);
- it is under `tests/fakes/`, not `tests/integration/`: a CI-default contract test in a
  product crate may spawn it by path, which `scripts/check-herdr-containment.mjs` check 9
  forbids for the opt-in `tests/integration/` leaf;
- it is never invoked by the `oac` binary and is not shipped.

A product path may not *import* code from `tests/fakes/` (spawning it by path from a
crate's test is the intended use): `scripts/check-containment.mjs` check 13 (#61, F12), the
check-9 sibling, enforces this in CI.

Its self-test runs in the `test` job of `.github/workflows/ci.yml` (#61; before that, job
`fake-codex` of `boundary-lint.yml`), on Linux, Windows and macOS.
