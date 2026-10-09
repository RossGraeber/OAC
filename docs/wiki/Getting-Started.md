# Getting started

OAC is in its provider and transport spike stage. There is no released OAC CLI to
install from this repository yet. Commands in the CLI planning document describe
the intended interface.

## Explore locally

```sh
git clone https://github.com/RossGraeber/OAC.git
cd OAC
```

Read these in order:

1. [README](https://github.com/RossGraeber/OAC/blob/main/README.md) for the project overview.
2. [Status](https://github.com/RossGraeber/OAC/blob/main/docs/planning/STATUS.md) for the current stage and open findings.
3. [ADR-001](https://github.com/RossGraeber/OAC/blob/main/docs/planning/ADR-001.md) and its [amendments](https://github.com/RossGraeber/OAC/blob/main/docs/planning/ADR-001-AMENDMENTS.md) for scope and boundaries.
4. [Design](https://github.com/RossGraeber/OAC/blob/main/docs/planning/DESIGN.md) and [gate evidence](https://github.com/RossGraeber/OAC/blob/main/docs/planning/gates/README.md) for the proposed architecture and observed results.

## Contribute

Choose an [issue](https://github.com/RossGraeber/OAC/issues) that belongs to an open stage.
Follow its **Skills:** line and [AGENTS.md](https://github.com/RossGraeber/OAC/blob/main/AGENTS.md).
The local skills are the maintained instructions for the work.

With Node.js available, repository checks include:

```sh
node scripts/check-skills.mjs
node scripts/check-herdr-containment.mjs
```

Run the checks appropriate to your changes. Gate fixtures also have
`node scripts/check-fixture-manifest.mjs`; their capture and review procedure lives
in the [gate guide](https://github.com/RossGraeber/OAC/blob/main/docs/planning/gates/README.md).

## Development tooling

[herdr](https://github.com/herdrdev/herdr) is used for scripted development and
test runs. Read the [OAC driver guide](https://github.com/RossGraeber/OAC/blob/main/tools/herdr/README.md) before running
live scenarios: those need external tooling and logged-in harnesses. The driver's
self-test uses test doubles:

```sh
node tools/herdr/run.mjs --self-test
```

See [Tooling & credits](Tooling-and-Credits) for roles and license records.
