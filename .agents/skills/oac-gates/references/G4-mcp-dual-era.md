# G4 — MCP dual-era server

Source: PLANNING-PROMPT.md §4 G4, §3.1, §3.3, conflict register C5. Backlog:
`docs/planning/backlog/03-tasks-CD.json` D4.

## Disposition

Has a fallback; not listed as v0.1 go/no-go in §4.

## What the spike proves

One OAC MCP server process can serve Claude's legacy-revision channel path and a
`2026-07-28` tool path (for Codex via `codex mcp add`) without either path degrading the
other.

## Pass criteria (evaluate each individually)

- [ ] The legacy path registers as a channel (negotiates `2025-11-25` or earlier) and delivers
      notifications successfully.
- [ ] The current-revision (`2026-07-28`) path serves `tools/call` correctly, carrying OAC
      `_meta` provenance (D4's own criterion: an OAC-defined `_meta` extension key on the
      response, not the protocol-version mechanism below). §3.3 notes `_meta` keys carry
      prefix rules — prefixes whose second label is `modelcontextprotocol` or `mcp` are
      reserved, so OAC's `_meta` key must not collide with those.
- [ ] Separately, every request on the `2026-07-28` path carries
      `_meta["io.modelcontextprotocol/protocolVersion"]` — this is the stateless-revision
      carrier §3.3 requires on every request (distinct from the OAC provenance key above; do
      not conflate the two).
- [ ] Neither path degrades the other across a full session (run both concurrently, not
      sequentially, and confirm neither breaks).
- [ ] A server negotiating `2026-07-28` is confirmed to be **rejected as a channel** — run
      this negative case explicitly; the constraint is real, not folklore, per PLANNING-PROMPT
      §3.1's stated MCP version constraint.

## Failure criteria

Any of the five unmet is a failure of the single-process design; take the fallback and record
the result as `PASS (FALLBACK TAKEN)` rather than a plain `PASS`, or `FAIL` if the fallback
was not attempted or also failed.

## Fallback

Two server entry points sharing one core. Record the cost of this fallback (extra process,
duplicated negotiation code, or whatever the spike surfaces) in the gate result — the fallback
being available does not make its cost free to skip documenting.

## Surfaces and version pins

- MCP current revision: `2026-07-28`. Legacy: `2025-11-25` and earlier — dual-era
  interoperability matrix is documented, per §3.3.
- Current revision is stateless: `initialize` is gone; every request carries
  `_meta["io.modelcontextprotocol/protocolVersion"]`; **servers cannot initiate requests or
  push unsolicited content into the model's context** (server-initiated requests were
  replaced by multi-round-trip results on `tools/call`, `prompts/get`, `resources/read`).
  Only subscription-gated `list_changed`/`resources/updated`, request-scoped progress, and
  deprecated logging notifications remain.
- **Consequence to hold onto while running this gate:** no MCP revision defines "external
  event becomes a user turn." That semantic is always provider-native (Claude's channel
  mechanism, not MCP itself) — this gate proves the two MCP *surfaces* can coexist in one
  process, not that MCP itself delivers the wake.
- Codex outbound tool registration: `codex mcp add` registers **external** MCP servers Codex
  calls as tools — this is the supported outbound surface the current-revision path in this
  gate must work against.
- Extensions are formal (SEP-2133, final 2026-01-26): identifier
  `{reverse-dns-prefix}/{name}` (e.g. `io.github.<owner>/oac`), negotiated through the
  `extensions` map in capabilities. `experimental` capabilities still exist and are what
  Claude Channels use (`capabilities.experimental["claude/channel"]`). Prefixes whose second
  label is `modelcontextprotocol` or `mcp` are reserved — do not choose one.

## §3.1/§3.3 facts this spike must confirm or refute

- Closes conflict **C5**: "Claude channel path needs legacy MCP; Codex tool path may
  negotiate current MCP" — this gate is the direct test of whether one process can hold both
  simultaneously.
- Confirms the MCP version constraint stated in §3.1 is real: "channel servers that negotiate
  MCP protocol `2026-07-28` cannot deliver channel messages and are not registered as
  channels." The negative-case pass criterion above is exactly this check.
- No UNVERIFIED item from §3 is specifically assigned to G4 in STATUS.md; if the spike
  surfaces a new one (e.g. an undocumented interaction between the two negotiated revisions in
  one process), record it as a new UNVERIFIED item, not a silent assumption.

## Fixtures to capture

Capture the legacy-path negotiation and notification delivery, and the current-revision
path's `tools/call` request/response pair showing `_meta` provenance, from the same server
process instance — the fixture should make the coexistence visible, not just each path in
isolation. Capture the pinned MCP revisions and date.
