// SPDX-License-Identifier: Apache-2.0

//! The daemon's log file (G10, #71, slice 2; lead decision 2026-10-18, Q2: "logs should always
//! be preserved to survive a crash"), with real processes and the real file system.
//!
//! - an in-process daemon writes its lines to the file of a test namespace, and a test
//!   namespace never touches the user's own log;
//! - the lines survive a daemon that is killed hard (the `live_daemon` example, ended with
//!   the system's kill);
//! - rotation keeps at most two files and loses no line but the oldest;
//! - the file and its directory are private to the user (Unix: mode and owner; Windows: the
//!   DACL, read back by the code under test and, independently, by `icacls`);
//! - a symlink in the log's place is refused (Unix), and so is a directory or file that other
//!   users could enter or read;
//! - a detached start that fails shows the child's own reason and names the log file.
//!
//! The "no forbidden content" check (no working directory, no native session id) is in
//! `tests/daemon.rs`, where real sessions with both exist.
//!
//! `oac start` itself is never run (it would make the user's device key). Every wait is
//! bounded and a child is killed when its guard drops (#435).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use oac_adapter_claude::ClaudeAdapter;
use oac_cli::daemon::{Daemon, LogSink, Parts};
use oac_cli::ipc::Scope;
use oac_cli::logfile::{
    self, LOG_FILE_NAME, LogFile, MAX_LOG_BYTES, ROTATED_FILE_NAME, log_directory,
};
use oac_core::clock::{Clock, SystemClock};
use oac_core::ids::Token;
use oac_core::keys::MemoryKeyStore;
use oac_core::pairing::MemoryPairingStore;
use oac_core::pipeline::PipelineConfig;
use oac_core::sealing::MemoryAgreementKeyStore;
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};

const WAIT: Duration = Duration::from_secs(30);

fn example() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // deps
    path.pop(); // debug
    path.push("examples");
    path.push(format!("live_daemon{}", std::env::consts::EXE_SUFFIX));
    path
}

