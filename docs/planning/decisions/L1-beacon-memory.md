# L1: Beacon beside OAC, not inside it — pin, license, operator decisions Q1-Q5

**Issue:** #166 (Epic L #165, backlog key `L1`). **Depends on:** none. **Source:** issue
#165 workstreams L1 and L3; `[ADR-001 Boundary]`; `[DESIGN Non-goals]`;
`[PLANNING-PROMPT §10]`; `oac-boundaries` #12.

**Status:** **Decided.** Beacon (agent-beacon) is an external, independently installed,
review-gated memory service that each harness connects to natively over MCP. OAC never
stores, fetches, summarizes, or injects memory. Beacon is pinned at `v1.3.29`, fixed
(§2). Its license is MIT, recorded as an external service that OAC does not ship (§3).
The operator's answers to Q1-Q5 (2026-09-29) are recorded in §4 and are not re-opened
by later Epic L tasks. Four facts stayed UNVERIFIED and were handed to L2 (§6); L2's
desk research closed three and narrowed one (§11), and §12 is the live leg for the last.

**Where this record lives.** `docs/planning/decisions/` holds the C-series product
decisions and K1's test-tooling decision. L1 is an integration-boundary decision about a
service OAC does not ship or call. It is **not** folded into
`docs/planning/v0.1/03-decisions-and-amendments.md`, and no ADR-001 text is changed by
it (§4 Q5, §5). `docs/planning/STATUS.md` carries a pointer to this file ("Decisions
landed", "Open UNVERIFIED items").

**Why no live leg.** This is desk work only. No Beacon install, no harness login and no
spike code was needed or used (issue #166, last paragraph). The live capture questions
belong to L2.

## 0. Timebox

- **Desk leg:** run 2026-09-29 by a cloud agent session. No timebox was declared before
  the tag re-read started. That is a process deviation from `oac-gates`' timebox policy,
  recorded here the way `docs/planning/decisions/K1-herdr-evaluation.md` §0 records the
  same deviation for its desk leg. The record was written as-is in one sitting and is not
  extended past it. Items not closed in that sitting stay UNVERIFIED (§6); none was
  guessed.
- **Live leg:** none for L1. L2 owns the timeboxed live spike (issue #165 workstream L2).

## 1. Decision

The shape is **"Beacon beside OAC, not inside it."** Seven points are in bounds, taken
from issue #165 "What stays in bounds" and adjusted only where §4 Q1 closed an option:

1. **Beacon is a peer service each harness reaches natively.** Claude Code and Codex
   connect to `beacon mcp serve` through their own MCP configuration, or through Beacon's
   plugin. That is the ADR-001 MAY clause in action, used by the *harness*, not by OAC:
   "It MAY use documented MCP extensions, app-server protocols, ACP, hooks, extensions,
   or other supported IPC, and translate neutral messages into provider-native
   live-session input operations." (`docs/planning/ADR-001.md` line 26). OAC does not
   launch, configure, proxy, or speak to Beacon's MCP server. It does not vendor or link
   any Beacon code.
2. **Memory stays in the harness's context, owned by the harness.** Recall happens when
   a harness runs `beacon-memory-recall` or `get_memory_context` in its own turn. OAC
   delivers only what a sender put in an envelope. No enrichment, no trimming, no
   summarization, no injection.
3. **OAC has no protocol touchpoint for memory.** Q1 chose option (a) (§4): a memory ID
   is ordinary text in `content[].text`, like any other text a sender writes. No spec
   content type is added. Option (b), a typed memory-reference content entry, is
   rejected for v0.1.
4. **Authenticated but untrusted, extended to memory.** A memory ID in an envelope is
   *content*: a claim by the sender, rendered inside the untrusted body, never as
   provenance. This follows `docs/planning/v0.1/06-security.md` §3(e) ("Provenance
   (machine-set metadata) versus content (untrusted body)") and ADR-001 "Security model":
   "Authenticated peer messages remain untrusted instructions and may contain prompt
   injection. Provenance must remain machine-enforced and distinct from message
   content." (`docs/planning/ADR-001.md` line 58). The receiving harness resolves the ID
   through its own Beacon connection. Whatever Beacon returns is untrusted text to that
   harness as well. Beacon's own skill already says it "treats memory as lessons from
   earlier sessions, not as policy" (`docs/concepts/beacon-skills.mdx` L84 at
   `v1.3.29`, §10). OAC never renders a memory ID or a memory body in the machine-set
   provenance block.
5. **No credential handling.** OAC never holds Beacon's hosted-service OAuth tokens or
   `--token-env` personal tokens. `beacon-managed` is out of scope.
6. **No durable-mailbox side door.** Beacon's `search_activity` over `runtime.jsonl`
   must not be used to "catch up on missed messages". That would be the deferred offline
   mailbox ("Defer ... durable offline mailboxes", `docs/planning/ADR-001.md` line 63)
   plus application-level polling ("application-level inbox polling is not for adapters
   claiming active inbound support", `docs/planning/DESIGN.md` "Delivery semantics").
7. **Transport containment is unaffected.** Nothing here touches `transports/zenoh/`.
   With option (a), the neutral protocol gains nothing at all.

**Boundary lines this decision runs up against, quoted verbatim from issue #165:**

- `[ADR-001 Boundary]` "MUST NOT ... implement inference/model routing/context
  management." (Full sentence: `docs/planning/ADR-001.md` line 24.)
- `[DESIGN Non-goals]` "shared context management". (`docs/planning/DESIGN.md` line 15.)
- `[PLANNING-PROMPT §10]` "Do not turn OAC into an agent framework, inference router,
  shared context manager, or provider-auth abstraction." (`docs/planning/PLANNING-PROMPT.md`
  line 200.)
- `oac-boundaries` #12's drift example, verbatim: "a 'conversation memory' store shared
  across both providers so replies can reference earlier turns from either harness —
  that is a shared context manager."
- `[ADR-001 v0.1 scope]` defers "durable offline mailboxes" and "attachments".
  (`docs/planning/ADR-001.md` line 63.)
- `[ADR-001 Boundary]` "MUST NOT steal or reuse another harness's provider
  credentials" — read here as also covering any Beacon Cloud OAuth session a harness
  holds. ("Beacon Cloud" is #165's branch-head name; the tag calls the same service
  "Beacon Managed", §2 D1.)
- `[ADR-001 Boundary]` "MUST NOT leak Zenoh-specific concepts into the neutral
  protocol."

**Out of scope (boundary, not backlog)**, restated from issue #165 so this record stands
alone: OAC reading or writing `memory.db`; calling any `beacon` MCP tool; running
`beacon` as a subprocess; bundling or vendoring Beacon; a Beacon workspace dependency;
auto-attaching memory to outbound messages; injecting recall results into inbound
messages; summarizing session history; routing on memory content; any OAC-held Beacon
or Jev credential; using Beacon's activity log or dashboard for delivery, presence or
catch-up; Beacon's hosted service, SIEM forwarding and the browser extension; a
Beacon-specific adapter, transport or content type name in the neutral spec; writing
memory on a peer's behalf.

## 2. Pin

- **Version and release tag:** `v1.3.29`. The annotated tag object is
  `72fd6643b5cd5c6ff6741f6016b3577654f61915` and points to commit
  `91e92216b79108475ba9b587d49c5ff3f7356fd8`. The tag message reads `v1.3.29`. The
  tagger timestamp is 2026-09-28T13:43:29Z, and the commit date is
  2026-09-28T09:39:51-04:00. Source: `git ls-remote --tags
  https://github.com/Asymptote-Labs/agent-beacon` and a `--depth 1` clone of tag
  `v1.3.29`, retrieved 2026-09-29. The GitHub release page for the tag could not be
  fetched from this session (the proxy refused it), so the release date recorded in
  `docs/planning/PINS.md` is the tagger date, and no release-page claim is made.
- **Still the latest release tag on the day.** Issue #165 named `v1.3.29` as the latest
  tag. That was re-checked, not trusted: `git ls-remote --tags` on 2026-09-29 lists no
  tag above `v1.3.29` (sorted by version, the top three are `v1.3.27`, `v1.3.28`,
  `v1.3.29`).
- **Why the tag and not branch head.** Issue #165's facts were read at default-branch
  head `26581914e86f525e225096613c3a9b808043ff85` (committed
  2026-09-29T06:18:22-04:00), which is newer than the tag and is not a release. A pin is
  a release. Every Beacon fact in this record was re-read **at `v1.3.29`**; two drifts
  between the tag and branch head are recorded below.
- **Fixed, not floating, while self-updates stay off.** Beacon is installed and upgraded
  by the operator through a package manager ("Upgrade | `brew upgrade beacon`",
  `docs/get-started/quickstart.mdx` L110 at `v1.3.29`). Beacon does ship a package
  self-update, but it is off by default and opt-in: "Endpoint package self-updates are
  available for Apple Silicon system package installs, but remain off by default; IT
  admins can opt into `check-only` monitoring or `auto` package updates during Jamf or
  Fleet rollout." (`docs/mdm/index.mdx` L10 at `v1.3.29`, commit
  `91e92216b79108475ba9b587d49c5ff3f7356fd8`, retrieved 2026-09-29; the same default is
  stated in `docs/mdm/fleet.mdx` L88-90, `docs/mdm/jamf.mdx` L76-77 and
  `docs/mdm/rippling.mdx` L235). The pin therefore holds a fixed version **only while
  those self-updates stay off**. Operators running Beacon beside OAC should leave them
  off (no `check-only` or `auto` opt-in), and L2's fixtures record the Beacon version
  actually running. The pin is still expected to move often: `v1.3.27`, `v1.3.28` and
  `v1.3.29` are consecutive recent tags (§6, `RISK-BEACON`).
- **Gates affected: none.** No G1-G5 verdict depends on Beacon. A move of this row
  invalidates no gate. It does require the §6 items and every §10 citation to be re-read
  at the new tag, and L2's fixtures to record the version they ran on.

**Drift between the pinned tag and issue #165's branch-head reading** (recorded per
`oac-evidence` §7 step 4; neither drift changes a decision):

| # | Fact | At tag `v1.3.29` | At head `26581914` (as #165 read it) | Effect |
|---|---|---|---|---|
| D1 | Name of Beacon's hosted forwarding destination | "Beacon Managed" (`README.md` L49, L151; `cli/beacon/README.md` `## Beacon Managed` L519; `docs/cli/mcp.mdx` `## Local and Beacon Managed servers` L22). `SECURITY.md` L71 names the same channel "Asymptote Managed forwarding" | "Beacon Cloud" (`README.md` L49, L151; `cli/beacon/README.md` `## Beacon Cloud` L519; `docs/cli/mcp.mdx` `## Local and Beacon Cloud servers` L22) | Naming only. The MCP server name `beacon-managed` and the flag `--privacy-mode metadata-only` are identical at both. This record says "Beacon's hosted service" and quotes the tag's own name where it quotes. |
| D2 | Where "hold other servers' credentials" is written | `docs/cli/mcp-connect.mdx` L102 | `docs/cli/mcp-connect.mdx` L102 | **Citation correction, not upstream drift.** Issue #166 attributes the quote to `docs/cli/mcp-doctor.mdx`; it is in `mcp-connect.mdx` at both commits. `mcp-doctor.mdx` holds the stdio `mcpServers` entry (L43-52). §4 Q2 cites the right file. |

## 3. License

**MIT.** `LICENSE` at the tag reads "MIT License" and "Copyright (c) 2026 Asymptote
Labs". Source: https://github.com/Asymptote-Labs/agent-beacon/blob/v1.3.29/LICENSE, tag
`v1.3.29`, retrieved 2026-09-29. The same text is at head `26581914` (issue #165).

Beacon is not linked into, vendored into, shipped with, or invoked by the `oac` binary
or OAC's test tooling. Each harness connects to it natively. It therefore adds nothing to
the Apache-2.0 compatibility verdict for the shipped inventory
(`docs/planning/v0.1/07-repository-and-dependencies.md` §7). It is recorded under a new
§5 heading in that file, "External services — not shipped (outside this inventory)".
MIT is permissive, so even a later doc-and-config-level integration raises no license
question. The Stage 6 license inventory (`oac-release`) should list it as an external
service the operator installs, not as a distribution dependency.

## 4. Operator decisions Q1-Q5

Made by the operator on 2026-09-29 (issue #166, "Operator decisions to record
verbatim"). Each entry gives the question as issue #165 posed it, the decision, and the
rationale.

### Q1 — Option (a) or (b)? → **(a), docs-only.**

- **Decision.** Memory IDs travel as ordinary text in `content[].text`. **No new spec
  content type.** Epic L workstream L3 (spec touchpoint) is **closed by this record:
  no spec change needed.** No Stage 2 spec task exists for Beacon, and none is created.
- **Rationale.** Nothing in the envelope or content model has to change. The envelope's
  `content` field already carries `{"type": "text", "text": ...}`
  (`docs/planning/v0.1/05-interfaces.md` §3), and a sender may write a memory ID into
  that text like any other identifier. §3's forward-compatibility rule ("a receiver
  `MUST` ignore an array entry whose `type` it does not recognize rather than rejecting
  the whole envelope for it") is cited only to show that option (b) is not needed to keep
  the door open: a typed entry could still be added later as a non-breaking change, if a
  concrete consumer is ever named. Option (a) is zero spec surface and zero neutral-
  vocabulary risk. Option (b) would have been a permanent v0.1 spec commitment made
  before Stage 2 opens, with no named consumer (issue #165, Q1 recommendation).
- **Consequence.** L3 in issue #165's workstream table is closed without work. The
  epic's acceptance criterion 4 ("If option (b) is chosen ...") does not apply. The
  mechanical boundary checks' expectation of zero Beacon references under `spec/` has no
  exception.

### Q2 — A read-only `oac doctor` check? → **No.**

- **Decision.** There is **no `oac doctor` check** that reads harness MCP configuration
  files to detect Beacon. L5 has no `cli/` touchpoint.
- **Rationale.** Those files hold other servers' credentials. Beacon's own wording:
  "Harness configs are your files, and several of them hold other servers' credentials."
  Source: https://github.com/Asymptote-Labs/agent-beacon/blob/v1.3.29/docs/cli/mcp-connect.mdx
  L102, tag `v1.3.29`, retrieved 2026-09-29 (not `mcp-doctor.mdx`, see §2 D2). Reading
  them sits next to `[ADR-001 Boundary]` "MUST NOT steal or reuse another harness's
  provider credentials". A Beacon detector would also be OAC configuring or inspecting
  Beacon, which point 1 of §1 rules out. An operator who wants to check a harness's
  Beacon entry runs Beacon's own `beacon mcp doctor` or `beacon mcp status --check`
  (`docs/cli/mcp-doctor.mdx` L53-57 at `v1.3.29`).

### Q3 — Capture-side privacy → **Recommend Local mode; if hosted forwarding is enabled, Metadata-only.**

- **Decision.** OAC's docs recommend, as a recommendation with rationale and not as a
  requirement, that OAC-enabled sessions run Beacon in **Local** mode (no forwarding).
  If the operator enables Beacon's hosted forwarding, OAC's docs recommend
  **Metadata-only** privacy for it.
- **Scope, re-checked at the pinned tag.** Metadata-only is a **hosted-forwarding** mode,
  not a capture mode:
  - It is set on the forwarding connection: `beacon endpoint connect --privacy-mode
    metadata-only` (`docs/cli/endpoint-connect.mdx` L104-106 at `v1.3.29`).
  - "Content handling is applied before events are written to `runtime.jsonl`." and
    "Metadata-only mode adds a local Vector transform that removes retained text, tool
    arguments/results, command output, raw fields, diffs, inventory content, and MCP
    definitions before buffering or upload while preserving stable metadata, hashes,
    counts, and token usage." (`docs/security/retention-redaction.mdx` §"Forwarding
    implications" L68-82 at `v1.3.29`).
  - "Standard privacy forwards locally sanitized retained content; `--privacy-mode
    metadata-only` strips retained text, raw fields, diffs, inventory content, and MCP
    definitions locally before upload." (`cli/beacon/README.md` `## Beacon Managed`
    L533-535 at `v1.3.29`; the same section is `## Beacon Cloud` at head, §2 D1).
  - Local capture is unchanged by it: "Beacon may write prompt text, command output, raw
    attributes, tool input, and diff content to local JSONL. Secret redaction,
    sanitization, truncation, and event-size limits are applied before events are
    written or forwarded" (`SECURITY.md` §"Redaction and Size Limits" L52-57 at
    `v1.3.29`).
  - Local mode is a first-class choice. Interactive setup "preselects Beacon Managed,
    with an explicit Local opt-out" and "Choose Local to keep everything on this
    machine" (`README.md` L48-49, L151-155 at `v1.3.29`). Because the hosted option is
    preselected, recommending Local is a real, not a redundant, recommendation.

  All sources: https://github.com/Asymptote-Labs/agent-beacon/tree/v1.3.29, tag
  `v1.3.29`, retrieved 2026-09-29.
- **Rationale.** With Beacon on, an OAC message delivered into a Beacon-instrumented
  session is candidate trace data (§6 item U1 on whether it is captured at all). Local
  mode keeps it on the machine. Metadata-only keeps its text off Beacon's hosted service
  when forwarding is on. This is Beacon's documented behaviour, not an OAC defect, but it
  changes the residual-risk column for "accidental cross-project disclosure" and "leaked
  credentials" (DESIGN "Security").
- **Residual, handed to L4.** Neither mode changes local capture: `runtime.jsonl` still
  holds redacted, sanitized and truncated content, rotated at 10 MiB with five archives
  (`SECURITY.md` L20 at `v1.3.29`). That local-capture residual is an **open risk**, not
  a mitigation, and goes to L4's threat rows in `docs/planning/v0.1/06-security.md` §14.
  Customer-configured SIEM or file-based shippers read the same local JSONL
  (`retention-redaction.mdx` L70-76) and are not covered by Metadata-only; they are out
  of Epic L's scope (issue #165 "Out of scope").

### Q4 — Project scoping mismatch → **Accepted, documented, not solved.**

- **Decision.** OAC accepts the mismatch and documents it. It does not try to align the
  two scopes.
- **The mismatch.** Beacon memory is per resolved repository: "Approved memory is scoped
  to the resolved project. Cross-project or user-global memory is not automatic."
  (`docs/concepts/cross-harness-memory.mdx` L71-72), and "A linked git worktree resolves
  to the same project as its main checkout." (`docs/cli/memory.mdx` L35-36), both at
  `v1.3.29`, retrieved 2026-09-29. OAC sessions are per `working_directory`: "a session
  registered from one working directory must not be discoverable by a peer not
  authorized for that directory" (`docs/planning/decisions/C4-session-identity.md` §5).
- **Consequence for docs (L5).** Two OAC peers in different checkouts of different
  repositories see different memory, even for what a human calls "the same project".
  Two OAC peers in two worktrees of one repository are two OAC sessions but one Beacon
  project. OAC docs must not promise shared memory across peers. Solving this would mean
  OAC mapping sessions to Beacon projects, which is OAC configuring Beacon (§1 point 1).

### Q5 — Does the operator want the out-of-bounds version? → **No. Stay inside ADR-001.**

- **Decision.** No ADR-001 amendment. No OAC-injected context. The chosen shape is §1.
- **Rationale.** The operator's goal ("a persistent memory all of those sessions can
  reach") is met by each harness connecting to Beacon natively. It does not need OAC to
  become the context layer. §5 records the rejected reading and the amendment route, so a
  later task does not have to re-derive them.

## 5. Rejected reading and amendment route

**Rejected reading: OAC as the shared context layer.** "Integrate Beacon as a shared
context layer *of OAC*" would mean OAC reading `memory.db`, calling
`get_memory_context` on a session's behalf, attaching memory to envelopes, summarizing
history, or choosing a recipient based on what memory says. Each of those is a shared
context manager: `[ADR-001 Boundary]` "MUST NOT ... implement inference/model
routing/context management", `[DESIGN Non-goals]` "shared context management",
`[PLANNING-PROMPT §10]` "Do not turn OAC into an agent framework, inference router,
shared context manager, or provider-auth abstraction", and `oac-boundaries` #12 ("a
'conversation memory' store shared across both providers so replies can reference
earlier turns from either harness — that is a shared context manager"). It is **out of
bounds without an ADR-001 amendment**.

**Amendment route, named, not proposed.** If the operator ever wants that reading, the
route is `oac-evidence` §6: record the conflict (what ADR-001 says, what the goal needs,
file and section for each), cite evidence per `oac-evidence` §2, and propose the next
free `ADR-001-A<n>` with old text (verbatim), new text and rationale, before any code.
The number is chosen at that time by checking PLANNING-PROMPT.md Appendix A and
`docs/planning/v0.1/03-decisions-and-amendments.md`. As of 2026-09-29, `A1`-`A3` are
allocated and `A4` is only named hypothetically
(`docs/planning/decisions/C5-envelope-auth.md` §"Does this need a new numbered
`ADR-001-A4` amendment"). Per Q5, this record proposes nothing. `docs/planning/ADR-001.md`
and `docs/planning/ADR-001-AMENDMENTS.md` are unchanged.

## 6. UNVERIFIED ledger

The four items issue #165 listed, re-read at `v1.3.29`. None closed. Each is handed to
L2 (the timeboxed desk-plus-live spike). Mirrored in `docs/planning/STATUS.md` "Open
UNVERIFIED items" and in `docs/planning/v0.1/11-risks.md` `RISK-BEACON`, traceability
rows 53-56.

**L2 update (issue #167, 2026-09-29):** the desk leg's results are in §11. U2 and U3 are
CONFIRMED and U4 is REFUTED, each with `path@v1.3.29` citations. U1 stays UNVERIFIED,
narrowed to the harness side, and goes to §12's operator-run live leg (herdr-driven since
2026-09-30, #187; see §12). The text below is
L1's original wording, kept for traceability.

**L3 update (issue #192, 2026-10-01):** U1 is CONFIRMED live for both harnesses (§13,
§11 item 1). No L1 item remains UNVERIFIED.

- **U1.** Whether Beacon's Claude Code capture (hooks/OTLP) records the content of an OAC
  channel notification delivered into the session, or only the harness-visible
  tool/prompt events (UNVERIFIED — no first-party statement found at `v1.3.29`).
  Narrowed at the tag: the Claude Code runtime page lists the hooks Beacon installs
  (`SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`,
  `PostToolUseFailure`, `Stop`, `SubagentStart`, `SubagentStop`, `PermissionRequest`,
  `SessionEnd`) and maps `user_prompt` to `prompt.submitted`
  (`docs/runtimes/claude-code.mdx` L86, L93). It says nothing about channel
  notifications. The Codex side has the same open question for input OAC's adapter
  delivers: "Beacon uses Codex semantic logs for session, prompt, approval, and
  tool-result events" (`docs/runtimes/codex-cli.mdx` L23). L2 owns the live capture.
- **U2.** The exact shape and stability of the memory item ID and of `get_memory` /
  `get_memory_context` results across releases (UNVERIFIED — the docs at `v1.3.29`
  describe the tools (`docs/cli/mcp.mdx` L57-69) and the skill provenance keys
  `beacon_memory_id`, `beacon_candidate_id`, `beacon_memory_kind`, `beacon_tags`
  (`docs/cli/memory.mdx` L257), but no versioned response schema). With Q1 = (a), OAC
  never parses these; the item matters only for L5's walkthrough and L6's scenario.
- **U3.** Whether one `memory.db` can be shared by several harness sessions concurrently
  without a documented locking model (UNVERIFIED — not stated at `v1.3.29`; `memory.db`
  is "durable local state" (`docs/cli/memory.mdx` L28-31), and the MCP memory tools are
  read-only (`docs/cli/memory.mdx` L265-271), but no concurrency statement exists).
- **U4.** Whether Beacon's Codex integration touches Codex configuration in a way that
  conflicts with OAC's Codex adapter launch (UNVERIFIED — OAC's launch path is not built,
  and no one has compared the two). Narrowed at the tag: `beacon endpoint install`
  "writes Codex OTLP exporter tables to `~/.codex/config.toml`"
  (`docs/runtimes/codex-cli.mdx` L43, L88), Beacon writes Codex hooks to
  `~/.codex/hooks.json` or `./.codex/hooks.json` (`docs/cli/hooks.mdx` L34, L449), and
  `beacon mcp connect` edits `~/.codex/config.toml` as text, touching only its
  `beacon-managed` entry (`docs/cli/mcp-connect.mdx` L102-105, L179).

All four sources: https://github.com/Asymptote-Labs/agent-beacon/tree/v1.3.29, tag
`v1.3.29`, retrieved 2026-09-29.

**Closed at the desk by first-party evidence** (not UNVERIFIED): the pin `v1.3.29` (§2);
the MIT license (§3); the Metadata-only scope (§4 Q3); the memory scoping rule (§4 Q4);
the local server's MCP protocol revision, `"protocolVersion": "2024-11-05"`
(`cli/beacon/internal/mcpserver/server.go` L211 at `v1.3.29`, same line as at head), an
older revision than OAC's "MCP — legacy era" pin `2025-11-25`. The revision is
informational: it is Beacon-to-harness, not an OAC surface, and does not touch
gate G4.

## 7. Surface labels, boundary pass

**Labels** (`oac-evidence` §4). These are Beacon's surfaces, which harnesses use. None is
an OAC provider surface. OAC never reaches them, so no compatibility shim boundary is
needed. The enforcement point is the mechanical boundary checks (zero Beacon references
in product paths).

| Surface | Label | Note |
|---|---|---|
| `beacon` local MCP server (`beacon mcp serve`) and its seven tools | supported | Documented in `docs/cli/mcp.mdx` and `docs/cli/mcp-serve.mdx` at `v1.3.29`, with no preview or experimental marker. Reached by the harness only |
| `beacon memory` CLI and approved-memory MCP tools (`search_memory`, `get_memory`, `get_memory_context`) | supported | Documented in `docs/cli/memory.mdx` at `v1.3.29`. The feature is "opt-in" (`cross-harness-memory.mdx` L8), which is a user choice, not a stability label. Issue #165 left the stability UNVERIFIED until the tag re-read; at `v1.3.29` no doc marks it preview, beta or experimental, so the label is the documented one |
| `beacon-managed` MCP server and Beacon's hosted forwarding | supported | Documented, hosted and account-bound. **Out of Epic L's scope** |
| Beacon hooks / OTLP capture per harness | supported | Documented (`README.md` "Local Agent Coverage": Claude Code and Codex CLI `OTLP + hooks + poll`, L272, L275). What it captures of OAC-delivered input is U1 |

**Boundary pass** — `oac-boundaries` "Pre-commit self-check", run 2026-09-29 against
every file this change touches:

- [x] **Mechanical checks.** Checks 3 and 8 from `.github/workflows/boundary-lint.yml`
      and `node scripts/check-herdr-containment.mjs` were run; this change touches only
      `docs/`, so they report the same result as `origin/main`. `git diff --name-only
      origin/main | grep -E '^(core|spec|adapters|cli|transports)/'` is empty. No
      Beacon reference is introduced outside `docs/planning/`. No new pending-path gap.
- [x] **No OAC-owned turn loop.** Recall runs in the harness's own turn (§1 point 2).
- [x] **No credentials held, read or reused.** OAC holds no Beacon or Jev token (§1
      point 5). Q2 declines a doctor check precisely because it would read files that
      hold other servers' credentials.
- [x] **No polling adapter.** §1 point 6 forbids Beacon's activity log as a catch-up
      path.
- [x] **Every boundary hit handled.** The one hit, the literal "OAC as shared context
      layer" reading, is recorded as rejected, with the amendment route named and not
      taken (§5).

Each `oac-boundaries` entry that could apply was checked: #2 and #12 (context
management — §1 points 2-3, §5); #3 and #10 (credentials, replacement auth — §1 point 5,
Q2); #5 (Zenoh — §1 point 7); #11 (deferred mailboxes and attachments — §1 point 6, Q1);
#14 (polling — §1 point 6). The `oac-security-work` §3 doctrine is stated in §1 point 4.
No threat-table row is written here; L4 owns them, and Q3's residual is handed to it as
an open risk.

## 8. Acceptance boxes

Ticked against issue #166's acceptance list:

- [x] `test -f docs/planning/decisions/L1-beacon-memory.md`, and the file has at least 11
      `## ` headings (§0-§10).
- [x] `grep -n 'Beacon (external memory service)' docs/planning/PINS.md` hits the
      pin-table row and the `###` record; the row's `Gates affected` cell reads `none`.
- [x] `grep -n 'External services — not shipped'
      docs/planning/v0.1/07-repository-and-dependencies.md` hits once; the row says MIT.
- [x] `grep -n 'RISK-BEACON' docs/planning/v0.1/11-risks.md docs/planning/STATUS.md`
      hits in both files.
- [x] Q1-Q5 each appear in §4 with the decision and rationale; §4 Q1 states "no spec
      change needed" and that no Stage 2 spec task exists for Beacon.
- [x] Every Beacon fact carries URL + tag/commit + retrieval date (§2, §3, §4, §6, §10);
      unresolved facts are labelled UNVERIFIED (§6) and mirrored in STATUS.md and
      `11-risks.md`.
- [x] `oac-boundaries` "Pre-commit self-check" run and recorded in §7.
- [x] No gate verdict, pin value or ADR text changes: `git diff --name-only origin/main
      -- docs/planning/gates docs/planning/ADR-001.md docs/planning/ADR-001-AMENDMENTS.md`
      is empty, and no path under `core/`, `spec/`, `adapters/`, `cli/` or
      `transports/` changes. The new PINS.md row is an addition, not a changed value.
- [x] `node scripts/check-skills.mjs`, `node scripts/check-herdr-containment.mjs` and
      `node scripts/check-fixture-manifest.mjs` still pass.

## 9. Cross-file updates in this change

- `docs/planning/PINS.md`: new pin-table row `Beacon (external memory service)`
  (`v1.3.29`, fixed, `Gates affected: none`), a new record `### Beacon (external memory
  service)` after `### herdr (test tooling)`, and `**Last updated:**` bumped. The
  pin-move checklist was executed in the same commit: the row names no gate, so no
  `G<n>-result.md` is invalidated, and no `Pin rows relied on` field or STATUS.md
  `Pins relied on` cell changes.
- `docs/planning/v0.1/07-repository-and-dependencies.md` §5: new heading "External
  services — not shipped (outside this inventory)" after "Dev/test tooling — not
  shipped", with one Beacon row (MIT). §9 gets a surface-label row.
- `docs/planning/v0.1/11-risks.md`: new risk `RISK-BEACON` under R5 and traceability
  rows 53-56.
- `docs/planning/STATUS.md`: a `**Last updated:**` entry, a `## Pins` summary row, a
  "Decisions landed" bullet, and one "Open UNVERIFIED items" entry carrying U1-U4.
- No skill file, backlog file, gate result, spec text or ADR text is changed. L7's
  `references/beacon.md` is a separate task.

## 10. Sources

All at https://github.com/Asymptote-Labs/agent-beacon, tag `v1.3.29` (commit
`91e92216b79108475ba9b587d49c5ff3f7356fd8`), retrieved 2026-09-29, unless stated:

