# Planning Prompt — Session Channels

Plan the implementation of **Session Channels**. Do not implement yet.

## Authoritative inputs
Read these files completely before planning:

1. `ADR-001.md` — architectural decisions, boundaries, v0.1 scope, security posture, Zenoh-first decision, and acceptance criteria.
2. `DESIGN.md` — component model, protocol responsibilities, adapter/transport contracts, data model, deployment model, security design, and testing direction.

Treat both as requirements. Do not restate them unless needed to resolve a design decision. If they conflict, identify the conflict explicitly rather than silently choosing one.

## Primary objective
Produce an **implementation-ready plan for v0.1** that proves the ADR's Claude Code ↔ Zenoh ↔ Codex active, secure, full-duplex communication path while preserving the boundaries in the ADR and Design.

## Work order

### 1. Verify gating assumptions first
Use current first-party sources and source code where available. Verify:

- MCP extension conventions relevant to Session Channels.
- Claude Code Channels: exact protocol, lifecycle, active-session association, inbound wake behavior, outbound/reply path, security model, custom-channel restrictions, and stability/support status.
- Codex App Server: exact IPC/API required for inbound turns and outbound events; thread/session addressing; lifecycle; whether an adapter can communicate with an **already-running Codex CLI/Desktop session** or must own/participate in session creation.
- Zenoh: bindings, P2P/local discovery, pub/sub, liveliness, security, identity, and packaging constraints.

Treat inability to support the required existing-session semantics for Claude or Codex through supported interfaces as a **go/no-go finding**, not an implementation detail. Do not substitute model APIs, UI automation, terminal scraping, credential reuse, or undocumented private RPCs.

### 2. Resolve implementation choices
Using the ADR/Design as constraints, decide and justify:

- implementation language/runtime;
- MCP extension shape and version negotiation;
- provider-adapter API;
- transport API;
- Zenoh mapping beneath that API;
- session identity/addressing and discovery;
- pairing, authentication, authorization, key storage, replay protection, provenance, and delivery acknowledgement;
- process model and lifecycle;
- local CLI/configuration model;
- dependency boundaries and OSS licenses.

Do not leak provider-specific or Zenoh-specific concepts into neutral core interfaces unless unavoidable; document any exception.

### 3. Design for replacement
Demonstrate from the proposed interfaces that:

- a new provider adapter can be added without modifying transport implementations;
- NATS/MQTT can later replace Zenoh without modifying provider adapters or Session Channels semantics;
- core CI can run without paid/live provider sessions using fake harness endpoints/contract fixtures.

### 4. Plan implementation in risk order
The first executable spikes must attack the highest-risk unknowns, especially live-session attachment/injection. Do not build substantial broker/core infrastructure before validating the provider boundaries.

For every milestone specify:

- code/modules created or changed;
- prerequisite decisions;
- executable test/demo;
- acceptance criteria;
- failure/go-no-go condition.

End with an end-to-end v0.1 acceptance test matching the ADR.

## Required output
Produce a concise planning package containing:

1. **Verified capability matrix** — Claude Code, Codex, MCP, Zenoh; Cursor only as a forward-compatibility check.
2. **Gating findings** — especially existing Codex-session attachment and Claude Channel constraints.
3. **Decisions/changes** — decisions needed beyond ADR/DESIGN; proposed ADR/DESIGN amendments if evidence invalidates an assumption.
4. **Implementation architecture** — components, process boundaries, data/control flow.
5. **Interfaces** — Session Channels extension surface, provider adapter API, transport API, core data types.
6. **Security/threat model** — identities, trust boundaries, authorization, transport security, replay/duplicate handling, prompt-injection provenance.
7. **Repository/dependency plan** — modules, ownership boundaries, selected libraries/licenses.
8. **CLI/deployment plan** — zero-container local path first; later LAN/external-transport hooks only where they affect v0.1 architecture.
9. **Test strategy** — unit, contract, security, fake-harness integration, provider integration, E2E.
10. **Ordered milestones** — risk-first, implementation-ready tasks with acceptance tests and go/no-go gates.
11. **Open risks/unknowns** — ranked by ability to invalidate v0.1.
12. **Deferred work** — enforce ADR/DESIGN non-goals and v0.1 exclusions.

## Planning behavior

- Prefer evidence over assumptions; cite current authoritative sources for externally verified capabilities.
- Prefer the smallest implementation that proves v0.1.
- Make concrete decisions where evidence permits; do not leave routine choices as vague alternatives.
- Mark experimental/preview/unsupported provider surfaces explicitly.
- Separate **normative Session Channels semantics** from **reference implementation choices**.
- Do not redesign settled ADR decisions without new evidence; propose an ADR amendment when necessary.
- Do not turn this project into an agent framework, inference router, shared context manager, or provider-auth abstraction.
