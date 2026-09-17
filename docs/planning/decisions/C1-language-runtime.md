# C1: Language, runtime, packaging, and dependency inventory

**Issue:** #13 (Epic C, backlog key `C1`). **Depends on:** #12. **Source:**
PLANNING-PROMPT.md §5.1, §5.12.

**Status:** Decided.

**Standalone-ledger note.** This file lives at `docs/planning/decisions/` because
`docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4) does not exist yet.
It folds into that file, unedited in substance, once A4 lands — mirroring
`docs/planning/ADR-001-AMENDMENTS.md`'s own standalone-ledger rationale (see that file's
opening paragraph). `docs/planning/STATUS.md` carries a one-line pointer to this file
until then.

---

## 1. Choice

OAC's reference implementation is written in **Rust**, built as a single OAC binary
(`oac`) per host platform, using the pinned toolchain `1.98.1`
(`docs/planning/PINS.md` — Rust toolchain; `rust-toolchain.toml` enforces it).

This is stated as the choice, not offered as one option among several: the language and
runtime for OAC's reference implementation is Rust.

## 2. The ADR-001 packaging claim, corrected before restating it

PLANNING-PROMPT.md §5 decision 1 and the issue #13 "Presumptive answer" both say
"single-binary cross-platform packaging is required by ADR-001." That overstates
`ADR-001.md`. The ADR's actual text (line 21) states:

```
The reference implementation is a CLI and must not require Docker, Kubernetes, a cloud
account, or a separately administered server for normal local use.
```

`ADR-001.md` never uses the words "static" or "single binary." Per `oac-evidence` §6,
this is recorded as a conflict rather than silently restated: **route (a)** is taken —
the requirement is restated verbatim as written above, and "single self-contained
binary per platform" is presented here as the **chosen means** of satisfying it, not as
a separate ADR-001 requirement. No ADR-001 amendment is needed: nothing in `ADR-001.md`
is wrong or contradicted, so this is not the kind of "evidence invalidates a settled
decision" case `oac-evidence` §6 amends for — the presumptive answer in
PLANNING-PROMPT.md/issue #13 overstated the ADR's own wording, and this section is the
correction. (If a future decision needs the stronger "single static binary" form stated
as a project requirement rather than a means, the next free amendment number is
`ADR-001-A4`, above `ADR-001-A3` — see `docs/planning/ADR-001-AMENDMENTS.md`. Not needed
here.)

Why a single self-contained binary is still the chosen means: it is the simplest way to
satisfy "must not require a separately administered server for normal local use" — one
executable a user downloads and runs, no daemon-as-a-service, no container runtime, no
package-manager dependency chain to install first. See §7 for what "self-contained"
actually resolves to once the real dependency set is checked (not fully static on every
target).

## 3. Rust MCP SDK: pin and evidence

**Crate:** `rmcp`. **Repository:** `modelcontextprotocol/rust-sdk` (first-party — the
official Rust SDK for the Model Context Protocol).

- Pinned version: `rmcp` **`3.4.0`**. Source:
  https://crates.io/crates/rmcp, latest stable version `3.4.0`, retrieved 2026-09-17.
- Release date: **2026-09-15**. Source:
  https://github.com/modelcontextprotocol/rust-sdk/releases, tag `rmcp-v3.4.0`,
  retrieved 2026-09-17.
- License: **Apache-2.0**. Source:
  https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/rmcp-v3.4.0/Cargo.toml,
  `[workspace.package]` `license = "Apache-2.0"`, `version = "3.4.0"` (the `rmcp` crate's
  own `Cargo.toml` inherits both via `{ workspace = true }`), retrieved 2026-09-17.
- Surface label: **supported** — `rmcp` is the official SDK published under the
  `modelcontextprotocol` GitHub organization, not a third-party wrapper.

This satisfies the acceptance-checkbox instruction to verify against the SDK repo,
releases, `Cargo.toml`, and the crates.io page — no blog or aggregator was used as a
deciding source; WebSearch snippets were used only to locate file paths, never to state
a fact without a first-party fetch backing it.

## 4. Legacy-revision support at the pinned version (load-bearing acceptance item)

Quoted verbatim from the pinned tag's own source (Source:
https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/rmcp-v3.4.0/crates/rmcp/src/model.rs,
retrieved 2026-09-17):

```rust
#[derive(Debug, Clone, Eq, PartialEq, Hash, PartialOrd)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ProtocolVersion(Cow<'static, str>);

