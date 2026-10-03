# Gate evidence store — policy

This directory is the evidence store for the five Stage 1 go/no-go gates (G1-G5,
PLANNING-PROMPT.md §4). Pinned versions live in `docs/planning/PINS.md`. Verdict
summary lives in `docs/planning/STATUS.md`'s Gate verdicts table. This directory holds
the evidence each verdict rests on: one result file per gate, plus captured fixtures. It
also holds the records of herdr-driven gate re-runs (Epic K), kept apart from the
verdicts under `herdr-runs/` (see "Scripted runs (herdr)" below).

## Naming convention

- Directory: `docs/planning/gates/`.
- Result file: `docs/planning/gates/G<n>-result.md`, one per gate —
  `G1-result.md`, `G2-result.md`, `G3-result.md`, `G4-result.md`, `G5-result.md`.
  No date or version in the filename. The file is rewritten in place on each re-run;
  the prior verdict moves into the file's own "Re-run history" table (see
  "Re-run/invalidation policy" below). Git history is the archive — do not create a
  dated copy.
- Fixtures for gate G<n>: `docs/planning/gates/fixtures/g<n>-<slug>/` (redacted per
  `oac-gates` §Fixture capture), as committed: `g1-claude-wake/`, `g2-codex-inject/`,
  `g3-zenoh-peer/`, `g4-mcp-dual-era/`, `g5-provenance/`. D6's protocol fixtures sit
  beside them in `d6-claude-protocol/` and `d6-codex-protocol/`. This is the Stage 1
  capture location. The path Stage 3's fake Claude/Codex endpoints finally load
  fixtures from is **not** decided here — Stage 3 may relocate or copy them. (Corrected
  at K5 to match the committed tree. The earlier `fixtures/G<n>/` form was never used.)
- Full command transcripts, when kept, go in the gate's fixture directory as
  `transcript-<YYYY-MM-DD>…`, with any qualifier the capture needs (e.g.
  `transcript-2026-09-28-2.1.283-boxC.jsonl`). The result file carries only the summary.
- Slug names are fixed to the backlog gate labels: G1 `claude-wake`, G2
  `codex-inject`, G3 `zenoh-peer`, G4 `mcp-dual-era`, G5 `provenance`. They are used in
  headings and in fixture directory names, never in result filenames.
- herdr-driven fixtures carry the **`-herdr` suffix**:
  `<kind>-<YYYY-MM-DD>-<version>-herdr.<ext>` (K4), e.g.
  `transcript-2026-09-28-2.1.283-herdr.jsonl` and `pane-2026-09-28-2.1.283-herdr.txt`.
  They sit in the same fixture directory as the gate's human-run fixtures (G1:
  `docs/planning/gates/fixtures/g1-claude-wake/`). The suffix is how the evidence store
  labels a herdr run; a human-run fixture never carries it. A capture named
  `unverified-*-herdr.*` (a run whose harness version was not verified against PINS.md)
  is never committed. A gate that drives both harnesses (K8) names the provider in the
  kind and carries that provider's version: `pane-claude-<date>-<version>-herdr.txt`,
  `transcript-codex-<date>-<version>-herdr.jsonl`; G4's one shared server transcript
  carries both, `transcript-<date>-claude-<version>-codex-<version>-herdr.jsonl`.
- herdr-run records: `docs/planning/gates/herdr-runs/G<n>-<YYYY-MM-DD>.md`, the
  criterion-by-criterion comparison with the human-run baseline, with the driver's
  redacted run manifest beside it as `G<n>-<YYYY-MM-DD>.run-manifest.json`. Written by the
  gate's report generator (G1: `tools/herdr/lib/g1-report.mjs --write`; G2:
  `tools/herdr/lib/g2-report.mjs --write`; G4 and G5: `tools/herdr/lib/g4-report.mjs`,
  `tools/herdr/lib/g5-report.mjs`), which never
  overwrites an existing file. G4 and G5 records compare against runs of
  **reconstructed** gate servers (`tools/herdr/gate-servers/`), because the originals
  were never committed; each record says so at its top.

## Fixture manifest

