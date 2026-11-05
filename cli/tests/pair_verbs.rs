// SPDX-License-Identifier: Apache-2.0

//! `oac pair`, `oac pair --respond`, `oac pair --list` and `oac unpair` (#437, slice S4;
//! `docs/planning/decisions/G-437-pairing.md` §6 and §10).
//!
//! Each verb runs through its `_with` function with in-memory streams. Two kinds of test:
//!
//! - **Scripted**: the verb runs over a [`Wire`] that plays back chosen daemon replies and
//!   records the lines the verb sent. It proves every word the daemon can say (every refusal,
//!   every end of a pairing) is mapped to an operator message and an exit status, and that
//!   what the verb must not send is not sent.
//! - **Real daemons**: in-process daemons over the in-memory transport, each in its own IPC
//!   namespace with in-memory stores, and the verbs on threads of their own with channel
//!   streams. The test is the operator: it carries each blob and each code from one verb's
//!   screen to the other's keyboard. Every wait is on a channel with a bound, never a sleep
//!   or a poll.
//!
//! Real credential stores and data directories are never touched.

use std::collections::VecDeque;
use std::io::{self, BufRead, Cursor, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use oac_cli::control::canonical_text;
use oac_cli::daemon::{Daemon, LogSink, Parts};
use oac_cli::device_pairing::{
    BLOB_PREFIX, Blob, decode_blob, encode_offer, encode_response, encode_reveal,
};
use oac_cli::ipc::Scope;
use oac_cli::pair_verbs::{
    PairCommand, PairRole, Recv, Terminal, WRAP, Wire, pair_over, pair_with, unpair_over,
    unpair_with,
};
use oac_core::clock::ManualClock;
use oac_core::ids::{KeyId, Timestamp, Token};
use oac_core::json::{self, Json, JsonObject};
use oac_core::keys::{DeviceIdentity, DeviceKey, MemoryKeyStore};
use oac_core::pairing::{
    MemoryPairingStore, PairingInitiator, PairingSnapshot, PairingStore, PairingStoreError,
    ResponderPairing,
};
use oac_core::pipeline::PipelineConfig;
use oac_core::sealing::MemoryAgreementKeyStore;
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};

/// How long a test waits for a verb or a daemon before it fails (real time, so a hung verb is
/// a failure and not a hang).
const WAIT: Duration = Duration::from_secs(30);

// ---- the streams of a verb --------------------------------------------------------------------

/// Standard input fed by the test, one chunk at a time; the end of input when the sender is
/// dropped (or nothing arrives within [`WAIT`]).
struct ChanReader {
    rx: mpsc::Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for ChanReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = {
            let b = self.fill_buf()?;
            let n = b.len().min(out.len());
            out[..n].copy_from_slice(&b[..n]);
            n
        };
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for ChanReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.pos >= self.buf.len() {
            self.buf = self.rx.recv_timeout(WAIT).unwrap_or_default();
            self.pos = 0;
        }
        Ok(&self.buf[self.pos..])
    }
    fn consume(&mut self, n: usize) {
        self.pos += n;
    }
}

/// Standard output or error: every write goes to the test.
struct ChanWriter(mpsc::Sender<Vec<u8>>);

impl Write for ChanWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _ = self.0.send(buf.to_vec());
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// What a verb wrote, read by the test as it arrives.
struct Screen {
    rx: mpsc::Receiver<Vec<u8>>,
    pending: String,
}

impl Screen {
    fn fill(&mut self) {
        let chunk = self
            .rx
            .recv_timeout(WAIT)
            .expect("the verb wrote what the test waits for");
        self.pending.push_str(&String::from_utf8_lossy(&chunk));
    }
    /// The next line, without its newline.
    fn line(&mut self) -> String {
        loop {
            if let Some(i) = self.pending.find('\n') {
                let line = self.pending[..i].trim_end_matches('\r').to_owned();
                self.pending.drain(..=i);
                return line;
            }
            self.fill();
        }
    }
    /// Everything up to and including the next `marker`.
    fn until(&mut self, marker: &str) -> String {
        loop {
            if let Some(i) = self.pending.find(marker) {
                return self.pending.drain(..i + marker.len()).collect();
            }
            self.fill();
        }
    }
    /// The next line as a canonical `oac_cli` object.
    fn json(&mut self) -> Obj {
        let line = self.line();
        assert_eq!(canonical_text(&line), line, "a canonical line");
        let object = json::parse(line.as_bytes())
            .unwrap_or_else(|e| panic!("{line:?}: {e}"))
            .into_object()
            .expect("an object");
        assert_eq!(
            object
                .get("oac_cli")
                .and_then(Json::as_number)
                .map(|n| n.raw().to_owned()),
            Some("1".to_owned()),
            "{line}"
        );
        Obj { object, line }
    }
    /// Everything written and not yet read; call after the verb has ended.
    fn rest(&mut self) -> String {
        while let Ok(chunk) = self.rx.try_recv() {
            self.pending.push_str(&String::from_utf8_lossy(&chunk));
        }
        std::mem::take(&mut self.pending)
    }
}

struct Obj {
    object: JsonObject,
    line: String,
}

impl Obj {
    fn str(&self, name: &str) -> String {
        self.object
            .get(name)
            .and_then(Json::as_str)
            .unwrap_or_else(|| panic!("no string {name:?} in {}", self.line))
            .to_owned()
    }
    fn flag(&self, name: &str) -> bool {
        self.object
            .get(name)
            .and_then(Json::as_bool)
            .unwrap_or_else(|| panic!("no boolean {name:?} in {}", self.line))
    }
    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.object.names().map(str::to_owned).collect();
        names.sort();
        names
    }
    fn step(&self) -> String {
        self.str("step")
    }
    fn child(&self, name: &str) -> Obj {
        let object = self
            .object
            .get(name)
            .and_then(Json::as_object)
            .unwrap_or_else(|| panic!("no object {name:?} in {}", self.line))
            .clone();
        Obj {
            line: self.line.clone(),
            object,
        }
    }
}

/// A verb running on a thread of its own, with channel streams.
struct Verb {
    input: Option<mpsc::Sender<Vec<u8>>>,
    out: Screen,
    err: Screen,
    handle: Option<std::thread::JoinHandle<i32>>,
}

