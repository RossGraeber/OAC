# C3: Spec packaging and the MCP extension identifier

**Issue:** #16 (Epic C, backlog key `C3`). **Depends on:** #15. **Source:**
PLANNING-PROMPT.md §5.3, §3.3, Appendix A conflict C3.

**Status:** Decided.

**Standalone-ledger note.** This file lives at `docs/planning/decisions/` because
`docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4) does not exist yet.
It folds into that file, unedited in substance, once A4 lands — mirroring
`docs/planning/ADR-001-AMENDMENTS.md`'s own standalone-ledger rationale (see that file's
opening paragraph). `docs/planning/STATUS.md` carries a one-line pointer to this file
until then.

**Scope note.** `ADR-001-A3` already resolves C3's *wording* (RESOLVED-HERE) — what the
layer is called and that it is packaged, not delivered, by MCP. That amendment is not
re-litigated here. This document owns only the part A3 left `ASSIGNED`: the extension
identifier string and the packaging shape.

---

## 1. Choice

OAC's spec packaging is: a **standalone normative document — `OAC Session Channels`,
under `spec/`** — plus an **MCP extension identifier**, used for capability negotiation,
the tool surface, and `_meta` provenance only. This is the choice, not an option among
several.

MCP-only packaging is rejected in one line: it would make normative text hostage to MCP
revision churn and implies MCP delivers the semantic OAC needs, which no MCP revision
defines (§5 expands this).

The chosen identifier (derivation in §2-§3): **`io.github.rossgraeber/oac-session-channels`**.

**The standalone document is normative-of-record.** The MCP extension registration is
packaging — capability negotiation, tool surface, `_meta` provenance — never the source
of normative text. If the two ever appear to disagree, the standalone document under
`spec/` governs.

## 2. Decide and record the identifier

**(a) Vendor prefix:** fixed as `io.github.rossgraeber` (lowercase; §3 justifies and
normalizes this).

**(b) Extension name:** fixed as `oac-session-channels`, not bare `oac`. The name
identifies the *extension* (the OAC Session Channels capability-negotiation surface),
not the product. `oac` alone would read as a product/company label; SEP-2133's own
examples name the capability (`oauth-client-credentials`, `websocket-transport`), not
the vendor's product line. Reserving bare `oac` costs nothing: a future second OAC
extension (e.g., a hypothetical transport-negotiation extension) would need its own
identifier regardless, under the same vendor prefix — there is no mechanism by which
claiming `oac-session-channels` today forecloses that.

**(c) SEP-2133's identifier format, quoted verbatim** (Source:
`docs/planning/PINS.md`, MCP revisions record, citing
https://modelcontextprotocol.io/seps/2133-extensions, retrieved 2026-09-16):

> "Extensions are identified using a unique *extension identifier* with the format:
> `{vendor-prefix}/{extension-name}`, e.g.
> `io.modelcontextprotocol/oauth-client-credentials` or
> `com.example/websocket-transport`."

Applied to OAC: `io.github.rossgraeber/oac-session-channels` — vendor prefix
`io.github.rossgraeber`, extension name `oac-session-channels`, joined by `/`, matching
the form of both worked examples.

**(d) Terminology note (already recorded in PINS.md, restated here for this document's
own self-containedness, not re-derived).** SEP-2133's own page uses the term
"vendor-prefix"; PLANNING-PROMPT.md §3.3 uses "reverse-dns-prefix". Both describe the
same reversed-domain-name convention: PINS.md records that "the vendor prefix SHOULD be
a reversed domain name that the extension author owns or controls" is the shared rule
either term names. This is a terminology note, not a fact drift (`docs/planning/PINS.md`,
MCP revisions record, retrieved 2026-09-16).

## 3. Justify ownership of the prefix

SEP-2133's ownership clause, quoted verbatim (Source:
https://modelcontextprotocol.io/seps/2133-extensions, retrieved 2026-09-16, re-quoted
via `docs/planning/REVERIFICATION-B2.md` §3.3 carry-over (b), same retrieval date):

> "To prevent identifier collisions, the vendor prefix SHOULD be a reversed domain name
> that the extension author owns or controls (similar to Java package naming
> conventions)."

**Evidence of control:**

- The GitHub account `RossGraeber` owns the repository this project lives in. Verified
  directly, not assumed: `gh api repos/RossGraeber/OAC` returns `.owner.login ==
  "RossGraeber"`, `.owner.type == "User"`, `.full_name == "RossGraeber/OAC"`. Source:
  GitHub REST API, `repos/RossGraeber/OAC` endpoint, retrieved 2026-09-17.
- Owning the account `RossGraeber` means controlling the subdomain
  `rossgraeber.github.io`: GitHub Pages' own docs state a user/organization site "must be
  stored in a repository named `<owner>.github.io`, where `<owner>` is the personal or
  organization account name", and allow "a maximum of one pages site per account" — i.e.
  the `<account>.github.io` subdomain is allocated one-to-one with the GitHub account
  that owns it, and no other account can claim `rossgraeber.github.io`. Source:
  https://docs.github.com/en/pages/getting-started-with-github-pages/about-github-pages,
  retrieved 2026-09-17.
- Reversing `rossgraeber.github.io` under the reversed-domain-name convention SEP-2133
  names ("similar to Java package naming conventions", i.e. `tld.domain.subdomain...`
  read right to left) gives `io.github.rossgraeber` — the label components reverse to
  `io` (TLD) . `github` (domain) . `rossgraeber` (subdomain/account), joined with dots.

**Lowercase normalization, stated explicitly.** GitHub account logins are
case-insensitive (the account is reachable as `RossGraeber`, `rossgraeber`, or any other
casing), while DNS labels are case-insensitive too but conventionally written lowercase,
and SEP-2133's own worked examples (`io.modelcontextprotocol`, `com.example`) are
lowercase throughout. The chosen prefix is therefore normalized to lowercase:
`io.github.rossgraeber` — never `io.github.RossGraeber`. This is stated as an explicit
rule for this project, not left to whoever next types the identifier to guess.

**Reversal condition for the domain itself, stated up front (expanded in §10).** If the
project ever moves to an apex domain it owns outright (e.g. a purchased `oac.<tld>`),
the correct reversal is that domain's own labels reversed (e.g. `<tld>.oac`), not a
continued reliance on the GitHub-account-derived prefix — see §10.

## 4. Guard — do NOT restate the reserved-prefix rule

Per `docs/planning/REVERIFICATION-B2.md` Drift register entry D2 and
`docs/planning/PINS.md`'s MCP revisions record: **no blanket reservation clause for
second labels `modelcontextprotocol` or `mcp` exists in the SEP-2133 text.** The only
normative prefix clauses SEP-2133 states are the domain-ownership SHOULD (§3 above) and:

> "Official extensions use the `io.modelcontextprotocol` vendor prefix."

(Source: https://modelcontextprotocol.io/seps/2133-extensions, retrieved 2026-09-16, as
re-quoted in `docs/planning/REVERIFICATION-B2.md` §3.3 carry-over (b).)

Issue #16's acceptance box reads "reserved second labels (`modelcontextprotocol`, `mcp`)
avoided." This document satisfies it by the **factual non-collision** — the chosen
prefix `io.github.rossgraeber` is plainly not `io.modelcontextprotocol` and contains
neither `modelcontextprotocol` nor `mcp` as its second label — not by asserting a
reservation rule the source text does not contain. Asserting such a rule would repeat
the exact drift `REVERIFICATION-B2.md` D2 already caught in PLANNING-PROMPT.md §3.3 and
`oac-mcp`'s own Pin section flags as corrected.

## 5. MCP is not the delivery mechanism

Quoted verbatim (Source: PLANNING-PROMPT.md §3.3, retrieved 2026-09-15; re-verified
against the `2026-07-28` pin in `docs/planning/REVERIFICATION-B2.md` §3.3, retrieved
2026-09-16, verdict **HOLDS** by full-changelog review):

> "no MCP revision defines 'external event becomes a user turn'. That semantic is
> always provider-native."

Backing facts, each carrying its own re-verification verdict from
`docs/planning/REVERIFICATION-B2.md` §3.3 (all HOLDS at the `2026-07-28` pin,
retrieved 2026-09-16):

- No `initialize` handshake — removed at `2026-07-28` (SEP-2575).
- Every request carries `_meta["io.modelcontextprotocol/protocolVersion"]`.
- Servers cannot initiate requests or push unsolicited content into the model's context
  (server-initiated requests replaced by the multi-round-trip-results pattern on
  `tools/call`, `prompts/get`, `resources/read`).

**The actual delivery paths, named explicitly, are provider-native, not MCP:**

- Claude Code: `notifications/claude/channel`.
- Codex app-server: `thread/queue/add`, `turn/steer`, `turn/start`.

**Guard against a stale method name (Drift register D1).** Do not write
`resources/subscribe` or `resources/updated` for the current MCP era. Per
`docs/planning/REVERIFICATION-B2.md` Drift register D1: the `2026-07-28` changelog
removes and replaces `resources/subscribe`/`resources/unsubscribe` and the HTTP GET
endpoint with `subscriptions/listen`, a single long-lived stream with opt-in change
types (`toolsListChanged`, `promptsListChanged`, `resourcesListChanged`,
`resourceSubscriptions`), tagged with `io.modelcontextprotocol/subscriptionId`. This
document names no delivery-notification method by name for OAC's own use (delivery is
provider-native per above), but any future spec text that does reach for an MCP
change-notification mechanism at `2026-07-28` must name `subscriptions/listen`, not the
removed methods.

## 6. The `extensions` map vs `experimental`

**(a)** SEP-2133 formal extensions are negotiated through the `extensions` map in MCP
capabilities; breaking changes require a new identifier. Quoted verbatim (Source:
https://modelcontextprotocol.io/seps/2133-extensions, retrieved 2026-09-16, re-verified
HOLDS in `docs/planning/REVERIFICATION-B2.md` §3.3): "Breaking changes MUST use a new
identifier, e.g. `io.modelcontextprotocol/oauth-client-credentials-v2`." Negotiation
itself is through `ClientCapabilities.extensions` / `ServerCapabilities.extensions`
(same source, `### Negotiation`, re-verified HOLDS).

