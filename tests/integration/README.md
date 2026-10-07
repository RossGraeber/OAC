# tests/integration/

Fake-endpoint and opt-in live-provider integration tests
(`docs/planning/v0.1/07-repository-and-dependencies.md` §2, `tests/` row;
`docs/planning/v0.1/09-test-strategy.md`).

Empty at the scaffold (#50). This directory is a leaf: it is not a workspace member, and no
product crate or root manifest may pull it into a default build
(`scripts/check-herdr-containment.mjs`, check 9).

The CI-default fake-harness tier does not live here: the fakes are under `tests/fakes/`
(#318), and the tier runs as `cargo test --workspace` in `.github/workflows/ci.yml`
(for example `transports/memory/tests/pipelines.rs`).

## Provider integration: opt-in and pinned (#61, F12)

Provider integration is the tier that drives a real, logged-in Claude Code or Codex
(`oac-testing` §1-2; 09 §4). It is opt-in by construction and never runs in the default tier.

What exists today:

- The herdr scenarios in `tools/herdr/scenarios/` (gates G1, G2, G4, G5, the smoke run),
  started by hand (`node tools/herdr/run.mjs`) or by `.github/workflows/herdr-provider-optin.yml`,
  which runs only on a manual dispatch or a push to main that changes `docs/planning/PINS.md`,
  on operator-owned self-hosted runners. Pinned: herdr must match its PINS.md row exactly
  (another version is NOT RUN); each run records the harness version it ran on and prints a
  `VERSION WARNING`, never a failure, when that differs from PINS.md's minimum or last tested
  version (#216).

What is pending: the provider-integration tests of the real adapters, which arrive with
Epic G (G4 Claude adapter, G7 Codex adapter) and H1/H4. They follow the herdr reuse contract
(`tools/herdr/README.md`, "Reuse contract"): they live here, drive the harness through the
driver as a separate process, and run only from the opt-in workflow.

What CI enforces so they stay opt-in:

- `scripts/check-workflows.mjs` (boundary-lint job `workflow-policy`) fails any default-tier
  workflow (one with a trigger other than `workflow_dispatch`, the herdr workflow aside) that
  names a secret, a provider credential, an `OAC_TEST_*` opt-in variable, an `--ignored` test
  run, the herdr driver, a self-hosted runner or a harness CLI install.
- Check 9 keeps this directory a leaf and keeps the driver out of everything else.
