# tests/security/

The security suite against the fakes (#60, F11): spoofing, tampering, replay, duplicate,
unauthorized-routing, provenance, receipt, presence, pairing, key-removal, never-steer and
exhaustion tests (`docs/planning/v0.1/07-repository-and-dependencies.md` §2, `tests/` row;
`docs/planning/v0.1/09-test-strategy.md` §7 and §12).

Crate `oac-security-suite`. CI-default: it runs in the plain `cargo test --workspace` with no
live provider, no API key and no network beyond loopback. It depends on `core/`, the fake
Claude Code endpoint (`tests/fakes/claude/`) and the in-memory transport
(`transports/memory/`) only, and nothing depends on it (`scripts/check-crate-deps.mjs`).

- `src/lib.rs`: the shared test world (a `Device` with its engine, duplicate store, pairing
  store and clock, driving the real receive path) and `THREATS`, the threat-row to test map.
- `tests/*.rs`: one file per threat family. Each test is named for the threat row it proves
  (`rowNN_` for `06-security.md` §14 row NN, `s13_` for a `spec/security.md` §13 row) and
  cites it in its doc comment.
- `tests/threat_map.rs`: checks `THREATS` against the sources and against the table in
  `09-test-strategy.md` §12.
- `gated_*` tests hold the place of mitigations that need a component not built yet (Epic G
  adapters, the G9 daemon and IPC, the #313 pipelines, #325). Each is
  `#[ignore = "GATED on #N ..."]` and fails if run: it is never a pass.
- `mutation-check.mjs`: plants one regression at a time in a copy of `core/` and checks that
  the suite catches each. Not run by CI (it recompiles the core per mutation):

  ```sh
  node tests/security/mutation-check.mjs          # all mutations
  node tests/security/mutation-check.mjs --only "row 4"
  ```
