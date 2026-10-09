# tests/integration/

Fake-endpoint and opt-in live-provider integration tests
(`docs/planning/v0.1/07-repository-and-dependencies.md` §2, `tests/` row;
`docs/planning/v0.1/09-test-strategy.md`).

Empty at the scaffold (#50). This directory is a leaf: it is not a workspace member, and no
product crate or root manifest may pull it into a default build
(`scripts/check-herdr-containment.mjs`, check 9).

The CI-default fake-harness tier does not live here: the fakes are under `tests/fakes/`
(#318), and the tier runs as `cargo test --workspace` in `node scripts/local-ci.mjs`, the
client-side CI (there is no GitHub-hosted CI since 2026-10-08), for example
`transports/memory/tests/pipelines.rs`.

## Provider integration: opt-in and pinned (#61, F12)

Provider integration is the tier that drives a real, logged-in Claude Code or Codex
(`oac-testing` §1-2; 09 §4). It is opt-in by construction and never runs in the default tier.

What exists today:

- The herdr scenarios in `tools/herdr/scenarios/` (gates G1, G2, G4, G5, the smoke run),
  started by hand (`node tools/herdr/run.mjs`) or by `.github/workflows/herdr-provider-optin.yml`,
  which runs only on a manual dispatch (its push-on-PINS.md trigger went on 2026-10-08),
  on operator-owned self-hosted runners. Pinned: herdr must match its PINS.md row exactly
  (another version is NOT RUN); each run records the harness version it ran on and prints a
  `VERSION WARNING`, never a failure, when that differs from PINS.md's minimum or last tested
  version (#216).

What is pending: provider-integration tests of OAC's own adapters, which cannot exist before
Epic G. The backlog names two:

- G4 (#65), the Claude adapter: its acceptance runs the adapter contract suite "against
  real Claude Code behind the opt-in tier";
- H1 (#73), the end-to-end test: live Claude Code and Codex sessions, opt-in by
  construction.

G7's acceptance (the Codex adapter) names no real-harness test, and H4 is CI-default CLI
smoke, not provider integration. These tests follow the herdr reuse contract
(`tools/herdr/README.md`, "Reuse contract"): they live here, drive the harness through the
driver as a separate process, and run only from the opt-in workflow.

### G4's live leg: planned, deferred to wave 4

The Claude adapter (#65) passes the adapter contract suite against the fake Claude Code
endpoint (`adapters/claude/tests/contract.rs`, CI-default). Its live leg, the same contract
against real Claude Code, waits for the daemon and the `oac mcp-shim` stdio relay (G9,
#70): without them nothing can start the adapter's channel server as a process Claude Code
spawns. It is wave 4 work, planned here so that it is not designed twice:

- **Where and how.** A herdr scenario under `tools/herdr/scenarios/` (Claude Code started
  with `--dangerously-load-development-channels server:oac`, the shim as the stdio server),
  with its assertions here, run only from the opt-in workflow or by hand. The driver
  handles the three Claude Code dialogs exactly as
  `docs/planning/decisions/K-196-driver-accepts-dialogs.md` decides; this plan changes none
  of that, and no run pre-answers or suppresses the development-channels confirmation by
  any other route.
- **What it asserts**, each against the pane text or the shim's wire log of that run:
  1. the channel registers on the legacy era with the adapter's own `initialize` result,
     which carries `capabilities.extensions` (binding hook H13, [MCPB-ERA-008]); if Claude
     Code refuses or ignores the channel with `extensions` present, that is a finding for
     the lead under the reversal condition of `spec/bindings/mcp.md` §4.3, not a spec edit;
  2. an idle session wakes on one hand-off, and the `<channel>` tag carries `source` and
     C6's five `oac_*` attributes with the verified values, no others;
  3. two hand-offs into a running turn are taken in order, whatever the grouping
     (11-risks row 49);
  4. `send` and `list_sessions` from the model reach the core and return its results;
  5. no `claude/channel/permission` is declared, and no permission prompt is relayed;
  6. a sender-written `<\/channel>` is captured, closing the `RenderGap` the security
     suite's 06 row 16 keeps gated on #65.
- **Recorded**: the Claude Code version (a warning, never a stop, when it differs from
  PINS.md, #216) and the `rmcp` pin, which answers 11-risks row 16.

What `node scripts/local-ci.mjs` enforces so they stay opt-in:

- Its `--self-test` fails a default-tier step that runs an `--ignored` test, sets an
  `OAC_TEST_*` opt-in variable, reaches the herdr driver beyond its offline self-test or
  installs a harness CLI (the policy `scripts/check-workflows.mjs` D2 and D3 held over the
  hosted default tier until 2026-10-08).
- `scripts/check-workflows.mjs` fails any workflow that names a secret or a provider
  credential (W4), starts other than by `workflow_dispatch` (W7) or runs on a GitHub-hosted
  runner (W8).
- Check 9 keeps this directory a leaf and keeps the driver out of everything else.
