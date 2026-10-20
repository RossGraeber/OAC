// SPDX-License-Identifier: Apache-2.0

//! The daemon's status report and its rendering (G10, #71, slice 3;
//! `docs/planning/v0.1/08-cli-and-deployment.md` §5): what the daemon answers to the
//! `status` op of its `control` endpoint (`crate::control`), and how `oac status` shows it.
//!
//! **The report** is one JSON object, built from the core's snapshot
//! ([`oac_core::pipeline::Pipelines::status`], which asks the transport and each adapter for
//! its `health` with no core lock held) and what only the daemon knows: its process id,
//! version, uptime, the test namespace if there is one, the endpoints it serves, and where its
//! log file is. Members:
//!
//! | member | meaning |
//! |---|---|
//! | `pid`, `version`, `uptime_ms` | the daemon process, the `oac` version that built it, time since it took the lock |
//! | `namespace` | the test namespace, when the daemon runs in one (absent otherwise) |
//! | `device_id` | this device's public key id |
//! | `endpoints` | the labels of the endpoints it serves, `control` included |
//! | `transport` | `{state, detail?}`: the transport's `health` |
//! | `adapters` | `[{label, state, detail?}]`: each adapter's `health` |
//! | `sessions_total`, `sessions` | this device's own sessions (at most [`MAX_SESSIONS`] listed): `{session_id, harness?, state, active_inbound?, content_types?, max_envelope_octets?}` |
//! | `counts` | `{open_connections, unbound_attachments, pending_signals, held_signals, trusted_devices, grants, discarded_frames}` |
//! | `log` | the path of the daemon's log file (absent when it has none) |
//!
//! **What it never holds:** a harness-native session id, a working directory, key material,
//! message content or a peer's address ([IFC-TYP-020], [IFC-TYP-092]). The vocabulary is the
//! neutral one: no transport-specific name appears in it.
//!
//! **Rendering.** `--json` prints the report's members beside `"oac_cli":1`, `"ok":true` and
//! `"running":true`, as one canonical object. The human form is a short list; text that a
//! component wrote (a health `detail`) is printed with control characters replaced, so it
//! cannot drive the operator's terminal.

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Instant;

use oac_core::health::HealthStatus;
use oac_core::json::{Json, JsonObject};
use oac_core::pipeline::{AdapterId, Pipelines};

use crate::control::{canonical_text, is_hostile, json_string};
use crate::daemon::LogSink;

/// The version of the `oac` that built this daemon.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The most sessions a report lists; `sessions_total` says how many there are.
pub const MAX_SESSIONS: usize = 256;

/// What the daemon reports about itself: the core it asks, and what only it knows.
pub struct DaemonStatus {
    pipelines: Arc<Pipelines>,
    adapters: Vec<(AdapterId, String)>,
    endpoints: Vec<String>,
    namespace: Option<String>,
    started: Instant,
    log: LogSink,
}

impl DaemonStatus {
    /// A reporter for `pipelines`. `adapters` pairs each adapter with the harness label of
    /// its endpoint, `endpoints` lists the labels of every endpoint served, and `started` is
    /// when the daemon took the lock.
    pub fn new(
        pipelines: Arc<Pipelines>,
        adapters: Vec<(AdapterId, String)>,
        endpoints: Vec<String>,
        namespace: Option<String>,
        started: Instant,
        log: LogSink,
    ) -> DaemonStatus {
        DaemonStatus {
            pipelines,
            adapters,
            endpoints,
            namespace,
            started,
            log,
        }
    }

