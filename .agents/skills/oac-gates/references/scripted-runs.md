# Scripted runs (herdr)

Source: Epic K #123 (non-goals; "Why this stays clear of ADR-001 boundary 4"). Backlog:
`docs/planning/backlog/06-tasks-K.json` K3-K5. Tool record:
`docs/planning/decisions/K1-herdr-evaluation.md`. This file states the rules the driver
already enforces (`tools/herdr/run.mjs`, `tools/herdr/lib/herdr.mjs`,
`tools/herdr/lib/manifest.mjs`, `tools/herdr/lib/pins.mjs`, `tools/herdr/lib/g1-report.mjs`,
`tools/herdr/lib/gate-report-common.mjs`) and the rules that sit
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
(`oac-testing` §2). The agent verifies the run from its evidence; the human signs only for
what they did that the agent could not (#252, "Verification").

## Status at K5

No scripted run of any gate has run live. K4 built the G1 scenario and tested it against
test doubles only. K1's live leg is NOT RUN. So no equivalence record exists, and every
scripted run is non-verdict-bearing. Current state: `docs/planning/STATUS.md` "Open
UNVERIFIED items" (the K1 and K4 entries). *This section is K5 history:* live runs and
records since then are under `docs/planning/gates/herdr-runs/` (see "Verification",
History).

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
  `launch.herdrReportedArgv`. Since #140 (`schemaVersion` 2) also `herdr.executable` and
  `harnessExecutables` (see "Executables and capture hashes") and `captures[].sha256`.
  Since #252 also `herdr.executableCheck` (see "Verification").
- **Record and gate result:** the `Driver:` line, as `g1-report.mjs` renders it: the
  `herdr --version` output, the PINS.md `herdr (test tooling)` tag, `tools/herdr/run.mjs`,
  the scenario file, and the driver commit.
- **Fixture manifest:** the `driver` block (`herdr_version`, `driver_commit`,
  `run_manifest`) on every `-herdr` entry. `node scripts/check-fixture-manifest.mjs`
  checks the block against the git-tracked run manifest: outcome, clean `tools/herdr/`,
  commit, herdr version, that the fixture is one of the run's written captures, and (for
  a `schemaVersion` 2 run) that the herdr was a hashed native binary, not the test double,
  and that the fixture's committed bytes hash to the capture's `sha256`. Since #252 it
  also refuses a fixture from a run whose `herdr.executableCheck` is not `match`.

The driver refuses to start unless `herdr --version` equals the PINS.md pin and, since
#252, the herdr executable's sha256 equals PINS.md's expected one for the platform
(a platform with no expected value is a finding, not a stop). A run whose
`driver.toolsHerdrDirty` is not `false` (`true`, or `null` when git could not answer), or
whose `driver.commit` is `null`, cannot be reproduced. Its run id and outcome may be
listed as a finding in a later record. Its captures are never committed as fixtures: the
checker refuses the entries, and `g1-report.mjs --write` does not check this, so do not
`--write` such a run. It is never an equivalence record and never verdict-bearing.

**Executables and capture hashes (#140).** The driver resolves herdr once (on `PATH`,
absolute entries only, or `--herdr-bin`) and spawns only that absolute path; a herdr it
cannot resolve is `NOT RUN` before anything is spawned, never spawned by bare name.
`herdr.executable` is the file resolved and hashed at run start, re-hashed at teardown
(`unchangedAfterRun`; a change is a finding):
`basename`, `realBasename` (after symlinks), `sha256`, `bytes`, `format` (`elf`, `pe`,
`mach-o`, `script` or `unknown`, by magic bytes), `runUnderNode` and `testDouble` (true for
a `.mjs` `--herdr-bin` run under node, i.e. `tools/herdr/test/fake-herdr.mjs`). Each harness
the scenario declares is resolved on the driver's `PATH`, `<harness> --version` is run on
that resolved file (on Windows a `.exe` directly, anything else through
`%SystemRoot%\System32\cmd.exe /d /v:off` by full path), and `harnessExecutables.<harness>`
records the same fields for it.
Directories are never recorded, only basenames, so no home path or user name enters the
manifest. On Windows that is the PATHEXT match (`claude.exe`, an npm `.cmd` shim, ...); for
a Codex standalone install it is the managed binary its launcher directory links to, under
`$CODEX_HOME/packages/`. Under a harness config directory only a file named for the
command itself is ever read; anything else there is recorded unhashed (ADR-001 boundary
3). The config directory is compared as spelled and never touched, so if it (or `$HOME`) is
reached through a symlink or junction, a `PATH` link landing on a non-command file in its
real location is hashed, not refused (known limit; only a sha256 is recorded). Each
written capture records the `sha256` of its redacted bytes.

