// SPDX-License-Identifier: Apache-2.0

//! The threat map ([`THREATS`]) against the sources, `spec/security.md` §13 and
//! `09-test-strategy.md` §12 (acceptance item 5 of #60: each test names the row it proves).
//!
//! These are the static checks; `tests/security/check-compiled-tests.mjs` (run in CI) checks
//! the same map against the tests cargo actually compiled, which no parser can see fully.
//!
//! - Every 06 §14 row and every `spec/security.md` §13 row is mapped.
//! - `tests/` holds only `.rs` files, found by listing it; each is parsed with `syn`, so a
//!   test indented or inside a module is seen. No `#[path]` and no item macro
//!   (`macro_rules!` or an invocation) may appear, and `src/` holds no test.
//! - Attributes are matched on the last segment of their path, so `#[core::...::test]` is a
//!   test too. No test, and no module holding one, may carry `#[cfg(...)]`, directly or
//!   through `cfg_attr`, and no function may be a test only through `#[cfg_attr(.., test)]`
//!   (#329): whether it is a test then depends on a cfg this parser cannot evaluate.
//! - No test escapes both this parser and cargo's listing (#329): the library's doc-tests are
//!   off (`[lib] doctest = false`) and no target sets `harness`.
//! - Every named test, fact and core test exists; every `#[test]` here is mapped; a fact is
//!   never also a proof, in any row; a cited core test is not ignored.
//! - A gated test is `#[ignore = "GATED on #N ..."]` with an issue of its row, and its body
//!   is a single `std::panic!` (a bare `panic!` could be a local macro), so it fails if run.
//!   No other test is ignored, by `#[ignore]` or by `#[cfg_attr(..., ignore)]`, and none is
//!   `#[should_panic]`.
//! - The table in 09 is the one this map renders.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use oac_security_suite::{Status, THREATS};
use syn::visit::Visit;

const BEGIN: &str =
    "<!-- F11 threat map: generated from tests/security/src/lib.rs THREATS; begin -->";
const END: &str = "<!-- F11 threat map: end -->";

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
}

/// One function, as the parser sees it.
#[derive(Debug, Default)]
struct Fn {
    file: String,
    is_test: bool,
    /// `Some(reason)` for `#[ignore = "reason"]`, `Some("")` for a bare `#[ignore]`.
    ignore: Option<String>,
    cfg_attr_ignore: bool,
    /// A `#[cfg_attr(.., test)]` (any spelling of the test attribute, nested or not): whether
    /// it is a test at all depends on a cfg this parser cannot evaluate (#329).
    cfg_attr_test: bool,
    should_panic: bool,
    /// The function, or a module around it, carries `#[cfg(...)]`.
    cfg: bool,
    /// The body is exactly one `std::panic!(...)`.
    panics_only: bool,
}

/// What a parsed file holds besides functions: things that hide tests from the parser.
#[derive(Debug, Default)]
struct Hazards {
    path_attrs: Vec<String>,
    item_macros: Vec<String>,
}

struct Collect<'a> {
    file: &'a str,
    fns: &'a mut BTreeMap<String, Vec<Fn>>,
    hazards: &'a mut Hazards,
    /// Depth of enclosing modules that carry `#[cfg]`.
    cfg_mods: usize,
}

fn last(a: &syn::Attribute) -> String {
    a.path()
        .segments
        .last()
        .map(|s| syn::ext::IdentExt::unraw(&s.ident).to_string())
        .unwrap_or_default()
}