    /// The reply line to a `status` request, newline included. **Blocking:** it asks the
    /// transport and the adapters for their health, so it runs off the runtime's threads.
    pub fn reply(&self) -> String {
        let status = self.pipelines.status();
        let mut members: Vec<String> = Vec::new();
        members.push(format!("\"pid\":{}", std::process::id()));
        members.push(format!("\"version\":{}", json_string(VERSION)));
        members.push(format!(
            "\"uptime_ms\":{}",
            self.started.elapsed().as_millis()
        ));
        if let Some(namespace) = &self.namespace {
            members.push(format!("\"namespace\":{}", json_string(namespace)));
        }
        members.push(format!(
            "\"device_id\":{}",
            json_string(self.pipelines.device().key_id().as_str())
        ));
        members.push(format!(
            "\"endpoints\":[{}]",
            self.endpoints
                .iter()
                .map(|e| json_string(e))
                .collect::<Vec<_>>()
                .join(",")
        ));
        members.push(format!(
            "\"transport\":{}",
            health_object(None, &status.transport)
        ));
        let adapters: Vec<String> = status
            .adapters
            .iter()
            .map(|a| {
                let label = self
                    .adapters
                    .iter()
                    .find(|(id, _)| *id == a.adapter)
                    .map_or_else(
                        || format!("adapter{}", a.adapter.index()),
                        |(_, l)| l.clone(),
                    );
                health_object(Some(&label), &a.health)
            })
            .collect();
        members.push(format!("\"adapters\":[{}]", adapters.join(",")));
        members.push(format!("\"sessions_total\":{}", status.sessions.len()));
        let sessions: Vec<String> = status
            .sessions
            .iter()
            .take(MAX_SESSIONS)
            .map(|s| {
                let mut m = vec![format!(
                    "\"session_id\":{}",
                    json_string(s.session_id.as_str())
                )];
                if let Some(label) = &s.harness_label {
                    m.push(format!("\"harness\":{}", json_string(label.as_str())));
                }
                m.push(format!("\"state\":{}", json_string(s.state.as_str())));
                if let Some(c) = &s.capabilities {
                    m.push(format!("\"active_inbound\":{}", c.active_inbound));
                    m.push(format!(
                        "\"content_types\":[{}]",
                        c.content_types
                            .iter()
                            .map(|t| json_string(t))
                            .collect::<Vec<_>>()
                            .join(",")
                    ));
                    m.push(format!("\"max_envelope_octets\":{}", c.max_envelope_octets));
                }
                format!("{{{}}}", m.join(","))
            })
            .collect();
        members.push(format!("\"sessions\":[{}]", sessions.join(",")));
        let c = &status.counts;
        members.push(format!(
            "\"counts\":{{\"open_connections\":{},\"unbound_attachments\":{},\"pending_signals\":{},\"held_signals\":{},\"trusted_devices\":{},\"grants\":{},\"discarded_frames\":{}}}",
            c.open_connections,
            c.unbound_attachments,
            c.pending_signals,
            c.held_signals,
            c.trusted_devices,
            c.grants,
            c.discarded_frames
        ));
        if let Some(path) = self.log.file_path() {
            members.push(format!(
                "\"log\":{}",
                json_string(&path.display().to_string())
            ));
        }
        let line = format!("{{\"oac_control\":1,\"status\":{{{}}}}}", members.join(","));
        let mut canonical = canonical_text(&line);
        canonical.push('\n');
        canonical
    }
}

fn health_object(label: Option<&str>, health: &HealthStatus) -> String {
    let mut m = Vec::new();
    if let Some(label) = label {
        m.push(format!("\"label\":{}", json_string(label)));
    }
    m.push(format!("\"state\":{}", json_string(health.state.as_str())));
    if let Some(detail) = &health.detail {
        m.push(format!("\"detail\":{}", json_string(&clean(detail))));
    }
    format!("{{{}}}", m.join(","))
}

// ---- rendering ------------------------------------------------------------------------------

/// `oac status --json`: the report's members beside `oac_cli`, `ok` and `running`, as one
/// canonical object.
pub fn render_json(status: &JsonObject) -> String {
    let mut out = String::from("{\"oac_cli\":1,\"ok\":true,\"running\":true");
    for (name, value) in status.iter() {
        let _ = write!(out, ",{}:{}", json_string(name), value.to_compact());
    }
    out.push('}');
    canonical_text(&out)
}

fn text<'a>(object: &'a JsonObject, name: &str) -> Option<&'a str> {
    object.get(name).and_then(Json::as_str)
}

fn int(object: &JsonObject, name: &str) -> Option<u64> {
    object
        .get(name)
        .and_then(Json::as_number)
        .and_then(|n| n.plain_integer_in(0, u64::MAX))
}

/// `text` with every control character (C0, DEL, C1) and bidirectional override replaced by
/// `?`. The canonical form writes such characters raw, so free text is cleaned at the source
/// as well as when it is shown.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() || is_hostile(c) {
                '?'
            } else {
                c
            }
        })
        .collect()
}

