// SPDX-License-Identifier: Apache-2.0

//! The pairing control ops and the daemon's state machine (#437, slice S3;
//! `docs/planning/decisions/G-437-pairing.md` §4, §9, §10).
//!
//! Every test runs real in-process daemons, each in its own IPC namespace with its own
//! in-memory stores, over the in-memory transport (the pairing exchange never touches the
//! peer transport), and speaks the control protocol over the real local IPC. A "device" that
//! is not a daemon is a core `DeviceIdentity` driving `PairingInitiator` or
//! `ResponderPairing` directly: the same code a daemon runs, so it proves the blobs
//! interoperate with the core's own exchange.
//!
//! **Time.** No test sleeps or polls. The core's clock is a `ManualClock`. The tests that
//! need the 2 s and 120 s deadlines pause tokio's clock **after** the I/O they depend on has
//! been set up (a paused runtime jumps to its next timer whenever it is idle, which would
//! race an in-flight read), and then let the runtime jump: the reads that wait for a
//! deadline are the only ones in flight. A test that waits for the end of a pairing waits on
//! the daemon's event signal (`DevicePairing::watch_events`), not on a timer.
//!
//! Real credential stores and real data directories are never touched.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use oac_cli::control::{STATUS_REQUEST, STOP_REQUEST, json_string};
use oac_cli::daemon::{Daemon, LogSink, Parts};
use oac_cli::device_pairing::{
    BLOB_PREFIX, Blob, EventKind, MAX_LINE, UnpairResult, decode_blob, encode_offer,
    encode_response, encode_reveal,
};
use oac_cli::ipc::{AsyncStream, Endpoint, Scope};
use oac_core::authorization::OperatorConfirmed;
use oac_core::clock::{Clock, ManualClock};
use oac_core::ids::{KeyId, Timestamp, Token};
use oac_core::json::{self, JsonObject};
use oac_core::keys::{DeviceIdentity, DeviceKey, MemoryKeyStore};
use oac_core::pairing::{
    MemoryPairingStore, PairedPeer, PairingInitiator, PairingReveal, PairingSnapshot, PairingStore,
    PairingStoreError, ResponderPairing,
};
use oac_core::pipeline::{MonotonicClock, PipelineConfig};
use oac_core::sealing::MemoryAgreementKeyStore;
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Notify, watch};

/// How long a test waits for a daemon to answer before it fails (real time on an unpaused
/// clock, so a hung daemon is a failure and not a hang).
const WAIT: Duration = Duration::from_secs(30);

// ---- the fixtures ---------------------------------------------------------------------------

static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
    fn lines_with(&self, needle: &str) -> usize {
        self.text().lines().filter(|l| l.contains(needle)).count()
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

/// A store save that blocks until released, once armed: a save in flight.
#[derive(Default)]
struct Gate {
    armed: AtomicBool,
    entered: Notify,
    released: (Mutex<bool>, Condvar),
}

impl Gate {
    fn arm(&self) {
        *self.released.0.lock().unwrap() = false;
        self.armed.store(true, Ordering::SeqCst);
    }
    fn release(&self) {
        *self.released.0.lock().unwrap() = true;
        self.released.1.notify_all();
    }
    fn pass(&self) {
        if self.armed.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            let mut open = self.released.0.lock().unwrap();
            while !*open {
                open = self.released.1.wait(open).unwrap();
            }
        }
    }
}

/// A pairing store the test can make fail, gate and watch.
#[derive(Clone)]
struct TestStore {
    inner: Arc<MemoryPairingStore>,
    fail: Arc<AtomicBool>,
    /// What happened to the store, in order.
    events: Arc<Mutex<Vec<String>>>,
    /// The virtual or real instant and outcome of each save.
    attempts: Arc<Mutex<Vec<(tokio::time::Instant, bool)>>>,
    changed: Arc<watch::Sender<u64>>,
    gate: Arc<Gate>,
}

impl TestStore {
    fn new() -> TestStore {
        TestStore {
            inner: Arc::new(MemoryPairingStore::new()),
            fail: Arc::new(AtomicBool::new(false)),
            events: Arc::default(),
            attempts: Arc::default(),
            changed: Arc::new(watch::channel(0).0),
            gate: Arc::default(),
        }
    }
    fn fail(&self, fail: bool) {
        self.fail.store(fail, Ordering::SeqCst);
    }
    fn saves(&self) -> usize {
        self.attempts
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, ok)| *ok)
            .count()
    }
    fn attempts(&self) -> usize {
        self.attempts.lock().unwrap().len()
    }
}

impl PairingStore for TestStore {
    fn load(&self) -> Result<PairingSnapshot, PairingStoreError> {
        self.inner.load()
    }
    fn save(&self, snapshot: &PairingSnapshot) -> Result<(), PairingStoreError> {
        self.gate.pass();
        let ok = !self.fail.load(Ordering::SeqCst);
        self.attempts
            .lock()
            .unwrap()
            .push((tokio::time::Instant::now(), ok));
        let result = if ok {
            self.events.lock().unwrap().push("save".to_owned());
            self.inner.save(snapshot)
        } else {
            Err(PairingStoreError::Failed("the test refuses".into()))
        };
        self.changed.send_modify(|n| *n += 1);
        result
    }
}

fn start_time() -> Timestamp {
    Timestamp::parse("2026-11-05T12:00:00.000Z").unwrap()
}

/// One daemon, in its own namespace.
struct Node {
    daemon: Option<Daemon>,
    endpoint: Endpoint,
    log: Captured,
    store: TestStore,
    clock: Arc<ManualClock>,
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
    async fn client(&self) -> Client {
        Client::open(&self.endpoint).await
    }
    async fn shutdown(&mut self) {
        if let Some(d) = self.daemon.take() {
            d.shutdown().await;
        }
    }
    /// The next pairing event, after `seen` events were seen.
    async fn event_after(&self, seen: u64) {
        let mut rx = self.daemon().pairing().watch_events();
        tokio::time::timeout(WAIT, rx.wait_for(|n| *n > seen))
            .await
            .expect("the daemon recorded an event")
            .expect("the daemon is alive");
    }
}

async fn node(principal: &str) -> Node {
    let network = MemoryNetwork::new();
    let net = network.clone();
    node_with(principal, Arc::new(move || net.now()), network).await
}

/// A node whose monotonic clock is tokio's (for the paused-clock tests).
async fn node_on_tokio_clock(principal: &str) -> Node {
    node_with(
        principal,
        Arc::new(|| tokio::time::Instant::now().into_std()),
        MemoryNetwork::new(),
    )
    .await
}

