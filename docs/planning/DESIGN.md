# Session Channels — Software Design

## Purpose
An open-source communication layer for secure, event-driven, full-duplex messaging between **existing provider harness sessions**. It is not an agent framework or model API router.

## Goals
- Preserve provider-native inference, authentication, policy, tools, and context.
- Active inbound delivery with no application-level polling.
- Full duplex, authenticated and authorized session communication.
- Provider adapters independent of transports.
- One-command local CLI deployment; no mandatory containers/cloud.
- Pluggable transports.

## Non-goals
Model inference/routing, agent planning, shared context management, replacement provider auth, or unsupported client impersonation.

## Components
### CLI / supervisor
Starts configuration, device identity, adapters, transport, diagnostics, and clean shutdown.
```text
sessionchannels start
sessionchannels status
sessionchannels sessions
sessionchannels doctor
```

### Core
Neutral types should include `SessionIdentity`, `SessionDescriptor`, `SessionCapabilities`, `ChannelMessage`, `DeliveryReceipt`, `PresenceRecord`, `SecurityPrincipal`, and authorization decisions.

### MCP Session Channels extension
Defines extension/version negotiation, session addressing, capabilities, message envelope, active-delivery semantics, replies/correlation, presence/discovery, acknowledgements/errors, identity/security, and unsupported-capability behavior.

Normative concept: **a harness advertising active inbound Session Channels support accepts an authorized external channel message as input to the addressed live session without application-level polling.**

The specification MUST NOT mention Zenoh keys, MQTT topics, NATS subjects, or provider-specific method names.

### Provider adapter contract
Conceptually:
```text
ProviderAdapter
  discover_sessions()
  attach(session)
  deliver(session, message)
  publish_output(callback)
  capabilities(session)
  health()
  shutdown()
```
Adapters should route through core policy/security rather than directly through transports.

**Claude adapter:** Claude Code Channels/MCP <-> neutral messages. Keep trusted sender metadata distinct from untrusted content.

**Codex adapter:** App Server threads/turns/events <-> neutral sessions/messages. Never call the OpenAI model API directly.

### Transport contract
Conceptually:
```text
Transport
  start(identity, config)
  publish(destination, envelope)
  subscribe(address, handler)
  announce_presence(record)
  watch_presence(handler)
  health()
  shutdown()
```
Optional capabilities: reliability, persistence, offline queueing, ordering, multicast discovery, routing/federation.

### Zenoh transport
v0.1 reference transport. Prefer local/peer mode. Session addressing may map privately to Zenoh key expressions; messages to pub/sub; neutral presence to Zenoh liveliness/discovery. Zenoh types must not escape the transport module.

## Preliminary message envelope
```json
{
  "version": "0.1",
  "id": "unique-message-id",
  "from": "session-address",
  "to": "session-address",
  "conversation_id": "optional-thread",
  "reply_to": "optional-message-id",
  "correlation_id": "optional-correlation-id",
  "created_at": "timestamp",
  "ttl_ms": 300000,
  "content": [{"type":"text","text":"Please review scheduler.rs"}],
  "security": {
    "principal": "authenticated-principal-reference",
    "key_id": "device-public-key-fingerprint",
    "nonce": "128-bit-csprng-value",
    "signature": "ed25519-signature-over-domain-separated-jcs-canonicalization"
  }
}
```
Signature is normative, not implementation-defined: Ed25519 (`ed25519-dalek`), verified
with `verify_strict`, over a domain-separated RFC 8785 JCS canonicalization of every
envelope field except `security.signature` itself. Full decision, evidence, and the
normative-text forecast for `spec/security.md`:
`docs/planning/decisions/C5-envelope-auth.md` §2-§6.

Trusted security metadata must be distinguishable from user/model-controlled content.

## Addressing
Do not expose broker-native addresses publicly. Consider opaque stable session IDs with descriptive metadata; a URI such as `session://device/harness/id` may be useful for display. Human aliases are separate from stable/cryptographic identity.

## Presence/discovery
Answer: which sessions are reachable, which harness owns them, capabilities, and whether active inbound is supported. Start with `online`, `unreachable`, `unknown`; avoid over-normalizing provider-specific activity states.

## Delivery semantics
At minimum: `accepted`, `rejected`, `unreachable`, `expired`, `duplicate`, `failed`. Do not promise exactly-once delivery. Use unique IDs, idempotency, and duplicate suppression.

Provider-supported streaming connections, subscriptions, event loops, and keepalives are acceptable; application-level inbox polling is not for adapters claiming active inbound support.

## Security
Threats include impersonation, unauthorized routing/discovery, tampering, replay, malicious peer prompt injection, compromised transport infrastructure, accidental cross-project disclosure, and leaked credentials.

Use cryptographic device/principal identity and bind ephemeral harness/session identities to it. Default deny. Local mode should expose only locally where practical and require no manual certificate management. LAN/remote mode requires authenticated encryption and explicit pairing/trust establishment.

## Configuration
Example only:
```toml
[transport]
type = "zenoh"
mode = "local"

[adapters.claude]
enabled = true

[adapters.codex]
enabled = true

[security]
default_policy = "deny"
```
Local defaults should work without editing a file.

## Suggested repository shape
```text
spec/
  session-channels.md
  security.md
ADR/
core/
cli/
transports/zenoh/
adapters/claude/
adapters/codex/
tests/{protocol,security,integration}/
examples/
docs/
```
Do not choose implementation language solely from this sketch; research provider SDK/IPC and Zenoh bindings first.

## Testing
Protocol serialization/validation; adapter contract tests with fake endpoints; transport contract tests; spoof/replay/unauthorized-routing tests; disconnect/reconnect/duplicate tests; cross-platform CLI smoke tests; and an end-to-end Claude -> Zenoh -> Codex -> Zenoh -> Claude test.

## v0.1 acceptance criteria
1. One-command local startup.
2. Claude and Codex adapters expose distinct neutral sessions.
3. Sessions discover one another through neutral APIs.
4. Claude actively messages Codex without receiver polling.
5. Codex actively replies to Claude.
6. Authenticated provenance and authorization are enforced.
7. Replay/duplicate handling exists.
8. No cross-provider model API invocation.
9. Zenoh-specific types stay inside its transport module.
10. Transport contract is documented enough to independently add a second backend.

## Planning questions
- Exact current MCP Extension conventions and compatibility requirements.
- Exact Claude Code Channel lifecycle/custom-channel constraints.
- Whether Codex App Server can attach to arbitrary already-running CLI/Desktop sessions or must own the session from creation.
- How session IDs survive resume/restart for each harness.
- Best Zenoh local discovery/security configuration on Windows/macOS/Linux.
- Language choice based on SDK quality, single-binary packaging, and cross-platform support.
- Identity key storage using OS keychains/credential stores.
- Whether v0.1 needs persistence or only live delivery.
