# F-6 — Stage 3 exit: Gate S3 and the Stage 4 go/no-go

- **Date:** 2026-10-07
- **Issue:** #6 (Epic F, Stage 3, milestone M4). Stage 3 has no exit task of its own in
  the backlog, so this record is named after the epic and its issue, as
  `E-5-stage2-exit.md` is.
- **Gate:** Gate S3 (`docs/planning/v0.1/10-stages.md` §7, L631-L651).
- **Status:** **Proposed: go, with one finding for the lead (F-1, criterion 5).** Criteria
  1-4 hold (§2). Criterion 5 holds for every recorded behaviour of both fakes, but not as
  worded for eight behaviours of the fake Codex app-server that are modelled from Codex
  source, not from a recorded fixture (§2, criterion 5; §3). Every exit artifact exists
  (§1). The lead's verdict is §5.
- **Owns:** its own text, the STATUS top entry for this exit, the STATUS "Current stage"
  cells, the `10-stages.md` §7 "Current verdict" paragraph and the `oac-implementation` §0
  stage-exit pointer. It changes no file under `spec/` (frozen, E7 §7) and no gate verdict,
  pin or ADR text.
- **Skills:** `oac`, `oac-evidence`, `oac-boundaries`.

This record does not restate the tests. Each crate's own documentation and each task's PR
stay authoritative for what that code does. This file adds the Gate S3 checklist re-run on
current `main`, the exit-artifact inventory, the Epic F close-out, the carry-overs and the
Stage 4 entry check.

