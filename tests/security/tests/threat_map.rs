// SPDX-License-Identifier: Apache-2.0

//! The threat map ([`THREATS`]) against the sources, `spec/security.md` §13 and
//! `09-test-strategy.md` §12 (acceptance item 5 of #60: each test names the row it proves).
//!
//! - Every 06 §14 row and every `spec/security.md` §13 row is mapped.
//! - Every test file under `tests/` is found by listing the directory, and parsed with
//!   `syn`, so a test in a new file, an indented one or one inside a module is seen.
//! - Every named test, fact and core test exists; every `#[test]` here is mapped.
//! - A gated test is `#[ignore = "GATED on #N ..."]` with an issue of its row, and its body
//!   is a single `panic!`, so it fails if run. No other test is ignored, by `#[ignore]` or
//!   by `#[cfg_attr(..., ignore)]`, and none is `#[should_panic]`.
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
    should_panic: bool,
    /// The body is exactly one `panic!(...)`.
    panics_only: bool,
}

struct Collect<'a> {
    file: &'a str,
    fns: &'a mut BTreeMap<String, Vec<Fn>>,
}

fn is_panic(mac: &syn::Macro) -> bool {
    mac.path.segments.last().is_some_and(|s| s.ident == "panic")
}

impl<'ast> Visit<'ast> for Collect<'_> {
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        let mut out = Fn {
            file: self.file.to_owned(),
            ..Fn::default()
        };
        for a in &f.attrs {
            let p = a.path();
            if p.is_ident("test") {
                out.is_test = true;
            } else if p.is_ident("ignore") {
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
            } else if p.is_ident("cfg_attr") {
                if let syn::Meta::List(l) = &a.meta {
                    let t = l.tokens.to_string();
                    out.cfg_attr_ignore |= t.contains("ignore");
                    out.should_panic |= t.contains("should_panic");
                }
            } else if p.is_ident("should_panic") {
                out.should_panic = true;
            }
        }
        out.panics_only = match f.block.stmts.as_slice() {
            [syn::Stmt::Macro(m)] => is_panic(&m.mac),
            [syn::Stmt::Expr(syn::Expr::Macro(m), _)] => is_panic(&m.mac),
            _ => false,
        };
        self.fns
            .entry(f.sig.ident.to_string())
            .or_default()
            .push(out);
        syn::visit::visit_item_fn(self, f);
    }
}

fn parse_into(path: &Path, label: &str, fns: &mut BTreeMap<String, Vec<Fn>>) {
    let file = syn::parse_file(&read(path)).unwrap_or_else(|e| panic!("{label}: {e}"));
    Collect { file: label, fns }.visit_file(&file);
}

/// Every function in every `tests/*.rs` file of this crate except this one, found by
/// listing the directory.
fn suite_fns() -> BTreeMap<String, Vec<Fn>> {
    let dir = manifest_dir().join("tests");
    let mut fns = BTreeMap::new();
    let mut files = 0;
    for e in std::fs::read_dir(&dir).expect("tests/") {
        let path = e.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.extension().is_some_and(|x| x == "rs") && name != "threat_map.rs" {
            parse_into(&path, &name, &mut fns);
            files += 1;
        }
    }
    assert!(
        files >= 12,
        "found only {files} test files in {}",
        dir.display()
    );
    fns
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

/// The attack cell of every row of `spec/security.md` §13's table.
fn spec13_attacks() -> Vec<String> {
    let spec = read(&manifest_dir().join("../../spec/security.md"));
    let start = spec
        .find("## 13. Threat-to-requirement traceability")
        .expect("§13");
    let end = spec[start..].find("### 13.1").expect("§13.1") + start;
    let rows: Vec<String> = spec[start..end]
        .lines()
        .filter(|l| l.starts_with("| ") && !l.starts_with("| Attack |"))
        .map(|l| l[2..].split(" | ").next().unwrap().to_owned())
        .collect();
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
            assert!(
                attacks.iter().any(|a| a.starts_with(p)),
                "{}: {p:?} matches no §13 row",
                t.row
            );
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
                    "{name}: a gated body must be a single panic!"
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
            assert!(
                !t.tests.contains(name),
                "{name}: a fact is not also a proof"
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
            parse_into(&path, c, &mut fns);
            the_test(&fns, name);
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
