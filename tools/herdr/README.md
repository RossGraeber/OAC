# tools/herdr: the scripted-run driver (dev/test tooling only)

Epic K (#123). A driver that runs a scenario against real, logged-in harness CLIs (Claude
Code, Codex) through [herdr](https://github.com/herdrdev/herdr), a terminal multiplexer, so a
gate re-run or an opt-in integration test can be scripted. herdr plays the operator's hands
and eyes: it starts the harness, types what an operator would type, and reads the screen. It
never replaces a supported interface and it decides nothing.

- Never part of the product. Nothing under `adapters/`, `core/`, `cli/`, `transports/` or
  `spec/` may import, link, vendor or invoke anything here, and no workspace or package
  manifest outside this directory may reference it. `node scripts/check-herdr-containment.mjs`
  enforces this (oac-boundaries checks 9 and 10), including that nothing here touches harness
  credentials or edits harness config.
- Never CI-default. Every run needs a real, logged-in harness, so it is opt-in by
  construction (`docs/planning/v0.1/09-test-strategy.md` §4). The only CI entry point is
  `ci.mjs`, run by the opt-in workflow on operator-owned runners (K6).
- Node built-ins only. No `package.json`.
- How a scripted run is recorded, what it may conclude, the operator attestation and verdict
  eligibility: `.claude/skills/oac-gates/references/scripted-runs.md`. Where its evidence
  lands: `docs/planning/gates/README.md` "Scripted runs (herdr)". Tool record and pin:
  `docs/planning/decisions/K1-herdr-evaluation.md`, `docs/planning/PINS.md` row
  `herdr (test tooling)`.

## Layout

| Path | What |
|---|---|
| `run.mjs` | The driver (K3): isolated herdr session, bounded waits, timebox, redaction, run manifest. `--self-test` runs every test below against test doubles. The manifest is written even when scratch removal fails (#202, `lib/scratch.mjs`): removal is retried with a bounded backoff, and a leftover is recorded (`teardown.clean=false`, `teardown.leftover` redacted, a finding naming any holder the scenario declared, e.g. the shared Codex daemon, released on `codex app-server daemon stop`) without changing the outcome. An escaping driver error is printed with its phase (setup, run, teardown, record). Teardown process accounting (#136, `lib/herdr.mjs` `teardown`, `lib/proc.mjs` `processTable`): every pane `workspaceCreate` returned is queried with `pane process-info` whether or not the scenario asked, and the pane processes' and the herdr server's descendants are recorded from one process table (Linux `/proc`, macOS `ps`, Windows `Get-CimInstance Win32_Process`: pid, parent pid, creation time, argv). After the stop a recorded pid still alive is killed, that pid only and never its tree, only if its creation time is unchanged, it is no older than the driver process, and it is not the Codex app-server (the shared daemon is never stopped). Anything it cannot verify is not killed: it is listed in `teardown.leftoverProcesses` (`unverifiedPids`), and teardown is not clean. |
| `ci.mjs`, `runner-hooks/` | The opt-in CI entry point and runner hooks (K6). |
| `lib/` | Driver internals, and per-gate helpers and report generators (`g1*.mjs` K4, `g2*.mjs` K7, `g4*.mjs` and `g5*.mjs` K8, `gate-common.mjs` and `gate-report-common.mjs` shared by K8, `l3.mjs` L3a). |
| `scenarios/` | `smoke`, `g1-claude-wake` (K4), `g2-codex-inject` (K7), `g4-mcp-dual-era` and `g5-provenance` (K8), `l3-beacon` (L3b; the Beacon live leg, not a gate). |
| `gate-servers/` | K8: the G4 and G5 gate servers, **reconstructed** (see below). |
| `test/` | The self-test and its test doubles (`fake-herdr.mjs`, `fake-claude.mjs`, `fake-codex.mjs`, `fake-beacon.mjs`, and `fake-rm-eperm.mjs`, which makes scratch removal throw EPERM, #202) and the file-access tracer (`fs-trace.mjs`). |

## Dialogs: the driver accepts them (dev/test runs, #196)

Operator decision of 2026-09-30 (#196, `docs/planning/decisions/K-196-driver-accepts-dialogs.md`):
in dev/test runs the driver accepts Claude Code's workspace-trust, project-MCP-server and
`--dangerously-load-development-channels` dialogs itself, and, since #199, Codex's
workspace-trust dialog: **only those four**. `accept=driver` is the default in every
scenario, `g1-claude-wake` included, and `accept=human` is still available. Each dialog is
read verbatim before any key. It is accepted only if its options on screen are exactly the
ones recorded in `lib/g1.mjs` `DIALOG_KINDS` (Claude Code) or `lib/g2.mjs`
`CODEX_DIALOG_KINDS` (Codex). The selection is moved one key at a time. Each move is
verified by a read that shows exactly one selection marker, on the expected option. Enter
is pressed only on the accepting option:

| Harness | Dialog | Preselected (seen live) | Driver keys | Recorded on |
|---|---|---|---|---|
| Claude Code | workspace-trust | "No, exit" | `down` (read shows "Yes, I trust this folder"), `enter` | v2.1.283, 2026-09-29/30 |
| Claude Code | mcp-server-approval | "Continue without using this MCP server" | `up`, `up` (read shows "Use this MCP server"; never "all future"), `enter` | v2.1.283, 2026-09-29/30 |
| Claude Code | dev-channels | "1. I am using this for local development" | `enter` | v2.1.283 (G1 Box C; 2026-09-30) |
| Codex | workspace-trust | "› 1. Trust and continue" (options "1. Trust and continue", "2. Back to Agent Command Center"; marker `›`) | `enter` | 0.159.2, 2026-09-30 (#199) |

Codex's trust dialog may carry a "Note: You’re in a subdirectory of a Git project. Trusting
will apply to the repository root: …" block above its question. The block may be present or
absent; when present it must be exactly the recorded text, and it is never an option.

- The question paragraph must be the recorded one, whole or truncated with "…".
- The options must be numbered 1 and 2 in that order, and the selection must show Codex's
  `›` marker.
- The footer must be exactly "enter continue · esc back". The sandbox footer "enter continue
  and create sandbox · esc back", or no footer at all, is refused.

Codex's other variants ("Quit" as option 2; "Open restricted" or "Open existing task" as
option 1) are not on record and are refused.

Any other text, an extra option at any indentation, a second selection marker, or a move that
does not land ends the run `NOT RUN`, with nothing guessed and nothing re-sent. **Every other
dialog is refused**, whatever is preselected: Claude Code's tool-permission prompt ("Do you
want to proceed?") and every other Codex dialog (#197 review). The run ends `NOT RUN` with
no key sent. Use `accept=human` for a run that may meet one. Each accept is recorded as `driver` with its keys, and every report says so.

**Codex startup: verified-ready before the first message (#204).** Codex 0.159.2 shows its
composer ("› Ask Codex to do anything") *before* its session exists: the startup draft. Text
typed there is held ("Waiting for startup · esc cancel") until the app-server bootstrap, any
startup hook review and `thread/start` have run. So `g2-codex-inject`, `g5-provenance` and
`l3-beacon` type the operator's first Codex message only once a thread not loaded before the
launch appears in the daemon's `thread/loaded/list` AND the pane shows the idle composer with no
dialog, startup screen or "Waiting for startup" (`lib/g2.mjs` `codexReadiness` /
`waitCodexReady`, bounded by `--param readyTimeoutMs`, default 120000, and the box). Otherwise
the run ends `NOT RUN` naming the blocker; nothing is typed or re-sent. Codex's startup hook
review ("Hooks need review", shown while any hook in the Codex hooks config is new or changed)
and the hooks browser it opens are recognized kinds that the driver **never** answers: trusting
a hook is the operator's decision. Review and trust the hooks in a Codex session of your own
(`/hooks`) before the run, or run with `accept=human` and answer the review yourself. Under
`accept=human`, L3 records a finding whenever the review was answered: the driver cannot see
the choice, and "Continue without trusting" means Beacon's SessionStart hook did not run. For
information only, not recommended and never set by the driver: Codex also skips this review when
its config sets `bypass_hook_trust` (read as `config.bypass_hook_trust` in
`codex-rs/tui/src/lib.rs@rust-v0.159.2`).

`g1-claude-wake` defaults to `accept=driver` too, per the operator's second decision on #196.
G1 criterion 5 is the dev-channels consent step itself, so on such a run it is `not
evaluable` and the run is never a G1 equivalence record. CI's G1 run is one of these. Use
`--param accept=human` for a G1 run meant to be an equivalence record. Rules and verdict
eligibility: `scripted-runs.md` "Operator-consent dialogs".

**Optional friction reducers (not required).**

- **A reusable project directory.** `g1-claude-wake --param projectDir=<absolute path outside
  the repo>` reuses one directory. Claude Code keeps the folder trust and the MCP approval
  it recorded for it, so those dialogs do not come back.
- **`enabledMcpjsonServers`.** An operator may pre-approve a project server in their own
  Claude Code settings, for example `"enabledMcpjsonServers": ["g5spike"]` in the project's
  `.claude/settings.local.json`. That is the operator's choice, made by hand.

The driver itself never writes harness config to skip a dialog (`oac-boundaries` check 10):
no `~/.claude.json` trust entry, no settings edit, and never herdr's hook-writing command.

**Known side effect: trust entries pile up in your harness config (#206).** When a trust
dialog is accepted, by the driver or by you, the harness records that trust in your own
config. Each run uses a fresh `oac-herdr-scratch-XXXXXX` directory, so each run adds one
new entry per project directory. The driver never writes, edits or prunes these entries.

- **Codex** adds a `[projects.'<tmpdir>\oac-herdr-scratch-XXXXXX\<name>']` table with
  `trust_level = "trusted"` to `~/.codex/config.toml` (or `$CODEX_HOME/config.toml` when
  `CODEX_HOME` is set). `<name>` is `g2-project`, `g4-codex-project`, `g5-codex-project` or
  `l3-codex-project`. On Windows the key is canonicalized and lowercased
  (`codex-rs/config/src/loader/mod.rs` L1367-1399), so compare it with your temp directory
  case-insensitively. The TUI sends `config/batchWrite` to the app-server, which writes only
  the user config (`codex-rs/tui/src/config_update.rs` L76-84, L173-178;
  `codex-rs/app-server/src/config_manager_service.rs` L218-238; identical at
  `rust-v0.159.2` and `rust-v0.159.3`, retrieved 2026-10-01). L3 saw exactly one such table
  added (`docs/planning/decisions/L1-beacon-memory.md` §13 B7).
- **Claude Code** changed `~/.claude.json` during the same L3 run. That is attributed to its
  trust record by timing only (§13 B7). The entry's shape is UNVERIFIED, so there is no
  pruning guidance for it. G1's `--param projectDir` avoids new entries for G1.
- **Pruning the Codex tables (optional, by hand).** Delete a projects table only if its path
  has an `oac-herdr-scratch-` plus six-character component directly under your temp
  directory (case-insensitively on Windows) and that directory no longer exists. The header
  may read `[projects.'…']` or `[projects."…"]` (with doubled backslashes). Trust is keyed by
  path and the scratch names are random, so reuse is unlikely; if a path does recur, deleting
  the entry costs only a fresh dialog. Keep it if the directory is still there (a teardown
  leftover, #202). Run `codex app-server daemon stop` and close Codex sessions first, and
  back up the file. Then delete the whole table, from its header through the line before the
  next `[` header (or the end of the file), and only if it contains nothing else you need:
  deleting the header alone would leave its other keys under the table above. Leave every
  other table alone.
- **L3 B7.** B0 backups predate the run, so they hold none of these entries. B7's hash check
  shows `config.toml` (and `.claude.json`) changed even though Beacon did not write them.
  Restoring a B0 backup removes the entries, but it also reverts any other change made to
  that file since B0.
- **Proposal only.** A reusable Codex project directory, like G1's `projectDir`, would stop
  the accumulation. It is not implemented and needs an operator decision.

Rules: `.claude/skills/oac-gates/references/scripted-runs.md` "Side effect: trust entries
accumulate in operator config".

## Scripted gate re-runs

Each gate scenario replays the human-run gate spike through herdr and records the run; its
report generator scores every pass criterion against the human run's committed fixture and
writes a `docs/planning/gates/herdr-runs/G<n>-<YYYY-MM-DD>.md` record. **None of them is
verdict-bearing**: a gate's verdict comes only from its human-run procedure unless
`scripted-runs.md` "Verdict eligibility" says otherwise. **None has run live**: each is
exercised only against the test doubles, so every harness-facing behavior is UNVERIFIED until
it runs live (the commands are in each scenario's header comment; an agent runs them,
see "Operator setup" below).

```bash
node tools/herdr/run.mjs --scenario g4-mcp-dual-era --out <run dir>   # accept=driver (default, #196)
node tools/herdr/lib/g4-report.mjs --run <run dir>                 # draft; add --write to record
node tools/herdr/run.mjs --scenario g5-provenance --out <run dir>
node tools/herdr/lib/g5-report.mjs --run <run dir>
```

- **G4** (`g4-mcp-dual-era`): the legacy (`2025-11-25`) channel path and the `2026-07-28` tool
  path, concurrently, on one server process, with Codex's legacy HTTP traffic between two
  modern calls, and the `2026-07-28` channel-rejection negative case (the `g4modern` copy).
  Codex's MCP registration is **per invocation** (`codex -c mcp_servers.<name>.url="..."`,
  `--param codexLaunch`): the operator's global Codex config is never edited, no Codex config
  file is written anywhere, and the Codex home is never copied or redirected. A
  project-scoped `.codex/config.toml` would also meet K8's acceptance, but check 10 cannot
  tell a scratch project's file from the operator's, so it is not used. The self-test proves
  the rule from a file-access trace (no write under either harness home, no `config.toml`
  written anywhere, nothing copied out of the Codex home). The driver cannot see the
  operator's own Codex config, and the human G4 run left a global entry for
  `127.0.0.1:17448` there: so the scenario refuses the human run's ports (default
  17458/17460), asks the operator to check `codex mcp list` (read-only) first, records a
  finding when more than one Codex HTTP session connects, and the report's criterion 4
  requires exactly one before attributing Codex's traffic to the per-invocation
  registration.
- **G5** (`g5-provenance`): Claude cases C1-C6 through the channel server's own case trigger
  (C6 mid-turn), Codex cases X1-X6 through the app-server client. **The spoofing bodies reach
  the harness only through the channel server or the app-server client**; herdr types only
  the thread marker, a busy prompt and the fixed question, each checked to carry no body,
  frame marker or case identity. G5's verdict is FAIL and stays FAIL: the report only says
  whether a scripted run reproduced the human run's results. The Codex rows are scored per
  case (`--case X2.c2=f`), never as one judgement: X2 is the harness-dependent case, and X5's
  criterion-2 failure is set by the reconstructed client's framing, so it reproduces by
  construction and cannot stand in for the model's behavior. Claude's criteria
  were judged, in the human run, on a render extracted from Claude Code's own session log; a
  scripted run has only the wire and pane text, and every Claude row says so.
- Both report generators read the gate's pass criteria from the `oac-gates` reference **as
  committed at HEAD**, bound by a sha256 pin (`G4_CRITERIA_SHA256`, `G5_CRITERIA_SHA256`), and
  refuse to score if the reference was reworded or reordered. `--write` refuses anything but a
  `PASS` run from a clean, committed `tools/herdr/`, never overwrites, and writes the operator
  attestation unticked. A version other than PINS.md's last tested one is not refused (#216):
  it is a `VERSION WARNING` finding and the fixture entry says `version_matches_pin: false`.
  When a harness's sources disagree or a version moves mid-run, the record and run manifest
  are still written with a `VERSION WARNING`, but no fixture (operator decision on #216).

## L3 Beacon live leg

`l3-beacon` (L3b, #190) runs `docs/planning/decisions/L1-beacon-memory.md` §12 against a
machine where the operator has already installed Beacon. It is **not a gate** and records only;
L3c (`lib/l3-report.mjs`, #191) drafts §13 from the runs. Three phases share one 60-minute L3
box, declared by the baseline; the operator's own Beacon steps (install, and B7 teardown and
restore) happen outside the driver, between phases:

```bash
node tools/herdr/run.mjs --scenario l3-beacon --param phase=baseline --param beaconBin=<abs path> --out <baseline dir>
node tools/herdr/run.mjs --scenario l3-beacon --param phase=probe --param baselineRun=<baseline dir> --param beaconBin=<abs path> --out <probe dir>
# operator: `codex app-server daemon stop` (the probe leaves the daemon running with Beacon's
# [otel] config loaded), then the L1 §12 B7 teardown and restore
node tools/herdr/run.mjs --scenario l3-beacon --param phase=verify --param baselineRun=<baseline dir> --out <verify dir>
node tools/herdr/lib/l3-report.mjs --baseline <baseline dir> --probe <probe dir> --verify <verify dir>
```

`sync --print` output is scanned as it streams, with no size cap (with no state file Beacon
re-emits all history oldest first, so the probe session comes last); a sync that times out or
fails is `NOT RUN` for its poll path only. `beacon endpoint status --system` writes nothing but
has recorded side effects (`beacon.status.sideEffects`: `--version` probes of harness binaries on
PATH, loopback probes, an outbound GET only if enrolled).

The driver runs only a read-only Beacon allowlist (`beacon version`, `beacon endpoint status
--system`, `beacon endpoint {claude,codex} sync --print`; `sync --print` is write-free, cited
from source in the scenario header), or none with `--param beaconCli=off`. The probe values are
generated inside the scenario, redacted everywhere, and reach the harnesses only through a
scratch copy of `gate-servers/` whose `g5-cases.json` alone is augmented. The driver hashes
harness config and never writes it; it reads Beacon's runtime log read-only, and the probe
project's own Claude session file for entry types and flags only. B1, B5 and B6 are recorded
`NOT RUN` (operator decisions on #168). Not in CI (`ci.mjs` `CI_SCENARIOS` excludes it). LIVE
STATUS: UNVERIFIED.

## gate-servers/: reconstructions, not the originals

The G4 and G5 spike servers (and G5's client and case table) were throwaway spike code and
were never committed (`docs/planning/gates/G4-result.md`, `G5-result.md`). K8 needed servers
to replay those gates, so it **rebuilt** them here as permanent test tooling, from the result
documents' architecture descriptions and the committed fixtures' wire shapes:

- `g4-server.mjs`: one process, a stdio channel surface (legacy, or modern-only with
  `G4_STDIO_MODERN=1`) and a dual-era HTTP surface; `g4_echo`, `g4_relay_to_claude`,
  `wake.trigger`.
- `g5-channel.mjs`: the Claude channel server, with the S1 key-table check, the pre-send
  refusal and its explicit hazard path; `case.trigger`.
- `g5-codex.mjs`: the app-server client (WebSocket over `codex app-server proxy`), framing
  per `docs/planning/decisions/C6-trust-rendering.md` §5.
- `g5-cases.json`: every body, meta map and header value, copied from the committed fixtures.

They are **not** the programs that produced the human-run fixtures and cannot be verified
byte-identical or behavior-identical to them. The self-test replays each committed fixture's
own requests (and case triggers) against the reconstruction and requires every response, push
and frame to match the fixture, volatile ids aside; that shows the recorded wire shapes are
reproduced, not that the originals behaved the same anywhere the fixtures do not reach. Every
comparison record says this at its top. Committing them is a deliberate, documented exception
to `oac-gates`' "nothing durable is built on spike code" rule (issue #131). Nothing outside
`tools/herdr/` may use them; the production OAC servers are built separately (Stage 3-4).

## Reuse contract: Stage 4 and Stage 5 opt-in tests

Backlog tasks G4 (Claude adapter) and G7 (Codex adapter) need provider-integration tests
against real harnesses, and H1 (end-to-end) and H4 (cross-platform CLI smoke) need live,
OAC-enabled sessions on three platforms. They reuse this driver as follows. This is the
contract as K8 leaves it; the open questions at the end are not settled by it.

1. **The launch is a parameter, never hardcoded.** A Stage 4/5 scenario takes each harness's
   OAC-enabled launch command, as backlog task G11 documents it, from the driver's `--launch`
   (a JSON argv; `launch[0]` is the herdr agent kind, the rest passes through unchanged) and,
   for a second harness, a scenario parameter holding a JSON argv (the pattern
   `g4-mcp-dual-era`'s `codexLaunch` sets, validated before anything starts). When G11's
   commands change, the test's parameters change, not its code. The gate scenarios above keep
   their own gate's verbatim launch, because they replay a recorded human run.
2. **Tests live under top-level `tests/integration/`**, never under `adapters/`, `core/`,
   `cli/`, `transports/` or `spec/`. A test there invokes the driver as a separate process
   (`node tools/herdr/run.mjs --scenario <path> ...`) and reads its `run-manifest.json` and
   captures; it never imports driver code into a product crate or module, and no product
   manifest (and no test manifest that a default build compiles) may reference
   `tools/herdr/`. The scenario module itself may live under `tests/integration/` and be
   passed to `--scenario` by path. `scripts/check-herdr-containment.mjs` check 9 enforces
   this (#146): `tests/integration/` may name herdr and run the driver by path, including
   from a script entry in its own manifest, but it fails if anything under it imports,
   compiles in, or symlinks to driver code. It also fails if a product path reaches
   `tests/integration/`, or a manifest outside it names it or a `tests/` glob or member.
   The exact rules are in `oac-boundaries` `references/mechanical-checks.md`, check 9.
3. **Opt-in and pinned.** Reached only by an explicit, separately invoked target, never the
   default `test` run; each test names its exact transport and herdr pins from
   `docs/planning/PINS.md` (`09-test-strategy.md` §4), and records the Claude Code and Codex
   versions it ran on. Those float: a version other than PINS.md's last tested one, or below its
   minimum, is a `VERSION WARNING` finding, never a stop and never a CI failure (#216). A herdr timeout or an expired box
   is `NOT RUN`, never a failure and never a pass, and nothing re-runs automatically.
4. **Evidence is the wire and verbatim pane text.** An OAC adapter's own transcript (as the
   gate servers' transcripts here) plus `agent read` output. herdr's agent state only
   schedules the next read. Input reaches a harness only as an operator would type it;
   anything the protocol under test must carry comes from OAC's side (a channel
   notification, an app-server request), never typed by herdr. If a test only works because
   herdr injects it, that is a boundary-13 finding, not a script.
5. **Consent dialogs.** Since #196 (2026-09-30) the driver accepts Claude Code's
   workspace-trust, MCP-server and dev-channels dialogs in dev/test runs by default,
   recorded as `driver`. The G11 confirmation is different: it is presented as a feature,
   and G11's acceptance says it is "not bypassed or automated away". No recorded operator
   decision lets a test driver-accept it. A test whose launch shows it runs with
   `--param accept=human`, and so is never fully unattended, until K8 and a recorded
   operator decision say otherwise (`scripted-runs.md` "Operator-consent dialogs").
6. **Credentials.** The harnesses serve every model turn from their own sign-in. Nothing a
   test adds may read a harness's credential files or ask an OS credential store for
   anything, or edit a harness's config: pass configuration per invocation (as G4's Codex
   launch does). Copy the tracer pattern (`test/fs-trace.mjs`, used by the G2, G4 and G5
   self-test cases) to prove it from a trace rather than assert it.
7. **Linux, macOS and Windows.** The driver and scenarios avoid POSIX-only calls on the
   harness path: the Codex CLI is run through `cmd.exe` on Windows, pane process argv is read
   from `/proc` (Linux), `ps` (macOS) or `Win32_Process` (Windows), and quoting follows the
   pane shell (`lib/pane-shell.mjs`). What is verified today is Linux only, and only against
   test doubles: the self-test's lifecycle half needs POSIX `sh`, K1's live leg has not run,
   and herdr's own Windows and macOS support is its documentation's claim (K1). A Stage 4/5
   test on macOS or Windows is UNVERIFIED until it runs live there.
8. **Records.** A Stage 4/5 run records its run manifest and redacted captures as any
   scripted run does; whether and where its results are committed is decided by that stage's
   own task, not here.

**Open questions K8 does not settle** (ADR-001 boundaries 4 and 13; recorded for a decision,
not resolved silently):

- `tests/integration/` is DESIGN's *suggested* layout; `oac-testing` says the actual test
  layout is confirmed by Stage 3 output (F8/F9). Point 2 above follows K8's acceptance, ahead
  of that confirmation.
- Point 5 means an unattended opt-in CI run of any test that shows the G11 confirmation is
  impossible under the current rule (#196 did not change G11).

## Operator setup (Windows, and Claude Code permissions)

Live runs are driven by `run.mjs`, started locally by an agent (once the operator has
added the allow rules in step 4 below to their own `.claude/settings.local.json`) or by the operator.
Automating these runs is why herdr was added (operator decision, #187). The human attends
only for what a driver must not do: signing in to the harnesses and granting elevation.
Since #196 the driver accepts Claude Code's dialogs itself in dev/test runs (see "Dialogs"
above), G1 included. A human accepts only in a run that must meet a consent-step criterion:
G1 criterion 5 with `--param accept=human`, and the G11 confirmation. Any dialog the driver
refuses (tool permission, Codex) also needs `accept=human`. Live runs stay out
of the default CI suite. These are the settings a run needs; none is committed, because
they are per-machine and belong to the operator.

**1. herdr on PATH.** The Windows installer puts the binary under
`%USERPROFILE%\.herdr\packages\standalone\releases\<version>-x86_64-pc-windows-msvc\herdr.exe`
and does not add it to PATH. Add that directory to the user PATH (or prepend it in the shell
that runs the driver), then check `herdr --version` prints the pin in `docs/planning/PINS.md`.
The driver reads that pin from PINS.md as committed at HEAD. An uncommitted edit to the
`herdr (test tooling)` row ends the run `NOT RUN` and is not applied; any other uncommitted
PINS.md edit is a finding only (#139). Commit or revert a herdr-row edit before `--self-test`.

**2. Isolation is the driver's job.** Set nothing. `run.mjs` starts its own named, headless
session (`oac-k-<scenario>-<stamp>-<rand>`), writes its own `herdr-config.toml` (with
`[update]` `version_check = false` and `manifest_check = false`) into a fresh scratch
directory, and strips any inherited `HERDR_*` variable from herdr's environment. A
`HERDR_SESSION` or `HERDR_CONFIG_PATH` you export has no effect on a run (#153). It also
strips an enclosing Claude Code session's variables (`CLAUDECODE`, `CLAUDE_CODE_*`,
`CLAUDE_PID`, `CLAUDE_AGENT_SDK_*`, `CLAUDE_PREVIEW_*`, `ANTHROPIC_BASE_URL`), so a harness
started from a shell inside Claude Code doesn't run as that session's child or through its
relay. It records their names and a finding (#163). No other `ANTHROPIC_*` variable is
touched.
An agent may therefore start the driver from its own shell; a standalone terminal works
too. When a scenario waits for you to accept a dialog, it prints the session name and the
command to attach (`herdr session attach <session>`). Never run herdr's command that
writes hooks into harness config, and do not commit raw pane or env captures.

**2a. Unattended runs.** Every run starts Claude Code in a fresh scratch project unless told
otherwise, so the folder-trust dialog, which preselects "No, exit", comes up every time.
**Amended 2026-09-30 (#196):** under `accept=driver` the driver accepts it by moving to
"Yes, I trust this folder", with the move verified by a read before Enter. Before #196 the
driver refused it (#156), and a run whose dialog nobody accepted ended `NOT RUN`. That
happened in L3 probe run 3 on 2026-09-30, after 300000 ms. `--param projectDir` (G1) remains
an optional way to avoid the dialog. A `g1-claude-wake` run on its default (`accept=driver`)
needs nobody at the keyboard, but its driver accept of the dev-channels dialog is never scored
as meeting G1 criterion 5 (`scripted-runs.md` "Operator-consent dialogs"). Since #199 the
driver also accepts Codex's workspace-trust dialog (G2, G4, G5, L3), which adds a trust entry to
your `~/.codex/config.toml` each run (#206, above). Any other Codex dialog is never
driver-accepted: if real Codex shows one in the scratch project, the run ends `NOT RUN`, and
it needs `accept=human`.

**2b. Line endings.** A Git for Windows checkout (`core.autocrlf=true`) is fine. The driver
compares working-tree files with HEAD in git's normalized form, as `git status` does (#152).

**3. Symlinks (self-test only).** `node tools/herdr/run.mjs --self-test` creates symlinks.
On Windows that needs Developer Mode (Settings > System > For developers) or an elevated
shell; without it the G1 git test fails with `EPERM` on `symlink`. The lifecycle half of the
self-test needs POSIX `sh` and is skipped on Windows.

**4. Claude Code permission rules, when an agent runs the live steps.** Claude Code's
permission prompts and its auto-mode classifier may refuse a command that launches a real
logged-in harness. To let an agent run the herdr commands, add allow rules to
`.claude/settings.local.json` (per operator; do not commit it), not `.claude/settings.json`:

```json
{
  "permissions": {
    "allow": [
      "Bash(herdr:*)",
      "Bash(node tools/herdr/run.mjs:*)",
      "Bash(node tools/herdr/lib/*:*)"
    ]
  }
}
```

Rules match the command prefix, so `herdr` must resolve on PATH (step 1) and be the first
word of the command; env assignments and `cd ... &&` prefixes make a rule miss. Keep them
narrow: do not allow `Bash(claude:*)` or `Bash(codex:*)`. The harness is started by herdr,
and its dialogs are answered as "Dialogs" above says. A rule does not
override a classifier denial in auto mode; if one is denied, run the step yourself or
switch the session's permission mode.

## Self-test

```bash
node tools/herdr/run.mjs --self-test
```

Unit checks in-process, then lifecycle runs of every scenario through `run.mjs` against
`test/fake-herdr.mjs`, `test/fake-claude.mjs`, `test/fake-codex.mjs` and (for `l3-beacon`)
`test/fake-beacon.mjs`. The doubles are this repository's inventions; the self-test proves the
driver's and the scenarios' behavior, never herdr's, Claude Code's, Codex's or Beacon's.
