// SPDX-License-Identifier: Apache-2.0

//! `oac doctor` (G10, #71, slice 5), with real processes where the earlier slices used them.
//!
//! The daemon runs in this process over the in-memory transport, in a test namespace, as in
//! `tests/status.rs`; `oac start` itself is never run (it would make the user's device key).
//! The `oac doctor` that checks it is a separate process (`oac` built by cargo). The default
//! tier never touches the host's real credential store: every run of the binary sets the
//! test-only switch `OAC_DOCTOR_NO_CREDENTIAL_PROBE` (honoured only inside a test namespace),
//! which makes `credential_store` SKIP "probe disabled for this test". The opt-in test at the
//! end probes a test service for real. Every read from a child is on a thread with a bound,
//! and every child is killed when its guard is dropped, a panic included (a known Linux
//! hang, #435).
//!
//! Proven here: no daemon is `daemon` FAIL, `transport` SKIP and exit 1; a running daemon
//! passes the checks that can pass, and zero trusted devices and grants is a `pairing`
//! WARN; a Claude client that probed for the stateless revision and never initialised makes
//! `mcp_revision` FAIL; `MCP_PROTOCOL_NEGOTIATION` other than `legacy` is a WARN; the
//! `claude --version` probe is bounded and warns on an old version; a bad argument is exit 2.

use std::future::Future;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use oac_adapter_claude::ClaudeAdapter;
use oac_cli::daemon::{Daemon, LogSink, Options, Parts};
use oac_cli::doctor::{self, ClaudeVersion, CredentialOutcome, Probes};
use oac_cli::ipc::Scope;
use oac_cli::keystore::CredentialProbe;
use oac_core::clock::{Clock, SystemClock};
use oac_core::ids::Token;
use oac_core::json::{self, Json, JsonObject};
use oac_core::keys::MemoryKeyStore;
use oac_core::pairing::MemoryPairingStore;
use oac_core::pipeline::PipelineConfig;
use oac_core::sealing::MemoryAgreementKeyStore;
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};
use tokio::runtime::Handle;

const WAIT: Duration = Duration::from_secs(30);

/// The recorded `server/discover` probe of Claude Code `2.1.283` (D6 line 3): the first thing
/// a client that can speak the stateless revision sends.
const PROBE: &str = "{\"jsonrpc\":\"2.0\",\"id\":\"server-discover-probe-1\",\"method\":\"server/discover\",\"params\":{\"_meta\":{\"io.modelcontextprotocol/protocolVersion\":\"2026-07-28\",\"io.modelcontextprotocol/clientInfo\":{\"name\":\"claude-code\",\"version\":\"2.1.283\"},\"io.modelcontextprotocol/clientCapabilities\":{}}}}\n";

fn oac() -> &'static str {
    env!("CARGO_BIN_EXE_oac")
}

