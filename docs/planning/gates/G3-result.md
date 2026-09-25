### G3 zenoh-peer

- **Gate id:** G3
- **Pinned version(s):** Zenoh `1.10.1`, which matches `docs/planning/PINS.md` and meets the
  `>= 1.10.0` floor.
  - **Binding:** the spike did **not** use the Rust crate. It used the Python binding
    `eclipse-zenoh==1.10.1` from PyPI:
    - Windows wheel: `eclipse_zenoh-1.10.1-cp39-abi3-win_amd64.whl`.
    - Linux wheel: `eclipse_zenoh-1.10.1-cp39-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl`.
  - **Core version:** zenoh-python's `Cargo.toml` at tag `1.10.1` (tag commit
    `630a02cb93bd292b2923db72ad58f4743a014c38`) builds on
    `zenoh = { version = "1.10.1", … branch = "release/1.10.1" }`. It is a native
    in-process peer on the same Zenoh core version, not a router client.
  - **Build features:** the wheel enables `zenoh-ext/unstable`. The spike called no
    `zenoh-ext` API, so no unstable behavior sits behind any pass criterion.
  - **Rust toolchain:** not exercised. The Rust toolchain pin (`1.98.1`) is not
    relevant to this run.
- **Platforms:**
  - **Windows 11:** native, Python 3.13.1.
  - **Linux:** Ubuntu 24.04.1 LTS on WSL2, kernel `6.18.33.2-microsoft-standard-WSL2`,
    Python 3.12.3. WSL2 runs a real Linux kernel, and every link ran on interface `lo`.
    Bare-metal Linux was not exercised.
  - **macOS:** not run; there was no macOS host (parked by the operator, 2026-09-25).
- **Date:** 2026-09-25
- **Timebox:** 90 minutes, declared at 06:55:01 UTC, before any install or run.
  - **Live work:** 06:55:01 to 07:01:24 UTC, about 6.5 minutes. Not expired.
  - **Before the box:** about 15 minutes of documentation and source research.
- **Command transcript summary:**
  - **Install:** `eclipse-zenoh==1.10.1` went into a venv on each platform. The WSL
    install needed no sudo: `python3 -m venv --without-pip` plus `get-pip.py`.
  - **Certificates:** a throwaway test CA and one peer certificate were generated with
    OpenSSL 3.0.13. The certificate has SAN `IP:127.0.0.1, DNS:localhost` and a 7-day
    expiry. It was never committed.
  - **Test design:** each peer is a **separate OS process**
    (`peer.py.throwaway-quarantined`). A peer opens a peer-mode session, subscribes to
    `oac/g3/**`, publishes its own name every 100 ms, and records three things: the time
    until `session.info.peers_zid()` shows the expected peers, the time until data has
    arrived from every one of them, and its link locators.
  - **Matrix:** `run_matrix.py.throwaway-quarantined` ran 6 scenarios × 3 repetitions on
    each platform, all cold start, each with fresh processes:

    | Scenario | Multicast | Endpoints | TLS | Peers |
    |---|---|---|---|---|
    | `mcast-tcp` | on | `tcp/127.0.0.1:0` | no | 2 |
    | `mcast-tls` | on | `tls/127.0.0.1:0`, `transport.link.protocols: ["tls"]` | yes | 2 |
    | `rdv-tcp` | off | fixed rendezvous `tcp/127.0.0.1:17447` | no | 2 |
    | `rdv-tls` | off | fixed rendezvous, TLS | yes | 2 |
    | `mcast-tcp-3` | on | `tcp/127.0.0.1:0` | no | 3 |
    | `rdv-tcp-3` | off | one rendezvous, two joiners | no | 3 |

  - **Extras:** `extras.py.throwaway-quarantined` ran a negative control, `#iface=` probes,
    and a check of the scouting socket bind with three live peers.
  - **Results:** 36 of 36 matrix runs passed (18 per platform).
    - Per-peer results: `docs/planning/gates/fixtures/g3-zenoh-peer/results-windows/` and
      `.../results-linux-wsl2/`.
    - Link checks by `verify.py.throwaway-quarantined`: the TLS runs carry only `tls/`
      links, and each 3-peer run forms a full mesh.
