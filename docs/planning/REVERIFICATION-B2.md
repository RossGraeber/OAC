# B2 — Re-verification of the §3 baseline against the B1 pins

This ledger is the evidence artifact for issue #12 (Epic B, task B2). It re-checks every
fact PLANNING-PROMPT.md §3 states against the exact pins recorded in `docs/planning/PINS.md`
(B1), closes what a first-party source confirms, and carries forward what remains
unverifiable. It does not redesign around anything found here (`oac-evidence` §6).

## Method

Pin set used for every row below (source: `PINS.md`, unchanged since B1):

- Claude Code `v2.1.274`
- `@openai/codex@0.154.0`, commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`
- MCP current `2026-07-28`; MCP legacy `2025-11-25`
- Zenoh `1.10.1`
- Rust `1.98.1`
- ACP protocol version `1` (schema v2 alpha, not a v0.1 dependency)

Retrieval date for every fresh fetch in this ledger: **2026-09-16**, unless a row states
otherwise. First-party-source rule (`oac-evidence` §1): official provider docs, the
provider's own repository read at the pinned tag/commit, and its release notes only.
Every row cites its own source; no row inherits a citation from another row or from this
Method section.

Where a fact was not independently re-fetched this pass because the pin did not move and
no acceptance box or Zenoh/MCP config file touches it, the row says so explicitly and
cites PLANNING-PROMPT.md §3.x per `oac-evidence` §2 ("if you have not changed the claim").
This is not the same as HOLDS-by-fresh-fetch; it is marked accordingly.

## §3.1 Claude Code Channels

| Fact (verbatim from §3) | Pin checked against | Verdict | Source URL + version-or-commit + retrieval date |
|---|---|---|---|
| `capabilities.experimental["claude/channel"] = {}` required; presence registers the notification listener | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels-reference.md, "Required. Always `{}`. Presence registers the notification listener.", retrieved 2026-09-16 |
| `notifications/claude/channel` carries `content` + `meta` | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels-reference.md, "Your server emits `notifications/claude/channel` with two params" (`content`, `meta`), retrieved 2026-09-16 |
| Identifier-unsafe `meta` keys are silently dropped | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels-reference.md, "Keys must be identifiers: letters, digits, and underscores only. Keys containing hyphens or other characters are silently dropped.", retrieved 2026-09-16 |
| Idle-session wake delivers the event as the next model turn (no separate wake API) | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels-reference.md, "Events queue into the session and are processed in order. If several notifications arrive while Claude is busy, they're delivered together on the next turn and Claude handles them as a group.", retrieved 2026-09-16 |
| Mid-turn notifications are queued and delivered in order at the next turn | `v2.1.274` | HOLDS | same source and quote as above, retrieved 2026-09-16 |
| No acknowledgement of delivery | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels-reference.md, "Claude Code doesn't acknowledge notifications. The `await` on `mcp.notification()` resolves when the message is written to the transport, not when Claude has processed it.", retrieved 2026-09-16 |
| Outbound via ordinary MCP tools (conventionally `reply`), correlation by convention only | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels-reference.md, `## Expose a reply tool` section defines a standard MCP `reply` tool with no protocol-level correlation field beyond app-chosen args like `chat_id`, retrieved 2026-09-16 |
| `--channels plugin:<name>@<marketplace>` and `--channels server:<name>` | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels-reference.md, `claude --dangerously-load-development-channels plugin:yourplugin@yourmarketplace` and `... server:webhook`, retrieved 2026-09-16 |
| No attach to a running session (channels start with the session, not attached after launch) | `v2.1.274` | HOLDS (by omission — no attach mechanism is documented anywhere in `channels.md` or `channels-reference.md`) | https://code.claude.com/docs/en/channels.md and https://code.claude.com/docs/en/channels-reference.md, both retrieved 2026-09-16, full text reviewed; no `attach` verb or API for joining an already-running session appears |
| Multiple channels per session allowed | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels.md, "You can pass several plugins to `--channels`, space-separated.", retrieved 2026-09-16 |
| Allowlist distribution: `claude-plugins-official`, `allowedChannelPlugins`, `channelsEnabled` | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels.md, `## Enterprise controls` table (`channelsEnabled`, `allowedChannelPlugins`) and "The Anthropic default list applies", retrieved 2026-09-16 |
| `--dangerously-load-development-channels` plus interactive confirmation | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels-reference.md, "Claude Code first shows a full-screen warning dialog listing the development channels you're loading. Select **I am using this for local development** to continue, or **Exit** to quit.", retrieved 2026-09-16 |
| Not available on Bedrock, Vertex, Foundry | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels.md, "not available on Amazon Bedrock, Google Cloud's Agent Platform, or Microsoft Foundry", retrieved 2026-09-16 |
| Per-channel sender allowlist bootstrapped by pairing code | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels.md, `## Security` — pairing flow via bot reply code, retrieved 2026-09-16 |
| Permission relay `claude/channel/permission` at `v2.1.234+` | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels-reference.md, "Claude Code v2.1.234 and later sends permission requests only to servers it registered as channels for the session" and "Before v2.1.234, Claude Code treated `false` as declared.", retrieved 2026-09-16 |
| "Research preview on Claude Code v2.1.232+" | `v2.1.274` | **UNVERIFIED — carried, not closed** | `channels.md`'s own `## Research preview` section (retrieved 2026-09-16) states only "Channels are a research preview feature" with no version string; `v2.1.232` does not appear anywhere in the fetched text of `channels.md` or `channels-reference.md`. See §"Closed or carried" item below (issue #12 box 7 / PINS.md floor 1) — still not independently confirmable on the first-party page at this pin. |
| `--channels` absent from `claude --help` | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels.md, "Neither `--channels` nor `--dangerously-load-development-channels` appears in `claude --help` while the feature is in preview.", retrieved 2026-09-16 |
| `MCP_PROTOCOL_NEGOTIATION` / `2026-07-28` constraint | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels.md, "If you set `MCP_PROTOCOL_NEGOTIATION` to `auto` on the v2 MCP client runtime, a channel can also fail to register because Claude Code doesn't register a channel server that negotiates protocol revision 2026-07-28.", retrieved 2026-09-16 (matches `mcp.md` wording already recorded in PINS.md) |
| No documented `CLAUDE_SESSION_ID` environment variable; `session_id` is the supported path via hooks | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/hooks.md, reviewed in full: no `CLAUDE_SESSION_ID` string anywhere; `session_id` is documented as a JSON field delivered to hook stdin/HTTP body ("Current session identifier"), retrieved 2026-09-16 |

