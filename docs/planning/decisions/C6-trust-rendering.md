# C6: Provider-facing trust rendering and outbound symmetry

**Issue:** #19 (Epic C, backlog key `C6`). **Depends on:** #12. **Source:**
PLANNING-PROMPT.md §5 decisions 8 and 9, §7; conflicts C9 and C10.

**Status:** Decided.

**Standalone-ledger note.** This file lives at `docs/planning/decisions/` because
`docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4) does not exist yet.
It folds into that file, unedited in substance, once A4 lands — mirroring
`docs/planning/ADR-001-AMENDMENTS.md`'s own standalone-ledger rationale (see that file's
opening paragraph) and `docs/planning/decisions/C5-envelope-auth.md`'s identical note.
`docs/planning/STATUS.md` carries a one-line pointer to this file until then.

**Provenance-proof caveat, stated once up front.** Per `docs/planning/STATUS.md`'s Gate
verdicts table, **gate G5 (Provenance) is `NOT RUN`**. This document designs the
provenance-rendering mechanism G5 will exercise; it does not assert that provenance is
rendered correctly, distinctly, or unforgeably end to end on either provider. Every claim
of the form "the mitigation is X" in §12's threat table is a **designed** mitigation, not
a **proven** one — the identical caveat `docs/planning/decisions/C5-envelope-auth.md`'s
opening paragraph and `docs/planning/decisions/C4-session-identity.md` §13 already state
for their own threat rows.

---

## 1. Choice

OAC's provider-facing trust rendering and outbound symmetry model is:

1. **Claude inbound:** provenance arrives as a fixed `meta` attribute set — five
   identifier-safe keys, never derived from content — attached to
   `notifications/claude/channel` (§2).
2. **Identifier-safety is enforced, not hoped for:** a const key table plus a contract
   test and a refusal fixture make a dropped key fail loudly rather than silently ship
   an unlabelled message (§3).
3. **Content and provenance never mix:** the Claude `content` string carries only the
   untrusted body; forged channel-tag-looking text inside it is neutralized, never
   trusted (§4).
4. **Codex inbound:** provenance rides inside the `{type:"text",text}` item as a
   machine-generated header, an unguessable per-message delimiter, then the untrusted
   body (§5).
5. **"Visibly distinct" means one thing on both providers:** machine-set, structurally
   separate from content, non-derivable, same field set across carriers (§6).
6. **Permission relay stays off by default in v0.1 — resolves C10** (§7).
7. **Outbound is symmetric and provider-neutral:** four OAC MCP tools, identical schemas
   on both providers, `whoami` never returns a credential (§8).
8. **Codex is reachable outbound via `codex mcp add`,** not the deleted
   `codex mcp-server`; the dual-MCP-era collision is flagged and deferred to C2/C3/gate
   G4, not re-decided here (§9).
9. **Codex reply correlation is layered, never trust-on-first-text — resolves C9** (§10).
10. **Receipt honesty on this path reuses C5 §9,** not restated (§11).

This is stated as the choice, not offered as one option among several.

## 2. Claude inbound: the `meta` attribute set

**A channel is an MCP server that declares `capabilities.experimental["claude/channel"] =
{}` and sends `notifications/claude/channel` with `content` (string) and `meta`
(string-to-string map).** Verbatim, `oac-claude-channels` §1, citing PLANNING-PROMPT.md
§3.1, retrieved 2026-09-15. Each `meta` key becomes an attribute on the `<channel>` tag
Claude sees.

**Identifier-safety constraint, verbatim.** Per `oac-claude-channels` §2: `meta` keys
"must be identifier-safe (letters, digits, underscore) or they are **silently
dropped** — no error, no warning, the attribute just does not appear." Pattern:
`^[A-Za-z0-9_]+$`.

**The fixed key set — five keys, all matching `^[A-Za-z0-9_]+$`:**

| Key | Value form | Cite |
|---|---|---|
| `oac_sender` | The claimed sender's OAC opaque session id — 128 bits (16 bytes) from the OS CSPRNG, 26-character lowercase Crockford Base32, no derivable structure. | `docs/planning/decisions/C4-session-identity.md` §1 leg 2 ("Opaque stable OAC session id... the address OAC's own protocol carries on the wire"), §2 (construction). |
| `oac_device` | The sending device's Ed25519 public-key fingerprint — the same fingerprint construction `security.key_id` uses (§4 below), never the display URI's `<device>` segment. | `docs/planning/decisions/C5-envelope-auth.md` §4 ("Public half is the device identity"), §10(b) (fingerprint construction, 128-bit floor). |
| `oac_session` | The **addressed** OAC opaque session id (the envelope's `to`), same construction as `oac_sender`. | `docs/planning/decisions/C4-session-identity.md` §2. |
| `oac_message_id` | The envelope's `id` field, unmodified. | `docs/planning/DESIGN.md`'s envelope shape, `"id": "unique-message-id"`, unchanged by this decision. |
| `oac_reply_to` | The envelope's `reply_to` field, unmodified, empty/absent when the message is not a reply. | Same envelope shape, `"reply_to": "optional-message-id"`. |

**Display URI is display-only — never a `meta` value.** Per `docs/planning/decisions/
C4-session-identity.md` §8: the display URI (`session://<device>/<harness>/<id>`) is
"display-only, stated in full... never parsed for authorization... never a routing key."
No `meta` key above carries the display URI; each carries the underlying opaque id or key
fingerprint the display URI is only a rendering of. A `<channel>` attribute is exactly the
kind of machine-consumed-adjacent surface C4 §8's "never parsed for authorization" rule
exists to keep display strings out of.

