// SPDX-License-Identifier: Apache-2.0

//! `--json` on `oac start`, `oac mcp-shim` and `oac hook` (G10, #71, slice 7; lead decision
//! 2026-10-20: every verb takes `--json`), with real processes.
//!
//! - **`oac start --json`** is never run here (it would make the user's device key). The
//!   detached path, which is the part that reads a child and a daemon, is run with the
//!   `live_daemon` example as the stand-in daemon, started by the same function
//!   `oac start --detach` uses (`start_detached_command_to`), in a test namespace; the rendering
//!   and the foreground contract are unit-tested in `cli/src/commands.rs`.
//! - **`oac mcp-shim --json`** is the real shim against a real daemon process: the shim's
//!   standard output is the harness's MCP stream, and carries the daemon's reply and nothing
//!   else; its diagnostics are JSON lines on standard error.
//! - **`oac hook --json`**: standard output empty, exit 0, standard error one JSON object.
//!
//! Every read from a child is on a thread with a bound, and every child is killed when its
//! guard drops (a known Linux hang, #435).

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use oac_cli::ipc::Scope;
use oac_core::json::{self, JsonObject};

const WAIT: Duration = Duration::from_secs(30);

fn oac() -> &'static str {
    env!("CARGO_BIN_EXE_oac")
}

fn example() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // deps
    path.pop(); // debug
    path.push("examples");
    path.push(format!("live_daemon{}", std::env::consts::EXE_SUFFIX));
    path
}

/// A private runtime directory per call (Unix), so that no test makes anything in the user's
/// own, removed when it drops.
struct Namespace {
    name: String,
    #[cfg(unix)]
    dir: Option<std::path::PathBuf>,
}

impl Namespace {
    fn new(tag: &str) -> Namespace {
        let name = format!("jv{}{tag}", std::process::id());
        #[cfg(unix)]
        let dir = {
            let scope = Scope::for_user_in(Some(&name)).unwrap();
            std::path::Path::new(&scope.endpoint("claude").display_name())
                .parent()
                .map(std::path::Path::to_path_buf)
        };
        Namespace {
            name,
            #[cfg(unix)]
            dir,
        }
    }

    fn scope(&self) -> Scope {
        Scope::for_user_in(Some(&self.name)).unwrap()
    }
}

#[cfg(unix)]
impl Drop for Namespace {
    fn drop(&mut self) {
        if let Some(dir) = self.dir.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// A child, killed and reaped when it drops.
struct Guard(Child);

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The text of a child's pipe, read on a thread: lines arrive on a channel, and the channel
/// disconnects when the stream ends.
fn lines_of(stream: impl Read + Send + 'static) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

/// Everything left on the channel until it disconnects, for at most [`WAIT`].
fn rest(rx: &mpsc::Receiver<String>) -> Vec<String> {
    let end = Instant::now() + WAIT;
    let mut out = Vec::new();
    loop {
        let left = end.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(line) => out.push(line),
            Err(mpsc::RecvTimeoutError::Disconnected) => return out,
            Err(mpsc::RecvTimeoutError::Timeout) => panic!("the stream did not end: {out:?}"),
        }
    }
}

fn exit_of(child: &mut Child) -> Option<i32> {
    let end = Instant::now() + WAIT;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code();
        }
        assert!(Instant::now() < end, "the process did not exit in {WAIT:?}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Parses `text` as a JSON object that is already in canonical form with `"oac_cli":1`.
fn cli_object(text: &str) -> JsonObject {
    let value = json::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{text:?}: {e:?}"));
    assert_eq!(
        oac_cli::control::canonical_text(text),
        text,
        "not canonical: {text}"
    );
    let object = value.into_object().expect("an object");
    assert_eq!(
        object
            .get("oac_cli")
            .and_then(|v| v.as_number())
            .and_then(|n| n.plain_integer_in(0, 9)),
        Some(1),
        "{text}"
    );
    object
}

fn text_of<'a>(object: &'a JsonObject, name: &str) -> &'a str {
    object
        .get(name)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("no string {name}"))
}

fn flag_of(object: &JsonObject, name: &str) -> bool {
    object
        .get(name)
        .and_then(|v| v.as_bool())
        .unwrap_or_else(|| panic!("no bool {name}"))
}