async fn node_with(principal: &str, monotonic: MonotonicClock, network: MemoryNetwork) -> Node {
    let namespace = format!(
        "dp{}n{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    );
    let scope = Scope::for_user_in(Some(&namespace)).unwrap();
    let dir = NamespaceDir::of(&scope);
    let log = Captured::default();
    let store = TestStore::new();
    let clock = Arc::new(ManualClock::new(start_time()));
    let daemon = Daemon::start(Parts {
        scope: scope.clone(),
        key_store: Box::new(MemoryKeyStore::new()),
        pairing_store: Box::new(store.clone()),
        agreement_store: Box::new(MemoryAgreementKeyStore::new()),
        principal: Token::parse(principal).unwrap(),
        transport: Arc::new(MemoryTransport::new()),
        transport_configuration: MemoryConfiguration::wrap(&network),
        clock: clock.clone(),
        monotonic,
        config: PipelineConfig::default(),
        harnesses: Vec::new(),
        log: LogSink::new(log.clone()),
        codex: None,
    })
    .await
    .unwrap_or_else(|e| panic!("the daemon starts: {e}"));
    let key_id = daemon.pipelines().device().key_id().clone();
    Node {
        daemon: Some(daemon),
        endpoint: scope.endpoint("control"),
        log,
        store,
        clock,
        key_id,
        _dir: dir,
    }
}

/// A reply line, read as an object.
struct Reply {
    text: String,
    object: JsonObject,
}

impl Reply {
    fn parse(text: String) -> Reply {
        let object = json::parse(text.trim_end().as_bytes())
            .unwrap_or_else(|e| panic!("{text:?}: {e}"))
            .into_object()
            .expect("an object");
        Reply { text, object }
    }
    fn str(&self, name: &str) -> String {
        self.object
            .get(name)
            .and_then(json::Json::as_str)
            .unwrap_or_else(|| panic!("no string {name:?} in {}", self.text))
            .to_owned()
    }
    fn has(&self, name: &str) -> bool {
        self.object.contains(name)
    }
    fn flag(&self, name: &str) -> bool {
        self.object
            .get(name)
            .and_then(json::Json::as_bool)
            .unwrap_or_else(|| panic!("no boolean {name:?} in {}", self.text))
    }
    /// The `pair` member.
    fn pair(&self) -> String {
        self.str("pair")
    }
    fn is_error(&self, code: &str) -> bool {
        self.object.get("error").and_then(json::Json::as_str) == Some(code)
    }
}

/// A control connection.
struct Client {
    stream: Box<dyn AsyncStream>,
    buf: Vec<u8>,
    wait: Duration,
    /// The longest line sent and received, in bytes, newline excluded.
    longest_sent: usize,
    longest_received: usize,
}

impl Client {
    async fn open(endpoint: &Endpoint) -> Client {
        Client {
            stream: endpoint
                .connect()
                .await
                .expect("the control endpoint answers"),
            buf: Vec::new(),
            wait: WAIT,
            longest_sent: 0,
            longest_received: 0,
        }
    }
    async fn send(&mut self, line: &str) {
        assert!(line.ends_with('\n'));
        self.longest_sent = self.longest_sent.max(line.len() - 1);
        self.stream.write_all(line.as_bytes()).await.expect("sent");
        self.stream.flush().await.expect("flushed");
    }
    /// The next line, or `None` when the daemon closed the connection.
    async fn line(&mut self) -> Option<String> {
        let wait = self.wait;
        tokio::time::timeout(wait, async {
            loop {
                if let Some(i) = self.buf.iter().position(|b| *b == b'\n') {
                    let line: Vec<u8> = self.buf.drain(..=i).collect();
                    let text = String::from_utf8(line).unwrap();
                    self.longest_received = self.longest_received.max(text.len() - 1);
                    return Some(text);
                }
                let mut chunk = [0u8; 4096];
                match self.stream.read(&mut chunk).await {
                    Ok(0) | Err(_) => return None,
                    Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                }
            }
        })
        .await
        .expect("the daemon answered in time")
    }
    async fn reply(&mut self) -> Reply {
        Reply::parse(self.line().await.expect("a reply, not a closed connection"))
    }
    async fn ask(&mut self, line: &str) -> Reply {
        self.send(line).await;
        self.reply().await
    }
    /// Whether the daemon has closed this connection (nothing more to read).
    async fn closed(&mut self) -> bool {
        self.line().await.is_none()
    }
}

fn pair_line(role: &str) -> String {
    format!("{{\"oac_control\":1,\"op\":\"pair\",\"role\":\"{role}\"}}\n")
}

fn op_line(op: &str, name: &str, value: &str) -> String {
    format!(
        "{{\"oac_control\":1,\"op\":\"{op}\",\"{name}\":{}}}\n",
        json_string(value)
    )
}

fn fake(principal: &str) -> DeviceIdentity {
    DeviceIdentity::new(DeviceKey::generate(), Token::parse(principal).unwrap())
}

/// A code that is not `code`.
fn other_than(code: &str) -> String {
    let first = if code.starts_with('9') { '1' } else { '9' };
    format!("{first}{}", &code[1..])
}

/// Both devices at their codes.
struct Shown {
    a: Client,
    b: Client,
    code_a: String,
    code_b: String,
    offer_reply: String,
    response_blob: String,
    reveal_blob: String,
}

/// The whole exchange between two daemons, `a` the initiator and `b` the responder, up to
/// the point where each shows its code; the operator carries each blob.
async fn exchange(a: &Node, b: &Node) -> Shown {
    let mut cb = b.client().await;
    assert_eq!(cb.ask(&pair_line("responder")).await.pair(), "waiting");
    let mut ca = a.client().await;
    let offer = ca.ask(&pair_line("initiator")).await;
    assert_eq!(offer.pair(), "offer");
    let response = cb
        .ask(&op_line("pair_offer", "blob", &offer.str("blob")))
        .await;
    assert_eq!(response.pair(), "response", "{}", response.text);
    let coded_a = ca
        .ask(&op_line("pair_response", "blob", &response.str("blob")))
        .await;
    assert_eq!(coded_a.pair(), "code", "{}", coded_a.text);
    let coded_b = cb
        .ask(&op_line("pair_reveal", "blob", &coded_a.str("blob")))
        .await;
    assert_eq!(coded_b.pair(), "code", "{}", coded_b.text);
    // Each side names the other device.
    assert_eq!(coded_a.str("peer_key_id"), b.key_id.as_str());
    assert_eq!(coded_b.str("peer_key_id"), a.key_id.as_str());
    Shown {
        a: ca,
        b: cb,
        code_a: coded_a.str("code"),
        code_b: coded_b.str("code"),
        offer_reply: offer.text,
        response_blob: response.str("blob"),
        reveal_blob: coded_a.str("blob"),
    }
}

/// A full honest pairing of `a` and `b`: both type the other's code.
async fn pair_fully(a: &Node, b: &Node) {
    let mut s = exchange(a, b).await;
    assert_eq!(
        s.a.ask(&op_line("pair_confirm", "entered", &s.code_b))
            .await
            .pair(),
        "paired"
    );
    assert_eq!(
        s.b.ask(&op_line("pair_confirm", "entered", &s.code_a))
            .await
            .pair(),
        "paired"
    );
}

// ---- the exchange between two honest daemons ----------------------------------------------

/// Proves only that a man in the middle between two **honest** devices does not get through
/// (T1, T3): a process that controls a device sees its code and types it (T5, lead decision
/// L5), and nothing here says otherwise.
#[tokio::test]
async fn pairing_completes_between_two_honest_daemons_only_on_the_entered_code() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    let mut s = exchange(&a, &b).await;
    assert_eq!(
        s.code_a, s.code_b,
        "honest devices derive one code: each shows it and the operator types it on the other"
    );
    assert_eq!(s.code_a.len(), 6);
    assert!(s.code_a.bytes().all(|c| c.is_ascii_digit()));
    assert!(
        !s.a.ask(&op_line("pair_confirm", "entered", "abc"))
            .await
            .has("key_id")
    );
    // A wrong entry trusts nothing, on either side.
    let wrong = other_than(&s.code_a);
    let m = s.a.ask(&op_line("pair_confirm", "entered", &wrong)).await;
    assert_eq!(m.pair(), "mismatch");
    let m = s.b.ask(&op_line("pair_confirm", "entered", &wrong)).await;
    assert_eq!(m.pair(), "mismatch");
    assert!(!a.trusts(&b.key_id) && !b.trusts(&a.key_id));
    assert_eq!(a.store.saves(), 0);
    // The right one, typed on each, does.
    let done =
        s.a.ask(&op_line("pair_confirm", "entered", &s.code_b))
            .await;
    assert_eq!(done.pair(), "paired");
    assert_eq!(done.str("key_id"), b.key_id.as_str());
    assert!(
        a.trusts(&b.key_id) && !b.trusts(&a.key_id),
        "half a pairing so far"
    );
    let done =
        s.b.ask(&op_line("pair_confirm", "entered", &s.code_a))
            .await;
    assert_eq!(done.pair(), "paired");
    assert!(b.trusts(&a.key_id));
    assert!(s.a.closed().await && s.b.closed().await);
    // Pairing grants nothing: no grant was made.
    for n in [&a, &b] {
        assert!(
            n.daemon()
                .pipelines()
                .with_engine(|e| e.snapshot().grants.is_empty())
        );
    }
    a.shutdown().await;
    b.shutdown().await;
}

