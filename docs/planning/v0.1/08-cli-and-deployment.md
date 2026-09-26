# 08 — CLI and Deployment

**Issue:** #29 (Epic A, backlog key A9). **Source:** `docs/planning/PLANNING-PROMPT.md`
§9 item 9, §5 decision 11, `docs/planning/DESIGN.md` "CLI / supervisor" (lines 18-25).

**Scope.** This file states the zero-container local launch story per harness, the CLI
command surface, and the LAN hooks that shape v0.1 architecture only. It does not own:
process-model justification (`docs/planning/decisions/C2-process-model.md`), architecture
diagrams (`docs/planning/v0.1/04-architecture.md` §12-§13), the threat model
(`docs/planning/v0.1/06-security.md`), or repository layout
(`docs/planning/v0.1/07-repository-and-dependencies.md`) — each is cited by path, not
restated.

**Naming.** Product and repository: **Open Agent Channel (OAC)**. Normative protocol
specification: **OAC Session Channels**. CLI binary: **`oac`**. Per ADR-001-A1
(`docs/planning/v0.1/03-decisions-and-amendments.md` §2), this file never writes bare
"Session Channels" or `sessionchannels`, except at §3 below, where `DESIGN.md`'s
pre-rename CLI block is quoted verbatim and marked as such.

---

## Gate-verdict caveat, read before the rest of this file

Per `docs/planning/STATUS.md`'s Gate verdicts table, **gate G1 (Claude wake) and gate G2
(Codex live inject) are both `NOT RUN`**. Every launch command in this file — §7's Claude
Code command, §9's Codex command, §10's daemon-attach target — is a **documented
target**, a command verified against first-party syntax, not a proven wake path. Stated
once here, in the style of `docs/planning/v0.1/04-architecture.md` lines 14-23, rather
than repeated at each command.

---

## 1. Naming note — the pre-rename CLI block

`docs/planning/DESIGN.md` lines 20-25 show the CLI's original, pre-rename form:

```text
sessionchannels start
sessionchannels status
sessionchannels sessions
sessionchannels doctor
```

This is quoted here only to record it as **pre-rename** — it is `DESIGN.md`'s own
still-unrenamed text (register entry C12,
`docs/planning/v0.1/03-decisions-and-amendments.md` §4), not a spelling this file or any
other reuses. Per ADR-001-A1, the resolved binary name is **`oac`**, and the resolved
command forms are `oac start` / `oac status` / `oac sessions` / `oac doctor` (§5 below).

**`oac` binary-name collision check.** Already performed and recorded, not re-run here:
`docs/planning/v0.1/03-decisions-and-amendments.md` §2 (ADR-001-A1), "CLI name-conflict
check for `oac`" — crates.io, Homebrew, and Debian/Ubuntu: not found; npm: an inactive,
contentless stub (`oac@0.0.0`, published 2021-06-17), a name-squat rather than an active
competing tool, non-blocking because OAC ships no npm-distributed package in v0.1.

---

## 2. Zero-container local path

Quoted verbatim, `docs/planning/ADR-001.md` line 21:

```text
The reference implementation is a CLI and must not require Docker, Kubernetes, a cloud
account, or a separately administered server for normal local use.
```

Quoted verbatim, `docs/planning/DESIGN.md` line 11: "One-command local CLI deployment; no
mandatory containers/cloud."