**(b)** Claude Channels use `capabilities.experimental["claude/channel"]` — a separate,
non-SEP-2133 mechanism, reachable only on the legacy MCP negotiation path (`2025-11-25`
or earlier), per the `MCP_PROTOCOL_NEGOTIATION` constraint recorded in
`docs/planning/PINS.md` (a channel server negotiating `2026-07-28` "can't deliver
channel messages, so Claude Code doesn't register it as a channel").

**(c)** The two are **parallel, not nested**. OAC declares the formal identifier
`io.github.rossgraeber/oac-session-channels` in the `extensions` map wherever that map
is available (the Codex tool path, §7), and separately declares
`capabilities.experimental["claude/channel"]` on the Claude legacy negotiation path
(§7). Neither substitutes for the other — a server does not get Claude channel delivery
by declaring the formal extension, nor does declaring `experimental["claude/channel"]`
satisfy SEP-2133 extension negotiation for a Codex-side client expecting the
`extensions` map.

Carrying `ADR-001-A3`'s guard (c) forward in effect: this document does **not** assert
that `capabilities.experimental` survives at MCP era `2026-07-28`. Whether
`experimental` capabilities still exist at `2026-07-28` is an **open UNVERIFIED item**
(`docs/planning/STATUS.md`, "Open UNVERIFIED items" — re-labelled from HOLDS to
UNVERIFIED in `docs/planning/REVERIFICATION-B2.md` §3.3, because the prior evidence cited
Claude Code's own client-capability declaration, not the `2026-07-28` schema itself, and
Claude Code never negotiates `2026-07-28` for a channel server). §6(b) above is scoped
to the legacy path precisely because of this open item, not despite it.

## 7. Dual-era / G4 tie-in

This packaging choice is what makes gate G4's "one process, two eras" claim testable,
not proven:

- The formal identifier `io.github.rossgraeber/oac-session-channels` rides the
  current-era Codex tool path — a server registered via `codex mcp add` (C2 §6) can
  negotiate `2026-07-28` and declare the identifier in its `extensions` map.
- `capabilities.experimental["claude/channel"]` rides the legacy Claude channel path —
  the same process, negotiating `2025-11-25` or earlier for that connection, per §6(b).
- The standalone spec document under `spec/` is era-independent: its normative text
  does not change depending on which MCP revision a given connection negotiated.

**This document does not claim G4 passed.** Every gate verdict is `NOT RUN` per
`docs/planning/STATUS.md` — G4 specifically: "One process serving both MCP eras.
Fallback: two entry points, one core." This packaging decision is what G4 will test,
not evidence that it already holds.

## 8. Versioning policy for the identifier

Per SEP-2133 (quoted in §6(a)): a breaking change to the OAC spec requires a **new
identifier**, e.g. `io.github.rossgraeber/oac-session-channels-v2` — the same pattern
SEP-2133's own example uses (`io.modelcontextprotocol/oauth-client-credentials-v2`). The
identifier is never silently reused across a breaking revision.

**Relationship to the standalone spec document's own version string.** The standalone
document under `spec/` (per §1, normative-of-record) carries its own version identifier,
governed by `oac-spec-authoring`'s versioning policy — that policy is not restated or
re-derived here. The relationship is one-directional for breaking changes: a breaking
change to the spec document's normative text is exactly the trigger that forces a new
extension identifier (this section); a non-breaking spec revision does not require a new
identifier, only a new spec document version.

**The identifier is frozen at the Stage 2 interface freeze** (`oac-spec-authoring`) —
once Stage 2 freezes the interface, `io.github.rossgraeber/oac-session-channels` is the
identifier every subsequent non-breaking spec revision negotiates under, until a
breaking change forces the `-v2` (or later) successor per this section.

## 9. Rejected alternatives

- **MCP-only, no standalone document.** Rejected: normative text would be hostage to MCP
  revision churn, and packaging OAC's semantics entirely inside MCP implies MCP delivers
  them — which no MCP revision does (§5).
- **Standalone document with no extension identifier.** Rejected: loses machine-readable
  capability negotiation and a stable `_meta` provenance-key prefix; every consumer would
  need out-of-band knowledge of OAC's presence instead of a declared identifier.
- **`com.example`-style or an unowned prefix.** Rejected: violates SEP-2133's
  domain-ownership SHOULD clause (§3) outright — `com.example` is explicitly a
  documentation placeholder in SEP-2133's own examples, not a real, owned domain.
- **Method-name namespacing convention.** Rejected: no SEP rule exists for namespacing
  custom method names — only `_meta` keys and extension identifiers carry prefix rules
  (`oac-mcp` Pin section; PLANNING-PROMPT.md §3.3). Inventing one here would be exactly
  the kind of unverified convention `oac-evidence` §3 prohibits.

## 10. Reversal condition

Reverse the identifier's prefix if any of the following fires:

- **The project acquires an owned apex domain.** A purchased domain (e.g. `oac.<tld>`)
  the project owns outright is a stronger, more direct satisfaction of SEP-2133's
  domain-ownership SHOULD than a GitHub-account-derived subdomain — the identifier would
  move to that domain's own reversed labels (§8's versioning policy then governs whether
  this is itself a breaking change requiring a new identifier, or a migration handled
  alongside one).
