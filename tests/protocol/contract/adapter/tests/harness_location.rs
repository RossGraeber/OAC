// SPDX-License-Identifier: Apache-2.0

//! Where the harness a real adapter runs under may live (#351; PR #355 reviews R3, B1, B2).
//!
//! The dependency direction is the documented one (07 section 3;
//! `scripts/check-crate-deps.mjs` rule 5; this crate's `Cargo.toml`): an adapter takes this
//! suite as a dev-dependency, and the suite never depends on an adapter. So the harness
//! for a real adapter lives in the adapter, at exactly one fixed path,
//! `adapters/<name>/tests/contract.rs` ([`HARNESS_FILE`]). The suite's own checks bound what
//! a harness can do. What they cannot see, a harness that fabricates or filters what the
//! fake observed, is covered by reviewing that one file, which Gate S4 evidence cites.
//!
//! This file keeps the rule mechanical:
//!
//! - **Who depends on the suite.** Only an adapter (`adapters/<name>`), the shared tool
//!   crate `adapters/mcp-tools` ([`NOT_ADAPTERS`]; #352 lets it dev-depend on a suite), and
//!   the packages in [`ALLOWED_DEPENDENTS`]. An adapter or the tool crate takes it as a
//!   dev-dependency only. No dependent renames it (`package = ..`), since a rename would
//!   hide every text rule below.
//! - **Which files may touch the harness.** In an adapter, only [`HARNESS_FILE`] may name
//!   `AdapterHarness` or this crate, or reach its `run`. Every other file outside this
//!   crate and the listed packages may name neither.
//! - **What hides a harness.** The harness file and every other file under an adapter's
//!   `tests/` may hold no `self as` import, `macro_rules!`, `include!` (any form) or
//!   `#[path]` attribute. The harness file also holds no file module (`mod x;`) and no glob
//!   or renamed import of this crate or of `AdapterHarness`, so it reads as one file. The
//!   files of a listed package may name `AdapterHarness` (to drive the fakes' harnesses),
//!   but may not implement it, rename it or this crate, glob this crate's items, reach its
//!   `run`, or hold any of those hiding shapes.
//!
//! These are text rules, enough for `rustfmt`-formatted code. What really bounds a listed
//! transport is `scripts/check-crate-deps.mjs`: a transport can never reach an adapter, so
//! a harness hidden there could only drive a stand-in, never a real adapter.
//!
//! - **What an adapter may take as a dev-dependency.** Only [`ADAPTER_DEV_DEPENDENCIES`]:
//!   the suite, `oac-core` and the one fake that is a crate. Anything else, such as an
//!   identifier-pasting proc macro (`paste`), could spell the trait and `run` in pieces
//!   that no text rule sees (PR #355 third review, C2).
//!
//! **The files read.** These come from git, not from a directory walk: every tracked file,
//! and every untracked file that is not ignored (`git ls-files -co --exclude-standard`),
//! less untracked files under `target/`. Any `CACHEDIR.TAG` in that set fails, since a
//! committed tag would hide a directory from the tools that honour it (PR #355 third
//! review, C1). So do a symlink, and a tracked file under `target/`.
//!
//! **Cargo's target directory** (fourth review, N2) is `<root>/target` or lies outside the
//! repository. Anywhere else inside it, set by a committed `.cargo/config.toml`
//! `target-dir` or by `CARGO_TARGET_DIR`, fails, since it would take a directory out of the
//! file set.
//!
//! **What cargo compiles for an adapter** (fourth review, N1), since git's file set misses
//! a file a committed `.gitignore` hides:
//! - no workspace member has a build script, neither a custom-build target nor a `build.rs`
//!   file (fifth review: any member's could write into `adapters/`);
//! - an adapter's lib target has `doctest = false`, and its `src/` files are held to the
//!   hiding shapes too, read from raw text, so a doc comment's doctest is covered; every
//!   file of an adapter, whatever its extension, is held to the name rule, since
//!   `include!` can load any file (fifth review, D1);
//! - every test, example and bench target `cargo metadata` reports for it is its harness
//!   file, or a file the name rules pass;
//! - no `.rs` file under `adapters/` is ignored by git
//!   (`git ls-files -oi --exclude-standard -- adapters/`).
//!
//! An adapter's normal and build dependencies are the static scan's vetted list
//! (`source::VETTED_DEPENDENCIES`), by `cargo metadata`, for every adapter directory.

use std::path::{Path, PathBuf};

use oac_core::json::{self, Json};

const SUITE: &str = "oac-contract-adapter";
const SUITE_CRATE: &str = "oac_contract_adapter";
const HARNESS_TRAIT: &str = "AdapterHarness";

/// The one file of an adapter, relative to `adapters/<name>/`, that may hold its harness.
pub const HARNESS_FILE: &str = "tests/contract.rs";

/// Directories under `adapters/` that are not adapters: the shared MCP tool crate (#7;
/// G-7 section 4). It may dev-depend on a suite (#352), and holds no harness.
pub const NOT_ADAPTERS: &[&str] = &["mcp-tools"];

/// Packages, not adapters, that may depend on this crate, with why. They hold no harness.
pub const ALLOWED_DEPENDENTS: &[(&str, &str)] = &[(
    "oac-transport-memory",
    "its pipeline test (#313, tests/pipelines.rs) drives the fakes through ClaudeHarness and \
     CodexFake to carry messages end to end; it runs no contract suite and implements no \
     harness",
)];

/// The only dev-dependencies an adapter may take: this suite, `oac-core`, and
/// `oac-fake-claude`, the one fake that is a crate (the fake Codex app-server is a `node`
/// program the suite spawns).
pub const ADAPTER_DEV_DEPENDENCIES: &[&str] = &[SUITE, "oac-core", "oac-fake-claude"];

/// This crate's directory, relative to the repository root.
const SUITE_DIR: &str = "tests/protocol/contract/adapter";

fn repo_root() -> PathBuf {
    std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.."))
        .expect("the repository root")
}

/// `p` relative to `root`, with `/` separators.
fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn get<'a>(v: &'a Json, key: &str) -> Option<&'a Json> {
    v.as_object().and_then(|o| o.get(key))
}

fn get_str<'a>(v: &'a Json, key: &str) -> Option<&'a str> {
    get(v, key).and_then(Json::as_str)
}

/// The adapter a repository-relative directory or file belongs to, if any.
fn adapter_of(rel: &str) -> Option<&str> {
    let rest = rel.strip_prefix("adapters/")?;
    let name = rest.split('/').next().filter(|n| !n.is_empty())?;
    (!NOT_ADAPTERS.contains(&name)).then_some(name)
}

