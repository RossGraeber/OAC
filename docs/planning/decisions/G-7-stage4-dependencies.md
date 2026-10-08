# G-7 — Stage 4 dependency decisions: pins, licences, upstream libraries, the shared tool crate

- **Date:** 2026-10-08.
- **Issue:** #7 (Epic G, Stage 4, milestone M5). Stage 4 has no task for this pre-adapter
  work, so the record is named after the epic and its issue, as `F-6-stage3-exit.md` is.
  Refs #59 (the adapter contract suite this changes before any adapter exists) and #69 (G8,
  whose reply pairing is a separate PR, §9).
- **Source:** lead decisions in chat, 2026-10-08: four numbered decisions; the licence
  clarification of the same day (§3.2); the resolution of the two findings this work
  stopped on (§2.2, §3.2). The standing permissive policy is the operator decision of
  2026-10-06 on #51 (https://github.com/RossGraeber/OAC/issues/51#issuecomment-6009697192).
- **Status:** **Decided by the lead.** In force from the lead's merge of the PR that adds
  this record: the lead authors and merges it, and GitHub does not let an author approve
  their own PR, so the merge is the lead's approval (as for PR #276, E-5 §2, and PR #342,
  F-6 §5).
- **Owns:** its own text; `docs/planning/v0.1/07-repository-and-dependencies.md` §1-§9
  edits dated 2026-10-08; the `docs/planning/PINS.md` rows and records it adds or changes;
  the license policy in `scripts/check-licenses.mjs`; rules 1, 3, 4 and 6 of
  `scripts/check-crate-deps.mjs` as changed here; check 12's scope in
  `scripts/check-containment.mjs`; `VETTED_DEPENDENCIES` and `FORBIDDEN_CRATES` in
  `tests/protocol/contract/adapter/src/source.rs`; the `adapters/mcp-tools/` skeleton; the
  vendored schema under `docs/planning/vendor/codex-app-server-protocol/`; the STATUS top entry.
  It changes no file under `spec/` (frozen, E7 §7), no gate verdict and no ADR text.
- **Skills:** `oac`, `oac-implementation`, `oac-boundaries`, `oac-evidence`, `oac-zenoh`,
  `oac-codex-appserver`.

No crate is added to any workspace member's `[dependencies]` here except `oac-core` to the
new `adapters/mcp-tools/` skeleton. `zenoh`, `tokio`, `rcgen`, `rmcp`, `windows-sys` and
`libc` are pinned and licence-checked here and added by the implementing tasks (G1 #62
onward). Every licence and dependency fact below was checked by a real resolve in scratch
crates outside the workspace, deleted afterwards, using the repository's own
`checkInventory` from `scripts/check-licenses.mjs` (§3.3).

---

## 1. The decisions at a glance

| # | Decision | Outcome |
|---|---|---|
| 1 | Downloads approved; pin each exactly; license-check each | Pins in §3.1. Licence policy clarified (§3.2); the full graph passes (§3.3) |
| 2 | Adapters may use upstream libraries: the Codex app-server protocol crates and `rmcp` | **`rmcp` only.** No Codex crate passes ADR-001 (§2). `VETTED_DEPENDENCIES` loosened for `oac-mcp-tools`, `rmcp` and `tokio` (§5); forbidden crates refused by name and by transitive presence (§2.3) |
| 3 | A shared crate for the MCP tool surface | `adapters/mcp-tools/` (`oac-mcp-tools`), its own module kind; skeleton only (§4) |
| 4 | The Codex reply pairing (G8, #69) as a frozen-spec change | Separate PR (§9) |
| D3 | IPC crate | Recommendation, G9 confirms (§8.1) |
| D5 | Codex schema artifacts | Live, not moot: hand-written adapter against the vendored schema (§8.2) |
| D8 | Who opens the Codex connection | Recommendation, G6 confirms (§8.3) |

D3, D5 and D8 are numbered as in the lead's Stage 4 decision list in chat; they are not the
Epic D spike keys D1-D7.

---

## 2. Decision 2: upstream libraries for adapters

### 2.1 What the lead decided

Adapters may use upstream libraries instead of hand-writing everything on `oac-core`: the
Codex app-server protocol crates from `openai/codex`, pinned to a tag (the current tested
Codex, `0.161.0`, so tag `rust-v0.161.0`), and `rmcp` for the MCP server side. ADR-001 still
binds: only protocol or types crates; never a Codex crate that does inference, calls OpenAI
or model APIs, or reads `auth.json`, keyrings or rollouts; each crate's own dependency tree
verified for that; a forbidden crate refused by name and by transitive presence, with
planted breaches proving it.

### 2.2 What the verification found: no Codex crate passes

The tag exists: `rust-v0.161.0`, tag object `7e21416b38834816c224ea0dfd135c3de94b2f15`,
commit `979011409de0a60b52f179721948e65531d26144` (`git ls-remote
https://github.com/openai/codex`, retrieved 2026-10-08). PINS.md records the same commit as
the last tested Codex.

A scratch crate depending on `codex-app-server-protocol` at that tag resolves only after
copying Codex's own `[patch.crates-io]` forks (`crossterm`, `tokio-tungstenite`,
`tungstenite` from `openai-oss-forks`; `codex-rs/Cargo.toml` L641-L652 at that commit). It
then reaches 801 packages. The crate manifests at the commit, and `cargo tree -i` over the
resolve, give these chains (all retrieved 2026-10-08):

| Crate | Reaches | Why that is forbidden |
|---|---|---|
| `codex-app-server-protocol` | `codex-rollout` → `codex-otel` → `codex-api` → `codex-client` | `codex-api` is a model API client: `codex-api/src/provider.rs` L55 `"https://api.openai.com/v1"`, `endpoint/responses.rs` L67 `api.path = "/responses"`. `codex-rollout` handles the rollout files ([ADR-001 Boundary]; PLANNING-PROMPT.md §10) |
| `codex-app-server-protocol` | `codex-secrets` → `codex-keyring-store` | `codex-keyring-store/Cargo.toml` L11-L26: `keyring` with the Windows, Apple and Secret Service native stores |
| `codex-app-server-protocol` | `codex-state`, `codex-network-proxy`, `codex-http-client` | through `codex-rollout` and `codex-protocol` |
| `codex-app-server-transport` | `codex-core`, `codex-login`, `codex-api`, `codex-model-provider` (direct, `app-server-transport/Cargo.toml` `[dependencies]`) | the harness core, its login and its model client |
| `codex-app-server-client` | `codex-app-server`, `codex-core` (direct) | the whole app-server |
| `codex-protocol` | `codex-network-proxy`, `codex-http-client` (536 packages) | not a types-only crate either, and it lacks the app-server v2 types |

