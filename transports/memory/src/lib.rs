// SPDX-License-Identifier: Apache-2.0

//! The in-memory transport (#56, F7): a loopback implementation of the transport contract
//! of `spec/interfaces.md` §6, with no network, used by every CI-default test. It depends on
//! `oac-core` and the Rust standard library only.
//!
//! A [`MemoryNetwork`] is the medium. Each [`MemoryTransport`] is one endpoint: `start`
//! joins it to a network for one device key, and `shutdown` leaves. One delivery thread per
//! network hands copies to handlers, and one purge thread drops copies at their deadline.
//! Fault injection ([`faults`]) loses, duplicates,
//! delays and reorders copies for tests of [IFC-TRN-011], and a [`ManualClock`] makes
//! deadlines testable.
//!
//! ```
//! use std::sync::{Arc, mpsc};
//! use std::time::Duration;
//! use oac_core::ids::{KeyId, SessionId};
//! use oac_core::transport::*;
//! use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};
//!
//! let network = MemoryNetwork::new();
//! let transport = MemoryTransport::new();
//! let key = KeyId::parse(&"0".repeat(64)).unwrap();
//! let caps = transport.start(&key, MemoryConfiguration::wrap(&network)).unwrap();
//! assert!(caps.contract_violations().is_empty());
//!
//! let session = Destination::Session(SessionId::from_random_octets([1; 16]));
//! let (tx, rx) = mpsc::channel();
//! let tx = std::sync::Mutex::new(tx);
//! let _sub = transport
//!     .subscribe(&session, Arc::new(move |i: Inbound| {
//!         tx.lock().unwrap().send(i.payload).unwrap();
//!     }))
//!     .unwrap();
//! let deadline = Deadline::at(network.now() + Duration::from_secs(5));
//! let payload = Payload::new(PayloadKind::Envelope, b"{}".to_vec());
//! assert_eq!(transport.publish(&session, payload.clone(), deadline), PublishResult::Taken);
//! assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), payload);
//! ```
//!
//! # Transport binding document
//!
//! This section is the transport binding document that [IFC-TRN-090] requires for this
//! transport.
//!
//! **Addressing.** A `Destination` maps to nothing else: the network matches payloads to
//! subscriptions by comparing `Destination` values for equality. A `session` payload goes to
//! every subscription made for that session id, on any endpoint of the network, the
//! sender's own included ([IFC-TRN-002]). A `device` payload goes to the endpoint started
//! with that key id: a receipt to its subscriptions for its own device, a presence record
//! to its `watch_presence` handlers. A `device` subscription for any other key id is
//! refused (`NotLocalDevice`). Payload kinds follow Table 6.1: `publish` takes `envelope`
//! to a `session` and `receipt` to a `device`, `send_presence` takes `presence` to a
//! `device`; anything else is `not-taken`.
//!
//! **Declaration (Table 6.3) and its evidence.**
//!
//! | Member | Value | Evidence |
//! |---|---|---|
//! | `reliability` | absent | A copy is handed to a handler once, or dropped; nothing is retransmitted. |
//! | `persistence` | absent ([IFC-TRN-026]) | State lives in the network's memory only, and every copy is dropped when its sending or receiving endpoint leaves. |
//! | `offline_queueing` | absent ([IFC-TRN-026]) | A copy is bound, when taken, to the subscriptions that exist then, and to no later one; a copy bound to none is dropped when it comes due ([IFC-TRN-036]). |
//! | `ordering` | present only when the fault injector preserves order ([`FaultInjector::preserves_order`]) | Copies are delivered by one thread in order of due instant, then of the order taken. [`NoFaults`] and [`FixedDelay`] keep the order passed; any other injector declares it absent. |
//! | `multicast_discovery` | absent | Endpoints join a network that the caller hands them; there is no discovery. |
//! | `routing_federation` | absent | One network, no relays. |
//! | `reach` | as built: `local-only` (default) or `cross-implementation` | A `local-only` network refuses to start for a second device key, so it carries payloads within one implementation only. A `cross-implementation` network lets endpoints for several keys join, each standing for an implementation; the transport suite's two-sided tests use one. |
//! | `destination_restricted` | absent | Any endpoint can subscribe to any session id (refusing would reveal a subscription, [IFC-TRN-043]), so a payload is not kept to one device's holder. [IFC-TRN-081] therefore keeps presence records off this transport for another implementation. |
//! | `max_payload_octets` | 65536 by default, as built otherwise, never less ([IFC-TRN-023]) | A larger payload is `not-taken`. |
//!
//! **What it holds** ([IFC-TRN-033] to [IFC-TRN-036]). A copy is in flight from `publish`
//! or `send_presence` until a handler has it or it is dropped (`spec/interfaces.md` §2.3).
//! It is dropped when its deadline comes ([IFC-TRN-034]): a copy whose delay would reach the
//! deadline (or past every instant the platform represents) is never queued, a queued copy
//! is removed when the deadline comes, and none is handed over at or after it. Removal at
//! the deadline does not wait for the delivery thread: a separate purge thread drops
//! expired copies even while a handler runs, and on a manual clock `advance` drops them
//! before it returns. It is dropped when its sending or its receiving endpoint
//! leaves, so it never crosses a restart of either ([IFC-TRN-035]), and when its
//! subscription ends. `publish` refuses (`not-taken`) only a payload of which no copy can be
//! delivered: before `start` or after `shutdown`, a kind that does not fit its destination
//! or operation, an oversized payload, or a deadline already passed ([IFC-TRN-031]).
//!
//! **Not revealing subscriptions** ([IFC-TRN-043], [IFC-TRN-044]). `publish` returns
//! `taken` for every payload that passes the checks above, whether or not any subscription
//! matches; none of those checks reads a subscription. It queues one item per copy whether
//! the copy is bound to recipients or to none, and handlers run on the network's delivery
//! thread, never inside the publisher's call; so the publisher's call never waits for a
//! subscriber and returns the same result either way. `subscribe` never fails because
//! another endpoint subscribed to the same destination. Neither health nor any other
//! endpoint operation reports subscriptions. [`MemoryNetwork::in_flight`] and
//! [`MemoryNetwork::settle`] are views for whoever built the network (a test), not for an
//! endpoint's core.
//!
//! **[IFC-TRN-080].** Not declared, so not claimed (see `destination_restricted`).
//!
//! **Carrier loss** ([IFC-TRN-061], taken). A carrier handle names the link from one
//! endpoint to another; it is opaque, and a restarted endpoint has new links. When an
//! endpoint leaves, each endpoint that received a payload from it gets a `carrier-loss` for
//! that link on its `watch_presence` handlers.
//!
//! **Handlers** ([IFC-TRN-040], [IFC-TRN-060], [IFC-TRN-071]). The delivery thread calls
//! handlers on its own initiative; the core never asks for payloads. Once `shutdown`, or the
//! ending of a subscription, returns, the handlers concerned are not called again: each waits
//! for a running call to finish, unless it is made from inside that call. A panicking
//! handler, including one of several presence watchers, does not stop delivery to others:
//! each call is isolated.

pub mod faults;
mod network;
mod transport;

pub use faults::{FaultInjector, FixedDelay, NoFaults, PublishInfo, ScriptedFaults};
pub use network::{DEFAULT_MAX_PAYLOAD_OCTETS, ManualClock, MemoryNetwork, MemoryNetworkBuilder};
pub use transport::{MemoryConfiguration, MemoryTransport};
