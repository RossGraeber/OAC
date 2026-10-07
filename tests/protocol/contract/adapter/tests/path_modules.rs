// SPDX-License-Identifier: Apache-2.0

//! The source scan on disk, as `tests/real_adapters.rs` runs it: `crate_files` gives
//! `src/` and `build.rs`, and the scan reads every file their `mod` declarations load.
//! Scratch crates are built under cargo's per-test temporary directory.
//!
//! - The PR #323 third review's plant, `#[path = "../zzhidden/h.rs"] mod h;` with the
//!   module outside `src/`, fails closed: an adapter holds no `#[path]` at all (#324; R2 of
//!   the PR #336 re-review).
//! - A plain `mod` is followed to its file.
//! - A symlink on the way to a module file fails closed (PR #336 review N7).

use std::path::{Path, PathBuf};

use oac_contract_adapter::source::{
    crate_files, implements_provider_adapter, package_sources, scan,
};

const ROWS: [&str; 4] = ["IFC-ADP-001", "IFC-ADP-002", "IFC-ADP-007", "IFC-ADP-013"];

fn scratch_crate(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    dir
}

fn assert_fails_closed(dir: &Path) {
    let files = crate_files(dir);
    let findings = scan(&files);
    for req in ROWS {
        assert!(
            findings.iter().any(|f| f.requirement == req),
            "{req}: {findings:?}"
        );
    }
    assert!(!implements_provider_adapter(&files).is_empty());
}

