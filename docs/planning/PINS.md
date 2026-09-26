# OAC pinned external surfaces

A pin is a first-party-observed version string for one external surface OAC depends
on: an exact release version (or, for MCP, an exact revision date-string), the date
that release was published, the first-party URL the pin was observed at, and the date
it was retrieved. A pin is not a preference or a "latest as of planning" note — it is
the version the rest of the plan is written against.

**Changing any row in this file is a trigger event.** Per `oac-evidence` §7, a moved
pin requires re-verifying every §3 fact that depended on it (Epic B2) and, per the
gate re-run policy (`docs/planning/gates/README.md`), re-running
every gate whose verdict depended on it. Do not silently bump a version in this file.

## Pin-move checklist

When any pin in the table below changes (version, release date, or a row's
presence), make all of these edits in the **same commit**:

- [ ] Read the moved row's `Gates affected` cell to find which gate results to
      invalidate.
- [ ] Each affected `docs/planning/gates/G<n>-result.md`: set `**Verdict:**` to
      `NOT RUN`, append the superseded verdict to its `Re-run history` table with
      `Invalidated by: <surface> pin <old> -> <new>, <YYYY-MM-DD>`, and add a
      `> INVALIDATED` callout at the top.
- [ ] `docs/planning/STATUS.md` Gate verdicts row for each affected gate reverts to
      `NOT RUN`.
- [ ] This file's `**Last updated:**` (below) is bumped.
- [ ] If a row was added, removed, or renamed (not just its version or release date
      changed): update `Pin rows relied on` in each affected `G<n>-result.md` and the
      `Pins relied on` cell in `docs/planning/STATUS.md`'s Gate verdicts table to match.

Full policy: `docs/planning/gates/README.md`.

This file is the single source of truth for pinned versions. `docs/planning/STATUS.md`
carries only a summary pointer back here — see its `## Pins` section.

**Last updated:** 2026-09-26 (Codex row changed from a fixed `0.154.0` pin to **floating**,
last observed `0.157.1`, by operator decision. The pin-move checklist was executed in the
same commit: G2 invalidated, and G4 added to the Codex row's gates. See "Floating-version
policy" under the Codex record.) Previously 2026-09-17 (C1: added Rust MCP SDK (`rmcp`) and `keyring` pin rows;
pin-move checklist executed in the same commit, see
`docs/planning/decisions/C1-language-runtime.md`; C2: added `interprocess` (IPC crate,
candidate) pin row, see `docs/planning/decisions/C2-process-model.md`; C4: added `age`
(encrypted-file key-storage fallback) pin row, see
`docs/planning/decisions/C4-session-identity.md`; C5: added `ed25519-dalek` (envelope
signature) and `serde_jcs` (canonical serialization) pin rows, see
`docs/planning/decisions/C5-envelope-auth.md`)

## Pin table

| Surface | Stability label | Pinned version | Release date | Observed at (URL) | Retrieved | Gates affected |
|---|---|---|---|---|---|---|
| Claude Code (Channels) | research preview | `v2.1.274` | 2026-09-17T00:12:02Z (UTC) | https://github.com/anthropics/claude-code/releases/tag/v2.1.274 | 2026-09-16 | G1; G4 (legacy-MCP negotiation); G5 |
| Codex CLI / app-server | experimental (per-method gating) | **floating** — last observed `@openai/codex@0.157.1` (commit `36650394c5b38c2990ccf2a3457165ca3e9d9726`); see "Floating-version policy" below | 2026-09-26 | https://github.com/openai/codex/releases/tag/rust-v0.157.1 | 2026-09-26 | G2, G5, G4 (Codex leg) |
| MCP — current era | supported | `2026-07-28` | 2026-07-28 | https://modelcontextprotocol.io/specification/2026-07-28/ | 2026-09-16 | G4, G1 |
| MCP — legacy era | supported | `2025-11-25` | 2025-11-25 | https://modelcontextprotocol.io/specification/2025-11-25/ | 2026-09-16 | G4, G1 |
| Rust MCP SDK (`rmcp`) | supported | `3.4.0` | 2026-09-15 | https://github.com/modelcontextprotocol/rust-sdk/releases (tag `rmcp-v3.4.0`); https://crates.io/crates/rmcp | 2026-09-17 | G4; G1 |
| `keyring` (credential store) | supported | `4.2.0` | 2026-08-29 | https://crates.io/crates/keyring; https://raw.githubusercontent.com/open-source-cooperative/keyring-rs/v4.2.0/Cargo.toml | 2026-09-17 | none directly (implementation dependency — see note) |
| `interprocess` (IPC crate, candidate) | supported | `2.4.4` | not stated on source page | https://crates.io/api/v1/crates/interprocess; https://raw.githubusercontent.com/kotauskas/interprocess/main/Cargo.toml | 2026-09-17 | none directly (implementation dependency — see note) |
| `age` (encrypted-file key-storage fallback) | supported | `0.12.1` | 2026-07-14 | https://crates.io/api/v1/crates/age; https://raw.githubusercontent.com/str4d/rage/v0.12.1/age/Cargo.toml | 2026-09-17 | none directly (implementation dependency — see note) |
| `ed25519-dalek` (envelope signature) | supported | `3.0.0` | not stated on source page | https://crates.io/api/v1/crates/ed25519-dalek; https://raw.githubusercontent.com/dalek-cryptography/curve25519-dalek/main/ed25519-dalek/Cargo.toml | 2026-09-17 | none directly (implementation dependency — see note) |
| `serde_jcs` (canonical serialization, RFC 8785 JCS) | supported | `0.2.0` | 2026-03-25 | https://crates.io/api/v1/crates/serde_jcs; https://docs.rs/serde_jcs/0.2.0/serde_jcs/ | 2026-09-17 | none directly (implementation dependency — see note) |
| Zenoh | supported | `1.10.1` | 2026-09-07 | https://github.com/eclipse-zenoh/zenoh/releases | 2026-09-16 | G3 |
| Rust toolchain | supported | `1.98.1` | 2026-09-03 | https://blog.rust-lang.org/2026/09/03/Rust-1.98.1/ | 2026-09-16 | G3 (build) |
| ACP (forward-compat only) | supported | protocol version `1` (schema v2 alpha) | not stated on source page | https://agentclientprotocol.com/protocol/ | 2026-09-16 | none (not a v0.1 dependency) |

