// SPDX-License-Identifier: Apache-2.0

//! The daemon serves the `codex` endpoint and opens the Codex carrier (G8 slice S5, #433
//! boxes 1-2, #69 box 2; `docs/planning/v0.1/11-risks.md` row 90), with real processes.
//!
//! **What is real.** The daemon runs in this process over the in-memory transport, with the
//! real `CodexAdapter` and the real carrier driver (`oac_cli::codex::CarrierDriver`). Each
//! "Codex" is a copy of this test binary named `codex` (a harness process to the operating
//! system, with a working directory of its own) that starts real `oac mcp-shim --harness codex`
//! processes, as Codex's app-server does for its MCP servers, and relays their stdio to the
//! test. The app-server is the node fake (`tests/fakes/codex-app-server/`, F9) over loopback
//! TCP: the carrier's *opener* is given (TCP to the fake, with a synthetic observation of the
//! server computed with `peer::observe_process` on the `codex` process), because the real
//! opener needs a real Codex. The real opener is tested against a real `AF_UNIX` socket in
//! `codex_socket.rs`.
//!
//! CI-default tier: no Codex install, no credential, no network beyond loopback. Every wait is
//! bounded, and every child is killed when its guard drops (#435).

#[path = "support/exec_copy.rs"]
mod exec_copy;

use exec_copy::SpawnRetrying;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oac_adapter_codex::client::CLIENT_NAME;
use oac_adapter_codex::{CarrierState, CodexAdapter};
use oac_cli::codex::{CarrierOpener, CarrierWiring, OpenError, OpenedCarrier, blocking_stream};
use oac_cli::daemon::{Daemon, Diagnostic, LogSink, Parts};
use oac_cli::ipc::Scope;
use oac_cli::peer::{self, ProcessId, SystemTable};
use oac_core::authorization::{Grant, LocalSide, OperatorConfirmed, PeerSide};
use oac_core::clock::{Clock, SystemClock};
use oac_core::ids::{SessionId, Token};
use oac_core::json::{self, Json};
use oac_core::keys::MemoryKeyStore;
use oac_core::pairing::MemoryPairingStore;
use oac_core::pipeline::PipelineConfig;
use oac_core::sealing::MemoryAgreementKeyStore;
use oac_core::session_binding::PeerObservation;
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};
use tokio::runtime::Handle;

const ROLE: &str = "OAC_CODEX_TEST_ROLE";
/// The test the `codex` host process re-runs; it dispatches on the role before anything else.
const HOST_TEST: &str = "a_codex_shim_without_a_paired_carrier_gets_a_plain_refusal";
const WAIT: Duration = Duration::from_secs(25);
/// Long enough for the daemon's one-second timer to have fired at least twice.
const QUIET: Duration = Duration::from_millis(2600);
const REGISTERED: &str = "oac";

fn oac() -> &'static str {
    env!("CARGO_BIN_EXE_oac")
}

fn get<'a>(v: &'a Json, name: &str) -> Option<&'a Json> {
    v.as_object()?.get(name)
}

fn get_str<'a>(v: &'a Json, name: &str) -> Option<&'a str> {
    get(v, name)?.as_str()
}

fn js(s: &str) -> String {
    Json::String(s.to_owned()).to_compact()
}

fn eventually<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let end = Instant::now() + WAIT;
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < end, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

// ---- the daemon's runtime ---------------------------------------------------------------

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
        rx.recv_timeout(WAIT).expect("the runtime answered")
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

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

struct DumpOnPanic(Captured);

impl Drop for DumpOnPanic {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("--- daemon log ---\n{}", self.0.text());
        }
    }
}

