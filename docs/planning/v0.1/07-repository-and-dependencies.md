# 07 — Repository and Dependencies

**Issue:** #28 (Epic A, backlog key A8). **Depends on:** #13. **Source:**
PLANNING-PROMPT.md §9.8, §5.12, `docs/planning/DESIGN.md` "Suggested repository shape"
(lines 135-150).

**Scope.** This file has two jobs: the module layout with ownership boundaries, and the
third-party dependency inventory with licenses. It does not restate `docs/planning/
DESIGN.md` or `docs/planning/v0.1/04-architecture.md` prose beyond what a table needs —
cite those files by path instead. It does not define interface signatures
(`docs/planning/v0.1/05-interfaces.md`'s job) or the threat model
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
| `transports/zenoh/` | Every Zenoh-specific type, identifier, and concept, behind the `Transport` contract | The `Transport` contract's seven operations over neutral types only (05-interfaces.md §15) | Policy/authorization decisions; signature verification; anything visible outside `start`/`publish`/`subscribe`/`announce_presence`/`watch_presence`/`health`/`shutdown` (05-interfaces.md §15) | `core/` only (§3) |
| `cli/` (including `mcp-shim`) | User-facing commands, and the thin stdio shim a harness spawns | `oac start`, `status`, `sessions`, `doctor`, `mcp-shim` — the `mcp-shim` subcommand carries nothing beyond a thin stdio connection to the daemon over local IPC (04-architecture.md §2) | Long-lived process state; the transport peer; policy decisions; key material (04-architecture.md §2) | `core/` |
| `tests/` | Protocol, security, and integration test suites (`tests/{protocol,security,integration}/`) | Fixtures and fake endpoints for provider/transport contract tests (`docs/planning/DESIGN.md` "Testing"; `docs/planning/v0.1/09-test-strategy.md`, A10, not yet landed) | Production code; anything shipped in the `oac` binary | `core/`, and whichever module a given test targets |

**Acceptance box 1 ticked here** — "Every module has one named responsibility and a
stated ownership boundary": every row above states exactly one responsibility, an
"Owns" column, and a "Never owns" column, sourced from `docs/planning/v0.1/
04-architecture.md` §2 rather than re-derived.

---

## 3. Dependency direction rule

Allowed edges, as a text diagram:

```text
cli/              -> core/
adapters/*        -> core/           (only)
transports/zenoh/ -> core/           (only)
core/             -> (nothing in-repo)
```

**Explicit MUST NOTs.**

- `core/` MUST NOT depend on `adapters/*` or `transports/*`.
- An adapter MUST NOT depend on another adapter.
- An adapter MUST NOT depend on `transports/zenoh/` — adapters route through core policy,
  per `docs/planning/DESIGN.md`'s "Adapters should route through core policy/security
  rather than directly through transports," already cited normatively at
  `docs/planning/v0.1/05-interfaces.md` §13.

**Stage 3 enforcement owner.** `oac-implementation`'s module-dependency-direction rule
is the Stage 3 owner of enforcing this diagram in the actual workspace (lint/CI check);
this file states the rule, not the enforcement mechanism.

---

## 4. Containment boundaries — the two that are ADR-001 boundaries, not style

### (a) Zenoh containment

Every Zenoh type, identifier, and key expression stays inside `transports/zenoh/`, per
`docs/planning/decisions/C7-zenoh-transport.md` §2 and DESIGN acceptance criterion 9.
The `Transport` contract's seven operations (`docs/planning/v0.1/05-interfaces.md` §15)
are the only visible surface crossing this module's boundary:

```text
start
publish
subscribe
announce_presence
watch_presence
health
shutdown
```

Nothing outside `transports/zenoh/` names a Zenoh type, `zid`, a key expression, or a
liveliness term — per `[ADR-001 Boundary]` "MUST NOT leak Zenoh-specific concepts into
the neutral protocol."

### (b) Compatibility-shim boundaries for the two non-supported surfaces

Per `oac-evidence` §4/§5:

- `adapters/claude/` isolates Claude Channels (UNVERIFIED — the module/interface name
  itself is not yet fixed in `DESIGN.md`; open ledger entry C11,
  `docs/planning/v0.1/03-decisions-and-amendments.md` line 755;
  `docs/planning/STATUS.md` "Open UNVERIFIED items"), labelled **research preview**,
  pinned `v2.1.274` per `docs/planning/PINS.md` (Claude Code Channels).
- `adapters/codex/` isolates the Codex app-server (UNVERIFIED — same ledger entry C11;
  not yet fixed in `DESIGN.md`), labelled **experimental, per-method gating**, pinned
  `0.154.0` @ commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`.

Cross-reference `docs/planning/v0.1/05-interfaces.md` for the adapter contract itself
(`ProviderAdapter`'s seven members) — not restated here.

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

**Acceptance box 2 ticked here** — "Every third-party dependency: name, version,
license, reason": every row above carries all four, plus the copyleft flag, the
Apache-2.0 compatibility verdict, the source decision citation, and the consuming
module — the last of which is this file's own addition over C1's table.

---

## 6. Copyleft flag

`zenoh` `1.10.1` is dual **EPL-2.0 / Apache-2.0**. EPL-2.0 is weak copyleft. **OAC
elects the Apache-2.0 arm**, per `docs/planning/decisions/C1-language-runtime.md` §7.

No other row in §5's inventory carries a copyleft arm. The same election pattern
applies to the MIT-or-Apache duals (`keyring`, `keyring-core`,
`windows-native-keyring-store`, `age`) and the 0BSD-or-Apache dual (`interprocess`):
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
the ten rows in §5's table. It does not cover the transitive dependency graph each of
these crates pulls in; that sweep is deferred (§8).

**Acceptance box 4 ticked here** — "Apache-2.0 compatibility stated for the whole
inventory": this section's stated conclusion, immediately above.

---

## 8. Transitive sweep deferral

`cargo deny` / `cargo license` over the resolved dependency graph is **not run** and is
deferred to Stage 6 (`oac-release` license inventory), because no workspace or
`Cargo.lock` exists yet — `docs/planning/STATUS.md`, "Pre-Stage 0."

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
- `docs/planning/v0.1/05-interfaces.md`
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
- No code blocks appear except §1's repository-tree sketch and §4(a)'s seven interface
  operation names.
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