impl Verb {
    fn spawn(run: impl FnOnce(&mut Terminal<'_>) -> i32 + Send + 'static) -> Verb {
        let (in_tx, in_rx) = mpsc::channel();
        let (out_tx, out_rx) = mpsc::channel();
        let (err_tx, err_rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let mut stdin = ChanReader {
                rx: in_rx,
                buf: Vec::new(),
                pos: 0,
            };
            let (mut stdout, mut stderr) = (ChanWriter(out_tx), ChanWriter(err_tx));
            run(&mut Terminal {
                stdin: &mut stdin,
                stdout: &mut stdout,
                stderr: &mut stderr,
            })
        });
        Verb {
            input: Some(in_tx),
            out: Screen {
                rx: out_rx,
                pending: String::new(),
            },
            err: Screen {
                rx: err_rx,
                pending: String::new(),
            },
            handle: Some(handle),
        }
    }
    fn pair(scope: &Scope, command: PairCommand, json: bool) -> Verb {
        let scope = scope.clone();
        Verb::spawn(move |t| pair_with(&scope, command, json, WAIT, t))
    }
    fn unpair(scope: &Scope, key: &KeyId, yes: bool, json: bool) -> Verb {
        let (scope, key) = (scope.clone(), key.as_str().to_owned());
        Verb::spawn(move |t| unpair_with(&scope, &key, yes, json, false, WAIT, t))
    }
    fn send(&self, text: &str) {
        self.input
            .as_ref()
            .expect("input is open")
            .send(text.as_bytes().to_vec())
            .expect("the verb reads its input");
    }
    /// A one-line blob and the empty line that ends it.
    fn paste(&self, blob: &str) {
        self.send(&format!("{blob}\n\n"));
    }
    fn type_line(&self, text: &str) {
        self.send(&format!("{text}\n"));
    }
    /// Closes the input, waits for the verb to end, and returns its exit status and what it
    /// wrote that the test has not read.
    fn end(mut self) -> (i32, String, String) {
        self.input = None;
        let status = self
            .handle
            .take()
            .unwrap()
            .join()
            .expect("the verb did not panic");
        (status, self.out.rest(), self.err.rest())
    }
}

// ---- the daemons ------------------------------------------------------------------------------

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A pairing store the test can make fail.
#[derive(Clone)]
struct FailingStore {
    inner: Arc<MemoryPairingStore>,
    fail: Arc<AtomicBool>,
}

impl PairingStore for FailingStore {
    fn load(&self) -> Result<PairingSnapshot, PairingStoreError> {
        self.inner.load()
    }
    fn save(&self, snapshot: &PairingSnapshot) -> Result<(), PairingStoreError> {
        if self.fail.load(Ordering::SeqCst) {
            Err(PairingStoreError::Failed("the test refuses".into()))
        } else {
            self.inner.save(snapshot)
        }
    }
}

