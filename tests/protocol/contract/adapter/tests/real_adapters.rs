// SPDX-License-Identifier: Apache-2.0

//! The adapter contract suite against the real adapters (#59, F10).
//!
//! The static checks run against every workspace member under `adapters/`, found through
//! `cargo metadata` (`source::adapter_members`; PR #352 second review finding 3): today
//! `adapters/claude`, `adapters/codex` and `adapters/mcp-tools`, the tool crate both may
//! depend on (#7). A dependency's own files are seen only when they are scanned too
//! (`source` module docs). The files
//! scanned are the ones cargo compiles, taken from `cargo metadata` (each target's
//! `src_path`, the build script included) and everything under `src/`, and the manifest
//! is checked for keys that move a target or switch its discovery and for unvetted
//! dependencies (`source::package_sources`). Their sources are parsed and every path
//! resolved through its imports (`source`). The behavioural suite runs from each adapter's
//! one harness file, `adapters/<name>/tests/contract.rs` (README, "Where a harness lives"),
//! never from here. The last test keeps that tied to the code: an adapter that implements
//! `ProviderAdapter`, under any alias or import style, must have that file, and it must
//! reach `oac_contract_adapter::run` and assert the report conformant. An adapter that does
//! not implement the trait yet (an F1 scaffold) needs none.
//!
//! *Dated note, 2026-10-09 (#65, G4):* until G4 the last test failed as soon as any adapter
//! implemented the trait, and said that the task adding the implementation replaces it
//! "with a run of `oac_contract_adapter::run` against the fakes". Since #351 that run may
//! live only in the adapter's `tests/contract.rs` (`tests/harness_location.rs`), so G4
//! replaced it with the check above, which holds every adapter, the Codex one included,
//! to the same rule.

use std::path::{Path, PathBuf};

use oac_contract_adapter::source::{
    FORBIDDEN_FAMILIES, FORBIDDEN_NAMES, Finding, PLANT, Resolved, Vetted, adapter_members,
    cargo_registry_src, implements_provider_adapter, macros_that_load_files, package_sources,
    rust_files, scan, vet_registry_location, vetted_dependencies,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..")
}

/// The crates under `adapters/` the static checks read: every workspace member there, from
/// `cargo metadata` (`source::adapter_members`), not a fixed list, so a new adapter is
/// vetted the moment it joins the workspace (PR #352 second review finding 3). Today: both
/// adapters and the tool crate they share (#7, G-7 §4), which this checks are among them.
fn scanned() -> Vec<PathBuf> {
    let dirs = adapter_members(&repo_root()).expect("the workspace's adapter members");
    for want in ["claude", "codex", "mcp-tools"] {
        assert!(
            dirs.iter().any(|d| d.ends_with(want)),
            "adapters/{want} is not a workspace member: {dirs:?}"
        );
    }
    dirs
}

