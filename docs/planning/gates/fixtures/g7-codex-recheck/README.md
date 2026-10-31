# G7 recheck live run: risk rows 66 and 73 on Codex 0.166.0 (#470)

> **Not a gate run and not verdict-bearing.** A G7 follow-up, issue #470 (risk rows 66 and 73, from G7 #68). Record:
> `docs/planning/gates/herdr-runs/G7-recheck-2026-10-31.md`; run manifest
> `docs/planning/gates/herdr-runs/G7-recheck-2026-10-31.run-manifest.json` (run `20261031T094523Z-f1de3d`, outcome PASS,
> driver commit `56aeb3b`, herdr `0.9.1`, Codex `0.166.0`).
>
> The transcript is exactly what the driver wrote, under the driver's own name. Its sha256 is the run manifest's
> `captures[].sha256`.

| File | What it holds |
|---|---|
| `transcript-g7recheck-2026-10-31-0.166.0-herdr.jsonl` | The quarantined client's own wire transcript: 828 lines, one JSON object each (`t`, `mode`, `direction`, `payload`), in the order the frames were sent and received. Lines with `"mode":"g7-observation"` are the client's observations, each citing the request and answer it came from. |
| `client.mjs.throwaway-quarantined` | The client that wrote the transcript. Throwaway, not product code, kept under the `.throwaway-quarantined` suffix (the oac-gates "Throwaway rule"). The scenario stages it from its blob and checks its sha256. Its manifest entry says what it may send (an allow-list; never `turn/steer`, never an approval answer) and that it records no frame about a thread it did not start. |
| `README.md` | This file. |

**Where the cases are in the transcript** (line numbers are the `t`-ordered lines of the committed file):

| Lines | Content |
|---|---|
| 1 to 5 | `hello` mode: the handshake the client made before the cases. |
| 6 to 128 | Row 66 (a), first half, on a thread of its own (`a1-main`): busy turn (22), two adds (38 to 45), interrupt (46 to 52), quiet window (54), the next turn (55 to 77), the two queued items (102, 127, 128). |
| 129 to 191 | Row 66 (a) second half and (d), on the `dac-main` thread: a completed first turn (167), the add with an extra member (168 to 172), its turn (191). |
| 192 to 265 | Row 66 (a), second half: busy turn (194), interrupt (201 to 213), add (214 to 217), quiet window (219), next turn (220 to 242), queued item (265). |
| 266 to 283 | Row 66 (c): `thread/unsubscribe` (266), the thread closed (272 and 273), `thread/loaded/list` (276), the add (277 to 279), the quiet window (280), still unloaded (283). |
| 284 to 329 | Row 66 (c), resume and dispatch: `thread/resume` from a new connection (289, observed 297), the queued item runs (329). |
| 330 to 474 | Row 66 (b): a busy turn (345), three adds (357 to 368), interrupt (369 to 375), the second client `b-other`, which never starts or resumes the thread, lists, updates, reorders and deletes (376 to 399), the held check (400), the next turn (401 to 424), the dispatch (474). |
| 475 to 550 | Row 73, state 1 (loaded, idle, with subscribers): start (480 to 483), override chosen (484), first turn (485 to 516), subscribers (529), override resume (537 to 540), the quiet window for a `thread/settings/updated` (550). |
| 551 to 559 | Row 73, state 1, readback: a resume without the override (556, observed 559). |
| 560 to 660 | Row 73, state 2 (loaded, idle, no subscriber): start (560 to 572), first turn (573 to 609), `thread/unsubscribe` (621 to 624), loaded (625 to 628), override resume (629 to 648), the events after it (657), readback (658 to 660). |
| 661 to 795 | Row 73, state 3 (not loaded, the control): start (661 to 675), first turn (676 to 713), `thread/unsubscribe` (714 to 720), thread closed (741), not loaded (742 to 749), override resume (755 to 776), readback (783 to 787). |
| 796 to 828 | Cleanup: six `thread/archive` requests (801, 806, 810, 814, 819, 824), one for each thread the client started, each answered with its own id (805, 809, 813, 818, 823, 828). |

## Redaction

All by the driver: `tools/herdr/lib/redact.mjs` and the shared `sanitizeTranscript` of `tools/herdr/lib/g2.mjs`, plus the
run's own literals (the scratch directory, the home directory, the host and the installation id). The client also
stubs other threads itself. No file was edited afterwards. `captures[].redaction` in the run manifest records
`droppedHazardLines` 0, `hazardProtocolFrames` empty, `residualLeaks` empty and `residualGenericHits` empty.

The placeholders that appear in the committed transcript, counted in the file (each is a whole JSON string value):

| Placeholder | Count | Where |
|---|---|---|
| `<SCRATCH>` | 118 | The client's scratch directory: `cwd` in the requests and thread objects, `runtimeWorkspaceRoots`, and environment entries. The manifest records 118 literal replacements. |
| `<USER_HOME>` | 71 | The Windows home directory: `codexHome` (`initialize` results), `thread.path`, a hook's `sourcePath` and run `id`. The manifest records 71 home replacements. |
| `<HOST>` | 15 | The machine's name, the `serverName` of `remoteControl/status/changed`. |
| `<INSTALLATION_ID>` | 15 | The `installationId` of `remoteControl/status/changed`. A run literal. |
| `<REDACTED>` | 48 | Account fields: `planType` of `account/updated` (14), and `planType` (17) and `credits` (17) of `account/rateLimits/updated`. |
| `<NOT-OWN-THREAD>` | 5 | Written by the client, not the sanitizer: a stub for another thread's id in a `thread/loaded/list` result (the client never sends `thread/list`) (lines 275, 282, 518, 627 and 747). The client records no frame about a thread it did not start. |
| `<ELIDED tool-output bytes=23 sha256=ec3e00ab...>` | 7 | The `text` of a hook run entry in `hook/completed` (lines 28, 150, 308, 351, 497, 584 and 692). Elided under #130; the 23-byte text has the same digest each time. The hook is the operator's `sessionStart` hook in `~/.codex/hooks.json`, which reported `failed`. |

Kept, deliberately:

- `account/rateLimits/updated` keeps `usedPercent` (26, in each of its 17 records) and the reset times. `usedPercent` is a
  usage figure, not personal data.
- The thread ids are the client's own scratch threads, all archived in this run (lines 801 to 828). They are not secrets.
- The prompts are OAC test prompts (`OAC G7 recheck: reply with exactly the words ...`). The `agentMessage` texts are the
  model's replies to them. The item types in the transcript are `userMessage` and `agentMessage` only: there is no
  command, file or MCP tool item.
- The `<ELIDED>` digest is of the elided hook text. It identifies nothing else.
