// SPDX-License-Identifier: Apache-2.0

//! The Zenoh reference transport (#62, G1): `oac_core::transport::Transport`
//! (`spec/interfaces.md` Table 6.4) over an in-process Zenoh peer, on the stable API's
//! synchronous `Wait` path.
//!
//! Every Zenoh type, identifier and concept stays inside this crate (C7 §2). The public
//! surface is [`PeerTransport`], [`PeerConfiguration`], [`DEFAULT_PARTITION`],
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
//! **Not for real traffic yet.** Core sealing has landed (#370), and this binding now
//! declares `sealing`. The restriction remains until #64 also authenticates the relay
//! with a per-user pinned TLS certificate (lead's condition of 2026-10-09).
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
//! **The rendezvous holder is not authenticated (port squatting).** Whatever binds the
//! loopback rendezvous port first, or first after the holder exits, is the relay every
//! other transport on the host uses. Nothing proves it is an OAC transport. So:
//!
//! - a squatting program, run by any local user, receives every frame and can drop, delay
//!   or withhold any of them, silently (envelope signatures and replay protection still
//!   hold);
//! - any program on the port that is not a Zenoh session stops OAC from starting at all
//!   (`start` names the port and says it is held by something else);
//! - one process, possibly another OS user's, relays the whole host, since every install
//!   uses the same default port and partition;
//! - between holders, transports are cut off;
//! - [`DEFAULT_RENDEZVOUS_PORT`] (`17447`) is fixed and unregistered, so other software may
//!   use it.
//!
//! The lead ratified this design on 2026-10-09 on condition that G3 (#64) authenticates the
//! relay with a per-user pinned TLS certificate before OAC carries real traffic
//! (`11-risks.md` row 81; C7 §9 threat row; C7 §5 dated note).
//!
//! Multicast scouting was the earlier default; it reached
//! the LAN (PR #364 review, finding 1), and confining it to loopback cannot be guaranteed
//! with the stable configuration (see `config.rs`), so it is not used. Only the
//! `transport_tcp` link is compiled in. No `unstable` or `shared-memory` feature is used.
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
//! at most 16 frames, dropping the **oldest** when full (formerly drop-newest). The small
//! receive callback checks size and fixed encoding before copying admitted payload octets
//! into the ring. Zenoh has no byte-capacity handler, so the 1 MiB per-frame cap times 16
//! gives a 16 MiB queued-payload bound; one dispatching frame adds at most 1 MiB, plus
//! transient callback copies and native receive buffers. Frame storage is owned `Vec<u8>`
//! so the ring does not retain larger native backing buffers. These are payload bounds,
//! not a bound on Zenoh's link buffers or process RSS. One dispatch thread calls core
//! handlers behind lifecycle gates. A stalled handler holds up only its own transport's
//! callbacks; the ring continues draining the link and other peers keep receiving.
//!
//! **Deadlines (IFC-TRN-034).** Sender admission checks the monotonic deadline immediately
//! before native `put`. No deadline travels beside the frame; receiving expiry is solely
//! the core's encrypted-payload check (SEC-SEL-036). Zenoh has no per-publication TTL.
//! Blocking congestion control closes a stalled link after 1 s (`wait_before_close`),
//! but does not prove native queued copies cannot outlive a shorter deadline. This is an
//! open sender-side limitation for the lead under #377; no stalled-link deadline guarantee
//! is claimed. Nothing is retried or stored by OAC across a lost link or restart.
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
//! Zenoh owns routing, reconnection, sample boundaries, fragmentation, overflow and
//! keep-alives. Ephemeral listeners use `:0`;
//! Zenoh allocates them without OAC's former bind/drop/rebind race.
//!
//! Stable API sources, version 1.10.1, inspected in cached first-party source 2026-10-10:
//! <https://github.com/eclipse-zenoh/zenoh/blob/1.10.1/zenoh/src/api/handlers/ring.rs>
//! (`IntoHandler`, `RingChannel`, drop-oldest),
//! <https://github.com/eclipse-zenoh/zenoh/blob/1.10.1/zenoh/src/api/builders/publisher.rs>
//! (`put`, fixed encoding; no TTL), and
//! <https://github.com/eclipse-zenoh/zenoh/blob/1.10.1/DEFAULT_CONFIG.json5>
//! (ephemeral listeners and `wait_before_close`).
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
//! | `multicast_discovery` | absent | Scouting and gossip off. |
//! | `routing_federation` | absent | Loopback relay only. |
//! | `reach` | `cross-implementation` | Peers in one partition through the loopback relay. |
//! | `destination_restricted` | absent | All peers receive all frames; sealing satisfies IFC-TRN-081. |
//! | `max_payload_octets` | 1048576 | Contract suite exercises exact cap and one-octet overflow. |

mod addressing;
mod config;
mod gate;
mod presence;
mod transport;

#[cfg(test)]
mod tests;

pub use config::{DEFAULT_PARTITION, DEFAULT_RENDEZVOUS_PORT, PeerConfiguration};
pub use transport::{MAX_PAYLOAD_OCTETS, PeerTransport};

#[cfg(feature = "test-support")]
pub use transport::CarriageObservation;
