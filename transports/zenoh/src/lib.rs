// SPDX-License-Identifier: Apache-2.0

//! The Zenoh reference transport (#62, G1): `oac_core::transport::Transport`
//! (`spec/interfaces.md` Table 6.4) over an in-process Zenoh peer, on the stable API's
//! synchronous `Wait` path.
//!
//! Every Zenoh type, identifier and concept stays inside this crate (C7 §2). The public
//! surface is [`PeerTransport`], [`PeerConfiguration`], [`CertificateFiles`],
//! [`PairingAuthority`], [`IssuedCertificate`], [`DEFAULT_PARTITION`],
//! [`DEFAULT_RENDEZVOUS_PORT`] and [`MAX_PAYLOAD_OCTETS`]; no signature names a Zenoh type.
//! `PeerTransport::identifiers_for_leak_checks` exists only with the `test-support`
//! feature, which only this crate's own tests turn on.
//!
//! ```no_run
//! use oac_core::ids::KeyId;
//! use oac_core::transport::Transport;
//! use oac_transport_zenoh::{PeerConfiguration, PeerTransport};
//!
//! let transport = PeerTransport::new();
//! let key = KeyId::parse(&"0".repeat(64)).unwrap();
//! let caps = transport.start(&key, PeerConfiguration::local().wrap()).unwrap();
//! assert!(caps.contract_violations().is_empty());
//! transport.shutdown();
//! ```
//!
//! # Transport binding document
//!
//! **Real local traffic is now permitted:** core-signed, per-recipient sealed payloads
//! may traverse the per-user pinned mTLS loopback relay. Core sealing (#370/#374) and
//! the relay authentication condition (#64, PR #364) are both implemented. This does
//! not declare Gate S4, live harness interoperability, or the two-host LAN leg passed.
//!
//! This section is the transport binding document that [IFC-TRN-090] requires, for the
//! publish, subscribe and presence-carriage half (G1). G2 (#63) adds carrier loss, and G3
//! (#64) the TLS listener and LAN mode. It states the mapping that
//! `docs/planning/decisions/C7-zenoh-transport.md` §3 designed, as built.
//!
//! **Local mode: loopback only, fail closed.** One Zenoh session per started transport,
//! in-process; no separately run router daemon (C7 §5). Every listener is bound to
//! `127.0.0.1`. Multicast scouting and gossip are **off**: transports meet at a fixed
//! loopback rendezvous port ([`PeerConfiguration::local`] uses [`DEFAULT_RENDEZVOUS_PORT`];
//! [`PeerConfiguration::rendezvous`] names another). The first transport to start listens
//! on it, with its session in Zenoh's `router` mode so that it relays; every later transport
//! opens its session in `client` mode, linked to that one alone (Zenoh peers do not relay,
//! and without gossip later peers would never link to each other). No session opens a
//! connection it was not configured with, so none links to a locator another process told
//! it about. If the first transport shuts down, the later ones are cut off until another
//! transport takes the port; they then reconnect to it. That is the cost of failing closed:
//! the first transport on a host is the relay for the others (in the daemon model of C2 the
//! host has one).
//!
//! **Per-user pinned relay authentication (#64).** One generated self-signed certificate
//! with IP SAN `127.0.0.1` and CN `oac-local` is stored under the OS user's OAC state
//! directory (`LOCALAPPDATA/OAC/relay-tls` on Windows, `HOME/.local/state/OAC/relay-tls`
//! on Unix). The owner-only directory holds one atomically published PEM bundle.
//! Existing state is never regenerated on connection failure. Both roles use that exact
//! certificate as the sole trust anchor and mTLS identity; Zenoh verifies certificates,
//! names and expiration. The same user's next holder reuses the pin. Another user's
//! certificate, plaintext Zenoh router, or arbitrary port holder is refused before
//! native sample traffic. A responding port is never retried as a competing holder.
//! Port denial of service, holder gaps and the unregistered fixed port remain; a process
//! with access to the user's secret or root/admin authority remains trusted by the OS.
//! Certificate names authorize transport links only, never OAC envelope signers.
//!
//! **LAN mode, explicit opt-in configuration.** `PeerConfiguration::lan` uses native
//! TLS peer mode with explicit IP endpoints and pairing-issued certificate files.
//! `PairingAuthority::issue` and `IssuedCertificate::install` are provisioning seams:
//! the caller must first authorize pairing and deliver credentials through that approved
//! provisioning path. They do not authorize themselves from an incoming message and
//! are not wired into a daemon/CLI pairing command yet (G9/G11). Use opaque TLS common
//! names, never OAC device/session ids. Keep the issuer on the provisioning host.
//! Every peer defaults to native `access_control` deny and permits only configured
//! certificate common names on its partition's frame key (`put`, `declare_subscriber`,
//! ingress and egress). A CA-valid but unlisted peer may establish a TLS link; Zenoh
//! drops its disallowed operations rather than rejecting the link. ACLs do not make
//! sealed payloads authentic: core signatures and authorization still govern delivery.
//! Authenticated peer messages remain untrusted instructions.
//!
//! Scouting and gossip default off. Explicit endpoints are the G3 fallback on every
//! platform. `with_lan_multicast(true)` enables native multicast scouting only in LAN
//! mode; gossip stays off. Native `transport/link/protocols=["tls"]` excludes plaintext
//! locators, including discovered ones. Both `transport_tcp` (native probes/tests) and
//! approved `transport_tls` are compiled; QUIC, unstable and shared memory are not.
//! LAN host reachability/scouting requires the unrun two-host leg in the integration
//! README; default tests exercise this configuration only on loopback.
//!
//! **Certificate lifetime and recovery (#384, review finding 3).** Local self-signed
//! certificates, pairing CAs and paired leaves retain rcgen 0.14.10 defaults:
//! 1975-01-01 00:00:00 UTC to 4096-01-01 00:00:00 UTC. This is a very long credential
//! lifetime, not an implemented renewal schedule. OAC sets no explicit validity and
//! performs no automatic regeneration/renewal on expiry, corruption, non-private
//! storage or failed connections. The valid/expired loopback control
//! `native_tls_refuses_expired_certificate_without_regenerating_state` verifies native
//! expiry refusal on join and unchanged stored material. Both roles enable Zenoh's
//! `close_link_on_expiration`; upstream source implements closure at peer-chain expiry.
//! Scheduled closure is source-verified, not a live timed-expiry test. Native TLS and
//! the host clock own these checks; OAC adds no expiry handshake or clock protocol.
//! Default-date source: <https://github.com/rustls/rcgen/blob/f4a3b165c467c070d5c74b86acc2c3f0a0be10ab/rcgen/src/certificate.rs>,
//! rcgen 0.14.10, cached first-party source inspected 2026-10-11. Zenoh expiry source:
//! <https://github.com/eclipse-zenoh/zenoh/blob/1.10.1/io/zenoh-link-tls/src/unicast.rs>,
//! version 1.10.1, cached first-party source inspected 2026-10-11.
//!
//! **Explicit local re-provisioning/rotation (planned outage).**
//! 1. Stop sends, shut down every local joiner and the live rendezvous holder, and
//!    disable automatic restarts. Confirm that the holder has released the port and
//!    no old session remains: file changes do not update credentials already in memory.
//! 2. Retire the old owner-only `relay-tls` directory offline; keep any required backup
//!    owner-only and inaccessible to running peers. Do not reset state while a holder
//!    is active or merely because a connection failed. Investigate privacy failures
//!    before deliberately replacing storage. Use a fresh absent state directory.
//! 3. Start one intended holder through normal provisioning. It creates a new key and
//!    complete bundle in private storage. Restart all joiners with that same directory;
//!    check native link establishment before resuming sends. Retire old backups under
//!    the operator's key-disposal policy. Certificate-only replacement with an old key
//!    is not revocation; this sequence makes no new exclusive-pin claim (finding 1).
//!
//! **Explicit LAN re-provisioning/rotation (planned group outage).**
//! 1. Coordinate all peers in the affected pairing trust group, stop sends and all live
//!    listeners/joiners, and disable restarts. Include offline peers in the rollout;
//!    leave an unupdated peer stopped. Live sessions retain loaded credentials.
//! 2. Through the authorized offline provisioning path, issue fresh keys/leaves with
//!    correct IP SANs and approved opaque CNs. For authority expiry, compromise or
//!    revocation, create a fresh `PairingAuthority` and reissue every group's leaf;
//!    replacing a leaf under the old CA does not revoke other old CA-valid leaves.
//! 3. Install each credential into a fresh owner-only directory (`install` refuses
//!    replacement). Update every peer's `CertificateFiles`, trusted roots, endpoints
//!    and CN allowlist together. Remove the old group's roots everywhere when retiring
//!    that authority; do not leave a dual-root transition for revocation. Retire old
//!    credential directories offline with owner-only backup/key-disposal handling.
//! 4. Restart intended listening peers, then their connecting peers using the new
//!    configuration. Verify native TLS links and allowed/denied CN operations before
//!    resuming sends. On failure keep traffic stopped and repair provisioning explicitly;
//!    never regenerate or trust a newly encountered peer automatically.
//!
//! This is operator coordination through existing provisioning/configuration seams,
//! not a custom rotation protocol. Daemon/CLI wiring and two-host validation remain
//! unimplemented/unrun; root changes do not assert the exclusive-pin condition awaiting
//! the lead's decision in review finding 1. TLS rotation does not rotate core signing
//! or agreement keys or change core authorization.
//!
//! Sources: Zenoh supported stable configuration / TLS / ACL implementation, version
//! 1.10.1, retrieved/inspected 2026-10-11:
//! <https://github.com/eclipse-zenoh/zenoh/blob/1.10.1/DEFAULT_CONFIG.json5>,
//! <https://github.com/eclipse-zenoh/zenoh/blob/1.10.1/io/zenoh-link-tls/src/utils.rs>,
//! <https://github.com/eclipse-zenoh/zenoh/blob/1.10.1/io/zenoh-link-tls/src/unicast.rs>,
//! <https://github.com/eclipse-zenoh/zenoh/blob/1.10.1/zenoh/src/net/routing/interceptor/access_control.rs>.
//!
//! The evidence that local mode reaches nothing beyond loopback is
//! `tests/peer_transport.rs` `local_mode_reaches_nothing_beyond_loopback`: a LAN-address
//! probe with multicast scouting on hears nothing and is never linked. The probe itself
//! listens beyond loopback, so the test is a **deliberate opt-in**, not lost coverage: it
//! runs with `OAC_TEST_LAN=1` (`node scripts/local-ci.mjs --tier lan`, which sets it for its
//! own child) and prints SKIPPED otherwise. The default tier's listeners are loopback only,
//! which `scripts/check-test-listeners.mjs` enforces; on Windows a non-loopback listener
//! raises a firewall prompt for every rebuilt test executable.
//!
//! **Sealed carriage (`sealing: true`, IFC-TRN-100..113).** Every frame uses exactly
//! `oac/1/<partition>/frames`, independent of destination, kind, deadline, device and
//! session. The partition is a hash of the configured install label only. The core's
//! sealed octets are the Zenoh sample payload directly; sample boundaries provide framing.
//! There is no OAC header, kind tag, expiry, destination digest or sender handle. Every
//! publication uses the fixed `Encoding::ZENOH_BYTES`; no attachment, timestamp or source
//! info is set; automatic timestamps are explicitly disabled, including on the relay.
//! Zenoh's native framing and random peer/link identifiers remain. None is
//! derived from the local key id or any session id. Size, timing and native link correlation
//! remain observable; this is not traffic-metadata confidentiality.
//!
//! **Admission and delivery.** Both send operations refuse every non-`sealed` kind,
//! session destinations, frames above 1 MiB, passed deadlines and stopped transports
//! (`not-taken`). Every admitted inbound sample goes unchanged, as `sealed`, to all
//! subscriptions for this transport's local device, including the sender's own stream.
//! Other-device subscriptions are refused; session subscriptions stay local and receive
//! no frames. Sealed presence uses this same device stream through `publish`; the core
//! opens and dispatches the inner record. G1's `watch_presence` reports nothing.
//! The core silently discards frames that cannot open, target another device or are late.
//! Sealing hides content, never authenticates; signatures remain core-owned. Authenticated
//! peer messages remain untrusted instructions and may contain prompt injection.
//!
//! **Hidden subscriptions (IFC-TRN-043/044).** Each start declares one native subscriber
//! on the fixed frame key until shutdown. Local subscription changes never affect native
//! interest or admission. Every peer receives every frame in its partition, regardless
//! of which local sessions listen. The binding tests exercise native interest with sealed
//! samples; the unchanged shared suite exercises sealed presence and every other check.
//!
//! **Native queue and isolation.** Zenoh `RingChannel` owns receive queuing and overflow:
//! at most 16 frames, dropping the **oldest** when full (formerly drop-newest). The
//! native `low_pass_filter` admits ingress `put` samples on `oac/**` only when payload
//! plus attachment is at most 1 MiB. The receive callback checks fixed encoding before
//! copying payload octets into the ring; it contains no custom size predicate.
//! `transport/link/rx/max_message_size` instead bounds the fragmented serialized message
//! (including protocol overhead), and does not filter unfragmented samples; it cannot
//! express the exact inclusive payload cap. Zenoh has no byte-capacity handler, so the
//! native 1 MiB per-frame cap times 16 gives a 16 MiB queued-payload bound;
//! one dispatching frame adds at most 1 MiB, plus
//! transient callback copies and native receive buffers. Frame storage is owned `Vec<u8>`
//! so the ring does not retain larger native backing buffers. These are payload bounds,
//! not a bound on Zenoh's link buffers or process RSS. One dispatch thread calls core
//! handlers behind lifecycle gates. A stalled handler holds up only its own transport's
//! callbacks; the ring continues draining the link and other peers keep receiving.
//!
//! **Cross-host clocks (#64, C7 §6).** No custom clock synchronization is provided.
//! The sender uses monotonic admission; after decrypting, the receiving core applies
//! SEC-SEL-036 using its own wall clock. A slow receiving clock can extend effective
//! validity by its skew, and a fast one can discard early. Operators must bound host
//! clock skew operationally; no measured cross-host skew bound is claimed. Zenoh has
//! no publication TTL: accept native retention and receiver late discard (#377); the
//! frozen sender requirement's residual remains for the lead (risk row 77).
//!
//! **Deadlines (IFC-TRN-034), review round 1.** Sender admission checks the monotonic
//! deadline before copying and again immediately before resolving native `put`.
//! All publications use fixed `CongestionControl::Drop`, priority `Data` and `express=true`:
//! no adaptive batching delay and no deadline-dependent observable QoS. Zenoh's native
//! `wait_before_drop=1000` and `max_wait_before_drop_fragments=0` (microseconds) give
//! batch-refill waiting a flat 1 ms budget, without fragment extensions. Zenoh preallocates its 16-batch data
//! pool (`transport/link/tx/queue/size/data=16`,
//! `transport/link/tx/queue/allocation/mode="init"`) at link initialization:
//! about 1 MiB of native batch storage per data queue, plus native metadata, other
//! priority queues and TCP buffers. There is no OAC send queue. With the default two
//! batches, zero/1 ms waits dropped an uncongested 1 MiB publication; 5 ms passed once
//! but failed the workspace's unchanged IFC-TRN-030 check. With 16 batches, zero wait
//! passed once but occasionally dropped the last fragment: a 1 MiB frame exceeds
//! 16 x 65535 batch octets even before native headers. Native capacity plus one short
//! refill budget supplies normal fragmentation; there is no increasing wait per
//! fragment. Fanout can visit multiple
//! links synchronously.
//! Native drops still return successful `put`; `Taken` means ownership, not delivery.
//! The 1 s `wait_before_close` remains for native blocking control traffic, not OAC frames.
//!
//! **Residual, not proven IFC-TRN-034 conformance.** A native wait budget is not a
//! wall-clock retention bound: serialization, native mutex contention and OS scheduling
//! add time. Serialized TX batches, TCP buffers and relay copies carry no TTL and cannot
//! be revoked by the stable API. No finite maximum deadline overrun is proved.
//! `short_deadline_does_not_wait_for_a_stalled_native_subscriber` reproduces the review's
//! 600 ms stalled plain subscriber, actual 1 MiB HPKE frames and 20 ms deadlines, for
//! relay and client senders. It separately reports maximum publish duration/call overrun
//! and late callback observation (the latter includes receiver scheduling, and cannot
//! locate a copy in sender TX vs TCP vs receiver buffers). Windows measurements are
//! recorded in `11-risks.md` row 77. Its 100 ms regression threshold catches the reviewed
//! 200-291 ms block, not a hard deadline guarantee. No deadline travels beside the frame;
//! receiving expiry is solely the core's encrypted-payload check (SEC-SEL-036).
//! Nothing is retried or stored by OAC across a lost link or restart.
//!
//! **Carrier handles.** Inbound frames share a receiver-local handle: a hash of this
//! transport's random native session id. It is never transmitted and groups the local
//! frame stream, not senders. No sending peer chooses it; no device/session id derives
//! it. G2 (#63) must use receiving-side native liveliness for carrier loss, never infer
//! sender identity from this stream handle. Peer ids and endpoints reach no neutral health
//! detail or error. G1 reports no carrier loss.
//!
//! **Why OAC still owns custom code (#377).** Zenoh has no first-holder rendezvous
//! election, byte-bounded receive handler, neutral payload-size/error/health semantics,
//! monotonic publication deadline, or core-handler shutdown gate (IFC-TRN-071). OAC keeps
//! those admission, local subscription and lifecycle checks. Native subscriptions cannot
//! express hidden session interest while delivering only to the device stream, so local
//! subscription bookkeeping stays OAC-owned.
//! Partition-label hashing supplies install isolation and a syntax-safe fixed key;
//! receiver-local hashing maps native identity to the neutral opaque carrier type.
//! Zenoh provides neither OAC's partition-label contract nor its neutral carrier type.
//! Encoding admission remains OAC-owned: the native size filter has no encoding predicate.
//! Outbound size admission remains OAC-owned to return neutral `NotTaken`; the native
//! silent filter cannot report that result. Owned frame copies keep native backing
//! buffers out of the bounded ring. Zenoh owns ingress size filtering, routing,
//! reconnection, congestion control, sample boundaries, fragmentation, overflow and
//! keep-alives. Ephemeral listeners use `:0`;
//! Zenoh allocates them without OAC's former bind/drop/rebind race.
//!
//! Stable API sources, version 1.10.1, inspected in cached first-party source 2026-10-10:
//! <https://github.com/eclipse-zenoh/zenoh/blob/1.10.1/zenoh/src/api/handlers/ring.rs>
//! (`IntoHandler`, `RingChannel`, drop-oldest),
//! <https://github.com/eclipse-zenoh/zenoh/blob/1.10.1/zenoh/src/api/builders/publisher.rs>
//! (`put`, fixed encoding; no TTL), and
//! <https://github.com/eclipse-zenoh/zenoh/blob/1.10.1/DEFAULT_CONFIG.json5>
//! (ephemeral listeners, TX wait settings and `low_pass_filter`),
//! <https://github.com/eclipse-zenoh/zenoh/blob/1.10.1/zenoh/src/net/routing/interceptor/low_pass.rs>
//! (inclusive payload + attachment bound),
//! <https://github.com/eclipse-zenoh/zenoh/blob/1.10.1/io/zenoh-transport/src/common/pipeline.rs>
//! (native waits, fragmentation and express), and
//! <https://github.com/eclipse-zenoh/zenoh/blob/1.10.1/io/zenoh-transport/src/unicast/universal/link.rs>
//! (TX batches have no publication expiry).
//!
//! **Contract observation mapping (IFC-TRN-112).** With `test-support`, captures at the
//! actual outgoing `put` boundary and incoming sample callback record exact frame bytes,
//! key, encoding and receiver-local carrier bytes. The harness checks that every key and
//! encoding is the binding's fixed constant, excluding those destination-independent
//! constants from application values. Incoming attachments and timestamps are captured
//! as accompanying values, so unexpected additions fail the audit; the expected list is empty.
//! Start/shutdown snapshots retain native peer/router ids, configured listener/rendezvous and key
//! strings, even after restart. No attachment, source info or liveness signal is emitted.
//! The contract audit checks every exercised medium and frame size. Native framing is
//! delegated to Zenoh; capture is at its sample API, not a packet trace. Actual ephemeral
//! locators are not exposed by the stable API; configured `:0` is captured. Binding review
//! must still check derivations and native capture completeness (suite README).
//!
//! **Declaration and evidence (Table 6.3).**
//!
//! | Member | Value | Evidence |
//! |---|---|---|
//! | `sealing` | present | Direct frame carriage; unchanged sealing-aware contract suite. |
//! | `reliability` | absent | Native ring drops oldest on overflow; no retry across lost links. |
//! | `persistence`, `offline_queueing` | absent | No durable store or OAC retry queue. |
//! | `ordering` | absent | No cross-link/restart ordering guarantee claimed. |
//! | `multicast_discovery` | local absent; LAN configurable | Scouting explicit opt-in; gossip off. |
//! | `routing_federation` | absent | Loopback relay only. |
//! | `reach` | `cross-implementation` | Pinned local relay or configured paired TLS LAN peers. |
//! | `destination_restricted` | absent | All peers receive all frames; sealing satisfies IFC-TRN-081. |
//! | `max_payload_octets` | 1048576 | Contract suite exercises exact cap and one-octet overflow. |

mod addressing;
mod certificates;
mod config;
mod gate;
mod presence;
mod transport;

#[cfg(test)]
mod security_tests;
#[cfg(test)]
mod tests;

pub use certificates::{CertificateFiles, IssuedCertificate, PairingAuthority};
pub use config::{DEFAULT_PARTITION, DEFAULT_RENDEZVOUS_PORT, PeerConfiguration};
pub use transport::{MAX_PAYLOAD_OCTETS, PeerTransport};

#[cfg(feature = "test-support")]
pub use transport::CarriageObservation;
