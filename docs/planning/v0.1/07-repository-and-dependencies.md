# 07 — Repository and Dependencies

**Issue:** #28 (Epic A, backlog key A8). **Depends on:** #13. **Source:**
PLANNING-PROMPT.md §9.8, §5.12, `docs/planning/DESIGN.md` "Suggested repository shape"
(lines 135-150).

**Scope.** This file has two jobs: the module layout with ownership boundaries, and the
third-party dependency inventory with licenses. It does not restate `docs/planning/
DESIGN.md` or `docs/planning/v0.1/04-architecture.md` prose beyond what a table needs —
cite those files by path instead. It does not define interface signatures (the frozen
`spec/interfaces.md`'s job; it supersedes `docs/planning/v0.1/05-interfaces.md` §13-§15)
or the threat model
(`docs/planning/v0.1/06-security.md`'s job).

**Cite-not-re-derive rule, stated up front.** Every dependency fact in §5-§9 comes from
`docs/planning/decisions/C1-language-runtime.md` §3/§6/§7/§8/§10,
`docs/planning/decisions/C2-process-model.md` §4, and `docs/planning/decisions/
C4-session-identity.md` §10/§11 — cited by section, not re-fetched. No crate, version,
license, or retrieval date below is independently re-verified or newly dated by this
file.

**Naming.** Product and repository: **Open Agent Channel (OAC)**. Normative protocol
specification: **OAC Session Channels**. CLI binary: **`oac`**. Per ADR-001-A1
(`docs/planning/v0.1/03-decisions-and-amendments.md` §2), this file never writes bare
"Session Channels" or `sessionchannels`, except where a source document (`DESIGN.md`,
pre-rename) is quoted verbatim, marked as such at the quotation.

---

## 1. Repository shape

**Source sketch, reproduced verbatim** from `docs/planning/DESIGN.md` lines 136-149,
marked as the source sketch, not the resolved layout:

```text
spec/
  session-channels.md
  security.md
ADR/
core/
cli/
transports/zenoh/
adapters/claude/
adapters/codex/
tests/{protocol,security,integration}/
examples/
docs/
```

**Resolved v0.1 layout.** `core/`, `spec/`, `adapters/claude/`, `adapters/codex/`,
`transports/zenoh/`, `cli/`, `tests/{protocol,security,integration}/`, `examples/`,
`docs/`, `ADR/`.
(Dated note, 2026-10-06, #56: `transports/memory/` joins as a sibling of
`transports/zenoh/`, the in-memory transport of F7, a workspace member under the same
rule: it depends on `core/` only (§3), and adds no third-party crate of its own. The `Transport`
contract and the §4.11 transport-boundary types it implements live in `core/`, which
calls them; `spec/interfaces.md` §6.1 notes the in-memory transport.)
(Dated note, 2026-10-06, #57: F8 confirms where fake endpoints live, the deferral §2's
`tests/` row and `09-test-strategy.md` §13-§14 left open. Fake harness endpoints live at
`tests/fakes/<name>/` (F9's Codex fake follows the same convention). The fake Claude Code
channel endpoint is `tests/fakes/claude/`, a Rust workspace member (`oac-fake-claude`),
because the Rust contract and security suites link it in-process. It is test-only: it depends on `core/`
only (for the JSON reader) and on no third-party crate, and a product crate may take it as
a dev-dependency only, never a normal or build one, so it is never built into the `oac`
binary (§3). It reads the recorded fixtures in place from `docs/planning/gates/fixtures/`;
nothing is copied under `tests/`. It is not herdr and does not drive a harness:
`tests/integration/` stays the opt-in provider-integration leaf.)

**Dev/test tooling in the tree, outside the product layout.** `tools/herdr/` is the herdr
test driver (Epic K #123, K3 #126), run as `node tools/herdr/run.mjs --scenario <name>`.
K4 (#127) adds the `g1-claude-wake` scenario and its comparison tooling
(`tools/herdr/lib/compare-transcripts.mjs`, `tools/herdr/lib/g1-report.mjs`) under the
same directory and the same rules. K6 (#129) adds `tools/herdr/ci.mjs`, the only entry
point of the opt-in workflow `.github/workflows/herdr-provider-optin.yml`
(`docs/planning/gates/herdr-runner.md`). K7 (#130) adds the `g2-codex-inject` scenario
(`tools/herdr/lib/g2.mjs`, `tools/herdr/lib/g2-report.mjs`) the same way; K8 (#131) adds
`g4-mcp-dual-era` and `g5-provenance`, their report generators, the reconstructed G4/G5 gate
servers under `tools/herdr/gate-servers/` (test tooling only, never product code) and the
Stage 4/5 reuse contract in `tools/herdr/README.md`. It is **dev/test only** and **never a workspace member**. It is written with Node
built-ins only and has no `package.json`. No workspace or package manifest outside it
may reference it. Nothing under `core/`, `spec/`, `adapters/*`, `transports/zenoh/` or
`cli/` may import, link, vendor or invoke it, and it is not shipped with the `oac`
binary. It is not part of the CI-default test tier (`oac-testing` §2), because its
scenarios need a herdr binary and, for harness scenarios, a logged-in harness; its own
`--self-test` runs against a fake herdr and needs neither. herdr runs from it as an
external process only; herdr's license and pin are recorded under §5 "Dev/test tooling —
not shipped". `scripts/check-herdr-containment.mjs` (`oac-boundaries` checks 9 and 10)
enforces both halves: no herdr reference in product paths or in a manifest outside
`tools/herdr/`, and no harness-credential access or harness-config mutation inside it.

**The daemon is not a separate top-level directory.** Per `docs/planning/v0.1/
04-architecture.md` §2's component table: the daemon is a binary, not a module — it
hosts `core/` and starts the transport module. There is no `daemon/` directory in the
resolved layout; the daemon's own code is `core/` plus the transport-start/process-glue
code that lives with the binary's own entry point, not a module this file gives its own
top-level path.

**DESIGN.md's own caveat, already discharged.** Quoted, `docs/planning/DESIGN.md` line
150: "Do not choose implementation language solely from this sketch; research provider
SDK/IPC and Zenoh bindings first." This caveat is discharged by `docs/planning/decisions/
C1-language-runtime.md`, which researched the Rust MCP SDK (`rmcp`), the Codex app-server
client crates, and the Zenoh crate before fixing Rust as the implementation language
(C1 §1, §3, §6, §11) — this file does not re-argue the language choice, only cites that
it was made on the required evidentiary basis.

---

## 2. Module ownership table

One row per module. Owns/never-owns columns are sourced from `docs/planning/v0.1/
04-architecture.md` §2's component table, cited by path and section — not restated in
its own prose.

| Module | Single named responsibility | Owns | Never owns | May depend on |
|---|---|---|---|---|
| `core/` | Neutral types and policy/authorization | `SessionIdentity`, `SessionDescriptor`, `SessionCapabilities`, `ChannelMessage`, `DeliveryReceipt`, `PresenceRecord`, `SecurityPrincipal` (04-architecture.md §2) | Provider-native vocabulary; transport-native vocabulary (04-architecture.md §2) | Nothing in-repo (§3 below) |
| `spec/` | The neutral OAC Session Channels specification text | Normative spec prose (04-architecture.md §2) | Implementation code; provider-specific or transport-specific vocabulary (04-architecture.md §2) | Nothing in-repo |
| `adapters/claude/` | Translating neutral envelopes to/from Claude Code's provider-native wake and reply operations | Claude-specific rendering/translation logic (04-architecture.md §2) | The transport peer; key material; policy decisions — routes through core policy/security instead (04-architecture.md §2, quoting `docs/planning/DESIGN.md`) | `core/` only (§3) |
| `adapters/codex/` | Translating neutral envelopes to/from the Codex app-server's provider-native turn/thread operations | Codex-specific rendering/translation logic (04-architecture.md §2) | The transport peer; key material; policy decisions; OpenAI model-API credentials (04-architecture.md §2) | `core/` only (§3) |
| `transports/zenoh/` | Every Zenoh-specific type, identifier, and concept, behind the `Transport` contract | The `Transport` contract's operations over neutral types only (`spec/interfaces.md` Table 6.4) | Policy/authorization decisions; signature verification; anything visible outside the operations of `spec/interfaces.md` Table 6.4 | `core/` only (§3) |
| `transports/memory/` (added 2026-10-06, #56) | The in-memory transport: a loopback implementation of the `Transport` contract, with fault injection, for CI-default tests (`spec/interfaces.md` §6.1 note, [IFC-TRN-002]) | The `Transport` contract's operations over neutral types only (`spec/interfaces.md` Table 6.4); its transport binding document, as crate documentation ([IFC-TRN-090]) | Policy/authorization decisions; signature verification; any network; a third-party dependency of its own | `core/` only (§3) |
| `cli/` (including `mcp-shim`) | User-facing commands, the thin stdio shim a harness spawns, and the `oac` binary's entry point | `oac start`, `status`, `sessions`, `doctor`, `mcp-shim` — the `mcp-shim` subcommand carries nothing beyond a thin stdio connection to the daemon over local IPC (04-architecture.md §2); the daemon's start-up glue (§1, §3) | Long-lived process state; the transport peer; policy decisions; key material (04-architecture.md §2). Hosting the start-up glue is not owning: `cli/` constructs the adapters and the transport and hands them to `core/`, while the state and the peer stay in `core/` and the transport module (§3) | `core/`, `adapters/*`, `transports/*` (§3) |
| `tests/` | Protocol, security, and integration test suites (`tests/{protocol,security,integration}/`) | Fixtures and fake endpoints for provider/transport contract tests (`docs/planning/DESIGN.md` "Testing"; `docs/planning/v0.1/09-test-strategy.md`, A10, not yet landed) | Production code; anything shipped in the `oac` binary | `core/`, and whichever module a given test targets |

**Acceptance box 1 ticked here** — "Every module has one named responsibility and a
stated ownership boundary": every row above states exactly one responsibility, an
"Owns" column, and a "Never owns" column, sourced from `docs/planning/v0.1/
04-architecture.md` §2 rather than re-derived.

---

## 3. Dependency direction rule

Allowed edges, as a text diagram:

```text
cli/              -> core/, adapters/*, transports/*
adapters/*        -> core/           (only)
transports/zenoh/ -> core/           (only)
transports/memory/ -> core/          (only; #56)
core/             -> (nothing in-repo)
(nothing)         -> cli/
tests/fakes/* (Rust workspace members) -> core/   (only; #57)
adapters/*, transports/*, cli/ -> tests/fakes/* (Rust members)   (dev-dependency only; #57)
```

(Dated note, 2026-10-05, #305: the original rule allowed only `cli/ -> core/`. #305
widened it to the edges above, for the reasons below.)

**Why `cli/` has the adapter and transport edges.** `cli/` is where the `oac` binary's
entry point lives, and that binary is also the daemon, so `cli/` is the one module that
puts the daemon together:

- `docs/planning/DESIGN.md` §Components, "CLI / supervisor": it "Starts configuration,
  device identity, adapters, transport, diagnostics, and clean shutdown."
- `docs/planning/decisions/C1-language-runtime.md` §1: one OAC binary, `oac`, per platform.
- `docs/planning/decisions/C2-process-model.md` §1 and §6: the daemon is that binary,
  started by `oac start`; harnesses spawn the same binary as `oac mcp-shim`. The daemon
  owns the transport peer (C2 §1, item 1).
- `docs/planning/v0.1/04-architecture.md` §2: the `oac` binary's repo path is `cli/`, and
  the daemon hosts `core/` and starts the transport module.
- §1 above: the daemon's transport-start glue lives with the binary's own entry point, and
  there is no `daemon/` module.

Starting adapters and a transport needs both modules in scope, and `core/` cannot be the
one to name them (it depends on nothing in-repo). That leaves `cli/`.

These edges are for construction only. `cli/` builds the adapters and the transport and
hands them to `core/`; `core/` makes every contract call. In `spec/interfaces.md`, the
caller of every operation in Table 5.2 (`ProviderAdapter`) and Table 6.4 (`Transport`) is
the core, and its §1.1 makes the core the only route from an adapter to a transport.

**Explicit MUST NOTs.**

- `core/` MUST NOT depend on `adapters/*` or `transports/*`.
- `core/`, `adapters/*` and `transports/*` MUST NOT depend on `cli/`.
- An adapter MUST NOT depend on another adapter.
- An adapter MUST NOT depend on `transports/zenoh/`, or on any other transport. Adapters
  route through core policy (`docs/planning/DESIGN.md`: "Adapters should route through
  core policy/security rather than directly through transports"). The frozen normative
  rule is `spec/interfaces.md` [IFC-ADP-001], "An adapter MUST NOT invoke an operation of
  a transport."
- `cli/` MUST NOT invoke an operation of `spec/interfaces.md` Table 5.2 or Table 6.4. The
  core is the caller of all of them.

**Stage 3 enforcement owner.** `oac-implementation`'s module-dependency-direction rule
is the Stage 3 owner of enforcing this diagram in the actual workspace (lint/CI check);
this file states the rule, not the enforcement mechanism.
(Dated note, 2026-10-05, #50: the mechanism is `scripts/check-crate-deps.mjs`, which checks
these edges over `cargo metadata` and runs in `.github/workflows/rust-workspace.yml`.)
(Dated note, 2026-10-06, #57: the same script admits `tests/fakes/<name>` members, lets
them reach `core/` only, and fails any product member (`core/`, `cli/`, an adapter or a
transport) that reaches one over normal or build edges; `core/` may not reach one at all.)

---

## 4. Containment boundaries — the two that are ADR-001 boundaries, not style

### (a) Zenoh containment

Every Zenoh type, identifier, and key expression stays inside `transports/zenoh/`, per
`docs/planning/decisions/C7-zenoh-transport.md` §2 and DESIGN acceptance criterion 9.
The `Transport` contract's operations, frozen in `spec/interfaces.md` Table 6.4, are the
only visible surface crossing this module's boundary. Table 6.4 is the list; this file
does not copy it.

Nothing outside `transports/zenoh/` names a Zenoh type, `zid`, a key expression, or a
liveliness term — per `[ADR-001 Boundary]` "MUST NOT leak Zenoh-specific concepts into
the neutral protocol."

### (b) Compatibility-shim boundaries for the two non-supported surfaces

Per `oac-evidence` §4/§5:

- `adapters/claude/` isolates Claude Channels (UNVERIFIED — the module/interface name
  itself is not yet fixed in `DESIGN.md`; open ledger entry C11,
  `docs/planning/v0.1/03-decisions-and-amendments.md` line 755;
  `docs/planning/STATUS.md` "Open UNVERIFIED items"), labelled **research preview**,
  version **floating** per `docs/planning/PINS.md` (Claude Code (Channels) row,
  "Floating-version policy"). (Note, 2026-10-01, issue #186: previously "pinned
  `v2.1.274`"; that row went floating 2026-09-27. Dated note, 2026-10-01, #216: the row now records minimum `v2.1.282` and last
  tested `v2.1.285` ("Version policy"); a version change warns, never gates.)
- `adapters/codex/` isolates the Codex app-server (UNVERIFIED — same ledger entry C11;
  not yet fixed in `DESIGN.md`), labelled **experimental, per-method gating**, CLI /
  app-server version **floating** per `docs/planning/PINS.md` (Codex CLI / app-server
  row, "Floating-version policy"); the `codex-app-server-*` git dependencies in §5 stay at
  `0.154.0` @ commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`. (Note, 2026-10-01, issue
  #186: previously "pinned `0.154.0` @ commit `6b98…`" for the surface itself; that row
  went floating 2026-09-26. Dated note, 2026-10-01, #216: the row now records minimum `0.154.0` and last tested `0.159.3`
  ("Version policy"); a version change warns, never gates.)

Cross-reference `spec/interfaces.md` §5 for the adapter contract itself
(`ProviderAdapter`'s operations, Table 5.2) — not restated here.

---

## 5. Dependency inventory

Copied in substance from `docs/planning/decisions/C1-language-runtime.md` §10 plus
`docs/planning/decisions/C4-session-identity.md` §11. This is the new content this file
adds over C1: the "which module consumes it" mapping onto §2's module table.

| Crate | Version | License | Why needed | Copyleft? | Apache-2.0 compatible? | Source decision | Consuming module |
|---|---|---|---|---|---|---|---|
| `rmcp` | `3.4.0` | Apache-2.0 | Rust MCP SDK — server/client protocol implementation for the OAC Session Channels MCP packaging layer | No | Yes — same license | C1 §3, §10 | `cli/` (`mcp-shim`) |
| `codex-app-server-client` | `0.154.0` @ commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` (git dep — not on crates.io at any version) | Apache-2.0 | Codex app-server JSON-RPC client — Codex adapter transport | No | Yes — same license | C1 §6, §10 | `adapters/codex/` |
| `codex-app-server-protocol` | `0.154.0` @ same commit (git dep — crates.io has this crate but only at stale `0.63.0`) | Apache-2.0 | Codex app-server request/response/schema types | No | Yes — same license | C1 §6, §10 | `adapters/codex/` |
| `codex-app-server-transport` | `0.154.0` @ same commit (git dep — not on crates.io at any version) | Apache-2.0 | Codex app-server transport framing | No | Yes — same license | C1 §6, §10 | `adapters/codex/` |
| `zenoh` | `1.10.1` | EPL-2.0 / Apache-2.0 (dual) | Reference peer-to-peer transport plugin | **Yes — EPL-2.0 is the other arm of the dual license; flagged** (§6) | Yes — OAC elects the Apache-2.0 arm | C1 §7, §10 | `transports/zenoh/` only |
| `keyring` | `4.2.0` | MIT OR Apache-2.0 | OS-native credential store facade for OAC's own device keys (Windows Credential Manager / macOS Keychain / Linux Secret Service or keyutils) | No | Yes — OAC elects the Apache-2.0 arm | C1 §8, §10 | Daemon binary's own identity code (not `core/`, per §1's daemon note) |
| `keyring-core` | `1.0.0` | MIT OR Apache-2.0 | `keyring`'s only unconditional dependency — the trait/error surface the backend crates implement | No | Yes | C1 §10 | Daemon binary's own identity code (not `core/`) |
| `windows-native-keyring-store` | `1.1.0` | MIT OR Apache-2.0 | The Windows Credential Manager backend `keyring`'s `v1`/default feature pulls in | No | Yes | C1 §10 | Daemon binary's own identity code (not `core/`) |
| `interprocess` | `2.4.4` (candidate — final IPC crate a Stage 3 detail, C2 §4) | 0BSD OR Apache-2.0 | Local IPC transport (Windows named pipe / Unix `AF_UNIX` socket) between the daemon and `oac mcp-shim` | No | Yes — OAC elects the Apache-2.0 arm | C2 §4 | `cli/` (`mcp-shim`) + daemon binary |
| `age` | `0.12.1` | MIT OR Apache-2.0 | Encrypted-file key fallback for the device key when no OS credential store is reachable | No | Yes — OAC elects the Apache-2.0 arm | C4 §11 | Daemon binary's own identity code (not `core/`) |
| `serde_jcs` | `0.2.0` | MIT OR Apache-2.0 | RFC 8785 (JCS) canonical form of the signing input, `spec/security.md` §6.2 | No | Yes — OAC elects the Apache-2.0 arm | C5 §3; `PINS.md` "`serde_jcs`" | `core/` (`canonical`) |
| `ed25519-dalek` | `3.0.0` | BSD-3-Clause | Ed25519 device key, signing and strict verification (`verify_strict`), `spec/security.md` §5.1, §6.3 | No | Yes — permissive, a single license with no arm to elect (C5 §2) | C5 §2; `PINS.md` "`ed25519-dalek`" | `core/` (`keys`, `signing`) |
| `sha2` | `0.11.0` | MIT OR Apache-2.0 | SHA-256 key ids, `spec/security.md` §5.2. Already in the graph as `ed25519-dalek`'s own SHA-512 dependency, same version | No | Yes — OAC elects the Apache-2.0 arm | #52 (no decision names a hash crate; RustCrypto `hashes`, the crate `ed25519-dalek` itself uses) | `core/` (`keys`) |
| `getrandom` | `0.4.3` | MIT OR Apache-2.0 | The operating system's CSPRNG for device-key seeds ([SEC-KEY-003]) and envelope nonces ([SEC-SIG-003]) | No | Yes — OAC elects the Apache-2.0 arm | #52 (no decision names an RNG crate; `rust-random/getrandom`, the OS-source crate under `rand`) | `core/` (`keys`) |
| `zeroize` | `1.9.0` | Apache-2.0 OR MIT | Zeroizing buffers for the private seed ([SEC-KEY-004]). Already in the graph through `ed25519-dalek`'s default `zeroize` feature, same version | No | Yes — OAC elects the Apache-2.0 arm | #52 | `core/` (`keys`), `cli/` (`keystore`) |
| `subtle` | `2.6.1` | BSD-3-Clause | Constant-time comparison of the read-back seed (#315 review N-e). Already in the graph under `ed25519-dalek`, same version | No | Yes — permissive (see the accepted list) | #52 | `core/` (`keys`) |
| `curve25519-dalek` | `5.0.0` | BSD-3-Clause | Dev-dependency only: scalar arithmetic that builds malleable and small-order signatures in `signing.rs`'s tests (#315 review N-g). Already `ed25519-dalek`'s curve crate, same version; not a new crate in the build | No | Yes — permissive | #52 | `core/` (tests only) |

(Dated note, 2026-10-06, #52 / F3.) **Consuming module of the key-storage crates.** The
`keyring`, `keyring-core`, `windows-native-keyring-store` and `age` rows above say "daemon
binary's own identity code (not `core/`)". That code is `cli/src/keystore.rs`, in a library
target of `cli/` (`oac_cli`) that the `oac` binary builds: the operating system's credential
store through `keyring` (default feature `v1`) and, on Unix, the `age`-encrypted file
fallback. Both implement `core/`'s `KeyStore` trait, so `cli/` constructs them and hands them
to the core, and the seed lives in `core/`'s `DeviceKey` (§2: `cli/` hosts start-up glue and
owns no key material). `keyring`'s `v1` feature also builds `apple-native-keyring-store`
`1.0.2` and `zbus-secret-service-keyring-store` `1.0.1` (both MIT OR Apache-2.0) on their
platforms. `age` is a Unix-only target dependency of `cli/`, since the fallback is built only
there (#315 review N-d), and `cli/` takes `libc` `0.2.190` (MIT OR Apache-2.0, already in the
graph) on Unix for the file store's ownership check.

(Dated note, 2026-10-06, #315 review N-b.) **Owner enforced.** `scripts/check-crate-deps.mjs`
now treats these crates as owned by `cli/`, as it does the zenoh crates for
`transports/zenoh/` and the Codex app-server crates for `adapters/codex/`: `keyring`,
`keyring-core`, every `*-keyring-store`, `secret-service` and `security-framework(-sys)`, and
`age` and `age-core` may be a dependency of `cli/` only, and none may be reachable from any
other member. The signature crates in the rows below (`ed25519-dalek` and its curve crates,
`sha2`, `getrandom`, `zeroize`, `subtle`) are `core/`'s and are not restricted.

(Dated note, 2026-10-06, #52 / F3.) **Transitive packages.** The rows above bring 241 new
packages into the `--all-features` graph that `scripts/check-licenses.mjs` reads, most of
them under `age` (its `i18n-embed`/`fluent` localisation stack, `scrypt`, `chacha20poly1305`,
the `ml-kem`/`hpke` post-quantum recipients) and the per-platform `keyring` backends (`zbus`,
`security-framework`). Their licenses: MIT OR Apache-2.0 or Apache-2.0 OR MIT (the large
majority; Apache-2.0 elected), MIT, Unicode-3.0 (`tinystr`, `zerofrom`, `zerovec`), Unlicense
OR MIT (MIT elected), BSD-2-Clause OR Apache-2.0 OR MIT and MIT OR Apache-2.0 OR BSD-1-Clause
(Apache-2.0 elected), and BSD-3-Clause (`curve25519-dalek` `4.1.3` and `5.0.0`, `x25519-dalek`
`2.0.1`, `subtle` `2.6.1`, besides `ed25519-dalek`). Two are dual-licensed with a copyleft
arm, flagged by the script and passed on their Apache-2.0 arm, as `zenoh` is (§6):
`r-efi` `6.0.0` (MIT OR Apache-2.0 OR LGPL-2.1-or-later) and `self_cell` `1.3.0` (Apache-2.0
OR GPL-2.0-only). No package is copyleft-only. The full per-package listing is the script's
output; the transitive audit and NOTICE stay I2's.

(Dated note, 2026-10-06, #51 / F2.) `serde_jcs` brings these transitive packages, each
built: `serde` and `serde_core` `1.0.229` (MIT OR Apache-2.0), `serde_json` `1.0.151` (MIT OR
Apache-2.0; features `std`, `float_roundtrip`), `ryu-js` `0.2.2` (Apache-2.0 OR BSL-1.0),
`itoa` `1.0.18` (MIT OR Apache-2.0), `memchr` `2.8.3` (Unlicense OR MIT; the script elects
MIT) and `zmij` `1.0.23` (MIT). `serde`'s optional `derive` feature, which nothing in the
workspace enables, also puts `serde_derive` `1.0.229`, `proc-macro2` `1.0.107`, `quote`
`1.0.47`, `syn` `3.0.6` (each MIT OR Apache-2.0) and `unicode-ident` `1.0.26` ((MIT OR
Apache-2.0) AND Unicode-3.0) in the `--all-features` graph that `scripts/check-licenses.mjs`
reads. None of them is compiled into the `oac` binary. No package is copyleft.
`core/` parses JSON with its own strict I-JSON reader (`core/src/json.rs`), not with
`serde_json`: the envelope rules need duplicate member names reported (RFC 7493 §2.3) and
each number's spelling kept ([SC-ENV-050], [SEC-SIG-013]), which `serde_json`'s value type
discards.

**Acceptance box 2 ticked here** — "Every third-party dependency: name, version,
license, reason": every row above carries all four, plus the copyleft flag, the
Apache-2.0 compatibility verdict, the source decision citation, and the consuming
module — the last of which is this file's own addition over C1's table.

### Accepted licenses

(Dated note, 2026-10-05, #50 / PR #311 review N3.) A dependency of the `oac` binary
passes `scripts/check-licenses.mjs` only if its SPDX license expression has an OR-arm made
only of licenses on this list. The list is exactly the licenses the table above already
records as acceptable, plus `Unicode-3.0` (dated note below the list):

| SPDX identifier | Recorded in the table above by |
|---|---|
| `Apache-2.0` | `rmcp`, the three Codex app-server crates, and the elected arm of every dual |
| `MIT` | the MIT arm of `keyring`, `keyring-core`, `windows-native-keyring-store`, `age`; `memchr` (Unlicense OR MIT, MIT elected) and `zmij` (MIT), transitive packages of `serde_jcs` (dated note above) |
| `0BSD` | the 0BSD arm of `interprocess` |
| `Unicode-3.0` | `unicode-ident` `1.0.26`, whose expression is (MIT OR Apache-2.0) AND Unicode-3.0; operator decision https://github.com/RossGraeber/OAC/issues/51#issuecomment-6009697192 (dated note below) |
| `BSD-3-Clause` | `ed25519-dalek` `3.0.0` (C5 §2), and `curve25519-dalek`, `x25519-dalek` and `subtle` beneath it and `age`; same operator decision (dated note below) |

(Dated note, 2026-10-06, #51 / F2. Approved by the operator:
https://github.com/RossGraeber/OAC/issues/51#issuecomment-6009697192, "Unicode-3.0 is
acceptable"; permissive licenses on par with MIT are acceptable, and GPL, LGPL, AGPL and
other copyleft-only licenses are not. Only the licenses a dependency actually needs are
added to this list, so this PR adds Unicode-3.0 alone.) `Unicode-3.0` is added
for `unicode-ident`, the Unicode identifier tables under `proc-macro2` and `syn`. It
reaches the `--all-features` graph through `serde`'s optional `derive` feature, by way of
`serde_jcs` (C5 §3, `PINS.md`); nothing in the workspace enables that feature, so
`unicode-ident` is not compiled into the `oac` binary today. The Unicode License v3 is a
permissive, OSI-approved license with no copyleft term; it asks that its notice travel with
copies of the Unicode data, which I2's NOTICE work covers. `unicode-ident`'s other licenses
are MIT OR Apache-2.0, of which OAC elects Apache-2.0, so the elected form is
`Apache-2.0 AND Unicode-3.0`.

(Dated note, 2026-10-06, #52 / F3.) `BSD-3-Clause` is added for `ed25519-dalek` `3.0.0`, the
signature crate decision C5 §2 chose and recorded as BSD-3-Clause, "permissive and
Apache-2.0-compatible". It is a permissive, OSI-approved license with no copyleft term, on
par with MIT, so it falls under the operator decision on #51 cited above
(https://github.com/RossGraeber/OAC/issues/51#issuecomment-6009697192: permissive licenses
on par with MIT are acceptable; GPL, LGPL, AGPL and other copyleft-only licenses are not).
It asks that its copyright notice and disclaimer travel with binary redistributions, which
I2's NOTICE work covers. Only this one license is added; `BSD-2-Clause` and `BSD-1-Clause`
appear only as non-elected arms and stay off the list.

`EPL-2.0` is not on the list. `zenoh` passes because its expression offers `Apache-2.0`,
the arm OAC elects (§6). When an expression offers `Apache-2.0`, the script records that
arm as elected (§6; C1 §10). Adding a license to the list is a license-policy change: add
it here in the same PR that adds it to the script's list.

### Dev/test tooling — not shipped (outside this inventory)

The tool below is recorded here only so its license is on file. It is **not** a
dependency of the `oac` binary. It is not part of the table above, not counted in §6's
copyleft flag, and not covered by §7's Apache-2.0 verdict. It is never imported by,
linked into, vendored into, or invoked from `core/`, `spec/`, `adapters/*`,
`transports/zenoh/` or `cli/`. It runs as an external process from test tooling only.
Its facts come from `docs/planning/decisions/K1-herdr-evaluation.md` §2-§3 and
`docs/planning/PINS.md` "herdr (test tooling)". They are cited, not re-derived, and
are an exception to this file's C1/C2/C4-only sourcing rule above.

| Tool | Version | License | Why needed | Shipped? | Source decision | Used from |
|---|---|---|---|---|---|---|
| herdr | `v0.9.1` (commit `065ef9d6a531c49fb8bee7e818ef837065b21ee9`) | Apache-2.0 | Terminal multiplexer that test tooling uses to drive real harness CLI sessions for scripted gate re-runs and opt-in integration runs (Epic K #123) | **No** — dev/test tooling only, never CI-default | K1 §2-§3 (live behavior UNVERIFIED; K1's go/no-go is provisional pending live confirmation) | Test tooling outside product paths only (Epic K driver) |

### External services — not shipped (outside this inventory)

The service below is recorded here only so its license is on file. It is **not** a
dependency of the `oac` binary. It is not part of the §5 table,
not counted in §6's copyleft flag, and not covered by §7's Apache-2.0 verdict. The
operator installs it separately, and each harness connects to it natively over its own
MCP configuration. It is never imported by, linked into, vendored into, spawned from or
configured by `core/`, `spec/`, `adapters/*`, `transports/zenoh/` or `cli/`. Its
facts come from `docs/planning/decisions/L1-beacon-memory.md` §2-§3 and
`docs/planning/PINS.md` "Beacon (external memory service)". They are cited, not
re-derived, and are an exception to this file's C1/C2/C4-only sourcing rule above.

| Tool | Version | License | Why needed | Shipped? | Source decision | Used from |
|---|---|---|---|---|---|---|
| Beacon (agent-beacon) | `v1.3.29` (commit `91e92216b79108475ba9b587d49c5ff3f7356fd8`) | MIT | Optional external memory service a harness may use beside OAC sessions (Epic L #165). OAC does not need it and does not use it | **No** — external service each harness connects to natively; OAC never imports, links, vendors, spawns or configures it | L1 §2-§3 (one fact UNVERIFIED after L2, L1 §6 and §11) | Nowhere in OAC. Harness MCP configuration only, owned by the harness and the operator |

---

## 6. Copyleft flag

`zenoh` `1.10.1` is dual **EPL-2.0 / Apache-2.0**. EPL-2.0 is weak copyleft. **OAC
elects the Apache-2.0 arm**, per `docs/planning/decisions/C1-language-runtime.md` §7.

No other row in §5's inventory carries a copyleft arm. The same election pattern
applies to the MIT-or-Apache duals (`keyring`, `keyring-core`,
`windows-native-keyring-store`, `age`, `serde_jcs`) and the 0BSD-or-Apache dual (`interprocess`):
these are permissive either way, and OAC elects Apache-2.0 for uniformity across the
whole inventory, not because either arm of those duals is copyleft.

**Acceptance box 3 ticked here** — "Anything copyleft is flagged explicitly": `zenoh`
is the only flagged row (§5's table, this section).

---

## 7. Apache-2.0 compatibility verdict for the whole inventory

After the elections in §6, every direct dependency in §5's table resolves to
Apache-2.0 or an Apache-2.0-compatible permissive license. The whole direct inventory
is distributable under Apache-2.0.

**Limit, stated honestly.** This verdict covers **direct named dependencies only** —
the rows in §5's table. It does not cover the transitive dependency graph each of
these crates pulls in; that sweep is deferred (§8).

**Acceptance box 4 ticked here** — "Apache-2.0 compatibility stated for the whole
inventory": this section's stated conclusion, immediately above.

---

## 8. Transitive sweep deferral

`cargo deny` / `cargo license` over the resolved dependency graph is **not run** and is
deferred to Stage 6 (`oac-release` license inventory), because no workspace or
`Cargo.lock` exists yet — `docs/planning/STATUS.md`, "Pre-Stage 0."
(Dated note, 2026-10-05, #50: the workspace and `Cargo.lock` now exist, with no third-party
dependency yet. `scripts/check-licenses.mjs` lists every resolved package with its declared
license, including optional dependencies (`--all-features`), and fails one with no OR-arm
on §5's "Accepted licenses" list. It is not the Stage 6 sweep: it reads each
crate's declared license field only, and NOTICE stays with I2.)

**Packaging consequence, carried from C1 §6.** The three Codex git-dep crates
(`codex-app-server-client`, `codex-app-server-protocol`, `codex-app-server-transport`)
must be swept from the vendored git tree directly, not from a crates.io-only sweep: a
crates.io-only sweep would find no entry at all for `codex-app-server-client` and
`codex-app-server-transport`, and would find the wrong, stale `0.63.0` metadata for
`codex-app-server-protocol` (C1 §6, §9).

Cross-reference `docs/planning/v0.1/12-deferred.md` (not yet landed) as the ledger for
this deferral once it exists.

---

## 9. Surface labels and UNVERIFIED carry-forward

Per `oac-evidence` §4/§5, one label per surface this file names:

| Surface | Label | Note |
|---|---|---|
| `rmcp` | supported | Official SDK, `modelcontextprotocol` org (C1 §13) |
| Codex app-server client/protocol/transport crates | experimental (per-method gating) | Inherits the Codex app-server surface label recorded in `docs/planning/PINS.md` (C1 §13) |
| `zenoh` | supported | Already labelled in `docs/planning/PINS.md` (C1 §13) |
| `keyring` | supported | General-purpose, actively maintained OS-credential crate (C1 §13, C2 §10) |
| `interprocess` | supported | General-purpose, actively maintained; not a preview/experimental provider surface (C2 §10) |
| `age` | supported | Actively maintained reference implementation of a published format (C4-session-identity.md line 677) |
| herdr (dev/test tooling, not shipped — §5 "Dev/test tooling") | supported | herdr's own documented CLI. Live behavior at `v0.9.1` is UNVERIFIED (`docs/planning/decisions/K1-herdr-evaluation.md` §9) |
| Beacon (external service, not shipped — §5 "External services") | supported | Beacon's own documented local MCP server and `beacon memory` CLI at `v1.3.29`, reached by harnesses only. One fact UNVERIFIED after L2's desk research; three closed (`docs/planning/decisions/L1-beacon-memory.md` §6, §7, §11) |

**Open UNVERIFIED items carried forward, not re-opened.** Per `docs/planning/
STATUS.md`, the ledger of record:

- The named compatibility shim boundary for the Claude Code Channels preview surface
  (UNVERIFIED — `DESIGN.md` names no such module; register entry C11,
  `docs/planning/v0.1/03-decisions-and-amendments.md` line 755).
- The named compatibility shim boundary for the Codex experimental live-inject surface
  (UNVERIFIED — same reason; register entry C11).
- Zenoh's default TLS stack being `rustls` rather than OpenSSL (UNVERIFIED — carried
  from PLANNING-PROMPT.md §3.4 unchanged; C1 §9, §13).
- Whether the Windows `windows-native-keyring-store` backend has been exercised
  end-to-end against live Windows Credential Manager (UNVERIFIED — declared
  feature/build target verified only; C1 §8, §12, §13).
- Whether `interprocess` `2.4.4` (or an alternative IPC crate) exposes a first-party
  peer-credential accessor — the final IPC crate choice is a Stage 3 detail (UNVERIFIED
  — not surfaced in the fetched crate docs; C2 §4, §10).

`docs/planning/STATUS.md` is the ledger of record for all three; `docs/planning/v0.1/
11-risks.md` (not yet landed) is their future second home, per `oac-evidence` §5.

---

## 10. Acceptance boxes, ticked against lines in this file

- [x] Every module has one named responsibility and a stated ownership boundary — §2.
- [x] Every third-party dependency: name, version, license, reason — §5.
- [x] Anything copyleft is flagged explicitly — §6.
- [x] Apache-2.0 compatibility stated for the whole inventory — §7.

---

## 11. Cross-reference block

Every reference below is a repo-relative path; no prior context is assumed.

- `docs/planning/ADR-001.md`
- `docs/planning/DESIGN.md`
- `docs/planning/PLANNING-PROMPT.md` §5.12, §9 item 8
- `docs/planning/decisions/C1-language-runtime.md`
- `docs/planning/decisions/C2-process-model.md`
- `docs/planning/decisions/C4-session-identity.md`
- `docs/planning/decisions/C7-zenoh-transport.md`
- `docs/planning/v0.1/03-decisions-and-amendments.md`
- `docs/planning/v0.1/04-architecture.md`
- `docs/planning/v0.1/05-interfaces.md` (§13-§15 superseded by `spec/interfaces.md`)
- `spec/interfaces.md` (frozen: Table 5.2, Table 6.4, [IFC-ADP-001])
- `docs/planning/v0.1/06-security.md`
- `docs/planning/v0.1/09-test-strategy.md` (A10, not yet landed)
- `docs/planning/v0.1/12-deferred.md` (not yet landed)
- `docs/planning/STATUS.md`
- `docs/planning/PINS.md`

---

## 12. Evidence pass

Per `oac-evidence` §8, checked against this file:

- Every crate/version/license/retrieval-date fact in §5-§9 is cited to
  `docs/planning/decisions/C1-language-runtime.md` §3/§6/§7/§8/§10,
  `docs/planning/decisions/C2-process-model.md` §4, or `docs/planning/decisions/
  C4-session-identity.md` §11 — none re-fetched, none given a new retrieval date.
- Every provider/dependency surface named is labelled — §9.
- No new UNVERIFIED item is introduced by this file; the five items §9 carries forward
  already appear in `docs/planning/STATUS.md`'s "Open UNVERIFIED items" list, including
  the two compatibility-shim-boundary items (C11) already carried at §4(b).

---

## 13. Boundary pass

Per `oac-boundaries`' pre-commit self-check:

- No neutral `core/` or `spec/` row in §2/§3 mentions Zenoh, Claude, Codex, or MCP
  method names — §2's `core/` and `spec/` rows name only neutral types and the spec
  text itself; §3's diagram uses module paths only.
- Zenoh vocabulary (Zenoh, `zid`, key expression, liveliness) is confined to §4(a)'s
  labelled Zenoh-containment subsection and §5/§6's `zenoh` inventory rows — nowhere
  else in this file.
- Claude/Codex naming is confined to §4(b)'s labelled compatibility-shim subsection and
  §5's Codex crate rows — nowhere else.
- House naming resolution applied throughout: "Open Agent Channel (OAC)", "OAC Session
  Channels", `oac` binary; no "Session Channels" or `sessionchannels` spelling appears
  outside the §1 verbatim quote (marked as such).
- No code blocks appear except §1's repository-tree sketch and §3's dependency diagram.
  §4(a) points at `spec/interfaces.md` Table 6.4 for the operation names rather than
  copying them.
- Every cross-reference is a repo-relative path (§11).
- No dependency fact is re-derived or given a new retrieval date not already present in
  C1/C2/C4.

---

## 14. Cross-file updates in this change

- `docs/planning/STATUS.md`: "Last updated" bullet and "Open epics" row updated to
  record A8 (issue #28) landing, matching the precedent set by A7 (`cf24ee6`). No
  UNVERIFIED-item wording changed — the two compatibility-shim-boundary items (C11) and
  the three carried §9 items already appear there; §9 of this file restates rather than
  resolves them.
- No ADR-001 amendment is made by this file. §2's resolved layout does not contradict
  `docs/planning/DESIGN.md`'s sketch beyond the daemon note (§1) already explained by
  `docs/planning/v0.1/04-architecture.md` §2 — no new conflict is recorded in
  `docs/planning/v0.1/03-decisions-and-amendments.md`.
