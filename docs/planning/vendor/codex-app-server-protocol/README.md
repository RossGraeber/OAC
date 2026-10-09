# Vendored: Codex app-server protocol JSON schema

The JSON Schema for the Codex `app-server` protocol, copied unmodified from
`openai/codex`. The Codex adapter (`adapters/codex/`, G6 #67 onward) is hand-written on
`oac-core` against these shapes, and its tests check against them. No Codex crate is a
dependency of OAC: each reaches a model API client, a keyring store or the rollout files
(decision record `docs/planning/decisions/G-7-stage4-dependencies.md` §2, D5).

| Field | Value |
|---|---|
| Upstream | https://github.com/openai/codex, `codex-rs/app-server-protocol/schema/json/` |
| Tag | `rust-v0.161.0` (tag object `7e21416b38834816c224ea0dfd135c3de94b2f15`) |
| Commit | `979011409de0a60b52f179721948e65531d26144` |
| Tree of `schema/json` | `a75f7eb21162b7fcba3102cf304d6f56099826d5` (315 files: 37 at the top level, 276 under `v2/`, 2 under `v1/`) |
| License | Apache-2.0 (`LICENSE`, blob `4606e72e042564097e8780d66c1d4dcb611869bd`); `NOTICE` (blob `2805899d56d0332d175cfc613c67d45d6f006db7`) copied beside it, as Apache-2.0 §4(d) asks |
| Retrieved | 2026-10-08 |
| Codex version | `0.161.0`, the last tested Codex (`docs/planning/PINS.md` "Version policy": Codex floats, so this is a recorded snapshot, not a gate) |

Layout: `rust-v0.161.0/json/` is the upstream `schema/json/` directory byte for byte (the
git tree hash above holds after checkout normalisation; verify with
`git write-tree --prefix=docs/planning/vendor/codex-app-server-protocol/rust-v0.161.0/json/`
on a clean index). `rust-v0.161.0/LICENSE` and `rust-v0.161.0/NOTICE` are the repository's
own files at the same commit.

**What this schema does not hold.** It is the default, stable schema. Experimental methods,
`thread/queue/add` among them, appear only in
`codex app-server generate-json-schema --experimental` output (`oac-codex-appserver`
skill, "Live injection"). Their shapes come from that output and from the recorded
fixtures under `docs/planning/gates/fixtures/`, and the task that needs them (G6 #67, G7
#68) records where it took them from.

**Where, and why here.** Provider-derived test data lives under `docs/planning/` and is
read in place, as the recorded fixtures are (07 §1). It is not under `tests/` because its
descriptions name the provider, which boundary-lint check 3 refuses in the code tree. It is
not a recorded harness capture, so it is outside `docs/planning/gates/fixtures/` and its
`MANIFEST.json` (`scripts/check-fixture-manifest.mjs` covers captures only). Never shipped,
and never vendored into product code: a test reads it at run time.

**Refreshing.** A new snapshot goes in a new `rust-v<version>/` directory beside this one,
copied the same way, with this table extended; the older one is removed only when no test
reads it.
