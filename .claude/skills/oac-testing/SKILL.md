---
name: oac-testing
description: Test tier taxonomy, CI-default vs. opt-in rule, fixture-adding procedure, the no-polling assertion, and the acceptance-criteria/threat-mitigation test mapping. Load for any type:test work item (Epic F test tasks, Epic H).
---

Loaded for any `type:test` work item. Scoped to the work items that name this skill in their
own Skills line: E8 (Stage 2 conformance fixtures), F8-F12 (Stage 3 fakes and contract/
security/CI suites), and H1-H5 (Stage 5 end-to-end/security/resilience/smoke suites). It
holds the test taxonomy and the rules that govern any test; it does not hold the threat table
itself (`oac-security-work`), the boundary MUST NOTs (`oac-boundaries`), or code conventions
(`oac-implementation`) — load those alongside this one, do not expect them restated here.

## 1. Test tier taxonomy

From DESIGN "Testing" and PLANNING-PROMPT.md §9 item 10.

| Tier | Proves | Runs against | CI-default or opt-in |
|---|---|---|---|
| Unit | Module-level logic (envelope (de)serialization, duplicate-suppression logic, receipt state machine) | In-process code only | CI-default |
| Spec conformance | Wire representation matches the frozen spec's conformance fixtures (E8) | The conformance fixture set (location per Stage 3 output — see §3) | CI-default |
| Contract — adapter | `ProviderAdapter` interface is obeyed identically by every implementation (F10) | Fake Claude/Codex endpoints (Stage 3); later the real adapters, unchanged suite (Stage 4) | CI-default against fakes; against real adapters it is provider integration (opt-in, pinned) |
| Contract — transport | `Transport` interface is obeyed identically by every implementation (F10) | In-memory transport (Stage 3); real Zenoh over loopback (Stage 4) | CI-default — loopback-only Zenoh needs no live provider or API key, so it stays default |
| Security | Spoof, replay, unauthorized-routing mitigations (F11, H2) | Fakes (Stage 3); real transport/adapters (Stage 5, H2) | CI-default against fakes; against real providers it is opt-in, pinned |
| Resilience | Disconnect/reconnect, daemon restart, TTL `expired`, `unreachable`, duplicate suppression across restart (H3) | In-memory transport and fakes (Stage 3-shaped); real transport in Stage 5 | CI-default against in-memory transport/fakes; opt-in where it needs the real transport |
| Fake-harness integration | Core + adapter + transport wired together with no live provider (Stage 3 exit condition) | Fake Claude endpoint, fake Codex endpoint, in-memory or loopback-Zenoh transport | CI-default |
| Provider integration | Real adapter behaviour against a real, pinned harness version (gates G1/G2; Stage 4) | Real Claude Code / real Codex on pinned versions | Opt-in only, explicit flag, pinned versions |
| End-to-end | The ADR-001 validation criterion for real: Claude -> OAC -> Zenoh -> Codex -> Zenoh -> OAC -> Claude (H1) | Real providers, real transport | Opt-in (needs live providers), but is the go/no-go test — see §5 |
| Cross-platform CLI smoke | One-command startup, `status`/`sessions`/`doctor`, clean shutdown (H4) | The built CLI binary, no live provider required | CI-default, matrixed on Windows/macOS/Linux |

## 2. The CI-default vs. opt-in rule

PLANNING-PROMPT.md §6 and §9.10, sharply:

- The **default** suite (everything the taxonomy above marks CI-default) runs with **no live
  provider, no API key, and no network beyond loopback.** A Zenoh peer talking to another
  Zenoh peer on `127.0.0.1` is loopback and stays default; a call that would touch Claude
  Code, Codex, or any hosted API does not.
- **Provider integration tests are isolated behind an explicit opt-in and pinned versions**
  (F12 acceptance: "Provider integration tests exist but are opt-in and pinned"). Opt-in means
  a separate flag/target a default `test` run never triggers, not a slower default tier.
- Anything that needs a real harness process — Claude Code, Codex, or the end-to-end run — is
  opt-in **by construction**, because it cannot satisfy the no-live-provider default rule, not
  because someone remembered to mark it slow.
- A test that seems to need a live provider "just this once" is a sign the fake/fixture is
  incomplete, not a reason to add a default-tier exception. Fix the fixture (§3); do not
  weaken the default tier.

