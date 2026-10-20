// SPDX-License-Identifier: Apache-2.0

//! `oac doctor`: a one-shot preflight that reports each check as PASS, WARN, FAIL or SKIP
//! (G10, #71, slice 5; `docs/planning/v0.1/08-cli-and-deployment.md` §5 and §6; lead
//! approval 2026-10-18).
//!
//! **What it asks, and of whom.** The daemon is asked once, over its `control` endpoint
//! (`crate::control::request_status`, the `status` op `oac status` uses): the transport's
//! health, the adapters' health and the trusted-device and grant counts all come from that
//! one reply. Doctor never starts a transport or an adapter itself, and never holds key
//! material: the credential check writes, reads back and deletes a fixed non-secret value
//! under its own entry (`crate::keystore::probe_credential_store`), never the device key's.
//!
//! **What it never reads** ([ADR-001 Boundary]; `scripts/local-ci.mjs` step `doctor-boundary`
//! enforces it over `cli/src`): a harness's credential files, its databases, or the contents
//! of the directories a harness keeps its configuration in. The only thing it runs of a
//! harness is `claude --version`, a documented flag, found on `PATH` and bounded in time and
//! output.
//!
//! | check | PASS | WARN | FAIL | SKIP |
//! |---|---|---|---|---|
//! | `daemon` | running | the control endpoint stayed busy | no daemon; another user's endpoint; another control-protocol version; no answer | |
//! | `ipc_permissions` | the run directory (Unix) or the pipe (Windows) is this user's alone | | it is not | Windows: no pipe to inspect |
//! | `credential_store` | write, read back and delete worked | | the store is unavailable or failed | |
//! | `transport` | the daemon reports it healthy | | degraded or unavailable | no daemon |
//! | `mcp_revision` | no session failed to negotiate, and the environment leaves the revision alone | the environment sets a revision other than `legacy`, or a Claude session did not negotiate a legacy revision (counted since the daemon started) | | |
//! | `pairing` | a trusted device or a grant exists | none of either | | no daemon |
//! | `claude_version` | at or above the recorded minimum | below it, or not recognised, or `claude` did not answer | | `claude` is not on `PATH` |
//! | `codex` | | | | always (#433) |
//!
//! The exit status is 0 unless a check FAILs (then 1); 2 is usage.

use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use oac_adapter_claude::UNNEGOTIATED_DETAIL;
use oac_core::json::{Json, JsonObject};

use crate::control::{self, StatusOutcome, canonical_text, json_string};
use crate::ipc::{CONTROL_LABEL, Scope, scope_name_is_valid};
use crate::keystore::CredentialProbe;
use crate::status::clean;

/// The Claude Code version below which `claude_version` warns: the minimum recorded in
/// `docs/planning/PINS.md` (`v2.1.282`). A version is a warning, never a gate (#216).
pub const CLAUDE_MINIMUM: (u64, u64, u64) = (2, 1, 282);

/// How long `claude --version` may run.
pub const CLAUDE_WAIT: Duration = Duration::from_secs(5);

/// The most output of `claude --version` that is read.
pub const CLAUDE_OUTPUT_LIMIT: u64 = 4096;

/// The environment variable that forces a stdio MCP server to a revision.
pub const NEGOTIATION_VARIABLE: &str = "MCP_PROTOCOL_NEGOTIATION";

/// The test-only switch that turns the credential probe off. It is honoured only when
/// `OAC_SCOPE` names a valid test namespace, so it cannot hide a failure on a user's own
/// host; the tests that run the binary set it so the default tier never touches the host's
/// real credential store.
pub const NO_CREDENTIAL_PROBE_VARIABLE: &str = "OAC_DOCTOR_NO_CREDENTIAL_PROBE";

/// What the credential-store step came to: a probe, or the reason none was made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CredentialOutcome {
    /// The store was probed.
    Probed(CredentialProbe),
    /// It was not probed, for this reason.
    Skipped(&'static str),
}