fn number_of(object: &JsonObject, name: &str) -> u64 {
    object
        .get(name)
        .and_then(|v| v.as_number())
        .and_then(|n| n.plain_integer_in(0, u64::MAX))
        .unwrap_or_else(|| panic!("no number {name}"))
}

// ---- a daemon process (the stand-in for `oac start`) -----------------------------------

/// The `live_daemon` example in a namespace, ready, killed when it drops.
struct Daemon {
    child: Guard,
    events: std::path::PathBuf,
    state: std::path::PathBuf,
}

impl Daemon {
    fn command(ns: &Namespace, tag: &str) -> (Command, std::path::PathBuf, std::path::PathBuf) {
        let events = std::env::temp_dir().join(format!(
            "oac-jv-{tag}-{}-{}.jsonl",
            std::process::id(),
            ns.name
        ));
        let _ = std::fs::remove_file(&events);
        let state = std::env::temp_dir().join(format!("oac-jv-state-{tag}-{}", std::process::id()));
        let mut command = Command::new(example());
        command
            .args(["--namespace", &ns.name, "--events"])
            .arg(&events)
            .arg("--state-dir")
            .arg(&state)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        (command, events, state)
    }

    fn start(ns: &Namespace, tag: &str) -> Daemon {
        let (mut command, events, state) = Daemon::command(ns, tag);
        let child = Guard(command.spawn().expect("the example is built by cargo test"));
        let daemon = Daemon {
            child,
            events,
            state,
        };
        daemon.wait_for("ready");
        daemon
    }

    fn wait_for(&self, event: &str) {
        let needle = format!("\"event\":\"{event}\"");
        let end = Instant::now() + WAIT;
        while !std::fs::read_to_string(&self.events)
            .unwrap_or_default()
            .contains(&needle)
        {
            assert!(Instant::now() < end, "no {event:?} event");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.events);
        let _ = std::fs::remove_dir_all(&self.state);
    }
}

/// `oac stop` for the namespace, bounded: ends a daemon that was started detached.
struct StopOnDrop(String);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        if let Ok(mut stop) = Command::new(oac())
            .args(["stop", "--json"])
            .env("OAC_SCOPE", &self.0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            let end = Instant::now() + WAIT;
            while stop.try_wait().ok().flatten().is_none() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(25));
            }
            let _ = stop.kill();
            let _ = stop.wait();
        }
    }
}

// ---- oac start --detach --json ----------------------------------------------------------

#[test]
fn a_detached_start_prints_one_object_with_the_pid_and_the_log_and_a_second_says_already_running() {
    let ns = Namespace::new("s");
    let scope = ns.scope();
    let _stop = StopOnDrop(ns.name.clone());
    let (mut command, events, state) = Daemon::command(&ns, "start");
    let log = state.join("the-log");
    let mut said = Vec::new();
    let mut out = Vec::new();
    let status = oac_cli::commands::start_detached_command_to(
        &mut command,
        &scope,
        WAIT,
        Some(&log),
        true,
        &mut out,
        &mut said,
    );
    let said = String::from_utf8_lossy(&said).into_owned();
    assert_eq!(status, 0, "{said}");
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text.lines().count(), 1, "exactly one object: {text:?}");
    let object = cli_object(text.trim_end());
    assert!(flag_of(&object, "ok"), "{text}");
    assert!(!flag_of(&object, "already_running"), "{text}");
    assert!(flag_of(&object, "detached"), "{text}");
    assert_eq!(text_of(&object, "log"), log.display().to_string());
    // The pid is the daemon's: the one its own `ready` event records.
    let pid = number_of(&object, "pid");
    let end = Instant::now() + WAIT;
    let recorded = loop {
        let events = std::fs::read_to_string(&events).unwrap_or_default();
        if events.contains("\"event\":\"ready\"") {
            break events;
        }
        assert!(Instant::now() < end, "no ready event");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(recorded.contains(&format!("\"pid\":{pid}")), "{recorded}");

    // A second start finds a daemon already up and names it.
    let (mut second, events2, state2) = Daemon::command(&ns, "start2");
    let mut said = Vec::new();
    let mut out = Vec::new();
    let status = oac_cli::commands::start_detached_command_to(
        &mut second,
        &scope,
        WAIT,
        Some(&log),
        true,
        &mut out,
        &mut said,
    );
    let _ = std::fs::remove_file(events2);
    let _ = std::fs::remove_dir_all(state2);
    let said = String::from_utf8_lossy(&said).into_owned();
    assert_eq!(status, 0, "{said}");
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text.lines().count(), 1, "{text:?}");
    let object = cli_object(text.trim_end());
    assert!(flag_of(&object, "already_running"), "{text}");
    assert_eq!(number_of(&object, "pid"), pid, "{text}");
    let _ = std::fs::remove_file(events);
    let _ = std::fs::remove_dir_all(state);
}

