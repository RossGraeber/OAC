# E9 — Design-for-replacement proofs

- **Date:** 2026-10-06.
- **Issue:** #49 (E9, Epic E #5). Depends on E7 (#47), the interface freeze.
- **Stage:** Stage 2 exit artifact (`docs/planning/v0.1/10-stages.md` §6, "Exit artifacts").
  It is not a Gate S2 criterion. It also proves DESIGN v0.1 acceptance criterion 10
  ("Transport contract is documented enough to independently add a second backend").
- **Status:** **Written.** All four acceptance items of #49 are met (§7). No amendment to a
  frozen item is proposed (§8).
- **Skills:** `oac-spec-authoring` (§7, the interface freeze), `oac-boundaries`,
  `oac-evidence`.

**What this record argues from.** The frozen interfaces at revision `0.1`:
`spec/interfaces.md` §4 (core neutral types), §5 (`ProviderAdapter`), §6 (`Transport`) and §7
(neutrality), as merged at `20482f1` (the PR #276 merge, which put the freeze in force,
`docs/planning/decisions/E7-interface-freeze.md` §8). Section and requirement numbers below
are to that commit. This record does not argue from the M0 draft
(`docs/planning/v0.1/05-interfaces.md` §13-§17), which `spec/interfaces.md` supersedes. It
replaces that draft's two proofs, §16 (ACP) and §17 (NATS/MQTT), and the §22 close-out that
recorded the second one as NOT MET.

**Where transport and harness names live.** This record is a proof document under
`docs/planning/decisions/`, not neutral text. NATS subjects, MQTT topics, ACP method names
and broker settings appear here because the proofs have to name the surface they map onto.
None of them is added to `spec/`, and none needs to be (§6).

**Method.** Each proof walks the frozen contract, operation by operation, and then every
requirement that `spec/interfaces.md` Appendix C assigns to that part. For each, it says how
the candidate meets it, which surface it uses, and the first-party evidence for that
surface. A requirement the candidate cannot meet would be a finding against the interface,
recorded in §8. Facts about NATS, MQTT and ACP come only from the first-party sources in §1,
fetched on 2026-10-06. A fact those sources do not settle is marked UNVERIFIED, with what is
missing.

---

## 1. Evidence

Every source below was fetched on 2026-10-06 and read at the line cited. Where a page has no
version of its own, the source-repository commit is the version.

### 1.1 NATS

Source repository of https://docs.nats.io: `nats-io/nats.docs`, commit
`f115becf6563e3bbe16bb94cbf87bfceb84199c1` (2026-08-24), the head of `master` when fetched.
The docs are not versioned per server release; the current server release is `v2.15.0`
(2026-09-17, https://github.com/nats-io/nats-server/releases). Paths below are relative to
`https://github.com/nats-io/nats.docs/blob/f115becf6563e3bbe16bb94cbf87bfceb84199c1/`.

| Id | Fact | Source (path, line) |
|---|---|---|
| N1 | "While Core NATS provides best-effort, at-most-once message delivery, JetStream introduces at-least-once and exactly-once semantics." JetStream "adds persistence capabilities". | `nats-concepts/core-nats/README.md` L5 |
| N2 | Core NATS is at most once: "If a subscriber is not listening on the subject (no subject match), or is not active when the message is sent, the message is not received." It "will only hold messages in memory and will never write messages directly to disk." | `nats-concepts/what-is-nats/README.md` L39 |
| N3 | "NATS implements source ordered delivery per publisher ... There are no guarantees of message delivery order amongst multiple publishers." | `reference/faq.md` L127 |
| N4 | Core NATS "offers only TCP reliability"; an offline subscriber "will not receive messages". | `reference/faq.md` L139 |
| N5 | A message is a subject, "a payload in the form of a byte array", headers and an optional reply address. `max_payload` is "1 MB by default" and "can be increased up to 64 MB". | `nats-concepts/core-nats/publish-subscribe/pubsub.md` (section "Messages") |
| N6 | A client "needs to be able to be configured with the details of how to connect to the NATS service infrastructure". | `using-nats/developing-with-nats/connecting/README.md` L3 |
| N7 | Servers gossip cluster members; clients connecting to a server "will discover other servers in the cluster"; seed servers "are the servers that clients have in their list of connect urls". | `running-a-nats-service/configuration/clustering/README.md` L7, L43-L47 |
| N8 | Gateways join clusters into superclusters; a leaf node "will transparently route messages as needed from local clients to one or more remote NATS system(s)". | `running-a-nats-service/configuration/gateways/README.md` L1-L5; `.../leafnodes/README.md` L1-L3 |
| N9 | "subject-level permissions on a per-user basis": each user's `publish` and `subscribe` subjects; an unauthorized publish or subscribe "fails ... and an error message is returned to the client". | `running-a-nats-service/configuration/securing_nats/authorization.md` L1-L5, permissions table, "Important Note" |
| N10 | "No responders": a request "sent to a subject with no subscribers will immediately receive a reply that has no body, and a `503` status", when the client opts in. | `nats-concepts/core-nats/request-reply/reqreply.md` L21-L23 |
| N11 | Client libraries may "buffer outgoing messages when the connection is down" and send them "once reconnected"; the buffer size is configurable. | `using-nats/developing-with-nats/reconnect/buffer.md` L3-L9 |
| N12 | The server's HTTP monitoring, "enabled in server configuration" (conventional port `8222`), includes "Subscription Routing (`/subsz`)". | `running-a-nats-service/nats_admin/monitoring/README.md` L12, L30 |
| N13 | The server publishes system events, including `$SYS.ACCOUNT.<id>.CONNECT` and `$SYS.ACCOUNT.<id>.DISCONNECT`. | `running-a-nats-service/configuration/sys_accounts/README.md` L1-L10, L29-L30 |
| N14 | Client libraries report connection events: "closed, disconnected or reconnected". | `using-nats/developing-with-nats/events/events.md` L1-L7 |
| N15 | The client protocol has `HPUB` and `HMSG` (messages with headers). | `reference/nats-protocol/nats-protocol/README.md` L48, L52 |

### 1.2 MQTT

MQTT Version 5.0, OASIS Standard, 07 March 2019:
https://docs.oasis-open.org/mqtt/mqtt/v5.0/mqtt-v5.0.html (the HTML rendering of
`os/mqtt-v5.0-os`). Clause ids are the standard's own conformance statement ids.

