# L1: Beacon beside OAC, not inside it — pin, license, operator decisions Q1-Q5

**Issue:** #166 (Epic L #165, backlog key `L1`). **Depends on:** none. **Source:** issue
#165 workstreams L1 and L3; `[ADR-001 Boundary]`; `[DESIGN Non-goals]`;
`[PLANNING-PROMPT §10]`; `oac-boundaries` #12.

**Status:** **Decided.** Beacon (agent-beacon) is an external, independently installed,
review-gated memory service that each harness connects to natively over MCP. OAC never
stores, fetches, summarizes, or injects memory. Beacon is pinned at `v1.3.29`, fixed
(§2). Its license is MIT, recorded as an external service that OAC does not ship (§3).
The operator's answers to Q1-Q5 (2026-09-29) are recorded in §4 and are not re-opened
by later Epic L tasks. Four facts stay UNVERIFIED and are handed to L2 (§6).

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