/// A span of milliseconds as `1h 2m 3s`, leaving off the leading units that are zero.
pub fn duration_text(ms: u64) -> String {
    let total = ms / 1000;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

fn health_text(object: &JsonObject) -> String {
    let state = clean(text(object, "state").unwrap_or("unknown"));
    match text(object, "detail") {
        Some(detail) => format!("{state} ({})", clean(detail)),
        None => state,
    }
}

/// The human form of a report.
pub fn render_human(status: &JsonObject) -> String {
    let mut out = String::new();
    let mut line = |s: String| {
        out.push_str(&s);
        out.push('\n');
    };
    line(format!(
        "oac daemon: running (pid {}, version {}, up {})",
        int(status, "pid").map_or("?".to_owned(), |p| p.to_string()),
        clean(text(status, "version").unwrap_or("?")),
        int(status, "uptime_ms").map_or("?".to_owned(), duration_text),
    ));
    if let Some(namespace) = text(status, "namespace") {
        line(format!("  test namespace: {}", clean(namespace)));
    }
    line(format!(
        "  device id: {}",
        clean(text(status, "device_id").unwrap_or("?"))
    ));
    let endpoints: Vec<String> = status
        .get("endpoints")
        .and_then(Json::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(Json::as_str)
        .map(clean)
        .collect();
    line(format!("  endpoints: {}", endpoints.join(", ")));
    line(format!(
        "  transport: {}",
        status
            .get("transport")
            .and_then(Json::as_object)
            .map_or("unknown".to_owned(), health_text)
    ));
    let adapters = status
        .get("adapters")
        .and_then(Json::as_array)
        .unwrap_or_default();
    if adapters.is_empty() {
        line("  adapters: none".to_owned());
    } else {
        line("  adapters:".to_owned());
        for adapter in adapters.iter().filter_map(Json::as_object) {
            line(format!(
                "    {}: {}",
                clean(text(adapter, "label").unwrap_or("?")),
                health_text(adapter)
            ));
        }
    }
    let total = int(status, "sessions_total").unwrap_or(0);
    line(format!("  sessions: {total}"));
    let sessions = status
        .get("sessions")
        .and_then(Json::as_array)
        .unwrap_or_default();
    for session in sessions.iter().filter_map(Json::as_object) {
        let inbound = match session.get("active_inbound").and_then(Json::as_bool) {
            Some(true) => ", live input",
            Some(false) => ", no live input",
            None => "",
        };
        line(format!(
            "    {} ({}): {}{inbound}",
            clean(text(session, "session_id").unwrap_or("?")),
            clean(text(session, "harness").unwrap_or("unknown harness")),
            clean(text(session, "state").unwrap_or("?")),
        ));
    }
    if usize::try_from(total).is_ok_and(|t| t > sessions.len()) {
        line(format!(
            "    ... and {} more",
            total - sessions.len() as u64
        ));
    }
    if let Some(counts) = status.get("counts").and_then(Json::as_object) {
        let n = |name: &str| int(counts, name).map_or("?".to_owned(), |v| v.to_string());
        line(format!(
            "  connections: {} open, {} unbound; signals: {} pending, {} held",
            n("open_connections"),
            n("unbound_attachments"),
            n("pending_signals"),
            n("held_signals"),
        ));
        line(format!(
            "  trusted devices: {}; grants: {}; discarded frames: {}",
            n("trusted_devices"),
            n("grants"),
            n("discarded_frames"),
        ));
    }
    match text(status, "log") {
        Some(path) => line(format!("  log: {}", clean(path))),
        None => line("  log: none (the daemon runs without a log file)".to_owned()),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use oac_core::json;

    fn parse_object(text: &str) -> JsonObject {
        json::parse(text.as_bytes()).unwrap().into_object().unwrap()
    }

    const SAMPLE: &str = r#"{"adapters":[{"label":"claude","state":"healthy"}],"counts":{"discarded_frames":0,"grants":1,"held_signals":0,"open_connections":2,"pending_signals":0,"trusted_devices":3,"unbound_attachments":1},"device_id":"k1","endpoints":["claude","control"],"log":"/state/daemon.log","pid":42,"sessions":[{"active_inbound":true,"harness":"claude","session_id":"s1","state":"online"}],"sessions_total":1,"transport":{"detail":"bad\u001b[2J","state":"degraded"},"uptime_ms":3723000,"version":"0.1.0"}"#;

    #[test]
    fn durations_leave_off_leading_zero_units() {
        assert_eq!(duration_text(0), "0s");
        assert_eq!(duration_text(59_999), "59s");
        assert_eq!(duration_text(61_000), "1m 1s");
        assert_eq!(duration_text(3_723_000), "1h 2m 3s");
    }

    #[test]
    fn the_json_form_is_canonical_and_keeps_every_member() {
        let object = parse_object(SAMPLE);
        let rendered = render_json(&object);
        assert_eq!(canonical_text(&rendered), rendered);
        let back = parse_object(&rendered);
        assert_eq!(
            back.get("oac_cli")
                .and_then(Json::as_number)
                .map(|n| n.raw()),
            Some("1")
        );
        assert_eq!(back.get("ok").and_then(Json::as_bool), Some(true));
        assert_eq!(back.get("running").and_then(Json::as_bool), Some(true));
        for name in object.names() {
            assert_eq!(back.get(name), object.get(name), "{name}");
        }
    }

    #[test]
    fn the_human_form_lists_the_report_and_neutralises_control_characters() {
        let human = render_human(&parse_object(SAMPLE));
        for expected in [
            "running (pid 42, version 0.1.0, up 1h 2m 3s)",
            "device id: k1",
            "endpoints: claude, control",
            "claude: healthy",
            "s1 (claude): online, live input",
            "trusted devices: 3; grants: 1; discarded frames: 0",
            "log: /state/daemon.log",
        ] {
            assert!(human.contains(expected), "{expected:?} in\n{human}");
        }
        assert!(human.contains("degraded (bad?[2J)"), "{human}");
        assert!(!human.contains('\u{1b}'), "{human}");
        assert_eq!(clean("a\u{7f}b\u{9b}c\u{202e}d"), "a?b?c?d");
    }
}
