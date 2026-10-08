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
  `docs/planning/gates/G1-result.md`; dated note, 2026-10-01, #216: G1 was re-run and
  PASSED on `v2.1.283` on 2026-09-28, and harness versions now warn, never gate, so a
  later version no longer makes it `NOT RUN`; G3 is
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
  both no-fallback gates (G1, G5) carry equal weight regardless of position. (Dated note,
  2026-10-03, #220: G5 is now **PASS**. Its Codex leg was re-run under C13 §11 and passed,
  and C13 is `RESOLVED-IN-DECISION`, so the Stage 2 stop described above no longer applies
  to Codex provenance. See RISK-G5's dated status below.)
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
- **Dated status, 2026-10-03 (#46).** `spec/bindings/mcp.md` §9 closes conflict-register
  row C5: the tool surface no longer depends on which era Codex negotiates. Two new
  UNVERIFIED items from that binding sit under this risk and in `docs/planning/STATUS.md`:
  (a) whether a documented per-request session signal exists that OAC can bind to a
  paired session. Codex's `_meta["x-codex-turn-metadata"]` does carry `session_id`,
  `thread_id` and `turn_id` (G4 fixtures), but it is undocumented and client-asserted, so
  it cannot pair alone. Until resolved, calls on any connection not bound by a documented
  pairing are refused (interim, §4.4). No Codex connection is bound yet, so Codex outbound
  calls are refused on both eras, including if Codex's default moves to `2026-07-28`;
  (b) whether one Codex legacy-era connection carries several threads' calls (owner #69);
  (c) whether legacy clients other than Codex `0.157.1`, Claude Code's channel path
  included, accept an `extensions` member in an `initialize` result (MCPB-ERA-008, tested in
  #65).

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
- **Status (2026-10-02, #219).** The macOS leg ran on a GitHub-hosted `macos-latest` VM
  (macOS 26.6.2, arm64) and passed every criterion on the primary multicast path, so **G3
  is PASS at gate level** and no platform needed the fallback. The risk stays open,
  narrower: for physical Mac hardware (the leg ran in a VM), bare-metal Linux, and the
  Rust crate (`docs/planning/gates/G3-result.md`).

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
- **Status (2026-10-03, issue #220): no longer realized at gate level; open as a design
  risk.** G5's Codex leg was re-run under C13 §11, through herdr under the one-off E1
  exception (`docs/planning/gates/herdr-runs/G5-c13-2026-10-02.md`, run
  `20261002T161612Z-4f2b53`, Codex `0.160.0`, 60-minute box not expired, operator
  attestation `062a67c27b7d5a332dedfe3cb392f9ccfe77393a`).
  - Arm 0 (the old C6 §5 frame) reproduced the 2026-09-27 FAIL.
  - Every required trial of arm F (`docs/planning/decisions/C6-trust-rendering.md` §5.0's
    floor) and arm C (the floor plus the `turn/start` anchor, Option C) was x on Codex
    criteria 2 and 3, and every mechanical check passed: X5 and X5c were refused, and X5b
    carried one sender line.
  - The 2026-09-27 Claude results stand.
  - **G5 is PASS** (`docs/planning/gates/G5-result.md`), C13 is `RESOLVED-IN-DECISION`,
    and DESIGN acceptance criterion 6 is re-established for Codex at gate level.

  The risk stays open, narrower:
  - the PASS confirms the gate's reconstructed client, not OAC's adapter. Until the G7/F11
    frame-builder and refusal tests exist, the mitigation in OAC is designed, not proven
    (`oac-security-work` §1, §6);
  - three trials per forged-block case bound the model's error rate; they do not prove it
    zero (C13 §9). Added 2026-10-03: the old frame's X2 failure was not reproduced
    across runs. The K8 run scored X2 x with 2026-09-27's frame, model and X1-then-X2
    thread, but on Codex `0.160.0` rather than 2026-09-27's `0.157.1`. Arm 0 scored f on
    `0.160.0` in a fresh thread. The cause is UNVERIFIED: model variance, or the Codex
    version combined with the shared-thread history (C13 §9 dated note;
    `G5-c13-2026-10-02.md` findings; row 59). If it is model variance, the control fails
    at an unmeasured rate. Arms F and C's three x per case would then separate the new
    frame from the old less sharply than a control that always fails;
  - the `turn/start` anchor is an experimental field behind an unnamed shim boundary (C11).
    The floor alone passed (arm F), so losing the field cannot reopen this risk on its
    own.

  The early-warning signal above still applies to any later G5 run. Rows 45-46 are CLOSED
  (traceability table).

## R2 — Preview/experimental surface drift

### RISK-CLAUDE-PREVIEW — Claude Channels research-preview surface drift

- **Risk.** The Claude Code Channels research-preview surface changes shape
  around G1: channel behaviour across `--resume`/`--continue` is undocumented: whether
  one MCP server can present more than one logical channel is undocumented; and
  the compatibility-shim boundary for this surface stays unnamed (conflict-
  register C11). Since 2026-09-27 the Claude Code (Channels) row is **floating**
  (`docs/planning/PINS.md`). Since 2026-10-01 (#216) a further release no longer
  invalidates G1, G4 or G5: PINS.md records a minimum (`v2.1.282`) and a last tested
  version, and a different version is a warning, never a gate ("Version policy"). The
  risk is now that a release changes behaviour while the recorded verdicts still stand,
  so it shows up first as a warning and then as a failing or changed live run. (Superseded
  text, kept as history: "so any further release again invalidates the gates relying on it
  (G1, G4, G5) until each is re-run against the new last-observed version".)
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
  G1 PASSED again on `v2.1.283` — see `docs/planning/gates/G1-result.md`. Since #216
  (2026-10-01) the signal on a further release is a `VERSION WARNING` in a scripted run,
  plus a new last tested version in PINS.md; it no longer invalidates a verdict.
- **Response.** Re-verify per `.claude/skills/oac-evidence/SKILL.md` §7 on the
  pin move. Until the shim boundary is named, containment already holds by
  construction: every Claude-specific type stays inside `adapters/claude/`
  (`docs/planning/v0.1/07-repository-and-dependencies.md` §4(b)) — no interim
  workaround is needed because the surface is already isolated by module
  boundary, only its own name is open.
- (Dated note, 2026-10-02, #122: the §3.1 re-check at `2.1.285`
  (`docs/planning/REVERIFICATION-B2.md`) found three documentation drifts, none of which
  changes a gate verdict:
  - D4: `--channels` takes `plugin:` entries only. OAC already uses the development flag.
  - D5: a documented `CLAUDE_CODE_SESSION_ID` reaches stdio MCP servers. This is an open
    conflict with C4 §3 in `docs/planning/STATUS.md`. (Resolved 2026-10-02, #236: C4 §3
    keeps the hook `session_id` authoritative and reads the variable as a cross-check;
    a mismatch fails closed only at `SessionStart` `source` `startup`. Threat: `06-security.md` §14 row 24.)
  - D6: the `2.1.232` floor is unsupported.

  The `--resume` and multi-channel docs are still silent, so rows 1-2 stay open.)
- (Dated note, 2026-10-02, #236: **`SessionStart` `source` drift.** The C4 §3 rule
  depends on the documented `source` values `startup`, `resume`, `clear`, `compact` and
  `fork` (`hooks.md`, retrieved 2026-10-02). A missing or unknown `source` is treated as
  `startup` and fails closed on a mismatch. That is a conservative default from the PR
  #238 review, outside the operator's literal rule.
  - **Early-warning signal:** `hooks.md` documents a new `source` value, or a supported
    Claude Code version sends a `SessionStart` payload without `source`. Either shows up
    as case 3(b) findings whose `source` is missing or unknown.
  - **Response:** re-decide that default in C4 §3 with the operator, and map the new
    value to case 3 or case 4 explicitly.)

### RISK-CODEX-EXPERIMENTAL — Codex experimental live-inject surface drift

- **Risk.** The Codex experimental live-inject surface changes. Implicit daemon attach
  at runtime was resolved on Windows by G2 on `0.154.0`
  (`docs/planning/gates/G2-result.md`), then re-confirmed on Windows on the Codex row's
  then last-observed version, `0.157.1` (re-run 2026-09-26, same result file). Since
  2026-09-26 the Codex version is **floating**. Since 2026-10-01 (#216) a further release
  no longer invalidates the gate: PINS.md records a minimum and a last tested version
  ("Codex CLI and app-server", "Version policy"), and a different version is a warning, never a gate. (Superseded
  text: "so any further release again invalidates the gate until re-run".) It remains
  unconfirmed on macOS and Linux at every version observed so far. A design consequence: the planned git dependencies
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
  **New (#274, 2026-10-04): all Codex inbound delivery now rests on this surface.**
  `spec/bindings/mcp.md` §8.2.1 sends every delivery through the experimental
  `thread/queue/add` and forbids `turn/start` and `turn/steer` for delivery. No
  non-experimental delivery path is left, so a change to `thread/queue/add` stops Codex
  inbound delivery until the G6 shim follows it. The queue also refuses some threads
  outright (rows 65-66).
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
  confirmable against `channels.md` at the version B1 checked (`v2.1.274`; see
  `docs/planning/PINS.md`, "Claude Code Channels" — the B1 record kept as history,
  "Floor 1").
- **What it invalidates.** `docs/planning/PINS.md` floor 1's stated minimum-
  version claim; it does not invalidate G1 itself, since G1's pin reliance is on
  the pinned version actually installed, not on the floor text
  (`docs/planning/v0.1/02-gating-findings.md` §3).
- **Early-warning signal.** A re-fetch of `channels.md` at the next Claude Code
  version change still omits `2.1.232` (per `REVERIFICATION-B2.md` §3.1 box 7).
- **Response.** Re-verify per `.claude/skills/oac-evidence/SKILL.md` §7 when a new
  Claude Code version is tested. Until confirmed, the unconfirmed `2.1.232` text is not
  treated as the operative floor. **Resolved 2026-10-01 (issue #216, operator decision):**
  the operative floor is the row's **minimum version**, `v2.1.282`, the first version the
  project actually worked with (G1, 2026-09-25; `docs/planning/PINS.md`, Claude Code
  Channels, "Version policy"). It lies above both capability floors. Like every harness
  version check, it warns and never gates. (Superseded: "the B1 record in
  `docs/planning/PINS.md` used the then-pinned `v2.1.274` for that role. Which version
  serves as the operative floor now that the row floats is open — issue #216.")
- (Note, 2026-10-01, issue #186: this entry previously said "at the pinned version
  (`v2.1.274`)" and that `PINS.md` "keeps the actually-pinned version (`v2.1.274`) as
  the operative floor"; the Claude Code (Channels) row went floating 2026-09-27, so
  there is no fixed pin to hold that role.)
- (Dated note, 2026-10-02, #122: **resolved as drift.** The first-party changelog
  places "Added `--channels` (research preview)" at `2.1.80`, so `2.1.232` is not a
  channels version (`anthropics/claude-code` `CHANGELOG.md` @
  `52c76441cae91f6891e4712306bffb057ff6fec5`, retrieved 2026-10-02;
  `docs/planning/REVERIFICATION-B2.md` Drift register D6). The `>= v2.1.232` channels
  floor text is unsupported. `2.1.232` is where https://code.claude.com/docs/en/mcp.md
  L324 (retrieved 2026-10-02) starts the v2 MCP client runtime for sessions that fetch
  feature flags: "uses the v2 runtime on Claude Code v2.1.232 or later". That runtime
  adds `2026-07-28`, and the channel-negotiation constraint applies to it. The operative floor stays the minimum version `v2.1.282`, so nothing
  this entry protects changes. Ledger row 13 is closed.)

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
- **Dated status, 2026-10-03 (#46): the existence question is closed.** The `2026-07-28`
  schema defines `experimental?: { [key: string]: JSONObject }` on both
  `ClientCapabilities` and `ServerCapabilities` (lines 720 and 797). Source:
  https://github.com/modelcontextprotocol/modelcontextprotocol/blob/271ecc9accafdd9b83a3c869fa67c22953b2af80/schema/2026-07-28/schema.ts,
  retrieved 2026-10-03. The ledger item is removed from `docs/planning/STATUS.md`. What
  remains is the G4-confirmed fact that Claude Code does not register a channel on a
  `2026-07-28` connection, which `spec/bindings/mcp.md` §8.1 handles by declaring
  `claude/channel` on legacy connections only.

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
  without extra configuration. (Dated note, 2026-10-02, #236: this risk also covers
  the hook-to-shim pairing mechanism that `docs/planning/decisions/
  C4-session-identity.md` §3 "Pairing requirement" requires: OS-reported peer PID and
  process ancestry on the same peer-auth path. It is UNVERIFIED on every OS, and the
  macOS peer-PID call is unknown, since `getpeereid()` reports only UID/GID.
  Early-warning signal: the Epic F Claude adapter cannot pair a hook handler with its
  shim by ancestry. Response: name another daemon-observed key in the Stage 3/4 design.
  The variable `CLAUDE_CODE_SESSION_ID` is never an allowed substitute. Until then,
  sessions stay unbound (fail closed). Traceability row 58.
  **Residual on the same item:** a transition payload that is dropped at the end of the
  pairing window, or is unpairable, cannot be attributed to a shim. The shim's old
  binding can then survive and deliver into a session that has moved on. C4 §3 requires
  the Stage 3/4 implementation to close this gap, for example by refusing that process's
  shim until it is re-paired, if the mechanism allows. Whether it does is UNVERIFIED.
  The early-warning signal is the same: the adapter cannot attribute a dropped payload
  to a process.)
  (Dated note, 2026-10-07, #331, PR #333: the core's binding decision now exists in
  `core/src/session_binding.rs` and `Pipelines`. The pairing key it takes is still the
  G9 item above. Three points follow for the G9 key design:
  - **Residual: same-key denial of service.** A process that descends from the victim's
    harness, such as a tool subprocess, can open a second attachment under the same
    observed key. From then on, every signal for that harness has two candidates and is
    unpairable. The victim's attachment is withheld ([SC-ID-154]) and stays withheld
    until one of the two ends. That is a denial of service, never a hijack: nothing binds
    to the second attachment. Early-warning signal: findings for SC-ID-129 and SC-ID-154
    on a harness that never forked a second shim. Response: make the G9 key distinguish
    the shim from other descendants, if the OS allows it.
  - **The stale-binding residual is closed where the key allows it.** The core withholds
    a bound attachment when an unpairable signal, or one dropped at the window or the
    pending cap, is attributed to it by its observed key. (Dated note, 2026-10-07, #338:
    nor can ordering reopen it. A held signal is decided when its candidate opens, so an
    older one could pair after a newer one of the same key and bind the key's attachment,
    or a new attachment after a shim reconnect, back to the older conversation. Now a
    held signal's window ends when a newer signal of its key, under its adapter, pairs
    (SC-ID-123): it is dropped before it pairs (SC-ID-124, SC-ID-128), binding and
    withholding nothing, and no state outlives an attachment. A drop already taken for
    eviction on another thread before that pairing, and applied after it, is answered by
    the attachment's latest pairing and withholds nothing either. Proving tests:
    `a_late_eviction_drop_does_not_withhold_a_newer_binding`,
    `a_late_eviction_drop_is_answered_by_the_latest_pairing`,
    `an_older_held_signal_does_not_rebind_over_a_newer_one`,
    `an_older_held_signal_does_not_bind_a_reconnected_attachment`,
    `every_older_held_signal_of_the_key_goes_when_a_newer_one_pairs`,
    `a_pairing_ends_only_its_own_keys_windows`,
    `an_older_signal_dropped_after_a_newer_pairing_withholds_nothing`.)
  - **The daemon must call `Pipelines::disconnect`** when it observes a local connection
    end, by end of stream or a broken pipe, never on a timeout. Otherwise connections
    that carry only native signals never free their place in `max_connections`.)
  (Dated note, 2026-10-07, #335: the pending-signal cap is now shared. Each held or
  queued native signal counts against its *holder*: the observed pairing key under the
  adapter that reported it, else its connection, else one holder shared by signals with
  neither. At the cap (`PipelineConfig::max_pending_signals`, default 64), a holder with
  at least two more than the newcomer, the most of any, gives up its oldest. Among tied
  holders, the one whose latest signal arrived last pays. No holder can steer that choice
  by its key or connection identity, though the timing of its own signals can decide a
  tie. Otherwise the newcomer gives up its own oldest, or is dropped when it has none.
  A dropped signal still records SC-ID-128 and withholds what its key attributes it to
  (SC-ID-154), whether it was the newcomer or an evicted held or queued signal. Only a
  signal reported after the dropped one releases the withholding (PR #337): a decision
  already running for an earlier signal neither releases it nor re-binds the attachment
  delivering, and nor does `Pipelines::bind`. Before PR #337, a drop that landed during
  such a decision was undone by it, so delivery could continue into a conversation the
  session had left (also on the refused-newcomer path #333 added). A
  flooding key or connection with `n` pending evicts from another holder only while
  that holder has at least `n + 2`, so it can bring another holder down to `n + 1`, never
  lower; once it holds the most, it evicts only its own signals. So a holder with one
  pending signal is never evicted. Memory stays bounded: at most the cap is pending, plus
  the one signal being decided. Proving tests are in `core/tests/pipeline.rs`
  (`a_flooding_key_cannot_evict_another_keys_held_signal`,
  `many_flooding_keys_cannot_evict_another_keys_held_signal`,
  `a_flood_cannot_evict_another_keys_queued_signal`,
  `the_heaviest_holder_makes_room_for_a_newcomer`,
  `held_signals_count_while_a_pass_decides_them`,
  `a_queued_transition_evicted_by_the_fair_share_still_withholds`,
  `a_queued_transition_replaced_by_its_own_holder_after_disconnect_still_withholds`,
  `an_evicted_held_signal_withholds_its_bound_attachment`, `a_key_is_shared_per_adapter`,
  `an_earlier_decision_does_not_release_a_later_drop`,
  `a_drop_between_bindings_withholds_the_new_binding`,
  `an_external_bind_does_not_answer_a_drop`) and the `pipeline::tests` unit tests for the
  boundary, the tie-break, the holders and "a later signal".
  The residuals are availability only, inside 06 row 13's same-UID boundary:
  - **Residual: many holders.** A process that presents at least the cap's worth of
    distinct holders, each with one pending signal, makes a holder with nothing pending
    lose its new signal. It must keep each one fresh within `native_signal_window`
    (10 s). A held signal always has an observed key, so the holders that last are
    distinct keys, which means distinct harness processes as the G9 key reports them.
    Unkeyed connections, up to `max_connections` (1024), hold places only while a
    decision is in progress (below). When the lost signal is a transition, its
    harness's bound attachment is withheld (SC-ID-154) until a later signal pairs: a
    DoS, never a hijack.
    Early-warning signal: bursts of SC-ID-128 diagnostics from many holders.
    Response: the G9 key design limits how cheaply such holders can be made.
  - **Residual: a burst is trimmed.** A holder with several pending signals can be
    trimmed to `n + 1` by a newcomer holding `n`, so to one by newcomers that hold none.
    Where holders tie, an attacker can time its own signals so that the victim pays
    first. The victim keeps its newest, since its oldest go first.
  - **Signals with no key are never held.** A signal with no observed key, whether or
    not it names a connection, is unpairable and fails closed at its decision
    (SC-ID-125). It takes a place only while it is queued behind a decision in progress.
    Signals that name no connection share one holder there.)
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

### RISK-SEC-SPEC — Security mitigations specified but not yet proven

- **Risk.** `spec/security.md` (E5, #45) makes the security model normative, with 121
  fixtures under `tests/protocol/sec-*/`. No runner executes them yet (E8, F12), and 30 of its
  requirements are `TODO(fixture)` *(dated note, 2026-10-04, #174: 38 Appendix A rows now
  name a `TODO(fixture)`, partial ones included; #174 added four, SEC-AUZ-024 and
  SEC-PRV-015 to SEC-PRV-017, naming F11 and L10)*, each naming the later test (F2, F4, F5, F6, F10, F11, G4,
  G7, G9, H2). Section 13's threat rows whose only proving test is one of those are open risks,
  not closed mitigations (`oac-security-work` §1). Two library facts are UNVERIFIED (rows 63
  and 64): whether the pinned Ed25519 crate's strict verify gives the verdicts of
  `spec/security.md` [SEC-SIG-021] to [SEC-SIG-024] when the fixtures are run (its source,
  read in the PR #265 review, uses the same cofactorless equation and small-order checks),
  and whether the pinned JCS
  crate matches RFC 8785 on the fixtures. The E5 vector check observed that Node.js 25.2.1's
  Ed25519 verify (OpenSSL 3.5.4) accepts the small-order-`R` fixture
  `sec-sig/SEC-SIG-022.n01`, so a default library verify is not enough.
- **What it invalidates.** DESIGN acceptance criterion "sender identity/authorization are
  enforceable rather than inferred from content" until the F-task tests pass; cross-install
  interoperability if two implementations' verifiers disagree.
- **Early-warning signal.** The first F4 run of the `sec-sig` fixtures through the Rust
  verifier, or an F12 conformance run, fails a fixture.
- **Response.** Treat a failing fixture as a defect in the implementation, not the fixture,
  unless the independent check (`spec/security.md` §3.3 note) also disagrees. Add a wrapper
  check before the library call for any rule the library does not enforce. The operator
  decisions on #45 (reply rights, the presence lifetime cap, no implicit same-device grant,
  grant granularity) are recorded in the spec as dated notes.

### RISK-REPLAY-STORE — Authorized peers crowd the duplicate store

- **Risk.** `core::replay::DuplicateStore` (F4, #53) holds at most `DEFAULT_CAPACITY`
  (65536) entries. It never evicts an entry before that entry's hand-off deadline, because
  early eviction would break [SEC-RPL-023]. Entries are added only after security step 4,
  so only an authorized sender can add one. Captured replays share one key, so they add at
  most one entry per captured envelope.

  Since #320, entries are counted per `key_id`, and a copy whose key holds `n` entries is
  admitted only when both of these hold:
  - **quota:** `n` is below `DuplicateStore::per_key_share`, by default a quarter of the
    capacity (16384);
  - **headroom:** `len + n < capacity`. A key never holds more entries than the store has
    free.

  A key past either limit is refused alone, with `failed` / `internal-error`. One trusted,
  granted device that sends many unique envelopes within the 300-second window fills only
  its own share, and other keys keep their room.

  The quota also caps an honest device. At the default it holds 16384 live entries per
  hand-off window, about 55 envelopes per second sustained over 300 seconds (65536, about
  218 per second, before #320). No MUST requires admitting more, and `failed` /
  `internal-error` is a retransmit class (`spec/session-channels.md` Table 8.3), so a
  sender above that rate sees retransmits succeed as its own entries age out (PR #334
  review N5).

  **Finding.** The presence registry and the record lists make room by evicting from the
  heaviest holder. The duplicate store cannot do that: every entry is live until its
  deadline, and evicting one could let a duplicate through ([SEC-RPL-023]). So it refuses
  instead of evicting. The residual is that `k` colluding granted devices that keep adding
  push every key's share toward `capacity / (k + 1)`, about 13107 entries each for four
  devices. The store fills only when about `capacity` distinct keys each hold an entry, and
  keys come only from operator pairing. The spec sets no per-sender bound; [SEC-RPL-031] is
  a per-device SHOULD for receipts only (PR #317 review, N3).
- **What it invalidates.** Nothing in the ADR-001 validation criterion. A misbehaving
  authorized device can deny delivery only to itself, past its share. Several colluding
  ones can shrink every device's share within the window.
- **Early-warning signal.** A receiver reports `internal-error` refusals from step 5 for a
  device that is not flooding, or its store length stays near the cap.
- **Response.** An operator removes the misbehaving devices' grants or keys
  ([SEC-KEY-035]); their entries then age out within the window.
  `DuplicateStore::with_limits` sets a smaller share. The tests are:
  - `x_full_duplicate_store_refuses_without_evicting`;
  - `x_one_key_cannot_fill_the_duplicate_store`;
  - `x_duplicate_store_headroom_leaves_room_for_another_key`;
  - `replay::tests::a_key_at_its_share_is_refused_alone`;
  - `replay::tests::keys_converge_on_an_equal_split_and_a_new_key_finds_room`.

### RISK-PRESENCE-SHARE — Related devices crowd the presence registry

- **Risk.** `core::registry::PresenceRegistry` (F6, #55) holds at most `capacity` sessions
  of other implementations (default 4096). Each signing key holds at most a quarter of
  them, and when the registry is full of `online` sessions, a new session from a key
  holding `n` evicts one session of a key holding the most, only when that is at least
  `n + 2`; both stop at an equal split. Among the keys holding the most, the session most
  recently taken in is evicted, so ties are not broken by key-id order and no key can
  steer the eviction onto another. One related
  device therefore cannot lock other peers out: a peer's session forgotten after a carrier
  loss comes back with its next announcement. Several colluding related devices (paired,
  granted, each under its quota) can still keep the registry full and push every key's
  share toward an equal split, so a legitimate device with many sessions loses some of
  them to evictions (PR #321 re-review, N10). *(Dated note, 2026-10-07, #320, #325: the
  duplicate store and the envelope-created binding entries now follow the same pattern,
  RISK-REPLAY-STORE and RISK-BINDING-TABLE; this row's residual is unchanged.)*
- **What it invalidates.** Nothing in the ADR-001 validation criterion. It needs several
  paired and granted devices to misbehave together.
- **Early-warning signal.** `PresenceDiscard::Full` or `PresenceDiscard::IssuerQuota`
  discards, or a peer's sessions flickering between `online` and `unknown`.
- **Response.** An operator removes the misbehaving devices' keys or grants
  ([SEC-KEY-035]); their sessions are then forgotten as they go stale. A lower per-key quota
  (`PresenceRegistry::with_limits`) narrows each device's share.

### RISK-BINDING-TABLE — Envelope-created bindings crowd each other out

- **Risk.** `AuthorizationEngine::authorize_delivery` (F5, #54) binds every new `from`
  that passes security step 4 ([SEC-PRS-005]). Before #325 nothing forgot those entries,
  so a device holding a device-wide inbound grant could grow the binding table without
  bound by sending from fresh session ids (PR #321 re-review, N11).

  Since #325, an entry an envelope creates is counted until something else refers to it:
  - an own session's registration replaces it;
  - an accepted presence record confirms it, after which the presence registry bounds it;
  - it becomes a conflict mark.

  The counted entries are bounded (`MAX_ENVELOPE_BINDINGS`, 65536 in all;
  `MAX_ENVELOPE_BINDINGS_PER_KEY`, 8192 per key):
  - **Share.** A key at its share gives up its own oldest entry.
  - **Fair share.** In a full table, a key holding `n` takes the oldest entry of a key
    holding at least `n + 2`, the most of any. Among the keys holding the most, the one
    whose latest entry is newest pays, so no key can steer the eviction onto another.
  - **Kept.** An entry that a hand-off record is looked up through is never evicted, so the
    MUSTs that read it hold: [SEC-AUZ-016] discovery, and [SC-RCP-053] and [SC-RCP-054]
    correlation. Conflict marks and own sessions' bindings are never counted. An entry
    evicted between step 4 and its hand-off is bound again by its hand-off record.

  Removing an entry is what [SEC-PRS-009] permits: the registry holds no record of the
  session, and the next envelope or announcement binds it again. With nothing to evict,
  the envelope is refused with `failed` / `internal-error` and nothing is bound.

  An entry that `record_handoff` binds again, because its binding went between step 4 and
  the hand-off, gets room as any new entry does. When every entry that could go is in use,
  it is kept above the bound; such entries number at most the hand-offs in flight when
  their bindings went (PR #334 review N3).

  The residual has three parts:
  - **An evicted id can be claimed.** While an evicted session id is unbound, another
    granted device can claim it with its own envelope. That is not impersonation:
    provenance and authorization follow the verifying key, so the claimant's envelopes are
    authorized under its own grants and rendered with its own principal, and [SEC-PRS-009]
    accepts that removing an entry reopens other keys' claims. It has two consequences
    (PR #334 review N1):
    1. the original device's later envelopes from that id are refused at step 4 with a
       finding ([SEC-AUZ-003], [SEC-PRS-004]) until an operator acts: a denial of service
       of that one id;
    2. a later send from an own session to that id is resolved against the claimant's key,
       and reaches the claimant if an outbound grant covers it.

    Both need an evicted entry that no live hand-off record uses, so the evicted device
    is at its own share, or the heaviest in a full table, and was not handed a message
    from that id within the reply period.
  - **A receipt can be discarded.** A receipt naming an evicted id is discarded
    ([SEC-RCT-003] check 4), so that envelope's state stays `unknown`.
  - **The table can fill.** It can fill with entries in use only when about 16 colluding
    devices each hold 4096 handed-off session ids within the 24-hour reply period. A new
    `from` is then refused.
- **What it invalidates.** Nothing in the ADR-001 validation criterion.
- **Early-warning signal.** `internal-error` refusals at step 4, or
  `AuthorizationEngine::envelope_bindings` staying near the cap.
- **Response.** An operator removes the device's grant or key ([SEC-KEY-035]), which
  removes its bindings in the same step. `AuthorizationEngine::with_envelope_binding_limits`
  sets other limits. The tests are:
  - `x_envelope_bindings_are_bounded`;
  - `x_binding_table_fair_share_takes_from_the_heaviest`;
  - `authorization::tests::envelope_bindings_are_bounded_and_keep_what_records_use`;
  - `authorization::tests::envelope_bindings_leave_the_bound_when_referred_to`;
  - `authorization::tests::entries_whose_records_came_first_are_kept`.

### RISK-RECORD-PARTITIONS — A writer's own sent and hand-off records end early under a flood

- **Risk.** Sent and hand-off records live in process memory and are bounded (#313, PR
  #326 review B2 and re-review B2'). They are partitioned so that no writer can evict
  another's records:
  - sent records by the own session that sent them;
  - hand-off records by the key that verified the envelope, and by the sending session as
    well under this device's own key.

  Each list has three bounds:
  - **Per partition:** `MAX_RECORDS_PER_PARTITION` (4096). A writer that sends, or is handed
    off, more than that within the 24-hour reply period ([SEC-AUZ-013], [SEC-AUZ-016]) loses
    its own oldest records early.
  - **Total:** `MAX_RECORDS_TOTAL` (262144). Past it, the largest partition gives up its
    oldest record, so a writer below its fair share keeps its records.
  - **Partitions:** `MAX_RECORD_PARTITIONS` (4096) partitions held at once.

  Partitions are reclaimed in four ways:
  - a partition goes as soon as it is empty;
  - an own session's partitions go when the session ends (its sent records, and what it
    sent as an own sender). Since #328, a late copy from an ended own session does not make
    its hand-off partition again;
  - a key's partitions go when the key is removed from trust;
  - a partition whose records are all past their reply period goes on the next record
    added. Since #328, expired partitions are found in order of expiry, at `O(log n)` per
    partition dropped. Before, a scan over every partition could run once per insert under
    a staggered-refresh schedule.

  Every session end also prunes all expired records (`AuthorizationEngine::prune`). A
  record for a new partition is refused only while 4096 partitions each hold a record still
  inside its reply period. For example, a peer device keyed partition or a live own sender
  that wrote in the last 24 hours. No live partition is evicted for it.

  Losing a record ends that writer's own reply right, discovery right and
  [SC-RCP-053]/[SC-RCP-054] correlation for that envelope, which fails closed.
- **What it invalidates.** Nothing in the ADR-001 validation criterion. These are MUSTs
  for the full reply period. Under each bound they are given up only by the writer that
  exceeds it:
  - a writer above about 0.05 records per second for a day;
  - the largest writer, past the total;
  - a new writer, while 4096 others are live within the day.

  A peer or a local session cannot use these bounds against another peer or session. The
  tests are:
  - `one_peer_cannot_evict_another_peers_handoff_records`;
  - `one_local_session_cannot_evict_another_sessions_records`;
  - `ended_sessions_free_their_partitions` (at a cap of 16, in every `cargo test`), and
    `full_scale_ended_sessions_free_their_partitions` (4096 short-lived own sessions, then
    a new peer is still recorded; `#[ignore]`d, run by `scale-optin.yml` or
    `cargo test -p oac-core --release -- --ignored full_scale`);
  - `expired_emptied_and_removed_partitions_are_reclaimed`;
  - `expired_partitions_go_in_expiry_order` and
    `x_expired_record_partitions_are_reclaimed_in_order`;
  - `a_late_copy_from_an_ended_own_session_records_nothing` and
    `x_late_copy_from_an_ended_session_records_nothing`;
  - `the_total_is_bounded_by_the_largest_partition`.
- **Early-warning signal.** Correlated replies to a busy peer arriving uncorrelated, or
  `unauthorized` replies from it, within 24 hours of the envelope. Also, more than 4096
  distinct peer keys or concurrently live own senders within a day.
- **Response.** Raise the caps for that deployment, or have the peer reply sooner. Peer
  partitions are bounded by the trusted keys an operator pairs. Own-sender partitions are
  bounded by the sessions live within a day, which the daemon admits.

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
- **Status: narrowed** (2026-10-06, #49). The schema v2 item is closed as verified, with no
  drift: the v2 JSON schemas are "published in the repository releases as `v2.0.0-alphaX`"
  (`agentclientprotocol/agent-client-protocol` at `487ad3ea`,
  `docs/announcements/acp-v2-draft.mdx` L70), the latest being the prerelease
  `schema-v2.0.0-alpha.7` of 2026-09-30, retrieved 2026-10-06. The v2 protocol docs are
  separately in Draft (same page). v1 stays the supported version
  (`docs/planning/decisions/E9-replacement-proofs.md` §1.3 A8, §9). One item stays open:
  what an ACP v1 agent does with a `session/prompt` received while a turn is running. The v1
  pages do not say. It decides whether a v1 binding could make the exception statement of
  [SEC-AUZ-022]. Until a binding does, the ACP adapter declares its sessions send-only as a
  conservative choice; for v2 (Draft) the exception may be available (E9 record §2.4,
  F-A2). Nothing in v0.1 depends on it.

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

### RISK-B2-CARRIED — §3 facts carried, not re-checked against the pin (#228)

*(Added 2026-10-02, #228, for Gate S0 criterion 3.)*

- **Risk.** Thirteen B2 rows carried a `docs/planning/PLANNING-PROMPT.md` §3
  fact as "Carried unchanged" or "PARTIAL", without re-checking it against the
  pin. Under `oac-evidence` §5 they are UNVERIFIED. They cover Codex background
  facts (the `codex queue` flag spelling, UUIDv7 thread ids surviving restarts,
  the unsupported rollout surface, hooks/`notify` being unable to originate a
  turn, Unix-socket peer-validation semantics), Zenoh background facts
  (licensing, the stable API, the binding matrix, the attribution of the
  loopback fix to PR #2671, the §3.4 multi-fact row) and ACP protocol version
  `1`. The full list is `docs/planning/REVERIFICATION-B2.md` "S0
  classification note (2026-10-02, #228)" and the grouped `docs/planning/STATUS.md`
  "Open UNVERIFIED items" entry from the Gate S0 check. Two of the rows
  (`codex mcp-server` dates, no SEP) are also tracked by their own entries,
  RISK-CODEX-MCP-DATES and RISK-SEP. ACP protocol version `1` is tracked only
  here; RISK-ACP covers the separate schema v2 alpha item.
- **What it invalidates.** Nothing load-bearing in v0.1. None of these facts is
  a G1-G5 pass criterion. The behaviours v0.1 depends on are evidenced
  separately: loopback discovery by G3 at Zenoh `1.10.1`, and `thread/queue/add`'s
  method and params at Codex `0.157.1` (REVERIFICATION-B2.md, `0.157.1` table
  fact 4). ACP is not a v0.1 dependency. A wrong fact here would misstate
  background in the package, not break a gate.
- **Early-warning signal.** A Stage 3 or Stage 4 task comes to rely on one of
  these facts, for example the Codex adapter depending on thread-id stability
  across restarts, or the license inventory (#79) needing Zenoh's license. A
  first-party source contradicting one of them is another signal.
- **Response.** Re-check the fact against the current pin with a first-party
  citation before any task relies on it (`oac-evidence` §5 promotion
  procedure). Then close or narrow it in STATUS.md and in the B2 note in the same
  change. This entry gates nothing.
- *(Dated note, 2026-10-06, #49: narrowed. ACP protocol version `1` is re-checked: the v1
  initialization page shows `"protocolVersion": 1` in its request and response examples
  (`docs/planning/decisions/E9-replacement-proofs.md` §1.3 A4). The pin does not move.)*

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
- **Status: narrowed** (2026-10-06, #49). E9 checked all six capability cells against the
  first-party NATS docs (`nats-io/nats.docs` at `f115becf`, retrieved 2026-10-06):
  `docs/planning/decisions/E9-replacement-proofs.md` §1.1, §3.2. One narrower item stays
  open: whether each NATS client library can disable its reconnect buffer (record F-T3);
  and how long a copy can wait in the server's buffer for a slow subscriber, which the
  server settings `write_deadline` and `max_pending` do not bound, so a binding has to
  (record F-T5). Both are in `docs/planning/STATUS.md`. NATS is not a v0.1 dependency.

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
- **Status: CLOSED** (2026-10-06, #49). E9 checked all six cells against the MQTT 5.0
  OASIS Standard and, for bridging and ACLs, the Mosquitto configuration reference
  (retrieved 2026-10-06): `docs/planning/decisions/E9-replacement-proofs.md` §1.2, §4.2. The
  multicast-discovery cell is now cited: the standard defines no discovery.

### RISK-HERDR — herdr test tooling's live behavior unverified off Windows

- **Risk.** herdr `v0.9.1`, the Epic K test-side driver for real harness CLI sessions,
  was evaluated at the desk and then live on Windows on 2026-09-28 (go on Windows, none of
  the §8 no-go conditions hit). Its live behavior on Linux and macOS is UNVERIFIED, so K1's
  overall go/no-go is provisional. The Windows run also found driver hazards (Codex reads
  `idle` on its trust dialog, inherited `CLAUDE_CODE_CHILD_SESSION`, three Claude startup
  dialogs) recorded in `docs/planning/decisions/K1-herdr-evaluation.md` §7.1.
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
- **Addition, 2026-10-03 (#220).** The driver hashes the harness config files only at
  run start and at teardown (`tools/herdr/run.mjs`). For the four October 2 G5 runs, the
  hashes show `~/.codex/config.toml` byte-identical at all eight snapshots. They cannot
  exclude a write that was reverted to the same bytes inside a run (UNVERIFIED — no
  in-run file monitor; `docs/planning/gates/herdr-runs/G5-c13-2026-10-02.md` findings;
  row 60). Response: none needed for v0.1. If a run must prove the files were never
  written, it needs an in-run monitor, not endpoint hashes.

### RISK-BEACON — Beacon external memory service: fast-moving upstream, older MCP revision, one fact unverified

- **Risk.** Beacon (agent-beacon), the external memory service Epic L documents beside
  OAC sessions, is pinned at `v1.3.29` (fixed) but ships often: `v1.3.27`, `v1.3.28`
  and `v1.3.29` are consecutive recent tags, and branch head had already renamed its
  hosted service between the tag and 2026-09-29
  (`docs/planning/decisions/L1-beacon-memory.md` §2). The pin stays fixed only while
  Beacon's opt-in package self-updates (off by default) stay off (L1 §2). Its local MCP
  server advertises an older MCP protocol revision, `2024-11-05`, which predates OAC's
  "MCP — legacy era" pin `2025-11-25` (L1 §6). L1 left four facts UNVERIFIED at the pin
  (L1 §6, rows 53-56). L2's desk research (L1 §11, 2026-09-29) settled three from source
  at the tag: the memory ID and memory-tool result shape is CONFIRMED (row 54),
  concurrent `memory.db` access is CONFIRMED as SQLite WAL with a 5 s busy timeout
  (row 55), and a collision between Beacon's config writes and OAC's planned launch
  paths is REFUTED (row 56). One stays open, narrowed: whether the harness reports
  OAC-delivered input to Beacon at all; Beacon's side and OAC's outbound tool-call
  capture are confirmed (row 53). **Update 2026-10-01 (L3, issue #192, L1 §13):** row 53
  is CONFIRMED live. Beacon captures OAC-delivered input in both harnesses and stored a
  fake secret-shaped token unredacted. No Beacon fact at the pin is UNVERIFIED any more.
  The risk stays open for the fast-moving pin and for the capture residual below. The
  heading's "one fact unverified" is kept as written.
- **What it invalidates.** Nothing in v0.1. Beacon is an external service, never
  shipped or called by OAC, with `Gates affected: none` (`docs/planning/PINS.md`
  "Beacon (external memory service)"). No spec text depends on it: L1 chose docs-only
  (L1 §4 Q1). A drift costs only Epic L's docs (L5), threat rows (L4) and opt-in
  scenario (L10). *(Dated note, 2026-10-04, #174: `spec/security.md` §12.5 and §13
  now carry the memory-reference doctrine and rows 21-23 in neutral terms, naming no
  service, so a Beacon drift changes no normative spec text; the §9.3 note and the
  row-23 residual there are dated, informative observations tied to L1 §2's pin.)* If a harness drops MCP revision `2024-11-05`, Beacon, not OAC, has to move.
- **Early-warning signal.** A new Beacon release tag appears; a cited Beacon doc
  changes at a new tag; L3's herdr-driven live leg shows OAC-delivered input in Beacon's
  `runtime.jsonl`, or shows Beacon editing a Codex config key OAC's launch path uses.
- **Response.** On a new tag, re-read L1 §6 and every L1 §10 citation at it before moving
  the PINS.md row (`oac-evidence` §7); L1 §11's source-level findings are re-read at the
  new tag too. For U1 only, L3 runs the herdr-driven live leg (L1 §12; run locally by
  an agent, not in the default CI suite, #187) and records the Beacon version it ran on
  in L1 §13. L4's row 23 already carries capture as
  a residual; L1 §4 Q3's Local / Metadata-only recommendation limits forwarding but not
  local capture. If L3's B1-B4 steps find a config collision after all, record it as a
  finding against the Codex adapter's launch design, not a workaround
  (`docs/planning/STATUS.md` "Open UNVERIFIED items").
- **Capture-side residual (`docs/planning/v0.1/06-security.md` §14 row 23, L4).** An
  external memory/telemetry service instrumenting a harness session may record messages
  OAC delivered into it. OAC cannot mitigate this — it never configures the service —
  so row 23 is an open risk with no proving test. L1 §4 Q3's Local / Metadata-only
  recommendation limits hosted forwarding only; local `runtime.jsonl` still holds
  redacted, sanitized, truncated content, and the service scopes recall per repository,
  not per `working_directory` (L1 §4 Q4). Whether capture happens at all is U1 (row 53).
  **Update 2026-10-01 (L3, L1 §13):** capture happens. A Claude Code channel delivery
  and Codex `turn/start` / `thread/queue/add` input are recorded verbatim as
  `prompt.submitted` in the local `runtime.jsonl`. A fake secret-shaped token in the
  message body was **not** redacted by Beacon `1.3.29` on any capturing path. Its
  redaction cannot be relied on to strip secrets from OAC messages. Row 23 stays an open
  risk with no proving test.

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
rather than silently overclaimed. (Dated note, 2026-10-03, #220: rows 45-46 are now CLOSED by G5's C13 §11 re-run, and row
48's carrier is now used, as C6 §5.0's anchor.) The table is therefore 56 rows: of rows 1-44 (the ones
that do correspond to STATUS.md's "Open UNVERIFIED items" list), 42 are still listed
there (row 31 among them, confirmed not a risk, but kept as a correction note per that
row's own text) and 2 are closed (rows 32, 40); rows 45-48 are additional risk-table
entries, tracked (45-46) or untracked (47-48) in STATUS.md as described above. Every row
carries a risk id, except row 31 (which cites the
evidence that confirmed it), row 32 (closed, cites its own closing evidence), and row 40
(closed, cites its own closing evidence). No cell is blank. Rows 49-52 were added after
rows 45-48 (G1 mid-turn batching, Codex `turn/start`/`thread/queue/add` event
subscription, herdr's non-Windows live legs, and the K6 self-hosted-runner exposure);
each cell carries its own disposition. Rows 53-56 were added
2026-09-29 from L1 (issue #166): the four Beacon facts left UNVERIFIED at pin `v1.3.29`
(`docs/planning/decisions/L1-beacon-memory.md` §6), all under `RISK-BEACON` and all
listed in STATUS.md's "Open UNVERIFIED items" as one L1 entry. L2 (issue #167,
2026-09-29) closed rows 54-56 and narrowed row 53. L3 (issue #192, 2026-10-01) closed
row 53 with live evidence (L1 §13), so no row 53-56 entry stays listed in STATUS.md.
Like rows 32 and 40, rows 53-56 are closed and cite their own closing evidence. Row 53
also keeps `RISK-BEACON` in its closing cell, because the capture it confirmed is still
an open risk. Row 57 was added 2026-10-02 (#228): the grouped Gate S0 entry for the
B2 rows classified UNVERIFIED, under `RISK-B2-CARRIED`. (Dated note, 2026-10-02, #122:
rows 13 and 30 are now CLOSED, by the §3.1 re-check at Claude Code `2.1.285`, and their
STATUS.md bullets are removed. Of rows 1-44, 40 are therefore still listed in STATUS.md,
and 4 are closed: rows 13, 30, 32 and 40. The counts above are kept as written at the
time.) Row 58 was added 2026-10-02 (#236): the hook-to-shim pairing mechanism that the
C4 §3 revision requires, under `RISK-LOCAL-IPC`. Rows 59-60 were added 2026-10-03 (#220),
from verifying the G5 E1 findings: the old frame's X2 result not reproducing across runs,
under `RISK-G5`, and the endpoint-only harness config hashes, under `RISK-HERDR`. Rows
61-62 were added 2026-10-03 (#43), from spec §7 (E3): transport carriage of presence
records, under `RISK-G3`, and the order of several inputs queued in Codex during a running
turn, under `RISK-CODEX-EXPERIMENTAL`. Row 67 was added 2026-10-06 (#58): the app-server
behaviours the fake Codex app-server models from source only, under
`RISK-CODEX-EXPERIMENTAL`. (Dated note, 2026-10-07, #343: the Stage 1 fixture capture for
Gate S3 criterion 5 recorded, on Codex `0.161.0` and Claude Code `2.1.285`, the behaviours
of rows 62, 66(a), (c), (d) and 67, and of rows 68-69, which this change adds with the text
PR #342 (the Stage 3 exit record, finding F-1) proposed for them. Rows 62, 67, 68 and 69 are
closed by that capture; rows 65 and 66 are narrowed. Records: `docs/planning/gates/herdr-runs/S3-codex-2026-10-07.md` and
`docs/planning/gates/herdr-runs/G1-2026-10-07.md`.) (Dated note, 2026-10-07, #6: the
Stage 3 exit record `docs/planning/decisions/F-6-stage3-exit.md` closes row 64 by run, and
its STATUS.md bullet is removed. Its re-run against the #343 capture adds row 70: which
connections Codex sends a thread's notifications to, recorded in the S3 capture but
UNVERIFIED as a rule and not modelled by the fake.)

| # | STATUS.md item (short) | Disposition |
|---|---|---|
| 1 | Claude channel behaviour across `--resume`/`--continue` (docs still silent at `2.1.285`, re-checked 2026-10-02, #122) | RISK-CLAUDE-PREVIEW |
| 2 | One MCP server presenting more than one logical channel (docs still silent at `2.1.285`, re-checked 2026-10-02, #122) | RISK-CLAUDE-PREVIEW |
| 3 | Implicit Codex daemon attach default at runtime: closed on Windows by G2, on `0.154.0` and again on the currently observed `0.157.1` (re-run 2026-09-26, `docs/planning/gates/G2-result.md`); still open on macOS/Linux | RISK-CODEX-EXPERIMENTAL |
| 4 | Codex Desktop control-socket exposure | RISK-CODEX-EXPERIMENTAL |
| 5 | Zenoh `auth.pubkey` semantics | RISK-ZENOH-AUTH |
| 6 | 5-15 MB Zenoh binary size estimate | RISK-BIN-SIZE |
| 7 | Claude Channels compatibility-shim boundary unnamed (C11) | RISK-CLAUDE-PREVIEW |
| 8 | Codex live-inject compatibility-shim boundary unnamed (C11) | RISK-CODEX-EXPERIMENTAL |
| 9 | ACP schema v2 "alpha" status | RISK-ACP — closed as verified 2026-10-06 (#49), no drift: prerelease `schema-v2.0.0-alpha.7`; v2 protocol docs in Draft. RISK-ACP now carries the ACP v1 `session/prompt`-during-a-turn item |
| 10 | Zenoh crate version/date read from GitHub, not crates.io | RISK-ZENOH-SOURCE |
| 11 | `codex mcp-server` deprecation/deletion dates | RISK-CODEX-MCP-DATES |
| 12 | No SEP for agent-to-agent messaging | RISK-SEP |
| 13 | "Research preview on Claude Code v2.1.232+" floor | **CLOSED as drift** (2026-10-02, #122). The first-party changelog dates "Added `--channels` (research preview)" to `2.1.80`, so `2.1.232` is not a channels version. It is instead where `mcp.md` L324 starts the v2 MCP client runtime, the runtime the channel-negotiation constraint applies to, for sessions that fetch feature flags. The operative floor is the minimum `v2.1.282` (#216). See `docs/planning/REVERIFICATION-B2.md` Drift register D6; RISK-FLOOR keeps its history |
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
| 25 | Device-key fingerprint truncation length | RISK-PAIRING — narrowed (2026-10-06, #54): the pairing code hashes the full public keys (`core/src/pairing.rs`); only the certificate common-name use stays open |
| 26 | 6-digit/120s/5-attempt pairing parameters | RISK-PAIRING — narrowed (2026-10-06, #54): implemented in `core/src/pairing.rs`, with a commit-then-reveal exchange so the code cannot be ground offline (STATUS.md finding F5-1); unit tests cover the 120-second expiry and the five-attempt abort. The live-network half stays open until a `cli/` pairing verb runs on a LAN |
| 27 | NATS capability claims | RISK-NATS — narrowed (2026-10-06, #49; E9 record §3.2): capability cells closed; the reconnect-buffer item (F-T3) and the slow-consumer buffer-age item (F-T5) stay open |
| 28 | MQTT capability claims | RISK-MQTT — **CLOSED** (2026-10-06, #49; E9 record §4.2) |
| 29 | 2026-09-17 `app-server` doc-drift signal (Codex daemon-attach default) | RISK-CODEX-EXPERIMENTAL |
| 30 | Claude Code Channels pin now floating (last observed `v2.1.283`, operator decision 2026-09-27, issue #39/T0, mirroring the Codex row); no full §3.1 re-verification done at `v2.1.282` or `v2.1.283`. Per the pin-move checklist, **G1 was invalidated** 2026-09-27 (it had run on `v2.1.282`, not the new last-observed `v2.1.283`) and was **re-run and PASSED again 2026-09-28** on `v2.1.283` (issue #39 T6/T7, Box C — `docs/planning/gates/G1-result.md`). A future release re-fires this same invalidation mechanism (`docs/planning/PINS.md`). Dated note, 2026-10-01, #216: that mechanism is retired for this row; a future release is a version warning and invalidates no verdict | **CLOSED** (2026-10-02, #122). The one §3.1 re-check the operator decided on #122 ran at `2.1.285` (`docs/planning/REVERIFICATION-B2.md` "§3.1 re-check at Claude Code `2.1.285`"). 18 of 23 rows hold and three drifted (D4-D6); rows 1-2 of this table stay open. Newer versions are version warnings only. RISK-CLAUDE-PREVIEW stays open |
| 31 | Claude Code MCP stdio wire framing is NDJSON (from G1) | Confirmed by evidence in `docs/planning/gates/G1-result.md` (UNVERIFIED items), not a risk. STATUS.md keeps it on the list only as a correction to an earlier wrong assumption. |
| 32 | Exact wrapper text for a mid-turn-delivered channel notification (from G1) | **CLOSED** — captured verbatim by G5 case C6 at Claude Code `2.1.283` (`docs/planning/gates/G5-result.md`): the full `<system-reminder>A message arrived from … while you were working: … IMPORTANT: This is NOT from your user …</system-reminder>` wrapper text, with the real `oac_*` attributes intact inside it. |
| 33 | G3 on physical Mac hardware (from G3). Was "G3 criteria 1-4 on macOS"; those closed PASS on a GitHub-hosted VM 2026-10-02 (#219), leaving physical hardware open | RISK-G3 |
| 34 | G3 on bare-metal Linux (from G3; the Linux leg ran on WSL2) | RISK-G3 |
| 35 | `#iface=` on macOS and on Windows with a valid interface name (from G3). Note 2026-10-02 (#219): on the macOS hosted VM a nonexistent name was accepted without exception, so `#iface=` is not enforced there; log warnings untested | RISK-G3 |
| 36 | G3 via the Rust `zenoh` crate built with `1.98.1` and embedded in OAC (from G3) | RISK-G3 |
| 37 | Unidentified second thread loaded in the Codex daemon (from G2) | RISK-CODEX-EXPERIMENTAL |
| 38 | Codex daemon `originator`/`source` do not reliably identify the creating client (from G2). At the `0.157.1` re-run the same TUI thread's `originator` matched the TUI itself (`codex-tui`), unlike at `0.154.0` (`oac_g2_spike`) — consistent with a first-initializing-client mechanism, not a fix | RISK-CODEX-EXPERIMENTAL |
| 39 | Cross-process resume does not attach (openai/codex #21743), not re-tested at `0.154.0` or `0.157.1` (from G2) | RISK-CODEX-EXPERIMENTAL |
| 40 | Codex §3.2 facts not re-verified at the observed `0.157.1`. The Codex row became floating 2026-09-26 by operator decision, and an auto-updater moves it with each release | **CLOSED** — re-verified 2026-09-26 against commit `36650394c5b38c2990ccf2a3457165ca3e9d9726`; every fact HOLDS, with two additive, off-by-default drifts (a new `--no-daemon` opt-out flag, an opt-in `mcp_2026_07_28` client mode — see row 41); see `docs/planning/REVERIFICATION-B2.md` §"§3.2 re-verification at Codex `0.157.1` (floating-pin trigger, 2026-09-26)" and the G2 re-run, `docs/planning/gates/G2-result.md`. RISK-CODEX-EXPERIMENTAL stays open for its other, still-unresolved items (macOS/Linux, the unidentified thread, `originator`/`source` provenance). |
| 41 | Codex `mcp_2026_07_28` client mode untested against a G4 server (new at `0.157.1`, source-level only; feature-flagged, `stage: UnderDevelopment`, off by default). **Now demonstrated once, opt-in only (2026-09-27 row-41 probe, `docs/planning/gates/G4-result.md` "Row-41 probe addendum"):** `codex exec --enable mcp_2026_07_28` negotiated `2026-07-28` on every request against the server's HTTP surface, reached via two separate registrations (`g4` and `g4http`; `server/discover`, `tools/list`, `tools/call`, all carrying `_meta["io.modelcontextprotocol/protocolVersion"]: "2026-07-28"`); the same run's separate stdio (`g4spike`, `.codex/config.toml`) registration still negotiated legacy `2025-11-25`. **Still open:** the feature stays `stage: UnderDevelopment`/`default_enabled: false` — this is Codex's opt-in leg working when explicitly enabled, not the default client behavior, and no `rmcp`-based server has been tested against either mode. Not a G4 criterion failure — criterion 2 is satisfied via Claude as the modern client. | RISK-G4 |
| 42 | Claude Code 2.1.283 sent a stdio `server/discover` probe with `MCP_SDK_GENERATION` confirmed empty (Box B/Box C, 2026-09-28, directory `g1-spike`), and Claude Code 2.1.283 sent it again in D6's own Box A capture (directory `d6-spike`, `_meta.mcp_sdk_generation` recorded as `"v2"` there, not confirmed empty — Box A is evidence for the `=v2` case, not the empty-var case); the same-directory (`g1-spike`) Claude Code 2.1.282 G1 run sent none -- contradicting the documented stdio default of not asking about the newer revision (from G4, reproduced identically in both runs, directory `g4-spike`; reproduced again 2026-09-28, issue #39 T4/T6/T7, across two directories that day (`d6-spike`, `g1-spike`), a third (`g4-spike`) only when counting the original G4 occurrence from a different date -- correcting an earlier draft's wrong claim that all occurrences shared one directory) | RISK-CLAUDE-PREVIEW |
| 43 | Codex-Desktop-originated threads showed Claude Code prompt text in `thread/list` previews; import mechanism UNVERIFIED. Also: at least three times now a client reporting user-agent `codex-mcp-client/0.155.0-alpha.16.4` connected to an idle instance of this same server via the (likely shared) global `codex mcp add` registration, initialized and listed tools — twice on 2026-09-26 while the out-of-box run's server sat unused (09:08:19Z, 15:57:32Z; uncommitted archive `scratchpad/g4-spike/transcript-2026-09-26-outofbox.jsonl` lines 60-73 and 76-89), and again on 2026-09-27 at 05:17:04Z, this time also probing seven OAuth/OIDC discovery paths (all rejected/404) before an `initialize` sent as `2025-06-18` and negotiated down to legacy `2025-11-25`, then `tools/list` (uncommitted archive `scratchpad/g4-spike/transcript-pre-row41-064223.jsonl` lines 58-73, cited in `docs/planning/gates/G4-result.md` "Row-41 probe addendum"). Attribution to Codex Desktop is inferred from the user-agent string alone across all three occurrences, and the cause is UNVERIFIED (from G4, security-relevant) | RISK-CODEX-EXPERIMENTAL |
| 44 | Claude Code does not surface a tool result's `_meta` field to the model, even though it is present on the wire; UNVERIFIED whether this is universal or specific to this tool-call path (from the G4 re-run, 2026-09-26) | RISK-CLAUDE-PREVIEW |
| 45 | Codex header-and-delimiter framing (`docs/planning/decisions/C6-trust-rendering.md` §5) does not reliably stop the model from naming a forged block's sender when it uses a wrong-but-plausible guessed delimiter (G5 case X2, the model named the forged id in part (1) of its answer); a real delimiter replayed from an earlier delivery in the same conversation (G5 case X3) did not get the model to name the forged id, but did cost it the ability to resolve a sender at all — a narrower, related gap, not an acceptance failure. The delimiter's per-delivery unguessability holds structurally on the wire in both cases (from G5) | **CLOSED** (2026-10-03, #220): under C6 §5.0's line-quoted body (C13 Option C), the G5 Codex-leg re-run scored every required X2×3, X3×3 and X3-anchored×3 trial x, in arms F and C, while arm 0 (the old frame) reproduced X2 f 3 of 3 (`docs/planning/gates/herdr-runs/G5-c13-2026-10-02.md`, attested `062a67c27b7d5a332dedfe3cb392f9ccfe77393a`). Residual (N=3 trials, gate client not OAC's adapter) stays under RISK-G5 |
| 46 | Peer-controlled envelope field values (`oac_reply_to` at minimum) are inserted unmodified into the Codex header block, so a value containing an embedded `oac_sender:`-shaped line produces a header with two `oac_sender:` lines the model cannot resolve (G5 case X5) — needs charset/format validation before header insertion, not just before body insertion (from G5) | **CLOSED** (2026-10-03, #220): C6 §5.0's whole-value validation refuses the envelope. In the G5 Codex-leg re-run, X5 and X5c were refused with no frame on the wire, and X5b carried exactly one `oac_sender:` line (`docs/planning/gates/herdr-runs/G5-c13-2026-10-02.md`). Residual: the charset stays adapter-local until backlog E1 adopts it (C13 §14), under RISK-G5. *(Dated note, 2026-10-03, #41: E1 adopted it as an envelope rule, `spec/session-channels.md` §4.3 [SC-ENV-010] and [SC-ENV-011], for `id`, `from`, `to`, `conversation_id`, `reply_to` and `correlation_id`, with negative fixtures under `tests/protocol/sc-env/`. The residual is closed in the spec; adapter tests (G7, F11) still prove it in code.)* |
| 47 | A `meta` key literally named `source` is not stripped and renders as a second, trailing `source` attribute after the harness's own — not previously stated in `oac-claude-channels` or `docs/planning/decisions/C6-trust-rendering.md` (from G5 case C5, informational) | RISK-CLAUDE-PREVIEW |
| 48 | `turn/start.additionalContext` (`kind: "application"`) is a second, presently unused, machine-set-metadata carrier on Codex, distinct from the header-and-delimiter framing; exploratory only, not verdict-bearing (from G5 case X6) | RISK-G5. *Dated note, 2026-10-03 (#220):* now used as C6 §5.0's `oac_provenance` anchor on `turn/start` (Option C). It is never load-bearing, and arm C of the G5 re-run passed with it |
| 49 | Whether mid-turn `notifications/claude/channel` deliveries batch together at a single tool-call boundary, or can arrive at separate boundaries one at a time, is UNVERIFIED as a guarantee (may depend on send timing). Original G1 PASS (`v2.1.282`) observed two notifications delivered together, between the same pair of tool calls; G1 Box C (`v2.1.283`, issue #39, 2026-09-28) observed two notifications, sent ~1.85s apart, delivered at two separate tool-call boundaries instead. Both agree on order-preserved, nothing dropped, nothing interleaved — only the batching claim is unconfirmed. `PLANNING-PROMPT.md` §3.1, `oac-claude-channels`, `oac-gates/references/G1-claude-wake.md`, and `docs/planning/v0.1/04-architecture.md` are each amended with a dated note, not silently rewritten (from G1 Box C, `docs/planning/gates/G1-result.md`) | RISK-CLAUDE-PREVIEW |
| 50 | Whether `turn/start` and `thread/queue/add` subscribe the calling connection to `turn/*`/`item/*` events, the way `thread/start`, `thread/resume`, and `thread/fork` are source-confirmed to (`codex-rs/app-server/src/request_processors/thread_processor.rs` L1562-1580, L4009-4015, L5227), is UNVERIFIED — inferred only from the same file's request-handling structure not carrying an equivalent "Auto-attach a thread listener" call near either handler; not directly source-confirmed. From D6/T5-T7 (issue #39), `oac-codex-appserver/references/thread-lifecycle.md` | RISK-CODEX-EXPERIMENTAL |
| 51 | herdr `v0.9.1` (Epic K test tooling) live behavior verified on Windows only (2026-09-28: go, no §8 no-go condition hit); Linux and macOS live legs NOT RUN, so K1's overall go/no-go is provisional. Windows findings for the driver: Codex reports `idle` on its trust dialog, the "`unknown` after a response" premise did not reproduce, the server inherits the launching shell's env, `agent read`/`agent send-keys` have no timeout option. Per-OS support (from K1, issue #124, `docs/planning/decisions/K1-herdr-evaluation.md`) | RISK-HERDR |
| 52 | The K6 opt-in workflow (`.github/workflows/herdr-provider-optin.yml`) runs on operator-owned self-hosted runners that hold a logged-in harness, in a **public** repository. GitHub's guidance is that self-hosted runners "should almost never be used for public repositories", and that fork-PR approval policies are not a protection for them. A fork's pull request can retarget any PR-triggered workflow at those runners, and a collaborator with write access can dispatch a modified branch. The runner-side pre-job hook (`tools/herdr/runner-hooks/`) refuses both, but it is UNVERIFIED on a real runner (bash logic self-tested; PowerShell 7 run by the K6 review; Windows PowerShell 5.1 untested; a compromised admitted job can unset it via the runner's `.env`). No runner is registered and no dispatch has run, so the workflow itself is also UNVERIFIED live (from K6, issue #129, `docs/planning/gates/herdr-runner.md` §1 and §6) | RISK-HERDR |
| 53 | Whether Beacon's Claude Code capture (hooks/OTLP) records the content of an OAC channel notification delivered into the session, or only harness-visible tool/prompt events; same open question for input OAC's Codex adapter delivers. No first-party statement at Beacon `v1.3.29` (from L1, issue #166, `docs/planning/decisions/L1-beacon-memory.md` §6 U1). **Narrowed by L2 (issue #167, 2026-09-29, L1 §11 item 1):** Beacon has no channel-specific handling and records whatever the harness reports as a prompt (hook `UserPromptSubmit`, OTLP `claude_code.user_prompt` / `codex.user_prompt`, or a non-meta `user` session-file entry) verbatim as `prompt.submitted`; OAC's outbound MCP tool-call arguments are confirmed captured (`cli/beacon/internal/endpoint/hooks/claude.go@v1.3.29` L63-77, `cli/beacon/internal/claudesession/mapper.go@v1.3.29` L68-107, L190-210). **Still open:** whether Claude Code and Codex emit any of those for OAC-delivered input — harness behaviour, owned by the operator-run live leg (L1 §12, not CI) | **CLOSED** — CONFIRMED live by L3 (issue #192, 2026-10-01, `docs/planning/decisions/L1-beacon-memory.md` §13 B2-B4; Windows, Beacon `1.3.29` Local, Claude Code `2.1.285`, Codex `0.159.3`): a `notifications/claude/channel` delivery is recorded as `prompt.submitted` (`prompt.text`, `gen_ai.input.messages`, `raw.attributes.prompt`; collection methods `hook` and `otlp`), but not by `beacon endpoint claude sync`, because Claude Code stores it as an `isMeta` `user` entry; the reply-tool call is recorded as `tool.invoked` / `mcp.tool_invoked` with its arguments; Codex input sent with `turn/start` and `thread/queue/add` is recorded as `prompt.submitted` via OTLP (harness `codex_desktop`) and via `beacon endpoint codex sync` (harness `codex_cli`). A fake secret-shaped token in the message body was stored **unredacted** on every capturing path. The capture itself stays an open risk under RISK-BEACON (`06-security.md` §14 row 23) |
| 54 | The exact shape and stability of Beacon's memory item ID and of `get_memory` / `get_memory_context` results across releases (from L1, issue #166, L1 §6 U2) | **CLOSED** — CONFIRMED by L2 (issue #167, 2026-09-29, L1 §11 item 2): ID is `memory_` + 32 hex chars, a truncated SHA-256 over project ID, candidate ID, kind and title (`cli/beacon/internal/learning/store.go@v1.3.29` L694-701); `get_memory` returns `LearningMemoryV1` with `schema_version` `beacon.learning.v1` (`pkg/asymptoteobserve/learning.go@v1.3.29` L3, L108-122); `search_memory` / `get_memory_context` summaries carry no version field (`cli/beacon/internal/mcpserver/server.go@v1.3.29` L80-99). Retrieved 2026-09-29. Not an OAC dependency (L1 §4 Q1). RISK-BEACON stays open for row 53 and the fast-moving pin |
| 55 | Whether one Beacon `memory.db` can be read by several harness sessions concurrently without a documented locking model (from L1, issue #166, L1 §6 U3) | **CLOSED** — CONFIRMED by L2 (issue #167, 2026-09-29, L1 §11 item 3): SQLite via `modernc.org/sqlite` `v1.59.0`, `PRAGMA journal_mode=WAL` and `busy_timeout=5000` on every per-call connection, single-statement upserts (`cli/beacon/internal/learning/store.go@v1.3.29` L14, L65-92, L569-595); MCP memory tools are read-only. No Beacon doc states the model, so it is re-read on every pin move. Retrieved 2026-09-29 |
| 56 | Whether Beacon's Codex integration (`beacon endpoint install` writes OTLP exporter tables to `~/.codex/config.toml`; hooks to `~/.codex/hooks.json`; `beacon mcp connect` edits one `beacon-managed` entry) conflicts with OAC's Codex adapter launch (from L1, issue #166, L1 §6 U4) | **CLOSED** — REFUTED by L2 (issue #167, 2026-09-29, L1 §11 item 4): Beacon replaces only `[otel]` / `[otel.*]` in `config.toml` and copies every other recognised table through (`cli/beacon/internal/endpoint/harness/harness.go@v1.3.29` L464-503); edge case: its line merge recognises a header only when the trimmed line starts with `[` and ends with `]` (L471-485), so a header with a trailing comment (e.g. `[mcp_servers.oac] # x`) directly after an `[otel]` section is dropped with its keys — not OAC's planned path, which registers `oac` with `codex mcp add` rather than by hand (not source-checked against Codex; L1 §12 B1 checks it), writes only `env` keys and its own hooks in Claude `settings.json` (same file L359-397; `cli/beacon/internal/endpoint/hooks/settings_hooks.go@v1.3.29` L191-205), and names its servers `beacon` / `beacon-managed`; no write touches a server named `oac` or the `--dangerously-load-development-channels` launch. Side effect recorded: user-level `log_user_prompt = true` also applies to OAC-launched Codex (feeds row 53). Retrieved 2026-09-29. **L3 live check (issue #192, 2026-10-01, L1 §13):** no collision seen. With Beacon `1.3.29` installed, the Claude development-channel launch connected its server, and Codex `0.159.3` took `turn/start` and `thread/queue/add` input on a daemon-loaded thread. The operator's `config.toml` had no trailing-comment headers. B1 itself was NOT RUN (operator decision on #168). The only B0-to-B7 config differences were Codex's own folder-trust entry in `config.toml` (`[projects.…]`, section diff) and a `~/.claude.json` change attributed, by inference, to Claude Code's own trust write (#206; content not read, and the orchestrating Claude Code session also writes that file); neither is a Beacon write |
| 57 | B2 rows carried from §3 without a re-check against the pin ("Carried unchanged" or "PARTIAL"), classified UNVERIFIED for Gate S0 (#228, 2026-10-02; `docs/planning/REVERIFICATION-B2.md` "S0 classification note"). Listed in STATUS.md as one grouped entry | RISK-B2-CARRIED |
| 58 | Hook-to-shim pairing by OS-reported peer PID and process ancestry: whether a Claude Code hook subprocess and its stdio MCP server subprocess share an OS-observable common ancestor on every OS, and which calls yield the peer PID (macOS) and parent PID (#236, 2026-10-02; `docs/planning/decisions/C4-session-identity.md` §3 "Pairing requirement") | RISK-LOCAL-IPC |
| 59 | The old C6 §5 frame's G5 X2 failure did not reproduce across runs. It was f on 2026-09-27 (Codex `0.157.1`) and in E1 arm 0 (`0.160.0`, fresh thread), and x in K8 (`0.160.0`, X1-then-X2 thread). Cause UNVERIFIED: model variance, or the Codex version combined with the shared-thread history (#220, 2026-10-03; C13 §9 dated note; `docs/planning/gates/herdr-runs/G5-c13-2026-10-02.md` findings) | RISK-G5 |
| 60 | Whether a herdr run writes a harness config file and reverts it to the same bytes. The driver hashes `~/.codex/config.toml` and the other files only at run start and teardown (#220, 2026-10-03; `docs/planning/gates/herdr-runs/G5-c13-2026-10-02.md` findings) | RISK-HERDR |
| 61 | Whether the v0.1 transport carries presence records (announcement, withdrawal, staleness, carrier loss) as `spec/session-channels.md` §7.2 requires, and how it meets SC-DLV-066 once records cross installs. G3 verified only peer discovery; presence records were not exercised. C7 §7 local mode has no transport-layer authorization, so v0.1 presence and discovery are same-install only (operator decision on #43) (#43, 2026-10-03; `docs/planning/gates/G3-result.md`) | RISK-G3 |
| 62 | Whether Codex's `thread/queue/add` keeps the order of several inputs queued during a running turn. G2's `busyqueue` step queued one input only; no first-party statement of order is cited. Spec §7.4 makes in-order hand-off a SHOULD (SC-DLV-080) (#43, 2026-10-03; `docs/planning/gates/G2-result.md`) | **CLOSED** — CONFIRMED by run (#343, 2026-10-07, Codex `0.161.0`): two `thread/queue/add` sent during one running turn ran as two turns, one per idle, in the order added (`docs/planning/gates/fixtures/s3-codex-capture/transcript-2026-10-07-0.161.0-herdr.jsonl` L135-L139, L826-L865; `docs/planning/gates/herdr-runs/S3-codex-2026-10-07.md` row 8). One observation, not a first-party guarantee. RISK-CODEX-EXPERIMENTAL stays open for its other items |
| 63 | Whether `ed25519-dalek` `3.0.0`'s `VerifyingKey::verify_strict` gives the verdicts of `spec/security.md` [SEC-SIG-021] to [SEC-SIG-024] (S below L, no small-order or non-canonical `R` or `A`, cofactorless equation) on every `sec-sig` fixture. *Narrowed 2026-10-03 (PR #265 review):* its source checks small-order `R` and `A` and the cofactorless equation by octet comparison of the recomputed `R` (https://docs.rs/ed25519-dalek/3.0.0/src/ed25519_dalek/verifying.rs.html); `VerifyingKey::from_bytes` keeps a non-canonical key encoding, so SEC-KEY-034 must reject one at admission. What stays open is running the fixtures. Its documentation says it performs scalar and point malleability checks and denies weak keys (https://docs.rs/ed25519-dalek/3.0.0/ed25519_dalek/struct.VerifyingKey.html, retrieved 2026-10-03); no Rust build has run the `sec-sig` fixtures. Node.js 25.2.1 / OpenSSL 3.5.4 accepted the small-order-`R` fixture in the E5 vector check (#45, 2026-10-03) | **CLOSED** — CONFIRMED by F3 (#52, 2026-10-06): `core/tests/conformance.rs` test `verify_strict_alone_gives_the_sec_sig_verdicts` runs 21 of the 24 `sec-sig` fixtures (all but the three nonce/signature form fixtures, which are refused before any arithmetic) through `ed25519-dalek` `3.0.0` `VerifyingKey::verify_strict` alone, over `oac-core`'s signing input, and its verdict is the fixture's on every one: it rejects `SEC-SIG-021.n01`/`.n02` (S not below L), `SEC-SIG-022.n01` to `.n03` (small-order and non-canonical `R`) and `SEC-SIG-024.n04`/`.n05` (mixed-order `R` and `A`), and accepts every positive. Key admission refuses non-canonical and small-order public keys (`core/src/keys.rs` `PublicKey::from_octets`, unit test `admission_refuses_non_canonical_and_small_order_keys`; [SEC-KEY-034]). Run on Windows (`x86_64-pc-windows-msvc`) and Linux (WSL, `x86_64-unknown-linux-gnu`) with Rust 1.98.1, and in CI |
| 64 | Whether `serde_jcs` `0.2.0` produces RFC 8785 output identical to the `expected.canonical` values of the `sec-*` fixtures (member order by UTF-16 code units, string escapes, non-ASCII text, unknown members). The fixtures were checked by two independent JavaScript serializers only (#45, 2026-10-03) | **CLOSED** — CONFIRMED by run (#6, 2026-10-07): `core/tests/conformance.rs` `conformance_fixtures` runs `run_canonical` (L854-L876, called at L930-L935) on every fixture that has an `expected.canonical`. There are 10, all `sec-*`: `SEC-SIG-010.p01`-`p04`, `SEC-SIG-011.p01`, `SEC-SIG-013.p01`-`p02`, `SEC-KEY-041.p01`, `SEC-PRS-001.p01` and `SEC-RCT-001.p01`. They go through `core::canonical::signed_text`, which writes every member name, string and number with `serde_jcs` `0.2.0` and sorts members itself by UTF-16 code units (`core/src/canonical.rs` L1-L14, L83-L130). `cargo test -p oac-core --test conformance` at `5877b39` reports `"canonical": 10` and passes; CI runs it on all three OSes. RISK-SEC-SPEC stays open for its other items |
| 65 | Which `thread/queue/add` errors, if any, mean "not now" (`spec/session-channels.md` [SC-DLV-008]) rather than a failed hand-off. Source at `rust-v0.160.0` shows at least four refusals, none meaning "not now", so each is `handoff-failed`: an ephemeral thread (`thread_queue_processor.rs` L261), a host with no queue service (L246), a subagent thread that does not accept direct input, either a loaded multi-agent-v2 subagent or an unloaded `ThreadSpawn` subagent (`ensure_direct_input_allowed`, L292-L309, called at L83), and an archived thread (L282). **Consequence:** under queue-only delivery, OAC cannot deliver to any of these threads at all. Other refusals are not classified (#274, 2026-10-04; `spec/bindings/mcp.md` §8.2.1). Owner G7 (#68). *Narrowed 2026-10-07 (#343):* the ephemeral and archived refusals are recorded live, `-32600` with the source's messages, and so is an unknown thread, which is `-32603 "failed to read thread: invalid thread-store request: no rollout found for thread id <id>"`, not the `-32600 "thread not found"` the source suggests (`docs/planning/gates/fixtures/s3-codex-capture/transcript-2026-10-07-0.161.0-herdr.jsonl` L114-L119, L1030-L1032, L109-L110). The subagent and no-queue-service refusals cannot be triggered by a documented client request and stay source-only. Which of these, if any, means "not now" is still open. *Dated note, 2026-10-08 (Gate S3 criterion 5, PR #342 review finding 1):* the fake Codex app-server no longer models the two subagent refusals or "no queue service". An add to a subagent thread answers its own `NOT_MODELLED` error, and the queue-unavailable control (`oacFake/queue/setAvailable`) is removed. The contract suite's turned-away hand-off and the pipelines demonstration now use the recorded archived refusal instead (L1030-L1032). No test or fake behaviour rests on the three; what Codex does in those cases stays open here | RISK-CODEX-EXPERIMENTAL |
| 66 | Runtime behaviour of the Codex queue that `spec/bindings/mcp.md` §8.2.1 relies on, read from source at `rust-v0.160.0` only. (a) After an interrupted turn nothing dispatches until a turn completes uninterrupted: an item already queued waits (`service.rs` L549-L566), and an add made later to an idle thread whose last turn was interrupted also waits, because `wake_if_loaded` skips a thread whose agent status is `Interrupted` (L477). A TUI user who interrupts and walks away stalls every later delivery, each reported `handed-to-harness`. (b) Other daemon clients can reorder, update or delete a queued item. (c) An add to an unloaded thread waits. (d) An extra member in a `thread/queue/add` request is probably ignored. (#224 C5; #274, 2026-10-04.) Owner G7 (#68); the CDX-002 fixture plan includes the idle-after-interrupt case. *Narrowed 2026-10-07 (#343), by run on Codex `0.161.0` (`docs/planning/gates/herdr-runs/S3-codex-2026-10-07.md`):* (a) CONFIRMED, both halves: two items already queued when the turn was interrupted waited (nothing in 25 s) and ran one per idle after the next uninterrupted turn (`docs/planning/gates/fixtures/s3-codex-capture/transcript-2026-10-08-0.161.0-queued-interrupt-herdr.jsonl` L79-L170, `docs/planning/gates/herdr-runs/S3-codex-2026-10-08.md`, 2026-10-08); and an add made after `turn/interrupt` to the idle thread waited (nothing in 25 s), and ran after the next turn completed (`docs/planning/gates/fixtures/s3-codex-capture/transcript-2026-10-07-0.161.0-herdr.jsonl` L901-L955); (c) CONFIRMED and extended: an add to an unloaded thread was accepted and waited, and loading the thread with `thread/resume` dispatched it (L956-L1019); (d) CONFIRMED: an extra member was accepted and ignored (L866-L873). (b) is still open | RISK-CODEX-EXPERIMENTAL |
| 67 | App-server behaviours the fake Codex app-server (F9, #58; `tests/fakes/codex-app-server/README.md` "Source-only behaviours") models from source at `rust-v0.160.0` only, beyond rows 65-66: (a) a `thread/queue/add` on a connection that did not set `capabilities.experimentalApi` is refused `-32600 "thread/queue/add requires experimentalApi capability"` (`message_processor.rs` L975-L979, `experimental_api.rs` L30-L32); (b) a request before `initialize` is refused `-32600 "Not initialized"` (`message_processor.rs` L971-L972); (c) the frames of a turn that ends `interrupted` (only the `TurnStatus` value is in source; no fixture records an interrupted turn); (d) `thread/resume` of an unknown thread id gets the same `-32600 "no rollout found for thread id <id>"` that D6 recorded for a known thread before its first turn (`thread-store/src/local/read_thread.rs` L97-L102; `thread_processor.rs` L3194-L3195 maps `ThreadNotFound` to it). No fixture records any of the four (#58, 2026-10-06). Owner G6 (#67) for (a), G7 (#68) for (b) to (d) | **CLOSED** — CONFIRMED by run (#343, 2026-10-07, Codex `0.161.0`, `docs/planning/gates/herdr-runs/S3-codex-2026-10-07.md`): (a) `docs/planning/gates/fixtures/s3-codex-capture/transcript-2026-10-07-0.161.0-herdr.jsonl` L99-L100, (b) L92-L93, (c) L899-L907 (`turn/completed` status `interrupted` with `items: []` and `itemsView: "notLoaded"`, and no `item/completed` for the agent message the turn had started; the fake now sends these frames), (d) L106-L108, each with the message above. The fake Codex app-server replays each from the fixture |
| 68 | Fake Codex app-server behaviours (F9, #58) modelled from source at `rust-v0.160.0` that had no row of their own (Gate S3 finding F-1, PR #342 `docs/planning/decisions/F-6-stage3-exit.md` §3). (a) A `thread/queue/add` to a loaded, idle thread whose last turn was not interrupted starts a turn at once (`tests/fakes/codex-app-server/lib/model.mjs` `wakeIfLoaded` and `dispatchHead`). Every recorded `thread/queue/add` was sent during a running turn, and `spec/bindings/mcp.md` §8.2.1 (L959-L965) says only the busy case was shown live. The `contract/adapter/no-polling` check and the pipelines demonstration reach Codex input this way. (b) One queued item per idle, from the head of the queue. (c) The `thread not found: <id>` refusal, which row 65 does not classify. (#6, 2026-10-07.) Owner #343 (Stage 1 fixture capture), then G7 (#68) | **CLOSED** — by run (#343, 2026-10-07, Codex `0.161.0`, `docs/planning/gates/herdr-runs/S3-codex-2026-10-07.md`): (a) CONFIRMED on the TUI's own idle thread, its last turn `completed`: the add was answered and a turn started at once, its `userMessage` carrying `clientId` = `clientUserMessageId`, with no `turn/start` sent (`docs/planning/gates/fixtures/s3-codex-capture/transcript-2026-10-07-0.161.0-herdr.jsonl` L57-L73, L90), and again on a client thread (L866-L873); the fake now sends the second `thread/queue/changed` before the response, as recorded. (b) CONFIRMED (L826-L865; row 62). (c) REFUTED: an unknown thread is `-32603 "failed to read thread: invalid thread-store request: no rollout found for thread id <id>"` (L109-L110); the fake now answers that. Row 65 still classifies none of the refusals as "not now" |
| 69 | Fake Claude Code endpoint (F8, #57) inferences beyond the recordings (Gate S3 finding F-1). (a) `tools/call` ids after the session's first go on 3, 4, ... with `progressToken` equal to the id; both recorded calls are id 2 (D6 line 13, G1 Box C line 24; `tests/fakes/claude/src/lib.rs`). (b) The synthetic `claudecode/toolUseId` `toolu_fake<20 digits>`: its `toolu_` prefix is recorded (D6 line 13), its suffix is not the recorded form. (#6, 2026-10-07.) Owner #343 (Stage 1 fixture capture) | **CLOSED** — by run (#343, 2026-10-07, Claude Code `2.1.285`, `docs/planning/gates/herdr-runs/G1-2026-10-07.md`): (a) CONFIRMED: three `reply` calls in one session are ids 2, 3 and 4, each with `progressToken` equal to its id (`docs/planning/gates/fixtures/g1-claude-wake/transcript-2026-10-07-2.1.285-herdr.jsonl` L14, L16, L18); `tests/fakes/claude/tests/replay.rs` `later_tool_calls_continue_the_recorded_id_sequence` replays them. (b) Recorded form: every one of the 17 `toolUseId` values in the Claude fixtures, 2.1.282 to 2.1.285 (the three of this capture among them), is `toolu_01` plus 22 ASCII letters and digits. An adapter checking that form could refuse the old `toolu_fake…`, so the fake now writes `toolu_01OacFake<15 digits>`, which has it |
| 70 | Which connections live Codex sends a thread's notifications to. In the S3 capture (Codex `0.161.0`), `thread/status/changed`, `thread/closed`, `thread/goal/cleared` and `thread/archived` for a thread reached a connection subscribed only to another thread, and a connection after its `thread/unsubscribe`. A connection that subscribed to nothing received none (`docs/planning/gates/fixtures/s3-codex-capture/transcript-2026-10-07-0.161.0-herdr.jsonl` L63-L69, L829, L959-L997). The fake Codex app-server sends them to the thread's subscribers only (`tests/fakes/codex-app-server/README.md` "Not modelled"), so a fake-backed test sees fewer notifications than a live one (#6, 2026-10-08, Stage 3 exit re-run). Owner G6 (#67) | RISK-CODEX-EXPERIMENTAL |

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