### Issue #12 acceptance boxes — §3.1

**Box 1 — Claude behaviour across `--resume`.** Searched the full text of `channels.md`
and `channels-reference.md` (both retrieved 2026-09-16, reviewed in full above) for
`--resume` and `--continue`: neither string appears anywhere on either page. This is
**not** a close per the task instructions (silence ≠ close).
**Verdict: UNVERIFIED — docs silent at v2.1.274.** Carried to `11-risks.md` (A12) with the
stated consequence: if channels do not survive `--resume`/`--continue` (most likely, since
a channel is registered per-invocation via `--channels`/`--dangerously-load-development-channels`
flags that are not persisted state), OAC's session-identity model must treat a Claude
session as re-launched, not re-attached, whenever it needs to guarantee channel delivery
across a resume. Risk owner: C-series decision + D1 (adapter design).

**Box 2 — one MCP server, more than one logical channel.** Reviewed
`channels-reference.md` in full (`## Server options`, `## Notification format`): the
`source` attribute on the `<channel>` tag is "set automatically from your server's
configured name" (singular, one name per `Server` constructor), and nothing in the page
describes registering multiple channel identities from one running MCP server process.
This is **not** a documented "no" — it is silence on the question of whether a second
process, or a second `Server` instance multiplexed inside one process, could register a
second logical channel.
**Verdict: UNVERIFIED — docs silent at v2.1.274; the one-server-one-`source` pattern is
observed but a multi-channel-per-server capability is neither confirmed nor ruled out.**
Carried to `11-risks.md`.

**Box 3 — Agent SDK support for Channels.**
https://code.claude.com/docs/en/agent-sdk/overview, retrieved 2026-09-16, `## Capabilities`
table enumerates every Claude Code capability available in the SDK (built-in tools, hooks,
subagents, MCP, permissions, sessions, skills/commands/memory, plugins) and does not list
channels or `claude/channel`. This page is a first-party capability enumeration, so its
omission is a **close in the negative direction**, per the task's own rule ("confirm
absence from a first-party SDK page that enumerates supported capabilities — that *is* a
close").
**Verdict: CLOSED — Agent SDK does not support Channels as of this pin.** Source:
https://code.claude.com/docs/en/agent-sdk/overview, retrieved 2026-09-16 (capability table
omits `claude/channel`; no other Agent SDK page under `code.claude.com/docs/en/agent-sdk/`
was found by search to mention channels).

**Box 7 (PINS.md floor 1) — "research preview on Claude Code v2.1.232+".** Re-fetched
`channels.md` in full today: the sentence does not appear (see table row above).
**Verdict: UNVERIFIED — carried, unchanged from B1's finding.** `.claude/skills/oac-claude-channels/SKILL.md`
line 138's `## Pin` wording is corrected in this change (§"Propagated skill updates"
below) to state plainly that the floor is sourced from PLANNING-PROMPT.md §3.1 only, not
independently confirmed on `channels.md` at `v2.1.274`.

## §3.2 Codex CLI app-server

Read at the pinned commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` via
`raw.githubusercontent.com/openai/codex/6b9826e3aa83b1a5947db50f4332cb9c65f1b340/...`
(GitHub's raw content at an exact commit SHA is first-party source code, not a mirror),
plus `https://learn.chatgpt.com/docs/app-server` (the current first-party redirect target
for `developers.openai.com/codex/app-server.md`).