**Keys are ASCII constants — stated explicitly, per the task instruction.** All five keys
above are fixed string literals in OAC's own source, chosen at design time by this
document. **None is derived from message content, and none is derived from a
peer-supplied string** — not the sender's chosen id, not any header the peer's message
claims, not a value read out of the envelope's `content` field. `oac_sender`,
`oac_device`, and `oac_session` are values the *daemon itself* resolved from its own
registration/pairing state (`docs/planning/decisions/C4-session-identity.md` §5, §10) —
never values copied verbatim from an untrusted peer's claim about its own identity.

## 3. Identifier-safety enforcement

**The problem, stated once more precisely.** `oac-claude-channels` §2's silent-drop
behaviour means a bug that emits a non-conforming key (a typo, a future key added without
checking the pattern, an accidental content-derived key) does not error — it ships a
message to the model with **less** provenance than intended, and nothing in Claude Code's
own surface signals that. Left unmitigated, this is a silent downgrade from "provenance
present" to "provenance missing," which is exactly the gap §4's neutralization rule and
§6's rendering guarantee depend on not happening.

**Mechanism: a const key table, enforced at two points, neither optional.**

1. **Compile-time/const key table.** The five keys in §2 are declared once, as constants
   (not computed strings), in the single module that constructs `meta` maps for outbound
   Claude notifications. No code path is permitted to build a `meta` key by string
   interpolation, concatenation with peer-supplied data, or any form other than reading
   one of these five named constants.
2. **Contract test asserting every emitted key matches `^[A-Za-z0-9_]+$`.** A test in the
   Claude adapter's contract suite (`oac-testing`; the adapter contract suite G4/G5
   already require per their own acceptance criteria) constructs a `meta` map through the
   real code path and asserts, for every key present, that it matches the pattern above.
   This is a static property of the const table (step 1) and should never fail once the
   table is correct — its job is to catch a future regression the moment a sixth key is
   added without checking the pattern, not to catch anything dynamic.
3. **Refusal fixture: a dropped key is treated as missing provenance, and the message is
   refused delivery.** A fixture feeds the adapter a `meta` map with one key deliberately
   mutated to violate the pattern (for example `oac-sender` with a hyphen) and asserts two
   things: (a) the adapter's own pre-send validation detects that the resulting `meta` map
   would ship with fewer than the five required keys once Claude Code's silent-drop
   behaviour is applied, and (b) the adapter refuses to send the notification at all
   rather than deliver an under-labelled message. This is the mechanism that turns
   `oac-claude-channels` §2's silent, no-signal drop into a loud, fail-closed refusal on
   OAC's own side of the boundary — Claude Code's own behaviour is unchanged (it still
   drops silently if a bad key ever reached it), but OAC's adapter never lets a bad key
   reach it in the first place, and treats "would ship incomplete provenance" as a send
   failure, not a degraded send.

**This is the proving test §12's threat table cites** for the "silently dropped `meta`
key" row: the refusal fixture in step 3, plus the contract test in step 2.

## 4. Claude inbound: content-vs-provenance separation

**The exact shape, verbatim, per the task instruction.** `notifications/claude/channel`
carries `content` (string) and `meta` (string-to-string map) — quoted from
`oac-claude-channels` §1, itself citing PLANNING-PROMPT.md §3.1 verbatim.

**`content` carries the untrusted body only.** The `content` string is populated from the
envelope's own `content` field (`docs/planning/DESIGN.md`'s
`"content": [{"type":"text","text":"..."}]`) — rendered to plain text for the string the
Claude Channels surface accepts — and from nothing else. **No provenance value from §2's
`meta` set is ever duplicated into, prepended to, or interpolated into `content`.** A
`content` string that happened to also state "from: oac_sender=abc123" would defeat the
entire point of a separate `meta` channel: it would put a provenance-shaped claim inside
the one field this design treats as always untrusted.

**Forgery case: body text that looks like a `<channel>` tag or attribute syntax.** Claude
renders `meta` as attributes on a `<channel>` tag wrapping the delivered content
(`oac-claude-channels` §1-§2). An adversarial sender could compose a message body whose
text is literally `<channel oac_sender="attacker-controlled">` or similar
attribute-syntax-looking text, attempting to make the model read a forged provenance
claim out of the content it is shown. **Neutralization rule: the adapter treats the
`content` string as opaque text handed to Claude Code's own notification delivery path —
it is never parsed, re-templated, or concatenated by OAC's own code as if it might contain
markup that becomes live.** Claude Code's channel rendering, not OAC, is what actually
turns real `meta` key/value pairs into the `<channel>` tag's attributes; a string
inside `content` that merely *looks like* channel-tag syntax is not itself interpreted as
one by that rendering path, because `content` and `meta` are separate fields on the wire
notification and Claude Code's `<channel>` tag construction draws its attributes from
`meta`, never by scanning `content` for tag-like substrings (verbatim shape,
`oac-claude-channels` §1). OAC adds no code that would change that — no sanitization step
is needed to *neutralize* forged markup inside content, because forged markup inside
content was never live markup to begin with; what OAC's own doctrine forbids is treating
such text as a provenance signal if it were somehow rendered adjacent to real attributes.

**The attribute set is the only provenance the model may rely on — stated as the rule
itself.** Whatever a message's `content` claims about who sent it, only the `meta`
attribute set (§2) is machine-set and trustworthy; this is the per-provider instance of
§6's common rule and the direct analogue of `oac-security-work` §3's authenticated-but-
untrusted doctrine applied to rendering rather than authorization: content is never
trusted to state identity, no matter how it is phrased.