struct NamespaceDir(#[cfg(unix)] Option<PathBuf>);

impl Drop for NamespaceDir {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(dir) = self.0.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

// ---- the fake app-server ----------------------------------------------------------------

type Waiters = Arc<Mutex<HashMap<u64, mpsc::Sender<Result<Json, Json>>>>>;

/// The node fake, killed when dropped (#435), with its control connection.
struct Fake {
    child: Child,
    stdin: Mutex<ChildStdin>,
    host: String,
    waiters: Waiters,
    next: std::sync::atomic::AtomicU64,
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Fake {
    fn spawn() -> Fake {
        let server = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fakes/codex-app-server/server.mjs");
        let mut child = Command::new("node")
            .arg(server)
            .args(["--stdio", "--listen", "ws://127.0.0.1:0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn the fake Codex app-server with node");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (url_tx, url_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut sent = false;
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if !sent
                    && let Ok(v) = json::parse(line.as_bytes())
                    && let Some(u) = get_str(&v, "url")
                {
                    let _ = url_tx.send(u.to_owned());
                    sent = true;
                    continue;
                }
                eprintln!("fake codex: {line}");
            }
        });
        let waiters: Waiters = Arc::default();
        let w = waiters.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Ok(v) = json::parse(line.as_bytes()) else {
                    continue;
                };
                let Some(id) = get(&v, "id")
                    .and_then(Json::as_number)
                    .and_then(|n| n.plain_integer_in(0, u64::MAX))
                else {
                    continue;
                };
                let reply = match (get(&v, "result"), get(&v, "error")) {
                    (Some(r), _) => Ok(r.clone()),
                    (_, Some(e)) => Err(e.clone()),
                    _ => continue,
                };
                if let Some(tx) = w.lock().unwrap().remove(&id) {
                    let _ = tx.send(reply);
                }
            }
        });
        let url = url_rx
            .recv_timeout(Duration::from_secs(60))
            .expect("the fake reports its listener");
        let host = url
            .strip_prefix("ws://")
            .unwrap()
            .trim_end_matches('/')
            .to_owned();
        let fake = Fake {
            child,
            stdin: Mutex::new(stdin),
            host,
            waiters,
            next: std::sync::atomic::AtomicU64::new(1),
        };
        fake.call(
            "initialize",
            "{\"clientInfo\":{\"name\":\"oac-s5-test-tui\",\"version\":\"0\"},\"capabilities\":{\"experimentalApi\":true}}",
        )
        .expect("initialize the TUI connection");
        fake
    }

    fn call(&self, method: &str, params: &str) -> Result<Json, Json> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.waiters.lock().unwrap().insert(id, tx);
        let frame = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":{},\"params\":{params}}}\n",
            js(method)
        );
        self.stdin
            .lock()
            .unwrap()
            .write_all(frame.as_bytes())
            .unwrap();
        rx.recv_timeout(Duration::from_secs(10))
            .expect("an answer from the fake")
    }

    fn control(&self, method: &str, params: &str) -> Json {
        self.call(method, params)
            .unwrap_or_else(|e| panic!("{method}: {}", e.to_compact()))
    }

    /// A thread whose turn is running (the TUI user's turn), so that a tool call can be made.
    fn busy_thread(&self) -> String {
        let r = self.control("oacFake/thread/create", "{\"cwd\":\"/oac-s5\"}");
        let t = get_str(&r, "threadId").unwrap().to_owned();
        self.call(
            "turn/start",
            &format!(
                "{{\"threadId\":{},\"input\":[{{\"type\":\"text\",\"text\":\"the TUI user's words\"}}]}}",
                js(&t)
            ),
        )
        .expect("the TUI's turn starts");
        t
    }

    fn complete_turn(&self, thread: &str) {
        self.control(
            "oacFake/turn/complete",
            &format!("{{\"threadId\":{},\"agentText\":\"ok\"}}", js(thread)),
        );
    }

    /// Codex reports a call as started, made on `server`; returns the item id.
    fn mcp_start_as(&self, server: &str, thread: &str, tool: &str, arguments: &str) -> String {
        let r = self.control(
            "oacFake/mcp/start",
            &format!(
                "{{\"threadId\":{},\"server\":{},\"tool\":{},\"arguments\":{arguments}}}",
                js(thread),
                js(server),
                js(tool)
            ),
        );
        get_str(&r, "itemId").unwrap().to_owned()
    }

    fn mcp_complete(&self, thread: &str, item: &str, failed: bool, text: &str) {
        self.control(
            "oacFake/mcp/complete",
            &format!(
                "{{\"threadId\":{},\"itemId\":{},\"outcome\":{},\"text\":{}}}",
                js(thread),
                js(item),
                js(if failed { "failed" } else { "completed" }),
                js(text)
            ),
        );
    }

    /// The OAC client's own `thread/queue/add` calls, from the fake's call log.
    fn adds(&self) -> Vec<Json> {
        let r = self.control(
            "oacFake/calls",
            &format!("{{\"clientName\":{}}}", js(CLIENT_NAME)),
        );
        get(&r, "calls")
            .and_then(Json::as_array)
            .unwrap_or(&[])
            .iter()
            .filter(|c| get_str(c, "method") == Some("thread/queue/add"))
            .cloned()
            .collect()
    }
}

