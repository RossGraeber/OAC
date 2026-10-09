# Security

These are planned application controls, not a claim that a released implementation
has passed security tests. [Status](https://github.com/RossGraeber/OAC/blob/main/docs/planning/STATUS.md) is the evidence ledger.

**Authenticated peer messages remain untrusted content and may contain prompt
injection.** Authentication identifies the sender. Authorization permits delivery.
Neither automatically authorizes the receiving harness to obey the body.
See [security plan §2](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/06-security.md).

## Identity and keys

```text
Security principal -> Device -> Harness -> Session
```

The design uses one Ed25519 keypair per device. Session registration derives its
authority from that device key; sessions do not hold separate signing keys.
The daemon binds opaque OAC IDs to harness-native sessions.
Aliases and identity claims in message text do not establish authority.

OAC device keys use the planned OS credential-store path with an encrypted-file
fallback. Provider login remains each harness's responsibility; OAC does not reuse
another harness's provider credentials.
See [identity decision](https://github.com/RossGraeber/OAC/blob/main/docs/planning/decisions/C4-session-identity.md)
and [security plan §1](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/06-security.md).

## Trust boundaries

| Boundary | Planned check | Limit |
|---|---|---|
| Shim → local daemon | Windows named-pipe security descriptors or Unix socket permissions and OS peer credentials | Same-user process identity alone does not prove a claimed harness/session |
| Device → device transport | Encryption and coarse transport access control | Transport identity does not authenticate message content by itself |
| Envelope → daemon | Signature, freshness, duplicate suppression, and session authorization | Successful verification grants delivery, not permission to obey |
| Adapter → harness | Machine-set provenance separated from untrusted text | Rendering alone does not eliminate prompt injection |

Envelope verification remains required after transport checks.
Transport-specific identifiers are not OAC security principals.
See [security plan §3 and §6](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/06-security.md).

## Pairing and authorization

Same-user local integrations rely on authenticated local IPC. LAN pairing establishes
trust between device keys through an operator-confirmed code.
**Pairing does not grant blanket access to every session on a device.**

Discovery and delivery start denied. Explicit sender allowlists address particular
OAC session IDs and are scoped by registered `working_directory`.
A new session gets a new ID and inherits no prior grant. Deregistration makes
old grants inert; revoking pairing removes the grants it seeded.

Project scoping does not constrain an independently configured harness's external
memory or telemetry capture. Details:
[security plan §4–§5](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/06-security.md).

## Tampering, freshness, and replay

Device signatures bind the signed envelope fields. The design independently checks
receiver-clock freshness and any sender-declared TTL, then suppresses repeated
signed key/nonce pairs using a bounded in-memory cache.

A daemon restart empties the cache: messages still within the freshness window can
be replayed during that gap. Duplicate suppression is not exactly-once delivery.
The cache is not a durable offline mailbox.
See [envelope-auth decision](https://github.com/RossGraeber/OAC/blob/main/docs/planning/decisions/C5-envelope-auth.md)
and [security plan §7](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/06-security.md).

## Provenance and permissions

Machine-set sender provenance must stay separate from message text, which may falsely
claim to be an operator, another peer, or a system instruction.
The receiving harness retains its own policy and tool-permission decisions.

Delivery authorization does not grant tool approval or control of an active turn.
Permission relay is off by default in the v0.1 plan; enabling it requires a separate,
explicit decision. Provider confirmation steps remain part of harness policy.

The ledger also carries an open finding on whether a nominal new-turn input path can
steer a busy provider session. Choosing that path alone is not proof that steering
cannot occur.
See [security plan §9 and §11](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/06-security.md)
and [open findings](https://github.com/RossGraeber/OAC/blob/main/docs/planning/STATUS.md).

## Current evidence and remaining risks

G5's Claude spike passed its provenance criteria; the Codex spike failed.
Neither result proves a product adapter, because those adapters are not built yet.
The revised Codex framing design is approved, with its validation re-run pending.
See [G5](https://github.com/RossGraeber/OAC/blob/main/docs/planning/gates/G5-result.md) and
[C13](https://github.com/RossGraeber/OAC/blob/main/docs/planning/decisions/C13-codex-provenance-framing.md).

Full end-to-end encryption is deferred. Authorized or compromised peers can still
send malicious content. Replay across restart, hook-to-shim process pairing, and
the revised Codex provenance path remain important residual risks or gaps.

The full threat inventory covers impersonation, unauthorized discovery/routing,
tampering, replay, prompt injection, compromised transport, cross-project disclosure,
and credential leakage. Mitigations require passing proving tests.
Consult [security plan §14](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/06-security.md),
[risks](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/11-risks.md), and
[gate evidence](https://github.com/RossGraeber/OAC/blob/main/docs/planning/gates/README.md).
