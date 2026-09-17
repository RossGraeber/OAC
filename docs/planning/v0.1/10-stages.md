# 10 — Stages: the stage-gate pipeline

**Issue:** #31 (Epic A, backlog key A11). **Depends on:** #25. **Source:**
`docs/planning/PLANNING-PROMPT.md` §8 (the pipeline) and §9 item 11 (this file's
required content).

**Scope.** This file owns the seven stages (0-6): for each one, the entry criteria, the
modules created or changed by name and responsibility, the prerequisite decisions, the
executable demonstration, the exit artifacts, the gate, the acceptance criteria, and the
go/no-go condition. It also owns the spike timebox values and the expiry rule (§3), the
risk-first ordering rule (§2), and the mapping from each `docs/planning/DESIGN.md` v0.1
acceptance criterion to the stage that proves it (§11).

This file does **not** own:

- Gate pass/fail/fallback criteria or verdicts — `docs/planning/v0.1/02-gating-findings.md`
  (generated from `docs/planning/gates/G1-result.md` … `G5-result.md`), cited here, never
  restated.
- Module layout, ownership boundaries, dependency direction, or the dependency inventory —
  `docs/planning/v0.1/07-repository-and-dependencies.md` §1-§3, §5.
- Decision content — `docs/planning/v0.1/03-decisions-and-amendments.md` §1 decisions 1-12
  and the `docs/planning/decisions/C1`…`C7` documents behind them.
- Test tiers, CI-default-versus-opt-in mechanics, or the traceability tables —
  `docs/planning/v0.1/09-test-strategy.md` §2-§4, §11-§12.
- **Ticket decomposition.** Per `docs/planning/PLANNING-PROMPT.md` §8: "Do not decompose
  stages into tickets; that is the build phase's job." The tickets already exist as the
  backlog: `docs/planning/backlog/01-epics.json` (one epic per stage) and
  `02-tasks-AB.json`, `03-tasks-CD.json`, `04-tasks-EF.json`, `05-tasks-GHIJ.json`. This
  file names task keys **only as citations** — to say which existing backlog item owns an
  artifact — and creates none. See §12.

**Naming.** Product and repository: **Open Agent Channel (OAC)**. Normative protocol
specification: **OAC Session Channels**. CLI binary: **`oac`**. Per ADR-001-A1
(`docs/planning/v0.1/03-decisions-and-amendments.md` §2), this file never writes bare
"Session Channels" or `sessionchannels`.

**Status caveat, stated once.** Per `docs/planning/STATUS.md` "Current stage", the project
is at **Pre-Stage 0**; Stages 1-6 are blocked. Every gate verdict in this file is `NOT RUN`,
and no stage below is recorded as entered, passed, or exited. The criteria are the plan,
not a report.

---

## 1. Pipeline overview

One row per stage. "Epic" and "Milestone" are the existing backlog containers
(`docs/planning/backlog/01-epics.json`), which map roughly one epic per stage — Epic A
(this planning package) and Epic C (the §5 decisions) sit in milestone M0 and precede
Stage 0's own epic rather than being a stage of their own.

| Stage | Name | Epic | Milestone | Gate at the end | Gate has a fallback? |
|---|---|---|---|---|---|
| 0 | Evidence and pinning | B | M1 | **Gate S0** — pins confirmed, §3 baseline re-verified, conflict register closed | N/A (internal gate) |
| 1 | Provider and transport spikes | D | M2 | **G1-G5** (`02-gating-findings.md` §3-§7), published by D7 | G1 no; G2 yes; G3 yes; G4 yes; G5 no |
| 2 | Normative spec v0.1 | E | M3 | **Gate S2** — interface freeze (E7) plus conformance fixture set (E8) | N/A (internal gate) |
| 3 | Core and fakes | F | M4 | **Gate S3** — CI default tier green with no live provider, no API key, no network beyond loopback (F12) | N/A (internal gate) |
| 4 | Adapters and Zenoh transport | G | M5 | **Gate S4** — contract suites pass unchanged on real modules; provider integration passes on pinned versions behind opt-in | N/A (internal gate) |
| 5 | End-to-end and threat verification | H | M6 | **Gate S5** — every DESIGN acceptance criterion 1-10 and every `06-security.md` §14 mitigation maps to a passing test or is declared a gap (H5) | N/A (internal gate) |
| 6 | Release hygiene | I | M7 | **Gate S6** — release checklist complete, gates re-run against shipped pins | N/A (internal gate) |

**Gate naming.** `G1`-`G5` are the five provider/transport gates defined in
`docs/planning/PLANNING-PROMPT.md` §4 and recorded in `docs/planning/v0.1/02-gating-findings.md`.
`S0`-`S6` are this file's stage-exit gates: a stage-exit gate is a checklist over that
stage's own exit artifacts, not a spike, and it has no `PASS (FALLBACK TAKEN)` value —
it is met or the stage does not exit.

---

## 2. Risk-first ordering rule

Stated normatively, because it is the ordering constraint
`docs/planning/PLANNING-PROMPT.md` §8 imposes on the whole pipeline:

**No substantial core or transport code is written before Stage 0 and Stage 1 complete.**
"Substantial core or transport code" means anything under `core/`, `transports/zenoh/`,
`adapters/claude/`, or `adapters/codex/` as those modules are defined in
`docs/planning/v0.1/07-repository-and-dependencies.md` §2. Spike code written during
Stage 1 is exempt because it is throwaway and quarantined (§5's Stage 1 exit artifacts,
and `.claude/skills/oac-gates/SKILL.md` "Throwaway rule": "No spike becomes Stage 3/4
code by default").

Three consequences, each a decision this file makes rather than leaves open:

1. **Spec text precedes implementation, but follows the spikes.** Stage 2 writes the
   normative spec only after G1-G5 return verdicts, so the spec is written against
   observed provider behaviour rather than documented intent. A failed gate changes the
   spec's scope before a line of it is frozen.
2. **Fakes precede real adapters.** Stage 3 builds fake Claude and fake Codex endpoints
   from the Stage 1 fixtures (`docs/planning/v0.1/09-test-strategy.md` §13) and gets CI
   green with no provider, before Stage 4 writes a single real adapter. The contract
   suites therefore exist and pass before the thing they constrain is built.
3. **Contracts freeze before the code that implements them.** Stage 2's E7 freeze is what
   Stage 3 and Stage 4 build against; a change to a frozen contract after Stage 2 is a
   spec-revision event under the versioning policy
   (`docs/planning/v0.1/05-interfaces.md` §11), not a refactor.

**What a gate failure does to the order.** A `FAIL` on G1 or G5 does not reorder the
pipeline — it stops it (§5's go/no-go conditions). A `FAIL` on G2, G3, or G4 moves the
named fallback into the design before Stage 2 freezes the interfaces, which is precisely
why the spikes sit before the spec rather than beside it.

---

## 3. Timebox policy and expiry

Only Stage 1 contains timeboxed work; Stages 0 and 2-6 are artifact-complete, not
time-bounded. The timebox **policy** — how an expiry is recorded — is
`.claude/skills/oac-gates/SKILL.md` "Timebox policy", cited here and not restated. The
timebox **values** are set here, because `docs/planning/PLANNING-PROMPT.md` §8 requires
the plan to state them and `.claude/skills/oac-gates/SKILL.md` requires the box to be set
before the spike starts.

| Spike | Backlog task | Timebox | Why this size |
|---|---|---|---|
| G1 Claude wake | D1 | 3 working days | Five pass criteria on a research-preview surface, including an interactive confirmation dialog that cannot be scripted around (`02-gating-findings.md` §3) |
| G2 Codex live inject | D2 | 4 working days | Two paths (implicit daemon attach, then the `codex --remote` fallback) must both be attempted before a `FAIL`; the task is additionally labelled `blocked-external` (`docs/planning/backlog/03-tasks-CD.json` D2) |
| G3 Zenoh local peer | D3 | 3 working days | Two discovery modes across three operating systems, plus a localhost TLS listener in both modes (`02-gating-findings.md` §5) |
| G4 MCP dual-era server | D4 | 3 working days | Both eras run concurrently, plus an explicit negative case (`02-gating-findings.md` §6) |
| G5 Provenance | D5 | 2 working days | Two providers, four criteria, no build of durable machinery required (`02-gating-findings.md` §7) |
| Fixture capture | D6 | 2 working days, run alongside D1-D5 | Fixtures are captured **during** the spikes, from the real harness, never reconstructed afterwards (`.claude/skills/oac-gates/SKILL.md` "Fixture capture procedure") |

**These are OAC's own scheduling parameters, not claims about an external system.**
Reversal condition: if a spike's first day shows the box is mis-sized, the box is changed
**before that spike starts** on a subsequent run, never mid-spike.

**What happens when a timebox expires.** Stated here in full because
`docs/planning/PLANNING-PROMPT.md` §8 requires this file to say it:

1. The spike **stops** at the box, regardless of how close it looks to succeeding.
2. The finding is recorded **as-is** in `docs/planning/gates/G<n>-result.md`: whichever
   pass criteria were actually confirmed or refuted at that moment, and no others.
3. The verdict written is the honest one for what was confirmed — `FAIL` if a go/no-go
   criterion is unmet on every path attempted, `PASS (FALLBACK TAKEN)` if only the
   fallback path was confirmed inside the box, `NOT RUN` if the spike could not be
   executed at all (blocked on an external dependency or a missing pin). **Never a plain
   `PASS` on an incomplete run.**
4. An expired box is a result. Extending it, or writing a passing verdict because the
   remaining criteria "should" hold, is the "probably" `docs/planning/PLANNING-PROMPT.md`
   §4 forbids: "The build phase may not proceed past a gate on a 'probably.'"
5. The expiry itself is recorded in the gate result's `**Timebox:**` field
   (`docs/planning/gates/README.md` gate-result template), so a later reader can tell an
   expired-box `FAIL` from a criterion-refuted `FAIL`.

**Re-running a spike after expiry is allowed; silently continuing one is not.** A re-run
opens a new box, records a new dated result, and appends the superseded verdict to that
gate file's `Re-run history` table (`docs/planning/gates/README.md` re-run policy).

---

## 4. Stage 0 — Evidence and pinning

**Epic B, milestone M1.** `docs/planning/backlog/02-tasks-AB.json` tasks B1-B4.

**Entry criteria.**

- `docs/planning/ADR-001.md` and `docs/planning/DESIGN.md` exist and are the working
  baseline.
- `docs/planning/PLANNING-PROMPT.md` §3 exists as the pre-verified evidence baseline.

**Prerequisite decisions (`docs/planning/v0.1/03-decisions-and-amendments.md` §1).**
None. Stage 0 is what the decisions are made **against** — decisions 1-12 all cite pins
and re-verified facts this stage produces. This is the one stage with no decision
prerequisite, and that is why it is first.

**Modules created or changed.** None. No code module in
`docs/planning/v0.1/07-repository-and-dependencies.md` §2 is touched; §2's rule that no
substantial core or transport code starts before Stage 0 and Stage 1 complete is in force
from here. The artifacts are documents.

**Executable demonstration.** `docs/planning/PINS.md`'s every pinned version string is
resolvable and fetchable at the pin — each surface's pinned artifact (release tag,
commit, crate version, protocol revision) is retrieved at its cited URL on the recorded
date, and the retrieval is what the re-verification record
(`docs/planning/REVERIFICATION-B2.md`) reports. The demonstration is a retrieval, not a
build: no workspace exists yet.

**Exit artifacts.**

- `docs/planning/PINS.md` — every external surface pinned with version, date, source,
  and the gates that pin affects (B1).
- `docs/planning/REVERIFICATION-B2.md` — the §3 baseline re-verified fact-by-fact against
  those pins, with drift recorded (B2).
- `docs/planning/ADR-001-AMENDMENTS.md` — amendments A1-A3 and the conflict register
  C1-C10 resolved (B3).
- `docs/planning/gates/README.md` plus the five `G<n>-result.md` stubs — the gate
  evidence store and the re-run/invalidation policy (B4).
- `docs/planning/STATUS.md` "Open UNVERIFIED items" — the ledger of record, populated.

**Gate S0 — acceptance criteria.**

1. Every surface named in `docs/planning/PLANNING-PROMPT.md` §3 has a pin row with a
   version, a date, a first-party source, and a `Gates affected` cell.
2. Every §3 fact is re-verified against its pin, and each outcome is one of: holds,
   drifted (old and new value recorded), or `UNVERIFIED` with a reason.
3. Every remaining `UNVERIFIED` item appears in `docs/planning/STATUS.md`'s ledger — none
   exists in only one document (`oac-evidence` §5).
4. Every conflict-register entry is either resolved by a numbered amendment, resolved in
   a decision document, or recorded as explicitly open with a named owner.
5. The gate re-run/invalidation policy exists and is mechanical: a pin-table change
   invalidates the gates named in that row.

**Go/no-go condition.** Go when all five criteria hold. **No-go** if any pinned surface
cannot be resolved at all at its pin — an unresolvable pin means later stages would be
built on an unfixable version, and the correct response is to re-pin to a resolvable
version and re-run criterion 2, not to proceed on the unresolvable one.

**Current verdict.** Not entered. `docs/planning/STATUS.md` records B1-B4 as landed
artifacts within milestone M0 work, with Stage 1 onward still blocked; several
`UNVERIFIED` items in that ledger remain open (criterion 3 is satisfied by their being
**listed**, not by their being closed).

---

## 5. Stage 1 — Provider and transport spikes

**Epic D, milestone M2.** Tasks D1-D7 (`docs/planning/backlog/03-tasks-CD.json`).

**Entry criteria.**

- Gate S0 met (§4).
- The pinned harnesses are installable on the spike machine at their exact pins
  (`docs/planning/PINS.md`).
- Each spike's timebox is set before it starts (§3).

**Prerequisite decisions.** None of decisions 1-12 is a hard prerequisite — the spikes
are deliberately allowed to run ahead of the design so they can invalidate it. Two
decisions are **informative** inputs only: decision 1 (language and runtime) supplies the
`rmcp` and Zenoh crates the G1/G3/G4 spikes exercise, and decision 3 (spec packaging)
supplies the MCP extension identifier G4 checks for `_meta` collisions
(`docs/planning/v0.1/03-decisions-and-amendments.md` §1, decisions 1 and 3). A gate result
that contradicts either is an amendment event — recorded in
`docs/planning/v0.1/03-decisions-and-amendments.md` §2 under the numbered-amendment
procedure `docs/planning/PLANNING-PROMPT.md` §1 fixes — not a reason to adjust the spike.

**Modules created or changed.** **None durable.** Spike code lives outside the
`docs/planning/v0.1/07-repository-and-dependencies.md` §1 layout and is discarded or
quarantined at D7. Nothing in `core/`, `adapters/*`, `transports/zenoh/`, or `cli/`
is created at this stage.

**Executable demonstration.** Five, one per gate, each run against the pinned harness on
the spike machine:

- **G1** — a throwaway development-flag channel server wakes an idle Claude Code session
  with a `notifications/claude/channel`, the message renders as a `<channel>` tag, a
  mid-turn second notification is queued and delivered in order, and Claude replies
  through an ordinary MCP tool, with the
  `--dangerously-load-development-channels` confirmation dialog actually exercised.
- **G2** — a second client of the Codex app-server delivers a message into a normally
  launched TUI, the user sees it and the model answers, with OAC holding no OpenAI
  credentials at any point.
- **G3** — two Zenoh peers discover each other over loopback on Windows 11, macOS, and
  Linux, in both the multicast and multicast-disabled modes, with a TLS listener bound to
  `127.0.0.1` in each.
- **G4** — one process serves the legacy MCP era as a channel and the current era as a
  tool surface **concurrently**, and a server negotiating the current revision is
  confirmed rejected as a channel.
- **G5** — a message whose body lies about its sender is shown to the model on both
  providers with the machine-set provenance visibly contradicting the body's claim.

Pass, fail, and fallback text for each is `docs/planning/v0.1/02-gating-findings.md`
§3-§7 — not restated here.

**Exit artifacts.**

- `docs/planning/gates/G1-result.md` … `G5-result.md`, each with verdict, date, pins
  relied on, timebox and whether it expired, transcript summary, and fixtures captured
  (D1-D5).
- The recorded protocol fixture set, captured from the real harnesses during the spikes
  (D6), covering the exchanges `docs/planning/v0.1/09-test-strategy.md` §13 names as the
  input to Stage 3's fakes.
- `docs/planning/v0.1/02-gating-findings.md`, regenerated from the five result files.
- The published stage-1 exit decision, with all spike code discarded or clearly
  quarantined (D7).

**Gate S1 = G1-G5 — acceptance criteria.**

1. Each of G1-G5 carries a closed verdict: `PASS`, `PASS (FALLBACK TAKEN)`, or `FAIL`.
   No gate exits Stage 1 at `NOT RUN`.
2. Every pass criterion in each gate's reference file is evaluated individually — a gate
   does not pass on a majority (`.claude/skills/oac-gates/SKILL.md`).
3. Where a fallback was taken, the gate result names which path actually passed and
   records the fallback's cost.
4. Fixtures exist for every exchange Stage 3's fakes need, captured live, credential-free.
5. No spike code remains on a path a later stage will build in.

**Go/no-go condition.** This is the pipeline's real go/no-go, per
`docs/planning/PLANNING-PROMPT.md` §4:

- **G1 `FAIL` → project no-go for the Claude adapter, with no fallback.** The Claude
  adapter is blocked entirely and ADR-001's validation criterion cannot be met as written.
- **G2 `FAIL` on both the implicit-attach path and the `codex --remote` fallback →
  v0.1 go/no-go.** A single-path failure is not a gate failure; it selects the fallback.
- **G5 `FAIL` on a provider → that provider's provenance rendering requires a design
  change before Stage 2 freezes the interfaces**, not a workaround. It invalidates
  `docs/planning/DESIGN.md` v0.1 acceptance criterion 6.
- **G3 or G4 `FAIL` → take the named fallback and proceed**, recording
  `PASS (FALLBACK TAKEN)`. Neither stops v0.1 on its own.

**Current verdict.** All five gates `NOT RUN` (`docs/planning/STATUS.md` "Gate verdicts";
`docs/planning/v0.1/02-gating-findings.md` §2). Stage 1 is not entered.

---

## 6. Stage 2 — Normative spec v0.1

**Epic E, milestone M3.** Tasks E1-E9 (`docs/planning/backlog/04-tasks-EF.json`).

**Entry criteria.**

- Gate S1 met (§5): every gate carries a closed verdict, and any fallback taken is
  recorded, because the spec is written against what the spikes observed.
- Any G5 design change identified at Stage 1 is folded into the design **before** the
  interface freeze, not after.

**Prerequisite decisions (`docs/planning/v0.1/03-decisions-and-amendments.md` §1).**
Decision 3 (spec packaging and the MCP extension identifier
`io.github.rossgraeber/oac-session-channels`), decision 4 (session identity, addressing,
discovery), decision 5 (envelope authenticity and replay), decision 6 (pairing and
authorization), decision 8 (provider-facing trust rendering), decision 9 (outbound
symmetry). Decision 3's dual-era sub-element is gate-decided at G4, so Stage 2 cannot
start before G4 returns a verdict.

**Modules created or changed** (names and responsibilities from
`docs/planning/v0.1/07-repository-and-dependencies.md` §2):

- **`spec/` — created.** Owns the normative OAC Session Channels specification text and
  the security specification. Never owns implementation code or provider/transport
  vocabulary.
- **`tests/protocol/` — created.** Owns the conformance fixture set (E8). Never owns
  production code.
- **`core/` — contract surface defined, not implemented.** E7 freezes the
  `ProviderAdapter` contract, the `Transport` contract, and the neutral core types
  (`SessionIdentity`, `SessionDescriptor`, `SessionCapabilities`, `ChannelMessage`,
  `DeliveryReceipt`, `PresenceRecord`, `SecurityPrincipal`) as normative text. No `core/`
  code is written at this stage — §2's risk-first rule still applies, and Stage 3 is where
  the implementation starts.

**Executable demonstration.** The conformance fixture set (E8) is executed against a
minimal reference validator: every row of the closed error taxonomy
(`docs/planning/v0.1/05-interfaces.md` §10, ten rows), every state of the frozen
delivery-state set (§9, eight states), and the `ttl_ms`-before-replay-window precedence
rule each have at least one fixture that a validator reads and decides, with the decision
matching the fixture's recorded expectation. A fixture set that no program can execute is
a document, not a demonstration.

**Exit artifacts.**

- `spec/` — the OAC Session Channels specification (E1-E4, E6) and the security
  specification (E5).
- The frozen adapter contract, transport contract, and core types (E7).
- The conformance fixture set under `tests/protocol/` (E8).
- The design-for-replacement proofs (E9), which
  `docs/planning/v0.1/05-interfaces.md` §22 currently records as **NOT MET as of the M0
  draft** — every NATS and MQTT capability cell in §17's proof is `UNVERIFIED` (carried
  from `docs/planning/STATUS.md`, not resolved here).

