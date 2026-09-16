---
name: oac-boundaries
description: The ADR-001 boundary self-check — every MUST NOT with a concrete drift example, the mechanical grep/lint checks, and the stop-cite-the-boundary protocol. Load for almost every work item, before writing code, spec text, or a decision.
---

OAC is a provider-neutral messaging layer between existing AI harness sessions, not an
agent framework. This skill holds every inviolable boundary and how to catch a drift into
one, mechanically where possible. Load it before any `type:code`, `type:spec`,
`type:decision`, `type:docs`, or `area:*` work item; almost everything loads it.

For the evidence/citation standard (sourcing, versions, UNVERIFIED labels), see
`oac-evidence` — not restated here.

## The boundaries

Each entry: source, the rule verbatim or near-verbatim, and a concrete way an agent drifts
into it on this repo.

1. **[ADR-001 Boundary]** "MUST NOT call provider model APIs as a substitute for native
   harnesses." Drift: you are writing the Codex adapter, `turn/start` returns an error you
   don't understand, and instead of filing a finding you reach for the OpenAI SDK (or the
   Anthropic SDK for the Claude side) to get a completion so the demo "works." That is
   substituting a model API for the native harness — forbidden even as a stopgap.

2. **[ADR-001 Boundary]** "MUST NOT implement inference/model routing/context management."
   Drift: you build a helper that decides which harness "should" answer a message based on
   its content, or one that trims/summarizes conversation history so it fits a context
   window before handing it to a provider. Both are inference/context decisions OAC does
   not own — the harness owns them.

3. **[ADR-001 Boundary]** "MUST NOT steal or reuse another harness's provider credentials."
   Drift: the Codex adapter reads `CODEX_HOME/auth.json` (or the OS keyring entry Codex
   uses) to call the OpenAI API directly, or the Claude adapter reuses a Claude Code OAuth
   token for a direct Anthropic API call "just for this one feature." DESIGN §Codex adapter
   says explicitly: "Never call the OpenAI model API directly." Same rule for Claude.

4. **[ADR-001 Boundary]** "MUST NOT depend on UI/terminal scraping or undocumented private
   RPCs for supported integrations." Drift: you can't get `thread/queue/add` to behave, so
   you screen-scrape the Codex TUI's stdout, or you call `codex mcp-server` (removed
   2026-09-05 per PLANNING-PROMPT.md §3.2 — "Do not plan on it") because it used to work, or
   you call an internal `codex-rs` function not in `app-server-protocol/schema`. Any of
   these is an undocumented private RPC or scraping substitute.

5. **[ADR-001 Boundary]** "MUST NOT leak Zenoh-specific concepts into the neutral protocol."
   Drift: while implementing the transport contract you add a `zenoh_key_expr` field to a
   `core/` type, or the spec text under `spec/` explains delivery in terms of Zenoh
   liveliness tokens instead of the neutral presence states. DESIGN §Zenoh transport: "Zenoh
   types must not escape the transport module." DESIGN §MCP Session Channels extension: "The
   specification MUST NOT mention Zenoh keys, MQTT topics, NATS subjects, or
   provider-specific method names."

6. **[ADR-001 Decision]** "The reference implementation is a CLI and must not require
   Docker, Kubernetes, a cloud account, or a separately administered server for normal local
   use." Drift: G3 loopback multicast scouting is flaky on your Windows box, so you make
   `zenohd` (the Zenoh router) a required background process for local mode — that turns an
   optional peer-to-peer transport into a separately administered server for the default
   path.