/// The outcome of one check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckResult {
    /// The check holds.
    Pass,
    /// Something is off, and nothing is broken.
    Warn,
    /// Something is broken.
    Fail,
    /// The check does not apply, or cannot be made now.
    Skip,
}

impl CheckResult {
    fn word(self) -> &'static str {
        match self {
            CheckResult::Pass => "PASS",
            CheckResult::Warn => "WARN",
            CheckResult::Fail => "FAIL",
            CheckResult::Skip => "SKIP",
        }
    }

    /// The word in `--json` output.
    pub fn as_str(self) -> &'static str {
        match self {
            CheckResult::Pass => "pass",
            CheckResult::Warn => "warn",
            CheckResult::Fail => "fail",
            CheckResult::Skip => "skip",
        }
    }
}

/// One line of the report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    /// The check's name.
    pub name: &'static str,
    /// Its outcome.
    pub result: CheckResult,
    /// One neutral line.
    pub detail: String,
}

fn check(name: &'static str, result: CheckResult, detail: impl Into<String>) -> Check {
    Check {
        name,
        result,
        detail: detail.into(),
    }
}

/// What running `claude --version` came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaudeVersion {
    /// No `claude` on `PATH`.
    NotOnPath,
    /// It answered; the (bounded) text it wrote.
    Output(String),
    /// It was found and did not answer: could not start, timed out or failed.
    Failed(String),
}

/// What the checks look at beyond the daemon: injected so a test decides each one.
pub struct Probes<'a> {
    /// How long to wait for the daemon's `status` answer.
    pub status_wait: Duration,
    /// The value of [`NEGOTIATION_VARIABLE`] in this process's environment.
    pub negotiation: Option<OsString>,
    /// The credential-store probe.
    pub credential: &'a dyn Fn() -> CredentialOutcome,
    /// Whether the encrypted-file fallback's key file exists (Unix only; existence only).
    pub fallback_file: Option<bool>,
    /// `claude --version`.
    pub claude: &'a dyn Fn() -> ClaudeVersion,
}

/// The credential-store service the probe uses: OAC's own, or, in a test namespace
/// (`OAC_SCOPE`), one of its own, so that a test never touches the user's entries.
pub fn credential_service(namespace: Option<&str>) -> String {
    match namespace {
        Some(n) => format!("{}.test.{n}", crate::keystore::SERVICE),
        None => crate::keystore::SERVICE.to_owned(),
    }
}

