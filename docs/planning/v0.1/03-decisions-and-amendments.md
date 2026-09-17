# 03 — Decisions and amendments

**Purpose.** State, for each of PLANNING-PROMPT.md §5's twelve decisions, the choice,
the decisive evidence, the rejected alternatives, and the reversal condition; record
ADR-001 amendments A1-A3 verbatim (old text, new text, rationale); and reproduce the
resolved Appendix A conflict register C1-C10 plus the two new open entries C11-C12.

**Generated-summary notice.** This file is a synthesis, assembled from
`docs/planning/ADR-001-AMENDMENTS.md` and `docs/planning/decisions/C1-language-runtime.md`
through `docs/planning/decisions/C7-zenoh-transport.md`. It is **not hand-authored** and
**does not re-derive or re-decide** anything: every decision, amendment, and conflict-
register resolution below was made in one of those source documents, cited here, not
recomputed. Where this file quotes a first-party external source verbatim (an ADR-001
old/new text pair), the quotation is copied only from `ADR-001-AMENDMENTS.md` — this file
does not independently re-open `ADR-001.md` to re-quote it, per `oac-evidence` §2's rule
against re-quoting a secondary file where a primary citation already exists and per the
drift risk that would create.

**Precedence.** Per decision (§5 below), the cited `docs/planning/decisions/C<n>-*.md`
file is authoritative for the choice, its decisive evidence, and its reversal condition —
this file restates those fields, it does not re-derive or override them. Per amendment
(§ Amendments below), `docs/planning/ADR-001-AMENDMENTS.md` is authoritative for the
verbatim old/new text and rationale. `docs/planning/PINS.md` is authoritative for every
version number wherever this file or a source document could be read as disagreeing about
one.

**Cross-references (repo-relative paths).** `docs/planning/ADR-001.md`,
`docs/planning/ADR-001-AMENDMENTS.md`, `docs/planning/DESIGN.md`,
`docs/planning/PLANNING-PROMPT.md` §4, §5, Appendix A, `docs/planning/PINS.md`,
`docs/planning/STATUS.md`, `docs/planning/v0.1/01-capability-matrix.md`,
`docs/planning/v0.1/02-gating-findings.md`, and each of
`docs/planning/decisions/C1-language-runtime.md` through
`docs/planning/decisions/C7-zenoh-transport.md`.

