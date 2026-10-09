# Tooling & credits

Thank you to the upstream maintainers whose projects support OAC's research.
Icons below are navigation symbols, not official vendor logos.

## 🛠️ herdr — development and test automation

[herdr](https://github.com/herdrdev/herdr), maintained by
[herdrdev](https://github.com/herdrdev), is the terminal multiplexer used by
OAC's scripted-run driver to exercise real harness CLIs during development and testing.

**Role:** external dev/test tooling. It is not an OAC runtime dependency, adapter,
or supported integration surface. Product modules must not import, vendor, or invoke
the driver. Screen interaction here is test instrumentation; supported OAC delivery
must use documented harness interfaces.

- [OAC driver and scenarios](https://github.com/RossGraeber/OAC/blob/main/tools/herdr/README.md)
- [Evaluation and license evidence](https://github.com/RossGraeber/OAC/blob/main/docs/planning/decisions/K1-herdr-evaluation.md)
- [Current tooling pin](https://github.com/RossGraeber/OAC/blob/main/docs/planning/PINS.md)
- [Upstream license](https://github.com/herdrdev/herdr/blob/v0.9.1/LICENSE)

License evidence: Apache-2.0. Source:
https://github.com/herdrdev/herdr/blob/v0.9.1/LICENSE, tag `v0.9.1`,
retrieved 2026-09-28 (recorded in OAC's K1 evaluation §3).

## 🔌 Harness and protocol projects

| Project / maintainers | Role in OAC's plan | Details |
|---|---|---|
| [Claude Code / Anthropic](https://github.com/anthropics/claude-code) | Claude harness integration research | [Capability matrix](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/01-capability-matrix.md) |
| [Codex / OpenAI](https://github.com/openai/codex) | Codex harness integration research | [G2 evidence](https://github.com/RossGraeber/OAC/blob/main/docs/planning/gates/G2-result.md) |
| [Model Context Protocol](https://github.com/modelcontextprotocol) | Protocol foundation for the planned OAC Session Channels extension | [Interface plan](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/05-interfaces.md) |
| [Eclipse Zenoh](https://github.com/eclipse-zenoh/zenoh) | Planned first reference transport | [G3 evidence](https://github.com/RossGraeber/OAC/blob/main/docs/planning/gates/G3-result.md) |
| [Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk) | SDK evaluated for the reference implementation | [Language/runtime decision](https://github.com/RossGraeber/OAC/blob/main/docs/planning/decisions/C1-language-runtime.md) |

These credits do not mean every listed project is shipped, every integration is
complete, or an upstream project endorses OAC. The linked records carry surface
labels, version evidence, limitations, and dependency license details.

## 🧠 Beacon — optional external service

[Beacon / Asymptote Labs](https://github.com/Asymptote-Labs/agent-beacon) is documented
as an independently installed memory service connected to each harness.
It runs beside OAC: OAC does not launch, configure, proxy, or call it, or store,
fetch, summarize, or inject its memory.

[Decision and findings](https://github.com/RossGraeber/OAC/blob/main/docs/planning/decisions/L1-beacon-memory.md) ·
[Operational notes](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/08-cli-and-deployment.md)

License evidence: MIT. Source:
https://github.com/Asymptote-Labs/agent-beacon/blob/v1.3.29/LICENSE, tag `v1.3.29`,
retrieved 2026-09-29 (recorded in OAC's L1 decision §3).

## 📜 License inventory

OAC's own license is [Apache-2.0](https://github.com/RossGraeber/OAC/blob/main/LICENSE).
The [repository and dependency inventory](https://github.com/RossGraeber/OAC/blob/main/docs/planning/v0.1/07-repository-and-dependencies.md)
is the detailed record for third-party dependency roles and licenses.
Upstream names and trademarks remain with their owners.