/// A current-thread runtime driven by a thread of its own (as in `tests/status.rs`).
struct Rt {
    handle: Handle,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Rt {
    fn new() -> Rt {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = rt.handle().clone();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let thread = std::thread::spawn(move || {
            rt.block_on(async {
                let _ = stopped.await;
            });
        });
        Rt {
            handle,
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    fn run<T: Send + 'static>(&self, f: impl Future<Output = T> + Send + 'static) -> T {
        let (tx, rx) = mpsc::channel();
        self.handle.spawn(async move {
            let _ = tx.send(f.await);
        });
        rx.recv_timeout(WAIT)
            .expect("the runtime finished the task")
    }
}

impl Drop for Rt {
    fn drop(&mut self) {
        let _ = self.stop.take().unwrap().send(());
        let _ = self.thread.take().unwrap().join();
    }
}

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Removes the namespace directory a Unix test made (Windows: nothing is made).
struct NamespaceDir(#[cfg(unix)] Option<PathBuf>);

impl NamespaceDir {
    fn of(scope: &Scope) -> NamespaceDir {
        #[cfg(unix)]
        return NamespaceDir(
            Path::new(&scope.endpoint("claude").display_name())
                .parent()
                .map(Path::to_path_buf),
        );
        #[cfg(not(unix))]
        {
            let _ = scope;
            NamespaceDir()
        }
    }
}

impl Drop for NamespaceDir {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(dir) = self.0.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// A directory removed when the test ends however it ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("oac-doctor-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A daemon in this process, in its own namespace, with an owner loop like `oac start`'s.
struct Rig {
    _rt: Rt,
    namespace: String,
    scope: Scope,
    _dir: NamespaceDir,
}

impl Rig {
    fn start(tag: &str) -> Rig {
        let namespace = format!("dr{}{tag}", std::process::id());
        let scope = Scope::for_user_in(Some(&namespace)).unwrap();
        let dir = NamespaceDir::of(&scope);
        let network = MemoryNetwork::new();
        let net = network.clone();
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let parts = Parts {
            scope: scope.clone(),
            key_store: Box::new(MemoryKeyStore::new()),
            pairing_store: Box::new(MemoryPairingStore::new()),
            agreement_store: Box::new(MemoryAgreementKeyStore::new()),
            principal: Token::parse("test").unwrap(),
            transport: Arc::new(MemoryTransport::new()),
            transport_configuration: MemoryConfiguration::wrap(&network),
            clock,
            monotonic: Arc::new(move || net.now()),
            config: PipelineConfig::default(),
            harnesses: vec![("claude".to_owned(), Arc::new(ClaudeAdapter::new()))],
            log: LogSink::new(Captured::default()),
        };
        let rt = Rt::new();
        let daemon = rt
            .run(Daemon::start_with(parts, Options::default()))
            .unwrap_or_else(|e| panic!("the daemon starts: {e}"));
        let requested = daemon.stop_requested();
        rt.handle.spawn(async move {
            requested.await;
            daemon.shutdown().await;
        });
        Rig {
            _rt: rt,
            namespace,
            scope,
            _dir: dir,
        }
    }

    /// The checks, made in this process with the credential store and `claude` decided here.
    fn checks(&self, negotiation: Option<&str>) -> Vec<doctor::Check> {
        checks_in(&Ok(self.scope.clone()), negotiation)
    }

    fn oac_doctor(&self, args: &[&str], env: &[(&str, &str)], path: &Path) -> Finished {
        run_doctor(Some(&self.namespace), args, env, path)
    }
}

fn checks_in(scope: &std::io::Result<Scope>, negotiation: Option<&str>) -> Vec<doctor::Check> {
    let credential = || CredentialOutcome::Probed(CredentialProbe::Works);
    let claude = || ClaudeVersion::Output("2.1.285 (Claude Code)".to_owned());
    let probes = Probes {
        status_wait: Duration::from_secs(10),
        negotiation: negotiation.map(Into::into),
        credential: &credential,
        fallback_file: None,
        claude: &claude,
    };
    doctor::run(scope, &probes)
}

fn named<'a>(checks: &'a [doctor::Check], name: &str) -> &'a doctor::Check {
    checks
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no check {name}"))
}

struct Finished {
    status: Option<ExitStatus>,
    stdout: String,
    stderr: String,
}

impl Finished {
    fn code(&self) -> Option<i32> {
        self.status.expect("the command ended in time").code()
    }

    /// The `--json` report as `(name, result)` pairs, after checking it is one canonical line.
    fn report(&self) -> Vec<(String, String, String)> {
        assert_eq!(self.stdout.matches('\n').count(), 1, "{}", self.stdout);
        assert_eq!(
            oac_cli::control::canonical_text(self.stdout.trim_end()),
            self.stdout.trim_end(),
            "canonical"
        );
        let object = object(&self.stdout);
        assert_eq!(
            member(&object, "oac_cli").as_number().map(|n| n.raw()),
            Some("1")
        );
        member(&object, "checks")
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let c = c.as_object().unwrap();
                (
                    string(c, "name").to_owned(),
                    string(c, "result").to_owned(),
                    string(c, "detail").to_owned(),
                )
            })
            .collect()
    }
}