So the lead's "use the libraries" decision applies to **`rmcp` only**. That is the lead's
resolution of 2026-10-08: "every codex-app-server crate at rust-v0.161.0 reaches a model API
client, a keyring store, or rollouts; ADR-001 is inviolable. So: no Codex crate is allowed."
The Codex adapter is hand-written on `oac-core` against the vendored schema (D5, §8.2).

### 2.3 Enforcement: refused by name and by transitive presence

The forbidden list is the crates named in §2.2: `codex-app-server`,
`codex-app-server-client`, `codex-app-server-protocol`, `codex-app-server-transport`,
`codex-core`, `codex-api`, `codex-client`, `codex-login`, `codex-keyring-store`,
`codex-secrets`, `codex-rollout`, `codex-state`, `codex-model-provider`, `codex-otel`,
`codex-http-client`, `codex-network-proxy`, `codex-protocol`. Names match lowercased with
`_` folded to `-`.

- **By transitive presence:** `scripts/check-crate-deps.mjs` rule 6 (`FORBIDDEN_EXTERNAL`)
  fails any workspace member, `cli/` included, whose closure over normal, build and dev
  edges, every platform and `--all-features` holds a forbidden name. A crate under an
  innocent name that reaches one fails through it. The old rule-4 family "Codex app-server
  crates, owned by `adapters/codex`" is removed: nothing owns them, because nothing may have
  them.
