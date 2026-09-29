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

Per `docs/planning/STATUS.md`'s Gate verdicts table, **gate G1 (Claude wake) is `PASS`**
(originally on Claude Code `v2.1.282`; the Claude Code (Channels) pin went floating
2026-09-27, last observed `v2.1.283`, invalidating that result; re-run and **PASSED
again** 2026-09-28 on `v2.1.283` — see `docs/planning/gates/G1-result.md`), **and gate
G2 (Codex live inject) is `PASS`** (`0.157.1`, re-run 2026-09-26, Windows only). Every
launch command in this file — §7's Claude Code command, §9's Codex command, §10's
daemon-attach target — is a **documented target**, a command verified against
first-party syntax, not a proven wake path. Stated once here, in the style of
`docs/planning/v0.1/04-architecture.md` lines 14-23, rather than repeated at each
command.

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

**No Beacon check, deliberately.** `oac doctor` does not inspect harness MCP configuration
for a Beacon entry: those files "hold other servers' credentials"
(https://github.com/Asymptote-Labs/agent-beacon/blob/v1.3.29/docs/cli/mcp-connect.mdx
L102, tag `v1.3.29`, retrieved 2026-09-29), and such a check would be OAC inspecting
Beacon, which `docs/planning/decisions/L1-beacon-memory.md` §4 Q2 rules out; an operator
runs Beacon's own `beacon mcp doctor` instead
(https://github.com/Asymptote-Labs/agent-beacon/blob/v1.3.29/docs/cli/mcp-doctor.mdx
L6-16, tag `v1.3.29`, commit `91e92216b79108475ba9b587d49c5ff3f7356fd8`, retrieved
2026-09-29; §20.2 step 4).

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
- **L5 (issue #170, Epic L #165):** new §20 "Running beside an external memory service
  (Beacon)", applying `docs/planning/decisions/L1-beacon-memory.md` Q1-Q4; one paragraph
  in §6 stating that `oac doctor` has no Beacon check (Q2);
  `docs/planning/v0.1/12-deferred.md` §2 names OAC as a shared memory layer as boundary,
  not backlog; `docs/planning/STATUS.md` `**Last updated:**`. No `cli/` path, pin, gate
  verdict or UNVERIFIED item changes.

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
- `docs/planning/decisions/L1-beacon-memory.md`
- `docs/planning/v0.1/03-decisions-and-amendments.md`
- `docs/planning/v0.1/04-architecture.md`
- `docs/planning/v0.1/06-security.md`
- `docs/planning/v0.1/07-repository-and-dependencies.md`
- `docs/planning/STATUS.md`
- `docs/planning/PINS.md`

---

## 20. Running beside an external memory service (Beacon)

**Issue:** #170 (Epic L #165, backlog key `L5`). **Decision this section applies:**
`docs/planning/decisions/L1-beacon-memory.md` (L1), shape "Beacon beside OAC, not inside
it", operator answers Q1-Q5 in L1 §4. Nothing here re-opens them.

**What this section is.** Beacon (agent-beacon) is an external, independently installed
memory service. Each harness connects to it **natively**, through that harness's own MCP
configuration, which the operator edits. OAC is not involved in that connection at any
step. This section tells an operator how to run an OAC-enabled Claude Code session and an
OAC-enabled Codex session with Beacon on both sides, and what OAC does not do with memory.

**Sources.** Every Beacon fact below is read at https://github.com/Asymptote-Labs/agent-beacon,
tag `v1.3.29` (commit `91e92216b79108475ba9b587d49c5ff3f7356fd8`), retrieved 2026-09-29,
and cited as `path@v1.3.29` with line numbers. Surface label: Beacon's local MCP server
(`beacon mcp serve`, `docs/cli/mcp-serve.mdx@v1.3.29` L6-18) and its approved-memory
tools are **supported** per L1 §7. They are Beacon's surfaces, reached by the harness
only; none is an OAC provider surface.

### 20.1 Prerequisites

- **Beacon at the L1 pin.** The version is recorded once, in L1 §2 and in
  `docs/planning/PINS.md` row "Beacon (external memory service)". It is not repeated here.
  The pin holds a fixed version **only while Beacon's package self-updates stay off**
  (L1 §2). Leave them off on an OAC-enabled machine.
- **OAC installed.** OAC is planned, not built (see "Gate-verdict caveat" at the top of
  this file). The OAC commands below are its documented targets.
- **One repository per pair of peers, if they are meant to share memory** (§20.6).

### 20.2 Beacon side — the operator's own configuration

Only Beacon's own documented commands are used. OAC does not run, wrap or generate any of
them.

1. **Install Beacon and choose Local during setup.** Follow Beacon's quickstart
   (`docs/get-started/quickstart.mdx@v1.3.29` L6-39). Interactive setup "preselects
   Beacon Managed, with an explicit Local opt-out"; choose Local
   (`README.md@v1.3.29` L48-49, L151-155; `docs/get-started/quickstart.mdx@v1.3.29`
   L41-49). The privacy reasoning is §20.5.
2. **Endpoint capture, if the operator wants traces to become memory.**
   `beacon endpoint install --harness claude,codex` configures capture for exactly these
   two harnesses: `--harness` takes "an exact comma-separated list", and an explicit list
   "configures only the runtimes it names" (`docs/cli/endpoint-install.mdx@v1.3.29` L16,
   L29). Beacon's quickstart shows the three-harness example
   `beacon endpoint install --harness claude,codex,cursor`
   (`docs/get-started/quickstart.mdx@v1.3.29` L37-39). The two-harness form writes to
   Codex's own configuration (`docs/runtimes/codex-cli.mdx@v1.3.29` L43, L88). Whether that conflicts with OAC's
   Codex launch is L1 item U4 (UNVERIFIED — OAC's Codex launch path is not built and no one
   has compared the two; see `docs/planning/STATUS.md` "Open UNVERIFIED items").
3. **Give each harness Beacon's local MCP server.** Pick one of Beacon's two documented
   routes, per harness:
   - **Stdio entry.** Add Beacon's local server entry, named `beacon`, to the harness's
     own MCP configuration. Beacon documents it as an `mcpServers.beacon` entry with
     `"command": "beacon"` and `"args": ["mcp", "serve", "--transport", "stdio"]`, that
     is, `beacon mcp serve --transport stdio` (`docs/cli/mcp-doctor.mdx@v1.3.29` L38-52;
     flag `--transport stdio|http` from `docs/cli/mcp-serve.mdx@v1.3.29` L37). The
     operator writes the entry in each harness's own configuration format.
   - **Plugin.** Claude Code: `/plugin marketplace add asymptote-labs/agent-beacon`, then
     `/plugin install beacon@beacon`. Codex: `codex plugin marketplace add
     asymptote-labs/agent-beacon`, then `codex plugin add beacon@beacon`
     (`docs/concepts/beacon-skills.mdx@v1.3.29` L44, L46). The plugin "also registers
     Beacon's local MCP server (`beacon mcp serve`) in harnesses that accept MCP servers
     from plugins" (`docs/concepts/beacon-skills.mdx@v1.3.29` L33-35). These commands name
     the repository with no tag, so L1's pin is stated against the installed `beacon`
     binary, not against the plugin contents.
4. **Check it with Beacon's own tool.** `beacon mcp doctor` validates the local server and
   prints the client entry (`docs/cli/mcp-doctor.mdx@v1.3.29` L6-16, L38-52). This is the
   check to run instead of an OAC one (§6, last paragraph).

Do not run `beacon mcp connect` for this setup. It registers the separate hosted server
`beacon-managed` (`docs/cli/mcp.mdx@v1.3.29` L22-37), which is out of Epic L's scope (L1
§7).