/// The attributes a `cfg_attr(predicate, attr, ..)` list applies, by the last segment of
/// each one's path, nested `cfg_attr`s included: `cfg_attr(any(), core::prelude::v1::test)`
/// gives `["test"]`. A list that does not parse as metas gives `["test"]` too, so it is
/// refused rather than read as harmless.
fn cfg_attr_names(list: &syn::MetaList) -> Vec<String> {
    let parser = syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated;
    let Ok(metas) = list.parse_args_with(parser) else {
        return vec!["test".to_owned()];
    };
    let mut names = Vec::new();
    // The first meta is the predicate.
    for meta in metas.iter().skip(1) {
        let name = meta
            .path()
            .segments
            .last()
            .map(|s| syn::ext::IdentExt::unraw(&s.ident).to_string())
            .unwrap_or_default();
        if let (true, syn::Meta::List(inner)) = (name == "cfg_attr", meta) {
            names.extend(cfg_attr_names(inner));
        }
        names.push(name);
    }
    names
}

fn is_std_panic(mac: &syn::Macro) -> bool {
    let segs: Vec<String> = mac
        .path
        .segments
        .iter()
        .map(|s| syn::ext::IdentExt::unraw(&s.ident).to_string())
        .collect();
    segs == ["std", "panic"]
}

impl<'ast> Visit<'ast> for Collect<'_> {
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        // `#[cfg]`, or a `cfg` applied through `cfg_attr`.
        let cfg = m.attrs.iter().any(|a| {
            last(a) == "cfg"
                || matches!(&a.meta, syn::Meta::List(l)
                    if last(a) == "cfg_attr" && cfg_attr_names(l).iter().any(|n| n == "cfg"))
        });
        for a in &m.attrs {
            if last(a) == "path" {
                self.hazards
                    .path_attrs
                    .push(format!("{}: mod {}", self.file, m.ident));
            }
        }
        self.cfg_mods += usize::from(cfg);
        syn::visit::visit_item_mod(self, m);
        self.cfg_mods -= usize::from(cfg);
    }

    fn visit_item_macro(&mut self, m: &'ast syn::ItemMacro) {
        let what = m
            .mac
            .path
            .segments
            .last()
            .map(|s| syn::ext::IdentExt::unraw(&s.ident).to_string())
            .unwrap_or_default();
        self.hazards
            .item_macros
            .push(format!("{}: {what}!", self.file));
        syn::visit::visit_item_macro(self, m);
    }

    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        let mut out = Fn {
            file: self.file.to_owned(),
            cfg: self.cfg_mods > 0,
            ..Fn::default()
        };
        for a in &f.attrs {
            match last(a).as_str() {
                "test" => out.is_test = true,
                "ignore" => {
                    out.ignore = Some(match &a.meta {
                        syn::Meta::NameValue(nv) => match &nv.value {
                            syn::Expr::Lit(syn::ExprLit {
                                lit: syn::Lit::Str(s),
                                ..
                            }) => s.value(),
                            _ => String::new(),
                        },
                        _ => String::new(),
                    });
                }
                "cfg_attr" => {
                    if let syn::Meta::List(l) = &a.meta {
                        for n in cfg_attr_names(l) {
                            match n.as_str() {
                                "ignore" => out.cfg_attr_ignore = true,
                                "should_panic" => out.should_panic = true,
                                "cfg" => out.cfg = true,
                                "test" => {
                                    out.is_test = true;
                                    out.cfg_attr_test = true;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "cfg" => out.cfg = true,
                "should_panic" => out.should_panic = true,
                _ => {}
            }
        }
        out.panics_only = match f.block.stmts.as_slice() {
            [syn::Stmt::Macro(m)] => is_std_panic(&m.mac),
            [syn::Stmt::Expr(syn::Expr::Macro(m), _)] => is_std_panic(&m.mac),
            _ => false,
        };
        self.fns
            .entry(syn::ext::IdentExt::unraw(&f.sig.ident).to_string())
            .or_default()
            .push(out);
        syn::visit::visit_item_fn(self, f);
    }
}

fn parse_into(
    path: &Path,
    label: &str,
    fns: &mut BTreeMap<String, Vec<Fn>>,
    hazards: &mut Hazards,
) {
    let file = syn::parse_file(&read(path)).unwrap_or_else(|e| panic!("{label}: {e}"));
    Collect {
        file: label,
        fns,
        hazards,
        cfg_mods: 0,
    }
    .visit_file(&file);
}

/// Every function in every `tests/*.rs` file of this crate except this one, found by
/// listing the directory, and the hazards in them.
fn suite_fns_and_hazards() -> (BTreeMap<String, Vec<Fn>>, Hazards) {
    let dir = manifest_dir().join("tests");
    let mut fns = BTreeMap::new();
    let mut hazards = Hazards::default();
    let mut files = 0;
    for e in std::fs::read_dir(&dir).expect("tests/") {
        let path = e.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            path.is_file() && path.extension().is_some_and(|x| x == "rs"),
            "tests/{name}: only .rs files may sit in tests/ (a directory is a test target cargo finds by itself)"
        );
        if name != "threat_map.rs" {
            parse_into(&path, &name, &mut fns, &mut hazards);
            files += 1;
        }
    }
    assert!(
        files >= 12,
        "found only {files} test files in {}",
        dir.display()
    );
    (fns, hazards)
}

fn suite_fns() -> BTreeMap<String, Vec<Fn>> {
    suite_fns_and_hazards().0
}

/// The one `#[test]` function named `name`.
fn the_test<'a>(fns: &'a BTreeMap<String, Vec<Fn>>, name: &str) -> &'a Fn {
    let found: Vec<&Fn> = fns
        .get(name)
        .map(|v| v.iter().filter(|f| f.is_test).collect())
        .unwrap_or_default();
    assert_eq!(found.len(), 1, "test {name} is not defined exactly once");
    found[0]
}