7. **[PLANNING-PROMPT §4 / ADR-001-A2]** Neither harness supports attaching to an arbitrary,
   already-running session through a supported interface: §3.1 — "A channel cannot be
   attached to an already-running session" (Claude); §3.2 — cross-process resume does not
   attach, a second app-server process appends to a held thread's history silently without
   notifying the live process (issue #21743). ADR-001-A2 therefore redefines "existing
   session" in the Validation criterion as a session **launched OAC-enabled**, not an
   arbitrary already-running one. Drift: reading the Validation criterion literally ("an
   existing Claude Code session") and concluding you need to reach into a session nobody
   launched with `--channels` or the daemon — which leads straight into boundary 4's
   scraping/private-RPC territory or boundary 13's rollout-file territory. This is the most
   likely drift on this project; check it before assuming a workaround is needed.

8. **[DESIGN Non-goals]** "Agent planning" is out of scope. Drift: you add a module that
   decomposes an incoming message into sub-tasks and schedules them across sessions — that
   is agent planning, not neutral message delivery.

9. **[DESIGN Non-goals]** "Unsupported client impersonation" is out of scope. Drift: you
   spoof the Codex Desktop/TUI client identity to reach the private stdio app-server or the
   control socket Desktop does not expose (§3.2), or you present your dev channel server as
   an allowlisted Claude channel plugin to bypass the `--dangerously-load-development-
   channels` confirmation (§3.1). (Declaring `capabilities.experimentalApi` to call a
   documented experimental app-server method is the supported way in and is *not*
   impersonation — don't over-block that.)

10. **[DESIGN Non-goals]** "Replacement provider auth" is out of scope. Drift: OAC issues its
    own session tokens and treats them as standing in for a harness's own login, instead of
    routing through each harness's existing auth (Codex's saved CLI login, Claude's own
    session).

11. **[ADR-001 v0.1 scope]** Group rooms/broadcast, attachments, durable offline mailboxes,
    federation, full E2E encryption, GUI, production Gemini/ChatGPT/Cursor adapters, and
    alternative transports are explicitly deferred, not v0.1 work. Drift: you add a durable
    store so a message survives an offline peer and gets delivered later — that is the
    deferred offline mailbox. A broadcast/room key pattern in the transport layer is the same
    failure (deferred group rooms).

12. **[PLANNING-PROMPT §10]** "Do not turn OAC into an agent framework, inference router,
    shared context manager, or provider-auth abstraction." Drift: a "conversation memory"
    store shared across both providers so replies can reference earlier turns from either
    harness — that is a shared context manager.

13. **[PLANNING-PROMPT §10]** "Do not substitute model APIs, UI automation, terminal
    scraping, credential reuse, private RPCs, or rollout-file manipulation for a supported
    interface. If a supported interface is missing, that is a finding, not a workaround."
    Drift: `thread/queue/add` isn't landing input the way you expect, so you write directly
    into a rollout file under `CODEX_HOME/sessions/` to force history — PLANNING-PROMPT.md
    §3.2 states the rollout file format "is explicitly not a supported surface."

14. **[DESIGN Delivery semantics / PLANNING-PROMPT §11.7]** "Application-level inbox polling
    is not [acceptable] for adapters claiming active inbound support." Drift: the Claude
    channel notification path is flaky in testing, so the adapter adds a background loop
    that polls an MCP resource every second for new messages, while still advertising
    active-inbound support in its capabilities. Either fix real delivery, turn off the
    active-inbound claim, or file a finding — do not poll silently.

**Explicitly allowed (ADR-001 Boundary, MAY clause):** "It MAY use documented MCP
extensions, app-server protocols, ACP, hooks, extensions, or other supported IPC, and
translate neutral messages into provider-native live-session input operations." This is the
counterweight to the list above — step 1 of the stop protocol is to check whether what
you're doing is actually this, before treating it as a violation.

## Mechanical checks

No boundary-specific lint exists in this repo yet — `scripts/check-skills.mjs` checks skill
budgets, not boundaries. The full runnable grep set, with current pass/pending status against
this repo, is `references/mechanical-checks.md`. Run it before every `type:code`/`type:spec`
work item; the paths it targets (`spec/`, `core/`, `adapters/`, `cli/`, `transports/zenoh/`)
mostly don't exist yet, so most checks report a missing-path error today — that error means
**pending**, not passing, and the whole set must be re-run once code lands.

## Stop, cite the boundary

The moment you notice a drift (your own plan, existing code, or a request from the user)
matches one of the boundaries above:

1. **Stop.** Do not continue implementing the drifting approach, even "temporarily" or "to
   unblock."
2. **Quote the exact boundary line** you matched against, with its source tag (e.g.
   `[ADR-001 Boundary]`), in your response or commit message.
3. Decide which of the two allowed routes applies:
   - **A real boundary conflict** (the plan's decision is wrong given evidence) — record the
     conflict, cite the evidence, and propose a numbered ADR-001 amendment: old text, new
     text, rationale. Do not silently redesign around it (PLANNING-PROMPT.md §1).
   - **A missing supported interface** (the documented interface doesn't do what you need
     yet, or its behavior is UNVERIFIED) — record it as a finding, not a workaround
     (PLANNING-PROMPT.md §10). File it where the current stage's output package expects
     findings (see `oac-gates` or `oac-planning-package`).
4. **Do not implement the workaround while waiting** for the amendment or finding to be
   resolved. Leave the work item blocked and say so.

## Pre-commit self-check

Before declaring any work item done (PLANNING-PROMPT.md §11 items 6-7):

- [ ] Ran the mechanical checks in `references/mechanical-checks.md` against every file you
      touched — zero hits where the path exists, no new pending-path gaps introduced.
- [ ] Nothing you wrote proposes OAC owning a harness's turn loop.
- [ ] Nothing you wrote has OAC holding, reading, or reusing a provider's own credentials.
- [ ] No adapter you touched polls an inbox while still claiming active-inbound support.
- [ ] Every boundary hit you found during the work item was either fixed, turned into an
      ADR-001 amendment proposal, or recorded as a finding — none were silently worked
      around.

## Where the content lives

- `docs/planning/ADR-001.md` — Decision, Boundary, v0.1 scope, Alternatives, Validation
  criterion.
- `docs/planning/DESIGN.md` — Non-goals, Zenoh transport module boundary, spec vocabulary
  rule, Delivery semantics.
- `docs/planning/PLANNING-PROMPT.md` §4 (structural finding), §10 (planning rules), §11
  items 6-7 (self-review).
- `docs/planning/SKILLS-MODEL.md` — Tier 2a description of this skill's role.
- `references/mechanical-checks.md` — the full grep set, status, and scope notes.
- `oac-evidence` — sourcing/citation standard (separate skill, not restated here).
