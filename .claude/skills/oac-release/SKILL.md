---
name: oac-release
description: Packaging checklist, license inventory, gate re-run rule, upgrade notes for preview surfaces, and deferred/risk lists for Stage 6 release hygiene. Load for work items labelled stage:6-release (Epic I, tasks I1-I4).
---

Stage 6 (`PLANNING-PROMPT.md` §8 Stage 6) closes v0.1 with docs, a license inventory, a
deferred-work list, a known-risk list, and upgrade notes for provider preview surfaces. This
skill holds the release procedure only. It does not restate the evidence standard
(`oac-evidence`), the ADR-001 boundaries (`oac-boundaries`), the test taxonomy (`oac-testing`),
or provider protocol detail (`oac-claude-channels`, `oac-codex-appserver`, `oac-mcp`,
`oac-zenoh`) — load those by name alongside this skill.

## 1. Packaging checklist (ADR-001 "Decision"; DESIGN "Goals")

Language and toolchain (Rust, single static binary) are the "Presumptive answer" of §5
decision 1, and `STATUS.md` lists the Rust toolchain as "not pinned | Stage 0". The
tool-shaped lines below (Windows MSVC/GNU targets) assume that presumption; rewrite them if
decision 1 reverses.

A release artifact must satisfy every line, per platform:

- [ ] One binary per platform, no runtime install required (I3 acceptance). ADR-001
      requires single-binary cross-platform packaging — one binary per platform, not one
      binary for every platform.
- [ ] One documented command starts the local deployment (DESIGN acceptance criterion 1;
      exact CLI command per §5 decision 11, undecided).
