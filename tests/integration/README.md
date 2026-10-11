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


## G3 native TLS two-host leg (#64) - planned, NOT RUN

Run only with explicit LAN opt-in (`node scripts/local-ci.mjs --tier lan` for the
existing host probe); no live harness, no Codex daemon. A future two-host driver must
also require `OAC_TEST_LAN=1` and reject absent opt-in before opening any LAN listener.
This implementation's native TLS/ACL tests run over loopback in the default tier;
they do not prove host firewall, multicast, interface or clock behavior.

1. On two operator-controlled Windows/Linux/macOS hosts, record OS, Zenoh 1.10.1,
   OAC SHA, IPs, firewall policy, UTC offset and measured wall-clock skew. Synchronize
   clocks through the hosts' existing OS service; do not add OAC clock sync.
2. Authorize pairing before provisioning: create `PairingAuthority`, issue distinct
   opaque CNs with each host's IP SAN, and install each `IssuedCertificate` into a fresh
   owner-only directory. Provision through the approved operator/pairing path; never
   distribute the authority's private key or a host key to other peers. CLI pairing
   command wiring remains G9/G11; these transport APIs are the current seam.
3. Configure `PeerConfiguration::lan(listen, connect, files, allowed_subjects)` on both
   hosts, allow only the other's CN, use the same partition. With scouting off, explicit
   endpoints must carry core-signed HPKE frames in both directions and pass unchanged
   transport-contract checks through a two-host binding harness.
4. Repeat with `with_lan_multicast(true)` and no explicit remote endpoints. Capture
   native discovery and verify TLS-only links. Disable multicast at the network and
   repeat step 3 (G3 fallback); never silently downgrade to plaintext.
5. Negative controls: wrong CA/pin, absent client certificate, wrong IP SAN, and a
   CA-valid unlisted CN. The first three must fail native authentication; the last may
   open a native link but must carry no frames through the ACL. Try a plaintext
   discovered locator: native protocol allowlist must refuse it.
6. Record cross-host clock skew before/after a short-deadline core-sealed exchange.
   Delay receipt beyond the encrypted deadline and assert receiving-core discard
   (SEC-SEL-036). Record native arrival independently. No native TTL/sender-copy expiry
   claim: slow receiver skew can extend effective validity and fast skew reject early.
   Report observed skew/expiry against operational clock tolerance; sender retention
   remains risk row 77 for the lead's spec decision.
7. Replace the authenticated local relay on each host with another same-user process;
   the persisted pin must remain unchanged and native reconnect must resume delivery.
   Replacing it with an untrusted/plain holder must refuse traffic. Record holder gaps
   and fixed-port denial of service as residuals, not failed authentication.

Do not promote the G3 two-host leg or Gate S4 until the evidence is captured. No part
of this task runs that leg or any provider harness.