// ---- who depends on the suite -------------------------------------------------------------

/// One workspace package's dependency on the suite.
struct Dependent {
    name: String,
    /// Repository-relative directory.
    dir: String,
    /// Each edge to the suite: its kind (`None` for normal) and its rename.
    edges: Vec<(Option<String>, Option<String>)>,
    /// The package names of its dev-dependencies.
    dev_deps: Vec<String>,
    /// The package names of its normal and build dependencies.
    other_deps: Vec<String>,
    /// Its targets: their kinds and their `src_path`.
    targets: Vec<(Vec<String>, PathBuf)>,
    /// True when a lib target of it has doctests on (`cargo metadata`'s `doctest`).
    lib_doctests: bool,
}

/// Where cargo's `target_directory` is (PR #355 fourth review, N2).
#[derive(Clone, Debug, PartialEq, Eq)]
enum TargetDir {
    /// `<root>/target`, which `.gitignore` ignores.
    RootTarget,
    /// Outside the repository.
    Outside,
    /// Anywhere else inside the repository, relative to its root: refused, since it would
    /// take its files out of the file set.
    Inside(String),
}

/// Where `target` (canonical) is, for the repository at `root` (canonical).
fn target_dir(root: &Path, target: &Path) -> TargetDir {
    if target == root.join("target") {
        TargetDir::RootTarget
    } else if target.starts_with(root) {
        TargetDir::Inside(rel(root, target))
    } else {
        TargetDir::Outside
    }
}

/// Every workspace package: its name, its repository-relative directory, its edges to the
/// suite and its dependencies and targets.
fn packages() -> Vec<Dependent> {
    metadata().0
}

/// The packages, and where cargo's `target_directory` is.
fn metadata() -> (Vec<Dependent>, TargetDir) {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let root = repo_root();
    let out = std::process::Command::new(cargo)
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
            "--manifest-path",
        ])
        .arg(root.join("Cargo.toml"))
        .output()
        .expect("run cargo metadata");
    assert!(
        out.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta = json::parse(&out.stdout).expect("cargo metadata output");
    let target = get_str(&meta, "target_directory").map_or(
        TargetDir::Inside("(none reported)".into()),
        |t| {
            // Fails closed: a directory that cannot be resolved (one that does not exist
            // yet, on Windows) is not taken to be outside the repository.
            std::fs::canonicalize(t).map_or_else(
                |_| TargetDir::Inside(format!("(cannot resolve {t})")),
                |t| target_dir(&root, &t),
            )
        },
    );
    let packages = get(&meta, "packages")
        .and_then(Json::as_array)
        .expect("a package list")
        .iter()
        .map(|p| {
            let dir = get_str(p, "manifest_path")
                .and_then(|m| std::fs::canonicalize(m).ok())
                .and_then(|m| m.parent().map(|d| rel(&root, d)))
                .unwrap_or_default();
            let edges = get(p, "dependencies")
                .and_then(Json::as_array)
                .unwrap_or_default()
                .iter()
                .filter(|d| get_str(d, "name") == Some(SUITE))
                .map(|d| {
                    (
                        get_str(d, "kind").map(str::to_owned),
                        get_str(d, "rename").map(str::to_owned),
                    )
                })
                .collect();
            let dev_deps = get(p, "dependencies")
                .and_then(Json::as_array)
                .unwrap_or_default()
                .iter()
                .filter(|d| get_str(d, "kind") == Some("dev"))
                .filter_map(|d| get_str(d, "name").map(str::to_owned))
                .collect();
            let other_deps = get(p, "dependencies")
                .and_then(Json::as_array)
                .unwrap_or_default()
                .iter()
                .filter(|d| get_str(d, "kind") != Some("dev"))
                .filter_map(|d| get_str(d, "name").map(str::to_owned))
                .collect();
            let targets = get(p, "targets")
                .and_then(Json::as_array)
                .unwrap_or_default()
                .iter()
                .map(|t| {
                    let kinds = get(t, "kind")
                        .and_then(Json::as_array)
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|k| k.as_str().map(str::to_owned))
                        .collect();
                    let src = get_str(t, "src_path")
                        .map(PathBuf::from)
                        .unwrap_or_default();
                    (kinds, src)
                })
                .collect();
            let lib_doctests = get(p, "targets")
                .and_then(Json::as_array)
                .unwrap_or_default()
                .iter()
                .filter(|t| {
                    get(t, "kind")
                        .and_then(Json::as_array)
                        .unwrap_or_default()
                        .iter()
                        .filter_map(Json::as_str)
                        .any(|k| {
                            matches!(
                                k,
                                "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro"
                            )
                        })
                })
                // Fails closed: a lib target that does not report `doctest` counts as on.
                .any(|t| get(t, "doctest").and_then(Json::as_bool) != Some(false));
            Dependent {
                name: get_str(p, "name").unwrap_or_default().to_owned(),
                dir,
                edges,
                dev_deps,
                other_deps,
                targets,
                lib_doctests,
            }
        })
        .collect();
    (packages, target)
}

/// What is wrong with the dev-dependencies `dev_deps` of package `name` at `dir`: an
/// adapter may take only [`ADAPTER_DEV_DEPENDENCIES`].
fn dev_dependency_findings(name: &str, dir: &str, dev_deps: &[String]) -> Vec<String> {
    if adapter_of(dir).is_none() {
        return Vec::new();
    }
    dev_deps
        .iter()
        .filter(|d| !ADAPTER_DEV_DEPENDENCIES.contains(&d.as_str()))
        .map(|d| {
            format!(
                "{name} ({dir}) takes {d} as a dev-dependency: an adapter takes only {ADAPTER_DEV_DEPENDENCIES:?}"
            )
        })
        .collect()
}

