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

**Pre-release, in build.** The repository holds the planning package, decisions, gate
evidence, development tooling, and a Rust workspace (`oac-cli`, `oac-core`, the provider
adapters, the transports) that is built stage by stage. The
[status ledger](docs/planning/STATUS.md#current-stage) is authoritative: Stage 4 (adapters
and the Zenoh transport) is open, and gates G1 to G5 have PASS verdicts. No release has been
published, so the installation and launch commands in planning documents are targets.

**What works on `main` today** (checked against the code and the merged pull requests):

- `oac hook` (`cli/src/hook.rs`, PR #405) reads a Claude Code `SessionStart` input and carries
  it as one line, as `spec/bindings/mcp.md` §8.1 requires. The binary has no connector to a
  daemon yet (`cli/src/main.rs`), so today the input is dropped with a diagnostic and the
  session stays unbound.
- A device key store (`cli/src/keystore.rs`, PR #407) that uses the operating system's
  credential store, or an encrypted file where none exists.
- Peer observation (`cli/src/peer/`, PR #410): which harness process a local connection
  belongs to, read from the operating system on Windows and Linux. macOS is written, not built.
- Local IPC (`cli/src/ipc/`, PR #411): a per-user named pipe on Windows or a per-user socket on
  Unix, with operating-system peer authentication and a one-daemon lock.

Not on `main` yet: the daemon, `oac start` and `oac mcp-shim` (PR #413, open), and interfaces
0.9 (PR #408, open).

**Build and test.** `rust-toolchain.toml` pins Rust 1.98.1.

```sh
cargo build -p oac-cli       # builds the oac binary
node scripts/local-ci.mjs    # the default check tier; run it on Windows and in WSL
```

There is no GitHub-hosted CI. Before a pull request is opened or merged, run the default
tier on Windows and in WSL, and paste each summary (it names the git HEAD) into the pull
request. The opt-in tiers are `keystore`, `scale`, `mutation`, `lan` and `g3-macos`
(see [the test strategy](docs/planning/v0.1/09-test-strategy.md) §3).

Start with the [getting started guide](https://github.com/RossGraeber/OAC/wiki/Getting-Started) to explore
the evidence and contribute.

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

**Authenticated messages remain untrusted content.** Delivery permission does not
grant tool approval or active-turn control. Permission relay is off by default in
the v0.1 plan, and machine-set provenance must stay separate from message text.
The Codex provenance framing passed its re-run on 2026-10-02 (G5, C13); that run tests the
gate client, not OAC's own adapter. Local IPC authenticates the peer's user. Three same-user
residuals of local IPC and peer pairing were accepted by the lead on 2026-10-15; they are
listed in [security plan §12](docs/planning/v0.1/06-security.md). Per-recipient payload
sealing is specified in `spec/security.md` §14 (ADR-001-A4); full end-to-end encryption
beyond it is deferred. Replay suppression has a restart gap.
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
`--tier keystore|scale|mutation|g3-macos`; see
[the test strategy](docs/planning/v0.1/09-test-strategy.md) §3.

## Credits and license

Thanks to the projects making this research possible, especially
[herdr](https://github.com/herdrdev/herdr) for development and test automation.
[Tooling & credits](https://github.com/RossGraeber/OAC/wiki/Tooling-and-Credits) explains each project's role;
[the dependency inventory](docs/planning/v0.1/07-repository-and-dependencies.md)
records the detailed license evidence.

OAC is licensed under [Apache-2.0](LICENSE). Third-party projects retain their own
licenses and trademarks. Project links acknowledge their work and do not imply endorsement.