/// Decides, from the raw `OAC_SCOPE`, which credential service to probe, and probes it with
/// `probe`. **It never probes OAC's real service unless no namespace was asked for**: a
/// value that is set and is not a valid namespace name (so that no scope was made from it)
/// is skipped, not treated as "no namespace". A valid namespace probes its own test service
/// and, when `disable` is set and not empty, is skipped (the test-only switch).
pub fn credential_outcome(
    requested: Option<&OsStr>,
    disable: Option<&OsStr>,
    probe: &dyn Fn(&str) -> CredentialProbe,
) -> CredentialOutcome {
    match requested.filter(|v| !v.is_empty()) {
        None => CredentialOutcome::Probed(probe(&credential_service(None))),
        Some(value) => match value.to_str().filter(|n| scope_name_is_valid(n)) {
            None => CredentialOutcome::Skipped("OAC_SCOPE is not a valid namespace; not probed"),
            Some(_) if disable.is_some_and(|d| !d.is_empty()) => {
                CredentialOutcome::Skipped("probe disabled for this test")
            }
            Some(name) => CredentialOutcome::Probed(probe(&credential_service(Some(name)))),
        },
    }
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

/// Runs every check, in the order of the report.
pub fn run(scope: &io::Result<Scope>, probes: &Probes<'_>) -> Vec<Check> {
    let outcome = match scope {
        Ok(s) => Some(control::request_status(
            &s.endpoint(CONTROL_LABEL),
            probes.status_wait,
        )),
        Err(_) => None,
    };
    let report = match &outcome {
        Some(StatusOutcome::Running(r)) => Some(r),
        _ => None,
    };
    vec![
        daemon_check(outcome.as_ref()),
        ipc_check(scope, outcome.as_ref()),
        credential_check(probes),
        transport_check(report),
        revision_check(probes, report),
        pairing_check(report),
        claude_check(probes),
        check(
            "codex",
            CheckResult::Skip,
            "not wired into the daemon (#433)",
        ),
    ]
}

fn daemon_check(outcome: Option<&StatusOutcome>) -> Check {
    use CheckResult::{Fail, Pass, Warn};
    match outcome {
        None => check(
            "daemon",
            Fail,
            "cannot look for the daemon: its run directory is unusable",
        ),
        Some(StatusOutcome::Running(r)) => check(
            "daemon",
            Pass,
            format!(
                "running (pid {}, version {})",
                int(r, "pid").map_or("?".to_owned(), |p| p.to_string()),
                clean(text(r, "version").unwrap_or("?"))
            ),
        ),
        Some(StatusOutcome::NotRunning) => check("daemon", Fail, "no daemon; run `oac start`"),
        Some(StatusOutcome::Foreign) => check("daemon", Fail, "endpoint served by another user"),
        Some(StatusOutcome::Busy) => check(
            "daemon",
            Warn,
            "the control endpoint stayed busy; try again",
        ),
        Some(StatusOutcome::Version) => check(
            "daemon",
            Fail,
            "the daemon speaks another control-protocol version; use the `oac` that started it",
        ),
        Some(StatusOutcome::Unreachable(why)) | Some(StatusOutcome::Failed(why)) => {
            check("daemon", Fail, format!("not reachable: {}", clean(why)))
        }
    }
}

fn ipc_check(scope: &io::Result<Scope>, outcome: Option<&StatusOutcome>) -> Check {
    use CheckResult::{Fail, Pass, Skip};
    if let Err(e) = scope {
        let why = if e.kind() == io::ErrorKind::PermissionDenied {
            "the run directory is not private to this user".to_owned()
        } else {
            format!("the run directory is unusable: {}", clean(&e.to_string()))
        };
        return check("ipc_permissions", Fail, why);
    }
    if matches!(outcome, Some(StatusOutcome::Foreign)) {
        return check(
            "ipc_permissions",
            Fail,
            "the endpoint or its directory is not this user's alone",
        );
    }
    if cfg!(unix) {
        // `Scope::for_user` refuses a run directory that is not a real directory owned by
        // this user and closed to everyone else, so having a scope is the check.
        return check(
            "ipc_permissions",
            Pass,
            "the run directory is owned by this user and closed to others",
        );
    }
    // Windows: the client reads the pipe's owner and access list from its handle before it
    // sends a byte and refuses a pipe that is not this user's alone (`crate::ipc`). An
    // outcome that came after the connect (an answer, another version, a failed exchange)
    // means that check passed.
    match outcome {
        Some(StatusOutcome::Running(_) | StatusOutcome::Version | StatusOutcome::Failed(_)) => {
            check(
                "ipc_permissions",
                Pass,
                "the pipe's owner and access list were verified when connecting",
            )
        }
        Some(StatusOutcome::Busy) => check(
            "ipc_permissions",
            Skip,
            "the endpoint stayed busy, so no connection was made to verify the pipe",
        ),
        _ => check(
            "ipc_permissions",
            Skip,
            "no connection was made, so no pipe was verified; its owner and access list are checked when connecting",
        ),
    }
}

fn credential_check(probes: &Probes<'_>) -> Check {
    use CheckResult::{Fail, Pass, Skip, Warn};
    let fallback = match probes.fallback_file {
        Some(true) => "; encrypted-file fallback: a key file exists",
        Some(false) => "; encrypted-file fallback: no key file",
        None => "",
    };
    match (probes.credential)() {
        CredentialOutcome::Skipped(why) => check("credential_store", Skip, why),
        CredentialOutcome::Probed(CredentialProbe::Works) => check(
            "credential_store",
            Pass,
            format!("the credential store accepted, returned and deleted a probe value{fallback}"),
        ),
        // The daemon falls back to the encrypted file only while the store reports itself
        // unavailable, and only when the file exists is there a device key to use.
        CredentialOutcome::Probed(CredentialProbe::Unavailable)
            if probes.fallback_file == Some(true) =>
        {
            check(
                "credential_store",
                Warn,
                "credential store unavailable; the daemon uses the encrypted-file fallback",
            )
        }
        CredentialOutcome::Probed(CredentialProbe::Unavailable) => check(
            "credential_store",
            Fail,
            format!("credential store unavailable{fallback}"),
        ),
        CredentialOutcome::Probed(CredentialProbe::Locked) => check(
            "credential_store",
            Fail,
            format!("credential store unavailable (locked){fallback}"),
        ),
        CredentialOutcome::Probed(CredentialProbe::Failed) => check(
            "credential_store",
            Fail,
            format!("credential store unavailable (the probe failed){fallback}"),
        ),
    }
}

fn transport_check(report: Option<&JsonObject>) -> Check {
    use CheckResult::{Fail, Pass, Skip};
    let Some(report) = report else {
        return check("transport", Skip, "no daemon to ask");
    };
    match report
        .get("transport")
        .and_then(Json::as_object)
        .and_then(|t| text(t, "state"))
    {
        Some("healthy") => check(
            "transport",
            Pass,
            "the daemon reports the transport healthy",
        ),
        Some(_) => check("transport", Fail, "the transport peer cannot link"),
        None => check("transport", Fail, "the daemon did not report the transport"),
    }
}

fn revision_check(probes: &Probes<'_>, report: Option<&JsonObject>) -> Check {
    use CheckResult::{Pass, Warn};
    let claude = report.and_then(|r| {
        r.get("adapters")
            .and_then(Json::as_array)?
            .iter()
            .filter_map(Json::as_object)
            .find(|a| text(a, "label") == Some("claude"))
    });
    if claude.is_some_and(|a| {
        text(a, "state") != Some("healthy")
            && text(a, "detail").is_some_and(|d| d.contains(UNNEGOTIATED_DETAIL))
    }) {
        // WARN, not FAIL (lead decision L1, 2026-10-20): the count is cumulative since the
        // daemon started and never decays, so it cannot tell a session that is broken now
        // from one that was fixed since. Revisit with a decay or per-attachment rule.
        return check(
            "mcp_revision",
            Warn,
            "a Claude session did not negotiate a legacy MCP revision (counted since the daemon started)",
        );
    }
    let non_legacy = probes
        .negotiation
        .as_ref()
        .is_some_and(|v| v.to_str() != Some("legacy"));
    if non_legacy {
        return check(
            "mcp_revision",
            Warn,
            format!(
                "{NEGOTIATION_VARIABLE} is set to a value other than `legacy`; it affects harnesses started from this environment"
            ),
        );
    }
    let sessions = if report.is_some() {
        "no session failed to negotiate"
    } else {
        "no daemon to ask about sessions"
    };
    check(
        "mcp_revision",
        Pass,
        format!("{NEGOTIATION_VARIABLE} leaves the revision alone; {sessions}"),
    )
}

fn pairing_check(report: Option<&JsonObject>) -> Check {
    use CheckResult::{Pass, Skip, Warn};
    let Some(report) = report else {
        return check("pairing", Skip, "no daemon to ask");
    };
    let counts = report.get("counts").and_then(Json::as_object);
    let n = |name: &str| counts.and_then(|c| int(c, name)).unwrap_or(0);
    let (devices, grants) = (n("trusted_devices"), n("grants"));
    if devices == 0 && grants == 0 {
        check(
            "pairing",
            Warn,
            "no pairings or grants; default deny, nothing is delivered",
        )
    } else {
        check(
            "pairing",
            Pass,
            format!("{devices} trusted device(s), {grants} grant(s)"),
        )
    }
}

fn claude_check(probes: &Probes<'_>) -> Check {
    use CheckResult::{Pass, Skip, Warn};
    // Not on PATH is a SKIP, not a WARN: a host that uses only another harness, or starts
    // Claude Code by an absolute path, has nothing wrong. A WARN is for a version we found.
    let (a, b, c) = CLAUDE_MINIMUM;
    let minimum = format!("{a}.{b}.{c}");
    match (probes.claude)() {
        ClaudeVersion::NotOnPath => check("claude_version", Skip, "`claude` is not on PATH"),
        ClaudeVersion::Failed(why) => check(
            "claude_version",
            Warn,
            format!("`claude --version` did not answer: {}", clean(&why)),
        ),
        ClaudeVersion::Output(out) => match parse_version(&out) {
            Some(v) if v >= CLAUDE_MINIMUM => check(
                "claude_version",
                Pass,
                format!("Claude Code {}.{}.{} (minimum {minimum})", v.0, v.1, v.2),
            ),
            Some(v) => check(
                "claude_version",
                Warn,
                format!(
                    "Claude Code {}.{}.{} is below the recorded minimum {minimum}",
                    v.0, v.1, v.2
                ),
            ),
            None => check(
                "claude_version",
                Warn,
                "`claude --version` did not print a recognisable version",
            ),
        },
    }
}

/// The first `major.minor.patch` in `text`.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let bytes = text.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        if bytes[start].is_ascii_digit() && (start == 0 || !bytes[start - 1].is_ascii_digit()) {
            let rest = &text[start..];
            let end = rest
                .find(|c: char| !(c.is_ascii_digit() || c == '.'))
                .unwrap_or(rest.len());
            let mut parts = rest[..end].split('.');
            let mut next = || parts.next().and_then(|p| p.parse::<u64>().ok());
            if let (Some(a), Some(b), Some(c)) = (next(), next(), next()) {
                return Some((a, b, c));
            }
            start += end.max(1);
        } else {
            start += 1;
        }
    }
    None
}

