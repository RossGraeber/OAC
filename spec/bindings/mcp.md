# OAC Session Channels — MCP binding

- **Document:** the MCP binding for OAC Session Channels (backlog task E6, issue #46).
- **Status:** Stage 2 draft, normative once Gate S2 freezes the spec surface
  (`docs/planning/v0.1/10-stages.md` §6). Every `MUST`/`MUST NOT` below is marked
  `TODO(fixture)` until its conformance fixture lands (§12).
- **Binding revision:** `0.1-draft`.
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

**Cross-dependency.** `spec/session-channels.md` is being written in parallel (issue #41,
E1, with the skeleton for E2-E4). This document cites its sections by their planned
names, in quotation marks, for example `spec/session-channels.md` "Session identity,
addressing, and capability/extension negotiation". Those names follow `oac-spec-authoring` §4. When #41 lands, each
such reference is re-pointed to the real heading. Error names are likewise placeholders
by role, pending the closed taxonomy from E4 (#44); see §5.4.

## 1. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD
NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be
interpreted as described in BCP 14 (RFC 2119, RFC 8174) when, and only when, they
appear in all capitals, as shown here.

- One requirement per sentence (`oac-spec-authoring` §1).
- Every `MUST`/`MUST NOT` carries `TODO(fixture)` and a fixture hook `H<n>` (§12). A hook
  is a placeholder for the fixture, not a requirement id. Requirement ids come from the
  scheme task E1 fixes (`oac-spec-authoring` `references/conformance-fixtures.md`);
  until then no fixture cites an id.
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

**2.1 — `TODO(fixture)`, hook H1.** An OAC server MUST NOT advertise, document or report
an MCP message as the mechanism that turns an OAC message into input for a live session.

**2.2 — `TODO(fixture)`, hook H2.** An OAC server MUST NOT offer an MCP resource, prompt
or subscription that a session is expected to read repeatedly in order to receive OAC
messages, while the session's capabilities claim active inbound delivery.

2.2 is the MCP form of the no-polling rule in `spec/session-channels.md` "Active-delivery
semantics and the no-polling rule" (`oac-boundaries` boundary 14).

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

**3.3 — `TODO(fixture)`, hook H3.** An OAC server MUST use exactly the string
`io.github.rossgraeber/oac-session-channels` wherever this document requires the
extension identifier.

**3.4 — breaking changes.** "Breaking changes MUST use a new identifier" (SEP-2133,
above). What counts as breaking for OAC is defined once, in `spec/session-channels.md`
"Versioning and unsupported-capability behaviour" (planned; E1 acceptance box 5). A
breaking change moves to `io.github.rossgraeber/oac-session-channels-v2` or later
(C3 §8).

**3.5 — `TODO(fixture)`, hook H4.** An OAC server MUST NOT advertise the identifier above
for a spec revision that `spec/session-channels.md` classifies as breaking against the
revision the identifier was frozen with.

**3.6 — the settings object.** MCP maps each extension identifier to "per-extension
settings objects", and "an empty object indicates support with no additional settings"
(MCP versioning page, above).

- **3.6.1 — `TODO(fixture)`, hook H5.** The value an OAC server places under the
  identifier MUST be a JSON object.
- **3.6.2 — `TODO(fixture)`, hook H6.** A receiver MUST ignore a settings member it does
  not recognize.
- The members of the settings object (for example the spec revision and the OAC
  capability set) are defined by `spec/session-channels.md` "Session identity,
  addressing, and capability/extension negotiation" (planned, task E2). This binding defines none. Until E2 lands, an OAC
  server that has nothing to declare sends `{}`.

3.6.2 is the MCP form of the forward-compatibility rule in `spec/session-channels.md`
"Versioning and unsupported-capability behaviour" (planned).

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
Claude Code channel path (§8.1). It is legacy-only by design (8.1.3). Requirements 4.3.4,
4.3.5, 4.3.7 and 4.3.9 apply to every OAC server except a channel-path server. Every other
requirement in this document applies to all OAC servers.

> **Reference implementation note:** the shim learns that it is a channel-path server from
> the command line its harness configuration starts it with, not from anything on the MCP
> wire.

**4.3.1 — `TODO(fixture)`, hook H7.** An OAC server MUST accept a legacy-era `initialize`
request.

**4.3.2 — `TODO(fixture)`, hook H8.** An OAC server MUST support the legacy revision
`2025-11-25`.

**4.3.3 — `TODO(fixture)`, hook H9.** When an `initialize` request names a legacy
revision the OAC server does not support, the server MUST answer with `2025-11-25`.

4.3.3 follows the legacy rule "Otherwise, the server **MUST** respond with another
protocol version it supports" (https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle,
"Version Negotiation", retrieved 2026-10-03). G4 fact 7 shows Codex `0.157.1` accepts
this answer to its `2025-06-18` request.

**4.3.4 — `TODO(fixture)`, hook H10.** An OAC server MUST serve modern-era requests at
`2026-07-28`.

**4.3.5 — `TODO(fixture)`, hook H11.** An OAC server MUST answer `server/discover` with a
`supportedVersions` list that contains both `2026-07-28` and `2025-11-25`.

G4 fact 1 shows a one-process server answering this way on HTTP (line 11). On stdio,
G4 also observed Claude Code probing `server/discover` before falling back to
`initialize` (lines 6, 8, 14). A stdio server that answered that probe as modern would
keep Claude Code on the modern era, and Claude Code would then not register it as a
channel (G4 fact 3). That is why a channel-path server is out of 4.3.5's scope.

**4.3.6 — `MAY`.** An OAC server MAY serve both eras concurrently in one process. A
server that serves them from separate processes still meets every other requirement in
§4, and both processes present the same tool surface (§5). G4 fact 1 shows the
one-process topology works, so the two-process fallback is not needed.

**4.3.7 — `TODO(fixture)`, hook H12.** An OAC server MUST present the same tool surface
on both eras: the same tool names, `inputSchema`, result shape and `tools/list` listing (§5).

4.3.7 covers the tool *surface* only. Whether a given call is served also depends on caller
attribution (§4.4), which today is satisfied only on a connection bound by a documented
pairing. In particular, if Codex's default client moves to `2026-07-28` before a Codex
binding signal exists, its outbound tool calls are refused (fail-closed, 4.4.2) until one
does. That stays tracked under `docs/planning/v0.1/11-risks.md` row 41 (RISK-G4).

**4.3.8 — `TODO(fixture)`, hook H13.** An OAC server MUST declare the extension
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
(line 15), so H13 against live Claude Code is the first test of it. That test is part of
#65. **Reversal condition:** if #65 shows that Claude Code refuses or ignores channel
registration when `extensions` is present in the `initialize` result, 4.3.8 is scoped
to servers other than a channel-path server, and a channel-path server omits the member.

**4.3.9 — `TODO(fixture)`, hook H14.** An OAC server MUST declare the extension
identifier (§3) in `capabilities.extensions` of every `server/discover` result it
returns.

`ServerCapabilities.extensions` is defined at `2026-07-28`: "Optional MCP extensions that
the server supports. Keys are extension identifiers ... and values are per-extension
settings objects." Source:
https://github.com/modelcontextprotocol/modelcontextprotocol/blob/271ecc9accafdd9b83a3c869fa67c22953b2af80/schema/2026-07-28/schema.ts,
`ServerCapabilities`, line 882, retrieved 2026-10-03. G4's server declared it in its
modern `server/discover` result (fixture line 11).

**4.3.10 — `SHOULD`.** An OAC server SHOULD include
`_meta["io.modelcontextprotocol/serverInfo"]` in every modern-era result. This repeats the
MCP recommendation ("Servers **SHOULD** include the following `io.modelcontextprotocol/*`
field in every result's `_meta`", MCP basic page, "Per-response protocol fields",
retrieved 2026-10-03). A server deviates by omitting it, which costs only diagnostics.

**4.3.11 — `TODO(fixture)`, hook H15.** An OAC server MUST reject with JSON-RPC error
`-32602` a request that lacks `_meta["io.modelcontextprotocol/protocolVersion"]` and does
not belong to a legacy-era connection (no `initialize` on that stdio process or HTTP
session).

4.3.11 repeats an MCP `2026-07-28` requirement ("A request missing any required field is
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

**4.4.1 — `TODO(fixture)`, hook H16.** On a connection that is not bound, an OAC server
MUST NOT attribute a tool call to an OAC session using only the identity of the
connection or process that carried it.

**4.4.2 — `TODO(fixture)`, hook H17.** When an OAC server cannot attribute a tool call
on a connection that is not bound to exactly one OAC session, it MUST refuse the call with
the error taxonomy's authorization-failure error (§5.4).

**The refusal in 4.4.2 is interim.** It holds until a documented per-request signal
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

> **Reference implementation note:** the v0.1 shim binds a Claude channel-path stdio
> connection to one harness session through the hook-to-shim pairing in C4 §3. No
> equivalent pairing exists yet for a Codex connection, which is why its tool calls are
> refused until one is designed.

## 5. Tool surface

The tool surface is the outbound half of OAC over MCP. Both harnesses send through the
same four OAC tools, and inbound stays provider-native (PLANNING-PROMPT.md §5 decision 9;
C6 §8). Argument and result semantics belong to `spec/session-channels.md` "Replies and
correlation", "Delivery states" and "Session identity, addressing, and capability/extension
negotiation" (planned). This section fixes only how they are carried.

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

**5.1.1 — `TODO(fixture)`, hook H18.** An OAC server MUST list the four tools above in
`tools/list` under exactly those names.

**5.1.2 — `TODO(fixture)`, hook H19.** An OAC server MUST present the same tool names,
the same `inputSchema` and the same result shape on a legacy-era connection as on a
modern-era request.

5.1.2 is C6 §9's "the four tool schemas in §8 are era-invariant", restated as a testable
requirement.

**5.1.3 — `TODO(fixture)`, hook H20.** An OAC server MUST present the same tool names,
`inputSchema` and result shape to every harness.

### 5.2 Arguments

**5.2.1 — `TODO(fixture)`, hook H21.** Each tool's `inputSchema` MUST be a JSON Schema
object whose properties are the tool's arguments as `spec/session-channels.md` names them.

**5.2.2 — `TODO(fixture)`, hook H22.** A `tools/call` that fails its tool's `inputSchema`
MUST return a tool execution error (§5.4) carrying the error taxonomy's malformed-input
error.

### 5.3 Results

G4 fact 6 shows that a result's `_meta` may never reach the model. Anything the model
needs in order to act therefore travels in `content`.

**5.3.1 — `TODO(fixture)`, hook H23.** A tool result MUST carry, in a `text` content
block, every value the calling model needs to act on: for `send` and `reply` the
assigned message id and the initial delivery state, for `list_sessions` the session
list, and for `whoami` the caller's own session id and device fingerprint.

**5.3.2 — `SHOULD`.** A tool result SHOULD also carry the same values in
`structuredContent`. MCP recommends the pairing in the other direction ("a tool that
returns structured content SHOULD also return the serialized JSON in a TextContent
block", MCP `2025-11-25` tools page, "Structured Content", retrieved 2026-10-03). A server
deviates by sending text only, which loses machine-readable results for clients that
use them.

**5.3.3 — `TODO(fixture)`, hook H24.** A `send` or `reply` result MUST NOT report a
delivery state beyond `accepted-by-adapter`.

The tool call returns before the receiving harness has the message. The states after
`accepted-by-adapter` are defined in `spec/session-channels.md` "Delivery states"
(planned, E4) and decided in C5 §9.

**5.3.4 — `TODO(fixture)`, hook H25.** A `whoami` result MUST NOT contain a private key,
bearer token or any other value that authenticates its holder as the caller (C6 §8).

### 5.4 Errors

MCP separates protocol errors (JSON-RPC errors "for issues like: Unknown tools, Malformed
requests ..., Server errors") from tool execution errors ("Reported in tool results with
`isError: true`"). Source: https://modelcontextprotocol.io/specification/2025-11-25/server/tools,
"Error Handling", retrieved 2026-10-03. `CallToolResult.isError` is present in both
the `2025-11-25` and `2026-07-28` schemas (lines 1129 and 1837 of the schema files cited
in §4.3).

**5.4.1 — `TODO(fixture)`, hook H26.** An OAC error from the closed taxonomy in
`spec/session-channels.md` "Error taxonomy" (planned, E4) MUST be returned as a tool
execution error with `isError: true`.

**5.4.2 — `TODO(fixture)`, hook H27.** A tool execution error MUST carry the OAC error
code, spelled exactly as the taxonomy spells it, in its `text` content.

**Error names are placeholders.** The closed taxonomy belongs to E4 (#44), which has not
landed. This document therefore names errors by role only: the *authorization-failure
error* (4.4.2), the *malformed-input error* (5.2.2) and the *unknown-destination error*
(H27). Each role is re-pointed to the taxonomy's exact code when E4 lands; until then no
fixture can assert a spelling.

**5.4.3 — `TODO(fixture)`, hook H28.** An OAC server MUST NOT return an OAC taxonomy
error as a JSON-RPC error object.

**5.4.4 — `TODO(fixture)`, hook H29.** An OAC server MUST NOT emit a JSON-RPC error code
in the range `-32020` to `-32099` that the MCP specification does not define.

5.4.4 repeats the MCP `2026-07-28` reservation of that range ("Implementations **MUST
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

**6.2.1 — `TODO(fixture)`, hook H30.** OAC provenance in `_meta` MUST be carried under
the single key `io.github.rossgraeber/oac-session-channels`.

The key is the extension identifier itself, checked against §6.1 in §3.2. G4 fact 5 shows
this key on the wire.

**6.2.2 — `TODO(fixture)`, hook H31.** The value under that key MUST be a JSON object.

**6.2.3 — `TODO(fixture)`, hook H32.** Every `_meta` key an OAC server writes MUST either
use the prefix `io.github.rossgraeber/` or be a key the MCP specification itself defines.

**6.2.4 — `TODO(fixture)`, hook H33.** An OAC server MUST NOT write a `_meta` key whose
prefix has `modelcontextprotocol` or `mcp` as its second label, except a key the MCP
specification defines for the role the server is playing.

**6.2.5 — members.** The members of the provenance object are copies of envelope and
receipt fields. They are named in `spec/session-channels.md` "Message envelope and content
model" and "Delivery states" (planned, E1 and E4). This binding defines none of them.

> **Reference implementation note:** the expected members are the message id, the
> conversation and correlation ids, the delivery state, and the spec revision. G4's spike
> carried only debugging members (`served_by_pid`, `surface`), which are not part of this
> binding.

### 6.3 What `_meta` provenance is not

**6.3.1 — `TODO(fixture)`, hook H34.** A receiver MUST NOT use a value read from `_meta`
as evidence of a message's authenticity or sender.

Authenticity comes from the envelope signature (`spec/security.md`, planned, E5; C5 §2-§6).
`_meta` is unsigned client- or server-supplied data. SEP-2133 says "Clients and servers
SHOULD treat any new fields or data introduced as part of an extension as untrusted"
(https://modelcontextprotocol.io/seps/2133-extensions, "Security Implications", retrieved
2026-10-03).

**6.3.2 — `TODO(fixture)`, hook H35.** An OAC server MUST NOT place a value only in
`_meta` when the calling model needs that value (see 5.3.1).

**6.3.3 — not the channel `meta`.** The Claude Code channel notification has its own
`meta` map (no underscore). Its keys become attributes on the `<channel>` tag, must be
identifier-safe (letters, digits, underscore), and are dropped silently otherwise
(`oac-claude-channels` §2). That map follows C6 §2's five `oac_*` keys, not §6.1's prefix
rules. A prefixed key such as `io.github.rossgraeber/oac-session-channels` contains `.`,
`/` and `-`, so Claude Code would drop it from a channel `meta` map.

**6.3.4 — `TODO(fixture)`, hook H36.** An OAC server MUST NOT put a `_meta`-style
prefixed key into a Claude Code channel `meta` map.

## 7. Unsupported extension

MCP: "If one party supports an extension but the other does not, the supporting party
**MUST** either revert to core protocol behavior or reject the request with an
appropriate error. Extensions **SHOULD** document their expected fallback behavior."
Source: MCP versioning page, "Extension Negotiation", revision `2026-07-28`, retrieved
2026-10-03. This section is OAC's fallback documentation.

**7.1 — `TODO(fixture)`, hook H37.** An OAC server MUST serve the tool surface (§5) to a
client that does not declare the OAC extension identifier.

Neither Claude Code nor Codex declares the identifier in the G4 fixtures: every
`extensions` map carrying it there is server-to-client (`transcript-2026-09-26.jsonl`
lines 7, 11, 27, 32; `transcript-row41-2026-09-27.jsonl` lines 2, 4). Requiring it would
cut off both harnesses. The OAC tools are ordinary MCP tools and need no client-side
extension support.

**7.2 — `SHOULD`.** A client that does understand the identifier SHOULD declare it in
its own `capabilities.extensions`. A client deviates by not declaring it; the server
still serves it under 7.1.

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
  (C5 §9; `spec/session-channels.md` "Delivery states", planned).
- **Era:** "A channel server that negotiates MCP protocol `2026-07-28` cannot deliver
  channel messages and is not registered as a channel" (`oac-claude-channels` §4; G4
  fact 3).
- **Outbound:** the four tools of §5.

**8.1.1 — `TODO(fixture)`, hook H38.** An OAC server MUST declare
`capabilities.experimental["claude/channel"]` only in an `initialize` result that
negotiates a legacy revision.

**8.1.2 — `TODO(fixture)`, hook H39.** An OAC server MUST NOT send
`notifications/claude/channel` on a connection that has not negotiated a legacy revision.

**8.1.3 — `TODO(fixture)`, hook H40.** A stdio OAC server started for the Claude channel
path MUST answer a `server/discover` probe with a JSON-RPC error that is not a recognized
modern error.

8.1.3 is how a stdio channel server stays on the legacy era. Under MCP `2026-07-28`, a
dual-era stdio client "probe[s] with `server/discover` and fall[s] back on any error that
is not a recognized modern error" (MCP versioning page, "Backward Compatibility",
retrieved 2026-10-03). G4's channel server did exactly this, with `-32601 Method not
found` (lines 6, 8), and Claude Code fell back to `initialize` (line 14). It is the one
exception to 4.3.5: a Claude-channel stdio process is legacy-only, by design.
`MCP_PROTOCOL_NEGOTIATION=legacy` can force the same result from the client side
(`oac-claude-channels` §4); 8.1.3 does not depend on the operator setting it.

The 2026-07-28 schema still defines `experimental` on both `ClientCapabilities` and
`ServerCapabilities` (schema at commit `271ecc9`, lines 720 and 797, cited in §4.3), so
8.1.1 restricts where the capability is declared, not whether the member exists.

> **Reference implementation note:** the Claude adapter's shim is a stdio process
> spawned by Claude Code (C2 §1). It therefore runs as the legacy-only exception in 8.1.3,
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
  under the interim rule in 4.4.2 until the Codex pairing (#69) is designed and documented.
- **Era:** legacy by default, modern only behind `mcp_2026_07_28` and only on HTTP
  registrations in the one run observed (G4 facts 7 and 8).

**8.2.1 — `TODO(fixture)`, hook H41.** An OAC server MUST NOT send any notification to a
Codex MCP client as a way of delivering an OAC message.

8.2.1 holds on both eras: Codex has no MCP surface that turns a notification into session
input, so such a notification could only mislead (§2.1). G4 recorded one such push with
no consumer (line 56).

## 9. Resolution of conflict-register row C5

Row C5 reads "Claude needs legacy MCP; Codex tool path may negotiate current MCP"
(`docs/planning/v0.1/03-decisions-and-amendments.md` §3;
`docs/planning/ADR-001-AMENDMENTS.md` "Conflict register"). It was `ASSIGNED` to this
task by #228 (2026-10-02). It is resolved here as follows.

1. **The conflict was a design question: can one OAC server serve both needs?** It
   can. §4 requires every OAC server other than a channel-path server to be dual-era
   (4.3.1-4.3.5), and the MCP specification permits a dual-era server to serve both eras
   in one process (§4.1). G4 showed that topology working with real clients, alongside a
   legacy channel in the same process (§4.2 facts 1-4).
2. **The Claude half is fixed by requirement, not by hope.** The channel capability is
   declared only on legacy connections (8.1.1), channel notifications are sent only
   there (8.1.2), and a Claude-channel stdio process refuses the modern probe so that the
   client falls back (8.1.3).
3. **The Codex half no longer needs to be true.** "Codex tool path *may* negotiate current
   MCP" was an open element because OAC's design seemed to depend on it. Under this
   binding the tool *contract* does not: the tool surface is identical in both eras
   (4.3.7, 5.1.2). The honest consequence is about calls, not the contract: whether a
   Codex call is served depends on caller attribution (§4.4), and no Codex connection is
   bound today. If Codex's default client moves to `2026-07-28` before a binding signal
   exists, Codex's outbound calls are refused (fail-closed) on that era as on the legacy
   one. That stays tracked under RISK-G4 row 41, and the Codex pairing design under #69.
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
| A legacy client other than Codex `0.157.1` ignores an `extensions` member in an `initialize` result | UNVERIFIED (new, §4.3.8) | added to `docs/planning/STATUS.md` in this change |

## 11. Boundary self-check (`oac-boundaries`)

- **Delivery.** No section makes MCP the delivery mechanism. §2 forbids the claim, and
  §8 names each provider's native path.
- **Polling.** 2.2 forbids an MCP polling surface while active inbound is claimed.
- **Credentials.** `whoami` never returns a credential (5.3.4). Nothing here reads or
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

## 12. Conformance fixture hooks

Each hook names the fixture that will prove one requirement. Fixtures land under
`tests/protocol/` (task E8) once E1 fixes the requirement-id scheme; until then every
row is `TODO(fixture)`. The input is MCP JSON-RPC traffic, and the expected outcome is
an accept, a reject or an emitted shape. A second implementation can run each one without
any provider installed.

| Hook | Requirement | Input | Expected outcome |
|---|---|---|---|
| H1 | 2.1 | `server/discover` and `initialize` results, `tools/list`, `resources/list`, `prompts/list` | exposed capability keys are exactly `tools`, `extensions` (holding only the §3 identifier) and, on a legacy channel-path connection only, `experimental["claude/channel"]`; tool names are exactly §5.1's four; no resource or prompt is listed |
| H2 | 2.2 | `resources/list`, `prompts/list`, `subscriptions/listen` while active inbound is declared | no inbox-shaped resource, prompt or subscription offered |
| H3 | 3.3 | `initialize` result; `server/discover` result | identifier string byte-equal to `io.github.rossgraeber/oac-session-channels` |
| H4 | 3.5 | the spec revision a server declares under the identifier (settings member, E2) | the revision is in `spec/session-channels.md`'s non-breaking list for the frozen identifier; any other value fails |
| H5 | 3.6.1 | settings value under the identifier | a JSON object |
| H6 | 3.6.2 | client capabilities with an unknown settings member | request served normally |
| H7 | 4.3.1 | legacy `initialize` | result returned |
| H8 | 4.3.2 | `initialize` with `protocolVersion: "2025-11-25"` | answered `2025-11-25` |
| H9 | 4.3.3 | `initialize` with `protocolVersion: "2025-06-18"` (from G4 line 26) | answered `2025-11-25` |
| H10 | 4.3.4 | modern `tools/list` with `protocolVersion` `2026-07-28` | served |
| H11 | 4.3.5 | modern `server/discover` on a non-channel surface | `supportedVersions` contains both revisions |
| H12 | 4.3.7 | `tools/list` on a legacy-era connection and as a modern-era request | identical names, `inputSchema` and declared result shape |
| H13 | 4.3.8 | legacy `initialize` | `capabilities.extensions` holds the identifier |
| H14 | 4.3.9 | `server/discover` | `capabilities.extensions` holds the identifier |
| H15 | 4.3.11 | (a) request with no `initialize` on its stdio process or HTTP session and no `protocolVersion` (negative); (b) request without `protocolVersion` inside an initialized legacy connection | (a) JSON-RPC `-32602`; (b) served |
| H16 | 4.4.1 | two tool calls on one unbound connection carrying different client-asserted session ids (negative) | neither attributed by connection alone; both refused per H17 |
| H17 | 4.4.2 | `send`, `list_sessions` and `whoami` on an unbound connection, legacy and modern (negative) | `isError: true` result carrying the authorization-failure error for each (code pending E4) |
| H18 | 5.1.1 | `tools/list` | exactly `send`, `reply`, `list_sessions`, `whoami` present |
| H19 | 5.1.2 | `tools/list` on each era | identical names, schemas and result shapes |
| H20 | 5.1.3 | `tools/list` as two different clients | identical |
| H21 | 5.2.1 | each `inputSchema` | valid JSON Schema object; property names match the neutral spec |
| H22 | 5.2.2 | `send` missing a required argument, on a bound connection (negative) | `isError: true`, the malformed-input error (code pending E4) |
| H23 | 5.3.1 | each tool's result | required values present in a `text` block |
| H24 | 5.3.3 | `send` result | state is `accepted-by-adapter` or an error |
| H25 | 5.3.4 | `whoami` result | no key material or token |
| H26 | 5.4.1 | `send` to an unknown session, on a bound connection (negative) | `isError: true` result, not a JSON-RPC error |
| H27 | 5.4.2 | same | `text` contains the unknown-destination error's exact code (pending E4) |
| H28 | 5.4.3 | every OAC error case | never a JSON-RPC error object |
| H29 | 5.4.4 | all server errors | no undefined code in `-32020`..`-32099` |
| H30 | 6.2.1 | any result carrying OAC provenance | under `io.github.rossgraeber/oac-session-channels` only |
| H31 | 6.2.2 | same | value is a JSON object |
| H32 | 6.2.3 | every `_meta` the server writes | prefix `io.github.rossgraeber/` or MCP-defined |
| H33 | 6.2.4 | every `_meta` key the server writes in a scripted session on both eras | no key matches `^[A-Za-z][A-Za-z0-9-]*\.(modelcontextprotocol\|mcp)[./]` except the keys MCP defines for a server's role (e.g. `io.modelcontextprotocol/serverInfo`, `io.modelcontextprotocol/subscriptionId`) |
| H34 | 6.3.1 | envelope whose `_meta` names a sender different from the signed `from` (negative) | sender taken from the verified envelope only |
| H35 | 6.3.2 | each tool's result | no model-needed value present only in `_meta` |
| H36 | 6.3.4 | each channel notification | every `meta` key matches `^[A-Za-z0-9_]+$` |
| H37 | 7.1 | `tools/list` from a client declaring no extensions; `tools/call` from such a client on a bound connection | both served |
| H38 | 8.1.1 | modern `server/discover` result (negative) | no `experimental["claude/channel"]` |
| H39 | 8.1.2 | modern-era traffic on a channel server (negative) | no `notifications/claude/channel` sent |
| H40 | 8.1.3 | stdio `server/discover` on a channel server | JSON-RPC error that is not a recognized modern error |
| H41 | 8.2.1 | a delivery to a Codex session | no MCP notification emitted toward the Codex client |

G4's committed transcripts are the first source of inputs for H7-H15, H38 and H40. They
are gate evidence, not conformance fixtures, so each fixture is re-captured in the E8
format rather than copied.