fn is_gated(name: &str) -> bool {
    name.starts_with("gated_")
}

/// Nothing in the test files can hide a test from this parser, and `src/` holds no test.
#[test]
fn no_test_is_hidden_from_the_parser() {
    let (fns, hazards) = suite_fns_and_hazards();
    assert!(
        hazards.path_attrs.is_empty(),
        "#[path] modules: {:?}",
        hazards.path_attrs
    );
    assert!(
        hazards.item_macros.is_empty(),
        "item macros (they can generate tests): {:?}",
        hazards.item_macros
    );
    for (name, defs) in &fns {
        for f in defs {
            assert!(
                !(f.is_test && f.cfg),
                "{}: {name} or a module around it carries #[cfg]; a proof must compile everywhere",
                f.file
            );
            assert!(
                !f.cfg_attr_test,
                "{}: {name} is a test only under #[cfg_attr(.., test)]; whether it runs depends on a cfg this check cannot evaluate (#329)",
                f.file
            );
        }
    }
    let mut src = Vec::new();
    let mut stack = vec![manifest_dir().join("src")];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                src.push(p);
            }
        }
    }
    for p in src {
        let mut fns = BTreeMap::new();
        let mut hz = Hazards::default();
        let label = p.display().to_string();
        parse_into(&p, &label, &mut fns, &mut hz);
        for (name, defs) in &fns {
            assert!(
                !defs.iter().any(|f| f.is_test),
                "{label}: test {name} in src/"
            );
        }
        assert!(hz.path_attrs.is_empty(), "{:?}", hz.path_attrs);
    }
}

/// The crate has no test that neither listing sees (#329): its library's doc-tests are off
/// (`[lib] doctest = false`), and no target sets `harness`, since a `harness = false` target
/// runs its own `main` and lists nothing.
#[test]
fn no_test_target_escapes_the_listing() {
    let manifest = read(&manifest_dir().join("Cargo.toml"));
    let mut table = String::new();
    let mut lib_doctest_off = false;
    for raw in manifest.lines() {
        // Cut a comment: `#` outside a string (the manifest has no `#` inside strings).
        let line = raw.split('#').next().unwrap_or_default().trim();
        if line.starts_with('[') {
            table = line.to_owned();
            continue;
        }
        let key = line.split('=').next().unwrap_or_default().trim();
        assert!(
            !line.contains("harness"),
            "Cargo.toml {table}: `{line}`: a harness setting; a harness = false target lists no tests, so none of its tests could count (#329)"
        );
        if table == "[lib]" && key == "doctest" {
            lib_doctest_off = line.split('=').nth(1).map(str::trim) == Some("false");
        }
    }
    assert!(
        lib_doctest_off,
        "Cargo.toml must set `[lib]` `doctest = false`: a doc-test is a test no listing check sees (#329)"
    );
}

