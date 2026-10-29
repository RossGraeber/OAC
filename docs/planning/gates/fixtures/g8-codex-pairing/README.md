# G8 S6 live leg: Codex pairing and hand-offs (Codex 0.165.0, Claude Code 2.1.312)

> **Not a gate run and not verdict-bearing.** Issues #69 and #433 (G8 slice S6), with #68. Record:
> `docs/planning/gates/herdr-runs/G8-S6-2026-10-29.md`; run manifest
> `docs/planning/gates/herdr-runs/G8-S6-2026-10-29.run-manifest.json` (run
> `20261029T123556Z-9173d9`, outcome PASS, driver commit `c45796a`, herdr `0.9.1`). The six
> files are exactly what the driver wrote: their sha256 values are the run manifest's
> `captures[].sha256`.

A real Codex TUI (plain `codex`, attached to the shared daemon) and a real Claude Code, both run by the
driver, with the OAC example live daemon between them (`cli/examples/live_daemon.rs`). The driver
registered the OAC MCP server with Codex as `oac-g8live` and removed it afterwards. A passive capture
client subscribed to the Codex thread (`watch`), and a second one made a `thread/resume` while the first
tool call was in flight (`midwatch`).

| File | What it holds |
|---|---|
| `transcript-g8live-2026-10-29-0.165.0-herdr.jsonl` | The capture clients' wire transcript (`list`, `watch`, `midwatch` connections). Pairing L76-L120; the mid-turn `thread/resume` and its `item/completed` L85-L102; the Codex `send` L224-L235; the three hand-off `userMessage` frames L270, L340, L408; the driver-typed reply prompts and the three `reply` calls L472-L653. |
| `pane-codex-g8live-2026-10-29-0.165.0-herdr.txt` | Verbatim reads of the Codex TUI before each driver key: six MCP tool-approval prompts (`1. Allow` answered), including the one-line array argument `content: [{"type": "text", "text": "..."}]`. |
| `pane-claude-g8live-2026-10-29-0.165.0-herdr.txt` | Verbatim reads of the Claude Code TUI: folder trust, the project MCP server approval, the development-channels warning, a Settings Warning about the operator's own settings file. |
| `events-g8live-2026-10-29-0.165.0.jsonl` | The example daemon's events, sanitized (pid and image only): `local_grant`, `carrier_server`, `accepted` per connection with the nearest harness ancestor, and its log lines. |
| `claude-g8live-2026-10-29-0.165.0.txt` | The report lines the Claude session printed: its id (redacted), the ids of its three sends and the four messages it received. |
| `transcript-g8live-proc-2026-10-29-0.165.0.jsonl` | One record: pid, parent pid and image name of the Codex shim's ancestors and of every `codex` process. |

## Redaction

All by the driver (`tools/herdr/lib/redact.mjs`, the shared `sanitizeTranscript` of `tools/herdr/lib/g2.mjs`,
and the scenario's run literals); no file was edited afterwards. Session ids, the device fingerprint, the
pairing value, the TUI thread's id and the project directory are replaced (`<SESSION_ID>`,
`<DEVICE_FINGERPRINT>`, `<PAIRING_VALUE>`, `<THREAD_ID>`, `<PROJECT_DIR>`); user name, home path and host
are replaced; no residual leak (`captures[].redaction` in the run manifest: `residualLeaks` and
`residualGenericHits` empty, `droppedHazardLines` 0). Tool-result bodies, approval request bodies and harness
instruction text are elided by #130 (`<ELIDED ...>`); the hand-off frames and the driver's own prompts are OAC's
and the driver's text and are kept. Message ids (`msg-...`) are not secrets and are kept.

## What it shows

See the claims table in the record. In short, at Codex `0.165.0` and Claude Code `2.1.312`, once each: the
refusal, the pairing and a session id (claim 1); a Codex `send` received by Claude (2); three C6 frames in the
Codex thread (3); a correlated and an uncorrelated reply (4); a `thread/resume` made mid-turn still received the
in-flight call's `item/completed` (5); the shim's nearest `codex` ancestor is the control-socket server (6);
Codex set `in_reply_to` to the last hand-off's `oac_message_id` when asked to answer it without the id (7: **confounded, not evidence**; the body of that hand-off named the field and an earlier prompt had shown it, see the record's claim 7);
Codex made no call after a hand-off whose body asked for a reply (8).

**Third-party text.** The only thread items are `userMessage`, `agentMessage` and `mcpToolCall`; no file,
command or other tool item. Codex's `agentMessage` texts in the transcript are model output on OAC test
prompts.
