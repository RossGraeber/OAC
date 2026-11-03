# G-437 — The pairing verb and the on-disk trust store: S0 design note

- **Date:** 2026-11-03 (revised the same day after review of PR #493, with the lead's round 2
  decisions, §1).
- **Issue:** #437 (split from G10, #71, by the lead's decision of 2026-10-18, Q6). Related:
  #409 (the pairing statements in core), #430 (confirming agreement values of an
  already-trusted key), #492 (the grant verbs, split out of #437 on 2026-11-03), #438
  (control-endpoint sizing), #70 (G9, key material stays in the daemon).
- **Source:** the lead's decisions of 2026-11-03, round 1 (L1 to L4) and round 2 (L5 to L8,
  and the accepted defaults), recorded in §1; and the defaults this note adopts (§2).
  Nothing here is a lead decision except §1.
- **Status:** **Proposed (S0, design only).** No code, no `spec/` file and nothing under
  `tests/protocol/contract/` changes in the PR that adds this note. The slices in §10 are
  separate PRs. Slices S1b and S6 are lead merges; S5 needs the lead's approval of the
  `--json` shapes. It amends `C2-process-model.md` §4 (§4 below) with the lead's approval of
  2026-11-03 (L8).
- **Owns:** its own text; the dated notes it adds to `C2-process-model.md` §4 and §6,
  `C5-envelope-auth.md` §10(b), `v0.1/08-cli-and-deployment.md` §5 and §12,
  `v0.1/11-risks.md` (`RISK-LOCAL-IPC`) and `STATUS.md`.
- **Skills:** `oac`, `oac-boundaries`, `oac-security-work`, `oac-planning-package`,
  `oac-evidence`.

Pairing makes a key trusted and **grants nothing** (`core/src/pairing.rs:13`; [SEC-AUZ-001]).
A device paired through this note receives nothing until a grant allows it; the grant verbs
are #492. `oac doctor` warns about a paired key that no grant names (§8).

---

## 1. The lead's decisions, 2026-11-03

### Round 1

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
3. **Control-endpoint authority (wording corrected by L5).** *Round 1 said the typed code
   stops a same-user process driving a pairing. That is wrong, and the lead replaced it:*
   a same-user process, a prompt-injected model's tool shell for example, **can add a trusted
   key through the control endpoint with no operator.** It pairs this device with a second
   `oac` daemon (or any program that speaks the exchange, whose blobs are public) that it
   controls, for example on loopback under its own `OAC_SCOPE`; it sees and types **both**
   codes. `confirm_entered` only compares strings (`core/src/pairing.rs:963-981`). The typed
   code defends only against a man-in-the-middle between two **honest** devices. See L5.
