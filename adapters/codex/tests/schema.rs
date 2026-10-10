// SPDX-License-Identifier: Apache-2.0

//! The checked-in schema is the source of truth for message shapes (G6 acceptance item 1).
//!
//! Every [`Shape`] in `src/schema.rs` names a file and a definition in the vendored Codex
//! app-server schema (`docs/planning/vendor/codex-app-server-protocol/rust-v0.161.0/json/`,
//! D5). This test reads each file at run time and fails when a shape differs from it:
//!
//! - its `required` list is not the schema's;
//! - a member the client relies on is not defined there, or not with that type;
//! - a member the client sends is not defined there.
//!
//! It also checks the messages the client actually builds against their shapes.

use std::path::PathBuf;

use oac_adapter_codex::schema::{self, Kind, SCHEMA_SNAPSHOT, Shape};
use oac_adapter_codex::shim;
use oac_core::json::{self, Json};

fn snapshot_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/planning/vendor/codex-app-server-protocol")
        .join(SCHEMA_SNAPSHOT)
        .join("json")
}

fn load(file: &str) -> Json {
    let p = snapshot_dir().join(file);
    let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
    json::parse(&bytes).unwrap_or_else(|e| panic!("parse {}: {e}", p.display()))
}

fn get<'a>(v: &'a Json, name: &str) -> Option<&'a Json> {
    v.as_object()?.get(name)
}

/// The schema object a shape names: the file's top level, a definition, or a `oneOf`
/// variant of a definition picked by its `type` enum value.
fn resolve<'a>(doc: &'a Json, shape: &Shape) -> &'a Json {
    let Some(def) = shape.definition else {
        return doc;
    };
    let (name, variant) = match def.split_once('/') {
        Some((n, v)) => (n, Some(v)),
        None => (def, None),
    };
    let d = get(doc, "definitions")
        .and_then(|d| get(d, name))
        .unwrap_or_else(|| panic!("{}: no definition {name} in {}", shape.name, shape.file));
    match variant {
        None => d,
        Some(v) => get(d, "oneOf")
            .and_then(Json::as_array)
            .and_then(|vs| {
                vs.iter().find(|x| {
                    get(x, "properties")
                        .and_then(|p| get(p, "type"))
                        .and_then(|t| get(t, "enum"))
                        .and_then(Json::as_array)
                        .is_some_and(|e| e.iter().any(|s| s.as_str() == Some(v)))
                })
            })
            .unwrap_or_else(|| panic!("{}: no {v} variant of {name}", shape.name)),
    }
}

/// The JSON Schema types a property may hold, through `type` (a string or an array),
/// `$ref`, `allOf`, `anyOf` and `oneOf`, one level of references deep.
fn types_of(doc: &Json, prop: &Json) -> Vec<String> {
    let mut out = Vec::new();
    match get(prop, "type") {
        Some(Json::String(s)) => out.push(s.clone()),
        Some(Json::Array(a)) => out.extend(a.iter().filter_map(|t| t.as_str().map(str::to_owned))),
        _ => {}
    }
    if let Some(r) = get(prop, "$ref").and_then(Json::as_str)
        && let Some(name) = r.strip_prefix("#/definitions/")
        && let Some(d) = get(doc, "definitions").and_then(|d| get(d, name))
    {
        out.extend(types_of(doc, d));
        if get(d, "oneOf").is_some() || get(d, "properties").is_some() {
            out.push("object".into());
        }
    }
    for k in ["allOf", "anyOf", "oneOf"] {
        if let Some(a) = get(prop, k).and_then(Json::as_array) {
            for x in a {
                out.extend(types_of(doc, x));
            }
        }
    }
    out
}

fn kind_ok(kind: Kind, types: &[String]) -> bool {
    match kind {
        Kind::Num => types.iter().any(|t| t == "integer" || t == "number"),
        k => types.iter().any(|t| t == k.schema_name()),
    }
}

#[test]
fn every_shape_matches_the_vendored_schema() {
    let mut problems = Vec::new();
    for shape in schema::ALL {
        let doc = load(shape.file);
        let s = resolve(&doc, shape);
        let mut required: Vec<String> = get(s, "required")
            .and_then(Json::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        required.sort_unstable();
        if required != shape.required {
            problems.push(format!(
                "{}: required {:?}, schema says {required:?}",
                shape.name, shape.required
            ));
        }
        let props = get(s, "properties");
        for (name, kind) in shape.relied_on {
            match props.and_then(|p| get(p, name)) {
                None => problems.push(format!(
                    "{}: relies on {name}, not in the schema",
                    shape.name
                )),
                Some(p) => {
                    let types = types_of(&doc, p);
                    if !kind_ok(*kind, &types) {
                        problems.push(format!(
                            "{}: relies on {name} as {kind:?}, schema types {types:?}",
                            shape.name
                        ));
                    }
                }
            }
        }
        for name in shape.sent {
            if props.and_then(|p| get(p, name)).is_none() {
                problems.push(format!("{}: sends {name}, not in the schema", shape.name));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_snapshot_is_the_vendored_one() {
    assert!(
        snapshot_dir().join("v2").is_dir(),
        "{} is missing",
        snapshot_dir().display()
    );
    // The experimental method is absent from the default schema: its shape comes from the
    // shim, with where it was read.
    let all = load("ClientRequest.json").to_compact();
    for m in shim::EXPERIMENTAL_METHODS {
        assert!(
            !all.contains(&format!("\"{m}\"")),
            "{m} is in the default schema"
        );
    }
}

#[test]
fn the_requests_the_client_builds_fit_their_shapes() {
    // The queue add's single input item is a TextUserInput.
    let call = shim::queue_add("01a1186d-b327-74f2-ac20-e3c5d0a9e81c", "x", "oac-1");
    let v = json::parse(call.params().as_bytes()).unwrap();
    let item = &get(&v, "input").and_then(Json::as_array).unwrap()[0];
    let mut names: Vec<&str> = item.as_object().unwrap().names().collect();
    names.sort_unstable();
    assert_eq!(names, schema::TEXT_USER_INPUT.sent);
    for r in schema::TEXT_USER_INPUT.required {
        assert!(names.contains(r));
    }
    // thread/resume sends only threadId and excludeTurns ([MCPB-CDX-006]); neither sets a
    // thread or turn setting.
    for setting in [
        "model",
        "cwd",
        "approvalPolicy",
        "sandbox",
        "permissions",
        "config",
    ] {
        assert!(!schema::THREAD_RESUME_PARAMS.sent.contains(&setting));
    }
}