**Carried by citation, not re-derived.** `docs/planning/decisions/C2-process-model.md`
§9's three-point argument that the daemon does not become "a separately administered
server for normal local use": the daemon is a **user-owned** process (no elevated or
service-account identity); it is **auto-startable by the shim** — `oac mcp-shim`, the
process a harness spawns automatically, is the natural place to check for a running
daemon and start one, so the user takes no separate administrative step; and it is **not
installed as a service** (no supervisor unit file, no auto-start on login, no multi-user
daemon). C2 §9's own conclusion: "This argument holds." `docs/planning/decisions/
C7-zenoh-transport.md` §5's "no `zenohd`": OAC's transport peer runs in-process inside
the daemon, in peer mode; no router process is required for the default local path.

---

## 3. Zero-file default

From `docs/planning/decisions/C2-process-model.md` §7. All of the following apply with
**no config file present**:

- **IPC path**, per platform default (C2 §4): Windows
  `\\.\pipe\oac-<user-sid-or-hash>`; Unix `$XDG_RUNTIME_DIR/oac/<socket-name>`, fallback
  `~/.oac/run/<socket-name>` when `$XDG_RUNTIME_DIR` is unset. No path needs to be
  supplied.
- **Device key**, auto-created on first `oac start`, written into the OS credential
  store via `keyring` `4.2.0` (`docs/planning/decisions/C1-language-runtime.md` §8) — no
  manual key-generation step, no file to place.
- **Transport**, local-only by default (loopback-bound where applicable); no LAN/remote
  transport is enabled without explicit configuration (§12 below).

**Load-bearing sentence.** Every command in §5's table below is reachable with no config
file at all — `oac start`, `oac status`, `oac sessions`, `oac doctor`, and
`oac mcp-shim` all function against pure defaults.

---

## 4. Config precedence

A config file is **optional**. When present, its path follows the conventional
per-platform config-directory form for the app name `oac` (e.g. under
`$XDG_CONFIG_HOME/oac/` on Unix, the platform config directory on Windows) — the exact
directory-resolution crate/library is a Stage 3 implementation detail, not decided here.

Precedence order, highest first: **flag > env > file > default**. An explicit CLI flag
always wins, then an environment variable, then a config-file value, then the built-in
default. Source: `docs/planning/decisions/C2-process-model.md` §7.

---

## 5. Command surface table

Reproduced from `docs/planning/decisions/C2-process-model.md` §6.

| Command | Purpose | Exit-code behaviour |
|---|---|---|
| `oac start` | Start (or confirm running) the per-device daemon. **Idempotent**: running it while a daemon is already up for this device is a no-op success, not an error. **Foreground by default**; `--detach` self-daemonizes into the background. | `0` on a daemon now running (newly started or already up); non-zero on failure to bind the IPC endpoint or start. |
| `oac status` | Report daemon up/down, device id, and transport state (peer up/down). | `0` when the daemon is up and reachable over IPC; non-zero when down or unreachable — scriptable as a liveness check. |
| `oac sessions` | List sessions currently registered with the daemon. | `0` on a successful listing (including an empty list); non-zero if the daemon is unreachable. |
| `oac doctor` | Preflight check (§6 below expands the check list). | `0` only if every check passes; non-zero and a per-check report otherwise. |
| `oac mcp-shim` | The stdio MCP server Claude Code and Codex spawn as their child process; connects to the daemon over local IPC. **Not meant to be typed by hand** — a harness config entry invokes it, a person does not run it directly. | N/A as a person-typed command; governed by the spawning harness's own process lifecycle. |
| *(named, semantics deferred)* | A pairing subcommand and any device-setup subcommand a future launch decision needs. Named here as a reserved placeholder; no flags or output shape are specified. Semantics owned by `docs/planning/decisions/C5-envelope-auth.md` §10(b) (pairing) — not designed here. | Not specified. |

---

## 6. `oac doctor` check list

Expanded from the doctor row in §5, per `docs/planning/decisions/C2-process-model.md`
§6 and `docs/planning/PINS.md`'s constraint floors:

1. **IPC path exists with correct permissions** — the Windows named-pipe DACL or Unix
   socket-directory/socket-file permissions (`0700`/`0600`), per
   `docs/planning/decisions/C2-process-model.md` §4.
2. **Credential store reachable** — the OS credential store `keyring` `4.2.0` targets
   (Windows Credential Manager / macOS Keychain / Linux Secret Service or
   `linux-keyutils`), per `docs/planning/decisions/C4-session-identity.md` §10.
3. **Transport peer able to come up** — the daemon's in-process transport peer can bind
   its local listener, per `docs/planning/decisions/C7-zenoh-transport.md` §5.
4. **Claude Code version at/above the floor in `docs/planning/PINS.md`** (UNVERIFIED —
   `>= v2.1.232` is an open UNVERIFIED item, not confirmable on `channels.md` at
   `v2.1.274`; see `docs/planning/STATUS.md` "Open UNVERIFIED items" and
   `docs/planning/PINS.md`) — **plus** `MCP_PROTOCOL_NEGOTIATION` set correctly
   (`legacy`, or unset), per `docs/planning/PINS.md` "Constraint floors" and the Claude
   Code Channels pin record.
5. **Codex daemon reachability** — whether the Codex app-server's control socket
   (`CODEX_HOME/app-server-control/app-server-control.sock`, §10 below) is reachable.

**Report shape.** Each failing check reports on its own; `oac doctor` exits `0` only if
every check passes, matching §5's exit-code cell for this command.

---

## 7. Claude Code launch command

**Re-check performed before writing this section, per the task's own instruction, not
skipped.** `docs/planning/decisions/C2-process-model.md` §6 already re-checked
`channels-reference.md` for whether a `--channels` flag exists and what it takes, before
this file was written — that finding is cited here, not re-derived: the presumptive
shape `claude --channels server:oac` does **not** match what the first-party docs show
for a channel not on Anthropic's allowlist (`claude-plugins-official` /
`allowedChannelPlugins`), which `oac mcp-shim` is not. `--channels` is the
allowlisted-plugin path, not OAC's path today; OAC does not use it, and this file does
not print it beside the command below as if either worked.

The documented form for a bare, non-plugin server during the research preview is the
**development flag**, quoted verbatim from a first-party example:
`claude --dangerously-load-development-channels server:webhook`. Source:
https://code.claude.com/docs/en/channels-reference.md, retrieved 2026-09-17, Claude Code
`v2.1.274`, surface label **research preview**. Applied to OAC's own server name:

```
claude --dangerously-load-development-channels server:oac
```

**Deferred, not decided here** (carried from `docs/planning/decisions/
C2-process-model.md` §6): whether OAC pins to this flag long-term or pursues an eventual
allowlist entry is a product decision this file does not make — stated only as the
currently correct documented command at the pinned version.

---

## 8. Claude consent step — preserved, not weakened

Own subsection, per the task breakdown, distinct from §7's command.

The source page documents the dialog this flag triggers, quoted verbatim: "Claude Code
first shows a full-screen warning dialog listing the development channels you're
loading." Source: https://code.claude.com/docs/en/channels-reference.md, retrieved
2026-09-17.

**Three non-weakenings, stated explicitly:**

- OAC ships no wrapper, alias, or script that suppresses this dialog.
- `oac doctor` and `oac start` never pass `--dangerously-load-development-channels` on
  the user's behalf — the flag is typed by the person launching Claude Code, or by their
  own shell configuration, never by OAC.
- Permission relay stays off by default (`docs/planning/v0.1/06-security.md` §11,
  strands (b)-(c)): the dialog's confirmation is the one and only consent step a user
  grants when loading OAC as a development channel; an on-by-default permission relay
  would add a second, silent grant behind that same confirmation, which
  `docs/planning/decisions/C6-trust-rendering.md` §7 and
  `docs/planning/v0.1/06-security.md` §11 both name and reject.

**Allowlist route named, not pursued here.** `claude-plugins-official` /
`allowedChannelPlugins` is deferred product work, not a v0.1 path — §7's "Deferred, not
decided here" note applies identically here.

---

## 9. Codex launch command

Quoted syntax, first-party: `add <name> -- <command...> | --url <value>`. Source:
https://learn.chatgpt.com/docs/cli/reference, retrieved 2026-09-17, Codex `0.154.0`.
Applied to OAC:

```
codex mcp add oac -- oac mcp-shim
```

This is the **supported outbound tool-registration** path
(`docs/planning/decisions/C2-process-model.md` §6) — distinct from the live-inject path
§10 below names, which is app-server runtime behaviour, not a CLI command a user types.

---

## 10. G2 target statement — pending, not proven

Separate from §9's command, per the task breakdown.

The live-inject path — daemon-attach, `codex app-server daemon start`, control socket
`CODEX_HOME/app-server-control/app-server-control.sock` — is confirmed working per gate
**G2**'s verdict. Per `docs/planning/STATUS.md`'s Gate verdicts table, **G2 is `PASS`**
(`0.157.1`, re-run 2026-09-26, Windows only; macOS and Linux remain unexercised). This
file does not present the inject path as a command a user types — it is app-server
runtime behaviour, consumed by the daemon's own Codex app-server client
(`docs/planning/decisions/C2-process-model.md` §1 item 4), not invoked from a shell.

G2's recorded fallback, per `docs/planning/STATUS.md`'s Gate verdicts table: "OAC-owned
app-server with `codex --remote`."

**Open UNVERIFIED items carried, not re-derived** (`docs/planning/decisions/
C2-process-model.md` §2 leg 4, §10):

- Whether implicit Codex daemon attach is enabled by default at runtime in the pinned
  release `0.154.0` (commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`) — the code path
  is source-confirmed present at this commit; runtime behaviour is G2's own go/no-go
  question.