**Gate S2 — acceptance criteria.**

1. The adapter contract, the transport contract, and the core types are frozen: a change
   after this point is a spec-revision event under
   `docs/planning/v0.1/05-interfaces.md` §11, recorded as such.
2. No neutral spec text mentions Zenoh, a key expression, an MQTT topic, a NATS subject,
   or a provider-specific method name — the `oac-boundaries` grep check passes over
   `spec/`. This is `docs/planning/ADR-001.md`'s "MUST NOT leak Zenoh-specific concepts
   into the neutral protocol" boundary, enforced mechanically.
3. Normative text (MUST/SHOULD/MAY) is separated from reference-implementation notes
   throughout, so a non-Rust implementation can tell which parts bind it.
4. Every conformance fixture is executable and carries a recorded expected outcome.
5. The versioning policy and the unsupported-capability behaviour are both stated
   normatively.
6. Each gate result from Stage 1 that contradicted a decision has produced a numbered
   amendment before the freeze, not after.

**Go/no-go condition.** Go when criteria 1-6 hold. **No-go — the freeze does not happen —
if criterion 2 fails**, because an interface that leaks transport or provider vocabulary
cannot be frozen without freezing the leak; the offending text is corrected and the freeze
is re-attempted. A failing criterion 4 (a fixture nothing can execute) blocks Stage 3,
which loads those fixtures as its CI-default spec-conformance tier.

