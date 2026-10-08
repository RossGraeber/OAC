# F-6 — Stage 3 exit: Gate S3 and the Stage 4 go/no-go

- **Date:** 2026-10-07; re-run 2026-10-08 against the #343 capture (PR #344).
- **Issue:** #6 (Epic F, Stage 3, milestone M4). Stage 3 has no exit task of its own in
  the backlog, so this record is named after the epic and its issue, as
  `E-5-stage2-exit.md` is. This PR is #343's last box: "Re-run the Stage 3 exit record".
- **Gate:** Gate S3 (`docs/planning/v0.1/10-stages.md` §7, L631-L651).
- **Status:** **Not decided: the lead's decision (§5).** The re-run gives this result:
  - criteria 1-4 hold (§2);
  - criterion 4's fake Codex half now rests on recorded behaviour;
  - **criterion 5 does not hold as worded.** The #343 capture recorded every F-1 row it
    could reach. Three fake Codex refusals stay source-only, because no documented client
    request triggers them (§3).

  The plan has no residual path for criterion 5. Going ahead needs the lead to depart from
  §7 on purpose, recorded as a dated note there. Until the lead decides, Gate S3 is not
  met and Stage 4 stays blocked. Every exit artifact exists (§1).
- **Owns:** its own text, the STATUS top entry for this exit, the Gate S3 sentences in the
  STATUS "Current stage" cells, the `10-stages.md` §7 "Current verdict" paragraph, and
  the ledger changes in §7: `11-risks.md` rows 64 and 70, plus three STATUS "Open
  UNVERIFIED items" entries and one new entry. It changes no file under `spec/` (frozen,
  E7 §7) and no gate verdict, pin or ADR text.
- **Skills:** `oac`, `oac-evidence`, `oac-boundaries`.

This record does not restate the tests. Each crate's own documentation and each task's PR
stay authoritative for what that code does. This file adds the Gate S3 checklist re-run on
current `main`, the exit-artifact inventory, the Epic F close-out, the carry-overs and the
Stage 4 entry check.