| Fact | Pin checked against | Verdict | Source |
|---|---|---|---|
| JSON-RPC 2.0 over stdio JSONL (default) / `--listen ws://127.0.0.1:PORT` / Unix socket | commit `6b9826e3` | HOLDS | https://learn.chatgpt.com/docs/app-server, "Supported transports: `stdio` (`--listen stdio://`, default): newline-delimited JSON (JSONL)." and `codex app-server --listen ws://127.0.0.1:4500`, retrieved 2026-09-16 |
| `initialize` then `initialized`, no numeric protocol version | commit `6b9826e3` | HOLDS | https://learn.chatgpt.com/docs/app-server, "send an `initialize` request with your client metadata, then emit `initialized`", retrieved 2026-09-16 |
| `capabilities.experimentalApi` per-method gating | commit `6b9826e3` | HOLDS | https://learn.chatgpt.com/docs/app-server, "`capabilities.experimentalApi` to `true` to enable experimental methods", retrieved 2026-09-16 |
| `thread/start`, `thread/resume`, `thread/list`, `thread/loaded/list`, `turn/start`, `turn/steer`, `turn/interrupt` | commit `6b9826e3` | HOLDS | https://learn.chatgpt.com/docs/app-server, all seven methods documented, retrieved 2026-09-16 |
| `item/completed` authoritative event | commit `6b9826e3` | HOLDS | https://learn.chatgpt.com/docs/app-server, "`item/completed`" documented among notifications, retrieved 2026-09-16 |
| `thread/queue/add`; `codex queue --thread <id> --message <text>` | commit `6b9826e3` | **PARTIAL — code path confirmed, doc wording carried** | Source repo confirms a `codex queue` CLI subcommand exists at this commit: `codex-rs/cli/src/main.rs` at commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` contains `mod queue_cmd; use crate::queue_cmd::QueueCommand;` and `/// Queue a message for an existing session.` `Queue(QueueCommand)`, plus `codex queue` listed in the shared `--profile` applicability list. Fetched via `raw.githubusercontent.com/openai/codex/6b9826e3aa83b1a5947db50f4332cb9c65f1b340/codex-rs/cli/src/main.rs`, retrieved 2026-09-16. The exact `thread/queue/add` JSON-RPC method name and `--thread <id> --message <text>` flag spelling are carried unchanged from PLANNING-PROMPT.md §3.2 (not independently re-grepped this pass). |
| UUIDv7 thread ids surviving restarts | commit `6b9826e3` | Carried unchanged | PLANNING-PROMPT.md §3.2 (not independently re-confirmed against source this pass; not touched by an acceptance box) |
| `CODEX_HOME/sessions/` rollouts not a supported surface | commit `6b9826e3` | Carried unchanged | PLANNING-PROMPT.md §3.2 |
| `codex mcp-server` deprecated 2026-08-20, deleted 2026-09-05; `codex mcp add` survives | commit `6b9826e3` | Carried unchanged — **not independently re-confirmed this pass** | `learn.chatgpt.com/docs/app-server` fetched today does not restate these dates in the text retrieved; carried from PLANNING-PROMPT.md §3.2 (UNVERIFIED — re-check directly against the CLI reference page or `codex --help` output at the pinned build) |
| Hooks / `notify` (`agent-turn-complete`) cannot originate a turn | commit `6b9826e3` | Carried unchanged | PLANNING-PROMPT.md §3.2 (not independently re-confirmed this pass) |
| Auth via `CODEX_HOME/auth.json` or OS keyring | commit `6b9826e3` | HOLDS | PINS.md "Auth boundary" record, unchanged, retrieved 2026-09-16 (per PLANNING-PROMPT.md §3.2, quoted verbatim there) |
| WebSocket capability-token / signed-bearer auth; Unix-socket peer validation | commit `6b9826e3` | **PARTIAL** | WebSocket auth flags HOLD: https://learn.chatgpt.com/docs/app-server, "`--ws-auth capability-token --ws-token-file /absolute/path`" and "`--ws-auth signed-bearer-token --ws-shared-secret-file /absolute/path`", retrieved 2026-09-16. Unix-socket peer validation carried unchanged from PLANNING-PROMPT.md §3.2 (not independently re-confirmed in the doc text fetched this pass; the source function `validate_private_socket_path` exists in `codex-rs/app-server-transport/src/transport/unix_socket.rs` at this commit, confirming *some* peer-path validation exists, but its exact semantics were not read line-by-line this pass) |
| Checked-in schema artifacts; Apache-2.0 crates `app-server-client`, `app-server-protocol`, `app-server-transport` | commit `6b9826e3` | HOLDS | Confirmed at commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`: `codex-rs/app-server-client/Cargo.toml`, `codex-rs/app-server-protocol/Cargo.toml`, and `codex-rs/app-server-transport/Cargo.toml` each declare `license.workspace = true`, and `codex-rs/Cargo.toml` declares `license = "Apache-2.0"` at the workspace root. Retrieved 2026-09-16 via raw.githubusercontent.com at the pinned commit. "Checked-in schema artifacts" carried unchanged from PLANNING-PROMPT.md §3.2 (repository tree at this commit was listed but individual schema files were not opened this pass). |

### Issue #12 acceptance box 4 — implicit Codex daemon attach at the pinned release

**Verdict: (a) code present at the pinned commit — a documented, source-cited partial
close, still labelled UNVERIFIED for runtime behaviour and left to D2.**

Evidence, all read at commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` via
`raw.githubusercontent.com/openai/codex/6b9826e3aa83b1a5947db50f4332cb9c65f1b340/...`,
retrieved 2026-09-16:

