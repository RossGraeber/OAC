# F-6 — Stage 3 exit: Gate S3 and the Stage 4 go/no-go

- **Date:** 2026-10-07
- **Issue:** #6 (Epic F, Stage 3, milestone M4). Stage 3 has no exit task of its own in
  the backlog, so this record is named after the epic and its issue, as
  `E-5-stage2-exit.md` is.
- **Gate:** Gate S3 (`docs/planning/v0.1/10-stages.md` §7, L631-L651).
- **Status:** **Not decided: pending #343.** Criteria 1-3 hold. Criterion 4 holds, but
  its fake Codex half rests on a source-only fake behaviour (§2). **Criterion 5 is not
  met** (finding F-1, §3). Under `10-stages.md` §7 (L649-L651) a failing criterion 5
  sends the work back to Stage 1 for a fixture capture. That capture is #343. This record
  is re-run once #343 merges, and the sign-off commit then sets this line (§5). Every
  exit artifact exists (§1). Stage 4 stays blocked.
- **Owns:** its own text, the STATUS top entry for this exit, the Gate S3 sentences in the
  STATUS "Current stage" cells, the `10-stages.md` §7 "Current verdict" paragraph, and
  the ledger changes in §7 (`11-risks.md` rows 64, 68 and 69, and three STATUS "Open
  UNVERIFIED items" entries). It changes no file under `spec/` (frozen, E7 §7) and no
  gate verdict, pin or ADR text.
- **Skills:** `oac`, `oac-evidence`, `oac-boundaries`.

This record does not restate the tests. Each crate's own documentation and each task's PR
stay authoritative for what that code does. This file adds the Gate S3 checklist re-run on
current `main`, the exit-artifact inventory, the Epic F close-out, the carry-overs and the
Stage 4 entry check.

Line citations are to `main` at `5877b39` (the PR #339 merge, 2026-10-07) unless a line
says otherwise. Every check in §2 and §8 was re-run on that commit for this record.

---

## 1. Stage 3 exit artifacts (`10-stages.md` §7 "Exit artifacts", L619-L629)

| Artifact | Where | Evidence |
|---|---|---|
| A CI pipeline whose default tier is green under the CI-default rule of `09-test-strategy.md` §3 (F12) | `.github/workflows/ci.yml` (jobs `test (<os>)`, `crate-deps (<os>)`, `licenses (<os>)` on ubuntu, windows and macos); `.github/workflows/boundary-lint.yml` (jobs `boundary-lint`, `fixture-manifest`, `containment`, `workflow-policy`, `agents-skills-sync`) | #61 closed by PR #330 (merge `9bba8b0`). On the `5877b39` push, `ci` run 37689772727 and `boundary-lint` run 37689772849 are `success`, all 14 jobs. |
| The fake Claude and fake Codex endpoints, loaded from the Stage 1 fixtures (F8, F9) | `tests/fakes/claude/` (`oac-fake-claude`), `tests/fakes/codex-app-server/` (Node) | #57 closed by PR #319, #58 by PR #318. Both load `docs/planning/gates/fixtures/` in place (`tests/fakes/claude/src/evidence.rs` L31-L47; `tests/fakes/codex-app-server/lib/fixtures.mjs` L20-L25). Not every behaviour is from a fixture: criterion 5 below. |
| The adapter and transport contract suites (F10), with the named `contract/adapter/no-polling` test | `tests/protocol/contract/adapter/` (`oac-contract-adapter`), `tests/protocol/contract/transport/` (`oac-contract-transport`) | PR #323 (merge `9292674`), with follow-ups PR #336 (#324) and PR #341 (#340). The named check is `tests/protocol/contract/adapter/src/lib.rs` L767. #59 stays open (§4). |
| The security suite against the fakes (F11) | `tests/security/` (`oac-security-suite`) | #60 closed by PR #327, follow-up PR #336 (#329). `node tests/security/check-compiled-tests.mjs`: "every compiled test is mapped, every mapped test compiled and run as mapped: CLEAN". |
| The provider-integration tier, its own separately invoked target, existing but not run by default (F12) | `.github/workflows/herdr-provider-optin.yml`; `tests/integration/README.md` | Exists for the herdr scenarios: manual dispatch, or a push to main that changes `docs/planning/PINS.md`, on operator-owned runners (`herdr-provider-optin.yml` L9-L14). Provider tests of OAC's own adapters cannot exist before Epic G; they are pending with G4 (#65) and H1 (#73) (`tests/integration/README.md` L30-L37). |

**The executable demonstration** (`10-stages.md` §7, L611-L617) runs, spread over named
tests rather than one. `cargo test --workspace` reaches green with nothing opt-in:

- compose, sign, publish over the in-memory transport, authorize in `core/`, deliver to the
  fake Claude endpoint and the fake Codex app-server, reply and correlate:
  `transports/memory/tests/pipelines.rs` `claude_and_codex_fakes_exchange_through_the_core_pipelines`
  (L741). The same run rejects an uncorrelated message without a grant as `rejected` /
  `unauthorized`. Its Codex leg reaches session input through a `thread/queue/add` to an
  idle thread (L457-L458, L644-L645), which is F-1 row i;
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
     and `test (macos-latest)`. Per OS, summed over every `test result` line of
     `cargo test --workspace`: ubuntu 450 passed, 0 failed, 13 ignored; macos 450, 0, 12;
     windows 445, 0, 12. A local run on Windows at `5877b39` (rustc `1.98.1`, Node
     `v25.2.1`, `--offline`) matches the windows job: 445, 0, 12.
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
   `herdr-provider-optin.yml`, which never starts from a pull request.
3. **The dependency-direction lint passes: holds.** `node scripts/check-crate-deps.mjs`:
   "crate dependency direction: CLEAN (10 workspace members, 265 packages)"; `--self-test`:
   62/62. The rules are in the script header (L7-L44): `core/` reaches nothing in-repo,
   `adapters/*` reach `core/` only (never another adapter or a transport), and the `zenoh`
   crates are reachable only from `transports/zenoh` and `cli/`. The `crate-deps (<os>)`
   jobs on `5877b39` also run the mutation test, all `success`.
4. **The `contract/adapter/no-polling` assertion passes against both fakes, in the
   call-class shape of `09-test-strategy.md` §5: holds; its fake Codex half rests on
   F-1 row i.**
   - Shape: the check compares establishment calls before and after three deliveries
     (flat), requires zero message-fetch calls between arrivals and no inbox-style surface
     offered, and requires each message to become session input as pushed
     (`tests/protocol/contract/adapter/src/lib.rs` L759-L803). That is §5's two classes
     (L218-L234), not call-count growth.
   - Both fakes: `tests/protocol/contract/adapter/tests/stand_in.rs`
     `the_channel_stand_in_passes_under_both_mid_turn_release_settings` (fake Claude) and
     `the_queue_stand_in_passes_against_the_fake_codex_app_server` (fake Codex), both `ok`
     in CI and locally; `the_suite_catches_each_planted_channel_breach` and
     `the_suite_catches_each_planted_queue_breach` show the suite fails a breach.
   - The fake Codex half: the check hands three messages to an **idle** session and
     requires each to become session input (L760-L797). `CodexFake::drain` only completes
     a turn already running (`tests/protocol/contract/adapter/src/codex.rs` L387-L401).
     So each message becomes input only because the fake starts a turn when an add
     reaches an idle thread (`tests/fakes/codex-app-server/lib/model.mjs` L519,
     L524-L535). No fixture records that (F-1 row i). The criterion holds against the
     fake as built; what the fake does there is unrecorded.
   - What runs the suite is two test-only stand-ins, not adapters: no crate implements
     `ProviderAdapter` yet. `tests/real_adapters.rs` `no_real_adapter_implements_the_trait_yet`
     (L101) fails as soon as one does, so the suite must be wired to it then (§4).
5. **Every fake's behaviour is traceable to a recorded Stage 1 fixture; no fake behaviour
   is invented from the spec: not met (finding F-1, §3).**
   - Fake Claude: every row of its behaviour table cites D6, G1 Box C, G4 or G5 fixture
     lines at Claude Code `2.1.283` (`tests/fakes/claude/src/lib.rs` L14-L29). The
     exception is `MidTurnRelease::AllAtBoundary` (L131-L133), from the original G1 run at
     `2.1.282` (`docs/planning/gates/fixtures/g1-claude-wake/transcript.jsonl`): it is
     recorded too, at the earlier version. Where no fixture shows what Claude Code does,
     the fake halts or refuses (L31-L60). The one first-party document it cites (no
     acknowledgement) sits beside D6 lines 12-13. Two behaviours are the fake's own
     inference, and both reach the contract suite: F-1 rows k and l.
   - Fake Codex: the 13 "Recorded behaviours" rows each cite a D6, G2 or G5 fixture
     (`tests/fakes/codex-app-server/README.md` L95-L115). Every other unrecorded method
     gets the fake's own `-32099` `not-modelled` error, never an invented Codex frame
     (L137-L160). `node tests/fakes/codex-app-server/self-test.mjs`: 22/22, including the
     D6 attempt-2 replay compared frame by frame.
   - Not recorded: the eight "Source-only behaviours (runtime UNVERIFIED)" rows (README
     L117-L135), modelled from first-party Codex source (`github.com/openai/codex` tag
     `rust-v0.160.0`, commit `a956835d020762cb2b570053af06f643a11c0ecc`, retrieved
     2026-10-06), and two more the README does not list (rows i and j). None is invented
     from the neutral spec, but none traces to a recording.

**Go/no-go (`10-stages.md` §7, L645-L651).** "Go when criteria 1-5 hold." Criterion 5
does not, and the plan's response to a failing criterion 5 is a Stage 1 fixture capture.
That capture is #343. Gate S3 stays pending until #343 lands and this record is re-run.

## 3. Finding F-1: fake behaviours with no recorded fixture

Rows a-h are the fake Codex README's "Source-only behaviours" (L128-L135). Rows i and j
are fake Codex behaviours modelled from source that the README does not list. Rows k and
l are the fake Claude's inferences. Every row is in #343's scope.

| # | Behaviour (where) | Basis | Ledger |
|---|---|---|---|
| a | `thread/queue/add` without `capabilities.experimentalApi` is refused `-32600` (README L128) | Codex source lines | STATUS (#58 entry); `11-risks.md` row 67(a) |
| b | A request before `initialize` is refused `-32600 "Not initialized"` (L129) | source lines | row 67(b) |
| c | `thread/resume` of an unknown id gets the recorded no-rollout error (L130) | source lines | row 67(d) |
| d | The `thread/queue/add` refusals, in handler order (L131) | source functions (`add()`, `require_thread()`, `ensure_direct_input_allowed()`, `service()`) and lines | row 65, except `thread not found: <id>`, which row 65 leaves unclassified; that refusal is row 68(c) |
| e | Nothing dispatches after an `interrupted` turn (L132) | source functions (`on_thread_idle`, `wake_if_loaded`) | row 66(a) |
| f | One queued item per idle, from the head (L133) | source function (`dispatch_if_idle`) | row 68(b) (new); row 62 covers only order among several adds |
| g | `turn/completed` with status `interrupted` (L134) | source lines (the status value only) | row 67(c) |
| h | An extra member in `thread/queue/add` is accepted (`lib/model.mjs` L504) and flagged in the call log (L250-L257) (L135) | `spec/bindings/mcp.md` §8.2.1 (L999-L1000), itself an UNVERIFIED reading of source | row 66(d) |
| **i** | **A `thread/queue/add` to a loaded, idle thread whose last turn was not interrupted starts a turn at once** (`lib/model.mjs` L519, `wakeIfLoaded` and `dispatchHead` L524-L535; self-test L272) | source (`spec/bindings/mcp.md` L959-L965: "The G2 `busyqueue` step showed the busy case live") | row 68(a) (new) |
| j | A `thread/queue/add` to an unloaded thread waits (`wakeIfLoaded`, L525) | source | row 66(c) |
| k | Fake Claude `tools/call` ids after the first go on 3, 4, ... with `progressToken` equal to the id (`tests/fakes/claude/src/lib.rs` L70-L72) | inference from two calls, both id 2 (D6 line 13, G1 Box C line 24) | row 69(a) (new) |
| l | Fake Claude's synthetic `claudecode/toolUseId`, `toolu_fake<20 digits>` (L68-L69, L593) | `toolu_` prefix recorded (D6 line 13); suffix synthetic | row 69(b) (new) |

**Row i is load-bearing, not an edge path.** All 9 recorded `thread/queue/add` requests
in the 8 Codex transcripts were sent during a running turn: D6 `conn3-queue`; the four G2
transcripts (the MANIFEST says "queued while busy"); G5 `transcript-codex-2026-09-27` X4;
G5 `-2026-10-02-0.160.0-herdr` F.X4 and C.X4a, each after a `setup-turn`; and K8 X4. Only
dispatch at `turn/completed` is recorded. Yet both criterion 4's fake Codex half (§2) and
the demonstration's Codex leg (§1) reach session input through an add to an idle thread.

**Row k reaches the suite.** The contract suite calls `FakeClaude::call_tool` once per
request (`tests/protocol/contract/adapter/src/claude.rs` L360), so every request after the
first runs on an inferred id.

**Row l is fake behaviour, because the suite can observe it.** The adapter receives the
id in `_meta`. The suite collects every `claudecode/toolUseId` it sees, plus the `toolu_`
prefix, as native ids that must not leak (`claude.rs` L472-L497, used at
`lib.rs` L1398-L1412), and the stand-in records it (`stand_in.rs` L282-L285). The verdicts
do not depend on the suffix: the leak check matches the prefix and the exact values sent.
But an adapter that parsed or validated the id would see a form Claude Code was not
recorded sending. The case that it is unobservable does not hold, so it is listed.

**Ledger corrections.** The fake Codex README says every source-only row is in
`11-risks.md` rows 65-67 (L122-L123). That is wrong for row f, and for row d's `thread not
found` refusal. Rows i and j are not in the README at all. This record adds `11-risks.md`
rows 68-69 and the matching STATUS entry. The README is fake code's documentation and is
left to #343, which updates the fakes' READMEs.

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

That matches what Stage 4's entry asks of the suites: "contract suites exist and pass
against fakes and the in-memory transport" (`10-stages.md` §8, L661-L662). The open half
is Stage 4's own executable demonstration 1 (L711-L716). PR #323 used "Refs #59" for this
reason.

The task checkboxes in #6's body, and closing #6, are left for the lead. Epic F stays
open.

## 5. Sign-off

The change-control rule is E7 §7: the change, a version bump where a frozen item changes,
and the lead's approval of the PR, with no separate document. No `spec/` item changes
here, so no version bump is due.

The lead authors this PR, and GitHub does not let an author approve their own PR. As for
PR #276 (E-5 §2, L52-L55), **the merge is the lead's approval.**

So the verdict is recorded in the PR itself, not in a review:

1. #343 captures the fixtures and merges.
2. This record is re-run against them: §2 criterion 5, §3 and the ledger rows that #343
   resolves.
3. A sign-off commit on this PR sets **Status** to *Decided*, writes the verdict below and
   the matching STATUS and `10-stages.md` §7 text, and, on go, opens Stage 4.
4. The lead merges. That merge is the sign-off.

If #343 leaves a behaviour uncaptured (its third item allows for one that cannot be
triggered live), the re-run states it and criterion 5 is judged on that record.

**Lead's verdict:** _pending #343; written by the sign-off commit._

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
  fixture that has an `expected.canonical`. There are 10, all `sec-*`: `SEC-SIG-010.p01`
  to `p04` (`p03` is UTF-16 member order, `p04` escapes), `SEC-SIG-011.p01` (unknown
  member, Unicode), `SEC-SIG-013.p01` and `p02` (numbers), `SEC-KEY-041.p01`,
  `SEC-PRS-001.p01` and `SEC-RCT-001.p01`. They run through `core::canonical::signed_text`,
  which writes every name, string and number with `serde_jcs` `0.2.0` and sorts members
  itself by UTF-16 code units (`core/src/canonical.rs` L1-L14, L83-L130). The test
  asserts that the count is above zero (L938-L985).
  `cargo test -p oac-core --test conformance` at `5877b39` printed `"canonical": 10` and passed; CI runs it on all three OSes. So
  the core's output, `serde_jcs` included, equals the fixtures' expected values. Row 64 is
  marked CLOSED and the STATUS entry is closed in place.
- **Reassigned: presence records over the transport** (`11-risks.md` row 61). The owner
  was "the transport binding, F6/F10, E5", and all three are closed for Stage 3. The
  in-memory transport carries presence under the transport contract suite, but the v0.1
  transport is Zenoh. New owners: G2 (#63, Zenoh presence via liveliness tokens) for
  carriage, and G3 (#64, local and LAN mode security) for SC-DLV-066 across installs.
- **Reassigned: C4 hook-to-shim pairing** (`11-risks.md` row 58). The owner was "Epic F
  Claude adapter work and G9", but the Claude adapter is Epic G work. New owners: G9 (#70,
  authenticated local IPC, where the native-signal pairing key already sits since #331)
  and G5 (#66, session registration from hook input).
- **New: rows 68 and 69** for F-1 rows f, i and k-l, and for row d's `thread not found`
  refusal, owned by #343. There is one STATUS entry for both rows.

### Continuing items (not decided by this record)

| Item | What it is | Status |
|---|---|---|
| #343 | Stage 1 fixture capture for F-1 | Open. Gate S3 waits on it (§2, §5). |
| #59 | F10, real-adapter and Zenoh halves | Open until Epic G (§4). |
| 10 gated security placeholders | `#[ignore = "GATED on #N"]` in `tests/security/` | Owned by #65, #68, #69, #70 and #74. The threat-map check keeps each one gated and failing if run. |
| Native-signal pairing key | Which OS facility yields the key (`session-channels.md` §6.7) | UNVERIFIED in STATUS (#331 entry), owned by G9 (#70). Until then every native signal fails closed. |
| #308 | Editorial amendments to frozen text, batched for the next minor version | Open. Two items, one from Stage 3 (F5, PR #316 review B1(c)). Lands under E7 §7. |
| #224 | Codex `turn/start` steers a running turn | Open, owned by backlog G7. The decision is in the frozen text. The fake Codex records the steering (README, G5 L58), and the contract suite asserts never-steer ([SEC-AUZ-022]). |
| Required CI checks | Branch protection on `main` | Not done: `main` has no protection and no rulesets (GitHub API, 2026-10-07). The candidate list is in PR #330. Which checks become required is the lead's decision. |
| #131, #124, #303 | herdr test tooling (Epic K, #123) | Open. Cannot change a gate verdict. |
| #175, #176 | Beacon L10 and L11 (Epic L, #165) | Blocked on Stages 5 and 6. |

Other open UNVERIFIED items stay in `docs/planning/STATUS.md` "Open UNVERIFIED items" and
`docs/planning/v0.1/11-risks.md`.

## 8. Checks run on this change

On Windows at `5877b39`, then re-run on this branch:

- `cargo fmt --all --check`: clean. `cargo test --workspace --offline`: 445 passed, 0
  failed, 12 ignored (the windows count; §2, criterion 1).
  `cargo test -p oac-core --test conformance -- --nocapture`: `"canonical": 10`, pass.
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
  README cites it. No new provider claim is made. One UNVERIFIED item is closed by run,
  with its verification recorded here, in STATUS and in `11-risks.md` row 64. The new
  rows 68-69 are labelled and appear in both ledgers (`oac-evidence` §5).
