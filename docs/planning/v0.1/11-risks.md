# 11 — Risks

This file is self-contained: every citation below is a repo-relative path plus
section, per `.claude/skills/oac-planning-package/SKILL.md` §2. Gate pass/fail/
fallback text stays owned by `docs/planning/v0.1/02-gating-findings.md`; stage
ordering and timeboxes stay owned by `docs/planning/v0.1/10-stages.md`; threat
rows stay owned by `docs/planning/v0.1/06-security.md`; license findings stay
owned by `docs/planning/v0.1/07-repository-and-dependencies.md`. This file does
not restate any of that text, only cites it.

## Ranking rule

Risks are ranked by **ability to invalidate v0.1**, judged against two fixed
references:

- `docs/planning/ADR-001.md` line 74's validation criterion — v0.1 is proven when
  an existing Claude Code session sends a message, over OAC Session Channels and
  Zenoh, to an existing Codex harness session without receiver polling, Codex
  responds and Claude receives the response actively, neither side invokes or
  holds credentials for the other's model API, and sender identity/
  authorization are enforceable rather than inferred from content (source text
  at that line still reads pre-rename "Session Channels/Zenoh," per
  `docs/planning/STATUS.md`'s note that `ADR-001.md`'s body is unchanged; this
  file uses the ADR-001-A1 name).
- `docs/planning/DESIGN.md` "v0.1 acceptance criteria" 1-10 (verbatim list
  reproduced, not re-derived, in `docs/planning/v0.1/10-stages.md` §11).

A reader can re-derive the tier order below from one rule: **a risk that removes
an ADR-001-required leg of the validation criterion, with no fallback, outranks
a risk that only degrades a preview surface, which outranks a risk that is only
a citation-source or pin-provenance gap.**

- **R1 — gate-decided viability risks.** G1, G2, G3, G4, G5
  (`docs/planning/v0.1/02-gating-findings.md` §3-§7). Each gate decides a named leg of
  the ADR-001 validation criterion or a DESIGN acceptance criterion outright; a `NOT RUN`
  or `FAIL` verdict on any of them blocks that leg (G2 and G4 have since run and
  PASSED — `docs/planning/gates/G2-result.md`, `G4-result.md`; G1 PASSED on Claude Code
  `v2.1.282` but is now `NOT RUN` for the current environment — the Claude Code
  (Channels) pin went floating 2026-09-27, last observed `v2.1.283` — see
  `docs/planning/gates/G1-result.md`; G3 is
  partial; G5 has run and recorded **FAIL** (Codex criteria 2/3 f; Claude all criteria
  x), `docs/planning/gates/G5-result.md`); G1 and G5 have no fallback. Per
  `docs/planning/v0.1/10-stages.md` §5's Gate S1 acceptance criterion 1, a `FAIL` is a
  closed verdict, so G5's `FAIL` does not itself block Stage 1's own exit; per §5's
  go/no-go condition and §2, it stops the pipeline at Stage 2's interface freeze on the
  Codex-provenance leg of the ADR-001 validation criterion until conflict-register entry
  C13 lands — unlike a G1 `FAIL`, which stops the whole pipeline outright, with no
  fallback and no path past it. The row order
  within R1 below (G1, G2, G4, G3, G5) is presentation order, following
  `02-gating-findings.md`'s own §3-§7 sequence — it is not itself a ranking;
  both no-fallback gates (G1, G5) carry equal weight regardless of position.
- **R2 — preview/experimental surface drift.** The Claude Code Channels research
  preview and the Codex experimental live-inject surface, including their
  unnamed compatibility-shim boundaries (conflict-register C11,
  `docs/planning/v0.1/03-decisions-and-amendments.md` §4). These surfaces are
  load-bearing for the R1 gates but are not themselves gate verdicts — they are
  the ground the gates stand on.
- **R3 — evidence/pin drift.** Facts pinned at a specific version or schema era
  that a future pin move or re-fetch could silently invalidate, without changing
  the gate mechanics themselves.
- **R4 — design-parameter and platform-runtime risks.** OAC's own chosen
  parameters (above a stated floor) and platform-specific runtime behaviour not
  yet exercised live. These affect Stage 3/4 implementation quality, not v0.1
  viability.
- **R5 — low-impact / non-dependency risks.** Facts that inform packaging,
  citation hygiene, or transports/protocols not in the v0.1 dependency set.
  Ranked last because nothing in the ADR-001 validation criterion or the ten
  DESIGN acceptance criteria depends on them.

Every risk row below carries four mandatory fields — **risk**, **what it
invalidates**, **early-warning signal**, **response** — and every response names
a trigger-to-action link, not a bare mitigation, per the issue #32 acceptance
list.

## R1 — Gate-decided viability risks

### RISK-G1 — Claude wake fails

- **Risk.** The Claude Code Channels research-preview surface does not wake an
  idle session as a user turn, or loses mid-turn message ordering, or the
  `--dangerously-load-development-channels` consent dialog cannot actually be
  exercised.
- **What it invalidates.** `docs/planning/ADR-001.md` line 74's validation
  criterion (the Claude-sends-and-Codex-receives leg depends on Claude waking at
  all); `docs/planning/DESIGN.md` acceptance criteria 4 and 6; the Claude adapter
  (`adapters/claude/`) entirely.
- **Early-warning signal.** G1 spike (task D1) records a `FAIL` against any of
  its five pass criteria in `docs/planning/gates/G1-result.md`, per
  `docs/planning/v0.1/02-gating-findings.md` §3.
- **Response.** None — go/no-go, no fallback. Per
  `docs/planning/v0.1/02-gating-findings.md` §3, failure "blocks the Claude
  adapter entirely," and per `docs/planning/v0.1/10-stages.md` §2, "a FAIL on G1
  or G5 does not reorder the pipeline — it stops it."

### RISK-G2 — Codex live inject fails

- **Risk.** Neither the implicit daemon-attach path nor the `codex --remote`
  fallback delivers an actively-injected message into a live Codex session
  without OAC holding OpenAI credentials.
- **What it invalidates.** `docs/planning/DESIGN.md` acceptance criteria 5 and 8;
  the Codex-responds leg of `docs/planning/ADR-001.md` line 74's validation
  criterion; the Codex adapter (`adapters/codex/`).
- **Early-warning signal.** G2 spike (task D2) records a `FAIL` on the primary
  (implicit attach) path in `docs/planning/gates/G2-result.md`, per
  `docs/planning/v0.1/02-gating-findings.md` §4.
- **Response.** Run the named fallback before recording the gate result — OAC
  owns the app-server, user runs `codex --remote ws://…`
  (`docs/planning/v0.1/02-gating-findings.md` §4). Failure of both paths is v0.1
  go/no-go and stops the pipeline per `docs/planning/v0.1/10-stages.md` §2.

