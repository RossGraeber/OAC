// SPDX-License-Identifier: Apache-2.0

//! The harness a real adapter runs under lives in this crate (#351, PR #355 review R3).
//!
//! The suite cannot see a harness that filters what the fake observed. Gate S4 criterion 1
//! bounds that risk with a diff of `tests/protocol/contract/` against the suite's baseline,
//! and the bound holds only if every harness is inside that diff. So the real-adapter runs
//! are this crate's own tests, and:
//!
//! - only the packages in [`ALLOWED_DEPENDENTS`] depend on this crate, under any name or
//!   dependency kind (`cargo metadata` lists a renamed dependency under its package name).
//!   The list lives here, so a new dependent is itself a diff the baseline shows;
//! - no Rust file outside this crate and those packages names `AdapterHarness`;
//! - in those packages, no file implements `AdapterHarness`, renames it or this crate,
//!   globs this crate's items, or reaches this crate's `run`: they may drive the fakes'
//!   harnesses, never run the suite. This is a text rule over one line or one `use`
//!   group, enough for `rustfmt`-formatted code; the list itself is the stronger bound.
//!
//! Directories skipped by the file walk: `.git`, `node_modules`, the agent tooling
//! directories `.claude` and `.agents` (which can hold other checkouts of this
//! repository), and every cargo target directory (one holding `CACHEDIR.TAG`, or named
//! `target`).

use std::path::{Path, PathBuf};

use oac_core::json::{self, Json};

const SUITE: &str = "oac-contract-adapter";

/// Packages that may depend on this crate, with why. None is an adapter.
const ALLOWED_DEPENDENTS: &[(&str, &str)] = &[(
    "oac-transport-memory",
    "its pipeline test (#313, tests/pipelines.rs) drives the fakes through ClaudeHarness and \
     CodexFake to carry messages end to end; it runs no contract suite and implements no \
     harness",
)];

fn repo_root() -> PathBuf {
    std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.."))
        .expect("the repository root")
}

fn this_crate() -> PathBuf {
    std::fs::canonicalize(env!("CARGO_MANIFEST_DIR")).expect("this crate's directory")
}

fn get<'a>(v: &'a Json, key: &str) -> Option<&'a Json> {
    v.as_object().and_then(|o| o.get(key))
}

fn get_str<'a>(v: &'a Json, key: &str) -> Option<&'a str> {
    get(v, key).and_then(Json::as_str)
}

/// Each workspace package: its name, its directory, and whether it depends on the suite.
fn packages() -> Vec<(String, PathBuf, bool)> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = std::process::Command::new(cargo)
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
            "--manifest-path",
        ])
        .arg(repo_root().join("Cargo.toml"))
        .output()
        .expect("run cargo metadata");
    assert!(
        out.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta = json::parse(&out.stdout).expect("cargo metadata output");
    get(&meta, "packages")
        .and_then(Json::as_array)
        .expect("a package list")
        .iter()
        .map(|p| {
            let name = get_str(p, "name").unwrap_or_default().to_owned();
            let dir = get_str(p, "manifest_path")
                .and_then(|m| std::fs::canonicalize(m).ok())
                .and_then(|m| m.parent().map(Path::to_path_buf))
                .unwrap_or_default();
            let depends = get(p, "dependencies")
                .and_then(Json::as_array)
                .unwrap_or_default()
                .iter()
                .any(|d| get_str(d, "name") == Some(SUITE));
            (name, dir, depends)
        })
        .collect()
}

fn allowed_dirs() -> Vec<PathBuf> {
    packages()
        .into_iter()
        .filter(|(n, _, _)| ALLOWED_DEPENDENTS.iter().any(|(a, _)| a == n))
        .map(|(_, d, _)| d)
        .collect()
}

#[test]
fn only_the_listed_packages_depend_on_the_suite() {
    let all = packages();
    let names: Vec<&str> = all.iter().map(|(n, _, _)| n.as_str()).collect();
    assert!(
        names.contains(&SUITE) && names.contains(&"oac-core"),
        "the workspace lists neither {SUITE} nor oac-core: {names:?}"
    );
    let unlisted: Vec<&str> = all
        .iter()
        .filter(|(n, _, dep)| *dep && n != SUITE && !ALLOWED_DEPENDENTS.iter().any(|(a, _)| a == n))
        .map(|(n, _, _)| n.as_str())
        .collect();
    assert!(
        unlisted.is_empty(),
        "{unlisted:?} depend on {SUITE}: the harness a real adapter runs under lives in \
         {SUITE}, so that Gate S4 criterion 1's baseline diff covers it (src/lib.rs, \
         \"Where a harness lives\")"
    );
    for (a, _) in ALLOWED_DEPENDENTS {
        assert!(
            !all.iter()
                .any(|(n, d, _)| n == a && d.starts_with(repo_root().join("adapters"))),
            "{a} is an adapter: an adapter never depends on {SUITE}"
        );
    }
}

