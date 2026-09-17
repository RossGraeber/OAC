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

**Scope.** This file owns four things: the eight-tier test taxonomy (§2), the
CI-default-versus-opt-in split and its mechanics (§3-§4), the fixture capture and
refresh process (§13), and the two mandatory traceability tables — DESIGN acceptance
criteria 1-10 to named test (§11), and threat-table mitigation to named test (§12). It
does **not** own the threat table itself — every threat row is
`docs/planning/v0.1/06-security.md` §14's, cited here by row, never restated — nor the
stage-gate pipeline's entry/exit criteria (`docs/planning/v0.1/10-stages.md`, task A11,
not yet landed) nor the module layout that places `tests/` (`docs/planning/v0.1/
07-repository-and-dependencies.md` §1-§2). Cited by path throughout, not re-derived.

**Proof caveat, stated once, applying to every section below.** Per
`docs/planning/STATUS.md`'s Gate verdicts table, every gate verdict (G1-G5) is
`NOT RUN`, and per its "Current stage" table the project is at **Pre-Stage 0** — no
E8/F8-F12/H1-H5 test exists yet. This file is the **design** these tasks build against,
not a report of tests that pass. Every "proves"/"asserts" statement below names what a
built test will assert once its owning task lands, not a result.

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
| 2 | Spec conformance | Wire representation matches the frozen closed error taxonomy (`05-interfaces.md` §10, 10 rows), the delivery-state set (§9, 8 states), and the `ttl_ms`-before-replay-window precedence rule (§10) — detail at §6 below | The E8 conformance fixture set | CI-default | E8; wired into CI by F12 | Stage 2 (fixture set built); wired into CI at Stage 3 |
| 3 | Contract — adapter + transport | The `ProviderAdapter` (`05-interfaces.md` §13) and `Transport` (§15) contracts are obeyed identically by every implementation, including the no-polling rule (§5 below) | Adapter sub-row: fake Claude/Codex endpoints (F8/F9) at Stage 3, real adapters unchanged at Stage 4. Transport sub-row: the in-memory transport (F7) at Stage 3, real Zenoh over loopback unchanged at Stage 4 | CI-default against fakes/in-memory; against real adapters or real Zenoh it is the provider-integration tier (row 6), opt-in, pinned | F10 | Stage 3 (fakes/in-memory); Stage 4 (real, opt-in) |
| 4 | Security (resilience folded in, see above) | Spoof, replay, duplicate-suppression-including-across-restart, unauthorized-routing, cross-project-leakage, `zid`-never-an-identity, and local-IPC-peer-auth mitigations from `06-security.md` §14 — detail at §7 below | Fakes (F11) at Stage 3; real transport/adapters (H2, and H3 for the resilience sub-row) at Stage 5 | CI-default against fakes; against real providers/transport it is opt-in, pinned | F11; H2; H3 | Stage 3 (fakes); Stage 5 (real, opt-in for provider-touching cases) |
| 5 | Fake-harness integration | Core + adapter + transport wired together with no live provider — the Stage 3 exit condition | Fake Claude endpoint (F8), fake Codex endpoint (F9), in-memory or loopback transport (F7) | CI-default | F12 | Stage 3 |
| 6 | Provider integration | Real adapter/transport behaviour against a real, pinned harness version — the runtime half of gates G1/G2/G3 | Real Claude Code / real Codex / real Zenoh, on pinned versions (`docs/planning/PINS.md`) | Opt-in only, explicit flag, pinned versions — never the default `test` run (F12 acceptance: "Provider integration tests exist but are opt-in and pinned") | F12 isolates it as its own target; exercised at Stage 4 | Stage 4 |
| 7 | End-to-end | The ADR-001 Validation criterion, decomposed into individually asserted clauses — detail at §9 below | Real providers, real transport | Opt-in (needs live providers) — the go/no-go test (H1, labelled `go-no-go`) | H1 | Stage 5 |
| 8 | Cross-platform CLI smoke | One-command startup, `status`/`sessions`/`doctor`, clean shutdown — detail at §10 below | The built CLI binary; no live provider required | CI-default, matrixed on Windows, macOS, Linux | H4 | Stage 5 |

---

## 3. The CI-default rule

Stated normative-style, binding on the default pipeline F12 stands up:

The default test suite — every tier row above marked CI-default (rows 1, 2, 3's
fake/in-memory half, 4's fake half, 5, 8) — **runs with no live provider, no API key, and
no network beyond loopback.**

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
(row 6) names the Claude Code, Codex, or Zenoh pin-table row it runs against
(`docs/planning/PINS.md` pin table); the end-to-end tier (row 7, §9 below) names all
three pins it depends on (Claude Code, Codex, and the transport pin, since it crosses
both adapters and Zenoh). No opt-in test description says "the current release" or
"latest" — it names the exact pinned version string, the same string
`docs/planning/gates/README.md`'s per-gate result files record.

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
- **The frozen delivery-state set** (§9, 8 states: `accepted-by-adapter`,
  `handed-to-harness`, `unknown`, `rejected`, `unreachable`, `expired`, `duplicate`,
  `failed`) — each state reachable from at least one positive or negative fixture.
- **The `ttl_ms`-before-replay-window precedence rule** (§10): "a receiver `MUST` check
  `ttl_ms` expiry first and emit `expired` if it fails, checking the replay accept-window
  (and emitting `replay-window-rejected` on failure) only once the `ttl_ms` check has
  passed" — a dedicated fixture exercises an envelope that fails both checks, asserting
  only `expired` is emitted, never `replay-window-rejected`, per that precedence.

**Adding an error row is a versioning event — a new fixture is required, not optional.**
Per `docs/planning/v0.1/05-interfaces.md` §11's versioning policy: an added error code an
old peer treats as `failed` is non-breaking and requires only a spec-revision bump; either
way, the new row's own conformance fixture must land in the same change that adds it —
the fixture set is never allowed to lag the error taxonomy it exists to prove.

---

## 7. Security tier detail

Named tests, each citing the decision section it proves rather than re-deriving the
mitigation:

- **Spoof.** A forged or altered signature is rejected with `signature-verification-failed`,
  mapping to the `rejected` delivery state (`05-interfaces.md` §10; `docs/planning/
  decisions/C5-envelope-auth.md` §2, §5). Owning tier: F11 (fakes), H2 (real).
- **Replay.** An envelope outside the `created_at` accept-window is rejected with
  `replay-window-rejected` (C5 §7). Owning tier: F11, H2.
- **Duplicate suppression by `(key_id, nonce)`, including across a daemon restart.** The
  dedup key is `(security.key_id, security.nonce)`, not `id` alone (C5 §7); the
  cold-restart gap — the window itself, not the dedup store, is the only defence for the
  interval immediately following a restart — is asserted as a named, accepted residual
  risk, not silently passed over (C5 §8). Owning tier: F11 (in-process store behaviour);
  **H3** (the resilience sub-row folded into this tier per §2 — "Duplicate suppression
  holds across restart" is H3's own acceptance box).
- **Unauthorized routing.** A peer not on a session's allowlist is rejected with
  `unauthorized`, per the default-deny, per-session-id, `working_directory`-scoped
  allowlist (C5 §11). Owning tier: F11, H2.
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
  v0.1/08-cli-and-deployment.md` §7 — surface label **research preview**, pinned
  `v2.1.274`).
- Codex: attached via the shared local app-server daemon, or, on the documented
  fallback, `codex --remote` (`docs/planning/v0.1/08-cli-and-deployment.md` §9-§10 —
  surface label **experimental (per-method gating)**, pinned `@openai/codex@0.154.0`).

H1's own acceptance wording, quoted: "A live Claude Code session, launched OAC-enabled,
sends to a live Codex session and the Codex model answers without any receiver-side
polling." A test that spins up an arbitrary pre-existing process and expects attach
without an OAC-enabled launch is not testing what H1, or ADR-001-A2, requires, and cannot
pass.

**Must run on all three platforms — a single-platform pass is not done.** H1's own
acceptance box: "Runs on Windows, macOS, and Linux on the pinned provider versions." Per
§4's opt-in mechanics, each platform run is pinned against the same
`docs/planning/PINS.md` Claude Code, Codex, and Zenoh rows — a pin move on any of the
three invalidates every recorded platform result for this test, not just one.

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
quietly marked done."

| # | Criterion (verbatim) | Named test | Tier | CI-default / opt-in | Current status |
|---|---|---|---|---|---|
| 1 | "One-command local startup." | Cross-platform CLI smoke — one-command start (§10) | CLI smoke | CI-default | not-yet-written |
| 2 | "Claude and Codex adapters expose distinct neutral sessions." | Adapter contract suite (`ProviderAdapter.discover_sessions`/`attach`, `05-interfaces.md` §13) | Contract — adapter | CI-default (fakes); opt-in (real, row 6) | not-yet-written |
| 3 | "Sessions discover one another through neutral APIs." | Presence/discovery contract assertions (`05-interfaces.md` §6, §15's `watch_presence`) | Contract — transport | CI-default (in-memory); opt-in (real Zenoh) | not-yet-written |
| 4 | "Claude actively messages Codex without receiver polling." | `contract/adapter/no-polling` (§5) + H1's clause 1 (§9) | Contract (CI-default) + end-to-end (opt-in) | CI-default for the contract half; opt-in for the E2E half | not-yet-written |
| 5 | "Codex actively replies to Claude." | H1's clause 2 (§9) | End-to-end | Opt-in | not-yet-written |
| 6 | "Authenticated provenance and authorization are enforced." | Security tier's spoof + unauthorized-routing tests (§7) | Security | CI-default (fakes); opt-in (real, H2) | not-yet-written |
| 7 | "Replay/duplicate handling exists." | Security tier's replay + duplicate-suppression tests, including across restart (§7) | Security | CI-default (fakes); opt-in (real, H2/H3) | not-yet-written |
| 8 | "No cross-provider model API invocation." | H1's clause 3 (§9) — asserted by the test itself — plus the `oac-boundaries` boundary-lint self-checks each planning file already carries | End-to-end (test) + lint (boundary) | Opt-in (E2E); CI-default (lint) | not-yet-written |
| 9 | "Zenoh-specific types stay inside its transport module." | The `oac-boundaries` CI lint (§8) — **not a runtime test** | Lint | CI-default | not-yet-written (lint itself pending — `core/`/`spec/` do not exist yet, per `docs/planning/decisions/C7-zenoh-transport.md` §2) |
| 10 | "Transport contract is documented enough to independently add a second backend." | `05-interfaces.md` §17's NATS/MQTT design-for-replacement proof (task E9) — **a doc proof, not a test** | Doc proof | N/A | not-yet-written; `05-interfaces.md` §22's own acceptance close-out already records this proof as **NOT MET as of the M0 draft** — every NATS/MQTT capability cell is `UNVERIFIED` |

---

## 12. Threat-mitigation traceability

One row per `docs/planning/v0.1/06-security.md` §14 mitigation, by row number — the
table body itself is **not restated here**; only the row number, a one-line attack label
for orientation, and the proving test already named at §7 above (or cited to §14's own
"Proving test" cell where this file does not repeat it). Any mitigation without a
currently passing proving test is an explicit v0.1 gap, per §14's row content and §15's
"unproven-mitigation disposition" — none of the twenty rows currently has a passing
test, because none of F11/H2/H3/G3/G7/G8/G9/gate G1-G5 has run (Pre-Stage 0).

| `06-security.md` §14 row | Attack (one line) | Proving test named at §7 or §14 | Status |
|---|---|---|---|
| 1 | Impersonation (forged envelope) | F11; H2 (§7 "Spoof") | v0.1 gap — `NOT RUN` |
| 2 | Unauthorized routing/discovery | F5; H2 (§7 "Unauthorized routing") | v0.1 gap — `NOT RUN` |
| 3 | Tampering (field mutation in flight) | F11; F4 | v0.1 gap — `NOT RUN` |
| 4 | Replay | F4 (§7 "Replay") | v0.1 gap — `NOT RUN` |
| 5 | Malicious peer prompt injection despite valid signature | gate G5; F11 | v0.1 gap — `NOT RUN`; this row's own mitigation is doctrine, not a test-closable control (`06-security.md` §14 row 5) |
| 6 | Compromised transport infrastructure | F11; D3/G3 | v0.1 gap — `NOT RUN` |
| 7 | Accidental cross-project disclosure | H2 (§7 "Cross-project leakage") | v0.1 gap — `NOT RUN` |
| 8 | Leaked credentials / device-key exfiltration | F5; F11 | v0.1 gap — `NOT RUN` |
| 9 | Zenoh has no payload authentication | F11 | v0.1 gap — `NOT RUN` |
| 10 | Zenoh `zid` used as an identity/ACL subject | task G3 (§7 "`zid`-never-an-identity"; §8's lint-scope-gap applies here too) | v0.1 gap — `NOT RUN`; lint scope narrower than the row's full claim, per §8 |
| 11 | Permission-relay abuse | task G4; H2 | v0.1 gap — `NOT RUN` |
| 12 | Unauthorized Codex `turn/steer` | F11; task G7; gate G2 | v0.1 gap — `NOT RUN` |
| 13 | Local IPC peer spoofing | F11; G9 (§7 "Local IPC peer auth") | v0.1 gap — `NOT RUN` |
| 14 | Cross-project leakage via `list_sessions` | H2 (§7 "Cross-project leakage") | v0.1 gap — `NOT RUN` |
| 15 | Silently dropped `meta` key yielding unlabelled provenance | F8; task G4 | v0.1 gap — `NOT RUN`; neither test exists yet |
| 16 | Provenance spoofing via message body, Claude | gate G1; gate G5 | v0.1 gap — `NOT RUN` |
| 17 | Provenance spoofing via forged header/delimiter, Codex | gate G2; gate G5 | v0.1 gap — `NOT RUN` |
| 18 | Reply misattribution via forged `in_reply_to` | task G8 | v0.1 gap — `NOT RUN`; whether Codex reliably echoes a header-supplied id back at all is itself UNVERIFIED (`docs/planning/STATUS.md`) |
| 19 | Stale-registration replay after resume | F4; H2 | v0.1 gap — `NOT RUN` |
| 20 | Session-id spoofing | F11; H2 | v0.1 gap — `NOT RUN` |

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
G<n>'s spike live at `docs/planning/gates/fixtures/G<n>/`, redacted per `oac-gates`'
fixture-capture procedure. Full command transcripts, when kept, live alongside them at
`docs/planning/gates/fixtures/G<n>/transcript-<YYYY-MM-DD>.txt`.

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

**The Stage 3 runtime fixture path under `tests/` is deliberately not fixed here.**
`docs/planning/DESIGN.md`'s "Suggested repository shape" sketches
`tests/{protocol,security,integration}/` and labels it "Suggested," not fixed — this file
does not invent a subpath under `tests/` for where F8/F9's fake endpoints load fixtures
from at runtime. That path is confirmed by F8 and F9 themselves when they land (Stage 3),
per `docs/planning/v0.1/07-repository-and-dependencies.md` §1's identical deferral for the
`tests/` module row.

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
  delivery-state set (§9) that §6's fixtures exercise.
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
  = **research preview** (`v2.1.274`, §9); Codex app-server = **experimental
  (per-method gating)** (`@openai/codex@0.154.0`, §9) — both cited via
  `docs/planning/v0.1/08-cli-and-deployment.md`, which itself carries the label.
- Every version reference is pinned from `docs/planning/PINS.md`, cited by pointer
  (§4, §9), never restated as "current" or "latest."
- **Existing UNVERIFIED items this file leans on, carried with their original reason, not
  restated as settled:** the Codex daemon-attach-default question (§9, via
  `docs/planning/v0.1/08-cli-and-deployment.md` §10, which itself carries the reason);
  whether Codex reliably echoes a header-supplied id back as `in_reply_to` (§12 row 18,
  via `docs/planning/v0.1/06-security.md` §14 row 18, which carries the reason); the six
  NATS/MQTT optional-capability items from task E9/A6 (§11 row 10, via `05-interfaces.md`
  §17/§20, which carries the reason). All three already appear in
  `docs/planning/STATUS.md`'s "Open UNVERIFIED items" list — not restated as new here.
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
      `docs/planning/gates/README.md`).
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
  traceability tables (all ten acceptance criteria, all twenty threat-table rows — every
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
- `docs/planning/v0.1/05-interfaces.md`
- `docs/planning/v0.1/06-security.md`
- `docs/planning/v0.1/07-repository-and-dependencies.md`
- `docs/planning/v0.1/08-cli-and-deployment.md`
- `docs/planning/v0.1/10-stages.md` (task A11, not yet landed)
- `docs/planning/v0.1/11-risks.md` (task A12, not yet landed)
- `docs/planning/v0.1/12-deferred.md` (task A12, not yet landed)
- `docs/planning/decisions/C5-envelope-auth.md`
- `docs/planning/decisions/C7-zenoh-transport.md`
- `docs/planning/gates/README.md`
- `docs/planning/PINS.md`
- `docs/planning/STATUS.md`
- `docs/planning/backlog/03-tasks-CD.json` (task D6)
- `docs/planning/backlog/04-tasks-EF.json` (tasks E8, F8-F12)
- `docs/planning/backlog/05-tasks-GHIJ.json` (tasks H1-H5)
