---
name: oac-codex-appserver
description: Codex `app-server` JSON-RPC surface — transport/handshake, thread/turn model, live-inject daemon, thread/queue/add vs turn/steer vs turn/start, auth boundary, schema artifacts. Load for area:adapter-codex work items and gate G2.
---

Holds the version-pinned Codex `app-server` protocol detail. Loaded for
`area:adapter-codex` work items (backlog G6-G8, G11) and for gate G2 (Codex live
inject). This is the one skill exception to link-don't-copy: the detail below is
copied in verbatim from PLANNING-PROMPT.md §3.2 because an agent cannot look it up
elsewhere in this repo, and it carries the pin in `## Pin` below. Companion skills,
not restated here: `oac-boundaries` (rollout-file / undocumented-RPC MUST NOTs),
`oac-security-work` (`turn/steer` threat row), `oac-evidence` (citation format).

## Constraints that most often trip an agent up

- **No numeric protocol version exists.** Do not write code or docs that check or
  branch on a "Codex app-server protocol version" — there isn't one. Stability is
  per-method (stable vs experimental), gated by `capabilities.experimentalApi`.
- **No attach to an arbitrary running session.** A second app-server process that
  resumes a thread another process holds does not attach to it — it loads history
  from disk and appends silently; the live process is not notified (issue #21743).
  This is the Codex half of the "no attach to an already-running session" finding
  in PLANNING-PROMPT.md §4 (`ADR-001-A2`).
- **The rollout file is not a supported surface.** `CODEX_HOME/sessions/` is where
  rollouts live, but the file format is explicitly not a supported surface. Never
  read or write it as an integration point — see `oac-boundaries` boundary 13.
- **`codex mcp-server` is gone.** Deprecated 2026-08-20, deleted 2026-09-05. Do not
  plan on it, do not let an agent "remember" it from training data. `codex mcp add`
  (registering *external* MCP servers Codex can call as tools) is the supported
  outbound surface and is unrelated to the deleted inbound one.
- **The daemon's control socket is WebSocket over UDS, not JSONL.**
  `codex app-server proxy` relays raw bytes, so a client must do the HTTP Upgrade and
  frame messages itself. Only `thread/resume` subscribers get `turn/*`/`item/*` events
  (G2, `docs/planning/gates/G2-result.md`).
- **`turn/steer` writes into an in-flight turn.** Treat any code path that can call
  it as a code-execution-adjacent authorization decision, not a convenience API.

## Transport and handshake

`codex app-server` speaks JSON-RPC 2.0 (the header is omitted on the wire) over one
of: stdio (JSONL), WebSocket (`--listen ws://127.0.0.1:PORT`), or a Unix socket.
Handshake is `initialize` then `initialized`; there is no numeric protocol version.
Methods are individually stable or experimental; experimental methods require
`capabilities.experimentalApi` and are documented as not durable.

## Thread and turn model

- `thread/start`, `thread/resume`, `thread/list`, `thread/loaded/list` manage
  threads. Thread ids are UUIDv7 strings and survive restarts.
- `turn/start` takes an input array of `{type:"text",text}` items.
- `turn/steer` appends to the in-flight turn (see security note above and
  `oac-security-work`).
- `turn/interrupt` stops the active turn.
- Rollouts live under `CODEX_HOME/sessions/`; **the rollout file format is
  explicitly not a supported surface** — a boundary trap, see `oac-boundaries`
  boundary 13.

## Events

`thread/started`, `thread/status/changed`, `turn/started`, `turn/completed`
(status `completed|interrupted|failed`), `item/started`, `item/completed`
(**authoritative** — deltas are not), `item/agentMessage/delta`, plus approval
requests, which arrive as server-to-client JSON-RPC requests, not notifications.
Full shapes and the schema-artifact locations: `references/events-and-schema.md`.

## Live injection: how narrow it is, and the decision rule

A shared local daemon (`codex app-server daemon start`, control socket
`CODEX_HOME/app-server-control/app-server-control.sock`) can host threads. The
TUI, when launched **without** config overrides, attaches to that daemon as a
client if one is running, otherwise it embeds its own server. Any client of the
same daemon can then deliver input to a live thread. The CLI wrapper is
`codex queue --thread <id> --message <text>` (merged 2026-08-17, not yet in the
CLI reference).

Decision rule for which call to use, drawn strictly from §3.2 — do not invent a
fourth option:

| Thread state | Call | Notes |
|---|---|---|
| Idle (no turn in flight) | `turn/start` | Starts a new turn. |
| A turn may be in flight and you want the input delivered once the thread goes idle | `thread/queue/add` | Experimental; queued until idle. |
| A turn is actively in flight | `turn/steer` | Appends into the *in-flight* turn. **Unauthorized steer is a code-execution risk (PLANNING-PROMPT.md §7)** — gate any code path that can call this behind an explicit authorization check, and see `oac-security-work` before wiring it up. |

Implicit daemon attach ran at runtime on 0.154.0 (G2 PASS, 2026-09-25; Windows only, with
default `CODEX_HOME` in a non-elevated terminal). **Codex is now floating**: an auto-updater
tracks each release, so re-verify on the version you observe (PINS.md "Floating-version policy"). `thread/queue/add` is absent from the default checked-in schema. Get its
shape from `codex app-server generate-json-schema --experimental`; it requires `threadId`,
`clientUserMessageId` and `input`.

Experimental methods (`thread/queue/add` and any other method gated by
`capabilities.experimentalApi`) must sit behind a named, version-pinned
compatibility shim per backlog task G6 and PLANNING-PROMPT.md §10 — never call
them from adapter code directly.

## Cross-process resume does not attach

A second app-server process resuming a thread another process holds loads history
from disk and appends silently — the live process is never notified. This is an
open issue against `openai/codex`, #21743 (PLANNING-PROMPT.md Appendix B). Codex
Desktop uses a private stdio app-server and does not expose the control socket
(**UNVERIFIED for current builds**).

## Remote TUI — the documented G2 fallback

`codex --remote ws://…` attaches the TUI to an app-server that OAC could own.
PLANNING-PROMPT.md §4 names this the documented fallback if daemon attach fails
gate G2 (`docs/planning/STATUS.md` Gate G2 row: fallback is "OAC-owned app-server
with `codex --remote`").

## Removed surface

`codex mcp-server` (Codex acting as an MCP server) was deprecated 2026-08-20 and
**deleted** 2026-09-05 — do not plan on it, and do not depend on it existing in any
future skill or design revision without a fresh re-verification. `codex mcp add`
still registers **external** MCP servers that Codex can call as tools; that
remains the supported outbound tool surface and is what backlog task G8 targets.

## What cannot originate a turn

Hooks (`SessionStart`, `UserPromptSubmit`, `Stop`, …) and `notify`
(`agent-turn-complete`, carrying `thread-id`, `turn-id`, `last-assistant-message`)
react to events only — neither can originate a turn. `codex exec` and the
TypeScript SDK each spawn their own process per run and cannot inject into another
process's session. None of these four are a substitute for the live-injection path
above.

## Auth boundary

The app-server uses the saved CLI login (`CODEX_HOME/auth.json` or the OS
keyring). **An OAC adapter never holds OpenAI credentials** — this is an
`oac-boundaries` MUST NOT (boundary 3), restated here because it is the Codex-side
instance of it. WebSocket listeners support capability-token or signed-bearer
auth; Unix sockets validate peers. Detail: `references/events-and-schema.md`.

## Schema artifacts and reusable crates

Message shapes are checked in at `codex-rs/app-server-protocol/schema/json` and
`schema/typescript` in `github.com/openai/codex`, regenerable with
`codex app-server generate-json-schema`. Treat the checked-in schema as the
source of truth for message shapes (backlog task G6 acceptance criterion), not
this skill's prose. Repo license Apache-2.0; reusable Rust crates
`app-server-client`, `app-server-protocol`, `app-server-transport` exist under
that license. Full listing: `references/events-and-schema.md`.

## The correlation gap — an open obligation, not a solved problem

Codex has no channel-tag convention for outbound replies, unlike Claude's `meta`
echo-back pattern. PLANNING-PROMPT.md Appendix A conflict C9 records this against
Decision 9 (§5): "Confirm [outbound-through-OAC-tools] holds for Codex through
`codex mcp add`, and define how a Codex reply is correlated when Codex has no
channel-tag convention." That definition is planning work this skill does not
supply — do not invent a correlation scheme here. Check
`docs/planning/v0.1/03-decisions-and-amendments.md` (once the M0 planning
package / Stage 0 produces it) or the current work item for the resolved
mechanism before implementing backlog task G8's correlation acceptance
criterion.

## Where the content lives

- Full baseline: `docs/planning/PLANNING-PROMPT.md` §3.2.
- Gate definition and fallback: `docs/planning/PLANNING-PROMPT.md` §4 (G2);
  current verdict in `docs/planning/STATUS.md`.
- Correlation decision: `docs/planning/PLANNING-PROMPT.md` §5 decision 9,
  Appendix A conflict C9.
- Security detail (`turn/steer`, auth): `docs/planning/PLANNING-PROMPT.md` §7;
  `oac-security-work`.
- Rollout-file and undocumented-RPC boundaries: `oac-boundaries`.
- Event shapes, schema paths, crate/license detail:
  `.claude/skills/oac-codex-appserver/references/events-and-schema.md`.

## Pin

**Re-verified pin (B2):** `@openai/codex@0.154.0` (published 2026-09-09T22:40:10.746Z),
cross-checked against GitHub tag `rust-v0.154.0`, commit
`6b9826e3aa83b1a5947db50f4332cb9c65f1b340` — matches the PLANNING-PROMPT.md
§3.2 baseline exactly, no drift. B2 read the source tree at this exact
commit and confirmed the daemon-attach code path is **present**: the
control socket path `CODEX_HOME/app-server-control/app-server-control.sock`
is defined verbatim in `codex-rs/app-server-transport/src/transport/mod.rs`,
`codex-rs/app-server-daemon/README.md` documents the "skip implicit daemon
attachment... otherwise the TUI starts an embedded server" branch, and a
`codex queue` CLI subcommand exists in `codex-rs/cli/src/main.rs`. This is a
source-level, documented close (`docs/planning/REVERIFICATION-B2.md` §3.2
box 4). The G2 spike added the runtime verdict (2026-09-25): the path executes for
an ordinary invocation on Windows (see `docs/planning/gates/G2-result.md`). Codex Desktop's control-socket exposure in
current builds also remains UNVERIFIED — no first-party statement found
(`REVERIFICATION-B2.md` §3.2 box 5); do not assume a Desktop-hosted thread
is reachable via the control socket. Source: `docs/planning/PINS.md` —
"Codex CLI and app-server" pin record, and
`docs/planning/REVERIFICATION-B2.md` §3.2, retrieved 2026-09-16.

Detail record, sources, and constraint floors: `docs/planning/PINS.md`. Full
re-verification ledger: `docs/planning/REVERIFICATION-B2.md`. A version bump
of `@openai/codex` past 0.154.0 invalidates this pin — re-verify per
`oac-evidence` §7 before trusting it again, and update this Pin section
(and `docs/planning/PINS.md`) when you do.
