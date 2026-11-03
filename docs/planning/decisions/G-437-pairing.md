# G-437 — The pairing verb and the on-disk trust store: S0 design note

- **Date:** 2026-11-03.
- **Issue:** #437 (split from G10, #71, by the lead's decision of 2026-10-18, Q6). Related:
  #409 (the pairing statements in core), #430 (confirming agreement values of an
  already-trusted key), #492 (the grant verbs, split out of #437 on 2026-11-03), #438
  (control-endpoint sizing), #70 (G9, key material stays in the daemon).
- **Source:** the lead's four decisions of 2026-11-03 (§1) and the defaults this note adopts
  (§2). Both are recorded as such below; nothing here is a lead decision except §1.
- **Status:** **Proposed (S0, design only).** No code, no `spec/` file and nothing under
  `tests/protocol/contract/` changes in the PR that adds this note. The slices in §10 are
  separate PRs. Slices S1b and S6 are lead merges; S5 needs the lead's approval of the
  `--json` shapes.
- **Owns:** its own text; the dated notes it adds to `C2-process-model.md` §6,
  `v0.1/08-cli-and-deployment.md` §5 and §12, `v0.1/11-risks.md` (`RISK-LOCAL-IPC`) and
  `STATUS.md`.
- **Skills:** `oac`, `oac-boundaries`, `oac-security-work`, `oac-planning-package`,
  `oac-evidence`.

Pairing makes a key trusted and **grants nothing** (`core/src/pairing.rs:13`; [SEC-AUZ-001]).
A device paired through this note receives nothing until a grant allows it; the grant verbs
are #492. `oac doctor` reports a pairing with no grant as WARN (§8).

---

## 1. The lead's decisions, 2026-11-03

1. **Carrier: the operator carries the exchange as pasted blobs.** Device A runs
   `oac pair` and prints an offer blob. Device B runs `oac pair --respond`; the operator
   pastes the offer, and B prints a response blob. The operator pastes the response into A;
   A prints a reveal blob and shows its six-digit code. The operator pastes the reveal into
   B; B shows its code. On each device the operator types the code shown on the **other**
   device (`PairingSession::confirm_entered`, five tries). **No spec change:**
   `spec/security.md` §5.3 fixes what pairing establishes, not its wire format, and the
   exchange already assumes a hostile channel (`core/src/pairing.rs` module documentation;
   C5 §10(b), dated note of 2026-10-06, F5-1).
2. **A durable agreement-key store is part of #437, as its own slice with a lead merge.**
   The private agreement keys go in an encrypted file next to the device key on every
   platform; the OS credential store stays at one entry (C4 §12; `cli/src/keystore.rs:12-14`).
   It is needed because `oac start` builds `MemoryAgreementKeyStore`
   (`cli/src/commands.rs:861`), so every restart issues `seq` 0 under a new key. That breaks
   [SEC-SEL-043], and peers that hold a higher `seq` refuse the new statement under
   [SEC-SEL-045]. How the file is encrypted on Windows is the design point §7 resolves.
3. **Control-endpoint authority.** The lead accepts, and this note documents (§9 and the
   `RISK-LOCAL-IPC` note), that with `pair` and `unpair` a same-user process, a prompt-injected
   model's tool shell included, can drive trust changes through the `control` endpoint. The
   mitigation is that a pairing completes only when the operator types the code the **other**
   device shows, which a process on one device cannot see. §9 adds the threat rows.
