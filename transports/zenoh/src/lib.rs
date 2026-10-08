// SPDX-License-Identifier: Apache-2.0

//! The Zenoh reference transport (#62, G1): `oac_core::transport::Transport`
//! (`spec/interfaces.md` Table 6.4) over an in-process Zenoh peer, on the stable API's
//! synchronous `Wait` path.
//!
//! Every Zenoh type, identifier and concept stays inside this crate (C7 §2). The public
//! surface is [`PeerTransport`], [`PeerConfiguration`], [`DEFAULT_PARTITION`] and
//! [`MAX_PAYLOAD_OCTETS`]; no signature names a Zenoh type.
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
//! This section is the transport binding document that [IFC-TRN-090] requires, for the
//! publish, subscribe and presence-carriage half (G1). G2 (#63) adds carrier loss, and G3
//! (#64) the TLS listener and LAN mode. It states the mapping that
//! `docs/planning/decisions/C7-zenoh-transport.md` §3 designed, as built.
//!
//! **Local mode.** One Zenoh session in `peer` mode per started transport, in-process, with
//! no router (C7 §5). Every listener is bound to `127.0.0.1`. Peers find each other by
//! multicast scouting (the default, [`PeerConfiguration::local`]), or by a fixed loopback
//! rendezvous port with scouting off ([`PeerConfiguration::rendezvous`], the G3 fallback).
//! Only the `transport_tcp` link is compiled in. No `unstable` or `shared-memory` feature
//! is used.
//!
//! **Addressing.** A `Destination` maps to the key expression `oac/1/<partition>/<digest>`
//! (`addressing.rs`). `<digest>` is 128 bits of SHA-256 over a kind tag and the session id
//! or key id, as 32 hex digits. It is one-way: a key expression on the wire, in a log or in
//! routing state does not yield the session id, and a session id, being 128 CSPRNG bits,
//! cannot be guessed from a pattern (C7 §3). `<partition>` separates installs and test
//! media ([`PeerConfiguration::with_partition`]). There is no group or room key: a
//! payload is put on exactly one destination's key.
//!
//! **Kinds (Table 6.1).** `publish` takes `envelope` to a `session` and `receipt` to a
//! `device`; `send_presence` takes `presence` to a `device`. Any other pairing, a payload
//! above [`MAX_PAYLOAD_OCTETS`], a deadline already reached, or a transport that is not
//! started is `not-taken`. Each payload travels as one frame (`frame.rs`): a 27-octet
//! header (kind, expiry, sending link) and then the octets exactly as passed
//! ([IFC-TRN-030]).
//!
//! **Delivery.** A `session` payload goes to every subscription for that session id on
//! every peer of the partition, the sender's own included ([IFC-TRN-002]). A `receipt`
//! goes to the subscriptions for the local device of the peer started with that key id; a
//! `device` subscription for any other key id is refused (`NotLocalDevice`). A presence
//! record goes, whole, to the `watch_presence` handlers of the peer started with the named
//! key id, and to no other peer's handlers ([IFC-TRN-050]). Handlers are called from
//! Zenoh's own threads, on the transport's initiative ([IFC-TRN-040], [IFC-TRN-060]);
//! nothing polls.
//!
//! **Non-disclosure of subscriptions ([IFC-TRN-043], [IFC-TRN-044]).** Each started
//! transport declares exactly one native subscriber, on `oac/1/<partition>/*`, at `start`,
//! and keeps it until `shutdown`. `subscribe` and ending a subscription change only a
//! local table. So what a peer declares, and the routing state other peers hold, are the
//! same whichever destinations it has subscriptions for, and no other implementation can
//! tell whether a subscription exists. `publish` puts the frame whatever the subscriptions
//! are: its result depends only on the checks under "Kinds" and on the native put
//! succeeding. The cost is that every peer of the partition receives every frame put on
//! it, and drops those for destinations it has no subscription for. What another peer can
//! observe is the traffic: that a frame of some size went to a key expression at some time.
//! Envelope content is not confidential from peers of the same partition; the envelope
//! signature, not the transport, is the authenticity proof (C7 §7), and full
//! confidentiality is out of scope for this revision (`spec/security.md` §1.3).
//!
//! **Deadlines ([IFC-TRN-034]).** A `Deadline` is an instant on the sender's monotonic
//! clock. The sender refuses a payload whose deadline has passed, and carries the time left
//! as a wall-clock expiry in the frame; the receiver drops a frame at or after its expiry.
//! On one host both read the same wall clock. Between hosts the bound is as good as the
//! hosts' clock agreement; LAN mode is G3's.
//!
//! **What is held ([IFC-TRN-033], [IFC-TRN-035], [IFC-TRN-036]).** Nothing is stored. A
//! frame exists only in Zenoh's send and receive queues while it is in flight; a frame for
//! a destination with no subscription is dropped on arrival; a restarted peer is a new
//! Zenoh session with a new peer id and an empty table.
//!
//! **Carrier handles ([IFC-TRN-012], [IFC-NEU-003]).** The handle on a received payload is
//! 128 bits of SHA-256 over the sending session's peer id: it names that link (a restart is
//! a new link) and carries no native identifier. No peer id, endpoint or key expression
//! reaches a health detail or an error.
//!
//! **Declaration (Table 6.3) and its evidence.**
//!
//! | Member | Value | Evidence |
//! |---|---|---|
//! | `reliability` | absent | Zenoh's reliable channel retransmits within one link only; nothing is retried across a lost link or reported back. |
//! | `persistence` | absent | Nothing is stored ([IFC-TRN-026]). |
//! | `offline_queueing` | absent | A frame for an unreachable destination is not held ([IFC-TRN-026], [IFC-TRN-036]). |
//! | `ordering` | absent | Not claimed: per-link order is Zenoh's behaviour, not a guarantee this transport tests across links and restarts. |
//! | `multicast_discovery` | present with [`PeerConfiguration::local`], absent with [`PeerConfiguration::rendezvous`] | Multicast scouting finds peers with no configured address (gate G3, `docs/planning/gates/G3-result.md`). |
//! | `routing_federation` | absent | Peer mode, no router, no relaying between networks. |
//! | `reach` | `cross-implementation` | Every peer of the partition on the host. |
//! | `destination_restricted` | absent | Every peer of the partition receives every frame (above), so [IFC-TRN-080] is not claimed, and [IFC-TRN-081] keeps other implementations' presence records off this transport until G3. |
//! | `max_payload_octets` | 1048576 | The contract suite carries a payload of exactly this size. |
//!
//! **Carrier loss.** None reported in G1 ([IFC-TRN-061] is optional); G2 maps liveliness
//! tokens to it (C7 §4).

mod addressing;
mod config;
mod frame;
mod gate;
mod presence;
mod transport;

#[cfg(test)]
mod tests;

pub use config::{DEFAULT_PARTITION, PeerConfiguration};
pub use transport::{MAX_PAYLOAD_OCTETS, PeerTransport};