## 5. Codex inbound: text-input framing

**Codex has no meta channel.** Unlike Claude's `<channel>` tag attributes, Codex's
inbound surface has no side-channel metadata field — verbatim, `oac-codex-appserver`:
`turn/start` "takes an input array of `{type:"text",text}` items," and PLANNING-PROMPT.md
§3.2's identical framing. Provenance must ride inside the `text` string itself, since no
other field exists to carry it.

**Frame shape, three parts in the one `text` string:**

1. **Machine-generated header block** — the same five fields §2 names, same names
   (`oac_sender`, `oac_device`, `oac_session`, `oac_message_id`, `oac_reply_to`), one
   `key: value` line each, so a party reading both renderings sees the identical field
   set regardless of which provider is rendering it (§6's cross-provider identity
   requirement).
2. **An unguessable-per-message delimiter**, opening and closing the untrusted body
   section.
3. **The untrusted body** — the envelope's rendered `content`, between the two delimiter
   occurrences, then the closing delimiter.

```
--- oac-envelope <delimiter> ---
oac_sender: <opaque session id>
oac_device: <device key fingerprint>
oac_session: <addressed opaque session id>
oac_message_id: <envelope id>
oac_reply_to: <envelope reply_to, or empty>
--- oac-body <delimiter> ---
<untrusted content, rendered to plain text>
--- oac-end <delimiter> ---
```

