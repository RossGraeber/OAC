# OAC pinned external surfaces

A pin is a first-party-observed version string for one external surface OAC depends
on: an exact release version (or, for MCP, an exact revision date-string), the date
that release was published, the first-party URL the pin was observed at, and the date
it was retrieved. A pin is not a preference or a "latest as of planning" note — it is
the version the rest of the plan is written against.

**Exception: the harness CLIs float, and their versions warn, never gate.** The Codex
CLI / app-server row (floating since 2026-09-26) and the Claude Code (Channels) row
(floating since 2026-09-27) hold no pin. Operator decision on #216 (2026-10-01): "Minimum
version is the first version encountered while working. Document last version tested
against. Allow version to float. Do not gate on version, warn on version." Each of the two
rows therefore records a **minimum version** (the first version the project actually worked
with, cited from the record) and a **last tested version** (updated after each live run).
A version other than the last tested one, or below the minimum, is a warning: it never
stops a run, never makes it `NOT RUN`, never blocks CI and never by itself invalidates a
gate verdict. This covers the CLIs and their wire and daemon versions. See "Version policy
(operator decision, 2026-10-01, #216)" under each record.

**Changing any other row in this file is a trigger event.** Per `oac-evidence` §7, a moved
pin requires re-verifying every §3 fact that depended on it (Epic B2) and, per the
gate re-run policy (`docs/planning/gates/README.md`), re-running
every gate whose verdict depended on it. Do not silently bump a version in this file.
For the two harness rows, updating the last tested version after a live run is routine
record-keeping, not a pin move: it invalidates no gate verdict (see "Pin-move checklist").

## Pin-move checklist

When any pin in the table below changes (version, release date, or a row's
presence), make all of these edits in the **same commit**. **The Claude Code (Channels)
and Codex CLI / app-server rows are exempt** (#216, 2026-10-01): a change to their minimum
or last tested version only bumps `**Last updated:**` below and adds a dated line to the
row's version history. It reverts no verdict and adds no `INVALIDATED` callout. A new
harness version still needs the §3 facts re-checked (`oac-evidence` §7) before a gate
result relies on it, but that is a finding to follow up, not a gate on the run.

- [ ] Read the moved row's `Gates affected` cell to find which gate results to
      invalidate (never for the two harness rows above; see
      `docs/planning/gates/README.md` §a).
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

**Last updated:** 2026-10-08 (issue #7, decision record
`docs/planning/decisions/G-7-stage4-dependencies.md`: added rows `tokio`, `rcgen`,
`windows-sys` and `libc`, fixed pins for Stage 4, each `Gates affected: none`
(implementation dependencies). The pin-move checklist was executed in the same commit: rows
were added, but none names a gate, so no `G<n>-result.md` `Pin rows relied on` field or
`docs/planning/STATUS.md` `Pins relied on` cell changes, and no verdict is invalidated. The
`Rust MCP SDK (rmcp)` and Zenoh pins are unchanged; dated notes record their Stage 4 features
and consumers. `rmcp` stays at `3.4.0` rather than the current `3.5.1`: moving it would
revert G1 and G4, G-7 §3.1.) Previously 2026-10-07 (issue #343: the `Codex CLI / app-server` row's last tested
version is now `@openai/codex@0.161.0`, from the S3 fixture capture herdr run of 2026-10-07.
Routine record-keeping for a floating harness row (#216): not a pin move, no verdict or
record is invalidated. The Claude Code row is unchanged: the same day's G1 capture ran on
`2.1.285`, its last tested version.) Previously 2026-10-06 (issue #49, E9: the ACP row is re-checked against the
protocol's source repository. Protocol version `1` holds and is no longer UNVERIFIED. Schema
v2 "alpha" is verified, with no drift: the v2 schemas are published as prereleases
`v2.0.0-alphaX` (latest `schema-v2.0.0-alpha.7`), and the v2 protocol docs are in Draft. Not
a pin move: the pinned version is unchanged, ACP affects no gate, and no record or verdict is
invalidated.) Previously 2026-10-06 (issue #131, PR #304 review: the 2026-10-01 entry below gets
a dated note that its last tested Codex `0.159.3` was superseded by `0.160.0` on 2026-10-04,
and the Codex record's version history lists the 2026-10-05 and 2026-10-06 herdr re-records.
No version changes: minimum and last tested are as before, so this is not a pin move and no
verdict or record is invalidated.) Previously 2026-10-04 (issue #131: the `Codex CLI / app-server` row's last tested
version is now `@openai/codex@0.160.0`, from the G4 herdr run of 2026-10-04. Routine
record-keeping for a floating harness row (#216): not a pin move, no verdict or record is
invalidated.) Previously 2026-10-03 (issue #252, operator decision: scripted runs are verified, not
attested. Added the "Expected herdr executable" table under "herdr (test tooling)". The
herdr tag is unchanged, so this is not a pin move and no record or verdict is invalidated.)
Previously 2026-10-01 (issue #216, operator decision: harness versions float; warn,
never gate. The `Claude Code (Channels)` and `Codex CLI / app-server` rows now record a
**minimum version** and a **last tested version** instead of a "last observed" version.
Claude Code: minimum `v2.1.282` (first version worked with, G1 2026-09-25), last tested
`v2.1.285` (L3, 2026-10-01). Codex: minimum `@openai/codex@0.154.0` (first version worked
with, G2 2026-09-25), last tested `@openai/codex@0.159.3` (L3, 2026-10-01; *dated note,
2026-10-06: superseded on 2026-10-04 by `@openai/codex@0.160.0`, the current last tested
version; see the 2026-10-04 entry above, the table row and the record's version history*). Citations are
under "Version policy" in each record. The pin-move checklist no longer applies to these two
rows, so no gate verdict is invalidated. G1, G2, G4 and G5 keep their recorded verdicts and
the versions they ran on.) Previously 2026-09-29 (L1, issue #166: added row `Beacon (external memory
service)`, a fixed pin at `v1.3.29`, `Gates affected: none`, see "Beacon (external memory
service)" below and `docs/planning/decisions/L1-beacon-memory.md`. The pin-move checklist
was executed in the same commit. A row was added, but it names no gate, so no
`G<n>-result.md` `Pin rows relied on` field or `docs/planning/STATUS.md` `Pins relied on`
cell changes. No gate verdict is invalidated. Four Beacon facts at this pin are
UNVERIFIED, L1 §6.) Previously 2026-09-28 (K1, issue #124: added row `herdr (test tooling)`, a fixed
pin at `v0.9.1`, `Gates affected: none`, see "herdr (test tooling)" below and
`docs/planning/decisions/K1-herdr-evaluation.md`. The pin-move checklist was executed in
the same commit. A row was added, but it names no gate, so no `G<n>-result.md` `Pin rows
relied on` field or `docs/planning/STATUS.md` `Pins relied on` cell changes. No gate
verdict is invalidated. herdr's live behavior at this pin is UNVERIFIED: K1's go/no-go is
provisional and its live leg is NOT RUN.) Previously 2026-09-27 (Claude Code row changed from a fixed `v2.1.274` pin to
**floating**, last observed `v2.1.283`, by operator decision, mirroring the Codex row's
floating-version policy. The pin-move checklist was executed in the same commit: G1
invalidated (its 2026-09-25 PASS ran on `v2.1.282`, one version behind the newly recorded
last-observed `v2.1.283`); G4 and G5 stay current, since both already ran on `v2.1.283`
(`docs/planning/gates/G4-result.md`, `docs/planning/gates/G5-result.md`) and this move
only records that same version in PINS.md — it does not move the last-observed version
forward. See "Floating-version policy" under the Claude Code Channels record.) Previously
2026-09-26 (Codex row changed from a fixed `0.154.0` pin to **floating**,
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
| Claude Code (Channels) | research preview | **floating** — minimum `v2.1.282`; last tested `v2.1.285` (L3, 2026-10-01); warn on version, never gate; see "Version policy" below | 2026-09-29T19:27:30Z (UTC; the last tested version) | https://github.com/anthropics/claude-code/releases/tag/v2.1.285 | 2026-10-01 | G1; G4 (legacy-MCP negotiation); G5 (a version change invalidates none of them, #216) |
| Codex CLI / app-server | experimental (per-method gating) | **floating** — minimum `@openai/codex@0.154.0` (commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`); last tested `@openai/codex@0.161.0` (commit `979011409de0a60b52f179721948e65531d26144`; S3 fixture capture herdr run, 2026-10-07, #343); warn on version, never gate; see "Version policy" below | 2026-10-07T15:58:45Z (UTC; the last tested version) | https://github.com/openai/codex/releases/tag/rust-v0.161.0 | 2026-10-07 | G2, G5, G4 (Codex leg) (a version change invalidates none of them, #216) |
| MCP — current era | supported | `2026-07-28` | 2026-07-28 | https://modelcontextprotocol.io/specification/2026-07-28/ | 2026-09-16 | G4, G1 |
| MCP — legacy era | supported | `2025-11-25` | 2025-11-25 | https://modelcontextprotocol.io/specification/2025-11-25/ | 2026-09-16 | G4, G1 |
| Rust MCP SDK (`rmcp`) | supported | `3.4.0` | 2026-09-15 | https://github.com/modelcontextprotocol/rust-sdk/releases (tag `rmcp-v3.4.0`); https://crates.io/crates/rmcp | 2026-09-17 | G4; G1 |
| `keyring` (credential store) | supported | `4.2.0` | 2026-08-29 | https://crates.io/crates/keyring; https://raw.githubusercontent.com/open-source-cooperative/keyring-rs/v4.2.0/Cargo.toml | 2026-09-17 | none directly (implementation dependency — see note) |
| `interprocess` (IPC crate, candidate) | supported | `2.4.4` | not stated on source page | https://crates.io/api/v1/crates/interprocess; https://raw.githubusercontent.com/kotauskas/interprocess/main/Cargo.toml | 2026-09-17 | none directly (implementation dependency — see note) |
| `age` (encrypted-file key-storage fallback) | supported | `0.12.1` | 2026-07-14 | https://crates.io/api/v1/crates/age; https://raw.githubusercontent.com/str4d/rage/v0.12.1/age/Cargo.toml | 2026-09-17 | none directly (implementation dependency — see note) |
| `ed25519-dalek` (envelope signature) | supported | `3.0.0` | not stated on source page | https://crates.io/api/v1/crates/ed25519-dalek; https://raw.githubusercontent.com/dalek-cryptography/curve25519-dalek/main/ed25519-dalek/Cargo.toml | 2026-09-17 | none directly (implementation dependency — see note) |
| `serde_jcs` (canonical serialization, RFC 8785 JCS) | supported | `0.2.0` | 2026-03-25 | https://crates.io/api/v1/crates/serde_jcs; https://docs.rs/serde_jcs/0.2.0/serde_jcs/ | 2026-09-17 | none directly (implementation dependency — see note) |
| Zenoh | supported | `1.10.1` | 2026-09-07 | https://github.com/eclipse-zenoh/zenoh/releases | 2026-09-16 | G3 |
| `tokio` (async runtime) | supported | `1.53.2` | 2026-10-03 | https://crates.io/api/v1/crates/tokio/1.53.2 | 2026-10-08 | none directly (implementation dependency — see note) |
| `rcgen` (local-mode TLS certificate) | supported | `0.14.10` | 2026-08-28 | https://crates.io/api/v1/crates/rcgen/0.14.10 | 2026-10-08 | none directly (implementation dependency — see note) |
| `windows-sys` (named-pipe peer PID) | supported | `0.61.2` | 2025-10-06 | https://crates.io/api/v1/crates/windows-sys/0.61.2 | 2026-10-08 | none directly (implementation dependency — see note) |
| `libc` (Unix socket peer credentials) | supported | `0.2.190` | not stated on source page | https://crates.io/crates/libc; the workspace `Cargo.lock` | 2026-10-08 | none directly (implementation dependency — see note) |
| Rust toolchain | supported | `1.98.1` | 2026-09-03 | https://blog.rust-lang.org/2026/09/03/Rust-1.98.1/ | 2026-09-16 | G3 (build) |
| ACP (forward-compat only) | supported | protocol version `1` (schema v2 alpha, verified 2026-10-06: prerelease `schema-v2.0.0-alpha.7`; v2 protocol docs in Draft since 2026-07-20; see "ACP" dated note) | not stated on source page | https://agentclientprotocol.com/protocol/; https://github.com/agentclientprotocol/agent-client-protocol at `487ad3ea` | 2026-10-06 | none (not a v0.1 dependency) |
| herdr (test tooling) | supported | `v0.9.1` (tag object `8544776216a8d28088db59a5344ea21ee2d05d2b` → commit `065ef9d6a531c49fb8bee7e818ef837065b21ee9`); fixed, not floating; live behavior verified on Windows 2026-09-28 (K1 go on Windows), Linux and macOS UNVERIFIED, overall go provisional, see "herdr (test tooling)" below | 2026-09-16 | https://github.com/herdrdev/herdr/releases/tag/v0.9.1 | 2026-09-28 | none (dev/test tooling, never shipped — see note) |
| Beacon (external memory service) | supported | `v1.3.29` (tag object `72fd6643b5cd5c6ff6741f6016b3577654f61915` → commit `91e92216b79108475ba9b587d49c5ff3f7356fd8`); fixed, not floating; external service each harness connects to natively, never called, launched, configured or shipped by OAC; no fact UNVERIFIED (all four L1 items closed: three by L2, U1 confirmed live by L3, 2026-10-01, L1 §13), see "Beacon (external memory service)" below | 2026-09-28 (tagger date) | https://github.com/Asymptote-Labs/agent-beacon/tree/v1.3.29 | 2026-09-29 | none |

## Pin records

### Claude Code Channels

#### Version policy (operator decision, 2026-10-01, #216)

The operator decision on #216 replaces the invalidation rules of the floating-version
policy below. The row still floats. It records two versions, and neither one gates
anything:

- **Minimum version: `v2.1.282`.** This is the first Claude Code version the project
  actually worked with. During the original G1 spike (2026-09-25), the client that
  connected reported `clientInfo.version: "2.1.282"`. Sources:
  `docs/planning/gates/G1-result.md`, "Original run (2026-09-25, v2.1.282)" and its "Pin
  drift found during this spike" field (transcript line 20); and
  `docs/planning/gates/fixtures/MANIFEST.json`, the `g1-claude-wake/transcript.jsonl`
  entry, `observed_version.claude_code` "2.1.282 (clientInfo.version, line 20 ...)".
  Lines 1-19 of that fixture come from synthetic `test` clients (`clientInfo` `{"name":
  "test","version":"0"}` at lines 4, 12 and 16; 2026-09-17 and 2026-09-25), not Claude
  Code. Line 20 is the first real Claude Code client. The B1 pin `v2.1.274` was never run
  against: it was read from the release page, and the client that ran was already
  `2.1.282`. The VS Code extension directories `anthropic.claude-code-2.1.274-win32-x64`
  and `...-2.1.276-win32-x64` seen on disk during G1 (`G1-result.md`, "Pin drift found
  during this spike") never ran: the connecting client reported `2.1.282`. The capability
  floors in the B1 record below (`>= v2.1.232` Channels, `>= v2.1.234` permission relay)
  are documentation facts about when the surface appeared. They are not this minimum.
  `v2.1.282` is above both floors.
- **Last tested version: `v2.1.285`** (GitHub release `v2.1.285`, published
  2026-09-29T19:27:30Z UTC, retrieved 2026-10-01 via `gh api
  repos/anthropics/claude-code/releases/tags/v2.1.285`). It ran in the L3 Beacon live leg
  on 2026-10-01: `claude --version` `2.1.285`
  (`docs/planning/decisions/L1-beacon-memory.md` §13, "Live results (L3)"). Update this
  field, and the version history below, after each live run.
- **Warn, never gate.** A version other than the last tested one, or below the minimum,
  is a `VERSION WARNING` finding (`tools/herdr/lib/pins.mjs` `claudeVersionWarning`).
  This applies to `claude --version` and to the wire `clientInfo.version`. The warning
  never stops a run, never makes it `NOT RUN`, never blocks CI and never by itself
  invalidates a gate verdict. A gate result still records the version it actually ran
  on.
- **Recorded verdicts stand.** G1 (PASS, `v2.1.283`), G4 (PASS, `v2.1.283`) and G5 (FAIL,
  `v2.1.283`) keep their verdicts and the versions they ran on. *(Dated note, 2026-10-03,
  #220: G5 is now PASS. Its Codex leg was re-run on Codex `0.160.0`, and its Claude leg is
  still the `v2.1.283` run. See `docs/planning/gates/G5-result.md`. This note changes no
  pin.)* The invalidation history
  below (G1 invalidated 2026-09-27, re-run 2026-09-28) is kept as history.
- **§3.1 facts.** Re-checking the §3.1 facts against a newly tested version
  (`oac-evidence` §7) is still worth doing, and is still open at `v2.1.282` and later
  (see "Open questions carried into B2"). It is tracked as a finding and never gates a run.
  *Dated note, 2026-10-02 (#122):* done once, at the last tested version `2.1.285`, by
  operator decision on #122 (`docs/planning/REVERIFICATION-B2.md` "§3.1 re-check at
  Claude Code `2.1.285` (2026-10-02, #122)"). 18 of 23 rows hold, three drifted (D4-D6)
  and two stay UNVERIFIED. Versions after `2.1.285` are version warnings only. No further
  §3.1 re-check is scheduled.
- Version history (dated additions only): `v2.1.284` in a manual L3 attempt
  (2026-09-30), and `v2.1.285` in the L3 live leg (2026-10-01), both recorded in L1 §13.
  `v2.1.285` is the last tested version from 2026-10-01.

#### Floating-version policy (operator decision, 2026-09-27)

> **Superseded in part, 2026-10-01 (#216).** The row still floats. The rule below that a
> newly observed version invalidates G1, G4 and G5, and the use of the pin-move checklist
> for this row, no longer apply: see "Version policy" above. The text is kept as history.

**This row no longer holds a fixed pin.** The surface auto-updates faster than this
project can re-pin it. Native installations "automatically update in the background to
keep you on the latest version"; a background update "download[s] and install[s] in the
background, then take[s] effect the next time you start Claude Code." A supported way to
hold a version exists but was not in effect here: the `minimumVersion` setting pins a
floor, and `DISABLE_AUTOUPDATER=1` (in `env`) stops the background update check (`claude
update`/`claude install` still work; `DISABLE_UPDATES` blocks all update paths). (Source:
https://code.claude.com/docs/en/setup, "Update Claude Code" § "Auto-updates", "Pin a
minimum version", and "Disable auto-updates", at Claude Code `v2.1.283`, retrieved
2026-09-27.) None of these holds
was configured for the gate spikes: the G1 spike (2026-09-25) already found the
connecting client reporting `v2.1.282`, one version above the then-current fixed pin
`v2.1.274` (`docs/planning/gates/G1-result.md`, "Pin drift found during this spike"), and
the G4 re-run (2026-09-26) and G5 spike (2026-09-27) both observed `v2.1.283` one patch
above that (`docs/planning/gates/G4-result.md`, `docs/planning/gates/G5-result.md`).
Rather than keep re-litigating each patch bump as a one-off drift note, the operator
elected to float this row the same way the Codex row already floats (see
"Floating-version policy" under the Codex record below), moving from a fixed-version to
a last-observed-version policy. Consequences, binding on every Claude-side gate,
mirroring the Codex row's rule:

- Each gate result records the Claude Code version it **actually ran on**, as reported by
  all three available sources, mirroring the Codex row's "as reported by the CLI, the
  daemon, and the client's `clientInfo`": the CLI (`claude --version`), the wire
  `initialize` result's `clientInfo.version`, and the transport's user-agent string when
  the transport carries one (e.g. an HTTP-registered channel server). `clientInfo.version`
  alone is not sufficient — record the triple, or record which of the three were actually
  checked and why the others were unavailable.
- Any newly observed Claude Code version invalidates the Claude-side results that ran on
  an older version: G1, G4, and G5. They revert to `NOT RUN` until they are re-run against
  the new last-observed version. The pin-move checklist in this file applies whenever this
  row's last-observed version changes, and — per `docs/planning/gates/README.md` §a — a
  change invalidates only the verdicts whose own recorded observed version differs from
  the new last-observed version; a gate that already ran on the version now being
  recorded stays current.
- The §3.1 facts must be re-verified against each newly observed version before a
  Claude-side gate is re-run on it (`oac-evidence` §7). This re-verification has not yet
  been done at either `v2.1.282` or `v2.1.283` — see "Open questions carried into B2"
  below, unchanged by this policy change.
- Version history of this row: `v2.1.274` (2026-09-17T00:12:02Z; B1 pin), then `v2.1.282`
  (GitHub release tag `v2.1.282`, published 2026-09-24T18:38:05Z UTC, retrieved
  2026-09-27 via `gh api repos/anthropics/claude-code/releases/tags/v2.1.282`; observed
  connecting during G1, 2026-09-25), then `v2.1.283` (GitHub release tag `v2.1.283`,
  published 2026-09-25T21:50:12Z UTC, retrieved 2026-09-27 via `gh api
  repos/anthropics/claude-code/releases/tags/v2.1.283`; observed connecting during both
  the G4 re-run, 2026-09-26, and the G5 spike, 2026-09-27; **now the last-observed
  version**).
- **Environment check at this pin move:** `claude --version` on this host reported
  `2.1.283 (Claude Code)` on 2026-09-27, matching the new last-observed version above —
  recorded here the same way the Codex floating-pin move recorded a fresh environment
  check before moving the row.
- Applying the pin-move checklist to this change: G1 ran on `v2.1.282`, which does not
  equal the new last-observed `v2.1.283`, so **G1 is invalidated** and reverts to
  `NOT RUN` (see `docs/planning/gates/G1-result.md`'s Re-run history and
  `docs/planning/STATUS.md`). G4 and G5 both already ran on `v2.1.283` — the version this
  move records as last-observed — so their verdicts stay current; this move does not
  advance the last-observed version past what they already ran on, it only records in
  PINS.md the version they already ran against. G1 will be re-run in the same HIL sitting
  as the D6 Claude capture (issue #39 T6), under its own declared timebox, per
  `docs/planning/STATUS.md`.
- **Re-run closed, 2026-09-28 (issue #39 T6/T7):** G1 was re-run against the current
  last-observed version (`v2.1.283`) in two attempts, Box B (incomplete) and Box C
  (verdict-bearing) — **G1 PASSED**, operator decision 2026-09-28. G1 is no longer
  `NOT RUN`; the invalidation this pin move triggered is closed. Full evidence:
  `docs/planning/gates/G1-result.md`.

The B1 record below describes the original `v2.1.274` pin and is kept as history.

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
  *Dated note, 2026-10-02 (#122): closed as drift (D6).* The first-party changelog
  places "Added `--channels` (research preview) — allow MCP servers to push messages
  into your session" at `2.1.80` (`anthropics/claude-code` `CHANGELOG.md` @
  `52c76441cae91f6891e4712306bffb057ff6fec5`, retrieved 2026-10-02). `2.1.232` is not a
  channels version, so this floor is unsupported. The version does appear first-party,
  for something else: https://code.claude.com/docs/en/mcp.md L324 (retrieved
  2026-10-02) says that in sessions that fetch feature flags, Claude Code "uses the v2
  runtime on Claude Code v2.1.232 or later". That is the v2 MCP client runtime, which
  adds revision `2026-07-28` (L322) and to which the channel-negotiation constraint
  (L398, "Constraint floors" below) applies. The operative floor is the minimum version
  `v2.1.282` ("Version policy" above).
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
- Gates affected: **G1** (Claude wake — go/no-go, no fallback; **currently `NOT RUN`,
  stale after this pin move** — see "Floating-version policy" above), **G4** via the
  legacy-MCP negotiation constraint above, and **G5** (Provenance: STATUS.md's Gate
  verdicts table states G5's verdict depends on machine-set provenance rendering on
  both providers, so it depends on this pin, not only on the Codex pin). *Dated note,
  2026-10-01 (#216):* G1 was re-run and PASSED on `v2.1.283` (2026-09-28), and a Claude
  Code version change no longer invalidates G1, G4 or G5 ("Version policy" above).

### Codex CLI and app-server

#### Version policy (operator decision, 2026-10-01, #216)

The operator decision on #216 replaces the invalidation rules of the floating-version
policy below. The row still floats. It records two versions, and neither one gates
anything:

- **Minimum version: `@openai/codex@0.154.0`** (tag `rust-v0.154.0` → commit
  `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`). This is the first Codex version the project
  actually worked with: the original G2 run on 2026-09-25 ran on `codex-cli 0.154.0`, with
  the daemon's `cliVersion` / `appServerVersion` / `managedCodexVersion` also `0.154.0`.
  Sources: `docs/planning/gates/G2-result.md`, "Re-run history", first row ("2026-09-25 |
  Codex CLI / app-server `0.154.0` ... | PASS"); and
  `docs/planning/gates/fixtures/MANIFEST.json`, the `g2-codex-inject/transcript.jsonl`
  entry, `observed_version` `codex_cli` / `codex_daemon` `0.154.0`, `capture_date`
  2026-09-25.
- **Last tested version: `@openai/codex@0.161.0`** (GitHub release `rust-v0.161.0`,
  published 2026-10-07T15:58:45Z UTC; tag object
  `7e21416b38834816c224ea0dfd135c3de94b2f15` → commit
  `979011409de0a60b52f179721948e65531d26144`; retrieved 2026-10-07 via `gh api
  repos/openai/codex/releases/tags/rust-v0.161.0`, `gh api
  repos/openai/codex/git/ref/tags/rust-v0.161.0` and `gh api
  repos/openai/codex/git/tags/7e21416b38834816c224ea0dfd135c3de94b2f15`). Codex's own
  auto-updater installed it on 2026-10-07, between the S3 capture's development probe and
  its first herdr run (#343). It ran in the S3 fixture capture herdr run of 2026-10-07 (run
  `20261007T221317Z-1a3890`): `codex --version` `codex-cli 0.161.0`, all three daemon
  version fields and every wire `initialize` `userAgent` `0.161.0`, and the post-run
  check `0.161.0` (`docs/planning/gates/herdr-runs/S3-codex-2026-10-07.md` and its run
  manifest, `scenarioData.s3.versions`, `scenarioData.s3.postRun`). Update this field, and
  the version history below, after each live run.
- *Previous last tested version (2026-10-04 to 2026-10-07):* `@openai/codex@0.160.0` (GitHub release `rust-v0.160.0`,
  published 2026-10-01T20:19:13Z UTC; tag object
  `79b1b666f2e8551f8abbbca34957227f67f3f553` → commit
  `a956835d020762cb2b570053af06f643a11c0ecc`; retrieved 2026-10-04 via `gh api
  repos/openai/codex/releases/tags/rust-v0.160.0`, `gh api
  repos/openai/codex/git/ref/tags/rust-v0.160.0` and `gh api
  repos/openai/codex/git/tags/79b1b666f2e8551f8abbbca34957227f67f3f553`). It ran in the G4 herdr run on
  2026-10-04 (run `20261004T093525Z`, the recorded run; also in the superseded run
  `20261004T085601Z` of the same day): `codex --version` `codex-cli 0.160.0`, the MCP client
  user-agent and `clientInfo.version` `0.160.0`, and the post-run check `0.160.0`
  (`docs/planning/gates/herdr-runs/G4-2026-10-04.md` and its run manifest,
  `scenarioData.g4.versions`, `scenarioData.g4.postRun`). Update this field, and the version
  history below, after each live run.
- **Warn, never gate.** A version other than the last tested one, or below the minimum,
  is a `VERSION WARNING` finding (`tools/herdr/lib/pins.mjs` `codexVersionWarning`). This
  applies to `codex --version`, to each `codex app-server daemon version` field and to the
  wire `initialize` `userAgent` (and to the MCP client user-agent in G4). The warning
  never stops a run, never makes it `NOT RUN`, never blocks CI and never by itself
  invalidates a gate verdict. A gate result still records the version it actually ran
  on. The daemon's auto-updater may keep moving the version; that is expected.
- **Recorded verdicts stand.** G2 (PASS, `0.157.1`), G4 (PASS, `0.157.1`) and G5 (FAIL,
  `0.157.1`) keep their verdicts and the versions they ran on. *(Dated note, 2026-10-03,
  #220: G5 is now PASS, from its Codex-leg re-run on `0.160.0`. That is a `VERSION WARNING`
  against the last tested `0.159.3`, a finding only (#216). Recording `0.160.0` as last
  tested is a separate change, and this note changes no pin. See
  `docs/planning/gates/G5-result.md`.)* The 2026-09-26 invalidation
  of G2 is kept as history.
- **§3.2 facts.** Re-checking the §3.2 facts against a newly tested version
  (`oac-evidence` §7) is still worth doing. It is tracked as a finding and never gates a
  run. The last re-verification is at `0.157.1` (`docs/planning/REVERIFICATION-B2.md`).
- Version history (dated additions only): `0.158.0` and `0.159.2` in L3 probe runs
  (2026-09-30), and `0.159.3` in the L3 live leg (2026-10-01), all recorded in L1 §13.
  `0.159.3` was the last tested version from 2026-10-01. `0.160.0` in the G5 K8 herdr run
  (2026-10-02, `docs/planning/gates/herdr-runs/G5-2026-10-02.md`, fixtures
  `docs/planning/gates/fixtures/g5-provenance/k8-2026-10-02/*-0.160.0-herdr*`), the G5 E1
  Codex re-run (2026-10-02, `docs/planning/gates/herdr-runs/G5-c13-2026-10-02.md`) and the
  G4 herdr run
  (2026-10-04, `docs/planning/gates/herdr-runs/G4-2026-10-04.md`); `0.160.0` is the last
  tested version from 2026-10-04. `0.160.0` again in the G2 and G4 herdr re-records
  (2026-10-05, `G2-2026-10-05.md` and `G4-2026-10-05.md`; 2026-10-06, `G2-2026-10-06.md` and
  `G4-2026-10-06.md`, all under `docs/planning/gates/herdr-runs/`). On 2026-10-06 Codex
  offered `0.160.1`; it was skipped in Codex's own TUI, not installed or tested
  (`G4-2026-10-06.md` Findings), so the last tested version stays `0.160.0`. On
  2026-10-07 the S3 capture's development probe ran on `0.160.0`; Codex then updated itself
  to `0.161.0`, which both S3 capture herdr runs used: `20261007T220941Z-fe9722` (not
  recorded: its captures kept `unverified-*` names, a scenario defect fixed before the next
  run) and `20261007T221317Z-1a3890` (recorded,
  `docs/planning/gates/herdr-runs/S3-codex-2026-10-07.md`). `0.161.0` is the last tested
  version from 2026-10-07.

#### Floating-version policy (operator decision, 2026-09-26)

> **Superseded in part, 2026-10-01 (#216).** The row still floats. The rule below that
> any new release invalidates G2, G5 and G4's Codex leg, and the use of the pin-move
> checklist for this row, no longer apply: see "Version policy" above. The text is kept as
> history.

**This row no longer holds a fixed pin.** During gate G4 on 2026-09-25/26, a Codex
auto-updater moved the environment from `0.154.0` to `0.157.0`, and then to `0.157.1`
within about 24 hours. The updater is a detached `codex app-server daemon
pid-update-loop` process that is not started by OAC. The daemon README says: "Eligible
managed daemons check for updates after five minutes, then hourly by default." When it
updates, "the running server restarts with the new binary". (Source:
https://github.com/openai/codex/blob/36650394c5b38c2990ccf2a3457165ca3e9d9726/codex-rs/app-server-daemon/README.md,
lines 48 and 119, retrieved 2026-09-26.) The same README documents a supported way to hold
a version: when the "Installer selected an explicit release", then "the selected release
stays pinned" (line 120). The operator chose to **leave the updater running** and accept a
moving target rather than hold a pin; that choice was made before the explicit-release
option was known. npm also shows releases `0.155.0`, `0.155.1`, `0.156.0` and `0.156.1`
(2026-09-17 to 2026-09-23). So the G2 run on 2026-09-25 was already behind the newest
release, although it matched the B1 pin at the time. Consequences, binding on every
Codex-side gate:

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
  `36650394c5b38c2990ccf2a3457165ca3e9d9726`; **G2 re-run PASS on it, 2026-09-26**). All
  retrieved 2026-09-26 via `npm view` and the GitHub API.

The B1/B2 record below describes the original `0.154.0` pin and is kept as history. Its
§3.2 facts were re-verified against `0.157.1` on 2026-09-26 — see
`docs/planning/REVERIFICATION-B2.md` §"§3.2 re-verification at Codex `0.157.1`
(floating-pin trigger, 2026-09-26)": no drift affecting any fact G2's PASS rested on.
Two additive drifts were found (a new `--no-daemon` opt-out flag, which includes a
matching rejection added to `codex-rs/tui/src/session_queue_commands.rs`; and the opt-in
`mcp_2026_07_28` MCP client mode), both off by default and found only by fetching source
directly after a `gh compare` 300-file cap hid them from the diff; neither was exercised
by G2. One behavior-preserving rename was also found (`accept_hdr_async` →
`accept_hdr_async_with_config`). Runtime confirmation on Windows is
`docs/planning/gates/G2-result.md`'s 2026-09-26 re-run.

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
- Gates affected: **G2** (Codex live inject), **G5** (Provenance). *Dated note,
  2026-10-01 (#216):* G4's Codex leg was added 2026-09-26, and a Codex version change no
  longer invalidates any of them ("Version policy" above).

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
  re-verification input, not resolved here. *(Dated note, 2026-10-03, #257: the rule is
  stated in the MCP base specification, "General fields" → "`_meta`", at both revisions;
  see the dated correction under the open item below.)*
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
- **Dated note, 2026-10-08 (#7, `docs/planning/decisions/G-7-stage4-dependencies.md`).** Pin
  unchanged. Stage 4 form: `rmcp = "=3.4.0"`, `default-features = false`, features `server`
  and `transport-async-rw` only (so `macros`, the `rmcp-macros` proc-macros, is refused),
  consumed by `adapters/mcp-tools/` and both adapters (the adapter contract suite's
  `VETTED_DEPENDENCIES`, G-7 §5), no longer by `cli/` (`mcp-shim`). Feature map and licence
  re-read 2026-10-08 from https://crates.io/api/v1/crates/rmcp/3.4.0 (published
  2026-09-15T15:44:08Z, `license` Apache-2.0). The current release is `3.5.1` (2026-10-05);
  it is not taken, because a move of this row reverts G1 and G4 under the checklist above
  although neither run exercised `rmcp` (G-7 §3.1). A later move is the lead's call, with the
  checklist.

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
- **Dated note, 2026-10-08 (#7, G-7 §8.1).** Not recommended for use: the recommendation is
  `tokio`'s named pipes and Unix sockets, with peer PID from `tokio`'s `peer_cred()` and
  `windows-sys`. G9 (#70) confirms. The row stays as the C2 candidate until then.

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

- **Dated note, 2026-10-08 (#7, G-7 §3.1, §3.3).** Pin unchanged. Stage 4 form:
  `zenoh = "=1.10.1"`, `default-features = false`, features `transport_tcp` and
  `transport_tls` only: no `unstable`, no `shared-memory`, no other transport. crates.io
  (https://crates.io/api/v1/crates/zenoh/1.10.1, retrieved 2026-10-08) records 1.10.1 as
  published 2026-09-07, license `EPL-2.0 OR Apache-2.0`, `rust_version` 1.75.0, matching the
  GitHub release above; the open crates.io cross-check item is G1's to close. Its resolved
  graph brings licences beyond the earlier accepted list (Zlib, ISC, BSD-2-Clause,
  CDLA-Permissive-2.0, `Apache-2.0 AND ISC`, and MPL-2.0 for `option-ext` 0.2.0), accepted
  by the lead's licence decisions of 2026-10-08 (07 §5 "Accepted licenses"). Its TLS link
  resolves to `rustls` 0.23.45 with `ring` 0.17.14.

### `tokio` (async runtime)

- Surface label: **supported** — general-purpose async runtime, not a provider surface.
- Pinned crate version: `1.53.2` (`=1.53.2`). Published 2026-10-03, license MIT. Source:
  https://crates.io/api/v1/crates/tokio/1.53.2, retrieved 2026-10-08.
- Features: per consumer. In `adapters/*` and `adapters/mcp-tools/` only those `rmcp` 3.4.0
  enables on it (`sync`, `macros`, `rt`, `time`, and `io-util` through
  `transport-async-rw`; `rmcp-3.4.0/Cargo.toml` L840-L847), enforced by the adapter contract
  suite's `VETTED_DEPENDENCIES` (G-7 §5). `cli/` and `transports/zenoh/` record theirs in
  their tasks.
- Decision: lead, in chat 2026-10-08 (G-7 §1, §5).
- **Gates affected: none directly** — implementation dependency.

### `rcgen` (local-mode TLS certificate)

- Surface label: **supported**.
- Pinned crate version: `0.14.10` (`=0.14.10`), `default-features = false`, features `pem`
  and `ring` (the ring backend, lead decision 2026-10-08). Published 2026-08-28, license
  MIT OR Apache-2.0 (Apache-2.0 elected). Source:
  https://crates.io/api/v1/crates/rcgen/0.14.10, retrieved 2026-10-08.
- Use: the automatically generated local-mode TLS certificate (C7 §5). Consumer
  (`transports/zenoh/` or the daemon's state code in `cli/`) decided by G3 (#64).
- **Gates affected: none directly** — implementation dependency.

### `windows-sys` (named-pipe peer PID)

- Surface label: **supported**.
- Pinned crate version: `0.61.2` (`=0.61.2`), features `Win32_Foundation`,
  `Win32_System_Pipes`. Published 2025-10-06, license MIT OR Apache-2.0 (Apache-2.0
  elected). Source: https://crates.io/api/v1/crates/windows-sys/0.61.2, retrieved
  2026-10-08. Already in the workspace `Cargo.lock` at this version.
- Use: `GetNamedPipeClientProcessId` (`src/Windows/Win32/System/Pipes/mod.rs` L14 at
  0.61.2) for local IPC peer auth in `cli/` (G-7 §8.1; G9 #70 confirms).
- **Gates affected: none directly** — implementation dependency.

### `libc` (Unix socket peer credentials)

- Surface label: **supported**.
- Pinned crate version: `0.2.190` (`=0.2.190`), already a Unix dependency of `cli/` and in
  the workspace `Cargo.lock` (07 §5, #52 dated note). License MIT OR Apache-2.0 (Apache-2.0
  elected), from the lock's resolve, retrieved 2026-10-08.
- Use: Unix socket peer credentials where `tokio`'s `peer_cred()` does not cover a need:
  `SO_PEERCRED` on Linux, `LOCAL_PEEREPID`/`LOCAL_PEERPID` on macOS
  (`src/unix/bsd/apple/mod.rs` L3172 at 0.2.190) (G-7 §8.1).
- **Gates affected: none directly** — implementation dependency.

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
- *Dated note, 2026-10-06 (#49): both open items above are re-checked against the
  protocol's source repository,
  https://github.com/agentclientprotocol/agent-client-protocol at commit
  `487ad3eacd30bb19f75f462f815e78d678e393c4` (latest schema release `schema-v1.24.1`),
  retrieved 2026-10-06. `docs/protocol/v1/initialization.mdx` shows
  `"protocolVersion": 1` in its request and response examples, so the pin holds and is no
  longer UNVERIFIED. Schema v2 "alpha" is verified, with no drift:
  `docs/announcements/acp-v2-draft.mdx` L70 says the v2 JSON schemas are "published in the
  repository releases as `v2.0.0-alphaX` alongside v1", and the latest is the prerelease
  `schema-v2.0.0-alpha.7` of 2026-09-30 (https://github.com/agentclientprotocol/agent-client-protocol/releases/tag/schema-v2.0.0-alpha.7).
  Separately, the v2 protocol docs have been "in Draft" since July 20, 2026 (same page,
  L1-L11). v1 stays the supported version, so the pin does not move. Detail:
  `docs/planning/decisions/E9-replacement-proofs.md` §1.3.*
- **ACP is not a v0.1 dependency.** A drift in this pin does not invalidate any gate.
  This row is kept (not dropped) because STATUS.md already carries an ACP row as a
  tracked baseline; dropping it here would lose that baseline.
- Gates affected: none.

### herdr (test tooling)

- Surface label: **supported**. herdr's own first-party CLI reference documents its
  `agent`, `pane`, `session` and `server` commands. herdr is Epic K test tooling that
  drives real harness CLIs. It is **not** an OAC provider surface and **not** a shipped
  dependency. It is never imported by, linked into, or invoked from `adapters/`,
  `core/`, `cli/`, `transports/` or `spec/` (Epic K #123; K2's containment lint).
- Pinned release: **`v0.9.1`**. It is the annotated tag `v0.9.1` (tag object
  `8544776216a8d28088db59a5344ea21ee2d05d2b`, tagger timestamp 2026-09-16T18:30:33Z)
  pointing to commit `065ef9d6a531c49fb8bee7e818ef837065b21ee9` ("release: v0.9.1").
  GitHub labels it **Latest**, published Sep 16. `Cargo.toml` at the tag reads
  `version = "0.9.1"`. Source: https://github.com/herdrdev/herdr/releases/tag/v0.9.1,
  https://github.com/herdrdev/herdr/releases, and `git ls-remote --tags
  https://github.com/herdrdev/herdr`, retrieved 2026-09-28. The only releases newer than
  it on that date are `preview-*` prereleases, which are not pin candidates.
- **Fixed, not floating.** herdr "checks for new releases and notifies you in the app".
  Updating is the manual `herdr update`, and there is no background self-install
  (https://github.com/herdrdev/herdr/blob/v0.9.1/docs/next/website/src/content/docs/install.mdx
  L116-120, tag `v0.9.1`, retrieved 2026-09-28). So unlike the Claude Code and Codex
  rows, this row can hold a fixed version. K3's driver enforces it by refusing any
  other `herdr --version`.
  **Caveat:** the binary pin does not fix herdr's agent-detection manifests. Those
  update from herdr.dev at runtime unless `update.manifest_check = false`
  (`docs/next/website/src/content/docs/agents.mdx` L67 at the same tag). See
  `docs/planning/decisions/K1-herdr-evaluation.md` §2.
- License: **Apache-2.0**. Source: https://github.com/herdrdev/herdr/blob/v0.9.1/LICENSE
  and https://github.com/herdrdev/herdr/blob/v0.9.1/Cargo.toml (`license =
  "Apache-2.0"`), retrieved 2026-09-28. It is recorded as dev/test tooling, not shipped,
  in `docs/planning/v0.1/07-repository-and-dependencies.md` §5.
- **Live behavior verified on Windows only.** K1's live leg ran on native Windows on
  2026-09-28 (herdr 0.9.1, Claude Code 2.1.283, Codex 0.158.0) and hit no no-go condition:
  named-session start, `agent start` argv/cwd, the timeout options, dialog readability,
  Codex's post-response state and the launch-environment delta were observed. Linux and
  macOS (herdr's documentation claim only) are UNVERIFIED, so K1's overall go/no-go is
  **provisional**. See `docs/planning/decisions/K1-herdr-evaluation.md` §5-§7 and
  `docs/planning/STATUS.md` "Open UNVERIFIED items".
- **Gates affected: none.** No `G<n>-result.md` verdict depends on this row. Human-operated
  gate spikes remain the authoritative verification method (Epic K #123). A move of
  this row therefore invalidates no gate verdict. It does re-open K1's live leg for the
  new version and requires K3's driver version check to be updated in the same change.
  It also invalidates every herdr equivalence record, in the same commit
  (`docs/planning/gates/README.md` §f, "Scripted runs (herdr)").
- **Expected herdr executable (#252, 2026-10-03).** The driver hashes the herdr it
  resolved and compares the hash with this table's row for its platform
  (`process.platform`-`process.arch`) before it spawns herdr (`tools/herdr/lib/pins.mjs`
  `checkHerdrExecutable`, run manifest `herdr.executableCheck`). A different hash is
  `NOT RUN`. A platform with no row is a finding, and the run's herdr identity is UNVERIFIED.
  A match counts as VERIFIED only on a row whose `First-party` cell is `yes`. A match on a
  `no` row (none at present) would show only that the binary is a locally observed one, so
  the record would state herdr UNVERIFIED (first-party source UNVERIFIED).
  The table is read from HEAD, like the tag. A pin move updates it in the same change.
  Sources, all retrieved 2026-10-03:
  - Asset digests: the GitHub release API `digest` field for tag `v0.9.1`
    (`gh api repos/herdrdev/herdr/releases/tags/v0.9.1`).
  - Release attestation: herdr publishes a *release* attestation, an in-toto Statement over
    the release's assets. It does not publish SLSA build provenance. So
    `gh release verify-asset v0.9.1 <file> --repo herdrdev/herdr` is the check that applies.
    `gh attestation verify` looks for build provenance by digest, and for herdr it returns
    `HTTP 404: Not Found`, as it did for the installed `herdr.exe` on 2026-10-03. That 404
    means there is no build-provenance attestation. It says nothing about the release
    attestation. `gh api repos/herdrdev/herdr/attestations/sha256:<digest>` returns one
    attestation for each of the five asset digests.
  - Windows (verified 2026-10-03 by the orchestrator, with the operator's permission; the
    downloaded files were deleted afterwards):
    1. `gh release download v0.9.1 --repo herdrdev/herdr --pattern herdr-windows-x86_64.zip`
       gave a zip with sha256 `04ce380c…a6e`, equal to the API asset digest.
    2. `gh release verify-asset v0.9.1 herdr-windows-x86_64.zip --repo herdrdev/herdr`
       printed "Verification succeeded! herdr-windows-x86_64.zip is present in release
       v0.9.1". That is the release attestation; the tag `v0.9.1` is sha1
       `8544776216a8d28088db59a5344ea21ee2d05d2b`.
    3. `herdr.exe` inside the zip hashes to `007781…9b6`, byte-identical to the installed
       binary.
  - All five assets, Linux and macOS included (verified 2026-10-03, no download):
    `gh release verify v0.9.1 --repo herdrdev/herdr` resolved tag `v0.9.1` to sha1
    `8544776216a8d28088db59a5344ea21ee2d05d2b`, loaded the release attestation from the
    GitHub API, and printed "Release v0.9.1 verified!". It lists `herdr-linux-aarch64`
    `f4ccf4de…`, `herdr-linux-x86_64` `2a02fed1…`, `herdr-macos-aarch64` `5fc7a7e7…`,
    `herdr-macos-x86_64` `053be063…` and `herdr-windows-x86_64.zip` `04ce380c…`. Each
    equals that row's asset digest below. For Linux and macOS the asset is the executable,
    so the expected value is attested directly. No run has yet hashed an installed herdr on
    those platforms. A mismatch there would be `NOT RUN`, and a finding to look into.

| Platform | Release asset | Asset digest (sha256) | Expected executable sha256 | First-party | Basis |
|---|---|---|---|---|---|
| `win32-x64` | `herdr-windows-x86_64.zip` | `04ce380cac5af27bfcf75d0951ac49b7afe4c984aee8852985806d4f71f93a6e` | `007781224360a8bdd1d1a35d34c08c11db3cc3c7132769cffea795869d36b9b6` | yes | First-party, verified 2026-10-03 in the three steps listed above. The release zip hashes to the API digest. `gh release verify-asset` confirmed it against the release attestation for tag `v0.9.1` (sha1 `8544776216a8d28088db59a5344ea21ee2d05d2b`). Its `herdr.exe` hashes to this value. The same value is the sha256 of the installed `herdr.exe` (25562624 bytes, standalone package `0.9.1-x86_64-pc-windows-msvc`) and of the herdr the driver recorded in run `G5-2026-10-02` (`herdr.executable.sha256`). |
| `linux-x64` | `herdr-linux-x86_64` | `2a02fed16beb651ef006e1d43f048f652ca4dc58ad053cd2d44450563d5c54b7` | `2a02fed16beb651ef006e1d43f048f652ca4dc58ad053cd2d44450563d5c54b7` | yes | The release's own asset digest, listed under the `v0.9.1` release attestation by `gh release verify v0.9.1 --repo herdrdev/herdr` (2026-10-03, above). The asset name has no archive extension, so it is taken to be the bare executable. No run has yet hashed an installed herdr on this platform; a mismatch there is `NOT RUN` and a finding to look into. |
| `linux-arm64` | `herdr-linux-aarch64` | `f4ccf4de745f2cb9a39a983e9ba3703dad50ec2a58dea83026ceab721bbd8d9e` | `f4ccf4de745f2cb9a39a983e9ba3703dad50ec2a58dea83026ceab721bbd8d9e` | yes | As `linux-x64`. |
| `darwin-x64` | `herdr-macos-x86_64` | `053be0639935fe54ab5efbdb46651054e4f6a753a5b43153c88bd6912bce1e94` | `053be0639935fe54ab5efbdb46651054e4f6a753a5b43153c88bd6912bce1e94` | yes | As `linux-x64`. |
| `darwin-arm64` | `herdr-macos-aarch64` | `5fc7a7e7adfaca56fa80aa89dcb025693357268dab8285b9ce2d08a2313c89de` | `5fc7a7e7adfaca56fa80aa89dcb025693357268dab8285b9ce2d08a2313c89de` | yes | As `linux-x64`. |

### Beacon (external memory service)

- Surface label: **supported**. Beacon's own docs at the tag document its local MCP
  server (`beacon mcp serve`, server name `beacon`) and its `beacon memory` CLI, with no
  preview or experimental marker. Beacon is an external memory service that each harness
  connects to natively over MCP. It is **not** an OAC provider surface and **not** a
  shipped dependency. OAC never imports, links, vendors, spawns, configures or calls it,
  and no Beacon reference appears under `adapters/`, `core/`, `cli/`, `transports/` or
  `spec/` (`docs/planning/decisions/L1-beacon-memory.md` §1).
- Pinned release: **`v1.3.29`**. It is the annotated tag `v1.3.29` (tag object
  `72fd6643b5cd5c6ff6741f6016b3577654f61915`, tagger timestamp 2026-09-28T13:43:29Z)
  pointing to commit `91e92216b79108475ba9b587d49c5ff3f7356fd8`. No tag above it exists
  on the retrieval date. The release date is the tagger date; the GitHub release page
  itself was not reachable from the session that wrote L1 and is not cited. Source:
  `git ls-remote --tags https://github.com/Asymptote-Labs/agent-beacon` and a clone of
  tag `v1.3.29`, retrieved 2026-09-29. Issue #165's facts were read at the newer, unreleased branch head
  `26581914e86f525e225096613c3a9b808043ff85`; L1 re-read every cited doc at the tag and
  records two drifts (L1 §2).
- **Fixed, not floating, while self-updates stay off.** Beacon is upgraded by the
  operator through a package manager (`brew upgrade beacon`,
  https://github.com/Asymptote-Labs/agent-beacon/blob/v1.3.29/docs/get-started/quickstart.mdx
  L110, retrieved 2026-09-29). Beacon also ships package self-updates for Apple Silicon
  system-package installs, off by default and opt-in (`check-only` or `auto`): "Endpoint
  package self-updates are available for Apple Silicon system package installs, but
  remain off by default"
  (https://github.com/Asymptote-Labs/agent-beacon/blob/v1.3.29/docs/mdm/index.mdx L10,
  commit `91e92216b79108475ba9b587d49c5ff3f7356fd8`, retrieved 2026-09-29). This row
  holds a fixed version only while those self-updates stay off; operators should leave
  them off (L1 §2). It ships often (`v1.3.27`-`v1.3.29` are consecutive recent
  tags), so this row is expected to move; see `RISK-BEACON` in
  `docs/planning/v0.1/11-risks.md`.
- License: **MIT** ("Copyright (c) 2026 Asymptote Labs"). Source:
  https://github.com/Asymptote-Labs/agent-beacon/blob/v1.3.29/LICENSE, retrieved
  2026-09-29. It is recorded as an external service, not shipped, in
  `docs/planning/v0.1/07-repository-and-dependencies.md` §5.
- **Four facts were UNVERIFIED at this pin; none remains after L3** (L1 §6,
  `docs/planning/STATUS.md` "Open UNVERIFIED items"): capture of OAC-delivered input, memory ID and result schema
  stability, concurrent `memory.db` access, and Codex config interaction. L2 owns them.
  **L2 (issue #167, 2026-09-29, L1 §11):** the last three are closed from source at this
  tag (CONFIRMED, CONFIRMED, REFUTED); the first stays UNVERIFIED, narrowed to harness
  behaviour, for the live leg (L1 §12; herdr-driven per #187, 2026-09-30).
  **L3 (issue #192, 2026-10-01, L1 §13 B2-B4):** the first, U1, is CONFIRMED live on
  Windows with Beacon `1.3.29` (this pin), Claude Code `2.1.285` and Codex `0.159.3`, so
  all four items are closed. (Note added 2026-10-01, issue #211; it changes no version,
  fixed/floating status or `Gates affected` cell, and invalidates no gate verdict.) A
  move of this row also requires L1 §11's source-level facts to be re-read at the new
  tag.
- **Gates affected: none.** No `G<n>-result.md` verdict depends on this row. A move of
  this row invalidates no gate verdict. It does require L1 §6's items and every L1 §10
  citation to be re-read at the new tag, and the L3 live leg to record the Beacon version
  it ran on in L1 §13 (L2 commits no fixtures).

## Constraint floors

Each floor below is independently checkable by a reader who has only this file open.

*Dated note, 2026-10-01 (#216):* the two Claude Code floors below record when the
capability appeared. The operative floor for the project is the row's **minimum version**,
`v2.1.282`, the first version the project worked with ("Version policy" under Claude Code
Channels). It is above both floors. Like every harness version check, it warns and never
gates.

- **Claude Code `>= v2.1.232`** — channels exist at all. Reason: Channels ship as a
  research-preview feature starting at this release (Source:
  https://code.claude.com/docs/en/channels.md, "Stability: research preview on Claude
  Code v2.1.232+"). Consequence of violating: `--channels` and
  `--dangerously-load-development-channels` do not exist below this version; the
  Claude adapter cannot be built at all. *(Dated note, 2026-10-02, #122: drift D6. The
  first-party changelog dates "Added `--channels` (research preview)" to `2.1.80`, so
  `2.1.232` is not a channels version. The quotation above is PLANNING-PROMPT.md §3.1's
  wording, not `channels.md`'s. `mcp.md` L324 does name `2.1.232`, as the version from
  which sessions that fetch feature flags use the v2 MCP client runtime ("uses the v2
  runtime on Claude Code v2.1.232 or later"). That runtime adds `2026-07-28` and is the
  one the `2025-11-25` floor below guards against (retrieved 2026-10-02). This floor
  binds nothing: the minimum `v2.1.282` is above both.)*
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
  *Dated correction, 2026-10-03 (#257, from PR #255 / #46): D2 holds for SEP-2133's own
  text only. The MCP base specification states the reservation for `_meta` key prefixes
  at both pinned revisions: "Any prefix where the second label is `modelcontextprotocol`
  or `mcp` is **reserved** for MCP use." Source:
  https://modelcontextprotocol.io/specification/2025-11-25/basic and
  https://modelcontextprotocol.io/specification/2026-07-28/basic, section "General
  fields" → "`_meta`", MCP revisions `2025-11-25` and `2026-07-28`, retrieved
  2026-10-03. At `2026-07-28` it binds extension identifiers too: "Extension identifiers
  **MUST** follow the `_meta` key naming rules, with a mandatory prefix." Source:
  https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning, section
  "Extension Negotiation", retrieved 2026-10-03. §3.3's rule is therefore supported, with
  the MCP base specification as its source rather than SEP-2133. No pin moves, and OAC's
  identifier `io.github.rossgraeber/oac-session-channels` (second label `github`) is
  unaffected. The `oac-mcp` Pin section carries the same correction.*
- ACP schema v2 "alpha" status was not independently re-confirmed on
  https://agentclientprotocol.com/protocol/ as fetched today; it is carried forward
  from PLANNING-PROMPT.md §3.5 only (UNVERIFIED — re-check against
  https://agentclientprotocol.com/protocol/ or its schema changelog directly; low
  priority since ACP is not a v0.1 dependency).
  **CARRIED — risk item 7, see REVERIFICATION-B2.md "Carried to 11-risks.md".**
  Re-checked in B2 (still absent from the page); low priority, not a v0.1 dependency.
  *(Dated note, 2026-10-06, #49: closed as verified, no drift. The v2 schemas are
  `v2.0.0-alphaX` prereleases (latest `schema-v2.0.0-alpha.7`); the v2 protocol docs are
  Draft. See the "ACP" section's dated note.)*
- Zenoh crate version/date were read from the GitHub releases page rather than
  directly from crates.io's rendered page, because the crates.io fetch did not return
  page content in this session (UNVERIFIED — re-confirm directly on
  https://crates.io/crates/zenoh when that page is reachable; GitHub releases is
  first-party for the same project and is not expected to disagree).
  **CARRIED — risk item 8, see REVERIFICATION-B2.md "Carried to 11-risks.md".** Not
  re-attempted in B2; GitHub Releases remains the source of record.
