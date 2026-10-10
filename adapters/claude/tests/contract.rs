// SPDX-License-Identifier: Apache-2.0

//! The adapter contract suite (#59, F10), run unchanged against the Claude adapter over the
//! fake Claude Code channel endpoint (F8), under both mid-turn release settings
//! ([`MidTurnRelease::BOTH`]). This is the one file in the adapter that may name the suite's
//! harness (`tests/protocol/contract/adapter/README.md`, "Where a harness lives").
//!
//! The harness is the suite's own `ClaudeHarness`. The only binding detail it leaves to the
//! adapter is the tool arguments ([MCPB-TOOL-004]); [`encode`] writes them in the shape of
//! `oac-mcp-tools`' `inputSchema`, with the spoofed sender, when there is one, as an extra
//! `from` member that the adapter must ignore ([SC-ID-162]).

use std::path::PathBuf;
use std::sync::Arc;

use oac_adapter_claude::ClaudeAdapter;
use oac_contract_adapter::HarnessRequest;
use oac_contract_adapter::claude::{ClaudeHarness, json_string};
use oac_contract_adapter::run;
use oac_contract_adapter::source::crate_files;
use oac_fake_claude::MidTurnRelease;

fn encode(r: &HarnessRequest) -> (String, String) {
    match r {
        HarnessRequest::Send {
            to,
            text,
            claimed_from,
        } => (
            "send".into(),
            format!(
                "{{\"to\":{},\"content\":[{{\"type\":\"text\",\"text\":{}}}]{}}}",
                json_string(to.as_str()),
                json_string(text),
                claimed_from
                    .as_ref()
                    .map(|f| format!(",\"from\":{}", json_string(f.as_str())))
                    .unwrap_or_default()
            ),
        ),
        HarnessRequest::Discover => ("list_sessions".into(), "{}".into()),
    }
}

#[test]
fn the_claude_adapter_passes_the_adapter_contract_suite() {
    let files = crate_files(&PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    for release in MidTurnRelease::BOTH {
        let mut h = ClaudeHarness::new(
            Arc::new(ClaudeAdapter::new()),
            release,
            encode,
            files.clone(),
        );
        let report = run(&mut h);
        println!("{report}");
        report.assert_conformant();
    }
}
