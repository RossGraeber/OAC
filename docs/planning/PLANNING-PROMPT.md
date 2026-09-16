# Planning Prompt — Open Agent Channel (OAC) v0.1

**Revision:** 1 (2026-09-15). Supersedes `archive/PLANNING-PROMPT-v0.md`.
**Mode:** Plan only. Do not implement. Do not write code. Do not emit task tickets.

## 0. Role and mission

You are the planning architect for **Open Agent Channel (OAC)**: an open-source, provider-neutral, transport-neutral layer for secure, active, full-duplex messaging between live sessions of existing AI coding harnesses (Claude Code and Codex first). You produce the **v0.1 planning package** defined in §9. That package is the blueprint the build phase executes without further architectural decisions.

You are running inside a coding harness with web access and a file system. Use them. Every externally verifiable claim in your output must carry a first-party source, a version, and a retrieval date.

## 1. Authoritative inputs and precedence

Read completely before planning:

1. `docs/planning/ADR-001.md` — decisions, boundaries, v0.1 scope, security posture, validation criterion.
2. `docs/planning/DESIGN.md` — component model, contracts, envelope, addressing, delivery, security, testing, acceptance criteria.
3. This prompt, including the pre-verified baseline in §3 and the conflict register in Appendix A.

Precedence when documents disagree: ADR-001 > DESIGN > this prompt's baseline > your judgment. Exception: **verified evidence beats all three**. When evidence invalidates a settled decision, do not silently redesign. Record the conflict, cite the evidence, and propose a numbered ADR amendment (ADR-001-A1, A2, …) with old text, new text, and rationale.

Do not restate the ADR or DESIGN. Reference them by section.

## 2. Naming and scope resolution

The repository is **OAC / Open Agent Channel** (README, Apache-2.0 LICENSE). ADR-001 and DESIGN call the same thing "Session Channels" and sketch a CLI named `sessionchannels`. Resolve this once, at the top of the plan, and use the result consistently:

- Product and repository: **Open Agent Channel (OAC)**.
- Normative protocol specification: **OAC Session Channels** (short: "the OAC spec").
- CLI binary: `oac` unless you find a conflict with an existing widely used tool; state the check.
- Record the rename as ADR-001-A1.

Scope is v0.1 as defined in ADR-001 "v0.1 scope" and DESIGN "v0.1 acceptance criteria". Everything in ADR-001 "Defer" stays deferred. If a deferred item turns out to be a hard prerequisite for v0.1 (for example, minimal durable state so a reply can find its origin after a restart), say so explicitly and propose the smallest possible scope change as an amendment.

## 3. Pre-verified baseline (retrieved 2026-09-15)

The previous revision of this prompt asked the planner to discover the facts below from scratch. They have now been verified against first-party sources. Treat them as **starting evidence, not as permanent truth**: pin the exact versions you plan against, re-verify each item against those pinned versions, and record any drift. Where a fact is marked UNVERIFIED, closing it is part of your job.

### 3.1 Claude Code Channels (research preview)

Sources: `https://code.claude.com/docs/en/channels.md`, `https://code.claude.com/docs/en/channels-reference.md`, `https://code.claude.com/docs/en/mcp.md`, `https://code.claude.com/docs/en/hooks.md`.

