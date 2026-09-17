# ADR-001 amendments A1-A3 and conflict register resolution (B3)

This is the amendment ledger for issue #15 (Epic B, task B3): it resolves conflict
register entries C1-C10 from `PLANNING-PROMPT.md` Appendix A and issues the three ADR-001
amendments (`ADR-001-A1` through `ADR-001-A3`) that PLANNING-PROMPT.md §2 and §4 already
earmark for C1, C2, and C3. It is a standalone ledger because
`docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4) does not exist yet;
per `oac-evidence` §6 step 5, an amendment lands here and is flagged in `STATUS.md` until
that file exists.

**Last updated:** 2026-09-16.

## Method

Precedence chain used throughout this file, per `oac-evidence` §6 and PLANNING-PROMPT.md
§1: `ADR-001` > `DESIGN` > `PLANNING-PROMPT.md` §3 baseline > judgment — except that
verified evidence beats all three.

Pin set used for every claim below (source: `PINS.md`, unchanged since B1/B2, retrieved
2026-09-16):

- Claude Code `v2.1.274`
- `@openai/codex@0.154.0`, commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`
- MCP current `2026-07-28`; MCP legacy `2025-11-25`
- Zenoh `1.10.1`
- Rust `1.98.1`
- ACP protocol version `1` (schema v2 alpha, not a v0.1 dependency)

Where a claim is unchanged from the pre-verified baseline, this file cites
`PLANNING-PROMPT.md §3.x` plus the `REVERIFICATION-B2.md` row that re-verified it against
the pin above — no fact below was re-fetched fresh in B3; B3 reuses B1/B2 evidence per
`oac-evidence` §2 ("if you have not changed the claim").

**Why `ADR-001.md`'s body is not rewritten.** This file adds exactly one pointer line to
`ADR-001.md` (`**Amendments:** A1-A3, see docs/planning/ADR-001-AMENDMENTS.md`) and
otherwise leaves its body text untouched. ADR house form keeps the decision record
immutable and carries changes as numbered amendments; rewriting the body in place would
destroy the verbatim "old text" the amendments above quote, which is the thing each
amendment's rationale depends on to prove what changed and why.

## Unchanged boundary

None of A1-A3 changes the ADR-001 Boundary. Quoted verbatim from `ADR-001.md`:

Line 24 (the MUST NOT list):

```
The project MUST NOT call provider model APIs as a substitute for native harnesses; implement inference/model routing/context management; steal or reuse another harness's provider credentials; depend on UI/terminal scraping or undocumented private RPCs for supported integrations; or leak Zenoh-specific concepts into the neutral protocol.
```

Line 26 (the MAY clause):

```
It MAY use documented MCP extensions, app-server protocols, ACP, hooks, extensions, or other supported IPC, and translate neutral messages into provider-native live-session input operations.
```

Per PLANNING-PROMPT.md §4: "The boundary that matters is unchanged: OAC never owns
inference, credentials, or the turn loop."

A2's redefinition (below) narrows the *validation target* — which sessions the
Validation criterion is proven against — not the boundary itself. A3's wording change
narrows how the extension packages capability negotiation, not what OAC is allowed to do.
The MAY clause above is what both A2 and A3 rely on: it is the documented basis for
translating neutral messages into provider-native live-session input operations without
that being a boundary violation.

## ADR-001-A1

**Resolves:** C1.

**Old text (verbatim):**

Six ADR-001.md sites carry the pre-rename names, each quoted with its line anchor:

(a) Line 1, title:

```
# ADR-001: Provider-Neutral Session Channels
```

(b) Line 15:

```
Create a provider-neutral **MCP Session Channels extension** with three replaceable layers:
```

(c) Lines 17-18, list items 1 and 2:

```
1. **Provider adapters** — translate native harness interfaces to/from Session Channels.
2. **MCP Session Channels** — define identity, addressing, capabilities, messages, replies/correlation, presence, delivery, and security semantics.
```

(d) Line 36, architecture diagram label:

```
            MCP Session Channels
```