**Delimiter derivation — unguessable per message, from `security.nonce`.** The delimiter
value is derived from the envelope's own `security.nonce` (`docs/planning/decisions/
C5-envelope-auth.md` §7: "128 bits (16 bytes) from the OS CSPRNG... a fresh nonce is
generated for every signed envelope; nonces are never reused across envelopes"). Because
the nonce is (a) CSPRNG-random, (b) unique per envelope, and (c) inside the signed field
set (`docs/planning/decisions/C5-envelope-auth.md` §5's signed-field-set table: "
`security.nonce`... if unsigned, an attacker could strip or substitute the nonce"), an
attacker composing the message body cannot predict the delimiter before the envelope is
signed, and cannot alter the nonce without invalidating the envelope signature. **This is
what makes it impossible for body content to forge a header or close the fence early:**
body text containing the literal string `--- oac-body ... ---` for a *different, guessed*
delimiter value does not match the *actual* per-message delimiter derived from that
envelope's own nonce, so it cannot be mistaken for a real section boundary by whatever
code parses the frame back out on the sending side, and — more importantly — a human or
downstream process reading the rendered text has no way to predict the delimiter in
advance well enough to plant a matching one in a message whose content the sender
(possibly the same untrusted peer) controls before the envelope is signed over that
content.

**Which call is used per thread state — citing the decision-rule table verbatim.** From
`oac-codex-appserver`'s "Live injection: how narrow it is, and the decision rule":

| Thread state | Call | Notes |
|---|---|---|
| Idle (no turn in flight) | `turn/start` | Starts a new turn. |
| A turn may be in flight and you want the input delivered once the thread goes idle | `thread/queue/add` | Experimental; queued until idle. |
| A turn is actively in flight | `turn/steer` | Appends into the *in-flight* turn. |

**`turn/steer` is not used on the inbound path.** The inbound delivery path this section
defines uses only `turn/start` and `thread/queue/add` from the table above — never
`turn/steer`. This is the G7 / `oac-security-work` §2 boundary, quoted: "`turn/steer` is
either unused or gated behind an explicit authorization check" — the framing this section
defines assumes the "unused" branch for ordinary inbound message delivery; if a future
decision routes any inbound content into `turn/steer`, that routing needs G7's own
separate authorization gate (`docs/planning/decisions/C5-envelope-auth.md` §11's
"message-delivery authorization and steer-authorization are two separate checks by
design"), not an implicit grant from this framing decision.

## 6. What "visibly distinct" means on each provider

One rule, satisfied by both renderings above even though the carrier differs:

**Provenance is machine-set, structurally separate from the content field, not derivable
from or overridable by content, and identical in field set across providers.**

- **Machine-set:** §2's `meta` values and §5's header-block values are both produced by
  OAC's own adapter code from its own registration/pairing state — never copied verbatim
  from anything a peer's message claims about itself (§2's "ASCII constants" rule, §5's
  nonce-derived, sender-unpredictable delimiter).
- **Structurally separate from content:** on Claude, a distinct wire field (`meta` vs
  `content`, §4); on Codex, a distinct, unguessably-delimited section of the one `text`
  string (§5) — the mechanism differs because Codex has no second field, but the
  separation property is the same.
- **Not derivable from or overridable by content:** §4's forgery-neutralization rule and
  §5's per-message delimiter both exist for the identical reason — a message body cannot
  manufacture a provenance-shaped claim the rendering treats as authoritative.
- **Identical field set across providers:** §2 and §5 name the same five fields, same
  names, on both.

**Ties to ADR-001.** Quoted: `docs/planning/ADR-001.md` "Security model" — "Provenance
must remain machine-enforced and distinct from message content." Both renderings above
are this sentence applied per-provider.

**Ties to G5's pass criterion.** Quoted, PLANNING-PROMPT.md §4: "A message whose text
claims a different sender is rendered to the model with machine-set provenance that
contradicts the claim, on both providers." This section's rule is exactly what G5
exercises — a message whose `content`/body text claims to be from someone else still
carries the real sender in `meta`/the header block, and the model sees both,
contradicting each other, rather than the forged claim silently winning. G5's verdict is
`NOT RUN` (this document's opening caveat); this section states the design G5 will test,
not a proven result.

## 7. Permission relay off by default — resolves C10

**OAC does not declare `capabilities.experimental['claude/channel/permission']` in
v0.1.** Verbatim key, `docs/planning/PINS.md` — "Claude Code Channels" pin record:
`capabilities.experimental['claude/channel/permission']`.

**Justification, three strands:**

**(a) C10's own finding.** `docs/planning/ADR-001-AMENDMENTS.md`'s conflict-register row
C10: "Permission relay lets any allowlisted sender approve tools." Quoted,
PLANNING-PROMPT.md §3.1: "Optional permission relay (`claude/channel/permission`,
v2.1.234+) lets any allowlisted sender approve or deny tool use in the session." An
allowlist entry (`docs/planning/decisions/C5-envelope-auth.md` §11) authorizes a sender's
*message* to be delivered — turning relay on would let that same authorization silently
extend to approving *tool calls*, collapsing two decisions C5 §11 deliberately keeps
separate.

**(b) The authenticated-but-untrusted doctrine.** Quoted, `oac-security-work` §3: "An
authenticated peer message is still an untrusted instruction and may carry prompt
injection. Authentication answers 'who sent it.' It never answers 'should this be
obeyed.'" A verified signature and a matched allowlist entry authorize *delivery into the
addressed session's input* — never an *action* the message's content requests. Permission
relay, if enabled, would make "who sent it, verified" stand in for "this tool call is
approved," exactly the collapse the doctrine forbids.

**(c) `--dangerously-load-development-channels` must not be weakened.** Verbatim flag,
`oac-security-work` §2: "the only user consent step on the Claude side until OAC is on an
allowlist; the plan must not weaken it." Leaving permission relay off by default keeps
that flag's confirmation as the one and only consent step a user grants when loading OAC
as a development channel — turning relay on by default would add a second, silent grant
(tool-approval authority for any allowlisted sender) behind the same single confirmation,
diluting what that confirmation actually consents to.

**Consequence of enabling it — H2's acceptance wording.** If a deployment turns relay on,
"any allowlisted sender for that session becomes able to approve tool use" —
`docs/planning/decisions/C5-envelope-auth.md` §13's threat-table row for this exact
attack, restated here because H2's own acceptance criterion is "Permission relay is
confirmed off, and the consequence of enabling it is documented"
(`docs/planning/backlog/05-tasks-GHIJ.json`, task H2) — this paragraph is that
documentation.

**Turning it on needs its own decision record.** This document fixes the v0.1 default; it
does not evaluate or design a permission-relay-on posture for any future deployment. A
future change to enable it is a separate, explicit decision — not a configuration flag
flipped inside this document's scope, and not a side effect of any allowlist grant.

**The floor is a chosen default, not an availability accident.** Per
`docs/planning/PINS.md`: "Floor 2 — permission relay (`claude/channel/permission`):
Claude Code `>= v2.1.234`," and the pinned version is `v2.1.274`, which satisfies that
floor — permission relay is *available* at the pinned version. This document's off-by-
default posture is therefore a decision made in spite of availability, not a limitation
imposed by it.

## 8. Outbound symmetry: the OAC MCP tool set

**Both harnesses send through OAC MCP tools; inbound is provider-native — this asymmetry
is deliberate.** Quoted, PLANNING-PROMPT.md §5 decision 9: "Both harnesses send through
OAC MCP tools (`send`, `reply`, `list_sessions`, `whoami`); inbound is provider-native."
Inbound delivery has no shared mechanism across providers (§2's `meta` attributes vs §5's
text framing are provider-specific by necessity — neither provider offers a neutral
inbound surface); outbound delivery does, because both providers can call an ordinary MCP
tool. The asymmetry mirrors what each provider actually offers, rather than inventing a
symmetric inbound mechanism neither provider supports.

**Four tools, identical argument and result schemas on both providers:**

- **`send`** — Arguments: `to` (OAC opaque session id or a resolvable target, per
  `docs/planning/decisions/C4-session-identity.md` §2), `content` (the envelope's own
  content shape, `[{"type":"text","text":"..."}]` per `docs/planning/DESIGN.md`'s
  envelope), `conversation_id` (optional), `correlation_id` (optional). Result: the
  envelope `id` assigned, plus the initial `DeliveryReceipt` state (`accepted-by-
  adapter`, `docs/planning/decisions/C5-envelope-auth.md` §9).
- **`reply`** — Arguments: `in_reply_to` (the message id being replied to — see §10 for
  how this is validated on the Codex path), `to` (optional; defaults to the original
  sender), `content` (same shape as `send`). Result: same shape as `send`'s result.
- **`list_sessions`** — Arguments: none required; an optional `working_directory` filter.
  Result: the list of sessions the caller is authorized to see — filtered by the same
  `working_directory`-scoped, default-deny allowlist `docs/planning/decisions/
  C5-envelope-auth.md` §11 already fixes, never an unfiltered directory of every session
  OAC knows about (the cross-project-disclosure threat, §12's table).
- **`whoami`** — Arguments: none. Result: the caller's **own** OAC session id and device
  identity (the opaque session id, `docs/planning/decisions/C4-session-identity.md` §2,
  and the device key fingerprint, `docs/planning/decisions/C5-envelope-auth.md` §4) —
  **never a credential.** Concretely: `whoami`'s result never contains the device's
  private Ed25519 key, never a bearer token, never anything that could itself be replayed
  to impersonate the caller — only identifiers that are already meant to be shared (the
  same session id that appears in every outbound envelope's `from` field, and the same
  fingerprint that already appears in `security.key_id`).

**Schema identity across providers, stated explicitly.** The four tools above are
registered with the same names, arguments, and result shapes whether the caller is a
Claude Code session or a Codex session — no provider-specific variant of `send` exists.
This is the concrete meaning of PLANNING-PROMPT.md §5 decision 9's "Both harnesses send
through OAC MCP tools," applied at the schema level.

## 9. Codex reachability via `codex mcp add`

**Confirmed outbound path.** Verbatim, PLANNING-PROMPT.md §3.2 / `oac-codex-appserver`:
"`codex mcp add` still registers **external** MCP servers that Codex can call as tools;
that is the supported outbound tool surface." §8's four tools (`send`, `reply`,
`list_sessions`, `whoami`) are exposed to a Codex session exactly this way — OAC is
registered as an external MCP server via `codex mcp add`, and Codex calls OAC's tools the
same way it calls any other registered MCP server's tools.

**`codex mcp-server` is not the path — flagged explicitly.** Verbatim,
`oac-codex-appserver`: "`codex mcp-server` (Codex acting as an MCP server) was deprecated
2026-08-20 and **deleted** 2026-09-05 — do not plan on it." That surface would have made
Codex itself an MCP *server* other processes could call into; it is unrelated to, and
does not substitute for, `codex mcp add`'s registration of *external* servers Codex calls
*as a client*. This document's outbound path uses only the latter.

**The dual-era collision, flagged, not re-decided.** Claude's channel server "must
negotiate `2025-11-25` or earlier or it is not registered as a channel" (§2's citation,
`oac-claude-channels` §4's "single most expensive trap"), while the Codex tool path "may
negotiate current MCP" (`2026-07-28`, PLANNING-PROMPT.md §5 decision 3's framing,
conflict C5 in the register). **The four tool schemas in §8 are era-invariant** — `send`,
`reply`, `list_sessions`, and `whoami`'s arguments and results are ordinary MCP tool
JSON-RPC shapes that do not depend on which MCP revision negotiated the connection; only
the *transport-level negotiation* differs per provider, not the *tool contract* this
document defines. **The process-topology resolution — whether one OAC MCP server process
serves both eras, or two entry points share one core — is gate G4's and decision tasks
C2/C3's job, not this document's.** This is the C5 conflict-register entry
(`docs/planning/ADR-001-AMENDMENTS.md`: "C5 | Claude channel path needs legacy MCP; Codex
tool path may negotiate current MCP | ASSIGNED"), explicitly out of scope here — this
document only fixes that the tool *schemas* §8 defines do not themselves force a
particular topology answer.

## 10. Codex reply correlation — resolves C9

**The core new design work.** Codex has no channel-tag convention for outbound replies —
quoted, `oac-codex-appserver`: "Codex has no channel-tag convention for outbound replies,
unlike Claude's `meta` echo-back pattern... [this] is planning work this skill does not
supply — do not invent a correlation scheme here." This section is that planning work.

**Layered rule, strongest first:**

**(a) Primary — explicit `in_reply_to`, model-instructed to echo `oac_message_id`
verbatim.** The `reply` tool (§8) takes an explicit `in_reply_to` argument. §5's header
block instructs the model, via the header's own `oac_message_id` field, to pass that
exact value back as `in_reply_to` when replying. This is the strongest signal because it
is explicit application data, not inferred from timing or thread state.

**(b) Secondary — independent adapter-side binding to thread id and in-flight turn id.**
The adapter does not rely solely on (a). It independently tracks, for each Codex thread
it delivered a message into, which envelope `id` was delivered as part of which turn —
bound to the thread id (`oac-codex-appserver`: "Thread ids are UUIDv7 strings and survive
restarts") and the turn id from `turn/started`, confirmed complete via `item/completed`.
**`item/completed` is authoritative, deltas are not** — verbatim, PLANNING-PROMPT.md §3.2
event list: "`item/completed` (authoritative)," `oac-codex-appserver`'s Events section
repeating the same authoritative/not-deltas distinction. When the `reply` tool is called
from within a given thread, the adapter's own thread-id/turn-id binding independently
identifies which inbound envelope(s) that turn's context plausibly replies to, without
depending on the model having echoed anything back correctly.

**(c) Tertiary — explicit downgrade, never silent attribution.** When (a) is absent (no
`in_reply_to` supplied) **and** (b) yields more than one candidate inbound message (the
thread received more than one OAC message before this turn's reply), the reply is
delivered **marked** as one of two explicit states:

- **correlation inferred** — (b) narrowed the candidates to exactly one plausible inbound
  message even without (a), and that single candidate is used, but the reply is tagged
  as inferred rather than asserted with the same confidence as (a)'s explicit match.
- **uncorrelated** — (b) still yields more than one candidate (or zero), and no reply
  target can be responsibly chosen; the reply is delivered without a resolved
  `reply_to`, explicitly marked uncorrelated rather than guessed.

**A model-supplied `in_reply_to` is untrusted content — validated against (b), not
trusted on its face.** Restating `oac-security-work` §3's doctrine at the correlation
layer: the model producing `in_reply_to` text is exactly the kind of content a malicious
or confused peer's injected instructions could manipulate, so (a) alone is never
sufficient. The adapter validates a supplied `in_reply_to` against (b)'s independently-
tracked thread-id/turn-id binding — does the claimed `oac_message_id` correspond to an
envelope the adapter itself actually delivered into this thread, at or before the turn
the reply tool call occurred in? **A mismatch downgrades, it does not override:** if (a)'s
claimed `in_reply_to` does not match anything (b) independently tracked for this thread,
the reply falls through to (c)'s tertiary rule (inferred, if (b) alone narrows to one
candidate; uncorrelated otherwise) — the mismatched claim is never used as if it were
correct, and it never causes the adapter to fabricate a correlation that (b)'s own state
does not support.

## 11. Receipt honesty on this path

Reuses `docs/planning/decisions/C5-envelope-auth.md` §9's three states
(`accepted-by-adapter`, `handed-to-harness`, `unknown`) unchanged — not restated here. A
resolved Claude notification send on this path (§2's `meta`-bearing
`notifications/claude/channel`) is `handed-to-harness`, **never** "seen by the model": no
acknowledgement exists for it, exactly as C5 §9's per-provider observability paragraph
already states for Claude. The Codex path's observable point (a successful `turn/start` /
`thread/queue/add` RPC response, §5) is likewise `handed-to-harness` in the same sense C5
§9 already fixes for Codex — this document adds no new receipt state and no new honesty
rule; it only confirms that the framing decisions in §2 and §5 do not change what is
observable, only what is rendered once delivery succeeds.

## 12. Threat table

Per `oac-security-work` §1's template, all five columns, no blank "proving test." C6-owned
threats — provider-facing rendering and outbound-correlation threats — are not duplicated
from `docs/planning/decisions/C5-envelope-auth.md` §13 (envelope-authenticity-level
threats) or `docs/planning/decisions/C4-session-identity.md` §13 (identity-level threats).

| Attack | Precondition | Mitigation | Proving test | Residual risk |
|---|---|---|---|---|
| Provenance spoofing via message body on Claude | Attacker controls a validly-signed envelope's `content` text and crafts it to look like a `<channel>` tag or forged `meta`-shaped claim | `content`/`meta` are separate wire fields on `notifications/claude/channel`; `meta` is machine-set from daemon state, never content-derived (§2, §4) | G1 (Claude wake, exercises real rendering); G5 (provenance) | G1/G5 `NOT RUN`; designed, not proven, mitigation |
| Same, on Codex via forged header/delimiter | Attacker crafts a message body containing text shaped like §5's header block or delimiter, hoping to fool a reader (human or downstream parser) into treating it as the real header | Delimiter is derived from `security.nonce`, CSPRNG-random and inside the envelope's signed field set — unpredictable and unforgeable before signing (§5) | G2 (Codex live inject); G5 (provenance) | G2/G5 `NOT RUN`; residual risk if a future implementation ever parses the header/body split from unsigned text rather than from the signed envelope directly — this design assumes the split is reconstructed from the signed envelope, not re-parsed from delivered text, and that assumption is not yet test-enforced |
| Silently dropped `meta` key yielding unlabelled provenance | A bug emits a non-identifier-safe `meta` key | Const key table (§3) plus the refusal fixture: incomplete provenance is detected pre-send and the message is refused, not delivered unlabelled (§3) | Contract test (§3 step 2); refusal fixture (§3 step 3) | Both tests do not exist yet (`NOT RUN`, no implementation); moves to the risk list until F8/G4 build them |
| Unauthorized tool approval via permission relay | `claude/channel/permission` is enabled for a deployment | Off by default in v0.1 (§7); enabling it requires its own separate decision, never a side effect of allowlisting | backlog task G4 acceptance ("Permission relay is off by default"); H2 acceptance ("Permission relay is confirmed off... documented") | G4/H2 `NOT RUN`; a deployment that opts in accepts the documented consequence (§7) — this is an accepted, named risk for that deployment, not eliminated by this table |
| Reply misattribution via forged `in_reply_to` | Attacker's message content instructs the model to claim a specific, incorrect `in_reply_to` value | (a) alone is never trusted; validated against (b)'s independently-tracked thread-id/turn-id binding; a mismatch downgrades to inferred/uncorrelated, never silently accepted (§10) | G8 (Codex adapter reply-correlation acceptance, "tested against a reply that omits the correlation hint") | G8 `NOT RUN`; whether Codex reliably reproduces a header-supplied id in a tool call at all is itself UNVERIFIED (§13) — a spike, not this document, resolves it |
| Unauthorized `turn/steer` | Attacker's message is delivered to a Codex session | Inbound path uses only `turn/start`/`thread/queue/add`, never `turn/steer` (§5); any future steer routing needs G7's own separate authorization gate, restated from `docs/planning/decisions/C5-envelope-auth.md` §11 | G7 (per `oac-security-work` §2's citation); G2 | G7/G2 `NOT RUN`; this document does not itself build the steer gate, it only confirms the inbound framing path never calls `turn/steer` |
| Cross-project disclosure via `list_sessions` | Two sessions exist under different `working_directory` values; a caller invokes `list_sessions` | Result filtered by the same `working_directory`-scoped, default-deny allowlist `docs/planning/decisions/C5-envelope-auth.md` §11 already fixes (§8) | H2 (fourth acceptance item, per `oac-security-work` §2) | H2 `NOT RUN`; same open item `docs/planning/decisions/C4-session-identity.md` §13 and `docs/planning/decisions/C5-envelope-auth.md` §13 already name for this exact threat class |

Every row names its proving test; none is marked mitigated without one, per
`oac-security-work` §1's rule. Because every named test's current verdict is `NOT RUN` or
the underlying task is not yet built, every row above describes a **designed** mitigation,
matching the caveat stated at the top of this document.

## 13. Rejected alternatives

- **Provenance inside the Claude `content` string.** Rejected: this is exactly the
  collapse §4 forbids — a content-embedded provenance claim is indistinguishable, to any
  downstream reader, from a forged one, and defeats the entire point of `meta` existing
  as a separate wire field the harness itself, not OAC's own text, renders as
  attributes.
- **A fixed static delimiter for the Codex body.** Rejected: a static, predictable
  delimiter (e.g. a hardcoded string every OAC message uses) can be guessed and
  pre-planted inside a message body before that body is even signed, letting an attacker
  craft content that closes the fence early or forges a second header — the entire
  unguessability property §5 relies on requires a fresh, unpredictable, signed-scope
  value per message, which only a nonce-derived delimiter provides.
- **Trusting model-echoed correlation ids alone.** Rejected: this is (a) alone, without
  (b) or (c) — §10 explicitly builds the layered rule because a bare "trust
  `in_reply_to`, no independent check" design is exactly what "a model-supplied
  `in_reply_to` is untrusted content" (§10) forbids; it would make reply correlation, and
  therefore who a reply is delivered as-if-responding-to, entirely dependent on text a
  potentially-injected model turn produced.
- **Permission relay on by default behind an allowlist.** Rejected: this is C10's own
  named attack (§7(a)) — an allowlist authorizes message delivery, not tool approval, and
  "on by default, gated only by the same allowlist that already gates delivery" is
  precisely the collapse of two separate authorization decisions §7(b)'s doctrine
  citation forbids.
- **A Codex-side channel-tag convention invented by OAC.** Rejected: Codex has no
  documented channel-tag convention (§10's opening citation), and inventing one that
  Codex itself does not recognize or render specially would be an unsupported surface —
  quoted, `oac-boundaries` boundary 4: "MUST NOT depend on... undocumented private RPCs
  for supported integrations," and boundary 13: "If a supported interface is missing,
  that is a finding, not a workaround." §5's header-and-delimiter framing works entirely
  within the one documented surface Codex does offer (`{type:"text",text}` items) rather
  than asking Codex to recognize a tag syntax it was never designed to parse.

## 14. Reversal condition

**Claude half.** Reverse §2-§4's `meta`-attribute design if Claude Code ships a
documented structured-provenance surface beyond the current `meta`-on-`<channel>-tag`
mechanism — for example, a typed provenance object distinct from the string/map pair
this decision is built against. Test: does a future `channels-reference.md` (or
equivalent first-party doc) at a newer pinned version describe a `notifications/claude/
channel` payload shape different from `content` (string) plus `meta` (string-to-string
map)? Not fired as of this pin (`v2.1.274`).

**Codex half.** Reverse §5's header-and-delimiter framing if Codex gains a native
channel-tag convention — a documented field or item type on `turn/start`/
`thread/queue/add` that carries structured metadata separate from the `text` payload, the
Codex-side analogue of Claude's `meta`. Test: does a future `app-server` schema artifact
(`codex-rs/app-server-protocol/schema/json`, per `oac-codex-appserver`) add such a field
to the turn-input item shape? Not fired as of this pin (`@openai/codex@0.154.0`).

**Either half.** Reverse if G5 fails on either rendering — that is, if a live run finds a
message whose text claims a different sender is **not** rendered to the model with
machine-set provenance that contradicts the claim, on the provider that failed. G5's
verdict is `NOT RUN`; this reversal condition is not yet evaluable.

Neither half has fired as of this document.

## 15. Surface labels and UNVERIFIED ledger

Per `oac-evidence` §4, one label per surface touched by this document, at first mention:

| Surface | Label | Note |
|---|---|---|
| Claude Code Channels (`notifications/claude/channel`, `claude/channel/permission`) | research preview | pinned `v2.1.274`, unchanged from `docs/planning/PINS.md`; referenced throughout §2-§4, §7 |
| Codex app-server (`turn/start`, `thread/queue/add`, `turn/steer`) | experimental (per-method gating via `capabilities.experimentalApi`) | pinned `@openai/codex@0.154.0` / commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`, unchanged from `docs/planning/PINS.md`; referenced throughout §5, §9-§10 |