#[test]
fn every_06_row_is_mapped_once() {
    for n in 1..=24 {
        let row = format!("06-{n}");
        assert_eq!(THREATS.iter().filter(|t| t.row == row).count(), 1, "{row}");
    }
    let mut rows: Vec<&str> = THREATS.iter().map(|t| t.row).collect();
    rows.sort_unstable();
    rows.dedup();
    assert_eq!(rows.len(), THREATS.len(), "a row is mapped twice");
    for t in THREATS {
        assert!(
            t.row.starts_with("06-") || t.row.starts_with("S13-") || t.row.starts_with("X-"),
            "{}",
            t.row
        );
    }
}

/// The attack cell of every row of `spec/security.md` §13's table. Every line of the
/// section that starts with `|` is a table line, spaced or compact; each row must split into
/// the five cells of the template, at `|` not escaped as `\|`.
fn spec13_attacks() -> Vec<String> {
    let spec = read(&manifest_dir().join("../../spec/security.md"));
    let start = spec
        .find("## 13. Threat-to-requirement traceability")
        .expect("§13");
    let end = spec[start..].find("### 13.1").expect("§13.1") + start;
    let mut rows = Vec::new();
    for line in spec[start..end]
        .lines()
        .filter(|l| l.trim_start().starts_with('|'))
    {
        let line = line.trim();
        let mut cells = Vec::new();
        let mut cur = String::new();
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' && chars.peek() == Some(&'|') {
                cur.push('|');
                chars.next();
            } else if c == '|' {
                cells.push(std::mem::take(&mut cur));
            } else {
                cur.push(c);
            }
        }
        // A row is `| a | b | c | d | e |`: an empty cell before the first bar and after
        // the last.
        let cells: Vec<String> = cells.iter().skip(1).map(|c| c.trim().to_owned()).collect();
        assert_eq!(
            cells.len(),
            5,
            "§13 table line has {} cells: {line}",
            cells.len()
        );
        if cells[0] == "Attack" || cells[0].chars().all(|c| c == '-' || c == ':') {
            continue;
        }
        rows.push(cells[0].clone());
    }
    assert!(rows.len() >= 30, "§13 has {} rows?", rows.len());
    rows
}

#[test]
fn every_spec_13_row_is_mapped() {
    let attacks = spec13_attacks();
    for a in &attacks {
        let by: Vec<&str> = THREATS
            .iter()
            .filter(|t| t.spec13.iter().any(|p| a.starts_with(p)))
            .map(|t| t.row)
            .collect();
        assert!(
            !by.is_empty(),
            "spec/security.md §13 row {a:?} is in no threat entry"
        );
    }
    for t in THREATS {
        for p in t.spec13 {
            let n = attacks.iter().filter(|a| a.starts_with(p)).count();
            assert_eq!(n, 1, "{}: {p:?} matches {n} §13 rows, not one", t.row);
        }
        if t.row.starts_with("S13-") {
            assert!(!t.spec13.is_empty(), "{} names no §13 row", t.row);
        }
        if t.row.starts_with("X-") {
            assert!(t.spec13.is_empty(), "{} claims a §13 row", t.row);
        }
    }
}