- [ ] No Docker, no Kubernetes, no cloud account, and no separately administered server
      required for normal local use (ADR-001 "Decision": "must not require Docker,
      Kubernetes, a cloud account, or a separately administered server for normal local
      use").
- [ ] Windows MSVC and GNU targets addressed explicitly, plus macOS and Linux (I3
      acceptance; §3.4 packaging note).
- [ ] Cross-platform CLI smoke run and passing on Windows, macOS, and Linux before
      publishing (DESIGN "Testing"; PLANNING-PROMPT §8 Stage 5 output) — use `oac-testing`
      for how the smoke suite is structured; this skill only gates on its result.
- [ ] Checksums and provenance published with the release artifacts (I3 acceptance).
- [ ] Actual binary size recorded against the ~5-15 MB estimate in
      `docs/planning/STATUS.md` "Open UNVERIFIED items" (§3.4) — close or carry forward.
- [ ] The full CI-default test tier, plus the opt-in provider-integration tier, run and
      pass before publishing (I3 acceptance) — see `oac-testing` for tier definitions.

## 2. License inventory procedure (§5 decision 12; I2)

Repository license, as recorded in `LICENSE`: **Apache License, Version 2.0**.

For every third-party dependency the build actually pulls in ("crate" is §5 decision 12's own
term and presumes the §1 Rust presumption; not aspirational):

1. List name, exact version, license identifier, and the one-line reason it is needed.
2. State compatibility with Apache-2.0 for the shipped binary (permissive licenses are
   compatible by inspection; state so).
3. **Flag copyleft explicitly** — name the dependency, the copyleft license, and whether it
   is (a) not shipped, (b) shipped but under a compatible dual/exception license, or (c) a
   blocker. A blocker fails the release; do not ship it.
4. Record Zenoh's dual EPL-2.0 / Apache-2.0 licensing explicitly and which term OAC relies on
   (§3.4: "dual EPL-2.0 / Apache-2.0"; I2 acceptance).
5. Record any reused Codex crates (`app-server-client`, `app-server-protocol`,
   `app-server-transport`) with their Apache-2.0 provenance (§3.2: "Repo license
   Apache-2.0... reusable under that license"; I2 acceptance).
6. Generate or refresh the `NOTICE` file from this inventory (I2 acceptance) and confirm it
   is current with the dependency set actually shipped. `NOTICE` must carry the actual
   copyright holder name and year — never the `LICENSE` appendix's placeholder line
   (`Copyright [yyyy] [name of copyright owner]`), which is a template, not a record.
7. Wire the CI check that fails on an unapproved license (I2 outcome) rather than relying on
   manual review at release time.

Never invent a dependency this repo has not chosen — if a crate is not yet selected in
`docs/planning/v0.1/07-repository-and-dependencies.md` or the workspace manifest, that is a
gap to report, not a name to guess.

## 3. Gate re-run rule — hard release gate (§4, §8 Stage 6)

> "Gates are re-run whenever a pinned provider version changes." (PLANNING-PROMPT §4)

This is not advisory at release time:

- [ ] Diff the pins a release ships against `docs/planning/STATUS.md` "Pins" as they stood
      at the last gate run. If any row of STATUS.md Pins moved (Claude Code, MCP, Codex CLI,
      Zenoh, ACP, or Rust toolchain), every gate that pin affects (G1-G5; see `oac-gates` for
      which pin feeds which gate) must be re-run before this release ships.
- [ ] Re-run those gates using `oac-gates`' gate-result template and timebox policy — do not
      hand-wave a re-verification.
- [ ] Update `docs/planning/STATUS.md`'s Gate verdicts table **and** its Pins table in the
      same change that updates the release. A verdict and a pin that disagree are both wrong
      until reconciled.
- [ ] **A release cannot ship on a stale verdict.** If a gate affected by a moved pin has not
      been re-run, block the release — do not carry forward the old PASS.

## 4. Upgrade notes for preview/experimental surfaces (§10 "Label every provider surface")

Every v0.1 surface labelled research preview or experimental gets an upgrade note. Use §10's
full vocabulary — supported / research preview / experimental / undocumented — and treat
`docs/planning/v0.1/01-capability-matrix.md` (§9 item 2) as the authoritative enumeration of
every surface and its label; this skill's table is only the §3-derived starting set, not a
completeness claim:

| Surface | Label | Baseline (not yet pinned) | Gates to re-run on provider move |
|---|---|---|---|
| Claude Code Channels | research preview | Claude Code v2.1.232+; permission relay v2.1.234+ | G1, G5 |
| Codex App Server (per-method gating; live-session inject specifically) | experimental | `@openai/codex` 0.154.0 (now floating; see PINS.md) | G2, G5, G4 |

`STATUS.md` "Pins" opens with "None of these are confirmed pins. They are the pre-verified
baseline recorded in PLANNING-PROMPT.md §3, retrieved 2026-09-15." Stage 0 must pin these
before any upgrade note built on them is trustworthy. Candidate rows the capability matrix
must also carry, not yet labelled above: Zenoh surfaces gated behind the `unstable` flag, and
Zenoh public-key auth (§3.4: semantics undocumented, UNVERIFIED).

For each surface, the upgrade note states, without restating the protocol detail itself
(that lives in `oac-claude-channels` / `oac-codex-appserver`):

- [ ] What may break when the provider moves — point at that surface skill's `## Pin`
      section rather than re-describing the wire behavior here.
- [ ] Which pin the note was verified against (the exact baseline/version in `STATUS.md`
      "Pins" at release time, not "latest").
- [ ] Which gates must be re-run when this surface's provider moves (table above; I4
      acceptance) — re-verify against `oac-evidence` and see `oac-gates` for how to run one.

## 5. Deferred-work and known-risk lists (§9.12, §9.13)

**Deferred (`docs/planning/v0.1/12-deferred.md`):** every entry in the Defer paragraph of
ADR-001 "## v0.1 scope" (second paragraph) and every DESIGN "Non-goals" entry restated as an
explicit "not in v0.1" line with its reason. Do not drop an entry silently at release — if
it shipped anyway, that is an ADR amendment, not a quiet removal from the list.

**Known risks (`docs/planning/v0.1/11-risks.md`):** ranked by ability to invalidate v0.1.
Each risk carries an early-warning signal (what an agent or operator would observe first) and
a response (what happens when that signal fires). Do not add a risk without both fields.

**Open UNVERIFIED items:** point at `docs/planning/STATUS.md` "Open UNVERIFIED items" rather
than copying it here — that list changes independently of this skill. Every item still open
at release time is carried into the shipped risk list (I4 acceptance: "Any UNVERIFIED item
still open at release is listed, not quietly dropped"); none may be silently promoted to
verified without the evidence `oac-evidence` requires.

## 6. Release checklist (run end to end, in order)

- [ ] Confirm `docs/planning/STATUS.md` shows Stage 5 closed and no `Blocked` note applies
      to Stage 6.
- [ ] Re-run any gate whose pin moved since its last verdict (§3); update `STATUS.md` Gate
      verdicts and Pins tables in the same change.
- [ ] Run the packaging checklist (§1) on Windows, macOS, and Linux artifacts.
- [ ] Complete the license inventory (§2); regenerate `NOTICE`; confirm the unapproved-
      license CI check is green.
- [ ] Write or refresh upgrade notes (§4) for Claude Code Channels and the Codex App Server
      experimental methods, citing the current pins.
- [ ] Refresh `docs/planning/v0.1/12-deferred.md` against the current ADR-001 "## v0.1 scope"
      Defer paragraph and DESIGN "Non-goals" text.
- [ ] Refresh `docs/planning/v0.1/11-risks.md`, ranked, each with signal and response; carry
      forward every open item from `STATUS.md` "Open UNVERIFIED items".
- [ ] Refresh user/contributor docs (I1): install and launch guide per harness, the
      untrusted-peer-content warning stated prominently (not a footnote), research-preview
      status with pinned versions stated, contributor guide routing labels to skills.
- [ ] Walk the install-and-launch path end to end using only the published docs — a new
      user gets two harnesses talking by following the docs alone (I1 acceptance) — before
      publishing.
- [ ] Run the full CI-default test tier plus the opt-in provider-integration tier; record
      the result.
- [ ] Publish checksums and provenance with the artifacts; record actual binary size against
      the STATUS.md estimate.
- [ ] Update `docs/planning/STATUS.md` "Current stage" to reflect the release.

## 7. Exit criteria for a release work item

- [ ] Every §1 packaging line is checked for every shipped platform.
- [ ] The license inventory is complete, copyleft is flagged (or none exists), and `NOTICE`
      is current.
- [ ] No gate affected by a moved pin is running on a stale verdict; `STATUS.md` verdicts and
      pins were updated in the same change as the release.
- [ ] Every research-preview/experimental surface in the table above has an upgrade note
      naming its pin and what the user must do on provider movement.
- [ ] `11-risks.md` and `12-deferred.md` are current, ranked/reasoned respectively, and every
      open UNVERIFIED item from `STATUS.md` is represented.
- [ ] A new user can get two harnesses talking by following the published docs alone (I1
      acceptance) — walked end to end, not assumed.
- [ ] The relevant Epic I task's own Acceptance checklist is fully satisfied.

## Where the content lives

- `docs/planning/PLANNING-PROMPT.md` §4 (gate re-run rule), §8 Stage 6, §9.12-.13, §10
  (surface labelling), §3.1/§3.2 (preview/experimental surface facts).
- `docs/planning/ADR-001.md` "## v0.1 scope" (Defer paragraph), "## Decision" (packaging
  constraint sentence) — deferred-work source and packaging constraints.
- `docs/planning/DESIGN.md` "Goals", "CLI / supervisor", "Testing", acceptance criteria.
- `docs/planning/STATUS.md` — current pins (unconfirmed baseline), gate verdicts, open
  UNVERIFIED items.
- `docs/planning/v0.1/01-capability-matrix.md` — authoritative per-surface label enumeration
  (§9 item 2); this skill's §4 table is only the §3-derived starting set.
- `docs/planning/backlog/05-tasks-GHIJ.json` — Epic I tasks I1-I4, acceptance checklists.
- `LICENSE` — repository license (Apache-2.0).
- `oac-gates` — gate-result template and timebox policy, for re-runs under §3.
- `oac-evidence`, `oac-boundaries`, `oac-testing` — guardrails and taxonomy, not restated
  here.
- `oac-claude-channels`, `oac-codex-appserver` — the pin sections the upgrade notes point at.
