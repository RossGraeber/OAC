# d6-codex-protocol fixtures

Captured for OAC issue #39 (D6 protocol fixtures), subtasks T5 (capture) and T7
(redaction, commit, schema record). Codex CLI / app-server `0.157.1`, commit
`36650394c5b38c2990ccf2a3457165ca3e9d9726` (matches `docs/planning/PINS.md`'s
Codex CLI / app-server floating-pin last-observed version at capture time — no
pin move between T2's schema regeneration and this capture).

## Files

Primary evidence (attempt 2, the retry that completed the full sequence):

- `transcript-conn1-2026-09-28.jsonl` — `thread/start`, kept open watching 3x
  `turn/completed` (auto-subscribe on the `thread/start` connection).
- `transcript-conn2-2026-09-28.jsonl` — `thread/resume` (`excludeTurns: true`),
  watching 2x `turn/completed`.
- `transcript-conn3-turn1-2026-09-28.jsonl`, `transcript-conn3-turn2-2026-09-28.jsonl`
  — the two `turn/start` calls.
- `transcript-conn3-queue-2026-09-28.jsonl` — `thread/queue/add`, sent while turn 2
  was still busy (confirmed: the response timestamp precedes conn2's second
  `turn/completed` — see `SESSION-LOG-codex.md`'s "Added while busy" check).

Negative fixtures (attempt 1, kept deliberately — see "Attempt 1" below):

- `transcript-conn1-attempt1-failed-resume-2026-09-27.jsonl` — `thread/start` on
  the thread that was never materialized.
- `transcript-conn2-attempt1-failed-resume-try1-2026-09-27.jsonl`,
  `transcript-conn2-attempt1-failed-resume-try2-2026-09-27.jsonl` — the two
  consecutive `thread/resume` calls that both failed `-32600 "no rollout found
  for thread id ..."`.

Quarantined driver/tooling scripts (`oac-gates` "Throwaway rule" — not OAC
source code):

- `client.mjs.throwaway-quarantined` — thin app-server JSON-RPC client
  (`start`/`resume`/`turn`/`queue` modes).
- `run-t5.mjs.throwaway-quarantined` — orchestrates the attempt-2 sequence,
  event-gated (no fixed sleeps), enforcing the operator-approved caps (exactly
  one `thread/start`, at most 3 model turns).
- `redact.mjs.throwaway-quarantined` (sha256
  `371d79640a661180e15e3c4afc788531567f1bbbfdf286ac542608aecf381872`) — the
  redaction script used on every fixture in this directory (see "Redaction"
  below). Committed here so redaction is independently reproducible.
  **This is a sanitized copy**, not byte-identical to the script that actually
  produced these fixtures (sha256
  `975de80c6462bdf1bbc21ee05e37730cde5cdf7ef74e42a556270a46ccbd9a3d`, never
  committed as that version): the version that ran hardcoded the operator's OS
  username, machine hostname, and a Codex installation UUID as literal string
  constants, which is a public-repo leak (T7 security fix, issue #39). This
  committed copy reads those three values from `--username`/`--hostname`/
  `--installation-id` flags or `REDACT_USERNAME`/`REDACT_HOSTNAME`/
  `REDACT_INSTALLATION_ID` env vars instead, with username/hostname defaulting
  to `os.userInfo().username`/`os.hostname()` — verified to produce
  byte-identical output to the script that actually ran, against every raw
  transcript in this directory, when the correct values are supplied.
  **Second security fix (same issue #39 review, later pass):** that first
  sanitized version still failed open — a raw Codex app-server transcript
  carries the installation id and hostname as plain JSON field *values*
  (`"installationId":"<uuid>"` on every `initialize`/
  `remoteControl/status/changed`-shaped frame, `"serverName":"<hostname>"`
  alongside it), and the string-level regex scrub only fires when
  `--installation-id`/env is actually supplied or the running machine's own
  `os.hostname()` happens to match — neither is guaranteed. Without
  `--installation-id`, the previous version left the real installation id in
  the output while still reporting `residualLeaks=[]`/`residualGenericHits=[]`
  clean (the generic structural checks only look for a UUID adjacent to a
  `scratchpad`/`claude` path segment, which a bare JSON field is not). Fixed
  by making `installationId`/`serverName` key-based, unconditional
  replacements in the script's `walk()` function (independent of any flag),
  plus a dedicated fail-closed check that scans the redacted output for any
  `"installationId":"..."` value — including one hidden behind an extra layer
  of JSON-string escaping — that isn't exactly `<INSTALLATION_ID>`, and
  refuses to exit 0 if one is found. Re-verified against all raw inputs in
  this PR, both with and without the three flags: every run produces
  byte-identical output to the committed fixture, in both cases (the sha256
  above is this corrected version; the version between the two fixes,
  sha256 `c3faff3ba152f128e6ea439021ab3a589a8cd633744179e41d354b036b9031f0`,
  was likewise never committed as that version).
- `hash-tree.mjs.throwaway-quarantined` (sha256
  `73dbcb0524be80e70acdbaad95dd5d7767c2c077e5b686a2392f1bd6aa97d808`) —
  computes the deterministic tree hashes cited throughout "Schema reference"
  below.
- `validate-frames.mjs.throwaway-quarantined` (sha256
  `bb8e84ccdc1fbf1f58d7e10cb3aaa7f614dc7c0be5f92987765fd01261919e08`) —
  validates a transcript's frames against the checked-in app-server schema
  (see "Redaction" below for the T7 mapping fix and its effect on the pass
  counts).

## Attempt 1: why the negative fixtures are kept

Attempt 1 called `thread/resume` on a freshly-`thread/start`-ed thread before any
turn had run on it, twice (~12s and ~86s after `thread/start` returned per the
transcripts' own `t` fields), and both times got JSON-RPC error `-32600 "no
rollout found for thread id 01a0e513-b789-7050-ab5d-260dadbddb65"`.

Root cause (first-party, from `openai/codex` @
`36650394c5b38c2990ccf2a3457165ca3e9d9726`): a thread's rollout file is created
lazily, on that thread's first user message (`codex-rs/rollout/src/recorder.rs`
L972/L1775 `deferred_creation`; `codex-rs/core/src/session/mod.rs` L4913-4915
`ensure_rollout_materialized`), not at `thread/start`. `thread/resume` always
reads the stored thread from disk first (`codex-rs/app-server/src/
request_processors/thread_processor.rs` L4245-4254); the error itself is
`codex-rs/thread-store/src/local/read_thread.rs` L97-101. Since no `turn/start`
ever ran on that thread, no rollout could exist for either resume attempt to
find, regardless of how long they waited.

These are committed as **negative fixtures**, clearly named
`*-attempt1-failed-resume-*`, because they are first-party evidence of a real
protocol ordering requirement (`thread/resume` before any turn genuinely fails,
not just theoretically) that a Stage 3 fake Codex endpoint should be able to
reproduce, and because `oac-gates`' fixture-capture procedure keeps a spike's
disclosed failed attempts rather than omitting them. The thread they name
(`01a0e513-b789-7050-ab5d-260dadbddb65`) was never materialized and holds
nothing on disk — it is not expected to be resumable and no further action is
needed on it.

## Redaction

Redacted with the version of `redact.mjs` that actually ran (sha256
`975de80c6462bdf1bbc21ee05e37730cde5cdf7ef74e42a556270a46ccbd9a3d` — see "Files"
above for why a *different*-hashed, sanitized copy, sha256
`371d79640a661180e15e3c4afc788531567f1bbbfdf286ac542608aecf381872`, is what's
actually committed), each file
with `--keep-thread <that file's own thread id>` (`01a0e513-b789-7050-ab5d-
260dadbddb65` for the three attempt-1 files, `01a0e550-1921-7000-93ef-
383c5acff39f` for the five attempt-2 files). All eight files: `droppedHazardLines
=0`, `droppedHazardProtocolFrames=0`, `residualLeaks=[]`, `residualGenericHits=
[]`.

Raw (pre-redaction) frames were validated against the checked-in app-server
schema before redaction: **225/225 validated frames pass (0 fail)** across all
eight files combined (11+6+6 for attempt 1; 105+79+6+6+6 for attempt 2). Each
file has exactly one remaining unresolvable method, `initialized`, which
genuinely has no dedicated schema anywhere in the checked-in tree. This
supersedes an earlier, lower count (106/106) that undercounted: the original
`validate-frames.mjs` guessed each schema's name purely from the wire method
string (`pascalMethod(method) + "Notification"/"Params"/"Response"`), which
missed two real, dedicated schemas whose definition names do not match that
guess — `item/agentMessage/delta`'s params schema is `AgentMessageDeltaNotification`
(not `ItemAgentMessageDeltaNotification`) and `mcpServer/startupStatus/updated`'s
is `McpServerStatusUpdatedNotification` (not
`McpServerStartupStatusUpdatedNotification`) — both are `definitions` entries
inside `ServerNotification.json`'s `oneOf` discriminated union, not missing
schemas. Fixed by reading each union file's own `oneOf` member (`method.enum[0]`
paired with `params.$ref`) directly instead of guessing; see
`validate-frames.mjs.throwaway-quarantined`'s comments for the full mechanism.
`validation-codex-a2.txt` (committed in the scratchpad kit that produced this
directory, not itself a repo file) and `SESSION-LOG-codex.md` record the
pre-fix counts; this directory's own notes and `MANIFEST.json` entries record
the corrected, post-fix counts.

## Schema reference

- Schema commit: `36650394c5b38c2990ccf2a3457165ca3e9d9726`
  (`github.com/openai/codex`, `rust-v0.157.1`).
- Upstream path: `codex-rs/app-server-protocol/schema/json`.
- Local regeneration commands: `codex app-server generate-json-schema
  --out schema/default` (default tier) and `codex app-server
  generate-json-schema --experimental --out schema/experimental`
  (experimental tier).
- **Hash method** (`hash-tree.mjs.throwaway-quarantined`, sha256
  `73dbcb0524be80e70acdbaad95dd5d7767c2c077e5b686a2392f1bd6aa97d808`): sorted
  relative POSIX file paths under the tree; for each, one line
  `<path>  <sha256-of-that-file>` (two literal spaces between path and hash);
  all lines `\n`-joined with a trailing `\n`; the tree hash is the sha256 of
  that whole listing text.
- Local generation hashes (recorded at capture time in
  `schema/SCHEMA-META.json`):
  - default (no flag): 314 files, tree sha256
    `6b5c39357ee0552bfa97c544a6fd16b0e5828eebb721c5313932b773105fca87`.
  - `--experimental`: 440 files, tree sha256
    `61fd7c82f78e129008387f30aeb7272ee457792292f840e01292306555c89a01`.
- **`--experimental` is NOT a pure superset of the default tier.** Beyond the
  126 files that exist only under `--experimental` (new `v2/` definitions such
  as `ThreadQueueAddParams`, `ThreadSearchParams`, `realtime/*`, plus top-level
  `CurrentTimeReadParams`/`Response` and the `FuzzyFileSearchSession*` set),
  **28 files exist in BOTH tiers with different content**
  (`diff -rq schema/default schema/experimental`, independently re-run for
  this fix): notably `ClientRequest.json`, `ServerNotification.json`,
  `ServerRequest.json`, `ThreadStartParams.json`, `ThreadStartResponse.json`,
  `ThreadResumeParams.json`, `ThreadResumeResponse.json`,
  `TurnStartParams.json`, and `TurnSteerParams.json` — **`TurnSteerParams.json`
  already exists in the default tier**; it is not an experimental-only
  addition, contrary to an earlier draft of this record. `ThreadForkParams.json`
  / `ThreadForkResponse.json` are likewise present (with differing content) in
  both tiers, consistent with `thread/fork` being a stable (non-experimental)
  method per `oac-codex-appserver`'s `references/thread-lifecycle.md`.
- **Validated against:** default tier first, experimental tier as a fallback
  for a method whose schema exists only there (e.g. `thread/queue/add`,
  `tier=experimental` in `validate-frames.mjs`'s own PASS lines).
- **Upstream comparison (T7):** fetched all 314 files under
  `codex-rs/app-server-protocol/schema/json/` at commit
  `36650394c5b38c2990ccf2a3457165ca3e9d9726` directly from
  `raw.githubusercontent.com` (`gh api repos/openai/codex/git/trees/
  36650394c5b38c2990ccf2a3457165ca3e9d9726?recursive=1` for the file list, then
  one `curl` per file, 2026-09-28), and ran the same `hash-tree.mjs`
  deterministic hash over the fetched tree.
  - **Result: MATCH against the local default (no-flag) generation.** 314/314
    files, tree sha256 `6b5c39357ee0552bfa97c544a6fd16b0e5828eebb721c5313932b773
    105fca87` — byte-identical to the local default generation above.
  - **Result: the local `--experimental` generation is a local-generation-time
    addition, not a pure superset** (see "`--experimental` is NOT a pure superset"
    above) — the upstream checked-in `schema/json` directory only ever held the
    314 default-tier files; neither the 126 experimental-only files nor the 28
    shared-but-differing files' experimental-tier content are checked in
    upstream as static files at this commit. **Conclusion: no schema drift
    found** for the default tier, which is what this comparison can directly
    verify against upstream — the default-tier schema this kit generated
    locally at pinned `codex-cli 0.157.1` is byte-identical to the schema
    checked in upstream at the exact pinned commit. The experimental tier
    (including its 28 files that differ from the default tier's own content)
    has no upstream checked-in counterpart to compare against at this path/
    commit, so "no drift" is stated for the default tier only.
- The schema tree itself is **not committed** to this repository (440/314
  generated files each) — only this hash-based record and the regeneration
  commands, per the T7 task brief's preference. Regenerate against Codex CLI
  `0.157.1` to reproduce.