## 3. Adding a fixture

Fixtures are recorded from the real harnesses **during Stage 1 spikes** (`oac-gates`), then
the fake Claude/Codex endpoints are built from them (F8, F9).

What a fixture file must carry and how it is captured is `oac-gates`'s "Fixture capture
procedure" — load that skill for the pin/date, literal-wire-traffic, redaction, and
Codex-schema-reference rules; not restated here.

What this skill adds, on the test-writing side:

- Never hand-write a fixture to make a test pass. A hand-written fixture reintroduces exactly
  the "probably" `oac-gates` forbids for gates, and here it would make a fake lie about what
  the real harness does.
- If a fixture is missing for behaviour a test needs, that is a Stage 1 gap to report
  (`oac-gates`), not something to synthesize by hand.

Location relative to `tests/`: DESIGN "Suggested repository shape" **suggests**
`tests/protocol/`, `tests/security/`, and `tests/integration/` as top-level suite directories
— DESIGN itself labels that shape "Suggested" and warns against treating any part of the
sketch as fixed prematurely. The actual layout, including any fixture subpath, is confirmed
by Stage 3 output (F8/F9); check `docs/planning/v0.1/` Stage 3 material or the task itself
before adding a fixture, and do not treat the sketch as settled ahead of that.

## 4. The no-polling assertion

DESIGN's normative concept: a harness advertising active inbound support accepts an
authorized message as input to the live session **without application-level polling** of an
inbox. F10 acceptance requires the contract suite to assert this, not assume it.

Shape of the assertion (no API invented — this is the observable behaviour, not a method
name). The assertion is defined by **operation class**, not by whether call counts grow over
time — a legitimate streaming adapter's blocking `recv`/read-next-frame loop and its keepalive
pings are also adapter-initiated calls whose counts grow with time, so a growth-based test
would fail a correct adapter:

- Subscriptions/attachments are established once at attach — one per session or per channel
  is a legitimate design — and are **not re-established or re-issued per message or per
  timer tick**. Assert this by counting attach/subscribe-establishment calls, which must stay
  flat after the initial attach, regardless of how many messages arrive.
- Separately, count only calls to operations that **request pending messages and return
  message payloads on demand** (a list-inbox, fetch-next, or receive-by-request style call
  issued by the adapter on its own initiative to ask "is there anything new"). This count
  must stay at zero between message arrivals for an adapter claiming active inbound.
- Explicitly **excluded** from that second count, because they are not the polling this rule
  targets: a blocking wait/read on an already-established stream or subscription (it does not
  "ask" anything; it waits for the transport to hand it data), and keepalive/ping operations
  that return no message payload. Provider-supported streaming connections, subscriptions,
  event loops, and keepalives are acceptable (DESIGN "Delivery semantics") and must not trip
  this assertion.
- A message becomes visible to the adapter only via the established subscription/stream/
  callback firing — never as the return value of a message-payload-fetching call the adapter
  made on its own initiative.
- Assert this against both fakes (Stage 3, CI-default) and, once real adapters exist, against
  the real adapter's instrumented transport call surface (Stage 4/5).

## 5. Acceptance-criteria and threat-mitigation mapping

H5 requires a traceability table, not a checked box on faith. Two tables, both mandatory
before Stage 5 exit:

**DESIGN v0.1 acceptance criteria (1-10) -> test:**

| # | Criterion (short) | Named test | Tier | Status |
|---|---|---|---|---|
| 1 | One-command local startup | e.g. "smoke / fresh-install one-command start" | CLI smoke | pass/fail/not-yet-written |
| 2 | Distinct neutral sessions per adapter | ... | contract/unit | ... |
| ... | ... (all 10, verbatim from DESIGN "v0.1 acceptance criteria") | ... | ... | ... |
| 9 | Zenoh types confined to its transport module | the `oac-boundaries` CI lint, not a runtime test | lint (CI-default) | ... |
| 10 | Transport contract documented enough for a second backend | evidenced by E9's design-for-replacement proof, not a test | doc proof | ... |

**Threat-model mitigation -> test:**

| Threat / mitigation (verbatim row from the `oac-security-work` threat table) | Proving test | Tier | Status |
|---|---|---|---|
| ... | ... | security | ... |

