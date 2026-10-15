// SPDX-License-Identifier: Apache-2.0

//! The `oac hook` handler against `spec/bindings/mcp.md` 0.3 §8.1, [MCPB-CLD-004] to
//! [MCPB-CLD-010] (G9, #70). Written before the handler.
//!
//! Each test names the requirement it proves. The line the handler sends is read back with
//! the Claude adapter's own recognition rule (`oac_adapter_claude::hook::classify`), the code
//! that decides on the server side whether a connection is a carrier, so the two cannot drift.

use std::cell::RefCell;
use std::io::{self, Cursor, Read, Write};
use std::rc::Rc;

use oac_adapter_claude::hook::{FirstLine, MAX_HOOK_INPUT, SESSION_START, classify};
use oac_cli::hook::{self, Outcome, Skip, carrier_line};
use oac_cli::verbs::{EXIT_USAGE, Streams, dispatch};
use oac_core::adapter::StartKind;

/// A `SessionStart` input written over several lines, as the hooks reference writes its
/// examples. Claude Code `2.1.285` wrote the same fields on one line (captured 2026-10-12,
/// `adapters/claude/src/hook.rs` `STARTUP`); the reference does not promise that.
const PRETTY: &str = "{\n  \"session_id\": \"f40e86db-b0cb-439a-bb2d-17f1dd86c1d5\",\n  \"transcript_path\": \"C:\\\\Users\\\\u\\\\.claude\\\\projects\\\\p\\\\f40e86db-b0cb-439a-bb2d-17f1dd86c1d5.jsonl\",\n  \"cwd\": \"C:\\\\work\",\n  \"hook_event_name\": \"SessionStart\",\n  \"source\": \"startup\"\n}\n";

const ONE_LINE: &str = "{\"session_id\":\"f40e86db-b0cb-439a-bb2d-17f1dd86c1d5\",\"transcript_path\":\"C:\\\\Users\\\\u\\\\.claude\\\\projects\\\\p\\\\f40e86db-b0cb-439a-bb2d-17f1dd86c1d5.jsonl\",\"cwd\":\"C:\\\\work\",\"hook_event_name\":\"SessionStart\",\"source\":\"startup\"}";

/// What a test carrier saw: every `write` call, and whether it was flushed.
#[derive(Default)]
struct Seen {
    writes: Vec<Vec<u8>>,
    flushed: bool,
}

struct Carrier(Rc<RefCell<Seen>>);

impl Write for Carrier {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().writes.push(buf.to_vec());
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.borrow_mut().flushed = true;
        Ok(())
    }
}

/// A carrier whose write fails.
struct Broken;

impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Run {
    outcome: Outcome,
    seen: Rc<RefCell<Seen>>,
    connects: Vec<String>,
}

fn run_with(input: &mut dyn Read) -> Run {
    let seen = Rc::new(RefCell::new(Seen::default()));
    let mut connects = Vec::new();
    let outcome = {
        let seen = seen.clone();
        let mut connect = |harness: &str| -> io::Result<Box<dyn Write>> {
            connects.push(harness.to_owned());
            Ok(Box::new(Carrier(seen.clone())))
        };
        hook::run(input, &mut connect)
    };
    Run {
        outcome,
        seen,
        connects,
    }
}

fn run_bytes(input: &[u8]) -> Run {
    run_with(&mut Cursor::new(input.to_vec()))
}

/// The one line the handler sent, asserting the form [MCPB-CLD-005] requires.
fn sent_line(run: &Run) -> Vec<u8> {
    assert!(matches!(run.outcome, Outcome::Sent), "{:?}", run.outcome);
    let seen = run.seen.borrow();
    assert!(seen.flushed, "the line is flushed");
    assert_eq!(seen.writes.len(), 1, "the line goes out in one write");
    let line = seen.writes[0].clone();
    assert_eq!(line.last(), Some(&b'\n'), "the line ends with a newline");
    assert!(
        !line[..line.len() - 1]
            .iter()
            .any(|b| matches!(b, b'\n' | b'\r')),
        "no line feed or carriage return before the line's end"
    );
    line
}

