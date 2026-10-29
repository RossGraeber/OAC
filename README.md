<p align="center">
  <img src="docs/assets/oac-test-card.svg" width="520" alt="OAC — Open Agent Channels, a colored character-art TV test card">
</p>

<p align="center"><strong>Messages between live AI harness sessions.</strong></p>
<p align="center">
  <a href="https://github.com/RossGraeber/OAC/wiki">Wiki</a> ·
  <a href="docs/planning/STATUS.md">Project status</a> ·
  <a href="docs/planning/DESIGN.md">Design</a> ·
  <a href="https://github.com/RossGraeber/OAC/issues">Issues</a> ·
  <a href="https://github.com/RossGraeber/OAC/wiki/Tooling-and-Credits">Tooling &amp; credits</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue" alt="Apache-2.0 license"></a>
  <a href="docs/planning/STATUS.md"><img src="https://img.shields.io/badge/status-Stage%204%20open-yellow" alt="Stage 4 open: adapters and transport"></a>
</p>

## What is OAC?

**Open Agent Channels (OAC)** is a project to define provider-neutral **OAC Session
Channels**: direct, two-way messages between sessions of existing AI harnesses.
Each harness keeps responsibility for its own inference, authentication, context,
tools, and policy.

The intended flow is simple: an OAC-enabled Claude Code session sends a message to
an OAC-enabled Codex session; the receiving harness gets active input and can reply.
The [architecture and boundaries](docs/planning/ADR-001.md) define that goal.
Sessions must be launched OAC-enabled; arbitrary running sessions are outside the
supported integration model.

## Where the project stands