Rules: every one of the ten criteria gets a named test or, for 9 and 10, the named lint/proof
that actually evidences it (H5 acceptance: "Criterion 9 ... proven by the CI lint, not by
assertion"; "Criterion 10 ... evidenced by E9"). Every threat-table mitigation row names the
test that proves it, and that test currently passes. An unproven criterion or mitigation is
listed as an explicit v0.1 gap in this table, never silently marked done (H5 acceptance,
final bullet). Do not restate the threat table itself here — pull rows from
`oac-security-work`.

## 6. The end-to-end test's definition of done

ADR-001 Validation criterion is the literal spec for the one required end-to-end test (H1):

> Architecture is proven when an existing Claude Code session sends through Session
> Channels/Zenoh to an existing Codex harness session without receiver polling; Codex
> responds and Claude receives the response actively; neither side invokes or holds
> credentials for the other's model API; and sender identity/authorization are enforceable
> rather than inferred from content.

Read "existing session" through the ADR-001-A2 qualification (PLANNING-PROMPT.md §4):
neither harness supports attaching to an arbitrary, already-running session process, so
"existing" means a live interactive session **launched OAC-enabled** — Claude Code started
with `--channels`, Codex attached via the shared local app-server daemon or, on the documented
fallback, `codex --remote`. H1's own acceptance wording is "a live Claude Code session,
launched OAC-enabled" — a test that spins up an arbitrary pre-existing process and expects
attach without an OAC-enabled launch is not testing what H1 requires, and cannot pass.

A test claiming to satisfy H1 must assert every clause above individually (H1 acceptance
mirrors this: no receiver polling, active reply received, no cross-model-API credential use
asserted by the test itself not by inspection, identity/authorization enforced not inferred),
and it must **run on Windows, macOS, and Linux** on the pinned provider versions (H1
acceptance, H4). A single-platform pass is not done.

## 7. Exit criteria for a test work item

- [ ] The test's tier is identified against §1 and placed per the confirmed Stage 3 test
      layout (DESIGN's "Suggested repository shape" sketch, not yet fixed — see §3); no ad
      hoc test location invented ahead of that confirmation.
- [ ] If CI-default per §1, it runs with no live provider, no API key, and no network beyond
      loopback; if it needs a real harness, it is behind an explicit opt-in flag and pinned
      to an exact version (§2).
- [ ] Any new fixture it depends on satisfies §3 — recorded from a real harness during a
      Stage 1 spike, never hand-written, no secrets, version and date carried.
- [ ] If the test concerns an adapter claiming active inbound, it asserts the no-polling
      shape in §4, not just that a message eventually arrived.
- [ ] If the test is meant to satisfy a DESIGN acceptance criterion or a threat-table
      mitigation, its name is added to the corresponding row in §5's tables.
- [ ] If the test is the H1 end-to-end test, it checks every clause of the ADR-001 Validation
      criterion individually and runs on all three platforms (§6).
- [ ] The task's own Acceptance checklist (`docs/planning/backlog/04-tasks-EF.json` or
      `05-tasks-GHIJ.json`) is satisfied.

## Where the content lives

- `docs/planning/DESIGN.md` — "Testing", "v0.1 acceptance criteria", "Delivery semantics"
  (polling vs. streaming/keepalive), "Suggested repository shape".
- `docs/planning/PLANNING-PROMPT.md` §6 (fake-harness/opt-in design-for-replacement proof),
  §8 Stages 3 and 5, §9 item 10 (`09-test-strategy.md`), §11 item 2 (criteria/mitigation
  mapping obligation).
- `docs/planning/ADR-001.md` — Validation criterion.
- `docs/planning/backlog/01-epics.json` — Epics E, F, H.
- `docs/planning/backlog/04-tasks-EF.json` — E8, F8-F12.
- `docs/planning/backlog/05-tasks-GHIJ.json` — H1-H5.
- `docs/planning/STATUS.md` — current stage, gate verdicts, pins the opt-in provider tests
  must match.
- `oac-gates` — fixture capture procedure during Stage 1 spikes, timebox/verdict rules.
- `oac-security-work` — the threat table itself (not restated here).
- `oac-boundaries` — the Zenoh-containment lint referenced by criterion 9.
- `oac-implementation` — workspace layout, contract-tests-first, dependency direction.
