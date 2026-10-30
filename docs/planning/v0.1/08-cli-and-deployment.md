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
first-party syntax. The wake and inject paths behind them are proven by G1 and G2 (G2 on
Windows only; macOS and Linux unexercised), but the `oac` side of each command does not
exist yet, so no command here has run end to end as written. Stated once here, in the
style of `docs/planning/v0.1/04-architecture.md` lines 14-23, rather than repeated at
each command. (Dated note, 2026-10-02, #122: this paragraph previously ended "not a
proven wake path", written before G2 passed.)

(Dated note, 2026-10-15, after PR #413 merged: the `oac` side of `oac start` and `oac mcp-shim` now exists on `main`, and `oac hook` reaches the daemon. The statement above that no command here has run end to end still holds for the live Claude Code and Codex legs, which are the operator's; `oac status`, `oac sessions` and `oac doctor` were not built then; see the dated note in §5.)

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
command forms are `oac start` / `oac status` / `oac sessions` / `oac doctor` (§5 below). *(Dated note, 2026-10-02, #228: `DESIGN.md` lines 21-24 now read `oac start` / `status` / `sessions` / `doctor`, and C12 is closed. The block above is the pre-rename form, kept as history.)*

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
| `oac start` | Start (or confirm running) the per-device daemon. **Idempotent**: running it while a daemon is already up for this device is a no-op success, not an error. **Foreground by default**; `--detach` self-daemonizes into the background. The daemon's log lines go to standard error and also to a **log file that survives a crash** (G10, #71 slice 2; below the table). `--detach` reads the child's standard error until it serves or ends, so a start that fails shows the child's own reason and the log's path. `oac start --help` prints the log path. **The daemon serves a `codex` endpoint beside `claude`** (G8 slice S5, #433): its adapter's carrier, the connection to Codex's app-server control socket (`<Codex home>/app-server-control/app-server-control.sock`), is opened by the daemon when it starts, if the socket is there, and again when a Codex shim connects and the adapter has no carrier or has lost it, never on a timer; the shim connection is handed over only after that attempt. With no socket, Codex shim connections are closed (the shim exits 1). The daemon never starts, stops or restarts Codex's app-server (lead decision Q11). **`--json`** (G10, #71 slice 7; lead decision 2026-10-20) prints **exactly one** canonical object with `"oac_cli":1` on standard output (members below) and nothing else there; the daemon's log lines and the words stay on standard error. In the foreground the object is printed **once the daemon serves**, before the command blocks until the daemon is stopped, and there is no second object at the end (a clean stop is exit `0` and silence); a start that fails prints the error object instead of the result. | `0` on a daemon now running (newly started or already up); non-zero on failure to bind the IPC endpoint or start. `--json` changes no exit status. |
| `oac status` | Report daemon up/down, device id, transport state and adapter state (G10, #71 slice 3; lead approval 2026-10-18). Asks the daemon's reserved `control` endpoint (`status` op: same bounds as `stop`, one JSON line back) for a snapshot the core takes with no lock held across a transport or adapter call (`spec/interfaces.md` Tables 5.2 and 6.4: the core is the caller of `health`). Shows: the daemon's pid, version and uptime; the test namespace, if any; the device id (the public key id); the endpoints served (labels); the transport's `health`; each adapter's `health`; this device's own sessions (session id, harness label if known, `online` or `withheld`, agreed `active_inbound`); counters (open connections, unbound attachments, pending and held signals, trusted devices, grants, discarded frames); and the path of the daemon's log file (below the table). It never shows a harness-native session id or a working directory ([IFC-TYP-020], [IFC-TYP-092]). `--json` prints one canonical object with `"oac_cli":1`, `"ok"`, `"running"` and the members below; diagnostics go to standard error. | `0` the daemon answered; `1` it did not answer in time, or answered something else; `2` usage; `3` the daemon is not running; `4` the endpoint is another user's, busy (retried until the deadline), or the daemon speaks another control-protocol version. Scriptable as a liveness check (`3` is "down"). |
| `oac sessions` | List this device's own sessions (G10, #71 slice 4; lead approval 2026-10-18, Q3: own sessions only; remote sessions come later, gated by grants). Asks the daemon for the same `status` reply as `oac status` (same bounds, retries and exit codes; no separate control op) and shows only its session rows: one row per session (session id, harness label, agreed extension and major version, active inbound yes or no, state `online` or `withheld`), then the open attachments with no session bound, counted per harness label. An empty list prints `no sessions`. It shows the full session id (an address, not a permission; lead decision Q4, residual recorded in `11-risks.md`, RISK-SESSION-ID-DISCLOSURE) and nothing else that could identify a project: no working directory, harness-native id or display name ([IFC-TYP-020], [IFC-TYP-092]). `--json` prints one canonical object with `"oac_cli":1`, `"ok"`, `"running"` and the members below; diagnostics go to standard error. | `0` the daemon answered, an empty list included; `1` it did not answer in time, or answered something else; `2` usage; `3` the daemon is not running; `4` the endpoint is another user's, busy (retried until the deadline), or the daemon speaks another control-protocol version. |
| `oac doctor` | Preflight check (G10, #71 slice 5; lead approval 2026-10-18). Runs the checks of §6 and reports each as `PASS`, `WARN`, `FAIL` or `SKIP` with a one-line neutral detail; the human form is one line per check and a count. `--json` prints one canonical object with `"oac_cli":1`, `"ok"` (no check failed) and `"checks"`: `[{"name","result":"pass"\|"warn"\|"fail"\|"skip","detail"}]`. It asks the running daemon once (the `status` op of `oac status`); it never starts a transport or an adapter, never holds key material, and never reads a harness's credential file, database or configuration directory. | `0` when no check fails (warnings and skips do not count); `1` when one does; `2` usage. It does not use `3` or `4`: a missing or unreachable daemon is the `daemon` check's `FAIL`. |
| `oac stop` | Stop the running per-device daemon cleanly (G10, #71; lead approval 2026-10-18). Connects to the daemon's reserved `control` endpoint (same lock, same peer authentication as a harness endpoint; no adapter, no session), asks it to stop, and waits at most about 15 s for the daemon to say it has finished: endpoints closed, withdrawals sent, transport shut down and the lock released, so a new `oac start` may follow at once. **Never kills anything**: on timeout it prints the daemon's pid and exits `1`. With no daemon running it prints "not running" and exits `0` (there is nothing to stop). `--json` prints one canonical JSON object on standard output with `"oac_cli":1` (and `"was_running"`); diagnostics go to standard error. | `0` stopped, or not running; `1` timed out or failed; `2` usage; `4` the endpoint is another user's, busy (retried until the deadline), or the daemon speaks another control-protocol version. |
| `oac mcp-shim --harness <claude or codex> [--json]` | The stdio MCP server Claude Code and Codex spawn as their child process; connects to the daemon over local IPC. **Not meant to be typed by hand** — a harness config entry invokes it, a person does not run it directly. **Its standard output is the harness's MCP stream**: only the daemon's bytes are ever written to it. `--json` (slice 7) changes only the form of the shim's own diagnostics, which are always on standard error: one canonical object per line instead of a sentence (members below). It adds nothing to standard output and changes no exit status. | `0` the harness closed the input; `1` the daemon could not be reached or ended the connection; `2` usage. |
| `oac hook [--json]` | The Claude Code `SessionStart` hook handler (G9, #70): carries the hook input to the daemon as one line (`spec/bindings/mcp.md` section 8.1). **Not meant to be typed by hand.** Writes **nothing** to standard output (a `SessionStart` hook's standard output is added to Claude's context, [MCPB-CLD-007]). `--json` (slice 7) changes only the form of its one diagnostic on standard error: a canonical object instead of a sentence (members below). It adds nothing to standard output and changes no exit status. | Always `0`: a hook that exits non-zero only shows a hook-error notice. |
| *(named, semantics deferred)* | A pairing subcommand and any device-setup subcommand a future launch decision needs. Named here as a reserved placeholder; no flags or output shape are specified. Semantics owned by `docs/planning/decisions/C5-envelope-auth.md` §10(b) (pairing) — not designed here. | Not specified. |

**Exit codes of the verbs (G10, #71; approved 2026-10-18; checked against `cli/src/verbs.rs`, `commands.rs` and `shim.rs` on 2026-10-20).** One table for every verb. The `--json` form of `status`, `sessions`, `doctor` and `stop` prints one canonical (RFC 8785) JSON object on standard output with `"oac_cli":1`; diagnostics go to standard error. Every verb takes `--json` (slice 7): `start` prints its one result object, and `mcp-shim` and `hook`, whose standard output belongs to the harness, use it for their diagnostics on standard error only (below).

| Code | Meaning |
|---|---|
| `0` | OK. For `oac stop`, also "the daemon was not running". For `oac start`, a daemon now running (new or already up) and `--help`. For `oac doctor`, no check failed (warnings and skips do not count). For `oac mcp-shim`, the harness closed its input. `oac hook` always exits `0`, so a misfire adds no hook-error notice. |
| `1` | Failed, or timed out: `oac stop` after about 15 s (the daemon's pid is printed; nothing is killed); `oac start` could not start or bind, or `OAC_SCOPE` is set; `oac doctor` with a failing check; `oac status` and `oac sessions` when the daemon did not answer in time or answered something else; `oac mcp-shim` could not reach the daemon, or the daemon ended the connection. |
| `2` | Usage error: an unknown flag or command, or no command. `oac mcp-shim` without `--harness <claude\|codex>`. |
| `3` | The daemon is not running, for the verbs that need one: `status` and `sessions`. `stop` uses `0` (lead decision Q1) and `doctor` reports it as the `daemon` check's `FAIL` (exit `1`). |
| `4` | The daemon is unreachable or incompatible: the endpoint is served by another user's process, every instance stayed busy until the deadline, or the control-protocol version differs. `status`, `sessions` and `stop` only. |

A daemon built before this change has no control endpoint (v0.1 is unreleased), so `oac stop` reports it as not running; end such a process yourself.

**The error object (`status`, `sessions`, `stop` with `--json`).** `{"error":"<kind>","oac_cli":1,"ok":false,...}` on standard output, with the human message on standard error. `error` is `not_running` (`status`, `sessions`; `running` is `false`), `version` or `busy` (`running` is `true`), `unreachable` or `failed` (with `detail`, a string; no `running` member) and, for `stop` only, `timeout` (with `pid` when known). The mapping to exit codes is the table above: `not_running` `3`; `version`, `busy`, `unreachable` `4`; `failed`, `timeout` `1`. `stop` carries `was_running` in place of `running`, except `unreachable` and `failed`, which carry neither. A usage error (`2`) prints no JSON, for any verb. Neither does a failure to make the user's scope (`user_scope`, for example no home or state directory): `stop`, `status` and `sessions` then exit `1` with the reason on standard error only, whatever `--json` says; `doctor` is the exception, as it reports a scope it cannot make as a finding.

**`oac stop --json` members (G10, #71, slice 1).** Success: `{"oac_cli":1,"ok":true,"stopped":<bool>,"was_running":<bool>}` and, when the daemon said it, `"pid"`. Both booleans are `true` when the daemon confirmed that it had finished and `false` when there was no daemon (exit `0` either way). Failure: the error object above (exit `1` or `4`).

**`oac start --json` members (G10, #71, slice 7).** One object, on standard output. Result: `{"already_running":<bool>,"detached":<bool>,"log"?:<string>,"oac_cli":1,"ok":true,"pid"?:<number>}`. `already_running` is `true` when this command did not start the daemon that serves (one was up, or another start won the lock); `detached` is `true` with `--detach`; `pid` is the serving daemon's (the process itself in the foreground; with `--detach` the pid its control endpoint names, which is the child's when the child is the daemon, else the child's own pid when the endpoint does not say; absent for a daemon that was already up whose endpoint does not say); `log` is the path of the daemon's log file, absent when the state directory cannot be found. Failure: the error object above with `error` `failed` (`OAC_SCOPE` is set, the scope or runtime could not be made, the daemon ended at start-up or could not be spawned) or `timeout` (nothing served within 20 s with `--detach`), and the same exit `1` as without `--json`. `oac start` still refuses `OAC_SCOPE`. The words (including the daemon's relayed standard error) stay on standard error; the child of `--detach` is the plain `oac start` and is never given `--json`.

**`oac mcp-shim --json` diagnostics (G10, #71, slice 7).** Each diagnostic is one canonical line on **standard error**, never on standard output: `{"detail":<string>,"error":<kind>,"namespace"?:<string>,"oac_cli":1,"ok":false}`, where `error` is `not_running` (no daemon; the detail names `oac start`), `unreachable` (another user's endpoint, busy, or an operating-system refusal), `failed` (the scope or runtime could not be made) or `daemon_closed` (the daemon ended the connection; shim only) and `namespace` is present in a test namespace (the sentence form says the namespace on a line of its own). A successful session writes no diagnostic. Exit statuses are those of the table above, with and without `--json`. `--json` may stand before or after `--harness <label>`.

**`oac hook --json` diagnostic (G10, #71, slice 7).** One outcome line on **standard error** per run, in place of the sentence (preceded by an `arguments_ignored` notice line when other arguments are given); standard output stays empty and the exit status stays `0`. Sent: `{"oac_cli":1,"ok":true,"sent":true}`. Nothing sent by design: `{"oac_cli":1,"ok":true,"reason":<r>,"sent":false}` with `reason` one of `unreadable`, `too_large`, `not_an_object`, `not_a_hook_input`, `not_a_signal`. The daemon was not reached: `{"detail":<text>,"error":<kind>,"oac_cli":1,"ok":false,"sent":false}` with `error` `not_running` or `unreachable`. It holds nothing from the hook input. An argument other than `--json` is noted on standard error as `{"notice":"arguments_ignored","oac_cli":1}` and never fails the hook. **Why this is safe, and why it is limited to standard error.** The hooks reference (https://code.claude.com/docs/en/hooks, section on `SessionStart` hooks, retrieved 2026-10-20) says that with exit `0` the hook's standard output, as plain text or as a JSON object, is added to Claude's context (or applied as hook output), while "Claude never sees stderr from a hook that exits 0" (it is read only with debug logging); any other exit status shows the user a notice. So the hook's output could change Claude Code's behaviour only through standard output or a non-zero exit, and `--json` touches neither: the handler has no handle to standard output (`verbs::dispatch` discards it), and the exit is fixed at `0`.

**`oac doctor --json` members (G10, #71, slice 5).** `{"checks":[{"detail","name","result"}],"ok":<bool>,"oac_cli":1}`. `result` is `pass`, `warn`, `fail` or `skip`; `name` is one of the §6 names, in the §6 order; `ok` is `true` when no check failed (exit `0`; `1` otherwise). A daemon that is not running is a `FAIL` row, never the error object, and `doctor` has no `3` or `4`.

**`oac status --json` members (G10, #71, slice 3).** One canonical object: `oac_cli` (`1`), `ok` (`true`), `running` (`true`), then the daemon's report. A daemon that is not running or not reachable prints the error object above, and the human message goes to standard error.

**`oac sessions --json` members (G10, #71, slice 4).** One canonical object: `oac_cli` (`1`), `ok` (`true`), `running` (`true`), `sessions_total` and `sessions` (as in the `oac status` table above, including `extension` and `major`), and `unbound`: `[{"harness","unbound_attachments"}]`, one entry per adapter, the harness label and the number of its open attachments with no session bound. A daemon that is not running or not reachable prints the same error object as `oac status`.

| Member | Meaning |
|---|---|
| `pid`, `version`, `uptime_ms` | The daemon process, the `oac` version that built it, and milliseconds since it took the lock. |
| `namespace` | The test namespace, when the daemon runs in one; absent otherwise. |
| `device_id` | This device's public key id. |
| `endpoints` | The labels of the endpoints it serves: each harness, then `control`. |
| `transport` | `{"state":"healthy"\|"degraded"\|"unavailable","detail"?}`: the transport's `health` (`spec/interfaces.md` §4.10, §6.8). |
| `adapters` | `[{"label","state","detail"?,"unbound_attachments"}]`: each adapter's `health` (§5.7), labelled by the harness label of its endpoint, and the number of its open attachments with no session bound. |
| `sessions_total`, `sessions` | This device's own sessions, at most 256 listed: `{"session_id","harness"?,"state":"online"\|"withheld","extension"?,"major"?,"active_inbound"?,"content_types"?,"max_envelope_octets"?}`. The capability members are the ones agreed for the version this implementation speaks (`spec/session-channels.md` §6.5). |
| `counts` | `open_connections`, `unbound_attachments`, `pending_signals`, `held_signals`, `trusted_devices` (other devices; this device's own key is not counted), `grants`, `discarded_frames`. |
| `log` | The path of the daemon's log file; absent when the daemon runs without one. |

The `status` op is bounded like `stop`: a request of at most 1 KiB within 2 s, at most 4 control connections. One snapshot is computed at a time, on a detached thread, and within 5 s. A second request while it is still within its deadline is answered `busy` (and `oac status` asks again); once it is past its deadline every request is answered `timeout` (exit `1`) at once, so a `health` that never returns leaves one stuck thread, never more, and does not keep the daemon from ending after `oac stop`. An adapter or transport whose `health` hangs is answered `timeout`, not waited for. A `detail` is written by the component that owns it, which may not put a credential, a native address, a harness-native id or a working directory in it ([IFC-TYP-092]); `oac status` prints it with control characters replaced.

**The daemon log file (G10, #71, slice 2; lead decision 2026-10-18, Q2: "logs should always be preserved to survive a crash").**

- **Where.** `LOCALAPPDATA\OAC\logs\daemon.log` on Windows and `~/.local/state/OAC/logs/daemon.log` on Unix: the per-user state directory the relay's TLS material also uses. The previous file is `daemon.log.1`. A test namespace (`OAC_SCOPE`, test processes only; `oac start` refuses it) writes under `logs/ns-<name>/` instead and never touches the user's own log. `oac start --help` prints the path. `oac status` prints the path of the running daemon's log file (the `log` member); there is no verb that reads it, so open the file.
- **Who can read it.** The user only. Unix: directory `0700`, file `0600`; a directory or file that is not the user's or is open to others is refused, never repaired, and a symlink in the file's place is refused (`O_NOFOLLOW`). Windows: the directory and the file carry a protected DACL with one ACE, for the user (the pattern of the named pipes, C2 §4); the DACL is read back before the file is used. An administrator or root remains inside the operating system's trust boundary.
- **Crash safety, and what it costs.** Each line is one `write` on a file opened for append, then a flush: the line is in the operating system's file cache when the call returns, so it survives the end of the daemon process at any instant, a kill or a crash included. At most the line being written at that instant is lost; a line is never split across the two files. It is **not fsynced**: an fsync per line is a disk round trip (milliseconds, on Windows `FlushFileBuffers`), and the requirement is a daemon crash, not a machine crash; a power loss or an operating-system crash can lose the last seconds.
- **Bounded.** At 1 MiB the file is renamed to `daemon.log.1` (replacing the previous one) and a new one begun: at most two files, about 2 MiB, however long the daemon runs. A line is cut at 4 KiB. Each line begins `unix_ms=<time> `. **A rotation that fails never deletes the current file** (a sharing violation on Windows, an antivirus scan, a viewer holding `.1`): the file keeps growing and each line retries; past 4 MiB it is truncated and begun again with a first line `log restarted: <N> bytes discarded, rotation failing (<error kind>)`, so the newest lines are kept (`daemon.log.1` is not touched).
- **When it is opened, and panics.** The daemon opens the file only after it holds the lock, so a second `oac start` never touches the running daemon's file; its "already running" line goes to standard error only. A panic appends `oac-daemon: panicked at <file>:<line>` (never the panic message) before the default panic hook runs, since a detached daemon's standard error is a closed pipe.
- **Content.** The same contract as the standard-error log: connection labels, session ids and requirement ids; no message content, native session id, working directory or key.
- **A log never stops the daemon.** If the file cannot be opened, the daemon prints one warning on standard error and runs without it; if a write fails later, the file is kept and tried again on the next line, with one warning on standard error per run of failures.
- **A failed `--detach`.** The child's standard error is a pipe that `oac start --detach` relays (at most 16 KiB, control characters made `?`) until the daemon serves or ends; a start that ends non-zero prints the child's own reason, its exit status and the log's path, in place of "run `oac start` in a terminal".

(Dated note, 2026-10-20, #71: the `credential_store` row of section 6 had its WARN, FAIL and SKIP cells misplaced; corrected to `cli/src/doctor.rs`, after a live run on WSL showed the FAIL case. `oac status` (slice 3), `oac sessions` (slice 4) and `oac doctor` (slice 5) are built. Slice 6 (this change) consolidates the exit codes and the `--json` shapes above and corrects the error-object and `stop` members to the code.)

(Dated note, 2026-10-18, #71: `oac stop` and the reserved `control` endpoint are on this branch of G10 slice 1. `oac start --detach` probes the `control` endpoint, not the `claude` endpoint, for "already running" and for "now serving", so the probe no longer takes an adapter place (`11-risks.md` row 86).)

---

## 6. `oac doctor` check list

Expanded from the doctor row in §5, per `docs/planning/decisions/C2-process-model.md`
§6 and `docs/planning/PINS.md`'s constraint floors, as built in G10 slice 5 (#71, lead
approval 2026-10-18). Each check is `PASS`, `WARN`, `FAIL` or `SKIP`.

| Check | What it does | `PASS` | `WARN` | `FAIL` | `SKIP` |
|---|---|---|---|---|---|
| `daemon` | Connects to the `control` endpoint, with the client's server check, and asks for `status` | running | the endpoint stayed busy | "no daemon; run `oac start`"; "endpoint served by another user"; another control-protocol version; no answer | |
| `ipc_permissions` | Unix: the run directory is a real directory owned by this user and closed to others (the check in `cli/src/ipc/unix.rs`, made when the scope is made). Windows: the client's read-back of the pipe's owner and access list, made while connecting (`cli/src/ipc/windows.rs`) | it is this user's alone | | it is not | Windows, no daemon or the endpoint stayed busy: no pipe to inspect |
| `credential_store` | Writes, reads back and deletes a fixed non-secret value under service `io.github.rossgraeber.oac` (a test namespace gets its own service) and entry `doctor-probe`. **Never `device-key`**, and key material stays in the daemon. On Unix it also says whether the encrypted-file fallback's key file exists, by path only; the file is never opened | it worked | unavailable, and the Unix fallback key file exists (the daemon uses it): "credential store unavailable; the daemon uses the encrypted-file fallback" | unavailable with no fallback key file (or the host cannot say), locked, or the probe failed: "credential store unavailable ...", with the fallback state appended | `OAC_SCOPE` is not a valid namespace; or the test-only switch `OAC_DOCTOR_NO_CREDENTIAL_PROBE` inside a test namespace |
| `transport` | The `transport` health in the daemon's `status` reply; doctor starts no transport (#305 owner comment) | healthy | | degraded or unavailable: "the transport peer cannot link" (local mode has no scouting) | no daemon |
| `mcp_revision` | (a) this process's `MCP_PROTOCOL_NEGOTIATION`; (b) the Claude adapter's `health`, which is `Degraded` after a connection sent the `server/discover` probe or asked for a revision this server does not speak and ended with no legacy `initialize` answered (counted since the daemon started) | unset or `legacy`, and no such connection | set to anything else (it affects harnesses started from this environment); or a connection that never negotiated a legacy revision: "a Claude session did not negotiate a legacy MCP revision (counted since the daemon started)". **WARN, not FAIL** (lead decision L1, 2026-10-20): the counter is cumulative and never decays, so it cannot tell a session broken now from one fixed since; revisit when a decay or per-attachment rule exists | | |
| `pairing` | `trusted_devices` and `grants` in the `status` reply (the pairing verb is #437) | either is above 0 | both 0: "no pairings or grants; default deny, nothing is delivered" | | no daemon |
| `sessions` | `adapters[].unbound_attachments`, `sessions_total` and the session rows' `harness` in the same `status` reply (G11, #72). Reads no harness file | at least one session is attached and no connection is waiting to be bound | a connection is open with no session bound (Claude: the `SessionStart` hook has not reported; Codex: the pairing has not completed), no session is attached at all, or a listed session is `withheld` (counts only; an unbound Codex connection is normal until the first OAC tool call in the thread) | | no daemon |
| `claude_version` | Runs `claude --version`, a documented flag, only if `claude` is on `PATH` (absolute `PATH` entries only), with a 5 s limit, no input and at most 4 KiB read; compares with the minimum `v2.1.282` of `docs/planning/PINS.md` | at or above the minimum | below it, not recognised, or `claude` did not answer. Never `FAIL`: warn on a version, never gate (#216) | | `claude` is not on `PATH` (a host that uses only another harness has nothing wrong) |
| `codex` | Connects to the Codex app-server control socket (`<Codex home>/app-server-control/app-server-control.sock`; `OAC_CODEX_CONTROL_SOCKET` overrides the path) and closes at once, **sending and reading nothing**; reads no other path under the Codex home. Windows and Unix both: an `AF_UNIX` socket, the server's process id from the operating system (Windows: `SIO_AF_UNIX_GETPEERPID`, UNVERIFIED on Microsoft Learn, `PINS.md`) and its owner compared with this user's | the socket accepted a connection and its server is this user's process | the socket is there and did not accept (a stale socket, or a server that is not accepting), or the owner of its server could not be verified (the same on every platform) | the server is another user's process (confirmed) | there is no socket (Codex's app-server daemon is not running; **OAC never starts it**), or no home directory to look in |
| `launch_env` | This process's own environment, by name: `CLAUDE_CODE_USE_BEDROCK`, `_VERTEX`, `_FOUNDRY`, `_MANTLE` (channels are not available on those providers) and `CODEX_EXEC_SERVER_URL` (Codex skips implicit daemon attachment). Names only, never a value; no settings file is read (G11, #72) | none is set, or each is `0`, `false`, `no`, `off` or empty | one is set | | |

(Dated note, 2026-10-30, #72, G11: the `sessions` and `launch_env` rows are new. Each closes a launch failure mode that had no check, so that every failure mode in [`docs/launch.md`](../../launch.md) either has a check or says why none is possible. Both are built from data the doctor already has (the `status` reply, this process's environment) and read no harness file, so `scripts/check-doctor-boundary.mjs` is unchanged. `launch_env` reads this process's environment, as `mcp_revision` already does; a provider selector set in a settings file's `env` block is not visible to it. Both are WARN, never FAIL: they do not change the exit status. The doctor still has no check for a missing `--dangerously-load-development-channels` (Claude Code acknowledges no channel notification and the session still binds), for `channelsEnabled` or `allowedChannelPlugins` (managed settings are Claude's configuration, which the doctor never reads), or for a signed-out Claude Code (credentials).)

**Deviations from C2 §6, approved 2026-10-18.** The Codex daemon check was `SKIP` until the
daemon wired the Codex adapter (#433; it does since G8 slice S5, dated note below), where C2 §6 lists "Codex daemon reachability". The
transport check reads the daemon's own `transport` health instead of binding a listener, and
its failure is worded "the transport peer cannot link". The Claude check is a warning on the
version, and the negotiation variable check gains the adapter's evidence of a session that
did not negotiate. The report shape is per check, not all-or-nothing: the exit status is `0`
unless a check fails.

(Dated note, 2026-10-22, #433, G8 slice S5: the `codex` check reports the control socket as in the table. It is the one place `cli/src` names the Codex home, in `cli/src/codex.rs` on two lines, to build the socket's path (lead approval 2026-10-21, approval 2: `scripts/check-doctor-boundary.mjs` allows exactly those lines). It looks at that one path and connects to it; it reads no credential file, database, rollout or configuration. The check says nothing of the daemon's carrier: whether the daemon has connected to the socket is in `oac status`, as the `codex` adapter's `health`.)

**Never read.** Doctor never reads a harness's credential file (`auth.json`,
`.credentials.json`), a `*.sqlite` file, or the contents of `CODEX_HOME` or
`CLAUDE_CONFIG_DIR` or their default locations (`~/.codex`, `~/.claude/`, `~/.claude.json`), and
never scrapes a UI (ADR-001 Boundary). `scripts/local-ci.mjs` step `doctor-boundary` fails if a
file in `cli/src` names any of those words.

**Report shape.** Each check reports on its own, whatever the others found. `oac doctor`
exits `0` unless a check fails.

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

(Dated note, 2026-10-30, #72, G11: the user-facing form of this section, with per-shell commands, the `.mcp.json` and hook registration, the distribution constraints and the failure table, is [`docs/launch.md`](../../launch.md). `--channels` takes `plugin:<name>@<marketplace>` entries only and "doesn't extend the bypass to the `--channels` entries" (channels reference, retrieved 2026-10-30, Claude Code `2.1.312`), so no `--channels` form is printed for OAC; drift D4, already recorded in `docs/planning/REVERIFICATION-B2.md`, still holds.)

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
codex mcp add oac -- oac mcp-shim --harness codex
```

(Dated note, 2026-10-22, #69: this line read `codex mcp add oac -- oac mcp-shim` until now. `oac mcp-shim` without `--harness <claude or codex>` exits `2` (usage, §5), so the registration as written could not work. The Codex registration names its harness. The part after `--` is what Codex runs; `codex mcp add` itself is as quoted above. A registered stdio MCP server was also seen to start when a Codex session opened, not at its first tool call (once, for one server): the G8 S1 capture saw `initialize` and `tools/list` arrive 57.6 s before the first prompt, on Codex `0.163.0` (`docs/planning/gates/fixtures/g8-codex-tools/README.md`).)

**The registered name (`oac` above) is a daemon setting.** Codex reports the name given to
`codex mcp add <name>` as the `server` of each of the shim's tool calls, and the Codex
pairing (`spec/bindings/mcp.md` §4.5.2, "Registered name") recognises a call only by it. So
the name must equal the daemon's **`OAC_CODEX_REGISTERED_NAME`** environment variable, read by
`oac start` (and by the live-leg example daemon) at start. Unset or empty, it is `oac`. It follows
the same pattern as `OAC_CODEX_CONTROL_SOCKET` (environment; `oac start` has no flag for it,
§4's precedence being flag, then environment). To register under another name, for example
`oac-work`:

```
codex mcp add oac-work -- oac mcp-shim --harness codex
OAC_CODEX_REGISTERED_NAME=oac-work oac start
```

A value is refused at start, with exit `1` (the error object under `--json`) and nothing
started, if it is longer than 64 characters or holds a character outside ASCII letters, digits
and `-`, `_`, `:`, `@`, `/`, `.` (the characters `codex mcp add` accepts in a server name,
read in Codex's source on 2026-10-26: UNVERIFIED against a Codex build; the 64-character bound
is OAC's, Codex sets none). A mismatch is not an error Codex or the shim can see: the shim
connects and gets the pairing window and its value, but Codex's report of the call names
another server, so nothing binds. The daemon's log says so: once per distinct name, "an
mcpToolCall from server X, not the registered name Y" (the names only: no thread id and no pairing value),
and the line that reports the carrier connected names the registered name in force. The
adapter keeps the counts (`other_server`, `unmatched_reports` and the others of
`PairingDiagnostics`). (Dated note, 2026-10-26, #69: the name was fixed at `oac` in the
product until now, and a registration under any other name, as a live run made, was dropped
silently.)

(Dated note, 2026-10-30, #72, G11: the full Codex launch sequence is in [`docs/launch.md`](../../launch.md): `codex app-server daemon start` (the user's, never OAC's), this registration, then plain `codex`. The `codex --remote` fallback stays deferred, per `adapters/codex/README.md`.)

This is the **supported outbound tool-registration** path
(`docs/planning/decisions/C2-process-model.md` §6) — distinct from the live-inject path
§10 below names, which is app-server runtime behaviour, not a CLI command a user types.

---

## 10. G2 target statement — proven on Windows

Separate from §9's command, per the task breakdown. (Dated note, 2026-10-02, #122: this
heading previously read "pending, not proven", written before G2 passed.)

The live-inject path — daemon-attach, `codex app-server daemon start`, control socket
`CODEX_HOME/app-server-control/app-server-control.sock` — is confirmed working per gate
**G2**'s verdict. Per `docs/planning/STATUS.md`'s Gate verdicts table, **G2 is `PASS`**
(`0.157.1`, re-run 2026-09-26, Windows only; macOS and Linux remain unexercised). This
file does not present the inject path as a command a user types — it is app-server
runtime behaviour, consumed by the daemon's own Codex app-server client
(`docs/planning/decisions/C2-process-model.md` §1 item 4), not invoked from a shell.

G2's recorded fallback, per `docs/planning/STATUS.md`'s Gate verdicts table: "OAC-owned
app-server with `codex --remote`." That fallback is explicitly deferred with a reason: `adapters/codex/README.md`, section "Deferred: the `codex --remote` fallback (G7, #68)".

**Items carried from** `docs/planning/decisions/C2-process-model.md` §2 leg 4, §10:

- Whether implicit Codex daemon attach is enabled by default at runtime — **closed on
  Windows by G2**, at `0.154.0` (commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`) and
  again at `0.157.1` (re-run 2026-09-26, `docs/planning/gates/G2-result.md`); still open
  on macOS and Linux (`docs/planning/v0.1/11-risks.md` row 3). (Dated note, 2026-10-02,
  #122: this bullet previously called the runtime behaviour "G2's own go/no-go
  question", written before G2 passed.)
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
| Claude Code Channels | research preview | floating — last observed per `docs/planning/PINS.md` (2026-10-01, #216: minimum and last tested versions; warns, never gates) | `adapters/claude/` (UNVERIFIED — module name not yet fixed in `DESIGN.md`; register entry C11, `docs/planning/v0.1/07-repository-and-dependencies.md` §4(b)) |
| Codex app-server / daemon-attach | experimental (per-method gating) | floating — last observed per `docs/planning/PINS.md` (2026-10-01, #216: minimum and last tested versions; warns, never gates) | `adapters/codex/` (UNVERIFIED — same C11, `docs/planning/v0.1/07-repository-and-dependencies.md` §4(b)) |
| `keyring` | supported | `4.2.0` (`docs/planning/PINS.md`) | not applicable — general-purpose crate, not a provider surface |
| `interprocess` | supported | `2.4.4`, candidate (`docs/planning/PINS.md`) | not applicable — general-purpose crate, not a provider surface |

Note, 2026-10-01, issue #186: the Claude Code Channels and Codex Pin cells previously
read `v2.1.274` and `@openai/codex@0.154.0` @ commit
`6b9826e3aa83b1a5947db50f4332cb9c65f1b340`, the fixed pins this file was written
against. Those `docs/planning/PINS.md` rows went floating (Claude Code 2026-09-27, Codex
2026-09-26); see each row's "Floating-version policy" for the last observed version.
(Dated note, 2026-10-01, #216: the row now records minimum and last tested versions instead, in each row's
"Version policy"; a version change warns, never gates.)

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
      (Codex: `codex mcp add oac -- oac mcp-shim --harness codex`).
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
- **OAC installed.** OAC is not released. On `main`, `oac start` (with `--detach`) and `oac mcp-shim` exist (PR #413), and `oac hook` reaches the daemon. `oac status`, `oac sessions` and `oac doctor` exist (G10 slices 3 to 5). The commands below are the documented targets; see the "Gate-verdict caveat" at the top of this file.
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
   Codex launch was L1 item U4. L2 refuted it for OAC's planned launch paths: Beacon
   replaces only the `[otel]` / `[otel.*]` tables, with one edge case (a table header
   carrying a trailing comment right after `[otel]` is dropped), and it uses its own
   server names (L1 §11 item 4).
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

Only the commands in §5, §7 and §9 are used. `oac start` and `oac mcp-shim` are on `main` (PR #413); the live run is the operator's. `oac start` writes its log to a file that survives a crash (§5). `oac start` serves on the real Zenoh transport since PR #425 (a multi-thread runtime; the first live run had panicked on a current-thread one). The "Gate-verdict caveat" at the top of this file applies to each.

1. Start the per-device daemon with `oac start` (§5), or `oac start --detach` to leave it running in the background. `oac doctor` runs the preflight (§6), `oac status` and `oac sessions` show the daemon and this device's own sessions, and `oac stop` ends it cleanly (§5). None of them reads, wraps or configures Beacon (§6, last paragraph).
2. Claude Code: launch the session with the development-channel command in §7. The
   consent dialog in §8 is unchanged.
3. Codex: register `oac mcp-shim` once with the command in §9. The daemon serves the `codex` endpoint (G8 slice S5, #433) and connects to Codex's app-server control socket when it is there (Codex's own `codex app-server daemon start` makes it); `oac doctor` reports the socket in its `codex` check.

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
- The shape of a memory ID is settled at the pin (L1 item U2, CONFIRMED by L2 in L1 §11
  item 2): `memory_` plus 32 hex characters, derived from the memory's content, with no
  cross-release promise. A superseded memory's ID stops resolving. OAC never parses it.

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
  Whether Beacon's capture records the content of an OAC-delivered message is L1 item U1
  (UNVERIFIED, narrowed by L2 in L1 §11 item 1: Beacon records whatever the harness
  reports as a prompt, and OAC's outbound tool-call arguments are confirmed captured;
  whether the harness reports a delivered message awaits the herdr-driven live leg, L1
  §12; see `docs/planning/STATUS.md` "Open UNVERIFIED items").
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
- No new UNVERIFIED item: U1, U2 and U4 are L1's. After L2 (L1 §11), only U1 is still
  in `docs/planning/STATUS.md` "Open UNVERIFIED items"; U2 and U4 are closed, and
  `docs/planning/v0.1/11-risks.md` rows 53-56 (`RISK-BEACON`) record all four.
