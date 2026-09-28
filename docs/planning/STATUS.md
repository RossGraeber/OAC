# OAC status

The single source of truth for where the project is. The `oac` router skill reads this file
rather than restating it. Update it when a stage opens or closes, when a gate returns a
verdict, or when a pin moves.

**Last updated:** 2026-09-28 (**K1/issue #124 (Epic K #123): herdr evaluation, desk leg
landed — provisional go, pending live confirmation; live leg NOT RUN.** The new file
`docs/planning/decisions/K1-herdr-evaluation.md` pins herdr at `v0.9.1`, fixed (tag
`v0.9.1` → commit `065ef9d6a531c49fb8bee7e818ef837065b21ee9`, GitHub "Latest",
retrieved 2026-09-28). It records the Apache-2.0 license and herdr's own per-OS
documentation claims, and gives the desk status of every K1 acceptance item. Two desk
findings: `agent read` and `agent send-keys` have no `--timeout` option at `v0.9.1`,
and no first-party source documents Codex settling to `unknown` after a response. The
live leg was not run: the cloud session that wrote the record had no herdr binary and no
logged-in harness. The operator's live checklist is in that file's §6. `docs/planning/PINS.md` gains row
`herdr (test tooling)` (`Gates affected: none`, so no gate verdict is invalidated).
`docs/planning/v0.1/07-repository-and-dependencies.md` §5 gains a "Dev/test tooling —
not shipped" heading. New UNVERIFIED entry below. `docs/planning/v0.1/11-risks.md` gains
`RISK-HERDR` and row 51. No gate verdict, skill, or ADR text changes.)

**Last updated:** 2026-09-28 (**G1/issue #34/D1: PASS**, operator decision, issue #39
chat. G1's `v2.1.282` PASS was invalidated 2026-09-27 by the Claude Code (Channels) pin
float; re-run in two attempts the same day, issue #39 T6/T7: Box B recorded incomplete,
Box C closed its gaps and is the verdict-bearing run. Judged directly against
criterion 3's literal text ("queued and delivered at the next turn, in order (not
dropped, not interleaved out of order)"): order preserved, nothing dropped, nothing
interleaved — **PASS**. Box C's own new finding — two mid-turn notifications landed at
two separate tool-call boundaries rather than batched together at one, unlike the
original PASS's own account — is recorded as an **observed `2.1.283` behavior change**,
not a criterion failure. Every "delivered together" statement this project makes has
been amended with a dated note, not silently rewritten: `PLANNING-PROMPT.md` §3.1,
`.claude/skills/oac-claude-channels/SKILL.md`, `.claude/skills/oac-gates/
references/G1-claude-wake.md` (including its "two notifications, one turn boundary"
fixture-capture line), `docs/planning/v0.1/04-architecture.md`, and
`docs/planning/REVERIFICATION-B2.md` (a cross-reference note added beside the verbatim
first-party quote, the quote itself untouched). Mid-turn delivery batching (vs.
per-boundary delivery) is now tracked as its own UNVERIFIED-as-a-guarantee item — see
"Open UNVERIFIED items" below and `docs/planning/v0.1/11-risks.md` row 49 (may depend on
notification send timing; only two data points exist so far). **G1's gate-level verdict
in "Gate verdicts" below, the "Open epics"/"Blocked" cells above, and every document
that mirrors G1's STATUS cell are synced to PASS in this same change**
(`02-gating-findings.md`, `03-decisions-and-amendments.md`, `04-architecture.md`,
`06-security.md`, `08-cli-and-deployment.md`, `09-test-strategy.md`, `10-stages.md`,
`11-risks.md`, `01-capability-matrix.md` (no stale cell found there to fix),
`decisions/C6-trust-rendering.md`) — the Stage 1 exit blocker lines naming "G1's pending
re-run" now read only "G3's parked macOS leg" (G3 remains the sole D3/macOS blocker on
Stage 1's exit). The original `v2.1.282` transcript's `MANIFEST.json` `superseded_by`
field now points to the Box C fixture; `pins_reference.claude_code_note` updated to
match. Full evidence: `docs/planning/gates/G1-result.md`. `node
scripts/check-fixture-manifest.mjs`, `node scripts/check-skills.mjs`, and `node
scripts/sync-backlog.mjs --check` all re-run clean after this change.)

