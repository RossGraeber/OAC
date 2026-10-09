# tests/security/

The security suite against the fakes (#60, F11). It covers spoofing, tampering, replay,
duplicates, unauthorized routing, provenance, receipts, presence, pairing, key removal,
never-steer, signature forms and exhaustion. See
`docs/planning/v0.1/07-repository-and-dependencies.md` §2 (`tests/` row) and
`docs/planning/v0.1/09-test-strategy.md` §7 and §12.

The crate is `oac-security-suite`. It is CI-default: it runs in the plain
`cargo test --workspace`, with no live provider, no API key and no network beyond loopback.

- **Dependencies.** It depends on `core/`, on the fake Claude Code endpoint
  (`tests/fakes/claude/`) and on the in-memory transport (`transports/memory/`). Its only
  test-only dependency is `syn`, which is already in the graph.
- **Nothing depends on it.** `scripts/check-crate-deps.mjs` enforces this.

## Files

- **`src/lib.rs`** holds the shared test world and the threat map.
  - The world is a `Device` with its engine, duplicate store, pairing store and clock. It
    drives the real receive path.
  - `THREATS` maps each threat row to:
    - the tests that prove it;
    - the harness facts it relies on (preconditions, not proofs);
    - the `oac-core` tests that carry what this suite cannot reach;
    - what is gated, and on which issue.
- **`tests/*.rs`** hold one file per threat family. Each test is named for the row it
  belongs to and cites it in its doc comment:
  - `rowNN_` for `06-security.md` §14 row NN;
  - `s13_` for a `spec/security.md` §13 row;
  - `x_` for a row from an earlier PR's threat table.
- **`tests/fixtures.rs`** sends the recorded signature-form conformance fixtures
  (`tests/protocol/sec-sig/`, `sec-key/`) through the same receive path.
- **`tests/threat_map.rs`** checks `THREATS` against the parsed test sources (every file
  under `tests/`), against `spec/security.md` §13 and against the table in
  `09-test-strategy.md` §12.
- **`check-compiled-tests.mjs`** checks the same map against what cargo actually compiled,
  which no parser sees fully (`#[cfg]`, `#[path]`, `tests/<dir>/main.rs`, macro-generated
  tests, other spellings of the test attribute). It lists every test target's tests with
  `--list` and `--list --ignored`, and runs the ignored ones. It fails when:
  - a mapped proof or fact is not compiled exactly once, or is ignored;
  - a gated placeholder is not ignored, or passes when run;
  - a compiled test is in no row.

  It also fails on a test no listing sees (#329): a target with doc-tests on (the crate
  sets `[lib] doctest = false`), and a target whose `--list` is not libtest's (a
  `harness = false` target). `tests/threat_map.rs` refuses both statically, and refuses a
  function that is a test only through `#[cfg_attr(.., test)]`.

  `node scripts/local-ci.mjs` runs it and its `--self-test` (the client-side CI; run it on
  Windows and in WSL). `--self-test` plants each known evasion in a copy and checks it
  is caught (the hosted CI ran it on the ubuntu image only). `--self-test --work-dir <dir>` puts the copy
  and its `target/` under `<dir>` (default `target/security-compiled-tests`), so a short
  path keeps Windows under its path-length limit. `<dir>` must be outside the repository
  or under its `target/`; both scripts refuse one inside the sources.
- **`gated_*` tests** hold the place of mitigations that need a component not built yet:
  - Epic G adapters;
  - the G9 daemon and IPC;
  - H2 and L10.

  Each is `#[ignore = "GATED on #N ..."]`. Its body is a single `std::panic!`, so it fails
  if run and is never a pass.
- **Un-gating a G4, G7 or G8 placeholder** means driving a real adapter. The crate rule does
  not let this suite reach `adapters/*` today. That rule must first admit an adapter as a
  dev-dependency, or the test must move into the adapter's crate.

## Mutation check

`mutation-check.mjs` plants one regression at a time in a copy of `core/` and checks that
the suite catches each one.

- A mutant marked `coreOnly` is one the suite cannot reach through the public API. For
  example, a pairing code that drops the keys cannot be caught when the nonces are random.
  Such a mutant must instead be caught by the `oac-core` test that `THREATS` cites.
- It is the one check that catches a proof which returns early before it asserts.
- The default CI tier does not run it, because it recompiles the core for each mutation.
  It is an opt-in tier of its own (#329): `node scripts/local-ci.mjs --tier mutation`, run
  by hand when the suite or a core mitigation changes. (Until 2026-10-08 a hosted workflow,
  `security-mutation-optin.yml`, also ran it weekly; there is no GitHub-hosted CI now, so
  nothing runs it on a schedule.)

```sh
node tests/security/mutation-check.mjs          # all mutations
node tests/security/mutation-check.mjs --only "row 4"
node tests/security/mutation-check.mjs --work-dir <dir>   # default: target/security-mutation
```
