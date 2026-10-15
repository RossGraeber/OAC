// SPDX-License-Identifier: Apache-2.0

//! The library half of the `oac` binary: what the binary constructs and hands to the core.
//! Nothing depends on this crate (`docs/planning/v0.1/07-repository-and-dependencies.md`
//! section 3); the library target exists so its parts are built and tested before a verb
//! wires them in.
//!
//! - [`keystore`]: the device-key stores, the operating system's credential store and the
//!   encrypted-file fallback (#52, F3; `docs/planning/decisions/C4-session-identity.md`
//!   §10-§11).
//! - [`daemon`]: the per-device daemon (G9, #70).
//! - [`hook`]: the `oac hook` handler that carries a Claude Code `SessionStart` input to the
//!   daemon as a native signal (G9, #70; `spec/bindings/mcp.md` §8.1).
//! - [`verbs`]: the verbs that exist so far and their dispatch.
//! - [`ipc`]: the local IPC between the daemon and its clients, with the peer authenticated by
//!   the operating system (G9, #70).
//! - [`relay`]: an authenticated IPC stream as a core `Connection`, bounded in both directions.
//! - [`shim`]: `oac mcp-shim`, the stdio byte relay a harness starts as its MCP server.
//! - [`peer`]: what the daemon observes about the process at the other end of a local
//!   connection (pairing key, harness working directory) from the operating system (G9, #70).

pub mod daemon;
pub mod hook;
pub mod ipc;
pub mod keystore;
pub mod peer;
pub mod relay;
pub mod shim;
pub mod verbs;
