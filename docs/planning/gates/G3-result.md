### G3 zenoh-peer

- **Gate id:** G3
- **Pinned version(s):** Zenoh core `1.10.1`. This matches `docs/planning/PINS.md` and meets
  the `>= 1.10.0` floor.
  - **Binding used:** the spike did not use the Rust crate. It used the Python binding
    `eclipse-zenoh==1.10.1` from PyPI:
    - Windows: `eclipse_zenoh-1.10.1-cp39-abi3-win_amd64.whl`
    - Linux: `eclipse_zenoh-1.10.1-cp39-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl`
  - **Same-core evidence:** zenoh-python's `Cargo.toml` at tag `1.10.1` (tag commit
    `630a02cb93bd292b2923db72ad58f4743a014c38`) depends on
    `zenoh = { version = "1.10.1", … branch = "release/1.10.1" }`. The wheel's own
    error path shows its core checkout is `1211779`, and GitHub's compare of
    `1.10.1…1211779c` reports **identical**. So the wheel runs the tag-`1.10.1` Zenoh core,
    as a native in-process peer and not as a router client.
  - **Why the Python binding:** on 2026-09-25 neither platform had a Rust toolchain, and the
    WSL Ubuntu had no C linker. `sudo` there needs a password, so a Rust build on the Linux
    leg would have needed the operator to install `build-essential`. The wheel installs
    without sudo on both platforms.
    - G3 tests Zenoh *discovery and connectivity behavior*, which lives in the shared core.
    - Spike code is throwaway (oac-gates "Throwaway rule"), so language does not carry
      forward.
  - **What the substitution does not prove.** All three items are open UNVERIFIED, below:
    - That the Rust `zenoh` crate behaves the same under the feature set and defaults OAC
      will choose. The wheel enables `shared-memory` and `zenoh-ext/unstable`. The spike
      called no `zenoh-ext` API, so no unstable behavior sits behind any pass criterion.
    - That Zenoh 1.10.1 builds with the pinned Rust toolchain `1.98.1`.
    - That Zenoh behaves the same when embedded in the OAC binary's own async runtime.
    - It also says nothing about the 5-15 MB binary-size estimate.
- **Platforms:**
  - Windows 11, native, Python 3.13.1.
  - Linux: Ubuntu 24.04.1 LTS on WSL2, kernel `6.18.33.2-microsoft-standard-WSL2`, Python
    3.12.3. WSL2 runs a real Linux kernel, and every link ran on interface `lo`. Bare-metal
    Linux was not exercised.
  - **macOS: not run.** No macOS host was available, and the operator parked this leg on
    2026-09-25.
- **Date:** 2026-09-25
- **Timebox:** 90 minutes, declared at 06:55:01 UTC before any install or run. Live work ran
  06:55:01–07:01:24 UTC, about 6.5 minutes, so the box did not expire. About 15 minutes of
  documentation and source research came first.
- **Command transcript summary:**
  - **Install:** `eclipse-zenoh==1.10.1` went into a venv on each platform. WSL needed no
    sudo: `python3 -m venv --without-pip` plus `get-pip.py`.
  - **Certificates:** a throwaway test CA and one peer certificate were generated with
    OpenSSL 3.0.13. The certificate has SAN `IP:127.0.0.1, DNS:localhost` and expires after
    7 days. It was never committed.
  - **Peer process:** each peer ran as a **separate OS process**
    (`peer.py.throwaway-quarantined`). Each one:
    - opens a peer-mode session
    - subscribes to `oac/g3/**`
    - publishes its name every 100 ms
    - records the time until `session.info.peers_zid()` shows the expected peers, the time
      until data arrived from all of them, and its link locators
  - **Smoke test:** a single Windows `mcast-tcp` run passed at 06:57 UTC. It is kept in
    `results-windows-smoke/` and is **not counted** in the matrix.
  - **Matrix:** `run_matrix.py.throwaway-quarantined` ran 6 scenarios × 3 repetitions per
    platform. Every run was a cold start with fresh processes.

    | Scenario | Multicast | Endpoints | TLS | Peers |
    |---|---|---|---|---|
    | `mcast-tcp` | on | `tcp/127.0.0.1:0` | no | 2 |
    | `mcast-tls` | on | `tls/127.0.0.1:0`, `transport.link.protocols: ["tls"]` | yes | 2 |
    | `rdv-tcp` | off | fixed rendezvous `tcp/127.0.0.1:17447` | no | 2 |
    | `rdv-tls` | off | fixed rendezvous, TLS | yes | 2 |
    | `mcast-tcp-3` | on | `tcp/127.0.0.1:0` | no | 3 |
    | `rdv-tcp-3` | off | one rendezvous, two joiners | no | 3 |

  - **Extras:** the negative control, the `#iface=` probes, and the scouting-socket binds
    with three live peers. Scripts: `extras.py.throwaway-quarantined`,
    `run_windows_extras.sh.throwaway-quarantined` and
    `run_linux_extras.sh.throwaway-quarantined`.
  - **Results:** 36 of 36 matrix runs passed, 18 per platform. Per-peer results are in
    `docs/planning/gates/fixtures/g3-zenoh-peer/results-windows/` and
    `.../results-linux-wsl2/`. `verify.py.throwaway-quarantined` checked the links.
