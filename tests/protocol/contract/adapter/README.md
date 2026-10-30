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
- `tests/real_adapters.rs` runs the static scan against every workspace member under
  `adapters/`, found through `cargo metadata` rather than a fixed list (today
  `adapters/claude`, `adapters/codex` and `adapters/mcp-tools`). A planted case shows a new
  `adapters/acp` member is found and refused.
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
crate gives it (`claude::ClaudeHarness`, `codex::CodexHarness`, `codex::CodexFake`).

The suite's own checks bound what a harness can do. What they cannot see is a harness that
fabricates or filters what the fake observed. That residual is covered by reviewing the one
fixed file, and Gate S4 evidence for an adapter must cite it at the commit that ran.
That review covers everything `contract.rs` calls outside this suite, including the
adapter's own `src/` helpers. A helper that filters what the fake observed is part of the
harness. A hostile edit to an allowed dependency (`oac-core` or `oac-fake-claude`) that
re-exports an `include` macro, such as `pub use std::include as load`, is covered by review
like any other hostile edit to reviewed code. Plain `include_str!` in `oac-fake-claude`
stays allowed.

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
  hold no `self as` import, `macro_rules!`, `include!` (any form) or `#[path]`. They also
  hold no identifier `include`, `include_str` or `include_bytes` as a whole word, which
  refuses an aliased include (`use std::include as x;`), as `source.rs`'s word rule does
  for adapter `src/`. This applies to adapter `src/` files and to listed packages too. The harness
  file also holds no file module (`mod x;`) and no glob or renamed import, so it reads as
  one file.
- **What a listed package may do.** It may name `AdapterHarness` to drive the fakes'
  harnesses. It may not implement or rename it, rename the suite, glob the suite's items,
  reach `run`, or hold those shapes.
- **What an adapter may take as a dev-dependency.** Only the suite, `oac-core` and
  `oac-fake-claude` (`ADAPTER_DEV_DEPENDENCIES`). An identifier-pasting proc macro such as
  `paste` could spell the trait and `run` in pieces that no text rule sees.
- **Which files are read.** The file set comes from git: every tracked file, and every
  untracked file that is not ignored (`git ls-files -co --exclude-standard`), minus
  untracked files under `target/`. These fail: any `CACHEDIR.TAG` in that set (a committed
  tag would hide its directory), a symlink, and a tracked file under `target/`.
- **Where cargo's target directory may be.** It must be `<root>/target` or outside the
  repository. A committed `.cargo/config.toml` `target-dir`, or `CARGO_TARGET_DIR`,
  pointing anywhere else inside the repository fails, since it would take that directory
  out of the file set.
- **What cargo compiles for an adapter.** Git's file set misses a file that a committed
  `.gitignore` hides, so three further rules cover it:
  - no workspace member has a build script (no custom-build target, no `build.rs` file),
    since any member's could write into `adapters/`;
  - an adapter's lib target has `doctest = false`. Its `src/` files are also held to the
    hiding shapes, read from raw text, so a doctest in a doc comment is covered. Every
    file of an adapter, whatever its extension, is held to the name rule, since `include!`
    can load any file;
  - each test, example and bench target `cargo metadata` reports for it is
    `tests/contract.rs`, or a file the name rules pass;
  - no `.rs` file under `adapters/` is ignored by git.
- **What an adapter may depend on.** Its normal and build dependencies are the static
  scan's vetted list (`source::VETTED_DEPENDENCIES`), checked by `cargo metadata` for every
  adapter directory. This is a second check beside the one `tests/real_adapters.rs` makes,
  by identity, pin and features, over every member under `adapters/`.

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
- **a `[features]` table at all**: an adapter may declare no feature, since a non-monotonic
  cfg (`cfg(all(feature = "a", not(feature = "b")))`) compiles under no adapters-alone run
  while another member turning on `a` builds it in; and, as a second line, no entry may turn
  on an unvetted feature of a vetted crate (`tokio/net`, `rmcp/macros`) or name a forbidden
  crate;