#[tokio::test]
async fn a_pairing_whose_save_fails_adds_no_key() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    let mut s = exchange(&a, &b).await;
    a.store.fail(true);
    let r =
        s.a.ask(&op_line("pair_confirm", "entered", &s.code_b))
            .await;
    assert_eq!(r.pair(), "not_saved");
    assert!(!a.trusts(&b.key_id));
    assert!(s.a.closed().await);
    assert_eq!(a.log.lines_with("pairing ended: not_saved"), 1);
    a.shutdown().await;
    b.shutdown().await;
}

#[tokio::test]
#[allow(non_snake_case)]
async fn AlreadyTrusted_is_its_own_outcome() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    pair_fully(&a, &b).await;
    let saves = a.store.saves();
    // The same two devices again: the reply that shows the code already says so.
    let mut s = exchange(&a, &b).await;
    let again =
        s.a.ask(&op_line("pair_confirm", "entered", &s.code_b))
            .await;
    assert_eq!(
        again.pair(),
        "already_trusted",
        "its own outcome, not `paired`"
    );
    assert!(!again.has("key_id"));
    assert_eq!(a.store.saves(), saves, "nothing was saved");
    assert!(s.a.closed().await);
    assert_eq!(a.log.lines_with("pairing ended: already_trusted"), 1);
    let listed = a
        .client()
        .await
        .ask("{\"oac_control\":1,\"op\":\"pairings\"}\n")
        .await;
    let body = listed.object.get("pairings").unwrap().as_object().unwrap();
    assert_eq!(
        body.get("keys").unwrap().as_array().unwrap().len(),
        1,
        "the key is listed once"
    );
    a.shutdown().await;
    b.shutdown().await;
}

/// The reply to the step that learns the peer's key already says it is trusted, before the
/// operator types a code.
#[tokio::test]
async fn the_code_reply_says_when_the_key_is_already_trusted() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    pair_fully(&a, &b).await;
    let mut cb = b.client().await;
    cb.ask(&pair_line("responder")).await;
    let mut ca = a.client().await;
    let offer = ca.ask(&pair_line("initiator")).await;
    let response = cb
        .ask(&op_line("pair_offer", "blob", &offer.str("blob")))
        .await;
    let coded = ca
        .ask(&op_line("pair_response", "blob", &response.str("blob")))
        .await;
    assert!(coded.flag("already_trusted"));
    a.shutdown().await;
    b.shutdown().await;
}

#[tokio::test]
async fn five_wrong_entries_abort() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    let mut s = exchange(&a, &b).await;
    let wrong = other_than(&s.code_b);
    for left in [4u32, 3, 2, 1] {
        let r = s.a.ask(&op_line("pair_confirm", "entered", &wrong)).await;
        assert_eq!(r.pair(), "mismatch");
        assert_eq!(
            r.object
                .get("attempts_left")
                .unwrap()
                .as_number()
                .unwrap()
                .raw(),
            left.to_string()
        );
    }
    let r = s.a.ask(&op_line("pair_confirm", "entered", &wrong)).await;
    assert_eq!(r.pair(), "aborted");
    assert!(s.a.closed().await, "the pairing is over");
    assert!(!a.trusts(&b.key_id));
    // The right code, typed now, on a new connection, pairs nothing: there is no pairing.
    let mut late = a.client().await;
    let r = late
        .ask(&op_line("pair_confirm", "entered", &s.code_b))
        .await;
    assert!(r.is_error("unknown_op"), "{}", r.text);
    assert!(late.closed().await);
    assert!(!a.trusts(&b.key_id));
    assert_eq!(a.log.lines_with("pairing ended: aborted"), 1);
    a.shutdown().await;
    b.shutdown().await;
}

#[tokio::test]
async fn a_second_pairing_is_busy() {
    let mut a = node("alpha").await;
    let mut first = a.client().await;
    assert_eq!(first.ask(&pair_line("responder")).await.pair(), "waiting");
    let mut second = a.client().await;
    let r = second.ask(&pair_line("initiator")).await;
    assert!(r.is_error("busy"), "{}", r.text);
    assert!(second.closed().await);
    // The first is untouched, and when it ends a new one is taken.
    let seen = *a.daemon().pairing().watch_events().borrow();
    drop(first);
    a.event_after(seen).await;
    let mut third = a.client().await;
    assert_eq!(third.ask(&pair_line("initiator")).await.pair(), "offer");
    a.shutdown().await;
}

// ---- the ends N1 requires -----------------------------------------------------------------

#[tokio::test]
async fn a_second_offer_ends_the_pairing() {
    let mut b = node("beta").await;
    let peer = fake("gamma");
    let now = b.clock.now();
    let mut c = b.client().await;
    c.ask(&pair_line("responder")).await;
    let (_init, offer) = PairingInitiator::start(&peer, now.clone());
    let first = c
        .ask(&op_line("pair_offer", "blob", &encode_offer(&offer)))
        .await;
    assert_eq!(first.pair(), "response");
    let (_other, offer2) = PairingInitiator::start(&peer, now);
    let r = c
        .ask(&op_line("pair_offer", "blob", &encode_offer(&offer2)))
        .await;
    assert_eq!(r.pair(), "ended");
    assert_eq!(r.str("end"), "second_offer");
    assert!(c.closed().await);
    assert_eq!(b.log.lines_with("pairing ended: second_offer"), 1);
    assert!(!b.trusts(&peer.key_id().clone()));
    b.shutdown().await;
}

#[tokio::test]
async fn a_second_response_ends_the_pairing() {
    let mut a = node("alpha").await;
    let peer = fake("gamma");
    let now = a.clock.now();
    let mut c = a.client().await;
    let offer = c.ask(&pair_line("initiator")).await;
    let Ok(Blob::Offer(offer)) = decode_blob(&offer.str("blob")) else {
        panic!("an offer")
    };
    let mut responder = ResponderPairing::start(now.clone());
    let response = responder.answer(&peer, offer, &now).unwrap();
    let coded = c
        .ask(&op_line(
            "pair_response",
            "blob",
            &encode_response(&response),
        ))
        .await;
    assert_eq!(coded.pair(), "code");
    // `Ni` is public now: a second response is ground-for-a-code material, and ends it.
    let (other, third) = (fake("delta"), fake("epsilon"));
    let mut responder2 = ResponderPairing::start(now.clone());
    let (_i, offer2) = PairingInitiator::start(&other, now.clone());
    let response2 = responder2.answer(&third, offer2, &now).unwrap();
    let r = c
        .ask(&op_line(
            "pair_response",
            "blob",
            &encode_response(&response2),
        ))
        .await;
    assert_eq!(r.pair(), "ended");
    assert_eq!(r.str("end"), "second_response");
    assert!(c.closed().await);
    assert!(!a.trusts(&peer.key_id().clone()));
    assert_eq!(a.log.lines_with("pairing ended: second_response"), 1);
    a.shutdown().await;
}

