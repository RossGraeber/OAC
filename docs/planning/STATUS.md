# OAC status

The single source of truth for where the project is. The `oac` router skill reads this file
rather than restating it. Update it when a stage opens or closes, when a gate returns a
verdict, or when a pin moves.

**Last updated:** 2026-09-17 (A1/issue #23: `docs/planning/v0.1/00-summary.md` landed,
closing Epic A — one page, no ADR-001/DESIGN restatement, citing by path throughout; the
structural finding (no attach to an arbitrary already-running session; both harnesses
support injection into a session launched OAC-enabled) stated up front, citing
`PLANNING-PROMPT.md` §4 and `ADR-001-A2`; "what v0.1 proves" as the five-leg
`ADR-001.md` line 74 validation criterion per `ADR-001-A2`, citing the ten DESIGN
acceptance criteria by path to `10-stages.md` §11 rather than reproducing them; language
(Rust, single `oac` binary, toolchain `1.98.1`) and process-model (one daemon plus
`oac mcp-shim` shims, OS-peer-authenticated local IPC) stated as decisions, each with one
decisive reason and one reversal condition, rejected alternatives pointed at
`03-decisions-and-amendments.md` §Decision 1/§Decision 2; top three risks named as the
first three `11-risks.md` §R1 rows in that file's own presentation order (RISK-G1,
RISK-G2, RISK-G4), with the file's own caveat restated verbatim — R1 row order is
presentation order, not a ranking, and the two no-fallback gates (G1, G5) carry equal
weight regardless of position — so this file does not re-rank the five R1 risks. No pin
moved, no gate verdict changed, no UNVERIFIED item closed or added.

**Epic A closing self-review** (`.claude/skills/oac-planning-package/SKILL.md` §6,
`PLANNING-PROMPT.md` §11), run against all thirteen `docs/planning/v0.1/*.md` files now
present, per issue #23's task 9:

1. Every ADR-001 boundary and every DESIGN non-goal respected-or-amended — **PASS**.
   `12-deferred.md` §2 restates the nine permanent boundary/non-goal items as
   "boundary, not backlog," never a v0.2 candidate; `03-decisions-and-amendments.md` §5
   runs a per-decision boundary self-check against all seven `oac-boundaries` checks and
   concludes "every boundary check above holds," with the one known gap (containment
   lint's `snake_case`/`adapters`/`cli` coverage) recorded open, not hidden.
2. Every DESIGN acceptance criterion 1-10 maps to a named test in `09-test-strategy.md`
   and a stage in `10-stages.md` — **PASS**. `09-test-strategy.md` §11's ten-row table
   and `10-stages.md` §11's ten-row table both cover all ten criteria verbatim; criteria
   9 (lint, not a runtime test) and 10 (doc proof, currently NOT MET) are recorded as
   such rather than papered over, in both files.
3. Every §5 decision made, or gate-decided with the gate named — **PASS**.
   `03-decisions-and-amendments.md` §1 makes all twelve decisions; decision 3's dual-era
   sub-element is explicitly gate-decided at G4, named, not left open.
4. Every gate G1-G5 has pass, fail, and fallback text — **PASS**.
   `02-gating-findings.md` §3-§7 each carry all three; G1 and G5 state "none" for
   fallback explicitly (go/no-go), which is closed text, not an open end.
5. Every §3 fact re-verified against the pinned version, every UNVERIFIED item closed or
   listed as a risk — **PASS**. `11-risks.md`'s closing traceability table disposes all
   29 "Open UNVERIFIED items" entries below by risk id; none were closed by evidence
   found while writing the package (per `oac-evidence` §5, none had a first-party
   re-verification citation available), so all 29 remain open here and also carry a risk
   row in `11-risks.md`.
6. No neutral interface mentions Zenoh, Claude, Codex, MCP method names, or key
   expressions — **PASS**. `05-interfaces.md` §21 runs the `oac-boundaries` mechanical
   check plus the spec-specific neutral-vocabulary word list against itself and reads
   every hit; all hits sit inside the labelled binding/mapping annex (§15), the
   replacement-proof section (§16, §17), or path citations (§19) — none inside normative
   text (§1-§14), the `ProviderAdapter`/`Transport` signatures, or a core-type field.
7. No section proposes owning a harness's turn loop, holding provider credentials, or
   polling an inbox from an adapter that claims active inbound — **PASS**.
   `03-decisions-and-amendments.md` §5's boundary self-check states this holds for every
   decision (no model-API call, no credential reuse, no cross-provider context store, no
   provider-auth abstraction, no polling while claiming active inbound); `06-security.md`
   and `05-interfaces.md`'s active-delivery no-polling rule carry the same finding for
   the security and interface surfaces respectively.
8. The naming resolution (`.claude/skills/oac-planning-package/SKILL.md` §4) applied
   consistently — **PASS**. A repo-wide sweep of `docs/planning/v0.1/` for bare "Session
   Channels" and `sessionchannels` found every hit is either a naming-rule statement (the
   rule text itself, naming the forbidden spelling to forbid it) or sits inside a source
   document explicitly marked as a pre-rename verbatim quotation (`ADR-001-A1`'s old-text
   blocks in `03-decisions-and-amendments.md` §2, the `DESIGN.md` legacy-name-site list in
   §4, `05-interfaces.md`'s two marked `DESIGN.md` quotations, `08-cli-and-deployment.md`'s
   marked `sessionchannels` CLI example) — no new prose written in the retired name
   anywhere in the package, including `00-summary.md`.

