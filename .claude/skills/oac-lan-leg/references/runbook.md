# Two-host LAN leg: runbook

Host A = authority + first peer. Host B = second peer, negative-control client, third
transport "C" and the side that performs the step-8 stops. Every `node tools/lan-leg/lan-leg.mjs`
command runs from the repo root and prints its evidence lines; they are also appended to
`DIR/evidence/<label>.jsonl`. `...` below means `node tools/lan-leg/lan-leg.mjs`.

Placeholders: `A_IP`, `B_IP` (host LAN addresses), `PA`, `PB`, `PC` (TCP ports),
`CN_A`, `CN_B`, `CN_C` (printed by `issue`), `DIR` (work directory).

Each step: the session that is to go second waits for the first one's "READY". A step is done
when its commands exited 0 and the expected lines appeared. Anything else: stop (SKILL.md
rule 5).

## 0. Both hosts: setup (run by Claude, after the operator confirms the opt-in)

State to the operator once: "This opens TLS listeners on <IP>:<ports> on this host for the
LAN leg and nothing else." Then:

```powershell
# PowerShell (Windows)
$env:OAC_TEST_LAN = '1'
$DIR = Join-Path $env:TEMP 'oac-lan-leg'
git rev-parse HEAD ; git status --porcelain      # SHA equal on both hosts; second command prints nothing
node tools/lan-leg/lan-leg.mjs build
```

```sh
# bash (Linux, macOS)
export OAC_TEST_LAN=1
DIR="${TMPDIR:-/tmp}/oac-lan-leg"
git rev-parse HEAD ; git status --porcelain
node tools/lan-leg/lan-leg.mjs build
```

Expected: the same 40-hex SHA on both hosts and `Finished` from cargo. Refusal check (no
listener is opened; exit code 3):

```sh
OAC_TEST_LAN= target/debug/examples/lan_leg peer --dir x     # PowerShell: Remove-Item Env:OAC_TEST_LAN first
# lan_leg: refused: this driver listens on a LAN address on purpose; set OAC_TEST_LAN=1 to opt in
```

### Step 1: record the host (both)

```sh
node tools/lan-leg/lan-leg.mjs host --dir "$DIR" --role a --firewall "<policy the operator states>"   # --role b on B
```

Expected: one `"event":"host"` line with `ipv4` (the addresses), `utc_offset_minutes`, and on
Windows `firewall_profiles` and `network_category`. Report the address to use. B sends
`B_IP` to A.

```json
{"event":"host","role":"a","platform":"win32","utc_offset_minutes":-360,"ipv4":[{"interface":"Wi-Fi 2","address":"..."}],"firewall_policy_stated_by_operator":"dry run","firewall_profiles":[{"Name":"Domain","Enabled":1,"DefaultInboundAction":0},{"Name":"Private","Enabled":1,"DefaultInboundAction":0},{"Name":"Public","Enabled":1,"DefaultInboundAction":0}],"network_category":{"InterfaceAlias":"Wi-Fi 2","NetworkCategory":0}}
```

(`NetworkCategory` 0 is Public, 1 Private. The list may include virtual adapters, for example
`vEthernet (WSL ...)`: the operator names the address on the real LAN segment.)

## 2. Pairing

Host A:

```sh
node tools/lan-leg/lan-leg.mjs run --dir "$DIR" --label step2-issue-a -- issue --dir "$DIR" --ip-a <A_IP> --ip-b <B_IP>
```

```json
{"event":"issue","role":"a","ip_a":"127.0.0.1","ip_b":"127.0.0.1","cn_a":"lan-a-ede93553","cn_b":"lan-b-f45d6585","cn_c":"lan-c-4e7d1623","authority_private_key_persisted":false,"allow_on_a":["lan-b-f45d6585","lan-c-4e7d1623"],"allow_on_b":["lan-a-ede93553","lan-c-4e7d1623"],"carry_to_host_b":"<DIR>/for-host-b","keep_on_host_a":"<DIR>/host-a"}
```

Loopback dry-run output, trimmed to the fields that matter; values differ per run.

