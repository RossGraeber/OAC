# G8 S1 capture: Codex `mcpToolCall` items (Codex 0.163.0)

> **Not a gate run and not verdict-bearing.** Issues #69 and #403, G8 slice S1. Record:
> `docs/planning/gates/herdr-runs/G8-S1-2026-10-23.md`; run manifest
> `docs/planning/gates/herdr-runs/G8-S1-2026-10-23.run-manifest.json` (run
> `20261023T043548Z-b28855`, outcome PASS, driver commit `fa69963`, herdr `0.9.1`). The three
> transcripts are exactly what the driver wrote: their sha256 values are the run manifest's.

The operator's own Codex TUI (plain `codex`, attached to the shared daemon) had a stub MCP
server, `oac-g8` (`tools/herdr/gate-servers/g8-server.mjs`), registered. The model was asked
to call its one tool, `g8_pair`, three times: first with no arguments (refused, with a
pairing value), then with that value, then with `{}`. A passive capture client
(`tools/herdr/gate-servers/g8-capture.mjs`) subscribed to the TUI's thread through
`codex app-server proxy` with `thread/resume` carrying only `threadId` and `excludeTurns`.

| File | What it holds |
|---|---|
| `transcript-g8-2026-10-23-0.163.0-herdr.jsonl` | The capture client's wire transcript: ten `list` connections (L1-L100) and one `watch` connection (L101 onward). The three calls are L120-L125 (refusal), L128-L134 (the value), L137-L143 (`{}`). Lines are `{t, mode, direction, payload}`. |
| `transcript-g8-server-2026-10-23-0.163.0.jsonl` | The stub server's own log, re-shaped as a transcript (`mode: "server"`; directions `codex->server`, `server->codex`, `event`). Its `t` stamps and the wire's are from one host clock. |
| `transcript-g8-proc-2026-10-23-0.163.0.jsonl` | One record: pid, parent pid and image name only, of the stub's ancestors and of the codex processes, taken when the stub's `tree` event appeared. |

## Redaction

