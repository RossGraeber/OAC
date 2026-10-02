# Claude Channels: distribution, allowlist, and security detail

Tier 3 reference for `.claude/skills/oac-claude-channels/SKILL.md`. Load only
when the work item touches loading/distribution or the security model of the
Claude adapter. All facts: PLANNING-PROMPT.md §3.1 and §7, retrieved
2026-09-15.

These facts, including the version-pinned `v2.1.234+` below, are the
pre-verified baseline retrieved 2026-09-15. Stage 0 (Epic B) has not run —
see the parent SKILL.md's `## Pin` section before treating any of this as a
confirmed pin.

## Distribution and allowlist

- Only plugins on Anthropic's allowlist (`claude-plugins-official`, or an
  org's `allowedChannelPlugins`) load without a flag.
- Anything else requires `--dangerously-load-development-channels` plus an
  interactive confirmation. The confirmation is the only user consent step on
  the Claude side until OAC is on an allowlist — the plan must not weaken or
  automate past it (PLANNING-PROMPT.md §7).
- Team/Enterprise orgs must set `channelsEnabled`.
- Not available on Amazon Bedrock, Google Cloud's Agent Platform (§3.1 said
  "Vertex"; drift D3), or Microsoft Foundry.

Until OAC is allowlisted, the launch command form (quoted verbatim from
`channels-reference.md`'s `server:webhook` example and `cli-reference.md`,
retrieved 2026-10-02 at Claude Code `2.1.285`; task G11 requires it verbatim)
is:

```
claude --dangerously-load-development-channels server:<name>
```

(`--channels plugin:<name>@<marketplace>` is for an allowlisted plugin-packaged
channel. `--channels` does not take `server:` entries: drift D4,
`docs/planning/REVERIFICATION-B2.md`. The form previously given here,
`claude --channels server:<name> --dangerously-load-development-channels`, came
from §3.1 and was corrected 2026-10-02, #122.) Quote this verbatim in any launch
doc (task G11) — do not paraphrase the flag names.

## Security

- Every approved channel plugin keeps a **per-channel sender allowlist**
  bootstrapped by pairing code.
- Optional **permission relay** (`claude/channel/permission`, v2.1.234+) lets
  **any allowlisted sender** approve or deny tool use in the session. This is
  the C10 conflict (PLANNING-PROMPT.md Appendix A): anyone able to reply on a
  channel can approve tool use if permission relay is enabled.
  - PLANNING-PROMPT.md §5 decision 8 proposes permission relay **off by
    default in v0.1**. §5 decision 8 requires this to be justified or
    reversed; Decision 8 is open (`docs/planning/STATUS.md`, Epic C). Do not
    enable it without an explicit, separately justified decision — enabling
    it changes the security posture with no additional gate to catch a
    mistake.
- Channel content is **untrusted**. Model-generated text never establishes
  identity; provenance must be machine-set metadata kept separate from
  content (PLANNING-PROMPT.md §7). On Claude this means: sender identity
  travels in identifier-safe `meta` attributes set by the adapter, never
  parsed out of `content`.

## Where this feeds

- Gate G1 pass criteria include the legacy MCP negotiation requirement and
  the interactive confirmation dialog (PLANNING-PROMPT.md §4).
- Task G11 (`docs/planning/backlog/05-tasks-GHIJ.json`) requires the launch
  command quoted verbatim, the confirmation presented as a feature, and the
  distribution constraints stated.
- Task G4/G5 (same file) require permission relay off by default and
  provenance as identifier-safe `meta` attributes.