### 20.3 OAC side — the launch commands already in this file

Only the commands in §5, §7 and §9 are used. Planned, not built; the "Gate-verdict
caveat" at the top of this file applies to each.

1. Start the per-device daemon with `oac start` (§5).
2. Claude Code: launch the session with the development-channel command in §7. The
   consent dialog in §8 is unchanged.
3. Codex: register `oac mcp-shim` once with the command in §9.

Each harness then holds two independent MCP servers in its own configuration: OAC's
(`oac mcp-shim`) and Beacon's (`beacon`). Neither knows about the other, and OAC never
reads the Beacon entry.

### 20.4 Referencing memory across sessions (Q1)

- **The memory ID goes in the message text.** A sender that wants a peer to look at a
  Beacon memory item writes its ID into the ordinary text of the message, like any other
  identifier. There is no memory content type in the OAC spec (L1 §4 Q1: option (a),
  docs-only).
- **The receiver looks it up itself.** The receiving harness fetches the item through its
  own Beacon connection, with Beacon's `get_memory` tool ("Fetch one approved memory item
  by ID", `docs/cli/mcp.mdx@v1.3.29` L68), in its own turn. OAC does not resolve, fetch or
  expand the ID.
- **The ID is untrusted content.** It is a claim by the sender, rendered inside the
  untrusted body, never in the machine-set provenance block
  (`docs/planning/v0.1/06-security.md` §3(e); L1 §1 point 4). Whatever Beacon returns is
  untrusted text to the receiving harness as well.
- The exact shape and stability of a memory ID across Beacon releases is L1 item U2
  (UNVERIFIED — no versioned response schema at the pin; see
  `docs/planning/STATUS.md` "Open UNVERIFIED items"). OAC never parses it.

### 20.5 Privacy recommendation (Q3)

A recommendation with rationale, not a requirement (L1 §4 Q3):

