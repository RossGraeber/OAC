# Communication

This guide describes the planned application. OAC is currently in its provider and
transport spike stage, before a released CLI. [Status](https://github.com/RossGraeber/OAC/blob/main/docs/planning/STATUS.md)
records what the experiments have verified.

## Message flow

```text
Sender harness -> adapter/shim -> local daemon
                                     |
                                  transport
                                     |
Receiving harness <- adapter <- receiving daemon
```

On one device, OAC-enabled sessions share the local daemon; a second device adds a
transport crossing between daemons. The receiving side verifies the signed envelope,
checks replay and authorization, then hands machine-set provenance and untrusted text
to the receiving harness. Replies travel back through the same layers.

The harness decides what to do and whether to reply. OAC does not generate answers,
select models, or combine harness contexts.
See the [architecture plan](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/04-architecture.md).

## Sessions and addressing

Sessions must be launched OAC-enabled. Registration binds an opaque OAC session ID
to the harness-native session. Human aliases are display labels, not authority.
Messages address a particular session; discovery and delivery are filtered by
session authorization and the registered working directory.

Presence has three planned states: `online`, `unreachable`, and `unknown`.
Presence does not prove that a model is idle or has read a message.
Learning previously unknown session IDs and the full presence mapping remain
design gaps in the [interface plan §6](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/05-interfaces.md).

## Message contents and replies

| Envelope part | Purpose |
|---|---|
| Message ID, sender ID, target ID | Identify the envelope and address a session |
| Text content | Carry the sender's untrusted message |
| Reply and correlation fields | Associate messages and replies |
| Creation time and optional TTL | Bound freshness and application-level expiry |
| Signed security fields | Bind sender identity and content to the authenticated device key |

The [interface plan §3–§4](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/05-interfaces.md)
defines exact fields and optionality. This overview does not define another wire format.

A model-supplied reply-correlation claim is checked against independent adapter state.
A mismatch downgrades correlation rather than overwriting the adapter's state.
See [interface plan §8](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/05-interfaces.md).

## Active input and delivery receipts

An adapter claiming active inbound support must use live harness input;
application-level inbox polling does not satisfy that claim.
Busy-session delivery timing depends on the provider surface and recorded experiments.
Unsupported capabilities must be reported explicitly.

| Planned receipt | Meaning |
|---|---|
| `accepted-by-adapter` | Signature, replay/duplicate, and authorization checks passed |
| `handed-to-harness` | The adapter completed its provider-specific delivery call |
| `unknown` | No stronger observation is available |
| `rejected`, `unreachable`, `expired`, `duplicate`, `failed` | Policy, reachability, expiry, duplicate, or execution failure |

**Handed to the harness does not mean seen by the model**, or that a requested task
was completed. Exactly-once delivery is not promised. Durable offline mailboxes,
attachments, rooms, and broadcasts are deferred.
See [interface plan §7–§10](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/05-interfaces.md).

## Deployment and evidence

The local design shares a daemon under the same OS user. The LAN design adds
device pairing and encrypted transport; federation is deferred.
See the [CLI/deployment plan](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/08-cli-and-deployment.md).

G1/G2 validate provider input mechanisms, G3 a transport spike, and G4 MCP-era
coexistence. These experiments do not prove that eventual OAC adapters implement the
complete flow. Review [gate evidence](https://github.com/RossGraeber/OAC/blob/main/docs/planning/gates/README.md)
and [Security](Security) for limitations.
