// SPDX-License-Identifier: Apache-2.0

//! The adapter contract suite against the real adapters (#59, F10).
//!
//! The static checks run against `adapters/claude` and `adapters/codex` today. The files
//! scanned are the ones cargo compiles, taken from `cargo metadata` (each target's
//! `src_path`, the build script included) and everything under `src/`, and the manifest
//! is checked for keys that move a target or switch its discovery and for unvetted
//! dependencies (`source::package_sources`). Their sources are parsed and every path
//! resolved through its imports (`source`). The behavioural suite cannot run yet: on this
//! revision neither crate implements `ProviderAdapter`; both are F1 scaffolds, and the
//! adapters are Epic G (G4 to G8). The last test keeps that fact checked rather than
//! assumed. It fails as soon as an adapter implements the trait, under any alias or import
//! style, and the task that adds the implementation replaces it with a run of
//! `oac_contract_adapter::run` against the fakes (`claude::ClaudeHarness` under both
//! `MidTurnRelease` settings; a harness over `codex::CodexFake`).

use std::path::{Path, PathBuf};

use oac_contract_adapter::source::{
    PLANT, implements_provider_adapter, package_sources, rust_files, scan,
};

fn adapter_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../adapters")
        .join(name)
}

fn listed(findings: &[oac_contract_adapter::source::Finding]) -> String {
    findings
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

#[test]
fn the_real_adapters_pass_the_static_routing_checks() {
    for name in ["claude", "codex"] {
        let s = package_sources(&adapter_dir(name));
        assert!(!s.built.is_empty(), "no sources for adapters/{name}");
        let mut findings = s.findings;
        findings.extend(scan(&s.built));
        assert!(
            findings.is_empty(),
            "adapters/{name}: {}",
            listed(&findings)
        );
    }
}

/// An adapter's own tests never reach the suite's planted breaches either (the TEST-PLANT
/// row, PR #336 review N6): its test, bench and example targets, and everything under
/// `tests/`. Only that row is checked there: an adapter's tests may do what its code may
/// not (make a connection, say), and may define test macros.
#[test]
fn no_real_adapter_test_reaches_the_planted_breaches() {
    for name in ["claude", "codex"] {
        let mut files = package_sources(&adapter_dir(name)).checks;
        files.extend(rust_files(&adapter_dir(name).join("tests")));
        let found: Vec<_> = scan(&files)
            .into_iter()
            .filter(|f| f.requirement == PLANT)
            .collect();
        assert!(
            found.is_empty(),
            "adapters/{name} tests: {}",
            listed(&found)
        );
    }
}

/// The vetted dependency exports no macro and is no proc-macro (`VETTED_DEPENDENCIES`,
/// PR #336 third review N-b): no `#[macro_export]` anywhere in `oac-core`'s sources, and
/// `oac-core` has no proc-macro target.
#[test]
fn the_vetted_dependency_exports_no_macro() {
    let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../core/src");
    let files = rust_files(&core);
    assert!(!files.is_empty());
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(
            !text.contains("macro_export"),
            "{}: oac-core exports a macro; re-vet it for adapters",
            f.display()
        );
    }
    let manifest = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../core/Cargo.toml"),
    )
    .unwrap();
    assert!(
        !manifest.contains("proc-macro"),
        "oac-core is a proc-macro crate; re-vet it for adapters"
    );
}

#[test]
fn no_real_adapter_implements_the_trait_yet() {
    for name in ["claude", "codex"] {
        let s = package_sources(&adapter_dir(name));
        // A manifest the checks cannot read fails closed here too (its IFC-ADP-010 row).
        let mut found: Vec<_> = s
            .findings
            .into_iter()
            .filter(|f| f.requirement == "IFC-ADP-010")
            .collect();
        found.extend(implements_provider_adapter(&s.built));
        assert!(
            found.is_empty(),
            "adapters/{name} implements ProviderAdapter ({}): run the adapter contract suite \
             against it through the fakes, and replace this test (see this file's docs)",
            listed(&found)
        );
    }
}
