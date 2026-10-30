# Launching OAC-enabled sessions

How to start Claude Code and Codex so that OAC can push messages into them, and what to
check when it does not work. This is the user-facing launch guide (backlog G11, #72). The
design record behind it is [CLI and deployment](planning/v0.1/08-cli-and-deployment.md)
(§7 Claude command, §8 consent step, §9 Codex command).

**Status.** OAC is pre-release: nothing is installed or published, and `oac` is built with
`cargo build -p oac-cli`. Every command below is a documented target. The parts a live run
exercised (a real Claude Code `2.1.312` and a real Codex `0.165.0`, on Windows, 2026-10-29) are named
in [the G8 S6 record](planning/gates/herdr-runs/G8-S6-2026-10-29.md). Claude Code Channels is a
**research preview** surface and the Codex live-inject path is **experimental**; both can change
without notice. Claude Code and Codex versions float: a version other than the last tested one is
a warning, never a stop.

> **Everything that arrives over OAC is untrusted content.** An authenticated sender is still
> only a sender. A message can ask your session to do things; your session is not obliged to, and
> you should not approve a tool call because a message said to. Delivery permission does not grant
> tool approval. OAC keeps permission relay off.

## Why you launch OAC-enabled sessions

Neither harness can attach a channel to a session that is already running. Claude Code's MCP page
says you opt a channel in "with the `--channels` flag at startup" (https://code.claude.com/docs/en/mcp.md,
"Push messages with channels", retrieved 2026-10-30), and a second Codex process does not attach to a
thread another process holds (open issue `openai/codex` #21743). The project records this as the
structural finding in `docs/planning/PLANNING-PROMPT.md` §4 and ADR-001-A2
(`docs/planning/ADR-001-AMENDMENTS.md`): "existing session" means a session launched OAC-enabled.
A session that was not launched that way is outside what OAC supports; restart it with the
commands below.

## Before either harness: the daemon

`oac start` runs the per-device daemon (foreground by default; `--detach` leaves it running).
Start it first, then check it. The commands are the same in both shells.

```powershell
# PowerShell (Windows)
oac start --detach
oac doctor
```

```sh
# POSIX shell (Linux, macOS)
oac start --detach
oac doctor
```

`oac doctor` prints one line per check (`PASS`, `WARN`, `FAIL` or `SKIP`) and exits `1` only if a
check fails. Re-run it after each launch below; the [failure table](#when-it-does-not-work) says
what each line means. Stop the daemon with `oac stop`. Right after `oac start`, before any
harness is launched and paired, the `sessions` and `pairing` WARNs are expected.

## Claude Code

### 1. Register the OAC server and the session hook (once per project)

Claude Code spawns `oac mcp-shim` as an MCP server, and a `SessionStart` hook tells the daemon
which session it is. Put both in the project you will launch from.

`.mcp.json` (project root):

```json
{
  "mcpServers": {
    "oac": { "command": "oac", "args": ["mcp-shim", "--harness", "claude"] }
  }
}
```

`.claude/settings.json` (or `.claude/settings.local.json` to keep it out of git):

```json
{
  "hooks": {
    "SessionStart": [
      { "hooks": [ { "type": "command", "command": "oac hook" } ] }
    ]
  }
}
```

The server's key (`oac`) must equal the name in the launch command (`server:oac`). If `oac` is
not on the `PATH` Claude Code sees, use the absolute path in both `command` fields (the live run
used absolute paths; a bare `oac` relies on `PATH` and has not been run on Windows: UNVERIFIED).
`oac hook` writes nothing to standard output and always exits `0`, so a misfire never shows a
hook error.

### 2. The launch command

Quoted verbatim from the first-party example for a server that is not on Anthropic's allowlist,
with OAC's server name:

```
claude --dangerously-load-development-channels server:oac
```

It is the same line in PowerShell and in a POSIX shell. Run it in an interactive terminal from the
project that holds the `.mcp.json`: with `-p` (or through the Agent SDK) Claude Code ignores the
flag and the channel does not register.

**About `--channels`.** The backlog item asks for `--channels` in the command. The first-party
documentation does not allow that for OAC today. `--channels` takes `plugin:<name>@<marketplace>`
entries only, and only for plugins on the Anthropic or organization allowlist; a bare server such
as `server:oac` is accepted only by `--dangerously-load-development-channels`; and "Combining this flag with
`--channels` doesn't extend the bypass to the `--channels` entries" (channels reference). OAC
publishes no channel plugin, so there is no OAC `--channels` command to quote. The shape that will
apply once an OAC plugin is on an allowlist is `claude --channels plugin:<name>@<marketplace>`,
with `<name>` and `<marketplace>` not yet existing (placeholders, not a command to run). Neither
flag appears in `claude --help` (checked at `2.1.312`); both work.

### 3. The confirmation is a feature

Claude Code first shows a full-screen warning dialog listing the development channels you are
loading. Choose **I am using this for local development** to continue, or **Exit** to quit. The
first time in a project it also asks you to approve the new `.mcp.json` server ("Use this MCP
server"). Afterwards a dim notice under the banner says that messages from `server:oac` inject
directly into this session, and how to stop (restart without the flag).

Read that dialog. It is the one consent step you give when OAC is loaded as a development
channel, and it is the reason a message pushed by another session can reach yours at all. OAC
does not remove it:

- OAC ships no wrapper, alias or script that answers or suppresses the dialog;
- `oac start`, `oac doctor` and every other `oac` verb never pass `--dangerously-load-development-channels`
  for you: you, or your own shell configuration, type it;
- permission relay stays off, so the dialog's confirmation is the only grant, with no second
  silent grant behind it.

Do not paste the command into a script that also feeds the dialog its answer.

## Codex

OAC reaches a live Codex thread through Codex's own app-server daemon, over its control socket,
and delivers with `thread/queue/add` (queued until the thread is idle; never `turn/start` or
`turn/steer`). This is the path gate G2 passed. The `codex --remote` fallback is deferred: see
[`adapters/codex/README.md`](../adapters/codex/README.md), "Deferred: the `codex --remote`
fallback".

### 1. Start Codex's app-server daemon (you start it; OAC never does)

```powershell
# PowerShell (Windows): a normal, non-elevated terminal
codex app-server daemon start
```

```sh
# POSIX shell (Linux, macOS)
codex app-server daemon start
```

`start` is idempotent. The daemon README calls this component experimental. OAC never starts, stops
or restarts the daemon; `oac doctor`'s `codex` check only connects to its control socket and
closes. On Windows the daemon needs a non-elevated terminal, and the 108-byte `AF_UNIX` limit
applies to Windows automatic attachment: a junction alias over the limit falls back to the embedded
server, so a very long `CODEX_HOME` can leave a Codex that attaches to nothing (shorten it). G2
passed on Windows only; macOS and Linux are unexercised.

### 2. Register the OAC server with Codex (once)

```
codex mcp add oac -- oac mcp-shim --harness codex
```

The same line in both shells. The name (`oac`) is a daemon setting: Codex reports it as the
`server` of each tool call and OAC recognises a call only by it. To register under another name,
start the daemon with the same name (`oac start` reads `OAC_CODEX_REGISTERED_NAME`; unset means
`oac`):

```powershell
codex mcp add oac-work -- oac mcp-shim --harness codex
$env:OAC_CODEX_REGISTERED_NAME = "oac-work"; oac start --detach
```

```sh
codex mcp add oac-work -- oac mcp-shim --harness codex
OAC_CODEX_REGISTERED_NAME=oac-work oac start --detach
```

Remove the registration with `codex mcp remove oac`.

### 3. Launch Codex

Run plain `codex` from your project, in a terminal where `CODEX_EXEC_SERVER_URL` is not set (it
makes Codex skip implicit daemon attachment). Do not pass `--no-daemon` ("Run without the shared
background server, even if it is already running", `codex --help` at `0.165.0`) or `--remote`:
either one keeps the thread out of the daemon OAC talks to. When a daemon is running, the TUI
attaches to it as a client. With no daemon, Codex runs on its own and OAC cannot reach the thread;
if a discovered daemon cannot initialize the connection, Codex starts an embedded server instead,
with the same result. You approve Codex's own MCP tool prompts yourself (`1. Allow` for one call
is enough); OAC answers none of them.

Codex starts a registered MCP server when the session opens, but pairing completes only on the
first OAC tool call in a thread. The first call is refused with a pairing value and the next one
returns the session id (G8 S6 record, claim 1). Until then `oac doctor` shows the Codex shim as
connected with no session bound (`sessions` WARN): that is normal. If it persists after a call,
check `codex mcp add <name>` against `OAC_CODEX_REGISTERED_NAME`. After the pairing the daemon
holds a session for that thread (`oac sessions`).

## Distribution constraints (Claude Code Channels)

- **Allowlist.** Channels are in research preview. Plugins on Anthropic's allowlist
  (`claude-plugins-official`) load with `--channels`. OAC is not on it, so it loads only through
  `--dangerously-load-development-channels`. Getting OAC allowlisted is deferred product work, not a
  v0.1 path.
- **Organization settings.** On claude.ai Team and Enterprise, an Owner must enable channels
  (Organization settings > Claude Code > Channels, or `channelsEnabled: true` in managed settings);
  until then channels are blocked and users cannot override it. `channelsEnabled` is the master
  switch and also blocks the development flag. `allowedChannelPlugins` (managed) replaces the
  Anthropic list for `--channels`; per the channels page an empty list still lets the development
  flag through, so it does not affect OAC's launch, but it would apply to a future OAC plugin.
  If channels are off, the MCP server still connects and its tools work, but channel messages do
  not arrive, and a startup warning says an admin must enable it. Pro, Max and Ultra users with no
  organization skip these checks. On Console organizations with managed settings, `channelsEnabled`
  must be set.
- **Platform.** Channels require Anthropic authentication through claude.ai or a Console API key
  and "are not available on Amazon Bedrock, Google Cloud's Agent Platform, or Microsoft Foundry".
  (The older planning text says "Vertex"; Google renamed it Agent Platform.) A Claude Code started
  with `CLAUDE_CODE_USE_BEDROCK`, `_VERTEX`, `_FOUNDRY` or `_MANTLE` set is on one of those.
- **MCP revision.** On the v2 runtime, a channel server that negotiates MCP revision `2026-07-28`
  cannot deliver channel messages and is not registered as a channel. `oac mcp-shim` stays on the earlier
  handshake; do not set `MCP_PROTOCOL_NEGOTIATION` to anything but `legacy`.

## When it does not work

Run `oac doctor` and read the line. The `launch_env` check reads the environment of the shell
it runs in, so run it in the same shell you launch the harness from. Where OAC cannot see a failure without reading a harness's
configuration or credentials, which it never does, the table says so and names where the harness
reports it instead.

| Failure mode | Doctor check | Fix |
|---|---|---|
| No daemon running; endpoint is another user's; daemon from another `oac` build | `daemon` (FAIL) | `oac start`; use the `oac` that started the daemon |
| IPC run directory or pipe not private to your user | `ipc_permissions` (FAIL) | Fix the directory owner and mode; on Windows, restart as the same user that started the daemon |
| Device key store unavailable or locked | `credential_store` (WARN with a fallback file, else FAIL) | Unlock or enable the OS credential store (the daemon can use an encrypted-file fallback on Unix) |
| Transport peer cannot link | `transport` (FAIL) | Check the network path between the daemons; local mode needs no scouting |
| Claude Code too old, version unreadable, or not on `PATH` | `claude_version` (WARN below `2.1.282`; SKIP if `claude` is not on `PATH`) | Update Claude Code; put it on `PATH`. A version is only ever a warning |
| Claude session negotiated a non-legacy MCP revision, so the channel never registered | `mcp_revision` (WARN) | `MCP_PROTOCOL_NEGOTIATION` unset or `legacy`; launch through `oac mcp-shim` (the config above) |
| Claude Code runs on Bedrock, Agent Platform (Vertex) or Foundry | `launch_env` (WARN, names the variable) | Unset the `CLAUDE_CODE_USE_*` variable in the launching shell and sign in with claude.ai or a Console API key. A provider set in a settings file's `env` block is not visible to doctor: `/status` in Claude Code shows the API provider |
| Server not in `.mcp.json`, key differs from `server:<name>`, project server not approved, or the dialog answered **Exit**: no Claude connection ever opens | `sessions` (WARN: no harness session is attached) | Match the key to `server:oac`; approve the server; relaunch and accept the dialog |
| `SessionStart` hook missing or not run: the shim is connected but unbound | `sessions` (WARN: claude connection open with no session bound) | Add the hook (step 1); trust the project folder if Claude Code asks; relaunch |
| Codex registered under a name the daemon does not expect, so pairing never completes | `sessions` (WARN: codex connection open with no session bound). The same WARN is normal before the first OAC tool call in the thread, and doctor cannot tell the two apart | After a call, make `codex mcp add <name>` and `OAC_CODEX_REGISTERED_NAME` agree and restart `oac start`; the daemon log names the mismatch |
| Codex ran without the daemon (`--no-daemon` or `--remote`; no daemon when it started; a discovered daemon that could not initialize the connection; a junction alias over the 108-byte limit on Windows), so the thread is not in the daemon | `sessions` (WARN: codex connection open with no session bound); `codex` (SKIP if no socket) | Start the daemon, then relaunch plain `codex` from a normal terminal; shorten `CODEX_HOME` on Windows |
| `CODEX_EXEC_SERVER_URL` set in the launching environment | `launch_env` (WARN) | Unset it before starting Codex |
| Codex app-server daemon not running (no control socket) | `codex` (SKIP: OAC never starts it) | `codex app-server daemon start` |
| Control socket there but not accepting, or its owner cannot be verified | `codex` (WARN) | Restart the daemon: `codex app-server daemon restart` |
| Control socket served by another user's process | `codex` (FAIL) | Stop using that socket; run your own daemon under your user |
| No pairing or grant, so default deny delivers nothing | `pairing` (WARN) | Pair devices or add a grant (the pairing verb, #437, is not built yet) |
| Launched without `--dangerously-load-development-channels`, or in `-p` mode, so the server runs as a plain MCP server and no wake arrives | none: Claude Code gives no acknowledgement, and the session still binds, so `sessions` passes | Relaunch interactively with the command above; look for the notice under the banner |
| `channelsEnabled` off or unset on a Team or Enterprise organization | none: reading managed settings or Claude's configuration is outside what doctor may do | Ask an Owner to enable channels; Claude Code prints a startup warning (the channels reference calls it "blocked by org policy") |
| Not signed in through claude.ai or a Console API key ("Login expired") | none: doctor never reads credentials | `/login` in Claude Code |

Two of these have no check by design, and one by the harness's silence. The doctor never reads a
harness's credential file, database or configuration directory (`scripts/check-doctor-boundary.mjs`
enforces it), and Claude Code acknowledges no channel notification.

## What was verified, and where

Retrieved 2026-10-30. Claude Code `2.1.312` and Codex CLI `0.165.0` are the installed versions
(`claude --version`, `codex --version`).

| Claim | Source |
|---|---|
| `claude --dangerously-load-development-channels server:webhook`; the dialog's options; the `.mcp.json` approval; the notice text; `-p` ignores the flag; the flag does not extend the bypass to `--channels` entries | https://code.claude.com/docs/en/channels-reference.md ("Test it", "Test during the research preview") |
| `--channels` takes `plugin:` entries; the development flag takes `plugin:` and `server:` entries, prompts, and `-p` ignores it | https://code.claude.com/docs/en/cli-reference.md (flag table) |
| `channelsEnabled`, `allowedChannelPlugins`, defaults by plan, empty-list behaviour, no Bedrock / Agent Platform / Foundry, claude.ai or Console key auth, flags absent from `--help`, "Being in `.mcp.json` isn't enough" | https://code.claude.com/docs/en/channels.md ("Enterprise controls", "Research preview") ; both keys are scope Managed in https://code.claude.com/docs/en/settings-reference.md (index) |
| Revision `2026-07-28` servers are not registered as channels; `MCP_PROTOCOL_NEGOTIATION=legacy` | https://code.claude.com/docs/en/mcp.md ("Push messages with channels") |
| `CLAUDE_CODE_USE_BEDROCK=1`, `CLAUDE_CODE_USE_VERTEX=1`, `CLAUDE_CODE_USE_FOUNDRY=1`, `CLAUDE_CODE_USE_MANTLE` | https://code.claude.com/docs/en/amazon-bedrock.md, google-vertex-ai.md, microsoft-foundry.md |
| `SessionStart` hook structure and settings locations | https://code.claude.com/docs/en/hooks.md ("Hook locations"); the structure for `SessionStart` is built from the `PostToolUse` example and from the live run's settings |
| `codex mcp add <name> -- <command...>`, `codex mcp remove`, `codex --remote`, `codex app-server` experimental | https://learn.chatgpt.com/docs/cli/reference; `codex mcp add --help` at `0.165.0` |
| `codex app-server daemon` start, stop, restart, version; idempotent `start`; the daemon is called experimental; `CODEX_EXEC_SERVER_URL` skips implicit attachment; a discovered daemon that cannot initialize the connection gives an embedded server; Windows non-elevated terminal and the 108-byte limit | `codex app-server daemon --help` at `0.165.0`; https://raw.githubusercontent.com/openai/codex/rust-v0.165.0/codex-rs/app-server-daemon/README.md |
| `turn/steer`, `turn/start`, experimental API gate | https://learn.chatgpt.com/docs/app-server (that page does not mention the daemon or `thread/queue/add`: they are documented in the daemon README and the generated schema) |
| What worked end to end on 2026-10-29 | [G8 S6 record](planning/gates/herdr-runs/G8-S6-2026-10-29.md) |

Not verified, stated as such above: a bare `oac` (no path) in the Claude `.mcp.json` and hook
`command` on Windows; whether Codex configuration overrides (`-c`) also disable implicit daemon
attachment (the daemon README names only `CODEX_EXEC_SERVER_URL`); whether every provider selector
treats words other than `1` and `true` as on; macOS and Linux for the Codex path.