- **SEP-2133 supersession.** If SEP-2133 is superseded by a new extensions SEP with a
  different identifier format or ownership rule, this document's identifier scheme must
  be re-derived against the new SEP's text, not carried forward unexamined.
- **GitHub account rename.** Renaming the `RossGraeber` account invalidates
  `io.github.rossgraeber` as a controlled prefix (GitHub Pages' one-site-per-account
  allocation, §3, moves with the account's current name) and forces a new identifier
  under the renamed account's own reversed form.

None of these three has fired as of this document.

## 11. Boundary self-check

Per `oac-boundaries`:

- **No Zenoh vocabulary in the identifier, this document's prose, or the `_meta` key
  shape.** Confirmed by re-reading §1-§10: no Zenoh type, key expression, or liveliness
  term appears anywhere in this file. Quoted boundary line matched:
  `[ADR-001 Boundary]` "MUST NOT leak Zenoh-specific concepts into the neutral
  protocol." / DESIGN.md: "The specification MUST NOT mention Zenoh keys, MQTT topics,
  NATS subjects, or provider-specific method names." This document's spec-packaging
  prose and identifier are Zenoh-free, satisfying both.
- **Nothing in this document has MCP owning the turn loop.** §5 states the opposite
  explicitly and by verbatim quote: "no MCP revision defines 'external event becomes a
  user turn'." §7 names the actual delivery paths as provider-native. No section of this
  document assigns MCP a role beyond capability negotiation, tool surface, and `_meta`
  provenance (§1).

## 12. Surface labels and UNVERIFIED ledger

Per `oac-evidence` §4, one label per surface touched by this document:

| Surface | Label | Note |
|---|---|---|
| MCP `2026-07-28` (current era) | supported | unchanged from `docs/planning/PINS.md` |
| SEP-2133 extensions | supported | Final per PINS.md; `oac-mcp` Pin section |
| Claude Code Channels | research preview | unchanged from `docs/planning/PINS.md` |
| Codex app-server | experimental (per-method gating) | unchanged from `docs/planning/PINS.md` |

**SEP-2133 finalization-date discrepancy — already closed, not reopened here.**
PLANNING-PROMPT.md §3.3 states "final 2026-01-26"; the SEP-2133 page itself states only
`Created: 2025-01-21` with a "Final" badge and no separate finalization-date field. This
was investigated and **closed with no drift** in `docs/planning/REVERIFICATION-B2.md`
§3.3 carry-over (a): the SEP's own pull request (`modelcontextprotocol/
modelcontextprotocol` PR #2133) has `"merged_at": "2026-01-26T23:57:49Z"`, which is the
first-party finalization event and confirms `2026-01-26` directly — `Created` and
finalized are simply different fields on the SEP page. `docs/planning/STATUS.md`'s
"Closed in B2" list already records this closure. This document does not re-add it to
STATUS.md's open list, per `oac-evidence` §5's rule against letting a resolved fact exist
as if still open.

**`capabilities.experimental` at MCP era `2026-07-28` — existing open item, carried, not
newly added.** Whether `experimental` capabilities still exist at the current MCP
revision is already listed in `docs/planning/STATUS.md`'s "Open UNVERIFIED items" (see
§6 above for how this document scopes around it). No new UNVERIFIED item is introduced
by this document beyond what §4's guard and §6's carry-forward already name — both are
existing entries this document relies on and does not resolve.

## 13. Acceptance boxes, ticked against lines in this file

- [x] Choice stated with the SEP-2133 rules applied: identifier form
      `{reverse-dns-prefix}/{name}`, breaking changes require a new identifier — §1
      (choice), §2(c) (format, verbatim), §8 (breaking-change policy, verbatim).
- [x] Chosen prefix is one the project demonstrably owns; reserved second labels
      (`modelcontextprotocol`, `mcp`) avoided — §3 (ownership evidence, verbatim-cited),
      §4 (guard against asserting a reservation rule that does not exist; satisfied by
      factual non-collision).
- [x] The spec explicitly does not claim MCP as the delivery mechanism — no MCP revision
      defines 'external event becomes a user turn' — §5 (verbatim quote and backing
      facts), §7 (delivery paths named as provider-native).
- [x] Relationship between the formal `extensions` map and the `experimental`
      capabilities that Claude Channels use is stated — §6 (three-part statement: formal
      mechanism, Claude's separate mechanism, parallel-not-nested relationship).

## Where this folds in

**Superseded note (§0 of `docs/planning/v0.1/03-decisions-and-amendments.md`, issue
#21).** `docs/planning/v0.1/03-decisions-and-amendments.md` now exists and resolved the
fold-in tension this section originally anticipated in favor of **cite, not copy**: that
file is a synthesis that cites this document's sections rather than absorbing them
unedited, and this file is **not** a redirect stub — it remains the authoritative,
standalone source for decision 3's choice, evidence, and reversal condition.
`docs/planning/STATUS.md` was updated to the same effect.
