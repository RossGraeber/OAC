# G8 S1 capture: Codex `mcpToolCall` items (Codex 0.163.0)

> **Not a gate run and not verdict-bearing.** Issues #69 and #403, G8 slice S1. Record:
> `docs/planning/gates/herdr-runs/G8-S1-2026-10-22.md`; run manifest
> `docs/planning/gates/herdr-runs/G8-S1-2026-10-22.run-manifest.json` (run
> `20261022T035218Z-760755`, outcome PASS, driver commit `060d0fd`, herdr `0.9.1`).

The operator's own Codex TUI (plain `codex`, attached to the shared daemon) had a stub MCP
server, `oac-g8` (`tools/herdr/gate-servers/g8-server.mjs`), registered. The model was asked
to call its one tool, `g8_pair`, three times: first with no arguments (refused, with a
pairing value), then with that value, then with `{}`. A passive capture client
(`tools/herdr/gate-servers/g8-capture.mjs`) subscribed to the TUI's thread through
`codex app-server proxy` with `thread/resume` carrying only `threadId` and `excludeTurns`.

| File | What it holds |
|---|---|
| `transcript-g8-2026-10-22-0.163.0-herdr.jsonl` | The capture client's wire transcript: ten `list` connections (L1-L100) and one `watch` connection (L101 onward). The three calls are L120-L125 (refusal), L128-L134 (the value), L138-L143 (`{}`). Lines are `{t, mode, direction, payload}`. |
| `transcript-g8-server-2026-10-22-0.163.0.jsonl` | The stub server's own log, re-shaped as a transcript (`mode: "server"`; directions `codex->server`, `server->codex`, `event`). Its `t` stamps and the wire's are from one host clock. |
| `transcript-g8-proc-2026-10-22-0.163.0.jsonl` | One record: pid, parent pid and image name only, of the stub's ancestors and of the codex processes, taken when the stub's `tree` event appeared. |

## Redaction

By the driver (`tools/herdr/lib/redact.mjs`): the TUI thread's id `<THREAD_ID>`, the pairing
value `<PAIRING_VALUE>`, the session id (equal to the thread id), user name, home path,
project directory `<PROJECT_DIR>`, host, installation id, plan fields; no residual leak
(`captures[].redaction` in the run manifest). By the recording agent, after the driver wrote
it: Codex 0.163.0 answers `thread/list` with a `readStates` map keyed by the ids of the
operator's other saved threads (20 of them); they are replaced by
`<OTHER_THREAD_ID_1>` to `<OTHER_THREAD_ID_20>` (`otherThreadIds`, `redactOtherThreadIds` in
`tools/herdr/scenarios/g8-codex-tools.mjs`, now run by the scenario itself). The committed
run manifest records the driver's sha256 of the capture (`driverWrittenSha256`) and the
committed file's (`sha256`). Turn ids, item ids, trace ids and `revision` values in the file are
not thread ids and are kept.

