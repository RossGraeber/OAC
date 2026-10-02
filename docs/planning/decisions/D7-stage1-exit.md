# D7 — Stage 1 exit: G1-G5 verdicts and the Stage 2 go/no-go

- **Date:** 2026-10-02
- **Issue:** #40 (D7, Epic D #4). Operator decision on #40, 2026-10-02: D7 proceeds once the
  C13 G5 Codex re-run is recorded, without waiting for the remaining herdr re-runs (G2 #130,
  G4/K8 Claude regression #131, K1 Linux leg #124). Those check test tooling and cannot
  change any gate verdict (https://github.com/RossGraeber/OAC/issues/40).
- **Gate:** Gate S1 (`docs/planning/v0.1/10-stages.md` §5).
- **Status:** **DRAFT — waits on PR #231 (the C13 G5 Codex re-run record) and its operator
  attestation.** Every `<pending #231 merge: …>` token below is filled in by the
  orchestrator after #231 merges. This record is written for the PASS branch only: C13
  closes and Stage 2 opens fully. If the attested G5 Codex verdict is anything other than
  PASS, this record does not land as written. The go/no-go in §5 has to be rewritten.
- **Skills:** `oac-gates`, `oac-evidence`, `oac-boundaries`.

This record is the published Stage 1 exit decision that `10-stages.md` §5 "Exit artifacts"
asks for. It does not restate gate evidence. Each gate's `docs/planning/gates/G<n>-result.md`
stays authoritative for its verdict, date, pins and transcript summary
(`docs/planning/v0.1/02-gating-findings.md` "Precedence"). This file adds four things: the
spike-code quarantine inventory, the Gate S1 checklist with evidence, the go/no-go
statement, and what continues after the exit.

Line citations are to `main` at `259d9ce` (the #242 merge, 2026-10-02) unless a line says
otherwise.

---

## 1. Gate verdicts at exit

| Gate | Closed verdict | Fallback taken | Result file |
|---|---|---|---|
| G1 Claude wake | **PASS** (Box C re-run, 2026-09-28, Claude Code `v2.1.283`) | none, no fallback exists | `docs/planning/gates/G1-result.md` L124 (Verdict), L145 (Fallback) |
| G2 Codex live inject | **PASS** (re-run 2026-09-26, Codex `0.157.1`, primary path: implicit daemon attach) | none, not needed | `docs/planning/gates/G2-result.md` L125-126 |
| G3 Zenoh local peer | **PASS** (gate level, 2026-10-02, #219; primary multicast path on Windows 11, Linux WSL2 and macOS 26.6.2 on a hosted VM) | none, not needed | `docs/planning/gates/G3-result.md` L182, L197 |
| G4 MCP dual-era server | **PASS** (re-run 2026-09-26, fresh 60-min box, not expired; primary single-process design) | none, not needed | `docs/planning/gates/G4-result.md` L180, L189 |
| G5 Provenance | <pending #231 merge: G5 Codex verdict> (gate-level verdict, Claude leg PASS 2026-09-27 per #220 ruling 3; Codex leg from the C13 §11 E1 re-run) | none, no fallback exists | `docs/planning/gates/G5-result.md` <pending #231 merge: G5-result.md verdict line> |

The G5 row was **FAIL** (2026-09-27; Codex criteria 2/3 f, Claude all criteria x) until the
C13 re-run. That FAIL was already a closed verdict, so it never blocked D7 itself
(`10-stages.md` §5 Gate S1 criterion 1). It did block Stage 2's interface freeze for Codex's
provenance framing until C13 closed (`10-stages.md` §5 go/no-go, third bullet). The C13 run
record is `docs/planning/gates/herdr-runs/G5-c13-2026-10-02.md` (PR #231, branch
`herdr/220-c13-g5-codex-framing`). Its generator outcome is "C13 outcome: PASS", agent-scored
under #220 ruling 2, and the operator attests it: <pending #231 merge: attestation commit and
G5 Codex verdict link>.

## 2. Spike-code quarantine inventory

The throwaway rule (`.claude/skills/oac-gates/SKILL.md` "Throwaway rule") and #40's fourth
acceptance item require all spike code to be discarded or clearly quarantined, with none of
it becoming Stage 3 code by default.

### 2.1 Committed, quarantined: 15 `*.throwaway-quarantined` files

Every file is under `docs/planning/gates/fixtures/`, a documentation path that no later stage
builds in (`docs/planning/v0.1/07-repository-and-dependencies.md` §1 layout). The
`.throwaway-quarantined` suffix stops any of them from being picked up as a `.mjs`, `.py` or
`.sh` source by tooling. Each one has an entry in `docs/planning/gates/fixtures/MANIFEST.json`
(15 `path` entries ending `.throwaway-quarantined`). The blob ids are from
`git ls-files -s` at `259d9ce`.

| Path (under `docs/planning/gates/fixtures/`) | Git blob | Spike / gate | Where it is described |
|---|---|---|---|
| `g1-claude-wake/channel-server.mjs.throwaway-quarantined` | `7a8bd420` | G1 (D1) channel server; Box C ran it unmodified | `gates/G1-result.md` (Box C transcript summary, original run) |
| `g2-codex-inject/client.mjs.throwaway-quarantined` | `6ac20c6e` | G2 (D2) second app-server client | `gates/G2-result.md` (second client) |
| `g3-zenoh-peer/peer.py.throwaway-quarantined` | `2ddfa005` | G3 (D3) Zenoh peer | `gates/G3-result.md` (command transcript summary) |
| `g3-zenoh-peer/run_matrix.py.throwaway-quarantined` | `5f680dbb` | G3 matrix driver (6 scenarios × 3 repetitions) | `gates/G3-result.md` |
| `g3-zenoh-peer/extras.py.throwaway-quarantined` | `056d2023` | G3 three-peer extras | `gates/G3-result.md` |
| `g3-zenoh-peer/verify.py.throwaway-quarantined` | `098fc720` | G3 link checker | `gates/G3-result.md` |
| `g3-zenoh-peer/run_linux.sh.throwaway-quarantined` | `f6b300cd` | G3 Linux (WSL2) launcher | `fixtures/MANIFEST.json` entry only |
| `g3-zenoh-peer/run_linux_extras.sh.throwaway-quarantined` | `7afb4e9b` | G3 Linux extras launcher | `gates/G3-result.md` |
| `g3-zenoh-peer/run_windows_extras.sh.throwaway-quarantined` | `7b62ebfd` | G3 Windows extras launcher | `gates/G3-result.md` |
| `d6-claude-protocol/claude-d6-channel.mjs.throwaway-quarantined` | `be30aa97` | D6 (#39) Claude fixture-capture channel server | `fixtures/MANIFEST.json` entry; its transcript `d6-claude-protocol/transcript-2026-09-28.jsonl` L1 names it as the tool |
| `d6-codex-protocol/client.mjs.throwaway-quarantined` | `57630f36` | D6 Codex app-server client | `d6-codex-protocol/README.md` |
| `d6-codex-protocol/run-t5.mjs.throwaway-quarantined` | `83e713e5` | D6 Codex T5 attempt-2 orchestrator | `d6-codex-protocol/README.md` |
| `d6-codex-protocol/redact.mjs.throwaway-quarantined` | `195261fd` | D6 redaction | `d6-codex-protocol/README.md`; `gates/G1-result.md` |
| `d6-codex-protocol/hash-tree.mjs.throwaway-quarantined` | `39999fde` | D6 schema hash method | `d6-codex-protocol/README.md` |
| `d6-codex-protocol/validate-frames.mjs.throwaway-quarantined` | `48dfecc9` | D6 frame validator | `d6-codex-protocol/README.md` |

**Who still runs a quarantined file, and how.** Three consumers, none of them on a product
path:

- `.github/workflows/g3-macos-hosted.yml` (dispatch-only, #219) copies the G3 `*.py`
  files unchanged into the runner's temp directory and logs both sha256 sums (L66-74).
- `tools/herdr/scenarios/g1-claude-wake.mjs` L448-463 runs a scratch copy of the G1
  channel server. It refuses to run if the blob at HEAD differs from
  `COMMITTED_SERVER_SHA256` (`tools/herdr/lib/g1.mjs` L26-33).
- `tools/herdr/scenarios/g2-codex-inject.mjs` L456-471 runs a scratch copy of the G2
  client under the same guard (`tools/herdr/lib/g2.mjs` L34-42).

In all three, the quarantined file is executed as recorded evidence (a replay), never
imported. `tools/herdr/test/g1-tests.mjs` L334 and `tools/herdr/test/g2-tests.mjs` L271
assert that the scenarios never import it.

### 2.2 Discarded, never committed

- **G4:** `g4-server.mjs` was kept in the spike scratchpad only (`gates/G4-result.md`
  L283-284).
- **G5:** `g5-channel.mjs`, `g5-codex.mjs`, `extract-claude-render.mjs`, `redact.mjs`,
  `cases.json`, `RUN-SHEET.md`, `S0-notes.md` and their self-tests stay in the scratchpad
  only (`gates/G5-result.md` L404-409).

### 2.3 The one documented exception: `tools/herdr/gate-servers/`

`tools/herdr/gate-servers/g4-server.mjs`, `g5-channel.mjs`, `g5-codex.mjs` and
`g5-cases.json` are committed. They are **reconstructions** built for K8 (#131) from the
result documents and the committed fixtures' wire shapes, not the original spike programs.
Committing them is a deliberate, documented exception to the throwaway rule
(`tools/herdr/README.md` "gate-servers/: reconstructions, not the originals", L237-260;
`docs/planning/gates/README.md` L49-51). PR #231 extends `g5-codex.mjs` and `g5-cases.json`
for the C13 §11 arms, within the E1 allow-list.

The exception is contained mechanically:

- `tools/herdr/README.md` L259-260: "Nothing outside `tools/herdr/` may use them; the
  production OAC servers are built separately (Stage 3-4)."
- `scripts/check-herdr-containment.mjs` (`oac-boundaries` check 9) fails on any `herdr`
  match under `adapters/`, `core/`, `cli/`, `transports/` or `spec/`, and on any manifest
  outside `tools/herdr/` that references it. It runs in CI
  (`.github/workflows/boundary-lint.yml`). Result at this change: PENDING, 0 violations
  (§3, criterion 5).
- `tools/herdr/` is dev/test tooling, never shipped (`docs/planning/PINS.md` "herdr (test
  tooling)"). The reconstructions are not Stage 3 code. Stage 3-4 builds the production
  servers separately.

## 3. Gate S1 acceptance criteria (`10-stages.md` §5)

1. **Each of G1-G5 carries a closed verdict — holds once the G5 row is filled.** G1-G4 are
   PASS (§1 table, each with its result-file line). G5 was already closed at FAIL
   (2026-09-27, `gates/G5-result.md` L307). After the C13 re-run it is <pending #231 merge:
   G5 Codex verdict>. No gate exits at `NOT RUN`. G3's gate-level `NOT RUN` ended
   2026-10-02 (#219). G4's earlier out-of-box `NOT RUN` was superseded by the 2026-09-26
   re-run (`gates/G4-result.md` L467, "Re-run history").
2. **Every pass criterion was evaluated individually — holds.** Each result has a "Pass
   criteria evaluated" list, one entry per criterion in the gate's reference file:
   - `gates/G1-result.md` L41 (Box C, five criteria);
   - `gates/G2-result.md` L91 (`0.157.1` re-run);
   - `gates/G3-result.md` L122, per platform, all three platforms;
   - `gates/G4-result.md` L98, verbatim from `references/G4-mcp-dual-era.md`;
   - `gates/G5-result.md` L238, per provider.

   The C13 re-run scores Codex criteria 2 and 3 "each on its own", per arm and per trial
   (`gates/herdr-runs/G5-c13-2026-10-02.md` on #231, "C13 outcome").
3. **Where a fallback was taken, the path that passed and its cost are named — holds
   vacuously.** No gate took a fallback (§4).
4. **Fixtures exist for every exchange Stage 3's fakes need, captured live and
   credential-free — holds.** D6 (#39, closed 2026-09-28) records all five of its
   acceptance criteria as met. Claude fixtures cover initialize/negotiation, notification
   delivery, mid-turn queueing and a tool reply (`fixtures/d6-claude-protocol/`). Codex
   fixtures cover `initialize`/`initialized`, `thread/start`, `thread/resume`,
   `turn/start`, `thread/queue/add` and the event stream (`fixtures/d6-codex-protocol/`).
   Every fixture records its version and capture date, and none contains credentials
   (`docs/planning/STATUS.md`, the 2026-09-28 #39/D6 "Last updated" entry, L378-389 at
   `259d9ce`). `node
   scripts/check-fixture-manifest.mjs` passes at this change: 203 MANIFEST entries match
   203 committed fixture files. #231 adds 8 more `-herdr` fixtures with MANIFEST entries,
   and its own run of the check passes with 211 entries. The fixture-capture process
   Stage 3 consumes is `docs/planning/v0.1/09-test-strategy.md` §13.
5. **No spike code remains on a path a later stage will build in — holds.** None of
   `core/`, `adapters/`, `cli/`, `transports/`, `spec/` or `tests/` exists at `259d9ce`.
   The only tracked code outside `docs/`, `.claude/`, `.agents/` and `tools/herdr/` is
   `scripts/*.mjs` (repository checks) and `rust-toolchain.toml`. Spike code is
   quarantined or discarded as §2 lists, and the one exception is contained (§2.3).
   `node scripts/check-herdr-containment.mjs` at this change: "PENDING -- 0 violations; 7
   of 10 target(s) not built yet". PENDING is not a pass, and the check must be re-run once
   Stage 2-4 paths exist.

**#40's own acceptance items:**

- Each result carries gate id, pinned versions, date, transcript summary and verdict:
  holds, from the result-file template in `docs/planning/gates/README.md`.
- Every gate reads pass, fail or fallback-taken: holds, per criterion 1.
- Fallbacks taken are reflected into decisions and `02-gating-findings.md`: none were
  taken. The one design change, C13, is reflected in C6 §5.0 and C13 (#220).
- Spike code discarded or quarantined: holds, per §2.

The issue's checkboxes and Epic D's checklist are left for the operator to tick.

## 4. No fallback taken

**No Stage 1 gate took a fallback.** G1 has none, and its primary path passed. G2 passed on
the primary implicit daemon attach path, so the OAC-owned app-server plus `codex --remote`
fallback was not needed (`gates/G2-result.md` L126). G3 passed on the primary multicast
path on all three platforms. The fixed-endpoint mode was exercised as criterion 2 but was
not needed as a fallback (`gates/G3-result.md` L197). G4 passed on the primary
single-process design, so two entry points were not needed (`gates/G4-result.md` L189). G5
has no fallback.

C13 is not a gate fallback. It is the **design change** that `10-stages.md` §5's go/no-go
requires after a G5 FAIL ("requires a design change before Stage 2 freezes the interfaces,
not a workaround"). It amends C6 §5 to §5.0 (Option C, approved 2026-10-02 on #220). C13's
own internal fallback, Option A if arm C showed anchor confusion
(`decisions/C13-codex-provenance-framing.md` "Status", decision 1), <pending #231 merge:
confirm arm C passed and Option C stands, with no Option A fallback>. No gate verdict reads
`PASS (FALLBACK TAKEN)`.

## 5. Stage 2 go/no-go

**Go. Stage 1 exits, and Stage 2 (Epic E, #5, milestone M3) opens fully.**

- **Gate S1** holds on all five criteria (§3). Every gate verdict is closed, and none is
  `NOT RUN`.
- **G1 PASS:** the Claude adapter is not blocked.
- **G2 PASS:** v0.1 is a go for Codex on the primary path.
- **G3 and G4 PASS:** no fallback is carried into Stage 2.
- **G5:** <pending #231 merge: G5 Codex verdict>. With the Codex leg passing under C13
  §11, **C13 closes**. DESIGN acceptance criterion 6 is re-established for Codex, through
  the C6 §5.0 framing (Option C). The block on Stage 2's interface freeze for Codex's
  provenance framing (`10-stages.md` §5, third go/no-go bullet) is lifted.
- `10-stages.md` §6 entry criterion 2 ("Any G5 design change identified at Stage 1 is
  folded into the design **before** the interface freeze") is met: C6 §5.0 landed on
  2026-10-02 (#220), before any Stage 2 work.

Stage 2's §6 prerequisite decisions (3, 4, 5, 6, 8, 9) landed with Epic C (#3, closed).
Stage 3 onward stays blocked until Gate S2 (`10-stages.md` §6). The rule that no
substantial core or transport code starts before Stage 0 and Stage 1 complete is now met,
and Stage 3's own entry gate (S2) takes over.

## 6. Continuing and deferred items (not exit blockers)

These continue alongside Stage 2. None can change a gate verdict. The first three are
covered by the operator decision on #40.

| Item | What it is | Why it does not block D7 |
|---|---|---|
| #130 | G2 herdr driver scenario (Codex live inject, bounded `unknown` state) | Test tooling. G2's verdict is the human-run PASS on `0.157.1`. |
| #131 | G4 and G5 herdr scenarios, the K8 Claude regression, and the Stage 4/5 reuse contract | Test tooling. The K8 G5 Claude regression ran on 2026-10-02 as a non-verdict run (#231). |
| #124 | herdr evaluation (K1): the Linux and macOS legs | herdr's pin row has "Gates affected: none" (`PINS.md`). |
| #239 | herdr self-test: g1 #196 stuck-selection case leaves a pane process behind (WSL teardown race) | Self-test hygiene. Not a gate input. |
| #243 | herdr: Windows `splitCommandLine` mis-parses backslash-escaped quotes (G4 launch proof) | Driver bug. Not a gate input. |
| #244 | herdr: allowlist G4 `-c` override values | Driver hardening. Not a gate input. |
| #246 | herdr(g5): settle before the after-delivery read in the arms path (`agent_not_idle` race) | It made run `20261002T160321Z` NOT RUN (it did not consume E1). The scored run was unaffected. |
| K6 (#129) | Opt-in CI on operator-owned runners | **Deferred past v0.1** (operator decision on #129, 2026-10-02; `docs/planning/v0.1/12-deferred.md` §3). |

Open UNVERIFIED items carried out of Stage 1 stay in `docs/planning/STATUS.md` "Open
UNVERIFIED items" and `docs/planning/v0.1/11-risks.md`. This record does not close any of
them.

## 7. `02-gating-findings.md` regeneration (pending #231)

There is no generator script. `scripts/` holds only `check-fixture-manifest.mjs`,
`check-herdr-containment.mjs`, `check-skills.mjs`, `sync-agents-skills.mjs` and
`sync-backlog.mjs`, and none of them reads or writes `02-gating-findings.md`. The file is
regenerated by hand, from the five `gates/G<n>-result.md` files
(`docs/planning/gates/README.md` "Reconciliation with PLANNING-PROMPT.md §9 item 3";
`02-gating-findings.md` "Generated-summary notice" and §8). It was last regenerated with G3
(#219, `879416c`).

The G5 inputs are not on `main` yet: #231 carries the run record, and the orchestrator
writes the G5 verdict after attestation. So the regeneration is **not run in this change**.
After #231 merges and `gates/G5-result.md` plus the STATUS "Gate verdicts" G5 row carry the
attested verdict, the steps are:

1. **§2 Verdict summary, G5 row:** copy the STATUS "Gate verdicts" G5 cell for cell.
2. **§2 "Reason for `NOT RUN` on G3 (partially)":** add a dated note (2026-10-02, #40) that
   the G5 FAIL text is history, and give the attested G5 verdict with its link.
3. **§7 G5 provenance, Verdict bullet:** restate the G5-result.md verdict, date, timebox
   and the C13 run link. Keep the 2026-09-27 FAIL as history, as §3 does for G1.
4. Check that no other §2-§7 row disagrees with its result file. Only G5 should change.
5. Grep every file that states G5's verdict (`oac-gates` `references/writeup-pitfalls.md`
   "Derived docs") and sync it in the same change.

---

## Boundary and evidence pass

- No boundary in `oac-boundaries` is touched. This record decides no provider integration,
  credential use, transport vocabulary or model routing. The herdr exception (§2.3) is test
  tooling, contained by check 9.
- Every claim cites a repo file and line at `259d9ce`, or an issue or PR. There are no new
  external provider claims, so no new URL, version or retrieval-date citations and no new
  UNVERIFIED items.
