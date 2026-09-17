# 04 — Architecture

**Source:** `docs/planning/PLANNING-PROMPT.md` §9 item 5.

**Scope.** This file fixes OAC's components, process boundaries, per-provider inbound and
outbound data/control flow, and local and LAN deployment topologies, in text diagrams. It
does not restate `docs/planning/ADR-001.md` or `docs/planning/DESIGN.md` prose beyond what
a diagram or flow needs — cite those files by path instead. It does not define interface
signatures or message shapes (`docs/planning/v0.1/05-interfaces.md`'s job), does not define
the threat model (`docs/planning/v0.1/06-security.md`'s job), and does not define the
CLI/launch story (`docs/planning/v0.1/08-cli-and-deployment.md`'s job, though §12-§13 below
name the topology facts that story is built on).

**[transport] Gate caveat, carried from `docs/planning/decisions/C7-zenoh-transport.md`.**
Per `docs/planning/STATUS.md`'s Gate verdicts table, **gate G3 (Zenoh local peer) is
`NOT RUN`**. Every transport-behavior statement below — loopback discovery, presence
liveliness, local/LAN security profiles — is a **designed** mechanism this file diagrams,
not a **proven** one. The same caveat applies to gates G1, G2, G4, and G5, each `NOT RUN`
per the same table: every provider-adapter flow below is the designed mechanism the named
gate will exercise, stated at each flow's own first mention below rather than repeated at
every sentence.

**Naming.** Product and repository: **Open Agent Channel (OAC)**. Normative protocol
specification: **OAC Session Channels**. CLI binary: **`oac`**. Per ADR-001-A1
(`docs/planning/v0.1/03-decisions-and-amendments.md` §2), this file never writes bare
"Session Channels" or `sessionchannels`, except where a source document (`DESIGN.md`,
pre-rename) is quoted verbatim, marked as such at the quotation.

---

## 1. Boundary-discipline rule (stated once, enforced throughout)

The words **Claude**, **Codex**, MCP method names, **Zenoh**, key expression, and
liveliness appear only inside boxes or paragraphs explicitly labelled **adapter** or
**transport**. Every other component box and every arrow between neutral components uses
neutral vocabulary only. Quoted, `[ADR-001 Boundary]`: "MUST NOT leak Zenoh-specific
concepts into the neutral protocol." Quoted, `docs/planning/DESIGN.md` line 70: "Zenoh
types must not escape the transport module." Quoted, `docs/planning/DESIGN.md` line 35:
"The specification MUST NOT mention Zenoh keys, MQTT topics, NATS subjects, or
provider-specific method names."

**Two deliberate exceptions, justified in one line each.** §5's process-boundary diagram
must show that the daemon holds the transport peer (`docs/planning/decisions/
C2-process-model.md` §1; issue #22 acceptance box 2). To do this without a Zenoh term in
the daemon box's own label, the diagram draws a **transport box nested inside** the
daemon's process box — the daemon box itself is labelled only "daemon (`oac`)"; the nested
box, and only that nested box, is labelled "transport" and may name Zenoh inside it.
Second, a handful of neutral component descriptions (the §2 component table, §3 item 4,
§5's daemon box, §12's harness boxes) must state a provider-specific fact (which adapter
needs a persistent app-server client; which harness maps to which adapter) — each such
spot carries an explicit `[adapter: <name>]` bracket label, the same convention §5 already
uses on its two harness boxes, so the surrounding text or box is itself "explicitly
labelled adapter" rather than a bare neutral mention.

---

## 2. Components and responsibilities

Derived from `docs/planning/DESIGN.md` §Components and `docs/planning/decisions/
C2-process-model.md` §1. Each row states what the component owns, what it never owns, and
its repo path (`docs/planning/DESIGN.md`'s Suggested repository shape, lines 136-149).

| Component | Owns | Never owns | Repo path | Key material |
|---|---|---|---|---|
| CLI (`oac` binary) | User-facing commands (`oac start`, `status`, `sessions`, `doctor`, `mcp-shim`) — the surface a person or a harness config invokes | Long-lived process state; the transport peer; policy decisions | `cli/` | none |
| Daemon | The transport peer, device identity/key material, policy/allowlists/pairing state, the single app-server client `[adapter: Codex]` (§3) — one long-lived process per device | Provider-specific rendering logic; per-session harness state beyond registration | (the daemon binary; hosts `core/` and starts the transport module) | **daemon only** — see §3 |
| `oac mcp-shim` | Nothing beyond a thin stdio connection to the daemon over local IPC, one per harness session | The transport peer; key material; policy; any provider-specific logic | `cli/` (the `mcp-shim` subcommand) | none |
| Core | Neutral types and policy/authorization: `SessionIdentity`, `SessionDescriptor`, `SessionCapabilities`, `ChannelMessage`, `DeliveryReceipt`, `PresenceRecord`, `SecurityPrincipal` (`docs/planning/DESIGN.md` line 28) | Provider-native vocabulary; transport-native vocabulary | `core/` | none (core holds no key material itself; the daemon process that hosts core does, per §3) |
| Spec surface | The neutral OAC Session Channels specification text | Implementation code; provider-specific or transport-specific vocabulary | `spec/` | none |
| Provider adapter A (Claude) | Translating neutral envelopes to/from Claude Code's provider-native wake and reply operations | The transport peer; key material; policy decisions (routes through core, per `docs/planning/DESIGN.md`'s "Adapters should route through core policy/security rather than directly through transports") | `adapters/claude/` | none |
| Provider adapter B (Codex) | Translating neutral envelopes to/from the Codex app-server's provider-native turn/thread operations | The transport peer; key material; policy decisions; OpenAI model-API credentials | `adapters/codex/` | none |
| Transport module | Every Zenoh-specific type, identifier, and concept (`docs/planning/decisions/C7-zenoh-transport.md` §2); the `Transport` contract's six operations over neutral types only | Policy/authorization decisions; signature verification; anything visible outside `publish`/`subscribe`/`announce_presence`/`watch_presence`/`health`/`shutdown` | `transports/zenoh/` | none (the daemon holds the key material; the transport module holds the Zenoh session/peer state the daemon starts it with) |

---

## 3. What the daemon owns

Cited to `docs/planning/decisions/C2-process-model.md` §1, restated here as the four items
this architecture file's component table and process-boundary diagram (§5) depend on:

1. **The transport peer** — the one process per device that holds the transport's
   discovery, presence, and pub/sub state (transport-specific detail: §5's nested
   transport box, §12).
2. **Device identity and key material** — the per-device signing key, read from and
   written to the OS credential store via `keyring` `4.2.0`.
3. **Policy and allowlists** — sender allowlists, pairing state, and ACL decisions, in one
   place instead of duplicated per session.
4. **`[adapter: Codex]` The Codex app-server client** — one client of the Codex
   app-server, not one per OAC session (provider-specific detail: §9-§10).

**The shim owns none of these.** Every "Key material" cell in §2's component table reads
"none" except the daemon's, which reads "daemon only" — this is the direct consequence of
`docs/planning/decisions/C2-process-model.md` §1 fixing option (a) (one daemon, thin shims)
over option (b) (a peer embedded in each shim).

---

## 4. Naming resolution note

Applied throughout this file per ADR-001-A1: "Open Agent Channel (OAC)", "OAC Session
Channels", binary `oac`. `docs/planning/DESIGN.md`'s own still-unrenamed text
(`sessionchannels` CLI examples, "MCP Session Channels extension") is never carried into
this file's diagrams — this file's diagrams use the resolved names directly rather than
quoting DESIGN.md's stale spelling.

---

## 5. Process-boundary diagram

One device, three process kinds, drawn as nested boxes. All process labels are neutral
except the two adapter boxes, the app-server-client line, and the one transport box
(§1's stated exceptions).

```
+-------------------------------------------------------------------+
| Device (one user)                                                 |
|                                                                     |
|  +----------------------+   +----------------------+               |
|  | Harness process A    |   | Harness process B    |               |
|  | [adapter: Claude Code]|   | [adapter: Codex]     |               |
|  +----------+-----------+   +----------+-----------+               |
|             | spawns                   | spawns                    |
|             v                          v                            |
|  +----------------------+   +----------------------+               |
|  | oac mcp-shim          |   | oac mcp-shim          |              |
|  | (stdio child process) |   | (stdio child process) |              |
|  | key material: none    |   | key material: none    |              |
|  +----------+-----------+   +----------+-----------+               |
|             |                          |                            |
|             |   local IPC boundary     |                            |
|             |   (trust boundary --     |                            |
|             |   peer-UID/SID asserted) |                            |
|             v                          v                            |
|  +-------------------------------------------------------------+   |
|  | oac daemon (one long-lived process per device)               |   |
|  |                                                               |   |
|  |  device identity / key material   <- daemon only              |   |
|  |  policy / allowlists / pairing state                          |   |
|  |  [adapter: Codex] app-server client (one, daemon-held)        |   |
|  |                                                               |   |
|  |  +---------------------------------------------------------+ |   |
|  |  | transport  [transport: Zenoh peer, in-process]           | |   |
|  |  +---------------------------------------------------------+ |   |
|  |                                                               |   |
|  +-------------------------------------------------------------+   |
+-------------------------------------------------------------------+
```

**IPC boundary and mechanism per platform (C2 §4).** Windows: named pipe
`\\.\pipe\oac-<user-sid-or-hash>`, DACL-restricted to the creating user's SID plus a
server-side `GetNamedPipeClientProcessId` peer check. Unix: `AF_UNIX` stream socket under
`$XDG_RUNTIME_DIR/oac/` (fallback `~/.oac/run/` when `$XDG_RUNTIME_DIR` is unset),
`0700`/`0600` permissions, `SO_PEERCRED` (Linux) or `getpeereid()` (macOS) peer-UID
assertion. Both mechanisms assert the connecting process is the same OS user as the
daemon — the boundary is a trust boundary, not just a process boundary.

**Key-material holder.** The daemon only, per §3. Every other box in the diagram carries
"key material: none."

**Transport peer.** Inside the daemon, drawn as the nested transport box per §1's stated
exception — never a separate process, never in a shim.

**Process/trust boundary lines.** Two boundaries are crossed in this diagram: the harness
process spawns its shim as a plain OS child-process boundary (no trust decision — the
harness already trusts what it spawns), and the shim-to-daemon local IPC connection is a
real trust boundary, crossed only after the OS-level peer check above passes.

---

## 6. Process lifetime and presence

Neutral vocabulary only, per `docs/planning/decisions/C2-process-model.md` §5 (cited, not
restated in full). Device presence equals daemon lifetime: it begins when the daemon
starts and ends when the daemon exits. Each `oac mcp-shim` registers a session with the
daemon when its IPC connection is established, and the daemon deregisters that session on
EOF — the connection closing, whether from a clean harness exit or a shim crash, is the
only signal the daemon reacts to. Quoted, `oac-boundaries` 14: "no polling" — the daemon
does not poll for session liveness. When the daemon exits, device presence ends and every
session still registered under it ends at the same moment; no session outlives the daemon
that tracks it.

---

## 7. Claude adapter — inbound flow

External message reaches a live Claude Code session, drawn separately from the outbound
flow (§8).

```
transport receives envelope
  -> transport module maps the wire form to the neutral envelope, crosses the transport boundary
    -> core verifies the envelope signature and checks the sender allowlist
      -> adapter renders the neutral envelope to the provider-native wake operation
        -> harness (Claude Code) delivers it as a user turn
```

**Provider-native names, adapter box only.** The channel server declares
`capabilities.experimental["claude/channel"] = {}` and delivers via
`notifications/claude/channel`, carrying `content` (string) and `meta` (a
string-to-string map); `meta` keys must be identifier-safe or Claude Code silently drops
them. An inbound notification wakes an idle session as a user turn; notifications
arriving mid-turn are queued and delivered together, in order, at the next turn. **There
is no acknowledgement** — a resolved send means "written to transport", not "seen by the
model." Source: PLANNING-PROMPT.md §3.1, retrieved 2026-09-15.

**Surface label, at first mention.** Claude Code Channels is a **research preview**.
Pinned version: Claude Code `v2.1.274` (`docs/planning/PINS.md` — Claude Code Channels).
Compatibility shim boundary: `adapters/claude/` (UNVERIFIED — the module/interface name
itself is not yet fixed in `DESIGN.md`; open ledger entry C11,
`docs/planning/v0.1/03-decisions-and-amendments.md` §4, **ASSIGNED**, not closed) — this
path is this file's working name for the isolation layer between the volatile preview
surface and the rest of OAC, pending C11's resolution.

---

## 8. Claude adapter — outbound flow

A Claude Code session sends to an external session, drawn separately from the inbound
flow (§7).

```
harness (Claude Code) calls an ordinary MCP tool the channel server exposes (conventionally `reply`)
  -> adapter maps the tool call to the neutral envelope
    -> core signs the envelope and authorizes it
      -> transport publishes it
```

**Correlation limitation, stated verbatim in substance.** Correlation is by convention
only, via `meta` attributes the server asks the model to echo back. Source:
PLANNING-PROMPT.md §3.1, retrieved 2026-09-15. No delivery acknowledgement exists on this
path either (§7's "no acknowledgement" statement applies to both directions of the Claude
surface).

**No attach to an already-running session.** Quoted, PLANNING-PROMPT.md §3.1: "A channel
cannot be attached to an already-running session." Per `oac-boundaries` 7 and
ADR-001-A2 (`docs/planning/v0.1/03-decisions-and-amendments.md` §2), "existing session" in
the Validation criterion is redefined as a session **launched OAC-enabled**, not an
arbitrary already-running one — this outbound flow, like the inbound flow, only ever
targets a session that was started with OAC's channel loaded.

---

## 9. Codex adapter — inbound flow

External message reaches a live Codex thread, drawn separately from the outbound flow
(§10).

```
transport receives envelope
  -> transport module maps the wire form to the neutral envelope, crosses the transport boundary
    -> core verifies the envelope signature and checks the sender allowlist
      -> adapter renders the neutral envelope to the provider-native live-inject operation
        -> daemon-hosted Codex thread receives it
```

**Provider-native names, adapter box only.** `thread/queue/add` (queued until the thread
is idle; experimental), `turn/steer` (injected into the active turn), `turn/start` (used
when the thread is idle). The daemon reaches the Codex app-server over its control socket,
`CODEX_HOME/app-server-control/app-server-control.sock`. Experimental methods require
`capabilities.experimentalApi`. Events consumed: `turn/completed`, `item/completed`
(authoritative), `thread/status/changed`. Source: PLANNING-PROMPT.md §3.2, retrieved
2026-09-15.

**One daemon-held app-server client only.** Per `docs/planning/decisions/
C2-process-model.md` §1 item 4 and §2 leg 4, a single client avoids the cross-process
silent-append hazard: "a second app-server process resuming a thread another process holds
loads history from disk and appends silently; the live process is not notified (open
issue #21743)." N per-session clients would each independently risk that hazard; one
daemon-held client does not.

**Surface label, at first mention.** Codex app-server live-inject is **experimental
(per-method gating)** — each method requires `capabilities.experimentalApi` and is
documented as not durable. Pinned version: `@openai/codex@0.154.0`, commit
`6b9826e3aa83b1a5947db50f4332cb9c65f1b340` (`docs/planning/PINS.md` — Codex CLI and
app-server). Compatibility shim boundary: `adapters/codex/` (UNVERIFIED — same ledger
entry C11 as §7; not yet fixed in `DESIGN.md`).

**Open UNVERIFIED item, named rather than assumed.** Whether the Codex daemon's implicit
attach is enabled by default in released `0.154.0` is UNVERIFIED (`docs/planning/
STATUS.md`'s "Open UNVERIFIED items"; `docs/planning/decisions/C2-process-model.md` §2 leg
4, §10). Resolution path: gate **G2**, currently `NOT RUN`.

---

## 10. Codex adapter — outbound flow

A Codex session sends to an external session, drawn separately from the inbound flow
(§9).

```
Codex session calls the OAC tool registered via `codex mcp add` (the supported outbound tool surface)
  -> oac mcp-shim (stdio child process)
    -> local IPC
      -> daemon core signs and authorizes the neutral envelope
        -> transport publishes it
```

**`codex mcp-server` is not the surface used.** It was deprecated 2026-08-20 and deleted
2026-09-05 (UNVERIFIED — both dates carried unchanged from PLANNING-PROMPT.md §3.2, not
independently re-confirmed; see `docs/planning/STATUS.md`'s "Open UNVERIFIED items");
`codex mcp add` — registering OAC's `oac mcp-shim` as an **external** MCP server Codex can
call as a tool — is the supported outbound path. Source: PLANNING-PROMPT.md §3.2, retrieved
2026-09-15.

**Credential boundary, stated explicitly.** The Codex app-server uses the saved CLI login
(`CODEX_HOME/auth.json` or the OS keyring); OAC never holds or reads it. Quoted,
`[ADR-001 Boundary]`: "MUST NOT steal or reuse another harness's provider credentials."
Quoted, `docs/planning/DESIGN.md` line 53: "Never call the OpenAI model API directly."

---

## 11. Cross-cutting control-flow statements

Stated once here for both providers, not repeated per adapter.

- **Signature verification** happens in core, inside the daemon — never in the shim and
  never in the transport module (§2's component table; `docs/planning/decisions/
  C5-envelope-auth.md` §2).
- **Authorization/allowlist decisions** happen in core, inside the daemon (§3 item 3).
- **Trusted-vs-untrusted rendering** is decided by the adapter, per provider. Full
  detail: `docs/planning/decisions/C6-trust-rendering.md` — not restated here.
- **Delivery states.** Quoted, `docs/planning/DESIGN.md` line 108: at minimum `accepted`,
  `rejected`, `unreachable`, `expired`, `duplicate`, `failed`. `accepted`/`rejected` are
  produced by core (the signature/allowlist check, §3 item 3); `unreachable` is produced
  by the transport's presence mapping (§12); `expired`/`duplicate` are produced by core's
  replay-window and nonce-deduplication check (`docs/planning/decisions/
  C5-envelope-auth.md`); `failed` may be produced by either the transport (publish
  failure) or the adapter (rendering failure). **No exactly-once promise** — quoted,
  `docs/planning/DESIGN.md` line 108: "Do not promise exactly-once delivery."

---

## 12. Local deployment topology

One device, one user, one daemon, N shims, two harnesses.

```
Device
  Harness A [adapter: Claude Code] --spawns--> oac mcp-shim --IPC--> oac daemon
  Harness B [adapter: Codex]        --spawns--> oac mcp-shim --IPC--> oac daemon
                                                                 |
                                                            [transport: Zenoh peer,
                                                             bound to 127.0.0.1]
```

**No container, no cloud account, no separately administered server.** Carried by
citation, not re-derived: `docs/planning/decisions/C2-process-model.md` §9's three-point
argument — the daemon is a **user-owned** process (no elevated or service account), it is
**auto-startable by the shim** rather than requiring a separate administrative step, and
it is **not installed as a service** (no supervisor unit file, no auto-start on login, no
multi-user daemon). And `docs/planning/decisions/C7-zenoh-transport.md` §5's "no
`zenohd`": OAC's transport peer runs in-process inside the daemon, in peer mode; no router
process is required for the default local path.

**Loopback bind and local certificate — transport box only.** Per
`docs/planning/decisions/C7-zenoh-transport.md` §5: local mode's Zenoh peer listens only
on `127.0.0.1`, including a TLS listener bound to `127.0.0.1` presenting an
automatically-generated, locally-stored certificate created on first run. Quoted,
`docs/planning/decisions/C7-zenoh-transport.md` §5: "there is no step in which a user
creates, installs, imports, or manages a certificate," satisfying
`docs/planning/DESIGN.md` line 115's "require no manual certificate management." This
paragraph is itself the transport box this file's §1 boundary-discipline rule scopes
Zenoh vocabulary to.

---

## 13. LAN deployment topology

Scoped narrowly to what shapes v0.1 architecture; everything else is out of scope for
this file.

- **No new process kind.** Same daemon, same shims — only the transport box's
  configuration changes (local-only bind versus a LAN-facing listener).
- **A pairing/authorization dependency.** LAN mode requires a short-code pairing flow
  that issues certificates, per `docs/planning/decisions/C5-envelope-auth.md` §10(b),
  wired to the transport's TLS configuration in `docs/planning/decisions/
  C7-zenoh-transport.md` §6. This is a dependency this architecture file names, not a flow
  it diagrams — the pairing flow itself belongs to `06-security.md`.
- **ACL subjects are certificate common name or username, never `zid`, default-deny.**
  Quoted, `docs/planning/decisions/C7-zenoh-transport.md` §6: "ACL subjects are
  certificate common name or username only — never `zid` — restated as finalized here."
  Default-deny is the starting posture for every ACL rule the transport ships with
  (`docs/planning/decisions/C7-zenoh-transport.md` §6-§7).

**Explicitly excluded.** Federation, routing, and multi-hop are deferred, per
`oac-boundaries` 11 ("Group rooms/broadcast... are explicitly deferred, not v0.1 work") —
this file draws no topology beyond one LAN hop between two devices' daemons. LAN launch
mechanics (how a user actually initiates pairing from the CLI) are deferred to
`docs/planning/v0.1/08-cli-and-deployment.md` — this section states only what shapes the
architecture, not the command sequence.

---

## 14. Rejected architectural alternatives

Decisions, not options — one line each, with the reversal condition given by reference
rather than restated.

- **Per-session embedded transport peer** (a peer inside each `oac mcp-shim` instead of
  the daemon). Rejected — `docs/planning/decisions/C2-process-model.md` §3 option (b).
  Reversal condition: `docs/planning/decisions/C2-process-model.md` §11.
- **Hybrid: shim embeds the peer, daemon holds only keys.** Rejected —
  `docs/planning/decisions/C2-process-model.md` §3. Reversal condition:
  `docs/planning/decisions/C2-process-model.md` §11.
- **Loopback TCP for local IPC.** Rejected — `docs/planning/decisions/
  C2-process-model.md` §4, "Rejected: loopback TCP" (no OS-level peer authentication).
  Reversal condition: `docs/planning/decisions/C2-process-model.md` §11.
- **[transport] Router process (`zenohd`) on the default local path.** Rejected —
  `docs/planning/decisions/C7-zenoh-transport.md` §10. Reversal condition:
  `docs/planning/decisions/C7-zenoh-transport.md` §11.

---

## 15. Cross-reference block

Every reference below is a repo-relative path; no prior context is assumed.

- `docs/planning/ADR-001.md`
- `docs/planning/DESIGN.md`
- `docs/planning/PLANNING-PROMPT.md` §9 item 5, §10
- `docs/planning/decisions/C2-process-model.md`
- `docs/planning/decisions/C7-zenoh-transport.md`
- `docs/planning/decisions/C4-session-identity.md`
- `docs/planning/decisions/C5-envelope-auth.md`
- `docs/planning/decisions/C6-trust-rendering.md`
- `docs/planning/v0.1/03-decisions-and-amendments.md`
- `docs/planning/v0.1/05-interfaces.md`
- `docs/planning/v0.1/06-security.md`
- `docs/planning/v0.1/07-repository-and-dependencies.md`
- `docs/planning/v0.1/08-cli-and-deployment.md`
- `docs/planning/v0.1/09-test-strategy.md`
- `docs/planning/STATUS.md`
- `docs/planning/PINS.md`

---

## 16. Evidence pass

Per `oac-evidence` §8, checked against this file:

- Every provider surface is labelled at first mention: Claude Code Channels —
  **research preview** (§7); Codex app-server live-inject — **experimental (per-method
  gating)** (§9).
- Preview/experimental surfaces carry a shim boundary and pinned version: Claude —
  `adapters/claude/` (UNVERIFIED — C11, not yet fixed in `DESIGN.md`), `v2.1.274` (§7);
  Codex — `adapters/codex/` (UNVERIFIED — same C11), `@openai/codex@0.154.0` @
  `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` (§9).
- Every verbatim API name (`capabilities.experimental["claude/channel"]`,
  `notifications/claude/channel`, `thread/queue/add`, `turn/steer`, `turn/start`,
  `CODEX_HOME/app-server-control/app-server-control.sock`, `codex mcp add`) is traced to
  PLANNING-PROMPT.md §3.1/§3.2, retrieved 2026-09-15 (§7, §9, §10).
- No new UNVERIFIED item is added by this file. Three already-open UNVERIFIED facts are
  carried forward with their label intact, not silently promoted: the Codex implicit
  daemon-attach default in `0.154.0` (§9), the `codex mcp-server` deprecation/deletion
  dates (§10), and the named compatibility shim boundary for both preview/experimental
  surfaces (§7, §9; ledger entry C11). All three already appear in
  `docs/planning/STATUS.md`'s "Open UNVERIFIED items" list and are not restated as new
  here.

---

## 17. Boundary pass

Per `oac-boundaries`' pre-commit self-check, run against this file:

- `rg -n -i '\bzenoh\b|\bzid\b|key[_-]?expr|liveliness'` hits in this file, complete: the
  gate caveat (labelled `[transport]`, preceding §1), §1 (the stated exceptions' own
  explanation), §5's nested transport box and its caption, §12's transport box paragraph,
  §13's ACL paragraph (quoting `zid`'s exclusion, not using it as a subject — the mention
  is of the *rejected* term), §14's `zenohd` bullet (labelled `[transport]`). §3 item 1's
  daemon-ownership list uses "discovery, presence, and pub/sub state" — "liveliness" was
  removed from that neutral list so it would not need its own exception. Every remaining
  hit sits inside a box or paragraph labelled `[transport]`/**transport**, or is this
  file's own boundary-discipline prose describing the rule (§1), never inside an
  unlabelled neutral component box or arrow.
- `Claude`/`Codex`/MCP-method-name hits, complete: §7-§10 prose, each explicitly labelled
  **adapter**; §2's component table (Daemon row, `[adapter: Codex]`), §3 item 4
  (`[adapter: Codex]`), §5's diagram (harness boxes' `[adapter: ...]` labels and the
  daemon box's `[adapter: Codex]` app-server-client line), and §12's diagram (harness
  boxes' `[adapter: ...]` labels) — each of these four carries the same explicit
  `[adapter: ...]` bracket label §5 already used for its harness boxes, so every hit sits
  inside a box or paragraph explicitly labelled adapter. No neutral-flow arrow (§7's,
  §8's, §9's, §10's flow-list arrows themselves) names a provider or a transport term —
  only the labelled box or paragraph beneath/beside each flow does.
- Nothing in this file has OAC owning a turn loop (every inbound/outbound flow ends at, or
  begins from, the harness's own provider-native operation — OAC never runs a turn
  itself), holding provider credentials (§10's credential-boundary paragraph states the
  opposite explicitly), or polling an inbox while claiming active inbound (§6 states EOF,
  not polling, drives presence; §9/§10's flows are live-inject/tool-call, not polled
  reads).

---

## 18. Acceptance close-out

Ticked against issue #22's five acceptance boxes, in the style of `docs/planning/
decisions/C7-zenoh-transport.md` §13.

- [x] **Inbound and outbound flows drawn separately per provider** — §7 (Claude inbound),
      §8 (Claude outbound), §9 (Codex inbound), §10 (Codex outbound); four separate flow
      lists, none merged.
- [x] **Process boundaries show which process holds key material and the transport
      peer** — §5's diagram ("key material: none" on every box but the daemon; the
      nested transport box inside the daemon), §3 (the four things the daemon owns,
      cited to C2 §1).
- [x] **Local topology needs no container, cloud account, or separately administered
      server** — §12, citing `docs/planning/decisions/C2-process-model.md` §9's
      three-point argument and `docs/planning/decisions/C7-zenoh-transport.md` §5's "no
      `zenohd`."
- [x] **LAN topology shown only where it shapes v0.1 architecture** — §13, scoped to
      three points (no new process kind; the pairing dependency; ACL-subject/default-deny
      rule) with federation/routing/multi-hop and LAN launch mechanics explicitly
      excluded.
- [x] **Provider and transport names appear only in adapter and transport boxes** —
      enforced by §1's two stated exceptions (nested transport box; `[adapter: ...]`
      bracket labels on the four remaining provider-specific mentions) and verified by
      §17's complete boundary pass.

**Cross-file updates in this change.**

- `docs/planning/STATUS.md`: add this file to the "Last updated" line as landed (issue
  #22, backlog key A5), alongside the existing A2/A4 pointers; no pin moved, no gate
  verdict changed, no new UNVERIFIED item added (§16).
- No `docs/planning/v0.1/README` or index file exists in this package yet — no index-line
  update is owed by this change.