4. **Grant verbs are out of scope (#492).** A pairing alone delivers nothing ([SEC-AUZ-001]).

### Round 2 (recorded as the lead's decisions of 2026-11-03)

- **L5 (supersedes the wording of decision 3).** As in item 3. **The lead accepts this as a
  residual on these corrected facts.** The mitigations are visibility only: a log line for
  every pairing and every unpair, `oac pair --list`, and doctor showing recent pairings
  (S5). A grant is still needed before anything is delivered (#492). §9 T3, T5 and the
  `RISK-LOCAL-IPC` note in `11-risks.md` are written accordingly.
- **L6.** No keyed tag on `trust.json`. A plain `device_key_id` member is added, for
  diagnosis only (§5.2). S1 does not depend on S1b.
- **L7.** The agreement-key file is named by the device key id (§7). A regenerated device
  key starts its own `seq` at 0: this is conformant, because [SEC-SEL-040] and
  [SEC-SEL-043] count what **this device key** issued ([SEC-KEY-002]; [SEC-SEL-043] keeps the
  count "as durably as its device key"; a peer that never paired the new key admits no
  statement for it, [SEC-SEL-049]). Files for other key ids are ignored and never deleted.
  The daemon refuses to start only when the **current** key id's file fails authentication.
  `trust.json` is not bound to the device key; doctor says peers must pair again after a
  regeneration.
- **L8.** The lead approves an explicit C2 §4 amendment: pairing ops get 4 KiB lines and a
  120 s per-pairing read deadline; the first line keeps the 2 s and 1 KiB bound. A dated note
  goes at C2 §4 (the `control` paragraph, L175), not only §6, and the `control.rs` invariant
  "nothing a peer writes to it reaches the core" (`cli/src/control.rs:11-12`) is reworded in
  S3 (§4).
- **Defaults accepted by the lead:** the at-rest helper lives in `oac-core` (§7); a repeated
  pairing of an already-trusted key exits 1 (§6); the `--json` per-step stream is approved in
  S5 (§6); the Windows rename durability is verified in S1 or kept as a risk (§5.3).
- **The 120 s window is kept for v0.1** (§3).

## 2. Defaults this note adopts (open to the lead)

| # | Default | Where |
|---|---|---|
| A | No LAN certificate issuance in #437. The v0.1 binary has no LAN configuration (`PeerConfiguration::local()`, `cli/src/commands.rs:864`); the proof is two daemons on loopback | §10 |
| B | The binary always uses the durable store; the core stays store-agnostic | §5 |
| C | The store is one canonical (RFC 8785) JSON file, `{"oac_trust_store":1,...}`, bounded at 1 MiB, written temp file, `sync_all`, `rename`, directory `fsync` on Unix | §5 |
| D | A missing file is an empty store (default deny). A corrupt file or an unknown version stops the daemon from starting; the file is never reset | §5 |
| E | New control ops under `oac_control:1` (the version stays 1); a pairing is one long-lived control connection | §4 |
| F | Verbs `oac pair`, `oac pair --respond`, `oac unpair <key_id>`, `oac pair --list`, all with `--json`; exit codes in §6 | §6 |
| G | `AlreadyTrusted` is a distinct outcome that names #430; #430 is not implemented here (lead-accepted: exit 1) | §6 |
| H | N5: an unpair whose save fails exits 1 with "revocation is NOT durable"; the daemon retries the save with a bounded backoff; `status` reports it and `doctor` FAILs | §6 |
| I | Core adds `Pipelines::pair` and `Pipelines::unpair` | §8 |
| J | Key material stays in the daemon; the `oac pair` client builds no `KeyStore`, and the initiator nonce never leaves the daemon before the response arrives | §3 |
| K | Names: `cli/src/trust_store.rs`, `cli/src/device_pairing.rs` (`core/src/issued_pairing.rs` is an unrelated "pairing": MCPB-ATT pairing windows for issued values) | §10 |
| L | The trust file carries a plain `device_key_id` for diagnosis and no tag (L6) | §5.2 |
| M | Pasted blobs may be wrapped over several lines; the client reads until an empty line | §3 |

## 3. The exchange, step by step

`core/src/pairing.rs` already implements the protocol; #437 carries it. The shapes are
`PairingOffer` (L309), `PairingResponse` (L324), `PairingReveal` (L339), `PairingInitiator`
(L405), `ResponderPairing` (L698), `PairingSession` (L868) and `PairedPeer` (L1004). The
window is `PAIRING_VALIDITY_MS` = 120 000 (L129) and the tries are `PAIRING_MAX_ATTEMPTS` = 5
(L133), both from C5 §10(b).

```
Operator          Device A (initiator)                    Device B (responder)
   |  oac pair --respond  |                                      |
   |------------------------------------------------------------>| ResponderPairing::start
   |  oac pair            |                                      |
   |--------------------->| PairingInitiator::start[_with_statement]
   |<-- offer blob -------|   (commitment to Ni; Ni stays in A's daemon)
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

**What the typed code is for.** It makes a man-in-the-middle between two honest devices fail:
the code covers both principals, both keys, both statements and both nonces, and an attacker
who substituted any of them cannot make the two codes agree except with probability 10^-6
per operator-visible exchange. It is **not** a proof that a human took part: a process that
controls a device sees that device's code and can type it (§1 item 3, T5).

**The window.** Each device's 120 s starts at its own verb (`PairingInitiator::start` at A's
`oac pair`; `ResponderPairing::start` at B's `oac pair --respond`) and covers the whole
exchange, six pastes and two typed codes included. The verbs print the seconds left. The help
text tells the operator to start `oac pair --respond` first, then `oac pair`, so that A's
window does not run while B is not ready (review item 7, accepted by the lead). The pasted-
blob carrier needs a **text channel between the devices** (a shared clipboard, a chat, a
terminal session on the other device); a dated note at C5 §10(b) says so. 120 s is kept for
v0.1. It is a parameter of the C5 §10(b) decision and of the informative reference-
implementation note at `spec/security.md:396-400`, not a normative requirement, so changing
it later is a C5 amendment and a note edit, not a protocol change.

**Blobs.** `oacpair1.` followed by the unpadded base64url of one canonical JSON object
(`oac_core::canonical`). Members: `kind` (`offer`, `response` or `reveal`), then for an offer
`principal`, `public_key`, `commitment` and, when the device gives one, `statement` (the
agreement statement's own JSON, `AgreementStatement::as_json`); for a response `principal`,
`public_key`, `nonce`, `statement`; for a reveal `nonce`. A blob is public: it holds no
private key, and an attacker who alters one is caught by the code, not by the blob. The
`kind` member only lets the daemon say "that is a response, not an offer" instead of
parsing a wrong blob as the right one.

**Size, and what a terminal accepts.** The 4 KiB limit (§4) is the limit of a *control line*,
not of the operator's terminal. In canonical mode macOS limits an input line to `MAX_CANON`
= 1024 bytes (`#define MAX_CANON 1024`, `bsd/sys/syslimits.h` in
https://github.com/apple-oss-distributions/xnu, branch `main`, retrieved 2026-11-03). The
worst case is not small: a principal is an identifier token of at most 128 octets
([SC-ENV-010]; `core/src/pairing.rs` `put_principal`), and an offer holds the principal, a
32-octet key and commitment, and a statement that holds the device's key id, key and
signature. By estimate (S3 measures it with 128-octet principals) an offer or response is
about 1.1 KiB as a blob. So:

- the verb **prints** a blob wrapped at 64 characters per line, then an empty line;
- the verb **reads** a pasted blob as one or more lines, concatenating them with whitespace
  removed, up to an empty line or end of input; a blob pasted as one line ends at the empty
  line the operator adds (the prompt says so), and a script that pipes blobs ends each with an
  empty line;
- the client joins the lines and sends one string, at most 4 KiB, to the daemon.

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
  connection is `ResponderPairing::abandon`. Every responder end (`PairingEnd`: `SecondOffer`,
  `Abandoned`, `Expired`, `OwnKey`, `CommitmentMismatch`) is returned to the verb and printed.
  The initiator side fails differently: `PairingInitiator::receive` returns
  `PairingError::OwnKey` or `PairingError::Expired` (`core/src/pairing.rs:486-`), not a
  `PairingEnd`; the daemon maps both to the same printed end. The initiator sends one offer
  per `start`; a new offer needs a new `oac pair`.
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
| `pairings` | its own short connection | the trusted keys with their confirmed values, `saved`, and the recent events |

**The amendment to C2 §4 (lead approval of 2026-11-03, L8).** C2 §4 (L175) fixes the control
protocol as "a request is at most 1 KiB and must arrive within 2 s". It becomes: the **first
line** of every control connection keeps the 1 KiB and 2 s bounds; after a `pair`, the lines
that follow on that connection may be 4 KiB each and each read is bounded by the time left of
the **120 s per-pairing deadline**. Every other op keeps the old bounds. A dated note is added
at C2 §4 beside the `control` paragraph, as well as the one at §6.

**The invariant in `cli/src/control.rs:11-12` is reworded in S3.** It says the control
endpoint "cannot send or receive a message, and nothing a peer writes to it reaches the core".
With `pair` and `unpair` that last clause is no longer true. S3 rewrites it: a control
connection is never handed to `Pipelines::connect_observed`, has no adapter and no session
and carries no message; **two ops, `pair` and `unpair`, change the core's trust state through
`Pipelines::pair` and `Pipelines::unpair`**; nothing else a peer writes reaches the core. The
C2 §4 note says the same.

**Invariants the state machine keeps (each has a test in §10).**

- **Deadlines.** The first line has 2 s and 1 KiB. After a `pair`, each read is bounded by the
  time left of the 120 s deadline, and a line may be 4 KiB. The daemon ends the pairing at the
  earlier of the tokio deadline and the core `Clock`'s 120 s (`deadline()` in
  `core/src/pairing.rs`), so both clocks fail closed.
- **One pairing per daemon.** A second `pair` while one is open is answered `busy` and
  closed. A pairing holds one of the four control permits for up to 120 s, so a same-user
  process can keep the operator's `oac pair` answered `busy` (availability only; the existing
  residual of the 4-slot endpoint, #438).
- **A disconnect is `abandon`.** The responder calls `ResponderPairing::abandon`; the
  initiator is dropped.
- **One response per initiator.** After the initiator has processed a response and revealed
  `Ni`, a **second** `pair_response` ends the pairing (the daemon's own end reason; the core
  consumes the initiator in `receive(self)` and has no variant for it). `Ni` is public once
  revealed, so an attacker who could present a second response could grind keys and `Nr`
  offline until the code matched the one the responder will show. Same reason as N1 for the
  responder.
- **Shutdown ends a pairing and no save follows the lock.** The control accept loop spawns each
  connection as a detached task (`cli/src/control.rs:266`, `:271`), and
  `Daemon::shutdown` awaits only its own task list (`cli/src/daemon.rs:614-618`), so today a
  connection can outlive the daemon. A pairing or unpair task that outlived shutdown could
  call `Pipelines::pair` or `remove_key`, and so write the store, after the namespace lock is
  released and while a new daemon is writing the same file. S3 therefore: (a) every pairing
  task watches the daemon's `stopped` channel and abandons at once; (b) `device_pairing`
  keeps the tasks in a set that `Daemon::shutdown` awaits **before** it releases the lock;
  (c) every engine call that writes a store (`pair`, `unpair`, the save retry) first checks a
  `closed` flag under the same mutex it saves under, and is refused after shutdown begins.
- **The pairing lines carry no key material.** A blob is public; the code is shown to the
  operator and typed back (it is not a secret against the same user, §1 item 3). Every
  pairing and unpair leaves **one log line** (`pairing started`, `pairing ended: <end>`,
  `paired key <key_id>`, `unpaired key <key_id>`), never a blob, nonce, code or statement,
  keeping the `LogSink` contract. These lines are the visibility L5 relies on.
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
{"device_key_id":...,"grants":[...],"oac_trust_store":1,"paired":[{"confirmed":
 {"agreement_key":...,"issued_at":...,"seq":...},"key_id":...,"paired_at":...,
 "principal":...,"public_key":...}],"relay_enabled":[...],"statements":[{"agreement_key":...,
 "issued_at":...,"key_id":...,"seq":...}]}
```

Arrays are sorted (paired and statements by `key_id`, grants in the engine's order) so equal
snapshots give equal bytes. Parsing is strict: an unknown member, an unknown
`oac_trust_store` version, a `key_id` that is not the SHA-256 of `public_key`, or a key that
fails [SEC-KEY-034] refuses the file. Records are rebuilt with `PairingRecord::from_store`
and `with_confirmed`, held statements with `HeldStatement::from_store`; `restore` then applies
its own checks. The grant forms are serialized from `PeerSide` and `LocalSide`
(`core/src/authorization.rs`); S1 lists their members and round-trips every variant, and the
`relay_enabled` list.

**`device_key_id` (L6, L7).** The key id of the device key that wrote the file, in plain text,
**for diagnosis only**. It is not checked for integrity and not bound to anything: the file is
not tied to the device key. If it differs from the running daemon's key id, the daemon logs
one warning and `status` reports `trust_store_foreign_key`; `oac doctor` then says the peers
trust the old key id and must pair with the new one again. The trusted peers in the file stay
loaded.

### 5.3 Where, who can read it, how it is written

- **Location.** Windows `%LOCALAPPDATA%\OAC\trust.json`. Unix `$XDG_DATA_HOME/oac/trust.json`,
  else `~/.oac/trust.json`: the directory the age key file already uses
  (`FileKeyStore::default_path`, `cli/src/keystore.rs:573`). A test namespace (`OAC_SCOPE`)
  uses `ns-<name>/` inside that directory, as the log does, so a live test never touches the
  user's store.
- **Private.** Unix: directory `0700`, file `0600`, owned by the user; a directory or file
  that is a link, not the user's, or open to others is refused, never repaired. Windows: a
  protected DACL with one ACE for the user. The helpers exist in `cli/src/logfile.rs` but sit
  in a private `mod os` (`make_private_directory`, `open_private_file`, `is_private`). They are
  lifted to a shared module in their own first slice, **S0.5** (§10), so that S1 and S1b are
  independent.
- **Atomic.** Write a temporary file in the same directory (random name, created
  exclusively, as `FileKeyStore::save` does), `sync_all`, `rename` over the target, then
  `fsync` the directory on Unix. A failed write leaves the previous file whole. The writer
  has a fault-injection seam (a small trait over create, write, sync, rename and directory
  sync) so that each step can fail in a test.
  `std::fs::rename` replaces an existing target and, on Windows, "corresponds to"
  `MoveFileExW` with a fallback to `SetFileInformationByHandle`
  (https://doc.rust-lang.org/std/fs/fn.rename.html, retrieved 2026-11-03). **UNVERIFIED:**
  that a *completed* rename is durable across a power loss on Windows. The `MoveFileExW`
  page (https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw,
  retrieved 2026-11-03) describes `MOVEFILE_WRITE_THROUGH` only for a move done as copy and
  delete and says nothing of a same-volume replace, and no first-party statement was found
  for a directory flush. The residual: a power loss may bring back the previous file (an
  older store; for an unpair, the revoked device). S1 verifies this on Windows or keeps the
  label and the risk (lead decision, round 2).
- **Missing, corrupt, unknown.** A missing file is the empty store. A file that cannot be
  parsed, exceeds 1 MiB, or has an unknown version makes `Daemon::start` fail with
  `StartError::Pairing` and a message naming the file; **it is never reset, repaired or
  renamed away.** The operator moves it aside by hand.
- **The daemon keeps the store** (`Arc<dyn PairingStore + Send + Sync>`) and uses it for
  every engine call that takes a `&dyn PairingStore` (`pair`, `remove_key`,
  `admit_statement`, `add_grant` and the rest).

### 5.4 The file is not authenticated (L6)

With a durable file, a same-user process need not use the control endpoint to change trust:
it can write `trust.json` and wait for the next start. A write can add a trusted key, add a
**grant** (the control endpoint has no grant op until #492), and switch on **permission relay**
for a session id: `restore` loads `relay_enabled` straight into the engine
(`core/src/authorization.rs:1436`; [SEC-AUZ-021]). The lead decided **not** to add a keyed tag
to the file (L6). That is part of the accepted same-user residual (T7), alongside L5: a
same-user process can already add a trusted key through the endpoint; the file adds grants and
the relay switch to what it can do without a verb. The agreement-key file is different: it
holds secrets and is sealed (§7).

## 6. The verbs (S4)

All four take `--json`. They talk to the running daemon over the `control` endpoint; none
reads a key, opens a key store, or starts a daemon.

| Verb | What it does |
|---|---|
| `oac pair` | Initiator. Prints the offer blob; asks for the response blob; prints the reveal blob and its code with both statement values; asks for the code the other device shows |
| `oac pair --respond` | Responder. Asks for the offer blob; prints the response blob; asks for the reveal blob; prints its code and statement values; asks for the code the other device shows |
| `oac unpair <key_id>` | Removes a trusted key and, in the same step, every grant and binding that names it ([SEC-KEY-035]), its held statement and confirmed values ([SEC-SEL-047]). Asks for confirmation on a terminal; `--yes` skips it and is required without one. Takes the full 64-digit key id from `oac pair --list`; no prefix matching |
| `oac pair --list` | The trusted keys: key id, principal, `paired_at`, whether agreement values were confirmed, whether the store is saved, and the recent pairing and unpair events |

**Exit codes** (the set of `08-cli-and-deployment.md` §5): `0` paired (or unpaired, or
listed); `1` the pairing ended (a `PairingEnd` or an initiator `PairingError`), the code did
not match or was refused, the result was not saved, or the key is already trusted; `2` usage;
`3` no daemon; `4` the endpoint is another user's, busy (another pairing is open), or speaks
another control version.

**`AlreadyTrusted`** (`PairError::AlreadyTrusted`, `core/src/authorization.rs` L1217) is its
own outcome with its own message and `error:"already_trusted"`: "this key is already
trusted; confirming its agreement values without removing it is #430; to pair again now,
run `oac unpair <key_id>` (this deletes its grants and bindings)". It exits `1` (lead
accepted) because the run did not do what a repeated pairing asks (new confirmed values). The
reply to the step that learns the peer's key already carries `already_trusted`, so the
operator is told before typing a code. **#430 is not implemented.** The final step is one
function (`finish(peer) -> Outcome`) so that #430 replaces only its `AlreadyTrusted` arm.

**N5** (#71, F5 review). `AuthorizationEngine::remove_key` returns
`RemoveKeyError::NotSaved` (L1190) when the removal stands in memory but the store still
holds the key. The verb must treat it as a failure: `oac unpair` exits `1` and prints
"removed in memory, but NOT saved: the revocation is NOT durable and a restart before it is
saved would restore this device". The daemon marks the store dirty and retries
`AuthorizationEngine::save` on a **bounded backoff** (1 s, doubling to a cap of 60 s) until it
succeeds; any later successful save also writes the whole snapshot. The timer runs on the
tokio clock and is tested with the paused clock (no sleeps). `status` reports the unsaved
state; `oac doctor` FAILs it. Residual: if the daemon ends before a save succeeds, the
revocation is lost; the verb's message says to check `oac pair --list` after a restart and to
unpair again.

**`--json`.** Because a pairing is several steps, `--json` is a stream: one canonical object
per line on standard output per step (`"oac_cli":1,"step":...`), blobs and codes read as
lines on standard input, the last line the result (`"ok"`, `"paired"` or `"error"`). That
departs from "exactly one object" (`oac start --json`); the lead approves the shapes in S5
(accepted in principle, round 2).

## 7. The durable agreement-key store (S1b; lead merge: the credential boundary)

**The need.** `AgreementKeys::load_or_generate` (`core/src/sealing.rs` L1135) issues `seq` 0
under a new key when the store is empty; with `MemoryAgreementKeyStore` that is every start.
[SEC-SEL-043] requires the highest issued `seq` to be kept "as durably as its device key",
and a consumer that holds `seq` ≥ 1 refuses the new statement ([SEC-SEL-045], [SEC-SEL-014]).
The state to keep is `AgreementKeyState` (L882): the private keys, each with its `seq` and
`issued_at`, and `highest_seq`. The private keys are the new secret on disk.

**Decision 2 fixes the shape**: an encrypted file next to the device key; the OS credential
store keeps its one entry. Unix: `device.key.age`'s directory. Windows: `%LOCALAPPDATA%\OAC\`,
there being no device-key file. **The file is named by the device key id** (L7):
`agreement-<key_id>.sealed`. Same privacy, atomic write and namespace rules as §5.3.

**The design point: how is it encrypted on Windows?** Facts, checked in this tree:

- `age` is a dependency of `oac-cli` on Unix only (`cli/Cargo.toml`,
  `[target.'cfg(unix)'.dependencies]`), with default features. Its key file is
  `age::scrypt` under a passphrase that a deployment provides
  (`PASSPHRASE_CREDENTIAL`, `PASSPHRASE_ENV`). **Windows has no age file and no passphrase**:
  `fallback_key_file_exists` returns `None` "where there is no such fallback"
  (`cli/src/keystore.rs:81-82`), and the file store is `#[cfg(unix)]` only. On a Unix host
  with a working keyring there is no passphrase either.
- `oac-core` depends directly on `hpke` 0.12.0 (features `alloc`, `x25519`), `sha2`,
  `x25519-dalek`, `getrandom` and `zeroize` (`core/Cargo.toml`). `hpke` carries
  HKDF-SHA256, ChaCha20-Poly1305, `Kem::derive_keypair` (RFC 9180 `DeriveKeyPair`,
  `hpke-0.12.0/src/kem.rs:44`, from the crate's source repository, in the local registry,
  read 2026-11-03), `single_shot_seal`/`single_shot_open` (`src/single_shot.rs`), and the four
  operation modes (`OpModeS::{Base, Psk, Auth, AuthPsk}`, `src/op_mode.rs:50-58`).
  `core/src/sealing.rs` already uses `Kem`, `Kdf` and `Aead` (L66-68) and `OpModeS::Base`.
- `Cargo.lock` has `hkdf` 0.12.4 (used by `hpke`, `age`, `age-core`) and `hkdf` 0.13.0
  (used by `secret-service`, a Unix keyring backend), `chacha20poly1305` 0.10.1, `aes-gcm`
  0.10.3, `scrypt` 0.11.0, `age` 0.12.1. **None of `hkdf`, `chacha20poly1305`, `hmac` is a
  direct dependency of `oac-core` or `oac-cli`**; on Windows `hkdf` 0.13.0 is not built at all.
- `DeviceKey` exposes no seed (`core/src/keys.rs`: `sign` is private); `core/src/keys.rs:5-16`
  states that the seed leaves the crate only through `SecretSeed::expose`, which a `KeyStore`
  calls, and `cli/src/keystore.rs:8-10` that the stores own no key material. The daemon reads
  the credential store **once**, at `cli/src/daemon.rs:466`.

| Option | New crate? | New feature? | Where it works | What it gives | Verdict |
|---|---|---|---|---|---|
| **(i-a) HPKE `Psk`-mode seal keyed from the device seed.** A storage key pair from `derive_keypair` over a labelled input, and a 32-octet PSK from SHA-256 over a different label; `single_shot_seal` the canonical-JSON state to the storage public key with `OpModeS::Psk` | No | No | Every platform, both device-key backends | A copy of the file alone is useless without the seed. **A file the seed's holder did not write fails to open**, because the PSK is needed to author one. It does **not** detect rollback to an older valid file (§9 T10) | **Recommended** |
| (i-a′) The same with `OpModeS::Auth`, the storage key pair as sender | No | No | Every platform | As (i-a); authors need the storage private key | Rejected: a self-addressed sender (sender = recipient) is an unusual use with no first-party text; `Psk` is the mode defined for a secret both ends hold |
| (i-b) `age` X25519 identity, held in memory only, derived from the seed | No | Yes on Windows (the `age` dependency becomes unconditional: `ml-kem`, `p256`, `i18n-embed`, `rust-embed` and the rest of `age`'s tree enter Windows builds; no new locked package, a new build surface) | Needs the `cfg(unix)` change in `cli/Cargo.toml` | As (i-a) | Rejected: the same protection for a larger Windows build and an extra dependency edge |
| (i-c) Symmetric key by HKDF over the seed, then `chacha20poly1305` | Direct dependency edges on `hkdf` and `chacha20poly1305` | No new package; new edges and `07` §5 rows | Everywhere | As (i-a) | Rejected: `hpke` already composes both and `sealing.rs` already uses it |
| (ii) The Unix age-scrypt passphrase path | No | No | Unix with a file-backed device key only | Strongest there | Rejected as the one mechanism: absent on Windows, and on Unix with a keyring the passphrase does not exist |
| (iii) Windows DPAPI (`CryptProtectData`) | No | Yes (`windows-sys` feature `Win32_Security_Cryptography`) | Windows only | Another user and offline theft; not a same-user process | Rejected: a second mechanism on Windows only, and a feature the lead asked to avoid |
| (iv) A second credential-store entry | No | No | Everywhere | As the device key | Excluded by decision 2 (one entry; C4 §12) |
| (v) Plaintext file, `0600` / user-only DACL | No | No | Everywhere | Other users only | Rejected: a leaked copy gives the private agreement keys, which open every payload sealed to this device |

**Why `Base` mode is not used (review blocker B2).** `OpModeS::Base` "is the only mode that
does not provide any kind of sender identity authentication" (`hpke-0.12.0/src/op_mode.rs:46-49`).
With `Base`, anyone who knows the storage **public** key can write a file that opens, holding
agreement private keys the attacker chose; the daemon would then issue statements for keys the
attacker holds. The seal is therefore `OpModeS::Psk` with the 32-octet PSK that only a holder
of the seed can compute. The crate requires "at least 32 bytes of entropy"
(`op_mode.rs:6-10`); the seed is 32 uniformly random octets and the PSK is SHA-256 over a
labelled input, so it meets that.

**The construction (S1b).**

- *Derivation labels, one per purpose.* Storage key pair: `derive_keypair` over
  `"oac-agreement-store-kem-v1" 0x00 || seed`. PSK: SHA-256 over
  `"oac-agreement-store-psk-v1" 0x00 || seed`; `psk_id` is `"oac-agreement-store-v1"`. The
  seal's HPKE `info` is `"oac-agreement-store-v1"`. **`SEAL_INFO` (`oac-seal-v1`,
  `core/src/sealing.rs:71`) is never used**, so a payload frame can never be confused with a
  stored file.
- *File layout.* A plain header (magic, version, kind, the 64-digit device key id), then the
  HPKE `enc` (32 octets) and the ciphertext with its tag. The **associated data is the header**:
  file kind (`agreement-keys`), version (`1`) and the device key id. A file renamed to another
  key id, or of another kind or version, fails to open.
- *A fresh `enc` on every write.* `single_shot_seal` draws a new ephemeral key from the
  operating system's random source (`OsRng`, as `core/src/sealing.rs:354-362` does) for each call, so no
  two writes share an `enc` or a content key, and the state is not re-encrypted under a reused
  nonce.
- *Seed containment (review should-fix 1).* The seed does not leave `oac-core`. A function
  inside `core/src/keys.rs` (for example `DeviceIdentity::at_rest()`, built from
  `DeviceKey`'s `SigningKey::as_bytes`, `ed25519-dalek-3.0.0/src/signing.rs:131`) derives the
  storage key pair and the PSK and returns an opaque handle, `AtRest`, that offers
  `seal(kind, key_id, plaintext)` and `open(kind, key_id, bytes)` and no accessor for the
  derived material (zeroized on drop). `Daemon::start` builds the identity at `daemon.rs:466-467`
  and then calls a constructor in `Parts::agreement_store` with that handle. The credential
  store is read once, at `daemon.rs:466`, and the `cli/` file store holds the handle, never the
  seed. Tests keep passing `MemoryAgreementKeyStore`.

**What it buys, stated plainly.** The file is as strong as the device key. It inherits the
key's residuals: a same-user process that can read the seed can read and forge the file (§9
T7), exactly as it can already sign as this device. It adds protection against the file alone
leaving the host, and against a writer who does not hold the seed. It adds **no** protection
against rollback (T10). The seed is used only as input to a domain-separated derivation inside
core; it is not disclosed ([SEC-KEY-004]).

**Regeneration and other key ids (L7).** The current key id's file is the only one read. A
regenerated device key has a new key id, so its file starts empty and its statements start at
`seq` 0. Files of other key ids are ignored and never deleted. The daemon refuses to start only
when the **current** key id's file exists and fails to open (corrupt, tampered, unknown
version). A missing file for the current key id is a first start, below.