#[test]
fn a_detached_start_that_fails_prints_the_error_object_and_exits_as_before() {
    let ns = Namespace::new("f");
    let scope = ns.scope();
    // Something holds the namespace's lock and serves nothing, so the daemon stops at the
    // lock and nothing answers on the control endpoint.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let lock = runtime
        .block_on(scope.acquire_lock())
        .expect("the lock is free");
    let (mut command, events, state) = Daemon::command(&ns, "fail");
    let mut said = Vec::new();
    let mut out = Vec::new();
    let status = oac_cli::commands::start_detached_command_to(
        &mut command,
        &scope,
        WAIT,
        Some(&state.join("the-log")),
        true,
        &mut out,
        &mut said,
    );
    let said = String::from_utf8_lossy(&said).into_owned();
    assert_eq!(status, 1, "{said}");
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text.lines().count(), 1, "{text:?}");
    let object = cli_object(text.trim_end());
    assert!(!flag_of(&object, "ok"), "{text}");
    assert_eq!(text_of(&object, "error"), "failed");
    assert!(
        text_of(&object, "detail").contains("ended at start-up"),
        "{text}"
    );
    // The words are still on standard error, as without --json.
    assert!(said.contains("ended at start-up"), "{said}");
    // Without --json: the same status, nothing on standard output.
    let (mut again, events2, state2) = Daemon::command(&ns, "fail2");
    let mut out = Vec::new();
    let mut said = Vec::new();
    let status = oac_cli::commands::start_detached_command_to(
        &mut again, &scope, WAIT, None, false, &mut out, &mut said,
    );
    assert_eq!(status, 1);
    assert!(out.is_empty());
    drop(lock);
    for path in [events, events2] {
        let _ = std::fs::remove_file(path);
    }
    for path in [state, state2] {
        let _ = std::fs::remove_dir_all(path);
    }
}

// ---- oac mcp-shim --json ----------------------------------------------------------------

const INITIALIZE: &str = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"clientInfo\":{\"name\":\"claude-code\",\"version\":\"2.1.285\"}}}\n";

/// A real shim: its standard input is held, its standard output and error are read on threads.
struct Shim {
    child: Guard,
    stdin: Option<ChildStdin>,
    stdout: mpsc::Receiver<String>,
    stderr: mpsc::Receiver<String>,
}

impl Shim {
    fn start(ns: &Namespace, args: &[&str]) -> Shim {
        let mut child = Command::new(oac())
            .args(args)
            .env("OAC_SCOPE", &ns.name)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("oac starts");
        let stdin = child.stdin.take();
        let stdout = lines_of(child.stdout.take().unwrap());
        let stderr = lines_of(child.stderr.take().unwrap());
        Shim {
            child: Guard(child),
            stdin,
            stdout,
            stderr,
        }
    }

