# 09 — Test Strategy

**Issue:** #30 (Epic A, backlog key `A10`). **Depends on:** #26. **Source:**
`docs/planning/PLANNING-PROMPT.md` §6 (design-for-replacement proofs 3-4), §8 Stages 3-5,
§9 item 10, §11 item 2; `docs/planning/DESIGN.md` "Testing"; `docs/planning/v0.1/
05-interfaces.md` §7, §9, §10, §12, §13, §15, §18; `docs/planning/decisions/
C5-envelope-auth.md` §7-§8, §13; `docs/planning/decisions/C7-zenoh-transport.md` §2, §9;
`docs/planning/gates/README.md` "Fixtures" and "Re-run/invalidation policy";
`docs/planning/backlog/03-tasks-CD.json` task D6; `docs/planning/backlog/
04-tasks-EF.json` tasks E8, F8-F12; `docs/planning/backlog/05-tasks-GHIJ.json` tasks
H1-H5.

(Dated note, 2026-10-05, #305: the `05-interfaces.md` §13 and §15 sources above are the M0
draft of the adapter and transport contracts. The frozen `spec/interfaces.md` supersedes
`05-interfaces.md` §13-§15 (`spec/interfaces.md` §9), and the live contract citations below point there.)

**Scope.** This file owns four things: the eight-tier test taxonomy (§2), the
CI-default-versus-opt-in split and its mechanics (§3-§4), the fixture capture and
refresh process (§13), and the two mandatory traceability tables — DESIGN acceptance
criteria 1-10 to named test (§11), and threat-table mitigation to named test (§12). It
does **not** own the threat table itself — every threat row is
`docs/planning/v0.1/06-security.md` §14's, cited here by row, never restated — nor the
stage-gate pipeline's entry/exit criteria (`docs/planning/v0.1/10-stages.md`, task A11,
not yet landed) nor the module layout that places `tests/` (`docs/planning/v0.1/
07-repository-and-dependencies.md` §1-§2). Cited by path throughout, not re-derived.