**Carried, still-open shim-boundary inputs (PINS.md risk items 9 and 10):** the
compatibility shim boundary for the Claude Channels research-preview surface and for the
Codex experimental live-inject surface are both `shim boundary: UNNAMED — see
DESIGN.md`, unchanged by this document. Per `docs/planning/STATUS.md` "Open UNVERIFIED
items": "The named compatibility shim boundary for the Claude Code Channels preview
surface (UNVERIFIED — DESIGN.md names no such module...)" and the identical Codex row.
This document does not name either boundary — it is a C-series-decision-or-DESIGN.md-
update item, per that same STATUS.md entry, and remains open.

**New UNVERIFIED item this document creates**, added to `docs/planning/STATUS.md`'s "Open
UNVERIFIED items" list in the same change (`oac-evidence` §5 — never recorded in only one
place):

- Whether Codex reliably reproduces a header-supplied id (§5's `oac_message_id` header
  field) in a subsequent `reply` tool call's `in_reply_to` argument is not established by
  any first-party doc (UNVERIFIED — this is a model-behaviour question about whether the
  Codex model, given text instructions inside the header block, actually echoes the id
  back reliably enough for §10(a)'s primary correlation path to fire in practice; resolved
  only by a spike exercising real Codex turns against the §5 framing, not by reading
  documentation further — §10(b)'s independent thread-id/turn-id binding exists
  specifically because this cannot be assumed).

