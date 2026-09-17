# C7: Zenoh transport mapping and its containment boundary

**Issue:** #20 (Epic C, backlog key `C7`). **Depends on:** B2. **Source:**
PLANNING-PROMPT.md §5 decision 10; DESIGN.md §Zenoh transport, §Addressing,
§Presence/discovery, §Security, §Suggested repository shape.

**Status:** Decided.

**Standalone-ledger note.** This file lives at `docs/planning/decisions/` because
`docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4) does not exist yet.
It folds into that file, unedited in substance, once A4 lands — mirroring
`docs/planning/ADR-001-AMENDMENTS.md`'s own standalone-ledger rationale (see that file's
opening paragraph) and `docs/planning/decisions/C4-session-identity.md`,
`docs/planning/decisions/C5-envelope-auth.md`, `docs/planning/decisions/
C6-trust-rendering.md`'s identical notes. `docs/planning/STATUS.md` carries a one-line
pointer to this file until then.

**Gate-verdict caveat, stated once up front.** Per `docs/planning/STATUS.md`'s Gate
verdicts table, **gate G3 (Zenoh local peer) is `NOT RUN`**. This document designs the
transport mapping G3 will exercise — key-expression layout, presence, local-mode and
LAN-mode security, and the containment boundary — it does not assert that loopback
discovery, presence liveliness, or either security profile works end to end on any
platform. Every claim of the form "the mitigation is X" in §9's threat table is a
**designed** mitigation, not a **proven** one, exactly as `docs/planning/decisions/
C4-session-identity.md` §13, `docs/planning/decisions/C5-envelope-auth.md` (opening
paragraph), and `docs/planning/decisions/C6-trust-rendering.md` (opening paragraph)
already state for their own threat rows.

---

## 1. Choice

OAC's Zenoh transport mapping and containment boundary are:

1. **Every Zenoh-specific type, identifier, and concept lives under `transports/
   zenoh/` and nowhere else** — one enforceable sentence, with the lint's literal grep
   form given so it is mechanically checkable (§2).
2. **Key expressions are private, derived, and never guessable** — computed inside the
   transport from the opaque session id, one-way on the wire, and never appear in the
   envelope, `_meta`, a receipt, or spec text (§3).
3. **Presence maps onto liveliness tokens with history-capable liveliness subscribers**,
   never a polling loop; the three neutral states (`online`/`unreachable`/`unknown`) are
   fixed against exactly what a missing token, an expired token, and a never-seen peer
   produce (§4).
4. **Local mode binds to `127.0.0.1`, runs no `zenohd`, and leaves multicast scouting
   on** by default, because the pinned Zenoh release satisfies the loopback-discovery
   floor; the G3 fixed-rendezvous-endpoint fallback is the named reversal path if
   scouting proves unreliable (§5).
5. **LAN mode defaults to TLS**, with QUIC named as the alternative, both using
   pairing-issued certificates from C5 §10(b)'s short-code flow; default-deny is the
   starting ACL posture (§6).
6. **ACL subjects are certificate common name or username only, never `zid`** — this
   document finalizes what C5 §12 deferred: the Zenoh ACL is a coarse pre-filter, the
   envelope signature is the authenticity proof, and C7 adds the key-expression scoping
   of ACL rules, the local-vs-LAN profile split, and the certificate-issuance wiring C5
   §12 left open (§7).
7. **Only the stable `zenoh` and `zenoh-ext` API surface is used — no `unstable`
   feature** — every feature this transport touches is enumerated against that claim
   (§8).

This is stated as the choice, not offered as one option among several.

## 2. The containment boundary — `transports/zenoh/`

**The module, named per DESIGN.** `docs/planning/DESIGN.md`'s Suggested repository
shape lists `transports/zenoh/` as a top-level directory alongside `core/`, `cli/`,
`adapters/claude/`, `adapters/codex/` (DESIGN.md line 143). This document fixes it as
*the* Zenoh boundary module — not a name this document invents, but the one DESIGN.md
already sketches, now made load-bearing.

**One enforceable sentence.** `zenoh`/`zenoh-ext` types, key expressions, `zid`, config
structs, session handles, and liveliness terms exist only under `transports/zenoh/`;
everything that crosses the boundary out of that module is exactly the `Transport`
contract's operations — `publish`, `subscribe`, `announce_presence`, `watch_presence`,
`health`, `shutdown` (DESIGN.md lines 60-65; `start` at line 59 is the same contract's
entry point, listed here for completeness though outside the cited line range) — over
neutral types only: the opaque session id (`docs/planning/decisions/
C4-session-identity.md` §2), the envelope (`docs/planning/DESIGN.md`'s envelope shape),
presence state (§4 below), and `DeliveryReceipt` (`docs/planning/decisions/
C5-envelope-auth.md` §9). Quoted, DESIGN.md line 70: "Zenoh types must not escape the
transport module." Quoted, `[ADR-001 Boundary]`: "MUST NOT leak Zenoh-specific concepts
into the neutral protocol." Quoted, `oac-zenoh` §8 (the containment rule): "key
expressions, `zid`, config structs, and session handles stay inside `transports/
zenoh/`. A `core/` type, the spec text under `spec/`, or a provider adapter must never
reference any of them."

**The lint's literal form — cross-referencing `oac-boundaries`, not inventing a second
grep set.** `oac-boundaries` `references/mechanical-checks.md` check 1 is this
boundary's mechanical enforcement, quoted verbatim rather than restated:

```bash
# 1. Zenoh vocabulary must not appear in the neutral spec or core types.
rg -n --glob '!target' -i '\bzenoh\b|\bzid\b|key[_-]?expr|liveliness' spec/ core/
```

A clean run is zero hits against `spec/` and `core/`; a hit anywhere outside
`transports/zenoh/` (and this planning prose, which is explicitly allowed to discuss
Zenoh in its own words per `docs/planning/decisions/C4-session-identity.md` §5's
identical carve-out) is a boundary violation, not a style nit. Per that same reference
file, the check reports a missing-path error today because `spec/`, `core/`, and
`transports/zenoh/` do not exist yet — **pending**, not passing; it must be re-run once
code lands at those paths (`oac-boundaries` "Mechanical checks").

**No identifier containing `zenoh`/`zid`/`key_expr` in `core/`, `spec/`, `adapters/`,
`cli/`.** This is the same check-1 pattern applied to the four consuming directories
named in the task instruction, not a new pattern: `-i '\bzenoh\b|\bzid\b|key[_-]?expr|
liveliness'` already matches an identifier such as `zenoh_key_expr` or `session_zid`
wherever it appears in text, including inside a Rust identifier, because ripgrep's
default word-boundary match (`\b`) still fires inside `snake_case` and `camelCase`
tokens at the substring boundary the pattern names. No second grep set is written here.

