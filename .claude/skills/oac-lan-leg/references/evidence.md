# Two-host LAN leg: evidence format, labels, posting

Evidence is JSON lines (schema `oac-lan-leg/1`). Every line has `event`, `role`, `utc`
(RFC 3339, microseconds) and `utc_us` (Unix microseconds, this host's wall clock). The
wrapper keeps them in `<DIR>/evidence/<label>.jsonl`, one file per step run, and appends a
`wrapper-exit` line with the driver's exit code. Evidence files hold no key material and
are the only files that leave a host. Never edit one; a wrong run is repeated under a new
label suffix (`-r2`) and the assembler is told which to use by leaving the bad one out.

## Events the driver prints

| `event` | Where | Fields that matter |
|---|---|---|
| `env` | every command | `os`, `os_version`, `arch`, `oac_commit.sha`, `oac_commit.tree_dirty`, `zenoh_version`, `link_lease_ms`, `skew_limit_ms` |
| `host` | wrapper `host` | `ipv4`, `utc_offset_minutes`, `firewall_policy_stated_by_operator`, on Windows `firewall_profiles` and `network_category` (read-only queries) |
| `issue` | A | `cn_a`, `cn_b`, `cn_c`, `ip_a`, `ip_b`, `authority_private_key_persisted:false`, `carry_to_host_b`, `allow_on_a`, `allow_on_b` |
| `import` | B | `hardened` (credential directories made owner-only) |
| `start` | peer | `ok`, `error`, `listen`, `connect`, `allow`, `scouting` (`off` or `multicast`), `partition` |
| `presence` | peer | `kind` (`appeared`, `loss`), `handle_index` (local index of the carrier handle; the transport reports one handle for the whole frame stream, so it shows sharing and does not attribute a loss), `since_start_ms`; stamped when the transport's handler runs |
| `link` | peer | `up`, `waited_ms`: whether a carrier appeared within `--link-timeout-secs` |
| `send` / `frame` | peer | `tag`, `seq`; on `frame`: `verdict` (`accepted`, `discard-expired`, `not-opened`), `native_utc_us` (arrival at the transport callback), `opened_utc_us`, `deadline_utc_us`, `late_by_ms` |
| `skew-sample` / `skew` | peer | `offset_us` = remote clock minus local clock, `delay_us`; `skew.abs_skew_ms` (fractional) from the minimum-delay sample, `samples`, `limit_ms`, `pass` (compared in exact microseconds; a batch with no sample has `pass:false`) |
| `probe` | peer with `--probe-peers` | `pongs_from_a`, `pongs_from_b`, `linked_to_both`: a late joiner shows it is linked to both earlier transports |
| `negative` | negative | `case`, `start_ok`, `error`, `listen`, `connect`, `linked`, `listener_reachable`, `native_open_failed`, `probes_sent`, `pass`, `counts_toward_readme`. The transport returns the same message ("the peer session did not open") for any failed native open: TLS refusal by the peer, a port in use, a `--listen` address the host lacks, a local TLS setup error. So a refusal counts only beside a passing `control` case (real credentials, same `--listen` and `--connect`, must link) with host A's port answering. `wrong-key` never counts: the API gives one file set to both the local listener and the connection, so the local native stack can refuse the mismatch before any peer is asked (zenoh-link-tls 1.10.1 `utils.rs:246`, `:410`; rustls behaviour not re-fetched here). `unlisted-cn` has `pass:null` and needs A's evidence |
| `action` | peer end, wrapper `mark` | `label` (`end-clean`, `end-abort`, `silent-loss-pre`, `silent-loss-post`), the instant of the action |
| `error` | any | `why`; the command then exits 2 |

## Labels (file names), per step and host

| Step | A label | B label |
|---|---|---|
| 1 | `host-a` | `host-b` |
| 2 | `step2-issue-a` | `step2-import-b` |
| 1 clock, 3 | `step3-a` | `step3-b` |
| 4 multicast | `step4-mc-a` | `step4-mc-b` |
| 4 fallback: multicast check (must not link), then step 3 again | `step4-fb-check-a`, `step4-fb-a` | `step4-fb-check-b`, `step4-fb-b` |
| 5 | `step5-a`, `step5-wrong-san-a` | `step5-control-b`, `step5-wrong-ca-b`, `step5-wrong-key-b`, `step5-unlisted-b`, `step5-wrong-san-b` |
| 6 | `step6-a` | `step6-b` |
| 8 | `step8-a` | `step8-b`, `step8-c-clean-b`, `step8-c-abort-b`, `step8-c-silent-b`, `step8-marks-b` |

`node tools/lan-leg/lan-leg.mjs assemble --dir <A DIR> --b-dir <B evidence copy>` reads those
names. It writes `lan-leg-summary.json` and `lan-leg-gate-result.md` next to `evidence/` and
prints one line per criterion: `PASS`, `FAIL` or `NOT RUN`. The verdict is `FAIL` if any
criterion failed, otherwise `NOT RUN` while any criterion was not run, otherwise `PASS`. At
this commit it is always `NOT RUN` at best: criteria `2.no-host-key-distribution`, `3.core-signed`,
`3.contract`, `4.tls-only`, `5.wrong-key`, `5.absent-client-cert`, `5.plaintext`, `6.core-discard` and
`7.relay-replace` cannot be shown (see SKILL.md). Do not turn that into a
pass; report it as the verdict with the list.

Cross-host timings (step 8): the action instant is on B's clock and the carrier-loss event
on A's. The assembler converts with the offset of the step-8 `skew` batch that B ran on its
own link to A (`step8-b`; A clock minus B clock) and prints both the raw and the
skew-corrected value with its error, half that batch's best `delay_us`. The bound is checked
on the corrected value, and a loss may precede the action only by that error. The 15 s bound
of `8.loss-clean` and `8.loss-abort` is the assembler's own (lease 10 s plus 5 s); the README
states a bound for the silent loss only. For the silent loss the operator runs `mark` before
and after the drop commands; the bound is checked from the earlier mark. Losses on A share
one carrier handle with every other transport, so the first loss after the action is taken
and the number of losses in the window is printed.

## Gate-result comment

`lan-leg-gate-result.md` is the gate-result template (`oac-gates`; extended form in
`docs/planning/gates/README.md`) with the criteria filled from the evidence. Fill the lines
marked `FILL:` (timebox, fallback, UNVERIFIED items, human actions) with facts from the
session, never with guesses; a field you cannot fill from the session is reported to the
operator, not invented. Add under the template:

- the two `env` lines (versions of both hosts) and the two `host` lines;
- the residuals the README names: holder gaps and fixed-port denial of service (step 7, not
  run), sender retention (risk row 77), and any step-8 value that differs from the loopback
  measurement (9.5 to 9.7 s), recorded as a residual and not as a new bound;
- the NOT RUN list with the reason for each.

Posting: ask the operator first, naming the issue numbers and showing the text. On a yes,
post the same comment on #64 (steps 1-7) and on #63 (step 8) with `gh issue comment`. Use
only `Refs #63` / `Refs #64` wording; never a closing keyword. Do not edit
`docs/planning/gates/G3-result.md` or `STATUS.md` and do not commit evidence files: promoting
the G3 two-host leg or Gate S4 is the lead's decision (`tests/integration/README.md`, last
paragraph of the leg), and committed fixtures need a manifest entry
(`docs/planning/gates/README.md`, "Fixture manifest").
