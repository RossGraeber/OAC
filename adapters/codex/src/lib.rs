// SPDX-License-Identifier: Apache-2.0

//! The Codex App Server adapter: its client half (G6, #67).
//!
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
//! **Not yet here.** This crate does not implement `ProviderAdapter` yet: hand-off is G7
//! (#68, through [`client::CodexClient::queue_add`]), and the attachment of a Codex session,
//! its MCP connection paired with a thread under `spec/bindings/mcp.md` §4.5, is G8 (#69).
//! The adapter contract suite runs against the adapter once it implements the trait, from
//! `tests/contract.rs`.

pub mod client;
pub mod events;
pub mod schema;
pub mod shim;
pub mod threads;
pub mod ws;

pub use client::{ClientError, CodexClient, Diagnostics, EventHandler, Options};
pub use events::{CodexEvent, CompletedItem};
pub use threads::{ThreadKey, ThreadState, ThreadView, TurnEnd};