| Id | Fact | Source (section, clause) |
|---|---|---|
| M1 | QoS 0: "The message arrives at the receiver either once or not at all"; "no retry is performed by the sender". | §4.3.1 |
| M2 | Retransmission happens only on reconnect: "When a Client reconnects with Clean Start set to 0 and a session is present, both the Client and Server MUST resend any unacknowledged PUBLISH packets ... This is the only circumstance where a Client or Server is REQUIRED to resend messages." | §4.4, [MQTT-4.4.0-1] |
| M3 | Clean Start set to 1: "the Client and Server MUST discard any existing Session and start a new Session". | §3.1.2.4, [MQTT-3.1.2-4] |
| M4 | Session Expiry Interval: absent means 0; at 0 "the Session ends when the Network Connection is closed"; state is stored after close only if it is greater than 0. | §3.1.2.11.2, [MQTT-3.1.2-23] |
| M5 | The Server's session state includes "QoS 1 and QoS 2 messages pending transmission to the Client and OPTIONALLY QoS 0 messages". | §4.1 |
| M6 | RETAIN set to 1: the Server "MUST replace any existing retained message for this topic and store the Application Message ... so that it can be delivered to future subscribers". | §3.3.1.3, [MQTT-3.3.1-5] |
| M7 | Message Expiry Interval: if it "has passed and the Server has not managed to start onward delivery to a matching subscriber, then it MUST delete the copy"; a forwarded PUBLISH carries "the received value minus the time that the Application Message has been waiting in the Server". | §3.3.2.3.3, [MQTT-3.3.2-5], [MQTT-3.3.2-6] |
| M8 | Ordered Topic: the Server "MUST send PUBLISH packets to consumers (for the same Topic and QoS) in the order that they were received from any given Client"; by default every Topic is ordered on Non-shared Subscriptions; a Server "MAY" let topics be unordered. | §4.6, [MQTT-4.6.0-5], [MQTT-4.6.0-6] |
| M9 | PUBACK (and PUBREC) Reason Code `0x10` "No matching subscribers": "If the Server knows that there are no matching subscribers, it MAY use this Reason Code instead of 0x00 (Success)." | §3.4.2.1 Table 3-4; §3.5.2.1 |
| M10 | A Client and Server use "an underlying transport that provides an ordered, lossless, stream of bytes from the Client to Server". The standard defines no way to find a Server: the word "discover" occurs once, in §1.8.2 ("capability discovery", a v5.0 objective), and "multicast" not at all. | §4.2, [MQTT-4.2-1]; whole text searched |
| M11 | Authorization is the Server implementation's: it "should provide access controls ... to restrict the Clients ability to publish to particular Topics or to subscribe using particular Topic Filters" (non-normative). | §5.4.2 |
| M12 | A packet's Remaining Length is at most 268,435,455 bytes; a Client or Server can declare a smaller Maximum Packet Size. | §2.1.4; §3.1.2.11.4, §3.2.2.3.6 |
| M13 | Payload Format Indicator `0`: "the Payload is unspecified bytes". | §3.3.2.3.2 |
| M14 | Bridging is not defined; "message bridge applications" appears only as the motivation for v5.0 subscription options. | Appendix C (summary of new features) |

Broker example, Eclipse Mosquitto: https://mosquitto.org/man/mosquitto-conf-5.html (the page
states no version; the current release is `2.1.2` per https://mosquitto.org/download/).

| Id | Fact | Source (section) |
|---|---|---|
| Q1 | "Multiple bridges (connections to other brokers) can be configured". | "Configuring Bridges" |
| Q2 | `acl_file` topic ACLs per user, and `pattern` ACLs with `%c` (client id) and `%u` (username) substitution. | `acl_file` |
| Q3 | With `log_dest topic`, subscribe and unsubscribe log types publish to `$SYS/broker/log/M/subscribe` and `$SYS/broker/log/M/unsubscribe`. | `log_dest` |
| Q4 | `max_packet_size` sets the maximum packet size sent to MQTT v5 clients. | `max_packet_size` |
| Q5 | `persistence true` writes "connection, subscription and message data" to disk. | `persistence` |

### 1.3 ACP

Source repository of https://agentclientprotocol.com: `agentclientprotocol/agent-client-protocol`,
commit `487ad3eacd30bb19f75f462f815e78d678e393c4` (2026-10-05), the head of `main` when
fetched. The latest schema release is `schema-v1.24.1` (2026-09-30). Paths are relative to
`https://github.com/agentclientprotocol/agent-client-protocol/blob/487ad3eacd30bb19f75f462f815e78d678e393c4/`.

