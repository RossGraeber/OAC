# C2: Process model, local IPC, CLI surface, and config model

**Issue:** #14 (Epic C, backlog key `C2`). **Depends on:** #12. **Source:**
PLANNING-PROMPT.md §5.2, §5.11.

**Status:** Decided.

**Standalone-ledger note.** This file lives at `docs/planning/decisions/` because
`docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4) does not exist yet.
It folds into that file, unedited in substance, once A4 lands — mirroring
`docs/planning/ADR-001-AMENDMENTS.md`'s own standalone-ledger rationale (see that file's
opening paragraph). `docs/planning/STATUS.md` carries a one-line pointer to this file
until then.

---

## 1. Choice

OAC's process model is **option (a): one long-lived per-device `oac` daemon, plus thin
`oac mcp-shim` stdio child processes.** Claude Code and Codex each spawn an `oac
mcp-shim` process as their MCP server; each shim is a thin stdio client that connects
to the daemon over local IPC (§4) and does nothing else.

The daemon owns:

1. **The Zenoh peer** — the one process per device that holds Zenoh's discovery,
   liveliness, and pub/sub state.
2. **Device identity and key material** — the per-device signing key, read from and
   written to the OS credential store via `keyring` `4.2.0` (`docs/planning/PINS.md` —
   `keyring`; C1 §8).
3. **Policy and allowlists** — sender allowlists, pairing state, and ACL decisions, in
   one place instead of duplicated per session.
4. **The Codex app-server client** — one client of the Codex app-server daemon
   (§3 below), not one per OAC session.

This is stated as the choice, not offered as one option among several.

## 2. Decisive evidence

**Leg 1 — Claude Code forces a per-session server process at its own edge, regardless
of what sits behind it.** Claude Code spawns the channel server as a child stdio MCP
process, and "A channel cannot be attached to an already-running session." Source:
PLANNING-PROMPT.md §3.1, retrieved 2026-09-15 (re-confirmed against
`https://code.claude.com/docs/en/channels.md`, retrieved 2026-09-17: "Claude Code
spawns your `webhook.ts` as a subprocess" — the spawn-per-session shape is unchanged).
This is not itself evidence for (a) over (b) — a per-session process is forced either
way — but it fixes the question this decision actually answers: what does that
per-session process talk to, a thing it embeds or a thing it connects to.