- **Pass criteria evaluated, per platform.** The G3 reference requires per-platform results,
  not a blended verdict.

  | Criterion | Windows 11 | Linux (WSL2 Ubuntu 24.04) | macOS |
  |---|---|---|---|
  | 1. Loopback discovery with UDP multicast scouting (`224.0.0.224:7446`, `interface: "auto"`), Zenoh `>= 1.10.0` | **PASS**: 9/9 multicast runs (`mcast-tcp`, `mcast-tls`, `mcast-tcp-3`) | **PASS**: 9/9 | NOT RUN |
  | 2. Discovery with multicast disabled, using a locally shared rendezvous endpoint | **PASS**: 9/9 rendezvous runs (`rdv-tcp`, `rdv-tls`, `rdv-tcp-3`) | **PASS**: 9/9 | NOT RUN |
  | 3. TLS listener bound to `127.0.0.1` works in both modes | **PASS**: `mcast-tls` 3/3 and `rdv-tls` 3/3, all links `tls/127.0.0.1:*` | **PASS**: 3/3 and 3/3 | NOT RUN |
  | 4. Multiple peers per host do not collide on ports | **PASS**: `mcast-tcp-3` and `rdv-tcp-3`, 3/3 each | **PASS**: 3/3 each | NOT RUN |

  - **Criterion 1 evidence:** every multicast run bound its listeners to `127.0.0.1` only,
    with dynamic ports. The peers still found each other through scouting and built a direct
    `tcp/127.0.0.1:A → tcp/127.0.0.1:B` link.
    - **Scope of this finding:** loopback discovery works on 1.10.1 with explicit
      `127.0.0.1` listeners on Windows and Linux (WSL2).
    - This spike did **not** reproduce the pre-1.10.0 failure modes that
      eclipse-zenoh/zenoh PR #2671 fixed (https://github.com/eclipse-zenoh/zenoh/pull/2671,
      shipped in 1.10.0 as "Fix loopback multicast scouting and autoconnect"). Those were
      macOS rejecting Scout sends from a loopback-bound socket, and unspecified (`0.0.0.0`)
      listeners on loopback-only hosts that did not advertise loopback locators in Hello.
    - No pre-1.10.0 control run was made. The listeners were explicit `127.0.0.1`, and
      neither host is loopback-only.
  - **Criterion 2 evidence:** the rendezvous runs used
    `scouting/multicast/enabled: false` with a fixed rendezvous endpoint, and every run
    connected.
    - **Negative control:** the same setting with no rendezvous discovered nothing on
      either platform (`negative-mcast-off-no-rdv-*.json`: `ok: false`, `peers_seen: []`,
      `links: []`). So the rendezvous passes do not quietly depend on scouting.
    - In `rdv-tcp-3`, joiners b and c each dialed only the rendezvous peer a. They still
      built a **direct b↔c link** by gossip in all three repetitions on both platforms.
  - **Criterion 3 evidence:** the TLS runs set `transport.link.protocols: ["tls"]`, and every
    link they recorded is `tls/127.0.0.1:*`. `verify_name_on_connect` was left at its
    default, `true` (`DEFAULT_CONFIG.json5` at 1.10.1), so the certificate's SAN
    `IP:127.0.0.1` was actually checked.
  - **Criterion 4 evidence:** in every 3-peer run, each peer listened on its own port and
    the three formed a full mesh (a↔b, a↔c, b↔c) with no bind error.
    - Three live peers shared the scouting socket on each platform. Preserved verbatim in
      `scouting-socket-binds.txt`:
      - Windows: `netstat` showed three processes, each bound to `UDP 0.0.0.0:7446`.
      - Linux: `ss` showed three processes, each bound to `224.0.0.224:7446`, the group
        address.
    - All three peers discovered each other on both platforms.