All by the driver (`tools/herdr/lib/redact.mjs` and the shared `sanitizeTranscript` of
`tools/herdr/lib/g2.mjs`); no file was edited afterwards. The TUI thread's id is
`<THREAD_ID>` (the session id equals it), one other loaded thread's id `<OTHER_THREAD_ID_1>`,
the pairing value `<PAIRING_VALUE>`, the project directory `<PROJECT_DIR>`; user name, home
path, host, installation id and plan fields are replaced; no residual leak
(`captures[].redaction` in the run manifest). Codex 0.163.0 answers `thread/list` with a
`readStates` map keyed by the ids of the operator's other saved threads; the shared sanitizer
keeps only the run's own thread there, as it does for the list's rows (199 rows and 199
`readStates` entries removed from ten results; `readStates` is `{}` in nine of them and holds only the
TUI thread's entry in the tenth). The 9 UUID-shaped strings left in the herdr
transcript are not thread ids: three turn ids (the marker turn `01a1670b-...`, the tool turn
`01a1670c-50f6-...`), three `exec-` item ids, the user message's item id, and two
`readState` `revision` values. The four 32-hex strings in the stub's transcript are the W3C
`traceparent` trace ids of `tools/list` and the three `tools/call`.

**Third-party text.** Tool-result bodies, the approval request's `message`,
`requestedSchema` and `_meta`, and the harness instruction text in the `thread/resume` result
are elided by the driver's #130 rule: `<ELIDED tool-output bytes=N sha256=H>`. The stub's
results are OAC's own text, so the stub's transcript keeps them, and the hash on the wire
equals the sha256 of that text (with the pairing value as `<PAIRING_VALUE>`): 144 bytes
`dba4a10d...` (refusal), 34 bytes `e8a55bbc...` (paired), 44 bytes `65e6505b...` (served). So
`result.content` on the wire is the server's `content`, byte for byte (L125, L134, L143; the
text itself is in the stub's transcript, `call-out` events).

## What it shows (facts C1 to C10 of `spec/bindings/mcp.md` §4.5.1, read at `rust-v0.161.0`)

| Fact | At 0.163.0 |
|---|---|
| C1 `_meta` of `tools/call` | `callId`, `x-codex-turn-metadata`, `threadId`, `sessionId`, `windowId` as read, **plus `itemId` (a `ctc_...` id, not the item's id), `traceparent` and `progressToken`** (stub transcript, every `tools/call`). `tools/list` carried `traceparent` and `progressToken`. `callId` equals the item's `id` (`exec-<uuid v4>`). Not used by OAC. |
| C2 per-thread runtime | UNVERIFIED still: one thread, one server process (pid in the stub's `start` event) for all three calls. The process started when the TUI session opened, 57.6 s before the marker prompt (stub `start` 04:35:24.307Z; the marker turn's UUIDv7 time 04:36:21.879Z), and `initialize` and `tools/list` arrived then, before any prompt. Seen for one server (the stub) in this run. |
| C4 item shape | Members on the wire: `appContext`, `arguments`, `durationMs`, `error`, `id`, `mcpAppUi`, `pluginId`, `readOnlyHint`, `result`, `server`, `status`, `tool`, `type`; `item/started` params `item`, `startedAtMs`, `threadId`, `turnId`; `item/completed` has `completedAtMs` instead. The `mcpToolCall` member of `ThreadItem`, `McpToolCallStatus`, `McpToolCallResult` and `McpToolCallError` in `v2/ItemStartedNotification.json` and `v2/ItemCompletedNotification.json` at tag `rust-v0.163.0` (commit `83086188ae0614bd882818ff1aa23f3194962012`, fetched from `openai/codex` on 2026-10-22 through the GitHub contents API) equal those of the vendored `rust-v0.161.0` (compared as parsed JSON). `durationMs` is the call alone: 3, 4 and 3 ms (L125, L134, L143). |
| C5 order | Held for all three calls. `item/started` came before the stub saw the call by 23869 ms, 1537 ms and 1778 ms; `item/completed` came 18, 14 and 20 ms after the stub answered. Between, for each call: `thread/status/changed` `waitingOnApproval`, an `mcpServer/elicitation/request` (server-to-client, mode `form`), `serverRequest/resolved`, `thread/status/changed` active (L121-L124, L129-L133, L138-L142; the requests are L122, L130, L139). The stub saw each call 0, 2 and 2 ms after `serverRequest/resolved`. A refused call: `status` `failed`, `result` `{content, structuredContent: null, _meta: null}`, `error` `null`, **no `isError` member** (L125). Whether `item/completed` precedes the model seeing the result is not observable from the wire (UNVERIFIED). |
| C6 subscription | Held: the carrier's `thread/resume` (`{threadId, excludeTurns: true}`, L106) subscribed it, and all six `mcpToolCall` frames carry the TUI thread's `threadId`. |
| C9 no arguments | **Not exercised.** C9 is conditional: a call with no `arguments` member is reported `null`. That case was not produced. The model was asked to call "with no arguments at all"; the call reached the stub as `arguments: {}` and Codex reported `arguments: {}` (L120, L125), the same as for the call it was asked to make with `{}` (L137). Whether the model's first call carried an `arguments` member, and what Codex reports for a call that has none, are UNVERIFIED. |
| Reveal | The completed refusal item's `result.content` equals the refusal's `content` (hash above); the run's analysis recorded `completedCarriesIssuedValue: true` on the raw bytes (`scenarioData.g8.ordering[0]`). |
| Process tree | The stub (pid 25796, `node.exe`) is a child of a `codex.exe` whose argv carries `app-server` (pid 19396), which is a child of a `codex.exe` without it (pid 13292) whose parent is `powershell.exe`: the shape of a TUI started from a shell (inferred from the parent and the missing argument; the process table shows no more). Pid 13292 has two children with `app-server` in their argv: 19396 and 27448. The snapshot lists six codex processes; it does not show which one serves the control socket (UNVERIFIED). |

## The approval step

The wire shows an approval step for each call, and the operator reports three approval dialogs
in the TUI and that "Allow" was chosen each time (an operator report). The dialogs' wording and
option list are **UNVERIFIED**: they were not captured (Codex keeps no copy, and the dialogs were
dismissed before the operator was asked). The model's own account is no evidence either way: asked
afterwards, it said it had sent no dialogs, although it had asked three times (operator report);
model self-reports are unreliable, which is why the wire capture is the evidence here. The step resolved 23.9 s, 1.5 s and 1.8 s after
`item/started` (`serverRequest/resolved` L123, L132, L141). The first interval fits a person
reading a dialog; the other two are short. So the answerer is **inferred** to be the operator,
through the TUI, from the operator's report and the timing; the wire does not say which
connection answered, and the capture client answers nothing (read from its code and
unit-tested, not recorded). The driver's hash of the harness config files is the same before
and after the run (`harnessConfig.unchanged` true), so no answer that writes configuration (an
"always allow") was made. Whether a policy could also have answered is not excluded.

**Who receives the request.** The `mcpServer/elicitation/request` frames (L122, L130, L139)
reached the capture client's connection, which had subscribed with `thread/resume` only. So
the request goes to every subscriber of the thread, not only to the TUI. An OAC adapter that
subscribes to a thread this way must never answer an `mcpServer/elicitation/request` (or any
server-to-client request): answering one decides a tool call's approval for the person using the
TUI (`oac-security-work`, permission relay). Risks row 88.

## Other observations

- **"Hook failed".** The operator reports that the TUI printed `Hook failed - hook exited with
  code 1` on the marker prompt. That it is a hook in the operator's own Codex configuration and
  nothing of OAC's is inferred (OAC registers no hook here; the configuration was not read).
- The model summarised the three results correctly ("Worked for 36s", operator report); no
  follow-up message was needed.