/// The text of the adds that went to `thread`.
fn add_texts(adds: &[Json], thread: &str) -> Vec<String> {
    adds.iter()
        .filter(|c| {
            get(c, "params")
                .and_then(|p| get_str(p, "threadId"))
                .is_some_and(|t| t == thread)
        })
        .filter_map(|c| {
            get(c, "params")
                .and_then(|p| get(p, "input"))
                .and_then(Json::as_array)
                .and_then(|a| a.first())
                .and_then(|i| get_str(i, "text"))
                .map(str::to_owned)
        })
        .collect()
}

// ---- the carrier opener (the given part) ------------------------------------------------

/// One socket, shared by the reader, the writer and whoever cuts it. On Windows a `shutdown`
/// does not wake a read blocked in another thread, so the read wakes by itself and looks at
/// `cut` (the same arrangement as the real Windows carrier stream).
#[derive(Clone)]
struct Shared(Arc<TcpStream>, Arc<std::sync::atomic::AtomicBool>);

impl Shared {
    fn cut(&self) {
        self.1.store(true, Ordering::SeqCst);
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}

impl std::io::Read for Shared {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if self.1.load(Ordering::SeqCst) {
                return Ok(0);
            }
            match (&*self.0).read(buf) {
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                other => return other,
            }
        }
    }
}

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        (&*self.0).write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        (&*self.0).flush()
    }
}

/// What the test gives the daemon in place of the control socket: TCP to the fake, with a
/// synthetic observation of the server. Until `serve` is set the socket is "absent".
struct Carrier {
    fake_host: String,
    observation: Mutex<Option<PeerObservation>>,
    opens: AtomicUsize,
    streams: Mutex<Vec<Shared>>,
}

impl Carrier {
    fn new(fake: &Fake) -> Arc<Carrier> {
        Arc::new(Carrier {
            fake_host: fake.host.clone(),
            observation: Mutex::new(None),
            opens: AtomicUsize::new(0),
            streams: Mutex::new(Vec::new()),
        })
    }

    fn serve(&self, observation: PeerObservation) {
        *self.observation.lock().unwrap() = Some(observation);
    }

    fn opens(&self) -> usize {
        self.opens.load(Ordering::SeqCst)
    }

    /// The app-server "restarts": every carrier stream is cut.
    fn sever(&self) {
        for s in self.streams.lock().unwrap().drain(..) {
            s.cut();
        }
    }

    fn opener(self: &Arc<Self>) -> CarrierOpener {
        let me = self.clone();
        Arc::new(move |runtime| {
            me.opens.fetch_add(1, Ordering::SeqCst);
            let Some(observation) = me.observation.lock().unwrap().clone() else {
                return Err(OpenError::Absent);
            };
            let tcp = TcpStream::connect(&me.fake_host)
                .map_err(|e| OpenError::Unreachable(format!("{:?}", e.kind())))?;
            tcp.set_read_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            let socket = Shared(Arc::new(tcp), Arc::default());
            let (reader, writer, closer) = (socket.clone(), socket.clone(), socket.clone());
            me.streams.lock().unwrap().push(socket);
            Ok(OpenedCarrier {
                stream: blocking_stream(
                    runtime,
                    Box::new(reader),
                    Box::new(writer),
                    Box::new(move || {
                        closer.cut();
                    }),
                ),
                observation,
            })
        })
    }
}

// ---- the scene --------------------------------------------------------------------------

struct Scene {
    // Drop order: the host and the daemon's runtime go before the fake and the directory.
    host: Option<Host>,
    daemon: Option<Daemon>,
    rt: Rt,
    adapter: Arc<CodexAdapter>,
    /// The name the adapter was set with, and the `server` Codex reports for the shim's calls.
    registered: String,
    carrier: Arc<Carrier>,
    fake: Fake,
    log: Captured,
    taps: Arc<Mutex<Vec<Diagnostic>>>,
    namespace: String,
    dir: PathBuf,
    _dump: DumpOnPanic,
    _namespace_dir: NamespaceDir,
}

impl Scene {
    fn new(tag: &str) -> Scene {
        Scene::named(tag, REGISTERED)
    }