**Leg 2 — presence must outlive a session, but a session-scoped process cannot hold
it.** ADR-001's v0.1 scope includes "presence/discovery" (`docs/planning/ADR-001.md`,
"v0.1 scope"). Zenoh's liveliness primitive is a token held by the process that
registered it — the token, and the presence it announces, ends when that process
exits (`oac-zenoh`; PLANNING-PROMPT.md §3.4, "Primitives: pub/sub, queryables,
liveliness tokens with history-capable liveliness subscribers"). If the peer holding
liveliness lived inside a per-session shim, device presence would flap every time any
one session opened or closed — presence needs a holder whose lifetime is the device,
not the session.

**Leg 3 — key material belongs in one process.** ADR-001's Security model states the
identity hierarchy verbatim:

```
Security principal -> Device -> Harness -> Session
```

(`docs/planning/ADR-001.md`, "Security model".) The device key is per-**device**, one
level above Harness and two above Session — duplicating it into every session-scoped
process multiplies the attack surface and the places a key can leak by the number of
concurrently open sessions. C1 §8 already pins the credential-store crate this key
lives in: `keyring` `4.2.0` (`docs/planning/decisions/C1-language-runtime.md` §8).

**Leg 4 — the Codex side already has a daemon precedent; per-session clients hit a
known hazard.** Per PLANNING-PROMPT.md §3.2, retrieved 2026-09-15: a shared local
daemon (`codex app-server daemon start`, control socket
`CODEX_HOME/app-server-control/app-server-control.sock`) can host threads, and "Cross-
process resume does not attach. A second app-server process resuming a thread another
process holds loads history from disk and appends silently; the live process is not
notified (open issue #21743)." One OAC daemon holding one Codex app-server client
avoids ever putting two independent OAC processes in that #21743 hazard against the
same thread; N per-session OAC processes each opening their own Codex client would not.
(A live re-fetch of `https://learn.chatgpt.com/docs/app-server` on 2026-09-17 did not
surface this daemon-command text in the page content returned to this session — carried
as a new UNVERIFIED item in §10, not silently dropped; the pinned-baseline citation
above stands per `oac-evidence` §2's "cite the section" allowance since the claim is
unchanged from §3.2.)

## 3. Rejected alternatives

- **Option (b): a self-contained per-session Zenoh peer embedded in each shim.**
  Rejected in one line each: N peers per device multiplies scouting/liveliness churn
  instead of one; the device key gets duplicated into every session process instead of
  living once; presence flaps on every session exit instead of tracking the device;
  N independent Codex app-server clients each individually hit the #21743 silent-append
  hazard instead of one client owning the thread relationship; policy/allowlist state
  has no single owner and must be synchronized across N processes instead of living in
  one.
- **Hybrid: shim embeds the Zenoh peer, daemon only holds keys.** Rejected as strictly
  more moving parts than (a) — it keeps (b)'s N-peers-per-device and N-Codex-clients
  problems (legs 2 and 4 above) while adding a second process (the daemon) that must
  still be reached over IPC for keys alone, buying none of (a)'s consolidation benefit
  in exchange for the same IPC dependency (a) already requires.

## 4. Local IPC mechanism per platform (acceptance box 1)

### Windows: named pipe

- **Path form:** `\\.\pipe\oac-<user-sid-or-hash>` — exact form as specified by the
  task; the pipe name namespace and syntax (`\\.\pipe\pipename`) are confirmed verbatim
  by Microsoft Learn's `CreateNamedPipeW` reference (`[in] lpName` — "The unique pipe
  name. This string must have the following form: \\.\pipe\*pipename*"). Source:
  https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-createnamedpipew,
  updated 2024-11-20, retrieved 2026-09-17.
- **Peer authentication, mechanism 1 — security descriptor on creation.**
  `CreateNamedPipeW`'s `[in, optional] lpSecurityAttributes` parameter takes a pointer
  to a `SECURITY_ATTRIBUTES` structure "that specifies a security descriptor for the new
  named pipe" (same source). `SECURITY_ATTRIBUTES.lpSecurityDescriptor` is documented
  verbatim as: "A pointer to a SECURITY_DESCRIPTOR structure that controls access to the
  object. If the value of this member is NULL, the object is assigned the default
  security descriptor associated with the access token of the calling process." Source:
  https://learn.microsoft.com/en-us/windows/win32/api/wtypesbase/ns-wtypesbase-security_attributes,
  updated 2024-02-22, retrieved 2026-09-17. The daemon builds a security descriptor
  whose DACL grants pipe access only to the creating user's SID (no `Everyone`, no
  anonymous, unlike the documented default descriptor, which "grant[s] read access to
  members of the Everyone group and the anonymous account" per the same
  `CreateNamedPipeW` reference — the default is explicitly not sufficient and the daemon
  must supply its own descriptor).
- **Peer authentication, mechanism 2 — server-side client check.**
  `GetNamedPipeClientProcessId(HANDLE Pipe, PULONG ClientProcessId)` — "Retrieves the
  client process identifier for the specified named pipe." Source:
  https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getnamedpipeclientprocessid,
  updated 2025-07-01, retrieved 2026-09-17. The daemon calls this (or the equivalent
  client-token check via `ImpersonateNamedPipeClient` + `GetTokenInformation`) after
  `ConnectNamedPipe` to confirm the connecting process belongs to the same user SID the
  DACL already restricted access to — belt-and-braces, not a substitute for the DACL.
- Both API names — `CreateNamedPipeW`, `SECURITY_ATTRIBUTES.lpSecurityDescriptor`,
  `GetNamedPipeClientProcessId` — are quoted verbatim from the Microsoft Learn pages
  above, not written from memory, per `oac-evidence` §3.

### Unix (Linux/macOS): `AF_UNIX` stream socket

- **Path:** under `$XDG_RUNTIME_DIR/oac/` (fallback `~/.oac/run/` when
  `XDG_RUNTIME_DIR` is unset). `$XDG_RUNTIME_DIR` is defined verbatim by the XDG Base
  Directory Specification: "$XDG_RUNTIME_DIR defines the base directory relative to
  which user-specific non-essential runtime files and other file objects (such as
  sockets, named pipes, ...) should be stored. The directory MUST be owned by the user,
  and they MUST be the only one having read and write access to it. Its Unix access mode
  MUST be 0700." Source: https://specifications.freedesktop.org/basedir/latest/,
  retrieved 2026-09-17. OAC's own socket directory under it is additionally created
  `0700` and the socket file itself `0600`, so the fallback path (`~/.oac/run/`, not
  guaranteed `0700` by any spec) gets the same enforced permissions the daemon sets
  explicitly rather than inherited ones.
- **Peer authentication — Linux:** `SO_PEERCRED`. Documented verbatim (`unix(7)`):
  "This read-only socket option returns the credentials of the peer process connected
  to this socket... This option works only for connected AF_UNIX stream sockets and for
  AF_UNIX stream and datagram socket pairs created using socketpair(2)." The returned
  `ucred` structure carries `pid`, `uid`, `gid` of the peer. Source:
  https://man7.org/linux/man-pages/man7/unix.7.html, retrieved 2026-09-17.
- **Peer authentication — macOS:** `getpeereid()` (Apple's own documented wrapper,
  implemented on top of the `LOCAL_PEERCRED` socket option). Quoted verbatim: "The
  getpeereid() function returns the effective user and group IDs of the peer connected
  to a UNIX-domain socket. The argument s must be a UNIX-domain socket (unix(4)) of type
  SOCK_STREAM on which either connect(2) or listen(2) have been called." Source:
  https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/getpeereid.3.html,
  retrieved 2026-09-17. `LOCAL_PEERCRED` itself is the underlying macOS/BSD socket
  option `getpeereid()` is built on (per the same page's "Implementation Notes";
  BSD-family, not independently re-fetched from an Apple-hosted `LOCAL_PEERCRED`-
  specific page in this document — carried as-is from the `getpeereid` page's own
  cross-reference, not from a third-party source).
- Both peer checks assert **peer UID equals the daemon's own UID** — the daemon refuses
  a connection from any other UID even if the socket file's mode would otherwise permit
  the connect (defense in depth over the `0600`/`0700` filesystem permissions alone).

### IPC crate

**Candidate confirmed, not assumed:** `interprocess`. Fetched, not recalled from
memory:

- Pinned version: **`2.4.4`**. Source: https://crates.io/api/v1/crates/interprocess,
  `max_stable_version` field, retrieved 2026-09-17.
- License: **`0BSD OR Apache-2.0`**. Source: same API response, `license` field,
  retrieved 2026-09-17; cross-checked against the crate's own `Cargo.toml` at the
  `main` branch (`license = "0BSD OR Apache-2.0"`), source:
  https://raw.githubusercontent.com/kotauskas/interprocess/main/Cargo.toml, retrieved
  2026-09-17. Apache-2.0 compatible: **yes** — OAC elects the Apache-2.0 arm of the dual
  license, the same election C1 §7 makes for `zenoh`'s EPL-2.0/Apache-2.0 dual license,
  keeping the dependency tree under one permissive family.
- Named-pipe and Unix-socket coverage, confirmed from the crate's own docs (not
  asserted from memory): Windows named pipes are documented at
  `interprocess::os::windows::named_pipe`; Unix domain sockets are provided through "the
  standard library types" (`std::os::unix::net`) as of `interprocess` 2.x, which removed
  its own first-party Unix-socket implementation in 2.0.0 "in favor of the support
  provided by the standard library." Source:
  https://docs.rs/interprocess/2.4.4/interprocess/, retrieved 2026-09-17.
