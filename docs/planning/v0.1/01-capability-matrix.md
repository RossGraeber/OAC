# 01 — Capability matrix

**Purpose.** One row per capability per surface: what each of Open Agent Channel (OAC)'s
five external surfaces — Claude Code (Channels), Codex CLI / app-server, MCP, Zenoh, ACP —
actually supports today, at the pinned version, with a first-party source, a retrieval
date, and a named gap. This file is the capability-level evidence record the rest of the
`docs/planning/v0.1/` package and the Epic C decisions cite rather than re-derive.

**Evidence standard.** Every claim below follows `oac-evidence` §2's citation format
(URL + version/commit + retrieval date) and §4's one-label-per-surface rule
(`supported` / `research preview` / `experimental` / `undocumented`). Facts already
recorded in `docs/planning/PINS.md` are cited by pointing at that file's record rather
than re-quoted from the original source a second time — this file adds no new pins.

**`docs/planning/PINS.md` is primary; its pin table is authoritative.** Where this file
and `docs/planning/PINS.md` could be read as disagreeing about a version, a release date,
or a URL, `docs/planning/PINS.md` wins — this file restates PINS.md's rows, it does not
re-verify or supersede them.

**Retrieval-date convention.** Every retrieval date in this file is carried unchanged
from the source record it cites (`docs/planning/PINS.md` or a `docs/planning/decisions/
C<n>-*.md` file) — it is **not** restamped with today's date. A date in this file
reports when the underlying fact was actually observed, not when this file was written.