#[test]
fn every_mapped_test_exists_and_every_test_is_mapped() {
    let fns = suite_fns();
    for t in THREATS {
        for name in t.tests {
            let f = the_test(&fns, name);
            if is_gated(name) {
                let reason = f
                    .ignore
                    .as_deref()
                    .unwrap_or_else(|| panic!("{name} is gated but not ignored"));
                assert!(
                    t.gated.iter().any(|g| g
                        .issue
                        .split(", ")
                        .any(|i| reason.starts_with(&format!("GATED on {i} ")))),
                    "{name}: {reason:?} does not name an issue of {}'s gates",
                    t.row
                );
                assert!(
                    f.panics_only,
                    "{name}: a gated body must be a single std::panic!"
                );
            } else {
                assert!(
                    f.ignore.is_none(),
                    "{name} proves a mitigation, so it must run"
                );
            }
        }
        for name in t.facts {
            assert!(!is_gated(name), "{name}: a fact runs");
            let proof_of: Vec<&str> = THREATS
                .iter()
                .filter(|u| u.tests.contains(name))
                .map(|u| u.row)
                .collect();
            assert!(
                proof_of.is_empty(),
                "{name}: a fact of {} is listed as a proof of {proof_of:?}",
                t.row
            );
            assert!(the_test(&fns, name).ignore.is_none(), "{name} must run");
        }
    }
    for (name, defs) in &fns {
        for f in defs.iter().filter(|f| f.is_test) {
            assert!(
                THREATS
                    .iter()
                    .any(|t| t.tests.contains(&name.as_str()) || t.facts.contains(&name.as_str())),
                "{}: {name} is in no threat row",
                f.file
            );
            assert!(
                !f.cfg_attr_ignore,
                "{}: {name} is ignored by cfg_attr",
                f.file
            );
            assert!(!f.should_panic, "{}: {name} is should_panic", f.file);
            assert!(
                f.ignore.is_none() || is_gated(name),
                "{}: {name} is ignored but not a gated_ placeholder",
                f.file
            );
        }
    }
}

/// Each test's name starts with the row it proves (`rowNN_` for 06 row NN, `s13_` for a
/// `spec/security.md` §13 row, `x_` for an `X-` row, `gated_` for a placeholder), and that
/// row lists it.
#[test]
fn every_test_name_starts_with_a_row() {
    let fns = suite_fns();
    let lists =
        |t: &oac_security_suite::Threat, n: &str| t.tests.contains(&n) || t.facts.contains(&n);
    for (name, defs) in &fns {
        if !defs.iter().any(|f| f.is_test) {
            continue;
        }
        let n = name.as_str();
        if let Some(rest) = n.strip_prefix("row") {
            assert!(
                rest.len() > 2 && rest[..2].bytes().all(|b| b.is_ascii_digit()),
                "{n}"
            );
            let row = format!("06-{}", rest[..2].trim_start_matches('0'));
            let t = THREATS
                .iter()
                .find(|t| t.row == row)
                .unwrap_or_else(|| panic!("{n}: no {row}"));
            assert!(lists(t, n), "{n} names {row} but {row} does not list it");
        } else if n.starts_with("s13_") {
            assert!(
                THREATS
                    .iter()
                    .any(|t| t.row.starts_with("S13-") && lists(t, n)),
                "{n} is in no S13 row"
            );
        } else if n.starts_with("x_") {
            assert!(
                THREATS
                    .iter()
                    .any(|t| t.row.starts_with("X-") && lists(t, n)),
                "{n} is in no X row"
            );
        } else {
            assert!(is_gated(n), "{n} does not name its threat row");
        }
    }
}

