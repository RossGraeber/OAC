# C5: Envelope authenticity, replay defence, pairing, and authorization

**Issue:** #18 (Epic C, backlog key `C5`). **Depends on:** B2. **Source:**
PLANNING-PROMPT.md §5 decisions 5 and 6, §7; conflicts C4 and C6.

**Status:** Decided.

**Standalone-ledger note.** This file lives at `docs/planning/decisions/` because
`docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4) does not exist yet.
It folds into that file, unedited in substance, once A4 lands — mirroring
`docs/planning/ADR-001-AMENDMENTS.md`'s own standalone-ledger rationale (see that file's
opening paragraph) and `docs/planning/decisions/C4-session-identity.md`'s own identical
note. `docs/planning/STATUS.md` carries a one-line pointer to this file until then.

**Provenance-proof caveat, stated once up front.** Per `docs/planning/STATUS.md`'s Gate
verdicts table, **gate G5 (Provenance) is `NOT RUN`**. This document designs the
envelope-authenticity mechanism G5 will exercise; it does not assert that provenance,
signature verification, or any other mechanism below is proven working end to end. Every
claim of the form "the mitigation is X" in §13's threat table is a **designed**
mitigation, not a **proven** one, exactly as `docs/planning/decisions/
C4-session-identity.md` §13 already states for its own threat rows.

---

## 1. Choice

OAC's envelope authenticity, replay defence, pairing, and authorization model is:

1. **Signature algorithm:** Ed25519, via crate `ed25519-dalek` (§2), with strict
   verification (`verify_strict`, §2) mandatory on every inbound envelope.
2. **Canonical signed form:** JSON Canonicalization Scheme (RFC 8785, JCS), via crate
   `serde_jcs` (§3), over a domain-separated byte string (§3).
3. **Signing key:** one Ed25519 keypair per device (§4), the same device key
   `docs/planning/decisions/C4-session-identity.md` §1a already names as the identity
   root — sessions never hold their own keys (§4).
4. **Signed field set:** every envelope field except `security.signature` itself (§5).
5. **Replay defence:** a `created_at` accept-window plus a per-envelope CSPRNG nonce,
   deduplicated on `(security.key_id, security.nonce)`, not on `id` alone (§7), against
   an in-memory, per-device, daemon-held duplicate-suppression store (§8).
6. **Delivery receipts:** three honest states — `accepted-by-adapter`,
   `handed-to-harness`, `unknown` — mapped onto DESIGN's delivery-state list (§9).
7. **Pairing:** zero-config for same-user multi-harness on one device (via C2's
   already-authenticated local IPC); a short numeric code binding exchanged device-key
   fingerprints for two devices on a LAN (§10).
8. **Authorization:** default-deny, per-OAC-session-id sender allowlists scoped by
   `working_directory` (§11), with authentication and authorization kept as two
   separate decisions (§11) — a verified signature authorizes delivery into the
   addressed session's input, never an action the content requests.
9. **Transport mapping:** OAC policy maps only onto authenticated Zenoh ACL subjects
   (certificate common name or username), never `zid`; the Zenoh ACL is a coarse
   pre-filter, the envelope signature is the authenticity proof (§12).

This is stated as the choice, not offered as one option among several.

## 2. Signature algorithm and crate

**Algorithm: Ed25519**, defined by RFC 8032, "Edwards-Curve Digital Signature Algorithm
(EdDSA)" — quoted: "Ed25519 is intended to operate at around the 128-bit security
level" and provides "high performance, deterministic signatures without requiring
unique random numbers per signature, resilience against side-channel attacks, and
compact key and signature sizes of 32 and 64 bytes respectively." Source:
https://www.rfc-editor.org/rfc/rfc8032, retrieved 2026-09-17.

**Crate: `ed25519-dalek` `3.0.0`.**

- Version and license: `max_stable_version` `3.0.0`, `license` `BSD-3-Clause`,
  `repository` `https://github.com/dalek-cryptography/curve25519-dalek/tree/main/ed25519-dalek`.
  Source: https://crates.io/api/v1/crates/ed25519-dalek, retrieved 2026-09-17.
  Cross-checked directly against the crate's own `Cargo.toml` at the repository's `main`
  branch: `version = "3.0.0"`, `license = "BSD-3-Clause"`, `rust-version = "1.85"`.
  Source:
  https://raw.githubusercontent.com/dalek-cryptography/curve25519-dalek/main/ed25519-dalek/Cargo.toml,
  retrieved 2026-09-17.
- **License flag, per PLANNING-PROMPT §5 decision 12 and `docs/planning/decisions/
  C1-language-runtime.md` §10's dependency-inventory shape.** `BSD-3-Clause` is
  permissive and Apache-2.0-compatible, but it is **not** the `MIT OR Apache-2.0` dual
  shape every other OAC pin carries (`keyring`, `age`, `interprocess`, `rmcp`) and **not**
  an "elect one arm of a dual license" case either (`zenoh`'s EPL-2.0/Apache-2.0, per C1
  §7) — it is a single permissive license with no OR-clause. This gets its own
  dependency-inventory row (§18) rather than being folded into either existing pattern,
  exactly as the task instruction requires.
- **MSRV vs the pinned Rust toolchain.** `ed25519-dalek` `3.0.0`'s own `rust-version` is
  `1.85` (cited above). OAC's pinned Rust toolchain is `1.98.1`
  (`docs/planning/PINS.md` — "Rust toolchain"). `1.98.1 >= 1.85`: **satisfied.**
- **RUSTSEC-2022-0093 (double-public-key signing oracle), confirmed past.** Quoted:
  "Patched Versions: `>=2`"; "Affected Versions: Prior to v2.0." The v2.0 patch "revised
  public APIs to prevent decoupled private/public keypair inputs during signing, except
  through clearly labeled 'hazmat' APIs with explicit danger warnings." Source:
  https://rustsec.org/advisories/RUSTSEC-2022-0093.html, retrieved 2026-09-17. The
  pinned version `3.0.0` is far past the patched floor (`>=2`); this advisory does not
  apply to OAC's pin. OAC's own code MUST NOT use any `hazmat`-labelled API the crate
  exposes for decoupled key assembly — the ordinary `SigningKey`/`VerifyingKey` API
  (§2's `verify_strict` call below) is the only surface this decision authorizes.
- **Repository, confirmed current host.** `dalek-cryptography/curve25519-dalek` "is a
  collection of pure-Rust crates for elliptic curve cryptography" and "now hosts
  `ed25519-dalek` as a sub-crate," described as "An implementation of the EdDSA digital
  signature scheme over Curve25519." Source:
  https://github.com/dalek-cryptography/curve25519-dalek, retrieved 2026-09-17. No
  repository-level MSRV policy statement was found on the page as fetched — the
  crate-level `rust-version` field (cited above) is what this decision relies on, not a
  repository-wide policy statement (UNVERIFIED — see §16).

**Verification is strict, mandatory.** OAC calls `VerifyingKey::verify_strict`, verbatim
method name copied from docs.rs, not invented:

```rust
pub fn verify_strict(
    &self,
    message: &[u8],
    signature: &Signature,
) -> Result<(), SignatureError>
```

Doc text, quoted: "Strictly verify a signature on a message with this keypair's public
key." The method performs both scalar and point malleability checks and rejects
signatures failing them, checking the group equation `[8][S]B = [8]R + [8][k]A'` —
which is exactly the small-order/torsion-public-key rejection this decision requires.
Source: https://docs.rs/ed25519-dalek/3.0.0/ed25519_dalek/struct.VerifyingKey.html,
retrieved 2026-09-17. **Decision: every inbound envelope signature verification in OAC
calls `verify_strict`, never the non-strict `verify`.** The non-strict path is not used
anywhere in the envelope-verification code path this decision defines.

**Alternatives rejected, one line each:**

- **ECDSA P-256 (`p256`).** Rejected: no offsetting benefit over Ed25519 for this use
  case, and Ed25519's deterministic-nonce property (RFC 8032, above) avoids the
  nonce-reuse signature-forgery class ECDSA implementations must guard against
  separately.
- **Ed25519 via `ring`.** Rejected: `ring` bundles its own BoringSSL-derived assembly
  and build tooling, adding cross-compilation/vendoring cost to the single
  self-contained binary target (`docs/planning/decisions/C1-language-runtime.md` §9)
  that a pure-Rust `ed25519-dalek` avoids; `ring`'s license (a mix of OpenSSL-style and
  ISC-style terms across vendored components) is also a less clean fit than
  `ed25519-dalek`'s single `BSD-3-Clause`.
- **`libsodium` bindings.** Rejected: introduces a C dependency into a project whose
  `ADR-001.md` Boundary already requires the reference implementation "must not
  require... a separately administered server" and whose single-static-binary
  packaging goal (C1 §9) is a pure-Rust dependency tree; a C library and its own build
  toolchain requirement is exactly the packaging complexity that goal avoids.