- `git ls-remote --tags https://github.com/Asymptote-Labs/agent-beacon` (tag list, tag
  object and commit SHAs)
- `LICENSE`; `README.md` (§"Beacon Overview" L34, §"Quick Start" L46-52, L148-155,
  §"Local Agent Coverage" L267-275); `SECURITY.md` (L11-20, L52-57, L62-78)
- `cli/beacon/README.md` §"Beacon Managed" (L519-540)
- `docs/cli/mcp.mdx`, `docs/cli/mcp-serve.mdx`, `docs/cli/mcp-connect.mdx`,
  `docs/cli/mcp-doctor.mdx`, `docs/cli/memory.mdx`, `docs/cli/endpoint-connect.mdx`,
  `docs/cli/hooks.mdx`
- `docs/concepts/cross-harness-memory.mdx`, `docs/concepts/beacon-skills.mdx`
- `docs/security/retention-redaction.mdx` §"Forwarding implications"
- `docs/runtimes/claude-code.mdx`, `docs/runtimes/codex-cli.mdx`
- `docs/get-started/quickstart.mdx` (§"Useful commands")
- `docs/mdm/index.mdx` L10, `docs/mdm/fleet.mdx` §"Self-updates" L88-90,
  `docs/mdm/jamf.mdx` L76-77, `docs/mdm/rippling.mdx` L235 (package self-updates, off by
  default)