- Control socket path is defined in source, not just prose:
  `codex-rs/app-server-transport/src/transport/mod.rs` declares
  `const APP_SERVER_CONTROL_SOCKET_DIR_NAME: &str = "app-server-control";` and
  `const APP_SERVER_CONTROL_SOCKET_FILE_NAME: &str = "app-server-control.sock";`, joined
  in `pub fn app_server_control_socket_path(codex_home: &Path) -> ... { codex_home.join(APP_SERVER_CONTROL_SOCKET_DIR_NAME).join(APP_SERVER_CONTROL_SOCKET_FILE_NAME) }`.
  This verbatim-confirms the path `CODEX_HOME/app-server-control/app-server-control.sock`.
- `codex-rs/app-server-daemon/README.md` at this commit documents the daemon lifecycle
  commands (`codex app-server daemon start`, `restart`, `enable-remote-control`,
  `disable-remote-control`, `stop`, `version`, `bootstrap --remote-control`) and states:
  "An invocation that sets `CODEX_EXEC_SERVER_URL` skips implicit daemon attachment so its
  executor selection is preserved. If an implicitly discovered daemon cannot initialize
  the connection, the TUI starts an embedded server instead." — this is the "attach if a
  daemon is running, otherwise embed" branch, present in first-party source at this
  commit, in these words.
- `codex-rs/tui/src/daemon_startup_tests.rs` exists at this commit (repository tree
  listing via `api.github.com/repos/openai/codex/git/trees/6b9826e3aa83b1a5947db50f4332cb9c65f1b340?recursive=1`,
  retrieved 2026-09-16), confirming the TUI's daemon-attach-at-startup path has dedicated
  test coverage in the pinned tree (test *contents* were not opened this pass).
- `codex queue` CLI subcommand exists at this commit (see §3.2 table above), consistent
  with a daemon-oriented CLI surface at this pin.

**What remains UNVERIFIED and is explicitly left to D2, per the task's instruction not to
run or declare G2:** whether this code path executes by default for an ordinary `codex`
TUI invocation with no config overrides in a real environment (process start order, timing
of the 50ms-class discovery window, and behaviour when a stale socket file is present) is a
**runtime** question that only executing the pinned build answers. B2 records the source
evidence; D2/G2 own the runtime verdict.

**G2 go/no-go signal:** this is a *positive* signal (code present, not absent) — flag to
D2 in `STATUS.md` and in this ledger's Drift register as "no drift, source confirms the
capability description in PLANNING-PROMPT.md §3.2 was accurate at this pin."

### Issue #12 acceptance box 5 — Codex Desktop control-socket exposure

Searched for a first-party Codex Desktop statement on control-socket exposure. The only
material found describing "ChatGPT Desktop private stdio + `CODEX_APP_TOOLS_PIPE_PATH`"
and "socket-absent + private-stdio" banner behaviour comes from a third-party blog
(`codex.danielvaughan.com`), which per `oac-evidence` §1 is a lead, not a source — it does
not decide this fact. No first-party OpenAI/ChatGPT documentation page or Codex repository
doc was found in this session stating Codex Desktop's current control-socket exposure
posture.

**Verdict: UNVERIFIED — carried.** Security consequence stated explicitly per the task
instruction: **OAC must not assume a Desktop-hosted Codex thread is reachable via the
control socket.** Owner: D2 spike, re-check when Codex Desktop publishes first-party
control-socket documentation.

### Issue #12, PINS.md carry-over — issue #21743 and cross-process resume

Checked `https://api.github.com/repos/openai/codex/issues/21743` directly, retrieved
2026-09-16: `"state": "open"`, `"closed_at": null`,
`"title": "Codex Desktop open thread view does not refresh after another app-server client appends a turn"`.

