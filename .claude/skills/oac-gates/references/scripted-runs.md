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
sign in to the harnesses and grant elevation. Since 2026-09-30 (#196) the driver accepts
Claude Code's trust, MCP-approval and dev-channels dialogs itself in dev/test runs, G1
included, and refuses every other dialog. A human accepts a dialog only in a run meant to
meet a consent-step criterion (G1 criterion 5 under `accept=human`, G11; see
"Operator-consent dialogs"). No live leg is "operator-typed only". Live runs are not in the default CI suite
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
own dialog records the same thing. `acceptOrigin: driver`, by contrast, is observed: every
key the driver sent is a `dialog-accept` command in the run's command log (#196). Until the
driver records the executables it ran
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

**Amended 2026-09-30 (#196).** Operator decision on #196: "Please revise. The point, again,
is automation of these processes during development and test." It follows the #187 decision.
Decision record: `docs/planning/decisions/K-196-driver-accepts-dialogs.md`. The rule below
replaces the K5 rule "only a human accept counts". The K5 text is kept, marked superseded,
at the end of this section.

**The rule.** In dev/test runs the driver accepts Claude Code's workspace-trust, project
MCP-server-approval and `--dangerously-load-development-channels` dialogs itself, and
**only those three**. `accept=driver` is the default in every scenario, `g1-claude-wake`
included (second operator decision on #196, 2026-09-30: "G1 is fully driver-accepted").
`accept=human` is still available.

**Amended 2026-09-30 (#199).** The driver also accepts Codex's workspace-trust dialog, as
recorded live on Codex CLI / app-server 0.159.2 (Windows, TUI attached to the daemon):
options "1. Trust and continue", "2. Back to Agent Command Center", marker `›`, option 1
preselected, so the driver sends `enter` alone. The kind table is `CODEX_DIALOG_KINDS` in
`tools/herdr/lib/g2.mjs`; every rule below applies to it unchanged. It also has these
checks:

- Its options must carry their numbers in order.
- The selection must show the `›` marker.
- The footer must be exactly "enter continue · esc back"; the sandbox footer, or none, is
  refused.
- The question paragraph must be the one recorded from Codex source, whole or truncated
  with "…".
- The optional "Note: You’re in a subdirectory of a Git project…" block above the question
  must match the recorded text when it appears, and is never an option.

Decision record: `K-196-driver-accepts-dialogs.md` §6.

- **Read before any keystroke.** The driver reads the dialog's pane text verbatim
  (`--source visible`) and keeps it in the pane capture before it sends any key.
- **Recognized dialogs only, and the pane decides the keys.** The kind table
  (`DIALOG_KINDS` in `tools/herdr/lib/g1.mjs`) records, for each dialog, the option texts
  seen live, their order, the option Claude Code preselects, and the option the driver
  selects. The driver accepts a dialog only when the pane shows exactly those options, in
  that order, with exactly one selected, and the selection on the recorded preselection or
  already on the accepting option. Anything else ends the run `NOT RUN`, and no key is sent:
  an unrecognized dialog, an option that is not on record, or an unexpected selection.
  Workspace-trust preselects "No, exit" and the MCP dialog preselects "Continue without
  using this MCP server", so the driver moves the selection first. It sends one key at a
  time, each straight after a read, and each move must be confirmed by a fresh read that
  shows the recorded options with exactly one selection marker, on the expected option
  (`selectionCheck` in `lib/g1.mjs`; a read with two markers never counts). It sends Enter
  only after such a read shows the accepting option selected. Once the option list has
  started, every non-empty line up to the footer must be a recorded option, whatever its
  indentation. For MCP that option is "Use this MCP server", never "all future MCP
  servers". Planning: `planDriverAccept` in `tools/herdr/lib/g1.mjs`. Execution:
  `driverAcceptDialog` in `tools/herdr/lib/gate-common.mjs`. A move that does not land
  ends the run `NOT RUN`, and nothing is re-sent.
- **Every other dialog is refused (#197 review).** A kind with no option text on record is
  never driver-accepted, whatever is preselected: Claude Code's tool-permission prompt ("Do
  you want to proceed?"), and every Codex dialog other than the trust dialog on record
  (#199).
  The run ends `NOT RUN` with no key sent. A run that may meet one uses `accept=human`.
  Tool approval is not in #196's scope. Driver-approving it would need its own recorded
  decision and a security note (`oac-security-work`, permission relay).
- **Recorded truthfully.** Every key is a `dialog-accept` command in the run manifest.
  Each dialog record carries `acceptOrigin: driver`, `acceptKeys` (key, herdr command seq,
  and the read that verified the move), and `acceptSeq` (the Enter). A driver accept is
  never recorded or rendered as `human`. Every report lib (`g1-report` … `g5-report`,
  `l3-report`) states it as the driver's.
- **No harness config is written.** The driver never pre-trusts a folder or pre-approves a
  server by editing `~/.claude.json`, settings or `.mcp.json` approvals (`oac-boundaries`
  check 10). Claude Code records the accepted trust in its own state, as it would for a
  human accept.

**Side effect: trust entries accumulate in operator config (#206).** Each accepted trust
dialog makes the harness itself, not the driver, record trust for the scratch project in the
operator's own config. Every run uses a fresh `oac-herdr-scratch-XXXXXX` directory, so one
new entry lands per run (more if a scenario has several project directories). The driver
never writes, edits or prunes these entries (check 10). That includes runs where the operator
accepted the dialog under `accept=human`.

- **Codex (source-confirmed).** Accepting "1. Trust and continue" sends `config/batchWrite`
  to the app-server. The edit is `projects."<key>".trust_level = "trusted"`, with
  `file_path: None` (`trusted_project_edit` / `write_trusted_project`,
  `codex-rs/tui/src/config_update.rs` L76-84, L173-178; called from
  `codex-rs/tui/src/onboarding/onboarding_screen.rs` L770-815). The app-server accepts writes
  to the user config only: "Only writes to the user config are allowed"
  (`apply_edits`, `codex-rs/app-server/src/config_manager_service.rs` L218-238). So the entry
  lands in `~/.codex/config.toml` as a `[projects.'<path>']` table with one `trust_level`
  line. The key is the git root of the project directory when it is inside a git repository,
  otherwise the directory itself, so for a scratch project it is
  `<tmpdir>/oac-herdr-scratch-XXXXXX/<scenario>-project`. All three files are identical at
  tags `rust-v0.159.2` and `rust-v0.159.3`, retrieved 2026-10-01. Observed live: L3 §13 B7
  (`docs/planning/decisions/L1-beacon-memory.md`), one table added, nothing else changed.
- **Claude Code (inference only).** L3 §13 B7 saw `~/.claude.json` change across the run and
  attributes it, from timing alone, to Claude Code recording the trust accept. The content
  was not read, so the entry's shape is UNVERIFIED, and the orchestrating Claude Code
  session may also have written the file. No pruning guidance is given for it.
- **Pruning (operator, by hand, optional).** A Codex `[projects.'…']` table is safe to
  delete when its path has an `oac-herdr-scratch-` plus six-character component directly
  under the OS temp directory, and that directory no longer exists. Trust is keyed by path,
  and mkdtemp names are random and never reused, so the entry can never match a directory
  again. Deleting it costs nothing beyond a fresh dialog should that path ever reappear.
  Keep the entry if the directory still exists (a teardown leftover, `teardown.clean=false`,
  #202). Stop Codex sessions and the daemon first (`codex app-server daemon stop`), back up
  the file, and delete the header and its `trust_level` line together. Touch no other table.
- **B7 interaction (L3).** B0 backups are taken before the run, so they never contain entries
  added during it. A B7 hash comparison therefore shows `config.toml` (and, by inference,
  `.claude.json`) differing from B0 even when Beacon wrote nothing. Restoring a B0 backup
  drops these entries too, but it also reverts anything else written to that file since
  B0. That matters most for `.claude.json`, which Claude Code updates often.
- **Proposal, not implemented.** A reusable Codex project directory, like G1's
  `--param projectDir`, trusted once by the operator, would stop the accumulation. It needs an
  operator decision first, including how it fits the scratch-containment rules.

**What a driver accept scores.** None of these three dialogs is named by a G2, G4 or G5
criterion, so a driver accept costs those gates nothing (see "Verdict eligibility"). A
criterion that is *about* a human consent step keeps its human accept and its human
attestation:

- **G1 criterion 5.** Verbatim: "actually exercised during the spike — not bypassed,
  scripted around, or skipped" (`references/G1-claude-wake.md`, PLANNING-PROMPT.md §4, §7:
  "the plan must not weaken it"). A driver-sent accept of the dev-channels dialog is never
  scored as meeting it. `g1-report.mjs` enforces this: criterion 5 is `not evaluable` when
  the run holds any `dialog-accept` command or ran under a policy other than `human`, and
  `--score 5=…` is refused. The operator accepted the consequence (second decision on
  #196): `g1-claude-wake` defaults to `accept=driver` for all three dialogs, so a default
  G1 run, including every CI G1 run, has criterion 5 `not evaluable` and is never a G1
  equivalence record. A G1 run meant to be one uses `--param accept=human`.
- **G11.** The G11 confirmation ("not bypassed or automated away",
  `docs/planning/backlog/05-tasks-GHIJ.json` G11) is a consent step by definition. This file
  grants no permission to driver-accept it in any test. That stays K8's call and needs a
  recorded operator decision.
- **Other dialogs.** Tool-permission prompts and Codex dialogs other than its trust dialog
  are never driver-accepted (above). No G2, G4 or G5 criterion names Codex's trust dialog. If a criterion later names any dialog as a consent step, the human rule above
  applies to it until an operator decision says otherwise.

**The origin `human` is inferred.** Under `accept=human` the driver sends no key and waits
for the screen to change. `acceptOrigin: human` therefore means only that the driver sent
nothing and the dialog went away (see "Driver identity"). It counts as a human accept only
when the person who accepted it attests to it ("Operator attestation").

**Changing the rule** again takes a separately recorded operator decision: a decision
record under `docs/planning/decisions/` that names the dialog, the criterion and the rule
that replaces this one. It lands together with the matching change to the report libs.
#196 and `K-196-driver-accepts-dialogs.md` are that record for this amendment. No agent,
reviewer or scenario parameter relaxes G1 criterion 5 or G11 without such a record.

*Superseded K5 text (kept for history; not in force since 2026-09-30):* "a driver-sent
accept of an operator-consent dialog is never verdict-bearing for G1 criterion 5 or for the
G11 confirmation … Only a human accept counts … Folder trust, project MCP-server approval
and tool permission … Under `accept=driver` the driver may accept one of them, but only …
when that dialog's accepting option is already preselected." The G1 criterion 5 and G11
parts are still in force, as above. The preselected-only accept of Codex dialogs and
tool-permission prompts was withdrawn in the #197 review: those are now refused.

## Operator attestation

Nothing mechanical separates a real run from a test-double run (see "Driver identity").
So every equivalence record, and every `G<n>-result.md` whose `Driver:` names herdr,
carries this section. The operator who ran the machine and accepted the dialog writes it
after the run:

```markdown
## Operator attestation

- [x] **herdr:** the real herdr binary ran, not a test double. `herdr --version`: `<output>`; sha256 of the executable: `<64 hex>`
- [x] **Harness:** the real, logged-in <harness> CLI ran, not a test double. `<harness> --version`: `<output>`
- [x] **Consent dialog:** <one of the forms below>
- **Attested by:** <operator>, <YYYY-MM-DD>
```

The consent line states who accepted each dialog, exactly as the run manifest records it
(amended 2026-09-30, #196). The report libs generate it that way:

- a gate criterion names a consent step (G1 criterion 5), and a human accepted it:
  `accepted by me, a human at the keyboard, during this run.`;
- no criterion of G<n> names a consent step: `none — no criterion of G<n> names a consent
  step. Dialogs on record: <each dialog, with "accepted by the DRIVER (herdr dialog-accept:
  <keys and seqs>)" or "recorded as human">; each driver accept above was the driver's, not
  mine.`;
- the driver accepted the consent step itself (G1 `accept=driver`, G1's default): the line says so. The
  record is then not an equivalence record (criterion 5 is `not evaluable`).

An operator never ticks a line that calls a driver accept their own.

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
  - a criterion that is a consent step (G1 criterion 5) was met by a human accept; other
    dialogs may be driver-accepted (see "Driver-accepted dialogs" below);
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
- **Driver-accepted dialogs (decided 2026-09-30, #196).** A driver accept of Claude Code's
  workspace-trust, MCP-server-approval or dev-channels dialog does not by itself make a
  run ineligible. It counts only when both of these hold:
  1. the dialog matched the recorded expected text: its kind is recognized and its options
     on screen are exactly the ones on record in `DIALOG_KINDS`. The driver refuses
     anything else, and the run then ends `NOT RUN`;
  2. the run manifest records the accept as the driver's: `acceptOrigin: driver`,
     `acceptKeys` and the `dialog-accept` commands.

  A criterion that is *about* a human consent step keeps its human accept and its human
  attestation: G1 criterion 5, and the G11 confirmation. So an `accept=driver` G1 run
  (G1's default since the operator's second #196 decision) is never an equivalence record
  and never verdict-bearing for G1. No other dialog can be driver-accepted at all: the
  driver refuses it and the run is `NOT RUN`. G2, G4 and G5 name no
  consent step, so for them `accept=driver` and `accept=human` are equally eligible. Their
  accept policy is still part of `scenario.params`, so a verdict-bearing run must use the
  policy of its equivalence record.
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
- [ ] Every other dialog accept is rendered as the manifest records it: `driver` with
      its keys, or `human` (inferred). Never a driver accept written up as a human's.
- [ ] Earlier `NOT RUN` or `FAIL` runs at the same pins are listed under Findings.
- [ ] `driver.toolsHerdrDirty` is `false` before `g1-report.mjs --write` puts anything
      into the repository.
- [ ] `g1-report.mjs --write` output reviewed. The `manifest-entries.draft.json` entries
      are merged into `MANIFEST.json`, and `node scripts/check-fixture-manifest.mjs` passes.
- [ ] Equivalence record or verdict-bearing run: the operator attestation is written by
      the person who ran it, every line true, before the callout or verdict is added.
- [ ] `G<n>-result.md` gains only a pointer, unless the run is verdict-eligible under
      "Verdict eligibility" above.