### RISK-G4 — MCP dual-era fails

- **Risk.** One process cannot serve both the legacy (`2025-11-25`) Claude-
  channel path and the current (`2026-07-28`) MCP path concurrently without one
  degrading the other, or the `rmcp` SDK does not register a legacy-era live
  channel at runtime (`docs/planning/STATUS.md` "Open UNVERIFIED items" —
  "`rmcp`-based OAC server ... registers as a channel").
- **What it invalidates.** `docs/planning/DESIGN.md` acceptance criterion 1's
  ("One-command local startup") single-process assumption — `docs/planning/
  v0.1/02-gating-findings.md` §6 states G4 failure is "a failure of the
  single-process design," naming no criterion; criterion 1 is the nearest
  stated match, not criterion 9 (Zenoh containment, unrelated); Decision 3's
  dual-era element
  (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 3); conflict-
  register row C5.
- **Early-warning signal.** G4 spike (task D4) records a `FAIL` against any of
  its five pass criteria — including the legacy-channel-registration criterion —
  in `docs/planning/gates/G4-result.md`, per
  `docs/planning/v0.1/02-gating-findings.md` §6.
- **Response.** Take the fallback — two server entry points sharing one core —
  and record the fallback's cost (extra process, duplicated negotiation code) in
  the gate result (`docs/planning/v0.1/02-gating-findings.md` §6).