/// Every cited core test exists in `oac-core`: `module::...::name` in `core/src/<module>.rs`,
/// `tests/<file>::name` in `core/tests/<file>`.
#[test]
fn every_cited_core_test_exists() {
    let core = manifest_dir().join("../../core");
    for t in THREATS {
        for c in t.core_tests {
            let (path, name) = match c.split_once("::") {
                Some((file, rest)) if file.starts_with("tests/") => (core.join(file), rest),
                Some((module, rest)) => (
                    core.join("src").join(format!("{module}.rs")),
                    rest.rsplit("::").next().unwrap(),
                ),
                None => panic!("{}: {c} is not a path", t.row),
            };
            let mut fns = BTreeMap::new();
            parse_into(&path, c, &mut fns, &mut Hazards::default());
            let f = the_test(&fns, name);
            assert!(
                f.ignore.is_none() && !f.cfg_attr_ignore,
                "{}: {c} is ignored",
                t.row
            );
            // (Core's unit tests sit in `#[cfg(test)]` modules, so `f.cfg` is expected here.)
            assert!(
                !f.cfg_attr_test,
                "{}: {c} is a test only under #[cfg_attr(.., test)]",
                t.row
            );
        }
    }
}

#[test]
fn statuses_match_what_runs() {
    for t in THREATS {
        let passing = t.tests.iter().filter(|n| !is_gated(n)).count();
        let gated = t.tests.iter().filter(|n| is_gated(n)).count();
        match t.status {
            Status::Proven => assert!(passing > 0, "{}: proven by nothing", t.row),
            Status::DecisionProven => {
                assert!(passing > 0, "{}: proven by nothing", t.row);
                assert!(gated > 0 && !t.gated.is_empty(), "{}: nothing gated", t.row);
            }
            Status::Gated => {
                assert_eq!(passing, 0, "{}: a gated row lists a proof", t.row);
                assert!(gated > 0 && !t.gated.is_empty(), "{}", t.row);
            }
            Status::OpenRisk => assert!(
                t.tests.is_empty() && t.facts.is_empty() && t.gated.is_empty(),
                "{}",
                t.row
            ),
        }
        if gated > 0 {
            assert!(!t.gated.is_empty(), "{}: a gated test with no gate", t.row);
        }
        for g in t.gated {
            assert!(g.issue.starts_with('#'), "{}: {}", t.row, g.issue);
        }
    }
}

fn cell(items: Vec<String>) -> String {
    if items.is_empty() {
        "none".to_owned()
    } else {
        items.join("<br>")
    }
}

/// The 09 §12 F11 table, as this map renders it.
fn table() -> String {
    let mut s = String::from(
        "| Threat row | Attack | Proving tests (`tests/security/`, passing) | Harness facts (precondition, not proof) | Core tests (`oac-core`) | Gated, with owning issue | Status |\n|---|---|---|---|---|---|---|\n",
    );
    let code = |n: &&str| format!("`{n}`");
    for t in THREATS {
        let status = match t.status {
            Status::Proven if t.gated.is_empty() => "proven",
            Status::Proven => "proven (core and fakes); gated part open",
            Status::DecisionProven => "core decision proven; pairing key gated (G9)",
            Status::Gated => "gated: v0.1 gap",
            Status::OpenRisk => "open risk (06 §15)",
        };
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            t.row,
            t.attack,
            cell(t.tests.iter().filter(|n| !is_gated(n)).map(code).collect()),
            cell(t.facts.iter().map(code).collect()),
            cell(t.core_tests.iter().map(code).collect()),
            cell(
                t.gated
                    .iter()
                    .map(|g| format!("{}: {}", g.issue, g.what))
                    .collect()
            ),
            status
        ));
    }
    s
}

#[test]
fn the_09_table_is_the_rendered_map() {
    let doc = read(&manifest_dir().join("../../docs/planning/v0.1/09-test-strategy.md"));
    let start = doc
        .find(BEGIN)
        .expect("09 has the F11 table's begin marker")
        + BEGIN.len();
    let end = doc[start..].find(END).expect("09 has the end marker") + start;
    let expected = table();
    assert!(
        doc[start..end].trim() == expected.trim(),
        "09-test-strategy.md §12's F11 table is stale; replace it with:\n\n{expected}"
    );
}
