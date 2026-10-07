// SPDX-License-Identifier: Apache-2.0

//! The threat map ([`THREATS`]) against the test sources and `09-test-strategy.md` §12:
//! every 06 row is mapped, every named test exists, every gated test is ignored with its
//! owning issue and no passing test is, no test is left unmapped, and the table in 09 is the
//! one this map renders (acceptance item 5 of #60: each test names the row it proves).

use oac_security_suite::{Status, THREATS};

const SOURCES: &[(&str, &str)] = &[
    ("spoofing.rs", include_str!("spoofing.rs")),
    ("replay.rs", include_str!("replay.rs")),
    ("transport.rs", include_str!("transport.rs")),
    ("routing.rs", include_str!("routing.rs")),
    ("provenance.rs", include_str!("provenance.rs")),
    ("keys_and_pairing.rs", include_str!("keys_and_pairing.rs")),
    ("never_steer.rs", include_str!("never_steer.rs")),
    ("receipts.rs", include_str!("receipts.rs")),
    ("presence.rs", include_str!("presence.rs")),
    ("exhaustion.rs", include_str!("exhaustion.rs")),
    ("local_ipc.rs", include_str!("local_ipc.rs")),
];

const TEST_STRATEGY: &str = include_str!("../../../docs/planning/v0.1/09-test-strategy.md");
const BEGIN: &str =
    "<!-- F11 threat map: generated from tests/security/src/lib.rs THREATS; begin -->";
const END: &str = "<!-- F11 threat map: end -->";

/// Every `#[test]` function in the sources, with the attribute lines above it.
fn test_fns() -> Vec<(&'static str, String, Vec<String>)> {
    let mut out = Vec::new();
    for (file, src) in SOURCES {
        let lines: Vec<&str> = src.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            let Some(rest) = l.strip_prefix("fn ") else {
                continue;
            };
            let name = rest.split('(').next().unwrap().to_owned();
            let mut attrs = Vec::new();
            let mut j = i;
            while j > 0 && lines[j - 1].starts_with("#[") {
                j -= 1;
                attrs.push(lines[j].to_owned());
            }
            if attrs.iter().any(|a| a == "#[test]") {
                out.push((*file, name, attrs));
            }
        }
    }
    out
}

fn is_gated(name: &str) -> bool {
    name.starts_with("gated_")
}

#[test]
fn every_06_row_is_mapped_once() {
    for n in 1..=24 {
        let row = format!("06-{n}");
        let count = THREATS.iter().filter(|t| t.row == row).count();
        assert_eq!(count, 1, "{row}");
    }
    let mut rows: Vec<&str> = THREATS.iter().map(|t| t.row).collect();
    rows.sort_unstable();
    rows.dedup();
    assert_eq!(rows.len(), THREATS.len(), "a row is mapped twice");
}

#[test]
fn every_mapped_test_exists_and_every_test_is_mapped() {
    let fns = test_fns();
    for t in THREATS {
        for name in t.tests {
            let found: Vec<_> = fns.iter().filter(|(_, n, _)| n == name).collect();
            assert_eq!(found.len(), 1, "{}: test {name} not found once", t.row);
            let (_, _, attrs) = found[0];
            let ignore = attrs.iter().find(|a| a.starts_with("#[ignore"));
            if is_gated(name) {
                let a = ignore.unwrap_or_else(|| panic!("{name} is gated but not ignored"));
                assert!(
                    t.gated.iter().any(|g| g
                        .issue
                        .split(", ")
                        .any(|i| a.contains(&format!("GATED on {i}")))),
                    "{name}: {a} does not name an issue of {}'s gates",
                    t.row
                );
            } else {
                assert!(
                    ignore.is_none(),
                    "{name} proves a mitigation, so it must run"
                );
            }
        }
    }
    for (file, name, _) in &fns {
        assert!(
            THREATS.iter().any(|t| t.tests.contains(&name.as_str())),
            "{file}: {name} is in no threat row"
        );
    }
}

/// Each test's name starts with the row it proves (`rowNN_` for 06 row NN, `s13_` for a
/// `spec/security.md` §13 row, `gated_` for a gated one), and that row lists it.
#[test]
fn every_test_name_starts_with_a_row() {
    for (file, name, _) in test_fns() {
        let ok = name.starts_with("gated_")
            || name.starts_with("s13_")
            || (name.starts_with("row")
                && name[3..5].bytes().all(|b| b.is_ascii_digit())
                && THREATS
                    .iter()
                    .any(|t| t.row == format!("06-{}", name[3..5].trim_start_matches('0'))));
        assert!(ok, "{file}: {name} does not name its threat row");
        if let Some(n) = name.strip_prefix("row") {
            let row = format!("06-{}", n[..2].trim_start_matches('0'));
            let t = THREATS.iter().find(|t| t.row == row).unwrap();
            assert!(
                t.tests.contains(&name.as_str()),
                "{name} names {row} but {row} does not list it"
            );
        }
        if name.starts_with("s13_") {
            assert!(
                THREATS
                    .iter()
                    .any(|t| t.row.starts_with("S13-") && t.tests.contains(&name.as_str())),
                "{name} is in no S13 row"
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
            Status::Gated => {
                assert_eq!(passing, 0, "{}", t.row);
                assert!(gated > 0 && !t.gated.is_empty(), "{}", t.row);
            }
            Status::OpenRisk => {
                assert!(t.tests.is_empty() && t.gated.is_empty(), "{}", t.row);
            }
        }
        if gated > 0 {
            assert!(!t.gated.is_empty(), "{}: a gated test with no gate", t.row);
        }
        for g in t.gated {
            assert!(g.issue.starts_with('#'), "{}: {}", t.row, g.issue);
        }
    }
}

/// The 09 §12 F11 table, as this map renders it.
fn table() -> String {
    let mut s = String::from(
        "| Threat row | Attack | Proving tests (`tests/security/`, passing) | Gated, with owning issue | Status |\n|---|---|---|---|---|\n",
    );
    for t in THREATS {
        let passing: Vec<String> = t
            .tests
            .iter()
            .filter(|n| !is_gated(n))
            .map(|n| format!("`{n}`"))
            .collect();
        let gated: Vec<String> = t
            .gated
            .iter()
            .map(|g| format!("{}: {}", g.issue, g.what))
            .collect();
        let status = match t.status {
            Status::Proven if t.gated.is_empty() => "proven",
            Status::Proven => "proven (core and fakes); gated part open",
            Status::Gated => "gated: v0.1 gap",
            Status::OpenRisk => "open risk (06 §15)",
        };
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            t.row,
            t.attack,
            if passing.is_empty() {
                "none".to_owned()
            } else {
                passing.join("<br>")
            },
            if gated.is_empty() {
                "none".to_owned()
            } else {
                gated.join("<br>")
            },
            status
        ));
    }
    s
}

#[test]
fn the_09_table_is_the_rendered_map() {
    let doc = TEST_STRATEGY.replace("\r\n", "\n");
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