- **Peer-credential retrieval is not itself asserted as a bundled `interprocess`
  feature** — the fetched crate documentation did not surface an explicit
  `SO_PEERCRED`/`getpeereid`/`GetNamedPipeClientProcessId` wrapper in `interprocess`
  2.4.4's own public API. Per the "do not assert a crate feature from memory" rule,
  this is stated as **not confirmed present**, not claimed either way with confidence:
  the daemon calls the raw OS APIs cited above directly (via `libc`/`windows-sys`, or
  whichever peer-credential accessor a future implementation task selects) for the
  authentication step, while `interprocess` (or `tokio`'s `net::windows::named_pipe` +
  `UnixListener`, the alternative PLANNING-PROMPT.md §5.2 names) supplies the pipe/
  socket transport primitives themselves. This split — transport crate plus a direct
  OS-API call for peer credentials — is recorded here rather than resolved, and is a
  Stage 3 implementation detail (`oac-implementation`), not re-litigated in this
  decision.

### Rejected: loopback TCP

Loopback TCP (`127.0.0.1:<port>`) is rejected as the local IPC mechanism: it has no
OS-level peer authentication — any local process, of any user, can connect to a
loopback listener unless the application layer adds its own auth, which is exactly the
gap named pipes and Unix sockets close for free via the OS's own process/credential
model (§4 above). Using it would mean re-inventing peer authentication in OAC's own
code instead of relying on the OS, for no offsetting benefit in the local, single-user
default case this decision targets.