- `cli/beacon/internal/mcpserver/server.go` L211
- For the drift table (§2) only: the same repository at default-branch head
  `26581914e86f525e225096613c3a9b808043ff85`, retrieved 2026-09-29 (`README.md`,
  `cli/beacon/README.md`, `docs/cli/mcp.mdx`, `docs/cli/mcp-connect.mdx`)

OAC-internal: `docs/planning/ADR-001.md` lines 24, 26, 58, 63; `docs/planning/DESIGN.md`
"Non-goals", "Delivery semantics", "Security"; `docs/planning/PLANNING-PROMPT.md` §10;
`docs/planning/v0.1/05-interfaces.md` §3; `docs/planning/v0.1/06-security.md` §3(e),
§14; `docs/planning/decisions/C4-session-identity.md` §5;
`docs/planning/decisions/C5-envelope-auth.md`; `docs/planning/decisions/K1-herdr-evaluation.md`
(section shape); issues #165 and #166.

## 11. Desk research (L2)

**Issue:** #167 (Epic L #165, backlog key `L2`), desk leg only. It closes, refutes or
carries each §6 item from Beacon's own source and docs **at the pinned tag**.

- **Timebox:** 90 minutes of desk reading, declared at 2026-09-29T18:39Z before the tag
  re-read started. The record below was written as-is inside the box. The box was not
  extended. What the desk could not settle is carried to §12's live leg, not guessed.