fn result<'a>(report: &'a [(String, String, String)], name: &str) -> &'a str {
    report
        .iter()
        .find(|(n, ..)| n == name)
        .map(|(_, r, _)| r.as_str())
        .unwrap_or_else(|| panic!("no check {name}"))
}

/// Runs `oac doctor` with a controlled environment: no inherited negotiation variable, the
/// given `PATH` (so the real `claude` is never run), the given namespace.
fn run_doctor(
    namespace: Option<&str>,
    args: &[&str],
    env: &[(&str, &str)],
    path: &Path,
) -> Finished {
    let mut command = Command::new(oac());
    command
        .arg("doctor")
        .args(args)
        .env_remove(doctor::NEGOTIATION_VARIABLE)
        .env(doctor::NO_CREDENTIAL_PROBE_VARIABLE, "1")
        .env("PATH", path)
        .stdin(Stdio::null());
    match namespace {
        Some(n) => command.env("OAC_SCOPE", n),
        None => command.env_remove("OAC_SCOPE"),
    };
    for (k, v) in env {
        command.env(k, v);
    }
    run_bounded(&mut command, WAIT)
}

/// A child killed and reaped when the guard drops, however the test ends (a panic included).
struct Guard(Child);

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run_bounded(command: &mut Command, wait: Duration) -> Finished {
    let mut child = Guard(
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the command starts"),
    );
    let (out, err) = (
        output_reader(&mut child.0, true),
        output_reader(&mut child.0, false),
    );
    let status = wait_for_exit(&mut child.0, wait);
    Finished {
        status,
        stdout: out.recv_timeout(Duration::from_secs(5)).unwrap_or_default(),
        stderr: err.recv_timeout(Duration::from_secs(5)).unwrap_or_default(),
    }
}

fn output_reader(child: &mut Child, stdout: bool) -> mpsc::Receiver<String> {
    let mut source: Box<dyn Read + Send> = if stdout {
        Box::new(child.stdout.take().unwrap())
    } else {
        Box::new(child.stderr.take().unwrap())
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = source.read_to_string(&mut text);
        let _ = tx.send(text);
    });
    rx
}

fn wait_for_exit(child: &mut Child, wait: Duration) -> Option<ExitStatus> {
    let end = Instant::now() + wait;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        if Instant::now() >= end {
            return None;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn object(text: &str) -> JsonObject {
    json::parse(text.trim_end().as_bytes())
        .unwrap_or_else(|e| panic!("not JSON ({e}): {text:?}"))
        .into_object()
        .unwrap_or_else(|| panic!("not an object: {text:?}"))
}

fn member<'a>(o: &'a JsonObject, name: &str) -> &'a Json {
    o.get(name)
        .unwrap_or_else(|| panic!("no member {name:?} in {:?}", o.names().collect::<Vec<_>>()))
}

fn string<'a>(o: &'a JsonObject, name: &str) -> &'a str {
    member(o, name).as_str().unwrap_or_else(|| panic!("{name}"))
}

/// An empty directory to use as `PATH`: no `claude` on it.
fn empty_path() -> TempDir {
    TempDir::new("path")
}

// ---- no daemon --------------------------------------------------------------------------

