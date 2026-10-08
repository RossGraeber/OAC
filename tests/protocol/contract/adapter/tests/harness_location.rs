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
//! **The files read.** These come from git, not from a directory walk: `git ls-files -co
//! --exclude-standard`, so every tracked file and every untracked file that is not ignored,
//! less cargo's own `target_directory` from `cargo metadata`. Any `CACHEDIR.TAG` left in that
//! set fails. A real target directory is never tracked, and a committed tag would hide a
//! directory from the tools that honour it (PR #355 third review, C1). A symlink fails too.

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
}

/// Every workspace package: its name, its repository-relative directory, its edges to the
/// suite and its dev-dependencies.
fn packages() -> Vec<Dependent> {
    metadata().0
}

/// The packages, and cargo's `target_directory` relative to the repository root when it
/// lies inside it.
fn metadata() -> (Vec<Dependent>, Option<String>) {
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
    let target = get_str(&meta, "target_directory")
        .and_then(|t| std::fs::canonicalize(t).ok())
        .filter(|t| t.starts_with(&root))
        .map(|t| rel(&root, &t));
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
            Dependent {
                name: get_str(p, "name").unwrap_or_default().to_owned(),
                dir,
                edges,
                dev_deps,
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
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-co", "--exclude-standard", "-z"])
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
        .filter(|f| !target.is_some_and(|t| f.starts_with(&format!("{t}/"))))
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
        ("t2/CACHEDIR.TAG", TAG),
        ("t2/debug/h.rs", HIDDEN),
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
    // t2/ plays a CARGO_TARGET_DIR inside the checkout that .gitignore does not cover.
    let bad = tree_findings(&t, Some("t2"), &[]);
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
        !bad.iter()
            .any(|b| b.starts_with("target/") || b.starts_with("t2/")),
        "read cargo's target directory: {bad:#?}"
    );
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
        Scope::AdapterOther | Scope::Other => names_either(text),
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
    let files = git_files(&root, target.as_deref());
    assert!(
        files.iter().any(|f| f == "core/src/adapter.rs"),
        "git listed no core/src/adapter.rs: {} file(s)",
        files.len()
    );
    let bad = tree_findings(&root, target.as_deref(), &listed);
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
