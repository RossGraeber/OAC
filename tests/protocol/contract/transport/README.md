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
Gate S4 contract-suite baseline: **90877c575de8dc43aaffdc9b7ec1d47afd663050** (implementation commit;
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
may hand over late network arrivals under IFC-TRN-034. A separate raw sender check
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