| Id | Fact | Source (path, line) |
|---|---|---|
| A1 | "the editor boots the agent sub-process on demand, and all communication happens over stdin/stdout." "Each connection can support several concurrent sessions." The agent makes requests of the editor, "for example to request permissions for a tool call". | `docs/get-started/architecture.mdx` (sections "Setup", "MCP") |
| A2 | Message flow: Client to Agent `initialize`, `session/new` or `session/load`, then `session/prompt`; Agent to Client `session/update`; the turn ends when the Agent "sends the `session/prompt` response with a stop reason". Agents "typically run as subprocesses of the Client". | `docs/protocol/v1/overview.mdx` L15-L39, L45 |
| A3 | stdio transport: "The client launches the agent as a subprocess." | `docs/protocol/v1/transports.mdx` L17-L21 |
| A4 | `initialize` carries `"protocolVersion": 1` in both the request and the response, and the Agent's `promptCapabilities` (`image`, `audio`, `embeddedContext`); the Client declares `fs` and `terminal` capabilities. | `docs/protocol/v1/initialization.mdx` L25-L80 |
| A5 | `session/new` takes `mcpServers`; "The Agent MUST respond with a unique Session ID". `session/load` replays the conversation; `session/resume` restores it without replay. | `docs/protocol/v1/session-setup.mdx` L59, L71, L108-L134, L217-L243 |
| A6 | A prompt turn: the Agent may call `session/request_permission`; the Client may send `session/cancel`; the turn ends with a `StopReason` response to `session/prompt`. The page says nothing about a second `session/prompt` sent while a turn is running. | `docs/protocol/v1/prompt-turn.mdx` L8, L57, L239-L251, L334-L367 |
| A7 | ACP v2 (Draft): "A prompt starts or contributes to foreground work in a session." Acceptance of `session/prompt` "means insertion"; the Agent reports `running` and `idle` through `state_update` notifications. | `docs/protocol/v2/prompt-lifecycle.mdx` L6-L8, L144, L193, L382 |
| A8 | "ACP v2 is available in Draft", published July 20, 2026. | `docs/announcements/acp-v2-draft.mdx` L1-L11 |
| A9 | ACP proxies, "components that sit between a client and an agent", can "Inject or modify prompts". The RFD is in the "Draft" group of the RFD navigation; a Draft RFD may get "Experimental implementation ... properly feature-gated". | `docs/rfds/proxy-chains.mdx` L9-L25; `docs/docs.json` (RFDs, Draft); `docs/rfds/about.mdx` L25-L27 |
| A10 | Method names starting with `_` are reserved for extensions; implementations "SHOULD ignore unrecognized notifications". | `docs/protocol/v1/extensibility.mdx` L47, L113 |

---

## 2. Proof A — a third adapter, ACP as the worked example

### 2.1 Claim

An adapter for an ACP agent implements the eight `ProviderAdapter` operations of
`spec/interfaces.md` Table 5.2 over the core types of §4, unchanged. It calls no `Transport`
operation. No transport module, no core type and no sentence of `spec/` changes for it.

### 2.2 What ACP is: a client-owned session, not a channel

ACP is a protocol between one **client** and one **agent**. The client starts the agent as
its subprocess and talks to it over that subprocess's stdin and stdout (A1, A3). The client
creates every session (`session/new`, `session/load`, `session/resume`; A2, A5), sends every
prompt (`session/prompt`; A2), answers the agent's permission requests (A1, A6), and may
provide the agent's file system and terminal (A4). The client is the session's owner and its
user interface.

A channel, in the sense of `spec/session-channels.md` §7.1, is a supported input surface
through which a party other than the session's owner hands content into a live session.
ACP v1 defines none. The only input operation is `session/prompt`, and only the session's
client can send it, on the client's own connection (A2). No ACP method lets another process
reach a session that a client created. That is why ACP is "a candidate neutral
'client-owned session' surface for later providers, not a v0.1 dependency"
(PLANNING-PROMPT.md §3.5), and this record keeps that characterization. It also matches the
conflict-register disposition recorded in `docs/planning/v0.1/05-interfaces.md` §16 (register
row "ACP is client-owned-session, not a channel", `RESOLVED-BY-EVIDENCE`).

So an ACP adapter can reach a session only from inside the client side of that session.
There are three shapes:

1. **OAC is the ACP client.** OAC starts the agent and drives it. OAC would then be the
   session's owner and front end: it would answer `session/request_permission` (A6) and may
   host the agent's file system and terminal (A4). That is the harness role ADR-001 keeps out
   of OAC: its Context asks for communication "without creating another agent harness",
   with each provider "responsible for inference, authentication, context, tools, and policy
   enforcement"; PLANNING-PROMPT.md §10 says "Do not turn OAC into an agent framework". It
   would also need OAC to answer consent steps, which [SEC-AUZ-023] forbids ("MUST NOT
   automate, suppress or pre-answer a consent step"). And it leaves no human at the session,
   which ADR-001-A2's definition of an existing session requires
   (`docs/planning/ADR-001-AMENDMENTS.md`). **Ruled out.**
2. **OAC is an ACP proxy between the human's client and the agent** (A9). The human's client
   stays the owner and keeps every permission request. The proxy's harness-side process sees
   the session ids and can add an MCP server to `session/new` (`mcpServers`, A5) for
   outbound requests. Proxies are a **Draft RFD** (A9). In the vocabulary of `oac-evidence`
   §4 that is **experimental**: it would need a named shim boundary and a pinned version
   before any adapter used it.
3. **No OAC process in the client path.** OAC never sees the session, so it has no native
   signal and cannot bind it ([SC-ID-120]). Nothing to adapt.

Shape 2 is the only one that keeps ADR-001. The walk below uses it.

### 2.3 Walk of `ProviderAdapter` (`spec/interfaces.md` Table 5.2)

| Operation | ACP realization (shape 2) | Contract rule it meets |
|---|---|---|
| `take_connection` | The proxy, a harness-side process the human's client starts through its agent command, opens one local connection to the core process per ACP session it sees. The core creates the `Connection` after authenticating the proxy process with an OS facility. | [IFC-ADP-012], [IFC-ADP-013]; §2.3 "Harness-side process" |
| `watch_attachments` | `attachment-opened` when the proxy reports an ACP session on its connection; `native-signal` with `native_id` = the ACP `sessionId` (A5) and start kind `fresh` for `session/new`, `transition` for `session/load` and `session/resume`; no cross-check value; `attachment-closed` when the session or the connection ends. | [IFC-ADP-020], [IFC-ADP-022]; `spec/session-channels.md` §6.7.1, case 3(c) of §6.7.3 ([SC-ID-139], with the diagnostic of [SC-ID-141]) |
| `set_binding` | Stored per attachment; nothing is handed off to an unbound attachment. | [IFC-ADP-030] |
| `capabilities` | `active_inbound` `false` (§2.4). `content_types` from the agent's `promptCapabilities` (A4), only for types the adapter can hand off unchanged. | [IFC-ADP-040], [IFC-ADP-041]; [SC-ID-104] |
| `deliver` | Not reached while `active_inbound` is `false`: the session is send-only (`spec/session-channels.md` §6.6). If called, it returns `failed` and makes no hand-off call. | [IFC-ADP-050], [IFC-ADP-057] |
| `accept_requests` | The MCP server the proxy adds to `session/new` receives the agent's send, reply and discovery requests; the proxy passes each, labelled with its `Connection`, to the request sink, and returns the `RequestResult` unchanged. | [IFC-ADP-003], [IFC-ADP-031], [IFC-ADP-060] |
| `health` | `HealthStatus` of the proxy connections, with no credential, address, native id or working directory. | [IFC-TYP-092] |
| `shutdown` | `attachment-closed` for each open attachment, then no further hand-off or request. | [IFC-ADP-071], [IFC-ADP-070] |

Every input and output in that table is a §4 type: `Connection`, `NativeSignal`,
`AdapterCapabilities`, `HandOff`, `HandOffOutcome`, `SendRequest`, `DiscoveryRequest`,
`RequestResult`, `HealthStatus`. The ACP-specific values are inside the adapter, except the
two that [IFC-NEU-002] and Table 7.1 already let cross: `native_id` (the ACP `sessionId`) and a
cross-check value, which ACP does not have.

**The other adapter-owned requirements** (`spec/interfaces.md` Appendix C, owner `adapter`).
None names a harness. Each is met by stating, in the ACP adapter binding document that
[IFC-ADP-080] requires, which ACP surface carries it:

- [IFC-ADP-001] to [IFC-ADP-007]: the proxy talks to the core only, never to a transport,
  and never creates or signs an envelope. It reads no provider credential: the agent's own
  `authenticate` exchange (A2) runs between the human's client and the agent, and the proxy
  only forwards it. It calls no model interface.
- SC-DLV 001-009, SC-RCP 004-006 and the SEC-AUZ adapter rows: they constrain a hand-off. A
  send-only session makes none.
- SEC-PRV (provenance rendering): it applies to hand-offs, so it is not exercised while the
  session is send-only. When a hand-off is defined (§2.4), the binding names how provenance
  renders in an ACP content block, and [IFC-ADP-054] refuses any hand-off the surface would
  alter.
- SC-ENV 064, 082, 091, 092 (adapter rows): unchanged by the harness.
- The MCPB rows bind the MCP binding's two v0.1 harness profiles (`spec/bindings/mcp.md` §8)
  and do not apply to an ACP adapter; its own binding document carries its own rows.

### 2.4 Why the ACP adapter declares `active_inbound` `false`

The hand-off surface would be `session/prompt`, sent by the proxy. The frozen security rules
decide whether it may be used:

- **ACP v1** does not say what a `session/prompt` sent during a running turn does (A6).
  Under [SEC-AUZ-022] an operation is a steering operation if the harness adds its input to a
  running turn, whatever its documentation says. Until that is shown either way, the binding
  cannot name `session/prompt` as non-steering (UNVERIFIED, §9).
- **ACP v2 (Draft, A7, A8)** documents that a prompt "starts or contributes to foreground
  work". Sent while work is running, it adds input to it: a steering operation under
  [SEC-AUZ-022].
- ACP offers no operation that holds input for a turn of its own, so [SEC-AUZ-025] has
  nothing to use. Under [SEC-AUZ-026] the session counts as possibly running unless the
  harness establishes otherwise in the hand-off operation itself. The `running`/`idle`
  notifications of v2 are a separate check before the call, which [SEC-AUZ-026] says does
  not establish it: the human's client can start a turn between the check and the call.

So no ACP hand-off yet meets [SEC-AUZ-022] and [SEC-AUZ-026] together. The adapter declares
`active_inbound` `false` ([IFC-ADP-040], [SC-ID-104]), and the session is send-only. That is
the contract's own path for "a harness lacking active inbound" (`spec/session-channels.md`
§6.6). It needs no new type, member or rule. If a later ACP revision adds a holding prompt, or
an agent is shown to reject a prompt during a running turn in the same call, the binding can
name it and declare `true`, still under the frozen contract.

### 2.5 No transport change, no core change

- **No transport module changes.** [IFC-ADP-001] forbids an adapter from invoking a
  transport operation, and §1.1 routes every request through the core. The ACP adapter's
  requests become envelopes in the core and reach a transport through `publish` exactly as
  the other adapters' do. `Transport` (Table 6.4) has no parameter that names an adapter or a
  harness ([IFC-NEU-001]).
- **No core type changes.** The walk in §2.3 used only §4 types, and every member it needed
  exists. `NativeSignal.start_kind` already admits `fresh` and `transition`, which cover
  `session/new`, `session/load` and `session/resume`. A missing cross-check value is already
  case 3(c).
- **No neutral text changes.** The ACP names above appear only in this record and would
  appear in the ACP adapter binding document ([IFC-ADP-080]).

### 2.6 Verdict

**HOLDS.** A third adapter, over ACP, implements the frozen adapter contract with no change
to any transport module, core type or `spec/` text. ACP is a client-owned-session protocol,
not a channel. Its only supported route that keeps ADR-001 is a proxy, which is a Draft RFD,
and under the frozen security rules its sessions are send-only. Findings: F-A1 and F-A2 (§8).

---

## 3. Proof B — NATS replaces the v0.1 transport

### 3.1 Claim