Line citations are to `main` at `4642b4f` (the PR #344 merge, 2026-10-08) unless a line
says otherwise. Between `5877b39` (the first run of this record) and `4642b4f`, nothing
changed under `.github/`, `core/`, `cli/`, `adapters/`, `transports/`, `spec/`, `scripts/`,
`tests/protocol/` or `tests/security/`. The changes are in the fakes, the fixtures, the
herdr tooling and the planning files. Every check in §2 and §8 was re-run on this branch
after merging `4642b4f`.

---

## 1. Stage 3 exit artifacts (`10-stages.md` §7 "Exit artifacts", L619-L629)

| Artifact | Where | Evidence |
|---|---|---|
| A CI pipeline whose default tier is green under the CI-default rule of `09-test-strategy.md` §3 (F12) | `.github/workflows/ci.yml` (jobs `test (<os>)`, `crate-deps (<os>)`, `licenses (<os>)` on ubuntu, windows and macos); `.github/workflows/boundary-lint.yml` (jobs `boundary-lint`, `fixture-manifest`, `containment`, `workflow-policy`, `agents-skills-sync`) | #61 closed by PR #330 (merge `9bba8b0`). On the `4642b4f` push, `ci` run 37724190562 and `boundary-lint` run 37724190502 are `success`, all 14 jobs. |
| The fake Claude and fake Codex endpoints, loaded from the Stage 1 fixtures (F8, F9) | `tests/fakes/claude/` (`oac-fake-claude`), `tests/fakes/codex-app-server/` (Node) | #57 closed by PR #319, #58 by PR #318, both aligned with the #343 capture by PR #344. Both load `docs/planning/gates/fixtures/` in place (`tests/fakes/claude/src/evidence.rs`; `tests/fakes/codex-app-server/lib/fixtures.mjs`). Three fake Codex refusals are still source-only: criterion 5 below. |
| The adapter and transport contract suites (F10), with the named `contract/adapter/no-polling` test | `tests/protocol/contract/adapter/` (`oac-contract-adapter`), `tests/protocol/contract/transport/` (`oac-contract-transport`) | PR #323 (merge `9292674`), with follow-ups PR #336 (#324) and PR #341 (#340). The named check is `tests/protocol/contract/adapter/src/lib.rs` L767. #59 stays open (§4). |
| The security suite against the fakes (F11) | `tests/security/` (`oac-security-suite`) | #60 closed by PR #327, follow-up PR #336 (#329). `node tests/security/check-compiled-tests.mjs`: "every compiled test is mapped, every mapped test compiled and run as mapped: CLEAN". |
| The provider-integration tier, its own separately invoked target, existing but not run by default (F12) | `.github/workflows/herdr-provider-optin.yml`; `tests/integration/README.md` | Exists for the herdr scenarios: manual dispatch, or a push to main that changes `docs/planning/PINS.md`, on operator-owned runners (`herdr-provider-optin.yml` L9-L14). PR #344 changed PINS.md, so the `4642b4f` push queued run 37724190520, which is `queued` on the operator-owned runner and not part of this check. Provider tests of OAC's own adapters cannot exist before Epic G; they are pending with G4 (#65) and H1 (#73) (`tests/integration/README.md` L30-L37). |

**The executable demonstration** (`10-stages.md` §7, L611-L617) runs, spread over named
tests rather than one. `cargo test --workspace` reaches green with nothing opt-in:

- compose, sign, publish over the in-memory transport, authorize in `core/`, deliver to the
  fake Claude endpoint and the fake Codex app-server, reply and correlate:
  `transports/memory/tests/pipelines.rs` `claude_and_codex_fakes_exchange_through_the_core_pipelines`
  (L741). The same run rejects an uncorrelated message without a grant as `rejected` /
  `unauthorized`. Its Codex leg reaches session input through a `thread/queue/add` to an
  idle thread (L457-L458, L644-L645), which is now recorded (F-1 row i, §3);
- replay: `tests/security/tests/replay.rs` `row04_copy_outside_the_replay_window_is_rejected`
  (L19) and the rest of row 4;
- duplicate: `replay.rs` `row04_replay_inside_the_window_is_a_duplicate_handed_off_once`
  (L114);
- unauthorized routing: `tests/security/tests/routing.rs`
  `row02_unauthorized_send_through_the_composed_pipeline` (L317) and the rest of rows 2, 7
  and 14.

The core send and receive pipelines these tests drive are #313 (PR #326), a Stage 3
follow-up that no F task named.

## 2. Gate S3 acceptance criteria (`10-stages.md` §7, L631-L643)

1. **The default test run is green and touches no live provider, no API key, and no
   network beyond loopback: holds.**
   - Green: the `ci` run on `4642b4f` passes `test (ubuntu-latest)`, `test (windows-latest)`
     and `test (macos-latest)`. These are per-OS totals over every `test result` line of
     `cargo test --workspace` (passed, failed, ignored):
     - ubuntu: 451, 0, 13;
     - macos: 451, 0, 12;
     - windows: 446, 0, 12.

     One test more than at `5877b39` (ubuntu 450, 0, 13; macos 450, 0, 12; windows 445,
     0, 12): `later_tool_calls_continue_the_recorded_id_sequence`, from PR #344. A local
     run on Windows on this branch (rustc `1.98.1`, Node `v25.2.1`, `--offline`) matches
     the windows job: 446, 0, 12.
   - No network beyond loopback: every cargo step after `cargo fetch` runs with
     `CARGO_NET_OFFLINE` (`ci.yml` L109-L133). On ubuntu the test steps run under
     `scripts/loopback-only.sh` (`ci.yml` L119-L133). Its probe on the `4642b4f` run
     printed "loopback connects; 192.0.2.1:443 ENETUNREACH; 1.1.1.1:443 ENETUNREACH;
     inherited fds closed". The only networked step is `cargo fetch` of the toolchain pin
     and `Cargo.lock`, before any test (`ci.yml` L22-L23, L105-L107).
   - No live provider and no API key: the only processes the Rust tests start are `node`
     running the fake Codex app-server (`tests/protocol/contract/adapter/src/codex.rs`
     L166), `sh`, `cargo`, and the platform `kill`/`tasklist`/`taskkill` used to reap it. No
     harness binary is started. `scripts/check-workflows.mjs` (job `workflow-policy`)
     fails a default-tier workflow that names a secret, a provider credential, an
     `OAC_TEST_*` switch, an `--ignored` run, a self-hosted runner or a harness CLI install
     (L55, L130). It reads "7 workflow(s), 0 local action(s), 0 violation(s)".
   - Residue, already recorded by PR #330: windows and macos have no runtime sandbox, so
     the rule there rests on the offline cargo setting, the fakes binding loopback only and
     the static lint (`ci.yml` L29-L33). A network namespace does not scope Unix-domain
     sockets (`scripts/loopback-only.sh` header).
2. **A plain default test invocation never triggers a provider-integration test: holds.**
   The ignored tests are the only opt-in ones: 12 on windows and macos, 13 on ubuntu.
   - 10 security placeholders `#[ignore = "GATED on #N ..."]` whose bodies fail if run
     (`tests/security/tests/provenance.rs` L365, L375, L386, L397; `never_steer.rs` L166,
     L176, L188; `local_ipc.rs` L29, L37, L46);
   - the real credential-store test (`cli/src/keystore.rs` L673), which also needs
     `OAC_TEST_REAL_KEYRING=1` and runs in `keystore-optin.yml`. A second one, L713, is
     compiled on Linux only (`#[cfg(target_os = "linux")]`, L711): the 13th on ubuntu;
   - the full-scale record churn test (`core/src/authorization.rs` L3115), in
     `scale-optin.yml`.

   None is a provider test. The herdr provider scenarios run only through
   `herdr-provider-optin.yml`, which never starts from a pull request. The herdr driver's
   own self-test (`node tools/herdr/run.mjs --self-test`) runs in no CI workflow at all
   (#345, open). That is a coverage gap in the test tooling, not a provider test in the
   default run.
3. **The dependency-direction lint passes: holds.** `node scripts/check-crate-deps.mjs`:
   "crate dependency direction: CLEAN (10 workspace members, 265 packages)"; `--self-test`:
   62/62. The rules are in the script header (L7-L44): `core/` reaches nothing in-repo,
   `adapters/*` reach `core/` only (never another adapter or a transport), and the `zenoh`
   crates are reachable only from `transports/zenoh` and `cli/`. The `crate-deps (<os>)`
   jobs on `4642b4f` also run the mutation test, all `success`.
4. **The `contract/adapter/no-polling` assertion passes against both fakes, in the
   call-class shape of `09-test-strategy.md` §5: holds.**
   - Shape: the check compares establishment calls before and after three deliveries
     (flat), requires zero message-fetch calls between arrivals and no inbox-style surface
     offered, and requires each message to become session input as pushed
     (`tests/protocol/contract/adapter/src/lib.rs` L759-L803). That is §5's two classes
     (L218-L234), not call-count growth.
   - Both fakes: `tests/protocol/contract/adapter/tests/stand_in.rs`
     `the_channel_stand_in_passes_under_both_mid_turn_release_settings` (fake Claude) and
     `the_queue_stand_in_passes_against_the_fake_codex_app_server` (fake Codex), both `ok`
     on all three OSes in the `4642b4f` run and locally.
     `the_suite_catches_each_planted_channel_breach` and
     `the_suite_catches_each_planted_queue_breach` show the suite fails a breach.
   - The fake Codex half: the check hands three messages to an **idle** session and
     requires each to become session input (L760-L797). `CodexFake::drain` only completes
     a turn already running (`tests/protocol/contract/adapter/src/codex.rs` L387-L401).
     So each message becomes input because the fake starts a turn when an add reaches an
     idle, loaded thread (`tests/fakes/codex-app-server/lib/model.mjs` L536-L538, L544-L563).
   - That behaviour is now recorded (F-1 row i). Live Codex `0.161.0` did the same on the
     TUI's own idle thread and on a client thread, with no `turn/start` sent
     (`docs/planning/gates/fixtures/s3-codex-capture/transcript-2026-10-07-0.161.0-herdr.jsonl`
     L57-L73, L90, L866-L873; `docs/planning/gates/herdr-runs/S3-codex-2026-10-07.md`
     row 1).
   - The fake also now sends the recorded frame order: the second `thread/queue/changed`
     before the response. The self-test checks it ("queue: an add to an idle, loaded thread
     starts a turn at once, frames in the recorded order (S3 capture)",
     `tests/fakes/codex-app-server/self-test.mjs` L295).
   - What runs the suite is two test-only stand-ins, not adapters: no crate implements
     `ProviderAdapter` yet. `tests/real_adapters.rs` `no_real_adapter_implements_the_trait_yet`
     (L101) fails as soon as one does, so the suite must be wired to it then (§4).
5. **Every fake's behaviour is traceable to a recorded Stage 1 fixture; no fake behaviour
   is invented from the spec: does not hold as worded.** The #343 capture closed eleven of
   F-1's twelve rows and most of the twelfth. Three fake Codex refusals stay source-only
   (§3).
   - **Fake Claude.** Every row of its behaviour table cites a fixture
     (`tests/fakes/claude/src/lib.rs` L20-L32). The table now includes the later tool-call
     ids and the `toolu_` form, both from the 2026-10-07 G1 herdr capture at Claude Code
     `2.1.285`.
     - One recorded part is older: `MidTurnRelease::AllAtBoundary` (L139-L141) comes from
       the original G1 run at `2.1.282`
       (`docs/planning/gates/fixtures/g1-claude-wake/transcript.jsonl`). It is recorded, at
       that earlier version.
     - Where no fixture shows what Claude Code does, the fake halts or refuses (L34-L84).
     - The one first-party document it cites (no acknowledgement) sits beside D6 lines
       12-13.
   - **Fake Codex.** The "Recorded behaviours" table cites a D6, G2, G5 or S3 fixture on
     every row (`tests/fakes/codex-app-server/README.md` L96-L122).
     - Every other unrecorded method gets the fake's own `-32099` `not-modelled` error,
       never an invented Codex frame (L167-L201).
     - `node tests/fakes/codex-app-server/self-test.mjs`: 25/25, including the D6
       attempt-2 replay compared frame by frame, and the S3 cases for the idle add, the
       reload, the interrupted turn and each recorded refusal.
   - **Still source-only:** the README's "Source-only behaviours (runtime UNVERIFIED)"
     table (L144-L165), now one row of three refusals (§3).

**Go/no-go (`10-stages.md` §7, L645-L651).** "Go when criteria 1-5 hold." Criteria 1-4
hold. Criterion 5 does not hold literally, so as the plan is written the verdict is
**no-go on criterion 5**. The plan's remedy is a Stage 1 fixture capture, and #343 ran it.
What remains is §5's decision.

## 3. Finding F-1, re-run against the #343 capture

Fixture paths are under `docs/planning/gates/fixtures/`. "S3" is
`s3-codex-capture/transcript-2026-10-07-0.161.0-herdr.jsonl`
(`docs/planning/gates/herdr-runs/S3-codex-2026-10-07.md`, Codex `0.161.0`). "S3-QI" is
`s3-codex-capture/transcript-2026-10-08-0.161.0-queued-interrupt-herdr.jsonl`
(`S3-codex-2026-10-08.md`). "G1-H" is
`g1-claude-wake/transcript-2026-10-07-2.1.285-herdr.jsonl`
(`G1-2026-10-07.md`, Claude Code `2.1.285`). README lines are
`tests/fakes/codex-app-server/README.md` at `4642b4f`.

| # | Behaviour | Now | Fixture (README row) | Ledger |
|---|---|---|---|---|
| a | `thread/queue/add` without `capabilities.experimentalApi` is refused `-32600` | recorded | S3 L99-L100 (L119); S3 record row 4 | `11-risks.md` row 67(a), CLOSED |
| b | A request before `initialize` is refused `-32600 "Not initialized"` | recorded | S3 L92-L93 (L118); row 3 | row 67(b), CLOSED |
| c | `thread/resume` of an unknown id gets the no-rollout error | recorded | S3 L106-L108 (L120); row 5 | row 67(d), CLOSED |
| d | The `thread/queue/add` refusals | **partly**: ephemeral, archived and unknown thread recorded; two subagent refusals and "no queue service" source-only | S3 L111-L121, L1030-L1032, L109-L110 (L121); rows 6, 7, 13. Source-only: README L165 | row 65, narrowed; row 68(c), CLOSED (REFUTED) |
| e | Nothing dispatches after an `interrupted` turn, for items already queued and for a later add | recorded | S3-QI L79-L170; S3 L907-L955 (L114); S3-QI record; S3 record row 10 | row 66(a), CONFIRMED |
| f | One queued item per idle, from the head | recorded | S3 L135-L139, L826-L865 (L113); row 8 | rows 62 and 68(b), CLOSED |
| g | The frames of an `interrupted` turn | recorded | S3 L899-L907; S3-QI L77-L90 (L115); row 9 | row 67(c), CLOSED |
| h | An extra member in `thread/queue/add` is accepted and ignored, and flagged in the call log (`lib/model.mjs` L249-L260) | recorded | S3 L866-L873 (L117); row 2 | row 66(d), CONFIRMED |
| i | An add to a loaded, idle thread whose last turn completed starts a turn at once | recorded | S3 L57-L73, L90, L866-L873 (L107); row 1 | row 68(a), CLOSED |
| j | An add to an unloaded thread waits, and loading the thread dispatches it | recorded | S3 L956-L1019 (L116); rows 11-12 | row 66(c), CONFIRMED |
| k | Fake Claude `tools/call` ids 3, 4, ... with `progressToken` equal to the id | recorded | G1-H L14, L16, L18 (fake Claude `lib.rs` L31); `tests/fakes/claude/tests/replay.rs` `later_tool_calls_continue_the_recorded_id_sequence` (L425) | row 69(a), CLOSED |
| l | Fake Claude's synthetic `claudecode/toolUseId` | form recorded: `toolu_01` + 22 letters and digits, as all 17 recorded ids are (G1-H L14, L16, L18 and every other Claude fixture; `lib.rs` L32, L71-L75). The fake writes `toolu_01OacFake<15 digits>` (L605) | row 69(b), CLOSED |

**Row l is now covered.** The id is the model's, so no recording can fix its value. The
fake now writes the recorded form, so an adapter checking the shape sees what Claude Code
sends. Fixed-form fresh values are how the fake Codex treats ids and times too ("Ids are
fresh UUIDv7 strings and times are the current clock", README L209-L210). The contract
suite's leak check still matches both the `toolu_` prefix and every value sent
(`tests/protocol/contract/adapter/src/claude.rs` L472-L497).

**What stays source-only** (README L144-L165; `11-risks.md` row 65; STATUS #274 entry;
owner G7, #68):

- **A loaded multi-agent v2 subagent and an unloaded spawned subagent**, each refused
  `-32600`. The model spawns subagent threads through its multi-agent tools; no documented
  client request creates one (`thread/start` makes a top-level thread).
- **"No queue service"** (`-32600 "user message queue is unavailable"`). This is a daemon
  built without the queue extension; no client request removes it.

All three are reached only through the fake's own control methods (`oacFake/thread/create`
with `subagent`, and `oacFake/queue/setAvailable`, `lib/model.mjs` L612-L627). They are
exercised only by the fake's self-test (`self-test.mjs` "refusals: …", L409 onward). The
contract suites, the security suite and the demonstration never reach them: no file under
`tests/protocol/contract/`, `tests/security/`, `transports/` or `core/` names a subagent
or the queue service.

**Not a fake behaviour, so outside criterion 5.** Row 66(b), other daemon clients
reordering, updating or deleting a queued item, is still an open Codex fact (STATUS #274
entry). The fake models no queue edits at all. `thread/queue/{update,delete,reorder}`
answer `NOT_MODELLED` (README L174-L176), so nothing in the fake rests on it.

**Recorded, but not modelled by the fake** (README L177-L187; S3 record "Also recorded").
These are omissions, not invented behaviour:

- the daemon's queue watcher sends an extra `thread/queue/changed` about every 10 s, and
  the fake has no timer;
- live Codex sends a thread's notifications to some connections not subscribed to it,
  while the fake sends them to subscribers only.

Which connections Codex chooses is UNVERIFIED. It had no ledger entry, so this re-run adds
one (`11-risks.md` row 70, STATUS, owner G6 #67).

### Live/fake differences the capture found, and the fake now follows (PR #344)

README "What the S3 capture changed" (L130-L142); S3 record rows 1, 6, 9 and 12;
`tests/fakes/claude/src/lib.rs` L71-L75.

1. **Unknown-thread refusal.** An add to an unknown thread is
   `-32603 "failed to read thread: invalid thread-store request: no rollout found for
   thread id <id>"` (S3 L109-L110), not the `-32600 "thread not found: <id>"` the source
   suggested.
2. **Dispatch on load.** Loading an unloaded thread with `thread/resume` dispatches its
   queue (S3 L981-L1019). The fake used to leave it waiting forever.
3. **`thread/queue/changed` ordering.** An idle add's second `thread/queue/changed`
   precedes the response (S3 L65-L73).
4. **Interrupted-turn frames.** An interrupted turn's `turn/completed` has `items: []` and
   `itemsView: "notLoaded"`, and its agent message never gets an `item/completed`
   (S3 L899-L907; S3-QI L77-L90).
5. **The `toolu_` form.** The fake Claude's synthetic id now has the recorded form
   (row l).

**Spec amendments.** The recordings contradict or settle frozen `spec/bindings/mcp.md`
§8.2.1 text in five places: L964-L965, L998-L1001, L1016-L1029, L1051-L1067 and the §10
UNVERIFIED row. No normative MUST changes; [MCPB-CDX-002] to [MCPB-CDX-005] are
unaffected. They are batched for the next minor version in #308:

- https://github.com/RossGraeber/OAC/issues/308#issuecomment-6051204351;
- the caveat on the fifth refusal,
  https://github.com/RossGraeber/OAC/issues/308#issuecomment-6051738540;
- the STATUS #343 entry, "Amendments needed for the #308 batch".

No `spec/` file changes here.

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
| F8 fake Claude | #57 | closed | #319; #344 (the #343 capture) |
| F9 fake Codex | #58 | closed | #318; #344 (the #343 capture) |
| F10 contract suites | #59 | **open** | #323; #336 (#324); #341 (#340) |
| F11 security suite | #60 | closed | #327; #336 (#329) |
| F12 default CI tier | #61 | closed | #330; #336 (#332) |

Follow-ups labelled `stage:3-core`, all closed: #305 (PR #309, landed just before Stage 3
opened), #313 (PR #326), #320, #325 and #328 (PR #334), #331 (PR #333), #335 (PR #337),
#324, #329 and #332 (PR #336), #338 (PR #339), #340 (PR #341). No other issue labelled
`stage:3-core` is open except #59 and #6. The Stage 1 capture for criterion 5 is #343
(label `stage:1-spikes`, PR #344). This PR is its last box. Nothing under `spec/` changed
in Stage 3: `git log 20482f1..4642b4f -- spec/` is empty.

**#59 (F10) stays open until Epic G.** Of its four acceptance items, the Stage 3 halves
are done:

- the adapter suite runs against both fakes; running it against the real adapters
  unchanged waits for G4-G8, because no adapter implements `ProviderAdapter` yet
  (`adapters/claude/src/lib.rs` and `adapters/codex/src/lib.rs` are the F1 scaffold);
- the transport suite runs unchanged against the in-memory transport under four media
  (`transports/memory/tests/contract.rs` `the_memory_transport_passes_the_contract_suite`,
  L145); running it against Zenoh waits for G1 (#62);
- the no-polling assertion is met (criterion 4);
- routing through core policy is checked statically against both scaffold crates
  (`tests/real_adapters.rs` `the_real_adapters_pass_the_static_routing_checks`, L39) and
  at run time against the stand-ins.

That matches what Stage 4's entry asks of the suites: "contract suites exist and pass
against fakes and the in-memory transport" (`10-stages.md` §8, L661-L662). The open half
is Stage 4's own executable demonstration 1 (L711-L716). PR #323 used "Refs #59" for this
reason.

The task checkboxes in #6's body, and closing #6, are left for the lead. Epic F stays
open.

## 5. The lead's decision, and sign-off

Under the plan as written, a criterion 5 that does not hold is no-go (`10-stages.md` §7,
L645-L651). The plan's remedy, a Stage 1 fixture capture, has run (#343). It recorded
every fake behaviour that a documented client request can reach. The lead chooses:

- **Hold Gate S3 (no-go).** Gate S3 stays unmet until the three refusals are recorded, or
  until the fake stops modelling them. Recording them would take a setup the capture did
  not have: a model-spawned subagent thread, or a daemon without the queue extension.
  Alternatively, the fake could answer the three with its own `NOT_MODELLED` error instead
  of the source's refusals. No test outside the fake's self-test reaches them (§3). Either
  path is a new change; this record would then be re-run.
- **Depart from §7 on purpose (go).** Gate S3 is met with the three refusals as recorded
  residuals. This is not a criterion that holds. It is a departure from §7's wording,
  recorded as a dated note under `10-stages.md` §7 that names the three refusals, why no
  documented request reaches them, and their owner, G7 (#68). The ledger already carries
  them (`11-risks.md` row 65; STATUS #274 entry).

This record does not make that choice.

**Sign-off mechanics.** The change-control rule is E7 §7: the change, a version bump where
a frozen item changes, and the lead's approval of the PR, with no separate document. No
`spec/` item changes here, so no version bump is due.

The lead authors this PR, and GitHub does not let an author approve their own PR. As for
PR #276 (E-5 §2, L52-L55), **the merge is the lead's approval.** So the verdict goes into
the PR as a sign-off commit before the merge:

- **On go**, it sets **Status** to *Decided: go*, writes the verdict below, adds the §7
  departure note, and opens Stage 4 in STATUS and `10-stages.md`.
- **On no-go**, it sets **Status** to *Decided: no-go* and names the follow-up. Or the PR
  waits, or closes unmerged.

**Lead's verdict:** _pending; written by the sign-off commit._

## 6. Stage 4 go/no-go (`10-stages.md` §8, L655-L668)

**Not decided: blocked on Gate S3.** The other §8 entry conditions are already met:

- **G1 and G2 carry `PASS` or `PASS (FALLBACK TAKEN)`.** Both are `PASS` with no fallback
  (`docs/planning/decisions/D7-stage1-exit.md` §1). G4 (#65), G6 (#67) and G7 (#68) carry
  `gate:*` labels; each checks its verdict in STATUS before starting, per the `oac` router.
- **G3's verdict fixes the discovery mode.** G3 is `PASS` on the primary multicast path
  (D7 §1), so Stage 4 implements multicast scouting.

Stages 4-6 stay blocked.

## 7. Continuing items and ledger changes

### Ledger changes in this record

- **Closed by run: `serde_jcs` against the `sec-*` canonical fixtures** (STATUS item from
  E5, owner F4 #53; `11-risks.md` row 64). `core/tests/conformance.rs`
  `conformance_fixtures` runs `run_canonical` (L854-L876, called at L930-L935) on every
  fixture that has an `expected.canonical`.
  - There are 10, all `sec-*`: `SEC-SIG-010.p01` to `p04` (`p03` is UTF-16 member order,
    `p04` escapes), `SEC-SIG-011.p01` (unknown member, Unicode), `SEC-SIG-013.p01` and
    `p02` (numbers), `SEC-KEY-041.p01`, `SEC-PRS-001.p01` and `SEC-RCT-001.p01`.
  - They run through `core::canonical::signed_text`, which writes every name, string and
    number with `serde_jcs` `0.2.0` and sorts members itself by UTF-16 code units
    (`core/src/canonical.rs` L1-L14, L83-L130).
  - The test asserts that the count is above zero (L938-L985).
    `cargo test -p oac-core --test conformance` at `5877b39` printed `"canonical": 10` and
    passed; CI runs it on all three OSes.
  - So the core's output, `serde_jcs` included, equals the fixtures' expected values.
    Row 64 is marked CLOSED in its disposition column. Its STATUS bullet is removed and
    replaced by a dated removal note that records the verification, as #344 did for the
    entries it closed.
- **Reassigned: presence records over the transport** (`11-risks.md` row 61). The owner
  was "the transport binding, F6/F10, E5", and all three are closed for Stage 3. The
  in-memory transport carries presence under the transport contract suite, but the v0.1
  transport is Zenoh. New owners: G2 (#63, Zenoh presence via liveliness tokens) for
  carriage, and G3 (#64, local and LAN mode security) for SC-DLV-066 across installs.
- **Reassigned: C4 hook-to-shim pairing** (`11-risks.md` row 58). The owner was "Epic F
  Claude adapter work and G9", but the Claude adapter is Epic G work. New owners: G9 (#70,
  authenticated local IPC, where the native-signal pairing key already sits since #331)
  and G5 (#66, session registration from hook input).
- **New: which connections Codex sends a thread's notifications to** (`11-risks.md` row
  70; STATUS entry; owner G6 #67). This was recorded by the S3 capture but was in no
  ledger (§3).
- **Rows 68-69** were added and closed by #344, with the text this record's first run
  proposed. This record keeps main's version of them and of rows 62 and 65-67.

### Continuing items (not decided by this record)

| Item | What it is | Status |
|---|---|---|
| §5 | Gate S3: hold, or depart from §7 for three source-only refusals | The lead's decision. |
| #59 | F10, real-adapter and Zenoh halves | Open until Epic G (§4). |
| #343 | Stage 1 capture for criterion 5 | Its last box is this PR (Refs #343). |
| #345 | The herdr driver self-test runs in no CI workflow | Open. Test-tooling coverage; it does not change criterion 1 or 2. |
| 10 gated security placeholders | `#[ignore = "GATED on #N"]` in `tests/security/` | Owned by #65, #68, #69, #70 and #74. The threat-map check keeps each one gated and failing if run. |
| Native-signal pairing key | Which OS facility yields the key (`session-channels.md` §6.7) | UNVERIFIED in STATUS (#331 entry), owned by G9 (#70). Until then every native signal fails closed. |
| #308 | Editorial amendments to frozen text, batched for the next minor version | Open. Items from Stage 3 (F5, PR #316 review B1(c)) and from the #343 capture (§3). Lands under E7 §7. |
| #224 | Codex `turn/start` steers a running turn | Open, owned by backlog G7. The decision is in the frozen text. The fake Codex records the steering (README, G5 L58), and the contract suite asserts never-steer ([SEC-AUZ-022]). |
| Required CI checks | Branch protection on `main` | Not done: `main` has no protection and no rulesets (GitHub API, 2026-10-07). The candidate list is in PR #330. Which checks become required is the lead's decision. |
| #131, #124, #303 | herdr test tooling (Epic K, #123) | Open. Cannot change a gate verdict. |
| #175, #176 | Beacon L10 and L11 (Epic L, #165) | Blocked on Stages 5 and 6. |

Other open UNVERIFIED items stay in `docs/planning/STATUS.md` "Open UNVERIFIED items" and
`docs/planning/v0.1/11-risks.md`.

## 8. Checks run on this change

On Windows, on this branch after merging `4642b4f`:

- `cargo test --workspace --offline`: 446 passed, 0 failed, 12 ignored (the windows count;
  §2, criterion 1). `cargo test -p oac-core --test conformance -- --nocapture` at
  `5877b39`: `"canonical": 10`, pass (the conformance test and `core/` are unchanged
  since).
- `node tests/security/check-compiled-tests.mjs`: CLEAN.
- `node tests/fakes/codex-app-server/self-test.mjs`: 25/25.
- `node tests/protocol/runner/run.mjs`: 527/527 pass, index checks clean. `--self-test`:
  PASS.
- `node scripts/check-crate-deps.mjs`: CLEAN. `--self-test`: 62/62.
- `node scripts/check-licenses.mjs`: CLEAN.
- `node scripts/check-containment.mjs`: CLEAN. `--self-test`: 32/32.
- `node scripts/check-workflows.mjs`: CLEAN. `--self-test`: 101/101.
- `node scripts/check-fixture-manifest.mjs`: 231/231 entries match, with the known
  #216 `VERSION WARNING` lines, none a gate.
- `node scripts/check-herdr-containment.mjs`: CLEAN, 0 violations across all 10 targets.
- `node scripts/check-skills.mjs`: all 15 skills within budget.
- `node scripts/sync-agents-skills.mjs --check`: matches.
- `boundary-lint.yml` checks 3, 8, 1-2 with the spec neutral-vocabulary group, and 11:
  each `rg` pattern and path set from the workflow, run on Windows (ripgrep 15.2.0), all
  zero hits. The workflow's shell wrappers themselves run in CI on this PR.

---

## Boundary and evidence pass

- No boundary in `oac-boundaries` is touched. This record decides no provider integration,
  credential use, transport vocabulary or model routing, and edits no neutral spec text.
- Every claim cites a repo file and line at `4642b4f` (or `5877b39` where it says so), a
  check run on it, a CI run, or an issue or PR. The external sources (Codex
  `rust-v0.160.0` and `rust-v0.161.0`, and the live captures) are cited as the fake's
  README, the herdr run records and the STATUS #343 entry cite them. No new provider claim
  is made.
- One UNVERIFIED item is closed by run, with its verification recorded here, in STATUS and
  in `11-risks.md` row 64. One is added, row 70, in both ledgers (`oac-evidence` §5).