- **Verdict:** **NOT RUN at gate level.** Per platform:
  - Windows 11: **PASS**
  - Linux (WSL2): **PASS**
  - macOS: **NOT RUN** (parked; no host)
  - Criteria 1-3 each name all three platforms, and oac-gates does not let a gate pass on a
    majority, so a plain PASS would claim an unobserved macOS result.
  - G3 becomes PASS only after a macOS leg runs the same matrix.
    `run_matrix.py.throwaway-quarantined` runs unchanged there with the `eclipse-zenoh`
    macOS wheel.
  - For downstream planning: no platform has recorded a FAIL, and the primary multicast path
    passed everywhere it ran.
- **Fallback taken:** none. The primary multicast path passed on both platforms that ran.
  The fallback (fixed local endpoint, scouting disabled) was also exercised as criterion 2
  and passes. It is a proven alternative operating mode, but it was not a needed fallback.
- **Measured cold-start discovery latency.** Measured from just before `zenoh.open`, with
  fresh processes. n counts peer results over 3 repetitions. Values are min / median / max
  in ms.

  | Scenario | Windows: discovery | Windows: data from all peers | Linux (WSL2): discovery | Linux: data from all peers |
  |---|---|---|---|---|
  | `mcast-tcp` (n=6) | 47 / 51 / 72 | 58 / 144 / 158 | 6 / 7 / 9 | 107 / 108 / 110 |
  | `mcast-tls` (n=6) | 54 / 62 / 72 | 69 / 115 / 166 | 18 / 19 / 23 | 118 / 120 / 123 |
  | `rdv-tcp` (n=6) | 60 / 104 / 108 | 75 / 95 / 195 | 7 / 104 / **1011** | 7 / 102 / **1015** |
  | `rdv-tls` (n=6) | 52 / 84 / 110 | 60 / 82 / 108 | 16 / 63 / 109 | 18 / 63 / 109 |
  | `mcast-tcp-3` (n=9) | 76 / 154 / 169 | 142 / 157 / 189 | 6 / 7 / 9 | 12 / 108 / 110 |
  | `rdv-tcp-3` (n=9) | 108 / 113 / 211 | 127 / 166 / 208 | 9 / 10 / 110 | 9 / 109 / 113 |

  - **Poll-interval artifact:** peers poll every 100 ms, so values near 100 ms or its
    multiples mostly reflect that interval, not Zenoh. The floors are more informative.
    On Windows, `zenoh.open` alone takes about 47 ms (median).
  - **The 1011 ms outlier** (Linux `rdv-tcp`, repetition 1) is **attributed, by inference
    from timings, to a start race.** No logs were captured.
    - Peer b's `t_open_ms` was 502.7 and its discovery came at about 1011 ms. That fits a
      first dial that reached the rendezvous port before peer a had bound it, followed by
      the default retry (`connect.retry.period_init_ms: 1000`, `DEFAULT_CONFIG.json5` at
      1.10.1).
    - If that reading is right, any design where a joiner can start before the rendezvous
      listener can hit the same ~1 s cold start. The Stage 4 daemon design should account
      for it.
- **UNVERIFIED items:**
  - **Confirmed:** loopback discovery works on Zenoh 1.10.1 with explicit `127.0.0.1`
    listeners on Windows and Linux (WSL2). This does **not** confirm the specific PR #2671
    failure modes, which were not exercised (see criterion 1).
  - **Confirmed:** `#iface=` is enforced on Linux. `tcp/127.0.0.1:0#iface=lo` works, and
    `#iface=bogus0` fails with `ZError: Can not create a new TCP listener bound to
    tcp/127.0.0.1:0#iface=bogus0: … "No such device"` (errno 19).
  - **Observed on Windows, narrower:** only a *nonexistent* interface name (`lo`) was tried.
    It was accepted **without exception**, and the link bound on the loopback interface
    anyway.
    - Not tested: behavior with a *valid* Windows interface name, and whether Zenoh logs a
      warning (Zenoh logging was not initialized and peer output was not kept).
    - Consequence, still justified: do not rely on `#iface=` as a containment control on
      Windows, because an invalid value raises no error.
  - **Recorded:** the scouting socket binds differently by platform. Windows binds
    `0.0.0.0:7446` and Linux binds `224.0.0.224:7446`. Several processes shared it on both,
    with no collision (`scouting-socket-binds.txt`).
  - **Recorded:** after the first Windows multicast run, the operator was asked whether a
    Windows Defender Firewall prompt had appeared. No answer came before this result was
    written. Loopback discovery passed regardless.
  - **New, UNVERIFIED:** G3 criteria via the Rust `zenoh` crate built with toolchain
    `1.98.1`, with OAC's chosen feature set, embedded in the OAC runtime. See "What the
    substitution does not prove".
  - **Still open:** macOS results for all four criteria. macOS is also the primary target of
    PR #2671's loopback-TX fix.
  - **Still open:** bare-metal Linux. The WSL2 result is taken as the Linux leg, as the
    operator directed.
  - **Still open, not closed by this spike:** the 5-15 MB binary-size estimate. No Rust
    artifact was built; the owner is task I3.
  - **Still open, out of G3 scope and unchanged:** Zenoh `auth.pubkey` semantics.
