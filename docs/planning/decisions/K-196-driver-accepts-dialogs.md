# K-196 — The herdr driver accepts harness dialogs in dev/test runs

- **Date:** 2026-09-30
- **Issue:** #196 (the operator decision is recorded there)
- **Amends:** `.claude/skills/oac-gates/references/scripted-runs.md` "Operator-consent
  dialogs" (the K5 rule), and its "Operator attestation" and "Verdict eligibility" sections.
- **Status:** decided by the operator; implemented in `tools/herdr/` (PR for #196). Live
  behavior is UNVERIFIED: the driver has only been tested against the test doubles.
- **Amended:** 2026-09-30 (#199): the driver also accepts Codex's workspace-trust dialog,
  now on record (§6). 2026-10-04 (#271): in the G4 scenario only, the driver answers Codex's
  MCP tool-approval prompt "1. Allow" for G4's own server and tools (§7).

This is the separately recorded operator decision that `scripted-runs.md` "Changing the
rule" requires.

## 1. Decision

Operator, on #196: "Please revise. The point, again, is automation of these processes
during development and test." This follows #187: "The ENTIRE POINT of adding herdr was to
automate implementation and testing tasks."

1. **Dialogs.** In dev/test runs the herdr driver accepts three Claude Code dialogs itself,
   and only these three: workspace trust, project MCP-server approval, and the
   `--dangerously-load-development-channels` warning. Every other dialog is refused (§2).
   Amended by #199 (§6): Codex's workspace-trust dialog is a fourth.
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
| Codex workspace-trust (#199, §6) | "Trust this folder? Codex can read, edit, and run files here, …" | "1. Trust and continue" (preselected, marker `›`), "2. Back to Agent Command Center" | `enter` |

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
  dialog other than the trust dialog on record since #199 (§6). The run ends `NOT RUN` with
  no key sent. There is no opt-in.
  - **Why.** The first draft of this PR kept the pre-#196 "accept when the accepting option
    is preselected" path for these kinds. With `accept=driver` now the default, that path
    would have let the driver approve any tool call mid-run. In G5 that includes one a
    spoofed body induced while Claude runs shell commands.
  - **Future change.** Tool approval is outside #196. Driver-approving it needs its own
    recorded decision and a security note (`oac-security-work`, permission relay).
  - **For runs that may meet such a dialog.** Use `accept=human`. (Superseded for Codex's
    trust dialog by #199, §6: its text is now on record and the driver accepts it.)
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

## 6. Amendment 2026-09-30 (#199): Codex's workspace-trust dialog

**Decision.** Operator, on #199 (2026-09-30): the driver accepts harness dialogs in dev/test
runs. #197 refused every Codex dialog because none was on record, so the 2026-09-30 L3 probe
run ended `NOT RUN` at Codex's trust dialog. That run captured the text, and the driver now
accepts that dialog, under every rule of §2 unchanged.

**Captured live.** 2026-09-30T17:28:35Z, Codex CLI / app-server 0.159.2, Windows, driver
run `probe4` of `l3-beacon`, read seq 48, TUI attached to the app-server daemon; scratch
path redacted to `<SCRATCH>`:

```
  <SCRATCH>\l3-codex-project

  Note: You’re in a subdirectory of a Git project. Trusting will apply to the repository root:
  <SCRATCH>\l3-codex-project

  Trust this folder? Codex can read, edit, and run files here, subject to your permission settings. Folder settings
  can run code automatically, even without a model request. Continue only if you trust these files. Your trust
  …
› 1. Trust and continue
  2. Back to Agent Command Center

  enter continue · esc back
```

**Mechanics.** The entry is `CODEX_DIALOG_KINDS['workspace-trust']` in
`tools/herdr/lib/g2.mjs`: options exactly "Trust and continue", "Back to Agent Command
Center", numbered 1 and 2 in that order; preselection and accepting option both option 1;
selection marker `›` (not Claude Code's `❯`). The driver sends `enter` alone, straight after
the read that planned it. Everything in §2 holds: the pane is read verbatim first, there must
be exactly one selection marker, and unknown, extra or reordered options, a different
preselection, other numbering or another marker are refused (`NOT RUN`, no key). The Enter is
bounded by the post-accept wait and never re-sent.

Tightened on the PR #201 review:

- **Footer.** The footer must be exactly the line "enter continue · esc back". A pane
  without it is refused. So is Codex's other footer, "enter continue and create sandbox ·
  esc back", where Enter also creates the Windows sandbox.
- **Question paragraph.** It is recorded verbatim from `trust_directory.rs` at
  `rust-v0.159.2`: "Trust this folder? … Your trust decision will be saved." The lines
  between it and option 1 must be that paragraph, either whole or as a prefix ending in
  "…" (the capture shows it truncated). Detection is anchored at a line start, so the phrase
  quoted elsewhere does not start a dialog.
- **Note block.** It sits above the question and may be present or absent. When present, it
  must be exactly the recorded text.
- **Below the footer.** A selection-marked line there counts as a second marker.

Codex's other variants of this dialog are not on record and are refused: "Quit" as option 2
(TUI not attached to a daemon), and "Open restricted" or "Open existing task" as option 1.

**Why "a subdirectory of a Git project" whose root is the directory itself.** No scenario
runs `git init`. Every scratch project is a plain directory made by `ctx.dir()` under
`os.tmpdir()` (`tools/herdr/run.mjs`), and `%TEMP%` on the capture machine is not inside a
Git repository. The Note is a Windows path-spelling artefact in Codex, read from source at
tag `rust-v0.159.2` (commit `8b9fa496bbf2c47aebd62e85a080b9a522a455b5`, read 2026-09-30):

- `codex-rs/tui/src/onboarding/trust_directory.rs` shows the Note when
  `self.cwd != self.trust_target`.
- For a TUI attached to the daemon, `read_remote_project_trust` in
  `codex-rs/tui/src/config_update.rs` sets `trust_target` from
  `normalized_project_trust_keys` of the project root. The project root falls back to the
  cwd when no root marker is found. `cwd` keeps its original spelling.
- `normalized_project_trust_keys` in `codex-rs/config/src/loader/mod.rs` lowercases the
  key on Windows (`to_ascii_lowercase`) and puts the canonical spelling first.

So on Windows the trust target is a lowercased or canonical spelling of the cwd itself. It
compares unequal to the cwd, and the Note shows the same directory as the "repository root".
The redaction to `<SCRATCH>` hides the spelling difference. This is a source reading, not
a runtime observation (UNVERIFIED at runtime). Either way the driver treats the Note as body
text.

**Evidence of need.** The 2026-09-30 L3 probe run `probe4` ended `NOT RUN` at this dialog:
"no option text on record".

**Where it lands.** `tools/herdr/lib/g2.mjs` (the entry), `tools/herdr/lib/g1.mjs`
(`numbered`, `marker`, `footer`, `body` and `note` kind fields), `tools/herdr/test/fake-codex.mjs` (the
dialog with and without the Note, and three refused variants), the G2 unit and lifecycle
tests, and one driver-accepted Codex trust dialog in each of the G4, G5 and L3 lifecycles.
Also `scripted-runs.md` (dated amendment), `tools/herdr/README.md` "Dialogs", and the G2
scenario's header comment. Live acceptance is UNVERIFIED until a live run shows it.

## 7. Amendment 2026-10-04 (#271): Codex's MCP tool approval, G4 only

**Decision.** Operator, on #271 (2026-10-04): a narrow exception to #197 ("the driver never
answers tool-permission prompts"). Codex asks before every MCP tool call, which blocks every
unattended G4 run. The driver MAY answer that prompt in the G4 scenario, under the conditions
below only. Everywhere else #197 stands unchanged.

**Captured live.** G4 herdr run 20261004T050646Z, Codex CLI 0.160.0, Windows, Codex pane
capture section seq 58 (the run ended `NOT RUN` there: "unrecognized dialog (unknown)"):

```
  Field 1/1
  Allow the g4http MCP server to run tool "g4_echo"?

  text: hello from codex through herdr

  › 1. Allow                   Run the tool and continue
    2. Allow for this session  Run the tool and remember this choice for this session
    3. Always allow            Run the tool and remember this choice for future tool calls
    4. Cancel                  Cancel this tool call
  enter to submit | esc to cancel
```

**Conditions** (`tools/herdr/lib/g2.mjs` `CODEX_TOOL_APPROVAL`, `planCodexToolApproval`,
`codexToolApprovalCheck`):

1. The prompt matches the recorded text exactly: the "Field 1/1" header, the question
   wording, the footer, the four options with their descriptions, numbered 1-4, and the `›`
   marker. The lines between question and options are `<input>: <value>`, and each input is
   one the tool declares.
2. The server and tool are ones the G4 scenario registered itself. They come from its own
   committed config, never from the pane: `tools/herdr/lib/g4.mjs` `g4CodexToolApproval`
   returns server `g4http` and tools `g4_echo` and `g4_relay_to_claude` (the committed
   `gate-servers/g4-server.mjs` tools, checked by a unit test), and only when the validated
   Codex launch registers `mcp_servers.g4http.url` per invocation.
3. The answer is option 1 "Allow", this call only. It must be the preselection, with exactly
   one marker, and a fresh read must confirm it before Enter. The driver never moves the
   selection on this prompt, so it never answers "Allow for this session" or "Always allow".
4. Anything else stops the run `NOT RUN` with no key sent, as before #271. That covers
   another server or tool, other wording, another selection, and any other scenario: only
   `g4-mcp-dual-era` passes an expectation (`driverMayAcceptCodexExpecting`). G2, G5 and L3
   refuse the exact recorded prompt.
5. Each answer is recorded on the dialog in the run manifest (`toolApproval`: prompt, server,
   tool, arguments, expected, answer; `acceptKeys`; `confirmReadSeq`; `acceptOrigin: driver`)
   and rendered in the Verification section's Dialogs line (`describeDialogs`,
   `tools/herdr/lib/gate-report-common.mjs`).
6. The harness-config hashes must be unchanged afterwards. After any driver Allow, `run.mjs`
   requires them unchanged at teardown (`harnessConfig.mustStayUnchanged`). A change is a
   finding and turns a PASS into FAIL, and the Dialogs line marks the check UNVERIFIED. A
   Codex trust accept in the same run also changes `config.toml`. The hash cannot tell that
   change apart, so it fails the check too (fail-closed).

**Why "Allow" persists nothing (source, not runtime).** Read from openai/codex tag
`rust-v0.160.0` (commit `79b1b666f2e8551f8abbbca34957227f67f3f553`) on 2026-10-04:

- The question is `Allow {actor} to run tool "{tool_name}"?`, with the actor `the {server}
  MCP server` (`codex-rs/core/src/mcp_tool_call.rs`,
  `build_mcp_tool_approval_fallback_message`).
- The TUI labels "Allow", "Allow for this session" and "Always allow" come from
  `codex-rs/tui/src/bottom_pane/mcp_server_elicitation.rs`.
- "Allow" is an elicitation Accept without a persist mode.
  `parse_mcp_tool_approval_elicitation_response` maps it to `ReviewDecision::Approved`.
- `apply_mcp_tool_approval_decision` does nothing for `Approved`. Only
  `ApprovedForSession` (remembered for the session) and `ApprovedMcpPolicyAmendment`
  (`maybe_persist_mcp_tool_approval`) keep anything.

Condition 6 checks this on every run rather than trusting the source reading.

**Security note (`oac-security-work`).** This is a driver-side test exception, not OAC
behavior. It does not enable permission relay or any product authorization path. No OAC
code approves tools.

| Attack | Precondition | Mitigation | Proving test | Residual risk |
|---|---|---|---|---|
| A tool call other than G4's own is approved unattended (prompt injection steers Codex to another server or tool) | A G4 run under `accept=driver`, and Codex prompts for a tool the scenario did not register | Server and tool must equal `g4CodexToolApproval`'s committed values; any other is refused, `NOT RUN` | `g4-tests.mjs` units "another server", "another tool"; lifecycle "g4 #271 tool approval: another server/tool refused" | The allowed tools themselves run with model-chosen arguments; they are the scenario's own test tools (echo, relay into the scenario's Claude session) |
| A persistent approval is written to the operator's Codex config | The driver selects "Allow for this session" or "Always allow", or Codex persists a plain Allow | Only a preselected "1. Allow", confirmed by a fresh read, is answered; harness-config hashes must be unchanged afterwards | Units "Always allow"/"Allow for this session" highlighted, confirm-read stop; lifecycle "an Allow that changes the harness config is not a PASS" | Config outside the hashed files (`config.toml`, `hooks.json`, Claude `settings.json`) is not checked |
| The exception spreads to other scenarios | Another scenario meets the prompt under `accept=driver` | Only `g4-mcp-dual-era` passes an expectation; without one the planner refuses | Unit "a non-G4 scenario"; lifecycle "g5 Codex MCP tool-approval prompt: refused outside G4 (#271)" | A future scenario could opt in only through a code change and a new recorded decision |
| A reworded or restructured prompt is answered | Codex changes the form | Every line is checked against the recorded text; off-record text is refused | Units "altered question wording", option/footer/header/marker variants; lifecycle "altered wording refused" | None beyond a Codex change that keeps the recorded text but changes its meaning |

**Verdict eligibility.** No G4 criterion names this prompt, so a driver Allow costs G4
nothing, provided the dialog matched the record, the answer is recorded as the driver's, and
the harness config is unchanged (`scripted-runs.md` "Verdict eligibility").

**Where it lands.** `tools/herdr/lib/g2.mjs` (the kind, parser, planner and confirm check),
`tools/herdr/lib/g4.mjs` (`G4_CODEX_SERVER`, `G4_CODEX_TOOLS`, `g4CodexToolApproval`), the G4
scenario, `tools/herdr/lib/gate-common.mjs` (the confirming read before Enter, and the
config requirement), `tools/herdr/run.mjs` (`requireHarnessConfigUnchanged`),
`tools/herdr/lib/gate-report-common.mjs` (the Dialogs line), `tools/herdr/test/fake-codex.mjs`
(`FAKE_CODEX_TOOL_APPROVAL` and its shape variables), the G4 unit and lifecycle tests and one G5
lifecycle test. Also `scripted-runs.md` (dated amendment) and `tools/herdr/README.md`
"Dialogs". Live acceptance is UNVERIFIED until a live G4 run shows it.