**A test asserting the module's public API signature set.** Once `transports/zenoh/`
exists (Stage 3, `oac-implementation`), its public surface is exactly the six
`Transport` operations named above, over the four neutral types named above — a
contract test constructs (or type-checks, depending on the implementation language's
own test idiom) the module's public function/method signatures and asserts each
parameter and return type is one of: an opaque session id, an envelope, a presence
state, a `DeliveryReceipt`, `config` (an opaque config handle passed to `start`, itself
never inspected outside the module), or a primitive (string/bool/duration) that carries
no Zenoh-specific meaning. This is the test the task instruction names; it does not yet
exist because `transports/zenoh/` does not yet exist (Pre-Stage 0, `docs/planning/
STATUS.md` "Current stage") — recorded here as the design the eventual Stage 3
implementation task must build against, the same forward-looking status §9's threat
table gives every row in this document.

## 3. Key-expression layout

**The public wire/display identifier stays C4's.** The opaque session id (`docs/
planning/decisions/C4-session-identity.md` §2) is what the wire protocol addresses
with, and `session://<device>/<harness>/<id>` (C4 §8) is what a human sees. Neither
changes here. Quoted, C4 §2: "the transport module maps between them internally
(`oac-zenoh`), and that mapping never runs in reverse on the wire" (C4-session-identity.md
lines 78-79, immediately following the "No broker-native address" bullet opening at
line 74).

