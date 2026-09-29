# Source index (PLANNING-PROMPT.md Appendix B)

Copy of the first-party source list from `docs/planning/PLANNING-PROMPT.md`
Appendix B. URLs are reproduced exactly as written there, with one mechanical
exception: Appendix B abbreviates repeated URLs within a group using `...`
against the group's first full URL (for example `.../basic/versioning` under
MCP, `.../protocol/extensibility` under ACP) — those ellipses are expanded here
against that first URL so each entry is pasteable on its own; nothing else is
altered, shortened, or guessed at. If a URL here 404s during Stage 0
re-verification, that is a finding: record it, do not silently substitute a
different one.

Not every §3.x "Sources:" URL is indexed in Appendix B (for example
`zenoh.io/docs/manual/configuration/`, `zenoh.io/docs/getting-started/deployment/`,
`agentclientprotocol.com/protocol/`). If you need one of those, cite the §3.x
"Sources:" line directly — do not invent a matching Appendix B entry.

This file is Tier 3: load it only when you need to cite or re-verify a specific
provider fact. For the citation format and the re-verification procedure, see
`.claude/skills/oac-evidence/SKILL.md`.

## Claude Code

- `https://code.claude.com/docs/en/channels.md`
- `https://code.claude.com/docs/en/channels-reference.md`
- `https://code.claude.com/docs/en/mcp.md`
- `https://code.claude.com/docs/en/hooks.md`
- `https://code.claude.com/docs/en/cross-session-messaging.md`

## Codex

- `https://learn.chatgpt.com/docs/app-server`
- `https://learn.chatgpt.com/docs/cli/reference`
- `https://learn.chatgpt.com/docs/hooks`
- `https://learn.chatgpt.com/docs/non-interactive-mode`
- `https://learn.chatgpt.com/docs/config-file/config-advanced`
- `https://github.com/openai/codex` — subpaths: `codex-rs/app-server`,
  `codex-rs/app-server-daemon`, `codex-rs/app-server-protocol/schema`,
  `codex-rs/cli/src/queue_cmd.rs`, `codex-rs/tui/src/lib.rs`;
  `openai/codex` issues: `#21743`, `#16614`, `#33957`, `#45251`;
  `openai/codex` PRs: `#39092`, `#39657`, `#42993`

## MCP

- `https://modelcontextprotocol.io/specification/2026-07-28/changelog`
- `https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning`
- `https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/mrtr`
- `https://modelcontextprotocol.io/extensions/overview`
- `https://modelcontextprotocol.io/seps/2133-extensions`

## Zenoh

- `https://github.com/eclipse-zenoh/zenoh` — releases, `DEFAULT_CONFIG.json5`,
  PR `#2671`, issue `#1432`
- `https://zenoh.io/docs/manual/access-control/`
- `https://zenoh.io/docs/manual/tls/`
- `https://zenoh.io/docs/manual/quic/`
- `https://zenoh.io/docs/manual/user-password/`
- `https://zenoh.io/docs/manual/plugins/`
- `https://github.com/eclipse-zenoh/zenoh-ts`
- `https://github.com/eclipse-zenoh/zenoh-python`

## ACP and Cursor

- `https://agentclientprotocol.com/protocol/overview`
- `https://agentclientprotocol.com/protocol/extensibility`
- `https://github.com/agentclientprotocol/agent-client-protocol`
- `https://github.com/agentclientprotocol/codex-acp`
- `https://cursor.com/docs/cli/acp`

## Provenance of this list

Source: `docs/planning/PLANNING-PROMPT.md` Appendix B.
Baseline retrieval date for the facts these sources back: 2026-09-15
(see `docs/planning/STATUS.md` Pins table — none are confirmed pins yet).