A NATS transport implements the seven `Transport` operations of `spec/interfaces.md` Table 6.4
and every transport-owned requirement, with no change to any adapter, core type or `spec/`
text. Its capability declaration (Table 6.3) is below, with the capabilities it lacks.

The profile is **Core NATS**: publish and subscribe on subjects, no JetStream, no
request-reply. Each implementation has one client connection, with its own NATS user. The
transport derives each subject inside the module from the `Destination`, by a one-way hash
of the session id or key id, as Decision C7 §3 does for the v0.1 transport. Session ids
(lower-case Crockford Base32, `spec/session-channels.md` §6.1) and key ids (lower-case hex)
are valid subject tokens in any case.

### 3.2 Capability declaration (Table 6.3)

| Member | NATS value | Evidence |
|---|---|---|
| `reliability` | **absent** | Core NATS is at most once and "offers only TCP reliability" (N1, N2, N4). JetStream adds at-least-once, but it does so by storing messages, which [IFC-TRN-026] and [IFC-TRN-033] forbid. |
| `persistence` | **absent** (required, [IFC-TRN-026]) | Core NATS holds messages "in memory" only and never on disk (N2). JetStream, the persistence layer (N1), is not used. |
| `offline_queueing` | **absent** (required, [IFC-TRN-026]) | A subscriber that is not active "will not receive messages" (N2, N4). The client reconnect buffer is the one exception and is handled under [IFC-TRN-036] (§3.4). |
| `ordering` | **present**, for one publishing connection | "source ordered delivery per publisher" (N3). The profile publishes from one connection per implementation. |
| `multicast_discovery` | **absent** | A client must be configured with how to connect (N6). Gossip discovers servers only after a connection to a configured seed (N7). |
| `routing_federation` | **present** when deployed as a cluster, supercluster or leaf node | Routes, gateways and leaf nodes relay messages between servers and systems (N7, N8). A single server declares it absent. |
| `reach` | `cross-implementation` | A server reaches every connected client. |
| `destination_restricted` | `true` only with per-user subscribe permissions (§3.4, [IFC-TRN-080]) | N9 |
| `max_payload_octets` | the server's `max_payload` (default 1 MB, at most 64 MB), at least 65536 | N5; [IFC-TRN-023] |

### 3.3 Walk of `Transport` (`spec/interfaces.md` Table 6.4)

| Operation | NATS realization | Contract rule it meets |
|---|---|---|
| `start` | Connect to the configured server URLs with the implementation's own NATS credentials (never the device key, [IFC-TRN-025]). Subscribe to the local device's subject, derived from the key id passed in. Return the §3.2 declaration. | [IFC-TRN-020], [IFC-TRN-021], [IFC-TRN-023], [IFC-TRN-026] |
| `publish` | Core NATS publish of the octets to the destination's subject. The deadline travels in a message header (N15). Returns `taken` once the client has written it, `not-taken` if the client refused it (closed connection, no buffer). | [IFC-TRN-030], [IFC-TRN-031], [IFC-TRN-044] |
| `subscribe` | Subscribe to the session's or device's subject. The server pushes each message (`MSG`/`HMSG`) to the handler. Unsubscribe ends it. | [IFC-TRN-040] |
| `send_presence` | Publish the authenticated presence record to the audience device's subject. | [IFC-TRN-050] |
| `watch_presence` | Records arriving on the device subject go to the handler with a `CarrierHandle` naming the server connection. A client "disconnected" event (N14) is a `carrier-loss` for that handle. | [IFC-TRN-060]; [IFC-TRN-061] (MAY, taken) |
| `health` | Connection status. No URL, credential or subject in `detail`. | [IFC-TYP-092] |
| `shutdown` | Unsubscribe and close; no handler runs afterwards. | [IFC-TRN-071] |

**The other transport-owned requirements** (Appendix C, owner `transport`):

- [IFC-TRN-001]: all three payload kinds are octets on a subject; NATS treats the payload as
  "a byte array" (N5).
- [IFC-TRN-033] to [IFC-TRN-035]: Core NATS keeps a message only in memory while forwarding
  it, never on disk, and never redelivers (N2). A receiving transport drops a copy whose
  carried deadline has passed. How the binding compares the sender's deadline with the
  receiver's clock is for the binding document to state; the core re-checks the hand-off
  deadline in any case ([SC-RCP-091]).
- [IFC-TRN-036]: the client reconnect buffer (N11) holds payloads while the connection is
  down and sends them later. That is holding a payload for a destination that is not
  reachable. The binding sets the buffer so that no payload is held while disconnected, and
  `publish` then returns `not-taken`. Whether every client library can turn the buffer off,
  rather than only resize it, is UNVERIFIED (N11 documents the size only); a library that
  cannot is not usable for this binding.
- [IFC-TRN-043], [IFC-TRN-044]: a Core NATS publish returns nothing about subscribers
  (fire-and-forget, N2), so the `PublishResult` cannot depend on one. The binding never uses
  request-reply, whose "no responders" status reveals that a subject has no subscriber
  (N10). Subscription state is visible on the server's monitoring endpoint `/subsz` and in
  system events (N12, N13). The binding leaves monitoring off or unreachable, and gives no
  implementation's user the system account or subscribe rights to `$SYS`.
- [IFC-TRN-080]: each implementation's NATS user may subscribe only to its own device subject
  and its own sessions' subjects (N9). The binding document states how a device key is
  provisioned to a NATS user. Without that provisioning the transport declares
  `destination_restricted` `false`, and [IFC-TRN-081] then keeps presence records off it.
- [IFC-NEU-003]: server URLs, subjects and the NATS connection stay inside the module; the
  core sees only the opaque `CarrierHandle`.
- [IFC-TRN-090]: the items above are the binding document's content.

### 3.4 What NATS changes for the core: nothing