## 3. Canonical serialization: JCS

A signature scope is meaningless without a byte-exact canonicalization of what was
signed — two semantically identical JSON documents with different key ordering or
whitespace must sign identically, or verification breaks on any re-serialization.

**Choice: JCS (RFC 8785, "JSON Canonicalization Scheme (JCS)"), via crate `serde_jcs`
`0.2.0`.**

- Version, license, repository: `max_stable_version` `0.2.0`, `license`
  `MIT OR Apache-2.0`, `repository` `https://github.com/l1h3r/serde_jcs`. Source:
  https://crates.io/api/v1/crates/serde_jcs, retrieved 2026-09-17.
- Published 2026-03-25, not yanked, 1,675,393 downloads at retrieval — an actively used
  crate, not an abandoned one. Source:
  https://crates.io/api/v1/crates/serde_jcs/0.2.0, retrieved 2026-09-17.
- API surface, quoted from the crate's own docs: top-level description "JSON
  Canonicalization Scheme (JCS)"; three public functions, `to_string` ("Serialize the
  given value as a String of JSON."), `to_vec` ("Serialize the given value as a JSON
  byte vector."), `to_writer` ("Serialize the given value as JSON into the IO
  stream."); the page's References section links RFC 8785, which the crate implements
  (function names verbatim; the RFC-8785-implements relationship is the page's own
  References link, not quoted description text). Source:
  https://docs.rs/serde_jcs/0.2.0/serde_jcs/, retrieved 2026-09-17. OAC's signing/
  verification code calls `serde_jcs::to_vec` on the signed field set (§5) to produce
  the exact byte string that is signed and verified.
- License election: `MIT OR Apache-2.0` — OAC elects the Apache-2.0 arm, the same
  election pattern `docs/planning/decisions/C1-language-runtime.md` §7 and
  `docs/planning/decisions/C4-session-identity.md` §10/§11 already apply to `zenoh`,
  `keyring`, and `age`.