/// Removes the namespace directory a Unix test made (Windows: nothing is made).
struct NamespaceDir(#[cfg(unix)] Option<std::path::PathBuf>);

impl NamespaceDir {
    fn of(scope: &Scope) -> NamespaceDir {
        #[cfg(unix)]
        return NamespaceDir(
            std::path::Path::new(&scope.endpoint("claude").display_name())
                .parent()
                .map(std::path::Path::to_path_buf),
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

struct Node {
    daemon: Option<Daemon>,
    scope: Scope,
    fail: Arc<AtomicBool>,
    key_id: KeyId,
    _dir: NamespaceDir,
}

impl Node {
    fn daemon(&self) -> &Daemon {
        self.daemon.as_ref().unwrap()
    }
    fn trusts(&self, key: &KeyId) -> bool {
        self.daemon()
            .pipelines()
            .with_engine(|e| e.trusted_keys().get(key).is_some())
    }
    async fn shutdown(&mut self) {
        if let Some(d) = self.daemon.take() {
            d.shutdown().await;
        }
    }
}

async fn node(principal: &str) -> Node {
    let network = MemoryNetwork::new();
    let net = network.clone();
    let namespace = format!(
        "pv{}n{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    );
    let scope = Scope::for_user_in(Some(&namespace)).unwrap();
    let dir = NamespaceDir::of(&scope);
    let fail = Arc::new(AtomicBool::new(false));
    let store = FailingStore {
        inner: Arc::new(MemoryPairingStore::new()),
        fail: fail.clone(),
    };
    let daemon = Daemon::start(Parts {
        scope: scope.clone(),
        key_store: Box::new(MemoryKeyStore::new()),
        pairing_store: Box::new(store),
        agreement_store: Box::new(MemoryAgreementKeyStore::new()),
        principal: Token::parse(principal).unwrap(),
        transport: Arc::new(MemoryTransport::new()),
        transport_configuration: MemoryConfiguration::wrap(&network),
        clock: Arc::new(ManualClock::new(
            Timestamp::parse("2026-11-05T12:00:00.000Z").unwrap(),
        )),
        monotonic: Arc::new(move || net.now()),
        config: PipelineConfig::default(),
        harnesses: Vec::new(),
        log: LogSink::new(std::io::sink()),
        codex: None,
    })
    .await
    .unwrap_or_else(|e| panic!("the daemon starts: {e}"));
    let key_id = daemon.pipelines().device().key_id().clone();
    Node {
        daemon: Some(daemon),
        scope,
        fail,
        key_id,
        _dir: dir,
    }
}

// ---- the operator's side of a pairing ----------------------------------------------------------

/// Both verbs at their codes, in `--json` mode; the test is the operator.
struct Pairing {
    a: Verb,
    b: Verb,
    waiting: Obj,
    offer: Obj,
    response: Obj,
    code_a: Obj,
    code_b: Obj,
}

fn begin(a: &Node, b: &Node) -> Pairing {
    let mut vb = Verb::pair(&b.scope, PairCommand::Pair(PairRole::Responder), true);
    let waiting = vb.out.json();
    assert_eq!(waiting.step(), "waiting");
    let mut va = Verb::pair(&a.scope, PairCommand::Pair(PairRole::Initiator), true);
    let offer = va.out.json();
    assert_eq!(offer.step(), "offer");
    vb.paste(&offer.str("blob"));
    let response = vb.out.json();
    assert_eq!(response.step(), "response");
    va.paste(&response.str("blob"));
    let code_a = va.out.json();
    assert_eq!(code_a.step(), "code");
    vb.paste(&code_a.str("blob"));
    let code_b = vb.out.json();
    assert_eq!(code_b.step(), "code");
    Pairing {
        a: va,
        b: vb,
        waiting,
        offer,
        response,
        code_a,
        code_b,
    }
}

/// A full honest pairing through the verbs; both end 0.
fn pair_fully(a: &Node, b: &Node) {
    let p = begin(a, b);
    let (code_a, code_b) = (p.code_a.str("code"), p.code_b.str("code"));
    let (mut va, mut vb) = (p.a, p.b);
    va.type_line(&code_b);
    let result = va.out.json();
    assert!(result.flag("paired"), "{}", result.line);
    vb.type_line(&code_a);
    let result = vb.out.json();
    assert!(result.flag("paired"), "{}", result.line);
    assert_eq!(va.end().0, 0);
    assert_eq!(vb.end().0, 0);
}

/// A code that is not `code`.
fn other_than(code: &str) -> String {
    let first = if code.starts_with('9') { '1' } else { '9' };
    format!("{first}{}", &code[1..])
}

fn at(ms: i64) -> Timestamp {
    Timestamp::from_unix_millis(1_790_000_000_000 + ms).unwrap()
}

/// A valid offer, response and reveal blob of one exchange between two made-up devices.
fn blobs() -> (String, String, String) {
    let a = DeviceIdentity::new(DeviceKey::generate(), Token::parse("alpha").unwrap());
    let b = DeviceIdentity::new(DeviceKey::generate(), Token::parse("beta").unwrap());
    let (init, offer) = PairingInitiator::start(&a, at(0));
    let offer_blob = encode_offer(&offer);
    let mut responder = ResponderPairing::start(at(0));
    let response = responder.answer(&b, offer, &at(1)).unwrap();
    let response_blob = encode_response(&response);
    let (_, reveal) = init.receive(response, &at(2)).unwrap();
    (offer_blob, response_blob, encode_reveal(&reveal))
}

// ---- scripted wires -----------------------------------------------------------------------------

struct Script {
    replies: VecDeque<Recv>,
    sent: Vec<String>,
}

impl Script {
    fn new(replies: impl IntoIterator<Item = Recv>) -> Script {
        Script {
            replies: replies.into_iter().collect(),
            sent: Vec::new(),
        }
    }
    fn lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> Script {
        Script::new(lines.into_iter().map(|l| Recv::Line(l.to_owned())))
    }
    fn sent_ops(&self) -> Vec<String> {
        self.sent
            .iter()
            .map(|l| {
                let o = json::parse(l.trim_end().as_bytes())
                    .unwrap()
                    .into_object()
                    .unwrap();
                o.get("op").and_then(Json::as_str).unwrap().to_owned()
            })
            .collect()
    }
}

impl Wire for Script {
    fn send(&mut self, line: &str) -> io::Result<()> {
        self.sent.push(line.to_owned());
        Ok(())
    }
    fn recv(&mut self, _wait: Duration) -> Recv {
        self.replies.pop_front().unwrap_or(Recv::Closed)
    }
}

/// What a scripted verb did.
struct Done {
    status: i32,
    stdout: String,
    stderr: String,
}

impl Done {
    fn json(&self) -> Obj {
        let line = self
            .stdout
            .lines()
            .last()
            .expect("a result line")
            .to_owned();
        let object = json::parse(line.as_bytes())
            .unwrap_or_else(|e| panic!("{line:?}: {e}"))
            .into_object()
            .unwrap();
        Obj { object, line }
    }
}

fn run_pair(script: &mut Script, role: PairRole, json: bool, input: &str) -> Done {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut stdin = Cursor::new(input.as_bytes().to_vec());
    let status = pair_over(
        script,
        role,
        json,
        Duration::from_secs(1),
        &mut Terminal {
            stdin: &mut stdin,
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );
    Done {
        status,
        stdout: String::from_utf8(stdout).unwrap(),
        stderr: String::from_utf8(stderr).unwrap(),
    }
}

fn run_unpair(script: &mut Script, key: &str, json: bool) -> Done {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut stdin = Cursor::new(Vec::new());
    let status = unpair_over(
        script,
        key,
        json,
        Duration::from_secs(1),
        &mut Terminal {
            stdin: &mut stdin,
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );
    Done {
        status,
        stdout: String::from_utf8(stdout).unwrap(),
        stderr: String::from_utf8(stderr).unwrap(),
    }
}

const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn waiting_line() -> &'static str {
    "{\"expires_ms\":119000,\"oac_control\":1,\"pair\":\"waiting\"}"
}

fn code_line(already_trusted: bool) -> String {
    format!(
        "{{\"already_trusted\":{already_trusted},\"code\":\"123456\",\"oac_control\":1,\"own_statement\":{{\"state\":\"none\"}},\"pair\":\"code\",\"peer_key_id\":\"{KEY}\",\"peer_principal\":\"device\",\"peer_statement\":{{\"state\":\"none\"}}}}"
    )
}

/// A responder script up to the code, for the tests of the last step: the operator pastes an
/// offer and a reveal and types `codes`.
fn responder_script(then: impl IntoIterator<Item = &'static str>) -> (Script, String) {
    let (offer, _, reveal) = blobs();
    let mut lines = vec![
        waiting_line().to_owned(),
        "{\"blob\":\"oacpair1.x\",\"oac_control\":1,\"pair\":\"response\"}".to_owned(),
        code_line(false),
    ];
    lines.extend(then.into_iter().map(str::to_owned));
    (
        Script::new(lines.into_iter().map(Recv::Line)),
        format!("{offer}\n\n{reveal}\n\n"),
    )
}

// ---- the tests ----------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pairing_completes_through_the_verbs_and_both_exit_0() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    pair_fully(&a, &b);
    assert!(a.trusts(&b.key_id), "A trusts B");
    assert!(b.trusts(&a.key_id), "B trusts A");
    a.shutdown().await;
    b.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_json_steps_have_the_documented_members() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    let p = begin(&a, &b);
    let marker = ["oac_cli", "step"];
    let with = |extra: &[&str]| -> Vec<String> {
        let mut v: Vec<String> = marker
            .iter()
            .chain(extra)
            .map(|s| (*s).to_owned())
            .collect();
        v.sort();
        v
    };
    assert_eq!(p.waiting.names(), with(&["expires_ms"]));
    assert_eq!(p.offer.names(), with(&["blob", "expires_ms"]));
    assert_eq!(p.response.names(), with(&["blob"]));
    let code = [
        "already_trusted",
        "code",
        "own_statement",
        "peer_key_id",
        "peer_statement",
    ];
    assert_eq!(p.code_a.names(), with(&[&code[..], &["blob"]].concat()));
    assert_eq!(p.code_b.names(), with(&code));
    // Each side names the other device; the codes are six digits.
    assert_eq!(p.code_a.str("peer_key_id"), b.key_id.as_str());
    assert_eq!(p.code_b.str("peer_key_id"), a.key_id.as_str());
    for c in [&p.code_a, &p.code_b] {
        assert!(c.str("code").len() == 6 && c.str("code").bytes().all(|d| d.is_ascii_digit()));
        assert!(!c.flag("already_trusted"));
        for s in ["own_statement", "peer_statement"] {
            let s = c.child(s);
            match s.str("state").as_str() {
                "given" => assert_eq!(s.names(), ["agreement_key", "issued_at", "seq", "state"]),
                "none" | "refused" => assert_eq!(s.names(), ["state"]),
                other => panic!("a statement state {other}"),
            }
        }
    }
    // A wrong entry is a `mismatch` step; the final line is the result.
    let (code_a, code_b) = (p.code_a.str("code"), p.code_b.str("code"));
    let (mut va, mut vb) = (p.a, p.b);
    va.type_line(&other_than(&code_b));
    let mismatch = va.out.json();
    assert_eq!(mismatch.names(), with(&["attempts_left"]));
    assert_eq!(mismatch.str("step"), "mismatch");
    va.type_line(&code_b);
    let result = va.out.json();
    assert_eq!(
        result.names(),
        ["key_id", "oac_cli", "ok", "paired"],
        "{}",
        result.line
    );
    assert!(result.flag("ok") && result.flag("paired"));
    assert_eq!(result.str("key_id"), b.key_id.as_str());
    vb.type_line(&code_a);
    assert!(vb.out.json().flag("paired"));
    // `--json` prints no prompt and nothing else on standard error for a success.
    let (status, rest, err) = va.end();
    assert_eq!((status, rest.as_str()), (0, ""));
    assert!(
        err.contains("does not match"),
        "the mismatch is explained: {err}"
    );
    let (status, _, err) = vb.end();
    assert_eq!((status, err.as_str()), (0, ""));
    a.shutdown().await;
    b.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_blob_printed_by_the_verb_has_no_line_over_64_characters() {
    let mut a = node("alpha").await;
    let mut v = Verb::pair(&a.scope, PairCommand::Pair(PairRole::Initiator), false);
    // The offer: the lines from the blob's first to the empty line that ends it.
    v.out.until(BLOB_PREFIX);
    let mut lines = vec![BLOB_PREFIX.to_owned() + &v.out.line()];
    loop {
        let line = v.out.line();
        if line.is_empty() {
            break;
        }
        lines.push(line);
    }
    assert!(lines.len() > 1, "the blob is wrapped over several lines");
    assert!(
        lines.iter().all(|l| l.len() <= WRAP),
        "{:?}",
        lines.iter().map(String::len).collect::<Vec<_>>()
    );
    assert!(lines[..lines.len() - 1].iter().all(|l| l.len() == WRAP));
    let blob: String = lines.concat();
    assert!(matches!(decode_blob(&blob), Ok(Blob::Offer(_))));
    // Abandon the pairing: the input ends.
    let (status, _, err) = v.end();
    assert_eq!(status, 1, "{err}");
    assert!(err.contains("input ended"), "{err}");
    a.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_prompt_names_the_empty_line_that_ends_a_blob() {
    let mut b = node("beta").await;
    let mut v = Verb::pair(&b.scope, PairCommand::Pair(PairRole::Responder), false);
    let prompt = v.err.until("s left): ");
    assert!(prompt.contains("empty line"), "{prompt}");
    assert!(prompt.contains("offer blob"), "{prompt}");
    let (status, _, _) = v.end();
    assert_eq!(status, 1);
    b.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_blob_wrapped_over_several_lines_is_accepted() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    let mut va = Verb::pair(&a.scope, PairCommand::Pair(PairRole::Initiator), true);
    let offer = va.out.json().str("blob");
    let mut vb = Verb::pair(&b.scope, PairCommand::Pair(PairRole::Responder), true);
    assert_eq!(vb.out.json().step(), "waiting");
    // Wrapped at 23 characters, Windows line ends, a blank inside each, empty lines before and
    // one empty line to end.
    let wrapped: String = offer
        .as_bytes()
        .chunks(23)
        .map(|c| format!(" {}\r\n", std::str::from_utf8(c).unwrap()))
        .collect();
    vb.send(&format!("\n\n{wrapped}\r\n"));
    let response = vb.out.json();
    assert_eq!(response.step(), "response", "{}", response.line);
    // The same for the response, on the other side, in lines of 64 characters.
    let wrapped: String = response
        .str("blob")
        .as_bytes()
        .chunks(64)
        .map(|c| format!("{}\n", std::str::from_utf8(c).unwrap()))
        .collect();
    va.send(&format!("{wrapped}\n"));
    let code = va.out.json();
    assert_eq!(code.step(), "code", "{}", code.line);
    assert_eq!(va.end().0, 1);
    assert_eq!(vb.end().0, 1);
    a.shutdown().await;
    b.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_blob_kind_is_diagnosed_without_a_state_change() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    let (_, response, reveal) = blobs();
    // The responder is given a response, then a reveal, then a text that is no blob, then the
    // offer it asked for: it names each mistake, sends nothing to the daemon for them, and
    // goes on with the right one.
    let mut vb = Verb::pair(&b.scope, PairCommand::Pair(PairRole::Responder), false);
    vb.err.until("Waiting for an offer");
    vb.paste(&response);
    let first = vb.err.until("this step needs the offer blob");
    assert!(first.contains("that is a response blob"), "{first}");
    vb.paste(&reveal);
    let second = vb.err.until("this step needs the offer blob");
    assert!(second.contains("that is a reveal blob"), "{second}");
    vb.paste("not a blob at all");
    let third = vb.err.until("Nothing was sent.");
    assert!(third.contains("not a pairing blob"), "{third}");
    assert!(
        !third.contains("notablobatall"),
        "the text is never echoed: {third}"
    );
    assert!(!b.trusts(&a.key_id));
    let mut va = Verb::pair(&a.scope, PairCommand::Pair(PairRole::Initiator), true);
    let offer = va.out.json().str("blob");
    vb.paste(&offer);
    vb.out.until(BLOB_PREFIX);
    let (status, _, err) = vb.end();
    assert_eq!(status, 1, "{err}");
    let (status, _, _) = va.end();
    assert_eq!(status, 1);
    a.shutdown().await;
    b.shutdown().await;
}

#[test]
fn a_wrong_blob_kind_sends_nothing_at_all() {
    let (_, response, reveal) = blobs();
    let mut script = Script::lines([waiting_line()]);
    let done = run_pair(
        &mut script,
        PairRole::Responder,
        false,
        &format!("{response}\n\n{reveal}\n\ngarbage\n\n"),
    );
    assert_eq!(done.status, 1);
    assert_eq!(script.sent_ops(), ["pair"], "{:?}", script.sent);
    assert!(
        done.stderr.contains("that is a response blob"),
        "{}",
        done.stderr
    );
    assert!(
        done.stderr.contains("that is a reveal blob"),
        "{}",
        done.stderr
    );
    assert!(done.stderr.contains("input ended"), "{}", done.stderr);
    // Without `--json` the sentences are the whole story.
    assert!(!done.stdout.contains("rejected"), "{}", done.stdout);
}

#[test]
fn a_code_that_is_not_six_digits_is_asked_again_and_not_sent() {
    let (mut script, input) = responder_script([
        "{\"attempts_left\":4,\"oac_control\":1,\"pair\":\"mismatch\"}",
        "{\"key_id\":\"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\",\"oac_control\":1,\"pair\":\"paired\"}",
    ]);
    let codes = "12345\nabcdef\n1234567\n 12 3456\n\n654321\n 111111 \n";
    let done = run_pair(
        &mut script,
        PairRole::Responder,
        false,
        &format!("{input}{codes}"),
    );
    assert_eq!(done.status, 0, "{}", done.stderr);
    assert_eq!(
        script.sent_ops(),
        [
            "pair",
            "pair_offer",
            "pair_reveal",
            "pair_confirm",
            "pair_confirm"
        ],
        "only the two well-formed entries were sent"
    );
    assert!(
        done.stderr.contains("exactly six digits"),
        "{}",
        done.stderr
    );
    assert!(done.stderr.contains("no try was used"), "{}", done.stderr);
    assert!(done.stderr.contains("4 tries left"), "{}", done.stderr);
    assert!(done.stdout.contains("paired"), "{}", done.stdout);
}

#[test]
fn the_daemons_bad_code_reply_is_a_plain_message_and_asks_again() {
    let (mut script, input) = responder_script([
        "{\"error\":\"bad_code\",\"oac_control\":1}",
        "{\"key_id\":\"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\",\"oac_control\":1,\"pair\":\"paired\"}",
    ]);
    let done = run_pair(
        &mut script,
        PairRole::Responder,
        false,
        &format!("{input}111111\n222222\n"),
    );
    assert_eq!(done.status, 0, "{}", done.stderr);
    assert!(
        done.stderr.contains("not a six-digit code"),
        "{}",
        done.stderr
    );
    assert!(
        done.stderr.contains("not counted as a try"),
        "{}",
        done.stderr
    );
}

#[test]
fn the_daemons_bad_blob_reply_is_a_message_and_asks_again() {
    let (offer, _, reveal) = blobs();
    let mut script = Script::lines([
        waiting_line(),
        "{\"error\":\"bad_blob\",\"oac_control\":1}",
        "{\"blob\":\"oacpair1.x\",\"oac_control\":1,\"pair\":\"response\"}",
        &code_line(false),
        "{\"oac_control\":1,\"pair\":\"expired\"}",
    ]);
    let done = run_pair(
        &mut script,
        PairRole::Responder,
        false,
        &format!("{offer}\n\n{offer}\n\n{reveal}\n\n111111\n"),
    );
    assert_eq!(done.status, 1);
    assert!(
        done.stderr.contains("did not accept that blob"),
        "{}",
        done.stderr
    );
    assert!(done.stderr.contains("window ran out"), "{}", done.stderr);
}

/// Every refusal and every end the daemon can send, with the exit status and the words the
/// operator gets.
#[test]
fn every_end_and_refusal_has_a_message_a_code_and_an_exit_status() {
    let refusals = [
        ("busy", 4, "busy"),
        ("version", 4, "another version"),
        ("unknown_op", 4, "older than this"),
        ("shutting_down", 4, "daemon is stopping"),
        ("malformed", 1, "could not read the request"),
        ("too_large", 1, "too large"),
        ("wrong_role", 1, "not for this side"),
        ("no_code_yet", 1, "no code to compare"),
        ("bad_blob", 1, "did not accept that blob"),
        ("bad_code", 1, "not a six-digit code"),
        ("something_new", 1, "the daemon refused: something_new"),
    ];
    for (error, status, words) in refusals {
        // Not `bad_blob`/`bad_code`: those are asked again where they can happen; as a first
        // reply they are unexpected, and still map to a message.
        let line = format!("{{\"error\":\"{error}\",\"oac_control\":1}}");
        for json in [false, true] {
            let mut script = Script::lines([line.as_str()]);
            let done = run_pair(&mut script, PairRole::Initiator, json, "");
            if matches!(error, "bad_blob" | "bad_code") {
                assert_eq!(done.status, 1, "{error}");
                continue;
            }
            assert_eq!(done.status, status, "{error}: {}", done.stderr);
            assert!(done.stderr.contains(words), "{error}: {}", done.stderr);
            if json {
                let result = done.json();
                assert!(!result.flag("ok"));
                let code = result.str("error");
                assert!(code == error || code == "failed", "{}", result.line);
            }
        }
    }
    let ends = [
        ("second_offer", "second offer"),
        ("second_response", "second response"),
        ("second_reveal", "second reveal"),
        ("reveal_without_offer", "reveal arrived before"),
        ("abandoned", "abandoned"),
        ("expired", "120 s window"),
        ("own_key", "own key"),
        ("commitment_mismatch", "does not match the offer"),
        ("bad_line", "could not read a request"),
        ("shutdown", "daemon is stopping"),
    ];
    for (end, words) in ends {
        let line = format!("{{\"end\":\"{end}\",\"oac_control\":1,\"pair\":\"ended\"}}");
        let mut script = Script::lines([line.as_str()]);
        let done = run_pair(&mut script, PairRole::Responder, true, "");
        assert_eq!(done.status, 1, "{end}");
        assert!(done.stderr.contains(words), "{end}: {}", done.stderr);
        assert_eq!(done.json().str("error"), end);
    }
    for (terminal, words) in [
        ("aborted", "five wrong codes"),
        ("expired", "120 s window"),
        ("already_trusted", "#430"),
        ("not_saved", "NOT added"),
    ] {
        let line = format!("{{\"oac_control\":1,\"pair\":\"{terminal}\"}}");
        let mut script = Script::lines([line.as_str()]);
        let done = run_pair(&mut script, PairRole::Responder, true, "");
        assert_eq!(done.status, 1, "{terminal}");
        assert!(done.stderr.contains(words), "{terminal}: {}", done.stderr);
        assert_eq!(done.json().str("error"), terminal);
    }
    // An end this verb does not know is still an end.
    let mut script =
        Script::lines(["{\"end\":\"new\\u0007\",\"oac_control\":1,\"pair\":\"ended\"}"]);
    let done = run_pair(&mut script, PairRole::Responder, false, "");
    assert_eq!(done.status, 1);
    assert!(
        done.stderr.contains("the pairing ended: new?"),
        "{}",
        done.stderr
    );
    // The connection: closed, timed out, garbage.
    for (recv, code, words) in [
        (Recv::Closed, "connection_closed", "closed the connection"),
        (Recv::TimedOut, "timeout", "did not answer in time"),
        (Recv::Line("garbage".into()), "failed", "not understood"),
        (
            Recv::Line("{\"pair\":\"surprise\"}".into()),
            "unexpected_reply",
            "did not expect",
        ),
    ] {
        let mut script = Script::new([recv]);
        let done = run_pair(&mut script, PairRole::Responder, true, "");
        assert_eq!(done.status, 1, "{code}");
        assert!(done.stderr.contains(words), "{code}: {}", done.stderr);
        assert_eq!(done.json().str("error"), code);
    }
}

#[test]
fn already_trusted_is_its_own_outcome_and_the_operator_is_told_before_the_code() {
    let (offer, _, reveal) = blobs();
    let mut script = Script::lines([
        waiting_line(),
        "{\"blob\":\"oacpair1.x\",\"oac_control\":1,\"pair\":\"response\"}",
        &code_line(true),
        "{\"oac_control\":1,\"pair\":\"already_trusted\"}",
    ]);
    let done = run_pair(
        &mut script,
        PairRole::Responder,
        false,
        &format!("{offer}\n\n{reveal}\n\n123456\n"),
    );
    assert_eq!(done.status, 1);
    let before_code = done
        .stderr
        .find("already trusted on this device")
        .expect("told");
    let prompt = done.stderr.rfind("Type the 6-digit code").expect("asked");
    assert!(before_code < prompt, "{}", done.stderr);
    assert!(
        done.stderr.contains(&format!("oac unpair {KEY}")),
        "{}",
        done.stderr
    );
    assert!(!done.stderr.contains("<key_id>"), "{}", done.stderr);
    assert!(
        done.stderr[before_code..prompt].contains(&format!("`oac unpair {KEY}`")),
        "the warning names the real key: {}",
        done.stderr
    );
    assert!(done.stderr.contains("#430"), "{}", done.stderr);
    // In `--json` the code step says so and the error carries the key id.
    let mut script = Script::lines([
        waiting_line(),
        "{\"blob\":\"oacpair1.x\",\"oac_control\":1,\"pair\":\"response\"}",
        &code_line(true),
        "{\"oac_control\":1,\"pair\":\"already_trusted\"}",
    ]);
    let done = run_pair(
        &mut script,
        PairRole::Responder,
        true,
        &format!("{offer}\n\n{reveal}\n\n123456\n"),
    );
    assert_eq!(done.status, 1);
    let steps: Vec<&str> = done.stdout.lines().collect();
    assert!(steps[2].contains("\"already_trusted\":true"), "{steps:?}");
    let result = done.json();
    assert_eq!(result.str("error"), "already_trusted");
    assert_eq!(result.str("key_id"), KEY);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pairing_the_same_two_devices_again_exits_1_as_already_trusted() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    pair_fully(&a, &b);
    let p = begin(&a, &b);
    assert!(p.code_a.flag("already_trusted"));
    assert!(p.code_b.flag("already_trusted"));
    let (code_a, code_b) = (p.code_a.str("code"), p.code_b.str("code"));
    let (mut va, mut vb) = (p.a, p.b);
    va.type_line(&code_b);
    let result = va.out.json();
    assert_eq!(result.str("error"), "already_trusted");
    assert_eq!(result.str("key_id"), b.key_id.as_str());
    vb.type_line(&code_a);
    assert_eq!(vb.out.json().str("error"), "already_trusted");
    let (status, _, err) = va.end();
    assert_eq!(status, 1);
    assert!(err.contains("run `oac unpair"), "{err}");
    assert_eq!(vb.end().0, 1);
    a.shutdown().await;
    b.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn five_wrong_codes_exit_1_and_a_malformed_entry_costs_no_try() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    // Four wrong entries and a right one, with malformed entries between: paired, so the
    // malformed ones were not counted.
    let p = begin(&a, &b);
    let (code_a, code_b) = (p.code_a.str("code"), p.code_b.str("code"));
    let (mut va, mut vb) = (p.a, p.b);
    for left in (1..=4).rev() {
        va.type_line("12x45");
        let rejected = va.out.json();
        assert_eq!(rejected.str("step"), "rejected", "{}", rejected.line);
        assert_eq!(rejected.str("reason"), "not_six_digits");
        va.type_line(&other_than(&code_b));
        let mismatch = va.out.json();
        assert_eq!(mismatch.str("step"), "mismatch");
        assert_eq!(
            mismatch
                .object
                .get("attempts_left")
                .and_then(Json::as_number)
                .map(|n| n.raw().to_owned()),
            Some(left.to_string())
        );
    }
    va.type_line(&code_b);
    assert!(va.out.json().flag("paired"));
    // The other side types five wrong codes: aborted, and B does not trust A.
    for _ in 0..4 {
        vb.type_line(&other_than(&code_a));
        assert_eq!(vb.out.json().step(), "mismatch");
    }
    vb.type_line(&other_than(&code_a));
    let result = vb.out.json();
    assert_eq!(result.str("error"), "aborted", "{}", result.line);
    let (status, _, err) = vb.end();
    assert_eq!(status, 1);
    assert!(err.contains("five wrong codes"), "{err}");
    assert_eq!(va.end().0, 0);
    assert!(a.trusts(&b.key_id), "A trusts B");
    assert!(!b.trusts(&a.key_id), "B does not trust A: a half pairing");
    a.shutdown().await;
    b.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_daemon_every_verb_exits_3() {
    let namespace = format!(
        "pv{}none{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    );
    let scope = Scope::for_user_in(Some(&namespace)).unwrap();
    let _dir = NamespaceDir::of(&scope);
    for command in [
        PairCommand::Pair(PairRole::Initiator),
        PairCommand::Pair(PairRole::Responder),
        PairCommand::List,
    ] {
        for json in [false, true] {
            let v = Verb::pair(&scope, command, json);
            let (status, out, err) = v.end();
            assert_eq!(status, 3, "{command:?}");
            assert!(err.contains("not running"), "{err}");
            if json {
                assert!(out.contains("\"error\":\"not_running\""), "{out}");
            } else {
                assert_eq!(out, "");
            }
        }
    }
    let v = Verb::unpair(&scope, &KeyId::parse(KEY).unwrap(), true, true);
    let (status, out, _) = v.end();
    assert_eq!(status, 3);
    assert!(out.contains("\"error\":\"not_running\""), "{out}");
    assert!(
        out.contains(&format!("\"key_id\":\"{KEY}\"")),
        "every unpair failure carries the key: {out}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_pairing_is_busy_and_exits_4() {
    let mut b = node("beta").await;
    let mut first = Verb::pair(&b.scope, PairCommand::Pair(PairRole::Responder), true);
    assert_eq!(first.out.json().step(), "waiting");
    let mut second = Verb::pair(&b.scope, PairCommand::Pair(PairRole::Initiator), true);
    let result = second.out.json();
    assert_eq!(result.str("error"), "busy", "{}", result.line);
    let (status, _, err) = second.end();
    assert_eq!(status, 4);
    assert!(err.contains("another pairing is open"), "{err}");
    // The first is untouched, until its input ends.
    let (status, out, _) = first.end();
    assert_eq!(status, 1);
    assert!(out.contains("input_ended"), "{out}");
    b.shutdown().await;
}

#[test]
fn usage_errors_exit_2_before_anything_is_connected() {
    let args = |a: &[&str]| -> Vec<String> { a.iter().map(|s| (*s).to_owned()).collect() };
    assert_eq!(oac_cli::pair_verbs::pair(&args(&["--bogus"])), 2);
    assert_eq!(
        oac_cli::pair_verbs::pair(&args(&["--list", "--respond"])),
        2
    );
    assert_eq!(oac_cli::pair_verbs::unpair(&args(&[])), 2);
    assert_eq!(oac_cli::pair_verbs::unpair(&args(&["0123"])), 2);
    // No terminal and no --yes: there is no one to confirm.
    let scope = Scope::unique().unwrap();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut stdin = Cursor::new(Vec::new());
    let status = unpair_with(
        &scope,
        KEY,
        false,
        false,
        false,
        Duration::from_secs(1),
        &mut Terminal {
            stdin: &mut stdin,
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );
    assert_eq!(status, 2);
    assert!(String::from_utf8(stderr).unwrap().contains("pass --yes"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unpair_asks_on_a_terminal_and_removes_only_on_yes() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    pair_fully(&a, &b);
    let scope = a.scope.clone();
    let key = b.key_id.as_str().to_owned();
    let run = |answer: &'static str| {
        let (scope, key) = (scope.clone(), key.clone());
        let mut v = Verb::spawn(move |t| unpair_with(&scope, &key, false, true, true, WAIT, t));
        v.err.until("Type yes to continue: ");
        v.type_line(answer);
        v
    };
    let (status, out, _) = run("no").end();
    assert_eq!(status, 1);
    assert!(out.contains("\"error\":\"declined\""), "{out}");
    assert!(a.trusts(&b.key_id), "nothing was removed");
    let mut v = run("YES");
    let result = v.out.json();
    assert_eq!(
        result.names(),
        ["durable", "key_id", "oac_cli", "ok", "unpaired"],
        "{}",
        result.line
    );
    assert!(result.flag("ok") && result.flag("unpaired") && result.flag("durable"));
    assert_eq!(v.end().0, 0);
    assert!(!a.trusts(&b.key_id));
    a.shutdown().await;
    b.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unpair_of_a_key_that_is_not_trusted_or_is_this_devices_own_exits_1() {
    let mut a = node("alpha").await;
    let stranger = KeyId::parse(KEY).unwrap();
    let (status, out, err) = Verb::unpair(&a.scope, &stranger, true, true).end();
    assert_eq!(status, 1);
    assert!(out.contains("\"error\":\"not_trusted\""), "{out}");
    assert!(out.contains(KEY), "{out}");
    assert!(err.contains("not trusted"), "{err}");
    let own = a.key_id.clone();
    let (status, out, _) = Verb::unpair(&a.scope, &own, true, true).end();
    assert_eq!(status, 1);
    assert!(out.contains("\"error\":\"own_key\""), "{out}");
    a.shutdown().await;
}

/// N5, at the verb: an unpair whose save fails exits 1 and says the revocation is not
/// durable; the daemon holds the retry (S3 proves the backoff on the paused clock).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unpair_whose_save_fails_exits_1_and_is_retried() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    pair_fully(&a, &b);
    a.fail.store(true, Ordering::SeqCst);
    let mut v = Verb::unpair(&a.scope, &b.key_id, true, true);
    let result = v.out.json();
    assert_eq!(
        result.names(),
        ["durable", "error", "key_id", "oac_cli", "ok", "unpaired"],
        "{}",
        result.line
    );
    assert_eq!(result.str("error"), "not_durable");
    assert!(!result.flag("ok") && !result.flag("durable") && result.flag("unpaired"));
    let (status, _, err) = v.end();
    assert_eq!(status, 1);
    assert!(err.contains("NOT saved"), "{err}");
    assert!(err.contains("NOT durable"), "{err}");
    assert!(!a.trusts(&b.key_id), "removed in memory");
    assert!(
        a.daemon().pipelines().next_trust_retry_at().is_some(),
        "the daemon holds the retry"
    );
    // `--list` says the store is not saved (and still exits 0: the list worked).
    let mut list = Verb::pair(&a.scope, PairCommand::List, true);
    let result = list.out.json();
    assert!(result.object.get("saved").and_then(Json::as_bool) == Some(false));
    assert_eq!(list.end().0, 0);
    let human = Verb::pair(&a.scope, PairCommand::List, false).end();
    assert_eq!(human.0, 0);
    assert!(
        human.2.contains("NOT saved"),
        "also on standard error: {}",
        human.2
    );
    assert!(human.1.contains("NOT saved"), "{}", human.1);
    a.shutdown().await;
    b.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_shows_the_trusted_keys_and_the_recent_events() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    let mut empty = Verb::pair(&a.scope, PairCommand::List, true);
    let result = empty.out.json();
    assert_eq!(result.names(), ["events", "keys", "oac_cli", "ok", "saved"]);
    assert_eq!(
        result.line.matches("\"keys\":[]").count(),
        1,
        "{}",
        result.line
    );
    assert_eq!(empty.end().0, 0);
    pair_fully(&a, &b);
    let mut list = Verb::pair(&a.scope, PairCommand::List, true);
    let result = list.out.json();
    assert_eq!(list.end().0, 0);
    assert_eq!(result.names(), ["events", "keys", "oac_cli", "ok", "saved"]);
    assert!(result.flag("saved") && result.flag("ok"));
    let keys = result.object.get("keys").and_then(Json::as_array).unwrap();
    assert_eq!(keys.len(), 1, "this device's own key is not listed");
    let key = keys[0].as_object().unwrap();
    let mut names: Vec<&str> = key.names().collect();
    names.sort_unstable();
    assert_eq!(names, ["confirmed", "key_id", "paired_at", "principal"]);
    assert_eq!(
        key.get("key_id").and_then(Json::as_str),
        Some(b.key_id.as_str())
    );
    let events = result
        .object
        .get("events")
        .and_then(Json::as_array)
        .unwrap();
    let event = events.last().unwrap().as_object().unwrap();
    assert_eq!(event.get("kind").and_then(Json::as_str), Some("paired"));
    assert_eq!(
        event.get("key_id").and_then(Json::as_str),
        Some(b.key_id.as_str())
    );
    // The human form leads with the key id.
    let (status, out, _) = Verb::pair(&a.scope, PairCommand::List, false).end();
    assert_eq!(status, 0);
    assert!(out.contains("Trusted keys (1):"), "{out}");
    assert!(out.contains(b.key_id.as_str()), "{out}");
    assert!(out.contains("Trust store: saved."), "{out}");
    assert!(out.contains("paired"), "{out}");
    a.shutdown().await;
    b.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_human_pairing_shows_the_codes_and_never_a_blob_on_standard_error() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    let mut vb = Verb::pair(&b.scope, PairCommand::Pair(PairRole::Responder), false);
    let mut va = Verb::pair(&a.scope, PairCommand::Pair(PairRole::Initiator), false);
    // Carry the blobs by reading the wrapped lines off the screens.
    let blob = |screen: &mut Screen| -> String {
        screen.until(BLOB_PREFIX);
        let mut blob = BLOB_PREFIX.to_owned() + &screen.line();
        loop {
            let line = screen.line();
            if line.is_empty() {
                return blob;
            }
            blob.push_str(&line);
        }
    };
    let code = |screen: &mut Screen| -> String {
        screen.until("This device's code: ");
        screen.line().trim().to_owned()
    };
    let offer = blob(&mut va.out);
    vb.paste(&offer);
    let response = blob(&mut vb.out);
    va.paste(&response);
    let reveal = blob(&mut va.out);
    let code_a = code(&mut va.out);
    vb.paste(&reveal);
    let code_b = code(&mut vb.out);
    assert_eq!(code_a.len(), 6);
    va.type_line(&code_b);
    vb.type_line(&code_a);
    let (status, out_a, err_a) = va.end();
    assert_eq!(status, 0, "{err_a}");
    let (status, out_b, err_b) = vb.end();
    assert_eq!(status, 0, "{err_b}");
    for err in [&err_a, &err_b] {
        assert!(
            !err.contains(BLOB_PREFIX),
            "no blob on standard error: {err}"
        );
    }
    for out in [&out_a, &out_b] {
        assert!(out.contains("paired. This device now trusts key"), "{out}");
        assert!(out.contains("grants nothing"), "{out}");
    }
    assert!(out_a.contains(b.key_id.as_str()) && out_b.contains(a.key_id.as_str()));
    // Each prompt names the other device's code and the seconds left.
    assert!(
        err_a.contains("Type the 6-digit code the OTHER device shows ("),
        "{err_a}"
    );
    assert!(err_a.contains("s left)"), "{err_a}");
    a.shutdown().await;
    b.shutdown().await;
}

/// ADR-001 and the design note §3 (default J): the client path holds no key.
#[test]
fn oac_pair_never_builds_a_key_store() {
    let source = include_str!("../src/pair_verbs.rs").replace("\r\n", "\n");
    // Everything before the unit tests module is the client path; comments are not code.
    let tests_at = source
        .rfind("#[cfg(test)]\nmod tests {")
        .expect("the unit tests module");
    let client: String = source[..tests_at]
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "KeyStore",
        "keystore",
        "Keyring",
        "keyring",
        "credential",
        "DeviceKey",
        "DeviceIdentity",
        "SecretSeed",
        "load_or_generate",
        "PairingInitiator",
        "ResponderPairing",
        "PairingSession",
        "PairingStore",
        "AgreementKey",
        "Pipelines",
        "Daemon",
        "trust_store",
        "agreement_file",
        "std::env::var",
    ] {
        assert!(
            !client.contains(forbidden),
            "the client path mentions `{forbidden}`"
        );
    }
    // Every reference into the crate, `use` or path, is to the control endpoint, the blob
    // decoder, the local IPC, the text cleaner or the verbs' shared items; and the core is
    // reached for JSON and ids only.
    let modules = |prefix: &str| -> Vec<String> {
        client
            .match_indices(prefix)
            .map(|(i, _)| {
                client[i + prefix.len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect()
            })
            .collect()
    };
    let crate_modules = modules("crate::");
    assert!(!crate_modules.is_empty());
    for module in &crate_modules {
        assert!(
            ["control", "device_pairing", "ipc", "status", "verbs"].contains(&module.as_str()),
            "a reference outside the client path: crate::{module}"
        );
    }
    for module in modules("oac_core::") {
        assert!(
            ["json", "ids"].contains(&module.as_str()),
            "a reference to the core outside json and ids: oac_core::{module}"
        );
    }
    let blob_imports = client
        .lines()
        .find(|l| l.contains("use crate::device_pairing::"))
        .unwrap();
    assert!(
        !blob_imports.contains("DevicePairing") && !blob_imports.contains("Role"),
        "{blob_imports}"
    );
    // And the binary's arms call this module, not the commands that build a key store.
    let main = include_str!("../src/main.rs");
    assert!(main.contains("pair_verbs::pair(") && main.contains("pair_verbs::unpair("));
    // The way the scope is made is shared through `verbs`, not taken from `commands`.
    assert!(client.contains("user_scope") && !client.contains("commands"));
}

/// Under `--json` standard output is the only thing a driving program reads: an input that is
/// asked for again must be announced there, or the program waits for a step that never comes.
#[test]
fn under_json_every_input_that_is_asked_again_is_a_rejected_step() {
    let (offer, response, reveal) = blobs();
    let paired = format!("{{\"key_id\":\"{KEY}\",\"oac_control\":1,\"pair\":\"paired\"}}");
    let mut script = Script::lines([
        waiting_line(),
        "{\"error\":\"bad_blob\",\"oac_control\":1}",
        "{\"blob\":\"oacpair1.x\",\"oac_control\":1,\"pair\":\"response\"}",
        &code_line(false),
        "{\"error\":\"bad_code\",\"oac_control\":1}",
        &paired,
    ]);
    let big = "A".repeat(5000);
    let input = format!(
        "{response}\n\ngarbage\n\n{big}\n\n{offer}\n\n{offer}\n\n{reveal}\n\n12345\n111111\n222222\n"
    );
    let done = run_pair(&mut script, PairRole::Responder, true, &input);
    assert_eq!(done.status, 0, "{}", done.stderr);
    let steps: Vec<Obj> = done
        .stdout
        .lines()
        .map(|line| Obj {
            object: json::parse(line.as_bytes()).unwrap().into_object().unwrap(),
            line: line.to_owned(),
        })
        .collect();
    let sequence: Vec<String> = steps
        .iter()
        .map(|o| {
            if o.object.contains("step") {
                match o.str("step").as_str() {
                    "rejected" => format!("rejected:{}", o.str("reason")),
                    other => other.to_owned(),
                }
            } else {
                "result".to_owned()
            }
        })
        .collect();
    assert_eq!(
        sequence,
        [
            "waiting",
            "rejected:wrong_kind",
            "rejected:not_a_blob",
            "rejected:too_large",
            "rejected:bad_blob",
            "response",
            "code",
            "rejected:not_six_digits",
            "rejected:bad_code",
            "result",
        ]
    );
    assert_eq!(steps[1].names(), ["oac_cli", "reason", "step"]);
    // Nothing the operator pasted is echoed, in either stream.
    assert!(!done.stdout.contains("garbage") && !done.stderr.contains("garbage"));
    assert!(!done.stdout.contains(&big[..64]) && !done.stderr.contains(&big[..64]));
}

/// Every answer the daemon can give to an unpair, with the exit status and the words.
#[test]
fn every_unpair_answer_has_a_message_a_code_and_an_exit_status() {
    let cases: [(&str, i32, &str, &str); 11] = [
        (
            "{\"durable\":true,\"oac_control\":1,\"unpaired\":true}",
            0,
            "removed key",
            "",
        ),
        (
            "{\"durable\":false,\"oac_control\":1,\"unpaired\":true}",
            1,
            "NOT durable",
            "not_durable",
        ),
        (
            "{\"error\":\"not_trusted\",\"oac_control\":1}",
            1,
            "not trusted",
            "not_trusted",
        ),
        (
            "{\"error\":\"own_key\",\"oac_control\":1}",
            1,
            "own key",
            "own_key",
        ),
        ("{\"error\":\"busy\",\"oac_control\":1}", 4, "busy", "busy"),
        (
            "{\"error\":\"shutting_down\",\"oac_control\":1}",
            4,
            "is stopping",
            "shutting_down",
        ),
        (
            "{\"error\":\"version\",\"oac_control\":1}",
            4,
            "another version",
            "version",
        ),
        (
            "{\"error\":\"unknown_op\",\"oac_control\":1}",
            4,
            "older than this",
            "unknown_op",
        ),
        (
            "{\"error\":\"malformed\",\"oac_control\":1}",
            1,
            "could not read",
            "malformed",
        ),
        (
            "{\"oac_control\":1}",
            1,
            "did not expect",
            "unexpected_reply",
        ),
        ("not json", 1, "not understood", "failed"),
    ];
    for (line, status, words, code) in cases {
        for json in [false, true] {
            let mut script = Script::lines([line]);
            let done = run_unpair(&mut script, KEY, json);
            assert_eq!(done.status, status, "{line}");
            assert_eq!(script.sent_ops(), ["unpair"]);
            assert!(script.sent[0].contains(KEY), "{}", script.sent[0]);
            let said = if status == 0 {
                &done.stdout
            } else {
                &done.stderr
            };
            // A success under `--json` is the object alone.
            if status != 0 || !json {
                assert!(said.contains(words), "{line}: {said}");
            }
            if json {
                let result = done.json();
                assert_eq!(result.str("key_id"), KEY, "{}", result.line);
                assert_eq!(result.flag("ok"), status == 0, "{}", result.line);
                if status != 0 {
                    assert_eq!(result.str("error"), code, "{}", result.line);
                }
            } else if status != 0 {
                assert_eq!(done.stdout, "", "a failure is on standard error only");
            }
        }
    }
    for (recv, code) in [
        (Recv::Closed, "connection_closed"),
        (Recv::TimedOut, "timeout"),
    ] {
        let mut script = Script::new([recv]);
        let done = run_unpair(&mut script, KEY, true);
        assert_eq!(done.status, 1);
        assert_eq!(done.json().str("error"), code);
        assert_eq!(done.json().str("key_id"), KEY);
    }
}