The core-owned transport rules hold unchanged. The core does not rely on NATS for integrity
or authenticity ([IFC-TRN-010]); NATS loses and duplicates nothing the core cannot absorb
([IFC-TRN-011]); the server connection is never a device or a session ([IFC-TRN-012],
[IFC-TRN-013]); and the core depends on none of the capabilities above ([IFC-TRN-022]). The
one carrier handle covers every issuer, so a lost server connection marks them all stale.
That only moves sessions toward `unreachable` (§6.6 of `spec/interfaces.md`).

### 3.5 Verdict

**HOLDS.** NATS can replace the v0.1 transport with no change to any adapter, core type or
`spec/` text. **NATS lacks:** reliability, persistence and offline queueing (the last two are
required absent anyway), and multicast discovery. It provides ordering per publisher and,
when deployed as a cluster, routing and federation. Findings: F-T1, F-T2 and F-T3 (§8).

---

## 4. Proof C — MQTT replaces the v0.1 transport

### 4.1 Claim

An MQTT transport implements the seven `Transport` operations and every transport-owned
requirement, with no change to any adapter, core type or `spec/` text.

The profile is **MQTT 5.0, QoS 0, Clean Start 1, Session Expiry Interval 0, RETAIN 0**. Each
implementation has one client connection with its own username. Topics are derived inside the
module from the `Destination` by the same one-way hash as §3.1.

### 4.2 Capability declaration (Table 6.3)

| Member | MQTT value | Evidence |
|---|---|---|
| `reliability` | **absent** | QoS 0 is at most once (M1). QoS 1 and 2 retransmit only on reconnect into a kept session (M2), and keeping a session means keeping its pending messages (M5), which [IFC-TRN-033] and [IFC-TRN-036] forbid. QoS 0 is also how the binding meets [IFC-TRN-043] (§4.3). |
| `persistence` | **absent** (required, [IFC-TRN-026]) | Clean Start 1 discards any session (M3); RETAIN 0 stores nothing for future subscribers (M6). A broker's own persistence (Q5) then has no message to keep. |
| `offline_queueing` | **absent** (required, [IFC-TRN-026]) | Session Expiry Interval 0 ends the session, with its pending messages, when the connection closes (M4, M5). |
| `ordering` | **present** | The Server forwards messages from one Client on one Topic at one QoS in order, and treats every topic as ordered by default (M8). The profile uses one topic per destination and one QoS. A Server configured to make topics unordered (M8, "MAY") declares it absent. |
| `multicast_discovery` | **absent** | The Client connects to a Server over a byte stream it already has (M10); the standard defines no discovery (M10). |
| `routing_federation` | **absent in the protocol**; present only through a broker feature | MQTT 5.0 defines no bridging (M14). A broker may provide it, such as Mosquitto bridges (Q1); a binding over such a broker declares it present, citing that broker. |
| `reach` | `cross-implementation` | A Server reaches every connected Client. |
| `destination_restricted` | `true` only with per-client topic ACLs (§4.3, [IFC-TRN-080]) | M11; Q2 |
| `max_payload_octets` | the broker's maximum packet size less packet overhead, at least 65536 | M12; Q4; [IFC-TRN-023] |

### 4.3 Walk of `Transport`

| Operation | MQTT realization | Contract rule it meets |
|---|---|---|
| `start` | CONNECT with Clean Start 1 and no Session Expiry Interval, using the implementation's own MQTT credentials (never the device key). SUBSCRIBE to the local device topic. Return the §4.2 declaration. | [IFC-TRN-020], [IFC-TRN-021], [IFC-TRN-023], [IFC-TRN-025], [IFC-TRN-026] |
| `publish` | PUBLISH at QoS 0, RETAIN 0, Payload Format Indicator 0 (M13), Message Expiry Interval set to the whole seconds left before the deadline, rounded down. A payload with less than one second left is not sent, and the result is `not-taken`. Otherwise `taken` once written. | [IFC-TRN-030], [IFC-TRN-031], [IFC-TRN-034], [IFC-TRN-044] |
| `subscribe` | SUBSCRIBE to the destination topic. The Server pushes each PUBLISH to the handler. UNSUBSCRIBE ends it. | [IFC-TRN-040] |
| `send_presence` | PUBLISH of the authenticated presence record to the audience device's topic. | [IFC-TRN-050] |
| `watch_presence` | Records on the device topic go to the handler with a `CarrierHandle` naming the Server connection; loss of that connection is a `carrier-loss`. | [IFC-TRN-060]; [IFC-TRN-061] |
| `health` | Connection status, with no broker address, credential or topic. | [IFC-TYP-092] |
| `shutdown` | DISCONNECT; no handler runs afterwards. | [IFC-TRN-071] |

**The other transport-owned requirements:**

- [IFC-TRN-001], [IFC-TRN-030]: the payload is carried as "unspecified bytes" (M13).
- [IFC-TRN-033] to [IFC-TRN-036]: Clean Start 1 and Session Expiry Interval 0 leave no
  session state after a connection closes (M3, M4), so nothing is kept across a restart or
  for an unreachable destination. RETAIN 0 stores nothing for later subscribers (M6). The
  Server deletes a copy whose expiry passes before onward delivery starts, and forwards the
  remaining lifetime, not an absolute time (M7). The receiving transport drops a copy whose
  forwarded lifetime is zero. This bounds the deadline without comparing two clocks, to
  within one second and the network delay.
- [IFC-TRN-043], [IFC-TRN-044]: MQTT 5.0 lets a Server answer a QoS 1 or QoS 2 PUBLISH with
  `0x10` "No matching subscribers" (M9). That tells the publishing implementation whether
  anyone subscribes to the destination, which [IFC-TRN-043] forbids. QoS 0 has no
  acknowledgement (M1), so the profile publishes at QoS 0 only. A broker may also publish
  subscribe events on `$SYS` topics (Q3 for Mosquitto). The binding disables them and denies
  every implementation's user read access to `$SYS`.
