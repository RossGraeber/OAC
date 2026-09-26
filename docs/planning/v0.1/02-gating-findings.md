# 02 — Gating findings

**Purpose.** State the go/no-go structural finding behind the five Stage 1 gates, then
give, for each of G1-G5: pass criteria, failure criteria, fallback, and the current
verdict. Per PLANNING-PROMPT.md §4: "The build phase may not proceed past a gate on a
'probably'." Every verdict below is a closed value — `PASS`, `PASS (FALLBACK TAKEN)`,
`FAIL`, or `NOT RUN` — never a hedge.

**Generated-summary notice.** This file is derived from
`docs/planning/gates/G1-result.md` through `docs/planning/gates/G5-result.md`, per
`docs/planning/gates/README.md` §"Reconciliation with PLANNING-PROMPT.md §9 item 3". It
is **not hand-authored**: a gate re-run writes the per-gate `G<n>-result.md` file, and
this file is regenerated from those five files, never hand-edited in place for a single
gate's result.

**Precedence.** Per gate, `docs/planning/gates/G<n>-result.md` is authoritative for
verdict, date, and pins-relied-on — this file restates those fields, it does not
re-derive or override them. `docs/planning/PINS.md` is authoritative for version
numbers and release dates wherever this file or a `G<n>-result.md` file could be read as
disagreeing about one.