Line citations are to `main` at `5877b39` (the PR #339 merge, 2026-10-07) unless a line
says otherwise. Every check in §2 and §7 was re-run on that commit for this record.

---

## 1. Stage 3 exit artifacts (`10-stages.md` §7 "Exit artifacts", L619-L629)

| Artifact | Where | Evidence |
|---|---|---|
| A CI pipeline whose default tier is green under the CI-default rule of `09-test-strategy.md` §3 (F12) | `.github/workflows/ci.yml` (jobs `test (<os>)`, `crate-deps (<os>)`, `licenses (<os>)` on ubuntu, windows and macos); `.github/workflows/boundary-lint.yml` (jobs `boundary-lint`, `fixture-manifest`, `containment`, `workflow-policy`, `agents-skills-sync`) | #61 closed by PR #330 (merge `9bba8b0`). On the `5877b39` push, `ci` run 37689772727 and `boundary-lint` run 37689772849 are `success`, all 14 jobs. |
| The fake Claude and fake Codex endpoints, loaded from the Stage 1 fixtures (F8, F9) | `tests/fakes/claude/` (`oac-fake-claude`), `tests/fakes/codex-app-server/` (Node) | #57 closed by PR #319, #58 by PR #318. Both load `docs/planning/gates/fixtures/` in place (`tests/fakes/claude/src/evidence.rs` L31-L47; `tests/fakes/codex-app-server/lib/fixtures.mjs` L20-L25). Criterion 5 below. |
| The adapter and transport contract suites (F10), with the named `contract/adapter/no-polling` test | `tests/protocol/contract/adapter/` (`oac-contract-adapter`), `tests/protocol/contract/transport/` (`oac-contract-transport`) | PR #323 (merge `9292674`), with follow-ups PR #336 (#324) and PR #341 (#340). The named check is `tests/protocol/contract/adapter/src/lib.rs` L767. #59 stays open (§4). |
| The security suite against the fakes (F11) | `tests/security/` (`oac-security-suite`) | #60 closed by PR #327, follow-up PR #336 (#329). `node tests/security/check-compiled-tests.mjs`: "every compiled test is mapped, every mapped test compiled and run as mapped: CLEAN". |
| The provider-integration tier, its own separately invoked target, existing but not run by default (F12) | `.github/workflows/herdr-provider-optin.yml`; `tests/integration/README.md` | Exists for the herdr scenarios: manual dispatch, or a push to main that changes `docs/planning/PINS.md`, on operator-owned runners (`herdr-provider-optin.yml` L9-L14). Provider tests of OAC's own adapters cannot exist before Epic G; they are pending with G4 (#65) and H1 (#73) (`tests/integration/README.md` L30-L37). |

**The executable demonstration** (`10-stages.md` §7, L611-L617) holds, spread over named
tests rather than one test. `cargo test --workspace` reaches green with nothing opt-in:

- compose, sign, publish over the in-memory transport, authorize in `core/`, deliver to the
  fake Claude endpoint and the fake Codex app-server, reply and correlate:
  `transports/memory/tests/pipelines.rs` `claude_and_codex_fakes_exchange_through_the_core_pipelines`
  (L741). The same run rejects an uncorrelated message without a grant as `rejected` /
  `unauthorized`;
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
   - Green: the `ci` run on `5877b39` passes `test (ubuntu-latest)`, `test (windows-latest)`
     and `test (macos-latest)`. Locally on Windows at `5877b39` (rustc `1.98.1`, Node
     `v25.2.1`), `cargo test --workspace --offline`: 445 passed, 0 failed, 12 ignored.
   - No network beyond loopback: every cargo step after `cargo fetch` runs with
     `CARGO_NET_OFFLINE` (`ci.yml` L109-L133). On ubuntu the test steps run under
     `scripts/loopback-only.sh` (`ci.yml` L119-L133). Its probe on the `5877b39` run
     printed "loopback connects; 192.0.2.1:443 ENETUNREACH; 1.1.1.1:443 ENETUNREACH;
     inherited fds closed; groups [1001]". The only networked step is `cargo fetch` of the
     toolchain pin and `Cargo.lock`, before any test (`ci.yml` L22-L23, L105-L107).
   - No live provider and no API key: the only processes the Rust tests start are `node`
     running the fake Codex app-server (`tests/protocol/contract/adapter/src/codex.rs`
     L166), `sh`, `cargo`, and the platform `kill`/`tasklist`/`taskkill` used to reap it. No
     harness binary is started. `scripts/check-workflows.mjs` (job `workflow-policy`)
     fails a default-tier workflow that names a secret, a provider credential, an
     `OAC_TEST_*` switch, an `--ignored` run, a self-hosted runner or a harness CLI install
     (L55, L130). It reads "7 workflow(s), 0 local action(s), 0 violation(s)" at `5877b39`.
   - Residue, already recorded by PR #330: windows and macos have no runtime sandbox, so
     the rule there rests on the offline cargo setting, the fakes binding loopback only and
     the static lint (`ci.yml` L29-L33). A network namespace does not scope Unix-domain
     sockets (`scripts/loopback-only.sh` header).
2. **A plain default test invocation never triggers a provider-integration test: holds.**
   The 12 ignored tests in the local run are the only opt-in ones:
   - 10 security placeholders `#[ignore = "GATED on #N ..."]` whose bodies fail if run
     (`tests/security/tests/provenance.rs` L365, L375, L386, L397; `never_steer.rs` L166,
     L176, L188; `local_ipc.rs` L29, L37, L46);
   - the real credential-store test (`cli/src/keystore.rs` L673; L713 is Linux-only), which
     also needs `OAC_TEST_REAL_KEYRING=1` and runs in `keystore-optin.yml`;
   - the full-scale record churn test (`core/src/authorization.rs` L3115), in
     `scale-optin.yml`.

   None is a provider test. The herdr provider scenarios run only through
   `herdr-provider-optin.yml`, which never starts from a pull request.
3. **The dependency-direction lint passes: holds.** `node scripts/check-crate-deps.mjs`:
   "crate dependency direction: CLEAN (10 workspace members, 265 packages)"; `--self-test`:
   62/62. The rules are in the script header (L7-L44): `core/` reaches nothing in-repo,
   `adapters/*` reach `core/` only (never another adapter or a transport), and the `zenoh`
   crates are reachable only from `transports/zenoh` and `cli/`. The `crate-deps (<os>)`
   jobs on `5877b39` also run the mutation test, all `success`.
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
     in the local run; `the_suite_catches_each_planted_channel_breach` and
     `the_suite_catches_each_planted_queue_breach` show the suite fails a breach.
   - What runs the suite is two test-only stand-ins, not adapters: no crate implements
     `ProviderAdapter` yet. `tests/real_adapters.rs` `no_real_adapter_implements_the_trait_yet`
     (L101) fails as soon as one does, so the suite must be wired to it then (§4).
5. **Every fake's behaviour is traceable to a recorded Stage 1 fixture; no fake behaviour
   is invented from the spec: holds for every recorded behaviour; not met as worded for
   eight fake Codex behaviours (finding F-1, §3).**
   - Fake Claude: every behaviour row cites D6, G1 Box C, G4 or G5 fixture lines, all at
     Claude Code `2.1.283` (`tests/fakes/claude/src/lib.rs` L14-L29). Where no fixture
     shows what Claude Code does, the fake halts or refuses instead of guessing (L31-L60).
     The one first-party document it cites (no acknowledgement) is beside D6 lines 12-13.
   - Fake Codex: the 13 "Recorded behaviours" rows each cite a D6, G2 or G5 fixture
     (`tests/fakes/codex-app-server/README.md` L95-L115). Every other unrecorded method
     gets the fake's own `-32099` `not-modelled` error, never an invented Codex frame
     (L137-L160). `node tests/fakes/codex-app-server/self-test.mjs`: 22/22, including the
     D6 attempt-2 replay compared frame by frame.
   - The exception is "Source-only behaviours (runtime UNVERIFIED)", README L117-L135:
     eight rows that no fixture records, modelled from first-party Codex source
     (`github.com/openai/codex` tag `rust-v0.160.0`, commit
     `a956835d020762cb2b570053af06f643a11c0ecc`, retrieved 2026-10-06). Seven cite source
     lines. The eighth (L135, an extra member in `thread/queue/add` is accepted and
     flagged) cites `spec/bindings/mcp.md` §8.2.1 (L999-L1000), whose own text is an
     UNVERIFIED reading of that source. None is invented from the neutral spec, but none is
     traceable to a recording.

**Go/no-go (`10-stages.md` §7, L645-L651).** Criteria 1 and 2, the no-go criteria, hold.
Criteria 3 and 4 hold. Criterion 5 is the lead's call (§5): its rule sends a failing
criterion 5 back to Stage 1 for a fixture capture.

## 3. Finding F-1: source-modelled fake Codex behaviours

| # | Behaviour (README row) | Ledger | Owner |
|---|---|---|---|
| a | `thread/queue/add` without `capabilities.experimentalApi` is refused `-32600` (L128) | STATUS "Open UNVERIFIED items" (#58 entry); `11-risks.md` row 67(a) | G6 (#67) |
| b | A request before `initialize` is refused `-32600 "Not initialized"` (L129) | row 67(b) | G7 (#68) |
| c | `thread/resume` of an unknown id gets the recorded no-rollout error (L130) | row 67(d) | G7 (#68) |
| d | The `thread/queue/add` refusals, in handler order (L131) | row 65 | G7 (#68) |
| e | Nothing dispatches after an `interrupted` turn (L132) | row 66(a) | G7 (#68) |
| f | One queued item per idle, from the head (L133) | README only; order among several adds is row 62 | G7 (#68) |
| g | `turn/completed` with status `interrupted` (L134) | row 67(c) | G7 (#68) |
| h | An extra member in `thread/queue/add` is accepted and flagged (L135) | row 66(d); `spec/bindings/mcp.md` §8.2.1 | G7 (#68) |

What bounds it:

- All eight are error, refusal or edge paths. The behaviour a passing adapter is held to
  (subscribe, queue while busy, dispatch at idle, never steer) is recorded.
- Row h never reaches a conforming adapter: [MCPB-CDX-005] forbids sending an extra
  member, and the call log flags one (`lib/model.mjs` L488-L504), so the suite fails an
  adapter that sends it.
- Each row is labelled UNVERIFIED in the fake's README section heading, and rows a-e, g
  and h are in `11-risks.md` (rows 65-67) and STATUS "Open UNVERIFIED items". The PR #318
  review accepted the source-only label. Row f has no ledger entry of its own: only the
  order among several adds is (row 62).

The two ways to settle it are in §5.

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
| F8 fake Claude | #57 | closed | #319 |
| F9 fake Codex | #58 | closed | #318 |
| F10 contract suites | #59 | **open** | #323; #336 (#324); #341 (#340) |
| F11 security suite | #60 | closed | #327; #336 (#329) |
| F12 default CI tier | #61 | closed | #330; #336 (#332) |

Follow-ups labelled `stage:3-core`, all closed: #305 (PR #309, landed just before Stage 3
opened), #313 (PR #326), #320, #325 and #328 (PR #334), #331 (PR #333), #335 (PR #337),
#324, #329 and #332 (PR #336), #338 (PR #339), #340 (PR #341). No other issue labelled
`stage:3-core` is open except #59 and #6. Nothing under `spec/` changed in Stage 3:
`git log 20482f1..5877b39 -- spec/` is empty.

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

That is what Stage 4's entry asks for: "contract suites exist and pass against fakes and
the in-memory transport" (`10-stages.md` §8, L661-L662). The open half is Stage 4's own
executable demonstration 1 (L711-L716). PR #323 used "Refs #59" for this reason.

The task checkboxes in #6's body, and closing #6, are left for the lead. This PR does
not close #6.

## 5. Lead's verdict

The lead's approval of this PR is the sign-off, under the same rule as the spec (E7 §7:
the change, a version bump where a frozen item changes, and the lead's approval; no
separate document). No `spec/` item changes here, so no version bump is due.

- **Approve** = **go**: Gate S3 is met with finding F-1 accepted as a residual. The eight
  behaviours stay UNVERIFIED, owned by G6 (#67) and G7 (#68), and each is captured or
  confirmed when those tasks run against a real app-server. Stage 3 exits and Stage 4
  (Epic G, #7, milestone M5) opens (§6).
- **Request changes** = **no-go on criterion 5**: the eight behaviours go back to Stage 1
  for a fixture capture (`10-stages.md` §7, L649-L651), and this record is re-run after
  it.

**Lead's verdict:** _pending; recorded by the lead's review of this PR._

## 6. Stage 4 go/no-go (`10-stages.md` §8, L655-L668)

On approval: **go.** Each §8 entry condition is met:

- **Gate S3 met (§7).** §2 above, with F-1 accepted.
- **G1 and G2 carry `PASS` or `PASS (FALLBACK TAKEN)`.** Both are `PASS` with no fallback
  (`docs/planning/decisions/D7-stage1-exit.md` §1). G4 (#65), G6 (#67) and G7 (#68) carry
  `gate:*` labels; each checks its verdict in STATUS before starting, per the `oac` router.
- **G3's verdict fixes the discovery mode.** G3 is `PASS` on the primary multicast path
  (D7 §1), so Stage 4 implements multicast scouting.

Stages 5 and 6 stay blocked, each behind its own gate. Stage 5 starts only after Gate S4.

## 7. Continuing items (not exit blockers)

None of these can change Gate S3. Each has an owner outside Stage 3's closed work.

| Item | What it is | Status |
|---|---|---|
| #59 | F10, real-adapter and Zenoh halves | Open until Epic G (§4). |
| 10 gated security placeholders | `#[ignore = "GATED on #N"]` in `tests/security/` | Owned by #65, #68, #69, #70 and #74. The threat-map check keeps each one gated and failing if run. |
| Native-signal pairing key | Which OS facility yields the key (`session-channels.md` §6.7) | UNVERIFIED in STATUS (#331 entry), owned by G9 (#70). Until then every native signal fails closed. |
| F-1 | Source-modelled fake Codex behaviours | §3, §5. |
| #308 | Editorial amendments to frozen text, batched for the next minor version | Open. Two items, one from Stage 3 (F5, PR #316 review B1(c)). Lands under E7 §7. |
| #224 | Codex `turn/start` steers a running turn | Open, owned by backlog G7. The decision is in the frozen text; the fake Codex records the steering (README, G5 L58) and the contract suite asserts never-steer ([SEC-AUZ-022]). |
| Required CI checks | Branch protection on `main` | Not done: `main` has no protection and no rulesets (GitHub API, 2026-10-07). The candidate list is in PR #330. Which checks become required is the lead's decision. |
| #131, #124, #303 | herdr test tooling (Epic K, #123) | Open. Cannot change a gate verdict. |
| #175, #176 | Beacon L10 and L11 (Epic L, #165) | Blocked on Stages 5 and 6. |

Open UNVERIFIED items stay in `docs/planning/STATUS.md` "Open UNVERIFIED items" and
`docs/planning/v0.1/11-risks.md`. This record closes none of them.

## 8. Checks run on this change

On Windows at `5877b39`, then re-run on this branch:

- `cargo fmt --all --check`: clean. `cargo test --workspace --offline`: 445 passed, 0
  failed, 12 ignored (§2, criterion 2).
- `node tests/security/check-compiled-tests.mjs`: CLEAN.
- `node tests/fakes/codex-app-server/self-test.mjs`: 22/22.
- `node tests/protocol/runner/run.mjs`: 527/527 pass, index checks clean. `--self-test`:
  PASS.
- `node scripts/check-crate-deps.mjs`: CLEAN. `--self-test`: 62/62.
- `node scripts/check-licenses.mjs`: CLEAN (265 packages, 89 headers).
- `node scripts/check-containment.mjs`: CLEAN. `--self-test`: 32/32.
- `node scripts/check-workflows.mjs`: CLEAN. `--self-test`: 101/101.
- `node scripts/check-fixture-manifest.mjs`: 224/224 entries match, with the known #216
  `VERSION WARNING` lines, none a gate.
- `node scripts/check-herdr-containment.mjs`: CLEAN, 0 violations across all 10 targets.
- `node scripts/check-skills.mjs`: all 15 skills within budget.
- `node scripts/sync-agents-skills.mjs --check`: matches.
- `boundary-lint.yml` checks 3, 8, 1-2 with the spec neutral-vocabulary group, and 11:
  each `rg` pattern and path set from the workflow, run on Windows (ripgrep 15.2.0), all
  zero hits (check 11 over 60 tracked files). The workflow's shell wrappers themselves
  run in CI on this PR.

---

## Boundary and evidence pass

- No boundary in `oac-boundaries` is touched. This record decides no provider integration,
  credential use, transport vocabulary or model routing, and edits no neutral spec text.
- Every claim cites a repo file and line at `5877b39`, a check run on it, a CI run, or an
  issue or PR. The one external source (Codex `rust-v0.160.0`) is cited as the fake's
  README cites it. There are no new provider claims and no new UNVERIFIED items.
