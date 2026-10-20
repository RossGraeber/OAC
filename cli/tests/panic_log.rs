// SPDX-License-Identifier: Apache-2.0

//! A panic leaves a line in the daemon's log file (G10, #71; lead decision 2026-10-18, Q2: logs
//! survive a crash). A detached daemon's standard error is a closed pipe, so the file is the
//! only place the panic can be seen. The line holds the source location and the word
//! "panicked", never the panic's message, which could carry what the log must not hold. The
//! hook is process-wide, so this test has a binary of its own.

use oac_cli::daemon::LogSink;
use oac_cli::logfile::{LOG_FILE_NAME, LogFile, MAX_LOG_BYTES, log_directory};

#[test]
fn a_panic_leaves_its_place_and_not_its_message_in_the_log_file() {
    let base = std::env::temp_dir().join(format!("oac-panic-log-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let file = LogFile::open_in(&base, Some("pl"), MAX_LOG_BYTES).unwrap();
    let sink = LogSink::new(std::io::sink()).with_file(file);
    sink.install_panic_hook();

    let result = std::panic::catch_unwind(|| {
        panic!("secret-payload-must-not-be-logged {}", "native-session-id");
    });
    assert!(result.is_err());

    let log = log_directory(&base, Some("pl"))
        .unwrap()
        .join(LOG_FILE_NAME);
    let kept = std::fs::read_to_string(&log).unwrap();
    let _ = std::fs::remove_dir_all(&base);
    assert!(kept.contains("oac-daemon: panicked at"), "{kept}");
    assert!(kept.contains("panic_log.rs:"), "{kept}");
    assert!(!kept.contains("secret-payload"), "{kept}");
    assert!(!kept.contains("native-session-id"), "{kept}");
}