- A channel is an MCP server that declares `capabilities.experimental["claude/channel"] = {}` and sends `notifications/claude/channel` with `content` (string) and `meta` (string-to-string map). Each `meta` key becomes an attribute on the `<channel>` tag Claude sees; keys must be identifier-safe (letters, digits, underscore) or they are silently dropped.
- Inbound notifications wake an idle session as a user turn. Notifications arriving mid-turn are queued and delivered together at the next turn, in order. Claude Code sends **no acknowledgement**; a resolved notification send means "written to transport", not "seen by the model".
- Outbound: the channel server exposes ordinary MCP tools (conventionally `reply`); correlation is by convention only, via `meta` attributes the server asks Claude to echo back.
- Loading: channels are passed at session start with `--channels plugin:<name>@<marketplace>` or `--channels server:<name>`. **A channel cannot be attached to an already-running session.** Behavior across `--resume` is UNVERIFIED. Multiple channels per session are allowed. Whether one server can present more than one logical channel is UNVERIFIED.
- Distribution: only plugins on Anthropic's allowlist (`claude-plugins-official`, or an org's `allowedChannelPlugins`) load without a flag. Anything else requires `--dangerously-load-development-channels` plus an interactive confirmation. Team/Enterprise orgs must set `channelsEnabled`. Not available on Bedrock, Vertex, or Foundry.
- Security: every approved channel plugin keeps a per-channel sender allowlist bootstrapped by pairing code. Optional permission relay (`claude/channel/permission`, v2.1.234+) lets any allowlisted sender approve or deny tool use in the session. Channel content is untrusted.
- Stability: research preview on Claude Code v2.1.232+. The flag syntax and protocol may change. The `--channels` flags do not appear in `claude --help`.
- **MCP version constraint:** channel servers that negotiate MCP protocol `2026-07-28` cannot deliver channel messages and are not registered as channels. Servers must negotiate a legacy revision (`2025-11-25` or earlier), optionally forced with `MCP_PROTOCOL_NEGOTIATION=legacy` for stdio servers.
- Session identity: hooks receive `session_id` in their input (for example `SessionStart`), which is the supported way for an external process to learn which live session it is talking to. No `CLAUDE_SESSION_ID` environment variable is documented (UNVERIFIED). Claude Code's own cross-session messaging (`ListAgents`/`SendMessage`) is a separate feature and is not integrated with Channels.
- Agent SDK support for Channels: UNVERIFIED, presumed absent.

### 3.2 Codex CLI App Server (experimental, per-method gating)

Sources: `https://learn.chatgpt.com/docs/app-server`, `https://learn.chatgpt.com/docs/cli/reference`, `https://learn.chatgpt.com/docs/hooks`, `https://learn.chatgpt.com/docs/config-file/config-advanced`, `https://github.com/openai/codex` (`codex-rs/app-server*`, `codex-rs/cli`, `codex-rs/tui`). Version context: `@openai/codex` 0.154.0 (2026-09-09).