No existing UNVERIFIED item this document relies on (Claude resume behaviour, Codex
daemon-attach runtime behaviour, the two shim boundaries above, etc.) is resolved or
reopened by this document — each is inherited unchanged from `docs/planning/STATUS.md`'s
existing list via the citations above.

## 16. Acceptance boxes, ticked against lines in this file

Per issue #19's six acceptance boxes (backlog task C6, `docs/planning/backlog/
03-tasks-CD.json`):

- [x] Claude: the `meta` attribute set for sender, device, session, message id, and
      reply target, with identifier-safe keys (non-conforming keys are silently
      dropped) — §2 (the five-key table, value forms, identifier-safety pattern), §3
      (the enforcement mechanism that makes a drop fail loudly on OAC's own side).
- [x] Codex: text-input framing with a machine-generated header and a delimited
      untrusted body — §5 (header block, nonce-derived delimiter, framed example,
      decision-rule table, `turn/steer` exclusion).
- [x] On both providers, trusted provenance is machine-set and visibly distinct from
      user- and model-controlled content — §4 (Claude content-vs-provenance separation
      and the forgery-neutralization rule), §6 (the common cross-provider rule and its
      ADR-001/G5 ties).
- [x] Claude permission relay (`claude/channel/permission`) off by default in v0.1, with
      justification (resolves C10) — §7 (the three-strand justification, the
      consequence-if-enabled paragraph, the floor-vs-default distinction, the C10
      conflict-register disposition).
- [x] Outbound is symmetric through OAC MCP tools (`send`, `reply`, `list_sessions`,
      `whoami`); confirmed reachable for Codex via `codex mcp add` — §8 (the four tool
      schemas, argument/result shapes, `whoami`'s never-a-credential rule), §9
      (`codex mcp add` confirmation, the deleted `codex mcp-server` distinction, the
      dual-era collision flagged and deferred).
- [x] Codex reply correlation defined explicitly, since Codex has no channel-tag
      convention (resolves C9) — §10 (the layered (a)/(b)/(c) rule, the untrusted-
      `in_reply_to`-validated-against-independent-state rule, the mismatch-downgrades-
      not-overrides statement, the C9 conflict-register disposition).

## 17. Cross-file updates in this change

- `docs/planning/ADR-001-AMENDMENTS.md`: conflict-register row **C9** moves from
  `ASSIGNED` to `RESOLVED-IN-DECISION`, resolution site `docs/planning/decisions/
  C6-trust-rendering.md` §10; conflict-register row **C10** moves from `ASSIGNED (v0.1
  default already fixed)` to `RESOLVED-IN-DECISION`, resolution site `docs/planning/
  decisions/C6-trust-rendering.md` §7. No new `ADR-001-A` amendment is issued for either
  — `docs/planning/ADR-001.md` carries no text about Codex correlation or about the
  `claude/channel/permission` default that needs correction (checked directly: neither
  string appears in `ADR-001.md`), matching the identical test `docs/planning/decisions/
  C5-envelope-auth.md` §6 and `docs/planning/decisions/C4-session-identity.md` §8 already
  apply for C4/C6/C8. The conflict-register legend's `RESOLVED-IN-DECISION` definition
  (added by C4 §8, reused by C5 §18) is reused here, its third use.
- `docs/planning/STATUS.md`: add a "C6 — provider-facing trust rendering and outbound
  symmetry (issue #19): decided" bullet under "Decisions landed," alongside the existing
  C1-C5 pointers; bump "Last updated"; add the one new UNVERIFIED item from §15 to "Open
  UNVERIFIED items"; update the "ADR amendments" summary paragraph to note C9/C10 are now
  closed the same way C4/C6/C8 already are (`RESOLVED-IN-DECISION`, no A-amendment).
- `docs/planning/v0.1/06-security.md` (Epic A task A7): named fold-in target — this
  document's §2-§7 (provenance rendering per provider, permission-relay default and
  justification) and §12 (threat table) are exactly what that eventual file's
  "provenance rendering per provider" and "threat table per §7" sections (per
  PLANNING-PROMPT.md §9 item 7) will draw from.
- `docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4): named fold-in
  target for this document's entire substance, per this document's own "Where this folds
  in" section below and the standalone-ledger note at the top.