**Loss and rollback of the file (review should-fix 2).** The cases, and what the daemon does:

| Case | What happens | Why it is acceptable |
|---|---|---|
| No file for the current key id | Treated as a first start: `load_or_generate` generates a key at `seq` 0 and saves it, reading back before use (`save_checked`) | The existing `None` arm (`sealing.rs:1135-1150`). If the file was in fact lost, this is the *lost count* case: peers holding a higher `seq` refuse the new statement ([SEC-SEL-045]) and keep sealing to the old key or none; recovery is a removal and a new pairing ([SEC-SEL-052]) |
| The file exists and fails to open | Refuse to start, naming the file; never reset | Resetting would issue `seq` 0 silently |
| A validly sealed **older** file returns (a backup restored, or an undone rename, §5.3) | Opens, and **cannot be told from the current one**: `highest_seq` is lower than what was issued, and a key erased under [SEC-SEL-018] is back on disk | Not detectable from the file alone. The next statement reuses a `seq` that peers hold, so they refuse it (fail closed); erased keys are erased again at the next `erase_replaced` |

**Mitigation available without a new crate (slice S1c, optional, §10).** Also record the
highest issued `seq` as `agreement_seq_floor` in `trust.json`, written *before* the statement
exists. The file store's `load()` then returns `highest_seq = max(file, floor)` and, when the
file is missing or older, an empty key list; the existing arm of `load_or_generate` for "every
key was erased, the count stays" (`sealing.rs:1143-1146`) then issues `floor + 1` under a new
key. That is conformant ([SEC-SEL-040]: higher than any before) and needs no re-pairing. It
does not help when both files return together (both renames undone) or are deleted together.
A third copy in the credential-store entry would survive both, but it changes the one-entry
credential boundary (C4 §12), so it is **not proposed**; it is a question for the lead (Q-B).

