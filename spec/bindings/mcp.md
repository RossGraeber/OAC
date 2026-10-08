# OAC Session Channels — MCP binding

- **Document:** the MCP binding for OAC Session Channels (backlog task E6, issue #46).
- **Status:** normative; frozen at Gate S2: signed off 2026-10-06, in force from the merge
  of PR #276 (E7, #47). Every requirement carries an `MCPB` id; the index in §12 lists its
  fixtures or marks it `TODO(fixture)`.
- **Binding revision:** `0.2`, a minor revision of the frozen `0.1`, made under
  `docs/planning/decisions/E7-interface-freeze.md` §7 and in force from the lead's approval
  and merge of PR #350 (issue #69). Appendix A records the change.
  Fixtures written against `0.1` keep citing `spec_revision` `0.1`; fixtures added in
  `0.2` cite `0.2`.
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
landed by #41) by section number and requirement id. Its sections 4 to 8 are written,
including §6 (E2, #42), §7 (E3, #43) and §8 (E4, #44). The error codes here are those of
its closed taxonomy, §8.3; see §5.4.

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
(`oac-boundaries` boundary 14).

**2.3 — what does deliver.** Inbound delivery into a live session happens through a
provider-native surface, named in the provider profiles (§8):

- Claude Code (research preview): the `notifications/claude/channel` notification on a
  legacy-era connection that declared `capabilities.experimental["claude/channel"]`
  (`oac-claude-channels` §1, §4).
- Codex (experimental, per-method gating): the app-server methods `thread/queue/add`,
  `turn/steer` and `turn/start` (`oac-codex-appserver`; C3 §5). These are not MCP
  methods. Codex uses MCP only for the outbound tool path (§8.2). Delivery uses
  `thread/queue/add` only; §8.2.1 says why and forbids the other two for delivery.

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
- This revision of the binding defines no members of the settings object, so an OAC
  server sends `{}`. `spec/session-channels.md` §6.4 defines a capabilities entry per
  *session*, and an OAC server process can serve connections for more than one session,
  so no session's declaration belongs in the server's settings object.
- A session's capability declaration reaches other implementations in its presence
  announcements (`spec/session-channels.md` §7.2). A sender takes the declaration it uses
  only from an announcement it accepted or from the descriptor it announces itself
  ([SC-DLV-070]). `list_sessions` (§5.5) shows the calling harness the descriptors of the
  sessions it may discover; it is a view for the harness, not a source of declarations.
  A declaration that a client sends back, in a tool argument or anywhere else, is never
  used under `spec/session-channels.md` §6.5 or §6.6.

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
attribution (§4.4), which is satisfied only on a connection bound by a documented
pairing. For Codex that pairing is §4.5, which applies to stdio connections only, whatever
era they negotiate. A Codex HTTP registration stays unbound, so its calls are refused
(fail-closed, [MCPB-ATT-002]) on both eras. Codex's default client era stays tracked under
`docs/planning/v0.1/11-risks.md` row 41 (RISK-G4).

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
envelope's `from` (`spec/session-channels.md` §6.8), `list_sessions` uses it as the
discovery requester (`spec/session-channels.md` §7.3; C5 §11), `send` and `reply` also use
it as the requester whose discovery authorization limits what a refusal reveals
([SC-DLV-075], [SC-DLV-076]), and `whoami` returns it.

**Bound connections.** A connection is *bound* when a documented OAC pairing has tied it
to exactly one harness session. Two pairings are documented: the Claude channel-path
stdio shim's hook-to-shim pairing (C4 §3), and, from binding revision 0.2, the Codex
issued-value pairing of §4.5, which binds a stdio connection that Codex started. No other
connection is bound, on either era.

*Dated note, 2026-10-08 (#69): in revision 0.1 this paragraph said that no Codex
connection is bound, because C4 §4 captures `thread.id` through the daemon's app-server
client but defines no attribution for outbound MCP tool calls. §4.5 is that attribution.*

**Why the connection alone is not enough.**

- On the modern era, MCP `2026-07-28` says "a server must not treat connection or process
  identity as a proxy for conversation or session continuity", and "Clients **SHOULD NOT**
  use an individual task, thread, or conversation as the lifetime boundary for the stdio
  process". Source: MCP basic page, "Statelessness", revision `2026-07-28`, retrieved
  2026-10-03.
- On the legacy era, Codex `0.157.1` sends a thread id on every `tools/call`
  (`transcript-2026-09-26.jsonl` lines 48, 50), which suggests one Codex MCP client
  connection can carry calls from several threads. At `0.161.0` the source says the
  opposite: each thread owns its MCP runtime and so its own connections (§4.5.1, fact
  C2). No run has shown either, so it stays UNVERIFIED (§10), and §4.5 is built so that
  its attribution does not depend on it.

[MCPB-ATT-001] On a connection that is not bound, an OAC server
MUST NOT attribute a tool call to an OAC session using only the identity of the
connection or process that carried it.

[MCPB-ATT-002] When an OAC server cannot attribute a tool call
on a connection that is not bound to exactly one OAC session, it MUST refuse the call with
the error code `unauthorized` (§5.4).

Requirement MCPB-ATT-002 is the MCP form of `spec/session-channels.md` [SC-ID-161]: a send
request on an attachment not bound to exactly one session is refused, with the code
`unauthorized` (`spec/session-channels.md` Table 8.3.3, request scope). The refusal of a
`list_sessions` call on such a connection is the MCP form of `spec/session-channels.md`
[SC-DLV-060], which refuses a discovery request on such an attachment with the same code
(§5.5).

**Per-request signals are not used.** In revision 0.1 the refusal in [MCPB-ATT-002] was
called interim: it was to hold until a documented per-request signal existed that OAC could
bind to a paired session. None exists. The first-party app-server documentation names no
`_meta` member that Codex sends to an MCP server (§4.5.1, fact C1). Codex does send several
in source and on the wire: `_meta["x-codex-turn-metadata"]` carries `session_id`,
`thread_id` and `turn_id` (plus `codex_version`, `model` and other fields) on the legacy
calls at `transcript-2026-09-26.jsonl` lines 48 and 50 and on the modern call at
`transcript-row41-2026-09-27.jsonl` line 17, and the source adds `callId`, `threadId` and
`sessionId` beside it (§4.5.1, fact C1). OAC relies on none of them, for two reasons:

1. They appear in no first-party Codex documentation this project cites, so depending on
   them would rely on an undocumented surface (`[ADR-001 Boundary]`: "MUST NOT depend on
   UI/terminal scraping or undocumented private RPCs for supported integrations";
   `oac-boundaries` boundary 4).
2. They are client-asserted ids with nothing binding them to a paired session. Under the
   "authenticated but untrusted" doctrine (`oac-security-work` §3), C4's rule that a
   self-reported id is never the sole pairing key (C4 §3, §13) and
   `spec/session-channels.md` [SC-ID-162], they cannot attribute a call.

The refusal in [MCPB-ATT-002] is therefore permanent for every connection that no documented
pairing binds. It does not affect Claude Code's channel path, which is bound (§8.1). A Codex
stdio connection is bound by the per-thread pairing of §4.5, which uses a report that Codex
makes about its own thread, not a value the request carries.

*Dated note, 2026-10-03: operator decision on #46 (https://github.com/RossGraeber/OAC/issues/46#issuecomment-5973893128, also recorded on #69).* Until
a documented pairing ties an MCP tool call to exactly one Codex conversation (#69), Codex's
outbound tool calls are refused on both MCP eras, as [MCPB-ATT-002] states. Codex sessions can still
receive OAC messages; sending from Codex waits for #69. Codex's self-reported
`x-codex-turn-metadata` ids are not used for pairing.

*Dated note, 2026-10-08 (#69): §4.5 is the pairing that decision waits for. It keeps the
decision's last sentence: no self-reported Codex id is used. It also corrects the decision's
second-last sentence. Under `spec/session-channels.md` §6.7 a Codex session receives OAC
messages only once it is bound, and in revision 0.1 no Codex native signal could be paired
([SC-ID-125]), so none could be. Under §4.5 a Codex session is bound, and can receive, from
its first OAC tool call on (§4.5.6).*

> **Reference implementation note:** the v0.1 shim binds a Claude channel-path stdio
> connection to one harness session through the hook-to-shim pairing in C4 §3, and a Codex
> stdio connection through the issued-value pairing of §4.5.

### 4.5 Codex: the issued-value pairing

This section is the documented pairing that the #46 decision waits for. The lead decided on
2026-10-08 to have it designed under #69 and proposed as a change to this frozen document.
It ties one Codex stdio connection to exactly one Codex thread. It uses two things only: an
observation the implementation makes itself from the operating system, and a random value
the implementation issues on the connection and then sees again in Codex's own report about
the thread. It never uses a value that the call or its `_meta` carries.

#### 4.5.1 Evidence

Source citations are to `openai/codex` tag `rust-v0.161.0`, commit
`979011409de0a60b52f179721948e65531d26144`, the last tested Codex version
(`docs/planning/PINS.md`), retrieved 2026-10-08. `B` below is
`https://github.com/openai/codex/blob/979011409de0a60b52f179721948e65531d26144/codex-rs`.
`D` is the first-party app-server documentation, https://learn.chatgpt.com/docs/app-server
(where https://developers.openai.com/codex/app-server redirects; unversioned), retrieved
2026-10-08. Each fact says how strong its backing is: documented (`D` or the checked-in
schema), recorded (a committed fixture), or source only.

- **C1 — no documented per-request signal.** `D` names no `_meta` member that Codex sends
  to an MCP server. Source only: Codex adds `callId` and `x-codex-turn-metadata`
  (`B/core/src/mcp_tool_call.rs#L1320-L1356`), `threadId` and `sessionId`, and, for a call
  that has an originating window, `windowId` (`#L1409-L1431`, key names at
  `#L1262-L1266`) to a `tools/call` `_meta`. Not used (§4.4).
- **C2 — the MCP runtime is per thread (source only).** Each Codex session creates its own
  MCP runtime, commented as "one stable thread-owned MCP runtime handle"
  (`B/core/src/session/session.rs#L1606-L1608`; `B/codex-mcp/src/runtime.rs#L3-L4`). So
  each thread starts its own stdio server process. `D` does not say this, and no run has
  shown it, so it is UNVERIFIED (§10). This section uses it for availability only: if it
  fails, calls are refused, never misattributed (§4.5.5).
- **C3 — the launch environment names no thread.** A stdio server's environment is a fixed
  allow-list of the Codex process's own variables plus the registration's static `env` and
  `env_vars` (`B/rmcp-client/src/utils.rs#L16-L60`, `#L162-L179`;
  `B/config/src/mcp_types.rs#L616-L623`). Every thread's server process gets the same
  values, so neither the environment nor the `codex mcp add` registration carries a
  per-thread value.
- **C4 — every MCP tool call is reported as an `mcpToolCall` item (documented).** `D` lists
  the item as "`mcpToolCall` - `{id, server, tool, status, arguments, appContext?,
  pluginId?, result?, error?}`". The checked-in schema makes `threadId`, `turnId` and
  `item` required members of both `item/started` and `item/completed`
  (`B/app-server-protocol/schema/json/v2/ItemStartedNotification.json`,
  `ItemCompletedNotification.json`; `B/app-server-protocol/src/protocol/v2/item.rs#L337-L356`,
  `#L1337-L1344`, `#L1415-L1422`), with no experimental marker. `server` is the name the
  server is registered under, `arguments` the call's parsed arguments, and `result` the
  call's result (`content`, `structuredContent`, `_meta`) (`item.rs#L1031-L1057`).
- **C5 — order (source only).** `item/started` is emitted before any approval prompt and
  before the call is made (`B/core/src/mcp_tool_call.rs#L259-L265`; approval `#L273`; call
  `#L466`). `item/completed` carries the result after the call returns and before the
  result goes back to the model (`#L623-L632`, `#L1058-L1102`). Between the two, the result
  can be changed by a tool-lifecycle extension built into Codex
  (`B/core/src/tools/lifecycle.rs#L86-L108`), for image or audio blocks the model cannot
  take (`B/core/src/mcp_tool_call.rs#L942-L979`), and above a size cap
  (`B/core/src/mcp_tool_call.rs#L981-L1023`). An error result (`isError: true`) is reported
  in `result`, with status `failed`, not in `error` (`#L1058-L1102`). Source shows no
  `item/completed` for a call whose turn is aborted while the call is in flight. A pairing
  refusal is short
  text, so only the first could touch it. A changed refusal is simply never a reveal, which
  costs availability only. Runtime order and content are UNVERIFIED (§10).
- **C6 — item notifications carry the thread's id and reach the thread's subscribers.**
  `D`: `thread/start` "automatically subscribes you to turn/item events for that thread".
  Source: an item notification carries the id of the thread that emitted it and is sent to
  the connections subscribed to that thread
  (`B/app-server/src/bespoke_event_handling.rs#L1073-L1135`;
  `B/app-server/src/outgoing_message.rs#L142-L209`). Recorded: a connection that subscribed
  to a TUI-hosted thread with `thread/resume` received `item/started` with that thread's
  `threadId` (`docs/planning/gates/fixtures/g2-codex-inject/transcript.jsonl` lines 51 and
  63, Codex `0.154.0`; `docs/planning/gates/G2-result.md`). Routing is not exact, though:
  at `0.161.0` some thread notifications (`thread/status/changed`, `thread/closed`,
  `thread/goal/cleared`, `thread/archived`) reached a connection subscribed only to another
  thread, and a connection after its `thread/unsubscribe`
  (the S3 Codex capture of 2026-10-07 in `docs/planning/gates/fixtures/s3-codex-capture/`,
  L63-L69, L829, L959-L997; `docs/planning/v0.1/11-risks.md` row 70). So which threads'
  notifications arrive on the carrier is not a guarantee; [MCPB-ATT-018] makes the
  subscription a rule of the server instead. No run has recorded an `mcpToolCall` item on a
  subscription, so that is UNVERIFIED (§10).
- **C7 — `thread/start` and `thread/resume` can change settings.** `thread/resume`'s
  parameters include `model`, `cwd`, `approvalPolicy`, `sandbox`, `permissions` and a
  `config` map of configuration keys (`B/app-server-protocol/src/protocol/v2/thread.rs#L354-L414`);
  `thread/start` has the same `config` map (`thread.rs#L100`). `D`: `thread/resume` takes
  "the same configuration overrides supported by `thread/start`".
- **C8 — dynamic tools.** A client can register tools with the experimental
  `thread/start.dynamicTools` (`thread.rs#L145-L151`), and their calls reach the client as
  the `item/tool/call` request with the thread's id (`B/app-server-protocol/src/protocol/common.rs#L1808-L1811`).
  Only the client that starts a thread can register them.
- **C9 — a call with no arguments (source only).** When the model gives an empty argument
  string, Codex parses it to no value (`B/core/src/mcp_tool_call.rs#L145-L150`), reports the
  item's `arguments` as `null` (`#L1041`, `#L1087`), and sends the `tools/call` without an
  `arguments` member (`B/rmcp-client/src/rmcp_client.rs#L847-L855`, `#L868`). Arguments are
  otherwise sent as parsed, unless the tool declares file-input fields, which the OAC tools
  do not (`B/core/src/mcp_openai_file.rs#L39-L41`). A non-object argument value is refused
  by Codex before any call is sent (`rmcp_client.rs#L847-L853`).
- **C10 — turns end with `turn/completed`.** `D`: it carries `{ turn }`, emitted "with final
  status when the model finishes or after a `turn/interrupt` cancellation". Recorded at
  `0.161.0` with `threadId` and `turn.id`
  (the S3 Codex capture of 2026-10-07 in `docs/planning/gates/fixtures/s3-codex-capture/`,
  L90). An `item/started` carries the same turn's id as `turnId` (C4).

#### 4.5.2 Terms

- **Codex connection:** the stdio connection between Codex and an OAC server process that
  Codex started from a stdio registration. A connection to an OAC server that Codex reaches
  over HTTP is not a Codex connection, and no pairing binds it.
- **Registered name:** the name under which the operator registered the OAC server with
  Codex (`codex mcp add <name> ...`, or a `[mcp_servers.<name>]` table). Codex reports it
  as an item's `server` (C4).
- **Carrier:** the Codex adapter's own connection to the Codex app-server (§2.3; C4 §4),
  on which it subscribes to threads and receives their notifications. It carries native
  signals (`spec/interfaces.md` §5.4); it is not an attachment.
- **Subscribed thread:** a thread to which the carrier subscribed (by `thread/start`,
  `thread/resume` or `thread/fork`) and from which it has not since unsubscribed.
- **Pairing value:** a random string that the implementation issues on one Codex connection
  for one pairing window.
- **Pairing window:** the period that starts when the server sends a pairing refusal on a
  Codex connection that has no open pairing window, and ends when a reveal of its value is
  paired or when its bounded length ([MCPB-ATT-019]) runs out, whichever comes first.
- **Pairing refusal:** the `unauthorized` refusal of [MCPB-ATT-002] for a call on an unbound
  Codex connection, or under [MCPB-ATT-015], when it carries a pairing value.
- **Matching arguments:** a call's `arguments` and an item's `arguments` match when they are
  equal JSON values, and also when the call has no `arguments` member and the item's
  `arguments` is `null` (C9). An empty object `{}` matches only `{}`.
- **Reveal:** an `item/completed` notification, received on the carrier, whose `item` has
  `type` `mcpToolCall`, whose `server` is the registered name, whose `tool` and `arguments`
  match the name and arguments of a call that got a pairing refusal, and whose
  `result.content` is the same JSON value as that pairing refusal's `content`. A reveal
  reveals the pairing value of that refusal.
- **Confirmation:** an `item/started` notification, received on the carrier for the thread
  a Codex connection is bound to, whose `item` has `type` `mcpToolCall`, whose `server` is
  the registered name, and whose `tool` and `arguments` match a call's name and arguments.
  It confirms that call while it is live ([MCPB-ATT-021]).

#### 4.5.3 How the pairing works

1. A Codex thread T calls an OAC tool on its connection C. C is unbound, so the call is
   refused with `unauthorized` ([MCPB-ATT-002]). The refusal carries the pairing value V of
   C's current pairing window. No envelope is created, and the refusal names no session.
2. Codex reports T's call on the carrier: an `item/completed` whose `threadId` is T and
   whose result is the refusal, V included (C4, C5, C6). That is a reveal.
3. The reveal is the native signal for the harness-native id T
   (`spec/session-channels.md` §6.7.1). The implementation pairs it with C using two keys.
   The first it observes from the operating system: C's process descends from the Codex
   process at the other end of the carrier ([SC-ID-121]). That key admits every process
   that Codex process starts, the OAC server processes of all its threads and any process a
   thread's own tools start alike, so it does not tell them apart. The second key does: V
   was written to C only, so it leaves C as the one candidate ([SC-ID-125]). The binding
   then follows `spec/session-channels.md` §6.7.3.
4. T's model calls the tool again, which the pairing refusal's text allows once. On a bound
   connection each call is served only once a live confirmation for it has arrived: Codex's
   own report that thread T is making that call now.

V decides which thread's reports are consulted for C. The confirmation decides each call.
A wrong pairing therefore cannot make a call count as T's unless T's own report shows T
making a call with the same name and arguments in its current turn (§4.5.5).

Neither key is a value the attaching process supplies ([SC-ID-121]). The OAC server process
on C relays nothing it chose; V is issued by the implementation and comes back in a report
the harness makes about its own thread, through the carrier. The thread id is the
notification's `threadId`, set by the app-server (C6), never a value from the call.

**One value per window.** Every pairing refusal on C during one pairing window carries the
same V ([MCPB-ATT-025]). The value therefore depends on the connection and the time only,
never on the call: two calls that differ only in `to` get the same result, so
[MCPB-TOOL-017], [MCPB-TOOL-019] and [MCPB-TOOL-021] hold for pairing refusals as written.

**Start kind and cross-check.** This binding maps every reveal to the start kind `fresh`.
Codex reports no cross-check value. A reveal that binds therefore binds under case 3(c) of
`spec/session-channels.md` §6.7.3, under a new session id, with its diagnostic
([SC-ID-139], [SC-ID-141]). A thread whose connection is replaced (for example after Codex
restarts its MCP servers) is paired again and gets a new session id; the old attachment
ended with its process ([SC-ID-155]).

**Operating-system key.** Which call yields the peer process of the carrier and of C, and
the ancestry between them, on each platform is UNVERIFIED. It is the same open item as the
Claude pairing's (`spec/session-channels.md` §6.7.2 dated note; C4 §3 "Pairing
requirement"), owned by G9 (#70). Until it is established every reveal is unpairable
([SC-ID-125]), and the server issues no pairing value ([MCPB-ATT-022]): that costs
availability, never authority.

#### 4.5.4 Requirements

[MCPB-ATT-003] An OAC server MAY bind a Codex connection by the pairing of this section.

An OAC server that does not leaves every Codex connection unbound and refuses its calls under
[MCPB-ATT-002], as revision 0.1 did. The requirements below bind only a server that pairs.

[MCPB-ATT-004] A pairing refusal MUST carry its pairing value in a `text` content block.

The value has to be in `content`, because a reveal carries `result.content` (C4) and is
compared with it.

[MCPB-ATT-005] A pairing value MUST be the string `oac-pair-` followed by 32 lower-case
hexadecimal digits.

The 32 digits carry the 128 random bits of `spec/interfaces.md` [IFC-ADP-090]. The form
cannot be read as a session id (26 Crockford Base32 characters, `spec/session-channels.md`
§6.1) or a device fingerprint (64 hexadecimal digits, `spec/security.md` §5.2).

[MCPB-ATT-025] Every pairing refusal that an OAC server sends on one Codex connection during
one pairing window MUST carry that window's pairing value.

A new window, on the same connection or another, gets a new value from [IFC-ADP-090]'s
source. "One value per window" above says why the value must not vary per call.

[MCPB-ATT-006] An OAC server MUST NOT place a pairing value in any message other than a
pairing refusal on the connection it was issued on.

A reveal counts only when its result equals a pairing refusal. If the server echoed a
pairing value anywhere else, for example by copying an argument into a result, a thread that
had learned the value could make Codex report a reveal for a connection that is not its own.

[MCPB-ATT-007] An OAC server MUST bind a Codex connection only on a reveal of a pairing value
that the server issued on that connection.

[MCPB-ATT-008] An OAC server MUST take the harness-native id of a reveal only from the
notification's `threadId` member.

Never from the call's arguments or `_meta` ([SC-ID-162]), and never from
`x-codex-turn-metadata` (#46 decision).

[MCPB-ATT-018] An OAC server MUST NOT use as a reveal or a confirmation a notification whose
`threadId` is not a subscribed thread of its carrier.

Notifications for other threads can reach the carrier (C6). Taking only subscribed threads
keeps pairing to the threads the adapter chose to serve, which are the threads launched
OAC-enabled (`[ADR-001-A2 Amendment]`).

[MCPB-ATT-019] An OAC server MUST end every pairing window no later than 60 seconds after it
started.

The length is an implementation setting; any length up to 60 seconds meets the rule. A reveal normally follows its refusal within one app-server notification (C5).

[MCPB-ATT-009] An OAC server MUST NOT bind on a reveal that it receives after the pairing
window of the revealed value has ended.

A reveal that arrives later is dropped with a diagnostic, as [SC-ID-124] and [SC-ID-128] drop
a signal whose window ended.

[MCPB-ATT-010] An OAC server MUST NOT pair a reveal of a pairing value that an earlier reveal
has already paired.

[MCPB-ATT-011] When an OAC server receives a reveal of an already paired pairing value that
names a thread other than the one the earlier reveal named, it MUST end the binding of the
connection the value was issued on.

[MCPB-ATT-012] An OAC server that ends a binding under [MCPB-ATT-011] MUST record a finding.

One value revealed for two threads can only mean that a second server under the registered
name, or something else outside this design, produced the value (§4.5.5). The connection
ends unbound, as for a stale binding ([SC-ID-152], [SC-ID-153]).

[MCPB-ATT-013] An OAC server MUST NOT serve a tool call on a bound Codex connection unless it
holds a live confirmation of that call.

[MCPB-ATT-020] An OAC server MUST NOT hold a call on a bound Codex connection waiting for its
confirmation for longer than 10 seconds.

The length is an implementation setting; any length up to 10 seconds meets the rule. A confirmation normally arrives before its call, because Codex reports the item
before any approval prompt and before the call (C5).

[MCPB-ATT-021] An OAC server MUST NOT use a confirmation after the first of: the
`item/completed` of the same item `id`, the `turn/completed` whose `threadId` and `turn.id`
are the confirmation's `threadId` and `turnId`, and 600 seconds after the confirmation
arrived.

Before the first of those a confirmation is **live**. Source shows no `item/completed` for a
call whose turn is aborted while the call is in flight (C5), so the turn's end (C10) and the
time limit are what end an orphaned confirmation. The 600 seconds cover an approval prompt
the user answers slowly; a call approved later than that is refused and the connection
re-pairs (availability only). Any shorter limit meets the rule too.

[MCPB-ATT-014] An OAC server MUST NOT use one confirmation for more than one call.

[MCPB-ATT-015] An OAC server that has no live confirmation for a call on a bound Codex
connection when the wait of [MCPB-ATT-020] ends MUST refuse the call with `unauthorized`.

[MCPB-ATT-016] An OAC server that refuses a call under [MCPB-ATT-015] MUST stop serving
calls on that connection until a later reveal is paired with it.

The refusal under [MCPB-ATT-015] is a pairing refusal when [MCPB-ATT-022] and
[MCPB-ATT-023] allow one, so the next call can re-pair the connection. A reveal for the
thread the connection is bound to leaves the binding unchanged (`spec/session-channels.md`
[SC-ID-130]) and lifts the stop. `spec/interfaces.md` [IFC-ADP-091] to [IFC-ADP-093] carry
this between the adapter and the core.

[MCPB-ATT-022] An OAC server MUST NOT send a pairing refusal on a Codex connection that it
cannot pair.

A connection cannot be paired when the operating-system key is not available on the
platform (§4.5.3), or when that key shows that the connection's process does not descend
from the Codex process at the other end of the carrier, as for a thread in an embedded TUI
server or in Codex Desktop. Its calls get the plain `unauthorized` refusal of
[MCPB-ATT-002], with no value.

[MCPB-ATT-023] An OAC server MUST NOT open more than three pairing windows on one Codex
connection without pairing a reveal on it.

After the third window ends unpaired, the connection's calls get the plain `unauthorized`
refusal for as long as the connection lasts. A new connection starts again.

[MCPB-ATT-024] The text of an `unauthorized` refusal on a Codex connection that carries no
pairing value SHOULD NOT invite the caller to call again.

Such a refusal is final for that connection. A server deviates by asking for a retry; each
retry is then refused again, and where Codex asks the user to approve OAC tool calls, each
retry also prompts the user for a call that is then refused.

[MCPB-ATT-017] An OAC server SHOULD check that the `item/completed` notification of each
confirmation it used reports, as `result.content`, the content that the server returned for
the confirmed call, and treat a difference as it treats a missing confirmation
([MCPB-ATT-015], [MCPB-ATT-016]).

A server deviates by not checking. It then does not notice a connection that carries another
thread's calls (fact C2 failing) until a confirmation is missing. A result above Codex's
size cap is reported truncated (C5), so a server that checks compares only results below it.

> **Reference implementation note:** the v0.1 pairing refusal's text reads "unauthorized:
> this connection is not yet paired with a session; pairing value oac-pair-<32 hex digits>.
> You can call the tool once more." The plain refusal reads "unauthorized: this connection
> cannot be paired with a session." The v0.1 windows are 30 seconds for pairing, 5 seconds
> for the confirmation wait and 600 seconds for a confirmation's life. The adapter subscribes
> the carrier to each Codex thread it serves with `thread/resume` ([MCPB-CDX-006]) and keeps
> the live `mcpToolCall` items of each thread, keyed by item `id`.

#### 4.5.5 Threat table

Each row uses the `oac-security-work` §1 template. A mitigation whose proving test does not
exist yet is an open risk, carried in `docs/planning/v0.1/11-risks.md` (RISK-SEC-SPEC,
RISK-G4), not a closed mitigation. "G8" is #69, tested against the F9 fake app-server (#58),
which needs `mcpToolCall` items added for it (`docs/planning/v0.1/11-risks.md` row 74).

| Attack | Precondition | Mitigation | Proving test | Residual risk |
|---|---|---|---|---|
| Impersonation: thread U's call is attributed to thread T's session | U and T run in one Codex daemon; both call OAC tools | A connection binds only on the reveal of a value issued on it ([MCPB-ATT-007]), taken from the notification's `threadId` ([MCPB-ATT-008]) of a subscribed thread ([MCPB-ATT-018]); every call needs a live confirmation from T ([MCPB-ATT-013], [MCPB-ATT-014], [MCPB-ATT-021]) | `mcpb-att/MCPB-ATT-004.*`, `MCPB-ATT-005.*`, `MCPB-ATT-006.*`, `MCPB-ATT-025.*` (value form, one value per window, non-disclosure); the rest G8: not yet built, open risk | Only if one connection carried calls from T and U (fact C2 failing): while T has a live confirmation, that is, a call with the same name and matching arguments that T started in its current turn, at most 600 s ago, and has not completed, a call of U with that name and those arguments is served as T's. For `send` and `reply` its content is exactly what T itself asked to send. For `list_sessions` and `whoami`, often called with `{}`, U reads T's discovery view and T's own session id and device fingerprint, scoped by T's grants. T's own call is then refused for want of a confirmation; [MCPB-ATT-017] detects it afterwards |
| Self-asserted identity in the request: `_meta` `threadId`, `sessionId`, `windowId`, `callId` or `x-codex-turn-metadata`, or a session id in the arguments | A Codex client, a model or a prompt-injected peer puts an id in a call | Never read for attribution ([MCPB-ATT-008]; [SC-ID-162]; [MCPB-ATT-001]) | `mcpb-att/MCPB-ATT-001.n01`, `.n02`; `sc-id/SC-ID-162.p01` | None known |
| Forged reveal: a thread that learned V makes Codex report a result containing V | V leaked, for example through the model of a thread whose reveal was missed | V appears in no message but a pairing refusal on its own connection, arguments echoed included ([MCPB-ATT-006]); a reveal's whole `content` must equal a refusal ([MCPB-ATT-007] with §4.5.2); bounded window ([MCPB-ATT-019], [MCPB-ATT-009]); single use ([MCPB-ATT-010]); subscribed threads only ([MCPB-ATT-018]) | `mcpb-att/MCPB-ATT-006.n01`; window, single use and subscription: G8, open risk | An MCP server under the registered name, other than the OAC server, could return V for another thread. Anyone who can write that thread's Codex configuration can add one, and so can any client of the same app-server, without writing a file, through the `config` override of `thread/start` or `thread/resume` (C7). Both are the same-user local boundary of [SEC-AUZ-030]. One value revealed for two threads ends the binding with a finding ([MCPB-ATT-011], [MCPB-ATT-012]) |
| Replay of a pairing or a confirmation | An old reveal, or one `item/started`, is offered for a second call | Single-use values ([MCPB-ATT-010]); one confirmation per call ([MCPB-ATT-014]); confirmations expire ([MCPB-ATT-021]); notifications are taken only from the adapter's own carrier | G8: not yet built, open risk | None known |
| Race: the call reaches the server before its `item/started`, or a turn starts between a check and a call | Notifications and calls travel on different connections | The call waits for its confirmation within a bounded wait; nothing is served on a guess ([MCPB-ATT-013], [MCPB-ATT-020], [MCPB-ATT-015]); a missed confirmation stops the connection until it is paired again ([MCPB-ATT-016]) | G8: not yet built, open risk | A late notification costs a refused call and a re-pairing (availability, not authority) |
| A connection from a process other than the thread's own OAC server process | A local process, or a process a thread's own tools start, opens a connection to the core or speaks MCP to it | OS peer authentication [SEC-AUZ-030]. The OS pairing key only narrows candidates to processes the Codex process started, and a process a thread's tools start passes it too; what tells connections apart is V, which only the OAC server's own result carries back ([MCPB-ATT-006], [MCPB-ATT-007]) | G9 (#70) and G8: not yet built, open risk | Platform facility UNVERIFIED (`RISK-LOCAL-IPC`); until it is established no pairing value is issued ([MCPB-ATT-022]) |
| Endless retries and repeated approval prompts | Pairing cannot complete: no OS facility, a thread outside the carrier's app-server, or reveals that never arrive | No value where pairing is impossible ([MCPB-ATT-022]); at most three windows per connection ([MCPB-ATT-023]); a final refusal does not invite a retry ([MCPB-ATT-024]) | G8: not yet built, open risk | Up to three refused calls, each possibly prompting the user, before the connection gets the final refusal |
| Settings changed by the subscription | The adapter subscribes with `thread/resume` | No setting members ([MCPB-CDX-006]) | G7 (#68) / G8 against the F9 fake: not yet built, open risk | Whether the app-server applies resume overrides to a loaded thread is UNVERIFIED; it is forbidden either way |
| Malicious peer prompt injection | A granted peer sends adversarial content to T | Unchanged: a binding sets `from` and authorizes nothing ([SC-ID-157]); content stays untrusted (`spec/security.md` §1.2, [SEC-AUZ-020]) | as `spec/security.md` §13 | The model judges |
| Unauthorized routing or discovery; cross-project disclosure | A bound Codex session sends, or calls `list_sessions` | Unchanged: default deny and working-directory scoping (`spec/security.md` §9); refusals on an unbound connection name no session ([MCPB-TOOL-021]) | as `spec/security.md` §13 | As there |
| Tampering, replay of envelopes, compromised transport, a transport peer id taken as identity | Attacker on the transport | Unchanged: envelope signatures (`spec/security.md` §6, §8) | as `spec/security.md` §13 | As there |
| Leaked credentials | An adapter reads Codex's login | Unchanged: nothing here reads or holds a credential ([IFC-ADP-005]); a pairing value authenticates nothing outside one pairing window | as `spec/security.md` §13 | None added |
| Steering through `turn/steer` | Delivery to a running turn | Unchanged: §8.2.1 ([MCPB-CDX-002] to [MCPB-CDX-005]) | as there | As there |
| Permission relay; bypass of the Claude development-channel confirmation | Claude path only | Not touched by this section | as `spec/security.md` §13 | As there |

#### 4.5.6 Consequences, stated plainly

- **The first OAC call on every Codex connection is refused.** It is the pairing refusal,
  and the model calls once more. A Codex thread that never calls an OAC tool stays unbound:
  its session is not present, not discoverable, and receives no hand-off
  (`spec/session-channels.md` [SC-ID-182]; `spec/interfaces.md` [IFC-ADP-030]).
- **Where Codex asks the user to approve OAC tool calls, the pairing call is approved too.**
  The user approves a call that is then refused. A connection that cannot pair gets the
  final refusal at once ([MCPB-ATT-022]), and at most three pairing windows are opened
  ([MCPB-ATT-023]), so this is bounded.
- **Every served call waits for its confirmation**, which adds the latency of one app-server
  notification, and a call approved more than 600 seconds after Codex reported it is
  refused ([MCPB-ATT-021]).
- **A replaced connection gets a new session id** (§4.5.3). Peers addressing the old id no
  longer reach the thread, and grants are not carried forward ([SC-ID-151]).
- **Only subscribed threads can pair** ([MCPB-ATT-018]). A thread in an app-server the
  carrier does not reach (an embedded TUI server, Codex Desktop) is never paired: the
  "launched OAC-enabled" rule of `[ADR-001-A2 Amendment]`.
- **HTTP registrations stay refused** on both eras ([MCPB-ATT-002]); Codex's modern-era
  client is opt-in and HTTP-only in the one run observed (§4.2 fact 8).

#### 4.5.7 Rejected alternatives

| Alternative | Why rejected |
|---|---|
| `_meta` ids Codex sends (`threadId`, `sessionId`, `windowId`, `callId`, `x-codex-turn-metadata`) | Undocumented and client-asserted (C1; §4.4); [SC-ID-162]; #46 decision |
| A per-thread value in the server's launch environment or `codex mcp add` registration | None exists (C3) |
| The connection alone, taking one connection per thread | Per-thread connections are source only (C2); MCP `2026-07-28` forbids treating connection identity as session continuity (§4.4); [MCPB-ATT-001] |
| A token placed in the model's context, or the model echoing `oac_message_id` (`docs/planning/v0.1/11-risks.md` row 17) | Model-generated text never establishes identity (`spec/security.md` [SEC-PRV-002]; `oac-security-work` §3); a token in context leaks with the context |
| Matching calls to `item/started` across every subscribed thread, without a pairing value | A thread the adapter does not observe could take another thread's pending report by sending the same arguments, and so bind a connection to the wrong session |
| A fresh pairing value for every refusal | The value would then vary per call, so two refusals that differ only in `to` would differ, against [MCPB-TOOL-017], [MCPB-TOOL-019] and [MCPB-TOOL-021] as frozen |
| Dynamic tools (C8) | Experimental, registered only by the client that starts a thread, so not available for a thread a person started in the TUI; it would also move the tool surface off this binding. A candidate if a later revision has OAC start the thread |

## 5. Tool surface

The tool surface is the outbound half of OAC over MCP. Both harnesses send through the
same four OAC tools, and inbound stays provider-native (PLANNING-PROMPT.md §5 decision 9;
C6 §8). Argument and result semantics belong to `spec/session-channels.md` §8.2 (replies and
correlation), §8.1 (delivery states), §8.3 (errors), §6 (session identity, addressing and
send attribution) and §7.3 (discovery). This section fixes only how they are carried.

### 5.1 Tools

| Tool name | Purpose (semantics in the neutral spec) | Source |
|---|---|---|
| `send` | Send a new message to an addressed session | C6 §8 |
| `reply` | Reply to a received message (`in_reply_to`) | C6 §8, §10 |
| `list_sessions` | List the sessions the caller is authorized to see: the discovery result of `spec/session-channels.md` §7.3 (§5.5) | C6 §8; C5 §11 |
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
MUST return a tool execution error (§5.4) carrying the error code `invalid-request`.

`invalid-request` is the request-scope code of `spec/session-channels.md` Table 8.3 for "a
harness's request is malformed".

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

The tool call returns before the receiving harness has the message. `accepted-by-adapter`
is defined in `spec/session-channels.md` §8.1.2 (Table 8.1): its observer is the sending
implementation, and it means that the sending implementation created a valid envelope and
passed it to a transport. That is the sender-side reading of C6 §8, which §8.1.2 adopts in
its dated note, not C5 §9's receiver-side acceptance. In Table 8.1's terms, "beyond
`accepted-by-adapter`" means a state whose only observer is the receiver
(`handed-to-harness`, `rejected`, `expired`, `duplicate`). The OAC server, as the
sending implementation, has not observed any of them when the call returns
(`spec/session-channels.md` [SC-RCP-003]).

What a `send` or `reply` result reports therefore depends only on how far the sending
implementation got:

- **No envelope created.** The request was refused (Table 8.3.3). That is not a delivery
  state; the result is a tool execution error carrying the refusal's code (§5.4).
- **Envelope created, not passed to a transport.** The sending implementation observed
  `failed` (Table 8.1). The result is a tool execution error carrying `transport-failure`,
  or `internal-error` for an internal error unrelated to the envelope (Table 8.3, sender
  scope).
- **Envelope passed to a transport.** The result reports `accepted-by-adapter`. From then
  on the sending implementation reports neither `unreachable` nor `failed` from its own
  observation ([SC-RCP-007]). `unknown` applies only once the envelope's hand-off deadline
  has passed ([SC-RCP-010]), so it is a later state that the result does not carry.

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
`spec/session-channels.md` §8.3 MUST be returned as a tool
execution error with `isError: true`.

[MCPB-TOOL-011] A tool execution error MUST carry the OAC error
code, spelled exactly as the taxonomy spells it, in its `text` content.

[MCPB-TOOL-016] A tool execution error that refuses a `send` or `reply` before an envelope
exists MUST carry the code that `spec/session-channels.md` Table 8.3.3 assigns to the
refusal cause, chosen in the order of §8.3.3.

Requirement MCPB-TOOL-016 is the MCP form of `spec/session-channels.md` [SC-RCP-079] and
[SC-RCP-090], with a session the calling session is not authorized to discover taken as
`unknown` ([SC-DLV-075]). [MCPB-TOOL-011] governs how the code is spelled; [MCPB-TOOL-016]
governs which code it is.

**The codes this binding returns.** The closed taxonomy is `spec/session-channels.md`
§8.3, Table 8.3. A refusal before any envelope exists refuses a harness's request, so its
code is one whose scope in Table 8.3 includes `request` ([SC-RCP-075]); for a refusal cause
listed in Table 8.3.3 it is the code that table assigns ([SC-RCP-079]). A failure the
sending implementation observes after it has created the envelope carries a sender-scope
code instead (§5.3, after [MCPB-TOOL-008]). The cases this document names map as follows:

| Case | Rule here | Code | Neutral source |
|---|---|---|---|
| Tool call on a connection not bound to exactly one session (any of the four tools) | [MCPB-ATT-002] | `unauthorized` | §6.8 [SC-ID-161]; §7.3.2 [SC-DLV-060]; Table 8.3.3 |
| `tools/call` arguments fail the tool's `inputSchema` | [MCPB-TOOL-005] | `invalid-request` | Table 8.3 |
| `send` or `reply` whose `to` names a session whose presence, as the calling session is allowed to see it, is `unknown`: a session the server does not know, one for which it has accepted no presence record and so holds no capability declaration, or one the calling session is not authorized to discover | [MCPB-TOOL-016], [MCPB-TOOL-017], [MCPB-TOOL-019], [MCPB-TOOL-020], [MCPB-TOOL-021] | `unknown-destination` | §7.3.3 [SC-DLV-071], [SC-DLV-072], [SC-DLV-075], [SC-DLV-076]; Table 8.3.3, step 2 |
| `send` or `reply` whose `to` names a session whose presence, as the calling session is allowed to see it, is `unreachable` | [MCPB-TOOL-016] | `destination-unavailable` | §7.3.3 [SC-DLV-071], [SC-DLV-073]; Table 8.3.3, step 2 |
| Any other refusal of a `send` or `reply` before an envelope exists | [MCPB-TOOL-016] | the code Table 8.3.3 assigns, reported in the order of §8.3.3 ([SC-RCP-090]) | Table 8.3.3 |
| `send` or `reply` whose envelope was created but could not be passed to a transport | [MCPB-TOOL-010], [MCPB-TOOL-011] | `transport-failure` (state `failed`, sender scope) | Table 8.1; Table 8.3 |
| `tools/call` naming a tool outside §5.1 | [MCPB-TOOL-018] | none: an MCP protocol error, not an OAC error | MCP tools page, "Error Handling" |

The text content carries the code exactly as Table 8.3 spells it: lower case, hyphenated,
compared case-sensitively (`spec/session-channels.md` §8.3.1). Neither a role name such as
"authorization failure" nor a re-spelled code such as `Unauthorized` meets
[MCPB-TOOL-011].

A session the calling session is not authorized to discover is refused with the same code
as one that does not exist ([SC-DLV-075], [SC-DLV-076]). The code alone does not stop a
result from revealing the session: free text, `structuredContent` or `_meta` could still
say that it exists, what its presence is, or what it accepts.

[MCPB-TOOL-017] When an OAC server refuses a `send` or `reply` whose `to` names a session
the calling session is not authorized to discover, the `result` it returns MUST be the same
JSON value as the `result` it returns for a request that differs only in `to`, naming a
session id it holds no record of.

"The same JSON value" covers every member of `result`: `content`, `isError`,
`structuredContent` and `_meta`. Only the JSON-RPC `id`, which is outside `result`, may
differ. A member that carries a presence state, a capability, or a hint about authorization
for such a session therefore fails [MCPB-TOOL-017], whatever the code.

The `result` is not the only thing the caller sees. While it handles a `tools/call`, a
server can send other messages on the same connection: a log message
(`notifications/message`) when it declared `logging`, a progress notification carrying
the request's `progressToken`, or any other notification. A message **related to** a
`tools/call` is any message the server sends on the connection as part of handling that
call, from the moment it receives the request until it has sent the response.

[MCPB-TOOL-019] When an OAC server refuses a `send` or `reply` whose `to` names a session
the calling session is not authorized to discover, the sequence of messages related to that
call MUST be the same, message for message and in the same order, as the sequence it sends
for a request that differs only in `to`, naming a session id it holds no record of, when
the client sends the same messages on the connection while each call is handled.

The comparison holds the client's side fixed. A cancellation (`notifications/cancelled`) or
a `ping` that arrives while the call is handled can change what the server sends: a server
that honours a cancellation in one case but has already answered in the other produces a
difference that comes from how long each case took. That is a timing difference, which
[MCPB-TOOL-019] does not cover; it is left to `spec/security.md` §13.1 ([SEC-STG-005] and
its timing residual) with the other timing questions (reference implementation note below).

Requirement MCPB-TOOL-019 extends [MCPB-TOOL-017] from the `result` to everything else the
caller observes about the call. A log line, a progress message, or any other notification
that names, counts, or describes the addressed session fails it. [SC-DLV-076] keeps the
server from consulting the session's presence or declaration, but the server still has to
decide the caller's discovery authorization, so it knows that the session exists;
[MCPB-TOOL-017] and [MCPB-TOOL-019] keep that knowledge out of what the caller sees.

Requirements [MCPB-TOOL-017] and [MCPB-TOOL-019] compare one call, and their window ends
when the response is sent. A server can still reveal a session after that: a log message
sent later, a `list_changed` notification, or state that changes how later calls are
answered, such as a counter of refused sends that later throttles the caller. The next rule
has no time window.

An **observable action** on a connection is anything the server does that the client can
observe on it apart from timing: sending a message; closing or resetting the connection, a
stdio pipe, a stream or a transport session; setting a transport status code or header (for
example an HTTP status or `Retry-After`); and dropping or re-issuing a transport session id.
A **hidden session** of a connection is a session that the connection's bound session is not
authorized to discover. On a connection that is not bound to exactly one session, every
session is a hidden session.

[MCPB-TOOL-021] An OAC server MUST NOT take, on any connection, an observable action whose
occurrence, content or order depends on whether a hidden session of that connection exists,
or on that session's presence state or capability declaration.

Requirement MCPB-TOOL-021 covers every observable action for as long as the connection
lasts:

- messages at any time, including log messages, progress notifications and `list_changed`
  notifications, and the responses to every later call;
- closing or resetting the connection, a stream or a transport session, and dropping or
  re-issuing a transport session id;
- transport status codes and headers, such as an HTTP `429` or a `Retry-After` value;
- state the server keeps between calls: rate limiting, throttling, refusal counters or any
  other behaviour that earlier calls naming a hidden session influence differently from
  earlier calls naming a session id the server holds no record of.

Because every session is hidden on an unbound connection, nothing the server does on a
connection before it is paired names or reveals any session, even though [MCPB-ATT-002]
already refuses that connection's tool calls. The rule does not restrict what the server
does about sessions the bound session is authorized to discover. Timing is outside it, as
the reference implementation note below explains.

*Dated note, 2026-10-08 (#69, binding revision 0.2): a server that pairs Codex connections
(§4.5) meets [MCPB-TOOL-017], [MCPB-TOOL-019] and [MCPB-TOOL-021] as written. The pairing
value in its refusals is the same for every call on one connection during one pairing window
([MCPB-ATT-025]), so it depends on the connection and the time, never on `to` or on any
session. Two requests that differ only in `to` get the same `result`; the meaning of these
three requirements does not change.*

Discovery grants are not symmetric: a session can be authorized to discover and send to the
bound session while the bound session is not authorized to discover it. [MCPB-TOOL-021]
therefore does not apply to two kinds of action, even when they depend on a hidden session:

1. **Inbound hand-off.** The hand-off, to the bound session, of an envelope addressed to
   that session, through the provider-native surface (§2.3, §8), including the provenance
   the envelope carries (§6). The hidden session revealed itself by sending.
2. **Outcomes of the bound session's own sends.** The result of a `send` or `reply` that
   created an envelope (§5.3), and any later delivery state or receipt for an envelope the
   bound session sent (`spec/session-channels.md` §8.1), even if the addressed session
   has become hidden since. The bound session named that session in a send that passed
   authorization when it was made.

Neither exception lets the server reveal anything else about the hidden session: its
presence state, its capability declaration, or its other traffic stay under
[MCPB-TOOL-021].

**Replies to a hidden sender.** Whether the bound session may reply to a message from a
session it is not authorized to discover is governed by `spec/security.md` §9.5 (reply
rights, [SEC-AUZ-014] to [SEC-AUZ-016]) together with `spec/session-channels.md`
[SC-DLV-075]. This binding adds no refusal of its own on that question: none of its rules,
[MCPB-TOOL-016], [MCPB-TOOL-017], [MCPB-TOOL-019], [MCPB-TOOL-020] and [MCPB-TOOL-021]
included, refuses or hides a `reply` that those documents authorize.

[MCPB-TOOL-020] The `result` of a `send` or `reply` refused with `unknown-destination`,
and every message related to that call, MUST NOT contain the value of the call's `to`
argument, or any other value taken from or derived from the addressed session.

With [MCPB-TOOL-020], the two requests that [MCPB-TOOL-017] and [MCPB-TOOL-019] compare
produce no value that depends on `to`. The comparison is then plain equality, with no
substitution of the `to` value, and an echoed id cannot carry a re-spelled or annotated
form of it that reveals more than the caller sent.

> **Reference implementation note:** the reference implementation builds the refusal for a
> session id it holds no record of and the refusal for a session the caller is not
> authorized to discover through one code path, which takes no input about the addressed
> session. That keeps serialization details outside JSON-value equality, such as member
> order and whitespace, from differing between the two cases. Timing is a side channel the
> fixtures above cannot test: answering one case faster than the other still reveals the
> session, directly or through whether a cancellation arrives before the response. Timing
> is left to `spec/security.md` §13.1 ([SEC-STG-005] and its timing residual); that
> document also owns discovery authorization (§9.4).

*Dated note, 2026-10-03 (#262): an earlier draft of this change mapped a `send` to an
unknown session to `unsupported-capability`, because E4 (#44) placed the
no-declaration cause ([SC-ID-086]) before any presence check. E3 (#43, PR #263) folded that
cause into a presence step, step 2 of the §8.3.3 order, so the case now carries
`unknown-destination`, as `spec/session-channels.md` §8.3.1 names it for this binding. The
conflict is resolved in the neutral spec, and this binding follows it.*

[MCPB-TOOL-012] An OAC server MUST NOT return an OAC taxonomy
error as a JSON-RPC error object.

[MCPB-TOOL-018] An OAC server MUST answer a `tools/call` that names a tool outside §5.1
with a JSON-RPC protocol error, not with a tool execution error carrying an OAC code.

MCP lists "Unknown tools" among the protocol errors (MCP tools page, "Error Handling",
quoted above). A call to a tool the server does not offer is therefore an MCP error, not an
OAC one, and [MCPB-TOOL-010] and [MCPB-TOOL-012] do not apply to it. `invalid-request` is
returned only for a call to one of the four tools whose arguments fail its `inputSchema`
([MCPB-TOOL-005]).

[MCPB-TOOL-013] An OAC server MUST NOT emit a JSON-RPC error code
in the range `-32020` to `-32099` that the MCP specification does not define.

Requirement MCPB-TOOL-013 repeats the MCP `2026-07-28` reservation of that range ("Implementations **MUST
NOT** emit any code from this sub-range that is not defined by this specification", MCP
basic page, "Error Codes", retrieved 2026-10-03).

### 5.5 `list_sessions` and discovery

`list_sessions` is the MCP form of a discovery request (`spec/session-channels.md` §7.3).
The requester is the session bound to the calling connection (§4.4). Which sessions the
result lists, and the scoping applied to it, belong to §7.3 of that document
([SC-DLV-060] to [SC-DLV-067]) and to `spec/security.md`; this binding adds none of it.

- On a connection that is not bound to exactly one session, the call is refused with
  `unauthorized` ([MCPB-ATT-002]). That is the refusal [SC-DLV-060] gives a discovery
  request on an attachment not bound to exactly one session.
- The descriptors are session descriptors (`spec/session-channels.md` §6.3). They carry
  the session id and capability declaration, and never a working directory
  ([SC-ID-045]) or a harness-native identifier ([SC-ID-006]).
- The result lists exactly the `online` sessions the calling session is authorized to
  discover, each once ([SC-DLV-061] to [SC-DLV-064]). Each descriptor is the one in the
  latest announcement the implementation accepted for the session or, for a session of
  its own, the descriptor it currently announces ([SC-DLV-065]).
- The OAC server is part of the implementation that holds the calling session's binding.
  In v0.1, presence and discovery stay within one implementation
  (`spec/session-channels.md` §7.3.2, dated note), so that implementation applies the
  discovery authorization, including working-directory scoping, for every session it
  lists ([SC-DLV-067]). The requester is the session bound to the connection (§4.4),
  never a session id that the call's arguments or `_meta` carry.

[MCPB-TOOL-014] An OAC server MUST answer a `list_sessions` call on a bound connection
with the discovery result that `spec/session-channels.md` §7.3 defines for the session
bound to that connection as the requester.

[MCPB-TOOL-015] A successful `list_sessions` result MUST include a `text` content block
whose text is a serialized JSON object with a member `sessions` whose value is the
discovery result, an array of session descriptors.

Requirement MCPB-TOOL-015 fixes the form of the session list that [MCPB-TOOL-006] requires
in `text`, so that a model or client reads the same structure on every harness and era. An
empty `sessions` array is a valid result. The text is an object rather than a bare array
because MCP's `structuredContent` is an object: a server that also sends
`structuredContent` ([MCPB-TOOL-007]) sends that same `{"sessions": [...]}` object, so the
text block is its serialization, as MCP recommends.

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
§8.1 (delivery states, §8.1.2; receipts, §8.1.4). This binding defines none of them.

> **Reference implementation note:** the expected members are the message id, the
> conversation and correlation ids, the delivery state, and the spec revision. G4's spike
> carried only debugging members (`served_by_pid`, `surface`), which are not part of this
> binding.

### 6.3 What `_meta` provenance is not

[MCPB-META-005] A receiver MUST NOT use a value read from `_meta`
as evidence of a message's authenticity or sender.

Authenticity comes from the envelope signature (`spec/security.md` §6; C5 §2-§6).
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
  so the strongest delivery state an OAC server can report for it is `handed-to-harness`,
  which on a surface that returns no response means only that the write completed
  (C5 §9; `spec/session-channels.md` §8.1.2, §8.1.3).
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

*Dated note, 2026-10-04 (#275): MCP `2026-07-28` leaves "recognized modern error" open. It
gives one example only: "a recognized modern JSON-RPC error (such as
`UnsupportedProtocolVersionError`) identifies a modern server" (MCP versioning page,
"Backward Compatibility with Initialization-Based Versions", retrieved 2026-10-04). The
fixtures of [MCPB-CLD-003] therefore decide three cases only: a result and `-32022`
(`UnsupportedProtocolVersionError`) are nonconformant, and `-32601` (the code G4's channel
server returned, after which Claude Code fell back) is conformant. The reference runner
treats any other error code as undecided.*

The 2026-07-28 schema still defines `experimental` on both `ClientCapabilities` and
`ServerCapabilities` (schema at commit `271ecc9`, lines 720 and 797, cited in §4.3), so
requirement MCPB-CLD-001 restricts where the capability is declared, not whether the member exists.

> **Reference implementation note:** the Claude adapter's shim is a stdio process
> spawned by Claude Code (C2 §1). It therefore runs as the legacy-only exception in [MCPB-CLD-003],
> and it serves the §5 tools over the same legacy connection.

**Steering: the exception of `spec/security.md` [SEC-AUZ-022], invoked.** Channel input
sent while Claude Code is running a turn **does join that turn**. In G1 Box C, two inputs
sent during a running turn were taken in at two later tool-call boundaries of that same
turn, in order (`docs/planning/gates/G1-result.md`, criterion 3, 2026-09-28, Claude Code
`v2.1.283`). This binding nevertheless classifies `notifications/claude/channel` as not a
steering operation, under the exception, on this evidence:

- **No holding hand-off.** The channel capability defines one inbound path, the
  `notifications/claude/channel` notification; Claude Code documents no other operation
  that holds channel input for a turn of its own (`oac-claude-channels` §1;
  PLANNING-PROMPT.md §3.1, retrieved 2026-09-15).
- **The harness picks the boundary.** Claude Code's documentation says channel events queue
  into the session and are processed in order, delivered together on the next turn when
  Claude is busy (`channels-reference.md`; `docs/planning/REVERIFICATION-B2.md`, "§3.1
  re-check", row 5, HOLDS, retrieved 2026-10-02). G1 instead saw them taken in at tool-call
  boundaries of the running turn; that per-boundary pattern is UNVERIFIED as a guarantee
  (`docs/planning/v0.1/11-risks.md` row 49). Either way Claude Code decides where the input
  enters; the sender decides only when to send.

The accepted consequence is stated in `spec/security.md` §13, steering row. The Codex
hand-off operations are classified in §8.2.1.

### 8.2 Codex — tool path (MCP: supported; app-server inbound: experimental)

- **Surface labels:** `codex mcp add` external MCP server registration, supported
  (PLANNING-PROMPT.md §3.2; C6 §9). App-server live inject, experimental (per-method
  gating) (`oac-codex-appserver`). Pin: Codex floating, minimum `0.154.0`, last tested
  `0.159.3` (`docs/planning/STATUS.md` "Pins").
- **Inbound:** not MCP. The Codex adapter uses the app-server methods named in §2.3.
  `codex mcp-server` is deleted and is not a path (C6 §9).
- **Outbound:** Codex calls the four tools of §5 on an OAC server registered with
  `codex mcp add` (C2 §6). A stdio connection is bound by the issued-value pairing of
  §4.5, and each call on it is served only once Codex's own report confirms it. Calls on an
  unbound connection, and every call over an HTTP registration, are refused under
  [MCPB-ATT-002].
- **Subscription:** the Codex adapter receives a thread's notifications, the reveals and
  confirmations of §4.5 among them, on its carrier after subscribing to the thread (§4.5.1,
  fact C6).
- **Era:** legacy by default, modern only behind `mcp_2026_07_28` and only on HTTP
  registrations in the one run observed (G4 facts 7 and 8).

[MCPB-CDX-001] An OAC server MUST NOT send any notification to a
Codex MCP client as a way of delivering an OAC message.

Requirement MCPB-CDX-001 holds on both eras: Codex has no MCP surface that turns a notification into session
input, so such a notification could only mislead ([MCPB-DLV-001]). G4 recorded one such push with
no consumer (line 56).

#### 8.2.1 Inbound hand-off: queue only, no steering, no setting overrides

This subsection is the Codex form of `spec/security.md` [SEC-AUZ-022] and [SEC-AUZ-025] to
[SEC-AUZ-027]. It records the operator decision on #224 (2026-10-02,
https://github.com/RossGraeber/OAC/issues/224#issuecomment-5958414229): OAC delivery must not
steer; delivery uses the queue; the race-free, queue-based form is the one chosen; and no
override fields are sent.

**Evidence.** All source citations are to `openai/codex` tag `rust-v0.160.0`, commit
`a956835d020762cb2b570053af06f643a11c0ecc`, the installed Codex `0.160.0`, retrieved
2026-10-02, as recorded in the #224 step-1 findings
(https://github.com/RossGraeber/OAC/issues/224#issuecomment-5956477165). There, `B` is
`https://github.com/openai/codex/blob/a956835d020762cb2b570053af06f643a11c0ecc/codex-rs`.

- **A `turn/start` during a regular turn steers it (C1).** The handler calls
  `start_or_steer_turn` and maps `TurnInputSubmission::Steered` to the active turn's id
  (`B/app-server/src/request_processors/turn_processor.rs#L651-L684`). Core tries the steer
  first and starts a turn only on `NoActiveTurn`
  (`B/core/src/session/turn_input.rs#L276-L373`), through the same `steer_input` that
  `turn/steer` uses (`turn_input.rs#L625-L719`). The upstream test
  `turn_start_steers_active_turn_and_returns_active_turn_id` asserts it
  (`B/app-server/tests/suite/v2/turn_start.rs#L648-L779`). The first-party app-server
  documentation (https://developers.openai.com/codex/app-server, unversioned, retrieved
  2026-10-02) describes steering only under `turn/steer` and says nothing about a
  `turn/start` sent during a turn.
- **Observed live once.** In the C13 E1 run of 2026-10-02 (Codex `0.160.0`, run
  `20261002T161612Z`), arm 0's first X2 delivery sent `turn/start`, got back the id of the
  marker turn that was still `inProgress` (`01a0fd67-566a-7173-a60e-2fc073ce3896`), and its
  input joined that turn (the run record that `docs/planning/gates/G5-result.md` cites for
  the Codex leg, "Findings and UNVERIFIED", transcript L58 and L66; the #224 operator
  decision comment).
- **A steering `turn/start` still applies its setting overrides to the thread (C3).**
  `approvalPolicy`, `sandboxPolicy`, `permissions`, `model`, `cwd` and the other overrides are
  built for every `turn/start` (`turn_processor.rs#L630-L649`) and, after a steer, applied to
  the thread's later turns (`turn_input.rs#L193-L203`).
- **`thread/queue/add` never steers (C4).** Its handler only enqueues
  (`B/app-server/src/request_processors/thread_queue_processor.rs#L77-L95`). Queued items
  are dispatched only through `start_turn_if_idle`, which checks idleness and reserves the
  turn under one lock and cannot steer (`B/core/src/codex_thread.rs#L374-L396`;
  `turn_input.rs#L432-L437`). After enqueueing, `wake_if_loaded` dispatches at once on a
  loaded thread that is idle, **unless the thread's agent status is `Interrupted`**, that
  is, its last turn ended interrupted (`B/ext/queue/src/service.rs#L265-L280`, `#L472-L482`,
  the check at `#L477`). On a busy thread, or an idle one whose last turn was interrupted,
  the item waits for a later idle that an interrupt did not cause (`#L405-L470`,
  `#L549-L566`). The G2 `busyqueue` step showed the busy case live
  (`docs/planning/gates/G2-result.md`); the interrupted case is from source only.
- **Checking for a running turn first does not help (C6).** `thread/status/changed`,
  `turn/started` and `Thread.status` all tell a client about a turn, but none is atomic
  with a later `turn/start`. The steer-or-start choice is made under Core's `active_turn`
  lock, which no client can hold. A turn can start between the check and the call: from the
  TUI's user, from the queue, or from another client of the daemon.
- **The add request has no override members.** `ThreadQueueAddParams` has exactly
  `threadId`, `input` and `clientUserMessageId`
  (`B/app-server-protocol/src/protocol/v2/thread.rs#L910-L914`, retrieved 2026-10-04).

**Classification.** Under `spec/security.md` [SEC-AUZ-022], `turn/steer` and `turn/start`
are steering operations: the first by its documentation, the second by source and by the
live observation above. `thread/queue/add` is the holding hand-off of [SEC-AUZ-025] and is
not a steering operation.

**Which form.** Under `spec/security.md` [SEC-AUZ-026], a Codex thread is always possibly
running a turn, because the app-server offers no operation that both checks for a running
turn and hands off. A "check, then `turn/start` when idle" rule would leave a race in which
the `turn/start` steers. This binding therefore uses the queue for every delivery, not only
when a turn is known to be running. No check-then-call race is left on this path.

[MCPB-CDX-002] The Codex adapter MUST hand off every OAC message to a Codex thread through
`thread/queue/add`.

[MCPB-CDX-003] The Codex adapter MUST NOT hand off an OAC message through `turn/start`.

[MCPB-CDX-004] The Codex adapter MUST NOT hand off an OAC message through `turn/steer`.

[MCPB-CDX-005] An app-server request that the Codex adapter sends to hand off an OAC message
MUST NOT carry a member that sets a thread or turn setting, such as `approvalPolicy`,
`sandboxPolicy`, `permissions`, `model` or `cwd`.

At `0.160.0` a `thread/queue/add` request satisfies [MCPB-CDX-005] when it carries only
`threadId`, `input` and `clientUserMessageId`. What the app-server does with an extra member
in a `thread/queue/add` request is UNVERIFIED (no `deny_unknown_fields` on the struct, so it
is probably ignored; not exercised). [MCPB-CDX-005] forbids sending one either way.

[MCPB-CDX-006] A `thread/resume` request that a Codex adapter pairing under §4.5 sends to
subscribe to a thread MUST NOT carry a member that sets a thread or turn setting, such as
`model`, `cwd`, `approvalPolicy`, `sandbox`, `permissions` or `config`.

`thread/resume` takes those overrides (§4.5.1, fact C7). Whether the app-server applies them
to a thread that is already loaded is UNVERIFIED (§10); [MCPB-CDX-006] forbids sending them
either way. A request carrying only `threadId`, or `threadId` and `excludeTurns`, satisfies
it; G2 subscribed with exactly those two (`docs/planning/gates/fixtures/g2-codex-inject/transcript.jsonl`
line 51). It is scoped to an adapter that pairs, so that an adapter conformant to revision
0.1, which does not, stays conformant (`spec/session-channels.md` §5.3, item 11).

The `turn/steer` and `turn/start` entries in §2.3 stay because they are app-server inbound
methods. [MCPB-CDX-003] and [MCPB-CDX-004] decide that delivery never uses them, whatever
the thread's state and whatever an operator configures: this revision has no setting that
enables steering (`spec/security.md` [SEC-AUZ-022]). Any future use of `turn/steer` would
need its own decision and task G7's authorization gate (#68) (`oac-security-work`).

**Surface label and shim.** `thread/queue/add` is experimental: it carries
`#[experimental("thread/queue/add")]`
(`B/app-server-protocol/src/protocol/common.rs#L623-L628`), is absent from the default
schema, and needs `capabilities.experimentalApi`. Every call goes through the
version-pinned compatibility shim of backlog task G6, not from adapter code directly. Codex
is floating (minimum `0.154.0`); the facts above were read at `0.160.0`, and a later version
is re-checked as a follow-up, never as a gate (#216).

**Reporting a turned-away add.** When `thread/queue/add` returns a JSON-RPC error, the
adapter does not fall back to `turn/start` or `turn/steer` ([MCPB-CDX-003],
[MCPB-CDX-004], `spec/security.md` [SEC-AUZ-027]). It reports the outcome under
`spec/session-channels.md` [SC-DLV-008] or [SC-DLV-009]. At least four of the add's
refusals are known from source (`thread_queue_processor.rs`, retrieved 2026-10-04):

- an ephemeral thread, "ephemeral thread does not support queued submissions" (`#L261`);
- a host with no queue service, "user message queue is unavailable" (`#L246`);
- a subagent thread that does not accept direct input: a loaded multi-agent-v2 subagent,
  or an unloaded `ThreadSpawn` subagent (`ensure_direct_input_allowed`, `#L292-L309`,
  called at `#L83`);
- an archived thread (`#L282`).

None says the thread cannot take input now, so each is `handoff-failed` ([SC-DLV-009]).
Whether `thread/queue/add` has any refusal that means "not now" is UNVERIFIED; owner G7
(#68).

**Consequences of queue-only delivery, stated plainly.**

- **OAC cannot deliver at all** to an ephemeral thread, to a thread on a host without the
  queue service, to a subagent thread of either kind above, or to an archived thread. Every
  delivery to one ends `handoff-failed`. Before this subsection, the idle path used
  `turn/start`, which has none of these queue refusals.
- **Every Codex delivery depends on one experimental method.** There is no
  non-experimental path left: if `thread/queue/add` changes shape or is removed in a later
  Codex version, Codex inbound delivery stops until the G6 shim is updated
  (`docs/planning/v0.1/11-risks.md`, RISK-CODEX-EXPERIMENTAL).
- **The `additionalContext` anchor is never sent.** C6 §5.0 sends the
  `oac_provenance` anchor on `turn/start` only, and `thread/queue/add` has no such field.
  Codex delivery therefore carries the frame alone (C13 Option A, `spec/security.md`
  [SEC-PRV-011] is a `MAY`). G5 arm F, the frame alone, passed every required trial
  (`docs/planning/gates/G5-result.md`).

**What the harness then holds.** A successful add is a completed hand-off, reported as
`handed-to-harness` (`spec/session-channels.md` §8.1). The input is then held by Codex,
not by the receiver (`spec/session-channels.md` [SC-DLV-007]). Three consequences follow
from source (C5) and are runtime-UNVERIFIED, owner G7 (#68):

- **After an interrupt, nothing dispatches until a turn completes uninterrupted.** An item
  already queued when a turn ends interrupted is not dispatched (`service.rs#L549-L566`).
  An add made later, to a thread that is idle but whose last turn was interrupted, also
  waits: `wake_if_loaded` skips a thread whose agent status is `Interrupted` (`#L477`). A
  TUI user who interrupts a turn and walks away therefore stalls every later OAC delivery
  to that thread, each reported as `handed-to-harness`, until some turn completes without
  an interrupt.
- The queue is durable and shared: any client of the daemon can list, reorder, update or
  delete queued items through `thread/queue/{list,update,delete,reorder}`. A handed-off
  item can therefore be changed or removed before it runs.
- An add to a thread that is not loaded stays queued until the thread is loaded and idle.
  A live OAC session's thread is loaded, so this needs a thread that was unloaded after
  presence was announced.

None of the three makes OAC hold an envelope: each is the harness's own queue. They are what
`handed-to-harness` leaves open for Codex: §8.1 states that the state proves only that the
input call completed.

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
   *Dated note, 2026-10-08 (#69, binding revision 0.2): a Codex stdio connection is now
   bound by the pairing of §4.5, on whichever era it negotiates; HTTP registrations stay
   refused on both eras.*
4. **What stays open is verification, not conflict.** Two items remain, each with an
   owner, and neither can reopen the design question in point 1:
   - an `rmcp`-based server registering as a legacy-era channel against live Claude Code
     (UNVERIFIED; issue #65, backlog G4);
   - Codex's default client era, a provider behavior OAC cannot change
     (`docs/planning/v0.1/11-risks.md` row 41, RISK-G4).

   Two new open items come out of this resolution, both UNVERIFIED (§4.4, §10): whether a
   documented per-request session signal exists that OAC can bind to a paired session,
   and whether one Codex legacy-era connection carries calls from several threads (#69).
   *Dated note, 2026-10-08 (#69): the first is answered, no (§4.4); the second is answered
   by source, no (§4.5.1, fact C2), and stays UNVERIFIED at runtime (§10).*

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
| A documented per-request session signal exists that OAC can bind to a paired session (Codex's `x-codex-turn-metadata` carries `session_id`/`thread_id`/`turn_id` but is undocumented and client-asserted) | **closed, 2026-10-08 (#69): none exists.** The app-server documentation names no `_meta` member Codex sends to an MCP server; the source at `rust-v0.161.0` adds `callId`, `threadId`, `sessionId` and `x-codex-turn-metadata`, all undocumented and client-asserted (§4.4; §4.5.1, fact C1) | removed from the `docs/planning/STATUS.md` open list in binding revision 0.2's change; `11-risks.md` RISK-G4 dated status |
| One Codex legacy-era MCP connection carries calls from several threads (a thread id is sent per call) | source at `rust-v0.161.0` says no: the MCP runtime is per thread (§4.5.1, fact C2); runtime UNVERIFIED; owner #69. §4.5 does not depend on it for attribution | `docs/planning/STATUS.md` entry reworded in binding revision 0.2's change; `11-risks.md` RISK-G4 |
| Codex emits a call's `item/started` before the call and its `item/completed`, with the returned result, before the model sees the result (§4.5.1, fact C5) | source only at `rust-v0.161.0`; runtime UNVERIFIED (new, §4.5); owner #69 | added to `docs/planning/STATUS.md` and `11-risks.md` RISK-G4 in binding revision 0.2's change |
| A carrier subscribed to a TUI-hosted thread with `thread/resume` receives that thread's `mcpToolCall` items (§4.5.1, fact C6) | other item types recorded (G2, `0.154.0`); `mcpToolCall` UNVERIFIED (new, §4.5); owner #69 | added to `docs/planning/STATUS.md` and `11-risks.md` RISK-G4 in binding revision 0.2's change |
| Whether the app-server applies `thread/resume` setting overrides to a loaded thread ([MCPB-CDX-006]) | UNVERIFIED (new, §8.2.1); owner G7 (#68) | added to `docs/planning/STATUS.md` in binding revision 0.2's change |
| Which operating-system call yields the peer process of the carrier and of a Codex connection, and their ancestry, on each platform (§4.5.3) | UNVERIFIED; the same item as the Claude pairing's (`spec/session-channels.md` §6.7.2); owner G9 (#70) | already in `docs/planning/STATUS.md`; `11-risks.md` RISK-LOCAL-IPC |
| A call with no arguments is reported with `arguments: null` and sent without an `arguments` member, so the two match (§4.5.2; §4.5.1 fact C9) | source only at `rust-v0.161.0`; runtime UNVERIFIED (new, §4.5); availability only: if a version sends `{}` instead, such calls are refused for want of a confirmation; owner G8 (#69) | added to `docs/planning/STATUS.md` and `11-risks.md` row 75 in binding revision 0.2's change |
| The F9 fake app-server emits `mcpToolCall` items, as every `TODO(fixture)` of §4.5 needs | not yet built; owners G8 (#69) and F9 (#58) | `docs/planning/STATUS.md`; `11-risks.md` row 74 |
| A legacy client other than Codex `0.157.1` ignores an `extensions` member in an `initialize` result | UNVERIFIED (new, [MCPB-ERA-008]) | added to `docs/planning/STATUS.md` in this change |
| A Codex `turn/start` sent during a regular turn steers it; `thread/queue/add` never does; a steering `turn/start` applies its setting overrides (§8.2.1) | verified from source at `rust-v0.160.0` and the upstream test; observed live once at `0.160.0` (E1 run) | #224 step-1 findings; `docs/planning/STATUS.md` (the S10 item closes in #274's change) |
| Which `thread/queue/add` errors, if any, mean "not now" ([SC-DLV-008]) | UNVERIFIED (#274); owner G7 (#68) | added to `docs/planning/STATUS.md` in #274's change |
| Runtime behaviour of the queue caveats of §8.2.1: no dispatch after an interrupted turn, including adds made later to an idle thread, until a turn completes uninterrupted; edits by other daemon clients; unloaded threads; and what the app-server does with an extra member in a `thread/queue/add` request | UNVERIFIED (#274; source only); owner G7 (#68) | added to `docs/planning/STATUS.md` in #274's change |

## 11. Boundary self-check (`oac-boundaries`)

- **Delivery.** No section makes MCP the delivery mechanism. §2 forbids the claim, and
  §8 names each provider's native path.
- **Polling.** [MCPB-DLV-002] forbids an MCP polling surface while active inbound is claimed.
- **Credentials.** `whoami` never returns a credential ([MCPB-TOOL-009]). Nothing here reads or
  reuses a provider credential.
- **Private RPCs.** Every provider surface named is documented and labelled (§8). No
  scraping, rollout file or deleted `codex mcp-server` path is used. The Codex pairing
  (§4.5) reads only documented, stable `item/started` and `item/completed` notifications on
  the adapter's own app-server connection; the undocumented `_meta` members Codex sends are
  never read for attribution (§4.4).
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
`EXT` (§3), `ERA` (§4.3), `ATT` (§4.4, §4.5), `TOOL` (§5), `META` (§6), `FBK` (§7), `CLD`
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
  stdio process or HTTP session has completed `initialize`),
  `supported_legacy_revisions`, `addressed` and `unknown_session_result`.
  - `codex_pairing`, `true` when the connection is a Codex connection whose server pairs
    under §4.5. Absent means `false`.
  - `issued_pairing_values`, an array of the pairing values (§4.5.2) that the server issued
    before this exchange, on another connection or in a pairing window that has ended.
    Absent means none.
  - `open_pairing_value`, the pairing value of the connection's open pairing window, when
    one is open before this exchange. Absent means none is open.
  - `pairing_possible`, `false` when the server cannot pair the connection
    ([MCPB-ATT-022]). Absent means `true`.
  - `addressed` describes the session a `send` or `reply` names in `to`. It is an
    object with `presence`, the presence state that the OAC server's implementation
    holds for that session as its observer (`online`, `unreachable` or `unknown`,
    `spec/session-channels.md` §7.2.1), and `discoverable`, whether the calling session
    is authorized to discover it (§7.3). The presence the calling session is allowed to
    see is derived from the pair: `unknown` when `discoverable` is `false`
    ([SC-DLV-075]), and `presence` otherwise. That derived value decides the code at
    Table 8.3.3, step 2.
  - `unknown_session_result` is the `result` object the same server returns for a request
    that differs only in `to`, naming a session id it holds no record of
    ([MCPB-TOOL-017]).
  - `unknown_session_messages` is the array of messages related to that same request
    ([MCPB-TOOL-019]); an empty array when there are none.
  - `unknown_session_subsequent_messages` is the `subsequent_messages` array the
    connection carries in that same unknown-id case, with the same client entries
    ([MCPB-TOOL-021]); an empty array when there are none.
  - `server_message_kind` says that `server_message` is one of the actions that
    [MCPB-TOOL-021] does not apply to (§5.4): `handoff` (the hand-off of an envelope
    addressed to the bound session, from the session `addressed` describes) or
    `own-send-outcome` (a delivery state or receipt for an envelope the bound session
    sent to that session). Absent means neither.
- `input` is `mcp_exchange`: an object with these members, in this order:
  - `request`, optional: the client's JSON-RPC message;
  - `related_messages`, optional: an array of the other messages the server sends on the
    connection that are related to the request, in order (§5.4); absent means none;
  - `server_message`, required: the server's response or notification;
  - `subsequent_messages`, optional: an array of what happens on the connection after
    `server_message`, in order ([MCPB-TOOL-021]); absent means nothing. Each entry is an
    object with `from` (`client` or `server`) and one of these shapes:
    - `message`: a JSON-RPC message (a later notification, call or response). A server
      entry over HTTP may also hold `http`, the transport envelope that carried it;
    - `http` alone (server only): an HTTP response with no JSON-RPC body. `http` is an
      object with `status` (an integer) and optionally `headers` (an object of header
      names to string values);
    - `close` alone (server only): the server closed or reset something on the
      connection. `close` is an object with `scope`: `connection` (the whole connection
      or stdio pipe), `stream` (one stream, such as an SSE stream) or `session` (the
      transport session, including dropping its session id).

    Re-issuing a transport session id is expressed by the action that forces it. Over
    Streamable HTTP at `2025-11-25`, a server assigns a session id only "at initialization
    time, by including it in an `MCP-Session-Id` header on the HTTP response containing the
    `InitializeResult`"; after it terminates a session it "MUST respond to requests
    containing that session ID with HTTP 404 Not Found", and the client then "MUST start a
    new session" (https://modelcontextprotocol.io/specification/2025-11-25/basic/transports,
    "Session Management", retrieved 2026-10-03). A re-issue therefore starts with a server
    `http` entry with `status` 404, or a `close` entry with `scope` `session`, in one case
    and not the other (E8, #48).
- `expected.result` is `conformant` or `nonconformant`: whether the server's messages and actions in
  `input` meet the requirement, given `request`, the client's entries in
  `subsequent_messages` and `context`. The server's messages and actions are `server_message` and,
  where the requirement covers them, `related_messages` ([MCPB-TOOL-019],
  [MCPB-TOOL-020]) and the server's entries in `subsequent_messages` ([MCPB-TOOL-021]).

Fixtures live in `tests/protocol/mcpb-<area>/`, named as `spec/session-channels.md` §3.3 says. A requirement that a
single exchange cannot decide (a whole session, a comparison across eras, or a value a
later task defines) stays `TODO(fixture)`, with the planned input and expected outcome.

### 12.3 Index

| Id | Keyword | Fixtures |
|---|---|---|
| MCPB-DLV-001 | MUST NOT | TODO(fixture), owner G9 (#70, the shim's MCP surface) with G5 (#66): the requirement also covers what a server documents or reports, and the planned check spans the whole advertised surface (`server/discover` and `initialize` results, `tools/list`, `resources/list`, `prompts/list`), so no single exchange decides it. Planned: every capability key is one the MCP revision in use defines (for example `tools`, `logging`) or `extensions` (holding only the §3 identifier) or, on a legacy channel-path connection only, `experimental["claude/channel"]`; no tool beyond §5.1's four; no resource or prompt is listed |
| MCPB-DLV-002 | MUST NOT | TODO(fixture), owner F10 (#59, the no-polling assertion of the adapter contract suite) with G9 (#70): "expected to read repeatedly" and the session's active-inbound claim are not in an `oac-mcpb-fixture/1` `context`. Planned: `resources/list`, `prompts/list`, `subscriptions/listen` while active inbound is declared → no inbox-shaped resource, prompt or subscription offered |
| MCPB-EXT-001 | MUST | `tests/protocol/mcpb-ext/MCPB-EXT-001.p01-initialize-exact-identifier.json`, `tests/protocol/mcpb-ext/MCPB-EXT-001.n01-identifier-wrong-case.json` |
| MCPB-EXT-002 | MUST NOT | TODO(fixture), owner G9 (#70): no MCP-observable surface in this revision, because the settings object of §3.6 is `{}` and carries no version. Planned once a binding revision defines a version member there: `initialize` or `server/discover` result declaring the identifier with a major version other than 0 (negative) → nonconformant |
| MCPB-EXT-003 | MUST | `tests/protocol/mcpb-ext/MCPB-EXT-003.n01-settings-not-object.json` |
| MCPB-EXT-004 | MUST | `tests/protocol/mcpb-ext/MCPB-EXT-004.p01-unknown-setting-ignored.json`, `tests/protocol/mcpb-ext/MCPB-EXT-004.n01-unknown-setting-refused.json` |
| MCPB-ERA-001 | MUST | `tests/protocol/mcpb-era/MCPB-ERA-001.p01-legacy-initialize-answered.json`, `tests/protocol/mcpb-era/MCPB-ERA-001.n01-legacy-initialize-refused.json` |
| MCPB-ERA-002 | MUST | `tests/protocol/mcpb-era/MCPB-ERA-002.p01-2025-11-25-answered-same.json`, `tests/protocol/mcpb-era/MCPB-ERA-002.n01-2025-11-25-downgraded.json` |
| MCPB-ERA-003 | MUST | `tests/protocol/mcpb-era/MCPB-ERA-003.p01-older-legacy-answered-2025-11-25.json`, `tests/protocol/mcpb-era/MCPB-ERA-003.n01-echoes-unsupported-revision.json` |
| MCPB-ERA-004 | MUST | `tests/protocol/mcpb-era/MCPB-ERA-004.p01-modern-tools-list-served.json`, `tests/protocol/mcpb-era/MCPB-ERA-004.n01-modern-tools-list-unsupported-version.json` |
| MCPB-ERA-005 | MUST | `tests/protocol/mcpb-era/MCPB-ERA-005.p01-discover-lists-both-eras.json`, `tests/protocol/mcpb-era/MCPB-ERA-005.n01-discover-modern-only.json` |
| MCPB-ERA-006 | MAY | none (not a `MUST`) |
| MCPB-ERA-007 | MUST | TODO(fixture), owner G9 (#70, the shim serving both eras): a comparison across eras, which no single exchange holds. Planned: `tools/list` on a legacy-era connection and as a modern-era request → identical names, `inputSchema` and declared result shape |
| MCPB-ERA-008 | MUST | `tests/protocol/mcpb-era/MCPB-ERA-008.p01-initialize-declares-extension.json`, `tests/protocol/mcpb-era/MCPB-ERA-008.n01-initialize-no-extensions.json` |
| MCPB-ERA-009 | MUST | `tests/protocol/mcpb-era/MCPB-ERA-009.p01-discover-declares-extension.json`, `tests/protocol/mcpb-era/MCPB-ERA-009.n01-discover-no-extensions.json` |
| MCPB-ERA-010 | SHOULD | none (not a `MUST`) |
| MCPB-ERA-011 | MUST | `tests/protocol/mcpb-era/MCPB-ERA-011.p01-uninitialized-missing-version-rejected.json`, `tests/protocol/mcpb-era/MCPB-ERA-011.p02-legacy-session-request-served.json`, `tests/protocol/mcpb-era/MCPB-ERA-011.n01-uninitialized-missing-version-served.json` |
| MCPB-ATT-001 | MUST NOT | `tests/protocol/mcpb-att/MCPB-ATT-001.p01-unbound-whoami-refused.json`, `tests/protocol/mcpb-att/MCPB-ATT-001.n01-unbound-whoami-answered.json`, `tests/protocol/mcpb-att/MCPB-ATT-001.n02-unbound-send-served-by-connection.json` |
| MCPB-ATT-002 | MUST | `tests/protocol/mcpb-att/MCPB-ATT-002.p01-send-unbound-legacy-unauthorized.json`, `tests/protocol/mcpb-att/MCPB-ATT-002.p02-list-sessions-unbound-modern-unauthorized.json`, `tests/protocol/mcpb-att/MCPB-ATT-002.p03-whoami-unbound-legacy-unauthorized.json`, `tests/protocol/mcpb-att/MCPB-ATT-002.n01-unbound-send-served.json`, `tests/protocol/mcpb-att/MCPB-ATT-002.n02-unbound-wrong-code.json` |
| MCPB-ATT-003 | MAY | none (not a `MUST`) |
| MCPB-ATT-004 | MUST | `tests/protocol/mcpb-att/MCPB-ATT-004.p01-pairing-refusal-carries-value.json`, `tests/protocol/mcpb-att/MCPB-ATT-004.n01-value-only-in-structured-content.json` |
| MCPB-ATT-005 | MUST | `tests/protocol/mcpb-att/MCPB-ATT-005.p01-value-form.json`, `tests/protocol/mcpb-att/MCPB-ATT-005.n01-value-upper-case.json`, `tests/protocol/mcpb-att/MCPB-ATT-005.n02-value-too-short.json` |
| MCPB-ATT-006 | MUST NOT | `tests/protocol/mcpb-att/MCPB-ATT-006.p01-send-result-without-value.json`, `tests/protocol/mcpb-att/MCPB-ATT-006.n01-argument-echoes-issued-value.json` |
| MCPB-ATT-007 | MUST | TODO(fixture), owner G8 (#69) against the F9 fake (#58), with `mcpToolCall` items added to it: app-server traffic on the carrier, outside the `mcp-binding` stage (§12.2). Planned: a pairing refusal on connection C, then an `item/completed` for thread T whose `result.content` equals it (positive) → C bound to T; the same with a different value, a different `server`, different `arguments`, or a value issued on another connection (negative) → C unbound |
| MCPB-ATT-008 | MUST | TODO(fixture), owner G8 (#69) against the F9 fake: a reveal whose `threadId` is T while the refused call's `_meta` names thread U (`threadId`, `x-codex-turn-metadata`) → bound to T, never U |
| MCPB-ATT-009 | MUST NOT | TODO(fixture), owner G8 (#69) against the F9 fake: a reveal received after the pairing window → unbound, with a diagnostic |
| MCPB-ATT-010 | MUST NOT | TODO(fixture), owner G8 (#69) against the F9 fake: a second reveal of a paired value for the same thread → no second binding and no change |
| MCPB-ATT-011 | MUST | TODO(fixture), owner G8 (#69) against the F9 fake: a second reveal of a paired value naming another thread → the connection ends unbound |
| MCPB-ATT-012 | MUST | TODO(fixture), owner G8 (#69) against the F9 fake: the case of MCPB-ATT-011 → one finding recorded |
| MCPB-ATT-013 | MUST NOT | TODO(fixture), owner G8 (#69) against the F9 fake: a call on a bound connection with a matching `item/started` for the bound thread (positive, either order) → served; with none, with one for another thread, or with different `arguments` (negative) → not served |
| MCPB-ATT-014 | MUST NOT | TODO(fixture), owner G8 (#69) against the F9 fake: two identical calls on a bound connection and one matching `item/started` → one served, one refused |
| MCPB-ATT-015 | MUST | TODO(fixture), owner G8 (#69) against the F9 fake: no confirmation within the window → refused with `unauthorized`, carrying a new pairing value |
| MCPB-ATT-016 | MUST | TODO(fixture), owner G8 (#69) against the F9 fake: after the refusal of MCPB-ATT-015 a further call is refused until a reveal for the bound thread arrives, then served on its confirmation |
| MCPB-ATT-017 | SHOULD | none (not a `MUST`) |
| MCPB-ATT-018 | MUST NOT | TODO(fixture), owner G8 (#69) against the F9 fake (#58), with `mcpToolCall` items added to it: an `item/completed` or `item/started` whose `threadId` is a thread the carrier never subscribed to, or unsubscribed from, arriving on the carrier (negative) → neither a reveal nor a confirmation |
| MCPB-ATT-019 | MUST | TODO(fixture), owner G8 (#69) against the F9 fake: a reveal arriving 61 seconds after its window opened → not paired, whatever the configured length |
| MCPB-ATT-020 | MUST NOT | TODO(fixture), owner G8 (#69) against the F9 fake: a call whose `item/started` never arrives → refused no later than 10 seconds after it arrived |
| MCPB-ATT-021 | MUST NOT | TODO(fixture), owner G8 (#69) against the F9 fake: a call matching an `item/started` whose `item/completed` arrived, whose turn ended with `turn/completed`, or that arrived 601 seconds earlier (negative) → not served |
| MCPB-ATT-022 | MUST NOT | `tests/protocol/mcpb-att/MCPB-ATT-022.p01-plain-refusal-when-unpairable.json`, `tests/protocol/mcpb-att/MCPB-ATT-022.n01-pairing-value-when-unpairable.json` |
| MCPB-ATT-023 | MUST NOT | TODO(fixture), owner G8 (#69) against the F9 fake: four calls on one connection, each window ending without a reveal → the fourth refusal carries no pairing value |
| MCPB-ATT-024 | SHOULD NOT | none (not a `MUST`) |
| MCPB-ATT-025 | MUST | `tests/protocol/mcpb-att/MCPB-ATT-025.p01-window-value-repeated.json`, `tests/protocol/mcpb-att/MCPB-ATT-025.n01-new-value-in-open-window.json` |
| MCPB-TOOL-001 | MUST | `tests/protocol/mcpb-tool/MCPB-TOOL-001.p01-four-tools-listed.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-001.n01-whoami-missing.json` |
| MCPB-TOOL-002 | MUST | TODO(fixture), owner G9 (#70): a comparison across eras, as for MCPB-ERA-007. Planned: `tools/list` on each era → identical names, schemas and result shapes |
| MCPB-TOOL-003 | MUST | TODO(fixture), owner G9 (#70) with G5 (#66) and G8 (#69): a comparison across harnesses, which no single exchange holds. Planned: `tools/list` as two different clients → identical |
| MCPB-TOOL-004 | MUST | TODO(fixture), owner G5 (#66) with G8 (#69): this revision does not fix the full argument list of each tool (§5.1 names only `to`, `in_reply_to` and `content`), so a checker cannot decide that the property names match. Planned: each `inputSchema` → valid JSON Schema object; property names match the neutral spec |
| MCPB-TOOL-005 | MUST | `tests/protocol/mcpb-tool/MCPB-TOOL-005.p01-missing-to-invalid-request.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-005.n01-role-name-not-code.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-005.n02-missing-to-served.json` |
| MCPB-TOOL-006 | MUST | `tests/protocol/mcpb-tool/MCPB-TOOL-006.p01-whoami-values-in-text.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-006.n01-whoami-values-structured-only.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-006.n02-send-state-structured-only.json`; the message id of a `send` or `reply` result is not decided, because this revision fixes no text layout that identifies it: TODO(fixture), owner G5 (#66) with G8 (#69) |
| MCPB-TOOL-007 | SHOULD | none (not a `MUST`) |
| MCPB-TOOL-008 | MUST NOT | `tests/protocol/mcpb-tool/MCPB-TOOL-008.p01-send-accepted-by-adapter.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-008.p02-error-text-with-state-word.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-008.n01-send-handed-to-harness.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-008.n02-reply-duplicate-in-structured-content.json` |
| MCPB-TOOL-009 | MUST NOT | TODO(fixture), owner F11 (#60, security suite against the fakes) with G5 (#66): whether a value authenticates its holder depends on the credentials the caller holds, which no `oac-mcpb-fixture/1` `context` member states. Planned: `whoami` result, given the caller's keys and tokens → none of them present |
| MCPB-TOOL-010 | MUST | `tests/protocol/mcpb-tool/MCPB-TOOL-010.p01-refusal-as-tool-error.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-010.p02-transport-failure-as-tool-error.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-010.n01-refusal-as-jsonrpc-error.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-010.n02-transport-failure-not-error.json` (`.p02`, `.n02`: `transport-failure`, §5.3) |
| MCPB-TOOL-011 | MUST | `tests/protocol/mcpb-tool/MCPB-TOOL-011.p01-code-spelled-exactly.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-011.n01-code-recased.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-011.n02-no-code.json` |
| MCPB-TOOL-012 | MUST NOT | `tests/protocol/mcpb-tool/MCPB-TOOL-012.n01-unauthorized-as-jsonrpc-error.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-012.n02-invalid-request-as-jsonrpc-error.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-012.n03-unknown-destination-as-jsonrpc-error.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-012.n04-destination-unavailable-as-jsonrpc-error.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-012.n05-unsupported-version-as-jsonrpc-error.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-012.n06-unsupported-capability-as-jsonrpc-error.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-012.n07-unsupported-content-type-as-jsonrpc-error.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-012.n08-envelope-too-large-as-jsonrpc-error.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-012.n09-transport-failure-as-jsonrpc-error.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-012.n10-internal-error-as-jsonrpc-error.json` |
| MCPB-TOOL-013 | MUST NOT | `tests/protocol/mcpb-tool/MCPB-TOOL-013.n01-undefined-reserved-code.json` |
| MCPB-TOOL-014 | MUST | TODO(fixture), owner G5 (#66) with F10 (#59): `list_sessions` on a bound connection, given the sessions, presence states and discovery grants → exactly the discovery result `spec/session-channels.md` §7.3 defines for the bound session; needs a discovery `context` that `oac-mcpb-fixture/1` does not hold; the selection itself is fixtured by the `discovery`-stage fixtures of `spec/session-channels.md` §7.5 under `tests/protocol/sc-dlv/` |
| MCPB-TOOL-015 | MUST | `tests/protocol/mcpb-tool/MCPB-TOOL-015.p01-sessions-object.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-015.p02-empty-sessions.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-015.n01-prose-list.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-015.n02-descriptor-missing-capabilities.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-015.n03-bare-array.json` |
| MCPB-TOOL-016 | MUST | `tests/protocol/mcpb-tool/MCPB-TOOL-016.p01-unknown-session-unknown-destination.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-016.p02-undiscoverable-session-unknown-destination.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-016.p03-unreachable-session-destination-unavailable.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-016.n01-unknown-session-unsupported-capability.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-016.n02-undiscoverable-session-destination-unavailable.json` |
| MCPB-TOOL-017 | MUST | `tests/protocol/mcpb-tool/MCPB-TOOL-017.p01-identical-to-unknown-id.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-017.n01-explanatory-text.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-017.n02-presence-in-structured-content.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-017.n03-hint-in-meta.json` |
| MCPB-TOOL-018 | MUST | `tests/protocol/mcpb-tool/MCPB-TOOL-018.p01-unknown-tool-protocol-error.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-018.n01-unknown-tool-invalid-request.json` |
| MCPB-TOOL-019 | MUST | `tests/protocol/mcpb-tool/MCPB-TOOL-019.p01-same-progress-sequence.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-019.p02-no-related-messages.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-019.n01-revealing-log-notification.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-019.n02-revealing-progress-notification.json` |
| MCPB-TOOL-020 | MUST NOT | `tests/protocol/mcpb-tool/MCPB-TOOL-020.p01-no-echo.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-020.n01-to-echoed-in-text.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-020.n02-to-echoed-in-log.json` |
| MCPB-TOOL-021 | MUST NOT | `tests/protocol/mcpb-tool/MCPB-TOOL-021.p01-later-call-unaffected.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-021.p02-unbound-nothing-revealed.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-021.p03-handoff-from-hidden-sender.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-021.n01-revealing-log-after-response.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-021.n02-list-changed-for-existing-target.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-021.n03-throttled-after-probe.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-021.n04-close-when-target-exists.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-021.n05-retry-after-on-later-call.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-021.n06-unbound-log-names-session.json`, `tests/protocol/mcpb-tool/MCPB-TOOL-021.n07-session-terminated-when-target-exists.json` (a transport session terminated, and so its id re-issued, only when a hidden session exists, §12.2) |
| MCPB-META-001 | MUST | `tests/protocol/mcpb-meta/MCPB-META-001.p01-provenance-under-identifier.json`, `tests/protocol/mcpb-meta/MCPB-META-001.n01-provenance-under-other-key.json` |
| MCPB-META-002 | MUST | `tests/protocol/mcpb-meta/MCPB-META-002.n01-provenance-not-object.json` |
| MCPB-META-003 | MUST | `tests/protocol/mcpb-meta/MCPB-META-003.p01-mcp-defined-key-allowed.json`, `tests/protocol/mcpb-meta/MCPB-META-003.n01-unprefixed-key.json` |
| MCPB-META-004 | MUST NOT | `tests/protocol/mcpb-meta/MCPB-META-004.n01-invented-reserved-key.json` |
| MCPB-META-005 | MUST NOT | TODO(fixture), owner F11 (#60) with G4 (#65) and G9 (#70): a receiver's use of `_meta` is inbound behaviour after verification, not an MCP exchange the server answers. Planned: envelope whose `_meta` names a sender different from the signed `from` (negative) → sender taken from the verified envelope only |
| MCPB-META-006 | MUST NOT | `tests/protocol/mcpb-meta/MCPB-META-006.p01-whoami-values-in-text-and-meta.json`, `tests/protocol/mcpb-meta/MCPB-META-006.n01-whoami-session-id-only-in-meta.json`; a `send` or `reply` message id placed only in `_meta` is not decided, because this revision fixes no text layout that identifies it: TODO(fixture), owner G5 (#66) with G8 (#69) |
| MCPB-META-007 | MUST NOT | `tests/protocol/mcpb-meta/MCPB-META-007.p01-identifier-safe-meta-keys.json`, `tests/protocol/mcpb-meta/MCPB-META-007.n01-prefixed-key-in-channel-meta.json` |
| MCPB-FBK-001 | MUST | `tests/protocol/mcpb-fbk/MCPB-FBK-001.p01-tools-list-without-extension.json`, `tests/protocol/mcpb-fbk/MCPB-FBK-001.p02-tools-call-without-extension.json`, `tests/protocol/mcpb-fbk/MCPB-FBK-001.n01-tools-list-requires-extension.json` |
| MCPB-FBK-002 | SHOULD | none (not a `MUST`) |
| MCPB-CLD-001 | MUST | `tests/protocol/mcpb-cld/MCPB-CLD-001.p01-channel-capability-on-legacy.json`, `tests/protocol/mcpb-cld/MCPB-CLD-001.n01-channel-capability-on-modern.json` |
| MCPB-CLD-002 | MUST NOT | `tests/protocol/mcpb-cld/MCPB-CLD-002.p01-channel-notification-on-legacy.json`, `tests/protocol/mcpb-cld/MCPB-CLD-002.n01-channel-notification-on-modern.json` |
| MCPB-CLD-003 | MUST | `tests/protocol/mcpb-cld/MCPB-CLD-003.p01-discover-method-not-found.json`, `tests/protocol/mcpb-cld/MCPB-CLD-003.n01-discover-answered.json`, `tests/protocol/mcpb-cld/MCPB-CLD-003.n02-discover-unsupported-version.json` |
| MCPB-CDX-001 | MUST NOT | TODO(fixture), owner G7 (#68) with G8 (#69), against the F9 fake (#58): whether a client is Codex, and whether a notification is meant to deliver an OAC message, are not in an `oac-mcpb-fixture/1` `context`. Planned: a delivery to a Codex session → no MCP notification emitted toward the Codex client |
| MCPB-CDX-002 | MUST | TODO(fixture): app-server traffic, not an MCP exchange, so outside the `mcp-binding` stage (§12.2). Planned with G7 (#68) against the F9 fake: a delivery to an idle thread, one to a thread with a running turn, and one to an idle thread whose last turn ended interrupted → each hand-off is one `thread/queue/add`; the busy case's input arrives in a new turn after `turn/completed`, never under the running turn's id; the idle-after-interrupt case's input is not dispatched until a turn completes uninterrupted (source only, `service.rs` L477; UNVERIFIED live) |
| MCPB-CDX-003 | MUST NOT | TODO(fixture): app-server traffic (§12.2). Planned with G7 (#68) against the F9 fake: deliveries to an idle thread, to a busy thread, to a thread a turn starts on just after an idle status, and after a turned-away `thread/queue/add` → no `turn/start` sent for any of them |
| MCPB-CDX-004 | MUST NOT | TODO(fixture): app-server traffic (§12.2). Planned with G7 (#68) against the F9 fake: deliveries to a busy thread, including after a turned-away `thread/queue/add` → no `turn/steer` sent |
| MCPB-CDX-005 | MUST NOT | TODO(fixture): app-server traffic (§12.2). Planned with G7 (#68) and G6 (shim) against the F9 fake: each hand-off request → members `threadId`, `input`, `clientUserMessageId` only; a request adding `approvalPolicy`, `sandboxPolicy`, `permissions`, `model` or `cwd` (negative) → nonconformant |
| MCPB-CDX-006 | MUST NOT | TODO(fixture): app-server traffic (§12.2). Planned with G8 (#69) against the F9 fake: each subscribing `thread/resume` → members `threadId` and optionally `excludeTurns` only; one adding `model`, `cwd`, `approvalPolicy`, `sandbox`, `permissions` or `config` (negative) → nonconformant |

G4's committed transcripts are the first source of inputs for the `ERA` and `CLD`
fixtures; several fixtures above reproduce a G4 line's shape. They
are gate evidence, not conformance fixtures, so each fixture is re-captured in the E8
format rather than copied.

## Appendix A. Revision history

Binding revision 0.1 had no table of its own; its history is in `docs/planning/STATUS.md`
(E6, #46) and its freeze in `docs/planning/decisions/E7-interface-freeze.md`.

| Revision | Date | Change |
|---|---|---|
| 0.1 | 2026-10-06 | Frozen at Gate S2 (E7, #47): signed off on this date, in force from the merge of PR #276. |
| 0.2 | 2026-10-08 | #69, PR #350 (lead decision of 2026-10-08 to design a Codex reply pairing and propose it as a frozen-spec change): §4.5, the Codex issued-value pairing, binds a stdio connection that Codex started to one subscribed Codex thread on a reveal of a value the implementation issued on it, one value per connection and pairing window, and serves each call only on a live confirmation from that thread (MCPB-ATT-003 to MCPB-ATT-025); bounded pairing window (60 s), confirmation wait (10 s) and confirmation life (600 s, or the item's or turn's end); no pairing value where pairing cannot complete, and at most three windows per connection; MCPB-CDX-006 (no setting members in a subscribing `thread/resume`); §4.4 states that no documented per-request signal exists, so the refusal of an unbound connection is no longer called interim; a dated note at §5.4 records that MCPB-TOOL-017, MCPB-TOOL-019 and MCPB-TOOL-021 are met as written, with their meaning unchanged; §8.2, §9, §10 and §12.2 updated; fixtures for MCPB-ATT-004 to MCPB-ATT-006, MCPB-ATT-022 and MCPB-ATT-025. Minor revision (`spec/session-channels.md` §5.2, item 6): MCPB-ATT-003 is a `MAY`, MCPB-ATT-017 and MCPB-ATT-024 are `SHOULD`/`SHOULD NOT`, and every new `MUST` or `MUST NOT` binds only an implementation that pairs, so an implementation conformant to 0.1 stays conformant (§5.3, item 11). No requirement of 0.1 changes meaning, and no wire form changes. Matching changes: `spec/interfaces.md` 0.2 and `spec/security.md` 0.2. |