#[test]
fn with_no_daemon_the_daemon_check_fails_the_transport_is_skipped_and_the_exit_is_1() {
    let namespace = format!("dr{}none", std::process::id());
    let path = empty_path();
    let out = run_doctor(Some(&namespace), &["--json"], &[], &path.0);
    assert_eq!(out.code(), Some(1), "{}{}", out.stdout, out.stderr);
    let report = out.report();
    assert_eq!(result(&report, "daemon"), "fail");
    assert!(
        report
            .iter()
            .any(|(n, _, d)| n == "daemon" && d == "no daemon; run `oac start`"),
        "{report:?}"
    );
    assert_eq!(result(&report, "transport"), "skip");
    assert_eq!(result(&report, "pairing"), "skip");
    assert_eq!(result(&report, "codex"), "skip");
    // No `claude` on PATH, and the negotiation variable is not set.
    assert_eq!(result(&report, "claude_version"), "skip");
    assert_eq!(result(&report, "mcp_revision"), "pass");
    // The default tier never probes the host's real credential store.
    assert_eq!(result(&report, "credential_store"), "skip");
    assert!(
        report
            .iter()
            .any(|(n, _, d)| n == "credential_store" && d == "probe disabled for this test"),
        "{report:?}"
    );
    // The human form: one line per check and a count.
    let human = run_doctor(Some(&namespace), &[], &[], &path.0);
    assert_eq!(human.code(), Some(1));
    assert!(
        human
            .stdout
            .contains("FAIL  daemon: no daemon; run `oac start`"),
        "{}",
        human.stdout
    );
    assert!(
        human.stdout.contains("SKIP  transport: no daemon to ask"),
        "{}",
        human.stdout
    );
    assert!(human.stderr.contains(&namespace), "{}", human.stderr);
}

#[test]
fn an_oac_scope_that_is_not_a_namespace_name_never_probes_the_credential_store() {
    // No scope can be made from the value, and the real credential service must not be used
    // in its place: the check is skipped, the run directory check fails, and the exit is 1.
    let path = empty_path();
    for bad in ["BAD!", "has space"] {
        let out = run_doctor(Some(bad), &["--json"], &[], &path.0);
        let report = out.report();
        assert_eq!(result(&report, "credential_store"), "skip", "{bad}: {report:?}");
        assert_eq!(result(&report, "daemon"), "fail", "{bad}");
        assert_eq!(out.code(), Some(1), "{bad}");
    }
}

#[test]
fn a_bad_argument_is_a_usage_error() {
    let path = empty_path();
    let out = run_doctor(Some("drusage"), &["--bogus"], &[], &path.0);
    assert_eq!(out.code(), Some(2), "{}", out.stderr);
    assert!(out.stdout.is_empty(), "{}", out.stdout);
    assert!(
        out.stderr.contains("usage: oac doctor [--json]"),
        "{}",
        out.stderr
    );
}

// ---- a running daemon -------------------------------------------------------------------

#[test]
fn a_running_daemon_passes_the_checks_that_can_pass_and_pairing_warns_with_no_keys_or_grants() {
    let rig = Rig::start("a");
    let checks = rig.checks(None);
    let daemon = named(&checks, "daemon");
    assert_eq!(daemon.result, doctor::CheckResult::Pass, "{daemon:?}");
    assert!(daemon.detail.contains("running"), "{daemon:?}");
    // On Unix having a private run directory passes the check; on Windows the client's
    // verification of the pipe's owner and access list, made while connecting, does.
    assert_eq!(
        named(&checks, "ipc_permissions").result,
        doctor::CheckResult::Pass
    );
    assert_eq!(
        named(&checks, "credential_store").result,
        doctor::CheckResult::Pass
    );
    assert_eq!(
        named(&checks, "transport").result,
        doctor::CheckResult::Pass
    );
    assert_eq!(
        named(&checks, "mcp_revision").result,
        doctor::CheckResult::Pass
    );
    let pairing = named(&checks, "pairing");
    assert_eq!(pairing.result, doctor::CheckResult::Warn, "{pairing:?}");
    assert_eq!(
        pairing.detail,
        "no pairings or grants; default deny, nothing is delivered"
    );
    assert_eq!(
        named(&checks, "claude_version").result,
        doctor::CheckResult::Pass
    );
    assert_eq!(named(&checks, "codex").result, doctor::CheckResult::Skip);
    assert!(doctor::all_clear(&checks), "warnings are not failures");

    // The same through the verb's own writer: exit 0 (a WARN does not fail).
    let credential = || CredentialOutcome::Probed(CredentialProbe::Works);
    let claude = || ClaudeVersion::NotOnPath;
    let probes = Probes {
        status_wait: Duration::from_secs(10),
        negotiation: None,
        credential: &credential,
        fallback_file: None,
        claude: &claude,
    };
    let mut out = Vec::new();
    let code = oac_cli::commands::doctor_with(&Ok(rig.scope.clone()), &probes, true, &mut out);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&out));

    // And the real binary, in the same namespace: the daemon and transport checks agree.
    // The credential store is not probed here (see the module comment).
    let path = empty_path();
    let real = rig.oac_doctor(&["--json"], &[], &path.0);
    let report = real.report();
    assert_eq!(result(&report, "daemon"), "pass", "{report:?}");
    assert_eq!(result(&report, "transport"), "pass");
    assert_eq!(result(&report, "pairing"), "warn");
    assert_eq!(result(&report, "mcp_revision"), "pass");
    assert_eq!(result(&report, "codex"), "skip");
    assert_eq!(result(&report, "credential_store"), "skip");
    assert_eq!(real.code(), Some(0), "{}", real.stdout);
}