#[test]
fn adapters_take_only_the_listed_dev_dependencies() {
    let bad: Vec<String> = packages()
        .iter()
        .flat_map(|p| dev_dependency_findings(&p.name, &p.dir, &p.dev_deps))
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn the_dev_dependency_rule_catches_a_pasting_macro() {
    let ok: Vec<String> = ADAPTER_DEV_DEPENDENCIES
        .iter()
        .map(|d| (*d).to_owned())
        .collect();
    assert!(dev_dependency_findings("oac-adapter-codex", "adapters/codex", &ok).is_empty());
    // The PR #355 third review's C2 plant: `paste` would spell the trait and `run` in
    // pieces (`[<oac_contract _adapter>]::[<Adapter Harness>]`).
    for extra in ["paste", "oac-transport-memory", "serde_json"] {
        let deps = vec![SUITE.to_owned(), extra.to_owned()];
        assert!(
            !dev_dependency_findings("oac-adapter-codex", "adapters/codex", &deps).is_empty(),
            "missed {extra}"
        );
    }
    // Not adapters: the tool crate and the other packages are not held to this list.
    let paste = vec!["paste".to_owned()];
    assert!(dev_dependency_findings("oac-mcp-tools", "adapters/mcp-tools", &paste).is_empty());
    assert!(dev_dependency_findings("oac-cli", "cli", &paste).is_empty());
}

/// What is wrong with one edge to the suite from package `name` at `dir`.
fn dependency_findings(
    name: &str,
    dir: &str,
    kind: Option<&str>,
    rename: Option<&str>,
) -> Vec<String> {
    let mut found = Vec::new();
    let under_adapters = dir.starts_with("adapters/");
    let listed = ALLOWED_DEPENDENTS.iter().any(|(a, _)| *a == name);
    if !under_adapters && !listed {
        found.push(format!(
            "{name} ({dir}) depends on {SUITE}: only an adapter, adapters/mcp-tools or a package in ALLOWED_DEPENDENTS may"
        ));
    }
    if under_adapters && kind != Some("dev") {
        found.push(format!(
            "{name} ({dir}) takes {SUITE} as a {} dependency: only as a dev-dependency",
            kind.unwrap_or("normal")
        ));
    }
    if let Some(r) = rename {
        found.push(format!(
            "{name} ({dir}) renames {SUITE} to {r:?}, which hides it from the file rules"
        ));
    }
    found
}

#[test]
fn only_adapters_and_the_listed_packages_depend_on_the_suite() {
    let all = packages();
    let names: Vec<&str> = all.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&SUITE) && names.contains(&"oac-core"),
        "the workspace lists neither {SUITE} nor oac-core: {names:?}"
    );
    for (a, _) in ALLOWED_DEPENDENTS {
        let p = all.iter().find(|p| p.name == *a);
        assert!(p.is_some(), "listed package {a} is not in the workspace");
        assert!(
            p.is_some_and(|p| !p.dir.starts_with("adapters/")),
            "{a} is under adapters/: list only packages that are not adapters"
        );
    }
    let bad: Vec<String> = all
        .iter()
        .filter(|p| p.name != SUITE)
        .flat_map(|p| {
            p.edges
                .iter()
                .flat_map(|(k, r)| dependency_findings(&p.name, &p.dir, k.as_deref(), r.as_deref()))
        })
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn the_dependency_rule_catches_each_shape() {
    assert!(
        dependency_findings("oac-adapter-claude", "adapters/claude", Some("dev"), None).is_empty()
    );
    assert!(
        dependency_findings("oac-mcp-tools", "adapters/mcp-tools", Some("dev"), None).is_empty()
    );
    assert!(
        dependency_findings(
            "oac-transport-memory",
            "transports/memory",
            Some("dev"),
            None
        )
        .is_empty()
    );
    for (name, dir, kind, rename) in [
        ("oac-cli", "cli", Some("dev"), None),
        ("oac-adapter-codex", "adapters/codex", None, None),
        ("oac-adapter-codex", "adapters/codex", Some("build"), None),
        (
            "oac-adapter-codex",
            "adapters/codex",
            Some("dev"),
            Some("suite"),
        ),
        (
            "oac-transport-memory",
            "transports/memory",
            Some("dev"),
            Some("suite"),
        ),
    ] {
        assert!(
            !dependency_findings(name, dir, kind, rename).is_empty(),
            "missed: {name} {kind:?} {rename:?}"
        );
    }
}

// ---- the files read ------------------------------------------------------------------------

/// Every file git knows under `root`, tracked or untracked and not ignored
/// (`git ls-files -co --exclude-standard`), repository-relative, less those under `target`
/// (cargo's target directory, relative to `root`). Fails closed when git cannot list them.
fn git_files(root: &Path, target: Option<&str>) -> Vec<String> {
    let mut files = git_list(root, &["-z"]);
    files.extend(
        git_list(root, &["-o", "--exclude-standard", "-z"])
            .into_iter()
            .filter(|f| !target.is_some_and(|t| f.starts_with(&format!("{t}/")))),
    );
    files
}

/// `git ls-files` under `root` with `args`, as repository-relative paths. Fails closed.
fn git_list(root: &Path, args: &[&str]) -> Vec<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("ls-files")
        .args(args)
        .output()
        .expect("run git ls-files: the harness location rule reads the file set from git");
    assert!(
        out.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|f| !f.is_empty())
        .map(str::to_owned)
        .collect()
}

/// What the rules find over every file git lists under `root`, with `listed_dirs` the
/// listed packages' directories. Files of this crate are not read.
fn tree_findings(root: &Path, target: Option<&str>, listed_dirs: &[String]) -> Vec<String> {
    let mut bad = Vec::new();
    for r in git_files(root, target) {
        if r.starts_with(&format!("{SUITE_DIR}/")) {
            continue;
        }
        let path = root.join(&r);
        if target.is_some_and(|t| r.starts_with(&format!("{t}/"))) {
            bad.push(format!("{r}: tracked inside cargo's target directory"));
            continue;
        }
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            bad.push(format!("{r}: a symlink"));
            continue;
        }
        if r.rsplit('/').next() == Some("CACHEDIR.TAG") {
            bad.push(format!(
                "{r}: a CACHEDIR.TAG outside cargo's target directory, which hides its directory from tools that honour it"
            ));
            continue;
        }
        if !r.ends_with(".rs") {
            // Any file of an adapter, whatever its extension, is held to the name rule: an
            // `include!` can load one (PR #355 fifth review, D1).
            if adapter_of(&r).is_some() {
                let text =
                    String::from_utf8_lossy(&std::fs::read(&path).unwrap_or_default()).into_owned();
                for x in names_either(&text) {
                    bad.push(format!("{r}: {x}"));
                }
            }
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        for x in file_findings(scope(&r, listed_dirs), &text) {
            bad.push(format!("{r}: {x}"));
        }
    }
    bad
}