// ---- `claude --version` -----------------------------------------------------------------

/// The absolute path of the first file called `claude` on `path` (`claude.exe` or
/// `claude.cmd` on Windows). A relative `PATH` entry is skipped: the search never looks in
/// the current directory.
pub fn find_on_path(path: Option<&OsStr>) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    };
    for dir in std::env::split_paths(path?) {
        if !dir.is_absolute() {
            continue;
        }
        for name in names {
            let candidate = dir.join(name);
            if std::fs::metadata(&candidate).is_ok_and(|m| m.is_file()) {
                return Some(candidate);
            }
        }
    }
    None
}

/// A child killed and reaped when it goes out of scope, however that happens (#435).
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Runs `claude --version` from `PATH`: found without searching the current directory, given
/// no input, at most [`CLAUDE_OUTPUT_LIMIT`] octets read, and ended after `wait`.
pub fn probe_claude(path: Option<&OsStr>, wait: Duration) -> ClaudeVersion {
    match find_on_path(path) {
        None => ClaudeVersion::NotOnPath,
        Some(exe) => run_version(&exe, wait),
    }
}

fn run_version(exe: &Path, wait: Duration) -> ClaudeVersion {
    let child = Command::new(exe)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(c) => KillOnDrop(c),
        Err(e) => return ClaudeVersion::Failed(format!("could not start: {}", e.kind())),
    };
    let Some(stdout) = child.0.stdout.take() else {
        return ClaudeVersion::Failed("no output".to_owned());
    };
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("oac-doctor-read".into())
        .spawn(move || {
            let mut out = Vec::new();
            let _ = stdout.take(CLAUDE_OUTPUT_LIMIT).read_to_end(&mut out);
            let _ = tx.send(out);
        });
    if spawned.is_err() {
        return ClaudeVersion::Failed("could not read its output".to_owned());
    }
    let end = Instant::now() + wait;
    loop {
        match child.0.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return ClaudeVersion::Failed("it exited with an error".to_owned()),
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => return ClaudeVersion::Failed("it did not answer in time".to_owned()),
            Err(_) => return ClaudeVersion::Failed("it could not be waited for".to_owned()),
        }
    }
    let left = end
        .saturating_duration_since(Instant::now())
        .max(Duration::from_millis(200));
    match rx.recv_timeout(left) {
        Ok(out) => ClaudeVersion::Output(String::from_utf8_lossy(&out).into_owned()),
        Err(_) => ClaudeVersion::Failed("it did not finish its output in time".to_owned()),
    }
}