- **a resolved dependency that is not the vetted identity**: the package `cargo metadata`
  resolves for each normal or build dependency must be from crates.io at its pin (or the
  repository's own directory), so a `[patch]` or `[replace]` cannot swap it; the
  workspace root manifest may hold no `[patch]` and no `[replace]` at all; every non-member
  package the adapter builds must be a crates.io registry package, with no path or git
  source anywhere in its closure (the repository's own `oac-core` excepted); and every
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
(G-7 §5). Since 2026-10-08 "CI" is the client-side `node scripts/local-ci.mjs` (no
GitHub-hosted CI): its `--self-test` refuses the same overrides in its steps, and its
summary lists any such variable inherited from the developer's environment.

No adapter needs any of the refused shapes today. If an adapter needs a macro, a module or
target outside cargo's default layout, or another dependency, that is a deliberate change
to these rules, not an exception to them.

An adapter's `tests/` directory is checked for the `TEST-PLANT` row only. Its tests may do
what adapter code may not, such as make a connection or define a test macro.

## Interfaces 0.4 connection factory revision (2026-10-10)

The lead authorized this spec-driven suite revision for #376. CoreSide::accept and
Claude's pipe helpers now take core-owned cancellable halves, replacing arbitrary
blocking Read/Write. There is no non-cancellable compatibility constructor. Every
existing check and planted breach remains; the pre-shutdown attachment snapshot stays
before the new probe, so attachment-closed reporting is still checked at the barrier.
The stand-ins close their supplied handles before reporting shutdown, including
Codex native-signal connections. Existing shutdown assertions use the event and request
snapshots up to the shutdown mark (revision 0.12 below), taken before the probe resumes the peer.

The added factory probe pauses an admitted write, times shutdown against the binding's
bound, checks every issued handle is closed at return, then resumes the peer and checks
its fake observations for late input. The planted WaitsBeforeClose and LateHandOff
adapters fail these checks; WorksAfterShutdown and KeepsAttachmentsAtShutdown retain
their original failures. No real harness runs or gate verdicts change.

Gate S4 suite baseline: `92a444d5e4bee86c3f8a782f5d9ab466966ca718` (2026-10-10).
The same pin is recorded in docs/planning/STATUS.md.
`adapters/claude/tests/contract.rs` remains unchanged.


## Interfaces 0.5/0.6 settlement revision (PR #380)

The requested spec-driven revision removes the post-return result fallback. Every
pending outcome must exist at the first shutdown return; event and request snapshots
must also remain unchanged after the stalled peer resumes. The 40 ms LateSettlement
plant returns a truthful failure too late and fails IFC-ADP-109. The shutdown timing
check uses replacement ID IFC-ADP-119. The suite sink also implements the additive
cancellation-aware request surface; all original assertions and planted breaches remain.
The real Claude adapter was separately mutated to release its call guard and delay its
cancellation outcome by 40 ms: both release profiles fail IFC-ADP-109 and IFC-ADP-070.
No other contract suite or Claude contract harness was changed. The suite baseline above
is the dedicated suite revision commit; implementation evidence uses its passing descendant.

## G8 slice S4: the Codex harness (#69, #403; lead-approved 2026-10-21, approval 4)

`codex::CodexHarness` (new) gives the Codex adapter the binding's two connections: the
carrier, a socket to the fake app-server that the daemon designates (`Designate`) before the
adapter is given it, and one shim connection per session, over which the harness speaks MCP
as Codex does. A session is paired the way Codex's is: a call the core side refuses with a
pairing value (`Scripted::RefusePairing`), reported to the carrier as an `mcpToolCall` item
(`oacFake/mcp/start`, `oacFake/mcp/complete`), and the harness waits for the adapter's reveal
(a `native-signal` on the carrier naming the thread, carrying the value). The suite runs
**unchanged** from `adapters/codex/tests/contract.rs`. Every edit to existing suite files,
none of which weakens a check:

- `src/codex.rs`: `PROFILE.makes_requests` is `true` (was `false`). Requests are now
  mandatory on the Codex binding, so `request` returning a `Gap` fails IFC-ADP-003,
  IFC-ADP-031 and IFC-ADP-060 for it, as for the channel binding. `CodexHarness`,
  `Designate`, `ADAPTER_CLIENT` and the harness types are added.
- `src/lib.rs`: `Scripted::RefusePairing` (a refusal `unauthorized` with a new pairing value
  of the form of MCPB-ATT-005, kept in `CoreSide::pairing_values`), `CoreSide::shared` (the
  core side as an `Arc`, for a step that is not given it), and the module documentation. No
  check changed.
- `tests/stand_in.rs`: the queue stand-in's harness, which returned `Gap::Unsupported` for a
  request, now passes the request to the stand-in's sink and renders the result as the
  binding does (it must, since requests are mandatory); its run reports IFC-ADP-003,
  IFC-ADP-031 and IFC-ADP-060 as passes, not not-applicable; the planted `RequestsAsGap` over
  the queue stand-in is now caught as those three rows (it was asserted to pass).


## Interfaces 0.12 settlement revision: judged by effects (#457)

Revision 0.12 of `spec/interfaces.md` (proposed; lead approval pending) judges settlement at
`shutdown` by the effects the core records, not by whether the pending `deliver` or `shutdown`
returned first. The earlier probe read its helper thread's result with `try_recv()` right after
`shutdown` returned. Under CPU load a conformant adapter read as having settled late, and the
order of two thread returns is not something a core can observe without a scheduler race. The
suite follows the new rule, with no sleep, retry or grace period before any verdict:

- `CoreSide` records, under one lock, every event report and request it receives, and the
  probe's `shutdown` thread marks the record right after `shutdown()` returns
  (`mark_shutdown_returned`). `IFC-ADP-109` row `no-effect-after-shutdown-return` fails if the
  record shows any effect after the mark, and `IFC-ADP-070` row
  `stalled-write-has-no-late-hand-off` fails on the same record. The record is read twice: after
  the 50 ms exposure window that follows the resume, and again after the 200 ms wait of the
  last stage; `IFC-ADP-109` and `IFC-ADP-110` row `no-late-effect-after-the-exposure-window`
  fail on any entry in the second read, so a report that comes later than the first read (the
  `ReportsOnceLate` plant, about 100 ms) is still caught. The exposure has an upper limit: the
  record is read a second time and not again, and the measured reach is roughly 300 to 600 ms
  after the return (a report at 300 ms was caught, one at 600 ms was not). A report later than
  that is not seen.
- Hand-offs are not in that record. They are judged by the closure of the connection, as the
  spec paragraph after `IFC-ADP-111` says: the core's closure barrier
  (`core/src/connection.rs`, `ConnectionWriter::transfer`) admits no transfer once the
  connection is closed, `IFC-ADP-073` requires every supplied connection to be closed before
  `shutdown` returns (checked at the instant it returns), and the peer is paused until after the
  mark, so stalled input that reaches it once it is resumed is a hand-off after the return
  (`IFC-ADP-070`).
- The pending `deliver` must return within the composed bound of the shutdown return while the
  peer is still paused: the suite waits for its result for `shutdown_bound_ms` (the core and the
  suite's absent transport add nothing, §5.3.2), then resumes the peer. `IFC-ADP-126` row
  `stalled-deliver-returns-within-composed-bound` fails if it has not, whether it ended only
  after the bound or waits on something the core must do. A `deliver` still returning when
  `shutdown` returns, with no effect, is conformant. That wait is judged at the spec's own bound
  scale (1000 ms for the stand-ins): a result a thread hop or a scheduling quantum late is on
  time, and only a result that overruns the bound fails. The tight comparison of two
  sub-millisecond events is what this revision removes.
- Connections are judged closed at the instant `shutdown` returned (`IFC-ADP-073`), and
  `IFC-ADP-071` uses the events up to the mark. The 50 ms wait after the resume is an exposure
  window for a detached writer, not a settlement allowance: no verdict about the pending
  `deliver` depends on it.

Planted breaches: `LateSettlement` is now a `deliver` that ends only after the composed bound
(900 ms against a 300 ms bound; the earlier 40 ms late failed result, with no effect, is
conformant and is the `SlowTailNoEffect` case, which must pass), caught as `IFC-ADP-126`.
`ReportsAfterShutdown` is a detached reporter that keeps reporting after the return, caught as
`IFC-ADP-109` and `IFC-ADP-110` (the second read of the record always sees it; the first read sees it
only if the reporter has run by then); `ReportsOnceLate` reports once about 100 ms after it, caught as
`IFC-ADP-109` and `IFC-ADP-110`. `LateHandOff` and `WorksAfterShutdown` (a hand-off that is still
pending, or works, after the return) stay planted and fail `IFC-ADP-070` and `IFC-ADP-126`.

The well-behaved channel stand-in's `shutdown` waits for its running `deliver` calls after
closing its connections, bounded by its own `shutdown_bound_ms`, as the real Claude adapter
waits for its call lock; before, its `deliver` could end after `shutdown` had returned under
load.

Residual window. The core notes an effect and the shutdown thread sets the mark, each under the
same lock, so there is no scheduler race on which came first. The mark is taken by the thread
that called `shutdown`, at or after the return, so an effect that falls between the return and
the mark counts as before it. That can only make the suite pass an adapter that reported in
that gap, never fail a conformant one. The gap runs from `shutdown()` returning, through the
`is_closed` scan of the supplied connections, to the mark. It is short, but the suite does not
bound it. The other limit is the exposure above: effects later than the second read are not seen.
