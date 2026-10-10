// SPDX-License-Identifier: Apache-2.0

//! The Claude Code Channels adapter (G4, #65): `ProviderAdapter` for the Claude Code
//! channel path (**research preview**; Claude Code version floating, minimum `v2.1.282`,
//! `docs/planning/PINS.md`).
//!
//! - [`ClaudeAdapter`] implements `oac_core::adapter::ProviderAdapter` (`spec/interfaces.md`
//!   Table 5.2). Its binding document is `spec/bindings/mcp.md` §8.1 with §4 and §5.
//! - [`channel`] is the shim boundary of C11 (07 §4(b)): the only module that names the
//!   research-preview surface.
//! - The MCP server is `rmcp` over the core-issued connection, newline-delimited JSON
//!   (G-7 §3.1); the tool surface is `oac-mcp-tools`, shared with the Codex adapter
//!   ([MCPB-TOOL-003]).
//!
//! What the adapter never does ([ADR-001 Boundary]): call a model API, read a credential,
//! scrape a terminal, poll for messages, or reach a transport. It routes every outbound
//! request through the core's request sink ([IFC-ADP-001], [IFC-ADP-003]) and signs,
//! verifies and binds nothing ([IFC-ADP-002], [IFC-ADP-007]). Provenance is the verified
//! envelope's, rendered as C6's five `meta` keys; content stays untrusted text ("authenticated
//! but untrusted", `oac-security-work` §3). A hand-off is reported as handed to the harness,
//! never as seen by the model.

pub mod adapter;
pub mod channel;
mod io;
mod server;

pub use adapter::ClaudeAdapter;