/// The signal the adapter would read from `line`: its native id and start kind.
fn signal_of(line: &[u8]) -> (String, Option<StartKind>) {
    match classify(line) {
        FirstLine::Hook(h) => {
            let s = h.session_start().expect("a SessionStart signal");
            (s.native_id, s.start_kind)
        }
        FirstLine::Mcp => panic!("the server would serve this as MCP"),
    }
}

// [MCPB-CLD-004], [MCPB-CLD-005]

#[test]
fn a_multi_line_input_is_sent_as_one_line_the_server_takes_for_a_hook_input() {
    let run = run_bytes(PRETTY.as_bytes());
    let line = sent_line(&run);
    assert_eq!(
        run.connects,
        ["claude"],
        "one connection, to the Claude endpoint"
    );
    assert_eq!(
        signal_of(&line),
        (
            "f40e86db-b0cb-439a-bb2d-17f1dd86c1d5".to_owned(),
            Some(StartKind::Fresh)
        )
    );
}

#[test]
fn carriage_returns_and_blank_lines_in_the_input_do_not_reach_the_line() {
    let crlf = PRETTY.replace('\n', "\r\n");
    let spaced = format!("\r\n\r\n  {crlf}\r\n\r\n");
    let line = sent_line(&run_bytes(spaced.as_bytes()));
    assert_eq!(signal_of(&line).0, "f40e86db-b0cb-439a-bb2d-17f1dd86c1d5");
}

#[test]
fn an_input_without_a_final_newline_is_sent_the_same() {
    let a = sent_line(&run_bytes(PRETTY.trim_end().as_bytes()));
    let b = sent_line(&run_bytes(ONE_LINE.as_bytes()));
    assert_eq!(a, b);
}

#[test]
fn an_escaped_line_break_inside_a_string_stays_escaped() {
    let input = "{\"session_id\":\"s1\",\"hook_event_name\":\"SessionStart\",\"cwd\":\"a\\nb\\rc\\u2028d\"}";
    let line = sent_line(&run_bytes(input.as_bytes()));
    let text = String::from_utf8(line).unwrap();
    assert!(text.contains("a\\nb\\rc"), "{text}");
    assert_eq!(signal_of(text.as_bytes()).0, "s1");
}

#[test]
fn non_ascii_text_survives_and_the_line_stays_valid_utf8() {
    let input = "{\"session_id\":\"s1\",\"hook_event_name\":\"SessionStart\",\"cwd\":\"C:\\\\tr\u{e4}ume\\\\\u{1F600}\"}";
    let line = sent_line(&run_bytes(input.as_bytes()));
    let text = String::from_utf8(line).expect("UTF-8");
    assert!(text.contains("tr\u{e4}ume"));
}

// [MCPB-CLD-006]

#[test]
fn the_bytes_of_the_input_are_never_copied_through() {
    // Unusual spacing, a numeric escape and a different member order than the compact form:
    // the line is the parsed object written again, not the input.
    let input = "{ \"hook_event_name\" :\t\"SessionStart\",\n\"session_id\":\"\\u0073\\u0031\" ,\"source\":\"resume\" }";
    let line = sent_line(&run_bytes(input.as_bytes()));
    assert_ne!(line, input.as_bytes());
    assert!(
        !line.windows(2).any(|w| w == b"\\u"),
        "escapes are written out"
    );
    assert_eq!(
        String::from_utf8(line.clone()).unwrap(),
        "{\"hook_event_name\":\"SessionStart\",\"session_id\":\"s1\",\"source\":\"resume\"}\n"
    );
    assert_eq!(
        signal_of(&line),
        ("s1".to_owned(), Some(StartKind::Transition))
    );
}

#[test]
fn members_the_adapter_does_not_read_are_carried_in_order() {
    let line = sent_line(&run_bytes(PRETTY.as_bytes()));
    let text = String::from_utf8(line).unwrap();
    assert_eq!(text, format!("{ONE_LINE}\n"));
}

// [MCPB-CLD-004]: standard input is a JSON object

