# tests/protocol/contract/adapter/

The adapter contract suite (#59, F10). It is one suite, written against
`dyn oac_core::adapter::ProviderAdapter`, that every adapter runs unchanged against the fake
harnesses. It is test-only: an adapter takes it as a dev-dependency.

- `src/lib.rs` holds the suite and its report.
- `src/claude.rs` and `src/codex.rs` hold the harnesses over the fake Claude Code endpoint
  and the fake Codex app-server.
- `src/source.rs` holds the static source scan (see below).
- `src/plant.rs` holds the planted breaches that `tests/stand_in.rs` uses. Adapter code
  never reaches them: a path into `plant` is a finding of its own row, `TEST-PLANT`.
- `tests/stand_in.rs` runs the suite against well-behaved stand-in adapters and against
  each planted breach.
- `tests/real_adapters.rs` runs the static scan against every workspace member under
  `adapters/`, found through `cargo metadata` rather than a fixed list (today
  `adapters/claude`, `adapters/codex` and `adapters/mcp-tools`). A planted case shows a new
  `adapters/acp` member is found and refused.
- `tests/path_modules.rs` runs the scan on scratch crates on disk.

## What an adapter's sources may not hold

The scan in `src/source.rs` reads what cargo compiles for an adapter, taken from
`cargo metadata`: every target's `src_path`, the build script included, and everything
under `src/` besides. It follows every file their `mod` declarations load. It resolves every
path through the imports and checks each one against the core items an adapter must not
reach ([IFC-ADP-001], [IFC-ADP-002], [IFC-ADP-007], [IFC-ADP-013]).

Some source shapes cannot be read statically, so the scan refuses them outright. Each one
is a finding under every row:

- **a `macro_rules!` definition**, anywhere (and an unparsed `macro` item);
- **a `#[path]` attribute**, anywhere, under any spelling: `#[r#path]`, or `path = ..`
  inside a `cfg_attr` at any depth;
- **`include!`, `include_str!` and `include_bytes!`**, under any path or alias;
- **the words `mod`, `include`, `include_str` and `include_bytes`** as tokens in a macro
  invocation's arguments or an attribute's list, and **`path`** there when it is written as
  `path = ..` or `#[path ..]`;
- **a manifest key that moves a target or switches discovery**: `path` or `build` outside a
  dependency table, and `autolib`, `autobins`, `autoexamples`, `autotests` or
  `autobenches`;
- **a normal or build dependency that is not vetted**: `VETTED_DEPENDENCIES` lists them
  (#7, `docs/planning/decisions/G-7-stage4-dependencies.md` §5). A dependency is vetted by
  identity, not by name, since any crate can call itself `oac-core`, and is depended on
  under its own name (no `package = ..` rename):
  - `oac-core` and `oac-mcp-tools`, the repository's own crates: path dependencies with no
    `source` (no registry, no git), whose directories canonicalize to `core/` and
    `adapters/mcp-tools/`. Each is checked to export no macro and to be no proc-macro.
  - `rmcp` `=3.4.0` (default features off; `server` and `transport-async-rw` only, so its
    `macros` feature is refused) and `tokio` `=1.53.2` (only the features `rmcp` enables on
    it: `sync`, `macros`, `rt`, `time`, `io-util`), from crates.io at exactly those
    requirements. Once either is in the workspace graph, `tests/real_adapters.rs` checks it
    is at its pin and is not itself a proc-macro, and that none of its own `macro_rules!`
    bodies can load a file. It does not vet the proc-macros reached through their features
    (`tokio-macros`, `schemars_derive`, `serde_derive`); G-7 §5 records those as read by
    hand, and the test lists them;
- **an entry of the package's own `[features]` table** that turns on an unvetted feature of a
  vetted crate (`tokio/net`, `rmcp/macros`) or names a forbidden crate;
- **a resolved dependency that is not the vetted identity**: the package `cargo metadata`
  resolves for each normal or build dependency must be from crates.io at its pin (or the
  repository's own directory), so a `[patch]` or `[replace]` cannot swap it; the
  workspace root manifest may not patch or replace a vetted or forbidden crate; and every
  registry package in the resolved closure must sit under `$CARGO_HOME/registry/src`
  (`~/.cargo` when `CARGO_HOME` is unset), so a `[source]` replacement in a cargo
  configuration file cannot swap in a vendored, edited copy that still reports crates.io;
- **a forbidden crate under any dependency kind, dev included, or anywhere in the resolved
  graph**: the whole `codex-` family (`FORBIDDEN_FAMILIES`), a crate named exactly `codex`,
  and `rmcp-macros` (`FORBIDDEN_NAMES`) ([ADR-001 Boundary]; G-7 §2). `scripts/check-crate-deps.mjs` rule 6
  refuses the same anywhere in the workspace graph, transitively, and a test checks the two
  lists agree;
- **a symlink** on the way to a module file, or among the files scanned;
- a file that cannot be read or does not parse.

Identifiers are compared without a raw `r#` prefix everywhere.

What these per-package checks cannot see is feature unification: another workspace member
turning on a feature (`tokio/net`) that an adapter then uses, directly or behind an adapter
feature that is off by default and that the other member turns on. `scripts/check-crate-deps.mjs
--adapters-alone`, run in CI, builds the adapters and `adapters/mcp-tools` alone four times,
with default features and with `--all-features`, each in the dev profile and in `--release`
(so code under `cfg(not(debug_assertions))` is compiled too), and all of these fail there
(G-7 §5). The same script's rule 7 refuses a tracked `.cargo/config` or `.cargo/config.toml`
that names `source`, `patch` or `paths`, and its rule 8 refuses a path package inside the
workspace root that is not a member, and a root `[workspace] exclude` that reaches
`adapters/`, so an excluded crate cannot be built in unchecked.

**The cost of the word rule.** A binding named `path` may be handed to a macro
(`format!("{}", path)`). A binding named `include`, `include_str` or `include_bytes` may
not, so rename it, or use the inline form (`format!("{include}")`), which is a string
literal.

**What stays out of reach.** A macro from another crate, or a derive or attribute
proc-macro, can load a file from a bare literal (`dep::load!("../x.rs")`). No static scan
sees that. This is why the dependency list is vetted: a new dependency is reviewed for the
macros it exports before it is added. `adapters/mcp-tools`, which the adapters may depend
on, is scanned under the same rules as an adapter.

The dependency checks trust CI's cargo command lines and environment. Three gaps stay open,
and each needs a reviewed workflow change: a cargo `--config` flag (the outer cargo's, which
this suite's inner `cargo metadata` never sees), a `CARGO_HOME` that CI points somewhere
else, and targets other than the three CI operating systems. `scripts/check-workflows.mjs`
W6 refuses `--config`, `CARGO_HOME` and `CARGO_SOURCE_*` / `CARGO_PATCH*` in workflows
(G-7 §5).

No adapter needs any of the refused shapes today. If an adapter needs a macro, a module or
target outside cargo's default layout, or another dependency, that is a deliberate change
to these rules, not an exception to them.

An adapter's `tests/` directory is checked for the `TEST-PLANT` row only. Its tests may do
what adapter code may not, such as make a connection or define a test macro.