    fn send(&mut self, text: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(text.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }
}

#[test]
fn a_json_shim_carries_the_daemons_reply_to_standard_output_and_nothing_else() {
    let ns = Namespace::new("m");
    let _daemon = Daemon::start(&ns, "shim");
    let mut shim = Shim::start(&ns, &["mcp-shim", "--harness", "claude", "--json"]);
    shim.send(INITIALIZE);
    // The one reply is a JSON-RPC answer to the request, the adapter's own bytes.
    let reply = shim
        .stdout
        .recv_timeout(WAIT)
        .expect("the daemon's answer reaches standard output");
    let answer = json::parse(reply.as_bytes()).expect("a JSON-RPC message");
    let answer = answer.as_object().expect("an object");
    assert_eq!(
        answer.get("jsonrpc").and_then(|v| v.as_str()),
        Some("2.0"),
        "{reply}"
    );
    assert!(answer.contains("result"), "{reply}");
    assert_eq!(
        answer
            .get("id")
            .and_then(|v| v.as_number())
            .and_then(|n| n.plain_integer_in(0, 9)),
        Some(1),
        "{reply}"
    );
    assert!(
        !answer.contains("oac_cli"),
        "no OAC diagnostic in the MCP stream: {reply}"
    );
    // The harness ends the session: closing the input ends the shim with 0, and standard
    // output held that one reply and nothing more (no diagnostic, JSON or words).
    shim.stdin = None;
    assert_eq!(exit_of(&mut shim.child.0), Some(0));
    assert_eq!(rest(&shim.stdout), Vec::<String>::new());
    // Nothing went wrong, so standard error is empty: the diagnostics are for failures.
    assert_eq!(rest(&shim.stderr), Vec::<String>::new());
}

#[test]
fn a_json_shim_whose_daemon_goes_away_says_so_as_one_json_line_on_standard_error() {
    let ns = Namespace::new("g");
    let mut daemon = Daemon::start(&ns, "gone");
    let mut shim = Shim::start(&ns, &["mcp-shim", "--harness", "claude", "--json"]);
    shim.send(INITIALIZE);
    let reply = shim.stdout.recv_timeout(WAIT).expect("a reply");
    assert!(reply.contains("\"result\""), "{reply}");
    // The daemon ends (a hard kill: the connection closes).
    let _ = daemon.child.0.kill();
    let _ = daemon.child.0.wait();
    // The exit status is the one without --json: 1.
    assert_eq!(exit_of(&mut shim.child.0), Some(1));
    assert_eq!(
        rest(&shim.stdout),
        Vec::<String>::new(),
        "standard output is the harness's: nothing after the reply"
    );
    let errors = rest(&shim.stderr);
    assert_eq!(errors.len(), 1, "one JSON line: {errors:?}");
    let object = cli_object(&errors[0]);
    assert!(!flag_of(&object, "ok"));
    assert_eq!(text_of(&object, "error"), "daemon_closed");
    assert_eq!(text_of(&object, "namespace"), ns.name);
}

#[test]
fn a_json_shim_with_no_daemon_exits_one_with_one_json_line_and_an_empty_standard_output() {
    let ns = Namespace::new("n");
    let mut shim = Shim::start(&ns, &["mcp-shim", "--json", "--harness", "claude"]);
    assert_eq!(exit_of(&mut shim.child.0), Some(1));
    assert_eq!(rest(&shim.stdout), Vec::<String>::new());
    let errors = rest(&shim.stderr);
    assert_eq!(errors.len(), 1, "{errors:?}");
    let object = cli_object(&errors[0]);
    assert!(!flag_of(&object, "ok"));
    assert_eq!(text_of(&object, "error"), "not_running");
    assert!(text_of(&object, "detail").contains("`oac start`"));
    assert_eq!(text_of(&object, "namespace"), ns.name);
}

#[test]
fn a_shim_without_json_keeps_its_words_and_exit_status() {
    let ns = Namespace::new("w");
    let mut shim = Shim::start(&ns, &["mcp-shim", "--harness", "claude"]);
    assert_eq!(exit_of(&mut shim.child.0), Some(1));
    assert_eq!(rest(&shim.stdout), Vec::<String>::new());
    let said = rest(&shim.stderr).join("\n");
    assert!(said.contains("the daemon is not running"), "{said}");
    assert!(!said.contains("oac_cli"), "{said}");
}

#[test]
fn the_shim_takes_json_before_or_after_its_harness_and_nothing_else() {
    let ns = Namespace::new("u");
    for args in [
        &["mcp-shim", "--json"][..],
        &["mcp-shim", "--json", "--json", "--harness", "claude"][..],
        &["mcp-shim", "--harness", "claude", "--json", "x"][..],
        &["mcp-shim", "--harness", "control", "--json"][..],
    ] {
        let mut shim = Shim::start(&ns, args);
        assert_eq!(exit_of(&mut shim.child.0), Some(2), "{args:?}");
        assert_eq!(rest(&shim.stdout), Vec::<String>::new(), "{args:?}");
        let said = rest(&shim.stderr).join("\n");
        assert!(
            said.contains("usage: oac mcp-shim --harness <claude|codex> [--json]"),
            "{args:?}: {said}"
        );
    }
}

// ---- oac hook --json --------------------------------------------------------------------

const SESSION_START: &str = "{\"session_id\":\"SECRET-SESSION\",\"cwd\":\"C:\\\\secret\",\"hook_event_name\":\"SessionStart\",\"source\":\"startup\"}\n";

fn hook(ns: &Namespace, args: &[&str], input: &str) -> (Option<i32>, Vec<u8>, Vec<String>) {
    let mut child = Command::new(oac())
        .arg("hook")
        .args(args)
        .env("OAC_SCOPE", &ns.name)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("oac starts");
    let mut stdin = child.stdin.take().unwrap();
    let _ = stdin.write_all(input.as_bytes());
    drop(stdin);
    let mut stdout = child.stdout.take().unwrap();
    let (tx, out) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        let _ = tx.send(bytes);
    });
    let stderr = lines_of(child.stderr.take().unwrap());
    let mut guard = Guard(child);
    let code = exit_of(&mut guard.0);
    let bytes = out.recv_timeout(WAIT).expect("standard output ended");
    (code, bytes, rest(&stderr))
}

