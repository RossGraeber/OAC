// SPDX-License-Identifier: Apache-2.0

//! The Codex App Server adapter: its client half (G6, #67) and the internals of the issued-value
//! pairing (G8 slice S3, #69, #403).
//!
//! - [`adapter`]: [`CodexAdapter`], the `ProviderAdapter` (G8 slice S4): the carrier, the
//!   shim connections, the hand-off through `thread/queue/add` (G7), and the
//!   [`pairing::CallPort`] seam.
//! - [`frame`]: the C6 §5.0 frame a hand-off is written as.
//! - [`ws`]: RFC 6455 framing over the stream `cli/` opens (D8).
//! - [`client`]: JSON-RPC over it, the `initialize` handshake, thread subscriptions, and a
//!   fixed allowlist of methods the client may send.
//! - [`shim`]: the named, version-labelled compatibility shim for experimental methods
//!   (`thread/queue/add`), the only place any is named.
//! - [`schema`]: the message shapes, each tied to the vendored app-server schema at
//!   `rust-v0.161.0` and checked against it by `tests/schema.rs`.
//! - [`threads`]: threads and turns, named by keys of the adapter's own; native ids stay
//!   inside.
//! - [`events`]: notifications for served threads, with `item/completed` authoritative.
//! - [`mcp`]: the OAC MCP server for one Codex stdio connection, on `rmcp`. [`pairing`] is the
//!   pairing of that connection with a thread (`spec/bindings/mcp.md` §4.5), made of
//!   [`reveal`] (refused-call records and reveal detection), [`confirm`] (the confirmation
//!   engine) and [`serving`] (which threads the carrier serves while a connection is paired).
//!
//! **Approvals.** Server requests are counted and left unanswered. Another thread
//! subscriber (the TUI) can answer; the first answer resolves Codex's shared callback.
//! With this carrier as the only subscriber, a tool approval waits until a subscriber
//! answers or a turn-state change aborts it. Source-only at `rust-v0.161.0`, with no
//! approval fixture: see `adapters/codex/README.md`, "Approval evidence".
//!
//! **Boundaries ([ADR-001 Boundary]).** The adapter calls no model API, and never reads or
//! holds a Codex credential (`auth.json`, the keyring) or a rollout file: it speaks only the
//! documented app-server protocol, over a stream it is handed. No Codex crate is a
//! dependency (G-7 §2): it depends on `oac-core` only. Provider-specific types stay inside
//! this crate. It depends on `oac-core` only and routes every outbound message through the
//! core, never to a transport (`spec/interfaces.md` [IFC-ADP-001]).
//!

pub mod adapter;
pub mod client;
pub mod confirm;
pub mod events;
pub mod frame;
mod gate;
pub mod mcp;
mod mcp_io;
pub mod pairing;
pub mod reveal;
pub mod schema;
pub mod serving;
pub mod shim;
pub mod threads;
pub mod ws;

pub use adapter::{CarrierState, CodexAdapter};
pub use client::{ClientError, CodexClient, Diagnostics, EventHandler, Options};
pub use events::{CodexEvent, CompletedItem, McpCallStart};
pub use pairing::{Answer, CallPort, CodexPairing, Config, PairingDiagnostics, Reveal};
pub use threads::{ThreadKey, ThreadState, ThreadView, TurnEnd};