**The key expression is computed inside the transport, from the opaque session id, by a
one-way function — never the reverse.** Concretely: `transports/zenoh/` derives a
publish/subscribe key expression as `oac/1/` followed by a fixed-width hex encoding of
the first 128 bits of a cryptographic hash (a SipHash/BLAKE3-class one-way function —
the exact primitive is a Stage 3 implementation detail, not fixed here) computed over
the opaque session id's own 128 bits of CSPRNG entropy. Because the opaque id itself is
already 128 bits of CSPRNG output (C4 §2) — not a sequential, timestamp-based, or
otherwise enumerable value — a party that does not already know a specific session's
opaque id cannot guess or enumerate that session's key expression by pattern-matching,
scanning, or trying nearby values: it would have to already possess the id, which is
exactly the "no broker-native address... publicly" property C4 §2's acceptance box
(lines 67-68, "Session ids are opaque and stable; broker-native addresses are never
exposed publicly") already establishes for the id itself. The hash step exists for the
**one-way** property C4 §2 lines 78-79 require: observing a key expression on the wire
(or in a Zenoh ACL rule, a log line, or router state, in LAN mode) never lets a party
recover the opaque session id that produced it, because the hash cannot be inverted.
This is the concrete mapping C4 §2 forward-references and leaves to this document to
define.

**Never appears outside the transport module.** The key expression string is never
written into the envelope (`docs/planning/DESIGN.md`'s envelope shape has no field for
it), never into a `meta` value or the Codex header block (`docs/planning/decisions/
C6-trust-rendering.md` §2, §5 — neither's fixed field set includes anything
transport-addressed), never into a `DeliveryReceipt` (`docs/planning/decisions/
C5-envelope-auth.md` §9's three states carry no address), and never into the spec text
under `spec/` (§2's containment rule, above). This is §2's boundary applied at this
specific layer, not a separate rule.

**No broadcast/room-shaped pattern.** The key-expression layout above is per-session
(one leaf key expression per opaque session id) with no wildcard-matched group,
mailing-list, or "room" pattern (e.g. no `oac/1/project/*` fan-out key a set of sessions
all subscribe to). Quoted, `oac-boundaries` boundary 11: "Group rooms/broadcast...are
explicitly deferred, not v0.1 work." A per-session key expression, addressed only by
publish-to-that-exact-key and subscribe-to-that-exact-key, is what makes point-to-point
delivery to a specific opaque session id (the only addressing mode C4 defines) the only
thing this layout can express — there is no key pattern in this design a v0.1 broadcast
feature could piggyback on without a new decision.

## 4. Presence mapping

**Neutral states, from DESIGN.** Quoted, DESIGN.md line 105: "Start with `online`,
`unreachable`, `unknown`; avoid over-normalizing provider-specific activity states."
These three states are the only presence vocabulary that crosses §2's boundary; the
Zenoh liveliness mechanism that produces them is internal to `transports/zenoh/`.

**Mechanism: liveliness tokens plus history-capable liveliness subscribers.** Quoted,
`oac-zenoh` §4: "Primitives: pub/sub, queryables, liveliness tokens with history-capable
liveliness subscribers." A session's transport declares a `LivelinessToken` (stable API,
`zenoh::liveliness::LivelinessToken` — verified directly against source, §8 below) bound
to that session's key expression (§3) when it starts, and lets the token's own lifetime
— tied to the declaring session's `zenoh::Session` — express liveliness; other peers
watch it via a `LivelinessSubscriberBuilder` (`zenoh::liveliness::Liveliness`'s
subscription builder, also stable, §8 below) with `.history(true)` set, so a subscriber
that starts *after* a token was already declared still learns about it, not only about
tokens that appear after the subscription begins.

**The three-way mapping, stated explicitly — this table lives inside the transport
module only, per §2's boundary:**

| Zenoh-observed condition | Neutral presence state |
|---|---|
| A liveliness token for the session's key expression is currently declared and the subscriber has observed it (via history replay or a live PUT event) | `online` |
| A liveliness token that was previously observed has since been undeclared (the token's owning `zenoh::Session` closed, or the token was explicitly undeclared) — a DELETE-shaped liveliness event fires | `unreachable` |
| No liveliness token for that key expression has ever been observed by this subscriber — no history replay hit, no live event — and there is no other record the peer expects a token to exist | `unknown` |

An **expired token** and a **missing token** are, per the row above, the same
`unreachable`/`unknown` split by observation history, not a fourth state: a token this
subscriber previously saw and has now lost (undeclared/session-closed) is
`unreachable`; a token this subscriber has never seen at all, for a key expression it
has no other reason to expect activity on, is `unknown`. A **never-seen peer** (no
registration record ever existed for that opaque session id, from this transport's own
point of view) is also `unknown` — the transport cannot distinguish "peer exists but its
token was never observed" from "no such peer" at the liveliness layer alone; that
distinction, if needed, is answered by the registration record (`docs/planning/
decisions/C4-session-identity.md` §5), a neutral-layer concern outside this document's
transport mapping.

**No polling — the liveliness subscriber stream only.** Quoted, DESIGN.md line 110:
"Provider-supported streaming connections, subscriptions, event loops, and keepalives
are acceptable; application-level inbox polling is not for adapters claiming active
inbound support." Quoted, `oac-boundaries` boundary 14: "the Claude channel notification
path is flaky in testing, so the adapter adds a background loop that polls an MCP
resource every second... Either fix real delivery, turn off the active-inbound claim, or
file a finding — do not poll silently." `watch_presence` (DESIGN.md's `Transport`
contract, line 63) is implemented as a live liveliness-subscriber callback/stream over
the primitive named above, never a periodic `get`/query loop issued on a timer against
the liveliness key space. `zenoh-ext`'s `LivelinessSpace`/`UserSpace`/`KeySpace` structs
are noted here only because they surfaced during this research and are themselves marked
Deprecated in the crate's own documentation as of `1.10.1` — retrieved 2026-09-17,
https://docs.rs/zenoh-ext/1.10.1/zenoh_ext/index.html — and are not used by this design;
the primitive this document relies on is `zenoh::liveliness::Liveliness` and its
`LivelinessSubscriberBuilder`, not the deprecated `zenoh-ext` structs of a similar name.

## 5. Local-mode section

**Listener bound to `127.0.0.1`; no manual certificate management.** Local mode's Zenoh
peer listens only on the loopback interface — no LAN-facing bind by default — and,
because no TLS/QUIC listener is configured on the local-mode path, there is no
certificate for a user to create, install, or manage. Quoted, DESIGN.md line 115:
"Local mode should expose only locally where practical and require no manual
certificate management."

**No `zenohd`.** Quoted, `oac-boundaries` boundary 6: "the reference implementation is a
CLI and must not require Docker, Kubernetes, a cloud account, or a separately
administered server for normal local use... G3 loopback multicast scouting is flaky on
your Windows box, so you make `zenohd`... a required background process for local mode —
that turns an optional peer-to-peer transport into a separately administered server for
the default path." Quoted, `oac-zenoh` §4: "Storage and other plugins run only inside
`zenohd` (router) — never inside an in-process peer... do not make `zenohd` a required
background process for the default local path just to get a plugin feature." OAC's
Zenoh peer runs in `peer` mode, in-process inside the `oac` daemon (`docs/planning/
decisions/C2-process-model.md` §1, §9 — the daemon hosts the Zenoh peer); no plugin
feature this design needs (presence history is liveliness-subscriber history, §4 above,
not a `zenohd` storage plugin) requires the router.

**Multicast scouting: ON by default — one sentence, with the reason.** Local mode leaves
Zenoh's default multicast scouting enabled (address `224.0.0.224:7446`, `interface:
"auto"`, per `oac-zenoh`'s Pin section citing the fetched `DEFAULT_CONFIG.json5` at tag
`1.10.1`), because the pinned Zenoh release `1.10.1` already satisfies the `>= 1.10.0`
floor that fixes same-host loopback discovery. Quoted, `oac-zenoh` §3: "Same-host
discovery over loopback was broken before 1.10.0 (PR #2671) — plan on >= 1.10.0."
Source: PR #2671, https://github.com/eclipse-zenoh/zenoh/pull/2671. `docs/planning/
PINS.md` — "Zenoh": "Hard constraint: pin **MUST be `>= 1.10.0`**... `1.10.1 >= 1.10.0`:
constraint satisfied." Leaving scouting on is therefore the zero-config default C2's
"zero-file local default" (`docs/planning/decisions/C2-process-model.md`) already
commits to: two OAC peers on one machine discover each other with no fixed-endpoint
configuration to write.

**Reversal path, named per the G3 fallback.** Quoted, `docs/planning/STATUS.md`'s Gate
verdicts table, G3 row: "Fallback: fixed local endpoint, no scouting." If gate G3 (Zenoh
local peer, currently `NOT RUN`) finds multicast scouting unreliable on a tested
platform, the reversal is a **fixed local rendezvous endpoint** — a well-known loopback
TCP address/port OAC's own peers connect to directly — with scouting disabled, not a
second scouting configuration. This mirrors `docs/planning/PLANNING-PROMPT.md` §4 G3's
own fallback framing and is not a new fallback this document invents.

**Windows scouting-socket behaviour and `#iface=`, recorded, not relied on.** Quoted,
`oac-zenoh` §3: "Windows binds the scouting socket to `0.0.0.0` with `SO_REUSEADDR`;
`#iface=` endpoint parameters are documented for Linux only." This document's local-mode
default (above) does not configure `#iface=` on any platform — the `interface: "auto"`
default is used everywhere — so the Linux-only restriction is a non-issue for the chosen
configuration; it is recorded here because a future local-mode tuning change that reaches
for `#iface=` on Windows or macOS would be relying on an undocumented behaviour, which
this document flags in advance rather than after the fact.

## 6. LAN-mode section

**TLS is the v0.1 default; QUIC is the named alternative.** Zenoh's config exposes both
TLS and QUIC listeners with per-endpoint certificates (`oac-zenoh` §5: "TLS/mTLS and
QUIC listeners with per-endpoint certificates"). This document picks **TLS** as the
default LAN-mode transport, for two reasons: (a) TLS is the transport C5 §12's
already-decided certificate model (below) was written against — a common-name-bearing
X.509 certificate is the same artifact either listener type would need, but TLS's config
surface (`root_ca_certificate`, `listen_private_key`, `listen_certificate`,
`enable_mtls`, per the fetched `DEFAULT_CONFIG.json5` at tag `1.10.1`, retrieved
2026-09-17) is the more conventionally deployed and more widely firewall-compatible
choice for a LAN peer-to-peer link, versus QUIC's UDP-based transport which some
consumer/enterprise LAN firewalls block or rate-limit by default; and (b) TLS keeps LAN
mode on the same TCP-based link family as local mode's underlying transport, minimizing
the number of distinct network code paths this v0.1 transport module needs to get right.
**QUIC is kept as the named alternative**, not rejected outright (§10 does not list it as
rejected): a future decision may switch the default to QUIC — for example, if NAT
traversal or head-of-line-blocking characteristics matter more than firewall
compatibility for a given deployment — without needing a new certificate model, since
both listener types consume the same certificate shape this section fixes.

**Certificates issued by C5 §10(b)'s short-code pairing flow — reused, not redesigned.**
This document does not invent a certificate issuance mechanism. `docs/planning/
decisions/C5-envelope-auth.md` §10(b) already defines the LAN pairing flow (a
6-digit/120-second/5-attempt short code, per `docs/planning/STATUS.md`'s C5 summary) that
issues the certificate this section's TLS listener presents and verifies; C7 wires that
already-decided flow to the Zenoh TLS config keys (`listen_private_key`,
`listen_certificate`, `root_ca_certificate`) rather than re-deciding how a certificate
comes to exist.

**ACL subjects are certificate common name or username only — never `zid` — restated as
finalized here.** Quoted, `docs/planning/decisions/C5-envelope-auth.md` §12: "OAC policy
maps only onto authenticated Zenoh ACL subjects — certificate common name or username —
never `zid`." Quoted, `oac-zenoh` §5: "`zid` subjects are explicitly unauthenticated and
unfit for production." This document does not change that rule; §7 below states exactly
what C7 adds to it.

**Common name derived from the device public-key fingerprint.** Quoted, `docs/planning/
decisions/C5-envelope-auth.md` §12: "a LAN-mode Zenoh TLS/QUIC certificate's common-name
field is set to the same device-public-key fingerprint the pairing flow (§10) already
computes and binds its short code to." Restated here as the certificate's concrete
common-name value this section's TLS listener will present: never a human-chosen label,
never derived from the display URI (`docs/planning/decisions/C4-session-identity.md` §8's
display-only rule applies here too — the CN is not a rendering of the URI, it is rooted
in the same device-key fingerprint the URI's `<device>` segment is a human-readable label
for, not a parse of it).

**Default-deny is the starting posture.** Quoted, `oac-security-work` §4: "Default-deny
is the standing posture... On Zenoh only certificate common names and usernames are
authenticated... ACL subjects must be drawn only from those, never from `zid`." Every
Zenoh ACL rule this transport ships with starts denied; an authenticated common
name/username is granted access to a specific key expression only by an explicit rule, mirroring the same default-deny
posture C5 §11's sender allowlist and C4 §5's registration model already apply one layer
up.

## 7. ACL-as-pre-filter — the C5 handoff

**The one-sentence relationship, carried verbatim from C5 §12.** Quoted, `docs/planning/
decisions/C5-envelope-auth.md` §12: "the Zenoh ACL is a coarse pre-filter, the envelope
signature is the authenticity proof, and Zenoh performs no payload authentication of its
own." Quoted, `oac-zenoh` §6: "Zenoh provides no application-layer message signing.
Authenticity of an OAC envelope must be established by OAC itself — the
`security.signature` field in the envelope, not transport security, is the authenticity
proof even over TLS/QUIC." This relationship is not renegotiated here; it is the fixed
premise C7 builds its ACL design against.

**C7 finalizes what C5 §12 deferred — stated as the split, not left implicit.** C5 §12
kept the **policy-to-subject rule**: OAC policy maps onto authenticated ACL subjects
(certificate common name or username), never `zid` — unchanged, restated in §6 above.
What C5 §12 explicitly left to a later document is the piece its own §2 (the C5-C7
boundary, restated here rather than re-quoted) marks out: **C7 adds** (a)
**key-expression scoping of ACL rules** — an ACL rule this transport ships with is
scoped to a specific key expression (§3's per-session, non-broadcast layout), not a
wildcard covering every session a peer might ever address, so a certificate/username
subject authorized for one session's key expression is not thereby authorized for every
other session on the same peer; (b) **the local-vs-LAN profile split** — local mode
(§5) ships with no ACL at all, because the listener binds only to loopback and no
external principal can reach it to need one, while LAN mode (§6) is exactly where the
certificate-common-name-keyed default-deny ACL applies; and (c) **certificate issuance
wiring** — §6's binding of C5 §10(b)'s pairing flow to the concrete Zenoh TLS config
keys. None of (a)-(c) changes the authenticity-proof relationship stated above; each is
a piece of Zenoh-side configuration this transport module owns, gated by the same
pre-filter-only role the envelope signature check (outside this module, in the neutral
core) always retains as the actual authenticity decision.

## 8. Stable API surface

**Only `zenoh` and `zenoh-ext` — no `unstable` feature is used.** This is the cleaner
answer named in the task instruction, and it is checked against every feature this
design actually touches, not assumed:

| Feature used | Crate/module | Stable or `unstable`? | Verified against |
|---|---|---|---|
| `Session::declare_publisher` / `Session::declare_subscriber` (pub/sub, §2's `publish`/`subscribe`) | `zenoh` | Stable | Ordinary session API, no `unstable`-gated builder involved |
| `zenoh::liveliness::Liveliness::declare_token` (`LivelinessToken`, §4) | `zenoh` | Stable | Fetched `zenoh::liveliness` module page: "None of the items are marked as 'Unstable'... All six structs are stable." Source: https://docs.rs/zenoh/1.10.1/zenoh/liveliness/index.html, retrieved 2026-09-17 |
| `zenoh::liveliness::Liveliness::declare_subscriber` with `.history(true)` (history-capable liveliness subscriber, §4) | `zenoh` | Stable | Fetched `LivelinessSubscriberBuilder` page: `history()` listed with "No feature badge," alongside `callback`/`callback_mut`/`with`/`background`. Source: https://docs.rs/zenoh/1.10.1/zenoh/liveliness/struct.LivelinessSubscriberBuilder.html, retrieved 2026-09-17 |
| TLS listener config (`listen_private_key`, `listen_certificate`, `root_ca_certificate`, `enable_mtls`, §6) | `zenoh` config | Stable | `DEFAULT_CONFIG.json5` at tag `1.10.1` — ordinary (non-`unstable`-flagged) config block. Source: https://raw.githubusercontent.com/eclipse-zenoh/zenoh/1.10.1/DEFAULT_CONFIG.json5, retrieved 2026-09-17 |
| ACL config block (`access_control`, rules keyed by `cert_common_names`/`usernames`, §6-§7) | `zenoh` config | Stable | Same source as above; the ACL block is an ordinary (commented-out-by-default) config section, not gated by an `unstable` cfg flag |
| Multicast scouting config (`scouting.multicast`, §5) | `zenoh` config | Stable | Same source; `oac-zenoh` Pin section already confirms these six keys verbatim against the same tag |

**`zenoh-ext`'s Advanced Publisher/Subscriber (cache, retransmission, miss detection) is
explicitly not used.** Fetched confirmation that these items exist behind `unstable`:
"Yes, this documentation page displays the 'Available on crate feature `unstable` only'
badge in multiple locations" for `AdvancedSubscriberBuilder`'s `IntoFuture`/`Resolvable`/
`Wait` implementations (the methods that actually build/resolve the subscriber). Source:
https://docs.rs/zenoh-ext/1.10.1/zenoh_ext/struct.AdvancedSubscriberBuilder.html,
retrieved 2026-09-17. This design does not need `AdvancedSubscriber`/`AdvancedPublisher`:
presence history (§4) is fully served by the plain, stable `LivelinessSubscriberBuilder`
`.history(true)` option confirmed stable in the table above, and this transport carries
no reliability/cache requirement beyond what the neutral `Transport` contract's optional
capabilities already scope as optional (DESIGN.md line 67: "Optional capabilities:
reliability, persistence, offline queueing, ordering, multicast discovery,
routing/federation" — none of which this v0.1 mapping claims). No named justification or
reversal condition for an `unstable` feature is therefore needed — the plainer,
unconditional statement is: **no `unstable` feature is used.**

**License.** Dual **EPL-2.0/Apache-2.0**; OAC elects the **Apache-2.0** arm — the same
election `docs/planning/decisions/C1-language-runtime.md` §7 already makes for `zenoh`
and `docs/planning/decisions/C4-session-identity.md` §10/§11 make for `keyring`/`age`
(the `interprocess` election, `docs/planning/decisions/C2-process-model.md`, follows the
identical pattern). Quoted, `docs/planning/PINS.md` — "Zenoh": "License: dual **EPL-2.0 /
Apache-2.0**." `zenoh-ext` ships from the same repository/workspace and is not
separately re-verified for license here — `docs/planning/PINS.md`'s Zenoh record and
`docs/planning/decisions/C1-language-runtime.md` §7's election already cover the
workspace this crate is published from.

## 9. Threat table

Per `oac-security-work` §1's template, all five columns, no blank "proving test." C7-owned
threats — the transport-mapping layer — are not duplicated from `docs/planning/
decisions/C4-session-identity.md` §13 (identity-level threats), `docs/planning/
decisions/C5-envelope-auth.md` §13 (envelope-authenticity-level threats), or `docs/
planning/decisions/C6-trust-rendering.md` §12 (provider-rendering-level threats).

| Attack | Precondition | Mitigation | Proving test | Residual risk |
|---|---|---|---|---|
| Unauthorized routing/discovery via a guessable key expression | Attacker can predict or enumerate a target session's Zenoh key expression without already knowing its opaque session id | Key expression is a one-way hash of the 128-bit CSPRNG opaque session id (§3); no broadcast/room-shaped pattern to scan against (§3) | The containment lint (`oac-boundaries` check 1, §2 — confirms no key-expression-shaped constant leaks where it could be statically enumerated); gate G3 | Gate G3 `NOT RUN`; whether the derivation function itself resists offline brute-force at scale is a Stage 3 implementation property (choice of hash primitive), not fixed by this document |
| Unauthorized discovery of a session in another working directory (cross-project leakage) | A peer paired on the same LAN, or a peer on the same local host, attempts to discover a session registered under a different `working_directory` | `working_directory` scoping lives in the registration record (`docs/planning/decisions/C4-session-identity.md` §5), filtered before a discovery grant — this document's transport carries no directory-shaped key-expression segment (§3) a peer could pattern-match to guess at project scope; local-mode ACL is absent (§7(b)) but the loopback bind (§5) limits reachability to the same host, and LAN-mode ACL (§6-§7) is default-deny per subject | F11 security suite; H2 (fourth acceptance item, per `oac-security-work` §2) | F11/H2 not yet built (`docs/planning/STATUS.md`); same open item `docs/planning/decisions/C4-session-identity.md` §13 and `docs/planning/decisions/C5-envelope-auth.md` §13 already name for this exact threat class, restated here at the transport layer |
| Compromised transport infrastructure | An attacker controls or observes traffic on the Zenoh link (local loopback or LAN TLS) | Zenoh transport security (loopback isolation locally, §5; TLS with pairing-issued certificates on LAN, §6) is defense in depth only — the envelope signature (`docs/planning/decisions/C5-envelope-auth.md` §2) is the actual authenticity proof regardless of transport compromise, per §7's restated relationship | Gate G3 (transport reachability); F11 security suite (signature verification independent of transport state) | Gate G3 `NOT RUN`; F11 not yet built; a fully compromised transport can still deny/drop/replay-at-the-network-layer even though it cannot forge a validly-signed envelope — replay defence is `docs/planning/decisions/C5-envelope-auth.md` §7-§8's job, not restated here |
| `zid`-as-identity misuse | A future code path is tempted to key an ACL rule, allowlist entry, or authorization decision on a Zenoh `zid` instead of an authenticated ACL subject | ACL subjects are certificate common name or username only, never `zid` (§6, restating `docs/planning/decisions/C5-envelope-auth.md` §12 and `oac-zenoh` §5's "explicitly unauthenticated and unfit for production"); the containment lint's `\bzid\b` pattern (§2) flags any `zid`-named identifier that leaks into `core/`, `spec/`, `adapters/`, `cli/` | The containment lint (`oac-boundaries` check 1, §2) | Pending — `core/`/`spec/`/`adapters/`/`cli/` do not exist yet, so the lint reports a missing-path error, not a pass, until Stage 3 code lands (`oac-boundaries` "Mechanical checks" status) |
| LAN certificate misissuance | An attacker completes, or forges completion of, C5 §10(b)'s short-code pairing flow and obtains a certificate with an attacker-controlled common name | Certificate issuance is entirely C5 §10(b)'s already-decided pairing flow (6-digit/120-second/5-attempt short code) — this document adds no separate issuance path an attacker could target instead; common name is derived from the device public-key fingerprint (§6), not attacker-suppliable free text | F5 (authorization engine and pairing store, per `docs/planning/decisions/C4-session-identity.md` §13's identical row for the analogous device-key-exfiltration threat); F11 security suite | F5/F11 not yet built; this row is the transport-layer restatement of the pairing-flow threat C5 §13 already owns at the identity layer — not a new attack surface C7 itself introduces, since C7 reuses rather than redesigns issuance (§6) |
| Scouting exposure beyond loopback in local mode | Local-mode multicast scouting (§5), left on by default, is reachable from outside the intended loopback-only scope (e.g. a misconfigured host where `127.0.0.1`-only binding does not actually prevent multicast group membership from being visible on a shared LAN segment) | Listener itself binds `127.0.0.1` (§5); scouting's own multicast address (`224.0.0.224:7446`) is a discovery-only channel, not the data-plane publish/subscribe path, and any peer that scouting helps discover still faces §5's loopback-bound listener for the actual pub/sub link; the G3 fixed-rendezvous-endpoint fallback (§5) is the named reversal path if a platform's scouting behaviour is found to leak beyond the intended scope | Gate G3 (must record real scouting-socket behaviour per platform, per `oac-zenoh` §3's Windows `0.0.0.0`/`SO_REUSEADDR` note); the containment lint does not cover this row (it is a runtime network-behaviour question, not a static-text one) | Gate G3 `NOT RUN`; the Windows `0.0.0.0`/`SO_REUSEADDR` scouting-socket behaviour (`oac-zenoh` §3) is not yet exercised against a real multi-host or shared-segment topology — this is exactly the class of finding G3 is timeboxed to produce, per `docs/planning/PLANNING-PROMPT.md` §4 |

Every row names its proving test; none is marked mitigated without one, per
`oac-security-work` §1's rule. Because every named test's current verdict is `NOT RUN` or
the underlying task/lint is pending, every row above describes a **designed** mitigation,
matching the caveat stated at the top of this document.

## 10. Rejected alternatives

- **`zenohd` router on the default local path.** Rejected: quoted, `oac-boundaries`
  boundary 6, "must not require... a separately administered server for normal local
  use"; quoted, `oac-zenoh` §4, "Storage and other plugins run only inside `zenohd`...
  do not make `zenohd` a required background process for the default local path just to
  get a plugin feature." §5's presence-history need (§4) is served by the stable, plugin-
  free `LivelinessSubscriberBuilder.history(true)`, so no plugin feature justifies the
  router on the default path.
- **`zid` as ACL subject or principal.** Rejected: quoted, `oac-zenoh` §5/§2, "`zid`
  subjects are explicitly unauthenticated and unfit for production"; §6-§7 restate C5
  §12's rule that only certificate common name or username are authenticated ACL
  subjects.
- **Public/guessable key expressions derived from the display URI.** Rejected: the
  display URI (`docs/planning/decisions/C4-session-identity.md` §8) is human-readable
  and, by design, not secret — deriving a key expression from it (rather than from the
  128-bit opaque id) would make the key expression guessable by anyone who has seen a
  session's display string (a CLI listing, a log line), defeating §3's "never
  guessable" requirement.
- **Transport TLS as the authenticity proof.** Rejected: quoted, `oac-zenoh` §6, "Zenoh
  provides no application-layer message signing... not transport security"; §7 restates
  C5 §12's relationship — TLS is a pre-filter and confidentiality layer, never a
  substitute for the envelope signature.
- **Multicast scouting left off by default in local mode.** Rejected (the alternative to
  §5's chosen "on"): would require every local single-host deployment to configure a
  fixed rendezvous endpoint even though the pinned Zenoh release (`1.10.1`) already
  satisfies the `>= 1.10.0` loopback-discovery floor (§5) — this would trade C2's
  zero-file local default for a manual step with no offsetting benefit at this pin,
  and is kept in reserve as the G3 fallback (§5) rather than the default.
- **`zenoh-ts`.** Rejected: quoted, `oac-zenoh` §2, "TypeScript is **not** native —
  `zenoh-ts` requires a `zenohd` router with the remote-api plugin and does not run on
  Node.js." Needs a router, the same disqualifier as the first rejected alternative
  above, and is language-choice-adjacent evidence already settled by `docs/planning/
  decisions/C1-language-runtime.md`'s Rust choice, not re-litigated here.
- **Plugin-backed storage for presence history.** Rejected: `zenoh-ext`'s (deprecated)
  `LivelinessSpace`/`UserSpace`/`KeySpace` structs and any `zenohd`-plugin-backed storage
  approach both require the router (first rejected alternative) or a superseded API
  surface; §4's stable, in-process `LivelinessSubscriberBuilder.history(true)` already
  gives history-capable presence with neither.

## 11. Reversal condition

**Zenoh is `supported` at `1.10.1`.** Reverse this document's transport mapping — not
silently promote any UNVERIFIED item below, per `oac-evidence` §5 — if either of two
conditions fires:

- **G3's verdict.** `docs/planning/STATUS.md`'s Gate verdicts table currently records G3
  (Zenoh local peer) as `NOT RUN`. If G3 runs and finds multicast scouting unreliable on
  a tested platform, §5's "scouting on by default" choice reverses to the named fallback
  (fixed local rendezvous endpoint, no scouting) for that platform, per the G3 row's own
  "Fallback: fixed local endpoint, no scouting" (`docs/planning/STATUS.md`). If G3 fails
  outright with no viable fallback, this document's local-mode section (§5) needs a new
  decision, not a silent patch. Check `docs/planning/STATUS.md`'s Gate verdicts table for
  the current verdict before assuming either outcome — do not assume PASS or FAIL.
- **A Zenoh version bump.** This document's pin is `1.10.1` (`docs/planning/PINS.md` —
  "Zenoh"). A future re-pin to a newer Zenoh release invalidates every fact this document
  cites against `1.10.1` specifically (the `DEFAULT_CONFIG.json5` config keys, §5's/§8's
  fetched confirmations, the liveliness API's stable/`unstable` status) — per `oac-evidence`
  §7, re-verify each fact against the new pinned version before trusting it again, and
  update `oac-zenoh`'s `## Pin` section and `docs/planning/PINS.md`, not just this
  document's prose.

Neither condition has fired as of this document.

## 12. Surface labels and UNVERIFIED ledger

Per `oac-evidence` §4, one label per surface touched by this document, at first mention:

| Surface | Label | Note |
|---|---|---|
| Zenoh (`zenoh`, `zenoh-ext`) | **supported** | pinned `1.10.1`, unchanged from `docs/planning/PINS.md` — "Zenoh": "Surface label: **supported**"; referenced throughout §2-§8 |

Zenoh is the one surface this document's transport mapping is built against, and it
already carries `supported`, not `research preview`/`experimental`/`undocumented` — no
compatibility shim boundary is owed for it the way `oac-evidence` §4 requires for a
preview/experimental surface (that requirement applies to the Claude Channels and Codex
app-server surfaces C4/C5/C6 already carry forward, not restated here).

**Carried, not silently promoted (`oac-evidence` §5) — every item below is unchanged by
this document and still appears in `docs/planning/STATUS.md`'s "Open UNVERIFIED items"
list:**

- **Pubkey-auth semantics UNVERIFIED.** Quoted, `oac-zenoh`'s Pin section: "the config
  file carries no semantics comment for this block, so pubkey-auth **semantics** remain
  UNVERIFIED." This document does not rely on `auth.pubkey` — §6's certificate-common-
  name model is the chosen LAN authentication mechanism, not public-key auth — but the
  item stays open because it is a general Zenoh-surface fact, not resolved by this
  document choosing a different mechanism.
- **Binary-size 5-15 MB derived/UNVERIFIED.** Quoted, `docs/planning/STATUS.md`: "The
  5-15 MB Zenoh binary size estimate (UNVERIFIED — derived estimate, resolved by the
  first G3 build artifact, task D3)." Unaffected by this document's transport-mapping
  choices.
- **Crates.io cross-check UNVERIFIED.** Quoted, `docs/planning/STATUS.md`: "Zenoh crate
  version/date read from GitHub releases rather than crates.io directly... UNVERIFIED —
  re-confirm on crates.io when reachable." This document's own §8 research (the
  `DEFAULT_CONFIG.json5`, liveliness module, and `AdvancedSubscriberBuilder` fetches) was
  performed against `docs.rs` and `raw.githubusercontent.com`, both first-party for the
  `eclipse-zenoh` project per `oac-evidence` §1 — it does not touch or resolve the
  crates.io-specific gap, which stays open.

**No new UNVERIFIED item is introduced by this document.** Every fact §3-§8 rely on was
either fetched directly against the pinned `1.10.1` tag this pass (§4, §8's table; dated
2026-09-17) or is cited from an already-verified section of `oac-zenoh`, `docs/planning/
PINS.md`, or an already-decided C-series document. The Rust-level derivation primitive
for §3's key expression (which specific hash function) is explicitly left as a Stage 3
implementation detail (§3, "the exact primitive is a Stage 3 implementation detail, not
fixed here") rather than recorded as UNVERIFIED — it is undecided by design, not an
unverified claim about an external system.

## 13. Acceptance boxes, ticked against lines in this file

Per issue #20's six acceptance boxes (backlog task C7, `docs/planning/backlog/
03-tasks-CD.json`):

- [x] Key-expression layout defined privately inside the transport; opaque session ids
      never map to guessable public addresses — §3 (one-way derivation from the opaque
      session id, no reverse mapping, no broadcast/room pattern), §2 (containment: key
      expressions never leave `transports/zenoh/`).
- [x] Presence mapped to liveliness tokens with history-capable liveliness subscribers —
      §4 (the mechanism, the three-way `online`/`unreachable`/`unknown` mapping table,
      the no-polling rule).
- [x] Local mode: listener bound to `127.0.0.1`, explicit decision on multicast
      scouting on or off, no manual certificate management — §5 (loopback bind, "no
      `zenohd`," scouting left on with the stated reason and the G3-fallback reversal
      path, Windows scouting-socket behaviour recorded).
- [x] LAN mode: TLS or QUIC with pairing-issued certificates; ACL subjects restricted to
      authenticated ones (certificate common name or username, never `zid`) — §6 (TLS
      chosen as default, QUIC named as alternative, C5 §10(b) pairing flow reused,
      common name derived from device-key fingerprint, default-deny posture), §7 (the
      C5-handoff relationship restated and what C7 adds).
- [x] Stable API surface only (`zenoh`, `zenoh-ext`); any `unstable` feature used is
      named and justified — §8 (every feature enumerated against stable/`unstable`
      status with a first-party citation each; the plain "no `unstable` feature is
      used" conclusion, with `AdvancedSubscriber`/`AdvancedPublisher`'s `unstable`
      gating confirmed and explicitly not relied on).
- [x] The containment boundary is stated precisely enough for a lint or test to
      enforce it — §2 (the one enforceable sentence, the `oac-boundaries` check-1 grep
      quoted verbatim rather than reinvented, the public-API-signature-set test
      described).

## 14. Cross-file updates in this change

- `docs/planning/STATUS.md`: add a "C7 — Zenoh transport mapping and containment
  boundary (issue #20): decided" bullet under "Decisions landed," alongside the existing
  C1-C6 pointers; bump "Last updated"; no new UNVERIFIED items are added (§12) — every
  item this document touches is already carried in the existing "Open UNVERIFIED items"
  list; add a one-line pointer to this file under the same standalone-ledger convention
  `docs/planning/STATUS.md` already uses for C4/C5/C6.
- `docs/planning/ADR-001-AMENDMENTS.md`: **no row moves.** This document introduces no
  new ADR-001-text-correcting finding — `docs/planning/ADR-001.md` carries no text about
  Zenoh key-expression layout, presence-liveliness mapping, or the local/LAN security
  profile split that this document contradicts or corrects (checked directly, the same
  test `docs/planning/decisions/C4-session-identity.md` §17, `docs/planning/decisions/
  C5-envelope-auth.md` §18, and `docs/planning/decisions/C6-trust-rendering.md` §17 each
  apply before declining a new amendment). **Collision warning, stated explicitly so a
  reviewer does not read this as a missed edit:** the conflict-register row literally
  named `C7` in `ADR-001-AMENDMENTS.md` line 395 and `PLANNING-PROMPT.md` Appendix A line
  229 is a **different, unrelated** item — "ACP is client-owned-session, not a channel,"
  already `RESOLVED-BY-EVIDENCE`. That row's backlog key coincidence with this
  document's own backlog key (`C7`, Epic C task "Decide the Zenoh transport mapping and
  its containment boundary") is exactly that — a coincidence of two independent
  numbering schemes (the conflict register's `C`-prefixed rows, and Epic C's own
  `C`-prefixed backlog task keys) landing on the same label. This document does **not**
  touch, resolve, or reference that conflict-register row, and no edit to it is made or
  intended here.
- `docs/planning/v0.1/06-security.md` (Epic A task A7): named fold-in target — this
  document's §6-§7 (LAN certificate/ACL model, the C5-handoff relationship) and §9
  (threat table) are exactly what that eventual file's "threat table per §7" section
  (per `docs/planning/PLANNING-PROMPT.md` §9 item 7) will draw from, alongside `docs/
  planning/decisions/C6-trust-rendering.md`'s already-named §2-§7/§12 contribution to the
  same file.
- `docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4): named fold-in
  target for this document's entire substance, per this document's own "Where this folds
  in" section below and the standalone-ledger note at the top.
- **Downstream implementation and verification items, named:** backlog task **G3** (Zenoh
  local-peer gate spike — this document's §5 local-mode design and §11 reversal
  condition are exactly what G3 exercises); backlog task **D3** (Zenoh gate spike build
  artifact — resolves the binary-size UNVERIFIED item this document carries forward,
  §12); backlog task **F11** (security suite — proving test for §9's key-expression-
  guessability and transport-infrastructure-compromise rows); backlog task **H2**
  (security verification — proving test for §9's cross-project-leakage row, the same
  test C4/C5 already name for their own identity- and envelope-layer instances of the
  same threat class).

## Where this folds in

Once `docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4) exists, this
file's content moves there unedited in substance (per `docs/planning/PLANNING-PROMPT.md`
§9's output package shape) and this file becomes a redirect stub, mirroring how
`docs/planning/ADR-001-AMENDMENTS.md`, `docs/planning/decisions/C4-session-identity.md`,
`docs/planning/decisions/C5-envelope-auth.md`, and `docs/planning/decisions/
C6-trust-rendering.md` already describe their own eventual fold-in. Consumed by: G3 (the
Zenoh local-peer gate this document's local-mode design and reversal condition directly
feed, §5, §11), D3 (the gate spike build artifact that resolves the binary-size
UNVERIFIED item, §12), F11 (transport-layer security suite, §9), H2 (cross-project-
leakage and transport-compromise verification, §9), and Epic A task A7's `docs/planning/
v0.1/06-security.md` (the normative-adjacent landing site named in §14).