4. **Grant verbs are out of scope (#492).** `oac doctor` reports paired-with-no-grants as
   WARN.

## 2. Defaults this note adopts (open to the lead)

| # | Default | Where |
|---|---|---|
| A | No LAN certificate issuance in #437. The v0.1 binary has no LAN configuration (`PeerConfiguration::local()`, `cli/src/commands.rs:864`); the proof is two daemons on loopback | §10 |
| B | The binary always uses the durable store; the core stays store-agnostic | §5 |
| C | The store is one canonical (RFC 8785) JSON file, `{"oac_trust_store":1,...}`, bounded at 1 MiB, written temp file, `sync_all`, `rename`, directory `fsync` on Unix | §5 |
| D | A missing file is an empty store (default deny). A corrupt file or an unknown version stops the daemon from starting; the file is never reset | §5 |
| E | New control ops under `oac_control:1` (the version stays 1); a pairing is one long-lived control connection | §4 |
| F | Verbs `oac pair`, `oac pair --respond`, `oac unpair <key_id>`, `oac pair --list`, all with `--json`; exit codes in §6 | §6 |
| G | `AlreadyTrusted` is a distinct outcome that names #430; #430 is not implemented here | §6 |
| H | N5: an unpair whose save fails exits 1 with "revocation is NOT durable"; the daemon retries the save; `status` reports it and `doctor` FAILs | §6 |
| I | Core adds `Pipelines::pair` and `Pipelines::unpair` | §8 |
| J | Key material stays in the daemon; the `oac pair` client builds no `KeyStore`, and the initiator nonce never leaves the daemon before the response arrives | §3 |
| K | Names: `cli/src/trust_store.rs`, `cli/src/device_pairing.rs` (`core/src/issued_pairing.rs` is an unrelated "pairing": MCPB-ATT pairing windows for issued values) | §10 |
| L | Recommended, not decided: authenticate the trust store with a tag keyed from the device seed (§5.4, Q1) | §5.4 |

## 3. The exchange, step by step

`core/src/pairing.rs` already implements the protocol; #437 carries it. The shapes are
`PairingOffer` (L309), `PairingResponse` (L324), `PairingReveal` (L339), `PairingInitiator`
(L405), `ResponderPairing` (L698), `PairingSession` (L868) and `PairedPeer` (L1004). The
window is `PAIRING_VALIDITY_MS` = 120 000 (L129) and the tries are `PAIRING_MAX_ATTEMPTS` = 5
(L133), both from C5 §10(b).

```
Operator          Device A (initiator)                    Device B (responder)
   |  oac pair            |                                      |
   |--------------------->| PairingInitiator::start[_with_statement]
   |<-- offer blob -------|   (commitment to Ni; Ni stays in A's daemon)
   |  oac pair --respond  |                                      |
   |------------------------------------------------------------>| ResponderPairing::start
   |  paste offer ---------------------------------------------->| ::answer (first offer only)
   |<------------------------------------------------ response blob (Nr, statement)
   |  paste response ---->| PairingInitiator::receive           |
   |<-- reveal blob + code A (and both statement values) -------|
   |  paste reveal ------------------------------------------->| ::reveal (checks commitment)
   |<---------------------------------------------- code B (and both statement values)
   |  type code B on A -->| PairingSession::confirm_entered     |
   |  type code A on B -------------------------------------->| PairingSession::confirm_entered
   |<-- paired / outcome -|    Pipelines::pair                  |  Pipelines::pair
```

**Blobs.** `oacpair1.` followed by the unpadded base64url of one canonical JSON object
(`oac_core::canonical`). Members: `kind` (`offer`, `response` or `reveal`), then for an offer
`principal`, `public_key`, `commitment` and, when the device gives one, `statement` (the
agreement statement's own JSON, `AgreementStatement::as_json`); for a response `principal`,
`public_key`, `nonce`, `statement`; for a reveal `nonce`. A blob is public: it holds no
private key, and an attacker who alters one is caught by the code, not by the blob. The
`kind` member only lets the daemon say "that is a response, not an offer" instead of
parsing a wrong blob as the right one. An offer with a statement is about 0.8 KiB of text
(an estimate; S3 measures it), well inside the 4 KiB pairing-line limit (§4).

**What the daemon does with each step.**

- *Key material stays in the daemon (default J).* The client sends and receives blobs, codes
  and key ids; it builds no `KeyStore`. `Ni` is held inside the daemon's `PairingInitiator`
  and appears in no reply until the reveal, which is created only after the response has
  arrived; the offer reply carries the commitment, never `Ni`.
- *The statement is judged before the code is shown* ([SEC-SEL-051]): `receive` and `reveal`
  call `judge_with`, so a refused statement is shown as its own value
  (`StatementValue::Refused`) and the codes differ. The reply to `pair_response` and
  `pair_reveal` therefore carries `own_statement` and `peer_statement`, and the verb prints
  both beside the code ([SEC-SEL-044]).
- *N1 (#71, F5 review).* A pairing starts only when the operator runs the verb. A responder
  answers one offer; a second offer ends the pairing (`PairingEnd::SecondOffer`). A dropped
  connection is `ResponderPairing::abandon`. Every `PairingEnd` (`SecondOffer`, `Abandoned`,
  `Expired`, `OwnKey`, `CommitmentMismatch`) is returned to the verb and printed. The
  initiator sends one offer per `start`; a new offer needs a new `oac pair`.
- *Half pairings.* A and B confirm independently. If the operator types B's code on A
  correctly and A's code on B wrongly five times, A has trusted B and B has not trusted A.
  That is benign (a key trusted by one side, no grant), visible in `oac pair --list`, and
  repaired by `oac unpair` on A and a new pairing. The verb says so.

## 4. Control ops and the daemon state machine (`cli/src/device_pairing.rs`, S3)

The control endpoint today answers one request line and closes (`cli/src/control.rs`
module documentation): a line of at most `MAX_REQUEST` = 1024 bytes within `READ_DEADLINE`
= 2 s, at most `MAX_CONNECTIONS` = 4 connections. `stop` is the one op that holds a
connection. **A pairing is a second.**

| Op (all `{"oac_control":1,"op":...}`) | Connection | Reply |
|---|---|---|
| `pair` `{role:"initiator"}` | opens the pairing connection | `{"pair":"offer","blob":...,"expires_ms":...}` |
| `pair` `{role:"responder"}` | opens the pairing connection | `{"pair":"waiting","expires_ms":...}` |
| `pair_offer` `{blob}` | the pairing connection, responder only | `{"pair":"response","blob":...}` |
| `pair_response` `{blob}` | the pairing connection, initiator only | `{"pair":"code","blob":<reveal>,"code":...,"peer_key_id":...,"own_statement":...,"peer_statement":...,"already_trusted":<bool>}` |
| `pair_reveal` `{blob}` | the pairing connection, responder only | `{"pair":"code","code":...,"peer_key_id":...,"own_statement":...,"peer_statement":...,"already_trusted":<bool>}` |
| `pair_confirm` `{entered}` | the pairing connection | `{"pair":"mismatch","attempts_left":n}`, or `paired`, `already_trusted`, `not_saved`, `aborted`, `expired` |
| `unpair` `{key_id}` | its own short connection | `{"unpaired":true,"durable":<bool>}` or an error |
| `pairings` | its own short connection | the trusted keys with their confirmed values and `saved` |

- **Deadlines.** The 2 s `READ_DEADLINE` applies **only to the first line** of every
  connection. After a `pair`, each read is bounded by the time left of the 120 s pairing
  deadline. Pairing lines (after the first) may be 4 KiB; the first line, and every
  non-pairing op, keeps the 1 KiB limit. The daemon ends the pairing at the earlier of the
  tokio deadline and the core `Clock`'s 120 s (`deadline()` in `core/src/pairing.rs`), so
  both clocks fail closed.
- **One pairing per daemon.** A second `pair` while one is open is answered `busy` and
  closed. A pairing holds one of the four control permits for up to 120 s, so a same-user
  process can keep the operator's `oac pair` answered `busy` (availability only; the
  existing residual of the 4-slot endpoint, #438).
- **A disconnect is `abandon`.** The responder calls `ResponderPairing::abandon`; the
  initiator is dropped. Either way the pairing ends and the next one starts fresh.
- **The pairing lines carry no key material.** A blob is public; the code is shown to the
  operator and typed back (it is not a secret against the same user, §9). Log lines name the
  event only (`pairing started`, `pairing ended: <PairingEnd>`, `paired key <key_id>`), never
  a blob, nonce, code or statement, keeping the `LogSink` contract.
- **The first line says which role**, because the daemon has to choose `start` or
  `ResponderPairing::start` before any blob exists.

## 5. The durable trust store (S1) and the daemon that keeps it

### 5.1 What exists, and the gaps

`PairingStore` (core, L1181) is the seam; `PairingSnapshot` (L1147) is what it keeps: the
paired records with their confirmed values, the grants, the relay-enabled sessions, and the
held agreement statements. `AuthorizationEngine::restore` (`core/src/authorization.rs`
L1416) rebuilds an engine from a snapshot and refuses a duplicate key, an own key, a
statement without a key, and a statement below its key's confirmed floor. **Gaps:**

- `Daemon::start` restores from `Parts::pairing_store` and then drops it
  (`cli/src/daemon.rs:471`; the destructure at L439). Nothing can save a later change.
  `AgreementKeys::replace` and `erase_replaced` take the agreement store too; it is dropped
  after `load_or_generate` (L476). S1 and S1b make the daemon keep both for its lifetime.
- `oac start` passes `MemoryPairingStore` and `MemoryAgreementKeyStore`
  (`cli/src/commands.rs:860-861`). The G9 slice 6 note in `STATUS.md` records the pairing
  store as empty (default deny, "D8"): that is **G9's D8, "no durable PairingStore in G9"**
  (decision list of the G9 slice, recorded in the lead's hand-off notes, which are not on
  `main`), **not G-7's D8** (`G-7-stage4-dependencies.md` §8.3, who opens the Codex
  connection). #437 ends G9's D8.
- Every principal is `"device"` (`cli/src/commands.rs:862`). The verbs lead with the key id
  and treat the principal as a label.
- F5's note on #71 says the on-disk store is "opt-in, as F3's key stores are". Default B
  departs from that on purpose: the key stores are opt-in in their *tests* (the real
  credential store), but a trust store that is optional is a trust store that is silently
  empty after a restart. The file store runs in CI-default tests against a temporary
  directory.

### 5.2 The file

One canonical JSON object (`oac_core::json`, written with `oac_core::canonical`), at most
**1 MiB**:

```
{"grants":[...],"oac_trust_store":1,"paired":[{"confirmed":{"agreement_key":...,
 "issued_at":...,"seq":...},"key_id":...,"paired_at":...,"principal":...,
 "public_key":...}],"relay_enabled":[...],"statements":[{"agreement_key":...,
 "issued_at":...,"key_id":...,"seq":...}]}
```

Arrays are sorted (paired and statements by `key_id`, grants in the engine's order) so equal
snapshots give equal bytes. Parsing is strict: an unknown member, an unknown
`oac_trust_store` version, a `key_id` that is not the SHA-256 of `public_key`, or a key that
fails [SEC-KEY-034] refuses the file. Records are rebuilt with `PairingRecord::from_store`
and `with_confirmed`, held statements with `HeldStatement::from_store`; `restore` then applies
its own checks. The grant forms are serialized from `PeerSide` and `LocalSide`
(`core/src/authorization.rs`); S1 lists their members and round-trips every variant.

### 5.3 Where, who can read it, how it is written

- **Location.** Windows `%LOCALAPPDATA%\OAC\trust.json`. Unix `$XDG_DATA_HOME/oac/trust.json`,
  else `~/.oac/trust.json`: the directory the age key file already uses
  (`FileKeyStore::default_path`, `cli/src/keystore.rs:573`). A test namespace (`OAC_SCOPE`)
  uses `ns-<name>/` inside that directory, as the log does, so a live test never touches the
  user's store.
- **Private.** Unix: directory `0700`, file `0600`, owned by the user; a directory or file
  that is a link, not the user's, or open to others is refused, never repaired. Windows: a
  protected DACL with one ACE for the user. The helpers exist in `cli/src/logfile.rs` but sit
  in a private `mod os` (`make_private_directory`, `open_private_file`, `is_private`);
  S1 lifts them to a shared module rather than copy them.
- **Atomic.** Write a temporary file in the same directory (random name, created
  exclusively, as `FileKeyStore::save` does), `sync_all`, `rename` over the target, then
  `fsync` the directory on Unix. A failed write leaves the previous file whole.
  `std::fs::rename` replaces an existing target and, on Windows, "corresponds to"
  `MoveFileExW` with a fallback to `SetFileInformationByHandle`
  (https://doc.rust-lang.org/std/fs/fn.rename.html, retrieved 2026-11-03). **UNVERIFIED:**
  that a *completed* rename is durable across a power loss on Windows. The `MoveFileExW`
  page (https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw,
  retrieved 2026-11-03) describes `MOVEFILE_WRITE_THROUGH` only for a move done as copy and
  delete and says nothing of a same-volume replace, and no first-party statement was found
  for a directory flush. The residual: a power loss may bring back the previous file (an
  older store; for an unpair, the revoked device). S1 verifies this on Windows or leaves the
  label and the risk.
- **Missing, corrupt, unknown.** A missing file is the empty store. A file that cannot be
  parsed, exceeds 1 MiB, or has an unknown version makes `Daemon::start` fail with
  `StartError::Pairing` and a message naming the file; **it is never reset, repaired or
  renamed away.** The operator moves it aside by hand. `oac doctor` reports the same file
  without opening a daemon (§8).
- **The daemon keeps the store** (`Arc<dyn PairingStore + Send + Sync>`) and uses it for
  every engine call that takes a `&dyn PairingStore` (`pair`, `remove_key`,
  `admit_statement`, `add_grant` and the rest).

### 5.4 Finding for the lead: the trust file is the boundary that `pair` protects

With a durable file, a same-user process need not use the control endpoint to trust a key:
it can write `trust.json` (with a key and a grant) and wait for the next start. That skips
the code the operator types, which is the whole of [SEC-KEY-032]'s protection, and a
trusted key with a grant lets a remote device inject prompts (the doctrine of
`oac-security-work` §3). The lead's decision 3 accepts a same-user process driving trust
changes **through the endpoint**, where the code still stops it; the file edit is a path
that decision did not weigh.

**Recommended (default L, Q1):** authenticate the file. Add a `tag` member: an AEAD tag over
the canonical bytes of the rest of the object, keyed from the device seed by the same
derivation as §7 (an HPKE single-shot seal of an empty message with the bytes as associated
data gives a keyed tag with no new primitive). The daemon verifies it before it trusts the
file and refuses a mismatch like a corrupt file. It does not stop a process that can read
the seed (it can forge the tag; §9, T7), but it stops a writer that has the file and not the
seed. It makes the trust store depend on the device key, so a file written for another
device key is refused with a message that names both key ids (Q2). Cost: S1 depends on the
S1b derivation helper, so S1b merges first or the two are one stack. If the lead declines,
S1 ships without the tag and the row stays an accepted residual.

## 6. The verbs (S4)

All four take `--json`. They talk to the running daemon over the `control` endpoint; none
reads a key, opens a key store, or starts a daemon.

| Verb | What it does |
|---|---|
| `oac pair` | Initiator. Prints the offer blob; asks for the response blob; prints the reveal blob and its code with both statement values; asks for the code the other device shows |
| `oac pair --respond` | Responder. Asks for the offer blob; prints the response blob; asks for the reveal blob; prints its code and statement values; asks for the code the other device shows |
| `oac unpair <key_id>` | Removes a trusted key and, in the same step, every grant and binding that names it ([SEC-KEY-035]), its held statement and confirmed values ([SEC-SEL-047]). Asks for confirmation on a terminal; `--yes` skips it and is required without one. Takes the full 64-digit key id from `oac pair --list`; no prefix matching |
| `oac pair --list` | The trusted keys: key id, principal, `paired_at`, whether agreement values were confirmed, and whether the store is saved |

**Exit codes** (the set of `08-cli-and-deployment.md` §5): `0` paired (or unpaired, or
listed); `1` the pairing ended (`PairingEnd`), the code did not match or was refused, the
result was not saved, or the key is already trusted; `2` usage; `3` no daemon; `4` the
endpoint is another user's, busy (another pairing is open), or speaks another control
version.

**`AlreadyTrusted`** (`PairError::AlreadyTrusted`, `core/src/authorization.rs` L1217) is its
own outcome with its own message and `error:"already_trusted"`: "this key is already
trusted; confirming its agreement values without removing it is #430; to pair again now,
run `oac unpair <key_id>` (this deletes its grants and bindings)". It exits `1` because the
run did not do what a repeated pairing asks (new confirmed values). The reply to the step
that learns the peer's key already carries `already_trusted`, so the operator is told before
typing a code. **#430 is not implemented.** The final step is one function
(`finish(peer) -> Outcome`) so that #430 replaces only its `AlreadyTrusted` arm.

**N5** (#71, F5 review). `AuthorizationEngine::remove_key` returns
`RemoveKeyError::NotSaved` (L1190) when the removal stands in memory but the store still
holds the key. The verb must treat it as a failure: `oac unpair` exits `1` and prints
"removed in memory, but NOT saved: the revocation is NOT durable and a restart before it is
saved would restore this device". The daemon marks the store dirty and retries
`AuthorizationEngine::save` on a timer until it succeeds (any later successful save also
writes the whole snapshot). `status` reports the unsaved state; `oac doctor` FAILs it.
Residual: if the daemon ends before a save succeeds, the revocation is lost; the verb's
message says to check `oac pair --list` after a restart and to unpair again.

**`--json`.** Because a pairing is several steps, `--json` is a stream: one canonical object
per line on standard output per step (`"oac_cli":1,"step":...`), blobs and codes read as
lines on standard input, the last line the result (`"ok"`, `"paired"` or `"error"`). That
departs from "exactly one object" (`oac start --json`); the lead approves the shapes in S5
(Q6).

## 7. The durable agreement-key store (S1b; lead merge: the credential boundary)

**The need.** `AgreementKeys::load_or_generate` (`core/src/sealing.rs` L1135) issues `seq` 0
under a new key when the store is empty; with `MemoryAgreementKeyStore` that is every start.
[SEC-SEL-043] requires the highest issued `seq` to be kept "as durably as its device key",
and a consumer that holds `seq` ≥ 1 refuses the new statement ([SEC-SEL-045], [SEC-SEL-014]).
The state to keep is `AgreementKeyState` (L882): the private keys, each with its `seq` and
`issued_at`, and `highest_seq`. The private keys are the new secret on disk.

**Decision 2 fixes the shape**: an encrypted file next to the device key; the OS credential
store keeps its one entry. Unix: `device.key.age`'s directory. Windows: `%LOCALAPPDATA%\OAC\`,
there being no device-key file. Same privacy, atomic write and namespace rules as §5.3.

**The design point: how is it encrypted on Windows?** Facts, checked in this tree:

- `age` is a dependency of `oac-cli` on Unix only (`cli/Cargo.toml`,
  `[target.'cfg(unix)'.dependencies]`), with default features. Its key file is
  `age::scrypt` under a passphrase that a deployment provides
  (`PASSPHRASE_CREDENTIAL`, `PASSPHRASE_ENV`). **Windows has no age file and no passphrase**
  (`cli/src/keystore.rs`: "Windows has no fallback"). On a Unix host with a working keyring
  there is no passphrase either.
- `oac-core` depends directly on `hpke` 0.12.0 (features `alloc`, `x25519`), `sha2`,
  `x25519-dalek`, `getrandom` and `zeroize` (`core/Cargo.toml`). `hpke` carries
  HKDF-SHA256, ChaCha20-Poly1305 and `Kem::derive_keypair` (RFC 9180 `DeriveKeyPair`,
  `hpke-0.12.0/src/kem.rs:44`, from the crate's source repository, in the local registry,
  read 2026-11-03), and `single_shot_seal`/`single_shot_open` (`src/single_shot.rs`).
  `core/src/sealing.rs` already uses them (`Kem`, `Kdf`, `Aead` at L66-68).
- `Cargo.lock` has `hkdf` 0.12.4 (used by `hpke`, `age`, `age-core`) and `hkdf` 0.13.0
  (used by `secret-service`, a Unix keyring backend), `chacha20poly1305` 0.10.1, `aes-gcm`
  0.10.3, `scrypt` 0.11.0, `age` 0.12.1. **None of `hkdf`, `chacha20poly1305`, `hmac` is a
  direct dependency of `oac-core` or `oac-cli`**; on Windows `hkdf` 0.13.0 is not built at all.
- `DeviceKey` exposes no seed (`core/src/keys.rs`: `sign` is private); the seed is read
  through the `KeyStore` seam (`load() -> SecretSeed`), once, in `Daemon::start`.

| Option | New crate? | New feature? | Where it works | What it protects against | Verdict |
|---|---|---|---|---|---|
| **(i-a) HPKE self-seal keyed from the device seed.** Derive an X25519 storage key pair with `derive_keypair` over a domain-separated input (`"oac-agreement-store-v1"` + seed); `single_shot_seal` the canonical-JSON state to its own public key, `aad` = file header (magic, version, device key id) | No | No | Every platform, both device-key backends | A copy of the file alone (backup, sync, disk image) is useless without the device key; tampering is detected; when the device key is the age file, the passphrase protects both | **Recommended** |
| (i-b) `age` X25519 identity, held in memory only, derived from the seed | No | Yes on Windows (the `age` dependency becomes unconditional: `ml-kem`, `p256`, `i18n-embed`, `rust-embed` and the rest of `age`'s tree enter Windows builds; no new locked package, a new build surface) | Needs the `cfg(unix)` change in `cli/Cargo.toml` | As (i-a) | Rejected: the same protection for a larger Windows build and an extra dependency edge |
| (i-c) Symmetric key by HKDF over the seed, then `chacha20poly1305` | Direct dependency edges on `hkdf` and `chacha20poly1305` | No new package; new edges and `07` §5 rows | Everywhere | As (i-a) | Rejected: `hpke` already composes both and `sealing.rs` already uses it |
| (ii) The Unix age-scrypt passphrase path | No | No | Unix with a file-backed device key only | Strongest there | Rejected as the one mechanism: absent on Windows, and on Unix with a keyring the passphrase does not exist |
| (iii) Windows DPAPI (`CryptProtectData`) | No | Yes (`windows-sys` feature `Win32_Security_Cryptography`) | Windows only | Another user and offline theft; not a same-user process | Rejected: a second mechanism on Windows only, and a feature the lead asked to avoid |
| (iv) A second credential-store entry | No | No | Everywhere | As the device key | Excluded by decision 2 (one entry; C4 §12) |
| (v) Plaintext file, `0600` / user-only DACL | No | No | Everywhere | Other users only | Rejected: a leaked copy gives the private agreement keys, which open every payload sealed to this device |

**Recommendation: (i-a).** The derivation and the seal are about 80 lines in a new
`oac-core` module (pure functions, no I/O, `oac_core::at_rest`); `cli/` owns the file, the
paths and the permissions. Putting the crypto in `core` keeps `hpke` out of `cli/`'s
dependency list. If the lead prefers the helper in `cli/`, the alternative is a direct `hpke`
edge for `oac-cli` with the same version and features: no new package, but a new row in `07`
§5 and a `check-crate-deps.mjs` review. **No crate and no feature is added either way.**
S1b's estimate rises from 350 to about 450 lines with the helper in core.

What it does and does not buy, stated plainly: the file is as strong as the device key. It
inherits the key's residuals: a same-user process that can read the seed can read the file
(§9, T7), exactly as it can already sign as this device. It adds protection against the file
alone leaving the host or being altered. The seed is used only as input to a domain-separated
derivation inside the daemon; it is not disclosed ([SEC-KEY-004]).

**Plumbing.** The derivation needs the seed, which `Parts` hands only to `DeviceKey`. S1b
makes `Parts::agreement_store` a constructor that `Daemon::start` calls with the seed it reads
once through `Parts::key_store` (after the lock, before the identity is built), and keeps the
result for the daemon's life. Tests keep passing `MemoryAgreementKeyStore`.

**Failure modes (all refuse to start; none resets).**

- File missing: `None`, so `load_or_generate` generates, saves and **reads back** before use
  (`save_checked`). [SEC-SEL-043] holds because the new `seq` is durable before its
  statement exists.
- File present but its header names another device key id, or authentication fails, or the
  version is unknown: `StartError::Agreement` naming the file. Silently regenerating would
  issue `seq` 0 (peers refuse it); the lead decides whether a stale file may be moved aside by
  a verb (Q2). [SEC-SEL-052] allows a restart of the count only after a consumer has removed
  the key, which is not the daemon's call.
- Write fails: the statement is not issued.

## 8. Core, doctor and status (S2, S5)

**`Pipelines::pair` and `Pipelines::unpair` (S2).** Today the pairing engine calls are made
through `Pipelines::with_engine` (`core/src/pipeline.rs` L1414), which changes trust and
releases nothing. Presence of an own session is released to the trusted devices **only when
the session registers** (`inner.release_presence(&sid)` at L1315; `release_presence` at L2465)
and when a peer's statement is admitted (`Pipelines::admit_statement`, L868, which announces
every own session again to the device at L881). A session that registered before the
pairing is never announced to the new key. S2 adds:

- `Pipelines::pair(peer, store)`: `engine.pair`, then `announce_to(session, key, false)` for
  every own session, as `admit_statement` does;
- `Pipelines::unpair(key_id, store)`: forgets the announcement records that name the device
  and sends the withdrawals `unbind` would, best effort, **before** the removal (afterwards
  [SEC-AUZ-011] no longer lets one go to it), then `engine.remove_key`; the result says
  whether it is durable;
- the save-retry bookkeeping of §6 (a dirty flag, `retry_save`, and `trust_unsaved` in
  `CoreStatus`, `core/src/status.rs`).

Observation for the lead: release needs a grant (`AuthorizationRequest::ReleasePresence`,
`core/src/pipeline.rs` L2301-2308), and a grant that names a key cannot survive its removal
([SEC-KEY-035]). So until #492, `Pipelines::pair` releases nothing; it gives #492's
`add_grant` path and this path one release point, and S2 tests it with a grant loaded through
`with_engine`. Precedent says no spec bump: `admit_statement` made the same change to the
pipeline without one.

**Doctor and status (S5).** The `pairing` check of `08` §6 becomes: PASS when a trusted
device and a grant exist; **WARN "paired, but no grants: nothing is delivered (#492)"** when
a device is trusted and no grant exists; WARN "no pairings or grants" as today; **FAIL** when
the status reply reports the store unsaved (N5). A new `trust_store` check reads the file's
path, privacy and size by `symlink_metadata` only and reports a refused file; doctor opens
no key and parses nothing it cannot parse safely, so `scripts/check-doctor-boundary.mjs`
stays as is (to be confirmed in S5). `status` gains `trust_unsaved` in `counts`. The lead
approves the JSON shapes.

## 9. Threats (`oac-security-work` template)

**Reading this table.** No row is a closed mitigation until its slice lands: a row whose
"proving test" is a test named here but not yet written is an **open risk** (`oac-security-work`
§1). Rows that rest on tests that exist today say so. T1-T6 are about this feature; the
inventory coverage that follows names every other row of the `oac-security-work` §2
checklist and says why this feature does not change it.

| # | Attack | Precondition | Mitigation | Proving test | Residual risk |
|---|---|---|---|---|---|
| T1 | The channel the operator carries blobs over swaps a key (MITM) | The attacker sees or edits the pasted blobs | Commit-then-reveal; the code covers both principals, both keys, both statements and both nonces; the operator types the **other** device's code; five tries, 120 s ([SEC-KEY-032]; C5 §10(b), F5-1) | Existing: `tests/security/tests/keys_and_pairing.rs` `s13_pairing_mitm_substitution_is_caught_by_the_code`. Planned (S3): `a_substituted_key_in_the_pasted_blob_gives_codes_that_differ` | 10^-6 per operator-visible exchange |
| T2 | Grinding: restart exchanges until the responder's code matches the one the initiator shows (N1) | The attacker can send offers | One offer per operator-started pairing; a second offer, a disconnect, expiry, an own key or a bad reveal each end the pairing and are shown | Existing: `s13_second_offer_ends_the_pairing`. Planned (S3): `a_second_offer_ends_the_pairing_and_the_verb_shows_it`, `a_disconnect_abandons_the_pairing`, `the_pairing_expires_at_120_seconds`, `an_offer_carrying_this_devices_key_ends_the_pairing`, `a_tampered_reveal_ends_the_pairing` | An attacker who also blocks the operator's view of the end |
| T3 | Brute-force the six digits on a device | The attacker can type at the verb | Five entries, then `aborted`; the code is on the other device's screen | Existing: `s13_five_wrong_codes_abort_and_the_window_expires`. Planned (S3): `five_wrong_entries_abort_over_the_control_connection` | A same-user process can start a pairing and guess, five tries per pairing it opens; the operator sees the pairing and each `PairingEnd` only on the verb that is open (T5) |
| T4 | A statement is stripped from the exchange so an older one is accepted later ([SEC-SEL-044], [SEC-SEL-045]) | The attacker alters the pasted blobs | Statements enter the commitment and the code; the daemon judges a statement before showing the code ([SEC-SEL-051]); confirmed values are saved with the key ([SEC-SEL-046]) | Existing: `s13_a_statement_stripped_from_the_response_is_a_mismatch`. Planned (S3): `a_stripped_statement_gives_codes_that_differ`; (S1) `confirmed_values_survive_a_restart_and_an_older_statement_is_refused` | The operator who confirms without comparing the statement values the verb prints |
| T5 | A same-user process, a model's tool shell included, drives `pair`, `unpair` or `pairings` through the `control` endpoint | A process of the user can open the endpoint (owner and DACL check pass for it) | The operator must type the code the other device shows, which a process on one device cannot see; `unpair` removes trust and cannot add it; the daemon logs each event; the lead accepted the residual (decision 3) | Planned (S3): `unpair_removes_the_key_and_its_grants_over_the_control_endpoint`, `pairing_completes_only_on_the_entered_code` | **Accepted.** A same-user process can end trust (unpair), start and abandon pairings (a denial of service on the operator's pairing), and read `pairings`. It cannot add a trusted key without the code. It could already kill the daemon (`RISK-LOCAL-IPC`) |
| T6 | A lost revocation: `unpair` returns, the store did not save, a restart restores the revoked device (N5) | A disk error, a locked file, a scanner | The verb exits 1 and says so; the daemon retries; `status` reports it; `doctor` FAILs | Existing: `row08_a_failed_save_still_revokes`, `row08_removal_survives_a_restart`. Planned (S2/S3): `an_unpair_whose_save_fails_exits_1_and_is_retried`, `a_failed_pair_save_adds_no_key` | If the daemon ends before a save succeeds, the revocation is lost until the operator unpairs again |
| T7 | The trust file or the agreement file is edited or copied by a same-user process | A process of the user can write or read the data directory | Private modes and DACL, refused-not-repaired; the agreement file is encrypted under a key derived from the device seed (§7); the trust file's tag (§5.4, recommended) | Planned (S1/S1b): `a_loose_mode_is_refused_not_repaired`, `a_tampered_trust_file_is_refused`, `the_agreement_file_holds_no_plaintext_key`, `an_edited_agreement_file_fails_authentication` | A process that can read the seed (the OS credential store as the user) decrypts and forges both: the same residual as the device key. Without the tag (Q1) any same-user write to `trust.json` adds a key and a grant with no code |
| T8 | A crash or power loss leaves a half-written or older store | Power loss, a kill | Temp file, `sync_all`, `rename`, directory `fsync` (Unix); a failed write leaves the previous file | Planned (S1): `a_failed_write_leaves_the_previous_file_whole` | Windows durability of a completed rename is **UNVERIFIED** (§5.3): an older file may return |
| T9 | The restart problem: after a restart the device issues `seq` 0 under a new key | The agreement store is in memory | The durable store (§7) | Planned (S1b): `seq_and_key_survive_a_restart` ([SEC-SEL-043]) | A lost or foreign file (§7 failure modes) stops the start; recovery is by re-pairing ([SEC-SEL-052]) |

**Inventory coverage** (`oac-security-work` §2). Impersonation: T1, T4 and the signature
rows of `spec/security.md` §13, unchanged. Unauthorized routing and discovery: pairing grants
nothing ([SEC-AUZ-001]); a paired device receives nothing until #492 and doctor says so.
Tampering: T1, T4, T7. Replay: unchanged; T4 covers statement replay at pairing. Malicious
peer prompt injection: unchanged; pairing is not an instruction channel, and the doctrine
"authenticated but untrusted" holds: a paired key's messages are still untrusted instructions
(`oac-security-work` §3). Compromised transport infrastructure: the blobs never touch the
peer transport; the exchange is the operator's. Cross-project disclosure: unchanged;
`pairings` shows key ids and times, no working directory or session id. Leaked credentials:
T7, and the log carries no blob, nonce, code or key. Zenoh `zid` is not an identity: unchanged
(the `zid` plays no part here). Claude permission relay stays off by default; this note does
not touch it. `turn/steer` is untouched. The `--dangerously-load-development-channels`
confirmation is untouched. Provenance stays machine-set metadata; nothing here renders
provenance. Local IPC peer authentication (named-pipe security descriptors, Unix socket
permissions) is what admits the control connection; T5 states what that does and does not
stop.

## 10. Slices and acceptance tests

Each test uses in-process daemons over `oac-transport-memory` (`MemoryNetwork`,
`MemoryTransport`), an injected `Clock` (`oac_core::clock::ManualClock`) and tokio's paused
clock, **no sleeps and no polling** (`oac-testing`; `cli/tests/appearance_timer.rs` is the
pattern), loopback only: the IPC endpoints are the user's own, in a test namespace
(`Scope::for_user_in`), each daemon with a store in its own temporary directory. Tests that
need a real credential store stay opt-in (`#[ignore]`, `OAC_TEST_REAL_KEYRING=1`).

| Slice | What | ~Lines | Lead merge? | Acceptance tests (names are the plan) |
|---|---|---|---|---|
| **S1** | `cli/src/trust_store.rs`: the durable `PairingStore` (§5); the daemon keeps the store; the private-file helpers lifted out of `logfile.rs`; `oac start` passes the file store | 500 | no | `snapshot_round_trips_every_grant_and_record`; `a_missing_file_is_an_empty_store`; `a_corrupt_or_unknown_version_file_stops_the_daemon_and_is_not_reset`; `an_oversize_file_is_refused`; `a_loose_mode_is_refused_not_repaired`; `a_failed_write_leaves_the_previous_file_whole`; **SEC-SEL-046** `confirmed_values_survive_a_restart_and_an_older_statement_is_refused` (pair with a statement, stop the daemon, start another on the same file, `admit_statement` of a lower `seq` is `BelowConfirmed`); **SEC-SEL-047** `after_removal_and_restart_the_record_and_confirmed_values_are_gone` |
| **S1b** | The durable `AgreementKeyStore` (§7): the `oac_core::at_rest` helper, `cli/src/agreement_file.rs`, the `Parts` constructor, the daemon keeps the store | 350 (about 450 with the helper in core) | **yes**, credential boundary | **SEC-SEL-043** `seq_and_key_survive_a_restart` (same statement after restart; after a `replace`, the next start issues `seq` 1 not 0); `the_agreement_file_holds_no_plaintext_key`; `an_edited_agreement_file_fails_authentication`; `a_file_for_another_device_key_stops_the_start_and_is_not_reset`; `the_file_is_private_and_written_atomically`; no `hkdf`, `chacha20poly1305` or `hmac` direct dependency (`scripts/check-crate-deps.mjs`, `Cargo.lock` unchanged) |
| **S2** | Core `Pipelines::pair` / `unpair`: presence release and withdrawal, save-retry bookkeeping | 250 | no | `pair_releases_an_existing_sessions_presence_to_the_new_key` (grant loaded with `with_engine`); `unpair_withdraws_then_removes_and_drops_the_announcement_records`; `an_unpair_whose_save_fails_reports_not_durable_and_retry_clears_it`; `a_failed_pair_save_adds_no_key` |
| **S3** | `cli/src/device_pairing.rs`: the control ops and the state machine (§4) | 600 | no | The N1 cases: `a_second_offer_ends_the_pairing`; `a_disconnect_abandons_the_pairing`; `the_pairing_expires_at_120_seconds` (the 2 s deadline applies to the first line only); `an_offer_carrying_this_devices_key_ends_the_pairing`; `a_tampered_reveal_ends_the_pairing`; `five_wrong_entries_abort`; `a_second_pairing_is_busy`. `a_stripped_statement_gives_codes_that_differ`. `the_offer_reply_carries_no_Ni` (the reply holds the commitment; `Ni` appears only in the reveal, after the response). `two_daemons_pair_and_both_trust_each_other`; `AlreadyTrusted_is_its_own_outcome`; `an_unpair_whose_save_fails_exits_1_and_is_retried` (N5) |
| **S4** | The verbs (§6) | 550 | no | Each verb through its `_with` function with in-memory streams: exit codes `0/1/2/3/4`; `--json` shapes; `oac_pair_never_builds_a_key_store` (no `KeyStore` symbol in the client path: source check); a wrong blob kind is diagnosed without a state change |
| **S5** | Doctor and status changes; the `08` §5/§6, C2 §6, `11-risks` and STATUS documentation | 300 | lead approves the JSON shapes | `doctor` WARN/PASS/FAIL cases for `pairing` and `trust_store`; `status` `trust_unsaved`; documentation sweep of the `pairing verb (#437)` mentions |
| **S6** | `spec/security.md` editorial revision: the Appendix A cells for [SEC-KEY-032] (about L2250), [SEC-SEL-046] and [SEC-SEL-047] (about L2389-2390), and the stale §13 rows "Trust on first use" and "Leaked device key" (about L1576-1577) | n/a | **yes** | The cells name the S1/S3 tests; `node tests/protocol/runner/run.mjs` stays green; a minor revision under E7 §7 |

Order: S1b before S1 if the lead accepts the tag (§5.4); otherwise S1 and S1b are independent.
S2 before S3. S3 before S4. S5 after S4. S6 last, naming the tests that exist.

## 11. Open questions for the lead

1. **Q1: authenticate `trust.json`** with a tag keyed from the device seed (§5.4, default L:
   yes)? It makes S1 depend on S1b.
2. **Q2: a store written for another device key** (the device key was deleted and made again,
   or a file was restored): refuse to start and name both key ids (default), or give the
   operator a verb to move the stores aside? The default needs no new verb.
3. **Q3: is 120 s enough for six pastes and two typed codes?** The window is C5 §10(b)'s
   parameter and is per device (A's starts at `oac pair`, B's at `oac pair --respond`). The
   note keeps 120 s; changing it is a spec change.
4. **Q4: where the at-rest helper lives** (§7: `oac-core`, recommended, or a direct `hpke`
   edge in `oac-cli`).
5. **Q5: `AlreadyTrusted` exits `1`** (default G) rather than `0`?
6. **Q6: the `--json` stream** for the multi-step pairing verbs (§6).
7. **Q7: confirm the control endpoint's size.** A pairing holds one of four permits for up to
   120 s (§4). #438 sizes the endpoint; this note does not change the number.
8. **Q8: the Windows power-loss durability of the rename** (§5.3, UNVERIFIED): verify in S1 on
   Windows, or accept and keep the risk row.

## 12. Gaps recorded

- The daemon drops its pairing store after `restore` (`cli/src/daemon.rs:471`) and its
  agreement store after `load_or_generate`; S1 and S1b close both.
- Every principal is `"device"`; the UI leads with key ids.
- C5 §10 ("the allowlist entries that pairing seeds", L696-701) and §11, and
  `v0.1/06-security.md` L172-176, say pairing seeds allowlist entries; the core seeds none
  (`core/src/pairing.rs:13`). Resolved under #492, not here.
- `docs/planning/HANDOFF.md`, where the G9 D8 note lives, is not on `main` (checked 2026-11-03);
  this note cites `STATUS.md`'s G9 slice 6 entry, which carries "(default deny, D8)".
- `core/src/issued_pairing.rs` is an unrelated "pairing" (windows for issued values,
  [MCPB-ATT-005] to [MCPB-ATT-026]); the new modules are `trust_store` and `device_pairing`.
  The lead's brief placed it under `cli/src/`; it is in `core/src/`.
- The lead's brief cited `core/src/pipeline.rs` L1335-1348 for release at registration; in
  this tree registration releases at L1315 and `release_presence` is L2465-2477; L1335-1348 is
  `refresh_presence`.

## 13. Boundary pass

Nothing here calls a provider model API, routes by content, manages model context, reads or
reuses a harness's credential, scrapes a terminal, or uses a private RPC. The exchange is
between two OAC daemons and the operator. No peer-transport concept appears in the blobs,
the ops or the store. The credential boundary (C4 §12) holds: the new file holds OAC's own
agreement keys, under OAC's own device key; no harness file or entry is read. No durable
offline mailbox, group room or federation is introduced ([ADR-001] v0.1 scope; `oac-boundaries`
11): the store keeps trust, not messages.

## 14. Evidence

Verified in this tree on 2026-11-03: `core/src/pairing.rs` (L13, L129, L133, L309-L339,
L405, L698, L868, L963, L1004, L1080, L1147, L1181, L1200); `core/src/authorization.rs`
(`restore` L1416, `snapshot` L1458, `save` L1477, `pair` L1585, `admit_statement` L1635,
`remove_key` L1716, `RemoveKeyError` L1190, `PairError` L1217); `core/src/pipeline.rs`
(L868-L886, L1315, L1335-L1348, L1414, L2301-L2308, L2465-L2477); `core/src/sealing.rs`
(L882, L910, L1135); `core/src/clock.rs` (`ManualClock` L53); `cli/src/control.rs`;
`cli/src/daemon.rs` (L317, L439, L471-L476); `cli/src/commands.rs` (L860-L864);
`cli/src/keystore.rs` (L12-L14, L573); `cli/src/logfile.rs`; `cli/Cargo.toml`;
`core/Cargo.toml`; `Cargo.lock`; `spec/security.md` [SEC-KEY-032], [SEC-SEL-043] to
[SEC-SEL-052]. External: `std::fs::rename` and `MoveFileExW`, both retrieved 2026-11-03
(§5.3). `hpke` 0.12.0 source, from the local cargo registry, read 2026-11-03.

**UNVERIFIED (carried to `STATUS.md`):** that a completed `rename` over an existing file is
durable across a power loss on Windows, with or without a directory flush (§5.3).