**Cross-references (repo-relative paths).** `docs/planning/PLANNING-PROMPT.md` §4 (gate
definitions) and §9.3 (this file's required content); `docs/planning/gates/README.md`
(evidence-store policy, naming convention, re-run/invalidation policy); `docs/planning/
STATUS.md` (current stage, Gate verdicts table, Pins table, Open UNVERIFIED items);
`docs/planning/PINS.md` (pin table, single source of truth for versions); the per-gate
reference files `.claude/skills/oac-gates/references/G1-claude-wake.md` through
`G5-provenance.md` (verbatim pass/fail/fallback criteria source).

**Naming.** Product and repository: **Open Agent Channel (OAC)**. Normative protocol
specification: **OAC Session Channels**. CLI binary: **`oac`**. Per ADR-001-A1
(`docs/planning/ADR-001-AMENDMENTS.md`), this file never writes "Session Channels" alone
or `sessionchannels`.

---

## 1. Structural finding — no attach to an arbitrary already-running session

Verbatim, PLANNING-PROMPT.md §4:

> **Neither Claude Code nor Codex supports attaching an external channel to an
> arbitrary, already-running session process through supported interfaces.** Both
> support live, active, full-duplex injection into a session that was **launched
> OAC-enabled**: Claude Code by starting with `--channels`, Codex by running through the
> shared local app-server daemon (or an OAC-owned app-server with the TUI attached via
> `--remote`).

This finding is prior to, and outside, the five gates below: **no gate spikes or
records a FAIL against attach-to-arbitrary-session** — it is not a supported target for
any gate (`.claude/skills/oac-gates/SKILL.md` §"Go/no-go vs. fallback": "Do not spike or
record a FAIL against attach-to-arbitrary-session; it is not a supported target for any
gate"). Each gate instead proves injection into a session launched OAC-enabled: Claude
Code via `--channels` at session start (G1), Codex via the shared app-server daemon or
an OAC-owned app-server with `codex --remote` (G2).

**ADR-001-A2 proposal.** PLANNING-PROMPT.md §4 proposes redefining ADR-001's validation
criterion on this finding: "existing session" becomes "a live interactive session,
launched OAC-enabled, whose owner is a human at a terminal, into which OAC injects
without owning the harness's model loop." This amendment is recorded in
`docs/planning/ADR-001-AMENDMENTS.md` §ADR-001-A2; see that file for the full text and
the resolution of conflict-register row C2.

## 2. Verdict summary

One row per gate. Every verdict below matches
`docs/planning/STATUS.md`'s Gate verdicts table cell-for-cell.

| Gate | Verdict | Disposition / fallback | Pins relied on | Result file |
|---|---|---|---|---|
| G1 Claude wake | **PASS** | go/no-go, no fallback | Claude Code (Channels); MCP — current era; MCP — legacy era; Rust MCP SDK (rmcp) | `docs/planning/gates/G1-result.md` |
| G2 Codex live inject | **PASS** (`0.157.1`, re-run 2026-09-26, Windows only) | go/no-go if both paths fail; fallback: OAC owns the app-server, user runs `codex --remote ws://…` | Codex CLI / app-server | `docs/planning/gates/G2-result.md` |
| G3 Zenoh local peer | NOT RUN at gate level — Windows 11 **PASS**, Linux (WSL2) **PASS**, macOS NOT RUN (parked) | has fallback: fixed local endpoint, multicast scouting disabled | Zenoh; Rust toolchain | `docs/planning/gates/G3-result.md` |
| G4 MCP dual-era server | NOT RUN | has fallback: two server entry points sharing one core | Claude Code (Channels); MCP — current era; MCP — legacy era; Rust MCP SDK (rmcp); Codex CLI / app-server (Codex leg) | `docs/planning/gates/G4-result.md` |
| G5 Provenance | NOT RUN | no fallback stated; failure invalidates DESIGN acceptance criterion 6 | Codex CLI / app-server; Claude Code (Channels) | `docs/planning/gates/G5-result.md` |

**Reason for `NOT RUN` on G3 (partially), G4 and G5.** Epic D spikes D1-D5 opened, but
D4 (G4) and D5 (G5) have not executed, and D3 (G3)'s macOS leg is parked
(`docs/planning/gates/G3-result.md`, `G4-result.md`, `G5-result.md`). D1 (G1) and D2 (G2)
have both run and PASSED — see `docs/planning/gates/G1-result.md` and
`docs/planning/gates/G2-result.md`. `docs/planning/STATUS.md`'s "Current stage" section
states the rest of Stage 1 and Stages 2-6 stay blocked until Stage 0 (Epic B) and Stage 1
fully complete.

**Verdict vocabulary.** Closed: `PASS` | `PASS (FALLBACK TAKEN)` | `FAIL` | `NOT RUN`.
No fifth value, no hedge — per `oac-gates` "never probably" rule.

## 3. G1 claude-wake

- **Verdict:** PASS (see `docs/planning/gates/G1-result.md`)
- **Pins relied on:** `Claude Code (Channels)`, `MCP — current era`, `MCP — legacy
  era`, `Rust MCP SDK (rmcp)` (`docs/planning/gates/G1-result.md`)
- **Result file:** `docs/planning/gates/G1-result.md`
- **Surface:** Claude Code (Channels) — **research preview**
  (`docs/planning/v0.1/01-capability-matrix.md` §1).

**Pass criteria** (verbatim, `.claude/skills/oac-gates/references/G1-claude-wake.md`):

- Server declares `capabilities.experimental["claude/channel"] = {}` and negotiates a
  legacy MCP revision (`2025-11-25` or earlier); use `MCP_PROTOCOL_NEGOTIATION=legacy`
  for stdio servers if needed.
- A `notifications/claude/channel` carrying `content` (string) and `meta`
  (string-to-string map) wakes an idle session as a user turn and appears as a
  `<channel>` tag with the expected attributes (one attribute per identifier-safe
  `meta` key).
- A second notification sent mid-turn is queued and delivered at the next turn, in
  order (not dropped, not interleaved out of order).
- Claude replies through an ordinary MCP tool (conventionally `reply`).
- The `--dangerously-load-development-channels` interactive confirmation dialog is
  actually exercised during the spike — not bypassed, scripted around, or skipped.

**Failure criteria.** Any pass criterion above unmet is a FAIL. There is no partial
pass: a server that wakes the session but loses mid-turn ordering, or that never
triggers the confirmation dialog because the spike bypassed it, is a FAIL, not a
qualified pass.

**Fallback.** None. PLANNING-PROMPT.md §4: "there is no supported fallback, so this is
v0.1 go/no-go." Failure blocks the Claude adapter entirely.

## 4. G2 codex-inject

- **Verdict:** PASS (`0.157.1`, re-run 2026-09-26, Windows only — see
  `docs/planning/gates/G2-result.md`)
- **Pins relied on:** `Codex CLI / app-server` (`docs/planning/gates/G2-result.md`)
- **Result file:** `docs/planning/gates/G2-result.md`
- **Surface:** Codex CLI / app-server — **experimental** (per-method gating via
  `capabilities.experimentalApi`) (`docs/planning/v0.1/01-capability-matrix.md` §1).

**Pass criteria** (verbatim, `.claude/skills/oac-gates/references/G2-codex-inject.md`):

- A TUI launched normally (no config overrides) attaches to a running
  `codex app-server daemon` (started via `codex app-server daemon start`, control
  socket `CODEX_HOME/app-server-control/app-server-control.sock`).
- A second client of the same daemon delivers a message using `thread/queue/add`
  (queued until idle) or `turn/start` (when idle) — or `turn/steer` if delivering into
  an active turn.
- The TUI user sees the delivered message and the model answers it.
- OAC holds no OpenAI credentials at any point in the exchange; the app-server uses
  the saved CLI login (`CODEX_HOME/auth.json` or OS keyring) throughout.

**Failure criteria.** Any of the four unmet on the primary (implicit daemon attach)
path is a failure of that path — run the fallback before recording the gate FAIL. The
gate only fails outright if both paths fail.

**Fallback.** OAC owns the app-server; the user runs `codex --remote ws://…` to attach
the TUI to the OAC-owned app-server. Record which path (implicit attach or `--remote`)
actually passed in the gate result. Failure of both paths is v0.1 go/no-go.

## 5. G3 zenoh-peer

- **Verdict:** NOT RUN at gate level — Windows 11 PASS, Linux (WSL2) PASS, macOS NOT RUN
  (parked); see `docs/planning/gates/G3-result.md`
- **Pins relied on:** `Zenoh`, `Rust toolchain` (`docs/planning/gates/G3-result.md`)
- **Result file:** `docs/planning/gates/G3-result.md`
- **Surface:** Zenoh — **supported**
  (`docs/planning/v0.1/01-capability-matrix.md` §1).

**Pass criteria** (verbatim, `.claude/skills/oac-gates/references/G3-zenoh-peer.md`):

- Discovery over loopback with UDP multicast scouting (`224.0.0.224:7446`,
  `interface: "auto"`) on Windows 11, macOS, and Linux, on Zenoh **>= 1.10.0**.
- Discovery with multicast scouting disabled, using a locally shared rendezvous
  endpoint, on the same three platforms.
- A TLS listener bound to localhost (`127.0.0.1`) works in both modes above.
- Multiple peers per host do not collide on ports (peers listen on dynamic TCP ports by
  default).

**Failure criteria.** Failure on any one platform for the multicast path, or any
platform for the no-multicast path, is a failure of that path specifically — attempt
the fallback for the failing case and record per-platform results rather than a single
blended verdict. Record `PASS (FALLBACK TAKEN)` where the fallback (fixed local
endpoint, no scouting) is what actually passed for a platform, and reserve plain `PASS`
for platforms where the primary multicast path passed.

**Fallback.** Fixed local endpoint with multicast scouting disabled (the second pass
criterion above, taken as the operating mode rather than merely tested as an
alternative). A platform passing only that way is `PASS (FALLBACK TAKEN)`.

**Note.** `zid` ACL subjects are explicitly unauthenticated — this gate tests
discovery/connectivity, not authorization; a passing G3 does not validate `zid`-based
ACLs (`.claude/skills/oac-gates/references/G3-zenoh-peer.md`).

## 6. G4 mcp-dual-era

- **Verdict:** NOT RUN
- **Pins relied on:** `Claude Code (Channels)`, `MCP — current era`, `MCP — legacy
  era`, `Rust MCP SDK (rmcp)`, `Codex CLI / app-server` (Codex leg)
  (`docs/planning/gates/G4-result.md`)
- **Result file:** `docs/planning/gates/G4-result.md`
- **Surfaces:** Claude Code (Channels) — **research preview**; MCP current
  (`2026-07-28`) and legacy (`2025-11-25`) — **supported**
  (`docs/planning/v0.1/01-capability-matrix.md` §1).

**Pass criteria** (verbatim, `.claude/skills/oac-gates/references/G4-mcp-dual-era.md`):

- The legacy path registers as a channel (negotiates `2025-11-25` or earlier) and
  delivers notifications successfully.
- The current-revision (`2026-07-28`) path serves `tools/call` correctly, carrying OAC
  `_meta` provenance (D4's own criterion: an OAC-defined `_meta` extension key on the
  response, not the protocol-version mechanism below). §3.3 notes `_meta` keys carry
  prefix rules — prefixes whose second label is `modelcontextprotocol` or `mcp` are
  reserved, so OAC's `_meta` key must not collide with those.
- Separately, every request on the `2026-07-28` path carries
  `_meta["io.modelcontextprotocol/protocolVersion"]` — this is the stateless-revision
  carrier §3.3 requires on every request (distinct from the OAC provenance key above; do
  not conflate the two).
- Neither path degrades the other across a full session (run both concurrently, not
  sequentially, and confirm neither breaks).
- A server negotiating `2026-07-28` is confirmed to be **rejected as a channel** — run
  this negative case explicitly; the constraint is real, not folklore, per PLANNING-PROMPT
  §3.1's stated MCP version constraint.

**Failure criteria.** Any of the five unmet is a failure of the single-process design;
take the fallback and record the result as `PASS (FALLBACK TAKEN)` rather than a plain
`PASS`, or `FAIL` if the fallback was not attempted or also failed.

**Fallback.** Two server entry points sharing one core. Record the cost of this
fallback (extra process, duplicated negotiation code, or whatever the spike surfaces)
in the gate result — the fallback being available does not make its cost free to skip
documenting.

**Conflict closed.** This gate closes conflict-register row C5: "Claude channel path
needs legacy MCP; Codex tool path may negotiate current MCP" — G4 is the direct test of
whether one process can hold both simultaneously.

## 7. G5 provenance

- **Verdict:** NOT RUN
- **Pins relied on:** `Codex CLI / app-server`, `Claude Code (Channels)`
  (`docs/planning/gates/G5-result.md`)
- **Result file:** `docs/planning/gates/G5-result.md`
- **Surfaces — both providers.** Claude Code (Channels) — **research preview**; Codex
  CLI / app-server — **experimental** (per-method gating)
  (`docs/planning/v0.1/01-capability-matrix.md` §1).

**Pass criteria** (verbatim, `.claude/skills/oac-gates/references/G5-provenance.md`):

- Claude: sender provenance arrives as `meta` attributes on the `<channel>` tag and
  **cannot be forged from message content** — a body claiming to be from someone else
  does not change the rendered `meta` attributes.
- Codex: the machine-generated header is visibly outside the delimited untrusted body
  (the model can distinguish header-asserted sender from body-claimed sender).
- In both cases the model is shown the contradiction rather than the claim alone —
  i.e. the spoofing attempt is observably distinguishable in what the model receives,
  not silently overwritten or silently accepted.
- A `meta` key that is not identifier-safe (not letters/digits/underscore) is
  confirmed to be silently dropped, and the spike confirms no security-relevant
  attribute (sender, device, session id) relies on such a key.

**Failure criteria.** Provenance that cannot be distinguished from content invalidates
DESIGN acceptance criterion 6. Any of the four unmet is a FAIL for that provider;
report per-provider results since the gate spans both.

**Fallback.** None stated. If the spike fails on one provider, that provider's
provenance rendering is a finding requiring a design change before Stage 2 spec
freeze, not a workaround. A failure invalidates DESIGN acceptance criterion 6.

## 8. Re-run / regeneration

Governed by `docs/planning/gates/README.md` §"Re-run/invalidation policy" (B4) — stated
here by reference, not copied:

- A `Pinned version`, `Release date`, or row-presence change in
  `docs/planning/PINS.md`'s pin table invalidates every gate named in that row's
  `Gates affected` cell. This is mechanical, not a judgement call.
- Each affected `docs/planning/gates/G<n>-result.md` reverts its `**Verdict:**` to
  `NOT RUN`, appends the superseded verdict to its `Re-run history` table with an
  `Invalidated by: <surface> pin <old> -> <new>, <YYYY-MM-DD>` note, and gets an
  `> INVALIDATED` callout at the top of the file.
- The matching `docs/planning/STATUS.md` Gate verdicts row reverts to `NOT RUN` in the
  same change.
- This file, `docs/planning/v0.1/02-gating-findings.md`, is **regenerated** from the
  five `G<n>-result.md` files — it is never hand-edited per gate.
- **ACP exemption.** The ACP pin row's `Gates affected` cell is `none`, so an ACP pin
  move invalidates no gate.

## 9. Sourcing discipline

Verdict, pins-relied-on, `PINS.md as-of`, and re-run history for each gate come only
from the corresponding `docs/planning/gates/G<n>-result.md` file. Pass/fail/fallback
criteria text above is verbatim from `.claude/skills/oac-gates/references/G<n>-*.md`
and `docs/planning/PLANNING-PROMPT.md` §4. No criterion, API name, or version string in
this file is invented.

---

## Self-check (`oac-evidence` §8, `oac-planning-package` §6)

- [x] Every quoted method name and flag is verbatim: `capabilities.experimental["claude/channel"]`,
      `notifications/claude/channel`, `thread/queue/add`, `turn/start`, `turn/steer`,
      `codex --remote`, `--dangerously-load-development-channels`,
      `_meta["io.modelcontextprotocol/protocolVersion"]`.
- [x] Every provider surface named (§3-§7) is labelled: Claude Code (Channels) research
      preview; Codex CLI / app-server experimental; MCP current/legacy supported; Zenoh
      supported.
- [x] `oac-planning-package` §6 exit criteria: file exists at
      `docs/planning/v0.1/02-gating-findings.md` with the content issue #25's Acceptance
      checklist describes; house style applied throughout (no open routine choices, no
      code beyond message-shape names, generated-not-hand-authored stated up front);
      naming resolution (ADR-001-A1) applied with no stale spelling.
- [x] Issue #25 acceptance boxes: (1) G1 pass criteria include the legacy MCP
      negotiation requirement and the interactive confirmation dialog — §3; (2) G2 names
      the `--remote` fallback and states failure of both paths is go/no-go — §4; (3) G3
      covers Windows 11/macOS/Linux, multicast and multicast-disabled, TLS bound to
      localhost — §5; (4) G4 names the two-entry-point fallback — §6; (5) G5 covers both
      providers — §7; (6) every gate has pass, fail, and fallback text, none open-ended —
      §3-§7 each carry all three.
- [x] No stale "Session Channels" (unqualified) or `sessionchannels` spelling.
- [x] Every gate's fallback text is closed: G1 none (go/no-go), G2 `codex --remote`
      named, G3 fixed local endpoint named, G4 two entry points named, G5 none stated
      (design-change consequence named) — no open end left on any gate.