**Failure modes.** Write fails: the statement is not issued ([SEC-SEL-043]: the new `seq` is
durable before its statement exists).

## 8. Core, doctor and status (S2, S5)

**`Pipelines::pair` and `Pipelines::unpair` (S2).** Today the pairing engine calls are made
through `Pipelines::with_engine` (`core/src/pipeline.rs` L1414), which changes trust and
releases nothing. Presence of an own session is released to the trusted devices when the
session registers (`inner.release_presence(&sid)` at L1315; `release_presence` at L2465) and
when a peer's statement is admitted (`Pipelines::admit_statement`, L868, which announces every
own session again to the device at L881). Neither covers a session that registered before the
pairing. The one other path is a carrier appearance: `on_carrier_appeared` and `announce_set`
(`core/src/pipeline.rs:2401-2460`) announce each own session to each trusted device, so a
newly paired device is reached at the next appearance, with no timing tied to the pairing. S2
adds:

- `Pipelines::pair(peer, store)`: `engine.pair`, then `announce_to(session, key, false)` for
  every own session, as `admit_statement` does;
- `Pipelines::unpair(key_id, store)`: forgets the announcement records that name the device
  and sends the withdrawals `unbind` would, best effort, **before** the removal (afterwards
  [SEC-AUZ-011] no longer lets one go to it), then `engine.remove_key`; the result says
  whether it is durable;
