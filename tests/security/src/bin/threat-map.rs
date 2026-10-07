// SPDX-License-Identifier: Apache-2.0

//! Prints [`oac_security_suite::THREATS`] as JSON, for `tests/security/check-compiled-tests.mjs`,
//! which compares the map with the tests cargo actually compiled (#60, PR #327 re-review B1).

use oac_security_suite::{Status, THREATS};

fn list(items: &[&str]) -> String {
    let quoted: Vec<String> = items.iter().map(|s| format!("{s:?}")).collect();
    format!("[{}]", quoted.join(","))
}

fn main() {
    let rows: Vec<String> = THREATS
        .iter()
        .map(|t| {
            let status = match t.status {
                Status::Proven => "proven",
                Status::Gated => "gated",
                Status::OpenRisk => "open-risk",
            };
            format!(
                "{{\"row\":{:?},\"status\":{status:?},\"tests\":{},\"facts\":{}}}",
                t.row,
                list(t.tests),
                list(t.facts)
            )
        })
        .collect();
    println!("[{}]", rows.join(","));
}