- [IFC-TRN-080]: per-client ACLs, such as Mosquitto `pattern read` rules on `%u` or `%c`
  (Q2), let each user subscribe only to its own device and session topics. Authorization is
  left to the Server implementation (M11), so the binding names the broker it relies on. As
  for NATS, the transport declares `destination_restricted` `false` until a device key is
  provisioned to a broker identity.
- [IFC-NEU-003], [IFC-TRN-090]: broker addresses, topics and MQTT sessions stay in the module
  and in its binding document.

### 4.4 What MQTT changes for the core: nothing

As for NATS (§3.4): the core-owned rules ([IFC-TRN-010] to [IFC-TRN-013], [IFC-TRN-022],
[IFC-TRN-032], [IFC-TRN-037], [IFC-TRN-041], [IFC-TRN-042], [IFC-TRN-051], [IFC-TRN-062],
[IFC-TRN-081]) hold as written. No core type gains a member.

### 4.5 Verdict

**HOLDS.** MQTT can replace the v0.1 transport with no change to any adapter, core type or
`spec/` text. **MQTT lacks:** reliability (in the only profile the contract allows),
persistence and offline queueing (required absent anyway), multicast discovery, and, in the
protocol itself, routing and federation. It provides ordering per client, topic and QoS.
Findings: F-T1, F-T2 and F-T4 (§8).

---

## 5. Capability comparison

This table replaces `docs/planning/v0.1/05-interfaces.md` §17's table, whose twelve NATS and
MQTT cells were UNVERIFIED. The Zenoh column is carried from that table and Decision C7, as
the v0.1 transport's recorded mapping; this record re-verifies nothing about Zenoh.

| Capability (Table 6.3) | Zenoh (v0.1, C7) | NATS (§3.2) | MQTT 5.0 (§4.2) |
|---|---|---|---|
| `reliability` | reliable pub/sub over TCP-based links (C7 §8) | absent (N1, N2, N4) | absent in the allowed profile (M1, M2, M5) |
| `persistence` | not used (C7 §4) | absent (N2) | absent (M3, M6) |
| `offline_queueing` | not provided in peer mode (C7 §5) | absent (N2, N4; N11 handled) | absent (M4, M5) |
| `ordering` | not claimed | present, per publisher (N3) | present, per client, topic and QoS (M8) |
| `multicast_discovery` | provided, via scouting (C7 §5) | absent (N6, N7) | absent (M10) |
| `routing_federation` | out of scope for the peer-mode profile (C7 §5) | present as cluster, supercluster or leaf node (N7, N8) | absent in the protocol (M14); broker-specific (Q1) |

Both candidates lack multicast discovery, persistence and offline queueing, and neither is
reliable end to end in a profile the contract allows. NATS adds routing and federation. Both
add ordering. [IFC-TRN-022] makes the core indifferent to all of it.

---

## 6. Proof D — the transport contract is enough for an independent second backend

DESIGN v0.1 acceptance criterion 10: "Transport contract is documented enough to
independently add a second backend."

**Shown by doing it twice.** Proofs B and C each walked `spec/interfaces.md` §6 from its own
text: Table 6.1 (what to carry and with what deadline), Table 6.3 (what to declare, with a
meaning for each member), Table 6.4 (the operations), and the requirements of §6.2 to §6.9.
Neither walk needed any other OAC document, any Zenoh detail or any reference-implementation
code. Both reached a complete operation mapping and a complete declaration, and both found
where their surface would break a rule (the NATS reconnect buffer, the MQTT `0x10` reason
code) from the rule text alone. [IFC-TRN-090] says what the backend's binding document has to
add, and §3.3 and §4.3 above are drafts of exactly that content.

**What the contract leaves to the binding, by design:** how a deadline is enforced at the
receiving end (§3.3, §4.3), how a device key is tied to a transport identity for
[IFC-TRN-080], and how carrier loss is detected. Each is named in [IFC-TRN-090] or in a rule's
own text, so a second backend knows it has to answer it. None needs a contract change.

**No core type or neutral text changes.** Across Proofs A to D:

- every value that crossed a contract was one of the §4 types (Table 4.1, §4.10, §4.11);
- no new member, operation, parameter, state or error code was needed;
- no harness or transport name had to enter `spec/` ([IFC-NEU-001] to [IFC-NEU-004]).
  The CI step "Checks 1-2 and spec neutral vocabulary" still reports four clean `spec/`
  files (§10), and this change touches no file under `spec/`.

**Verdict: MET.**

---

## 7. #49 acceptance

| # | Acceptance item | Status | Where |
|---|---|---|---|
| 1 | A third provider adapter, ACP as the worked example, can be added with no change to any transport module, shown by walking the interfaces | **Met** | §2.3 to §2.6 |
| 2 | NATS or MQTT can replace Zenoh with no change to any adapter or to the spec, with the optional capabilities each candidate lacks listed | **Met**, for both | §3, §4, §5. All twelve NATS and MQTT capability cells are now cited to first-party sources. |
| 3 | The transport contract is documented well enough for an independent second backend (DESIGN acceptance criterion 10) | **Met** | §6 |
| 4 | ACP is characterized correctly as a client-owned-session protocol, not a channel | **Met** | §2.2 |

PLANNING-PROMPT §6's other two items (CI without providers, and opt-in provider tests) are
owned by the test strategy (`docs/planning/v0.1/09-test-strategy.md`) and are not E9
acceptance items; `docs/planning/v0.1/05-interfaces.md` §18 records them.

---

## 8. Findings

None of these is a gap in a frozen interface. Each is recorded so that the binding document
or the decision it belongs to carries it.

