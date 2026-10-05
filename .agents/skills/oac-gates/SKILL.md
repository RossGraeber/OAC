---
name: oac-gates
description: Gate-result template, timebox policy, fixture capture procedure, one reference per gate (G1-G5), and herdr scripted-run rules. Load for any work item labelled type:spike or any gate:* label (D1-D7 in Epic D).
---

Stage 1 (PLANNING-PROMPT.md §8) decides five go/no-go gates through throwaway spikes before
any substantial core or transport code is written. This skill holds the procedure that
governs every gate work item. It does not restate `oac-boundaries` or `oac-evidence` — load
those too; link here.

## The rule: never "probably"

PLANNING-PROMPT.md §4: "The build phase may not proceed past a gate on a 'probably'." A gate
work item is not done when the spike "seems to work." It is done when a written result exists
with an explicit verdict: **PASS**, **PASS (FALLBACK TAKEN)**, **FAIL**, or **NOT RUN**. There
is no fifth state, and there is no plain PASS when only the fallback path succeeded — use
`PASS (FALLBACK TAKEN)` for that case (D7: "Every gate reads pass, fail, or fallback-taken").
An agent that is unsure runs the spike further within the timebox, or records `NOT RUN` with
the specific blocker — it does not write "probably passes" into the result and move on.

A move of a fixed pin (MCP revision, `rmcp`, Zenoh, Rust toolchain) invalidates the gates
its row names: the `docs/planning/STATUS.md` row reverts to `NOT RUN` until re-run.
**Harness versions are not pins (#216, 2026-10-01).** Claude Code and Codex (CLI, wire,
daemon) float: PINS.md records a minimum and a last tested version. A different version, or
one below the minimum, is a warning: it never stops a run, never makes it `NOT RUN`, never
blocks CI and never by itself invalidates a verdict. Record the version the gate ran on.

## Gate-result template

Copy this verbatim into the gate result. This is the base template; results land in
`docs/planning/gates/G<n>-result.md`, one file per gate, each extending this template
with additional fields (`Driver:` among them) — see `docs/planning/gates/README.md`
§Gate-result template for the extended form and the naming convention. The same change
that writes a gate result also updates the verdict row for that gate in
`docs/planning/STATUS.md`'s Gate verdicts table. Do not land a gate result without
updating STATUS.md in the same change. `docs/planning/v0.1/02-gating-findings.md`
(PLANNING-PROMPT.md §9 item 3) is not hand-authored from this template directly — it is
assembled from the five `G<n>-result.md` files when Epic A writes the output package.

```markdown
### G<n> <name>

- **Gate id:** G<n>
- **Pinned version(s):** <exact versions the spike ran against, e.g. Claude Code v2.1.285,
  Codex 0.159.3, Zenoh 1.10.1 — never "latest">
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

Evaluate every pass criterion listed in the gate's reference file individually — a gate does
not pass on a majority of its criteria. If a fallback exists for the gate (see the table
below) and the primary path fails, run the fallback and record which path passed.
Every finding rests on cited evidence (fixture, wire frame, pane capture, hash, rule text) or
is UNVERIFIED. An operator's attestation is never the basis; evidence against it is a finding (#252).

## Timebox policy

Every spike is timeboxed before it starts. Set the box when opening the gate work item; do
not extend it mid-spike.

- The spike stops at the timebox regardless of how close it looks to succeeding.
- The finding is recorded **as-is** — whatever pass criteria were actually confirmed or
  refuted at that point.
- The gate verdict becomes the honest one given what was confirmed: `FAIL` if a go/no-go
  criterion is unmet on every path attempted, `PASS (FALLBACK TAKEN)` if only the fallback
  path was confirmed within the box, `NOT RUN` if the spike could not even be executed
  (blocked on an external dependency, etc.; never a harness version), never a plain `PASS` on an
  incomplete run.
- **An expired timebox is a result, not a licence to keep going or to guess.** Extending
  "just a little more" or writing a passing verdict because the remaining criteria "should"
  hold is exactly the "probably" the rule above forbids.

## Fixture capture procedure

Stage 1 exists partly to capture recorded protocol fixtures so Stage 3's fake Claude/Codex
endpoints can be built without a live provider (PLANNING-PROMPT.md §8 Stage 1, §6). Fixtures
are captured **during** the spike, from the real harness — never reconstructed afterward from
memory or from the spec, since a reconstructed fixture silently reintroduces the "probably."

A fixture must contain:

- The pinned version and capture date (matches the gate result's pin and date).
- The literal wire traffic (JSON-RPC frames, MCP notifications/requests, or equivalent) for
  the exchange it documents — not a paraphrase.
- No credentials, tokens, or private filesystem paths (redact before committing), and no
  third-party text a harness read or a tool returned (`references/scripted-runs.md`, #130).
- For Codex: reference the checked-in schema (`codex-rs/app-server-protocol/schema/json`)
  rather than hand-transcribing method shapes.

D6 (Epic D) enumerates exactly which exchanges each provider's fixture set must cover; see
that task for the checklist. Fixtures land wherever Stage 3 expects fakes to load them from
(no `core/`/`adapters/` tree exists yet — do not invent the path; check `docs/planning/`
Stage 3 output or ask if it is not yet decided).

Adding, renaming, or superseding a fixture updates
`docs/planning/gates/fixtures/MANIFEST.json` (D6, issue #39 T1) in the same change — one
entry per file, derived from the file itself, not from a result doc's prose. Run
`node scripts/check-fixture-manifest.mjs` before calling the fixture work done.

## Throwaway rule

Spike code is discarded once its gate result is written. Nothing durable is built on it:

- No spike becomes Stage 3/4 code by default (D7 acceptance: "All spike code is discarded or
  clearly quarantined").
- If spike code is kept for reference, it is clearly quarantined outside the paths a later
  stage will build in, and the gate result says so.
- No substantial core or transport code starts before **both** Stage 0 (evidence/pinning,
  Epic B) and Stage 1 (this stage, Epic D) complete. `docs/planning/STATUS.md` "Blocked"
  states this is currently in force.

## Go/no-go vs. fallback

**Structural finding to hold before reading this table:** neither Claude Code nor Codex
supports attaching an external channel to an arbitrary, already-running session process. Both
support injection only into a session launched OAC-enabled — Claude Code via `--channels` at
session start, Codex via the shared app-server daemon or an OAC-owned app-server with
`codex --remote` (PLANNING-PROMPT.md §4). Do not spike or record a FAIL against
attach-to-arbitrary-session; it is not a supported target for any gate.

| Gate | v0.1 disposition | Fallback |
|---|---|---|
| G1 Claude wake | **go/no-go — no fallback** | none; failure blocks the Claude adapter entirely |
| G2 Codex live inject | **go/no-go if both paths fail** | OAC owns the app-server; user runs `codex --remote ws://…` |
| G3 Zenoh local peer | has fallback | fixed local endpoint, multicast scouting disabled |
| G4 MCP dual-era server | has fallback | two server entry points sharing one core |
| G5 Provenance | no fallback stated | failure invalidates DESIGN acceptance criterion 6 |

