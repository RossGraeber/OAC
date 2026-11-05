---
name: oac-lan-leg
description: Runbook for the two-host LAN leg (G3 #64 steps 1-7, #63 step 8): host roles, driver commands, evidence, cleanup, fail-closed rules. Load when an operator asks to run the LAN test on real hosts, or when touching tools/lan-leg or the lan_leg example.
---

Operator-run, opt-in, never part of `local-ci`. The requirements are the steps of
`tests/integration/README.md` "G3 native TLS two-host leg (#64)"; this skill is only how to
run them consistently. Load `oac-gates` (result template, never "probably") and `oac-zenoh`
(pin, trust rule) with it. No live harness, no Codex daemon, no model API, no sign-in.

## How the operator invokes it

Open one Claude session (or Codex, which reads the identical `.agents/skills/` copy) in a
checkout of the same commit on each machine, and say on host A: "Load the oac-lan-leg skill
and run the LAN leg as host A", on host B: "... as host B". The sessions never talk to each
other: the operator relays the values in the table below.

## Fail-closed rules (read first, obey always)

1. Claude never changes a firewall, network, routing, time-sync, power or other system
   setting. It prints the exact command (runbook "Operator commands") and waits for the
   operator to say it ran. The same goes for removing what the operator added.
2. Claude never signs in, never reads `auth.json`, keyrings, `*.sqlite`, rollout files or
   any harness configuration, never touches the operator's real OAC keys or data directories
   (`OAC/relay-tls`, the real state dir), and never runs Claude Code or Codex for this leg.
   State goes only under the work directory `DIR` below, which is new and outside the repo.
3. `OAC_TEST_LAN=1` is the opt-in for listening on a LAN address. At the start, tell the
   operator which `IP:PORT` TLS listeners the session will open (and, for step 4 only, the
   UDP 7446 scouting socket, rule 4), and set it only after they agree. The driver refuses to
   run without it, before any listener. Shell state does not persist between Claude tool
   calls: prefix every command with it (`OAC_TEST_LAN=1 ...`; PowerShell
   `$env:OAC_TEST_LAN='1'; ...`) and use full paths, never a `DIR` variable set earlier.
4. The TLS listener is opened only on the `IP:PORT` named in the command, never the
   unspecified address. `--multicast` (step 4 only) also makes the transport join the
   scouting group on UDP 7446 (zenoh 1.10.1 `DEFAULT_CONFIG.json5` `scouting.multicast`:
   `address`, `listen`); that socket's local bind address is not the `--listen` address and
   is UNVERIFIED here (the pin notes say Windows binds the unspecified address). Say so to
   the operator before step 4 and open it only on their yes.
5. On any unexpected output (an `error` line, a nonzero exit, a `start` with `ok:false`
   outside a negative control, a clock failure, a missing line): stop, copy the evidence
   lines, say what was expected, and ask. No retry loop, no changed flags, no second
   approach. One repeat of a step is allowed only for a setup fault the evidence names, and
   the first run stays in the record.
6. The authority private key exists only inside the `issue` process on host A and is never
   written. B's bundle contains B's private key and is test-only (see Pairing). Never post,
   paste, email or commit anything under `host-a/` or `for-host-b/`.
7. Skew above 5 s fails the clock condition and the leg must not be recorded as passed. A
   silent loss reported later than the 10 s lease plus 5 s fails step 8. Record residuals as
   the README says; do not tune a bound to make a value pass.
8. Never post a result without the operator's yes (Evidence and posting).

## Prerequisites (check each; any gap is a stop)

- [ ] Two machines (Windows, Linux or macOS), each with a fixed IPv4 address the other can
      reach directly: one routed segment, no NAT, no VPN between them. IP SAN equals the
      address dialled, so NAT breaks it.
- [ ] Same OAC commit, clean tree, on both: `git rev-parse HEAD` equal, `git status
      --porcelain` empty. Pinned toolchain from `rust-toolchain.toml`; Zenoh is pinned 1.10.1
      (`oac-zenoh` Pin). Node installed. Built with the wrapper (`build`).
- [ ] Three TCP ports chosen by the operator, free on their hosts: `PA` (A), `PB`, `PC`
      (B). TLS over TCP only. Step 4 also needs UDP multicast `224.0.0.224:7446` in and out on
      both hosts, one L2 segment, and no AP/client isolation.
- [ ] Firewall: the operator allows inbound TCP `PA` on A from B's IP only, and `PB`, `PC` on B
      from A's IP only (runbook "Operator commands"). On Windows the network profile must be
      Private or a scoped rule must exist; the first run of `lan_leg.exe` may raise an
      "allow access" prompt, which only the operator answers.
- [ ] Clocks synchronised through the OS service on both (NTP or OS time sync). OAC adds no
      clock sync; the leg only measures.
- [ ] A way to carry one directory from A to B out of band (removable media or a channel the
      operator chooses). Not a chat, issue, email or synced folder.

## Roles and what the operator relays

| Host | Does | Prints for the operator |
|---|---|---|
| A | authority (`issue`), first peer (listener), observer of B's carrier losses | its IP; after `issue`: `CN_A`, `CN_B`, `CN_C`, the path `DIR/for-host-b` to carry |
| B | second peer, negative-control client, third transport "C" (step 8), performs the stops | its IP (needed before `issue`); evidence files to bring back |

| Value | From -> to | When |
|---|---|---|
| `B_IP` | B -> A | before `issue` |
| `A_IP`, `PA`, `PB`, `PC` | operator -> both | before step 1 |
| `CN_A`, `CN_B`, `CN_C` | A -> B | after `issue` |
| directory `DIR/for-host-b` (holds B's key) | A -> B, by hand | after `issue` |
| "A READY" (A's `start` line shows `ok:true`) | A -> B | before every B run |
| "B DONE" and B's `evidence/*.jsonl` | B -> A | after the last step |

## Pairing (README step 2) and the key rule

The API gives no certificate-request form: `PairingAuthority::issue` generates a leaf's
private key together with its certificate (`transports/zenoh/src/certificates.rs`), and
`IssuedCertificate` has no accessor or constructor from PEM. So host B cannot generate its own
key. The driver keeps what the API allows: the authority is created and dropped inside one
`issue` run on A and never persisted; A's own key never leaves A; each credential is installed
into a fresh owner-only directory by the crate. B's bundle (its certificate, its private key,
the root, plus test-only negative-control credentials) is carried by the operator out of
band, lives under `DIR/for-host-b` in a fresh directory, is hardened by `import` on arrival
and is deleted at cleanup. This is a test-only deviation from "never distribute ... a host
key to other peers"; state it in the result. Do not extend it to production pairing, which is
G9/G11.

## Step map

Status is at this commit. Commands, expected output and per-step stop rules:
`references/runbook.md`.

| README step | What runs | Status |
|---|---|---|
| 1 record environment, clock | `host` + `env` lines; NTP-style skew over the carrier | automated |
| 2 pairing | `issue` on A, carry, `import` on B | issuing automated; "never distribute a host key" NOT RUN: B's and C's keys are generated on A and carried (above) |
| 3 explicit endpoints, scouting off | `peer` on both; sealed frames both ways | frames automated; core-signed frames NOT RUN (synthetic frames, RFC 7748 test keys); the shared contract suite over two hosts NOT RUN |
| 4 multicast, then fallback | `peer --multicast`; then a multicast check that must not link, then step 3 again | automated; the operator blocks multicast at the network; TLS-only links NOT RUN (configuration only) |
| 5 negative controls | `negative` control, wrong-ca, wrong-san, unlisted-cn (wrong-key recorded) | wrong CA, wrong IP SAN and unlisted CN automated beside a positive control; wrong key NOT counted (refused locally); absent client certificate and plaintext locator NOT RUN |
| 6 expiry and skew | `peer --timed-test` with skew before and after | expiry judged by a driver stand-in for SEC-SEL-036; receiving-core discard NOT RUN |
| 7 replace the local relay | needs the CLI daemon | NOT RUN at this commit; record it so |
| 8 presence and carrier loss | late-joining third transport; clean, killed, silent stops | clean and killed automated; silent: operator-guided drop with a `mark` before and after |

Under Claude tools the default command timeout is 120 s: start every long-running peer
(host A's, host B's step-8 peers) with `run_in_background` and read its evidence file.

## Evidence, posting

Format, labels and the gate-result comment: `references/evidence.md`. `assemble` prints one
line per criterion and writes `lan-leg-gate-result.md`. Show the operator the filled
template, ask, then post on #64 and #63 with `gh issue comment` and `Refs` wording only.
Nothing is committed. Failures and NOT RUN items are posted as they are.

## Cleanup (both hosts, in this order)

1. Ask the operator to remove any drop rule first, then the allow rules they added
   (runbook "Operator commands"). Verify with a read-only query.
2. Stop only driver processes this session started (`lan_leg`); check none remain.
3. Copy `DIR/evidence` and the two `lan-leg-*` files to a place the operator names, then
   delete `DIR/host-a`, `DIR/for-host-b` and, when finished, `DIR` itself.
4. Ask the operator to erase the carried copy on the removable medium.

## Troubleshooting

`references/troubleshooting.md`: multicast blocked, firewall and Windows Public profile,
clock skew above 5 s, wrong IP SAN, NAT/VPN and virtual interfaces, import failures.

## Where the content lives

- Steps and bounds: `tests/integration/README.md` "G3 native TLS two-host leg (#64)"
- Binding, trust rule, `LINK_LEASE_MILLIS`, 5 s limit: `transports/zenoh/src/lib.rs`
- Driver: `transports/zenoh/examples/lan_leg.rs`; wrapper: `tools/lan-leg/lan-leg.mjs`
- Result template and verdicts: `oac-gates`; `docs/planning/gates/README.md`
- Pins: `oac-zenoh` Pin, `docs/planning/PINS.md`