**Result: 8/8 PASS. Epic A is closed.** No cross-reference sweep failure, no UNVERIFIED
label dropped; `STATUS.md`'s history block (this entry) is additive only, prepended above
the existing A12 entry, nothing prior removed.

**Last updated:** 2026-09-17 (A12: `docs/planning/v0.1/11-risks.md` and
`docs/planning/v0.1/12-deferred.md` landed (issue #32) — `11-risks.md`: a ranking
rule (ability to invalidate the `ADR-001.md` line 74 validation criterion and
the DESIGN acceptance criteria 1-10, re-derivable from a stated one-sentence
rule) with five tiers (R1 gate-decided viability G1-G5, R2 preview/experimental
surface drift, R3 evidence/pin drift, R4 design-parameter/platform-runtime, R5
low-impact/non-dependency); 21 risk rows, each with risk, what it invalidates,
an early-warning signal, and a trigger-linked response citing the fallback
already named in `02-gating-findings.md`/`10-stages.md` rather than
re-deriving it; a closing traceability table disposing of all 29
`docs/planning/STATUS.md` "Open UNVERIFIED items" entries by risk id (none
closed by evidence — no cited merged-package section carried first-party
evidence closing any of the 28, per `oac-evidence` §5's "never silently
promoted" rule). `12-deferred.md`: the eight `ADR-001.md` line 63 v0.1
exclusions (group rooms/broadcast, attachments, durable offline mailboxes,
federation, full E2E encryption, GUI, production Gemini/ChatGPT/Cursor
adapters, alternative transports) each as "not in v0.1" with reason and
reversal condition; the nine `ADR-001.md`/`DESIGN.md` boundary items marked
"boundary, not backlog" (model inference, model routing, agent
planning/orchestration, shared context management, replacement provider auth,
unsupported client impersonation, UI/terminal scraping, undocumented private
RPCs, Zenoh concepts in the neutral protocol); seven package-level v0.1
exclusions already decided elsewhere, indexed with their citing file
(permission relay off by default, manual key rotation, transitive license
sweep deferred to Stage 6, C7 presence/discovery gap 1, C7 presence/discovery
gap 2, ACP forward-compatibility-only, Cursor design-proof-only). No pin
moved, no gate verdict changed, no UNVERIFIED item closed or added — all 29
items this file carries stay open, each now also carrying a risk id in
`11-risks.md`. A11:
`docs/planning/v0.1/10-stages.md` landed (issue #31) —
the §8 stage-gate pipeline as seven stages (0-6), each with entry criteria, modules
created or changed by name and responsibility (cited to
`docs/planning/v0.1/07-repository-and-dependencies.md` §1-§5, not re-derived),
prerequisite §5 decisions (cited to `docs/planning/v0.1/03-decisions-and-amendments.md`
§1, with Stage 0 recorded as having none and Stage 1 as having two informative-only
inputs), an executable demonstration, exit artifacts, numbered acceptance criteria, and a
named go/no-go condition; the risk-first ordering rule stated normatively with its three
consequences and the gate-failure-does-not-reorder rule; stage-exit gates named S0-S6 and
distinguished from the five provider/transport gates G1-G5, whose pass/fail/fallback text
stays owned by `docs/planning/v0.1/02-gating-findings.md`; per-spike timebox values
(D1 3d, D2 4d, D3 3d, D4 3d, D5 2d, D6 2d alongside) decided as OAC's own scheduling
parameters with a stated reversal condition, plus the five-step expiry rule (stop at the
box, record as-is, write the honest verdict, never a plain `PASS` on an incomplete run,
record the expiry in the gate file); the DESIGN acceptance-criteria-1-10-to-stage map
with criteria 9 (lint) and 10 (doc proof, currently NOT MET) recorded as not
runtime-tested; and the explicit no-ticket-decomposition statement citing
`docs/planning/backlog/01-epics.json` and the four task files. No pin moved, no gate
verdict changed, no new UNVERIFIED item added — the four items this file carries (the two
C11 shim-boundary items, the E9 NATS/MQTT cells, the Zenoh-containment lint scope gap)
were already open. A10: `docs/planning/v0.1/09-test-strategy.md` landed
(issue #30) — the eight-tier test taxonomy (unit, spec conformance, contract
adapter+transport, security, fake-harness integration, provider integration,
end-to-end, cross-platform CLI smoke) with resilience decided as a named sub-row of the
security tier rather than its own row, and the Zenoh-containment criterion (9) decided
as proven by the `oac-boundaries` CI lint rather than a runtime test, with that lint's
recorded scope gap (no `snake_case`-embedded `zid`/`zenoh` match, no coverage of
`adapters/`/`cli/`) carried forward as an open v0.1 gap; the CI-default rule with
loopback defined explicitly and the anti-exception rule stated; opt-in mechanics
(separate flag/target, exact pinned version per test, a pin move invalidating a
recorded opt-in result the same way it invalidates a gate); the
`contract/adapter/no-polling` no-polling assertion spec; the mandatory
acceptance-criteria (all ten rows, verbatim criterion text) and threat-mitigation (all
twenty rows) traceability tables, every row `not-yet-written`/`NOT RUN`, none marked
done; the fixture capture/refresh process (Stage 1 capture via D6, per-gate location,
pin-move refresh trigger); and the normative/reference-implementation split and
deliberately-omitted-from-v0.1 test surface. No pin moved, no gate verdict changed, no
new UNVERIFIED item added — every UNVERIFIED item this file cites was already open,
carried from `docs/planning/v0.1/06-security.md`, `08-cli-and-deployment.md`, and
`05-interfaces.md`. A9: `docs/planning/v0.1/08-cli-and-deployment.md` landed
(issue #29) — zero-container local path (C2 §9's three-point argument, C7's "no
`zenohd`", cited not re-derived), zero-file local default and `flag > env > file >
default` config precedence, the five-row CLI command surface plus the reserved-
placeholder row, the `oac doctor` check list, the two verified one-line launch commands
(`claude --dangerously-load-development-channels server:oac`;
`codex mcp add oac -- oac mcp-shim`) with the `--channels`-does-not-apply finding cited
from C2 §6 rather than re-derived, the preserved Claude consent-dialog step, the G2
live-inject target statement stated as pending (verdict `NOT RUN`) with its fallback
named, the "one command" reconciliation (`oac start` vs. the two per-harness launch
commands), LAN hooks scoped to the reserved pairing subcommand only, and the
smallest-design deliberately-omitted list; no pin moved, no gate verdict changed, no new
UNVERIFIED item added — the two items this file cites (Codex daemon-attach-default
UNVERIFIED, the 2026-09-17 `app-server` fetch drift signal) were already open, cited from
C2. A8: `docs/planning/v0.1/07-repository-and-dependencies.md`
landed (issue #28) — module ownership table and dependency direction rule, the two
ADR-001 containment boundaries (Zenoh containment; the Claude/Codex compatibility-shim
boundaries, both still UNVERIFIED per C11 and carried forward, not resolved), the
ten-row dependency inventory with licenses and consuming modules, the copyleft flag on
`zenoh`, the Apache-2.0 compatibility verdict for direct dependencies, and the
transitive-sweep deferral to Stage 6; no pin moved, no gate verdict changed, no new
UNVERIFIED item added. A7: `docs/planning/v0.1/06-security.md` landed
(issue #27) — identities (the C4 four-layer model), model-generated text never
establishes identity as its own named section, trust boundaries per process-boundary
crossing, pairing (both C5 flows), authorization (default-deny, per-session-id
allowlists), transport security (the Zenoh-ACL-vs-signature relationship, `zid` never an
ACL subject), replay and duplicate handling, provenance rendering per provider (Claude
`meta` keys, Codex header-and-delimiter framing), permission relay off by default with
its three-strand justification, local IPC peer authentication, cross-project leakage, and
a twenty-row threat table merging C4 §13/C5 §13/C6 §12; every mitigation stated as
designed, not proven, per the Pre-Stage 0 gate-verdict caveat; no pin moved, no gate
verdict changed, no new UNVERIFIED item added — every item this file relies on was
already open, cited from C4/C5/C6. A6: `docs/planning/v0.1/05-interfaces.md` landed
(issue #26) — OAC spec surface (envelope/content model, addressing, capability
negotiation, presence/discovery with its two recorded gaps, active-delivery no-polling
rule, replies/correlation, the frozen delivery-state set, the closed error taxonomy,
versioning policy, unsupported-capability behaviour), the `ProviderAdapter` and
`Transport` contracts and core neutral types, and the ACP-adapter and NATS/MQTT
design-for-replacement proofs, all with normative text separated from reference-
implementation notes; six new NATS/MQTT optional-capability items added below as
UNVERIFIED; no pin moved, no gate verdict changed. A5:
`docs/planning/v0.1/04-architecture.md` landed
(issue #22) — components and responsibilities table, the four things the daemon owns
(cited to C2 §1), the process-boundary diagram (key material/transport peer location),
per-provider inbound/outbound flows drawn separately for Claude and Codex, local and LAN
deployment topologies, rejected alternatives, and the boundary/evidence self-checks; no
pin moved, no gate verdict changed, no new UNVERIFIED item added. A4:
`docs/planning/v0.1/03-decisions-and-amendments.md`
landed (issue #21) — all twelve §5 decisions assembled and cited (decision 3's dual-era
sub-element gate-decided at G4, not deferred), ADR-001-A1-A3 reproduced verbatim from the
amendments ledger, the C1-C10 conflict register and open C11-C12 entries reproduced;
C1-C7 decision documents remain authoritative, not stubbed; no pin moved, no gate verdict
changed. A2: `docs/planning/v0.1/01-capability-matrix.md` landed
(issue #24) — surface labels, shim-boundary carry, full capability matrix, and the C7
conflict-register row's resolution, which `docs/planning/ADR-001-AMENDMENTS.md`'s
conflict table already pointed at; no pin moved, no gate verdict changed; C7: Zenoh
transport mapping and containment boundary decision landed, see
`docs/planning/decisions/C7-zenoh-transport.md`; C6: provider-facing
trust rendering/outbound symmetry decision landed, see
`docs/planning/decisions/C6-trust-rendering.md`; C5: envelope
authenticity/replay/pairing/authorization decision landed, see
`docs/planning/decisions/C5-envelope-auth.md`; C4: session
identity/addressing/discovery/key storage decision landed, see
`docs/planning/decisions/C4-session-identity.md`; C3: spec
packaging/MCP extension identifier decision landed, see
`docs/planning/decisions/C3-spec-packaging.md`; C2: process model/local IPC/CLI
surface/config model decision landed, see
`docs/planning/decisions/C2-process-model.md`; C1:
language/runtime/packaging/dependency-inventory decision landed, see
`docs/planning/decisions/C1-language-runtime.md`; Rust MCP SDK (`rmcp`) pin added; B4:
gate re-run policy and evidence store; B3: conflict register resolved, ADR-001
amendments A1-A3 issued)

## Current stage

| | |
|---|---|
| Milestone | M0 — Planning package v0.1 |
| Stage | Pre-Stage 0. The §9 planning package is not yet written. |
| Open epics | A (planning package — A2, A4, A5, A6, A7, A8, A9, A10, A11, A12 landed), C (decisions), J (agent skills) |
| Blocked | Stages 1-6. No substantial core or transport code starts before Stage 0 and Stage 1 complete. |

## ADR amendments

ADR amendments: A1-A3 issued, see `docs/planning/ADR-001-AMENDMENTS.md`
(authoritative for verbatim old/new text), assembled and cited (not copied) in
`docs/planning/v0.1/03-decisions-and-amendments.md`. Resolves
conflict register entries C1-C3 directly (`RESOLVED-HERE`); C5, C7 assigned or
resolved-by-evidence per that file's conflict register table; new entries C11-C12 added,
both open (see below). C8 is closed separately, by `docs/planning/decisions/
C4-session-identity.md` §8 (issue #17), with status `RESOLVED-IN-DECISION` — no A-
amendment, because that document found no `ADR-001.md` text needing correction. C4 and
C6 are likewise closed separately, by `docs/planning/decisions/C5-envelope-auth.md` §6
and §9 (issue #18), also `RESOLVED-IN-DECISION` — no A-amendment, for the same reason
(`ADR-001.md` carries neither the `implementation-defined` signature text nor a claim
about Claude acknowledgements). C9 and C10 are likewise closed separately, by
`docs/planning/decisions/C6-trust-rendering.md` §10 and §7 (issue #19), also
`RESOLVED-IN-DECISION` — no A-amendment, for the same reason (`ADR-001.md` carries
neither Codex-correlation text nor a `claude/channel/permission`-default claim). See
`ADR-001-AMENDMENTS.md`'s conflict-register legend for how `RESOLVED-IN-DECISION`
differs from `RESOLVED-HERE`. `docs/planning/ADR-001.md` carries a one-line pointer to
the amendments file; its body text is unchanged.

## Gate verdicts

No gate has been run. Every verdict below is `NOT RUN`, and every task labelled `gate:*`
is blocked until the corresponding spike in Epic D executes.

| Gate | Verdict | Decides | Pins relied on | Result file |
|---|---|---|---|---|
| G1 Claude wake | NOT RUN | Claude adapter viability. go/no-go, no fallback. | Claude Code (Channels); MCP — current era; MCP — legacy era; Rust MCP SDK (rmcp) | `docs/planning/gates/G1-result.md` |
| G2 Codex live inject | NOT RUN | Codex adapter viability. Fallback: OAC-owned app-server with `codex --remote`. | Codex CLI / app-server | `docs/planning/gates/G2-result.md` |
| G3 Zenoh local peer | NOT RUN | Loopback peer discovery on Windows, macOS, Linux. Fallback: fixed local endpoint, no scouting. | Zenoh; Rust toolchain | `docs/planning/gates/G3-result.md` |
| G4 MCP dual-era server | NOT RUN | One process serving both MCP eras. Fallback: two entry points, one core. | Claude Code (Channels); MCP — current era; MCP — legacy era; Rust MCP SDK (rmcp) | `docs/planning/gates/G4-result.md` |
| G5 Provenance | NOT RUN | Machine-set provenance contradicts a spoofing claim on both providers. | Codex CLI / app-server; Claude Code (Channels) | `docs/planning/gates/G5-result.md` |

Re-run/invalidation policy (what moves a verdict back to `NOT RUN`, and the pin-move
checklist): `docs/planning/gates/README.md`.

## Pins

Confirmed. Detailed record, sources, and constraint floors: `docs/planning/PINS.md`.

| Surface | Pinned version | Source |
|---|---|---|
| Claude Code | `v2.1.274` (Channels research preview; permission relay `>= v2.1.234` satisfied) | PINS.md — Claude Code Channels |
| MCP | current `2026-07-28`; legacy `2025-11-25` | PINS.md — MCP revisions |
| Codex CLI | `@openai/codex@0.154.0`, commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` (2026-09-09) | PINS.md — Codex CLI and app-server |
| Zenoh | `1.10.1` (2026-09-07); `>= 1.10.0` required for loopback discovery | PINS.md — Zenoh |
| ACP | protocol version `1` (schema v2 alpha); not a v0.1 dependency | PINS.md — ACP |
| Rust toolchain | `1.98.1` (2026-09-03); `rust-toolchain.toml` enforces it | PINS.md — Rust toolchain |
| Rust MCP SDK | `rmcp` `3.4.0` (2026-09-15); legacy revision `2025-11-25` supported and is the SDK's default | PINS.md — Rust MCP SDK (`rmcp`) |
| `ed25519-dalek` | `3.0.0`; envelope signature algorithm; BSD-3-Clause (flagged, not the usual `MIT OR Apache-2.0` shape) | PINS.md — `ed25519-dalek` |
| `serde_jcs` | `0.2.0`; RFC 8785 JCS canonicalization; MIT OR Apache-2.0 | PINS.md — `serde_jcs` |

## Decisions landed

- **C1 — language, runtime, packaging, dependency inventory** (issue #13): decided.
  Rust, single self-contained binary (dynamically linked against OS system libraries
  only — not bit-for-bit static, see the file's §9). Full decision, evidence, and
  dependency inventory: `docs/planning/decisions/C1-language-runtime.md`, which remains authoritative;
  cited (not copied) in `docs/planning/v0.1/03-decisions-and-amendments.md` decision 1
  and decision 12 (Epic A task A4, landed) — this file is not a redirect stub.
- **C2 — process model, local IPC, CLI surface, config model** (issue #14): decided.
  One long-lived per-device `oac` daemon (Zenoh peer, device identity/keys, policy,
  Codex app-server client) plus thin `oac mcp-shim` stdio child processes; Windows named
  pipe / Unix `AF_UNIX` socket IPC with OS-level peer authentication (candidate crate
  `interprocess` `2.4.4` pinned; final IPC crate a Stage 3 detail, see C2 §4); zero-file
  local default. Full decision and evidence:
  `docs/planning/decisions/C2-process-model.md`, which remains authoritative; cited
  (not copied) in `docs/planning/v0.1/03-decisions-and-amendments.md` decision 2 and
  decision 11 (Epic A task A4, landed) — this file is not a redirect stub.
- **C3 — spec packaging and the MCP extension identifier** (issue #16): decided.
  Standalone normative document (`OAC Session Channels`, under `spec/`, normative-of-
  record) plus MCP extension identifier `io.github.rossgraeber/oac-session-channels`
  for capability negotiation, tool surface, and `_meta` provenance only; MCP is not the
  delivery mechanism. Full decision and evidence:
  `docs/planning/decisions/C3-spec-packaging.md`, which remains authoritative; cited
  (not copied) in `docs/planning/v0.1/03-decisions-and-amendments.md` decision 3
  (Epic A task A4, landed) — this file is not a redirect stub.
- **C4 — session identity, addressing, discovery, key storage** (issue #17): decided.
  Four-layer identity model (device key, opaque stable session id, display-only URI,
  human alias — only the device key and opaque id carry authority); Claude `session_id`
  and Codex `thread.id` capture paths stated per harness; Claude resume treated as a new
  OAC session id unless a registration is observed, Codex resume MAY re-bind the same
  id through the daemon's own authoritative client; display URI form
  `session://<device>/<harness>/<id>` resolves conflict C8 (`RESOLVED-IN-DECISION`); key
  storage `keyring` `4.2.0` (per-platform backends) plus `age` `0.12.1` encrypted-file
  fallback. Full decision and evidence:
  `docs/planning/decisions/C4-session-identity.md`, which remains authoritative; cited
  (not copied) in `docs/planning/v0.1/03-decisions-and-amendments.md` decisions 4 and 7
  (Epic A task A4, landed) — this file is not a redirect stub.
- **C5 — envelope authenticity, replay defence, pairing, authorization** (issue #18):
  decided. Ed25519 via `ed25519-dalek` `3.0.0` (`verify_strict`, mandatory) over a
  domain-separated RFC 8785 JCS canonicalization (`serde_jcs` `0.2.0`) of the signed
  field set (every envelope field except `security.signature` itself); signature made
  normative, resolving conflict C4 (`RESOLVED-IN-DECISION`); one Ed25519 keypair per
  device (no v0.1 automatic rotation, manual re-pair); ±300s replay accept-window plus a
  128-bit CSPRNG nonce deduplicated on `(key_id, nonce)` against an in-memory,
  per-device, daemon-held duplicate-suppression store; honest `DeliveryReceipt` states
  (`accepted-by-adapter`, `handed-to-harness`, `unknown`), resolving conflict C6
  (`RESOLVED-IN-DECISION`); pairing zero-config for same-device multi-harness, a 6-digit/
  120-second/5-attempt short code for two devices on a LAN; default-deny, per-OAC-
  session-id, `working_directory`-scoped sender allowlists; OAC policy maps only onto
  authenticated Zenoh ACL subjects (certificate common name or username), never `zid`.
  Full decision and evidence: `docs/planning/decisions/C5-envelope-auth.md`, which
  remains authoritative; cited (not copied) in `docs/planning/v0.1/
  03-decisions-and-amendments.md` decisions 5 and 6 (Epic A task A4, landed) — this
  file is not a redirect stub.
- **C6 — provider-facing trust rendering, outbound symmetry** (issue #19): decided.
  Claude inbound provenance is a fixed five-key `meta` set (`oac_sender`, `oac_device`,
  `oac_session`, `oac_message_id`, `oac_reply_to`), identifier-safe by a const key table
  plus a contract test and a refusal fixture (a dropped key refuses delivery rather than
  shipping unlabelled); Codex inbound provenance rides a machine-generated header plus a
  `security.nonce`-derived, per-message-unguessable delimiter inside the
  `{type:"text",text}` item, never `turn/steer`; permission relay
  (`claude/channel/permission`) stays off by default in v0.1, resolving conflict C10
  (`RESOLVED-IN-DECISION`); outbound is symmetric through four OAC MCP tools (`send`,
  `reply`, `list_sessions`, `whoami`), reachable for Codex via `codex mcp add`; Codex
  reply correlation is a layered rule — explicit `in_reply_to`, an adapter-independent
  thread-id/turn-id binding, and an explicit inferred/uncorrelated downgrade rather than
  silent attribution — resolving conflict C9 (`RESOLVED-IN-DECISION`). Full decision and
  evidence: `docs/planning/decisions/C6-trust-rendering.md`, which remains
  authoritative; cited (not copied) in `docs/planning/v0.1/03-decisions-and-amendments.md`
  decisions 8 and 9 (Epic A task A4, landed) — this file is not a redirect stub.
- **C7 — Zenoh transport mapping and containment boundary** (issue #20): decided. Key
  expressions are a one-way hash of the opaque session id, computed only inside
  `transports/zenoh/`, never guessable and never on the wire in reverse; presence maps
  to liveliness tokens plus history-capable liveliness subscribers (stable API, no
  polling); local mode binds `127.0.0.1`, runs no `zenohd`, leaves multicast scouting on
  by default (pinned `1.10.1` satisfies the `>= 1.10.0` loopback-discovery floor), with
  the G3 fixed-rendezvous-endpoint fallback as the named reversal path; LAN mode defaults
  to TLS (QUIC named as the alternative) with C5 §10(b)'s pairing-issued certificates;
  ACL subjects are certificate common name or username only, never `zid` (restated from
  C5 §12, which already fixed the policy-to-subject rule and per-key-expression scoping —
  not a deferral C7 closes); C7 adds the local-vs-LAN profile split and
  certificate-issuance wiring; also adds a local-mode TLS listener (auto-generated
  certificate, no manual management) to satisfy G3's pass criterion. Only the
  stable `zenoh`/`zenoh-ext` surface is used, no `unstable` feature. Full decision and
  evidence: `docs/planning/decisions/C7-zenoh-transport.md`, which remains
  authoritative; cited (not copied) in `docs/planning/v0.1/03-decisions-and-amendments.md`
  decision 10 (Epic A task A4, landed) — this file is not a redirect stub.

## Open conflicts (oac-evidence §6)

- **PLANNING-PROMPT.md §9 item 3 vs. the per-gate evidence store (issue #33).** §9
  item 3 states G1-G5 land in one file, `docs/planning/v0.1/02-gating-findings.md`.
  This task's evidence-store design instead makes `docs/planning/gates/G<n>-result.md`
  (one file per gate) the record, with `02-gating-findings.md` generated from those
  five files rather than hand-authored. Not an ADR-001 claim, so no ADR amendment is
  proposed. Full reconciliation: `docs/planning/gates/README.md` §"Reconciliation with
  PLANNING-PROMPT.md §9 item 3". `docs/planning/v0.1/03-decisions-and-amendments.md`
  now exists (Epic A task A4, landed); this entry stays recorded here rather than moving,
  since it is not an ADR-001-decision conflict that file's §5/§ Amendments/§ Conflict
  register sections scope to — it is a PLANNING-PROMPT.md-vs-evidence-store shape note.
- **C7 local-mode TLS, caught and resolved in-document (issue #20).** An earlier C7 draft
  said local mode has no TLS/QUIC listener, citing DESIGN.md line 115's "no manual
  certificate management." That conflicted with `PLANNING-PROMPT.md` §4 G3's pass
  criterion, which requires "a TLS listener bound to localhost" even in local mode.
  Resolved in `docs/planning/decisions/C7-zenoh-transport.md` §5: local mode now also
  binds a TLS listener using an auto-generated, unmanaged local certificate — satisfies
  both sources; no ADR-001 amendment needed. Restated in `docs/planning/v0.1/
  03-decisions-and-amendments.md` decision 10 (Epic A task A4, landed), citing C7 §5;
  full resolution stays in C7, not copied here or there.
- **C7 presence/discovery gaps, open (issue #20).** `docs/planning/decisions/
  C7-zenoh-transport.md` §4 records two unresolved gaps: (1) the liveliness-token
  presence mapping carries only reachability, not the full `PresenceRecord` (harness
  ownership, capabilities, active-inbound support) DESIGN.md lines 104-105 ask for; (2) no
  discovery path is defined by which a peer learns an *unknown* session's opaque id, even
  though ADR-001.md line 61 puts presence/discovery in v0.1 scope. Neither is fixed by
  this document; a future decision must close them. Restated (not copied) in
  `docs/planning/v0.1/03-decisions-and-amendments.md` decision 10 (Epic A task A4,
  landed), citing C7 §4.

## Open conflict-register items

Verified, directly observable open work items from the conflict register — not
UNVERIFIED claims (per `oac-evidence` §5, that list is for claims no first-party source
states or that are inferred/stale). Closed when the named resolution lands.

- C11: the named compatibility shim boundary for the Claude Code Channels
  research-preview surface and the Codex experimental live-inject surface is UNNAMED
  (register entry restating the two shim-boundary rows in "Open UNVERIFIED items"
  above; see `ADR-001-AMENDMENTS.md` "New register entries"). Owner: a C-series decision
  or a DESIGN.md update. **Narrowed, not closed, by C4** (issue #17): harness-native-id
  capture is fixed as daemon-owned-only, reached only via the hook/JSON-RPC surfaces
  named in `docs/planning/decisions/C4-session-identity.md` §3/§4, over C2's local IPC —
  the module name/path itself is still unnamed and stays owned by the Epic F/G adapter
  implementation tasks (see C4 §16).
- C12: `DESIGN.md` still carries the retired names (`sessionchannels`, "Session
  Channels", "MCP Session Channels extension") after ADR-001-A1 (exact sites listed in
  `ADR-001-AMENDMENTS.md` "Carried to later tasks"). Owner: Epic A task A9 plus a
  DESIGN.md follow-up edit.

## Open UNVERIFIED items

Carried from PLANNING-PROMPT.md §3, re-verified against the B1 pins in B2
(`docs/planning/REVERIFICATION-B2.md`). Until closed, no plan or skill may rely on them
without an UNVERIFIED label.

- Claude channel behaviour across `--resume`/`--continue` (UNVERIFIED — docs silent at
  v2.1.274; see REVERIFICATION-B2.md §3.1 box 1).
- Whether one MCP server can present more than one logical channel (UNVERIFIED — docs
  silent at v2.1.274; see REVERIFICATION-B2.md §3.1 box 2).
- Whether implicit Codex daemon attach executes by default at runtime in the pinned
  release `0.154.0` (commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`) (UNVERIFIED for
  runtime behaviour — the code path is now source-confirmed present at this commit, see
  REVERIFICATION-B2.md §3.2 box 4; runtime verdict is G2 go/no-go, owned by task D2, not
  by B2).
- Whether Codex Desktop exposes the control socket in current builds (UNVERIFIED — no
  first-party statement found; see REVERIFICATION-B2.md §3.2 box 5).
- Zenoh `auth.pubkey` semantics (UNVERIFIED — see REVERIFICATION-B2.md §3.4 box 6; the
  six key names themselves are now CLOSED, confirmed verbatim in `DEFAULT_CONFIG.json5`
  at tag 1.10.1).
- The 5-15 MB Zenoh binary size estimate (UNVERIFIED — derived estimate, resolved by the
  first G3 build artifact, task D3; see REVERIFICATION-B2.md §3.4 box 7).
- The named compatibility shim boundary for the Claude Code Channels preview surface
  (UNVERIFIED — DESIGN.md names no such module; out of scope for B2, needs a C-series
  decision or a DESIGN.md update; see REVERIFICATION-B2.md "Carried to 11-risks.md").
- The named compatibility shim boundary for the Codex experimental live-inject surface
  (UNVERIFIED — same reason; see REVERIFICATION-B2.md "Carried to 11-risks.md").
- ACP schema v2 "alpha" status (UNVERIFIED — carried from PLANNING-PROMPT.md §3.5 only,
  not independently re-confirmed on agentclientprotocol.com in B1 or B2; low priority,
  ACP is not a v0.1 dependency).
- Zenoh crate version/date read from GitHub releases rather than crates.io directly,
  because the crates.io page did not return content in B1 and was not re-attempted in B2
  (UNVERIFIED — re-confirm on crates.io when reachable; see PINS.md).
- `codex mcp-server` deprecation date (2026-08-20) and deletion date (2026-09-05)
  (UNVERIFIED — carried unchanged from PLANNING-PROMPT.md §3.2, not independently
  re-confirmed against the CLI reference in B1 or B2; see REVERIFICATION-B2.md §3.2
  table).
- No SEP or working-group item for agent-to-agent messaging (UNVERIFIED — carried
  unchanged from PLANNING-PROMPT.md §3.3, not independently re-searched against the SEP
  index in B1 or B2; see REVERIFICATION-B2.md §3.3 table and "Carried to 11-risks.md"
  item 12).
- "Research preview on Claude Code v2.1.232+" floor (UNVERIFIED — not confirmable on
  `channels.md` at `v2.1.274`; `2.1.232` does not appear in its fetched text; see
  REVERIFICATION-B2.md §3.1 box 7 and PINS.md floor 1).
- Whether MCP `experimental` capabilities still exist at the current era `2026-07-28`
  (UNVERIFIED — re-labelled from HOLDS in B2; the prior inference cited Claude Code's own
  client capability, not the `2026-07-28` schema itself, and Claude Code does not
  register a channel server negotiating `2026-07-28`; see REVERIFICATION-B2.md §3.3
  table and "Carried to 11-risks.md" item 13).

- Zenoh's default TLS stack being `rustls` rather than OpenSSL (UNVERIFIED — carried
  from PLANNING-PROMPT.md §3.4 unchanged; not independently re-fetched from Zenoh's own
  `Cargo.toml`/feature docs; see `docs/planning/decisions/C1-language-runtime.md` §9).
- Whether an `rmcp`-based OAC server, run end-to-end against a live Claude Code
  instance with `MCP_PROTOCOL_NEGOTIATION=legacy`, actually registers as a channel
  (UNVERIFIED — SDK capability verified, runtime behaviour is gate G4's job, verdict
  `NOT RUN`; see `docs/planning/decisions/C1-language-runtime.md` §5, §13).
- Whether Codex reliably reproduces a header-supplied id (`oac_message_id`, per
  `docs/planning/decisions/C6-trust-rendering.md` §5) in a subsequent `reply` tool call's
  `in_reply_to` argument (UNVERIFIED — a model-behaviour question, resolved only by a
  spike exercising real Codex turns against the framing, not by documentation; see
  `docs/planning/decisions/C6-trust-rendering.md` §10, §15).
- Whether the Windows `windows-native-keyring-store` `keyring` backend has been
  exercised end-to-end against live Windows Credential Manager (UNVERIFIED — declared
  feature/build target verified only; runtime confirmation belongs to a future
  `oac-implementation`/`oac-testing` task; see
  `docs/planning/decisions/C1-language-runtime.md` §8, §12, §13).
- Whether `oac mcp-shim`, spawned by Claude Code as a child stdio process, inherits an
  environment sufficient to locate the daemon's IPC path without extra configuration
  (UNVERIFIED — depends on Claude Code's channel-spawn environment passthrough, not
  established by any cited source; see
  `docs/planning/decisions/C2-process-model.md` §10, §11).
- Whether the Codex daemon's implicit attach is enabled by default in released
  `0.154.0`, plus a possible documentation-drift signal: a 2026-09-17 re-fetch of
  `https://learn.chatgpt.com/docs/app-server` did not surface the `codex app-server
  daemon start` command or the `app-server-control.sock` control-socket path that
  PLANNING-PROMPT.md §3.2's pre-verified baseline states (UNVERIFIED — already an open
  item per gate G2/task D2; this is one added data point, not a resolution; see
  `docs/planning/decisions/C2-process-model.md` §2, §10).
- Named-pipe DACL peer-authentication behaviour not yet exercised on a live Windows
  host (UNVERIFIED — API shape verified against Microsoft Learn only; see
  `docs/planning/decisions/C2-process-model.md` §4, §10, §11).
- Whether `interprocess` `2.4.4` (or an alternative IPC crate) exposes a first-party
  peer-credential accessor (UNVERIFIED — not surfaced in the fetched crate docs; the
  daemon is expected to call the raw OS API directly instead; see
  `docs/planning/decisions/C2-process-model.md` §4, §10).
- Whether Codex's `thread` object's `sessionId` field always equals `thread.id`, or
  denotes something distinct in some other case (UNVERIFIED — only one worked example
  observed at https://learn.chatgpt.com/docs/app-server, retrieved 2026-09-17; not
  relied on by any C4 decision; see `docs/planning/decisions/C4-session-identity.md`
  §4).
- Whether `ed25519-dalek` `3.0.0` builds and links cleanly on the
  `x86_64-pc-windows-msvc` target (UNVERIFIED — no live Windows build run against this
  pin; the crate is pure-Rust with no documented C/assembly dependency, which is
  favorable but not a substitute for an actual build; see
  `docs/planning/decisions/C5-envelope-auth.md` §15, §16).
- `dalek-cryptography/curve25519-dalek`'s repository-level MSRV *policy* (UNVERIFIED —
  not stated on the repository overview page as fetched; the crate-level `rust-version`
  field this decision actually relies on is confirmed; see
  `docs/planning/decisions/C5-envelope-auth.md` §2, §16).
- The exact byte-truncation length for the device-key-fingerprint hash used in LAN
  pairing and Zenoh certificate common names, above the 128-bit minimum floor C5 §10(b)
  fixes (UNVERIFIED — deliberately left as a Stage 3 implementation detail above that
  floor; see `docs/planning/decisions/C5-envelope-auth.md` §10, §12, §16).
- Whether the 6-digit/120-second/5-attempt LAN pairing-code parameters hold up against a
  live implementation's actual network conditions (UNVERIFIED — these are OAC's own
  design parameters, not a claim about an external system; runtime validation is a Stage
  3/4 task; see `docs/planning/decisions/C5-envelope-auth.md` §10, §16).

- NATS reliability, persistence, offline queueing, ordering, multicast discovery, and
  routing/federation capability claims, for the `05-interfaces.md` transport
  design-for-replacement proof (task A6, issue #26) (UNVERIFIED — not independently
  checked against first-party NATS specification/documentation this pass; see
  `docs/planning/v0.1/05-interfaces.md` §17).
- MQTT reliability, persistence, offline queueing, ordering, multicast discovery, and
  routing/federation capability claims, for the same proof (UNVERIFIED — not
  independently checked against first-party MQTT specification/broker documentation
  this pass; the multicast-discovery cell additionally carries a structural, unverified
  observation about MQTT's broker-based client model; see
  `docs/planning/v0.1/05-interfaces.md` §17).

**Closed in B2** (removed from this list; see REVERIFICATION-B2.md "Closed UNVERIFIED
items" for citations): Agent SDK does not support Channels (confirmed absent from the
Agent SDK's own capability table); SEP-2133's finalization date `2026-01-26` (confirmed
via its PR's `merged_at`); no documented `CLAUDE_SESSION_ID` environment variable
(confirmed absent from `hooks.md`, `session_id` is the supported path); Codex issue
#21743 status (confirmed still open, no drift to the "no attach" premise). The floor-1
`>= v2.1.232` sentence was re-checked and remains UNVERIFIED, not closed — still not
present on `channels.md` verbatim — see REVERIFICATION-B2.md §3.1 box 7; it stays in the
"Open UNVERIFIED items" list above. SEP-2133's reserved-prefix rule for extension
prefixes whose second label is `modelcontextprotocol` or `mcp` is **drift, not
UNVERIFIED** — no such clause exists in the SEP text; see REVERIFICATION-B2.md Drift
register D2.
