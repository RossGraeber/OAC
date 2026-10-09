// SPDX-License-Identifier: Apache-2.0

//! The named, version-labelled compatibility shim for experimental app-server methods (G6
//! acceptance item 5; conflict-register C11; `docs/planning/v0.1/11-risks.md` row 8,
//! RISK-CODEX-EXPERIMENTAL).
//!
//! Every experimental method the adapter may call is listed here, with the Codex version
//! its shape was read at and where it was read. Nothing else in the crate names an
//! experimental method: the client's method allowlist ([`crate::client::STABLE_METHODS`]
//! plus [`EXPERIMENTAL_METHODS`]) refuses any other, and an experimental call is built only
//! through this module ([`ExperimentalCall`]). When Codex changes one of these methods,
//! this module is the one place that follows it (`spec/bindings/mcp.md` §8.2.1,
//! "Consequences of queue-only delivery").
//!
//! Experimental methods need `capabilities.experimentalApi` at `initialize`, which the
//! client declares because this shim exists ([`DECLARES_EXPERIMENTAL_API`]). Declaring it
//! is the documented way in, not impersonation (`oac-boundaries` boundary 9).
//!
//! Codex floats (#216): [`SHIM_CODEX_VERSION`] is the version the shapes were read at, not
//! a gate. A different running version is reported as a warning
//! ([`crate::client::CodexClient::version_note`]); re-checking the shapes on it is a
//! follow-up, never a refusal to run.

use oac_core::json::Json;

/// The shim's name, for diagnostics and the decision record.
pub const SHIM_NAME: &str = "oac-codex-experimental";

/// The Codex version every shape here was read at: the last tested Codex
/// (`docs/planning/PINS.md`, "Codex CLI and app-server").
pub const SHIM_CODEX_VERSION: &str = "0.161.0";

/// The shim's version label: its name at the Codex version it was read at.
pub const SHIM_LABEL: &str = "oac-codex-experimental@codex-0.161.0";

/// True: the client declares `capabilities.experimentalApi`, because the methods below
/// need it.
pub const DECLARES_EXPERIMENTAL_API: bool = true;

/// `thread/queue/add`: the holding hand-off (`spec/bindings/mcp.md` §8.2.1,
/// [MCPB-CDX-002]). Experimental: `#[experimental("thread/queue/add")]`.
pub const THREAD_QUEUE_ADD: &str = "thread/queue/add";

/// Every experimental method the adapter may call.
pub const EXPERIMENTAL_METHODS: &[&str] = &[THREAD_QUEUE_ADD];

/// The members of `thread/queue/add` params, sorted: exactly the three of
/// `ThreadQueueAddParams`, so no member sets a thread or turn setting ([MCPB-CDX-005]).
///
/// Where read: `ThreadQueueAddParams` has exactly `threadId`, `input` and
/// `clientUserMessageId` (upstream Codex repository, tag `rust-v0.160.0`,
/// `codex-rs/app-server-protocol/src/protocol/v2/thread.rs#L910-L914`, cited in
/// `spec/bindings/mcp.md` §8.2.1, retrieved 2026-10-04); the request was recorded with
/// exactly these at `0.161.0` (`docs/planning/gates/fixtures/s3-codex-capture/transcript-2026-10-07-0.161.0-herdr.jsonl`
/// L65, L99, L109). The method is absent from the default schema
/// (`docs/planning/vendor/codex-app-server-protocol/README.md`).
pub const QUEUE_ADD_MEMBERS: &[&str] = &["clientUserMessageId", "input", "threadId"];

/// The member of the `thread/queue/add` result the client reads: `queuedSubmission`, an
/// object (S3 capture L69, L136).
pub const QUEUE_ADD_RESULT_MEMBER: &str = "queuedSubmission";

/// One experimental call, built only by this module.
#[derive(Clone, PartialEq, Eq)]
pub struct ExperimentalCall {
    method: &'static str,
    params: String,
}

impl std::fmt::Debug for ExperimentalCall {
    // The params name a thread: never printed (IFC-TYP-092).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExperimentalCall")
            .field("method", &self.method)
            .finish_non_exhaustive()
    }
}

impl ExperimentalCall {
    /// The method.
    pub fn method(&self) -> &'static str {
        self.method
    }

    /// The params, as JSON object text.
    pub fn params(&self) -> &str {
        &self.params
    }
}

/// The `thread/queue/add` call that holds `text` for `thread`, with
/// `client_user_message_id`. One text input item; nothing else.
///
/// It frames nothing and decides nothing: what text to hold, and whether to hold it at all,
/// is the caller's (G7, #68: provenance framing and the outcome mapping of
/// `spec/bindings/mcp.md` §8.2.1).
pub fn queue_add(thread: &str, text: &str, client_user_message_id: &str) -> ExperimentalCall {
    let s = |v: &str| Json::String(v.to_owned()).to_compact();
    ExperimentalCall {
        method: THREAD_QUEUE_ADD,
        params: format!(
            "{{\"threadId\":{},\"input\":[{{\"type\":\"text\",\"text\":{}}}],\"clientUserMessageId\":{}}}",
            s(thread),
            s(text),
            s(client_user_message_id)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oac_core::json::parse;

    #[test]
    fn queue_add_carries_exactly_its_three_members() {
        let c = queue_add("01a1186d-b327-74f2-ac20-e3c5d0a9e81c", "hi \"x\"", "oac-1");
        assert_eq!(c.method(), "thread/queue/add");
        let v = parse(c.params().as_bytes()).unwrap();
        let mut names: Vec<&str> = v.as_object().unwrap().names().collect();
        names.sort_unstable();
        assert_eq!(names, QUEUE_ADD_MEMBERS);
        let input = v
            .as_object()
            .unwrap()
            .get("input")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(input.len(), 1);
        let item = input[0].as_object().unwrap();
        assert_eq!(item.get("text").and_then(Json::as_str), Some("hi \"x\""));
        assert!(!format!("{c:?}").contains("01a1186d"));
    }

    #[test]
    fn the_shim_is_named_and_labelled_with_its_version() {
        assert!(SHIM_LABEL.starts_with(SHIM_NAME));
        assert!(SHIM_LABEL.ends_with(SHIM_CODEX_VERSION));
        assert_eq!(EXPERIMENTAL_METHODS, [THREAD_QUEUE_ADD]);
    }
}
