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
| `run.mjs` | The driver (K3): isolated herdr session, bounded waits, timebox, redaction, run manifest. `--self-test` runs every test below against test doubles. |
| `ci.mjs`, `runner-hooks/` | The opt-in CI entry point and runner hooks (K6). |
| `lib/` | Driver internals, and per-gate helpers and report generators (`g1*.mjs` K4, `g2*.mjs` K7, `g4*.mjs` and `g5*.mjs` K8, `gate-common.mjs` and `gate-report-common.mjs` shared by K8). |
| `scenarios/` | `smoke`, `g1-claude-wake` (K4), `g2-codex-inject` (K7), `g4-mcp-dual-era` and `g5-provenance` (K8). |
| `gate-servers/` | K8: the G4 and G5 gate servers, **reconstructed** (see below). |
| `test/` | The self-test and its test doubles (`fake-herdr.mjs`, `fake-claude.mjs`, `fake-codex.mjs`) and the file-access tracer (`fs-trace.mjs`). |

## Scripted gate re-runs

Each gate scenario replays the human-run gate spike through herdr and records the run; its
report generator scores every pass criterion against the human run's committed fixture and
writes a `docs/planning/gates/herdr-runs/G<n>-<YYYY-MM-DD>.md` record. **None of them is
verdict-bearing**: a gate's verdict comes only from its human-run procedure unless
`scripted-runs.md` "Verdict eligibility" says otherwise. **None has run live**: each is
exercised only against the test doubles, so every harness-facing behavior is UNVERIFIED until
an operator runs it (the commands are in each scenario's header comment).

```bash
node tools/herdr/run.mjs --scenario g4-mcp-dual-era --param accept=human --out <run dir>
node tools/herdr/lib/g4-report.mjs --run <run dir>                 # draft; add --write to record
node tools/herdr/run.mjs --scenario g5-provenance --param accept=human --out <run dir>
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
  `PASS` run with verified harness versions from a clean, committed `tools/herdr/`, never
  overwrites, and writes the operator attestation unticked.

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
   passed to `--scenario` by path.
3. **Opt-in and pinned.** Reached only by an explicit, separately invoked target, never the
   default `test` run; each test names its exact Claude Code, Codex, transport and herdr pins
   from `docs/planning/PINS.md` (`09-test-strategy.md` §4). A herdr timeout or an expired box
   is `NOT RUN`, never a failure and never a pass, and nothing re-runs automatically.
4. **Evidence is the wire and verbatim pane text.** An OAC adapter's own transcript (as the
   gate servers' transcripts here) plus `agent read` output. herdr's agent state only
   schedules the next read. Input reaches a harness only as an operator would type it;
   anything the protocol under test must carry comes from OAC's side (a channel
   notification, an app-server request), never typed by herdr. If a test only works because
   herdr injects it, that is a boundary-13 finding, not a script.
5. **Consent dialogs.** The G11 confirmation is presented as a feature and is never
   driver-accepted in any test: run with `--param accept=human` (the default in every
   scenario here). `scripted-runs.md` "Operator-consent dialogs" states the rule; changing it
   takes a recorded operator decision, and none exists. So a test whose launch shows that
   confirmation is never fully unattended.
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
   test on macOS or Windows is UNVERIFIED until an operator runs it there.
8. **Records.** A Stage 4/5 run records its run manifest and redacted captures as any
   scripted run does; whether and where its results are committed is decided by that stage's
   own task, not here.

**Open questions K8 does not settle** (ADR-001 boundaries 4 and 13; recorded for a decision,
not resolved silently):

- `tests/integration/` is DESIGN's *suggested* layout; `oac-testing` says the actual test
  layout is confirmed by Stage 3 output (F8/F9). Point 2 above follows K8's acceptance, ahead
  of that confirmation.
- `scripts/check-herdr-containment.mjs` scans the product paths, not `tests/`. Nothing
  mechanical yet stops a product crate from depending on a test under `tests/integration/`
  that in turn drives herdr, or a default build from compiling such a test. Whether check 9
  should grow a `tests/integration/` rule is open.
- Point 5 means an unattended opt-in CI run of any test that shows the G11 confirmation is
  impossible under the current rule.

## Self-test

```bash
node tools/herdr/run.mjs --self-test
```

Unit checks in-process, then lifecycle runs of every scenario through `run.mjs` against
`test/fake-herdr.mjs`, `test/fake-claude.mjs` and `test/fake-codex.mjs`. The doubles are this
repository's inventions; the self-test proves the driver's and the scenarios' behavior, never
herdr's, Claude Code's or Codex's.
