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
| **Amendment, 2026-09-28 (issue #39 G1 Box C):** the doc text above ("delivered together on the next turn ... as a group") describes the *documentation*, which this table confirms is unchanged. Live runtime evidence on `v2.1.283` (`docs/planning/gates/G1-result.md` "Re-run attempt 2 (Box C)") shows two mid-turn notifications sent close together can each render at its own tool-call boundary rather than batched together at one — order preserved, nothing dropped or interleaved, but not "together as a group" in the sense the doc text's wording suggests. This is a **runtime-observation vs. doc-text finding**, not a correction to this table's HOLDS verdicts (the doc's own words are unchanged at `v2.1.283`, only re-verified here at `v2.1.274`) — recorded as its own UNVERIFIED-as-guarantee item, see `docs/planning/v0.1/11-risks.md` row 49. | n/a — cross-reference note, not a new source citation |
| No acknowledgement of delivery | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels-reference.md, "Claude Code doesn't acknowledge notifications. The `await` on `mcp.notification()` resolves when the message is written to the transport, not when Claude has processed it.", retrieved 2026-09-16 |
| Outbound via ordinary MCP tools (conventionally `reply`), correlation by convention only | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels-reference.md, `## Expose a reply tool` section defines a standard MCP `reply` tool with no protocol-level correlation field beyond app-chosen args like `chat_id`, retrieved 2026-09-16 |
| `--channels plugin:<name>@<marketplace>` and `--channels server:<name>` | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels-reference.md, `claude --dangerously-load-development-channels plugin:yourplugin@yourmarketplace` and `... server:webhook`, retrieved 2026-09-16 |
| No attach to a running session (channels start with the session, not attached after launch) | `v2.1.274` | HOLDS (by omission — no attach mechanism is documented anywhere in `channels.md` or `channels-reference.md`) | https://code.claude.com/docs/en/channels.md and https://code.claude.com/docs/en/channels-reference.md, both retrieved 2026-09-16, full text reviewed; no `attach` verb or API for joining an already-running session appears |
| Multiple channels per session allowed | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels.md, "You can pass several plugins to `--channels`, space-separated.", retrieved 2026-09-16 |
| Allowlist distribution: `claude-plugins-official`, `allowedChannelPlugins`, `channelsEnabled` | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels.md, `## Enterprise controls` table (`channelsEnabled`, `allowedChannelPlugins`) and "The Anthropic default list applies", retrieved 2026-09-16 |
| `--dangerously-load-development-channels` plus interactive confirmation | `v2.1.274` | HOLDS | https://code.claude.com/docs/en/channels-reference.md, "Claude Code first shows a full-screen warning dialog listing the development channels you're loading. Select **I am using this for local development** to continue, or **Exit** to quit.", retrieved 2026-09-16 |
| Not available on Bedrock, Vertex, Foundry | `v2.1.274` | HOLDS (naming drift — see Drift register D3) | https://code.claude.com/docs/en/channels.md, "not available on Amazon Bedrock, Google Cloud's Agent Platform, or Microsoft Foundry", retrieved 2026-09-16. §3.1's own wording says "Vertex"; the first-party text now reads "Google Cloud's Agent Platform" — "Vertex" no longer appears. Recorded as prose drift, not absorbed silently. |
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

### §3.1 re-check at Claude Code `2.1.285` (2026-10-02, #122)

Trigger: issue #122 item 4, and the operator decision on #122 (2026-10-02): the §3.1
facts get **one** B2-style desk re-check at the last tested version, `2.1.285`
(`docs/planning/PINS.md`, Claude Code Channels, "Version policy"); newer versions after
this are version warnings only (#216). This section is the record of that pass. It
closes the "§3.1 not re-verified since `v2.1.274`" gap (`docs/planning/v0.1/11-risks.md`
row 30).

**Method.** Retrieval date for every source below: **2026-10-02**.

- Docs pages, fetched as raw Markdown with `curl` from `https://code.claude.com/docs/en/`:
  `channels.md`, `channels-reference.md`, `mcp.md`, `hooks.md`, `env-vars.md`,
  `cli-reference.md`, `agent-sdk/overview.md`. Line numbers below (`L<n>`) are lines of
  those fetched files.
- Release notes: `CHANGELOG.md` in `anthropics/claude-code` at commit
  `52c76441cae91f6891e4712306bffb057ff6fec5` (2026-10-01T18:43:08Z), cited as
  "changelog `<version>`". `v2.1.285` was published 2026-09-29T19:27:30Z (`gh api
  repos/anthropics/claude-code/releases/tags/v2.1.285`).
- Local binary: `claude --version` printed `2.1.285 (Claude Code)` on the operator's
  Windows host; `claude --help` was run on that binary.

**Version caveat.** The docs pages are not versioned. On the retrieval date the newest
release was `v2.1.287` (2026-10-01T18:00:22Z), so the pages may describe `2.1.286` or
`2.1.287`, not `2.1.285`. The changelog entries for `2.1.286` and `2.1.287` were read for
anything touching channels, `MCP_PROTOCOL_NEGOTIATION`, hook `session_id` or permission
relay. None changes a fact below. (`2.1.287` adds URL elicitation for servers on the
`2025-11-25` protocol, changelog `2.1.287`; it does not touch channel registration.) A
fact is therefore marked HOLDS at `2.1.285` only when the doc text and the changelog
agree. Only the `claude --help` row was observed directly on `2.1.285`.

| # | §3.1 fact | B2 (`v2.1.274`) | `2.1.285` result | Evidence (retrieved 2026-10-02) |
|---|---|---|---|---|
| 1 | `capabilities.experimental["claude/channel"] = {}` required; presence registers the listener | HOLDS | **HOLDS** | `channels-reference.md` L200: "Required. Always `{}`. Presence registers the notification listener." |
| 2 | `notifications/claude/channel` carries `content` (string) + `meta` (string map) | HOLDS | **HOLDS** | `channels-reference.md` L225 ("Your server emits `notifications/claude/channel` with two params"), L230 (`meta`, `Record<string, string>`) |
| 3 | Non-identifier `meta` keys silently dropped | HOLDS | **HOLDS** | `channels-reference.md` L230: "Keys must be identifiers: letters, digits, and underscores only. Keys containing hyphens or other characters are silently dropped." Runtime detail (ASCII-only, G5 at `2.1.283`) unchanged |
| 4 | Idle-session wake as a user turn | HOLDS | **HOLDS** | `channels.md` L13: "A channel is an MCP server that pushes events into your running Claude Code session, so Claude can react to things that happen while you're not at the terminal."; L15: "the event arrives in the session you already have open" |
| 5 | Mid-turn notifications queued, delivered in order | HOLDS | **HOLDS (doc text unchanged)** | `channels-reference.md` L256, same sentence as B2, still "delivered together on the next turn". Runtime per-boundary delivery seen at `2.1.283` stays UNVERIFIED as a guarantee (`11-risks.md` row 49); not re-run here |
| 6 | No acknowledgement | HOLDS | **HOLDS** | `channels-reference.md` L252: "Claude Code doesn't acknowledge notifications. The `await` on `mcp.notification()` resolves when the message is written to the transport, not when Claude has processed it." |
| 7 | Outbound via ordinary MCP tools (`reply`); correlation by convention only | HOLDS | **HOLDS** | `channels-reference.md` L24 ("Expose a reply tool"), L254 ("If you need delivery confirmation, track event state in your server and expose a reply tool") |
| 8 | Loading: `--channels plugin:<name>@<marketplace>` **or** `--channels server:<name>` | HOLDS | **DRIFTED (D4)** | `cli-reference.md` L75: `--channels` takes a "Space-separated list of `plugin:<name>@<marketplace>` entries". L79: `--dangerously-load-development-channels` "Accepts `plugin:<name>@<marketplace>` and `server:<name>` entries". `server:<name>` is documented only on the development flag |
| 9 | No attach to a running session | HOLDS (by omission) | **HOLDS** | `mcp.md` L396: "your server declares the `claude/channel` capability and you opt it in with the `--channels` flag at startup"; no attach mechanism on `channels.md` or `channels-reference.md` |
| 10 | Behavior across `--resume` | UNVERIFIED | **UNVERIFIED — docs still silent** | Neither `channels.md` nor `channels-reference.md` contains `resume` or `--continue`. `env-vars.md` L365 (below) covers only the session-id variable on resume, not channel registration |
| 11 | Multiple channels per session | HOLDS | **HOLDS** | `channels.md` L263: "You can pass several plugins to `--channels`, space-separated."; `cli-reference.md` L75 |
| 12 | One server presenting more than one logical channel | UNVERIFIED | **UNVERIFIED — docs still silent** | `channels-reference.md` L244: the `source` attribute "is set automatically from your server's configured name"; nothing on multiple channels per server |
| 13 | Allowlist: `claude-plugins-official`, `allowedChannelPlugins`, `channelsEnabled` | HOLDS | **HOLDS** | `channels.md` L312-313 (Enterprise controls table), L348 ("The channel plugins in claude-plugins-official ... are the default approved set") |
| 14 | `--dangerously-load-development-channels` plus interactive confirmation | HOLDS | **HOLDS** | `channels-reference.md` L141 ("full-screen warning dialog ... Select **I am using this for local development**"); `cli-reference.md` L79 ("Prompts for confirmation") |
| 15 | Not available on Bedrock, Vertex, Foundry | HOLDS (naming drift D3) | **HOLDS (D3 unchanged)** | `channels.md` L10: "not available on Amazon Bedrock, Google Cloud's Agent Platform, or Microsoft Foundry" |
| 16 | Per-channel sender allowlist bootstrapped by pairing code | HOLDS | **HOLDS** | `channels.md` L284-290 ("Every approved channel plugin maintains a sender allowlist"; "The bot replies with a pairing code") |
| 17 | Permission relay `claude/channel/permission`, `v2.1.234+` | HOLDS | **HOLDS** | `channels-reference.md` L201, L446 ("Claude Code v2.1.234 and later sends permission requests only to servers it registered as channels for the session"); changelog `2.1.234`: "permission previews now relay only to channel servers admitted by the inbound trust gate" |
| 18 | "Research preview on Claude Code v2.1.232+" | UNVERIFIED | **Research preview: HOLDS. `v2.1.232+`: DRIFTED (D6)** | `channels.md` L344: "Channels are a research preview feature." Changelog `2.1.80`: "Added `--channels` (research preview) — allow MCP servers to push messages into your session"; changelog `2.1.81`: "Added `--channels` permission relay". `2.1.232` is not a channels version. Its only first-party mention found is `mcp.md` L324: in sessions that fetch feature flags, Claude Code "uses the v2 runtime on Claude Code v2.1.232 or later". That is the v2 MCP client runtime, which "adds MCP protocol revision 2026-07-28" (L322) and to which row 20's channel-negotiation constraint applies |
| 19 | `--channels` flags absent from `claude --help` | HOLDS | **HOLDS, observed on `2.1.285`** | `claude --help` on `2.1.285`: no line contains `channel`. `channels.md` L346: "Neither `--channels` nor `--dangerously-load-development-channels` appears in `claude --help` while the feature is in preview." (Both flags are now listed on the `cli-reference.md` docs page, L75/L79. That page is not `--help`) |
| 20 | `2026-07-28` channel servers cannot deliver; negotiate legacy, `MCP_PROTOCOL_NEGOTIATION=legacy` | HOLDS | **HOLDS, wording narrower** | `mcp.md` L398: "On the v2 runtime, if you set `MCP_PROTOCOL_NEGOTIATION` to `auto` and a channel server negotiates MCP protocol revision 2026-07-28, it can't deliver channel messages, so Claude Code doesn't register it as a channel. Leaving the variable unset, or setting it to `legacy`, keeps stdio servers on the earlier handshake." The literal "`2025-11-25` or earlier" is §3.1's paraphrase. No page fetched states that revision string for channels; G1 observed `protocolVersion: "2025-11-25"` at runtime (`docs/planning/gates/G1-result.md`) |
| 21 | `session_id` in hook input is the supported path; no `CLAUDE_SESSION_ID` env var | HOLDS | **Literal fact HOLDS; "the supported way" DRIFTED (D5)** | `hooks.md` L726: `session_id`, "Current session identifier". No `CLAUDE_SESSION_ID` on `hooks.md` or `env-vars.md`. But `env-vars.md` L365 documents **`CLAUDE_CODE_SESSION_ID`**: "Set automatically to the current session ID in Bash and PowerShell tool subprocesses, hook command subprocesses, and stdio MCP server subprocesses." It is not new. Changelog entries well before `2.1.200` add it to the Bash tool and then to stdio MCP server subprocesses. B2 searched only `hooks.md` for the `CLAUDE_SESSION_ID` spelling and missed it |
| 22 | Cross-session messaging (`ListAgents`/`SendMessage`) is separate from Channels | not tabled | **HOLDS (by omission)** | Neither `channels.md` nor `channels-reference.md` mentions `SendMessage` or `ListAgents`; the changelog treats them as their own feature (e.g. changelog `2.1.248`: "Added cross-session messaging (`SendMessage` / `ListAgents`) between sessions on the same machine on Bedrock, Vertex, and Foundry", which is unrelated to channels) |
| 23 | Agent SDK does not support Channels | CLOSED (negative) | **HOLDS** | `agent-sdk/overview.md` L24-37, the `## Capabilities` table, still omits channels; the page contains no `channel` string |

**Drift found** (added to the Drift register below as D4-D6):

- **D4, `--channels server:<name>`.** §3.1 lists `--channels server:<name>` as a loading
  form. `cli-reference.md` gives `--channels` `plugin:` entries only; `server:<name>` is
  documented only on `--dangerously-load-development-channels`. B2's HOLDS row cited
  development-flag examples for it. This is no design change:
  `docs/planning/decisions/C2-process-model.md` §6 and
  `docs/planning/v0.1/08-cli-and-deployment.md` §7 already launch OAC with
  `--dangerously-load-development-channels server:oac` and do not use `--channels`.
- **D5, `CLAUDE_CODE_SESSION_ID`.** A documented environment variable gives the session id
  to stdio MCP server subprocesses, which includes `oac mcp-shim`. `env-vars.md` L365
  adds caveats. An MCP server "retains the ID it was spawned with". On
  `--resume <session-id>` it "receives the resumed ID". On `--continue`, or `--resume`
  without an ID, "it may receive the initial startup ID instead". This conflicts with
  `docs/planning/decisions/C4-session-identity.md` §3, "the hook's own stdin/HTTP body is
  the only supported surface". C4's hook path still works and is still documented, so
  nothing breaks. Per `oac-evidence` §6 this is recorded as a conflict, not redesigned
  here. It is filed in `docs/planning/STATUS.md` "Open conflicts (oac-evidence §6)". It is
  not an ADR-001 conflict, so no `ADR-001-A*` amendment is proposed.
- **D6, the `v2.1.232` floor.** The first-party changelog places "Added `--channels`
  (research preview)" at `2.1.80`, not `2.1.232`. `2.1.232` is not a channels version.
  The only first-party mention of it found is `mcp.md` L324: in sessions that fetch
  feature flags, Claude Code "uses the v2 runtime on Claude Code v2.1.232 or later". That
  is the v2 MCP client runtime, which adds MCP revision `2026-07-28` (L322). The L398
  channel-negotiation constraint (row 20) applies to that runtime. This closes the open
  floor-1 item as drift: the `>= v2.1.232` "channels exist at all" floor in
  `docs/planning/PINS.md` is not supported. The `2.1.232` boundary is real, but for the
  v2 runtime, not for channels. The project's
  operative floor has been the minimum version `v2.1.282` since #216, and that sits above
  both versions. No decision changes.

**Still UNVERIFIED after this pass:** behavior across `--resume`/`--continue` (row 10);
one server presenting more than one logical channel (row 12); mid-turn batching as a
guarantee (row 5, `11-risks.md` row 49). Each stays in `docs/planning/STATUS.md` "Open
UNVERIFIED items".

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

### §3.2 re-verification at Codex `0.157.1` (floating-pin trigger, 2026-09-26)

Trigger: `docs/planning/PINS.md`'s Codex CLI / app-server row became **floating** by
operator decision on 2026-09-26, with last-observed version `@openai/codex@0.157.1`
(commit `36650394c5b38c2990ccf2a3457165ca3e9d9726`), per `oac-evidence` §7's "a pin
moves" trigger. This re-checks every §3.2 fact the B1/B2 ledger above verified against
the prior pin, `@openai/codex@0.154.0` (commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`),
against the newly observed `0.157.1`. Sources: `gh api
repos/openai/codex/contents/<path>?ref=36650394c5b38c2990ccf2a3457165ca3e9d9726` (raw
reads of the pinned commit's source tree) and the installed `0.157.1` binary's own
`--version`/`--help`/`generate-json-schema` output, all retrieved 2026-09-26. This is a
Codex-only re-check — the Claude, MCP, Zenoh and ACP sections above are unaffected by
this trigger and are not re-run here. **This section is the authoritative record of this
re-verification** — `docs/planning/gates/G2-result.md` cites this heading by name rather
than restating the fact table or citations.

| # | Fact | `0.154.0` status | `0.157.1` result | Evidence |
|---|---|---|---|---|
| 1 | Control socket path `CODEX_HOME/app-server-control/app-server-control.sock`, WebSocket-over-UDS | HOLDS | **HOLDS** | `codex-rs/app-server-transport/src/transport/mod.rs`@`36650394`; `unix_socket.rs`@`36650394` line 168 — see bullet below for the one naming drift |
| 2 | `codex app-server proxy` is a raw stdio↔UDS byte relay | HOLDS | **HOLDS** | `codex-rs/cli/src/main.rs`@`36650394` lines 1390/1794; `codex app-server --help` |
| 3 | Implicit daemon attach path present in source | HOLDS | **HOLDS, source-level; new opt-out flag** | daemon README unchanged verbatim; `codex-rs/tui/src/cli.rs`@`36650394` line 85 adds `--no-daemon` (absent at `6b9826e3`) — see bullet below |
| 4 | `thread/queue/add` absent from default schema, present with `--experimental`; same required params | HOLDS | **HOLDS, with one addition** | local `generate-json-schema` runs (with/without `--experimental`); `session_queue_commands.rs` now also rejects `--no-daemon` — see bullet below |
| 5 | G2's method/event name set (`thread/*`, `turn/*`) | HOLDS | **HOLDS** | `ClientRequest.json`/`ServerNotification.json`@`36650394` |
| 6 | Windows control-socket protection (DACL check, `ensure_non_elevated_peer`, non-Unix no-op) | HOLDS | **HOLDS, byte-identical** | `windows_socket_validation.rs`/`windows_peer.rs` fetched and diffed directly at both commits |
| 7 | `codex mcp-server` absent; `codex mcp add --url` streamable HTTP | HOLDS | **HOLDS** | local `--help` output |
| 8 | Codex's default MCP client era (`2025-06-18` only) | HOLDS (no dual-era client) | **HOLDS by default; opt-in `2026-07-28` mode now in source** | `codex-rs/rmcp-client/src/protocol_mode.rs`@`36650394`; `codex-rs/features/src/lib.rs`@`36650394` — see bullet below |
| 9 | `codex-rs/tui/src/session_queue_commands.rs` behavior | N/A | **Part of additive drift (a), `--no-daemon`** — file changed, not just "absent from diff" | blob `sha` `485c8077a5...` (`6b9826e3`) vs. `9752a42817...` (`36650394`), diffed directly — see "Note on the GitHub compare API's 300-file cap" below |

- **Control socket path and WebSocket-over-UDS transport.** HOLDS, unchanged. The daemon
  still binds `CODEX_HOME/app-server-control/app-server-control.sock`
  (`codex-rs/app-server-transport/src/transport/mod.rs`@`36650394`, constants
  `APP_SERVER_CONTROL_SOCKET_DIR_NAME`/`APP_SERVER_CONTROL_SOCKET_FILE_NAME`), and the
  handshake is still an HTTP Upgrade to WebSocket over that socket
  (`codex-rs/app-server-transport/src/transport/unix_socket.rs`@`36650394` line 168).
  **Drift, behavior-preserving:** the call is now named `accept_hdr_async_with_config`
  (was `accept_hdr_async` at `0.154.0`) and additionally advertises an
  `x-codex-websocket-max-unfragmented-message-bytes` response header. The client-side
  contract G2/`oac-codex-appserver` rely on — do the Upgrade yourself, then speak
  JSON-RPC frames — is unaffected.
- **`codex app-server proxy` byte-relay behavior.** HOLDS, unchanged. Still
  `codex_stdio_to_uds::run(socket_path.as_path())`
  (`codex-rs/cli/src/main.rs`@`36650394` lines 1390/1794); `codex app-server --help` still
  reads "Proxy stdio bytes to the running app-server control socket".
- **Implicit daemon attach (attach-or-embed branch).** HOLDS at the **source level**.
  The daemon README changed substantially between pins overall (a new `codex app-server
  daemon update` command, a new `settings.json` updater-preference format, and rewritten
  `bootstrap`/package-path semantics) — the whole file was diffed at both commits; the
  attach-or-embed paragraph is byte-identical: "An invocation that sets
  `CODEX_EXEC_SERVER_URL` skips implicit daemon attachment... If an implicitly
  discovered daemon cannot initialize the connection, the TUI starts an embedded server
  instead." Runtime
  confirmation at `0.157.1` is recorded separately in `docs/planning/gates/G2-result.md`
  (the G2 re-run), not here — this ledger re-verifies source-level facts only, per its
  own "Not re-checkable without a gate" convention below.
  **Drift, new: an explicit `--no-daemon` opt-out flag now exists.**
  `codex-rs/tui/src/cli.rs`@`36650394` line 85: `#[arg(long)] pub no_daemon: bool,`
  documented "Run without the shared background server, even if it is already running."
  `grep -n "no_daemon\|no-daemon"` against the same file fetched at `6b9826e3` (0.154.0)
  returns no match — the flag is absent there. `codex-rs/cli/src/main.rs`@`36650394` line
  2383's `run_interactive_tui` checks `interactive.no_daemon` before ever attempting
  implicit attach; lines 2383-2387 reject `--no-daemon` together with `codex agents`
  ("--no-daemon cannot be used with codex agents. The agents overview requires a shared
  server..."). (Line 336 merely declares a separate, hidden `no_daemon` field on
  `AgentsCommand` itself — it is not where the rejection runs.)
  **Consequence:** G2's own runs never passed `--no-daemon` (plain `codex`, no flags), so
  this does not change either verdict; it is recorded because a future spike or adapter
  invocation that copies a flag from an example must not accidentally pass `--no-daemon`
  and then misread the resulting embedded-server behavior as an attach failure. Both
  `codex-rs/tui/src/cli.rs` and `codex-rs/cli/src/main.rs` fall outside the 300-file cap
  on the `rust-v0.154.0...rust-v0.157.1` compare (see note below), so this drift was only
  found by fetching and diffing the specific files directly, not by trusting "absent from
  the diff."
- **`thread/queue/add` shape and experimental gating.** HOLDS, unchanged, **with one
  addition**: `codex-rs/tui/src/session_queue_commands.rs`@`36650394` now rejects
  `codex queue` when `--no-daemon` was passed: "`--no-daemon` cannot be used with `codex
  queue`. Queuing must discover the shared server to avoid writing through a separate
  server." — absent from the same file at `6b9826e3`. Confirmed by fetching both blobs
  directly (`sha` `485c8077a518bad9ce7445ee1a15904369a8c2dc` at `6b9826e3` vs.
  `9752a4281737248ab8c4d0c7a0718ef7f11d5e0b` at `36650394`) and diffing their content —
  **this file is a case in point for the note below: it changed between pins despite not
  appearing in the compare's file list.** Absent from the default schema (`codex
  app-server generate-json-schema --out <dir>` produced no `"thread/queue/add"` match);
  present, with `capabilities.experimentalApi` required, only via `--experimental`
  (`v2/ThreadQueueAddParams.json`'s `required` array is exactly `["clientUserMessageId",
  "input", "threadId"]`, same as `0.154.0`).
- **Note on the GitHub compare API's 300-file cap.** `gh api
  repos/openai/codex/compare/rust-v0.154.0...rust-v0.157.1 --jq '{total_commits, files}'`
  reports `total_commits: 869` and exactly `300` files in `.files` — GitHub's compare
  endpoint caps the `files` array at 300 and does not indicate truncation in this
  response shape. **A file's absence from this diff is therefore not evidence that the
  file is unchanged** between the two commits; the only two facts in this ledger that
  used "absent from the diff" as their reasoning have been corrected above (the
  `session_queue_commands.rs` "unchanged" claim was wrong — it changed — and the
  `--no-daemon` flag itself was found only by fetching `cli.rs`/`main.rs` directly, never
  by reading the diff). No other claim in this section relies on diff-list absence
  either: most facts (1, 2, 5, 8) were read at `36650394` and compared against the
  `0.154.0` ledger's recorded value, not diffed byte-for-byte; facts 4 and 7 come from
  local `0.157.1` `--help`/`generate-json-schema` output, not from source reads at all;
  and only six files were actually fetched and diffed directly at both commits:
  `windows_socket_validation.rs`, `windows_peer.rs`, `session_queue_commands.rs`,
  `codex-rs/tui/src/cli.rs`, `codex-rs/cli/src/main.rs`, and
  `codex-rs/app-server-daemon/README.md`.
- **G2's method/event name set** (`initialize`, `thread/list`, `thread/loaded/list`,
  `thread/resume`, `thread/turns/list`, `turn/interrupt`, `turn/start`, `turn/steer`;
  `turn/started`, `turn/completed`, `item/started`, `item/completed`,
  `item/agentMessage/delta`, `thread/status/changed`). HOLDS, unchanged — all present
  verbatim in `codex-rs/app-server-protocol/schema/json/ClientRequest.json` and
  `ServerNotification.json`@`36650394`.
- **Windows control-socket protection** (`set_control_socket_permissions` no-op on
  non-Unix; `windows_socket_validation.rs`'s DACL/ownership check;
  `windows_peer.rs`'s `ensure_non_elevated_peer`). HOLDS, **byte-identical**:
  `windows_socket_validation.rs` and `windows_peer.rs` were each fetched directly at both
  `6b9826e3` and `36650394` and diffed byte-for-byte, zero differences — this claim rests
  on that direct fetch-and-diff alone, not on either file's presence or absence in the
  (capped) compare list above.
- **`codex mcp-server` absence; `codex mcp add --url` streamable HTTP.** HOLDS, unchanged.
  `codex mcp-server --help` still falls through to top-level help; `codex mcp add --help`
  still shows `--url <URL>  URL for a streamable HTTP MCP server`.
- **Drift, new and material: an opt-in MCP `2026-07-28` client mode now exists in
  source.** `codex-rs/rmcp-client/src/protocol_mode.rs`@`36650394` defines
  `McpProtocolMode` with `#[default] Legacy` (→ `ProtocolVersion::V_2025_06_18`, the same
  default `oac-mcp`/G2/G4 planning assumed) and a second variant `V20260728` (→
  `ProtocolVersion::V_2026_07_28`). It is gated behind feature flags
  `codex-rs/features/src/lib.rs`@`36650394`: `Feature::Mcp20260728`
  (`key: "mcp_2026_07_28"`, `stage: UnderDevelopment`, `default_enabled: false`) and
  `Feature::CodexAppsMcp20260728` (same stage/default). Stdio additionally requires
  `CODEX_MCP_PROTOCOL_VERSION=2026-07-28`. A new test suite exists alongside it
  (`codex-rs/rmcp-client/tests/mcp_2026_*.rs`, six files) that was not present in the
  `0.154.0` tree. **Consequence:** the default-configuration fact ("Codex's MCP client
  speaks only `2025-06-18`") still HOLDS, but OAC's dual-era design (`oac-mcp`, G4)
  should note this opt-in path exists before its next run. Per `oac-evidence` §6, this is
  flagged rather than treated as invalidating a decision, since no ADR-001/DESIGN.md text
  currently names Codex's MCP client version by number.
- **Additive, out of scope:** a new `v2` schema directory
  (`codex-rs/app-server-protocol/schema/json/v2/`) and a larger CLI surface (`agents`,
  `resume`, `archive`, `delete`, `migrate-rollouts`, `unarchive`, `fork`, `cloud`
  (experimental), `exec-server` (experimental), `doctor`, `sandbox`, `debug`, `features`)
  were observed. None of these are named by any current §3.2 fact or G2 pass criterion;
  recorded only so a future task does not mistake them for undocumented surfaces.

**Verdict: no drift affecting any fact G2's PASS rested on.** Two additive drifts were
found, corrected during review from an earlier, wrong "absent from the diff" reading (the
compare list is capped at 300 of 869 changed files):
(a) a new **`--no-daemon`** opt-out flag (`codex-rs/tui/src/cli.rs`,
`codex-rs/cli/src/main.rs`), which includes a matching rejection added to
`codex-rs/tui/src/session_queue_commands.rs` for `codex queue`; and
(b) the opt-in **`mcp_2026_07_28`** MCP client mode. Neither is reachable from the capped
compare, both are off by default, and G2's own runs never exercised either (plain
`codex`, no flags), so neither changes the G2 verdict. One **behavior-preserving** rename
was also found: the WebSocket-upgrade call `accept_hdr_async` became
`accept_hdr_async_with_config`. This closes the STATUS.md/`11-risks.md` row-40 item
"Codex §3.2 facts not re-verified at the observed `0.157.1`" at the **source level**;
runtime confirmation on Windows is recorded separately by the G2 re-run
(`docs/planning/gates/G2-result.md`).

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
| `experimental` capabilities still exist | `2026-07-28` | **UNVERIFIED — carried, not independently confirmed at this era** | No first-party `2026-07-28` schema or changelog text was fetched stating `capabilities.experimental` still exists at this revision. The §3.1 evidence cited previously (Claude Channels declaring `capabilities.experimental["claude/channel"]`) is Claude Code's own client-capability surface, not a citation of the `2026-07-28` MCP schema itself, and Claude Code does not register a channel server negotiating `2026-07-28` (per §3.1 `MCP_PROTOCOL_NEGOTIATION` row) — so that evidence does not answer whether `experimental` survives at the current revision. This is exactly the G1/G4 dual-era question. Re-labelled UNVERIFIED per `oac-evidence` §1/§2; needs a direct fetch of the `2026-07-28` schema (`modelcontextprotocol.io/specification/2026-07-28/schema`) before it can HOLD. Carried to `11-risks.md`. |
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
| D3 | §3.1: "Not available on Bedrock, Vertex, Foundry" | `channels.md` now reads "not available on Amazon Bedrock, Google Cloud's Agent Platform, or Microsoft Foundry" — "Vertex" no longer appears | https://code.claude.com/docs/en/channels.md, retrieved 2026-09-16 | No: this is a surface-provider renaming in Anthropic's own prose, not a design or decision naming "Vertex" by name in ADR-001.md or DESIGN.md (checked: neither does). §3.1 prose and any future skill text citing this exclusion list must say "Google Cloud's Agent Platform," not "Vertex." |

| D4 | §3.1: "channels are passed at session start with `--channels plugin:<name>@<marketplace>` or `--channels server:<name>`" | `cli-reference.md` L75: `--channels` takes "Space-separated list of `plugin:<name>@<marketplace>` entries"; L79: `--dangerously-load-development-channels` "Accepts `plugin:<name>@<marketplace>` and `server:<name>` entries" | https://code.claude.com/docs/en/cli-reference.md, retrieved 2026-10-02 (§3.1 re-check at `2.1.285`, row 8) | No. OAC already launches with the development flag (`docs/planning/decisions/C2-process-model.md` §6, `docs/planning/v0.1/08-cli-and-deployment.md` §7). Skill and prose that list `--channels server:<name>` are corrected in the same change (#122) |
| D5 | §3.1: "hooks receive `session_id` in their input ..., which is the supported way for an external process to learn which live session it is talking to. No `CLAUDE_SESSION_ID` environment variable is documented" | The `CLAUDE_SESSION_ID` spelling is still absent, but `env-vars.md` L365 documents `CLAUDE_CODE_SESSION_ID`, "Set automatically to the current session ID in Bash and PowerShell tool subprocesses, hook command subprocesses, and stdio MCP server subprocesses", with resume caveats | https://code.claude.com/docs/en/env-vars.md, retrieved 2026-10-02 (row 21) | It conflicts with `docs/planning/decisions/C4-session-identity.md` §3's "only supported surface" sentence. It does not break C4's hook path. Recorded in `docs/planning/STATUS.md` "Open conflicts (oac-evidence §6)"; no ADR-001 amendment, since C4 is not ADR-001 text |
| D6 | §3.1: "research preview on Claude Code v2.1.232+"; `PINS.md` floor 1, "Channels exist at all: Claude Code `>= v2.1.232`" | Changelog `2.1.80`: "Added `--channels` (research preview) — allow MCP servers to push messages into your session". `2.1.232` is not a channels version. `mcp.md` L324 gives it as the version from which feature-flag-fetching sessions use the v2 MCP client runtime ("uses the v2 runtime on Claude Code v2.1.232 or later"), the runtime that adds `2026-07-28` and to which the L398 channel-negotiation constraint applies | `anthropics/claude-code` `CHANGELOG.md` @ `52c76441cae91f6891e4712306bffb057ff6fec5`; https://code.claude.com/docs/en/mcp.md L322-324; both retrieved 2026-10-02 (row 18) | No. The operative floor is the minimum version `v2.1.282` (#216), above both. Closes the floor-1 UNVERIFIED item as drift |

No drift item touches ADR-001, DESIGN.md, or a §5 decision directly (checked: none of
`resources/updated`, the reserved-prefix rule, or "Vertex" is named in ADR-001.md or
DESIGN.md), so no numbered `ADR-001-A*` amendment is proposed here.

(Dated note, 2026-10-02, #122: D4 and D6 also touch none of these. D5 touches the
session-identity decision, `docs/planning/decisions/C4-session-identity.md` §3. It is
recorded as an open conflict in `docs/planning/STATUS.md`, not resolved here. ADR-001,
DESIGN.md and §5's text are unchanged, so no `ADR-001-A*` amendment is proposed.)

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
13. **Whether `experimental` capabilities still exist at MCP `2026-07-28`** — UNVERIFIED,
    re-labelled from HOLDS in this pass (§3.3 table above): the prior indirect inference
    from Claude Code's own client capability was not a citation of the `2026-07-28`
    schema itself. Owner: whichever task first needs `capabilities.experimental` at the
    current era — a direct G1/G4 question.

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

## S0 classification note (2026-10-02, #228)

Gate S0 criterion 2 (`docs/planning/v0.1/10-stages.md` §4) accepts three outcomes for a §3
fact: holds, drifted, or `UNVERIFIED` with a reason. Some rows above carry a fourth
label, "Carried unchanged" or "PARTIAL". Under `oac-evidence` §5, a fact that "has not been
re-checked against the currently pinned version" is `UNVERIFIED`. These rows are therefore
classified as follows. Their verdict cells above are left as written, as history.

Each row below is `UNVERIFIED — carried from PLANNING-PROMPT.md §3.x, not re-checked
against the pin in B2`, unless the row says otherwise:

- §3.2 `thread/queue/add`; `codex queue --thread <id> --message <text>` (PARTIAL). The
  method name and its required params are now confirmed against the schema at `0.157.1`
  (fact 4 of the `0.157.1` table above, HOLDS). The `--thread`/`--message` flag spelling
  stays UNVERIFIED.
- §3.2 UUIDv7 thread ids surviving restarts.
- §3.2 `CODEX_HOME/sessions/` rollouts are not a supported surface.
- §3.2 `codex mcp-server` deprecation and deletion dates. This was already in the
  `STATUS.md` ledger; absence of the subcommand HOLDS (fact 7 of the `0.157.1` table).
- §3.2 hooks / `notify` (`agent-turn-complete`) cannot originate a turn.
- §3.2 Unix-socket peer validation semantics (PARTIAL; the WebSocket auth flags HOLD).
- §3.3 no SEP or working-group item for agent-to-agent messaging. Already in the ledger.
- §3.4 Zenoh dual EPL-2.0 / Apache-2.0 licensing; the stable API being `zenoh` +
  `zenoh-ext` only; the binding matrix (`zenoh-ts` needs `zenohd`).
- §3.4 the loopback fix attributed to PR #2671. The loopback discovery behavior itself
  is confirmed at `1.10.1` by G3 PASS (`docs/planning/gates/G3-result.md`); the PR
  attribution stays carried.
- §3.4 the multi-fact row: Windows scouting bind, `#iface=` platform scope, dynamic
  listen ports, liveliness history, storage/plugins in `zenohd`, `zenoh-ext` `unstable`,
  TLS/mTLS and QUIC certificates, ACL subjects, `zid` unauthenticated, no
  application-layer message signing. The `#iface=` sub-fact already has its own ledger
  entry.
- §3.5 ACP protocol version `1` (the literal `protocolVersion` field was not observed on
  the fetched page; `PINS.md` "ACP" already labels it UNVERIFIED).

All of these are listed in `docs/planning/STATUS.md` "Open UNVERIFIED items" under one
grouped entry dated 2026-10-02 (#228), which satisfies S0 criterion 3. No verdict above is
upgraded, and no fact is closed by this note.