fn walk(dir: &Path, skip: &Path, out: &mut Vec<PathBuf>) {
    if dir == skip || dir.join("CACHEDIR.TAG").exists() {
        return;
    }
    if let Some(n) = dir.file_name().and_then(|n| n.to_str())
        && matches!(
            n,
            ".git" | "node_modules" | ".claude" | ".agents" | "target"
        )
    {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let Ok(m) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if m.is_dir() {
            walk(&p, skip, out);
        } else if m.is_file() && p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

fn rust_files_outside_the_suite() -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk(&repo_root(), &this_crate(), &mut files);
    assert!(
        files.iter().any(|f| f.ends_with("core/src/adapter.rs")),
        "the walk did not reach core/src/adapter.rs: {} file(s)",
        files.len()
    );
    files
}

#[test]
fn no_rust_file_outside_the_listed_packages_names_the_harness_trait() {
    let allowed = allowed_dirs();
    let naming: Vec<String> = rust_files_outside_the_suite()
        .into_iter()
        .filter(|f| !allowed.iter().any(|d| f.starts_with(d)))
        .filter(|f| std::fs::read_to_string(f).is_ok_and(|t| t.contains("AdapterHarness")))
        .map(|f| f.display().to_string())
        .collect();
    assert!(
        naming.is_empty(),
        "{naming:?} name AdapterHarness: the harness a real adapter runs under lives in \
         {SUITE} (src/lib.rs, \"Where a harness lives\")"
    );
}

/// What in `text` implements or renames the harness trait, or reaches the suite's `run`
/// or all of its items.
fn harness_findings(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.contains("AdapterHarness")
            && (l.starts_with("impl") || l.contains(" impl") || l.contains("AdapterHarness as"))
        {
            found.push(format!("implements or renames AdapterHarness: {l}"));
        }
    }
    if text.contains("oac_contract_adapter as") {
        found.push("renames the suite crate, which hides what it reaches".into());
    }
    let crate_path = "oac_contract_adapter::";
    let mut rest = text;
    while let Some(i) = rest.find(crate_path) {
        rest = &rest[i + crate_path.len()..];
        let after = rest.trim_start();
        let word: String = after
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if word == "run" || after.starts_with('*') {
            found.push(format!(
                "reaches {crate_path}{}",
                if word.is_empty() { "*" } else { "run" }
            ));
        } else if after.starts_with('{') {
            let group = &after[..after.find('}').map_or(after.len(), |e| e + 1)];
            let words: Vec<&str> = group
                .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '*'))
                .collect();
            if words.contains(&"run") || words.contains(&"*") {
                found.push(format!("reaches the suite's run or all its items: {group}"));
            }
        }
    }
    found
}

#[test]
fn the_listed_packages_implement_no_harness_and_run_no_suite() {
    let allowed = allowed_dirs();
    assert_eq!(
        allowed.len(),
        ALLOWED_DEPENDENTS.len(),
        "a listed package is missing from the workspace"
    );
    let mut bad = Vec::new();
    for f in rust_files_outside_the_suite() {
        if !allowed.iter().any(|d| f.starts_with(d)) {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        for x in harness_findings(&text) {
            bad.push(format!("{}: {x}", f.display()));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn the_harness_text_rule_catches_each_shape() {
    for plant in [
        "impl AdapterHarness for H {}",
        "impl oac_contract_adapter::AdapterHarness for H {}",
        "use oac_contract_adapter::AdapterHarness as Harness;",
        "let r = oac_contract_adapter::run(&mut h);",
        "use oac_contract_adapter::{CoreSide, run};",
        "use oac_contract_adapter::*;",
        "use oac_contract_adapter::{claude, *};",
        "use oac_contract_adapter as suite;",
    ] {
        assert!(!harness_findings(plant).is_empty(), "missed: {plant}");
    }
    for clean in [
        "use oac_contract_adapter::{AdapterHarness, CoreSide, HarnessRequest};",
        "use oac_contract_adapter::claude::{ClaudeHarness, pipe};",
        "claude.start_turn(0).unwrap();",
        "use oac_contract_adapter::{runner_free, CoreSide};",
    ] {
        assert!(harness_findings(clean).is_empty(), "flagged: {clean}");
    }
}