#[tokio::test]
async fn a_disconnect_abandons_the_pairing() {
    let mut b = node("beta").await;
    let peer = fake("gamma");
    let mut c = b.client().await;
    c.ask(&pair_line("responder")).await;
    let (_init, offer) = PairingInitiator::start(&peer, b.clock.now());
    c.ask(&op_line("pair_offer", "blob", &encode_offer(&offer)))
        .await;
    let seen = *b.daemon().pairing().watch_events().borrow();
    drop(c);
    b.event_after(seen).await;
    let events = b.daemon().pairing().recent_events();
    assert_eq!(
        events.last().unwrap().kind,
        EventKind::Ended(oac_cli::device_pairing::End::Abandoned)
    );
    assert_eq!(b.log.lines_with("pairing ended: abandoned"), 1);
    // The slot is free again.
    let mut again = b.client().await;
    assert_eq!(again.ask(&pair_line("responder")).await.pair(), "waiting");
    b.shutdown().await;
}

#[tokio::test]
async fn an_offer_carrying_this_devices_key_ends_the_pairing() {
    let mut b = node("beta").await;
    let mut c = b.client().await;
    c.ask(&pair_line("responder")).await;
    // An offer from "another device" that holds this device's own key.
    let (_init, offer) = PairingInitiator::start(b.daemon().pipelines().device(), b.clock.now());
    let r = c
        .ask(&op_line("pair_offer", "blob", &encode_offer(&offer)))
        .await;
    assert_eq!(r.pair(), "ended");
    assert_eq!(r.str("end"), "own_key");
    assert!(c.closed().await);
    b.shutdown().await;
}

#[tokio::test]
async fn an_initiator_refuses_a_response_carrying_its_own_key() {
    let mut a = node("alpha").await;
    let mut c = a.client().await;
    let offer = c.ask(&pair_line("initiator")).await;
    let Ok(Blob::Offer(offer)) = decode_blob(&offer.str("blob")) else {
        panic!("an offer")
    };
    let mut responder = ResponderPairing::start(a.clock.now());
    let response = responder
        .answer(&fake("gamma"), offer, &a.clock.now())
        .unwrap();
    let mut own = response.clone();
    own.public_key = *a.daemon().pipelines().device().public_key();
    let r = c
        .ask(&op_line("pair_response", "blob", &encode_response(&own)))
        .await;
    assert_eq!(
        (r.pair().as_str(), r.str("end").as_str()),
        ("ended", "own_key")
    );
    a.shutdown().await;
}

#[tokio::test]
async fn a_tampered_reveal_ends_the_pairing() {
    let mut b = node("beta").await;
    let peer = fake("gamma");
    let mut c = b.client().await;
    c.ask(&pair_line("responder")).await;
    let (_init, offer) = PairingInitiator::start(&peer, b.clock.now());
    let r = c
        .ask(&op_line("pair_offer", "blob", &encode_offer(&offer)))
        .await;
    assert_eq!(r.pair(), "response");
    let bad = encode_reveal(&PairingReveal { nonce: [9u8; 32] });
    let r = c.ask(&op_line("pair_reveal", "blob", &bad)).await;
    assert_eq!(r.pair(), "ended");
    assert_eq!(r.str("end"), "commitment_mismatch");
    assert!(c.closed().await);
    assert!(!b.trusts(&peer.key_id().clone()));
    b.shutdown().await;
}

/// A blob of the wrong kind, or the wrong op for the role, is diagnosed and changes nothing.
#[tokio::test]
async fn a_wrong_blob_or_op_is_diagnosed_without_a_state_change() {
    let mut b = node("beta").await;
    let peer = fake("gamma");
    let mut c = b.client().await;
    c.ask(&pair_line("responder")).await;
    let reveal = encode_reveal(&PairingReveal { nonce: [1u8; 32] });
    assert!(
        c.ask(&op_line("pair_offer", "blob", &reveal))
            .await
            .is_error("bad_blob")
    );
    assert!(
        c.ask(&op_line("pair_offer", "blob", "not a blob"))
            .await
            .is_error("bad_blob")
    );
    assert!(
        c.ask(&op_line("pair_response", "blob", &reveal))
            .await
            .is_error("wrong_role")
    );
    assert!(
        c.ask(&op_line("pair_confirm", "entered", "123456"))
            .await
            .is_error("no_code_yet")
    );
    // The pairing is as it was: a real offer is answered.
    let (_init, offer) = PairingInitiator::start(&peer, b.clock.now());
    let r = c
        .ask(&op_line("pair_offer", "blob", &encode_offer(&offer)))
        .await;
    assert_eq!(r.pair(), "response");
    b.shutdown().await;
}

/// An entry that is not six digits is refused with a diagnosis of its own and costs no try.
#[tokio::test]
async fn a_code_that_is_not_six_digits_is_refused_without_costing_a_try() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    let mut s = exchange(&a, &b).await;
    for bad in ["abc", "", "12345", "1234567", "12 456", "١٢٣٤٥٦"] {
        let r = s.a.ask(&op_line("pair_confirm", "entered", bad)).await;
        assert!(r.is_error("bad_code"), "{bad:?}: {}", r.text);
    }
    // Six tries of the wrong kind later, the first real wrong entry still has four left.
    let r =
        s.a.ask(&op_line("pair_confirm", "entered", &other_than(&s.code_b)))
            .await;
    assert_eq!(r.pair(), "mismatch");
    assert_eq!(
        r.object
            .get("attempts_left")
            .unwrap()
            .as_number()
            .unwrap()
            .raw(),
        "4"
    );
    let r =
        s.a.ask(&op_line("pair_confirm", "entered", &s.code_b))
            .await;
    assert_eq!(r.pair(), "paired");
    a.shutdown().await;
    b.shutdown().await;
}

/// A second reveal, and a reveal before any offer, end the pairing under names of the
/// module's own (the core reports them as a second offer and as abandoned).
#[tokio::test]
async fn a_second_reveal_and_a_reveal_before_any_offer_end_the_pairing() {
    let mut b = node("beta").await;
    let reveal = encode_reveal(&PairingReveal { nonce: [5u8; 32] });
    let mut c = b.client().await;
    c.ask(&pair_line("responder")).await;
    let r = c.ask(&op_line("pair_reveal", "blob", &reveal)).await;
    assert_eq!(
        (r.pair().as_str(), r.str("end").as_str()),
        ("ended", "reveal_without_offer")
    );
    assert!(c.closed().await);

    let peer = fake("gamma");
    let now = b.clock.now();
    let mut c = b.client().await;
    c.ask(&pair_line("responder")).await;
    let (init, offer) = PairingInitiator::start(&peer, now.clone());
    let response = c
        .ask(&op_line("pair_offer", "blob", &encode_offer(&offer)))
        .await;
    let Ok(Blob::Response(response)) = decode_blob(&response.str("blob")) else {
        panic!("a response")
    };
    let (_session, revealed) = init.receive(response, &now).unwrap();
    let revealed = encode_reveal(&revealed);
    let coded = c.ask(&op_line("pair_reveal", "blob", &revealed)).await;
    assert_eq!(coded.pair(), "code");
    let r = c.ask(&op_line("pair_reveal", "blob", &revealed)).await;
    assert_eq!(
        (r.pair().as_str(), r.str("end").as_str()),
        ("ended", "second_reveal")
    );
    assert!(c.closed().await);
    assert!(!b.trusts(&peer.key_id().clone()));
    b.shutdown().await;
}

