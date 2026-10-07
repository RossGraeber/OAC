// SPDX-License-Identifier: Apache-2.0

//! The source scan follows `#[path]` modules on disk (#324), as `tests/real_adapters.rs`
//! runs it: `crate_files` gives `src/` and `build.rs`, and the scan reads every file their
//! `mod` declarations load. The PR #323 third review's plant, `#[path = "../zzhidden/h.rs"]
//! mod h;` with the module outside `src/`, is built in a scratch crate under cargo's
//! per-test temporary directory.

use std::path::{Path, PathBuf};

use oac_contract_adapter::source::{crate_files, implements_provider_adapter, scan};

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

#[test]
fn a_path_module_outside_src_is_scanned() {
    let dir = scratch_crate(
        "path-module-outside-src",
        &[
            ("src/lib.rs", "#[path = \"../zzhidden/h.rs\"]\nmod h;\n"),
            (
                "zzhidden/h.rs",
                "pub struct Q;\nimpl oac_core::adapter::ProviderAdapter for Q {}\n\
                 pub fn f(_: &dyn oac_core::transport::Transport) {}\n",
            ),
        ],
    );
    let files = crate_files(&dir);
    assert_eq!(files.len(), 1, "{files:?}");
    let findings = scan(&files);
    assert!(
        findings
            .iter()
            .any(|f| f.requirement == "IFC-ADP-001" && f.file.ends_with("zzhidden/h.rs")),
        "{findings:?}"
    );
    let impls = implements_provider_adapter(&files);
    assert_eq!(impls.len(), 1, "{impls:?}");
    assert!(impls[0].file.ends_with("zzhidden/h.rs"), "{impls:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// PR #336 review N7: `#[path = "l/../x.rs"]` with `src/l` a symlink. rustc resolves `..`
/// physically (beside the link's target); the scan resolves lexically, so it fails closed.
#[cfg(unix)]
#[test]
fn a_symlink_on_the_way_to_a_path_module_fails_closed() {
    let dir = scratch_crate(
        "path-module-symlink",
        &[
            ("src/lib.rs", "#[path = \"l/../x.rs\"]\nmod h;\n"),
            ("src/x.rs", "pub fn clean() {}\n"),
            ("elsewhere/inner/keep.rs", "\n"),
            (
                "elsewhere/x.rs",
                "pub fn f(_: &dyn oac_core::transport::Transport) {}\n",
            ),
        ],
    );
    std::os::unix::fs::symlink(dir.join("elsewhere/inner"), dir.join("src/l")).unwrap();
    let findings = scan(&crate_files(&dir));
    for req in ["IFC-ADP-001", "IFC-ADP-002", "IFC-ADP-007", "IFC-ADP-013"] {
        assert!(
            findings.iter().any(|f| f.requirement == req),
            "{req}: {findings:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_path_module_that_names_no_file_fails_closed() {
    let dir = scratch_crate(
        "path-module-missing",
        &[("src/lib.rs", "#[path = \"../zzhidden/gone.rs\"]\nmod h;\n")],
    );
    let files = crate_files(&dir);
    let findings = scan(&files);
    for req in ["IFC-ADP-001", "IFC-ADP-002", "IFC-ADP-007", "IFC-ADP-013"] {
        assert!(
            findings.iter().any(|f| f.requirement == req),
            "{req}: {findings:?}"
        );
    }
    assert!(!implements_provider_adapter(&files).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
