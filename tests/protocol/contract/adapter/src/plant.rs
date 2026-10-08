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
//!
//! Harness breaches (#351, from the PR #348 review): a harness that drops a binding's
//! checks through what it supplies, rather than through what the adapter does.
//!
//! - `WeakensProfile`: the binding's own profile with its refusal code and rules removed.
//! - `ClaimsOtherBinding`: the other binding's profile.
//! - `InventsProfile`: a profile no binding has, naming no surface and no rule.
//! - `RefusalAsGap`: `refuse_hand_offs` reports the refusal as unsupported.
//! - `TurnAsGap`: `start_turn` reports a turn as unsupported.
//! - `RequestsAsGap`: `request` reports requests as unsupported.
//! - `NoSourceFiles`: `source_files` gives nothing to scan.
//! - `NoNativeIds`: `native_ids` gives nothing to look for (PR #355 review R1).
//! - `TurnNoop`: `start_turn` returns Ok without starting a turn (PR #355 review R2).

use std::path::PathBuf;

use oac_core::adapter::{Attachment, Connection, HandOffOutcome};

use crate::{BINDINGS, Gap, Profile, Step};

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

/// `profile` with no refusal code and no binding rule: the [SC-DLV-009] row and every
/// rule row (the `MCPB-CDX-*` rows for Codex) would drop out.
pub fn weakened_profile(profile: Profile) -> Profile {
    Profile {
        turned_away_code: None,
        rules: &[],
        ..profile
    }
}

/// The suite's profile of a binding other than `profile`'s.
pub fn other_binding(profile: Profile) -> Profile {
    *BINDINGS
        .iter()
        .find(|p| p.name != profile.name)
        .expect("more than one binding")
}

/// A profile no binding has: no surface, no holding hand-off, no rule, no refusal, and no
/// mandatory step.
pub fn invented_profile() -> Profile {
    Profile {
        name: "none",
        surface_operations: &[],
        holding_hand_off: None,
        steering_operations: &[],
        rules: &[],
        turned_away_code: None,
        runs_own_turns: false,
        makes_requests: false,
    }
}

/// What `refuse_hand_offs` returns instead of making the harness refuse.
pub fn refusal_as_gap() -> Gap {
    Gap::Unsupported("planted: the harness says it cannot turn a hand-off away".into())
}

/// What `start_turn` returns instead of starting a turn.
pub fn turn_as_gap() -> Gap {
    Gap::Unsupported("planted: the harness says it cannot start a turn".into())
}

/// What `request` returns instead of making the request.
pub fn requests_as_gap() -> Gap {
    Gap::Unsupported("planted: the harness says it makes no requests".into())
}

/// What `source_files` returns instead of the adapter's sources.
pub fn no_source_files() -> Vec<PathBuf> {
    Vec::new()
}

/// What `native_ids` returns instead of the ids the fake knows.
pub fn no_native_ids() -> Vec<String> {
    Vec::new()
}

/// What `start_turn` returns instead of starting a turn: success, with nothing done.
pub fn turn_noop() -> Step<()> {
    Ok(())
}
