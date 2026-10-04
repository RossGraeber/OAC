# OAC Session Channels — MCP binding

- **Document:** the MCP binding for OAC Session Channels (backlog task E6, issue #46).
- **Status:** Stage 2 draft, normative once Gate S2 freezes the spec surface
  (`docs/planning/v0.1/10-stages.md` §6). Every requirement carries an `MCPB` id; the index
  in §12 lists its fixtures or marks it `TODO(fixture)`.
- **Binding revision:** `0.1` (draft until Gate S2; fixtures cite `spec_revision` `0.1`).
- **Extension identifier:** `io.github.rossgraeber/oac-session-channels`
  (`docs/planning/decisions/C3-spec-packaging.md` §1-§3).
- **MCP revisions bound:** current (modern) era `2026-07-28`; legacy era `2025-11-25`
  (`docs/planning/PINS.md` "MCP revisions"; `docs/planning/STATUS.md` "Pins").
- **Skills:** `oac-spec-authoring`, `oac-mcp`, `oac-boundaries`, `oac-evidence`.

## 0. Why this document is separate

OAC Session Channels is a provider-neutral protocol. Its normative text lives in
`spec/session-channels.md` (envelope, addressing, negotiation, presence, delivery,
replies, delivery states, errors, versioning) and `spec/security.md` (signing, replay,
authorization, provenance). Those documents name no transport, no provider and no MCP
method. `[DESIGN §OAC Session Channels specification, packaged as an MCP extension]`
forbids the specification from naming transport keys, topics or subjects, or
provider-specific method names; the exact sentence is quoted in `oac-spec-authoring` §3.
This document cites it by reference only, so that it carries no transport vocabulary.

This document is the one place where OAC is mapped onto the Model Context Protocol. MCP
is one binding of OAC Session Channels, not the protocol. It carries three things only:
capability negotiation, the tool surface, and `_meta` provenance
(`docs/planning/ADR-001-AMENDMENTS.md` §ADR-001-A3; C3 §1). It is the task-scoped
exemption from the neutral-vocabulary rule (`oac-spec-authoring` §3), so it quotes MCP and
provider identifiers verbatim.

**Precedence.** If this document and `spec/session-channels.md` or `spec/security.md`
disagree, the neutral document governs (C3 §1: "the standalone document under `spec/`
governs"). This document adds no requirement on message semantics. It says only how
those semantics are carried over MCP.

**Cross-dependency.** This document cites `spec/session-channels.md` (revision 0.1,
landed by #41) by section number. Sections 4 and 5 of that document are written. Its §6
(E2, #42), §7 (E3, #43) and §8 (E4, #44) are titled stubs, so a citation of them names a
section whose content is still to come. Error names here are placeholders by role,
pending the closed taxonomy of its §8.3 (E4, #44); see §5.4.

## 1. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD
NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be
interpreted as described in BCP 14 (RFC 2119, RFC 8174) when, and only when, they
appear in all capitals, as shown here.

- One requirement per sentence (`oac-spec-authoring` §1).
- Each normative sentence starts with a requirement id `[MCPB-<AREA>-<NNN>]`, under the
  scheme of `spec/session-channels.md` §3.2; §12.1 registers the `MCPB` prefix and its
  areas. §12.3 lists every id with its fixtures, or `TODO(fixture)` with the planned
  input and expected outcome.
- A **reference implementation note** is a labelled blockquote. It binds no other
  implementation and contains no BCP 14 keyword (`oac-spec-authoring` §2).
- `UNVERIFIED` marks a claim no first-party source states directly
  (`oac-evidence` §5). Every such claim is also listed in `docs/planning/STATUS.md`.
- **Roles.** The *OAC server* is the MCP server process OAC provides to a harness (the
  reference implementation's `oac mcp-shim`, C2 §1). The *client* is the harness's own MCP
  client. A *legacy-era connection* is one that began with `initialize`. A *modern-era
  request* is one that carries `_meta["io.modelcontextprotocol/protocolVersion"]` and no
  `initialize`.
- **MCP terms.** "Modern", "legacy" and "dual-era" are used exactly as the MCP versioning
  page defines them: modern is `2026-07-28` and later, legacy is "protocol versions that
  establish a session with an `initialize` handshake (`2025-11-25` and earlier)", and a
  dual-era implementation "supports both modern and legacy versions". Source:
  https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning, revision
  `2026-07-28`, retrieved 2026-10-03.

## 2. MCP is not the delivery mechanism

No MCP revision defines "external event becomes a user turn". That semantic is always
provider-native. Source: PLANNING-PROMPT.md §3.3, retrieved 2026-09-15; re-verified
HOLDS at `2026-07-28` in `docs/planning/REVERIFICATION-B2.md` §3.3, retrieved 2026-09-16.

The current revision says the same thing in structural terms. It is stateless, and a
server cannot push unsolicited content into the model's context: "Servers **MUST NOT**
rely on prior requests over the same connection to establish context", and long-lived
streams such as `subscriptions/listen` "remain request/response". Source:
https://modelcontextprotocol.io/specification/2026-07-28/basic, section "Statelessness",
revision `2026-07-28`, retrieved 2026-10-03.

[MCPB-DLV-001] An OAC server MUST NOT advertise, document or report
an MCP message as the mechanism that turns an OAC message into input for a live session.

[MCPB-DLV-002] An OAC server MUST NOT offer an MCP resource, prompt
or subscription that a session is expected to read repeatedly in order to receive OAC
messages, while the session's capabilities claim active inbound delivery.

Requirement MCPB-DLV-002 is the MCP form of the no-polling rule in `spec/session-channels.md` §7.1
(E3) (`oac-boundaries` boundary 14).

**2.3 — what does deliver.** Inbound delivery into a live session happens through a
provider-native surface, named in the provider profiles (§8):

- Claude Code (research preview): the `notifications/claude/channel` notification on a
  legacy-era connection that declared `capabilities.experimental["claude/channel"]`
  (`oac-claude-channels` §1, §4).
- Codex (experimental, per-method gating): the app-server methods `thread/queue/add`,
  `turn/steer` and `turn/start` (`oac-codex-appserver`; C3 §5). These are not MCP
  methods. Codex uses MCP only for the outbound tool path (§8.2).

`notifications/claude/channel` travels on an MCP connection, but MCP does not define it.
It is a Claude Code extension point outside every MCP revision. This document therefore
treats it as provider-native, exactly like the Codex app-server methods.

## 3. Extension identifier

**3.1 — the identifier.** The OAC extension identifier is
`io.github.rossgraeber/oac-session-channels`. Its derivation and ownership evidence are in
C3 §2-§3 and are not repeated here.

**3.2 — conformance to MCP naming rules.** At `2026-07-28`, "Extension identifiers
**MUST** follow the `_meta` key naming rules, with a mandatory prefix". Source:
https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning, section
"Extension Negotiation", retrieved 2026-10-03. SEP-2133 states the same rule: "The names
follow the same rules as the \_meta keys, except that the prefix is mandatory." Source:
https://modelcontextprotocol.io/seps/2133-extensions, status Final, retrieved 2026-10-03.
The identifier is checked against those rules, quoted in §6.1. The reservation of second
labels `modelcontextprotocol`/`mcp` is stated in the MCP base spec, not in SEP-2133's own
text; C3 §4 and `REVERIFICATION-B2.md` D2 still carry the narrower wording, and #257
corrects them.

| Rule | Value | Result |
|---|---|---|
| Prefix present (mandatory for identifiers) | `io.github.rossgraeber/` | yes |
| Each label starts with a letter, ends with a letter or digit, interior letters/digits/hyphens | `io`, `github`, `rossgraeber` | yes |
| Reverse DNS notation (SHOULD) | reversal of `rossgraeber.github.io` (C3 §3) | yes |
| Second label is not `modelcontextprotocol` or `mcp` (reserved) | second label `github` | not reserved |
| Name begins and ends with an alphanumeric; interior may hold `-`, `_`, `.` | `oac-session-channels` | yes |

[MCPB-EXT-001] An OAC server MUST use exactly the string
`io.github.rossgraeber/oac-session-channels` wherever this document requires the
extension identifier.

**3.4 — breaking changes.** "Breaking changes MUST use a new identifier" (SEP-2133,
above). What counts as breaking for OAC is defined once, in `spec/session-channels.md`
§5.3. A breaking change is a new major version, and it moves to `io.github.rossgraeber/oac-session-channels-v2` or later
(C3 §8).

[MCPB-EXT-002] An OAC server MUST NOT advertise the identifier above for a protocol
version whose major version is not 0.

Major version 0 maps one-to-one to this identifier (`spec/session-channels.md` §5.1
table).

**3.6 — the settings object.** MCP maps each extension identifier to "per-extension
settings objects", and "an empty object indicates support with no additional settings"
(MCP versioning page, above).

- [MCPB-EXT-003] The value an OAC server places under the
  identifier MUST be a JSON object.
- [MCPB-EXT-004] A receiver MUST ignore a settings member it does
  not recognize.
- The members of the settings object (for example the spec revision and the OAC
  capability set) are defined by `spec/session-channels.md` §6.4 and §6.5 (E2,
  #42). This binding defines none. Until E2 fills those sections, an OAC server that has
  nothing to declare sends `{}`.

Requirement MCPB-EXT-004 is the MCP form of the forward-compatibility rule in `spec/session-channels.md`
§4.8 (unrecognized members are ignored).

## 4. Era handling — the dual-era server

### 4.1 What the MCP specification allows

- "A server that wishes to support both legacy clients (which expect an `initialize`
  handshake) and modern clients (which use per-request metadata) **MAY** implement both
  behaviors."
- "A dual-era server **MAY** serve both eras concurrently on the same endpoint or
  process."
- "A dual-era **server** selects its behavior from how the client opens: A request
  carrying modern per-request `_meta` is served statelessly according to this revision.
  An `initialize` request selects legacy semantics, scoped to the stdio process (stdio)
  or the session (HTTP), as specified by the negotiated legacy protocol version."
- "Servers **MUST** implement `server/discover`."

Source for all four: https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning,
sections "Protocol Version Negotiation" and "Backward Compatibility with
Initialization-Based Versions", revision `2026-07-28`, retrieved 2026-10-03.

### 4.2 What gate G4 showed

Gate G4 is **PASS** (`docs/planning/gates/G4-result.md`, re-run 2026-09-26, Claude Code
`v2.1.283`, Codex `0.157.1`, MCP `2026-07-28`/`2025-11-25`; fixture
`docs/planning/gates/fixtures/g4-mcp-dual-era/transcript-2026-09-26.jsonl`). The facts
this binding relies on, each with its evidence:

1. One process (pid 19680) held a legacy stdio channel open for about 6.5 minutes while
   its HTTP surface served modern `tools/call` from Claude Code, before and after legacy
   traffic from Codex, with no degradation (criteria 1 and 4; fixture lines 14-15,
   24-25, 53-54).
2. The legacy path negotiated `2025-11-25`, declared
   `capabilities.experimental["claude/channel"]`, and delivered two channel
   notifications that Claude Code rendered (criterion 1; lines 14-15, 23, 51).
3. A stdio server answering `server/discover` as modern-only (`supportedVersions:
   ["2026-07-28"]`) and still declaring `experimental["claude/channel"]` was refused as a
   channel. Its pushes were not observed arriving (criterion 5; lines 7, 22, 57; operator
   UI observation 1).
4. Every modern-era request carried `_meta["io.modelcontextprotocol/protocolVersion"]:
   "2026-07-28"` (criterion 3; lines 5, 6, 10, 12, 16, 24, 53).
5. The modern `tools/call` result carried OAC provenance under
   `_meta["io.github.rossgraeber/oac-session-channels"]` (criterion 2; lines 25, 54).
6. Claude Code did not surface that result `_meta` to the model, although it was on the
   wire (G4 "UNVERIFIED items", new observation; UI observation 3). Whether this holds
   for every Claude Code tool-call path is UNVERIFIED.
7. Codex `0.157.1` reached the server through `codex mcp add` (HTTP) and through a stdio
   `config.toml` registration. With default settings it sent
   `initialize` with `protocolVersion: "2025-06-18"` and accepted the answer
   `"2025-11-25"` on every connection, so it stayed legacy (lines 26-47).
8. Behind the opt-in flag `--enable mcp_2026_07_28` (`stage: UnderDevelopment`,
   `default_enabled: false`), Codex `0.157.1` negotiated `2026-07-28` on its HTTP
   registrations, while its stdio registration in the same run stayed legacy (G4
   "Row-41 probe addendum", fixture `transcript-row41-2026-09-27.jsonl`, lines 1-18).

G4's server was a hand-written Node.js spike, not the `rmcp` SDK the reference
implementation will use. Whether an `rmcp`-based server registers as a legacy-era channel
against live Claude Code is UNVERIFIED and owned by issue #65 (backlog G4).

### 4.3 Requirements

**Scope.** A *channel-path server* is an OAC server process started over stdio for the
Claude Code channel path (§8.1). It is legacy-only by design ([MCPB-CLD-003]). Requirements [MCPB-ERA-004],
[MCPB-ERA-005], [MCPB-ERA-007] and [MCPB-ERA-009] apply to every OAC server except a channel-path server. Every other
requirement in this document applies to all OAC servers.

> **Reference implementation note:** the shim learns that it is a channel-path server from
> the command line its harness configuration starts it with, not from anything on the MCP
> wire.

[MCPB-ERA-001] An OAC server MUST accept a legacy-era `initialize`
request.

[MCPB-ERA-002] An OAC server MUST support the legacy revision
`2025-11-25`.

[MCPB-ERA-003] When an `initialize` request names a legacy
revision the OAC server does not support, the server MUST answer with `2025-11-25`.

Requirement MCPB-ERA-003 follows the legacy rule "Otherwise, the server **MUST** respond with another
protocol version it supports" (https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle,
"Version Negotiation", retrieved 2026-10-03). G4 fact 7 shows Codex `0.157.1` accepts
this answer to its `2025-06-18` request.

[MCPB-ERA-004] An OAC server MUST serve modern-era requests at
`2026-07-28`.

[MCPB-ERA-005] An OAC server MUST answer `server/discover` with a
`supportedVersions` list that contains both `2026-07-28` and `2025-11-25`.

G4 fact 1 shows a one-process server answering this way on HTTP (line 11). On stdio,
G4 also observed Claude Code probing `server/discover` before falling back to
`initialize` (lines 6, 8, 14). A stdio server that answered that probe as modern would
keep Claude Code on the modern era, and Claude Code would then not register it as a
channel (G4 fact 3). That is why a channel-path server is out of [MCPB-ERA-005]'s scope.

[MCPB-ERA-006] An OAC server MAY serve both eras concurrently in one process. A
server that serves them from separate processes still meets every other requirement in
§4, and both processes present the same tool surface (§5). G4 fact 1 shows the
one-process topology works, so the two-process fallback is not needed.

[MCPB-ERA-007] An OAC server MUST present the same tool surface
on both eras: the same tool names, `inputSchema`, result shape and `tools/list` listing (§5).

Requirement MCPB-ERA-007 covers the tool *surface* only. Whether a given call is served also depends on caller
attribution (§4.4), which today is satisfied only on a connection bound by a documented
pairing. In particular, if Codex's default client moves to `2026-07-28` before a Codex
binding signal exists, its outbound tool calls are refused (fail-closed, [MCPB-ATT-002]) until one
does. That stays tracked under `docs/planning/v0.1/11-risks.md` row 41 (RISK-G4).

[MCPB-ERA-008] An OAC server MUST declare the extension
identifier (§3) in `capabilities.extensions` of every `initialize` result it returns.

The `2025-11-25` schema defines no `extensions` member, but its `ServerCapabilities` is
"not a closed set: any server can define its own, additional capabilities". Source:
https://github.com/modelcontextprotocol/modelcontextprotocol/blob/c4c367f9f58296a7053f5c78a52fd02bfbb56a49/schema/2025-11-25/schema.ts,
`ServerCapabilities` doc comment, retrieved 2026-10-03. SEP-2133 shows `extensions` in an
`initialize` response at `protocolVersion: "2025-06-18"`. G4's server declared the
identifier this way in its legacy `initialize` results to Codex `0.157.1` (fixture lines
27, 32), and Codex went on to list and call tools (lines 28-39, 48-52). That a legacy
client ignores a capability member it does not know is shown for Codex in that run; for
other legacy clients it is UNVERIFIED. That includes Claude Code on the channel path:
G4's channel server did not declare the member in its legacy `initialize` result
(line 15), so [MCPB-ERA-008] against live Claude Code is the first test of it. That test is part of
#65. **Reversal condition:** if #65 shows that Claude Code refuses or ignores channel
registration when `extensions` is present in the `initialize` result, [MCPB-ERA-008] is scoped
to servers other than a channel-path server, and a channel-path server omits the member.

[MCPB-ERA-009] An OAC server MUST declare the extension
identifier (§3) in `capabilities.extensions` of every `server/discover` result it
returns.

`ServerCapabilities.extensions` is defined at `2026-07-28`: "Optional MCP extensions that
the server supports. Keys are extension identifiers ... and values are per-extension
settings objects." Source:
https://github.com/modelcontextprotocol/modelcontextprotocol/blob/271ecc9accafdd9b83a3c869fa67c22953b2af80/schema/2026-07-28/schema.ts,
`ServerCapabilities`, line 882, retrieved 2026-10-03. G4's server declared it in its
modern `server/discover` result (fixture line 11).

[MCPB-ERA-010] An OAC server SHOULD include
`_meta["io.modelcontextprotocol/serverInfo"]` in every modern-era result. This repeats the
MCP recommendation ("Servers **SHOULD** include the following `io.modelcontextprotocol/*`
field in every result's `_meta`", MCP basic page, "Per-response protocol fields",
retrieved 2026-10-03). A server deviates by omitting it, which costs only diagnostics.

[MCPB-ERA-011] An OAC server MUST reject with JSON-RPC error
`-32602` a request that lacks `_meta["io.modelcontextprotocol/protocolVersion"]` and does
not belong to a legacy-era connection (no `initialize` on that stdio process or HTTP
session).

Requirement MCPB-ERA-011 repeats an MCP `2026-07-28` requirement ("A request missing any required field is
malformed; the server **MUST** reject it with JSON-RPC error code `-32602`", MCP basic
page, "Per-request protocol fields", retrieved 2026-10-03) so that a fixture can hold the
OAC server to it. A request inside an established legacy-era connection carries no
`protocolVersion` in `_meta` and is legitimate, so it is out of scope.

### 4.4 Caller attribution

All four tools (§5) depend on the caller's identity: `send` and `reply` use it as the
envelope's `from`, `list_sessions` filters by it (C5 §11), and `whoami` returns it.

**Bound connections.** A connection is *bound* when a documented OAC pairing has tied it
to exactly one harness session. Today the only such pairing is the Claude channel-path
stdio shim's hook-to-shim pairing (C4 §3). C4 §4 (Codex) captures `thread.id` through the
daemon's app-server client but defines no attribution for outbound MCP tool calls, so no
Codex connection is bound on either era.

**Why the connection alone is not enough.**

- On the modern era, MCP `2026-07-28` says "a server must not treat connection or process
  identity as a proxy for conversation or session continuity", and "Clients **SHOULD NOT**
  use an individual task, thread, or conversation as the lifetime boundary for the stdio
  process". Source: MCP basic page, "Statelessness", revision `2026-07-28`, retrieved
  2026-10-03.
- On the legacy era, Codex `0.157.1` sends a thread id on every `tools/call`
  (`transcript-2026-09-26.jsonl` lines 48, 50), which suggests one Codex MCP client
  connection can carry calls from several threads. Whether it does is UNVERIFIED (§10).

[MCPB-ATT-001] On a connection that is not bound, an OAC server
MUST NOT attribute a tool call to an OAC session using only the identity of the
connection or process that carried it.

[MCPB-ATT-002] When an OAC server cannot attribute a tool call
on a connection that is not bound to exactly one OAC session, it MUST refuse the call with
the error taxonomy's authorization-failure error (§5.4).

**The refusal in [MCPB-ATT-002] is interim.** It holds until a documented per-request signal
exists that OAC can bind to a paired session. Codex does send a per-request candidate:
`_meta["x-codex-turn-metadata"]` carries `session_id`, `thread_id` and `turn_id`
(plus `codex_version`, `model` and other fields) on the legacy calls at
`transcript-2026-09-26.jsonl` lines 48 and 50 and on the modern call at
`transcript-row41-2026-09-27.jsonl` line 17. OAC does not rely on it, for two reasons:

1. It appears in no first-party Codex documentation this project cites, so depending on
   it would rely on an undocumented surface (`[ADR-001 Boundary]`: "MUST NOT depend on
   UI/terminal scraping or undocumented private RPCs for supported integrations";
   `oac-boundaries` boundary 4).
2. It is a client-asserted id with nothing binding it to a paired session. Under the
   "authenticated but untrusted" doctrine (`oac-security-work` §3) and C4's rule that a
   self-reported id is never the sole pairing key (C4 §3, §13), it cannot attribute a
   call by itself.

Whether a documented per-request session signal exists that OAC can bind to a paired
session is UNVERIFIED (§10). The refusal does not affect Claude Code's channel path, which
is bound (§8.1). It does refuse Codex's outbound tool calls on both eras until a Codex
pairing is designed and documented; that design is owned by #69 (backlog: Codex adapter
outbound tool surface and reply correlation).

*Dated note, 2026-10-03: operator decision on #46 (https://github.com/RossGraeber/OAC/issues/46#issuecomment-5973893128, also recorded on #69).* Until
a documented pairing ties an MCP tool call to exactly one Codex conversation (#69), Codex's
outbound tool calls are refused on both MCP eras, as [MCPB-ATT-002] states. Codex sessions can still
receive OAC messages; sending from Codex waits for #69. Codex's self-reported
`x-codex-turn-metadata` ids are not used for pairing.

> **Reference implementation note:** the v0.1 shim binds a Claude channel-path stdio
> connection to one harness session through the hook-to-shim pairing in C4 §3. No
> equivalent pairing exists yet for a Codex connection, which is why its tool calls are
> refused until one is designed.

## 5. Tool surface

The tool surface is the outbound half of OAC over MCP. Both harnesses send through the
same four OAC tools, and inbound stays provider-native (PLANNING-PROMPT.md §5 decision 9;
C6 §8). Argument and result semantics belong to `spec/session-channels.md` §8.2 (replies and
correlation), §8.1 (delivery states) and §6 (session identity and addressing), all
filled by E2 and E4. This section fixes only how they are carried.

### 5.1 Tools

| Tool name | Purpose (semantics in the neutral spec) | Source |
|---|---|---|
| `send` | Send a new message to an addressed session | C6 §8 |
| `reply` | Reply to a received message (`in_reply_to`) | C6 §8, §10 |
| `list_sessions` | List the sessions the caller is authorized to see | C6 §8; C5 §11 |
| `whoami` | Return the caller's own session id and device fingerprint, never a credential | C6 §8 |

All four names meet the MCP tool-name guidance. MCP allows 1-128 characters drawn from
ASCII letters, digits, underscore, hyphen and dot
(https://modelcontextprotocol.io/specification/2025-11-25/server/tools, "Tool Names",
retrieved 2026-10-03; the same at `2026-07-28`). The four names use a subset of those
characters: lowercase letters and underscore.

[MCPB-TOOL-001] An OAC server MUST list the four tools above in
`tools/list` under exactly those names.

[MCPB-TOOL-002] An OAC server MUST present the same tool names,
the same `inputSchema` and the same result shape on a legacy-era connection as on a
modern-era request.

Requirement MCPB-TOOL-002 is C6 §9's "the four tool schemas in §8 are era-invariant", restated as a testable
requirement.

[MCPB-TOOL-003] An OAC server MUST present the same tool names,
`inputSchema` and result shape to every harness.

### 5.2 Arguments

[MCPB-TOOL-004] Each tool's `inputSchema` MUST be a JSON Schema
object whose properties are the tool's arguments as `spec/session-channels.md` §4.4 and
§8.2 name them.

[MCPB-TOOL-005] A `tools/call` that fails its tool's `inputSchema`
MUST return a tool execution error (§5.4) carrying the error taxonomy's malformed-input
error.

### 5.3 Results

G4 fact 6 shows that a result's `_meta` may never reach the model. Anything the model
needs in order to act therefore travels in `content`.

[MCPB-TOOL-006] A tool result MUST carry, in a `text` content
block, every value the calling model needs to act on: for `send` and `reply` the
assigned message id and the initial delivery state, for `list_sessions` the session
list, and for `whoami` the caller's own session id and device fingerprint.

[MCPB-TOOL-007] A tool result SHOULD also carry the same values in
`structuredContent`. MCP recommends the pairing in the other direction ("a tool that
returns structured content SHOULD also return the serialized JSON in a TextContent
block", MCP `2025-11-25` tools page, "Structured Content", retrieved 2026-10-03). A server
deviates by sending text only, which loses machine-readable results for clients that
use them.

[MCPB-TOOL-008] A `send` or `reply` result MUST NOT report a
delivery state beyond `accepted-by-adapter`.

The tool call returns before the receiving harness has the message. The states after
`accepted-by-adapter` are decided in C5 §9 and are to be defined in
`spec/session-channels.md` §8.1 (E4). The name `accepted-by-adapter` is C5 §9's; it is
re-pointed when §8.1 lands.

[MCPB-TOOL-009] A `whoami` result MUST NOT contain a private key,
bearer token or any other value that authenticates its holder as the caller (C6 §8).

### 5.4 Errors

MCP separates protocol errors (JSON-RPC errors "for issues like: Unknown tools, Malformed
requests ..., Server errors") from tool execution errors ("Reported in tool results with
`isError: true`"). Source: https://modelcontextprotocol.io/specification/2025-11-25/server/tools,
"Error Handling", retrieved 2026-10-03. `CallToolResult.isError` is present in both
the `2025-11-25` and `2026-07-28` schemas (lines 1129 and 1837 of the schema files cited
in §4.3).

[MCPB-TOOL-010] An OAC error from the closed taxonomy in
`spec/session-channels.md` §8.3 (E4) MUST be returned as a tool
execution error with `isError: true`.

[MCPB-TOOL-011] A tool execution error MUST carry the OAC error
code, spelled exactly as the taxonomy spells it, in its `text` content.

**Error names are placeholders.** The closed taxonomy is `spec/session-channels.md` §8.3,
a stub E4 (#44) fills. This document therefore names errors by role only: the *authorization-failure
error* ([MCPB-ATT-002]), the *malformed-input error* ([MCPB-TOOL-005]) and the *unknown-destination error*
([MCPB-TOOL-011]). Each role is re-pointed to the taxonomy's exact code when E4 lands; until then no
fixture can assert a spelling. Once #44 lands, these roles map to the codes in
`spec/session-channels.md` §8.3.1 as proposed in PR #261: authorization failure →
`unauthorized`, malformed input → `invalid-request`, unknown destination →
`unknown-destination`.

[MCPB-TOOL-012] An OAC server MUST NOT return an OAC taxonomy
error as a JSON-RPC error object.

[MCPB-TOOL-013] An OAC server MUST NOT emit a JSON-RPC error code
in the range `-32020` to `-32099` that the MCP specification does not define.

Requirement MCPB-TOOL-013 repeats the MCP `2026-07-28` reservation of that range ("Implementations **MUST
NOT** emit any code from this sub-range that is not defined by this specification", MCP
basic page, "Error Codes", retrieved 2026-10-03).

## 6. `_meta` provenance

### 6.1 MCP's key-name rules, quoted

The rules are the same at both revisions. Source:
https://modelcontextprotocol.io/specification/2026-07-28/basic and
https://modelcontextprotocol.io/specification/2025-11-25/basic, section "General fields"
→ "`_meta`", retrieved 2026-10-03:

> **Prefix:** If specified, MUST be a series of labels separated by dots (`.`), followed
> by a slash (`/`). Labels MUST start with a letter and end with a letter or digit;
> interior characters can be letters, digits, or hyphens (`-`). Implementations SHOULD
> use reverse DNS notation (e.g., `com.example/` rather than `example.com/`). Any prefix
> where the second label is `modelcontextprotocol` or `mcp` is **reserved** for MCP use.
>
> **Name:** Unless empty, MUST begin and end with an alphanumeric character
> (`[a-z0-9A-Z]`). MAY contain hyphens (`-`), underscores (`_`), dots (`.`), and
> alphanumerics in between.

At `2026-07-28` the page adds: "third-party extensions use their own vendor prefix. In
both cases the keys are specified in the extension's documentation." This section is that
documentation for OAC.

### 6.2 The OAC key

[MCPB-META-001] OAC provenance in `_meta` MUST be carried under
the single key `io.github.rossgraeber/oac-session-channels`.

The key is the extension identifier itself, checked against §6.1 in §3.2. G4 fact 5 shows
this key on the wire.

[MCPB-META-002] The value under that key MUST be a JSON object.

[MCPB-META-003] Every `_meta` key an OAC server writes MUST either
use the prefix `io.github.rossgraeber/` or be a key the MCP specification itself defines.

[MCPB-META-004] An OAC server MUST NOT write a `_meta` key whose
prefix has `modelcontextprotocol` or `mcp` as its second label, except a key the MCP
specification defines for the role the server is playing.

**6.2.5 — members.** The members of the provenance object are copies of envelope and
receipt fields. They are named in `spec/session-channels.md` §4.2 (envelope members) and
§8.1 (delivery states, E4). This binding defines none of them.

> **Reference implementation note:** the expected members are the message id, the
> conversation and correlation ids, the delivery state, and the spec revision. G4's spike
> carried only debugging members (`served_by_pid`, `surface`), which are not part of this
> binding.

### 6.3 What `_meta` provenance is not

[MCPB-META-005] A receiver MUST NOT use a value read from `_meta`
as evidence of a message's authenticity or sender.

Authenticity comes from the envelope signature (`spec/security.md`, planned, E5; C5 §2-§6).
`_meta` is unsigned client- or server-supplied data. SEP-2133 says "Clients and servers
SHOULD treat any new fields or data introduced as part of an extension as untrusted"
(https://modelcontextprotocol.io/seps/2133-extensions, "Security Implications", retrieved
2026-10-03).

[MCPB-META-006] An OAC server MUST NOT place a value only in
`_meta` when the calling model needs that value (see [MCPB-TOOL-006]).

**6.3.3 — not the channel `meta`.** The Claude Code channel notification has its own
`meta` map (no underscore). Its keys become attributes on the `<channel>` tag, must be
identifier-safe (letters, digits, underscore), and are dropped silently otherwise
(`oac-claude-channels` §2). That map follows C6 §2's five `oac_*` keys, not §6.1's prefix
rules. A prefixed key such as `io.github.rossgraeber/oac-session-channels` contains `.`,
`/` and `-`, so Claude Code would drop it from a channel `meta` map.

[MCPB-META-007] An OAC server MUST NOT put a `_meta`-style
prefixed key into a Claude Code channel `meta` map.

## 7. Unsupported extension

MCP: "If one party supports an extension but the other does not, the supporting party
**MUST** either revert to core protocol behavior or reject the request with an
appropriate error. Extensions **SHOULD** document their expected fallback behavior."
Source: MCP versioning page, "Extension Negotiation", revision `2026-07-28`, retrieved
2026-10-03. This section is OAC's fallback documentation.

[MCPB-FBK-001] An OAC server MUST serve the tool surface (§5) to a
client that does not declare the OAC extension identifier.

Neither Claude Code nor Codex declares the identifier in the G4 fixtures: every
`extensions` map carrying it there is server-to-client (`transcript-2026-09-26.jsonl`
lines 7, 11, 27, 32; `transcript-row41-2026-09-27.jsonl` lines 2, 4). Requiring it would
cut off both harnesses. The OAC tools are ordinary MCP tools and need no client-side
extension support.

[MCPB-FBK-002] A client that does understand the identifier SHOULD declare it in
its own `capabilities.extensions`. A client deviates by not declaring it; the server
still serves it under [MCPB-FBK-001].

## 8. Provider profiles

These subsections are provider-facing detail. They cite the provider skills and do not
restate them. Each provider surface carries one label (`oac-evidence` §4).

### 8.1 Claude Code — channel path (research preview)

- **Surface label:** research preview. Shim boundary: the Claude adapter's channel
  server. Pin: Claude Code floating, minimum `v2.1.282`, last tested `v2.1.285`
  (`docs/planning/STATUS.md` "Pins").
- **Inbound:** the provider-native `notifications/claude/channel` notification, with
  `content` and `meta` (`oac-claude-channels` §1). Claude Code sends no acknowledgement,
  so the strongest delivery state an OAC server can report for it is `handed-to-harness`
  (C5 §9; `spec/session-channels.md` §8.1, E4).
- **Era:** "A channel server that negotiates MCP protocol `2026-07-28` cannot deliver
  channel messages and is not registered as a channel" (`oac-claude-channels` §4; G4
  fact 3).
- **Outbound:** the four tools of §5.

[MCPB-CLD-001] An OAC server MUST declare
`capabilities.experimental["claude/channel"]` only in an `initialize` result that
negotiates a legacy revision.

[MCPB-CLD-002] An OAC server MUST NOT send
`notifications/claude/channel` on a connection that has not negotiated a legacy revision.

[MCPB-CLD-003] A stdio OAC server started for the Claude channel
path MUST answer a `server/discover` probe with a JSON-RPC error that is not a recognized
modern error.

Requirement MCPB-CLD-003 is how a stdio channel server stays on the legacy era. Under MCP `2026-07-28`, a
dual-era stdio client "probe[s] with `server/discover` and fall[s] back on any error that
is not a recognized modern error" (MCP versioning page, "Backward Compatibility",
retrieved 2026-10-03). G4's channel server did exactly this, with `-32601 Method not
found` (lines 6, 8), and Claude Code fell back to `initialize` (line 14). It is the one
exception to [MCPB-ERA-005]: a Claude-channel stdio process is legacy-only, by design.
`MCP_PROTOCOL_NEGOTIATION=legacy` can force the same result from the client side
(`oac-claude-channels` §4); [MCPB-CLD-003] does not depend on the operator setting it.

The 2026-07-28 schema still defines `experimental` on both `ClientCapabilities` and
`ServerCapabilities` (schema at commit `271ecc9`, lines 720 and 797, cited in §4.3), so
requirement MCPB-CLD-001 restricts where the capability is declared, not whether the member exists.

> **Reference implementation note:** the Claude adapter's shim is a stdio process
> spawned by Claude Code (C2 §1). It therefore runs as the legacy-only exception in [MCPB-CLD-003],
> and it serves the §5 tools over the same legacy connection.

### 8.2 Codex — tool path (MCP: supported; app-server inbound: experimental)

- **Surface labels:** `codex mcp add` external MCP server registration, supported
  (PLANNING-PROMPT.md §3.2; C6 §9). App-server live inject, experimental (per-method
  gating) (`oac-codex-appserver`). Pin: Codex floating, minimum `0.154.0`, last tested
  `0.159.3` (`docs/planning/STATUS.md` "Pins").
- **Inbound:** not MCP. The Codex adapter uses the app-server methods named in §2.3.
  `codex mcp-server` is deleted and is not a path (C6 §9).
- **Outbound:** Codex calls the four tools of §5 on an OAC server registered with
  `codex mcp add` (C2 §6). No Codex connection is bound today, so these calls are refused
  under the interim rule in [MCPB-ATT-002] until the Codex pairing (#69) is designed and documented.
- **Era:** legacy by default, modern only behind `mcp_2026_07_28` and only on HTTP
  registrations in the one run observed (G4 facts 7 and 8).

[MCPB-CDX-001] An OAC server MUST NOT send any notification to a
Codex MCP client as a way of delivering an OAC message.

Requirement MCPB-CDX-001 holds on both eras: Codex has no MCP surface that turns a notification into session
input, so such a notification could only mislead ([MCPB-DLV-001]). G4 recorded one such push with
no consumer (line 56).

## 9. Resolution of conflict-register row C5

Row C5 reads "Claude needs legacy MCP; Codex tool path may negotiate current MCP"
(`docs/planning/v0.1/03-decisions-and-amendments.md` §3;
`docs/planning/ADR-001-AMENDMENTS.md` "Conflict register"). It was `ASSIGNED` to this
task by #228 (2026-10-02). It is resolved here as follows.

1. **The conflict was a design question: can one OAC server serve both needs?** It
   can. §4 requires every OAC server other than a channel-path server to be dual-era
   ([MCPB-ERA-001] to [MCPB-ERA-005]), and the MCP specification permits a dual-era server to serve both eras
   in one process (§4.1). G4 showed that topology working with real clients, alongside a
   legacy channel in the same process (§4.2 facts 1-4).
2. **The Claude half is fixed by requirement, not by hope.** The channel capability is
   declared only on legacy connections ([MCPB-CLD-001]), channel notifications are sent only
   there ([MCPB-CLD-002]), and a Claude-channel stdio process refuses the modern probe so that the
   client falls back ([MCPB-CLD-003]).
3. **The Codex half no longer needs to be true.** "Codex tool path *may* negotiate current
   MCP" was an open element because OAC's design seemed to depend on it. Under this
   binding the tool *contract* does not: the tool surface is identical in both eras
   ([MCPB-ERA-007], [MCPB-TOOL-002]). The honest consequence is about calls, not the contract: whether a
   Codex call is served depends on caller attribution (§4.4), and no Codex connection is
   bound today. If Codex's default client moves to `2026-07-28` before a binding signal
   exists, Codex's outbound calls are refused (fail-closed) on that era as on the legacy
   one. That stays tracked under RISK-G4 row 41, and the Codex pairing design under #69.
   *Dated note, 2026-10-03: the operator confirmed this refusal on #46 (https://github.com/RossGraeber/OAC/issues/46#issuecomment-5973893128); see §4.4.*
4. **What stays open is verification, not conflict.** Two items remain, each with an
   owner, and neither can reopen the design question in point 1:
   - an `rmcp`-based server registering as a legacy-era channel against live Claude Code
     (UNVERIFIED; issue #65, backlog G4);
   - Codex's default client era, a provider behavior OAC cannot change
     (`docs/planning/v0.1/11-risks.md` row 41, RISK-G4).

   Two new open items come out of this resolution, both UNVERIFIED (§4.4, §10): whether a
   documented per-request session signal exists that OAC can bind to a paired session,
   and whether one Codex legacy-era connection carries calls from several threads (#69).

C5's status therefore moves from `ASSIGNED` to closed when this document lands, per the
register's own legend ("the register entry closes when that task lands"). The register
rows record the closure with a dated note in the same change.

## 10. Evidence and UNVERIFIED items

| Claim | Status | Where recorded |
|---|---|---|
| MCP `_meta` key-name rules, including the reserved second labels `modelcontextprotocol`/`mcp`, at both revisions | verified, retrieved 2026-10-03 (§6.1) | this document; `docs/planning/STATUS.md` 2026-10-03 #46 entry (drift against B2 D2 noted there; follow-up #257) |
| Extension identifiers follow the `_meta` rules with a mandatory prefix | verified, retrieved 2026-10-03 (§3.2) | this document |
| `experimental` exists in `ClientCapabilities`/`ServerCapabilities` at `2026-07-28` | verified against the schema at commit `271ecc9`, retrieved 2026-10-03 (§8.1) | closes the STATUS ledger item; `11-risks.md` RISK-MCP-EXPERIMENTAL dated note |
| One process can serve both eras | verified by G4 PASS (§4.2) | `gates/G4-result.md` |
| Claude Code does not surface tool-result `_meta` to the model on every path | UNVERIFIED (observed once, G4) | already in `docs/planning/STATUS.md` and `11-risks.md` row 44 |
| An `rmcp`-based server registers as a legacy-era channel | UNVERIFIED | already in `docs/planning/STATUS.md`; #65 |
| Codex default client negotiates `2026-07-28` | UNVERIFIED (never observed) | already in `docs/planning/STATUS.md`; `11-risks.md` row 41 |
| A documented per-request session signal exists that OAC can bind to a paired session (Codex's `x-codex-turn-metadata` carries `session_id`/`thread_id`/`turn_id` but is undocumented and client-asserted) | UNVERIFIED (new, §4.4) | added to `docs/planning/STATUS.md` and `11-risks.md` RISK-G4 in this change |
| One Codex legacy-era MCP connection carries calls from several threads (a thread id is sent per call) | UNVERIFIED (new, §4.4); owner #69 | added to `docs/planning/STATUS.md` and `11-risks.md` RISK-G4 in this change |
| A legacy client other than Codex `0.157.1` ignores an `extensions` member in an `initialize` result | UNVERIFIED (new, [MCPB-ERA-008]) | added to `docs/planning/STATUS.md` in this change |

## 11. Boundary self-check (`oac-boundaries`)

- **Delivery.** No section makes MCP the delivery mechanism. §2 forbids the claim, and
  §8 names each provider's native path.
- **Polling.** [MCPB-DLV-002] forbids an MCP polling surface while active inbound is claimed.
- **Credentials.** `whoami` never returns a credential ([MCPB-TOOL-009]). Nothing here reads or
  reuses a provider credential.
- **Private RPCs.** Every provider surface named is documented and labelled (§8). No
  scraping, rollout file or deleted `codex mcp-server` path is used.
- **Impersonation.** The Claude channel path is the documented development-channel
  loading path (`oac-claude-channels` §6). No allowlisted plugin is impersonated.
- **Neutral spec.** This file is the task-scoped exemption. It contains no transport
  vocabulary, so boundary-lint check 1 needs no exemption for it. Only the
  provider-method check (check 2) and the spec-specific zero-hits group exempt it, by
  exact path (`oac-boundaries` `references/mechanical-checks.md`;
  `oac-spec-authoring` `references/neutral-vocabulary-check.md`).

## 12. Requirement index and conformance fixtures

### 12.1 Ids

This document registers the requirement-id prefix `MCPB` ("MCP binding") under
`spec/session-channels.md` §3.2, which lets a binding document register its own `<DOC>`.
`MCPB` names this document rather than the protocol it binds, so it cannot be read as an
MCP-defined code, and it does not collide with `SC` or `SEC`. Its areas are: `DLV` (§2),
`EXT` (§3), `ERA` (§4.3), `ATT` (§4.4), `TOOL` (§5), `META` (§6), `FBK` (§7), `CLD`
(§8.1) and `CDX` (§8.2). The stability rules of `spec/session-channels.md` §3.2 apply unchanged.

### 12.2 Fixture profile

Fixtures follow the shape of `spec/session-channels.md` §3.3 under their own format
string, `oac-mcpb-fixture/1`. The core format `oac-conformance-fixture/1` stays unchanged;
a runner dispatches on `fixture_format`. The binding format differs from the core one
only as follows, because the core `input` kinds and `result` values are envelope-stage
ones:

- `fixture_format` is `oac-mcpb-fixture/1`.
- `stage` is `mcp-binding`.
- `context` holds `era` and `server_role` (`channel-path` or `general`, §4.3). `era` is
  `legacy` (the request belongs to a legacy-era connection), `modern` (a modern-era
  request) or `unestablished` (neither: no `initialize` on that stdio process or HTTP
  session and no `protocolVersion`, §1). Where a requirement needs them, `context` also
  holds `bound` (whether the connection is bound by a documented pairing, §4.4; every
  fixture that shows a served `tools/call` sets it to `true`), `legacy_initialized` (whether the
  stdio process or HTTP session has completed `initialize`) and
  `supported_legacy_revisions`.
- `input` is `mcp_exchange`: an object with an optional `request` (the client's
  JSON-RPC message) and a required `server_message` (the server's response or
  notification).
- `expected.result` is `conformant` or `nonconformant`: whether `server_message` meets
  the requirement, given `request` and `context`.

Fixtures live in `tests/protocol/mcpb-<area>/`, named as `spec/session-channels.md` §3.3 says. A requirement that a
single exchange cannot decide (a whole session, a comparison across eras, or a value a
later task defines) stays `TODO(fixture)`, with the planned input and expected outcome.

### 12.3 Index

| Id | Keyword | Fixtures |
|---|---|---|
| MCPB-DLV-001 | MUST NOT | TODO(fixture): `server/discover` and `initialize` results, `tools/list`, `resources/list`, `prompts/list` → every capability key is one the MCP revision in use defines (for example `tools`, `logging`) or `extensions` (holding only the §3 identifier) or, on a legacy channel-path connection only, `experimental["claude/channel"]`; no tool beyond §5.1's four; no resource or prompt is listed |
| MCPB-DLV-002 | MUST NOT | TODO(fixture): `resources/list`, `prompts/list`, `subscriptions/listen` while active inbound is declared → no inbox-shaped resource, prompt or subscription offered |
| MCPB-EXT-001 | MUST | `tests/protocol/mcpb-ext/MCPB-EXT-001.p01-initialize-exact-identifier.json`, `tests/protocol/mcpb-ext/MCPB-EXT-001.n01-identifier-wrong-case.json` |
| MCPB-EXT-002 | MUST NOT | TODO(fixture), needs the version member of `spec/session-channels.md` §6.5 (E2): a settings object declaring version `1.0` under the identifier (negative) → nonconformant |
| MCPB-EXT-003 | MUST | `tests/protocol/mcpb-ext/MCPB-EXT-003.n01-settings-not-object.json` |
| MCPB-EXT-004 | MUST | TODO(fixture): client capabilities with an unknown settings member → request served normally |
| MCPB-ERA-001 | MUST | TODO(fixture): legacy `initialize` → result returned |
| MCPB-ERA-002 | MUST | TODO(fixture): `initialize` with `protocolVersion: "2025-11-25"` → answered `2025-11-25` |
| MCPB-ERA-003 | MUST | `tests/protocol/mcpb-era/MCPB-ERA-003.p01-older-legacy-answered-2025-11-25.json`, `tests/protocol/mcpb-era/MCPB-ERA-003.n01-echoes-unsupported-revision.json` |
| MCPB-ERA-004 | MUST | TODO(fixture): modern `tools/list` with `protocolVersion` `2026-07-28` → served |
| MCPB-ERA-005 | MUST | `tests/protocol/mcpb-era/MCPB-ERA-005.p01-discover-lists-both-eras.json`, `tests/protocol/mcpb-era/MCPB-ERA-005.n01-discover-modern-only.json` |
| MCPB-ERA-006 | MAY | none (not a `MUST`) |
| MCPB-ERA-007 | MUST | TODO(fixture): `tools/list` on a legacy-era connection and as a modern-era request → identical names, `inputSchema` and declared result shape |
| MCPB-ERA-008 | MUST | `tests/protocol/mcpb-era/MCPB-ERA-008.p01-initialize-declares-extension.json`, `tests/protocol/mcpb-era/MCPB-ERA-008.n01-initialize-no-extensions.json` |
| MCPB-ERA-009 | MUST | `tests/protocol/mcpb-era/MCPB-ERA-009.p01-discover-declares-extension.json`, `tests/protocol/mcpb-era/MCPB-ERA-009.n01-discover-no-extensions.json` |
| MCPB-ERA-010 | SHOULD | none (not a `MUST`) |
| MCPB-ERA-011 | MUST | `tests/protocol/mcpb-era/MCPB-ERA-011.p01-uninitialized-missing-version-rejected.json`, `tests/protocol/mcpb-era/MCPB-ERA-011.p02-legacy-session-request-served.json`, `tests/protocol/mcpb-era/MCPB-ERA-011.n01-uninitialized-missing-version-served.json` |
| MCPB-ATT-001 | MUST NOT | TODO(fixture): two tool calls on one unbound connection carrying different client-asserted session ids (negative) → neither attributed by connection alone; both refused per MCPB-ATT-002 |
| MCPB-ATT-002 | MUST | TODO(fixture): `send`, `list_sessions` and `whoami` on an unbound connection, legacy and modern (negative) → `isError: true` result carrying the authorization-failure error for each (code pending E4) |
| MCPB-TOOL-001 | MUST | `tests/protocol/mcpb-tool/MCPB-TOOL-001.p01-four-tools-listed.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-001.n01-whoami-missing.json` |
| MCPB-TOOL-002 | MUST | TODO(fixture): `tools/list` on each era → identical names, schemas and result shapes |
| MCPB-TOOL-003 | MUST | TODO(fixture): `tools/list` as two different clients → identical |
| MCPB-TOOL-004 | MUST | TODO(fixture): each `inputSchema` → valid JSON Schema object; property names match the neutral spec |
| MCPB-TOOL-005 | MUST | TODO(fixture): `send` missing a required argument, on a bound connection (negative) → `isError: true`, the malformed-input error (code pending E4) |
| MCPB-TOOL-006 | MUST | TODO(fixture): each tool's result → required values present in a `text` block |
| MCPB-TOOL-007 | SHOULD | none (not a `MUST`) |
| MCPB-TOOL-008 | MUST NOT | TODO(fixture): `send` result → state is `accepted-by-adapter` or an error |
| MCPB-TOOL-009 | MUST NOT | TODO(fixture): `whoami` result → no key material or token |
| MCPB-TOOL-010 | MUST | TODO(fixture): `send` to an unknown session, on a bound connection (negative) → `isError: true` result, not a JSON-RPC error |
| MCPB-TOOL-011 | MUST | TODO(fixture): same → `text` contains the unknown-destination error's exact code (pending E4) |
| MCPB-TOOL-012 | MUST NOT | TODO(fixture): every OAC error case → never a JSON-RPC error object |
| MCPB-TOOL-013 | MUST NOT | `tests/protocol/mcpb-tool/MCPB-TOOL-013.n01-undefined-reserved-code.json` |
| MCPB-META-001 | MUST | `tests/protocol/mcpb-meta/MCPB-META-001.p01-provenance-under-identifier.json`, `tests/protocol/mcpb-meta/MCPB-META-001.n01-provenance-under-other-key.json` |
| MCPB-META-002 | MUST | `tests/protocol/mcpb-meta/MCPB-META-002.n01-provenance-not-object.json` |
| MCPB-META-003 | MUST | `tests/protocol/mcpb-meta/MCPB-META-003.p01-mcp-defined-key-allowed.json`, `tests/protocol/mcpb-meta/MCPB-META-003.n01-unprefixed-key.json` |
| MCPB-META-004 | MUST NOT | `tests/protocol/mcpb-meta/MCPB-META-004.n01-invented-reserved-key.json` |
| MCPB-META-005 | MUST NOT | TODO(fixture): envelope whose `_meta` names a sender different from the signed `from` (negative) → sender taken from the verified envelope only |
| MCPB-META-006 | MUST NOT | TODO(fixture): each tool's result → no model-needed value present only in `_meta` |
| MCPB-META-007 | MUST NOT | `tests/protocol/mcpb-meta/MCPB-META-007.p01-identifier-safe-meta-keys.json`, `tests/protocol/mcpb-meta/MCPB-META-007.n01-prefixed-key-in-channel-meta.json` |
| MCPB-FBK-001 | MUST | TODO(fixture): `tools/list` from a client declaring no extensions; `tools/call` from such a client on a bound connection → both served |
| MCPB-FBK-002 | SHOULD | none (not a `MUST`) |
| MCPB-CLD-001 | MUST | `tests/protocol/mcpb-cld/MCPB-CLD-001.p01-channel-capability-on-legacy.json`, `tests/protocol/mcpb-cld/MCPB-CLD-001.n01-channel-capability-on-modern.json` |
| MCPB-CLD-002 | MUST NOT | TODO(fixture): modern-era traffic on a channel server (negative) → no `notifications/claude/channel` sent |
| MCPB-CLD-003 | MUST | TODO(fixture): stdio `server/discover` on a channel server → JSON-RPC error that is not a recognized modern error |
| MCPB-CDX-001 | MUST NOT | TODO(fixture): a delivery to a Codex session → no MCP notification emitted toward the Codex client |

G4's committed transcripts are the first source of inputs for the `ERA` and `CLD`
fixtures; several fixtures above reproduce a G4 line's shape. They
are gate evidence, not conformance fixtures, so each fixture is re-captured in the E8
format rather than copied.