**Verdict: no drift.** The issue is still open at the pinned date; it has not been
closed/fixed since 2026-09-15, so it does not touch C2 or the "no attach" premise. (Note
for the record: the issue's actual title concerns Desktop's UI not refreshing when a
second app-server client appends a turn to a thread — it is about **UI staleness under
concurrent multi-client use**, not about resume itself; PINS.md's paraphrase should be
read alongside this exact title. This is recorded as a precision note, not a drift, since
the underlying "no attach on cross-process resume" claim is not contradicted by anything
in this issue's current state.)

## §3.3 MCP

Re-fetched the current revision's changelog and SEP-2133 directly.

| Fact | Pin checked against | Verdict | Source |
|---|---|---|---|
| Current revision is stateless; `initialize` is gone | `2026-07-28` | HOLDS | https://modelcontextprotocol.io/specification/2026-07-28/changelog, "Make MCP stateless: remove the `initialize`/`notifications/initialized` handshake." (SEP-2575), retrieved 2026-09-16 |
| Every request carries `_meta["io.modelcontextprotocol/protocolVersion"]` | `2026-07-28` | HOLDS | same source, "Every request now carries its protocol version and client capabilities in `_meta` (`io.modelcontextprotocol/protocolVersion`, `io.modelcontextprotocol/clientCapabilities`)", retrieved 2026-09-16 |
| Servers cannot initiate requests or push unsolicited content | `2026-07-28` | HOLDS | same source, item 7: "Multi Round-Trip Requests (MRTR) pattern ... replaces the previous approach of sending server-initiated requests, such as `roots/list`, `sampling/createMessage`, or `elicitation/create`.", retrieved 2026-09-16 |
| Only subscription-gated `list_changed`/`resources/updated`, request-scoped progress, and deprecated logging notifications remain | `2026-07-28` | **DRIFT (narrow) — mechanism renamed, effect preserved** | same source, item 4: the HTTP GET endpoint and `resources/subscribe`/`resources/unsubscribe` are **replaced by `subscriptions/listen`**, a single long-lived stream with opt-in types (`toolsListChanged`, `promptsListChanged`, `resourcesListChanged`, `resourceSubscriptions`), tagged with `io.modelcontextprotocol/subscriptionId`; request-scoped `notifications/progress` is unchanged; `ping`, `logging/setLevel`, and `notifications/roots/list_changed` are **removed** (item 5), and Logging itself is **deprecated** (item, "Deprecated" section, SEP-2577). §3.3's summary is directionally correct (subscription-gated change notifications + request-scoped progress + logging on its way out) but the *method name* `resources/updated`/`resources/subscribe` no longer exists verbatim at `2026-07-28` — it is now `subscriptions/listen` with opt-in change types. See Drift register. |
| No MCP revision defines "external event becomes a user turn" | `2026-07-28` / `2025-11-25` | HOLDS (by omission) | Full changelog for `2026-07-28` reviewed above; no server-push-to-user-turn concept appears in any of the nine major changes. https://modelcontextprotocol.io/specification/2026-07-28/changelog, retrieved 2026-09-16 |
| Extension identifier format `{vendor-prefix}/{extension-name}` | SEP-2133, Final | HOLDS | https://modelcontextprotocol.io/seps/2133-extensions, "Extensions are identified using a unique *extension identifier* with the format: `{vendor-prefix}/{extension-name}`, e.g. `io.modelcontextprotocol/oauth-client-credentials` or `com.example/websocket-transport`.", retrieved 2026-09-16 |
| Negotiation through the `extensions` map | SEP-2133 | HOLDS | https://modelcontextprotocol.io/seps/2133-extensions, `### Negotiation` — `ClientCapabilities.extensions` / `ServerCapabilities.extensions`, worked JSON examples, retrieved 2026-09-16 |
| Breaking changes need a new identifier | SEP-2133 | HOLDS | https://modelcontextprotocol.io/seps/2133-extensions, "Breaking changes MUST use a new identifier, e.g. `io.modelcontextprotocol/oauth-client-credentials-v2`.", retrieved 2026-09-16 |
| `experimental` capabilities still exist | `2026-07-28` | HOLDS | Confirmed indirectly: the Claude Channels capability itself is declared under `capabilities.experimental` per §3.1 evidence above, on a client speaking this MCP era range; `experimental` remains a valid capabilities key. Retrieved 2026-09-16. |
| No SEP or working-group item for agent-to-agent messaging | SEP index | Carried unchanged | Not independently re-searched against the full SEP index this pass; carried from PLANNING-PROMPT.md §3.3 (UNVERIFIED — low priority, re-check against https://modelcontextprotocol.io/community/seps if this becomes load-bearing for a spec decision) |

### Issue #12 SEP-2133 carry-overs (PINS.md "Open questions carried into B2")

**(a) SEP-2133 finalization date.** PLANNING-PROMPT.md §3.3 states "final 2026-01-26". The
SEP-2133 page itself states only `Created: 2025-01-21` and a "Final" badge, with no
separate finalization-date field (confirmed again this pass:
https://modelcontextprotocol.io/seps/2133-extensions, retrieved 2026-09-16). Per the task
instruction, located the merge date of the SEP's own PR directly:

```
GET https://api.github.com/repos/modelcontextprotocol/modelcontextprotocol/pulls/2133
"merged_at": "2026-01-26T23:57:49Z"
```

Retrieved 2026-09-16.

**Verdict: CLOSED — no drift.** PLANNING-PROMPT.md §3.3's "final 2026-01-26" is confirmed
by the PR merge date `2026-01-26T23:57:49Z` (UTC), which is the first-party finalization
event for this SEP (a SEP becomes Final when its PR merges). The SEP page's own `Created`
field (`2025-01-21`) is a different field — the date the SEP was opened, not finalized —
and both are now correctly distinguished in this ledger. **Skill/doc action:** none needed;
§3.3's date was correct, it was the *source page's own field label* that was ambiguous.
Closed.

**(b) Reserved-prefix rule for prefixes whose second label is `modelcontextprotocol` or
`mcp`.** Re-read the full SEP-2133 text (quoted in full in the WebFetch above, retrieved
2026-09-16). The only prefix-related normative clause is: "To prevent identifier
collisions, the vendor prefix SHOULD be a reversed domain name that the extension author
owns or controls (similar to Java package naming conventions)." and "Official extensions
use the `io.modelcontextprotocol` vendor prefix." **No clause reserves every prefix whose
second label is `modelcontextprotocol` or `mcp`** — the SEP only says official extensions
*use* `io.modelcontextprotocol`; it does not forbid third parties from using any other
`*.modelcontextprotocol` or `*.mcp` second-label prefix.

**Verdict: DRIFT in §3.3 — the reserved-prefix rule as stated in PLANNING-PROMPT.md §3.3
does not exist in the SEP-2133 text.** See Drift register below. `oac-spec-authoring` must
stop relying on this rule as stated and instead cite only the domain-ownership SHOULD
clause actually present.

**(c) Terminology note (not drift, already decided in PINS.md).** The SEP page uses
"vendor-prefix"; §3.3 uses "reverse-dns-prefix". Both describe the same reversed-domain
convention. Carried forward as a terminology pointer only, per PINS.md.

## §3.4 Zenoh

Re-fetched `DEFAULT_CONFIG.json5` at tag `1.10.1` directly
(`raw.githubusercontent.com/eclipse-zenoh/zenoh/1.10.1/DEFAULT_CONFIG.json5`).

| Fact | Pin checked against | Verdict | Source |
|---|---|---|---|
| Dual EPL-2.0 / Apache-2.0 | `1.10.1` | Carried unchanged | PINS.md Zenoh pin record, unchanged, retrieved 2026-09-16 (per PLANNING-PROMPT.md §3.4, not re-fetched from a LICENSE file this pass) |
| Stable API is `zenoh` + `zenoh-ext` only | `1.10.1` | Carried unchanged | PLANNING-PROMPT.md §3.4 |
| Binding matrix incl. `zenoh-ts` not native, needs `zenohd` + remote-api plugin | `1.10.1` | Carried unchanged | PLANNING-PROMPT.md §3.4 (not re-fetched this pass) |
| Peer mode default | `1.10.1` | HOLDS | `DEFAULT_CONFIG.json5` at tag `1.10.1`, line 12: `mode: "peer",`, retrieved 2026-09-16 |
| UDP multicast scouting on `224.0.0.224:7446` with `interface: "auto"` | `1.10.1` | HOLDS | `DEFAULT_CONFIG.json5` at tag `1.10.1`: `address: "224.0.0.224:7446",` and `interface: "auto", // If not set or set to "auto" the interface is picked automatically`, retrieved 2026-09-16 |
| Gossip | `1.10.1` | HOLDS | `DEFAULT_CONFIG.json5` at tag `1.10.1`, `gossip: { ... }` block present and enabled by default, retrieved 2026-09-16 |
| Loopback fix in 1.10.0 via PR #2671 | `1.10.1` | Carried unchanged (constraint floor, PINS.md) | https://github.com/eclipse-zenoh/zenoh/pull/2671, per PINS.md, unchanged, retrieved 2026-09-16 |
| Windows binds scouting to `0.0.0.0` with `SO_REUSEADDR`; `#iface=` documented Linux-only; dynamic TCP listen ports; liveliness tokens with history-capable subscribers; storage/plugins only inside `zenohd`; advanced pub/sub in `zenoh-ext` behind `unstable`; TLS/mTLS and QUIC per-endpoint certs; ACL subjects (interface, cert CN, username, link protocol, `zid`); `zid` unauthenticated/unfit for production; no application-layer message signing | `1.10.1` | Carried unchanged | PLANNING-PROMPT.md §3.4 (not independently re-fetched against `zenoh.io/docs/manual/{access-control,tls,quic,user-password,configuration}/` this pass — no acceptance box touches these rows; `DEFAULT_CONFIG.json5`'s `auth:` block, reviewed in full at tag `1.10.1`, is consistent with the ACL-subjects and no-payload-signing claims: it defines `usrpwd` and `pubkey` sub-blocks and nothing resembling per-message signing) |

### Issue #12 acceptance box 6 — Zenoh public-key auth semantics

Fetched `DEFAULT_CONFIG.json5` at tag `1.10.1` in full and located the exact block:

```
auth: {
  usrpwd: {
    user: null,
    password: null,
    dictionary_file: null,
  },
  pubkey: {
    public_key_pem: null,
    private_key_pem: null,
    public_key_file: null,
    private_key_file: null,
    key_size: null,
    known_keys_file: null,
  },
},
```

Source: https://raw.githubusercontent.com/eclipse-zenoh/zenoh/1.10.1/DEFAULT_CONFIG.json5,
tag `1.10.1`, retrieved 2026-09-16. Every key is quoted verbatim above.

**This is a two-row close, per the task instruction not to collapse it into one:**

- **Row 1 — key names: CLOSED.** The six `auth.pubkey` key names
  (`public_key_pem`, `private_key_pem`, `public_key_file`, `private_key_file`,
  `key_size`, `known_keys_file`) are confirmed verbatim in the pinned config schema.
- **Row 2 — semantics: UNVERIFIED, carried.** `DEFAULT_CONFIG.json5` provides no inline
  documentation comment for the `pubkey` block (unlike `usrpwd`, which has "A password
  implies a username is required." and a dictionary-file comment). No accompanying
  semantics (how `known_keys_file` is matched against a connecting peer's identity, what
  `key_size` constrains, whether pubkey auth is enforced at session-open or per-message)
  were found in the config file text. The manual pages under `zenoh.io/docs/manual/`
  were not independently re-fetched this pass for this specific sub-feature (out of
  scope: not needed to resolve the key-names half of the question, and public-key auth
  is not on OAC's v0.1 critical path per DESIGN.md's current transport security posture).
  Carried to `11-risks.md`.

### Issue #12 acceptance box 7 — binary size estimate

Per the task instruction, B2 does not build anything. Searched for a first-party published
Zenoh artifact size (release binary size on the GitHub Releases page for tag `1.10.1`) —
not confirmed in this session (release asset sizes were not enumerated by the fetches
performed here).

**Verdict: UNVERIFIED — labelled, not deleted, not replaced with a guess.** Reason
recorded verbatim per the task instruction: "derived estimate; resolved by the first G3
build artifact, task D3." Carried to `11-risks.md` unchanged.

## §3.5 ACP

| Fact | Pin checked against | Verdict | Source |
|---|---|---|---|
| Protocol version `1` | ACP protocol v1 | Carried unchanged | https://agentclientprotocol.com/protocol/, retrieved 2026-09-16 — page paths are all `/protocol/v1/...`; no literal `protocolVersion` field appears in the page's own text (same finding as B1; not newly closed) |
| Schema v2 alpha | ACP protocol v1 / schema v2 | **UNVERIFIED — carried, not independently re-confirmed** | https://agentclientprotocol.com/protocol/, retrieved 2026-09-16, reviewed in full: no "alpha" or "v2" schema-status language appears on this page. Carried from PLANNING-PROMPT.md §3.5 only. Low priority — ACP is not a v0.1 dependency. |
| C7 framing: ACP is client-owned-session, not a channel | ACP protocol v1 | HOLDS | Confirmed by the protocol page's own architecture description (client drives the session; nothing on the page describes server-initiated push into an already-open client session the way Claude Channels or a Codex daemon attach does). https://agentclientprotocol.com/protocol/, retrieved 2026-09-16. This framing still holds and A2 may continue to depend on it. |

ACP is not a v0.1 dependency; neither open item here blocks a gate.

## Drift register

| # | Old value (verbatim) | New value (verbatim/quoted) | Source | Invalidates a decision? |
|---|---|---|---|---|
| D1 | §3.3: "only subscription-gated `list_changed`/`resources/updated` ... notifications remain" | `2026-07-28` changelog item 4: `resources/subscribe`/`resources/unsubscribe` and the HTTP GET endpoint are **removed and replaced** by `subscriptions/listen`, a single long-lived stream with opt-in types (`toolsListChanged`, `promptsListChanged`, `resourcesListChanged`, `resourceSubscriptions`) tagged with `io.modelcontextprotocol/subscriptionId`; `ping`, `logging/setLevel`, `notifications/roots/list_changed` removed; Logging deprecated (SEP-2577) | https://modelcontextprotocol.io/specification/2026-07-28/changelog, retrieved 2026-09-16 | No ADR-001/DESIGN/§5 decision named `resources/updated` or `resources/subscribe` by method name in this repo's decisions; this is a §3 prose correction, not a design invalidation. `oac-spec-authoring` and any future G4 fixture work must use `subscriptions/listen`, not `resources/subscribe`, when targeting `2026-07-28`. No ADR amendment needed — flagged here so G4 fixture authors (Epic D) don't build against a removed method name. |
| D2 | §3.3: "Prefixes whose second label is `modelcontextprotocol` or `mcp` are reserved." | No such clause exists in SEP-2133. The only normative clause is: "the vendor prefix SHOULD be a reversed domain name that the extension author owns or controls" and "Official extensions use the `io.modelcontextprotocol` vendor prefix." | https://modelcontextprotocol.io/seps/2133-extensions, retrieved 2026-09-16 | Yes, narrowly: `oac-spec-authoring` (Epic E, not yet started) must not state or enforce a blanket reservation rule for any `*.modelcontextprotocol` / `*.mcp` second label. This is flagged rather than redesigned around, per `oac-evidence` §6 — no §5 decision currently cites this rule, so no numbered ADR amendment is required at this time. If a future spec-authoring task needs a reserved-prefix rule for OAC's own extension identifiers, it must derive one from the domain-ownership SHOULD clause actually present, not from the unconfirmed §3.3 sentence. |

Neither drift item touches ADR-001, DESIGN.md, or a §5 decision directly (checked: neither
`resources/updated` nor the reserved-prefix rule is named in ADR-001.md or DESIGN.md), so
no numbered `ADR-001-A*` amendment is proposed here.

## Closed UNVERIFIED items

Removed from `STATUS.md`'s "Open UNVERIFIED items" list in this change:

1. Agent SDK support for Channels — **CLOSED, negative** (§3.1 box 3 above).
2. SEP-2133 finalization date (2026-01-26) — **CLOSED, no drift**, confirmed via PR
   `#2133` `merged_at: 2026-01-26T23:57:49Z` (§3.3 carry-over (a) above).
3. Zenoh `auth.pubkey` config key names — **CLOSED** (key names only; semantics remain
   open, see Carried list below) (§3.4 box 6 above).
4. Issue #21743 status check — **CLOSED, no drift**: confirmed still open
   (`"state": "open"`, `"closed_at": null`) at the pinned retrieval date; the "no attach"
   premise is unaffected.
5. Floor 2 permission-relay pin (`>= v2.1.234`) satisfaction at `v2.1.274` — already closed
   in B1's PINS.md pin record; independently re-confirmed here via the same
   `channels-reference.md` "Claude Code v2.1.234 and later..." sentence, retrieved fresh
   2026-09-16. No change to PINS.md needed (not a new close, re-stated for completeness).

## Carried to 11-risks.md

Every item below stays UNVERIFIED and is not resolvable by B2. Each states its owner.

1. **Claude channel behaviour across `--resume`/`--continue`** (issue #12 box 1) —
   UNVERIFIED, docs silent at `v2.1.274`. Consequence: OAC's session-identity model may
   need to re-launch rather than re-attach a Claude session to guarantee channel
   delivery. Owner: C-series decision + D1.
2. **Whether one MCP server can present more than one logical channel** (box 2) —
   UNVERIFIED, docs silent at `v2.1.274`. Owner: D1, or a direct question to Anthropic via
   the channels GitHub issue tracker.
3. **Whether implicit Codex daemon attach executes by default at runtime in the pinned
   build** (box 4) — source-code path confirmed at the pinned commit (§3.2 above);
   runtime behaviour UNVERIFIED. Owner: D2 (G2 spike). B2 does not declare G2 passed.
4. **Codex Desktop control-socket exposure in current builds** (box 5) — UNVERIFIED, no
   first-party statement found. Security consequence: OAC must not assume a
   Desktop-hosted Codex thread is reachable via the control socket. Owner: D2, re-check
   when OpenAI publishes first-party Codex Desktop documentation.
5. **Zenoh `auth.pubkey` semantics** (box 6, second row) — UNVERIFIED, key names closed,
   behavior undocumented in `DEFAULT_CONFIG.json5` or the manual pages checked. Owner:
   G3 spike / D3, or a direct read of the `zenoh` crate's auth implementation source if
   this becomes load-bearing before G3.
6. **Binary size estimate (5-15 MB)** (box 7) — UNVERIFIED, derived estimate. Owner: D3,
   resolved by the first G3 build artifact.
7. **ACP schema v2 "alpha" status** — UNVERIFIED, not independently re-confirmed on
   `agentclientprotocol.com/protocol/`. Low priority, ACP is not a v0.1 dependency. Owner:
   whichever task first depends on ACP forward-compatibility (none currently open).
8. **Zenoh crate version/date via crates.io directly** — UNVERIFIED, crates.io did not
   return page content when checked in B1; not re-attempted in B2 (GitHub Releases is
   first-party for the same project and is not expected to disagree). Owner: any future
   task that needs the crates.io page specifically (e.g. Stage 6 license inventory).
9. **The named compatibility shim boundary for the Claude Code Channels research-preview
   surface** — UNVERIFIED. DESIGN.md names no such module (`shim boundary: UNNAMED — see
   DESIGN.md`). **Owner: a C-series decision or a DESIGN.md update, before A2 can satisfy
   its "Preview and experimental rows name the compatibility shim boundary" acceptance
   box.** Out of scope for B2 per the task's explicit instruction — B2 cannot name a
   module DESIGN.md does not define.
10. **The named compatibility shim boundary for the Codex experimental live-inject
    surface** — UNVERIFIED, same reason and same owner as item 9. Out of scope for B2.
11. **`codex mcp-server` deprecation date (2026-08-20) and deletion date (2026-09-05)** —
    carried unchanged from PLANNING-PROMPT.md §3.2, not independently re-confirmed this
    pass against `learn.chatgpt.com/docs/app-server` or the CLI reference. Owner: next
    task that touches the Codex CLI surface directly (D2 or an Epic F adapter task).
12. **No SEP or working-group item for agent-to-agent messaging** — carried unchanged
    from PLANNING-PROMPT.md §3.3, not independently re-searched against the SEP index
    this pass. Owner: `oac-spec-authoring` (Epic E), if and when it needs to cite this
    absence directly.

## Not re-checkable without a gate

These §3 facts require running code, not reading documentation, and are explicitly left
to their owning gate/task rather than guessed at here:

- Codex implicit daemon attach **runtime** behaviour (item 3 above) — D2 / G2.
- Zenoh loopback discovery working end-to-end on Windows/macOS/Linux at `1.10.1` — D2 /
  G3 (the `DEFAULT_CONFIG.json5` and PR #2671 evidence above establish the *fix exists in
  source*, not that it behaves correctly on every OAC target platform).
- The Zenoh binary size estimate (item 6 above) — D3.
- MCP dual-era server behaviour (one process serving both `2026-07-28` and `2025-11-25`) —
  D2 / G4. §3.3's documentation-level facts are re-verified above; the dual-era
  implementation itself is not exercised by B2.