/// A scratch git repository under the test target directory.
fn scratch_repo(name: &str, files: &[(&str, &str)], commit: &[&str]) -> PathBuf {
    let t = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&t);
    std::fs::create_dir_all(&t).unwrap();
    for (p, text) in files {
        let f = t.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    }
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(&t)
            .args(["-c", "user.name=oac", "-c", "user.email=oac@invalid"])
            .args(args)
            .output()
            .expect("run git")
            .status
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"]);
    if !commit.is_empty() {
        let mut add = vec!["add", "--"];
        add.extend_from_slice(commit);
        git(&add);
        git(&["commit", "-q", "-m", "plant"]);
    }
    t
}

#[test]
fn a_committed_cachedir_tag_hides_no_harness() {
    // The PR #355 third review's C1 plant: a dev-dependency on the suite, and a second
    // harness at tests/zz/main.rs (a test target cargo finds with no manifest key) behind a
    // committed CACHEDIR.TAG; and the same under examples/ and benches/, one of them left
    // untracked. The real target directory holds a tag too, and is not read.
    const TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55\n";
    const HIDDEN: &str =
        "use oac_contract_adapter::{AdapterHarness, run};\n#[test]\nfn hidden() {}\n";
    let files = [
        (".gitignore", "/target\n"),
        (
            "adapters/codex/Cargo.toml",
            "[package]\nname = \"oac-adapter-codex\"\n\n[dev-dependencies]\noac-contract-adapter = { path = \"../../tests/protocol/contract/adapter\" }\n",
        ),
        ("adapters/codex/tests/zz/CACHEDIR.TAG", TAG),
        ("adapters/codex/tests/zz/main.rs", HIDDEN),
        ("adapters/codex/examples/zz/CACHEDIR.TAG", TAG),
        ("adapters/codex/examples/zz/main.rs", HIDDEN),
        ("adapters/codex/benches/zz/CACHEDIR.TAG", TAG),
        ("adapters/codex/benches/zz/main.rs", HIDDEN),
        ("target/CACHEDIR.TAG", TAG),
        ("target/debug/h.rs", HIDDEN),
    ];
    let t = scratch_repo(
        "harness-location-c1",
        &files,
        &[
            ".gitignore",
            "adapters/codex/Cargo.toml",
            "adapters/codex/tests/zz/CACHEDIR.TAG",
            "adapters/codex/tests/zz/main.rs",
            "adapters/codex/examples/zz/CACHEDIR.TAG",
            "adapters/codex/examples/zz/main.rs",
        ],
    );
    let bad = tree_findings(&t, Some("target"), &[]);
    for d in ["tests", "examples", "benches"] {
        for want in [
            format!("adapters/codex/{d}/zz/CACHEDIR.TAG: a CACHEDIR.TAG"),
            format!("adapters/codex/{d}/zz/main.rs: names AdapterHarness"),
        ] {
            assert!(
                bad.iter().any(|b| b.starts_with(&want)),
                "missed {want}: {bad:#?}"
            );
        }
    }
    assert!(
        !bad.iter().any(|b| b.starts_with("target/")),
        "read cargo's ignored target directory: {bad:#?}"
    );
    std::fs::remove_dir_all(&t).unwrap();
}

#[test]
fn a_tracked_file_in_the_target_directory_is_read_and_refused() {
    let t = scratch_repo(
        "harness-location-tracked-target",
        &[("target/debug/h.rs", "impl AdapterHarness for H {}\n")],
        &["target/debug/h.rs"],
    );
    let bad = tree_findings(&t, Some("target"), &[]);
    assert!(
        bad.iter()
            .any(|b| b.starts_with("target/debug/h.rs: tracked inside cargo's target directory")),
        "{bad:#?}"
    );
    std::fs::remove_dir_all(&t).unwrap();
}

// ---- cargo's target directory (PR #355 fourth review, N2) -------------------------------------

#[test]
fn cargos_target_directory_is_target_or_outside_the_repository() {
    let (_, target) = metadata();
    assert!(
        !matches!(target, TargetDir::Inside(_)),
        "cargo's target directory is {target:?}: inside the repository it may only be target/ \
         (a committed .cargo/config.toml target-dir, or CARGO_TARGET_DIR, would take a \
         directory out of the file set)"
    );
}

#[test]
fn the_target_directory_rule_refuses_one_inside_the_repository() {
    let root = Path::new("/r");
    assert_eq!(
        target_dir(root, &root.join("target")),
        TargetDir::RootTarget
    );
    assert_eq!(
        target_dir(root, Path::new("/elsewhere/t")),
        TargetDir::Outside
    );
    // The fourth review's N2 plant: `[build] target-dir = "adapters/codex/tests/zz"`.
    assert_eq!(
        target_dir(root, &root.join("adapters/codex/tests/zz")),
        TargetDir::Inside("adapters/codex/tests/zz".into())
    );
    assert_eq!(
        target_dir(root, &root.join("t")),
        TargetDir::Inside("t".into())
    );
}

// ---- what cargo compiles for an adapter (PR #355 fourth review, N1) --------------------------

/// No workspace member has a build script: a custom-build target, or a `build.rs` file at
/// all (even one a `build = false` key leaves out). None needs one, and one could write a
/// harness the file set never sees, into its own directory or an adapter's (PR #355
/// fourth review N1; fifth review, every member).
fn build_script_findings(root: &Path, p: &Dependent) -> Vec<String> {
    let mut found: Vec<String> = p
        .targets
        .iter()
        .filter(|(kinds, _)| kinds.iter().any(|k| k == "custom-build"))
        .map(|(_, src)| {
            format!(
                "{} ({}) has a build script, {}: no workspace member has one",
                p.name,
                p.dir,
                rel(root, src)
            )
        })
        .collect();
    if root.join(&p.dir).join("build.rs").exists() {
        found.push(format!(
            "{}: a build.rs file: no workspace member has one",
            p.dir
        ));
    }
    found
}

/// An adapter's lib target has `doctest = false`: a doctest is compiled with the
/// dev-dependencies, so one could hold a harness (PR #355 fifth review, D1).
fn doctest_findings(p: &Dependent) -> Vec<String> {
    if adapter_of(&p.dir).is_some() && p.lib_doctests {
        vec![format!(
            "{} ({}): its lib target has doctests on; set doctest = false under [lib]",
            p.name, p.dir
        )]
    } else {
        Vec::new()
    }
}

