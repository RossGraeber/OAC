// SPDX-License-Identifier: Apache-2.0

//! Planted breaches of `tests/stand_in.rs` that live in the suite's library.
//!
//! - `ForgesAttachment`: a connection handle the adapter makes itself ([IFC-ADP-013]). It
//!   lives here, which the static scan of a stand-in is not given, so that the dynamic check
//!   is what catches it there; the static scan catches the same call in `source.rs`'s own
//!   tests. (It was a `#[path]` module of `stand_in.rs` until the scan learned to follow
//!   `#[path]`, #324.)
//! - `ReportsNotNow`: the outcome a Codex adapter reports for a `thread/queue/add` that
//!   the app-server turned away, taken as "cannot take input now" ([SC-DLV-008]). The
//!   recorded archived refusal is `handoff-failed` ([SC-DLV-009], `spec/bindings/mcp.md`
//!   §8.2.1). Mutation M6 of the PR #346 review (#347).

use oac_core::adapter::{Attachment, Connection, HandOffOutcome};

/// A handle the core never issued.
pub fn forged_attachment() -> Attachment {
    Connection::accept(std::io::empty(), std::io::sink())
        .handle()
        .clone()
}

/// The outcome for a turned-away hand-off that reads every refusal as "not now".
pub fn turned_away_outcome() -> HandOffOutcome {
    HandOffOutcome::NotNow
}