#[test]
fn a_json_hook_writes_nothing_to_standard_output_exits_zero_and_describes_itself_on_standard_error()
{
    let ns = Namespace::new("h");
    // No daemon: the signal is lost; standard error says so as a JSON object.
    let (code, stdout, stderr) = hook(&ns, &["--json"], SESSION_START);
    assert_eq!(code, Some(0));
    assert!(stdout.is_empty(), "standard output is added to context");
    assert_eq!(stderr.len(), 1, "{stderr:?}");
    let object = cli_object(&stderr[0]);
    assert!(!flag_of(&object, "ok"));
    assert!(!flag_of(&object, "sent"));
    assert_eq!(text_of(&object, "error"), "not_running");
    assert!(text_of(&object, "detail").contains("`oac start`"));
    // Nothing from the input.
    assert!(!stderr[0].contains("SECRET") && !stderr[0].contains("secret"));

    // Inputs that are not a signal: nothing is sent, by design; ok stays true.
    for (input, reason) in [
        (
            "{\"hook_event_name\":\"SessionEnd\",\"session_id\":\"s\"}",
            "not_a_signal",
        ),
        ("not json", "not_an_object"),
        ("", "not_an_object"),
    ] {
        let (code, stdout, stderr) = hook(&ns, &["--json"], input);
        assert_eq!(code, Some(0), "{input:?}");
        assert!(stdout.is_empty(), "{input:?}");
        assert_eq!(stderr.len(), 1, "{input:?}: {stderr:?}");
        let object = cli_object(&stderr[0]);
        assert!(flag_of(&object, "ok"), "{input:?}");
        assert!(!flag_of(&object, "sent"), "{input:?}");
        assert_eq!(text_of(&object, "reason"), reason, "{input:?}");
    }

    // Another argument is noted as JSON too, and still never fails the hook.
    let (code, stdout, stderr) = hook(&ns, &["--json", "extra"], "");
    assert_eq!(code, Some(0));
    assert!(stdout.is_empty());
    assert_eq!(stderr.len(), 2, "{stderr:?}");
    assert_eq!(
        text_of(&cli_object(&stderr[0]), "notice"),
        "arguments_ignored"
    );
    cli_object(&stderr[1]);
}

#[test]
fn a_json_hook_that_reached_the_daemon_says_sent_and_still_writes_nothing_to_standard_output() {
    let ns = Namespace::new("k");
    let _daemon = Daemon::start(&ns, "hook");
    let (code, stdout, stderr) = hook(&ns, &["--json"], SESSION_START);
    assert_eq!(code, Some(0));
    assert!(stdout.is_empty());
    assert_eq!(stderr.len(), 1, "{stderr:?}");
    let object = cli_object(&stderr[0]);
    assert!(flag_of(&object, "ok"));
    assert!(flag_of(&object, "sent"));
}

#[test]
fn a_hook_without_json_is_unchanged() {
    let ns = Namespace::new("p");
    let (code, stdout, stderr) = hook(&ns, &[], SESSION_START);
    assert_eq!(code, Some(0));
    assert!(stdout.is_empty());
    let said = stderr.join("\n");
    assert!(said.contains("the daemon was not reached"), "{said}");
    assert!(!said.contains("oac_cli"), "{said}");
    // The test-namespace sentence is still said without --json.
    assert!(said.contains(&ns.name), "{said}");
}