/// A line the daemon cannot read as a request ends the pairing: the framing cannot be
/// trusted after it.
#[tokio::test]
async fn an_unreadable_line_ends_the_pairing() {
    let mut b = node("beta").await;
    let mut c = b.client().await;
    c.ask(&pair_line("responder")).await;
    let r = c.ask("not json\n").await;
    assert!(r.is_error("malformed"));
    let r = c.reply().await;
    assert_eq!(
        (r.pair().as_str(), r.str("end").as_str()),
        ("ended", "bad_line")
    );
    assert!(c.closed().await);
    // A line over the pairing bound.
    let mut c = b.client().await;
    c.ask(&pair_line("responder")).await;
    let long = format!("{}\n", "x".repeat(MAX_LINE + 1));
    let r = c.ask(&long).await;
    assert!(r.is_error("too_large"));
    b.shutdown().await;
}

// ---- the exchange itself ------------------------------------------------------------------

/// The reply to `pair` holds the commitment and never `Ni`; `Ni` appears in the reveal only,
/// after the response, and it opens that commitment.
#[tokio::test]
#[allow(non_snake_case)]
async fn the_offer_reply_carries_no_Ni() {
    let mut a = node("alpha").await;
    let peer = fake("gamma");
    let mut c = a.client().await;
    let offer_reply = c.ask(&pair_line("initiator")).await;
    let blob = offer_reply.str("blob");
    let Ok(Blob::Offer(offer)) = decode_blob(&blob) else {
        panic!("an offer")
    };
    assert_ne!(offer.commitment, [0u8; 32]);
    // The decoded object has no nonce member at all.
    let octets = decode_text(&blob);
    assert!(!octets.contains("nonce"), "{octets}");
    assert!(
        c.ask(&op_line(
            "pair_reveal",
            "blob",
            &encode_reveal(&PairingReveal { nonce: [3; 32] })
        ))
        .await
        .is_error("wrong_role")
    );
    // The response arrives; only now does a reveal exist.
    let now = a.clock.now();
    let mut responder = ResponderPairing::start(now.clone());
    let response = responder.answer(&peer, offer, &now).unwrap();
    let coded = c
        .ask(&op_line(
            "pair_response",
            "blob",
            &encode_response(&response),
        ))
        .await;
    let reveal_blob = coded.str("blob");
    let Ok(Blob::Reveal(reveal)) = decode_blob(&reveal_blob) else {
        panic!("a reveal")
    };
    let ni = oac_cli_b64(&reveal.nonce);
    assert!(!offer_reply.text.contains(&ni), "Ni was in the offer reply");
    assert!(!blob.contains(&ni));
    // It opens the commitment: the responder's core accepts it.
    let session = responder
        .reveal(reveal, &now)
        .expect("Ni matches the commitment");
    assert_eq!(session.code(), coded.str("code"));
    a.shutdown().await;
}

/// The test's own copy of the unpadded base64url of 32 octets, so that it does not lean on the
/// code it checks.
fn oac_cli_b64(octets: &[u8; 32]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in octets.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |a, (k, &b)| a | (u32::from(b) << (16 - 8 * k)));
        for k in 0..=chunk.len() {
            out.push(A[((n >> (18 - 6 * k)) & 63) as usize] as char);
        }
    }
    out
}

/// The canonical JSON a blob carries, as text.
fn decode_text(blob: &str) -> String {
    let rest = blob.strip_prefix(BLOB_PREFIX).unwrap();
    // Reuse the library's inverse by re-encoding what it decodes: the JSON text is what the
    // base64url spells, so decode it with the test's own table.
    const A: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut bits = 0u32;
    let mut n = 0;
    let mut out = Vec::new();
    for c in rest.chars() {
        bits = (bits << 6) | A.find(c).unwrap() as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
            bits &= (1 << n) - 1;
        }
    }
    String::from_utf8(out).unwrap()
}

/// A statement removed on the way gives two codes that differ, and the entry fails.
#[tokio::test]
async fn a_stripped_statement_gives_codes_that_differ() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    let mut cb = b.client().await;
    cb.ask(&pair_line("responder")).await;
    let mut ca = a.client().await;
    let offer = ca.ask(&pair_line("initiator")).await;
    let response = cb
        .ask(&op_line("pair_offer", "blob", &offer.str("blob")))
        .await;
    let Ok(Blob::Response(mut response)) = decode_blob(&response.str("blob")) else {
        panic!("a response")
    };
    assert!(
        response.agreement_statement.is_some(),
        "the daemon gives its statement"
    );
    response.agreement_statement = None;
    let coded_a = ca
        .ask(&op_line(
            "pair_response",
            "blob",
            &encode_response(&response),
        ))
        .await;
    assert_eq!(
        coded_a
            .object
            .get("peer_statement")
            .unwrap()
            .as_object()
            .unwrap()
            .get("state")
            .unwrap()
            .as_str(),
        Some("none")
    );
    let coded_b = cb
        .ask(&op_line("pair_reveal", "blob", &coded_a.str("blob")))
        .await;
    assert_ne!(coded_a.str("code"), coded_b.str("code"), "the codes differ");
    let r = ca
        .ask(&op_line("pair_confirm", "entered", &coded_b.str("code")))
        .await;
    assert_eq!(r.pair(), "mismatch");
    assert!(!a.trusts(&b.key_id));
    a.shutdown().await;
    b.shutdown().await;
}

/// T1: a key swapped in a pasted blob gives codes that differ.
#[tokio::test]
async fn a_substituted_key_in_the_pasted_blob_gives_codes_that_differ() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    let mut cb = b.client().await;
    cb.ask(&pair_line("responder")).await;
    let mut ca = a.client().await;
    let offer = ca.ask(&pair_line("initiator")).await;
    let response = cb
        .ask(&op_line("pair_offer", "blob", &offer.str("blob")))
        .await;
    let Ok(Blob::Response(mut response)) = decode_blob(&response.str("blob")) else {
        panic!("a response")
    };
    // A man in the middle puts its own key in the response.
    response.public_key = *fake("mallory").public_key();
    let coded_a = ca
        .ask(&op_line(
            "pair_response",
            "blob",
            &encode_response(&response),
        ))
        .await;
    let coded_b = cb
        .ask(&op_line("pair_reveal", "blob", &coded_a.str("blob")))
        .await;
    assert_ne!(coded_a.str("code"), coded_b.str("code"));
    a.shutdown().await;
    b.shutdown().await;
}

// ---- deadlines ----------------------------------------------------------------------------

