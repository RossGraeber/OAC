// SPDX-License-Identifier: Apache-2.0

//! The adapter contract suite against the real adapters (#59, F10).
//!
//! The static checks run against `adapters/claude` and `adapters/codex` today: their
//! sources are parsed and every path resolved through its imports (`source`). The
//! behavioural suite cannot run yet: on this revision neither crate implements
//! `ProviderAdapter`; both are F1 scaffolds, and the adapters are Epic G (G4 to G8). The
//! second test keeps that fact checked rather than assumed. It fails as soon as an adapter
//! implements the trait, under any alias or import style, and the task that adds the
//! implementation replaces it with a run of `oac_contract_adapter::run` against the fakes
//! (`claude::ClaudeHarness` under both `MidTurnRelease` settings; a harness over
//! `codex::CodexFake`).

use std::path::{Path, PathBuf};

use oac_contract_adapter::source::{crate_files, implements_provider_adapter, scan};

fn adapter_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../adapters")
        .join(name)
}

#[test]
fn the_real_adapters_pass_the_static_routing_checks() {
    for name in ["claude", "codex"] {
        let files = crate_files(&adapter_dir(name));
        assert!(!files.is_empty(), "no sources under adapters/{name}/src");
        let findings = scan(&files);
        assert!(
            findings.is_empty(),
            "adapters/{name}: {}",
            findings
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
}

#[test]
fn no_real_adapter_implements_the_trait_yet() {
    for name in ["claude", "codex"] {
        let found = implements_provider_adapter(&crate_files(&adapter_dir(name)));
        assert!(
            found.is_empty(),
            "adapters/{name} implements ProviderAdapter ({}): run the adapter contract suite \
             against it through the fakes, and replace this test (see this file's docs)",
            found
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
}