#[test]
fn a_path_module_outside_src_fails_closed() {
    let dir = scratch_crate(
        "path-module-outside-src",
        &[
            ("src/lib.rs", "#[path = \"../zzhidden/h.rs\"]\nmod h;\n"),
            (
                "zzhidden/h.rs",
                "pub struct Q;\nimpl oac_core::adapter::ProviderAdapter for Q {}\n",
            ),
        ],
    );
    assert_fails_closed(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_path_module_that_names_no_file_fails_closed() {
    let dir = scratch_crate(
        "path-module-missing",
        &[("src/lib.rs", "#[path = \"../zzhidden/gone.rs\"]\nmod h;\n")],
    );
    assert_fails_closed(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_plain_module_is_followed() {
    let dir = scratch_crate(
        "plain-module",
        &[
            ("src/lib.rs", "mod m;\n"),
            (
                "src/m/mod.rs",
                "pub struct Q;\nimpl oac_core::adapter::ProviderAdapter for Q {}\n\
                 pub fn f(_: &dyn oac_core::transport::Transport) {}\n",
            ),
        ],
    );
    let files = crate_files(&dir);
    let findings = scan(&files);
    assert!(
        findings
            .iter()
            .any(|f| f.requirement == "IFC-ADP-001" && f.file.ends_with("src/m/mod.rs")),
        "{findings:?}"
    );
    assert_eq!(implements_provider_adapter(&files).len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

const PACKAGE: &str =
    "[package]\nname = \"scratch-adapter\"\nversion = \"0.0.0\"\nedition = \"2024\"\n";
/// Its own workspace root, so cargo does not look for this repository's.
const OWN_WORKSPACE: &str = "\n[workspace]\n";

/// PR #336 third review S1: a manifest pointing the library elsewhere. The file cargo
/// compiles is scanned (its `src_path` from `cargo metadata`), and the manifest key itself
/// fails closed.
#[test]
fn a_target_path_in_the_manifest_is_scanned_and_fails_closed() {
    let dir = scratch_crate(
        "target-path",
        &[
            (
                "Cargo.toml",
                &format!("{PACKAGE}\n[lib]\npath = \"hidden/lib.rs\"\n{OWN_WORKSPACE}"),
            ),
            (
                "hidden/lib.rs",
                "pub fn f(_: &dyn oac_core::transport::Transport) {}\n",
            ),
        ],
    );
    let s = package_sources(&dir);
    assert!(
        s.built.iter().any(|p| p.ends_with("hidden/lib.rs")),
        "{:?}",
        s.built
    );
    for req in ROWS {
        assert!(
            s.findings
                .iter()
                .any(|f| f.requirement == req && f.what.contains("path")),
            "{req}: {:?}",
            s.findings
        );
    }
    let findings = scan(&s.built);
    assert!(
        findings
            .iter()
            .any(|f| f.requirement == "IFC-ADP-001" && f.file.ends_with("hidden/lib.rs")),
        "{findings:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn layout_keys_and_unvetted_dependencies_fail_closed() {
    for (name, manifest) in [
        (
            "build-key",
            format!("{PACKAGE}build = \"gen/b.rs\"\n{OWN_WORKSPACE}"),
        ),
        (
            "autobins-key",
            format!("{PACKAGE}autobins = false\n{OWN_WORKSPACE}"),
        ),
        (
            "bin-path",
            format!(
                "{PACKAGE}\n[[bin]]\nname = \"x\"\npath = \"elsewhere/main.rs\"\n{OWN_WORKSPACE}"
            ),
        ),
        (
            "unvetted-dep",
            format!("{PACKAGE}\n[dependencies]\ndep = {{ path = \"dep\" }}\n{OWN_WORKSPACE}"),
        ),
    ] {
        let dir = scratch_crate(
            &format!("manifest-{name}"),
            &[
                ("Cargo.toml", &manifest),
                ("src/lib.rs", "\n"),
                ("gen/b.rs", "fn main() {}\n"),
                ("elsewhere/main.rs", "fn main() {}\n"),
                (
                    "dep/Cargo.toml",
                    "[package]\nname = \"dep\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
                ),
                ("dep/src/lib.rs", "\n"),
            ],
        );
        let s = package_sources(&dir);
        for req in ROWS {
            assert!(
                s.findings.iter().any(|f| f.requirement == req),
                "{name} {req}: {:?}",
                s.findings
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The control: a crate on cargo's default layout, with a path dependency on a vetted name
/// (`[dependencies]` keys are not layout keys).
#[test]
fn a_default_layout_crate_passes_the_manifest_checks() {
    let dir = scratch_crate(
        "default-layout",
        &[
            (
                "Cargo.toml",
                &format!(
                    "{PACKAGE}\n[dependencies]\noac-core = {{ path = \"core\" }}\n\n[dev-dependencies]\nanything = {{ path = \"core\", package = \"oac-core\" }}\n{OWN_WORKSPACE}"
                ),
            ),
            ("src/lib.rs", "pub fn f() {}\n"),
            ("tests/t.rs", "#[test]\nfn t() {}\n"),
            (
                "core/Cargo.toml",
                "[package]\nname = \"oac-core\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
            ),
            ("core/src/lib.rs", "\n"),
        ],
    );
    let s = package_sources(&dir);
    assert!(s.findings.is_empty(), "{:?}", s.findings);
    assert!(
        s.built.iter().any(|p| p.ends_with("src/lib.rs")),
        "{:?}",
        s.built
    );
    assert!(
        s.checks.iter().any(|p| p.ends_with("tests/t.rs")),
        "{:?}",
        s.checks
    );
    assert!(scan(&s.built).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// `mod l;` with `src/l` a symlink to a directory outside `src/`: the scan resolves paths
/// lexically, so it does not follow the link and fails closed.
#[cfg(unix)]
#[test]
fn a_symlink_on_the_way_to_a_module_fails_closed() {
    let dir = scratch_crate(
        "module-symlink",
        &[
            ("src/lib.rs", "mod l;\n"),
            (
                "elsewhere/mod.rs",
                "pub fn f(_: &dyn oac_core::transport::Transport) {}\n",
            ),
        ],
    );
    std::os::unix::fs::symlink(dir.join("elsewhere"), dir.join("src/l")).unwrap();
    assert_fails_closed(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}