#[test]
fn adapter_libs_have_no_doctests() {
    let bad: Vec<String> = packages().iter().flat_map(doctest_findings).collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

/// Every test, example and bench target of an adapter is its harness file, or a file the
/// name rules pass.
fn target_findings(root: &Path, p: &Dependent, listed: &[String]) -> Vec<String> {
    if adapter_of(&p.dir).is_none() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for (kinds, src) in &p.targets {
        if !kinds
            .iter()
            .any(|k| matches!(k.as_str(), "test" | "example" | "bench"))
        {
            continue;
        }
        let src = std::fs::canonicalize(src).unwrap_or_else(|_| src.clone());
        if !src.starts_with(root) {
            found.push(format!(
                "{}: target {} lies outside the repository",
                p.name,
                src.display()
            ));
            continue;
        }
        let r = rel(root, &src);
        if r == format!("{}/{HARNESS_FILE}", p.dir) {
            continue;
        }
        let text = std::fs::read_to_string(&src).unwrap_or_default();
        for x in file_findings(scope(&r, listed), &text) {
            found.push(format!("{} target {r}: {x}", p.name));
        }
    }
    found
}

/// Every `.rs` file git ignores under `adapters/` in `root`. Cargo compiles what it finds
/// on disk, ignored or not.
fn ignored_rust_findings(root: &Path) -> Vec<String> {
    git_list(
        root,
        &["-o", "-i", "--exclude-standard", "-z", "--", "adapters/"],
    )
    .into_iter()
    .filter(|f| f.ends_with(".rs"))
    .map(|f| format!("{f}: a Rust file git ignores under adapters/"))
    .collect()
}

#[test]
fn no_workspace_member_has_a_build_script() {
    let root = repo_root();
    let bad: Vec<String> = packages()
        .iter()
        .flat_map(|p| build_script_findings(&root, p))
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn every_adapter_test_example_and_bench_target_keeps_the_rule() {
    let root = repo_root();
    let all = packages();
    let listed: Vec<String> = all
        .iter()
        .filter(|p| ALLOWED_DEPENDENTS.iter().any(|(a, _)| *a == p.name))
        .map(|p| p.dir.clone())
        .collect();
    let bad: Vec<String> = all
        .iter()
        .flat_map(|p| target_findings(&root, p, &listed))
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn no_git_ignored_rust_file_under_adapters() {
    let bad = ignored_rust_findings(&repo_root());
    assert!(bad.is_empty(), "{bad:#?}");
}

/// An adapter's normal and build dependencies are the vetted ones, by `cargo metadata`
/// (`source::VETTED_DEPENDENCIES`, the list the static scan vets them by identity against).
#[test]
fn adapters_take_only_vetted_normal_and_build_dependencies() {
    use oac_contract_adapter::source::VETTED_DEPENDENCIES;
    let bad: Vec<String> = packages()
        .iter()
        .filter(|p| adapter_of(&p.dir).is_some())
        .flat_map(|p| {
            p.other_deps
                .iter()
                .filter(|d| !VETTED_DEPENDENCIES.contains(&d.as_str()))
                .map(|d| format!("{} ({}) depends on {d}, which is not vetted", p.name, p.dir))
        })
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn a_build_script_and_its_ignored_harness_are_caught() {
    // The fourth review's N1 plant: a build script that writes tests/zz/main.rs, which a
    // committed tests/.gitignore hides from the file set; here as it stands after a build
    // ran it.
    let hidden = "use oac_contract_adapter::{AdapterHarness, run};\n#[test]\nfn hidden() {}\n";
    let t = scratch_repo(
        "harness-location-n1",
        &[
            (
                "adapters/codex/Cargo.toml",
                "[package]\nname = \"oac-adapter-codex\"\n\n[dev-dependencies]\noac-contract-adapter = { path = \"../../tests/protocol/contract/adapter\" }\n",
            ),
            (
                "adapters/codex/build.rs",
                "fn main() { let _ = std::fs::write(\"tests/zz/main.rs\", [\"use oac_contract\", \"_adapter\"].concat()); }\n",
            ),
            ("adapters/codex/tests/.gitignore", "zz/\n"),
            ("adapters/codex/tests/zz/main.rs", hidden),
        ],
        &[
            "adapters/codex/Cargo.toml",
            "adapters/codex/build.rs",
            "adapters/codex/tests/.gitignore",
        ],
    );
    let t = std::fs::canonicalize(&t).unwrap();
    // The file set alone does not see it: that is the regression.
    assert!(
        !tree_findings(&t, Some("target"), &[])
            .iter()
            .any(|b| b.contains("zz/main.rs")),
    );
    // Each of the three sides does.
    let codex = Dependent {
        name: "oac-adapter-codex".into(),
        dir: "adapters/codex".into(),
        edges: vec![(Some("dev".into()), None)],
        dev_deps: vec![SUITE.into()],
        other_deps: vec!["oac-core".into()],
        targets: vec![
            (
                vec!["custom-build".into()],
                t.join("adapters/codex/build.rs"),
            ),
            (
                vec!["test".into()],
                t.join("adapters/codex/tests/zz/main.rs"),
            ),
        ],
        lib_doctests: false,
    };
    let build = build_script_findings(&t, &codex);
    assert!(
        build.iter().any(|b| b.contains("has a build script"))
            && build.iter().any(|b| b.contains("a build.rs file")),
        "{build:#?}"
    );
    let targets = target_findings(&t, &codex, &[]);
    assert!(
        targets
            .iter()
            .any(|b| b.contains("adapters/codex/tests/zz/main.rs: names AdapterHarness")),
        "{targets:#?}"
    );
    let ignored = ignored_rust_findings(&t);
    assert!(
        ignored
            .iter()
            .any(|b| b.starts_with("adapters/codex/tests/zz/main.rs: a Rust file git ignores")),
        "{ignored:#?}"
    );
    // A harness file in its place is a target the rule passes.
    let ok = Dependent {
        targets: vec![(
            vec!["test".into()],
            t.join("adapters/codex/tests/contract.rs"),
        )],
        ..codex
    };
    std::fs::write(t.join("adapters/codex/tests/contract.rs"), hidden).unwrap();
    assert!(target_findings(&t, &ok, &[]).is_empty());
    // A build script in any other workspace member is refused too: one could write into
    // adapters/ (the fifth review's residual).
    let memory = Dependent {
        name: "oac-transport-memory".into(),
        dir: "transports/memory".into(),
        targets: vec![(
            vec!["custom-build".into()],
            t.join("transports/memory/build.rs"),
        )],
        ..ok
    };
    assert!(
        build_script_findings(&t, &memory)
            .iter()
            .any(|b| b.contains("has a build script")),
    );
    std::fs::remove_dir_all(&t).unwrap();
}

#[test]
fn a_doctest_that_includes_a_harness_is_caught() {
    // The fifth review's D1 plant: a doctest in adapter src/ that include!s a non-.rs file
    // holding the harness. Doctests build with the dev-dependencies, the static scan reads
    // code tokens (a doc comment is a string), and the name rule read only .rs files.
    let lib = "/// Notes.\n///\n/// ```\n/// include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/notes.txt\"));\n/// fn main() { hidden(); }\n/// ```\npub fn notes() {}\n";
    let notes = "struct H;\nfn hidden() {\n    let _run = oac_contract_adapter::run as fn(&mut dyn oac_contract_adapter::AdapterHarness) -> oac_contract_adapter::Report;\n}\n";
    let t = scratch_repo(
        "harness-location-d1",
        &[
            (
                "adapters/codex/Cargo.toml",
                "[package]\nname = \"oac-adapter-codex\"\n\n[dev-dependencies]\noac-contract-adapter = { path = \"../../tests/protocol/contract/adapter\" }\n",
            ),
            ("adapters/codex/src/lib.rs", lib),
            ("adapters/codex/notes.txt", notes),
        ],
        &[
            "adapters/codex/Cargo.toml",
            "adapters/codex/src/lib.rs",
            "adapters/codex/notes.txt",
        ],
    );
    let bad = tree_findings(&t, Some("target"), &[]);
    // Side 1: the hiding shapes, read from the raw text of an adapter src/ file.
    assert!(
        bad.iter()
            .any(|b| b.starts_with("adapters/codex/src/lib.rs: include!")),
        "{bad:#?}"
    );
    // Side 2: the name rule over a file of any extension.
    for want in [
        "adapters/codex/notes.txt: names AdapterHarness",
        "adapters/codex/notes.txt: names oac_contract_adapter",
    ] {
        assert!(bad.iter().any(|b| b == want), "missed {want}: {bad:#?}");
    }
    // Side 3: an adapter lib target with doctests on.
    let codex = Dependent {
        name: "oac-adapter-codex".into(),
        dir: "adapters/codex".into(),
        edges: vec![(Some("dev".into()), None)],
        dev_deps: vec![SUITE.into()],
        other_deps: vec!["oac-core".into()],
        targets: vec![(vec!["lib".into()], t.join("adapters/codex/src/lib.rs"))],
        lib_doctests: true,
    };
    assert!(!doctest_findings(&codex).is_empty());
    let off = Dependent {
        lib_doctests: false,
        ..codex
    };
    assert!(doctest_findings(&off).is_empty());
    // A lib target of a package that is not an adapter keeps its doctests.
    let core = Dependent {
        name: "oac-core".into(),
        dir: "core".into(),
        lib_doctests: true,
        ..off
    };
    assert!(doctest_findings(&core).is_empty());
    std::fs::remove_dir_all(&t).unwrap();
}

#[test]
fn an_aliased_include_of_a_harness_is_caught() {
    // The sixth review's E1 plant: an adapter test file aliases `include` and loads a
    // harness from a non-.rs file outside adapters/, which the name rule does not read.
    let notes = "#[test]\nfn hidden() {\n    let _run = oac_contract_adapter::run as fn(&mut dyn oac_contract_adapter::AdapterHarness) -> oac_contract_adapter::Report;\n}\n";
    let t = scratch_repo(
        "harness-location-e1",
        &[
            (
                "adapters/codex/Cargo.toml",
                "[package]\nname = \"oac-adapter-codex\"\n\n[dev-dependencies]\noac-contract-adapter = { path = \"../../tests/protocol/contract/adapter\" }\n",
            ),
            (
                "adapters/codex/tests/notes.rs",
                "use std::include as notes;\nnotes!(\"../../../docs/zz/notes.txt\");\n",
            ),
            ("docs/zz/notes.txt", notes),
        ],
        &[
            "adapters/codex/Cargo.toml",
            "adapters/codex/tests/notes.rs",
            "docs/zz/notes.txt",
        ],
    );
    let bad = tree_findings(&t, Some("target"), &[]);
    assert!(
        bad.iter()
            .any(|b| b.starts_with("adapters/codex/tests/notes.rs: the identifier `include`")),
        "{bad:#?}"
    );
    // The included file itself sits outside adapters/ and is not .rs, so only the word
    // rule on the including file catches the route.
    assert!(!bad.iter().any(|b| b.starts_with("docs/")), "{bad:#?}");
    std::fs::remove_dir_all(&t).unwrap();
}

// ---- the file rules -----------------------------------------------------------------------

/// Where a file sits, for the rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    /// `adapters/<name>/tests/contract.rs`.
    Harness,
    /// Any other file under an adapter's `tests/`.
    AdapterTest,
    /// Any other file of an adapter.
    AdapterOther,
    /// A file of a listed package or of `adapters/mcp-tools`.
    Listed,
    /// Anything else outside this crate.
    Other,
}

fn scope(rel_file: &str, listed_dirs: &[String]) -> Scope {
    if let Some(name) = adapter_of(rel_file) {
        let base = format!("adapters/{name}/");
        let inner = &rel_file[base.len()..];
        if inner == HARNESS_FILE {
            Scope::Harness
        } else if inner.starts_with("tests/") {
            Scope::AdapterTest
        } else {
            Scope::AdapterOther
        }
    } else if listed_dirs
        .iter()
        .any(|d| rel_file.starts_with(&format!("{d}/")))
        || NOT_ADAPTERS
            .iter()
            .any(|n| rel_file.starts_with(&format!("adapters/{n}/")))
    {
        Scope::Listed
    } else {
        Scope::Other
    }
}

/// `text` with every whitespace run collapsed to one space.
fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` with no whitespace at all.
fn dense(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// True when `word` occurs in `text` as a whole identifier.
fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
    })
}

/// Shapes that can hide what a file loads or names: `self as`, `macro_rules!`, `include!`
/// in any form, and a `#[path]` attribute.
fn hiding_shapes(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let s = squeeze(text);
    let d = dense(text);
    if has_word(&s, "self") && s.contains("self as ") {
        found.push("a `self as` import".to_owned());
    }
    if has_word(text, "macro_rules") {
        found.push("macro_rules!".to_owned());
    }
    for m in ["include!", "include_str!", "include_bytes!"] {
        if d.contains(m) {
            found.push(m.to_owned());
        }
    }
    // The identifiers themselves, as whole words, so an alias (`use std::include as x;`)
    // or a path form (`std::include!`) is refused too: the word rule `source.rs` applies to
    // adapter `src/` (PR #355 sixth review, E1). Its cost is the same: a binding may not be
    // named `include`, `include_str` or `include_bytes`, even in a comment.
    for w in ["include", "include_str", "include_bytes"] {
        if has_word(text, w) {
            found.push(format!(
                "the identifier `{w}`, which can load a file under any alias"
            ));
        }
    }
    let attrs = d.match_indices("#[").chain(d.match_indices("#!["));
    for (i, _) in attrs {
        let a = &d[i..d[i..].find(']').map_or(d.len(), |e| i + e + 1)];
        if a.contains("[path") || a.contains("(path") || a.contains(",path") || a.contains("path=")
        {
            found.push(format!("a path attribute: {a}"));
        }
    }
    found
}

/// How a file reaches the suite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reach {
    /// The suite's `run`.
    Run,
    /// All of the suite's items, by a glob.
    Glob,
    /// The suite crate or the trait under another name.
    Rename,
}

/// How `text` reaches the suite's `run`, all of its items, or renames it or the trait.
fn reaching_shapes(text: &str) -> Vec<(Reach, String)> {
    let mut found = Vec::new();
    let s = squeeze(text);
    if s.contains(&format!("{SUITE_CRATE} as ")) {
        found.push((Reach::Rename, format!("renames {SUITE_CRATE}")));
    }
    if s.contains(&format!("{HARNESS_TRAIT} as ")) {
        found.push((Reach::Rename, format!("renames {HARNESS_TRAIT}")));
    }
    let d = dense(text);
    let path = format!("{SUITE_CRATE}::");
    let mut rest = d.as_str();
    while let Some(i) = rest.find(&path) {
        rest = &rest[i + path.len()..];
        let word: String = rest.chars().take_while(|c| is_ident(*c)).collect();
        if word == "run" {
            found.push((Reach::Run, format!("reaches {path}run")));
        } else if rest.starts_with('*') {
            found.push((Reach::Glob, format!("reaches {path}*")));
        } else if rest.starts_with('{') {
            let group = &rest[..rest.find('}').map_or(rest.len(), |e| e + 1)];
            let words: Vec<&str> = group.split(|c: char| !(is_ident(c) || c == '*')).collect();
            if words.contains(&"run") {
                found.push((Reach::Run, format!("reaches the suite's run: {group}")));
            }
            if words.contains(&"*") {
                found.push((
                    Reach::Glob,
                    format!("reaches all the suite's items: {group}"),
                ));
            }
        }
    }
    found
}

/// What implements the trait: a line naming it after `impl`.
fn implementing_shapes(text: &str) -> Vec<String> {
    squeeze(text)
        .split(['{', '}', ';'])
        .filter(|chunk| {
            chunk.contains(HARNESS_TRAIT)
                && chunk.split(|c: char| !is_ident(c)).any(|w| w == "impl")
        })
        .map(|chunk| format!("implements {HARNESS_TRAIT}: {}", chunk.trim()))
        .collect()
}

/// A file module (`mod x;`), which the harness file may not have.
fn file_modules(text: &str) -> Vec<String> {
    let s = squeeze(text);
    let mut found = Vec::new();
    for (i, _) in s.match_indices("mod ") {
        if s[..i].chars().next_back().is_some_and(is_ident) {
            continue;
        }
        let rest = &s[i + 4..];
        let name: String = rest.chars().take_while(|c| is_ident(*c)).collect();
        if !name.is_empty() && rest[name.len()..].trim_start().starts_with(';') {
            found.push(format!("a file module: mod {name};"));
        }
    }
    found
}

fn names_either(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    if text.contains(HARNESS_TRAIT) {
        found.push(format!("names {HARNESS_TRAIT}"));
    }
    if text.contains(SUITE_CRATE) {
        found.push(format!("names {SUITE_CRATE}"));
    }
    found
}

/// What the rules find in a file at `scope` holding `text`.
fn file_findings(scope: Scope, text: &str) -> Vec<String> {
    match scope {
        Scope::Harness => {
            let mut f = hiding_shapes(text);
            f.extend(file_modules(text));
            f.extend(
                reaching_shapes(text)
                    .into_iter()
                    .filter(|(r, _)| *r != Reach::Run)
                    .map(|(_, x)| x),
            );
            f
        }
        Scope::AdapterTest => {
            let mut f = names_either(text);
            f.extend(hiding_shapes(text));
            f
        }
        // An adapter's other files: the hiding shapes too, read from raw text, so a doc
        // comment's doctest is covered (PR #355 fifth review, D1).
        Scope::AdapterOther => {
            let mut f = names_either(text);
            f.extend(hiding_shapes(text));
            f
        }
        Scope::Other => names_either(text),
        Scope::Listed => {
            let mut f = hiding_shapes(text);
            f.extend(reaching_shapes(text).into_iter().map(|(_, x)| x));
            f.extend(implementing_shapes(text));
            f
        }
    }
}

#[test]
fn every_file_outside_the_suite_keeps_the_harness_rule() {
    let root = repo_root();
    let (all, target) = metadata();
    let listed: Vec<String> = all
        .iter()
        .filter(|p| ALLOWED_DEPENDENTS.iter().any(|(a, _)| *a == p.name))
        .map(|p| p.dir.clone())
        .collect();
    // Untracked files are dropped from target/ only; any other target directory inside
    // the repository fails cargos_target_directory_is_target_or_outside_the_repository.
    let target = (target == TargetDir::RootTarget).then_some("target");
    let files = git_files(&root, target);
    assert!(
        files.iter().any(|f| f == "core/src/adapter.rs"),
        "git listed no core/src/adapter.rs: {} file(s)",
        files.len()
    );
    let bad = tree_findings(&root, target, &listed);
    assert!(
        bad.is_empty(),
        "the harness a real adapter runs under lives at adapters/<name>/{HARNESS_FILE} only \
         (src/lib.rs, \"Where a harness lives\"):\n{bad:#?}"
    );
}

#[test]
fn the_scopes_are_where_the_rule_says() {
    let listed = vec!["transports/memory".to_owned()];
    for (p, want) in [
        ("adapters/claude/tests/contract.rs", Scope::Harness),
        ("adapters/codex/tests/contract.rs", Scope::Harness),
        ("adapters/codex/tests/other.rs", Scope::AdapterTest),
        (
            "adapters/codex/tests/contract/helpers.rs",
            Scope::AdapterTest,
        ),
        ("adapters/codex/tests/target/h.rs", Scope::AdapterTest),
        ("adapters/codex/src/lib.rs", Scope::AdapterOther),
        ("adapters/codex/contract.rs", Scope::AdapterOther),
        ("adapters/mcp-tools/tests/contract.rs", Scope::Listed),
        ("transports/memory/tests/pipelines.rs", Scope::Listed),
        ("transports/memory-x/tests/a.rs", Scope::Other),
        ("cli/tests/contract.rs", Scope::Other),
    ] {
        assert_eq!(scope(p, &listed), want, "{p}");
    }
}

#[test]
fn the_file_rules_catch_each_planted_shape() {
    // (scope, planted text): each must be found.
    let planted: &[(Scope, &str)] = &[
        // A listed package (the PR #355 review's B2 plants).
        (
            Scope::Listed,
            "use oac_contract_adapter::{self as suite};\nfn f() { suite::run(h); }",
        ),
        (
            Scope::Listed,
            "macro_rules! imp { ($t:path) => { impl $t for H {} } }\nimp!(oac_contract_adapter::AdapterHarness);",
        ),
        (Scope::Listed, "include!(\"target/h.rs\");"),
        (Scope::Listed, "include_str!(\"h.rs\");"),
        (Scope::Listed, "#[path = \"target/h.rs\"]\nmod h;"),
        (Scope::Listed, "#[cfg_attr(test, path = \"h.rs\")]\nmod h;"),
        (Scope::Listed, "impl AdapterHarness for H {}"),
        (
            Scope::Listed,
            "impl oac_contract_adapter::AdapterHarness\n    for H {}",
        ),
        (
            Scope::Listed,
            "use oac_contract_adapter::AdapterHarness as Harness;",
        ),
        (Scope::Listed, "let r = oac_contract_adapter::run(&mut h);"),
        (Scope::Listed, "use oac_contract_adapter::{CoreSide, run};"),
        (Scope::Listed, "use oac_contract_adapter::*;"),
        (Scope::Listed, "use oac_contract_adapter::{claude, *};"),
        (Scope::Listed, "use oac_contract_adapter as suite;"),
        (Scope::Listed, "extern crate oac_contract_adapter as suite;"),
        // An adapter's other test files.
        (Scope::AdapterTest, "use oac_contract_adapter::run;"),
        (Scope::AdapterTest, "impl X for Y {} // AdapterHarness"),
        (Scope::AdapterTest, "macro_rules! m { () => {} }"),
        (Scope::AdapterTest, "include!(\"../h.txt\");"),
        (Scope::AdapterTest, "use crate::{self as me};"),
        (Scope::AdapterTest, "#[path = \"h.txt\"]\nmod h;"),
        // The harness file.
        (Scope::Harness, "use oac_contract_adapter::{self as s};"),
        (Scope::Harness, "macro_rules! m { () => {} }"),
        (Scope::Harness, "include!(\"h.rs\");"),
        (Scope::Harness, "#[path = \"h.rs\"]\nmod h;"),
        (Scope::Harness, "mod helpers;"),
        (Scope::Harness, "use oac_contract_adapter::*;"),
        (Scope::Harness, "use oac_contract_adapter as suite;"),
        (
            Scope::Harness,
            "use oac_contract_adapter::AdapterHarness as H;",
        ),
        // An adapter's other files, and everything else.
        (
            Scope::AdapterOther,
            "use oac_contract_adapter::AdapterHarness;",
        ),
        // The sixth review's E1: an aliased include, in each scope it reaches.
        (
            Scope::AdapterTest,
            "use std::include as notes;\nnotes!(\"../../../docs/zz/notes.txt\");",
        ),
        (Scope::AdapterTest, "std::include ! (\"../h.txt\");"),
        (
            Scope::Harness,
            "use std::include_str as text;\nconst T: &str = text!(\"h.txt\");",
        ),
        (
            Scope::AdapterOther,
            "use std::include_str as text;\n#[doc = text!(\"../n.md\")]\npub fn f() {}",
        ),
        (
            Scope::Listed,
            "use core::include_bytes as b;\nstatic B: &[u8] = b!(\"h.bin\");",
        ),
        (Scope::Listed, "use std::{include as i};"),
        // The fifth review's D1: a doctest in a doc comment.
        (
            Scope::AdapterOther,
            "/// ```\n/// include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/notes.txt\"));\n/// ```\npub fn notes() {}",
        ),
        (
            Scope::AdapterOther,
            "//! ```\n//! macro_rules! m { () => {} }\n//! ```",
        ),
        (Scope::Other, "struct H; impl AdapterHarness for H {}"),
        (Scope::Other, "fn f() { oac_contract_adapter::run(h) }"),
    ];
    for (s, text) in planted {
        assert!(
            !file_findings(*s, text).is_empty(),
            "missed in {s:?}: {text}"
        );
    }
    // (scope, clean text): none may be found.
    let clean: &[(Scope, &str)] = &[
        (
            Scope::Listed,
            "use oac_contract_adapter::{AdapterHarness, CoreSide, HarnessRequest};",
        ),
        (
            Scope::Listed,
            "use oac_contract_adapter::claude::{ClaudeHarness, pipe};",
        ),
        (Scope::Listed, "claude.start_turn(0).unwrap();"),
        (
            Scope::Listed,
            "use oac_contract_adapter::{runner_free, CoreSide};",
        ),
        (
            Scope::Listed,
            "let path = dir.join(\"x\"); let included = 1;",
        ),
        (Scope::Listed, "impl ProviderAdapter for ChannelAdapter {}"),
        (
            Scope::Harness,
            "use oac_contract_adapter::{AdapterHarness, CoreSide, run};\nstruct H;\nimpl AdapterHarness for H {}\n#[test]\nfn t() { run(&mut H).assert_conformant(); }\nmod inline { pub fn f() {} }",
        ),
        (Scope::Harness, "let r = oac_contract_adapter::run(&mut h);"),
        (
            Scope::AdapterTest,
            "#[test]\nfn plain() { assert_eq!(1, 1); }",
        ),
        (Scope::AdapterOther, "pub struct Adapter;"),
        (Scope::Other, "pub fn harness() {}"),
    ];
    for (s, text) in clean {
        let f = file_findings(*s, text);
        assert!(f.is_empty(), "flagged in {s:?}: {text}: {f:?}");
    }
}
