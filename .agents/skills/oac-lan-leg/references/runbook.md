# Two-host LAN leg: runbook

Host A = authority + first peer. Host B = second peer, negative-control client, third
transport "C" and the side that performs the step-8 stops. Every `node tools/lan-leg/lan-leg.mjs`
command runs from the repo root and prints its evidence lines; they are also appended to
`<DIR>/evidence/<label>.jsonl`.

Placeholders: `A_IP`, `B_IP` (host LAN addresses), `PA`, `PB`, `PC` (TCP ports), `CN_A`,
`CN_B`, `CN_C` (printed by `issue`), `<DIR>` (the full path of a new work directory outside the
repo, for example `C:\Users\you\AppData\Local\Temp\oac-lan-leg` or `/tmp/oac-lan-leg`).

**Claude tool environment.** Shell state does not persist between tool calls, so every
command below carries its own prefix and full paths: bash `OAC_TEST_LAN=1 node ...`, PowerShell
`$env:OAC_TEST_LAN='1'; node ...`. The blocks show the bash form; use the PowerShell prefix
there. The default tool timeout is 120 s: start every peer that runs longer (all host A peers,
host B's step-8 peers) with `run_in_background` and read its evidence file; A's `--run-secs`
below are generous (300 s or more) because the operator relays "A READY" by hand. A peer that
outlives its step is stopped by the operator or by Claude (its own process) before the next one.

Each step: the session that goes second waits for the first one's "READY" (A's `start` line with
`"ok":true`). A step is done when its commands exited 0 and the expected lines appeared.
Anything else: stop (SKILL.md rule 5).

## 0. Both hosts: setup (after the operator confirms the opt-in)

Tell the operator once: "This opens TLS listeners on <IP>:<ports> on this host for the LAN leg
and nothing else (step 4 also joins the UDP 7446 scouting group)." Then:

```sh
git rev-parse HEAD ; git status --porcelain      # SHA equal on both hosts; the second prints nothing
OAC_TEST_LAN=1 node tools/lan-leg/lan-leg.mjs build
```

Expected: the same 40-hex SHA on both hosts and cargo's `Finished`. Refusal check (no listener is
opened; exit code 3). Bash:

```sh
OAC_TEST_LAN= target/debug/examples/lan_leg peer --dir x     # exit 3
```

PowerShell, restoring the variable:

```powershell
$old = $env:OAC_TEST_LAN; Remove-Item Env:OAC_TEST_LAN -ErrorAction SilentlyContinue; target\debug\examples\lan_leg.exe peer --dir x; "exit=$LASTEXITCODE"; if ($old) { $env:OAC_TEST_LAN = $old }
```

Expected stderr: `lan_leg: refused: this driver listens on a LAN address on purpose; set OAC_TEST_LAN=1 to opt in`.

## 1. Record the host (both)

```sh
OAC_TEST_LAN=1 node tools/lan-leg/lan-leg.mjs host --dir <DIR> --role a --firewall "<policy the operator states>"   # --role b on B
```

Expected: one `"event":"host"` line with `ipv4`, `utc_offset_minutes`, and on Windows
`firewall_profiles` and `network_category`. Report the address to use; B sends `B_IP` to A.

```json
{"event":"host","role":"a","platform":"win32","utc_offset_minutes":-360,"ipv4":[{"interface":"Wi-Fi 2","address":"..."}],"firewall_policy_stated_by_operator":"dry run","firewall_profiles":[{"Name":"Domain","Enabled":1,"DefaultInboundAction":0},{"Name":"Private","Enabled":1,"DefaultInboundAction":0},{"Name":"Public","Enabled":1,"DefaultInboundAction":0}],"network_category":{"InterfaceAlias":"Wi-Fi 2","NetworkCategory":0}}
```

Real `host` output from a Windows machine (address elided).

(`NetworkCategory` 0 is Public, 1 Private. The list may include virtual adapters, for example
`vEthernet (WSL ...)`: the operator names the address on the real LAN segment.)

## 2. Pairing

Host A:

```sh
OAC_TEST_LAN=1 node tools/lan-leg/lan-leg.mjs run --dir <DIR> --label step2-issue-a -- issue --dir <DIR> --ip-a <A_IP> --ip-b <B_IP>
```

