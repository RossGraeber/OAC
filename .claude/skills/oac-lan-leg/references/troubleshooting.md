# Two-host LAN leg: troubleshooting

Diagnose by reading evidence lines, then print the fix for the operator. Claude never
changes a firewall, network, time-sync or system setting itself (SKILL.md, fail-closed
rules). Every fix below is something the operator runs. After any fix, re-run only the step
that failed; a step that ran under a broken condition is not evidence.

| Symptom (evidence line or output) | Likely cause | What to print for the operator |
|---|---|---|
| `start` has `"ok":false` on B, `error` mentions the configuration or a connect failure, after about 5 s | Host A's listener is not reachable: A not started yet, wrong `--connect`, firewall on A drops the port, or A bound a different address | On A: confirm the `start` line shows `ok:true` and the exact `listen`. From B: `Test-NetConnection <A_IP> -Port <PORT_A>` (PowerShell) or `nc -vz <A_IP> <PORT_A>` (bash). Windows: add an inbound allow rule for TCP `<PORT_A>` from `<B_IP>` only (see runbook "Firewall") |
| Same, and TCP connects but `start` still fails | Certificate name or chain: the address B dials is not an IP SAN of A's certificate, or the two bundles come from different `issue` runs | Compare `ip_a` in the `issue` line with the `--connect` address; they must match exactly. If the bundle on either host came from an earlier `issue`, delete both credential directories and re-issue and re-carry (the authority is not kept, so there is no partial re-issue) |
| `import` fails on Windows with an icacls message | Carried directory is on a volume without ACLs (FAT/exFAT medium) or owned by another user | Copy the directory to an NTFS location under the same user, then run `import` again. Never loosen the check |
| `import` ok but `start` says the certificate storage must be owner-only | The bundle was copied again after `import`, or a sync tool rewrote modes | Run `import` again after the final copy; do not place the directory in a synced folder |
| Windows prompt "allow access" for `lan_leg.exe` | First listener on a LAN address by a new executable (every rebuild is new) | Operator decides: allow on Private networks only, or cancel and add the explicit rule. Choosing Public/Domain-wide access is the operator's call, not Claude's |
| A listener is up, both hosts can ping, but B's connect times out only on Windows | Network profile is Public, which blocks inbound by default | `Get-NetConnectionProfile` shows `NetworkCategory`. The operator may set the interface Private or add a scoped rule; record the profile in the `host` line (the wrapper already does) |
| `link` shows `up:false` in step 4 (multicast) while step 3 passed | Multicast blocked: AP/client isolation, IGMP snooping without a querier, a VPN or virtual adapter chosen as the scouting interface (`interface: auto` picks one), or host firewall dropping UDP 7446 | Both hosts need inbound UDP `7446` and to receive group `224.0.0.224`. Disable VPN and virtual adapters (Hyper-V, WSL, Docker, VPN TAP) for the run, or use a wired segment. If multicast stays blocked, record 4.multicast as FAIL (network) and run 4.fallback (step 3 repeated) as the G3 fallback; never fall back to plaintext |
| `skew` has `"pass":false`, or `abs_skew_ms` above 5000 | Clock not synchronised | Stop. The clock condition fails and the leg must not be recorded as passed. Operator checks the OS time service (`w32tm /query /status` on Windows, `timedatectl` on Linux, `sntp`/System Settings on macOS), fixes it with the OS service (never an OAC mechanism), then re-run step 1 and everything after |
| `skew` samples have large `best_delay_us` (above a few hundred ms) | Congested or lossy link; offset estimate is coarse | Re-run the pings on a quieter link; the uncertainty of the offset is half the delay |
| Step 6: `ontime` shows `discard-expired` | Skew near the limit, or encrypted window too short for the link | Re-run with `--window-ms 4000 --hold-ms 6000` only if `skew` passed; record the skew next to the result. If skew is above 1 s, say so in the residuals |
| Step 6: `late` shows `accepted` | The receiver's clock is behind the sender's by more than the hold, or `--hold-ms` not longer than the window | Check `skew` and the flags. A slow receiver clock extends validity by the skew (README step 6); record it as the finding, do not tune until it passes |
| Step 5 wrong-san: B `start` ok | A is not serving with the `neg-wrong-san` credential, so B saw a valid certificate | Re-run A with `--cred neg-wrong-san`, then B again |
| Step 5 unlisted-cn: A shows `accepted` frames tagged `neg-probe` | The unlisted CN is on A's `--allow` list | Fix A's `--allow` (only `CN_B,CN_C`) and re-run. If it still leaks, that is a FAIL to report, not to work around |
| Step 8: no `loss` on A after the action | Packet drop incomplete (only one direction), or the rule matched the wrong address | Both directions must drop. `Get-NetFirewallRule -DisplayName oac-lan-leg-drop*` / `sudo iptables -S \| grep <A_IP>`. Record the observation window; do not extend a loss that never arrives |
| Step 8: `loss` on A later than lease plus 5 s | The step fails (README step 8) | Record the measured values as residuals and report FAIL. Do not retry until it passes; one corrected re-run is allowed only for a setup fault that the evidence shows (for example a one-direction drop) |
| Link stays up after the drop rule | VPN or a second route carries the traffic | `Get-NetRoute` / `ip route get <A_IP>` shows the interface; the drop must cover it. Disable the second route, repeat |
| Both hosts see each other but on different network segments through NAT | NAT or a VPN rewrites the address | The IP SAN must be the address the peer dials, so NAT breaks name verification. Use two addresses on one routed segment without NAT, or stop and report |

If the evidence does not explain the failure, stop and report the evidence lines and the
`host` records. A guess is not a diagnosis, and a workaround that the README step does not
describe is not a pass.