Print for the operator: `CN_A`, `CN_B`, `CN_C`, and "carry the directory `DIR/for-host-b`
(it holds host B's private key, test-only) to host B by hand into B's `DIR`". Claude on A
does not copy it anywhere itself.

Host B, after the operator has placed it at `DIR/for-host-b`:

```sh
node tools/lan-leg/lan-leg.mjs run --dir "$DIR" --label step2-import-b -- import --dir "$DIR"
```

```json
{"event":"import","hardened":["b","c","neg-unlisted","neg-wrong-ca","neg-wrong-key"],"role":"b"}
```

Loopback dry-run output, trimmed to the fields that matter; values differ per run.

`import` tightens only the directories under `DIR/for-host-b` (mode 0700/0600 on Unix, an
owner-only ACL through `icacls` on Windows). If it fails, stop (troubleshooting).

## 1 and 3. Clock and explicit endpoints, scouting off

A first (it waits `--run-secs`, then ends cleanly; B must connect inside it):

```sh
node tools/lan-leg/lan-leg.mjs run --dir "$DIR" --label step3-a -- peer --dir "$DIR" --cred a --role a --listen <A_IP>:<PA> --allow <CN_B>,<CN_C> --run-secs 60
```

Wait for the `start` line with `"ok":true` and `"scouting":"off"`; say "A READY". Then B:

```sh
node tools/lan-leg/lan-leg.mjs run --dir "$DIR" --label step3-b -- peer --dir "$DIR" --cred b --role b --listen <B_IP>:<PB> --connect <A_IP>:<PA> --allow <CN_A>,<CN_C> --skew-pings 8 --run-secs 5
```

```json
A: {"event":"start","role":"a","ok":true,"scouting":"off","connect":null,"listen":"127.0.0.1:41400","cred":"a"}
A: {"event":"link","role":"a","up":true,"waited_ms":3644}
A: {"event":"frame","role":"a","verdict":"accepted","tag":"ping","from":"b","seq":1,"late_by_ms":-9955}
B: {"event":"start","role":"b","ok":true,"scouting":"off","connect":"127.0.0.1:41400","listen":"127.0.0.1:41401"}
B: {"event":"link","role":"b","up":true,"waited_ms":1142}
B: {"event":"frame","role":"b","verdict":"accepted","tag":"pong","from":"a","seq":1}
B: {"event":"skew","local":"b","remote":"a","phase":"before","pings":6,"samples":6,"best_offset_us":-169,"best_delay_us":38989,"abs_skew_ms":0,"limit_ms":5000,"pass":true}
```

Loopback dry-run output, trimmed to the fields that matter; values differ per run.

Pass: both `start` ok; `link` `up:true`; A has `frame` lines with `verdict:accepted`,
`from:b` (pings) and B has `from:a` (pongs), so sealed frames crossed both ways; the `skew`
line has `"pass":true` (`abs_skew_ms` at most 5000). `skew` is B's clock offset to A over the
carrier (NTP-style, four timestamps); `best_delay_us` bounds its error. If `pass` is false:
clock condition fails, stop.

## 4. Multicast scouting, then the fallback

Same commands, no `--connect`, plus `--multicast`, labels `step4-mc-a` / `step4-mc-b`
(`--skew-pings 4`, `--link-timeout-secs 40`). Expected: `start.scouting` is `multicast`,
`start.connect` is `null`, both sides print `presence` `appeared`, frames cross both ways.
Native links stay TLS-only because the configuration allows only `tls`; this step does not
inspect link protocols.

Fallback: the operator disables multicast at the network (for example switches the segment or
the hosts' multicast filter; Claude prints the instruction it was given by the operator's
setup, it does not choose one), then repeat step 3 under the labels `step4-fb-a` /
`step4-fb-b`. Never accept a plaintext link.

## 5. Negative controls

A runs one long peer; B runs the four controls against it.

```sh
node tools/lan-leg/lan-leg.mjs run --dir "$DIR" --label step5-a -- peer --dir "$DIR" --cred a --role a --listen <A_IP>:<PA> --allow <CN_B>,<CN_C> --run-secs 150
```

B, one per case (`wrong-ca`, `wrong-key`, `unlisted-cn`), label `step5-<case>-b`:

```sh
node tools/lan-leg/lan-leg.mjs run --dir "$DIR" --label step5-wrong-ca-b -- negative --dir "$DIR" --case wrong-ca --role b --listen <B_IP>:<PB> --connect <A_IP>:<PA> --allow <CN_A>
```

```json
{"event":"negative","role":"b","case":"wrong-ca","start_ok":false,"listener_reachable":true,"native_refusal":true,"setup_fault":false,"pass":true,"error":"invalid transport configuration: the peer session did not open"}
{"event":"negative","role":"b","case":"unlisted-cn","start_ok":true,"probes_sent":4,"pass":null,"needs_peer_evidence":true}
```

Loopback dry-run output, trimmed to the fields that matter; values differ per run.

`wrong-ca` (certificate and key from a different authority) and `wrong-key` (right
certificate, someone else's key) must show `"start_ok":false`: native authentication
refused. `unlisted-cn` (valid chain, CN not on A's list) may link; it then sends probes tagged
`neg-probe` that A must not accept: the assembler checks A's `step5-a` has no accepted
`neg-probe` frame.

Wrong IP SAN needs A's credential, not B's: after `step5-a` ends, A runs
`peer --cred neg-wrong-san` (label `step5-wrong-san-a`, `--run-secs 40`, certificate issued for a
documentation address `192.0.2.1` it does not own) and B runs `negative --case wrong-san`
(label `step5-wrong-san-b`); B's start must be refused.

Not driven: absent client certificate and plaintext discovered locator. Both are `NOT RUN`
in the assembled table with their reason; do not improvise them.

## 6. Delayed receipt beyond the encrypted deadline, with skew before and after

A: `step6-a` as in step 3 (`--run-secs 60`). B:

```sh
node tools/lan-leg/lan-leg.mjs run --dir "$DIR" --label step6-b -- peer --dir "$DIR" --cred b --role b --listen <B_IP>:<PB> --connect <A_IP>:<PA> --allow <CN_A>,<CN_C> --skew-pings 8 --timed-test --skew-pings-after 8 --run-secs 5
```

```json
A: {"event":"frame","role":"a","verdict":"accepted","tag":"ontime","from":"b","late_by_ms":-1933}
A: {"event":"frame","role":"a","verdict":"discard-expired","tag":"late","from":"b","late_by_ms":1547,"native_utc_us":1793844571489506}
B: {"event":"skew","phase":"before","best_offset_us":-289,"best_delay_us":50135,"abs_skew_ms":0,"pass":true}
B: {"event":"skew","phase":"after","best_offset_us":996,"best_delay_us":39591,"abs_skew_ms":0,"pass":true}
```

Loopback dry-run output, trimmed to the fields that matter; values differ per run.

Pass: `skew` `before` and `after` both at most 5000 ms; A's `ontime` frame `accepted`; A's
`late` frame `discard-expired` with its `native_utc_us` (arrival) recorded. The `late` frame
is sealed with a 2000 ms encrypted deadline and held 3500 ms by the sender before
publication, so it arrives after the deadline; a slow receiver clock would extend validity by
up to the skew and a fast one reject early (README step 6): report the skew with the result.
The expiry check is the driver's stand-in for SEC-SEL-036, not the core pipeline; say so.

## 7. Replace the local relay: NOT RUN

Needs the CLI daemon, which is not built at this commit. Record in the result: "step 7: NOT
RUN at this commit (no daemon); holder gaps and fixed-port denial of service stay residuals".

## 8. Presence and carrier loss (#63)

Needed at the start of this step (it measures cross-host timing): the `skew` from step 3/6
stands for the offset; `step8-b` re-measures it.

1. A: `peer ... --cred a --role a --listen <A_IP>:<PA> --allow <CN_B>,<CN_C> --run-secs 400`, label `step8-a`.
2. B (transport "B"): `peer ... --cred b --role b --listen <B_IP>:<PB> --connect <A_IP>:<PA> --allow <CN_A>,<CN_C> --skew-pings 6 --run-secs 380`, label `step8-b`. Wait for its `link` `up:true` and `skew` line.
3. B, late join and clean stop (the third transport "C" joins last and links to both):

   ```sh
   node tools/lan-leg/lan-leg.mjs run --dir "$DIR" --label step8-c-clean-b -- peer --dir "$DIR" --cred c --role c --listen <B_IP>:<PC> --connect <A_IP>:<PA>,<B_IP>:<PB> --allow <CN_A>,<CN_B> --run-secs 15 --end clean
   ```

   ```json
C: {"event":"start","role":"c","ok":true,"connect":"127.0.0.1:41400,127.0.0.1:41401","listen":"127.0.0.1:41402"}
C: {"event":"presence","role":"c","kind":"appeared","n":1,"since_start_ms":2849}
A: {"event":"presence","role":"a","kind":"appeared","n":2,"utc":"2026-11-05T02:12:02.141122Z"}
C: {"event":"action","role":"c","label":"end-clean","utc":"2026-11-05T02:12:06.195912Z"}
A: {"event":"presence","role":"a","kind":"loss","utc":"2026-11-05T02:12:06.198899Z"}
```

Loopback dry-run output, trimmed to the fields that matter; values differ per run.

   "C learns of the first two" is C's `presence` `appeared` before any frame (it sends and
   receives none): native liveliness history. Each of A and B prints `appeared` at C's
   start. At the end C prints `action` `end-clean` at the instant it calls shutdown; A prints
   `presence` `loss`. The assembler subtracts, correcting by the measured clock offset.
4. B, killed: same command, label `step8-c-abort-b`, `--end abort` (C prints `action
   end-abort`, then aborts the process: the OS closes its sockets, as for a killed process).
5. B, silent loss: label `step8-c-silent-b`, `--run-secs 120`, no `--end`. When `link` is
   up, the operator runs the drop on B and the mark in one command line (Operator commands
   below), so the action instant is the instant the drop returned. Observe A until a `loss`
   line or 25 s; then the operator removes the drop; then C ends on its own.

Pass (README step 8): late join seen; each loss reported on A; the silent loss at most
lease (10 s) plus 5 s after the action. Clean and killed are expected within a second or
two; the assembler also states whether each was under the lease. Record every value; where it
differs from the loopback measurement (clean/killed under 0.2 s; silent 9.5 to 9.7 s), record it
as a residual, not a new bound.

## Assemble (host A, after B's evidence is brought back)

The operator copies B's `DIR/evidence/*.jsonl` (no key material) into `DIR/b-side/evidence/`
on A. Then:

```sh
node tools/lan-leg/lan-leg.mjs assemble --dir "$DIR" --b-dir "$DIR/b-side"
```

```text
{"schema":"oac-lan-leg/1","event":"assembled","verdict":"NOT RUN","pass":15,"fail":0,"not_run":7}
PASS    3/3.explicit  starts ok=true; A accepted 6 frame(s) from B; B accepted 6 from A; scouting off/off
PASS    5/5.wrong-ca  start_ok=false; listener_reachable=true; native_refusal=true; setup_fault=false; ...
PASS    6/6.expiry  ontime accepted; late discard-expired (opened 1545 ms past its deadline, ...)
PASS    8/8.loss-clean  ... raw 3.847 ms, skew-corrected 4.115 ms (offset A-B -0.268 ms); within lease=true
PASS    8/8.loss-abort  ... raw 297.268 ms, skew-corrected 297.536 ms ...; within lease=true
NOT RUN 8/8.loss-silent  no action instant or no observer evidence
NOT RUN 7/7.relay-replace  needs the CLI daemon (not built at this commit)
```

That run was one machine, both roles on 127.0.0.1, clean tree. The verdict is `NOT RUN` because 7 criteria cannot be driven or were not run there; on real hosts steps 4 and the silent loss are added and the verdict is still `NOT RUN` at best (evidence.md).

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

Silent loss on B: drop both directions to A, and mark in the same command line.

```powershell
# Windows, elevated, on B
New-NetFirewallRule -DisplayName 'oac-lan-leg-drop-out' -Direction Outbound -RemoteAddress <A_IP> -Action Block ; New-NetFirewallRule -DisplayName 'oac-lan-leg-drop-in' -Direction Inbound -RemoteAddress <A_IP> -Action Block ; node tools/lan-leg/lan-leg.mjs mark --dir "$DIR" --label step8-marks-b --action silent-loss
```

```sh
# Linux, on B
sudo iptables -I OUTPUT -d <A_IP> -j DROP ; sudo iptables -I INPUT -s <A_IP> -j DROP ; node tools/lan-leg/lan-leg.mjs mark --dir "$DIR" --label step8-marks-b --action silent-loss
# macOS: unplug the cable (or switch the interface off) instead, running the mark command immediately after
```

Remove (always, also after a failure):

```powershell
Remove-NetFirewallRule -DisplayName 'oac-lan-leg-drop-out','oac-lan-leg-drop-in','oac-lan-leg-in','oac-lan-leg-mc' -ErrorAction SilentlyContinue
```

```sh
sudo iptables -D OUTPUT -d <A_IP> -j DROP ; sudo iptables -D INPUT -s <A_IP> -j DROP
sudo ufw delete allow from <B_IP> to any port <PA> proto tcp     # repeat for each rule added
```

Read-only verification after removal: `Get-NetFirewallRule -DisplayName 'oac-lan-leg*'`
(prints nothing) / `sudo iptables -S | grep <A_IP>` and `sudo ufw status`.

## Cleanup commands (Claude may run these on its own `DIR` after copying evidence out)

```powershell
Get-Process lan_leg -ErrorAction SilentlyContinue          # none should remain; stop only ones this session started
Remove-Item -Recurse -Force (Join-Path $DIR 'host-a'), (Join-Path $DIR 'for-host-b')
```

```sh
pgrep -x lan_leg            # none should remain
rm -rf "$DIR/host-a" "$DIR/for-host-b"
```
