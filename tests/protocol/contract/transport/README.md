<!-- SPDX-License-Identifier: Apache-2.0 -->

# Shared transport contract suite

`oac-contract-transport` is a test-only crate. Every transport runs `run(&harness)`
and calls `Report::assert_conformant()`. Each check gets a fresh isolated `Medium`.
The harness supplies clock, settle/subscription barriers, optional scripted faults,
and implementation configuration. Default local-ci runs the suite against the memory
transport and the loopback reference transport.

## Spec-driven sealing revision, 2026-10-10

The lead approved this suite revision before PR #374, against `spec/interfaces.md`
revision 0.3 §6.10 and `spec/security.md` revision 0.3 §14. It establishes a new
Gate S4 contract-suite baseline: **e7d55d59a84bea587e992da5b680be9569336fcd** (implementation commit;
`docs/planning/STATUS.md` records the same baseline). This changes the suite rather
than requirement text. Refs #59, #370, #374 and #62.

The harness must supply `TransportHarness::binding_sealing()` from its independently
reviewed binding, never from `start`. Every observed start is cross-checked against
that value (IFC-TRN-110); a false declaration cannot disable conformance obligations.
The actual `start` declaration selects logical sealing/opening. With `sealing = false`,
every existing assertion,
plain payload, destination, operation and maximum-size check stays unchanged. The
additional sealing rows report that the capability is absent.

With `sealing = true`, the suite-side `sealed::Logical` helper builds HPKE frames
using `oac_core::sealing::seal`, and opens them using the core sealing API from
PR #370. Synthetic payloads have a 100-octet routing/expiry control **inside the encrypted
plaintext** so the original session/device and kind assertions can run unchanged.
The last eight control octets encode the deadline relative to the world's shared
clock epoch. The simulated receiving core checks it after opening and silently drops
expired payloads (SEC-SEL-036), including presence. The receiving sealing transport
may hand over late network arrivals, which IFC-TRN-034 (revision 0.7) leaves to the
receiving core. A separate raw sender check
keeps already-expired input observable, and `FaultControl::next_sender_hold` tests
sender-side in-flight retention when the medium supports that control; ordinary
`next(Fate::Delay)` may represent network delay. Unsupported sender holding is
reported in the sender check's evidence, not claimed as exercised.
The two exercised devices have separate test agreement key pairs. The tested
transport receives only `PayloadKind::Sealed`, device destinations and the original
send deadlines. Presence travels through `publish` and the local device subscription;
`watch_presence` supplies carrier loss. Exact sent frame octets are tracked before
opening, so a changed frame cannot satisfy the original payload checks. The large
logical-payload case reserves the routing label and 54-octet frame overhead; the
separate raw-frame case exercises the full declared transport maximum.

Sealing hides content, never authenticates a sender. These are synthetic transport
payloads, not signed-protocol conformance fixtures. Authenticated peer messages remain
untrusted instructions; this suite does not authorize their content.

| Check | Evidence exercised |
|---|---|
| IFC-TRN-113 | Every plain kind, through both `publish` and `send_presence`, to session and device destinations, returns `NotTaken`; refused traffic is not delivered. |
| IFC-TRN-104 | Using the core's 54-octet minimum overhead, a real frame at `max_payload_octets` is taken and delivered byte-for-byte; a real frame one octet larger is refused and never delivered. The session declaration rule itself remains core-owned. |
| IFC-TRN-105 | All three inner kinds arrive as unchanged sealed frames on the local device subscription. |
| IFC-TRN-107 | Run-wide captures from every exercised medium cover taken frames of every size, including maximum-size, fault, expiry, restart and presence traffic; no accompanying application values. |
| IFC-TRN-108 | Observed link, peer, carrier and liveness identifiers do not contain the test device/session ids in text or raw-octet form. |
| IFC-TRN-110 | Every start agrees with the binding expectation supplied independently by the harness. |
| IFC-TRN-071 | Raw subscription and presence callbacks are counted before recognition/opening; malformed callbacks after shutdown fail. |
| IFC-TRN-109 | The declared sealing frame cap is at least 65590. |

## IFC-TRN-030 and lost payloads, 2026-10-13 (#392)

IFC-TRN-030 says a transport delivers a payload's octets, "when it delivers them", exactly
as passed. It is an integrity rule, not a delivery rule; delivery of every payload is the
`reliability` capability (Table 6.3). The check holds the two payloads it passes through
the medium (all 256 octet values, and one of the declared maximum, up to 1 MiB) to
delivery only when `start` declares `reliability` present. When it declares it absent, a
payload that never arrives is not a failure, but every payload that does arrive must be one
passed, octet for octet, at least one must arrive, and the duplicate copies scripted by
fault control must still all arrive. Before this, a transport that declared no reliability
failed the check whenever its native best-effort send queue dropped the large frame on a
loaded host (a native send queue that drops under congestion).

The lead approved working on a fix for #392, including a contract-suite change if the
investigation supported one (2026-10-13); the lead merges the PR that makes it.

Residuals, recorded for the lead rather than fixed here:

- *Closed 2026-10-15 (#396, #421).* The gap that no check named a duty to carry a payload
  of the declared `max_payload_octets` is covered. Interfaces 0.9 states the rule
  (IFC-TRN-118) and the section below describes the check. What remains open under it: the
  check asks for acceptance (`taken`), not delivery, of a transport that declares no
  `reliability`, because the spec requires no delivery then.
- IFC-TRN-104 (`frame-size-cap`, sealing transports only) still publishes a frame of the
  declared maximum and requires it to arrive. On a transport whose native send queue
  can drop it, it carries the same exposure as the old IFC-TRN-030 and is not changed here.
- No transport declares `reliability`, so the branch that holds a declaring transport to
  delivery runs only against the planted breach `DropsLargeClaimsReliability`
  (`transports/memory/tests/contract.rs`). `DropsLarge` is the positive control that a lost
  large payload on a transport declaring no reliability passes the check. Since #421 the
  IFC-TRN-119 row has the same limit: it is `NotApplicable` against every real transport
  here and is exercised only by the memory plants (see the section below).
- A sealing transport that corrupts a frame fails opening and shows up as a missing
  payload, which this check now forgives; the sealed-frame checks, not IFC-TRN-030, speak to
  that.

## IFC-TRN-118 and IFC-TRN-119, 2026-10-15 (#421)

The lead approved adding these checks to the suite (2026-10-15), against `spec/interfaces.md`
0.9 (PR #408, #396). Refs #396, #392, #421.

| Check | Evidence exercised |
|---|---|
| IFC-TRN-118 `declared-size-is-not-refused` | A payload of exactly the declared `max_payload_octets` octets is `taken`, for each kind the transport accepts: an envelope to a session, a receipt to a device, a presence record to a device through `send_presence`. A 16-octet payload of the same kind is passed first as a control. Delivery is not asked for. |
| IFC-TRN-118, sealing transport | A sealing transport carries only sealed frames (IFC-TRN-113) and a sealed payload is measured as its frame (§6.10). The check builds, with the core's sealing API, a frame of exactly the declared maximum for each of the three inner kinds, passes each raw, and requires `taken`. A small frame of each kind is the control. |
| IFC-TRN-119 `taken-is-delivered-when-reliability-declared` | Only for a transport that declares `reliability`. For each kind, three distinct payloads (16 octets, 4096 octets and the declared maximum, up to 1 MiB, less the suite's routing control on a sealing transport) are passed with a deadline a minute away to a subscribed destination. Every payload that came back `taken` must arrive, octet for octet. |

Reported honestly, not as a pass:

- IFC-TRN-118 is `NotApplicable` when the control is itself refused (a refusal at the
  maximum could not be put down to size; IFC-TRN-001 and IFC-TRN-031 fail then) and when the
  declared maximum is over 64 MiB, which the check will not allocate.
- IFC-TRN-119 is `NotApplicable` when `start` declares `reliability` absent, which is the
  case for every transport in this repository (the memory transport, the Zenoh transport and
  the sealing stand-in). It is also `NotApplicable` if a declaring transport takes none of the
  payloads.
- The three stop conditions of IFC-TRN-119 (the deadline passes first, the destination is not
  reachable, the transport is shut down or restarted first) relax the rule and are not
  exercised: the check runs where none applies. It does not script a link that loses copies
  under a retransmitting transport, nor a restart across a retransmission; that needs a
  scripted carrier on a transport that really declares `reliability` and stays open for G1
  (#62) and F10 (#59). Appendix A of `spec/interfaces.md` still names those cases as
  `TODO(fixture)`.

Planted breaches in `transports/memory/tests/contract.rs`:

- `UnderSize` declares 65536 octets and returns `not-taken` above 32768. It fails
  IFC-TRN-118 on all four memory media (and IFC-TRN-030, which publishes the maximum as an
  envelope).
- `UnderSizeNonEnvelope` refuses only a receipt and a presence record above 32768. An envelope
  of the maximum is taken and delivered, so no earlier row notices; IFC-TRN-118 is the only
  row that fails (besides IFC-TRN-003). The test asserts that.
- The sealing stand-in's `UnderCap` refuses a frame of exactly its declared 65590 octets
  and is caught as IFC-TRN-118 (and IFC-TRN-104).
- `DropsLarge` takes the maximum and loses it with `reliability` declared absent: it passes
  IFC-TRN-118, and IFC-TRN-119 is not applicable. `DropsLargeClaimsReliability` loses the same
  payload with `reliability` declared: IFC-TRN-119 fails it (and IFC-TRN-030), and IFC-TRN-118
  passes it because the payload was taken. `ClaimsReliabilityHonestly` declares `reliability`
  and loses nothing: IFC-TRN-119 passes, a pass and not a skip.
- The memory transport and the Zenoh transport (over a sealing binding, at 1048576 octets)
  pass IFC-TRN-118.

## Required carriage observations

A sealing harness must implement `Medium::sealing_observations()` with observations
captured from the implementation's carriage boundary, including received/emitted
headers and raw identifiers behind opaque carrier handles. Each `SealingObservation`
contains the actual frame octets, accompanying application values, and link/peer/carrier/
liveness identifiers. Include identifiers observed during start, subscription and
shutdown in the capture; do not strip them because the Rust carrier type is opaque.
Transport-owned framing constants may be excluded from application values only when
independent of payload, destination, deadline, device and session. Explain that mapping
and the capture location in the transport binding/harness documentation (IFC-TRN-112).

Each isolated medium snapshots its carriage capture on drop; `run` retains only
those audit records, preserving transport/medium lifetimes. A missing hook, empty capture
where frames were taken, or omitted/changed taken frame fails IFC-TRN-107/108. All
captured values are checked, not only the small metadata controls. The run-wide
identifier needles come from every actual `start` key and every raw/logical destination,
including the suite's type-only session control. Text and decoded raw octets are checked
against every captured identifier across all worlds and frame sizes. A
capability declaration alone cannot establish sealing conformance. `assert_conformant`
also refuses a report missing any mandatory sealing row, or having that row only as
`NotApplicable`. Always run the entire suite, not selected exported checks.

The observation interface is instrumentation, not a proof that its author captured
all native fields. Binding review must verify capture completeness and arbitrary
identifier derivations (hashes, encodings, hidden native fields). Wire/header values
and opaque carrier internals are invisible through `Transport` alone. The suite does
not claim traffic-metadata confidentiality or verification of every possible derived
identifier. The core conformance/security suites retain signing, agreement-key,
opening, recipient and opened-payload expiry checks.

## Transport deadline, interfaces revision 0.7 (IFC-TRN-034)

Lead approved revising the deadline checks to the merged `spec/interfaces.md` 0.7
(PR #385, Refs #377). IFC-TRN-034 now binds the sending end only: a transport does not
accept a payload at or after its `Deadline` and hands no copy to the carrier then, whether
as a first hand-over, a retransmission, a retry after a link failure or a replay after a
reconnection. A copy handed over in time may arrive late; the receiving core refuses it
on its own clock (SC-RCP-091, SEC-PRS-006, SEC-SEL-036; a receipt has no receiving-side
rule, §7.4 of the spec). The suite therefore never fails a
transport for a late arrival.

| Check | Evidence exercised |
|---|---|
| IFC-TRN-034 `no-accept-or-hand-over-at-or-after-deadline` | An envelope, a receipt and a presence record passed at the deadline and after it each get `not-taken` and none reaches the receiving end; in-time controls of all three kinds are carried. With fault control, a copy the network delays past its deadline is exercised but not asserted. Under a sealing declaration the simulated receiving core drops expired payloads, so only the `not-taken` half can fail; the arrival half is observed raw by `sealing-sender-expired-input`. |
| IFC-TRN-034 `no-hand-over-of-a-held-copy-after-deadline` | With `FaultControl::next_sender_hold`, a copy held at the sender beyond its deadline never arrives (the retransmission, retry and replay case); a copy held for less than its deadline does arrive (control). Not applicable, never a pass, without sender-hold control or when sealing is declared (the raw `sealing-sender-expired-input` row observes the sender then). |

Limits of the raw `sealing-sender-expired-input` row, which is all that observes the sender
of a sealing transport: it tries one envelope passed at the deadline only (not after it, and
no receipt or presence record), and it has no in-time held control. The sealed hold case on a
medium without sender-hold control is not observed at all.

Not observable neutrally yet, and recorded as `TODO(fixture)` for F10 with G1 and G3: the
replay after a reconnection, which needs a medium that can cut and restore a link, and the
hand-over timestamp of Appendix A's test note, which needs a scripted carrier on the real
transport. IFC-TRN-038 is a SHOULD with no fixture. IFC-TRN-117 (a binding document states
its clock-skew limit) is checked by reading the binding document, not by this suite.
The planted breaches in `transports/memory/tests/contract.rs` show each row failing:
`ExtendsDeadlines` fails the first row, and `ReplaysAfterDeadline` (which refuses late
payloads correctly but replays a copy it accepted) fails the held row alone.

## Default-tier stand-in and planted breaches

`transports/memory/tests/contract.rs` contains a sealing stand-in entirely in test
code, wrapping the unchanged memory transport. It runs the same complete suite on
manual-clock media with ordered/no-fault delivery and with scripted faults. No product
transport declares a new capability as a result of this revision.

The stand-in plants acceptance of each plain kind and plain `send_presence`, delivery
despite `NotTaken`, kind/destination/deadline side values, device/session identifier
leaks, missing/empty observations, acceptance over the frame cap, a below-floor cap,
changed inbound frames, ignored shutdown, deadline leaks only above 4096 octets,
false non-sealing declarations with plain delegation, malformed raw callbacks after
normal shutdown, sender retention beyond expiry, and text/raw-octet disclosures of the
actual third device's start key (with the small metadata controls staying clean). A positive late-network control
proves that expired frames really reach the raw receiver, then disappear before the
logical callback, with no accompanying deadline. Each is caught under its requirement id and IFC-TRN-003.
Deleting each mandatory sealing row from an otherwise passing report is also caught.
Existing memory breach tests still run unchanged.

Run the default gate on Windows:

```powershell
node scripts/local-ci.mjs --self-test
node scripts/local-ci.mjs
```
