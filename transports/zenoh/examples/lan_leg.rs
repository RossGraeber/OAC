// SPDX-License-Identifier: Apache-2.0

//! Two-host LAN leg driver (#64 steps 1 to 6, #63 step 8). One binary, run once per role
//! and step on each host; it prints one JSON evidence line per observation.
//!
//!     OAC_TEST_LAN=1 cargo run -p oac-transport-zenoh --example lan_leg -- help
//!
//! It refuses every command unless `OAC_TEST_LAN=1`, before any listener is opened. A
//! listener is opened only on the `--listen IP:PORT` the operator passes (never the
//! unspecified address, never a default). State is written only under `--dir`.
//!
//! Scope, stated plainly. It drives the transport's public surface (`PairingAuthority`,
//! `PeerConfiguration::lan`, `PeerTransport`) and `oac_core::sealing` seal and open. It does
//! not run the core pipeline: the expiry check of step 6 is this file's stand-in for the
//! receiving core's check (SEC-SEL-036), the frames are sealed with the RFC 7748 test keys
//! (the shared contract suite uses the same ones) and carry synthetic bytes, not signed
//! envelopes. Step 7 and two of step 5's controls are not driven (see the oac-lan-leg skill).

use std::collections::HashMap;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use oac_core::ids::KeyId;
use oac_core::sealing::{self, AgreementPrivateKey, AgreementPublicKey};
use oac_core::transport::{Deadline, Destination, Payload, PayloadKind, PresenceEvent, Transport};
use oac_transport_zenoh::{
    CertificateFiles, LINK_LEASE_MILLIS, MAX_PAYLOAD_OCTETS, PairingAuthority, PeerConfiguration,
    PeerTransport,
};
use serde_json::{Value, json};

const SCHEMA: &str = "oac-lan-leg/1";
/// The operating limit on cross-host clock difference (`transports/zenoh/src/lib.rs`).
const SKEW_LIMIT_MS: i64 = 5_000;
const DEFAULT_PARTITION: &str = "lan-leg";
/// What the transport reports when the native session did not open (the LAN role has no
/// second attempt): TLS refusal, unreachable listener or port in use.
const NATIVE_OPEN_FAILED: &str = "the peer session did not open";
const USAGE: &str = "\
lan_leg: two-host LAN leg driver. Every command needs OAC_TEST_LAN=1.

  env
  issue  --dir D --ip-a IP --ip-b IP [--cn-a N --cn-b N --cn-c N]
  import --dir D                       (host B: harden the carried bundle, owner-only)
  peer   --dir D --cred NAME --role a|b|c --listen IP:PORT --allow CN[,CN]
         [--connect IP:PORT[,..]] [--multicast] [--partition P] [--to a|b|c]
         [--skew-pings N] [--timed-test] [--window-ms N] [--hold-ms N] [--skew-pings-after N]
         [--link-timeout-secs N] [--run-secs N] [--end clean|abort]
  negative --dir D --case wrong-ca|wrong-key|wrong-san|unlisted-cn --role b --listen IP:PORT
         --connect IP:PORT --allow CN [--partition P] [--to a] [--probes N]
";

// ---- output ----------------------------------------------------------------------------

fn now_us() -> i64 {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(d.as_micros()).unwrap_or(i64::MAX)
}

/// RFC 3339 UTC with microseconds (civil-from-days, H. Hinnant).
fn iso(us: i64) -> String {
    let days = us.div_euclid(86_400_000_000);
    let rem = us.rem_euclid(86_400_000_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let (s, micro) = (rem / 1_000_000, rem % 1_000_000);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{micro:06}Z",
        s / 3600,
        (s / 60) % 60,
        s % 60
    )
}

/// One evidence line. `utc_us` is the instant of the observation on this host's clock.
fn emit(role: &str, event: &str, mut fields: Value) {
    let at_us = now_us();
    let mut line = json!({"schema": SCHEMA, "event": event, "role": role,
        "utc": iso(at_us), "utc_us": at_us});
    if let (Some(a), Some(b)) = (line.as_object_mut(), fields.as_object_mut()) {
        a.append(b);
    }
    println!("{line}");
}

fn fail(role: &str, why: &str) -> ! {
    emit(role, "error", json!({"why": why}));
    eprintln!("lan_leg: {why}");
    std::process::exit(2);
}

// ---- arguments -------------------------------------------------------------------------

struct Args(HashMap<String, String>);