**Proof caveat, stated once, applying to every section below.** At this document's own
landing (A10, Epic A), `docs/planning/STATUS.md`'s Gate verdicts table read every gate
verdict (G1-G5) `NOT RUN`. Per the current `docs/planning/STATUS.md` Gate verdicts table,
G1, G2, and G4 are **PASS** (G1 originally PASSED on Claude Code `v2.1.282`; the Claude
Code (Channels) pin went floating 2026-09-27, last observed `v2.1.283`, invalidating that
PASS; re-run and **PASSED again** 2026-09-28 on `v2.1.283` — see
`docs/planning/gates/G1-result.md`), G5 is **FAIL** (Codex
criteria 2/3 f; Claude all criteria x)
(`docs/planning/gates/G5-result.md`) (dated note, 2026-10-03, #220: G5 is now **PASS**
after its Codex-leg re-run under C13 §11), and G3 stays `NOT RUN` at gate level (note 2026-10-02, #219: G3 is now **PASS** at gate level, macOS leg run on a GitHub-hosted VM — see `docs/planning/gates/G3-result.md`); per
`docs/planning/STATUS.md` "Current stage," the project is still at **Pre-Stage 0** — no
E8/F8-F12/H1-H5 test exists yet. *(Dated note, 2026-10-03, #254: the "Pre-Stage 0"
statement is stale. Read the current stage from `docs/planning/STATUS.md` "Current stage"
rather than from this caveat; this file does not restate it. E8's conformance fixtures have
since started landing under `tests/protocol/`; no F8-F12 or H1-H5 test exists yet.)* This file is the **design** these tasks build against,
not a report of tests that pass; a gate result is not one of the test tiers this file
owns. Every "proves"/"asserts" statement below names what a built test will assert once
its owning task lands, not a result.

**Naming.** Product and repository: **Open Agent Channel (OAC)**. Normative protocol
specification: **OAC Session Channels**. CLI binary: **`oac`**. Per ADR-001-A1
(`docs/planning/v0.1/03-decisions-and-amendments.md` §2), this file never writes bare
"Session Channels" or `sessionchannels`.

---

## 1. Scope and ownership — restated as a short list

This file owns:

- The tier taxonomy — exactly eight tiers, named as issue #30 names them (§2).
- The CI-default-versus-opt-in split: what it means, and the mechanics that keep it
  enforceable rather than aspirational (§3-§4).
- The no-polling assertion's exact shape (§5).
- Per-tier detail for the three tiers with the most designed-but-unbuilt surface area:
  spec conformance (§6), security (§7, including the Zenoh-containment criterion's lint
  proof, §8), end-to-end (§9), and CLI smoke (§10).
- The two mandatory traceability tables (§11-§12).
- Fixture capture and refresh (§13) and test-location policy (§14).
- The normative/reference-implementation split as it applies to tests (§15) and the
  deliberately-omitted-from-v0.1 test surface (§16).

This file does **not** own:

- The threat table's rows (attack, precondition, mitigation, residual risk) —
  `docs/planning/v0.1/06-security.md` §14, cited by row number in §12 below, never
  copied.
- The stage-gate pipeline's entry/exit criteria and go/no-go conditions —
  `docs/planning/v0.1/10-stages.md` (task A11, not yet landed); §2's "stage introduced"
  column below names which stage a tier first appears in, but the stage's own
  entry/exit criteria are that file's job.
- Module layout and `tests/` ownership boundaries —
  `docs/planning/v0.1/07-repository-and-dependencies.md` §1-§2.

---

## 2. Tier taxonomy — eight tiers

Named exactly as backlog task A10 (issue #30) names them: unit, spec conformance,
contract (adapter + transport), security, fake-harness integration, provider
integration, end-to-end, cross-platform CLI smoke.

**Resilience is folded into the security tier, as a named sub-row — decided, not left
open.** Task H3 (disconnect/reconnect/restart/expiry) tests duplicate suppression across
a daemon restart and TTL-expiry delivery states — the same replay/dedup machinery
`docs/planning/decisions/C5-envelope-auth.md` §7-§8 defines and
`docs/planning/v0.1/06-security.md` §7's "cold-restart gap" already names as a security
property, not a transport-contract property. Folding H3 into the security tier (§7 below)
keeps one tier per named security/replay concern rather than splitting a single
mitigation's proof across two tiers.

| # | Tier | Proves | Runs against | CI-default or opt-in | Owning backlog task | Stage introduced |
|---|---|---|---|---|---|---|
| 1 | Unit | Module-level logic: envelope encode/decode, replay/dedup logic, the delivery-receipt state machine (F2/F4/F6 acceptance boxes) | In-process code only | CI-default | F2-F7 (each carries its own unit-test acceptance box); wired into the default pipeline by F12 | Stage 3 |
| 2 | Spec conformance | Wire representation matches the frozen closed error taxonomy (`05-interfaces.md` §10, 10 rows), the delivery-state set (`05-interfaces.md` §9, 8 states), and the `ttl_ms`-before-replay-window precedence rule (`05-interfaces.md` §10) — detail at §6 below | The E8 conformance fixture set | CI-default | E8; wired into CI by F12 | Stage 2 (fixture set built); wired into CI at Stage 3 |
| 3 | Contract — adapter + transport | The `ProviderAdapter` (`spec/interfaces.md` §5, Table 5.2) and `Transport` (`spec/interfaces.md` §6, Table 6.4) contracts are obeyed identically by every implementation, including the no-polling rule (§5 below) | Adapter sub-row: fake Claude/Codex endpoints (F8/F9) at Stage 3, real adapters unchanged at Stage 4. Transport sub-row: the in-memory transport (F7) at Stage 3, real Zenoh over loopback unchanged at Stage 4 | CI-default against fakes/in-memory; against real adapters it is the provider-integration tier (row 6), opt-in, pinned — but real Zenoh over loopback stays CI-default (§3's loopback rule: no live provider, no API key, no network beyond loopback) | F10 | Stage 3 (fakes/in-memory, adapter sub-row real at Stage 4 opt-in); Stage 4 (transport sub-row real, CI-default) |
| 4 | Security (resilience folded in, see above) | Spoof, replay, duplicate-suppression-including-across-restart, unauthorized-routing, cross-project-leakage, `zid`-never-an-identity, and local-IPC-peer-auth mitigations from `06-security.md` §14 — detail at §7 below | Fakes (F11) at Stage 3; real transport/adapters (H2, and H3 for the resilience sub-row) at Stage 5 | CI-default against fakes; against real adapters (Claude Code, Codex) it is opt-in, pinned — but against real Zenoh over loopback, with no live adapter in the path, it stays CI-default (§3's loopback rule) | F11; H2; H3 | Stage 3 (fakes); Stage 5 (real adapters, opt-in; real-Zenoh-only cases CI-default) |
| 5 | Fake-harness integration | Core + adapter + transport wired together with no live provider — the Stage 3 exit condition | Fake Claude endpoint (F8), fake Codex endpoint (F9), in-memory or loopback transport (F7) | CI-default | F12 | Stage 3 |
| 6 | Provider integration | Real adapter behaviour against a real, pinned harness version — the runtime half of gates G1/G2 | Real Claude Code / real Codex, on pinned versions (`docs/planning/PINS.md`), driven through herdr by default (Epic K; §4), the operator attending only for sign-in (#187); since 2026-09-30 the driver accepts harness dialogs in dev/test runs, recorded as `driver`, except a consent step a gate criterion names (#196, `oac-gates` `references/scripted-runs.md`) | Opt-in only, explicit flag, pinned versions — never the default `test` run (F12 acceptance: "Provider integration tests exist but are opt-in and pinned"). Real Zenoh over loopback is not this row — it needs no live provider, so it is CI-default under row 3/4 (§3's loopback rule), never this opt-in tier | F12 isolates it as its own target; exercised at Stage 4 | Stage 4 |
| 7 | End-to-end | The ADR-001 Validation criterion, decomposed into individually asserted clauses — detail at §9 below | Real providers, real transport | Opt-in (needs live providers) — the go/no-go test (H1, labelled `go-no-go`) | H1 | Stage 5 |
| 8 | Cross-platform CLI smoke | One-command startup, `status`/`sessions`/`doctor`, clean shutdown — detail at §10 below | The built CLI binary; no live provider required | CI-default, matrixed on Windows, macOS, Linux | H4 | Stage 5 |

---

## 3. The CI-default rule

Stated normative-style, binding on the default pipeline F12 stands up:

The default test suite — every tier row above marked CI-default (rows 1, 2, 3's
fake/in-memory and real-Zenoh-over-loopback transport sub-row, 4's fake half and its
real-Zenoh-over-loopback-only cases, 5, 8) — **runs with no live provider, no API key,
and no network beyond loopback.**

**Loopback, defined explicitly.** A Zenoh peer on the local machine talking to another
Zenoh peer on `127.0.0.1` — the local-mode bind `docs/planning/decisions/
C7-zenoh-transport.md` §5 fixes, with no `zenohd` and no LAN-facing listener — **stays
default.** A call that would touch Claude Code, Codex, or any hosted API **does not**,
regardless of whether that call happens to route over loopback too (for example, a local
Codex app-server socket is still a live-provider call, not a loopback exemption, because
the thing on the other end is the real harness, not a fake).

**The anti-exception rule, stated once, binding on every future test author.** A test
that "just this once" needs a live provider to pass is a signal the fake or fixture it
should be running against is incomplete — it is a Stage 1 (§13) or Stage 3 (F8/F9) gap
to report, never a reason to add a default-tier waiver. Per `docs/planning/
PLANNING-PROMPT.md` §6: "the core, the spec conformance tests, and the security tests
run in CI with fake harness endpoints and recorded protocol fixtures, with no live
provider, no API key, and no network beyond loopback" is a design-for-replacement
requirement of the plan, not a target to relax under schedule pressure.

---

## 4. Opt-in mechanics

**A separate flag or target, never a slower default tier.** Opt-in means a plain
`cargo test` (or equivalent default `test` invocation) never runs a row-6/7 test. F12's
own acceptance box states it exactly: "Provider integration tests exist but are opt-in
and pinned." An opt-in test is reached only by an explicit, separately-invoked target —
never a test attribute that merely marks it slow inside the same default run.

**Every opt-in test names its exact pinned provider version, read from
`docs/planning/PINS.md`, never "latest."** Concretely: the provider-integration tier
(row 6) names the Claude Code or Codex pin-table row it runs against
(`docs/planning/PINS.md` pin table) — real Zenoh over loopback is not row 6 (§2's
loopback carve-out: it is CI-default, so it carries no opt-in pin-naming obligation
here, though the fixture-refresh trigger below still applies to it); the end-to-end
tier (row 7, §9 below) names all three pins it depends on (Claude Code, Codex, and the
transport pin, since it crosses both adapters and Zenoh). No opt-in test description
says "the current release" or "latest" — it names the exact pinned version string, the
same string `docs/planning/gates/README.md`'s per-gate result files record.

*(Dated note, 2026-10-02, #228: for the floating Claude Code and Codex rows (#216), "pinned
version" means the installed version the test ran on, which it records; a version other
than the last tested one is a `VERSION WARNING`, never a failure. Fixed rows are
unchanged.)*

**A pin move invalidates the recorded opt-in result the same way it invalidates a gate
— cited, not re-derived.** Per `docs/planning/gates/README.md` "Re-run/invalidation
policy" §a: "Changing the Pinned version cell, the Release date cell, or a row's
presence... in the `docs/planning/PINS.md` pin table invalidates every gate named in
that row's Gates affected column." This file adopts the identical rule for a recorded
opt-in test result: a pin move against the surface an opt-in test names invalidates that
test's last recorded result exactly as it invalidates the gate result file for the same
surface — the invalidated result is not silently assumed to still hold, and re-running
the opt-in test against the new pin is required before its result is cited again as
current. This is the same obligation §a-§e of that policy already state for gate files;
this file does not invent a second invalidation mechanism, it applies the existing one to
opt-in test results.

*Dated note, 2026-10-01 (#216, operator decision):* the Claude Code and Codex rows are exempt. Their versions float: PINS.md
records a minimum and a last tested version, and a different version is a warning that
never stops a run, never fails CI and never by itself invalidates a gate or an opt-in
result (`docs/planning/gates/README.md` §a). An opt-in result records the harness version
it ran on. The rule above still applies to every other row (MCP, `rmcp`, Zenoh, Rust).

**Dated note, 2026-09-28 (K5, issue #128): herdr-driven runs are
provider-integration tier, opt-in by construction, never default.** Epic K (#123) adds a test-side driver,
`tools/herdr/run.mjs`, that drives real Claude Code and Codex CLI sessions through herdr
(pinned in `docs/planning/PINS.md`, row `herdr (test tooling)`; tool record
`docs/planning/decisions/K1-herdr-evaluation.md`). A herdr-driven run is **not a ninth
tier**. It is row 6 (and, once H1 reuses it, row 7) run with a driver in the operator's
place. Every such run needs a real, logged-in harness process, so §3's rule already puts
it outside the default suite: it is opt-in by construction, not by a flag someone
remembered to set. No default `test` invocation and no default CI workflow invokes it. It
names its exact harness pins as every row-6 test does, and it also names the herdr pin.
herdr itself is never a dependency of anything the default suite builds: it is dev/test
tooling only, kept out of `adapters/`, `core/`, `cli/`, `transports/` and `spec/` by
`scripts/check-herdr-containment.mjs`. How a scripted run is recorded, and when it may
count toward a gate verdict, is `oac-gates` `references/scripted-runs.md` and
`docs/planning/gates/README.md` "Scripted runs (herdr)". This file does not restate
either. K6 (issue #129) adds the one workflow that runs the driver,
`.github/workflows/herdr-provider-optin.yml`. It starts only on a manual dispatch or on a
push to `main` that changes `docs/planning/PINS.md`, and it runs only on operator-owned
self-hosted runners (`docs/planning/gates/herdr-runner.md`). No default workflow runs it,
and `scripts/check-herdr-containment.mjs` check 9 fails any other workflow that reaches
the driver or those runners.

> **Reference implementation note:** the v0.1 Rust workspace's own mechanism for
> separating the opt-in tier from the default `cargo test` run — a Cargo feature flag, a
> separate test binary/target, or an environment-variable gate inside `#[ignore]`-style
> attributes — is a Stage 3/4 implementation detail, not fixed by this file. Whatever
> mechanism Stage 3 (F12) chooses, it must satisfy §3's rule that a plain default `test`
> invocation never triggers it.

---

## 5. The no-polling assertion spec

**Shape, by operation class — not by call-count growth over time.** A legitimate
streaming adapter's blocking receive loop and its keepalive pings are also
adapter-initiated calls whose counts grow with time; a growth-based assertion would fail
a correct adapter. The assertion instead separates two call classes, per
`docs/planning/v0.1/05-interfaces.md` §7 and `docs/planning/DESIGN.md` "Delivery
semantics":

- **Attach/subscribe-establishment calls stay flat after the initial attach.** A
  subscription or attachment is established once per session or per channel; the test
  counts establishment calls and asserts the count does not grow as further messages
  arrive — no re-subscription per message, no re-attachment per timer tick.
- **On-demand message-fetch call count stays zero between arrivals.** Any operation that
  *requests* pending messages and returns payloads on demand (a list-inbox, fetch-next,
  or receive-by-request call the adapter issues on its own initiative) must be called
  zero times in the interval between one message's arrival and the next — a message
  becomes visible to the adapter only via the established subscription/stream/callback
  firing, never as the return value of a call the adapter made to ask "is there anything
  new."

**Explicitly excluded from both counts.** A blocking wait/read on an already-established
stream (it does not "ask" anything; it waits for the transport to hand it data) and
keepalive/ping operations that return no message payload. Neither trips this assertion —
`docs/planning/DESIGN.md` "Delivery semantics": "Provider-supported streaming
connections, subscriptions, event loops, and keepalives are acceptable; application-level
inbox polling is not for adapters claiming active inbound support."

**Named test: `contract/adapter/no-polling`.** Owned by F10 (the adapter and transport
contract suites). It runs against both fakes — F8's fake Claude endpoint and F9's fake
Codex endpoint — as a CI-default test, and, once real adapters exist (Stage 4), against
the real adapter's instrumented transport call surface as a provider-integration
(row 6) test, opt-in and pinned per §4.

---

## 6. Spec-conformance tier detail

**Coverage, drawn from the frozen §9/§10 sets — one fixture per row, no gaps.** The E8
conformance fixture set (`docs/planning/backlog/04-tasks-EF.json` task E8) covers:

- **The closed error taxonomy** (`docs/planning/v0.1/05-interfaces.md` §10) — one
  fixture per row, all ten: `malformed-envelope`, `unsupported-spec-revision`,
  `unsupported-capability`, `signature-verification-failed`, `replay-window-rejected`,
  `duplicate`, `unknown-destination`, `unauthorized`, `expired`, `internal-failure`.
- **The frozen delivery-state set** (`05-interfaces.md` §9, 8 states: `accepted-by-adapter`,
  `handed-to-harness`, `unknown`, `rejected`, `unreachable`, `expired`, `duplicate`,
  `failed`) — each state reachable from at least one positive or negative fixture.
- **The `ttl_ms`-before-replay-window precedence rule** (`05-interfaces.md` §10): "a receiver `MUST` check
  `ttl_ms` expiry first and emit `expired` if it fails, checking the replay accept-window
  (and emitting `replay-window-rejected` on failure) only once the `ttl_ms` check has
  passed" — a dedicated fixture exercises an envelope that fails both checks, asserting
  only `expired` is emitted, never `replay-window-rejected`, per that precedence.

**Adding an error row is a versioning event — a new fixture is required, not optional.**
Per `docs/planning/v0.1/05-interfaces.md` §11's versioning policy: an added error code an
old peer treats as `failed` is non-breaking and requires only a spec-revision bump; either
way, the new row's own conformance fixture must land in the same change that adds it —
the fixture set is never allowed to lag the error taxonomy it exists to prove.

*(Dated note, 2026-10-07, #61, F12: CI runs the fixture set twice on every OS: through the
reference runner (`tests/protocol/runner/`, every stage from the spec text) and through the
workspace's own code (`core/tests/conformance.rs`). The Rust harness runs every stage
except four, which it lists by name; a fixture of any other unrun stage fails. `binding`
(26 `sc-id` fixtures) is the core's, pending #331: the core has no
binding-from-native-signal logic for §6.7 yet, although `spec/session-channels.md` §3.3's
reference implementation note has F12 drive it. `mcp-binding`, `provenance` and `body` are
adapter work. It requires the two `send`-stage fixtures F2 deferred (SC-ENV-021.p01,
SC-ENV-066.n01) to run and pass.)*

---

## 7. Security tier detail

Named tests, each citing the decision section it proves rather than re-deriving the
mitigation:

- **Spoof.** A forged or altered signature is rejected with `signature-verification-failed`,
  mapping to the `rejected` delivery state (`05-interfaces.md` §10; `docs/planning/
  decisions/C5-envelope-auth.md` §2, §5). Owning tier: F11 (fakes), H2 (real).
- **Replay.** An envelope outside the `created_at` accept-window is rejected with
  `replay-window-rejected` (C5 §7). Owning tier: F4, per `06-security.md` §14 row 4.
- **Duplicate suppression by `(key_id, nonce)`, including across a daemon restart.** The
  dedup key is `(security.key_id, security.nonce)`, not `id` alone (C5 §7); the
  cold-restart gap — the window itself, not the dedup store, is the only defence for the
  interval immediately following a restart — is asserted as a named, accepted residual
  risk, not silently passed over (C5 §8). Owning tier: F11 (in-process store behaviour);
  **H3** (the resilience sub-row folded into this tier per §2 — "Duplicate suppression
  holds across restart" is H3's own acceptance box).
- **Unauthorized routing.** A peer not on a session's allowlist is rejected with
  `unauthorized`, per the default-deny, per-session-id, `working_directory`-scoped
  allowlist (C5 §11). Owning tier: F5, H2, per `06-security.md` §14 row 2.
- **Cross-project leakage.** A session registered under one `working_directory` is not
  discoverable by a peer not authorized for it — `list_sessions` returns only sessions
  the caller is authorized to see (C5 §11; `docs/planning/decisions/
  C6-trust-rendering.md` §8). Owning tier: **H2**'s fourth acceptance item, per
  `docs/planning/v0.1/06-security.md` §13.
- **`zid`-never-an-identity.** No allowlist entry, pairing record, or ACL rule is ever
  keyed on a Zenoh `zid` — ACL subjects are drawn only from authenticated certificate
  common name or username (C5 §12; `docs/planning/decisions/C7-zenoh-transport.md` §9's
  threat-table row 4 on this exact attack). Owning tier: F11 (any code-path assertion);
  gate-level proof is task G3, per `06-security.md` §14 row 10 — this file does not
  claim a stronger proof than that row already names.
- **Local IPC peer auth.** The connecting shim process's OS-asserted UID must equal the
  daemon's own UID — Windows named-pipe security descriptors /
  `GetNamedPipeClientProcessId`, Unix `SO_PEERCRED`/`getpeereid()` (`docs/planning/
  decisions/C2-process-model.md` §4; C5 §10(a)). Owning tier: F11; task **G9** (the named
  local-IPC-peer-auth verification task, `06-security.md` §12).

**Resilience sub-row, stated once, per §2's decision.** H3's remaining acceptance boxes
not already covered above — peer disconnect/reconnect restoring presence without
duplicate delivery, daemon restart re-registering live sessions, TTL expiry producing
`expired` rather than silent loss, and delivery to a mid-flight-exited session producing
`unreachable` — are this tier's resilience sub-row, run against the in-memory transport
and fakes at Stage 3-shaped scope and against the real transport at Stage 5 (opt-in where
it needs the real transport, CI-default where it does not).

---

## 8. Zenoh-containment criterion — handled by lint, not by a runtime test

**Criterion 9 ("Zenoh-specific types stay inside its transport module") is proven by the
`oac-boundaries` CI lint, not by any test row in §2.** This is a decided disposition, not
an open question: `docs/planning/backlog/05-tasks-GHIJ.json` task H5's own acceptance box
states it directly — "Criterion 9 (Zenoh types confined) is proven by the CI lint, not by
assertion." No tier in §2 above claims to prove criterion 9; §11's traceability table
names the lint, not a test, in that row.

**The lint's recorded scope gap is carried here verbatim in spirit, not narrowed.** Per
`docs/planning/decisions/C7-zenoh-transport.md` §2's correction: the lint pattern
(`\bzenoh\b|\bzid\b|key[_-]?expr|liveliness`) does **not** match `zid`/`zenoh` embedded
inside a longer `snake_case` or `camelCase` identifier (e.g. `session_zid`,
`x_zenoh_helper`) — `\b` finds no word boundary inside such a token, so a leak shaped
that way passes the lint undetected. Separately, the lint as currently specified
(`oac-boundaries` `references/mechanical-checks.md` check 1) runs only against `spec/`
and `core/` — it does **not** run against `adapters/` or `cli/` at all, despite both being
named consuming directories.

**This remainder is labelled an open v0.1 gap, not a closed mitigation — stated so a
reader does not read criterion 9 as fully proven once the lint runs clean.** Once
`core/`/`spec/` exist and the lint reports zero hits, that result proves only: no
whole-token `zenoh`/`zid` hit in `spec/` or `core/`. It does not prove: no
`snake_case`/`camelCase`-embedded `zid`/`zenoh` leak anywhere, and no leak of any shape
in `adapters/` or `cli/`. Closing this gap requires either extending check 1's own path
list to cover `adapters/`/`cli/`, or a second check for embedded-token leaks, or both —
neither exists today. Carried forward to `docs/planning/v0.1/11-risks.md` (task A12, not
yet landed) as an open item, per the same disposition `docs/planning/v0.1/06-security.md`
§15 already applies to every unproven mitigation.

*(Dated note, 2026-10-07, #61, F12: the second check now exists. `oac-boundaries` check 12,
`scripts/check-containment.mjs`, runs as the build-failing `containment` job of
`.github/workflows/boundary-lint.yml`. It matches `zenoh` anywhere in a token, `zid` as a
`snake_case`, kebab-case or `camelCase` segment, key expressions and liveliness terms, in
every git-tracked entry outside `transports/zenoh/` under `core/`, `cli/`, `adapters/`,
`transports/`, `spec/`, `tests/fakes/` and `tests/protocol/`, and in the root manifests,
with a self-test planting each shape. Both halves of the gap above are closed for those
paths. What it still does not prove: a leak spelled some other way (a Zenoh type behind a
`type` alias whose name avoids every pattern; a name built at compile time with `concat!`
or a pasting macro, or at run time; `\u{...}` or other escapes; homoglyphs, such as a
Cyrillic letter inside `zenoh`), and any
path outside its scope (`tests/security/`, `tests/integration/`, which compose a real
transport in Stages 4-5). Criterion 9's proof is that lint, as above; this residue stays a
v0.1 gap.)*

---

## 9. End-to-end tier detail (H1)

**The ADR-001 Validation criterion, decomposed into individually asserted clauses — not
one pass/fail bit.** Quoted, `docs/planning/ADR-001.md` "Validation criterion", as
amended by ADR-001-A2 (`docs/planning/v0.1/03-decisions-and-amendments.md` §2):

> Architecture is proven when a Claude Code session, launched OAC-enabled, sends through
> OAC Session Channels/Zenoh to a Codex harness session, launched OAC-enabled, without
> receiver polling; Codex responds and Claude receives the response actively; neither side
> invokes or holds credentials for the other's model API; and sender identity/authorization
> are enforceable rather than inferred from content.

H1's own acceptance boxes mirror this decomposition exactly, and this file adopts the
same four clauses as four separately asserted checks a single test must make, per
`docs/planning/backlog/05-tasks-GHIJ.json` task H1:

1. **No receiver polling.** Asserted by the §5 no-polling shape (`contract/adapter/
   no-polling`), applied here against the real, live session rather than a fake.
2. **Active reply received.** Codex replies actively and Claude receives it as a channel
   wake — asserted as an observed event, not inferred from the absence of a timeout.
3. **No cross-model-API credential use, asserted by the test itself, not by inspection.**
   The test must itself assert neither side invoked or held the other's model-API
   credentials (for example, by instrumenting the adapter's outbound call surface and
   asserting no OpenAI-API or Anthropic-model-API call was made) — a passing test that
   merely didn't happen to make such a call, unasserted, does not satisfy this clause.
4. **Identity/authorization enforced, not inferred.** The delivered message's
   authorization must be traceable to a verified signature and a matched allowlist entry
   (C5 §2, §11) — never inferred from the message's own content claiming a sender.

**"Existing session," qualified per ADR-001-A2.** Neither harness supports attaching to
an arbitrary, already-running session process (the structural finding ADR-001-A2's
rationale cites). "Existing" in the Validation criterion, as amended, means a live
interactive session **launched OAC-enabled**:

- Claude: `claude --dangerously-load-development-channels server:oac` (`docs/planning/
  v0.1/08-cli-and-deployment.md` §7 — surface label **research preview**, version
  floating per `docs/planning/PINS.md`).
- Codex: attached via the shared local app-server daemon, or, on the documented
  fallback, `codex --remote` (`docs/planning/v0.1/08-cli-and-deployment.md` §9-§10 —
  surface label **experimental (per-method gating)**, version floating per
  `docs/planning/PINS.md`).

(Note, 2026-10-01, issue #186: these two bullets previously read "pinned `v2.1.274`" and
"pinned `@openai/codex@0.154.0`"; those `docs/planning/PINS.md` rows went floating
2026-09-27 and 2026-09-26.)

H1's own acceptance wording, quoted: "A live Claude Code session, launched OAC-enabled,
sends to a live Codex session and the Codex model answers without any receiver-side
polling." A test that spins up an arbitrary pre-existing process and expects attach
without an OAC-enabled launch is not testing what H1, or ADR-001-A2, requires, and cannot
pass.

**Must run on all three platforms — a single-platform pass is not done.** H1's own
acceptance box: "Runs on Windows, macOS, and Linux on the pinned provider versions." Per
§4's opt-in mechanics, each platform run is pinned against the same
`docs/planning/PINS.md` Claude Code, Codex, and Zenoh rows — a pin move on any of the
three invalidates every recorded platform result for this test, not just one. *Dated note, 2026-10-01 (#216, operator decision):* for
Claude Code and Codex, read "pinned" as "recorded": a harness version change warns and
invalidates nothing; only a Zenoh pin move invalidates.

---

## 10. Cross-platform CLI smoke tier (H4)

**Built binary only, no live provider, CI-default, matrixed on the three platforms.**
Per `docs/planning/backlog/05-tasks-GHIJ.json` task H4, this tier runs against the built
`oac` CLI binary with no live Claude Code or Codex process required — the command
surface it exercises is documented, cited, not re-listed as new fact, at
`docs/planning/v0.1/08-cli-and-deployment.md` §5-§6.

**Coverage, matching H4's own acceptance boxes:**

- **One-command start.** `oac start` from a fresh install with no config file present —
  the zero-file default `08-cli-and-deployment.md` §3 fixes — satisfies DESIGN
  acceptance criterion 1 (§11 below).
- **`status`.** Reports daemon up/down, device id, and transport peer state
  (`08-cli-and-deployment.md` §5).
- **`sessions`.** Behaves correctly with zero, one, and two registered sessions
  (`08-cli-and-deployment.md` §5).
- **`doctor`.** Runs the five-check preflight list (`08-cli-and-deployment.md` §6):
  IPC path permissions, credential-store reachability, transport-peer bind, the Claude
  Code version/`MCP_PROTOCOL_NEGOTIATION` floor, and Codex control-socket reachability —
  exercised on the credential-store path per platform, plus the `age`-encrypted-file
  fallback on headless Linux.
- **Clean shutdown.** Leaves no orphaned process and no stale presence record.

Matrixed on Windows, macOS, and Linux, per H4's own acceptance box and the same "no
single-platform pass" rule §9 states for H1.

---

## 11. Acceptance-criteria traceability table — all ten rows, mandatory

Criterion text verbatim, `docs/planning/DESIGN.md` "v0.1 acceptance criteria" (lines
155-165). Pre-Stage 0: **every status below is `not-yet-written` and every named test's
owning gate, where one applies, is `NOT RUN`** — no row is marked done ahead of its test
actually existing and passing, per `docs/planning/backlog/05-tasks-GHIJ.json` task H5's
own acceptance box: "Any unproven criterion is called out as a v0.1 gap rather than
quietly marked done." *(Dated note, 2026-10-03, #254: "Pre-Stage 0" here is stale; read
the current stage from `docs/planning/STATUS.md` "Current stage", and gate verdicts from its
Gate verdicts table. This table's statuses are not re-authored here.)*

| # | Criterion (verbatim) | Named test | Tier | CI-default / opt-in | Current status |
|---|---|---|---|---|---|
| 1 | "One-command local startup." | Cross-platform CLI smoke — one-command start (§10) | CLI smoke | CI-default | not-yet-written |
| 2 | "Claude and Codex adapters expose distinct neutral sessions." | `contract/adapter/session-binding-signals` (`ProviderAdapter`'s session binding signals, `spec/interfaces.md` §5.4) | Contract — adapter | CI-default (fakes); opt-in (real, row 6) | not-yet-written |
| 3 | "Sessions discover one another through neutral APIs." | `contract/transport/presence-discovery` (`05-interfaces.md` §6; the `Transport` presence operations, `spec/interfaces.md` §6.5-§6.6) | Contract — transport | CI-default (in-memory and real Zenoh over loopback — §3's loopback rule) | not-yet-written |
| 4 | "Claude actively messages Codex without receiver polling." | `contract/adapter/no-polling` (§5) + H1's clause 1 (§9) | Contract (CI-default) + end-to-end (opt-in) | CI-default for the contract half; opt-in for the E2E half | not-yet-written |
| 5 | "Codex actively replies to Claude." | H1's clause 2 (§9) | End-to-end | Opt-in | not-yet-written |
| 6 | "Authenticated provenance and authorization are enforced." | Security tier's spoof + unauthorized-routing tests (§7) | Security | CI-default (fakes); opt-in (real, H2) | not-yet-written |
| 7 | "Replay/duplicate handling exists." | Security tier's replay + duplicate-suppression tests, including across restart (§7) | Security | CI-default (fakes); opt-in (real, H2/H3) | not-yet-written |
| 8 | "No cross-provider model API invocation." | H1's clause 3 (§9) — asserted by the test itself — plus the `oac-boundaries` boundary-lint self-checks each planning file already carries | End-to-end (test) + lint (boundary) | Opt-in (E2E); CI-default (lint) | not-yet-written |
| 9 | "Zenoh-specific types stay inside its transport module." | The `oac-boundaries` CI lint (§8) — **not a runtime test** | Lint | CI-default | not-yet-written (lint itself pending — `core/`/`spec/` do not exist yet, per `docs/planning/decisions/C7-zenoh-transport.md` §2) |
| 10 | "Transport contract is documented enough to independently add a second backend." | The E9 design-for-replacement proofs, `docs/planning/decisions/E9-replacement-proofs.md` §3-§6 (task E9, #49), which supersede `05-interfaces.md` §17 — **a doc proof, not a test** | Doc proof | N/A | **MET** (2026-10-06, #49): NATS and MQTT walked against the frozen `spec/interfaces.md` §6, every capability cell cited to first-party sources. *(Dated note, 2026-10-06, #49: until now this cell read "not-yet-written; `05-interfaces.md` §22 ... records this proof as NOT MET as of the M0 draft — every NATS/MQTT capability cell is `UNVERIFIED`".)* |

---

## 12. Threat-mitigation traceability

One row per `docs/planning/v0.1/06-security.md` §14 mitigation, by row number — the
table body itself is **not restated here**; only the row number, a one-line attack label
for orientation, and the proving test already named at §7 above (or cited to §14's own
"Proving test" cell where this file does not repeat it). Any mitigation without a
currently passing proving test is an explicit v0.1 gap, per §14's row content and §15's
"unproven-mitigation disposition" — none of the twenty-four rows (row 24 added by #236,
2026-10-02) currently has a passing
**test** (F11/H2/H3/G3/G7/G8/G9 are all `NOT RUN`, Pre-Stage 0 — dated note, 2026-10-03,
#254: "Pre-Stage 0" is stale, read the current stage from `docs/planning/STATUS.md`
"Current stage"; none of these tests has run yet), though rows 5, 16, and
17's named gate has since run: gate **G1 `PASS`** (row 16 — originally on Claude Code
`v2.1.282`, invalidated when the pin went floating 2026-09-27, re-run and PASSED again
2026-09-28 on `v2.1.283`, see `docs/planning/gates/G1-result.md`), gate G2 `PASS` (row 17), and
gate G5 **FAIL** (Codex criteria 2/3 f; Claude all criteria x) (rows 5, 16, 17 — `docs/planning/gates/
G5-result.md`; dated note, 2026-10-03, #220: G5 is now **PASS** after its Codex-leg
re-run under C13 §11, so the "G5 FAIL" citations in the table below are history, and C13
is `RESOLVED-IN-DECISION`). A gate result is not one of this file's test tiers, so it does not by
itself close a row; see §14 rows 5, 16, 17 (`06-security.md`) for what each gate
actually confirmed. Rows 21-23 (L4, issue #169) cover an external memory service beside
OAC (`docs/planning/decisions/L1-beacon-memory.md`); rows 21-22's scenario is L10, an
opt-in provider-integration scenario blocked until Stage 5 opens, never CI-default
(§3). Row 23 has no proving test by design — it is an open risk outside OAC's control
(`06-security.md` §15), carried as `RISK-BEACON` in `docs/planning/v0.1/11-risks.md`.

| `06-security.md` §14 row | Attack (one line) | Proving test named at §7 or §14 | Status |
|---|---|---|---|
| 1 | Impersonation (forged envelope) | F11; H2 (§7 "Spoof") | v0.1 gap — `NOT RUN` |
| 2 | Unauthorized routing/discovery | F5; H2 (§7 "Unauthorized routing") | v0.1 gap — `NOT RUN` |
| 3 | Tampering (field mutation in flight) | F11; F4 | v0.1 gap — `NOT RUN` |
| 4 | Replay | F4 (§7 "Replay") | v0.1 gap — `NOT RUN` |
| 5 | Malicious peer prompt injection despite valid signature | gate G5; F11 | gate G5 **FAIL** (Codex criteria 2/3 f; Claude all criteria x) (2026-09-27, `docs/planning/gates/G5-result.md`); F11 `NOT RUN`; this row's own mitigation is doctrine, not a test-closable control (`06-security.md` §14 row 5) |
| 6 | Compromised transport infrastructure | F11; D3/G3 | v0.1 gap — `NOT RUN` |
| 7 | Accidental cross-project disclosure | H2 (§7 "Cross-project leakage") | v0.1 gap — `NOT RUN` |
| 8 | Leaked credentials / device-key exfiltration | F5; F11 | v0.1 gap — `NOT RUN` |
| 9 | Zenoh has no payload authentication | F11 | v0.1 gap — `NOT RUN` |
| 10 | Zenoh `zid` used as an identity/ACL subject | task G3 (§7 "`zid`-never-an-identity"; §8's lint-scope-gap applies here too) | v0.1 gap — `NOT RUN`; lint scope narrower than the row's full claim, per §8 |
| 11 | Permission-relay abuse | task G4; H2 | v0.1 gap — `NOT RUN` |
| 12 | Unauthorized Codex `turn/steer` | F11; task G7; gate G2 | v0.1 gap — `NOT RUN` |
| 13 | Local IPC peer spoofing | F11; G9 (§7 "Local IPC peer auth") | v0.1 gap — `NOT RUN` |
| 14 | Cross-project leakage via `list_sessions` | H2 (§7 "Cross-project leakage") | v0.1 gap — `NOT RUN` |
| 15 | Silently dropped `meta` key yielding unlabelled provenance | F8; task G4 | F8 fake reproduces the drop (`tests/fakes/claude/tests/replay.rs` `g5_meta_key_cases`, replaying G5 C4/C4b, passes); the adapter-side test (G4) is not written — still a v0.1 gap |
| 16 | Provenance spoofing via message body, Claude | gate G1; gate G5 | gate **G1 `PASS`** (originally on Claude Code `v2.1.282`, invalidated by the 2026-09-27 pin float, re-run and PASSED again 2026-09-28 on `v2.1.283` — `docs/planning/gates/G1-result.md`); gate **G5 `FAIL`; Claude criteria met** (2026-09-27, `docs/planning/gates/G5-result.md`) — still a v0.1 gap: a gate result is not one of this file's test tiers (F11/H2 `NOT RUN`), so it does not by itself close this row |
| 17 | Provenance spoofing via forged header/delimiter, Codex | gate G2; gate G5 | gate G2 `PASS`; gate **G5 `FAIL`; Codex criteria 2/3 f** (2026-09-27, `docs/planning/gates/G5-result.md`) — v0.1 gap, tracked as conflict-register entry C13, required before Stage 2's interface freeze on this surface |
| 18 | Reply misattribution via forged `in_reply_to` | task G8 | v0.1 gap — `NOT RUN`; whether Codex reliably echoes a header-supplied id back at all is itself UNVERIFIED (`docs/planning/STATUS.md`) |
| 19 | Stale-registration replay after resume | F4; H2 | v0.1 gap — `NOT RUN` |
| 20 | Session-id spoofing | F11; H2 | v0.1 gap — `NOT RUN` |
| 21 | Prompt injection via a memory reference or resolved memory | L10 (Stage 5 opt-in scenario); gate G5; F11 | v0.1 gap — L10 `NOT RUN` (blocked, Stage 5); gate G5 **FAIL** (2026-09-27, `docs/planning/gates/G5-result.md`); F11 `NOT RUN`; mitigation is doctrine plus rendering, as row 5 |
| 22 | False authority via a cited memory ID | L10 (memory ID absent from Claude `meta` and the Codex header block); gate G5; F11 | v0.1 gap — L10/F11 `NOT RUN`; inherits row 17's Codex residual until C13 is resolved |
| 23 | Capture of an OAC-delivered message by an external memory/telemetry service | None — open risk, not a mitigation (`06-security.md` §15); L2/L3 observe whether capture happens | Open risk — `RISK-BEACON`; capture itself UNVERIFIED (L1 §6 U1) |
| 24 | Session binding via a spoofed `CLAUDE_CODE_SESSION_ID` | F11; G9 (§7 "Local IPC peer auth") | v0.1 gap — `NOT RUN` |

*Dated note, 2026-10-07, #60 (F11): the security suite against the fakes has landed at
`tests/security/` (`oac-security-suite`, CI-default: no live provider, no API key, loopback
only). The table below is its threat-to-test map, rendered from `THREATS` in
`tests/security/src/lib.rs`; `tests/security/tests/threat_map.rs` fails if it drifts from
that map or names a test that does not exist, and `tests/security/check-compiled-tests.mjs`
(CI, every OS) fails if a test the map counts is not what cargo compiled and ran. "Proven" means every test named passes in the
default `cargo test` run and drives the real core (envelope stage, security steps 1 to 5,
authorization, presence and receipt authentication, pairing, key removal), with envelopes
over the in-memory transport where the row is about the carrying path, and the fake Claude
Code endpoint for what the harness renders. "Gated" names the part that needs a component
not built yet, with its owning issue; each gated test is `#[ignore = "GATED on #N ..."]` and
fails if run, so it is never a pass. A gated part keeps that part of the row a v0.1 gap, as
§15 of `06-security.md` requires. "Harness facts" are tests that record what the fake
Claude Code endpoint (replaying recorded harness behaviour) does with hostile content: a
precondition a mitigation relies on, never a proof of it. Row 16 is proven by its core-side
tests; its harness cases run the core's verified message through a stand-in for the G4
adapter's `meta` mapping (`stand_in_provenance`) and are listed as facts. Row 15 is gated:
its mitigation is the G4 adapter's, and its harness test is a fact. "Core tests" are
`oac-core` tests that carry a part this suite cannot reach through the public API (the
deterministic pairing-code binding, the window function at both ends), cited by path and
checked to exist. The table above is unchanged; where it says `NOT RUN` for F11, read this
table. The `S13-` rows are the `spec/security.md` §13 rows that 06 does not number, and
`threat_map.rs` checks that every §13 row is covered by some entry; `X-exhaustion` comes
from the PR #317 and #321 threat rows, not from §13.*

<!-- F11 threat map: generated from tests/security/src/lib.rs THREATS; begin -->

| Threat row | Attack | Proving tests (`tests/security/`, passing) | Harness facts (precondition, not proof) | Core tests (`oac-core`) | Gated, with owning issue | Status |
|---|---|---|---|---|---|---|
| 06-1 | Impersonation: a forged envelope | `row01_envelope_signed_by_an_unpaired_key_is_rejected`<br>`row01_claiming_a_trusted_principal_with_another_key_is_rejected`<br>`row01_a_trusted_key_id_with_a_forged_signature_is_rejected`<br>`row01_signature_from_another_trusted_device_does_not_verify` | none | none | none | proven |
| 06-2 | Unauthorized routing or discovery | `row02_trusted_peer_without_a_grant_is_rejected_unauthorized`<br>`row02_a_grant_is_one_way`<br>`row02_a_grant_for_one_session_does_not_cover_another`<br>`row02_unauthorized_peer_cannot_discover_a_session`<br>`row02_unauthorized_peer_cannot_address_a_session_through_presence`<br>`row02_reply_right_covers_only_the_reply_to_the_one_message`<br>`row02_unauthorized_send_through_the_composed_pipeline` | none | none | none | proven |
| 06-3 | Tampering in flight | `row03_rewriting_any_signed_member_breaks_the_signature`<br>`row03_a_flipped_signature_bit_is_rejected`<br>`row03_tampered_bytes_over_the_transport_are_rejected` | none | none | none | proven |
| 06-4 | Replay | `row04_copy_outside_the_replay_window_is_rejected`<br>`row04_copy_dated_ahead_of_the_window_is_rejected`<br>`row04_the_window_is_open_at_its_edge`<br>`row04_replay_inside_the_window_is_a_duplicate_handed_off_once`<br>`row04_expiry_is_checked_before_the_replay_window`<br>`row04_replay_after_restart_inside_the_window_is_the_named_residual`<br>`row04_transport_duplicates_are_handed_off_once`<br>`row04_receipt_flooding_by_replay_is_bounded` | none | `replay::tests::window_is_open_at_both_ends_at_full_precision` | none | proven |
| 06-5 | Prompt injection from an authenticated peer | `row05_hostile_content_is_delivered_as_content_and_obeyed_never`<br>`row05_content_never_reaches_an_authorization_decision`<br>`row05_the_decision_log_holds_no_content` | none | none | none | proven |
| 06-6 | Compromised transport infrastructure | `row06_forged_envelope_injected_on_the_transport_is_rejected`<br>`row03_tampered_bytes_over_the_transport_are_rejected`<br>`row04_transport_duplicates_are_handed_off_once` | none | none | #62, #64: the same cases over the real reference transport on loopback (G1 transport, G3 security configuration) | proven (core and fakes); gated part open |
| 06-7 | Accidental cross-project disclosure | `row07_scope_grant_does_not_cover_another_working_directory`<br>`row07_scope_grant_does_not_cover_a_subdirectory`<br>`row07_presence_is_released_only_to_granted_devices` | none | none | #74: H2's live cross-project check against real adapters | proven (core and fakes); gated part open |
| 06-8 | Leaked device key | `row08_removing_a_key_revokes_it_in_one_step`<br>`row08_captured_envelope_from_a_removed_key_is_rejected`<br>`row08_removal_survives_a_restart`<br>`row08_a_failed_save_still_revokes`<br>`row08_own_key_cannot_be_removed`<br>`row08_device_key_never_appears_in_debug_output` | none | none | none | proven |
| 06-9 | Transport-only authenticity assumed | `row09_a_payload_from_any_endpoint_is_judged_by_its_signature_alone`<br>`row06_forged_envelope_injected_on_the_transport_is_rejected` | none | none | none | proven |
| 06-10 | A transport peer identifier used as an identity | `row09_a_payload_from_any_endpoint_is_judged_by_its_signature_alone` | none | none | #64: ACL subjects of the reference transport's configuration (G3); grants are keyed by key id only, by construction | proven (core and fakes); gated part open |
| 06-11 | Permission-relay abuse | `row11_relay_is_off_by_default_even_with_a_device_wide_grant`<br>`row11_relay_is_enabled_for_one_session_only_by_the_operator`<br>`row11_a_deliver_permit_never_permits_relay` | none | none | #65: the Claude adapter's permission relay surface stays off (gated_row11_adapter_never_relays_without_a_relay_permit) | proven (core and fakes); gated part open |
| 06-12 | Steering a running turn | `row12_hand_off_is_made_at_most_once_and_no_outcome_steers`<br>`row12_no_decision_kind_enables_steering` | none | none | #68: the Codex adapter hands off queue-only and never calls a steering method (gated_row12_codex_hand_off_is_queue_only) | proven (core and fakes); gated part open |
| 06-13 | Local IPC peer spoofing | none | none | none | #70: the daemon's local IPC endpoint and its OS peer check (gated_row13_ipc_admits_only_the_same_user) | gated: v0.1 gap |
| 06-14 | Cross-project leakage through discovery | `row02_unauthorized_peer_cannot_discover_a_session`<br>`row14_discovery_lists_only_sessions_the_requester_may_reach` | none | none | #74: H2's live check of the discovery tool against real adapters | proven (core and fakes); gated part open |
| 06-15 | A silently dropped meta key leaves provenance unlabelled | none | `row15_the_harness_drops_unsafe_keys_so_provenance_keys_must_be_safe` | none | #65: the mitigation (const key table, partial provenance detected before send, message refused) is the Claude adapter's (gated_row15_adapter_refuses_a_partial_provenance_set) | gated: v0.1 gap |
| 06-16 | Provenance spoofing through the body, Claude | `row16_content_claiming_another_sender_does_not_change_provenance`<br>`row16_a_line_break_in_a_provenance_value_cannot_pass_the_envelope_stage` | `row16_forged_channel_tag_in_content_adds_no_attribute`<br>`row16_pre_escaped_closer_in_content_adds_no_attribute`<br>`row16_mid_turn_hostile_content_adds_no_attribute`<br>`row16_meta_key_injection_through_content_adds_no_attribute` | none | #65: the G4 adapter's own meta mapping (gated_row16_adapter_takes_provenance_only_from_verified_members); the exact text of a sender-written `<\/channel>` is a RenderGap until a capture records it | proven (core and fakes); gated part open |
| 06-17 | Provenance spoofing through a forged header or delimiter, Codex | none | none | none | #68: the Codex adapter's frame builder (gated_row17_codex_frame_uses_a_receiver_generated_delimiter) | gated: v0.1 gap |
| 06-18 | Reply misattribution through a forged in_reply_to | `row02_reply_right_covers_only_the_reply_to_the_one_message` | none | none | #69: the Codex adapter's reply correlation (gated_row18_codex_reply_correlation_is_not_trusted_alone) | proven (core and fakes); gated part open |
| 06-19 | Stale registration replay after resume | `row19_a_registration_record_signed_by_another_device_binds_nothing`<br>`row19_an_ended_session_receives_nothing` | none | none | #70: a session's lifetime tied to its IPC connection (gated_row19_session_lifetime_follows_the_ipc_connection) | proven (core and fakes); gated part open |
| 06-20 | Session-id spoofing | `row20_claiming_a_session_id_bound_to_another_key_is_refused_with_a_finding`<br>`row20_a_refused_claim_binds_nothing` | none | none | none | proven |
| 06-21 | Prompt injection through a memory reference | `row21_a_memory_reference_stays_content` | none | none | #175: L10's opt-in scenario with a real memory service (Stage 5) | proven (core and fakes); gated part open |
| 06-22 | False authority through a cited memory reference | `row22_a_cited_memory_reference_is_never_provenance_or_authority`<br>`row05_content_never_reaches_an_authorization_decision` | none | none | #175: L10's opt-in scenario with a real memory service (Stage 5) | proven (core and fakes); gated part open |
| 06-23 | Capture of delivered content by an external memory service | none | none | none | none | open risk (06 §15) |
| 06-24 | Session binding through a spoofed CLAUDE_CODE_SESSION_ID | none | none | none | #70: the daemon's binding of native signals to attachments (gated_row24_spoofed_session_variable_binds_nothing) | gated: v0.1 gap |
| S13-squatting | Session-id squatting by a related device, through presence | `s13_squatting_announcement_marks_conflict_and_fails_closed`<br>`s13_own_session_is_never_marked_under_conflict` | none | none | none | proven |
| S13-malleability | Signature malleability and weak or mixed-order points | `s13_malleable_and_weak_point_signatures_are_rejected` | none | `signing::tests::constructed_malleable_and_small_order_signatures_are_rejected` | none | proven |
| S13-cross-protocol | A signature over one kind of object presented as another | `s13_an_envelope_signed_under_another_domain_is_rejected`<br>`s13_a_registration_signed_under_the_envelope_domain_binds_nothing` | none | `signing::tests::a_signature_does_not_cross_domains` | none | proven |
| S13-canonicalization | Canonicalization mismatch between signer and verifier | `s13_canonical_forms_verify_and_altered_forms_do_not` | none | `tests/conformance.rs::conformance_fixtures` | none | proven |
| S13-retransmission | Duplicate suppression that blocks a legitimate retransmission | `s13_a_copy_not_handed_off_does_not_block_its_retransmission`<br>`row12_hand_off_is_made_at_most_once_and_no_outcome_steers` | none | none | none | proven |
| S13-existence-oracle | An unauthorized sender learns whether a session exists | `s13_unauthorized_refusal_is_the_same_whether_or_not_the_session_exists` | none | none | none | proven |
| S13-consent | Bypass of a harness's own consent step | none | none | none | #74: [SEC-AUZ-023] is checked by H2's review of the launch path (gated_s13_no_harness_consent_step_is_automated) | gated: v0.1 gap |
| S13-presence-forgery | Presence forgery, tampering, forwarding or replay | `s13_presence_signed_by_an_unpaired_key_is_discarded`<br>`s13_tampered_presence_record_is_discarded`<br>`s13_forwarded_presence_record_is_discarded`<br>`s13_replayed_stale_presence_record_is_discarded`<br>`s13_unrelated_issuer_cannot_announce`<br>`s13_forged_withdrawal_cannot_take_a_session_offline` | none | none | none | proven |
| S13-receipt-forgery | Receipt forgery, or a receipt from the wrong receiver | `s13_receipt_from_an_unpaired_key_is_discarded`<br>`s13_receipt_from_another_trusted_device_is_discarded`<br>`s13_altered_receipt_is_discarded`<br>`s13_receipt_for_an_envelope_never_sent_is_discarded`<br>`s13_receipt_with_the_wrong_nonce_is_discarded`<br>`s13_receipt_claiming_the_sender_observer_is_discarded`<br>`s13_no_receipt_for_an_unverified_copy` | none | none | none | proven |
| S13-pairing | Trust on first use, and a party in the middle of pairing | `s13_pairing_mitm_substitution_is_caught_by_the_code`<br>`s13_second_offer_ends_the_pairing`<br>`s13_five_wrong_codes_abort_and_the_window_expires`<br>`s13_key_id_comparison_refuses_a_substituted_key`<br>`s13_a_signature_alone_never_makes_a_key_trusted` | none | `pairing::tests::substituted_key_or_nonce_changes_the_code_or_fails` | none | proven |
| X-exhaustion | Registry and quota exhaustion (PR #317 and #321 threat rows; spec §8.3, §8.4, §11) | `x_full_duplicate_store_refuses_without_evicting`<br>`x_one_issuer_cannot_fill_the_presence_registry`<br>`x_presence_registry_capacity_is_bounded`<br>`x_receipt_allowance_is_per_device_and_bounded`<br>`x_oversized_envelope_is_refused_before_parsing` | none | none | #325: binding-table entries that security step 4 creates are not bounded yet (gated_x_envelope_bindings_are_bounded) | proven (core and fakes); gated part open |

<!-- F11 threat map: end -->

---

## 13. Fixture capture and refresh process

**Capture, during Stage 1 spikes (task D6), from the real harnesses.** Per
`docs/planning/backlog/03-tasks-CD.json` task D6, fixtures are recorded wire traffic —
not hand-transcribed — from a real Claude Code channel session and a real Codex
app-server, captured as part of Stage 1's gate spikes:

- Claude fixtures cover: initialize/negotiation, notification delivery, mid-turn
  queueing, and a tool reply.
- Codex fixtures cover: `initialize`/`initialized`, `thread/start`, `thread/resume`,
  `turn/start`, `thread/queue/add`, and the event stream including `item/completed` and
  `turn/completed`.

**Per-gate location, per `docs/planning/gates/README.md`.** Captured fixtures for gate
G<n>'s spike live at `docs/planning/gates/fixtures/g<n>-<slug>/` (e.g.
`g1-claude-wake/`), redacted per `oac-gates`' fixture-capture procedure. Full command
transcripts, when kept, live alongside them as `transcript-<YYYY-MM-DD>…`. (Path form
corrected 2026-09-28, K5, to match the committed tree and the README.)

**Every fixture carries its pinned version and capture date — no exception.** D6's own
acceptance box: "Each fixture records the pinned version and capture date." A fixture
with neither is incomplete, not merely under-documented.

**No credentials, tokens, or private paths.** D6's own acceptance box, stated as a hard
requirement, not a preference: "Fixtures contain no credentials, tokens, or private
paths."

**Codex schema artifacts referenced, never hand-transcribed.** D6's own acceptance box:
"Codex schema artifacts (`codex-rs/app-server-protocol/schema`) referenced rather than
hand-transcribed." A fixture's Codex message shapes are captured from real traffic and
checked against that schema source, not typed from memory of what the schema is expected
to say.

**Never hand-write a fixture to make a test pass — a missing fixture is a Stage 1 gap to
report.** If a test needs behaviour no D6 fixture yet covers, that is a gap in D6's
capture pass, reported and closed by re-running the capture against the real harness —
never closed by authoring a fixture file by hand. A hand-written fixture would make a
fake lie about what the real harness actually does, defeating the fake's entire purpose.

**Refresh trigger: a pin move requires re-capture plus gate re-run.** Per
`docs/planning/gates/README.md`'s "Re-run/invalidation policy," a version bump to the
Claude Code or Codex pin invalidates the gate(s) that pin affects (G1/G4/G5 for Claude
Code; G2/G5 for Codex) and requires the same-commit edits that policy's §b lists. This
file adopts the identical trigger for fixture freshness: a pin move against the surface a
fixture was captured from requires re-capturing that fixture against the new pinned
version before it is trusted again, in the same change that re-runs the affected gate(s).
*Dated note, 2026-10-01 (#216, operator decision):* a Claude Code or Codex version change no longer invalidates a gate, so it no
longer forces a re-capture either. Each fixture records the version it was captured on
(`MANIFEST.json` `observed_version`); one not captured on PINS.md's last tested version
carries `version_matches_pin: false`, which is a warning. Re-capture when a newer
version changes the behaviour a fixture documents.

*Dated note, 2026-10-06, #57 (F8): the fake Claude endpoint, `tests/fakes/claude/`, reads
the fixtures in place from `docs/planning/gates/fixtures/` (compiled in), so there is no
runtime copy under `tests/` to drift. Each value it sends or renders is checked against a
named fixture line by `tests/fakes/claude/tests/replay.rs`; behaviour no fixture shows makes
it halt, refuse or leave the text unfixed, never guess (crate docs).*

**The Stage 3 runtime fixture path under `tests/` is deliberately not fixed here.**
`docs/planning/DESIGN.md`'s "Suggested repository shape" sketches
`tests/{protocol,security,integration}/` and labels it "Suggested," not fixed — this file
does not invent a subpath under `tests/` for where F8/F9's fake endpoints load fixtures
from at runtime. That path is confirmed by F8 and F9 themselves when they land (Stage 3),
per `docs/planning/v0.1/07-repository-and-dependencies.md` §1's identical deferral for the
`tests/` module row.

*Dated note, 2026-10-06 (#58, F9): the fake Codex app-server is at
`tests/fakes/codex-app-server/`. It loads the recorded fixtures in place, from
`docs/planning/gates/fixtures/` (D6 `d6-codex-protocol/`, G2 and G5 Codex transcripts),
and copies none of them, so `MANIFEST.json` stays the one inventory. It is a Node process
(built-ins only) over stdio and loopback WebSocket, so a Rust contract test spawns it by
path. It sits under `tests/fakes/`, not `tests/integration/`, because the latter is the
opt-in leaf that no product path may reference (`scripts/check-herdr-containment.mjs`
check 9), and the fakes are CI-default. Its provenance table, source-only behaviours and
not-modelled list are its `README.md`.*

---

## 14. Test-location policy

Module ownership for `tests/` is `docs/planning/v0.1/07-repository-and-dependencies.md`
§2's job, cited here, not restated: `tests/{protocol,security,integration}/` holds
"fixtures and fake endpoints for provider/transport contract tests," may depend on
`core/` and whichever module a given test targets, and owns no production code shipped in
the `oac` binary.

**The sketch is a sketch, not a commitment.** Per `docs/planning/DESIGN.md`'s own
caveat on its "Suggested repository shape," and per `oac-testing`'s own §3: inventing an
ad hoc test location before F8/F9 confirm the actual Stage 3 layout is forbidden. Any
fixture path this file or a future test-writing task needs is provisional until F8/F9
land.

*Dated note, 2026-10-06, #57: F8 confirms the fake-endpoint location: `tests/fakes/<name>/`.
The fake Claude endpoint is `tests/fakes/claude/`, a test-only workspace member that
product crates may take as a dev-dependency only (`07-repository-and-dependencies.md` §1,
§3).*

---

## 15. Normative vs. reference-implementation split, applied throughout

What **binds** a future non-Rust implementation of OAC, versus what is specific to the
v0.1 Rust test harness:

**Binds any conformant implementation:**

- The E8 conformance fixture set itself (§6) — a language-independent data format any
  implementation runs to claim conformance, per E8's own acceptance box: "Fixtures are
  data files, not code, and are readable by a second implementation."
- The no-polling assertion's *observable behaviour* (§5): attach/subscribe-establishment
  calls stay flat, on-demand fetch calls stay zero between arrivals. This is a behaviour
  any adapter implementation must exhibit, not a Rust-specific test-harness detail.
- The error-code-to-delivery-state mapping (`05-interfaces.md` §10) and the frozen
  delivery-state set (`05-interfaces.md` §9) that §6's fixtures exercise.
- The CI-default rule itself (§3): no live provider, no API key, no network beyond
  loopback — a property of *what a default test run may touch*, not of any one language's
  test runner.
- The eight-tier taxonomy (§2) as a description of *what gets proven*, independent of how
  any one implementation's build tooling organizes its test suites.

**Does not bind — v0.1 Rust workspace / `cargo test` harness detail only:**

- The exact Cargo feature/target mechanism §4's reference-implementation note names for
  separating opt-in from default tests.
- Where under `tests/` a fixture physically loads from at runtime (§13's deferral, §14).
- The specific instrumentation technique (e.g. a mock/spy crate) a Rust test uses to
  count attach/subscribe/fetch calls for §5's assertion — the *count itself* is
  normative; the mechanism that produces the count is not.
- `cargo test`'s own default-vs-`--ignored` conventions, or whatever CI job-matrix
  mechanism F12/H4 use to run the three-platform matrix (§9, §10) — the *requirement*
  that all three platforms run is normative; the CI system that schedules them is not.

---

## 16. Deliberately omitted from v0.1 — smallest-implementation rule

Each omission, one line with reason, per `oac-planning-package`'s "smallest
implementation that proves v0.1" house-style rule:

- **No performance/load tier.** v0.1 proves correctness and the ADR-001 Validation
  criterion, not throughput or latency under load — no acceptance criterion (§11) or
  threat-table row (§12) requires a performance claim.
- **No fuzzing tier.** The closed error taxonomy (§6) and its ten negative fixtures
  already exercise the malformed/adversarial-input surface the spec defines; open-ended
  fuzzing is a strictly larger surface than what any v0.1 acceptance criterion or threat
  row requires proven.
- **No mutation testing.** Mutation testing measures test-suite quality, not the system's
  own behaviour against a named criterion or threat — out of scope for a strategy whose
  every tier traces to a criterion (§11) or a mitigation (§12).
- **No persistence tests.** Per `docs/planning/DESIGN.md`'s own open planning question
  ("Whether v0.1 needs persistence or only live delivery") resolved toward live-delivery
  only, and per `docs/planning/decisions/C5-envelope-auth.md` §8's explicit "not a
  durable offline mailbox" boundary check — v0.1 has no durable message store to test the
  persistence of.

Each of the above also belongs in `docs/planning/v0.1/12-deferred.md` (task A12, not yet
landed) as an explicit "not in v0.1" entry with its reason restated in that file's own
format — this file names them here as the test-strategy consequence of that same v0.1
scope limit, not as a duplicate ledger.

---

## 17. Evidence pass

Per `oac-evidence` §8, checked against this file:

- Every provider surface named carries its label at first mention: Claude Code Channels
  = **research preview** (version floating per `docs/planning/PINS.md`, §9); Codex
  app-server = **experimental (per-method gating)** (version floating per
  `docs/planning/PINS.md`, §9) — both cited via
  `docs/planning/v0.1/08-cli-and-deployment.md`, which itself carries the label.
- Every version reference is taken from `docs/planning/PINS.md`, cited by pointer
  (§4, §9), never restated as "current" or "latest." The Claude Code and Codex rows
  there are floating (a last-observed version, not a fixed pin); every other version
  is a fixed pin. (Note, 2026-10-01, issue #186: this pass previously listed the
  then-fixed pins `v2.1.274` and `@openai/codex@0.154.0` and said every version was
  "pinned from" `PINS.md`.)
- **Existing UNVERIFIED items this file leans on, carried with their original reason, not
  restated as settled:** the Codex daemon-attach-default question (§9, via
  `docs/planning/v0.1/08-cli-and-deployment.md` §10, which itself carries the reason);
  whether Codex reliably echoes a header-supplied id back as `in_reply_to` (§12 row 18,
  via `docs/planning/v0.1/06-security.md` §14 row 18, which carries the reason); the six
  NATS/MQTT optional-capability items from task E9/A6 (§11 row 10, via `05-interfaces.md`
  §17/§20, which carries the reason). All three already appear in
  `docs/planning/STATUS.md`'s "Open UNVERIFIED items" list — not restated as new here.
  *(Dated note, 2026-10-06, #49: the NATS/MQTT items are closed. E9 re-verified them against
  first-party sources, `docs/planning/decisions/E9-replacement-proofs.md` §9, and removed
  them from STATUS.md.)*
- **No new UNVERIFIED item is added by this file.** Every fact this file states is either
  a design decision this file itself makes (the resilience-into-security fold, §2; the
  `contract/adapter/no-polling` test name, §5) or a citation to an already-landed source.
  `docs/planning/STATUS.md`'s "Open UNVERIFIED items" list is unchanged by this change.

---

## 18. Boundary pass

Per `oac-boundaries`'s pre-commit self-check, confirmed for this file:

- **No test in this file implies OAC calling a provider model API, owning a turn loop, or
  holding provider credentials.** §9's end-to-end test asserts the *absence* of
  cross-model-API credential use (clause 3) — it does not describe OAC making such a
  call; §5's no-polling assertion describes an adapter receiving delivery, never running
  a turn.
- **No adapter described here polls an inbox while claiming active inbound.** §5's
  assertion is precisely the mechanical check that a passing adapter does not poll; §7's
  security tier and §9's end-to-end tier both build on that same assertion rather than
  relaxing it.
- **No Zenoh, Claude, Codex, or MCP method name appears inside a neutral-interface test
  description.** Provider/transport vocabulary in this file is confined to: §7's named
  security tests (which name real provider surfaces because they are provider-integration
  or provenance tests, not neutral-interface descriptions); §8's Zenoh-containment lint
  discussion; §9's end-to-end tier (which by definition names both real providers); §10's
  CLI smoke tier (citing documented CLI flags/commands by path); and §13's fixture
  capture detail (naming the real harnesses fixtures are captured from). §2's tier
  taxonomy table itself, §5's no-polling spec, §6's conformance detail, §11's criteria
  table, §14's location policy, §15's normative split, and §16's omissions list all stay
  in neutral vocabulary — no Zenoh/Claude/Codex/MCP method name appears in any of those
  sections.

---

## 19. Acceptance close-out

Checked against issue #30's four acceptance boxes:

- [x] **Each of DESIGN acceptance criteria 1-10 maps to a named test** — §11 (all ten
      rows, criterion text verbatim from `docs/planning/DESIGN.md`, each naming its test
      or, for criteria 9-10, its lint/doc-proof per H5's own acceptance wording).
- [x] **CI default tier runs with no live provider, no API key, and no network beyond
      loopback** — §3 (the rule, loopback defined explicitly, the anti-exception rule
      stated).
- [x] **Provider integration tests isolated behind an explicit opt-in and pinned
      versions** — §4 (separate flag/target, never a default-tier waiver; every opt-in
      test names its exact pinned version from `docs/planning/PINS.md`; a pin move
      invalidates the recorded result the same way it invalidates a gate, per
      `docs/planning/gates/README.md`; since #216, 2026-10-01, harness versions are
      recorded and warn, never invalidate).
- [x] **Fixture capture and refresh process defined** — §13 (Stage 1 capture via D6,
      per-gate location per `docs/planning/gates/README.md`, pinned-version-plus-date
      requirement, no-credentials rule, Codex-schema-reference rule, the
      never-hand-write rule, and the pin-move refresh trigger).

---

## 20. Cross-file updates in this change

- `docs/planning/STATUS.md`: prepend an **A10** entry to the `**Last updated:**` history
  block (additive only, nothing removed) — `docs/planning/v0.1/09-test-strategy.md`
  landed (issue #30): the eight-tier taxonomy with resilience folded into the security
  tier and the Zenoh-containment criterion resolved to the lint (both decided, not left
  open), the CI-default/opt-in split and its pin-move-invalidation mechanics, the
  no-polling assertion spec naming `contract/adapter/no-polling`, the two mandatory
  traceability tables (all ten acceptance criteria, all threat-table rows then in §14,
  rows 1-20; L4 later added rows 21-23 — every
  row `not-yet-written`/`NOT RUN`, none marked done), and the fixture capture/refresh
  process. No pin moved, no gate verdict changed, no new UNVERIFIED item added — every
  UNVERIFIED item this file cites (§17) was already open, carried from
  `docs/planning/v0.1/06-security.md`, `08-cli-and-deployment.md`, and `05-interfaces.md`.
- `docs/planning/v0.1/05-interfaces.md` §18: its forward reference currently reads "task
  A10, not yet landed" — the parenthetical is dropped now that this file exists (edited
  in this same change; that file's own §-reference sweep is re-run after the edit, per
  its own pre-commit mechanical check).

---

## 21. Cross-reference block

Every reference below is a repo-relative path; no prior context is assumed.

- `docs/planning/ADR-001.md`
- `docs/planning/DESIGN.md`
- `docs/planning/PLANNING-PROMPT.md` §6, §8, §9 item 10, §11 item 2
- `docs/planning/v0.1/03-decisions-and-amendments.md`
- `docs/planning/v0.1/05-interfaces.md` (§13-§15 superseded by `spec/interfaces.md`)
- `spec/interfaces.md` (frozen: §5, §6)
- `docs/planning/v0.1/06-security.md`
- `docs/planning/v0.1/07-repository-and-dependencies.md`
- `docs/planning/v0.1/08-cli-and-deployment.md`
- `docs/planning/v0.1/10-stages.md` (task A11, not yet landed)
- `docs/planning/v0.1/11-risks.md` (task A12, not yet landed)
- `docs/planning/v0.1/12-deferred.md` (task A12, not yet landed)
- `docs/planning/decisions/C5-envelope-auth.md`
- `docs/planning/decisions/C7-zenoh-transport.md`
- `docs/planning/decisions/K1-herdr-evaluation.md` (§4 dated note, K5)
- `docs/planning/gates/README.md`
- `docs/planning/PINS.md`
- `docs/planning/STATUS.md`
- `docs/planning/backlog/03-tasks-CD.json` (task D6)
- `docs/planning/backlog/04-tasks-EF.json` (tasks E8, F8-F12)
- `docs/planning/backlog/05-tasks-GHIJ.json` (tasks H1-H5)