- **Prefer Beacon Local mode** on a machine that runs OAC-enabled sessions. Local mode
  keeps agent history on the machine; "Nothing is forwarded unless you explicitly
  configure it" (`docs/get-started/quickstart.mdx@v1.3.29` L45-49).
- **If Beacon's hosted forwarding is on** (named "Beacon Managed" at the pin, "Beacon
  Cloud" at a later branch head, L1 §2 D1), use Metadata-only privacy for it:
  `beacon endpoint connect --privacy-mode metadata-only`
  (`docs/cli/endpoint-connect.mdx@v1.3.29` L104-106). It strips retained text, tool
  arguments/results, command output, raw fields and diffs "before buffering or upload"
  (`docs/security/retention-redaction.mdx@v1.3.29` L78-82).
- **Local capture still happens in both modes.** Metadata-only is a forwarding mode, not a
  capture mode. Beacon "may write prompt text, command output, raw attributes, tool input,
  and diff content to local JSONL", after redaction, sanitization, truncation and
  event-size limits (`SECURITY.md@v1.3.29` L54-57), into local `runtime.jsonl`, rotated
  at 10 MiB with five archives (`SECURITY.md@v1.3.29` L20). A message OAC delivered into a
  Beacon-instrumented session may therefore sit in that file, redacted and sanitized.
  Whether Beacon's capture records the content of an OAC-delivered message at all is L1
  item U1 (UNVERIFIED — no first-party statement at the pin; see
  `docs/planning/STATUS.md` "Open UNVERIFIED items").
- **Rationale.** Local mode keeps delivered peer messages on the machine; Metadata-only
  keeps their text off the hosted service when forwarding is on. Neither removes the local
  copy, so that residual is an **open risk**, not a mitigation. It is recorded as threat
  row 23 in `docs/planning/v0.1/06-security.md` §14, added by L4 (issue #169).

### 20.6 Scoping mismatch (Q4) — documented, not solved

- Beacon memory is **per resolved repository**: "Approved memory is scoped to the
  resolved project. Cross-project or user-global memory is not automatic."
  (`docs/concepts/cross-harness-memory.mdx@v1.3.29` L71-72). "A linked git worktree
  resolves to the same project as its main checkout." (`docs/cli/memory.mdx@v1.3.29`
  L35-36).
- OAC sessions are **per `working_directory`**
  (`docs/planning/decisions/C4-session-identity.md` §5).
- **Consequence.** Two peers in checkouts of different repositories see different memory,
  even for what a person calls "the same project"; a memory ID from one may not resolve
  for the other. Two peers in two worktrees of one repository are two OAC sessions but one
  Beacon project. OAC does not promise shared memory across peers, and does not map
  sessions to Beacon projects: that would be OAC configuring Beacon (L1 §4 Q4).

### 20.7 What OAC does not do

Each line is `oac-boundaries` #12 (OAC is not a "shared context manager",
`[PLANNING-PROMPT §10]`) or another L1 §1 boundary, applied to Beacon:

- OAC **never calls, spawns, proxies or configures Beacon.** It does not run `beacon` as a
  subprocess, call any `beacon` MCP tool, write or edit a harness's Beacon entry, or read
  harness MCP configuration to find one (§6; L1 §4 Q2).
- OAC **never reads or writes `memory.db`.**
- OAC **never attaches, injects, trims or summarizes memory.** It delivers only what a
  sender put in a message; recall happens in each harness's own turn
  (`[ADR-001 Boundary]` "MUST NOT ... implement inference/model routing/context
  management", `docs/planning/ADR-001.md` line 24).
- OAC **never holds credentials for Beacon's hosted service (Beacon Managed at the pin)
  or Jev**, nor any hosted-service OAuth
  session or personal token a harness holds for Beacon (`[ADR-001 Boundary]` "MUST NOT
  steal or reuse another harness's provider credentials"; L1 §1 point 5).
- **Beacon's activity log is not a mailbox, a presence source or a catch-up mechanism.**
  Using `search_activity` over `runtime.jsonl` to recover missed messages would be the
  deferred durable offline mailbox (`docs/planning/ADR-001.md` line 63) plus
  application-level polling, which `docs/planning/DESIGN.md` "Delivery semantics" rules
  out for adapters claiming active inbound support (L1 §1 point 6).

### 20.8 Checks run for this section

- Every OAC command above is in §5's table. Every `beacon` command carries a
  `path@v1.3.29` citation.
- No step has OAC call, spawn or configure Beacon (checked against `oac-boundaries` #12
  and §20.7).
- No new UNVERIFIED item: U1, U2 and U4 are L1's, already in `docs/planning/STATUS.md`
  "Open UNVERIFIED items" and `docs/planning/v0.1/11-risks.md` `RISK-BEACON`.