impl Args {
    fn parse(items: &[String]) -> Args {
        const FLAGS: [&str; 2] = ["multicast", "timed-test"];
        let mut map = HashMap::new();
        let mut it = items.iter();
        while let Some(k) = it.next() {
            let Some(name) = k.strip_prefix("--") else {
                fail("-", &format!("unexpected argument {k}"));
            };
            if FLAGS.contains(&name) {
                map.insert(name.to_owned(), "1".to_owned());
            } else if let Some(v) = it.next() {
                map.insert(name.to_owned(), v.clone());
            } else {
                fail("-", &format!("--{name} needs a value"));
            }
        }
        Args(map)
    }
    fn get(&self, k: &str) -> Option<&str> {
        self.0.get(k).map(String::as_str)
    }
    fn need(&self, k: &str) -> &str {
        self.get(k)
            .unwrap_or_else(|| fail("-", &format!("--{k} is required")))
    }
    fn flag(&self, k: &str) -> bool {
        self.0.contains_key(k)
    }
    fn num(&self, k: &str, default: u64) -> u64 {
        self.get(k).map_or(default, |v| {
            v.parse()
                .unwrap_or_else(|_| fail("-", &format!("--{k} must be a number")))
        })
    }
    fn addr(&self, k: &str) -> SocketAddr {
        let a: SocketAddr = self
            .need(k)
            .parse()
            .unwrap_or_else(|_| fail("-", &format!("--{k} must be IP:PORT")));
        if a.ip().is_unspecified() {
            fail(
                "-",
                &format!("--{k}: the unspecified address is refused; name one host address"),
            );
        }
        a
    }
    fn addrs(&self, k: &str) -> Vec<SocketAddr> {
        self.get(k)
            .map(|v| {
                v.split(',')
                    .map(|s| {
                        s.parse()
                            .unwrap_or_else(|_| fail("-", &format!("--{k}: {s} is not IP:PORT")))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    fn role(&self, k: &str) -> char {
        match self.need(k) {
            "a" => 'a',
            "b" => 'b',
            "c" => 'c',
            _ => fail("-", &format!("--{k} must be a, b or c")),
        }
    }
}

// ---- environment evidence --------------------------------------------------------------

fn run_text(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn commit_sha(args: &Args) -> Value {
    if let Some(c) = args.get("commit") {
        return json!(c);
    }
    match run_text("git", &["rev-parse", "HEAD"]) {
        Some(sha) => {
            let dirty = run_text("git", &["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
            json!({"sha": sha, "tree_dirty": dirty})
        }
        None => json!("unknown"),
    }
}

fn env_evidence(role: &str, args: &Args) {
    let os_version = if cfg!(windows) {
        run_text("cmd", &["/c", "ver"])
    } else {
        run_text("uname", &["-sr"])
    };
    emit(
        role,
        "env",
        json!({
            "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
            "os_version": os_version,
            "oac_commit": commit_sha(args),
            "zenoh_version": zenoh::GIT_VERSION,
            "link_lease_ms": LINK_LEASE_MILLIS,
            "skew_limit_ms": SKEW_LIMIT_MS,
            "driver_pid": std::process::id(),
        }),
    );
}

// ---- credentials -----------------------------------------------------------------------

fn files_in(dir: &Path) -> CertificateFiles {
    CertificateFiles {
        certificate: dir.join("certificate.pem"),
        private_key: dir.join("key.pem"),
        trusted_roots: dir.join("roots.pem"),
    }
}

fn opaque_cn(prefix: &str) -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_i64(now_us());
    format!("{prefix}-{:08x}", h.finish() & 0xffff_ffff)
}

fn ip_arg(args: &Args, k: &str) -> IpAddr {
    args.need(k)
        .parse()
        .unwrap_or_else(|_| fail("-", &format!("--{k} must be an IP address")))
}

/// Host A. One fresh authority issues every leaf and is dropped when this returns: its
/// private key is never written anywhere. Each leaf is installed (by the crate) into its
/// own fresh owner-only directory. A leaf's private key is generated with the leaf, here,
/// because the transport API has no request-signing form; host B's bundle is therefore
/// carried by the operator (see the oac-lan-leg skill).
fn issue(args: &Args) {
    let root = PathBuf::from(args.need("dir"));
    let (ip_a, ip_b) = (ip_arg(args, "ip-a"), ip_arg(args, "ip-b"));
    let cn_a = args
        .get("cn-a")
        .map_or_else(|| opaque_cn("lan-a"), str::to_owned);
    let cn_b = args
        .get("cn-b")
        .map_or_else(|| opaque_cn("lan-b"), str::to_owned);
    let cn_c = args
        .get("cn-c")
        .map_or_else(|| opaque_cn("lan-c"), str::to_owned);
    let cn_x = opaque_cn("lan-x");
    let place = |parent: &str, name: &str, cn: &str, ips: &[IpAddr], auth: &PairingAuthority| {
        let dir = root.join(parent).join(name);
        auth.issue(cn, ips)
            .and_then(|c| c.install(&dir))
            .unwrap_or_else(|e| fail("a", &format!("issue {name}: {e}")));
        dir
    };
    let authority = PairingAuthority::new().unwrap_or_else(|e| fail("a", &e));
    let rogue = PairingAuthority::new().unwrap_or_else(|e| fail("a", &e));
    // A third address no host has, for the wrong IP SAN control (RFC 5737 TEST-NET-1).
    let wrong: IpAddr = "192.0.2.1".parse().unwrap();
    place("host-a", "a", &cn_a, &[ip_a], &authority);
    place("host-a", "neg-wrong-san", &cn_a, &[wrong], &authority);
    place("for-host-b", "b", &cn_b, &[ip_b], &authority);
    place("for-host-b", "c", &cn_c, &[ip_b], &authority);
    place("for-host-b", "neg-unlisted", &cn_x, &[ip_b], &authority);
    place("for-host-b", "neg-wrong-ca", &cn_b, &[ip_b], &rogue);
    place("for-host-b", "neg-wrong-key", &cn_b, &[ip_b], &rogue);
    drop((authority, rogue));
    emit(
        "a",
        "issue",
        json!({"dir": root, "cn_a": cn_a, "cn_b": cn_b, "cn_c": cn_c, "ip_a": ip_a.to_string(),
            "ip_b": ip_b.to_string(), "authority_private_key_persisted": false,
            "carry_to_host_b": root.join("for-host-b"),
            "keep_on_host_a": root.join("host-a"),
            "allow_on_a": [cn_b, cn_c], "allow_on_b": [cn_a, cn_c]}),
    );
}

/// Host B. A carried directory has lost its owner-only mode; tighten it (the crate refuses
/// anything else). Touches only the credential directories under `--dir/for-host-b`.
fn import(args: &Args) {
    let root = PathBuf::from(args.need("dir")).join("for-host-b");
    let names = fs::read_dir(&root).unwrap_or_else(|_| fail("b", "no for-host-b under --dir"));
    let mut done = Vec::new();
    for entry in names.flatten() {
        let path = entry.path();
        if !path.join("key.pem").is_file() {
            continue;
        }
        harden(&path).unwrap_or_else(|e| fail("b", &format!("{}: {e}", path.display())));
        done.push(entry.file_name().to_string_lossy().into_owned());
    }
    done.sort();
    emit("b", "import", json!({"hardened": done}));
}

fn harden(dir: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
        for f in fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
            fs::set_permissions(f.path(), fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        // The directory keeps no inherited access and grants the current user alone; each
        // file is then reset to inherit exactly that.
        let user = run_text("whoami", &[]).ok_or("cannot name the current user")?;
        let grant = format!("{user}:(OI)(CI)F");
        let icacls = |path: &Path, args: &[&str]| -> Result<(), String> {
            let out = Command::new("icacls")
                .arg(path)
                .args(args)
                .output()
                .map_err(|e| e.to_string())?;
            out.status
                .success()
                .then_some(())
                .ok_or_else(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        };
        icacls(dir, &["/inheritance:r", "/grant:r", &grant, "/Q"])?;
        for f in fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
            icacls(&f.path(), &["/reset", "/Q"])?;
        }
        Ok(())
    }
}

// ---- frames ----------------------------------------------------------------------------

/// RFC 7748 section 6.1 test key pairs, as the shared contract suite uses them. Public
/// vectors: sealing with them exercises the frame format and hides nothing.
fn hex32(s: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).expect("hex");
    }
    out
}
fn test_public(role: char) -> AgreementPublicKey {
    let v = if role == 'b' {
        "de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f"
    } else {
        "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
    };
    AgreementPublicKey::from_octets(hex32(v)).expect("test key")
}
fn test_private(role: char) -> AgreementPrivateKey {
    let v = if role == 'b' {
        "5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb"
    } else {
        "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a"
    };
    AgreementPrivateKey::from_octets(&mut hex32(v))
}
fn device(role: char) -> KeyId {
    KeyId::parse(&role.to_string().repeat(64)).expect("key id")
}

/// The sealed plaintext: seq, sent, deadline, aux1, aux2 (each u64 BE), from, to, tag(16).
struct Inner {
    seq: u64,
    sent_us: u64,
    deadline_us: u64,
    aux: [u64; 2],
    from: char,
    to: char,
    tag: String,
}

impl Inner {
    fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(58);
        for n in [
            self.seq,
            self.sent_us,
            self.deadline_us,
            self.aux[0],
            self.aux[1],
        ] {
            v.extend_from_slice(&n.to_be_bytes());
        }
        v.push(self.from as u8);
        v.push(self.to as u8);
        let mut tag = [0u8; 16];
        tag[..self.tag.len().min(16)]
            .copy_from_slice(&self.tag.as_bytes()[..self.tag.len().min(16)]);
        v.extend_from_slice(&tag);
        v
    }
    fn decode(b: &[u8]) -> Option<Inner> {
        if b.len() != 58 {
            return None;
        }
        let n = |i: usize| u64::from_be_bytes(b[i..i + 8].try_into().unwrap());
        let tag = b[42..58]
            .iter()
            .take_while(|c| **c != 0)
            .map(|c| *c as char)
            .collect();
        Some(Inner {
            seq: n(0),
            sent_us: n(8),
            deadline_us: n(16),
            aux: [n(24), n(32)],
            from: b[40] as char,
            to: b[41] as char,
            tag,
        })
    }
}

struct Peer {
    role: char,
    transport: PeerTransport,
    seq: AtomicU64,
    t0: Instant,
}

impl Peer {
    /// Seal at `now`, publish after `hold`. The transport deadline is separate and long:
    /// only the encrypted deadline is the receiving core's to enforce.
    fn send(&self, to: char, tag: &str, window_ms: u64, hold: Duration, aux: [u64; 2]) -> bool {
        let sent = now_us();
        let inner = Inner {
            seq: self.seq.fetch_add(1, Ordering::SeqCst),
            sent_us: sent as u64,
            deadline_us: (sent + i64::try_from(window_ms * 1000).unwrap_or(0)) as u64,
            aux,
            from: self.role,
            to,
            tag: tag.to_owned(),
        };
        let frame = sealing::seal(
            &test_public(to),
            PayloadKind::Envelope,
            &inner.encode(),
            MAX_PAYLOAD_OCTETS,
        )
        .unwrap_or_else(|e| fail(&self.role.to_string(), &format!("seal: {e}")));
        thread::sleep(hold);
        let taken = self.transport.publish(
            &Destination::Device(device(to)),
            Payload::new(PayloadKind::Sealed, frame),
            Deadline::at(Instant::now() + Duration::from_secs(30)),
        );
        emit(
            &self.role.to_string(),
            "send",
            json!({"tag": tag, "seq": inner.seq, "to": to.to_string(), "sealed_utc_us": sent,
                "hold_ms": hold.as_millis() as u64, "window_ms": window_ms,
                "published_utc_us": now_us(), "taken": format!("{taken:?}")}),
        );
        format!("{taken:?}") == "Taken"
    }
}

struct Pong {
    t1: i64,
    t2: i64,
    t3: i64,
    t4: i64,
}

// ---- the peer command ------------------------------------------------------------------

/// The credential directory `sub` under `--dir`: where `issue` put it (`host-a/` on host A,
/// `for-host-b/` on host B). Looked up first, so a missing one never gets created empty.
fn creds(args: &Args, sub: &str) -> CertificateFiles {
    let root = PathBuf::from(args.need("dir"));
    ["host-a", "for-host-b"]
        .iter()
        .map(|d| root.join(d).join(sub))
        .find(|d| d.join("key.pem").is_file())
        .map(|d| files_in(&d))
        .unwrap_or_else(|| {
            fail(
                "-",
                &format!("no credential {sub} under --dir (issue/import first)"),
            )
        })
}

fn configuration(args: &Args, files: CertificateFiles) -> PeerConfiguration {
    let allow: Vec<String> = args.need("allow").split(',').map(str::to_owned).collect();
    let mut conf = PeerConfiguration::lan(args.addr("listen"), args.addrs("connect"), files, allow)
        .with_partition(args.get("partition").unwrap_or(DEFAULT_PARTITION));
    if args.flag("multicast") {
        conf = conf.with_lan_multicast(true);
    }
    conf
}

fn start_fields(args: &Args, cred: &str) -> Value {
    json!({"cred": cred, "listen": args.get("listen"), "connect": args.get("connect"),
        "allow": args.get("allow"), "multicast": args.flag("multicast"),
        "partition": args.get("partition").unwrap_or(DEFAULT_PARTITION),
        "scouting": if args.flag("multicast") { "multicast" } else { "off" }})
}

fn peer(args: &Args) {
    let role = args.role("role");
    let rs = role.to_string();
    let cred = args.need("cred");
    let conf = configuration(args, creds(args, cred));
    let me = Arc::new(Peer {
        role,
        transport: PeerTransport::new(),
        seq: AtomicU64::new(1),
        t0: Instant::now(),
    });
    let started = me.transport.start(&device(role), conf.wrap());
    let mut f = start_fields(args, cred);
    f["ok"] = json!(started.is_ok());
    if let Err(e) = &started {
        f["error"] = json!(e.to_string());
    }
    emit(&rs, "start", f);
    if started.is_err() {
        std::process::exit(1);
    }

    // Native presence: appearances and losses, stamped when the handler runs.
    let appeared = Arc::new(AtomicU64::new(0));
    let (ap, rp, t0) = (appeared.clone(), rs.clone(), me.t0);
    let watch = me.transport.watch_presence(Arc::new(move |e| {
        let (kind, n) = match e {
            PresenceEvent::CarrierAppeared { .. } => {
                ("appeared", ap.fetch_add(1, Ordering::SeqCst) + 1)
            }
            PresenceEvent::CarrierLoss { .. } => ("loss", 0),
            PresenceEvent::Record { .. } => ("record", 0),
        };
        emit(
            &rp,
            "presence",
            json!({"kind": kind, "n": n, "since_start_ms": t0.elapsed().as_millis() as u64}),
        );
    }));
    if let Err(e) = watch {
        fail(&rs, &format!("watch_presence: {e}"));
    }

    // Frames: the handler only queues; a worker opens, judges and answers.
    let (tx, rx) = mpsc::channel::<(i64, Vec<u8>)>();
    let tx = Mutex::new(tx);
    let _sub = me
        .transport
        .subscribe(
            &Destination::Device(device(role)),
            Arc::new(move |i| {
                let at = now_us();
                let _ = tx.lock().unwrap().send((at, i.payload.octets().to_vec()));
            }),
        )
        .unwrap_or_else(|e| fail(&rs, &format!("subscribe: {e}")));
    let pongs: Arc<Mutex<Vec<Pong>>> = Arc::new(Mutex::new(Vec::new()));
    let worker = {
        let (me, pongs) = (me.clone(), pongs.clone());
        thread::spawn(move || {
            let key = test_private(me.role);
            while let Ok((native_us, octets)) = rx.recv() {
                worker_frame(&me, &key, &pongs, native_us, &octets);
            }
        })
    };

    // Phases.
    let link_wait = Duration::from_secs(args.num("link-timeout-secs", 30));
    let linked = wait_for(link_wait, || appeared.load(Ordering::SeqCst) > 0);
    emit(
        &rs,
        "link",
        json!({"up": linked, "waited_ms": me.t0.elapsed().as_millis() as u64}),
    );
    let to = args
        .get("to")
        .map_or(if role == 'a' { 'b' } else { 'a' }, |s| {
            s.chars().next().unwrap_or('a')
        });
    if linked {
        if args.num("skew-pings", 0) > 0 {
            skew(&me, &pongs, to, args.num("skew-pings", 0), "before");
        }
        if args.flag("timed-test") {
            let window = args.num("window-ms", 2000);
            let hold = args.num("hold-ms", window + 1500);
            me.send(to, "ontime", window, Duration::ZERO, [0, 0]);
            me.send(to, "late", window, Duration::from_millis(hold), [0, 0]);
            thread::sleep(Duration::from_secs(2));
        }
        if args.num("skew-pings-after", 0) > 0 {
            skew(&me, &pongs, to, args.num("skew-pings-after", 0), "after");
        }
    }

    // Run, then end the way the operator asked.
    let run = args.num("run-secs", 120);
    let end = Instant::now() + Duration::from_secs(run);
    while run == 0 || Instant::now() < end {
        thread::sleep(Duration::from_millis(100));
    }
    match args.get("end").unwrap_or("clean") {
        "abort" => {
            emit(&rs, "action", json!({"label": "end-abort"}));
            std::process::abort();
        }
        _ => {
            emit(&rs, "action", json!({"label": "end-clean"}));
            me.transport.shutdown();
            emit(&rs, "stop", json!({"clean": true}));
        }
    }
    drop(worker);
}

fn wait_for(limit: Duration, cond: impl Fn() -> bool) -> bool {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if cond() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    cond()
}

/// Opens one frame, applies the stand-in for the receiving core's expiry check, records the
/// native arrival independently of the verdict, and answers pings.
fn worker_frame(
    me: &Peer,
    key: &AgreementPrivateKey,
    pongs: &Mutex<Vec<Pong>>,
    native_us: i64,
    octets: &[u8],
) {
    let rs = me.role.to_string();
    let Some(opened) = sealing::open(key, octets) else {
        emit(
            &rs,
            "frame",
            json!({"verdict": "not-opened", "native_utc_us": native_us, "len": octets.len()}),
        );
        return;
    };
    let Some(inner) = Inner::decode(&opened.payload) else {
        emit(
            &rs,
            "frame",
            json!({"verdict": "not-ours", "native_utc_us": native_us}),
        );
        return;
    };
    if inner.to != me.role {
        return; // sealed for another device of this partition: not ours to judge
    }
    let opened_us = now_us();
    let expired = opened_us >= inner.deadline_us as i64;
    let verdict = if expired {
        "discard-expired"
    } else {
        "accepted"
    };
    emit(
        &rs,
        "frame",
        json!({"verdict": verdict, "tag": inner.tag, "seq": inner.seq, "from": inner.from.to_string(),
            "sent_utc_us": inner.sent_us, "deadline_utc_us": inner.deadline_us,
            "native_utc_us": native_us, "opened_utc_us": opened_us,
            "late_by_ms": (opened_us - inner.deadline_us as i64) / 1000}),
    );
    if expired {
        return;
    }
    match inner.tag.as_str() {
        "ping" => {
            // t1 = sent, t2 = native arrival; t3 is the reply's own sent time.
            me.send(
                inner.from,
                "pong",
                10_000,
                Duration::ZERO,
                [inner.sent_us, native_us as u64],
            );
        }
        "pong" => pongs.lock().unwrap().push(Pong {
            t1: inner.aux[0] as i64,
            t2: inner.aux[1] as i64,
            t3: inner.sent_us as i64,
            t4: native_us,
        }),
        _ => {}
    }
}

/// NTP-style exchange over the carrier: offset is the peer's clock minus this one's.
fn skew(me: &Peer, pongs: &Mutex<Vec<Pong>>, to: char, n: u64, phase: &str) {
    pongs.lock().unwrap().clear();
    for _ in 0..n {
        me.send(to, "ping", 10_000, Duration::ZERO, [0, 0]);
        thread::sleep(Duration::from_millis(250));
    }
    wait_for(Duration::from_secs(5), || {
        pongs.lock().unwrap().len() as u64 >= n
    });
    let rs = me.role.to_string();
    let samples: Vec<(i64, i64)> = pongs
        .lock()
        .unwrap()
        .iter()
        .map(|p| {
            (
                ((p.t2 - p.t1) + (p.t3 - p.t4)) / 2,
                (p.t4 - p.t1) - (p.t3 - p.t2),
            )
        })
        .collect();
    for (off, delay) in &samples {
        emit(
            &rs,
            "skew-sample",
            json!({"phase": phase, "offset_us": off, "delay_us": delay}),
        );
    }
    let best = samples.iter().min_by_key(|s| s.1);
    let abs_ms = best.map(|(o, _)| o.abs() / 1000);
    emit(
        &rs,
        "skew",
        json!({"phase": phase, "local": rs, "remote": to.to_string(), "pings": n,
            "samples": samples.len(), "best_offset_us": best.map(|s| s.0),
            "best_delay_us": best.map(|s| s.1), "abs_skew_ms": abs_ms,
            "limit_ms": SKEW_LIMIT_MS,
            "pass": abs_ms.map(|a| a <= SKEW_LIMIT_MS),
            "meaning": "offset = remote clock minus local clock"}),
    );
}

// ---- negative controls -----------------------------------------------------------------

fn negative(args: &Args) {
    let role = args.role("role");
    let rs = role.to_string();
    let case = args.need("case");
    let real = creds(args, "b");
    let rogue = |n: &str| creds(args, n);
    let files = match case {
        // CA-valid chain from a different authority: the listener refuses the client.
        "wrong-ca" => CertificateFiles {
            trusted_roots: real.trusted_roots.clone(),
            ..rogue("neg-wrong-ca")
        },
        // The right certificate with a private key that does not belong to it.
        "wrong-key" => CertificateFiles {
            certificate: real.certificate.clone(),
            trusted_roots: real.trusted_roots.clone(),
            ..rogue("neg-wrong-key")
        },
        // Normal client credentials; host A must serve with its neg-wrong-san credential.
        "wrong-san" => real,
        // CA-valid, but its CN is not on host A's allow list.
        "unlisted-cn" => CertificateFiles {
            trusted_roots: real.trusted_roots.clone(),
            ..rogue("neg-unlisted")
        },
        _ => fail(&rs, "unknown --case"),
    };
    let me = Peer {
        role,
        transport: PeerTransport::new(),
        seq: AtomicU64::new(1),
        t0: Instant::now(),
    };
    // Is the listener there at all? A refusal only counts as native authentication when it
    // is: the host answered on TCP, and the failure is the native open (not a setup fault).
    let reachable = args
        .addrs("connect")
        .first()
        .is_some_and(|a| std::net::TcpStream::connect_timeout(a, Duration::from_secs(3)).is_ok());
    let started = me
        .transport
        .start(&device(role), configuration(args, files).wrap());
    let started_ok = started.is_ok();
    let error = started.as_ref().err().map(ToString::to_string);
    // The transport's own message for a native open that failed; anything else (storage,
    // configuration) is a setup fault and proves nothing about authentication.
    let native_refusal = error
        .as_deref()
        .is_some_and(|e| e.ends_with(NATIVE_OPEN_FAILED));
    let mut probes = 0u64;
    if started_ok && case == "unlisted-cn" {
        let to = args
            .get("to")
            .map_or('a', |s| s.chars().next().unwrap_or('a'));
        for _ in 0..args.num("probes", 5) {
            me.send(to, "neg-probe", 10_000, Duration::ZERO, [0, 0]);
            probes += 1;
            thread::sleep(Duration::from_millis(300));
        }
        thread::sleep(Duration::from_secs(2));
    }
    if started_ok {
        me.transport.shutdown();
    }
    // The first three must fail native authentication (start refused by the native open,
    // with the listener reachable). The unlisted CN may link, but host A's evidence must
    // then show no accepted frame tagged neg-probe; a setup fault is never a pass.
    let pass = if case == "unlisted-cn" {
        Value::Null
    } else {
        json!(!started_ok && reachable && native_refusal)
    };
    emit(
        &rs,
        "negative",
        json!({"case": case, "start_ok": started_ok, "error": error, "probes_sent": probes,
            "listener_reachable": reachable, "native_refusal": native_refusal,
            "setup_fault": !started_ok && !native_refusal,
            "expected": if case == "unlisted-cn" { "link may open; no frame carried" } else { "start refused by native authentication" },
            "pass": pass, "needs_peer_evidence": case == "unlisted-cn"}),
    );
}

// ---- main ------------------------------------------------------------------------------

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let cmd = argv.first().map_or("help", String::as_str);
    if matches!(cmd, "help" | "--help" | "-h") {
        print!("{USAGE}");
        return;
    }
    // Opt-in first: nothing below this line opens a listener or writes a file.
    if std::env::var("OAC_TEST_LAN").as_deref() != Ok("1") {
        eprintln!(
            "lan_leg: refused: this driver listens on a LAN address on purpose; set OAC_TEST_LAN=1 to opt in"
        );
        std::process::exit(3);
    }
    let args = Args::parse(&argv[1..]);
    match cmd {
        "env" => env_evidence(args.get("role").unwrap_or("-"), &args),
        "issue" => {
            env_evidence("a", &args);
            issue(&args);
        }
        "import" => {
            env_evidence("b", &args);
            import(&args);
        }
        "peer" => {
            env_evidence(&args.role("role").to_string(), &args);
            peer(&args);
        }
        "negative" => {
            env_evidence(&args.role("role").to_string(), &args);
            negative(&args);
        }
        other => fail("-", &format!("unknown command {other}; try help")),
    }
}