```json
{"event":"issue","role":"a","ip_a":"127.0.0.1","ip_b":"127.0.0.1","cn_a":"lan-a-37b52b49","cn_b":"lan-b-2ac6d35a","cn_c":"lan-c-32031966","authority_private_key_persisted":false,"allow_on_a":["lan-b-2ac6d35a","lan-c-32031966"],"allow_on_b":["lan-a-37b52b49","lan-c-32031966"],"carry_to_host_b":"<DIR>/for-host-b","keep_on_host_a":"<DIR>/host-a"}
```

Loopback dry-run output, trimmed to the fields that matter; values differ per run.

Print for the operator: `CN_A`, `CN_B`, `CN_C`, and "carry the directory `<DIR>/for-host-b`
(it holds host B's private key, test-only) to host B by hand into B's `<DIR>`". Claude on A does
not copy it anywhere itself.

Host B, after the operator has placed it at `<DIR>/for-host-b`:

```sh
OAC_TEST_LAN=1 node tools/lan-leg/lan-leg.mjs run --dir <DIR> --label step2-import-b -- import --dir <DIR>
```

```json
{"event":"import","role":"b","hardened":["b","c","neg-unlisted","neg-wrong-ca","neg-wrong-key"]}
```

Loopback dry-run output, trimmed to the fields that matter; values differ per run.

`import` tightens only the directories under `<DIR>/for-host-b` (mode 0700/0600 on Unix, an
owner-only ACL through `icacls` on Windows). If it fails, stop (troubleshooting).

## 3. Clock (step 1) and explicit endpoints, scouting off

A first (background; B must connect inside `--run-secs`):

```sh
OAC_TEST_LAN=1 node tools/lan-leg/lan-leg.mjs run --dir <DIR> --label step3-a -- peer --dir <DIR> --cred a --role a --listen <A_IP>:<PA> --allow <CN_B>,<CN_C> --run-secs 300
```

Wait for the `start` line with `"ok":true` and `"scouting":"off"`; say "A READY". Then B:

```sh
OAC_TEST_LAN=1 node tools/lan-leg/lan-leg.mjs run --dir <DIR> --label step3-b -- peer --dir <DIR> --cred b --role b --listen <B_IP>:<PB> --connect <A_IP>:<PA> --allow <CN_A>,<CN_C> --skew-pings 8 --run-secs 5
```

```json
A: {"event":"start","role":"a","ok":true,"scouting":"off","connect":null,"listen":"127.0.0.1:41800"}
A: {"event":"link","role":"a","up":true,"waited_ms":3544}
A: {"event":"frame","role":"a","verdict":"accepted","tag":"ping","from":"b","seq":1,"late_by_ms":-9963}
B: {"event":"start","role":"b","ok":true,"scouting":"off","connect":"127.0.0.1:41800","listen":"127.0.0.1:41801"}
B: {"event":"frame","role":"b","verdict":"accepted","tag":"pong","from":"a","seq":1}
B: {"event":"skew","role":"b","local":"b","remote":"a","phase":"before","pings":6,"samples":6,"best_offset_us":484,"best_delay_us":37190,"abs_skew_ms":0.484,"limit_ms":5000,"pass":true}
```

Loopback dry-run output, trimmed to the fields that matter; values differ per run.

Pass: both `start` ok; `link` `up:true`; A has `frame` lines with `verdict:accepted`, `from:b`
(pings) and B has `from:a` (pongs), so sealed frames crossed both ways; the `skew` line has
`"pass":true` (|offset| at most 5000 ms, compared exactly). `skew` is B's clock offset to A over
the carrier (NTP-style, four timestamps); `best_delay_us` bounds its error. A batch with
`samples:0` fails. If `pass` is false: clock condition fails, stop. The frames are synthetic,
sealed with RFC 7748 test keys, not core-signed (`3.core-signed` is NOT RUN).

## 4. Multicast scouting, then the fallback

Disclosure first: `--multicast` also joins the UDP 7446 scouting group; its local bind address
is not `--listen`'s (UNVERIFIED, SKILL.md rule 4). Proceed on the operator's yes.

Same commands, no `--connect`, plus `--multicast`, labels `step4-mc-a` (A, `--run-secs 300
--link-timeout-secs 60`) and `step4-mc-b` (B, `--skew-pings 4 --run-secs 5 --link-timeout-secs 40`).
Expected: `start.scouting` is `multicast`, `start.connect` is `null`, both sides print `presence`
`appeared`, frames cross both ways. Links stay TLS-only only because the configuration allows
`tls`; links are not inspected (`4.tls-only` is NOT RUN).

Fallback. The operator blocks multicast at the network or host (print the "Block multicast"
commands under Operator commands, after removing the 7446 allow rule). Then prove the block:
the same multicast peers, no `--connect`, must NOT link. A's check peer starts first, in the
background, with a long `--run-secs`, and B's runs inside it: the assembler requires the two run
windows to overlap (by 10 s beyond the clock error), else `4.fallback` is NOT RUN.

```sh
OAC_TEST_LAN=1 node tools/lan-leg/lan-leg.mjs run --dir <DIR> --label step4-fb-check-a -- peer --dir <DIR> --cred a --role a --listen <A_IP>:<PA> --allow <CN_B>,<CN_C> --multicast --link-timeout-secs 15 --run-secs 120
OAC_TEST_LAN=1 node tools/lan-leg/lan-leg.mjs run --dir <DIR> --label step4-fb-check-b -- peer --dir <DIR> --cred b --role b --listen <B_IP>:<PB> --allow <CN_A>,<CN_C> --multicast --link-timeout-secs 15 --run-secs 20
```

Start A's (background), then B's on "A READY"; B's must start while A's is still waiting. Expected on both:
`"link":{"up":false}` and no `appeared`. If either links, the block is not in place: stop; the
assembler marks `4.fallback` FAIL. Then repeat step 3 under the labels `step4-fb-a` /
`step4-fb-b` (explicit endpoints). Never accept a plaintext link.

## 5. Negative controls

A runs one long peer; B runs a positive control and then the cases against it.

```sh
OAC_TEST_LAN=1 node tools/lan-leg/lan-leg.mjs run --dir <DIR> --label step5-a -- peer --dir <DIR> --cred a --role a --listen <A_IP>:<PA> --allow <CN_B>,<CN_C> --run-secs 600
```

B, label `step5-<case>-b`. Run `control` straight before EACH case (`wrong-ca`, `wrong-key`,
`unlisted-cn`, and once more before `wrong-san`, below); the assembler pairs a case with the latest
control before it. Every run uses the same `--listen`, `--connect` and `--allow`:

```sh
OAC_TEST_LAN=1 node tools/lan-leg/lan-leg.mjs run --dir <DIR> --label step5-control-b -- negative --dir <DIR> --case control --role b --listen <B_IP>:<PB> --connect <A_IP>:<PA> --allow <CN_A>
```

```json
{"event":"negative","role":"b","case":"control","start_ok":true,"linked":true,"listener_reachable":true,"native_open_failed":false,"probes_sent":0,"pass":true,"counts_toward_readme":false}
{"event":"negative","role":"b","case":"wrong-ca","start_ok":false,"linked":false,"listener_reachable":true,"native_open_failed":true,"probes_sent":0,"pass":true,"counts_toward_readme":true}
{"event":"negative","role":"b","case":"wrong-key","start_ok":false,"linked":false,"listener_reachable":true,"native_open_failed":true,"probes_sent":0,"pass":null,"counts_toward_readme":false}
{"event":"negative","role":"b","case":"unlisted-cn","start_ok":true,"linked":false,"listener_reachable":true,"native_open_failed":false,"probes_sent":4,"pass":null,"counts_toward_readme":true}
```

Loopback dry-run output, trimmed to the fields that matter; values differ per run.

The transport returns one message for any failed native open (TLS refusal by the peer, a port
in use, a `--listen` address the host lacks, a local TLS setup error). So:

- `control` (real credentials) must link: it shows `--listen` is usable and A accepts the real
  credentials. Without a passing control no refusal counts. The driver does not bind-test `--listen`
  itself (the loopback-listener lint forbids it), so a port taken between a control and its case
  would read as a refusal: that is why the control goes straight before each case.
- `wrong-ca` (certificate and key from another authority) and `wrong-san` must show
  `"start_ok":false`, `listener_reachable:true`, `native_open_failed:true`, beside a passing control.
- `wrong-key` (right certificate, someone else's key) is recorded and NOT counted: the API gives one
  file set to both the local listener and the connection, so the local native stack can refuse the
  mismatch before any peer is asked.
- `unlisted-cn` (valid chain, CN not on A's list) may link; it then sends probes tagged
  `neg-probe` that A must not accept (the assembler checks A's `step5-a`).

Wrong IP SAN needs A's credential, not B's: after `step5-a` ends, A runs
`peer --cred neg-wrong-san` (label `step5-wrong-san-a`, `--run-secs 120`; certificate issued for the
documentation address `192.0.2.1`) and B runs `negative --case wrong-san` (label
`step5-wrong-san-b`). The control must have been run against the same `--listen`/`--connect`.

Not driven: absent client certificate and plaintext discovered locator. Both are `NOT RUN` in the
assembled table with their reason; do not improvise them.

## 6. Delayed receipt beyond the encrypted deadline, with skew before and after

A: `step6-a` as in step 3 (`--run-secs 300`). B:

```sh
OAC_TEST_LAN=1 node tools/lan-leg/lan-leg.mjs run --dir <DIR> --label step6-b -- peer --dir <DIR> --cred b --role b --listen <B_IP>:<PB> --connect <A_IP>:<PA> --allow <CN_A>,<CN_C> --skew-pings 8 --timed-test --skew-pings-after 8 --run-secs 5
```

```json
A: {"event":"frame","role":"a","verdict":"accepted","tag":"ontime","from":"b","late_by_ms":-1938}
A: {"event":"frame","role":"a","verdict":"discard-expired","tag":"late","from":"b","late_by_ms":1538}
B: {"event":"skew","role":"b","phase":"before","best_offset_us":-234,"best_delay_us":36021,"abs_skew_ms":0.234,"pass":true}
B: {"event":"skew","role":"b","phase":"after","best_offset_us":726,"best_delay_us":37292,"abs_skew_ms":0.726,"pass":true}
```

Loopback dry-run output, trimmed to the fields that matter; values differ per run.

Pass: `skew` `before` and `after` both at most 5000 ms; A's `ontime` frame `accepted`; A's `late`
frame `discard-expired` with its `native_utc_us` (arrival) recorded. The `late` frame is sealed with
a 2000 ms encrypted deadline and held 3500 ms by the sender before publication, so it arrives after
the deadline; a slow receiver clock would extend validity by up to the skew and a fast one reject
early (README step 6): report the skew with the result. The expiry check is the driver's own
stand-in for SEC-SEL-036 (`6.expiry`), not the core pipeline (`6.core-discard` is NOT RUN).

## 7. Replace the local relay: NOT RUN

Needs the CLI daemon, which is not built at this commit. Record in the result: "step 7: NOT RUN at
this commit (no daemon); holder gaps and fixed-port denial of service stay residuals".

## 8. Presence and carrier loss (#63)

1. A (background): `peer ... --cred a --role a --listen <A_IP>:<PA> --allow <CN_B>,<CN_C> --run-secs 900`, label `step8-a`.
2. B (transport "B", background): `peer ... --cred b --role b --listen <B_IP>:<PB> --connect <A_IP>:<PA> --allow <CN_A>,<CN_C> --skew-pings 6 --run-secs 800`, label `step8-b`. Wait for its `link` `up:true` and `skew` line: the assembler uses this batch's offset, and its half round trip as the error, to correct cross-host timings.
3. B, late join and clean stop (the third transport "C" joins last and links to both):

   ```sh
   OAC_TEST_LAN=1 node tools/lan-leg/lan-leg.mjs run --dir <DIR> --label step8-c-clean-b -- peer --dir <DIR> --cred c --role c --listen <B_IP>:<PC> --connect <A_IP>:<PA>,<B_IP>:<PB> --allow <CN_A>,<CN_B> --probe-peers --run-secs 15 --end clean
   ```

   ```json
C: {"event":"start","role":"c","ok":true,"connect":"127.0.0.1:41800,127.0.0.1:41801","listen":"127.0.0.1:41802"}
C: {"event":"presence","role":"c","kind":"appeared","n":1,"handle_index":0,"since_start_ms":959}
C: {"event":"probe","role":"c","pongs_from_a":3,"pongs_from_b":3,"linked_to_both":true}
C: {"event":"action","role":"c","label":"end-clean","utc":"2026-11-05T03:00:41.349259Z"}
A: {"event":"presence","role":"a","kind":"loss","handle_index":0,"utc":"2026-11-05T03:00:41.351208Z"}
```

Loopback dry-run output, trimmed to the fields that matter; values differ per run.

   What this shows (`8.late-join`): C prints `presence` `appeared`, from the transport's liveliness
   history, and `probe` shows pongs from both A and B (`linked_to_both`). "Appearance before the
   first send" holds by construction (the peer waits for it before sending), so it is not independent
   evidence. The transport reports one appearance event per carrier handle and coalesces appearances
   that fall together, so learning each of the first two tokens cannot be shown through the public API:
   `8.late-join-both-tokens` is NOT RUN. A and B each print `appeared`
   at C's start. At the end C prints `action` `end-clean` at the instant it calls shutdown; A prints
   `presence` `loss`. The assembler subtracts, correcting by the clock offset and printing its error.
4. B, killed: same command, label `step8-c-abort-b`, `--end abort` (C prints `action end-abort`, then
   aborts the process: the OS closes its sockets, as for a killed process).
5. B, silent loss: label `step8-c-silent-b`, `--run-secs 120`, no `--end`, `--probe-peers`. When `link`
   is up, the operator runs the drop on B bracketed by two marks in one command line (Operator commands
   below): a mark before and one after the drop commands. The bound is checked from the earlier mark.
   Observe A until a `loss` line or 25 s; then the operator removes the drop; then C ends on its own.
   If nothing is lost (see troubleshooting: an established flow may survive a new block), the step is
   recorded as it is.

Pass (README step 8): late join seen; each loss reported on A; the silent loss at most the lease (10 s)
plus 5 s after the action. The 15 s limit the assembler also applies to the clean and killed stops is its
own, not the README's (the README states a bound for the silent loss only); those are expected within a
second or two. Record every value; where it differs from the loopback measurement (clean/killed under
0.5 s; silent 9.5 to 9.7 s), record it as a residual, not a new bound.

## Assemble (host A, after B's evidence is brought back)

The operator copies B's `<DIR>/evidence/*.jsonl` (no key material) into `<DIR>/b-side/evidence/` on A.
Then:

```sh
OAC_TEST_LAN=1 node tools/lan-leg/lan-leg.mjs assemble --dir <DIR> --b-dir <DIR>/b-side
```

```text
{"schema":"oac-lan-leg/1","event":"assembled","verdict":"NOT RUN","pass":13,"fail":0,"not_run":12}
PASS    2/2.issue  authority key persisted=false; CNs lan-a-37b52b49, lan-b-2ac6d35a, lan-c-32031966; hardened on B: b, c, neg-unlisted, neg-wrong-ca, neg-wrong-key
PASS    3/3.explicit  starts ok=true; A accepted 6 frame(s) from B; B accepted 6 from A; scouting off/off
PASS    5/5.wrong-ca  start_ok=false; listener_reachable=true; native_open_failed=true; control linked=true on the same listen/connect=true; A served wrong-SAN credential=true; invalid transport configuration: the peer session ...
NOT RUN 5/5.wrong-key  cannot be shown with this API (one file set serves both the local listener and the connection, so the local native stack can refuse the mismatch first); observed start_ok=false: invalid transport configur...
PASS    6/6.expiry  ontime accepted; late discard-expired (opened 1538 ms past its deadline, native arrival 2026-11-05T02:58:36.138Z). The discard is the driver's own check, not the core pipeline.
PASS    8/8.late-join  clean: 2 appearance event(s) on C (first after 959 ms) before any frame; pongs from A=3, from B=3; abort: 1 appearance event(s) on C (first after 2711 ms) before any frame; pongs from A=3, from B=3. The t...
PASS    8/8.loss-clean  action 2026-11-05T03:00:41.349Z on B; first loss on A 2026-11-05T03:00:41.351Z; raw 1.949 ms, skew-corrected 2.886 ms (+-19.152 ms: half the batch's best round trip); offset A-B -0.937 ms; 2 loss event(s...
PASS    8/8.loss-abort  action 2026-11-05T03:00:52.620Z on B; first loss on A 2026-11-05T03:00:53.136Z; raw 516.352 ms, skew-corrected 517.289 ms (+-19.152 ms: half the batch's best round trip); offset A-B -0.937 ms; 1 loss eve...
NOT RUN 8/8.loss-silent  no action instant, no observer evidence or no step-8 skew batch
NOT RUN 7/7.relay-replace  needs the CLI daemon (not built at this commit)
```

That run was one machine, both roles on 127.0.0.1, clean tree. The verdict is `NOT RUN`; 12 criteria were not run or cannot be shown there (step 4, the silent loss, step 7, and the ones the driver cannot show). On real hosts steps 4 and the silent loss are added and the verdict is still `NOT RUN` at best (evidence.md).

## Operator commands (Claude prints, the operator runs)

Allow rules (names are distinct so cleanup can find them), scoped to the other host.

```powershell
# Windows, elevated, on A (on B use ports PB, PC and A_IP as the remote)
New-NetFirewallRule -DisplayName 'oac-lan-leg-in' -Direction Inbound -Protocol TCP -LocalPort <PA> -RemoteAddress <B_IP> -Action Allow -Profile Private
New-NetFirewallRule -DisplayName 'oac-lan-leg-mc' -Direction Inbound -Protocol UDP -LocalPort 7446 -RemoteAddress <B_IP> -Action Allow -Profile Private   # step 4 only
```

```sh
# Linux (ufw), on A; on B use PB, PC
sudo ufw allow from <B_IP> to any port <PA> proto tcp comment oac-lan-leg
sudo ufw allow from <B_IP> to any port 7446 proto udp comment oac-lan-leg     # step 4 only
# macOS: the application firewall prompts per executable; the operator answers it, or runs with the firewall off for the test
```

Block multicast (step 4 fallback), on both hosts, after removing the 7446 allow rule:

```powershell
New-NetFirewallRule -DisplayName 'oac-lan-leg-mc-block-in' -Direction Inbound -Protocol UDP -LocalPort 7446 -Action Block ; New-NetFirewallRule -DisplayName 'oac-lan-leg-mc-block-out' -Direction Outbound -Protocol UDP -RemotePort 7446 -Action Block
```

```sh
sudo iptables -I INPUT -p udp --dport 7446 -j DROP ; sudo iptables -I OUTPUT -p udp --dport 7446 -j DROP
```

Silent loss on B: drop both directions to A, with a mark before and after, in one command line.
A new block rule may not cut a connection that is already established; if the link stays up
(troubleshooting), unplug the cable or disable the adapter instead and run the same two marks around it.

```powershell
# Windows, elevated, on B
$env:OAC_TEST_LAN='1'; & node tools/lan-leg/lan-leg.mjs mark --dir <DIR> --label step8-marks-b --action silent-loss-pre ; New-NetFirewallRule -DisplayName 'oac-lan-leg-drop-out' -Direction Outbound -RemoteAddress <A_IP> -Action Block ; New-NetFirewallRule -DisplayName 'oac-lan-leg-drop-in' -Direction Inbound -RemoteAddress <A_IP> -Action Block ; & node tools/lan-leg/lan-leg.mjs mark --dir <DIR> --label step8-marks-b --action silent-loss-post
```

```sh
# Linux, on B
export OAC_TEST_LAN=1 ; node tools/lan-leg/lan-leg.mjs mark --dir <DIR> --label step8-marks-b --action silent-loss-pre ; sudo iptables -I OUTPUT -d <A_IP> -j DROP ; sudo iptables -I INPUT -s <A_IP> -j DROP ; node tools/lan-leg/lan-leg.mjs mark --dir <DIR> --label step8-marks-b --action silent-loss-post
# macOS: unplug the cable (or switch the interface off) between two mark commands instead
```

Remove (always, also after a failure):

```powershell
Remove-NetFirewallRule -DisplayName 'oac-lan-leg-drop-out','oac-lan-leg-drop-in','oac-lan-leg-in','oac-lan-leg-mc','oac-lan-leg-mc-block-in','oac-lan-leg-mc-block-out' -ErrorAction SilentlyContinue
```

```sh
sudo iptables -D OUTPUT -d <A_IP> -j DROP ; sudo iptables -D INPUT -s <A_IP> -j DROP
sudo iptables -D INPUT -p udp --dport 7446 -j DROP ; sudo iptables -D OUTPUT -p udp --dport 7446 -j DROP
sudo ufw delete allow from <B_IP> to any port <PA> proto tcp     # repeat for each rule added
```

Read-only verification after removal: `Get-NetFirewallRule -DisplayName 'oac-lan-leg*'` (prints
nothing) / `sudo iptables -S | grep -e <A_IP> -e 7446` and `sudo ufw status`.

## Cleanup commands (Claude may run these on its own `<DIR>` after copying evidence out)

```powershell
Get-Process lan_leg -ErrorAction SilentlyContinue          # none should remain; stop only ones this session started
Remove-Item -Recurse -Force '<DIR>\host-a', '<DIR>\for-host-b'
```

```sh
pgrep -x lan_leg            # none should remain
rm -rf <DIR>/host-a <DIR>/for-host-b
```