**Last updated:** 2026-09-28 (**superseded by the entry above (G1 PASS)** for G1's
verdict specifically — this entry's own G1 narrative below predates the operator's
2026-09-28 PASS decision and is kept as history, not rewritten; everything else in this
entry about D6 fixtures, redaction, and the schema comparison still stands. issue
#39/D6 protocol fixtures, subtasks T5-T7, plus a
post-review correction pass and a second G1 re-run attempt (Box C). **D6's five acceptance
criteria (issue #39 body) are all met by the fixtures this change lands**: Claude fixtures
cover initialize/negotiation, notification delivery, mid-turn queueing, and a tool reply
(`d6-claude-protocol/`); Codex fixtures cover `initialize`/`initialized`, `thread/start`,
`thread/resume`, `turn/start`, `thread/queue/add`, and the event stream including
`item/completed`/`turn/completed` (`d6-codex-protocol/`); every fixture records its pinned
version and capture date (`MANIFEST.json`); fixtures contain no credentials/tokens/private
paths (redaction results below); the Codex schema is referenced (commit, path, regen
commands, hashes, upstream comparison), not hand-transcribed. D6 itself does not depend on
G1's own gate verdict being resolved (a separate concern — see below).

**Codex (T5):** two boxed attempts. Attempt 1 (2026-09-27T22:54:20Z-22:57:21Z) correctly hit
a real protocol-ordering failure — `thread/resume` on a freshly-`thread/start`-ed thread
before any turn ran on it failed `-32600 "no rollout found"` twice — root-caused to
first-party source (`codex-rs/rollout/src/recorder.rs` `deferred_creation`; commit
`36650394c5b38c2990ccf2a3457165ca3e9d9726`): a rollout materializes lazily, on a thread's
first user message, not at `thread/start`. Attempt 2 (retry, 2026-09-27T23:59:41Z-
2026-09-28T00:00:55Z) sequenced correctly and captured `thread/start`, `thread/resume`, two
`turn/start` calls, and a `thread/queue/add` genuinely queued while busy (response timestamp
precedes the second `turn/completed`), all against Codex CLI / app-server `0.157.1` —
closing G2-result.md's "`thread/start` was not captured" gap (the two fixtures now
*complement*, not supersede, each other — see G2-result.md). **225/225** validated protocol
frames pass across all eight raw transcripts (0 fail; corrected from an initial 100/106 after
fixing a `validate-frames.mjs` schema-name-mapping bug — see `d6-codex-protocol/README.md`
"Redaction"); redacted with the `redact.mjs` that actually ran (sha256
`975de80c6462bdf1bbc21ee05e37730cde5cdf7ef74e42a556270a46ccbd9a3d`); a
**sanitized copy** (sha256 `371d79640a661180e15e3c4afc788531567f1bbbfdf286ac542608aecf381872`
— username/hostname/installation-id literals now read from env/argv instead of
hardcoded, verified byte-identical output when configured correctly) is what's
actually **committed** quarantined at `d6-codex-protocol/redact.mjs.throwaway-quarantined`,
alongside `hash-tree.mjs.throwaway-quarantined` and `validate-frames.mjs.throwaway-quarantined`
(neither of which needed sanitizing),
`residualLeaks=[]`/`residualGenericHits=[]` on every file. Attempt 1's failed-resume
transcripts are committed as clearly-named negative fixtures, not discarded. Schema record
(T2/T7): the local default-tier generation (314 files, tree sha256
`6b5c39357ee0552bfa97c544a6fd16b0e5828eebb721c5313932b773105fca87`) was compared against the
upstream `codex-rs/app-server-protocol/schema/json` tree fetched directly from
`raw.githubusercontent.com` at the exact pinned commit — **314/314 files, byte-identical
match, no schema drift found** for the default tier (the only tier upstream checks in at
this path/commit); the local `--experimental` generation (440 files) is NOT a pure superset
of the default tier — 126 files exist only under `--experimental`, and a further 28 files
exist in *both* tiers with different content (e.g. `TurnSteerParams.json`, which is already
present in the default tier, not experimental-only). Fixtures: `docs/planning/gates/fixtures/
d6-codex-protocol/` (README.md documents all of the above in full).

**Claude (T4/T6):** Box A (04:37:29Z-04:44:36Z, after discarding a ~60s pre-box relaunch)
captured server/discover (protocolVersion `2026-07-28`), legacy initialize, idle wake, a
reply tool call, and a mid-turn pair, on Claude Code `v2.1.283`; redacted clean; fixtures at
`docs/planning/gates/fixtures/d6-claude-protocol/`.

**G1 re-run — TWO attempts, both incomplete or under operator review; verdict stays NOT
RUN.** Box B (04:44:55Z-04:56:46Z, Claude Code v2.1.283) is recorded as an **incomplete**
re-run attempt: criteria 1, 2, and 4 are evidenced, but criterion 2 only re-confirmed the
wake itself (not the `<channel>` attribute set or the dropped-key behavior), criterion 3
sent only one mid-turn notification (the original PASS used two, to test ordering), and
criterion 5's dialog text was not captured before the operator accepted it — gaps in the
run sheet the operator identified afterward, not a time-limit problem (the box closed with
~33 minutes still available). The operator ordered a fresh re-run, **Box C**
(05:33:29Z-05:44:27Z, same environment), which closed those specific gaps: the dev-channels
dialog text was captured verbatim *before* accepting (criterion 5); the `<channel>` attribute
query was repeated and got the same three-attribute answer as the original PASS plus
confirmation the non-identifier-safe key is dropped (criterion 2, now fully re-confirmed);
and two mid-turn notifications were sent during a four-`Start-Sleep` busy turn (criterion 3).
**Box C's own new finding, not yet resolved:** the two mid-turn notifications were delivered
in order, not dropped, and never interleaved inside a tool call — but at **two separate**
tool-call boundaries (one after each of the first two `Start-Sleep` calls), not "together" at
a single boundary the way the original `v2.1.282` PASS recorded it ("two notifications sent
mid-turn arrived together between tool calls") and the way `G1-claude-wake.md`'s own fixture
note and `oac-claude-channels`' §3.1 fact both describe it ("delivered together, in order";
"two notifications, one turn boundary"). Full per-criterion write-up for both boxes:
`docs/planning/gates/G1-result.md`. **Per `oac-gates`' "no partial pass" rule, this file does
not pick a verdict** — the open question (does one-notification-per-boundary delivery still
satisfy criterion 3's "queued and delivered at the next turn, in order," and does the §3.1
fact / skill text describing "delivered together" need amending as a genuine 2.1.283
behavior change, or was the original PASS's "together" merely an artifact of how *that* spike
happened to time its two sends) is recorded for the operator to decide. **G1's gate-level
verdict below, the pins table, and every other G1-mirroring document
(`02-gating-findings.md`, `03-decisions-and-amendments.md`, `04-architecture.md`,
`06-security.md`, `08-cli-and-deployment.md`, `09-test-strategy.md`, `10-stages.md`,
`11-risks.md`, `01-capability-matrix.md`, `decisions/C6-trust-rendering.md`, the Stage 1
exit blocker lines) are left unchanged by this entry — they still read "G1 NOT RUN for the
current environment, re-run pending."**

Separately, new evidence (not a G1 criterion): a `server/discover` probe (Claude Code
`2.1.283`) reproduced across Box A, Box B, and Box C, including with `$env:MCP_SDK_GENERATION`
confirmed empty in the Box B/C terminal — see "Open UNVERIFIED items" below and
`docs/planning/v0.1/11-risks.md` row 42 (also corrects an earlier draft of this note that
wrongly claimed all occurrences shared one working directory — they do not).

`docs/planning/gates/fixtures/MANIFEST.json` now carries 149 entries (up from the pre-D6
131): 8 attempt-1/attempt-2 Codex transcripts, 1 Claude Box A transcript, 1 G1 Box B
transcript, 1 G1 Box C transcript, 2 quarantined Claude/Codex channel-server-adjacent
scripts, 3 newly-committed quarantined tooling scripts (`redact.mjs`, `hash-tree.mjs`,
`validate-frames.mjs`), and 1 README — plus corrections to several pre-existing entries'
stale notes, capture-time ranges verified directly against each raw file's own timestamps
(not approximated), the `schema` block's shape redesigned to separately record the upstream
comparison and the local default/experimental generations (and to correctly show a `null`
schema hash, with a note, for the one fixture whose observed Codex version, `0.154.0`, was
never re-schema'd), and three review-nit fixes (G2 baseline `version_matches_pin_note` now
reads "per PINS.md," the g1 quarantined-script entry's stale second note shortened and
corrected, and the truncated `2f916d9` commit references resolved to the full 40-character
SHA `f5adee2ad6595a1e650feda89487ae92e0659d4f` — the commit that actually last changed
PINS.md, distinct from PR #120's own merge commit `2f916d9a0e8e4ccb088e9c9937ab63b611affd3b`).
`node scripts/check-fixture-manifest.mjs` and `node scripts/check-skills.mjs` both pass
clean. Issue #39: T0-T2 already landed (prior PRs); T5-T7 land in this change; T3/T4 landed
as prerequisites of T5-T7 in this same change (the redaction script and Claude channel
server used above).)

**Last updated:** 2026-09-27 (Claude Code pin changed to **floating** by operator decision,
issue #39/T0, mirroring the Codex row. The connecting client reported `v2.1.282` during G1
(2026-09-25) and `v2.1.283` during both the G4 re-run and the G5 spike (2026-09-26 and
2026-09-27). Rather than keep treating each patch bump as a one-off drift note, the
PINS.md Claude Code (Channels) row now records the last observed version (`v2.1.283`,
GitHub release tag `v2.1.283`, published 2026-09-25T21:50:12Z UTC) under a written
floating-version policy. Per the pin-move checklist, **G1 is invalidated**: it PASSED on
`v2.1.282` and is `NOT RUN` for the current environment, since `v2.1.282` does not equal
the new last-observed `v2.1.283`. G4 and G5 both already ran on `v2.1.283` — the version
this move records as last-observed — so their verdicts stay current; this move does not
advance the last-observed version past what they already ran on. **Environment check at
this move:** `claude --version` on this host reported `2.1.283 (Claude Code)` on
2026-09-27, matching the newly recorded last-observed version. G1 will be re-run in the
same HIL sitting as the D6 Claude capture (issue #39 T6), under its own declared box. No
new UNVERIFIED item is added — the Claude §3.1 re-verification gap this pin drift implies
was already open (see "Open UNVERIFIED items" below, "New, from G1").)

**Last updated:** 2026-09-27 (**G5/issue #38/D5: FAIL** (Codex criteria 2/3 f; Claude all
criteria x). A 60-minute timebox (declared 2026-09-27T06:08:53Z, closed 06:38:46Z,
~29m53s elapsed, not expired) ran six verdict-bearing Claude cases (C1, C2, C3, C4, C4b,
C6) plus one informational case (C5), and five verdict-bearing Codex cases (X1-X5) plus
one exploratory, non-verdict-bearing case (X6), against Claude Code `2.1.283` and Codex
`0.157.1` (both rechecked at box-open, no pin move). **Claude passed every criterion
evaluated for it** (1, 3, 4): every harness-recorded `<channel>` render carried exactly
one `oac_sender` attribute regardless of prose sender claims, fake nested `<channel>`
tags (Claude Code escapes `</channel>` in content as `<\/channel>`), or attribute-value
quote injection (escaped as `&quot;`); a non-identifier-safe `meta` key (hyphen, dot,
space, or non-ASCII) is silently dropped from both the rendered tag and the whole raw
session-log line, and a message whose *only* sender field used an unsafe key rendered
with no sender at all — Claude answered "unknown sender" rather than guessing; a
mid-turn delivery closed G1's open "exact wrapper text" item, captured verbatim inside a
`<system-reminder>` wrapper that itself labels the tag untrusted. **Codex FAILED
criterion 2 (evidence: X2, X5) and criterion 3 (evidence: X2):** a forged nested envelope
using a non-matching guessed delimiter (case X2) got the model to name the forged id as
the sender in part (1) of its own answer, though it declined to pick either id as
authoritative in part (3) — frozen rule (b) scores on part (1)'s naming, so this is f
regardless; a peer-controlled `oac_reply_to` value inserted unmodified into the header
block produced two `oac_sender:` lines the model correctly reported as unresolvable
(case X5, predicted by construction, criterion 2 only). A forged block replaying a real,
already-sent delimiter (case X3) did **not** get the model to name the forged id — it
named neither id and quoted the claim as a claim — so X3 does not fail criteria 2 or 3;
the replay cost the model its ability to resolve a sender, not its ability to reject the
forged one. Three of five Codex cases (X1, X3, X4) passed. An exploratory,
non-verdict-bearing case (X6) showed `turn/start.additionalContext` is a second, unused,
machine-set-metadata carrier the model attributed correctly once asked, after first
acting on the unframed body's claim. **Verdict: FAIL — no fallback exists for G5.** Per
`docs/planning/v0.1/10-stages.md` §5's "Gate S1" acceptance criterion 1, a `FAIL` is a
closed verdict like `PASS`, so this does not by itself block **Stage 1's own exit** (D7);
what it blocks is narrower: per §5's "Go/no-go condition" and §2 ("A `FAIL` on G1 or G5
... stops it"), the pipeline cannot proceed past **Stage 2's interface freeze** for
Codex's provenance framing until conflict-register entry **C13** (`docs/planning/v0.1/
03-decisions-and-amendments.md` §4, `docs/planning/ADR-001-AMENDMENTS.md` "New register
entries") lands and DESIGN acceptance criterion 6 is re-established for Codex — tracked
also at `docs/planning/v0.1/11-risks.md` RISK-G5 (rows 45-46, plus informational rows
47-48). Full result: `docs/planning/gates/G5-result.md`. Fixtures:
`docs/planning/gates/fixtures/g5-provenance/transcript-claude-2026-09-27.jsonl` (39
lines), `claude-rendered-2026-09-27.jsonl` (14 records), `transcript-codex-2026-09-27.jsonl`
(142 lines), all redacted, no residual leaks beyond the intentional public extension
identifier. **Separately, a 20-minute row-41 probe** (declared 2026-09-27T06:42:35Z,
closed 06:42:59Z, after G5's own box closed, never part of G5's verdict) showed Codex
`0.157.1` **can** negotiate MCP `2026-07-28` against a G4-shaped server when launched
with `codex exec --enable mcp_2026_07_28` (every request on the HTTP-registered leg
carried `_meta["io.modelcontextprotocol/protocolVersion"]: "2026-07-28"`), while the same
run's separate stdio (`config.toml`) registration stayed on legacy `2025-11-25` — this
is Codex's opt-in, `stage: UnderDevelopment`, `default_enabled: false` leg working when
explicitly enabled, not the default client behavior. Does not change G4's `PASS`. Closes
the on-the-wire half of `11-risks.md` row 41 for the opt-in path only; conflict-register
row C5 stays `ASSIGNED` (its "Codex tool path may negotiate current MCP" element still
needs the *default* client behavior, or a tested `rmcp`-based server). Full result:
`docs/planning/gates/G4-result.md` "Row-41 probe addendum"; fixture:
`docs/planning/gates/fixtures/g4-mcp-dual-era/transcript-row41-2026-09-27.jsonl` (18
lines; the uncommitted background file `transcript-pre-row41-064223.jsonl`, 83 lines, is
lines 1-57 the unrelated 2026-09-26 G4 PASS transcript, lines 58-73 a new, previously
unrecorded `codex-mcp-client/0.155.0-alpha.16.4` connection at 05:17:04Z (attribution to
Codex Desktop UNVERIFIED, same user-agent as two earlier 2026-09-26 occurrences —
`11-risks.md` row 43, updated), lines 74-75 this probe's own server startup, and lines
76-83 an unrelated pre-timebox OAuth-discovery burst), redacted (nothing to redact — the
server logs only protocol JSON).)

**Last updated:** 2026-09-26 (**G4/issue #37/D4 re-run: PASS.** A fresh, redeclared
60-minute timebox (2026-09-26T20:46:03Z; the operator closed the prior session, its
servers exited, and the previous transcript was archived first) closed with all five
pass criteria confirmed inside the box (live work finished 20:54:30Z, ~8.5 minutes
elapsed, not expired) — superseding the out-of-box `NOT RUN` recorded earlier (timebox
declared 2026-09-25, evidence gathered 2026-09-26; kept below, and in
`docs/planning/gates/G4-result.md`'s "Re-run history," as the prior entry). Evidence,
same shape as the superseded run: `g4spike` (pid 19680) negotiated the legacy stdio
channel (`2025-11-25`) and delivered two pushed notifications Claude actually rendered
(the operator's own UI paste: "← g4spike: G4 wake test from the LEGACY..." and
"← g4spike: relayed from http-legacy: codex relay during modern session"); the same
pid's HTTP surface served Claude's modern (`2026-07-28`) `tools/call` twice — once
before and once **after** two independent Codex `0.157.1` legacy HTTP sessions and a
Codex-triggered relay — both times returning correct OAC `_meta` provenance, which is
the direct no-degradation evidence; and the negative case (`g4modern`, modern-only,
declaring `capabilities.experimental["claude/channel"]`) was refused registration as a
channel — confirmed once, by the operator's paste "Channel messages from \"g4modern\"
are unavailable..." — and its wake-push delivery failed twice (both the first and
second wake pushes from `g4modern` went unobserved in Claude, while the legacy server's
equivalent pushes both arrived). **Verdict: PASS, no fallback needed.** Two new
observations, neither a criterion failure: Claude Code does not surface a tool result's
`_meta` to the model even though it is present on the wire (Claude, asked directly:
"The call to mcp__g4http__g4_echo worked, but no _meta reached me."); and Codex `0.157.1`
again never negotiated the modern MCP era (only `2025-06-18`), so criterion 2's Codex
leg stays open at `11-risks.md` row 41/RISK-G4 — the criterion itself is satisfied via
Claude as the modern client. **Caveat, made prominent, not a footnote:** the G4
reference states Codex's outbound registration "is the supported outbound surface the
current-revision path in this gate must work against," and Codex did reach the server
that way (`codex mcp add g4 --url ...`) but stayed legacy-only throughout; a
`codex --enable mcp_2026_07_28` probe was on the spike's own pending list with ~51
minutes of this run's box still remaining and was not attempted. Fixtures:
`docs/planning/gates/fixtures/g4-mcp-dual-era/transcript-2026-09-26.jsonl` (57 lines,
primary) plus the superseded `transcript-2026-09-26-outofbox.jsonl` (58 lines, kept) and
the unrelated `transcript-run1.jsonl` (10 lines, kept), all redacted. Full detail:
`docs/planning/gates/G4-result.md`.)

**Last updated:** 2026-09-26 (**G4/issue #37/D4, out-of-box run: NOT RUN — timebox
expired, superseded by the re-run above.** A throwaway
dual-era MCP server (`g4-server.mjs`, one Node.js process, quarantined, not committed)
served Claude Code's legacy stdio channel and a dual-era HTTP surface for Codex from a
single pid at once. Every one of the five G4 pass criteria individually confirmed by
direct transcript evidence, none failed: legacy stdio channel registration
(`2025-11-25`) with two delivered `notifications/claude/channel` pushes; Claude's
current-era (`2026-07-28`) HTTP `tools/call` carrying OAC `_meta` provenance
(`io.github.rossgraeber/oac-session-channels`); every modern-era request carrying
`_meta["io.modelcontextprotocol/protocolVersion"]`; the same pid (16712) serving both
eras concurrently for over 11 minutes without either surface degrading, including a
Codex `0.157.1`-triggered relay that reached Claude's live session through the same
process's stdio channel; and the negative case — a modern-only stdio server
(`g4modern`) that declared `capabilities.experimental["claude/channel"]` was
**refused registration as a channel** by Claude, confirmed by the operator's own UI
paste ("Channel messages from \"g4modern\" are unavailable..."). **Despite this, the
verdict is `NOT RUN`, not `PASS`:** the 120-minute timebox declared 2026-09-25T07:20:35Z
closed at 09:20:35Z the same day with zero live-Claude evidence gathered (only a local
self-test, explicitly not gate evidence, plus a real but partial Codex HTTP-legacy
check). All of the confirming evidence above was captured roughly 21-23 hours later, on
2026-09-26, after the box had already expired, with no new timebox declared before that
work resumed — a direct instance of `oac-gates`' "an expired timebox is a result, not a
licence to keep going" rule. Full reasoning: `docs/planning/gates/G4-result.md`,
"Superseded: 2026-09-25/26 out-of-box run (NOT RUN)". Codex `0.157.1`'s opt-in `mcp_2026_07_28` client
mode was again not exercised (a real `codex-mcp-client/0.157.1` connected but only ever
negotiated `2025-06-18`), so the modern-era leg of criterion 2 is confirmed for Claude
only, not Codex — the existing open item at `11-risks.md` row 41/RISK-G4, unchanged, not
closed. Two new UNVERIFIED items: (1) Claude's v2 runtime (`MCP_SDK_GENERATION=v2`) sent
a stdio `server/discover` probe even though `MCP_PROTOCOL_NEGOTIATION` was never set this
session, which contradicts `code.claude.com/docs/en/mcp.md`'s stated stdio default
("Connects on earlier protocol, doesn't ask about newer revision") — not yet
independently re-confirmed in a clean session; (2) security-relevant — a Codex daemon
`thread/list` query during this spike showed Codex-Desktop-originated thread entries
whose preview text was **Claude Code prompt content**, i.e. cross-harness prompt
visibility through Codex's own session history, mechanism not investigated. Fixtures:
`docs/planning/gates/fixtures/g4-mcp-dual-era/transcript-2026-09-26-outofbox.jsonl`
(58 lines, renamed in the same change that added the PASS re-run's fixture above) and
`transcript-run1.jsonl` (10 lines), both redacted, no residual leaks beyond the
intentional public extension identifier.)

**Last updated:** 2026-09-26 (**G2 re-run on Codex `0.157.1`, issue #35/D2: PASS.**
The Codex row's last-observed version (`docs/planning/PINS.md`) and the environment both
report `0.157.1`, commit `36650394c5b38c2990ccf2a3457165ca3e9d9726`, so the floating-pin
currency condition (`docs/planning/gates/README.md` §"Re-run/invalidation policy") is
satisfied and G2's verdict is current again. The re-run repeated the two live-inject
paths (idle `turn/start`, mid-turn `thread/queue/add`) against the same TUI thread and
daemon on Windows, same scope as the `0.154.0` run (default `CODEX_HOME`, non-elevated
terminal; macOS/Linux still not exercised); the operator's own TUI paste (quoted verbatim
in `docs/planning/gates/G2-result.md`) confirms both deliveries rendered in order: "OAC
G2 RERUN RECEIVED", the full 40-item lighthouse list, then "OAC G2 QUEUED". Before
re-running the gate, every §3.2 fact G2's PASS rested on was re-checked against the
`0.157.1` source tree (`docs/planning/REVERIFICATION-B2.md` §"§3.2 re-verification at
Codex `0.157.1` (floating-pin trigger, 2026-09-26)"): **no drift affecting any fact G2's
PASS rested on**, including the Windows DACL/peer-elevation protection code, which is
byte-identical between the `0.154.0` and `0.157.1` commits. Two additive drifts were
found, both off by default and neither exercised by either G2 run: (a) a new
`--no-daemon` opt-out flag (`codex-rs/tui/src/cli.rs`), which includes a matching
rejection added to `codex-rs/tui/src/session_queue_commands.rs`; and (b) the opt-in
`mcp_2026_07_28` MCP client mode (`stage: UnderDevelopment`) — flagged for G4/`oac-mcp`,
not a G2 concern. One behavior-preserving rename was also found (`accept_hdr_async` →
`accept_hdr_async_with_config`). (An earlier draft of this re-verification wrongly
inferred "unchanged" for two facts from their absence in a `gh compare` diff that turned
out to be capped at 300 of 869 changed files; both were corrected by fetching the files
directly — see REVERIFICATION-B2.md's "Note on the GitHub compare API's 300-file cap.")
This closes the "Codex §3.2 facts not re-verified at the observed `0.157.1`" open item
below and `11-risks.md` row 40. Full result: `docs/planning/gates/G2-result.md`
("Re-run history"); fixture:
`docs/planning/gates/fixtures/g2-codex-inject/transcript-2026-09-26-0.157.1.jsonl`,
redacted. The unidentified second thread and the `originator`/`source` provenance items
from the `0.154.0` run stay open — this run's daemon loaded only one thread, which does
not itself close either item.)

**Last updated:** 2026-09-26 (Codex pin changed to **floating** by operator decision.
During G4, a Codex auto-updater (`codex app-server daemon pid-update-loop`, not started by
OAC) moved the environment from `0.154.0` to `0.157.0`, and then to `0.157.1` within
about 24 hours. The operator chose to leave it running. The PINS.md Codex row now records
the last observed version (`0.157.1`, commit `36650394c5b38c2990ccf2a3457165ca3e9d9726`)
under a written floating-version policy. Per the pin-move checklist, **G2 is invalidated**:
it PASSED on `0.154.0` and is `NOT RUN` for the current environment. G4 gains the Codex
row as a relied-on pin. Re-verifying the Codex §3.2 facts on `0.157.1` is a new
UNVERIFIED item. Stale copies were updated in `oac-codex-appserver` and `11-risks.md`.)

**G3/issue #36/D3:** Zenoh 1.10.1 local-peer spike on two of
three platforms. **Windows 11 PASS, Linux (WSL2 Ubuntu 24.04) PASS, macOS NOT RUN**
(parked, no host), so the gate-level verdict stays `NOT RUN`. 36 of 36 matrix runs passed:
multicast and rendezvous, each over TCP and TLS on `127.0.0.1`, plus 3-peer collision
runs. Separate peer processes found each other over loopback multicast on 1.10.1 with
explicit `127.0.0.1` listeners. This run did not reproduce the pre-1.10.0 failure modes
that PR #2671 fixed. A negative control (multicast off, no rendezvous) correctly failed. In
rendezvous mode, joiners built a direct link to each other by gossip. `#iface=` is enforced
on Linux. On Windows, a nonexistent interface name was accepted without exception; a valid
name and log warnings were not tested. The scouting socket binds `0.0.0.0:7446` on Windows
and `224.0.0.224:7446` on Linux, and several processes shared it without collision. Median
cold-start discovery was 51 ms on Windows and 7 ms on Linux for multicast TCP. One
rendezvous run took 1011 ms; by inference from its timings, that was a start race plus the
default 1 s connect retry. The spike used the Python binding `eclipse-zenoh==1.10.1`
(core = tag `1.10.1`), not the Rust crate. As a result, "G3 via the Rust crate" is a new
UNVERIFIED item, and the binary-size estimate stays open, owned by task I3. Result:
`docs/planning/gates/G3-result.md`; fixtures: `docs/planning/gates/fixtures/g3-zenoh-peer/`.

**G2/issue #35/D2:** gate spike PASS on the primary path,
implicit daemon attach, on Codex `0.154.0`, with no pin drift. A plainly launched `codex`
TUI attached to a running `codex app-server daemon`: its thread was loaded in the daemon
process. A second daemon client delivered a message into that live thread by
`turn/start` when idle and by `thread/queue/add` mid-turn. The queued turn started at the
same second the running turn completed. The TUI showed both deliveries and the model
answered each one. The spike client handled no credentials. Closes the G2 go/no-go
UNVERIFIED item. Daemon attach runs at runtime in the released `0.154.0`, not only on
`main`. This was shown **on Windows, with default `CODEX_HOME`, from a non-elevated
terminal**; macOS and Linux were not exercised. New facts recorded in
`docs/planning/gates/G2-result.md`: the control socket speaks WebSocket over UDS,
`codex app-server proxy` is a raw byte relay, `thread/queue/add` is experimental and
absent from the default schema, and a client must call `thread/resume` to receive
`turn/*`/`item/*` events. On Windows the socket is protected by a user-only directory
DACL, and no server-side per-connection check was found. Also recorded: `thread/list`
exposes every session's preview to any same-user socket client. Two new UNVERIFIED items
are added below: an unidentified second loaded thread, and the non-per-client
`originator`/`source` fields. Stale copies of the closed daemon-attach item are corrected
in `oac-codex-appserver`, `PINS.md` and `11-risks.md`. Fixture:
`docs/planning/gates/fixtures/g2-codex-inject/transcript.jsonl`, redacted; D6 gap:
`thread/start` was not captured.

**G1/issue #34/D1:** gate spike PASS — a throwaway Claude
Channels MCP server negotiated legacy MCP `2025-11-25`, declared
`capabilities.experimental["claude/channel"]`, and all five G1 pass criteria were
confirmed live against a real Claude Code session: legacy negotiation, idle-session wake
as a user turn with the correct `<channel>` tag attribute set, mid-turn queueing
delivered together and in order, a tool-based reply, and the
`--dangerously-load-development-channels` consent dialog actually exercised. Full result:
`docs/planning/gates/G1-result.md`; raw transcript:
`docs/planning/gates/fixtures/g1-claude-wake/transcript.jsonl`. Pin drift found: the
connecting client reported `v2.1.282`, not the pinned `v2.1.274` — flagged above as a new
open item, not silently re-pinned. Epic D (Stage 1) is now open; Epic A and C rows
corrected above to reflect they closed earlier.

**A1/issue #23:** `docs/planning/v0.1/00-summary.md` landed,
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
| Open epics | A (closed — full v0.1 package landed), C (closed), D (Stage 1 gate spikes — G1/D1 PASSED on Claude Code `v2.1.282`, invalidated 2026-09-27 when the Claude Code (Channels) pin went floating (last observed `v2.1.283`), **re-run and PASSED again 2026-09-28** on `v2.1.283` (issue #39 T6/T7, Box C — see `docs/planning/gates/G1-result.md`); G2/D2 PASS, re-run 2026-09-26 on the current Codex `0.157.1`; G3/D3 Windows and Linux PASS, macOS parked; G4/D4 PASS, re-run 2026-09-26 under a fresh timebox after an earlier out-of-box `NOT RUN`, see `docs/planning/gates/G4-result.md`; G5/D5 **FAIL** on Codex (2026-09-27), Claude PASS — see `docs/planning/gates/G5-result.md`), J (agent skills) |
| Blocked | Stages 2-6, and the rest of Stage 1 pending D3's macOS leg, and D7 (D5 has now run; a G5 `FAIL` does not itself block D7 per `docs/planning/v0.1/10-stages.md` §5's Gate S1 acceptance criterion 1 (`FAIL` is a closed verdict) — D7 is still blocked on D3's macOS leg only, G1's re-run PASSED 2026-09-28 and no longer blocks it; separately Stage 2 cannot freeze the Codex provenance interface until the C13 design change lands). No substantial core or transport code starts before Stage 0 and Stage 1 fully complete. |

## ADR amendments

ADR amendments: A1-A3 issued, see `docs/planning/ADR-001-AMENDMENTS.md`
(authoritative for verbatim old/new text), assembled and cited (not copied) in
`docs/planning/v0.1/03-decisions-and-amendments.md`. Resolves
conflict register entries C1-C3 directly (`RESOLVED-HERE`); C5, C7 assigned or
resolved-by-evidence per that file's conflict register table; new entries C11-C13 added,
all open (see below). C8 is closed separately, by `docs/planning/decisions/
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

G1 PASSED on Claude Code `v2.1.282` (2026-09-25, issue #34/D1). The Claude Code
(Channels) pin went floating (2026-09-27, operator decision, issue #39/T0), last
observed `v2.1.283`, one version above what G1 recorded, so per the floating-row rule
the verdict went stale pending re-run. **Re-run in the same HIL sitting as the D6
Claude capture (issue #39 T6/T7): two attempts.** Box B (2026-09-28) was recorded
incomplete (several pass-criteria probes not attempted). Box C (2026-09-28, same day)
closed those gaps and **PASSED**: criteria 1, 2, 4 evidenced as cleanly as the original;
criterion 5 with the dialog's own text captured verbatim before acceptance; criterion 3
judged directly against its literal wording (order preserved, nothing dropped, nothing
interleaved) — operator decision, 2026-09-28. Box C also observed mid-turn
notifications landing at two separate tool-call boundaries rather than batched together
at one, as the original PASS recorded — this is treated as an observed `2.1.283`
behavior change, not a criterion failure; every "delivered together" statement in this
repo has been amended with a dated note, and delivery batching is now tracked as
UNVERIFIED-as-a-guarantee (`docs/planning/v0.1/11-risks.md` row 49). **G1 is now PASS**
(current, `v2.1.283`); the original `v2.1.282` PASS is superseded and kept as history.
See `docs/planning/gates/G1-result.md`. G2 PASSED on Codex `0.154.0` (2026-09-25,
issue #35/D2), was invalidated when the Codex row went floating, and has been **re-run and
PASSED on `0.157.1`** (2026-09-26, same issue/task) — the row's last-observed version and
the environment both report `0.157.1`, so the verdict is current. G3 has run on two of its three platforms
(2026-09-25, issue #36/D3): Windows and Linux (WSL2) PASS. macOS is NOT RUN, so the
gate-level verdict stays `NOT RUN` until the macOS leg runs. G4 spiked on 2026-09-25/26
(issue #37/D4); that run's evidence was gathered after its declared 120-minute timebox
had already expired, so it recorded `NOT RUN` despite every criterion individually
confirming. A fresh, redeclared 60-minute timebox on 2026-09-26 stayed unexpired
(~8.5 minutes of live work) and reconfirmed all five criteria — **G4 is now PASS**, no
fallback needed. See `docs/planning/gates/G4-result.md` for both runs and the full
timebox accounting. **G5 spiked on 2026-09-27 (issue #38/D5) under a 60-minute timebox
that closed early (~30 min elapsed) without expiring and recorded FAIL** (Codex criteria
2/3 f; Claude all criteria x): Claude passed every criterion evaluated for it; Codex
failed criterion 2 (a forged nested envelope with a non-matching guessed delimiter named
the forged id as the sender in part (1) of the model's answer — case X2; a
peer-controlled header-value injection produced two `oac_sender:` lines the model could
not resolve — case X5) and criterion 3 (case X2 alone). A forged block replaying a real,
already-sent delimiter (case X3) caused ambiguity, not acceptance — the model named
neither id — so it is not failing evidence for either criterion. No fallback exists for
G5. See `docs/planning/gates/G5-result.md`.
Every task labelled `gate:G1`, `gate:G2`, `gate:G3`, `gate:G4` or `gate:G5` stays blocked until its
gate has a current gate-level verdict. Per `docs/planning/v0.1/10-stages.md` §5's Gate S1
acceptance criterion 1, a `FAIL` is a closed verdict, so this does not itself block
**Stage 1's own exit** (D7 — separately still blocked by G3's macOS leg only; G1's
re-run PASSED 2026-09-28 and no longer blocks it); per §5's
go/no-go condition and §2, it blocks the pipeline from proceeding past **Stage 2's
interface freeze** for Codex's provenance framing until conflict-register entry C13
lands and DESIGN acceptance criterion 6 is re-established for Codex.

| Gate | Verdict | Decides | Pins relied on | Result file |
|---|---|---|---|---|
| G1 Claude wake | **PASS** (re-run 2026-09-28 on Claude Code `v2.1.283`, Box C; primary path: legacy MCP negotiation, idle wake, mid-turn queueing, tool reply — the mid-turn pair landed at two separate tool-call boundaries rather than batched together at one, an observed `2.1.283` behavior change, not a criterion failure). Originally PASSED on `v2.1.282` (2026-09-25); invalidated 2026-09-27 when the Claude Code (Channels) pin went floating; that original record is kept as history. | Claude adapter viability. go/no-go, no fallback. | Claude Code (Channels); MCP — current era; MCP — legacy era; Rust MCP SDK (rmcp) | `docs/planning/gates/G1-result.md` |
| G2 Codex live inject | **PASS** (re-run 2026-09-26 on `0.157.1`, the Codex row's current last-observed version; primary path: implicit daemon attach). Previously invalidated 2026-09-26 when the Codex row went floating; was **PASS** on `0.154.0` before that. | Codex adapter viability. Fallback: OAC-owned app-server with `codex --remote`. | Codex CLI / app-server | `docs/planning/gates/G2-result.md` |
| G3 Zenoh local peer | NOT RUN at gate level — Windows 11 **PASS**, Linux (WSL2) **PASS**, macOS NOT RUN (parked) | Loopback peer discovery on Windows, macOS, Linux. Fallback: fixed local endpoint, no scouting. | Zenoh; Rust toolchain | `docs/planning/gates/G3-result.md` |
| G4 MCP dual-era server | **PASS** (re-run 2026-09-26, fresh 60-min timebox, not expired; primary single-process design, no fallback needed). An earlier attempt (timebox declared 2026-09-25, evidence gathered 2026-09-26) recorded `NOT RUN` — all five criteria confirmed then too, but after its own timebox had expired. A 2026-09-27 row-41 probe addendum (separate 20-min box) does not change this verdict. | One process serving both MCP eras. Fallback: two entry points, one core. | Claude Code (Channels); MCP — current era; MCP — legacy era; Rust MCP SDK (rmcp); Codex CLI / app-server (Codex leg) | `docs/planning/gates/G4-result.md` |
| G5 Provenance | **FAIL** (Codex criteria 2/3 f; Claude all criteria x) (2026-09-27, 60-min timebox, not expired; no fallback exists). | Machine-set provenance contradicts a spoofing claim on both providers. | Codex CLI / app-server; Claude Code (Channels) | `docs/planning/gates/G5-result.md` |

Re-run/invalidation policy (what moves a verdict back to `NOT RUN`, and the pin-move
checklist): `docs/planning/gates/README.md`.

## Pins

Confirmed. Detailed record, sources, and constraint floors: `docs/planning/PINS.md`.

| Surface | Pinned version | Source |
|---|---|---|
| Claude Code | **floating**; last observed `v2.1.283` (2026-09-25T21:50:12Z UTC). The earlier fixed pin was `v2.1.274`. Channels research preview; permission relay `>= v2.1.234` satisfied | PINS.md — Claude Code Channels ("Floating-version policy") |
| MCP | current `2026-07-28`; legacy `2025-11-25` | PINS.md — MCP revisions |
| Codex CLI | **floating**; last observed `@openai/codex@0.157.1`, commit `36650394c5b38c2990ccf2a3457165ca3e9d9726` (2026-09-26). The earlier fixed pin was `0.154.0` | PINS.md — Codex CLI and app-server ("Floating-version policy") |
| Zenoh | `1.10.1` (2026-09-07); `>= 1.10.0` required for loopback discovery | PINS.md — Zenoh |
| ACP | protocol version `1` (schema v2 alpha); not a v0.1 dependency | PINS.md — ACP |
| Rust toolchain | `1.98.1` (2026-09-03); `rust-toolchain.toml` enforces it | PINS.md — Rust toolchain |
| Rust MCP SDK | `rmcp` `3.4.0` (2026-09-15); legacy revision `2025-11-25` supported and is the SDK's default | PINS.md — Rust MCP SDK (`rmcp`) |
| `ed25519-dalek` | `3.0.0`; envelope signature algorithm; BSD-3-Clause (flagged, not the usual `MIT OR Apache-2.0` shape) | PINS.md — `ed25519-dalek` |
| `serde_jcs` | `0.2.0`; RFC 8785 JCS canonicalization; MIT OR Apache-2.0 | PINS.md — `serde_jcs` |
| herdr (test tooling) | `v0.9.1` (2026-09-16), fixed; Apache-2.0; dev/test tooling only, never shipped; gates affected: none; live behavior UNVERIFIED (K1 provisional go) | PINS.md — herdr (test tooling) |

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
- **K1 — herdr evaluation (test tooling)** (issue #124, Epic K #123): **provisional go,
  pending live confirmation; live leg NOT RUN.** herdr is pinned at `v0.9.1` (fixed,
  Apache-2.0) as dev/test tooling only, never shipped, with `Gates affected: none`. The
  final go/no-go waits on the operator-run live checklist. Full record:
  `docs/planning/decisions/K1-herdr-evaluation.md`. This is a test-tooling decision, not
  folded into `docs/planning/v0.1/03-decisions-and-amendments.md`.

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
- C13 (new, from G5, issue #38/D5, 2026-09-27): `docs/planning/decisions/
  C6-trust-rendering.md` §5's Codex header-and-delimiter framing got the model to name
  the forged id as the sender in part (1) of its answer against a forged block using a
  wrong-but-plausible, guessed delimiter (case X2); a real delimiter replayed from an
  earlier delivery in the same conversation (case X3) did not get the model to name the
  forged id — it caused ambiguity, not acceptance — but did leave it unable to resolve a
  sender at all, a related, narrower finding, not itself failing evidence; separately, a
  peer-controlled `oac_reply_to` value is inserted unmodified into the header block,
  producing a header with two `oac_sender:` lines the model cannot resolve (case X5).
  Owner: unassigned — no backlog task owns amending `C6-trust-rendering.md` itself; task
  E5 (Stage 2 security spec) is the nearest downstream consumer blocked by it, not its
  owner. See `docs/planning/gates/G5-result.md`,
  `docs/planning/v0.1/03-decisions-and-amendments.md` §4,
  `docs/planning/ADR-001-AMENDMENTS.md` "New register entries".

## Open UNVERIFIED items

Carried from PLANNING-PROMPT.md §3, re-verified against the B1 pins in B2
(`docs/planning/REVERIFICATION-B2.md`). Until closed, no plan or skill may rely on them
without an UNVERIFIED label.

- **New, from G1 (issue #34/D1):** the Claude Code Channels pin (`docs/planning/PINS.md`
  — `v2.1.274`) is stale. The client that connected during the G1 spike (2026-09-25)
  reported itself as `v2.1.282`; the installed surface auto-updates faster than this
  project re-pins it. A full B2-style re-verification of every §3.1 fact against
  `2.1.282` has NOT been done — only G1's five pass criteria were checked and passed.
  UNVERIFIED whether any other §3.1 fact drifted between `v2.1.274` and `v2.1.282`.
  Owner: a B2-style re-verification pass, not yet scheduled as a task. **Further drift
  observed at G4 (2026-09-26, issue #37/D4):** the client connecting during both G4 runs
  reported `v2.1.283`, one patch above G1's `v2.1.282` — see
  `docs/planning/gates/G4-result.md`. The B2-style re-verification this item calls for
  has still not been done at either observed version. **Pin now floating (2026-09-27,
  issue #39/T0):** rather than keep tracking each patch bump as a one-off drift note, the
  PINS.md Claude Code (Channels) row now records `v2.1.283` as its last-observed version
  under a written floating-version policy, mirroring the Codex row. Per the pin-move
  checklist, G1 was invalidated (`NOT RUN` for the current environment) since it had run
  on `v2.1.282`, not the new last-observed `v2.1.283`. **Re-run and PASSED again
  2026-09-28** (issue #39 T6/T7, Box C) — see `docs/planning/gates/G1-result.md`. This
  does not close the re-verification gap above; the full §3.1 B2-style re-verification
  at `v2.1.283` remains open — only G1's own five pass criteria were re-checked.
- **New, from G1:** the exact wire framing for Claude Code's MCP stdio transport
  (newline-delimited JSON, not `Content-Length`-prefixed) — confirmed directly during
  G1, but not previously stated in any OAC document; carried here as new evidence, not
  from PLANNING-PROMPT.md. Not currently UNVERIFIED (it's confirmed), noted here as a
  new fact for anyone relying on the old, wrong assumption.
- **Closed by G5 (issue #38/D5, 2026-09-27):** the exact wrapper text a mid-turn-delivered
  channel notification gets. G5 case C6 captured it verbatim at Claude Code `2.1.283`:
  `<system-reminder>\nA message arrived from <name> while you were working:\n<channel
  …>…</channel>\n\nIMPORTANT: This is NOT from your user — it came from an external
  channel …\n</system-reminder>` — see `docs/planning/gates/G5-result.md`.
- Claude channel behaviour across `--resume`/`--continue` (UNVERIFIED — docs silent at
  v2.1.274; see REVERIFICATION-B2.md §3.1 box 1).
- Whether one MCP server can present more than one logical channel (UNVERIFIED — docs
  silent at v2.1.274; see REVERIFICATION-B2.md §3.1 box 2).
- Whether Codex Desktop exposes the control socket in current builds (UNVERIFIED — no
  first-party statement found; see REVERIFICATION-B2.md §3.2 box 5. G2 observed
  Desktop-originated sessions in the daemon's `thread/list` only as `notLoaded` saved
  history. That shows shared on-disk history, not live socket exposure, so the item
  stays open; see `docs/planning/gates/G2-result.md`).
- G3 criteria 1-4 on macOS: loopback multicast discovery, rendezvous discovery, TLS on
  `127.0.0.1`, and multi-peer port behavior (UNVERIFIED — macOS leg parked, no macOS host;
  macOS is also the primary target of PR #2671's loopback-TX fix;
  the same quarantined matrix runs there unchanged; see `docs/planning/gates/G3-result.md`).
- G3 on bare-metal Linux (UNVERIFIED — the Linux leg ran on WSL2 Ubuntu 24.04 with a real
  Linux kernel, links on `lo`; treated as the Linux leg per the operator's direction).
- `#iface=` behavior on macOS, and on Windows with a *valid* interface name (UNVERIFIED —
  G3 confirmed Linux enforces it. On Windows, G3 tried only a nonexistent name, which was
  accepted without exception while the link bound on loopback. A valid Windows name and
  Zenoh log warnings were not tested. macOS was not tested).
- G3 criteria via the Rust `zenoh` crate built with toolchain `1.98.1`, using OAC's feature
  set and embedded in the OAC runtime (UNVERIFIED — G3 ran the Python binding
  `eclipse-zenoh==1.10.1` on the same tag-`1.10.1` core; see
  `docs/planning/gates/G3-result.md`).
- Implicit Codex daemon attach at runtime on macOS and Linux, `0.157.1` (UNVERIFIED — G2
  has exercised Windows only, on both `0.154.0` and the `0.157.1` re-run; see
  `docs/planning/gates/G2-result.md`).
- An unidentified second thread (`01a0d744-b34a-7c92-9011-20d95fe5f98a`) was loaded in
  the daemon during G2 but never listed by `thread/list`. It is probably a TUI-spawned
  side thread; its purpose is unknown (UNVERIFIED — see
  `docs/planning/gates/G2-result.md`).
- In a shared Codex daemon, a thread's `originator` and `source` fields do not reliably
  identify the client that created it. At `0.154.0` the TUI's thread was stamped with the
  first-initializing probe's `clientInfo.name` (`oac_g2_spike`) and `source: "vscode"`.
  At the `0.157.1` re-run, on a separately fresh daemon, it instead carried `originator:
  "codex-tui"` and `source: "vscode"` — matching the TUI itself, consistent with the TUI
  being the first client to initialize that daemon this time (UNVERIFIED — mechanism
  inferred from two fresh-daemon observations, one per version; do not use either field
  for provenance; see `docs/planning/gates/G2-result.md`).
- Cross-process resume does not attach (openai/codex #21743) at runtime, not re-tested at
  `0.154.0` or `0.157.1` (UNVERIFIED — the test appends silently to a real thread's
  history; see `docs/planning/gates/G2-result.md`).
- Zenoh `auth.pubkey` semantics (UNVERIFIED — see REVERIFICATION-B2.md §3.4 box 6; the
  six key names themselves are now CLOSED, confirmed verbatim in `DEFAULT_CONFIG.json5`
  at tag 1.10.1).
- The 5-15 MB Zenoh binary size estimate (UNVERIFIED — derived estimate; see
  REVERIFICATION-B2.md §3.4 box 7. Earlier text said the first G3 build artifact from task
  D3 would resolve it. D3 ran the Python wheel and built no Rust artifact, so the owner is
  now task I3, which records the actual release binary size).
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
  table). G2 confirmed only that the subcommand is absent at runtime, at `0.154.0` and
  again at `0.157.1` (`docs/planning/REVERIFICATION-B2.md`'s 0.157.1 fact table, fact 7).
  The two dates themselves remain unverified.
- Codex `mcp_2026_07_28` client mode untested against a G4 server. Source-level only, at
  `0.157.1`: an opt-in MCP `2026-07-28` client protocol mode now exists
  (`codex-rs/rmcp-client/src/protocol_mode.rs`, feature flag `mcp_2026_07_28`, stage
  `UnderDevelopment`, `default_enabled: false`), alongside a new `mcp_2026_*` test suite
  (UNVERIFIED — not exercised by G2, which relies only on Codex's default `2025-06-18`
  client behavior; see `docs/planning/REVERIFICATION-B2.md` §"§3.2 re-verification
  at Codex `0.157.1` (floating-pin trigger, 2026-09-26)"). **Open after both G4
  runs (issue #37/D4, 2026-09-25/26 out-of-box and 2026-09-26 re-run):** a real
  `codex-mcp-client/0.157.1` process connected to the G4 spike server in both runs but
  every one of its `initialize` requests, both times, negotiated
  `protocolVersion: "2025-06-18"` — it never attempted the modern era. G4 itself is
  `PASS` (its second pass criterion is satisfied via Claude as the modern client, not
  Codex) — see `docs/planning/gates/G4-result.md`. **Narrowed by the 2026-09-27
  row-41 probe:** `codex exec --enable mcp_2026_07_28` **does** negotiate `2026-07-28`
  against this server on every request over the HTTP-registered leg — the flag works —
  while a separate, same-run stdio (`config.toml`) registration still negotiated legacy.
  What stays UNVERIFIED is only the *default* (flag-off) client behavior and any
  `rmcp`-based server; see `docs/planning/gates/G4-result.md` "Row-41 probe addendum".
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
- **New, from G4 (issue #37/D4):** Claude Code's v2 runtime (`MCP_SDK_GENERATION=v2`) sent a stdio
  `server/discover` probe carrying `_meta["io.modelcontextprotocol/protocolVersion"]:
  "2026-07-28"` even though `MCP_PROTOCOL_NEGOTIATION` was never set in that session,
  which contradicts `https://code.claude.com/docs/en/mcp.md`'s stated stdio default
  ("By default (unset): Connects on earlier protocol, doesn't ask about newer
  revision") (UNVERIFIED — not re-tested against a fresh `.mcp.json` in a clean
  session; reproduced identically in both the out-of-box run and the 2026-09-26 PASS
  re-run; see `docs/planning/gates/G4-result.md`). **Reproduced a second, third, and
  fourth time, 2026-09-28 (issue #39 T4/T6/T7, D6 Claude capture Box A, and the G1
  re-run's Box B and Box C):** Box A's own capture
  (`docs/planning/gates/fixtures/d6-claude-protocol/transcript-2026-09-28.jsonl` line 3)
  shows the same probe. In Box B, the operator confirmed `$env:MCP_SDK_GENERATION` was
  empty in that terminal (checked directly, after `Remove-Item`), yet
  `docs/planning/gates/fixtures/g1-claude-wake/transcript-2026-09-28-2.1.283.jsonl`
  (line 3) still shows a `server/discover` probe — so this is not an
  `MCP_SDK_GENERATION=v2`-only effect as the G4 finding's own framing implied; the probe
  fired with that variable unset. Box C (same terminal session as Box B, same
  `MCP_SDK_GENERATION`-unset state) reproduced it again
  (`transcript-2026-09-28-2.1.283-boxC.jsonl` line 3). **Correction to an earlier draft
  of this note:** the claim that "all three observations share a project directory with
  prior MCP-server registrations" was wrong — the original G4 observation was captured
  in the unrelated `g4-spike/` scratchpad directory, while Box A was captured in
  `d6-spike/` and Boxes B/C were captured in `g1-spike/`. **Precisely:** the 2026-09-28
  occurrences alone span two distinct working directories (`d6-spike/` for Box A, where
  `MCP_SDK_GENERATION` was recorded as `"v2"`, not confirmed empty — Box A is evidence
  for the `=v2` case, not the empty-var case; `g1-spike/` for Boxes B and C, where the
  env var was confirmed empty); a third directory (`g4-spike/`) only enters the count
  when including the original G4 occurrence, from a different date. Still UNVERIFIED
  against a fresh `.mcp.json` in an otherwise-clean session with no prior MCP-server
  registrations at all; the documented stdio default is still not confirmed to hold
  under any tested condition on Claude Code `2.1.283`, in either the `=v2` or the
  empty-var case. See `docs/planning/v0.1/11-risks.md` row 42, updated with these
  occurrences.
- **New, security-relevant, from G4:** a Codex daemon `thread/list` query made during
  the G4 spike returned Codex-Desktop-originated thread entries whose `preview` text was
  Claude Code prompt content (e.g. slash-command text the operator typed into Claude
  Code), i.e. cross-harness prompt visibility through Codex's own session history. The
  import mechanism was not investigated (UNVERIFIED — see
  `docs/planning/gates/G4-result.md` and `docs/planning/v0.1/11-risks.md`
  RISK-CODEX-EXPERIMENTAL). Twice while the out-of-box run's idle server sat unused
  afterward (09:08:19Z and 15:57:32Z, both well before the later PASS run's own
  20:46Z-20:54Z window), a client reporting user-agent `codex-mcp-client/0.155.0-alpha.16.4`
  connected via the (likely shared) global `codex mcp add` registration, initialized and
  listed tools; its MCP OAuth well-known discovery probes all returned 404. Attribution
  to Codex Desktop is inferred from the user-agent string alone, and the cause is
  UNVERIFIED — cited to the uncommitted archive
  `scratchpad/g4-spike/transcript-2026-09-26-outofbox.jsonl` lines 60-73 and 76-89.
- **New, from the G4 re-run (issue #37/D4, 2026-09-26):** Claude Code does not surface a
  tool result's `_meta` field to the model. The G4 spike server's HTTP-modern
  `tools/call` response carried `_meta["io.github.rossgraeber/oac-session-channels"]` on
  the wire, but when the operator asked Claude to show the full result including
  `_meta`, Claude reported: "The call to mcp__g4http__g4_echo worked, but no _meta
  reached me." (UNVERIFIED whether this is universal or specific to this tool-call path;
  design-relevant to Decision 8/9's provenance rendering — see
  `docs/planning/gates/G4-result.md` and `docs/planning/decisions/C6-trust-rendering.md`).
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
- Whether implicit daemon attach is enabled by default at runtime is now **confirmed,
  not open** (G2 PASS on both `0.154.0` and the `0.157.1` re-run, Windows only — see
  `docs/planning/gates/G2-result.md`). What stays open is only the **documentation-drift
  half**: a 2026-09-17 re-fetch of `https://learn.chatgpt.com/docs/app-server` did not
  surface the `codex app-server daemon start` command or the `app-server-control.sock`
  control-socket path that PLANNING-PROMPT.md §3.2's pre-verified baseline states, and
  this has not been re-checked against the docs page since (UNVERIFIED — one added data
  point, not itself a resolution; see
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
- **New, from G1 Box C (issue #39, 2026-09-28):** whether mid-turn `notifications/
  claude/channel` deliveries are batched together at a single tool-call boundary, or can
  arrive at separate boundaries one at a time, is UNVERIFIED as a guarantee — it may
  depend on the notifications' relative send timing. The original G1 PASS
  (`v2.1.282`) observed two notifications arrive together, between the same pair of
  tool calls; G1 Box C (`v2.1.283`) observed two notifications, sent ~1.85s apart, arrive
  at two separate tool-call boundaries instead. Both runs agree on order-preserved,
  nothing dropped, nothing interleaved inside a tool call — only the batching claim is
  unconfirmed. `PLANNING-PROMPT.md` §3.1, `oac-claude-channels`, `oac-gates/
  references/G1-claude-wake.md`, and `docs/planning/v0.1/04-architecture.md` are each
  amended with a dated note pointing here rather than silently rewritten. See
  `docs/planning/gates/G1-result.md` "Re-run attempt 2 (Box C)" and
  `docs/planning/v0.1/11-risks.md` row 49.
- **New, from K1 (issue #124, 2026-09-28):** herdr `v0.9.1`'s live behavior is
  UNVERIFIED. The K1 live leg is NOT RUN: the session that wrote the record had no herdr
  binary and no logged-in harness. Open items:
  - named-session start with no attached terminal;
  - `agent start --kind claude` / `--kind codex` argv and cwd, including `claude
    --dangerously-load-development-channels server:<name>`;
  - the timeout options: confirmed in docs and source for `agent prompt --wait`,
    `agent wait` and `pane wait-output`, and refuted at the desk for `agent read` and
    `agent send-keys`;
  - the dialog text being readable through `agent read` before any keystroke (expected
    from the bundled Claude manifest, not observed);
  - Codex's post-response state (the "`unknown` after a response" premise was not found
    in any first-party herdr source);
  - the launch-environment delta, derived from source only;
  - harness `settings.json`/`hooks.json`/`config.toml` staying unchanged;
  - per-OS support on Linux, macOS and Windows, which is herdr's documentation claim
    only.

  K1's go/no-go is therefore provisional, not final. Owner: an operator-run live leg per
  `docs/planning/decisions/K1-herdr-evaluation.md` §6. See also
  `docs/planning/v0.1/11-risks.md` row 51 (`RISK-HERDR`).
- **New, from K4 (issue #127, 2026-09-28):** the scripted G1 re-run through herdr is
  built but has **never run live**. `tools/herdr/scenarios/g1-claude-wake.mjs`, the
  comparator `tools/herdr/lib/compare-transcripts.mjs` and the report generator
  `tools/herdr/lib/g1-report.mjs` are exercised only against test doubles (a fake herdr
  and a fake Claude Code, `node tools/herdr/run.mjs --self-test`). No `-herdr` fixture,
  no `docs/planning/gates/herdr-runs/G1-<date>.md` record and no comparison result exist
  yet. The Claude Code pane-text patterns the scenario schedules on (the in-progress
  indicator, and every dialog except the dev-channels one recorded in Box C) are
  unconfirmed. The run is not verdict-bearing either way: G1's verdict is unchanged.
  Owner: an operator run per the scenario's header comment.
- **New, from K6 (issue #129, 2026-09-28):** the opt-in CI workflow
  `.github/workflows/herdr-provider-optin.yml` and its entry point `tools/herdr/ci.mjs`
  are built but have **never run on GitHub Actions**. No self-hosted runner with label
  `oac-harness` is registered, and no dispatch has run on Linux or Windows. K6's
  acceptance item "one successful G1 dispatch on Linux and one on Windows, run URLs
  recorded" is open: the record table in `docs/planning/gates/herdr-runner.md` §8 is
  empty. Checked statically only: actionlint, the containment lint's workflow rules
  (check 9) and the stage-gate self-test against the fake herdr. The repository is public,
  so the fork-PR controls in `herdr-runner.md` §1 (repository settings, not testable
  here) must be set before any runner is registered (`docs/planning/v0.1/11-risks.md`
  row 52). Owner: the operator, per `herdr-runner.md` §7.
- **New, from D6/T5-T7 (issue #39):** whether `turn/start` and `thread/queue/add`
  subscribe the calling connection to `turn/*`/`item/*` events, the way `thread/start`,
  `thread/resume`, and `thread/fork` are source-confirmed to
  (`codex-rs/app-server/src/request_processors/thread_processor.rs` L1562-1580,
  L4009-4015, L5227), is UNVERIFIED — inferred only from the same file's
  request-handling structure not carrying an equivalent "Auto-attach a thread
  listener" call near either handler; not directly source-confirmed the way the three
  auto-attach sites are. See `oac-codex-appserver/references/thread-lifecycle.md` and
  `docs/planning/v0.1/11-risks.md` row 50.

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
