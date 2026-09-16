# G3 — Zenoh local peer

Source: PLANNING-PROMPT.md §4 G3, §3.4. Backlog: `docs/planning/backlog/03-tasks-CD.json` D3.

## Disposition

Has a fallback; not listed as v0.1 go/no-go in §4 (unlike G1/G2).

## What the spike proves

Two peers on one host discover each other, proven on all three target platforms, with and
without multicast, and with TLS bound to localhost.

## Pass criteria (evaluate each individually)

- [ ] Discovery over loopback with UDP multicast scouting (`224.0.0.224:7446`,
      `interface: "auto"`) on Windows 11, macOS, and Linux, on Zenoh **>= 1.10.0**.
- [ ] Discovery with multicast scouting disabled, using a locally shared rendezvous endpoint,
      on the same three platforms.
- [ ] A TLS listener bound to localhost (`127.0.0.1`) works in both modes above.
- [ ] Multiple peers per host do not collide on ports (peers listen on dynamic TCP ports by
      default).

## Failure criteria

Failure on any one platform for the multicast path, or any platform for the no-multicast
path, is a failure of that path specifically — attempt the fallback for the failing case and
record per-platform results rather than a single blended verdict. Record `PASS (FALLBACK
TAKEN)` where the fallback (fixed local endpoint, no scouting) is what actually passed for a
platform, and reserve plain `PASS` for platforms where the primary multicast path passed.

## Fallback

Fixed local endpoint with multicast scouting disabled (the second pass criterion above, taken
as the operating mode rather than merely tested as an alternative).

## Surfaces and version pins

- Pinned baseline: Zenoh stable release **1.10.1** (2026-09-07), dual EPL-2.0 / Apache-2.0.
  Stable API surface is `zenoh` and `zenoh-ext` only; several features sit behind the
  `unstable` flag — do not depend on unstable-flagged behavior in this gate's pass criteria.
- Native in-process peer-mode bindings relevant to the language decision: Rust, Python (PyO3
  wheels, no Windows arm64), C, C++, Java/Kotlin (JNI), Go (CGo, needs unstable API).
  TypeScript is not native (`zenoh-ts` needs a `zenohd` router with the remote-api plugin).
- Peer mode is default; discovery is UDP multicast scouting on `224.0.0.224:7446` plus gossip.
- Windows binds the scouting socket to `0.0.0.0` with `SO_REUSEADDR`.
- `#iface=` endpoint parameters are documented for Linux only — do not rely on them on
  Windows/macOS.
- Access control subjects: interface, certificate common name, username, link protocol, or
  `zid`. **`zid` subjects are explicitly unauthenticated and unfit for production** — this
  gate tests discovery/connectivity, not authorization; do not let a passing G3 be read as
  validating `zid`-based ACLs.

## §3.4 facts this spike must confirm or refute

- **The point this gate exists to confirm:** same-host discovery over loopback was broken
  before 1.10.0 (PR #2671); plan on >= 1.10.0. The spike must run on the pinned 1.10.1 (or
  later) and demonstrate loopback discovery actually works, not merely cite the changelog.
- Also record:
  - Windows scouting-socket binding behavior (`0.0.0.0` with `SO_REUSEADDR`) and whether it
    causes any observed collision with other local Zenoh processes.
  - That `#iface=` is Linux-only, confirmed by testing (or by the binding failing to apply) on
    Windows/macOS.
  - Measured cold-start discovery latency on each platform (numeric, for the risk register).
- UNVERIFIED items in this area not closed by this gate (out of scope for G3, tracked
  separately per STATUS.md): Zenoh public-key auth semantics; the 5-15 MB binary size
  estimate.

## Fixtures to capture

G3 is a transport-only gate; it does not feed the Stage 3 fake **harness** endpoints (those
are Claude/Codex). If Stage 3's in-memory transport or contract tests need a Zenoh
config/topology sample, capture the working peer configuration (JSON5) for each of the four
mode combinations (multicast/no-multicast x TLS/no-TLS) tested, with the pinned version and
platform noted on each.