**What the record cannot show.** The harness that a pane starts is looked up by the pane
shell, whose startup files may change `PATH`, so `harnessExecutables` is what answered
`--version`, not proof of what the pane ran. The record does not judge a harness real: a
fake Claude Code on `PATH` is recorded with its own hash and answers `--version` too.
`acceptOrigin: human` in the run manifest means only that the driver sent no keystroke to
the dialog and the screen then changed. It does not prove that a human pressed a key. A
test double that accepts its own dialog records the same thing. `acceptOrigin: driver`, by
contrast, is observed: every key the driver sent is a `dialog-accept` command in the run's
command log (#196). The record states what the evidence shows and marks the rest
UNVERIFIED; a human accept counts only as a human action that the person who pressed the
key names (see "Verification"). No attestation fills these gaps. A `schemaVersion` 1
run manifest (before #140, e.g. `G1-2026-09-29`) records no executables or capture hashes
at all, and the checker only WARNs on it. A hand edit to `schemaVersion: 1` would downgrade
every #140 refusal to that WARN; refusing a v1 manifest whose `driver.commit` postdates
#140 is an optional tightening, not done (any run manifest is forgeable by hand).

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
- `herdr --version` differs from the pin, or the pin cannot be read (herdr is test tooling
  with a fixed pin; #216 does not cover it);
- the herdr executable's sha256 is not PINS.md's expected one for the platform, or a
  native herdr could not be hashed (#252, `herdr.executableCheck` `mismatch` or
  `unhashed`). It is checked before herdr is spawned. A platform with no expected value
  is a finding only;
- PINS.md has an uncommitted edit to its `herdr (test tooling)` row: the row is missing,
  unparseable, or its tag differs from HEAD's; or to its expected herdr executable table
  (#252). The driver reads the pin only as committed at HEAD (#139). Any other uncommitted
  PINS.md edit is a finding only;
- a scenario preflight stops the run, for example a harness CLI that cannot be run at all.

**A harness version never stops a run (operator decision on #216, 2026-10-01).** Claude
Code and Codex versions float. PINS.md records a minimum and a last tested version for
each. When the CLI, the wire or the Codex daemon reports another version, or one below the
minimum, the scenario records a `VERSION WARNING` finding (`claudeVersionWarning` /
`codexVersionWarning`, `tools/herdr/lib/pins.mjs`) and goes on. The warning never makes the
run `NOT RUN`, never fails a CI job and never by itself invalidates a verdict. The run does
not edit PINS.md: after a live run, the operator records the version there as last
tested. Captures are named after a version only when every source of one harness reports
the same version, so that a fixture names one version. Otherwise they stay `unverified-*`
and the run still goes on. A PINS.md harness row that cannot be read (malformed or
missing) is a `VERSION WARNING` too (`pinsReadWarning`), never a stop.

**Mixed versions within a run (operator decision on #216, 2026-10-01).** When a harness's
CLI, wire or daemon disagree, or a version changes mid-run, the report generators'
`--write` still writes the `herdr-runs/` record and its run manifest, with a
`VERSION WARNING` finding. The captures stay `unverified-*`, and no fixture and no
`MANIFEST.json` draft entry is produced, because a fixture names one version. *Superseded text, kept as history:* "Example: G1's Claude Code
version differs from the PINS.md last-observed version. That is a pin-move trigger."

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
- A criterion that needs a reading of pane text stays `not evaluable` until it is scored
  with a note citing the pane lines (G1 criteria 2 and 3: `--score` and `--note`). The
  recording agent reads the committed pane capture and scores it; the score rests on the
  cited lines, not on who gave it (#252). The libs still call these "operator" scores.
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
  time, each straight after a read of that pane, and each move must be confirmed by a fresh read that
  shows the recorded options with exactly one selection marker, on the expected option
  (`selectionCheck` in `lib/g1.mjs`; a read with two markers never counts). It sends Enter
  only after such a read shows the accepting option selected. Once the option list has
  started, every non-empty line up to the footer must be a recorded option, whatever its
  indentation. For MCP that option is "Use this MCP server", never "all future MCP
  servers". Planning: `planDriverAccept` in `tools/herdr/lib/g1.mjs`. Execution:
  `driverAcceptDialog` in `tools/herdr/lib/gate-common.mjs`. A move that does not land
  ends the run `NOT RUN`, and nothing is re-sent. The read guard (`dialogAccept`,
  `lib/herdr.mjs`) is per pane (#139): an agent name and its pane share one guard, only
  `agent read` and `pane read` count as reads, and input with no target resets every guard.
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
  lands in `~/.codex/config.toml` (or `$CODEX_HOME/config.toml` when `CODEX_HOME` is set) as
  a `[projects.'<path>']` table with one `trust_level` line. The key is the git root of the
  project directory when it is inside a git repository, otherwise the directory itself
  (`codex-rs/tui/src/onboarding/onboarding_screen.rs` L194-200). For a scratch project it is
  `<tmpdir>/oac-herdr-scratch-XXXXXX/<name>`, where `<name>` is `g2-project`,
  `g4-codex-project`, `g5-codex-project` or `l3-codex-project` (`tools/herdr/scenarios/`). On
  Windows Codex writes the key canonicalized and lowercased (`project_trust_key`,
  `codex-rs/config/src/loader/mod.rs` L1367-1399), so compare it with the temp directory
  case-insensitively. All four files are identical at tags `rust-v0.159.2` and
  `rust-v0.159.3`, retrieved 2026-10-01. Observed live: L3 §13 B7
  (`docs/planning/decisions/L1-beacon-memory.md`), one table added, nothing else changed.
- **Claude Code (inference only).** L3 §13 B7 saw `~/.claude.json` change across the run and
  attributes it, from timing alone, to Claude Code recording the trust accept. The content
  was not read, so the entry's shape is UNVERIFIED, and the orchestrating Claude Code
  session may also have written the file. No pruning guidance is given for it.
- **Pruning (operator, by hand, optional).** A Codex projects table is safe to delete when
  its path has an `oac-herdr-scratch-` plus six-character component directly under the OS
  temp directory (compared case-insensitively on Windows), and that directory no longer
  exists. The header may read `[projects.'…']` or `[projects."…"]` (with doubled
  backslashes). Trust is keyed by path and mkdtemp names are random, so reuse is unlikely; if
  a path does recur, deleting the entry costs only a fresh dialog. Keep the entry if the
  directory still exists (a teardown leftover, `teardown.clean=false`, #202). Stop Codex
  sessions and the daemon first (`codex app-server daemon stop`) and back up the file. Then
  delete the whole table, from its header through the line before the next `[` header (or
  the end of the file), and only if it contains nothing else you need. Deleting the header
  alone would leave any remaining keys under the table above it. Touch no other table.
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
criterion that is *about* a human consent step keeps its human accept, which the record
names as a human action, with who did it (see "Verification"):

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
when the person who accepted it is named under "Human actions" in the record's
Verification section. That is a real human sign-off: only that person knows who pressed the
key (#252).

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

## Verification

**Decided 2026-10-03 (#252).** Operator decisions on #252: "My attestation should not be
your law. If you find differently, it is different and should be looked into. Attestation
should not be a critical part of this workflow as much as ACTUAL VERIFICATION." and "Sign
off should be on me for when you NEED me to actually do something you cannot do." This
section replaces the routine operator attestation (its text is kept, superseded, at the
end of this section).

**The rule.** Every finding and judgment in a run record or a verdict rests on
verification from the evidence, cited: run-manifest fields, wire transcripts, pane
captures, hashes, rule text. What the evidence cannot show is marked UNVERIFIED. "The
operator confirms at attestation" is never a basis. If the evidence disagrees with what
anyone says, the operator included, that is a finding to look into, not something a
signature settles.

**What the driver verifies and records** (`tools/herdr/run.mjs`, run manifest):

- **herdr.** `herdr --version` against the pin, and the sha256 of the executable it
  resolved against PINS.md "Expected herdr executable" for its platform
  (`process.platform`-`process.arch`), before herdr is spawned at all. The comparison is
  `herdr.executableCheck` (`tools/herdr/lib/pins.mjs` `checkHerdrExecutable`): `match`;
  `mismatch` or `unhashed`, which end the run `NOT RUN` (see "Timeout means NOT RUN");
  `no-expected-value`, a finding, after which the run goes on with herdr UNVERIFIED; or
  `test-double`, never compared and never verified. What `run.mjs` does with each result is
  `herdrCheckDecision` in the same file. It is unit-tested, and the self-test drives
  `run.mjs` with a native non-herdr binary to check the mismatch, match and
  no-expected-value paths. A `match` counts as VERIFIED only on a row whose `First-party`
  cell is `yes` (`executableCheck.firstParty`). A match on a locally observed value (the
  Windows row) shows only that the binary is that one: the record states herdr
  UNVERIFIED, first-party source UNVERIFIED. The table is read from PINS.md at HEAD, like
  the tag. The run manifest is `schemaVersion` 3. The executable is re-hashed at teardown
  (`unchangedAfterRun`).
- **Harness versions** from every source the scenario reads (CLI, wire, the Codex daemon,
  post-run), and the hash of each harness executable that answered `--version`
  (`harnessExecutables`).
- **Every dialog accept, with its origin:** `driver`, its keys in the command log, or
  `human`, inferred (see "Driver identity" and "Operator-consent dialogs").

**What the recording agent does.** The report libs (`g1-report` … `g5-report`,
`l3-report`; `verification()` in `tools/herdr/lib/gate-report-common.mjs`) generate the
section from the run manifest, each line `VERIFIED` or `UNVERIFIED` with the fields it
rests on. The recording agent re-checks every citation against the committed run manifest
and captures: the fixture hashes, the herdr hash against the PINS.md row at the driver
commit, and the versions against the wire. It turns a line to UNVERIFIED, and records a
finding, wherever a check fails. It fills each `<TO FILL: ...>` slot and the
`Verified by` line. Where it cannot fill a slot itself, it asks the person who did the
action.

**Human actions: the only human sign-off.** A person signs only for what the agent cannot
do itself:

- a criterion that is itself a human consent step: G1 criterion 5's dev-channels accept
  (`accept=human`) and the G11 confirmation;
- an interactive sign-in to a harness;
- anything that needs credentials.

The record names each action and who did it. Under `accept=human` the driver records only
that it sent no key and the screen changed, so only the person who pressed the key can say
who did it. Nothing else in the record is signed.

```markdown
## Verification

- **herdr:** VERIFIED — <version against the pin; sha256 `<64 hex>` against PINS.md's expected value for `<platform>`; native, not the test double; unchanged at teardown; each with its run-manifest field> | UNVERIFIED — <why>
- **Harness:** VERIFIED — <each harness: the version from every source, with fields; executable hashes> | UNVERIFIED — <why>
- **Dialogs:** <each dialog: accepted by the DRIVER (keys and seqs), or recorded as human>
- **Human actions:** <consent step, sign-in or credentials, and who did each; or "none ...">
- **Verified by:** <recording agent>, <YYYY-MM-DD>
```

- A herdr or Harness line that is UNVERIFIED means the record is not an equivalence record
  and the run is not verdict-bearing. A G1 run whose dev-channels dialog the driver
  accepted has no human action for criterion 5, so it is neither.
- `node scripts/check-fixture-manifest.mjs` fails a tracked `herdr-runs/*.md` that carries
  the equivalence callout, and a tracked `G<n>-result.md` whose `- **Driver:**` line
  starts with `herdr`, unless this section is complete: herdr VERIFIED with a 64-hex hash,
  Harness VERIFIED, the Dialogs, Human actions and dated Verified by lines, and no
  `<TO FILL` left. Every such file must be tied to a committed run manifest that parses:
  - an equivalence record, to its own run manifest;
  - a gate result, to the run manifest beside the `herdr-runs/` record its Driver line
    names.

  For a verified file, that manifest must be the #252 driver's (`schemaVersion` 3, or
  carrying `herdr.executableCheck`). It must record a first-party `match`, and its
  expected value must equal PINS.md's committed row for the platform, and the row at the
  run's driver commit when that commit is in the repository. The stated herdr hash must
  be the manifest's. The script refuses a `-herdr` fixture from a run whose
  `herdr.executableCheck` is not `match`, or from a `schemaVersion` 3 manifest without
  one. It proves completeness and those bindings. The rest is the recording agent's
  check, from the citations.
- **History.** Records made before #252 carry the `## Operator attestation` of their time:
  `herdr-runs/G1-2026-09-29.md`, `G5-2026-10-02.md`, `G5-c13-2026-10-02.md` and
  `G5-result.md`. They stay as written. The checker accepts a complete attestation only
  when all of these hold:
  - the run manifest is pre-#252 (`schemaVersion` <= 2, no `executableCheck`);
  - the record is dated no later than 2026-10-03;
  - the file carries the callout `> **Pre-#252 attestation (history).**`, which says it is
    not a current basis.

  An attested line is history, not verification. `G1-2026-09-29` and `G5-c13-2026-10-02`
  have `schemaVersion` 1 run manifests, so their herdr hash was taken by hand, not recorded
  by the driver. Their herdr identity beyond `herdr --version` is UNVERIFIED (dated notes
  in each, 2026-10-03).
- **G1-2026-09-29 is not a current equivalence record (2026-10-03).** Under this rule an
  equivalence record needs herdr VERIFIED, and this one cannot be. It stays on record with
  its callouts. A G1 run under the #252 driver must re-establish G1 equivalence before
  any scripted G1 run can carry a verdict. G1's verdict rests on the human-run Box C and
  is unaffected.

*Superseded #140/#196 text (kept for history; not in force since 2026-10-03):* "every
equivalence record, and every `G<n>-result.md` whose `Driver:` names herdr, carries this
section. The operator who ran the machine and accepted the dialog writes it after the run
… `- [x] **herdr:** the real herdr binary ran, not a test double` … `- [x] **Harness:** the
real, logged-in <harness> CLI ran` … `- [x] **Consent dialog:**` … `- **Attested by:**`
… That check proves the attestation is complete … not that the rest is true. That rests on
the operator who signed it."

## Verdict eligibility

- **Default: non-verdict-bearing.** Every scripted run of G<n> is non-verdict-bearing.
  Its record lives under `docs/planning/gates/herdr-runs/`. It never changes
  `G<n>-result.md`'s verdict or `docs/planning/STATUS.md`. `G<n>-result.md` gains only a
  pointer to it.
- **Equivalence record for G<n>.** This is a `herdr-runs/G<n>-<YYYY-MM-DD>.md` record
  whose run meets all of these:
  - run outcome `PASS`;
  - `herdr.observedVersionOutput` equals the current PINS.md `herdr (test tooling)` pin,
    and `herdr.executableCheck` is a first-party `match` (#252);
  - `driver.toolsHerdrDirty` is `false`;
  - it records the harness version(s) it ran on, each verified as one version across the
    CLI and the wire (G1: `versionsVerified`). The human run it compares against is the
    verdict-bearing run of G<n> when the record was made. Since #216 a harness version
    other than that run's, or other than PINS.md's last tested one, is listed under
    "Findings" as a `VERSION WARNING`. It does not by itself disqualify the record:
    equivalence across harness versions is allowed, and the difference is a finding
    (operator decision on #216, 2026-10-01). Every report lib states this in its
    Findings. (Until 2026-10-01 the same harness version as the human run was required.);
  - every pass criterion of G<n> is scored `equivalent`: none `not equivalent`, none
    `not evaluable`, and every score carries its note citing the evidence;
  - a criterion that is a consent step (G1 criterion 5) was met by a human accept, named
    with who did it under "Human actions"; other dialogs may be driver-accepted (see
    "Driver-accepted dialogs" below);
  - its `-herdr` fixtures are committed with `driver` blocks;
  - it carries a complete Verification section, herdr and Harness `VERIFIED`, every line
    checked by the recording agent from its citations (see "Verification").

  The recording agent adds a callout at the top of the record, `> **Equivalence record**
  for G<n> at herdr <tag>`, once it has verified every condition above from the record's
  citations; review re-checks them. The equivalence record is itself non-verdict-bearing:
  it calibrates the method against the human run. (Before #252 the operator added the
  callout.)
- **When a scripted run may carry a verdict.** All of these must hold:
  - an equivalence record for G<n> exists at the current herdr pin and is not invalidated, and is current under #252 (not marked `Pre-#252 attestation (history)`);
  - `git diff <record's driver commit> <run's driver commit> -- tools/herdr/
    ':!tools/herdr/test/'` is empty, so the driver and scenario code are unchanged;
  - the run used the same `scenario.file`, `launch.argv` and `scenario.params` as the
    equivalence record's run (compare the two run manifests). A different prompt, accept
    policy or timing parameter is a different method, and needs its own equivalence record;
  - `driver.toolsHerdrDirty` is `false`;
  - the run carries its own complete Verification section, herdr and Harness `VERIFIED`
    and checked by the recording agent, naming every human action and who did it (#252);
  - every criterion is scored from cited evidence (wire transcript, pane text, hashes,
    rule text), or the result says it is UNVERIFIED; never on the operator's word;
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

  A criterion that is *about* a human consent step keeps its human accept, a human action
  the record names: G1 criterion 5, and the G11 confirmation. So an `accept=driver` G1 run
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
  carry a verdict and needs no equivalence record. Its findings, like any, rest on cited
  evidence or are UNVERIFIED (#252); "authoritative" is about the method, not about taking
  the operator's word.
- **One-off exception: the C13 G5 Codex re-run (operator decision, 2026-10-02, #220,
  route E1).** Exactly one herdr run of `g5-provenance`, the G5 Codex-leg re-run defined in
  `docs/planning/decisions/C13-codex-provenance-framing.md` §11, may carry G5's Codex
  verdict. It needs no G5 equivalence record, and its `tools/herdr/` diff need not be
  empty. All of these must hold:
  - its arm 0 (the old C6 §5 frame) reproduces the 2026-09-27 FAIL, as C13 §11 defines it.
    Otherwise the run is inconclusive and carries no verdict;
  - its `tools/herdr/` diff (excluding `tools/herdr/test/`) from commit
    `2776e7a89bc3d7f5d7c39bea791a1919dd17119a` (the #217 merge on `main`) is limited to
    the list below. *Corrected 2026-10-02:* `tools/herdr/` on `main` has changed since
    2776e7a8 (#222, #226, #227), so the run is driven from the PR #231 branch,
    `herdr/220-c13-g5-codex-framing`, which is based on `74e3e64` (PR #223's head; its
    `tools/herdr/` equals 2776e7a8's) and never merges `main`. The list:
    - the approved framing in `gate-servers/g5-codex.mjs`;
    - the new cases in `gate-servers/g5-cases.json`;
    - the scoring in `lib/g5-report.mjs`;
    - arm and case selection only in `scenarios/g5-provenance.mjs`;
  - it carries a complete, truthful operator attestation (the rule when E1 was used;
    E1 is consumed, and #252 replaced attestation for every later run);
  - it meets every other condition of "When a scripted run may carry a verdict", except the
    three waived here:
    - an equivalence record exists;
    - an empty `tools/herdr/` diff against that record's commit;
    - the same `scenario.file`, `launch.argv` and `scenario.params` as that record's run.

    The rest still apply: the box, each criterion scored individually, the closed verdict
    vocabulary, fixtures, and `STATUS.md` in the same change.

  **Which run consumes it.** The exception is used by the first run that meets both of
  these:
  - its outcome is `PASS` or `FAIL`;
  - its arm 0 reproduced the old FAIL.

  That run's verdict is final under the exception. A run that is `NOT RUN`, or inconclusive
  (arm 0 did not reproduce), does not consume it; list such runs under that run record's
  "Findings". A `FAIL` is not re-run under this exception.

  It covers only the Codex leg. The G5 Claude results of 2026-09-27 stand (same decision).
  It does not create a G5 equivalence record, does not extend to any later run, and does
  not change the rule for any other gate. `Driver:` cites this bullet. The rationale is
  consistency with the standing herdr-first decisions (#187, #196, #216); the record is
  C13 §0/§11.

  **Operator rulings, 2026-10-02 (#220, from the PR #231 review).** These bind the run:
  1. *Scope.* Arm and case selection includes three things, in arms mode only
     (`--param arms`): a fresh Codex TUI per arm, handling a refused delivery, and no
     operator question for a mechanically scored case. The K8 path (no `arms`) stays as it
     was.
  2. *Scoring.* The agent scores Codex criteria 2 and 3 for each reply from the captures.
     For this run that replaces "Evidence"'s operator-scored reading. The operator attests
     the run, and the record states the basis as agent-scored.
  3. *Claude leg.* The run skips the Claude leg (`claudeCases=none`). The 2026-09-27
     Claude results stand. A separate, non-verdict K8 run re-checks Claude for regression.
  4. *Non-consuming runs.* A run that a tooling problem leaves unscorable does not consume
     the exception. Examples: a refusal case never attempted, a delivery in the wrong
     thread, or a dirty `tools/herdr/`. Such a run is `NOT EVALUABLE` in the report. Only
     a real `PASS` or `FAIL` with arm 0 reproduced consumes it.

  **Consumed, 2026-10-02.** Run `20261002T161612Z` used E1 up. Its C13 outcome was `PASS`,
  and its arm 0 reproduced the old FAIL. Record: `docs/planning/gates/herdr-runs/
  G5-c13-2026-10-02.md`.
  - The earlier run `20261002T160321Z` ended `FAIL` on a scenario race, with C13 `NOT RUN`.
    It did not consume E1.
  - The rule that the run never merges `main` bound the run's driver commit, `a86e620`. The
    branch merged `main` after the run (`078c794`), to land the record. That does not
    affect the verdict, because the E1 path rule was checked at the driver commit.
  - No later run may use E1.

## Checklist: recording a scripted run

- [ ] Box declared before the first herdr command (scenario default or `--timebox-ms`),
      and never extended.
- [ ] Run outcome read from `run-manifest.json`. `NOT RUN` or `FAIL` makes every criterion
      `not evaluable`, and no fixture is written.
- [ ] No criterion scored on herdr state. Each score cites the wire transcript, the
      verbatim pane text, or the driver's command log.
- [ ] Pane-text scores (G1 criteria 2 and 3) carry a note citing pane lines.
- [ ] Every finding and judgment cites its evidence or says UNVERIFIED. Nothing rests on
      "the operator confirms". Evidence that disagrees with anyone's account is a finding.
- [ ] `herdr.executableCheck` read: a first-party `match`, or the herdr line is UNVERIFIED
      (a `no-expected-value`, or a match on a locally observed value, is under Findings).
- [ ] Criterion 5 or the G11 confirmation: human accept only, named under "Human actions"
      with who did it. A driver-sent accept is recorded and left `not evaluable`.
- [ ] Every other dialog accept is rendered as the manifest records it: `driver` with
      its keys, or `human` (inferred). Never a driver accept written up as a human's.
- [ ] Earlier `NOT RUN` or `FAIL` runs at the same pins are listed under Findings.
- [ ] Any `VERSION WARNING` (harness version not PINS.md's last tested one, or below its
      minimum) is listed under Findings, and is never the reason for a `NOT RUN` (#216).
      After a live run, PINS.md's last tested version is updated in a separate change.
- [ ] `driver.toolsHerdrDirty` is `false` before `g1-report.mjs --write` puts anything
      into the repository.
- [ ] `g1-report.mjs --write` output reviewed. The `manifest-entries.draft.json` entries
      are merged into `MANIFEST.json`, and `node scripts/check-fixture-manifest.mjs` passes.
- [ ] Verification section re-checked by the recording agent against the committed run
      manifest and captures, every `<TO FILL: ...>` slot filled, before the callout or
      verdict is added. A person signs only the human actions: consent steps, sign-ins,
      credentials. Ask them only for those.
- [ ] `G<n>-result.md` gains only a pointer, unless the run is verdict-eligible under
      "Verdict eligibility" above.