- A 2026-09-17 re-fetch of `https://learn.chatgpt.com/docs/app-server` did not surface
  the `codex app-server daemon start` command or the `app-server-control.sock` control-
  socket path in the page content returned — recorded as a possible documentation-drift
  signal, not confirmed drift; resolution path is task D2 / gate G2, not a documentation
  re-read.

Both items already appear in `docs/planning/STATUS.md`'s "Open UNVERIFIED items" list;
this section restates them by pointer, it does not add a new one.

---

## 11. Which command is "the one command"

`docs/planning/DESIGN.md`'s acceptance criterion 1, "One-command local startup," and the
two-harness launch reality of §7/§9 above are reconciled explicitly, rather than left
ambiguous:

- **`oac start`** is the one local startup command — it starts (or confirms running) the
  per-device daemon, with no config file required (§3).
- **§7's Claude command** and **§9's Codex command** are per-harness
  registration/launch commands, not startup commands: the Claude command is run once per
  Claude Code session (it launches that session with OAC's development channel loaded);
  the Codex command is run once, at registration time, to add `oac mcp-shim` as an
  external MCP server Codex can call as a tool.

`oac start` answers DESIGN.md's criterion; §7 and §9 are a separate, per-harness concern
this file states plainly rather than folding into "the one command."

---

## 12. LAN hooks

Scoped to what shapes v0.1 architecture only, per the task breakdown.

- **No new process kind.** Same daemon, same shims — only the transport module's
  configuration differs (local-only bind versus a LAN-facing listener). Diagrammed at
  `docs/planning/v0.1/04-architecture.md` §13, not redrawn here.
- **The CLI hook is the reserved pairing subcommand** named in §5's table — named here,
  semantics owned by `docs/planning/decisions/C5-envelope-auth.md` §10(b), not designed
  in this file.
- **LAN mode is off unless explicitly configured.** Per §3's zero-file default: no
  LAN/remote transport is enabled without explicit configuration.
- **Federation, routing, and multi-hop are deferred**, per `oac-boundaries` 11 — this
  file draws no topology; `docs/planning/v0.1/04-architecture.md` §13 already draws the
  one LAN hop this scope permits.

---

## 13. Smallest-design statement

Carried from `docs/planning/decisions/C2-process-model.md` §8's deliberately-omitted
list, one line each:

- **No supervisor/service installer.** `oac start` is a command a user runs (or a shim
  runs on their behalf, §2); OAC does not register itself with `systemd`, Windows
  Services, or `launchd`.
- **No auto-start on login.** The daemon starts when something needs it — first
  `oac start`, or the first `oac mcp-shim` that finds no daemon running — not at every
  login regardless of use.
- **No multi-user daemon.** One daemon per device is scoped to one user.
- **No durable message store.** Deferred per `docs/planning/ADR-001.md` "Defer" —
  durable offline mailboxes are explicitly deferred, not v0.1 work.

---

## 14. Surface labels table

One label per surface named in this file. Shim boundaries are cited, not restated.

| Surface | Label | Pin | Shim boundary |
|---|---|---|---|
| Claude Code Channels | research preview | `v2.1.274` (`docs/planning/PINS.md`) | `adapters/claude/` (UNVERIFIED — module name not yet fixed in `DESIGN.md`; register entry C11, `docs/planning/v0.1/07-repository-and-dependencies.md` §4(b)) |
| Codex app-server / daemon-attach | experimental (per-method gating) | `@openai/codex@0.154.0` @ commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` (`docs/planning/PINS.md`) | `adapters/codex/` (UNVERIFIED — same C11, `docs/planning/v0.1/07-repository-and-dependencies.md` §4(b)) |
| `keyring` | supported | `4.2.0` (`docs/planning/PINS.md`) | not applicable — general-purpose crate, not a provider surface |
| `interprocess` | supported | `2.4.4`, candidate (`docs/planning/PINS.md`) | not applicable — general-purpose crate, not a provider surface |

---

## 15. Boundary pass

Grep-checkable, per `oac-boundaries`' pre-commit self-check.

- No Zenoh vocabulary (Zenoh, `zid`, key expression, liveliness) appears anywhere in
  this file — §2's transport paragraph names "the transport peer" and "in-process," and
  cites `docs/planning/decisions/C7-zenoh-transport.md` §5 by path rather than
  describing Zenoh internals; the one exception, "no `zenohd`," is a rejected-term
  citation identical in kind to `docs/planning/v0.1/04-architecture.md` §13's own `zid`
  citation, not a use of Zenoh vocabulary as a subject.
- No CLI table row (§5), launch command (§7, §9), or LAN section (§12) names a Zenoh
  type or key expression — transport vocabulary stays confined to §2's one cited
  paragraph, matching `docs/planning/v0.1/04-architecture.md` §1's "transport box only"
  rule.
- Nothing in the CLI surface (§5-§6) owns a harness turn loop, holds provider
  credentials, or polls: `oac doctor`'s checks (§6) are one-shot preflight checks, not a
  polling loop; `oac mcp-shim` is a thin stdio connection (§5), holding no key material
  (`docs/planning/v0.1/04-architecture.md` §2's component table); the Codex credential
  boundary is restated at §10 by pointer to
  `docs/planning/v0.1/04-architecture.md` §10, not re-derived.

---

## 16. Evidence pass

Per `oac-evidence` §8, checked against this file:

- Every externally verifiable claim in §7, §8, §9, §10 carries URL + version + retrieval
  date, or cites a landed decision document (`C2-process-model.md`) that already carries
  one, per `oac-evidence` §2's "cite the section" allowance.
- Every quoted flag/command (`--dangerously-load-development-channels`,
  `--channels`, `codex mcp add`, `codex app-server daemon start`) is copied verbatim
  from a cited first-party source, none invented.
- Every provider surface named is labelled at first mention (§14): Claude Code Channels
  — research preview; Codex app-server / daemon-attach — experimental (per-method
  gating); `keyring`, `interprocess` — supported.
- No new UNVERIFIED item is introduced by this file. §10 restates two already-open items
  by pointer to `docs/planning/STATUS.md`; the `--channels`-does-not-apply finding is not
  itself UNVERIFIED — it is cited to `docs/planning/decisions/C2-process-model.md` §6,
  which already completed the first-party check this task asked for.

---

## 17. Acceptance close-out

Ticked against issue #29's four acceptance boxes, in the style of
`docs/planning/v0.1/07-repository-and-dependencies.md` §10.

- [x] **One documented command each for Claude/Codex, quoted verbatim with flags** —
      §7 (Claude: `claude --dangerously-load-development-channels server:oac`), §9
      (Codex: `codex mcp add oac -- oac mcp-shim`).
- [x] **Zero-file local default; config optional** — §3-§4.
- [x] **Command surface stated** — §5-§6.
- [x] **`--dangerously-load-development-channels` consent step preserved** — §8.

---

## 18. Cross-file updates in this change

- `docs/planning/STATUS.md`: "Last updated" line records A9 (issue #29) landing.
- No new UNVERIFIED item is added by this file — §10 restates two already-open items by
  pointer; `docs/planning/STATUS.md`'s "Open UNVERIFIED items" list is unchanged.
- **No pin moved and no gate verdict changed by this file** — stated explicitly, per the
  task's own instruction to say so if true.

---

## 19. Cross-reference block

Every reference below is a repo-relative path; no prior context is assumed.

- `docs/planning/ADR-001.md`
- `docs/planning/DESIGN.md`
- `docs/planning/PLANNING-PROMPT.md` §5 decision 11, §9 item 9
- `docs/planning/decisions/C2-process-model.md`
- `docs/planning/decisions/C4-session-identity.md`
- `docs/planning/decisions/C5-envelope-auth.md`
- `docs/planning/decisions/C6-trust-rendering.md`
- `docs/planning/decisions/C7-zenoh-transport.md`
- `docs/planning/v0.1/03-decisions-and-amendments.md`
- `docs/planning/v0.1/04-architecture.md`
- `docs/planning/v0.1/06-security.md`
- `docs/planning/v0.1/07-repository-and-dependencies.md`
- `docs/planning/STATUS.md`
- `docs/planning/PINS.md`