- **Where each fact was read.** A `--depth 1` clone of tag `v1.3.29` (commit
  `91e92216b79108475ba9b587d49c5ff3f7356fd8`). Every citation below is written
  `path@v1.3.29 L<n>` and means
  `https://github.com/Asymptote-Labs/agent-beacon/blob/v1.3.29/<path>`, retrieved
  2026-09-29. Branch head was not used for any fact. As a drift check, nine of the Go
  source files cited below were byte-compared against branch head
  `26581914e86f525e225096613c3a9b808043ff85` (retrieved 2026-09-29): the MCP server, the
  memory store, the memory record type, both session mappers, the harness configurator,
  the Claude and Codex hook installers and the prompt hook. All nine are identical, so
  §2's drift table gains no row.
- **Harness-side behaviour is out of reach here.** Item 1 turns partly on what Claude
  Code and Codex write into their own hooks, telemetry and session files when OAC
  delivers input. Beacon's tree cannot answer that. The desk leg records what Beacon
  does with whatever the harness emits, and hands the harness side to §12.
- **Verdict words.** CONFIRMED means the question's fact was established from
  first-party Beacon source at the tag. REFUTED means the hypothesis in the question (for
  item 4, a collision) is false at the tag. UNVERIFIED means it stays open, narrowed.

### Item 1 — Does Beacon's capture record OAC-delivered input? → CONFIRMED live by L3 (2026-10-01, §13); L2's desk verdict was UNVERIFIED (narrowed; Beacon side confirmed, harness side open)

**L3 update (issue #192, 2026-10-01): CONFIRMED live for both harnesses** (§13 B2-B4;
Beacon `1.3.29`, Claude Code `2.1.285`, Codex `0.159.3`, Windows). The text below is L2's
desk record, kept as written.

- **Claude Code.** A `notifications/claude/channel` delivery is recorded as
  `prompt.submitted`, with the text in `prompt.text`, `gen_ai.input.messages` and
  `raw.attributes.prompt` (collection methods `hook` and `otlp`).
- **Not on the poll path.** Claude Code writes the delivery to its session file as a
  `user` entry with `isMeta` `true`, which the poll mapper skips, so
  `beacon endpoint claude sync` records nothing for it.
- **Reply tool.** The reply-tool call is recorded as `tool.invoked` and
  `mcp.tool_invoked`, with the arguments at `gen_ai.tool.call.arguments.message`.
- **Codex.** Input sent with `turn/start` and with `thread/queue/add` is recorded as
  `prompt.submitted` via OTLP (harness `codex_desktop`) and on the
  `beacon endpoint codex sync` poll path (harness `codex_cli`).
- **Redaction.** A fake secret-shaped token in the message body was stored **unredacted**
  on every path that captured it.
- **Answers to "What stays open" below.** Claude Code reports a channel delivery to both
  Beacon's hook and its OTLP receiver: at the B2 snapshot there is one `prompt.submitted`
  line per method. It also writes the delivery as a meta `user` entry.
  - The harness event names are inferred, not recorded: that the hook line comes from
    `UserPromptSubmit` and the OTLP line from `claude_code.user_prompt` follows from
    Beacon's source mapping (points 1 and 3 below). The record names collection methods,
    not harness events.
  - By the same mapping, Codex's OTLP line implies `codex.user_prompt` and its poll line
    a `user` rollout message.

**Beacon has no channel-specific handling at all.** A search of the whole tag tree for
`notifications/claude/channel`, `claude/channel` and `<channel` (Go, TypeScript, JSON,
Markdown and MDX files) returns zero hits. Beacon neither filters nor tags a channel
delivery. It records whatever the harness reports through its three Claude Code paths
and its Codex paths:

- **Claude Code hooks.** `beacon endpoint hooks install` writes a `UserPromptSubmit`
  hook, a `PreToolUse` hook whose matcher includes `mcp__.*`, and `PostToolUse` /
  `PostToolUseFailure` hooks with matcher `*` (`cli/beacon/internal/endpoint/hooks/claude.go@v1.3.29`
  L63-77). The prompt hook takes the text from the first non-empty field among
  `prompt`, `user_prompt`, `userPrompt`, `text`, `promptText`, `input`, and stores it as
  `prompt.text` plus retained content on a `prompt.submitted` event
  (`cli/beacon-hooks/cmd/prompt_submit.go@v1.3.29` L37-60).
- **Claude Code OTLP.** `beacon endpoint install` sets `OTEL_LOG_USER_PROMPTS` = `1`
  and `OTEL_LOG_TOOL_DETAILS` = `1` in the `env` block of `~/.claude/settings.json`
  (`cli/beacon/internal/endpoint/harness/harness.go@v1.3.29` L359-397, the two keys at
  L386-387). The collector maps `claude_code.user_prompt` to `prompt.submitted`
  (`collector-builder/exporter/beaconjsonexporter/internal/beaconevent/converter.go@v1.3.29`
  L43). Tool details include "MCP server/tool arguments"
  (`docs/runtimes/claude-code.mdx@v1.3.29` L69).