**Naming.** Product and repository: **Open Agent Channel (OAC)**. Normative protocol
specification: **OAC Session Channels** (short form: "the OAC spec"). CLI binary:
**`oac`**. Per ADR-001-A1 (§ Amendments below), this file never writes "Session Channels"
alone or `sessionchannels`, except inside a marked pre-rename verbatim quotation (the
ADR-001-A1 old-text blocks below, which quote `ADR-001.md`'s pre-rename wording by design).

---

## 0. The fold-in tension, resolved for this file

Every `docs/planning/decisions/C<n>-*.md` file's own "Where this folds in" section, and
`docs/planning/STATUS.md` lines 93-172 (the seven "Folds into ... once that file exists"
pointers), state that once this file existed the C-doc's content would move here
"unedited in substance" and the C-doc would "become a redirect stub." This file, as
scoped by issue #21 and the `oac-planning-package` skill's file-4 job ("every §5 decision
made; ADR-001-A1 onward; the resolved conflict register"), is a **synthesis/assembly
file that cites its sources rather than copying their prose** — the house style this
skill requires throughout the v0.1 package ("Concise beats complete-looking... cite the
section instead"). Both readings cannot be true of the same file at once: a
several-thousand-line unedited merge of `ADR-001-AMENDMENTS.md` and seven `C<n>` decision
documents is not "concise," and a citing synthesis is not "unedited in substance" in the
sense of literal copy. **This file takes the cite-not-copy reading.** The seven C-docs
and the amendments ledger remain the authoritative, standalone source documents — none is
stubbed or redirected by this task. `docs/planning/STATUS.md`'s seven fold-in pointers
are updated (§13, cross-referenced below) to say this file is the assembled view that
cites them, not a file that has absorbed and replaced their content. This is the explicit
resolution the task instruction requires be recorded rather than silently picked: the
discrepancy between "moves unedited" and "cite, don't copy" is real, and this paragraph
is where it is decided, in favor of citation, for the reasons above.

---

## 1. Decisions (PLANNING-PROMPT.md §5)

All twelve decisions are **made** below, each citing its owning `docs/planning/
decisions/C<n>-*.md` file — no decision is left open as "either X or Y." One narrower
element, inside decision 3, is explicitly **gate-decided**: the dual-era process-topology
answer (one process serving both MCP eras, versus two entry points sharing one core) is
assigned to gate G4, per conflict-register row C5 and `docs/planning/decisions/
C3-spec-packaging.md` §7 — named at decision 3 below, not glossed over. This satisfies
issue #21's acceptance box 1: **every one of the twelve decisions is made or, for the one
sub-element that is not, gate-decided, and the deciding gate is named** (G4).

**Gate-dependency note, read before the twelve decisions.** A decision being "made" does
not mean the mechanism it designs is proven to work — every gate verdict in
`docs/planning/STATUS.md` is **`NOT RUN`**, and this file never writes a gate as passed.
Four decisions carry a named, unrun gate as the thing that will prove (not decide) their
design, restated per-decision below and carrying forward each source document's own
opening caveat:

- **Decision 5 (envelope authenticity) and decision 8 (provenance rendering)** — gate
  **G5 (Provenance) is `NOT RUN`**. `docs/planning/decisions/C5-envelope-auth.md` and
  `docs/planning/decisions/C6-trust-rendering.md` each open with this caveat verbatim in
  substance: the document "designs the mechanism G5 will exercise; it does not assert
  that provenance ... works end to end."
- **Decision 10 (transport mapping)** — gate **G3 (Zenoh local peer) is `NOT RUN`**.
  `docs/planning/decisions/C7-zenoh-transport.md`'s opening caveat: the document "designs
  the transport mapping G3 will exercise... it does not assert that loopback discovery,
  presence liveliness, or either security profile works end to end on any platform."
- **Decision 3 (spec packaging), dual-era sub-element** — gate **G4 (MCP dual-era
  server) is `NOT RUN`**, per C3 §7 below.
- **Decision 2 (process model)** — the Codex daemon-attach runtime behaviour C2 §2 leg 4
  and C2 §11's second reversal-condition half both depend on is **UNVERIFIED**, carried
  open pending gate **G2 (Codex live inject)**, currently `NOT RUN`.

### Decision 1 — Language and runtime

**Choice.** Rust, built as a single OAC binary (`oac`) per host platform, pinned
toolchain `1.98.1`. Source: `docs/planning/decisions/C1-language-runtime.md` §1.

**Decisive evidence.** Rust MCP SDK `rmcp` `3.4.0` declares and, per its own changelog,
still defaults to the pinned legacy MCP revision `2025-11-25` as a server, not only a
client (C1 §3-§5); first-party Codex app-server client/protocol/transport crates exist
only in Rust, Apache-2.0 (C1 §6); Zenoh peer mode is native only in Rust, Python, C/C++,
JVM, and Go (PLANNING-PROMPT.md §3.4, cited by C1 §11).

**Correction carried forward, per the task breakdown's own note.** PLANNING-PROMPT.md
§5.1 and issue #13 both overstate `ADR-001.md` by saying single-binary packaging is
"required by ADR-001" — the ADR's actual text (quoted at C1 §2) requires only that the
reference implementation "must not require Docker, Kubernetes, a cloud account, or a
separately administered server for normal local use." C1 §2 records this as a correction,
not an amendment (no `ADR-001.md` text is wrong), and states a single self-contained
binary as the **chosen means** of satisfying that requirement, not a separate ADR-001
requirement in its own right. C1 §9 further corrects "static" to "single self-contained
binary per platform, dynamically linked against OS system libraries only" — not
bit-for-bit static on every target.

**Rejected alternatives, one line each** (C1 §11): Go (no first-party Rust MCP SDK
analogue, no Codex client crates); Python (no single-binary packaging story); TypeScript/
Node (no native Zenoh peer binding, no single-binary story); C++/JVM (no first-party
Codex client crates; JVM breaks the single-binary requirement outright).

**Reversal condition** (C1 §12, verbatim): "a disqualifying gap in the Rust MCP SDK's
legacy-revision support, or in Windows credential-store access." Neither half has fired
(C1 §12's two falsifiable tests both pass at the pinned versions).

### Decision 2 — Process model

**Choice.** Option (a): one long-lived per-device `oac` daemon (owns the Zenoh peer,
device identity/keys, policy/allowlists, and the Codex app-server client) plus thin
`oac mcp-shim` stdio child processes that Claude Code and Codex spawn. IPC: Windows named
pipe (`\\.\pipe\oac-<user-sid-or-hash>`, DACL-restricted plus
`GetNamedPipeClientProcessId` peer check) or Unix `AF_UNIX` stream socket
(`$XDG_RUNTIME_DIR/oac/`, `SO_PEERCRED`/`getpeereid()` peer check). Source: `docs/
planning/decisions/C2-process-model.md` §1, §4.

**Decisive evidence, four legs** (C2 §2): Claude Code forces a per-session spawned
process regardless of what sits behind it; Zenoh liveliness/presence must outlive a
single session, which a session-scoped peer cannot provide; device key material belongs
in one process per ADR-001's `Security principal -> Device -> Harness -> Session`
hierarchy; a single daemon-owned Codex app-server client avoids N independent processes
each individually hitting the cross-process-resume silent-append hazard (issue #21743).

**Rejected alternatives, one line each** (C2 §3): option (b), a self-contained
per-session Zenoh peer embedded in each shim (multiplies scouting/liveliness churn, key
duplication, presence flapping, and N-Codex-client exposure to #21743); a hybrid where
the shim embeds the peer and the daemon holds only keys (keeps (b)'s problems while
adding a second process for no offsetting benefit). Loopback TCP is separately rejected
as the local IPC mechanism itself (C2 §4, "Rejected: loopback TCP") for having no
OS-level peer authentication.

**Reversal condition** (C2 §11, two independent halves): local IPC cannot be
peer-authenticated on a supported platform — **not fired**, both platform families pass
the test at the cited API/crate versions; Claude Code's spawned shim cannot reliably
locate/connect to the daemon — **not yet run**, carried open pending the shim
environment-inheritance UNVERIFIED item, a G1-adjacent runtime question.

**Gate dependency.** The Codex daemon-attach runtime behaviour this decision's leg 4
relies on is UNVERIFIED at the pinned release (C2 §2 leg 4's own carried note), owned by
gate **G2**, `NOT RUN`.

### Decision 3 — Spec packaging

**Choice.** A standalone normative document — `OAC Session Channels`, under `spec/` —
plus an MCP extension identifier, `io.github.rossgraeber/oac-session-channels`, used only
for capability negotiation, the tool surface, and `_meta` provenance; the standalone
document is normative-of-record. Source: `docs/planning/decisions/C3-spec-packaging.md`
§1.

**Decisive evidence.** SEP-2133's identifier format `{vendor-prefix}/{extension-name}`
(C3 §2(c)) applied to a prefix the project demonstrably controls,
`io.github.rossgraeber` (C3 §3, GitHub-account/Pages ownership evidence); no MCP revision
defines "external event becomes a user turn" — delivery is always provider-native
(C3 §5, re-verified quote from PLANNING-PROMPT.md §3.3).

**Rejected alternatives, one line each** (C3 §9): MCP-only packaging (normative text
hostage to MCP revision churn); a standalone document with no extension identifier (loses
machine-readable capability negotiation); a `com.example`-style/unowned prefix (violates
SEP-2133's domain-ownership SHOULD); a custom method-name-namespacing convention (no SEP
rule for it — would be an invented, unverified convention).

**Reversal condition** (C3 §10): the project acquires an owned apex domain; SEP-2133 is
superseded by a new extensions SEP with a different identifier rule; the GitHub account
`RossGraeber` is renamed. None fired.

**The dual-era sub-element — gate-decided, not deferred.** C3 §7 and conflict-register
row C5 (§4 below) fix the **process topology** for serving Claude's legacy-MCP channel
path and Codex's current-MCP tool path simultaneously as **`ASSIGNED` to gate G4**
(`docs/planning/gates/G4-result.md`, verdict `NOT RUN`): "This document does not claim G4
passed... This packaging decision is what G4 will test, not evidence that it already
holds." The packaging choice above (identifier, standalone document) does not itself
depend on G4's outcome — only the one-process-vs-two-entry-points topology answer does,
per the fallback `docs/planning/STATUS.md` already names for G4 ("two entry points, one
core").

### Decision 4 — Session identity, addressing, discovery

**Choice.** A four-layer model in which only two layers carry authority: (1) the device
key (cryptographic root, OS credential store or encrypted-file fallback); (2) an opaque
128-bit CSPRNG session id, Crockford Base32-encoded, bound to the device key by a signed
registration record. (3) a display-only URI `session://<device>/<harness>/<id>` and (4) a
local human alias carry no authority. Source: `docs/planning/decisions/
C4-session-identity.md` §1.

**Decisive evidence.** Claude's `session_id` is captured only from `SessionStart` hook
stdin, never from files (§3); Codex's `thread.id` is captured only from the daemon's own
app-server client's `thread/start`/`thread/started` responses, never from rollout files
(§4); the registration record binds native id to opaque id via device-key signature (§5);
resume behaviour differs per harness by evidence, not assumption — Claude resumes mint a
new OAC session id unless a fresh registration is observed (docs silent on `--resume`
survival, §6), Codex resumes MAY re-bind the same OAC id because `thread.id` is
documented to survive restart (§7); the display URI resolves conflict C8 (§8, §4 below).

**Rejected alternatives, one line each** (C4 §14): harness-native id used directly as the
OAC wire address (hostage to a preview/experimental surface's own id scheme); id derived
deterministically from the device key (correlatable back to the device, defeating
opacity); the display URI as the wire address (leaks device/harness metadata onto every
message); alias as an ACL subject (aliases are local and mutable, unfit as a stable
grant target); keys in a plain config file (defeats the credential-store's purpose).

**Reversal condition** (C4 §15, two independent halves): the Claude-resume rule reverses
if evidence shows channels survive `--resume` — not fired, still UNVERIFIED/docs-silent;
the OS-keyring-first default reverses per-platform if that platform's `keyring` backend
fails C1 §12's falsifiable Windows test — not fired, test currently passes.

### Decision 5 — Envelope authenticity and replay

**Choice.** Ed25519 via `ed25519-dalek` `3.0.0` with mandatory `verify_strict`; signed
bytes are a domain-separated RFC 8785 JCS canonicalization (`serde_jcs` `0.2.0`) of every
envelope field except `security.signature` itself; one Ed25519 keypair per device, no
sessions holding their own keys, no v0.1 automatic rotation (manual re-pair only); replay
defence is a ±300s `created_at` accept-window plus a 128-bit CSPRNG nonce, deduplicated
on `(security.key_id, security.nonce)` against an in-memory, per-device,
daemon-held duplicate-suppression store bounded to the replay window; delivery receipts
are three honest states — `accepted-by-adapter`, `handed-to-harness`, `unknown`. Source:
`docs/planning/decisions/C5-envelope-auth.md` §1 items 1-2, 4-6.

**Decisive evidence.** RFC 8032 Ed25519 and `verify_strict`'s documented
malleability/torsion rejection (C5 §2); JCS canonicalizes the exact JSON already on the
wire, avoiding a second signed representation (C5 §3); the signed field set is enumerated
field-by-field with a stated reason each (C5 §5); the ±300s window equals the envelope's
own default `ttl_ms` (C5 §7); Claude Code sends no delivery acknowledgement, so a
resolved notification send is honestly `handed-to-harness`, never "seen by the model"
(C5 §9, resolving conflict C6, §4 below). This resolves conflict C4 (§4 below): the
signature is made normative, replacing DESIGN.md's `"implementation-defined"` placeholder
(C5 §6).

**Rejected alternatives, one line each** (C5 §14): transport-TLS-only authenticity
(Zenoh signs no payload); Zenoh `zid` as principal (explicitly unauthenticated); per-
session keys (reverses C2's one-process key-custody decision); HMAC with a shared pairing
secret (no non-repudiation, no multi-peer scaling); full E2E encryption (deferred per
v0.1 scope); a durable replay log surviving restart indefinitely (the deferred
offline-mailbox pattern).

**Reversal condition** (C5 §15, two halves, both algorithm/crate-scoped): an unfixable
`ed25519-dalek` MSRV conflict with the pinned Rust toolchain — not fired, `1.85 <=
1.98.1`; a Windows build failure for `ed25519-dalek` on the static-binary target — not
independently exercised, carried UNVERIFIED, not fired.

**Gate dependency.** Gate **G5 (Provenance)**, `NOT RUN` — this decision designs the
mechanism G5 exercises; it is not proven end to end.

### Decision 6 — Pairing and authorization

**Choice.** Zero-config pairing for same-device multi-harness, riding C2's already-
authenticated local IPC; a 6-digit, 120-second, 5-attempt short numeric code binding
exchanged device-key fingerprints for two devices on a LAN; default-deny,
per-OAC-session-id sender allowlists scoped by `working_directory`; message-delivery
authorization and any future action-level authorization (`turn/steer`, permission relay)
kept as two separate checks. OAC policy maps only onto authenticated Zenoh ACL subjects
(certificate common name or username), never `zid`. Source: `docs/planning/decisions/
C5-envelope-auth.md` §1 items 3, 7-9, §10-§12.

**Decisive evidence.** Same-device pairing needs no separate step because the OS itself
already authenticates the connecting peer as the same local user before any OAC-level
step could run (C5 §10(a)); the short code is chosen over file exchange because it needs
no shared filesystem access between two LAN devices and matches DESIGN.md's "no manual
certificate management" requirement for LAN mode (C5 §10(b)); the fingerprint truncation
floor is fixed at 128 bits because it is security-load-bearing across three roles
(pairing MITM defeat, Zenoh certificate common name, envelope `key_id`) (C5 §10(b));
default-deny and `working_directory` scoping directly reuse C4 §5's registration-record
field to close the cross-project-leakage threat (C5 §11).

**Rejected alternative, stated inline** (C5 §10(b)): file exchange, over the chosen short
numeric code, for requiring shared filesystem access two LAN devices may not have.

**Reversal condition.** None separately stated for the pairing/authorization design
itself beyond decision 5's signature-crate reversal (§15, above), which this decision's
signing mechanism shares. The v0.1 posture (no online key rotation, manual re-pair only)
is a deliberate scope limit (C5 §4), not an open reversal test.

**Gate dependency.** Gate **G5 (Provenance)**, `NOT RUN` — the same caveat as decision 5.

### Decision 7 — Key storage

**Choice.** OS credential stores via `keyring` `4.2.0` (Windows Credential Manager via
`windows-native-keyring-store`; macOS/iOS Keychain via `apple-native-keyring-store`;
Linux Secret Service via `zbus-secret-service-keyring-store`/
`dbus-secret-service-keyring-store`, or `linux-keyutils-keyring-store` for the
headless/no-D-Bus path), with an `age` `0.12.1`-encrypted-file fallback engaging
specifically on headless Linux with no reachable Secret Service bus. License:
`keyring` MIT OR Apache-2.0, `age` MIT OR Apache-2.0 — OAC elects the Apache-2.0 arm of
each, per decision 12's dependency table. Source: `docs/planning/decisions/
C4-session-identity.md` §10-§11; license election restated from `docs/planning/
decisions/C1-language-runtime.md` §7-§8, §10.

**Decisive evidence.** `keyring` `4.2.0`'s own `Cargo.toml` names each per-platform
backend crate directly, verified not assumed (C4 §10, citing C1 §8); the encrypted-file
fallback uses `age`'s pre-specified `age-encryption.org/v1` format rather than inventing
OAC's own file format (C4 §11, "smaller design" preference); `scrypt::Recipient` is
deliberately used over the format's own "for programmatic use, use `x25519::Identity`"
guidance, because the KDF's per-guess cost is a defense-in-depth property this decision's
threat model wants for an exfiltrated file (C4 §11's recorded override).

**Rejected alternatives, one line each** (C4 §14): a custom OS-keystore wrapper instead
of `keyring` (duplicates already-pinned, already-evidenced work); keys in a plain config
file (defeats the entire purpose of the credential store). The RustCrypto primitive
composition (`chacha20poly1305` + `argon2`) is rejected as the fallback's own mechanism,
in favor of `age`'s pre-specified format, for requiring OAC to design and version its own
file format from scratch (C4 §11).

**Reversal condition** (C4 §15, key-storage half): reverses per-platform to the
encrypted-file fallback as **primary** if that platform's `keyring` backend fails C1
§12's falsifiable test. Not fired — the Windows test passes today.

### Decision 8 — Provider-facing trust rendering

**Choice.** Claude inbound: a fixed five-key `meta` attribute set —
`oac_sender`, `oac_device`, `oac_session`, `oac_message_id`, `oac_reply_to`, all
identifier-safe ASCII constants, never content- or peer-derived, enforced by a const key
table plus a contract test plus a refusal fixture that refuses delivery rather than
shipping under-labelled provenance. Content (`content`) and provenance (`meta`) never
mix; body text shaped like a forged `<channel>` tag is never parsed as live markup.
Permission relay (`claude/channel/permission`) stays off by default in v0.1. Source:
`docs/planning/decisions/C6-trust-rendering.md` §1 items 1-3, 6, §2-§4, §7.

**Decisive evidence.** `oac-claude-channels` §2's silently-dropped-`meta`-key behaviour
means a bad key ships an under-labelled message with no signal — the const table plus
contract test plus refusal fixture is what turns that silent gap into a loud, fail-closed
refusal on OAC's own side (C6 §3); permission relay off by default is justified three
ways — it would collapse two separately-decided authorizations (message delivery vs. tool
approval), it would weaken the one Claude-side consent step
(`--dangerously-load-development-channels`), and it restates conflict C10's own finding
(C6 §7, resolving C10, §4 below).

**Rejected alternatives, one line each** (C6 §13): provenance inside the Claude `content`
string (indistinguishable from a forged claim, defeats `meta`'s entire purpose);
permission relay on by default behind an allowlist (the exact collapse C10 names).

**Reversal condition** (C6 §14, Claude half): reverses if Claude Code ships a documented
structured-provenance surface beyond the current `content`/`meta` string-and-map pair.
Not fired at the pinned version.

**Gate dependency.** Gate **G1 (Claude wake)**, `NOT RUN`, exercises the real Claude
rendering this design assumes; gate **G5 (Provenance)**, `NOT RUN`, exercises whether
forged-sender content is actually shown contradicted by machine-set provenance.

### Decision 9 — Outbound symmetry

**Choice.** Both harnesses send through four identical-schema OAC MCP tools — `send`,
`reply`, `list_sessions`, `whoami` (`whoami` never returns a credential); inbound stays
provider-native, an asymmetry the design states is deliberate because no provider offers
a shared inbound mechanism. Codex reachability is confirmed via `codex mcp add`, not the
deleted `codex mcp-server`. Codex inbound provenance rides a machine-generated header
plus a receiver-generated, per-delivery-unguessable delimiter inside the
`{type:"text",text}` item; `turn/steer` is never used on the inbound path. Codex reply
correlation is a layered rule: explicit `in_reply_to` (validated, not trusted on its
face), an adapter-independent thread-id/turn-id binding, and an explicit
inferred/uncorrelated downgrade rather than silent attribution. Source: `docs/planning/
decisions/C6-trust-rendering.md` §1 items 4, 7-9, §5, §8-§10.

**Decisive evidence.** The delimiter must be generated by the *receiving* adapter after
the envelope arrives, never derived from any sender-controlled or sender-computable
field — an earlier draft's sender-nonce-derived delimiter is recorded and corrected in
C6 §5 as exactly the defect a fixed/static delimiter would have; the layered
correlation rule exists because Codex has no channel-tag convention and a model-echoed
`in_reply_to` alone is untrusted content that must be validated against independently-
tracked thread/turn state (C6 §10, resolving conflict C9, §4 below).

**Rejected alternatives, one line each** (C6 §13): a fixed static delimiter (guessable
and pre-plantable by an attacker — the corrected design's predecessor); trusting
model-echoed correlation ids alone (exactly the "(a) alone" design the layered rule
exists to avoid); a Codex-side channel-tag convention invented by OAC (an unsupported
surface Codex was never designed to parse).

**Reversal condition** (C6 §14, Codex half): reverses if Codex gains a native
channel-tag convention — a documented structured-metadata field on `turn/start`/
`thread/queue/add` distinct from the `text` payload. Not fired at the pinned version.

**Gate dependency.** Gate **G2 (Codex live inject)**, `NOT RUN`; gate **G5
(Provenance)**, `NOT RUN`.

### Decision 10 — Transport mapping

**Choice.** Key expressions are a private, one-way-derived hash of the opaque session id
(never guessable, never on the wire in reverse, no broadcast/room-shaped pattern);
presence maps onto Zenoh liveliness tokens plus history-capable liveliness subscribers
(three neutral states — `online`/`unreachable`/`unknown` — no polling); local mode binds
`127.0.0.1`, runs no `zenohd`, leaves multicast scouting on by default (pinned `1.10.1`
satisfies the `>= 1.10.0` loopback-discovery floor) and additionally binds a
localhost TLS listener with an auto-generated, unmanaged certificate to satisfy gate G3's
own pass criterion; LAN mode defaults to TLS (QUIC named as the alternative) using C5
§10(b)'s pairing-issued certificates, common name derived from the device-key
fingerprint, default-deny ACL; ACL subjects are certificate common name or username only,
never `zid`; only the stable `zenoh`/`zenoh-ext` API surface is used, no `unstable`
feature; every Zenoh type stays inside `transports/zenoh/`. Source: `docs/planning/
decisions/C7-zenoh-transport.md` §1.

**Decisive evidence.** The `oac-boundaries` containment grep (`references/
mechanical-checks.md` check 1) is the mechanical enforcement of the module boundary,
quoted and cross-referenced rather than re-invented, with a stated, checked correction
that it does not catch `snake_case`-embedded identifiers and does not run against
`adapters/`/`cli/` at all (C7 §2); every Zenoh feature this design touches is enumerated
against its stable-or-`unstable` status with a first-party citation each (C7 §8); local
mode's TLS listener is added after an earlier draft's conflict with G3's own pass
criterion was recorded and resolved in-document (C7 §5, also tracked at `docs/planning/
STATUS.md`'s "Open conflicts").

**Two gaps recorded, not silently resolved** (C7 §4): the liveliness-token presence
mapping carries only reachability, not the full `PresenceRecord` (harness ownership,
capabilities, active-inbound support) DESIGN.md asks for; no discovery path is yet
defined by which a peer learns an *unknown* session's opaque id, though presence/
discovery is in v0.1 scope. Both stay open, tracked at `docs/planning/STATUS.md`'s "Open
conflicts."

**Rejected alternatives, one line each** (C7 §10): a `zenohd` router on the default
local path (turns an optional peer-to-peer transport into a separately administered
server); `zid` as ACL subject or principal (explicitly unauthenticated); public/guessable
key expressions derived from the display URI (defeats the never-guessable requirement);
transport TLS as the authenticity proof (Zenoh signs no payload); multicast scouting off
by default in local mode (trades the zero-file default for a manual step with no
offsetting benefit at the pinned version); `zenoh-ts` (needs a router, not native);
plugin-backed presence-history storage (needs the router or a deprecated API).

**Reversal condition** (C7 §11): reverses per-platform to the fixed-local-endpoint
fallback if gate G3 finds multicast scouting unreliable on a tested platform; reverses
every version-pinned fact in this document on a future Zenoh version bump. Neither has
fired.

**Gate dependency.** Gate **G3 (Zenoh local peer)**, `NOT RUN` — this decision designs
the mapping G3 exercises; it is not proven end to end on any platform.

### Decision 11 — Configuration and CLI model

**Choice.** Zero-file local default: IPC path, device key, and local-only transport all
work with no config file present; a config file, when present, is optional and follows
`flag > env > file > default` precedence. CLI surface: `oac start` (idempotent,
foreground by default, `--detach` to background), `oac status`, `oac sessions`,
`oac doctor` (preflight checks), `oac mcp-shim` (the stdio server harnesses spawn, not
meant to be typed by hand). Two verified one-line launch commands: `claude
--dangerously-load-development-channels server:oac` for Claude Code; `codex mcp add oac
-- oac mcp-shim` for Codex outbound registration. Source: `docs/planning/decisions/
C2-process-model.md` §6-§7.

**Decisive evidence.** The Claude launch command is verified against the first-party
`channels-reference.md` example shape rather than the presumptive `--channels
server:oac` form the task breakdown originally sketched, because `oac mcp-shim` is not on
Anthropic's allowlist and the development-flag form is what the docs actually show for a
non-allowlisted server (C2 §6); every CLI table command is reachable with no config file
at all (C2 §7).

**Deliberately omitted, stated as the smallest-design preference** (C2 §8, functioning
as this decision's own rejected-alternatives list): no supervisor/service installer, no
auto-start on login, no multi-user daemon, no durable message store — each omitted
because ADR-001's Validation criterion requires none of them, not because they were
overlooked.

**Boundary self-check, carried from C2.** C2 §9 argues, point by point, that the daemon
does not become "a separately administered server for normal local use" (`[ADR-001
Boundary]`, quoted there verbatim) — user-owned process, auto-startable by the shim
rather than separately administered, not installed as a service. "This argument holds,"
per C2 §9's own stated conclusion.

**Reversal condition.** Shares decision 2's process-model reversal condition (§ above,
C2 §11) — this decision's CLI/config surface is the same daemon-plus-shim design decision
2 fixes; no separate reversal test is stated for the CLI surface alone.

### Decision 12 — Dependencies and licenses

**Choice.** Every third-party crate this planning package names, with license, reason,
copyleft flag, and Apache-2.0-compatibility verdict, per the tables below (reproduced from
their source documents, not re-derived):

| Crate | Version | License | Apache-2.0 compatible? | Source table |
|---|---|---|---|---|
| `rmcp` | `3.4.0` | Apache-2.0 | Yes — same license | C1 §10 |
| `codex-app-server-client`/`-protocol`/`-transport` | `0.154.0` @ commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340` (git dep) | Apache-2.0 | Yes — same license | C1 §10 |
| `zenoh` / `zenoh-ext` | `1.10.1` | EPL-2.0 / Apache-2.0 (dual) | **Yes — OAC elects the Apache-2.0 arm** (EPL-2.0 flagged, weak copyleft) | C1 §7, §10; C7 §8 |
| `keyring` | `4.2.0` | MIT OR Apache-2.0 | Yes — OAC elects the Apache-2.0 arm | C1 §10 |
| `keyring-core` | `1.0.0` | MIT OR Apache-2.0 | Yes | C1 §10 |
| `windows-native-keyring-store` | `1.1.0` | MIT OR Apache-2.0 | Yes | C1 §10 |
| `interprocess` | `2.4.4` (candidate) | 0BSD OR Apache-2.0 | Yes — OAC elects the Apache-2.0 arm | C1 §10 |
| `age` | `0.12.1` | MIT OR Apache-2.0 | Yes — OAC elects the Apache-2.0 arm | C4 §11 |
| `ed25519-dalek` | `3.0.0` | **BSD-3-Clause** (own row shape, not a dual license) | Yes — permissive, no OR-clause | C5 §2, §18 |
| `serde_jcs` | `0.2.0` | MIT OR Apache-2.0 | Yes — OAC elects the Apache-2.0 arm | C5 §3, §18 |

**Decisive evidence.** Every row above was fetched from crates.io/GitHub first-party
sources at the pinned version, not recalled from memory, per each source document's own
citation trail (C1 §3-§10, C4 §10-§11, C5 §2-§3, C7 §8). `zenoh`'s EPL-2.0 arm is
explicitly flagged as copyleft and explicitly not the arm OAC uses.

**Rejected alternatives, one line each, per dependency choice**: `ring` (bundled
BoringSSL-derived assembly, cross-compilation cost) and `libsodium` bindings (C dependency)
rejected for Ed25519 in favor of pure-Rust `ed25519-dalek` (C5 §2); ECDSA P-256 rejected,
no offsetting benefit over Ed25519's deterministic-nonce property (C5 §2); deterministic
CBOR (`ciborium`) rejected for canonicalization, would require a second signed
representation alongside the JSON wire format JCS already canonicalizes (C5 §3); the
RustCrypto `chacha20poly1305`+`argon2` composition rejected for the key-storage fallback
in favor of `age`'s pre-specified file format (C4 §11); Go/Python/TypeScript/C++/JVM
rejected at the language level (decision 1, above), which determines the whole
dependency set's ecosystem.

**Reversal condition.** Per-crate, as stated at each crate's own decision (decision 1's
Rust MCP SDK/credential-store test, C1 §12; decision 5's Ed25519 MSRV/Windows-build test,
C5 §15).

**Full transitive sweep deferred to Stage 6**, recorded not run, per C1 §10 and C4 §11:
`cargo deny`/`cargo license` against the resolved dependency graph has no `Cargo.lock` to
run against yet (no code has landed). This is `oac-release`'s job, not this file's.

---

## 2. ADR-001 amendments A1-A3

**Only three amendment numbers are allocated, in strict sequence: A1, A2, A3. The next
free number is A4.** Checked, per `oac-evidence` §6 step 4, against both
PLANNING-PROMPT.md Appendix A's "Expected resolution" column (which earmarks exactly A1
for C1, A2 for C2, A3 for C3, and no further number) and this file (no amendment above A3
is issued anywhere in this document). Every decision document that considered whether its
own finding needed a new numbered amendment (C1 §2, C4 §8, C5 §6, C6 §17, C7 §14) checked
this same allocation and found no `ADR-001.md` text needing correction beyond what A1-A3
already fixed — each closed its own conflict as `RESOLVED-IN-DECISION` instead (§3
below), explicitly declining to issue `A4`. `A4` remains the next number a future
amendment would take, and is not issued by this file.

Every quotation below is copied from `docs/planning/ADR-001-AMENDMENTS.md`, not
independently re-fetched from `ADR-001.md` — per this file's own Generated-summary
notice and `oac-evidence` §2's "cite, do not re-quote a secondary source" rule.

### ADR-001-A1 — naming rename

**Resolves:** conflict C1.

**Old text (verbatim, six sites, `ADR-001.md`).** Attributed to
`docs/planning/ADR-001-AMENDMENTS.md` §ADR-001-A1:

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

**New text.** Product and repository: **Open Agent Channel (OAC)**; normative protocol
specification: **OAC Session Channels**; CLI binary: **`oac`**; `sessionchannels` is
retired. Applying this at each site: (a) title becomes "ADR-001: Open Agent Channel
(OAC) — Provider-Neutral Session Channels"; (b) "MCP Session Channels extension" becomes
"OAC Session Channels specification, packaged as an MCP extension" (see A3); (c) item 1
unchanged substance, "Session Channels" -> "OAC Session Channels"; item 2 heading "MCP
Session Channels" -> "OAC Session Channels"; (d) diagram label -> "OAC Session Channels";
(e) -> "OAC Session Channels operation"; (f) -> "OAC Session Channels/Zenoh".

**A1/A3 overlap-ordering note.** Sites (b) and (d) carry both A1's naming substitution
and A3's semantic correction (what MCP does for this layer) — apply A1's naming
substitution first, A3's semantic correction second, per `ADR-001-AMENDMENTS.md`
§ADR-001-A1's own "Overlap note."

**CLI name-conflict check for `oac`, carried forward in full (PLANNING-PROMPT.md §2
requires this be recorded here).** From `ADR-001-AMENDMENTS.md` §ADR-001-A1:

| Check | Result |
|---|---|
| crates.io `oac` | Not found (HTTP 404) |
| npm `oac` | **Found — conflict.** Inactive stub `oac@0.0.0`, published 2021-06-17, MIT, 84-byte tarball, no README — a name-squat, not an active competing tool |
| Homebrew formula `oac` | Not found (HTTP 404) |
| Debian/Ubuntu package `oac` | Not found (18 packages contain the substring "oac"; none named exactly `oac`) |
| `where`/`which oac` on dev machine | Not found on `PATH` |

**Verdict.** Free on crates.io, Homebrew, and Debian/Ubuntu, and absent from the dev
machine's `PATH`. Taken on npm by an inactive, contentless stub — **non-blocking**
because OAC's CLI is a Rust binary (decision 1, above) with no current npm-distributed
package plan. Recorded, not silently dropped: if OAC ever ships an npm-distributed
package under the name `oac`, the existing squat must be resolved first (reclaim via npm
support, or use a scoped name such as `@openagentchannel/oac`).

**Rationale (summarized; full text at the citation).** PLANNING-PROMPT.md §2 resolves the
OAC/Session-Channels naming split once, at the top of the plan, and requires the
resolution applied consistently across the package (§11 item 8). The repository's own
README and Apache-2.0 LICENSE already name it OAC — ADR-001 and DESIGN.md are the
documents that still carried the pre-rename name, so this amendment brings them into
line with the naming already in force at the repository root.

### ADR-001-A2 — Validation criterion redefinition

**Resolves:** conflict C2.

**Old text (verbatim, `ADR-001.md` line 74, whole Validation criterion sentence),
attributed to `docs/planning/ADR-001-AMENDMENTS.md` §ADR-001-A2:**

```
Architecture is proven when an existing Claude Code session sends through Session Channels/Zenoh to an existing Codex harness session without receiver polling; Codex responds and Claude receives the response actively; neither side invokes or holds credentials for the other's model API; and sender identity/authorization are enforceable rather than inferred from content.
```

**New text:**

```
Architecture is proven when a Claude Code session, launched OAC-enabled, sends through OAC Session Channels/Zenoh to a Codex harness session, launched OAC-enabled, without receiver polling; Codex responds and Claude receives the response actively; neither side invokes or holds credentials for the other's model API; and sender identity/authorization are enforceable rather than inferred from content.
```

Only the two occurrences of "existing" are replaced with "launched OAC-enabled" (plus
A1's naming rename applied at the same site); every word from "without receiver polling"
onward is kept byte-identical.

**Guard — G2 not claimed passed.** This amendment does not claim gate G2 has passed —
every gate verdict is `NOT RUN`, and whether implicit Codex daemon attach executes by
default at runtime in the pinned release is UNVERIFIED (decision 2's gate dependency,
above).

**Scope note.** The compatibility shim boundary for the Claude/Codex preview-and-
experimental surfaces stays out of this amendment's scope — recorded as new register
entry C11 (§4 below) instead of invented here.

**Rationale (summarized).** ADR-001's Validation criterion, read literally, requires "an
existing" session of each kind — language that invites reaching into an arbitrary
already-running session nobody launched OAC-enabled, exactly `oac-boundaries` boundary
7's named drift risk. PLANNING-PROMPT.md §4's structural finding shows neither harness
supports attach-to-arbitrary-running-process through a supported interface; redefining
"existing" as "launched OAC-enabled" keeps the criterion provable without a boundary
violation, leaving every other clause untouched.

### ADR-001-A3 — MCP's role reworded

**Resolves:** conflict C3 (wording only — the extension identifier itself is decision 3,
above).

**Old text (verbatim, two sites, `ADR-001.md`), attributed to `docs/planning/
ADR-001-AMENDMENTS.md` §ADR-001-A3:**

Line 15, in full:

```
Create a provider-neutral **MCP Session Channels extension** with three replaceable layers:
```

Line 18, list item 2, in full:

```
2. **MCP Session Channels** — define identity, addressing, capabilities, messages, replies/correlation, presence, delivery, and security semantics.
```

**New text.** Line 15 becomes:

```
Create a provider-neutral **OAC Session Channels specification**, packaged as an MCP extension for capability negotiation, the tool surface, and `_meta` provenance only, with three replaceable layers:
```

List item 2 becomes:

```
2. **OAC Session Channels** — the neutral specification: identity, addressing, capabilities, messages, replies/correlation, presence, delivery, and security semantics. Packaged as an MCP extension for capability negotiation, the tool surface, and `_meta` provenance only; MCP is not the delivery mechanism.
```

The three-layer structure and layers 1 and 3 (provider adapters, transport plugins) are
unchanged — only layer 2's description changes, to state what MCP does and does not
provide for it. Active inbound delivery is always provider-native: Claude Code via
`notifications/claude/channel`; Codex via app-server live-inject (`thread/queue/add`,
`turn/steer`, `turn/start`).

**Rationale (summarized).** ADR-001's original phrase reads as if MCP itself carries the
active-delivery semantic. The current MCP era removed server-initiated push entirely; no
MCP revision, past or present, defines an external-event-becomes-a-user-turn semantic.
This amendment corrects the wording without changing the architecture: OAC Session
Channels remains layer 2 of the same three-layer decision, MCP remains negotiation/
tool-surface packaging, delivery remains provider-native.

**A1/A3 overlap-ordering note.** Restated from A1, above: at sites (b) and (d), apply
A1's naming substitution first, then A3's semantic correction.

---

## 3. Conflict register C1-C10 (resolved)

Reproduced from `docs/planning/ADR-001-AMENDMENTS.md` §"Conflict register C1-C10
(resolved)". Status legend, defined once there and reused here unchanged:
**RESOLVED-HERE** (an A-amendment closes it); **RESOLVED-BY-EVIDENCE** (already closed by
B1/B2 verification); **RESOLVED-BY-DECISION** (an A-amendment plus a landed C-series
decision document together close it); **RESOLVED-IN-DECISION** (closed entirely by a
C-series decision document, no A-amendment issued because no `ADR-001.md` text needed
correction); **ASSIGNED** (an open, named task closes it — never marked resolved where
the resolution is an unrun gate). Each "Resolution lives in" path below was checked
against its source document during this file's assembly and confirmed to still exist and
point at the stated section.

| # | Conflict (short) | Status | Resolution lives in | Evidence |
|---|---|---|---|---|
| C1 | Product named OAC in repo, "Session Channels" in ADR/DESIGN, CLI `sessionchannels` | RESOLVED-HERE | `ADR-001-AMENDMENTS.md` §ADR-001-A1 | PLANNING-PROMPT.md §2 |
| C2 | ADR Validation criterion says "existing Claude Code session" / "existing Codex harness session" | RESOLVED-HERE | `ADR-001-AMENDMENTS.md` §ADR-001-A2 | PLANNING-PROMPT.md §3.1, §3.2, §4 |
| C3 | ADR names the layer an "MCP Session Channels extension" | RESOLVED-BY-DECISION | `ADR-001-AMENDMENTS.md` §ADR-001-A3 (wording) + `docs/planning/decisions/C3-spec-packaging.md` (identifier `io.github.rossgraeber/oac-session-channels`) | PLANNING-PROMPT.md §3.3 |
| C4 | DESIGN envelope `security.signature` "implementation-defined" vs enforced-provenance acceptance criterion | RESOLVED-IN-DECISION | `docs/planning/decisions/C5-envelope-auth.md` §6 | PLANNING-PROMPT.md §3.4 — "Zenoh provides no application-layer message signing" |
| C5 | Claude needs legacy MCP; Codex tool path may negotiate current MCP | **ASSIGNED** — gate G4 (`NOT RUN`), decision tasks C2/C3 | Gate `docs/planning/gates/G4-result.md`; decision 3 above (§7 of this file) | PLANNING-PROMPT.md §3.1 MCP version constraint + §3.3; `STATUS.md` G4 fallback "two entry points, one core" |
| C6 | No Claude acknowledgement vs DESIGN `accepted` delivery state | RESOLVED-IN-DECISION | `docs/planning/decisions/C5-envelope-auth.md` §9 | PLANNING-PROMPT.md §3.1, "Claude Code sends no acknowledgement" |
| C7 | ACP is client-owned-session, not a channel | RESOLVED-BY-EVIDENCE | `docs/planning/v0.1/01-capability-matrix.md` §4 | REVERIFICATION-B2.md §3.5 — HOLDS |
| C8 | URI leaking device/harness vs no transport-concept leak | RESOLVED-IN-DECISION | `docs/planning/decisions/C4-session-identity.md` §8 | DESIGN.md Addressing section; ADR-001 Boundary |
| C9 | Codex has no channel-tag convention | RESOLVED-IN-DECISION | `docs/planning/decisions/C6-trust-rendering.md` §10 | PLANNING-PROMPT.md §3.2 turn/event model |
| C10 | Permission relay lets any allowlisted sender approve tools | RESOLVED-IN-DECISION | `docs/planning/decisions/C6-trust-rendering.md` §7 | PLANNING-PROMPT.md §3.1, §7 — off by default in v0.1 |

**C5 stays `ASSIGNED`, not upgraded.** Per the acceptance-box instruction: C5's
resolution is an unrun gate (G4), so its status remains `ASSIGNED` here exactly as
`ADR-001-AMENDMENTS.md` states it — this file does not promote it to `RESOLVED-*` on the
strength of the packaging decision alone.

---

## 4. New/open register entries C11-C12

Both remain **open** — neither is closed by this file, consistent with `docs/planning/
ADR-001-AMENDMENTS.md`'s own statement that neither is closed there either.

| # | Conflict (short) | Status | Resolution lives in | Evidence |
|---|---|---|---|---|
| C11 | Compatibility shim boundary is UNNAMED for both the Claude Code Channels research-preview surface and the Codex experimental live-inject surface (both surfaces) | **ASSIGNED** | A C-series decision or a `DESIGN.md` update naming the module/interface; narrowed but not closed by `docs/planning/decisions/C4-session-identity.md` §16 (harness-native-id capture is fixed as daemon-owned-only, reached only via the hook/JSON-RPC surfaces C4 §3-§4 name — the module name/path itself stays owned by Epic F/G adapter implementation) | `docs/planning/STATUS.md` "Open UNVERIFIED items" |
| C12 | `DESIGN.md` still carries retired names (`sessionchannels`, "Session Channels", "MCP Session Channels extension") after ADR-001-A1 | **ASSIGNED**, owner Epic A task A9 plus a `DESIGN.md` edit | The four `DESIGN.md` legacy-name sites below | `docs/planning/ADR-001-AMENDMENTS.md` "Carried to later tasks" |

**The four `DESIGN.md` legacy-name sites, carried verbatim from `docs/planning/
ADR-001-AMENDMENTS.md` "Carried to later tasks":**

- Line 1 title: `# Session Channels — Software Design`.
- Lines 21-24, CLI block: `sessionchannels start` / `status` / `sessions` / `doctor`.
- Line 30 heading: `### MCP Session Channels extension`.
- Line 33, normative-concept sentence: "a harness advertising active inbound **Session
  Channels** support accepts an authorized external channel message as input to the
  addressed live session without application-level polling."

`ADR-001.md` outranks `DESIGN.md` in the precedence chain, so the A1 rename is
authoritative even before `DESIGN.md`'s own text is edited; this file does not edit
`DESIGN.md` (that is task A9's job), and does not stub it.

---

## 5. Boundary self-check (per `oac-boundaries`)

One line per check, decision-by-decision, per issue #21's acceptance box 4:

- **No decision has OAC calling a provider model API as a substitute for a native
  harness.** Every inbound path is provider-native (decisions 8-9): Claude Channels
  notifications, Codex app-server live-inject. No decision routes a message through the
  Anthropic or OpenAI model APIs directly. **Verdict: holds.**
- **No decision implements inference, model routing, or context management.** Decision 9's
  reply-correlation rule (C6 §10) resolves *which prior message a reply targets*, using
  thread/turn ids and explicit correlation fields — never which harness "should" answer
  or a summarized/trimmed context. **Verdict: holds.**
- **No decision reads or reuses harness credentials.** Decision 7's credential-boundary
  self-check (C4 §12, cited verbatim: "OAC MUST NOT read `CODEX_HOME/auth.json`... OAC
  MUST NOT read any Claude Code OAuth token") and decision 2's daemon key custody (C2 §1
  leg 2, keys are OAC-issued device keys only) are the evidence. `keyring`/`age` are
  generic secret-storage clients; OAC's own code scopes which service/account/path they
  touch, confined to OAC-issued keys only. **Verdict: holds.**
- **No decision introduces a shared cross-provider context store.** Decision 5's
  duplicate-suppression store (C5 §8) holds only `(key_id, nonce)` pairs for replay
  detection, no message content, bounded to a ~10-minute rolling window — explicitly
  checked against boundary 11 (durable offline mailboxes) in its own source and found not
  to be one. No decision defines a message-content store shared across providers.
  **Verdict: holds.**
- **No decision abstracts provider auth.** Decision 6's pairing (C5 §10) is OAC-to-OAC —
  it authenticates a device-key exchange between two OAC installations, never a login to
  Claude or Codex; decision 4 §12's explicit check: "OAC never issues a token presented to
  Claude or Codex in place of the harness's own authentication." **Verdict: holds.**
- **No adapter polls while claiming active inbound.** Decision 10's presence mapping is
  explicit on this point (C7 §4: "No polling — the liveliness subscriber stream only"),
  and decision 9's Codex inbound path uses live-inject calls
  (`turn/start`/`thread/queue/add`), never a polled resource read. **Verdict: holds.**
- **No Zenoh concept escapes `transports/zenoh/`.** Decision 10's containment boundary
  (C7 §2) is the enforceable rule, with the `oac-boundaries` check-1 grep quoted rather
  than re-invented, and a checked correction recorded (the grep does not catch
  `snake_case`-embedded identifiers or cover `adapters/`/`cli/`). Every neutral-layer
  decision in this file (4-6, 8-9, 11) is Zenoh-vocabulary-free, confirmed by re-reading
  each cited source section. **Verdict: holds, with the stated lint-coverage gap carried
  open, not hidden** (C7 §2, §9's `zid`-misuse threat row).

**Conclusion, stated as a verdict.** Every boundary check above holds. No decision in
this file crosses an ADR-001 `MUST NOT`, and the one known enforcement gap (the
containment lint's `snake_case`/`adapters`/`cli` coverage) is recorded as an open,
tracked item rather than glossed over as closed.

---

## 6. Evidence-standard pass (per `oac-evidence` §8)

- **Every version number traceable to `docs/planning/PINS.md`.** Rust `1.98.1`, `rmcp`
  `3.4.0`, `zenoh` `1.10.1`, `keyring` `4.2.0`, `age` `0.12.1`, `ed25519-dalek` `3.0.0`,
  `serde_jcs` `0.2.0`, Claude Code `v2.1.274`, `@openai/codex@0.154.0` — every one of
  these appears in `PINS.md`'s pin table (some, `ed25519-dalek`/`serde_jcs`, added by
  decision 5's own source document, C5 §18, and carried into `PINS.md` in the same
  change).
- **Every provider surface labelled.** Claude Channels = **research preview**; Codex
  app-server = **experimental** (per-method gating via `capabilities.experimentalApi`);
  MCP `2026-07-28` = current/**supported**, `2025-11-25` = legacy/**supported**; Zenoh =
  **supported**, `1.10.1`. Each label is restated at first mention within this file's
  decision sections, matching the label each source `C<n>` document already carries.
- **No UNVERIFIED label dropped.** This file references `docs/planning/STATUS.md`'s
  "Open UNVERIFIED items" list by pointer wherever a decision depends on one (decision 2's
  daemon-attach runtime behaviour, decision 4's Claude-resume behaviour, decision 9's
  Codex reply-echo reliability) rather than copying the list — the list is not restated
  here in full, per that section's own "single source of truth" framing.
- **No new first-party URL fetched in this task.** Every citation in this file is a
  pointer to an already-landed source document (`ADR-001-AMENDMENTS.md`, `C1`-`C7`) or to
  `PINS.md`/`STATUS.md`; no external source was independently re-fetched to produce this
  file, consistent with the Generated-summary notice at the top.

---

## 7. House-style pass (per `oac-planning-package` §2, §6)

- **Decisions, not options.** All twelve §5 decisions above are stated as made choices
  with rejected alternatives, not open questions — including decision 3's dual-era
  sub-element, which is stated as gate-decided (a closed category, not an open one) with
  the deciding gate named.
- **No code blocks except verbatim amendment text and interface-shaped names.** The only
  code-fenced blocks in this file are the ADR-001-A1/A2/A3 old/new text quotations (§2)
  and the license-inventory/conflict-register tables, which are data tables, not code.
- **Self-contained, cross-referenced by repo-relative path.** Every source cited above
  names its repo-relative path on first use per section; no section assumes the reader
  has `ADR-001.md`, `DESIGN.md`, `PLANNING-PROMPT.md`, `PINS.md`, `STATUS.md`, the
  capability matrix, the gating findings, or any `C<n>` decision document already open.
- **`sessionchannels`/bare "Session Channels" grep.** This file's only occurrences of
  bare "Session Channels" or `sessionchannels` are inside the marked ADR-001-A1 old-text
  verbatim quotation blocks (§2), which are pre-rename quotations by design, consistent
  with the Naming paragraph's stated exception at the top of this file.

---

## 8. Close-out against issue #21's four acceptance boxes

Ticked against line ranges of this file, matching the pattern each `C<n>` decision
document and `docs/planning/v0.1/02-gating-findings.md` use for their own close-out
sections.

- [x] **Acceptance box 1 — every §5 decision made, or gate-decided with the gate named.**
      §1 "Decisions" states this explicitly up front (the gate-dependency note) and all
      twelve subsections make their choice; decision 3's dual-era sub-element names gate
      G4 as the decider, not left open.
- [x] **Acceptance box 2 — ADR-001-A1 through A3 reproduced with verbatim old/new text
      and rationale, cited to the amendments ledger, not re-derived.** §2, all three
      amendments, each quoting `ADR-001-AMENDMENTS.md` directly; the amendment-numbering
      statement (top of §2) confirms A1-A3 are the only numbers allocated and A4 is next.
- [x] **Acceptance box 3 — the conflict register reproduced with status, resolution
      location, and evidence, each checked to still exist and point correctly.** §3 (C1-
      C10, with C5 kept `ASSIGNED`, not upgraded) and §4 (C11-C12, both kept open).
- [x] **Acceptance box 4 — explicit boundary self-check, stated as a verdict.** §5,
      one line per `oac-boundaries` check, decision-by-decision, concluding "every
      boundary check above holds."

Additional self-review items this file also satisfies, per `oac-planning-package` §5's
checklist item 3 ("Every §5 decision is made, not deferred, or has a gate whose result
decides it") and item 8 (naming resolution applied consistently): confirmed by §1 and
the Naming paragraph at the top of this file, respectively.