- **Fixtures captured** in `docs/planning/gates/fixtures/g3-zenoh-peer/`:
  - `configs/`: six single-role configs:
    - `multicast-tcp`
    - `multicast-tls`
    - `rendezvous-{tcp,tls}-listener`
    - `rendezvous-{tcp,tls}-joiner`
    - **Provenance:** the values are **derived from zenoh 1.10.1 `Config.get_json` output
      and annotated.** Each file has a hand-added `_fixture` block (role, pinned version,
      platforms, capture date, notes on unset keys) and `<CERTS>` placeholder paths.
    - To load one, remove `_fixture` and set real certificate paths.
    - One file per peer role, so a rendezvous listener and a joiner are separate files.
  - `results-windows/`, `results-linux-wsl2/`: every per-peer result JSON (zid, locators,
    ports, timings) plus the extras.
  - `results-windows-smoke/`: the uncounted smoke run.
  - `scouting-socket-binds.txt`: the verbatim `netstat` and `ss` lines.
  - The quarantined throwaway scripts (`*.throwaway-quarantined`), with the username redacted
    from the WSL scripts' paths.
  - **No key material was committed.** A scan of the committed fixtures found no username,
    hostname, or `PRIVATE KEY` block.
  - Per the G3 reference, this gate does not feed the Stage 3 fake *harness* endpoints. These
    configs serve Stage 3 transport contract tests that need a Zenoh topology sample.
- **Pin rows relied on:** `Zenoh`, `Rust toolchain`.
  - `Rust toolchain` stays on this list on purpose. The gate is defined against the Rust
    build (`docs/planning/PINS.md` Rust toolchain record: "G3 (build must succeed to run
    the Zenoh spike)"). A re-run that satisfies the open Rust-crate item above will rely
    on it.
  - This run substituted the wheel, so the toolchain pin was **not exercised** here.
  - The wheel has no row of its own in PINS.md. This gate result is tied to the Zenoh
    **core** tag `1.10.1`. A different wheel version must not be used for a G3 re-run
    unless it is verified to wrap the pinned core tag.
- **PINS.md as-of:** 2026-09-16, commit `a44a7ed7278caab383f264f5bc8e12bb5c8c73e9`
- **Process notes (for future gate spikes, not part of the pass/fail record):**
  - Docs and source came before code. First, the pinned `DEFAULT_CONFIG.json5` at tag 1.10.1
    and the TLS manual page were fetched. Then the binding's core version was confirmed at
    its tag, and the installed API was introspected before `peer.py` was written. The
    timebox was declared before the first install.
  - **Slip, contained:** the first WSL certificate-generation command ran in the
    **repository root** instead of the scratch directory.
    - It wrote a throwaway test CA and peer **private keys** as untracked files in
      `C:\sources\OAC`.
    - They were moved to scratch within minutes. `git status` showed nothing staged, and
      `git log --all` on every branch shows no `.key`, `.pem` or `.crt` file ever committed.
    - **Root cause:** `wsl.exe -- bash -lc '<script>'` hands the joined arguments to a login
      shell that re-parses them. The single quotes were lost, `$D` expanded to empty, and
      `cd ""` left the shell in the Windows working directory.
    - My first diagnosis, MSYS path conversion, was wrong. `MSYS_NO_PATHCONV=1` did not fix
      it, and the second occurrence was caught by printing `pwd` before any write.
    - **Fix:** run a script file with `wsl.exe --exec bash <file>`, which re-parses nothing,
      and assert `pwd` before writing anything.
- **Re-run history:**

  | Date | Pinned versions | Verdict | Invalidated by |
  |---|---|---|---|
  | 2026-09-25 | Zenoh core 1.10.1 (Python binding `eclipse-zenoh` 1.10.1, core = tag `1.10.1`) | NOT RUN at gate level: Windows 11 PASS, Linux (WSL2) PASS, macOS NOT RUN | — |