    /// A scene whose adapter is built as the daemon builds it, for the server registered with
    /// Codex as `registered`.
    fn named(tag: &str, registered: &str) -> Scene {
        let namespace = format!("cx{}{tag}", std::process::id());
        let dir =
            std::env::temp_dir().join(format!("oac-codex-daemon-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake = Fake::spawn();
        let carrier = Carrier::new(&fake);
        let network = MemoryNetwork::new();
        let log = Captured::default();
        let dump = DumpOnPanic(log.clone());
        let taps: Arc<Mutex<Vec<Diagnostic>>> = Arc::default();
        let tap_sink = taps.clone();
        let rt = Rt::new();
        let scope = Scope::for_user_in(Some(&namespace)).unwrap();
        let namespace_dir = NamespaceDir(
            #[cfg(unix)]
            Path::new(&scope.endpoint("codex").display_name())
                .parent()
                .map(Path::to_path_buf),
        );
        let sink = LogSink::new(log.clone()).with_tap(Arc::new(move |d| {
            tap_sink.lock().unwrap().push(d.clone());
        }));
        let adapter = Arc::new(
            oac_cli::codex::adapter(registered, &sink).expect("a registered name Codex accepts"),
        );
        let net = network.clone();
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let parts = Parts {
            scope,
            key_store: Box::new(MemoryKeyStore::new()),
            pairing_store: Box::new(MemoryPairingStore::new()),
            agreement_store: Box::new(MemoryAgreementKeyStore::new()),
            principal: Token::parse("test").unwrap(),
            transport: Arc::new(MemoryTransport::new()),
            transport_configuration: MemoryConfiguration::wrap(&network),
            clock,
            monotonic: Arc::new(move || net.now()),
            config: PipelineConfig::default(),
            harnesses: vec![("codex".to_owned(), adapter.clone() as _)],
            log: sink,
            codex: Some(CarrierWiring {
                adapter: adapter.clone(),
                opener: carrier.opener(),
            }),
        };
        let daemon = rt
            .run(Daemon::start(parts))
            .unwrap_or_else(|e| panic!("the daemon starts: {e}"));
        Scene {
            host: None,
            daemon: Some(daemon),
            rt,
            adapter,
            registered: registered.to_owned(),
            carrier,
            fake,
            log,
            taps,
            namespace,
            dir,
            _dump: dump,
            _namespace_dir: namespace_dir,
        }
    }

    fn host(&mut self) -> &mut Host {
        if self.host.is_none() {
            self.host = Some(Host::start(&self.dir, &self.namespace));
        }
        self.host.as_mut().unwrap()
    }

    fn daemon(&self) -> &Daemon {
        self.daemon.as_ref().unwrap()
    }

    /// The server's observation as the daemon would make it, for the `codex` process `pid`.
    fn observe_host(&mut self) -> PeerObservation {
        let pid = self.host().pid();
        let table = SystemTable::new().expect("the process table");
        let started = peer::process_started(pid).expect("the host's start time");
        peer::observe_process(&table, "codex", &ProcessId { pid, started })
    }

    fn sessions(&self) -> Vec<SessionId> {
        self.daemon()
            .pipelines()
            .status()
            .sessions
            .iter()
            .map(|s| s.session_id.clone())
            .collect()
    }

    /// A tool call as Codex makes it in `thread`: reported started, called on shim `n`,
    /// reported completed with what the shim returned. `(is_error, text)`.
    fn codex_call(&mut self, n: u32, thread: &str, tool: &str, args: &str) -> (bool, String) {
        let item = self.fake.mcp_start_as(&self.registered, thread, tool, args);
        let (err, text) = self.host().call(n, tool, args);
        self.fake.mcp_complete(thread, &item, err, &text);
        (err, text)
    }

    /// The pairing of shim `n` with `thread`: a call refused with a value, reported back as
    /// its reveal, bound by the core. Returns the session the core made.
    fn pair(&mut self, n: u32, thread: &str) -> SessionId {
        let before = self.sessions();
        self.host().open(n);
        let (err, text) = self.codex_call(
            n,
            thread,
            "send",
            &format!(
                "{{\"to\":\"{}\",\"content\":[{{\"type\":\"text\",\"text\":\"hello\"}}]}}",
                SessionId::from_random_octets([0xee; 16]).as_str()
            ),
        );
        assert!(err, "{text}");
        assert!(
            text.contains("oac-pair-"),
            "a refusal that carries a pairing value: {text}"
        );
        eventually("a binding", || {
            self.sessions().into_iter().find(|s| !before.contains(s))
        })
    }

    fn wait_log(&self, needle: &str) {
        eventually(needle, || self.log.text().contains(needle).then_some(()));
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        drop(self.host.take());
        if let Some(daemon) = self.daemon.take() {
            let (tx, rx) = mpsc::channel();
            self.rt.handle.spawn(async move {
                daemon.shutdown().await;
                let _ = tx.send(());
            });
            let _ = rx.recv_timeout(WAIT);
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// ---- the `codex` host process: the parent side -------------------------------------------

/// A process named `codex` that starts and relays real shims.
struct Host {
    child: Child,
    commands: ChildStdin,
    lines: Receiver<(u32, String)>,
    queued: HashMap<u32, VecDeque<String>>,
    ids: HashMap<u32, u64>,
    started: std::collections::HashSet<u32>,
}

impl Host {
    fn start(dir: &Path, namespace: &str) -> Host {
        let bin = dir.join("bin");
        let work = dir.join("work");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        let codex = bin.join(if cfg!(windows) { "codex.exe" } else { "codex" });
        exec_copy::install_exe_copy(&codex, exec_copy::Existing::Reuse);
        let mut child = Command::new(&codex)
            .args(["--exact", HOST_TEST, "--nocapture", "--test-threads=1"])
            .env(ROLE, "host")
            .env("OAC_SCOPE", namespace)
            .env("OAC_HOST_WORK", &work)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn_retrying()
            .expect("the codex host starts");
        let commands = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                // Lines of the host: `@@<n> <text>`. The test harness prints its own, and its
                // first line shares a line with the host's.
                if let Some(i) = line.find("@@")
                    && let Some((n, rest)) = line[i + 2..].split_once(' ')
                    && let Ok(n) = n.parse::<u32>()
                {
                    let _ = tx.send((n, rest.to_owned()));
                }
            }
        });
        let mut host = Host {
            child,
            commands,
            lines,
            queued: HashMap::new(),
            ids: HashMap::new(),
            started: std::collections::HashSet::new(),
        };
        host.command("0 ping");
        assert_eq!(host.next_line(0), "pong");
        host
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn command(&mut self, text: &str) {
        writeln!(self.commands, "{text}").unwrap();
        self.commands.flush().unwrap();
    }

    fn next_line(&mut self, n: u32) -> String {
        let end = Instant::now() + WAIT;
        loop {
            if let Some(l) = self.queued.get_mut(&n).and_then(VecDeque::pop_front) {
                return l;
            }
            let now = Instant::now();
            assert!(now < end, "no line from shim {n}");
            match self.lines.recv_timeout(end - now) {
                Ok((m, l)) => self.queued.entry(m).or_default().push_back(l),
                Err(RecvTimeoutError::Timeout) => panic!("no line from shim {n}"),
                Err(RecvTimeoutError::Disconnected) => panic!("the host ended"),
            }
        }
    }

    fn start_shim(&mut self, n: u32) {
        self.command(&format!("{n} start"));
        assert_eq!(self.next_line(n), "started");
        self.started.insert(n);
    }

    /// Starts shim `n` and says whether the daemon served it (it was closed if not).
    fn send_line(&mut self, n: u32, frame: &str) {
        self.command(&format!("{n} send {frame}"));
    }

    fn request(&mut self, n: u32, method: &str, params: &str) -> Json {
        let id = self.ids.entry(n).or_insert(0);
        *id += 1;
        let id = *id;
        self.send_line(
            n,
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":{},\"params\":{params}}}",
                js(method)
            ),
        );
        loop {
            let line = self.next_line(n);
            let Some(frame) = line.strip_prefix("out ") else {
                panic!("shim {n} said {line} while a response to {method} was awaited");
            };
            let v = json::parse(frame.as_bytes()).expect("JSON from the shim");
            if get(&v, "id").and_then(Json::as_number).map(|x| x.raw()) == Some(&id.to_string()) {
                return v;
            }
        }
    }

    /// `initialize`, `notifications/initialized`, `tools/list`: the way Codex opens.
    fn open(&mut self, n: u32) {
        if !self.started.contains(&n) {
            self.start_shim(n);
        }
        self.request(
            n,
            "initialize",
            "{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},\"clientInfo\":{\"name\":\"codex-mcp-client\",\"version\":\"0.163.0\"}}",
        );
        self.send_line(
            n,
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}",
        );
        let list = self.request(n, "tools/list", "{}");
        assert!(get(&list, "result").is_some(), "{}", list.to_compact());
    }

    /// A tool call: `(is_error, text)`.
    fn call(&mut self, n: u32, tool: &str, args: &str) -> (bool, String) {
        let v = self.request(
            n,
            "tools/call",
            &format!("{{\"name\":{},\"arguments\":{args}}}", js(tool)),
        );
        let r = get(&v, "result").unwrap_or_else(|| panic!("an error: {}", v.to_compact()));
        (
            get(r, "isError").and_then(Json::as_bool) == Some(true),
            get(r, "content")
                .and_then(Json::as_array)
                .and_then(|c| c.first())
                .and_then(|b| get_str(b, "text"))
                .unwrap_or_default()
                .to_owned(),
        )
    }

    /// The exit status the shim `n` ended with, once it has.
    fn exit_of(&mut self, n: u32) -> i32 {
        self.command(&format!("{n} wait"));
        loop {
            let line = self.next_line(n);
            if let Some(code) = line.strip_prefix("exit ") {
                return code.parse().expect("an exit code");
            }
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        // Ask the host to end its shims and itself, and give it a bounded moment, so that the
        // shim processes (its children) exit before it is killed (#435).
        let _ = writeln!(self.commands, "0 exit");
        let _ = self.commands.flush();
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end && !matches!(self.child.try_wait(), Ok(Some(_))) {
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---- the `codex` host process: the child side --------------------------------------------

fn is_host() -> bool {
    std::env::var(ROLE).as_deref() == Ok("host")
}

type Say = Arc<dyn Fn(u32, &str) + Send + Sync>;

/// Starts real `oac mcp-shim --harness codex` processes on command and relays their stdio, as
/// Codex's app-server does for its MCP servers: the shims are this process's children, so the
/// daemon finds a `codex` executable above each of them. Writes only `@@<n> <text>` lines.
fn host() {
    let work = PathBuf::from(std::env::var("OAC_HOST_WORK").unwrap());
    std::env::set_current_dir(&work).unwrap();
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let say: Say = Arc::new({
        let out = out.clone();
        move |n: u32, text: &str| {
            let mut o = out.lock().unwrap();
            let _ = writeln!(o, "@@{n} {text}");
            let _ = o.flush();
        }
    });
    let mut shims: HashMap<u32, (Child, Option<ChildStdin>)> = HashMap::new();
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        let mut parts = line.splitn(3, ' ');
        let n: u32 = parts.next().unwrap().parse().unwrap();
        let verb = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("");
        match verb {
            "ping" => say(0, "pong"),
            "start" => {
                let mut child = Command::new(oac())
                    .args(["mcp-shim", "--harness", "codex"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .expect("the shim starts");
                let stdin = child.stdin.take().unwrap();
                let stdout = child.stdout.take().unwrap();
                let relay_say = say.clone();
                std::thread::spawn(move || {
                    for l in BufReader::new(stdout).lines().map_while(Result::ok) {
                        relay_say(n, &format!("out {l}"));
                    }
                    relay_say(n, "eof");
                });
                shims.insert(n, (child, Some(stdin)));
                say(n, "started");
            }
            "send" => {
                if let Some((_, Some(stdin))) = shims.get_mut(&n) {
                    let _ = writeln!(stdin, "{rest}");
                    let _ = stdin.flush();
                }
            }
            "wait" => {
                if let Some((child, _)) = shims.get_mut(&n) {
                    let end = Instant::now() + WAIT;
                    let code = loop {
                        if let Some(s) = child.try_wait().unwrap() {
                            break s.code().unwrap_or(-1);
                        }
                        if Instant::now() > end {
                            break -999;
                        }
                        std::thread::sleep(Duration::from_millis(25));
                    };
                    say(n, &format!("exit {code}"));
                }
            }
            "exit" => break,
            other => panic!("unknown host command {other}"),
        }
    }
    for (_, (mut child, _)) in shims {
        let _ = child.kill();
        let _ = child.wait();
    }
}

// ---- tests -------------------------------------------------------------------------------

/// The `codex` endpoint is served and its peer authenticated (the real shim of a `codex`
/// process is accepted, observed and paired with a harness process), the carrier connects when
/// the first shim does, and a shim whose harness process is not the carrier's server gets a
/// plain refusal with no pairing value ([MCPB-ATT-022], ATT-022).
#[test]
fn a_codex_shim_without_a_paired_carrier_gets_a_plain_refusal() {
    if is_host() {
        return host();
    }
    let mut s = Scene::new("a");
    // The control socket "exists", but its server is some other process: a key that no shim
    // under this `codex` process has.
    let other = ProcessId {
        pid: u32::MAX - 1,
        started: 1,
    };
    s.carrier.serve(PeerObservation {
        pairing_key: Some(peer::pairing_key("codex", &other)),
        harness_label: Token::parse("codex"),
        working_directory: None,
    });
    assert!(
        s.daemon().endpoints().iter().any(|e| e.contains("codex")),
        "the codex endpoint is served: {:?}",
        s.daemon().endpoints()
    );
    // Nothing connected a carrier: at start the opener ran once.
    eventually("the start-up attempt", || {
        (s.carrier.opens() >= 1).then_some(())
    });

    s.host().open(1);
    s.wait_log("the Codex carrier is connected");
    // The peer was authenticated and observed: a real process of this user, with a `codex`
    // ancestor (so a pairing key).
    let accepted: Vec<Diagnostic> = s.taps.lock().unwrap().clone();
    assert!(
        accepted.iter().any(|d| matches!(
            d,
            Diagnostic::Accepted { label, peer: Some(_), paired: true } if label == "codex"
        )),
        "{accepted:?}"
    );
    // Its tool call is refused, and the refusal carries no pairing value.
    let (err, text) = s.host().call(
        1,
        "send",
        "{\"to\":\"01harn7x9k2m4p6q8r0s2t4v6w\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}",
    );
    assert!(err, "{text}");
    assert!(!text.contains("oac-pair-"), "a plain refusal: {text}");
    assert!(s.sessions().is_empty());
}

/// With no control socket, a Codex shim connection is closed (a shim under no carrier is not
/// served), and the daemon tries again only when another shim connects: never on a timer.
#[test]
fn with_no_control_socket_a_shim_is_closed_and_the_daemon_tries_only_when_a_shim_connects() {
    if is_host() {
        return host();
    }
    let mut s = Scene::new("b");
    eventually("the start-up attempt", || {
        (s.carrier.opens() == 1).then_some(())
    });
    // No timer reconnects: with no shim, the count does not move over several timer ticks.
    std::thread::sleep(QUIET);
    assert_eq!(s.carrier.opens(), 1, "{}", s.log.text());
    assert_eq!(s.adapter.carrier_state(), CarrierState::None);

    s.host().start_shim(1);
    assert_eq!(s.host().exit_of(1), 1, "the daemon closed the connection");
    assert_eq!(s.carrier.opens(), 2, "one attempt for the shim");
    s.host().start_shim(2);
    assert_eq!(s.host().exit_of(2), 1);
    assert_eq!(s.carrier.opens(), 3);
    std::thread::sleep(QUIET);
    assert_eq!(s.carrier.opens(), 3, "nothing polls");
    assert!(
        s.log.text().contains("no Codex control socket"),
        "{}",
        s.log.text()
    );
}

/// Pairing end to end, `deliver`, the correlation of a reply (#69 box 2), and a lost carrier
/// replaced when the next shim connects (risk row 90).
#[test]
fn codex_sessions_pair_receive_reply_with_correlation_and_survive_a_lost_carrier() {
    if is_host() {
        return host();
    }
    let mut s = Scene::new("c");
    let observation = s.observe_host();
    assert!(
        observation.pairing_key.is_some(),
        "the host is a codex process"
    );
    // The start-up attempt found no socket; from now on there is one.
    eventually("the start-up attempt", || {
        (s.carrier.opens() == 1).then_some(())
    });
    s.carrier.serve(observation);

    // Two Codex sessions in the app-server's one `codex` process: A on thread ta, B on tb.
    let (ta, tb) = (s.fake.busy_thread(), s.fake.busy_thread());
    let a = s.pair(1, &ta);
    let b = s.pair(2, &tb);
    assert_ne!(a, b);
    assert_eq!(s.adapter.carrier_state(), CarrierState::Up);
    // The carrier was connected for the first shim, once.
    assert_eq!(
        s.carrier.opens(),
        2,
        "the start-up attempt, then the first shim"
    );

    // One grant, "A may write to B" (spec/security.md §9.5).
    let (sa, sb) = (a.clone(), b.clone());
    let store = MemoryPairingStore::new();
    s.daemon().pipelines().with_engine(|e| {
        let own = e.own_key_id().clone();
        e.add_grant(
            Grant::Inbound {
                writer: PeerSide::session(own, sa),
                target: LocalSide::Session(sb),
            },
            OperatorConfirmed::by_operator(),
            &store,
        )
        .unwrap();
    });

    // A -> B: the message reaches Codex as one `thread/queue/add` on B's thread.
    s.fake.complete_turn(&tb);
    let body = "\"content\":[{\"type\":\"text\",\"text\":\"Please review the scheduler.\"}]";
    let (err, text) = s.codex_call(
        1,
        &ta,
        "send",
        &format!("{{\"to\":\"{}\",{body}}}", b.as_str()),
    );
    assert!(!err, "{text}");
    let delivered = eventually("the add on B's thread", || {
        let t = add_texts(&s.fake.adds(), &tb);
        (!t.is_empty()).then_some(t)
    });
    assert_eq!(delivered.len(), 1);
    assert!(
        delivered[0].contains("Please review the scheduler."),
        "{}",
        delivered[0]
    );
    assert!(
        add_texts(&s.fake.adds(), &ta).is_empty(),
        "nothing went to A"
    );
    let id = delivered[0]
        .lines()
        .find_map(|l| l.strip_prefix("oac_message_id: "))
        .expect("the frame names the message")
        .to_owned();

    // B replies. A reply that omits the hint, or garbles it, is sent uncorrelated and says so.
    s.fake.complete_turn(&ta);
    let answer = "\"content\":[{\"type\":\"text\",\"text\":\"Reviewed.\"}]";
    let (err, text) = s.codex_call(
        2,
        &tb,
        "reply",
        &format!("{{\"to\":\"{}\",{answer}}}", a.as_str()),
    );
    assert!(!err, "{text}");
    assert!(text.contains("\"correlation\":\"uncorrelated\""), "{text}");
    let (err, text) = s.codex_call(
        2,
        &tb,
        "reply",
        &format!(
            "{{\"in_reply_to\":\"not-an-id\",\"to\":\"{}\",{answer}}}",
            a.as_str()
        ),
    );
    assert!(!err, "{text}");
    assert!(text.contains("\"correlation\":\"uncorrelated\""), "{text}");
    // The id the message was handed with: correlated.
    let (err, text) = s.codex_call(
        2,
        &tb,
        "reply",
        &format!("{{\"in_reply_to\":\"{id}\",{answer}}}"),
    );
    assert!(!err, "{text}");
    assert!(text.contains("\"correlation\":\"correlated\""), "{text}");
    // The correlated reply, and only it (the others have no grant), reached A's thread.
    let got = eventually("the reply on A's thread", || {
        let t = add_texts(&s.fake.adds(), &ta);
        (!t.is_empty()).then_some(t)
    });
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].contains("Reviewed."), "{}", got[0]);

    // The app-server goes away: the carrier is lost. Nothing reconnects it by itself.
    let before = s.carrier.opens();
    s.carrier.sever();
    eventually("the lost carrier", || {
        (s.adapter.carrier_state() == CarrierState::Lost).then_some(())
    });
    std::thread::sleep(QUIET);
    assert_eq!(
        s.carrier.opens(),
        before,
        "no timer reconnects a lost carrier"
    );
    assert_eq!(s.adapter.carrier_state(), CarrierState::Lost);

    // The next Codex shim re-designates a carrier, and is paired through it.
    let tc = s.fake.busy_thread();
    let c = s.pair(3, &tc);
    assert_eq!(s.carrier.opens(), before + 1);
    assert_eq!(s.adapter.carrier_state(), CarrierState::Up);
    assert!(![a, b].contains(&c));
    s.wait_log("the Codex carrier is connected");
}

/// An operator who registered the shim under another name than the default (`codex mcp add
/// oac-g8live ...`) and set the daemon to match: Codex reports that name as the `server` of
/// the shim's calls, and the connection pairs. A report under any other name pairs nothing,
/// and the daemon's log says so, naming both names (#69; the live leg that registered
/// `oac-g8live` against a daemon that expected `oac` never paired and logged nothing).
#[test]
fn a_registered_name_other_than_the_default_pairs_codex_sessions() {
    if is_host() {
        return host();
    }
    let mut s = Scene::named("d", "oac-g8live");
    assert_eq!(s.adapter.registered_name(), "oac-g8live");
    let observation = s.observe_host();
    eventually("the start-up attempt", || {
        (s.carrier.opens() == 1).then_some(())
    });
    s.carrier.serve(observation);
    let thread = s.fake.busy_thread();
    s.host().open(1);
    s.wait_log("the Codex carrier is connected (registered name oac-g8live)");

    // The call, reported under the default name: not this daemon's server.
    let args = format!(
        "{{\"to\":\"{}\",\"content\":[{{\"type\":\"text\",\"text\":\"hello\"}}]}}",
        SessionId::from_random_octets([0xee; 16]).as_str()
    );
    let item = s.fake.mcp_start_as("oac", &thread, "send", &args);
    let (err, text) = s.host().call(1, "send", &args);
    assert!(err && text.contains("oac-pair-"), "{text}");
    s.fake.mcp_complete(&thread, &item, err, &text);
    s.wait_log("not the registered name oac-g8live");
    assert!(s.sessions().is_empty(), "{}", s.log.text());
    assert!(
        !s.log.text().contains(&thread),
        "no thread id in the log: {}",
        s.log.text()
    );

    // The same call, reported under the registered name: the reveal.
    let before = s.sessions();
    let (err, text) = s.codex_call(1, &thread, "send", &args);
    assert!(err && text.contains("oac-pair-"), "{text}");
    eventually("a binding", || {
        s.sessions().into_iter().find(|id| !before.contains(id))
    });
}