---

## 7. Stage 3 — Core and fakes

**Epic F, milestone M4.** Tasks F1-F12 (`docs/planning/backlog/04-tasks-EF.json`).

**Entry criteria.**

- Gate S2 met (§6): contracts frozen, fixtures executable.
- Stage 1 fixtures exist (D6) — the fakes are built from recorded traffic, not from the
  spec.
- Gate S0 and Gate S1 both met, which is when §2's "no substantial core or transport code"
  rule releases.

**Prerequisite decisions (`docs/planning/v0.1/03-decisions-and-amendments.md` §1).**
Decision 1 (language and runtime: Rust, single self-contained binary), decision 2 (process
model: one long-lived per-device `oac` daemon plus thin `oac mcp-shim` stdio children,
local IPC with OS-level peer authentication), decision 5 (envelope authenticity and
replay: Ed25519 over RFC 8785 JCS canonicalization, ±300s accept-window, 128-bit nonce
deduplicated on `(key_id, nonce)`), decision 6 (pairing and authorization: default-deny,
per-session-id, `working_directory`-scoped allowlists), decision 7 (key storage:
`keyring` with an `age` encrypted-file fallback), decision 12 (dependencies and licenses).

**Modules created or changed** (`docs/planning/v0.1/07-repository-and-dependencies.md`
§1-§2, §5):

- **`core/` — created and implemented.** Owns the neutral types and the policy and
  authorization decisions. Depends on nothing in-repo. This is where F2 (envelope
  serialization), F4 (replay defence and duplicate suppression), F5 (authorization engine
  and pairing store), and F6 (presence registry and delivery-receipt state machine) land.
- **The daemon binary's own identity code — created.** Device identity, key storage,
  signing, and verification (F3). Per
  `docs/planning/v0.1/07-repository-and-dependencies.md` §1, the daemon is a binary, not a
  module: this code lives with the binary's entry point, not in `core/`, and it is what
  consumes `keyring`, `keyring-core`, `windows-native-keyring-store`, and `age`.
- **The in-memory transport — created** (F7), implementing the frozen `Transport`
  contract's seven operations so the contract suites have something to run against before
  `transports/zenoh/` exists.
- **`tests/` — created and populated.** `tests/protocol/` gains the contract suites (F10),
  `tests/security/` gains the security suite against the fakes (F11), and the fake Claude
  (F8) and fake Codex (F9) endpoints are built from the Stage 1 fixtures. `tests/` never
  owns production code.
- **The workspace scaffold — created** (F1), including the module-dependency-direction
  lint that enforces
  `docs/planning/v0.1/07-repository-and-dependencies.md` §3's allowed edges and the
  Zenoh-containment lint §4(a) requires.
- **`transports/zenoh/`, `adapters/claude/`, `adapters/codex/` — not created.** They are
  Stage 4's work.

**Executable demonstration.** The default test suite runs to green **with no live
provider, no API key, and no network beyond loopback** (F12): a message is composed,
signed, published over the in-memory transport, authorized by `core/`, delivered to the
fake Claude endpoint and the fake Codex endpoint, replied to, and correlated — with the
replay, duplicate, and unauthorized-routing cases each rejected as the frozen error
taxonomy specifies. The command a reviewer runs is the project's plain default test
invocation; nothing opt-in is required to reach green.

**Exit artifacts.**

- A CI pipeline whose default tier is green under the §3 CI-default rule of
  `docs/planning/v0.1/09-test-strategy.md` (F12).