**Third-party text.** Tool-result bodies and the approval request's `message`,
`requestedSchema` and `_meta` are elided by the driver's #130 rule: `<ELIDED tool-output
bytes=N sha256=H>`. The stub's results are OAC's own text, so the stub's transcript keeps
them, and the hash on the wire equals the sha256 of that text (with the pairing value as
`<PAIRING_VALUE>`): 144 bytes `dba4a10d...` (refusal), 34 bytes `e8a55bbc...` (paired), 44
bytes `65e6505b...` (served). So `result.content` on the wire is the server's `content`,
byte for byte (L125, L134, L143; the stub's own text is in its transcript, `call-out` events).

## What it shows (facts C1 to C10 of `spec/bindings/mcp.md` §4.5.1, read at `rust-v0.161.0`)

| Fact | At 0.163.0 |
|---|---|
| C1 `_meta` of `tools/call` | `callId`, `x-codex-turn-metadata`, `threadId`, `sessionId`, `windowId` as read, **plus `itemId` (a `ctc_...` id, not the item's id), `traceparent` and `progressToken`** (server transcript, every `tools/call`). `tools/list` carried `traceparent` and `progressToken`. `callId` equals the item's `id` (`exec-<uuid>`). Not used by OAC. |
| C2 per-thread runtime | UNVERIFIED still: one thread, one server process (pid in the stub's `start` event) for all three calls. Seen once, for one server: it started when the TUI session opened, 3 min 50 s before the marker prompt (stub `start` 03:49:01.753Z; the marker turn's UUIDv7 time 03:52:52.470Z), and `initialize` and `tools/list` arrived then, before any prompt. |
| C4 item shape | Members on the wire: `appContext`, `arguments`, `durationMs`, `error`, `id`, `mcpAppUi`, `pluginId`, `readOnlyHint`, `result`, `server`, `status`, `tool`, `type`; `item/started` params `item`, `startedAtMs`, `threadId`, `turnId`; `item/completed` `completedAtMs` instead. The `mcpToolCall` member of `ThreadItem`, `McpToolCallStatus`, `McpToolCallResult` and `McpToolCallError` in `v2/ItemStartedNotification.json` and `v2/ItemCompletedNotification.json` at tag `rust-v0.163.0` (commit `83086188ae0614bd882818ff1aa23f3194962012`, fetched from `openai/codex` on 2026-10-22 through the GitHub contents API) equal those of the vendored `rust-v0.161.0` (compared as parsed JSON). A local `generate-json-schema --experimental` at 0.163.0 gave the same. |
| C5 order | Held for all three calls. `item/started` came before the stub saw the call by 4148 ms, 979 ms and 890 ms; `item/completed` came 18, 26 and 27 ms after the stub answered. Between, for each call: `thread/status/changed` `waitingOnApproval`, an `mcpServer/elicitation/request` (server-to-client, mode `form`), `serverRequest/resolved`, `thread/status/changed` active (L121-L124, L129-L133, L139-L142; the elicitation requests are L122, L130, L140). A refused call: `status` `failed`, `result` `{content, structuredContent: null, _meta: null}`, `error` `null`, **no `isError` member** (L125). Whether `item/completed` precedes the model seeing the result is not observable from the wire (UNVERIFIED). |
| C6 subscription | Held: the carrier's `thread/resume` (`{threadId, excludeTurns: true}`, L106) subscribed it, and all six `mcpToolCall` frames carry the TUI thread's `threadId`. |
| C9 no arguments | **Not exercised.** C9 is conditional: a call with no `arguments` member is reported `null`. That case was not produced. The model was asked to call "with no arguments at all"; the call reached the stub as `arguments: {}` and Codex reported `arguments: {}` (L120, L125), the same as for the call it was asked to make with `{}` (L138). Whether the model's first call carried an `arguments` member, and what Codex reports for a call that has none, are UNVERIFIED. |
| Reveal | The completed refusal item's `result.content` equals the refusal's `content` (hash above); the run's analysis recorded `completedCarriesIssuedValue: true` on the raw bytes. |
| Process tree | The stub (pid 41628, `node.exe`) is a child of a `codex.exe` whose argv carries `app-server` (pid 30180). The TUI's own process (`codex.exe` without `app-server`) is not an ancestor. Which process serves the control socket is not shown (UNVERIFIED). |

## Not recorded, or not what the operator's script expected

- **The approval.** The operator reports that no MCP approval dialog appeared and that nothing
  was answered by hand; the capture client answered nothing (it ignores server requests; that
  is read from its code and unit-tested, not recorded). Yet the wire shows the
  approval step above, resolved 4.1 s, 1.0 s and 0.9 s after `item/started`. Who answered it,
  and why the operator saw no dialog, is UNVERIFIED: the stub declares no tool annotations
  (`readOnlyHint` is `null` in every item), and the operator's Codex configuration, which
  was not read, may set an approval policy.
- **"Hook failed".** The operator reports that the TUI printed `Hook failed - hook exited with
  code 1` on the marker prompt. That it is a hook in the operator's own Codex configuration
  and nothing of OAC's is inferred (OAC registers no hook here; the configuration was not
  read).
- The operator also sent one harmless follow-up ("Please do the remaining calls..."); the
  model answered that all three were done and made no further call.
- **Who receives the approval request.** The `mcpServer/elicitation/request` frames (L122, L130, L140) reached the capture client's connection, which had subscribed with `thread/resume` only. So the request goes to every subscriber of the thread, not only to the TUI. An OAC adapter that subscribes to a thread this way must never answer an `mcpServer/elicitation/request` (or any server-to-client request): answering one decides a tool call's approval for the person using the TUI (`oac-security-work`, permission relay). Risks row 88.