`docs/planning/gates/fixtures/MANIFEST.json` (D6, issue #39 T1) is the inventory of
every committed fixture file in this tree: one entry per file, giving its provider,
surface, observed version(s), the `docs/planning/PINS.md` row/as-of date it is checked
against, whether that observation currently matches the pin, capture date/time window,
any newer fixture that supersedes it, redaction provenance, and a line-range map of the
protocol exchanges it records. It also carries the top-level note that the path Stage
3's fake endpoints will finally load fixtures from is still undecided (see "Naming
convention" above). Keep it in sync with new fixtures in the same change that adds
them — `node scripts/check-fixture-manifest.mjs` checks that every committed fixture
file has a manifest entry and vice versa.

Every `-herdr` fixture entry also carries a **`driver` block**: `herdr_version` (the
verbatim `herdr --version` output), `driver_commit` (the full commit the driver ran
from) and `run_manifest` (the git-tracked `herdr-runs/*.run-manifest.json`, with its
`.md` record tracked beside it). The same script requires the block on every `-herdr`
entry, requires a boolean `version_matches_pin` (since #216, 2026-10-01, `false` prints a
`VERSION WARNING` and does not fail the check; it was `true`-only before), and checks the block against that run
manifest. The run manifest must show outcome `PASS`, a clean `tools/herdr/`
(`driver.toolsHerdrDirty: false`), the same driver commit and the same herdr version, and
it must list the fixture's file name among its written captures. The script rejects a
`driver` block on an entry without the suffix, and any `unverified-*` capture that also
carries the `-herdr` suffix.

Since #140 (run manifest `schemaVersion` 2) the driver records the herdr executable it
spawned and the sha256 of every capture it wrote, and the script uses both. It refuses a
`-herdr` fixture whose run used the node-run test-double herdr or a herdr that is not a
hashed native binary, and one whose committed bytes (the git index blob) do not hash to
the capture's recorded `sha256`. Since #252 (2026-10-03) the driver also compares that
herdr hash with PINS.md's expected one for its platform (`herdr.executableCheck`), and the
script refuses a fixture from a run where they did not match. For an equivalence record it
requires the herdr sha256 the record states to equal the one the run manifest recorded,
and for a verified record a recorded `match`. A `schemaVersion` 1 run manifest predates
#140: its fixtures print a WARN line, as `G1-2026-09-29`'s do, because nothing binds them
mechanically. The script still cannot tell a real, logged-in harness from a test double
(the manifest records `harnessExecutables`, it does not judge them), nor who pressed a
key. The record states those from the evidence, or as UNVERIFIED, and names any human
action and who did it (`oac-gates` `references/scripted-runs.md` "Verification"). The
script checks that the verification section is present and complete wherever one is
required (an operator attestation from before #252 is accepted as history).
`node scripts/check-fixture-manifest.mjs --self-test` plants one violation per rule.

## Gate-result template

Canonical source: `.claude/skills/oac-gates/SKILL.md` §Gate-result template. Copied
here **verbatim**, with the fields this evidence store needs added at the end (the
skill's copy is the base template and stays unextended; this is the only place the
extended form is maintained):

```markdown
### G<n> <name>

- **Gate id:** G<n>
- **Pinned version(s):** <exact versions the spike ran against, e.g. Claude Code v2.1.232,
  Codex 0.154.0, Zenoh 1.10.1 — never "latest">
- **Date:** <YYYY-MM-DD the spike ran>
- **Timebox:** <box set> / <elapsed; expired or not>
- **Command transcript summary:** <what was run and observed, condensed; full transcript, if
  kept, lives with the fixtures, not inline here>
- **Pass criteria evaluated:**
  - [x|f] <criterion 1, verbatim from the gate's reference file> — <observed result, citing
    the evidence it rests on>
  - ...
- **Verdict:** PASS | PASS (FALLBACK TAKEN) | FAIL | NOT RUN
- **Fallback taken:** <name it, or "none — not needed" or "none — no fallback exists">
- **UNVERIFIED items:** <each item this gate was positioned to close — closed with evidence,
  or still open>
- **Fixtures captured:** <path(s) under the fixtures location, or "none">
- **Human actions:** <each step the agent could not do (consent step, sign-in), and who; or "none">
```

Since #252 (operator decision, 2026-10-03), every finding and judgment in a result rests on
cited evidence or is marked UNVERIFIED. "The operator confirms" is never a basis. A person
signs off only on the human actions: steps the agent could not do itself. Results written
before #252 are not rewritten.

Added fields (B4, this evidence store only):

- **`Pin rows relied on:`** — the exact `docs/planning/PINS.md` pin-table row names
  this verdict depends on (e.g. `Claude Code (Channels)`, `MCP — legacy era`). This
  set must be the inverse of that row's `Gates affected` cell: a pin row is relied on
  by gate G<n> if and only if its `Gates affected` cell names G<n>.
- **`PINS.md as-of:`** — the `**Last updated:**` date of `docs/planning/PINS.md` at
  run time, plus the commit SHA of `PINS.md` at that point.
- **`Re-run history:`** — table `| Date | Pinned versions | Verdict | Invalidated by |`.

Added field (K5, issue #128):

- **`Driver:`** — what drove the run. For a run with no herdr in it: `human operator`.
  For a herdr-driven run: the line `tools/herdr/lib/g1-report.mjs` renders — the
  `herdr --version` output, the PINS.md `herdr (test tooling)` tag,
  `tools/herdr/run.mjs`, the scenario file, and the driver commit. A verdict-bearing
  scripted run also names the equivalence record it relies on, and carries a
  `## Verification` section (#252): herdr, the harness versions and every dialog accept,
  each verified from the run manifest with the field cited or marked UNVERIFIED, plus any
  human action and who did it (an `## Operator attestation` before #252).
  Every `herdr-runs/`
  record carries the same line. The rules behind it are `oac-gates`
  `references/scripted-runs.md` "Driver identity". Every result on record when K5
  landed (G1-G5) was run without herdr and predates this field. The field is added at
  each gate's next re-run, not retrofitted.

Required fields from the issue #33 acceptance criteria must all be present and named:
gate id, pinned version(s), date, command transcript summary, verdict, fallback taken.
Verdict vocabulary is closed: `PASS` | `PASS (FALLBACK TAKEN)` | `FAIL` | `NOT RUN` —
no fifth value, no hedge (`oac-gates` "never probably" rule).

## Re-run/invalidation policy

A version bump to any pin in `docs/planning/PINS.md` visibly, mechanically invalidates
the gate results that depend on it. This is not a judgement call.

a. **The rule.** Changing the **`Pinned version`** cell, the **`Release date`** cell, or
   a row's presence (added or removed) in the `docs/planning/PINS.md` pin table
   invalidates every gate named in that row's **`Gates affected`** column. This does
   **not** include the `Retrieved` or `Observed at (URL)` cells — refreshing a
   retrieval date or citation URL for the same version/release-date (e.g. a re-
   verification pass per `oac-evidence` §7 that confirms no drift) does not invalidate
   the gate. Invalidation is immediate and independent of whether the spike would still
   pass if re-run.

   **The harness rows are exempt (operator decision on #216, 2026-10-01).** The Codex CLI
   / app-server and Claude Code (Channels) rows float. Each records a **minimum version**
   (the first version the project worked with) and a **last tested version** (see each
   row's "Version policy" in PINS.md). Changing either one invalidates **no** gate
   verdict, and no `> INVALIDATED` callout or `NOT RUN` revert follows. A harness version
   other than the last tested one, or below the minimum, is a warning. It never stops a
   run, never makes it `NOT RUN`, never blocks CI and never by itself invalidates a gate
   verdict. A gate result records the version it actually ran on, and a later reader
   compares that version with PINS.md. This covers the CLIs and their wire and daemon
   versions. The rest of §a and §b-§c applies to every other row.

   *Superseded 2026-10-01 (#216), kept as history:* **Floating rows** (currently Codex CLI / app-server and Claude Code (Channels); see
   each row's "Floating-version policy" in PINS.md) follow a stricter rule. A gate verdict
   that relies on the row is current only if the version the gate recorded equals
   **both** the row's last-observed
   version in PINS.md **and** the version the environment reports now. If a release has
   been observed locally but not yet recorded in PINS.md, every verdict relying on the
   row is stale, and stays stale until this checklist is executed for the new version.
   Invalidation applies to the **whole gate**. G4 has a Codex leg and a Claude leg, and
   it is re-run as a whole; one leg is never re-run alone. When a floating row's
   last-observed version changes, this invalidates only the verdicts whose *own*
   recorded observed version differs from the new last-observed version (equivalently,
   from what the environment currently reports) — a gate that already ran on the version
   now being recorded stays current; the move does not blanket-invalidate every gate
   relying on the row regardless of what version each one actually ran against.

b. **Required same-change edits.** When a pin moves, all three of the following land
   in the **same commit** as the pin move:
   1. Each affected `docs/planning/gates/G<n>-result.md` gets:
      - its `**Verdict:**` set to `NOT RUN`,
      - the superseded verdict appended to its `Re-run history` table with
        `Invalidated by: <surface> pin <old> -> <new>, <YYYY-MM-DD>`,
      - a `> INVALIDATED` callout at the top of the file.
   2. The `docs/planning/STATUS.md` Gate verdicts row for that gate reverts to
      `NOT RUN`.
   3. `docs/planning/PINS.md`'s `**Last updated:**` is bumped.
   4. If the pin row itself was added, removed, or renamed (not just its version or
      release date changed), update every affected `G<n>-result.md`'s
      `Pin rows relied on` field and the matching `docs/planning/STATUS.md` Gate
      verdicts table `Pins relied on` cell so both keep naming the row by its current
      name and keep listing exactly the rows the pin table's `Gates affected` column
      says are relied on.

c. **The partial move is forbidden.** A commit that edits the PINS.md pin table
   without the step-b edits is incomplete and must be rejected in review. This is the
   **pin-move checklist** — a copy of it lives in `docs/planning/PINS.md` itself
   (its "Pin-move checklist" section) so a reviewer sees it beside the table being
   changed.

d. **ACP exemption.** The ACP row's `Gates affected` cell is `none`, so an ACP pin
   move invalidates nothing. This is read directly off the column — not a judgement
   call.

e. **Cross-reference to fact re-verification.** Per `oac-evidence` §7, a pin move also
   triggers §3 re-verification (Epic B2, `docs/planning/REVERIFICATION-B2.md`). Gate
   re-run (this policy) and fact re-verification (`oac-evidence` §7) are two separate
   obligations of the same trigger — a pin move requires both, not one in place of the
   other.

f. **herdr pin move (K5).** The `herdr (test tooling)` row's `Gates affected` cell is
   `none`, so, as in §d, a herdr pin move invalidates **no gate verdict** — not even one
   written from a scripted run. It **does** invalidate every equivalence record. The
   same-commit edits are listed in "Scripted runs (herdr)" below. They are not part of
   the §b checklist or its PINS.md copy, which govern gate verdicts only.

## Scripted runs (herdr)

A scripted run is a gate re-run driven through herdr (Epic K #123; driver
`tools/herdr/run.mjs`). The rules for running and recording one — driver identity,
timebox, timeout means `NOT RUN`, no automatic re-submission, herdr state never scores a
criterion, the operator-consent dialog rule, and verdict eligibility — are `oac-gates`
`references/scripted-runs.md`. This section covers only what lands in this directory.

- **Record.** Each scripted run that is written lands as
  `herdr-runs/G<n>-<YYYY-MM-DD>.md`, with its run manifest beside it (see "Naming
  convention"). Only a run with outcome `PASS` is written (`g1-report.mjs --write` and
  the other generators refuse anything else). Since #216 the harness version need not be
  PINS.md's last tested one. A difference is a `VERSION WARNING` finding and makes the
  fixture entry `version_matches_pin: false`, but it never refuses the write. Fixtures
  are written only when each harness's sources (CLI, wire and, for Codex, the daemon)
  reported one and the same version throughout the run, so that a fixture can name it.
  Otherwise (operator decision on #216, 2026-10-01) the record and run manifest are
  still written, with a `VERSION WARNING`, the captures stay `unverified-*`, and no
  fixture or `MANIFEST.json` entry is added. The record's own header
  says it is not verdict-bearing.
- **Fixtures.** The record's captures are committed as `-herdr` fixtures, each with a
  `MANIFEST.json` entry carrying the `driver` block (see "Fixture manifest").
- **Pointer only.** The gate's `G<n>-result.md` gains a pointer to the record, not a
  verdict change. `docs/planning/STATUS.md` is not touched. The one exception is a run
  that `references/scripted-runs.md` "Verdict eligibility" allows to carry a verdict. That
  run is written as an ordinary gate result, with `Driver:` naming it and the equivalence
  record it relies on.
- **Equivalence record.** This is a `herdr-runs/` record that shows a scripted run of
  G<n> equivalent to the human run on every criterion, at the current herdr pin. It is
  marked by the callout `> **Equivalence record** for G<n> at herdr <tag>` at its top,
  and it carries a `## Verification` section (#252, 2026-10-03). That section states herdr
  VERIFIED (its sha256, equal to the run manifest's `herdr.executable.sha256` and matched
  against PINS.md's expected value), the harness VERIFIED (one version per harness from
  every source), who accepted each dialog as the run manifest records it, and the human
  actions with who did them. The recording agent checks each line from the cited evidence;
  the operator's word is not the basis. In an equivalence record, a consent step that a
  criterion names (G1 criterion 5) needs a human accept, and that accept is a human action
  the person who did it names. Since 2026-09-30 (#196) the driver accepts Claude Code's
  three recorded dialogs by default (G1 included, where criterion 5 is then `not
  evaluable`) and refuses every other dialog. Each driver accept is recorded as the
  driver's. A verdict-bearing scripted `G<n>-result.md` carries the same section. The
  definition is in `references/scripted-runs.md` "Verification" and "Verdict
  eligibility". `herdr-runs/G1-2026-09-29.md` is the one equivalence record on record; it
  and `G5-result.md` carry the `## Operator attestation` of their time, kept as history.
- **A herdr pin move invalidates equivalence records, never gate verdicts (§f).** A change
  to the `herdr (test tooling)` row's `Pinned version` cell, its `Release date` cell, or
  its presence in the `docs/planning/PINS.md` pin table (the same cells as §a) triggers
  these edits, in the same commit as the pin move:
  1. Each equivalence record gets the callout `> INVALIDATED as an equivalence record:
     herdr pin <old> -> <new>, <YYYY-MM-DD>` at its top. The rest of the record is left
     unchanged as history.
  2. No `G<n>-result.md` verdict and no `docs/planning/STATUS.md` Gate verdicts row
     changes.

  From that commit on, scripted runs of every gate are non-verdict-bearing until a new
  equivalence record exists at the new pin. A harness version change (Claude Code, Codex)
  invalidates neither a gate verdict nor an equivalence record (§a, #216, 2026-10-01).
  *Superseded text, kept as history:* "A move of a harness row (Claude Code, Codex)
  follows §a-§c for gate verdicts, including a verdict written from a scripted run. It
  does not invalidate an equivalence record, because re-running a gate after a harness
  pin moves is what the record exists for."
- **Opt-in CI (K6).** `herdr-runner.md` in this directory covers the operator-owned
  self-hosted runners that `.github/workflows/herdr-provider-optin.yml` runs on: their
  prerequisites, the upload gate, the threat table, and the first-dispatch record. A CI
  run uploads redacted evidence as a workflow artifact only. It writes nothing into this
  directory. Committing one of its runs follows the same record, fixture and verification
  rules as any other scripted run.
- **Status.** *Dated note, 2026-10-03 (#252):* scripted runs have run live, and
  `herdr-runs/` holds them: `G1-2026-09-29` (an equivalence record), `G5-2026-10-02` (the
  K8 run) and `G5-c13-2026-10-02` (the E1 run that carries G5's Codex verdict). Each has
  its run manifest beside it. All three carry pre-#252 operator attestations, kept as
  history. For the opt-in CI workflow, see `docs/planning/STATUS.md` "Open UNVERIFIED
  items", K6 entry. *Superseded text, kept as history:* "No scripted run has
  run live yet, so `herdr-runs/` does not exist yet."

## Volatility note — the two preview surfaces most likely to move

Per `oac-evidence` §4, every preview/experimental surface carries its stability label
at first mention. Both surfaces below already carry a pinned version and a named (or
explicitly unnamed) shim boundary in `docs/planning/PINS.md`; this section only names
which are likeliest to force a re-run.

- **Claude Code Channels — `research preview`** is **floating** by operator decision
  (2026-09-27), mirroring the Codex row. Since #216 (2026-10-01) it records minimum
  `v2.1.282` and last tested `v2.1.285` (`docs/planning/PINS.md`, Claude Code Channels,
  "Version policy"). Channels floor `v2.1.232` and permission-relay floor `v2.1.234` hold
  at both. (Dated note, 2026-10-02, #122: the `v2.1.232` floor is unsupported. The
  changelog dates `--channels` to `2.1.80`; drift D6, `docs/planning/REVERIFICATION-B2.md`.
  Both versions are still above `2.1.80`.) Relied on by **G1**, **G4** (legacy-MCP negotiation) and **G5**. Claude Code
  ships releases at high cadence and the channel surface is preview, so expect frequent
  version warnings here. A version change no longer invalidates a verdict (§a).
- **Codex CLI / app-server — `experimental` (per-method gating)** is **floating** by
  operator decision (2026-09-26). Since #216 it records minimum `@openai/codex@0.154.0`
  and last tested `@openai/codex@0.159.3`, commit
  `01fc69f4026735edfdf6789820549727a4867b11` (`docs/planning/PINS.md`, Codex CLI and
  app-server, "Version policy"). An auto-updater moves it with every release. Relied on
  by **G2**, **G5** and **G4** (Codex leg). Expect a version warning on most runs. A
  version change no longer invalidates a verdict (§a).
- Contrast: Zenoh, MCP revisions, and the Rust toolchain are `supported` and move on
  slower, announced cadences.
- Both preview surfaces still have `shim boundary: UNNAMED — see DESIGN.md`
  (conflict-register C11). This is not resolved here — cited only.
- Every version string above is cited per `docs/planning/PINS.md`. Invent no new API
  name; if one is needed, it must already appear in `PINS.md` or
  `docs/planning/PLANNING-PROMPT.md` §3.

## Reconciliation with PLANNING-PROMPT.md §9 item 3

`.claude/skills/oac-gates/SKILL.md` and PLANNING-PROMPT.md §9 item 3 both state that
G1-G5 land in **one** file, `docs/planning/v0.1/02-gating-findings.md`. Issue #33
proposes per-gate files instead. This is a conflict between a settled deliverable
(§9 item 3) and this task's evidence-store design — recorded per `oac-evidence` §6 as
a conflict note. It is not an ADR-001 claim, so no ADR amendment is proposed.

- **What §9 item 3 says:** "`02-gating-findings.md` — G1 to G5 with pass, fail,
  fallback, and current verdict," as one of the thirteen files under
  `docs/planning/v0.1/`.
- **What this task proposes:** per-gate files under `docs/planning/gates/`.
- **Reconciliation:** the per-gate files (`docs/planning/gates/G<n>-result.md`) are
  the **evidence store of record** — the place a gate re-run actually writes.
  `docs/planning/v0.1/02-gating-findings.md` is a **derived summary**, assembled from
  the five per-gate files, one section per gate (verdict, date, pins, a link back to
  the per-gate file). §9's deliverable is not dropped — it is made generated-from
  rather than hand-authored. Assembling it is Epic A's task, not B4's (see
  "Do not create `docs/planning/v0.1/`" below).
- This directory (`docs/planning/gates/`) did not exist before this change; Epic A
  reads it once it exists.

`docs/planning/v0.1/` is Epic A's deliverable and Epic A is the currently open epic.
B4 does not create it — B4 only records that `02-gating-findings.md`, once written,
is derived from these files.

## Where the content lives

- Gate definitions, pass/fail criteria, fallback table: PLANNING-PROMPT.md §4.
- Timebox policy, throwaway rule, per-gate reference files: `.claude/skills/oac-gates/SKILL.md`.
- Stage 0/Stage 1 entry-exit criteria and the fixture-capture rationale: PLANNING-PROMPT.md §8.
- The one-file gating-findings deliverable this policy reconciles with: PLANNING-PROMPT.md §9 item 3.
- Pinned versions, `Gates affected` column, pin-move checklist: `docs/planning/PINS.md`.
- Gate verdicts table, pins-relied-on/result-file columns: `docs/planning/STATUS.md`.
- Gate procedure, template, fixture capture, exit criteria: skill `oac-gates`.
- Scripted-run (herdr) rules: `.claude/skills/oac-gates/references/scripted-runs.md`;
  the herdr pin and tool record: `docs/planning/PINS.md` "herdr (test tooling)",
  `docs/planning/decisions/K1-herdr-evaluation.md`; the driver: `tools/herdr/`.
