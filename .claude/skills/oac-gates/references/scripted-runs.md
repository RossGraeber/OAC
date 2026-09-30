# Scripted runs (herdr)

Source: Epic K #123 (non-goals; "Why this stays clear of ADR-001 boundary 4"). Backlog:
`docs/planning/backlog/06-tasks-K.json` K3-K5. Tool record:
`docs/planning/decisions/K1-herdr-evaluation.md`. This file states the rules the driver
already enforces (`tools/herdr/run.mjs`, `tools/herdr/lib/herdr.mjs`,
`tools/herdr/lib/manifest.mjs`, `tools/herdr/lib/g1-report.mjs`) and the rules that sit
outside the code. The evidence-store layout (the `Driver:` field, the `-herdr` fixture
suffix, `docs/planning/gates/herdr-runs/`, what a herdr pin move invalidates) is
`docs/planning/gates/README.md` "Scripted runs (herdr)". It is linked here, not restated.

A scripted run is a gate re-run in which herdr plays the operator's hands and eyes: it
starts the harness, types what the operator would type, and reads the screen. It never
replaces a supported interface, and it decides nothing.

**Who runs it.** An agent runs `node tools/herdr/run.mjs` locally; that is the default for
any live leg, and the point of herdr (operator decision, #187). The human attends only to
sign in to the harnesses, grant elevation, and accept operator-consent dialogs (below). No
live leg is "operator-typed only". Live runs are not in the default CI suite
(`oac-testing` §2).

## Status at K5

No scripted run of any gate has run live. K4 built the G1 scenario and tested it against
test doubles only. K1's live leg is NOT RUN. So no equivalence record exists, and every
scripted run is non-verdict-bearing. Current state: `docs/planning/STATUS.md` "Open
UNVERIFIED items" (the K1 and K4 entries).

## Three vocabularies — never mix them

| Vocabulary | Values | Set by | Means |
|---|---|---|---|
| Run outcome | `PASS` / `FAIL` / `NOT RUN` (exit 0 / 1 / 3) | `tools/herdr/run.mjs` | Whether the scenario ran as specified, to the end. Says nothing about any gate criterion |
| Criterion score | `equivalent` / `not equivalent` / `not evaluable` | `tools/herdr/lib/g1-report.mjs`, plus operator scores | One criterion of this run compared with the human-run baseline, in a `herdr-runs/` record |
| Gate verdict | `PASS` / `PASS (FALLBACK TAKEN)` / `FAIL` / `NOT RUN` | The gate procedure in `oac-gates` | Only in `docs/planning/gates/G<n>-result.md` |

A run outcome of `PASS` is never copied into a gate verdict or a criterion.

## Driver identity

Every scripted run names what drove it, in three places that must agree:

- **Run manifest** (`run-manifest.json`, schema version 1, written by `run.mjs`):
  `driver.entry`, `driver.commit` (git `HEAD` at run start), `driver.toolsHerdrDirty`
  (uncommitted changes under `tools/herdr/`), `driver.node`; `herdr.pinRow`,
  `herdr.pinnedTag`, `herdr.expectedVersionOutput`, `herdr.observedVersionOutput`,
  `herdr.config` (the run's own herdr config, with `version_check` and `manifest_check`
  off), `herdr.agentManifests`; `scenario.file` and `scenario.params`; `launch.argv` and
  `launch.herdrReportedArgv`.
- **Record and gate result:** the `Driver:` line, as `g1-report.mjs` renders it: the
  `herdr --version` output, the PINS.md `herdr (test tooling)` tag, `tools/herdr/run.mjs`,
  the scenario file, and the driver commit.
- **Fixture manifest:** the `driver` block (`herdr_version`, `driver_commit`,
  `run_manifest`) on every `-herdr` entry. `node scripts/check-fixture-manifest.mjs`
  checks the block against the git-tracked run manifest: outcome, clean `tools/herdr/`,
  commit, herdr version, and that the fixture is one of the run's written captures. It
  does not bind the fixture's bytes to the run, because the run manifest records no
  capture hash yet (K3-level follow-up, issue #140).

The driver refuses to start unless `herdr --version` equals the PINS.md pin. A run whose
`driver.toolsHerdrDirty` is not `false` (`true`, or `null` when git could not answer), or
whose `driver.commit` is `null`, cannot be reproduced. Its run id and outcome may be
listed as a finding in a later record. Its captures are never committed as fixtures: the
checker refuses the entries, and `g1-report.mjs --write` does not check this, so do not
`--write` such a run. It is never an equivalence record and never verdict-bearing.

**What the record cannot show.** The run manifest does not record which herdr executable
or which harness executable ran. `--herdr-bin` accepts any path, including the test double
`tools/herdr/test/fake-herdr.mjs`, and the recorded argv still starts with `herdr`. A
fake Claude Code on `PATH` also answers `claude --version`. A run against the test
doubles therefore produces a record, fixtures and `MANIFEST.json` entries that look like a
real run's and pass every mechanical check. Likewise, `acceptOrigin: human` in the run
manifest means only that the driver sent no keystroke to the dialog and the screen then
changed. It does not prove that a human pressed a key. A test double that accepts its
own dialog records the same thing. Until the driver records the executables it ran
(issue #140), these facts rest on the operator attestation below.

## Timebox

- The box is declared before the first herdr command. It is the scenario's
  `defaults.timeboxMs`, or `--timebox-ms` on the command line. A gate scenario's default
  equals the box of the human run it replays (`g1-claude-wake`: 45 minutes, Box C's box
  in `docs/planning/gates/G1-result.md`).
- The box covers the whole run, including any wait for the operator (`accept=human`).
  `timebox.budgetMs`, `start`, `end`, `elapsedMs` and `expired` record it.
- It is never extended. `run.mjs` stops the run at the budget plus a fixed 5-second grace
  for bounded commands already in flight. A longer box means a new run, with its own box
  declared before it starts.
- Every herdr wait carries an explicit `--timeout`. At `v0.9.1`, `agent read` and
  `agent send-keys` have no timeout option (K1 §5 item 3), so the driver bounds them with
  a process deadline.

## Timeout means NOT RUN

The run ends `NOT RUN`, never `FAIL` and never a pass, when:

- a herdr command times out (the driver's deadline, or herdr's `timeout` or
  `agent_prompt_stalled`);
- the timebox expires;
- the operator aborts (a signal);
- `herdr --version` differs from the pin, or the pin cannot be read;
- a scenario preflight stops the run. Example: G1's Claude Code version differs from the
  PINS.md last-observed version. That is a pin-move trigger, and nothing in PINS.md is
  edited.

A scenario that catches a timeout and returns normally is still `NOT RUN`. So is a
scenario that fails after a timeout. A `NOT RUN` run makes every criterion
`not evaluable`, and `g1-report.mjs --write` refuses it, so it produces no fixture.

## No automatic re-submission

- After a timeout the driver halts input. Every later `operator-input` or `dialog-accept`
  command is refused. herdr's own documentation gives the reason: "A timeout or
  `agent_prompt_stalled` does not prove that no input was sent. Read the agent before
  retrying" (K1 §5 item 3).
- Nothing re-runs `run.mjs` on its own after a `NOT RUN` or `FAIL`: no scenario, no
  wrapper, and no CI job. A retry is a new run with its own run id, box and manifest. It
  starts only as a deliberate new invocation by the agent or operator running the work
  item, after reading the failed run, never from a loop.
- If earlier runs of the same scenario, at the same pins, ended `NOT RUN` or `FAIL`, the
  record lists them under "Findings and UNVERIFIED": run id, outcome, reason. Take these
  from each run's own `run-manifest.json`. `g1-report.mjs` does not track earlier runs, so
  add them to the draft by hand. Re-running until one run passes selects the evidence.

## Evidence: herdr state is a scheduling signal only

- herdr's agent states (`idle`, `working`, `blocked`, `done`, `unknown`) come from
  `agent wait`, `agent get` and `agent prompt --wait`. They decide only when the driver
  reads next, or whether it may continue. They never feed a criterion score and are never
  cited as evidence. K1 §9 labels them "supported (scheduling signal only)".
- The driver's own screen classification (`classifyScreen` in `tools/herdr/lib/g1.mjs`)
  is not herdr state. It classifies the verbatim pane text the driver read: is a dialog
  up, is the in-progress indicator showing. It schedules and guards the run. It also
  feeds mechanical preconditions: G1 criterion 2's "no work in progress and no dialog"
  before the wake, and criterion 3's `midTurnWindow`, which asks whether the turn is
  visibly in progress. Each of these can be re-derived from the committed pane capture.
  herdr's own state classification never enters them.
- Criteria are scored on two things:
  1. **OAC's wire transcript** — the JSONL that the OAC-side endpoint writes itself. For a
     gate spike that is the spike's own server or client (G1: the channel server). Later
     it is OAC's adapter.
  2. **Verbatim pane text** — `agent read` output kept whole. Each section is keyed to the
     herdr command seq and timestamps that produced it.
- The run manifest's command log (role, seq, timestamps) is the driver's own record of
  what input it sent and when. It may be used to prove, for example, that no keystroke
  came before a dialog read. It is not herdr's classification.
- A timing claim, such as "sent mid-turn", comes from wire timestamps against timestamped
  pane reads (G1: `midTurnWindow`), with the unobserved window reported.
- A criterion that needs a human reading of pane text stays `not evaluable` until an
  operator scores it, with a note citing the pane lines (G1 criteria 2 and 3: `--score`
  and `--note`).
- Input reaches the harness only as an operator would type it. Anything the protocol
  under test must carry comes from the OAC-side endpoint, never typed by herdr: a channel
  notification, an app-server request. If a run only works because herdr injects it,
  that is a boundary-13 finding (`oac-boundaries`), not a script.

## Operator-consent dialogs

**The rule: a driver-sent accept of an operator-consent dialog is never verdict-bearing
for G1 criterion 5 or for the G11 confirmation. Full stop.** It is never scored as
meeting the criterion, and it is never evidence that the confirmation was exercised. No
equivalence record, harness version, or dialog-text match changes this.

- **Driver-sent** means the run holds any herdr command with role `dialog-accept`, or ran
  under an accept policy other than human (G1: `--param accept=driver`). For G1,
  `g1-report.mjs` enforces the rule: criterion 5 is `not evaluable`, and `--score 5=…` is
  refused.
- **Only a human accept counts.** The dialog text is read verbatim (`--source visible`)
  before any keystroke. Then the operator accepts at the keyboard while the driver sends
  nothing (`accept=human`). The run manifest then records the accept origin as `human`,
  but that value is inferred, not observed (see "Driver identity"). The human accept
  counts only when the person who accepted it attests to it in the record ("Operator
  attestation" below).
- **Consequence for G1.** A scripted G1 run with a driver accept can be neither an
  equivalence record nor verdict-bearing. Criterion 5 is `not evaluable`, and a gate does
  not pass on a majority of its criteria. A scripted G1 run that is meant to count uses
  `accept=human`, so it is never fully unattended.
- **G11.** The G11 confirmation is the interactive confirmation in G11's documented Claude
  Code launch. This file grants no permission to driver-accept it in any test, including
  the Stage 4 and 5 opt-in tests that K8 prepares to reuse that launch. G11's own
  acceptance says the confirmation is "not bypassed or automated away", and no recorded
  operator decision says otherwise. Whether any such test may driver-accept it is left to
  K8 and a recorded operator decision, if one is ever needed.
- **Where the rule comes from.** It is taken from recorded operator decisions, not
  assumed:
  - G1 criterion 5, verbatim: "actually exercised during the spike — not bypassed, scripted
    around, or skipped" (`references/G1-claude-wake.md`, from PLANNING-PROMPT.md §4 and
    §7: "the plan must not weaken it").
  - K4 acceptance, issue #127 (`docs/planning/backlog/06-tasks-K.json` K4): "a
    driver-sent accept is not scored as meeting criterion 5".
  - G11 acceptance (`docs/planning/backlog/05-tasks-GHIJ.json` G11): "The interactive
    confirmation is presented as a feature, not bypassed or automated away".
- **Changing the rule** takes a separately recorded operator decision: a decision record
  under `docs/planning/decisions/` that names the dialog, the criterion, and the rule that
  replaces this one. It lands together with the matching change to `g1-report.mjs`. None
  exists at K5. Until one does, no agent, reviewer or scenario parameter relaxes this
  rule.
- **Other dialogs.** Folder trust, project MCP-server approval and tool permission
  (`DIALOG_KINDS` in `tools/herdr/lib/g1.mjs`) are not consent steps that any gate
  criterion names. Under `accept=driver` the driver may accept one of them, but only a
  dialog it recognizes, and only when that dialog's accepting option is already
  preselected. The accept is recorded with its origin and scores nothing. If a criterion
  later names such a dialog, the rule above applies to it until an operator decision says
  otherwise.

## Operator attestation

Nothing mechanical separates a real run from a test-double run (see "Driver identity").
So every equivalence record, and every `G<n>-result.md` whose `Driver:` names herdr,
carries this section. The operator who ran the machine and accepted the dialog writes it
after the run:

```markdown
## Operator attestation

- [x] **herdr:** the real herdr binary ran, not a test double. `herdr --version`: `<output>`; sha256 of the executable: `<64 hex>`
- [x] **Harness:** the real, logged-in <harness> CLI ran, not a test double. `<harness> --version`: `<output>`
- [x] **Consent dialog:** accepted by me, a human at the keyboard, during this run. (Or: `none — no criterion of G<n> names a consent step`.)
- **Attested by:** <operator>, <YYYY-MM-DD>
```

- Take the sha256 from the executable that actually ran, for example `sha256sum "$(command
  -v herdr)"`. It gives a later reviewer something to compare. At K5 there is no
  published hash to check it against.
- An unticked box, or a line the operator cannot truthfully write, means the record is
  not an equivalence record and the run is not verdict-bearing.
- `node scripts/check-fixture-manifest.mjs` fails a tracked `herdr-runs/*.md` that carries
  the equivalence callout, and a tracked `G<n>-result.md` whose `- **Driver:**` line
  starts with `herdr`, unless this section is present with all four lines. That check
  proves the attestation is complete, not that it is true. Its truth rests on the operator
  who signed it.

## Verdict eligibility

- **Default: non-verdict-bearing.** Every scripted run of G<n> is non-verdict-bearing.
  Its record lives under `docs/planning/gates/herdr-runs/`. It never changes
  `G<n>-result.md`'s verdict or `docs/planning/STATUS.md`. `G<n>-result.md` gains only a
  pointer to it.
- **Equivalence record for G<n>.** This is a `herdr-runs/G<n>-<YYYY-MM-DD>.md` record
  whose run meets all of these:
  - run outcome `PASS`;
  - `herdr.observedVersionOutput` equals the current PINS.md `herdr (test tooling)` pin;
  - `driver.toolsHerdrDirty` is `false`;
  - it ran on the same harness version(s) as the human run it compares against, which
    was the verdict-bearing run of G<n> when the record was made, verified on both the
    CLI and the wire (G1: `versionsVerified`). A later pin move that makes that human run
    non-verdict-bearing does not undo the record's basis;
  - every pass criterion of G<n> is scored `equivalent`: none `not equivalent`, none
    `not evaluable`, and every operator score carries its note;
  - consent-dialog criteria were met by a human accept (see above);
  - its `-herdr` fixtures are committed with `driver` blocks;
  - it carries a complete, truthful operator attestation (see above).

  The operator adds a callout at the top of the record, `> **Equivalence record** for
  G<n> at herdr <tag>`, when the record is reviewed. The equivalence record is itself
  non-verdict-bearing: it calibrates the method against the human run.
- **When a scripted run may carry a verdict.** All of these must hold:
  - an equivalence record for G<n> exists at the current herdr pin and is not invalidated;
  - `git diff <record's driver commit> <run's driver commit> -- tools/herdr/
    ':!tools/herdr/test/'` is empty, so the driver and scenario code are unchanged;
  - the run used the same `scenario.file`, `launch.argv` and `scenario.params` as the
    equivalence record's run (compare the two run manifests). A different prompt, accept
    policy or timing parameter is a different method, and needs its own equivalence record;
  - `driver.toolsHerdrDirty` is `false`;
  - the run carries its own complete, truthful operator attestation;
  - the run meets the whole gate procedure in `oac-gates`. That means the box, every pass
    criterion from the gate's reference file evaluated individually on the wire transcript
    and pane text, the closed verdict vocabulary, the fixtures, and `STATUS.md` updated in
    the same change;
  - the consent-dialog rule above holds.

  The verdict is written from the evidence by that procedure. herdr decides nothing.
  `Driver:` names the run and the equivalence record it relies on.
- **Reverts on any herdr pin move.** A move of the `herdr (test tooling)` row invalidates
  every equivalence record, and scripted runs of every gate become non-verdict-bearing
  again until a new equivalence record exists at the new pin. The mechanics are in
  `docs/planning/gates/README.md` "Scripted runs (herdr)". A herdr pin move never
  invalidates a gate verdict, because that row's `Gates affected` is `none`.
- **Human runs stay authoritative.** A human-operated run of G<n> is always eligible to
  carry a verdict and needs no equivalence record.

## Checklist: recording a scripted run

- [ ] Box declared before the first herdr command (scenario default or `--timebox-ms`),
      and never extended.
- [ ] Run outcome read from `run-manifest.json`. `NOT RUN` or `FAIL` makes every criterion
      `not evaluable`, and no fixture is written.
- [ ] No criterion scored on herdr state. Each score cites the wire transcript, the
      verbatim pane text, or the driver's command log.
- [ ] Operator scores (G1 criteria 2 and 3) carry a note citing pane lines.
- [ ] Criterion 5 or the G11 confirmation: human accept only. A driver-sent accept is
      recorded and left `not evaluable`.
- [ ] Earlier `NOT RUN` or `FAIL` runs at the same pins are listed under Findings.
- [ ] `driver.toolsHerdrDirty` is `false` before `g1-report.mjs --write` puts anything
      into the repository.
- [ ] `g1-report.mjs --write` output reviewed. The `manifest-entries.draft.json` entries
      are merged into `MANIFEST.json`, and `node scripts/check-fixture-manifest.mjs` passes.
- [ ] Equivalence record or verdict-bearing run: the operator attestation is written by
      the person who ran it, every line true, before the callout or verdict is added.
- [ ] `G<n>-result.md` gains only a pointer, unless the run is verdict-eligible under
      "Verdict eligibility" above.
