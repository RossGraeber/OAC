---
name: oac-mcp
description: Model Context Protocol revision history, the stateless 2026-07-28 era, SEP-2133 extensions, and the dual-era constraint gate G4 tests. Load for area:spec work items and gate:G4.
---

Version-pinned MCP protocol detail, copied in per the link-don't-copy exception (this is
the one skill allowed to carry third-party protocol detail verbatim, with its pin). Do not
restate this content elsewhere; other skills link to it by name.

## Constraints that most often trip an agent up

- MCP has no concept of "external event becomes a user turn." That semantic does not exist
  in any MCP revision. Do not design, describe, or accept a design where OAC's delivery
  mechanism is "MCP." MCP is packaging (capability negotiation, tool surface, `_meta`
  provenance) only. See ADR-001 conflict below.
- The current revision (`2026-07-28`) is **stateless**: there is no `initialize` handshake,
  and servers cannot initiate requests or push unsolicited content into the model's context.
  Do not assume a server can proactively send anything into a session on this revision.
- A server that negotiates `2026-07-28` cannot deliver Claude channel messages and is not
  registered as a channel (cross-reference `oac-claude-channels` and PLANNING-PROMPT.md
  §3.1). The Claude path must negotiate a legacy revision; gate G4 tests whether one process
  can still serve the `2026-07-28` tool path as well.
- There is no spec rule for namespacing custom method names. Only `_meta` keys and extension
  identifiers carry prefix rules. Do not invent a namespacing convention for method names and
  do not assume `_meta`-style prefix rules apply to method names.
- Prefixes whose second label is `modelcontextprotocol` or `mcp` are reserved — do not choose
  an extension identifier or `_meta` key that collides with this.

## Revision history and the dual era

Current revision: `2026-07-28`. `2025-11-25` and earlier are legacy, with a documented
dual-era interoperability matrix (see `docs/planning/PLANNING-PROMPT.md` §3.3 and its cited
MCP changelog/versioning sources in Appendix B — do not re-derive the matrix here; it belongs
to the MCP spec itself).

## What the current revision changed

- Stateless: `initialize` is gone. Every request carries
  `_meta["io.modelcontextprotocol/protocolVersion"]`.
- Servers cannot initiate requests or push unsolicited content into the model's context.
  Server-initiated requests were replaced by multi-round-trip results on `tools/call`,
  `prompts/get`, `resources/read`.
- Only subscription-gated `list_changed`/`resources/updated`, request-scoped progress, and
  deprecated logging notifications remain.

## The headline consequence — this is why this skill exists

No MCP revision defines "external event becomes a user turn." That semantic is always
provider-native. The OAC spec can be packaged as an MCP extension for capability
negotiation, tool surface, and `_meta` provenance, but it must not claim MCP as the delivery
mechanism.

This directly conflicts with ADR-001's Decision section, which names the layer an
"**MCP Session Channels extension**" (see PLANNING-PROMPT.md Appendix A, conflict C3). Do
not silently resolve this conflict in a skill, spec draft, or implementation. It is resolved
in PLANNING-PROMPT.md §5 decision 3 (spec packaging), not by any agent reading this skill —
if you are about to write or implement spec-packaging language, read that decision's outcome
first (see `docs/planning/v0.1/03-decisions-and-amendments.md` once it exists, otherwise
`docs/planning/STATUS.md`) rather than guessing.

## Extensions — SEP-2133 (final 2026-01-26)

- Formal extension identifier form: `{reverse-dns-prefix}/{name}` (for example
  `io.github.<owner>/oac`).
- Negotiated through the `extensions` map in capabilities.
- Breaking changes require a new identifier.
- `experimental` capabilities still exist separately and are what Claude Channels use
  (`capabilities.experimental["claude/channel"]` — see `oac-claude-channels`, not restated
  here).
- No spec rule for namespacing custom method names; only `_meta` keys and extension
  identifiers have prefix rules.
- Prefixes whose second label is `modelcontextprotocol` or `mcp` are reserved.

## The dual-era constraint gate G4 tests

Gate G4 (MCP dual-era server): one OAC MCP server process must serve Claude's legacy-revision
channel path and a `2026-07-28` tool path (for Codex via `codex mcp add`) without breaking
either.