- **Downstream implementation and verification items, named:** backlog task **G4**
  (Claude adapter — "Permission relay is off by default" is G4's own acceptance
  criterion, built against §7's default here); backlog task **G8** (Codex adapter
  outbound/correlation — "Correlation mechanism from the C9 decision implemented and
  tested against a reply that omits the correlation hint" is G8's own acceptance
  criterion, built against §10's layered rule here); backlog task **H2** (security
  verification — "Permission relay is confirmed off, and the consequence of enabling it
  is documented" is H2's own acceptance criterion, verified against §7 here).

## Where this folds in

Once `docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4) exists, this
file's content moves there unedited in substance (per PLANNING-PROMPT.md §9's output
package shape) and this file becomes a redirect stub, mirroring how
`docs/planning/ADR-001-AMENDMENTS.md`, `docs/planning/decisions/C4-session-identity.md`,
and `docs/planning/decisions/C5-envelope-auth.md` already describe their own eventual
fold-in. Consumed by: G4 (Claude adapter inbound delivery and provenance rendering, §2-
§4), G8 (Codex adapter outbound tool surface and reply correlation, §8-§10), G5 (the
provenance gate this document's whole rendering design ultimately feeds, §6, §14), G7
(`turn/steer` authorization gate, §5's exclusion and §12's threat row), H2 (permission-
relay and cross-project-disclosure verification, §7, §12), F8/E7 (channel-server/spec
groundwork G4 depends on, per that task's own `depends` list), and Epic A task A7's
`docs/planning/v0.1/06-security.md` (the normative-adjacent landing site named in §17).
