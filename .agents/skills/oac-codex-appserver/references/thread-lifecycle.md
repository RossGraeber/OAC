# Thread lifecycle: auto-subscription and lazy rollout materialization

Findings from OAC issue #39 D6/T5 (Codex live app-server protocol fixture capture),
re-examining `openai/codex` source at commit `36650394c5b38c2990ccf2a3457165ca3e9d9726`
(`rust-v0.157.1`), retrieved 2026-09-27. Fixture evidence:
`docs/planning/gates/fixtures/d6-codex-protocol/` (`README.md`,
`transcript-conn*-attempt1-failed-resume-*.jsonl` for the failure this caused,
`transcript-conn*-2026-09-28.jsonl` for the corrected sequence).

## Auto-subscription is not `thread/resume`-only

The G2 gate result (`docs/planning/gates/G2-result.md`) originally stated, correctly
for what it tested, "a client must call `thread/resume` to receive `turn/*`/`item/*`
events." D6/T5 found this understates the actual rule: **the connection that calls
`thread/start` also auto-subscribes**, without a separate `thread/resume`. Source:
`codex-rs/app-server/src/request_processors/thread_processor.rs` L1562-1580 (auto-attach
on `thread/start`), L4009-4015 (`thread/resume` also subscribes its caller — unchanged
from the G2-era understanding), L5227 (`thread/fork` also auto-attaches). `turn/start`
and `thread/queue/add` do **not** themselves subscribe the caller — **UNVERIFIED**,
inferred from the same file's request-handling structure not carrying an equivalent
"Auto-attach a thread listener" comment/call near either handler; not directly
source-confirmed the way the three auto-attach sites above are. Re-check before relying
on this for anything security- or correctness-sensitive. This UNVERIFIED item is also
recorded in `docs/planning/STATUS.md` and `docs/planning/v0.1/11-risks.md`.

D6/T5 attempt 2 exploited this directly: the `thread/start`-issuing connection stayed
open and observed all 3 `turn/completed` events for the run without ever calling
`thread/resume` itself (`docs/planning/gates/fixtures/d6-codex-protocol/
transcript-conn1-2026-09-28.jsonl`).

## Rollout materialization is lazy, keyed on the first user message

A thread's rollout file (`CODEX_HOME/sessions/.../rollout-<timestamp>-<threadId>.jsonl`
— not a supported surface, per this skill's "The rollout file is not a supported
surface" note) is **not** created at `thread/start`. It is created lazily, when the
thread's first user message is
appended. Source: `codex-rs/rollout/src/recorder.rs` L972 and L1775
(`deferred_creation`); `codex-rs/core/src/session/mod.rs` L4913-4915
(`ensure_rollout_materialized`, invoked right after the `userMessage` item is
appended).

`thread/resume` always reads the stored thread from disk first, before anything
else — source: `codex-rs/app-server/src/request_processors/thread_processor.rs`
L4245-4254 — and if no rollout exists yet, fails with a JSON-RPC error whose message
is `codex-rs/thread-store/src/local/read_thread.rs` L97-101's
`"no rollout found for thread id <id>"` (code `-32600`).

**Consequence:** calling `thread/resume` on a thread before any `turn/start` has run
on it will *always* fail this way, regardless of how long the caller waits between
`thread/start` and `thread/resume` — there is no race to retry past. D6/T5's attempt 1
hit this twice (waited ~12s, then ~86s, both failed identically) before the structural
cause was found; the fix is sequencing, not backoff: run a turn on the same connection
that called `thread/start` (or any connection) before any connection calls
`thread/resume` on that thread.

A related fact for anyone building a Codex adapter: a thread left with no subscribers
unloads after `thread_unload_delay` (default 60s; `codex-rs/core/src/config/mod.rs`
L3876-3877; unload-trigger logic `codex-rs/app-server/src/request_processors/
thread_lifecycle.rs` L56-79, `unloading_target`/`should_unload_now`), and a connection
can be explicitly unsubscribed from a thread
(`codex-rs/app-server/src/thread_state.rs` L512-538,
`unsubscribe_connection_from_thread`) — do not assume a subscription survives a
reconnect; re-subscribe explicitly (`thread/resume`) after any reconnect.