/// The 120 s deadline on the tokio clock. The clock is paused only once the connection is
/// open, so that the only read in flight is the one that waits for the deadline.
#[tokio::test]
async fn the_pairing_expires_at_120_seconds() {
    let mut b = node_on_tokio_clock("beta").await;
    let mut c = b.client().await;
    assert_eq!(c.ask(&pair_line("responder")).await.pair(), "waiting");
    c.wait = Duration::from_secs(600);
    tokio::time::pause();
    let began = tokio::time::Instant::now();
    let r = c.reply().await;
    let elapsed = tokio::time::Instant::now() - began;
    assert_eq!(r.pair(), "expired");
    assert!(elapsed >= Duration::from_secs(120), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(125), "{elapsed:?}");
    assert!(c.closed().await);
    assert_eq!(b.log.lines_with("pairing ended: expired"), 1);
    b.shutdown().await;
}

/// The 2 s deadline is the first line's only: a connection that sends none is closed at 2 s,
/// and a pairing waits far longer than 2 s for its next line.
#[tokio::test]
async fn the_two_second_deadline_applies_to_the_first_line_only() {
    let mut b = node_on_tokio_clock("beta").await;
    // No first line: closed at 2 s.
    let mut idle = b.client().await;
    let mut c = b.client().await;
    assert_eq!(c.ask(&pair_line("responder")).await.pair(), "waiting");
    let peer = fake("gamma");
    let (_init, offer) = PairingInitiator::start(&peer, b.clock.now());
    idle.wait = Duration::from_secs(60);
    c.wait = Duration::from_secs(600);
    tokio::time::pause();
    let began = tokio::time::Instant::now();
    assert!(idle.closed().await, "closed with no request");
    let elapsed = tokio::time::Instant::now() - began;
    // The 2 s began at the connection, in real time, a moment before the pause: no lower
    // bound on what is left of it, and the timer rounds up to a millisecond or more.
    assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
    // The pairing has been open longer than that and goes on.
    tokio::time::advance(Duration::from_secs(30)).await;
    let r = c
        .ask(&op_line("pair_offer", "blob", &encode_offer(&offer)))
        .await;
    assert_eq!(r.pair(), "response", "{}", r.text);
    b.shutdown().await;
}

/// The core's own 120 s, on its own clock, fails closed too.
#[tokio::test]
async fn the_core_clock_ends_the_pairing_at_120_seconds() {
    let mut b = node("beta").await;
    let peer = fake("gamma");
    let (_init, offer) = PairingInitiator::start(&peer, b.clock.now());
    let mut c = b.client().await;
    c.ask(&pair_line("responder")).await;
    b.clock.advance_nanos(120_000_000_000);
    let r = c
        .ask(&op_line("pair_offer", "blob", &encode_offer(&offer)))
        .await;
    assert_eq!(r.pair(), "expired");
    assert!(c.closed().await);
    assert!(!b.trusts(&peer.key_id().clone()));
    b.shutdown().await;
}

// ---- unpair -------------------------------------------------------------------------------

fn paired_key(node: &Node, principal: &str) -> KeyId {
    let peer = fake(principal);
    let p = PairedPeer::by_key_id_comparison(
        peer.principal().clone(),
        *peer.public_key(),
        peer.key_id(),
        node.clock.now(),
        OperatorConfirmed::by_operator(),
    )
    .unwrap();
    node.daemon()
        .pipelines()
        .pair(p, &node.store)
        .expect("paired");
    peer.key_id().clone()
}

/// N5, with the bounded backoff on tokio's paused clock: an unpair whose save fails reports
/// `durable:false` (the verb exits 1 on it), the daemon retries at 1 s, 2 s, 4 s ... up to
/// 60 s, and the first retry that succeeds clears it.
#[tokio::test]
async fn an_unpair_whose_save_fails_exits_1_and_is_retried() {
    let mut a = node_on_tokio_clock("alpha").await;
    let key = paired_key(&a, "gamma");
    a.store.fail(true);
    tokio::time::pause();
    let result = a.daemon().pairing().clone().unpair(&key).await;
    assert!(matches!(result, UnpairResult::NotDurable(_)), "{result:?}");
    assert!(!a.trusts(&key), "the removal stands in memory");
    let listed = Reply::parse(a.daemon().pairing().pairings_reply());
    let body = listed.object.get("pairings").unwrap().as_object().unwrap();
    assert_eq!(body.get("saved").unwrap().as_bool(), Some(false));
    assert_eq!(a.log.lines_with("is NOT saved"), 1);

    // Retries while the store keeps failing: the gaps double, then stay at the cap.
    let mut rx = a.store.changed.subscribe();
    let first_failure = a.store.attempts() - 1; // the pairing's own save came before it
    let wanted = first_failure + 1 + 8; // the failed unpair, then eight retries
    tokio::time::timeout(
        Duration::from_secs(3600),
        rx.wait_for(|_| a.store.attempts() >= wanted),
    )
    .await
    .unwrap()
    .unwrap();
    let times: Vec<_> = a
        .store
        .attempts
        .lock()
        .unwrap()
        .iter()
        .skip(first_failure)
        .map(|(t, _)| *t)
        .collect();
    let gaps: Vec<u64> = times.windows(2).map(|w| (w[1] - w[0]).as_secs()).collect();
    assert_eq!(&gaps[..8], &[1, 2, 4, 8, 16, 32, 60, 60], "{gaps:?}");

    // The store recovers; the next retry saves, and nothing is unsaved after.
    a.store.fail(false);
    let saves_before = a.store.saves();
    tokio::time::timeout(
        Duration::from_secs(3600),
        rx.wait_for(|_| a.store.saves() > saves_before),
    )
    .await
    .unwrap()
    .unwrap();
    // The retry task ends once nothing is unsaved: wait for that (closing waits for it).
    a.daemon().pairing().close().await;
    let listed = Reply::parse(a.daemon().pairing().pairings_reply());
    let body = listed.object.get("pairings").unwrap().as_object().unwrap();
    assert_eq!(body.get("saved").unwrap().as_bool(), Some(true));
    assert!(a.daemon().pipelines().next_trust_retry_in().is_none());
    assert_eq!(a.log.lines_with("the trust store was saved again"), 1);
    let kinds: Vec<_> = a
        .daemon()
        .pairing()
        .recent_events()
        .iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, [EventKind::Unpaired]);
    a.shutdown().await;
}

/// The op answers each outcome on one line.
#[tokio::test]
async fn the_unpair_op_reports_each_outcome() {
    let mut a = node("alpha").await;
    let key = paired_key(&a, "gamma");
    let ask = |key: &KeyId| op_line("unpair", "key_id", key.as_str());
    // Not trusted, and this device's own key.
    let r = a
        .client()
        .await
        .ask(&ask(&KeyId::parse(&"a".repeat(64)).unwrap()))
        .await;
    assert!(r.is_error("not_trusted"), "{}", r.text);
    let r = a.client().await.ask(&ask(&a.key_id)).await;
    assert!(r.is_error("own_key"), "{}", r.text);
    // A key id that is not one is malformed (the first line's own check).
    let r = a
        .client()
        .await
        .ask(&op_line("unpair", "key_id", "abc"))
        .await;
    assert!(r.is_error("malformed"));
    // The store fails: removed, not durable.
    a.store.fail(true);
    let r = a.client().await.ask(&ask(&key)).await;
    assert!(r.flag("unpaired") && !r.flag("durable"), "{}", r.text);
    assert!(!a.trusts(&key));
    // A durable one.
    a.store.fail(false);
    let other = paired_key(&a, "delta");
    let r = a.client().await.ask(&ask(&other)).await;
    assert!(r.flag("unpaired") && r.flag("durable"), "{}", r.text);
    a.shutdown().await;
}