#[test]
fn input_that_is_not_a_json_object_sends_nothing_and_opens_no_connection() {
    let cases: &[(&str, &[u8])] = &[
        ("empty", b""),
        ("whitespace", b" \r\n\t"),
        ("not json", b"not json\n"),
        ("array", b"[1,2]"),
        ("string", b"\"SessionStart\""),
        ("truncated", b"{\"session_id\":\"s1\",\"hook_event_name\":"),
        (
            "trailing text",
            b"{\"session_id\":\"s1\",\"hook_event_name\":\"SessionStart\"} x",
        ),
        (
            "repeated member",
            b"{\"session_id\":\"a\",\"session_id\":\"b\",\"hook_event_name\":\"SessionStart\"}",
        ),
        (
            "not UTF-8",
            b"{\"session_id\":\"\xff\",\"hook_event_name\":\"SessionStart\"}",
        ),
    ];
    for (name, input) in cases {
        let run = run_bytes(input);
        assert!(
            matches!(run.outcome, Outcome::Skipped(Skip::NotAnObject)),
            "{name}: {:?}",
            run.outcome
        );
        assert!(run.connects.is_empty(), "{name}: no connection");
        assert!(run.seen.borrow().writes.is_empty(), "{name}");
    }
}

// [MCPB-CLD-008], [MCPB-CLD-009], [MCPB-CLD-010]: only SessionStart is a signal

#[test]
fn only_a_session_start_input_is_sent() {
    assert_eq!(
        SESSION_START, "SessionStart",
        "the event the server accepts"
    );
    for event in [
        "SessionEnd",
        "UserPromptSubmit",
        "PreToolUse",
        "Stop",
        "sessionstart",
    ] {
        let input = format!(
            "{{\"session_id\":\"s1\",\"hook_event_name\":\"{event}\",\"reason\":\"other\"}}"
        );
        let run = run_bytes(input.as_bytes());
        assert!(
            matches!(run.outcome, Outcome::Skipped(Skip::NotASignal)),
            "{event}: {:?}",
            run.outcome
        );
        assert!(
            run.connects.is_empty(),
            "{event}: a hook without a signal opens nothing"
        );
    }
}

#[test]
fn a_session_start_without_a_usable_session_id_is_not_sent() {
    for input in [
        "{\"hook_event_name\":\"SessionStart\",\"source\":\"startup\"}",
        "{\"session_id\":\"\",\"hook_event_name\":\"SessionStart\"}",
        "{\"session_id\":12,\"hook_event_name\":\"SessionStart\"}",
        "{\"session_id\":null,\"hook_event_name\":\"SessionStart\"}",
    ] {
        let run = run_bytes(input.as_bytes());
        assert!(
            matches!(run.outcome, Outcome::Skipped(Skip::NotASignal)),
            "{input}"
        );
        assert!(run.connects.is_empty());
    }
}

#[test]
fn an_object_that_is_not_a_hook_input_is_not_sent() {
    for input in [
        "{\"session_id\":\"s1\"}",
        "{\"hook_event_name\":5,\"session_id\":\"s1\"}",
        // Claims to be JSON-RPC: the server would serve it as an MCP frame.
        "{\"jsonrpc\":\"2.0\",\"hook_event_name\":\"SessionStart\",\"session_id\":\"s1\"}",
        "{}",
    ] {
        let run = run_bytes(input.as_bytes());
        assert!(
            matches!(
                run.outcome,
                Outcome::Skipped(Skip::NotAHookInput | Skip::NotASignal)
            ),
            "{input}: {:?}",
            run.outcome
        );
        assert!(run.connects.is_empty());
    }
}

#[test]
fn every_documented_start_source_is_carried_for_the_server_to_map() {
    for (source, kind) in [
        ("startup", StartKind::Fresh),
        ("resume", StartKind::Transition),
        ("clear", StartKind::Transition),
        ("fork", StartKind::Transition),
        ("compact", StartKind::Transition),
        ("teleport", StartKind::Unmapped),
    ] {
        let input = format!(
            "{{\"session_id\":\"s1\",\"hook_event_name\":\"SessionStart\",\"source\":\"{source}\"}}"
        );
        let line = sent_line(&run_bytes(input.as_bytes()));
        assert_eq!(signal_of(&line), ("s1".to_owned(), Some(kind)), "{source}");
    }
}

// Bounds

/// An input that never ends.
struct Endless;

impl Read for Endless {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        buf.fill(b' ');
        Ok(buf.len())
    }
}

