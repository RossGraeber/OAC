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
//! **Not for real traffic yet.** Until per-recipient frame encryption (the spec PR #367) and
//! the core's sealing work that uses it have landed, this transport must not carry real
//! traffic: every peer of a partition, and the rendezvous holder, can read every frame
//! (PR #364, lead's condition of 2026-10-09; C7 §3 dated note).
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
//! **Addressing.** A `Destination` maps to the key expression `oac/1/<partition>/<digest>`
//! (`addressing.rs`). `<digest>` is 128 bits of SHA-256 over a kind tag and the session id
//! or key id, as 32 hex digits. It is one-way: a key expression on the wire, in a log or in
//! routing state does not yield the session id, and a session id, being 128 CSPRNG bits,
//! cannot be guessed from a pattern (C7 §3). `<partition>` separates installs and test
//! media ([`PeerConfiguration::with_partition`]). There is no group or room key: a
//! payload is put on exactly one destination's key.
//!
//! The digest is **unkeyed**, and the same in every partition: only the partition chunk of
//! the key expression differs. So an observer of the traffic links every frame for one
//! session, in any partition, to every other, and anyone who already holds a session id can
//! recognise its frames. The digest hides the id; it does not make a session's traffic
//! unlinkable.
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
//! key id, and to no other peer's handlers ([IFC-TRN-050]). Handlers are called on the
//! transport's own dispatch thread, on its initiative ([IFC-TRN-040], [IFC-TRN-060]);
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
//! **Timing.** Receiving and handing over are separate:
//!
//! - Zenoh's receive callback only decodes a frame, drops it if it is expired or if this
//!   peer has no consumer for it, and otherwise queues a copy. No handler runs on Zenoh's
//!   thread, so a slow or stalled handler never holds up a link, and so never slows the
//!   sender. The work the callback does for a frame this peer has a consumer for (a copy
//!   and a queue push) is local and does not reach the sender: it adds no timing difference
//!   another implementation can observe.
//! - One dispatch thread per transport hands queued frames to handlers, in arrival order.
//!   The queue is bounded (1024 frames, 16 MiB of payload). A frame that arrives while it is
//!   full is dropped, which `reliability` absent allows. A handler that stalls holds up
//!   only its own transport's later handlers, and then frames for that transport are
//!   dropped once the queue fills.
//! - `publish` uses Zenoh's blocking congestion control, so a payload is not dropped just
//!   because a send queue is momentarily full. The cost: if a peer's link stops draining (a
//!   stalled or hostile local process), `publish` can block for up to 1 s, after which Zenoh
//!   closes that link (`wait_before_close`, set in `config.rs`). This is the same whether or
//!   not the destination is subscribed anywhere ([IFC-TRN-044]); it depends only on the
//!   links, which every peer of the partition holds alike.
//!
//! **Deadlines ([IFC-TRN-034]).** A `Deadline` is an instant on the sender's monotonic
//! clock. The sender refuses a payload whose deadline has passed, and carries the time left
//! as a wall-clock expiry in the frame; the receiver drops a frame at or after its expiry,
//! on arrival and again on the dispatch thread. On one host both read the same wall clock,
//! so the check is exact up to a step of that clock (a backwards step lets a frame in
//! flight live longer by the step). Between hosts it is not safe as it stands: a receiver
//! whose clock runs `d` behind the sender's can hand a frame over up to `d` after its
//! deadline, which breaks [IFC-TRN-034]; a receiver `d` ahead drops frames early, which is
//! safe. LAN mode (G3, #64) must bound this before it carries a frame between hosts: for
//! example, carry the time left instead and subtract a stated transit allowance, or
//! subtract a declared skew bound at the sender and refuse payloads with less time left than
//! that bound (C7 §6 dated note; `11-risks.md` row 77).
//!
//! **What is held ([IFC-TRN-033], [IFC-TRN-035], [IFC-TRN-036]).** Nothing is stored. A
//! frame exists only in Zenoh's send and receive queues and this transport's bounded
//! dispatch queue while it is in flight; a frame for a destination with no subscription is
//! dropped on arrival; a restarted peer is a new Zenoh session with a new peer id, an empty
//! table and an empty queue.
//!
//! **Carrier handles ([IFC-TRN-012], [IFC-NEU-003]).** The handle on a received payload is
//! the 16 octets the *sending* peer wrote into the frame header: for this transport, 128
//! bits of SHA-256 over its own session's peer id. Nothing checks them. Any peer of the
//! partition can write any value there, including another peer's. So the handle names a
//! link only as far as the senders are honest: it is fit for grouping what one sender
//! sent, and unfit for attributing anything, carrier loss included. G2 (#63) must derive
//! carrier loss from what the receiving transport itself observes (its own link or
//! liveliness events), never from a handle a frame carried (`11-risks.md` row 76). The
//! handle carries no native identifier, and no peer id, endpoint or key expression reaches
//! a health detail or an error.
//!
//! **Declaration (Table 6.3) and its evidence.**
//!
//! | Member | Value | Evidence |
//! |---|---|---|
//! | `reliability` | absent | Zenoh's reliable channel retransmits within one link only; nothing is retried across a lost link or reported back, and the dispatch queue drops when full. |
//! | `persistence` | absent | Nothing is stored ([IFC-TRN-026]). |
//! | `offline_queueing` | absent | A frame for an unreachable destination is not held ([IFC-TRN-026], [IFC-TRN-036]). |
//! | `ordering` | absent | Not claimed: per-link order is Zenoh's behaviour, not a guarantee this transport tests across links and restarts. |
//! | `multicast_discovery` | absent | Scouting is off; peers meet at a configured loopback port. |
//! | `routing_federation` | absent | The in-process relay links loopback sessions on one host only; nothing is relayed between networks. |
//! | `reach` | `cross-implementation` | Every transport of the partition on the host, through the loopback rendezvous. |
//! | `destination_restricted` | absent | Every peer of the partition receives every frame (above), so [IFC-TRN-080] is not claimed, and [IFC-TRN-081] keeps other implementations' presence records off this transport until G3. |
//! | `max_payload_octets` | 1048576 | The contract suite carries a payload of exactly this size. |
//!
//! **Carrier loss.** None reported in G1 ([IFC-TRN-061] is optional); G2 maps liveliness
//! tokens to it (C7 §4), under the rule in "Carrier handles" above.

mod addressing;
mod config;
mod frame;
mod gate;
mod presence;
mod transport;

#[cfg(test)]
mod tests;

pub use config::{DEFAULT_PARTITION, DEFAULT_RENDEZVOUS_PORT, PeerConfiguration};
pub use transport::{MAX_PAYLOAD_OCTETS, PeerTransport};