(e) Line 44:

```
- **Claude Code:** neutral inbound message -> Claude Channel event; outbound send/reply -> neutral Session Channels operation.
```

(f) Line 74, inside the Validation criterion:

```
Architecture is proven when an existing Claude Code session sends through Session Channels/Zenoh to an existing Codex harness session without receiver polling
```

**Overlap note:** (b), the "MCP Session Channels" phrase in list item 2 (c), and the
diagram label (d) also carry the claim ADR-001-A3 rewords (what MCP *is* and *does* for
this layer). To keep the two amendments from silently conflicting: **A1 owns the
product/CLI/spec names** (the strings "Session Channels", "MCP Session Channels
extension", `sessionchannels`); **A3 owns the claim about what MCP does** (that MCP is
the delivery mechanism versus a capability-negotiation/provenance packaging only). Where
a site carries both (sites (b) and (d)), A1's new text renames it and A3's new text
restates the layer's actual role; apply both amendments to that site — A1's naming
substitution first, A3's semantic correction second. Site (c) list item 1 and item 2's
lead-in are A1-only (naming); (c) item 2's substance ("define identity, addressing,
...") is unaffected by A3 and carries over unchanged under the new name.

**New text:**

- Product and repository: **Open Agent Channel (OAC)**.
- Normative protocol specification: **OAC Session Channels** (short form: "the OAC
  spec").
- CLI binary: `oac`.
- `sessionchannels` (the CLI name sketched in ADR-001/DESIGN) is retired.

Applying this to each site: (a) title becomes `# ADR-001: Open Agent Channel (OAC) —
Provider-Neutral Session Channels`; (b) "MCP Session Channels extension" becomes "OAC
Session Channels specification, packaged as an MCP extension" (see A3 for why this
phrasing, not a bare rename); (c) item 1 unchanged substance, "Session Channels" ->
"OAC Session Channels"; item 2 heading "MCP Session Channels" -> "OAC Session Channels";
(d) diagram label "MCP Session Channels" -> "OAC Session Channels"; (e) "Session Channels
operation" -> "OAC Session Channels operation"; (f) "Session Channels/Zenoh" -> "OAC
Session Channels/Zenoh".

**CLI name-conflict check for `oac`** (PLANNING-PROMPT.md §2 requires this be run and
recorded):

| Check | Method | Result | Source | Retrieved |
|---|---|---|---|---|
| crates.io `oac` | `https://crates.io/api/v1/crates/oac` (registry API, avoids JS-rendered page) | Not found (HTTP 404) | https://crates.io/api/v1/crates/oac | 2026-09-16 |
| npm `oac` | `https://registry.npmjs.org/oac` (registry API) | **Found — conflict.** Package `oac` exists: version `0.0.0`, published 2021-06-17, MIT license, maintainer `configurator`, 84-byte unpacked tarball, no README. Reads as an inactive name-squat, not an active competing tool, but the exact name is taken on the npm registry. | https://registry.npmjs.org/oac | 2026-09-16 |
| Homebrew formula `oac` | `https://formulae.brew.sh/formula/oac` | Not found (HTTP 404) | https://formulae.brew.sh/formula/oac | 2026-09-16 |
| Debian/Ubuntu package `oac` | `https://packages.ubuntu.com/search?keywords=oac&searchon=names` (apt-file-style name search) | Not found. 18 packages contain the substring "oac" (e.g. `goaccess`, `openarena-oacmp1`, `python3-aioacaia`); none is named exactly `oac`. | https://packages.ubuntu.com/search?keywords=oac&searchon=names | 2026-09-16 |
| `where oac` / `which oac` on dev machine | Shell lookup (`where oac`, `which oac`) on the Windows dev machine used for this task | Not found on `PATH` | Local shell, `c:\sources\OAC` working tree | 2026-09-16 |

