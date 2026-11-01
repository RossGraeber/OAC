# Codex app-server — events, schema artifacts, auth detail

Reference detail for `.claude/skills/oac-codex-appserver/SKILL.md`. Source for
everything below: `docs/planning/PLANNING-PROMPT.md` §3.2, retrieved 2026-09-15,
version context `@openai/codex` 0.154.0 (2026-09-09). Same pin caveat as the
parent skill: Stage 0 has not run; this is a pre-verified baseline, not a
confirmed pin.

## Event list (verbatim method names)

- `thread/started`
- `thread/status/changed`
- `turn/started`
- `turn/completed` — status `completed|interrupted|failed`.
- `item/started`
- `item/completed` — **authoritative**; deltas are not.
- `item/agentMessage/delta`
- Approval requests — arrive as **server-to-client JSON-RPC requests** (not
  notifications), meaning the client must reply with a JSON-RPC response, not
  just observe. Do not implement approval handling as a fire-and-forget listener.

## Thread/turn method list (verbatim)

- `thread/start`
- `thread/resume`
- `thread/list`
- `thread/loaded/list`
- `turn/start` — input is an array of `{type:"text",text}` items.
- `turn/steer` — appends to the in-flight turn. Security-sensitive; see
  `oac-security-work` and the parent skill's decision-rule table.
- `turn/interrupt`
- `thread/queue/add` — experimental; queued until the thread is idle.

Thread ids are UUIDv7 strings and survive restarts. Rollouts persist under
`CODEX_HOME/sessions/`; the file format there is explicitly not a supported
surface (do not read or write it).

## Queue management methods (experimental; observed on 0.166.0)

Pin: `@openai/codex` 0.166.0 (user agent `codex-tui/0.166.0`, Windows 10, x86_64), live G7
recheck 2026-10-31. Sources: `docs/planning/gates/herdr-runs/G7-recheck-2026-10-31.md`
(Judgments row 66(b)); transcript `docs/planning/gates/fixtures/g7-codex-recheck/transcript-g7recheck-2026-10-31-0.166.0-herdr.jsonl`,
lines 376-399 (requests and responses) and 474 (dispatch); risk row 66(b) in
`docs/planning/v0.1/11-risks.md`. One run, one daemon, one machine.

- `thread/queue/list` `{threadId}` -> `{data: [{id, input, clientUserMessageId}]}`.
- `thread/queue/update` `{threadId, queuedSubmissionId, input}` -> `{queuedSubmission}`; the id is kept.
- `thread/queue/reorder` `{threadId, queuedSubmissionIds}` -> `{}`.
- `thread/queue/delete` `{threadId, queuedSubmissionId}` -> `{deleted: true}`.
- The daemon sent `thread/queue/changed` `{threadId}` to the main client after each write.
- The observing client declared `capabilities.experimentalApi: true` at `initialize`. The
  record reads the methods as experimental in the schema.

Shown: a second client that never started or resumed the thread listed, updated, reordered
and deleted three items queued before `turn/interrupt`. The listing showed each change, and
the next uninterrupted turn dispatched ITEM 3, then ITEM 2 UPDATED; the deleted item never ran.

Not shown: other versions or platforms, the stability of these methods, error responses, and
any item not held after an interrupted turn.

## Daemon and live-inject detail

- Start: `codex app-server daemon start`.
- Control socket: `CODEX_HOME/app-server-control/app-server-control.sock`.
- The TUI, launched **without config overrides**, attaches to a running daemon
  as a client; otherwise it embeds its own server.
- CLI wrapper for queueing input into a live thread: `codex queue --thread <id>
  --message <text>` — merged 2026-08-17, not yet reflected in the CLI reference
  doc (`https://learn.chatgpt.com/docs/cli/reference`).
- Whether implicit daemon attach is enabled in released 0.154.0 vs. only on
  `main` is **UNVERIFIED** (G2 go/no-go item).
- Codex Desktop uses a private stdio app-server and does not expose the control
  socket (**UNVERIFIED for current builds**).
- Cross-process resume does not attach: a second process resuming a thread
  another process holds loads history from disk and appends silently; the live
  process is not notified. Open issue: `openai/codex` #21743.

## Auth boundary detail

- The app-server uses the saved CLI login: `CODEX_HOME/auth.json` or the OS
  keyring.
- An OAC adapter never holds OpenAI credentials (ADR-001 boundary, restated in
  `oac-boundaries`).
- WebSocket listeners (`--listen ws://127.0.0.1:PORT`) support capability-token
  or signed-bearer auth.
- Unix sockets validate peers.

## Schema artifacts and crates

- JSON schema: `codex-rs/app-server-protocol/schema/json` (in
  `github.com/openai/codex`).
- TypeScript schema: `codex-rs/app-server-protocol/schema/typescript`.
- Regeneration command: `codex app-server generate-json-schema`.
- Treat these checked-in files as the source of truth for message shapes, not
  any skill's prose — schemas can be regenerated and drift from a
  hand-maintained description.
- Repo license: Apache-2.0.
- Rust crates, all Apache-2.0 via the repo license:
  - `app-server-client`
  - `app-server-protocol`
  - `app-server-transport`
- **OAC uses none of them.** At `rust-v0.161.0` each reaches a model API client
  (`codex-api`), a keyring store (`codex-keyring-store`) or the rollouts
  (`codex-rollout`), so ADR-001 refuses them; `scripts/check-crate-deps.mjs`
  rule 6 and the adapter scan refuse the whole `codex-` family
  (`docs/planning/decisions/G-7-stage4-dependencies.md` §2). The JSON schema is
  vendored at `docs/planning/vendor/codex-app-server-protocol/rust-v0.161.0/` instead.

## Removed and non-originating surfaces (for completeness)

- `codex mcp-server` (Codex as an MCP server): deprecated 2026-08-20, deleted
  2026-09-05. Not available at the pinned version or later.
- `codex mcp add`: registers **external** MCP servers Codex calls as tools —
  the supported outbound tool surface, unrelated to the deleted inbound one.
- Hooks (`SessionStart`, `UserPromptSubmit`, `Stop`, …): react to events only.
- `notify` (`agent-turn-complete`): carries `thread-id`, `turn-id`,
  `last-assistant-message`; reacts only, cannot originate a turn.
- `codex exec` and the TypeScript SDK: each spawns its own process per run;
  cannot inject into another process's session.

## Sources (Appendix B, Codex row)

`https://learn.chatgpt.com/docs/app-server`,
`https://learn.chatgpt.com/docs/cli/reference`,
`https://learn.chatgpt.com/docs/hooks`,
`https://learn.chatgpt.com/docs/non-interactive-mode`,
`https://learn.chatgpt.com/docs/config-file/config-advanced`,
`https://github.com/openai/codex` (`codex-rs/app-server`,
`codex-rs/app-server-daemon`, `codex-rs/app-server-protocol/schema`,
`codex-rs/cli/src/queue_cmd.rs`, `codex-rs/tui/src/lib.rs`), issues #21743,
#16614, #33957, #45251, PRs #39092, #39657, #42993. Retrieved 2026-09-15.