## Pin records

### Claude Code Channels

- Surface label: **research preview**. Per PLANNING-PROMPT.md §3.1, this label is
  fixed by the source itself and is not upgraded.
- Pinned version: `v2.1.274`. Source: https://github.com/anthropics/claude-code/releases/tag/v2.1.274,
  published 2026-09-17T00:12:02Z (UTC — GitHub shows this as "Sep 17, 00:12"),
  retrieved 2026-09-16 (local calendar date; the UTC publish instant is within
  the retrieval day in the retriever's local timezone, so retrieval is not
  actually earlier than publication once both are read as the same UTC
  instant window). Record all timestamps in this row as UTC to avoid the
  apparent one-day mismatch.
- Floor 1 — Channels exist at all: Claude Code `>= v2.1.232`. Source (secondary):
  PLANNING-PROMPT.md §3.1 line 49, "Stability: research preview on Claude Code
  v2.1.232+." Re-fetched https://code.claude.com/docs/en/channels.md today
  (2026-09-16): the page's "Research preview" section states only "Channels are
  a research preview feature" and contains no string `2.1.232` and no minimum
  version. The floor is therefore carried from the project's own summary, not
  independently confirmed on the first-party page as of this retrieval
  (UNVERIFIED — need the first-party release notes for v2.1.232, or the
  channels-reference page, to state this floor directly; see "Open questions
  carried into B2" below).
  `v2.1.232` itself: published 2026-08-13T23:29:59Z. Source:
  https://github.com/anthropics/claude-code/releases/tag/v2.1.232, retrieved 2026-09-16.
- Floor 2 — permission relay (`claude/channel/permission`): Claude Code `>= v2.1.234`.
  Source: https://code.claude.com/docs/en/channels-reference.md — "Before v2.1.234,
  Claude Code treated `false` as declared" and "Claude Code v2.1.234 and later sends
  permission requests only to servers it registered as channels for the session" —
  retrieved 2026-09-16. `v2.1.234` itself: published 2026-08-17T20:20:58Z. Source:
  https://github.com/anthropics/claude-code/releases/tag/v2.1.234, retrieved 2026-09-16.
- The pinned version `v2.1.274` is `>=` both floors (`>= v2.1.232` and `>= v2.1.234`),
  so permission relay is in scope at this pin. Both floors are satisfied; permission
  relay is not out of scope.
- Verbatim API names carried by this pin (Source:
  https://code.claude.com/docs/en/channels-reference.md, retrieved 2026-09-16):
  `capabilities.experimental['claude/channel']`, `capabilities.experimental['claude/channel/permission']`,
  `notifications/claude/channel`, `notifications/claude/channel/permission_request`,
  `notifications/claude/channel/permission`.
- MCP negotiation constraint carried by this pin (Source:
  https://code.claude.com/docs/en/mcp.md, retrieved 2026-09-16): "if you set
  `MCP_PROTOCOL_NEGOTIATION` to `auto` and a channel server negotiates MCP protocol
  revision 2026-07-28, it can't deliver channel messages, so Claude Code doesn't
  register it as a channel. Leaving the variable unset, or setting it to `legacy`,
  keeps stdio servers on the earlier handshake." — quoted verbatim, `MCP_PROTOCOL_NEGOTIATION=legacy`.
- Compatibility shim boundary: DESIGN.md names no module boundary specific to the
  Claude Channels preview surface (checked: no "shim" term appears in DESIGN.md).
  `shim boundary: UNNAMED — see DESIGN.md`. Carried to task 12's open-items list below
  and is a B2/C-decision input, not resolved here.
- Gates affected: **G1** (Claude wake — go/no-go, no fallback), **G4** via the
  legacy-MCP negotiation constraint above, and **G5** (Provenance: STATUS.md's Gate
  verdicts table states G5's verdict depends on machine-set provenance rendering on
  both providers, so it depends on this pin, not only on the Codex pin).

### Codex CLI and app-server

#### Floating-version policy (operator decision, 2026-09-26)

**This row no longer holds a fixed pin.** During gate G4 on 2026-09-25/26, a Codex
auto-updater moved the environment from `0.154.0` to `0.157.0`, and then to `0.157.1`
within about 24 hours. The updater is a detached `codex app-server daemon
pid-update-loop` process that is not started by OAC. Per the Codex daemon README it
"fetches via the platform's installer" hourly and restarts the app-server onto the new
binary. The operator chose to **leave the updater running** and accept a moving target
rather than hold a pin. Consequences, binding on every Codex-side gate:

- Each gate result records the Codex version it **actually ran on**, as reported by the
  CLI, the daemon (`codex app-server daemon version`), and the client's `clientInfo`.
- Any new Codex release invalidates the Codex-side results that ran on an older version:
  G2, G5, and G4's Codex leg. They revert to `NOT RUN` for the current environment until
  they are re-run. The pin-move checklist above applies whenever this row's last-observed
  version changes.
- The §3.2 facts must be re-verified against each newly observed version before a
  Codex-side gate is re-run on it (`oac-evidence` §7).
- Version history of this row: `0.154.0` (2026-09-09; B1 pin; G2 PASS on it), then
  `0.157.0` (npm `2026-09-25T02:35:19.752Z`; tag `rust-v0.157.0` → commit
  `00c972ed5d6ff6499317fd41b7f23605b8e6850d`), then `0.157.1` (npm
  `2026-09-26T01:06:57.149Z`; GitHub release `rust-v0.157.1` published
  `2026-09-26T01:02:31Z`; tag object `ac0e23e5232692b95268583c8278c50b8c436d2b` → commit
  `36650394c5b38c2990ccf2a3457165ca3e9d9726`). All retrieved 2026-09-26 via `npm view` and
  the GitHub API.

The B1/B2 record below describes the original `0.154.0` pin and is kept as history.
Its facts are not re-verified for `0.157.1`; that re-verification is an open item in
`docs/planning/STATUS.md`.

- Surface label: **experimental**, per-method gating (PLANNING-PROMPT.md §3.2: methods
  are individually stable or experimental; experimental ones require
  `capabilities.experimentalApi`).
- Pinned npm package version: `@openai/codex@0.154.0`. Source: `npm view
  @openai/codex@0.154.0 time`, publish timestamp `2026-09-09T22:40:10.746Z`, retrieved
  2026-09-16.
- Cross-checked against GitHub release: tag `rust-v0.154.0`, published by
  `github-actions` on 2026-09-09 (22:35), commit SHA
  `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`. Source:
  https://github.com/openai/codex/releases/tag/rust-v0.154.0, retrieved 2026-09-16.
  This commit is the answer surface for the daemon-attach question below — a version
  string alone cannot answer it.
- Baseline for comparison (PLANNING-PROMPT.md §3.2): `@openai/codex` 0.154.0
  (2026-09-09) — the pin matches the baseline exactly; no drift.
- Auth boundary (task 14 boundary check): the app-server uses the saved CLI login
  (`CODEX_HOME/auth.json` or OS keyring). OAC never holds OpenAI credentials. Source:
  PLANNING-PROMPT.md §3.2, "Auth: the app-server uses the saved CLI login
  (`CODEX_HOME/auth.json` or OS keyring). An OAC adapter never holds OpenAI
  credentials." — unchanged, retrieved 2026-09-16.
- Compatibility shim boundary: DESIGN.md names no module boundary specific to the
  Codex experimental live-inject surface (checked: no "shim" term appears in
  DESIGN.md; PLANNING-PROMPT.md §5.11 mentions `oac mcp-shim` only as an illustrative
  CLI-model example, not a named module in DESIGN.md).
  `shim boundary: UNNAMED — see DESIGN.md`. Carried to task 12's open-items list below.
- Gates affected: **G2** (Codex live inject), **G5** (Provenance).

#### Daemon-attach open question (G2 input)

Whether implicit daemon attach — the TUI, launched without config overrides,
attaching to a running `codex app-server daemon start` via control socket
`CODEX_HOME/app-server-control/app-server-control.sock` — is present in the pinned
release `0.154.0` (commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`) or only on
`main`, is **UNVERIFIED — B1 does not run the G2 spike; only executing the pinned
build against the control socket resolves it.**

This is a **G2 go/no-go input**, resolved by task D2 (the G2 spike) and gate G2
itself, not by B1. B1's job is only to fix the version and commit the question is
asked against.

**RESOLVED by G2 (2026-09-25, `docs/planning/gates/G2-result.md`).** Implicit daemon
attach runs at runtime in the released `0.154.0`. A plainly launched TUI's thread was
loaded in the running daemon's process. Shown on Windows, with default `CODEX_HOME`, from
a non-elevated terminal; macOS and Linux were not exercised.

Verbatim API names for this question, quoted exactly (Source: PLANNING-PROMPT.md
§3.2, unchanged, retrieved 2026-09-16):

- `thread/queue/add`
- `turn/steer`
- `turn/start`
- `codex queue --thread <id> --message <text>`
- `codex --remote ws://…`

### MCP revisions (dual era)

Both revisions carry the **supported** label (PLANNING-PROMPT.md §3.3: no preview or
experimental language is used for MCP itself). MCP revisions are date strings, not
semver, and are recorded verbatim — never reformatted.

- **Current: `2026-07-28`.** Source:
  https://modelcontextprotocol.io/specification/2026-07-28/, the revision is
  identified and published under this date string on the spec site; changelog page
  https://modelcontextprotocol.io/specification/2026-07-28/changelog confirms it
  supersedes `2025-11-25`. Retrieved 2026-09-16.
- **Legacy: `2025-11-25`.** Source:
  https://modelcontextprotocol.io/specification/2025-11-25/, retrieved 2026-09-16.
- SEP-2133 (extensions) is **Final**, created 2025-01-21. Source:
  https://modelcontextprotocol.io/seps/2133-extensions, badge "Final", field table
  `Created: 2025-01-21`, retrieved 2026-09-16. (PLANNING-PROMPT.md §3.3 states a
  2026-01-26 finalization date for this SEP; the SEP page itself states only the
  creation date 2025-01-21 and a "Final" status with no separate finalization date
  field. This is flagged as an open item below rather than silently reconciled.)
- Extension identifier scheme, quoted verbatim (Source:
  https://modelcontextprotocol.io/seps/2133-extensions, retrieved 2026-09-16):
  "Extensions are identified using a unique *extension identifier* with the format:
  `{vendor-prefix}/{extension-name}`, e.g. `io.modelcontextprotocol/oauth-client-credentials`
  or `com.example/websocket-transport`." — note this SEP page uses the term
  "vendor-prefix", not "reverse-dns-prefix"; PLANNING-PROMPT.md §3.3 uses
  "reverse-dns-prefix". Both describe the same reversed-domain-name convention
  ("the vendor prefix SHOULD be a reversed domain name that the extension author owns
  or controls"). Recorded as a terminology note, not a fact drift.
- Reserved-prefix rule: PLANNING-PROMPT.md §3.3 states "Prefixes whose second label is
  `modelcontextprotocol` or `mcp` are reserved." The SEP-2133 page fetched today states
  official extensions "use the `io.modelcontextprotocol` vendor prefix" but does not,
  in the text retrieved, spell out a general reservation rule for any second label of
  `modelcontextprotocol` or `mcp`. This is carried as an open item below — it is a B2
  re-verification input, not resolved here.
- Gates affected: **G4** (dual-era server), **G1** (Claude channels require
  negotiating legacy per the Claude Code pin record above).

### Rust MCP SDK (`rmcp`)

- Surface label: **supported** — official SDK published under the
  `modelcontextprotocol` GitHub organization.
- Pinned crate version: `3.4.0`. Source: https://crates.io/crates/rmcp, retrieved
  2026-09-17.
- Release date: 2026-09-15. Source:
  https://github.com/modelcontextprotocol/rust-sdk/releases, tag `rmcp-v3.4.0`,
  retrieved 2026-09-17.
- License: Apache-2.0. Source:
  https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/rmcp-v3.4.0/Cargo.toml,
  `[workspace.package]` `license = "Apache-2.0"`, retrieved 2026-09-17.
- Legacy-revision support: `ProtocolVersion::V_2025_11_25` is a declared constant at
  this tag, and `ProtocolVersion::LATEST` resolves to it (not to `V_2026_07_28`).
  Source:
  https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/rmcp-v3.4.0/crates/rmcp/src/model.rs,
  retrieved 2026-09-17 — full verbatim quote and analysis:
  `docs/planning/decisions/C1-language-runtime.md` §4-§5.
- Gates affected: **G4** (dual-era server — this is the SDK the server is built on),
  **G1** (Claude wake — Claude Code requires `MCP_PROTOCOL_NEGOTIATION=legacy`, i.e. a
  server that can negotiate `2025-11-25`).

### `keyring` (credential store)

- Surface label: **supported** — general-purpose, actively maintained OS-credential
  crate, not a preview/experimental provider surface.
- Pinned crate version: `4.2.0`. Source: https://crates.io/crates/keyring, retrieved
  2026-09-17.
- Release date: 2026-08-29. Source: crates.io publish metadata for `keyring` `4.2.0`,
  retrieved 2026-09-17.
- License: MIT OR Apache-2.0. Source:
  https://raw.githubusercontent.com/open-source-cooperative/keyring-rs/v4.2.0/Cargo.toml,
  `license = "MIT OR Apache-2.0"`, retrieved 2026-09-17.
- Windows Credential Manager backend: `windows-native-keyring-store` (crate `1.1.0`,
  MIT OR Apache-2.0) is the optional dependency crate `keyring`'s `v1`/default feature
  pulls in on `cfg(windows)`. Source:
  https://raw.githubusercontent.com/open-source-cooperative/keyring-rs/v4.2.0/Cargo.toml
  and https://crates.io/api/v1/crates/windows-native-keyring-store, retrieved
  2026-09-17. Full analysis: `docs/planning/decisions/C1-language-runtime.md` §8.
- **Gates affected: none directly** — `keyring` is an implementation dependency
  (Stage 3+ credential-store crate for OAC's own device keys), not a gate-spike
  dependency the way `rmcp`/Codex crates/Zenoh are. It is load-bearing for the
  reversal condition's Windows half (§12 of the C1 decision) and for the §9 packaging
  conclusion in that same file, so a version bump here still requires re-checking
  that reversal test even though no `G<n>-result.md` verdict is invalidated by it.

### `interprocess` (IPC crate, candidate)

- Surface label: **supported** — general-purpose, actively maintained crate, not a
  preview/experimental provider surface.
- Pinned crate version: **`2.4.4`** (candidate, not final — C2 §4 leaves the final IPC
  crate open between `interprocess` and `tokio`'s `net::windows::named_pipe` +
  `UnixListener`; this is a Stage 3 implementation detail). Source:
  https://crates.io/api/v1/crates/interprocess, `max_stable_version` field, retrieved
  2026-09-17.
- License: `0BSD OR Apache-2.0`. Source: same API response, `license` field, retrieved
  2026-09-17; cross-checked against
  https://raw.githubusercontent.com/kotauskas/interprocess/main/Cargo.toml, retrieved
  2026-09-17. OAC elects the Apache-2.0 arm (same election as `zenoh`, `keyring`).
- Full analysis, including named-pipe/Unix-socket coverage and the unresolved
  peer-credential-accessor question: `docs/planning/decisions/C2-process-model.md` §4
  "IPC crate".
- **Gates affected: none directly** — implementation dependency (Stage 3+ local-IPC
  transport crate for the daemon/`oac mcp-shim` connection), not a gate-spike
  dependency.

### `age` (encrypted-file key-storage fallback)

- Surface label: **supported** — actively maintained reference implementation of the
  published `age-encryption.org/v1` format, not a preview/experimental provider
  surface.
- Pinned crate version: `0.12.1`. Source:
  https://raw.githubusercontent.com/str4d/rage/v0.12.1/age/Cargo.toml,
  `version = "0.12.1"`, retrieved 2026-09-17.
- Release date: 2026-07-14. Source: crates.io publish metadata for `age` `0.12.1`
  (`created_at`), retrieved 2026-09-17.
- License: MIT OR Apache-2.0 (workspace-level, inherited via `license.workspace =
  true`). Source: https://raw.githubusercontent.com/str4d/rage/v0.12.1/Cargo.toml,
  `[workspace.package]` `license = "MIT OR Apache-2.0"`, retrieved 2026-09-17. OAC
  elects the Apache-2.0 arm (same election as `zenoh`, `keyring`, `interprocess`).
- Passphrase-based encryption: `age::scrypt::Recipient` / `age::scrypt::Identity`.
  Source: https://docs.rs/age/0.12.1/age/, retrieved 2026-09-17. Full analysis,
  including when the fallback engages, file location/permissions, and passphrase/KDF
  source: `docs/planning/decisions/C4-session-identity.md` §11.
- **Gates affected: none directly** — implementation dependency (Stage 3+ fallback path
  for OAC's own device key when no OS credential store is reachable), not a gate-spike
  dependency.

### `ed25519-dalek` (envelope signature)

- Surface label: **supported** — general-purpose, actively maintained cryptography
  crate under `dalek-cryptography`, not a preview/experimental provider surface.
- Pinned crate version: `3.0.0`. Source:
  https://crates.io/api/v1/crates/ed25519-dalek, `max_stable_version` field, retrieved
  2026-09-17.
- License: **BSD-3-Clause** — flagged separately from OAC's usual `MIT OR Apache-2.0`
  dual-license shape; permissive and Apache-2.0-compatible, but not an OR-clause dual
  license to elect an arm of. Source: same crates.io response, `license` field, retrieved
  2026-09-17; cross-checked against
  https://raw.githubusercontent.com/dalek-cryptography/curve25519-dalek/main/ed25519-dalek/Cargo.toml,
  `license = "BSD-3-Clause"`, `rust-version = "1.85"`, retrieved 2026-09-17.
- MSRV vs Rust toolchain pin: `1.85 <= 1.98.1` — satisfied.
- RUSTSEC-2022-0093 (double-public-key signing oracle): patched at `>=2`; pinned `3.0.0`
  is well past the patched floor. Source: https://rustsec.org/advisories/RUSTSEC-2022-0093.html,
  retrieved 2026-09-17.
- Strict verification method: `VerifyingKey::verify_strict`, rejects small-order/torsion
  public keys via the group-equation check. Source:
  https://docs.rs/ed25519-dalek/3.0.0/ed25519_dalek/struct.VerifyingKey.html, retrieved
  2026-09-17. Full analysis: `docs/planning/decisions/C5-envelope-auth.md` §2.
- **Gates affected: none directly** — implementation dependency (Stage 3+ envelope
  signing/verification crate), not a gate-spike dependency.

### `serde_jcs` (canonical serialization, RFC 8785 JCS)

- Surface label: **supported** — actively downloaded (1,675,393 downloads at
  retrieval), not yanked, implements the published RFC 8785 standard.
- Pinned crate version: `0.2.0`. Source: https://crates.io/api/v1/crates/serde_jcs,
  `max_stable_version` field, retrieved 2026-09-17.
- Release date: 2026-03-25. Source: https://crates.io/api/v1/crates/serde_jcs/0.2.0,
  publish metadata, retrieved 2026-09-17.
- License: MIT OR Apache-2.0. Source: same crates.io response, `license` field,
  retrieved 2026-09-17. OAC elects the Apache-2.0 arm (same election as `zenoh`,
  `keyring`, `interprocess`, `age`).
- API surface: `to_string`, `to_vec`, `to_writer` — all implement RFC 8785 JCS. Source:
  https://docs.rs/serde_jcs/0.2.0/serde_jcs/, retrieved 2026-09-17. Full analysis,
  including the domain-separation prefix built on top: `docs/planning/decisions/
  C5-envelope-auth.md` §3.
- **Gates affected: none directly** — implementation dependency (Stage 3+ envelope
  canonicalization crate), not a gate-spike dependency.

### Zenoh

- Surface label: **supported**.
- Pinned crate version: `1.10.1`. Source:
  https://github.com/eclipse-zenoh/zenoh/releases, tagged release "Latest", published
  2026-09-07, retrieved 2026-09-16. Attempted cross-check against crates.io
  (https://crates.io/crates/zenoh): the fetch did not return page content in
  this session, so crates.io does **not** confirm anything here (UNVERIFIED —
  see "Open questions carried into B2" below). The version/date figures above
  are taken from the GitHub releases page alone, which is first-party for this
  project and states them unambiguously.
- Baseline (PLANNING-PROMPT.md §3.4): 1.10.1 (2026-09-07) — matches exactly, no drift.
- Hard constraint: pin **MUST be `>= 1.10.0`**. Reason: same-host loopback discovery
  was broken before 1.10.0. Fixed by PR #2671
  (https://github.com/eclipse-zenoh/zenoh/pull/2671). Source: PLANNING-PROMPT.md §3.4,
  "Same-host discovery over loopback was broken before 1.10.0 (PR #2671); plan on
  ≥1.10.0." — unchanged, retrieved 2026-09-16. `1.10.1 >= 1.10.0`: constraint satisfied.
- License: dual **EPL-2.0 / Apache-2.0**. Source: PLANNING-PROMPT.md §3.4, "Stable
  release 1.10.1 (2026-09-07), dual EPL-2.0 / Apache-2.0." — unchanged, retrieved
  2026-09-16. Recorded here because B4 and the Stage 6 license inventory read it.
- Gates affected: **G3** (Zenoh local peer).
  **Note (G3 run 2026-09-25):** G3 ran against this core version through the PyPI wheel
  `eclipse-zenoh==1.10.1`. GitHub's compare of the wheel's core checkout `1211779` with tag
  `1.10.1` reports them identical. The wheel has no pin row of its own. A G3 re-run must use
  a wheel verified to wrap this core tag, or use the Rust crate.

### Rust toolchain

- Surface label: **supported**. This was previously unpinned (STATUS.md said "not
  pinned"); this record closes that.
- Pinned stable release: `1.98.1`. Source:
  https://blog.rust-lang.org/2026/09/03/Rust-1.98.1/, published 2026-09-03; also
  https://github.com/rust-lang/rust/releases/tag/1.98.1. Retrieved 2026-09-16.
- Zenoh MSRV at the pinned Zenoh tag: `1.75.0`. Source: `Cargo.toml`,
  `rust-version = "1.75.0"`, at tag `1.10.1` in
  https://github.com/eclipse-zenoh/zenoh/blob/1.10.1/Cargo.toml (fetched as
  https://raw.githubusercontent.com/eclipse-zenoh/zenoh/1.10.1/Cargo.toml), retrieved
  2026-09-16.
- Constraint: OAC toolchain pin `>=` Zenoh MSRV. `1.98.1 >= 1.75.0`: satisfied.
- Deliverable: `rust-toolchain.toml` at the repo root pins `channel = "1.98.1"` with
  `rustfmt` and `clippy` components, so the pin is enforced by the toolchain, not only
  by this prose. See `c:\sources\OAC\rust-toolchain.toml`.
- Gates affected: **G3** (build must succeed to run the Zenoh spike).
  **Note (G3 run 2026-09-25):** that run used the Python binding `eclipse-zenoh==1.10.1`,
  whose core is Zenoh tag `1.10.1`, not a Rust build. So this pin was **not exercised**.
  G3 on the Rust crate built with `1.98.1` remains UNVERIFIED (`docs/planning/STATUS.md`,
  `docs/planning/gates/G3-result.md`). The G3 row is kept here because a Rust-crate re-run
  will rely on it.

### ACP (forward-compatibility only)

- Surface label per PLANNING-PROMPT.md §3.5: stable protocol version `1`; schema v2 is
  alpha.
- Pinned: protocol version `1` (stable). Source:
  https://agentclientprotocol.com/protocol/, retrieved 2026-09-16 — page links
  throughout to `/protocol/v1/...` paths. On fetch today the page itself shows
  no literal `protocolVersion` field in its own text; the earlier claim that it
  showed `"protocolVersion": 1` in a client initialization example overstated
  the page's content and is corrected here (UNVERIFIED — the `/protocol/v1/`
  path naming supports version `1`, but the literal field was not observed on
  this page as fetched; not a v0.1 dependency, non-blocking).
- Schema v2 alpha: carried from PLANNING-PROMPT.md §3.5 (unchanged) — this specific
  page as fetched today did not independently restate "schema v2 is alpha" in the text
  retrieved; flagged below as an open item rather than silently re-asserted as
  independently re-confirmed.
- Cross-check source: https://cursor.com/docs/cli/acp — confirms Cursor CLI runs as an
  ACP agent negotiating `"protocolVersion": 1`; retrieved 2026-09-16.
- **ACP is not a v0.1 dependency.** A drift in this pin does not invalidate any gate.
  This row is kept (not dropped) because STATUS.md already carries an ACP row as a
  tracked baseline; dropping it here would lose that baseline.
- Gates affected: none.

## Constraint floors

Each floor below is independently checkable by a reader who has only this file open.

- **Claude Code `>= v2.1.232`** — channels exist at all. Reason: Channels ship as a
  research-preview feature starting at this release (Source:
  https://code.claude.com/docs/en/channels.md, "Stability: research preview on Claude
  Code v2.1.232+"). Consequence of violating: `--channels` and
  `--dangerously-load-development-channels` do not exist below this version; the
  Claude adapter cannot be built at all.
- **Claude Code `>= v2.1.234`** — permission relay. Reason: `claude/channel/permission`
  capability semantics changed in this release (Source:
  https://code.claude.com/docs/en/channels-reference.md, "Before v2.1.234, Claude Code
  treated `false` as declared" and "Claude Code v2.1.234 and later sends permission
  requests only to servers it registered as channels for the session"). Consequence of
  violating: below v2.1.234, permission relay either is absent or has different
  opt-out semantics (`false` treated as declared), so the OAC permission-relay path is
  unsafe to build against.
- **Zenoh `>= 1.10.0`** — loopback discovery. Reason: same-host discovery over
  loopback was broken before this release, fixed by PR #2671 (Source:
  https://github.com/eclipse-zenoh/zenoh/pull/2671; PLANNING-PROMPT.md §3.4).
  Consequence of violating: two OAC peers on one machine cannot discover each other
  over loopback, which breaks the default local-mode deployment (G3).
- **Rust `>= Zenoh MSRV` (currently `>= 1.75.0`)** — build correctness. Reason: the
  pinned Zenoh release's `Cargo.toml` declares `rust-version = "1.75.0"` (Source:
  https://github.com/eclipse-zenoh/zenoh/blob/1.10.1/Cargo.toml, tag `1.10.1`).
  Consequence of violating: the build fails below this Rust version; OAC's own pin
  (`1.98.1`) already satisfies it, so this floor is currently non-binding but must be
  re-checked whenever either pin moves.
- **MCP channel servers must negotiate `2025-11-25` or earlier** — Claude Channels
  compatibility. Reason: quoted verbatim (Source:
  https://code.claude.com/docs/en/mcp.md): "if you set `MCP_PROTOCOL_NEGOTIATION` to
  `auto` and a channel server negotiates MCP protocol revision 2026-07-28, it can't
  deliver channel messages, so Claude Code doesn't register it as a channel." The
  environment variable and forced value are `MCP_PROTOCOL_NEGOTIATION=legacy`.
  Consequence of violating: a channel server built against the current MCP era
  (`2026-07-28`) silently fails to register as a Claude channel — G4's dual-era design
  exists specifically to satisfy this floor.

## Open questions carried into B2

Every entry below uses the oac-evidence §5 form: `<claim> (UNVERIFIED — <reason>)`, plus
a one-line resolution pointer added by B2 (`docs/planning/REVERIFICATION-B2.md`). B2 also
corrects a small number of B1 prose errors above this line, found during re-verification:
the SEP-2133 bullet's `Created: 2025-01-26/2025-01-21` is corrected to `Created:
2025-01-21` (the SEP page's own field, re-confirmed 2026-09-16), and the Claude Code
release-date, floor-1, crates.io cross-check, and ACP `protocolVersion` rows carry their
own re-verified wording as of this pass. Those corrections are noted inline where they
occur; everything else above this line is unchanged from B1.

- Whether implicit Codex daemon attach (TUI attaching to a running `codex app-server
  daemon start` via `CODEX_HOME/app-server-control/app-server-control.sock`) is
  present in the pinned release `@openai/codex@0.154.0` / commit
  `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`, or only on `main` (UNVERIFIED — resolved
  only by running the G2 spike, task D2, against the pinned build; not answerable from
  a version string alone).
  **PARTIALLY RESOLVED — see REVERIFICATION-B2.md §3.2 box 4.** Source code confirmed
  present at the pinned commit (control socket path, attach-or-embed branch, `codex
  queue` subcommand). Runtime behaviour is CARRIED — risk owner D2/G2.
  **RESOLVED by G2 (2026-09-25):** runtime attach confirmed on `0.154.0` on Windows,
  default `CODEX_HOME`, non-elevated terminal; macOS and Linux not exercised. See
  `docs/planning/gates/G2-result.md`.
- The named compatibility shim boundary for the Claude Code Channels preview surface
  (UNVERIFIED — DESIGN.md names no such module; `shim boundary: UNNAMED — see
  DESIGN.md`, needs a C-series decision or a DESIGN.md update before B2/D1 can cite a
  real boundary).
  **CARRIED — risk item 9, see REVERIFICATION-B2.md "Carried to 11-risks.md".** Out of
  scope for B2 by design; needs a C-series decision or a DESIGN.md update.
- The named compatibility shim boundary for the Codex experimental live-inject surface
  (UNVERIFIED — same reason as above; `shim boundary: UNNAMED — see DESIGN.md`).
  **CARRIED — risk item 10, see REVERIFICATION-B2.md "Carried to 11-risks.md".** Same
  reason and owner as the Claude Channels shim boundary above.
- SEP-2133's finalization date: PLANNING-PROMPT.md §3.3 states "final 2026-01-26", but
  the SEP-2133 page itself (retrieved 2026-09-16) states only `Created: 2025-01-21`
  and a "Final" status badge, with no distinct finalization-date field visible in the
  fetched text (UNVERIFIED — need to locate the PR merge date for PR #2133 as the
  first-party finalization date, or confirm the discrepancy is a transcription
  difference between "created" and "final" dates).
  **RESOLVED — see REVERIFICATION-B2.md §3.3 carry-over (a).** PR #2133's
  `merged_at` is `2026-01-26T23:57:49Z`, confirming §3.3's date exactly. No drift.
- SEP-2133's reserved-prefix rule for any extension prefix whose second label is
  `modelcontextprotocol` or `mcp` (PLANNING-PROMPT.md §3.3) was not independently
  re-confirmed verbatim on the SEP-2133 page as fetched today, which describes only
  that official extensions use the `io.modelcontextprotocol` prefix (UNVERIFIED —
  re-fetch the SEP text in full, or locate the exact clause, before relying on the
  general reservation rule in spec-authoring work).
  **RESOLVED as DRIFT — see REVERIFICATION-B2.md §3.3 carry-over (b) and Drift register
  D2.** No such reservation clause exists in the SEP-2133 text; §3.3's rule is
  unsupported and `oac-spec-authoring` must not rely on it.
- ACP schema v2 "alpha" status was not independently re-confirmed on
  https://agentclientprotocol.com/protocol/ as fetched today; it is carried forward
  from PLANNING-PROMPT.md §3.5 only (UNVERIFIED — re-check against
  https://agentclientprotocol.com/protocol/ or its schema changelog directly; low
  priority since ACP is not a v0.1 dependency).
  **CARRIED — risk item 7, see REVERIFICATION-B2.md "Carried to 11-risks.md".**
  Re-checked in B2 (still absent from the page); low priority, not a v0.1 dependency.
- Zenoh crate version/date were read from the GitHub releases page rather than
  directly from crates.io's rendered page, because the crates.io fetch did not return
  page content in this session (UNVERIFIED — re-confirm directly on
  https://crates.io/crates/zenoh when that page is reachable; GitHub releases is
  first-party for the same project and is not expected to disagree).
  **CARRIED — risk item 8, see REVERIFICATION-B2.md "Carried to 11-risks.md".** Not
  re-attempted in B2; GitHub Releases remains the source of record.