- **Claude Code poll** (`beacon endpoint claude sync`, explicit or scheduled, "for
  backfill and catch-up", `docs/runtimes/claude-code.mdx@v1.3.29` L43, L123-125). It
  reads the session files under `~/.claude/projects` and maps **every** `user` entry
  that is not `isMeta` to `prompt.submitted` with its text
  (`cli/beacon/internal/claudesession/mapper.go@v1.3.29` L68-107, L161-166). The only
  text it drops contains `<local-command-caveat>` or `<command-name>` (same file
  L416-422). It also maps each assistant `tool_use` block, input included, to
  `tool.invoked`, or `mcp.tool_invoked` for an `mcp__<server>__<tool>` name (L128-139,
  L190-210). It maps `tool_result` blocks and assistant text as well (L96-99, L140-143).
- **Codex OTLP.** `beacon endpoint install` writes an `[otel]` table with
  `log_user_prompt = true` into `~/.codex/config.toml`
  (`cli/beacon/internal/endpoint/harness/harness.go@v1.3.29` L399-415, L505-521). The
  collector handles `codex.user_prompt` (`converter.go@v1.3.29` L22). Beacon's doc says
  Codex Desktop's `codex-app-server` emits the same telemetry once the user-level
  `~/.codex/config.toml` enables OTLP (`docs/runtimes/codex-cli.mdx@v1.3.29` L23).
- **Codex hooks.** Metadata-only: the Codex hook set is a single `SessionStart`
  `codex-session-context` hook (`cli/beacon/internal/endpoint/hooks/codex.go@v1.3.29`
  L63-73). Beacon's doc says it "never reads transcript content"
  (`docs/runtimes/codex-cli.mdx@v1.3.29` L23).
- **Codex poll** (`beacon endpoint codex sync`, explicit, "Beacon does not read
  `~/.codex/sessions` during normal Codex hook execution", `docs/runtimes/codex-cli.mdx@v1.3.29`
  L35-39). It maps every `message` response item with role `user` to `prompt.submitted`
  with its text (`cli/beacon/internal/codexsession/mapper.go@v1.3.29` L154-168,
  L291-296).

**What this settles (CONFIRMED at the tag):**

1. If Claude Code reports a channel delivery through `UserPromptSubmit`, through its
   `claude_code.user_prompt` OTLP event, or as a non-meta `user` entry in its session
   file, Beacon records the text verbatim as `prompt.submitted`, subject only to its own
   redaction and truncation (`SECURITY.md@v1.3.29` L52-57).
2. Independent of item 1's open half, OAC message content reaches Beacon's
   `runtime.jsonl` on the **outbound** side. A Claude session that sends through an OAC
   MCP tool is recorded with the tool's arguments by the `mcp__.*` `PreToolUse` hook,
   by OTLP tool details, and by the poll mapper. A harness's own reply is recorded as
   `agent.message`. A tool result is recorded as well, so a message read back through an
   OAC tool is captured on the poll path.
3. The same holds for Codex: any turn input Codex logs as `codex.user_prompt`, or writes
   to its session file as a `user` message, is recorded as `prompt.submitted`.

**What stays open (UNVERIFIED — harness behaviour, not Beacon's):**

- Whether Claude Code fires `UserPromptSubmit`, emits `claude_code.user_prompt`, or
  writes a non-meta `user` entry for a `notifications/claude/channel` delivery, or
  represents it some other way (for example as an attachment or a meta entry, which the
  poll mapper skips). No first-party Claude Code statement was found. G1 and G5's
  fixtures are JSON-RPC wire transcripts, not Claude session files, so they do not answer
  it.
- Whether Codex logs `codex.user_prompt`, or writes a `user` message to the rollout, for
  input an app-server client sends with `thread/queue/add` or `turn/start`.

**Input to L4 (row 23's residual-risk cell).** Treat OAC message content as **captured
into local `runtime.jsonl`** when Beacon runs beside an OAC session. The outbound half is
confirmed above. The inbound half is unverified but likely enough that the conservative
reading is the safe one. §4 Q3's Local / Metadata-only recommendation limits forwarding,
not this local capture.

### Item 2 — Memory item ID shape and memory-tool result shape → CONFIRMED (no versioned schema for summaries)

- **ID shape.** An approved memory ID is `memory_` followed by 32 lowercase hex
  characters: the first 16 bytes of a SHA-256 over the project ID, candidate ID, kind and
  title joined with NUL bytes (`MemoryID` and `idFor`,
  `cli/beacon/internal/learning/store.go@v1.3.29` L694-701). Candidate IDs use the same
  scheme with prefix `candidate_` (L686-692).
- **Stability.** The ID is content-derived and deterministic, not random. It is stable
  for as long as its inputs are. The project ID is
  `sha256:` + hex SHA-256 of the lower-cased remote URL, or the path when no remote exists
  (`store.go@v1.3.29` L199-206). A repository whose remote URL changes therefore gets a
  new project ID, and memories approved afterwards get IDs under it. A superseded memory
  keeps its ID but has `superseded_by` set (`cli/beacon/internal/learning/candidate.go@v1.3.29`
  L235-251). `get_memory` then answers with an error, `memory not found: <id>`
  (`cli/beacon/internal/mcpserver/server.go@v1.3.29` L338-357). An ID quoted in an OAC
  message can therefore go stale. With Q1 = (a), OAC never resolves it; L5's walkthrough
  should say so.
- **Result shape.** Every tool result is one MCP `text` content item holding the JSON
  encoding of the value (`server.go@v1.3.29` L245-257).
  - `get_memory` returns the full stored record, `LearningMemoryV1`: `schema_version`,
    `id`, `candidate_id`, `kind`, `title`, `body`, `applicability`, `tags`, `project`,
    `evidence`, `created_at`, `updated_at`, `superseded_by`
    (`pkg/asymptoteobserve/learning.go@v1.3.29` L108-122). Its `schema_version` is
    `beacon.learning.v1` (same file L3, set in `PutMemory`, `store.go@v1.3.29`
    L569-572).
  - `get_memory_context` returns `{"context": [...], "returned": n, "limit": n}` with at
    most 5 entries. `search_memory` returns `{"memories": [...], "returned": n,
    "limit": n}` with at most 20. Each entry is a summary with `id`, `kind`, `title`,
    `applicability`, `body` (cleaned and cut to 1200 characters) and `tags`
    (`server.go@v1.3.29` L80-99, L318-378, L419-431).
- **Versioned response schema?** **Partly.** `get_memory`'s record carries
  `schema_version` = `beacon.learning.v1`. The `search_memory` / `get_memory_context`
  wrappers and summaries carry no version field. No Beacon doc at the tag promises either
  shape across releases. The summary shape is a Beacon implementation detail. Since OAC
  never parses these results (Q1 = (a)), none of this is an OAC dependency.
- **Local store schema.** `memory.db` tables `evaluations`, `candidates`, `memories`,
  with `PRAGMA user_version` = 1. A Beacon build refuses a newer store: "memory store
  schema %d is newer than this beacon supports (%d)" (`store.go@v1.3.29` L19-22,
  L94-157).

### Item 3 — Concurrent access to one `memory.db` → CONFIRMED (SQLite WAL with a 5 s busy timeout, in source only; no documented model)

- **Storage engine.** SQLite, through the pure-Go driver `modernc.org/sqlite` `v1.59.0`
  (`cli/beacon/internal/learning/store.go@v1.3.29` L14; `cli/beacon/go.mod@v1.3.29`
  L14).
- **Locking model, from source.** Every store operation opens its own connection and
  closes it when done. On open, Beacon runs `PRAGMA journal_mode=WAL; PRAGMA
  busy_timeout=5000;` and its connection URI also carries `_pragma=busy_timeout(5000)`
  (`store.go@v1.3.29` L65-92). Each write is one `INSERT ... ON CONFLICT(id) DO UPDATE`
  statement (e.g. `PutMemory`, L569-595). `store.go` opens no explicit transaction (no
  `Begin` / `Tx` in the file). Multi-step writes are therefore not atomic: supersede
  reads the memory with `GetMemory` and writes it back with `PutMemory`
  (`cli/beacon/internal/learning/candidate.go@v1.3.29` L243-251), so two concurrent
  operator writes to the same memory can lose an update. MCP readers are unaffected. Each open also runs the idempotent
  `CREATE TABLE IF NOT EXISTS` set (L94-157).
- **Who writes.** The MCP memory tools are read-only (`docs/cli/memory.mdx@v1.3.29`
  L265-271; the three handlers call only `ListMemories` / `GetMemory`,
  `server.go@v1.3.29` L318-378). Several harness sessions resolving memory at once are
  therefore concurrent **readers**, apart from the idempotent schema check each open
  runs. Writes come from operator-run `beacon memory`
  commands (evaluate, create, approve, supersede).
- **Not documented.** No Beacon doc at the tag states a concurrency or locking model.
  `memory.mdx` calls the store "durable local state" (L28-31) and says nothing more.
  The model above is read from code, so it can change in any release without notice. It
  is re-read whenever the PINS.md row moves.
- **OAC consequence.** None. OAC never opens `memory.db` (§1). The item matters only to
  L5's docs and L10's opt-in scenario. Neither needs to say more than "Beacon's store is a local
  SQLite file in WAL mode that several harness sessions may read".

### Item 4 — Do Beacon's config writes collide with OAC's launch paths? → REFUTED (no shared key; two side effects recorded)

**OAC's planned launch paths** (`docs/planning/v0.1/08-cli-and-deployment.md`):

- **Claude (§7).** `claude --dangerously-load-development-channels server:oac`. This is
  a command-line flag typed by the user. It writes no file. It needs an MCP server named
  `oac` in Claude Code's MCP configuration; §7 does not name the file, and G1 used a
  project `.mcp.json` (`docs/planning/gates/G1-result.md`).
- **Codex (§9).** `codex mcp add oac -- oac mcp-shim`: Codex's own CLI registers a server
  named `oac` in Codex's configuration. The live-inject path (§10) is app-server runtime
  behaviour, not a config write.

**What Beacon writes, at the tag:**

| Beacon command | File | What it changes | Can it touch OAC's entry? |
|---|---|---|---|
| `beacon endpoint install` (Claude) | `~/.claude/settings.json` | Sets 8 keys in the `env` object (telemetry and OTLP exporter settings, including `OTEL_LOG_USER_PROMPTS`). Backs the file up, then re-encodes the whole file with Go's `json.MarshalIndent` (`harness.go@v1.3.29` L359-397) | **No.** It changes only those `env` keys. It never writes MCP server entries |
| `beacon endpoint hooks install` (Claude) | `~/.claude/settings.json` (user) or `./.claude/settings.json` (project) | Removes Beacon's own hook entries, then merges its hook groups for the 10 events. Non-Beacon hooks are kept (`cli/beacon/internal/endpoint/hooks/settings_hooks.go@v1.3.29` L191-205; path from `docs/cli/hooks.mdx@v1.3.29` L33) | **No.** OAC's plan installs no Claude hook |
| `beacon endpoint install` (Codex) | `~/.codex/config.toml` | Replaces the `[otel]` table and every `[otel.*]` table with Beacon's block. Every other line under a header it recognises is copied through unchanged (`mergeCodexOTELWithPrompt`, `harness.go@v1.3.29` L464-503). **Edge case:** the merge is line-based and treats a line as a table header only when the trimmed line starts with `[` **and** ends with `]` (L471-485). A header with a trailing comment, e.g. `[mcp_servers.oac] # oac`, directly after an `[otel]` section is therefore not recognised: it and its keys are dropped until the next recognised header | **No, on OAC's planned path**: OAC registers `oac` with `codex mcp add`, not by hand-editing, and a CLI-written header carries no trailing comment (not source-checked against Codex; §12 B1 checks it). A hand-edited `oac` header with a trailing comment placed after `[otel]` would be lost; §12 B1 checks for it |
| `beacon endpoint hooks install --harness codex` | `~/.codex/hooks.json` or `./.codex/hooks.json` | One `SessionStart` hook, merged the same way (`hooks/codex.go@v1.3.29` L63-73; `docs/cli/hooks.mdx@v1.3.29` L34, L449) | **No.** OAC's plan installs no Codex hook |
| `beacon mcp connect` (explicit, opt-in; `endpoint install` never runs it) | `~/.claude.json` via `claude mcp add --scope user`; `~/.codex/config.toml` | Adds exactly one server named `beacon-managed`, as a text edit. It re-parses the result and writes nothing if any other byte would change (`docs/cli/mcp-connect.mdx@v1.3.29` L32-35, L100-107, L176-179) | **No.** It uses a different name. Out of Epic L's scope anyway (§1) |
| `beacon mcp serve` config printed by `beacon mcp doctor` | none; the operator pastes it | A stdio server named `beacon` (`docs/cli/mcp-connect.mdx@v1.3.29` L19-26) | **No.** It uses a different name |

**Verdict.** On OAC's planned launch paths, no Beacon write at the tag touches a server
named `oac`, the `--dangerously-load-development-channels` flag, or the app-server. The
hypothesised collision is refuted for those paths. The one exception is the
trailing-comment header edge case in the table above, which OAC's documented
`codex mcp add` path is not expected to produce (§12 B1 checks it live).

**Paths Beacon writes, whatever the environment says.** `ConfigureClaude` always writes
`$HOME/.claude/settings.json` and ignores `CLAUDE_CONFIG_DIR` (`harness.go@v1.3.29`
L364). `ConfigureCodex` always writes `$HOME/.codex/config.toml` and ignores
`CODEX_HOME` (L404). An operator who runs Claude Code or Codex with either variable set
gets Beacon's telemetry settings in a file that session may not read.

**Two side effects recorded as findings, not collisions:**

1. **Codex prompt logging applies to OAC-launched Codex too.** The `[otel]` table Beacon
   writes (`log_user_prompt = true`) is user-level. It therefore applies to every Codex
   process that reads that `~/.codex/config.toml`, including an app-server OAC's Codex
   adapter talks to under the same Codex home. This feeds item 1's Codex half and L4's
   residual. It is Beacon's behaviour at the tag, not an OAC defect.
2. **Formatting churn in `~/.claude/settings.json`.** The Claude telemetry write
   re-encodes the whole file, so key order and formatting change. The write is
   `os.WriteFile(path, data, 0600)` (`harness.go@v1.3.29` L396): the 0600 mode applies
   when Beacon creates the file; an existing file keeps its mode. Content outside `env`
   is kept when the file parses. If it does not parse, the parse error is ignored and the
   file is rewritten holding only Beacon's `env` block; the backup taken just before is
   then the only copy of the old content (`harness.go@v1.3.29` L365-371).
   `beacon mcp connect`'s byte-preserving guarantee does **not** extend to this write.
   OAC's plan does not write this file, so nothing of OAC's is lost. An operator diffing
   the file should expect the churn.

Anything that replaces a user's pre-existing `[otel]` table is the operator's concern
between Beacon and Codex. OAC's plan writes no `[otel]` table.

**Cross-file updates in L2's change.** `docs/planning/STATUS.md`: a `**Last updated:**`
entry; the L1 "Open UNVERIFIED items" entry narrowed to U1, with U2-U4 recorded as closed
by this section; the `## Pins` summary cell. `docs/planning/v0.1/11-risks.md`: rows 54-56
marked CLOSED with citations, row 53 narrowed, one sentence in the traceability preamble.
`docs/planning/PINS.md`: the Beacon row's UNVERIFIED count and one note in its record.
`docs/planning/v0.1/06-security.md` §14 row 23: the residual cell now carries item 1's
finding (L4 merged first, so this lands as a follow-up edit).
After L5 and L7 merged, the review round also updated stale U1-U4 wording in
`docs/planning/v0.1/08-cli-and-deployment.md` §20, `07-repository-and-dependencies.md`
§5/§9 and `11-risks.md` `RISK-BEACON`, and pointers in
`.claude/skills/oac-boundaries/references/beacon.md` (the one edit outside
`docs/planning/`, made at review request).
No pin value, gate verdict, skill, spec or ADR text changes. No Beacon code, binary,
fixture or spike code is committed.

## 12. Live-verification checklist (herdr-driven, NOT RUN)

**Note 2026-10-01 (L3d, #192): this checklist has run.** The herdr-driven leg ran on
Windows against Beacon `1.3.29`, Claude Code `2.1.285` and Codex `0.159.3`. Its results are
in §13: B0 and B2-B4 recorded, B1, B5 and B6 NOT RUN by operator decision, B7 hash-checked
with Beacon kept installed. The heading and the text below are kept as written, as the
specification the run followed.

Every step below is **UNVERIFIED — pending a live run**. Backlog task L3 ("Beacon live
leg (herdr-driven)", `docs/planning/backlog/07-tasks-L.json`) executes it. That key reuses
#165's closed spec-touchpoint number (§4 Q1).

**Amended 2026-09-30 (operator decision, issue #187).** This section first said the leg
was "operator-run only", needed "a person at the keyboard", and that no `tools/herdr/`
scenario might drive it. That was wrong: herdr was added to automate exactly this kind of
work. **The live leg is driven by a herdr scenario (`node tools/herdr/run.mjs`), run
locally by an agent** on a real machine with Beacon installed and Claude Code and Codex
logged in. The operator's role is reduced to what only a human may do: installing Beacon
(elevation), signing in to the harnesses, and accepting operator-consent dialogs such as
Claude Code's development-channels warning. A driver-sent accept of such a dialog is never
verdict-bearing (`oac-gates` `references/scripted-runs.md` "Operator-consent dialogs").
B1 (Beacon install) and B7 (`beacon endpoint uninstall` and restoring the B0 backups onto
harness config files) are operator steps, outside the driver. The driver only reads and
hashes those files (`oac-boundaries` mechanical check 10). The leg is **not in the
default CI suite**, which has no logged-in harness (`oac-testing` §2).

**Amended 2026-09-30 (operator decision, issue #196).** The consent-dialog part of the
paragraph above is superseded. L3 is dev/test automation and not a gate, and no L3 step
is a consent criterion. So `l3-beacon` accepts Claude Code's workspace-trust, project
MCP-server and development-channels dialogs itself by default (`accept=driver`). It reads
each dialog verbatim first and records the accept as `driver`. The §13 draft
(`tools/herdr/lib/l3-report.mjs`) states it that way and never as a human accept.
`accept=human` remains available. Evidence of need: probe run 3 of 2026-09-30 ended
`NOT RUN` because nobody accepted the workspace-trust dialog within 300000 ms, and that
dialog preselects "No, exit". Rule and record:
`docs/planning/decisions/K-196-driver-accepts-dialogs.md`.

OAC never calls, spawns or configures Beacon in any step (§1). The operator installs and
connects Beacon only with Beacon's own documented commands. A test driver that launches
the harnesses beside that operator-installed Beacon and reads Beacon's own local log and
read-only CLI output is test tooling, not OAC product behaviour. The driver never reads
harness credentials and never runs herdr's hook-writing command (`oac-boundaries`
mechanical checks 9 and 10).

**Live observations, Windows, Beacon `1.3.29` (2026-09-30).** Source: the MSI's own
install script, `C:\Program Files\Beacon\scripts\install-endpoint.ps1`, and the resulting
machine state. Observed on Windows only; not checked on Linux or macOS.

- The Windows MSI runs a **system-mode** install itself:
  `beacon endpoint install --system --harness claude,codex`, then
  `beacon endpoint user-config repair-installed --system --harness claude,codex` (script
  L76; `claude,codex` is the script's default harness list). It shows no Local/Managed
  prompt, and it installs Beacon's harness hooks as well as its telemetry settings. The
  collector runs as the `BeaconCollector` service under LocalSystem.
- The first pass configures the account that ran the installer (an admin, or SYSTEM under
  a management tool); the second resolves and configures the interactive user (script
  comment L65-68). B0 therefore baselines the interactive user's files.
- The runtime log is `C:\ProgramData\Beacon\Endpoint\logs\runtime.jsonl`, not
  `~/.beacon/endpoint/logs/runtime.jsonl`. Use `beacon endpoint status --system` to
  confirm the path on the machine. Run it elevated to see the service state: unelevated,
  it reports `Access is denied` for the service even while the service runs. It also
  reports `Beacon Managed: not connected`, i.e. no hosted forwarding.
- Beacon's own `.beacon.bak` backups can be overwritten by the MSI's second pass
  (`user-config repair-installed`). Do not rely on them for B7; B0's own backups outside
  the repo are the restore source.

Consequences for the steps below on Windows: B1's "run `beacon endpoint install`, choose
Local" does not apply as written, because the MSI has already installed; B1 instead
records what the MSI changed against B0. `status --system` reported `Beacon Managed: not
connected` (observed 2026-09-30). B2 and B4 grep the `C:\ProgramData` log path.

**Note 2026-09-30 (L3b, #190): the herdr scenario.** `tools/herdr/scenarios/l3-beacon.mjs`
implements B0 (`--param phase=baseline`), B2-B4 (`phase=probe`, which also records B1 as
`NOT RUN` by the operator decision on #168) and B7's hash check (`phase=verify`). One 60-minute
box spans the three phases, declared by the baseline. The markers and fake tokens are
generated inside the scenario, searched for by their full value (never the `L3-PROBE-` or
`sk-l3fake-` prefix the greps below use), and recorded as hashes only. The "Probe content"
scratch copy of `tools/herdr/gate-servers/` is the scenario's own scratch directory, and only
that copy's `g5-cases.json` is augmented. The greps and `sync --print` counts below are done
in-process, and `sync --print` is run only after confirming from source at `v1.3.29` that it
writes nothing (citations in the scenario header). B5 and B6 are not automated. The steps
below are unchanged and remain the specification; the scenario is UNVERIFIED until #192 runs
it live.

**Timebox: 60 minutes**, declared before B0 and not extended. When it expires, write
the results into `## 13. Live results (L3)` in this file as-is. A step not reached is
recorded `NOT RUN` with the reason, never guessed.

**Probe content.** There is no OAC product yet, so delivery uses the Stage 1 pattern.
Copy `tools/herdr/gate-servers/` (`g5-channel.mjs`, `g5-codex.mjs`, `g5-cases.json`) to a
scratch directory **outside the repo**. In the copy's `g5-cases.json`, add one Claude case
and one Codex case whose body carries a unique marker (`L3-PROBE-<random>`) and a fake,
secret-shaped token (for example `sk-l3fake-<random>`, never a real one). Both tools read
case bodies only from that file. Never commit the modified copy.

**Redaction rules** (same spirit as `docs/planning/decisions/K1-herdr-evaluation.md` §6
and `oac-gates` §"Fixture capture procedure"):

- Commit no raw `runtime.jsonl` lines, session files, `settings.json`, `.claude.json`,
  `config.toml` or `hooks.json`. They hold prompts, paths, usernames and other servers'
  credentials. **No fixture file** is committed: `docs/planning/gates/fixtures/` is
  reserved for the G1-G5 and D6 slugs.
- Record field names, `event.action` values and hashes. Inline excerpts in §13 are short
  and redacted: home paths become `~`, usernames `<user>`, account IDs `<account>`, and
  anything token-shaped other than the fake probe token becomes `<redacted>`.
- Stay in Beacon **Local** mode (§4 Q3). Do not run `beacon endpoint connect` or
  `beacon mcp connect`, and do not sign in to Beacon's hosted service.

**B0 — Preflight and baselines**

```bash
beacon version                          # must report version 1.3.29; stop otherwise (§2)
claude --version; codex --version       # record both (floating pins, PINS.md)
# Beacon writes these fixed paths whatever CLAUDE_CONFIG_DIR / CODEX_HOME say
# (harness.go@v1.3.29 L364, L404; §11 item 4)
sha256sum "$HOME/.claude/settings.json" "$HOME/.claude.json" "$HOME/.codex/config.toml" "$HOME/.codex/hooks.json" 2>&1 | tee hashes-before.txt
env | grep -E '^(CLAUDE_CONFIG_DIR|CODEX_HOME)=' | sed 's/=.*/=<set>/'   # record only whether set
```

If `CLAUDE_CONFIG_DIR` or `CODEX_HOME` is set, the harness may read a different file
than the one Beacon writes: record that as a finding and also hash the harness's own
files. Also copy the four files to a backup directory outside the repo. Evidence to record:
the three versions, `hashes-before.txt` (hashes only, paths redacted), the timebox start
time, and confirmation that Beacon package self-updates are off (§2).

**B1 — Install Beacon's endpoint (item 4)**

Run `beacon endpoint install`, choose Local, and include the `claude` and `codex`
harnesses. Hash the four files again and diff each against its B0 backup.

Before installing, check the edge case from §11 item 4: whether `config.toml` has any
table header with a trailing comment (`grep -nE '^\s*\[[^]]*\]\s*#' ~/.codex/config.toml`)
and whether one follows an `[otel]` section. If so, confirm after install whether that
table and its keys survived.

Evidence to record: which files changed. For `config.toml`, whether any line outside
`[otel]` / `[otel.*]` changed, and the result of the trailing-comment check. For `settings.json`, whether anything outside `env` and
`hooks` changed in content, not just in formatting. For `.claude.json`, whether it
changed at all. Attribute each change to this Beacon step. Any change beyond §11 item 4's
table is a finding.

**B2 — Claude channel delivery (item 1, Claude half)**

Register the copied `g5-channel.mjs` in a scratch project's `.mcp.json` and launch Claude
Code with the development-channel flag, as G1 did (`docs/planning/gates/G1-result.md`).
Confirm the development-channel dialog appears and the server connects, which also checks
item 4 live for the Claude launch. Trigger the probe case. Wait for the turn to finish.

```bash
grep -c 'L3-PROBE-' ~/.beacon/endpoint/logs/runtime.jsonl
grep -c 'sk-l3fake-' ~/.beacon/endpoint/logs/runtime.jsonl
beacon endpoint claude sync --print | grep -c 'L3-PROBE-'
```

Evidence to record: for each marker hit, its `event.action`,
`harness.collection_method`, and which field held it (prompt text, tool arguments,
agent message, tool result). Whether the fake token appears unredacted. How Claude's
session file represents the delivery: a `user` entry, a meta entry or an attachment
(entry type and flags only, no content). Zero hits on a path is a result.

**B3 — Claude outbound capture (item 1, the confirmed half)**

Have the B2 session answer through the channel server's reply tool with the marker in
the reply. Repeat B2's greps.

Evidence to record: `mcp.tool_invoked` / `tool.invoked` hits and whether their
arguments carried the marker. This checks §11 item 1 point 2 live.

**B4 — Codex app-server input (item 1, Codex half; item 4 for Codex)**

With the shared Codex daemon running (G2 setup, `docs/planning/gates/G2-result.md`),
deliver the Codex probe case with the copied `g5-codex.mjs`: once with `deliver`
(`turn/start`) and once with `x4` (which sends `thread/queue/add` while a turn runs).
Confirm the thread took both inputs, which also checks item 4 live for the Codex path.

```bash
grep -c 'L3-PROBE-' ~/.beacon/endpoint/logs/runtime.jsonl
grep -c 'sk-l3fake-' ~/.beacon/endpoint/logs/runtime.jsonl
beacon endpoint codex sync --print | grep -c 'L3-PROBE-'
```

Evidence to record: hits per method and per path (OTLP versus poll), each with
`event.action`, and whether the fake token appears unredacted. Zero hits is a result.

**B5 — Memory candidates (optional, only if time remains)**

Run Beacon's own `beacon memory` evaluation on the B2 trace
(`docs/cli/memory.mdx@v1.3.29` §"Evaluate traces"). Evidence to record: whether any
candidate's text contains the marker. Do not approve a candidate.

**B6 — Concurrent memory reads (item 3, live confirmation only)**

With one approved memory in a scratch repository, have the Claude and the Codex session
call `get_memory_context` at the same time, three times, while `beacon memory list` runs
in a third pane or terminal, the driver's or the operator's
(`docs/cli/memory.mdx@v1.3.29` L220-229).

Evidence to record: any error text (for example `database is locked`), and whether all
six calls returned. §11 item 3 is already CONFIRMED from source. A lock error here is a
finding against that record.

**B7 — Tear down**

The operator runs `beacon endpoint uninstall` if the machine should not keep Beacon, and
restores the B0 backups where files differ (operator steps; the driver writes no harness
config file, check 10). Hash the four files and compare with `hashes-before.txt`.
Delete the scratch copy of the gate servers.

Evidence to record: the hash comparison. If a file could not be restored, say which.

**Exit.** Write `## 13. Live results (L3)`: one line per step (PASS / FINDING /
NOT RUN) with the recorded evidence, the Beacon, Claude Code and Codex versions, and the
date. In the same change, update item 1's verdict in §11 (and item 4's, if B1-B4 found
a collision), `docs/planning/STATUS.md` "Open UNVERIFIED items", and
`docs/planning/v0.1/11-risks.md` rows 53 and 56.

## 13. Live results (L3)

**Issue:** #192 (L3d), parent #168 (backlog `L3`, Epic L #165). Drafted by
`tools/herdr/lib/l3-report.mjs` (L3c, #191; leak guard passed, print-only) from the three
verdict-bearing phase runs below, then reviewed and edited by hand. Excerpts were re-read
after editing: no credential, home path, username or account ID; the fake probe token and
the marker appear only as the report's placeholders, never as values. No raw
`runtime.jsonl` line, session file or harness config file is committed, and there is no
fixture file. This is not a gate result and changes no verdict (PINS.md Beacon row:
`Gates affected: none`). Step results are PASS / FINDING / NOT RUN. A phase run's own
outcome (`PASS` below) only means the scenario ran to the end; it is not a step result.

- **Date:** 2026-10-01. Machine: Windows 11 (`win32`, `10.0.26300`, x64).
- **Beacon:** `1.3.29` (`beacon version`, read-only allowlist), Windows MSI, system-mode
  install (§12 2026-09-30 observations), **Local** mode: no `beacon endpoint connect`, no
  `beacon mcp connect`, no hosted sign-in. Self-updates off (§2). Matches the §2 pin.
- **Claude Code:** `claude --version` `2.1.285`. PINS.md last observed `2.1.283`: pin
  drift, recorded as a finding (operator decision 4 on #168, 2026-09-30; PINS.md not
  moved). Channel handshake: `initialize` requested and negotiated `2025-11-25`, channel
  capability present, permission capability absent.
- **Codex:** CLI `0.159.3`; app-server daemon `running`, `cliVersion` / `appServerVersion`
  / `managedCodexVersion` `0.159.3`; wire `userAgent` `0.159.3`. PINS.md last observed
  `0.157.1`: pin drift, recorded as a finding (same decision).
- **Driver:** herdr `herdr 0.9.1` (PINS.md `herdr (test tooling)` `v0.9.1`) via
  `node tools/herdr/run.mjs`, scenario `tools/herdr/scenarios/l3-beacon.mjs`, driver commit
  `2c6f6af607158ed6c0bddc9f63b4615fa89a9233`, `driver.toolsHerdrDirty` false, Node
  `v25.2.1`. Run by an agent from inside a Claude Code session; the driver removed 27
  inherited session variables before launching the harnesses (#163).
- **Dialog accepts:** `accept=driver` in all three phases (operator decision #196,
  `docs/planning/decisions/K-196-driver-accepts-dialogs.md`). Four dialogs, each read
  verbatim first and accepted by the **driver**, none by a human: Claude workspace-trust
  (read #11; `down` #12, `enter` #14), Claude project MCP-server approval (read #17; `up`
  #18, `up` #20, `enter` #22), Claude development-channels (read #25; `enter` #26), Codex
  workspace-trust (read #47; `enter` #48). Codex's startup hook review (Beacon's
  `SessionStart` hook, #204) did not appear: the operator had trusted that hook before
  B0 (operator statement; not in the run record).
- **L3 box:** declared by the baseline run at 2026-10-01T19:52:44.821Z; 60 minutes, never
  extended; would end 20:52:44.821Z. The last phase ended 19:54:50.886Z; **not expired**.
- **Phase runs** (each outcome `PASS`):
  - baseline (B0): `20261001T195242Z-72d47c`, 19:52:42.783Z to 19:52:48.655Z
  - probe (B1 record, B2-B4): `20261001T195249Z-5982fd`, 19:52:49.346Z to 19:54:24.284Z
  - verify (B7 hash check): `20261001T195449Z-c25113`, 19:54:49.979Z to 19:54:50.886Z

### Results, one line per step

- **B0:** FINDING — preflight done, four config files hashed; the findings are pin drift
  (Claude Code, Codex) and the false-positive "Beacon Managed" check (#209).
- **B1:** NOT RUN — operator decision 2 on #168 (2026-09-30): Beacon was installed before
  any B0, and it is not uninstalled and re-installed. Evidence is the §12 2026-09-30 MSI
  observations plus the weaker `.beacon.bak` evidence (the driver does not read it).
  Supporting only: all four files unchanged from B0 to probe start; zero trailing-comment
  table headers in `config.toml`.
- **B2:** FINDING — Claude Code channel delivery **is captured**: `prompt.submitted` in
  Beacon's local log (hook and OTLP), with the fake token **unredacted**. Poll path: 0 hits.
- **B3:** FINDING — the reply-tool call is captured as `tool.invoked` and
  `mcp.tool_invoked` with the marker in its arguments (§11 item 1 point 2 confirmed live).
- **B4:** FINDING — Codex input sent with `turn/start` **and** with `thread/queue/add` is
  captured: `prompt.submitted` via OTLP and on the poll path, fake token **unredacted** on
  both.
- **B5:** NOT RUN — default in the operator decisions on #168 (2026-09-30): evaluating
  traces is an operator-only Beacon step, not automated.
- **B6:** NOT RUN — same decision: it needs Beacon's MCP server registered in both
  harnesses and an approved memory, operator-only Beacon steps.
- **B7:** FINDING — Beacon kept installed by operator decision, so no uninstall or
  restore. Two files differ from B0:
  - `config.toml`: Codex's own folder-trust entry (section diff);
  - `~/.claude.json`: attributed by inference only to Claude Code's trust write.

  Details below.

### Evidence

**B0.** Hashes (sha256 of the file; labels only, paths not recorded):

- `~/.claude/settings.json`: `70754e478f634d201457029e2c51d0dac3947d326002b31e098c371051fb9af3`
- `~/.claude.json`: `7f8c6083bab0a8e1471e797f669d6198b36d48b2007d1827261c9c5afeabcfcb`
- `~/.codex/config.toml`: `5376ce47a582c1ad7fff2ed03bf75ad90f36347510f430497abb3b4698b22588`
- `~/.codex/hooks.json`: `190a0ca24dd0fe9a4b3b2bc232915064a53ae4ec6b9d52bf8924d49255866f93`

`CLAUDE_CONFIG_DIR` and `CODEX_HOME` not set, so the harnesses read the files Beacon
writes (§11 item 4). `config.toml` has no table header with a trailing comment, so §11
item 4's edge case cannot arise on this machine.

**B2 (Claude, inbound).** The probe case was triggered at 19:53:10.839Z. Beacon's log
(`runtime.jsonl`, 108 lines scanned from the probe start) holds **6** lines with the
claude-channel marker, all in one `claude_code` session:

- At the B2 snapshot, before B3: **2** lines, both `prompt.submitted`. The marker sits in
  `prompt.text` and `gen_ai.input.messages[0].parts[0].content` (both lines) and
  `raw.attributes.prompt` (one line).
  The fake token is **unredacted** in both lines.
- Across all 6 hits the collection methods are `hook` (3) and `otlp` (3). The record does
  not pair a method with each line, except at the B2 snapshot, where the two
  `prompt.submitted` lines are `hook` 1 and `otlp` 1.
- Poll path (`beacon endpoint claude sync --print`, 40714 event lines): **0** hits at B2.
  This is a result. Claude Code's session file holds the delivery as a `queue-operation`
  entry and a `user` entry with `isMeta` `true` (entry types and flags only; no
  attachment). Beacon's poll mapper skips `isMeta` entries (§11 item 1), which explains
  the zero.

Redacted excerpt, one of the two `prompt.submitted` lines (Beacon log line 2872; a
delimiter nonce cut off): `oac_message_id=\"l3-c\" oac_reply_to=\"\">\nL3 probe
{L3-MARKER claude-channel}. Test credential {L3-FAKE-TOKEN claude-channel}.`

**B3 (Claude, outbound).** The session answered through the channel server's reply tool
once, with the marker in its arguments; the fake token was not in them (checked
in-process). The B3 snapshot adds 2 lines, `tool.invoked` and `mcp.tool_invoked`, with the
marker at `gen_ai.tool.call.arguments.message`. Later lines add `mcp.tool_invoked`
(`raw.attributes.tool_input`) and `session.activity` (the reply text, at
`gen_ai.output.messages[0].parts[0].content`). Poll path at B3: 3 lines
(`mcp.tool_invoked`, `tool.completed`, `agent.message`), token not present. Redacted
excerpt (line 2917): `"tool":{"call":{"arguments":{"in_reply_to":"l3-c","message":"{L3-MARKER
claude-channel}"},"id":"<redacted>"},"name":"repl`.

**B4 (Codex).** The Codex TUI was attached to the shared daemon, and the driver's
app-server client delivered both cases to the thread the TUI loaded. `turn/start` (case
`L3X`, 19:54:00.870Z) and `thread/queue/add` while a turn ran (case `X4`, 19:54:05.078Z):
both turns `completed`, each input recorded byte-identical in the thread. The Codex launch
and both inputs worked with Beacon's `[otel]` table and `SessionStart` hook in place, so no
item 4 collision was seen live for Codex.

- OTLP path (Beacon log): 1 `prompt.submitted` line per method, harness `codex_desktop`
  (inferred: Beacon's label for app-server OTLP), collection method `otlp`. Paths: `prompt.text`,
  `gen_ai.input.messages[0].parts[0].content`, `raw.attributes.prompt`. Fake token
  **unredacted** in both.
- Poll path (`beacon endpoint codex sync --print`, 2299 event lines): 1 `prompt.submitted`
  line per method, harness `codex_cli`, fake token **unredacted** in both.
- Redacted excerpt (line 2951): `oac_reply_to: \n--- oac-body <nonce> ---\nL3 probe
  {L3-MARKER codex-turn-start}. Test credential {L3-FAKE-TOKEN codex-turn-start}.`

**B7.** Baseline vs. verify:

- `~/.claude/settings.json`: unchanged.
- `~/.codex/hooks.json`: unchanged.
- `~/.codex/config.toml`: changed. One table added, `[projects.<sha256:4bcfac99a97dd3bd>]`
  (the label hashes the scratch project path), and no table changed or removed. This is
  Codex's own trust entry, written when the driver accepted its folder-trust dialog (#206).
  It is not a Beacon write and it is outside §11 item 4's table. The `[otel]` table was not
  touched.
- `~/.claude.json`: changed (hash only; the record has no section diff for this file).
  Attributed to Claude Code recording the workspace-trust accept for the scratch project
  in its own state file. That is an inference from the timing only:
  - the content was not read;
  - the hash is known only at probe start (unchanged) and at verify (changed);
  - the top-level key set is unchanged.

  An alternative cause is that the orchestrating Claude Code session, which ran as the
  same user throughout, also writes this file. Beacon writes this file only through `beacon mcp connect`, which was not run.
- Restore: none needed beyond optional pruning of the scratch trust entries (#206). The
  operator keeps the B0 backups.

### Findings

1. **U1 answered: capture is confirmed in both harnesses** (B2-B4). OAC-delivered input
   reaches Beacon's local `runtime.jsonl` **verbatim**, and the secret-shaped fake token
   was **not redacted** on any path that captured it: Claude hook/OTLP, Codex OTLP and the
   Codex poll path. Beacon's own redaction (§11 item 1, `SECURITY.md@v1.3.29` L52-57) did
   not catch a token of this shape. This feeds `docs/planning/v0.1/06-security.md` §14
   row 23.
2. **Claude Code's poll path misses channel deliveries.** The session file marks the
   delivery `isMeta` `true`, which `beacon endpoint claude sync` skips. Capture of Claude
   inbound therefore depends on Beacon's hooks and OTLP settings being installed.
3. **MSI install** performs a system-mode install itself (§12 2026-09-30 observations;
   re-confirmed, no new data).
4. **Beacon's Codex `SessionStart` hook blocks unattended Codex until trusted.** Codex
   shows a startup hook review for an untrusted hook in `~/.codex/hooks.json`, and no
   `thread/start` happens until someone answers it. The driver never answers that screen
   (#204, fixed in #205 by a ready check that ends the run `NOT RUN` naming the blocker).
   The operator trusted the hook before this box (operator statement; not in the run
   record).
5. **The Codex daemon holds a handle on a loaded thread's cwd on Windows.** Scratch cleanup
   failed with `EPERM` (#202). Since #203 the run manifest survives the failure. In this
   probe the scratch directory was again left behind, possibly with unredacted captures.
   The operator deletes it by hand once the daemon releases it.
6. **Harness trust entries.** Accepting folder-trust dialogs makes Claude Code
   (`~/.claude.json`) and Codex (`~/.codex/config.toml` `[projects.…]`) record trust
   entries, one per scratch project (#206). That accounts for the `config.toml` difference
   (section diff: one `[projects.…]` table added). The `~/.claude.json` difference is
   attributed to the same trust write by inference only (§13 B7):
   - its content was not read;
   - its top-level key set is unchanged;
   - the orchestrating Claude Code session, which writes the same file, was running
     throughout.
7. **`codex app-server daemon start` cannot be launched from Claude Code's shell on
   Windows.** The first probe failed on it (exit 1, below); a job object around that shell
   is the likely cause. The daemon was started outside it (WMI process creation;
   orchestrator statement, not in the run record) and was already running at this probe. The probe recorded `alreadyRunning`, which the scenario
   flags because a daemon started before Beacon's `[otel]` write may not export OTLP. Here
   the OTLP counts are non-zero, so that caveat did not bite, which implies the daemon had
   Beacon's `[otel]` config loaded.
8. **A stale operator MCP server blocked Codex startup.** An unreachable `mcp_servers.g4`
   entry was in the operator's `config.toml`. The operator removed it during the
   2026-09-30 attempts (operator statement; not in the run record). #205 later found the hook review, not MCP servers, to be the
   blocker in those runs.
9. **Pin drift:** Claude Code `2.1.283` → `2.1.285`, Codex `0.157.1` → `0.159.3`. Recorded
   by operator decision 4 on #168; PINS.md not moved; L3 is not a gate.
10. **False positive: "Beacon Managed".** The scenario flags "hosted forwarding may be on"
    because Beacon `1.3.29` prints `Beacon Managed: not connected (run \`beacon endpoint
    connect\`)` and the parser expects exactly `not connected`. The machine was in Local
    mode. Follow-up: #209. Fixed 2026-10-01 (#209): the scenario now accepts only an exact
    allowlist as Local mode: bare `not connected` and the live line's hint
    (`cli/beacon/cmd/endpoint_connect.go@v1.3.29` L291). Everything else is still a
    finding, because forwarding may be on. That includes the
    `cli/beacon/internal/endpoint/asymptote/status.go@v1.3.29` messages:
    - an enrollment-read error (L62, e.g. `Access is denied` unelevated);
    - "connect incomplete" (L64, L71). A first connect starts the forwarder
      (`connect.go@v1.3.29` L261-L267) before it records the enrollment (L290, L302), and
      only an error return stops it, so a forwarder may still run. Either message also
      means `beacon endpoint connect` was run, contrary to §4 Q3;
    - "re-connect incomplete" (L78);
    - "disconnected; credentials ... kept" (L101).

    So are `connected ...`, a missing line, and any other wording. The runs above predate
    the fix; their record is unchanged.
11. **Manifest redaction** withheld `$.scenario.params.baselineRun` in the probe and verify
    manifests: the value is a local scratch path.

### Prior attempts (never evidence)

All were on Windows with herdr `0.9.1` and Claude Code `2.1.285`, unless noted. None ran in
this L3 box, and none is a step result.

- **Manual attempt**, 2026-09-30 05:50Z (Claude Code `2.1.284`, recorded on #168): B0
  done, B1 the MSI finding (§12), paused at B2. The operator moved to a herdr-driven run.
- `20260930T114931Z-1ee506` (Codex `0.158.0`, `accept=human`): **FAIL** —
  `codex app-server daemon start` failed (exit 1) from the agent's shell (finding 7).
- `20260930T155917Z-d42593`: **NOT RUN** — it pointed at the first baseline, whose box
  had expired. The probe does not start.
- `20260930T155948Z-efd7ec` (`accept=human`): **NOT RUN** — Claude workspace-trust was
  not accepted within 300000 ms. That run led to #196.
- `20260930T172713Z-e73e65` (Codex `0.159.2`, `accept=driver`): **NOT RUN** — Codex
  workspace-trust had no option text on record; the driver refused it. Added to K-196 by
  #199.
- `20260930T184103Z-8a617f` and `20260930T200753Z-a4d484` (Codex `0.159.2`): **NOT RUN** —
  no loaded thread within 180000 ms, because of the hook review (finding 4, #204).
- Two probe runs at about 19:04Z and 19:09Z on 2026-09-30 aborted on scratch-cleanup
  `EPERM` before writing a manifest (finding 5, #202). Nothing is recorded from them.

### Operator attestation

Generated unticked. Only the operator who ran this machine ticks these lines, each only if
true (`.claude/skills/oac-gates/references/scripted-runs.md` "Operator attestation",
adapted to name Beacon).

- [ ] **herdr:** the real herdr binary ran, not a test double. `herdr --version`:
  `herdr 0.9.1`; sha256 of the executable: `<64 hex>`
- [ ] **Harness:** the real, logged-in Claude Code CLI (`claude --version`: `2.1.285`) and
  Codex CLI (`codex --version`: `0.159.3`) ran, not test doubles.
- [ ] **Beacon:** the real, operator-installed Beacon endpoint (`beacon version`:
  `1.3.29`) ran in Local mode, not a test double.
- [ ] **Consent dialog:** accepted by the DRIVER (`accept=driver`, #196), not by me:
  Claude workspace-trust, Claude MCP-server approval, Claude development-channels and Codex
  workspace-trust (sequence numbers above).
- **Attested by:** <operator>, <YYYY-MM-DD>