- **By name, in the adapter static scan:** `FORBIDDEN_CRATES` in
  `tests/protocol/contract/adapter/src/source.rs`. `vet_dependency` refuses a forbidden crate
  under every dependency kind, dev included (a dev-dependency is otherwise the adapter's own
  tests' business). `tests/real_adapters.rs` `the_forbidden_lists_agree` checks that the two
  lists name the same crates.
- **Planted breaches.** `check-crate-deps.mjs --self-test`: one must-fail case per forbidden
  crate as a direct dependency of `adapters/codex`; `cli/` reaching `codex-core` through a
  third-party crate; the recorded chain `codex-app-server-protocol` → `codex-rollout` →
  `codex-otel` → `codex-api`; an unlisted crate that only reaches `codex-keyring-store`; a
  dev edge, a build edge, and a mixed-case underscore spelling; and a control that an
  unlisted `codex-utils-*` crate is not refused by name. The former control "adapters/codex
  may depend on codex_app_server_protocol" is now a must-fail case.
  `check-crate-deps.mjs --mutation-test` plants the same in a copy of the real workspace
  with real cargo: `adapters/codex` depending on `codex_app_server_protocol`, `cli/`
  depending on a stub that reaches `codex-api`, and a dev-dependency on `codex-api`.
  `source.rs` `forbidden_crates_are_refused_under_every_kind` plants each forbidden name
  under every kind and spelling.

---

## 3. Decision 1: pins and licences

### 3.1 Pins

Versions and licences from the crates.io API (`https://crates.io/api/v1/crates/<name>/<version>`),
retrieved 2026-10-08, and confirmed by the resolve in §3.3.

| Crate | Pin | Published | Licence (elected) | Features | Consumer (recorded; implementing task confirms) |
|---|---|---|---|---|---|
| `zenoh` | `=1.10.1` | 2026-09-07 | EPL-2.0 OR Apache-2.0 (Apache-2.0) | `default-features = false`; `transport_tcp`, `transport_tls` only. No `unstable`, no `shared-memory`, no other transport (the C7 local mode: a loopback TCP listener and a loopback TLS listener; LAN mode TLS; QUIC is C7's named alternative, not enabled) | `transports/zenoh/` only (G1 #62) |
| `tokio` | `=1.53.2` | 2026-10-03 | MIT | Per consumer. In `adapters/*` and `adapters/mcp-tools/`: only what `rmcp` enables on it (`sync`, `macros`, `rt`, `time`, `io-util`; §5). `cli/` (daemon runtime, IPC) and `transports/zenoh/` take what they need, recorded by their task | `cli/`, `adapters/*`, `adapters/mcp-tools/`, `transports/zenoh/` |
| `rcgen` | `=0.14.10` | 2026-08-28 | MIT OR Apache-2.0 (Apache-2.0) | `default-features = false`; `pem`, `ring` (the ring backend, lead decision 2026-10-08) | the local-mode TLS certificate (C7 §5): `transports/zenoh/` or the daemon's state code in `cli/`, as G3 (#64) decides |
| `rmcp` | `=3.4.0` | 2026-09-15 | Apache-2.0 | `default-features = false`; `server`, `transport-async-rw`. `macros` refused (§5) | `adapters/mcp-tools/`, `adapters/claude/`, `adapters/codex/` (G4 #65, G5 #66, G8 #69) |
| `windows-sys` | `=0.61.2` | 2025-10-06 | MIT OR Apache-2.0 (Apache-2.0) | `Win32_Foundation`, `Win32_System_Pipes` (`GetNamedPipeClientProcessId`, `Windows/Win32/System/Pipes/mod.rs` L14 at 0.61.2) | `cli/` (G9 #70). Already in `Cargo.lock` at this version |
| `libc` | `=0.2.190` | — | MIT OR Apache-2.0 (Apache-2.0) | none | `cli/` on Unix, as today. macOS peer PID: `LOCAL_PEEREPID`/`LOCAL_PEERPID` (`src/unix/bsd/apple/mod.rs` L3172 at 0.2.190); Linux `SO_PEERCRED`. Already in `Cargo.lock` |

**`rmcp` stays at the existing pin, `3.4.0`, not `3.5.1`.** The current release is `3.5.1`
(2026-10-05). Moving the `Rust MCP SDK (rmcp)` row is a pin move under PINS.md's
"Pin-move checklist": its `Gates affected` cell names G4 and G1, and both result files list
it under `Pin rows relied on` (`G1-result.md` L189-L190, `G4-result.md` L298-L300), so the
move would revert both verdicts to `NOT RUN` in the same commit, although neither run
exercised `rmcp` (`G4-result.md` L18-L24). That is a gate decision this record does not
make. `3.4.0` has the same feature structure as `3.5.1` (`server` → `transport-async-rw`,
`schemars`; `macros` → `rmcp-macros`), the same three exported `macro_rules!`, the same
`tokio` features, and the same licence result in §3.3. Moving to `3.5.1` is a pin move the
lead can make later, with the checklist.

### 3.2 Licence policy (lead clarification, 2026-10-08)

The no-copyleft rule exists to stop OAC (Apache-2.0) from being forced to relicense; in the
lead's words, LGPL sub-dependencies "are not as much of a concern". Implemented in
`scripts/check-licenses.mjs` and 07 §5 "Accepted licenses":

- **Accepted, permissive:** the existing list (Apache-2.0, MIT, 0BSD, Unicode-3.0,
  BSD-3-Clause) plus **Zlib, ISC, BSD-2-Clause and CDLA-Permissive-2.0**, under the standing
  permissive policy (operator decision on #51). With ISC on the list, ring's compound
  **`Apache-2.0 AND ISC`** is an accepted arm as written.
- **Accepted, weak or file-level copyleft used as an unmodified dependency:** MPL-2.0,
  LGPL-2.1-only, LGPL-2.1-or-later, LGPL-3.0-only, LGPL-3.0-or-later, EPL-2.0. Elected only
  when no permissive arm is offered, and reported in the listing. `option-ext` `0.2.0`
  (MPL-2.0) needs no exception. A statically linked LGPL crate may carry relink
  obligations; they are listed in the Stage 6 licence inventory (I3, #79), not blocked on
  now.
- **Refused, strong copyleft:** the GPL family (`-only` and `-or-later`), AGPL, SSPL, OSL
  and the like. An unknown identifier, an unparseable expression, or no expression at all
  fails, as before.
- **Dual licences:** an expression passes when one OR-arm is accepted. Apache-2.0 is
  elected whenever offered (zenoh: Apache-2.0), then a permissive arm, then a weak-copyleft
  arm.

This replaces the first instruction of the day, a single named MPL-2.0 exception for
`option-ext`; that exception was never landed.

Self-test cases added: Zlib, ISC, BSD-2-Clause, CDLA-Permissive-2.0 and `Apache-2.0 AND ISC`
pass; MPL-2.0, each LGPL form, EPL-2.0 and `MIT AND LGPL-2.1-only` pass; `MPL-2.0 OR MIT`
elects MIT; GPL-3.0-only, GPL-3.0-or-later, GPL-2.0-only, GPL-2.0-or-later, AGPL-3.0-only,
AGPL-3.0-or-later, SSPL-1.0, OSL-3.0 and the deprecated `GPL-3.0` fail; `MIT OR GPL-3.0`
passes on MIT; `MIT AND GPL-3.0` and `MPL-2.0 AND GPL-2.0-or-later` fail; BSD-1-Clause,
`ISC AND OpenSSL` and the deprecated `LGPL-2.1+` fail; an MPL-2.0 inventory row passes and
reports its weak copyleft.

### 3.3 The resolved graph

A scratch crate with exactly the §3.1 pins and features (zenoh, tokio with `rt`,
`rt-multi-thread`, `net`, `io-util`, `sync` and `time`, rcgen, rmcp, and the two OS
binding crates on their platforms), resolved with the Rust 1.98.1 toolchain and read with
`cargo metadata --all-features --locked`, the same view `check-licenses.mjs` takes: **311
third-party packages, all accepted.** By declared licence:

- 168 `MIT OR Apache-2.0`, 15 `Apache-2.0 OR MIT`, 12 `MIT/Apache-2.0`, 1
  `Apache-2.0/MIT`, 2 `Apache-2.0` (`rmcp` 3.4.0, `tls-listener` 0.11.2), 44 `MIT`, 7
  `Unlicense OR MIT`, and 13 under nine other permissive expressions (among them
  `subtle`'s BSD-3-Clause and `unicode-ident`'s `(MIT OR Apache-2.0) AND Unicode-3.0`).
- 30 `EPL-2.0 OR Apache-2.0` (the zenoh crates, `keyed-set`, `ringbuffer-spsc`,
  `token-cell`, `uhlc`, `validated_struct(_macros)`, `stabby(-abi, -macros)`): Apache-2.0
  elected, EPL-2.0 flagged.
- 2 `MIT OR Apache-2.0 OR LGPL-2.1-or-later` (`r-efi` 5.3.0, 6.0.0): Apache-2.0 elected.
- **Newly accepted permissive:** Zlib: `const_format` 0.2.36, `const_format_proc_macros`
  0.2.34, `foldhash` 0.1.5 and 0.2.0, `konst` 0.2.20, `konst_macro_rules` 0.2.19,
  `nanorand` 0.7.0, `zlib-rs` 0.6.8 (in the metadata graph, not compiled: `cargo tree`
  prints nothing for it). ISC: `json5` 0.4.1, `libloading` 0.8.9, `rustls-webpki`
  0.103.15, `untrusted` 0.9.0. BSD-2-Clause: `git-version` and `git-version-macro` 0.3.9.
  CDLA-Permissive-2.0: `webpki-roots` 1.0.9. `Apache-2.0 AND ISC`: `ring` 0.17.14.
- **Weak copyleft elected:** MPL-2.0: `option-ext` 0.2.0, reached unconditionally by zenoh:
  `zenoh-util` → `shellexpand` 3.1.2 → `dirs` 6.0.0 → `dirs-sys` 0.5.0 → `option-ext`.

Where each newly accepted licence comes from: the Zlib, ISC (`json5`, `libloading`),
BSD-2-Clause and MPL-2.0 crates are reached by `zenoh` even with no features; TLS adds
`ring`, `rustls-webpki`, `untrusted` and `webpki-roots` (through `zenoh-link-tls`,
`tls-listener`, `tokio-rustls` and `rustls` 0.23.45); `rcgen` with `ring` adds `ring` and
`untrusted`. `tokio`, `rmcp` (as pinned), `windows-sys` and `libc` alone add none.

The per-package listing is the appendix. For the record, the refused Codex graph (§2.2)
would also have needed BSL-1.0 (`clipboard-win`, `error-code`), more MPL-2.0 (`nucleo`,
`nucleo-matcher`) and the `aws-lc` ISC/OpenSSL terms; none of it is in OAC's graph.

---

## 4. Decision 3: the shared MCP tool crate

- **Path and name:** `adapters/mcp-tools/`, crate `oac-mcp-tools`, following the
  `oac-<name>` naming of every member.
- **Why there:** both adapters present the same four tools (`send`, `reply`,
  `list_sessions`, `whoami`; `spec/bindings/mcp.md` §5) and [MCPB-TOOL-003] requires the
  same tool names, `inputSchema` and result shape to every harness, but an adapter may not
  depend on another adapter (07 §3). The definitions are adapter-side, MCP-specific and
  provider-neutral, so they sit beside the adapters, not in `core/` (which stays free of MCP
  vocabulary) and not in `cli/` (which nothing may depend on).
- **Module kind:** `tools` in `scripts/check-crate-deps.mjs` `moduleOf`, matched before the
  adapter pattern, so it is never an adapter itself. Edges: `adapters/mcp-tools` → `core/`
  only (rule 3 requires the normal edge), plus fakes and suites as dev-dependencies;
  `adapters/*` and `cli/` may depend on it; `core/`, transports, fakes, suites and the
  security suite may not. Self-test: two controls and ten must-fail cases; mutation
  test: `adapters/mcp-tools` → `adapters/claude`, and `transports/memory` →
  `adapters/mcp-tools`.
- **Scanned as an adapter:** the adapter static scan reads only the files it is given, so
  `tests/real_adapters.rs` scans `adapters/mcp-tools` under the same rules as both adapters
  (and checks that it exports no macro). `scripts/check-containment.mjs` already covers it
  (`adapters/` is in both checks' scope).
- **Skeleton only:** a manifest depending on `oac-core` and a documented empty `lib.rs`.
  G5 (#66) and G8 (#69) fill it.

---

## 5. The `VETTED_DEPENDENCIES` change

`tests/protocol/contract/adapter/src/source.rs` `VETTED_DEPENDENCIES` was `["oac-core"]`. It
is now `["oac-core", "oac-mcp-tools", "rmcp", "tokio"]`, each vetted by identity (`Vetted`):

- `oac-core`, `oac-mcp-tools`: path dependencies with no `source`, under their own names,
  whose directories canonicalize to `core/` and `adapters/mcp-tools/`.
- `rmcp`: from crates.io (`registry+https://github.com/rust-lang/crates.io-index`) at
  exactly `=3.4.0`, default features off, features only `server` and `transport-async-rw`.
  So `macros` (the `rmcp-macros` attribute proc-macros and the `pastey` re-export) is
  refused, and so is every HTTP, auth or child-process feature.
- `tokio`: from crates.io at exactly `=1.53.2`, features only those `rmcp` 3.4.0 enables on
  it (`sync`, `macros`, `rt`, `time`, and `io-util` through `transport-async-rw`;
  `rmcp-3.4.0/Cargo.toml` L840-L847). `tokio` is allowed in adapters because it is `rmcp`'s
  runtime (lead, 2026-10-08). `net` and `process` are refused in adapters (§8.3).

Anything else stays refused, renames included.

**Macro vetting.** The old rule was "each vetted crate exports no macro". It still holds for
the two repository crates. The two crates.io crates export `macro_rules!`, so for them the
rule is that no exported macro can load a file. Read at the pinned versions on 2026-10-08:

- `rmcp` 3.4.0 exports `object!`, `const_string!` and (feature `elicitation`, not enabled)
  `elicit_safe!`. With `macros` off it re-exports no proc-macro of its own. Through
  `server` → `schemars` it re-exports the `schemars` crate, whose `JsonSchema` derive
  (`schemars_derive` 1.2.2) reads the annotated item, not files (its only `include_str!` is
  its own documentation, `src/lib.rs` L30).
- `tokio` 1.53.2 exports `join!`, `try_join!`, `pin!`, `select!` and `task_local!`.
  `select!` expands to an inline `mod __tokio_select_util { .. }`, which loads no file.

`macros_that_load_files` in `source.rs` makes this mechanical: it finds `include!`,
`include_str!`, `include_bytes!`, `#[path ..]` and `mod name;` in any `macro_rules!` body,
with planted cases. Run over the `rmcp` 3.4.0 and 3.5.1 and `tokio` 1.53.2 sources it finds
nothing. `tests/real_adapters.rs`
`the_vetted_registry_dependencies_export_no_file_loading_macro` re-runs it, and checks for a
proc-macro target and the pinned version, once either crate is in the workspace graph;
until then it says it is not in the graph rather than passing on a crate it never read.

---

## 6. Gate S4 criterion 1: the contract-suite baseline

Gate S4 criterion 1 (`10-stages.md` §8, L763-L765): the contract suites pass on the real
modules without modification; a suite edited to make a real module pass is a contract
change. The changes here to `tests/protocol/contract/adapter/` (`src/source.rs`,
`tests/real_adapters.rs`, `README.md`) are a deliberate, recorded pre-adapter change, made
before any adapter code exists, not an edit to make a real module pass.

- **Baseline candidate:** the head commit of the PR that adds this record, for
  `tests/protocol/contract/`.
- **Parallel pre-adapter change:** #347 (PR #348, branch
  `test/347-handoff-failed-assertion`) edits `tests/protocol/contract/adapter/src/lib.rs`,
  `src/plant.rs`, `src/claude.rs`, `src/codex.rs` and `tests/stand_in.rs`. This PR touches
  none of those files. The Gate S4 baseline is `tests/protocol/contract/` on `main` once both
  have merged; the Stage 4 exit record cites that merge commit.

---

## 7. Containment check 12 no longer scans `Cargo.lock`

Lead decision, 2026-10-08. Once `transports/zenoh/` depends on `zenoh` (G1, #62, branch
`transport/62-zenoh`), the root `Cargo.lock` names the zenoh crates by design, and check 12
would fail on it. Check 12's `ZENOH_SCOPE` drops `Cargo.lock`; the code scan is unchanged
(the root `Cargo.toml`, `core/`, `cli/`, `adapters/`, `transports/` outside
`transports/zenoh/`, `spec/`, `tests/fakes/`, `tests/protocol/`). Which member may depend on
a zenoh crate is `scripts/check-crate-deps.mjs` rule 4, over the resolved graph: a zenoh
crate is a direct dependency of `transports/zenoh` only and reachable from no member but it
and `cli/`. The self-test's "zenoh crate in the root lock" must-fail case is replaced by a
control (a zenoh crate in the lock is out of scope) and a case that runs `check-crate-deps`'s
`checkMetadata` on a graph where `adapters/codex` depends on `zenoh` and requires rule 4 to
catch it.

---

## 8. D3, D5, D8

### 8.1 D3: the IPC crate (recommendation; G9 #70 confirms)

**Recommend `tokio`'s own local IPC, not `interprocess`:** `tokio::net::windows::named_pipe`
on Windows and `tokio::net::UnixListener`/`UnixStream` on Unix, in `cli/` only. Peer PID:
on Unix, `UnixStream::peer_cred()` (`tokio-1.53.2/src/net/unix/ucred.rs`: Linux
`SO_PEERCRED`; macOS `getsockopt(SOL_LOCAL, LOCAL_PEEREPID)` plus `getpeereid`, L294-L335);
on Windows, `GetNamedPipeClientProcessId` from `windows-sys` `Win32_System_Pipes`. This
answers C2 §4's open peer-credential question (07 §9) for the `tokio` route, with crates
already pinned here, and adds no new crate: `interprocess` `2.4.4` stays a C2 candidate
only and is not pinned for use. Left for G9 to confirm against the named-pipe DACL design
(RISK-LOCAL-IPC; risks rows 19-20).

### 8.2 D5: Codex schema artifacts (decided by §2; G6 #67 implements)

Not moot: with no Codex crate (§2), the Codex adapter is hand-written on `oac-core` against
the app-server-protocol `schema/json` at `rust-v0.161.0`, vendored unmodified at
`docs/planning/vendor/codex-app-server-protocol/rust-v0.161.0/json/` with the upstream `LICENSE`
(Apache-2.0, blob `4606e72e`) and `NOTICE` beside it. Tree hash
`a75f7eb21162b7fcba3102cf304d6f56099826d5`, 315 files, matches the upstream tree exactly
after staging (`git write-tree --prefix`). **Where:** the repository keeps provider-derived
test data under `docs/planning/` and reads it in place, as the fakes read the recorded
fixtures in `docs/planning/gates/fixtures/` (07 §1, #57 note). It cannot sit under `tests/`:
its descriptions name the provider ("OpenAI API key provided by the caller …"), which
boundary-lint check 3 refuses anywhere in the code tree outside `docs/`, rightly, since the
check guards against provider SDK use. It is not a recorded harness capture, so it is
outside `docs/planning/gates/fixtures/` and its `MANIFEST.json`, which covers captures only
(`scripts/check-fixture-manifest.mjs`). The
experimental methods (`thread/queue/add` among them) are not in this default schema; their
shapes come from `codex app-server generate-json-schema --experimental` and the recorded
fixtures, and G6/G7 record which (`docs/planning/vendor/codex-app-server-protocol/README.md`).

### 8.3 D8: who opens the Codex connection (recommendation; G6 #67 confirms)

The lead decided the framing: hand-written RFC 6455 framing on std or `tokio`, no WebSocket
crate; the minimal client in `tests/protocol/contract/adapter/tests/stand_in.rs` (about L547)
is the pattern.

**Recommend: the daemon's start-up glue in `cli/` opens the byte stream; the Codex adapter
owns everything above bytes.** `cli/` connects to the app-server's control socket
(`CODEX_HOME/app-server-control/app-server-control.sock`, `oac-codex-appserver`) and hands
the adapter a connected stream. The adapter does the HTTP Upgrade, the RFC 6455 framing, the
JSON-RPC and the `initialize`/`initialized` handshake. Reasons:

- The control socket is a Unix domain socket on Windows too (G2 ran on Windows). Neither std
  nor `tokio` opens an `AF_UNIX` socket on Windows; Codex itself uses `uds_windows`. Opening
  it needs Winsock through `windows-sys`, an OS binding the lead approved for `cli/` (D3),
  which §5 does not admit into adapters.
- It keeps the adapter's vetted set at §5 (`tokio` without `net`).
- It matches 07 §2: `cli/` constructs and connects, and the provider protocol stays inside
  `adapters/codex/`.

The adapter never reads `CODEX_HOME/auth.json`, the keyring or rollouts; it only speaks the
documented app-server protocol over the stream it is handed.

---

## 9. Decision 4: the Codex reply pairing (G8, #69)

Designed and proposed separately, as a frozen-spec change for lead approval under E7 §7:
PR #350 (`spec/69-codex-reply-pairing`, "Codex issued-value pairing for outbound tool calls
(MCP binding 0.2)"). Only noted here.

---

## 10. Where it lands

| File | Change |
|---|---|
| `docs/planning/decisions/G-7-stage4-dependencies.md` | This record |
| `docs/planning/v0.1/07-repository-and-dependencies.md` | §1 layout note (`adapters/mcp-tools/`, `docs/planning/vendor/`); §2 module row; §3 diagram; §4(b) Codex shim; §5 inventory rows, licence policy, dated notes; §6 copyleft flag; §8 Codex packaging note; §9 labels |
| `docs/planning/PINS.md` | New rows and records: `tokio`, `rcgen`, `windows-sys`, `libc`; dated notes on `rmcp` (pin unchanged; consumers), Zenoh (features, licences of its graph), `interprocess` (not chosen for use, D3) |
| `docs/planning/v0.1/11-risks.md` | Traceability row 16: the `rmcp`-based server is now the adapters' MCP server side |
| `scripts/check-licenses.mjs` | Licence policy (§3.2) and self-test |
| `scripts/check-crate-deps.mjs` | `tools` module kind; rule 6 forbidden crates; Codex app-server owner family removed; self-test and mutation cases |
| `scripts/check-containment.mjs` | Check 12 scope without `Cargo.lock`; self-test (§7) |
| `tests/protocol/contract/adapter/src/source.rs`, `tests/real_adapters.rs`, `README.md` | §2.3, §5 |
| `adapters/mcp-tools/`, `Cargo.toml`, `Cargo.lock` | The skeleton crate (§4) |
| `docs/planning/vendor/codex-app-server-protocol/` | The vendored schema (§8.2) |
| `.claude/skills/oac-codex-appserver/` (and its `.agents/` copy) | The reusable-crates note now says ADR-001 refuses them (§2) and points at the vendored schema |
| `.claude/skills/oac-boundaries/references/mechanical-checks.md` (and its `.agents/` copy) | Check 12's description no longer names `Cargo.lock` (§7) |
| `docs/planning/STATUS.md` | Top entry |

---

## 11. Checks run

On this branch (Windows 11, Rust 1.98.1, short target dir):

- `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`;
  `cargo test --workspace`.
- `node scripts/check-licenses.mjs` (and `--self-test`, `--mutation-test`);
  `node scripts/check-crate-deps.mjs` (and `--self-test`, `--mutation-test`);
  `node scripts/check-containment.mjs` (and `--self-test`); `check-fixture-manifest.mjs`,
  `check-herdr-containment.mjs`, `check-skills.mjs`, `check-workflows.mjs`, each with its
  self-test; `node scripts/sync-agents-skills.mjs --check`.

All pass: `cargo test --workspace` green; `check-licenses` CLEAN (self-test 60/60,
mutation 4/4); `check-crate-deps` CLEAN, 11 members (self-test 98/98, mutation 28/28);
`check-containment` CLEAN (self-test 33/33); `check-fixture-manifest` (self-test 73/73);
`check-herdr-containment` CLEAN (self-test 102/103, one case skipped on this platform);
`check-skills` within budget; `check-workflows` CLEAN (self-test 101/101);
`sync-agents-skills --check` in sync (self-test 28/28). The `boundary-lint.yml` ripgrep
checks 3 and 8, run locally, are clean. CI on the PR is the record for the three OSes.

---

## Boundary and evidence pass

- **[ADR-001 Boundary]** "MUST NOT call provider model APIs as a substitute for native
  harnesses" and "MUST NOT steal or reuse another harness's provider credentials": the only
  upstream route that would have pulled a model API client (`codex-api`) and a keyring store
  (`codex-keyring-store`) into OAC is refused by name and by transitive presence (§2.3).
  `rmcp` and `tokio` hold neither.
- **[ADR-001 Boundary]** "MUST NOT leak Zenoh-specific concepts into the neutral protocol":
  unchanged. The lock leaves check 12's scope; the graph rule confines the zenoh crates
  (§7); code scanning is unchanged.
- Rollout files (PLANNING-PROMPT.md §10): `codex-rollout` is forbidden.
- No `spec/` file changes. `core/` gains nothing. `adapters/mcp-tools/` holds no provider
  or transport type.
- Every crate, version, licence and line citation above was fetched or resolved on
  2026-10-08 from crates.io, the crate sources at the pinned versions, or
  `github.com/openai/codex` at commit `979011409de0a60b52f179721948e65531d26144`. No
  UNVERIFIED item is opened or closed. Two existing items gain evidence that their owners
  can use: zenoh 1.10.1's TLS link uses `rustls` 0.23.45 (risks row 15, Zenoh's TLS stack),
  and crates.io confirms zenoh 1.10.1's 2026-09-07 publication (risks row 10). G1 (#62)
  owns closing them.

---

## Appendix: the resolved graph of §3.3, by declared licence

Output of `checkInventory` (`scripts/check-licenses.mjs` on this branch) over the scratch
crate of §3.3, grouped by declared licence expression. 311 packages, 0 violations.

| Count | Declared licence | Verdict under the policy (`checkInventory`) | Packages |
|---|---|---|---|
| 168 | `MIT OR Apache-2.0` | ok (elects Apache-2.0) | `aes@0.8.4`, `ahash@0.8.12`, `allocator-api2@0.2.21`, `android_system_properties@0.1.6`, `anyhow@1.0.104`, `arc-swap@1.9.2`, `array-init@2.1.0`, `asn1-rs@0.7.2`, `asn1-rs-derive@0.6.0`, `async-trait@0.1.92`, `base64@0.22.1`, `base64@0.23.1`, `bitflags@2.13.2`, `block-buffer@0.10.4`, `bumpalo@3.20.3`, `cc@1.6.0`, `cfg-if@1.0.5`, `chrono@0.4.45`, `cipher@0.4.4`, `core-foundation-sys@0.8.7`, `cpufeatures@0.2.17`, `crc32fast@1.5.2`, `crossbeam@0.8.5`, `crossbeam-channel@0.5.17`, `crossbeam-deque@0.8.8`, `crossbeam-epoch@0.9.21`, `crossbeam-queue@0.3.14`, `crossbeam-utils@0.8.23`, `crypto-common@0.1.7`, `defmt@1.1.1`, `defmt-macros@1.1.1`, `defmt-parser@1.0.0`, `der-parser@10.0.0`, `deranged@0.5.8`, `digest@0.10.7`, `dirs@6.0.0`, `dirs-sys@0.5.0`, `displaydoc@0.2.7`, `dyn-clone@1.0.20`, `either@1.19.0`, `find-msvc-tools@0.1.14`, `fixedbitset@0.5.7`, `flate2@1.1.10`, `futures@0.3.34`, `futures-channel@0.3.34`, `futures-core@0.3.34`, `futures-executor@0.3.34`, `futures-io@0.3.34`, `futures-macro@0.3.34`, `futures-sink@0.3.34`, `futures-task@0.3.34`, `futures-util@0.3.34`, `getrandom@0.2.17`, `getrandom@0.3.4`, `getrandom@0.4.3`, `hashbrown@0.12.3`, `hashbrown@0.14.5`, `hashbrown@0.15.5`, `hashbrown@0.16.1`, `hashbrown@0.17.1`, `hermit-abi@0.5.3`, `hex@0.4.3`, `hmac@0.12.1`, `home@0.5.12`, `humantime@2.4.0`, `iana-time-zone@0.1.65`, `iana-time-zone-haiku@0.1.2`, `inout@0.1.4`, `ipnetwork@0.20.0`, `itertools@0.14.0`, `itoa@1.0.18`, `js-sys@0.3.106`, `lazy_static@1.5.1`, `libc@0.2.190`, `lock_api@0.4.14`, `log@0.4.34`, `num_cpus@1.17.0`, `num-bigint@0.4.8`, `num-conv@0.2.2`, `num-integer@0.1.47`, `num-traits@0.2.19`, `oid-registry@0.8.1`, `once_cell@1.21.4`, `paste@1.0.15`, `pastey@0.2.3`, `pest@2.9.2`, `pest_derive@2.9.2`, `pest_generator@2.9.2`, `pest_meta@2.9.2`, `petgraph@0.8.3`, `pnet_base@0.35.0`, `pnet_datalink@0.35.0`, `pnet_sys@0.35.0`, `powerfmt@0.2.1`, `ppv-lite86@0.2.21`, `proc-macro-crate@3.5.0`, `proc-macro2@1.0.107`, `quote@1.0.47`, `rand@0.8.8`, `rand_chacha@0.3.1`, `rand_core@0.6.4`, `rcgen@0.14.10`, `ref-cast@1.0.27`, `ref-cast-impl@1.0.27`, `regex-automata@0.4.18`, `regex-syntax@0.8.11`, `ron@0.12.2`, `rustc_version@0.4.1`, `rustls-pki-types@1.15.1`, `rustversion@1.0.23`, `scopeguard@1.2.0`, `semver@1.0.28`, `serde@1.0.229`, `serde_core@1.0.229`, `serde_derive@1.0.229`, `serde_derive_internals@0.30.0`, `serde_json@1.0.151`, `serde_with@3.24.0`, `serde_with_macros@3.24.0`, `serde_yaml@0.9.34+deprecated`, `sha2-const-stable@0.1.0`, `sha3@0.10.9`, `shlex@2.0.1`, `siphasher@1.0.4`, `smallvec@1.16.2`, `socket2@0.5.10`, `socket2@0.6.5`, `static_assertions@1.1.0`, `syn@2.0.119`, `syn@3.0.6`, `thiserror@2.0.21`, `thiserror-impl@2.0.21`, `thread_local@1.1.10`, `time@0.3.55`, `time-core@0.1.9`, `time-macros@0.2.32`, `tokio-rustls@0.26.6`, `toml_datetime@1.1.2+spec-1.1.0`, `toml_edit@0.25.16+spec-1.1.0`, `toml_parser@1.1.4+spec-1.1.0`, `typeid@1.0.3`, `typenum@1.20.1`, `ucd-trie@0.1.7`, `unicode-xid@0.2.6`, `unzip-n@0.1.4`, `wasm-bindgen@0.2.129`, `wasm-bindgen-macro@0.2.129`, `wasm-bindgen-macro-support@0.2.129`, `wasm-bindgen-shared@0.2.129`, `windows_aarch64_gnullvm@0.52.6`, `windows_aarch64_msvc@0.52.6`, `windows_i686_gnu@0.52.6`, `windows_i686_gnullvm@0.52.6`, `windows_i686_msvc@0.52.6`, `windows_x86_64_gnu@0.52.6`, `windows_x86_64_gnullvm@0.52.6`, `windows_x86_64_msvc@0.52.6`, `windows-core@0.62.2`, `windows-implement@0.60.2`, `windows-interface@0.59.3`, `windows-link@0.2.1`, `windows-result@0.4.1`, `windows-strings@0.5.1`, `windows-sys@0.52.0`, `windows-sys@0.61.2`, `windows-targets@0.52.6`, `x509-parser@0.18.1`, `yasna@0.6.0` |
| 44 | `MIT` | ok (elects MIT) | `bytes@1.12.1`, `darling@0.24.1`, `darling_core@0.24.1`, `darling_macro@0.24.1`, `data-encoding@2.11.1`, `generic-array@0.14.7`, `libredox@0.1.25`, `lz4_flex@0.10.0`, `matchers@0.2.0`, `mio@1.2.4`, `no-std-net@0.6.0`, `nom@7.1.3`, `nonempty-collections@0.3.1`, `nu-ansi-term@0.50.3`, `pem@4.0.0`, `phf@0.13.1`, `phf_generator@0.13.1`, `phf_macros@0.13.1`, `phf_shared@0.13.1`, `redox_users@0.5.3`, `schemars@0.9.0`, `schemars@1.2.2`, `schemars_derive@1.2.2`, `sharded-slab@0.1.7`, `simd-adler32@0.3.10`, `slab@0.4.12`, `spin@0.9.9`, `spin@0.10.1`, `strsim@0.11.1`, `synstructure@0.13.2`, `tokio@1.53.2`, `tokio-macros@2.7.2`, `tokio-util@0.7.19`, `tracing@0.1.44`, `tracing-attributes@0.1.31`, `tracing-core@0.1.36`, `tracing-log@0.2.0`, `tracing-serde@0.2.0`, `tracing-subscriber@0.3.23`, `twox-hash@1.6.3`, `unsafe-libyaml@0.2.11`, `valuable@0.1.1`, `winnow@1.0.4`, `zmij@1.0.23` |
| 27 | `EPL-2.0 OR Apache-2.0` | ok (elects Apache-2.0); copyleft arm flagged: EPL-2.0 | `keyed-set@1.1.0`, `ringbuffer-spsc@0.1.15`, `token-cell@2.1.1`, `uhlc@0.8.2`, `validated_struct@2.2.0`, `validated_struct_macros@2.2.0`, `zenoh@1.10.1`, `zenoh-buffers@1.10.1`, `zenoh-codec@1.10.1`, `zenoh-collections@1.10.1`, `zenoh-config@1.10.1`, `zenoh-core@1.10.1`, `zenoh-crypto@1.10.1`, `zenoh-keyexpr@1.10.1`, `zenoh-link@1.10.1`, `zenoh-link-commons@1.10.1`, `zenoh-link-tcp@1.10.1`, `zenoh-link-tls@1.10.1`, `zenoh-macros@1.10.1`, `zenoh-plugin-trait@1.10.1`, `zenoh-protocol@1.10.1`, `zenoh-result@1.10.1`, `zenoh-runtime@1.10.1`, `zenoh-sync@1.10.1`, `zenoh-task@1.10.1`, `zenoh-transport@1.10.1`, `zenoh-util@1.10.1` |
| 15 | `Apache-2.0 OR MIT` | ok (elects Apache-2.0) | `autocfg@1.5.1`, `bit-vec@0.9.1`, `equivalent@1.0.2`, `event-listener@5.4.2`, `fastrand@2.5.0`, `indexmap@1.9.3`, `indexmap@2.14.2`, `keccak@0.1.6`, `parking@2.2.1`, `pin-project-lite@0.2.17`, `portable-atomic@1.15.0`, `portable-atomic-util@0.2.8`, `secrecy@0.8.0`, `uuid@1.27.0`, `zeroize@1.9.1` |
| 12 | `MIT/Apache-2.0` | ok (elects Apache-2.0) | `asn1-rs-impl@0.2.0`, `bitflags@1.3.2`, `bs58@0.5.1`, `ident_case@1.0.1`, `minimal-lexical@0.2.1`, `rusticata-macros@4.1.0`, `shellexpand@3.1.2`, `vec_map@0.8.2`, `version_check@0.9.5`, `winapi@0.3.9`, `winapi-i686-pc-windows-gnu@0.4.0`, `winapi-x86_64-pc-windows-gnu@0.4.0` |
| 8 | `Zlib` | ok (elects Zlib) | `const_format@0.2.36`, `const_format_proc_macros@0.2.34`, `foldhash@0.1.5`, `foldhash@0.2.0`, `konst@0.2.20`, `konst_macro_rules@0.2.19`, `nanorand@0.7.0`, `zlib-rs@0.6.8` |
| 7 | `Unlicense OR MIT` | ok (elects MIT) | `aho-corasick@1.1.5`, `jiff@0.2.38`, `jiff-core@0.1.1`, `jiff-static@0.2.38`, `jiff-tzdb@0.1.9`, `jiff-tzdb-platform@0.1.3`, `memchr@2.8.3` |
| 4 | `ISC` | ok (elects ISC) | `json5@0.4.1`, `libloading@0.8.9`, `rustls-webpki@0.103.15`, `untrusted@0.9.0` |
| 3 | `EPL-2.0 OR Apache-2.0` | ok (elects Apache-2.0); copyleft arm flagged: EPL-2.0 | `stabby@72.1.16`, `stabby-abi@72.1.16`, `stabby-macros@72.1.16` |
| 3 | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | ok (elects Apache-2.0) | `wasi@0.11.1+wasi-snapshot-preview1`, `wasip2@1.0.4+wasi-0.2.12`, `wit-bindgen@0.57.1` |
| 2 | `BSD-2-Clause` | ok (elects BSD-2-Clause) | `git-version@0.3.9`, `git-version-macro@0.3.9` |
| 2 | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | ok (elects Apache-2.0); copyleft arm flagged: LGPL-2.1-or-later | `r-efi@5.3.0`, `r-efi@6.0.0` |
| 2 | `Apache-2.0` | ok (elects Apache-2.0) | `rmcp@3.4.0`, `tls-listener@0.11.2` |
| 2 | `Apache-2.0 OR ISC OR MIT` | ok (elects Apache-2.0) | `rustls@0.23.45`, `rustls-pemfile@2.2.0` |
| 2 | `BSD-2-Clause OR Apache-2.0 OR MIT` | ok (elects Apache-2.0) | `zerocopy@0.8.62`, `zerocopy-derive@0.8.62` |
| 1 | `0BSD OR MIT OR Apache-2.0` | ok (elects Apache-2.0) | `adler2@2.0.1` |
| 1 | `Apache-2.0/MIT` | ok (elects Apache-2.0) | `flume@0.11.1` |
| 1 | `MIT OR Zlib OR Apache-2.0` | ok (elects Apache-2.0) | `miniz_oxide@0.9.1` |
| 1 | `MPL-2.0` | ok (elects MPL-2.0); weak copyleft elected, unmodified dependency: MPL-2.0 | `option-ext@0.2.0` |
| 1 | `Apache-2.0 AND ISC` | ok (elects Apache-2.0 AND ISC) | `ring@0.17.14` |
| 1 | `Apache-2.0 OR BSL-1.0` | ok (elects Apache-2.0) | `ryu@1.0.23` |
| 1 | `BSD-3-Clause` | ok (elects BSD-3-Clause) | `subtle@2.6.1` |
| 1 | `Zlib OR Apache-2.0 OR MIT` | ok (elects Apache-2.0) | `tinyvec@1.13.3` |
| 1 | `(MIT OR Apache-2.0) AND Unicode-3.0` | ok (elects Apache-2.0 AND Unicode-3.0) | `unicode-ident@1.0.26` |
| 1 | `CDLA-Permissive-2.0` | ok (elects CDLA-Permissive-2.0) | `webpki-roots@1.0.9` |