- **Pass criteria evaluated, per platform** (the G3 reference requires per-platform results,
  not a blended verdict):

  | Criterion | Windows 11 | Linux (WSL2 Ubuntu 24.04) | macOS |
  |---|---|---|---|
  | 1. Loopback discovery with UDP multicast scouting (`224.0.0.224:7446`, `interface: "auto"`), Zenoh `>= 1.10.0` | **PASS** — 9/9 multicast runs (`mcast-tcp`, `mcast-tls`, `mcast-tcp-3`) | **PASS** — 9/9 | NOT RUN |
  | 2. Discovery with multicast disabled, using a locally shared rendezvous endpoint | **PASS** — 9/9 rendezvous runs (`rdv-tcp`, `rdv-tls`, `rdv-tcp-3`) | **PASS** — 9/9 | NOT RUN |
  | 3. TLS listener bound to `127.0.0.1` works in both modes | **PASS** — `mcast-tls` 3/3 and `rdv-tls` 3/3, all links `tls/127.0.0.1:*` | **PASS** — 3/3 and 3/3 | NOT RUN |
  | 4. Multiple peers per host do not collide on ports | **PASS** — `mcast-tcp-3` and `rdv-tcp-3`, 3/3 each | **PASS** — 3/3 each | NOT RUN |

  - **Criterion 1 evidence:** every multicast run bound its listeners to `127.0.0.1` only,
    with dynamic ports. The peers still found each other through scouting and built a
    direct `tcp/127.0.0.1:A → tcp/127.0.0.1:B` link. This is the same-host loopback case
    that was broken before Zenoh 1.10.0 (PR #2671), and it now works on 1.10.1 on both
    platforms.
  - **Criterion 2 evidence:** peers were configured with `scouting/multicast/enabled: false`.
    - With a fixed rendezvous endpoint, every run connected.
    - **Negative control:** the same setting *without* a rendezvous never discovered a
      peer on either platform (`negative-mcast-off-no-rdv-*.json`). So the rendezvous
      passes do not secretly depend on scouting.
    - In `rdv-tcp-3`, the two joiners b and c each dialed only the rendezvous peer a.
      They still built a **direct b↔c link** by gossip, so a does not need to relay
      between them.
  - **Criterion 3 evidence:** the TLS runs set `transport.link.protocols: ["tls"]`, and every
    link recorded is `tls/127.0.0.1:*`. `verify_name_on_connect` kept its default (`true`),
    so the certificate's SAN `IP:127.0.0.1` was actually checked.
  - **Criterion 4 evidence:**
    - In every 3-peer run, each peer listened on its own port. The peers formed a full
      mesh (a↔b, a↔c, b↔c) with no bind error.
    - Scouting socket bind with three live peers on each platform:
      - **Windows:** `netstat` showed three processes each bound to `UDP 0.0.0.0:7446` at
        the same time. This matches the documented `SO_REUSEADDR` sharing.
      - **Linux:** `ss` showed three processes each bound to `224.0.0.224:7446`, the
        multicast group address, not `0.0.0.0`.
    - All three peers discovered each other on both platforms, so there was no collision.
- **Verdict:** **NOT RUN at gate level.** The per-platform results are Windows 11 **PASS**,
  Linux (WSL2) **PASS**, and macOS **NOT RUN**, parked because no macOS host was available.
  - **Why not PASS:** criteria 1-3 each name all three platforms. oac-gates does not let a
    gate pass on a majority. A plain PASS would claim a macOS result that nobody observed.
  - **When G3 becomes PASS:** only after a macOS leg runs this same matrix
    (`run_matrix.py.throwaway-quarantined` runs unchanged on macOS with the `eclipse-zenoh`
    macOS wheel).
  - **For downstream planning:** no platform has recorded a FAIL. The primary multicast path
    passed wherever it was run.
- **Fallback taken:** none. The primary multicast path passed on both platforms that ran.
  The fallback (fixed local endpoint, scouting disabled) was also exercised as criterion 2
  and passes. That makes it a proven alternative operating mode, not a path that was needed.
- **Measured cold-start discovery latency** (from just before `zenoh.open`, fresh processes,
  n = peer results over 3 repetitions; min / median / max in ms):

  | Scenario | Windows: discovery | Windows: data from all peers | Linux (WSL2): discovery | Linux: data from all peers |
  |---|---|---|---|---|
  | `mcast-tcp` (n=6) | 47 / 51 / 72 | 58 / 144 / 158 | 6 / 7 / 9 | 107 / 108 / 110 |
  | `mcast-tls` (n=6) | 54 / 62 / 72 | 69 / 115 / 166 | 18 / 19 / 23 | 118 / 120 / 123 |
  | `rdv-tcp` (n=6) | 60 / 104 / 108 | 75 / 95 / 195 | 7 / 104 / **1011** | 7 / 102 / **1015** |
  | `rdv-tls` (n=6) | 52 / 84 / 110 | 60 / 82 / 108 | 16 / 63 / 109 | 18 / 63 / 109 |
  | `mcast-tcp-3` (n=9) | 76 / 154 / 169 | 142 / 157 / 189 | 6 / 7 / 9 | 12 / 108 / 110 |
  | `rdv-tcp-3` (n=9) | 108 / 113 / 211 | 127 / 166 / 208 | 9 / 10 / 110 | 9 / 109 / 113 |

  **Caveats for the risk register:**
  - **Poll resolution:** peers poll every 100 ms. Values near 100 ms or multiples of it
    mostly reflect poll timing, not Zenoh latency. The latency floors are more
    informative. On Windows, `zenoh.open` itself takes about 47 ms (median).
  - **The 1011 ms outlier** (Linux `rdv-tcp`, rep 1) is a **start race**. Peer b's first
    dial to the rendezvous port arrived before peer a had bound it. b's `open` blocked
    about 500 ms, and it then connected on the default retry schedule
    (`connect.retry.period_init_ms: 1000`, `DEFAULT_CONFIG.json5` at 1.10.1). This is a
    real cold-start hazard for any design where a joiner can start before the rendezvous
    listener. The Stage 4 daemon design should account for it.
- **UNVERIFIED items:**
  - **Confirmed empirically, previously asserted only from the changelog:** same-host
    loopback discovery works on Zenoh `>= 1.10.0` (PR #2671). It was shown on 1.10.1 on
    Windows and Linux (WSL2). macOS is not observed.
  - **Confirmed:** `#iface=` is enforced on Linux and ignored on Windows.
    - On Linux, `tcp/127.0.0.1:0#iface=lo` works. `#iface=bogus0` fails with `ZError: Can
      not create a new TCP listener bound to tcp/127.0.0.1:0#iface=bogus0: … "No such
      device"` (errno 19).
    - On Windows, `#iface=lo` names an interface Windows does not have, yet it was
      **silently accepted and ignored**: the peers connected normally.
    - Consequence: do not use `#iface=` as a security or containment control on Windows.
      It restricts nothing there, and there is no error to warn you. macOS is not tested.
  - **Recorded:** the scouting socket binds differently per platform. Windows binds
    `0.0.0.0:7446`; Linux binds `224.0.0.224:7446`. Both allow shared binding by several
    processes, and no collision was observed on either.
  - **Recorded:** the operator was asked whether a Windows Defender Firewall prompt appeared
    during the first Windows multicast run. No answer was received before this result was
    written. Loopback discovery passed regardless.
  - **Still open (out of G3 scope, unchanged):** Zenoh `auth.pubkey` semantics.
  - **Still open, not closed by this spike:** the 5-15 MB binary size estimate. The spike
    used the Python wheel, not a Rust binary, so it measures nothing about the size of the
    Rust artifact.
  - **Still open:** macOS results for all four criteria.
  - **New:** bare-metal Linux was not exercised. WSL2 runs a real Linux kernel, the links
    ran on `lo`, and `SO_BINDTODEVICE`-style `#iface=` enforcement behaved like Linux. So
    the WSL2 result is taken as the Linux leg, as the operator directed.
- **Fixtures captured:** `docs/planning/gates/fixtures/g3-zenoh-peer/`
  - `configs/`: four mode-combination configs (`multicast-tcp`, `multicast-tls`,
    `rendezvous-tcp`, `rendezvous-tls`). They were generated by the Zenoh 1.10.1 library's own
    `Config.get_json`, and each carries the pinned version, the platforms, and the capture
    date. Certificate paths are `<CERTS>` placeholders.
  - `results-windows/`, `results-linux-wsl2/`: every per-peer result JSON (zid, locators,
    ports, timings) plus the extras.
  - The quarantined throwaway scripts (`*.throwaway-quarantined`). The username has been
    redacted from the WSL scripts' paths.
  - **Not committed:** no key material. The test CA and peer keys stayed in the uncommitted
    spike scratch. A committed-fixture scan found no username, hostname, or `PRIVATE KEY`
    block.
  - The G3 reference says this gate does not feed the Stage 3 fake *harness* endpoints.
    These configs are for Stage 3's transport contract tests, if those need a Zenoh
    topology sample.
- **Pin rows relied on:** `Zenoh`, `Rust toolchain` (the Rust toolchain row was not exercised;
  see "Pinned version(s)")
- **PINS.md as-of:** 2026-09-16, commit `a44a7ed7278caab383f264f5bc8e12bb5c8c73e9`
- **Process notes (for future gate spikes, not part of the pass/fail record):**
  - Docs and source came before any code. The pinned `DEFAULT_CONFIG.json5` was fetched
    at tag 1.10.1, and the TLS manual page was read. The binding's core version was
    confirmed at its tag. The installed API was introspected before `peer.py` was written.
    The timebox was declared before the first install.
  - **Slip, contained:** the first WSL certificate-generation command ran in the **repository
    root** instead of the scratch directory. It wrote a throwaway test CA and peer
    **private keys** as untracked files in `C:\sources\OAC`. They were moved to the scratch
    directory within minutes, and `git status` confirmed nothing was staged or committed.
    - Root cause: `wsl.exe -- bash -lc '<script>'` passes the joined arguments to a login
      shell that re-parses them. That dropped the single quotes and expanded `$D` to an
      empty string, and `cd ""` left the shell in the Windows working directory.
    - My first diagnosis was MSYS path conversion. That was wrong: setting
      `MSYS_NO_PATHCONV=1` did not fix it, and the same empty-variable failure recurred on
      the next run. That second occurrence was caught by printing `pwd` before anything
      was written.
    - Fix: run a script file with `wsl.exe --exec bash <file>`, which re-parses nothing,
      and assert `pwd` before any write.
- **Re-run history:**

  | Date | Pinned versions | Verdict | Invalidated by |
  |---|---|---|---|
  | 2026-09-25 | Zenoh 1.10.1 (Python binding `eclipse-zenoh` 1.10.1; core `release/1.10.1`) | NOT RUN at gate level — Windows 11 PASS, Linux (WSL2) PASS, macOS NOT RUN | — |