**Verdict:** the `oac` binary name is free on crates.io, Homebrew, and Debian/Ubuntu, and
absent from this dev machine's `PATH`. It is **taken on npm** by an inactive, contentless
stub package (`oac@0.0.0`, 2021, 84 bytes). Because OAC's CLI is a Rust binary (per §5
decision 1's presumptive answer, Rust/single static binary) and does not currently plan
an npm-distributed package, this does not block the `oac` binary name for v0.1. It is
recorded, not silently dropped: if OAC later ships an npm-distributed wrapper or MCP
package under the name `oac`, the existing npm squat must be resolved first (contact npm
support to reclaim an abandoned name, or pick a scoped/alternate npm package name such as
`@openagentchannel/oac`). This is not labelled UNVERIFIED — it is a checked, positive
finding (conflict exists) with a stated non-blocking rationale, not an unanswerable
question.

**Rationale:** PLANNING-PROMPT.md §2 resolves the OAC/Session-Channels naming split once,
at the top of the plan: "Product and repository: Open Agent Channel (OAC). Normative
protocol specification: OAC Session Channels ... CLI binary: `oac` unless you find a
conflict with an existing widely used tool; state the check. ... Record the rename as
ADR-001-A1." PLANNING-PROMPT.md §11 item 8 requires the naming resolution be "applied
consistently" in self-review. The repository's own README and Apache-2.0 LICENSE already
name the repository OAC, not Session Channels or `sessionchannels` — ADR-001 and
DESIGN.md are the documents that still carry the pre-rename name, so this amendment
brings them into line with the naming already in force at the repository root.

## ADR-001-A2

**Resolves:** C2.

**Old text (verbatim):**

`ADR-001.md` line 74, the whole Validation criterion sentence, unedited:

```
Architecture is proven when an existing Claude Code session sends through Session Channels/Zenoh to an existing Codex harness session without receiver polling; Codex responds and Claude receives the response actively; neither side invokes or holds credentials for the other's model API; and sender identity/authorization are enforceable rather than inferred from content.
```

**New text:**

```
Architecture is proven when a Claude Code session, launched OAC-enabled, sends through OAC Session Channels/Zenoh to a Codex harness session, launched OAC-enabled, without receiver polling; Codex responds and Claude receives the response actively; neither side invokes or holds credentials for the other's model API; and sender identity/authorization are enforceable rather than inferred from content.
```

Only the two occurrences of "existing" are replaced, with "launched OAC-enabled" (plus
the naming rename from A1, "Session Channels" -> "OAC Session Channels", applied at the
same site since A1 and A2 touch the same sentence). The rest of the criterion — "without
receiver polling; Codex responds and Claude receives the response actively; neither side
invokes or holds credentials for the other's model API; and sender identity/authorization
are enforceable rather than inferred from content" — is kept byte-identical. Stated
explicitly, per the task instruction: this clause was not reworded, trimmed, or
reordered; every word after "without receiver polling" through the end of the sentence is
unchanged from the old text above.

"Launched OAC-enabled" is defined per PLANNING-PROMPT.md §4: "a live interactive session,
launched OAC-enabled, whose owner is a human at a terminal, into which OAC injects
without owning the harness's model loop."

**Evidence-backed launch forms.** The two OAC-enabled launch forms named as examples,
quoted verbatim from their sources:

- **Claude Code:** started with `--channels` (for example `--channels
  plugin:<name>@<marketplace>` or `--channels server:<name>`, or, for a development
  channel, `--dangerously-load-development-channels`). Source:
  PLANNING-PROMPT.md §3.1, re-verified against the `v2.1.274` pin in
  `REVERIFICATION-B2.md` §3.1 table, row "`--channels plugin:<name>@<marketplace>` and
  `--channels server:<name>`" — HOLDS.
- **Codex:** through the shared local app-server daemon (`codex app-server daemon
  start`), which the TUI attaches to automatically when launched without config
  overrides, or through an OAC-owned app-server with the TUI attached via `codex
  --remote`. Source: PLANNING-PROMPT.md §3.2, re-verified in `REVERIFICATION-B2.md` §3.2
  acceptance box 4 — source code for the control socket path and the attach-or-embed
  branch is confirmed present at the pinned commit
  `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`.