fn listed(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// Every static-routing and dependency finding for the adapter package at `dir`.
fn static_findings(dir: &Path) -> Vec<Finding> {
    let s = package_sources(dir);
    assert!(!s.built.is_empty(), "no sources for {}", dir.display());
    let mut findings = s.findings;
    findings.extend(scan(&s.built));
    findings
}

#[test]
fn the_real_adapters_pass_the_static_routing_checks() {
    for dir in scanned() {
        let findings = static_findings(&dir);
        assert!(
            findings.is_empty(),
            "{}: {}",
            dir.display(),
            listed(&findings)
        );
    }
}

/// The planted case for PR #352 second review finding 3: a new `adapters/acp` member that
/// takes `tokio` with `net`, `process` and `fs`, and an unvetted crate, and uses
/// `tokio::net`. It is found by `adapter_members` (the fixed list it replaced did not hold
/// it) and refused by the same checks as the real adapters. A scratch workspace in the
/// system temp directory, removed afterwards; no network (`--offline`).
#[test]
fn a_new_adapter_member_is_found_and_vetted() {
    let ws = std::env::temp_dir().join(format!("oac-new-adapter-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    let core = std::fs::canonicalize(repo_root().join("core")).unwrap();
    let core = core.to_string_lossy().replace('\\', "/");
    let core = core.trim_start_matches("//?/");
    let write = |rel: &str, text: &str| {
        let p = ws.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    write(
        "Cargo.toml",
        "[workspace]\nresolver = \"3\"\nmembers = [\"adapters/acp\", \"stub\"]\n",
    );
    write(
        "stub/Cargo.toml",
        "[package]\nname = \"planted-stub\"\nversion = \"0.0.1\"\nedition = \"2024\"\n",
    );
    write("stub/src/lib.rs", "");
    write(
        "adapters/acp/Cargo.toml",
        &format!(
            "[package]\nname = \"oac-adapter-acp\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n\
             [dependencies]\noac-core = {{ path = \"{core}\" }}\n\
             planted-stub = {{ path = \"../../stub\" }}\n\
             tokio = {{ version = \"=1.53.2\", features = [\"net\", \"process\", \"fs\"] }}\n"
        ),
    );
    write(
        "adapters/acp/src/lib.rs",
        "pub type Probe = Option<tokio::net::TcpStream>;\n",
    );
    let dirs = adapter_members(&ws).expect("the scratch workspace's adapter members");
    assert_eq!(dirs.len(), 1, "{dirs:?}");
    assert!(dirs[0].ends_with("adapters/acp"), "{dirs:?}");
    let all = listed(&static_findings(&dirs[0]));
    let _ = std::fs::remove_dir_all(&ws);
    for want in ["`planted-stub`", "feature `net` is not vetted"] {
        assert!(all.contains(want), "no `{want}` finding in: {all}");
    }
}

/// The planted cases for PR #352 fourth review findings 1 and 2, on a scratch workspace that
/// resolves offline (no registry crate in it): an adapter declaring features `a` and `b`
/// with `cfg(all(feature = "a", not(feature = "b")))` code, a root `[patch.crates-io]` of
/// `tokio-macros` to a path outside the workspace, and a `tokio-macros` path package in the
/// adapter's closure. Each is its own finding.
#[test]
fn features_root_patches_and_path_packages_in_the_closure_are_refused() {
    let tmp = std::env::temp_dir().join(format!("oac-fourth-review-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let write = |rel: &str, text: &str| {
        let p = tmp.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    let edited = "[package]\nname = \"tokio-macros\"\nversion = \"2.7.2\"\nedition = \"2024\"\n";
    write("outside/tokio-macros/Cargo.toml", edited);
    write("outside/tokio-macros/src/lib.rs", "");
    write(
        "ws/Cargo.toml",
        "[workspace]\nresolver = \"3\"\nmembers = [\"adapters/acp\"]\n\n\
         [patch.crates-io]\ntokio-macros = { path = \"../outside/tokio-macros\" }\n",
    );
    write(
        "ws/adapters/acp/Cargo.toml",
        "[package]\nname = \"oac-adapter-acp\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n\
         [dependencies]\ntokio-macros = { path = \"../../../outside/tokio-macros\" }\n\n\
         [features]\na = []\nb = []\n",
    );
    write(
        "ws/adapters/acp/src/lib.rs",
        "#[cfg(all(feature = \"a\", not(feature = \"b\")))]\npub struct Probe;\n",
    );
    let dirs = adapter_members(&tmp.join("ws")).expect("the scratch workspace's adapter members");
    assert_eq!(dirs.len(), 1, "{dirs:?}");
    let all = listed(&static_findings(&dirs[0]));
    let _ = std::fs::remove_dir_all(&tmp);
    for want in [
        "declares [features] (a, b)",
        "the root manifest may hold no [patch] or [replace]",
        "resolved package `tokio-macros 2.7.2` in the closure comes from a path, not crates.io",
    ] {
        assert!(all.contains(want), "no `{want}` finding in: {all}");
    }
}

/// An adapter's own tests never reach the suite's planted breaches either (the TEST-PLANT
/// row, PR #336 review N6): its test, bench and example targets, and everything under
/// `tests/`. Only that row is checked there: an adapter's tests may do what its code may
/// not (make a connection, say), and may define test macros.
#[test]
fn no_real_adapter_test_reaches_the_planted_breaches() {
    for dir in scanned() {
        let name = dir.display();
        let mut files = package_sources(&dir).checks;
        files.extend(rust_files(&dir.join("tests")));
        let found: Vec<_> = scan(&files)
            .into_iter()
            .filter(|f| f.requirement == PLANT)
            .collect();
        assert!(found.is_empty(), "{name} tests: {}", listed(&found));
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
            // The copy read here is the registry's own, not a vendored one a `[source]`
            // replacement points at (PR #352 second review finding 2).
            let at = Resolved {
                name: name.to_owned(),
                version: got.to_owned(),
                source: p.get("source").and_then(|s| s.as_str()).map(str::to_owned),
                dir: Path::new(manifest).parent().unwrap().to_path_buf(),
            };
            assert!(
                at.source.is_some(),
                "{name} resolves to a path, not crates.io"
            );
            if let Some(why) = vet_registry_location(&at, cargo_registry_src().as_deref()) {
                panic!("{why}");
            }
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
fn every_real_adapter_that_implements_the_trait_runs_the_suite() {
    for dir in scanned() {
        let name = dir.display();
        let s = package_sources(&dir);
        // A manifest the checks cannot read fails closed here too (its IFC-ADP-010 row).
        let unreadable: Vec<_> = s
            .findings
            .into_iter()
            .filter(|f| f.requirement == "IFC-ADP-010")
            .collect();
        assert!(unreadable.is_empty(), "{name}: {}", listed(&unreadable));
        let implements = implements_provider_adapter(&s.built);
        if implements.is_empty() {
            continue;
        }
        assert!(
            !dir.ends_with("mcp-tools"),
            "{name} is the shared tool crate, not an adapter, and implements ProviderAdapter ({})",
            listed(&implements)
        );
        let harness = dir.join("tests").join("contract.rs");
        let text = std::fs::read_to_string(&harness).unwrap_or_else(|e| {
            panic!(
                "{name} implements ProviderAdapter ({}) but has no harness file {}: {e}",
                listed(&implements),
                harness.display()
            )
        });
        for needed in ["oac_contract_adapter", "run(", ".assert_conformant()"] {
            assert!(
                text.contains(needed),
                "{name} implements ProviderAdapter ({}), and its harness file {} does not \
                 run the adapter contract suite (no `{needed}`)",
                listed(&implements),
                harness.display()
            );
        }
    }
}