- **Status (2026-09-26, issue #37/D4): the core single-process question is closed by G4
  PASS; RISK-G4 remains open for the `rmcp` and Codex-modern legs.** A first spike
  (2026-09-25/26) confirmed all five pass criteria individually against a live Claude
  Code session and a live Codex `0.157.1` process, none failed, but its confirming
  evidence was gathered after its declared 120-minute timebox had already expired, so it
  recorded `NOT RUN` per `oac-gates`' timebox policy rather than a `PASS`
  (`docs/planning/gates/G4-result.md`, superseded section). A fresh, redeclared
  60-minute timebox (declared before any live work; the operator closed the prior
  session, its servers exited, and the transcript was archived first) closed at ~8.5
  minutes elapsed, not expired, and reconfirmed all five criteria the same way
  (criteria 2-3 via Claude as the modern client; Codex legacy-only), including the
  modern HTTP path proven correct again *after* interleaved Codex/legacy traffic (the
  direct no-degradation check). **Verdict: PASS, no fallback needed.** This
  risk's core question — can one process serve both eras without degradation — is
  closed. This risk stays open for two legs it also names: whether an `rmcp`-based
  server (not the hand-rolled Node.js spike server used in both runs) shows the same
  behavior (`docs/planning/STATUS.md`), and Codex's modern-era (`2026-07-28`) leg, which
  Codex
  `0.157.1` never negotiated in either run — still tracked under this risk id at
  traceability row 41 below, even though the risk's core viability question is closed.

### RISK-G3 — Zenoh loopback discovery fails

- **Risk.** Multicast-scouting loopback peer discovery fails on Windows, macOS,
  or Linux at the pinned Zenoh `1.10.1`.
- **What it invalidates.** `docs/planning/DESIGN.md` acceptance criteria 1 and 3;
  the zero-container local deployment story
  (`docs/planning/v0.1/08-cli-and-deployment.md` §2).
- **Early-warning signal.** G3 spike (task D3) records a `FAIL` for a platform's
  multicast path in `docs/planning/gates/G3-result.md`, per
  `docs/planning/v0.1/02-gating-findings.md` §5.
- **Response.** Take the fallback per failing platform — fixed local rendezvous
  endpoint, multicast scouting disabled — recorded as `PASS (FALLBACK TAKEN)`
  for that platform (`docs/planning/v0.1/02-gating-findings.md` §5).
- **Status (2026-09-25).** The G3 spike (task D3) found no FAIL on any platform that
  ran. Windows 11 PASS and Linux (WSL2) PASS on the primary multicast path. macOS was
  NOT RUN because the leg is parked. The Rust-crate build is also untested: the spike
  ran the `eclipse-zenoh` Python wheel on the same core. So this risk stays open for
  macOS and for the Rust crate (`docs/planning/gates/G3-result.md`).

### RISK-G5 — Provenance fails

- **Risk.** Sender provenance can be forged from message content on Claude or
  Codex, or a non-identifier-safe `meta` key silently carries a security-
  relevant attribute.
- **What it invalidates.** `docs/planning/DESIGN.md` acceptance criterion 6
  directly — `docs/planning/v0.1/02-gating-findings.md` §7 states this verbatim;
  the "sender identity/authorization are enforceable rather than inferred from
  content" clause of `docs/planning/ADR-001.md` line 74.
- **Early-warning signal.** G5 spike (task D5) records a `FAIL` for either
  provider in `docs/planning/gates/G5-result.md`, per
  `docs/planning/v0.1/02-gating-findings.md` §7.
- **Response.** None stated at the gate level. A failing provider's provenance
  rendering becomes a required design-change finding before Stage 2 spec freeze
  (`docs/planning/v0.1/02-gating-findings.md` §7); this also stops the pipeline
  per `docs/planning/v0.1/10-stages.md` §2's G5 no-reorder rule.
- **Status (2026-09-27, issue #38/D5): materialized on Codex — RISK-G5 is now realized,
  not hypothetical.** G5 ran under a 60-minute timebox (declared 06:08:53Z, closed
  06:38:46Z, ~29m53s elapsed, not expired) and recorded **FAIL** (Codex criteria 2/3 f;
  Claude all criteria x). Claude passed every criterion evaluated for it (sender
  provenance cannot be forged from content, the contradiction is shown, a
  non-identifier-safe `meta` key is dropped and no security attribute relies on one —
  `docs/planning/gates/G5-result.md`). Codex failed criterion 2 (evidence: X2, X5) and
  criterion 3 (evidence: X2): a forged nested envelope using a non-matching guessed
  delimiter (case X2) got the model to name the forged id as the sender in part (1) of
  its own answer, though it declined to pick either id as authoritative in part (3); a
  peer-controlled `oac_reply_to` value inserted unmodified into the header block
  produced two `oac_sender:` lines the model could not resolve (case X5, criterion 2
  only). A forged block replaying a real, already-sent delimiter (case X3) did **not**
  get the model to name the forged id — it named neither id and quoted the claim as a
  claim — so X3 is not failing evidence for either criterion; the replay caused
  ambiguity, not acceptance. Per `docs/planning/v0.1/10-stages.md` §5's Gate S1
  acceptance criterion 1, a `FAIL` is a closed verdict, so this does not itself block
  Stage 1's own exit; per §5's go/no-go condition and §2, it blocks the pipeline from
  proceeding past Stage 2's interface freeze until a Codex-side provenance-framing
  design change lands and DESIGN acceptance criterion 6 is re-established for Codex.
  Tracked as new conflict-register entry C13
  (`docs/planning/v0.1/03-decisions-and-amendments.md` §4,
  `docs/planning/ADR-001-AMENDMENTS.md` "New register entries") and in
  `docs/planning/STATUS.md`. The amendment need has three parts: (1) charset/format
  validation on peer-controlled envelope field values before they are inserted into the
  Codex header block, not only the delimited body; (2) a Codex framing that gives the
  model a reliable way to reject a forged block using a wrong delimiter or a replayed
  real one, since `docs/planning/decisions/C6-trust-rendering.md` §5's per-delivery
  unguessable delimiter holds structurally on the wire in both X2 and X3 but the
  model's own reading of the text does not reliably track it; (3) the exploratory,
  non-verdict-bearing `turn/start.additionalContext` observation (case X6, a second,
  presently unused, machine-set-metadata carrier on Codex) as an input to consider, not
  a requirement. This status note does not change RISK-G5's own risk/invalidates/
  response text above, which stays the standing description for any future G5 re-run.

## R2 — Preview/experimental surface drift

### RISK-CLAUDE-PREVIEW — Claude Channels research-preview surface drift

- **Risk.** The Claude Code Channels research-preview surface changes shape
  around G1: channel behaviour across `--resume`/`--continue` is undocumented: whether
  one MCP server can present more than one logical channel is undocumented; and
  the compatibility-shim boundary for this surface stays unnamed (conflict-
  register C11). Since 2026-09-27 the Claude Code (Channels) row is **floating**
  (`docs/planning/PINS.md`, "Floating-version policy"), so any further release again
  invalidates the gates relying on it (G1, G4, G5) until each is re-run against the new
  last-observed version — mirroring the Codex row's floating risk below.
- **What it invalidates.** `docs/planning/v0.1/01-capability-matrix.md` §1's
  Claude research-preview label and its pinned-version assumption;
  `docs/planning/v0.1/07-repository-and-dependencies.md` §4(b)'s `adapters/claude/`
  shim-boundary containment claim.
- **Early-warning signal.** A Claude Code pin move in `docs/planning/STATUS.md`'s
  Pins table, or `channels.md` gaining or removing text on `--resume`/
  `--continue` or multi-channel support at the next re-verification pass
  (`.claude/skills/oac-evidence/SKILL.md` §7 trigger). **This signal fired 2026-09-27**:
  the pin moved to floating (last observed `v2.1.283`), invalidating G1's original
  `PASS` on `v2.1.282`. **Re-run and closed 2026-09-28** (issue #39 T6/T7, Box C):
  G1 PASSED again on `v2.1.283` — see `docs/planning/gates/G1-result.md`. The signal
  will fire again on any further Claude Code Channels release.
- **Response.** Re-verify per `.claude/skills/oac-evidence/SKILL.md` §7 on the
  pin move. Until the shim boundary is named, containment already holds by
  construction: every Claude-specific type stays inside `adapters/claude/`
  (`docs/planning/v0.1/07-repository-and-dependencies.md` §4(b)) — no interim
  workaround is needed because the surface is already isolated by module
  boundary, only its own name is open.

### RISK-CODEX-EXPERIMENTAL — Codex experimental live-inject surface drift

- **Risk.** The Codex experimental live-inject surface changes. Implicit daemon attach
  at runtime was resolved on Windows by G2 on `0.154.0`
  (`docs/planning/gates/G2-result.md`), then re-confirmed on Windows on the Codex row's
  current last-observed version, `0.157.1` (re-run 2026-09-26, same result file). Since
  2026-09-26 the Codex version is **floating**, so any further release again invalidates
  the gate until re-run; it remains unconfirmed on macOS and Linux at every version
  observed so far. A design consequence: the planned git dependencies
  `codex-app-server-{client,protocol,transport}` are pinned to the 0.154.0 commit
  (`docs/planning/decisions/C1-language-runtime.md` §10), while the runtime floats, so
  their schema can drift from the running app-server. Also still
  open: whether Codex
  Desktop exposes the control socket; the compatibility-shim boundary for this
  surface stays unnamed (conflict-register C11); whether Codex reliably
  reproduces a header-supplied `oac_message_id` in a subsequent `reply` tool
  call's `in_reply_to` argument; and whether Codex's `thread.sessionId` field
  always equals `thread.id`. **New (G4, 2026-09-25/26):** a Codex daemon
  `thread/list` query made during the G4 spike returned Codex-Desktop-originated
  thread entries (`originator: "Codex Desktop"`) whose `preview` text was Claude
  Code prompt content the operator had typed into a separate Claude Code session —
  cross-harness prompt visibility through Codex's own session history, security-
  relevant, mechanism not investigated (`docs/planning/gates/G4-result.md`).
- **What it invalidates.** `docs/planning/v0.1/01-capability-matrix.md` §1's
  Codex experimental label; `docs/planning/v0.1/07-repository-and-dependencies.md`
  §4(b)'s `adapters/codex/` shim-boundary containment claim; Decision 9's layered
  reply-correlation rule
  (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 9).
- **Early-warning signal.** A Codex CLI pin move in `docs/planning/STATUS.md`'s
  Pins table; the 2026-09-17 `app-server` documentation-drift signal recurring or
  worsening at the next re-fetch
  (`docs/planning/v0.1/08-cli-and-deployment.md` §10); the G2 or G4 spike
  surfacing unexpected `in_reply_to`/`sessionId` behaviour.
- **Response.** Re-verify per `.claude/skills/oac-evidence/SKILL.md` §7 on the
  pin move. Decision 9 already treats a model-echoed `in_reply_to` as untrusted
  content, validated against independently-tracked thread/turn state — drift
  here degrades to the already-named inferred/uncorrelated downgrade rather than
  silent misattribution
  (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 9).

## R3 — Evidence/pin drift

### RISK-FLOOR — `>= v2.1.232` floor unverifiable

- **Risk.** The "research preview on Claude Code v2.1.232+" floor is not
  confirmable against `channels.md` at the pinned version (`v2.1.274`).
- **What it invalidates.** `docs/planning/PINS.md` floor 1's stated minimum-
  version claim; it does not invalidate G1 itself, since G1's pin reliance is on
  the pinned version actually installed, not on the floor text
  (`docs/planning/v0.1/02-gating-findings.md` §3).
- **Early-warning signal.** A re-fetch of `channels.md` at the next Claude Code
  pin move still omits `2.1.232` (per `REVERIFICATION-B2.md` §3.1 box 7).
- **Response.** Re-verify per `.claude/skills/oac-evidence/SKILL.md` §7 on every
  Claude Code pin move. Until confirmed, `docs/planning/PINS.md` keeps the
  actually-pinned version (`v2.1.274`) as the operative floor rather than the
  unconfirmed `2.1.232` text.

### RISK-MCP-EXPERIMENTAL — `experimental` capability existence at `2026-07-28`

- **Risk.** MCP `experimental` capabilities may not exist in the current-era
  schema (`2026-07-28`) the way the Claude-channel registration path assumes.
- **What it invalidates.** G1 and G4's shared reliance on
  `capabilities.experimental["claude/channel"]` negotiating correctly
  (`docs/planning/v0.1/02-gating-findings.md` §3, §6).
- **Early-warning signal.** G4's concurrent legacy-vs-current test
  (`docs/planning/v0.1/02-gating-findings.md` §6) surfaces a negotiation failure
  tied to the `experimental` key at `2026-07-28`.
- **Response.** Re-verify per `.claude/skills/oac-evidence/SKILL.md` §7 against
  the MCP `2026-07-28` schema on the next pin move. G4's own fallback (two
  server entry points) already isolates the legacy negotiation path from any
  current-era schema change (`docs/planning/v0.1/02-gating-findings.md` §6).

## R4 — Design-parameter and platform-runtime risks

### RISK-KEYRING — Windows keyring runtime unverified

- **Risk.** The `windows-native-keyring-store` `keyring` backend has not been
  exercised end-to-end against live Windows Credential Manager.
- **What it invalidates.** Decision 7's Windows key-storage claim
  (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 7).
- **Early-warning signal.** The first live-Windows Stage 3/4 test
  (`docs/planning/v0.1/09-test-strategy.md`) fails to read or write a keyring
  entry.
- **Response.** Fall back to the already-decided `age` `0.12.1` encrypted-file
  store — the fallback Decision 7 already names for key storage
  (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 7).

### RISK-LOCAL-IPC — Local IPC peer authentication unverified on Windows

- **Risk.** Named-pipe DACL peer authentication is unexercised on a live Windows
  host; `interprocess` `2.4.4` exposes no first-party peer-credential accessor;
  `oac mcp-shim`'s inherited environment may not locate the daemon's IPC path
  without extra configuration.
- **What it invalidates.** Decision 2's OS-level peer-authentication claim
  (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 2); the zero-
  container launch story's "no extra configuration" assumption
  (`docs/planning/v0.1/08-cli-and-deployment.md` §2).
- **Early-warning signal.** The first live-Windows Stage 3/4 IPC contract test
  fails to authenticate a peer, or the shim cannot locate the daemon socket
  without explicit configuration.
- **Response.** Call the raw OS API directly instead of relying on
  `interprocess`'s accessor — the substitute already named
  (`docs/planning/decisions/C2-process-model.md` §4, §10). If shim environment
  inheritance fails, fall back to explicit IPC-path configuration, the
  alternative already scoped (`docs/planning/decisions/C2-process-model.md`
  §10-§11).

### RISK-CRYPTO-BUILD — Signing-crate Windows build and MSRV policy unverified

- **Risk.** `ed25519-dalek` `3.0.0` may not build cleanly on
  `x86_64-pc-windows-msvc`; `curve25519-dalek`'s repository-level MSRV *policy*
  is unstated.
- **What it invalidates.** Decision 5's envelope-signature dependency choice on
  Windows (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 5); the
  Rust toolchain pin's (`1.98.1`) compatibility assumption for this dependency.
- **Early-warning signal.** The first Windows CI build of the signing crate
  fails, or a future toolchain bump breaks against an undocumented MSRV.
- **Response.** The crate-level `rust-version` field (already confirmed,
  distinct from the unstated repository-level policy) governs the toolchain
  floor (`docs/planning/decisions/C5-envelope-auth.md` §2, §16); a build failure
  is a Stage 3 blocker resolved by pinning the last-working toolchain version,
  not a dependency-choice change.

### RISK-PAIRING — Pairing and fingerprint parameters unvalidated

- **Risk.** The device-key fingerprint truncation length and the 6-digit/120-
  second/5-attempt LAN pairing-code parameters are OAC's own unvalidated design
  choices.
- **What it invalidates.** Decision 6's pairing-flow parameters
  (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 6); the 128-bit
  minimum floor the fingerprint length must stay above
  (`docs/planning/decisions/C5-envelope-auth.md` §10(b)).
- **Early-warning signal.** A Stage 3/4 pairing contract test shows brute-force
  feasibility below the 128-bit floor, or measured LAN latency exceeds the
  120-second window in practice.
- **Response.** Both parameters sit above a fixed floor and are adjustable
  without an ADR amendment — tune the truncation length or the window/attempt
  counts during Stage 3/4 implementation; only the 128-bit minimum floor itself
  is fixed (`docs/planning/decisions/C5-envelope-auth.md` §10, §16).

### RISK-ZENOH-AUTH — Zenoh auth and TLS-stack assumptions unverified

- **Risk.** Zenoh's `auth.pubkey` semantics are unconfirmed, and the default TLS
  stack (`rustls` vs. an alternative) is unverified against Zenoh's own
  `Cargo.toml`/feature docs.
- **What it invalidates.** Decision 10's LAN-mode TLS/ACL assumptions
  (`docs/planning/decisions/C7-zenoh-transport.md` §5, §12); the Apache-2.0
  compatibility verdict in
  `docs/planning/v0.1/07-repository-and-dependencies.md` §7 (a different TLS
  backend could carry a different license).
- **Early-warning signal.** G3's TLS-listener pass criterion
  (`docs/planning/v0.1/02-gating-findings.md` §5) surfaces an unexpected
  cipher/backend, or a Zenoh pin move changes the default TLS feature.
- **Response.** Re-verify per `.claude/skills/oac-evidence/SKILL.md` §7 against
  Zenoh's own `Cargo.toml`/feature docs on the next pin move
  (`docs/planning/decisions/C1-language-runtime.md` §9). ACL subjects already
  never map to `zid` regardless of the auth mechanism confirmed
  (`docs/planning/STATUS.md`, C5 decision: "ACL subjects are certificate common
  name or username only, never `zid`"), so this drift cannot silently weaken
  authorization.

## R5 — Low-impact / non-dependency risks

### RISK-ACP — ACP schema v2 alpha status unconfirmed

- **Risk.** ACP schema v2's "alpha" status is carried from
  `docs/planning/PLANNING-PROMPT.md` §3.5 only, not independently re-confirmed.
- **What it invalidates.** Nothing load-bearing — ACP is explicitly not a v0.1
  dependency (`docs/planning/v0.1/05-interfaces.md` §16).
- **Early-warning signal.** ACP is named as a dependency in a later milestone —
  the trigger event the response below already implies.
- **Response.** No action required before v0.1; re-confirm on
  agentclientprotocol.com only if ACP becomes a dependency in a later milestone
  (`docs/planning/STATUS.md` "Open UNVERIFIED items").

### RISK-ZENOH-SOURCE — Zenoh crate version read from GitHub, not crates.io

- **Risk.** The pinned Zenoh crate version/date (`1.10.1`, 2026-09-07) was read
  from GitHub releases because crates.io did not return content when fetched.
- **What it invalidates.** Nothing load-bearing — the pinned value itself is not
  disputed, only its source path (`docs/planning/PINS.md`).
- **Early-warning signal.** crates.io becomes reachable and shows a different
  version/date than `docs/planning/PINS.md`.
- **Response.** Re-confirm on crates.io when reachable
  (`docs/planning/STATUS.md`); no design change unless the re-confirmation
  contradicts the pin.

### RISK-CODEX-MCP-DATES — `codex mcp-server` deprecation/deletion dates unconfirmed

- **Risk.** The `codex mcp-server` deprecation (2026-08-20) and deletion
  (2026-09-05) dates are carried unchanged from
  `docs/planning/PLANNING-PROMPT.md` §3.2, not independently re-confirmed against
  the CLI reference.
- **What it invalidates.** Nothing load-bearing — Decision 9 already routes
  Codex reachability through `codex mcp add`, not the deleted `codex
  mcp-server` (`docs/planning/v0.1/03-decisions-and-amendments.md` Decision 9).
- **Early-warning signal.** `codex mcp-server` reappears or behaves differently
  than "deleted" at the next Codex CLI pin move.
- **Response.** Re-verify per `.claude/skills/oac-evidence/SKILL.md` §7 on the
  next Codex CLI pin move; no interim action needed since the design does not
  depend on `codex mcp-server`.

### RISK-SEP — No confirmed SEP for agent-to-agent messaging

- **Risk.** No SEP or working-group item for agent-to-agent messaging was found,
  carried unchanged from `docs/planning/PLANNING-PROMPT.md` §3.3, not
  independently re-searched.
- **What it invalidates.** Nothing load-bearing — this affects only whether
  OAC's own MCP extension identifier
  (`io.github.rossgraeber/oac-session-channels`,
  `docs/planning/decisions/C3-spec-packaging.md`) could later collide with a
  future standard, not v0.1 function.
- **Early-warning signal.** A new SEP for agent-to-agent messaging is published
  naming a conflicting extension identifier.
- **Response.** Re-search the SEP index at the next MCP pin move
  (`REVERIFICATION-B2.md` §3.3); rename the extension identifier only if a
  conflict is actually found.

### RISK-BIN-SIZE — Zenoh binary size estimate unmeasured

- **Risk.** The 5-15 MB Zenoh binary size figure is a derived estimate, not a
  measurement.
- **What it invalidates.** Nothing load-bearing — informs packaging expectations
  only (`docs/planning/v0.1/08-cli-and-deployment.md`), not v0.1 function.
- **Early-warning signal.** The first Rust release artifact measures outside the
  5-15 MB range (task I3 records the actual binary size). The G3 spike (task D3) ran
  the `eclipse-zenoh` Python wheel and built no Rust artifact, so it produced no
  measurement (`docs/planning/gates/G3-result.md`).
- **Response.** Replace the estimate with the value measured on the first Rust
  release artifact (task I3). No design change either way
  (`docs/planning/STATUS.md`).

### RISK-NATS — NATS capability claims unverified

- **Risk.** NATS reliability, persistence, offline-queueing, ordering,
  multicast-discovery, and routing/federation capability claims are unverified
  against first-party NATS documentation.
- **What it invalidates.** Only `docs/planning/v0.1/05-interfaces.md` §17's NATS
  replacement-proof table cells — NATS is not a v0.1 dependency.
- **Early-warning signal.** NATS is proposed as a second-transport candidate —
  the trigger event the response below already implies.
- **Response.** No action required for v0.1; check every claimed cell against
  first-party NATS specification/documentation before any NATS transport module
  is built (`docs/planning/v0.1/05-interfaces.md` §17).

### RISK-MQTT — MQTT capability claims unverified

- **Risk.** MQTT reliability, persistence, offline-queueing, ordering,
  multicast-discovery, and routing/federation capability claims are unverified
  against first-party MQTT/broker documentation, including a structural
  (unverified) observation that MQTT's broker-based client model lacks a
  Zenoh-style multicast-discovery primitive.
- **What it invalidates.** Only `docs/planning/v0.1/05-interfaces.md` §17's MQTT
  replacement-proof table cells — MQTT is not a v0.1 dependency.
- **Early-warning signal.** MQTT is proposed as a second-transport candidate —
  the trigger event the response below already implies.
- **Response.** No action required for v0.1; check every claimed cell against
  first-party MQTT specification/broker documentation before any MQTT transport
  module is built (`docs/planning/v0.1/05-interfaces.md` §17).

### RISK-HERDR — herdr test tooling's live behavior unverified

- **Risk.** herdr `v0.9.1`, the Epic K test-side driver for real harness CLI sessions,
  has been evaluated only at the desk. Its live behavior is UNVERIFIED, and K1's go/no-go
  is provisional pending live confirmation. The unobserved behaviors are named-session
  start with no attached terminal, `agent start` argv/cwd, dialog readability before any
  keystroke, Codex's post-response state, the launch-environment delta, and per-OS
  support (`docs/planning/decisions/K1-herdr-evaluation.md` §5-§7).
- **What it invalidates.** Nothing in v0.1. herdr is dev/test tooling, never shipped,
  with `Gates affected: none` (`docs/planning/PINS.md` "herdr (test tooling)").
  Human-operated gate spikes remain authoritative. A no-go would cost only Epic K's
  scripted re-runs (K3-K8) and leave gate re-runs human-operated as today.
- **Early-warning signal.** The K1 live leg (§6 of the K1 record) hits one of the no-go
  conditions in its §8. Or a later herdr re-pin changes the CLI options or license that
  K1 cites.
- **Response.** Run the K1 live leg on each target OS and record it in K1 §7. On a
  no-go, stop Epic K's driver work for that OS and keep human-operated gate re-runs. A
  go closes this risk for the tested OSes (`docs/planning/STATUS.md` "Open UNVERIFIED
  items").
- **K6 addition (issue #129).** The opt-in CI workflow puts a logged-in harness on a
  self-hosted runner reachable from a public repository's workflows. GitHub states that
  fork-PR approval policies do not protect self-hosted runners: approved or
  approval-exempt fork code "will execute automatically". The control that keeps fork
  code off the runner is a pre-job hook on the runner machine
  (`tools/herdr/runner-hooks/`), which admits only the opt-in workflow from `main` on a
  dispatch or a push. Its logic is self-tested (bash) and was run by the K6 review under
  PowerShell 7 on Linux. It has not run on a real runner, and Windows PowerShell 5.1 is
  untested. It does not survive a compromised admitted job, which can unset it through the
  runner's own `.env` (`herdr-runner.md` §1). The repository-side controls
  (allowlisted triggers, permissions, SHA-pinned actions, no secrets, allowlisted
  scenario, fail-closed upload gate, `oac-boundaries` check 9) are tested but catch drift
  only. Response: install the hook and observe it refuse and allow a job on each runner
  before any harness run, per `docs/planning/gates/herdr-runner.md` §1 and §7. If that
  cannot be done, do not register the runners and keep scripted runs local (row 52).

## Traceability — every `docs/planning/STATUS.md` "Open UNVERIFIED items" entry

Mechanical proof for issue #32's first acceptance box: every entry in
`docs/planning/STATUS.md`'s "Open UNVERIFIED items" list is disposed of here. That was 29
entries when this file was written, and it grew to 40 by 2026-09-26: rows 30-39 from the
G1, G2 and G3 gate spikes, and row 40 from the same day's separate floating-pin decision
(the Codex row went floating, which itself raised a new re-verification item). None of
the original 29 were closed by evidence found while writing this file (per `oac-evidence`
§5, "never silently promoted": closing an item requires a re-verification citation in
the same change, and none of the 29 had one available). Row 40 was subsequently
**closed** later the same day, 2026-09-26, when the Codex §3.2 facts it named were
re-verified against `0.157.1` — its STATUS.md bullet was removed accordingly (§5's
promotion procedure), and this table keeps the row, marked CLOSED, for traceability
rather than deleting it. Row 41 was added the same day for a new item that
re-verification surfaced. Rows 42-43 were added 2026-09-26 from the first G4 run
(issue #37/D4, out-of-box): that run confirmed all five pass criteria but recorded
`NOT RUN` because its declared timebox had already expired before the confirming
evidence was gathered, and it re-confirmed row 41 (Codex `0.157.1` connected but never
negotiated the modern era) plus surfaced two genuinely new UNVERIFIED items
(rows 42-43). A fresh, redeclared timebox the same day (2026-09-26) reconfirmed all five
criteria without expiring, so **G4 is now `PASS`**
(`docs/planning/gates/G4-result.md`); that re-run reproduced rows 41-43 unchanged and
surfaced one further new item, row 44. Rows 45-48 were added 2026-09-27 from the G5 spike
(issue #38/D5): row 45 (Codex header/delimiter framing does not reliably stop a forged
sender from being named), row 46 (peer-controlled envelope field values inserted
unmodified into the Codex header block), row 47 (a `meta` key literally named `source`
renders as a second attribute, informational), and row 48 (`turn/start.additionalContext`
as an unused second metadata carrier, exploratory). Unlike rows 1-44, rows 45-48 are not
themselves reproduced as bullets in `docs/planning/STATUS.md`'s "Open UNVERIFIED items"
list — rows 45-46 are tracked there through conflict-register entry C13
(`docs/planning/STATUS.md` "Open conflict-register items"), while rows 47 (the `meta` key
literally named `source`) and 48 (`turn/start.additionalContext`) are informational/
exploratory and are **not** tracked in STATUS.md at all, under C13 or otherwise. This
table's own "every entry disposed of here" claim (above) is scoped to STATUS.md's "Open
UNVERIFIED items" list specifically, which rows 45-48 are not members of — noted here
rather than silently overclaimed. The table is therefore 48 rows: of rows 1-44 (the ones
that do correspond to STATUS.md's "Open UNVERIFIED items" list), 42 are still listed
there (row 31 among them, confirmed not a risk, but kept as a correction note per that
row's own text) and 2 are closed (rows 32, 40); rows 45-48 are additional risk-table
entries, tracked (45-46) or untracked (47-48) in STATUS.md as described above. Every row
carries a risk id, except row 31 (which cites the
evidence that confirmed it), row 32 (closed, cites its own closing evidence), and row 40
(closed, cites its own closing evidence). No cell is blank.

| # | STATUS.md item (short) | Disposition |
|---|---|---|
| 1 | Claude channel behaviour across `--resume`/`--continue` | RISK-CLAUDE-PREVIEW |
| 2 | One MCP server presenting more than one logical channel | RISK-CLAUDE-PREVIEW |
| 3 | Implicit Codex daemon attach default at runtime: closed on Windows by G2, on `0.154.0` and again on the currently observed `0.157.1` (re-run 2026-09-26, `docs/planning/gates/G2-result.md`); still open on macOS/Linux | RISK-CODEX-EXPERIMENTAL |
| 4 | Codex Desktop control-socket exposure | RISK-CODEX-EXPERIMENTAL |
| 5 | Zenoh `auth.pubkey` semantics | RISK-ZENOH-AUTH |
| 6 | 5-15 MB Zenoh binary size estimate | RISK-BIN-SIZE |
| 7 | Claude Channels compatibility-shim boundary unnamed (C11) | RISK-CLAUDE-PREVIEW |
| 8 | Codex live-inject compatibility-shim boundary unnamed (C11) | RISK-CODEX-EXPERIMENTAL |
| 9 | ACP schema v2 "alpha" status | RISK-ACP |
| 10 | Zenoh crate version/date read from GitHub, not crates.io | RISK-ZENOH-SOURCE |
| 11 | `codex mcp-server` deprecation/deletion dates | RISK-CODEX-MCP-DATES |
| 12 | No SEP for agent-to-agent messaging | RISK-SEP |
| 13 | "Research preview on Claude Code v2.1.232+" floor | RISK-FLOOR |
| 14 | MCP `experimental` capabilities at current era `2026-07-28` | RISK-MCP-EXPERIMENTAL |
| 15 | Zenoh default TLS stack `rustls` | RISK-ZENOH-AUTH |
| 16 | `rmcp`-based server registering as a legacy-era live channel | RISK-G4 |
| 17 | Codex reproducing `oac_message_id` in `in_reply_to` | RISK-CODEX-EXPERIMENTAL |
| 18 | Windows `windows-native-keyring-store` runtime | RISK-KEYRING |
| 19 | `oac mcp-shim` environment inheritance for the IPC path | RISK-LOCAL-IPC |
| 20 | Named-pipe DACL peer auth on live Windows | RISK-LOCAL-IPC |
| 21 | `interprocess` `2.4.4` peer-credential accessor | RISK-LOCAL-IPC |
| 22 | Codex `thread.sessionId` vs. `thread.id` | RISK-CODEX-EXPERIMENTAL |
| 23 | `ed25519-dalek` `3.0.0` on `x86_64-pc-windows-msvc` | RISK-CRYPTO-BUILD |
| 24 | `curve25519-dalek` repository MSRV policy | RISK-CRYPTO-BUILD |
| 25 | Device-key fingerprint truncation length | RISK-PAIRING |
| 26 | 6-digit/120s/5-attempt pairing parameters | RISK-PAIRING |
| 27 | NATS capability claims | RISK-NATS |
| 28 | MQTT capability claims | RISK-MQTT |
| 29 | 2026-09-17 `app-server` doc-drift signal (Codex daemon-attach default) | RISK-CODEX-EXPERIMENTAL |
| 30 | Claude Code Channels pin now floating (last observed `v2.1.283`, operator decision 2026-09-27, issue #39/T0, mirroring the Codex row); no full §3.1 re-verification done at `v2.1.282` or `v2.1.283`. Per the pin-move checklist, **G1 was invalidated** 2026-09-27 (it had run on `v2.1.282`, not the new last-observed `v2.1.283`) and was **re-run and PASSED again 2026-09-28** on `v2.1.283` (issue #39 T6/T7, Box C — `docs/planning/gates/G1-result.md`). A future release re-fires this same invalidation mechanism (`docs/planning/PINS.md`) | RISK-CLAUDE-PREVIEW |
| 31 | Claude Code MCP stdio wire framing is NDJSON (from G1) | Confirmed by evidence in `docs/planning/gates/G1-result.md` (UNVERIFIED items), not a risk. STATUS.md keeps it on the list only as a correction to an earlier wrong assumption. |
| 32 | Exact wrapper text for a mid-turn-delivered channel notification (from G1) | **CLOSED** — captured verbatim by G5 case C6 at Claude Code `2.1.283` (`docs/planning/gates/G5-result.md`): the full `<system-reminder>A message arrived from … while you were working: … IMPORTANT: This is NOT from your user …</system-reminder>` wrapper text, with the real `oac_*` attributes intact inside it. |
| 33 | G3 criteria 1-4 on macOS (from G3) | RISK-G3 |
| 34 | G3 on bare-metal Linux (from G3; the Linux leg ran on WSL2) | RISK-G3 |
| 35 | `#iface=` on macOS and on Windows with a valid interface name (from G3) | RISK-G3 |
| 36 | G3 via the Rust `zenoh` crate built with `1.98.1` and embedded in OAC (from G3) | RISK-G3 |
| 37 | Unidentified second thread loaded in the Codex daemon (from G2) | RISK-CODEX-EXPERIMENTAL |
| 38 | Codex daemon `originator`/`source` do not reliably identify the creating client (from G2). At the `0.157.1` re-run the same TUI thread's `originator` matched the TUI itself (`codex-tui`), unlike at `0.154.0` (`oac_g2_spike`) — consistent with a first-initializing-client mechanism, not a fix | RISK-CODEX-EXPERIMENTAL |
| 39 | Cross-process resume does not attach (openai/codex #21743), not re-tested at `0.154.0` or `0.157.1` (from G2) | RISK-CODEX-EXPERIMENTAL |
| 40 | Codex §3.2 facts not re-verified at the observed `0.157.1`. The Codex row became floating 2026-09-26 by operator decision, and an auto-updater moves it with each release | **CLOSED** — re-verified 2026-09-26 against commit `36650394c5b38c2990ccf2a3457165ca3e9d9726`; every fact HOLDS, with two additive, off-by-default drifts (a new `--no-daemon` opt-out flag, an opt-in `mcp_2026_07_28` client mode — see row 41); see `docs/planning/REVERIFICATION-B2.md` §"§3.2 re-verification at Codex `0.157.1` (floating-pin trigger, 2026-09-26)" and the G2 re-run, `docs/planning/gates/G2-result.md`. RISK-CODEX-EXPERIMENTAL stays open for its other, still-unresolved items (macOS/Linux, the unidentified thread, `originator`/`source` provenance). |
| 41 | Codex `mcp_2026_07_28` client mode untested against a G4 server (new at `0.157.1`, source-level only; feature-flagged, `stage: UnderDevelopment`, off by default). **Now demonstrated once, opt-in only (2026-09-27 row-41 probe, `docs/planning/gates/G4-result.md` "Row-41 probe addendum"):** `codex exec --enable mcp_2026_07_28` negotiated `2026-07-28` on every request against the server's HTTP surface, reached via two separate registrations (`g4` and `g4http`; `server/discover`, `tools/list`, `tools/call`, all carrying `_meta["io.modelcontextprotocol/protocolVersion"]: "2026-07-28"`); the same run's separate stdio (`g4spike`, `.codex/config.toml`) registration still negotiated legacy `2025-11-25`. **Still open:** the feature stays `stage: UnderDevelopment`/`default_enabled: false` — this is Codex's opt-in leg working when explicitly enabled, not the default client behavior, and no `rmcp`-based server has been tested against either mode. Not a G4 criterion failure — criterion 2 is satisfied via Claude as the modern client. | RISK-G4 |
| 42 | Claude Code 2.1.283 sent a stdio `server/discover` probe with `MCP_SDK_GENERATION` confirmed empty (Box B/Box C, 2026-09-28, directory `g1-spike`), and Claude Code 2.1.283 sent it again in D6's own Box A capture (directory `d6-spike`, `_meta.mcp_sdk_generation` recorded as `"v2"` there, not confirmed empty — Box A is evidence for the `=v2` case, not the empty-var case); the same-directory (`g1-spike`) Claude Code 2.1.282 G1 run sent none -- contradicting the documented stdio default of not asking about the newer revision (from G4, reproduced identically in both runs, directory `g4-spike`; reproduced again 2026-09-28, issue #39 T4/T6/T7, across two directories that day (`d6-spike`, `g1-spike`), a third (`g4-spike`) only when counting the original G4 occurrence from a different date -- correcting an earlier draft's wrong claim that all occurrences shared one directory) | RISK-CLAUDE-PREVIEW |
| 43 | Codex-Desktop-originated threads showed Claude Code prompt text in `thread/list` previews; import mechanism UNVERIFIED. Also: at least three times now a client reporting user-agent `codex-mcp-client/0.155.0-alpha.16.4` connected to an idle instance of this same server via the (likely shared) global `codex mcp add` registration, initialized and listed tools — twice on 2026-09-26 while the out-of-box run's server sat unused (09:08:19Z, 15:57:32Z; uncommitted archive `scratchpad/g4-spike/transcript-2026-09-26-outofbox.jsonl` lines 60-73 and 76-89), and again on 2026-09-27 at 05:17:04Z, this time also probing seven OAuth/OIDC discovery paths (all rejected/404) before an `initialize` sent as `2025-06-18` and negotiated down to legacy `2025-11-25`, then `tools/list` (uncommitted archive `scratchpad/g4-spike/transcript-pre-row41-064223.jsonl` lines 58-73, cited in `docs/planning/gates/G4-result.md` "Row-41 probe addendum"). Attribution to Codex Desktop is inferred from the user-agent string alone across all three occurrences, and the cause is UNVERIFIED (from G4, security-relevant) | RISK-CODEX-EXPERIMENTAL |
| 44 | Claude Code does not surface a tool result's `_meta` field to the model, even though it is present on the wire; UNVERIFIED whether this is universal or specific to this tool-call path (from the G4 re-run, 2026-09-26) | RISK-CLAUDE-PREVIEW |
| 45 | Codex header-and-delimiter framing (`docs/planning/decisions/C6-trust-rendering.md` §5) does not reliably stop the model from naming a forged block's sender when it uses a wrong-but-plausible guessed delimiter (G5 case X2, the model named the forged id in part (1) of its answer); a real delimiter replayed from an earlier delivery in the same conversation (G5 case X3) did not get the model to name the forged id, but did cost it the ability to resolve a sender at all — a narrower, related gap, not an acceptance failure. The delimiter's per-delivery unguessability holds structurally on the wire in both cases (from G5) | RISK-G5 |
| 46 | Peer-controlled envelope field values (`oac_reply_to` at minimum) are inserted unmodified into the Codex header block, so a value containing an embedded `oac_sender:`-shaped line produces a header with two `oac_sender:` lines the model cannot resolve (G5 case X5) — needs charset/format validation before header insertion, not just before body insertion (from G5) | RISK-G5 |
| 47 | A `meta` key literally named `source` is not stripped and renders as a second, trailing `source` attribute after the harness's own — not previously stated in `oac-claude-channels` or `docs/planning/decisions/C6-trust-rendering.md` (from G5 case C5, informational) | RISK-CLAUDE-PREVIEW |
| 48 | `turn/start.additionalContext` (`kind: "application"`) is a second, presently unused, machine-set-metadata carrier on Codex, distinct from the header-and-delimiter framing; exploratory only, not verdict-bearing (from G5 case X6) | RISK-G5 |
| 49 | Whether mid-turn `notifications/claude/channel` deliveries batch together at a single tool-call boundary, or can arrive at separate boundaries one at a time, is UNVERIFIED as a guarantee (may depend on send timing). Original G1 PASS (`v2.1.282`) observed two notifications delivered together, between the same pair of tool calls; G1 Box C (`v2.1.283`, issue #39, 2026-09-28) observed two notifications, sent ~1.85s apart, delivered at two separate tool-call boundaries instead. Both agree on order-preserved, nothing dropped, nothing interleaved — only the batching claim is unconfirmed. `PLANNING-PROMPT.md` §3.1, `oac-claude-channels`, `oac-gates/references/G1-claude-wake.md`, and `docs/planning/v0.1/04-architecture.md` are each amended with a dated note, not silently rewritten (from G1 Box C, `docs/planning/gates/G1-result.md`) | RISK-CLAUDE-PREVIEW |
| 50 | Whether `turn/start` and `thread/queue/add` subscribe the calling connection to `turn/*`/`item/*` events, the way `thread/start`, `thread/resume`, and `thread/fork` are source-confirmed to (`codex-rs/app-server/src/request_processors/thread_processor.rs` L1562-1580, L4009-4015, L5227), is UNVERIFIED — inferred only from the same file's request-handling structure not carrying an equivalent "Auto-attach a thread listener" call near either handler; not directly source-confirmed. From D6/T5-T7 (issue #39), `oac-codex-appserver/references/thread-lifecycle.md` | RISK-CODEX-EXPERIMENTAL |
| 51 | herdr `v0.9.1` (Epic K test tooling) live behavior UNVERIFIED — K1's live leg is NOT RUN, so K1's go/no-go is provisional pending live confirmation. Covers named-session start, `agent start` argv/cwd, the timeout options (`agent read`/`agent send-keys` have none, refuted at the desk), dialog readability before any keystroke, Codex's post-response state, the launch-environment delta, and per-OS support (from K1, issue #124, `docs/planning/decisions/K1-herdr-evaluation.md`) | RISK-HERDR |
| 52 | The K6 opt-in workflow (`.github/workflows/herdr-provider-optin.yml`) runs on operator-owned self-hosted runners that hold a logged-in harness, in a **public** repository. GitHub's guidance is that self-hosted runners "should almost never be used for public repositories", and that fork-PR approval policies are not a protection for them. A fork's pull request can retarget any PR-triggered workflow at those runners, and a collaborator with write access can dispatch a modified branch. The runner-side pre-job hook (`tools/herdr/runner-hooks/`) refuses both, but it is UNVERIFIED on a real runner (bash logic self-tested; PowerShell 7 run by the K6 review; Windows PowerShell 5.1 untested; a compromised admitted job can unset it via the runner's `.env`). No runner is registered and no dispatch has run, so the workflow itself is also UNVERIFIED live (from K6, issue #129, `docs/planning/gates/herdr-runner.md` §1 and §6) | RISK-HERDR |

## Self-check (`oac-evidence` §8, `oac-planning-package` §6)

- Every `§n` cross-reference above was re-swept against each target file's final
  section numbering as read for this task (`02-gating-findings.md`,
  `10-stages.md`, `03-decisions-and-amendments.md`, `07-repository-and-
  dependencies.md`, `05-interfaces.md`, `06-security.md`, `08-cli-and-
  deployment.md`); none pointed at a stale number.
- Every UNVERIFIED label carried forward keeps its original reason from
  `docs/planning/STATUS.md`; none is restated as settled.
- Naming resolution (`.claude/skills/oac-planning-package/SKILL.md` §4) applied:
  "Open Agent Channel (OAC)", "OAC Session Channels", `oac` throughout; no
  `sessionchannels` or bare "Session Channels" spelling introduced.
- No neutral-interface text in this file names Zenoh/Claude/Codex/MCP method
  names where neutral text is required (this file discusses gate surfaces by
  name deliberately, as `02-gating-findings.md` and the capability matrix
  already do — it is not spec surface text, per `.claude/skills/oac-boundaries/
  SKILL.md`).
- No risk response above proposes calling a provider model API, holding
  provider credentials, scraping a UI, or polling an inbox from an adapter
  claiming active inbound.