// ---- shutdown -----------------------------------------------------------------------------

#[tokio::test]
async fn stop_ends_an_open_pairing() {
    let mut b = node("beta").await;
    let mut pairing = b.client().await;
    assert_eq!(pairing.ask(&pair_line("responder")).await.pair(), "waiting");
    let requested = b.daemon().stop_requested();
    let mut stopper = b.client().await;
    let r = stopper.ask(STOP_REQUEST).await;
    assert!(r.flag("stopping"));
    requested.await;
    // The owner's shutdown does not wait on the pairing's 120 s: the pairing ends at once.
    tokio::time::timeout(Duration::from_secs(10), b.shutdown())
        .await
        .expect("the shutdown did not wait for the pairing's deadline");
    let r = pairing.reply().await;
    assert_eq!(
        (r.pair().as_str(), r.str("end").as_str()),
        ("ended", "shutdown")
    );
    assert!(pairing.closed().await);
    assert_eq!(b.log.lines_with("pairing ended: shutdown"), 1);
}

/// What `DevicePairing::close` adds to the core's own `closed` flag: when the daemon's
/// shutdown returns, the pairing ops are closed and none of their tasks is left running. With
/// a save retry armed (the one task here that holds no core lock), the clock then moves past
/// several retry instants and the store is not touched again.
#[tokio::test]
async fn shutdown_closes_the_pairing_ops_and_ends_every_task() {
    let mut a = node_on_tokio_clock("alpha").await;
    let key = paired_key(&a, "gamma");
    a.store.fail(true);
    tokio::time::pause();
    let pairing = a.daemon().pairing().clone();
    assert!(matches!(
        pairing.unpair(&key).await,
        UnpairResult::NotDurable(_)
    ));
    assert_eq!(pairing.running_tasks(), 1, "the save retry is armed");
    assert!(!pairing.is_closed());
    let store = a.store.clone();
    a.shutdown().await;
    assert!(pairing.is_closed(), "the shutdown closed the pairing ops");
    assert_eq!(pairing.running_tasks(), 0, "no task outlives the shutdown");
    let attempts = store.attempts();
    // Past several retry instants (1, 2, 4, 8, 16, 32 s ... would all have passed).
    tokio::time::advance(Duration::from_secs(300)).await;
    assert_eq!(store.attempts(), attempts, "nothing wrote the store");
    assert_eq!(pairing.unpair(&key).await, UnpairResult::Closed);
    assert_eq!(pairing.running_tasks(), 0);
}

/// A confirmed pairing is writing the store when the stop arrives: the shutdown waits for the
/// write, which is the last; and a pairing that has not confirmed by then writes nothing.
#[tokio::test]
async fn no_store_write_follows_the_shutdown() {
    // (1) A save in flight when the shutdown begins finishes before the shutdown does. That
    // ordering is the core's: its lock is held by the save, and `Pipelines::shutdown` takes
    // it. What `DevicePairing::close` adds (no task left, none started) is tested by
    // `shutdown_closes_the_pairing_ops_and_ends_every_task`, which fails without it.
    let a = node("alpha").await;
    let peer = fake("gamma");
    let now = a.clock.now();
    let mut c = a.client().await;
    let offer = c.ask(&pair_line("initiator")).await;
    let Ok(Blob::Offer(offer)) = decode_blob(&offer.str("blob")) else {
        panic!("an offer")
    };
    let mut responder = ResponderPairing::start(now.clone());
    let response = responder.answer(&peer, offer, &now).unwrap();
    let coded = c
        .ask(&op_line(
            "pair_response",
            "blob",
            &encode_response(&response),
        ))
        .await;
    a.store.gate.arm();
    c.send(&op_line("pair_confirm", "entered", &coded.str("code")))
        .await;
    a.store.gate.entered.notified().await;
    let store = a.store.clone();
    let events = store.events.clone();
    let daemon = a.daemon.unwrap();
    let shutdown = tokio::spawn(async move { daemon.shutdown().await });
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
    assert!(
        !shutdown.is_finished(),
        "the shutdown waits for the write in flight"
    );
    assert!(
        events.lock().unwrap().is_empty(),
        "the write has not finished"
    );
    store.gate.release();
    shutdown.await.unwrap();
    events.lock().unwrap().push("shutdown".to_owned());
    // The pairing's save completed, then the shutdown; nothing wrote after it.
    assert_eq!(*events.lock().unwrap(), ["save", "shutdown"]);
    let saved_at_shutdown = store.saves();
    assert_eq!(saved_at_shutdown, 1);

    // (2) A pairing that has its code but has not confirmed when the stop arrives writes
    // nothing: it ends at once, and a confirmation sent after that has no pairing to reach.
    let mut b = node("beta").await;
    let mut c = b.client().await;
    let offer = c.ask(&pair_line("initiator")).await;
    let Ok(Blob::Offer(offer)) = decode_blob(&offer.str("blob")) else {
        panic!("an offer")
    };
    let mut responder = ResponderPairing::start(b.clock.now());
    let response = responder.answer(&peer, offer, &b.clock.now()).unwrap();
    let coded = c
        .ask(&op_line(
            "pair_response",
            "blob",
            &encode_response(&response),
        ))
        .await;
    let store = b.store.clone();
    b.shutdown().await;
    let _ = c
        .stream
        .write_all(op_line("pair_confirm", "entered", &coded.str("code")).as_bytes())
        .await;
    let mut ended = None;
    while let Some(line) = c.line().await {
        ended = Some(Reply::parse(line));
    }
    let ended = ended.expect("told the pairing ended");
    assert_eq!(ended.str("end"), "shutdown", "{}", ended.text);
    assert_eq!(store.attempts(), 0, "nothing was written");
}

// ---- visibility ---------------------------------------------------------------------------

#[tokio::test]
async fn every_pairing_and_unpair_leaves_one_log_line() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    pair_fully(&a, &b).await;
    assert_eq!(a.log.lines_with("pairing started (initiator)"), 1);
    assert_eq!(b.log.lines_with("pairing started (responder)"), 1);
    let (paired_b, paired_a) = (
        format!("oac-daemon: paired key {}", b.key_id),
        format!("oac-daemon: paired key {}", a.key_id),
    );
    assert_eq!(a.log.lines_with(&paired_b), 1);
    assert_eq!(b.log.lines_with(&paired_a), 1);
    assert_eq!(
        a.log.lines_with("pairing ended"),
        0,
        "a pairing that paired ended no other way"
    );
    let r = a
        .client()
        .await
        .ask(&op_line("unpair", "key_id", b.key_id.as_str()))
        .await;
    assert!(r.flag("durable"));
    assert_eq!(a.log.lines_with(&format!("unpaired key {}", b.key_id)), 1);
    // An end leaves its line.
    let mut c = a.client().await;
    c.ask(&pair_line("responder")).await;
    let seen = *a.daemon().pairing().watch_events().borrow();
    drop(c);
    a.event_after(seen).await;
    assert_eq!(a.log.lines_with("pairing ended: abandoned"), 1);
    // The events the log mirrors, in order.
    let kinds: Vec<_> = a
        .daemon()
        .pairing()
        .recent_events()
        .iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            EventKind::Paired,
            EventKind::Unpaired,
            EventKind::Ended(oac_cli::device_pairing::End::Abandoned)
        ]
    );
    a.shutdown().await;
    b.shutdown().await;
}