- `codex app-server` speaks JSON-RPC 2.0 (header omitted on the wire) over stdio (JSONL), WebSocket (`--listen ws://127.0.0.1:PORT`), or Unix socket. Handshake is `initialize` then `initialized`; there is no numeric protocol version. Methods are individually stable or experimental; experimental ones require `capabilities.experimentalApi` and are documented as not durable.
- Thread and turn model: `thread/start`, `thread/resume`, `thread/list`, `thread/loaded/list`, `turn/start` (input array of `{type:"text",text}`), `turn/steer` (append to the in-flight turn), `turn/interrupt`. Thread ids are UUIDv7 strings and survive restarts. Rollouts live under `CODEX_HOME/sessions/`; the rollout file format is explicitly not a supported surface.
- Events: `thread/started`, `thread/status/changed`, `turn/started`, `turn/completed` (status `completed|interrupted|failed`), `item/started`, `item/completed` (authoritative), `item/agentMessage/delta`, plus approval requests as server-to-client JSON-RPC requests.
- **Live-session injection exists but is narrow and experimental.** A shared local daemon (`codex app-server daemon start`, control socket `CODEX_HOME/app-server-control/app-server-control.sock`) can host threads; the TUI, when launched without config overrides, attaches to that daemon as a client if it is running, otherwise embeds its own server. Any client of the same daemon can then deliver input to a live thread with `thread/queue/add` (queued until idle; experimental), `turn/steer` (into the active turn), or `turn/start` (when idle). The CLI wrapper is `codex queue --thread <id> --message <text>` (merged 2026-08-17, not yet in the CLI reference). Whether implicit daemon attach is enabled in released 0.154.0 versus only on `main` is UNVERIFIED and is a go/no-go item.
- **Cross-process resume does not attach.** A second app-server process resuming a thread another process holds loads history from disk and appends silently; the live process is not notified (open issue #21743). Codex Desktop uses a private stdio app-server and does not expose the control socket (UNVERIFIED for current builds).
- Remote TUI: `codex --remote ws://…` attaches the TUI to an app-server that OAC could own. This is the documented fallback if daemon attach fails the gate.
- Removed surface: `codex mcp-server` (Codex as an MCP server) was deprecated 2026-08-20 and deleted 2026-09-05. Do not plan on it. `codex mcp add` still registers **external** MCP servers that Codex can call as tools; that is the supported outbound tool surface.
- Hooks (`SessionStart`, `UserPromptSubmit`, `Stop`, …) and `notify` (`agent-turn-complete` with `thread-id`, `turn-id`, `last-assistant-message`) react to events and cannot originate a turn. `codex exec` and the TypeScript SDK spawn their own process per run and cannot inject into another process's session.
- Auth: the app-server uses the saved CLI login (`CODEX_HOME/auth.json` or OS keyring). An OAC adapter never holds OpenAI credentials. WebSocket listeners support capability-token or signed-bearer auth; Unix sockets validate peers.
- Schema artifacts are checked in (`codex-rs/app-server-protocol/schema/json`, `schema/typescript`) and regenerable with `codex app-server generate-json-schema`. Repo license Apache-2.0. Rust crates `app-server-client`, `app-server-protocol`, `app-server-transport` exist and are reusable under that license.

### 3.3 Model Context Protocol

Sources: `https://modelcontextprotocol.io/specification/2026-07-28/`, `https://modelcontextprotocol.io/extensions/overview`, `https://modelcontextprotocol.io/seps/2133-extensions`.

- Current revision `2026-07-28`; `2025-11-25` and earlier are legacy with a documented dual-era interoperability matrix. The current revision is stateless: `initialize` is gone, every request carries `_meta["io.modelcontextprotocol/protocolVersion"]`, and **servers cannot initiate requests or push unsolicited content into the model's context** (server-initiated requests were replaced by multi-round-trip results on `tools/call`, `prompts/get`, `resources/read`). Only subscription-gated `list_changed`/`resources/updated`, request-scoped progress, and deprecated logging notifications remain.
- Consequence: **no MCP revision defines "external event becomes a user turn". That semantic is always provider-native.** The OAC spec can be packaged as an MCP extension for capability negotiation, tool surface, and `_meta` provenance, but it must not claim MCP as the delivery mechanism.
- Extensions are formal (SEP-2133, final 2026-01-26): identifier `{reverse-dns-prefix}/{name}` (for example `io.github.<owner>/oac`), negotiated through the `extensions` map in capabilities; breaking changes require a new identifier. `experimental` capabilities still exist and are what Claude Channels use. There is no spec rule for namespacing custom method names; only `_meta` keys and extension identifiers have prefix rules. Prefixes whose second label is `modelcontextprotocol` or `mcp` are reserved.
- No SEP or working-group item for agent-to-agent messaging exists as of the retrieval date.

### 3.4 Zenoh

Sources: `https://github.com/eclipse-zenoh/zenoh` (DEFAULT_CONFIG.json5, releases), `https://zenoh.io/docs/manual/{access-control,tls,quic,user-password,configuration}/`, `https://zenoh.io/docs/getting-started/deployment/`, binding repositories.

- Stable release 1.10.1 (2026-09-07), dual EPL-2.0 / Apache-2.0. Stable API surface is `zenoh` and `zenoh-ext` only; several features sit behind the `unstable` flag.
- Bindings with a native in-process runtime capable of router-less peer mode: Rust, Python (PyO3 wheels for win_amd64, macOS, manylinux; no Windows arm64), C, C++, Java/Kotlin (JNI, Windows MSVC supported), Go (CGo, needs unstable API). **TypeScript is not native**: `zenoh-ts` requires a `zenohd` router with the remote-api plugin and does not run on Node.js. C# has no code. Dart is third-party and pre-1.0.
- Peer mode is default. Discovery is UDP multicast scouting on `224.0.0.224:7446` (`interface: "auto"`, single interface) plus gossip. **Same-host discovery over loopback was broken before 1.10.0** (PR #2671); plan on ≥1.10.0. Windows binds the scouting socket to `0.0.0.0` with `SO_REUSEADDR`; `#iface=` endpoint parameters are documented for Linux only. Peers listen on dynamic TCP ports by default, so multiple peers per host do not collide.
- Primitives: pub/sub, queryables, liveliness tokens with history-capable liveliness subscribers. Storage and other plugins run only inside `zenohd` (router). Advanced publisher/subscriber (cache, retransmission, miss detection) is in `zenoh-ext` behind `unstable`.
- Security: TLS/mTLS and QUIC listeners with per-endpoint certificates; user/password auth; public-key auth config keys exist but semantics are undocumented (UNVERIFIED). Access control rules are keyed by interface, certificate common name, username, link protocol, or `zid`; **`zid` subjects are explicitly unauthenticated and unfit for production**. Endpoints can be bound to `127.0.0.1`; multicast scouting can be disabled. Config is JSON5 or programmatic.
- **Zenoh provides no application-layer message signing.** Authenticity of an OAC envelope must be established by OAC itself.
- Packaging: a Rust binary embeds the runtime with no daemon. Windows MSVC and GNU artifacts ship. Expect a single OAC binary in the 5 to 15 MB range (derived, UNVERIFIED).

### 3.5 Cursor and ACP (forward-compatibility only)

Sources: `https://cursor.com/docs/cli/acp`, `https://agentclientprotocol.com/protocol/`, `https://github.com/agentclientprotocol/`.

- ACP is the Agent Client Protocol (originated at Zed, now under the `agentclientprotocol` org). Stable protocol version `1`; schema v2 is alpha. Cursor CLI runs as an ACP agent with `agent acp` and accepts `session/prompt` into a session **owned by the ACP client**; `session/load` and `session/resume` exist. Custom methods start with `_`; unknown notifications should be ignored.
- No documented API pushes input into a live local Cursor session that OAC does not own. Cloud Agents accept follow-up runs by HTTP and reject concurrent runs.
- ACP adapters exist for Codex (wraps the app-server) and for the Claude Agent SDK (not the Claude Code CLI). ACP is a candidate neutral "client-owned session" surface for later providers, not a v0.1 dependency.

## 4. Gating findings and the go/no-go gates you must define

The baseline already establishes one structural finding. State it up front in the plan and design around it:

> **Neither Claude Code nor Codex supports attaching an external channel to an arbitrary, already-running session process through supported interfaces.** Both support live, active, full-duplex injection into a session that was **launched OAC-enabled**: Claude Code by starting with `--channels`, Codex by running through the shared local app-server daemon (or an OAC-owned app-server with the TUI attached via `--remote`).

Therefore redefine "existing session" in ADR-001's validation criterion as "a live interactive session, launched OAC-enabled, whose owner is a human at a terminal, into which OAC injects without owning the harness's model loop". Propose that as ADR-001-A2. The boundary that matters is unchanged: OAC never owns inference, credentials, or the turn loop.

Define these gates with explicit pass criteria, failure criteria, and a fallback for each. The build phase may not proceed past a gate on a "probably":

- **G1 Claude wake.** A development-flag channel server delivers a notification that wakes an idle Claude Code session and appears as a `<channel>` tag; a second notification sent mid-turn is delivered at the next turn; Claude replies through a tool. Include the legacy MCP negotiation requirement and the interactive confirmation dialog in the pass criteria. Failure means the Claude adapter is blocked; there is no supported fallback, so this is v0.1 go/no-go.
- **G2 Codex live inject.** On the pinned released version, a TUI launched normally attaches to a running daemon, and a second client delivers a message that the TUI user sees and the model answers, using `thread/queue/add` or `turn/start`. Fallback if implicit attach is not released: OAC owns the app-server and the user runs `codex --remote`. Failure of both is v0.1 go/no-go.
- **G3 Zenoh local peer.** Two peers on one host discover each other over loopback with multicast, and alternatively with multicast disabled using a locally shared rendezvous endpoint, on Windows 11, macOS, and Linux, with a TLS listener bound to localhost. Fallback: fixed local endpoint without scouting.
- **G4 MCP dual-era server.** One OAC MCP server process serves Claude's legacy-revision channel path and a `2026-07-28` tool path (for Codex via `codex mcp add`) without breaking either. Fallback: two server entry points sharing one core.
- **G5 Provenance.** A message whose text claims a different sender is rendered to the model with machine-set provenance that contradicts the claim, on both providers.

Each gate produces a written result with version, date, command transcript summary, and verdict. Gates are re-run whenever a pinned provider version changes.

## 5. Decisions you must make

For each decision give the choice, the decisive evidence, the rejected alternatives in one line each, and the condition that would reverse it. Do not leave routine choices as open alternatives.

1. **Language and runtime.** Presumptive answer: Rust, single static binary. Evidence: native Zenoh peer mode is available only in Rust, Python, C/C++, JVM, and Go; the Codex app-server client crates are Rust and Apache-2.0; single-binary cross-platform packaging is required by ADR-001. Reverse if you find a disqualifying gap in the Rust MCP SDK's legacy-revision support or in Windows keychain access.
2. **Process model.** Choose between (a) one long-lived `oac` daemon per device that hosts the Zenoh peer, identity, policy, and the Codex client, with thin stdio MCP shims that Claude Code and Codex spawn and that connect to the daemon over local IPC; or (b) a self-contained MCP server per harness session that embeds its own Zenoh peer. Weigh: Claude spawns its channel server as a child process; presence must survive individual session exit; key material should live in one process; local IPC on Windows means named pipes. State the choice and the IPC mechanism.
3. **Spec packaging.** Standalone normative document plus an MCP extension identifier for negotiation and `_meta` provenance, versus MCP-only. Choose the reverse-DNS prefix and justify ownership.
4. **Session identity, addressing, and discovery.** Opaque stable session ids bound to a device key; how Claude `session_id` (from hooks) and Codex `threadId` are captured and registered; what survives `--resume`/`thread/resume`; display URI form.
5. **Envelope authenticity and replay.** Per-device signing keys, signature scope (which envelope fields), timestamp plus nonce window, duplicate suppression store, and what `DeliveryReceipt` states mean given that Claude gives no acknowledgement (distinguish "accepted by adapter", "handed to harness", and "unknown" honestly).
6. **Pairing and authorization.** Default deny; pairing flow for same-user multi-harness on one device (should be zero-config) and for two devices on a LAN (short code or file exchange); per-session sender allowlists; how OAC policy maps onto Zenoh ACL subjects given that only certificate common names and usernames are authenticated there.
7. **Key storage.** OS credential stores (Windows Credential Manager, macOS Keychain, Linux Secret Service) with an encrypted-file fallback; state which library and its license.
8. **Provider-facing trust rendering.** Exactly what the model sees for an inbound OAC message on each provider: Claude `meta` attributes for sender, device, session, message id, and reply target; Codex text-input framing with a machine-generated header and delimited untrusted body. Permission relay on Claude is off by default in v0.1; justify or reverse.
9. **Outbound symmetry.** Both harnesses send through OAC MCP tools (`send`, `reply`, `list_sessions`, `whoami`); inbound is provider-native. Confirm this holds for Codex through `codex mcp add`, and define how a Codex reply is correlated when Codex has no channel-tag convention.
10. **Transport mapping.** Zenoh key-expression layout, liveliness for presence, local-mode security (localhost-bound listener, scouting on or off), LAN-mode security (TLS or QUIC with pairing-issued certificates), and the exact module boundary that keeps Zenoh types private.
11. **Configuration and CLI model.** Zero-file local default; `oac start|status|sessions|doctor` plus whatever the process model needs (for example `oac mcp-shim`); how a user launches Claude and Codex OAC-enabled with one documented command each.
12. **Dependencies and licenses.** Every third-party crate with license and why it is needed. OSS-compatible with Apache-2.0; flag anything copyleft.

Provider-specific or Zenoh-specific concepts may not appear in neutral core interfaces. If unavoidable, document the exception and the reason.

## 6. Design-for-replacement proofs

Show from the proposed interfaces, not from prose, that:

- a third provider adapter (use ACP as the worked example) can be added with no change to any transport module;
- NATS or MQTT can replace Zenoh with no change to any adapter or to the OAC spec, and list which optional transport capabilities (persistence, ordering, multicast discovery) each candidate lacks;
- the core, the spec conformance tests, and the security tests run in CI with fake harness endpoints and recorded protocol fixtures, with no live provider, no API key, and no network beyond loopback;
- provider integration tests are isolated behind an explicit opt-in and pinned versions.

## 7. Security requirements the plan must satisfy

Produce a threat model that covers at least the DESIGN "Security" threat list and these additions surfaced by the baseline:

- Zenoh has no payload authentication, so envelope signatures are the only authenticity proof; Zenoh `zid` is not an identity.
- Anyone able to reply on a Claude channel can approve tool use if permission relay is enabled.
- Codex `turn/steer` writes into an in-flight turn; unauthorized steer is a code-execution risk.
- The `--dangerously-load-development-channels` confirmation is the only user consent step on the Claude side until OAC is on an allowlist; the plan must not weaken it.
- Model-generated text never establishes identity; provenance is machine-set metadata separate from content on both providers.
- Local IPC between shims and daemon must authenticate the peer process (Windows named-pipe security descriptors, Unix socket permissions).
- Cross-project leakage: a session in one working directory must not be discoverable by a peer that is not authorized for it.

For each threat: attack, precondition, mitigation, which test proves the mitigation, and residual risk.

## 8. The factory: stage-gate pipeline the plan must lay out

Lay the plan out as the following stages. Each stage has entry criteria, exit artifacts, and a gate. Order is risk-first: no substantial core or transport code before the provider gates pass. Timebox the spikes and say what happens when a timebox expires.

- **Stage 0 — Evidence and pinning.** Pin exact versions of Claude Code, Codex, MCP revisions, Zenoh, and the Rust toolchain. Re-verify §3 against those pins. Output: capability matrix, gating findings, ADR amendments, conflict register resolved.
- **Stage 1 — Provider and transport spikes.** Throwaway spikes for G1 through G5. Output: gate results with transcripts; recorded protocol fixtures captured from the real harnesses for later fake endpoints.
- **Stage 2 — Normative spec v0.1.** OAC Session Channels spec and security spec, with a conformance fixture set and a versioning policy. Interfaces (adapter, transport, core types) frozen at the end of this stage. Output: `spec/` documents reviewed against ADR boundaries.
- **Stage 3 — Core and fakes.** Core types, policy, identity, duplicate suppression, fake Claude and fake Codex endpoints built from Stage 1 fixtures, in-memory transport, contract test suites. Output: CI green with no providers.
- **Stage 4 — Adapters and Zenoh transport.** Built against the frozen contracts. Output: contract suites pass on real modules; provider integration tests pass on pinned versions behind opt-in.
- **Stage 5 — End-to-end and threat verification.** The ADR validation criterion executed on Windows, macOS, and Linux; spoof, replay, unauthorized-routing, disconnect, and duplicate tests; CLI smoke. Output: DESIGN acceptance criteria 1 through 10 each mapped to a passing test.
- **Stage 6 — Release hygiene.** Docs, license inventory, deferred-work list, known-risk list, upgrade notes for provider preview surfaces.

For every stage specify: modules created or changed (by name and responsibility, not code), prerequisite decisions from §5, the executable demonstration, acceptance criteria, and the go/no-go condition. Do not decompose stages into tickets; that is the build phase's job.

## 9. Required output package

Write the package under `docs/planning/v0.1/` as separate files, each self-contained, cross-referenced by path:

1. `00-summary.md` — one page: what v0.1 proves, the structural finding from §4, the language and process-model decisions, the top three risks.
2. `01-capability-matrix.md` — Claude Code, Codex, MCP, Zenoh, ACP: capability, supported or preview or experimental, version pinned, source, date, gap.
3. `02-gating-findings.md` — G1 to G5 with pass, fail, fallback, and current verdict.
4. `03-decisions-and-amendments.md` — every §5 decision; ADR-001-A1 onward with old text, new text, rationale; the resolved conflict register from Appendix A.
5. `04-architecture.md` — components, process boundaries, data and control flow for inbound and outbound on each provider, local and LAN deployment topologies. Diagrams in text.
6. `05-interfaces.md` — OAC spec surface (envelope, addressing, capabilities, presence, delivery states, errors, versioning, unsupported-capability behavior), provider adapter contract, transport contract, core types. Normative language (MUST/SHOULD/MAY) for spec items; clearly separated reference-implementation notes.
7. `06-security.md` — identities, trust boundaries, pairing, authorization, transport security, replay and duplicate handling, provenance rendering per provider, threat table per §7.
8. `07-repository-and-dependencies.md` — module layout, ownership boundaries, third-party inventory with licenses.
9. `08-cli-and-deployment.md` — zero-container local path first; exact user-facing launch story for each harness; LAN hooks only where they shape v0.1 architecture.
10. `09-test-strategy.md` — unit, spec conformance, contract, security, fake-harness integration, provider integration, end-to-end, cross-platform smoke; what runs in CI by default and what is opt-in.
11. `10-stages.md` — the §8 pipeline with gates.
12. `11-risks.md` — ranked by ability to invalidate v0.1, each with an early-warning signal and a response.
13. `12-deferred.md` — every ADR non-goal and v0.1 exclusion, restated as an explicit "not in v0.1" with the reason.

Concise beats complete-looking. A section that is a list of verified facts and decisions is worth more than a section that reads well.

## 10. Planning rules and evidence standard

- Cite first-party sources only for capability claims: official docs, the provider's source repository, release notes. Blog posts and third-party wrappers may inform, not decide.
- Every cited fact carries URL, version or commit, and retrieval date. Facts you could not verify are labeled UNVERIFIED and appear in `11-risks.md`.
- Quote protocol method names, capability keys, flags, and file paths verbatim from the source. Never invent an API name.
- Label every provider surface as supported, research preview, experimental, or undocumented. Preview and experimental surfaces get a compatibility shim boundary and a pinned version.
- Prefer the smallest implementation that proves v0.1. When two designs both satisfy the ADR, choose the one with fewer moving parts and say so.
- Separate normative OAC semantics from reference-implementation choices in every interface section.
- Do not turn OAC into an agent framework, inference router, shared context manager, or provider-auth abstraction. If a decision drifts that way, stop and cite the ADR boundary.
- Do not substitute model APIs, UI automation, terminal scraping, credential reuse, private RPCs, or rollout-file manipulation for a supported interface. If a supported interface is missing, that is a finding, not a workaround.
- Write in plain prose. No tickets, no code, no pseudo-code beyond interface signatures and message shapes.

## 11. Self-review before delivering

Before you finish, check the package against each item and fix what fails:

1. Every ADR-001 boundary and every DESIGN non-goal is either respected or has a numbered amendment.
2. Every DESIGN v0.1 acceptance criterion (1 to 10) maps to a named test in `09-test-strategy.md` and a stage in `10-stages.md`.
3. Every §5 decision is made, not deferred, or has a gate whose result decides it.
4. Every gate G1 to G5 has pass, fail, and fallback text.
5. Every §3 fact you rely on has been re-verified against the pinned version, and every UNVERIFIED item is closed or listed as a risk.
6. No neutral interface mentions Zenoh, Claude, Codex, MCP method names, or key expressions.
7. No section proposes owning a harness's turn loop, holding provider credentials, or polling an inbox from an adapter that claims active inbound.
8. The naming resolution in §2 is applied consistently.

## Appendix A — Conflict and ambiguity register (seed)

Resolve each entry in `03-decisions-and-amendments.md`. Add entries you discover.

| # | Conflict or ambiguity | Where | Evidence | Expected resolution |
|---|---|---|---|---|
| C1 | Product named OAC in repo, "Session Channels" in ADR and DESIGN, CLI `sessionchannels` | README, ADR-001, DESIGN | §2 | ADR-001-A1 rename |
| C2 | ADR validation criterion says "existing Claude Code session" and "existing Codex harness session" | ADR-001 Validation criterion | §3.1, §3.2: no attach to arbitrary running process | ADR-001-A2 redefine as OAC-enabled launch |
| C3 | ADR names the layer an "MCP Session Channels extension" | ADR-001 Decision | §3.3: MCP cannot carry server-to-model push; extension identifiers now formal | Decision 3; possibly ADR-001-A3 wording |
| C4 | DESIGN envelope `security.signature` is "implementation-defined" while acceptance criterion 6 requires enforced provenance | DESIGN envelope and acceptance | §3.4: Zenoh has no payload signing | Make signature normative in the spec |
| C5 | Claude channel path needs legacy MCP; Codex tool path may negotiate current MCP | §3.1, §3.3 | Gate G4 | Decision 2 and 3 |
| C6 | Claude gives no delivery acknowledgement; DESIGN lists `accepted` as a delivery state | §3.1, DESIGN Delivery semantics | Decision 5 | Define receipt states by what is knowable |
| C7 | ADR treats Cursor as "ACP"; ACP is a client-owned-session protocol like the app-server, not a channel | §3.5 | Forward-compat check only | Note in capability matrix |
| C8 | DESIGN suggests session ids may map to a URI containing device and harness; ADR requires no leak of transport concepts | DESIGN Addressing | Decision 4 | Opaque id plus display form |
| C9 | Codex outbound has no channel-tag convention; DESIGN assumes symmetric reply and correlation | §3.2 | Decision 9 | Define Codex correlation explicitly |
| C10 | Claude permission relay lets any allowlisted sender approve tools | §3.1 | §7 | Off by default in v0.1 |

## Appendix B — Source index

Claude Code: `https://code.claude.com/docs/en/channels.md`, `https://code.claude.com/docs/en/channels-reference.md`, `https://code.claude.com/docs/en/mcp.md`, `https://code.claude.com/docs/en/hooks.md`, `https://code.claude.com/docs/en/cross-session-messaging.md`.
Codex: `https://learn.chatgpt.com/docs/app-server`, `https://learn.chatgpt.com/docs/cli/reference`, `https://learn.chatgpt.com/docs/hooks`, `https://learn.chatgpt.com/docs/non-interactive-mode`, `https://learn.chatgpt.com/docs/config-file/config-advanced`, `https://github.com/openai/codex` (`codex-rs/app-server`, `codex-rs/app-server-daemon`, `codex-rs/app-server-protocol/schema`, `codex-rs/cli/src/queue_cmd.rs`, `codex-rs/tui/src/lib.rs`), issues #21743, #16614, #33957, #45251, PRs #39092, #39657, #42993.
MCP: `https://modelcontextprotocol.io/specification/2026-07-28/changelog`, `.../basic/versioning`, `.../basic/patterns/mrtr`, `https://modelcontextprotocol.io/extensions/overview`, `https://modelcontextprotocol.io/seps/2133-extensions`.
Zenoh: `https://github.com/eclipse-zenoh/zenoh` (releases, `DEFAULT_CONFIG.json5`, PR #2671, issue #1432), `https://zenoh.io/docs/manual/access-control/`, `https://zenoh.io/docs/manual/tls/`, `https://zenoh.io/docs/manual/quic/`, `https://zenoh.io/docs/manual/user-password/`, `https://zenoh.io/docs/manual/plugins/`, `https://github.com/eclipse-zenoh/zenoh-ts`, `https://github.com/eclipse-zenoh/zenoh-python`.
ACP and Cursor: `https://agentclientprotocol.com/protocol/overview`, `.../protocol/extensibility`, `https://github.com/agentclientprotocol/agent-client-protocol`, `https://github.com/agentclientprotocol/codex-acp`, `https://cursor.com/docs/cli/acp`.