#[test]
fn an_environment_that_forces_another_mcp_revision_is_a_warning_and_legacy_is_not() {
    let rig = Rig::start("b");
    let path = empty_path();
    let other = rig.oac_doctor(
        &["--json"],
        &[(doctor::NEGOTIATION_VARIABLE, "2026-07-28")],
        &path.0,
    );
    let report = other.report();
    assert_eq!(result(&report, "mcp_revision"), "warn", "{report:?}");
    let detail = &report.iter().find(|(n, ..)| n == "mcp_revision").unwrap().2;
    assert!(detail.contains(doctor::NEGOTIATION_VARIABLE), "{detail}");
    assert!(
        !detail.contains("2026-07-28"),
        "the value is not echoed: {detail}"
    );
    let legacy = rig.oac_doctor(
        &["--json"],
        &[(doctor::NEGOTIATION_VARIABLE, "legacy")],
        &path.0,
    );
    assert_eq!(result(&legacy.report(), "mcp_revision"), "pass");
    // A warning alone never fails the run.
    let checks = rig.checks(Some("2026-07-28"));
    assert_eq!(
        named(&checks, "mcp_revision").result,
        doctor::CheckResult::Warn
    );
    assert!(doctor::all_clear(&checks));
}

#[test]
fn a_claude_client_that_probed_and_never_initialised_makes_the_revision_check_fail() {
    let rig = Rig::start("c");
    // The client plays Claude Code limited to the stateless revision: it sends the probe,
    // takes the `-32601` answer, and goes away without a legacy `initialize`.
    let mut stream = rig
        .scope
        .endpoint("claude")
        .connect_blocking()
        .expect("the claude endpoint accepts a same-user client");
    stream.write_all(PROBE.as_bytes()).unwrap();
    stream.flush().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(&mut stream).read_line(&mut line);
        let _ = tx.send(line);
        // `stream` drops here: the connection ends with no `initialize`.
    });
    let answer = rx.recv_timeout(WAIT).expect("the server answers the probe");
    assert!(answer.contains("-32601"), "{answer}");
    // The adapter counts the connection when it ends; wait for the report to say so.
    let end = Instant::now() + WAIT;
    let checks = loop {
        let checks = rig.checks(None);
        if named(&checks, "mcp_revision").result == doctor::CheckResult::Fail
            || Instant::now() >= end
        {
            break checks;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let revision = named(&checks, "mcp_revision");
    assert_eq!(revision.result, doctor::CheckResult::Fail, "{revision:?}");
    assert_eq!(
        revision.detail,
        "a Claude session did not negotiate a legacy MCP revision"
    );
    assert!(!doctor::all_clear(&checks));
    // The real binary agrees, and exits 1.
    let path = empty_path();
    let real = rig.oac_doctor(&["--json"], &[], &path.0);
    assert_eq!(result(&real.report(), "mcp_revision"), "fail");
    assert_eq!(real.code(), Some(1));
}

// ---- the credential store ---------------------------------------------------------------

#[test]
fn the_probe_entry_is_not_the_device_keys() {
    assert_ne!(oac_cli::keystore::PROBE_ACCOUNT, oac_cli::keystore::ACCOUNT);
    assert_ne!(
        doctor::credential_service(Some("x")),
        oac_cli::keystore::SERVICE
    );
}

/// Opt-in (`oac-testing` §2): the real operating-system credential store, under a service of
/// a test namespace. Writes, reads back and deletes the probe entry, and leaves nothing. The
/// name contains `real_credential_store_round_trip`, so the keystore tier's existing filter
/// (`local-ci.mjs` steps `keystore-os-store` and `keystore-secret-service`) runs it.
#[test]
#[ignore = "opt-in: touches the real credential store; set OAC_TEST_REAL_KEYRING=1 and pass --ignored"]
fn doctor_probe_real_credential_store_round_trip() {
    assert_eq!(
        std::env::var("OAC_TEST_REAL_KEYRING").as_deref(),
        Ok("1"),
        "set OAC_TEST_REAL_KEYRING=1 to run the real credential-store test"
    );
    let service = doctor::credential_service(Some(&format!("t{}", std::process::id())));
    assert_eq!(
        oac_cli::keystore::probe_credential_store(&service),
        CredentialProbe::Works
    );
    // Run again: the first probe left no entry behind to trip over.
    assert_eq!(
        oac_cli::keystore::probe_credential_store(&service),
        CredentialProbe::Works
    );
}

// ---- `claude --version` -----------------------------------------------------------------

/// Writes a `claude` into a fresh directory: a script that runs `body`.
fn fake_claude(tag: &str, windows_body: &str, unix_body: &str) -> TempDir {
    let dir = TempDir::new(tag);
    if cfg!(windows) {
        std::fs::write(
            dir.0.join("claude.cmd"),
            format!("@echo off\r\n{windows_body}\r\n"),
        )
        .unwrap();
    } else {
        let path = dir.0.join("claude");
        std::fs::write(&path, format!("#!/bin/sh\n{unix_body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    dir
}

#[test]
fn claude_version_is_warned_about_below_the_minimum_and_passes_at_or_above() {
    let namespace = format!("dr{}ver", std::process::id());
    for (version, expected) in [("2.1.200", "warn"), ("2.1.290", "pass")] {
        let dir = fake_claude(
            &format!("v{}", version.replace('.', "")),
            &format!("echo {version} (Claude Code)"),
            &format!("echo '{version} (Claude Code)'"),
        );
        let out = run_doctor(Some(&namespace), &["--json"], &[], &dir.0);
        let report = out.report();
        assert_eq!(result(&report, "claude_version"), expected, "{report:?}");
        let detail = &report
            .iter()
            .find(|(n, ..)| n == "claude_version")
            .unwrap()
            .2;
        assert!(detail.contains(version), "{detail}");
    }
}

#[test]
fn a_claude_that_does_not_answer_is_ended_at_the_bound_and_is_a_warning() {
    let dir = fake_claude("hang", "ping -n 30 127.0.0.1 > nul", "sleep 30");
    let began = Instant::now();
    let probed = doctor::probe_claude(Some(dir.0.as_os_str()), Duration::from_millis(700));
    assert!(
        began.elapsed() < Duration::from_secs(10),
        "{:?}",
        began.elapsed()
    );
    assert!(matches!(probed, ClaudeVersion::Failed(_)), "{probed:?}");
}

#[test]
fn a_claude_that_errors_is_a_warning_and_never_a_failure() {
    let dir = fake_claude("err", "exit /b 3", "exit 3");
    let out = run_doctor(Some("drerr"), &["--json"], &[], &dir.0);
    let report = out.report();
    assert_eq!(result(&report, "claude_version"), "warn", "{report:?}");
}
