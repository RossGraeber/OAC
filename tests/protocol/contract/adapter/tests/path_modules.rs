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

use oac_contract_adapter::source::{crate_files, implements_provider_adapter, scan};

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