#[test]
fn an_input_that_never_ends_is_cut_off() {
    let run = run_with(&mut Endless);
    assert!(
        matches!(run.outcome, Outcome::Skipped(Skip::TooLarge)),
        "{:?}",
        run.outcome
    );
    assert!(run.connects.is_empty());
}

#[test]
fn the_clients_line_limit_is_the_conservative_one_and_a_longer_line_is_not_sent() {
    let with = |n: usize| {
        format!(
            "{{\"session_id\":\"s1\",\"hook_event_name\":\"SessionStart\",\"p\":\"{}\"}}",
            "x".repeat(n)
        )
    };
    // The limit here is the conservative one: the server still reads a longer line that
    // arrives with its newline in one chunk (`adapters/claude/src/io.rs` checks for the
    // newline before the length), but not one that arrives in pieces, so the client sends
    // nothing past `MAX_HOOK_INPUT` rather than rely on chunking.
    let base = with(0).len();
    // The compact line and its newline fit the limit exactly: sent.
    let fits = with(MAX_HOOK_INPUT - 1 - base);
    assert_eq!(carrier_line(fits.as_bytes()).unwrap().len(), MAX_HOOK_INPUT);
    assert!(matches!(run_bytes(fits.as_bytes()).outcome, Outcome::Sent));
    // One byte more: not sent, and no connection is opened.
    let over = with(MAX_HOOK_INPUT - base);
    assert_eq!(carrier_line(over.as_bytes()).unwrap_err(), Skip::TooLarge);
    let run = run_bytes(over.as_bytes());
    assert!(
        matches!(run.outcome, Outcome::Skipped(Skip::TooLarge)),
        "{:?}",
        run.outcome
    );
    assert!(run.connects.is_empty());
    // An input beyond the limit is cut off while being read.
    let run = run_bytes(with(MAX_HOOK_INPUT).as_bytes());
    assert!(matches!(run.outcome, Outcome::Skipped(Skip::TooLarge)));
}

#[test]
fn a_read_error_sends_nothing() {
    struct Failing;
    impl Read for Failing {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("closed"))
        }
    }
    let run = run_with(&mut Failing);
    assert!(matches!(run.outcome, Outcome::Skipped(Skip::Unreadable)));
    assert!(run.connects.is_empty());
}

// The signal is lost, the session is not disturbed

#[test]
fn a_connector_that_fails_reports_the_signal_lost() {
    let mut input = Cursor::new(PRETTY.as_bytes().to_vec());
    let mut connect = |_: &str| -> io::Result<Box<dyn Write>> {
        Err(io::Error::new(io::ErrorKind::NotFound, "no daemon"))
    };
    let outcome = hook::run(&mut input, &mut connect);
    assert!(matches!(outcome, Outcome::NotSent(_)));
    let line = outcome.diagnostic().expect("a diagnostic");
    assert!(line.contains("no daemon"));
}

#[test]
fn a_carrier_that_breaks_reports_the_signal_lost() {
    let mut input = Cursor::new(PRETTY.as_bytes().to_vec());
    let mut connect = |_: &str| -> io::Result<Box<dyn Write>> { Ok(Box::new(Broken)) };
    assert!(matches!(
        hook::run(&mut input, &mut connect),
        Outcome::NotSent(_)
    ));
}

#[test]
fn a_diagnostic_never_holds_the_input() {
    // The session id and the paths are the user's; the diagnostic stream may be logged.
    for input in [
        PRETTY,
        "{\"session_id\":\"SECRET-ID\",\"cwd\":\"C:\\\\secret\"} trailing",
    ] {
        let mut stdin = Cursor::new(input.as_bytes().to_vec());
        let mut connect = |_: &str| -> io::Result<Box<dyn Write>> {
            Err(io::Error::new(io::ErrorKind::NotFound, "no daemon"))
        };
        let outcome = hook::run(&mut stdin, &mut connect);
        let text = outcome.diagnostic().unwrap_or_default();
        assert!(!text.contains("SECRET") && !text.contains("f40e86db") && !text.contains("secret"));
    }
}

// The verb: nothing on standard output, always exit 0 ([MCPB-CLD-007])

struct Dispatched {
    status: i32,
    stdout: Vec<u8>,
    stderr: String,
    seen: Rc<RefCell<Seen>>,
}