impl ProtocolVersion {
    pub const V_2026_07_28: Self = Self(Cow::Borrowed("2026-07-28"));
    pub const V_2025_11_25: Self = Self(Cow::Borrowed("2025-11-25"));
    pub const V_2025_06_18: Self = Self(Cow::Borrowed("2025-06-18"));
    pub const V_2025_03_26: Self = Self(Cow::Borrowed("2025-03-26"));
    pub const V_2024_11_05: Self = Self(Cow::Borrowed("2024-11-05"));
    pub const LATEST: Self = Self::V_2025_11_25;
```

`V_2025_11_25` is a declared constant at the pinned tag `rmcp-v3.4.0`, matching OAC's
pinned legacy MCP revision `2025-11-25` (`docs/planning/PINS.md` — MCP revisions)
exactly. Note also, as a secondary point (not required by the acceptance box, but
relevant to §5): `LATEST` at this tag equals `V_2025_11_25`, not `V_2026_07_28` — the
legacy revision is still the SDK's own default, not merely an accepted fallback.

**The reversal condition's first half — "a disqualifying gap in the Rust MCP SDK's
legacy-revision support" — has not fired.** `2025-11-25` is declared and, per §5 below,
negotiable as a server.

Corroborating changelog evidence (Source:
https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/rmcp-v3.4.0/crates/rmcp/CHANGELOG.md,
retrieved 2026-09-17): `rmcp` `1.5.0` — "add 2025-11-25 protocol version support";
`2.1.0` — "negotiate protocol version in handler"; `3.1.0` — "honor
supported_protocol_versions when negotiating initialize"; `3.0.0` (2026-07-28) —
"recognize 2026 MCP methods." Read together: `2025-11-25` support was added in `1.5.0`
and is still present, unremoved, at the pinned `3.4.0` (confirmed directly by the
`model.rs` constant above, not merely inferred from the changelog).

## 5. Cross-check against the Claude Channels floor in PINS.md

`docs/planning/PINS.md` records, verbatim from Claude Code's own docs: "if you set
`MCP_PROTOCOL_NEGOTIATION` to `auto` and a channel server negotiates MCP protocol
revision 2026-07-28, it can't deliver channel messages, so Claude Code doesn't register
it as a channel." The floor is `MCP_PROTOCOL_NEGOTIATION=legacy`. G4's dual-era design
(`oac-mcp`, gate `G4-mcp-dual-era`) depends on a server, not just a client, being able to
negotiate `2025-11-25`.

`rmcp` supports this as a **server**, not only as a parser of the string: the
`CHANGELOG.md` entries `2.1.0` ("negotiate protocol version in handler") and `3.1.0`
("honor `supported_protocol_versions` when negotiating initialize") describe
server-side initialize-time negotiation, and `ProtocolVersion::LATEST` defaulting to
`V_2025_11_25` at the pinned tag (§4) means an `rmcp`-based server's default advertised
revision is already the legacy one Claude Code requires — no extra negotiation
configuration is needed to default into the compatible revision. **Explicit
statement:** the SDK can be made to negotiate legacy MCP as a server at the pinned
version; this is not merely "the SDK can parse the revision string."

One item is carried as UNVERIFIED rather than asserted: whether an OAC server built on
`rmcp` `3.4.0`, run end-to-end against a live Claude Code instance with
`MCP_PROTOCOL_NEGOTIATION=legacy`, actually registers as a channel is a runtime question
resolved only by gate `G4`, not by this document (`G4` verdict: `NOT RUN`, per
`docs/planning/gates/G4-result.md`). This document verifies the SDK *capability*; `G4`
verifies the *running system*.

## 6. Codex client crates — verified, not recalled from memory

Verified at commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` (the pinned Codex commit,
`docs/planning/PINS.md` — Codex CLI and app-server), workspace root
`https://raw.githubusercontent.com/openai/codex/6b9826e3aa83b1a5947db50f4332cb9c65f1b340/codex-rs/Cargo.toml`,
retrieved 2026-09-17:

- Directory entries confirmed present at this commit
  (https://github.com/openai/codex/tree/6b9826e3aa83b1a5947db50f4332cb9c65f1b340/codex-rs,
  retrieved 2026-09-17): `app-server-client`, `app-server-protocol`,
  `app-server-transport`.
- Workspace `[workspace.package]`: `license = "Apache-2.0"`, `version = "0.154.0"` —
  matches the pinned Codex version in `PINS.md` exactly, no drift.
- Package names (confirmed via each crate's `Cargo.toml`, e.g.
  https://raw.githubusercontent.com/openai/codex/6b9826e3aa83b1a5947db50f4332cb9c65f1b340/codex-rs/app-server-protocol/Cargo.toml,
  retrieved 2026-09-17): `codex-app-server-client`, `codex-app-server-protocol`,
  `codex-app-server-transport`. Each inherits `version`/`license` from the workspace via
  `{ workspace = true }` — same Apache-2.0 license as the workspace.
- **crates.io publication check:** `https://crates.io/api/v1/crates/app-server-protocol`
  returns HTTP 404 (retrieved 2026-09-17) — **not published to crates.io.** These are
  **workspace members consumed only as a git dependency**
  (`{ git = "https://github.com/openai/codex", rev = "6b9826e..." }` in `Cargo.toml`),
  not a registry package.

**Packaging consequence, stated explicitly (acceptance requirement):** a git dependency
pinned by commit SHA is reproducible (the exact `rev` is fixed, same as a registry
pin) but has different consequences than a crates.io dependency for a static/self-
contained binary build: (a) `cargo vendor`/offline builds must vendor the git tree, not
just a registry tarball; (b) `Cargo.lock` records the git commit, so CI must have network
access to `github.com` (or a vendored mirror) at first build/lockfile-refresh time, not
just to `crates.io`; (c) license/version metadata for these three crates will not appear
in a `crates.io`-only license-inventory sweep (relevant to §9's transitive-license
method) — they must be swept from the vendored git source directly. None of this blocks
the choice; it is recorded because the acceptance box requires stating which packaging
path applies.

## 7. Zenoh: license election

`docs/planning/PINS.md` already pins `zenoh` `1.10.1`, dual **EPL-2.0 / Apache-2.0**.
EPL-2.0 is weak copyleft — **flagged**. OAC elects the **Apache-2.0 arm** of the dual
license for its own use of `zenoh`. This election is what makes an "Apache-2.0
compatible" verdict on the `zenoh` dependency true: EPL-2.0 alone would need its own
compatibility analysis (weak copyleft, file-level, generally compatible with combined
works but not identical to Apache-2.0's permissions); by exercising the Apache-2.0 option
the dual license offers, OAC's dependency tree stays under one permissive license family
end to end for this crate.

## 8. Credential-store crate

**Crate:** `keyring`. **Repository:** `open-source-cooperative/keyring-rs` (the crate's
current maintaining organization).

- Pinned version: **`4.2.0`**. Source: https://crates.io/crates/keyring, latest stable
  version `4.2.0`, retrieved 2026-09-17.
- Release date: **2026-08-29**. Source: crates.io publish metadata for `keyring`
  `4.2.0`, retrieved 2026-09-17.
- License: **MIT OR Apache-2.0**. Source:
  https://raw.githubusercontent.com/hwchen/keyring-rs/master/Cargo.toml,
  `license = "MIT OR Apache-2.0"`, retrieved 2026-09-17.
- Per-platform backend support, confirmed from the crate's own `Cargo.toml` feature
  flags (same source, retrieved 2026-09-17):
  - Windows: `windows-native-keyring-store` — Windows Credential Manager.
  - macOS: `apple-native-keyring-store` — macOS/iOS Keychain.
  - Linux: `zbus-secret-service-keyring-store` / `dbus-secret-service-keyring-store` —
    Secret Service over D-Bus (two client implementations of the same backend); also
    `linux-keyutils-keyring-store` — the Linux kernel keyutils backend, which does not
    talk to D-Bus at all.
  - The `v1` feature (default-adjacent, cross-platform) bundles Apple, Windows-native,
    and zbus Secret Service support together.

**This closes the second half of the reversal condition's falsifiable test for
Windows:** Windows Credential Manager access is a named, verified backend
(`windows-native-keyring-store`) at the pinned version, not assumed from memory. The
reversal condition's Windows half has not fired.

**Boundary note (ADR-001 watch item, per the task breakdown).** The credential store
`keyring` accesses is for **OAC's own device keys only** — the identity hierarchy
`Security principal -> Device -> Harness -> Session` in `ADR-001.md`'s Security model.
OAC MUST NOT read a credential-store entry that Codex or Claude Code owns (e.g. Codex's
`CODEX_HOME/auth.json`/OS-keyring login entry, or any Claude Code OAuth token). Doing so
would hit `[ADR-001 Boundary]` "MUST NOT steal or reuse another harness's provider
credentials." `keyring` is a generic OS-secret-store client; OAC's own code is what
scopes which service/account name it reads and writes, and that scoping must stay
confined to OAC-issued device keys, never a provider's own credential entry.

## 9. Static-binary claim, verified against the real dependency set

Per the ADR-001 text quoted in §2, what actually matters is "must not require ... a
separately administered server for normal local use" — not bit-for-bit static linkage.
Checked per target:

- **Linux — Secret Service backends (`zbus-secret-service-keyring-store` /
  `dbus-secret-service-keyring-store`):** `zbus` is a pure-Rust reimplementation of the
  D-Bus wire protocol (no `libdbus` C library to link), so this does not force dynamic
  linking against a system D-Bus library. It does, however, require a **running D-Bus
  session bus and a Secret Service provider (e.g. `gnome-keyring`, `kwallet`)** to be
  present at runtime — a system service dependency, not a linking dependency, but a real
  one on headless/minimal Linux hosts. The `linux-keyutils-keyring-store` backend avoids
  this entirely (kernel `keyctl` syscalls only, no D-Bus, no daemon) and is the better
  default for `x86_64-unknown-linux-musl`/headless targets; this is recorded as an open
  implementation choice, not resolved here (§10, Stage 3/`oac-implementation`).
- **macOS — `apple-native-keyring-store`:** links against `Security.framework`, a system
  framework. macOS does not support fully static binaries at all (there is no static
  `libSystem`); this is normal and unavoidable for any macOS binary, not specific to
  `keyring`.
- **Windows — `windows-native-keyring-store`:** links against the Windows Credential
  Manager API (via system DLLs present on every supported Windows version) — again
  normal for any Windows binary.
- **`zenoh` `1.10.1`:** the reference transport is pure Rust; TLS is provided by
  `rustls` rather than OpenSSL in Zenoh's default configuration (per
  PLANNING-PROMPT.md §3.4, carried unchanged — not independently re-verified in this
  document; flagged UNVERIFIED below), so Zenoh itself is not expected to force a
  dynamic OpenSSL/libcrypto dependency.
- **`x86_64-unknown-linux-musl`:** achievable for the Rust code itself (no C-library
  linkage required by `rmcp`, the Codex crates, or Zenoh's default TLS stack), but
  musl-vs-glibc linkage is orthogonal to the Secret Service **runtime** dependency
  above — a musl-linked binary using the D-Bus Secret Service backend still needs a
  running Secret Service provider on the host. Choosing
  `linux-keyutils-keyring-store` removes that runtime dependency independent of the
  libc choice.

**Stated conclusion, replacing the "static binary" claim per the task instruction:**
OAC ships a **single self-contained binary per platform, dynamically linked against OS
system libraries only** (`Security.framework` on macOS, Windows Credential Manager DLLs
on Windows) — not a bit-for-bit static binary on every target, and on Linux the default
Secret Service credential backend adds a runtime dependency on a running Secret Service
provider unless the `linux-keyutils-keyring-store` backend is selected instead. This
satisfies ADR-001's actual requirement (§2) — no Docker/Kubernetes/cloud
account/separately-administered server is needed for normal local use — without
overclaiming "static."

`Zenoh's default TLS stack being rustls rather than OpenSSL` is **UNVERIFIED — carried
from PLANNING-PROMPT.md §3.4 unchanged; not independently re-fetched from Zenoh's own
`Cargo.toml`/feature-flag docs in this document; added to STATUS.md's Open UNVERIFIED
items (§13 below).**

## 10. Dependency inventory

Transitive-license method: **not run in this document.** `cargo deny` or `cargo license`
against the full resolved dependency graph is deferred to Stage 6
(`oac-release` license inventory) — no `Cargo.lock` exists yet (no code has landed; see
`docs/planning/STATUS.md`, "Pre-Stage 0"). The table below covers only the direct,
named dependencies this decision selects.

| Crate | Version | License | Why needed | Copyleft? | Apache-2.0 compatible? |
|---|---|---|---|---|---|
| `rmcp` | `3.4.0` | Apache-2.0 | Rust MCP SDK — server/client protocol implementation for the OAC Session Channels MCP packaging layer (§3) | No | Yes — same license |
| `codex-app-server-client` (dir `app-server-client`) | `0.154.0` @ commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` (git dep, not on crates.io) | Apache-2.0 | Codex app-server JSON-RPC client — Codex adapter transport | No | Yes — same license |
| `codex-app-server-protocol` (dir `app-server-protocol`) | `0.154.0` @ same commit (git dep) | Apache-2.0 | Codex app-server request/response/schema types | No | Yes — same license |
| `codex-app-server-transport` (dir `app-server-transport`) | `0.154.0` @ same commit (git dep) | Apache-2.0 | Codex app-server transport framing | No | Yes — same license |
| `zenoh` | `1.10.1` | EPL-2.0 / Apache-2.0 (dual) | Reference peer-to-peer transport plugin (§Zenoh transport, ADR-001) | **Yes — EPL-2.0 is the other arm of the dual license; flagged** | Yes — **OAC elects the Apache-2.0 arm** (§7); the EPL-2.0 arm is not used |
| `keyring` | `4.2.0` | MIT OR Apache-2.0 | OS-native credential store for OAC's own device keys (Windows Credential Manager / macOS Keychain / Linux Secret Service or keyutils) — identity hierarchy in ADR-001's Security model | No | Yes — OAC elects the Apache-2.0 arm |

Full transitive sweep (`cargo deny` or `cargo license` over the resolved dependency
graph, including everything `rmcp`, the Codex crates, `zenoh`, and `keyring` pull in
transitively) is **deferred to Stage 6** (`oac-release` license inventory), recorded
here per the acceptance requirement rather than run now, because no workspace/lockfile
exists yet to sweep.

## 11. Rejected alternatives

- **Go.** A Zenoh binding exists (`zenoh-go`), but no first-party Codex app-server
  client crates exist for Go, and there is no first-party Rust MCP SDK to reuse (Go
  would need its own MCP SDK evaluation from scratch).
- **Python.** A native Zenoh binding exists (`zenoh-python`), but Python offers no
  single-binary/self-contained cross-platform packaging story comparable to a compiled
  Rust binary — deployment would require a Python runtime and dependency environment on
  the target host, working against ADR-001's "no separately administered server /
  simple local use" requirement.
- **TypeScript/Node.** The most mature first-party MCP SDK ecosystem, but no native
  Zenoh peer binding and no single-binary packaging story (Node deployment needs either
  a Node runtime on the host or a bundler/packager step that does not produce a true
  single self-contained binary the way a compiled Rust target does).
- **C++/JVM.** A Zenoh binding exists for both, but neither has first-party Codex
  app-server client crates (`app-server-client`/`app-server-protocol`/
  `app-server-transport` are Rust-only), and the JVM breaks the single-binary
  requirement outright (a JVM runtime dependency is exactly the kind of "separately
  administered" runtime prerequisite ADR-001 rules out for normal local use).

## 12. Reversal condition

Stated verbatim as issue #13 writes it: **"a disqualifying gap in the Rust MCP SDK's
legacy-revision support, or in Windows credential-store access."**

Falsifiable tests for each half:

- **Rust MCP SDK legacy-revision half.** Test: does
  `crates/rmcp/src/model.rs` at the currently pinned `rmcp` tag declare a
  `ProtocolVersion` constant equal to the string `"2025-11-25"`, **and** does the
  pinned `CHANGELOG.md` show that support was never removed after being added? At
  `rmcp-v3.4.0` today: **test passes** — `V_2025_11_25` is declared (§4) and `LATEST`
  itself resolves to it; no removal is recorded in the changelog between `1.5.0` (added)
  and `3.4.0` (current). A future re-pin must re-run this exact test (`oac-evidence` §7,
  triggered by any pin move in `docs/planning/PINS.md`).
- **Windows credential-store half.** Test: does the pinned `keyring` crate's
  `Cargo.toml` declare a `windows-native-keyring-store` feature (or equivalent
  Windows-Credential-Manager-backed feature) that builds and links on the
  `x86_64-pc-windows-msvc` target? At `keyring` `4.2.0` today: **test passes** —
  `windows-native-keyring-store` is declared (§8). This document does not additionally
  claim the feature was exercised end-to-end against live Windows Credential Manager;
  that runtime confirmation belongs to a future implementation/testing task
  (`oac-implementation`/`oac-testing`), not to this decision.

Neither half has fired. The presumptive answer stands.

## 13. Surface labels and open UNVERIFIED items

Per `oac-evidence` §4, every provider/dependency surface touched gets one label:

| Surface | Label | Note |
|---|---|---|
| `rmcp` (Rust MCP SDK) | supported | official SDK, `modelcontextprotocol` org |
| Codex app-server client/protocol/transport crates | experimental (per-method gating) | inherits the Codex app-server surface label already recorded in `PINS.md` |
| `zenoh` | supported | already labelled in `PINS.md` |
| `keyring` | supported | general-purpose, actively maintained OS-credential crate; not a preview/experimental provider surface |
| MCP legacy (`2025-11-25`) / current (`2026-07-28`) | supported (both) | already labelled in `PINS.md` |

New UNVERIFIED items from this document, added to `docs/planning/STATUS.md`'s "Open
UNVERIFIED items" list (§9 above has the reasons in full; this row is the ledger
entry):

- Zenoh's default TLS stack being `rustls` rather than OpenSSL (UNVERIFIED — carried
  from PLANNING-PROMPT.md §3.4 unchanged; not independently re-fetched from Zenoh's own
  `Cargo.toml`/feature docs in this document).
- Whether an `rmcp`-based OAC server, run end-to-end against a live Claude Code
  instance with `MCP_PROTOCOL_NEGOTIATION=legacy`, actually registers as a channel
  (UNVERIFIED — this document verifies SDK capability only; runtime confirmation is
  gate `G4`'s job, verdict currently `NOT RUN`).
- Whether the Windows `windows-native-keyring-store` backend has been exercised
  end-to-end against live Windows Credential Manager (UNVERIFIED — this document
  verifies the declared feature/build target only; runtime confirmation belongs to a
  future `oac-implementation`/`oac-testing` task).

## 14. Acceptance boxes, ticked against lines in this file

- [x] Choice stated, not offered as alternatives — §1.
- [x] Rejected alternatives in one line each — §11.
- [x] Reversal condition stated — §12, verbatim quote plus two falsifiable tests.
- [x] Rust MCP SDK legacy-revision (`2025-11-25` or earlier) support verified against a
      pinned version — §3 (pin) and §4 (verbatim source quote at that pin).
- [x] Every dependency listed with license; copyleft flagged; Apache-2.0 compatibility
      stated — §10 table (EPL-2.0 flagged on the `zenoh` row; every row's Apache-2.0
      compatibility verdict stated).

## Where this folds in

Once `docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4) exists, this
file's content moves there unedited in substance (per PLANNING-PROMPT.md §9's output
package shape) and this file becomes a redirect stub, mirroring how
`ADR-001-AMENDMENTS.md` already describes its own eventual fold-in.