- the save-retry bookkeeping of §6 (a dirty flag, `retry_save`, and `trust_unsaved` in
  `CoreStatus`, `core/src/status.rs`).

**The save runs under the core mutex.** `with_engine` runs its closure on the engine under the
core lock (`pipeline.rs:1414`), and `admit_statement` shows the pattern (`:874-875`: the lock
is taken, then `engine.admit_statement(statement, store)` saves). So the write, `sync_all`
and `rename` of a `pair`, `unpair` or retry hold the core mutex for as long as the disk takes,
and delivery waits meanwhile. Saves are rare (a pairing, an unpair, a statement) and the file
is at most 1 MiB; the note accepts this and S1 measures it. Nothing that waits on the network
may run under that lock.

Observation for the lead: release needs a grant (`AuthorizationRequest::ReleasePresence`,
`core/src/pipeline.rs` L2301-2308), and a grant that names a key cannot survive its removal
([SEC-KEY-035]). So until #492, `Pipelines::pair` releases nothing; it gives #492's
`add_grant` path and this path one release point, and S2 tests it with a grant loaded through
`with_engine`. Precedent says no spec bump: `admit_statement` made the same change to the
pipeline without one.

**Doctor and status (S5).** The `pairing` check (`cli/src/doctor.rs:485-505`) today WARNs when there are no
pairings *and* no grants, and PASSes when either count is above 0. It becomes:

- *Grants with no paired device stay PASS.* That is the single-device setup: two harnesses on
  one device share the device key and need grants to reach each other (C5 §10(a);
  [SEC-AUZ-007]; `core/src/pairing.rs` module documentation). The count the check needs is
  `paired_without_grant`, a new member of `status` `counts` (trusted keys other than this
  device's that no grant names, `Grant::names_key`), so the check cannot mistake a grant for
  a device.
- *WARN only for a paired key that no grant names:* "N paired device(s) have no grant: nothing
  is delivered to or from them (#492)". WARN "no pairings or grants" stays as today.
- ***Recent pairings* (L5).** `status` carries `recent_pairings`: the last 16 pairing and
  unpair events since the daemon started (kind, key id, time), kept in memory, and the
  `paired_at` of each trusted key from the store. The `pairing` check's detail lists up to
  three of them, so an operator sees "paired `<key id>` 4 minutes ago" without opening the
  log. The same events are in the log (§4) and in `oac pair --list`.
- ***FAIL*** when the status reply reports the store unsaved (N5).
- *WARN* when `status` reports `trust_store_foreign_key`: "the trust store was written by
  another device key: peers must pair with this device again" (L7).
- The `trust_store` file itself is not parsed by doctor: it reads `device_key_id` through the
  daemon's `status`, so `scripts/check-doctor-boundary.mjs` stays as is (to be confirmed in
  S5).

The lead approves the JSON shapes.

## 9. Threats (`oac-security-work` template)

**Reading this table.** No row is a closed mitigation until its slice lands: a row whose
"proving test" is a test named here but not yet written is an **open risk** (`oac-security-work`
§1). Rows that rest on tests that exist today say so. T1-T12 are about this feature; the
inventory coverage that follows names every other row of the `oac-security-work` §2
checklist and says why this feature does not change it.

| # | Attack | Precondition | Mitigation | Proving test | Residual risk |
|---|---|---|---|---|---|
| T1 | The channel the operator carries blobs over swaps a key (MITM between two honest devices) | The attacker sees or edits the pasted blobs, and does not control either device | Commit-then-reveal; the code covers both principals, both keys, both statements and both nonces; the operator types the **other** device's code; five tries, 120 s ([SEC-KEY-032]; C5 §10(b), F5-1) | Existing: `tests/security/tests/keys_and_pairing.rs` `s13_pairing_mitm_substitution_is_caught_by_the_code`. Planned (S3): `a_substituted_key_in_the_pasted_blob_gives_codes_that_differ` | 10^-6 per operator-visible exchange |
| T2 | Grinding: restart exchanges until the responder's code matches the one the initiator shows (N1); or present a second response to the initiator after `Ni` is revealed | The attacker can send offers or responses | One offer per operator-started pairing; a second offer, a disconnect, expiry, an own key or a bad reveal each end the pairing and are shown; **a second response ends the pairing** (§4) | Existing: `s13_second_offer_ends_the_pairing`. Planned (S3): `a_second_offer_ends_the_pairing_and_the_verb_shows_it`, `a_second_response_ends_the_pairing`, `a_disconnect_abandons_the_pairing`, `the_pairing_expires_at_120_seconds`, `an_offer_carrying_this_devices_key_ends_the_pairing`, `a_tampered_reveal_ends_the_pairing` | An attacker who also blocks the operator's view of the end |
| T3 | An attacker **between two honest devices** guesses the six digits | The attacker is on the channel and sees neither device's screen | Five wrong entries then `aborted`; the code is on the other device's screen; 120 s | Existing: `s13_five_wrong_codes_abort_and_the_window_expires`. Planned (S3): `five_wrong_entries_abort_over_the_control_connection` | 10^-6 per try, five tries per pairing. **It does not apply to a same-user process that controls a device: that process sees the code and types it (T5)** |
| T4 | A statement is stripped from the exchange so an older one is accepted later ([SEC-SEL-044], [SEC-SEL-045]) | The attacker alters the pasted blobs | Statements enter the commitment and the code; the daemon judges a statement before showing the code ([SEC-SEL-051]); confirmed values are saved with the key ([SEC-SEL-046]) | Existing: `s13_a_statement_stripped_from_the_response_is_a_mismatch`. Planned (S3): `a_stripped_statement_gives_codes_that_differ`; (S1) `confirmed_values_survive_a_restart_and_an_older_statement_is_refused` | The operator who confirms without comparing the statement values the verb prints |
| T5 | **A same-user process, a prompt-injected model's tool shell included, adds a trusted key through the `control` endpoint with no operator** (L5). It pairs this device with a second `oac` daemon it controls, for example on loopback under its own `OAC_SCOPE`, or with any program that speaks the public exchange, and types both codes. It can also `unpair`, start and abandon pairings, and read `pairings` | A process of the user can open the endpoint (the owner and DACL check pass for it) and can run a program | **Visibility only (L5):** one log line for every pairing and unpair (§4); `oac pair --list`; `oac doctor` shows recent pairings (S5). A trusted key receives nothing until a grant names it ([SEC-AUZ-001]; the grant verbs are #492) | Planned (S3): `pairing_completes_between_two_honest_daemons_only_on_the_entered_code` — **this proves only MITM resistance between two honest devices (T1/T3); it proves nothing against a same-user process**. Planned (S3/S5), visibility: `every_pairing_and_unpair_leaves_one_log_line`, `status_reports_recent_pairings`, `doctor_lists_recent_pairings` | **Accepted by the lead (L5).** The operator's typed code does not stop this attacker. Residual: a same-user process can add a trusted key, remove one, and deny the operator's pairing (it can already kill the daemon, `RISK-LOCAL-IPC`) |
| T6 | A lost revocation: `unpair` returns, the store did not save, a restart restores the revoked device (N5) | A disk error, a locked file, a scanner | The verb exits 1 and says so; the daemon retries with a bounded backoff; `status` reports it; `doctor` FAILs | Existing: `row08_a_failed_save_still_revokes`, `row08_removal_survives_a_restart`. Planned (S2/S3): `an_unpair_whose_save_fails_exits_1_and_is_retried`, `a_failed_pair_save_adds_no_key` | If the daemon ends before a save succeeds, the revocation is lost until the operator unpairs again |
| T7 | The trust file or the agreement file is edited or copied by a same-user process | A process of the user can write or read the data directory | Private modes and DACL, refused-not-repaired; the agreement file is sealed under a key derived from the device seed (§7); **the trust file is not authenticated (L6)** | Planned (S1/S1b): `a_loose_mode_is_refused_not_repaired`, `the_dacl_on_trust_json_is_user_only` (Windows), `a_tampered_trust_file_is_refused_when_unparsable`, `the_agreement_file_holds_no_plaintext_key`, `a_file_the_seed_did_not_seal_fails_to_open`, `an_edited_agreement_file_fails_authentication` | **Accepted same-user residual (L6).** A same-user write to `trust.json` can add a trusted key, add a grant, and switch on `relay_enabled`, because `restore` loads it into the engine (`core/src/authorization.rs:1436`). A process that can read the seed (the OS credential store as the user) opens and forges the agreement file: the same residual as the device key |
| T8 | A crash or power loss leaves a half-written or older store | Power loss, a kill | Temp file, `sync_all`, `rename`, directory `fsync` (Unix); a failed write leaves the previous file | Planned (S1): `a_failed_write_leaves_the_previous_file_whole` (through the fault-injection seam, one case per step) | Windows durability of a completed rename is **UNVERIFIED** (§5.3): an older file may return |
| T9 | The restart problem: after a restart the device issues `seq` 0 under a new key | The agreement store is in memory | The durable store (§7) | Planned (S1b): `seq_and_key_survive_a_restart` ([SEC-SEL-043]) | A missing file for the current key id is a first start and issues `seq` 0 (§7); an unreadable one stops the start |
| T10 | **Rollback or loss of the agreement file:** a validly sealed older file (backup restored, or an undone rename) lowers `highest_seq` and brings back keys erased under [SEC-SEL-018]; or the file is deleted | A same-user process, a restore, a power loss | Fail closed at the peers (a repeated `seq` is refused); optional floor in `trust.json` (S1c) issues `floor + 1` | Planned (S1b): `a_missing_file_is_a_first_start`; (S1c, if adopted) `a_rolled_back_file_issues_the_floor_plus_one`, `a_floor_without_a_file_issues_the_floor_plus_one` | An older file cannot be told from the current one; erased keys are back on disk until the next erase; if both files roll back or go together, peers refuse the new statement until a re-pairing ([SEC-SEL-052]). The credential-store copy of the count is not proposed (Q-B) |
| T11 | A pairing or unpair task outlives the daemon and writes the store after the lock is released, racing the next daemon | A control connection open at `oac stop` (`control.rs:266,271` detach connections; `daemon.rs:614-618` does not await them) | Every pairing task watches `stopped` and abandons; the tasks are awaited before the lock is released; a store-writing call is refused after shutdown begins (§4) | Planned (S3): `stop_ends_an_open_pairing`, `no_store_write_follows_the_shutdown` | None known once built |
| T12 | The typed code is brute-forced through the verb by a same-user process | Control access | Five entries, then `aborted`; but see T5: it needs no guessing | Planned (S3): as T3 | Covered by T5 |

**Inventory coverage** (`oac-security-work` §2). Impersonation: T1, T4 and the signature rows
of `spec/security.md` §13, unchanged; T5 for a same-user process. Unauthorized routing and
discovery: pairing grants nothing ([SEC-AUZ-001]); a paired device receives nothing until a
grant (#492) and doctor says so. Tampering: T1, T4, T7. Replay: unchanged; T4 covers statement
replay at pairing, T10 covers a replayed file. Malicious peer prompt injection: unchanged;
pairing is not an instruction channel, and the doctrine "authenticated but untrusted" holds: a
paired key's messages are still untrusted instructions (`oac-security-work` §3), and a
prompt-injected process is the T5 attacker. Compromised transport infrastructure: the blobs
never touch the peer transport; the exchange is the operator's. Cross-project disclosure:
unchanged; `pairings` shows key ids and times, no working directory or session id. Leaked
credentials: T7, and the log carries no blob, nonce, code or key. Zenoh `zid` is not an
identity: unchanged (the `zid` plays no part here). Claude permission relay stays off by
default in the product; this note does not enable it, but T7 records that a same-user write to
the trust file can switch it on. `turn/steer` is untouched. The
`--dangerously-load-development-channels` confirmation is untouched. Provenance stays
machine-set metadata; nothing here renders provenance. Local IPC peer authentication
(named-pipe security descriptors, Unix socket permissions) is what admits the control
connection; T5 states what that does and does not stop.

## 10. Slices and acceptance tests

Each test uses in-process daemons over `oac-transport-memory` (`MemoryNetwork`,
`MemoryTransport`), an injected `Clock` (`oac_core::clock::ManualClock`) and tokio's paused
clock, **no sleeps and no polling** (`oac-testing`; `cli/tests/appearance_timer.rs` is the
pattern), loopback only: the IPC endpoints are the user's own, in a test namespace
(`Scope::for_user_in`), each daemon with a store in its own temporary directory. Tests that
need a real credential store stay opt-in (`#[ignore]`, `OAC_TEST_REAL_KEYRING=1`).

| Slice | What | ~Lines | Lead merge? | Acceptance tests (names are the plan) |
|---|---|---|---|---|
| **S0.5** | Lift the private-file helpers (`make_private_directory`, `open_private_file`, `is_private`, the Unix and Windows `mod os` of `cli/src/logfile.rs`) to a shared `cli/src/private_file.rs`; the log keeps working | 120 | no | The existing `cli/tests/logfile.rs` passes unchanged; `a_loose_mode_is_refused_not_repaired` (Unix) and `the_dacl_is_user_only` (Windows) move with the helper |
| **S1** | `cli/src/trust_store.rs`: the durable `PairingStore` (§5), its fault-injection seam; the daemon keeps the store; `oac start` passes the file store. Depends on S0.5 only | 500 | no | `snapshot_round_trips_every_grant_and_record`; `relay_enabled_round_trips`; `a_missing_file_is_an_empty_store`; `a_corrupt_or_unknown_version_file_stops_the_daemon_and_is_not_reset`; `an_oversize_file_is_refused`; `a_loose_mode_is_refused_not_repaired`; `the_dacl_on_trust_json_is_user_only` (Windows); `a_failed_write_leaves_the_previous_file_whole` (one case per step of the seam: create, write, sync, rename, directory sync); `a_foreign_device_key_id_is_reported_not_refused`; **SEC-SEL-046** `confirmed_values_survive_a_restart_and_an_older_statement_is_refused` (pair with a statement, stop the daemon, start another on the same file, `admit_statement` of a lower `seq` is `BelowConfirmed`); **SEC-SEL-047** `after_removal_and_restart_the_record_and_confirmed_values_are_gone` |
| **S1b** | The durable `AgreementKeyStore` (§7): the `oac-core` at-rest handle, `cli/src/agreement_file.rs`, the `Parts` constructor, the daemon keeps the store. Depends on S0.5 only, so S1 and S1b are independent | 450 | **yes**, credential boundary | **SEC-SEL-043** `seq_and_key_survive_a_restart` (same statement after restart; after a `replace`, the next start issues `seq` 1 not 0); `the_agreement_file_holds_no_plaintext_key`; `a_file_the_seed_did_not_seal_fails_to_open` (a file sealed in `Base` mode, or under another PSK, is refused: review blocker B2); `an_edited_agreement_file_fails_authentication`; `a_file_renamed_to_another_key_id_fails_to_open` (the header is the AAD); `each_write_draws_a_fresh_enc`; `the_info_is_not_the_payload_info`; `a_regenerated_key_starts_its_own_file_at_seq_0_and_leaves_the_old_file`; `a_missing_file_is_a_first_start`; `the_file_is_private_and_written_atomically`; no `hkdf`, `chacha20poly1305` or `hmac` direct dependency and `Cargo.lock` unchanged (`scripts/check-crate-deps.mjs`); no `SecretSeed` in any `cli/` signature |
| **S1c** (optional) | `agreement_seq_floor` in `trust.json` (§7). Depends on S1 and S1b | 120 | no | `a_rolled_back_file_issues_the_floor_plus_one`; `a_floor_without_a_file_issues_the_floor_plus_one`; `the_floor_is_written_before_the_statement_exists` |
| **S2** | Core `Pipelines::pair` / `unpair`: presence release and withdrawal, save-retry bookkeeping, the `closed` flag. Independent of S1 | 250 | no | `pair_releases_an_existing_sessions_presence_to_the_new_key` (grant loaded with `with_engine`); `unpair_withdraws_then_removes_and_drops_the_announcement_records`; `an_unpair_whose_save_fails_reports_not_durable_and_retry_clears_it` (paused clock, bounded backoff); `a_failed_pair_save_adds_no_key`; `a_store_writing_call_is_refused_after_shutdown_begins` |
| **S3** | `cli/src/device_pairing.rs`: the control ops and the state machine (§4); the C2 §4 amendment; the `control.rs` invariant reworded. Depends on S1 and S2 | 600 | no | The N1 cases: `a_second_offer_ends_the_pairing`; `a_second_response_ends_the_pairing`; `a_disconnect_abandons_the_pairing`; `the_pairing_expires_at_120_seconds` (the 2 s deadline applies to the first line only); `an_offer_carrying_this_devices_key_ends_the_pairing`; `a_tampered_reveal_ends_the_pairing`; `five_wrong_entries_abort`; `a_second_pairing_is_busy`. `a_stripped_statement_gives_codes_that_differ`. `the_offer_reply_carries_no_Ni` (the reply holds the commitment; `Ni` appears only in the reveal, after the response). `pairing_completes_between_two_honest_daemons_only_on_the_entered_code` (proves only MITM resistance between honest devices); `AlreadyTrusted_is_its_own_outcome`; `an_unpair_whose_save_fails_exits_1_and_is_retried` (N5). **Shutdown:** `stop_ends_an_open_pairing`; `no_store_write_follows_the_shutdown`. **Size:** `the_worst_case_blob_with_128_octet_principals_fits_a_control_line` (measured; asserts it is under 4 KiB and reports its size). **Visibility:** `every_pairing_and_unpair_leaves_one_log_line`; `no_log_line_holds_a_blob_nonce_code_or_statement` |
| **S4** | The verbs (§6). Depends on S3 | 550 | no | Each verb through its `_with` function with in-memory streams: exit codes `0/1/2/3/4`; `--json` shapes; `oac_pair_never_builds_a_key_store` (no `KeyStore` symbol in the client path: source check); a wrong blob kind is diagnosed without a state change; `a_blob_wrapped_over_several_lines_is_accepted`; `a_blob_printed_by_the_verb_has_no_line_over_64_characters`; `the_prompt_names_the_empty_line_that_ends_a_blob` |
| **S5** | Doctor and status changes (§8); the `08` §5/§6, C2 §6, `11-risks` and STATUS documentation. Depends on S4 | 350 | lead approves the JSON shapes | `doctor` cases for `pairing`: grants and no device is PASS; a paired key no grant names is WARN; no pairings and no grants is WARN; unsaved is FAIL; `trust_store_foreign_key` is WARN; the detail lists recent pairings; `status` carries `paired_without_grant`, `recent_pairings`, `trust_unsaved`; documentation sweep of the `pairing verb (#437)` mentions |
| **S6** | `spec/security.md` editorial revision: the Appendix A cells for [SEC-KEY-032] (about L2250), [SEC-SEL-046] and [SEC-SEL-047] (about L2389-2390), the stale §13 rows "Trust on first use" and "Leaked device key" (about L1576-1577), and the §5.3 reference-implementation note at L396-400, whose "a six-digit code derived from both devices' key ids" (L397) is stale (the code covers the nonces and the statements) | n/a | **yes** | The cells name the S1/S3 tests; `node tests/protocol/runner/run.mjs` stays green; a minor revision under E7 §7 |

**Order.** S0.5 first. Then S1, S1b and S2 in any order or in parallel (S1 and S1b depend on
S0.5 and on nothing else; S2 is core only). S1c after S1 and S1b, if adopted. S3 after S1 and
S2 (the binary also needs S1b, but S3's tests use the in-memory agreement store). S4 after S3.
S5 after S4. S6 last, naming the tests that exist.

## 11. Open questions for the lead

Resolved by the lead's decisions of 2026-11-03: the tag on `trust.json` (L6: none); a store for
another device key (L7); the C2 §4 amendment (L8); where the helper lives, `AlreadyTrusted`'s
exit code, the `--json` stream, the Windows rename durability (accepted defaults); the 120 s
window (kept). Still open:

1. **Q-A: adopt S1c** (the `agreement_seq_floor` in `trust.json`), which reduces T10 without a
   new crate or a boundary change? Recommended: yes, as a small follow-up after S1 and S1b.
2. **Q-B: a copy of the highest `seq` in the credential-store entry.** It would survive the
   loss of both files, and it changes the one-entry credential boundary (C4 §12) that decision 2
   keeps. Recommended: no.
3. **Q-C: the numbers for visibility.** The last 16 events kept in memory, three shown by
   doctor, and the backoff of 1 s doubling to 60 s are defaults of this note.
4. **Q-D: review of the `Psk`-mode seal.** `Psk` is the mode defined for a secret both ends
   hold, and the construction uses only `derive_keypair`, SHA-256 and `single_shot_seal`; there
   is no first-party text on sealing a file to a key derived from a seed. S1b is a lead merge;
   a cryptographic review is a condition the lead may want to set.
5. **Q-E: a pairing holds one of four control permits for up to 120 s** (§4). #438 sizes the
   endpoint; this note does not change the number.

## 12. Gaps recorded

- The daemon drops its pairing store after `restore` (`cli/src/daemon.rs:471`) and its
  agreement store after `load_or_generate` (L476); S1 and S1b close both.
- Control connections are detached tasks that a daemon shutdown does not await
  (`cli/src/control.rs:266,271`; `cli/src/daemon.rs:614-618`); S3 closes it for pairing (§4).
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
- The reference-implementation note at `spec/security.md:396-400` describes the code as derived
  from both devices' key ids; the code also covers the nonces and the statements. Folded into
  S6.

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
L405, L486, L698, L868, L963-L981, L1004, L1080, L1147, L1181, L1200); `core/src/authorization.rs`
(`restore` L1416 with the relay load at L1436, `snapshot` L1458, `save` L1477, `pair` L1585,
`admit_statement` L1635, `remove_key` L1716, `RemoveKeyError` L1190, `PairError` L1217);
`core/src/pipeline.rs` (L868-L886, L1315, L1335-L1348, L1414, L2301-L2308, L2401-L2460,
L2465-L2477); `core/src/sealing.rs` (L66-L71, L882, L910, L1135-L1150); `core/src/keys.rs`
(L5-L16, `DeviceIdentity` L343); `core/src/clock.rs` (`ManualClock` L53); `cli/src/control.rs`
(L8-L14, L266, L271); `cli/src/daemon.rs` (L317, L439, L466, L471-L476, L614-L618);
`cli/src/commands.rs` (L860-L864); `cli/src/doctor.rs` (L485-L505); `cli/src/keystore.rs`
(L8-L14, L81-L82, L573); `cli/src/logfile.rs`; `cli/Cargo.toml`; `core/Cargo.toml`;
`Cargo.lock`; `spec/security.md` [SEC-KEY-002], [SEC-KEY-032], [SEC-SEL-040], [SEC-SEL-043] to
[SEC-SEL-052] (L396-L400, L1727-L1731). External, retrieved 2026-11-03: `std::fs::rename` and
`MoveFileExW` (§5.3); `bsd/sys/syslimits.h` of `apple-oss-distributions/xnu` on `main`
(`MAX_CANON` 1024; the commit was not recorded, so the value is as of that fetch). From the
local cargo registry, read 2026-11-03: `hpke` 0.12.0 (`src/op_mode.rs:6-10, 46-58`,
`src/kem.rs:44`, `src/single_shot.rs`) and `ed25519-dalek` 3.0.0 (`src/signing.rs:131`).

**UNVERIFIED (carried to `STATUS.md`):** that a completed `rename` over an existing file is
durable across a power loss on Windows, with or without a directory flush (§5.3).