**Evidence block:**

- PLANNING-PROMPT.md §3.1: "A channel cannot be attached to an already-running session."
- PLANNING-PROMPT.md §3.2: cross-process resume does not attach (issue #21743) — a
  second app-server process resuming a thread another process holds loads history from
  disk and appends silently, without notifying the live process. Re-confirmed in
  `REVERIFICATION-B2.md` "Closed UNVERIFIED items" item 4: issue #21743 checked directly
  at `https://api.github.com/repos/openai/codex/issues/21743`, `"state": "open"`,
  `"closed_at": null`, retrieved 2026-09-16 — still open at the pinned retrieval date, no
  drift to the "no attach" premise.
- PLANNING-PROMPT.md §4's structural finding paragraph: "Neither Claude Code nor Codex
  supports attaching an external channel to an arbitrary, already-running session process
  through supported interfaces. Both support live, active, full-duplex injection into a
  session that was launched OAC-enabled ... Therefore redefine 'existing session' in
  ADR-001's validation criterion as 'a live interactive session, launched OAC-enabled,
  whose owner is a human at a terminal, into which OAC injects without owning the
  harness's model loop.' Propose that as ADR-001-A2."

**Guard — G2 not claimed passed.** This amendment does not claim gate G2 (Codex live
inject) has passed. Per `STATUS.md`, every gate verdict is `NOT RUN`, and whether implicit
Codex daemon attach executes by default at runtime in the pinned release is UNVERIFIED for
runtime behaviour, owner D2 (see `PINS.md` "Daemon-attach open question (G2 input)" and
`REVERIFICATION-B2.md` "Carried to 11-risks.md" item 3). This amendment writes the Codex
daemon-attach launch form as the **documented path whose runtime behaviour is gated on
G2**, and names the `codex --remote` fallback explicitly, as PLANNING-PROMPT.md §4 itself
does ("Fallback if implicit attach is not released: OAC owns the app-server and the user
runs `codex --remote`").

**Scope note — compatibility shim boundary is out of B3's scope.** Issue #15's A2
acceptance box interacts with `REVERIFICATION-B2.md` "Carried to 11-risks.md" items 9 and
10: the compatibility shim boundary for both the Claude Code Channels research-preview
surface and the Codex experimental live-inject surface is UNNAMED (DESIGN.md names no
such module), and B2 assigned the owner to "a C-series decision or a DESIGN.md update."
This amendment does not invent a module name. It is recorded as new register entry C11
(below) and stays out of B3's scope: A2 redefines the Validation criterion's session
launch condition; it does not name an implementation module.

**Rationale:** ADR-001's own Validation criterion, read literally, requires "an existing
Claude Code session" and "an existing Codex harness session" — language that, per
`oac-boundaries` boundary 7, invites reaching into an arbitrary already-running session
nobody launched OAC-enabled, which leads into scraping or private-RPC territory. The
evidence above shows neither harness supports attach-to-arbitrary-running-process through
a supported interface; redefining "existing" as "launched OAC-enabled" keeps the
Validation criterion provable without a boundary violation, while leaving every other
clause of the criterion — no receiver polling, no cross-provider credential use,
enforceable sender identity/authorization — untouched.

## ADR-001-A3

**Resolves:** C3 (wording only; the extension identifier itself is `ASSIGNED` to decision
task C3 — see the conflict register below).

**Old text (verbatim):**

`ADR-001.md` line 15, in full:

```
Create a provider-neutral **MCP Session Channels extension** with three replaceable layers:
```

`ADR-001.md` line 18, list item 2, in full:

```
2. **MCP Session Channels** — define identity, addressing, capabilities, messages, replies/correlation, presence, delivery, and security semantics.
```

**New text:**

Line 15 becomes:

```
Create a provider-neutral **OAC Session Channels specification**, packaged as an MCP extension for capability negotiation, the tool surface, and `_meta` provenance only, with three replaceable layers:
```

List item 2 becomes:

```
2. **OAC Session Channels** — the neutral specification: identity, addressing, capabilities, messages, replies/correlation, presence, delivery, and security semantics. Packaged as an MCP extension for capability negotiation, the tool surface, and `_meta` provenance only; MCP is not the delivery mechanism.
```

The three-layer structure (provider adapters / neutral spec layer / transport plugins)
and layers 1 and 3 (provider adapters, transport plugins) are unchanged by this
amendment — only layer 2's description changes, to state what MCP does and does not
provide for it.

**What changes semantically:** the layer is the neutral **OAC Session Channels**
specification (renamed per A1). It is packaged as an MCP extension, but only for
capability negotiation, the tool surface, and `_meta` provenance. MCP is explicitly **not**
the delivery mechanism. Active inbound delivery is always provider-native: Claude Code
via `notifications/claude/channel`; Codex via app-server live-inject
(`thread/queue/add`, `turn/steer`, or `turn/start`).

**Evidence:**

- PLANNING-PROMPT.md §3.3: the current MCP revision (`2026-07-28`) is stateless —
  `initialize` is gone, every request carries
  `_meta["io.modelcontextprotocol/protocolVersion"]`, and servers cannot initiate
  requests or push unsolicited content into the model's context; consequence stated
  verbatim: "no MCP revision defines 'external event becomes a user turn'. That semantic
  is always provider-native." Re-verified against the pin in `REVERIFICATION-B2.md` §3.3
  table: "Current revision is stateless; `initialize` is gone" — HOLDS; "Every request
  carries `_meta[\"io.modelcontextprotocol/protocolVersion\"]`" — HOLDS; "Servers cannot
  initiate requests or push unsolicited content" — HOLDS; "No MCP revision defines
  'external event becomes a user turn'" — HOLDS (by omission, full `2026-07-28` changelog
  reviewed). All re-verified 2026-09-16 against the `2026-07-28` pin — no fresh fetch
  performed in B3; these citations reuse the B2 rows per `oac-evidence` §2.

**Three guards, each stated in one line:**

(a) Per `REVERIFICATION-B2.md` Drift register D1: this amendment does not write
`resources/subscribe` or `resources/updated` for the `2026-07-28` era — those methods are
removed; the surviving mechanism at this era is `subscriptions/listen`.

(b) Per `REVERIFICATION-B2.md` Drift register D2: this amendment does not state any
reserved-prefix rule for `*.modelcontextprotocol` / `*.mcp` second labels — SEP-2133 has
no such clause; the only clause present is the domain-ownership SHOULD ("the vendor
prefix SHOULD be a reversed domain name that the extension author owns or controls").

(c) Per `STATUS.md` "Open UNVERIFIED items": whether `capabilities.experimental` still
exists at MCP era `2026-07-28` is UNVERIFIED (re-labelled from HOLDS in B2 —
`REVERIFICATION-B2.md` §3.3 table, "the prior indirect inference from Claude Code's own
client capability was not a citation of the `2026-07-28` schema itself"). This amendment
states only that Claude Channels uses `capabilities.experimental["claude/channel"]` on
the legacy negotiation path (`2025-11-25` or earlier, per the `MCP_PROTOCOL_NEGOTIATION`
constraint in PINS.md), and does **not** assert that `experimental` survives at the
current era `2026-07-28`.

**Extension identifier out of scope:** choosing the actual extension identifier string
(for example an `io.github.<owner>/oac`-shaped identifier per SEP-2133's
`{vendor-prefix}/{extension-name}` format) is decision task C3 (PLANNING-PROMPT.md §5
decision 3, "Spec packaging"), not B3. This amendment fixes the wording of what the layer
is and how MCP relates to it; it does not name an identifier.

**Rationale:** ADR-001's original phrase "MCP Session Channels extension" reads as if MCP
itself carries the active-delivery semantic ("an external event can become model-visible
input", per ADR-001's own Context section). The re-verified evidence shows the current
MCP era removed server-initiated push entirely (replaced by the Multi-Round-Trip-Requests
pattern on `tools/call`/`prompts/get`/`resources/read`) and that no MCP revision, past or
present, defines an external-event-becomes-a-user-turn semantic. Keeping ADR-001's wording
unchanged would leave the decision text asserting something MCP does not do. This
amendment corrects the wording without changing the architecture: OAC Session Channels
remains layer 2 of the same three-layer decision, MCP remains the negotiation/tool-surface
packaging, and delivery remains provider-native as PLANNING-PROMPT.md §3.3 already
establishes.

## Conflict register C1-C10 (resolved)

Statuses, defined once:

- **RESOLVED-HERE** — an A-amendment in this file closes it.
- **RESOLVED-BY-EVIDENCE** — B1/B2 already closed it; nothing left to decide.
- **RESOLVED-BY-DECISION** — an A-amendment in this file resolved the wording, and a
  separate, now-landed C-series decision document resolved the remainder; nothing left
  to decide.
- **ASSIGNED** — resolution is a named open task; the register entry closes when that
  task lands.

No row below is marked `RESOLVED` where the resolution is an unrun gate.

| # | Conflict (short) | Status | Resolution lives in | Evidence |
|---|---|---|---|---|
| C1 | Product named OAC in repo, "Session Channels" in ADR/DESIGN, CLI `sessionchannels` | RESOLVED-HERE | `docs/planning/ADR-001-AMENDMENTS.md` §ADR-001-A1; applied to `docs/planning/ADR-001.md` pointer line | PLANNING-PROMPT.md §2 |
| C2 | ADR Validation criterion says "existing Claude Code session" / "existing Codex harness session" | RESOLVED-HERE | `docs/planning/ADR-001-AMENDMENTS.md` §ADR-001-A2; applied to `docs/planning/ADR-001.md` pointer line | PLANNING-PROMPT.md §3.1, §3.2, §4; `REVERIFICATION-B2.md` issue #21743 close |
| C3 | ADR names the layer an "MCP Session Channels extension" | RESOLVED-BY-DECISION | `docs/planning/ADR-001-AMENDMENTS.md` §ADR-001-A3 (wording); `docs/planning/decisions/C3-spec-packaging.md` (extension identifier `io.github.rossgraeber/oac-session-channels`, issue #16) | PLANNING-PROMPT.md §3.3; `REVERIFICATION-B2.md` §3.3 table |
| C4 | DESIGN envelope `security.signature` "implementation-defined" vs acceptance criterion 6 (enforced provenance) | ASSIGNED | backlog task C5 (envelope authenticity, replay, pairing, authorization decision); normative MUST lands in a future `spec/security.md` (Epic E) | PLANNING-PROMPT.md §3.4 / `REVERIFICATION-B2.md` §3.4 — "Zenoh provides no application-layer message signing," so the envelope signature is the only authenticity proof |
| C5 | Claude needs legacy MCP; Codex tool path may negotiate current MCP | ASSIGNED | gate G4 (`docs/planning/backlog` Epic D), spike task D4; decision tasks C2 (process model) and C3 (spec packaging) | PLANNING-PROMPT.md §3.1 MCP version constraint + §3.3, both re-verified in `REVERIFICATION-B2.md` §3.1/§3.3 tables; `STATUS.md` G4 fallback "two entry points, one core" |
| C6 | No Claude acknowledgement vs DESIGN `accepted` delivery state | ASSIGNED | backlog task C5 / PLANNING-PROMPT.md §5 decision 5 (receipt states defined by what is knowable: "accepted by adapter", "handed to harness", "unknown") | PLANNING-PROMPT.md §3.1, "Claude Code sends no acknowledgement; a resolved notification send means 'written to transport', not 'seen by the model'"; re-verified `REVERIFICATION-B2.md` §3.1 table row "No acknowledgement of delivery" — HOLDS |
| C7 | ACP is client-owned-session, not a channel | RESOLVED-BY-EVIDENCE | `docs/planning/v0.1/01-capability-matrix.md` (Epic A task A2, not yet written) | `REVERIFICATION-B2.md` §3.5 row "C7 framing: ACP is client-owned-session, not a channel" — HOLDS. ACP is not a v0.1 dependency; ACP schema v2 "alpha" remains UNVERIFIED (`STATUS.md` open list) |
| C8 | URI leaking device/harness vs no transport-concept leak | ASSIGNED | backlog task C4 (session identity, addressing, discovery, key storage decision) — opaque stable id plus a separate display form | `DESIGN.md` Addressing section; `ADR-001.md` Boundary (no Zenoh concepts in the neutral protocol) |
| C9 | Codex has no channel-tag convention | ASSIGNED | backlog task C6 (provider-facing trust rendering and outbound symmetry decision) / PLANNING-PROMPT.md §5 decision 9 | PLANNING-PROMPT.md §3.2 turn/event model; §3.1 Claude correlation is convention-only via `meta` |
| C10 | Permission relay lets any allowlisted sender approve tools | ASSIGNED (v0.1 default already fixed) | recording site `docs/planning/v0.1/06-security.md` (Epic A task A7) and backlog task C6 | PLANNING-PROMPT.md §3.1 `claude/channel/permission`, v2.1.234+; `REVERIFICATION-B2.md` re-confirmed the `>= v2.1.234` floor is satisfied at `v2.1.274` (`PINS.md` Claude Code Channels pin record); PLANNING-PROMPT.md §5 decision 8 and §7 already fix the v0.1 default: permission relay **off by default in v0.1** |

## New register entries

New entries discovered during B3, added per PLANNING-PROMPT.md Appendix A's instruction
to "Add entries you discover." Neither is closed here.

| # | Conflict (short) | Status | Resolution lives in | Evidence |
|---|---|---|---|---|
| C11 | Compatibility shim boundary is UNNAMED for both the Claude Code Channels research-preview surface and the Codex experimental live-inject surface | ASSIGNED | a C-series decision, or a `DESIGN.md` update, naming the module/interface that isolates each volatile surface (per `oac-evidence` §4, preview/experimental surfaces must name one) | `REVERIFICATION-B2.md` "Carried to 11-risks.md" items 9 and 10; `PINS.md` "Compatibility shim boundary: ... `shim boundary: UNNAMED — see DESIGN.md`" for both the Claude Code Channels and Codex pin records; `STATUS.md` "Open UNVERIFIED items" |
| C12 | `DESIGN.md` still carries the retired names (`sessionchannels`, "Session Channels", "MCP Session Channels extension") after ADR-001-A1 | ASSIGNED | Epic A task A9 (`docs/planning/v0.1/08-cli-and-deployment.md`) and a `DESIGN.md` follow-up edit | see "Carried to later tasks" below for the exact sites |

Both C11 and C12 are kept in `STATUS.md`'s open list (see the pointer added there); they
are not closed by this file.

## Carried to later tasks

A1 does **not** edit `DESIGN.md`. ADR-001 outranks DESIGN in the precedence chain (§1 of
this file; `oac-evidence` §6), so the ADR amendment above is the authoritative rename and
DESIGN.md follows it in a later task, not in B3. B3's scope per issue #15 is the ADR and
the conflict register, not a DESIGN.md rewrite.

`DESIGN.md` sites that still carry legacy names after ADR-001-A1, to be updated when Epic
A task A9 (`08-cli-and-deployment.md`) and a `DESIGN.md` follow-up land:

- Line 1 title: `# Session Channels — Software Design`.
- Lines 21-24, CLI block:
  ```
  sessionchannels start
  sessionchannels status
  sessionchannels sessions
  sessionchannels doctor
  ```
- Line 30 heading: `### MCP Session Channels extension`.
- Line 33, normative-concept sentence: "a harness advertising active inbound **Session
  Channels** support accepts an authorized external channel message as input to the
  addressed live session without application-level polling."

Owner: Epic A task A9 for the CLI/deployment-facing rename, plus a direct `DESIGN.md`
edit for the title/heading/prose sites. Tracked as register entry C11's sibling, C12,
above, and in `STATUS.md`'s open list.