**Cross-references (repo-relative paths).** `docs/planning/PLANNING-PROMPT.md` §3
(pre-verified baseline) and §9.2 (this file's required content, "one row per capability
across Claude Code, Codex, MCP, Zenoh, ACP with status/version/source/date/gap");
`docs/planning/PINS.md` (pin table and pin records, single source of truth for
versions); `docs/planning/STATUS.md` (current stage, gate verdicts, decisions landed,
Open UNVERIFIED items); `docs/planning/decisions/C1-language-runtime.md` through
`docs/planning/decisions/C7-zenoh-transport.md` (the seven landed Epic C decisions this
file draws its shim-boundary and C7-record content from).

**Naming.** Product and repository: **Open Agent Channel (OAC)**. Normative protocol
specification: **OAC Session Channels**. CLI binary: **`oac`**. Per ADR-001-A1
(`docs/planning/ADR-001-AMENDMENTS.md`), this file never writes "Session Channels" alone
or `sessionchannels`.

---

## 1. Surface labels

Per `oac-evidence` §4, each surface gets exactly one label, stated at first mention.
Rows below are copied verbatim from `docs/planning/PINS.md`'s pin table — see that file
for the full pin record behind each row.

| Surface | Label | Pinned version | Release date | Observed-at URL | Retrieved | Shim boundary |
|---|---|---|---|---|---|---|
| Claude Code (Channels) | research preview | `v2.1.274` | 2026-09-17T00:12:02Z (UTC) | https://github.com/anthropics/claude-code/releases/tag/v2.1.274 | 2026-09-16 | `shim boundary: UNNAMED — see docs/planning/DESIGN.md` |
| Codex CLI / app-server | experimental (per-method gating via `capabilities.experimentalApi`) | `@openai/codex@0.154.0`, commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` | 2026-09-09 | https://github.com/openai/codex/releases/tag/rust-v0.154.0 | 2026-09-16 | `shim boundary: UNNAMED — see docs/planning/DESIGN.md` |
| MCP — current era | supported | `2026-07-28` | 2026-07-28 | https://modelcontextprotocol.io/specification/2026-07-28/ | 2026-09-16 | n/a — `supported` surface |
| MCP — legacy era | supported | `2025-11-25` | 2025-11-25 | https://modelcontextprotocol.io/specification/2025-11-25/ | 2026-09-16 | n/a — `supported` surface |
| Zenoh | supported | `1.10.1` | 2026-09-07 | https://github.com/eclipse-zenoh/zenoh/releases | 2026-09-16 | n/a — `supported` surface |
| ACP (forward-compat only) | supported | protocol version `1` (schema v2 alpha) | not stated on source page | https://agentclientprotocol.com/protocol/ | 2026-09-16 | n/a — `supported` surface; **not a v0.1 dependency** |

Five surfaces, five rows, one label each (MCP's two eras are recorded as two rows of the
same `supported` label per `docs/planning/PINS.md`'s own pin-table shape, not a second
surface). Only Claude Code (Channels) and Codex CLI / app-server carry the
`research preview` / `experimental` labels that require a shim-boundary entry (§2).

## 2. Shim boundary, carried honestly

Per `oac-evidence` §4: "`research preview` and `experimental` surfaces get two
additional things: a named compatibility shim boundary ... and a pinned version." The
pinned versions are in §1. The shim boundary itself is carried exactly as
`docs/planning/PINS.md` states it for both surfaces:

> `shim boundary: UNNAMED — see docs/planning/DESIGN.md`

This is not a placeholder this file invents — it is `docs/planning/PINS.md`'s own
Claude Code Channels and Codex CLI/app-server pin records, quoted verbatim (both say the
identical sentence; `docs/planning/PINS.md` — "Compatibility shim boundary: DESIGN.md
names no module boundary specific to the [surface] ... `shim boundary: UNNAMED — see
DESIGN.md`").

**The narrowing `docs/planning/decisions/C4-session-identity.md` §16 made.** C4 does not
name the module, but it fixes the boundary's *behaviour*: harness-native-id capture
(Claude's `session_id`, Codex's `thread.id`) happens **only** through daemon-owned
per-harness capture code, reached from the harness process **only** via the named
hook/JSON-RPC surfaces (`SessionStart` hook stdin for Claude, the daemon's own
app-server client's `thread/start`/`thread/started` observation for Codex) — never by
reading a harness's on-disk files, and never by a per-session process holding its own
copy of the capture logic — over C2's local IPC (`docs/planning/decisions/
C2-process-model.md` §4). Quoted, C4 §16: "harness-native-id capture happens **only**
through the daemon-owned per-harness capture code, reached from the harness process
**only** via the hook/JSON-RPC surfaces named there, over C2's local IPC." This fixes
*behaviour*, not a module path — **the module name stays open**, deliberately not
invented here (per `oac-evidence` §3's "if you cannot point at the line ... it is
invented" test; `docs/planning/DESIGN.md` defines no module list to draw a name from).
Owner: Epic F/G adapter implementation tasks (`oac-implementation`), the stage where a
concrete module path (e.g. an eventual `adapters/claude`, `adapters/codex` directory)
will actually be created.

**Both items stay open, referenced here, not closed.** Both shim-boundary entries remain
in `docs/planning/STATUS.md`'s "Open UNVERIFIED items" list, as risk items 9
(Claude Code Channels preview shim boundary) and 10 (Codex experimental live-inject shim
boundary). This file references that ledger; it does not close either item, and does not
claim a module path that does not exist.

## 3. Capability matrix

One row per surface per capability. Columns: Capability | Surface | Status | Pinned
version | Verbatim API names | First-party source URL | Retrieved | Gap.

### 3.1 Inbound wake / injection — external event becomes a user turn

| Capability | Surface | Status | Pinned version | Verbatim API names | First-party source URL | Retrieved | Gap |
|---|---|---|---|---|---|---|---|
| External event becomes a user turn | Claude Code | research preview | `v2.1.274` | `notifications/claude/channel` (`content`: string, `meta`: string-to-string map) | https://code.claude.com/docs/en/channels-reference.md | 2026-09-16 | No attach to an already-running session — a channel wakes an idle session; a mid-turn notification is queued and delivered in order on the next turn |
| External event becomes a user turn | Codex | experimental (per-method gating) | `@openai/codex@0.154.0`, commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` | `thread/queue/add` (queued until idle, experimental), `turn/steer` (injects into an active turn), `turn/start` (when idle); CLI `codex queue --thread <id> --message <text>`; daemon `codex app-server daemon start`; control socket `CODEX_HOME/app-server-control/app-server-control.sock` | https://github.com/openai/codex/releases/tag/rust-v0.154.0 (pin); `docs/planning/PINS.md` — "Codex CLI and app-server" | 2026-09-16 | Whether implicit daemon-attach executes by default at runtime in the pinned build is UNVERIFIED — source-confirmed present at the pinned commit, runtime behaviour is gate G2's go/no-go, owner D2/G2 |
| External event becomes a user turn | MCP | **no capability** — structural gap | current `2026-07-28` | none — `initialize` is gone at this revision; servers cannot initiate requests or push unsolicited content | https://modelcontextprotocol.io/specification/2026-07-28/ | 2026-09-16 | Structural: no MCP revision defines "external event becomes a user turn" — this is the §4 finding `docs/planning/ADR-001-AMENDMENTS.md` §ADR-001-A3 rests on, not a version-specific limitation that a future MCP revision is expected to lift |
| External event becomes a user turn | Zenoh | supported — transport only | `1.10.1` | n/a — Zenoh carries bytes; it defines no session-wake semantic | https://github.com/eclipse-zenoh/zenoh/releases | 2026-09-16 | No session-wake semantics at this layer by design — wake semantics are entirely provider-native (Claude/Codex rows above); Zenoh is the transport underneath OAC's own protocol, not a wake mechanism itself |
| External event becomes a user turn | ACP | supported | protocol version `1` (schema v2 alpha) | `session/prompt` (into a session owned by the ACP client), `session/load`, `session/resume` | https://agentclientprotocol.com/protocol/ | 2026-09-16 | No documented API pushes input into a live local Cursor session OAC does not own — `session/prompt` operates only inside a session the calling ACP client itself owns and drives; see §4 for the full C7 record |

### 3.2 Outbound reply and correlation

| Capability | Surface | Status | Pinned version | Verbatim API names | First-party source URL | Retrieved | Gap |
|---|---|---|---|---|---|---|---|
| Outbound reply | Claude Code | research preview | `v2.1.274` | ordinary MCP tools (conventionally `reply`); correlation is convention-only, via `meta` attributes echoed back on the reply | https://code.claude.com/docs/en/channels-reference.md | 2026-09-16 | `meta` keys must be identifier-safe (letters, digits, underscore) or are silently dropped — an OAC provenance key that fails this test never arrives, per `docs/planning/decisions/C6-trust-rendering.md` §2's const key table and refusal-fixture design |
| Outbound reply / correlation | Codex | experimental (per-method gating) | `@openai/codex@0.154.0`, commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` | events `thread/started`, `thread/status/changed`, `turn/started`, `turn/completed` (`completed`\|`interrupted`\|`failed`), `item/started`, `item/completed` (authoritative), `item/agentMessage/delta` | https://github.com/openai/codex/releases/tag/rust-v0.154.0 (pin); `docs/planning/PINS.md` — "Codex CLI and app-server" | 2026-09-16 | No channel-tag convention on Codex the way Claude's `meta` echo provides — correlation is a layered rule (explicit `in_reply_to`, adapter-independent thread-id/turn-id binding, explicit inferred/uncorrelated downgrade), per `docs/planning/decisions/C6-trust-rendering.md` §10 |
| Outbound reply / provenance | MCP | supported — tool surface + `_meta` provenance | current `2026-07-28` | `_meta["io.modelcontextprotocol/protocolVersion"]` | https://modelcontextprotocol.io/specification/2026-07-28/ | 2026-09-16 | None — `_meta` provenance and the ordinary tool-call surface are the supported mechanism outbound reply rides on for both harnesses; the structural gap is inbound only (§3.1) |

### 3.3 Negotiation / capability keys

| Capability | Surface | Status | Pinned version | Verbatim API names | First-party source URL | Retrieved | Gap |
|---|---|---|---|---|---|---|---|
| Channel + permission-relay negotiation | Claude Code | research preview | `v2.1.274` | `capabilities.experimental['claude/channel']`, `capabilities.experimental['claude/channel/permission']`, `notifications/claude/channel/permission_request`, `notifications/claude/channel/permission` | https://code.claude.com/docs/en/channels-reference.md | 2026-09-16 | Permission relay floor `>= v2.1.234`, satisfied at pinned `v2.1.274`; kept off by default in v0.1 per `docs/planning/decisions/C6-trust-rendering.md` §7 (conflict C10, `RESOLVED-IN-DECISION`) — not a capability gap, a deliberate default |
| Handshake / experimental-method gating | Codex | experimental (per-method gating) | `@openai/codex@0.154.0`, commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` | `initialize` then `initialized` (JSON-RPC 2.0 over stdio/JSONL), `--listen ws://127.0.0.1:PORT`, Unix socket; no numeric protocol version; `capabilities.experimentalApi` | https://github.com/openai/codex/releases/tag/rust-v0.154.0 (pin); `docs/planning/PINS.md` — "Codex CLI and app-server" | 2026-09-16 | Experimental methods require declaring `capabilities.experimentalApi`; no gap beyond the surface's own `experimental` label |
| Extension negotiation | MCP | supported | SEP-2133, Final | SEP-2133 extensions, identifier format `{vendor-prefix}/{extension-name}`; OAC's own identifier `io.github.rossgraeber/oac-session-channels` (`docs/planning/decisions/C3-spec-packaging.md`) | https://modelcontextprotocol.io/seps/2133-extensions | 2026-09-16 | None for the extension-identifier mechanism itself. **No reserved-prefix rule is restated here** — B2 resolved that claim as drift (`docs/planning/STATUS.md` "Closed in B2": "SEP-2133's reserved-prefix rule ... is **drift, not UNVERIFIED** — no such clause exists in the SEP text"); no such clause exists in SEP-2133 |

### 3.4 Dual-era MCP, loading, and distribution

| Capability | Surface | Status | Pinned version | Verbatim API names | First-party source URL | Retrieved | Gap |
|---|---|---|---|---|---|---|---|
| Dual-era negotiation constraint | Claude Code / MCP | research preview (Claude) / supported (MCP) | `v2.1.274` (Claude); `2026-07-28` current, `2025-11-25` legacy (MCP) | `MCP_PROTOCOL_NEGOTIATION=legacy`; verbatim, "if you set `MCP_PROTOCOL_NEGOTIATION` to `auto` and a channel server negotiates MCP protocol revision 2026-07-28, it can't deliver channel messages, so Claude Code doesn't register it as a channel"; constraint floor "channel servers must negotiate `2025-11-25` or earlier" | https://code.claude.com/docs/en/mcp.md | 2026-09-16 | None — this is the fixed floor, not a gap; it is the reason G4's dual-era design (one process serving both MCP eras) exists |
| Channel loading | Claude Code | research preview | `v2.1.274` | `--channels plugin:<name>@<marketplace>`, `--channels server:<name>`, `--dangerously-load-development-channels`; allowlists `claude-plugins-official` / `allowedChannelPlugins` / `channelsEnabled`; not available on Bedrock, Vertex, or Foundry; `--channels` absent from `claude --help` | https://code.claude.com/docs/en/channels.md; PLANNING-PROMPT.md §3.1 | 2026-09-16 | `--channels` does not appear in `claude --help` output — undocumented in the CLI's own self-help surface, documented only on the channels reference page |
| Outbound tool registration | Codex | supported (registration surface) / experimental (methods behind it) | `@openai/codex@0.154.0`, commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` | `codex mcp add oac -- oac mcp-shim` | https://github.com/openai/codex/releases/tag/rust-v0.154.0 (pin); `docs/planning/PINS.md` — "Codex CLI and app-server" | 2026-09-16 | None — this is the supported way OAC registers as an outbound tool for Codex |
| **Removed surface** | Codex | **removed** | n/a — removed, no version serves it | `codex mcp-server` — deprecated 2026-08-20, deleted 2026-09-05 | PLANNING-PROMPT.md §3.2 (UNVERIFIED — dates carried unchanged, not independently re-confirmed against the CLI reference in B1 or B2, per `docs/planning/STATUS.md` "Open UNVERIFIED items") | 2026-09-16 | Do not plan on it — `codex mcp-server` is gone; `codex mcp add` (row above) is the supported path |

### 3.5 Identity, transport, security

| Capability | Surface | Status | Pinned version | Verbatim API names | First-party source URL | Retrieved | Gap |
|---|---|---|---|---|---|---|---|
| Harness-native session identity | Claude Code | research preview | `v2.1.274` | `session_id` in hook input (e.g. `SessionStart`) | https://code.claude.com/docs/en/hooks.md | 2026-09-16 | No documented `CLAUDE_SESSION_ID` environment variable — confirmed absent, closed in B2, not carried UNVERIFIED (`docs/planning/decisions/C4-session-identity.md` §3) |
| Harness-native session identity | Codex | experimental (per-method gating) | `@openai/codex@0.154.0`, commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` | `thread.id` (UUIDv7, survives restarts); rollouts under `CODEX_HOME/sessions/` | https://learn.chatgpt.com/docs/app-server | 2026-09-17 | The rollout file format under `CODEX_HOME/sessions/` is explicitly **not** a supported surface — OAC never reads it |
| Provider credential boundary | Codex | experimental (per-method gating) | `@openai/codex@0.154.0`, commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` | `CODEX_HOME/auth.json` or OS keyring | PLANNING-PROMPT.md §3.2 | 2026-09-16 | None — OAC never holds OpenAI credentials; this is a fixed boundary (`[ADR-001 Boundary]` "MUST NOT steal or reuse another harness's provider credentials"), not a capability gap |
| Transport peer discovery | Zenoh | supported | `1.10.1` | peer mode default; scouting `224.0.0.224:7446`; `interface: "auto"`; loopback discovery fix floor `>= 1.10.0` (PR #2671) | https://github.com/eclipse-zenoh/zenoh/pull/2671; `docs/planning/PINS.md` — "Zenoh" | 2026-09-16 | None at the pinned version (`1.10.1 >= 1.10.0`, floor satisfied); Gate G3 (`NOT RUN`) is the runtime proof this floor's real-world behaviour still needs |
| Stable surface scope | Zenoh | supported | `1.10.1` | `zenoh` + `zenoh-ext` only (stable API); `unstable` feature not used | https://github.com/eclipse-zenoh/zenoh/releases; `docs/planning/decisions/C7-zenoh-transport.md` §8 | 2026-09-16 | None — every feature this design touches is verified stable, per C7 §8's table |
| ACL subject authentication | Zenoh | supported | `1.10.1` | `zid` (explicitly unauthenticated ACL subject) | `docs/planning/decisions/C5-envelope-auth.md` §12; `docs/planning/decisions/C7-zenoh-transport.md` §6 | 2026-09-16 | `zid` ACL subjects are explicitly unauthenticated — OAC policy maps only onto authenticated ACL subjects (certificate common name or username), never `zid` |
| Application-layer message signing | Zenoh | supported (transport) — **gap closed by OAC, not by Zenoh** | `1.10.1` | n/a — Zenoh provides no application-layer message signing | `docs/planning/decisions/C7-zenoh-transport.md` §7, quoting `oac-zenoh` §6 | 2026-09-16 | Zenoh itself provides no payload signing; gap closed by OAC's own envelope signature (`docs/planning/decisions/C5-envelope-auth.md`, Ed25519 via `ed25519-dalek`, `verify_strict`). **No Zenoh concept — `zid`, key expression, liveliness token — may leak into the neutral protocol** (`[ADR-001 Boundary]` "MUST NOT leak Zenoh-specific concepts into the neutral protocol"), a boundary this file does not itself violate: every Zenoh-specific term above is confined to this row's own cells, describing the Zenoh surface, not proposing neutral-protocol vocabulary |

## 4. C7 record — ACP is a client-owned-session protocol, not a channel

**Stated explicitly, as the issue #24 acceptance item requires: ACP is a
client-owned-session protocol, not a channel.** Per §3.1's ACP row, `session/prompt`
operates only inside a session the calling ACP client itself already owns and drives —
there is no documented API by which an external party pushes input into a live local
Cursor (or other ACP-hosted) session it does not own. This is a structural property of
the protocol, not a version-specific gap `docs/planning/PINS.md`'s pinned protocol
version `1` might close in a later revision.

**This file is the named resolution home for conflict-register row C7.** Per
`docs/planning/ADR-001-AMENDMENTS.md`'s conflict register table, row C7 ("ACP is
client-owned-session, not a channel") is `RESOLVED-BY-EVIDENCE`, with "Resolution lives
in `docs/planning/v0.1/01-capability-matrix.md`." Per `docs/planning/
PLANNING-PROMPT.md` Appendix A, C7's expected resolution is "Note in capability matrix."
This section is that note.

**Evidence.** `docs/planning/REVERIFICATION-B2.md` §3.5 row "C7 framing: ACP is
client-owned-session, not a channel" — **HOLDS**. Quoted: "Confirmed by the protocol
page's own architecture description (client drives the session; nothing on the page
describes server-initiated push into an already-open client session the way Claude
Channels or a Codex daemon attach does)." Source:
https://agentclientprotocol.com/protocol/, retrieved 2026-09-16.

**Collision warning, restated from `docs/planning/decisions/C7-zenoh-transport.md`
§"Cross-file updates in this change."** Register row `C7` (this section's subject, "ACP
is client-owned-session, not a channel") is **not** the same thing as Epic C backlog
task `C7` ("Decide the Zenoh transport mapping and its containment boundary," issue #20,
`docs/planning/decisions/C7-zenoh-transport.md`). These are two independent numbering
schemes — the conflict register's `C`-prefixed rows (`docs/planning/
ADR-001-AMENDMENTS.md`), and Epic C's own `C`-prefixed backlog task keys — that happen
to land on the same label. Quoted, `docs/planning/decisions/C7-zenoh-transport.md`
§"Cross-file updates in this change": "the conflict-register row literally named `C7` in
`ADR-001-AMENDMENTS.md` line 395 and `PLANNING-PROMPT.md` Appendix A line 229 is a
**different, unrelated** item ... a coincidence of two independent numbering schemes."
This file's §4 resolves the former (the ACP conflict-register row); it does not touch,
resolve, or reference the Zenoh transport-mapping decision.

**Consequence.** ACP is forward-compatibility only, and is **not a v0.1 dependency** —
no gate in `docs/planning/STATUS.md`'s Gate verdicts table (G1-G5) depends on it. ACP
adapters exist for Codex (which wraps the app-server) and for the Claude Agent SDK (not
the Claude Code CLI) — named here as context for why ACP appears in this matrix at all,
not as a v0.1 build target.

## 5. Gaps and UNVERIFIED ledger

Every row in §3 carries a filled `Gap` cell — "none" is written only where the row's
capability truly has no open gap (§3.3's extension-negotiation row, §3.4's dual-era
constraint and outbound-registration rows, §3.5's credential-boundary and
stable-surface-scope rows). Every other row states its gap plainly.

**UNVERIFIED items touched by this file, carried per `oac-evidence` §5 (never
promoted here — each already appears in `docs/planning/STATUS.md`'s "Open UNVERIFIED
items" list):**

- Claude channel behaviour across `--resume`/`--continue` (UNVERIFIED — docs silent at
  `v2.1.274`; see `docs/planning/REVERIFICATION-B2.md` §3.1 box 1;
  `docs/planning/decisions/C4-session-identity.md` §6/§15).
- Whether one MCP server can present more than one logical channel
  (one-server-many-channels) (UNVERIFIED — docs silent at `v2.1.274`; see
  `docs/planning/REVERIFICATION-B2.md` §3.1 box 2).
- "Research preview on Claude Code `v2.1.232+`" floor (UNVERIFIED — not confirmable on
  `channels.md` at `v2.1.274`; see `docs/planning/REVERIFICATION-B2.md` §3.1 box 7 and
  `docs/planning/PINS.md` floor 1).
- Whether the Claude Agent SDK supports Channels — **closed in B2**, not carried open:
  confirmed absent from the Agent SDK's own capability table (`docs/planning/STATUS.md`
  "Closed in B2"); recorded here only because §4 names the Agent SDK as an ACP adapter
  host, not because the Channels-support question is still open.
- Whether implicit Codex daemon attach executes by default at runtime in the pinned
  release `0.154.0` / commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`
  (**PARTIALLY RESOLVED** — source code confirmed present at the pinned commit (control
  socket path, attach-or-embed branch, `codex queue` subcommand); runtime behaviour is
  **CARRIED**, owner **D2/G2**; see `docs/planning/REVERIFICATION-B2.md` §3.2 box 4 and
  `docs/planning/PINS.md` "Daemon-attach open question (G2 input)").
- Whether Codex Desktop exposes the control socket in current builds (UNVERIFIED — no
  first-party statement found; see `docs/planning/REVERIFICATION-B2.md` §3.2 box 5).
- Zenoh `auth.pubkey` semantics (UNVERIFIED — the six key names are CLOSED, confirmed
  verbatim in `DEFAULT_CONFIG.json5` at tag `1.10.1`; semantics remain open; see
  `docs/planning/REVERIFICATION-B2.md` §3.4 box 6).
- Zenoh crate version/date read from GitHub releases rather than crates.io directly
  (UNVERIFIED — re-confirm on crates.io when reachable; see `docs/planning/PINS.md`).
- The 5-15 MB Zenoh binary size estimate (UNVERIFIED — derived estimate, resolved by the
  first G3 build artifact, task D3; see `docs/planning/REVERIFICATION-B2.md` §3.4 box 7).
- ACP schema v2 "alpha" status (UNVERIFIED — carried from PLANNING-PROMPT.md §3.5 only,
  not independently re-confirmed on agentclientprotocol.com in B1 or B2; low priority,
  ACP is not a v0.1 dependency; see `docs/planning/REVERIFICATION-B2.md` §3.5).

**Second required home.** Per the task instruction and `oac-evidence` §5, `docs/planning/
v0.1/11-risks.md` (Epic A task A12) is the second required home for every UNVERIFIED item
above once it exists. Until then, `docs/planning/STATUS.md`'s "Open UNVERIFIED items"
list is the ledger of record; this file does not remove or promote any entry from it.

---

## Self-check (`oac-evidence` §8, `oac-planning-package` §6)

- [x] Every row carries a URL + version/commit and a retrieval date, or cites a
      `docs/planning/PINS.md` / `docs/planning/decisions/C<n>-*.md` record that already
      carries them.
- [x] Every quoted method name, capability key, flag, and path is copied verbatim from
      `docs/planning/PINS.md` or a landed `docs/planning/decisions/C<n>-*.md` file — none
      invented; the reserved-prefix rule is explicitly **not** restated (§3.3), per B2's
      drift finding.
- [x] Five surfaces, five labels — one each (§1); MCP's two eras share the one
      `supported` label as two pin-table rows, matching `docs/planning/PINS.md`'s own
      shape, not a sixth surface.
- [x] Both preview/experimental surfaces (Claude Code Channels, Codex CLI/app-server)
      name their shim-boundary state (`UNNAMED — see docs/planning/DESIGN.md`) plus C4's
      behavioural narrowing (§2) — no module path invented.
- [x] C7 (register row) is recorded, with the register-row-vs-backlog-task collision
      warning stated explicitly (§4).
- [x] No neutral-interface text in this file proposes Zenoh/Claude/Codex vocabulary as
      OAC's own protocol vocabulary — every provider- or transport-specific term appears
      only inside that surface's own matrix cell, describing that surface, per
      `[ADR-001 Boundary]` "MUST NOT leak Zenoh-specific concepts into the neutral
      protocol" (checked directly against §3.5's Zenoh rows and §4's ACP section).
- [x] Naming resolution (ADR-001-A1) applied throughout — no "Session Channels" alone,
      no `sessionchannels`.
- [x] No restatement of ADR-001/DESIGN prose where a path reference serves instead —
      shim-boundary, C7, and identity/transport content are cited from
      `docs/planning/decisions/C4-session-identity.md` and `docs/planning/decisions/
      C7-zenoh-transport.md` rather than re-derived.
- [x] No ADR-001 boundary violation — verified against `oac-boundaries`' grep/lint
      checks description; this file adds no code, only prose, and its Zenoh vocabulary
      is confined to describing the Zenoh surface itself (see the neutral-vocabulary
      check above).