- A Claude channel server negotiating `2026-07-28` cannot deliver channel messages and is not
  registered as a channel (PLANNING-PROMPT.md §3.1). The Claude path must negotiate a legacy
  revision (`2025-11-25` or earlier), optionally forced with `MCP_PROTOCOL_NEGOTIATION=legacy`
  for stdio servers.
- The Codex tool path (via `codex mcp add`) may negotiate the current revision.
- Pass/fail/fallback for G4: one process serves both eras, or — fallback — two entry points
  share one core (PLANNING-PROMPT.md §4, Appendix A conflict C5).
- G4 cross-references PLANNING-PROMPT.md §3.1 (Claude's MCP version constraint) and §5
  decisions 2 and 3.

## Agent-to-agent messaging: nothing to wait for

No SEP or working-group item for agent-to-agent messaging exists as of the retrieval date
(2026-09-15). There is nothing upstream to track or copy for this. Do not invent or assume
one is in progress.

## Pin

- MCP current revision: `2026-07-28`.
- MCP legacy revisions: `2025-11-25` and earlier.
- SEP-2133 (extensions): final `2026-01-26` — **re-verified in B2**: PR #2133's
  `merged_at` is `2026-01-26T23:57:49Z`, confirming this date directly (the SEP-2133
  page's own `Created` field, `2025-01-21`, is a different field — when the SEP was
  opened, not when it finalized). No drift.
- **Drift found in B2:** the reserved-prefix rule ("prefixes whose second label is
  `modelcontextprotocol` or `mcp` are reserved") does **not** appear anywhere in the
  SEP-2133 text. The only normative prefix clause is that the vendor prefix SHOULD be a
  reversed domain name the extension author owns or controls, and that official
  extensions use the `io.modelcontextprotocol` prefix. Do not state or enforce a blanket
  reservation rule for any `*.modelcontextprotocol` / `*.mcp` second label; cite only the
  domain-ownership SHOULD clause that is actually present. See
  `docs/planning/REVERIFICATION-B2.md` §3.3 carry-over (b) and Drift register D2.
- The `subscriptions/listen` mechanism (opt-in `toolsListChanged`, `promptsListChanged`,
  `resourcesListChanged`, `resourceSubscriptions`, tagged with
  `io.modelcontextprotocol/subscriptionId`) **replaces** the older `resources/subscribe`
  / `resources/unsubscribe` / HTTP GET pattern at `2026-07-28`. If you have prior context
  naming `resources/subscribe` directly, it no longer exists at the current revision. See
  `docs/planning/REVERIFICATION-B2.md` Drift register D1.
- Source: `docs/planning/PLANNING-PROMPT.md` §3.3, plus the MCP URLs in Appendix B
  (`https://modelcontextprotocol.io/specification/2026-07-28/changelog`,
  `.../basic/versioning`, `.../basic/patterns/mrtr`,
  `https://modelcontextprotocol.io/extensions/overview`,
  `https://modelcontextprotocol.io/seps/2133-extensions`), re-verified against
  `docs/planning/PINS.md` (`2026-07-28` / `2025-11-25`) and
  `docs/planning/REVERIFICATION-B2.md` §3.3.
- Retrieval date: 2026-09-16 (B2 re-verification; original baseline 2026-09-15).
- **Confirmed pin (B2).** The MCP pin is now load-bearing per `docs/planning/PINS.md`.
  A revision bump invalidates this skill and it must be re-verified per `oac-evidence`
  §7 before being trusted again, updating this `## Pin` section and `PINS.md` together.

## Where the content lives

- Full baseline text: `docs/planning/PLANNING-PROMPT.md` §3.3 (MCP), §3.1 (Claude MCP version
  constraint), §4 (gate G4), §5 decision 3 (spec packaging), Appendix A conflicts C3 and C5,
  Appendix B (MCP URLs).
- ADR conflict: `docs/planning/ADR-001.md` Decision section ("MCP Session Channels
  extension").
- Current pin status and open items: `docs/planning/STATUS.md`.
- Evidence and citation rules, UNVERIFIED handling: `oac-evidence`.
- Boundary guardrails: `oac-boundaries`.
- Spec-authoring rules for writing normative text that uses this detail: `oac-spec-authoring`.