**Pre-release, in build.** The repository holds the planning package, decisions, gate evidence, development tooling, and a Rust workspace (`oac-cli`, `oac-core`, the provider adapters, the transports) built stage by stage. The [status ledger](docs/planning/STATUS.md#current-stage) is authoritative: Stage 4 (adapters and the Zenoh transport) is open, and gates G1 to G5 have PASS verdicts. No release has been published, so installation and launch commands in planning documents are targets.

**What works on `main` today** (checked against the code and the merged pull requests):

- `oac start` (`cli/src/commands.rs`, PR #413) runs the daemon in the foreground, or with `--detach` as a detached process, on the real Zenoh peer transport. Since PR #425 it serves on a multi-thread Tokio runtime (`commands::daemon_runtime()`); the reference transport refuses a current-thread one. Its log lines are also appended to a file that survives a crash, and `oac start --help` prints the path (G10 slice 2, PR #442).
- `oac stop` (PR #440), `oac status` (PR #444), `oac sessions` (PR #446) and `oac doctor` (PR #447) are G10's verbs. Since G10 slice 7 (lead decision 2026-10-20) every verb takes `--json`: `oac start` prints one object once the daemon serves, `oac mcp-shim` and `oac hook` send only their diagnostics to standard error as JSON lines. `oac stop` asks a detached daemon to stop cleanly, which a detached daemon on Windows could not do before.
- `oac mcp-shim --harness <claude|codex>` (PR #413) is the stdio server a harness starts: a byte relay to that harness's endpoint on the daemon. With no daemon running, it says to run `oac start` and does not start one.
- `oac hook` (`cli/src/hook.rs`, PR #405, connected to the daemon by PR #413) carries a Claude Code `SessionStart` input to the daemon as one line, as `spec/bindings/mcp.md` section 8.1 requires. Its connection stays open for at most 3 seconds, so the daemon can look up the peer. If the daemon is not running, the input is dropped with a diagnostic and the session stays unbound (fail closed).
- A device key store (`cli/src/keystore.rs`, PRs #315 and #407) that uses the operating system's credential store, or an encrypted file where none exists.
- Peer observation (`cli/src/peer/`, PR #410): which harness process a local connection belongs to, read from the operating system on Windows and Linux. macOS is written, not built.
- Local IPC (`cli/src/ipc/`, PR #411): a per-user named pipe on Windows or a per-user socket on Unix, with operating-system peer authentication and a one-daemon lock.
- Interfaces 0.11 (PRs #408, #424 and #427), security 0.7 (PR #439; 0.6 from PR #427) and session-channels 0.4 (PR #427), and the MCP binding 0.5 (PR #454; 0.4 from PR #424), are in force: their spec headers read "in force". Interfaces 0.12 (settlement at shutdown judged by the effects the core records, Refs #457) is proposed; its header reads "proposed; lead approval pending" until the lead merges it.

**Not on `main` yet, or not run:**

- The pairing verbs (#437) and a verb that reads the daemon log.
- The live check of the G10 verbs with a real Claude Code session ran on Windows only (2026-10-20, print mode, one device); on WSL only `oac doctor` ran, because no device key exists there yet. See the status ledger. The daemon test runs on Windows and Linux.
- The Codex endpoint: there is no Codex adapter until G7 and G8.

## Using the CLI

The `oac` binary (build it with `cargo build -p oac-cli`; nothing is released or installed yet) needs no
config file. The verbs (full detail: [CLI and deployment](docs/planning/v0.1/08-cli-and-deployment.md) section 5):

```sh
oac start [--detach] [--json]   # run the per-device daemon; --detach leaves it in the background
oac status [--json]    # is it running: pid, version, uptime, device id, transport and adapter health
oac sessions [--json]  # this device's own sessions (session id, harness, online or withheld)
oac doctor [--json]    # preflight checks: PASS, WARN, FAIL or SKIP, one line each
oac stop [--json]      # ask the daemon to stop cleanly (never kills; waits about 15 s)
```

- `oac start` is idempotent: with a daemon already up it succeeds and does nothing. Its log lines go to standard error and to a log file that survives a crash; `oac start --help` prints the path.
- `--json` prints one object with `"oac_cli":1` on standard output; diagnostics go to standard error. `oac start --json` prints its one object once the daemon serves (`ok`, `already_running`, `detached`, `pid`, `log`), or an error object if the start fails.
- Exit codes: `0` ok (`oac stop` also when no daemon was running), `1` failed or timed out, `2` usage, `3` the daemon is not running (`status`, `sessions`), `4` the daemon is unreachable or incompatible. `oac doctor` exits `1` only when a check fails.
- `oac mcp-shim [--json]` and `oac hook [--json]` are started by a harness, not typed. Their standard output is the harness's (the MCP stream; the hook's context), so `--json` changes only the form of their diagnostics on standard error: one JSON object per line. The hook still writes nothing to standard output and exits `0`.
- The first `oac start` creates the device key in the operating system's credential store, or in an encrypted file where none exists.

**Build and test.** `rust-toolchain.toml` pins Rust 1.98.1.

```sh
cargo build -p oac-cli       # builds the oac binary
node scripts/local-ci.mjs    # the default check tier; run it on Windows and in WSL
```

There is no GitHub-hosted CI. Before a pull request is opened or merged, run the default tier on Windows and in WSL, and paste each summary (it names the git HEAD) into the pull request. The opt-in tiers are `keystore`, `scale`, `mutation`, `lan` and `g3-macos` (see [the test strategy](docs/planning/v0.1/09-test-strategy.md) section 3).

## Designed around

- **Active delivery:** push messages into OAC-enabled sessions, with replies and correlation.
- **Harness independence:** provider adapters behind a neutral messaging contract.
- **Transport independence:** Zenoh is the planned first reference transport.
- **Explicit trust:** authenticated sender identity, authorization, and untrusted message content.
- **Local CLI use:** the design does not require a cloud account or separately administered server for normal local operation.

See the [design](docs/planning/DESIGN.md), [security plan](docs/planning/v0.1/06-security.md),
and [deferred scope](docs/planning/v0.1/12-deferred.md) for the details.

## Communication

The planned message flow is harness → adapter → local daemon → transport → receiving
daemon → adapter → harness. Sessions on one device share the daemon. Messages target
opaque session IDs and carry text, reply correlation, freshness, and signed security
fields. Active delivery uses native harness input rather than inbox polling.

Receipts distinguish adapter acceptance from being handed to the harness. Neither
proves that the model read the message or completed a task. Exactly-once delivery and
durable offline mailboxes are not promised.
Read [communication details](https://github.com/RossGraeber/OAC/wiki/Communication)
for addressing, discovery, replies, failures, and deployment.

## Security

The design combines device-key signatures, replay checks, authenticated local IPC,
encrypted transport, and default-deny session allowlists scoped by working directory.
Pairing trusts a device key; separate grants authorize particular sessions.
Each harness keeps its own provider credentials.

**Authenticated messages remain untrusted content.** Delivery permission does not grant tool approval or active-turn control. Permission relay is off by default in the v0.1 plan, and machine-set provenance must stay separate from message text. The Codex provenance framing passed its re-run on 2026-10-02 (G5, C13); that run tests the gate client, not OAC's own adapter. Local IPC authenticates the peer's user. Three same-user residuals of local IPC and peer pairing were accepted by the lead on 2026-10-15; they are listed in [security plan section 12](docs/planning/v0.1/06-security.md). Per-recipient payload sealing is specified in `spec/security.md` section 14 (ADR-001-A4); full end-to-end encryption beyond it is deferred. Replay suppression has a restart gap.
Read [security details](https://github.com/RossGraeber/OAC/wiki/Security) for trust
boundaries, controls, current evidence, and residual risks.

## Explore the docs

| Start here | What you will find |
|---|---|
| [📖 Wiki](https://github.com/RossGraeber/OAC/wiki) | Overview and guided navigation |
| [🧭 Getting started](https://github.com/RossGraeber/OAC/wiki/Getting-Started) | Repository orientation and contributor checks |
| [🏗️ Architecture](https://github.com/RossGraeber/OAC/wiki/Architecture) | Layers, scope, and trust boundaries |
| [🧪 Gate evidence](docs/planning/gates/README.md) | Recorded provider and transport experiments |
| [🛠️ Tooling & credits](https://github.com/RossGraeber/OAC/wiki/Tooling-and-Credits) | herdr, upstream projects, licenses, and their roles |
| [📺 Brand assets](docs/assets/README.md) | Character-art logo for documentation and terminals |

## Contribute

Pick an [issue](https://github.com/RossGraeber/OAC/issues), read its **Skills:** line, and check the
[current stage](docs/planning/STATUS.md) before starting implementation.
[AGENTS.md](AGENTS.md) explains repository guidance; [ADR-001](docs/planning/ADR-001.md)
sets the architectural boundaries. Include evidence for provider behavior and keep
unverified findings explicit.

There is no GitHub-hosted CI. Before opening or merging a pull request, run
`node scripts/local-ci.mjs` on Windows and in WSL and paste each summary (it names the
git HEAD SHA) into the pull request. Opt-in tiers run on demand with
`--tier keystore|scale|mutation|lan|g3-macos`; see
[the test strategy](docs/planning/v0.1/09-test-strategy.md) §3.

## Credits and license

Thanks to the projects making this research possible, especially
[herdr](https://github.com/herdrdev/herdr) for development and test automation.
[Tooling & credits](https://github.com/RossGraeber/OAC/wiki/Tooling-and-Credits) explains each project's role;
[the dependency inventory](docs/planning/v0.1/07-repository-and-dependencies.md)
records the detailed license evidence.

OAC is licensed under [Apache-2.0](LICENSE). Third-party projects retain their own
licenses and trademarks. Project links acknowledge their work and do not imply endorsement.
