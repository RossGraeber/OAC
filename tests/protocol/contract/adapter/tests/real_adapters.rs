// SPDX-License-Identifier: Apache-2.0

//! The adapter contract suite against the real adapters (#59, F10).
//!
//! The static checks run against `adapters/claude` and `adapters/codex` today, and against
//! `adapters/mcp-tools`, the tool crate both may depend on (#7): a dependency's own files
//! are seen only when they are scanned too (`source` module docs). The files
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
    FORBIDDEN_FAMILIES, FORBIDDEN_NAMES, PLANT, Vetted, implements_provider_adapter,
    macros_that_load_files, package_sources, rust_files, scan, vetted_dependencies,
};

/// The crates under `adapters/` the static checks read: both adapters, and the tool crate
/// they share (#7, `docs/planning/decisions/G-7-stage4-dependencies.md` §4).
const SCANNED: [&str; 3] = ["claude", "codex", "mcp-tools"];

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
    for name in SCANNED {
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
    for name in SCANNED {
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

/// The repository's own vetted dependencies export no macro and are no proc-macro
/// (`VETTED_DEPENDENCIES`, PR #336 third review N-b): no `#[macro_export]` anywhere in
/// `oac-core`'s or `oac-mcp-tools`'s sources, and neither has a proc-macro target.
#[test]
fn the_vetted_dependency_exports_no_macro() {
    let mut paths = 0;
    for (name, v) in vetted_dependencies() {
        let Vetted::Path(dir) = v else { continue };
        paths += 1;
        let files = rust_files(&dir.join("src"));
        assert!(!files.is_empty(), "{name}: no sources");
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap();
            assert!(
                !text.contains("macro_export"),
                "{}: {name} exports a macro; re-vet it for adapters",
                f.display()
            );
        }
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(
            !manifest.contains("proc-macro"),
            "{name} is a proc-macro crate; re-vet it for adapters"
        );
    }
    assert_eq!(paths, 2, "oac-core and oac-mcp-tools");
}

/// The crates.io vetted dependencies (`rmcp`, `tokio`; G-7 §5), once they are in the
/// workspace graph at their pins: neither crate itself has a proc-macro target, and none of
/// their own `macro_rules!` bodies can load a file (`macros_that_load_files`). Until an
/// adapter takes them (G4, G6) they are not in the graph, and this says so rather than
/// passing on a crate it never read. Not covered: proc-macro crates reached through their
/// vetted features (`tokio-macros`, `schemars_derive`, `serde_derive`); G-7 §5 records those
/// as vetted by reading at their versions, and this test lists them when present.
#[test]
fn the_vetted_registry_dependencies_export_no_file_loading_macro() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = std::process::Command::new(cargo)
        // As scripts/check-crate-deps.mjs METADATA_ARGS: a crate reached only behind a
        // feature is still in the graph (PR #352 review finding 10).
        .args([
            "metadata",
            "--format-version",
            "1",
            "--offline",
            "--all-features",
            "--locked",
        ])
        .current_dir(&root)
        .output()
        .expect("cargo metadata runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = oac_core::json::parse(&out.stdout).expect("cargo metadata is JSON");
    let packages = json
        .as_object()
        .and_then(|o| o.get("packages"))
        .and_then(|p| p.as_array())
        .unwrap_or_default();
    for (name, v) in vetted_dependencies() {
        let Vetted::Registry { req, .. } = v else {
            continue;
        };
        let version = req.trim_start_matches('=');
        let found: Vec<_> = packages
            .iter()
            .filter_map(|p| p.as_object())
            .filter(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))
            .collect();
        for p in &found {
            let got = p
                .get("version")
                .and_then(|n| n.as_str())
                .unwrap_or_default();
            assert_eq!(
                got, version,
                "{name} is in the graph at {got}, not its pin {req}"
            );
            let targets = p
                .get("targets")
                .and_then(|t| t.as_array())
                .unwrap_or_default();
            for t in targets.iter().filter_map(|t| t.as_object()) {
                let kinds = t.get("kind").and_then(|k| k.as_array()).unwrap_or_default();
                assert!(
                    !kinds.iter().any(|k| k.as_str() == Some("proc-macro")),
                    "{name} has a proc-macro target; re-vet it for adapters"
                );
            }
            for q in packages.iter().filter_map(|q| q.as_object()) {
                let proc_macro = q
                    .get("targets")
                    .and_then(|t| t.as_array())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|t| t.as_object())
                    .any(|t| {
                        t.get("kind")
                            .and_then(|k| k.as_array())
                            .unwrap_or_default()
                            .iter()
                            .any(|k| k.as_str() == Some("proc-macro"))
                    });
                if proc_macro {
                    eprintln!(
                        "proc-macro in the graph (vetted by reading, G-7 section 5): {} {}",
                        q.get("name").and_then(|n| n.as_str()).unwrap_or_default(),
                        q.get("version")
                            .and_then(|n| n.as_str())
                            .unwrap_or_default()
                    );
                }
            }
            let manifest = p.get("manifest_path").and_then(|m| m.as_str()).unwrap();
            let src = Path::new(manifest).parent().unwrap().join("src");
            for f in rust_files(&src) {
                let text = std::fs::read_to_string(&f).unwrap();
                if !text.contains("macro_rules") {
                    continue;
                }
                let hits = macros_that_load_files(&text);
                assert!(hits.is_empty(), "{}: {}", f.display(), hits.join("; "));
            }
        }
        if found.is_empty() {
            eprintln!("{name} {req}: not in the workspace graph yet; vetted on adoption (G-7 §5)");
        }
    }
}

/// The forbidden lists here and `scripts/check-crate-deps.mjs` rule 6 are the same (#7,
/// G-7 §2): the families (`codex-`) and the names (`rmcp-macros`). The static scan refuses
/// them in an adapter's own dependencies and resolved graph, the graph check in every member.
#[test]
fn the_forbidden_lists_agree() {
    let script = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../scripts/check-crate-deps.mjs"),
    )
    .unwrap();
    let js = |name: &str| {
        let start = script
            .find(&format!("export const {name} = ["))
            .unwrap_or_else(|| panic!("{name} in check-crate-deps.mjs"));
        let end = start + script[start..].find("];").unwrap();
        let mut v: Vec<String> = script[start..end]
            .lines()
            .skip(1)
            .map(|l| l.trim().trim_end_matches(',').trim_matches('\'').to_owned())
            .filter(|l| !l.is_empty())
            .collect();
        v.sort_unstable();
        v
    };
    let rs = |v: &[&str]| {
        let mut v: Vec<String> = v.iter().map(|s| (*s).to_owned()).collect();
        v.sort_unstable();
        v
    };
    assert_eq!(js("FORBIDDEN_FAMILIES"), rs(FORBIDDEN_FAMILIES));
    assert_eq!(js("FORBIDDEN_NAMES"), rs(FORBIDDEN_NAMES));
}

#[test]
fn no_real_adapter_implements_the_trait_yet() {
    for name in SCANNED {
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
