# tests/protocol/contract/adapter/

The adapter contract suite (#59, F10). It is one suite, written against
`dyn oac_core::adapter::ProviderAdapter`, that every adapter runs unchanged against the fake
harnesses. It is test-only: an adapter takes it as a dev-dependency, and runs it from its own
`tests/contract.rs` (see "Where a harness lives").

- `src/lib.rs` holds the suite and its report.
- `src/claude.rs` and `src/codex.rs` hold the harnesses over the fake Claude Code endpoint
  and the fake Codex app-server.
- `src/source.rs` holds the static source scan (see below).
- `src/plant.rs` holds the planted breaches that `tests/stand_in.rs` uses. Adapter code
  never reaches them: a path into `plant` is a finding of its own row, `TEST-PLANT`.
- `tests/stand_in.rs` runs the suite against well-behaved stand-in adapters and against
  each planted breach, an adapter's or a harness's.
- `tests/real_adapters.rs` runs the static scan against `adapters/claude` and
  `adapters/codex`.
- `tests/path_modules.rs` runs the scan on scratch crates on disk.
- `tests/harness_location.rs` checks where a harness may live.

## What a harness may not change

A harness is written by the adapter's task, so the suite does not take its word for the
binding (#351). It identifies the binding from the hand-off calls the fake recorded, and
runs every check under its own profile for that binding (`BINDINGS`). The following all
fail:

- a harness profile that differs;
- a `Gap` from a step that the binding's profile makes mandatory;
- a `start_turn` after which a busy message becomes input before the drain;
- an empty list of source files or of native ids.

The module doc of `src/lib.rs` has the table of which steps are mandatory for which
binding, and why the rest are honestly not applicable.

## Where a harness lives

The dependency direction is the documented one (07 §3, `scripts/check-crate-deps.mjs` rule 5,
this crate's `Cargo.toml`): an adapter takes this suite as a dev-dependency, and the suite
never depends on an adapter. So the harness a real adapter runs under lives in that adapter,
at exactly one fixed path: **`adapters/<name>/tests/contract.rs`**. It may use what this
crate gives it (`claude::ClaudeHarness`, `codex::CodexFake`).

The suite's own checks bound what a harness can do. What they cannot see is a harness that
fabricates or filters what the fake observed. That residual is covered by reviewing the one
fixed file, and Gate S4 evidence for an adapter must cite it at the commit that ran.
That review covers everything `contract.rs` calls outside this suite, including the
adapter's own `src/` helpers. A helper that filters what the fake observed is part of the
harness.

`tests/harness_location.rs` keeps this mechanical:

- **Who may depend on the suite.** Only an adapter, `adapters/mcp-tools` (#352 lets it
  dev-depend on a suite), and the packages in `ALLOWED_DEPENDENTS` may depend on
  `oac-contract-adapter`. Today that list names only `oac-transport-memory`, whose pipeline
  test drives the fakes. An adapter or the tool crate depends on the suite as a
  dev-dependency only. No dependent renames it (`package = ..`).
- **Which files may touch the harness.** In an adapter, only `tests/contract.rs` names
  `AdapterHarness` or `oac_contract_adapter`, or reaches `run`. No other file outside this
  crate and the listed packages names either.
- **Shapes that could hide a harness.** The harness file and an adapter's other test files
  hold no `self as` import, `macro_rules!`, `include!` (any form) or `#[path]`. The harness
  file also holds no file module (`mod x;`) and no glob or renamed import, so it reads as
  one file.
- **What a listed package may do.** It may name `AdapterHarness` to drive the fakes'
  harnesses. It may not implement or rename it, rename the suite, glob the suite's items,
  reach `run`, or hold those shapes.
- **What an adapter may take as a dev-dependency.** Only the suite, `oac-core` and
  `oac-fake-claude` (`ADAPTER_DEV_DEPENDENCIES`). An identifier-pasting proc macro such as
  `paste` could spell the trait and `run` in pieces that no text rule sees.
- **Which files are read.** The file set comes from git: `git ls-files -co
  --exclude-standard`, minus cargo's `target_directory` from `cargo metadata`. Any
  `CACHEDIR.TAG` left in that set fails, since a committed tag would hide its directory. A
  symlink fails too.

These are text rules. What really bounds a listed transport is
`scripts/check-crate-deps.mjs`: a transport can never reach an adapter, so a harness hidden
there could only drive a stand-in.

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
- **a normal or build dependency that is not vetted**: `VETTED_DEPENDENCIES` lists them,
  `oac-core` only today, and each one is checked to export no macro and to be no
  proc-macro. A dependency is vetted by identity, not by name, since any crate can call
  itself `oac-core`. It must be a path dependency with no `source` (no registry, no git),
  must be depended on under its own name (no `package = ..` rename), and its directory
  must canonicalize to the repository's own `core/`;
- **a symlink** on the way to a module file, or among the files scanned;
- a file that cannot be read or does not parse.

Identifiers are compared without a raw `r#` prefix everywhere.

**The cost of the word rule.** A binding named `path` may be handed to a macro
(`format!("{}", path)`). A binding named `include`, `include_str` or `include_bytes` may
not, so rename it, or use the inline form (`format!("{include}")`), which is a string
literal.

**What stays out of reach.** A macro from another crate, or a derive or attribute
proc-macro, can load a file from a bare literal (`dep::load!("../x.rs")`). No static scan
sees that. This is why the dependency list is vetted: a new dependency is reviewed for the
macros it exports before it is added.

No adapter needs any of the refused shapes today. If an adapter needs a macro, a module or
target outside cargo's default layout, or another dependency, that is a deliberate change
to these rules, not an exception to them.

An adapter's `tests/` directory is checked for the `TEST-PLANT` row only. Its tests may do
what adapter code may not, such as make a connection or define a test macro.