| Id | Finding | Kind | Where it belongs |
|---|---|---|---|
| F-A1 | ACP gives OAC no supported route into a client-owned session except a proxy, which is a Draft RFD (A9). An ACP adapter therefore depends on an experimental surface and needs a named shim boundary and a pinned version before it is built (`oac-evidence` §4). | Surface stability | A future ACP adapter's binding document; `docs/planning/v0.1/12-deferred.md` |
| F-A2 | Under [SEC-AUZ-022] and [SEC-AUZ-026], no ACP hand-off is yet usable: v1 does not document `session/prompt` during a running turn (UNVERIFIED), v2 Draft documents it as steering, and ACP has no holding hand-off. ACP sessions are send-only. | Declaration | §2.4; the ACP binding document |
| F-A3 | One ACP connection can carry several sessions (A1). The adapter contract makes one `Connection` one attachment (§5.4). The proxy shape fits by opening one local connection per ACP session (§2.3). This is the same shape as the open MCP binding item "one Codex legacy-era MCP connection carries calls from several threads" (`spec/bindings/mcp.md` §10, owner #69). | Constraint on the adapter | §2.3; the ACP binding document |
| F-T1 | A broker transport needs a server that someone runs. ADR-001 ("Decision") rules out "a separately administered server for normal local use". Within one installation this does not arise: [IFC-TRN-002] and [IFC-TRN-081] keep v0.1 same-install. Across installations, choosing NATS or MQTT is a deployment decision under ADR-001, not an interface change. | Deployment | ADR-001 if a broker transport is ever proposed; `docs/planning/v0.1/12-deferred.md` |
| F-T2 | Neither broker transport is `destination_restricted` until a device key is provisioned to a broker identity, which neither protocol defines (N9, M11). Until then [IFC-TRN-081] keeps all cross-implementation traffic off it, as it does for the v0.1 transport today (`spec/interfaces.md` §6.7 note). | Binding | The transport binding document ([IFC-TRN-090]) |
| F-T3 | The NATS client reconnect buffer must hold nothing while disconnected ([IFC-TRN-036]). The docs show only how to size it (N11); whether each client library can disable it is UNVERIFIED. | Binding; UNVERIFIED | §3.3; `docs/planning/STATUS.md` |
| F-T4 | MQTT QoS 1 and 2 can reveal whether a destination is subscribed (`0x10`, M9), so an MQTT binding is QoS 0 only and therefore not reliable. | Binding | §4.3 |

**No amendment is proposed.** The procedure of
`docs/planning/decisions/E7-interface-freeze.md` §7 and `oac-spec-authoring` §7 applies when
a proof shows a frozen interface cannot accommodate a replacement. None did: every finding
above is met inside an adapter or transport, or in its binding document, under the frozen
text. F-A3 was the closest. It would need an amendment only if an adapter had to report
several sessions on one `Connection`, and the proxy shape does not.

---

## 9. UNVERIFIED items

Resolved by this record (`oac-evidence` §5 promotion: re-verified above, removed from
`docs/planning/STATUS.md` "Open UNVERIFIED items" in the same change):

- NATS reliability, persistence, offline queueing, ordering, multicast discovery and
  routing/federation (§3.2, N1-N8).
- MQTT reliability, persistence, offline queueing, ordering, multicast discovery and
  routing/federation (§4.2, M1-M14, Q1), including the cell `05-interfaces.md` §17 called a
  "structural observation" (multicast discovery, now M10).
- ACP protocol version `1`: the literal `"protocolVersion": 1` appears in the v1
  initialization example (A4). The pin does not move.
- ACP schema v2 "alpha": **drift.** ACP v2 was published "in Draft" on 2026-07-20 (A8).
  "Alpha" is no longer the source's word. ACP v1 stays the stable version, so the pin does
  not move. PLANNING-PROMPT.md §3.5 keeps its 2026-09-15 wording as the baseline.
- Also observed, not an UNVERIFIED item: PLANNING-PROMPT.md §3.5 records that "unknown
  notifications should be ignored", lowercase. The v1 page now says implementations "SHOULD
  ignore unrecognized notifications" (A10).

Added by this record (in `docs/planning/STATUS.md` in the same change):

- Whether each NATS client library can disable the reconnect buffer, not only resize it
  (F-T3). Missing: a first-party statement per client library. Owner: a future NATS
  transport binding; not a v0.1 dependency.
- What an ACP v1 agent does with a `session/prompt` received while a turn is running:
  rejects it, holds it, or adds it to the running turn (F-A2). Missing: a statement in the
  v1 protocol pages (A6 has none). Owner: a future ACP adapter binding; not a v0.1
  dependency.

---

## 10. Checks run on this change

Run on branch `spec/49-replacement-proofs`, from `origin/main` at `20482f1`. Results are
quoted in the pull request description.

- `node tests/protocol/runner/run.mjs` and `--self-test`.
- `node scripts/check-fixture-manifest.mjs` and `--self-test`.
- `node scripts/check-skills.mjs`; `node scripts/sync-agents-skills.mjs --check` and
  `--self-test`.
- `.github/workflows/boundary-lint.yml`: checks 3, 8, 9-10 (with self-test), "Checks 1-2 and
  spec neutral vocabulary" and check 11, run verbatim.

## Boundary and evidence pass

- Nothing here has OAC call a model, hold a provider credential, own a turn loop, answer a
  consent step or poll. §2.2 rules out the one shape (OAC as the ACP client) that would.
- No durable store, mailbox or broadcast is proposed: both broker profiles keep persistence
  and offline queueing off ([IFC-TRN-026]).
- Transport and harness names are confined to this record. No file under `spec/` or
  `core/` changes.
- Every NATS, MQTT, Mosquitto and ACP fact carries a source, a version or commit, and a
  retrieval date (§1). Method names, clause ids and settings are copied from those sources.
- ACP's proxy route is labelled experimental (Draft RFD, A9); ACP v1 is the stable protocol
  (A4). NATS, MQTT 5.0 and Mosquitto are named as candidates, not dependencies, and no pin is
  added.