- The fake Claude and fake Codex endpoints, loaded from the Stage 1 fixtures (F8, F9).
- The adapter and transport contract suites (F10), including the named
  `contract/adapter/no-polling` test whose exact assertion shape is
  `docs/planning/v0.1/09-test-strategy.md` §5.
- The security suite against the fakes (F11).
- The provider-integration tier isolated as its own separately invoked target, existing
  but not run by default (F12).

**Gate S3 — acceptance criteria.**

1. The default test run is green and touches no live provider, no API key, and no network
   beyond loopback, where loopback is defined as
   `docs/planning/v0.1/09-test-strategy.md` §3 defines it — a local Codex app-server
   socket is a live-provider call, not a loopback exemption.
2. A plain default test invocation never triggers a provider-integration test.
3. The dependency-direction lint passes: `core/` depends on nothing in-repo, no adapter
   depends on another adapter, and no adapter depends on `transports/zenoh/`.
4. The `contract/adapter/no-polling` assertion passes against both fakes, in the
   call-class shape §5 of the test strategy fixes — not a call-count-growth shape.
5. Every fake's behaviour is traceable to a recorded Stage 1 fixture; no fake behaviour is
   invented from the spec.

**Go/no-go condition.** Go when criteria 1-5 hold. **No-go on criterion 1 or 2**: a
default tier that needs a live provider to be green is, per
`docs/planning/v0.1/09-test-strategy.md` §3's anti-exception rule, a signal that a fake or
fixture is incomplete — the response is to report the Stage 1 or Stage 3 gap and fix the
fake, never to grant a default-tier waiver. A failing criterion 5 sends the work back to
Stage 1 for a fixture capture, because a fake built from the spec proves only that the
spec agrees with itself.

---

## 8. Stage 4 — Adapters and Zenoh transport

**Epic G, milestone M5.** Tasks G1-G11 (`docs/planning/backlog/05-tasks-GHIJ.json`).

**Entry criteria.**

- Gate S3 met (§7): contract suites exist and pass against fakes and the in-memory
  transport.
- G1 and G2 carry `PASS` or `PASS (FALLBACK TAKEN)` — tasks G4, G6, and G7 in the backlog
  each carry a `gate:*` label, which per the `oac` router is a blocker: the verdict is
  checked in `docs/planning/STATUS.md` before that work proceeds.
- G3's verdict determines the Zenoh discovery mode this stage implements (multicast
  scouting, or the fixed local rendezvous endpoint).

**Prerequisite decisions (`docs/planning/v0.1/03-decisions-and-amendments.md` §1).**
Decision 8 (provider-facing trust rendering: the fixed five-key Claude `meta` set, the
Codex machine-generated header plus nonce-derived delimiter, permission relay off by
default), decision 9 (outbound symmetry: the four OAC MCP tools `send`, `reply`,
`list_sessions`, `whoami`), decision 10 (transport mapping: key expressions as a one-way
hash of the opaque session id computed only inside `transports/zenoh/`, presence via
liveliness tokens with history-capable subscribers, local mode binding `127.0.0.1` with
no `zenohd`), decision 2 (process model, for the daemon and IPC work in G9), decision 11
(configuration and CLI model, for G10).

**Modules created or changed** (`docs/planning/v0.1/07-repository-and-dependencies.md`
§2, §4):

- **`transports/zenoh/` — created.** Owns every Zenoh-specific type, identifier, and key
  expression, behind the `Transport` contract's seven operations. Depends on `core/` only.
  Tasks G1 (session mapping, pub/sub, containment), G2 (presence via liveliness tokens),
  G3 (local-mode and LAN-mode security configuration).
- **`adapters/claude/` — created.** Owns translating neutral envelopes to and from Claude
  Code's provider-native wake and reply operations, and nothing else: never the transport
  peer, never key material, never a policy decision. Depends on `core/` only. Tasks G4
  (channel server, legacy negotiation, inbound delivery), G5 (outbound tools and session
  registration). This module **is** the compatibility shim boundary for the Claude Code
  Channels research-preview surface — a boundary that is still UNNAMED at the module or
  interface level (UNVERIFIED — `docs/planning/DESIGN.md` names no such module; register
  entry C11, `docs/planning/v0.1/03-decisions-and-amendments.md` §4 and
  `docs/planning/STATUS.md`). Naming it is owned by this stage's adapter tasks.
- **`adapters/codex/` — created.** Owns translating neutral envelopes to and from the
  Codex app-server's thread and turn operations; never OpenAI model-API credentials.
  Depends on `core/` only. Tasks G6 (app-server client and thread/turn mapping), G7 (live
  inbound injection with the documented fallback), G8 (outbound tool surface and reply
  correlation). Same UNVERIFIED shim-boundary-naming carry as the Claude adapter
  (register entry C11).
- **`cli/` (including `mcp-shim`) — created.** Owns the user-facing commands and the thin
  stdio shim a harness spawns; never long-lived process state, never the transport peer,
  never key material. Tasks G9 (daemon, MCP shims, authenticated local IPC), G10 (`start`,
  `status`, `sessions`, `doctor`, zero-file defaults), G11 (the documented per-harness
  launch).
- **`core/`, `spec/` — unchanged.** A contract change here would be a Stage 2 freeze
  violation (§6 criterion 1).

**Executable demonstration.** Two, both required:

1. **The Stage 3 contract suites pass unchanged against the real modules** — the real
   Claude adapter, the real Codex adapter, and real Zenoh over loopback are substituted
   for the fakes and the in-memory transport, and the suites are not edited to accommodate
   them. Real Zenoh over loopback stays CI-default
   (`docs/planning/v0.1/09-test-strategy.md` §2 row 3, §3's loopback rule); the real
   adapters run in the opt-in provider-integration tier.
2. **The documented one-line launch works on each harness**: a Claude Code session
   launched with `claude --dangerously-load-development-channels server:oac`, and Codex
   registered with `codex mcp add oac -- oac mcp-shim`, each reach a running daemon and
   appear in `oac sessions` (`docs/planning/v0.1/08-cli-and-deployment.md`, cited; the
   consent dialog step is preserved, not scripted around).

**Exit artifacts.**

- Real `transports/zenoh/`, `adapters/claude/`, `adapters/codex/`, and `cli/` modules
  passing the frozen contracts.
- Provider integration tests passing against the exact pinned versions, behind an explicit
  opt-in target, each naming its pin string rather than "latest"
  (`docs/planning/v0.1/09-test-strategy.md` §4).
- The per-harness launch documentation and scripts (G11).

**Gate S4 — acceptance criteria.**

1. The contract suites pass on the real modules **without modification** — a suite edited
   to make a real module pass is a contract change, therefore a Stage 2 freeze violation.
2. Provider integration tests pass on the pinned versions, and each names the exact pin
   string it ran against.
3. The Zenoh-containment lint passes: no Zenoh type, `zid`, key expression, or liveliness
   term appears outside `transports/zenoh/`. The lint's recorded scope gap — no
   `snake_case`-embedded `zid`/`zenoh` match, and no coverage of `adapters/` or `cli/` —
   is carried forward as an open v0.1 gap, not silently treated as full coverage
   (`docs/planning/v0.1/09-test-strategy.md` §8).
4. No adapter holds provider credentials, calls a provider model API, or reaches the
   transport peer directly — each routes through `core/` policy.
5. The `contract/adapter/no-polling` test passes against the real adapters' instrumented
   call surface, opt-in and pinned.
6. If G2's fallback path was taken, G7's implementation is the fallback path and says so.

**Go/no-go condition.** Go when criteria 1-6 hold. **No-go on criterion 1** — the correct
response is to fix the module, or to open a spec revision under
`docs/planning/v0.1/05-interfaces.md` §11, never to edit the suite. **No-go on criterion
4**, which is an `docs/planning/ADR-001.md` boundary violation ("MUST NOT call provider
model APIs as a substitute for a native harness"; "MUST NOT steal or reuse another
harness's provider credentials"), not a test failure to be triaged.

---

## 9. Stage 5 — End-to-end and threat verification

**Epic H, milestone M6.** Tasks H1-H5 (`docs/planning/backlog/05-tasks-GHIJ.json`).

**Entry criteria.**

- Gate S4 met (§8).
- Real Claude Code and real Codex, at their pinned versions, are installable on each of
  Windows, macOS, and Linux — the validation criterion is executed on all three.
- `docs/planning/v0.1/06-security.md` §14's twenty-row threat table exists as the checklist
  H2, H3, and H5 verify against.

**Prerequisite decisions.** All twelve (`docs/planning/v0.1/03-decisions-and-amendments.md`
§1). Stage 5 verifies the system the decisions describe; the decisions with the most
directly testable claims here are decision 5 (replay and duplicate suppression), decision
6 (default-deny authorization), decision 8 (provenance rendering), and decision 10
(transport mapping and containment).

**Modules created or changed.**

- **`tests/integration/` and `tests/security/` — extended.** The end-to-end suite (H1),
  the security verification suite (H2), the resilience sub-row (H3, folded into the
  security tier per `docs/planning/v0.1/09-test-strategy.md` §2), and the cross-platform
  CLI smoke suite (H4).
- **Production modules — bug fixes only.** A behaviour change required at this stage is a
  defect in a Stage 3 or Stage 4 module, fixed there and re-verified by the contract
  suites; it is not new scope.

**Executable demonstration.** The `docs/planning/ADR-001.md` validation criterion, as
redefined by ADR-001-A2, executed on Windows, macOS, and Linux: a live interactive Claude
Code session, launched OAC-enabled and owned by a human at a terminal, actively messages a
live Codex session over Zenoh with no receiver polling; Codex actively replies; the reply
is correlated back; provenance and authorization are enforced throughout; and OAC invokes
no provider model API at any point (H1, labelled `go-no-go`). Alongside it: the spoof,
replay, duplicate, unauthorized-routing, cross-project-leakage, and disconnect/restart/
expiry cases (H2, H3), and one-command startup plus `status`, `sessions`, `doctor`, and
clean shutdown on all three platforms (H4).

**Exit artifacts.**

- The end-to-end, security, resilience, and CLI smoke suites, with recorded results per
  platform.
- H5's completed traceability: every `docs/planning/DESIGN.md` v0.1 acceptance criterion
  1-10 mapped to a named passing test, and every `docs/planning/v0.1/06-security.md` §14
  mitigation mapped to the test that proves it.
- An explicit, published **v0.1 gap list** for anything not proven.

**Gate S5 — acceptance criteria.**

1. Every one of `docs/planning/DESIGN.md` acceptance criteria 1-10 maps to a **named**
   test, and that test's current result is recorded. The mapping is
   `docs/planning/v0.1/09-test-strategy.md` §11's table, updated from `not-yet-written` to
   a real result — this file does not duplicate that table, it requires it be completed.
2. Every one of the twenty threat-table mitigations maps to a proving test whose result is
   recorded (`docs/planning/v0.1/09-test-strategy.md` §12).
3. Anything unproven is published as an explicit v0.1 gap rather than quietly marked done
   — H5's own acceptance box states exactly this.
4. The end-to-end criterion passes on all three platforms, or the failing platform is a
   published gap with a named cause, never a blended verdict.
5. Two criteria are proven by means other than a runtime test, and are recorded that way
   rather than as test rows: criterion 9 (Zenoh containment) is proven by the
   `oac-boundaries` CI lint with its scope gap carried
   (`docs/planning/v0.1/09-test-strategy.md` §8), and criterion 10 (transport contract
   documented enough to add a second backend) is proven by the E9 design-for-replacement
   doc proof, which `docs/planning/v0.1/05-interfaces.md` §22 currently records as **NOT
   MET**, with every NATS and MQTT capability cell `UNVERIFIED`.

**Go/no-go condition.** **H1 is the v0.1 go/no-go.** If the end-to-end criterion does not
pass on any platform, v0.1 does not ship: the ADR's validation criterion is the definition
of what v0.1 proves, and a release that cannot demonstrate it has nothing to release.
Criteria 2, 3, and 5 do **not** individually block the release — an unproven mitigation is
allowed to ship as a published gap — but an unproven mitigation that is silently omitted
from the gap list does block it, because that converts an honest gap into a false claim.

---

## 10. Stage 6 — Release hygiene

**Epic I, milestone M7.** Tasks I1-I4 (`docs/planning/backlog/05-tasks-GHIJ.json`).

**Entry criteria.**

- Gate S5 met (§9), including the published gap list.
- A `Cargo.lock` exists — it is the input to the transitive license sweep that
  `docs/planning/v0.1/07-repository-and-dependencies.md` §8 defers to this stage precisely
  because no lockfile existed at M0.

**Prerequisite decisions.** Decision 1 (language, runtime, packaging: single
self-contained binary, dynamically linked against OS system libraries only) and decision
12 (dependencies and licenses), plus decision 11 (configuration and CLI model) for the
user documentation (`docs/planning/v0.1/03-decisions-and-amendments.md` §1).

**Modules created or changed.**

- **`docs/` — populated.** User and contributor documentation (I1).
- **Packaging configuration — created.** Release artifacts for Windows, macOS, and Linux
  (I3).
- **No production module changes.** A code change at Stage 6 re-opens Gate S5, because the
  thing S5 verified is no longer the thing being shipped.

**Executable demonstration.** The published release artifact, downloaded fresh on each of
Windows, macOS, and Linux, starts with one command and passes the cross-platform CLI smoke
suite — the same suite H4 built, run against the packaged binary rather than a build tree.
Alongside it, the transitive license sweep runs to completion over the resolved dependency
graph, including the three Codex crates that must be swept from the vendored git tree
rather than from crates.io (`docs/planning/v0.1/07-repository-and-dependencies.md` §8).

**Exit artifacts.**

- User and contributor documentation (I1).
- The complete license inventory and third-party audit, covering the transitive graph, not
  only the ten direct dependencies
  (`docs/planning/v0.1/07-repository-and-dependencies.md` §5, §7-§8) (I2).
- Release packaging for all three platforms (I3).
- The published deferred-work list, known-risk list, and upgrade notes for the provider
  preview surfaces (I4), sourced from `docs/planning/v0.1/11-risks.md` and
  `docs/planning/v0.1/12-deferred.md`.

**Gate S6 — acceptance criteria.**

1. Every gate G1-G5 has been re-run against the versions actually shipped, or the shipped
   pins are unchanged from the versions the gates last passed against. A pin move
   invalidates the gate mechanically (`docs/planning/gates/README.md` re-run policy), and
   shipping on an invalidated gate is shipping on a `NOT RUN`.
2. The license inventory covers the transitive graph and every copyleft arm is flagged and
   dispositioned — currently one: `zenoh`'s EPL-2.0 arm, with OAC electing Apache-2.0
   (`docs/planning/v0.1/07-repository-and-dependencies.md` §6).
3. Upgrade notes exist for each non-supported surface, naming its label and its pinned
   version: Claude Code Channels (**research preview**) and the Codex app-server
   (**experimental**, per-method gating).
4. The deferred list restates every `docs/planning/ADR-001.md` non-goal and v0.1 exclusion
   as an explicit "not in v0.1" with a reason.
5. Every `UNVERIFIED` item still open in `docs/planning/STATUS.md` appears in the published
   known-risk list — no item is closed by the act of releasing.

**Go/no-go condition.** **No-go on criterion 1.** Shipping against a moved pin without
re-running the affected gates ships a `NOT RUN` as though it were a `PASS`, which is the
"probably" the whole pipeline is built to refuse. Criteria 2-5 are release-blocking as
documentation completeness, not as engineering defects: their failure mode is a user
who cannot tell what is proven from what is assumed.

---

## 11. DESIGN acceptance criteria to stage

Required by `docs/planning/PLANNING-PROMPT.md` §11 item 2: every
`docs/planning/DESIGN.md` v0.1 acceptance criterion maps to a named test in
`docs/planning/v0.1/09-test-strategy.md` **and a stage in this file**. Criterion text is
verbatim from `docs/planning/DESIGN.md` "v0.1 acceptance criteria". The named test is
`docs/planning/v0.1/09-test-strategy.md` §11's cell, cited not restated.

| # | Criterion (verbatim) | First proven at | Finally verified at |
|---|---|---|---|
| 1 | "One-command local startup." | Stage 4 (§8, the launch demonstration) | Stage 5 (H4), re-run on the packaged binary at Stage 6 |
| 2 | "Claude and Codex adapters expose distinct neutral sessions." | Stage 3 (§7, against the fakes) | Stage 4 (§8, real adapters, opt-in) |
| 3 | "Sessions discover one another through neutral APIs." | Stage 3 (§7, in-memory transport) | Stage 4 (§8, real Zenoh over loopback, CI-default) |
| 4 | "Claude actively messages Codex without receiver polling." | Stage 1 (§5, G1 proves the wake semantics exist at all) | Stage 5 (§9, H1 clause 1); contract half at Stage 3 |
| 5 | "Codex actively replies to Claude." | Stage 1 (§5, G2) | Stage 5 (§9, H1 clause 2) |
| 6 | "Authenticated provenance and authorization are enforced." | Stage 1 (§5, G5 for the provenance half) | Stage 5 (§9, H2) |
| 7 | "Replay/duplicate handling exists." | Stage 3 (§7, F4 against the fakes) | Stage 5 (§9, H2 and H3 across a restart) |
| 8 | "No cross-provider model API invocation." | Stage 1 (§5, G2's no-credentials criterion) | Stage 5 (§9, H1 clause 3), plus the CI-default boundary lint from Stage 3 on |
| 9 | "Zenoh-specific types stay inside its transport module." | Stage 3 (§7, the lint exists and passes over an empty tree) | Stage 4 (§8, the lint over a real `transports/zenoh/`) — **lint, not a runtime test**, with the scope gap carried |
| 10 | "Transport contract is documented enough to independently add a second backend." | Stage 2 (§6, E9) | Stage 2 — **a doc proof, not a test**; currently **NOT MET**, every NATS/MQTT cell `UNVERIFIED` |

**Two criteria are not proven by a runtime test, and that is recorded rather than
papered over** — 9 (lint) and 10 (doc proof), per
`docs/planning/v0.1/09-test-strategy.md` §11's own rows.

---

## 12. Stages are not decomposed into tickets here

Stated explicitly because it is one of issue #31's acceptance boxes and one of
`docs/planning/PLANNING-PROMPT.md` §8's closing instructions.

This file creates no work item. Every task key it names (B1-B4, D1-D7, E1-E9, F1-F12,
G1-G11, H1-H5, I1-I4) is a **citation** to an item that already exists in
`docs/planning/backlog/02-tasks-AB.json`, `03-tasks-CD.json`, `04-tasks-EF.json`, or
`05-tasks-GHIJ.json`, cited to say which existing item owns an artifact this file
requires. The epic-to-stage mapping is `docs/planning/backlog/01-epics.json`'s, not a new
one. Sequencing within a stage, task sizing, and assignment are the backlog's job and the
build phase's.

---

## 13. Cross-reference block

Every reference is a repo-relative path; no prior context is assumed.

- `docs/planning/ADR-001.md`
- `docs/planning/ADR-001-AMENDMENTS.md`
- `docs/planning/DESIGN.md`
- `docs/planning/PLANNING-PROMPT.md` §4, §8, §9 item 11, §11
- `docs/planning/PINS.md`
- `docs/planning/REVERIFICATION-B2.md`
- `docs/planning/STATUS.md`
- `docs/planning/gates/README.md`
- `docs/planning/gates/G1-result.md` … `G5-result.md`
- `docs/planning/backlog/01-epics.json`, `02-tasks-AB.json`, `03-tasks-CD.json`,
  `04-tasks-EF.json`, `05-tasks-GHIJ.json`
- `docs/planning/v0.1/02-gating-findings.md`
- `docs/planning/v0.1/03-decisions-and-amendments.md`
- `docs/planning/v0.1/05-interfaces.md`
- `docs/planning/v0.1/06-security.md`
- `docs/planning/v0.1/07-repository-and-dependencies.md`
- `docs/planning/v0.1/08-cli-and-deployment.md`
- `docs/planning/v0.1/09-test-strategy.md`
- `docs/planning/v0.1/11-risks.md` (task A12, not yet landed)
- `docs/planning/v0.1/12-deferred.md` (task A12, not yet landed)
- `.claude/skills/oac-gates/SKILL.md`

---

## 14. Evidence pass (`oac-evidence` §8)

- **No new externally verifiable claim is made by this file.** Every provider, protocol,
  crate, and version fact above is cited to `docs/planning/PINS.md`,
  `docs/planning/v0.1/02-gating-findings.md`, `docs/planning/v0.1/07-repository-and-dependencies.md`,
  or `docs/planning/v0.1/09-test-strategy.md`. Nothing is re-fetched and no new retrieval
  date is issued.
- **Verbatim names only**, each copied from the cited file:
  `notifications/claude/channel`, `--dangerously-load-development-channels`,
  `codex --remote`, `codex mcp add oac -- oac mcp-shim`,
  `claude --dangerously-load-development-channels server:oac`,
  `io.github.rossgraeber/oac-session-channels`, `127.0.0.1`, `zenohd`, `Cargo.lock`.
- **Surface labels stated where a surface is named:** Claude Code Channels — **research
  preview**; Codex CLI / app-server — **experimental** (per-method gating); Zenoh and the
  MCP current (`2026-07-28`) and legacy (`2025-11-25`) revisions — **supported**
  (`docs/planning/v0.1/01-capability-matrix.md` §1). Both non-supported surfaces carry
  their compatibility-shim boundary at §8, still UNNAMED and labelled `UNVERIFIED`
  (register entry C11), exactly as `docs/planning/STATUS.md` carries it.
- **No `UNVERIFIED` label is dropped.** Four are carried into this file with their reasons
  intact: the two shim-boundary items (§8), the E9 NATS/MQTT design-for-replacement cells
  (§6, §9, §11), and the Zenoh-containment lint's recorded scope gap (§8, §9).
- **The timebox values in §3 are OAC's own scheduling parameters**, not claims about an
  external system, and carry a stated reversal condition instead of a citation.
- **No gate verdict is changed.** All five remain `NOT RUN`.

---

## 15. Boundary pass (`oac-boundaries`)

- No stage above proposes calling a provider model API, holding provider credentials,
  implementing inference or routing, or owning a harness's turn loop. Stage 4's §8
  criterion 4 makes the opposite an explicit no-go condition.
- No neutral artifact in any stage mentions Zenoh: Zenoh vocabulary appears only where the
  `transports/zenoh/` module, the G3 gate, or the containment lint is the subject (§5, §7,
  §8, §9, §11). Stage 2's Gate S2 criterion 2 enforces this mechanically over `spec/`.
- No adapter in any stage polls an inbox: the `contract/adapter/no-polling` test is a
  Stage 3 acceptance criterion and a Stage 4 acceptance criterion (§7, §8).
- No stage depends on UI or terminal scraping, or on an undocumented private RPC: each
  executable demonstration names a documented surface.
- Naming resolution applied throughout — "Open Agent Channel (OAC)", "OAC Session
  Channels", `oac`; no bare "Session Channels" or `sessionchannels` appears anywhere in
  this file.
- No code blocks; the only quoted symbols are command lines, method names, and paths.

---

## 16. Acceptance close-out against issue #31

- [x] **Stages 0-6, each with entry criteria, exit artifacts, and a gate** — §4-§10; the
      gate column of §1's overview names each one.
- [x] **Every stage names modules created or changed, by name and responsibility, citing
      `07-repository-and-dependencies.md`** — §4 (none, and why), §5 (none durable), §6
      (`spec/`, `tests/protocol/`, the `core/` contract surface), §7 (`core/`, the daemon
      identity code, the in-memory transport, `tests/`, the scaffold), §8
      (`transports/zenoh/`, both adapters, `cli/`), §9 (test trees only), §10 (`docs/`,
      packaging).
- [x] **Every stage names its prerequisite §5 decisions, citing
      `03-decisions-and-amendments.md`** — §4 (none, and why), §5 (two informative only),
      §6 (3, 4, 5, 6, 8, 9), §7 (1, 2, 5, 6, 7, 12), §8 (2, 8, 9, 10, 11), §9 (all
      twelve), §10 (1, 11, 12).
- [x] **Every stage names its executable demonstration** — §4-§10 each carry an
      "Executable demonstration" paragraph.
- [x] **Every stage states acceptance criteria and a go/no-go condition** — §4-§10 each
      carry both, as numbered criteria plus a named go/no-go.
- [x] **Order is risk-first: no substantial core or transport code before the provider
      gates pass** — §2 states the rule normatively; §4 and §5 create no modules; §7's
      entry criteria name Gate S0 and Gate S1 as the release condition for that rule.
- [x] **Every timebox states what happens when it expires** — §3, five numbered
      consequences, with the values set per spike.
- [x] **Stages are not decomposed into tickets here** — §12; every task key is a citation
      to an existing backlog item.

---

## 17. Cross-file updates in this change

- `docs/planning/STATUS.md`: "Last updated" history block and the "Open epics" row updated
  to record A11 (issue #31) landing, following the precedent A8-A10 set. The history block
  is appended to, never replaced.
- No pin moved, no gate verdict changed, no new `UNVERIFIED` item added — every
  `UNVERIFIED` item this file cites (§14) was already open and is carried with its
  original reason.
- No ADR-001 amendment is proposed. Nothing in this file contradicts a settled
  `docs/planning/ADR-001.md` decision; the pipeline it lays out is
  `docs/planning/PLANNING-PROMPT.md` §8's, with the timebox values (§3) and the stage-exit
  gate names `S0`-`S6` (§1) added as this file's own decisions rather than as amendments,
  since §8 leaves both to the plan.
- `docs/planning/v0.1/09-test-strategy.md` §1 already forward-references this file as the
  owner of stage entry/exit criteria and go/no-go conditions; that reference is now
  satisfied. No edit to that file is required.
