# K-196 — The herdr driver accepts harness dialogs in dev/test runs

- **Date:** 2026-09-30
- **Issue:** #196 (the operator decision is recorded there)
- **Amends:** `.claude/skills/oac-gates/references/scripted-runs.md` "Operator-consent
  dialogs" (the K5 rule), and its "Operator attestation" and "Verdict eligibility" sections.
- **Status:** decided by the operator; implemented in `tools/herdr/` (PR for #196). Live
  behavior is UNVERIFIED: the driver has only been tested against the test doubles.

This is the separately recorded operator decision that `scripted-runs.md` "Changing the
rule" requires.

## 1. Decision

Operator, on #196: "Please revise. The point, again, is automation of these processes
during development and test." This follows #187: "The ENTIRE POINT of adding herdr was to
automate implementation and testing tasks."

1. **Dialogs.** In dev/test runs the herdr driver accepts three Claude Code dialogs itself,
   and only these three: workspace trust, project MCP-server approval, and the
   `--dangerously-load-development-channels` warning. Every other dialog is refused (§2).
2. **Default.** `accept=driver` is the default in every scenario: `g1-claude-wake`,
   `g2-codex-inject`, `g4-mcp-dual-era`, `g5-provenance` and `l3-beacon`. `accept=human`
   remains available everywhere.
3. **G1 is fully driver-accepted.** Second operator decision on #196, 2026-09-30, recorded
   for the #197 fix round: "G1 is fully driver-accepted — `g1-claude-wake` defaults to
   `accept=driver` for all three dialogs". The operator accepted the consequence: G1
   criterion 5 is `not evaluable` on driver-accepted runs, so they are not G1 equivalence
   records (§3). This supersedes the PR's first draft, which kept G1 on `accept=human`. CI
   runs G1 on its defaults, so a CI G1 run is now unattended and never a G1 equivalence
   record.
4. **Pane read first.** The pane text of every dialog is read verbatim, and kept, before any
   keystroke.
5. **Truthful record.** The accept origin is recorded as `driver`. It is never inferred or
   rendered as `human`.

## 2. Mechanics (what "accept" may mean)

| Dialog | Recognized by | Options on record, in order (preselected) | Driver keys |
|---|---|---|---|
| workspace-trust | "Is this a project you created or one you trust?" | "No, exit" (preselected), "Yes, I trust this folder" | `down`, `enter` |
| mcp-server-approval | "New MCP server found in this project: …" | "Use this MCP server", "Use this and all future MCP servers in this project", "Continue without using this MCP server" (preselected) | `up`, `up`, `enter` |
| dev-channels | "WARNING: Loading development channels" | "1. I am using this for local development" (preselected), "2. Exit" | `enter` |

- **Source of the option texts.** Observed live: G1 Box C (dev-channels), herdr runs of
  2026-09-29 (#156 trust, #161 MCP), and the 2026-09-30 L3 probe runs (all three) on Claude
  Code v2.1.283, Windows. The table lives in `tools/herdr/lib/g1.mjs` `DIALOG_KINDS`.
- **Conservative match.** The driver acts only if the options on screen are exactly the
  recorded ones, in order, with exactly one selected, and the selection is on the recorded
  preselection or already on the accepting option. Anything else is refused and the run ends
  `NOT RUN` with no key sent: an unrecognized dialog, an extra or changed option, or an
  unexpected selection. The option list starts at the first recorded option or selection
  marker. From there to the footer, every non-empty line must be a recorded option, at any
  indentation. An unmarked extra line indented differently is refused too (#197 review).
- **Keys one at a time.** Each key is a herdr `dialog-accept` command sent straight after a
  read of the pane. Each selection move must be confirmed by a later read that shows the
  recorded options with exactly one selection marker, on the expected option
  (`selectionCheck`). A read with two markers never verifies a move (#197 review). Enter is
  sent only after such a read shows the accepting option selected. The driver approves one MCP server only, never "all future MCP servers". A move
  that does not land within 10 s ends the run `NOT RUN`, and nothing is re-sent. Code:
  `planDriverAccept` (`lib/g1.mjs`) and `driverAcceptDialog` (`lib/gate-common.mjs`).
- **Everything else is refused (#197 review).** The driver never accepts a dialog kind
  with no option text on record, whatever is preselected. That covers Claude Code's
  tool-permission prompt ("Do you want to proceed?" before a tool call) and every Codex
  dialog: no Codex dialog text is on record. The run ends `NOT RUN` with no key sent. There
  is no opt-in.
  - **Why.** The first draft of this PR kept the pre-#196 "accept when the accepting option
    is preselected" path for these kinds. With `accept=driver` now the default, that path
    would have let the driver approve any tool call mid-run. In G5 that includes one a
    spoofed body induced while Claude runs shell commands.
  - **Future change.** Tool approval is outside #196. Driver-approving it needs its own
    recorded decision and a security note (`oac-security-work`, permission relay).
  - **For runs that may meet such a dialog.** Use `accept=human`. That includes a real Codex
    trust dialog in a fresh scratch project, which is UNVERIFIED: no live Codex dialog text
    is on record.
- **No harness config.** The driver writes no harness config: no `~/.claude.json` trust
  entry and no settings edit (`oac-boundaries` check 10). It never runs herdr's hook-writing
  command.

**Optional friction reducers.** These are documented, not required, and never done by the
driver:

- a reusable project directory (`g1-claude-wake --param projectDir`), so a trust once
  recorded by Claude Code persists;
- an operator's own `enabledMcpjsonServers` setting.

## 3. Verdict eligibility

The issue's default proposal, adopted.

A run whose dialogs the driver accepted is verdict-eligible, under the rest of
`scripted-runs.md` "Verdict eligibility", when both of these hold:

1. the dialog text matched the recorded expected text: the kind was recognized and its
   options were exactly the ones on record, otherwise the run is `NOT RUN`;
2. the run manifest records the driver accept: `acceptOrigin: driver`, `acceptKeys`, and
   the `dialog-accept` commands.

A gate criterion that is *about* a human consent step keeps its human accept and its human
attestation:

- **G1 criterion 5.** "actually exercised during the spike — not bypassed, scripted around,
  or skipped". A driver-sent accept is never scored as meeting it, and `g1-report.mjs` keeps
  it `not evaluable`. An `accept=driver` G1 run is therefore never an equivalence record.
  Per the operator's second decision (§1 item 3) that is G1's default, CI included. A G1 run
  meant to be an equivalence record uses `--param accept=human`.
- **G11 confirmation.** "not bypassed or automated away". It is not driver-accepted in any
  test until K8 and a further recorded operator decision.

G2, G4 and G5 name no consent step. For them a driver accept costs nothing.

## 4. Evidence of need

The 2026-09-30 L3 probe run 3 ended `NOT RUN`. The workspace-trust dialog, with
"No, exit" preselected, was not accepted within 300000 ms.

## 5. Where it lands

- Rule: `oac-gates` `references/scripted-runs.md`, dated amendment, with the superseded K5
  text kept; `oac-gates` SKILL.md "Scripted runs".
- Operator docs and backlog: `tools/herdr/README.md` "Dialogs"; the dated notes in
  `docs/planning/gates/README.md`, `herdr-runner.md`, L1 §12, `K1-herdr-evaluation.md`, the
  L3 backlog entry and `09-test-strategy.md`; `oac-testing`.
- Code: `tools/herdr/lib/g1.mjs`, `g2.mjs`, `gate-common.mjs`, every report lib
  (`g1-report` … `g5-report`, `l3-report`), the scenarios, `ci.mjs` and the opt-in workflow's
  comments. Self-tests: `tools/herdr/test/g1-tests.mjs`, `g2-tests.mjs` and `l3-tests.mjs`,
  with `fake-claude.mjs` option and tool-permission dialogs.
