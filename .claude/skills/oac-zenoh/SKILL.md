---
name: oac-zenoh
description: Zenoh protocol detail — peer mode, scouting and the >= 1.10.0 loopback fix, zid is not an identity, no payload signing, ACL subjects, TLS/QUIC, packaging, the containment rule. Load for area:transport-zenoh work items and gate G3.
---

Version-pinned Zenoh detail. Loaded for work items labelled
`area:transport-zenoh` and for gate G3 (Zenoh local peer). This is the one
exception to "link, don't copy": version-pinned third-party protocol detail
an agent cannot look up in this repo is copied here, carrying its pin. All
facts below are PLANNING-PROMPT.md §3.4, retrieved 2026-09-15 — see `## Pin`.
Companion guardrails: `oac-boundaries`, `oac-evidence` (do not restate their
content).

## Constraints that most often trip an agent up

1. **>= 1.10.0 for loopback discovery.** Same-host discovery over loopback
   was broken before 1.10.0 (PR #2671). Do not test or plan G3 against an
   older build. See section 3 below.
2. **`zid` is not an identity.** `zid` subjects are explicitly unauthenticated
   and unfit for production. Never map OAC's `SecurityPrincipal` onto a
   `zid`. See section 5 below.
3. **No payload signing.** Zenoh provides no application-layer message
   signing. Envelope authenticity is OAC's job, not Zenoh's. See section 6
   below.
4. **Plugins need `zenohd`.** Storage and other plugins run only inside the
   `zenohd` router process — they are not available to an in-process peer.
   See section 4 below.
5. **`#iface=` is Linux-only.** The endpoint parameter is documented for
   Linux only; do not rely on it for the Windows or macOS path. See section 3
   below.

## 1. Release and licensing

Stable release 1.10.1 (2026-09-07), dual EPL-2.0 / Apache-2.0. Stable API
surface is `zenoh` and `zenoh-ext` only; several features sit behind the
`unstable` flag. Any `unstable` feature used in the transport must be named
and justified (backlog task G1, Epic G, Zenoh transport). Source:
PLANNING-PROMPT.md §3.4.

## 2. Bindings

Bindings with a native in-process runtime capable of router-less peer mode:
Rust, Python (PyO3 wheels for `win_amd64`, macOS, `manylinux`; no Windows
arm64), C, C++, Java/Kotlin (JNI, Windows MSVC supported), Go (CGo, needs
`unstable` API). TypeScript is **not** native — `zenoh-ts` requires a
`zenohd` router with the remote-api plugin and does not run on Node.js. C#
has no code. Dart is third-party and pre-1.0.

This list is load-bearing evidence for PLANNING-PROMPT.md §5 Decision 1's
presumptive answer (Rust, single static binary) — not yet a made decision:
STATUS.md lists the Rust toolchain as "not pinned" with Epic C open. §5
Decision 1 also names the condition that would reverse the presumptive
answer (a disqualifying gap in the Rust MCP SDK's legacy-revision support or
in Windows keychain access). See `oac-implementation` for the
language-choice consequences; do not decide or re-derive the choice here.
Source: PLANNING-PROMPT.md §3.4, §5 Decision 1.

## 3. Peer mode and discovery

Peer mode is default. Discovery is UDP multicast scouting on
`224.0.0.224:7446` (`interface: "auto"`, single interface) plus gossip.

**Same-host discovery over loopback was broken before 1.10.0** (PR #2671) —
plan on >= 1.10.0. Windows binds the scouting socket to `0.0.0.0` with
`SO_REUSEADDR`; `#iface=` endpoint parameters are documented for **Linux
only**. Peers listen on dynamic TCP ports by default, so multiple peers per
host do not collide.

Gate G3 fallback if scouting is unreliable: a fixed local rendezvous
endpoint without multicast scouting (PLANNING-PROMPT.md §4). Source:
PLANNING-PROMPT.md §3.4, Appendix B (`eclipse-zenoh/zenoh` PR #2671, issue
#1432).

## 4. Primitives and plugins

Primitives: pub/sub, queryables, liveliness tokens with history-capable
liveliness subscribers. Storage and other plugins run **only** inside
`zenohd` (router) — never inside an in-process peer. This interacts directly
with ADR-001's "must not require... a separately administered server for
normal local use": do not make `zenohd` a required background process for
the default local path just to get a plugin feature (see `oac-boundaries`
boundary 6).

Advanced publisher/subscriber (cache, retransmission, miss detection) is in
`zenoh-ext` behind `unstable`. Source: PLANNING-PROMPT.md §3.4.

## 5. Security

TLS/mTLS and QUIC listeners with per-endpoint certificates; user/password
auth; public-key auth config keys exist but semantics are **undocumented
(UNVERIFIED)**. Access control rules are keyed by interface, certificate
common name, username, link protocol, or `zid`; **`zid` subjects are
explicitly unauthenticated and unfit for production** — never use them as
the ACL subject in a production configuration. Endpoints can be bound to
`127.0.0.1`; multicast scouting can be disabled. Config is JSON5 or
programmatic.

Map OAC policy onto authenticated ACL subjects (certificate common name or
username) only, per Decision 6 and 10. Source: PLANNING-PROMPT.md §3.4.

## 6. No application-layer message signing (the second trap)

**Zenoh provides no application-layer message signing.** Authenticity of an
OAC envelope must be established by OAC itself — the `security.signature`
field in the envelope, not transport security, is the authenticity proof
even over TLS/QUIC. See `oac-security-work` for the threat-table format and
the "authenticated but untrusted" doctrine; do not restate it here. Source:
PLANNING-PROMPT.md §3.4, §7.

## 7. Packaging

A Rust binary embeds the runtime with no daemon. Windows MSVC and GNU
artifacts ship. The 5 to 15 MB single-binary estimate is **derived and
UNVERIFIED** — keep both labels whenever you cite it; do not state it as a
measured fact until task I3 records the actual binary size. Source:
PLANNING-PROMPT.md §3.4.

## 8. The containment rule

Governs every use of this skill: Zenoh types must not escape the transport
module, and Zenoh-specific concepts must not appear in the neutral protocol
or in neutral core interfaces (ADR-001 Boundary; DESIGN §Zenoh transport:
"Zenoh types must not escape the transport module"; DESIGN §MCP Session
Channels extension: "The specification MUST NOT mention Zenoh keys...").

Practical consequence: key expressions, `zid`, config structs, and session
handles stay inside `transports/zenoh/`. A `core/` type, the spec text under
`spec/`, or a provider adapter must never reference any of them. See
`oac-boundaries` for the grep and lint commands that catch this
mechanically — do not restate them here.

## Where the content lives

- Source facts: `docs/planning/PLANNING-PROMPT.md` §3.4 (baseline), §4 gate
  G3, §5 Decisions 1, 6, 10, §7 (security requirements), Appendix B (Zenoh
  URLs, PR #2671, issue #1432).
- Gate G3 pass/fail/fallback: `docs/planning/PLANNING-PROMPT.md` §4; gate
  procedure: `oac-gates`.
- Containment rule enforcement (grep/lint commands): `oac-boundaries`.
- Envelope authenticity, threat table, "authenticated but untrusted":
  `oac-security-work`.
- Language choice consequences of the binding list in §2:
  `oac-implementation`.
- Current gate verdict, pin status, and open UNVERIFIED items:
  `docs/planning/STATUS.md`.
- Evidence standard and re-verification procedure: `oac-evidence`.
- ADR boundaries: `oac-boundaries`.

## Pin

Written against the **pre-verified baseline**, not a confirmed pin: Zenoh
1.10.1 (2026-09-07); minimum 1.10.0 required for loopback discovery (PR
#2671). Source: `docs/planning/PLANNING-PROMPT.md` §3.4, Appendix B
(`https://github.com/eclipse-zenoh/zenoh` releases, `DEFAULT_CONFIG.json5`,
PR #2671, issue #1432, plus the `zenoh.io/docs/manual/{access-control,tls,
quic,user-password,configuration}` and `zenoh.io/docs/getting-started/
deployment` pages and the binding repositories). Retrieved 2026-09-15.

**Stage 0 (Epic B) has not run.** This is a pre-verified baseline, not a
confirmed pin. A version bump to Zenoh, or Stage 0 running, invalidates this
skill — re-verify every fact per `oac-evidence` §7 before trusting it again,
and update this `## Pin` section, not just the prose.
