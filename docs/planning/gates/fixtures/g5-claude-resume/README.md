# G5 resume live leg: registration across start, `--resume` and `--continue` (Claude Code 2.1.312)

> **Not a gate run and not verdict-bearing.** Issue #66, box 4. Record:
> `docs/planning/gates/herdr-runs/G5-resume-2026-10-31.md`; run manifest
> `docs/planning/gates/herdr-runs/G5-resume-2026-10-31.run-manifest.json` (run `20261031T032533Z-ad4023`,
> outcome PASS, driver commit `4f256b5`, herdr `0.9.1`).
>
> The files are the driver's captures, renamed only: each one's sha256 equals the manifest's
> `captures[].sha256` for the name in brackets.

| File | What it holds |
|---|---|
| `pane-claude-A-2026-10-31-2.1.312-herdr.txt` (`pane-claude-A.txt`) | Run A, fresh: the command line and Claude Code's `--output-format json` result (`session_id`, the `whoami` result text). |
| `pane-claude-B-2026-10-31-2.1.312-herdr.txt` (`pane-claude-B.txt`) | Run B, `--resume <CLAUDE_SESSION_1>`. One pane served all three runs, so this capture also holds run A's output above run B's. |
| `pane-claude-C-2026-10-31-2.1.312-herdr.txt` (`pane-claude-C.txt`) | Run C, `--continue`, below runs A and B. |
| `sessions-{1,3,5}-{A,B,C}-during-2026-10-31.txt` | `oac sessions --json` while each run was alive: one session, `harness` `claude`, `state` `online`. |
| `sessions-{2,4,6}-{A,B,C}-after-2026-10-31.txt` | `oac sessions --json` after each run exited: `sessions_total` 0. |
| `events-2026-10-31.jsonl` (`events.jsonl.txt`) | The example daemon's events: six `accepted` and six `ended`, and its log lines, including the one SC-ID-142 diagnostic in run C (line 22). |
| `pane-daemon-2026-10-31-herdr.txt` (`pane-daemon.txt`) | The daemon pane, through `stopped`. |

## Redaction

All by the driver (`tools/herdr/lib/redact.mjs` and the scenario's run literals). No file was edited afterwards.
- OAC session ids are `<OAC_SESSION_A>`, `<OAC_SESSION_B>` and `<OAC_SESSION_C>`. Claude Code's `--output-format json` results all carry the same `session_id`, `<CLAUDE_SESSION_1>`. Message `uuid` values in those results are not redacted; they are not personal data.
- The Windows account SID is `<SID>`, and the user name and scratch paths are replaced.
- The `device_fingerprint` in the `whoami` results is of the run's in-memory key, which is made for the run and kept nowhere.
- A known over-redaction: the user name is also replaced inside the extension identifier `io.github.<USER>raeber/...`.