/// A directory of the test's own, removed when it drops.
struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!("oac-logfile-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Temp(dir)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// On Unix the endpoints of a namespace are a directory under the real run directory.
struct NamespaceDir(#[cfg(unix)] Option<PathBuf>);

impl NamespaceDir {
    #[cfg_attr(not(unix), allow(unused_variables))]
    fn of(scope: &Scope) -> NamespaceDir {
        NamespaceDir(
            #[cfg(unix)]
            Path::new(&scope.endpoint("claude").display_name())
                .parent()
                .map(Path::to_path_buf),
        )
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

/// A child killed and reaped when the guard drops.
struct Kill(Child);

impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let end = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn parts(scope: Scope, network: &MemoryNetwork, log: LogSink) -> Parts {
    let net = network.clone();
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    Parts {
        scope,
        key_store: Box::new(MemoryKeyStore::new()),
        pairing_store: Box::new(MemoryPairingStore::new()),
        agreement_store: Box::new(MemoryAgreementKeyStore::new()),
        principal: Token::parse("test").unwrap(),
        transport: Arc::new(MemoryTransport::new()),
        transport_configuration: MemoryConfiguration::wrap(network),
        clock,
        monotonic: Arc::new(move || net.now()),
        config: PipelineConfig::default(),
        harnesses: vec![("claude".to_owned(), Arc::new(ClaudeAdapter::new()) as _)],
        log,
    }
}

// ---- 1: an in-process daemon writes its lines to the file of its namespace -----------------

#[test]
fn an_in_process_daemon_writes_its_lines_to_the_file_of_its_test_namespace() {
    let namespace = format!("lf{}a", std::process::id());
    let scope = Scope::for_user_in(Some(&namespace)).unwrap();
    let _run_dir = NamespaceDir::of(&scope);
    // A state directory of the test's own: nothing in the user's is touched.
    let temp = Temp::new("inproc");
    let file = LogFile::open_in(&temp.0, Some(&namespace), MAX_LOG_BYTES)
        .expect("the log file of the namespace opens");
    let path = file.path();
    assert_eq!(
        path,
        log_directory(&temp.0, Some(&namespace))
            .unwrap()
            .join(LOG_FILE_NAME),
        "the namespace's log is in the namespace's directory"
    );
    assert!(
        path.to_string_lossy().contains(&format!("ns-{namespace}")),
        "{}",
        path.display()
    );
    assert!(
        !log_directory(&temp.0, None)
            .unwrap()
            .join(LOG_FILE_NAME)
            .exists()
    );

    let sink = LogSink::new(std::io::sink()).with_file(file);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let network = MemoryNetwork::new();
    runtime.block_on(async {
        let daemon = Daemon::start(parts(scope, &network, sink))
            .await
            .unwrap_or_else(|e| panic!("the daemon starts: {e}"));
        daemon.shutdown().await;
    });

    let kept = read(&path);
    assert!(kept.contains("oac-daemon: serving"), "{kept}");
    assert!(kept.contains("oac-daemon: shut down"), "{kept}");
    assert!(
        kept.lines().all(|l| l.starts_with("unix_ms=")),
        "every line carries its time: {kept}"
    );
}

// ---- 2: the lines survive a daemon killed hard -------------------------------------------

#[test]
fn lines_survive_a_daemon_that_is_killed_hard() {
    let temp = Temp::new("kill");
    let namespace = format!("lf{}b", std::process::id());
    let scope = Scope::for_user_in(Some(&namespace)).unwrap();
    let _run_dir = NamespaceDir::of(&scope);
    let events = temp.0.join("events.jsonl");
    let state = temp.0.join("state");
    let child = Command::new(example())
        .args(["--namespace", &namespace, "--events"])
        .arg(&events)
        .arg("--state-dir")
        .arg(&state)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the example is built by `cargo test`");
    let mut daemon = Kill(child);
    wait_until("the daemon to be ready", || {
        read(&events).contains("\"event\":\"ready\"")
    });
    let log = log_directory(&state, Some(&namespace))
        .unwrap()
        .join(LOG_FILE_NAME);
    // The line written before the kill is already on disk: no clean shutdown is needed.
    daemon.0.kill().expect("the daemon is killed");
    let status = daemon.0.wait().expect("the daemon is reaped");
    assert!(!status.success(), "the daemon was killed: {status:?}");
    let recorded = read(&events);
    assert!(
        !recorded.contains("\"event\":\"stopped\""),
        "no clean shutdown ran: {recorded}"
    );
    let kept = read(&log);
    assert!(kept.contains("oac-daemon: serving"), "{kept}");
    assert!(!kept.contains("shut down"), "{kept}");
}

// ---- 3: rotation at the limit ------------------------------------------------------------

#[test]
fn the_log_rotates_at_its_limit_and_keeps_one_previous_file() {
    assert_eq!(MAX_LOG_BYTES, 1024 * 1024, "the limit is 1 MiB");
    let temp = Temp::new("rotate");
    let limit = 4096;
    let mut log = LogFile::open_in(&temp.0, Some("rot"), limit).unwrap();
    let dir = log_directory(&temp.0, Some("rot")).unwrap();
    let mut total = 0;
    for i in 0..300 {
        log.append(&format!("line number {i:04} {}", "x".repeat(20)))
            .unwrap();
        total += 1;
        for name in [LOG_FILE_NAME, ROTATED_FILE_NAME] {
            let size = std::fs::metadata(dir.join(name)).map_or(0, |m| m.len());
            assert!(size <= limit, "{name} is {size} bytes after line {i}");
        }
    }
    assert!(dir.join(ROTATED_FILE_NAME).exists(), "it rotated");
    assert!(!dir.join("daemon.log.2").exists(), "one previous file only");
    let current = read(&dir.join(LOG_FILE_NAME));
    let previous = read(&dir.join(ROTATED_FILE_NAME));
    assert!(current.contains("line number 0299"), "the newest is kept");
    assert!(!current.contains("line number 0000"), "the oldest is gone");
    assert!(!previous.contains("line number 0000"));
    // What is kept is contiguous and in order, and no line is split across the two files.
    let kept: Vec<&str> = previous.lines().chain(current.lines()).collect();
    assert!(kept.len() < total);
    let first: usize = kept[0]
        .split("line number ")
        .nth(1)
        .unwrap()
        .get(..4)
        .unwrap()
        .parse()
        .unwrap();
    for (offset, line) in kept.iter().enumerate() {
        assert!(
            line.contains(&format!("line number {:04} ", first + offset)) && line.ends_with('x'),
            "line {offset}: {line}"
        );
    }
}

// ---- 4: the permissions ------------------------------------------------------------------

#[test]
fn the_file_and_its_directory_are_private_to_the_user() {
    let temp = Temp::new("private");
    let mut log = LogFile::open_in(&temp.0, None, MAX_LOG_BYTES).unwrap();
    log.append("a line").unwrap();
    let file = log.path();
    let dir = file.parent().unwrap().to_path_buf();
    assert!(logfile::is_private(&file).unwrap());
    assert!(logfile::is_private(&dir).unwrap());

    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let euid = unsafe { libc::geteuid() };
        let f = std::fs::metadata(&file).unwrap();
        let d = std::fs::metadata(&dir).unwrap();
        assert_eq!(f.permissions().mode() & 0o777, 0o600);
        assert_eq!(d.permissions().mode() & 0o777, 0o700);
        assert_eq!((f.uid(), d.uid()), (euid, euid));
        // A previous file made by a rotation is as private.
        let mut small = LogFile::open_in(&temp.0, Some("rp"), 100).unwrap();
        for _ in 0..10 {
            small.append("rotate me please").unwrap();
        }
        let old = log_directory(&temp.0, Some("rp"))
            .unwrap()
            .join(ROTATED_FILE_NAME);
        assert_eq!(
            std::fs::metadata(old).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(windows)]
    {
        // The DACL read back by `icacls`, independently of the code that set it: one entry,
        // the user's, and none for Everyone, Users, Authenticated Users, SYSTEM or the
        // Administrators group.
        for (path, inherit) in [(&file, ""), (&dir, "(OI)(CI)")] {
            let out = Command::new("icacls")
                .arg(path)
                .stdin(Stdio::null())
                .output()
                .expect("icacls runs");
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            let entries = text.matches(":(").count();
            assert_eq!(entries, 1, "{text}");
            assert!(text.contains(&format!("{inherit}(F)")), "{text}");
            let user = std::env::var("USERNAME").unwrap();
            assert!(
                text.to_lowercase().contains(&user.to_lowercase()),
                "{user}: {text}"
            );
            for other in [
                "Everyone",
                "BUILTIN\\Users",
                "Authenticated Users",
                "SYSTEM",
                "Administrators",
            ] {
                assert!(!text.contains(other), "{other}: {text}");
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn a_directory_or_file_that_others_can_use_is_refused_and_not_repaired() {
    use std::os::unix::fs::PermissionsExt;
    let temp = Temp::new("loose");
    let dir = log_directory(&temp.0, Some("loose")).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let e = LogFile::open_in(&temp.0, Some("loose"), MAX_LOG_BYTES).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
    assert_eq!(
        std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o755,
        "not repaired"
    );

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let file = dir.join(LOG_FILE_NAME);
    std::fs::write(&file, "old\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let e = LogFile::open_in(&temp.0, Some("loose"), MAX_LOG_BYTES).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
    assert_eq!(read(&file), "old\n", "nothing was written");
}

// ---- 5: a symlinked log path is refused (Unix) -------------------------------------------

#[cfg(unix)]
#[test]
fn a_symlinked_log_path_is_refused() {
    let temp = Temp::new("link");
    let dir = log_directory(&temp.0, Some("lnk")).unwrap();
    // Made private by a first open, then the file is replaced by a link to a victim.
    drop(LogFile::open_in(&temp.0, Some("lnk"), MAX_LOG_BYTES).unwrap());
    let victim = temp.0.join("victim.txt");
    std::fs::write(&victim, "victim\n").unwrap();
    let file = dir.join(LOG_FILE_NAME);
    std::fs::remove_file(&file).unwrap();
    std::os::unix::fs::symlink(&victim, &file).unwrap();
    let e = LogFile::open_in(&temp.0, Some("lnk"), MAX_LOG_BYTES).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
    assert_eq!(
        read(&victim),
        "victim\n",
        "the link's target was not written"
    );

    // The same for the log directory.
    let elsewhere = temp.0.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let ns = log_directory(&temp.0, Some("lnk2")).unwrap();
    std::fs::create_dir_all(ns.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &ns).unwrap();
    assert!(LogFile::open_in(&temp.0, Some("lnk2"), MAX_LOG_BYTES).is_err());
    assert!(!elsewhere.join(LOG_FILE_NAME).exists());
}

// ---- failure handling: a log never stops the daemon ----------------------------------------

#[test]
fn a_log_file_that_cannot_be_opened_is_a_warning_and_the_daemon_runs() {
    let temp = Temp::new("noopen");
    // A file where the `OAC` directory should be: the log directory cannot be made.
    std::fs::write(temp.0.join("OAC"), "in the way").unwrap();
    assert!(LogFile::open_in(&temp.0, Some("blk"), MAX_LOG_BYTES).is_err());

    let namespace = format!("lf{}c", std::process::id());
    let scope = Scope::for_user_in(Some(&namespace)).unwrap();
    let _run_dir = NamespaceDir::of(&scope);
    let seen = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    struct Stream(Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Stream {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    // The sink as `oac start` builds it, with an invalid namespace standing for a file that
    // cannot be opened (it opens nothing): one warning on the stream, no file, the sink works.
    let sink = LogSink::new(Stream(seen.clone())).with_default_file_after_lock(Some("NOT A NAME"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let network = MemoryNetwork::new();
    runtime.block_on(async {
        let daemon = Daemon::start(parts(scope, &network, sink))
            .await
            .unwrap_or_else(|e| panic!("the daemon runs without a log file: {e}"));
        daemon.shutdown().await;
    });
    let text = String::from_utf8_lossy(&seen.lock().unwrap()).into_owned();
    assert_eq!(
        text.matches("warning: the log file could not be opened")
            .count(),
        1,
        "{text}"
    );
    assert!(text.contains("continuing without it"), "{text}");
    assert!(text.contains("oac-daemon: serving"), "{text}");
}

// ---- 7: a detached start that fails shows its reason ---------------------------------------

#[test]
fn a_detached_start_that_fails_relays_the_childs_reason_and_names_the_log() {
    let namespace = format!("lf{}d", std::process::id());
    let scope = Scope::for_user_in(Some(&namespace)).unwrap();
    let _run_dir = NamespaceDir::of(&scope);
    // A state directory of the test's own, with the running daemon's log in it.
    let temp = Temp::new("failed-start");
    let state = temp.0.join("state");
    let log_path = log_directory(&state, Some(&namespace))
        .unwrap()
        .join(LOG_FILE_NAME);
    let mut running = LogFile::open_in(&state, Some(&namespace), 200).unwrap();
    for i in 0..3 {
        running
            .append(&format!("the running daemon, line {i}"))
            .unwrap();
    }
    let before = (
        read(&log_path),
        std::fs::metadata(&log_path).unwrap().modified().ok(),
    );

    // Something holds the namespace's lock and serves nothing, so a second daemon stops at
    // the lock and nothing answers on the control endpoint.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let lock = runtime
        .block_on(scope.acquire_lock())
        .expect("the lock is free");

    let mut command = Command::new(example());
    command
        .args(["--namespace", &namespace, "--state-dir"])
        .arg(&state)
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    let mut said = Vec::new();
    let began = Instant::now();
    let status = oac_cli::commands::start_detached_command(
        &mut command,
        &scope,
        WAIT,
        Some(&log_path),
        &mut said,
    );
    let said = String::from_utf8_lossy(&said).into_owned();
    assert_eq!(status, 1, "{said}");
    assert!(
        began.elapsed() < WAIT,
        "reported at once, not at the end of the wait"
    );
    // The child's own reason, then where its log is.
    assert!(said.contains("already running"), "{said}");
    assert!(said.contains("ended at start-up"), "{said}");
    assert!(said.contains(&log_path.display().to_string()), "{said}");
    assert!(!said.contains("in a terminal"), "{said}");
    // The start that lost the race did not touch the running daemon's log: not a byte, not a
    // permission change (the daemon's own line would have been appended to it).
    let after = (
        read(&log_path),
        std::fs::metadata(&log_path).unwrap().modified().ok(),
    );
    assert_eq!(before, after);
    assert!(!log_path.with_file_name(ROTATED_FILE_NAME).exists());
    assert!(logfile::is_private(&log_path).unwrap());
    drop(lock);
    drop(runtime);
}

// ---- a rotation that fails never loses the current log -------------------------------------

/// Makes `daemon.log.1` impossible to replace (a non-empty directory is in its place), on
/// every system; the stand-in for a sharing violation, an antivirus scan or a viewer.
fn block_rotation(directory: &Path) {
    let previous = directory.join(ROTATED_FILE_NAME);
    std::fs::create_dir_all(&previous).unwrap();
    std::fs::write(previous.join("in-the-way"), "x").unwrap();
}

fn unblock_rotation(directory: &Path) {
    std::fs::remove_dir_all(directory.join(ROTATED_FILE_NAME)).unwrap();
}

#[test]
fn a_rotation_that_fails_keeps_the_current_log_and_logging_goes_on() {
    let temp = Temp::new("rotfail");
    let limit = 1000;
    let mut log = LogFile::open_in(&temp.0, Some("rf"), limit).unwrap();
    let dir = log_directory(&temp.0, Some("rf")).unwrap();
    let current = dir.join(LOG_FILE_NAME);
    block_rotation(&dir);

    // Past the limit the rotation fails every time; the lines still go to the current file,
    // which is never deleted, until the hard cap; then the file restarts with a marker line
    // and the newest lines are kept.
    let mut longest = 0;
    for i in 0..200 {
        log.append(&format!("line {i:04}")).unwrap();
        assert!(current.exists(), "the current log was deleted at line {i}");
        longest = longest.max(std::fs::metadata(&current).unwrap().len());
    }
    assert!(longest > limit, "the file grew past the limit: {longest}");
    assert!(
        longest <= limit * logfile::HARD_CAP_FACTOR + 200,
        "{longest}"
    );
    let kept = read(&current);
    let first = kept.lines().next().unwrap();
    assert!(
        first.contains("log restarted: ") && first.contains("bytes discarded, rotation failing ("),
        "the restarted file begins with the marker: {first}"
    );
    assert_eq!(
        kept.matches("log restarted").count(),
        1,
        "the marker appears exactly once: {kept}"
    );
    assert!(
        kept.lines().last().unwrap().ends_with("line 0199"),
        "the newest line is kept"
    );
    assert!(
        !kept.contains("line 0000"),
        "the oldest lines were discarded"
    );
    // What follows the marker is contiguous and in order.
    let numbers: Vec<usize> = kept
        .lines()
        .filter_map(|l| l.rsplit("line ").next().and_then(|n| n.parse().ok()))
        .collect();
    assert!(numbers.windows(2).all(|w| w[1] == w[0] + 1), "{numbers:?}");
    // The line right after the marker is the one that crossed the cap: the discarded bytes plus
    // that line are past the cap, and it is the first numbered line kept.
    let discarded: u64 = first
        .split("log restarted: ")
        .nth(1)
        .unwrap()
        .split(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let second = kept.lines().nth(1).unwrap();
    assert_eq!(
        second
            .rsplit("line ")
            .next()
            .unwrap()
            .parse::<usize>()
            .unwrap(),
        numbers[0],
        "{second}"
    );
    assert!(
        discarded + second.len() as u64 + 1 > limit * logfile::HARD_CAP_FACTOR,
        "{discarded} + {second}"
    );
    assert!(discarded <= limit * logfile::HARD_CAP_FACTOR);
    assert!(
        dir.join(ROTATED_FILE_NAME).join("in-the-way").exists(),
        "the previous log's place was not touched"
    );

    // The obstacle goes: the next write rotates (the old log is kept as .1) and goes on.
    unblock_rotation(&dir);
    log.append("after the obstacle").unwrap();
    log.append("and one more").unwrap();
    let previous = read(&dir.join(ROTATED_FILE_NAME));
    assert_eq!(previous, kept, "the whole current log became .1");
    let now = read(&current);
    assert!(
        now.contains("after the obstacle") && now.contains("and one more"),
        "{now}"
    );
    assert!(!now.contains("line 0000"));
}

#[test]
fn a_sink_whose_rotation_fails_keeps_the_newest_lines_and_keeps_using_the_file() {
    struct Stream(Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Stream {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let temp = Temp::new("sinkfail");
    let dir = log_directory(&temp.0, Some("sf")).unwrap();
    let seen = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let sink = LogSink::new(Stream(seen.clone()))
        .with_file(LogFile::open_in(&temp.0, Some("sf"), 500).unwrap());
    block_rotation(&dir);
    for i in 0..100 {
        sink.line(&format!("while blocked {i:03}"));
    }
    unblock_rotation(&dir);
    sink.line("recovered");
    sink.line("and still logging");

    let text = String::from_utf8_lossy(&seen.lock().unwrap()).into_owned();
    assert!(
        !text.contains("warning: writing the log file failed"),
        "a restart is not a failed write: {text}"
    );
    assert_eq!(
        text.matches("while blocked").count(),
        100,
        "the stream lost nothing"
    );
    let all = format!(
        "{}{}",
        read(&dir.join(ROTATED_FILE_NAME)),
        read(&dir.join(LOG_FILE_NAME))
    );
    assert!(
        all.contains("while blocked 099"),
        "the newest lines are kept"
    );
    assert!(
        !all.contains("while blocked 000"),
        "the oldest were discarded"
    );
    assert!(
        all.contains("log restarted: ") && all.contains("rotation failing"),
        "the restart is marked: {all}"
    );
    assert!(
        all.contains("recovered") && all.contains("and still logging"),
        "{all}"
    );
}

/// Makes `path` impossible to open for writing (not a directory in the way: the file itself),
/// and back.
fn block_writes(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o400)).unwrap();
    }
    #[cfg(not(unix))]
    {
        let mut p = std::fs::metadata(path).unwrap().permissions();
        p.set_readonly(true);
        std::fs::set_permissions(path, p).unwrap();
    }
}

fn allow_writes(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    #[cfg(not(unix))]
    {
        let mut p = std::fs::metadata(path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        p.set_readonly(false);
        std::fs::set_permissions(path, p).unwrap();
    }
}

#[test]
fn a_restart_whose_open_fails_does_not_stall_the_log() {
    let temp = Temp::new("restartfail");
    let limit = 1000;
    let cap = limit * logfile::HARD_CAP_FACTOR;
    let mut log = LogFile::open_in(&temp.0, Some("rsf"), limit).unwrap();
    let dir = log_directory(&temp.0, Some("rsf")).unwrap();
    let current = dir.join(LOG_FILE_NAME);
    block_rotation(&dir);
    // Fill to just under the cap; the rotation fails each time past the limit.
    while std::fs::metadata(&current).map_or(0, |m| m.len()) <= cap - 40 {
        log.append("filler").unwrap();
    }
    // The next line crosses the cap, so the file is to restart; its truncating open fails.
    block_writes(&current);
    let before = read(&current);
    assert!(
        log.append(&"t".repeat(100)).is_err(),
        "the restart's open fails"
    );
    assert_eq!(read(&current), before, "nothing was lost");

    // The obstacle goes (the rotation is still blocked): the very next line is logged.
    allow_writes(&current);
    log.append("the next line").unwrap();
    assert!(read(&current).ends_with("the next line\n"));
    log.append("and another").unwrap();
    assert!(read(&current).ends_with("and another\n"));
}
