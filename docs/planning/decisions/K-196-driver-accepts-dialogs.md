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

1. **Dialogs.** In dev/test runs the herdr driver accepts three Claude Code dialogs itself:
   workspace trust, project MCP-server approval, and the
   `--dangerously-load-development-channels` warning.
2. **Default.** `accept=driver` is the default in `g2-codex-inject`, `g4-mcp-dual-era`,
   `g5-provenance` and `l3-beacon`. `accept=human` remains available everywhere.
3. **G1 keeps `accept=human`.** `g1-claude-wake` keeps it as the default, and `accept=driver`
   is allowed there (see §3).
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
  unexpected selection.
- **Keys one at a time.** Each key is a herdr `dialog-accept` command sent straight after a
  read of the pane. Each selection move must be confirmed by a later read that shows the
  expected option selected. Enter is sent only after a read shows the accepting option
  selected. The driver approves one MCP server only, never "all future MCP servers". A move
  that does not land within 10 s ends the run `NOT RUN`, and nothing is re-sent. Code:
  `planDriverAccept` (`lib/g1.mjs`) and `driverAcceptDialog` (`lib/gate-common.mjs`).
- **Codex.** No Codex dialog text is on record, so a Codex dialog is accepted only when its
  accepting option is already preselected. The pre-#196 rule is unchanged for Codex, and for
  Claude Code's tool-permission prompt.
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
  it `not evaluable`. An `accept=driver` G1 run is therefore never an equivalence record,
  which is why `g1-claude-wake` and CI keep `accept=human`.
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
  (`g1-report` … `g5-report`, `l3-report`) and the scenarios. Self-tests:
  `tools/herdr/test/g1-tests.mjs` and `l3-tests.mjs`, with `fake-claude.mjs` option dialogs.