fn dispatch_args(args: &[&str], stdin: &[u8], connects: bool) -> Dispatched {
    let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
    let seen = Rc::new(RefCell::new(Seen::default()));
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut input = Cursor::new(stdin.to_vec());
    let status = {
        let seen = seen.clone();
        let mut connect = move |_: &str| -> io::Result<Box<dyn Write>> {
            if connects {
                Ok(Box::new(Carrier(seen.clone())))
            } else {
                Err(io::Error::new(io::ErrorKind::NotFound, "no daemon"))
            }
        };
        dispatch(
            &args,
            Streams {
                stdin: &mut input,
                stdout: &mut stdout,
                stderr: &mut stderr,
            },
            &mut connect,
        )
    };
    Dispatched {
        status,
        stdout,
        stderr: String::from_utf8(stderr).unwrap(),
        seen,
    }
}

#[test]
fn the_hook_verb_writes_nothing_to_standard_output_and_exits_zero() {
    let cases: &[(&str, &[u8], bool)] = &[
        ("sent", PRETTY.as_bytes(), true),
        ("daemon down", PRETTY.as_bytes(), false),
        (
            "not a signal",
            b"{\"hook_event_name\":\"SessionEnd\",\"session_id\":\"s\"}",
            true,
        ),
        ("garbage", b"\x00\x01\x02", true),
        ("empty", b"", true),
    ];
    for (name, stdin, connects) in cases {
        let d = dispatch_args(&["hook"], stdin, *connects);
        assert_eq!(d.status, 0, "{name}");
        assert!(d.stdout.is_empty(), "{name}: standard output stays empty");
    }
    let sent = dispatch_args(&["hook"], PRETTY.as_bytes(), true);
    assert_eq!(sent.seen.borrow().writes.len(), 1);
    assert!(
        sent.stderr.is_empty(),
        "nothing to say when the signal went out"
    );
    let down = dispatch_args(&["hook"], PRETTY.as_bytes(), false);
    assert!(down.stderr.starts_with("oac hook:"), "{}", down.stderr);
}

#[test]
fn the_hook_verb_ignores_arguments_rather_than_failing_the_hook() {
    let d = dispatch_args(&["hook", "--unexpected"], PRETTY.as_bytes(), true);
    assert_eq!(d.status, 0);
    assert!(d.stdout.is_empty());
    assert_eq!(d.seen.borrow().writes.len(), 1);
    assert!(d.stderr.contains("arguments are ignored"));
}

#[test]
fn a_missing_or_unknown_verb_is_a_usage_error() {
    for args in [
        &[][..],
        &["start"][..],
        &["mcp-shim"][..],
        &["nonsense"][..],
    ] {
        let d = dispatch_args(args, b"", true);
        assert_eq!(d.status, EXIT_USAGE, "{args:?}");
        assert!(d.stdout.is_empty());
        assert!(!d.stderr.is_empty());
    }
}

// Key material lives in the daemon only (G9 box 2): the handler, the shim and the relay never
// touch it. A static check of the sources is context for the behavioural test in
// `tests/daemon.rs` (the shim and the hook run with no key store and a poisoned passphrase
// variable), not proof of it: a word list cannot see a key reached under another name.

fn code_of(file: &str) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let text = std::fs::read_to_string(root.join(file)).unwrap();
    // Strip comments: the module documentation names what the code does not do.
    text.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}

#[test]
fn the_handler_names_no_key_store_credential_or_environment() {
    for file in ["hook.rs", "verbs.rs"] {
        let code = code_of(file);
        for word in [
            "keystore",
            "keyring",
            "oac_core::keys",
            "DeviceKey",
            "PASSPHRASE",
            "std::env",
            "var_os",
        ] {
            assert!(!code.contains(word), "{file} names {word}");
        }
    }
}

#[test]
fn the_shim_and_the_relay_name_no_key_store_or_credential() {
    for file in ["shim.rs", "relay.rs"] {
        let code = code_of(file);
        for word in [
            "keystore",
            "keyring",
            "oac_core::keys",
            "DeviceKey",
            "KeyStore",
            "PASSPHRASE",
            "SecretSeed",
        ] {
            assert!(!code.contains(word), "{file} names {word}");
        }
    }
}
