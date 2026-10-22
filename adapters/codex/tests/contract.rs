// SPDX-License-Identifier: Apache-2.0

//! The adapter contract suite (#59, F10), run unchanged against the Codex adapter over the
//! fake Codex app-server (F9) in a harness that gives it the Codex binding's two connections:
//! the carrier, which the daemon designates (`CodexAdapter::designate_carrier`) before the
//! adapter is given the connection, and one shim connection per session, over which the
//! harness speaks MCP as Codex does and reports each call to the carrier as the app-server
//! reports it (`oacFake/mcp/*`). This is the one file in the adapter that may name the
//! suite's harness (`tests/protocol/contract/adapter/README.md`, "Where a harness lives").
//!
//! The harness is the suite's own `CodexHarness`. It leaves the adapter two things to say:
//! the name the OAC server is registered under with Codex, and how the carrier is designated.

use std::path::PathBuf;
use std::sync::Arc;

use oac_adapter_codex::CodexAdapter;
use oac_adapter_codex::adapter::DEFAULT_REGISTERED_NAME;
use oac_contract_adapter::codex::CodexHarness;
use oac_contract_adapter::run;
use oac_contract_adapter::source::crate_files;

#[test]
fn the_codex_adapter_passes_the_adapter_contract_suite() {
    let adapter = Arc::new(CodexAdapter::new());
    let designating = adapter.clone();
    let mut h = CodexHarness::new(
        adapter,
        DEFAULT_REGISTERED_NAME,
        Box::new(move |handle| designating.designate_carrier(handle)),
        crate_files(&PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
    )
    .expect("spawn the fake Codex app-server with node");
    let report = run(&mut h);
    println!("{report}");
    report.assert_conformant();
}
