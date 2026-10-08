# F-6 — Stage 3 exit: Gate S3 and the Stage 4 go/no-go

- **Date:** 2026-10-08. First run 2026-10-07 on `5877b39`, re-run against the #343
  capture (PR #344), final run after PR #346.
- **Issue:** #6 (Epic F, Stage 3, milestone M4). Stage 3 has no exit task of its own in
  the backlog, so this record is named after the epic and its issue, as
  `E-5-stage2-exit.md` is. This PR is #343's last box: "Re-run the Stage 3 exit record".
- **Gate:** Gate S3 (`docs/planning/v0.1/10-stages.md` §7, L631-L651).
- **Status:** **Decided: go.** Gate S3 holds on all five criteria (§2), every Stage 3 exit
  artifact exists (§1), and Stage 4 (Epic G, #7, milestone M5) opens (§6). Criterion 5 holds
  after the #343 capture and the lead's decision to remove the fake's last unrecorded
  behaviours (PR #346, §3, §5).
- **Owns:** its own text, the STATUS top entry for this exit, the STATUS "Current stage"
  cells, the `10-stages.md` §7 "Current verdict" paragraph, the `oac-implementation` §0
  stage-exit pointer, and the ledger changes in §7: `11-risks.md` rows 64 and 70, plus
  three STATUS "Open UNVERIFIED items" entries and one new entry. It changes no file under
  `spec/` (frozen, E7 §7) and no gate verdict, pin or ADR text.
- **Skills:** `oac`, `oac-evidence`, `oac-boundaries`.

This record does not restate the tests. Each crate's own documentation and each task's PR
stay authoritative for what that code does. This file adds:

- the Gate S3 checklist, re-run on current `main`;
- the exit-artifact inventory;
- the criterion 5 history (finding F-1, #343, #346);
- the Epic F close-out, the carry-overs and the Stage 4 entry check.

Line citations are to `main` at `090d2a7` (the PR #346 merge, 2026-10-08) unless a line
says otherwise. Between `5877b39` (this record's first run) and `090d2a7`, nothing changed
under `.github/`, `core/`, `cli/`, `adapters/`, `spec/`, `scripts/` or `tests/security/`.
The only changes there are in:

- the fakes, the fixtures and the herdr tooling (#344, #346);
- `tests/protocol/contract/adapter/src/codex.rs`, `tests/protocol/contract/adapter/tests/stand_in.rs`
  and `transports/memory/tests/pipelines.rs` (#346);
- the planning files.

Every check in §2 and §8 was re-run on this branch after merging `090d2a7`.

---

## 1. Stage 3 exit artifacts (`10-stages.md` §7 "Exit artifacts", L619-L629)

| Artifact | Where | Evidence |
|---|---|---|
| A CI pipeline whose default tier is green under the CI-default rule of `09-test-strategy.md` §3 (F12) | `.github/workflows/ci.yml` (jobs `test (<os>)`, `crate-deps (<os>)`, `licenses (<os>)` on ubuntu, windows and macos); `.github/workflows/boundary-lint.yml` (jobs `boundary-lint`, `fixture-manifest`, `containment`, `workflow-policy`, `agents-skills-sync`) | #61 closed by PR #330 (merge `9bba8b0`). On the `090d2a7` push, `ci` run 37740602170 and `boundary-lint` run 37740602168 are `success`, all 14 jobs. |
| The fake Claude and fake Codex endpoints, loaded from the Stage 1 fixtures (F8, F9) | `tests/fakes/claude/` (`oac-fake-claude`), `tests/fakes/codex-app-server/` (Node) | #57 closed by PR #319, #58 by PR #318. Both were aligned with the #343 capture by PR #344, and the fake Codex was cut to recorded behaviour by PR #346. Both load `docs/planning/gates/fixtures/` in place. Criterion 5 below. |
| The adapter and transport contract suites (F10), with the named `contract/adapter/no-polling` test | `tests/protocol/contract/adapter/` (`oac-contract-adapter`), `tests/protocol/contract/transport/` (`oac-contract-transport`) | PR #323 (merge `9292674`), with follow-ups PR #336 (#324), PR #341 (#340) and PR #346. The named check is `tests/protocol/contract/adapter/src/lib.rs` L767. #59 stays open (§4). |
| The security suite against the fakes (F11) | `tests/security/` (`oac-security-suite`) | #60 closed by PR #327, follow-up PR #336 (#329). `node tests/security/check-compiled-tests.mjs`: "every compiled test is mapped, every mapped test compiled and run as mapped: CLEAN". |
| The provider-integration tier, its own separately invoked target, existing but not run by default (F12) | `.github/workflows/herdr-provider-optin.yml`; `tests/integration/README.md` | Exists for the herdr scenarios: manual dispatch, or a push to main that changes `docs/planning/PINS.md`, on operator-owned runners (`herdr-provider-optin.yml` L9-L14). Provider tests of OAC's own adapters cannot exist before Epic G; they are pending with G4 (#65) and H1 (#73) (`tests/integration/README.md` L30-L37). |

**The executable demonstration** (`10-stages.md` §7, L611-L617) holds, spread over named
tests rather than one. `cargo test --workspace` reaches green with nothing opt-in:

- **Compose to correlate.** `transports/memory/tests/pipelines.rs`
  `claude_and_codex_fakes_exchange_through_the_core_pipelines` (L775) composes, signs,
  publishes over the in-memory transport, authorizes in `core/`, delivers to the fake
  Claude endpoint and the fake Codex app-server, replies and correlates.
  - The same run rejects an uncorrelated message without a grant as `rejected` /
    `unauthorized`.
  - Its Codex leg reaches session input through a `thread/queue/add` to an idle thread
    (`codex_inputs`, L457, used at L645), which is recorded (§3, row i).
  - Its turned-away hand-off ends `failed` / `handoff-failed`, with no steering fallback,
    against the recorded archived refusal (L703-L752).
- **Replay.** `tests/security/tests/replay.rs` `row04_copy_outside_the_replay_window_is_rejected`
  (L19) and the rest of row 4.
- **Duplicate.** `replay.rs` `row04_replay_inside_the_window_is_a_duplicate_handed_off_once`
  (L114).
- **Unauthorized routing.** `tests/security/tests/routing.rs`
  `row02_unauthorized_send_through_the_composed_pipeline` (L317) and the rest of rows 2, 7
  and 14.

The core send and receive pipelines these tests drive are #313 (PR #326), a Stage 3
follow-up that no F task named.

## 2. Gate S3 acceptance criteria (`10-stages.md` §7, L631-L643)

1. **The default test run is green and touches no live provider, no API key, and no
   network beyond loopback: holds.**
   - **Green.** The `ci` run on `090d2a7` passes `test (ubuntu-latest)`,
     `test (windows-latest)` and `test (macos-latest)`. Per-OS totals over every
     `test result` line of `cargo test --workspace` (passed, failed, ignored):
     - ubuntu: 451, 0, 13;
     - macos: 451, 0, 12;
     - windows: 446, 0, 12.

     A local run on Windows on this branch (rustc `1.98.1`, Node `v25.2.1`, `--offline`)
     matches the windows job: 446, 0, 12.
   - **No network beyond loopback.** Every cargo step after `cargo fetch` runs with
     `CARGO_NET_OFFLINE` (`ci.yml` L109-L133).
     - On ubuntu the test steps run under `scripts/loopback-only.sh` (`ci.yml`
       L119-L133). Its probe prints "loopback connects; 192.0.2.1:443 ENETUNREACH;
       1.1.1.1:443 ENETUNREACH; inherited fds closed".
     - The only networked step is `cargo fetch` of the toolchain pin and `Cargo.lock`,
       before any test (`ci.yml` L22-L23, L105-L107).
   - **No live provider and no API key.** The only processes the Rust tests start are:
     - `node`, running the fake Codex app-server
       (`tests/protocol/contract/adapter/src/codex.rs` L166);
     - `sh` and `cargo`;
     - the platform `kill`/`tasklist`/`taskkill`, used to reap it.

     No harness binary is started. `scripts/check-workflows.mjs` (job `workflow-policy`)
     fails a default-tier workflow that names a secret, a provider credential, an
     `OAC_TEST_*` switch, an `--ignored` run, a self-hosted runner or a harness CLI
     install (L55, L130). It reads "7 workflow(s), 0 local action(s), 0 violation(s)".
   - **Residue, already recorded by PR #330.** Windows and macos have no runtime sandbox,
     so the rule there rests on the offline cargo setting, the fakes binding loopback only
     and the static lint (`ci.yml` L29-L33). A network namespace does not scope
     Unix-domain sockets (`scripts/loopback-only.sh` header).
2. **A plain default test invocation never triggers a provider-integration test: holds.**
   The ignored tests are the only opt-in ones: 12 on windows and macos, 13 on ubuntu.
   - 10 security placeholders `#[ignore = "GATED on #N ..."]` whose bodies fail if run
     (`tests/security/tests/provenance.rs` L365, L375, L386, L397; `never_steer.rs` L166,
     L176, L188; `local_ipc.rs` L29, L37, L46).
   - The real credential-store test (`cli/src/keystore.rs` L673). It also needs
     `OAC_TEST_REAL_KEYRING=1` and runs in `keystore-optin.yml`. A second one, L713, is
     compiled on Linux only (`#[cfg(target_os = "linux")]`, L711): the 13th on ubuntu.
   - The full-scale record churn test (`core/src/authorization.rs` L3115), in
     `scale-optin.yml`.

   None is a provider test. The herdr provider scenarios run only through
   `herdr-provider-optin.yml`, which never starts from a pull request. The herdr driver's
   own self-test runs in no CI workflow at all (#345, open). That is a coverage gap in the
   test tooling, not a provider test in the default run.
3. **The dependency-direction lint passes: holds.**
   - `node scripts/check-crate-deps.mjs`: "crate dependency direction: CLEAN (10 workspace
     members, 265 packages)". `--self-test`: 62/62.
   - The rules are in the script header (L7-L44). `core/` reaches nothing in-repo.
     `adapters/*` reach `core/` only, never another adapter or a transport. The `zenoh`
     crates are reachable only from `transports/zenoh` and `cli/`.
   - The `crate-deps (<os>)` jobs on `090d2a7` also run the mutation test, all `success`.
4. **The `contract/adapter/no-polling` assertion passes against both fakes, in the
   call-class shape of `09-test-strategy.md` §5: holds.**
   - **Shape.** The check (`tests/protocol/contract/adapter/src/lib.rs` L759-L803):
     - compares establishment calls before and after three deliveries, which must stay
       flat;
     - requires zero message-fetch calls between arrivals, and no inbox-style surface
       offered;
     - requires each message to become session input as pushed.

     That is §5's two classes (L218-L234), not call-count growth.
   - **Both fakes.** Both checks pass on all three OSes and locally:
     - `the_channel_stand_in_passes_under_both_mid_turn_release_settings` (fake Claude,
       `tests/protocol/contract/adapter/tests/stand_in.rs` L502);
     - `the_queue_stand_in_passes_against_the_fake_codex_app_server` (fake Codex, L921).

     `the_suite_catches_each_planted_channel_breach` (L516) and
     `the_suite_catches_each_planted_queue_breach` (L959) show the suite fails a breach.
   - **The fake Codex half rests on recorded behaviour.** `CodexFake::drain` only
     completes a turn that is already running (`codex.rs` L388-L402). So each idle-session
     message becomes input because the fake starts a turn when an add reaches an idle,
     loaded thread (`tests/fakes/codex-app-server/lib/model.mjs` L534, L541-L560). Live
     Codex `0.161.0` did the same, with no `turn/start` sent
     (`docs/planning/gates/fixtures/s3-codex-capture/transcript-2026-10-07-0.161.0-herdr.jsonl`
     L57-L73, L90, L866-L873).
   - **No adapter yet.** The suite runs on two test-only stand-ins: no crate implements
     `ProviderAdapter` yet. `tests/real_adapters.rs` `no_real_adapter_implements_the_trait_yet`
     (L101) fails as soon as one does, so the suite must be wired to it then (§4).
5. **Every fake's behaviour is traceable to a recorded Stage 1 fixture; no fake behaviour
   is invented from the spec: holds.**
   - **Fake Claude.** Every row of its behaviour table cites a fixture
     (`tests/fakes/claude/src/lib.rs` L20-L32), at Claude Code `2.1.283`. Three parts come
     from other recorded versions:
     - the later tool-call ids and the `toolu_` form, at `2.1.285` (G1 herdr capture);
     - `MidTurnRelease::AllAtBoundary` (L139-L141), from the original G1 run at `2.1.282`
       (`docs/planning/gates/fixtures/g1-claude-wake/transcript.jsonl`).

     Where no fixture shows what Claude Code does, the fake halts or refuses (L34-L84).
     #346 did not change the fake Claude.
   - **Fake Codex.**
     - "Recorded behaviours" cites a D6, G2, G5 or S3 fixture on every row
       (`tests/fakes/codex-app-server/README.md` L96-L143).
     - "Source-only behaviours" reads "None. Every behaviour the fake models traces to a
       recorded fixture" (L144-L160).
     - Every other unrecorded method gets the fake's own `-32099` `not-modelled` error,
       never an invented Codex frame (L161-L208).
     - `node tests/fakes/codex-app-server/self-test.mjs`: 29/29.
   - **Checked by hand on top of the README** (§3): the test controls, the stand-in values
     and the turned-away checks.

**Go/no-go (`10-stages.md` §7, L645-L651): go.** Criteria 1-5 hold. Criteria 1 and 2,
the no-go criteria, hold, so no default-tier waiver is involved.

## 3. Criterion 5: finding F-1, #343 and #346

**F-1** (this record's first run, 2026-10-07) found twelve fake behaviours with no
recorded fixture: ten fake Codex rows (a-j) and two fake Claude rows (k-l). Under §7 that
was no-go, and the work went back to Stage 1 (#343).

**The #343 capture** (PR #344) recorded every row a documented client request can reach.

- **Paths.** Fixtures are under `docs/planning/gates/fixtures/`:
  - **S3**: `s3-codex-capture/transcript-2026-10-07-0.161.0-herdr.jsonl`, run record
    `docs/planning/gates/herdr-runs/S3-codex-2026-10-07.md`;
  - **S3-QI**: `s3-codex-capture/transcript-2026-10-08-0.161.0-queued-interrupt-herdr.jsonl`,
    run record `S3-codex-2026-10-08.md`;
  - **G1-H**: `g1-claude-wake/transcript-2026-10-07-2.1.285-herdr.jsonl`, run record
    `G1-2026-10-07.md`.
- **Codex rows a-j:**
  - a: S3 L99-L100;
  - b: L92-L93;
  - c: L106-L108;
  - d: ephemeral L111-L119, archived L1030-L1032, unknown thread `-32603` L109-L110;
  - e: S3-QI L79-L170 and S3 L907-L955;
  - f: S3 L135-L139, L826-L865;
  - g: S3 L899-L907 and S3-QI L77-L90;
  - h: S3 L866-L873;
  - i: S3 L57-L73, L90, L866-L873;
  - j: S3 L956-L1019.
- **Claude rows k-l:**
  - k: G1-H L14, L16, L18;
  - l: the `toolu_01` + 22-character form of all 17 recorded ids.
- **Ledger.** `11-risks.md` rows 62 and 67-69 are closed, and rows 65-66 are narrowed.

**Left after #343, and a correction.** Three fake Codex refusals stayed source-only: a
loaded multi-agent v2 subagent, an unloaded spawned subagent, and "no queue service". No
documented client request triggers any of them.

The previous run of this record said that only the fake's own self-test reached them. That
was true for the two subagent refusals and **false for "no queue service"** (PR #342
review, https://github.com/RossGraeber/OAC/pull/342#issuecomment-6051936763, finding 1).
At `4642b4f`, three things rested on that unrecorded refusal:

- **The queue stand-in.** Its `refuse_hand_offs` / `allow_hand_offs` called
  `CodexFake::set_queue_available` (`stand_in.rs` L874-L881; `codex.rs` L404-L407 at
  `4642b4f`).
- **The contract suite's turned-away hand-off checks.** These ran through that refusal:
  - [SEC-AUZ-027] `turned-away-makes-no-other-call` and [IFC-ADP-051]
    `completed-only-on-success` (`lib.rs` L910-L947);
  - the planted `QueueBreach::FallsBackToSteer`.
- **The demonstration's turned-away hand-off.** `pipelines.rs` L705-L722 at `4642b4f`
  turned off the queue service the same way.

**The lead's decision, and #346.** The lead decided to remove the unrecorded behaviours
from the fake, rather than hold the gate or depart from §7. PR #346 (merge `090d2a7`) did
that:

- **Subagent threads.** An add to a subagent thread answers the fake's own `NOT_MODELLED`,
  and so does every Codex method on such a thread.
- **No queue service.** The queue-unavailable control `oacFake/queue/setAvailable` is
  removed.
- **Thread states.** `oacFake/thread/create` makes only recorded thread states.
- **Other unrecorded answers.** `NOT_MODELLED` now also answers an add before a thread's
  first turn, and a list taken while an ephemeral or archived thread exists. So does a
  `thread/turns/list` that would be empty or that covers a thread made materialized by the
  control.
- **The turned-away checks.** SEC-AUZ-027, IFC-ADP-051, `FallsBackToSteer` and the
  demonstration now:
  - archive the session's own thread with the new control `oacFake/thread/setArchived`;
  - get the recorded archived refusal, S3 L1030-L1032 (`stand_in.rs` L878-L885,
    `codex.rs` L404-L422, `pipelines.rs` L703-L752).

  `spec/bindings/mcp.md` §8.2.1 classifies that refusal `handoff-failed` ([SC-DLV-009]),
  as it did "no queue service", so no expected outcome changed.

The README's "Source-only behaviours" section now reads "None" (L144-L160). What Codex
itself does in the three cases stays open as Codex behaviour, not fake behaviour:
`11-risks.md` row 65 and the STATUS #274 entry, owner G7 (#68).

**What I checked by hand beyond the README, at `090d2a7`:**

- **`oacFake/thread/setArchived`** (`model.mjs`; README L51) is a test set-up control. It
  sends no frame. `true` puts an idle, materialized top-level thread with an empty queue
  into the state S3 recorded after `thread/archive` (archived, not loaded, S3
  L1023-L1030), so an add gets the recorded refusal. `false` restores a recorded state
  (loaded and idle, S3 L981-L1018).
  - The transition back is not recorded (no `thread/unarchive` capture). But the fake
    shows no frame for it, and it is not a Codex answer.
  - It also omits the `thread/status/changed` / `thread/archived` notifications real
    Codex sends on archive (S3 L1024-L1029; G7 note on #68,
    https://github.com/RossGraeber/OAC/issues/68#issuecomment-6053040118). That is an
    omission, not invented behaviour.
  - What the adapter sees on the wire, the add and its refusal, is recorded.
  - The control log keeps it out of any adapter's path (README "Control methods").
- **`PRIOR_TURNS_PREVIEW`** (`model.mjs` L42, L676) is the `preview` of a thread made
  materialized by `oacFake/thread/create`. Its form is recorded: every recorded
  `thread/list` row and `thread/resume` result after a turn has a non-empty preview, the
  thread's first user message (S3 L42, L50; "" only before the first turn, S3 L113,
  L124).
  - Its value stands in for a first message the fake never saw. It reads
    `oac fake Codex app-server: earlier turns not modelled`, so it cannot pass for real
    text.
  - The fake treats ids and clocks the same way (README "Omitted notifications": "Ids are
    fresh UUIDv7 strings and times are the current clock").
  - No check in the contract suites, the security suite or the demonstration reads a
    preview. `git grep -i preview` under `tests/protocol/contract/`, `tests/security/`,
    `transports/` and `core/` finds only a doc comment, `codex.rs` L65.
  - The fake's self-test pins the value (L515).

  I judge this a placeholder of a recorded form, like the `toolu_01OacFake…` id (row l),
  not an invented behaviour.
- **`oacFake/turn/complete`** ends a turn as completed or interrupted. Both frame sequences
  are recorded (D6 `transcript-conn1`; S3 L899-L907).
- **Not modelled, and so not fake behaviour.** Two things live Codex does are omitted from
  the fake, not invented:
  - the 10 s queue-watcher `thread/queue/changed`;
  - thread notifications to connections not subscribed to the thread (row 70, §7).

  Row 66(b), other clients editing the queue, is likewise not a fake behaviour: those
  methods answer `NOT_MODELLED`.

### Live/fake differences the capture found, and the fake now follows (PR #344)

Sources: README "What the S3 capture changed"; S3 record rows 1, 6, 9 and 12;
`tests/fakes/claude/src/lib.rs` L71-L75.

1. **Unknown-thread refusal.** An add to an unknown thread is
   `-32603 "failed to read thread: invalid thread-store request: no rollout found for
   thread id <id>"` (S3 L109-L110). The source suggested `-32600 "thread not found: <id>"`.
2. **Dispatch on load.** Loading an unloaded thread with `thread/resume` dispatches its
   queue (S3 L981-L1019). The fake used to leave it waiting forever.
3. **Queue-changed ordering.** An idle add's second `thread/queue/changed` comes before
   the response (S3 L65-L73).
4. **Interrupted-turn frames.** An interrupted turn's `turn/completed` has `items: []` and
   `itemsView: "notLoaded"`, and its agent message never gets an `item/completed`
   (S3 L899-L907; S3-QI L77-L90).
5. **The `toolu_` form.** The fake Claude's synthetic id now has the recorded form,
   `toolu_01OacFake<15 digits>`.

**Spec amendments: #308.** The recordings contradict or settle frozen
`spec/bindings/mcp.md` §8.2.1 text at L964-L965, L998-L1001, L1016-L1029 and L1051-L1067,
and in the §10 UNVERIFIED row. No normative MUST changes; [MCPB-CDX-002] to [MCPB-CDX-005]
are unaffected. They are batched for the next minor version in #308:

- the amendments, https://github.com/RossGraeber/OAC/issues/308#issuecomment-6051204351;
- the fifth-refusal caveat, https://github.com/RossGraeber/OAC/issues/308#issuecomment-6051738540.

They land under E7 §7. No `spec/` file changes here.

## 4. Epic F close-out

| Task | Issue | State | Merged PR(s) |
|---|---|---|---|
| F1 scaffold and module boundaries | #50 | closed | #311 |
| F2 core types and envelope | #51 | closed | #312 |
| F3 identity, keys, signing | #52 | closed | #315 |
| F4 replay and duplicates | #53 | closed | #317 |
| F5 authorization and pairing | #54 | closed | #316; #322 (finding F5-1 acknowledged) |
| F6 presence and receipts | #55 | closed | #321 |
| F7 in-memory transport | #56 | closed | #314; #323 (its last acceptance item) |
| F8 fake Claude | #57 | closed | #319; #344 |
| F9 fake Codex | #58 | closed | #318; #344; #346 |
| F10 contract suites | #59 | **open** | #323; #336 (#324); #341 (#340); #346 |
| F11 security suite | #60 | closed | #327; #336 (#329) |
| F12 default CI tier | #61 | closed | #330; #336 (#332) |

**Follow-ups labelled `stage:3-core`, all closed:**

- #305 (PR #309, landed just before Stage 3 opened);
- #313 (PR #326);
- #320, #325 and #328 (PR #334);
- #331 (PR #333);
- #335 (PR #337);
- #324, #329 and #332 (PR #336);
- #338 (PR #339);
- #340 (PR #341).

**Open `stage:3-core` issues.** Three remain:

- #6, the epic itself;
- #59, below;
- #347, a contract-suite gap, also below.

**The Stage 1 capture** for criterion 5 is #343 (PR #344), and this PR is its last box.
Nothing under `spec/` changed in Stage 3: `git log 20482f1..090d2a7 -- spec/` is empty.

**#59 (F10) stays open until Epic G.** Of its four acceptance items, the Stage 3 halves
are done:

- **Real adapters.** The adapter suite runs against both fakes. Running it against the
  real adapters unchanged waits for G4-G8: no adapter implements `ProviderAdapter` yet
  (`adapters/claude/src/lib.rs` and `adapters/codex/src/lib.rs` are the F1 scaffold).
- **Zenoh.** The transport suite runs unchanged against the in-memory transport under four
  media (`transports/memory/tests/contract.rs`
  `the_memory_transport_passes_the_contract_suite`, L145). Running it against Zenoh waits
  for G1 (#62).
- **No polling.** The no-polling assertion is met (criterion 4).
- **Routing through core.** Routing through core policy is checked statically against
  both scaffold crates (`tests/real_adapters.rs`
  `the_real_adapters_pass_the_static_routing_checks`, L39) and at run time against the
  stand-ins.

That matches what Stage 4's entry asks of the suites: "contract suites exist and pass
against fakes and the in-memory transport" (`10-stages.md` §8, L661-L662). The open half
is Stage 4's own executable demonstration 1 (L711-L716). PR #323 used "Refs #59" for this
reason.

**#347 (open, `stage:3-core`, Refs #59).** The adapter contract suite does not tell
`handoff-failed` from not-now. A stand-in that reported the recorded archived refusal as
`NotNow` would still pass every check (PR #346 review mutation M6). The gap predates #346.

Does it affect a Gate S3 criterion? I judge that **it does not**:

- **Criteria 1-3** are about the default run, provider isolation and dependency
  direction.
- **Criterion 4** is the no-polling assertion. It does not involve refusal outcomes.
- **Criterion 5** is about fake behaviour. The fake answers the recorded refusal
  correctly. The gap is a missing assertion in the suite, not something the fake does.

The suite is an exit artifact, and it exists. The demonstration does assert
`handoff-failed` for the turned-away hand-off (`pipelines.rs` L703-L723). The gap matters
when real adapters run the suite, which is Stage 4 work under #59. It is a known weakness
of the F10 artifact that Stage 3 hands on, and #347 tracks it.

The task checkboxes in #6's body, and closing #6, are left for the lead. Epic F stays
open.

## 5. Sign-off

**The lead's decision on criterion 5.** Remove the last unrecorded behaviours from the
fake (PR #346), rather than hold Gate S3 or depart from `10-stages.md` §7. With #346
merged, criterion 5 holds as worded. No §7 departure is recorded, and none is needed.

**Change control.** The rule is E7 §7: the change, a version bump where a frozen item
changes, and the lead's approval of the PR, with no separate document. No `spec/` item
changes here, so no version bump is due.

**The merge is the approval.** The lead authors this PR, and GitHub does not let an author
approve their own PR. As for PR #276 (E-5 §2, L52-L55), the merge is the lead's approval.
This commit is the sign-off commit. It sets the Status above and the verdict below, and
opens Stage 4 in STATUS and `10-stages.md`. All of that is in force from the merge.

**Lead's verdict:** **go.** Gate S3 is met. Stage 3 exits, and Stage 4 opens.

## 6. Stage 4 go/no-go (`10-stages.md` §8, L655-L668)

**Go. Stage 3 exits, and Stage 4 (Epic G, #7, milestone M5) opens.** Each §8 entry
condition is met:

- **Gate S3 met (§7).** §2 above: contract suites exist and pass against the fakes and the
  in-memory transport.
- **G1 and G2 carry `PASS` or `PASS (FALLBACK TAKEN)`.** Both are `PASS` with no fallback
  (`docs/planning/decisions/D7-stage1-exit.md` §1). G4 (#65), G6 (#67) and G7 (#68) carry
  `gate:*` labels; each checks its verdict in STATUS before starting, per the `oac` router.
- **G3's verdict fixes the discovery mode.** G3 is `PASS` on the primary multicast path
  (D7 §1), so Stage 4 implements multicast scouting.

Stages 5 and 6 stay blocked, each behind its own gate. Stage 5 starts only after Gate S4
(`10-stages.md` §9).

## 7. Continuing items and ledger changes

### Ledger changes in this record

- **Closed by run: `serde_jcs` against the `sec-*` canonical fixtures** (STATUS item from
  E5, owner F4 #53; `11-risks.md` row 64). `core/tests/conformance.rs`
  `conformance_fixtures` runs `run_canonical` (L854-L876, called at L930-L935) on every
  fixture that has an `expected.canonical`.
  - There are 10, all `sec-*`:
    - `SEC-SIG-010.p01` to `p04` (`p03` is UTF-16 member order, `p04` escapes);
    - `SEC-SIG-011.p01` (unknown member, Unicode);
    - `SEC-SIG-013.p01` and `p02` (numbers);
    - `SEC-KEY-041.p01`, `SEC-PRS-001.p01` and `SEC-RCT-001.p01`.
  - They run through `core::canonical::signed_text`. It writes every name, string and
    number with `serde_jcs` `0.2.0`, and sorts members itself by UTF-16 code units
    (`core/src/canonical.rs` L1-L14, L83-L130).
  - The test asserts that the count is above zero (L938-L985).
    `cargo test -p oac-core --test conformance` printed `"canonical": 10` and passed. CI
    runs it on all three OSes.
  - Row 64 is marked CLOSED in its disposition column. Its STATUS bullet is removed and
    replaced by a dated removal note that records the verification.
- **Reassigned: presence records over the transport** (`11-risks.md` row 61).
  - The owner was "the transport binding, F6/F10, E5", and all three are closed for
    Stage 3.
  - The in-memory transport carries presence under the transport contract suite, but the
    v0.1 transport is Zenoh.
  - New owners: G2 (#63, Zenoh presence via liveliness tokens) for carriage, and G3 (#64,
    local and LAN mode security) for SC-DLV-066 across installs.
- **Reassigned: C4 hook-to-shim pairing** (`11-risks.md` row 58).
  - The owner was "Epic F Claude adapter work and G9", but the Claude adapter is Epic G
    work.
  - New owners: G9 (#70, authenticated local IPC, where the native-signal pairing key
    already sits since #331) and G5 (#66, session registration from hook input).
- **New: which connections Codex sends a thread's notifications to** (`11-risks.md`
  row 70, STATUS entry, owner G6 #67). The S3 capture recorded this, but it was in no
  ledger.
- **Main's text kept.** Rows 62 and 65-69 keep main's text (#344, #346).

### Continuing items (none blocks this exit)

| Item | What it is | Status |
|---|---|---|
| #59 | F10, real-adapter and Zenoh halves | Open until Epic G (§4). |
| #347 | The contract suite does not tell `handoff-failed` from not-now | Open, `stage:3-core`. It affects no Gate S3 criterion (§4). Fix before the real adapters run the suite. |
| #68 note | `oacFake/thread/setArchived` sends no `thread/status/changed` / `thread/archived`; real archiving does (S3 L1024-L1029) | For G7: decide and test how the Codex adapter treats an unprompted `thread/archived`, against the recorded frames (https://github.com/RossGraeber/OAC/issues/68#issuecomment-6053040118). |
| #345 | The herdr driver self-test runs in no CI workflow | Open. Test-tooling coverage; it does not change criterion 1 or 2. |
| #308 | Editorial amendments to frozen text, batched for the next minor version | Open. Items from F5 (PR #316 review B1(c)) and from the #343 capture (§3, both comments). Lands under E7 §7. |
| #343 | Stage 1 capture for criterion 5 | Its last box is this PR (Refs #343). Closing it is left for the lead. |
| 10 gated security placeholders | `#[ignore = "GATED on #N"]` in `tests/security/` | Owned by #65, #68, #69, #70 and #74. The threat-map check keeps each one gated and failing if run. |
| Native-signal pairing key | Which OS facility yields the key (`session-channels.md` §6.7) | UNVERIFIED in STATUS (#331 entry), owned by G9 (#70). Until then every native signal fails closed. |
| Codex refusals not recorded | Subagent and no-queue-service refusals; whether any refusal means "not now" | Codex behaviour only, no longer modelled by the fake. `11-risks.md` row 65, STATUS #274 entry, owner G7 (#68). |
| #224 | Codex `turn/start` steers a running turn | Open, owned by backlog G7. The decision is in the frozen text. The fake Codex records the steering (README, G5 L58), and the contract suite asserts never-steer ([SEC-AUZ-022]). |
| Required CI checks | Branch protection on `main` | Not done: `main` has no protection and no rulesets (GitHub API, 2026-10-07). The candidate list is in PR #330. Which checks become required is the lead's decision. |
| #131, #124, #303 | herdr test tooling (Epic K, #123) | Open. Cannot change a gate verdict. |
| #175, #176 | Beacon L10 and L11 (Epic L, #165) | Blocked on Stages 5 and 6. |

Other open UNVERIFIED items stay in `docs/planning/STATUS.md` "Open UNVERIFIED items" and
`docs/planning/v0.1/11-risks.md`.

## 8. Checks run on this change

On Windows, on this branch after merging `090d2a7`:

- **Tests.**
  - `cargo test --workspace --offline`: 446 passed, 0 failed, 12 ignored (the windows
    count; §2, criterion 1).
  - `cargo test -p oac-core --test conformance -- --nocapture`: `"canonical": 10`, pass.
- **Security and fakes.**
  - `node tests/security/check-compiled-tests.mjs`: CLEAN.
  - `node tests/fakes/codex-app-server/self-test.mjs`: 29/29.
- **Conformance.** `node tests/protocol/runner/run.mjs`: 527/527 pass, index checks
  clean. `--self-test`: PASS.
- **Lints.**
  - `node scripts/check-crate-deps.mjs`: CLEAN. `--self-test`: 62/62.
  - `node scripts/check-licenses.mjs`: CLEAN.
  - `node scripts/check-containment.mjs`: CLEAN. `--self-test`: 32/32.
  - `node scripts/check-workflows.mjs`: CLEAN. `--self-test`: 101/101.
  - `node scripts/check-herdr-containment.mjs`: CLEAN, 0 violations across all 10
    targets.
- **Fixtures.** `node scripts/check-fixture-manifest.mjs`: 231/231 entries match, with the
  known #216 `VERSION WARNING` lines, none a gate.
- **Skills.**
  - `node scripts/check-skills.mjs`: all 15 skills within budget.
  - `node scripts/sync-agents-skills.mjs --check`: matches.
- **Boundary lint.** `boundary-lint.yml` checks 3, 8, 1-2 with the spec neutral-vocabulary
  group, and 11: each `rg` pattern and path set from the workflow, run on Windows
  (ripgrep 15.2.0), all zero hits. The workflow's shell wrappers themselves run in CI on
  this PR.

---

## Boundary and evidence pass

- No boundary in `oac-boundaries` is touched. This record decides no provider integration,
  credential use, transport vocabulary or model routing, and edits no neutral spec text.
- **Every claim is cited.** Each one cites a repo file and line at `090d2a7` (or an
  earlier commit where it says so), a check run on it, a CI run, or an issue or PR.
- **External sources** are cited as the fake's README, the herdr run records and the
  STATUS #343 and #346 entries cite them: Codex `rust-v0.160.0` and `rust-v0.161.0`, and
  the live captures. No new provider claim is made.
- **Ledger.** One UNVERIFIED item is closed by run, with its verification recorded here,
  in STATUS and in `11-risks.md` row 64. One is added: row 70, in both ledgers
  (`oac-evidence` §5).