// ---- rendering --------------------------------------------------------------------------

/// True when no check FAILed.
pub fn all_clear(checks: &[Check]) -> bool {
    checks.iter().all(|c| c.result != CheckResult::Fail)
}

/// `oac doctor --json`: `oac_cli`, `ok` (no check FAILed) and `checks`, as one canonical
/// object.
pub fn render_json(checks: &[Check]) -> String {
    let rows: Vec<String> = checks
        .iter()
        .map(|c| {
            format!(
                "{{\"detail\":{},\"name\":{},\"result\":{}}}",
                json_string(&clean(&c.detail)),
                json_string(c.name),
                json_string(c.result.as_str())
            )
        })
        .collect();
    let mut out = String::new();
    let _ = write!(
        out,
        "{{\"checks\":[{}],\"oac_cli\":1,\"ok\":{}}}",
        rows.join(","),
        all_clear(checks)
    );
    canonical_text(&out)
}

/// The human form: one line per check, then a count.
pub fn render_human(checks: &[Check]) -> String {
    let mut out = String::new();
    for c in checks {
        let _ = writeln!(out, "{}  {}: {}", c.result.word(), c.name, clean(&c.detail));
    }
    let n = |r: CheckResult| checks.iter().filter(|c| c.result == r).count();
    let _ = writeln!(
        out,
        "{} pass, {} warn, {} fail, {} skip",
        n(CheckResult::Pass),
        n(CheckResult::Warn),
        n(CheckResult::Fail),
        n(CheckResult::Skip)
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probes<'a>(
        negotiation: Option<&str>,
        claude: &'a dyn Fn() -> ClaudeVersion,
        credential: &'a dyn Fn() -> CredentialOutcome,
    ) -> Probes<'a> {
        Probes {
            status_wait: Duration::from_millis(200),
            negotiation: negotiation.map(OsString::from),
            credential,
            fallback_file: None,
            claude,
        }
    }

    fn by_name<'a>(checks: &'a [Check], name: &str) -> &'a Check {
        checks.iter().find(|c| c.name == name).expect(name)
    }

    #[test]
    fn versions_are_found_in_the_text_claude_prints() {
        assert_eq!(parse_version("2.1.285 (Claude Code)\n"), Some((2, 1, 285)));
        assert_eq!(parse_version("claude v10.20.30-beta"), Some((10, 20, 30)));
        assert_eq!(parse_version("build 7 of 2.1"), None);
        assert_eq!(parse_version(""), None);
        assert!(parse_version("2.1.281") < Some(CLAUDE_MINIMUM));
        assert!(parse_version("2.1.282") >= Some(CLAUDE_MINIMUM));
    }

    #[test]
    fn the_claude_check_warns_on_a_version_and_skips_when_absent() {
        let cred = || CredentialOutcome::Probed(CredentialProbe::Works);
        for (claude, result) in [
            (ClaudeVersion::NotOnPath, CheckResult::Skip),
            (ClaudeVersion::Output("2.1.285".into()), CheckResult::Pass),
            (ClaudeVersion::Output("2.1.282".into()), CheckResult::Pass),
            (ClaudeVersion::Output("2.1.281".into()), CheckResult::Warn),
            (ClaudeVersion::Output("garbage".into()), CheckResult::Warn),
            (ClaudeVersion::Failed("x".into()), CheckResult::Warn),
        ] {
            let f = move || claude.clone();
            let p = probes(None, &f, &cred);
            assert_eq!(claude_check(&p).result, result);
        }
    }

    #[test]
    fn the_environment_decides_the_revision_check_when_no_daemon_answers() {
        let (cred, claude) = (
            || CredentialOutcome::Probed(CredentialProbe::Works),
            || ClaudeVersion::NotOnPath,
        );
        for (value, result) in [
            (None, CheckResult::Pass),
            (Some("legacy"), CheckResult::Pass),
            (Some("2026-07-28"), CheckResult::Warn),
            (Some(""), CheckResult::Warn),
        ] {
            let p = probes(value, &claude, &cred);
            assert_eq!(revision_check(&p, None).result, result, "{value:?}");
        }
    }

    #[test]
    fn credential_outcomes_are_a_pass_or_a_fail_that_names_the_store() {
        let claude = || ClaudeVersion::NotOnPath;
        for (probe, result) in [
            (CredentialProbe::Works, CheckResult::Pass),
            (CredentialProbe::Unavailable, CheckResult::Fail),
            (CredentialProbe::Locked, CheckResult::Fail),
            (CredentialProbe::Failed, CheckResult::Fail),
        ] {
            let cred = move || CredentialOutcome::Probed(probe);
            let mut p = probes(None, &claude, &cred);
            p.fallback_file = Some(false);
            let c = credential_check(&p);
            assert_eq!(c.result, result);
            assert!(c.detail.contains("no key file"), "{}", c.detail);
            if result == CheckResult::Fail {
                assert!(c.detail.starts_with("credential store unavailable"));
            }
        }
    }

    #[test]
    fn an_unavailable_store_with_a_fallback_key_file_is_a_warning_and_locked_is_not() {
        let claude = || ClaudeVersion::NotOnPath;
        for (probe, file, result) in [
            (CredentialProbe::Unavailable, Some(true), CheckResult::Warn),
            (CredentialProbe::Unavailable, Some(false), CheckResult::Fail),
            (CredentialProbe::Unavailable, None, CheckResult::Fail),
            (CredentialProbe::Locked, Some(true), CheckResult::Fail),
            (CredentialProbe::Failed, Some(true), CheckResult::Fail),
        ] {
            let cred = move || CredentialOutcome::Probed(probe);
            let mut p = probes(None, &claude, &cred);
            p.fallback_file = file;
            assert_eq!(credential_check(&p).result, result, "{probe:?} {file:?}");
        }
        let skipped = || CredentialOutcome::Skipped("probe disabled for this test");
        let p = probes(None, &claude, &skipped);
        let c = credential_check(&p);
        assert_eq!(c.result, CheckResult::Skip);
        assert_eq!(c.detail, "probe disabled for this test");
    }

    #[test]
    fn a_namespace_that_was_asked_for_never_reaches_the_real_service() {
        use std::cell::RefCell;
        let seen = RefCell::new(Vec::<String>::new());
        let probe = |service: &str| {
            seen.borrow_mut().push(service.to_owned());
            CredentialProbe::Works
        };
        let real = crate::keystore::SERVICE;
        // Unset or empty: no namespace was asked for, so the real service is probed.
        for value in [None, Some(OsStr::new(""))] {
            seen.borrow_mut().clear();
            assert!(matches!(
                credential_outcome(value, None, &probe),
                CredentialOutcome::Probed(CredentialProbe::Works)
            ));
            assert_eq!(*seen.borrow(), [real]);
        }
        // Set but not a namespace name (so no scope can be made from it): never probed.
        for bad in [
            "Bad!",
            "UPPER",
            "a b",
            "this-name-is-far-too-long-for-a-scope",
            "x/y",
        ] {
            seen.borrow_mut().clear();
            let out = credential_outcome(Some(OsStr::new(bad)), None, &probe);
            assert!(matches!(out, CredentialOutcome::Skipped(_)), "{bad}");
            assert!(seen.borrow().is_empty(), "{bad}: {:?}", seen.borrow());
        }
        // A valid namespace probes its own test service, never the real one.
        seen.borrow_mut().clear();
        let _ = credential_outcome(Some(OsStr::new("abc1")), None, &probe);
        assert_eq!(seen.borrow().len(), 1);
        assert_ne!(seen.borrow()[0], real);
        // The test switch skips a valid namespace, and does nothing without one.
        seen.borrow_mut().clear();
        let off = Some(OsStr::new("1"));
        assert_eq!(
            credential_outcome(Some(OsStr::new("abc1")), off, &probe),
            CredentialOutcome::Skipped("probe disabled for this test")
        );
        assert!(seen.borrow().is_empty());
        let _ = credential_outcome(None, off, &probe);
        assert_eq!(
            *seen.borrow(),
            [real],
            "the switch is ignored outside a namespace"
        );
    }

    #[test]
    fn an_unusable_run_directory_fails_the_permission_check_and_the_daemon_check() {
        let (cred, claude) = (
            || CredentialOutcome::Probed(CredentialProbe::Works),
            || ClaudeVersion::NotOnPath,
        );
        let p = probes(None, &claude, &cred);
        let scope = Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the run directory is accessible to other users",
        ));
        let checks = run(&scope, &p);
        assert_eq!(
            by_name(&checks, "ipc_permissions").result,
            CheckResult::Fail
        );
        assert_eq!(by_name(&checks, "daemon").result, CheckResult::Fail);
        assert_eq!(by_name(&checks, "transport").result, CheckResult::Skip);
        assert_eq!(by_name(&checks, "pairing").result, CheckResult::Skip);
        assert_eq!(by_name(&checks, "codex").result, CheckResult::Skip);
        assert!(!all_clear(&checks));
    }

    #[test]
    fn the_report_is_canonical_json_and_a_plain_list() {
        let checks = vec![
            check("daemon", CheckResult::Fail, "no daemon; run `oac start`"),
            check("codex", CheckResult::Skip, "bad\u{1b}[2J"),
        ];
        let json = render_json(&checks);
        assert_eq!(canonical_text(&json), json);
        assert!(json.contains(r#""oac_cli":1"#) && json.contains(r#""ok":false"#));
        assert!(json.contains(r#""result":"fail""#), "{json}");
        let human = render_human(&checks);
        assert!(human.starts_with("FAIL  daemon: no daemon; run `oac start`\n"));
        assert!(human.contains("SKIP  codex: bad?[2J"), "{human}");
        assert!(human.ends_with("0 pass, 0 warn, 1 fail, 1 skip\n"));
    }

    #[test]
    fn a_relative_path_entry_is_never_searched() {
        assert_eq!(find_on_path(None), None);
        assert_eq!(find_on_path(Some(OsStr::new(""))), None);
        let relative = std::env::join_paths(["."]).unwrap();
        assert_eq!(find_on_path(Some(&relative)), None);
    }

    #[test]
    fn the_probe_service_in_a_test_namespace_is_not_the_users() {
        assert_eq!(credential_service(None), crate::keystore::SERVICE);
        assert_ne!(credential_service(Some("abc")), crate::keystore::SERVICE);
        assert!(credential_service(Some("abc")).ends_with(".test.abc"));
    }
}
