# Architecture

OAC's design separates provider integration, neutral message semantics, and transport.
This is an architectural plan; consult [Status](https://github.com/RossGraeber/OAC/blob/main/docs/planning/STATUS.md)
for what has been validated.

```text
       OAC-enabled harness sessions
          Claude Code     Codex
               |            |
             Provider adapters
                      |
             OAC Session Channels
                      |
                Transport API
                      |
          Zenoh reference transport
```

## Layers

| Layer | Responsibility |
|---|---|
| Provider adapters | Translate neutral messages into harness-native live-session input and replies |
| OAC Session Channels | Identity, addressing, capabilities, message correlation, presence, delivery, and security semantics |
| Transport | Carry messages without exposing transport-specific concepts in the neutral protocol |

[Design](https://github.com/RossGraeber/OAC/blob/main/docs/planning/DESIGN.md) and the
[architecture plan](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/04-architecture.md) hold the detailed contracts.
Provider surface labels, versions, and compatibility findings belong in the
[capability matrix](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/01-capability-matrix.md)
and [pins](https://github.com/RossGraeber/OAC/blob/main/docs/planning/PINS.md); this overview does not establish new compatibility claims.

## Trust and ownership

[Communication](Communication) follows a message and reply through these layers.
[Security](Security) explains the checks at each boundary and current evidence gaps.

The harness owns inference, provider authentication, context, tools, and policy.
OAC's design owns message delivery and enforceable sender provenance.
Authenticated peer messages remain untrusted content.

OAC must not substitute model APIs for a harness, manage shared context, reuse another
harness's credentials, depend on scraping or private RPCs for supported integrations,
or expose Zenoh-specific concepts in the neutral protocol.
See [ADR-001](https://github.com/RossGraeber/OAC/blob/main/docs/planning/ADR-001.md) and
[the security plan](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/06-security.md).

## Scope

The v0.1 plan covers direct text messages, replies, session identity, active push,
presence, authorization, Claude and Codex adapters, a Zenoh transport, and local CLI use.
Group rooms, attachments, durable offline mailboxes, federation, full end-to-end
encryption, a GUI, other production adapters, and alternative transports are deferred.

[Deferred scope](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/12-deferred.md) records those decisions.