**Why JCS over deterministic CBOR (`ciborium` + explicit ordering).** `ciborium`
`0.2.2` (`Apache-2.0`, published 2024-01-24, MSRV `1.58`, repository
`https://github.com/enarx/ciborium`, per
https://crates.io/api/v1/crates/ciborium, retrieved 2026-09-17) is a viable
alternative, but OAC's envelope is already JSON on the wire (DESIGN.md's "Preliminary
message envelope," `docs/planning/DESIGN.md` lines 72-92) — JCS canonicalizes the exact
representation already transmitted, so no second serialization format needs to exist
alongside the wire format. Deterministic CBOR would require either (a) transmitting CBOR
on the wire instead of JSON (a larger change than this decision's scope), or (b)
maintaining a CBOR-shaped canonical form that is never actually transmitted, purely for
signing — extra surface for no offsetting benefit given RFC 8785 already specifies a
byte-exact canonical form for the JSON OAC already sends. JCS is the smaller design,
matching the same "smallest design that proves the criterion" preference
`docs/planning/decisions/C2-process-model.md` §8 already applies elsewhere in this
project.

**Domain separation.** The signed bytes are **not** `serde_jcs::to_vec` applied to the
raw envelope alone. They are:

```
domain_separated_bytes = b"oac-envelope-v1" || 0x00 || jcs_bytes
```

where `jcs_bytes = serde_jcs::to_vec(signed_field_set)` (§5) and the fixed context
string `oac-envelope-v1` encodes both a fixed OAC-specific context tag and the envelope
version (`"version": "0.1"` in the envelope today, per DESIGN.md line 75 — the domain
string's own trailing version segment is bumped only when the *signed field set itself*
changes shape, not on every envelope schema tweak, exactly the way a protocol version
number is bumped for wire-breaking changes elsewhere in OAC). A single `0x00` byte
separates the fixed prefix from the JCS-canonicalized payload so no valid JCS output
(which per RFC 8785 is always valid UTF-8 JSON text, and JSON text cannot itself contain
a raw `0x00` byte outside an escaped form) can be crafted to make the prefix and payload
boundary ambiguous.

This is the mechanism that makes the rule in the task breakdown concrete: **a signature
over an envelope can never verify as a signature over a registration record
(`docs/planning/decisions/C4-session-identity.md` §5) or a pairing blob (§10 below)**,
because each of those structures is signed under its own distinct domain-separation
prefix (`oac-registration-v1`, `oac-pairing-v1` respectively — named here so a future
implementation does not need to invent them, though defining the registration record's
own signature scope in full is C4's job, not re-derived here; C4 §5 already states the
device key signs the registration record, this decision only fixes that its domain
prefix must differ from the envelope's).

## 4. Per-device signing keys

**One Ed25519 keypair per device**, generated by the daemon (the single process C2
already assigns key material to: `docs/planning/decisions/C2-process-model.md` §1,
"Device identity and key material — the per-device signing key, read from and written
to the OS credential store via `keyring` `4.2.0`"). This decision does not redesign key
storage: the private key half lives in the OS credential store, `keyring` `4.2.0`, with
the `age` `0.12.1` encrypted-file fallback, exactly as `docs/planning/decisions/
C4-session-identity.md` §10-§11 already fix, reused verbatim here. The only change this
decision makes to that stored secret's *shape* is that it is now specifically an Ed25519
signing key (§2) rather than an unspecified "signing key" — C4 §1a already named the
device key as the identity root without fixing its algorithm; this decision fixes the
algorithm.

**Public half is the device identity.** The Ed25519 public key is the device identity
in the `Security principal -> Device -> Harness -> Session` chain
(`docs/planning/ADR-001.md`, "Security model", cited already in C4 §1). It is what a
peer's `security.key_id` (§5) and a verifier's stored public key set resolve to.

**Sessions do not hold their own keys — stated explicitly.** A session's authenticity
derives entirely from the device key signing its registration record
(`docs/planning/decisions/C4-session-identity.md` §5). No OAC session, Claude or Codex,
generates, holds, or uses its own Ed25519 keypair. This is the direct restatement of C2
§1 leg 3 ("key material belongs in one process") one level up the identity hierarchy:
just as the daemon is the one process holding the device key, the device key is the one
signer for every session that device registers.

**Key rotation and expiry, v0.1 posture — stated, not left open.** **No automatic
rotation or expiry in v0.1.** A device's Ed25519 keypair, once generated, is used for
the life of that device's OAC installation. Rotation is **manual re-pair**: an operator
who suspects key compromise, or who wants to rotate on a schedule, deletes the stored
key (forcing a fresh keypair on next `oac start`, per `docs/planning/decisions/
C2-process-model.md` §7's "created automatically on first `oac start`") and every peer
that had paired with the old device-key fingerprint (§10) must re-pair against the new
one — there is no online rotation protocol, no key-rollover message type, and no grace
period during which both an old and a new device key are simultaneously valid for the
same device identity. This is a deliberate v0.1 scope limit, not an oversight: building
an online rotation protocol is exactly the kind of standing-infrastructure feature
`docs/planning/ADR-001.md`'s v0.1 scope defers (full E2E encryption and durable
federation-adjacent machinery are deferred for the same reason — see §14).

## 5. Signed field set

The DESIGN envelope (`docs/planning/DESIGN.md` lines 72-92):

```json
{
  "version": "0.1",
  "id": "unique-message-id",
  "from": "session-address",
  "to": "session-address",
  "conversation_id": "optional-thread",
  "reply_to": "optional-message-id",
  "correlation_id": "optional-correlation-id",
  "created_at": "timestamp",
  "ttl_ms": 300000,
  "content": [{"type":"text","text":"Please review scheduler.rs"}],
  "security": {
    "principal": "authenticated-principal-reference",
    "signature": "implementation-defined"
  }
}
```

**Signed field set table — the exact acceptance criterion "signed field set enumerated
exactly":**

| Field | Signed? | Reason |
|---|---|---|
| `version` | Yes | Prevents a downgrade attack that replays an old-version envelope shape under a new signature scope; the domain-separation prefix (§3) already pins the *signature scheme's* version, this field pins the *envelope's* version, and both must agree. |
| `id` | Yes | The message identity itself; an attacker who could mutate `id` post-signature could break correlation (`reply_to`, `correlation_id` below) without invalidating the signature. |
| `from` | Yes | The claimed sender address; this is exactly what a forged-sender / impersonation attack (§13) targets, and it must be inside the signed scope or the signature proves nothing about who sent the message. |
| `to` | Yes | The addressed destination; an unsigned `to` would let an intermediary re-route a validly-signed envelope to a different session without detection (an unauthorized-routing attack, §13). |
| `conversation_id` | Yes | Part of the message's addressing/threading context; mutable-in-flight would let an attacker splice a message into a different conversation thread than the sender intended. |
| `reply_to` | Yes | Correlation target; unsigned, an attacker could redirect what a reply is "to" without the sender's knowledge. |
| `correlation_id` | Yes | Same reasoning as `reply_to` — a correlation field left outside the signature is a splicing vector. |
| `created_at` | Yes | This is the field the replay-window check (§7) reads; if it were unsigned, an attacker could strip the timestamp check by rewriting it on a captured envelope and replaying it outside the original window. |
| `ttl_ms` | Yes | Governs how long the envelope is considered valid; unsigned, an attacker could extend or shrink the sender's intended validity window. |
| `content` | Yes | The message payload itself — the one field tampering (§13) most directly targets; excluding it from the signature would make the signature authenticate only the envelope's metadata, not its substance. |
| `security.principal` | Yes | The authenticated-principal reference (`docs/planning/ADR-001.md`'s `SecurityPrincipal` type, `docs/planning/DESIGN.md` line 28) this envelope claims to be from; must be inside the signed scope for the same impersonation reason as `from`. |
| `security.nonce` (new field, §7) | Yes | The replay-defence nonce; if unsigned, an attacker could strip or substitute the nonce and defeat the dedup key (§7) while leaving the rest of the envelope's signature intact. |
| `security.key_id` (new field, §7) | Yes | Identifies which of a device's keys (see §15's rotation-adjacent note — v0.1 has exactly one key per device, but the field is still named and signed so a future rotation scheme is not a wire-breaking change) signed the envelope; unsigned, an attacker could claim a different `key_id` than the one that actually signed, confusing verification bookkeeping even if the cryptographic check itself still passes against the correct key. |
| `security.signature` | **No — excluded by construction** | The signature cannot sign itself; this field holds the Ed25519 signature bytes (§2) over the domain-separated JCS canonicalization (§3) of every other field above. |

**Rule for unknown/future fields.** Because the signed bytes are the JCS
canonicalization (§3) of the entire signed field set as a JSON object, and JCS
canonicalizes *whatever object is passed to it* — not a fixed allowlist of keys chosen
at verification time — **any field present in the signed object at signing time is
inside the signed scope, including a field a future envelope version adds that this
document does not yet name.** Concretely: the signing code constructs the object to be
canonicalized as "the envelope with `security.signature` removed" (not as "these twelve
named fields"), so an intermediary cannot strip a field, cannot add a field, and cannot
reorder fields without invalidating the signature — reordering has no effect under JCS
canonicalization (RFC 8785's whole purpose), but addition or removal changes the
canonicalized bytes and therefore the signature no longer verifies. This is the concrete
mechanism behind the task instruction's "an intermediary cannot strip or add one."

**Transport-supplied values are out of scope, stated per boundary 5.** No transport-
supplied value — no Zenoh key expression, no Zenoh `zid` — is ever part of the signed
field set, ever appears in the envelope JSON above, or is ever consulted by signature
verification. Quoted: `[ADR-001 Boundary]` "MUST NOT leak Zenoh-specific concepts into
the neutral protocol," and `oac-zenoh` §8's containment rule: "Zenoh types must not
escape the transport module." The envelope's `from`/`to` fields carry OAC opaque session
ids (`docs/planning/decisions/C4-session-identity.md` §2), never a Zenoh key expression
or `zid` — the transport module maps between the two internally and that mapping never
runs in reverse onto the signed wire form.

## 6. Signature is normative — resolves C4

**Replacement text for `docs/planning/DESIGN.md` lines 85-90** (the `security` block),
to replace the current:

```json
  "security": {
    "principal": "authenticated-principal-reference",
    "signature": "implementation-defined"
  }
```

with:

```json
  "security": {
    "principal": "authenticated-principal-reference",
    "key_id": "device-public-key-fingerprint",
    "nonce": "128-bit-csprng-value",
    "signature": "ed25519-signature-over-domain-separated-jcs-canonicalization"
  }
```

and DESIGN.md's line 99 sentence ("Trusted security metadata must be distinguishable
from user/model-controlled content") is unchanged — this decision does not touch it,
only the `security` block's shape and the `signature` field's prior placeholder value.

**Where the normative MUST lands.** Per `docs/planning/ADR-001-AMENDMENTS.md`'s C4 row
("normative MUST lands in a future `spec/security.md` (Epic E)"), the actual
normative-language requirement ("implementations MUST verify `security.signature` using
`verify_strict` over the domain-separated JCS canonicalization of the signed field set,
and MUST reject an envelope whose signature does not verify") is written in
`spec/security.md` when Epic E (spec authoring, `oac-spec-authoring`) produces that file
— it does not exist yet, so this decision states the rule in decision-document prose
now and forecasts the normative landing site, exactly as `docs/planning/
ADR-001-AMENDMENTS.md`'s C4 row already forecasts.

**Does this need a new numbered `ADR-001-A4` amendment, or does it resolve inside the
decision like C8 did? Decided: resolves inside the decision, `RESOLVED-IN-DECISION`, no
new numbered amendment.** Checked directly: `docs/planning/ADR-001.md` contains no
occurrence of the string `implementation-defined` and no occurrence of the string
`signature` at all (confirmed by a direct grep of the file during this decision's
drafting). The placeholder text C4's conflict-register row names (`security.signature`:
`"implementation-defined"`) lives entirely in `docs/planning/DESIGN.md`, not in
`docs/planning/ADR-001.md` — the same shape `docs/planning/decisions/
C4-session-identity.md` §8 found for conflict C8 ("no `ADR-001.md` text needs
correction, so no new numbered amendment is issued"). Applying that same test here: this
document is the resolution; `DESIGN.md`'s follow-up edit (this section's replacement
text above) is the cross-file update (§18), not a numbered ADR-001 amendment. The next
free amendment number, checked against every number in `docs/planning/
ADR-001-AMENDMENTS.md` (`A1`-`A3` issued) and PLANNING-PROMPT.md Appendix A (which
earmarks only `A1`-`A3` by name), would be `ADR-001-A4` if one were needed — it is not,
for the reason above, and is not issued by this document.

## 7. Timestamp and nonce replay window

**Accept-window on `created_at`: ±300 seconds (5 minutes) of the verifying daemon's own
clock.** Justified against the envelope's existing `ttl_ms: 300000` default
(`docs/planning/DESIGN.md` line 83) — 300000 ms is exactly 300 seconds, so the replay
accept-window is set equal to the envelope's own default TTL: an envelope outside its
own stated validity window is, by the envelope's own semantics, no longer something the
sender considers current, and accepting it past that point would mean OAC enforcing a
weaker freshness bar than the message's own author declared. A message with a
non-default `ttl_ms` still uses the fixed ±300s **clock-skew/replay** window for
signature-timestamp acceptance — `ttl_ms` and the replay-accept-window are two separate
checks: `ttl_ms` is the sender's own application-level expiry (checked at delivery time
against `created_at + ttl_ms`), while the replay-accept-window is OAC's own defence
against a captured-and-replayed envelope being accepted long after it was signed,
independent of what TTL the sender declared.

**Clock-skew allowance.** The ±300s window above already carries the clock-skew
allowance — it is not a separate, additional tolerance layered on top. Concretely: an
envelope is accepted if `created_at` is within `[now - 300s, now + 300s]` of the
verifying daemon's own clock, where the `+300s` half (an envelope claiming a timestamp
up to 5 minutes in the *future*) exists specifically to absorb clock skew between the
sending and receiving devices, not to extend the sender's own intended validity window.

**Outside the window: reject as `expired`.** An envelope whose `created_at` falls
outside the accept-window is rejected using DESIGN's existing `expired` delivery state
(`docs/planning/DESIGN.md` line 108's list: `accepted`, `rejected`, `unreachable`,
`expired`, `duplicate`, `failed`) — no new delivery state is invented for this case.

**Nonce.** Size: **128 bits (16 bytes) from the OS CSPRNG**, matching
`docs/planning/decisions/C4-session-identity.md` §2's session-id construction exactly
("128 bits (16 bytes) from the OS CSPRNG") — the same primitive class, reused for the
same reason (unguessable, uncorrelatable). Encoding on the wire: raw bytes, base64 or
hex at the implementation's discretion (not fixed by this decision, since — unlike the
session id, §2 of C4 — the nonce is never displayed to a human and has no readability
requirement). Per-envelope uniqueness: a fresh nonce is generated for every signed
envelope; nonces are never reused across envelopes, even retries of "the same" logical
message — a retry is a new envelope with a new `id` and a new `nonce`.

**The dedup key is `(security.key_id, security.nonce)`, not `id` alone.** Stated
explicitly per the task instruction: an envelope's `id` (DESIGN.md line 75) is chosen by
the sender and this decision does not require it to be globally unique across all
senders — only the `(key_id, nonce)` pair, both CSPRNG-random and both inside the signed
field set (§5), is guaranteed collision-resistant enough to serve as the dedup key. Two
envelopes with the same `id` but different `(key_id, nonce)` pairs (for example, a
sender that reuses `id` values by mistake) are treated as **distinct** envelopes by the
duplicate-suppression store (§8); two envelopes with the same `(key_id, nonce)` pair are
the same envelope, regardless of `id`, and the second is suppressed and reported via the
existing `duplicate` delivery state (DESIGN.md line 108).

## 8. Duplicate-suppression store

**Location: the daemon (`docs/planning/decisions/C2-process-model.md` §1's "Policy and
allowlists" role), one store per device.** Not per session — a single device-scoped
store deduplicates across every session the device's daemon serves, the same
single-owner-per-device pattern C2 §1 already applies to key material and policy.

**In-memory, not on-disk.** The store holds `(key_id, nonce)` pairs (§7) seen within the
current replay-accept-window (§7) plus the clock-skew margin already folded into that
window. It is process memory, not a database file or durable log.

**Eviction: bounded by the replay window, not forever.** An entry is retained for
exactly the accept-window duration (±300s, §7) past its envelope's `created_at` — after
that point, the envelope itself would be rejected as `expired` on a fresh replay attempt
regardless of whether its `(key_id, nonce)` pair is still remembered, so remembering it
longer buys no additional protection. Concretely: entries are evicted once
`now > created_at + 300s` (the window's own upper bound), giving a maximum retention of
roughly 2×300s = 600s per entry in the worst case (an entry admitted at the earliest
allowed `created_at`, evicted at the latest point it could still matter) — a small,
bounded, self-limiting store, not a growing log.

**Memory bound under flood.** Because eviction is time-bounded (above), the store's
size is naturally bounded by "envelopes-per-device-within-a-10-minute-rolling-window ×
entry size" rather than by total message volume ever seen — a flood of envelopes does
not grow the store without bound the way an unbounded-retention design would; it only
grows to the flood's own rate times the fixed window, which caps at whatever the
device's actual inbound rate ceiling is (a further explicit rate-limit on inbound
envelope acceptance is a Stage 3/4 implementation detail this decision does not fix, but
the *dedup store's own* memory footprint is bounded by construction regardless of
whether such a rate limit is separately added).

**Behaviour on daemon restart: cold store, stated honestly.** The store is in-memory
(above), so a daemon restart empties it. **For the interval immediately following a
restart, up to the replay-accept-window's own duration (§7, ±300s from the daemon's
restart moment), the window itself — not the dedup store — is the only defence against
a replay of an envelope whose `created_at` still falls inside that window.** This is
stated plainly rather than glossed over: an attacker who captured a validly-signed
envelope and held it, then replayed it in the narrow gap between the old store being
lost and the new store re-observing it, would succeed if the envelope's `created_at` is
still within the accept-window at replay time. This is an accepted, named residual risk
(carried into §13's threat table), not a defence this design claims to close — closing
it fully would require either a durable dedup store (which §14 explicitly rejects, see
below) or a hard requirement that no valid, unexpired signed envelope can ever be
recaptured and replayed by an attacker in the first place (a stronger transport-security
property this decision does not assume).

**Exactly-once is not promised — stated per DESIGN's own "Delivery semantics."**
Quoted: `docs/planning/DESIGN.md` line 108, "Do not promise exactly-once delivery. Use
unique IDs, idempotency, and duplicate suppression." This decision's dedup store is
exactly the "duplicate suppression" DESIGN already names — it suppresses observed
duplicates and reports them via the existing `duplicate` state; it does not, and cannot,
guarantee a receiver never sees a duplicate under every failure mode (the cold-restart
gap above is the concrete example), which is precisely why DESIGN's own language is "do
not promise," not "guarantee."

**This is not a durable offline mailbox — boundary check.** Per `oac-boundaries`
boundary 11, "durable offline mailboxes... [are] explicitly deferred, not v0.1 work."
The dedup store described above holds no message content, is bounded to a ~10-minute
rolling window (above), and exists only to detect replay — it is not a queue that lets a
sender's message wait for an offline receiver to come back online, which is what the
deferred offline-mailbox boundary actually targets. `docs/planning/decisions/
C4-session-identity.md` §5's own "Lifetime" section draws the identical distinction for
the registration record ("durable, cross-restart persistence of registration records is
not built... a persisted registration record that survives daemon restart would be the
same kind of durable store"); this decision applies the same reasoning to the dedup
store and reaches the same conclusion: no durable persistence across restart (confirmed
by the cold-restart paragraph above).

## 9. `DeliveryReceipt` states — resolves C6

**DESIGN's list, quoted:** `accepted`, `rejected`, `unreachable`, `expired`,
`duplicate`, `failed` (`docs/planning/DESIGN.md` line 108).

**The knowable states this decision maps onto that list, at minimum:**

| State | Meaning | Maps onto DESIGN state |
|---|---|---|
| `accepted-by-adapter` | The OAC daemon/adapter accepted the envelope: signature verified (§2), replay window and dedup check passed (§7-§8), authorization check passed (§11) — it is now the adapter's job to hand it to the harness. | `accepted` |
| `handed-to-harness` | The adapter completed its provider-specific delivery call (Claude: a resolved `notifications/claude/channel` send; Codex: an app-server live-inject call the daemon's client issued) — **never asserted as "seen by the model."** | A refinement of `accepted`; DESIGN's bare `accepted` alone is ambiguous between "the adapter took it" and "the harness ingested it," so this decision names both explicitly rather than overloading one DESIGN state for two distinct facts. |
| `unknown` | The adapter cannot observe what happened past `handed-to-harness` — this is the honest terminal state for any provider that gives no delivery/read acknowledgement (Claude, per the row below). | Not a DESIGN state by that name; recorded as the receipt's own terminal status when neither `accepted`/`rejected`/`expired`/`duplicate`/`failed` applies and no further observation is possible. |

Plus DESIGN's own `rejected` (signature/replay/authorization failure — see §7's
`expired` mapping and §11's default-deny rejection), `expired` (§7), `duplicate` (§8),
`unreachable` (the addressed session is not currently registered/reachable, per
`docs/planning/decisions/C4-session-identity.md` §5's registration-record lifetime), and
`failed` (a transport or adapter-internal error unrelated to the envelope's own
validity) are used as DESIGN already defines them — this decision does not redefine
them, only adds the `handed-to-harness`/`unknown` refinement above `accepted` and states
the per-provider observability boundary below.

**No state may be reported that the adapter cannot actually observe — stated per
provider:**

- **Claude Code sends no acknowledgement.** Quoted, re-verified: "No acknowledgement of
  delivery" — HOLDS, per `docs/planning/REVERIFICATION-B2.md` §3.1 row of the same name,
  citing PLANNING-PROMPT.md §3.1. A resolved `notifications/claude/channel` send
  (verbatim method name, `docs/planning/PINS.md` — "Claude Code Channels") is therefore
  `handed-to-harness` and **never** "seen by the model" — the adapter's receipt state
  for Claude never progresses past `handed-to-harness` (or, if nothing further is ever
  observable for that session, terminates at `unknown`) because Claude Code's channels
  surface, as documented at the pinned version, gives the adapter no further signal.
  This is exactly the honesty rule PLANNING-PROMPT.md §3.1 states and
  `docs/planning/ADR-001-AMENDMENTS.md`'s C6 row already forecasts:
  "'accepted' — 'handed to harness'... a resolved Claude notification send means
  'written to transport', not 'seen by the model'."
- **Codex's observable point, per `oac-codex-appserver`.** The Codex app-server surface
  gives the daemon's own client (`docs/planning/decisions/C2-process-model.md` §1, "one
  client of the Codex app-server daemon") a JSON-RPC response/event for its live-inject
  call (`thread/queue/add`, `turn/steer`, or `turn/start`, per `docs/planning/PINS.md` —
  "Codex CLI and app-server," verbatim method names) — a successful RPC response is
  `handed-to-harness` in the same sense as the Claude case: it confirms the app-server
  accepted the call, not that the model has processed the resulting turn content. This
  decision does not claim a stronger observable point for Codex than the RPC
  response/event itself provides; the exact mapping of which Codex RPC outcomes map to
  which of this section's states in full protocol detail is `oac-codex-appserver`'s own
  surface-skill territory, not re-derived here — this decision fixes only the *naming
  and honesty rule* (three states, no overstatement), which applies identically to both
  providers.

**Flip C6 to resolved.** `docs/planning/ADR-001-AMENDMENTS.md`'s conflict-register row
C6 ("No Claude acknowledgement vs DESIGN `accepted` delivery state") moves from
`ASSIGNED` to `RESOLVED-IN-DECISION`, pointing at this section — the cross-file update
is recorded in §18.

## 10. Pairing model, two flows

### (a) Same-user multi-harness on one device — zero-config

Both `oac mcp-shim` processes (one spawned by Claude Code, one by Codex, per
`docs/planning/decisions/C2-process-model.md` §1) connect to the **same daemon** over
**authenticated local IPC** — Windows named-pipe security descriptors
(`SECURITY_ATTRIBUTES.lpSecurityDescriptor`, verbatim per C2 §4) or Unix socket
permissions plus `SO_PEERCRED`/`getpeereid()` (verbatim per C2 §4). **Why this is
zero-config, stated explicitly rather than asserted:** the OS itself already
authenticates the connecting peer process as belonging to the same local user account
before any OAC-level pairing step could even run (C2 §4's "Both peer checks assert peer
UID equals the daemon's own UID") — there is no separate identity for OAC to establish
between two harnesses on the same device, because both harnesses' shim processes are,
from the daemon's point of view, just "another process owned by the same OS user I
already trust to talk to me." The same device key (§4) already covers both harnesses'
sessions, since the device key is per-device, not per-harness (`docs/planning/decisions/
C4-session-identity.md` §1's identity hierarchy: device key sits one level above
harness). No pairing code, no exchanged secret, and no user-visible confirmation step is
needed for this flow — it falls entirely out of C2's local-IPC peer-authentication
design, applied here rather than re-derived.

**This is not a substitute for, and does not touch, the Claude-side consent step.**
`--dangerously-load-development-channels` (verbatim flag, `oac-security-work` §2: "the
only user consent step on the Claude side until OAC is on an allowlist; the plan must
not weaken it") is a separate, provider-side interactive confirmation that a Claude Code
user grants when loading a development channel at all — it happens before any OAC
pairing flow runs and gates whether Claude Code loads the channel surface in the first
place. This section's zero-config local-IPC peer authentication answers a different
question ("is this connecting process the same OS user as the daemon?"); it neither
stands in for nor pre-answers that flag's confirmation. Nothing in this decision
automates past, suppresses, or pre-answers `--dangerously-load-development-channels`.

### (b) Two devices on a LAN — short code, the chosen default

**Chosen: a short numeric code**, over the file-exchange alternative, because a spoken
or manually-typed code requires no filesystem access shared between the two devices (a
LAN pairing scenario by definition has two devices that may share nothing but network
reachability) and matches the "no manual certificate management" requirement DESIGN.md's
Security section already states for local/LAN mode ("LAN/remote mode requires
authenticated encryption and explicit pairing/trust establishment," `docs/planning/
DESIGN.md` line 115) — a short code read aloud or typed by the person pairing the two
devices is the lowest-friction "explicit" step available.

- **What the code authenticates.** Not the devices' identities directly — it
  authenticates an **exchange of device public keys** (§4's per-device Ed25519 public
  key). Each device computes a fingerprint of the other device's advertised public key
  (a truncated hash — SHA-256 truncated to a fixed length) and the short code is derived
  from, and must match, both devices' independently-computed fingerprints of each
  other's key.
- **Minimum floor on the truncation length — fixed here, not deferred.** The
  fingerprint carries three security-load-bearing roles: the pairing short code's
  binding and MITM defeat (this section), the Zenoh TLS/QUIC certificate common name
  (§12), and the envelope's `security.key_id` (§5-§6). The MITM argument below holds
  only if an attacker cannot find a second public key whose truncated fingerprint
  collides with the legitimate key's — a second-preimage/collision-resistance property
  that a short enough truncation would silently break. **The truncated fingerprint MUST
  be at least 128 bits (16 bytes) of the SHA-256 output** — the same 128-bit floor this
  decision already uses for the replay nonce (§7) as OAC's standing minimum for a
  security-load-bearing random or hash-derived value, chosen because 128 bits of a
  cryptographic hash keeps both preimage and collision resistance well beyond any
  practical attack budget. The exact truncation length *at or above* this floor (e.g.
  128 bits exactly vs. a longer value for display or protocol convenience) remains a
  Stage 3 implementation detail; the floor itself is not.
- **Length and entropy.** **6 decimal digits (000000-999999), ~19.9 bits of entropy**
  (`log2(1,000,000) ≈ 19.93`). Chosen as the same order of magnitude widely used for
  short-lived, rate-limited pairing/verification codes (the same shape as a TOTP code or
  a Bluetooth/Wi-Fi numeric-comparison pairing code) — short enough for a person to read
  or type without error, and short-lived plus rate-limited (below) so its low entropy on
  its own is not the thing doing the security work; the entropy that matters is the
  fingerprint of the full 256-bit-class Ed25519 public key (§2) each side is confirming,
  not the 6-digit code's own keyspace.
- **Time limit.** **120 seconds** from code generation. A code not confirmed by both
  sides within this window expires and a fresh pairing attempt must generate a new one
  — short enough to bound an offline brute-force window against the 6-digit code space,
  long enough for a person to read a code aloud or type it on the other device.
- **Rate limit on attempts.** **5 incorrect confirmation attempts per pairing session**,
  after which that pairing session is aborted and a fresh one (new code, new 120s
  window) must be started. Combined with the 120s time limit, this bounds a brute-force
  attacker to at most 5 guesses against a ~20-bit space before the code rotates —
  matching the same order-of-magnitude attempt-limiting practice short numeric pairing
  codes generally use, stated as OAC's own decision (not sourced from a third-party
  pairing protocol, since this is OAC's own pairing-flow design, not a claim about an
  external system requiring citation).
- **MITM defeat on the LAN.** The short code binds to the **exchanged key fingerprints**,
  not to the network path: each device displays (or otherwise communicates) the
  fingerprint-derived code for the *other* device's advertised public key, and pairing
  succeeds only if both devices' independently-computed codes match what the person
  confirms on each side. A network-level MITM sitting between the two devices during key
  exchange could substitute its own public key for one side's — but doing so changes
  that key's fingerprint, and therefore the derived code, which the person on the other
  device would see does not match what they expected to confirm. This is the standard
  numeric-comparison pairing defeat of a LAN MITM: the attacker can intercept the key
  exchange, but cannot make its substituted key produce the same fingerprint-derived
  code as the legitimate key without breaking the hash function the fingerprint uses.
- **Where LAN certificates come from, and the common-name link (forward reference to
  §12).** Where pairing issues LAN certificates for the Zenoh TLS/QUIC transport, the
  certificate's common name is **derived from the device public key fingerprint** —
  the same fingerprint the pairing code itself binds to, so the identity a peer's Zenoh
  ACL subject (§12) authenticates is traceable back to the same device key this
  decision's whole identity hierarchy is rooted in, not an independently-issued identity
  the pairing flow could get out of sync with.

**Pairing store's location.** The daemon, alongside policy (`docs/planning/decisions/
C2-process-model.md` §1's "Policy and allowlists" role) — the same single-owner-per-
device location as the duplicate-suppression store (§8) and the device key (§4). A
pairing record holds at minimum: the peer device's public key, its fingerprint, when
pairing completed, and (once F5 builds it) the allowlist entries that pairing seeds
(§11).

**This store is F5's input.** Per PLANNING-PROMPT.md §5 decision 6 and the task
breakdown's own framing, the pairing store's schema and lookup behaviour as fixed above
is the input F5 (the authorization-engine-and-pairing-store implementation task,
`docs/planning/backlog`) builds against — this decision does not implement F5, it fixes
what F5 must build to.

## 11. Default-deny and per-session sender allowlists

**Standing posture: every routing rule starts denied.** Quoted, `oac-security-work` §4:
"Default-deny is the standing posture... Any new routing rule, ACL subject, or
authorization path an agent adds must start denied and be opened explicitly." This
decision's allowlist, the Zenoh ACL mapping (§12), and the pairing-seeded grants (§10)
all start from nothing-is-permitted and are opened only by an explicit grant.

**Allowlist granularity: per OAC session id, scoped by `working_directory`.** An
allowlist entry names a specific addressed OAC session id (`docs/planning/decisions/
C4-session-identity.md` §2) as the granularity unit — not "any session on this paired
device," not "any session for this harness." Cross-project leakage (the
`oac-security-work` §2 checked item, and `docs/planning/decisions/
C4-session-identity.md` §5's own "Cross-project scoping" paragraph) is enforced the same
way C4 §5 already fixes it: a session's `working_directory` field, recorded at
registration, is what a discovery/allowlist check filters on before a grant applies — an
allowlist entry authored against a session in `/repo-a` does not implicitly authorize
delivery to, or discovery of, a session the same paired peer might separately hold in
`/repo-b`, unless a distinct grant names that session too.

**Where entries come from: pairing.** An allowlist entry is created as a direct
consequence of a completed pairing (§10) — same-device pairing (flow a) authorizes both
local harnesses' sessions against the single device key that already covers them;
cross-device LAN pairing (flow b) authorizes the specific peer device's key fingerprint,
and a further, separate grant (not automatic from pairing alone) names which of that
peer's sessions may reach which of the local device's sessions — pairing establishes
*that the two devices trust each other's key*, it does not, by itself, grant blanket
access from every session on one device to every session on the other; the per-session
allowlist entry (this section) is the separate, explicit authorization step pairing
feeds but does not substitute for.

**Eviction/revocation path.** An allowlist entry is removed (a) explicitly, by an
operator command (a future `oac` CLI verb this decision does not name, since C2 §6
already reserves room for "pairing-flow and one-shot-launch command semantics" under a
placeholder it deliberately does not fill in), or (b) implicitly, when the OAC session id
it names is deregistered (`docs/planning/decisions/C4-session-identity.md` §5's
"Lifetime" — the registration record does not outlive the session) — an allowlist entry
naming a session id that no longer has a live registration record is inert (matches
nothing) even if not yet explicitly deleted, and a fresh session (even one from the same
harness/working-directory) gets a **new** opaque session id (C4 §2/§6) that is **not**
automatically covered by a prior session's allowlist entry — re-authorization (a fresh
grant, from the still-valid pairing relationship) is required per new session, not
inherited automatically. Revoking a *pairing* (removing the peer device's key from the
pairing store, §10) cascades to remove every allowlist entry that pairing had seeded, so
a revoked peer loses access to every session it had been granted, not just future ones.

**Restating "authenticated but untrusted" — verbatim-in-substance, per the task
instruction.** Quoted, `oac-security-work` §3: "An authenticated peer message is still
an untrusted instruction and may carry prompt injection. Authentication answers 'who
sent it.' It never answers 'should this be obeyed.'" Applied here: a verified signature
(§2) plus a matched allowlist entry (this section) authorizes **delivery into the
addressed session's input** — nothing more. It never authorizes an **action** the
message's content asks for. Two concrete consequences, named explicitly because they are
the two live-code-execution-risk surfaces this project has:

- **`turn/steer` needs its own separate authorization decision.** Codex's `turn/steer`
  (verbatim method name, `docs/planning/PINS.md` — "Codex CLI and app-server") writes
  into an in-flight turn — PLANNING-PROMPT.md §7's own threat list names this
  explicitly: "Codex `turn/steer` writes into an in-flight turn; unauthorized steer is a
  code-execution risk." A message's signature verifying, and its sender being on the
  addressed session's allowlist, authorizes that message being delivered as *input* to
  the session — it does not, by itself, authorize an adapter to route that message's
  content into a `turn/steer` call rather than, say, queuing it as a new turn
  (`thread/queue/add`). Whether/when `turn/steer` is used at all, and what additional,
  separate authorization gate (if any) an OAC-initiated steer requires beyond ordinary
  message-delivery authorization, is deferred to `oac-security-work`'s own named owner
  for this check — task G7 (per `oac-security-work` §2's own citation: "`turn/steer` is
  either unused or gated behind an explicit authorization check") — not decided by this
  document, which only fixes that message-delivery authorization and steer-authorization
  are two separate checks, never one.
- **Claude permission relay stays off by default in v0.1 — restated, not re-decided
  here.** `claude/channel/permission` (verbatim capability key, `docs/planning/PINS.md`
  — "Claude Code Channels") lets an allowlisted sender approve tool use if enabled.
  `docs/planning/ADR-001-AMENDMENTS.md`'s C10 row already fixes the v0.1 default: "off
  by default in v0.1" — this decision does not reopen that default, it restates the
  reason in this document's own terms: a verified-and-allowlisted sender is authorized
  to have its *message* delivered into the session's input; permission-relay approval is
  a distinct *action* (approving a tool call) that message delivery alone must never be
  treated as implicitly authorizing, which is exactly why the default is off until a
  separate, explicit decision (owned by backlog task C6, per `docs/planning/
  ADR-001-AMENDMENTS.md`'s C10 row) turns it on for a given deployment.

## 12. Zenoh ACL subject mapping

**OAC policy maps only onto authenticated Zenoh ACL subjects — certificate common name
or username — never `zid`.** Quoted, `oac-zenoh` §5: "Map OAC policy onto authenticated
ACL subjects (certificate common name or username) only, per Decision 6 and 10." Quoted,
`oac-zenoh` §2 (constraint list): "`zid` is not an identity... `zid` subjects are
explicitly unauthenticated and unfit for production." PLANNING-PROMPT.md §3.4 states the
same constraint at the baseline level. This decision applies it: no OAC allowlist entry
(§11), pairing record (§10), or ACL rule is ever keyed on a Zenoh `zid`.

**The one-sentence relationship, stated so it cannot be misread:** **the Zenoh ACL is a
coarse pre-filter, the envelope signature is the authenticity proof, and Zenoh performs
no payload authentication of its own.** Quoted, `oac-zenoh` §6: "Zenoh provides no
application-layer message signing. Authenticity of an OAC envelope must be established
by OAC itself — the `security.signature` field in the envelope, not transport security,
is the authenticity proof even over TLS/QUIC." A Zenoh ACL rule permitting a given
certificate-common-name/username subject to publish/subscribe on a given key expression
stops obviously-unauthorized traffic from reaching OAC's own verification code at all
(the "coarse pre-filter" half) — it never substitutes for, and is never treated as
equivalent to, the envelope signature check (§2) that is the actual authenticity proof
for any envelope that does reach OAC's code.

**Where pairing issues LAN certificates, the common name is derived from the device
public key fingerprint.** Restated from §10's forward reference: a LAN-mode Zenoh
TLS/QUIC certificate's common-name field is set to the same device-public-key
fingerprint the pairing flow (§10) already computes and binds its short code to — this
keeps the Zenoh-layer authenticated identity traceable to the same device-key root every
other identity claim in this document (§1a's device key, §4, §10) is rooted in, instead
of the certificate issuance process inventing a second, independent identity namespace
that could drift out of sync with the pairing store (§10).

**Containment — the neutral envelope/protocol text stays free of Zenoh vocabulary.** Per
`oac-zenoh` §8's containment rule and `[ADR-001 Boundary]` "MUST NOT leak Zenoh-specific
concepts into the neutral protocol": the rule governs the neutral spec/core surface —
the envelope shape (§5-§6), the signed field set, the replay/receipt design (§7-§9), the
pairing flow (§10), and the allowlist model (§11) — none of which names `zid`, a key
expression, a liveliness term, or any other Zenoh-specific concept; each is written in
OAC's own neutral vocabulary (session id, device key, allowlist, pairing), confirmed by
re-reading each of those sections. It does not mean this planning document's own prose
may never use the word "Zenoh" — §2's license-election cross-reference, §13's transport-
layer threat row, and §14's rejected-alternatives discussion all name Zenoh directly, the
same way `docs/planning/decisions/C4-session-identity.md` §12 and
`docs/planning/decisions/C2-process-model.md` §9 both discuss Zenoh in their own prose
while keeping the *decision's substance* (ids, presence vocabulary, CLI surface) neutral.
This section, §12, is where the Zenoh-to-OAC-policy *mapping itself* is defined — the
piece of substance the containment rule would otherwise forbid from leaking into §5's
envelope shape or §11's allowlist model, and it does not.

## 13. Threat table

Per `oac-security-work` §1's template, all five columns, no blank "proving test."
C5-owned threats — session-identity-level threats (spoofing, cross-project discovery)
are already covered by `docs/planning/decisions/C4-session-identity.md` §13 and are not
duplicated here.

| Attack | Precondition | Mitigation | Proving test | Residual risk |
|---|---|---|---|---|
| Impersonation (forged envelope) | Attacker can construct an envelope claiming a `from`/`security.principal` it does not control | Ed25519 signature (§2) over the full signed field set (§5) including `from` and `security.principal`; `verify_strict` rejects malformed/small-order keys | F11 security suite; H2 | F11/H2 `NOT RUN` (`docs/planning/STATUS.md`); designed, not proven, mitigation |
| Tampering (field mutation in flight) | Attacker sits on the transport path (a compromised Zenoh peer, router, or intermediary) and can rewrite envelope bytes | Every signed field (§5) is inside the JCS-canonicalized, domain-separated signed scope (§3); any mutation invalidates the signature | F11; F4 | F11/F4 `NOT RUN`; a field this decision left unsigned (`security.signature` itself, by construction) is the only mutable-without-detection field, and that is expected, not a gap |
| Replay | Attacker captures a validly-signed envelope and re-sends it later | `created_at` accept-window (§7, ±300s) plus `(key_id, nonce)` dedup (§7-§8) | F4 | F4 `NOT RUN`; cold-restart gap (§8) is a named, accepted residual risk even once F4 passes — the window alone, not the store, covers the post-restart interval |
| Unauthorized routing/discovery | A peer not on a session's allowlist attempts delivery or discovery | Default-deny (§11); `working_directory`-scoped, per-session-id allowlist entries seeded only by completed pairing (§10) | F5; H2 | F5/H2 `NOT RUN`; the allowlist-check implementation itself does not exist yet |
| Unauthorized `turn/steer` (code-execution risk) | Attacker's message is delivered (passes §2/§11 checks) to a Codex session | Message-delivery authorization (§11) and steer-authorization are two separate checks by design (§11); `turn/steer` itself gated per G7's own criterion, not by delivery authorization alone | F11; G7 (per `oac-security-work` §2's citation) | F11/G7 `NOT RUN`; the separate steer-gate this table relies on is not built yet — until it is, delivery authorization alone is not sufficient to prevent an authorized-but-malicious sender from having its content routed into a steer call, which is exactly the gap G7 exists to close |
| Permission-relay abuse (`claude/channel/permission`) | Permission relay is enabled for a deployment | Off by default in v0.1 (§11, restating `docs/planning/ADR-001-AMENDMENTS.md` C10); enabling it is its own explicit decision, not a side effect of allowlisting | F11; H2 | F11/H2 `NOT RUN`; if a deployment does enable relay, any allowlisted sender for that session becomes able to approve tool use — this is the accepted-and-named risk the off-by-default posture exists to avoid by default, not eliminate for deployments that opt in |
| Malicious peer prompt injection despite valid signature | Attacker is a validly-paired, allowlisted peer whose message content itself is adversarial | None at the envelope-authenticity layer — by design, per the authenticated-but-untrusted doctrine (§11); mitigation lives at the harness/prompt-handling layer, out of this decision's scope | G5 (provenance rendering keeps the doctrine visible to the model); F11 | G5 `NOT RUN`; this is the doctrine's own stated limit, not a gap this document claims to close — quoted again for emphasis: authentication "never answers 'should this be obeyed'" |
| Compromised transport infrastructure (signature survives a hostile Zenoh path) | A Zenoh router or peer on the path is compromised or malicious | Envelope signature (§2-§3) is verified independent of transport state — per `oac-zenoh` §6, Zenoh performs no payload authentication, so OAC's own verification is unaffected by what happens to the envelope in transit, only by whether the bytes that arrive still verify | F11; D3/G3 (transport-layer robustness, out of this document's direct scope) | F11 `NOT RUN`; a compromised transport can still drop, delay (within the replay window), or duplicate envelopes even if it cannot forge or silently tamper with one — availability/DoS residual risk is not eliminated by signature verification, only forgery/undetected-tampering risk is |
| Cross-project disclosure | Two sessions exist under different `working_directory` values | `working_directory`-scoped allowlist (§11), reusing `docs/planning/decisions/C4-session-identity.md` §5's registration-record field | H2 (fourth acceptance item, per `oac-security-work` §2) | H2 `NOT RUN`; same open item C4 §13 already names for this exact threat |
| Leaked/exfiltrated device key | Attacker gains the device's private Ed25519 key (from the OS credential store or the `age`-encrypted fallback file) | `docs/planning/decisions/C4-session-identity.md` §11's own mitigation (encryption, permissions) applies unchanged, since this is the same key; no rotation-in-flight recovery exists in v0.1 (§4) — the only remediation is manual re-pair (§4) | F5; F11 | F5/F11 `NOT RUN`; v0.1 has no automatic rotation (§4), so a leaked key remains valid for every peer that has not yet manually re-paired until the operator notices and forces regeneration — this is a stated, accepted v0.1 scope limit, not an oversight |
| Local IPC peer spoofing | An attacker-controlled local process attempts to connect to the daemon's IPC endpoint pretending to be a legitimate shim | `docs/planning/decisions/C2-process-model.md` §4's OS-level peer authentication (named-pipe DACL + `GetNamedPipeClientProcessId` on Windows; `SO_PEERCRED`/`getpeereid()` on Unix), reused unchanged by this decision — the zero-config pairing flow (§10a) depends directly on this being sound | F11; G9 (per `oac-security-work` §2's citation) | F11/G9 `NOT RUN`; C2 §10's own UNVERIFIED item ("Named-pipe DACL peer-authentication behaviour not yet exercised on a live Windows host") is inherited unchanged here, since this decision does not re-verify it, only relies on it |

Every row names its proving test; none is marked mitigated without one, per
`oac-security-work` §1's rule. Because every named test's current verdict is `NOT RUN`
or the underlying task is not yet built (`docs/planning/STATUS.md`, Pre-Stage 0), every
row above describes a **designed** mitigation, matching the caveat stated at the top of
this document and the identical precedent `docs/planning/decisions/
C4-session-identity.md` §13 sets.

## 14. Rejected alternatives

- **Transport-TLS-only authenticity.** Rejected: Zenoh performs no application-layer
  payload signing (`oac-zenoh` §6) — TLS/QUIC secures the transport hop, not the
  envelope's own authenticity end to end through any intermediary, and does nothing
  once the envelope has left the wire onto the receiving daemon's own processing.
- **Zenoh `zid` as principal.** Rejected: `zid` is explicitly unauthenticated and unfit
  for production (`oac-zenoh` §2, §5) — using it as `SecurityPrincipal` would build
  OAC's entire identity hierarchy on a value the transport itself does not authenticate.
- **Per-session keys.** Rejected: key sprawl — C2 §1 leg 3 already puts key material in
  one process (the daemon) specifically to avoid duplicating it per session; per-session
  keys would reverse that decision one level down and multiply both the attack surface
  and the number of places a leak can originate, the same reasoning `docs/planning/
  decisions/C4-session-identity.md` §14's "Id derived deterministically from device key"
  rejection uses in the adjacent case.
- **HMAC with a shared pairing secret.** Rejected: no non-repudiation (any party holding
  the shared secret could have produced any signed message, so a signature proves only
  "someone with the secret," not "this specific device") and no multi-peer scaling (a
  shared secret pairwise-negotiated between every pair of devices does not compose the
  way a public-key scheme does — Ed25519's per-device keypair, verified against a
  publicly-shareable public key, lets any number of peers verify the same device's
  signatures without each needing a separate shared secret with it).
- **Full E2E encryption.** Rejected for v0.1: `docs/planning/ADR-001.md`'s v0.1 scope
  explicitly defers full E2E encryption (`oac-boundaries` 11 names the same deferral) —
  this decision's signature scheme provides authenticity and integrity, not
  confidentiality against a compromised transport path, which is the property E2E
  encryption would add and which v0.1 does not require.
- **Durable replay log surviving restart indefinitely.** Rejected: this is exactly the
  deferred offline-mailbox drift `oac-boundaries` boundary 11 and §8 above both name —
  an indefinitely-persisted dedup/replay log is a durable store with no bound tied to
  the replay window it exists to serve, and the same reasoning `docs/planning/decisions/
  C4-session-identity.md` §5 already applies to the registration record's own lifetime
  applies here without modification.

## 15. Reversal condition

Reverse the algorithm/crate choice (§2) if either of these concrete findings occurs:

- **An unfixable `ed25519-dalek` MSRV conflict with the pinned Rust `1.98.1`.** Test: at
  the time of any future re-pin of either `ed25519-dalek` or the Rust toolchain, does
  the pinned `ed25519-dalek` version's own `Cargo.toml` `rust-version` field remain
  `<= ` the pinned Rust toolchain version? At the versions cited in §2 today
  (`ed25519-dalek` `3.0.0`, `rust-version = "1.85"`, vs Rust `1.98.1`): **test passes.**
  If a future `ed25519-dalek` release raises its MSRV above whatever Rust version OAC
  has pinned, and OAC's own toolchain pin cannot be raised to match (for example because
  raising it would break `zenoh`'s own MSRV floor, per `docs/planning/PINS.md`'s
  Rust-toolchain constraint-floor entry), this half fires. Fallback if it fires:
  re-evaluate the rejected `ring`-based Ed25519 implementation (§2) against the MSRV
  conflict specifically, since `ring`'s own MSRV policy is independent of
  `curve25519-dalek`'s.
- **A Windows build failure in the static-binary target.** Test: does `ed25519-dalek`
  `3.0.0` (a pure-Rust crate with no documented C/assembly dependency in its own
  `Cargo.toml`, per the fetched file cited in §2) build and link cleanly for
  `x86_64-pc-windows-msvc`, the same target C1 §9's single-static-binary packaging
  targets? Not independently exercised by this document (UNVERIFIED — no live Windows
  build has been run against this pin; see §16). If a future build attempt fails on this
  target specifically, that is the fallback-triggering finding; the fallback is the same
  as above — `ring`'s Ed25519 implementation, which carries its own (heavier, per §2's
  rejection) but independently-proven Windows build story via its long-standing use in
  other Rust TLS stacks.

Neither half has fired as of this document.

## 16. Surface labels and UNVERIFIED ledger

Per `oac-evidence` §4, one label per surface touched by this document:

| Surface | Label | Note |
|---|---|---|
| `ed25519-dalek` | supported | general-purpose, actively maintained cryptography crate under `dalek-cryptography`; not a preview/experimental provider surface, per §2 |
| `serde_jcs` | supported | actively downloaded (1.67M+ downloads at retrieval), not yanked, implements a published standard (RFC 8785); not a preview/experimental provider surface, per §3 |
| Claude Code Channels (`notifications/claude/channel`, `claude/channel/permission`) | research preview | pinned `v2.1.274`, unchanged from `docs/planning/PINS.md`; referenced by §9 and §11 |
| Codex app-server (`thread/queue/add`, `turn/steer`, `turn/start`) | experimental (per-method gating via `capabilities.experimentalApi`) | pinned `@openai/codex@0.154.0` / commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`, unchanged from `docs/planning/PINS.md`; referenced by §9 and §11 |
| `keyring`, `age` | supported | unchanged from `docs/planning/decisions/C4-session-identity.md` §16; reused unedited by §4 |

**New UNVERIFIED items from this document**, added to `docs/planning/STATUS.md`'s "Open
UNVERIFIED items" list in the same change (`oac-evidence` §5 — never recorded in only
one place):

- Whether `ed25519-dalek` `3.0.0` builds and links cleanly on the
  `x86_64-pc-windows-msvc` target has not been independently exercised (UNVERIFIED — no
  live Windows build run against this pin; the crate is pure-Rust with no documented
  C/assembly dependency, which is favorable evidence, not a substitute for an actual
  build; see §15's second reversal-condition half).
- `dalek-cryptography/curve25519-dalek`'s repository-level MSRV *policy* (as distinct
  from the `ed25519-dalek` crate's own pinned `rust-version` field, which is confirmed —
  see §2) was not found stated on the repository's overview page as fetched (UNVERIFIED
  — the crate-level field is what this decision actually relies on; the policy statement
  would only matter for predicting *future* MSRV bumps, which §15 already treats as an
  open reversal condition regardless).
- The exact byte-truncation length for the device-key-fingerprint hash used in the LAN
  pairing flow (§10) and in Zenoh certificate common names (§12), above the 128-bit
  minimum floor §10(b) fixes, is not pinned to a single value by this document
  (UNVERIFIED — deliberately left as a Stage 3 implementation detail above the floor;
  the fingerprint mechanism's existence, role, and minimum collision-resistant length
  are fixed here, the exact length above that floor is not, matching the same
  granularity `docs/planning/decisions/C2-process-model.md` §7 already leaves the
  config-directory-resolution crate open at).
- Whether the 6-digit/120-second/5-attempt LAN pairing-code parameters (§10) hold up
  against a live implementation's actual network conditions (code-display latency,
  clock sync between the two devices for the 120s window) has not been exercised
  (UNVERIFIED — these are this decision's own design parameters, not a claim about an
  external system, so no first-party citation applies; runtime validation is a Stage 3/4
  implementation and testing task, `oac-implementation`/`oac-testing`, not this
  document).

No existing UNVERIFIED item this document relies on (Claude resume behaviour, Codex
daemon-attach runtime behaviour, named-pipe DACL live-host behaviour, etc.) is resolved
or reopened by this document — each is inherited unchanged from `docs/planning/
STATUS.md`'s existing list via the citations above.

## 17. Acceptance boxes, ticked against lines in this file

Per issue #18's seven acceptance boxes (backlog task C5,
`docs/planning/backlog/03-tasks-CD.json`):

- [x] Signature is normative, not 'implementation-defined' (resolves C4) — §6
      (replacement `security` block text, resolution status, C4 conflict-register
      disposition).
- [x] Signed field set enumerated exactly; algorithm and key format chosen — §5 (the
      field-by-field table), §2 (`ed25519-dalek` `3.0.0`, `verify_strict`, licensing),
      §3 (JCS canonicalization and domain separation).
- [x] Replay window, nonce handling, and duplicate-suppression store defined;
      exactly-once is not promised — §7 (±300s window, 128-bit nonce, `(key_id, nonce)`
      dedup key), §8 (in-memory per-device store, eviction, cold-restart honesty,
      exactly-once disclaimer quoting DESIGN.md line 108).
- [x] Receipt states distinguish 'accepted by adapter', 'handed to harness', and
      'unknown' honestly, given that Claude Code sends no acknowledgement (resolves C6)
      — §9 (the three-state table, per-provider observability, C6 conflict-register
      disposition).
- [x] Default deny; per-session sender allowlists — §11 (default-deny posture,
      per-OAC-session-id granularity, `working_directory` scoping, eviction/revocation).
- [x] Same-user multi-harness pairing on one device is zero-config; two devices on a LAN
      use a short code or file exchange — §10 (flow (a) zero-config via C2 local IPC,
      with the "why" stated explicitly; flow (b) short numeric code, chosen over file
      exchange, with length/time-limit/rate-limit/MITM-defeat all specified).
- [x] States how OAC policy maps onto Zenoh ACL subjects, given that only certificate
      common names and usernames are authenticated there — §12 (authenticated-subjects-
      only mapping, the one-sentence pre-filter-vs-authenticity-proof relationship,
      pairing-issued certificate common names).

## 18. Cross-file updates in this change

- `docs/planning/PINS.md`: new pin rows for `ed25519-dalek` `3.0.0` (BSD-3-Clause,
  flagged as its own license shape per §2) and `serde_jcs` `0.2.0` (MIT OR Apache-2.0,
  Apache-2.0 arm elected per §3), in the existing record shape (pin table row plus a
  `### <crate>` record section), each with `Gates affected: none directly
  (implementation dependency — see note)`, matching the shape `keyring`/`interprocess`/
  `age` already use.
- `docs/planning/ADR-001-AMENDMENTS.md`: conflict-register row **C4** moves from
  `ASSIGNED` to `RESOLVED-IN-DECISION`, resolution site `docs/planning/decisions/
  C5-envelope-auth.md` §6; conflict-register row **C6** moves from `ASSIGNED` to
  `RESOLVED-IN-DECISION`, resolution site `docs/planning/decisions/C5-envelope-auth.md`
  §9. No new `ADR-001-A4` amendment is issued, per §6's finding above ("no `ADR-001.md`
  text needs correction"). The conflict-register legend already defines
  `RESOLVED-IN-DECISION` (added by `docs/planning/decisions/C4-session-identity.md` §8);
  this file reuses that same status, its second use.
- `docs/planning/STATUS.md`: add a "C5 — envelope authenticity, replay defence,
  pairing, authorization (issue #18): decided" bullet under "Decisions landed"; bump
  "Last updated"; add the four new UNVERIFIED items from §16 to "Open UNVERIFIED items";
  add the two new pin rows (above) to the "## Pins" summary table.
- `docs/planning/DESIGN.md`: the `security` block replacement text from §6 (lines
  85-90) — tracked as this document's own direct follow-up edit, not deferred to a
  later Epic A task the way C1's/C4's naming-only DESIGN.md sites are (register entry
  C12) — because unlike C12's pure renames, this is the actual conflict C4 names
  (`security.signature: "implementation-defined"`) and leaving it unedited would mean
  the repository's own DESIGN.md still contradicted a decision this document just made.
- Forward pointer: the normative MUST text (§6) lands in `spec/security.md` once Epic E
  (`oac-spec-authoring`) writes it; this decision's prose is what Epic E's author reads
  as the source for that normative text. `docs/planning/v0.1/06-security.md` (Epic A
  task A7) is the other forward-pointed destination — the threat table (§13) and the
  pairing/authorization model (§10-§12) are exactly what that file's eventual authoring
  task will draw from, the same way `docs/planning/decisions/C4-session-identity.md`
  §18 forward-points at the same eventual output-package files.

## Where this folds in

Once `docs/planning/v0.1/03-decisions-and-amendments.md` (Epic A task A4) exists, this
file's content moves there unedited in substance (per PLANNING-PROMPT.md §9's output
package shape) and this file becomes a redirect stub, mirroring how
`docs/planning/ADR-001-AMENDMENTS.md` and `docs/planning/decisions/
C4-session-identity.md` already describe their own eventual fold-in. Consumed by: F4
(replay defence and duplicate-suppression implementation, §7-§8), F5 (authorization
engine and pairing store implementation, §10-§11), F11 (security test suite, exercising
every threat-table row in §13), G7 (`turn/steer` authorization gate, §11), G9 (local IPC
peer-auth verification, inherited unchanged from C2 §4 and relied on by §10a), H2
(spoofing/routing/cross-project verification, §13), G5 (the provenance gate this
document's whole signature-and-authorization design ultimately feeds), and Epic E's
`spec/security.md` (the normative-MUST landing site named in §6).