## Per-gate reference files

Load only the one reference file for the gate you are running — not the whole set.

| Gate | Reference |
|---|---|
| G1 Claude wake | `references/G1-claude-wake.md` |
| G2 Codex live inject | `references/G2-codex-inject.md` |
| G3 Zenoh local peer | `references/G3-zenoh-peer.md` |
| G4 MCP dual-era server | `references/G4-mcp-dual-era.md` |
| G5 Provenance | `references/G5-provenance.md` |

Each reference carries, for that gate only: full pass criteria, failure criteria, the
fallback, the surfaces and version pins, the specific PLANNING-PROMPT.md §3 facts the spike
closes (including UNVERIFIED items), and the fixtures to capture.

## Scripted runs (herdr)

A gate re-run driven through herdr (Epic K, `tools/herdr/`) also loads
`references/scripted-runs.md`: driver identity, the scripted-run timebox, timeout means
`NOT RUN`, no automatic re-submission, herdr state never scores a criterion, verdict
eligibility (equivalence records), the dialog rule (#196: the driver accepts only Claude
Code's trust, MCP-approval and dev-channels dialogs; #271: G4 alone may answer Codex's MCP
tool approval "1. Allow" for its own tools), and verification (#252): herdr,
harness versions and dialog accepts are verified with citations, not attested; a person
signs only for what the agent cannot do (G1 criterion 5, G11, sign-ins, credentials). A
scripted run is non-verdict-bearing unless that file says it may carry a verdict.

## Exit criteria for a gate work item

- [ ] The gate's own reference file has been loaded and every pass criterion in it evaluated
      individually against the observed spike behaviour.
- [ ] The gate-result template above is filled in completely — no field left as "TBD" —
      and written to `docs/planning/gates/G<n>-result.md` using the extended form in
      `docs/planning/gates/README.md`.
- [ ] Verdict is PASS, PASS (FALLBACK TAKEN), FAIL, or NOT RUN — never a hedge.
- [ ] If a fallback exists and the primary path failed, the fallback was attempted and its
      outcome recorded.
- [ ] `docs/planning/STATUS.md` Gate verdicts table is updated in the same change.
- [ ] `docs/planning/v0.1/02-gating-findings.md` is not hand-edited for this gate; it is
      regenerated from `docs/planning/gates/G<n>-result.md` when Epic A assembles the
      output package.
- [ ] Any fixtures required for that gate (per its reference file / D6) were captured during
      the spike and committed, redacted of secrets.
- [ ] Spike code is discarded or quarantined; nothing from it is left wired into a durable
      module path.
- [ ] Any UNVERIFIED item the gate was meant to close is either closed (with evidence) or
      explicitly still open in the result.

Before sending a gate result for review, check `references/writeup-pitfalls.md` — the
recurring mistakes that cost G2 and G4 three review rounds each.

## Where the content lives

- `docs/planning/PLANNING-PROMPT.md` §4 (gates), §3 (baseline facts), §8 Stage 1 (timebox,
  fixtures), §6 (fake-endpoint design-for-replacement proof).
- `docs/planning/backlog/03-tasks-CD.json` — Epic D tasks D1-D7, the per-gate acceptance
  criteria this skill's references are built from.
- `docs/planning/STATUS.md` — current gate verdicts (today: all `NOT RUN`) and pins.
- `docs/planning/gates/README.md` — the evidence-store naming convention, the extended
  gate-result template, and the pin-move re-run/invalidation policy (harness rows exempt).
- `oac-boundaries`, `oac-evidence` — guardrail content, not restated here.
- `references/writeup-pitfalls.md`, `references/scripted-runs.md` — linked above.