## 5. Presence lifetime (acceptance box 2)

Neutral vocabulary only — no Zenoh terms below, per `[ADR-001 Boundary]` "MUST NOT
leak Zenoh-specific concepts into the neutral protocol."

- The daemon holds **device presence** for as long as it runs. Device presence begins
  when the daemon starts and ends when the daemon exits — nothing else changes it.
- Each `oac mcp-shim` registers a **session** with the daemon when its IPC connection
  is established. The session is live from that point.
- The daemon deregisters a session when its IPC connection drops. **End-of-file on the
  pipe or socket is the exit signal** — the daemon reacts to the connection closing, it
  does not poll for liveness (`oac-boundaries` 14, "no polling"). This covers both exit
  paths with the same mechanism:
  - **Clean session exit** (the harness process exits normally, or the shim is told to
    stop): the shim closes its end of the connection; the daemon sees EOF and
    deregisters the session immediately.
  - **Shim crash**: the OS closes the crashed process's handles, which closes its end
    of the pipe/socket the same way a clean exit would; the daemon sees the same EOF
    and deregisters the session the same way. The daemon cannot distinguish a crash
    from a clean exit from the EOF alone, and does not need to — both end the session.
- The device stays present the entire time the daemon runs, independent of how many
  sessions are registered, including zero. Sessions come and go under a present device;
  a device with no sessions registered is still present.
- **When the daemon itself exits:** device presence ends, and every session still
  registered under it ends at the same moment — a session cannot outlive the daemon
  that tracks it. Each shim, on losing its own connection to the daemon (the daemon's
  end closes), treats that the same way it treats any dropped connection: the session
  it represented is gone.

## 6. CLI surface (acceptance box 3)

| Command | Purpose | Exit-code behaviour |
|---|---|---|
| `oac start` | Start (or confirm running) the per-device daemon. **Idempotent**: running it while a daemon is already up for this device is a no-op success, not an error. **Foreground by default**; add `--detach` to self-daemonize into the background. Exit `0` on a daemon now running (whether newly started or already up); non-zero on failure to bind the IPC endpoint or start. |
| `oac status` | Report daemon up/down, device id, and transport state (peer up/down). Exit `0` when the daemon is up and reachable over IPC; non-zero when it is down or unreachable — scriptable as a liveness check. |
| `oac sessions` | List sessions currently registered with the daemon (per §5). Exit `0` on a successful listing (including an empty list); non-zero if the daemon is unreachable. |
| `oac doctor` | Preflight check: IPC path exists with correct permissions (§4), OS credential store reachable, Zenoh peer able to come up, Claude Code version at/above the floor in `docs/planning/PINS.md` plus `MCP_PROTOCOL_NEGOTIATION` set correctly, and Codex daemon reachability. Exit `0` only if every check passes; non-zero and a per-check report otherwise. |
| `oac mcp-shim` | The stdio MCP server Claude Code and Codex spawn as their child process (§1); connects to the daemon over local IPC. **Not meant to be typed by hand** — a harness config entry invokes it, a person does not run it directly. |
| *(named, semantics deferred)* | §5.6 (pairing and authorization) and the C-series launch decisions own pairing-flow and one-shot-launch command semantics; naming them here would invent behaviour this decision does not own. Placeholders this CLI surface reserves room for: a pairing subcommand (owned by the pairing decision) and any device-setup subcommand a future launch decision needs. No flags or output shape are specified for these here. |

Two documented one-line launch commands, verified against first-party docs rather than
copied from the presumptive shape in the task breakdown:

- **Claude Code.** The presumptive shape `claude --channels server:oac ...` does **not**
  match what the first-party docs show for a channel not on Anthropic's allowlist
  (which `oac mcp-shim` is, until/unless OAC is added to `claude-plugins-official` or an
  org's `allowedChannelPlugins`). The documented form for a bare, non-plugin server
  during the research preview is the **development flag**, with the server named
  `server:<name>` as its argument — quoted verbatim from a first-party example: `claude
  --dangerously-load-development-channels server:webhook`. Source:
  https://code.claude.com/docs/en/channels-reference.md, retrieved 2026-09-17. Carried
  over to OAC's own server name, the documented launch command is:

  ```
  claude --dangerously-load-development-channels server:oac
  ```

  This requires the interactive confirmation dialog the source page also documents
  ("Claude Code first shows a full-screen warning dialog listing the development
  channels you're loading") — not a silent flag, consistent with `oac-boundaries` 9's
  note that this confirmation must not be weakened. **UNVERIFIED — pinning OAC to this
  flag long-term (rather than an eventual allowlist entry) is a product decision this
  document does not make;** stated here only as the currently correct documented
  command for a non-allowlisted server at the pinned Claude Code version
  (`docs/planning/PINS.md` — Claude Code Channels, `v2.1.274`).
- **Codex.** `codex mcp add` is the documented, supported way to register OAC's
  `oac mcp-shim` as an external MCP server Codex can call as a tool (PLANNING-PROMPT.md
  §3.2: "`codex mcp add` still registers external MCP servers that Codex can call as
  tools; that is the supported outbound tool surface."). Exact syntax, quoted from the
  first-party CLI reference: `add <name> -- <command...> | --url <value>`, with
  `--env KEY=VALUE` supported for stdio transports. Source:
  https://learn.chatgpt.com/docs/cli/reference, retrieved 2026-09-17. Applied to OAC:

  ```
  codex mcp add oac -- oac mcp-shim
  ```

  This is the outbound tool-registration path (§9 of PLANNING-PROMPT.md's decision
  list), distinct from the daemon-attach live-inject path (§2 leg 4 above), which is a
  runtime behaviour of the Codex app-server itself, not a CLI command a user types.

## 7. Zero-file local default (acceptance box 4)

A fresh install with **no config file works**. Defaults, all of which apply with zero
configuration present:

- **IPC path:** the platform default from §4 — `\\.\pipe\oac-<user-sid-or-hash>` on
  Windows, `$XDG_RUNTIME_DIR/oac/<socket-name>` (fallback `~/.oac/run/<socket-name>`)
  on Unix. No path needs to be supplied.
- **Device key:** created automatically on first `oac start`, written into the OS
  credential store via `keyring` `4.2.0` (C1 §8) — no manual key-generation step, no
  file to place.
- **Transport:** local-only by default (loopback-bound where applicable; no LAN/remote
  transport is enabled without explicit configuration — that is a §5.6/§5.10 concern
  this document does not expand).

A config file is **optional**. When present, its path follows the conventional
per-platform config-directory form for the `oac` app name (e.g. under
`$XDG_CONFIG_HOME/oac/` on Unix, the platform config directory on Windows — the exact
directory-resolution crate/library is a Stage 3 implementation detail, not decided
here). Precedence order, highest first: **flag > env > file > default** — an explicit
CLI flag always wins, then an environment variable, then a config-file value, then the
built-in default.

**Every command in §6's table is reachable with no config file at all** — `oac start`,
`oac status`, `oac sessions`, `oac doctor`, and `oac mcp-shim` all function against pure
defaults; nothing in the CLI surface requires a config file to exist.

## 8. Smallest-design preference (acceptance box 5)

**The smallest design that proves ADR-001's Validation criterion is preferred.** The
validation criterion (`docs/planning/ADR-001.md`, "Validation criterion") requires an
existing Claude Code session and an existing Codex harness session to exchange a
message through Session Channels/Zenoh actively, without either side holding the
other's model-API credentials, and with enforceable sender identity — nothing in it
requires OAC to behave like installed system software. Deliberately left out of this
process model **for that reason**:

- **No supervisor/service installer.** `oac start` is a command a user runs (or a shim
  runs on their behalf, §"boundary self-check" below); OAC does not register itself
  with `systemd`, Windows Services, or `launchd`.
- **No auto-start on login.** The daemon starts when something needs it — first
  `oac start`, or the first `oac mcp-shim` that finds no daemon running (§9) — not at
  every login regardless of use.
- **No multi-user daemon.** One daemon per device is scoped to one user (§4's peer
  checks assert the daemon's own UID/SID); no multi-tenant daemon process serving
  several OS users is built.
- **No durable message store.** Deferred per ADR-001 "Defer" (`docs/planning/ADR-001.md`
  — "durable offline mailboxes" is explicitly deferred, not v0.1 work; `oac-boundaries`
  11 names the same deferral). The daemon's session/presence bookkeeping is in-memory
  process state, not a durable store surviving daemon restart.

## 9. Boundary self-check (§9)

Quoted verbatim (`docs/planning/ADR-001.md` line 21):

```
The reference implementation is a CLI and must not require Docker, Kubernetes, a cloud
account, or a separately administered server for normal local use.
```

**Argument that the daemon does not become "a separately administered server for
normal local use":**

- **User-owned.** The daemon runs as the invoking user's own OS process, under that
  user's own account, with no elevated or service-account identity — the same process
  ownership model as any other CLI tool the user runs, not a system service with its
  own account.
- **Auto-startable by the shim, not separately administered.** `oac mcp-shim` — the
  process a harness spawns automatically when a session with OAC configured starts —
  is the natural place to check for a running daemon and start one (via `oac start`)
  if none is found, so the user does not perform a separate administrative step before
  a session can use OAC. This keeps starting the daemon inside the same "launch a
  session" action the user already takes, rather than a distinct "administer a server"
  action beforehand. (Whether the shim performs this check-and-start itself, or merely
  fails fast with instructions to run `oac start`, is a Stage 3 implementation detail
  this document does not fix; either way, no separate administrative account, install
  step, or ongoing maintenance is required.)
- **Not a service the user administers.** There is no install-as-a-service step, no
  service-manager unit file this decision defines (§8 — deliberately out of scope), and
  no requirement that the daemon survive a reboot unattended. A user who never starts
  OAC never has a daemon running; nothing runs in the background without the user
  having taken an action (starting a session with OAC configured, or running
  `oac start` directly) that caused it.

**This argument holds** — proceeding with the daemon design as decided. Per
`oac-boundaries`, "Stop, cite the boundary" is the fallback for when this argument does
*not* hold; it is not invoked here because the three points above are affirmatively
satisfied, not merely asserted.

**No Zenoh vocabulary leaked into §5 or §6:** confirmed by re-reading both sections —
§5 uses only "device presence," "session," "IPC connection," "EOF" (all neutral);
§6's CLI table and launch commands name no Zenoh type, key expression, or liveliness
term. The daemon's *internal* use of a Zenoh peer is named once in §1 (what the daemon
owns), which is describing an implementation detail of the daemon, not defining the
neutral protocol — consistent with DESIGN.md's "Zenoh types must not escape the
transport module" boundary, which governs the neutral spec/core surface, not this
process-model decision's own prose about what one component internally holds.

## 10. Surface labels and UNVERIFIED ledger (acceptance-adjacent, §10)

Per `oac-evidence` §4, one label per surface touched by this document:

| Surface | Label | Note |
|---|---|---|
| Claude Code Channels | research preview | unchanged from `docs/planning/PINS.md` |
| Codex app-server / daemon-attach | experimental (per-method gating) | unchanged from `docs/planning/PINS.md` |
| `interprocess` (IPC crate) | supported | general-purpose, actively maintained; not a preview/experimental provider surface |
| `keyring` | supported | already labelled in C1 §13 |

New UNVERIFIED items from this document, added to `docs/planning/STATUS.md`'s "Open
UNVERIFIED items" list in the same change (`oac-evidence` §5 — never recorded in only
one place):

- Whether `oac mcp-shim`, spawned by Claude Code as a child stdio process, inherits an
  environment sufficient to locate the daemon's IPC path (the per-user pipe name or
  `$XDG_RUNTIME_DIR` value) without additional configuration (UNVERIFIED — depends on
  what environment Claude Code's channel-spawn mechanism passes through to a spawned
  stdio server; not established by any source cited in this document).
- Whether the Codex daemon's implicit attach is enabled by default in released
  `0.154.0` (UNVERIFIED — already an open item per `docs/planning/STATUS.md`; this
  document adds one data point rather than resolving it: a live re-fetch of
  `https://learn.chatgpt.com/docs/app-server` on 2026-09-17 did not surface the
  `codex app-server daemon start` command or the `app-server-control.sock` control-
  socket path in the page content returned to this session, where PLANNING-PROMPT.md
  §3.2's pre-verified baseline (retrieved 2026-09-15) states both. This is recorded as
  a possible documentation drift signal, not confirmed drift — the fetch tool's
  extraction may simply have missed the relevant section of a large page. Resolution
  path is the same as the existing open item: task D2 / gate G2 against the pinned
  build, not a documentation re-read.
- Named-pipe DACL peer-authentication behaviour (§4, Windows) has not been exercised
  against a live Windows host in this document — the API shape is verified against
  Microsoft Learn, but end-to-end behaviour (a second-user SID actually being denied
  connection) is UNVERIFIED until a Stage 3/4 implementation task runs it.
- Whether `interprocess` `2.4.4` (or an alternative such as `tokio`'s
  `net::windows::named_pipe` + `UnixListener`) exposes a first-party peer-credential
  accessor is UNVERIFIED (§4, "IPC crate" — the fetched crate documentation did not
  surface one; the daemon is expected to call the raw OS API directly instead, but this
  has not been confirmed against the crate's full public API surface, only against the
  docs page fetched).

## 11. Reversal condition

Reverse to option (b) (§3) if either half below fires:

- **Local IPC cannot be peer-authenticated on a supported platform.** Test: does the
  platform expose *either* (Windows) a named-pipe security descriptor mechanism
  (`SECURITY_ATTRIBUTES.lpSecurityDescriptor`) plus a server-side client-identity check
  (`GetNamedPipeClientProcessId` or equivalent), *or* (Unix) a connected-socket peer-
  credential mechanism (`SO_PEERCRED` on Linux, `getpeereid()`/`LOCAL_PEERCRED` on
  macOS)? At the versions/APIs cited in §4 today: **test passes on both platform
  families** — both mechanisms are documented and verbatim-cited above. A future
  platform target (e.g. a BSD without `LOCAL_PEERCRED`-equivalent, or a sandboxed
  environment that blocks named-pipe DACL customization) that fails this test on a
  platform OAC must support would fire this half.
- **Claude Code's spawned channel server cannot reliably locate/connect to the
  daemon.** Test: does `oac mcp-shim`, launched exactly as Claude Code launches a
  channel server (no OAC-controlled command-line arguments beyond what `--channels`/
  `--dangerously-load-development-channels` pass), successfully connect to a daemon
  already running for the same user, using only information available in its inherited
  environment and well-known per-platform default paths (§4), across repeated launches?
  This test is **not yet run** — it depends on the first UNVERIFIED item in §10 (shim
  environment inheritance), which is a G1-adjacent runtime question, not resolvable
  from documentation alone. Until it is run, this half of the reversal condition is
  **open, not failed** — a future gate or implementation task must execute it before
  this decision can be called fully validated end-to-end, the same way C1 §5 carries
  its own SDK-capability-vs-runtime-registration distinction forward to gate G4.

Neither half has fired as of this document; the first half's test passes outright, the
second is carried open pending runtime confirmation, matching the "carried, not
resolved" pattern C1 uses for its own SDK-vs-runtime split.

## 12. Acceptance boxes, ticked against lines in this file

- [x] IPC mechanism defined per platform, with peer authentication and a verified
      crate pin — §4 (load-bearing).
- [x] Presence lifetime state machine described in neutral vocabulary, with daemon-exit
      and shim-crash-vs-clean-exit behaviour — §5.
- [x] CLI surface table with purpose and exit-code behaviour per command, plus the two
      verified one-line launch commands — §6.
- [x] Zero-file local default confirmed, with config file path form and
      flag > env > file > default precedence — §7.
- [x] Smallest-design preference stated explicitly, with the deliberately-omitted list
      — §8.

## Where this folds in

Once `docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4) exists, this
file's content moves there unedited in substance (per PLANNING-PROMPT.md §9's output
package shape) and this file becomes a redirect stub, mirroring how
`ADR-001-AMENDMENTS.md` already describes its own eventual fold-in.