/// The log holds no blob, nonce, code or statement of any pairing: an honest one, one that
/// mismatches, and one that ends.
#[tokio::test]
async fn no_log_line_holds_a_blob_nonce_code_or_statement() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    let mut s = exchange(&a, &b).await;
    let r =
        s.a.ask(&op_line("pair_confirm", "entered", &other_than(&s.code_b)))
            .await;
    assert_eq!(r.pair(), "mismatch");
    s.a.ask(&op_line("pair_confirm", "entered", &s.code_b))
        .await;
    s.b.ask(&op_line("pair_confirm", "entered", &s.code_a))
        .await;
    let mut secrets = secrets_of(&s);
    secrets.push(other_than(&s.code_b));
    // A pairing the store did not save, between two more devices: the line that carries the
    // store's error text.
    let (mut e, mut f) = (node("epsilon").await, node("zeta").await);
    let mut t = exchange(&e, &f).await;
    e.store.fail(true);
    let r =
        t.a.ask(&op_line("pair_confirm", "entered", &t.code_b))
            .await;
    assert_eq!(r.pair(), "not_saved");
    assert_eq!(e.log.lines_with("the pairing was not saved"), 1);
    secrets.extend(secrets_of(&t));
    // And a pairing that ends.
    let mut c = b.client().await;
    c.ask(&pair_line("responder")).await;
    c.ask("not json\n").await;
    for node in [&a, &b, &e, &f] {
        for line in node.log.text().lines() {
            // A key id is public, and 64 hexadecimal digits can hold any six decimal ones.
            let line: String = line
                .split(' ')
                .filter(|w| !(w.len() == 64 && w.bytes().all(|c| c.is_ascii_hexdigit())))
                .collect::<Vec<_>>()
                .join(" ");
            for secret in &secrets {
                assert!(!line.contains(secret.as_str()), "{line:?} holds {secret:?}");
            }
            assert!(!line.contains("statement") || !line.contains("agreement_key"));
        }
    }
    a.shutdown().await;
    b.shutdown().await;
    e.shutdown().await;
    f.shutdown().await;
}

/// What no log line may hold, of one exchange: the codes, the blobs, both nonces, the
/// commitment, and the values of both devices' statements (the initiator's from its offer, the
/// responder's from its response).
fn secrets_of(s: &Shown) -> Vec<String> {
    let mut secrets = vec![
        s.code_a.clone(),
        s.code_b.clone(),
        BLOB_PREFIX.to_owned(),
        s.response_blob.clone(),
        s.reveal_blob.clone(),
    ];
    let mut values = |st: &oac_core::sealing::AgreementStatement| {
        for (_, v) in st.as_json().iter() {
            if let Some(text) = v.as_str() {
                secrets.push(text.to_owned());
            }
        }
    };
    let offer = Reply::parse(s.offer_reply.clone()).str("blob");
    let Ok(Blob::Offer(offer)) = decode_blob(&offer) else {
        panic!("an offer")
    };
    values(
        &offer
            .agreement_statement
            .expect("the initiator's statement"),
    );
    let Ok(Blob::Response(response)) = decode_blob(&s.response_blob) else {
        panic!("a response")
    };
    values(
        &response
            .agreement_statement
            .expect("the responder's statement"),
    );
    secrets.push(oac_cli_b64(&offer.commitment));
    secrets.push(oac_cli_b64(&response.nonce));
    let Ok(Blob::Reveal(reveal)) = decode_blob(&s.reveal_blob) else {
        panic!("a reveal")
    };
    secrets.push(oac_cli_b64(&reveal.nonce));
    secrets
}

// ---- size ---------------------------------------------------------------------------------

/// The worst case: both principals at the 128 octets an identifier token allows, and a
/// statement in each blob. Measured, and reported with `--nocapture`.
#[tokio::test]
async fn the_worst_case_blob_with_128_octet_principals_fits_a_control_line() {
    let long_a = "a".repeat(128);
    let long_b = format!("b{}", "-".repeat(127));
    let (mut a, mut b) = (node(&long_a).await, node(&long_b).await);
    let mut s = exchange(&a, &b).await;
    let offer = Reply::parse(s.offer_reply.clone()).str("blob");
    let sizes = [
        ("offer", offer.len()),
        ("response", s.response_blob.len()),
        ("reveal", s.reveal_blob.len()),
    ];
    let wrapped_lines = |n: usize| n.div_ceil(64);
    for (name, len) in sizes {
        eprintln!(
            "worst case {name} blob: {len} bytes ({} lines of 64 characters)",
            wrapped_lines(len)
        );
    }
    eprintln!(
        "worst case control line sent by the client: {} bytes; longest reply read: {} bytes (the bound is {MAX_LINE})",
        s.a.longest_sent.max(s.b.longest_sent),
        s.a.longest_received.max(s.b.longest_received)
    );
    let longest_blob = sizes.iter().map(|(_, l)| *l).max().unwrap();
    assert!(longest_blob < MAX_LINE, "{longest_blob}");
    assert!(
        s.a.longest_sent.max(s.b.longest_sent) < MAX_LINE,
        "a request line fits"
    );
    assert!(s.a.longest_received.max(s.b.longest_received) < MAX_LINE);
    // The first line (`pair`) keeps the 1 KiB bound with room to spare.
    assert!(pair_line("initiator").len() < 1024);
    // And the pairing completes with those principals.
    s.a.ask(&op_line("pair_confirm", "entered", &s.code_b))
        .await;
    s.b.ask(&op_line("pair_confirm", "entered", &s.code_a))
        .await;
    assert!(a.trusts(&b.key_id) && b.trusts(&a.key_id));
    a.shutdown().await;
    b.shutdown().await;
}

// ---- the endpoint around them -------------------------------------------------------------

/// `status` and `stop` still answer on their one-line, 1 KiB connections beside a pairing.
#[tokio::test]
async fn status_still_answers_beside_an_open_pairing() {
    let mut b = node("beta").await;
    let mut pairing = b.client().await;
    pairing.ask(&pair_line("responder")).await;
    let r = b.client().await.ask(STATUS_REQUEST).await;
    assert!(r.has("status"), "{}", r.text);
    let r = b
        .client()
        .await
        .ask("{\"oac_control\":1,\"op\":\"pairings\"}\n")
        .await;
    assert!(r.has("pairings"));
    b.shutdown().await;
}

#[tokio::test]
async fn pairings_lists_keys_and_events() {
    let (mut a, mut b) = (node("alpha").await, node("beta").await);
    pair_fully(&a, &b).await;
    let r = a
        .client()
        .await
        .ask("{\"oac_control\":1,\"op\":\"pairings\"}\n")
        .await;
    let body = r.object.get("pairings").unwrap().as_object().unwrap();
    assert_eq!(body.get("saved").unwrap().as_bool(), Some(true));
    let keys = body.get("keys").unwrap().as_array().unwrap();
    assert_eq!(keys.len(), 1);
    let key = keys[0].as_object().unwrap();
    assert_eq!(key.get("key_id").unwrap().as_str(), Some(b.key_id.as_str()));
    assert!(
        key.get("confirmed").is_some(),
        "the confirmed agreement values are listed"
    );
    let events = body.get("events").unwrap().as_array().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].as_object().unwrap().get("kind").unwrap().as_str(),
        Some("paired")
    );
    a.shutdown().await;
    b.shutdown().await;
}
